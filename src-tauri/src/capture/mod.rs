use crate::app_state::{CapturedFrame, DisplayBounds, DisplayInfo};
use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use image::{ExtendedColorType, ImageEncoder};
use log::{debug, info, warn};
use std::io::Cursor;
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use xcap::Monitor;

/// Wall-clock UNIX epoch milliseconds. Shared between the native and webview
/// sides so latency can be measured across the IPC boundary.
pub fn now_epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

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

/// Grabs the monitor pixels directly into an RGBA buffer.
///
/// On Windows this uses a single GDI `BitBlt` + `GetDIBits` pass. It avoids the
/// per-pixel OS-version lookup performed by the `xcap` capture helper, which is
/// the dominant cost of the previous capture path on some machines.
#[cfg(target_os = "windows")]
fn capture_monitor_rgba(monitor: &Monitor) -> Result<(Vec<u8>, u32, u32), String> {
    match gdi_capture(monitor.x(), monitor.y(), monitor.width(), monitor.height()) {
        Ok(frame) => Ok(frame),
        Err(err) => {
            warn!("GDI capture failed ({err}); falling back to xcap capture");
            let image = monitor
                .capture_image()
                .map_err(|e| format!("Failed to capture display image: {e}"))?;
            let (width, height) = image.dimensions();
            let mut rgba = image.into_raw();
            for px in rgba.chunks_exact_mut(4) {
                px[3] = 255;
            }
            Ok((rgba, width, height))
        }
    }
}

#[cfg(target_os = "windows")]
fn gdi_capture(x: i32, y: i32, width: u32, height: u32) -> Result<(Vec<u8>, u32, u32), String> {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Gdi::{
        BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC,
        GetDIBits, ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS, HGDIOBJ,
        SRCCOPY,
    };

    if width == 0 || height == 0 {
        return Err("Monitor has an empty capture area".to_string());
    }

    unsafe {
        let desktop = HWND::default();
        let screen_dc = GetDC(desktop);
        if screen_dc.is_invalid() {
            return Err("GetDC failed".to_string());
        }

        let mem_dc = CreateCompatibleDC(screen_dc);
        if mem_dc.is_invalid() {
            ReleaseDC(desktop, screen_dc);
            return Err("CreateCompatibleDC failed".to_string());
        }

        let bitmap = CreateCompatibleBitmap(screen_dc, width as i32, height as i32);
        if bitmap.is_invalid() {
            let _ = DeleteDC(mem_dc);
            ReleaseDC(desktop, screen_dc);
            return Err("CreateCompatibleBitmap failed".to_string());
        }

        let previous = SelectObject(mem_dc, HGDIOBJ(bitmap.0));

        let capture_result = (|| -> Result<Vec<u8>, String> {
            BitBlt(
                mem_dc,
                0,
                0,
                width as i32,
                height as i32,
                screen_dc,
                x,
                y,
                SRCCOPY,
            )
            .map_err(|e| format!("BitBlt failed: {e}"))?;

            let mut info = BITMAPINFO::default();
            info.bmiHeader = BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width as i32,
                // Negative height requests a top-down DIB, matching the
                // webview's coordinate space.
                biHeight: -(height as i32),
                biPlanes: 1,
                biBitCount: 32,
                biCompression: 0,
                ..Default::default()
            };

            let mut buffer = vec![0u8; (width as usize * height as usize * 4) as usize];
            let scanlines = GetDIBits(
                mem_dc,
                bitmap,
                0,
                height,
                Some(buffer.as_mut_ptr().cast()),
                &mut info,
                DIB_RGB_COLORS,
            );
            if scanlines <= 0 {
                return Err("GetDIBits returned no scanlines".to_string());
            }

            // GDI returns BGRA with an undefined alpha channel. Convert to RGBA
            // once, in a single pass, and make the screenshot opaque.
            for px in buffer.chunks_exact_mut(4) {
                px.swap(0, 2);
                px[3] = 255;
            }
            Ok(buffer)
        })();

        SelectObject(mem_dc, previous);
        let _ = DeleteObject(HGDIOBJ(bitmap.0));
        let _ = DeleteDC(mem_dc);
        ReleaseDC(desktop, screen_dc);

        capture_result.map(|rgba| (rgba, width, height))
    }
}

#[cfg(not(target_os = "windows"))]
fn capture_monitor_rgba(monitor: &Monitor) -> Result<(Vec<u8>, u32, u32), String> {
    let image = monitor.capture_image().map_err(|e| {
        #[cfg(target_os = "macos")]
        {
            format!(
                "Failed to capture display image: {e}. Grant Carambola Screen Recording \
                 permission in System Settings > Privacy & Security > Screen Recording, then \
                 restart the app."
            )
        }
        #[cfg(target_os = "linux")]
        {
            format!(
                "Failed to capture display image: {e}. On Wayland, screen capture requires \
                 the desktop screenshot portal and permission; X11 sessions are also supported."
            )
        }
        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        {
            format!("Failed to capture display image: {e}")
        }
    })?;
    let (width, height) = image.dimensions();
    let mut rgba = image.into_raw();
    for px in rgba.chunks_exact_mut(4) {
        px[3] = 255;
    }
    Ok((rgba, width, height))
}

fn encode_png(rgba: &[u8], width: u32, height: u32) -> Result<Vec<u8>, String> {
    let mut png_bytes = Vec::new();
    // Lossless, but with a single cheap scanline filter and fast deflate level.
    // The visual result is bit-identical to the default encoder; only the
    // compression effort (and therefore latency) changes.
    PngEncoder::new_with_quality(
        Cursor::new(&mut png_bytes),
        CompressionType::Fast,
        FilterType::Up,
    )
    .write_image(rgba, width, height, ExtendedColorType::Rgba8)
    .map_err(|e| format!("Failed to encode captured PNG: {e}"))?;
    Ok(png_bytes)
}

pub fn capture_screen() -> Result<CapturedFrame, String> {
    let triggered_at_ms = now_epoch_ms();
    let started = Instant::now();

    let monitors = Monitor::all().map_err(|e| format!("Failed to enumerate monitors: {e}"))?;
    if monitors.is_empty() {
        return Err("No active displays found".to_string());
    }
    let enumerate_ms = started.elapsed().as_secs_f64() * 1000.0;

    let monitor = select_monitor(&monitors);

    info!(
        "Capturing monitor: {} ({}x{} @ {}x)",
        monitor.name(),
        monitor.width(),
        monitor.height(),
        monitor.scale_factor()
    );

    let capture_started = Instant::now();
    let (rgba, width, height) = capture_monitor_rgba(&monitor)?;
    let capture_ms = capture_started.elapsed().as_secs_f64() * 1000.0;

    let encode_started = Instant::now();
    let png_bytes = encode_png(&rgba, width, height)?;
    let encode_ms = encode_started.elapsed().as_secs_f64() * 1000.0;

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

    debug!(
        "[perf] capture pipeline: enumerate={:.1}ms capture={:.1}ms encode={:.1}ms png={}B {}x{}",
        enumerate_ms,
        capture_ms,
        encode_ms,
        png_bytes.len(),
        width,
        height
    );

    Ok(CapturedFrame {
        width,
        height,
        scale_factor,
        rgba,
        png_bytes,
        display_info,
        triggered_at_ms,
        capture_ms,
        encode_ms,
    })
}
