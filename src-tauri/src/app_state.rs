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

#[derive(Clone, Debug)]
pub struct NativeOverlayGeometry {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub scale_factor: f64,
}

impl NativeOverlayGeometry {
    fn same_as(&self, other: &NativeOverlayGeometry) -> bool {
        self.x == other.x
            && self.y == other.y
            && self.width == other.width
            && self.height == other.height
            && (self.scale_factor - other.scale_factor).abs() < 0.01
    }
}

/// Complete description of the display/frame/native-window relationship that
/// was last synchronized. The bitmap dimensions are included separately from
/// the monitor's advertised bounds because capture backends can return pixels
/// in a different unit space (notably on Retina displays).
#[derive(Clone, Debug)]
pub struct OverlayGeometrySignature {
    pub monitor_id: u32,
    pub monitor_name: String,
    pub bounds: DisplayBounds,
    pub scale_factor: f64,
    pub physical_width: f64,
    pub physical_height: f64,
    pub frame_width: u32,
    pub frame_height: u32,
    pub native: NativeOverlayGeometry,
}

impl OverlayGeometrySignature {
    pub fn same_as(&self, other: &OverlayGeometrySignature) -> bool {
        self.monitor_id == other.monitor_id
            && self.monitor_name == other.monitor_name
            && self.bounds.same_as(&other.bounds)
            && (self.scale_factor - other.scale_factor).abs() < 0.01
            && (self.physical_width - other.physical_width).abs() < 0.5
            && (self.physical_height - other.physical_height).abs() < 0.5
            && self.frame_width == other.frame_width
            && self.frame_height == other.frame_height
            && self.native.same_as(&other.native)
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
    pub next_capture_id: u64,
    pub current_capture_id: Option<u64>,
    pub render_ready_capture_id: Option<u64>,
    /// Set by the tray "Settings..." action so the Settings screen is opened
    /// only after the fresh capture has been presented. This keeps settings
    /// navigation state from leaking into a later capture session.
    pub open_settings_on_present: bool,
    /// Full display/frame/window state last synchronized with the overlay.
    pub last_overlay_geometry: Option<OverlayGeometrySignature>,
    /// Set only after the frontend has registered its capture event listener.
    pub overlay_frontend_ready: bool,
    /// A frame captured before the listener was ready must be delivered once it
    /// subscribes; Tauri events are not retained for later listeners.
    pub pending_capture_event: bool,
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
                next_capture_id: 1,
                current_capture_id: None,
                render_ready_capture_id: None,
                open_settings_on_present: false,
                last_overlay_geometry: None,
                overlay_frontend_ready: false,
                pending_capture_event: false,
            })),
        }
    }

    /// Stores a freshly captured frame. Frames are shared behind an `Arc` so
    /// read-only consumers (frame transfer, crop, OCR) never copy the multi-MB
    /// pixel buffers.
    pub fn set_frame(&self, frame: CapturedFrame, capture_id: u64) -> Arc<CapturedFrame> {
        let frame = Arc::new(frame);
        let mut state = self.inner.write();
        state.current_frame = Some(frame.clone());
        state.current_capture_id = Some(capture_id);
        state.render_ready_capture_id = None;
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
    pub fn try_begin_session(&self) -> Option<u64> {
        let mut state = self.inner.write();
        if state.session_active {
            None
        } else {
            state.session_active = true;
            let capture_id = state.next_capture_id;
            state.next_capture_id = state.next_capture_id.wrapping_add(1).max(1);
            state.current_capture_id = Some(capture_id);
            state.render_ready_capture_id = None;
            Some(capture_id)
        }
    }

    /// Releases the session slot. Called only once the overlay has fully closed
    /// and the application is ready for the next capture.
    pub fn end_session(&self) {
        let mut state = self.inner.write();
        state.session_active = false;
        state.pending_capture_event = false;
        state.render_ready_capture_id = None;
    }

    /// Returns true when a capture event must wait for the frontend listener.
    pub fn defer_capture_event_until_frontend_ready(&self) -> bool {
        let mut state = self.inner.write();
        if state.overlay_frontend_ready {
            false
        } else {
            state.pending_capture_event = true;
            true
        }
    }

    /// Marks the listener ready and atomically claims any capture event queued
    /// before the subscription completed.
    pub fn mark_overlay_frontend_ready(&self) -> bool {
        let mut state = self.inner.write();
        state.overlay_frontend_ready = true;
        std::mem::take(&mut state.pending_capture_event)
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

    pub fn current_capture_id(&self) -> Option<u64> {
        self.inner.read().current_capture_id
    }

    pub fn is_active_capture(&self, capture_id: u64) -> bool {
        let state = self.inner.read();
        state.session_active && state.current_capture_id == Some(capture_id)
    }

    pub fn mark_capture_render_ready(&self, capture_id: u64) -> Result<(), String> {
        let mut state = self.inner.write();
        if !state.session_active || state.current_capture_id != Some(capture_id) {
            return Err("Capture was superseded before overlay rendering completed".to_string());
        }
        let frame = state
            .current_frame
            .as_ref()
            .ok_or_else(|| "No captured frame is ready".to_string())?;
        if frame.width == 0 || frame.height == 0 {
            return Err("Captured frame has invalid dimensions".to_string());
        }
        state.render_ready_capture_id = Some(capture_id);
        Ok(())
    }

    pub fn is_capture_render_ready(&self, capture_id: u64) -> bool {
        let state = self.inner.read();
        state.session_active
            && state.current_capture_id == Some(capture_id)
            && state.render_ready_capture_id == Some(capture_id)
    }

    /// Returns `true` when any display, captured-frame, or native-window
    /// geometry differs from the last synchronized capture.
    pub fn overlay_geometry_changed(&self, signature: &OverlayGeometrySignature) -> bool {
        let state = self.inner.read();
        match &state.last_overlay_geometry {
            Some(last) => !last.same_as(signature),
            None => true,
        }
    }

    pub fn set_last_overlay_geometry(&self, signature: OverlayGeometrySignature) {
        self.inner.write().last_overlay_geometry = Some(signature);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_lock_allows_only_one_active_session() {
        let state = AppState::new();
        assert!(!state.is_session_active());

        let first = state
            .try_begin_session()
            .expect("first activation claims lock");
        assert!(first > 0);
        assert!(
            state.try_begin_session().is_none(),
            "second activation is ignored"
        );
        assert!(state.is_session_active());

        // Releasing (after the overlay closes) makes the lock claimable again.
        state.end_session();
        assert!(!state.is_session_active());
        assert!(state.try_begin_session().is_some());
    }

    #[test]
    fn released_lock_survives_repeated_cycles() {
        let state = AppState::new();
        for _ in 0..5 {
            assert!(state.try_begin_session().is_some());
            assert!(state.try_begin_session().is_none());
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

    #[test]
    fn geometry_signature_detects_monitor_dpi_frame_and_native_changes() {
        let signature = OverlayGeometrySignature {
            monitor_id: 4,
            monitor_name: "Display 4".to_string(),
            bounds: DisplayBounds {
                x: -1920.0,
                y: 0.0,
                width: 1920.0,
                height: 1080.0,
            },
            scale_factor: 1.25,
            physical_width: 2400.0,
            physical_height: 1350.0,
            frame_width: 2400,
            frame_height: 1350,
            native: NativeOverlayGeometry {
                x: -2400,
                y: 0,
                width: 2400,
                height: 1350,
                scale_factor: 1.25,
            },
        };
        assert!(signature.same_as(&signature.clone()));

        let mut changed = signature.clone();
        changed.monitor_id += 1;
        assert!(!signature.same_as(&changed));
        let mut changed = signature.clone();
        changed.scale_factor = 1.5;
        assert!(!signature.same_as(&changed));
        let mut changed = signature.clone();
        changed.frame_width += 1;
        assert!(!signature.same_as(&changed));
        let mut changed = signature.clone();
        changed.native.x -= 100;
        assert!(!signature.same_as(&changed));
    }
}
