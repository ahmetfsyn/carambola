//! Windows DPI process initialization. This must run before Slint creates Winit windows.

use windows::Win32::UI::HiDpi::{
    SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};

use crate::core::frame::CapturedFrame;

/// Keep one tray and global hotkey owner per interactive Windows session.
pub struct SingleInstanceGuard(windows::Win32::Foundation::HANDLE);

impl SingleInstanceGuard {
    pub fn acquire() -> Result<Option<Self>, String> {
        Self::acquire_named(windows::core::w!(
            "Local\\Carambola.Native.9B0C7DC2-E7ED-4CF6-A9A4-3A5E7E871A45"
        ))
    }

    fn acquire_named(name: windows::core::PCWSTR) -> Result<Option<Self>, String> {
        use windows::Win32::{
            Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS},
            System::Threading::CreateMutexW,
        };

        let handle = unsafe { CreateMutexW(None, false, name) }
            .map_err(|error| format!("Could not establish the Carambola instance lock: {error}"))?;
        if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            unsafe { CloseHandle(handle) }.map_err(|error| {
                format!("Could not release the duplicate instance lock: {error}")
            })?;
            return Ok(None);
        }
        Ok(Some(Self(handle)))
    }
}

impl Drop for SingleInstanceGuard {
    fn drop(&mut self) {
        let _ = unsafe { windows::Win32::Foundation::CloseHandle(self.0) };
    }
}

/// Resolve a recorded logical character to the virtual key used by the
/// foreground thread's active keyboard layout. RegisterHotKey only accepts a
/// virtual key, while Slint records the character the user actually pressed.
pub fn resolve_hotkey_for_foreground_layout(
    shortcut: crate::core::hotkey::HotkeyConfig,
) -> Result<crate::core::hotkey::HotkeyConfig, String> {
    use windows::Win32::{
        Foundation::HWND,
        UI::{
            Input::KeyboardAndMouse::{GetKeyboardLayout, VkKeyScanExW},
            WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId},
        },
    };

    shortcut.validate().map_err(str::to_owned)?;
    if shortcut.key_character == 0 {
        return Ok(shortcut);
    }
    let character = char::from_u32(shortcut.key_character)
        .ok_or_else(|| "The selected shortcut character is invalid".to_string())?;
    let mut utf16 = [0u16; 2];
    let encoded = character.encode_utf16(&mut utf16);
    if encoded.len() != 1 {
        return Err("This character has no Windows virtual-key mapping".into());
    }

    let foreground = unsafe { GetForegroundWindow() };
    let foreground_thread = unsafe { GetWindowThreadProcessId(HWND(foreground.0), None) };
    let layout = unsafe { GetKeyboardLayout(foreground_thread) };
    let mapping = unsafe { VkKeyScanExW(encoded[0], layout) };
    if mapping == -1 {
        return Err(format!(
            "The active keyboard layout cannot map {character:?} to a key"
        ));
    }
    let mapping = mapping as u16;
    let virtual_key = u32::from(mapping & 0x00ff);
    let required = ((mapping >> 8) & 0x00ff) as u8;
    let required_modifiers = (if required & 0x01 != 0 {
        crate::core::hotkey::MOD_SHIFT
    } else {
        0
    }) | (if required & 0x02 != 0 {
        crate::core::hotkey::MOD_CONTROL
    } else {
        0
    }) | (if required & 0x04 != 0 {
        crate::core::hotkey::MOD_ALT
    } else {
        0
    });
    if required_modifiers & !shortcut.modifiers != 0 {
        let mut required_names = Vec::new();
        if required_modifiers & crate::core::hotkey::MOD_CONTROL != 0 {
            required_names.push("Ctrl");
        }
        if required_modifiers & crate::core::hotkey::MOD_SHIFT != 0 {
            required_names.push("Shift");
        }
        if required_modifiers & crate::core::hotkey::MOD_ALT != 0 {
            required_names.push("Alt");
        }
        return Err(format!(
            "The active keyboard layout requires {} to type {character:?}; include those modifiers in the shortcut",
            required_names.join("+")
        ));
    }
    let mut resolved = shortcut;
    resolved.virtual_key = virtual_key;
    Ok(resolved)
}

/// Resolve the Settings HWND directly from Slint so a similarly titled window
/// in another process cannot receive window operations.
fn settings_hwnd(window: &slint::Window) -> Result<windows::Win32::Foundation::HWND, String> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows::Win32::{Foundation::HWND, UI::WindowsAndMessaging::GetWindowThreadProcessId};

    let handle = window.window_handle();
    let raw = handle
        .window_handle()
        .map_err(|error| format!("Settings HWND is not ready: {error}"))?;
    let RawWindowHandle::Win32(raw) = raw.as_raw() else {
        return Err("Settings has no Windows HWND".into());
    };
    let hwnd = HWND(raw.hwnd.get() as *mut _);
    let mut pid = 0;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    if pid != std::process::id() {
        return Err("Settings HWND does not belong to Carambola".into());
    }
    Ok(hwnd)
}

fn settings_icon_handles() -> Result<(isize, isize), String> {
    use std::sync::OnceLock;
    use windows::Win32::UI::WindowsAndMessaging::DestroyIcon;
    static ICONS: OnceLock<Result<(isize, isize), String>> = OnceLock::new();
    ICONS
        .get_or_init(|| {
            let small = crate::platform::tray::create_carambola_window_icon(16, 16)?;
            let big = match crate::platform::tray::create_carambola_window_icon(32, 32) {
                Ok(icon) => icon,
                Err(error) => {
                    let _ = unsafe { DestroyIcon(small) };
                    return Err(error);
                }
            };
            Ok((small.0 as isize, big.0 as isize))
        })
        .clone()
}

unsafe extern "system" fn settings_background_proc(
    hwnd: windows::Win32::Foundation::HWND,
    message: u32,
    wparam: windows::Win32::Foundation::WPARAM,
    lparam: windows::Win32::Foundation::LPARAM,
    subclass_id: usize,
    first_paint_done: usize,
) -> windows::Win32::Foundation::LRESULT {
    use windows::Win32::{
        Foundation::{COLORREF, LRESULT, RECT},
        Graphics::Gdi::{
            CreateSolidBrush, DeleteObject, FillRect, GetDC, ReleaseDC, HBRUSH, HDC, HGDIOBJ,
        },
        UI::{
            Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
            WindowsAndMessaging::{GetClientRect, WM_ERASEBKGND, WM_NCDESTROY, WM_PAINT},
        },
    };
    let paint_background = |dc: HDC| {
        let mut rect = RECT::default();
        if GetClientRect(hwnd, &mut rect).is_ok() {
            let brush: HBRUSH = CreateSolidBrush(COLORREF(0x0020_120B));
            let painted = FillRect(dc, &rect, brush) != 0;
            let _ = DeleteObject(HGDIOBJ(brush.0));
            return painted;
        }
        false
    };
    if message == WM_ERASEBKGND && paint_background(HDC(wparam.0 as *mut _)) {
        return LRESULT(1);
    }
    if message == WM_PAINT && first_paint_done == 0 {
        let dc = GetDC(Some(hwnd));
        let _ = paint_background(dc);
        let _ = ReleaseDC(Some(hwnd), dc);
        let _ = SetWindowSubclass(hwnd, Some(settings_background_proc), subclass_id, 1);
    }
    if message == WM_NCDESTROY {
        let _ = RemoveWindowSubclass(hwnd, Some(settings_background_proc), subclass_id);
    }
    DefSubclassProc(hwnd, message, wparam, lparam)
}

/// Give the hidden Settings HWND the same dark initial client/title background
/// as its Slint component before Windows makes it visible.
pub fn prepare_settings_window(window: &slint::Window) -> Result<(), String> {
    use windows::Win32::{
        Foundation::COLORREF,
        Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_CAPTION_COLOR},
        UI::{
            Shell::SetWindowSubclass,
            WindowsAndMessaging::{SendMessageW, ICON_BIG, ICON_SMALL, ICON_SMALL2, WM_SETICON},
        },
    };
    let hwnd = settings_hwnd(window)?;
    let (small_icon, big_icon) = settings_icon_handles()?;
    unsafe {
        SendMessageW(
            hwnd,
            WM_SETICON,
            Some(windows::Win32::Foundation::WPARAM(ICON_SMALL as usize)),
            Some(windows::Win32::Foundation::LPARAM(small_icon)),
        );
        SendMessageW(
            hwnd,
            WM_SETICON,
            Some(windows::Win32::Foundation::WPARAM(ICON_SMALL2 as usize)),
            Some(windows::Win32::Foundation::LPARAM(small_icon)),
        );
        SendMessageW(
            hwnd,
            WM_SETICON,
            Some(windows::Win32::Foundation::WPARAM(ICON_BIG as usize)),
            Some(windows::Win32::Foundation::LPARAM(big_icon)),
        );
    }
    if !unsafe { SetWindowSubclass(hwnd, Some(settings_background_proc), 1, 0) }.as_bool() {
        return Err(format!(
            "Could not set the initial Settings background (Windows error {})",
            unsafe { windows::Win32::Foundation::GetLastError().0 }
        ));
    }
    let color = COLORREF(0x0020_120B);
    if let Err(error) = unsafe {
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_CAPTION_COLOR,
            &color as *const _ as *const _,
            std::mem::size_of_val(&color) as u32,
        )
    } {
        log::warn!("Could not set the Settings title background: {error}");
    }
    Ok(())
}

pub fn activate_settings_window(window: &slint::Window) -> Result<(), String> {
    use windows::Win32::UI::WindowsAndMessaging::{
        BringWindowToTop, IsIconic, SetForegroundWindow, ShowWindow, SW_RESTORE,
    };
    let hwnd = settings_hwnd(window)?;
    if unsafe { IsIconic(hwnd) }.as_bool() {
        let _ = unsafe { ShowWindow(hwnd, SW_RESTORE) };
    }
    unsafe { BringWindowToTop(hwnd) }
        .map_err(|error| format!("Could not raise Settings: {error}"))?;
    if !unsafe { SetForegroundWindow(hwnd) }.as_bool() {
        return Err("Windows did not allow Settings to take foreground focus".into());
    }
    Ok(())
}

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
pub fn save_png_dialog(
    default_directory: Option<&std::path::Path>,
) -> Result<Option<std::path::PathBuf>, String> {
    use std::{os::windows::ffi::OsStringExt, path::PathBuf};
    use windows::{
        core::{w, PCWSTR, PWSTR},
        Win32::UI::Controls::Dialogs::{
            CommDlgExtendedError, GetSaveFileNameW, OFN_EXPLORER, OFN_HIDEREADONLY,
            OFN_NOCHANGEDIR, OFN_OVERWRITEPROMPT, OFN_PATHMUSTEXIST, OPENFILENAMEW,
        },
    };

    let owner = find_app_window(w!("Carambola"))?;
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
    let initial_directory = default_directory
        .filter(|path| path.is_dir())
        .and_then(|path| path.to_str())
        .map(|path| {
            path.encode_utf16()
                .chain(std::iter::once(0))
                .collect::<Vec<_>>()
        });
    let default_extension = w!("png");
    let mut dialog = OPENFILENAMEW {
        lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
        hwndOwner: owner,
        lpstrFilter: PCWSTR(filter.as_ptr()),
        nFilterIndex: 1,
        lpstrFile: PWSTR(file_name.as_mut_ptr()),
        nMaxFile: file_name.len() as u32,
        lpstrTitle: PCWSTR(title.as_ptr()),
        lpstrInitialDir: initial_directory
            .as_ref()
            .map_or(PCWSTR::null(), |path| PCWSTR(path.as_ptr())),
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

/// Display a native folder browser for the default screenshot directory.
pub fn pick_save_directory(
    initial_directory: Option<&std::path::Path>,
) -> Result<Option<std::path::PathBuf>, String> {
    use std::{os::windows::ffi::OsStringExt, path::PathBuf};
    use windows::{
        core::{w, PCWSTR, PWSTR},
        Win32::{
            Foundation::{HWND, LPARAM, WPARAM},
            System::Com::CoTaskMemFree,
            UI::Shell::{
                SHBrowseForFolderW, SHGetPathFromIDListW, BFFM_INITIALIZED, BFFM_SETSELECTIONW,
                BIF_NEWDIALOGSTYLE, BIF_RETURNONLYFSDIRS, BROWSEINFOW,
            },
            UI::WindowsAndMessaging::SendMessageW,
        },
    };

    unsafe extern "system" fn initialize_folder_picker(
        dialog: HWND,
        message: u32,
        _parameter: LPARAM,
        initial_path: LPARAM,
    ) -> i32 {
        if message == BFFM_INITIALIZED {
            let path = initial_path.0 as *const u16;
            if !path.is_null() {
                let _ = SendMessageW(
                    dialog,
                    BFFM_SETSELECTIONW,
                    Some(WPARAM(1)),
                    Some(LPARAM(path as isize)),
                );
            }
        }
        0
    }

    let owner = find_app_window(w!("Carambola Settings"))?;
    let title = "Choose the default screenshot save folder"
        .encode_utf16()
        .chain([0])
        .collect::<Vec<_>>();
    let initial = initial_directory
        .filter(|path| path.is_dir())
        .and_then(|path| path.to_str())
        .map(|path| path.encode_utf16().chain([0]).collect::<Vec<_>>());
    let mut display_name = [0u16; 32_768];
    let mut info = BROWSEINFOW {
        hwndOwner: owner,
        pszDisplayName: PWSTR(display_name.as_mut_ptr()),
        lpszTitle: PCWSTR(title.as_ptr()),
        ulFlags: BIF_RETURNONLYFSDIRS | BIF_NEWDIALOGSTYLE,
        lpfn: initial.as_ref().map(|_| {
            initialize_folder_picker as unsafe extern "system" fn(HWND, u32, LPARAM, LPARAM) -> i32
        }),
        lParam: initial
            .as_ref()
            .map_or(LPARAM(0), |path| LPARAM(path.as_ptr() as isize)),
        ..Default::default()
    };
    let selected = unsafe { SHBrowseForFolderW(&mut info) };
    if selected.is_null() {
        return Ok(None);
    }
    let mut path_buffer = [0u16; 260];
    let copied = unsafe { SHGetPathFromIDListW(selected, &mut path_buffer) }.as_bool();
    unsafe { CoTaskMemFree(Some(selected.cast())) };
    if !copied {
        return Err("Windows could not read the selected save folder path".into());
    }
    let length = path_buffer
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(path_buffer.len());
    Ok(Some(PathBuf::from(std::ffi::OsString::from_wide(
        &path_buffer[..length],
    ))))
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
pub fn copy_image_to_clipboard(frame: &CapturedFrame, owner_hwnd: isize) -> Result<(), String> {
    use std::ptr::copy_nonoverlapping;
    use windows::Win32::{
        Foundation::{HANDLE, HWND},
        System::{DataExchange::EmptyClipboard, Memory::GlobalAlloc, Memory::GlobalLock},
        UI::WindowsAndMessaging::{GetWindowThreadProcessId, IsWindow},
    };

    const CF_DIBV5: u32 = 17;
    if owner_hwnd == 0 {
        return Err("Clipboard owner window is unavailable".into());
    }
    let owner = HWND(owner_hwnd as *mut std::ffi::c_void);
    if !unsafe { IsWindow(Some(owner)) }.as_bool() {
        return Err("Clipboard owner HWND is no longer valid".into());
    }
    let mut owner_process_id = 0u32;
    unsafe { GetWindowThreadProcessId(owner, Some(&mut owner_process_id)) };
    if owner_process_id != std::process::id() {
        return Err("Clipboard owner HWND belongs to another process".into());
    }

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

    let clipboard = ClipboardLock::open(owner)?;
    unsafe { EmptyClipboard() }
        .map_err(|error| format!("Could not clear the Windows clipboard: {error}"))?;
    let handle = HANDLE(allocation.handle().0);
    unsafe { windows::Win32::System::DataExchange::SetClipboardData(CF_DIBV5, Some(handle)) }
        .map_err(|error| format!("Could not copy the image to the Windows clipboard: {error}"))?;
    allocation.transfer_to_clipboard();
    clipboard.close()
}

pub fn copy_text_to_clipboard(text: &str, owner_hwnd: isize) -> Result<(), String> {
    use std::ptr::copy_nonoverlapping;
    use windows::Win32::{
        Foundation::{HANDLE, HWND},
        System::{
            DataExchange::{EmptyClipboard, SetClipboardData},
            Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE},
        },
        UI::WindowsAndMessaging::{GetWindowThreadProcessId, IsWindow},
    };

    if owner_hwnd == 0 {
        return Err("Clipboard owner window is unavailable".into());
    }
    let owner = HWND(owner_hwnd as *mut std::ffi::c_void);
    if !unsafe { IsWindow(Some(owner)) }.as_bool() {
        return Err("Clipboard owner HWND is no longer valid".into());
    }
    let mut owner_process_id = 0u32;
    unsafe { GetWindowThreadProcessId(owner, Some(&mut owner_process_id)) };
    if owner_process_id != std::process::id() {
        return Err("Clipboard owner HWND belongs to another process".into());
    }

    let wide = text
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let size = wide.len() * std::mem::size_of::<u16>();
    let allocation = unsafe { GlobalAlloc(GMEM_MOVEABLE, size) }
        .map_err(|error| format!("Could not allocate clipboard text memory: {error}"))?;
    let mut allocation = ClipboardMemory::new(allocation);
    let destination = unsafe { GlobalLock(allocation.handle()) };
    if destination.is_null() {
        return Err("Could not lock clipboard text memory".into());
    }
    unsafe { copy_nonoverlapping(wide.as_ptr(), destination.cast::<u16>(), wide.len()) };
    let _ = unsafe { GlobalUnlock(allocation.handle()) };

    let clipboard = ClipboardLock::open(owner)?;
    unsafe { EmptyClipboard() }
        .map_err(|error| format!("Could not clear the Windows clipboard: {error}"))?;
    unsafe { SetClipboardData(13, Some(HANDLE(allocation.handle().0))) }.map_err(|error| {
        format!("Could not copy recognized text to the Windows clipboard: {error}")
    })?;
    allocation.transfer_to_clipboard();
    clipboard.close()
}

pub fn recognize_text(frame: &CapturedFrame) -> Result<String, String> {
    use windows::{
        Graphics::Imaging::{BitmapPixelFormat, SoftwareBitmap},
        Media::Ocr::OcrEngine,
        Storage::Streams::DataWriter,
    };

    let engine = OcrEngine::TryCreateFromUserProfileLanguages()
        .map_err(|error| format!("Could not initialize Windows OCR: {error}. Install an OCR language pack in Windows Settings."))?;
    let writer =
        DataWriter::new().map_err(|error| format!("Could not create OCR pixel buffer: {error}"))?;
    writer
        .WriteBytes(frame.pixels())
        .map_err(|error| format!("Could not prepare pixels for OCR: {error}"))?;
    let buffer = writer
        .DetachBuffer()
        .map_err(|error| format!("Could not prepare pixels for OCR: {error}"))?;
    let bitmap = SoftwareBitmap::CreateCopyFromBuffer(
        &buffer,
        BitmapPixelFormat::Rgba8,
        frame.width() as i32,
        frame.height() as i32,
    )
    .map_err(|error| format!("Could not create an OCR bitmap: {error}"))?;
    let result = engine
        .RecognizeAsync(&bitmap)
        .map_err(|error| format!("Windows OCR could not start: {error}"))?
        .join()
        .map_err(|error| format!("Windows OCR failed: {error}"))?;
    let lines = result
        .Lines()
        .map_err(|error| format!("Could not read Windows OCR output: {error}"))?;
    let mut text = String::new();
    for line in lines {
        let line = line
            .Text()
            .map_err(|error| format!("Could not read a Windows OCR line: {error}"))?;
        let line = line.to_string_lossy();
        if !line.trim().is_empty() {
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str(&line);
        }
    }
    Ok(text)
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

/// Snapshot the display under the physical desktop cursor. This is called on
/// the UI thread when capture is requested so a later worker-thread delay or
/// cursor movement cannot silently retarget the screenshot.
pub fn display_at_cursor() -> Result<crate::core::display::DisplayMetadata, String> {
    use crate::core::display::{select_display_at_point, PhysicalPoint};
    use windows::Win32::{Foundation::POINT, UI::WindowsAndMessaging::GetCursorPos};

    let mut cursor = POINT::default();
    unsafe { GetCursorPos(&mut cursor) }
        .map_err(|error| format!("Could not read the desktop cursor position: {error}"))?;
    let displays = enumerate_displays()?;
    let selected = select_display_at_point(
        &displays,
        PhysicalPoint {
            x: cursor.x,
            y: cursor.y,
        },
    )?
    .clone();
    crate::platform::trace_event(&format!(
        "capture.cursor_display point=({}, {}) id={:?} bounds=({}, {}, {}, {}) dpi={}",
        cursor.x,
        cursor.y,
        selected.id,
        selected.bounds.left,
        selected.bounds.top,
        selected.bounds.right,
        selected.bounds.bottom,
        selected.effective_dpi
    ));
    Ok(selected)
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

/// Synchronize with DWM before the capture worker reads desktop pixels.
pub fn prepare_capture() -> Result<(), String> {
    use windows::Win32::Graphics::Dwm::DwmFlush;

    unsafe { DwmFlush() }.map_err(|error| format!("DwmFlush failed before capture: {error}"))
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
        FindWindowW(None, w!("Carambola"))
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
    use super::{enumerate_displays, SingleInstanceGuard};
    use crate::core::display::select_display;
    use std::collections::HashSet;

    #[test]
    fn instance_lock_rejects_a_second_owner_and_releases_on_drop() {
        let name = format!("Local\\Carambola.Native.Test.{}", std::process::id())
            .encode_utf16()
            .chain([0])
            .collect::<Vec<_>>();
        let name = windows::core::PCWSTR(name.as_ptr());
        let first = SingleInstanceGuard::acquire_named(name).unwrap().unwrap();
        assert!(SingleInstanceGuard::acquire_named(name).unwrap().is_none());
        drop(first);
        assert!(SingleInstanceGuard::acquire_named(name).unwrap().is_some());
    }

    #[test]
    fn settings_icon_uses_the_embedded_carambola_ico() {
        let icon = image::load_from_memory_with_format(
            include_bytes!("../../assets/carambola.ico"),
            image::ImageFormat::Ico,
        )
        .expect("the Settings window icon must be decodable");
        assert!(icon.width() > 0 && icon.height() > 0);
    }

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
