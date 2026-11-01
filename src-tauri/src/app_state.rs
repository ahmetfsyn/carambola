use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DisplayBounds {
    /// Display origin and dimensions in the coordinate units expected by the
    /// native window backend: macOS points; physical pixels elsewhere.
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
    /// True from the moment an activation begins until the overlay has fully
    /// closed. Guarantees that only one screenshot session can exist at a time.
    pub session_active: bool,
    /// Set by the tray "Settings..." action so the Settings screen is opened
    /// only after the fresh capture has been presented. This keeps settings
    /// navigation state from leaking into a later capture session.
    pub open_settings_on_present: bool,
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
                session_active: false,
                open_settings_on_present: false,
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

    /// Attempts to claim the single session slot. Returns `false` when a
    /// screenshot session is already running (visible, fading out, or still
    /// initializing), in which case the new activation must be ignored.
    pub fn try_begin_session(&self) -> bool {
        let mut state = self.inner.write();
        if state.session_active {
            false
        } else {
            state.session_active = true;
            true
        }
    }

    /// Releases the session slot. Called only once the overlay has fully closed
    /// and the application is ready for the next capture.
    pub fn end_session(&self) {
        self.inner.write().session_active = false;
    }

    pub fn is_session_active(&self) -> bool {
        self.inner.read().session_active
    }

    /// Remembers that Settings should open once the overlay is presented.
    pub fn request_settings_on_present(&self) {
        self.inner.write().open_settings_on_present = true;
    }

    /// Returns and clears the pending Settings request.
    pub fn take_settings_on_present(&self) -> bool {
        let mut state = self.inner.write();
        std::mem::take(&mut state.open_settings_on_present)
    }

    /// Drops a pending Settings request when its capture session ends without
    /// being presented, so it cannot leak into a later, unrelated capture.
    pub fn clear_settings_on_present(&self) {
        self.inner.write().open_settings_on_present = false;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_lock_allows_only_one_active_session() {
        let state = AppState::new();
        assert!(!state.is_session_active());

        assert!(
            state.try_begin_session(),
            "first activation must claim the lock"
        );
        assert!(
            !state.try_begin_session(),
            "second activation while active must be ignored"
        );
        assert!(state.is_session_active());

        // Releasing (after the overlay closes) makes the lock claimable again.
        state.end_session();
        assert!(!state.is_session_active());
        assert!(state.try_begin_session());
    }

    #[test]
    fn released_lock_survives_repeated_cycles() {
        let state = AppState::new();
        for _ in 0..5 {
            assert!(state.try_begin_session());
            assert!(!state.try_begin_session());
            state.end_session();
        }
    }

    #[test]
    fn pending_settings_request_is_consumed_once_or_discarded() {
        let state = AppState::new();
        assert!(!state.take_settings_on_present());

        state.request_settings_on_present();
        assert!(state.take_settings_on_present());
        assert!(
            !state.take_settings_on_present(),
            "a request must not reopen Settings in a later session"
        );

        // An aborted session discards its own pending request.
        state.request_settings_on_present();
        state.clear_settings_on_present();
        assert!(!state.take_settings_on_present());
    }
}
