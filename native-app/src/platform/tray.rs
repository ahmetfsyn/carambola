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
                GetKeyboardLayout, RegisterHotKey, UnregisterHotKey, VkKeyScanExW,
                HOT_KEY_MODIFIERS, MOD_ALT, MOD_NOREPEAT,
            },
            Shell::{
                Shell_NotifyIconW, NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP, NIIF_INFO, NIM_ADD,
                NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW, NOTIFYICONDATAW_0,
            },
            WindowsAndMessaging::{
                AppendMenuW, CreateIconFromResourceEx, CreatePopupMenu, CreateWindowExW,
                DefWindowProcW, DestroyIcon, DestroyMenu, DestroyWindow, DispatchMessageW,
                GetCursorPos, GetForegroundWindow, GetMessageW, GetWindowLongPtrW,
                GetWindowThreadProcessId, KillTimer, LoadIconW, PostMessageW, PostQuitMessage,
                RegisterClassW, RegisterWindowMessageW, SendMessageW, SetForegroundWindow,
                SetTimer, SetWindowLongPtrW, TrackPopupMenu, TranslateMessage, UnregisterClassW,
                CREATESTRUCTW, GWLP_USERDATA, HICON, HMENU, IDI_APPLICATION, LR_DEFAULTCOLOR,
                MF_SEPARATOR, MF_STRING, MSG, TPM_LEFTALIGN, TPM_NONOTIFY, TPM_RETURNCMD,
                TPM_RIGHTBUTTON, WM_APP, WM_CLOSE, WM_CONTEXTMENU, WM_CREATE, WM_DESTROY,
                WM_HOTKEY, WM_INPUTLANGCHANGE, WM_LBUTTONDBLCLK, WM_NCCREATE, WM_NULL,
                WM_RBUTTONUP, WM_TIMER, WNDCLASSW, WS_EX_TOOLWINDOW, WS_POPUP,
            },
        },
    },
};

const HOTKEY_ID: i32 = 1;
const TRAY_ICON_ID: u32 = 1;
const COMMAND_NEW_SCREENSHOT: u32 = 1;
const COMMAND_SETTINGS: u32 = 2;
const COMMAND_EXIT: u32 = 3;
const TRAY_CALLBACK_MESSAGE: u32 = WM_APP + 1;
const TRAY_STOP_MESSAGE: u32 = WM_APP + 2;
const TRAY_UPDATE_HOTKEY_MESSAGE: u32 = WM_APP + 3;
const SECONDARY_HOTKEY_ID: i32 = 2;
const ALT_BACKTICK_HOTKEY_ID: i32 = 3;
const ALT_BACKTICK_TIMER_ID: usize = 1;
const CLASS_NAME: PCWSTR = w!("CarambolaNativeTrayHost");

/// Ask Explorer to show a native notification balloon for the existing tray
/// icon. It does not create or show another application window.
pub fn show_notification(owner_hwnd: isize, message: &str) -> Result<(), String> {
    let hwnd = HWND(owner_hwnd as *mut c_void);
    if hwnd.is_invalid() {
        return Err("The native tray notification owner is unavailable".into());
    }

    let mut info = [0u16; 256];
    let mut title = [0u16; 64];
    let info_units = message
        .encode_utf16()
        .take(info.len() - 1)
        .collect::<Vec<_>>();
    info[..info_units.len()].copy_from_slice(&info_units);
    let title_units = "Carambola"
        .encode_utf16()
        .take(title.len() - 1)
        .collect::<Vec<_>>();
    title[..title_units.len()].copy_from_slice(&title_units);

    let data = NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: TRAY_ICON_ID,
        uFlags: NIF_INFO,
        szInfo: info,
        szInfoTitle: title,
        dwInfoFlags: NIIF_INFO,
        ..Default::default()
    };
    if unsafe { Shell_NotifyIconW(NIM_MODIFY, &data) }.as_bool() {
        Ok(())
    } else {
        let error = unsafe { GetLastError().0 };
        Err(format!(
            "Windows could not show the notification (error {error})"
        ))
    }
}

type CommandCallback = Arc<dyn Fn(TrayCommand) + Send + Sync + 'static>;

fn native_window_handle_value(window: HWND) -> isize {
    window.0 as isize
}

pub struct TrayService {
    window: HWND,
    worker: Option<JoinHandle<()>>,
    hotkey_state: RegistrationState,
}

impl TrayService {
    pub fn start<F>(callback: F, hotkey_config: HotkeyConfig) -> Result<Self, String>
    where
        F: Fn(TrayCommand) + Send + Sync + 'static,
    {
        crate::platform::window_diagnostics::trace_event("TRAY_START begin");
        let callback: CommandCallback = Arc::new(callback);
        let (sender, receiver) = mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("carambola-native-tray".into())
            .spawn(move || run_tray_thread(sender, callback, hotkey_config))
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

    pub fn native_window_handle(&self) -> isize {
        native_window_handle_value(self.window)
    }

    pub fn replace_hotkey(
        &mut self,
        current: HotkeyConfig,
        requested: HotkeyConfig,
    ) -> Result<(), String> {
        requested.validate().map_err(str::to_owned)?;
        if requested == current
            && requested.key_character == 0
            && self.hotkey_state == RegistrationState::Registered
        {
            return Ok(());
        }
        let result = unsafe {
            SendMessageW(
                self.window,
                TRAY_UPDATE_HOTKEY_MESSAGE,
                Some(WPARAM(
                    requested.modifiers as usize | ((requested.key_character as usize) << 8),
                )),
                Some(LPARAM(requested.virtual_key as isize)),
            )
        };
        if result.0 == 1 {
            self.hotkey_state = RegistrationState::Registered;
            Ok(())
        } else {
            let error = result.0.unsigned_abs() as u32;
            // Candidate registration failed before the existing hotkey was
            // removed. Keep reporting the registration state of that hotkey.
            match RegistrationState::from_result(false, error) {
                RegistrationState::Conflict => Err(format!(
                    "{} is already registered by another application",
                    requested.display()
                )),
                RegistrationState::Failed(code) => Err(format!(
                    "Windows could not register {} (error {code})",
                    requested.display()
                )),
                RegistrationState::Registered => unreachable!("failed update cannot be registered"),
            }
        }
    }
}

impl crate::core::settings::ShortcutRegistrar for TrayService {
    fn replace(&mut self, current: HotkeyConfig, requested: HotkeyConfig) -> Result<(), String> {
        self.replace_hotkey(current, requested)
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
    hotkey: HotkeyConfig,
    hotkey_id: i32,
    hotkey_registered: bool,
    alt_backtick_key: Option<u32>,
    alt_backtick_timer_started: bool,
    alt_backtick_last_failed_key: Option<u32>,
    alt_backtick_no_mapping_logged: bool,
    configured_layout_error: Option<String>,
}

// VkKeyScanEx includes the modifiers required to *type* the character. For
// Alt+Backtick we register Alt with the physical key's virtual key instead;
// this also works when a layout types backtick through Shift or AltGr.
fn backtick_virtual_key(mapping: i16) -> Option<u32> {
    if mapping == -1 {
        return None;
    }
    let key = u32::from(mapping as u16 & 0x00ff);
    (key != 0).then_some(key)
}

fn foreground_backtick_virtual_key() -> Option<u32> {
    let foreground = unsafe { GetForegroundWindow() };
    let foreground_thread = unsafe { GetWindowThreadProcessId(foreground, None) };
    let layout = unsafe { GetKeyboardLayout(foreground_thread) };
    backtick_virtual_key(unsafe { VkKeyScanExW('`' as u16, layout) })
}

impl TrayState {
    fn refresh_alt_backtick_hotkey(&mut self, window: HWND) {
        let desired = foreground_backtick_virtual_key();
        if desired == self.alt_backtick_key {
            if desired.is_none() && !self.alt_backtick_no_mapping_logged {
                log::warn!("Alt+Backtick is unavailable in the foreground keyboard layout");
                self.alt_backtick_no_mapping_logged = true;
            }
            return;
        }
        if let Some(previous_key) = self.alt_backtick_key.take() {
            if let Err(error) = unsafe { UnregisterHotKey(Some(window), ALT_BACKTICK_HOTKEY_ID) } {
                log::warn!("Could not release the previous Alt+Backtick key: {error}");
                self.alt_backtick_key = Some(previous_key);
                return;
            }
        }
        let Some(key) = desired else {
            if !self.alt_backtick_no_mapping_logged {
                log::warn!("Alt+Backtick is unavailable in the foreground keyboard layout");
                self.alt_backtick_no_mapping_logged = true;
            }
            return;
        };
        self.alt_backtick_no_mapping_logged = false;
        match unsafe {
            RegisterHotKey(
                Some(window),
                ALT_BACKTICK_HOTKEY_ID,
                MOD_ALT | MOD_NOREPEAT,
                key,
            )
        } {
            Ok(()) => {
                self.alt_backtick_key = Some(key);
                self.alt_backtick_last_failed_key = None;
                log::info!("Alt+Backtick registered for virtual key 0x{key:02X}");
                crate::platform::window_diagnostics::trace_event(&format!(
                    "TRAY_ALT_BACKTICK registered=true vk=0x{key:02X}"
                ));
            }
            Err(error) => {
                let code = unsafe { GetLastError().0 };
                if self.alt_backtick_last_failed_key != Some(key) {
                    log::warn!("Alt+Backtick registration failed for key 0x{key:02X} (Windows error {code}): {error}");
                    crate::platform::window_diagnostics::trace_event(&format!(
                        "TRAY_ALT_BACKTICK registered=false vk=0x{key:02X} win32_error={code}"
                    ));
                }
                self.alt_backtick_last_failed_key = Some(key);
            }
        }
    }

    fn start_alt_backtick_timer(&mut self, window: HWND) {
        self.alt_backtick_timer_started =
            unsafe { SetTimer(Some(window), ALT_BACKTICK_TIMER_ID, 500, None) } != 0;
        if !self.alt_backtick_timer_started {
            log::warn!("Could not watch keyboard layout changes for Alt+Backtick");
        }
    }

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

    fn replace_hotkey(&mut self, window: HWND, requested: HotkeyConfig) -> Result<(), u32> {
        let requested = crate::platform::windows::resolve_hotkey_for_foreground_layout(requested)
            .map_err(|error| {
            log::warn!(
                "Could not map {} in the active keyboard layout: {error}",
                requested.display()
            );
            1113u32 // ERROR_NO_UNICODE_TRANSLATION
        })?;
        if self.hotkey_registered && requested == self.hotkey {
            self.configured_layout_error = None;
            return Ok(());
        }
        let candidate_id = if self.hotkey_id == HOTKEY_ID {
            SECONDARY_HOTKEY_ID
        } else {
            HOTKEY_ID
        };
        if unsafe {
            RegisterHotKey(
                Some(window),
                candidate_id,
                HOT_KEY_MODIFIERS(requested.modifiers) | MOD_NOREPEAT,
                requested.virtual_key,
            )
        }
        .is_err()
        {
            return Err(unsafe { GetLastError().0 });
        }

        if self.hotkey_registered
            && unsafe { UnregisterHotKey(Some(window), self.hotkey_id) }.is_err()
        {
            let error = unsafe { GetLastError().0 };
            let _ = unsafe { UnregisterHotKey(Some(window), candidate_id) };
            return Err(error);
        }

        self.hotkey = requested;
        self.hotkey_id = candidate_id;
        self.hotkey_registered = true;
        self.configured_layout_error = None;
        self.resources.mark_hotkey_registered();
        Ok(())
    }

    fn refresh_configured_hotkey(&mut self, window: HWND) {
        if self.hotkey.key_character == 0 {
            return;
        }
        match crate::platform::windows::resolve_hotkey_for_foreground_layout(self.hotkey) {
            Ok(resolved) if resolved.virtual_key == self.hotkey.virtual_key => {
                self.configured_layout_error = None;
            }
            Ok(resolved) => {
                if let Err(error) = self.replace_hotkey(window, resolved) {
                    let message = format!("Windows could not register {} after a keyboard layout change (error {error})", self.hotkey.display());
                    if self.configured_layout_error.as_deref() != Some(message.as_str()) {
                        log::warn!("{message}");
                    }
                    self.configured_layout_error = Some(message);
                }
            }
            Err(error) => {
                if self.configured_layout_error.as_deref() != Some(error.as_str()) {
                    log::warn!(
                        "The active keyboard layout cannot register {}: {error}",
                        self.hotkey.display()
                    );
                }
                self.configured_layout_error = Some(error);
            }
        }
    }

    fn cleanup(&mut self, window: HWND) {
        if self.alt_backtick_timer_started {
            let _ = unsafe { KillTimer(Some(window), ALT_BACKTICK_TIMER_ID) };
            self.alt_backtick_timer_started = false;
        }
        if self.alt_backtick_key.take().is_some() {
            if let Err(error) = unsafe { UnregisterHotKey(Some(window), ALT_BACKTICK_HOTKEY_ID) } {
                log::warn!("Could not unregister Alt+Backtick: {error}");
            }
        }
        let cleanup = self.resources.release();
        if cleanup.unregister_hotkey {
            if let Err(error) = unsafe { UnregisterHotKey(Some(window), self.hotkey_id) } {
                log::warn!("Could not unregister the capture shortcut: {error}");
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
    hotkey_config: HotkeyConfig,
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
        hotkey: hotkey_config,
        hotkey_id: HOTKEY_ID,
        hotkey_registered: false,
        alt_backtick_key: None,
        alt_backtick_timer_started: false,
        alt_backtick_last_failed_key: None,
        alt_backtick_no_mapping_logged: false,
        configured_layout_error: None,
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

    let hotkey_state = match hotkey_config
        .validate()
        .map_err(str::to_owned)
        .and_then(|()| {
            crate::platform::windows::resolve_hotkey_for_foreground_layout(hotkey_config)
        }) {
        Ok(resolved) => match unsafe {
            RegisterHotKey(
                Some(window),
                HOTKEY_ID,
                HOT_KEY_MODIFIERS(resolved.modifiers) | MOD_NOREPEAT,
                resolved.virtual_key,
            )
        } {
            Ok(()) => {
                state.hotkey = resolved;
                state.resources.mark_hotkey_registered();
                state.hotkey_registered = true;
                RegistrationState::Registered
            }
            Err(error) => {
                let code = unsafe { GetLastError().0 };
                log::warn!(
                    "Global hotkey registration failed for {} (error {code}): {error}",
                    hotkey_config.display()
                );
                RegistrationState::from_result(false, code)
            }
        },
        Err(error) => {
            log::warn!(
                "Global capture shortcut {} is unavailable: {error}",
                hotkey_config.display()
            );
            state.configured_layout_error = Some(error);
            RegistrationState::Failed(1113)
        }
    };
    crate::platform::window_diagnostics::trace_event(&format!(
        "TRAY_HOTKEY registration_result={hotkey_state:?}"
    ));
    state.refresh_alt_backtick_hotkey(window);
    state.refresh_configured_hotkey(window);

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
    state.start_alt_backtick_timer(window);

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
    create_carambola_window_icon(32, 32)
}

pub(crate) fn create_carambola_window_icon(width: u32, height: u32) -> Result<HICON, String> {
    let image = embedded_carambola_icon_image_for(width, height)?;
    unsafe {
        CreateIconFromResourceEx(
            image,
            true,
            0x0003_0000,
            width as i32,
            height as i32,
            LR_DEFAULTCOLOR,
        )
    }
    .map_err(|error| format!("CreateIconFromResourceEx failed: {error}"))
}

fn embedded_carambola_icon_image_for(
    target_width: u32,
    target_height: u32,
) -> Result<&'static [u8], String> {
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
        let score = width.abs_diff(target_width) + height.abs_diff(target_height);
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

    if message == TRAY_UPDATE_HOTKEY_MESSAGE {
        let modifiers_and_character = wparam.0 as u64;
        return match (*state_pointer).replace_hotkey(
            window,
            HotkeyConfig {
                modifiers: (modifiers_and_character & 0xff) as u32,
                virtual_key: lparam.0 as u32,
                key_character: (modifiers_and_character >> 8) as u32,
            },
        ) {
            Ok(()) => LRESULT(1),
            Err(error) => LRESULT(-(error as isize)),
        };
    }

    if (message == WM_TIMER && wparam.0 == ALT_BACKTICK_TIMER_ID) || message == WM_INPUTLANGCHANGE {
        (*state_pointer).refresh_alt_backtick_hotkey(window);
        (*state_pointer).refresh_configured_hotkey(window);
        return LRESULT(0);
    }

    if message == WM_HOTKEY && wparam.0 as i32 == ALT_BACKTICK_HOTKEY_ID {
        let state = &mut *state_pointer;
        if state.alt_backtick_key.is_some()
            && state.alt_backtick_key == foreground_backtick_virtual_key()
        {
            crate::platform::window_diagnostics::trace_event(
                "WM_HOTKEY shortcut=Alt+Backtick command=NewScreenshot",
            );
            state.dispatch(TrayCommand::NewScreenshot);
        } else {
            crate::platform::window_diagnostics::trace_event(
                "WM_HOTKEY shortcut=Alt+Backtick ignored_stale_layout=true",
            );
            state.refresh_alt_backtick_hotkey(window);
        }
        return LRESULT(0);
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
        WM_HOTKEY if wparam.0 as i32 == state.hotkey_id => {
            crate::platform::window_diagnostics::trace_event(&format!(
                "WM_HOTKEY id={} shortcut={} command=NewScreenshot",
                wparam.0,
                state.hotkey.display()
            ));
            state.dispatch(TrayCommand::NewScreenshot);
            LRESULT(0)
        }
        TRAY_CALLBACK_MESSAGE => {
            match lparam.0 as u32 {
                WM_CONTEXTMENU | WM_RBUTTONUP => show_tray_menu(window, state),
                WM_LBUTTONDBLCLK => state.dispatch(TrayCommand::NewScreenshot),
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
    let screenshot_label = format!("New Screenshot\t{}", state.hotkey.display())
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let items = [
        (COMMAND_NEW_SCREENSHOT, PCWSTR(screenshot_label.as_ptr())),
        (COMMAND_SETTINGS, w!("Settings")),
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
        COMMAND_SETTINGS => state.dispatch(TrayCommand::Settings),
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
    use super::{
        backtick_virtual_key, embedded_carambola_icon_image_for, native_window_handle_value,
    };
    use windows::Win32::Foundation::HWND;

    #[test]
    fn clipboard_owner_handle_preserves_the_tray_host_hwnd() {
        let hwnd = HWND(0x1234usize as *mut std::ffi::c_void);
        assert_eq!(native_window_handle_value(hwnd), 0x1234);
    }

    #[test]
    fn backtick_hotkey_uses_the_active_layouts_key_without_typing_modifiers() {
        // US Backquote and Turkish-Q AltGr+Comma are different virtual keys.
        assert_eq!(backtick_virtual_key(0x00c0), Some(0xc0));
        assert_eq!(backtick_virtual_key(0x06bc), Some(0xbc));
        assert_eq!(backtick_virtual_key(-1), None);
    }

    #[test]
    fn embedded_carambola_ico_directory_resolves_to_image_data() {
        let image =
            embedded_carambola_icon_image_for(32, 32).expect("valid embedded Carambola ICO");
        assert!(!image.is_empty());
    }
}
