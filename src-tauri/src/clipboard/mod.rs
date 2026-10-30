use arboard::{Clipboard, ImageData};
use std::borrow::Cow;

pub fn copy_rgba_to_clipboard(rgba: &[u8], width: usize, height: usize) -> Result<(), String> {
    let mut clipboard = Clipboard::new().map_err(|e| format!("Clipboard error: {e}"))?;

    let img_data = ImageData {
        width,
        height,
        bytes: Cow::Borrowed(rgba),
    };

    clipboard
        .set_image(img_data)
        .map_err(|e| format!("Failed to set clipboard image: {e}"))?;

    Ok(())
}

pub fn copy_text_to_clipboard(text: &str) -> Result<(), String> {
    let mut clipboard = Clipboard::new().map_err(|e| format!("Clipboard error: {e}"))?;
    clipboard
        .set_text(text.to_string())
        .map_err(|e| format!("Failed to set clipboard text: {e}"))?;
    Ok(())
}

