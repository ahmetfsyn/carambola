use log::info;

pub fn extract_text(rgba: &[u8], width: u32, height: u32) -> Result<String, String> {
    #[cfg(target_os = "windows")]
    {
        use windows::Graphics::Imaging::{BitmapPixelFormat, SoftwareBitmap};
        use windows::Media::Ocr::OcrEngine;
        use windows::Storage::Streams::DataWriter;

        info!("Running Windows Media OCR on {}x{} crop", width, height);

        let engine = OcrEngine::TryCreateFromUserProfileLanguages()
            .map_err(|e| format!("Windows OCR engine init error: {e}"))?;

        // Write RGBA pixel data into a WinRT IBuffer via DataWriter
        let writer = DataWriter::new().map_err(|e| format!("Failed to create DataWriter: {e}"))?;
        writer
            .WriteBytes(rgba)
            .map_err(|e| format!("Failed to write pixel data: {e}"))?;
        let buffer = writer
            .DetachBuffer()
            .map_err(|e| format!("Failed to detach buffer: {e}"))?;

        // Construct WinRT SoftwareBitmap from the pixel buffer
        let bitmap = SoftwareBitmap::CreateCopyFromBuffer(
            &buffer,
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
        info!(
            "macOS OCR extraction requested for {}x{} crop",
            width, height
        );
        let _ = rgba;
        // Apple Vision (VNRecognizeTextRequest) integration is not implemented.
        // Fail explicitly instead of returning placeholder text that could be
        // mistaken for a successful recognition result.
        Err(
            "OCR is not available on macOS in this build. The screenshot itself was captured \
             and can still be copied or saved."
                .to_string(),
        )
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let _ = (rgba, width, height);
        Err(
            "OCR is not supported on this platform. Install a system OCR engine or use a \
             Windows build for text extraction."
                .to_string(),
        )
    }
}
