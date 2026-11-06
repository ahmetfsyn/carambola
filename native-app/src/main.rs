#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod core;
mod platform;

mod settings_ui {
    include!(concat!(env!("OUT_DIR"), "/windows.rs"));
}
mod overlay_ui {
    include!(concat!(env!("OUT_DIR"), "/overlay.rs"));
}

use core::{
    annotation_render,
    annotations::{
        Annotation, AnnotationEditor, AnnotationTool, ImagePoint, StrokeStyle, ViewportToImage,
    },
    display::DisplayMetadata,
    frame::{CapturedFrame, PixelRect},
    geometry::{FrameViewport, ViewportToFrame},
    lifecycle::{
        overlay_close_policy, settings_close_policy, AppAction, ClosePolicy, SettingsRestoreAction,
        SettingsVisibility, TrayCommand,
    },
    selection::{LogicalRect, Selection},
    session::CaptureSession,
};
use overlay_ui::OverlayPrototype;
use settings_ui::SettingsWindow;
use slint::winit_030::{winit, EventResult, WinitWindowAccessor};
use slint::{ComponentHandle, Image, ModelRc, Rgba8Pixel, SharedPixelBuffer, VecModel};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU8, Ordering},
        Arc, Mutex,
    },
    thread,
};

fn set_status(settings: &SettingsWindow, message: impl Into<slint::SharedString>) {
    settings.set_status(message.into());
}

fn finish_capture_session(session: &CaptureSession, reason: &str) {
    platform::trace_event(&format!("capture.session.finish reason={reason:?}"));
    session.finish();
}

fn trace_settings_state(settings: &SettingsWindow, operation: &str) {
    platform::trace_component_state(
        "Settings",
        operation,
        settings.window().is_visible(),
        settings.window().is_minimized(),
    );
}

fn trace_overlay_state(overlay: &OverlayPrototype, operation: &str) {
    platform::trace_component_state(
        "Overlay",
        operation,
        overlay.window().is_visible(),
        overlay.window().is_minimized(),
    );
}

fn settings_visibility(settings: &SettingsWindow) -> SettingsVisibility {
    SettingsVisibility::from_window_state(
        settings.window().is_visible(),
        settings.window().is_minimized(),
    )
}

fn restore_capture_settings(settings: Option<&SettingsWindow>, visibility: SettingsVisibility) {
    platform::trace_event(&format!(
        "capture.restore.begin saved_visibility={visibility:?}"
    ));
    if let Err(error) = platform::reset_capture_affinity() {
        log::warn!("Could not reset settings-window capture affinity: {error}");
    }

    let Some(settings) = settings else {
        platform::trace_event("capture.restore.settings_component_unavailable");
        return;
    };

    match visibility.restore_action() {
        SettingsRestoreAction::Hide => {
            trace_settings_state(settings, "capture.restore.hide.before");
            if let Err(error) = settings.hide() {
                log::warn!("Could not keep the settings window hidden: {error}");
            }
            trace_settings_state(settings, "capture.restore.hide.after");
        }
        SettingsRestoreAction::Show => {
            platform::trace_event("capture.restore.settings_show_minimized_false");
            trace_settings_state(settings, "capture.restore.unminimize.before");
            settings.window().set_minimized(false);
            trace_settings_state(settings, "capture.restore.unminimize.after");
            trace_settings_state(settings, "capture.restore.show.before");
            if let Err(error) = settings.show() {
                log::warn!("Could not show the settings window after capture: {error}");
                return;
            }
            trace_settings_state(settings, "capture.restore.show.after");
            if let Err(error) = platform::activate_settings() {
                log::warn!("Could not activate the settings window after capture: {error}");
            }
        }
        SettingsRestoreAction::ShowMinimized => {
            trace_settings_state(settings, "capture.restore.show_minimized.before");
            if let Err(error) = settings.show() {
                log::warn!("Could not restore the minimized settings window: {error}");
                return;
            }
            trace_settings_state(settings, "capture.restore.show_minimized.show_after");
            platform::trace_event("capture.restore.settings_set_minimized_true");
            settings.window().set_minimized(true);
            trace_settings_state(settings, "capture.restore.show_minimized.after");
        }
    }
    platform::trace_event("capture.restore.end");
}

fn close_request_response(policy: ClosePolicy) -> slint::CloseRequestResponse {
    match policy {
        ClosePolicy::Hide => slint::CloseRequestResponse::HideWindow,
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
    settings: slint::Weak<SettingsWindow>,
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
            if let Some(settings) = settings.upgrade() {
                set_status(&settings, message);
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

fn install_overlay_scale_factor_handler(
    overlay: &OverlayPrototype,
    viewport_slot: Arc<Mutex<Option<FrameViewport>>>,
    selection_slot: Arc<Mutex<Selection>>,
    selection_pixels_slot: Arc<Mutex<Option<PixelRect>>>,
    selection_bounds_slot: Arc<Mutex<Option<LogicalRect>>>,
    crop_slot: Arc<Mutex<Option<CapturedFrame>>>,
    text_position_slot: Arc<Mutex<Option<ImagePoint>>>,
) {
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
        selection_slot.lock().expect("selection lock").cancel();

        let synchronized = {
            let mut viewport_guard = viewport_slot.lock().expect("frame viewport lock");
            let Some(viewport) = viewport_guard.as_mut() else {
                platform::trace_event("overlay.scale_factor_changed.ignored no_captured_frame");
                return EventResult::Propagate;
            };
            match viewport.synchronize_scale_factor(effective_scale_factor) {
                Ok(logical_size) => Ok((*viewport, logical_size)),
                Err(error) => {
                    *viewport_guard = None;
                    Err(error)
                }
            }
        };

        let (viewport, logical_size) = match synchronized {
            Ok(viewport) => viewport,
            Err(error) => {
                log::warn!("Rejected Overlay scale-factor change: {error}");
                platform::trace_event(&format!(
                    "overlay.scale_factor_changed.invalid error={error}"
                ));
                return EventResult::Propagate;
            }
        };

        let selected_pixels = *selection_pixels_slot
            .lock()
            .expect("selection pixel rectangle lock");
        let logical_selection = selected_pixels
            .map(|pixels| viewport.project_pixels(pixels))
            .transpose();
        let logical_selection = match logical_selection {
            Ok(selection) => selection,
            Err(error) => {
                *viewport_slot.lock().expect("frame viewport lock") = None;
                *selection_bounds_slot.lock().expect("selection bounds lock") = None;
                log::warn!("Rejected Overlay selection geometry after DPI change: {error}");
                platform::trace_event(&format!(
                    "overlay.scale_factor_changed.selection_invalid error={error}"
                ));
                return EventResult::Propagate;
            }
        };
        *selection_bounds_slot.lock().expect("selection bounds lock") = logical_selection;

        let Some(overlay) = weak_overlay.upgrade() else {
            return EventResult::Propagate;
        };
        overlay.set_frame_viewport_width(logical_size.width as f32);
        overlay.set_frame_viewport_height(logical_size.height as f32);
        if let Some(rect) = logical_selection {
            overlay.set_selection_x(rect.x as f32);
            overlay.set_selection_y(rect.y as f32);
            overlay.set_selection_width(rect.width as f32);
            overlay.set_selection_height(rect.height as f32);
        } else {
            clear_selection_preview(&overlay);
        }

        if let (Some(point), Some(bounds), Some(crop)) = (
            *text_position_slot.lock().expect("text position lock"),
            logical_selection,
            crop_slot
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

fn contains(bounds: LogicalRect, x: f64, y: f64) -> bool {
    x >= bounds.x && y >= bounds.y && x <= bounds.x + bounds.width && y <= bounds.y + bounds.height
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
    platform::trace_event("backend.select.before");
    // The default FemtoVG/OpenGL path leaves the native cursor physically
    // invisible over the fullscreen overlay on the affected Windows display.
    // Slint's software renderer preserves the Win32 cursor there.
    #[cfg(target_os = "windows")]
    let backend_result = slint::BackendSelector::new()
        .backend_name("winit".into())
        .renderer_name("software".into())
        .select();
    #[cfg(target_os = "windows")]
    platform::trace_event(&format!(
        "backend.select.after success={}",
        backend_result.is_ok()
    ));
    #[cfg(target_os = "windows")]
    backend_result?;
    log::info!("Native capture prototype started");

    #[cfg(target_os = "windows")]
    let displays = platform::enumerate_displays().map_err(std::io::Error::other)?;
    #[cfg(not(target_os = "windows"))]
    let displays = Vec::<DisplayMetadata>::new();
    let default_display_index = displays
        .iter()
        .position(|display| display.is_primary)
        .unwrap_or(0);
    let display_options = displays
        .iter()
        .map(|display| display.label().into())
        .collect::<Vec<slint::SharedString>>();
    let displays = Arc::new(displays);

    platform::trace_event("main.before_settings_create");

    let settings = SettingsWindow::new().map_err(|error| {
        platform::trace_event(&format!("main.settings_create.failed error={error}"));
        error
    })?;
    settings.set_display_options(ModelRc::new(VecModel::from(display_options)));
    settings.set_selected_display_index(default_display_index as i32);
    platform::trace_event("main.after_settings_create");
    platform::trace_slint_native_handle("Settings", "after_component_create", settings.window());
    let overlay = OverlayPrototype::new().map_err(|error| {
        platform::trace_event(&format!("main.overlay_create.failed error={error}"));
        error
    })?;
    platform::trace_event("main.after_overlay_create");
    platform::trace_slint_native_handle("Overlay", "after_component_create", overlay.window());
    trace_settings_state(&settings, "main.after_windows_created");
    trace_overlay_state(&overlay, "main.after_windows_created");
    platform::diagnose_cursor("windows created");
    let captured_frame = Arc::new(Mutex::new(None::<CapturedFrame>));
    let selected_crop = Arc::new(Mutex::new(None::<CapturedFrame>));
    let selection_bounds = Arc::new(Mutex::new(None::<LogicalRect>));
    let selection_pixels = Arc::new(Mutex::new(None::<PixelRect>));
    let annotation_editor = Arc::new(Mutex::new(None::<AnnotationEditor>));
    let pending_text_position = Arc::new(Mutex::new(None::<ImagePoint>));
    let selection = Arc::new(Mutex::new(Selection::default()));
    let frame_viewport = Arc::new(Mutex::new(None::<FrameViewport>));
    let capture_session = Arc::new(CaptureSession::default());
    let export_busy = Arc::new(AtomicBool::new(false));
    let selected_display_id = Arc::new(Mutex::new(None::<String>));

    install_overlay_scale_factor_handler(
        &overlay,
        frame_viewport.clone(),
        selection.clone(),
        selection_pixels.clone(),
        selection_bounds.clone(),
        selected_crop.clone(),
        pending_text_position.clone(),
    );

    {
        let displays = displays.clone();
        let selected_display_id = selected_display_id.clone();
        settings.on_display_selected(move |index| {
            let display_id = usize::try_from(index)
                .ok()
                .and_then(|index| displays.get(index))
                .map(|display| display.id.clone());
            if let Some(display_id) = display_id {
                *selected_display_id.lock().expect("selected display lock") = Some(display_id);
            }
        });
    }

    let weak_settings = settings.as_weak();
    let weak_overlay = overlay.as_weak();
    let frame_slot = captured_frame.clone();
    let crop_slot = selected_crop.clone();
    let bounds_slot = selection_bounds.clone();
    let selection_pixels_slot = selection_pixels.clone();
    let editor_slot = annotation_editor.clone();
    let text_position_slot = pending_text_position.clone();
    let selection_slot = selection.clone();
    let frame_viewport_slot = frame_viewport.clone();
    let active_flag = capture_session.clone();
    let export_busy_flag = export_busy.clone();
    let selected_display_slot = selected_display_id.clone();
    let displays_for_capture = displays.clone();
    let capture_restore_state = Arc::new(AtomicU8::new(SettingsVisibility::Hidden as u8));
    let restore_state_slot = capture_restore_state.clone();
    settings.on_open_overlay(move || {
        platform::clear_window_action("new_capture_requested");
        platform::trace_event("capture.start.requested");
        if let Some(settings) = weak_settings.upgrade() {
            trace_settings_state(&settings, "capture.start.requested");
        }
        log::info!("Capture requested");
        platform::diagnose_cursor("capture requested");
        if export_busy_flag.load(Ordering::Acquire) {
            if let Some(settings) = weak_settings.upgrade() {
                set_status(&settings, "Please wait for the current export to finish.");
            }
            return;
        }
        if !active_flag.begin_capture() {
            platform::trace_event("capture.start.rejected_session_already_active");
            return;
        }
        platform::trace_event("capture.session.transition=Capturing");
        let return_visibility = weak_settings
            .upgrade()
            .map(|settings| settings_visibility(&settings))
            .unwrap_or(SettingsVisibility::Hidden);
        restore_state_slot.store(return_visibility as u8, Ordering::Release);
        platform::trace_event(&format!(
            "capture.start.accepted saved_visibility={return_visibility:?}"
        ));
        frame_slot.lock().expect("capture frame lock").take();
        crop_slot.lock().expect("selected crop lock").take();
        *bounds_slot.lock().expect("selection bounds lock") = None;
        *selection_pixels_slot
            .lock()
            .expect("selection pixel rectangle lock") = None;
        *frame_viewport_slot
            .lock()
            .expect("frame viewport lock") = None;
        *editor_slot.lock().expect("annotation editor lock") = None;
        *text_position_slot.lock().expect("text position lock") = None;
        *selection_slot.lock().expect("selection lock") = Selection::default();
        let requested_display = selected_display_slot
            .lock()
            .expect("selected display lock")
            .clone();
        let display_label = requested_display
            .as_deref()
            .and_then(|id| displays_for_capture.iter().find(|display| display.id == id))
            .or_else(|| displays_for_capture.iter().find(|display| display.is_primary))
            .map(DisplayMetadata::label)
            .unwrap_or_else(|| "selected display".to_owned());
        if let Some(overlay) = weak_overlay.upgrade() {
            overlay.set_annotation_mode(false);
            overlay.set_annotation_image(Image::default());
            overlay.set_text_entry_visible(false);
            overlay.set_text_entry_value(String::new().into());
            overlay.set_can_undo(false);
            overlay.set_can_redo(false);
            overlay.set_export_busy(false);
            overlay.set_export_status(String::new().into());
            overlay.set_selection_x(0.0);
            overlay.set_selection_y(0.0);
            overlay.set_selection_width(0.0);
            overlay.set_selection_height(0.0);
        }
        if let Some(settings) = weak_settings.upgrade() {
            set_status(&settings, format!("Capturing {display_label}…"));
        }

        let weak_settings = weak_settings.clone();
        let weak_overlay = weak_overlay.clone();
        let frame_slot = frame_slot.clone();
        let selection_slot = selection_slot.clone();
        let frame_viewport_slot = frame_viewport_slot.clone();
        let active_flag = active_flag.clone();
        let restore_state_slot = restore_state_slot.clone();
        let requested_display = requested_display.clone();
        if weak_overlay.upgrade().is_none() {
            finish_capture_session(&active_flag, "overlay_unavailable_before_capture");
            return;
        }
        let Some(settings) = weak_settings.upgrade() else {
            finish_capture_session(&active_flag, "settings_unavailable_before_capture");
            return;
        };
        trace_settings_state(&settings, "capture.start.hide_settings.before");
        platform::trace_event("settings.hide.capture_start.begin");
        if let Err(error) = settings.hide() {
            log::error!("Could not hide the settings window through Slint: {error}");
            set_status(
                &settings,
                format!("Could not prepare the desktop for capture: {error}"),
            );
            restore_capture_settings(Some(&settings), return_visibility);
            finish_capture_session(&active_flag, "settings_hide_failed");
            return;
        }
        platform::trace_event("settings.hide.capture_start.end");
        trace_settings_state(&settings, "capture.start.hide_settings.after");
        platform::trace_event("capture.start.prepare_capture.begin");
        if let Err(error) = platform::prepare_capture() {
            log::error!("Could not synchronize hidden settings window before capture: {error}");
            set_status(
                &settings,
                format!("Could not prepare the desktop for capture: {error}"),
            );
            restore_capture_settings(Some(&settings), return_visibility);
            finish_capture_session(&active_flag, "prepare_capture_failed");
            return;
        }
        platform::trace_event("capture.start.prepare_capture.complete");
        platform::diagnose_cursor("settings hidden");

        // Capture before showing the transparent overlay so GDI cannot include
        // the overlay itself in the desktop pixels. Capture work stays off the
        // UI thread; the frozen frame is shown after the worker completes.
        let bounds_for_result = bounds_slot.clone();
        let selection_pixels_for_result = selection_pixels_slot.clone();
        std::thread::spawn(move || {
            platform::trace_event(&format!(
                "capture.capture_display.worker_begin requested_id={requested_display:?}"
            ));
            let captured = match requested_display.as_deref() {
                Some(display_id) => platform::capture_display(Some(display_id)),
                None => platform::capture_primary(),
            };
            platform::trace_event(&format!(
                "capture.capture_display.worker_end success={}",
                captured.is_ok()
            ));
            log::info!(
                "Selected-display capture completed: success={}",
                captured.is_ok()
            );
            let session_for_dispatch = active_flag.clone();
            let return_visibility =
                SettingsVisibility::from_code(restore_state_slot.load(Ordering::Acquire));
            platform::trace_event(&format!(
                "capture.capture_result.dispatch_queued saved_visibility={return_visibility:?}"
            ));
            let dispatched = slint::invoke_from_event_loop(move || {
                platform::trace_event("capture.capture_result.dispatch_entered");
                let Some(settings) = weak_settings.upgrade() else {
                    restore_capture_settings(None, return_visibility);
                    finish_capture_session(&active_flag, "settings_dropped_during_capture");
                    return;
                };
                let Some(overlay) = weak_overlay.upgrade() else {
                    restore_capture_settings(Some(&settings), return_visibility);
                    finish_capture_session(&active_flag, "overlay_dropped_during_capture");
                    return;
                };
                match captured {
                    Ok(frame) => {
                        let Some(display) = frame.display_metadata().cloned() else {
                            set_status(&settings, "The captured frame has no display metadata.");
                            restore_capture_settings(Some(&settings), return_visibility);
                            finish_capture_session(&active_flag, "capture_metadata_missing");
                            return;
                        };
                        let current_displays = match platform::enumerate_displays() {
                            Ok(displays) => displays,
                            Err(error) => {
                                set_status(
                                    &settings,
                                    format!("Could not verify the captured display: {error}"),
                                );
                                restore_capture_settings(Some(&settings), return_visibility);
                                finish_capture_session(&active_flag, "display_verification_failed");
                                return;
                            }
                        };
                        if !current_displays.iter().any(|current| current == &display) {
                            set_status(
                                &settings,
                                format!("Display {} changed or disconnected before the overlay opened.", display.id),
                            );
                            restore_capture_settings(Some(&settings), return_visibility);
                            finish_capture_session(&active_flag, "display_changed_before_overlay");
                            return;
                        }

                        let (Some(frame_origin), Some(frame_bottom_right)) = (
                            frame.desktop_origin(),
                            frame.local_to_desktop(frame.width(), frame.height()),
                        ) else {
                            set_status(&settings, "The captured frame has invalid desktop bounds.");
                            restore_capture_settings(Some(&settings), return_visibility);
                            finish_capture_session(&active_flag, "frame_desktop_bounds_invalid");
                            return;
                        };
                        if frame_bottom_right.x != display.bounds.right
                            || frame_bottom_right.y != display.bounds.bottom
                        {
                            set_status(&settings, "The captured frame dimensions do not match the selected display.");
                            restore_capture_settings(Some(&settings), return_visibility);
                            finish_capture_session(&active_flag, "frame_display_bounds_mismatch");
                            return;
                        }

                        let scale_factor = display.scale_factor() as f32;
                        if scale_factor <= 0.0 || !scale_factor.is_finite() {
                            set_status(
                                &settings,
                                "The selected display reported an invalid scale factor.",
                            );
                            restore_capture_settings(Some(&settings), return_visibility);
                            finish_capture_session(&active_flag, "invalid_display_scale");
                            return;
                        }
                        let frame_viewport = match FrameViewport::new(
                            frame.width(),
                            frame.height(),
                            f64::from(scale_factor),
                        ) {
                            Ok(viewport) => viewport,
                            Err(error) => {
                                set_status(
                                    &settings,
                                    format!("The selected display has invalid Overlay geometry: {error}"),
                                );
                                restore_capture_settings(Some(&settings), return_visibility);
                                finish_capture_session(&active_flag, "invalid_frame_viewport");
                                return;
                            }
                        };
                        let logical_viewport = frame_viewport.logical_size();
                        platform::trace_event(&format!(
                            "overlay.target_display id={:?} bounds=({}, {}, {}, {}) dpi={} scale_factor={}",
                            display.id,
                            display.bounds.left,
                            display.bounds.top,
                            display.bounds.right,
                            display.bounds.bottom,
                            display.effective_dpi,
                            scale_factor
                        ));
                        platform::trace_event(&format!(
                            "capture.frame_desktop_geometry monitor_id={:?} origin=({}, {}) end=({}, {})",
                            display.id,
                            frame_origin.x,
                            frame_origin.y,
                            frame_bottom_right.x,
                            frame_bottom_right.y
                        ));
                        // Slint's fullscreen boolean maps to winit Borderless(None),
                        // which chooses the monitor containing the HWND. Set both
                        // physical bounds first so this session's target is
                        // independent of the Overlay's previous placement/size.
                        overlay
                            .window()
                            .set_position(slint::PhysicalPosition::new(frame_origin.x, frame_origin.y));
                        overlay.window().set_size(slint::PhysicalSize::new(
                            frame.width(),
                            frame.height(),
                        ));
                        *frame_viewport_slot
                            .lock()
                            .expect("frame viewport lock") = Some(frame_viewport);
                        overlay.set_frame_viewport_width(logical_viewport.width as f32);
                        overlay.set_frame_viewport_height(logical_viewport.height as f32);
                        log::info!(
                            "Capture geometry: frame={}x{} logical_viewport={}x{} scale_factor={}",
                            frame.width(),
                            frame.height(),
                            logical_viewport.width,
                            logical_viewport.height,
                            scale_factor
                        );
                        overlay.set_captured_image(image_from_rgba(
                            frame.pixels(),
                            frame.width(),
                            frame.height(),
                        ));
                        overlay.invoke_focus_selection();
                        overlay.set_selection_x(0.0);
                        overlay.set_selection_y(0.0);
                        overlay.set_selection_width(0.0);
                        overlay.set_selection_height(0.0);
                        overlay.set_annotation_mode(false);
                        overlay.set_annotation_image(Image::default());
                        *selection_pixels_for_result
                            .lock()
                            .expect("selection pixel rectangle lock") = None;
                        *bounds_for_result
                            .lock()
                            .expect("selection bounds lock") = None;
                        *frame_slot.lock().expect("capture frame lock") = Some(frame);
                        *selection_slot.lock().expect("selection lock") = Selection::default();
                        #[cfg(target_os = "windows")]
                        let taskbar_window_pending =
                            !platform::taskbar::ensure_overlay_removed(
                                overlay.window(),
                                settings.window(),
                            );
                        platform::trace_event("overlay.set_fullscreen_true.before");
                        trace_overlay_state(&overlay, "overlay.set_fullscreen_true.before");
                        overlay.window().set_fullscreen(true);
                        trace_overlay_state(&overlay, "overlay.set_fullscreen_true.after");
                        platform::trace_event("overlay.show.before");
                        trace_overlay_state(&overlay, "overlay.show.before");
                        if let Err(error) = overlay.show() {
                            set_status(
                                &settings,
                                format!("Could not show selection overlay: {error}"),
                            );
                            platform::trace_event(
                                "overlay.set_fullscreen_false.show_failed.before",
                            );
                            trace_overlay_state(
                                &overlay,
                                "overlay.set_fullscreen_false.show_failed.before",
                            );
                            overlay.window().set_fullscreen(false);
                            trace_overlay_state(
                                &overlay,
                                "overlay.set_fullscreen_false.show_failed.after",
                            );
                            restore_capture_settings(Some(&settings), return_visibility);
                            finish_capture_session(&active_flag, "overlay_show_failed");
                            return;
                        }
                        trace_overlay_state(&overlay, "overlay.show.after");
                        platform::trace_slint_native_handle(
                            "Overlay",
                            "after_show",
                            overlay.window(),
                        );
                        #[cfg(target_os = "windows")]
                        if taskbar_window_pending
                            && !platform::taskbar::ensure_overlay_removed(
                                overlay.window(),
                                settings.window(),
                            )
                        {
                            let weak_overlay = overlay.as_weak();
                            let weak_settings = settings.as_weak();
                            if let Err(error) = slint::spawn_local(async move {
                                let Some(overlay) = weak_overlay.upgrade() else {
                                    return;
                                };
                                let native_window = match overlay.window().winit_window().await {
                                    Ok(window) => window,
                                    Err(error) => {
                                        log::warn!("Overlay HWND unavailable for taskbar removal: {error}");
                                        return;
                                    }
                                };
                                drop(native_window);
                                if overlay.window().is_visible() {
                                    if let Some(settings) = weak_settings.upgrade() {
                                        platform::taskbar::ensure_overlay_removed(
                                            overlay.window(),
                                            settings.window(),
                                        );
                                    }
                                }
                            }) {
                                log::warn!("Could not wait for Overlay HWND to remove its taskbar tab: {error}");
                            }
                        }
                        platform::diagnose_cursor("overlay shown");
                        if !active_flag.capture_ready() {
                            set_status(
                                &settings,
                                "Capture session state changed before selection.",
                            );
                            platform::trace_event(
                                "overlay.set_fullscreen_false.capture_not_ready.before",
                            );
                            trace_overlay_state(
                                &overlay,
                                "overlay.set_fullscreen_false.capture_not_ready.before",
                            );
                            overlay.window().set_fullscreen(false);
                            trace_overlay_state(
                                &overlay,
                                "overlay.set_fullscreen_false.capture_not_ready.after",
                            );
                            platform::trace_event("overlay.hide.capture_not_ready.before");
                            trace_overlay_state(&overlay, "overlay.hide.capture_not_ready.before");
                            let _ = overlay.hide();
                            trace_overlay_state(&overlay, "overlay.hide.capture_not_ready.after");
                            platform::trace_event("overlay.hide.capture_not_ready.after");
                            overlay.set_captured_image(Image::default());
                            restore_capture_settings(Some(&settings), return_visibility);
                            finish_capture_session(
                                &active_flag,
                                "capture_not_ready_after_overlay_show",
                            );
                            return;
                        }
                        platform::trace_event("capture.session.transition=Selecting");
                        platform::trace_event("overlay.activate.begin");
                        if let Err(error) = platform::activate_overlay() {
                            log::warn!("Could not activate the native selection overlay: {error}");
                        }
                        platform::trace_event("overlay.activate.end");
                        platform::diagnose_cursor("overlay activated");
                        overlay.invoke_focus_selection();
                        log::info!("Selection overlay shown with frozen capture frame");
                    }
                    Err(error) => {
                        platform::trace_event(&format!(
                            "capture.capture_primary.failed error={error:?}"
                        ));
                        set_status(&settings, format!("Screen capture failed: {error}"));
                        restore_capture_settings(Some(&settings), return_visibility);
                        finish_capture_session(&active_flag, "screen_capture_failed");
                    }
                }
            });
            if let Err(error) = dispatched {
                log::error!("Could not return capture result to the UI thread: {error}");
                restore_capture_settings(None, return_visibility);
                finish_capture_session(&session_for_dispatch, "capture_result_dispatch_failed");
            }
        });
    });

    let weak_overlay = overlay.as_weak();
    let weak_settings = settings.as_weak();
    let frame_slot = captured_frame.clone();
    let crop_slot = selected_crop.clone();
    let bounds_slot = selection_bounds.clone();
    let selection_pixels_slot = selection_pixels.clone();
    let editor_slot = annotation_editor.clone();
    let text_position_slot = pending_text_position.clone();
    let selection_slot = selection.clone();
    let frame_viewport_slot = frame_viewport.clone();
    let active_flag = capture_session.clone();
    let restore_state_slot = capture_restore_state.clone();
    overlay.on_cancel_selection(move || {
        let action_id = platform::begin_window_action("Escape");
        platform::trace_event("capture.cancel_selection.requested");
        if let Some(overlay) = weak_overlay.upgrade() {
            trace_overlay_state(&overlay, "capture.cancel_selection.requested");
        }
        platform::diagnose_cursor("escape before close");
        if !active_flag.cancel() {
            platform::trace_event("capture.cancel_selection.rejected_no_active_session");
            platform::finish_window_action(action_id, "rejected_no_active_session");
            return;
        }
        platform::trace_event("capture.cancel_selection.accepted");
        let return_visibility =
            SettingsVisibility::from_code(restore_state_slot.load(Ordering::Acquire));
        frame_slot.lock().expect("capture frame lock").take();
        crop_slot.lock().expect("selected crop lock").take();
        *bounds_slot.lock().expect("selection bounds lock") = None;
        *selection_pixels_slot
            .lock()
            .expect("selection pixel rectangle lock") = None;
        *frame_viewport_slot.lock().expect("frame viewport lock") = None;
        *editor_slot.lock().expect("annotation editor lock") = None;
        *text_position_slot.lock().expect("text position lock") = None;
        selection_slot.lock().expect("selection lock").cancel();
        let settings = weak_settings.upgrade();
        if let Some(settings) = settings.as_ref() {
            set_status(&settings, "Capture cancelled. Ready for another capture.");
        }
        restore_capture_settings(settings.as_ref(), return_visibility);
        if let Some(overlay) = weak_overlay.upgrade() {
            trace_overlay_state(&overlay, "capture.cancel_selection.hide_overlay.before");
            overlay.set_annotation_mode(false);
            overlay.set_annotation_image(Image::default());
            overlay.set_text_entry_visible(false);
            platform::trace_event("overlay.hide.cancel.before");
            trace_overlay_state(&overlay, "overlay.hide.cancel.before");
            let hide_result = overlay.hide();
            platform::trace_event(&format!(
                "overlay.hide.cancel.result success={}",
                hide_result.is_ok()
            ));
            trace_overlay_state(&overlay, "overlay.hide.cancel.after");
            platform::trace_event("overlay.set_fullscreen_false.cancel.before");
            trace_overlay_state(&overlay, "overlay.set_fullscreen_false.cancel.before");
            overlay.window().set_fullscreen(false);
            trace_overlay_state(&overlay, "overlay.set_fullscreen_false.cancel.after");
            platform::trace_slint_native_handle(
                "Overlay",
                "cancel.after_hide_and_fullscreen_exit",
                overlay.window(),
            );
            platform::trace_event("overlay.hide.cancel.after");
            overlay.set_captured_image(Image::default());
            platform::diagnose_cursor("escape after close");
        }
        platform::finish_window_action(action_id, "cancel_teardown_complete");
    });

    let weak_overlay = overlay.as_weak();
    let selection_slot = selection.clone();
    let editor_slot = annotation_editor.clone();
    let text_position_slot = pending_text_position.clone();
    let bounds_slot = selection_bounds.clone();
    let crop_slot = selected_crop.clone();
    let frame_viewport_slot = frame_viewport.clone();
    let active_flag = capture_session.clone();
    overlay.on_pointer_down(move |x, y, width, height| {
        if !active_flag.is_active() {
            return;
        }
        let Some(overlay) = weak_overlay.upgrade() else {
            return;
        };
        if current_frame_transform(&frame_viewport_slot, width as f64, height as f64).is_none() {
            selection_slot.lock().expect("selection lock").cancel();
            if editor_slot
                .lock()
                .expect("annotation editor lock")
                .is_none()
            {
                clear_selection_preview(&overlay);
            }
            return;
        }
        if editor_slot
            .lock()
            .expect("annotation editor lock")
            .is_some()
        {
            let bounds = *bounds_slot.lock().expect("selection bounds lock");
            let crop_guard = crop_slot.lock().expect("selected crop lock");
            let (Some(bounds), Some(crop)) = (bounds, crop_guard.as_ref()) else {
                return;
            };
            if !contains(bounds, x as f64, y as f64) {
                return;
            }
            let point = annotation_point(x as f64, y as f64, bounds, crop);
            drop(crop_guard);
            let Ok(Some(point)) = point else {
                return;
            };
            if editor_slot
                .lock()
                .expect("annotation editor lock")
                .as_ref()
                .is_some_and(|editor| editor.active_tool() == AnnotationTool::Text)
            {
                *text_position_slot.lock().expect("text position lock") = Some(point);
                overlay.set_text_entry_value(String::new().into());
                overlay.set_text_entry_x((x + 8.0).max(0.0) as f32);
                overlay.set_text_entry_y((y + 8.0).min(height - 52.0).max(0.0) as f32);
                overlay.set_text_entry_visible(true);
                return;
            }
            if let Some(editor) = editor_slot.lock().expect("annotation editor lock").as_mut() {
                editor.begin(point);
            }
            if let Err(error) = refresh_annotation_preview(&overlay, &crop_slot, &editor_slot) {
                log::warn!("Could not render annotation preview: {error}");
            }
            return;
        }
        selection_slot.lock().expect("selection lock").begin(
            x as f64,
            y as f64,
            width as f64,
            height as f64,
        );
        overlay.set_selection_width(0.0);
        overlay.set_selection_height(0.0);
    });

    let weak_overlay = overlay.as_weak();
    let selection_slot = selection.clone();
    let frame_viewport_slot = frame_viewport.clone();
    let bounds_slot = selection_bounds.clone();
    let crop_slot = selected_crop.clone();
    let editor_slot = annotation_editor.clone();
    overlay.on_pointer_move(move |x, y, width, height| {
        platform::diagnose_cursor_motion();
        let Some(overlay) = weak_overlay.upgrade() else {
            return;
        };
        let Some(transform) =
            current_frame_transform(&frame_viewport_slot, width as f64, height as f64)
        else {
            selection_slot.lock().expect("selection lock").cancel();
            if editor_slot
                .lock()
                .expect("annotation editor lock")
                .is_none()
            {
                clear_selection_preview(&overlay);
            }
            return;
        };
        if editor_slot
            .lock()
            .expect("annotation editor lock")
            .as_ref()
            .is_some_and(AnnotationEditor::is_drawing)
        {
            let bounds = *bounds_slot.lock().expect("selection bounds lock");
            let crop_guard = crop_slot.lock().expect("selected crop lock");
            let (Some(bounds), Some(crop)) = (bounds, crop_guard.as_ref()) else {
                return;
            };
            let point = annotation_point(x as f64, y as f64, bounds, crop);
            drop(crop_guard);
            let Ok(Some(point)) = point else {
                return;
            };
            if let Some(editor) = editor_slot.lock().expect("annotation editor lock").as_mut() {
                editor.update(point);
            }
            if let Err(error) = refresh_annotation_preview(&overlay, &crop_slot, &editor_slot) {
                log::warn!("Could not render annotation preview: {error}");
            }
            return;
        }

        let mut state = selection_slot.lock().expect("selection lock");
        if !state.is_dragging() {
            return;
        }
        state.update(
            x as f64,
            y as f64,
            transform.viewport_width,
            transform.viewport_height,
        );
        if let Ok(Some((_, rect))) = state.preview(transform) {
            overlay.set_selection_x(rect.x as f32);
            overlay.set_selection_y(rect.y as f32);
            overlay.set_selection_width(rect.width as f32);
            overlay.set_selection_height(rect.height as f32);
        }
    });

    let weak_overlay = overlay.as_weak();
    let weak_settings = settings.as_weak();
    let frame_slot = captured_frame.clone();
    let crop_slot = selected_crop.clone();
    let bounds_slot = selection_bounds.clone();
    let selection_slot = selection.clone();
    let selection_pixels_slot = selection_pixels.clone();
    let frame_viewport_slot = frame_viewport.clone();
    let editor_slot = annotation_editor.clone();
    let text_position_slot = pending_text_position.clone();
    let active_flag = capture_session.clone();
    overlay.on_pointer_up(move |x, y, width, height| {
        let Some(overlay) = weak_overlay.upgrade() else {
            return;
        };
        let Some(transform) =
            current_frame_transform(&frame_viewport_slot, width as f64, height as f64)
        else {
            selection_slot.lock().expect("selection lock").cancel();
            if editor_slot
                .lock()
                .expect("annotation editor lock")
                .is_none()
            {
                clear_selection_preview(&overlay);
            }
            return;
        };
        if text_position_slot
            .lock()
            .expect("text position lock")
            .is_some()
        {
            return;
        }
        if editor_slot
            .lock()
            .expect("annotation editor lock")
            .as_ref()
            .is_some_and(AnnotationEditor::is_drawing)
        {
            let bounds = *bounds_slot.lock().expect("selection bounds lock");
            let crop_guard = crop_slot.lock().expect("selected crop lock");
            let (Some(bounds), Some(crop)) = (bounds, crop_guard.as_ref()) else {
                return;
            };
            let mapped = annotation_point(x as f64, y as f64, bounds, crop);
            let crop_width = crop.width();
            let crop_height = crop.height();
            drop(crop_guard);
            if let Ok(Some(point)) = mapped {
                if let Some(editor) = editor_slot.lock().expect("annotation editor lock").as_mut() {
                    editor.update(point);
                    if let Err(error) =
                        editor.finish(StrokeStyle::default(), crop_width, crop_height)
                    {
                        log::warn!("Could not finish annotation: {error}");
                    }
                }
                if let Err(error) = refresh_annotation_preview(&overlay, &crop_slot, &editor_slot) {
                    log::warn!("Could not render annotation preview: {error}");
                }
            }
            return;
        }

        selection_slot.lock().expect("selection lock").update(
            x as f64,
            y as f64,
            transform.viewport_width,
            transform.viewport_height,
        );
        let result = selection_slot
            .lock()
            .expect("selection lock")
            .finish(transform);
        let Ok(Some((pixels, logical))) = result else {
            if let Err(error) = result {
                log::error!("Selection geometry rejected: {error}");
            }
            // A click has no area; leave the frozen frame ready for another drag.
            return;
        };
        let crop = {
            let frame = frame_slot.lock().expect("capture frame lock");
            frame.as_ref().and_then(|frame| frame.crop(pixels).ok())
        };
        let Some(crop) = crop else {
            log::error!("Selected crop could not be copied from the captured frame");
            if let Some(settings) = weak_settings.upgrade() {
                set_status(&settings, "Could not copy selected screenshot region.");
            }
            return;
        };
        if !active_flag.begin_annotation() {
            log::error!("Could not transition capture session into annotation state");
            return;
        }
        *selection_pixels_slot
            .lock()
            .expect("selection pixel rectangle lock") = Some(pixels);
        *bounds_slot.lock().expect("selection bounds lock") = Some(logical);
        platform::trace_event("capture.session.transition=Annotating");
        overlay.set_selection_x(logical.x as f32);
        overlay.set_selection_y(logical.y as f32);
        overlay.set_selection_width(logical.width as f32);
        overlay.set_selection_height(logical.height as f32);
        overlay.set_selected_tool(AnnotationTool::Pen.ui_index());
        *crop_slot.lock().expect("selected crop lock") = Some(crop);
        *editor_slot.lock().expect("annotation editor lock") = Some(AnnotationEditor::default());
        overlay.set_annotation_mode(true);
        if let Err(error) = refresh_annotation_preview(&overlay, &crop_slot, &editor_slot) {
            log::error!("Could not initialize annotation preview: {error}");
            if let Some(settings) = weak_settings.upgrade() {
                set_status(
                    &settings,
                    format!("Could not initialize annotation preview: {error}"),
                );
            }
        }
        overlay.invoke_focus_selection();
        if let Some(settings) = weak_settings.upgrade() {
            set_status(&settings, "Choose a drawing tool. Press Escape to cancel.");
        }
    });

    let editor_slot = annotation_editor.clone();
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
    let crop_slot = selected_crop.clone();
    let editor_slot = annotation_editor.clone();
    let text_position_slot = pending_text_position.clone();
    overlay.on_submit_text(move |text| {
        let Some(overlay) = weak_overlay.upgrade() else {
            return;
        };
        overlay.set_text_entry_visible(false);
        let position = text_position_slot
            .lock()
            .expect("text position lock")
            .take();
        let Some(position) = position else {
            return;
        };
        let dimensions = crop_slot
            .lock()
            .expect("selected crop lock")
            .as_ref()
            .map(|crop| (crop.width(), crop.height()));
        let Some((width, height)) = dimensions else {
            return;
        };
        let result = match editor_slot.lock().expect("annotation editor lock").as_mut() {
            Some(editor) => editor.add_text(position, text.to_string(), width, height),
            None => Err("annotation editor is no longer available"),
        };
        match result {
            Ok(_) => update_preview_if_possible(&overlay, &crop_slot, &editor_slot),
            Err(error) => log::warn!("Could not add text annotation: {error}"),
        }
    });

    let weak_overlay = overlay.as_weak();
    let text_position_slot = pending_text_position.clone();
    overlay.on_cancel_text(move || {
        *text_position_slot.lock().expect("text position lock") = None;
        if let Some(overlay) = weak_overlay.upgrade() {
            overlay.set_text_entry_visible(false);
            overlay.set_text_entry_value(String::new().into());
            overlay.invoke_focus_selection();
        }
    });

    let weak_overlay = overlay.as_weak();
    let crop_slot = selected_crop.clone();
    let editor_slot = annotation_editor.clone();
    overlay.on_undo_annotation(move || {
        if let Some(editor) = editor_slot.lock().expect("annotation editor lock").as_mut() {
            editor.undo();
        }
        if let Some(overlay) = weak_overlay.upgrade() {
            update_preview_if_possible(&overlay, &crop_slot, &editor_slot);
        }
    });

    let weak_overlay = overlay.as_weak();
    let crop_slot = selected_crop.clone();
    let editor_slot = annotation_editor.clone();
    overlay.on_redo_annotation(move || {
        if let Some(editor) = editor_slot.lock().expect("annotation editor lock").as_mut() {
            editor.redo();
        }
        if let Some(overlay) = weak_overlay.upgrade() {
            update_preview_if_possible(&overlay, &crop_slot, &editor_slot);
        }
    });

    let weak_overlay = overlay.as_weak();
    let weak_settings = settings.as_weak();
    let crop_slot = selected_crop.clone();
    let editor_slot = annotation_editor.clone();
    let text_position_slot = pending_text_position.clone();
    let busy = export_busy.clone();
    overlay.on_save_png(move || {
        platform::trace_event("export.save_png.callback");
        let Some(overlay) = weak_overlay.upgrade() else {
            return;
        };
        if text_position_slot
            .lock()
            .expect("pending text position lock")
            .is_some()
        {
            overlay.set_export_status("Add or cancel the current text before saving.".into());
            return;
        }
        let (crop, annotations) = match annotation_snapshot(&crop_slot, &editor_slot) {
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
            weak_settings.clone(),
            busy.clone(),
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
    let weak_settings = settings.as_weak();
    let crop_slot = selected_crop.clone();
    let editor_slot = annotation_editor.clone();
    let text_position_slot = pending_text_position.clone();
    let busy = export_busy.clone();
    overlay.on_copy_image(move || {
        platform::trace_event("export.copy_image.callback");
        let Some(overlay) = weak_overlay.upgrade() else {
            return;
        };
        if text_position_slot
            .lock()
            .expect("pending text position lock")
            .is_some()
        {
            overlay.set_export_status("Add or cancel the current text before copying.".into());
            return;
        }
        let (crop, annotations) = match annotation_snapshot(&crop_slot, &editor_slot) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                overlay.set_export_status(format!("Could not copy screenshot: {error}").into());
                return;
            }
        };
        start_export_operation(
            &overlay,
            weak_settings.clone(),
            busy.clone(),
            "Copying image to clipboard…",
            move || {
                let rendered = annotation_render::render_composite(&crop, &annotations)
                    .map_err(str::to_owned)?;
                platform::copy_image_to_clipboard(&rendered)?;
                Ok("Image copied to clipboard".into())
            },
        );
    });

    let weak_overlay = overlay.as_weak();
    let weak_settings = settings.as_weak();
    let frame_slot = captured_frame.clone();
    let crop_slot = selected_crop.clone();
    let bounds_slot = selection_bounds.clone();
    let selection_pixels_slot = selection_pixels.clone();
    let frame_viewport_slot = frame_viewport.clone();
    let editor_slot = annotation_editor.clone();
    let active_flag = capture_session.clone();
    let busy = export_busy.clone();
    let restore_state_slot = capture_restore_state.clone();
    overlay.on_finish_annotations(move || {
        let action_id = platform::begin_window_action("Done");
        platform::trace_event("capture.done.requested");
        if let Some(overlay) = weak_overlay.upgrade() {
            trace_overlay_state(&overlay, "capture.done.requested");
        }
        if busy.load(Ordering::Acquire) {
            platform::trace_event("capture.done.rejected_export_busy");
            if let Some(overlay) = weak_overlay.upgrade() {
                overlay.set_export_status("Wait for the current export before finishing.".into());
            }
            platform::finish_window_action(action_id, "rejected_export_busy");
            return;
        }
        platform::diagnose_cursor("done before close");
        let crop = crop_slot.lock().expect("selected crop lock").take();
        let annotations = editor_slot
            .lock()
            .expect("annotation editor lock")
            .as_ref()
            .map(|editor| editor.document().annotations().to_vec());
        let (Some(crop), Some(annotations)) = (crop, annotations) else {
            log::error!("Cannot finish annotation: crop or editor state is missing");
            platform::finish_window_action(action_id, "rejected_missing_crop_or_editor");
            return;
        };
        if !active_flag.begin_saving() {
            platform::trace_event("capture.done.rejected_session_transition");
            *crop_slot.lock().expect("selected crop lock") = Some(crop);
            log::error!("Could not transition capture session into saving state");
            platform::finish_window_action(action_id, "rejected_session_transition");
            return;
        }
        platform::trace_event("capture.session.transition=Saving");
        let return_visibility =
            SettingsVisibility::from_code(restore_state_slot.load(Ordering::Acquire));
        platform::trace_event(&format!(
            "capture.done.accepted saved_visibility={return_visibility:?}"
        ));
        frame_slot.lock().expect("capture frame lock").take();
        *bounds_slot.lock().expect("selection bounds lock") = None;
        *selection_pixels_slot
            .lock()
            .expect("selection pixel rectangle lock") = None;
        *frame_viewport_slot.lock().expect("frame viewport lock") = None;
        *editor_slot.lock().expect("annotation editor lock") = None;
        if let Some(overlay) = weak_overlay.upgrade() {
            trace_overlay_state(&overlay, "capture.done.hide_overlay.before");
            overlay.set_annotation_mode(false);
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
            platform::trace_event("overlay.set_fullscreen_false.done.before");
            trace_overlay_state(&overlay, "overlay.set_fullscreen_false.done.before");
            overlay.window().set_fullscreen(false);
            trace_overlay_state(&overlay, "overlay.set_fullscreen_false.done.after");
            platform::trace_slint_native_handle(
                "Overlay",
                "done.after_hide_and_fullscreen_exit",
                overlay.window(),
            );
            platform::trace_event("overlay.hide.done.after");
            platform::diagnose_cursor("done after close");
        }
        platform::trace_event("capture.done.save_worker.queued");
        if let Some(settings) = weak_settings.upgrade() {
            set_status(&settings, "Saving annotated screenshot…");
        }
        let weak_settings = weak_settings.clone();
        let active_for_dispatch = active_flag.clone();
        let session_for_dispatch = active_flag.clone();
        std::thread::spawn(move || {
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
            let dispatched = slint::invoke_from_event_loop(move || {
                platform::trace_event("capture.done.result_dispatch.entered");
                let settings = weak_settings.upgrade();
                if let Some(settings) = settings.as_ref() {
                    match saved {
                        Ok((path, width, height)) => set_status(
                            &settings,
                            format!(
                                "Capture complete. Temporary PNG: {} ({width}×{height})",
                                path.display()
                            ),
                        ),
                        Err(error) => set_status(
                            &settings,
                            format!("Could not save annotated screenshot: {error}"),
                        ),
                    }
                }
                restore_capture_settings(settings.as_ref(), return_visibility);
                finish_capture_session(&active_for_dispatch, "done_result_dispatched");
                platform::trace_event("capture.done.session_finished");
                platform::finish_window_action(action_id, "result_dispatched");
            });
            if let Err(error) = dispatched {
                log::error!(
                    "Could not return annotated screenshot result to the UI thread: {error}"
                );
                restore_capture_settings(None, return_visibility);
                finish_capture_session(&session_for_dispatch, "done_result_dispatch_failed");
                platform::finish_window_action(action_id, "result_dispatch_failed");
            }
        });
    });

    #[cfg(target_os = "windows")]
    {
        let overlay_close_policy = overlay_close_policy();
        let weak_overlay = overlay.as_weak();
        overlay.window().on_close_requested(move || {
            platform::trace_event("overlay.close_requested");
            if let Some(overlay) = weak_overlay.upgrade() {
                trace_overlay_state(&overlay, "overlay.close_requested");
            }
            platform::trace_event(&format!("overlay.close_policy={overlay_close_policy:?}"));
            close_request_response(overlay_close_policy)
        });
    }
    #[cfg(not(target_os = "windows"))]
    overlay
        .window()
        .on_close_requested(|| slint::CloseRequestResponse::HideWindow);

    #[cfg(target_os = "windows")]
    let tray_ready = Arc::new(AtomicBool::new(false));
    #[cfg(target_os = "windows")]
    let tray_service = {
        let weak_settings = settings.as_weak();
        platform::trace_event("main.tray_start.before");
        let service = platform::tray::TrayService::start(move |command: TrayCommand| {
            let weak_settings = weak_settings.clone();
            platform::trace_event(&format!(
                "tray.command.dispatch_to_slint command={command:?}"
            ));
            if let Err(error) = slint::invoke_from_event_loop(move || {
                platform::trace_event(&format!("tray.command.slint_callback command={command:?}"));
                match command.action() {
                    AppAction::StartCapture => {
                        platform::trace_event("tray.action.start_capture");
                        if let Some(settings) = weak_settings.upgrade() {
                            trace_settings_state(&settings, "tray.start_capture.before_callback");
                            settings.invoke_open_overlay();
                            trace_settings_state(&settings, "tray.start_capture.after_callback");
                        }
                    }
                    AppAction::ShowSettings => {
                        if let Some(settings) = weak_settings.upgrade() {
                            trace_settings_state(&settings, "tray.open_settings.before_unminimize");
                            settings.window().set_minimized(false);
                            trace_settings_state(&settings, "tray.open_settings.after_unminimize");
                            trace_settings_state(&settings, "tray.open_settings.before_show");
                            if let Err(error) = settings.show() {
                                set_status(
                                    &settings,
                                    format!("Could not show the settings window: {error}"),
                                );
                                return;
                            }
                            trace_settings_state(&settings, "tray.open_settings.after_show");
                            if let Err(error) = platform::activate_settings() {
                                set_status(
                                    &settings,
                                    format!("Could not activate the settings window: {error}"),
                                );
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
        });

        match service {
            Ok(service) => {
                platform::trace_event("main.tray_start.after success=true");
                tray_ready.store(true, Ordering::Release);
                settings.set_tray_available(true);
                match service.hotkey_state() {
                    core::hotkey::RegistrationState::Registered => set_status(
                        &settings,
                        "Ready — Ctrl + Shift + S starts a capture. Carambola is in the notification area.",
                    ),
                    core::hotkey::RegistrationState::Conflict => set_status(
                        &settings,
                        "The tray is active, but Ctrl + Shift + S is already registered by another application.",
                    ),
                    core::hotkey::RegistrationState::Failed(code) => set_status(
                        &settings,
                        format!("The tray is active, but Ctrl + Shift + S could not register (Windows error {code})."),
                    ),
                }
                Some(service)
            }
            Err(error) => {
                platform::trace_event(&format!(
                    "main.tray_start.after success=false error={error}"
                ));
                log::error!("Native system tray initialization failed: {error}");
                set_status(
                    &settings,
                    format!("The system tray could not start: {error}. Use Exit Carambola below to close the app."),
                );
                None
            }
        }
    };

    settings.on_exit_app(|| {
        platform::trace_event("settings.exit_app.callback");
        let _ = slint::quit_event_loop();
    });

    #[cfg(target_os = "windows")]
    {
        let tray_ready = tray_ready.clone();
        let weak_settings = settings.as_weak();
        settings.window().on_close_requested(move || {
            platform::trace_event("settings.close_requested");
            if let Some(settings) = weak_settings.upgrade() {
                trace_settings_state(&settings, "settings.close_requested");
            }
            let policy = settings_close_policy(tray_ready.load(Ordering::Acquire));
            platform::trace_event(&format!("settings.close_policy={policy:?}"));
            close_request_response(policy)
        });
    }

    trace_settings_state(&settings, "startup.show.before");
    platform::trace_slint_native_handle("Settings", "before_show", settings.window());
    platform::trace_event("settings.startup_show.begin");
    settings.show().map_err(|error| {
        platform::trace_event(&format!("settings.startup_show.failed error={error}"));
        error
    })?;
    platform::trace_event("settings.startup_show.end");
    trace_settings_state(&settings, "startup.show.after");
    platform::trace_slint_native_handle("Settings", "after_show", settings.window());
    platform::trace_event("event_loop.first_callback.queue.before");
    let weak_settings_for_startup = settings.as_weak();
    let weak_overlay_for_startup = overlay.as_weak();
    let first_callback_result = slint::invoke_from_event_loop(move || {
        platform::trace_event("event_loop.first_callback.entered");
        platform::trace_event("event_loop.initialized");
        if let Some(settings) = weak_settings_for_startup.upgrade() {
            platform::trace_slint_native_handle(
                "Settings",
                "first_event_loop_callback",
                settings.window(),
            );
        }
        if let Some(overlay) = weak_overlay_for_startup.upgrade() {
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
    platform::trace_event("main.before_window_component_drop");
    Ok(())
}
