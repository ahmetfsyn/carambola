use arboard::{Clipboard, ImageData};
use std::borrow::Cow;

pub fn copy_rgba_to_clipboard(rgba: &[u8], width: usize, height: usize) -> Result<(), String> {
    // The Windows clipboard can be transiently locked by another process.
    // Retry briefly instead of failing the whole copy.
    let mut last_error = String::new();
    for attempt in 0..5 {
        if attempt > 0 {
            std::thread::sleep(std::time::Duration::from_millis(60));
        }
        match Clipboard::new() {
            Ok(mut clipboard) => {
                let img_data = ImageData {
                    width,
                    height,
                    bytes: Cow::Borrowed(rgba),
                };
                match clipboard.set_image(img_data) {
                    Ok(()) => return Ok(()),
                    Err(e) => last_error = format!("Failed to set clipboard image: {e}"),
                }
            }
            Err(e) => last_error = format!("Clipboard error: {e}"),
        }
    }
    Err(last_error)
}

pub fn copy_text_to_clipboard(text: &str) -> Result<(), String> {
    let mut clipboard = Clipboard::new().map_err(|e| format!("Clipboard error: {e}"))?;
    clipboard
        .set_text(text.to_string())
        .map_err(|e| format!("Failed to set clipboard text: {e}"))?;
    Ok(())
}

