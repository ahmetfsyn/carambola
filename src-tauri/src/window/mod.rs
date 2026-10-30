use crate::app_state::{AppState, DisplayBounds};
use crate::capture::capture_screen;
use log::{error, info, warn};
use serde::Serialize;
use std::time::Instant;
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, WebviewWindow};

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
    Ok(())
}

/// Applies the frameless, always-on-top, full-monitor geometry required for a
/// Lightshot-style overlay.
///
/// The position/size are applied in **physical** pixels straight from the
/// captured monitor rectangle, so the overlay exactly covers the target display
/// regardless of Windows DPI scaling and no matter what its native resolution is.
pub fn apply_overlay_geometry(
    window: &WebviewWindow,
    bounds: &DisplayBounds,
) -> Result<(), String> {
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
    Ok(())
}

/// On macOS a borderless top-most window still sits below the menu bar and
/// Dock. Native "simple fullscreen" covers the whole screen without switching
/// Spaces, which is the macOS equivalent of the Windows full-monitor overlay.
/// It is best-effort: if it fails we keep the explicit physical geometry.
#[cfg(target_os = "macos")]
fn enter_native_fullscreen(window: &WebviewWindow) {
    if let Err(e) = window.set_simple_fullscreen(true) {
        warn!("Failed to enter native fullscreen: {e}");
    }
}

#[cfg(not(target_os = "macos"))]
fn enter_native_fullscreen(_window: &WebviewWindow) {}

/// Counterpart to [`enter_native_fullscreen`], called when the overlay hides so
/// later geometry changes take effect on the next activation.
pub fn leave_native_fullscreen(window: &WebviewWindow) {
    #[cfg(target_os = "macos")]
    {
        let _ = window.set_simple_fullscreen(false);
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = window;
    }
}

/// Blocks until the desktop composition pass has consumed pending window
/// updates. Only used when the overlay is already visible and must be excluded
/// from the next capture.
#[cfg(target_os = "windows")]
fn flush_composition() {
    use windows::Win32::Graphics::Dwm::DwmFlush;
    unsafe {
        let _ = DwmFlush();
    }
}

#[cfg(not(target_os = "windows"))]
fn flush_composition() {}

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
pub fn activate_overlay(app_handle: &AppHandle, state: &AppState) -> Result<(), String> {
    let activation_started = Instant::now();
    info!("Activating screenshot overlay...");

    let window = app_handle
        .get_webview_window(OVERLAY_LABEL)
        .ok_or_else(|| "Overlay window not found".to_string())?;

    // If the overlay is currently on screen, remove it before capturing so the
    // frozen frame contains the real desktop rather than our own UI. DwmFlush
    // waits for the window removal to reach the screen instead of sleeping.
    if state.is_overlay_visible() {
        leave_native_fullscreen(&window);
        let _ = window.hide();
        state.set_overlay_visible(false);
        flush_composition();
    }

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

    info!(
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

    if state.get_frame().is_none() {
        return Err("No captured frame is ready".to_string());
    }

    enter_native_fullscreen(&window);
    window
        .show()
        .map_err(|e| format!("Failed to show overlay: {e}"))?;
    let _ = window.set_focus();
    state.set_overlay_visible(true);

    info!("[perf] overlay presented (window visible and focused)");
    Ok(())
}
