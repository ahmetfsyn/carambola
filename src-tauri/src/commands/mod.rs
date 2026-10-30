use crate::app_state::{AppState, DisplayInfo};
use crate::clipboard::{copy_rgba_to_clipboard, copy_text_to_clipboard};
use crate::ocr::extract_text;
use chrono::Local;
use image::{codecs::png::PngEncoder, ExtendedColorType, ImageEncoder};
use log::info;
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::BufWriter;
use tauri::{AppHandle, Manager, State};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CropRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AnnotationElementDto {
    pub id: String,
    #[serde(rename = "type")]
    pub el_type: String,
}

#[tauri::command]
pub fn get_display_info(state: State<'_, AppState>) -> Result<DisplayInfo, String> {
    if let Some(frame) = state.get_frame() {
        Ok(frame.display_info)
    } else {
        Err("No captured frame available".to_string())
    }
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
pub fn copy_crop(
    crop: CropRect,
    _elements: Vec<AnnotationElementDto>,
    scale_factor: f64,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let frame = state
        .get_frame()
        .ok_or_else(|| "No captured frame available".to_string())?;

    let (crop_rgba, w, h) = extract_cropped_rgba(
        &frame.rgba,
        frame.width,
        frame.height,
        &crop,
        scale_factor,
    )?;

    copy_rgba_to_clipboard(&crop_rgba, w, h)?;
    info!("Successfully copied {}x{} crop to clipboard", w, h);
    Ok(())
}

#[tauri::command]
pub fn save_crop(
    crop: CropRect,
    _elements: Vec<AnnotationElementDto>,
    scale_factor: f64,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let frame = state
        .get_frame()
        .ok_or_else(|| "No captured frame available".to_string())?;

    let (crop_rgba, w, h) = extract_cropped_rgba(
        &frame.rgba,
        frame.width,
        frame.height,
        &crop,
        scale_factor,
    )?;

    let pictures_dir = dirs::picture_dir().unwrap_or_else(|| std::env::current_dir().unwrap());
    let carambola_dir = pictures_dir.join("Carambola");
    std::fs::create_dir_all(&carambola_dir)
        .map_err(|e| format!("Failed to create save directory: {e}"))?;

    let timestamp = Local::now().format("%Y-%m-%d_%H%M%S");
    let file_path = carambola_dir.join(format!("Carambola_{timestamp}.png"));

    let file = File::create(&file_path).map_err(|e| format!("Failed to create file: {e}"))?;
    let ref mut w_file = BufWriter::new(file);

    let encoder = PngEncoder::new(w_file);
    encoder
        .write_image(&crop_rgba, w as u32, h as u32, ExtendedColorType::Rgba8)
        .map_err(|e| format!("Failed to write PNG: {e}"))?;

    let saved_str = file_path.to_string_lossy().to_string();
    info!("Saved screenshot to: {}", saved_str);
    Ok(saved_str)
}

#[tauri::command]
pub fn hide_overlay(app_handle: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    state.set_overlay_visible(false);
    if let Some(window) = app_handle.get_webview_window("overlay-main") {
        let _ = window.hide();
    }
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

    let (crop_rgba, w, h) = extract_cropped_rgba(
        &frame.rgba,
        frame.width,
        frame.height,
        &crop,
        scale_factor,
    )?;

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


