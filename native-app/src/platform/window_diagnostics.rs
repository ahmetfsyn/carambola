//! Opt-in diagnostics for identifying transient native windows and visibility
//! changes during a physical Windows capture run.

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::{
    collections::HashMap,
    fs::{create_dir_all, File, OpenOptions},
    io::Write,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicU8, Ordering},
        Mutex, OnceLock,
    },
    time::{Instant, SystemTime, UNIX_EPOCH},
};
use windows::core::BOOL as WinBool;
use windows::Win32::{
    Foundation::{COLORREF, HWND, LPARAM},
    System::{SystemInformation::GetTickCount64, Threading::GetCurrentThreadId},
    UI::{
        Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK},
        Input::KeyboardAndMouse::IsWindowEnabled,
        WindowsAndMessaging::{
            EnumChildWindows, EnumWindows, GetClassNameW, GetClientRect, GetForegroundWindow,
            GetLayeredWindowAttributes, GetParent, GetWindow, GetWindowLongPtrW, GetWindowRect,
            GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId, IsIconic, IsWindow,
            IsWindowVisible, EVENT_OBJECT_CREATE, EVENT_OBJECT_DESTROY, EVENT_OBJECT_HIDE,
            EVENT_OBJECT_LOCATIONCHANGE, EVENT_OBJECT_SHOW, EVENT_SYSTEM_FOREGROUND,
            EVENT_SYSTEM_MINIMIZEEND, EVENT_SYSTEM_MINIMIZESTART, GWL_EXSTYLE, GWL_STYLE, GW_OWNER,
            LAYERED_WINDOW_ATTRIBUTES_FLAGS, OBJID_WINDOW, WINEVENT_OUTOFCONTEXT, WS_EX_LAYERED,
        },
    },
};

const SETTINGS_TITLE_START: &str = "Carambola";
const SETTINGS_TITLE_END: &str = "Native Screenshot Annotation";
const OVERLAY_TITLE: &str = "Carambola screenshot selection";
const TRAY_HOST_TITLE: &str = "Carambola tray callback";

static TRACE_FILE: OnceLock<Mutex<File>> = OnceLock::new();
static TRACE_STARTED: OnceLock<Instant> = OnceLock::new();
static TRACE_ENABLED: AtomicBool = AtomicBool::new(false);
static TRACE_PROCESS_ID: AtomicU32 = AtomicU32::new(0);
static STARTUP_ID: OnceLock<String> = OnceLock::new();
static NEXT_ACTION_ID: AtomicU64 = AtomicU64::new(1);
static ACTION_ID: AtomicU64 = AtomicU64::new(0);
// 0 = no action, 1 = in progress, 2 = completed (kept for delayed WinEvents).
static ACTION_PHASE: AtomicU8 = AtomicU8::new(0);
static WINDOW_IDENTITIES: OnceLock<Mutex<WindowIdentities>> = OnceLock::new();

#[derive(Default)]
struct WindowIdentities {
    next_instance: u64,
    by_hwnd: HashMap<isize, WindowIdentity>,
}

struct WindowIdentity {
    instance: u64,
    origin: &'static str,
    role: &'static str,
}

/// Owns the WinEvent hooks for one diagnostic application run.
pub struct WindowDiagnostics {
    hooks: Vec<HWINEVENTHOOK>,
}

impl WindowDiagnostics {
    /// Trace in the diagnostic build or when explicitly enabled at runtime.
    pub fn start() -> Result<Option<Self>, String> {
        let enabled_by_environment =
            std::env::var("CARAMBOLA_WINDOW_DIAGNOSTICS").is_ok_and(|value| value == "1");
        if !cfg!(feature = "window-diagnostics") && !enabled_by_environment {
            return Ok(None);
        }

        let (path, file) = open_trace_file()?;
        TRACE_FILE
            .set(Mutex::new(file))
            .map_err(|_| "Window diagnostics were initialized more than once".to_string())?;
        let _ = TRACE_STARTED.set(Instant::now());
        let process_id = std::process::id();
        let started_ns = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default();
        let _ = STARTUP_ID.set(format!("{process_id}-{started_ns}"));
        TRACE_PROCESS_ID.store(process_id, Ordering::Release);
        TRACE_ENABLED.store(true, Ordering::Release);

        trace_event("APP_ENTRY observed_after_dpi_initialization=true");
        trace_event(&format!(
            "TRACE_START version=5 pid={process_id} executable={:?} log_file={:?} hooks=WinEvent(object-create,destroy,show,hide,location-change;foreground;minimize-start,end) bounds=GetWindowRect,GetClientRect",
            std::env::current_exe().ok(),
            path
        ));

        let mut hooks = Vec::new();
        for (event_min, event_max, label) in [
            (EVENT_OBJECT_CREATE, EVENT_OBJECT_HIDE, "object-lifecycle"),
            (
                EVENT_SYSTEM_FOREGROUND,
                EVENT_SYSTEM_FOREGROUND,
                "foreground",
            ),
            (
                EVENT_SYSTEM_MINIMIZESTART,
                EVENT_SYSTEM_MINIMIZEEND,
                "minimize",
            ),
            (
                EVENT_OBJECT_LOCATIONCHANGE,
                EVENT_OBJECT_LOCATIONCHANGE,
                "object-location",
            ),
        ] {
            let hook = unsafe {
                SetWinEventHook(
                    event_min,
                    event_max,
                    None,
                    Some(win_event_proc),
                    process_id,
                    0,
                    WINEVENT_OUTOFCONTEXT,
                )
            };
            if hook.0.is_null() {
                let error = unsafe { windows::Win32::Foundation::GetLastError().0 };
                trace_event(&format!(
                    "HOOK_INSTALL_FAILED name={label:?} event_min=0x{event_min:04x} event_max=0x{event_max:04x} win32_error={error}"
                ));
            } else {
                trace_event(&format!(
                    "HOOK_INSTALLED name={label:?} event_min=0x{event_min:04x} event_max=0x{event_max:04x} hook={}",
                    hwnd_text(HWND(hook.0))
                ));
                hooks.push(hook);
            }
        }

        trace_process_windows("diagnostics-start");
        Ok(Some(Self { hooks }))
    }
}

impl Drop for WindowDiagnostics {
    fn drop(&mut self) {
        for hook in self.hooks.drain(..) {
            if !unsafe { UnhookWinEvent(hook) }.as_bool() {
                let error = unsafe { windows::Win32::Foundation::GetLastError().0 };
                trace_event(&format!(
                    "HOOK_REMOVE_FAILED hook={} win32_error={error}",
                    hwnd_text(HWND(hook.0))
                ));
            }
        }
        trace_event("TRACE_STOP");
        TRACE_ENABLED.store(false, Ordering::Release);
    }
}

/// Log a lifecycle event from application code and take a low-frequency window
/// snapshot. Call this around capture, Done, Escape, tray, and export actions.
pub fn trace_event(event: &str) {
    if !TRACE_ENABLED.load(Ordering::Acquire) {
        return;
    }
    write_line(format!("APP_EVENT event={event:?}"));
}

/// Start a Done/Escape correlation span before any callback work. The ID stays
/// attached to late WinEvents after completion until the next capture starts.
pub fn begin_action(action: &str) -> u64 {
    if !TRACE_ENABLED.load(Ordering::Acquire) {
        return 0;
    }
    let id = NEXT_ACTION_ID.fetch_add(1, Ordering::Relaxed);
    ACTION_ID.store(id, Ordering::Release);
    ACTION_PHASE.store(1, Ordering::Release);
    trace_event(&format!("ACTION_BEGIN name={action} id={id}"));
    id
}

pub fn finish_action(id: u64, outcome: &str) {
    if id == 0 || !TRACE_ENABLED.load(Ordering::Acquire) {
        return;
    }
    trace_event(&format!("ACTION_END id={id} outcome={outcome}"));
    if ACTION_ID.load(Ordering::Acquire) == id {
        ACTION_PHASE.store(2, Ordering::Release);
    }
}

pub fn clear_action(reason: &str) {
    if !TRACE_ENABLED.load(Ordering::Acquire) {
        return;
    }
    trace_event(&format!("ACTION_CLEAR reason={reason}"));
    ACTION_ID.store(0, Ordering::Release);
    ACTION_PHASE.store(0, Ordering::Release);
}

/// Log Slint's state for a component alongside every top-level HWND owned by
/// this process. `role` is descriptive only; HWND ownership comes from Win32.
pub fn trace_component_state(role: &str, operation: &str, visible: bool, minimized: bool) {
    if !TRACE_ENABLED.load(Ordering::Acquire) {
        return;
    }
    let windows = process_windows();
    let hwnd = role_hwnd(role, &windows)
        .map(hwnd_text)
        .unwrap_or_else(|| "none".into());
    write_line(format!(
        "SLINT_STATE role={role:?} operation={operation:?} visible={visible} minimized={minimized} hwnd={hwnd}"
    ));
    write_window_list(operation, &windows);
}

/// Record a native operation around a known HWND (activation, affinity, or a
/// dialog operation). Visibility events themselves are independently observed
/// through WinEvent hooks.
pub fn trace_native_operation(operation: &str, hwnd: HWND) {
    if !TRACE_ENABLED.load(Ordering::Acquire) {
        return;
    }
    let foreground = unsafe { GetForegroundWindow() };
    write_line(format!(
        "NATIVE_OP operation={operation:?} hwnd={} foreground={}",
        hwnd_text(hwnd),
        hwnd_text(foreground)
    ));
    write_line(format!(
        "NATIVE_WINDOW operation={operation:?} {}",
        describe_window(hwnd)
    ));
    trace_process_windows(operation);
}

/// Record operations where the HWND is not available (for example, Slint
/// `show`/`hide` or the Save As dialog before its native handle exists).
pub fn trace_native_event(operation: &str) {
    if !TRACE_ENABLED.load(Ordering::Acquire) {
        return;
    }
    let foreground = unsafe { GetForegroundWindow() };
    write_line(format!(
        "NATIVE_OP operation={operation:?} foreground={}",
        hwnd_text(foreground)
    ));
    trace_process_windows(operation);
}

/// Use on the tray worker while the UI thread is waiting for startup. A full
/// process snapshot can synchronously query the UI thread and deadlock here.
pub fn trace_native_event_brief(operation: &str) {
    trace_event(&format!("NATIVE_CALL operation={operation}"));
}

/// Describe only the caller-owned HWND, without enumerating other threads.
pub fn trace_native_window_brief(operation: &str, hwnd: HWND) {
    if !TRACE_ENABLED.load(Ordering::Acquire) {
        return;
    }
    write_line(format!(
        "NATIVE_WINDOW operation={operation:?} valid={} {}",
        unsafe { IsWindow(Some(hwnd)) }.as_bool(),
        describe_window(hwnd)
    ));
}

/// Query Slint's raw handle without causing native window creation.
pub fn trace_slint_native_handle(role: &str, stage: &str, window: &slint::Window) {
    if !TRACE_ENABLED.load(Ordering::Acquire) {
        return;
    }
    let handle = window.window_handle();
    match handle.window_handle() {
        Ok(handle) => match handle.as_raw() {
            RawWindowHandle::Win32(raw) => {
                let hwnd = HWND(raw.hwnd.get() as *mut _);
                write_line(format!(
                    "SLINT_HWND role={role:?} stage={stage:?} result=available slint_visible={} slint_minimized={} valid={} {}",
                    window.is_visible(),
                    window.is_minimized(),
                    unsafe { IsWindow(Some(hwnd)) }.as_bool(),
                    describe_window(hwnd)
                ));
            }
            other => write_line(format!(
                "SLINT_HWND role={role:?} stage={stage:?} result=wrong_platform handle={other:?}"
            )),
        },
        Err(error) => write_line(format!(
            "SLINT_HWND role={role:?} stage={stage:?} result=unavailable error={error:?} native_creation_may_be_deferred_until_event_loop=true slint_visible={} slint_minimized={}",
            window.is_visible(),
            window.is_minimized()
        )),
    }
}

fn open_trace_file() -> Result<(PathBuf, File), String> {
    let preferred_root = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let preferred = preferred_root
        .join("Carambola")
        .join("diagnostics")
        .join("window-lifecycle.log");
    match open_at(&preferred) {
        Ok(file) => Ok((preferred, file)),
        Err(preferred_error) => {
            let fallback = std::env::temp_dir()
                .join("Carambola")
                .join("diagnostics")
                .join("window-lifecycle.log");
            if fallback == preferred {
                return Err(format!(
                    "Could not open diagnostics log {}: {preferred_error}",
                    preferred.display()
                ));
            }
            open_at(&fallback)
                .map(|file| (fallback.clone(), file))
                .map_err(|fallback_error| {
                    format!(
                        "Could not open diagnostics log {} ({preferred_error}) or fallback {} ({fallback_error})",
                        preferred.display(),
                        fallback.display()
                    )
                })
        }
    }
}

fn open_at(path: &PathBuf) -> Result<File, std::io::Error> {
    if let Some(parent) = path.parent() {
        create_dir_all(parent)?;
    }
    OpenOptions::new().create(true).append(true).open(path)
}

unsafe extern "system" fn win_event_proc(
    _hook: HWINEVENTHOOK,
    event: u32,
    hwnd: HWND,
    object_id: i32,
    child_id: i32,
    event_thread: u32,
    event_time_ms: u32,
) {
    if !TRACE_ENABLED.load(Ordering::Acquire) || hwnd.0.is_null() {
        return;
    }
    if object_id != OBJID_WINDOW.0 || child_id != 0 {
        return;
    }

    let mut process_id = 0u32;
    let window_thread = unsafe { GetWindowThreadProcessId(hwnd, Some(&mut process_id)) };
    // The hook itself is scoped to this PID. A destroyed HWND may no longer
    // answer GetWindowThreadProcessId by the time the out-of-context hook runs.
    if process_id != TRACE_PROCESS_ID.load(Ordering::Acquire) && event != EVENT_OBJECT_DESTROY {
        return;
    }

    if event == EVENT_OBJECT_CREATE {
        register_window(hwnd, true, "UnexpectedAppWindow");
    }

    let foreground = unsafe { GetForegroundWindow() };
    let description = describe_window(hwnd);
    write_line(format!(
        "WIN_EVENT event={} event_id=0x{event:04x} hwnd={} process_id={process_id} window_thread={window_thread} event_thread={event_thread} event_time_ms={event_time_ms} foreground={} window={}",
        event_name(event),
        hwnd_text(hwnd),
        hwnd_text(foreground),
        description
    ));
    if matches!(event, EVENT_OBJECT_CREATE | EVENT_OBJECT_SHOW) {
        let role = window_role(&window_title(hwnd));
        if matches!(role, "Settings" | "Overlay") {
            write_line(format!(
                "SLINT_NATIVE_WINDOW role={role:?} event={} hwnd={} {description}",
                event_name(event),
                hwnd_text(hwnd)
            ));
        }
    }

    if matches!(
        event,
        EVENT_OBJECT_CREATE
            | EVENT_OBJECT_DESTROY
            | EVENT_OBJECT_SHOW
            | EVENT_OBJECT_HIDE
            | EVENT_SYSTEM_MINIMIZESTART
            | EVENT_SYSTEM_MINIMIZEEND
    ) {
        trace_process_windows(event_name(event));
    }
    if event == EVENT_OBJECT_DESTROY {
        forget_window(hwnd);
    }
}

unsafe extern "system" fn enum_window_proc(hwnd: HWND, parameter: LPARAM) -> WinBool {
    if hwnd.0.is_null() {
        return WinBool(1);
    }
    let mut process_id = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut process_id)) };
    if process_id == TRACE_PROCESS_ID.load(Ordering::Acquire) {
        let windows = unsafe { &mut *(parameter.0 as *mut Vec<HWND>) };
        windows.push(hwnd);
    }
    WinBool(1)
}

fn process_windows() -> Vec<HWND> {
    let mut windows = Vec::new();
    let parameter = LPARAM((&mut windows as *mut Vec<HWND>) as isize);
    let result = unsafe { EnumWindows(Some(enum_window_proc), parameter) };
    if let Err(error) = result {
        write_line(format!("WINDOW_ENUMERATION_FAILED error={error:?}"));
    }
    let top_level_windows = windows.clone();
    for hwnd in top_level_windows {
        let result = unsafe { EnumChildWindows(Some(hwnd), Some(enum_window_proc), parameter) };
        if !result.as_bool() {
            write_line(format!(
                "CHILD_WINDOW_ENUMERATION_FAILED parent={}",
                hwnd_text(hwnd)
            ));
        }
    }
    windows
}

fn trace_process_windows(reason: &str) {
    if !TRACE_ENABLED.load(Ordering::Acquire) {
        return;
    }
    let windows = process_windows();
    write_window_list(reason, &windows);
}

fn write_window_list(reason: &str, windows: &[HWND]) {
    write_line(format!(
        "WINDOW_SNAPSHOT_BEGIN reason={reason:?} count={}",
        windows.len()
    ));
    for hwnd in windows {
        write_line(format!("WINDOW {}", describe_window(*hwnd)));
    }
    write_line(format!("WINDOW_SNAPSHOT_END reason={reason:?}"));
}

fn role_hwnd(role: &str, windows: &[HWND]) -> Option<HWND> {
    windows.iter().copied().find(|hwnd| {
        let title = window_title(*hwnd);
        match role {
            "Settings" => {
                title.starts_with(SETTINGS_TITLE_START) && title.ends_with(SETTINGS_TITLE_END)
            }
            "Overlay" => title == OVERLAY_TITLE,
            "TrayHost" => title == TRAY_HOST_TITLE,
            _ => false,
        }
    })
}

fn describe_window(hwnd: HWND) -> String {
    let mut process_id = 0u32;
    let thread_id = unsafe { GetWindowThreadProcessId(hwnd, Some(&mut process_id)) };
    let title = window_title(hwnd);
    let mut class_name = [0u16; 256];
    let class_len = unsafe { GetClassNameW(hwnd, &mut class_name) }.max(0) as usize;
    let class_name = String::from_utf16_lossy(&class_name[..class_len.min(class_name.len())]);
    let role = if class_name == "CarambolaNativeTrayHost" {
        "TrayHost"
    } else {
        window_role(&title)
    };
    let (instance, origin, known_role) = register_window(hwnd, false, role);
    let style = unsafe { GetWindowLongPtrW(hwnd, GWL_STYLE) } as u32;
    let ex_style = unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) } as u32;
    let parent = unsafe { GetParent(hwnd) }.unwrap_or_default();
    let owner = unsafe { GetWindow(hwnd, GW_OWNER) }.unwrap_or_default();
    let layered = ex_style & WS_EX_LAYERED.0 != 0;
    let visible = unsafe { IsWindowVisible(hwnd) }.as_bool();
    let minimized = unsafe { IsIconic(hwnd) }.as_bool();
    let enabled = unsafe { IsWindowEnabled(hwnd) }.as_bool();
    let window_bounds = describe_rect(hwnd, true);
    let client_bounds = describe_rect(hwnd, false);
    let mut color_key = COLORREF(0);
    let mut alpha = 0u8;
    let mut alpha_flags = LAYERED_WINDOW_ATTRIBUTES_FLAGS(0);
    let layered_attributes = if layered {
        match unsafe {
            GetLayeredWindowAttributes(
                hwnd,
                Some(&mut color_key),
                Some(&mut alpha),
                Some(&mut alpha_flags),
            )
        } {
            Ok(()) => format!(
                "available alpha={alpha} alpha_enabled={} color_key=0x{:08x} color_key_enabled={}",
                alpha_flags.contains(windows::Win32::UI::WindowsAndMessaging::LWA_ALPHA),
                color_key.0,
                alpha_flags.contains(windows::Win32::UI::WindowsAndMessaging::LWA_COLORKEY)
            ),
            Err(error) => format!("unavailable error={error:?}"),
        }
    } else {
        "not-layered".into()
    };

    format!(
        "hwnd={} instance={instance} origin={origin} parent={} owner={} role={known_role:?} pid={process_id} tid={thread_id} title={title:?} class={class_name:?} visible={visible} minimized={minimized} enabled={enabled} window_bounds=({window_bounds}) client_bounds=({client_bounds}) style=0x{style:08x} ex_style=0x{ex_style:08x} layered={layered} layered_attributes=({layered_attributes})",
        hwnd_text(hwnd),
        hwnd_text(parent),
        hwnd_text(owner),
    )
}

fn register_window(
    hwnd: HWND,
    created: bool,
    role: &'static str,
) -> (u64, &'static str, &'static str) {
    let registry = WINDOW_IDENTITIES.get_or_init(|| Mutex::new(WindowIdentities::default()));
    let Ok(mut registry) = registry.lock() else {
        return (0, "registry-unavailable", role);
    };
    let key = hwnd.0 as isize;
    if !registry.by_hwnd.contains_key(&key) {
        registry.next_instance += 1;
        let instance = registry.next_instance;
        registry.by_hwnd.insert(
            key,
            WindowIdentity {
                instance,
                origin: if created {
                    "create-event"
                } else {
                    "first-observed"
                },
                role,
            },
        );
    }
    let identity = registry
        .by_hwnd
        .get_mut(&key)
        .expect("window identity just inserted");
    if created && identity.origin == "first-observed" {
        identity.origin = "create-event-delivered-later";
    }
    if role != "UnexpectedAppWindow" {
        identity.role = role;
    }
    (identity.instance, identity.origin, identity.role)
}

fn forget_window(hwnd: HWND) {
    if let Some(registry) = WINDOW_IDENTITIES.get() {
        if let Ok(mut registry) = registry.lock() {
            registry.by_hwnd.remove(&(hwnd.0 as isize));
        }
    }
}

fn describe_rect(hwnd: HWND, window: bool) -> String {
    let mut rect = windows::Win32::Foundation::RECT::default();
    let result = unsafe {
        if window {
            GetWindowRect(hwnd, &mut rect)
        } else {
            GetClientRect(hwnd, &mut rect)
        }
    };
    match result {
        Ok(()) => format!(
            "left={} top={} right={} bottom={} width={} height={}",
            rect.left,
            rect.top,
            rect.right,
            rect.bottom,
            rect.right - rect.left,
            rect.bottom - rect.top
        ),
        Err(error) => format!("unavailable error={error:?}"),
    }
}

fn window_title(hwnd: HWND) -> String {
    let length = unsafe { GetWindowTextLengthW(hwnd) }.max(0) as usize;
    let capacity = length.saturating_add(1).clamp(2, 4096);
    let mut text = vec![0u16; capacity];
    let copied = unsafe { GetWindowTextW(hwnd, &mut text) }.max(0) as usize;
    String::from_utf16_lossy(&text[..copied.min(text.len())])
}

fn window_role(title: &str) -> &'static str {
    match title {
        _ if title.starts_with(SETTINGS_TITLE_START) && title.ends_with(SETTINGS_TITLE_END) => {
            "Settings"
        }
        OVERLAY_TITLE => "Overlay",
        TRAY_HOST_TITLE => "TrayHost",
        _ => "UnexpectedAppWindow",
    }
}

fn event_name(event: u32) -> &'static str {
    match event {
        EVENT_OBJECT_CREATE => "OBJECT_CREATE",
        EVENT_OBJECT_DESTROY => "OBJECT_DESTROY",
        EVENT_OBJECT_SHOW => "OBJECT_SHOW",
        EVENT_OBJECT_HIDE => "OBJECT_HIDE",
        EVENT_OBJECT_LOCATIONCHANGE => "OBJECT_LOCATIONCHANGE",
        EVENT_SYSTEM_FOREGROUND => "SYSTEM_FOREGROUND",
        EVENT_SYSTEM_MINIMIZESTART => "SYSTEM_MINIMIZESTART",
        EVENT_SYSTEM_MINIMIZEEND => "SYSTEM_MINIMIZEEND",
        _ => "OTHER",
    }
}

fn hwnd_text(hwnd: HWND) -> String {
    format!("0x{:X}", hwnd.0 as usize)
}

fn write_line(line: String) {
    if !TRACE_ENABLED.load(Ordering::Acquire) {
        return;
    }
    let epoch_ns = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let elapsed_ns = TRACE_STARTED
        .get()
        .map(Instant::elapsed)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let elapsed_us = elapsed_ns / 1_000;
    let thread_id = unsafe { GetCurrentThreadId() };
    let tick_ms = unsafe { GetTickCount64() };
    let tick_ms_low32 = tick_ms as u32;
    let action_id = ACTION_ID.load(Ordering::Acquire);
    let action_phase = match ACTION_PHASE.load(Ordering::Acquire) {
        1 => "active",
        2 => "completed",
        _ => "none",
    };
    if let Some(file) = TRACE_FILE.get() {
        if let Ok(mut file) = file.lock() {
            let _ = writeln!(
                file,
                "unix_ns={epoch_ns} elapsed_ns={elapsed_ns} elapsed_us={elapsed_us} tick_ms={tick_ms} tick_ms_low32={tick_ms_low32} startup_id={} pid={} thread_id={thread_id} action_id={action_id} action_phase={action_phase} {line}",
                STARTUP_ID.get().map(String::as_str).unwrap_or("uninitialized"),
                TRACE_PROCESS_ID.load(Ordering::Acquire)
            );
            let _ = file.flush();
        }
    }
}
