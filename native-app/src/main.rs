#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod core;
mod platform;

mod overlay_ui {
    include!(concat!(env!("OUT_DIR"), "/overlay.rs"));
}

use core::{
    annotation_render,
    annotations::{
        Annotation, AnnotationEditor, AnnotationSizeLevel, AnnotationTool, ImagePoint, StrokeColor,
        TextAlignment, TextAnnotationDraft, ViewportToImage,
    },
    frame::{CapturedFrame, PixelRect},
    geometry::{FrameViewport, LogicalViewport, ViewportToFrame},
    lifecycle::{
        export_notification, overlay_close_policy, overlay_shortcut, AppAction, ClosePolicy,
        ExportKind, OverlayShortcut, TrayCommand,
    },
    ocr::{complete as complete_ocr, normalize_result, OcrCompletion},
    selection::{hit_test_selection, GestureKind, LogicalRect, SelectionGeometry, SelectionHit},
    session::CaptureSession,
    settings::{self, AppSettings, StartupRegistrar},
};
use overlay_ui::{OverlayPrototype, SettingsPrototype};
use slint::private_unstable_api::re_exports::PointerEventKind;
use slint::winit_030::{winit, EventResult, WinitWindowAccessor};
use slint::{ComponentHandle, Image, Rgba8Pixel, SharedPixelBuffer};
use std::{
    sync::{
        atomic::{AtomicBool, AtomicIsize, AtomicU64, Ordering},
        Arc, Mutex,
    },
    thread,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

static DEFAULT_SAVE_SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static PREVIEW_TIMING_SAMPLES: AtomicU64 = AtomicU64::new(0);
static SELECTION_TIMING_SAMPLES: AtomicU64 = AtomicU64::new(0);

fn trace_sampled_duration(name: &str, started: Instant, counter: &AtomicU64) {
    let sample = counter.fetch_add(1, Ordering::Relaxed) + 1;
    if sample == 1 || sample.is_multiple_of(60) {
        platform::trace_event(&format!(
            "perf.{name} sample={sample} elapsed_us={}",
            started.elapsed().as_micros()
        ));
    }
}

struct ExportSuccess {
    message: String,
    preview: CapturedFrame,
}

struct PendingScreenshotNotification {
    message: String,
    preview: CapturedFrame,
    owner_hwnd: isize,
    enabled: bool,
}

fn report_copy_failure(error: &str, owner_hwnd: isize) {
    let message = format!("Could not copy screenshot: {error}");
    log::error!("{message}");
    if let Err(notification_error) = platform::show_notification(&message, owner_hwnd) {
        log::error!("Could not show native copy failure notification: {notification_error}");
    }
}

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
    notification: PendingScreenshotNotification,
) -> Result<(), String> {
    platform::trace_event(&format!(
        "capture.export_success.close.begin reason={reason:?}"
    ));
    schedule_capture_close(overlay, state, reason, true, Some(notification))
}

fn schedule_capture_close(
    overlay: &OverlayPrototype,
    state: &SelectionState,
    reason: &str,
    finish_session: bool,
    notification: Option<PendingScreenshotNotification>,
) -> Result<(), String> {
    if state.closing.swap(true, Ordering::AcqRel) {
        return Ok(());
    }
    state.cancel_ocr_operation();
    overlay.set_overlay_closing(true);
    // Leave the captured desktop image visible while the controls fade away.
    // A black cover would otherwise become a full-screen black frame just
    // before the fullscreen HWND is hidden.
    overlay.set_fade_animation_enabled(false);
    overlay.set_fade_cover_color(slint::Color::from_argb_u8(0, 0, 0, 0));
    overlay.set_fade_animation_enabled(true);
    overlay.set_content_opacity(0.0);
    platform::trace_event(&format!("capture.fade_out.begin reason={reason:?}"));
    let weak_overlay = overlay.as_weak();
    let state = state.clone();
    let reason = reason.to_owned();
    slint::Timer::single_shot(std::time::Duration::from_millis(160), move || {
        if let Some(overlay) = weak_overlay.upgrade() {
            trace_overlay_state(&overlay, "capture.fade_out.complete.before_hide");
            if let Err(error) = overlay.hide() {
                log::warn!("Could not hide the selection overlay after fade-out: {error}");
            }
            overlay.window().set_fullscreen(false);
            overlay.set_fade_animation_enabled(false);
            overlay.set_content_opacity(1.0);
            overlay.set_annotation_mode(false);
            overlay.set_color_palette_open(false);
            overlay.set_size_menu_open(false);
            overlay.set_copy_in_progress(false);
            overlay.set_ocr_in_progress(false);
            overlay.set_export_busy(false);
            overlay.set_export_status("".into());
            overlay.set_annotation_image(Image::default());
            overlay.set_text_entry_visible(false);
            overlay.set_text_entry_line_count(1);
            overlay.set_captured_image(Image::default());
            overlay.set_control_held(false);
            overlay.set_overlay_closing(false);
            trace_overlay_state(&overlay, "capture.fade_out.complete.hidden");
        }
        *state.frame.lock().expect("capture frame lock") = None;
        *state.crop.lock().expect("selected crop lock") = None;
        *state.bounds.lock().expect("selection bounds lock") = None;
        *state.geometry.lock().expect("selection geometry lock") = SelectionGeometry::default();
        *state.active_kind.lock().expect("active gesture lock") = None;
        *state.viewport.lock().expect("frame viewport lock") = None;
        *state.editor.lock().expect("annotation editor lock") = None;
        state.clear_text_interaction();
        if finish_session {
            finish_capture_session(&state.session, &reason);
        }
        state.closing.store(false, Ordering::Release);
        platform::trace_event(&format!("capture.fade_out.complete reason={reason:?}"));
        if let Some(notification) = notification.filter(|notification| notification.enabled) {
            // Fullscreen can silence Windows toast banners. Submit only after
            // the overlay is hidden and its fullscreen transition has ended.
            slint::Timer::single_shot(std::time::Duration::from_millis(100), move || {
                thread::spawn(move || {
                    if let Err(error) = platform::show_screenshot_notification(
                        &notification.message,
                        notification.owner_hwnd,
                        &notification.preview,
                    ) {
                        log::warn!("Could not show export success notification: {error}");
                    }
                });
            });
        }
    });
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

fn claim_export_operation(
    overlay: &OverlayPrototype,
    busy: &AtomicBool,
    status: &'static str,
) -> bool {
    if busy
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        overlay.set_export_status("An export operation is already running.".into());
        return false;
    }
    overlay.set_export_busy(true);
    overlay.set_export_status(status.into());
    true
}

fn release_export_claim(overlay: &OverlayPrototype, busy: &AtomicBool, message: &str) {
    overlay.set_export_busy(false);
    overlay.set_export_status(message.into());
    busy.store(false, Ordering::Release);
}

fn request_save(
    overlay: &OverlayPrototype,
    state: &SelectionState,
    busy: Arc<AtomicBool>,
    notification_owner: Arc<AtomicIsize>,
    application_settings: Arc<Mutex<AppSettings>>,
    save_as: bool,
) {
    if state.closing.load(Ordering::Acquire) {
        return;
    }
    if state
        .text_position
        .lock()
        .expect("pending text position lock")
        .is_some()
    {
        overlay.set_export_status("Add or cancel the current text before saving.".into());
        return;
    }
    let (crop, annotations) = match annotation_snapshot(&state.crop, &state.editor) {
        Ok(snapshot) => snapshot,
        Err(error) => {
            overlay.set_export_status(format!("Could not save screenshot: {error}").into());
            return;
        }
    };
    let (save_directory, notifications_enabled) = {
        let settings = application_settings
            .lock()
            .expect("application settings lock");
        (
            settings.save_directory.clone(),
            settings.notifications_enabled,
        )
    };

    let path = if save_as {
        if !claim_export_operation(overlay, &busy, "Choose where to save the screenshot…") {
            return;
        }
        match platform::save_png_dialog(save_directory.as_deref()) {
            Ok(Some(path)) => path,
            Ok(None) => {
                release_export_claim(
                    overlay,
                    &busy,
                    "Save cancelled. The capture session is unchanged.",
                );
                return;
            }
            Err(error) => {
                release_export_claim(overlay, &busy, &format!("Save As failed: {error}"));
                return;
            }
        }
    } else {
        let Some(directory) = save_directory else {
            overlay.set_export_status(
                "Choose a default save folder in Settings before saving.".into(),
            );
            return;
        };
        if let Err(error) = settings::validate_save_directory(&directory) {
            overlay.set_export_status(
                format!("Default save folder is unavailable: {error}. Choose another in Settings.")
                    .into(),
            );
            return;
        }
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let sequence = DEFAULT_SAVE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        crate::core::export::default_screenshot_path(
            &directory,
            nanos,
            std::process::id(),
            sequence,
        )
    };

    let owner_hwnd = notification_owner.load(Ordering::Acquire);
    let busy_for_completion = Arc::clone(&busy);
    let state_for_completion = state.clone();
    let save = move || {
        let rendered =
            annotation_render::render_composite(&crop, &annotations).map_err(str::to_owned)?;
        crate::core::export::write_png(&path, &rendered)?;
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "screenshot.png".into());
        Ok(ExportSuccess {
            message: format!("Saved {name}"),
            preview: rendered,
        })
    };
    let completed =
        move |overlay: &OverlayPrototype, success: ExportSuccess, message: &'static str| {
            close_capture_after_export(
                overlay,
                &state_for_completion,
                "saved",
                PendingScreenshotNotification {
                    message: message.to_owned(),
                    preview: success.preview,
                    owner_hwnd,
                    enabled: notifications_enabled,
                },
            )
        };
    if save_as {
        start_export_operation_claimed(
            overlay,
            busy_for_completion,
            "Saving PNG…",
            ExportKind::Save,
            owner_hwnd,
            save,
            completed,
        );
    } else {
        start_export_operation(
            overlay,
            busy_for_completion,
            "Saving PNG…",
            ExportKind::Save,
            owner_hwnd,
            save,
            completed,
        );
    }
}

fn start_export_operation(
    overlay: &OverlayPrototype,
    busy: Arc<AtomicBool>,
    status: &'static str,
    kind: ExportKind,
    notification_owner_hwnd: isize,
    operation: impl FnOnce() -> Result<ExportSuccess, String> + Send + 'static,
    on_success: impl FnOnce(&OverlayPrototype, ExportSuccess, &'static str) -> Result<(), String>
        + Send
        + 'static,
) {
    platform::trace_event(&format!("export.requested status={status:?}"));
    if !claim_export_operation(overlay, &busy, status) {
        return;
    }
    start_export_operation_claimed(
        overlay,
        busy,
        status,
        kind,
        notification_owner_hwnd,
        operation,
        on_success,
    );
}

fn start_export_operation_claimed(
    overlay: &OverlayPrototype,
    busy: Arc<AtomicBool>,
    status: &'static str,
    kind: ExportKind,
    notification_owner_hwnd: isize,
    operation: impl FnOnce() -> Result<ExportSuccess, String> + Send + 'static,
    on_success: impl FnOnce(&OverlayPrototype, ExportSuccess, &'static str) -> Result<(), String>
        + Send
        + 'static,
) {
    overlay.set_export_busy(true);
    overlay.set_copy_in_progress(kind == ExportKind::Copy);
    overlay.set_export_status(status.into());
    platform::trace_native_event(&format!("export.started status={status:?}"));
    trace_overlay_state(overlay, "export.started");
    let weak_overlay = overlay.as_weak();
    thread::spawn(move || {
        platform::trace_event(&format!("export.worker.begin status={status:?}"));
        let worker_started = Instant::now();
        let result = operation();
        platform::trace_event(&format!(
            "perf.export.operation status={status:?} elapsed_ms={} success={}",
            worker_started.elapsed().as_millis(),
            result.is_ok()
        ));
        platform::trace_event(&format!(
            "export.worker.end status={status:?} success={}",
            result.is_ok()
        ));
        platform::trace_event(&format!(
            "perf.export.worker_total status={status:?} elapsed_ms={}",
            worker_started.elapsed().as_millis()
        ));
        let busy_for_dispatch = busy.clone();
        let dispatched = slint::invoke_from_event_loop(move || {
            platform::trace_event(&format!("export.result_dispatch.entered status={status:?}"));
            let notification_message = export_notification(&result, kind);
            let (mut message, success) = match result {
                Ok(success) => (success.message.clone(), Some(success)),
                Err(error) => {
                    if kind == ExportKind::Copy {
                        report_copy_failure(&error, notification_owner_hwnd);
                    }
                    (format!("Export failed: {error}"), None)
                }
            };
            if let Some(overlay) = weak_overlay.upgrade() {
                if let Some(success) = success {
                    let notification_message = notification_message
                        .expect("successful export has a success notification message");
                    if let Err(error) = on_success(&overlay, success, notification_message) {
                        message = format!("{message}, but the overlay could not close: {error}");
                    }
                }
                overlay.set_export_busy(false);
                overlay.set_copy_in_progress(false);
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
    ocr_cancellation: Arc<Mutex<Option<Arc<AtomicBool>>>>,
    session: Arc<CaptureSession>,
    toolbar_snapshot: Arc<Mutex<Option<ToolbarPositionSnapshot>>>,
    toolbar_drag: Arc<Mutex<Option<ToolbarDrag>>>,
    closing: Arc<AtomicBool>,
}

const TOOLBAR_DESIGN_WIDTH: f64 = 466.0;
const TOOLBAR_DESIGN_HEIGHT: f64 = 38.0;
const TOOLBAR_GAP: f64 = 8.0;
const TEXT_EDITOR_WIDTH: f64 = 360.0;
const TEXT_EDITOR_HEIGHT: f64 = 208.0;

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
            ocr_cancellation: Arc::new(Mutex::new(None)),
            session: Arc::new(CaptureSession::default()),
            toolbar_snapshot: Arc::new(Mutex::new(None)),
            toolbar_drag: Arc::new(Mutex::new(None)),
            closing: Arc::new(AtomicBool::new(false)),
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

    fn cancel_ocr_operation(&self) {
        if let Some(cancellation) = self
            .ocr_cancellation
            .lock()
            .expect("OCR cancellation lock")
            .take()
        {
            cancellation.store(true, Ordering::Release);
        }
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
    overlay.set_annotation_size(
        AnnotationSizeLevel::closest_for(AnnotationTool::Text, draft.text_style.font_size).as_ui(),
    );
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
    schedule_text_entry_focus(overlay);
}

fn schedule_text_entry_focus(overlay: &OverlayPrototype) {
    let weak_overlay = overlay.as_weak();
    slint::Timer::single_shot(std::time::Duration::ZERO, move || {
        let Some(overlay) = weak_overlay.upgrade() else {
            return;
        };
        if !should_focus_text_entry(
            overlay.get_text_entry_visible(),
            overlay.get_text_entry_awaiting_position(),
        ) {
            return;
        }

        overlay.invoke_request_text_entry_focus();
        if !overlay.get_text_entry_input_focused() {
            log::warn!("Text editor is visible but did not obtain keyboard focus");
        }
    });
}

fn should_focus_text_entry(visible: bool, awaiting_position: bool) -> bool {
    visible && !awaiting_position
}

fn sync_annotation_controls(overlay: &OverlayPrototype, editor: &AnnotationEditor) {
    overlay.set_annotation_size(editor.active_size_level().as_ui());
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

fn crop_move_modifier(control: bool, meta: bool) -> bool {
    #[cfg(target_os = "macos")]
    {
        // Slint maps the physical Control key to `meta` on macOS because
        // Command is exposed as `control` for conventional shortcuts.
        let _ = control;
        meta
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = meta;
        control
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
    if x >= left && x <= right && y >= top && y <= bottom {
        return true;
    }
    let scale = f64::from(overlay.get_toolbar_scale());
    if overlay.get_size_menu_open() {
        let menu_top = top
            + if top + 184.0 * scale > f64::from(overlay.get_frame_viewport_height()) {
                -146.0 * scale
            } else {
                38.0 * scale
            };
        if x >= left + 398.0 * scale
            && x <= left + 444.0 * scale
            && y >= menu_top
            && y <= menu_top + 144.0 * scale
        {
            return true;
        }
    }
    if overlay.get_color_palette_open() {
        let palette_top = top
            + if top + 70.0 * scale > f64::from(overlay.get_frame_viewport_height()) {
                -30.0 * scale
            } else {
                40.0 * scale
            };
        if x >= left + 294.0 * scale
            && x <= left + 440.0 * scale
            && y >= palette_top
            && y <= palette_top + 26.0 * scale
        {
            return true;
        }
    }
    false
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
    overlay.set_toolbar_resize_active(false);
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

fn toolbar_hidden_for_gesture(kind: GestureKind) -> bool {
    matches!(kind, GestureKind::Resize(_))
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
    if toolbar_hidden_for_gesture(kind) {
        overlay.set_toolbar_resize_active(true);
    }
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
    let started = Instant::now();
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
    trace_sampled_duration(
        "selection_gesture_update",
        started,
        &SELECTION_TIMING_SAMPLES,
    );
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
    let kind = state
        .active_kind
        .lock()
        .expect("active gesture lock")
        .take();
    if kind.is_some_and(toolbar_hidden_for_gesture) {
        overlay.set_toolbar_resize_active(false);
    }
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
    if kind.is_some_and(toolbar_hidden_for_gesture) {
        overlay.set_toolbar_resize_active(false);
    }
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
        overlay.set_size_menu_open(false);
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
    let started = Instant::now();
    let crop_guard = crop_slot.lock().expect("selected crop lock");
    let Some(crop) = crop_guard.as_ref() else {
        return Err("selected crop is no longer available");
    };
    let (annotations, can_undo, can_redo, can_clear_annotations) = {
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
    overlay.set_can_clear_annotations(can_clear_annotations);
    trace_sampled_duration("annotation_preview", started, &PREVIEW_TIMING_SAMPLES);
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
    let startup_started = Instant::now();
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    #[cfg(target_os = "windows")]
    if let Some(result) = platform::windows_updater::run_handoff_if_requested() {
        result.map_err(std::io::Error::other)?;
        return Ok(());
    }
    #[cfg(target_os = "windows")]
    if std::env::args_os().any(|argument| argument == "--test-notification") {
        let frame = CapturedFrame::new(8, 8, vec![48, 128, 224, 255].repeat(64))
            .map_err(std::io::Error::other)?;
        platform::show_screenshot_notification("Carambola test notification", 0, &frame)
            .map_err(std::io::Error::other)?;
        std::thread::sleep(std::time::Duration::from_secs(2));
        let history_count =
            platform::notification_center_entry_count().map_err(std::io::Error::other)?;
        if history_count == 0 {
            return Err(std::io::Error::other(
                "Windows accepted the test toast, but Notification Center has no Carambola entry",
            )
            .into());
        }
        std::thread::sleep(std::time::Duration::from_secs(8));
        return Ok(());
    }
    #[cfg(target_os = "windows")]
    let _single_instance =
        match platform::windows::SingleInstanceGuard::acquire().map_err(std::io::Error::other)? {
            Some(guard) => guard,
            None => {
                log::info!("Carambola is already running in this Windows session");
                return Ok(());
            }
        };
    platform::initialize().map_err(std::io::Error::other)?;
    #[cfg(target_os = "windows")]
    if let Err(error) = platform::initialize_notification_registration() {
        log::warn!("Could not register Windows notifications at startup: {error}");
    }
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
        use slint::winit_030::winit::platform::windows::WindowAttributesExtWindows;

        let backend_result = slint::BackendSelector::new()
            .backend_name("winit".into())
            .renderer_name("software".into())
            // Carambola has no file-drop workflow; avoid registering Winit's OLE drop target.
            .with_winit_window_attributes_hook(|attributes| attributes.with_drag_and_drop(false))
            .select();
        platform::trace_event(&format!(
            "backend.select.after success={}",
            backend_result.is_ok()
        ));
        backend_result?;
    }
    log::info!("Carambola capture app started");

    platform::trace_event("main.before_overlay_create");
    let overlay = OverlayPrototype::new().map_err(|error| {
        platform::trace_event(&format!("main.overlay_create.failed error={error}"));
        error
    })?;
    platform::trace_event("main.after_overlay_create");
    platform::trace_slint_native_handle("Overlay", "after_component_create", overlay.window());
    trace_overlay_state(&overlay, "main.after_overlay_create");
    platform::diagnose_cursor("overlay component created");

    let (mut initial_settings, mut settings_load_error, loaded_settings_path) =
        match AppSettings::load() {
            Ok((settings, path)) => (settings, None, Some(path)),
            Err(error) => {
                log::warn!("Could not load Carambola settings: {error}");
                (
                    AppSettings::default(),
                    Some(format!("Settings could not be loaded: {error}")),
                    settings::settings_file_path(),
                )
            }
        };
    if let Err(error) = initial_settings.capture_shortcut.validate() {
        log::warn!("Stored capture shortcut is invalid: {error}");
        settings_load_error = Some(format!(
            "The saved capture shortcut is unsupported ({error}). Record a supported key in Settings."
        ));
    }
    initial_settings.capture_shortcut = initial_settings.capture_shortcut.normalize_legacy_letter();
    #[cfg(target_os = "windows")]
    let initial_startup_enabled = {
        let mut registrar = platform::windows_startup::WindowsStartupRegistrar;
        let mut startup_error = registrar
            .set_enabled(initial_settings.launch_on_startup)
            .err()
            .map(|error| format!("Could not apply the Windows startup preference: {error}"));
        let actual_enabled = match registrar.is_enabled() {
            Ok(enabled) => enabled,
            Err(error) => {
                let message = format!("Could not read the Windows startup registration: {error}");
                startup_error = Some(match startup_error {
                    Some(previous) => format!("{previous}. {message}"),
                    None => message,
                });
                false
            }
        };
        if let Some(error) = startup_error {
            log::warn!("{error}");
            settings_load_error = Some(match settings_load_error.take() {
                Some(previous) => format!("{previous}. {error}"),
                None => error,
            });
        }
        actual_enabled
    };
    #[cfg(not(target_os = "windows"))]
    let initial_startup_enabled = false;
    let settings_path = Arc::new(loaded_settings_path);
    let application_settings = Arc::new(Mutex::new(initial_settings));
    let settings_window = SettingsPrototype::new().map_err(|error| {
        std::io::Error::other(format!("Could not create the Settings window: {error}"))
    })?;
    {
        let current = application_settings
            .lock()
            .expect("application settings lock");
        settings_window.set_save_directory(
            current
                .save_directory
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_default()
                .into(),
        );
        settings_window.set_capture_shortcut(current.capture_shortcut.display().into());
        settings_window.set_notifications_enabled(current.notifications_enabled);
    }
    settings_window.set_startup_supported(cfg!(target_os = "windows"));
    settings_window.set_startup_enabled(initial_startup_enabled);
    #[cfg(target_os = "windows")]
    settings_window.set_updater_supported(platform::windows_updater::is_installed_copy());
    if let Some(error) = settings_load_error {
        settings_window.set_settings_message(error.into());
    }
    let weak_settings_window = settings_window.as_weak();
    let settings_for_folder = application_settings.clone();
    let settings_path_for_folder = settings_path.clone();
    settings_window.on_choose_save_directory(move || {
        let weak_settings_window = weak_settings_window.clone();
        let initial = settings_for_folder
            .lock()
            .expect("application settings lock")
            .save_directory
            .clone();
        let result = platform::pick_save_directory(initial.as_deref()).and_then(|selection| {
            let Some(directory) = selection else {
                return Ok(None);
            };
            settings::validate_save_directory(&directory)?;
            let mut updated = settings_for_folder
                .lock()
                .map_err(|_| "Application settings lock is unavailable".to_string())?
                .clone();
            updated.save_directory = Some(directory.clone());
            let path = settings_path_for_folder
                .as_ref()
                .as_ref()
                .ok_or_else(|| "Carambola settings file path is unavailable".to_string())?;
            updated.save_to(path)?;
            *settings_for_folder
                .lock()
                .map_err(|_| "Application settings lock is unavailable".to_string())? = updated;
            Ok(Some(directory))
        });
        if let Some(settings_window) = weak_settings_window.upgrade() {
            match result {
                Ok(Some(directory)) => {
                    settings_window.set_save_directory(directory.display().to_string().into());
                    settings_window.set_settings_message("Default save folder updated.".into());
                }
                Ok(None) => {}
                Err(error) => settings_window.set_settings_message(error.into()),
            }
        }
    });
    let settings_for_notifications = application_settings.clone();
    let settings_path_for_notifications = settings_path.clone();
    let weak_settings_for_notifications = settings_window.as_weak();
    settings_window.on_notifications_toggled(move |enabled| {
        let result = (|| {
            let mut current = settings_for_notifications
                .lock()
                .map_err(|_| "Application settings lock is unavailable".to_string())?;
            let mut updated = current.clone();
            updated.notifications_enabled = enabled;
            let path = settings_path_for_notifications
                .as_ref()
                .as_ref()
                .ok_or_else(|| "Carambola settings file path is unavailable".to_string())?;
            updated.save_to(path)?;
            *current = updated;
            Ok::<(), String>(())
        })();
        if let Some(window) = weak_settings_for_notifications.upgrade() {
            match result {
                Ok(()) => {
                    window.set_notifications_enabled(enabled);
                    window.set_settings_message(if enabled {
                        "Screenshot notifications enabled.".into()
                    } else {
                        "Screenshot notifications disabled.".into()
                    });
                }
                Err(error) => window.set_settings_message(error.into()),
            }
        }
    });
    #[cfg(target_os = "windows")]
    {
        let settings_for_startup = application_settings.clone();
        let settings_path_for_startup = settings_path.clone();
        let weak_settings_for_startup = settings_window.as_weak();
        settings_window.on_startup_toggled(move |enabled| {
            let result = (|| {
                let mut current = settings_for_startup
                    .lock()
                    .map_err(|_| "Application settings lock is unavailable".to_string())?;
                let mut registrar = platform::windows_startup::WindowsStartupRegistrar;
                settings::change_startup_preference(
                    &mut current,
                    enabled,
                    &mut registrar,
                    |updated| {
                        let path =
                            settings_path_for_startup.as_ref().as_ref().ok_or_else(|| {
                                "Carambola settings file path is unavailable".to_string()
                            })?;
                        updated.save_to(path)
                    },
                )
            })();
            if let Some(window) = weak_settings_for_startup.upgrade() {
                match result {
                    Ok(()) => {
                        window.set_startup_enabled(enabled);
                        window.set_settings_message(if enabled {
                            "Carambola will start when you sign in.".into()
                        } else {
                            "Carambola will not start automatically.".into()
                        });
                    }
                    Err(error) => {
                        let actual_enabled =
                            platform::windows_startup::is_startup_enabled().unwrap_or(false);
                        window.set_startup_enabled(actual_enabled);
                        window.set_settings_message(
                            format!("Could not update the Windows startup setting: {error}").into(),
                        );
                    }
                }
            }
        });
    }
    #[cfg(target_os = "windows")]
    {
        let weak_settings_for_update = settings_window.as_weak();
        settings_window.on_check_for_updates(move || {
            let Some(window) = weak_settings_for_update.upgrade() else {
                return;
            };
            if window.get_update_checking() {
                return;
            }
            if !platform::windows_updater::is_installed_copy() {
                window.set_settings_message(
                    "Updates are available for the installed copy of Carambola.".into(),
                );
                return;
            }

            window.set_update_checking(true);
            window.set_settings_message("Checking for Carambola updates…".into());
            let weak_settings_for_update_result = weak_settings_for_update.clone();
            thread::spawn(move || {
                let result = platform::windows_updater::check_for_updates(env!("CARGO_PKG_VERSION"));
                if let Err(error) = &result {
                    log::warn!("Carambola update check failed: {error}");
                }
                let dispatch_result = slint::invoke_from_event_loop(move || {
                    let Some(window) = weak_settings_for_update_result.upgrade() else {
                        return;
                    };
                    window.set_update_checking(false);
                    match result {
                        Ok(Some(offer)) => {
                            let version = offer.version.to_string();
                            if !platform::windows_updater::confirm_update(&version) {
                                window.set_settings_message(
                                    format!("Carambola {version} is available; installation was cancelled.")
                                        .into(),
                                );
                                return;
                            }

                            window.set_update_checking(true);
                            window.set_settings_message(
                                format!("Downloading and verifying Carambola {version}…").into(),
                            );
                            let weak_settings_for_download = window.as_weak();
                            thread::spawn(move || {
                                let result = platform::windows_updater::download_and_verify(&offer)
                                    .and_then(|installer| {
                                        platform::windows_updater::start_handoff(&installer)
                                    });
                                match result {
                                    Ok(()) => {
                                        log::info!("Verified update is ready; closing Carambola for Setup");
                                        if let Err(error) = slint::invoke_from_event_loop(move || {
                                            if let Some(window) = weak_settings_for_download.upgrade() {
                                                window.set_update_checking(false);
                                                window.set_settings_message(
                                                    "Update verified. Carambola is closing before Setup starts.".into(),
                                                );
                                            }
                                            let _ = slint::quit_event_loop();
                                        }) {
                                            log::warn!("Could not finish the update handoff: {error}");
                                        }
                                    }
                                    Err(error) => {
                                        log::warn!("Carambola update download or handoff failed: {error}");
                                        let display_error = error.clone();
                                        if let Err(dispatch_error) = slint::invoke_from_event_loop(move || {
                                            if let Some(window) = weak_settings_for_download.upgrade() {
                                                window.set_update_checking(false);
                                                window.set_settings_message(
                                                    format!("Update failed: {display_error}").into(),
                                                );
                                            }
                                        }) {
                                            log::warn!("Could not show the update error in Settings: {dispatch_error}");
                                        }
                                    }
                                }
                            });
                        }
                        Ok(None) => window.set_settings_message(
                            "Carambola is up to date; no newer compatible Windows release was found."
                                .into(),
                        ),
                        Err(error) => window.set_settings_message(
                            format!("Could not check for updates: {error}").into(),
                        ),
                    }
                });
                if let Err(error) = dispatch_result {
                    log::warn!("Could not display the Carambola update check result: {error}");
                }
            });
        });
    }
    let settings_for_shortcut = application_settings.clone();
    let settings_path_for_shortcut = settings_path.clone();
    #[cfg(target_os = "windows")]
    let tray_service_slot: Arc<Mutex<Option<platform::tray::TrayService>>> =
        Arc::new(Mutex::new(None));
    #[cfg(target_os = "windows")]
    let tray_service_for_shortcut = tray_service_slot.clone();
    let weak_settings_for_shortcut = settings_window.as_weak();
    settings_window.on_shortcut_key_captured(move |text, control, shift, alt, meta| {
        let requested =
            core::hotkey::HotkeyConfig::from_recorded_key(text.as_str(), control, shift, alt, meta);
        let result = match requested {
            Err(error) => Err(error.to_string()),
            Ok(requested) => {
                #[cfg(target_os = "windows")]
                {
                    let mut settings_guard = settings_for_shortcut
                        .lock()
                        .expect("application settings lock");
                    let mut service_guard = tray_service_for_shortcut
                        .lock()
                        .expect("tray service slot lock");
                    match service_guard.as_mut() {
                        Some(service) => settings::change_shortcut(
                            &mut settings_guard,
                            requested,
                            service,
                            |updated| {
                                let path = settings_path_for_shortcut
                                    .as_ref()
                                    .as_ref()
                                    .ok_or_else(|| {
                                        "Carambola settings file path is unavailable".to_string()
                                    })?;
                                updated.save_to(path)
                            },
                        ),
                        None => Err("The tray hotkey service is unavailable".into()),
                    }
                }
                #[cfg(not(target_os = "windows"))]
                {
                    let _ = (
                        requested,
                        &settings_for_shortcut,
                        &settings_path_for_shortcut,
                    );
                    Err("Global shortcut changes are currently supported on Windows only".into())
                }
            }
        };
        if let Some(settings_window) = weak_settings_for_shortcut.upgrade() {
            settings_window.set_shortcut_recording(false);
            match result {
                Ok(()) => {
                    if let Ok(settings) = settings_for_shortcut.lock() {
                        settings_window
                            .set_capture_shortcut(settings.capture_shortcut.display().into());
                    }
                    settings_window.set_settings_message("Capture shortcut updated.".into());
                }
                Err(error) => settings_window.set_settings_message(error.into()),
            }
        }
    });
    let weak_settings_for_close = settings_window.as_weak();
    settings_window.on_close_settings(move || {
        if let Some(settings_window) = weak_settings_for_close.upgrade() {
            settings_window.set_shortcut_recording(false);
            let _ = settings_window.hide();
        }
    });
    let weak_settings_for_window_close = settings_window.as_weak();
    settings_window.window().on_close_requested(move || {
        if let Some(settings_window) = weak_settings_for_window_close.upgrade() {
            settings_window.set_shortcut_recording(false);
        }
        slint::CloseRequestResponse::HideWindow
    });

    let selection = SelectionState::new();
    let export_busy = Arc::new(AtomicBool::new(false));
    let clipboard_owner_hwnd = Arc::new(AtomicIsize::new(0));
    install_overlay_scale_factor_handler(&overlay, selection.clone());

    let weak_overlay = overlay.as_weak();
    let state_for_capture = selection.clone();
    let busy_for_capture = export_busy.clone();
    overlay.on_request_capture(move || {
        let capture_requested_at = Instant::now();
        platform::clear_window_action("new_capture_requested");
        platform::trace_event("capture.start.requested");
        platform::diagnose_cursor("capture requested");
        if busy_for_capture.load(Ordering::Acquire) {
            if let Some(overlay) = weak_overlay.upgrade() {
                set_status(&overlay, "Please wait for the current export to finish.");
            }
            return;
        }
        if state_for_capture.closing.load(Ordering::Acquire) {
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
        overlay.set_overlay_closing(false);
        overlay.set_control_held(false);
        overlay.set_fade_animation_enabled(false);
        // The captured image is installed before show. Start its fade with a
        // light tint instead of an opaque black frame on the first capture.
        overlay.set_fade_cover_color(slint::Color::from_argb_u8(64, 7, 16, 29));
        overlay.set_content_opacity(1.0);
        overlay.set_color_palette_open(false);
        overlay.set_size_menu_open(false);
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
        overlay.set_copy_in_progress(false);
        overlay.set_ocr_in_progress(false);
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

        let weak_overlay = overlay.as_weak();
        let state_for_result = state_for_capture.clone();
        thread::spawn(move || {
            let worker_started = Instant::now();
            platform::trace_event("capture.start.prepare_capture.begin");
            let prepared = platform::prepare_capture();
            platform::trace_event(&format!(
                "capture.start.prepare_capture.complete success={}",
                prepared.is_ok()
            ));
            platform::diagnose_cursor("capture prepared");
            platform::trace_event(&format!(
                "capture.capture_display.worker_begin display={requested_display_id:?}"
            ));
            let captured = prepared
                .map_err(|error| format!("Could not prepare the desktop for capture: {error}"))
                .and_then(|()| platform::capture_display(requested_display_id.as_deref()))
                .map(|frame| {
                    let pixels = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(
                        frame.pixels(), frame.width(), frame.height(),
                    );
                    (frame, pixels)
                });
            platform::trace_event(&format!(
                "capture.capture_display.worker_end display={requested_display_id:?} success={}",
                captured.is_ok()
            ));
            platform::trace_event(&format!(
                "perf.capture.worker elapsed_ms={} success={}",
                worker_started.elapsed().as_millis(),
                captured.is_ok()
            ));
            let session_for_dispatch = state_for_result.session.clone();
            let session_for_queue_error = state_for_result.session.clone();
            let dispatched = slint::invoke_from_event_loop(move || {
                let dispatch_started = Instant::now();
                platform::trace_event("capture.capture_result.dispatch_entered");
                let Some(overlay) = weak_overlay.upgrade() else {
                    finish_capture_session(&session_for_dispatch, "overlay_dropped_during_capture");
                    return;
                };
                let (frame, image_pixels) = match captured {
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
                overlay.set_captured_image(Image::from_rgba8(image_pixels));
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
                platform::trace_event(&format!(
                    "perf.capture.to_show elapsed_ms={} dispatch_ms={}",
                    capture_requested_at.elapsed().as_millis(),
                    dispatch_started.elapsed().as_millis()
                ));
                overlay.set_fade_animation_enabled(true);
                overlay.set_fade_cover_color(slint::Color::from_argb_u8(0, 0, 0, 0));
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
        if let Some(overlay) = weak_overlay.upgrade() {
            if let Err(error) =
                schedule_capture_close(&overlay, &state_for_cancel, "cancelled", false, None)
            {
                log::warn!("Could not schedule overlay fade-out: {error}");
            }
            platform::diagnose_cursor("escape after close");
        } else {
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
        }
        platform::finish_window_action(action_id, "cancel_teardown_complete");
    });

    let weak_overlay = overlay.as_weak();
    let state_for_down = selection.clone();
    overlay.on_pointer_down(move |x, y, width, height, control, meta| {
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
                let hit = match hit_test_selection(rect, x as f64, y as f64, 5.0, transform) {
                    Ok(hit) => hit,
                    Err(error) => {
                        log::warn!("Could not hit-test crop selection: {error}");
                        return;
                    }
                };
                let starts_crop_gesture = match hit {
                    SelectionHit::Outside | SelectionHit::Resize(_) => true,
                    SelectionHit::Move => crop_move_modifier(control, meta),
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
        let bounds = *state_for_move.bounds.lock().expect("selection bounds lock");
        let Some(bounds) = bounds else {
            return;
        };
        // A crop can be many megabytes; pointer movement needs only its
        // dimensions. Borrow it briefly instead of cloning every pixel.
        let mapped = {
            let crop = state_for_move.crop.lock().expect("selected crop lock");
            let Some(crop) = crop.as_ref() else {
                return;
            };
            annotation_point(x as f64, y as f64, bounds, crop)
        };
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
    let state_for_clear = selection.clone();
    overlay.on_clear_annotations(move || {
        if let Some(overlay) = weak_overlay.upgrade() {
            if state_for_clear.closing.load(Ordering::Acquire)
                || !overlay.get_annotation_mode()
                || overlay.get_text_entry_visible()
            {
                return;
            }
            let cleared = state_for_clear
                .editor
                .lock()
                .expect("annotation editor lock")
                .as_mut()
                .is_some_and(AnnotationEditor::clear_annotations);
            if cleared {
                update_preview_if_possible(
                    &overlay,
                    &state_for_clear.crop,
                    &state_for_clear.editor,
                );
                overlay.set_export_status("All annotations cleared. Undo is available.".into());
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
    overlay.on_annotation_size_level_selected(move |value| {
        let Some(overlay) = weak_overlay.upgrade() else {
            return;
        };
        let Some(level) = AnnotationSizeLevel::from_ui(value) else {
            return;
        };
        let size_level = {
            let mut guard = state_for_size
                .editor
                .lock()
                .expect("annotation editor lock");
            let Some(editor) = guard.as_mut() else {
                return;
            };
            editor.set_active_size_level(level);
            editor.active_size_level().as_ui()
        };
        overlay.set_annotation_size(size_level);
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

    let weak_overlay_for_ocr = overlay.as_weak();
    let state_for_ocr = selection.clone();
    let busy_for_ocr = export_busy.clone();
    let clipboard_owner_for_ocr = clipboard_owner_hwnd.clone();
    let settings_for_ocr = application_settings.clone();
    overlay.on_recognize_text(move || {
        let Some(overlay) = weak_overlay_for_ocr.upgrade() else {
            return;
        };
        if state_for_ocr.closing.load(Ordering::Acquire) {
            return;
        }
        if state_for_ocr
            .text_position
            .lock()
            .expect("pending text position lock")
            .is_some()
        {
            overlay.set_export_status(
                "Add or cancel the current text before recognizing text.".into(),
            );
            return;
        }
        let crop = match state_for_ocr
            .crop
            .lock()
            .expect("selected crop lock")
            .clone()
        {
            Some(crop) => crop,
            None => {
                overlay.set_export_status("There is no crop to recognize yet.".into());
                return;
            }
        };
        if busy_for_ocr
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            log::info!("Ignoring OCR request while another export or OCR operation is running");
            return;
        }
        let notifications_enabled = settings_for_ocr
            .lock()
            .expect("application settings lock")
            .notifications_enabled;
        let cancellation = Arc::new(AtomicBool::new(false));
        *state_for_ocr
            .ocr_cancellation
            .lock()
            .expect("OCR cancellation lock") = Some(cancellation.clone());
        overlay.set_ocr_in_progress(true);
        overlay.set_export_busy(true);
        overlay.set_export_status("".into());
        let owner_hwnd = clipboard_owner_for_ocr.load(Ordering::Acquire);
        let weak_overlay = overlay.as_weak();
        let state_for_worker = state_for_ocr.clone();
        let state_for_dispatch = state_for_ocr.clone();
        let cancellation_for_worker = cancellation.clone();
        let cancellation_for_dispatch = cancellation.clone();
        let busy_for_worker = busy_for_ocr.clone();
        let busy_for_dispatch = busy_for_ocr.clone();
        thread::spawn(move || {
            let recognized = normalize_result(platform::recognize_text(&crop));
            if cancellation_for_worker.load(Ordering::Acquire)
                || !state_for_worker.session.is_active()
            {
                busy_for_worker.store(false, Ordering::Release);
                return;
            }
            let completion = complete_ocr(recognized, |text| {
                platform::copy_text_to_clipboard(text, owner_hwnd)
            });
            if cancellation_for_worker.load(Ordering::Acquire)
                || !state_for_worker.session.is_active()
            {
                busy_for_worker.store(false, Ordering::Release);
                return;
            }
            let message = completion.notification_message();
            let close_reason = match &completion {
                OcrCompletion::Copied { .. } => {
                    log::info!("{message}");
                    "ocr_copied"
                }
                OcrCompletion::Empty => {
                    log::info!("{message}");
                    "ocr_empty"
                }
                OcrCompletion::Error(error) => {
                    log::error!("OCR failed: {error}");
                    "ocr_failed"
                }
            };
            let notification = PendingScreenshotNotification {
                message,
                preview: crop,
                owner_hwnd,
                enabled: notifications_enabled,
            };
            let dispatched = slint::invoke_from_event_loop(move || {
                if cancellation_for_dispatch.load(Ordering::Acquire)
                    || !state_for_dispatch.session.is_active()
                {
                    busy_for_dispatch.store(false, Ordering::Release);
                    return;
                }
                if let Some(overlay) = weak_overlay.upgrade() {
                    if let Err(error) = schedule_capture_close(
                        &overlay,
                        &state_for_dispatch,
                        close_reason,
                        true,
                        Some(notification),
                    ) {
                        log::error!("Could not close the overlay after OCR: {error}");
                        overlay.set_ocr_in_progress(false);
                        overlay.set_export_busy(false);
                        overlay.set_export_status("".into());
                        overlay.set_overlay_closing(false);
                        state_for_dispatch.closing.store(false, Ordering::Release);
                    }
                }
                busy_for_dispatch.store(false, Ordering::Release);
            });
            if let Err(error) = dispatched {
                cancellation_for_worker.store(true, Ordering::Release);
                busy_for_worker.store(false, Ordering::Release);
                log::error!("Could not return OCR results to the UI thread: {error}");
            }
        });
    });

    let weak_overlay = overlay.as_weak();
    let state_for_save = selection.clone();
    let busy_for_save = export_busy.clone();
    let notification_owner_for_save = clipboard_owner_hwnd.clone();
    let settings_for_save = application_settings.clone();
    overlay.on_save_png(move || {
        platform::trace_event("export.save_as.callback");
        let Some(overlay) = weak_overlay.upgrade() else {
            return;
        };
        request_save(
            &overlay,
            &state_for_save,
            Arc::clone(&busy_for_save),
            Arc::clone(&notification_owner_for_save),
            Arc::clone(&settings_for_save),
            true,
        );
    });

    let weak_overlay = overlay.as_weak();
    let state_for_default_save = selection.clone();
    let busy_for_default_save = export_busy.clone();
    let notification_owner_for_default_save = clipboard_owner_hwnd.clone();
    let settings_for_default_save = application_settings.clone();
    overlay.on_save_default_png(move || {
        platform::trace_event("export.save_default.callback");
        let Some(overlay) = weak_overlay.upgrade() else {
            return;
        };
        request_save(
            &overlay,
            &state_for_default_save,
            Arc::clone(&busy_for_default_save),
            Arc::clone(&notification_owner_for_default_save),
            Arc::clone(&settings_for_default_save),
            false,
        );
    });

    let weak_overlay = overlay.as_weak();
    let state_for_copy = selection.clone();
    let state_for_copy_completion = selection.clone();
    let busy_for_copy = export_busy.clone();
    let clipboard_owner_for_copy = clipboard_owner_hwnd.clone();
    let settings_for_copy = application_settings.clone();
    overlay.on_copy_image(move || {
        platform::trace_event("export.copy_image.callback");
        let Some(overlay) = weak_overlay.upgrade() else {
            return;
        };
        let owner_hwnd = clipboard_owner_for_copy.load(Ordering::Acquire);
        let notifications_enabled = settings_for_copy
            .lock()
            .expect("application settings lock")
            .notifications_enabled;
        if state_for_copy.closing.load(Ordering::Acquire) {
            return;
        }
        if state_for_copy
            .text_position
            .lock()
            .expect("pending text position lock")
            .is_some()
        {
            report_copy_failure("Add or cancel the current text before copying.", owner_hwnd);
            return;
        }
        let (crop, annotations) =
            match annotation_snapshot(&state_for_copy.crop, &state_for_copy.editor) {
                Ok(snapshot) => snapshot,
                Err(error) => {
                    report_copy_failure(&error, owner_hwnd);
                    return;
                }
            };
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
                Ok(ExportSuccess {
                    message: "Image copied to clipboard".into(),
                    preview: rendered,
                })
            },
            {
                let state = state_for_copy_completion.clone();
                move |overlay, success: ExportSuccess, message| {
                    close_capture_after_export(
                        overlay,
                        &state,
                        "copied",
                        PendingScreenshotNotification {
                            message: message.to_owned(),
                            preview: success.preview,
                            owner_hwnd,
                            enabled: notifications_enabled,
                        },
                    )
                }
            },
        );
    });

    let weak_overlay = overlay.as_weak();
    overlay.on_overlay_shortcut_requested(move |key, control, shift, text_editor_active| {
        let Some(overlay) = weak_overlay.upgrade() else {
            return;
        };
        match overlay_shortcut(key.as_str(), control, shift, text_editor_active) {
            Some(OverlayShortcut::Undo) => overlay.invoke_undo_annotation(),
            Some(OverlayShortcut::Redo) => overlay.invoke_redo_annotation(),
            Some(OverlayShortcut::Copy) => overlay.invoke_copy_image(),
            Some(OverlayShortcut::SaveDefault) => overlay.invoke_save_default_png(),
            Some(OverlayShortcut::SaveAs) => overlay.invoke_save_png(),
            None => {}
        }
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
    {
        let weak_overlay = overlay.as_weak();
        let weak_settings = settings_window.as_weak();
        let settings_for_tray = application_settings.clone();
        let startup_hotkey = application_settings
            .lock()
            .expect("application settings lock")
            .capture_shortcut;
        platform::trace_event("main.tray_start.before");
        let service = platform::tray::TrayService::start(
            move |command: TrayCommand| {
                let weak_overlay = weak_overlay.clone();
                let weak_settings = weak_settings.clone();
                let settings_for_tray = settings_for_tray.clone();
                platform::trace_event(&format!(
                    "tray.command.dispatch_to_slint command={command:?}"
                ));
                if let Err(error) = slint::invoke_from_event_loop(move || {
                    platform::trace_event(&format!(
                        "tray.command.slint_callback command={command:?}"
                    ));
                    match command.action() {
                        AppAction::StartCapture => {
                            platform::trace_event("tray.action.start_capture");
                            if let Some(overlay) = weak_overlay.upgrade() {
                                overlay.invoke_request_capture();
                            }
                        }
                        AppAction::OpenSettings => {
                            platform::trace_event("tray.action.open_settings");
                            if let Some(settings_window) = weak_settings.upgrade() {
                                if let Ok(settings) = settings_for_tray.lock() {
                                    settings_window.set_save_directory(
                                        settings
                                            .save_directory
                                            .as_ref()
                                            .map(|path| path.display().to_string())
                                            .unwrap_or_default()
                                            .into(),
                                    );
                                    settings_window.set_capture_shortcut(
                                        settings.capture_shortcut.display().into(),
                                    );
                                    settings_window.set_notifications_enabled(settings.notifications_enabled);
                                }
                                match platform::windows_startup::is_startup_enabled() {
                                    Ok(enabled) => settings_window.set_startup_enabled(enabled),
                                    Err(error) => {
                                        settings_window.set_startup_enabled(false);
                                        settings_window.set_settings_message(
                                            format!("Could not read the Windows startup registration: {error}").into(),
                                        );
                                    }
                                }
                                settings_window.set_shortcut_recording(false);
                                let background_ready = match platform::windows::prepare_settings_window(settings_window.window()) {
                                    Ok(()) => true,
                                    Err(error) => {
                                        log::warn!("Settings native window was not ready before show: {error}");
                                        false
                                    }
                                };
                                settings_window.window().set_minimized(false);
                                if let Err(error) = settings_window.show() {
                                    settings_window.set_settings_message(
                                        format!("Could not open Settings: {error}").into(),
                                    );
                                } else {
                                    let activation_ready = match platform::windows::activate_settings_window(settings_window.window()) {
                                        Ok(()) => true,
                                        Err(error) => {
                                            log::warn!("Could not activate Settings immediately: {error}");
                                            false
                                        }
                                    };
                                    if !background_ready || !activation_ready {
                                    let weak_settings = settings_window.as_weak();
                                    slint::Timer::single_shot(std::time::Duration::ZERO, move || {
                                        if let Some(settings_window) = weak_settings.upgrade() {
                                            if !background_ready {
                                                if let Err(error) = platform::windows::prepare_settings_window(settings_window.window()) {
                                                    log::warn!("Could not initialize the Settings background: {error}");
                                                }
                                            }
                                            if !activation_ready {
                                                if let Err(error) = platform::windows::activate_settings_window(settings_window.window()) {
                                                    log::warn!("Could not bring Settings to the foreground: {error}");
                                                    settings_window.set_settings_message(error.into());
                                                }
                                            }
                                        }
                                    });
                                    }
                                }
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
            },
            startup_hotkey,
        )
        .map_err(std::io::Error::other)?;
        clipboard_owner_hwnd.store(service.native_window_handle(), Ordering::Release);
        let shortcut_label = startup_hotkey.display();
        match service.hotkey_state() {
            core::hotkey::RegistrationState::Registered => {
                log::info!("Tray ready; {shortcut_label} starts a capture")
            }
            core::hotkey::RegistrationState::Conflict => {
                log::warn!("Tray ready, but {shortcut_label} is registered by another application")
            }
            core::hotkey::RegistrationState::Failed(code) => log::warn!(
                "Tray ready, but {shortcut_label} registration failed with Windows error {code}"
            ),
        }
        platform::trace_event(&format!(
            "main.tray_start.after hwnd=0x{:X}",
            service.native_window_handle() as usize
        ));
        *tray_service_slot.lock().expect("tray service slot lock") = Some(service);
    }

    platform::trace_event("event_loop.first_callback.queue.before");
    let weak_overlay_startup = overlay.as_weak();
    let first_callback_result = slint::invoke_from_event_loop(move || {
        platform::trace_event("event_loop.initialized");
        platform::trace_event(&format!(
            "perf.startup.to_event_loop elapsed_ms={}",
            startup_started.elapsed().as_millis()
        ));
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
    platform::trace_event("main.after_event_loop");
    platform::trace_event("main.before_overlay_component_drop");
    Ok(())
}

#[cfg(test)]
mod toolbar_layout_tests {
    use super::*;
    use crate::core::selection::ResizeHandle;

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
    fn toolbar_hides_only_while_resizing_a_selection() {
        assert!(!toolbar_hidden_for_gesture(GestureKind::Create));
        assert!(!toolbar_hidden_for_gesture(GestureKind::Move));
        assert!(toolbar_hidden_for_gesture(GestureKind::Resize(
            ResizeHandle::BottomRight
        )));
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
        assert_eq!(layout.y, 514.0);
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
        assert_eq!((layout.x, layout.y), (226.0, 0.0));
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
            assert!((0.0..=334.0).contains(&x));
            assert!((0.0..=562.0).contains(&y));
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
    fn toolbar_design_dimensions_match_the_slint_toolbar_bounds() {
        assert_eq!(toolbar_scale_for_viewport(viewport(466.0, 38.0)), 1.0);
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
    fn crop_move_requires_control_without_changing_other_pointer_gestures() {
        #[cfg(not(target_os = "macos"))]
        {
            assert!(crop_move_modifier(true, false));
            assert!(!crop_move_modifier(false, true));
            assert!(!crop_move_modifier(false, false));
        }
        #[cfg(target_os = "macos")]
        {
            assert!(crop_move_modifier(false, true));
            assert!(!crop_move_modifier(true, false));
            assert!(!crop_move_modifier(false, false));
        }
    }

    #[test]
    fn text_editor_grows_for_multiline_input_and_caps_its_height() {
        assert_eq!(text_editor_visual_line_count("one line"), 1);
        assert_eq!(text_editor_visual_line_count("one\ntwo\nthree"), 3);
        assert_eq!(text_editor_visual_line_count(&"x\n".repeat(12)), 6);
    }

    #[test]
    fn deferred_text_focus_only_runs_for_an_active_editor() {
        assert!(should_focus_text_entry(true, false));
        assert!(!should_focus_text_entry(false, false));
        assert!(!should_focus_text_entry(true, true));
        assert!(!should_focus_text_entry(false, true));
    }
}
