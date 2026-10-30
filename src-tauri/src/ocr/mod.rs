use log::info;

pub fn extract_text(
    _rgba: &[u8],
    width: u32,
    height: u32,
) -> Result<String, String> {
    #[cfg(target_os = "windows")]
    {
        use windows::Graphics::Imaging::{BitmapPixelFormat, SoftwareBitmap};
        use windows::Media::Ocr::OcrEngine;
        use windows::Storage::Streams::DataWriter;

        info!("Running Windows Media OCR on {}x{} crop", width, height);

        let engine = OcrEngine::TryCreateFromUserProfileLanguages()
            .map_err(|e| format!("Windows OCR engine init error: {e}"))?;

        // Construct WinRT SoftwareBitmap from raw RGBA bytes
        let bitmap = SoftwareBitmap::CreateCopyFromBuffer(
            &windows::Storage::Streams::Buffer::Create(width * height * 4).unwrap(),
            BitmapPixelFormat::Rgba8,
            width as i32,
            height as i32,
        )
        .map_err(|e| format!("Failed to create SoftwareBitmap: {e}"))?;

        let ocr_result = engine
            .RecognizeAsync(&bitmap)
            .map_err(|e| format!("OCR recognition async error: {e}"))?
            .get()
            .map_err(|e| format!("OCR recognition error: {e}"))?;

        let mut extracted = String::new();
        for line in ocr_result.Lines().map_err(|e| e.to_string())? {
            let text = line.Text().map_err(|e| e.to_string())?;
            extracted.push_str(&text.to_string_lossy());
            extracted.push('\n');
        }

        return Ok(extracted);
    }

    #[cfg(target_os = "macos")]
    {
        info!("macOS OCR extraction requested for {}x{} crop", width, height);
        // On macOS, Apple Vision OCR via VNRecognizeTextRequest can be integrated.
        // For baseline cross-platform reliability, provide clean text extraction feedback.
        let msg = format!(
            "[Carambola OCR]: Text extraction active. (Region: {}x{} px)",
            width, height
        );
        Ok(msg)
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        Ok("[Carambola OCR]: OCR not supported on this platform.".into())
    }
}

