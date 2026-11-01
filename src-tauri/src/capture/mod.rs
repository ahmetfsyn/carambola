use crate::app_state::{CapturedFrame, DisplayBounds, DisplayInfo};
use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use image::{ExtendedColorType, ImageEncoder};
#[cfg(target_os = "windows")]
use log::warn;
use log::{debug, info};
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

/// Returns the center of the frontmost ordinary application window. This keeps
/// capture on the display containing the app the user can currently see, even
/// when the pointer is resting on another monitor.
#[cfg(target_os = "macos")]
fn frontmost_application_window_center() -> Option<(i32, i32)> {
    use core_foundation::{
        base::TCFType,
        dictionary::{CFDictionaryGetValue, CFDictionaryRef},
        number::{kCFNumberIntType, CFNumberGetValue, CFNumberRef},
        string::CFString,
    };
    use core_graphics::{
        geometry::CGRect,
        window::{copy_window_info, kCGNullWindowID, kCGWindowListOptionOnScreenOnly},
    };

    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGRectMakeWithDictionaryRepresentation(dict: CFDictionaryRef, rect: *mut CGRect)
            -> bool;
    }

    unsafe fn number_value(dictionary: CFDictionaryRef, key: &str) -> Option<i32> {
        let key = CFString::new(key);
        let value = CFDictionaryGetValue(dictionary, key.as_CFTypeRef());
        if value.is_null() {
            return None;
        }
        let mut result = 0i32;
        let valid = CFNumberGetValue(
            value as CFNumberRef,
            kCFNumberIntType,
            (&mut result as *mut i32).cast(),
        );
        valid.then_some(result)
    }

    let windows = copy_window_info(kCGWindowListOptionOnScreenOnly, kCGNullWindowID)?;
    for window in windows.get_all_values() {
        let dictionary = window as CFDictionaryRef;
        if unsafe { number_value(dictionary, "kCGWindowLayer") } != Some(0)
            || unsafe { number_value(dictionary, "kCGWindowOwnerPID") }
                == Some(std::process::id() as i32)
        {
            continue;
        }

        let bounds_key = CFString::new("kCGWindowBounds");
        let bounds = unsafe { CFDictionaryGetValue(dictionary, bounds_key.as_CFTypeRef()) };
        if bounds.is_null() {
            continue;
        }
        let mut rect = CGRect::default();
        if !unsafe { CGRectMakeWithDictionaryRepresentation(bounds as CFDictionaryRef, &mut rect) }
            || rect.size.width <= 0.0
            || rect.size.height <= 0.0
        {
            continue;
        }

        let center_x = (rect.origin.x + rect.size.width / 2.0).round() as i32;
        let center_y = (rect.origin.y + rect.size.height / 2.0).round() as i32;
        return Some((center_x, center_y));
    }
    None
}

#[cfg(target_os = "macos")]
fn macos_capture_permission() -> Result<(), String> {
    use std::sync::atomic::{AtomicBool, Ordering};
    static REQUESTED: AtomicBool = AtomicBool::new(false);

    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGPreflightScreenCaptureAccess() -> bool;
        fn CGRequestScreenCaptureAccess() -> bool;
    }

    let granted = unsafe { CGPreflightScreenCaptureAccess() };
    info!("[capture] screen_recording_preflight={granted}");
    if granted {
        return Ok(());
    }

    if !REQUESTED.swap(true, Ordering::SeqCst) {
        let requested = unsafe { CGRequestScreenCaptureAccess() };
        info!("[capture] screen_recording_request_result={requested}");
        if requested && unsafe { CGPreflightScreenCaptureAccess() } {
            return Ok(());
        }
    }

    Err("Screen Recording access is not available to this running Carambola app. Enable Carambola in System Settings > Privacy & Security > Screen & System Audio Recording, then quit and restart Carambola.".to_string())
}

/// Log the running bundle and non-prompting authorization state at startup.
#[cfg(target_os = "macos")]
pub fn log_macos_startup_diagnostics() {
    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGPreflightScreenCaptureAccess() -> bool;
    }
    log_macos_identity();
    info!("[capture] startup_screen_recording_preflight={}", unsafe {
        CGPreflightScreenCaptureAccess()
    });
}

#[cfg(target_os = "macos")]
fn log_macos_identity() {
    use core_foundation::{
        base::TCFType,
        string::{CFString, CFStringRef},
    };
    use std::ffi::c_void;
    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFBundleGetMainBundle() -> *const c_void;
        fn CFBundleGetIdentifier(bundle: *const c_void) -> CFStringRef;
    }
    let bundle_id = unsafe {
        let bundle = CFBundleGetMainBundle();
        let identifier = if bundle.is_null() {
            std::ptr::null()
        } else {
            CFBundleGetIdentifier(bundle)
        };
        if identifier.is_null() {
            "<none>".to_string()
        } else {
            CFString::wrap_under_get_rule(identifier).to_string()
        }
    };
    info!("[capture] timestamp_ms={} bundle_id={bundle_id:?} executable={:?} api=CGWindowListCreateImage", now_epoch_ms(), std::env::current_exe());
}

/// Core Graphics and xcap use the same global display-point coordinate space
/// on macOS. This is only a fallback if the frontmost app has no ordinary
/// visible window (for example, when the desktop itself is active).
#[cfg(target_os = "macos")]
fn cursor_position() -> Option<(i32, i32)> {
    use core_graphics::event::CGEvent;
    use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};

    let source = CGEventSource::new(CGEventSourceStateID::CombinedSessionState).ok()?;
    let point = CGEvent::new(source).ok()?.location();
    Some((point.x as i32, point.y as i32))
}

/// Picks the monitor to capture. The monitor under the cursor is preferred so a
/// multi-monitor setup captures the screen the user is actually working on;
/// otherwise the primary monitor is used, and finally the first available one.
fn select_monitor(monitors: &[Monitor]) -> Monitor {
    #[cfg(target_os = "macos")]
    {
        if let Some((x, y)) = frontmost_application_window_center() {
            if let Ok(monitor) = Monitor::from_point(x, y) {
                return monitor;
            }
        }
        if let Some((x, y)) = cursor_position() {
            if let Ok(monitor) = Monitor::from_point(x, y) {
                return monitor;
            }
        }
    }

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
            format!("macOS CoreGraphics display capture failed: {e}")
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
    if width == 0 || height == 0 {
        return Err("Display capture returned an empty image".to_string());
    }
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

    #[cfg(target_os = "macos")]
    {
        log_macos_identity();
        macos_capture_permission()?;
    }

    let monitors = Monitor::all().map_err(|e| format!("Failed to enumerate monitors: {e}"))?;
    if monitors.is_empty() {
        return Err("No active displays found".to_string());
    }
    let enumerate_ms = started.elapsed().as_secs_f64() * 1000.0;

    let monitor = select_monitor(&monitors);

    info!(
        "Capturing monitor: id={} {} ({}x{} @ {}x, origin={},{} )",
        monitor.id(),
        monitor.name(),
        monitor.width(),
        monitor.height(),
        monitor.scale_factor(),
        monitor.x(),
        monitor.y()
    );

    let capture_started = Instant::now();
    let (rgba, width, height) = capture_monitor_rgba(&monitor)?;
    #[cfg(target_os = "macos")]
    {
        let mut min = [u8::MAX; 3];
        let mut max = [u8::MIN; 3];
        for pixel in rgba.chunks_exact(4).step_by(256) {
            for channel in 0..3 {
                min[channel] = min[channel].min(pixel[channel]);
                max[channel] = max[channel].max(pixel[channel]);
            }
        }
        let uniform = (0..3).all(|channel| max[channel].saturating_sub(min[channel]) <= 2);
        info!("[capture] sampled_nearly_uniform={uniform}");
    }
    info!("[capture] image_width={width} image_height={height} bytes={} foreground_content=not_checked", rgba.len());
    let capture_ms = capture_started.elapsed().as_secs_f64() * 1000.0;

    let encode_started = Instant::now();
    let png_bytes = encode_png(&rgba, width, height)?;
    let encode_ms = encode_started.elapsed().as_secs_f64() * 1000.0;

    // xcap reports macOS monitor bounds in points, while capture_image returns
    // Retina pixels. Keep native bounds in points and report the actual image
    // dimensions separately for crop/export scaling.
    let native_w = monitor.width() as f64;
    let native_h = monitor.height() as f64;
    let physical_w = width as f64;
    let physical_h = height as f64;
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
            width: native_w,
            height: native_h,
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
