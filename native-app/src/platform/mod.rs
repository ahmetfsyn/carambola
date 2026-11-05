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
