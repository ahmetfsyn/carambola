//! Framework-independent annotation model in selected-image physical pixels.
//!
//! Points use image-edge coordinates: `(0, 0)` is the selected crop's top-left
//! edge and `(width, height)` is its bottom-right edge. Viewport input is
//! converted once through `ViewportToImage`; it is never stored in UI units.

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ImagePoint {
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ImageBounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl ImageBounds {
    pub fn from_points(a: ImagePoint, b: ImagePoint) -> Self {
        let left = a.x.min(b.x);
        let top = a.y.min(b.y);
        Self {
            x: left,
            y: top,
            width: a.x.max(b.x) - left,
            height: a.y.max(b.y) - top,
        }
    }

    pub fn is_valid_for(self, width: u32, height: u32) -> bool {
        self.x.is_finite()
            && self.y.is_finite()
            && self.width.is_finite()
            && self.height.is_finite()
            && self.width > 0.0
            && self.height > 0.0
            && self.x >= 0.0
            && self.y >= 0.0
            && self.x + self.width <= width as f64
            && self.y + self.height <= height as f64
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StrokeColor {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl StrokeColor {
    pub const RED: Self = Self {
        r: 255,
        g: 55,
        b: 80,
        a: 255,
    };
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StrokeStyle {
    /// Width in selected-image physical pixels.
    pub width: f64,
    pub color: StrokeColor,
}

impl Default for StrokeStyle {
    fn default() -> Self {
        Self {
            width: 3.0,
            color: StrokeColor::RED,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnnotationTool {
    Pen,
    Arrow,
    Rectangle,
    Triangle,
    Ellipse,
}

impl AnnotationTool {
    pub fn from_ui_index(index: i32) -> Option<Self> {
        match index {
            0 => Some(Self::Pen),
            1 => Some(Self::Arrow),
            2 => Some(Self::Rectangle),
            3 => Some(Self::Triangle),
            4 => Some(Self::Ellipse),
            _ => None,
        }
    }

    pub fn ui_index(self) -> i32 {
        match self {
            Self::Pen => 0,
            Self::Arrow => 1,
            Self::Rectangle => 2,
            Self::Triangle => 3,
            Self::Ellipse => 4,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum AnnotationGeometry {
    Pen { points: Vec<ImagePoint> },
    Arrow { start: ImagePoint, end: ImagePoint },
    Rectangle { bounds: ImageBounds },
    Triangle { bounds: ImageBounds },
    Ellipse { bounds: ImageBounds },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Annotation {
    /// Stable, monotonically assigned within its document.
    pub id: u64,
    /// Stable painter's order, first annotation drawn first.
    pub order: u64,
    pub geometry: AnnotationGeometry,
    pub style: StrokeStyle,
}

impl Annotation {
    pub fn validate(&self, width: u32, height: u32) -> Result<(), &'static str> {
        if width == 0 || height == 0 {
            return Err("annotation image dimensions must be positive");
        }
        if !self.style.width.is_finite() || self.style.width <= 0.0 || self.style.width > 128.0 {
            return Err("annotation stroke width must be finite and in 0..=128 pixels");
        }
        let valid_point = |p: ImagePoint| {
            p.x.is_finite()
                && p.y.is_finite()
                && p.x >= 0.0
                && p.y >= 0.0
                && p.x <= width as f64
                && p.y <= height as f64
        };
        match &self.geometry {
            AnnotationGeometry::Pen { points } => {
                if points.is_empty() || !points.iter().copied().all(valid_point) {
                    return Err("pen must contain finite points within the selected image");
                }
            }
            AnnotationGeometry::Arrow { start, end } => {
                if !valid_point(*start) || !valid_point(*end) || start == end {
                    return Err(
                        "arrow endpoints must be distinct points within the selected image",
                    );
                }
            }
            AnnotationGeometry::Rectangle { bounds }
            | AnnotationGeometry::Triangle { bounds }
            | AnnotationGeometry::Ellipse { bounds } => {
                if !bounds.is_valid_for(width, height) {
                    return Err(
                        "shape bounds must be finite, non-empty, and within the selected image",
                    );
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct AnnotationDocument {
    annotations: Vec<Annotation>,
    next_id: u64,
}

impl AnnotationDocument {
    pub fn annotations(&self) -> &[Annotation] {
        &self.annotations
    }

    pub fn len(&self) -> usize {
        self.annotations.len()
    }

    pub fn is_empty(&self) -> bool {
        self.annotations.is_empty()
    }

    pub fn add(
        &mut self,
        geometry: AnnotationGeometry,
        style: StrokeStyle,
        width: u32,
        height: u32,
    ) -> Result<u64, &'static str> {
        let annotation = Annotation {
            id: self.next_id,
            order: self.annotations.len() as u64,
            geometry,
            style,
        };
        annotation.validate(width, height)?;
        let id = annotation.id;
        let next_id = self
            .next_id
            .checked_add(1)
            .ok_or("annotation id overflow")?;
        self.annotations.push(annotation);
        self.next_id = next_id;
        Ok(id)
    }
}

/// Converts local logical overlay coordinates to selected-image physical pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewportToImage {
    pub viewport_width: f64,
    pub viewport_height: f64,
    pub image_width: u32,
    pub image_height: u32,
}

impl ViewportToImage {
    pub fn map(self, x: f64, y: f64) -> Result<ImagePoint, &'static str> {
        if !self.viewport_width.is_finite()
            || !self.viewport_height.is_finite()
            || self.viewport_width <= 0.0
            || self.viewport_height <= 0.0
            || self.image_width == 0
            || self.image_height == 0
            || !x.is_finite()
            || !y.is_finite()
        {
            return Err("viewport and image geometry must be finite and positive");
        }
        Ok(ImagePoint {
            x: x.clamp(0.0, self.viewport_width) * self.image_width as f64 / self.viewport_width,
            y: y.clamp(0.0, self.viewport_height) * self.image_height as f64 / self.viewport_height,
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
struct Draft {
    tool: AnnotationTool,
    start: ImagePoint,
    current: ImagePoint,
    pen_points: Vec<ImagePoint>,
}

/// Edits annotation vectors independently of the screenshot buffer.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AnnotationEditor {
    document: AnnotationDocument,
    active_tool: AnnotationTool,
    draft: Option<Draft>,
}

impl Default for AnnotationTool {
    fn default() -> Self {
        Self::Pen
    }
}

impl AnnotationEditor {
    pub fn document(&self) -> &AnnotationDocument {
        &self.document
    }

    pub fn is_drawing(&self) -> bool {
        self.draft.is_some()
    }

    pub fn select_tool(&mut self, tool: AnnotationTool) {
        self.active_tool = tool;
    }

    pub fn begin(&mut self, point: ImagePoint) {
        self.draft = Some(Draft {
            tool: self.active_tool,
            start: point,
            current: point,
            pen_points: vec![point],
        });
    }

    pub fn update(&mut self, point: ImagePoint) {
        let Some(draft) = self.draft.as_mut() else {
            return;
        };
        draft.current = point;
        if draft.tool == AnnotationTool::Pen && draft.pen_points.last().copied() != Some(point) {
            draft.pen_points.push(point);
        }
    }

    pub fn cancel_draft(&mut self) {
        self.draft = None;
    }

    pub fn finish(
        &mut self,
        style: StrokeStyle,
        width: u32,
        height: u32,
    ) -> Result<bool, &'static str> {
        let Some(draft) = self.draft.take() else {
            return Ok(false);
        };
        let geometry = draft.geometry();
        match self.document.add(geometry, style, width, height) {
            Ok(_) => Ok(true),
            Err("shape bounds must be finite, non-empty, and within the selected image")
            | Err("arrow endpoints must be distinct points within the selected image") => Ok(false),
            Err(error) => Err(error),
        }
    }

    pub fn annotations_for_render(&self, style: StrokeStyle) -> Vec<Annotation> {
        let mut annotations = self.document.annotations.clone();
        if let Some(draft) = &self.draft {
            annotations.push(Annotation {
                id: u64::MAX,
                order: self.document.annotations.len() as u64,
                geometry: draft.geometry(),
                style,
            });
        }
        annotations
    }
}

impl Draft {
    fn geometry(&self) -> AnnotationGeometry {
        match self.tool {
            AnnotationTool::Pen => AnnotationGeometry::Pen {
                points: self.pen_points.clone(),
            },
            AnnotationTool::Arrow => AnnotationGeometry::Arrow {
                start: self.start,
                end: self.current,
            },
            AnnotationTool::Rectangle => AnnotationGeometry::Rectangle {
                bounds: ImageBounds::from_points(self.start, self.current),
            },
            AnnotationTool::Triangle => AnnotationGeometry::Triangle {
                bounds: ImageBounds::from_points(self.start, self.current),
            },
            AnnotationTool::Ellipse => AnnotationGeometry::Ellipse {
                bounds: ImageBounds::from_points(self.start, self.current),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AnnotationDocument, AnnotationEditor, AnnotationGeometry, AnnotationTool, ImageBounds,
        ImagePoint, StrokeStyle, ViewportToImage,
    };

    #[test]
    fn creates_unique_stable_annotations_and_rejects_invalid_geometry() {
        let mut document = AnnotationDocument::default();
        let first = document
            .add(
                AnnotationGeometry::Arrow {
                    start: ImagePoint { x: 2.0, y: 3.0 },
                    end: ImagePoint { x: 8.0, y: 9.0 },
                },
                StrokeStyle::default(),
                20,
                20,
            )
            .unwrap();
        let second = document
            .add(
                AnnotationGeometry::Rectangle {
                    bounds: ImageBounds {
                        x: 1.0,
                        y: 2.0,
                        width: 5.0,
                        height: 4.0,
                    },
                },
                StrokeStyle::default(),
                20,
                20,
            )
            .unwrap();
        assert_eq!((first, second), (0, 1));
        assert_eq!(document.annotations()[1].order, 1);
        assert!(document
            .add(
                AnnotationGeometry::Ellipse {
                    bounds: ImageBounds {
                        x: 2.0,
                        y: 3.0,
                        width: 0.0,
                        height: 5.0,
                    },
                },
                StrokeStyle::default(),
                20,
                20,
            )
            .is_err());
        assert_eq!(document.len(), 2);
    }

    #[test]
    fn maps_viewport_points_at_multiple_scales_and_clamps_edges() {
        for (viewport_width, viewport_height) in [
            (1000.0, 500.0),
            (800.0, 400.0),
            (666.666_666_7, 333.333_333_3),
        ] {
            let map = ViewportToImage {
                viewport_width,
                viewport_height,
                image_width: 1000,
                image_height: 500,
            };
            let center = map
                .map(viewport_width / 2.0, viewport_height / 2.0)
                .unwrap();
            assert_eq!(center, ImagePoint { x: 500.0, y: 250.0 });
            assert_eq!(
                map.map(-1.0, 9999.0).unwrap(),
                ImagePoint { x: 0.0, y: 500.0 }
            );
        }
    }

    #[test]
    fn editor_preserves_pen_points_and_normalizes_shape_drag() {
        let mut editor = AnnotationEditor::default();
        editor.begin(ImagePoint { x: 8.0, y: 9.0 });
        editor.update(ImagePoint { x: 6.0, y: 5.0 });
        editor.update(ImagePoint { x: 3.0, y: 2.0 });
        assert!(editor.finish(StrokeStyle::default(), 20, 20).unwrap());
        let AnnotationGeometry::Pen { points } = &editor.document().annotations()[0].geometry
        else {
            panic!("expected pen geometry");
        };
        assert_eq!(
            points,
            &[
                ImagePoint { x: 8.0, y: 9.0 },
                ImagePoint { x: 6.0, y: 5.0 },
                ImagePoint { x: 3.0, y: 2.0 },
            ]
        );

        editor.select_tool(AnnotationTool::Rectangle);
        editor.begin(ImagePoint { x: 15.0, y: 14.0 });
        editor.update(ImagePoint { x: 4.0, y: 3.0 });
        assert!(editor.finish(StrokeStyle::default(), 20, 20).unwrap());
        assert_eq!(
            editor.document().annotations()[1].geometry,
            AnnotationGeometry::Rectangle {
                bounds: ImageBounds {
                    x: 4.0,
                    y: 3.0,
                    width: 11.0,
                    height: 11.0
                }
            }
        );
    }

    #[test]
    fn zero_sized_shape_drafts_are_discarded() {
        let mut editor = AnnotationEditor::default();
        editor.select_tool(AnnotationTool::Ellipse);
        editor.begin(ImagePoint { x: 5.0, y: 5.0 });
        assert!(!editor.finish(StrokeStyle::default(), 20, 20).unwrap());
        assert!(editor.document().is_empty());
    }

    #[test]
    fn rectangle_triangle_and_ellipse_normalize_drags_in_all_quadrants() {
        let directions = [
            (
                ImagePoint { x: 16.0, y: 16.0 },
                ImagePoint { x: 4.0, y: 4.0 },
            ),
            (
                ImagePoint { x: 16.0, y: 16.0 },
                ImagePoint { x: 28.0, y: 4.0 },
            ),
            (
                ImagePoint { x: 16.0, y: 16.0 },
                ImagePoint { x: 4.0, y: 28.0 },
            ),
            (
                ImagePoint { x: 16.0, y: 16.0 },
                ImagePoint { x: 28.0, y: 28.0 },
            ),
        ];
        for tool in [
            AnnotationTool::Rectangle,
            AnnotationTool::Triangle,
            AnnotationTool::Ellipse,
        ] {
            for (start, end) in directions {
                let expected = ImageBounds::from_points(start, end);
                let mut editor = AnnotationEditor::default();
                editor.select_tool(tool);
                editor.begin(start);
                editor.update(end);
                assert!(editor.finish(StrokeStyle::default(), 32, 32).unwrap());
                let geometry = &editor.document().annotations()[0].geometry;
                let bounds = match geometry {
                    AnnotationGeometry::Rectangle { bounds }
                    | AnnotationGeometry::Triangle { bounds }
                    | AnnotationGeometry::Ellipse { bounds } => *bounds,
                    _ => panic!("expected a bounding shape"),
                };
                assert_eq!(bounds, expected);
            }
        }
    }

    #[test]
    fn cancelling_a_draft_does_not_add_an_annotation() {
        let mut editor = AnnotationEditor::default();
        editor.begin(ImagePoint { x: 2.0, y: 2.0 });
        editor.update(ImagePoint { x: 8.0, y: 8.0 });
        assert!(editor.is_drawing());
        editor.cancel_draft();
        assert!(!editor.is_drawing());
        assert!(!editor.finish(StrokeStyle::default(), 10, 10).unwrap());
        assert!(editor.document().is_empty());
    }
}
