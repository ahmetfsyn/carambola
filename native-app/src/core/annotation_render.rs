//! Deterministic software rasterization for annotation previews and export.
//!
//! Geometry is expressed in selected-image edge coordinates. The returned
//! layer has the same physical dimensions and remains transparent wherever no
//! annotation was drawn, so editing never changes the source screenshot.

use super::annotations::{Annotation, AnnotationGeometry, ImagePoint, StrokeColor, StrokeStyle};
use super::frame::CapturedFrame;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnnotationLayer {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

impl AnnotationLayer {
    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn pixels(&self) -> &[u8] {
        &self.rgba
    }
}

/// Rasterize the provided vector annotations into a transparent image layer.
/// Invalid entries are skipped so transient UI drafts cannot panic rendering.
pub fn render(
    annotations: &[Annotation],
    width: u32,
    height: u32,
) -> Result<AnnotationLayer, &'static str> {
    if width == 0 || height == 0 {
        return Err("annotation layer dimensions must be positive");
    }
    let byte_len = (width as usize)
        .checked_mul(height as usize)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or("annotation layer dimensions overflow addressable memory")?;
    let mut layer = AnnotationLayer {
        width,
        height,
        rgba: vec![0; byte_len],
    };
    let mut ordered = annotations.iter().collect::<Vec<_>>();
    ordered.sort_by_key(|annotation| annotation.order);

    for annotation in ordered {
        if annotation.validate(width, height).is_err() {
            continue;
        }
        draw_annotation(&mut layer, annotation);
    }
    Ok(layer)
}

fn draw_annotation(layer: &mut AnnotationLayer, annotation: &Annotation) {
    match &annotation.geometry {
        AnnotationGeometry::Pen { points } => {
            if points.len() == 1 {
                layer.stamp(points[0], annotation.style);
            } else {
                for pair in points.windows(2) {
                    layer.line(pair[0], pair[1], annotation.style);
                }
            }
        }
        AnnotationGeometry::Arrow { start, end } => {
            layer.arrow(*start, *end, annotation.style);
        }
        AnnotationGeometry::Rectangle { bounds } => {
            let top_left = ImagePoint {
                x: bounds.x,
                y: bounds.y,
            };
            let top_right = ImagePoint {
                x: bounds.x + bounds.width,
                y: bounds.y,
            };
            let bottom_right = ImagePoint {
                x: bounds.x + bounds.width,
                y: bounds.y + bounds.height,
            };
            let bottom_left = ImagePoint {
                x: bounds.x,
                y: bounds.y + bounds.height,
            };
            layer.path(
                &[top_left, top_right, bottom_right, bottom_left, top_left],
                annotation.style,
            );
        }
        AnnotationGeometry::Triangle { bounds } => {
            let top = ImagePoint {
                x: bounds.x + bounds.width / 2.0,
                y: bounds.y,
            };
            let bottom_right = ImagePoint {
                x: bounds.x + bounds.width,
                y: bounds.y + bounds.height,
            };
            let bottom_left = ImagePoint {
                x: bounds.x,
                y: bounds.y + bounds.height,
            };
            layer.path(&[top, bottom_right, bottom_left, top], annotation.style);
        }
        AnnotationGeometry::Ellipse { bounds } => {
            layer.ellipse(
                bounds.x,
                bounds.y,
                bounds.width,
                bounds.height,
                annotation.style,
            );
        }
        AnnotationGeometry::Text {
            position,
            text,
            font_size,
        } => {
            layer.text(*position, text, *font_size, annotation.style.color);
        }
        AnnotationGeometry::Mosaic { .. } => {}
    }
}

/// Return the two arrowhead wing endpoints, oriented behind the arrow tip.
pub fn arrowhead_wings(start: ImagePoint, end: ImagePoint, width: f64) -> Option<[ImagePoint; 2]> {
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

/// Composite a transparent annotation layer onto a copy of the crop.
pub fn composite(
    source: &CapturedFrame,
    layer: &AnnotationLayer,
) -> Result<CapturedFrame, &'static str> {
    if source.width() != layer.width || source.height() != layer.height {
        return Err("annotation layer dimensions must match the source crop");
    }
    let mut rgba = source.pixels().to_vec();
    for (target, overlay) in rgba.chunks_exact_mut(4).zip(layer.rgba.chunks_exact(4)) {
        blend_over(target, overlay);
    }
    CapturedFrame::new(source.width(), source.height(), rgba)
}

/// Apply annotations to a copy of the source in painter's order. The captured
/// crop is never modified, and mosaic effects include earlier annotations.
pub fn render_composite(
    source: &CapturedFrame,
    annotations: &[Annotation],
) -> Result<CapturedFrame, &'static str> {
    let mut pixels = source.pixels().to_vec();
    let mut ordered = annotations.iter().collect::<Vec<_>>();
    ordered.sort_by_key(|annotation| annotation.order);
    let mut layer = render(&[], source.width(), source.height())?;
    for annotation in ordered {
        if annotation
            .validate(source.width(), source.height())
            .is_err()
        {
            continue;
        }
        if let AnnotationGeometry::Mosaic { bounds, block_size } = &annotation.geometry {
            composite_layer(&mut pixels, &mut layer);
            pixelate_region(
                &mut pixels,
                source.width(),
                source.height(),
                *bounds,
                *block_size,
            );
        } else {
            draw_annotation(&mut layer, annotation);
        }
    }
    composite_layer(&mut pixels, &mut layer);
    CapturedFrame::new(source.width(), source.height(), pixels)
}

fn composite_layer(pixels: &mut [u8], layer: &mut AnnotationLayer) {
    for (target, overlay) in pixels.chunks_exact_mut(4).zip(layer.rgba.chunks_exact(4)) {
        blend_over(target, overlay);
    }
    layer.rgba.fill(0);
}

fn pixelate_region(
    pixels: &mut [u8],
    width: u32,
    height: u32,
    bounds: super::annotations::ImageBounds,
    block: u32,
) {
    let left = (bounds.x.floor() as u32).min(width);
    let top = (bounds.y.floor() as u32).min(height);
    let right = (bounds.x + bounds.width).ceil() as u32;
    let bottom = (bounds.y + bounds.height).ceil() as u32;
    let block = block.max(2);
    let mut y = top;
    while y < bottom {
        let y_end = y.saturating_add(block).min(bottom);
        let mut x = left;
        while x < right {
            let x_end = x.saturating_add(block).min(right);
            let mut sum = [0u64; 4];
            let mut count = 0u64;
            for sample_y in y..y_end {
                for sample_x in x..x_end {
                    let i = (sample_y as usize * width as usize + sample_x as usize) * 4;
                    for channel in 0..4 {
                        sum[channel] += pixels[i + channel] as u64;
                    }
                    count += 1;
                }
            }
            if count > 0 {
                let average = sum.map(|value| ((value + count / 2) / count) as u8);
                for target_y in y..y_end {
                    for target_x in x..x_end {
                        let i = (target_y as usize * width as usize + target_x as usize) * 4;
                        pixels[i..i + 4].copy_from_slice(&average);
                    }
                }
            }
            x = x_end;
        }
        y = y_end;
    }
}

impl AnnotationLayer {
    fn text(&mut self, position: ImagePoint, text: &str, font_size: f64, color: StrokeColor) {
        if !font_size.is_finite() || font_size <= 0.0 {
            return;
        }
        let cell = font_size / 7.0;
        let origin_x = position.x;
        let mut x = origin_x;
        let mut y = position.y;
        for ch in text.chars() {
            if ch == '\n' {
                x = origin_x;
                y += cell * 9.0;
                continue;
            }
            if ch == '\r' {
                continue;
            }
            let rows = glyph_rows(ch);
            for (row, bits) in rows.iter().enumerate() {
                for col in 0..5 {
                    if bits & (1 << (4 - col)) != 0 {
                        self.fill_rect(
                            x + col as f64 * cell,
                            y + row as f64 * cell,
                            x + (col + 1) as f64 * cell,
                            y + (row + 1) as f64 * cell,
                            color,
                        );
                    }
                }
            }
            x += cell * 6.0;
            if x >= self.width as f64 {
                break;
            }
        }
    }

    fn fill_rect(&mut self, left: f64, top: f64, right: f64, bottom: f64, color: StrokeColor) {
        let x0 = left.floor().max(0.0).min(self.width as f64) as u32;
        let y0 = top.floor().max(0.0).min(self.height as f64) as u32;
        let x1 = right.ceil().max(0.0).min(self.width as f64) as u32;
        let y1 = bottom.ceil().max(0.0).min(self.height as f64) as u32;
        for y in y0..y1 {
            for x in x0..x1 {
                let offset = (y as usize * self.width as usize + x as usize) * 4;
                blend_pixel(&mut self.rgba[offset..offset + 4], color);
            }
        }
    }

    fn path(&mut self, points: &[ImagePoint], style: StrokeStyle) {
        for pair in points.windows(2) {
            self.line(pair[0], pair[1], style);
        }
    }

    fn arrow(&mut self, start: ImagePoint, end: ImagePoint, style: StrokeStyle) {
        self.line(start, end, style);
        if let Some(wings) = arrowhead_wings(start, end, style.width) {
            self.line(end, wings[0], style);
            self.line(end, wings[1], style);
        }
    }

    fn ellipse(&mut self, x: f64, y: f64, width: f64, height: f64, style: StrokeStyle) {
        let rx = width / 2.0;
        let ry = height / 2.0;
        let circumference_bound = std::f64::consts::TAU * rx.max(ry);
        let steps = (circumference_bound * 2.0).ceil().clamp(24.0, 16_384.0) as usize;
        let center = ImagePoint {
            x: x + rx,
            y: y + ry,
        };
        let points = (0..=steps)
            .map(|step| {
                let angle = std::f64::consts::TAU * step as f64 / steps as f64;
                ImagePoint {
                    x: center.x + rx * angle.cos(),
                    y: center.y + ry * angle.sin(),
                }
            })
            .collect::<Vec<_>>();
        self.path(&points, style);
    }

    fn line(&mut self, start: ImagePoint, end: ImagePoint, style: StrokeStyle) {
        let distance = (end.x - start.x).hypot(end.y - start.y);
        let steps = (distance * 2.0).ceil().clamp(1.0, 65_536.0) as usize;
        for step in 0..=steps {
            let t = step as f64 / steps as f64;
            self.stamp(
                ImagePoint {
                    x: start.x + (end.x - start.x) * t,
                    y: start.y + (end.y - start.y) * t,
                },
                style,
            );
        }
    }

    fn stamp(&mut self, point: ImagePoint, style: StrokeStyle) {
        if !point.x.is_finite()
            || !point.y.is_finite()
            || !style.width.is_finite()
            || style.width <= 0.0
        {
            return;
        }
        let radius = style.width / 2.0;
        let min_x = (point.x - radius - 1.0).floor().max(0.0) as u32;
        let min_y = (point.y - radius - 1.0).floor().max(0.0) as u32;
        let max_x = (point.x + radius + 1.0).ceil().min(self.width as f64) as u32;
        let max_y = (point.y + radius + 1.0).ceil().min(self.height as f64) as u32;
        let radius_squared = radius * radius;
        for y in min_y..max_y {
            for x in min_x..max_x {
                let dx = x as f64 + 0.5 - point.x;
                let dy = y as f64 + 0.5 - point.y;
                if dx * dx + dy * dy <= radius_squared.max(0.25) {
                    let offset = (y as usize * self.width as usize + x as usize) * 4;
                    blend_pixel(&mut self.rgba[offset..offset + 4], style.color);
                }
            }
        }
    }
}

// Compact 5x7 Latin bitmap font; unsupported Unicode glyphs use a visible
// replacement box.
fn glyph_rows(ch: char) -> [u8; 7] {
    match ch {
        'a' => [0, 0, 14, 1, 15, 17, 15],
        'b' => [16, 16, 30, 17, 17, 17, 30],
        'c' => [0, 0, 14, 17, 16, 17, 14],
        'd' => [1, 1, 15, 17, 17, 17, 15],
        'e' => [0, 0, 14, 17, 31, 16, 14],
        'f' => [6, 9, 8, 28, 8, 8, 8],
        'g' => [0, 0, 15, 17, 15, 1, 14],
        'h' => [16, 16, 30, 17, 17, 17, 17],
        'i' => [4, 0, 12, 4, 4, 4, 14],
        'j' => [2, 0, 6, 2, 2, 18, 12],
        'k' => [16, 16, 18, 20, 24, 20, 18],
        'l' => [12, 4, 4, 4, 4, 4, 14],
        'm' => [0, 0, 26, 21, 21, 21, 21],
        'n' => [0, 0, 30, 17, 17, 17, 17],
        'o' => [0, 0, 14, 17, 17, 17, 14],
        'p' => [0, 0, 30, 17, 30, 16, 16],
        'q' => [0, 0, 15, 17, 15, 1, 1],
        'r' => [0, 0, 22, 25, 16, 16, 16],
        's' => [0, 0, 15, 16, 14, 1, 30],
        't' => [8, 8, 28, 8, 8, 9, 6],
        'u' => [0, 0, 17, 17, 17, 19, 13],
        'v' => [0, 0, 17, 17, 17, 10, 4],
        'w' => [0, 0, 17, 17, 21, 21, 10],
        'x' => [0, 0, 17, 10, 4, 10, 17],
        'y' => [0, 0, 17, 17, 15, 1, 14],
        'z' => [0, 0, 31, 2, 4, 8, 31],
        'A' => [14, 17, 17, 31, 17, 17, 17],
        'B' => [30, 17, 17, 30, 17, 17, 30],
        'C' => [14, 17, 16, 16, 16, 17, 14],
        'D' => [30, 17, 17, 17, 17, 17, 30],
        'E' => [31, 16, 16, 30, 16, 16, 31],
        'F' => [31, 16, 16, 30, 16, 16, 16],
        'G' => [14, 17, 16, 23, 17, 17, 15],
        'H' => [17, 17, 17, 31, 17, 17, 17],
        'I' => [14, 4, 4, 4, 4, 4, 14],
        'J' => [7, 2, 2, 2, 18, 18, 12],
        'K' => [17, 18, 20, 24, 20, 18, 17],
        'L' => [16, 16, 16, 16, 16, 16, 31],
        'M' => [17, 27, 21, 21, 17, 17, 17],
        'N' => [17, 25, 21, 19, 17, 17, 17],
        'O' => [14, 17, 17, 17, 17, 17, 14],
        'P' => [30, 17, 17, 30, 16, 16, 16],
        'Q' => [14, 17, 17, 17, 21, 18, 13],
        'R' => [30, 17, 17, 30, 20, 18, 17],
        'S' => [15, 16, 16, 14, 1, 1, 30],
        'T' => [31, 4, 4, 4, 4, 4, 4],
        'U' => [17, 17, 17, 17, 17, 17, 14],
        'V' => [17, 17, 17, 17, 17, 10, 4],
        'W' => [17, 17, 17, 21, 21, 21, 10],
        'X' => [17, 17, 10, 4, 10, 17, 17],
        'Y' => [17, 17, 10, 4, 4, 4, 4],
        'Z' => [31, 1, 2, 4, 8, 16, 31],
        '0' => [14, 17, 19, 21, 25, 17, 14],
        '1' => [4, 12, 4, 4, 4, 4, 14],
        '2' => [14, 17, 1, 2, 4, 8, 31],
        '3' => [30, 1, 1, 14, 1, 1, 30],
        '4' => [2, 6, 10, 18, 31, 2, 2],
        '5' => [31, 16, 16, 30, 1, 1, 30],
        '6' => [14, 16, 16, 30, 17, 17, 14],
        '7' => [31, 1, 2, 4, 8, 8, 8],
        '8' => [14, 17, 17, 14, 17, 17, 14],
        '9' => [14, 17, 17, 15, 1, 1, 14],
        '.' => [0, 0, 0, 0, 0, 12, 12],
        ',' => [0, 0, 0, 0, 4, 4, 8],
        ':' => [0, 12, 12, 0, 12, 12, 0],
        ';' => [0, 12, 12, 0, 4, 4, 8],
        '!' => [4, 4, 4, 4, 4, 0, 4],
        '?' => [14, 17, 1, 2, 4, 0, 4],
        '-' => [0, 0, 0, 31, 0, 0, 0],
        '_' => [0, 0, 0, 0, 0, 0, 31],
        '+' => [0, 4, 4, 31, 4, 4, 0],
        '=' => [0, 31, 0, 31, 0, 0, 0],
        '/' => [1, 2, 2, 4, 8, 8, 16],
        '\\' => [16, 8, 8, 4, 2, 2, 1],
        '(' => [2, 4, 8, 8, 8, 4, 2],
        ')' => [8, 4, 2, 2, 2, 4, 8],
        '\'' => [4, 4, 8, 0, 0, 0, 0],
        '"' => [10, 10, 20, 0, 0, 0, 0],
        ' ' => [0; 7],
        _ => [31, 17, 21, 21, 21, 17, 31],
    }
}

fn blend_pixel(pixel: &mut [u8], color: StrokeColor) {
    blend_over(pixel, &[color.r, color.g, color.b, color.a]);
}

fn blend_over(destination: &mut [u8], source: &[u8]) {
    let source_alpha = source[3] as u32;
    let destination_alpha = destination[3] as u32;
    let inverse_source_alpha = 255 - source_alpha;
    let output_alpha = source_alpha + (destination_alpha * inverse_source_alpha + 127) / 255;
    if output_alpha == 0 {
        return;
    }
    for channel in 0..3 {
        let source_premultiplied = source[channel] as u32 * source_alpha;
        let destination_premultiplied =
            (destination[channel] as u32 * destination_alpha * inverse_source_alpha + 127) / 255;
        let output_premultiplied = source_premultiplied + destination_premultiplied;
        destination[channel] = ((output_premultiplied + output_alpha / 2) / output_alpha) as u8;
    }
    destination[3] = output_alpha.min(255) as u8;
}

#[cfg(test)]
mod tests {
    use super::{arrowhead_wings, composite, glyph_rows, render, render_composite};
    use crate::core::{
        annotations::{
            AnnotationDocument, AnnotationGeometry, ImageBounds, ImagePoint, StrokeColor,
            StrokeStyle, ViewportToImage,
        },
        frame::CapturedFrame,
    };

    fn style() -> StrokeStyle {
        StrokeStyle {
            width: 2.0,
            color: StrokeColor {
                r: 255,
                g: 0,
                b: 0,
                a: 255,
            },
        }
    }

    fn add_shape(document: &mut AnnotationDocument, geometry: AnnotationGeometry) {
        document.add(geometry, style(), 64, 64).unwrap();
    }

    #[test]
    fn renders_each_shape_with_expected_geometry_and_transparent_background() {
        let mut document = AnnotationDocument::default();
        add_shape(
            &mut document,
            AnnotationGeometry::Rectangle {
                bounds: ImageBounds {
                    x: 4.0,
                    y: 5.0,
                    width: 20.0,
                    height: 12.0,
                },
            },
        );
        add_shape(
            &mut document,
            AnnotationGeometry::Triangle {
                bounds: ImageBounds {
                    x: 30.0,
                    y: 4.0,
                    width: 20.0,
                    height: 20.0,
                },
            },
        );
        add_shape(
            &mut document,
            AnnotationGeometry::Ellipse {
                bounds: ImageBounds {
                    x: 5.0,
                    y: 30.0,
                    width: 24.0,
                    height: 12.0,
                },
            },
        );
        add_shape(
            &mut document,
            AnnotationGeometry::Pen {
                points: vec![
                    ImagePoint { x: 35.0, y: 35.0 },
                    ImagePoint { x: 40.0, y: 42.0 },
                    ImagePoint { x: 46.0, y: 34.0 },
                ],
            },
        );
        add_shape(
            &mut document,
            AnnotationGeometry::Arrow {
                start: ImagePoint { x: 35.0, y: 55.0 },
                end: ImagePoint { x: 55.0, y: 55.0 },
            },
        );

        let layer = render(document.annotations(), 64, 64).unwrap();
        assert_eq!((layer.width(), layer.height()), (64, 64));
        let red_at = |x: usize, y: usize| {
            let i = (y * 64 + x) * 4;
            layer.pixels()[i] == 255 && layer.pixels()[i + 1] == 0 && layer.pixels()[i + 3] > 0
        };
        assert!(red_at(4, 10)); // Rectangle left edge.
        assert!(red_at(40, 4)); // Triangle apex.
        assert!(red_at(17, 30)); // Ellipse top edge (non-square bounds).
        assert!(red_at(40, 42)); // Intermediate pen point.
        assert!(red_at(48, 51)); // Arrow head wing, oriented toward right endpoint.
        let transparent = (0..64 * 64)
            .find(|index| layer.pixels()[index * 4 + 3] == 0)
            .unwrap();
        assert_eq!(
            &layer.pixels()[transparent * 4..transparent * 4 + 4],
            &[0, 0, 0, 0]
        );
    }

    #[test]
    fn arrowhead_wings_always_face_back_from_the_endpoint() {
        for (start, end) in [
            (
                ImagePoint { x: 5.0, y: 5.0 },
                ImagePoint { x: 50.0, y: 5.0 },
            ),
            (
                ImagePoint { x: 50.0, y: 50.0 },
                ImagePoint { x: 5.0, y: 50.0 },
            ),
            (
                ImagePoint { x: 5.0, y: 50.0 },
                ImagePoint { x: 5.0, y: 5.0 },
            ),
            (
                ImagePoint { x: 5.0, y: 5.0 },
                ImagePoint { x: 5.0, y: 50.0 },
            ),
            (
                ImagePoint { x: 5.0, y: 5.0 },
                ImagePoint { x: 50.0, y: 50.0 },
            ),
            (
                ImagePoint { x: 50.0, y: 5.0 },
                ImagePoint { x: 5.0, y: 50.0 },
            ),
        ] {
            let wings = arrowhead_wings(start, end, 3.0).unwrap();
            let direction = (end.x - start.x, end.y - start.y);
            for wing in wings {
                let from_tip = (wing.x - end.x, wing.y - end.y);
                assert!(direction.0 * from_tip.0 + direction.1 * from_tip.1 < 0.0);
            }
        }
        assert!(arrowhead_wings(
            ImagePoint { x: 1.0, y: 1.0 },
            ImagePoint { x: 1.0, y: 1.0 },
            3.0,
        )
        .is_none());
    }

    #[test]
    fn same_image_geometry_renders_identically_at_different_viewport_scales() {
        let mut rasters = Vec::new();
        for (viewport_width, viewport_height) in [(320.0, 160.0), (256.0, 128.0), (200.0, 100.0)] {
            let map = ViewportToImage {
                viewport_width,
                viewport_height,
                image_width: 320,
                image_height: 160,
            };
            let start = map
                .map(viewport_width * 0.2, viewport_height * 0.25)
                .unwrap();
            let end = map
                .map(viewport_width * 0.8, viewport_height * 0.75)
                .unwrap();
            let mut document = AnnotationDocument::default();
            document
                .add(AnnotationGeometry::Arrow { start, end }, style(), 320, 160)
                .unwrap();
            rasters.push(render(document.annotations(), 320, 160).unwrap());
        }
        assert_eq!(rasters[0], rasters[1]);
        assert_eq!(rasters[1], rasters[2]);
    }

    #[test]
    fn composite_preserves_source_and_crop_dimensions() {
        let source = CapturedFrame::new(12, 10, vec![24, 48, 72, 255].repeat(120)).unwrap();
        let original = source.clone();
        let mut document = AnnotationDocument::default();
        document
            .add(
                AnnotationGeometry::Rectangle {
                    bounds: ImageBounds {
                        x: 2.0,
                        y: 2.0,
                        width: 6.0,
                        height: 5.0,
                    },
                },
                style(),
                12,
                10,
            )
            .unwrap();
        let layer = render(document.annotations(), 12, 10).unwrap();
        let output = composite(&source, &layer).unwrap();
        assert_eq!(
            (output.width(), output.height()),
            (source.width(), source.height())
        );
        assert_eq!(source, original);
        assert_eq!(&source.pixels()[0..4], &[24, 48, 72, 255]);
        assert_ne!(output.pixels(), source.pixels());
    }

    #[test]
    fn invalid_geometry_is_ignored_and_dimension_mismatch_is_rejected() {
        let invalid = crate::core::annotations::Annotation {
            id: 0,
            order: 0,
            geometry: AnnotationGeometry::Ellipse {
                bounds: ImageBounds {
                    x: 1.0,
                    y: 1.0,
                    width: 0.0,
                    height: 4.0,
                },
            },
            style: style(),
        };
        let blank = render(&[invalid], 8, 8).unwrap();
        assert!(blank.pixels().iter().all(|value| *value == 0));
        assert!(render(&[], 0, 8).is_err());

        let source = CapturedFrame::new(8, 8, vec![0; 8 * 8 * 4]).unwrap();
        let other_size = render(&[], 9, 8).unwrap();
        assert!(composite(&source, &other_size).is_err());
    }

    #[test]
    fn text_renders_visible_colored_pixels_and_clips_at_image_edges() {
        let mut doc = AnnotationDocument::default();
        doc.add(
            AnnotationGeometry::Text {
                position: ImagePoint { x: 0.0, y: 0.0 },
                text: "A".into(),
                font_size: 14.0,
            },
            style(),
            12,
            12,
        )
        .unwrap();
        let layer = render(doc.annotations(), 12, 12).unwrap();
        let painted = layer.pixels().chunks_exact(4).filter(|p| p[3] != 0).count();
        assert!(painted > 8);
        assert!(layer
            .pixels()
            .chunks_exact(4)
            .all(|p| p[3] == 0 || p[0] == 255));
    }

    #[test]
    fn lowercase_text_uses_lowercase_glyphs() {
        assert_ne!(glyph_rows('a'), glyph_rows('A'));
        assert_ne!(glyph_rows('g'), glyph_rows('G'));
    }

    #[test]
    fn mosaic_pixelates_only_each_selected_region_without_mutating_source() {
        let mut pixels = Vec::new();
        for value in 0..16u8 {
            pixels.extend_from_slice(&[value, 0, 0, 255]);
        }
        let source = CapturedFrame::new(4, 4, pixels).unwrap();
        let original = source.clone();
        let mut doc = AnnotationDocument::default();
        for bounds in [
            ImageBounds {
                x: 0.0,
                y: 0.0,
                width: 2.0,
                height: 2.0,
            },
            ImageBounds {
                x: 2.0,
                y: 2.0,
                width: 2.0,
                height: 2.0,
            },
        ] {
            doc.add(
                AnnotationGeometry::Mosaic {
                    bounds,
                    block_size: 2,
                },
                style(),
                4,
                4,
            )
            .unwrap();
        }
        let output = render_composite(&source, doc.annotations()).unwrap();
        assert_eq!(source, original);
        for i in [0usize, 1, 4, 5] {
            assert_eq!(output.pixels()[i * 4], 3);
        }
        for i in [10usize, 11, 14, 15] {
            assert_eq!(output.pixels()[i * 4], 13);
        }
        assert_eq!(output.pixels()[2 * 4], 2);
    }

    #[test]
    fn mosaic_and_vector_annotations_follow_document_painter_order() {
        let source = CapturedFrame::new(4, 4, vec![0, 0, 255, 255].repeat(16)).unwrap();
        let bounds = ImageBounds {
            x: 0.0,
            y: 0.0,
            width: 4.0,
            height: 4.0,
        };
        let mut pen_then_mosaic = AnnotationDocument::default();
        pen_then_mosaic
            .add(
                AnnotationGeometry::Pen {
                    points: vec![ImagePoint { x: 1.5, y: 1.5 }],
                },
                StrokeStyle {
                    width: 3.0,
                    color: StrokeColor::RED,
                },
                4,
                4,
            )
            .unwrap();
        pen_then_mosaic
            .add(
                AnnotationGeometry::Mosaic {
                    bounds,
                    block_size: 4,
                },
                style(),
                4,
                4,
            )
            .unwrap();

        let mut mosaic_then_pen = AnnotationDocument::default();
        mosaic_then_pen
            .add(
                AnnotationGeometry::Mosaic {
                    bounds,
                    block_size: 4,
                },
                style(),
                4,
                4,
            )
            .unwrap();
        mosaic_then_pen
            .add(
                AnnotationGeometry::Pen {
                    points: vec![ImagePoint { x: 1.5, y: 1.5 }],
                },
                StrokeStyle {
                    width: 3.0,
                    color: StrokeColor::RED,
                },
                4,
                4,
            )
            .unwrap();

        let pixelated_after_pen = render_composite(&source, pen_then_mosaic.annotations()).unwrap();
        let pen_after_pixelation =
            render_composite(&source, mosaic_then_pen.annotations()).unwrap();
        let first_pixel = &pixelated_after_pen.pixels()[..4];
        assert!(pixelated_after_pen
            .pixels()
            .chunks_exact(4)
            .all(|pixel| pixel == first_pixel));
        assert!(pen_after_pixelation
            .pixels()
            .chunks_exact(4)
            .any(|pixel| pixel[0] == 255 && pixel[1] == 55 && pixel[2] == 80));
        assert_ne!(pixelated_after_pen, pen_after_pixelation);
    }

    #[test]
    fn small_mosaic_regions_are_safe_and_mixed_output_keeps_text() {
        let source = CapturedFrame::new(3, 3, vec![80, 100, 120, 255].repeat(9)).unwrap();
        let mut doc = AnnotationDocument::default();
        doc.add(
            AnnotationGeometry::Mosaic {
                bounds: ImageBounds {
                    x: 2.0,
                    y: 2.0,
                    width: 1.0,
                    height: 1.0,
                },
                block_size: 12,
            },
            style(),
            3,
            3,
        )
        .unwrap();
        doc.add(
            AnnotationGeometry::Text {
                position: ImagePoint { x: 0.0, y: 0.0 },
                text: "OK".into(),
                font_size: 16.0,
            },
            style(),
            3,
            3,
        )
        .unwrap();
        let output = render_composite(&source, doc.annotations()).unwrap();
        assert_eq!((output.width(), output.height()), (3, 3));
        assert_eq!(output.pixels().len(), 36);
    }

    #[test]
    fn rendered_export_combines_vector_text_and_mosaic_without_changing_source() {
        let source = CapturedFrame::new(32, 24, vec![40, 80, 120, 255].repeat(32 * 24)).unwrap();
        let original = source.clone();
        let mut doc = AnnotationDocument::default();
        add_shape(
            &mut doc,
            AnnotationGeometry::Rectangle {
                bounds: ImageBounds {
                    x: 18.0,
                    y: 8.0,
                    width: 10.0,
                    height: 10.0,
                },
            },
        );
        doc.add(
            AnnotationGeometry::Text {
                position: ImagePoint { x: 1.0, y: 1.0 },
                text: "OK".into(),
                font_size: 14.0,
            },
            style(),
            32,
            24,
        )
        .unwrap();
        doc.add(
            AnnotationGeometry::Mosaic {
                bounds: ImageBounds {
                    x: 4.0,
                    y: 4.0,
                    width: 12.0,
                    height: 12.0,
                },
                block_size: 4,
            },
            style(),
            32,
            24,
        )
        .unwrap();
        let output = render_composite(&source, doc.annotations()).unwrap();
        assert_eq!(source, original);
        assert_eq!((output.width(), output.height()), (32, 24));
        assert_ne!(output.pixels(), source.pixels());
        assert_eq!(&output.pixels()[0..4], &[40, 80, 120, 255]);
    }
}
