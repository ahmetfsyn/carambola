use crate::app_state::{AppState, DisplayBounds};
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
    #[cfg(target_os = "macos")]
    configure_macos_overlay(window)?;
    Ok(())
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

    if let Some(frame) = state.get_frame() {
        if let Err(e) = apply_overlay_geometry(&window, &frame.display_info.bounds) {
            warn!("Failed to prime overlay geometry: {e}");
        } else {
            state.set_last_overlay_bounds(frame.display_info.bounds.clone());
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

    if !state.try_begin_session() {
        info!("Ignoring activation: a screenshot session is already active");
        return Ok(());
    }

    match prepare_overlay(app_handle, state, activation_started) {
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

    let bounds = frame.display_info.bounds.clone();
    let payload = CaptureReadyPayload {
        triggered_at_ms: frame.triggered_at_ms,
        display: frame.display_info.clone(),
        capture_ms: frame.capture_ms,
        encode_ms: frame.encode_ms,
        png_bytes: frame.png_bytes.len(),
    };
    state.set_frame(frame);

    // Geometry is applied while the window is still hidden, so the webview has
    // already laid out at the target size by the time it becomes visible.
    if state.overlay_geometry_changed(&bounds) {
        apply_overlay_geometry(&window, &bounds)?;
        state.set_last_overlay_bounds(bounds);
    }

    debug!(
        "[perf] native prepare: {} ({:.1}ms since activation start)",
        summarize(&payload),
        activation_started.elapsed().as_secs_f64() * 1000.0
    );

    // Phase 2 is driven by the frontend: it resets the session, decodes this
    // frame, then invokes `present_overlay`.
    window
        .emit("capture_triggered", payload)
        .map_err(|e| format!("Failed to notify overlay frontend: {e}"))?;

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
pub fn present_overlay(app_handle: &AppHandle, state: &AppState) -> Result<(), String> {
    let window = app_handle
        .get_webview_window(OVERLAY_LABEL)
        .ok_or_else(|| "Overlay window not found".to_string())?;

    if !state.is_session_active() {
        return Err("No active capture session".to_string());
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
