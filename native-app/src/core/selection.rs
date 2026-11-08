//! Pointer drag normalization into exact captured-frame pixel bounds.

pub use super::geometry::LogicalRect;
use super::{
    frame::PixelRect,
    geometry::{FramePoint, ViewportToFrame},
};

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
        if !self.dragging {
            return Ok(None);
        }
        self.dragging = false;
        normalized_bounds(self.start, self.current, transform)
    }
}

/// The eight borders that can resize a committed pixel rectangle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResizeHandle {
    TopLeft,
    Top,
    TopRight,
    Right,
    BottomRight,
    Bottom,
    BottomLeft,
    Left,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectionHit {
    Outside,
    Move,
    Resize(ResizeHandle),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GestureKind {
    Create,
    Move,
    Resize(ResizeHandle),
}

/// Hit-test a committed frame-pixel rectangle using a logical-pixel radius.
/// Corners win over edges, and edges win over the interior.
pub fn hit_test_selection(
    rect: PixelRect,
    x: f64,
    y: f64,
    radius: f64,
    transform: ViewportToFrame,
) -> Result<SelectionHit, &'static str> {
    if !x.is_finite() || !y.is_finite() || !radius.is_finite() || radius < 0.0 {
        return Err("hit-test coordinates and radius must be finite and nonnegative");
    }
    let logical = transform.logical_rect_for_pixels(rect)?;
    let left = logical.x;
    let top = logical.y;
    let right = left + logical.width;
    let bottom = top + logical.height;
    let near_x = x >= left - radius && x <= right + radius;
    let near_y = y >= top - radius && y <= bottom + radius;
    if !near_x || !near_y {
        return Ok(SelectionHit::Outside);
    }

    let horizontal = nearest_edge(x, left, right, radius);
    let vertical = nearest_edge(y, top, bottom, radius);
    Ok(match (horizontal, vertical) {
        (Some(Edge::Start), Some(Edge::Start)) => SelectionHit::Resize(ResizeHandle::TopLeft),
        (None, Some(Edge::Start)) => SelectionHit::Resize(ResizeHandle::Top),
        (Some(Edge::End), Some(Edge::Start)) => SelectionHit::Resize(ResizeHandle::TopRight),
        (Some(Edge::End), None) => SelectionHit::Resize(ResizeHandle::Right),
        (Some(Edge::End), Some(Edge::End)) => SelectionHit::Resize(ResizeHandle::BottomRight),
        (None, Some(Edge::End)) => SelectionHit::Resize(ResizeHandle::Bottom),
        (Some(Edge::Start), Some(Edge::End)) => SelectionHit::Resize(ResizeHandle::BottomLeft),
        (Some(Edge::Start), None) => SelectionHit::Resize(ResizeHandle::Left),
        (None, None) if x >= left && x <= right && y >= top && y <= bottom => SelectionHit::Move,
        (None, None) => SelectionHit::Outside,
    })
}

#[derive(Clone, Copy)]
enum Edge {
    Start,
    End,
}

fn nearest_edge(value: f64, start: f64, end: f64, radius: f64) -> Option<Edge> {
    let start_distance = (value - start).abs();
    let end_distance = (value - end).abs();
    if start_distance <= radius && start_distance <= end_distance {
        Some(Edge::Start)
    } else if end_distance <= radius {
        Some(Edge::End)
    } else {
        None
    }
}

/// A committed physical-pixel rectangle plus at most one uncommitted gesture.
/// A new Create gesture may replace a rectangle; cancel restores that rectangle.
#[derive(Default)]
pub struct SelectionGeometry {
    committed: Option<PixelRect>,
    active: Option<ActiveGesture>,
}

struct ActiveGesture {
    initial: Option<PixelRect>,
    preview: Option<PixelRect>,
    start: FramePoint,
    transform: ViewportToFrame,
    kind: GestureKind,
}

impl SelectionGeometry {
    pub fn new(
        committed: Option<PixelRect>,
        transform: ViewportToFrame,
    ) -> Result<Self, &'static str> {
        transform.validate(1.0)?;
        if let Some(rect) = committed {
            transform.logical_rect_for_pixels(rect)?;
        }
        Ok(Self {
            committed,
            active: None,
        })
    }

    pub fn committed(&self) -> Option<PixelRect> {
        self.committed
    }

    pub fn preview(&self) -> Option<PixelRect> {
        self.active
            .as_ref()
            .map_or(self.committed, |gesture| gesture.preview)
    }

    pub fn is_active(&self) -> bool {
        self.active.is_some()
    }

    pub fn begin(
        &mut self,
        kind: GestureKind,
        x: f64,
        y: f64,
        transform: ViewportToFrame,
    ) -> Result<(), &'static str> {
        if self.active.is_some() {
            return Err("a selection gesture is already active");
        }
        transform.validate(1.0)?;
        if let Some(rect) = self.committed {
            transform.logical_rect_for_pixels(rect)?;
        } else if kind != GestureKind::Create {
            return Err("moving or resizing requires a committed selection");
        }
        let start = finite_frame_point(transform.to_frame(x, y)?)?;
        self.active = Some(ActiveGesture {
            initial: self.committed,
            preview: if kind == GestureKind::Create {
                None
            } else {
                self.committed
            },
            start,
            transform,
            kind,
        });
        Ok(())
    }

    pub fn update(
        &mut self,
        x: f64,
        y: f64,
        transform: ViewportToFrame,
    ) -> Result<Option<PixelRect>, &'static str> {
        let Some(gesture) = self.active.as_mut() else {
            return Ok(self.committed);
        };
        if transform != gesture.transform {
            return Err("viewport changed during selection gesture");
        }
        let point = finite_frame_point(transform.to_frame(x, y)?)?;
        gesture.preview = match gesture.kind {
            GestureKind::Create => normalized_pixels(
                gesture.start,
                point,
                transform.frame_width,
                transform.frame_height,
            ),
            GestureKind::Move => Some(move_rect(
                gesture.initial.expect("move requires initial rectangle"),
                gesture.start,
                point,
                transform,
            )?),
            GestureKind::Resize(handle) => Some(resize_rect(
                gesture.initial.expect("resize requires initial rectangle"),
                handle,
                gesture.start,
                point,
                transform,
            )?),
        };
        Ok(gesture.preview)
    }

    pub fn commit(&mut self) -> Option<PixelRect> {
        if let Some(gesture) = self.active.take() {
            self.committed = gesture.preview.or(gesture.initial);
        }
        self.committed
    }

    pub fn cancel(&mut self) -> Option<PixelRect> {
        if let Some(gesture) = self.active.take() {
            self.committed = gesture.initial;
        }
        self.committed
    }
}

fn finite_frame_point(point: FramePoint) -> Result<FramePoint, &'static str> {
    if point.x.is_finite() && point.y.is_finite() {
        Ok(point)
    } else {
        Err("mapped pointer coordinates must be finite")
    }
}

fn move_rect(
    original: PixelRect,
    start: FramePoint,
    current: FramePoint,
    transform: ViewportToFrame,
) -> Result<PixelRect, &'static str> {
    let dx = current.x - start.x;
    let dy = current.y - start.y;
    if !dx.is_finite() || !dy.is_finite() {
        return Err("pointer movement must be finite");
    }
    Ok(PixelRect {
        x: (f64::from(original.x) + dx.round())
            .clamp(0.0, f64::from(transform.frame_width - original.width)) as u32,
        y: (f64::from(original.y) + dy.round())
            .clamp(0.0, f64::from(transform.frame_height - original.height)) as u32,
        ..original
    })
}

fn resize_rect(
    original: PixelRect,
    handle: ResizeHandle,
    start: FramePoint,
    current: FramePoint,
    transform: ViewportToFrame,
) -> Result<PixelRect, &'static str> {
    let dx = current.x - start.x;
    let dy = current.y - start.y;
    if !dx.is_finite() || !dy.is_finite() {
        return Err("pointer movement must be finite");
    }
    let mut left = f64::from(original.x);
    let mut top = f64::from(original.y);
    let mut right = left + f64::from(original.width);
    let mut bottom = top + f64::from(original.height);
    let moves_left = matches!(
        handle,
        ResizeHandle::TopLeft | ResizeHandle::Left | ResizeHandle::BottomLeft
    );
    let moves_right = matches!(
        handle,
        ResizeHandle::TopRight | ResizeHandle::Right | ResizeHandle::BottomRight
    );
    let moves_top = matches!(
        handle,
        ResizeHandle::TopLeft | ResizeHandle::Top | ResizeHandle::TopRight
    );
    let moves_bottom = matches!(
        handle,
        ResizeHandle::BottomLeft | ResizeHandle::Bottom | ResizeHandle::BottomRight
    );
    if moves_left {
        left = (left + dx.round()).clamp(0.0, right - 1.0);
    }
    if moves_right {
        right = (right + dx.round()).clamp(left + 1.0, f64::from(transform.frame_width));
    }
    if moves_top {
        top = (top + dy.round()).clamp(0.0, bottom - 1.0);
    }
    if moves_bottom {
        bottom = (bottom + dy.round()).clamp(top + 1.0, f64::from(transform.frame_height));
    }
    let x = left as u32;
    let y = top as u32;
    Ok(PixelRect {
        x,
        y,
        width: right as u32 - x,
        height: bottom as u32 - y,
    })
}

fn normalized_bounds(
    start: (f64, f64),
    current: (f64, f64),
    transform: ViewportToFrame,
) -> Result<Option<(PixelRect, LogicalRect)>, &'static str> {
    transform.validate(1.0)?;
    let start = transform.to_frame(start.0, start.1)?;
    let end = transform.to_frame(current.0, current.1)?;
    let Some(pixels) = normalized_pixels(start, end, transform.frame_width, transform.frame_height)
    else {
        return Ok(None);
    };
    let logical = transform.logical_rect_for_pixels(pixels)?;
    Ok(Some((pixels, logical)))
}

fn normalized_pixels(
    start: FramePoint,
    end: FramePoint,
    frame_width: u32,
    frame_height: u32,
) -> Option<PixelRect> {
    let left = pixel_floor(start.x.min(end.x), frame_width);
    let top = pixel_floor(start.y.min(end.y), frame_height);
    let right = pixel_ceil(start.x.max(end.x), frame_width);
    let bottom = pixel_ceil(start.y.max(end.y), frame_height);
    if left == right || top == bottom {
        return None;
    }
    Some(PixelRect {
        x: left,
        y: top,
        width: right - left,
        height: bottom - top,
    })
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
    use super::{
        hit_test_selection, GestureKind, ResizeHandle, Selection, SelectionGeometry, SelectionHit,
    };
    use crate::core::frame::PixelRect;
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

    fn small_transform() -> ViewportToFrame {
        ViewportToFrame {
            viewport_width: 100.0,
            viewport_height: 80.0,
            scale_factor: 1.0,
            frame_width: 100,
            frame_height: 80,
        }
    }

    fn initial_rect() -> PixelRect {
        PixelRect {
            x: 20,
            y: 30,
            width: 40,
            height: 20,
        }
    }

    fn started_gesture(kind: GestureKind, x: f64, y: f64) -> SelectionGeometry {
        let transform = small_transform();
        let mut geometry = SelectionGeometry::new(Some(initial_rect()), transform).unwrap();
        geometry.begin(kind, x, y, transform).unwrap();
        geometry
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

    #[test]
    fn hit_testing_finds_all_handles_with_corner_priority() {
        let transform = small_transform();
        let cases = [
            (21.0, 31.0, ResizeHandle::TopLeft),
            (40.0, 31.0, ResizeHandle::Top),
            (59.0, 31.0, ResizeHandle::TopRight),
            (59.0, 40.0, ResizeHandle::Right),
            (59.0, 49.0, ResizeHandle::BottomRight),
            (40.0, 49.0, ResizeHandle::Bottom),
            (21.0, 49.0, ResizeHandle::BottomLeft),
            (21.0, 40.0, ResizeHandle::Left),
        ];
        for (x, y, handle) in cases {
            assert_eq!(
                hit_test_selection(initial_rect(), x, y, 5.0, transform).unwrap(),
                SelectionHit::Resize(handle)
            );
        }
        assert_eq!(
            hit_test_selection(initial_rect(), 40.0, 40.0, 5.0, transform).unwrap(),
            SelectionHit::Move
        );
        assert_eq!(
            hit_test_selection(initial_rect(), 5.0, 40.0, 5.0, transform).unwrap(),
            SelectionHit::Outside
        );
    }

    #[test]
    fn hit_radius_stays_in_logical_pixels_across_dpi_scales() {
        let normal = small_transform();
        let scaled = ViewportToFrame {
            viewport_width: 100.0,
            viewport_height: 80.0,
            scale_factor: 1.25,
            frame_width: 125,
            frame_height: 100,
        };
        let scaled_rect = PixelRect {
            x: 25,
            y: 25,
            width: 50,
            height: 25,
        };
        let normal_rect = PixelRect {
            x: 20,
            y: 20,
            width: 40,
            height: 20,
        };
        for (rect, transform) in [(normal_rect, normal), (scaled_rect, scaled)] {
            assert_eq!(
                hit_test_selection(rect, 23.0, 23.0, 4.0, transform).unwrap(),
                SelectionHit::Resize(ResizeHandle::TopLeft)
            );
            assert_eq!(
                hit_test_selection(rect, 28.0, 28.0, 4.0, transform).unwrap(),
                SelectionHit::Move
            );
        }
    }

    #[test]
    fn each_resize_handle_moves_only_its_own_edges() {
        let transform = small_transform();
        let cases = [
            (
                ResizeHandle::TopLeft,
                (20.0, 30.0),
                PixelRect {
                    x: 25,
                    y: 34,
                    width: 35,
                    height: 16,
                },
            ),
            (
                ResizeHandle::Top,
                (40.0, 30.0),
                PixelRect {
                    x: 20,
                    y: 34,
                    width: 40,
                    height: 16,
                },
            ),
            (
                ResizeHandle::TopRight,
                (60.0, 30.0),
                PixelRect {
                    x: 20,
                    y: 34,
                    width: 45,
                    height: 16,
                },
            ),
            (
                ResizeHandle::Right,
                (60.0, 40.0),
                PixelRect {
                    x: 20,
                    y: 30,
                    width: 45,
                    height: 20,
                },
            ),
            (
                ResizeHandle::BottomRight,
                (60.0, 50.0),
                PixelRect {
                    x: 20,
                    y: 30,
                    width: 45,
                    height: 24,
                },
            ),
            (
                ResizeHandle::Bottom,
                (40.0, 50.0),
                PixelRect {
                    x: 20,
                    y: 30,
                    width: 40,
                    height: 24,
                },
            ),
            (
                ResizeHandle::BottomLeft,
                (20.0, 50.0),
                PixelRect {
                    x: 25,
                    y: 30,
                    width: 35,
                    height: 24,
                },
            ),
            (
                ResizeHandle::Left,
                (20.0, 40.0),
                PixelRect {
                    x: 25,
                    y: 30,
                    width: 35,
                    height: 20,
                },
            ),
        ];
        for (handle, start, expected) in cases {
            let mut geometry = started_gesture(GestureKind::Resize(handle), start.0, start.1);
            assert_eq!(
                geometry
                    .update(start.0 + 5.0, start.1 + 4.0, transform)
                    .unwrap(),
                Some(expected)
            );
            assert_eq!(geometry.committed(), Some(initial_rect()));
            assert_eq!(geometry.commit(), Some(expected));
        }
    }

    #[test]
    fn moving_uses_initial_pointer_and_clamps_at_every_frame_edge() {
        let transform = small_transform();
        let cases = [
            ((-500.0, 40.0), (0, 30)),
            ((500.0, 40.0), (60, 30)),
            ((30.0, -500.0), (20, 0)),
            ((30.0, 500.0), (20, 60)),
        ];
        for (pointer, (x, y)) in cases {
            let mut geometry = started_gesture(GestureKind::Move, 30.0, 40.0);
            let preview = geometry
                .update(pointer.0, pointer.1, transform)
                .unwrap()
                .unwrap();
            assert_eq!(
                (preview.x, preview.y, preview.width, preview.height),
                (x, y, 40, 20)
            );
        }
        let mut geometry = started_gesture(GestureKind::Move, 30.0, 40.0);
        geometry.update(35.0, 47.0, transform).unwrap();
        assert_eq!(
            geometry.update(33.0, 42.0, transform).unwrap(),
            Some(PixelRect {
                x: 23,
                y: 32,
                width: 40,
                height: 20
            })
        );
    }

    #[test]
    fn resizing_clamps_to_every_frame_edge_and_keeps_the_opposite_edge() {
        let transform = small_transform();
        let cases = [
            (
                ResizeHandle::Left,
                (20.0, 40.0),
                (-500.0, 40.0),
                PixelRect {
                    x: 0,
                    y: 30,
                    width: 60,
                    height: 20,
                },
            ),
            (
                ResizeHandle::Right,
                (60.0, 40.0),
                (500.0, 40.0),
                PixelRect {
                    x: 20,
                    y: 30,
                    width: 80,
                    height: 20,
                },
            ),
            (
                ResizeHandle::Top,
                (40.0, 30.0),
                (40.0, -500.0),
                PixelRect {
                    x: 20,
                    y: 0,
                    width: 40,
                    height: 50,
                },
            ),
            (
                ResizeHandle::Bottom,
                (40.0, 50.0),
                (40.0, 500.0),
                PixelRect {
                    x: 20,
                    y: 30,
                    width: 40,
                    height: 50,
                },
            ),
            (
                ResizeHandle::TopLeft,
                (20.0, 30.0),
                (-500.0, -500.0),
                PixelRect {
                    x: 0,
                    y: 0,
                    width: 60,
                    height: 50,
                },
            ),
            (
                ResizeHandle::BottomRight,
                (60.0, 50.0),
                (500.0, 500.0),
                PixelRect {
                    x: 20,
                    y: 30,
                    width: 80,
                    height: 50,
                },
            ),
        ];
        for (handle, start, pointer, expected) in cases {
            let mut geometry = started_gesture(GestureKind::Resize(handle), start.0, start.1);
            assert_eq!(
                geometry.update(pointer.0, pointer.1, transform).unwrap(),
                Some(expected)
            );
        }
    }

    #[test]
    fn reverse_resize_stops_at_one_pixel_without_flipping_the_handle() {
        let transform = small_transform();
        let mut geometry = started_gesture(GestureKind::Resize(ResizeHandle::TopLeft), 20.0, 30.0);
        assert_eq!(
            geometry.update(500.0, 500.0, transform).unwrap(),
            Some(PixelRect {
                x: 59,
                y: 49,
                width: 1,
                height: 1
            })
        );
        assert_eq!(
            geometry.update(-500.0, -500.0, transform).unwrap(),
            Some(PixelRect {
                x: 0,
                y: 0,
                width: 60,
                height: 50
            })
        );
        let mut geometry =
            started_gesture(GestureKind::Resize(ResizeHandle::BottomRight), 60.0, 50.0);
        assert_eq!(
            geometry.update(-500.0, -500.0, transform).unwrap(),
            Some(PixelRect {
                x: 20,
                y: 30,
                width: 1,
                height: 1
            })
        );
    }

    #[test]
    fn create_can_replace_a_selection_and_cancel_restores_the_original() {
        let transform = small_transform();
        let mut geometry = SelectionGeometry::new(Some(initial_rect()), transform).unwrap();
        geometry
            .begin(GestureKind::Create, 10.0, 10.0, transform)
            .unwrap();
        assert_eq!(geometry.preview(), None);
        assert_eq!(
            geometry.update(11.0, 11.0, transform).unwrap(),
            Some(PixelRect {
                x: 10,
                y: 10,
                width: 1,
                height: 1
            })
        );
        assert_eq!(geometry.committed(), Some(initial_rect()));
        assert_eq!(geometry.cancel(), Some(initial_rect()));
        assert!(!geometry.is_active());
        assert_eq!(geometry.preview(), Some(initial_rect()));
        geometry
            .begin(GestureKind::Create, 10.0, 10.0, transform)
            .unwrap();
        assert_eq!(geometry.commit(), Some(initial_rect()));
    }

    #[test]
    fn commit_keeps_updated_geometry_and_idle_pointer_up_is_harmless() {
        let transform = small_transform();
        let mut geometry = started_gesture(GestureKind::Move, 30.0, 40.0);
        let expected = PixelRect {
            x: 25,
            y: 36,
            width: 40,
            height: 20,
        };
        geometry.update(35.0, 46.0, transform).unwrap();
        assert_eq!(geometry.commit(), Some(expected));
        assert_eq!(geometry.commit(), Some(expected));
        assert_eq!(geometry.cancel(), Some(expected));
        assert_eq!(
            geometry.update(99.0, 79.0, transform).unwrap(),
            Some(expected)
        );

        let mut old_selection = Selection::default();
        old_selection.begin(20.0, 20.0, 100.0, 80.0);
        old_selection.update(40.0, 40.0, 100.0, 80.0);
        assert!(old_selection.finish(transform).unwrap().is_some());
        assert!(old_selection.finish(transform).unwrap().is_none());
    }

    #[test]
    fn invalid_rectangles_transforms_and_pointer_values_are_rejected() {
        let transform = small_transform();
        let empty = PixelRect {
            x: 20,
            y: 20,
            width: 0,
            height: 1,
        };
        let outside = PixelRect {
            x: 99,
            y: 20,
            width: 2,
            height: 1,
        };
        assert!(SelectionGeometry::new(Some(empty), transform).is_err());
        assert!(SelectionGeometry::new(Some(outside), transform).is_err());
        assert!(hit_test_selection(empty, 20.0, 20.0, 5.0, transform).is_err());
        assert!(hit_test_selection(initial_rect(), f64::NAN, 30.0, 5.0, transform).is_err());
        assert!(hit_test_selection(initial_rect(), 20.0, 30.0, -1.0, transform).is_err());
        assert!(hit_test_selection(initial_rect(), 20.0, 30.0, f64::INFINITY, transform).is_err());
        let invalid_transform = ViewportToFrame {
            viewport_width: 0.0,
            ..transform
        };
        assert!(SelectionGeometry::new(None, invalid_transform).is_err());
        let mut geometry = SelectionGeometry::new(None, transform).unwrap();
        assert!(geometry
            .begin(GestureKind::Move, 0.0, 0.0, transform)
            .is_err());
        assert!(geometry
            .begin(GestureKind::Create, f64::NAN, 0.0, transform)
            .is_err());
        assert!(geometry
            .begin(GestureKind::Create, f64::MAX, 0.0, transform)
            .is_err());
        geometry
            .begin(GestureKind::Create, 10.0, 10.0, transform)
            .unwrap();
        assert!(geometry
            .begin(GestureKind::Create, 11.0, 11.0, transform)
            .is_err());
        assert!(geometry.update(f64::INFINITY, 10.0, transform).is_err());
        assert!(geometry.update(11.0, 11.0, invalid_transform).is_err());
        assert_eq!(geometry.committed(), None);
        assert_eq!(geometry.preview(), None);
        assert_eq!(geometry.cancel(), None);
    }
}
