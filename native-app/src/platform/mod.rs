#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(target_os = "windows")]
pub mod windows;

/// Configure native DPI behavior before the first window is created.
pub fn initialize() -> Result<(), String> {
    #[cfg(target_os = "windows")]
    windows::initialize()?;

    #[cfg(target_os = "macos")]
    macos::initialize()?;

    Ok(())
}

pub fn capture_primary() -> Result<crate::core::frame::CapturedFrame, String> {
    #[cfg(target_os = "windows")]
    return windows::capture_primary();

    #[cfg(target_os = "macos")]
    return macos::capture_primary();

    #[allow(unreachable_code)]
    Err("Native screen capture is not implemented for this platform".into())
}

pub fn prepare_capture() -> Result<(), String> {
    #[cfg(target_os = "windows")]
    return windows::prepare_capture();

    #[allow(unreachable_code)]
    Ok(())
}

pub fn reset_capture_affinity() -> Result<(), String> {
    #[cfg(target_os = "windows")]
    return windows::reset_capture_affinity();

    #[allow(unreachable_code)]
    Ok(())
}

pub fn activate_settings() -> Result<(), String> {
    #[cfg(target_os = "windows")]
    return windows::activate_settings();

    #[allow(unreachable_code)]
    Ok(())
}

pub fn activate_overlay() -> Result<(), String> {
    #[cfg(target_os = "windows")]
    return windows::activate_overlay();

    #[allow(unreachable_code)]
    Ok(())
}

/// Show the native PNG Save As dialog. Supported by the Windows prototype.
pub fn save_png_dialog() -> Result<Option<std::path::PathBuf>, String> {
    #[cfg(target_os = "windows")]
    return windows::save_png_dialog();

    #[cfg(not(target_os = "windows"))]
    Err("Native PNG Save As is not implemented on this platform".into())
}

/// Transfer an opaque screenshot image to the platform image clipboard.
pub fn copy_image_to_clipboard(frame: &crate::core::frame::CapturedFrame) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    return windows::copy_image_to_clipboard(frame);

    #[cfg(not(target_os = "windows"))]
    {
        let _ = frame;
        Err("Image clipboard support is not implemented on this platform".into())
    }
}

/// Opt-in Win32 cursor trace for diagnosing a physically missing pointer.
pub fn diagnose_cursor(stage: &str) {
    #[cfg(target_os = "windows")]
    windows::diagnose_cursor(stage);
    #[cfg(not(target_os = "windows"))]
    let _ = stage;
}

pub fn diagnose_cursor_motion() {
    #[cfg(target_os = "windows")]
    windows::diagnose_cursor_motion();
}

pub fn trace_event(event: &str) {
    #[cfg(target_os = "windows")]
    window_diagnostics::trace_event(event);

    #[cfg(not(target_os = "windows"))]
    let _ = event;
}

pub fn begin_window_action(action: &str) -> u64 {
    #[cfg(target_os = "windows")]
    return window_diagnostics::begin_action(action);
    #[cfg(not(target_os = "windows"))]
    {
        let _ = action;
        0
    }
}

pub fn finish_window_action(id: u64, outcome: &str) {
    #[cfg(target_os = "windows")]
    window_diagnostics::finish_action(id, outcome);
    #[cfg(not(target_os = "windows"))]
    let _ = (id, outcome);
}

pub fn clear_window_action(reason: &str) {
    #[cfg(target_os = "windows")]
    window_diagnostics::clear_action(reason);
    #[cfg(not(target_os = "windows"))]
    let _ = reason;
}

pub fn trace_component_state(role: &str, operation: &str, visible: bool, minimized: bool) {
    #[cfg(target_os = "windows")]
    window_diagnostics::trace_component_state(role, operation, visible, minimized);

    #[cfg(not(target_os = "windows"))]
    let _ = (role, operation, visible, minimized);
}

pub fn trace_slint_native_handle(role: &str, stage: &str, window: &slint::Window) {
    #[cfg(target_os = "windows")]
    window_diagnostics::trace_slint_native_handle(role, stage, window);
    #[cfg(not(target_os = "windows"))]
    let _ = (role, stage, window);
}

pub fn trace_native_event(operation: &str) {
    #[cfg(target_os = "windows")]
    window_diagnostics::trace_native_event(operation);

    #[cfg(not(target_os = "windows"))]
    let _ = operation;
}

#[cfg(target_os = "windows")]
pub mod tray;
#[cfg(target_os = "windows")]
pub mod window_diagnostics;
