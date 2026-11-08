#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(target_os = "windows")]
pub mod windows;
#[cfg(target_os = "windows")]
mod windows_notifications;

/// Configure native DPI behavior before the first window is created.
pub fn initialize() -> Result<(), String> {
    #[cfg(target_os = "windows")]
    windows::initialize()?;

    #[cfg(target_os = "macos")]
    macos::initialize()?;

    Ok(())
}

/// Capture one explicitly selected display. The ID is resolved against the
/// current topology at capture time so a disconnected monitor is an error.
pub fn capture_display(
    requested_id: Option<&str>,
) -> Result<crate::core::frame::CapturedFrame, String> {
    #[cfg(target_os = "windows")]
    return windows::capture_display(requested_id);

    #[cfg(target_os = "macos")]
    {
        if requested_id.is_some() {
            return Err("Display-specific capture is not implemented on macOS".into());
        }
        return macos::capture_primary();
    }

    #[allow(unreachable_code)]
    Err("Native screen capture is not implemented for this platform".into())
}

/// Show a short native success notification without creating another window.
/// Windows uses the already-running tray icon; macOS and Linux use their
/// standard notification entry points.
pub fn show_notification(message: &str, owner_hwnd: isize) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    return tray::show_notification(owner_hwnd, message);

    #[cfg(target_os = "macos")]
    return macos::show_notification(message);

    #[cfg(target_os = "linux")]
    {
        use std::process::Command;
        let status = Command::new("notify-send")
            .arg("--app-name=Carambola")
            .arg("Carambola")
            .arg(message)
            .status()
            .map_err(|error| format!("Could not start notify-send: {error}"))?;
        if status.success() {
            Ok(())
        } else {
            Err(format!("notify-send exited with status {status}"))
        }
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        let _ = (message, owner_hwnd);
        Err("Native notifications are not supported on this platform".into())
    }
}

/// Show a notification for a successful screenshot export. Windows uses a
/// toast hero image built from this exact rendered screenshot; other platforms
/// keep their existing text notification behavior.
pub fn show_screenshot_notification(
    message: &str,
    owner_hwnd: isize,
    screenshot: &crate::core::frame::CapturedFrame,
) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    return windows_notifications::show_screenshot_notification(message, owner_hwnd, screenshot);

    #[cfg(not(target_os = "windows"))]
    {
        let _ = screenshot;
        show_notification(message, owner_hwnd)
    }
}

/// Resolve the display under the cursor at request time. Non-Windows
/// platforms retain their existing primary-display capture behavior.
pub fn display_at_cursor_id() -> Result<Option<String>, String> {
    #[cfg(target_os = "windows")]
    return windows::display_at_cursor().map(|display| Some(display.id));

    #[cfg(not(target_os = "windows"))]
    Ok(None)
}

/// Return current display metadata in physical virtual-desktop coordinates.
pub fn enumerate_displays() -> Result<Vec<crate::core::display::DisplayMetadata>, String> {
    #[cfg(target_os = "windows")]
    return windows::enumerate_displays();

    #[cfg(not(target_os = "windows"))]
    Ok(Vec::new())
}

pub fn prepare_capture() -> Result<(), String> {
    #[cfg(target_os = "windows")]
    return windows::prepare_capture();

    #[allow(unreachable_code)]
    Ok(())
}

pub fn activate_overlay() -> Result<(), String> {
    #[cfg(target_os = "windows")]
    return windows::activate_overlay();

    #[allow(unreachable_code)]
    Ok(())
}

/// Show the operating system Save As dialog, starting in the chosen folder.
pub fn save_png_dialog(
    default_directory: Option<&std::path::Path>,
) -> Result<Option<std::path::PathBuf>, String> {
    #[cfg(target_os = "windows")]
    return windows::save_png_dialog(default_directory);

    #[cfg(target_os = "macos")]
    return macos::save_png_dialog(default_directory);

    #[cfg(target_os = "linux")]
    return linux_dialog::save_png_dialog(default_directory);

    #[allow(unreachable_code)]
    Err("Native PNG Save As is not implemented on this platform".into())
}

pub fn pick_save_directory(
    initial_directory: Option<&std::path::Path>,
) -> Result<Option<std::path::PathBuf>, String> {
    #[cfg(target_os = "windows")]
    return windows::pick_save_directory(initial_directory);

    #[cfg(target_os = "macos")]
    return macos::pick_save_directory(initial_directory);

    #[cfg(target_os = "linux")]
    return linux_dialog::pick_save_directory(initial_directory);

    #[allow(unreachable_code)]
    Err("A native folder picker is not implemented on this platform".into())
}

/// Transfer an opaque screenshot image to the platform image clipboard.
pub fn copy_image_to_clipboard(
    frame: &crate::core::frame::CapturedFrame,
    owner_hwnd: isize,
) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    return windows::copy_image_to_clipboard(frame, owner_hwnd);

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        use std::borrow::Cow;
        let _ = owner_hwnd;
        let mut clipboard = arboard::Clipboard::new()
            .map_err(|error| format!("Could not open the image clipboard: {error}"))?;
        clipboard
            .set_image(arboard::ImageData {
                width: frame.width() as usize,
                height: frame.height() as usize,
                bytes: Cow::Borrowed(frame.pixels()),
            })
            .map_err(|error| format!("Could not copy the screenshot image: {error}"))
    }
}

pub fn copy_text_to_clipboard(text: &str, owner_hwnd: isize) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    return windows::copy_text_to_clipboard(text, owner_hwnd);

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        let _ = owner_hwnd;
        let mut clipboard = arboard::Clipboard::new()
            .map_err(|error| format!("Could not open the text clipboard: {error}"))?;
        clipboard
            .set_text(text.to_owned())
            .map_err(|error| format!("Could not copy recognized text: {error}"))
    }
}

/// Perform on-device OCR. Windows uses the installed Windows OCR language
/// packs, macOS uses Vision, and Linux invokes the local Tesseract executable.
pub fn recognize_text(frame: &crate::core::frame::CapturedFrame) -> Result<String, String> {
    #[cfg(target_os = "windows")]
    return windows::recognize_text(frame);

    #[cfg(target_os = "macos")]
    return macos::recognize_text(frame);

    #[cfg(target_os = "linux")]
    return linux_ocr::recognize_text(frame);

    #[allow(unreachable_code)]
    Err("Offline OCR is not supported on this platform".into())
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
pub mod taskbar;
#[cfg(target_os = "windows")]
pub mod tray;
#[cfg(target_os = "windows")]
pub mod window_diagnostics;

#[cfg(target_os = "linux")]
mod linux_dialog {
    use std::{
        path::{Path, PathBuf},
        process::Command,
    };

    fn run_zenity(arguments: &[String]) -> Result<Option<PathBuf>, String> {
        match Command::new("zenity").args(arguments).output() {
            Ok(output) if output.status.success() => {
                let path = String::from_utf8_lossy(&output.stdout).trim().to_owned();
                Ok((!path.is_empty()).then(|| PathBuf::from(path)))
            }
            Ok(output) if output.status.code() == Some(1) => Ok(None),
            Ok(output) => Err(format!(
                "zenity could not display the folder dialog: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Err("Install zenity or kdialog to use the native file dialogs".into())
            }
            Err(error) => Err(format!("Could not start zenity: {error}")),
        }
    }

    fn run_kdialog(arguments: &[String]) -> Result<Option<PathBuf>, String> {
        match Command::new("kdialog").args(arguments).output() {
            Ok(output) if output.status.success() => {
                let path = String::from_utf8_lossy(&output.stdout).trim().to_owned();
                Ok((!path.is_empty()).then(|| PathBuf::from(path)))
            }
            Ok(output) if output.status.code() == Some(1) => Ok(None),
            Ok(output) => Err(format!("kdialog exited with status {}", output.status)),
            Err(error) => Err(format!("Could not start kdialog: {error}")),
        }
    }

    fn show_dialog(
        zenity_arguments: Vec<String>,
        kdialog_arguments: Vec<String>,
    ) -> Result<Option<PathBuf>, String> {
        match run_zenity(&zenity_arguments) {
            Err(error) if error.starts_with("Install zenity") => run_kdialog(&kdialog_arguments),
            result => result,
        }
    }

    pub fn pick_save_directory(initial: Option<&Path>) -> Result<Option<PathBuf>, String> {
        let mut zenity = vec![
            "--file-selection".into(),
            "--directory".into(),
            "--title=Choose screenshot save folder".into(),
        ];
        let mut kdialog = vec!["--getexistingdirectory".into()];
        if let Some(path) = initial.filter(|path| path.is_dir()) {
            zenity.push(format!("--filename={}/", path.display()));
            kdialog.push(path.display().to_string());
        } else {
            kdialog.push(String::new());
        }
        Ok(show_dialog(zenity, kdialog)?.map(|path| path.canonicalize().unwrap_or(path)))
    }

    pub fn save_png_dialog(default_directory: Option<&Path>) -> Result<Option<PathBuf>, String> {
        let mut zenity = vec![
            "--file-selection".into(),
            "--save".into(),
            "--confirm-overwrite".into(),
            "--title=Save annotated screenshot".into(),
        ];
        let mut kdialog = vec!["--getsavefilename".into()];
        let initial = default_directory
            .filter(|path| path.is_dir())
            .map(|path| path.join("screenshot.png"));
        if let Some(path) = initial {
            zenity.push(format!("--filename={}", path.display()));
            kdialog.push(path.display().to_string());
        } else {
            kdialog.push("screenshot.png".into());
        }
        Ok(show_dialog(zenity, kdialog)?
            .map(crate::core::export::ensure_png_extension)
            .transpose()?)
    }
}

#[cfg(target_os = "linux")]
mod linux_ocr {
    use crate::core::frame::CapturedFrame;
    use image::{codecs::png::PngEncoder, ExtendedColorType, ImageEncoder};
    use std::io::Write;
    use std::process::{Command, Stdio};

    pub fn recognize_text(frame: &CapturedFrame) -> Result<String, String> {
        let mut png = Vec::new();
        PngEncoder::new(&mut png)
            .write_image(
                frame.pixels(),
                frame.width(),
                frame.height(),
                ExtendedColorType::Rgba8,
            )
            .map_err(|error| format!("Could not encode the crop for Tesseract: {error}"))?;
        let mut child = Command::new("tesseract")
            .args(["stdin", "stdout", "--oem", "1", "--psm", "6", "-l", "eng"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| if error.kind() == std::io::ErrorKind::NotFound {
                "Install Tesseract OCR and the eng.traineddata language file to use offline text recognition".to_string()
            } else { format!("Could not start Tesseract OCR: {error}") })?;
        child
            .stdin
            .take()
            .ok_or("Tesseract stdin is unavailable")?
            .write_all(&png)
            .map_err(|error| format!("Could not send the crop to Tesseract: {error}"))?;
        let output = child
            .wait_with_output()
            .map_err(|error| format!("Could not read Tesseract output: {error}"))?;
        if !output.status.success() {
            return Err(format!(
                "Tesseract OCR failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        String::from_utf8(output.stdout)
            .map_err(|error| format!("Tesseract returned invalid UTF-8: {error}"))
    }
}
