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
    annotations::{AnnotationEditor, AnnotationTool, ImagePoint, StrokeStyle, ViewportToImage},
    frame::CapturedFrame,
    selection::{LogicalRect, Selection},
    session::CaptureSession,
};
use overlay_ui::OverlayPrototype;
use settings_ui::SettingsWindow;
use slint::{ComponentHandle, Image, Rgba8Pixel, SharedPixelBuffer};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

fn set_status(settings: &SettingsWindow, message: impl Into<slint::SharedString>) {
    settings.set_status(message.into());
}

fn crop_output_path() -> PathBuf {
    std::env::temp_dir()
        .join("carambola-native")
        .join("selected-crop.png")
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
    // The default FemtoVG/OpenGL path leaves the native cursor physically
    // invisible over the fullscreen overlay on the affected Windows display.
    // Slint's software renderer preserves the Win32 cursor there.
    #[cfg(target_os = "windows")]
    slint::BackendSelector::new()
        .backend_name("winit".into())
        .renderer_name("software".into())
        .select()?;
    log::info!("Native capture prototype started");

    let settings = SettingsWindow::new()?;
    let overlay = OverlayPrototype::new()?;
    platform::diagnose_cursor("windows created");
    let captured_frame = Arc::new(Mutex::new(None::<CapturedFrame>));
    let selected_crop = Arc::new(Mutex::new(None::<CapturedFrame>));
    let selection_bounds = Arc::new(Mutex::new(None::<LogicalRect>));
    let annotation_editor = Arc::new(Mutex::new(None::<AnnotationEditor>));
    let pending_text_position = Arc::new(Mutex::new(None::<ImagePoint>));
    let selection = Arc::new(Mutex::new(Selection::default()));
    let capture_session = Arc::new(CaptureSession::default());

    let weak_settings = settings.as_weak();
    let weak_overlay = overlay.as_weak();
    let frame_slot = captured_frame.clone();
    let crop_slot = selected_crop.clone();
    let bounds_slot = selection_bounds.clone();
    let editor_slot = annotation_editor.clone();
    let text_position_slot = pending_text_position.clone();
    let selection_slot = selection.clone();
    let active_flag = capture_session.clone();
    settings.on_open_overlay(move || {
        log::info!("Capture requested");
        platform::diagnose_cursor("capture requested");
        if !active_flag.begin_capture() {
            return;
        }
        frame_slot.lock().expect("capture frame lock").take();
        crop_slot.lock().expect("selected crop lock").take();
        *bounds_slot.lock().expect("selection bounds lock") = None;
        *editor_slot.lock().expect("annotation editor lock") = None;
        *text_position_slot.lock().expect("text position lock") = None;
        *selection_slot.lock().expect("selection lock") = Selection::default();
        if let Some(overlay) = weak_overlay.upgrade() {
            overlay.set_annotation_mode(false);
            overlay.set_annotation_image(Image::default());
            overlay.set_text_entry_visible(false);
            overlay.set_text_entry_value(String::new().into());
            overlay.set_can_undo(false);
            overlay.set_can_redo(false);
            overlay.set_selection_x(0.0);
            overlay.set_selection_y(0.0);
            overlay.set_selection_width(0.0);
            overlay.set_selection_height(0.0);
        }
        if let Some(settings) = weak_settings.upgrade() {
            set_status(&settings, "Capturing the primary display…");
        }

        let weak_settings = weak_settings.clone();
        let weak_overlay = weak_overlay.clone();
        let frame_slot = frame_slot.clone();
        let selection_slot = selection_slot.clone();
        let active_flag = active_flag.clone();
        if weak_overlay.upgrade().is_none() {
            active_flag.finish();
            return;
        }
        if let Err(error) = platform::prepare_capture() {
            log::error!("Could not synchronize hidden settings window before capture: {error}");
            if let Some(settings) = weak_settings.upgrade() {
                set_status(
                    &settings,
                    format!("Could not prepare the desktop for capture: {error}"),
                );
            }
            let _ = platform::restore_settings();
            active_flag.finish();
            return;
        }
        platform::diagnose_cursor("settings hidden");

        // Capture before showing the transparent overlay so GDI cannot include
        // the overlay itself in the desktop pixels. Capture work stays off the
        // UI thread; the frozen frame is shown after the worker completes.
        std::thread::spawn(move || {
            let captured = platform::capture_primary();
            log::info!(
                "Primary-display capture completed: success={}",
                captured.is_ok()
            );
            let session_for_dispatch = active_flag.clone();
            let dispatched = slint::invoke_from_event_loop(move || {
                let Some(settings) = weak_settings.upgrade() else {
                    return;
                };
                let Some(overlay) = weak_overlay.upgrade() else {
                    active_flag.finish();
                    let _ = platform::restore_settings();
                    return;
                };
                match captured {
                    Ok(frame) => {
                        let scale_factor = overlay.window().scale_factor();
                        if scale_factor <= 0.0 || !scale_factor.is_finite() {
                            set_status(
                                &settings,
                                "The overlay reported an invalid display scale factor.",
                            );
                            let _ = platform::restore_settings();
                            active_flag.finish();
                            return;
                        }
                        overlay.set_frame_viewport_width(frame.width() as f32 / scale_factor);
                        overlay.set_frame_viewport_height(frame.height() as f32 / scale_factor);
                        log::info!(
                            "Capture geometry: frame={}x{} logical_viewport={}x{} scale_factor={}",
                            frame.width(),
                            frame.height(),
                            frame.width() as f32 / scale_factor,
                            frame.height() as f32 / scale_factor,
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
                        *frame_slot.lock().expect("capture frame lock") = Some(frame);
                        *selection_slot.lock().expect("selection lock") = Selection::default();
                        overlay.window().set_fullscreen(true);
                        if let Err(error) = overlay.show() {
                            set_status(
                                &settings,
                                format!("Could not show selection overlay: {error}"),
                            );
                            overlay.window().set_fullscreen(false);
                            let _ = platform::restore_settings();
                            active_flag.finish();
                            return;
                        }
                        platform::diagnose_cursor("overlay shown");
                        if !active_flag.capture_ready() {
                            set_status(
                                &settings,
                                "Capture session state changed before selection.",
                            );
                            overlay.window().set_fullscreen(false);
                            let _ = overlay.hide();
                            overlay.set_captured_image(Image::default());
                            let _ = platform::restore_settings();
                            active_flag.finish();
                            return;
                        }
                        if let Err(error) = platform::activate_overlay() {
                            log::warn!("Could not activate the native selection overlay: {error}");
                        }
                        platform::diagnose_cursor("overlay activated");
                        overlay.invoke_focus_selection();
                        log::info!("Selection overlay shown with frozen capture frame");
                    }
                    Err(error) => {
                        set_status(&settings, format!("Screen capture failed: {error}"));
                        let _ = platform::restore_settings();
                        active_flag.finish();
                    }
                }
            });
            if let Err(error) = dispatched {
                log::error!("Could not return capture result to the UI thread: {error}");
                if let Err(restore_error) = platform::restore_settings() {
                    log::error!(
                        "Could not restore settings after dispatch failure: {restore_error}"
                    );
                }
                session_for_dispatch.finish();
            }
        });
    });

    let weak_overlay = overlay.as_weak();
    let weak_settings = settings.as_weak();
    let frame_slot = captured_frame.clone();
    let crop_slot = selected_crop.clone();
    let bounds_slot = selection_bounds.clone();
    let editor_slot = annotation_editor.clone();
    let text_position_slot = pending_text_position.clone();
    let selection_slot = selection.clone();
    let active_flag = capture_session.clone();
    overlay.on_cancel_selection(move || {
        platform::diagnose_cursor("escape before close");
        if !active_flag.cancel() {
            return;
        }
        frame_slot.lock().expect("capture frame lock").take();
        crop_slot.lock().expect("selected crop lock").take();
        *bounds_slot.lock().expect("selection bounds lock") = None;
        *editor_slot.lock().expect("annotation editor lock") = None;
        *text_position_slot.lock().expect("text position lock") = None;
        selection_slot.lock().expect("selection lock").cancel();
        if let Some(settings) = weak_settings.upgrade() {
            set_status(&settings, "Capture cancelled. Ready for another capture.");
            let _ = platform::restore_settings();
        }
        if let Some(overlay) = weak_overlay.upgrade() {
            overlay.set_annotation_mode(false);
            overlay.set_annotation_image(Image::default());
            overlay.set_text_entry_visible(false);
            overlay.window().set_fullscreen(false);
            let _ = overlay.hide();
            overlay.set_captured_image(Image::default());
            platform::diagnose_cursor("escape after close");
        }
    });

    let weak_overlay = overlay.as_weak();
    let selection_slot = selection.clone();
    let editor_slot = annotation_editor.clone();
    let text_position_slot = pending_text_position.clone();
    let bounds_slot = selection_bounds.clone();
    let crop_slot = selected_crop.clone();
    let active_flag = capture_session.clone();
    overlay.on_pointer_down(move |x, y, width, height| {
        if !active_flag.is_active() {
            return;
        }
        let Some(overlay) = weak_overlay.upgrade() else {
            return;
        };
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
    let frame_slot = captured_frame.clone();
    let bounds_slot = selection_bounds.clone();
    let crop_slot = selected_crop.clone();
    let editor_slot = annotation_editor.clone();
    overlay.on_pointer_move(move |x, y, width, height| {
        platform::diagnose_cursor_motion();
        let Some(overlay) = weak_overlay.upgrade() else {
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

        let width = width as f64;
        let height = height as f64;
        let Some(frame) = frame_slot
            .lock()
            .expect("capture frame lock")
            .as_ref()
            .map(|frame| (frame.width(), frame.height()))
        else {
            return;
        };
        let mut state = selection_slot.lock().expect("selection lock");
        if !state.is_dragging() {
            return;
        }
        state.update(x as f64, y as f64, width, height);
        let transform = core::geometry::ViewportToFrame {
            viewport_width: width,
            viewport_height: height,
            scale_factor: overlay.window().scale_factor() as f64,
            frame_width: frame.0,
            frame_height: frame.1,
        };
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
    let editor_slot = annotation_editor.clone();
    let text_position_slot = pending_text_position.clone();
    let active_flag = capture_session.clone();
    overlay.on_pointer_up(move |x, y, width, height| {
        let Some(overlay) = weak_overlay.upgrade() else {
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

        let width = width as f64;
        let height = height as f64;
        selection_slot
            .lock()
            .expect("selection lock")
            .update(x as f64, y as f64, width, height);
        let Some((frame_width, frame_height)) = frame_slot
            .lock()
            .expect("capture frame lock")
            .as_ref()
            .map(|frame| (frame.width(), frame.height()))
        else {
            return;
        };
        let transform = core::geometry::ViewportToFrame {
            viewport_width: width,
            viewport_height: height,
            scale_factor: overlay.window().scale_factor() as f64,
            frame_width,
            frame_height,
        };
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
        overlay.set_selection_x(logical.x as f32);
        overlay.set_selection_y(logical.y as f32);
        overlay.set_selection_width(logical.width as f32);
        overlay.set_selection_height(logical.height as f32);
        overlay.set_selected_tool(AnnotationTool::Pen.ui_index());
        *bounds_slot.lock().expect("selection bounds lock") = Some(logical);
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
    let frame_slot = captured_frame.clone();
    let crop_slot = selected_crop.clone();
    let bounds_slot = selection_bounds.clone();
    let editor_slot = annotation_editor.clone();
    let active_flag = capture_session.clone();
    overlay.on_finish_annotations(move || {
        platform::diagnose_cursor("done before close");
        let crop = crop_slot.lock().expect("selected crop lock").take();
        let annotations = editor_slot
            .lock()
            .expect("annotation editor lock")
            .as_ref()
            .map(|editor| editor.document().annotations().to_vec());
        let (Some(crop), Some(annotations)) = (crop, annotations) else {
            log::error!("Cannot finish annotation: crop or editor state is missing");
            return;
        };
        if !active_flag.begin_saving() {
            *crop_slot.lock().expect("selected crop lock") = Some(crop);
            log::error!("Could not transition capture session into saving state");
            return;
        }
        frame_slot.lock().expect("capture frame lock").take();
        *bounds_slot.lock().expect("selection bounds lock") = None;
        *editor_slot.lock().expect("annotation editor lock") = None;
        if let Some(overlay) = weak_overlay.upgrade() {
            overlay.set_annotation_mode(false);
            overlay.set_annotation_image(Image::default());
            overlay.set_text_entry_visible(false);
            overlay.window().set_fullscreen(false);
            let _ = overlay.hide();
            platform::diagnose_cursor("done after close");
        }
        if let Some(settings) = weak_settings.upgrade() {
            set_status(&settings, "Saving annotated screenshot…");
            let _ = settings.show();
        }
        let weak_settings = weak_settings.clone();
        let active_for_dispatch = active_flag.clone();
        let session_for_dispatch = active_flag.clone();
        std::thread::spawn(move || {
            let saved = (|| {
                let output = annotation_render::render_composite(&crop, &annotations)
                    .map_err(str::to_owned)?;
                let output_path = crop_output_path();
                std::fs::create_dir_all(output_path.parent().expect("temp path parent"))
                    .map_err(|error| error.to_string())?;
                image::save_buffer_with_format(
                    &output_path,
                    output.pixels(),
                    output.width(),
                    output.height(),
                    image::ColorType::Rgba8,
                    image::ImageFormat::Png,
                )
                .map_err(|error| error.to_string())?;
                Ok::<_, String>((output_path, output.width(), output.height()))
            })();
            let dispatched = slint::invoke_from_event_loop(move || {
                if let Some(settings) = weak_settings.upgrade() {
                    match saved {
                        Ok((path, width, height)) => set_status(
                            &settings,
                            format!("Screenshot saved at {} ({width}×{height})", path.display()),
                        ),
                        Err(error) => set_status(
                            &settings,
                            format!("Could not save annotated screenshot: {error}"),
                        ),
                    }
                }
                let _ = platform::restore_settings();
                active_for_dispatch.finish();
            });
            if let Err(error) = dispatched {
                log::error!(
                    "Could not return annotated screenshot result to the UI thread: {error}"
                );
                if let Err(restore_error) = platform::restore_settings() {
                    log::error!(
                        "Could not restore settings after save dispatch failure: {restore_error}"
                    );
                }
                session_for_dispatch.finish();
            }
        });
    });

    overlay
        .window()
        .on_close_requested(|| slint::CloseRequestResponse::HideWindow);

    settings.show()?;
    slint::run_event_loop()?;
    Ok(())
}
