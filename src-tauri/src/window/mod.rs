use crate::app_state::{AppState, DisplayBounds, NativeOverlayGeometry, OverlayGeometrySignature};
use crate::capture::capture_screen;
use log::{debug, error, info, warn};
use serde::Serialize;
use std::time::Instant;
use tauri::{AppHandle, Emitter, Manager, WebviewWindow};
#[cfg(not(target_os = "macos"))]
use tauri::{PhysicalPosition, PhysicalSize};

/// Label of the single, pre-created screenshot overlay window.
pub const OVERLAY_LABEL: &str = "overlay-main";

/// Payload emitted to the hidden overlay as soon as a fresh frame is frozen in
/// the backend. The frontend must reset its session and decode this frame
/// *before* asking the backend to present the window (`present_overlay`), so the
/// overlay is never shown with stale pixels or a stale selection.
#[derive(Clone, Debug, Serialize)]
pub struct CaptureReadyPayload {
    #[serde(rename = "captureId")]
    pub capture_id: u64,
    #[serde(rename = "triggeredAt")]
    pub triggered_at_ms: u64,
    pub display: crate::app_state::DisplayInfo,
    #[serde(rename = "captureMs")]
    pub capture_ms: f64,
    #[serde(rename = "encodeMs")]
    pub encode_ms: f64,
    #[serde(rename = "pngBytes")]
    pub png_bytes: usize,
}

/// Applies the one-time, per-window style required by the screenshot overlay.
/// These properties do not change between activations, so they are applied once
/// during setup instead of on every shortcut press.
fn configure_overlay(window: &WebviewWindow) -> Result<(), String> {
    window
        .set_decorations(false)
        .map_err(|e| format!("Failed to remove window decorations: {e}"))?;
    if let Err(e) = window.set_shadow(false) {
        warn!("Failed to disable window shadow: {e}");
    }
    if let Err(e) = window.set_skip_taskbar(true) {
        warn!("Failed to skip taskbar entry: {e}");
    }
    window
        .set_resizable(false)
        .map_err(|e| format!("Failed to lock window size: {e}"))?;
    window
        .set_always_on_top(true)
        .map_err(|e| format!("Failed to raise overlay: {e}"))?;
    #[cfg(target_os = "windows")]
    if let Err(e) = configure_windows_overlay(window) {
        // Keep the screenshot workflow available if DWM rejects the hint on a
        // particular Windows build; the frontend fade remains the fallback.
        warn!("Could not disable native DWM transitions for overlay: {e}");
    }
    #[cfg(target_os = "macos")]
    configure_macos_overlay(window)?;
    Ok(())
}

/// Prevent Windows from applying its own first-show/hide animation to the
/// transparent overlay HWND. The frontend's existing opacity transition is
/// still responsible for the intended fade-in and fade-out.
#[cfg(target_os = "windows")]
fn configure_windows_overlay(window: &WebviewWindow) -> Result<(), String> {
    use windows::Win32::Foundation::BOOL;
    use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_TRANSITIONS_FORCEDISABLED};

    // Tauri's HWND comes from its newer `windows` crate version; rebuild the
    // transparent pointer wrapper expected by Carambola's Win32 bindings.
    let hwnd = windows::Win32::Foundation::HWND(
        window
            .hwnd()
            .map_err(|e| format!("Failed to get overlay HWND: {e}"))?
            .0,
    );
    let disable_transitions = BOOL(1);
    unsafe {
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_TRANSITIONS_FORCEDISABLED,
            (&disable_transitions as *const BOOL).cast(),
            std::mem::size_of::<BOOL>() as u32,
        )
    }
    .map_err(|e| format!("Failed to disable DWM transitions: {e}"))
}

/// Configure the native window so transparent pixels still receive input and
/// the overlay remains above the active app, including its full-screen Space.
#[cfg(target_os = "macos")]
fn configure_macos_overlay(window: &WebviewWindow) -> Result<(), String> {
    use objc2_app_kit::{
        NSScreenSaverWindowLevel, NSWindow, NSWindowAnimationBehavior, NSWindowCollectionBehavior,
    };

    window
        .set_visible_on_all_workspaces(true)
        .map_err(|e| format!("Failed to show overlay on all Spaces: {e}"))?;
    window
        .with_webview(|webview| unsafe {
            let native_window = &*(webview.ns_window().cast::<NSWindow>());
            let behavior = native_window.collectionBehavior()
                | NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::FullScreenAuxiliary
                | NSWindowCollectionBehavior::Stationary;
            native_window.setCollectionBehavior(behavior);
            native_window.setAnimationBehavior(NSWindowAnimationBehavior::None);
            native_window.setIgnoresMouseEvents(false);
            native_window.setLevel(NSScreenSaverWindowLevel);

            debug!(
                "[macOS overlay] collection={behavior:?} ignores_mouse_events={} level={}",
                native_window.ignoresMouseEvents(),
                native_window.level()
            );
        })
        .map_err(|e| format!("Failed to configure native macOS overlay: {e}"))
}

/// Applies the frameless, always-on-top, full-monitor geometry required for a
/// Lightshot-style overlay.
///
/// The position and size use the native display-coordinate units: logical
/// points on macOS, physical pixels on Windows and Linux.
pub fn apply_overlay_geometry(
    window: &WebviewWindow,
    bounds: &DisplayBounds,
) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        use core_graphics::display::CGDisplay;
        use objc2_foundation::{NSPoint, NSRect, NSSize};

        // CoreGraphics uses a top-left origin in display points; AppKit uses a
        // bottom-left origin. Tao's position conversion uses the main display's
        // physical pixel height, which shifts a window on Retina displays.
        let main_height = CGDisplay::main().bounds().size.height;
        let x = bounds.x;
        let y = main_height - bounds.y - bounds.height;
        let width = bounds.width.max(1.0);
        let height = bounds.height.max(1.0);
        window
            .with_webview(move |webview| unsafe {
                use objc2_app_kit::NSWindow;
                let native_window = &*(webview.ns_window().cast::<NSWindow>());
                native_window.setFrame_display(
                    NSRect::new(NSPoint::new(x, y), NSSize::new(width, height)),
                    true,
                );
                let actual = native_window.frame();
                info!(
                    "[macOS overlay] frame_points=({}, {}, {}, {})",
                    actual.origin.x, actual.origin.y, actual.size.width, actual.size.height
                );
            })
            .map_err(|e| format!("Failed to position and resize overlay: {e}"))?;
    }

    #[cfg(not(target_os = "macos"))]
    {
        let x = bounds.x.round() as i32;
        let y = bounds.y.round() as i32;
        let width = bounds.width.round().max(1.0) as u32;
        let height = bounds.height.round().max(1.0) as u32;

        window
            .set_position(PhysicalPosition::new(x, y))
            .map_err(|e| format!("Failed to position overlay: {e}"))?;
        window
            .set_size(PhysicalSize::new(width, height))
            .map_err(|e| format!("Failed to resize overlay: {e}"))?;
        info!("Overlay geometry set to {width}x{height} at ({x}, {y})");
    }
    Ok(())
}

/// Called once during application setup. Sizes the hidden overlay to the
/// captured monitor so it is never presented at the default window size and so
/// the webview lays out at the correct dimensions ahead of the first capture.
pub fn prime_overlay(app_handle: &AppHandle, state: &AppState) {
    let Some(window) = app_handle.get_webview_window(OVERLAY_LABEL) else {
        return;
    };

    if let Err(e) = configure_overlay(&window) {
        warn!("Failed to configure overlay window: {e}");
    }

    let mut bounds = state
        .get_frame()
        .map(|frame| frame.display_info.bounds.clone());

    // The app starts without a captured frame, so the configured 200x200
    // default would otherwise remain the hidden WebView's size until the first
    // shortcut press. Prime only the geometry here; never capture pixels at
    // startup. The first presentation then reuses the already-sized HWND and
    // WebView surface instead of resizing them immediately before ShowWindow.
    #[cfg(target_os = "windows")]
    if bounds.is_none() {
        match crate::capture::selected_display_bounds() {
            Ok(selected_bounds) => bounds = Some(selected_bounds),
            Err(e) => warn!("Failed to discover display bounds while priming overlay: {e}"),
        }
    }

    if let Some(bounds) = bounds {
        if let Err(e) = apply_overlay_geometry(&window, &bounds) {
            warn!("Failed to prime overlay geometry: {e}");
        }
    }

    // Ensure the overlay stays hidden until the user triggers a capture.
    let _ = window.hide();
}

/// First phase of an activation: freeze the current desktop and hand it to the
/// (still hidden) webview. The window is intentionally **not** shown here.
///
/// Exactly one session may exist at a time: the session lock is claimed here and
/// only released by `hide_overlay` once the overlay finishes its closing
/// lifecycle. Repeated activations during a session are ignored, not queued.
pub fn activate_overlay(app_handle: &AppHandle, state: &AppState) -> Result<(), String> {
    let activation_started = Instant::now();

    let Some(capture_id) = state.try_begin_session() else {
        info!("Ignoring activation: a screenshot session is already active");
        return Ok(());
    };

    match prepare_overlay(app_handle, state, capture_id, activation_started) {
        Ok(()) => Ok(()),
        Err(err) => {
            // Never leave the lock held when initialization fails.
            state.end_session();
            Err(err)
        }
    }
}

fn prepare_overlay(
    app_handle: &AppHandle,
    state: &AppState,
    capture_id: u64,
    activation_started: Instant,
) -> Result<(), String> {
    info!("Activating screenshot overlay...");

    let window = app_handle
        .get_webview_window(OVERLAY_LABEL)
        .ok_or_else(|| "Overlay window not found".to_string())?;

    // Capture the target monitor immediately and freeze the frame in memory.
    // The desktop is captured exactly once per activation; it is never
    // re-captured while the overlay is shown.
    let frame = capture_screen().map_err(|e| {
        error!("Screen capture failed: {e}");
        e
    })?;

    let frame = state.set_frame(frame, capture_id);
    let payload = payload_for_frame(&frame, capture_id);

    // Geometry is applied while the window is still hidden, so the webview has
    // already laid out at the target size by the time it becomes visible.
    let before = overlay_geometry_signature(&window, &frame)?;
    if state.overlay_geometry_changed(&before) && !native_geometry_matches_frame(&before, &frame) {
        apply_overlay_geometry(&window, &frame.display_info.bounds)?;
    }
    let after = overlay_geometry_signature(&window, &frame)?;
    if after.native.width.abs_diff(frame.width) > 2
        || after.native.height.abs_diff(frame.height) > 2
    {
        return Err(format!(
            "Native overlay size {}x{} does not match captured frame {}x{}",
            after.native.width, after.native.height, frame.width, frame.height
        ));
    }
    #[cfg(not(target_os = "macos"))]
    if (after.native.x as f64 - frame.display_info.bounds.x).abs() > 1.0
        || (after.native.y as f64 - frame.display_info.bounds.y).abs() > 1.0
    {
        return Err(format!(
            "Native overlay origin ({}, {}) does not match selected display origin ({}, {})",
            after.native.x,
            after.native.y,
            frame.display_info.bounds.x,
            frame.display_info.bounds.y
        ));
    }
    state.set_last_overlay_geometry(after);

    debug!(
        "[perf] native prepare: {} ({:.1}ms since activation start)",
        summarize(&payload),
        activation_started.elapsed().as_secs_f64() * 1000.0
    );

    // Tauri does not retain events for listeners that have not subscribed yet.
    // The global shortcut is available during startup, before React's async
    // listen() registration necessarily completes, so queue that first frame.
    if state.defer_capture_event_until_frontend_ready() {
        info!("Overlay frontend is not ready; deferring capture delivery until its listener is registered");
    } else {
        emit_capture_ready(&window, payload)?;
    }

    Ok(())
}

fn native_geometry_matches_frame(
    signature: &OverlayGeometrySignature,
    frame: &crate::app_state::CapturedFrame,
) -> bool {
    let native = &signature.native;
    let size_matches =
        native.width.abs_diff(frame.width) <= 2 && native.height.abs_diff(frame.height) <= 2;
    let scale_matches = (native.scale_factor - frame.display_info.scale_factor).abs() < 0.01;
    #[cfg(target_os = "macos")]
    let origin_matches = true;
    #[cfg(not(target_os = "macos"))]
    let origin_matches = (native.x as f64 - frame.display_info.bounds.x).abs() <= 1.0
        && (native.y as f64 - frame.display_info.bounds.y).abs() <= 1.0;
    size_matches && scale_matches && origin_matches
}

fn payload_for_frame(
    frame: &crate::app_state::CapturedFrame,
    capture_id: u64,
) -> CaptureReadyPayload {
    CaptureReadyPayload {
        capture_id,
        triggered_at_ms: frame.triggered_at_ms,
        display: frame.display_info.clone(),
        capture_ms: frame.capture_ms,
        encode_ms: frame.encode_ms,
        png_bytes: frame.png_bytes.len(),
    }
}

fn emit_capture_ready(window: &WebviewWindow, payload: CaptureReadyPayload) -> Result<(), String> {
    window
        .emit("capture_triggered", payload)
        .map_err(|e| format!("Failed to notify overlay frontend: {e}"))
}

/// Completes the startup handshake after the frontend's capture listener is
/// registered. If the user triggered capture during that registration window,
/// deliver its already-captured frame now without showing an intermediate UI.
pub fn overlay_frontend_ready(app_handle: &AppHandle, state: &AppState) -> Result<(), String> {
    if !state.mark_overlay_frontend_ready() {
        info!("Overlay frontend listener is ready");
        return Ok(());
    }

    let window = app_handle
        .get_webview_window(OVERLAY_LABEL)
        .ok_or_else(|| "Overlay window not found".to_string())?;
    let frame = state
        .get_frame()
        .ok_or_else(|| "Pending capture event has no captured frame".to_string())?;
    let capture_id = state
        .current_capture_id()
        .ok_or_else(|| "Pending capture has no id".to_string())?;
    emit_capture_ready(&window, payload_for_frame(&frame, capture_id))?;
    info!("Delivered the first queued capture after overlay listener registration");
    Ok(())
}

fn summarize(payload: &CaptureReadyPayload) -> String {
    format!(
        "capture={:.1}ms encode={:.1}ms png={}B display={}x{}",
        payload.capture_ms,
        payload.encode_ms,
        payload.png_bytes,
        payload.display.bounds.width,
        payload.display.bounds.height
    )
}

/// Second phase of an activation: the frontend has reset its session and
/// decoded the fresh frame, so the pre-created window can be presented.
pub fn present_overlay(
    app_handle: &AppHandle,
    state: &AppState,
    capture_id: u64,
) -> Result<(), String> {
    let window = app_handle
        .get_webview_window(OVERLAY_LABEL)
        .ok_or_else(|| "Overlay window not found".to_string())?;

    if !state.is_capture_render_ready(capture_id) {
        return Err(
            "Capture is not current or frontend has not confirmed stable rendering".to_string(),
        );
    }

    if state.get_frame().is_none() {
        return Err("No captured frame is ready".to_string());
    }

    window
        .show()
        .map_err(|e| format!("Failed to show overlay: {e}"))?;
    #[cfg(target_os = "macos")]
    {
        if let Err(e) = window.set_focus() {
            let _ = window.hide();
            return Err(format!("Failed to focus screenshot overlay: {e}"));
        }
        if let Err(e) = window.with_webview(|webview| unsafe {
            use objc2::MainThreadMarker;
            use objc2_app_kit::{NSApplication, NSWindow, NSScreenSaverWindowLevel};
            let native_window = &*(webview.ns_window().cast::<NSWindow>());
            // Tauri's always-on-top configuration can subsequently replace the
            // setup-time level with NSFloatingWindowLevel. Restore the capture
            // level only when presenting the overlay, after that configuration.
            native_window.setLevel(NSScreenSaverWindowLevel);
            native_window.setIgnoresMouseEvents(false);
            if let Some(main_thread) = MainThreadMarker::new() {
                #[allow(deprecated)]
                NSApplication::sharedApplication(main_thread).activateIgnoringOtherApps(true);
            }
            native_window.makeKeyAndOrderFront(None);
            info!(
                "[macOS overlay] visible={} key={} can_become_key={} ignores_mouse_events={} level={}",
                native_window.isVisible(),
                native_window.isKeyWindow(),
                native_window.canBecomeKeyWindow(),
                native_window.ignoresMouseEvents(),
                native_window.level()
            );
        }) {
            warn!("Could not inspect macOS overlay focus: {e}");
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = window.set_focus();
    state.set_overlay_visible(true);

    // Tray "Settings..." opens only after the fresh capture has been presented,
    // so the session reset has already cleared any stale Settings state.
    if state.take_settings_on_present() {
        if let Err(e) = app_handle.emit("open_shortcut_settings", ()) {
            warn!("Failed to open Settings after presentation: {e}");
        }
    }

    debug!("[perf] overlay presented (window visible and focused)");
    Ok(())
}

fn overlay_geometry_signature(
    window: &WebviewWindow,
    frame: &crate::app_state::CapturedFrame,
) -> Result<OverlayGeometrySignature, String> {
    let position = window
        .outer_position()
        .map_err(|e| format!("Failed to read overlay position: {e}"))?;
    let size = window
        .inner_size()
        .map_err(|e| format!("Failed to read overlay size: {e}"))?;
    let scale_factor = window
        .scale_factor()
        .map_err(|e| format!("Failed to read overlay scale factor: {e}"))?;
    Ok(OverlayGeometrySignature {
        monitor_id: frame.display_info.id,
        monitor_name: frame.display_info.name.clone(),
        bounds: frame.display_info.bounds.clone(),
        scale_factor: frame.display_info.scale_factor,
        physical_width: frame.display_info.physical_width,
        physical_height: frame.display_info.physical_height,
        frame_width: frame.width,
        frame_height: frame.height,
        native: NativeOverlayGeometry {
            x: position.x,
            y: position.y,
            width: size.width,
            height: size.height,
            scale_factor,
        },
    })
}

pub fn acknowledge_overlay_render(
    state: &AppState,
    capture_id: u64,
    viewport_width: f64,
    viewport_height: f64,
    dpr: f64,
    canvas_width: u32,
    canvas_height: u32,
    image_width: u32,
    image_height: u32,
) -> Result<(), String> {
    let frame = state
        .get_frame()
        .ok_or_else(|| "No captured frame is ready".to_string())?;
    if !state.is_active_capture(capture_id) {
        return Err("Capture was superseded before render acknowledgement".to_string());
    }
    validate_overlay_render_metrics(
        viewport_width,
        viewport_height,
        dpr,
        canvas_width,
        canvas_height,
        image_width,
        image_height,
        frame.width,
        frame.height,
    )?;
    state.mark_capture_render_ready(capture_id)
}

fn validate_overlay_render_metrics(
    viewport_width: f64,
    viewport_height: f64,
    dpr: f64,
    canvas_width: u32,
    canvas_height: u32,
    image_width: u32,
    image_height: u32,
    frame_width: u32,
    frame_height: u32,
) -> Result<(), String> {
    if ![viewport_width, viewport_height, dpr]
        .iter()
        .all(|n| n.is_finite() && *n > 0.0)
    {
        return Err("Frontend reported invalid viewport or device pixel ratio".to_string());
    }
    if image_width != frame_width || image_height != frame_height {
        return Err("Rendered image dimensions do not match the captured frame".to_string());
    }
    let expected_canvas_width = (viewport_width * dpr).round();
    let expected_canvas_height = (viewport_height * dpr).round();
    if (canvas_width as f64 - expected_canvas_width).abs() > 1.0
        || (canvas_height as f64 - expected_canvas_height).abs() > 1.0
    {
        return Err(
            "Canvas backing dimensions have not synchronized with the viewport and DPI".to_string(),
        );
    }
    if (expected_canvas_width - frame_width as f64).abs() > 2.0
        || (expected_canvas_height - frame_height as f64).abs() > 2.0
    {
        return Err(format!(
            "Overlay viewport/DPI ({expected_canvas_width:.0}x{expected_canvas_height:.0}) does not match captured frame ({}x{})",
            frame_width, frame_height
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::validate_overlay_render_metrics;

    #[test]
    fn render_metrics_accept_matching_dpi_and_reject_stale_geometry() {
        assert!(validate_overlay_render_metrics(
            1536.0, 864.0, 1.25, 1920, 1080, 1920, 1080, 1920, 1080
        )
        .is_ok());
        assert!(validate_overlay_render_metrics(
            1536.0, 864.0, 1.0, 1536, 864, 1920, 1080, 1920, 1080
        )
        .is_err());
        assert!(validate_overlay_render_metrics(
            1600.0, 900.0, 1.1, 1920, 1080, 1920, 1080, 1920, 1080
        )
        .is_err());
    }
}

/// Reports a failed capture through the system log and gives macOS users an
/// actionable Screen Recording permission prompt when that is the cause.
pub fn report_activation_error(app_handle: &AppHandle, error: &str) {
    error!("Screenshot activation failed: {error}");

    #[cfg(target_os = "macos")]
    {
        let permission_denied = error.starts_with("Screen Recording access is not available");
        let detail = error.to_string();
        let app_handle = app_handle.clone();
        if let Err(dispatch_error) = app_handle.clone().run_on_main_thread(move || {
            use objc2::MainThreadMarker;
            use objc2_app_kit::{NSAlert, NSAlertFirstButtonReturn, NSWorkspace};
            use objc2_foundation::{NSString, NSURL};

            let Some(main_thread) = MainThreadMarker::new() else {
                warn!("Cannot show Screen Recording guidance outside the macOS main thread");
                return;
            };
            let alert = NSAlert::new(main_thread);
            alert.setMessageText(&NSString::from_str(if permission_denied {
                "Screen Recording access needed"
            } else {
                "Screen capture failed"
            }));
            alert.setInformativeText(&NSString::from_str(&detail));
            if permission_denied {
                alert.addButtonWithTitle(&NSString::from_str("Open Screen Recording Settings"));
                alert.addButtonWithTitle(&NSString::from_str("Cancel"));
            } else {
                alert.addButtonWithTitle(&NSString::from_str("OK"));
            }

            if alert.runModal() == NSAlertFirstButtonReturn && permission_denied {
                let settings_url = NSString::from_str(
                    "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture",
                );
                if let Some(url) = NSURL::URLWithString(&settings_url) {
                    if !NSWorkspace::sharedWorkspace().openURL(&url) {
                        warn!("macOS refused to open Screen Recording settings");
                    }
                }
            }
        }) {
            warn!("Could not schedule Screen Recording guidance: {dispatch_error}");
        }
    }
}

/// Explains when a previously saved shortcut cannot be restored at startup.
/// The fallback remains available for this run while the saved value is left
/// untouched so the user can resolve the conflict in Settings.
pub fn report_hotkey_fallback(app_handle: &AppHandle, shortcut: &str, error: &str) {
    warn!("Saved shortcut {shortcut:?} could not be restored: {error}");

    #[cfg(target_os = "macos")]
    {
        let message = format!(
            "Carambola could not restore the saved shortcut {shortcut:?}. The fallback Alt+A is active for this session. Right-click the Carambola menu bar icon and open Settings to choose another shortcut."
        );
        let app_handle = app_handle.clone();
        if let Err(dispatch_error) = app_handle.clone().run_on_main_thread(move || {
            use objc2::MainThreadMarker;
            use objc2_app_kit::NSAlert;
            use objc2_foundation::NSString;

            let Some(main_thread) = MainThreadMarker::new() else {
                warn!("Cannot show shortcut registration guidance outside the macOS main thread");
                return;
            };
            let alert = NSAlert::new(main_thread);
            alert.setMessageText(&NSString::from_str("Saved shortcut is unavailable"));
            alert.setInformativeText(&NSString::from_str(&message));
            alert.addButtonWithTitle(&NSString::from_str("OK"));
            alert.runModal();
        }) {
            warn!("Could not schedule shortcut registration guidance: {dispatch_error}");
        }
    }
}
