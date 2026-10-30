use crate::app_state::{CapturedFrame, DisplayBounds, DisplayInfo};
use image::{codecs::png::PngEncoder, ExtendedColorType, ImageEncoder};
use log::info;
use std::io::Cursor;
use xcap::Monitor;

/// Returns the current mouse position in physical (virtual screen) coordinates.
#[cfg(target_os = "windows")]
fn cursor_position() -> Option<(i32, i32)> {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

    let mut point = POINT::default();
    unsafe { GetCursorPos(&mut point).ok()? };
    Some((point.x, point.y))
}

/// Picks the monitor to capture. The monitor under the cursor is preferred so a
/// multi-monitor setup captures the screen the user is actually working on;
/// otherwise the primary monitor is used, and finally the first available one.
fn select_monitor(monitors: &[Monitor]) -> Monitor {
    #[cfg(target_os = "windows")]
    {
        if let Some((x, y)) = cursor_position() {
            if let Ok(monitor) = Monitor::from_point(x, y) {
                return monitor;
            }
        }
    }

    monitors
        .iter()
        .find(|m| m.is_primary())
        .cloned()
        .unwrap_or_else(|| monitors[0].clone())
}

pub fn capture_screen() -> Result<CapturedFrame, String> {
    let monitors = Monitor::all().map_err(|e| format!("Failed to enumerate monitors: {e}"))?;
    if monitors.is_empty() {
        return Err("No active displays found".to_string());
    }

    let monitor = select_monitor(&monitors);

    info!(
        "Capturing monitor: {} ({}x{} @ {}x)",
        monitor.name(),
        monitor.width(),
        monitor.height(),
        monitor.scale_factor()
    );

    let image = monitor
        .capture_image()
        .map_err(|e| format!("Failed to capture display image: {e}"))?;

    let width = image.width();
    let height = image.height();
    let rgba = image.into_raw();

    // Fast PNG encoding in-memory for webview consumption
    let mut png_bytes = Vec::new();
    let encoder = PngEncoder::new(Cursor::new(&mut png_bytes));
    encoder
        .write_image(&rgba, width, height, ExtendedColorType::Rgba8)
        .map_err(|e| format!("Failed to encode captured PNG: {e}"))?;

    // The overlay window must cover the *entire* physical monitor rectangle.
    let physical_w = monitor.width() as f64;
    let physical_h = monitor.height() as f64;
    let x = monitor.x() as f64;
    let y = monitor.y() as f64;
    // Real DPI scale for this monitor. Used by the frontend to convert the
    // CSS-pixel selection rectangle into screenshot (physical) pixels.
    let scale_factor = monitor.scale_factor() as f64;

    let display_info = DisplayInfo {
        id: monitor.id(),
        name: monitor.name().to_string(),
        is_primary: monitor.is_primary(),
        scale_factor,
        bounds: DisplayBounds {
            x,
            y,
            width: physical_w,
            height: physical_h,
        },
        physical_width: physical_w,
        physical_height: physical_h,
    };

    Ok(CapturedFrame {
        width,
        height,
        scale_factor,
        rgba,
        png_bytes,
        display_info,
    })
}
