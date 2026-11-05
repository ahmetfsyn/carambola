//! Coordinate contracts for a captured physical-pixel frame and a native UI viewport.

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
}

#[cfg(test)]
mod tests {
    use super::ViewportToFrame;

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
