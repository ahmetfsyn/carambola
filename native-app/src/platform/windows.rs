//! Windows DPI process initialization. This must run before Slint creates Winit windows.

use windows::Win32::UI::HiDpi::{
    SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};

use crate::core::frame::CapturedFrame;

/// Record what Windows reports inside the application's UI thread. This is
/// deliberately observational: ShowCursor changes a display counter and
/// cannot be used as a harmless query.
pub fn diagnose_cursor(stage: &str) {
    use std::io::Write;
    use windows::Win32::UI::WindowsAndMessaging::{
        GetCursorInfo, GetForegroundWindow, LoadCursorW, WindowFromPoint, CURSORINFO,
        CURSOR_SHOWING, IDC_ARROW, IDC_CROSS, IDC_HAND,
    };

    if std::env::var_os("CARAMBOLA_CURSOR_DIAGNOSTICS").is_none() {
        return;
    }
    let mut cursor = CURSORINFO {
        cbSize: std::mem::size_of::<CURSORINFO>() as u32,
        ..Default::default()
    };
    let snapshot = match unsafe { GetCursorInfo(&mut cursor) } {
        Ok(()) => {
            let foreground = unsafe { GetForegroundWindow() };
            let under_pointer = unsafe { WindowFromPoint(cursor.ptScreenPos) };
            let named_cursor = [
                ("arrow", IDC_ARROW),
                ("crosshair", IDC_CROSS),
                ("pointer", IDC_HAND),
            ]
            .into_iter()
            .find_map(|(name, resource)| {
                (unsafe { LoadCursorW(None, resource) }.ok() == Some(cursor.hCursor))
                    .then_some(name)
            })
            .unwrap_or("other");
            format!(
                "showing={} flags={} handle={:?} resource={} position=({}, {}) foreground={:?} under_pointer={:?}",
                cursor.flags.0 & CURSOR_SHOWING.0 == CURSOR_SHOWING.0,
                cursor.flags.0,
                cursor.hCursor,
                named_cursor,
                cursor.ptScreenPos.x,
                cursor.ptScreenPos.y,
                foreground,
                under_pointer,
            )
        }
        Err(error) => format!("GetCursorInfo failed: {error}"),
    };
    let directory = std::env::temp_dir().join("carambola-native");
    if std::fs::create_dir_all(&directory).is_ok() {
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(directory.join("cursor-diagnostic.log"))
        {
            let _ = writeln!(
                file,
                "{} thread={:?} {stage}: {snapshot}",
                std::process::id(),
                std::thread::current().id(),
            );
        }
    }
}

pub fn diagnose_cursor_motion() {
    use std::sync::atomic::{AtomicIsize, Ordering};
    use windows::Win32::UI::WindowsAndMessaging::{GetCursorInfo, CURSORINFO};

    if std::env::var_os("CARAMBOLA_CURSOR_DIAGNOSTICS").is_none() {
        return;
    }
    static LAST_HANDLE: AtomicIsize = AtomicIsize::new(-1);
    let mut cursor = CURSORINFO {
        cbSize: std::mem::size_of::<CURSORINFO>() as u32,
        ..Default::default()
    };
    if unsafe { GetCursorInfo(&mut cursor) }.is_ok() {
        let state = (cursor.hCursor.0 as isize) ^ (cursor.flags.0 as isize);
        if LAST_HANDLE.swap(state, Ordering::Relaxed) != state {
            diagnose_cursor("pointer state changed");
        }
    }
}

pub fn initialize() -> Result<(), String> {
    // On Windows 10 version 1703 and later this opts the process into the
    // per-monitor-v2 coordinate contract before any HWND is created.
    unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) }
        .map_err(|error| format!("Could not enable Per-Monitor DPI Awareness V2: {error}"))
}

/// Capture the primary monitor's current desktop pixels through GDI.
/// The process is Per-Monitor-V2 aware before this function is called, so the
/// screen metrics and BitBlt source coordinates describe physical pixels.
/// Protected surfaces and secure-desktop content remain subject to Windows'
/// normal capture restrictions.
pub fn capture_primary() -> Result<CapturedFrame, String> {
    use windows::Win32::Graphics::Gdi::{
        BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC,
        GetDIBits, ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS, HGDIOBJ,
        SRCCOPY,
    };
    use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};

    let width = unsafe { GetSystemMetrics(SM_CXSCREEN) };
    let height = unsafe { GetSystemMetrics(SM_CYSCREEN) };
    if width <= 0 || height <= 0 {
        return Err("Windows reported an empty primary monitor".into());
    }

    unsafe {
        let screen_dc = GetDC(None);
        if screen_dc.is_invalid() {
            return Err("GetDC failed while capturing the primary monitor".into());
        }
        let memory_dc = CreateCompatibleDC(Some(screen_dc));
        if memory_dc.is_invalid() {
            ReleaseDC(None, screen_dc);
            return Err("CreateCompatibleDC failed while capturing the primary monitor".into());
        }
        let bitmap = CreateCompatibleBitmap(screen_dc, width, height);
        if bitmap.is_invalid() {
            let _ = DeleteDC(memory_dc);
            ReleaseDC(None, screen_dc);
            return Err("CreateCompatibleBitmap failed while capturing the primary monitor".into());
        }
        let previous = SelectObject(memory_dc, HGDIOBJ(bitmap.0));

        let captured = (|| -> Result<Vec<u8>, String> {
            BitBlt(
                memory_dc,
                0,
                0,
                width,
                height,
                Some(screen_dc),
                0,
                0,
                SRCCOPY,
            )
            .map_err(|error| {
                format!("BitBlt failed while capturing the primary monitor: {error}")
            })?;

            // GetDIBits requires the bitmap to be deselected from every DC.
            let _ = SelectObject(memory_dc, previous);

            let mut info = BITMAPINFO::default();
            info.bmiHeader = BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: 0,
                ..Default::default()
            };
            let mut bgra = vec![0u8; width as usize * height as usize * 4];
            let rows = GetDIBits(
                memory_dc,
                bitmap,
                0,
                height as u32,
                Some(bgra.as_mut_ptr().cast()),
                &mut info,
                DIB_RGB_COLORS,
            );
            if rows != height {
                return Err(format!("GetDIBits returned {rows} rows, expected {height}"));
            }
            Ok(bgra)
        })();

        SelectObject(memory_dc, previous);
        let _ = DeleteObject(HGDIOBJ(bitmap.0));
        let _ = DeleteDC(memory_dc);
        ReleaseDC(None, screen_dc);

        CapturedFrame::from_bgra32(width as u32, height as u32, captured?).map_err(str::to_owned)
    }
}

/// Hide the settings HWND synchronously, then synchronize with DWM before the
/// capture worker reads desktop pixels.
pub fn prepare_capture() -> Result<(), String> {
    use windows::core::w;
    use windows::Win32::{
        Foundation::HWND,
        Graphics::Dwm::DwmFlush,
        UI::WindowsAndMessaging::{
            FindWindowW, GetWindowThreadProcessId, IsWindowVisible, SetWindowDisplayAffinity,
            ShowWindow, SW_HIDE, WDA_EXCLUDEFROMCAPTURE,
        },
    };

    let window: HWND = unsafe {
        FindWindowW(None, w!("Carambola — Native Screenshot Annotation"))
            .map_err(|error| format!("Could not locate the native settings window: {error}"))?
    };
    let mut process_id = 0u32;
    unsafe { GetWindowThreadProcessId(window, Some(&mut process_id)) };
    if process_id != std::process::id() {
        return Err("The located settings window belongs to a different process".into());
    }
    unsafe { SetWindowDisplayAffinity(window, WDA_EXCLUDEFROMCAPTURE) }
        .map_err(|error| format!("Could not exclude the settings window from capture: {error}"))?;
    let _ = unsafe { ShowWindow(window, SW_HIDE) };
    if unsafe { IsWindowVisible(window).as_bool() } {
        return Err("Windows did not hide the native settings window before capture".into());
    }
    unsafe { DwmFlush() }.map_err(|error| format!("DwmFlush failed before capture: {error}"))
}

/// Restore the settings HWND after capture. Slint keeps the settings window
/// logically visible while this native HWND is hidden, keeping its event loop
/// alive before the selection overlay is shown.
pub fn restore_settings() -> Result<(), String> {
    use windows::core::w;
    use windows::Win32::{
        Foundation::HWND,
        UI::WindowsAndMessaging::{
            FindWindowW, GetWindowThreadProcessId, SetWindowDisplayAffinity, ShowWindow, SW_SHOW,
            WDA_NONE,
        },
    };

    let window: HWND = unsafe {
        FindWindowW(None, w!("Carambola — Native Screenshot Annotation"))
            .map_err(|error| format!("Could not locate the native settings window: {error}"))?
    };
    let mut process_id = 0u32;
    unsafe { GetWindowThreadProcessId(window, Some(&mut process_id)) };
    if process_id != std::process::id() {
        return Err("The located settings window belongs to a different process".into());
    }
    unsafe { SetWindowDisplayAffinity(window, WDA_NONE) }
        .map_err(|error| format!("Could not reset settings-window capture affinity: {error}"))?;
    let _ = unsafe { ShowWindow(window, SW_SHOW) };
    Ok(())
}

pub fn activate_overlay() -> Result<(), String> {
    use windows::core::w;
    use windows::Win32::{
        Foundation::HWND,
        UI::{
            Input::KeyboardAndMouse::{SetActiveWindow, SetFocus},
            WindowsAndMessaging::{FindWindowW, GetWindowThreadProcessId, SetForegroundWindow},
        },
    };

    let window: HWND = unsafe {
        FindWindowW(None, w!("Carambola screenshot selection"))
            .map_err(|error| format!("Could not locate the selection overlay window: {error}"))?
    };
    let mut process_id = 0u32;
    unsafe { GetWindowThreadProcessId(window, Some(&mut process_id)) };
    if process_id != std::process::id() {
        return Err("The located selection overlay belongs to a different process".into());
    }
    unsafe {
        let _ = SetForegroundWindow(window);
        let _ = SetActiveWindow(window);
        let _ = SetFocus(Some(window));
    }
    Ok(())
}
