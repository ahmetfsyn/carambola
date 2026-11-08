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

fn escape_applescript_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn run_osascript(script: String) -> Result<Option<std::path::PathBuf>, String> {
    use std::process::Command;
    let output = Command::new("osascript")
        .arg("-e")
        .arg(script)
        .output()
        .map_err(|error| format!("Could not start the macOS file picker: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "macOS file picker failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let path = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    Ok((!path.is_empty()).then(|| std::path::PathBuf::from(path)))
}

pub fn pick_save_directory(
    initial_directory: Option<&std::path::Path>,
) -> Result<Option<std::path::PathBuf>, String> {
    let initial = initial_directory
        .filter(|path| path.is_dir())
        .and_then(|path| path.to_str())
        .map(|path| {
            format!(
                " default location POSIX file \"{}\"",
                escape_applescript_string(path)
            )
        })
        .unwrap_or_default();
    let script = format!(
        "try\nset selectedFolder to choose folder with prompt \"Choose screenshot save folder\"{initial}\nPOSIX path of selectedFolder\non error number -128\nreturn \"\"\nend try"
    );
    run_osascript(script)
}

pub fn save_png_dialog(
    default_directory: Option<&std::path::Path>,
) -> Result<Option<std::path::PathBuf>, String> {
    let initial = default_directory
        .filter(|path| path.is_dir())
        .and_then(|path| path.to_str())
        .map(|path| {
            format!(
                " default location POSIX file \"{}\"",
                escape_applescript_string(path)
            )
        })
        .unwrap_or_default();
    let script = format!(
        "try\nset selectedFile to choose file name with prompt \"Save annotated screenshot\" default name \"screenshot.png\"{initial}\nPOSIX path of selectedFile\non error number -128\nreturn \"\"\nend try"
    );
    run_osascript(script)?
        .map(crate::core::export::ensure_png_extension)
        .transpose()
}

pub fn recognize_text(frame: &crate::core::frame::CapturedFrame) -> Result<String, String> {
    use std::{ffi::CStr, os::raw::c_char};

    unsafe extern "C" {
        fn carambola_vision_ocr(
            rgba: *const u8,
            width: u32,
            height: u32,
            output: *mut c_char,
            output_capacity: usize,
        ) -> i32;
    }

    let mut output = vec![0i8; 65_536];
    let result = unsafe {
        carambola_vision_ocr(
            frame.pixels().as_ptr(),
            frame.width(),
            frame.height(),
            output.as_mut_ptr(),
            output.len(),
        )
    };
    if result != 0 {
        return Err(match result {
            -1 => "macOS Vision could not create an OCR image".into(),
            -2 => "macOS Vision could not recognize the selected crop".into(),
            -3 => "Recognized text exceeded the supported output size".into(),
            _ => format!("macOS Vision OCR failed (code {result})"),
        });
    }
    let output = unsafe { CStr::from_ptr(output.as_ptr()) };
    output
        .to_str()
        .map(str::to_owned)
        .map_err(|error| format!("macOS Vision returned invalid UTF-8: {error}"))
}
