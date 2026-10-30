use crate::app_state::AppState;
use crate::capture::capture_screen;
use log::{error, info};
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize};

pub fn activate_overlay(app_handle: &AppHandle, state: &AppState) -> Result<(), String> {
    info!("Activating screenshot overlay...");

    // 1. Capture the screen
    let frame = capture_screen().map_err(|e| {
        error!("Screen capture failed: {e}");
        e
    })?;

    let bounds = frame.display_info.bounds.clone();
    state.set_frame(frame);
    state.set_overlay_visible(true);

    // 2. Position and display the pre-warmed window
    if let Some(window) = app_handle.get_webview_window("overlay-main") {
        // Match window position and size to the captured display
        let pos = PhysicalPosition::new(bounds.x as i32, bounds.y as i32);
        let size = PhysicalSize::new(bounds.width as u32, bounds.height as u32);

        let _ = window.set_position(pos);
        let _ = window.set_size(size);
        let _ = window.set_always_on_top(true);

        // Platform-specific styling tweaks
        #[cfg(target_os = "macos")]
        {
            // macOS transparent window setup if needed
        }

        let _ = window.show();
        let _ = window.set_focus();

        // 3. Emit event to notify frontend to refresh its canvas
        let _ = window.emit("capture_triggered", ());
        info!("Overlay presented successfully");
    } else {
        return Err("Overlay window not found".into());
    }

    Ok(())
}

