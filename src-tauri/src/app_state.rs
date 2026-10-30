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
}

pub struct InnerState {
    pub current_frame: Option<CapturedFrame>,
    pub is_overlay_visible: bool,
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
            })),
        }
    }

    pub fn set_frame(&self, frame: CapturedFrame) {
        let mut state = self.inner.write();
        state.current_frame = Some(frame);
    }

    pub fn get_frame(&self) -> Option<CapturedFrame> {
        let state = self.inner.read();
        state.current_frame.clone()
    }

    pub fn set_overlay_visible(&self, visible: bool) {
        let mut state = self.inner.write();
        state.is_overlay_visible = visible;
    }
}

