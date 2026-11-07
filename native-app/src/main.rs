#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod core;
mod platform;

mod overlay_ui {
    include!(concat!(env!("OUT_DIR"), "/overlay.rs"));
}

use core::{
    annotation_render,
    annotations::{
        Annotation, AnnotationEditor, AnnotationTool, ImagePoint, StrokeStyle, ViewportToImage,
    },
    frame::{CapturedFrame, PixelRect},
    geometry::{FrameViewport, LogicalViewport, ViewportToFrame},
    lifecycle::{overlay_close_policy, AppAction, ClosePolicy, TrayCommand},
    selection::{hit_test_selection, GestureKind, LogicalRect, SelectionGeometry, SelectionHit},
    session::CaptureSession,
};
use overlay_ui::OverlayPrototype;
use slint::private_unstable_api::re_exports::PointerEventKind;
use slint::winit_030::{winit, EventResult, WinitWindowAccessor};
use slint::{ComponentHandle, Image, Rgba8Pixel, SharedPixelBuffer};
use std::{
    path::PathBuf,
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

fn crop_output_path() -> PathBuf {
    std::env::temp_dir()
        .join("carambola-native")
        .join("selected-crop.png")
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
    operation: impl FnOnce() -> Result<String, String> + Send + 'static,
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
        let busy_for_dispatch = busy.clone();
        let dispatched = slint::invoke_from_event_loop(move || {
            platform::trace_event(&format!("export.result_dispatch.entered status={status:?}"));
            busy_for_dispatch.store(false, Ordering::Release);
            let message = match result {
                Ok(message) => message,
                Err(error) => format!("Export failed: {error}"),
            };
            if let Some(overlay) = weak_overlay.upgrade() {
                overlay.set_export_busy(false);
                overlay.set_export_status(message.clone().into());
                trace_overlay_state(&overlay, "export.completed");
            }
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
    session: Arc<CaptureSession>,
    toolbar_snapshot: Arc<Mutex<Option<ToolbarPositionSnapshot>>>,
    toolbar_drag: Arc<Mutex<Option<ToolbarDrag>>>,
}

const TOOLBAR_DESIGN_WIDTH: f64 = 620.0;
const TOOLBAR_DESIGN_HEIGHT: f64 = 64.0;
const TOOLBAR_GAP: f64 = 8.0;

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
            session: Arc::new(CaptureSession::default()),
            toolbar_snapshot: Arc::new(Mutex::new(None)),
            toolbar_drag: Arc::new(Mutex::new(None)),
        }
    }

    fn current_transform(&self, width: f64, height: f64) -> Option<ViewportToFrame> {
        current_frame_transform(&self.viewport, width, height)
    }
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
    let above_y = selection.y - height - TOOLBAR_GAP;
    if above_y >= 0.0 {
        return ToolbarLayout {
            x,
            y: above_y,
            scale,
            forced_inside: false,
        };
    }
    let below_y = selection.y + selection.height + TOOLBAR_GAP;
    if below_y + height <= viewport.height {
        return ToolbarLayout {
            x,
            y: below_y,
            scale,
            forced_inside: false,
        };
    }
    let max_x = (viewport.width - width).max(0.0);
    let max_y = (viewport.height - height).max(0.0);
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

fn request_selection_reset(overlay: &OverlayPrototype, state: &SelectionState) {
    if selection_has_pending_edits(state) {
        overlay.set_confirm_selection_action_message(
            "Discard the current selection and its annotations?".into(),
        );
        overlay.set_confirm_selection_action_visible(true);
    } else if let Err(error) = reset_to_new_selection(overlay, state) {
        log::warn!("Could not start a new selection: {error}");
        set_status(overlay, format!("Could not start a new selection: {error}"));
    }
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
    *state.text_position.lock().expect("text position lock") = None;
    *state
        .toolbar_snapshot
        .lock()
        .expect("toolbar position snapshot lock") = None;
    cancel_toolbar_drag(overlay, state);
    overlay.set_toolbar_manual_position(false);
    overlay.set_annotation_mode(false);
    overlay.set_selection_adjustment_mode(false);
    overlay.set_confirm_selection_action_visible(false);
    overlay.set_annotation_image(Image::default());
    overlay.set_text_entry_visible(false);
    overlay.set_text_entry_value(String::new().into());
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
        if !overlay.get_selection_adjustment_mode() {
            return Ok(false);
        }
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
        *state.text_position.lock().expect("text position lock") = None;
        overlay.set_text_entry_visible(false);
        overlay.set_text_entry_value(String::new().into());
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
    overlay.set_selected_tool(AnnotationTool::Pen.ui_index());
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
                overlay.set_text_entry_x(x.max(0.0) as f32);
                overlay
                    .set_text_entry_y(y.min((logical_size.height - 52.0).max(0.0)).max(0.0) as f32);
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
    let crop = crop_slot.lock().expect("selected crop lock").clone();
    let Some(crop) = crop else {
        return Err("selected crop is no longer available");
    };
    let (annotations, can_undo, can_redo) = {
        let guard = editor_slot.lock().expect("annotation editor lock");
        let Some(editor) = guard.as_ref() else {
            return Err("annotation editor is no longer available");
        };
        (
            editor.annotations_for_render(StrokeStyle::default()),
            editor.can_undo(),
            editor.can_redo(),
        )
    };
    let rendered = annotation_render::render_composite(&crop, &annotations)?;
    let output = image_from_rgba(rendered.pixels(), rendered.width(), rendered.height());
    overlay.set_annotation_image(output);
    overlay.set_can_undo(can_undo);
    overlay.set_can_redo(can_redo);
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
        *state_for_capture.text_position.lock().expect("text position lock") = None;

        let Some(overlay) = weak_overlay.upgrade() else {
            finish_capture_session(&state_for_capture.session, "overlay_unavailable_before_capture");
            return;
        };
        overlay.set_annotation_mode(false);
        overlay.set_selection_adjustment_mode(false);
        overlay.set_confirm_selection_action_visible(false);
        overlay.set_annotation_image(Image::default());
        overlay.set_text_entry_visible(false);
        overlay.set_text_entry_value(String::new().into());
        overlay.set_can_undo(false);
        overlay.set_can_redo(false);
        overlay.set_export_busy(false);
        overlay.set_export_status("Capturing desktop…".into());
        clear_selection_preview(&overlay);

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
            platform::trace_event("capture.capture_primary.worker_begin");
            let captured = platform::capture_primary();
            platform::trace_event(&format!("capture.capture_primary.worker_end success={}", captured.is_ok()));
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
                        set_status(&overlay, "The captured frame dimensions do not match the primary display.");
                        finish_capture_session(&state_for_result.session, "frame_display_bounds_mismatch");
                        return;
                    }
                    platform::trace_event(&format!(
                        "capture.frame_desktop_geometry display={:?} origin=({}, {}) end=({}, {})",
                        display.id, origin.x, origin.y, bottom_right.x, bottom_right.y
                    ));
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
                overlay.set_selection_adjustment_mode(false);
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
        *state_for_cancel
            .text_position
            .lock()
            .expect("text position lock") = None;
        if let Some(overlay) = weak_overlay.upgrade() {
            overlay.set_annotation_mode(false);
            overlay.set_selection_adjustment_mode(false);
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
        if overlay.get_annotation_mode() && !overlay.get_selection_adjustment_mode() {
            if state_for_down
                .current_transform(width as f64, height as f64)
                .is_none()
            {
                return;
            }
            if toolbar_contains(&overlay, x as f64, y as f64)
                || state_for_down
                    .toolbar_drag
                    .lock()
                    .expect("toolbar drag lock")
                    .is_some()
                || state_for_down
                    .text_position
                    .lock()
                    .expect("text position lock")
                    .is_some()
            {
                return;
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
            if state_for_down
                .editor
                .lock()
                .expect("annotation editor lock")
                .as_ref()
                .is_some_and(|editor| editor.active_tool() == AnnotationTool::Text)
            {
                *state_for_down
                    .text_position
                    .lock()
                    .expect("text position lock") = Some(point);
                overlay.set_text_entry_value(String::new().into());
                overlay.set_text_entry_x((x + 8.0).max(0.0));
                overlay.set_text_entry_y((y + 8.0).min((height - 52.0).max(0.0)));
                overlay.set_text_entry_visible(true);
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
                if let Err(error) =
                    editor.finish(StrokeStyle::default(), crop.width(), crop.height())
                {
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
    let state_for_adjustment = selection.clone();
    overlay.on_adjustment_pointer_event(move |kind, x, y, width, height| {
        let Some(overlay) = weak_overlay.upgrade() else {
            return;
        };
        let (x, y, width, height) = (x as f64, y as f64, width as f64, height as f64);
        match kind {
            PointerEventKind::Down => {
                if let Err(error) =
                    begin_selection_gesture(&overlay, &state_for_adjustment, x, y, width, height)
                {
                    log::warn!("Could not begin adjustment gesture: {error}");
                }
            }
            PointerEventKind::Move => {
                if let Err(error) =
                    update_selection_gesture(&overlay, &state_for_adjustment, x, y, width, height)
                {
                    log::warn!("Could not update adjustment gesture: {error}");
                    cancel_selection_gesture(&overlay, &state_for_adjustment, width, height);
                }
            }
            PointerEventKind::Up => {
                finish_selection_gesture(&overlay, &state_for_adjustment, x, y, width, height);
            }
            PointerEventKind::Cancel => {
                cancel_selection_gesture(&overlay, &state_for_adjustment, width, height);
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
    let state_for_adjustment_mode = selection.clone();
    overlay.on_request_adjustment_mode(move |enabled| {
        let Some(overlay) = weak_overlay.upgrade() else {
            return;
        };
        let has_selection = state_for_adjustment_mode
            .geometry
            .lock()
            .expect("selection geometry lock")
            .committed()
            .is_some();
        if enabled && !has_selection {
            overlay.set_selection_adjustment_mode(false);
            return;
        }
        if !enabled {
            cancel_selection_gesture(
                &overlay,
                &state_for_adjustment_mode,
                f64::from(overlay.get_frame_viewport_width()),
                f64::from(overlay.get_frame_viewport_height()),
            );
        }
        overlay.set_selection_adjustment_mode(enabled);
        overlay.invoke_focus_selection();
    });

    let weak_overlay = overlay.as_weak();
    let state_for_new_selection = selection.clone();
    overlay.on_request_new_selection(move || {
        if let Some(overlay) = weak_overlay.upgrade() {
            request_selection_reset(&overlay, &state_for_new_selection);
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
    let weak_overlay = overlay.as_weak();
    overlay.on_tool_selected(move |index| {
        platform::diagnose_cursor("tool selected");
        let Some(tool) = AnnotationTool::from_ui_index(index) else {
            return;
        };
        if let Some(editor) = editor_slot.lock().expect("annotation editor lock").as_mut() {
            editor.select_tool(tool);
            if let Some(overlay) = weak_overlay.upgrade() {
                overlay.set_selected_tool(index);
                overlay.invoke_focus_selection();
            }
        }
    });

    let weak_overlay = overlay.as_weak();
    let state_for_text = selection.clone();
    overlay.on_submit_text(move |text| {
        let Some(overlay) = weak_overlay.upgrade() else {
            return;
        };
        overlay.set_text_entry_visible(false);
        let position = state_for_text
            .text_position
            .lock()
            .expect("text position lock")
            .take();
        let Some(position) = position else { return };
        let dimensions = state_for_text
            .crop
            .lock()
            .expect("selected crop lock")
            .as_ref()
            .map(|crop| (crop.width(), crop.height()));
        let Some((width, height)) = dimensions else {
            return;
        };
        let result = match state_for_text
            .editor
            .lock()
            .expect("annotation editor lock")
            .as_mut()
        {
            Some(editor) => editor.add_text(position, text.to_string(), width, height),
            None => Err("annotation editor is no longer available"),
        };
        match result {
            Ok(_) => {
                update_preview_if_possible(&overlay, &state_for_text.crop, &state_for_text.editor)
            }
            Err(error) => log::warn!("Could not add text annotation: {error}"),
        }
    });

    let weak_overlay = overlay.as_weak();
    let state_for_cancel_text = selection.clone();
    overlay.on_cancel_text(move || {
        *state_for_cancel_text
            .text_position
            .lock()
            .expect("text position lock") = None;
        if let Some(overlay) = weak_overlay.upgrade() {
            overlay.set_text_entry_visible(false);
            overlay.set_text_entry_value(String::new().into());
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
    let busy_for_save = export_busy.clone();
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
        start_export_operation(
            &overlay,
            busy_for_save.clone(),
            "Saving PNG…",
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
        );
    });

    let weak_overlay = overlay.as_weak();
    let state_for_copy = selection.clone();
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
            move || {
                let rendered = annotation_render::render_composite(&crop, &annotations)
                    .map_err(str::to_owned)?;
                platform::copy_image_to_clipboard(&rendered, owner_hwnd)?;
                Ok("Image copied to clipboard".into())
            },
        );
    });

    let weak_overlay = overlay.as_weak();
    let state_for_done = selection.clone();
    let busy_for_done = export_busy.clone();
    overlay.on_finish_annotations(move || {
        let action_id = platform::begin_window_action("Done");
        platform::trace_event("capture.done.requested");
        if let Some(overlay) = weak_overlay.upgrade() {
            trace_overlay_state(&overlay, "capture.done.requested");
        }
        if busy_for_done.load(Ordering::Acquire) {
            if let Some(overlay) = weak_overlay.upgrade() {
                overlay.set_export_status("Wait for the current export before finishing.".into());
            }
            platform::finish_window_action(action_id, "rejected_export_busy");
            return;
        }
        let Some(overlay) = weak_overlay.upgrade() else {
            platform::finish_window_action(action_id, "overlay_unavailable");
            return;
        };
        platform::diagnose_cursor("done before close");
        if state_for_done
            .text_position
            .lock()
            .expect("pending text position lock")
            .is_some()
        {
            overlay.set_export_status("Add or cancel the current text before finishing.".into());
            platform::finish_window_action(action_id, "rejected_pending_text");
            return;
        }
        let crop = state_for_done
            .crop
            .lock()
            .expect("selected crop lock")
            .take();
        let annotations = state_for_done
            .editor
            .lock()
            .expect("annotation editor lock")
            .as_ref()
            .map(|editor| editor.document().annotations().to_vec());
        let (Some(crop), Some(annotations)) = (crop, annotations) else {
            log::error!("Cannot finish annotation: crop or editor state is missing");
            platform::finish_window_action(action_id, "rejected_missing_crop_or_editor");
            return;
        };
        if !state_for_done.session.begin_saving() {
            *state_for_done.crop.lock().expect("selected crop lock") = Some(crop);
            log::error!("Could not transition capture session into saving state");
            platform::finish_window_action(action_id, "rejected_session_transition");
            return;
        }
        platform::trace_event("capture.session.transition=Saving");
        *state_for_done.frame.lock().expect("capture frame lock") = None;
        *state_for_done.bounds.lock().expect("selection bounds lock") = None;
        *state_for_done
            .geometry
            .lock()
            .expect("selection geometry lock") = SelectionGeometry::default();
        *state_for_done
            .active_kind
            .lock()
            .expect("active gesture lock") = None;
        *state_for_done.viewport.lock().expect("frame viewport lock") = None;
        *state_for_done
            .editor
            .lock()
            .expect("annotation editor lock") = None;
        *state_for_done
            .text_position
            .lock()
            .expect("text position lock") = None;
        overlay.set_annotation_mode(false);
        overlay.set_selection_adjustment_mode(false);
        overlay.set_annotation_image(Image::default());
        overlay.set_text_entry_visible(false);
        platform::trace_event("overlay.hide.done.before");
        trace_overlay_state(&overlay, "overlay.hide.done.before");
        let hide_result = overlay.hide();
        platform::trace_event(&format!(
            "overlay.hide.done.result success={}",
            hide_result.is_ok()
        ));
        trace_overlay_state(&overlay, "overlay.hide.done.after");
        overlay.window().set_fullscreen(false);
        platform::trace_slint_native_handle(
            "Overlay",
            "done.after_hide_and_fullscreen_exit",
            overlay.window(),
        );
        overlay.set_captured_image(Image::default());
        platform::diagnose_cursor("done after close");

        let weak_overlay_result = overlay.as_weak();
        let session_result = state_for_done.session.clone();
        thread::spawn(move || {
            platform::trace_event("capture.done.save_worker.begin");
            let saved = (|| {
                let output = annotation_render::render_composite(&crop, &annotations)
                    .map_err(str::to_owned)?;
                let output_path = crop_output_path();
                std::fs::create_dir_all(output_path.parent().expect("temp path parent"))
                    .map_err(|error| error.to_string())?;
                crate::core::export::write_png(&output_path, &output)?;
                Ok::<_, String>((output_path, output.width(), output.height()))
            })();
            platform::trace_event(&format!(
                "capture.done.save_worker.end success={}",
                saved.is_ok()
            ));
            let session_for_dispatch = session_result.clone();
            if let Err(error) = slint::invoke_from_event_loop(move || {
                platform::trace_event("capture.done.result_dispatch.entered");
                if let Some(overlay) = weak_overlay_result.upgrade() {
                    match saved {
                        Ok((path, width, height)) => set_status(
                            &overlay,
                            format!(
                                "Capture complete. Temporary PNG: {} ({width}×{height})",
                                path.display()
                            ),
                        ),
                        Err(error) => set_status(
                            &overlay,
                            format!("Could not save annotated screenshot: {error}"),
                        ),
                    }
                }
                finish_capture_session(&session_for_dispatch, "done_result_dispatched");
                platform::trace_event("capture.done.session_finished");
            }) {
                log::error!(
                    "Could not return annotated screenshot result to the UI thread: {error}"
                );
                finish_capture_session(&session_result, "done_result_dispatch_failed");
            }
        });
        platform::finish_window_action(action_id, "save_worker_started");
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
    fn toolbar_prefers_above_selection_when_it_fits() {
        let layout = calculate_toolbar_layout(
            Some(selection(120.0, 100.0, 240.0, 100.0)),
            viewport(900.0, 700.0),
        );
        assert_eq!(
            layout,
            ToolbarLayout {
                x: 120.0,
                y: 28.0,
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
            assert!((0.0..=180.0).contains(&x));
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
}
