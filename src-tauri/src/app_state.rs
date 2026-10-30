use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DisplayBounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl DisplayBounds {
    fn same_as(&self, other: &DisplayBounds) -> bool {
        (self.x - other.x).abs() < 0.5
            && (self.y - other.y).abs() < 0.5
            && (self.width - other.width).abs() < 0.5
            && (self.height - other.height).abs() < 0.5
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DisplayInfo {
    pub id: u32,
    pub name: String,
    #[serde(rename = "isPrimary")]
    pub is_primary: bool,
    #[serde(rename = "scaleFactor")]
    pub scale_factor: f64,
    pub bounds: DisplayBounds,
    #[serde(rename = "physicalWidth")]
    pub physical_width: f64,
    #[serde(rename = "physicalHeight")]
    pub physical_height: f64,
}

#[derive(Clone)]
pub struct CapturedFrame {
    pub width: u32,
    pub height: u32,
    pub scale_factor: f64,
    pub rgba: Vec<u8>,
    pub png_bytes: Vec<u8>,
    pub display_info: DisplayInfo,
    /// Wall-clock timestamp (UNIX epoch ms) taken when the activation was
    /// requested. Used for end-to-end latency instrumentation.
    pub triggered_at_ms: u64,
    /// Time spent acquiring the pixels from the OS, in milliseconds.
    pub capture_ms: f64,
    /// Time spent encoding the lossless PNG payload, in milliseconds.
    pub encode_ms: f64,
}

pub struct InnerState {
    pub current_frame: Option<Arc<CapturedFrame>>,
    pub is_overlay_visible: bool,
    /// Bounds of the last geometry applied to the overlay window. Used to skip
    /// redundant native move/resize calls when the target monitor is unchanged.
    pub last_overlay_bounds: Option<DisplayBounds>,
}

#[derive(Clone)]
pub struct AppState {
    pub inner: Arc<RwLock<InnerState>>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(InnerState {
                current_frame: None,
                is_overlay_visible: false,
                last_overlay_bounds: None,
            })),
        }
    }

    /// Stores a freshly captured frame. Frames are shared behind an `Arc` so
    /// read-only consumers (frame transfer, crop, OCR) never copy the multi-MB
    /// pixel buffers.
    pub fn set_frame(&self, frame: CapturedFrame) -> Arc<CapturedFrame> {
        let frame = Arc::new(frame);
        let mut state = self.inner.write();
        state.current_frame = Some(frame.clone());
        frame
    }

    pub fn get_frame(&self) -> Option<Arc<CapturedFrame>> {
        let state = self.inner.read();
        state.current_frame.clone()
    }

    pub fn set_overlay_visible(&self, visible: bool) {
        let mut state = self.inner.write();
        state.is_overlay_visible = visible;
    }

    pub fn is_overlay_visible(&self) -> bool {
        self.inner.read().is_overlay_visible
    }

    /// Returns `true` when the given bounds differ from the last applied window
    /// geometry (or when no geometry has been applied yet).
    pub fn overlay_geometry_changed(&self, bounds: &DisplayBounds) -> bool {
        let state = self.inner.read();
        match &state.last_overlay_bounds {
            Some(last) => !last.same_as(bounds),
            None => true,
        }
    }

    pub fn set_last_overlay_bounds(&self, bounds: DisplayBounds) {
        self.inner.write().last_overlay_bounds = Some(bounds);
    }
}
