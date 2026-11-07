//! AppKit and display-change integration is implemented in a later Phase B step.

pub fn initialize() -> Result<(), String> {
    // AppKit reports backing scale per window/display. No process-wide DPI mode
    // is required on macOS; per-display transition handling remains to be added.
    Ok(())
}

pub fn capture_primary() -> Result<crate::core::frame::CapturedFrame, String> {
    Err(
        "macOS screen capture is not implemented yet; ScreenCaptureKit integration is required"
            .into(),
    )
}

/// Request a Notification Center message through macOS's built-in
/// AppleScript notification command. Arguments are escaped and passed without
/// invoking a shell.
pub fn show_notification(message: &str) -> Result<(), String> {
    use std::process::Command;

    let message = message
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\r', " ")
        .replace('\n', " ");
    let script = format!("display notification \"{message}\" with title \"Carambola\"");
    let status = Command::new("osascript")
        .arg("-e")
        .arg(script)
        .status()
        .map_err(|error| format!("Could not start osascript: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("osascript exited with status {status}"))
    }
}
