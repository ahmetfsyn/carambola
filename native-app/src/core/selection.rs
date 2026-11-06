//! Pointer drag normalization into exact captured-frame pixel bounds.

pub use super::geometry::LogicalRect;
use super::{frame::PixelRect, geometry::ViewportToFrame};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Selection {
    start: (f64, f64),
    current: (f64, f64),
    dragging: bool,
}

impl Default for Selection {
    fn default() -> Self {
        Self {
            start: (0.0, 0.0),
            current: (0.0, 0.0),
            dragging: false,
        }
    }
}

impl Selection {
    pub fn begin(&mut self, x: f64, y: f64, viewport_width: f64, viewport_height: f64) {
        let point = clamp_point(x, y, viewport_width, viewport_height);
        self.start = point;
        self.current = point;
        self.dragging = true;
    }

    pub fn update(&mut self, x: f64, y: f64, viewport_width: f64, viewport_height: f64) {
        if self.dragging {
            self.current = clamp_point(x, y, viewport_width, viewport_height);
        }
    }

    pub fn cancel(&mut self) {
        self.dragging = false;
        self.start = (0.0, 0.0);
        self.current = (0.0, 0.0);
    }

    pub fn is_dragging(&self) -> bool {
        self.dragging
    }

    pub fn preview(
        self,
        transform: ViewportToFrame,
    ) -> Result<Option<(PixelRect, LogicalRect)>, &'static str> {
        normalized_bounds(self.start, self.current, transform)
    }

    /// Return the pixel crop and a logical rectangle aligned to those pixel
    /// edges. Floor/ceil are applied once, after mapping, to avoid DPI drift.
    pub fn finish(
        &mut self,
        transform: ViewportToFrame,
    ) -> Result<Option<(PixelRect, LogicalRect)>, &'static str> {
        self.dragging = false;
        normalized_bounds(self.start, self.current, transform)
    }
}

fn normalized_bounds(
    start: (f64, f64),
    current: (f64, f64),
    transform: ViewportToFrame,
) -> Result<Option<(PixelRect, LogicalRect)>, &'static str> {
    transform.validate(1.0)?;
    let start = transform.to_frame(start.0, start.1)?;
    let end = transform.to_frame(current.0, current.1)?;
    let left = pixel_floor(start.x.min(end.x), transform.frame_width);
    let top = pixel_floor(start.y.min(end.y), transform.frame_height);
    let right = pixel_ceil(start.x.max(end.x), transform.frame_width);
    let bottom = pixel_ceil(start.y.max(end.y), transform.frame_height);
    if left == right || top == bottom {
        return Ok(None);
    }
    let pixels = PixelRect {
        x: left,
        y: top,
        width: right - left,
        height: bottom - top,
    };
    let logical = transform.logical_rect_for_pixels(pixels)?;
    Ok(Some((pixels, logical)))
}

fn stable_pixel_edge(value: f64) -> f64 {
    let nearest = value.round();
    if (value - nearest).abs() <= 1e-6 {
        nearest
    } else {
        value
    }
}

fn pixel_floor(value: f64, limit: u32) -> u32 {
    stable_pixel_edge(value).floor().clamp(0.0, limit as f64) as u32
}

fn pixel_ceil(value: f64, limit: u32) -> u32 {
    stable_pixel_edge(value).ceil().clamp(0.0, limit as f64) as u32
}

fn clamp_point(x: f64, y: f64, width: f64, height: f64) -> (f64, f64) {
    if !x.is_finite() || !y.is_finite() || !width.is_finite() || !height.is_finite() {
        return (0.0, 0.0);
    }
    (x.clamp(0.0, width.max(0.0)), y.clamp(0.0, height.max(0.0)))
}

#[cfg(test)]
mod tests {
    use super::Selection;
    use crate::core::geometry::ViewportToFrame;

    fn transform(scale_factor: f64) -> ViewportToFrame {
        ViewportToFrame {
            viewport_width: 800.0,
            viewport_height: 600.0,
            scale_factor,
            frame_width: (800.0 * scale_factor) as u32,
            frame_height: (600.0 * scale_factor) as u32,
        }
    }

    #[test]
    fn normalizes_drag_in_all_directions_and_clamps_to_edges() {
        let mut selection = Selection::default();
        selection.begin(900.0, 700.0, 800.0, 600.0);
        selection.update(-10.0, -10.0, 800.0, 600.0);
        let (crop, boundary) = selection.finish(transform(1.0)).unwrap().unwrap();
        assert_eq!((crop.x, crop.y, crop.width, crop.height), (0, 0, 800, 600));
        assert_eq!((boundary.x, boundary.y), (0.0, 0.0));
        assert_eq!((boundary.width, boundary.height), (800.0, 600.0));
    }

    #[test]
    fn maps_logical_selection_to_physical_pixels_at_common_dpi_scales() {
        for scale in [1.0, 1.25, 1.5] {
            let mut selection = Selection::default();
            selection.begin(100.0, 80.0, 800.0, 600.0);
            selection.update(300.0, 200.0, 800.0, 600.0);
            let (crop, _) = selection.finish(transform(scale)).unwrap().unwrap();
            assert_eq!(crop.x, (100.0 * scale) as u32);
            assert_eq!(crop.y, (80.0 * scale) as u32);
            assert_eq!(crop.width, (200.0 * scale) as u32);
            assert_eq!(crop.height, (120.0 * scale) as u32);
        }
    }

    #[test]
    fn rejects_clicks_but_preserves_one_pixel_selections() {
        let mut selection = Selection::default();
        selection.begin(20.0, 20.0, 800.0, 600.0);
        assert!(selection.finish(transform(1.0)).unwrap().is_none());

        selection.begin(20.0, 20.0, 800.0, 600.0);
        selection.update(20.2, 20.1, 800.0, 600.0);
        let (crop, _) = selection.finish(transform(1.0)).unwrap().unwrap();
        assert_eq!((crop.width, crop.height), (1, 1));
    }

    #[test]
    fn reverse_diagonal_drag_has_identical_normalized_bounds() {
        let mut forward = Selection::default();
        forward.begin(100.0, 80.0, 800.0, 600.0);
        forward.update(300.0, 200.0, 800.0, 600.0);
        let expected = forward.finish(transform(1.25)).unwrap().unwrap().0;

        let mut reverse = Selection::default();
        reverse.begin(300.0, 200.0, 800.0, 600.0);
        reverse.update(100.0, 80.0, 800.0, 600.0);
        assert_eq!(
            reverse.finish(transform(1.25)).unwrap().unwrap().0,
            expected
        );
    }
}
