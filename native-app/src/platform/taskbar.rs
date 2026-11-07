//! Keep the Slint Overlay out of Explorer's taskbar once its native window exists.

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use slint::winit_030::WinitWindowAccessor;
use windows::core::Interface;
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
    WaitForHwnd,
    Remove,
}

fn removal_action(hwnd_available: bool) -> RemovalAction {
    if hwnd_available {
        RemovalAction::Remove
    } else {
        RemovalAction::WaitForHwnd
    }
}

fn delete_tab(
    hwnd: HWND,
    pid: u32,
    phase: &str,
    after_fullscreen: bool,
    after_show: bool,
) -> windows::core::Result<()> {
    let initialized = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
    if let Err(error) = initialized.ok() {
        super::trace_event(&format!(
            "taskbar.delete_tab.not_invoked phase={phase} hwnd=0x{:X} pid={pid} stage=CoInitializeEx hresult=0x{:08X} error={error}",
            hwnd.0 as usize,
            error.code().0 as u32,
        ));
        return Err(error);
    }
    let result = (|| {
        let taskbar: ITaskbarList = match unsafe {
            CoCreateInstance(&TaskbarList, None, CLSCTX_INPROC_SERVER)
        } {
            Ok(taskbar) => taskbar,
            Err(error) => {
                super::trace_event(&format!(
                    "taskbar.delete_tab.not_invoked phase={phase} hwnd=0x{:X} pid={pid} stage=CoCreateInstance hresult=0x{:08X} error={error}",
                    hwnd.0 as usize,
                    error.code().0 as u32,
                ));
                return Err(error);
            }
        };
        if let Err(error) = unsafe { taskbar.HrInit() } {
            super::trace_event(&format!(
                "taskbar.delete_tab.not_invoked phase={phase} hwnd=0x{:X} pid={pid} stage=HrInit hresult=0x{:08X} error={error}",
                hwnd.0 as usize,
                error.code().0 as u32,
            ));
            return Err(error);
        }

        let hresult = unsafe { (taskbar.vtable().DeleteTab)(taskbar.as_raw(), hwnd) };
        super::trace_event(&format!(
            "taskbar.delete_tab.result phase={phase} hwnd=0x{:X} pid={pid} after_fullscreen={after_fullscreen} after_show={after_show} success={} hresult=0x{:08X}",
            hwnd.0 as usize,
            hresult.is_ok(),
            hresult.0 as u32,
        ));
        hresult.ok()
    })();
    unsafe { CoUninitialize() };
    result
}

fn finish_ensure(phase: &str, result: bool, reason: &str) -> bool {
    super::trace_event(&format!(
        "taskbar.ensure_overlay_removed.return phase={phase} result={result} reason={reason}"
    ));
    result
}

/// Returns false only if Slint has not yet created the Overlay's winit window.
/// Called once per capture, before fullscreen/show when the HWND already exists.
pub fn ensure_overlay_removed(
    overlay: &slint::Window,
    phase: &str,
    after_fullscreen: bool,
    after_show: bool,
) -> bool {
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
    let hwnd_available = matches!(handle.as_ref(), Some(Ok(_)));
    super::trace_event(&format!(
        "taskbar.ensure_overlay_removed.begin phase={phase} hwnd_available={hwnd_available} handle_slot_available={} overlay_visible={} after_fullscreen={after_fullscreen} after_show={after_show}",
        handle.is_some(),
        overlay.is_visible(),
    ));
    match removal_action(handle.is_some()) {
        RemovalAction::WaitForHwnd => {
            return finish_ensure(phase, false, "overlay_hwnd_not_ready");
        }
        RemovalAction::Remove => {}
    }
    let hwnd = match handle.expect("Overlay HWND decision") {
        Ok(hwnd) => hwnd,
        Err(error) => {
            log::warn!("{error}");
            return finish_ensure(phase, true, "overlay_handle_error_skipped");
        }
    };
    let mut pid = 0;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    if !unsafe { IsWindow(Some(hwnd)) }.as_bool() || pid != std::process::id() {
        log::warn!(
            "Overlay HWND is invalid or belongs to another process; taskbar removal skipped"
        );
        super::trace_event(&format!(
            "taskbar.delete_tab.not_invoked phase={phase} hwnd=0x{:X} pid={pid} expected_pid={} reason=invalid_hwnd_or_foreign_process",
            hwnd.0 as usize,
            std::process::id(),
        ));
        return finish_ensure(phase, true, "invalid_hwnd_or_foreign_process_skipped");
    }

    super::trace_event(&format!(
        "taskbar.delete_tab.attempt phase={phase} hwnd=0x{:X} pid={pid} after_fullscreen={after_fullscreen} after_show={after_show} overlay_visible={}",
        hwnd.0 as usize,
        overlay.is_visible(),
    ));
    match delete_tab(hwnd, pid, phase, after_fullscreen, after_show) {
        Ok(()) => finish_ensure(phase, true, "delete_tab_succeeded"),
        Err(error) => {
            log::warn!("Could not remove Overlay HWND from taskbar: {error}");
            finish_ensure(
                phase,
                true,
                "delete_tab_failed_current_behavior_returns_true",
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{removal_action, RemovalAction};

    #[test]
    fn removal_waits_only_for_the_overlay_hwnd() {
        assert_eq!(removal_action(false), RemovalAction::WaitForHwnd);
        assert_eq!(removal_action(true), RemovalAction::Remove);
    }
}
