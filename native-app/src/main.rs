#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod core;
mod platform;

mod overlay_ui {
    include!(concat!(env!("OUT_DIR"), "/overlay.rs"));
}

use core::{
    annotation_render,
    annotations::{
        Annotation, AnnotationEditor, AnnotationTool, ImagePoint, StrokeColor, TextAlignment,
        TextAnnotationDraft, ViewportToImage,
    },
    frame::{CapturedFrame, PixelRect},
    geometry::{FrameViewport, LogicalViewport, ViewportToFrame},
    lifecycle::{
        export_notification, export_should_close_overlay, overlay_close_policy, AppAction,
        ClosePolicy, ExportKind, TrayCommand,
    },
    selection::{hit_test_selection, GestureKind, LogicalRect, SelectionGeometry, SelectionHit},
    session::CaptureSession,
};
use overlay_ui::OverlayPrototype;
use slint::private_unstable_api::re_exports::PointerEventKind;
use slint::winit_030::{winit, EventResult, WinitWindowAccessor};
use slint::{ComponentHandle, Image, Rgba8Pixel, SharedPixelBuffer};
use std::{
    sync::{
        atomic::{AtomicBool, AtomicIsize, Ordering},
        Arc, Mutex,
    },
    thread,
};

fn set_status(overlay: &OverlayPrototype, message: impl Into<slint::SharedString>) {
    let message = message.into();
    overlay.set_export_status(message.clone());
    log::info!("{message}");
}

fn finish_capture_session(session: &CaptureSession, reason: &str) {
    platform::trace_event(&format!("capture.session.finish reason={reason:?}"));
    session.finish();
}

fn close_capture_after_export(
    overlay: &OverlayPrototype,
    state: &SelectionState,
    reason: &str,
) -> Result<(), String> {
    platform::trace_event(&format!(
        "capture.export_success.close.begin reason={reason:?}"
    ));
    trace_overlay_state(overlay, "capture.export_success.close.before");
    overlay
        .hide()
        .map_err(|error| format!("Could not hide the selection overlay: {error}"))?;
    trace_overlay_state(overlay, "capture.export_success.close.hidden");
    overlay.window().set_fullscreen(false);
    platform::trace_slint_native_handle(
        "Overlay",
        "export_success.after_hide_and_fullscreen_exit",
        overlay.window(),
    );

    *state.frame.lock().expect("capture frame lock") = None;
    *state.crop.lock().expect("selected crop lock") = None;
    *state.bounds.lock().expect("selection bounds lock") = None;
    *state.geometry.lock().expect("selection geometry lock") = SelectionGeometry::default();
    *state.active_kind.lock().expect("active gesture lock") = None;
    *state.viewport.lock().expect("frame viewport lock") = None;
    *state.editor.lock().expect("annotation editor lock") = None;
    state.clear_text_interaction();
    overlay.set_annotation_mode(false);
    overlay.set_toolbar_tooltip_text("".into());
    overlay.set_color_palette_open(false);
    overlay.set_annotation_image(Image::default());
    overlay.set_text_entry_visible(false);
    overlay.set_text_entry_line_count(1);
    overlay.set_captured_image(Image::default());
    finish_capture_session(&state.session, reason);
    platform::trace_event(&format!(
        "capture.export_success.close.complete reason={reason:?}"
    ));
    Ok(())
}

fn trace_overlay_state(overlay: &OverlayPrototype, operation: &str) {
    platform::trace_component_state(
        "Overlay",
        operation,
        overlay.window().is_visible(),
        overlay.window().is_minimized(),
    );
}

fn close_request_response(policy: ClosePolicy) -> slint::CloseRequestResponse {
    match policy {
        ClosePolicy::KeepShown => slint::CloseRequestResponse::KeepWindowShown,
    }
}

fn annotation_snapshot(
    crop_slot: &Arc<Mutex<Option<CapturedFrame>>>,
    editor_slot: &Arc<Mutex<Option<AnnotationEditor>>>,
) -> Result<(CapturedFrame, Vec<Annotation>), String> {
    let crop = crop_slot
        .lock()
        .map_err(|_| "Selected crop lock is unavailable")?
        .clone()
        .ok_or("Selected crop is no longer available")?;
    let annotations = editor_slot
        .lock()
        .map_err(|_| "Annotation editor lock is unavailable")?
        .as_ref()
        .map(|editor| editor.document().annotations().to_vec())
        .ok_or("Annotation session is no longer available")?;
    Ok((crop, annotations))
}

fn start_export_operation(
    overlay: &OverlayPrototype,
    busy: Arc<AtomicBool>,
    status: &'static str,
    kind: ExportKind,
    notification_owner_hwnd: isize,
    operation: impl FnOnce() -> Result<String, String> + Send + 'static,
    on_success: impl FnOnce(&OverlayPrototype) -> Result<(), String> + Send + 'static,
) {
    platform::trace_event(&format!("export.requested status={status:?}"));
    if busy
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        overlay.set_export_status("An export operation is already running.".into());
        return;
    }
    overlay.set_export_busy(true);
    overlay.set_export_status(status.into());
    platform::trace_native_event(&format!("export.started status={status:?}"));
    trace_overlay_state(overlay, "export.started");
    let weak_overlay = overlay.as_weak();
    thread::spawn(move || {
        platform::trace_event(&format!("export.worker.begin status={status:?}"));
        let result = operation();
        platform::trace_event(&format!(
            "export.worker.end status={status:?} success={}",
            result.is_ok()
        ));
        if let Some(message) = export_notification(&result, kind) {
            if let Err(error) = platform::show_notification(message, notification_owner_hwnd) {
                log::warn!("Could not show export success notification: {error}");
            }
        }
        let busy_for_dispatch = busy.clone();
        let dispatched = slint::invoke_from_event_loop(move || {
            platform::trace_event(&format!("export.result_dispatch.entered status={status:?}"));
            let was_successful = export_should_close_overlay(&result);
            let mut message = match result {
                Ok(message) => message,
                Err(error) => format!("Export failed: {error}"),
            };
            if let Some(overlay) = weak_overlay.upgrade() {
                if was_successful {
                    if let Err(error) = on_success(&overlay) {
                        message = format!("{message}, but the overlay could not close: {error}");
                    }
                }
                overlay.set_export_busy(false);
                overlay.set_export_status(message.clone().into());
                trace_overlay_state(&overlay, "export.completed");
            }
            busy_for_dispatch.store(false, Ordering::Release);
        });
        if let Err(error) = dispatched {
            busy.store(false, Ordering::Release);
            log::error!("Could not return export result to the UI thread: {error}");
        }
    });
}

fn image_from_rgba(pixels: &[u8], width: u32, height: u32) -> Image {
    let pixels = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(pixels, width, height);
    Image::from_rgba8(pixels)
}

fn annotation_point(
    x: f64,
    y: f64,
    bounds: LogicalRect,
    crop: &CapturedFrame,
) -> Result<Option<ImagePoint>, &'static str> {
    ViewportToImage {
        viewport_width: bounds.width,
        viewport_height: bounds.height,
        image_width: crop.width(),
        image_height: crop.height(),
    }
    .map(x - bounds.x, y - bounds.y)
    .map(Some)
}

fn text_alignment_from_index(index: i32) -> TextAlignment {
    match index {
        1 => TextAlignment::Center,
        2 => TextAlignment::Right,
        _ => TextAlignment::Left,
    }
}

fn text_alignment_index(alignment: TextAlignment) -> i32 {
    match alignment {
        TextAlignment::Left => 0,
        TextAlignment::Center => 1,
        TextAlignment::Right => 2,
    }
}

fn current_frame_transform(
    viewport_slot: &Arc<Mutex<Option<FrameViewport>>>,
    viewport_width: f64,
    viewport_height: f64,
) -> Option<ViewportToFrame> {
    viewport_slot
        .lock()
        .expect("frame viewport lock")
        .as_ref()
        .copied()?
        .transform_for(viewport_width, viewport_height)
        .ok()
}

fn clear_selection_preview(overlay: &OverlayPrototype) {
    overlay.set_selection_x(0.0);
    overlay.set_selection_y(0.0);
    overlay.set_selection_width(0.0);
    overlay.set_selection_height(0.0);
}

#[derive(Clone)]
struct SelectionState {
    frame: Arc<Mutex<Option<CapturedFrame>>>,
    crop: Arc<Mutex<Option<CapturedFrame>>>,
    bounds: Arc<Mutex<Option<LogicalRect>>>,
    geometry: Arc<Mutex<SelectionGeometry>>,
    active_kind: Arc<Mutex<Option<GestureKind>>>,
    viewport: Arc<Mutex<Option<FrameViewport>>>,
    editor: Arc<Mutex<Option<AnnotationEditor>>>,
    text_position: Arc<Mutex<Option<ImagePoint>>>,
    text_awaiting_position: Arc<Mutex<bool>>,
    session: Arc<CaptureSession>,
    toolbar_snapshot: Arc<Mutex<Option<ToolbarPositionSnapshot>>>,
    toolbar_drag: Arc<Mutex<Option<ToolbarDrag>>>,
}

const TOOLBAR_DESIGN_WIDTH: f64 = 472.0;
const TOOLBAR_DESIGN_HEIGHT: f64 = 64.0;
const TOOLBAR_GAP: f64 = 8.0;
const TEXT_EDITOR_WIDTH: f64 = 360.0;
const TEXT_EDITOR_HEIGHT: f64 = 208.0;
const CROP_MOVE_BAND: f64 = 12.0;

#[derive(Clone, Copy, Debug, PartialEq)]
struct ToolbarLayout {
    x: f64,
    y: f64,
    scale: f64,
    forced_inside: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct ToolbarPositionSnapshot {
    manual: bool,
    x: f64,
    y: f64,
}

#[derive(Clone, Copy, Debug)]
struct ToolbarDrag {
    pointer_x: f64,
    pointer_y: f64,
    toolbar_x: f64,
    toolbar_y: f64,
    initial: ToolbarPositionSnapshot,
    moved: bool,
}

impl SelectionState {
    fn new() -> Self {
        Self {
            frame: Arc::new(Mutex::new(None)),
            crop: Arc::new(Mutex::new(None)),
            bounds: Arc::new(Mutex::new(None)),
            geometry: Arc::new(Mutex::new(SelectionGeometry::default())),
            active_kind: Arc::new(Mutex::new(None)),
            viewport: Arc::new(Mutex::new(None)),
            editor: Arc::new(Mutex::new(None)),
            text_position: Arc::new(Mutex::new(None)),
            text_awaiting_position: Arc::new(Mutex::new(false)),
            session: Arc::new(CaptureSession::default()),
            toolbar_snapshot: Arc::new(Mutex::new(None)),
            toolbar_drag: Arc::new(Mutex::new(None)),
        }
    }

    fn current_transform(&self, width: f64, height: f64) -> Option<ViewportToFrame> {
        current_frame_transform(&self.viewport, width, height)
    }

    fn clear_text_interaction(&self) {
        *self.text_position.lock().expect("text position lock") = None;
        *self
            .text_awaiting_position
            .lock()
            .expect("text reposition lock") = false;
    }
}

fn show_text_editor(
    overlay: &OverlayPrototype,
    draft: &TextAnnotationDraft,
    x: f64,
    y: f64,
    viewport_width: f64,
    viewport_height: f64,
) {
    overlay.set_text_entry_value(draft.text.clone().into());
    overlay.set_text_entry_line_count(text_editor_visual_line_count(&draft.text));
    overlay.set_annotation_size(draft.text_style.font_size.round().clamp(8.0, 256.0) as i32);
    overlay.set_annotation_color_index(draft.color.palette_index());
    overlay.set_color_palette_open(false);
    overlay.set_text_bold(draft.text_style.bold);
    overlay.set_text_italic(draft.text_style.italic);
    overlay.set_text_alignment(text_alignment_index(draft.text_style.alignment));
    overlay.set_text_entry_existing(draft.annotation_id.is_some());
    overlay.set_text_entry_awaiting_position(false);
    overlay.set_text_entry_message("".into());
    overlay.set_text_entry_x(
        x.max(0.0)
            .min((viewport_width - TEXT_EDITOR_WIDTH).max(0.0)) as f32,
    );
    overlay.set_text_entry_y(
        y.max(0.0)
            .min((viewport_height - TEXT_EDITOR_HEIGHT).max(0.0)) as f32,
    );
    overlay.set_text_entry_visible(true);
}

fn sync_annotation_controls(overlay: &OverlayPrototype, editor: &AnnotationEditor) {
    overlay.set_annotation_size(editor.active_size().round() as i32);
    overlay.set_annotation_color_index(editor.active_color().palette_index());
}

fn text_editor_visual_line_count(text: &str) -> i32 {
    let lines = text
        .split('\n')
        .map(|line| {
            line.chars()
                .filter(|character| *character != '\r')
                .count()
                .div_ceil(42)
                .max(1)
        })
        .sum::<usize>();
    lines.clamp(1, 6) as i32
}

fn selection_move_band_contains(selection: LogicalRect, x: f64, y: f64) -> bool {
    let right = selection.x + selection.width;
    let bottom = selection.y + selection.height;
    x >= selection.x
        && x <= right
        && y >= selection.y
        && y <= bottom
        && (x - selection.x <= CROP_MOVE_BAND
            || right - x <= CROP_MOVE_BAND
            || y - selection.y <= CROP_MOVE_BAND
            || bottom - y <= CROP_MOVE_BAND)
}

fn present_pixel_rect(
    overlay: &OverlayPrototype,
    rect: Option<PixelRect>,
    transform: ViewportToFrame,
) -> Result<Option<LogicalRect>, &'static str> {
    let viewport = LogicalViewport {
        width: transform.viewport_width,
        height: transform.viewport_height,
    };
    let Some(rect) = rect else {
        clear_selection_preview(overlay);
        apply_toolbar_layout(overlay, None, viewport);
        return Ok(None);
    };
    let logical = transform.logical_rect_for_pixels(rect)?;
    overlay.set_selection_x(logical.x as f32);
    overlay.set_selection_y(logical.y as f32);
    overlay.set_selection_width(logical.width as f32);
    overlay.set_selection_height(logical.height as f32);
    apply_toolbar_layout(overlay, Some(logical), viewport);
    Ok(Some(logical))
}

fn present_gesture_preview(
    overlay: &OverlayPrototype,
    state: &SelectionState,
    transform: ViewportToFrame,
) -> Result<(), &'static str> {
    let preview = state
        .geometry
        .lock()
        .expect("selection geometry lock")
        .preview();
    present_pixel_rect(overlay, preview, transform)?;
    Ok(())
}

fn toolbar_scale_for_viewport(viewport: LogicalViewport) -> f64 {
    if !viewport.width.is_finite()
        || !viewport.height.is_finite()
        || viewport.width <= 0.0
        || viewport.height <= 0.0
    {
        return 0.0;
    }
    (viewport.width / TOOLBAR_DESIGN_WIDTH)
        .min(viewport.height / TOOLBAR_DESIGN_HEIGHT)
        .min(1.0)
}

fn clamp_toolbar_position(x: f64, y: f64, viewport: LogicalViewport, scale: f64) -> (f64, f64) {
    let width = TOOLBAR_DESIGN_WIDTH * scale;
    let height = TOOLBAR_DESIGN_HEIGHT * scale;
    let x = if x.is_finite() { x } else { 0.0 };
    let y = if y.is_finite() { y } else { 0.0 };
    (
        x.clamp(0.0, (viewport.width - width).max(0.0)),
        y.clamp(0.0, (viewport.height - height).max(0.0)),
    )
}

fn rebase_toolbar_position(
    position: ToolbarPositionSnapshot,
    old_viewport: LogicalViewport,
    new_viewport: LogicalViewport,
) -> (f64, f64) {
    clamp_toolbar_position(
        position.x * old_viewport.width / new_viewport.width,
        position.y * old_viewport.height / new_viewport.height,
        new_viewport,
        toolbar_scale_for_viewport(new_viewport),
    )
}

fn calculate_toolbar_layout(
    selection: Option<LogicalRect>,
    viewport: LogicalViewport,
) -> ToolbarLayout {
    let scale = toolbar_scale_for_viewport(viewport);
    let width = TOOLBAR_DESIGN_WIDTH * scale;
    let height = TOOLBAR_DESIGN_HEIGHT * scale;
    let (Some(selection), true) = (selection, scale > 0.0) else {
        return ToolbarLayout {
            x: 0.0,
            y: 0.0,
            scale,
            forced_inside: false,
        };
    };
    let (x, _) = clamp_toolbar_position(selection.x, 0.0, viewport, scale);
    let below_y = selection.y + selection.height + TOOLBAR_GAP;
    if below_y + height <= viewport.height {
        return ToolbarLayout {
            x,
            y: below_y,
            scale,
            forced_inside: false,
        };
    }
    let above_y = selection.y - height - TOOLBAR_GAP;
    if above_y >= 0.0 {
        return ToolbarLayout {
            x,
            y: above_y,
            scale,
            forced_inside: false,
        };
    }
    let max_x = (viewport.width - width).max(0.0);
    let max_y = (viewport.height - height).max(0.0);
    let side_y = selection.y.clamp(0.0, max_y);
    let right_x = selection.x + selection.width + TOOLBAR_GAP;
    if right_x + width <= viewport.width {
        return ToolbarLayout {
            x: right_x,
            y: side_y,
            scale,
            forced_inside: false,
        };
    }
    let left_x = selection.x - width - TOOLBAR_GAP;
    if left_x >= 0.0 {
        return ToolbarLayout {
            x: left_x,
            y: side_y,
            scale,
            forced_inside: false,
        };
    }
    let x = if selection.width >= width {
        selection
            .x
            .clamp(0.0, (selection.x + selection.width - width).min(max_x))
    } else {
        selection.x.clamp(0.0, max_x)
    };
    let y = if selection.height >= height {
        selection
            .y
            .clamp(0.0, (selection.y + selection.height - height).min(max_y))
    } else {
        selection.y.clamp(0.0, max_y)
    };
    ToolbarLayout {
        x,
        y,
        scale,
        forced_inside: true,
    }
}

fn apply_toolbar_layout(
    overlay: &OverlayPrototype,
    selection: Option<LogicalRect>,
    viewport: LogicalViewport,
) {
    let layout = calculate_toolbar_layout(selection, viewport);
    overlay.set_toolbar_auto_x(layout.x as f32);
    overlay.set_toolbar_auto_y(layout.y as f32);
    overlay.set_toolbar_scale(layout.scale as f32);
    overlay.set_toolbar_forced_inside(layout.forced_inside);
}

fn toolbar_snapshot(overlay: &OverlayPrototype) -> ToolbarPositionSnapshot {
    ToolbarPositionSnapshot {
        manual: overlay.get_toolbar_manual_position(),
        x: f64::from(overlay.get_toolbar_manual_x()),
        y: f64::from(overlay.get_toolbar_manual_y()),
    }
}

fn restore_toolbar_snapshot(overlay: &OverlayPrototype, state: &SelectionState) {
    if let Some(snapshot) = state
        .toolbar_snapshot
        .lock()
        .expect("toolbar position snapshot lock")
        .take()
    {
        overlay.set_toolbar_manual_x(snapshot.x as f32);
        overlay.set_toolbar_manual_y(snapshot.y as f32);
        overlay.set_toolbar_manual_position(snapshot.manual);
    }
}

fn cancel_toolbar_drag(overlay: &OverlayPrototype, state: &SelectionState) {
    if let Some(active) = state.toolbar_drag.lock().expect("toolbar drag lock").take() {
        overlay.set_toolbar_manual_x(active.initial.x as f32);
        overlay.set_toolbar_manual_y(active.initial.y as f32);
        overlay.set_toolbar_manual_position(active.initial.manual);
    }
}

fn toolbar_contains(overlay: &OverlayPrototype, x: f64, y: f64) -> bool {
    if !overlay.get_annotation_mode() {
        return false;
    }
    let left = f64::from(overlay.get_toolbar_position_x());
    let top = f64::from(overlay.get_toolbar_position_y());
    let right = left + f64::from(overlay.get_toolbar_effective_width());
    let bottom = top + f64::from(overlay.get_toolbar_effective_height());
    x >= left && x <= right && y >= top && y <= bottom
}

fn handle_toolbar_pointer_event(
    overlay: &OverlayPrototype,
    state: &SelectionState,
    kind: PointerEventKind,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
) {
    let viewport = LogicalViewport { width, height };
    let scale = f64::from(overlay.get_toolbar_scale());
    match kind {
        PointerEventKind::Down => {
            if !overlay.get_toolbar_drag_enabled() {
                return;
            }
            let mut drag = state.toolbar_drag.lock().expect("toolbar drag lock");
            if drag.is_some() {
                return;
            }
            *drag = Some(ToolbarDrag {
                pointer_x: x,
                pointer_y: y,
                toolbar_x: f64::from(overlay.get_toolbar_position_x()),
                toolbar_y: f64::from(overlay.get_toolbar_position_y()),
                initial: toolbar_snapshot(overlay),
                moved: false,
            });
        }
        PointerEventKind::Move | PointerEventKind::Up => {
            let mut drag = state.toolbar_drag.lock().expect("toolbar drag lock");
            let Some(active) = drag.as_mut() else { return };
            let dx = x - active.pointer_x;
            let dy = y - active.pointer_y;
            if active.moved || dx.abs() >= 2.0 || dy.abs() >= 2.0 {
                let (x, y) = clamp_toolbar_position(
                    active.toolbar_x + dx,
                    active.toolbar_y + dy,
                    viewport,
                    scale,
                );
                overlay.set_toolbar_manual_x(x as f32);
                overlay.set_toolbar_manual_y(y as f32);
                overlay.set_toolbar_manual_position(true);
                active.moved = true;
            }
            if kind == PointerEventKind::Up {
                drag.take();
            }
        }
        PointerEventKind::Cancel => {
            if let Some(active) = state.toolbar_drag.lock().expect("toolbar drag lock").take() {
                overlay.set_toolbar_manual_x(active.initial.x as f32);
                overlay.set_toolbar_manual_y(active.initial.y as f32);
                overlay.set_toolbar_manual_position(active.initial.manual);
            }
        }
        _ => {}
    }
}

fn selection_has_pending_edits(state: &SelectionState) -> bool {
    state
        .editor
        .lock()
        .expect("annotation editor lock")
        .as_ref()
        .is_some_and(|editor| editor.document().len() > 0 || editor.is_drawing())
        || state
            .text_position
            .lock()
            .expect("text position lock")
            .is_some()
}

fn reset_to_new_selection(
    overlay: &OverlayPrototype,
    state: &SelectionState,
) -> Result<(), &'static str> {
    let viewport = state
        .viewport
        .lock()
        .expect("frame viewport lock")
        .as_ref()
        .copied()
        .ok_or("captured frame viewport is unavailable")?;
    let logical_size = viewport.logical_size();
    let transform = viewport.transform_for(logical_size.width, logical_size.height)?;
    let editor_was_present = state
        .editor
        .lock()
        .expect("annotation editor lock")
        .is_some();
    if editor_was_present && !state.session.restart_selection() {
        return Err("capture session could not return to selection mode");
    }
    if editor_was_present {
        state.editor.lock().expect("annotation editor lock").take();
    }
    *state.geometry.lock().expect("selection geometry lock") =
        SelectionGeometry::new(None, transform)?;
    *state.active_kind.lock().expect("active gesture lock") = None;
    *state.crop.lock().expect("selected crop lock") = None;
    *state.bounds.lock().expect("selection bounds lock") = None;
    state.clear_text_interaction();
    *state
        .toolbar_snapshot
        .lock()
        .expect("toolbar position snapshot lock") = None;
    cancel_toolbar_drag(overlay, state);
    overlay.set_toolbar_manual_position(false);
    overlay.set_annotation_mode(false);
    overlay.set_confirm_selection_action_visible(false);
    overlay.set_annotation_image(Image::default());
    overlay.set_text_entry_visible(false);
    overlay.set_text_entry_value(String::new().into());
    overlay.set_text_entry_line_count(1);
    overlay.set_color_palette_open(false);
    overlay.set_can_undo(false);
    overlay.set_can_redo(false);
    clear_selection_preview(overlay);
    overlay.invoke_focus_selection();
    Ok(())
}

fn begin_selection_gesture(
    overlay: &OverlayPrototype,
    state: &SelectionState,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
) -> Result<bool, &'static str> {
    if toolbar_contains(overlay, x, y)
        || state
            .toolbar_drag
            .lock()
            .expect("toolbar drag lock")
            .is_some()
        || state
            .text_position
            .lock()
            .expect("text position lock")
            .is_some()
    {
        return Ok(false);
    }
    let transform = state
        .current_transform(width, height)
        .ok_or("selection viewport is unavailable or out of sync")?;
    let committed = state
        .geometry
        .lock()
        .expect("selection geometry lock")
        .committed();
    let kind = if let Some(rect) = committed {
        match hit_test_selection(rect, x, y, 5.0, transform)? {
            SelectionHit::Outside => GestureKind::Create,
            SelectionHit::Move => GestureKind::Move,
            SelectionHit::Resize(handle) => GestureKind::Resize(handle),
        }
    } else {
        GestureKind::Create
    };
    if kind == GestureKind::Create && selection_has_pending_edits(state) {
        overlay.set_confirm_selection_action_message(
            "Creating a new selection discards the current annotations.".into(),
        );
        overlay.set_confirm_selection_action_visible(true);
        return Ok(false);
    }
    *state
        .toolbar_snapshot
        .lock()
        .expect("toolbar position snapshot lock") = Some(toolbar_snapshot(overlay));
    state
        .geometry
        .lock()
        .expect("selection geometry lock")
        .begin(kind, x, y, transform)?;
    *state.active_kind.lock().expect("active gesture lock") = Some(kind);
    // The committed annotation image belongs to the old crop. The frozen
    // capture beneath it tracks the live geometry until a new crop is committed.
    overlay.set_annotation_image(Image::default());
    platform::trace_event(&format!(
        "selection.gesture.begin kind={kind:?} x={x} y={y}"
    ));
    present_gesture_preview(overlay, state, transform)?;
    Ok(true)
}

fn update_selection_gesture(
    overlay: &OverlayPrototype,
    state: &SelectionState,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
) -> Result<bool, &'static str> {
    if !state
        .geometry
        .lock()
        .expect("selection geometry lock")
        .is_active()
    {
        return Ok(false);
    }
    let transform = state
        .current_transform(width, height)
        .ok_or("selection viewport is unavailable or out of sync")?;
    state
        .geometry
        .lock()
        .expect("selection geometry lock")
        .update(x, y, transform)?;
    let (committed, preview) = {
        let geometry = state.geometry.lock().expect("selection geometry lock");
        (geometry.committed(), geometry.preview())
    };
    if preview != committed {
        // A manual toolbar position belongs to the old selection geometry.
        // Follow the new geometry automatically while the gesture is active;
        // cancellation restores the snapshot captured at pointer-down.
        overlay.set_toolbar_manual_position(false);
    }
    present_gesture_preview(overlay, state, transform)?;
    Ok(true)
}

fn cancel_selection_gesture(
    overlay: &OverlayPrototype,
    state: &SelectionState,
    width: f64,
    height: f64,
) {
    let was_active = state
        .geometry
        .lock()
        .expect("selection geometry lock")
        .is_active();
    if !was_active {
        return;
    }
    state
        .geometry
        .lock()
        .expect("selection geometry lock")
        .cancel();
    *state.active_kind.lock().expect("active gesture lock") = None;
    if let Some(transform) = state.current_transform(width, height) {
        let committed = state
            .geometry
            .lock()
            .expect("selection geometry lock")
            .committed();
        if let Ok(bounds) = present_pixel_rect(overlay, committed, transform) {
            *state.bounds.lock().expect("selection bounds lock") = bounds;
        }
    }
    restore_toolbar_snapshot(overlay, state);
    cancel_toolbar_drag(overlay, state);
    update_preview_if_possible(overlay, &state.crop, &state.editor);
    platform::trace_event("selection.gesture.cancel restored_committed_geometry=true");
}

fn finish_selection_gesture(
    overlay: &OverlayPrototype,
    state: &SelectionState,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
) {
    let active = state
        .geometry
        .lock()
        .expect("selection geometry lock")
        .is_active();
    if !active {
        return;
    }
    let Some(transform) = state.current_transform(width, height) else {
        cancel_selection_gesture(overlay, state, width, height);
        return;
    };
    if let Err(error) = update_selection_gesture(overlay, state, x, y, width, height) {
        log::warn!("Could not update selection gesture: {error}");
        cancel_selection_gesture(overlay, state, width, height);
        return;
    }
    let (original, candidate) = {
        let geometry = state.geometry.lock().expect("selection geometry lock");
        (geometry.committed(), geometry.preview())
    };
    let kind = state
        .active_kind
        .lock()
        .expect("active gesture lock")
        .take();
    let Some(candidate) = candidate else {
        state
            .geometry
            .lock()
            .expect("selection geometry lock")
            .commit();
        if let Ok(bounds) = present_pixel_rect(overlay, original, transform) {
            *state.bounds.lock().expect("selection bounds lock") = bounds;
        }
        update_preview_if_possible(overlay, &state.crop, &state.editor);
        restore_toolbar_snapshot(overlay, state);
        platform::trace_event("selection.gesture.commit ignored_empty_preview=true");
        return;
    };
    if Some(candidate) == original {
        state
            .geometry
            .lock()
            .expect("selection geometry lock")
            .commit();
        if let Ok(bounds) = present_pixel_rect(overlay, Some(candidate), transform) {
            *state.bounds.lock().expect("selection bounds lock") = bounds;
        }
        update_preview_if_possible(overlay, &state.crop, &state.editor);
        restore_toolbar_snapshot(overlay, state);
        platform::trace_event("selection.gesture.commit changed=false");
        return;
    }
    let crop = state
        .frame
        .lock()
        .expect("capture frame lock")
        .as_ref()
        .and_then(|frame| frame.crop(candidate).ok());
    let Some(crop) = crop else {
        state
            .geometry
            .lock()
            .expect("selection geometry lock")
            .cancel();
        if let Ok(bounds) = present_pixel_rect(overlay, original, transform) {
            *state.bounds.lock().expect("selection bounds lock") = bounds;
        }
        restore_toolbar_snapshot(overlay, state);
        log::error!("Selected crop could not be copied from the captured frame");
        update_preview_if_possible(overlay, &state.crop, &state.editor);
        set_status(overlay, "Could not copy selected screenshot region.");
        return;
    };
    let committed = state
        .geometry
        .lock()
        .expect("selection geometry lock")
        .commit();
    if committed != Some(candidate) {
        log::error!("Selection geometry commit did not retain its preview rectangle");
        return;
    }
    let logical = match transform.logical_rect_for_pixels(candidate) {
        Ok(logical) => logical,
        Err(error) => {
            log::error!("Committed selection geometry is invalid: {error}");
            return;
        }
    };
    *state.bounds.lock().expect("selection bounds lock") = Some(logical);
    *state.crop.lock().expect("selected crop lock") = Some(crop);
    *state
        .toolbar_snapshot
        .lock()
        .expect("toolbar position snapshot lock") = None;
    cancel_toolbar_drag(overlay, state);
    overlay.set_toolbar_manual_position(false);
    apply_toolbar_layout(
        overlay,
        Some(logical),
        LogicalViewport {
            width: transform.viewport_width,
            height: transform.viewport_height,
        },
    );
    let kind = kind.unwrap_or(GestureKind::Create);
    if kind == GestureKind::Create {
        *state.editor.lock().expect("annotation editor lock") = Some(AnnotationEditor::default());
        state.clear_text_interaction();
        overlay.set_annotation_size(3);
        overlay.set_annotation_color_index(0);
        overlay.set_color_palette_open(false);
        overlay.set_text_entry_visible(false);
        overlay.set_text_entry_value(String::new().into());
        overlay.set_text_entry_line_count(1);
        overlay.set_text_entry_existing(false);
        overlay.set_text_entry_awaiting_position(false);
        overlay.set_annotation_image(Image::default());
        if !state.session.is_active() {
            log::warn!("Selection was committed after its capture session ended");
        } else if state
            .editor
            .lock()
            .expect("annotation editor lock")
            .is_some()
            && original.is_none()
            && !state.session.begin_annotation()
        {
            log::error!("Could not transition capture session into annotation state");
        }
    }
    overlay.set_selection_x(logical.x as f32);
    overlay.set_selection_y(logical.y as f32);
    overlay.set_selection_width(logical.width as f32);
    overlay.set_selection_height(logical.height as f32);
    let selected_tool_index = state
        .editor
        .lock()
        .expect("annotation editor lock")
        .as_ref()
        .map(|editor| editor.active_tool().ui_index())
        .unwrap_or_else(|| AnnotationTool::Pen.ui_index());
    overlay.set_selected_tool(selected_tool_index);
    overlay.set_annotation_mode(true);
    overlay.set_confirm_selection_action_visible(false);
    platform::trace_event(&format!(
        "selection.gesture.commit kind={kind:?} pixels=({}, {}, {}, {})",
        candidate.x, candidate.y, candidate.width, candidate.height
    ));
    if let Err(error) = refresh_annotation_preview(overlay, &state.crop, &state.editor) {
        log::warn!("Could not refresh annotation preview after selection update: {error}");
    }
    overlay.invoke_focus_selection();
}

fn install_overlay_scale_factor_handler(overlay: &OverlayPrototype, state: SelectionState) {
    let weak_overlay = overlay.as_weak();
    overlay.window().on_winit_window_event(move |_, event| {
        let winit::event::WindowEvent::ScaleFactorChanged { scale_factor, .. } = event else {
            return EventResult::Propagate;
        };

        // Slint stores its scale factor as f32, so derive the viewport from
        // the same effective value before Slint lays out the new event.
        let effective_scale_factor = f64::from(*scale_factor as f32);
        platform::trace_event(&format!(
            "overlay.scale_factor_changed.begin scale_factor={effective_scale_factor}"
        ));
        let Some(overlay) = weak_overlay.upgrade() else {
            return EventResult::Propagate;
        };
        state
            .geometry
            .lock()
            .expect("selection geometry lock")
            .cancel();
        *state.active_kind.lock().expect("active gesture lock") = None;
        restore_toolbar_snapshot(&overlay, &state);
        cancel_toolbar_drag(&overlay, &state);
        let toolbar_before_scale = toolbar_snapshot(&overlay);

        let synchronized = {
            let mut viewport_guard = state.viewport.lock().expect("frame viewport lock");
            let Some(viewport) = viewport_guard.as_mut() else {
                platform::trace_event("overlay.scale_factor_changed.ignored no_captured_frame");
                return EventResult::Propagate;
            };
            let old_logical_size = viewport.logical_size();
            match viewport.synchronize_scale_factor(effective_scale_factor) {
                Ok(logical_size) => Ok((*viewport, old_logical_size, logical_size)),
                Err(error) => {
                    *viewport_guard = None;
                    Err(error)
                }
            }
        };

        let (viewport, old_logical_size, logical_size) = match synchronized {
            Ok(viewport) => viewport,
            Err(error) => {
                log::warn!("Rejected Overlay scale-factor change: {error}");
                platform::trace_event(&format!(
                    "overlay.scale_factor_changed.invalid error={error}"
                ));
                return EventResult::Propagate;
            }
        };

        let selected_pixels = state
            .geometry
            .lock()
            .expect("selection geometry lock")
            .committed();
        let logical_selection = selected_pixels
            .map(|pixels| viewport.project_pixels(pixels))
            .transpose();
        let logical_selection = match logical_selection {
            Ok(selection) => selection,
            Err(error) => {
                *state.viewport.lock().expect("frame viewport lock") = None;
                *state.bounds.lock().expect("selection bounds lock") = None;
                log::warn!("Rejected Overlay selection geometry after DPI change: {error}");
                platform::trace_event(&format!(
                    "overlay.scale_factor_changed.selection_invalid error={error}"
                ));
                return EventResult::Propagate;
            }
        };
        *state.bounds.lock().expect("selection bounds lock") = logical_selection;

        overlay.set_frame_viewport_width(logical_size.width as f32);
        overlay.set_frame_viewport_height(logical_size.height as f32);
        if toolbar_before_scale.manual {
            let (x, y) =
                rebase_toolbar_position(toolbar_before_scale, old_logical_size, logical_size);
            overlay.set_toolbar_manual_x(x as f32);
            overlay.set_toolbar_manual_y(y as f32);
        }
        if let Some(rect) = logical_selection {
            overlay.set_selection_x(rect.x as f32);
            overlay.set_selection_y(rect.y as f32);
            overlay.set_selection_width(rect.width as f32);
            overlay.set_selection_height(rect.height as f32);
        } else {
            clear_selection_preview(&overlay);
        }
        apply_toolbar_layout(&overlay, logical_selection, logical_size);

        if let (Some(point), Some(bounds), Some(crop)) = (
            *state.text_position.lock().expect("text position lock"),
            logical_selection,
            state
                .crop
                .lock()
                .expect("selected crop lock")
                .as_ref()
                .cloned(),
        ) {
            if crop.width() > 0 && crop.height() > 0 {
                let x = bounds.x + point.x * bounds.width / f64::from(crop.width()) + 8.0;
                let y = bounds.y + point.y * bounds.height / f64::from(crop.height()) + 8.0;
                overlay.set_text_entry_x(
                    x.max(0.0)
                        .min((logical_size.width - TEXT_EDITOR_WIDTH).max(0.0))
                        as f32,
                );
                overlay.set_text_entry_y(
                    y.min((logical_size.height - TEXT_EDITOR_HEIGHT).max(0.0))
                        .max(0.0) as f32,
                );
            }
        }

        platform::trace_event(&format!(
            "overlay.scale_factor_changed.synchronized scale_factor={} frame={}x{} viewport={}x{}",
            viewport.scale_factor(),
            viewport.frame_width(),
            viewport.frame_height(),
            logical_size.width,
            logical_size.height
        ));
        EventResult::Propagate
    });
}

fn refresh_annotation_preview(
    overlay: &OverlayPrototype,
    crop_slot: &Arc<Mutex<Option<CapturedFrame>>>,
    editor_slot: &Arc<Mutex<Option<AnnotationEditor>>>,
) -> Result<(), &'static str> {
    let crop_guard = crop_slot.lock().expect("selected crop lock");
    let Some(crop) = crop_guard.as_ref() else {
        return Err("selected crop is no longer available");
    };
    let (annotations, can_undo, can_redo, can_delete_last) = {
        let guard = editor_slot.lock().expect("annotation editor lock");
        let Some(editor) = guard.as_ref() else {
            return Err("annotation editor is no longer available");
        };
        (
            editor.annotations_for_render(editor.active_stroke_style()),
            editor.can_undo(),
            editor.can_redo(),
            editor.can_delete_last(),
        )
    };
    let rendered = annotation_render::render_composite(crop, &annotations)?;
    let output = image_from_rgba(rendered.pixels(), rendered.width(), rendered.height());
    overlay.set_annotation_image(output);
    overlay.set_can_undo(can_undo);
    overlay.set_can_redo(can_redo);
    overlay.set_can_delete_last_annotation(can_delete_last);
    Ok(())
}

fn update_preview_if_possible(
    overlay: &OverlayPrototype,
    crop_slot: &Arc<Mutex<Option<CapturedFrame>>>,
    editor_slot: &Arc<Mutex<Option<AnnotationEditor>>>,
) {
    if crop_slot.lock().expect("selected crop lock").is_none()
        || editor_slot
            .lock()
            .expect("annotation editor lock")
            .is_none()
    {
        return;
    }
    if let Err(error) = refresh_annotation_preview(overlay, crop_slot, editor_slot) {
        log::warn!("Could not render annotation preview: {error}");
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    platform::initialize().map_err(std::io::Error::other)?;
    #[cfg(target_os = "windows")]
    let _window_diagnostics = match platform::window_diagnostics::WindowDiagnostics::start() {
        Ok(diagnostics) => diagnostics,
        Err(error) => {
            log::warn!("Could not start opt-in window diagnostics: {error}");
            None
        }
    };
    platform::trace_event("main.entry dpi_initialized=true");
    #[cfg(target_os = "windows")]
    {
        let backend_result = slint::BackendSelector::new()
            .backend_name("winit".into())
            .renderer_name("software".into())
            .select();
        platform::trace_event(&format!(
            "backend.select.after success={}",
            backend_result.is_ok()
        ));
        backend_result?;
    }
    log::info!("Carambola native capture app started");

    platform::trace_event("main.before_overlay_create");
    let overlay = OverlayPrototype::new().map_err(|error| {
        platform::trace_event(&format!("main.overlay_create.failed error={error}"));
        error
    })?;
    platform::trace_event("main.after_overlay_create");
    platform::trace_slint_native_handle("Overlay", "after_component_create", overlay.window());
    trace_overlay_state(&overlay, "main.after_overlay_create");
    platform::diagnose_cursor("overlay component created");

    let selection = SelectionState::new();
    let export_busy = Arc::new(AtomicBool::new(false));
    let clipboard_owner_hwnd = Arc::new(AtomicIsize::new(0));
    install_overlay_scale_factor_handler(&overlay, selection.clone());

    let weak_overlay = overlay.as_weak();
    let state_for_capture = selection.clone();
    let busy_for_capture = export_busy.clone();
    overlay.on_request_capture(move || {
        platform::clear_window_action("new_capture_requested");
        platform::trace_event("capture.start.requested");
        platform::diagnose_cursor("capture requested");
        if busy_for_capture.load(Ordering::Acquire) {
            if let Some(overlay) = weak_overlay.upgrade() {
                set_status(&overlay, "Please wait for the current export to finish.");
            }
            return;
        }
        if !state_for_capture.session.begin_capture() {
            platform::trace_event("capture.start.rejected_session_already_active");
            return;
        }
        platform::trace_event("capture.session.transition=Capturing");
        *state_for_capture.frame.lock().expect("capture frame lock") = None;
        *state_for_capture.crop.lock().expect("selected crop lock") = None;
        *state_for_capture.bounds.lock().expect("selection bounds lock") = None;
        *state_for_capture.geometry.lock().expect("selection geometry lock") = SelectionGeometry::default();
        *state_for_capture.active_kind.lock().expect("active gesture lock") = None;
        *state_for_capture.viewport.lock().expect("frame viewport lock") = None;
        *state_for_capture.editor.lock().expect("annotation editor lock") = None;
        state_for_capture.clear_text_interaction();

        let Some(overlay) = weak_overlay.upgrade() else {
            finish_capture_session(&state_for_capture.session, "overlay_unavailable_before_capture");
            return;
        };
        overlay.set_annotation_mode(false);
        overlay.set_toolbar_tooltip_text("".into());
        overlay.set_color_palette_open(false);
        overlay.set_annotation_color_index(0);
        overlay.set_annotation_size(3);
        overlay.set_confirm_selection_action_visible(false);
        overlay.set_annotation_image(Image::default());
        overlay.set_text_entry_visible(false);
        overlay.set_text_entry_value(String::new().into());
        overlay.set_text_entry_line_count(1);
        overlay.set_can_undo(false);
        overlay.set_can_redo(false);
        overlay.set_export_busy(false);
        overlay.set_export_status("Capturing desktop…".into());
        clear_selection_preview(&overlay);

        let requested_display_id = match platform::display_at_cursor_id() {
            Ok(display_id) => display_id,
            Err(error) => {
                set_status(&overlay, format!("Could not identify the display under the cursor: {error}"));
                finish_capture_session(&state_for_capture.session, "cursor_display_query_failed");
                return;
            }
        };

        platform::trace_event("capture.start.prepare_capture.begin");
        if let Err(error) = platform::prepare_capture() {
            set_status(&overlay, format!("Could not prepare the desktop for capture: {error}"));
            finish_capture_session(&state_for_capture.session, "prepare_capture_failed");
            return;
        }
        platform::trace_event("capture.start.prepare_capture.complete");
        platform::diagnose_cursor("capture prepared");

        let weak_overlay = overlay.as_weak();
        let state_for_result = state_for_capture.clone();
        thread::spawn(move || {
            platform::trace_event(&format!(
                "capture.capture_display.worker_begin display={requested_display_id:?}"
            ));
            let captured = platform::capture_display(requested_display_id.as_deref());
            platform::trace_event(&format!(
                "capture.capture_display.worker_end display={requested_display_id:?} success={}",
                captured.is_ok()
            ));
            let session_for_dispatch = state_for_result.session.clone();
            let session_for_queue_error = state_for_result.session.clone();
            let dispatched = slint::invoke_from_event_loop(move || {
                platform::trace_event("capture.capture_result.dispatch_entered");
                let Some(overlay) = weak_overlay.upgrade() else {
                    finish_capture_session(&session_for_dispatch, "overlay_dropped_during_capture");
                    return;
                };
                let frame = match captured {
                    Ok(frame) => frame,
                    Err(error) => {
                        set_status(&overlay, format!("Screen capture failed: {error}"));
                        finish_capture_session(&state_for_result.session, "screen_capture_failed");
                        return;
                    }
                };

                if let Some(display) = frame.display_metadata().cloned() {
                    let current_displays = match platform::enumerate_displays() {
                        Ok(displays) => displays,
                        Err(error) => {
                            set_status(&overlay, format!("Could not verify the captured display: {error}"));
                            finish_capture_session(&state_for_result.session, "display_verification_failed");
                            return;
                        }
                    };
                    if !current_displays.iter().any(|current| current == &display) {
                        set_status(&overlay, format!("Display {} changed or disconnected before the overlay opened.", display.id));
                        finish_capture_session(&state_for_result.session, "display_changed_before_overlay");
                        return;
                    }
                    let Some((origin, bottom_right)) = frame.desktop_origin()
                        .zip(frame.local_to_desktop(frame.width(), frame.height()))
                    else {
                        set_status(&overlay, "The captured frame has invalid desktop bounds.");
                        finish_capture_session(&state_for_result.session, "frame_desktop_bounds_invalid");
                        return;
                    };
                    if bottom_right.x != display.bounds.right || bottom_right.y != display.bounds.bottom {
                        set_status(&overlay, "The captured frame dimensions do not match the selected display.");
                        finish_capture_session(&state_for_result.session, "frame_display_bounds_mismatch");
                        return;
                    }
                    platform::trace_event(&format!(
                        "capture.frame_desktop_geometry display={:?} origin=({}, {}) end=({}, {})",
                        display.id, origin.x, origin.y, bottom_right.x, bottom_right.y
                    ));
                    // Place and size the hidden HWND on the selected monitor
                    // before borderless fullscreen. Winit resolves
                    // Borderless(None) from MonitorFromWindow(hwnd).
                    overlay.window().set_position(slint::PhysicalPosition::new(origin.x, origin.y));
                }

                let scale = frame.display_metadata().map(|display| display.scale_factor())
                    .unwrap_or_else(|| f64::from(overlay.window().scale_factor()));
                if !scale.is_finite() || scale <= 0.0 {
                    set_status(&overlay, "The captured display reported an invalid scale factor.");
                    finish_capture_session(&state_for_result.session, "invalid_display_scale");
                    return;
                }
                let viewport = match FrameViewport::new(frame.width(), frame.height(), f64::from(scale as f32)) {
                    Ok(viewport) => viewport,
                    Err(error) => {
                        set_status(&overlay, format!("The captured frame has invalid Overlay geometry: {error}"));
                        finish_capture_session(&state_for_result.session, "invalid_frame_viewport");
                        return;
                    }
                };
                let logical_size = viewport.logical_size();
                let transform = match viewport.transform_for(logical_size.width, logical_size.height) {
                    Ok(transform) => transform,
                    Err(error) => {
                        set_status(&overlay, format!("The Overlay viewport is invalid: {error}"));
                        finish_capture_session(&state_for_result.session, "invalid_initial_transform");
                        return;
                    }
                };
                platform::trace_event(&format!(
                    "capture.geometry frame={}x{} viewport={}x{} scale_factor={}",
                    frame.width(), frame.height(), logical_size.width, logical_size.height, viewport.scale_factor()
                ));
                overlay.window().set_size(slint::PhysicalSize::new(frame.width(), frame.height()));
                *state_for_result.viewport.lock().expect("frame viewport lock") = Some(viewport);
                *state_for_result.geometry.lock().expect("selection geometry lock") =
                    match SelectionGeometry::new(None, transform) {
                        Ok(geometry) => geometry,
                        Err(error) => {
                            set_status(&overlay, format!("Could not initialize selection geometry: {error}"));
                            finish_capture_session(&state_for_result.session, "selection_geometry_initialization_failed");
                            return;
                        }
                    };
                overlay.set_frame_viewport_width(logical_size.width as f32);
                overlay.set_frame_viewport_height(logical_size.height as f32);
                overlay.set_captured_image(image_from_rgba(frame.pixels(), frame.width(), frame.height()));
                overlay.set_annotation_mode(false);
                overlay.set_annotation_image(Image::default());
                clear_selection_preview(&overlay);
                *state_for_result.frame.lock().expect("capture frame lock") = Some(frame);

                #[cfg(target_os = "windows")]
                let taskbar_window_pending = !platform::taskbar::ensure_overlay_removed(
                    overlay.window(), "capture.before_fullscreen_and_show", false, false,
                );
                platform::trace_event("overlay.set_fullscreen_true.before");
                trace_overlay_state(&overlay, "overlay.set_fullscreen_true.before");
                overlay.window().set_fullscreen(true);
                trace_overlay_state(&overlay, "overlay.set_fullscreen_true.after");
                platform::trace_event("overlay.show.before");
                if let Err(error) = overlay.show() {
                    set_status(&overlay, format!("Could not show selection overlay: {error}"));
                    overlay.window().set_fullscreen(false);
                    overlay.set_captured_image(Image::default());
                    finish_capture_session(&state_for_result.session, "overlay_show_failed");
                    return;
                }
                trace_overlay_state(&overlay, "overlay.show.after");
                platform::trace_slint_native_handle("Overlay", "after_show", overlay.window());

                #[cfg(target_os = "windows")]
                if taskbar_window_pending {
                    let weak_overlay_retry = overlay.as_weak();
                    if let Err(error) = slint::spawn_local(async move {
                        let Some(overlay) = weak_overlay_retry.upgrade() else { return };
                        let native_window = match overlay.window().winit_window().await {
                            Ok(window) => window,
                            Err(error) => {
                                log::warn!("Overlay HWND unavailable for deferred taskbar removal: {error}");
                                return;
                            }
                        };
                        drop(native_window);
                        if overlay.window().is_visible() {
                            platform::taskbar::ensure_overlay_removed(
                                overlay.window(), "capture.deferred_after_show", true, true,
                            );
                        }
                    }) {
                        log::warn!("Could not wait for Overlay HWND to remove its taskbar tab: {error}");
                    }
                }
                platform::diagnose_cursor("overlay shown");
                if !state_for_result.session.capture_ready() {
                    set_status(&overlay, "Capture session state changed before selection.");
                    overlay.window().set_fullscreen(false);
                    let _ = overlay.hide();
                    overlay.set_captured_image(Image::default());
                    finish_capture_session(&state_for_result.session, "capture_not_ready_after_overlay_show");
                    return;
                }
                platform::trace_event("capture.session.transition=Selecting");
                if let Err(error) = platform::activate_overlay() {
                    log::warn!("Could not activate the native selection overlay: {error}");
                }
                overlay.invoke_focus_selection();
                overlay.set_export_status("Drag across the screenshot to select a region.".into());
                platform::diagnose_cursor("overlay activated");
            });
            if let Err(error) = dispatched {
                log::error!("Could not return capture result to the UI thread: {error}");
                finish_capture_session(&session_for_queue_error, "capture_result_dispatch_failed");
            }
        });
    });
    let weak_overlay = overlay.as_weak();
    let state_for_cancel = selection.clone();
    overlay.on_cancel_selection(move || {
        let action_id = platform::begin_window_action("Escape");
        platform::trace_event("capture.cancel_selection.requested");
        if let Some(overlay) = weak_overlay.upgrade() {
            trace_overlay_state(&overlay, "capture.cancel_selection.requested");
        }
        platform::diagnose_cursor("escape before close");
        if !state_for_cancel.session.cancel() {
            platform::trace_event("capture.cancel_selection.rejected_no_active_session");
            platform::finish_window_action(action_id, "rejected_no_active_session");
            return;
        }
        *state_for_cancel.frame.lock().expect("capture frame lock") = None;
        *state_for_cancel.crop.lock().expect("selected crop lock") = None;
        *state_for_cancel
            .bounds
            .lock()
            .expect("selection bounds lock") = None;
        *state_for_cancel
            .geometry
            .lock()
            .expect("selection geometry lock") = SelectionGeometry::default();
        *state_for_cancel
            .active_kind
            .lock()
            .expect("active gesture lock") = None;
        *state_for_cancel
            .viewport
            .lock()
            .expect("frame viewport lock") = None;
        *state_for_cancel
            .editor
            .lock()
            .expect("annotation editor lock") = None;
        state_for_cancel.clear_text_interaction();
        if let Some(overlay) = weak_overlay.upgrade() {
            overlay.set_annotation_mode(false);
            overlay.set_annotation_image(Image::default());
            overlay.set_text_entry_visible(false);
            overlay.set_confirm_selection_action_visible(false);
            platform::trace_event("overlay.hide.cancel.before");
            trace_overlay_state(&overlay, "overlay.hide.cancel.before");
            let result = overlay.hide();
            platform::trace_event(&format!(
                "overlay.hide.cancel.result success={}",
                result.is_ok()
            ));
            trace_overlay_state(&overlay, "overlay.hide.cancel.after");
            overlay.window().set_fullscreen(false);
            platform::trace_slint_native_handle(
                "Overlay",
                "cancel.after_hide_and_fullscreen_exit",
                overlay.window(),
            );
            overlay.set_captured_image(Image::default());
            platform::diagnose_cursor("escape after close");
        }
        platform::finish_window_action(action_id, "cancel_teardown_complete");
    });

    let weak_overlay = overlay.as_weak();
    let state_for_down = selection.clone();
    overlay.on_pointer_down(move |x, y, width, height| {
        let Some(overlay) = weak_overlay.upgrade() else {
            return;
        };
        if !state_for_down.session.is_active()
            || state_for_down
                .geometry
                .lock()
                .expect("selection geometry lock")
                .is_active()
        {
            return;
        }
        if overlay.get_annotation_mode() {
            let transform = match state_for_down.current_transform(width as f64, height as f64) {
                Some(transform) => transform,
                None => return,
            };
            if toolbar_contains(&overlay, x as f64, y as f64)
                || state_for_down
                    .toolbar_drag
                    .lock()
                    .expect("toolbar drag lock")
                    .is_some()
            {
                return;
            }

            let committed = state_for_down
                .geometry
                .lock()
                .expect("selection geometry lock")
                .committed();
            if let Some(rect) = committed {
                let logical_bounds = *state_for_down.bounds.lock().expect("selection bounds lock");
                let hit = match hit_test_selection(rect, x as f64, y as f64, 5.0, transform) {
                    Ok(hit) => hit,
                    Err(error) => {
                        log::warn!("Could not hit-test crop selection: {error}");
                        return;
                    }
                };
                let starts_crop_gesture = match hit {
                    SelectionHit::Outside | SelectionHit::Resize(_) => true,
                    SelectionHit::Move => logical_bounds.is_some_and(|bounds| {
                        selection_move_band_contains(bounds, x as f64, y as f64)
                    }),
                };
                if starts_crop_gesture {
                    if let Err(error) = begin_selection_gesture(
                        &overlay,
                        &state_for_down,
                        x as f64,
                        y as f64,
                        width as f64,
                        height as f64,
                    ) {
                        log::warn!("Could not begin crop selection gesture: {error}");
                    }
                    return;
                }
            }
            let bounds = *state_for_down.bounds.lock().expect("selection bounds lock");
            let crop = state_for_down
                .crop
                .lock()
                .expect("selected crop lock")
                .clone();
            let (Some(bounds), Some(crop)) = (bounds, crop) else {
                return;
            };
            let Ok(Some(point)) = annotation_point(x as f64, y as f64, bounds, &crop) else {
                return;
            };
            let awaiting_position = *state_for_down
                .text_awaiting_position
                .lock()
                .expect("text reposition lock");
            if awaiting_position {
                let text_draft = {
                    let mut guard = state_for_down
                        .editor
                        .lock()
                        .expect("annotation editor lock");
                    let Some(editor) = guard.as_mut() else {
                        return;
                    };
                    if let Err(error) = editor.move_text_draft(point) {
                        log::warn!("Could not move text annotation: {error}");
                        return;
                    }
                    let Some(draft) = editor.text_draft().cloned() else {
                        return;
                    };
                    draft
                };
                *state_for_down
                    .text_position
                    .lock()
                    .expect("text position lock") = Some(point);
                *state_for_down
                    .text_awaiting_position
                    .lock()
                    .expect("text reposition lock") = false;
                overlay.set_text_entry_awaiting_position(false);
                overlay.set_text_entry_message("Text position updated.".into());
                show_text_editor(
                    &overlay,
                    &text_draft,
                    x as f64 + 8.0,
                    y as f64 + 8.0,
                    width as f64,
                    height as f64,
                );
                if let Err(error) = refresh_annotation_preview(
                    &overlay,
                    &state_for_down.crop,
                    &state_for_down.editor,
                ) {
                    log::warn!("Could not render text position preview: {error}");
                }
                return;
            }
            if state_for_down
                .text_position
                .lock()
                .expect("text position lock")
                .is_some()
            {
                return;
            }
            let text_tool = state_for_down
                .editor
                .lock()
                .expect("annotation editor lock")
                .as_ref()
                .is_some_and(|editor| editor.active_tool() == AnnotationTool::Text);
            if text_tool {
                let text_draft = {
                    let mut guard = state_for_down
                        .editor
                        .lock()
                        .expect("annotation editor lock");
                    let Some(editor) = guard.as_mut() else { return };
                    if let Some(id) = editor.text_annotation_at(point) {
                        if let Err(error) = editor.begin_text_edit(id) {
                            log::warn!("Could not open text annotation for editing: {error}");
                            return;
                        }
                    } else {
                        editor.begin_text_annotation(point);
                    }
                    let Some(draft) = editor.text_draft().cloned() else {
                        return;
                    };
                    editor.set_active_text_style(draft.text_style);
                    editor.set_active_color(draft.color);
                    Some(draft)
                };
                let Some(text_draft) = text_draft else {
                    return;
                };
                *state_for_down
                    .text_position
                    .lock()
                    .expect("text position lock") = Some(text_draft.position);
                show_text_editor(
                    &overlay,
                    &text_draft,
                    x as f64 + 8.0,
                    y as f64 + 8.0,
                    width as f64,
                    height as f64,
                );
                if let Err(error) = refresh_annotation_preview(
                    &overlay,
                    &state_for_down.crop,
                    &state_for_down.editor,
                ) {
                    log::warn!("Could not render text editor preview: {error}");
                }
                return;
            }
            if let Some(editor) = state_for_down
                .editor
                .lock()
                .expect("annotation editor lock")
                .as_mut()
            {
                editor.begin(point);
            }
            if let Err(error) =
                refresh_annotation_preview(&overlay, &state_for_down.crop, &state_for_down.editor)
            {
                log::warn!("Could not render annotation preview: {error}");
            }
            return;
        }
        if let Err(error) = begin_selection_gesture(
            &overlay,
            &state_for_down,
            x as f64,
            y as f64,
            width as f64,
            height as f64,
        ) {
            log::warn!("Could not begin selection gesture: {error}");
        }
    });

    let weak_overlay = overlay.as_weak();
    let state_for_move = selection.clone();
    overlay.on_pointer_move(move |x, y, width, height| {
        platform::diagnose_cursor_motion();
        let Some(overlay) = weak_overlay.upgrade() else {
            return;
        };
        if state_for_move
            .geometry
            .lock()
            .expect("selection geometry lock")
            .is_active()
        {
            if let Err(error) = update_selection_gesture(
                &overlay,
                &state_for_move,
                x as f64,
                y as f64,
                width as f64,
                height as f64,
            ) {
                log::warn!("Could not update selection gesture: {error}");
                cancel_selection_gesture(&overlay, &state_for_move, width as f64, height as f64);
            }
            return;
        }
        if state_for_move
            .current_transform(width as f64, height as f64)
            .is_none()
        {
            return;
        }
        let crop = state_for_move
            .crop
            .lock()
            .expect("selected crop lock")
            .clone();
        let bounds = *state_for_move.bounds.lock().expect("selection bounds lock");
        let (Some(crop), Some(bounds)) = (crop, bounds) else {
            return;
        };
        let mapped = annotation_point(x as f64, y as f64, bounds, &crop);
        if let Ok(Some(point)) = mapped {
            let drawing = {
                let mut guard = state_for_move
                    .editor
                    .lock()
                    .expect("annotation editor lock");
                if let Some(editor) = guard.as_mut().filter(|editor| editor.is_drawing()) {
                    editor.update(point);
                    true
                } else {
                    false
                }
            };
            if drawing {
                if let Err(error) = refresh_annotation_preview(
                    &overlay,
                    &state_for_move.crop,
                    &state_for_move.editor,
                ) {
                    log::warn!("Could not render annotation preview: {error}");
                }
            }
        }
    });

    let weak_overlay = overlay.as_weak();
    let state_for_up = selection.clone();
    overlay.on_pointer_up(move |x, y, width, height| {
        let Some(overlay) = weak_overlay.upgrade() else {
            return;
        };
        if state_for_up
            .geometry
            .lock()
            .expect("selection geometry lock")
            .is_active()
        {
            finish_selection_gesture(
                &overlay,
                &state_for_up,
                x as f64,
                y as f64,
                width as f64,
                height as f64,
            );
            return;
        }
        if state_for_up
            .current_transform(width as f64, height as f64)
            .is_none()
        {
            return;
        }
        if state_for_up
            .text_position
            .lock()
            .expect("text position lock")
            .is_some()
        {
            return;
        }
        let crop = state_for_up
            .crop
            .lock()
            .expect("selected crop lock")
            .clone();
        let bounds = *state_for_up.bounds.lock().expect("selection bounds lock");
        let (Some(crop), Some(bounds)) = (crop, bounds) else {
            return;
        };
        let Ok(Some(point)) = annotation_point(x as f64, y as f64, bounds, &crop) else {
            return;
        };
        {
            let mut guard = state_for_up.editor.lock().expect("annotation editor lock");
            if let Some(editor) = guard.as_mut().filter(|editor| editor.is_drawing()) {
                editor.update(point);
                let style = editor.active_stroke_style();
                if let Err(error) = editor.finish(style, crop.width(), crop.height()) {
                    log::warn!("Could not finish annotation: {error}");
                }
            } else {
                return;
            }
        }
        if let Err(error) =
            refresh_annotation_preview(&overlay, &state_for_up.crop, &state_for_up.editor)
        {
            log::warn!("Could not render annotation preview: {error}");
        }
    });

    let weak_overlay = overlay.as_weak();
    let state_for_pointer_cancel = selection.clone();
    overlay.on_pointer_cancel(move || {
        let Some(overlay) = weak_overlay.upgrade() else {
            return;
        };
        let width = f64::from(overlay.get_frame_viewport_width());
        let height = f64::from(overlay.get_frame_viewport_height());
        cancel_selection_gesture(&overlay, &state_for_pointer_cancel, width, height);
        let cancelled = {
            let mut guard = state_for_pointer_cancel
                .editor
                .lock()
                .expect("annotation editor lock");
            if let Some(editor) = guard.as_mut().filter(|editor| editor.is_drawing()) {
                editor.cancel_draft();
                true
            } else {
                false
            }
        };
        if cancelled {
            if let Err(error) = refresh_annotation_preview(
                &overlay,
                &state_for_pointer_cancel.crop,
                &state_for_pointer_cancel.editor,
            ) {
                log::warn!(
                    "Could not restore annotation preview after pointer cancellation: {error}"
                );
            }
        }
    });

    let weak_overlay = overlay.as_weak();
    let state_for_crop_selection = selection.clone();
    overlay.on_crop_selection_pointer_event(move |kind, x, y, width, height| {
        let Some(overlay) = weak_overlay.upgrade() else {
            return;
        };
        let (x, y, width, height) = (x as f64, y as f64, width as f64, height as f64);
        match kind {
            PointerEventKind::Down => {
                if let Err(error) = begin_selection_gesture(
                    &overlay,
                    &state_for_crop_selection,
                    x,
                    y,
                    width,
                    height,
                ) {
                    log::warn!("Could not begin crop selection gesture: {error}");
                }
            }
            PointerEventKind::Move => {
                if let Err(error) = update_selection_gesture(
                    &overlay,
                    &state_for_crop_selection,
                    x,
                    y,
                    width,
                    height,
                ) {
                    log::warn!("Could not update adjustment gesture: {error}");
                    cancel_selection_gesture(&overlay, &state_for_crop_selection, width, height);
                }
            }
            PointerEventKind::Up => {
                finish_selection_gesture(&overlay, &state_for_crop_selection, x, y, width, height);
            }
            PointerEventKind::Cancel => {
                cancel_selection_gesture(&overlay, &state_for_crop_selection, width, height);
            }
            _ => {}
        }
    });

    let weak_overlay = overlay.as_weak();
    let state_for_toolbar_drag = selection.clone();
    overlay.on_toolbar_pointer_event(move |kind, x, y, width, height| {
        let Some(overlay) = weak_overlay.upgrade() else {
            return;
        };
        handle_toolbar_pointer_event(
            &overlay,
            &state_for_toolbar_drag,
            kind,
            x as f64,
            y as f64,
            width as f64,
            height as f64,
        );
    });

    let weak_overlay = overlay.as_weak();
    let state_for_delete_last = selection.clone();
    overlay.on_delete_last_annotation(move || {
        if let Some(overlay) = weak_overlay.upgrade() {
            if !overlay.get_annotation_mode() || overlay.get_text_entry_visible() {
                return;
            }
            let deleted = state_for_delete_last
                .editor
                .lock()
                .expect("annotation editor lock")
                .as_mut()
                .is_some_and(AnnotationEditor::delete_last);
            if deleted {
                update_preview_if_possible(
                    &overlay,
                    &state_for_delete_last.crop,
                    &state_for_delete_last.editor,
                );
                overlay.set_export_status("Last annotation deleted. Undo is available.".into());
            }
            overlay.invoke_focus_selection();
        }
    });
    let weak_overlay = overlay.as_weak();
    let state_for_confirm = selection.clone();
    overlay.on_confirm_selection_action(move || {
        if let Some(overlay) = weak_overlay.upgrade() {
            match reset_to_new_selection(&overlay, &state_for_confirm) {
                Ok(()) => set_status(&overlay, "Drag to create a new selection."),
                Err(error) => {
                    log::warn!("Could not discard the current selection: {error}");
                    set_status(
                        &overlay,
                        format!("Could not start a new selection: {error}"),
                    );
                }
            }
        }
    });

    let weak_overlay = overlay.as_weak();
    overlay.on_cancel_selection_action(move || {
        if let Some(overlay) = weak_overlay.upgrade() {
            overlay.set_confirm_selection_action_visible(false);
            overlay.invoke_focus_selection();
        }
    });

    let editor_slot = selection.editor.clone();
    let state_for_tool_selection = selection.clone();
    let weak_overlay = overlay.as_weak();
    overlay.on_tool_selected(move |index| {
        platform::diagnose_cursor("tool selected");
        let Some(tool) = AnnotationTool::from_ui_index(index) else {
            return;
        };
        if state_for_tool_selection
            .text_position
            .lock()
            .expect("text position lock")
            .is_some()
        {
            return;
        }
        if let Some(editor) = editor_slot.lock().expect("annotation editor lock").as_mut() {
            editor.select_tool(tool);
            if let Some(overlay) = weak_overlay.upgrade() {
                sync_annotation_controls(&overlay, editor);
                overlay.set_selected_tool(index);
                overlay.invoke_focus_selection();
            }
        }
    });

    let weak_overlay = overlay.as_weak();
    let state_for_shape_cycle = selection.clone();
    overlay.on_cycle_shape_tool(move || {
        let Some(overlay) = weak_overlay.upgrade() else {
            return;
        };
        let mut guard = state_for_shape_cycle
            .editor
            .lock()
            .expect("annotation editor lock");
        let Some(editor) = guard.as_mut() else {
            return;
        };
        let tool = editor.active_tool().next_shape();
        editor.select_tool(tool);
        sync_annotation_controls(&overlay, editor);
        overlay.set_selected_tool(tool.ui_index());
        overlay.invoke_focus_selection();
    });

    let weak_overlay = overlay.as_weak();
    let state_for_color = selection.clone();
    overlay.on_annotation_color_changed(move |index| {
        let Some(overlay) = weak_overlay.upgrade() else {
            return;
        };
        let color = StrokeColor::from_palette_index(index);
        let changed = {
            let mut guard = state_for_color
                .editor
                .lock()
                .expect("annotation editor lock");
            if let Some(editor) = guard.as_mut() {
                editor.set_active_color(color);
                true
            } else {
                false
            }
        };
        if changed {
            overlay.set_annotation_color_index(color.palette_index());
            update_preview_if_possible(&overlay, &state_for_color.crop, &state_for_color.editor);
        }
        overlay.invoke_focus_selection();
    });

    let weak_overlay = overlay.as_weak();
    let state_for_size = selection.clone();
    overlay.on_annotation_size_adjusted(move |direction| {
        let Some(overlay) = weak_overlay.upgrade() else {
            return;
        };
        let size = {
            let mut guard = state_for_size
                .editor
                .lock()
                .expect("annotation editor lock");
            let Some(editor) = guard.as_mut() else {
                return;
            };
            let step = if editor.active_tool() == AnnotationTool::Text {
                2.0
            } else {
                1.0
            };
            editor.set_active_size(editor.active_size() + f64::from(direction) * step)
        };
        overlay.set_annotation_size(size.round() as i32);
        update_preview_if_possible(&overlay, &state_for_size.crop, &state_for_size.editor);
        overlay.invoke_focus_selection();
    });

    let weak_overlay = overlay.as_weak();
    let state_for_text_content = selection.clone();
    overlay.on_text_content_changed(move |text| {
        let Some(overlay) = weak_overlay.upgrade() else {
            return;
        };
        overlay.set_text_entry_line_count(text_editor_visual_line_count(text.as_str()));
        let result = {
            let mut guard = state_for_text_content
                .editor
                .lock()
                .expect("annotation editor lock");
            match guard
                .as_mut()
                .and_then(|editor| editor.text_draft().cloned().map(|draft| (editor, draft)))
            {
                Some((editor, draft)) => {
                    editor.update_text_draft(text.to_string(), draft.text_style, draft.color)
                }
                None => Err("text editor draft is unavailable"),
            }
        };
        if let Err(error) = result {
            log::warn!("Could not update text preview: {error}");
            return;
        }
        update_preview_if_possible(
            &overlay,
            &state_for_text_content.crop,
            &state_for_text_content.editor,
        );
    });

    let weak_overlay = overlay.as_weak();
    let state_for_text_style = selection.clone();
    overlay.on_text_style_changed(move |bold, italic, alignment| {
        let Some(overlay) = weak_overlay.upgrade() else {
            return;
        };
        let result = {
            let mut guard = state_for_text_style
                .editor
                .lock()
                .expect("annotation editor lock");
            if let Some(editor) = guard.as_mut() {
                if let Some(draft) = editor.text_draft().cloned() {
                    let mut text_style = draft.text_style;
                    text_style.bold = bold;
                    text_style.italic = italic;
                    text_style.alignment = text_alignment_from_index(alignment);
                    editor.set_active_text_style(text_style);
                    editor.update_text_draft(draft.text, text_style, draft.color)
                } else {
                    Err("text editor draft is unavailable")
                }
            } else {
                Err("annotation editor is unavailable")
            }
        };
        if let Err(error) = result {
            log::warn!("Could not update text styling preview: {error}");
            return;
        }
        update_preview_if_possible(
            &overlay,
            &state_for_text_style.crop,
            &state_for_text_style.editor,
        );
    });

    let weak_overlay = overlay.as_weak();
    let state_for_text_reposition = selection.clone();
    overlay.on_request_text_reposition(move || {
        let Some(overlay) = weak_overlay.upgrade() else {
            return;
        };
        let is_existing_text = state_for_text_reposition
            .editor
            .lock()
            .expect("annotation editor lock")
            .as_ref()
            .and_then(|editor| {
                editor
                    .text_draft()
                    .map(|draft| draft.annotation_id.is_some())
            })
            .unwrap_or(false);
        if !is_existing_text {
            return;
        }
        let mut awaiting = state_for_text_reposition
            .text_awaiting_position
            .lock()
            .expect("text reposition lock");
        *awaiting = !*awaiting;
        overlay.set_text_entry_awaiting_position(*awaiting);
        overlay.set_text_entry_message(if *awaiting {
            "Click the new text position in the selection.".into()
        } else {
            "Text position unchanged.".into()
        });
        if *awaiting {
            overlay.set_text_entry_x(8.0);
            overlay.set_text_entry_y(8.0);
        }
    });

    let weak_overlay = overlay.as_weak();
    let state_for_text = selection.clone();
    overlay.on_submit_text(move |text| {
        let Some(overlay) = weak_overlay.upgrade() else {
            return;
        };
        if *state_for_text
            .text_awaiting_position
            .lock()
            .expect("text reposition lock")
        {
            return;
        }
        if text.trim().is_empty() {
            overlay.set_text_entry_message("Text cannot be empty. Enter text or cancel.".into());
            return;
        }
        let dimensions = state_for_text
            .crop
            .lock()
            .expect("selected crop lock")
            .as_ref()
            .map(|crop| (crop.width(), crop.height()));
        let Some((width, height)) = dimensions else {
            overlay
                .set_text_entry_message("The selected screenshot is no longer available.".into());
            return;
        };
        let result = {
            let mut guard = state_for_text
                .editor
                .lock()
                .expect("annotation editor lock");
            let Some(editor) = guard.as_mut() else {
                overlay
                    .set_text_entry_message("The annotation editor is no longer available.".into());
                return;
            };
            let Some(draft) = editor.text_draft().cloned() else {
                overlay
                    .set_text_entry_message("The text editor draft is no longer available.".into());
                return;
            };
            if let Err(error) =
                editor.update_text_draft(text.to_string(), draft.text_style, draft.color)
            {
                Err(error)
            } else {
                editor.commit_text_draft(width, height)
            }
        };
        match result {
            Ok(_) => {
                state_for_text.clear_text_interaction();
                overlay.set_text_entry_visible(false);
                overlay.set_text_entry_value(String::new().into());
                overlay.set_text_entry_line_count(1);
                overlay.set_text_entry_existing(false);
                overlay.set_text_entry_awaiting_position(false);
                overlay.set_text_entry_message("".into());
                update_preview_if_possible(&overlay, &state_for_text.crop, &state_for_text.editor);
                overlay.invoke_focus_selection();
            }
            Err(error) => {
                log::warn!("Could not commit text annotation: {error}");
                overlay.set_text_entry_message(format!("Could not save text: {error}").into());
            }
        }
    });

    let weak_overlay = overlay.as_weak();
    let state_for_cancel_text = selection.clone();
    overlay.on_cancel_text(move || {
        if let Some(editor) = state_for_cancel_text
            .editor
            .lock()
            .expect("annotation editor lock")
            .as_mut()
        {
            editor.cancel_text_draft();
        }
        state_for_cancel_text.clear_text_interaction();
        if let Some(overlay) = weak_overlay.upgrade() {
            overlay.set_text_entry_visible(false);
            overlay.set_text_entry_value(String::new().into());
            overlay.set_text_entry_line_count(1);
            overlay.set_text_entry_existing(false);
            overlay.set_text_entry_awaiting_position(false);
            overlay.set_text_entry_message("".into());
            update_preview_if_possible(
                &overlay,
                &state_for_cancel_text.crop,
                &state_for_cancel_text.editor,
            );
            overlay.invoke_focus_selection();
        }
    });

    let weak_overlay = overlay.as_weak();
    let state_for_undo = selection.clone();
    overlay.on_undo_annotation(move || {
        if let Some(editor) = state_for_undo
            .editor
            .lock()
            .expect("annotation editor lock")
            .as_mut()
        {
            editor.undo();
        }
        if let Some(overlay) = weak_overlay.upgrade() {
            update_preview_if_possible(&overlay, &state_for_undo.crop, &state_for_undo.editor);
        }
    });

    let weak_overlay = overlay.as_weak();
    let state_for_redo = selection.clone();
    overlay.on_redo_annotation(move || {
        if let Some(editor) = state_for_redo
            .editor
            .lock()
            .expect("annotation editor lock")
            .as_mut()
        {
            editor.redo();
        }
        if let Some(overlay) = weak_overlay.upgrade() {
            update_preview_if_possible(&overlay, &state_for_redo.crop, &state_for_redo.editor);
        }
    });
    let weak_overlay = overlay.as_weak();
    let state_for_save = selection.clone();
    let state_for_save_completion = selection.clone();
    let busy_for_save = export_busy.clone();
    let notification_owner_for_save = clipboard_owner_hwnd.clone();
    overlay.on_save_png(move || {
        platform::trace_event("export.save_png.callback");
        let Some(overlay) = weak_overlay.upgrade() else {
            return;
        };
        if state_for_save
            .text_position
            .lock()
            .expect("pending text position lock")
            .is_some()
        {
            overlay.set_export_status("Add or cancel the current text before saving.".into());
            return;
        }
        let (crop, annotations) =
            match annotation_snapshot(&state_for_save.crop, &state_for_save.editor) {
                Ok(snapshot) => snapshot,
                Err(error) => {
                    overlay.set_export_status(format!("Could not save screenshot: {error}").into());
                    return;
                }
            };
        let path = match platform::save_png_dialog() {
            Ok(Some(path)) => path,
            Ok(None) => {
                overlay
                    .set_export_status("Save cancelled. The capture session is unchanged.".into());
                return;
            }
            Err(error) => {
                overlay.set_export_status(format!("Save As failed: {error}").into());
                return;
            }
        };
        let notification_owner = notification_owner_for_save.load(Ordering::Acquire);
        start_export_operation(
            &overlay,
            busy_for_save.clone(),
            "Saving PNG…",
            ExportKind::Save,
            notification_owner,
            move || {
                let rendered = annotation_render::render_composite(&crop, &annotations)
                    .map_err(str::to_owned)?;
                crate::core::export::write_png(&path, &rendered)?;
                let name = path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "screenshot.png".into());
                Ok(format!("Saved {name}"))
            },
            {
                let state = state_for_save_completion.clone();
                move |overlay| close_capture_after_export(overlay, &state, "saved")
            },
        );
    });

    let weak_overlay = overlay.as_weak();
    let state_for_copy = selection.clone();
    let state_for_copy_completion = selection.clone();
    let busy_for_copy = export_busy.clone();
    let clipboard_owner_for_copy = clipboard_owner_hwnd.clone();
    overlay.on_copy_image(move || {
        platform::trace_event("export.copy_image.callback");
        let Some(overlay) = weak_overlay.upgrade() else {
            return;
        };
        if state_for_copy
            .text_position
            .lock()
            .expect("pending text position lock")
            .is_some()
        {
            overlay.set_export_status("Add or cancel the current text before copying.".into());
            return;
        }
        let (crop, annotations) =
            match annotation_snapshot(&state_for_copy.crop, &state_for_copy.editor) {
                Ok(snapshot) => snapshot,
                Err(error) => {
                    overlay.set_export_status(format!("Could not copy screenshot: {error}").into());
                    return;
                }
            };
        let owner_hwnd = clipboard_owner_for_copy.load(Ordering::Acquire);
        start_export_operation(
            &overlay,
            busy_for_copy.clone(),
            "Copying image to clipboard…",
            ExportKind::Copy,
            owner_hwnd,
            move || {
                let rendered = annotation_render::render_composite(&crop, &annotations)
                    .map_err(str::to_owned)?;
                platform::copy_image_to_clipboard(&rendered, owner_hwnd)?;
                Ok("Image copied to clipboard".into())
            },
            {
                let state = state_for_copy_completion.clone();
                move |overlay| close_capture_after_export(overlay, &state, "copied")
            },
        );
    });

    #[cfg(target_os = "windows")]
    overlay.window().on_close_requested(|| {
        platform::trace_event("overlay.close_requested");
        close_request_response(overlay_close_policy())
    });
    #[cfg(not(target_os = "windows"))]
    overlay
        .window()
        .on_close_requested(|| slint::CloseRequestResponse::HideWindow);

    #[cfg(target_os = "windows")]
    let tray_service = {
        let weak_overlay = overlay.as_weak();
        platform::trace_event("main.tray_start.before");
        let service = platform::tray::TrayService::start(move |command: TrayCommand| {
            let weak_overlay = weak_overlay.clone();
            platform::trace_event(&format!(
                "tray.command.dispatch_to_slint command={command:?}"
            ));
            if let Err(error) = slint::invoke_from_event_loop(move || {
                platform::trace_event(&format!("tray.command.slint_callback command={command:?}"));
                match command.action() {
                    AppAction::StartCapture => {
                        platform::trace_event("tray.action.start_capture");
                        if let Some(overlay) = weak_overlay.upgrade() {
                            overlay.invoke_request_capture();
                        }
                    }
                    AppAction::Quit => {
                        platform::trace_event("tray.action.exit");
                        let _ = slint::quit_event_loop();
                    }
                }
            }) {
                log::error!("Could not dispatch tray command {command:?}: {error}");
            }
        })
        .map_err(std::io::Error::other)?;
        clipboard_owner_hwnd.store(service.native_window_handle(), Ordering::Release);
        match service.hotkey_state() {
            core::hotkey::RegistrationState::Registered => {
                log::info!("Tray ready; Ctrl+Shift+S starts a capture")
            }
            core::hotkey::RegistrationState::Conflict => {
                log::warn!("Tray ready, but Ctrl+Shift+S is registered by another application")
            }
            core::hotkey::RegistrationState::Failed(code) => log::warn!(
                "Tray ready, but Ctrl+Shift+S registration failed with Windows error {code}"
            ),
        }
        platform::trace_event(&format!(
            "main.tray_start.after hwnd=0x{:X}",
            service.native_window_handle() as usize
        ));
        Some(service)
    };

    platform::trace_event("event_loop.first_callback.queue.before");
    let weak_overlay_startup = overlay.as_weak();
    let first_callback_result = slint::invoke_from_event_loop(move || {
        platform::trace_event("event_loop.initialized");
        if let Some(overlay) = weak_overlay_startup.upgrade() {
            platform::trace_slint_native_handle(
                "Overlay",
                "first_event_loop_callback",
                overlay.window(),
            );
        }
        platform::trace_event("event_loop.first_callback.completed");
    });
    platform::trace_event(&format!(
        "event_loop.first_callback.queue.after success={}",
        first_callback_result.is_ok()
    ));
    platform::trace_event("main.before_event_loop");
    #[cfg(target_os = "windows")]
    let event_loop_result = slint::run_event_loop_until_quit();
    #[cfg(not(target_os = "windows"))]
    let event_loop_result = slint::run_event_loop();
    platform::trace_event(&format!(
        "main.event_loop.returned result={event_loop_result:?}"
    ));
    event_loop_result?;
    #[cfg(target_os = "windows")]
    drop(tray_service);
    platform::trace_event("main.after_event_loop");
    platform::trace_event("main.before_overlay_component_drop");
    Ok(())
}

#[cfg(test)]
mod toolbar_layout_tests {
    use super::*;

    fn viewport(width: f64, height: f64) -> LogicalViewport {
        LogicalViewport { width, height }
    }

    fn selection(x: f64, y: f64, width: f64, height: f64) -> LogicalRect {
        LogicalRect {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn toolbar_prefers_below_selection_when_it_fits() {
        let layout = calculate_toolbar_layout(
            Some(selection(120.0, 100.0, 240.0, 100.0)),
            viewport(900.0, 700.0),
        );
        assert_eq!(
            layout,
            ToolbarLayout {
                x: 120.0,
                y: 208.0,
                scale: 1.0,
                forced_inside: false,
            }
        );
    }

    #[test]
    fn toolbar_uses_below_when_above_does_not_fit() {
        let layout = calculate_toolbar_layout(
            Some(selection(40.0, 20.0, 300.0, 80.0)),
            viewport(900.0, 700.0),
        );
        assert_eq!(layout.y, 108.0);
        assert!(!layout.forced_inside);
    }

    #[test]
    fn toolbar_uses_above_when_below_does_not_fit() {
        let layout = calculate_toolbar_layout(
            Some(selection(120.0, 560.0, 240.0, 100.0)),
            viewport(900.0, 700.0),
        );
        assert_eq!(layout.y, 488.0);
        assert!(!layout.forced_inside);
    }

    #[test]
    fn toolbar_uses_right_side_when_above_and_below_do_not_fit() {
        let layout = calculate_toolbar_layout(
            Some(selection(0.0, 0.0, 100.0, 600.0)),
            viewport(900.0, 600.0),
        );
        assert_eq!((layout.x, layout.y), (108.0, 0.0));
        assert!(!layout.forced_inside);
    }

    #[test]
    fn toolbar_uses_left_side_when_right_above_and_below_do_not_fit() {
        let layout = calculate_toolbar_layout(
            Some(selection(700.0, 0.0, 100.0, 600.0)),
            viewport(900.0, 600.0),
        );
        assert_eq!((layout.x, layout.y), (220.0, 0.0));
        assert!(!layout.forced_inside);
    }

    #[test]
    fn toolbar_moves_inside_when_above_and_below_do_not_fit() {
        let layout = calculate_toolbar_layout(
            Some(selection(12.0, 10.0, 760.0, 580.0)),
            viewport(800.0, 600.0),
        );
        assert_eq!(layout.x, 12.0);
        assert_eq!(layout.y, 10.0);
        assert!(layout.forced_inside);
    }

    #[test]
    fn toolbar_is_clamped_at_each_viewport_edge() {
        let viewport = viewport(800.0, 600.0);
        for (x, y) in [
            (-50.0, -20.0),
            (790.0, 590.0),
            (790.0, -20.0),
            (-50.0, 590.0),
        ] {
            let (x, y) = clamp_toolbar_position(x, y, viewport, 1.0);
            assert!((0.0..=328.0).contains(&x));
            assert!((0.0..=536.0).contains(&y));
        }
    }

    #[test]
    fn toolbar_scales_to_fit_small_logical_viewports() {
        let viewport = viewport(320.0, 180.0);
        let layout = calculate_toolbar_layout(Some(selection(0.0, 0.0, 200.0, 100.0)), viewport);
        assert!((layout.scale - 320.0 / TOOLBAR_DESIGN_WIDTH).abs() < f64::EPSILON);
        assert!(layout.x >= 0.0);
        assert!(layout.y >= 0.0);
        assert!(layout.x + TOOLBAR_DESIGN_WIDTH * layout.scale <= viewport.width);
        assert!(layout.y + TOOLBAR_DESIGN_HEIGHT * layout.scale <= viewport.height);
    }

    #[test]
    fn manual_position_tracks_the_same_physical_location_after_dpi_change() {
        let (x, y) = rebase_toolbar_position(
            ToolbarPositionSnapshot {
                manual: true,
                x: 100.0,
                y: 50.0,
            },
            viewport(1000.0, 750.0),
            viewport(800.0, 600.0),
        );
        assert_eq!((x, y), (125.0, 62.5));
    }

    #[test]
    fn zero_or_invalid_viewport_produces_a_safe_hidden_size() {
        assert_eq!(toolbar_scale_for_viewport(viewport(0.0, 400.0)), 0.0);
        assert_eq!(toolbar_scale_for_viewport(viewport(f64::NAN, 400.0)), 0.0);
        assert_eq!(
            calculate_toolbar_layout(Some(selection(2.0, 2.0, 10.0, 10.0)), viewport(0.0, 0.0)),
            ToolbarLayout {
                x: 0.0,
                y: 0.0,
                scale: 0.0,
                forced_inside: false,
            }
        );
    }

    #[test]
    fn crop_move_band_is_inside_the_perimeter_and_not_the_drawing_interior() {
        let crop = selection(20.0, 30.0, 200.0, 120.0);
        assert!(selection_move_band_contains(crop, 80.0, 35.0));
        assert!(selection_move_band_contains(crop, 25.0, 90.0));
        assert!(!selection_move_band_contains(crop, 100.0, 90.0));
        assert!(!selection_move_band_contains(crop, 10.0, 90.0));
    }

    #[test]
    fn text_editor_grows_for_multiline_input_and_caps_its_height() {
        assert_eq!(text_editor_visual_line_count("one line"), 1);
        assert_eq!(text_editor_visual_line_count("one\ntwo\nthree"), 3);
        assert_eq!(text_editor_visual_line_count(&"x\n".repeat(12)), 6);
    }
}
