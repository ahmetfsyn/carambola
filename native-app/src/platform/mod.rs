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

pub fn restore_settings() -> Result<(), String> {
    #[cfg(target_os = "windows")]
    return windows::restore_settings();

    #[allow(unreachable_code)]
    Ok(())
}

pub fn activate_overlay() -> Result<(), String> {
    #[cfg(target_os = "windows")]
    return windows::activate_overlay();

    #[allow(unreachable_code)]
    Ok(())
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
