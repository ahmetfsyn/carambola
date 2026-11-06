//! Native Windows notification-area icon and global hotkey owner.
//!
//! A dedicated Win32 thread owns the hidden callback window, shell icon,
//! context menu, and hotkey registration. Commands are posted to Slint's event
//! loop; no Windows message pump runs on the UI thread.

use crate::core::{
    hotkey::{HotkeyConfig, RegistrationState},
    lifecycle::{TrayCommand, TrayResourceOwnership},
};
use std::{
    ffi::c_void,
    sync::{mpsc, Arc},
    thread::{self, JoinHandle},
};
use windows::{
    core::{w, PCWSTR},
    Win32::{
        Foundation::{GetLastError, HINSTANCE, HWND, LPARAM, POINT, WPARAM},
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            Input::KeyboardAndMouse::{
                RegisterHotKey, UnregisterHotKey, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT,
            },
            Shell::{
                Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE,
                NOTIFYICONDATAW, NOTIFYICONDATAW_0,
            },
            WindowsAndMessaging::{
                AppendMenuW, CreateIconFromResourceEx, CreatePopupMenu, CreateWindowExW,
                DefWindowProcW, DestroyIcon, DestroyMenu, DestroyWindow, DispatchMessageW,
                GetCursorPos, GetMessageW, GetWindowLongPtrW, LoadIconW, PostMessageW,
                PostQuitMessage, RegisterClassW, RegisterWindowMessageW, SetForegroundWindow,
                SetWindowLongPtrW, TrackPopupMenu, TranslateMessage, UnregisterClassW,
                CREATESTRUCTW, GWLP_USERDATA, HICON, HMENU, IDI_APPLICATION, LR_DEFAULTCOLOR,
                MF_SEPARATOR, MF_STRING, MSG, TPM_LEFTALIGN, TPM_NONOTIFY, TPM_RETURNCMD,
                TPM_RIGHTBUTTON, WM_APP, WM_CLOSE, WM_CONTEXTMENU, WM_CREATE, WM_DESTROY,
                WM_HOTKEY, WM_LBUTTONDBLCLK, WM_NCCREATE, WM_NULL, WM_RBUTTONUP, WNDCLASSW,
                WS_EX_TOOLWINDOW, WS_POPUP,
            },
        },
    },
};

const HOTKEY_ID: i32 = 1;
const TRAY_ICON_ID: u32 = 1;
const COMMAND_NEW_SCREENSHOT: u32 = 1;
const COMMAND_OPEN_CARAMBOLA: u32 = 2;
const COMMAND_EXIT: u32 = 3;
const TRAY_CALLBACK_MESSAGE: u32 = WM_APP + 1;
const TRAY_STOP_MESSAGE: u32 = WM_APP + 2;
const CLASS_NAME: PCWSTR = w!("CarambolaNativeTrayHost");

type CommandCallback = Arc<dyn Fn(TrayCommand) + Send + Sync + 'static>;

pub struct TrayService {
    window: HWND,
    worker: Option<JoinHandle<()>>,
    hotkey_state: RegistrationState,
}

impl TrayService {
    pub fn start<F>(callback: F) -> Result<Self, String>
    where
        F: Fn(TrayCommand) + Send + Sync + 'static,
    {
        crate::platform::window_diagnostics::trace_event("TRAY_START begin");
        let callback: CommandCallback = Arc::new(callback);
        let (sender, receiver) = mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("carambola-native-tray".into())
            .spawn(move || run_tray_thread(sender, callback))
            .map_err(|error| {
                crate::platform::window_diagnostics::trace_event(&format!(
                    "TRAY_START thread_spawn_failed error={error}"
                ));
                format!("Could not start the native tray thread: {error}")
            })?;

        crate::platform::window_diagnostics::trace_event("TRAY_START waiting_for_worker");
        match receiver.recv() {
            Ok(Ok((window_address, hotkey_state))) => {
                crate::platform::window_diagnostics::trace_event(&format!(
                    "TRAY_START worker_ready hwnd=0x{:X} hotkey={hotkey_state:?}",
                    window_address as usize
                ));
                Ok(Self {
                    window: HWND(window_address as *mut c_void),
                    worker: Some(worker),
                    hotkey_state,
                })
            }
            Ok(Err(error)) => {
                crate::platform::window_diagnostics::trace_event(&format!(
                    "TRAY_START worker_failed error={error}"
                ));
                let _ = worker.join();
                Err(error)
            }
            Err(error) => {
                crate::platform::window_diagnostics::trace_event(&format!(
                    "TRAY_START channel_closed error={error}"
                ));
                let _ = worker.join();
                Err(format!(
                    "Native tray initialization ended unexpectedly: {error}"
                ))
            }
        }
    }

    pub fn hotkey_state(&self) -> RegistrationState {
        self.hotkey_state
    }
}

impl Drop for TrayService {
    fn drop(&mut self) {
        unsafe {
            if let Err(error) =
                PostMessageW(Some(self.window), TRAY_STOP_MESSAGE, WPARAM(0), LPARAM(0))
            {
                log::warn!("Could not request tray-thread shutdown: {error}");
            }
        }
        if let Some(worker) = self.worker.take() {
            if worker.join().is_err() {
                log::error!("Native tray thread panicked during shutdown");
            }
        }
    }
}

struct TrayState {
    callback: CommandCallback,
    taskbar_created_message: u32,
    notify_data: NOTIFYICONDATAW,
    resources: TrayResourceOwnership,
    owned_icon: Option<HICON>,
}

impl TrayState {
    fn dispatch(&self, command: TrayCommand) {
        crate::platform::window_diagnostics::trace_event(&format!(
            "TRAY_DISPATCH command={command:?}"
        ));
        let callback = Arc::clone(&self.callback);
        if let Err(error) = slint::invoke_from_event_loop(move || callback(command)) {
            log::error!("Could not dispatch {command:?} from the tray thread: {error}");
        }
    }

    fn add_icon(&mut self) -> Result<(), String> {
        crate::platform::window_diagnostics::trace_event(&format!(
            "TRAY_ICON add.call hwnd=0x{:X} id={} callback_message=0x{:X} flags=0x{:X}",
            self.notify_data.hWnd.0 as usize,
            self.notify_data.uID,
            self.notify_data.uCallbackMessage,
            self.notify_data.uFlags.0
        ));
        let added = unsafe { Shell_NotifyIconW(NIM_ADD, &self.notify_data) }.as_bool();
        let error_code = if added {
            0
        } else {
            unsafe { GetLastError().0 }
        };
        crate::platform::window_diagnostics::trace_event(&format!(
            "TRAY_ICON add.result success={added} win32_error={error_code} hwnd=0x{:X} id={} callback_message=0x{:X}",
            self.notify_data.hWnd.0 as usize,
            self.notify_data.uID,
            self.notify_data.uCallbackMessage
        ));
        if added {
            self.resources.mark_icon_added();
            Ok(())
        } else {
            Err(format!(
                "Shell_NotifyIconW could not add the Carambola icon (Windows error {error_code})"
            ))
        }
    }

    fn cleanup(&mut self, window: HWND) {
        let cleanup = self.resources.release();
        if cleanup.unregister_hotkey {
            if let Err(error) = unsafe { UnregisterHotKey(Some(window), HOTKEY_ID) } {
                log::warn!("Could not unregister Ctrl+Shift+S: {error}");
            }
        }
        if cleanup.remove_icon {
            if !unsafe { Shell_NotifyIconW(NIM_DELETE, &self.notify_data) }.as_bool() {
                log::warn!("Windows did not confirm removal of the Carambola tray icon");
            }
        }
    }
}

impl Drop for TrayState {
    fn drop(&mut self) {
        if let Some(icon) = self.owned_icon.take() {
            if let Err(error) = unsafe { DestroyIcon(icon) } {
                log::warn!("Could not destroy the Carambola tray icon handle: {error}");
            }
        }
    }
}

fn run_tray_thread(
    startup: mpsc::SyncSender<Result<(isize, RegistrationState), String>>,
    callback: CommandCallback,
) {
    crate::platform::window_diagnostics::trace_event("TRAY_THREAD entered");
    let instance = match unsafe { GetModuleHandleW(None) } {
        Ok(module) => HINSTANCE(module.0),
        Err(error) => {
            crate::platform::window_diagnostics::trace_event(&format!(
                "TRAY_THREAD early_return stage=GetModuleHandleW error={error}"
            ));
            let _ = startup.send(Err(format!(
                "Could not get the application module: {error}"
            )));
            return;
        }
    };
    let class = WNDCLASSW {
        lpfnWndProc: Some(tray_window_proc),
        hInstance: instance,
        lpszClassName: CLASS_NAME,
        ..Default::default()
    };
    crate::platform::window_diagnostics::trace_event(
        "TRAY_CLASS register.call name=CarambolaNativeTrayHost",
    );
    let registered = unsafe { RegisterClassW(&class) };
    let register_error = if registered == 0 {
        unsafe { GetLastError().0 }
    } else {
        0
    };
    crate::platform::window_diagnostics::trace_event(&format!(
        "TRAY_CLASS register.result atom={registered} success={} win32_error={register_error}",
        registered != 0 || register_error == 1410
    ));
    if registered == 0 {
        // A prior startup attempt may have registered this process-local class.
        if register_error != 1410 {
            crate::platform::window_diagnostics::trace_event(&format!(
                "TRAY_THREAD early_return stage=RegisterClassW win32_error={register_error}"
            ));
            let _ = startup.send(Err(format!(
                "Could not register the native tray window class (error {register_error})"
            )));
            return;
        }
    }

    let taskbar_created_message = unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) };
    if taskbar_created_message == 0 {
        let code = unsafe { GetLastError().0 };
        crate::platform::window_diagnostics::trace_event(&format!(
            "TRAY_THREAD early_return stage=RegisterWindowMessageW win32_error={code}"
        ));
        let _ = unsafe { UnregisterClassW(CLASS_NAME, Some(instance)) };
        let _ = startup.send(Err(format!(
            "Could not register the Explorer restart notification (error {code})"
        )));
        return;
    }
    crate::platform::window_diagnostics::trace_event(&format!(
        "TRAY_THREAD taskbar_created_message=0x{taskbar_created_message:X}"
    ));

    let (icon, owned_icon) = match load_carambola_icon() {
        Ok(icon) => {
            crate::platform::window_diagnostics::trace_event("TRAY_ICON source=embedded");
            (icon, Some(icon))
        }
        Err(error) => {
            crate::platform::window_diagnostics::trace_event(&format!(
                "TRAY_ICON embedded_load_failed error={error}"
            ));
            log::warn!("Could not load the embedded Carambola icon: {error}");
            match unsafe { LoadIconW(None, IDI_APPLICATION) } {
                Ok(icon) => {
                    crate::platform::window_diagnostics::trace_event(
                        "TRAY_ICON source=system_fallback",
                    );
                    (icon, None)
                }
                Err(error) => {
                    crate::platform::window_diagnostics::trace_event(&format!(
                        "TRAY_THREAD early_return stage=LoadIconW error={error}"
                    ));
                    let _ = unsafe { UnregisterClassW(CLASS_NAME, Some(instance)) };
                    let _ = startup.send(Err(format!("Could not load a tray icon: {error}")));
                    return;
                }
            }
        }
    };
    let mut state = Box::new(TrayState {
        callback,
        taskbar_created_message,
        notify_data: notify_icon_data(HWND::default(), icon),
        resources: TrayResourceOwnership::default(),
        owned_icon,
    });
    let state_pointer = (&mut *state as *mut TrayState).cast::<c_void>();
    crate::platform::window_diagnostics::trace_native_event_brief(
        "CreateWindowExW(TrayHost).before",
    );
    crate::platform::window_diagnostics::trace_event(&format!(
        "TRAY_WINDOW create.call class=CarambolaNativeTrayHost title=Carambola tray callback style=0x{:08X} ex_style=0x{:08X} parent=0x0 owner=0x0",
        WS_POPUP.0, WS_EX_TOOLWINDOW.0
    ));
    let window = match unsafe {
        CreateWindowExW(
            WS_EX_TOOLWINDOW,
            CLASS_NAME,
            w!("Carambola tray callback"),
            WS_POPUP,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(instance),
            Some(state_pointer.cast_const()),
        )
    } {
        Ok(window) => {
            crate::platform::window_diagnostics::trace_event(&format!(
                "TRAY_WINDOW create.result success=true hwnd=0x{:X}",
                window.0 as usize
            ));
            window
        }
        Err(error) => {
            let code = unsafe { GetLastError().0 };
            crate::platform::window_diagnostics::trace_event(&format!(
                "TRAY_WINDOW create.result success=false hwnd=0x0 win32_error={code} error={error}"
            ));
            let _ = unsafe { UnregisterClassW(CLASS_NAME, Some(instance)) };
            let _ = startup.send(Err(format!(
                "Could not create the tray callback window: {error}"
            )));
            return;
        }
    };
    crate::platform::window_diagnostics::trace_native_window_brief(
        "CreateWindowExW(TrayHost).after",
        window,
    );
    state.notify_data.hWnd = window;

    let hotkey_config = HotkeyConfig::DEFAULT;
    if let Err(error) = hotkey_config.validate() {
        crate::platform::window_diagnostics::trace_event(&format!(
            "TRAY_THREAD early_return stage=hotkey_validation error={error}"
        ));
        state.cleanup(window);
        destroy_tray_window(window);
        let _ = unsafe { UnregisterClassW(CLASS_NAME, Some(instance)) };
        let _ = startup.send(Err(format!("The default shortcut is invalid: {error}")));
        return;
    }
    let hotkey_state = match unsafe {
        RegisterHotKey(
            Some(window),
            HOTKEY_ID,
            MOD_CONTROL | MOD_SHIFT | MOD_NOREPEAT,
            hotkey_config.virtual_key,
        )
    } {
        Ok(()) => {
            state.resources.mark_hotkey_registered();
            RegistrationState::Registered
        }
        Err(error) => {
            let code = unsafe { GetLastError().0 };
            log::warn!("Global hotkey registration failed (error {code}): {error}");
            RegistrationState::from_result(false, code)
        }
    };
    crate::platform::window_diagnostics::trace_event(&format!(
        "TRAY_HOTKEY registration_result={hotkey_state:?}"
    ));

    if let Err(error) = state.add_icon() {
        crate::platform::window_diagnostics::trace_event(&format!(
            "TRAY_THREAD early_return stage=Shell_NotifyIconW error={error}"
        ));
        state.cleanup(window);
        destroy_tray_window(window);
        let _ = unsafe { UnregisterClassW(CLASS_NAME, Some(instance)) };
        let _ = startup.send(Err(error));
        return;
    }

    if startup.send(Ok((window.0 as isize, hotkey_state))).is_err() {
        crate::platform::window_diagnostics::trace_event(
            "TRAY_THREAD early_return stage=startup_channel_send",
        );
        state.cleanup(window);
        destroy_tray_window(window);
        let _ = unsafe { UnregisterClassW(CLASS_NAME, Some(instance)) };
        return;
    }
    crate::platform::window_diagnostics::trace_event(
        "TRAY_THREAD startup_complete message_loop_enter",
    );

    let mut message = MSG::default();
    loop {
        let result = unsafe { GetMessageW(&mut message, None, 0, 0) }.0;
        if result <= 0 {
            if result < 0 {
                crate::platform::window_diagnostics::trace_event(&format!(
                    "TRAY_THREAD GetMessageW.failed win32_error={}",
                    unsafe { GetLastError().0 }
                ));
                log::error!(
                    "Tray thread GetMessageW failed with Windows error {}",
                    unsafe { GetLastError().0 }
                );
            }
            break;
        }
        unsafe {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }

    state.cleanup(window);
    crate::platform::window_diagnostics::trace_event("TRAY_THREAD message_loop_exit");
    destroy_tray_window(window);
    let _ = unsafe { UnregisterClassW(CLASS_NAME, Some(instance)) };
}

fn destroy_tray_window(window: HWND) {
    crate::platform::window_diagnostics::trace_native_window_brief(
        "DestroyWindow(TrayHost).before",
        window,
    );
    let _ = unsafe { DestroyWindow(window) };
    crate::platform::window_diagnostics::trace_native_event_brief("DestroyWindow(TrayHost).after");
}

fn load_carambola_icon() -> Result<HICON, String> {
    let image = embedded_carambola_icon_image()?;
    unsafe { CreateIconFromResourceEx(image, true, 0x0003_0000, 32, 32, LR_DEFAULTCOLOR) }
        .map_err(|error| format!("CreateIconFromResourceEx failed: {error}"))
}

fn embedded_carambola_icon_image() -> Result<&'static [u8], String> {
    const ICO: &[u8] = include_bytes!("../../assets/carambola.ico");
    if ICO.len() < 6 || ICO[0..2] != [0, 0] || ICO[2..4] != [1, 0] {
        return Err("the embedded ICO header is invalid".into());
    }
    let count = u16::from_le_bytes([ICO[4], ICO[5]]) as usize;
    if count == 0 {
        return Err("the embedded ICO contains no images".into());
    }
    let entries_end = 6usize
        .checked_add(count.checked_mul(16).ok_or("ICO entry count overflow")?)
        .ok_or("ICO directory length overflow")?;
    if entries_end > ICO.len() {
        return Err("the embedded ICO directory is truncated".into());
    }

    let mut best: Option<(u32, &'static [u8])> = None;
    for entry in ICO[6..entries_end].chunks_exact(16) {
        let width = if entry[0] == 0 { 256 } else { entry[0] as u32 };
        let height = if entry[1] == 0 { 256 } else { entry[1] as u32 };
        let size =
            u32::from_le_bytes(entry[8..12].try_into().expect("four-byte ICO size")) as usize;
        let offset =
            u32::from_le_bytes(entry[12..16].try_into().expect("four-byte ICO offset")) as usize;
        let end = offset
            .checked_add(size)
            .ok_or("ICO image length overflow")?;
        if size == 0 || end > ICO.len() {
            continue;
        }
        let score = width.abs_diff(32) + height.abs_diff(32);
        if best
            .as_ref()
            .map_or(true, |(best_score, _)| score < *best_score)
        {
            best = Some((score, &ICO[offset..end]));
        }
    }
    let Some((_, image)) = best else {
        return Err("the embedded ICO has no usable image data".into());
    };
    Ok(image)
}

fn notify_icon_data(window: HWND, icon: HICON) -> NOTIFYICONDATAW {
    let mut tip = [0u16; 128];
    let label = "Carambola".encode_utf16().collect::<Vec<_>>();
    tip[..label.len()].copy_from_slice(&label);
    NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: window,
        uID: TRAY_ICON_ID,
        uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP,
        uCallbackMessage: TRAY_CALLBACK_MESSAGE,
        hIcon: icon,
        szTip: tip,
        Anonymous: NOTIFYICONDATAW_0::default(),
        ..Default::default()
    }
}

unsafe extern "system" fn tray_window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> windows::Win32::Foundation::LRESULT {
    use windows::Win32::Foundation::LRESULT;

    if message == WM_NCCREATE {
        crate::platform::window_diagnostics::trace_event(&format!(
            "TRAY_WINDOW wndproc.WM_NCCREATE hwnd=0x{:X}",
            window.0 as usize
        ));
        let create = (lparam.0 as *const CREATESTRUCTW).as_ref();
        if let Some(create) = create {
            let _ = SetWindowLongPtrW(window, GWLP_USERDATA, create.lpCreateParams as isize);
        }
        return LRESULT(1);
    }

    let state_pointer = GetWindowLongPtrW(window, GWLP_USERDATA) as *mut TrayState;
    if message == WM_CREATE {
        crate::platform::window_diagnostics::trace_event(&format!(
            "TRAY_WINDOW wndproc.WM_CREATE hwnd=0x{:X}",
            window.0 as usize
        ));
    }
    if state_pointer.is_null() {
        return DefWindowProcW(window, message, wparam, lparam);
    }

    if message == (*state_pointer).taskbar_created_message {
        if let Err(error) = (*state_pointer).add_icon() {
            log::warn!("Could not restore the tray icon after Explorer restarted: {error}");
        }
        return LRESULT(0);
    }

    let state = &*state_pointer;
    match message {
        TRAY_STOP_MESSAGE | WM_CLOSE => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        WM_DESTROY => {
            crate::platform::window_diagnostics::trace_event(&format!(
                "TRAY_WINDOW wndproc.WM_DESTROY hwnd=0x{:X}",
                window.0 as usize
            ));
            PostQuitMessage(0);
            LRESULT(0)
        }
        WM_HOTKEY if wparam.0 as i32 == HOTKEY_ID => {
            crate::platform::window_diagnostics::trace_event(&format!(
                "WM_HOTKEY id={} command=NewScreenshot",
                wparam.0
            ));
            state.dispatch(TrayCommand::NewScreenshot);
            LRESULT(0)
        }
        TRAY_CALLBACK_MESSAGE => {
            match lparam.0 as u32 {
                WM_CONTEXTMENU | WM_RBUTTONUP => show_tray_menu(window, state),
                WM_LBUTTONDBLCLK => state.dispatch(TrayCommand::OpenCarambola),
                _ => {}
            }
            LRESULT(0)
        }
        _ => DefWindowProcW(window, message, wparam, lparam),
    }
}

fn show_tray_menu(window: HWND, state: &TrayState) {
    crate::platform::window_diagnostics::trace_native_operation("tray_menu.open", window);
    let menu = match unsafe { CreatePopupMenu() } {
        Ok(menu) => PopupMenu(menu),
        Err(error) => {
            log::warn!("Could not create the tray context menu: {error}");
            return;
        }
    };
    let items = [
        (COMMAND_NEW_SCREENSHOT, w!("New Screenshot\tCtrl+Shift+S")),
        (COMMAND_OPEN_CARAMBOLA, w!("Open Carambola")),
    ];
    for (id, label) in items {
        if let Err(error) = unsafe { AppendMenuW(menu.0, MF_STRING, id as usize, label) } {
            log::warn!("Could not populate the tray context menu: {error}");
            return;
        }
    }
    if let Err(error) = unsafe { AppendMenuW(menu.0, MF_SEPARATOR, 0, PCWSTR::null()) } {
        log::warn!("Could not add the tray menu separator: {error}");
        return;
    }
    if let Err(error) = unsafe { AppendMenuW(menu.0, MF_STRING, COMMAND_EXIT as usize, w!("Exit")) }
    {
        log::warn!("Could not add Exit to the tray menu: {error}");
        return;
    }

    let mut position = POINT::default();
    if let Err(error) = unsafe { GetCursorPos(&mut position) } {
        log::warn!("Could not get the cursor position for the tray menu: {error}");
        return;
    }
    crate::platform::window_diagnostics::trace_native_operation(
        "SetForegroundWindow(TrayHost).before",
        window,
    );
    let _ = unsafe { SetForegroundWindow(window) };
    crate::platform::window_diagnostics::trace_native_operation(
        "SetForegroundWindow(TrayHost).after",
        window,
    );
    let selected = unsafe {
        TrackPopupMenu(
            menu.0,
            TPM_LEFTALIGN | TPM_RIGHTBUTTON | TPM_RETURNCMD | TPM_NONOTIFY,
            position.x,
            position.y,
            None,
            window,
            None,
        )
    }
    .0 as u32;
    let _ = unsafe { PostMessageW(Some(window), WM_NULL, WPARAM(0), LPARAM(0)) };
    crate::platform::window_diagnostics::trace_native_operation("tray_menu.close", window);
    match selected {
        COMMAND_NEW_SCREENSHOT => state.dispatch(TrayCommand::NewScreenshot),
        COMMAND_OPEN_CARAMBOLA => state.dispatch(TrayCommand::OpenCarambola),
        COMMAND_EXIT => state.dispatch(TrayCommand::Exit),
        _ => {}
    }
}

struct PopupMenu(HMENU);

impl Drop for PopupMenu {
    fn drop(&mut self) {
        if let Err(error) = unsafe { DestroyMenu(self.0) } {
            log::warn!("Could not destroy the tray context menu: {error}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::embedded_carambola_icon_image;

    #[test]
    fn embedded_carambola_ico_directory_resolves_to_image_data() {
        let image = embedded_carambola_icon_image().expect("valid embedded Carambola ICO");
        assert!(!image.is_empty());
    }
}
