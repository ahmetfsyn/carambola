//! Coordinate contracts for a captured physical-pixel frame and a native UI viewport.

use super::frame::PixelRect;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LogicalViewport {
    pub width: f64,
    pub height: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LogicalRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// The captured frame remains in physical pixels; this value tracks only the
/// Overlay's current effective scale and derives logical dimensions from that
/// immutable frame on every transition.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrameViewport {
    frame_width: u32,
    frame_height: u32,
    scale_factor: f64,
}

impl FrameViewport {
    pub fn new(
        frame_width: u32,
        frame_height: u32,
        scale_factor: f64,
    ) -> Result<Self, &'static str> {
        if frame_width == 0 || frame_height == 0 {
            return Err("captured frame dimensions must be positive");
        }
        validate_scale_factor(scale_factor)?;
        Ok(Self {
            frame_width,
            frame_height,
            scale_factor,
        })
    }

    pub fn frame_width(self) -> u32 {
        self.frame_width
    }

    pub fn frame_height(self) -> u32 {
        self.frame_height
    }

    pub fn scale_factor(self) -> f64 {
        self.scale_factor
    }

    pub fn logical_size(self) -> LogicalViewport {
        LogicalViewport {
            width: f64::from(self.frame_width) / self.scale_factor,
            height: f64::from(self.frame_height) / self.scale_factor,
        }
    }

    /// Synchronize from the captured physical dimensions, never from the
    /// previous logical dimensions, so round trips cannot accumulate drift.
    pub fn synchronize_scale_factor(
        &mut self,
        scale_factor: f64,
    ) -> Result<LogicalViewport, &'static str> {
        validate_scale_factor(scale_factor)?;
        self.scale_factor = scale_factor;
        Ok(self.logical_size())
    }

    /// Build a transform using the dimensions Slint reports for the current
    /// input event. Reject callbacks from a viewport that has not caught up to
    /// the new scale factor yet.
    pub fn transform_for(
        self,
        viewport_width: f64,
        viewport_height: f64,
    ) -> Result<ViewportToFrame, &'static str> {
        let transform = ViewportToFrame {
            viewport_width,
            viewport_height,
            scale_factor: self.scale_factor,
            frame_width: self.frame_width,
            frame_height: self.frame_height,
        };
        transform.validate(1.0)?;
        Ok(transform)
    }

    pub fn project_pixels(self, rect: PixelRect) -> Result<LogicalRect, &'static str> {
        let viewport = self.logical_size();
        ViewportToFrame {
            viewport_width: viewport.width,
            viewport_height: viewport.height,
            scale_factor: self.scale_factor,
            frame_width: self.frame_width,
            frame_height: self.frame_height,
        }
        .logical_rect_for_pixels(rect)
    }
}

fn validate_scale_factor(scale_factor: f64) -> Result<(), &'static str> {
    if !scale_factor.is_finite() || scale_factor <= 0.0 {
        return Err("scale factor must be finite and positive");
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewportToFrame {
    pub viewport_width: f64,
    pub viewport_height: f64,
    pub scale_factor: f64,
    pub frame_width: u32,
    pub frame_height: u32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FramePoint {
    pub x: f64,
    pub y: f64,
}

impl ViewportToFrame {
    /// Reject stale geometry instead of stretching the screenshot into a mismatched viewport.
    pub fn validate(self, tolerance_px: f64) -> Result<(), &'static str> {
        if !self.viewport_width.is_finite()
            || !self.viewport_height.is_finite()
            || !self.scale_factor.is_finite()
            || self.viewport_width <= 0.0
            || self.viewport_height <= 0.0
            || self.scale_factor <= 0.0
            || tolerance_px < 0.0
            || !tolerance_px.is_finite()
            || self.frame_width == 0
            || self.frame_height == 0
        {
            return Err("viewport geometry must be finite and positive");
        }
        let width_error = (self.viewport_width * self.scale_factor - self.frame_width as f64).abs();
        let height_error =
            (self.viewport_height * self.scale_factor - self.frame_height as f64).abs();
        if width_error > tolerance_px || height_error > tolerance_px {
            return Err("viewport and captured frame geometry are out of sync");
        }
        Ok(())
    }

    /// Maps local logical window coordinates to physical screenshot pixels.
    /// Separate axis ratios preserve exact frame bounds after fractional-DPI rounding.
    pub fn to_frame(self, x: f64, y: f64) -> Result<FramePoint, &'static str> {
        self.validate(1.0)?;
        if !x.is_finite() || !y.is_finite() {
            return Err("pointer coordinates must be finite");
        }
        Ok(FramePoint {
            x: x * self.frame_width as f64 / self.viewport_width,
            y: y * self.frame_height as f64 / self.viewport_height,
        })
    }

    pub fn logical_rect_for_pixels(self, rect: PixelRect) -> Result<LogicalRect, &'static str> {
        self.validate(1.0)?;
        let right = rect
            .x
            .checked_add(rect.width)
            .ok_or("selection bounds overflow")?;
        let bottom = rect
            .y
            .checked_add(rect.height)
            .ok_or("selection bounds overflow")?;
        if rect.width == 0
            || rect.height == 0
            || right > self.frame_width
            || bottom > self.frame_height
        {
            return Err("selection rectangle is empty or outside the captured frame");
        }
        Ok(LogicalRect {
            x: f64::from(rect.x) * self.viewport_width / f64::from(self.frame_width),
            y: f64::from(rect.y) * self.viewport_height / f64::from(self.frame_height),
            width: f64::from(rect.width) * self.viewport_width / f64::from(self.frame_width),
            height: f64::from(rect.height) * self.viewport_height / f64::from(self.frame_height),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{FrameViewport, ViewportToFrame};

    #[test]
    fn calculates_logical_viewports_at_common_dpi_scales() {
        let frame = FrameViewport::new(2400, 1500, 1.0).unwrap();
        assert_eq!(frame.logical_size().width, 2400.0);
        assert_eq!(frame.logical_size().height, 1500.0);

        let mut frame = frame;
        assert_eq!(frame.synchronize_scale_factor(1.25).unwrap().width, 1920.0);
        assert_eq!(frame.logical_size().height, 1200.0);
        assert_eq!(frame.synchronize_scale_factor(1.5).unwrap().width, 1600.0);
        assert_eq!(frame.logical_size().height, 1000.0);
    }

    #[test]
    fn scale_transitions_are_recomputed_from_physical_dimensions_without_drift() {
        let mut frame = FrameViewport::new(2560, 1440, 1.0).unwrap();
        for factor in [1.5, 1.0, 1.25, 1.5, 1.25, 1.0] {
            let viewport = frame.synchronize_scale_factor(factor).unwrap();
            assert_eq!(viewport.width, 2560.0 / factor);
            assert_eq!(viewport.height, 1440.0 / factor);
        }
        assert_eq!(frame.scale_factor(), 1.0);
        assert_eq!(frame.logical_size().width, 2560.0);
        assert_eq!(frame.logical_size().height, 1440.0);
    }

    #[test]
    fn rejects_invalid_scale_and_stale_callback_geometry() {
        let mut frame = FrameViewport::new(2400, 1350, 1.0).unwrap();
        frame.synchronize_scale_factor(1.5).unwrap();
        assert!(frame.transform_for(2400.0, 1350.0).is_err());
        assert!(frame.synchronize_scale_factor(f64::NAN).is_err());
        assert_eq!(frame.scale_factor(), 1.5);
    }

    #[test]
    fn maps_fractional_dpi_viewport_to_physical_frame_without_axis_drift() {
        let transform = ViewportToFrame {
            viewport_width: 1536.0,
            viewport_height: 864.0,
            scale_factor: 1.25,
            frame_width: 1920,
            frame_height: 1080,
        };
        assert!(transform.validate(0.5).is_ok());
        assert_eq!(
            transform.to_frame(1536.0, 864.0).unwrap(),
            super::FramePoint {
                x: 1920.0,
                y: 1080.0
            }
        );
        assert_eq!(
            transform.to_frame(768.0, 432.0).unwrap(),
            super::FramePoint { x: 960.0, y: 540.0 }
        );
    }

    #[test]
    fn rejects_stale_dpi_or_viewport_before_overlay_presentation() {
        let stale = ViewportToFrame {
            viewport_width: 1920.0,
            viewport_height: 1080.0,
            scale_factor: 1.0,
            frame_width: 2400,
            frame_height: 1350,
        };
        assert!(stale.validate(1.0).is_err());
        assert!(stale.to_frame(10.0, 10.0).is_err());
    }
}
