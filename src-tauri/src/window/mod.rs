use crate::app_state::{AppState, DisplayBounds};
use crate::capture::capture_screen;
use log::{error, info, warn};
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, WebviewWindow};

/// Label of the single, pre-created screenshot overlay window.
pub const OVERLAY_LABEL: &str = "overlay-main";

/// Applies the frameless, always-on-top, full-monitor geometry required for a
/// Lightshot-style overlay.
///
/// The position/size are applied in **physical** pixels straight from the
/// captured monitor rectangle, so the overlay exactly covers the target display
/// regardless of Windows DPI scaling and no matter what its native resolution is.
pub fn apply_overlay_geometry(window: &WebviewWindow, bounds: &DisplayBounds) -> Result<(), String> {
    // No title bar, frame, resize borders or drop shadow.
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

/// Called once during application setup. Sizes the hidden overlay to the
/// captured monitor so it is never presented at the default window size and so
/// the webview lays out at the correct dimensions ahead of the first capture.
pub fn prime_overlay(app_handle: &AppHandle, state: &AppState) {
    let Some(window) = app_handle.get_webview_window(OVERLAY_LABEL) else {
        return;
    };

    if let Some(frame) = state.get_frame() {
        if let Err(e) = apply_overlay_geometry(&window, &frame.display_info.bounds) {
            warn!("Failed to prime overlay geometry: {e}");
        }
    }

    // Ensure the overlay stays hidden until the user triggers a capture.
    let _ = window.hide();
}

pub fn activate_overlay(app_handle: &AppHandle, state: &AppState) -> Result<(), String> {
    info!("Activating screenshot overlay...");

    // 1. Capture the target monitor immediately and freeze the frame in memory.
    //    The desktop is captured exactly once per activation; it is never
    //    re-captured while the overlay is shown.
    let frame = capture_screen().map_err(|e| {
        error!("Screen capture failed: {e}");
        e
    })?;

    // TEMP diagnostic: dump the captured frame for offline comparison.
    #[cfg(debug_assertions)]
    let _ = std::fs::write(
        r"C:\Users\furka\AppData\Local\Temp\kilo\frame.png",
        &frame.png_bytes,
    );

    let bounds = frame.display_info.bounds.clone();
    let frame_scale = frame.display_info.scale_factor;
    let frame_dims = (frame.width, frame.height);
    state.set_frame(frame);
    state.set_overlay_visible(true);

    // 2. Present the pre-created, frameless, fullscreen overlay window.
    let window = app_handle
        .get_webview_window(OVERLAY_LABEL)
        .ok_or_else(|| "Overlay window not found".to_string())?;

    // Geometry is applied *before* the window becomes visible so the user never
    // sees an application window at the wrong size.
    apply_overlay_geometry(&window, &bounds)?;
    enter_native_fullscreen(&window);

    window
        .show()
        .map_err(|e| format!("Failed to show overlay: {e}"))?;
    let _ = window.set_focus();

    // Temporary pipeline diagnostics.
    if let Ok(pos) = window.outer_position() {
        info!("[diag] native outer_position = ({}, {})", pos.x, pos.y);
    }
    if let Ok(size) = window.inner_size() {
        info!(
            "[diag] native inner_size = {}x{} (physical)",
            size.width, size.height
        );
    }
    if let Ok(sf) = window.scale_factor() {
        info!("[diag] native window scale_factor = {sf}");
    }
    info!(
        "[diag] captured frame = {}x{} px, bounds = {}x{} at ({}, {}), scale_factor = {}",
        frame_dims.0, frame_dims.1, bounds.width, bounds.height, bounds.x, bounds.y, frame_scale
    );

    // 3. Tell the frontend a fresh frozen frame is ready to be displayed.
    let _ = window.emit("capture_triggered", ());

    info!("Overlay presented successfully");
    Ok(())
}
