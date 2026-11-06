//! Keep the Slint Overlay out of Explorer's taskbar once its native window exists.

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use slint::winit_030::WinitWindowAccessor;
use windows::Win32::{
    Foundation::HWND,
    System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
        COINIT_APARTMENTTHREADED,
    },
    UI::{
        Shell::{ITaskbarList, TaskbarList},
        WindowsAndMessaging::{GetWindowThreadProcessId, IsWindow},
    },
};

#[derive(Debug, PartialEq, Eq)]
enum RemovalAction {
    Skip,
    WaitForHwnd,
    Remove,
}

fn removal_action(settings_visible: bool, hwnd_available: bool) -> RemovalAction {
    if settings_visible {
        RemovalAction::Skip
    } else if hwnd_available {
        RemovalAction::Remove
    } else {
        RemovalAction::WaitForHwnd
    }
}

fn delete_tab(hwnd: HWND) -> windows::core::Result<()> {
    let initialized = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
    initialized.ok()?;
    let result = (|| {
        let taskbar: ITaskbarList =
            unsafe { CoCreateInstance(&TaskbarList, None, CLSCTX_INPROC_SERVER) }?;
        unsafe {
            taskbar.HrInit()?;
            taskbar.DeleteTab(hwnd)
        }
    })();
    unsafe { CoUninitialize() };
    result
}

/// Returns false only if Slint has not yet created the Overlay's winit window.
/// Called once per capture, before fullscreen/show when the HWND already exists.
pub fn ensure_overlay_removed(overlay: &slint::Window, settings: &slint::Window) -> bool {
    let handle = overlay.with_winit_window(|window| {
        let hwnd = window
            .window_handle()
            .map_err(|error| format!("Overlay window handle unavailable: {error}"))?
            .as_raw();
        match hwnd {
            RawWindowHandle::Win32(raw) => Ok(HWND(raw.hwnd.get() as *mut _)),
            other => Err(format!("Unexpected Overlay window handle: {other:?}")),
        }
    });
    match removal_action(settings.is_visible(), handle.is_some()) {
        RemovalAction::Skip => return true,
        RemovalAction::WaitForHwnd => return false,
        RemovalAction::Remove => {}
    }
    let hwnd = match handle.expect("Overlay HWND decision") {
        Ok(hwnd) => hwnd,
        Err(error) => {
            log::warn!("{error}");
            return true;
        }
    };
    let mut pid = 0;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    if !unsafe { IsWindow(Some(hwnd)) }.as_bool() || pid != std::process::id() {
        log::warn!(
            "Overlay HWND is invalid or belongs to another process; taskbar removal skipped"
        );
        return true;
    }

    if let Err(error) = delete_tab(hwnd) {
        log::warn!("Could not remove Overlay HWND from taskbar: {error}");
    }
    true
}

#[cfg(test)]
mod tests {
    use super::{removal_action, RemovalAction};

    #[test]
    fn removal_waits_for_hidden_settings_and_the_overlay_hwnd() {
        assert_eq!(removal_action(true, false), RemovalAction::Skip);
        assert_eq!(removal_action(true, true), RemovalAction::Skip);
        assert_eq!(removal_action(false, false), RemovalAction::WaitForHwnd);
        assert_eq!(removal_action(false, true), RemovalAction::Remove);
    }
}
