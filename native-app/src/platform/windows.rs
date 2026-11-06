//! Windows DPI process initialization. This must run before Slint creates Winit windows.

use windows::Win32::UI::HiDpi::{
    SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};

use crate::core::frame::CapturedFrame;

fn find_app_window(
    title: windows::core::PCWSTR,
) -> Result<windows::Win32::Foundation::HWND, String> {
    use windows::Win32::{
        Foundation::HWND,
        UI::WindowsAndMessaging::{FindWindowW, GetWindowThreadProcessId},
    };

    let window: HWND = unsafe { FindWindowW(None, title) }
        .map_err(|error| format!("Could not locate the Carambola window: {error}"))?;
    let mut process_id = 0u32;
    unsafe { GetWindowThreadProcessId(window, Some(&mut process_id)) };
    if process_id != std::process::id() {
        return Err("The located Carambola window belongs to another process".into());
    }
    Ok(window)
}

/// Display the standard Windows Save As dialog for a PNG file.
pub fn save_png_dialog() -> Result<Option<std::path::PathBuf>, String> {
    use std::{os::windows::ffi::OsStringExt, path::PathBuf};
    use windows::{
        core::{w, PCWSTR, PWSTR},
        Win32::UI::Controls::Dialogs::{
            CommDlgExtendedError, GetSaveFileNameW, OFN_EXPLORER, OFN_HIDEREADONLY,
            OFN_NOCHANGEDIR, OFN_OVERWRITEPROMPT, OFN_PATHMUSTEXIST, OPENFILENAMEW,
        },
    };

    let owner = find_app_window(w!("Carambola screenshot selection"))?;
    crate::platform::window_diagnostics::trace_native_operation("save_png_dialog.before", owner);
    let mut file_name = "Carambola Screenshot.png"
        .encode_utf16()
        .collect::<Vec<_>>();
    file_name.resize(32_768, 0);
    let filter = "PNG image (*.png)\0*.png\0\0"
        .encode_utf16()
        .collect::<Vec<_>>();
    let title = "Save annotated screenshot as PNG\0"
        .encode_utf16()
        .collect::<Vec<_>>();
    let default_extension = w!("png");
    let mut dialog = OPENFILENAMEW {
        lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
        hwndOwner: owner,
        lpstrFilter: PCWSTR(filter.as_ptr()),
        nFilterIndex: 1,
        lpstrFile: PWSTR(file_name.as_mut_ptr()),
        nMaxFile: file_name.len() as u32,
        lpstrTitle: PCWSTR(title.as_ptr()),
        Flags: OFN_EXPLORER
            | OFN_PATHMUSTEXIST
            | OFN_OVERWRITEPROMPT
            | OFN_NOCHANGEDIR
            | OFN_HIDEREADONLY,
        lpstrDefExt: default_extension,
        ..Default::default()
    };

    let accepted = unsafe { GetSaveFileNameW(&mut dialog) }.as_bool();
    crate::platform::window_diagnostics::trace_native_operation("save_png_dialog.after", owner);
    if !accepted {
        let error = unsafe { CommDlgExtendedError() }.0;
        if error == 0 {
            return Ok(None);
        }
        return Err(format!(
            "Windows Save As dialog failed (error 0x{error:08x})"
        ));
    }

    let length = file_name
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(file_name.len());
    let path = PathBuf::from(std::ffi::OsString::from_wide(&file_name[..length]));
    Ok(Some(crate::core::export::ensure_png_extension(path)?))
}

struct ClipboardMemory(Option<windows::Win32::Foundation::HGLOBAL>);

impl ClipboardMemory {
    fn new(handle: windows::Win32::Foundation::HGLOBAL) -> Self {
        Self(Some(handle))
    }

    fn handle(&self) -> windows::Win32::Foundation::HGLOBAL {
        self.0.expect("clipboard allocation is still owned")
    }

    fn transfer_to_clipboard(&mut self) {
        self.0 = None;
    }
}

impl Drop for ClipboardMemory {
    fn drop(&mut self) {
        if let Some(handle) = self.0.take() {
            let _ = unsafe { windows::Win32::Foundation::GlobalFree(Some(handle)) };
        }
    }
}

struct ClipboardLock(bool);

impl ClipboardLock {
    fn open(owner: windows::Win32::Foundation::HWND) -> Result<Self, String> {
        unsafe { windows::Win32::System::DataExchange::OpenClipboard(Some(owner)) }
            .map_err(|error| format!("Clipboard is busy or unavailable: {error}"))?;
        Ok(Self(true))
    }

    fn close(mut self) -> Result<(), String> {
        self.0 = false;
        unsafe { windows::Win32::System::DataExchange::CloseClipboard() }
            .map_err(|error| format!("Could not close the Windows clipboard: {error}"))
    }
}

impl Drop for ClipboardLock {
    fn drop(&mut self) {
        if self.0 {
            let _ = unsafe { windows::Win32::System::DataExchange::CloseClipboard() };
        }
    }
}

/// Place an owned CF_DIBV5 block on the Windows clipboard.
pub fn copy_image_to_clipboard(frame: &CapturedFrame) -> Result<(), String> {
    use std::ptr::copy_nonoverlapping;
    use windows::core::w;
    use windows::Win32::{
        Foundation::HANDLE,
        System::{DataExchange::EmptyClipboard, Memory::GlobalAlloc, Memory::GlobalLock},
    };

    const CF_DIBV5: u32 = 17;
    let dib = crate::core::clipboard::dib_v5_bytes(frame)?;
    let allocation =
        unsafe { GlobalAlloc(windows::Win32::System::Memory::GMEM_MOVEABLE, dib.len()) }
            .map_err(|error| format!("Could not allocate clipboard image memory: {error}"))?;
    let mut allocation = ClipboardMemory::new(allocation);
    let destination = unsafe { GlobalLock(allocation.handle()) };
    if destination.is_null() {
        return Err("Could not lock clipboard image memory".into());
    }
    unsafe { copy_nonoverlapping(dib.as_ptr(), destination.cast::<u8>(), dib.len()) };
    let _ = unsafe { windows::Win32::System::Memory::GlobalUnlock(allocation.handle()) };

    let owner = find_app_window(w!("Carambola — Native Screenshot Annotation"))?;
    let clipboard = ClipboardLock::open(owner)?;
    unsafe { EmptyClipboard() }
        .map_err(|error| format!("Could not clear the Windows clipboard: {error}"))?;
    let handle = HANDLE(allocation.handle().0);
    unsafe { windows::Win32::System::DataExchange::SetClipboardData(CF_DIBV5, Some(handle)) }
        .map_err(|error| format!("Could not copy the image to the Windows clipboard: {error}"))?;
    allocation.transfer_to_clipboard();
    clipboard.close()
}

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

struct MonitorEnumeration {
    displays: Vec<crate::core::display::DisplayMetadata>,
    error: Option<String>,
}

unsafe extern "system" fn enumerate_monitor_callback(
    monitor: windows::Win32::Graphics::Gdi::HMONITOR,
    _dc: windows::Win32::Graphics::Gdi::HDC,
    _rect: *mut windows::Win32::Foundation::RECT,
    data: windows::Win32::Foundation::LPARAM,
) -> windows::core::BOOL {
    use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MONITORINFOEXW};

    let enumeration = unsafe { &mut *(data.0 as *mut MonitorEnumeration) };
    let mut info = MONITORINFOEXW::default();
    info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
    if !unsafe { GetMonitorInfoW(monitor, &mut info.monitorInfo) }.as_bool() {
        enumeration.error = Some(format!(
            "GetMonitorInfoW failed while enumerating displays: {}",
            windows::core::Error::from_thread()
        ));
        return false.into();
    }

    let bounds = match crate::core::display::PhysicalBounds::new(
        info.monitorInfo.rcMonitor.left,
        info.monitorInfo.rcMonitor.top,
        info.monitorInfo.rcMonitor.right,
        info.monitorInfo.rcMonitor.bottom,
    ) {
        Ok(bounds) => bounds,
        Err(error) => {
            enumeration.error = Some(format!("Windows reported invalid display bounds: {error}"));
            return false.into();
        }
    };

    let device_name_end = info
        .szDevice
        .iter()
        .position(|character| *character == 0)
        .unwrap_or(info.szDevice.len());
    let device_name = String::from_utf16_lossy(&info.szDevice[..device_name_end]);

    let mut dpi_x = 0;
    let mut dpi_y = 0;
    if let Err(error) = unsafe {
        windows::Win32::UI::HiDpi::GetDpiForMonitor(
            monitor,
            windows::Win32::UI::HiDpi::MDT_EFFECTIVE_DPI,
            &mut dpi_x,
            &mut dpi_y,
        )
    } {
        enumeration.error = Some(format!(
            "Could not query effective DPI for {device_name}: {error}"
        ));
        return false.into();
    }
    if dpi_x == 0 || dpi_x != dpi_y {
        enumeration.error = Some(format!(
            "Windows reported invalid effective DPI for {device_name}: {dpi_x}x{dpi_y}"
        ));
        return false.into();
    }

    match crate::core::display::DisplayMetadata::new(
        device_name,
        bounds,
        dpi_x,
        info.monitorInfo.dwFlags & windows::Win32::UI::WindowsAndMessaging::MONITORINFOF_PRIMARY
            != 0,
    ) {
        Ok(display) => enumeration.displays.push(display),
        Err(error) => {
            enumeration.error = Some(format!("Invalid Windows display metadata: {error}"));
            return false.into();
        }
    }
    true.into()
}

/// Enumerate current Windows displays with signed physical virtual-desktop
/// bounds. Device names identify the current topology; callers must re-resolve
/// a selected ID before capture rather than persisting it across sessions.
pub fn enumerate_displays() -> Result<Vec<crate::core::display::DisplayMetadata>, String> {
    use windows::Win32::{Foundation::LPARAM, Graphics::Gdi::EnumDisplayMonitors};

    let mut enumeration = MonitorEnumeration {
        displays: Vec::new(),
        error: None,
    };
    let succeeded = unsafe {
        EnumDisplayMonitors(
            None,
            None,
            Some(enumerate_monitor_callback),
            LPARAM(&mut enumeration as *mut MonitorEnumeration as isize),
        )
    }
    .as_bool();

    if let Some(error) = enumeration.error {
        return Err(error);
    }
    if !succeeded {
        return Err(format!(
            "EnumDisplayMonitors failed: {}",
            windows::core::Error::from_thread()
        ));
    }
    if enumeration.displays.is_empty() {
        return Err("Windows did not report any connected displays".into());
    }

    enumeration.displays.sort_by(|left, right| {
        right
            .is_primary
            .cmp(&left.is_primary)
            .then_with(|| left.id.cmp(&right.id))
    });
    Ok(enumeration.displays)
}

/// Capture the requested display, defaulting to the primary display.
/// The process is Per-Monitor-V2 aware before this function is called, so
/// monitor bounds and BitBlt source coordinates describe physical pixels.
/// Protected surfaces and secure-desktop content remain subject to Windows'
/// normal capture restrictions.
pub fn capture_display(requested_id: Option<&str>) -> Result<CapturedFrame, String> {
    use crate::core::display::select_display;
    use windows::Win32::Graphics::Gdi::{
        BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC,
        GetDIBits, ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS, HGDIOBJ,
        SRCCOPY,
    };

    let displays = enumerate_displays()?;
    let selected = select_display(&displays, requested_id)?.clone();
    let width = i32::try_from(selected.bounds.width())
        .map_err(|_| "Selected display width exceeds GDI limits")?;
    let height = i32::try_from(selected.bounds.height())
        .map_err(|_| "Selected display height exceeds GDI limits")?;
    let source_x = selected.bounds.left;
    let source_y = selected.bounds.top;

    unsafe {
        let screen_dc = GetDC(None);
        if screen_dc.is_invalid() {
            return Err("GetDC failed while capturing the selected display".into());
        }
        let memory_dc = CreateCompatibleDC(Some(screen_dc));
        if memory_dc.is_invalid() {
            ReleaseDC(None, screen_dc);
            return Err("CreateCompatibleDC failed while capturing the selected display".into());
        }
        let bitmap = CreateCompatibleBitmap(screen_dc, width, height);
        if bitmap.is_invalid() {
            let _ = DeleteDC(memory_dc);
            ReleaseDC(None, screen_dc);
            return Err(
                "CreateCompatibleBitmap failed while capturing the selected display".into(),
            );
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
                source_x,
                source_y,
                SRCCOPY,
            )
            .map_err(|error| format!("BitBlt failed while capturing {}: {error}", selected.id))?;

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
            let buffer_len = (width as usize)
                .checked_mul(height as usize)
                .and_then(|pixels| pixels.checked_mul(4))
                .ok_or("Selected display image dimensions overflow addressable memory")?;
            let mut bgra = Vec::new();
            bgra.try_reserve_exact(buffer_len)
                .map_err(|error| format!("Could not allocate selected display pixels: {error}"))?;
            bgra.resize(buffer_len, 0);
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

        let bgra = captured?;
        let current_displays = enumerate_displays()?;
        let current = current_displays
            .iter()
            .find(|display| display.id == selected.id)
            .ok_or_else(|| {
                format!(
                    "Selected display '{}' disconnected during capture",
                    selected.id
                )
            })?;
        if current != &selected {
            return Err(format!(
                "Selected display '{}' changed bounds or DPI during capture",
                selected.id
            ));
        }

        CapturedFrame::from_bgra32(width as u32, height as u32, bgra)
            .map_err(str::to_owned)?
            .with_display_metadata(selected)
            .map_err(str::to_owned)
    }
}

pub fn capture_primary() -> Result<CapturedFrame, String> {
    capture_display(None)
}

/// Exclude the already-hidden settings HWND from capture and synchronize with
/// DWM before the capture worker reads desktop pixels. Slint owns visibility.
pub fn prepare_capture() -> Result<(), String> {
    use windows::core::w;
    use windows::Win32::{
        Foundation::HWND,
        Graphics::Dwm::DwmFlush,
        UI::WindowsAndMessaging::{
            FindWindowW, GetWindowThreadProcessId, SetWindowDisplayAffinity, WDA_EXCLUDEFROMCAPTURE,
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
    crate::platform::window_diagnostics::trace_native_operation(
        "SetWindowDisplayAffinity(EXCLUDEFROMCAPTURE).before",
        window,
    );
    unsafe { SetWindowDisplayAffinity(window, WDA_EXCLUDEFROMCAPTURE) }
        .map_err(|error| format!("Could not exclude the settings window from capture: {error}"))?;
    crate::platform::window_diagnostics::trace_native_operation(
        "SetWindowDisplayAffinity(EXCLUDEFROMCAPTURE).after",
        window,
    );
    let flush =
        unsafe { DwmFlush() }.map_err(|error| format!("DwmFlush failed before capture: {error}"));
    crate::platform::window_diagnostics::trace_native_operation("DwmFlush.after", window);
    flush
}

/// Reset capture affinity. Slint owns all settings-window visibility changes.
pub fn reset_capture_affinity() -> Result<(), String> {
    use windows::core::w;
    use windows::Win32::{
        Foundation::HWND,
        UI::WindowsAndMessaging::{
            FindWindowW, GetWindowThreadProcessId, SetWindowDisplayAffinity, WDA_NONE,
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
    crate::platform::window_diagnostics::trace_native_operation(
        "SetWindowDisplayAffinity(NONE).before",
        window,
    );
    let result = unsafe { SetWindowDisplayAffinity(window, WDA_NONE) }
        .map_err(|error| format!("Could not reset settings-window capture affinity: {error}"));
    crate::platform::window_diagnostics::trace_native_operation(
        "SetWindowDisplayAffinity(NONE).after",
        window,
    );
    result
}

pub fn activate_settings() -> Result<(), String> {
    use windows::core::w;
    use windows::Win32::{
        Foundation::HWND,
        UI::WindowsAndMessaging::{FindWindowW, GetWindowThreadProcessId, SetForegroundWindow},
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
    crate::platform::window_diagnostics::trace_native_operation(
        "SetForegroundWindow(Settings).before",
        window,
    );
    let _ = unsafe { SetForegroundWindow(window) };
    crate::platform::window_diagnostics::trace_native_operation(
        "SetForegroundWindow(Settings).after",
        window,
    );
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
    crate::platform::window_diagnostics::trace_native_operation("ActivateOverlay.before", window);
    unsafe {
        let _ = SetForegroundWindow(window);
        let _ = SetActiveWindow(window);
        let _ = SetFocus(Some(window));
    }
    crate::platform::window_diagnostics::trace_native_operation("ActivateOverlay.after", window);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::enumerate_displays;
    use crate::core::display::select_display;
    use std::collections::HashSet;

    #[test]
    fn win32_monitor_enumeration_metadata_and_selection_are_consistent() {
        let displays = enumerate_displays().expect("EnumDisplayMonitors should report the desktop");
        assert!(!displays.is_empty());

        let primary_displays = displays
            .iter()
            .filter(|display| display.is_primary)
            .collect::<Vec<_>>();
        assert_eq!(
            primary_displays.len(),
            1,
            "Windows should report exactly one primary monitor"
        );
        assert!(
            displays[0].is_primary,
            "the primary display is sorted first"
        );

        let mut device_names = HashSet::new();
        for display in &displays {
            assert!(!display.id.trim().is_empty());
            assert!(
                device_names.insert(display.id.as_str()),
                "monitor IDs must be unique"
            );
            assert!(display.bounds.right > display.bounds.left);
            assert!(display.bounds.bottom > display.bounds.top);
            assert!(display.bounds.width() > 0);
            assert!(display.bounds.height() > 0);
            assert!(display.effective_dpi > 0);
        }

        let primary = primary_displays[0];
        assert_eq!(select_display(&displays, None).unwrap(), primary);
        for display in &displays {
            assert_eq!(
                select_display(&displays, Some(&display.id)).unwrap(),
                display,
                "an enumerated display ID should resolve to that display"
            );
        }
    }
}
