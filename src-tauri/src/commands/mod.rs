use crate::app_state::{AppState, DisplayInfo};
use crate::clipboard::{copy_rgba_to_clipboard, copy_text_to_clipboard};
use crate::ocr::extract_text;
use chrono::Local;
use image::{codecs::png::PngEncoder, ExtendedColorType, ImageEncoder};
use log::info;
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::BufWriter;
use tauri::ipc::InvokeBody;
use tauri::{AppHandle, Manager, State};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CropRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[tauri::command]
pub fn get_display_info(state: State<'_, AppState>) -> Result<DisplayInfo, String> {
    if let Some(frame) = state.get_frame() {
        Ok(frame.display_info.clone())
    } else {
        Err("No captured frame available".to_string())
    }
}

/// Returns the freshly captured PNG as a raw binary IPC body. Returning
/// `Vec<u8>` directly would serialize every byte as a JSON number, which costs
/// several megabytes of string parsing inside the webview on every activation.
#[tauri::command]
pub fn get_current_frame(state: State<'_, AppState>) -> Result<tauri::ipc::Response, String> {
    if let Some(frame) = state.get_frame() {
        Ok(tauri::ipc::Response::new(frame.png_bytes.clone()))
    } else {
        Err("No captured frame available".to_string())
    }
}

/// Second half of the activation handshake: called by the frontend once the new
/// screenshot has been decoded and the previous session state has been cleared.
#[tauri::command]
pub fn present_overlay(app_handle: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    crate::window::present_overlay(&app_handle, &state)
}

/// Decodes the raw RGBA payload produced by the frontend compositor. The buffer
/// is laid out as `[u32 LE width][u32 LE height][RGBA bytes...]` so the annotation
/// compositing (including pixelation redaction) happens once, on the real
/// screenshot pixels, with no extra image encode/decode round trip.
fn parse_rgba_payload(bytes: &[u8]) -> Result<(u32, u32, &[u8]), String> {
    if bytes.len() < 8 {
        return Err("Image payload is too short".to_string());
    }
    let width = u32::from_le_bytes(bytes[0..4].try_into().unwrap());
    let height = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
    let expected = 8usize
        .checked_add((width as usize) * (height as usize) * 4)
        .ok_or_else(|| "Image dimensions are too large".to_string())?;
    if width == 0 || height == 0 || bytes.len() != expected {
        return Err("Image payload has invalid dimensions".to_string());
    }
    Ok((width, height, &bytes[8..]))
}

fn raw_body<'a>(request: &'a tauri::ipc::Request<'_>) -> Result<&'a [u8], String> {
    match request.body() {
        InvokeBody::Raw(bytes) => Ok(bytes),
        _ => Err("Expected raw RGBA request body".to_string()),
    }
}

/// Copies the composited crop (annotations + redaction already applied) to the
/// system clipboard.
#[tauri::command]
pub fn copy_crop_image(request: tauri::ipc::Request<'_>) -> Result<(), String> {
    let bytes = raw_body(&request)?;
    let (width, height, rgba) = parse_rgba_payload(bytes)?;
    copy_rgba_to_clipboard(rgba, width as usize, height as usize)?;
    info!(
        "Successfully copied {}x{} composite to clipboard",
        width, height
    );
    Ok(())
}

/// Saves the composited crop (annotations + redaction already applied) to disk.
#[tauri::command]
pub fn save_crop_image(request: tauri::ipc::Request<'_>) -> Result<String, String> {
    let bytes = raw_body(&request)?;
    let (width, height, rgba) = parse_rgba_payload(bytes)?;

    let pictures_dir = dirs::picture_dir().unwrap_or_else(|| std::env::current_dir().unwrap());
    let carambola_dir = pictures_dir.join("Carambola");
    std::fs::create_dir_all(&carambola_dir)
        .map_err(|e| format!("Failed to create save directory: {e}"))?;

    let timestamp = Local::now().format("%Y-%m-%d_%H%M%S");
    let file_path = carambola_dir.join(format!("Carambola_{timestamp}.png"));

    let file = File::create(&file_path).map_err(|e| format!("Failed to create file: {e}"))?;
    let mut writer = BufWriter::new(file);

    PngEncoder::new(&mut writer)
        .write_image(rgba, width, height, ExtendedColorType::Rgba8)
        .map_err(|e| format!("Failed to write PNG: {e}"))?;

    let saved_str = file_path.to_string_lossy().to_string();
    info!("Saved screenshot to: {}", saved_str);
    Ok(saved_str)
}

fn extract_cropped_rgba(
    frame_rgba: &[u8],
    frame_w: u32,
    frame_h: u32,
    crop: &CropRect,
    scale: f64,
) -> Result<(Vec<u8>, usize, usize), String> {
    let sx = ((crop.x * scale).round() as u32).min(frame_w);
    let sy = ((crop.y * scale).round() as u32).min(frame_h);
    let sw = ((crop.width * scale).round() as u32).min(frame_w.saturating_sub(sx));
    let sh = ((crop.height * scale).round() as u32).min(frame_h.saturating_sub(sy));

    if sw == 0 || sh == 0 {
        return Err("Crop area is zero".into());
    }

    let mut crop_rgba = Vec::with_capacity((sw * sh * 4) as usize);

    for row in sy..(sy + sh) {
        let start_idx = ((row * frame_w + sx) * 4) as usize;
        let end_idx = start_idx + (sw * 4) as usize;
        if end_idx <= frame_rgba.len() {
            crop_rgba.extend_from_slice(&frame_rgba[start_idx..end_idx]);
        }
    }

    Ok((crop_rgba, sw as usize, sh as usize))
}

#[tauri::command]
pub fn hide_overlay(app_handle: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    state.set_overlay_visible(false);
    if let Some(window) = app_handle.get_webview_window(crate::window::OVERLAY_LABEL) {
        let _ = window.hide();
    }
    // The overlay has fully closed: release the single-session lock so the next
    // global shortcut activation can start a fresh capture. A Settings request
    // that was never presented belongs to this aborted session and must not
    // reopen Settings during a later capture.
    state.clear_settings_on_present();
    state.end_session();
    info!("Overlay hidden; capture session released");
    Ok(())
}

#[tauri::command]
pub fn extract_ocr(
    crop: CropRect,
    scale_factor: f64,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let frame = state
        .get_frame()
        .ok_or_else(|| "No captured frame available".to_string())?;

    let (crop_rgba, w, h) =
        extract_cropped_rgba(&frame.rgba, frame.width, frame.height, &crop, scale_factor)?;

    let text = extract_text(&crop_rgba, w as u32, h as u32)?;
    let _ = copy_text_to_clipboard(&text);
    info!("OCR extracted {} chars and copied to clipboard", text.len());
    Ok(text)
}

#[tauri::command]
pub fn get_shortcut() -> Result<String, String> {
    Ok(crate::hotkey::load_or_create_config())
}

#[tauri::command]
pub fn set_shortcut(app_handle: AppHandle, shortcut: String) -> Result<(), String> {
    crate::hotkey::update_global_shortcut(&app_handle, &shortcut)
}

/// Reports the real OS startup registration, not just the stored preference.
#[tauri::command]
pub fn get_autostart(app_handle: AppHandle) -> Result<crate::autostart::AutostartStatus, String> {
    Ok(crate::autostart::status(&app_handle))
}

/// Enables/disables Start on Boot and persists the explicit choice.
#[tauri::command]
pub fn set_autostart(
    app_handle: AppHandle,
    enabled: bool,
) -> Result<crate::autostart::AutostartStatus, String> {
    crate::autostart::set_enabled(&app_handle, enabled)
}

#[derive(Clone, Debug, Serialize)]
pub struct ScreenshotNotificationSettings {
    pub enabled: bool,
    pub permission_prompted: bool,
}

fn notification_settings(config: &crate::config::UserConfig) -> ScreenshotNotificationSettings {
    ScreenshotNotificationSettings {
        enabled: config.screenshot_notifications,
        permission_prompted: config.notification_permission_prompted,
    }
}

#[tauri::command]
pub fn get_screenshot_notification_settings() -> ScreenshotNotificationSettings {
    notification_settings(&crate::config::load_or_create())
}

/// Start the native macOS notification flow after a successful export.
#[cfg(target_os = "macos")]
#[tauri::command]
pub fn notify_macos_screenshot() -> Result<(), String> {
    unsafe extern "C" {
        fn carambola_notify_screenshot() -> i32;
    }
    if !crate::config::load_or_create().screenshot_notifications {
        return Ok(());
    }
    if unsafe { carambola_notify_screenshot() } == 1 {
        Ok(())
    } else {
        Err("macOS notifications are unavailable for this app".to_string())
    }
}

#[cfg(not(target_os = "macos"))]
#[tauri::command]
pub fn notify_macos_screenshot() -> Result<(), String> {
    Err("macOS notifications are unavailable on this platform".to_string())
}

#[tauri::command]
pub fn set_screenshot_notifications(
    enabled: bool,
) -> Result<ScreenshotNotificationSettings, String> {
    let mut config = crate::config::load_or_create();
    config.screenshot_notifications = enabled;
    crate::config::save(&config)?;
    Ok(notification_settings(&config))
}

/// Persists the one-time permission prompt claim so a user who denies system
/// notifications is not asked again on every successful screenshot export.
#[tauri::command]
pub fn claim_notification_permission_prompt() -> Result<bool, String> {
    let mut config = crate::config::load_or_create();
    if config.notification_permission_prompted {
        return Ok(false);
    }
    config.notification_permission_prompted = true;
    crate::config::save(&config)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::parse_rgba_payload;

    fn payload(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(8 + (width * height * 4) as usize);
        bytes.extend_from_slice(&width.to_le_bytes());
        bytes.extend_from_slice(&height.to_le_bytes());
        bytes.extend(std::iter::repeat(200u8).take((width * height * 4) as usize));
        bytes
    }

    #[test]
    fn parses_valid_rgba_payload() {
        let bytes = payload(2, 1);
        let (width, height, rgba) = parse_rgba_payload(&bytes).expect("valid payload");
        assert_eq!((width, height), (2, 1));
        assert_eq!(rgba.len(), 8);
    }

    #[test]
    fn rejects_short_and_mismatched_payloads() {
        assert!(parse_rgba_payload(&[0, 1, 2]).is_err());
        let mut wrong = payload(2, 2);
        wrong.truncate(8 + 4);
        assert!(parse_rgba_payload(&wrong).is_err());
        assert!(parse_rgba_payload(&payload(0, 2)).is_err());
    }
}
