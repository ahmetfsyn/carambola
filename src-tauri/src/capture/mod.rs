use crate::app_state::{CapturedFrame, DisplayBounds, DisplayInfo};
use image::{codecs::png::PngEncoder, ExtendedColorType, ImageEncoder};
use log::info;
use std::io::Cursor;
use xcap::Monitor;

pub fn capture_screen() -> Result<CapturedFrame, String> {
    let monitors = Monitor::all().map_err(|e| format!("Failed to enumerate monitors: {e}"))?;
    if monitors.is_empty() {
        return Err("No active displays found".to_string());
    }

    // Prefer primary monitor, otherwise pick the first available
    let monitor = monitors
        .iter()
        .find(|m| m.is_primary())
        .unwrap_or(&monitors[0]);

    info!(
        "Capturing monitor: {} ({}x{})",
        monitor.name(),
        monitor.width(),
        monitor.height()
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

    let x = monitor.x() as f64;
    let y = monitor.y() as f64;
    let physical_w = width as f64;
    let physical_h = height as f64;

    let display_info = DisplayInfo {
        id: monitor.id(),
        name: monitor.name().to_string(),
        is_primary: monitor.is_primary(),
        scale_factor: 1.0,
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
        scale_factor: 1.0,
        rgba,
        png_bytes,
        display_info,
    })
}
