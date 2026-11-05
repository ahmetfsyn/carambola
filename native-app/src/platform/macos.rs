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
