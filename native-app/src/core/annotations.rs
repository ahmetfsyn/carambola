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

    pub fn contains(self, point: ImagePoint) -> bool {
        point.x >= self.x
            && point.x <= self.x + self.width
            && point.y >= self.y
            && point.y <= self.y + self.height
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

    pub const PALETTE: [Self; 5] = [
        Self::RED,
        Self {
            r: 255,
            g: 255,
            b: 255,
            a: 255,
        },
        Self {
            r: 250,
            g: 204,
            b: 21,
            a: 255,
        },
        Self {
            r: 34,
            g: 211,
            b: 238,
            a: 255,
        },
        Self {
            r: 34,
            g: 197,
            b: 94,
            a: 255,
        },
    ];

    pub fn from_palette_index(index: i32) -> Self {
        Self::PALETTE
            .get(index.clamp(0, Self::PALETTE.len() as i32 - 1) as usize)
            .copied()
            .unwrap_or(Self::RED)
    }

    pub fn palette_index(self) -> i32 {
        Self::PALETTE
            .iter()
            .position(|color| *color == self)
            .unwrap_or(0) as i32
    }
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextAlignment {
    #[default]
    Left,
    Center,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextStyle {
    /// Font size in selected-image physical pixels.
    pub font_size: f64,
    pub bold: bool,
    pub italic: bool,
    pub alignment: TextAlignment,
}

impl Default for TextStyle {
    fn default() -> Self {
        Self {
            font_size: 24.0,
            bold: false,
            italic: false,
            alignment: TextAlignment::Left,
        }
    }
}

impl TextStyle {
    /// Bounds of the software bitmap text, including its inter-line spacing.
    pub fn bounds_for(self, position: ImagePoint, text: &str) -> ImageBounds {
        if !self.font_size.is_finite() || self.font_size <= 0.0 {
            return ImageBounds {
                x: position.x,
                y: position.y,
                width: 0.0,
                height: 0.0,
            };
        }
        let cell = self.font_size / 7.0;
        let max_line_chars = text
            .split('\n')
            .map(|line| line.chars().filter(|ch| *ch != '\r').count())
            .max()
            .unwrap_or(0);
        let italic_overhang = if self.italic { cell * 0.9 } else { 0.0 };
        let bold_overhang = if self.bold { cell * 0.2 } else { 0.0 };
        ImageBounds {
            x: position.x,
            y: position.y,
            width: max_line_chars as f64 * cell * 6.0 + italic_overhang + bold_overhang,
            height: text.split('\n').count() as f64 * cell * 9.0,
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
    Text,
    Mosaic,
}

impl AnnotationTool {
    /// Select the next tool in the single-button shape cycle. Non-shape tools
    /// enter the cycle at Rectangle.
    pub fn next_shape(self) -> Self {
        match self {
            Self::Rectangle => Self::Ellipse,
            Self::Ellipse => Self::Triangle,
            Self::Triangle => Self::Rectangle,
            _ => Self::Rectangle,
        }
    }

    pub fn from_ui_index(index: i32) -> Option<Self> {
        match index {
            0 => Some(Self::Pen),
            1 => Some(Self::Arrow),
            2 => Some(Self::Rectangle),
            3 => Some(Self::Triangle),
            4 => Some(Self::Ellipse),
            5 => Some(Self::Text),
            6 => Some(Self::Mosaic),
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
            Self::Text => 5,
            Self::Mosaic => 6,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum AnnotationGeometry {
    Pen {
        points: Vec<ImagePoint>,
    },
    Arrow {
        start: ImagePoint,
        end: ImagePoint,
    },
    Rectangle {
        bounds: ImageBounds,
    },
    Triangle {
        bounds: ImageBounds,
    },
    Ellipse {
        bounds: ImageBounds,
    },
    Text {
        position: ImagePoint,
        text: String,
        text_style: TextStyle,
    },
    Mosaic {
        bounds: ImageBounds,
        block_size: u32,
    },
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
            AnnotationGeometry::Text {
                position,
                text,
                text_style,
            } => {
                if text.trim().is_empty() || text.chars().count() > 2_048 {
                    return Err("text annotation must contain 1..=2048 non-empty characters");
                }
                if !valid_point(*position) {
                    return Err("text position must be within the selected image");
                }
                if !text_style.font_size.is_finite()
                    || !(8.0..=256.0).contains(&text_style.font_size)
                {
                    return Err("text size must be finite and between 8 and 256 pixels");
                }
            }
            AnnotationGeometry::Mosaic { bounds, block_size } => {
                if !bounds.is_valid_for(width, height) {
                    return Err(
                        "mosaic bounds must be finite, non-empty, and within the selected image",
                    );
                }
                if !(2..=128).contains(block_size) {
                    return Err("mosaic block size must be between 2 and 128 pixels");
                }
            }
        }
        Ok(())
    }
}

pub(crate) fn arrowhead_wings(
    start: ImagePoint,
    end: ImagePoint,
    width: f64,
) -> Option<[ImagePoint; 2]> {
    let dx = end.x - start.x;
    let dy = end.y - start.y;
    let length = dx.hypot(dy);
    if !length.is_finite() || length <= f64::EPSILON || !width.is_finite() || width <= 0.0 {
        return None;
    }
    let ux = dx / length;
    let uy = dy / length;
    let head_length = (width.max(1.0) * 4.0).max(8.0).min(length * 0.45);
    let wing_angle = std::f64::consts::PI / 6.0;
    let back_x = -ux * head_length;
    let back_y = -uy * head_length;
    let rotate = |angle: f64| ImagePoint {
        x: end.x + back_x * angle.cos() - back_y * angle.sin(),
        y: end.y + back_x * angle.sin() + back_y * angle.cos(),
    };
    Some([rotate(wing_angle), rotate(-wing_angle)])
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct AnnotationDocument {
    annotations: Vec<Annotation>,
    history: Vec<AnnotationAction>,
    cursor: usize,
    next_id: u64,
    next_order: u64,
}

#[derive(Clone, Debug, PartialEq)]
enum AnnotationAction {
    Added(Annotation),
    Modified {
        before: Annotation,
        after: Annotation,
    },
    Deleted(Annotation),
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
        let next_id = self
            .next_id
            .checked_add(1)
            .ok_or("annotation id overflow")?;
        let next_order = self
            .next_order
            .checked_add(1)
            .ok_or("annotation order overflow")?;
        let annotation = Annotation {
            id: self.next_id,
            order: self.next_order,
            geometry,
            style,
        };
        annotation.validate(width, height)?;
        let id = annotation.id;
        self.history.truncate(self.cursor);
        self.insert_ordered(annotation.clone());
        self.history.push(AnnotationAction::Added(annotation));
        self.cursor += 1;
        self.next_id = next_id;
        self.next_order = next_order;
        Ok(id)
    }

    fn insert_ordered(&mut self, annotation: Annotation) {
        let index = self
            .annotations
            .partition_point(|current| current.order <= annotation.order);
        self.annotations.insert(index, annotation);
    }

    fn modify(&mut self, before: Annotation, after: Annotation) -> bool {
        if before == after {
            return false;
        }
        let Some(index) = self
            .annotations
            .iter()
            .position(|annotation| annotation.id == before.id)
        else {
            return false;
        };
        self.history.truncate(self.cursor);
        self.annotations[index] = after.clone();
        self.history
            .push(AnnotationAction::Modified { before, after });
        self.cursor += 1;
        true
    }

    pub fn delete(&mut self, id: u64) -> bool {
        let Some(index) = self
            .annotations
            .iter()
            .position(|annotation| annotation.id == id)
        else {
            return false;
        };
        let annotation = self.annotations.remove(index);
        self.history.truncate(self.cursor);
        self.history.push(AnnotationAction::Deleted(annotation));
        self.cursor += 1;
        true
    }

    pub fn edit_text(
        &mut self,
        id: u64,
        position: ImagePoint,
        text: String,
        text_style: TextStyle,
        color: StrokeColor,
        width: u32,
        height: u32,
    ) -> Result<bool, &'static str> {
        let index = self
            .annotations
            .iter()
            .position(|annotation| annotation.id == id)
            .ok_or("text annotation no longer exists")?;
        let before = self.annotations[index].clone();
        if !matches!(&before.geometry, AnnotationGeometry::Text { .. }) {
            return Err("annotation is not text");
        }
        let after = Annotation {
            id: before.id,
            order: before.order,
            geometry: AnnotationGeometry::Text {
                position,
                text,
                text_style,
            },
            style: StrokeStyle {
                width: before.style.width,
                color,
            },
        };
        after.validate(width, height)?;
        if before == after {
            return Ok(false);
        }
        let _ = index;
        Ok(self.modify(before, after))
    }

    pub fn annotation(&self, id: u64) -> Option<&Annotation> {
        self.annotations
            .iter()
            .find(|annotation| annotation.id == id)
    }

    pub fn undo(&mut self) -> bool {
        if self.cursor == 0 {
            return false;
        }
        match self.history[self.cursor - 1].clone() {
            AnnotationAction::Added(annotation) => {
                if let Some(index) = self
                    .annotations
                    .iter()
                    .position(|current| current.id == annotation.id)
                {
                    self.annotations.remove(index);
                }
            }
            AnnotationAction::Modified { before, .. } => {
                if let Some(current) = self
                    .annotations
                    .iter_mut()
                    .find(|current| current.id == before.id)
                {
                    *current = before;
                }
            }
            AnnotationAction::Deleted(annotation) => self.insert_ordered(annotation),
        }
        self.cursor -= 1;
        true
    }

    pub fn redo(&mut self) -> bool {
        if self.cursor >= self.history.len() {
            return false;
        }
        match self.history[self.cursor].clone() {
            AnnotationAction::Added(annotation) => self.insert_ordered(annotation),
            AnnotationAction::Modified { after, .. } => {
                if let Some(current) = self
                    .annotations
                    .iter_mut()
                    .find(|current| current.id == after.id)
                {
                    *current = after;
                }
            }
            AnnotationAction::Deleted(annotation) => {
                if let Some(index) = self
                    .annotations
                    .iter()
                    .position(|current| current.id == annotation.id)
                {
                    self.annotations.remove(index);
                }
            }
        }
        self.cursor += 1;
        true
    }

    pub fn can_undo(&self) -> bool {
        self.cursor > 0
    }
    pub fn can_redo(&self) -> bool {
        self.cursor < self.history.len()
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

#[derive(Clone, Debug, PartialEq)]
pub struct TextAnnotationDraft {
    pub annotation_id: Option<u64>,
    pub position: ImagePoint,
    pub text: String,
    pub text_style: TextStyle,
    pub color: StrokeColor,
}

/// Edits annotation vectors independently of the screenshot buffer.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AnnotationEditor {
    document: AnnotationDocument,
    active_tool: AnnotationTool,
    active_stroke_style: StrokeStyle,
    active_text_style: TextStyle,
    draft: Option<Draft>,
    text_draft: Option<TextAnnotationDraft>,
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

    pub fn active_tool(&self) -> AnnotationTool {
        self.active_tool
    }

    pub fn active_stroke_style(&self) -> StrokeStyle {
        self.active_stroke_style
    }

    pub fn active_color(&self) -> StrokeColor {
        self.active_stroke_style.color
    }

    pub fn active_size(&self) -> f64 {
        if self.active_tool == AnnotationTool::Text {
            self.active_text_style.font_size
        } else {
            self.active_stroke_style.width
        }
    }

    /// Update the one toolbar size value using the active tool's natural unit.
    /// Vector tools use physical stroke pixels; Text uses physical font pixels.
    pub fn set_active_size(&mut self, size: f64) -> f64 {
        if self.active_tool == AnnotationTool::Text {
            let size = if size.is_finite() {
                size.clamp(8.0, 256.0)
            } else {
                self.active_text_style.font_size
            };
            self.active_text_style.font_size = size;
            if let Some(draft) = self.text_draft.as_mut() {
                draft.text_style.font_size = size;
            }
            size
        } else {
            let size = if size.is_finite() {
                size.clamp(1.0, 32.0)
            } else {
                self.active_stroke_style.width
            };
            self.active_stroke_style.width = size;
            size
        }
    }

    /// The active color is shared by every tool; committed annotations retain
    /// their own stored colors.
    pub fn set_active_color(&mut self, color: StrokeColor) {
        self.active_stroke_style.color = color;
        if let Some(draft) = self.text_draft.as_mut() {
            draft.color = color;
        }
    }

    pub fn set_active_text_style(&mut self, text_style: TextStyle) {
        self.active_text_style = text_style;
        if let Some(draft) = self.text_draft.as_mut() {
            draft.text_style = text_style;
        }
    }

    pub fn add_text(
        &mut self,
        position: ImagePoint,
        text: String,
        width: u32,
        height: u32,
    ) -> Result<bool, &'static str> {
        self.add_styled_text(
            position,
            text,
            TextStyle::default(),
            StrokeColor::RED,
            width,
            height,
        )
    }

    pub fn add_styled_text(
        &mut self,
        position: ImagePoint,
        text: String,
        text_style: TextStyle,
        color: StrokeColor,
        width: u32,
        height: u32,
    ) -> Result<bool, &'static str> {
        if text.trim().is_empty() {
            return Ok(false);
        }
        self.document
            .add(
                AnnotationGeometry::Text {
                    position,
                    text,
                    text_style,
                },
                StrokeStyle {
                    width: StrokeStyle::default().width,
                    color,
                },
                width,
                height,
            )
            .map(|_| true)
    }

    pub fn begin_text_annotation(&mut self, position: ImagePoint) {
        self.draft = None;
        self.text_draft = Some(TextAnnotationDraft {
            annotation_id: None,
            position,
            text: String::new(),
            text_style: self.active_text_style,
            color: self.active_color(),
        });
    }

    pub fn begin_text_edit(&mut self, id: u64) -> Result<(), &'static str> {
        let annotation = self
            .document
            .annotation(id)
            .ok_or("text annotation no longer exists")?;
        let (position, text, text_style) = match &annotation.geometry {
            AnnotationGeometry::Text {
                position,
                text,
                text_style,
            } => (*position, text.clone(), *text_style),
            _ => return Err("annotation is not text"),
        };
        self.draft = None;
        self.text_draft = Some(TextAnnotationDraft {
            annotation_id: Some(id),
            position,
            text,
            text_style,
            color: annotation.style.color,
        });
        Ok(())
    }

    pub fn text_draft(&self) -> Option<&TextAnnotationDraft> {
        self.text_draft.as_ref()
    }

    pub fn text_annotation_at(&self, point: ImagePoint) -> Option<u64> {
        self.document
            .annotations()
            .iter()
            .rev()
            .find_map(|annotation| {
                let AnnotationGeometry::Text {
                    position,
                    text,
                    text_style,
                } = &annotation.geometry
                else {
                    return None;
                };
                text_style
                    .bounds_for(*position, text)
                    .contains(point)
                    .then_some(annotation.id)
            })
    }

    pub fn update_text_draft(
        &mut self,
        text: String,
        text_style: TextStyle,
        color: StrokeColor,
    ) -> Result<(), &'static str> {
        let draft = self
            .text_draft
            .as_mut()
            .ok_or("text editor draft is unavailable")?;
        draft.text = text;
        draft.text_style = text_style;
        draft.color = color;
        Ok(())
    }

    pub fn move_text_draft(&mut self, position: ImagePoint) -> Result<(), &'static str> {
        let draft = self
            .text_draft
            .as_mut()
            .ok_or("text editor draft is unavailable")?;
        draft.position = position;
        Ok(())
    }

    pub fn commit_text_draft(&mut self, width: u32, height: u32) -> Result<bool, &'static str> {
        let draft = self
            .text_draft
            .as_ref()
            .ok_or("text editor draft is unavailable")?
            .clone();
        if draft.text.trim().is_empty() {
            return Ok(false);
        }
        let result = if let Some(id) = draft.annotation_id {
            self.document.edit_text(
                id,
                draft.position,
                draft.text,
                draft.text_style,
                draft.color,
                width,
                height,
            )
        } else {
            self.add_styled_text(
                draft.position,
                draft.text,
                draft.text_style,
                draft.color,
                width,
                height,
            )
        }?;
        self.text_draft = None;
        Ok(result)
    }

    pub fn cancel_text_draft(&mut self) {
        self.text_draft = None;
    }

    pub fn undo(&mut self) -> bool {
        self.draft = None;
        self.text_draft = None;
        self.document.undo()
    }

    pub fn redo(&mut self) -> bool {
        self.draft = None;
        self.text_draft = None;
        self.document.redo()
    }

    pub fn can_undo(&self) -> bool {
        self.document.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.document.can_redo()
    }

    pub fn can_delete_last(&self) -> bool {
        !self.document.is_empty()
    }

    pub fn delete_last(&mut self) -> bool {
        let Some(id) = self
            .document
            .annotations()
            .last()
            .map(|annotation| annotation.id)
        else {
            return false;
        };
        self.document.delete(id)
    }

    pub fn select_tool(&mut self, tool: AnnotationTool) {
        self.active_tool = tool;
    }

    pub fn begin(&mut self, point: ImagePoint) {
        self.text_draft = None;
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
        let mut annotations = self.document.annotations().to_vec();
        if let Some(draft) = &self.draft {
            annotations.push(Annotation {
                id: u64::MAX,
                order: self.document.len() as u64,
                geometry: draft.geometry(),
                style,
            });
        }
        if let Some(draft) = &self.text_draft {
            let annotation = Annotation {
                id: draft.annotation_id.unwrap_or(u64::MAX),
                order: draft
                    .annotation_id
                    .and_then(|id| {
                        self.document
                            .annotation(id)
                            .map(|annotation| annotation.order)
                    })
                    .unwrap_or(annotations.len() as u64),
                geometry: AnnotationGeometry::Text {
                    position: draft.position,
                    text: draft.text.clone(),
                    text_style: draft.text_style,
                },
                style: StrokeStyle {
                    width: style.width,
                    color: draft.color,
                },
            };
            if let Some(existing) = draft.annotation_id.and_then(|id| {
                annotations
                    .iter_mut()
                    .find(|annotation| annotation.id == id)
            }) {
                *existing = annotation;
            } else {
                annotations.push(annotation);
            }
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
            AnnotationTool::Text => AnnotationGeometry::Text {
                position: self.start,
                text: String::new(),
                text_style: TextStyle::default(),
            },
            AnnotationTool::Mosaic => AnnotationGeometry::Mosaic {
                bounds: ImageBounds::from_points(self.start, self.current),
                block_size: 12,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AnnotationDocument, AnnotationEditor, AnnotationGeometry, AnnotationTool, ImageBounds,
        ImagePoint, StrokeColor, StrokeStyle, TextAlignment, TextStyle, ViewportToImage,
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
    fn toolbar_tool_indices_round_trip_to_the_matching_interaction_tool() {
        let tools = [
            AnnotationTool::Pen,
            AnnotationTool::Arrow,
            AnnotationTool::Rectangle,
            AnnotationTool::Triangle,
            AnnotationTool::Ellipse,
            AnnotationTool::Text,
            AnnotationTool::Mosaic,
        ];
        for (index, tool) in tools.into_iter().enumerate() {
            assert_eq!(tool.ui_index(), index as i32);
            assert_eq!(AnnotationTool::from_ui_index(index as i32), Some(tool));
        }
        assert_eq!(AnnotationTool::from_ui_index(-1), None);
        assert_eq!(AnnotationTool::from_ui_index(7), None);
    }

    #[test]
    fn shape_tool_button_cycles_rectangle_ellipse_triangle_then_rectangle() {
        assert_eq!(AnnotationTool::Pen.next_shape(), AnnotationTool::Rectangle);
        assert_eq!(
            AnnotationTool::Rectangle.next_shape(),
            AnnotationTool::Ellipse
        );
        assert_eq!(
            AnnotationTool::Ellipse.next_shape(),
            AnnotationTool::Triangle
        );
        assert_eq!(
            AnnotationTool::Triangle.next_shape(),
            AnnotationTool::Rectangle
        );
        assert_eq!(
            AnnotationTool::Mosaic.next_shape(),
            AnnotationTool::Rectangle
        );
    }

    #[test]
    fn shared_color_palette_is_distinct_and_keeps_red_as_the_default() {
        assert_eq!(StrokeColor::from_palette_index(0), StrokeColor::RED);
        assert_eq!(StrokeColor::RED.palette_index(), 0);
        for (index, color) in StrokeColor::PALETTE.into_iter().enumerate() {
            assert_eq!(StrokeColor::from_palette_index(index as i32), color);
            assert_eq!(color.palette_index(), index as i32);
        }
        for (index, color) in StrokeColor::PALETTE.iter().enumerate() {
            assert!(!StrokeColor::PALETTE[..index].contains(color));
        }
    }

    #[test]
    fn shared_size_and_color_apply_to_new_tools_without_recoloring_committed_annotations() {
        let mut editor = AnnotationEditor::default();
        let yellow = StrokeColor::from_palette_index(2);
        editor.set_active_color(yellow);
        assert_eq!(editor.set_active_size(6.0), 6.0);
        editor.begin(ImagePoint { x: 2.0, y: 2.0 });
        editor.update(ImagePoint { x: 18.0, y: 18.0 });
        assert!(editor.finish(editor.active_stroke_style(), 32, 32).unwrap());

        let committed_style = editor.document().annotations()[0].style;
        assert_eq!(committed_style.width, 6.0);
        assert_eq!(committed_style.color, yellow);

        let cyan = StrokeColor::from_palette_index(3);
        editor.set_active_color(cyan);
        editor.set_active_size(9.0);
        assert_eq!(editor.document().annotations()[0].style, committed_style);

        for tool in [
            AnnotationTool::Pen,
            AnnotationTool::Arrow,
            AnnotationTool::Rectangle,
            AnnotationTool::Triangle,
            AnnotationTool::Ellipse,
            AnnotationTool::Mosaic,
        ] {
            editor.select_tool(tool);
            assert_eq!(editor.set_active_size(5.0), 5.0);
            assert_eq!(editor.active_stroke_style().width, 5.0);
            assert_eq!(editor.active_stroke_style().color, cyan);
        }

        editor.select_tool(AnnotationTool::Text);
        assert_eq!(editor.set_active_size(36.0), 36.0);
        editor.begin_text_annotation(ImagePoint { x: 1.0, y: 1.0 });
        let draft = editor.text_draft().unwrap();
        assert_eq!(draft.text_style.font_size, 36.0);
        assert_eq!(draft.color, cyan);
        assert_eq!(editor.set_active_size(1000.0), 256.0);
        assert_eq!(editor.text_draft().unwrap().text_style.font_size, 256.0);
    }

    #[test]
    fn switching_tools_preserves_committed_annotations_and_the_active_draft_tool() {
        let mut editor = AnnotationEditor::default();
        editor.begin(ImagePoint { x: 2.0, y: 2.0 });
        editor.update(ImagePoint { x: 8.0, y: 8.0 });
        assert!(editor.finish(StrokeStyle::default(), 32, 32).unwrap());
        let committed = editor.document().annotations().to_vec();

        editor.select_tool(AnnotationTool::Rectangle);
        editor.begin(ImagePoint { x: 10.0, y: 11.0 });
        editor.update(ImagePoint { x: 24.0, y: 27.0 });
        editor.select_tool(AnnotationTool::Ellipse);

        assert_eq!(editor.active_tool(), AnnotationTool::Ellipse);
        assert_eq!(editor.document().annotations(), committed);
        let preview = editor.annotations_for_render(StrokeStyle::default());
        assert_eq!(preview.len(), 2);
        assert_eq!(
            preview[1].geometry,
            AnnotationGeometry::Rectangle {
                bounds: ImageBounds {
                    x: 10.0,
                    y: 11.0,
                    width: 14.0,
                    height: 16.0,
                }
            }
        );

        assert!(editor.finish(StrokeStyle::default(), 32, 32).unwrap());
        assert_eq!(&editor.document().annotations()[..1], committed.as_slice());
        assert!(matches!(
            &editor.document().annotations()[1].geometry,
            AnnotationGeometry::Rectangle { .. }
        ));
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

    #[test]
    fn text_requires_content_and_keeps_its_image_position() {
        let mut editor = AnnotationEditor::default();
        assert!(!editor
            .add_text(ImagePoint { x: 2.0, y: 3.0 }, "  ".into(), 20, 20)
            .unwrap());
        assert!(editor
            .add_text(ImagePoint { x: 2.0, y: 3.0 }, "Hello".into(), 20, 20)
            .unwrap());
        assert_eq!(
            editor.document().annotations()[0].geometry,
            AnnotationGeometry::Text {
                position: ImagePoint { x: 2.0, y: 3.0 },
                text: "Hello".into(),
                text_style: TextStyle::default(),
            }
        );
        assert!(editor
            .add_text(ImagePoint { x: 21.0, y: 0.0 }, "outside".into(), 20, 20)
            .is_err());
    }

    #[test]
    fn text_style_keeps_the_system_default_and_supported_style_defaults() {
        let style = TextStyle::default();
        assert_eq!(style.font_size, 24.0);
        assert!(!style.bold);
        assert!(!style.italic);
        assert_eq!(style.alignment, TextAlignment::Left);
    }

    #[test]
    fn text_hit_testing_selects_topmost_multiline_annotation() {
        let mut editor = AnnotationEditor::default();
        editor
            .add_styled_text(
                ImagePoint { x: 5.0, y: 4.0 },
                "AB\nCD".into(),
                TextStyle {
                    font_size: 14.0,
                    ..TextStyle::default()
                },
                StrokeColor::RED,
                40,
                40,
            )
            .unwrap();
        let lower_id = editor.document().annotations()[0].id;
        editor
            .add_styled_text(
                ImagePoint { x: 5.0, y: 4.0 },
                "Z".into(),
                TextStyle {
                    font_size: 14.0,
                    ..TextStyle::default()
                },
                StrokeColor::RED,
                40,
                40,
            )
            .unwrap();
        let top_id = editor.document().annotations()[1].id;

        assert_eq!(
            editor.text_annotation_at(ImagePoint { x: 6.0, y: 5.0 }),
            Some(top_id)
        );
        assert_eq!(
            editor.text_annotation_at(ImagePoint { x: 6.0, y: 25.0 }),
            Some(lower_id)
        );
        assert_eq!(
            editor.text_annotation_at(ImagePoint { x: 39.0, y: 39.0 }),
            None
        );
    }

    #[test]
    fn text_edit_preserves_identity_and_undo_redo_restores_content_and_style() {
        let mut editor = AnnotationEditor::default();
        let initial_style = TextStyle {
            font_size: 18.0,
            bold: true,
            italic: false,
            alignment: TextAlignment::Center,
        };
        assert!(editor
            .add_styled_text(
                ImagePoint { x: 4.0, y: 5.0 },
                "First\nline".into(),
                initial_style,
                StrokeColor::RED,
                64,
                64,
            )
            .unwrap());
        let original = editor.document().annotations()[0].clone();
        let id = original.id;

        editor.begin_text_edit(id).unwrap();
        editor
            .update_text_draft(
                "Updated\ntext".into(),
                TextStyle {
                    font_size: 30.0,
                    bold: false,
                    italic: true,
                    alignment: TextAlignment::Right,
                },
                StrokeColor {
                    r: 255,
                    g: 255,
                    b: 255,
                    a: 255,
                },
            )
            .unwrap();
        editor
            .move_text_draft(ImagePoint { x: 9.0, y: 11.0 })
            .unwrap();
        assert_eq!(editor.document().annotations(), &[original.clone()]);
        let preview = editor.annotations_for_render(StrokeStyle::default());
        assert_eq!(preview[0].id, id);
        assert!(matches!(
            &preview[0].geometry,
            AnnotationGeometry::Text { text, text_style, position }
                if text == "Updated\ntext"
                    && *position == ImagePoint { x: 9.0, y: 11.0 }
                    && text_style.font_size == 30.0
                    && text_style.italic
                    && text_style.alignment == TextAlignment::Right
        ));
        assert!(editor.commit_text_draft(64, 64).unwrap());
        let edited = editor.document().annotations()[0].clone();
        assert_eq!(edited.id, original.id);
        assert_eq!(edited.order, original.order);
        assert!(editor.undo());
        assert_eq!(editor.document().annotations(), &[original.clone()]);
        assert!(editor.redo());
        assert_eq!(editor.document().annotations(), &[edited]);
    }

    #[test]
    fn empty_text_draft_cannot_create_an_annotation_and_cancel_discards_it() {
        let mut editor = AnnotationEditor::default();
        editor.begin_text_annotation(ImagePoint { x: 2.0, y: 3.0 });
        editor
            .update_text_draft(" \n ".into(), TextStyle::default(), StrokeColor::RED)
            .unwrap();
        assert!(!editor.commit_text_draft(20, 20).unwrap());
        assert!(editor.document().is_empty());
        assert!(editor.text_draft().is_some());
        editor.cancel_text_draft();
        assert!(editor.text_draft().is_none());
        assert!(editor.document().is_empty());
    }

    #[test]
    fn cancelling_text_edit_keeps_committed_content_and_style() {
        let mut editor = AnnotationEditor::default();
        editor
            .add_styled_text(
                ImagePoint { x: 2.0, y: 3.0 },
                "Original".into(),
                TextStyle {
                    font_size: 18.0,
                    bold: true,
                    ..TextStyle::default()
                },
                StrokeColor::RED,
                40,
                40,
            )
            .unwrap();
        let original = editor.document().annotations()[0].clone();
        editor.begin_text_edit(original.id).unwrap();
        editor
            .update_text_draft(
                "Discard this edit".into(),
                TextStyle::default(),
                StrokeColor {
                    r: 255,
                    g: 255,
                    b: 255,
                    a: 255,
                },
            )
            .unwrap();

        editor.cancel_text_draft();
        assert!(editor.text_draft().is_none());
        assert_eq!(editor.document().annotations(), &[original]);
    }

    #[test]
    fn delete_last_remains_undoable_without_annotation_selection_state() {
        let mut editor = AnnotationEditor::default();
        assert!(!editor.can_delete_last());
        assert!(!editor.delete_last());
        editor
            .document
            .add(
                AnnotationGeometry::Rectangle {
                    bounds: ImageBounds {
                        x: 2.0,
                        y: 3.0,
                        width: 10.0,
                        height: 8.0,
                    },
                },
                StrokeStyle::default(),
                32,
                32,
            )
            .unwrap();
        let last = editor
            .document
            .add(
                AnnotationGeometry::Text {
                    position: ImagePoint { x: 4.0, y: 5.0 },
                    text: "last".into(),
                    text_style: TextStyle::default(),
                },
                StrokeStyle::default(),
                32,
                32,
            )
            .unwrap();
        assert!(editor.can_delete_last());
        assert!(editor.delete_last());
        assert!(editor.document().annotation(last).is_none());
        assert!(editor.undo());
        assert!(editor.document().annotation(last).is_some());
        assert!(editor.redo());
        assert!(editor.document().annotation(last).is_none());
    }

    #[test]
    fn undo_redo_mixed_operations_and_new_operation_clears_redo() {
        let mut editor = AnnotationEditor::default();
        editor.begin(ImagePoint { x: 1.0, y: 1.0 });
        editor.update(ImagePoint { x: 8.0, y: 8.0 });
        assert!(editor.finish(StrokeStyle::default(), 32, 32).unwrap());
        editor.select_tool(AnnotationTool::Mosaic);
        editor.begin(ImagePoint { x: 10.0, y: 10.0 });
        editor.update(ImagePoint { x: 18.0, y: 18.0 });
        assert!(editor.finish(StrokeStyle::default(), 32, 32).unwrap());
        assert!(editor
            .add_text(ImagePoint { x: 2.0, y: 20.0 }, "A".into(), 32, 32)
            .unwrap());
        assert_eq!(editor.document().len(), 3);

        assert!(editor.undo());
        assert_eq!(editor.document().len(), 2);
        assert!(editor.can_redo());
        assert!(editor.redo());
        assert_eq!(editor.document().len(), 3);
        assert!(editor.undo());
        editor.select_tool(AnnotationTool::Ellipse);
        editor.begin(ImagePoint { x: 20.0, y: 2.0 });
        editor.update(ImagePoint { x: 28.0, y: 12.0 });
        assert!(editor.finish(StrokeStyle::default(), 32, 32).unwrap());
        assert!(!editor.can_redo());
        assert!(!editor.redo());
        assert_eq!(editor.document().len(), 3);
    }

    #[test]
    fn history_boundaries_are_safe_and_new_ids_remain_unique() {
        let mut doc = AnnotationDocument::default();
        assert!(!doc.undo());
        assert!(!doc.redo());
        let first = doc
            .add(
                AnnotationGeometry::Text {
                    position: ImagePoint { x: 0.0, y: 0.0 },
                    text: "one".into(),
                    text_style: TextStyle {
                        font_size: 12.0,
                        ..TextStyle::default()
                    },
                },
                StrokeStyle::default(),
                20,
                20,
            )
            .unwrap();
        assert!(doc.undo());
        let second = doc
            .add(
                AnnotationGeometry::Mosaic {
                    bounds: ImageBounds {
                        x: 0.0,
                        y: 0.0,
                        width: 1.0,
                        height: 1.0,
                    },
                    block_size: 2,
                },
                StrokeStyle::default(),
                20,
                20,
            )
            .unwrap();
        assert!(second > first);
        assert!(!doc.can_redo());
    }
}
