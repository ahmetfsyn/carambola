//! Immutable screenshot pixels and crop operations in physical frame pixels.

use super::display::{DisplayMetadata, PhysicalPoint};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PixelRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// A captured monitor image. Pixel dimensions are physical pixels and RGBA is
/// tightly packed, matching the capture/export boundary used by the old app.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CapturedFrame {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
    display: Option<FrameDisplay>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct FrameDisplay {
    metadata: DisplayMetadata,
    origin: PhysicalPoint,
}

impl CapturedFrame {
    pub fn new(width: u32, height: u32, rgba: Vec<u8>) -> Result<Self, &'static str> {
        if width == 0 || height == 0 {
            return Err("frame dimensions must be positive");
        }
        let expected = (width as usize)
            .checked_mul(height as usize)
            .and_then(|pixels| pixels.checked_mul(4))
            .ok_or("frame dimensions overflow addressable memory")?;
        if rgba.len() != expected {
            return Err("RGBA buffer length does not match frame dimensions");
        }
        Ok(Self {
            width,
            height,
            rgba,
            display: None,
        })
    }

    /// Associate a full-monitor frame with the display and physical desktop
    /// origin that produced it.
    pub fn with_display_metadata(
        mut self,
        metadata: DisplayMetadata,
    ) -> Result<Self, &'static str> {
        if self.width != metadata.bounds.width() || self.height != metadata.bounds.height() {
            return Err("frame dimensions must match the selected display bounds");
        }
        self.display = Some(FrameDisplay {
            origin: PhysicalPoint {
                x: metadata.bounds.left,
                y: metadata.bounds.top,
            },
            metadata,
        });
        Ok(self)
    }

    pub fn display_metadata(&self) -> Option<&DisplayMetadata> {
        self.display.as_ref().map(|display| &display.metadata)
    }

    pub fn desktop_origin(&self) -> Option<PhysicalPoint> {
        self.display.as_ref().map(|display| display.origin)
    }

    /// Map a frame-local pixel edge to the signed desktop coordinate space.
    pub fn local_to_desktop(&self, x: u32, y: u32) -> Option<PhysicalPoint> {
        let display = self.display.as_ref()?;
        if x > self.width || y > self.height {
            return None;
        }
        let monitor_x =
            u32::try_from(i64::from(display.origin.x) - i64::from(display.metadata.bounds.left))
                .ok()?
                .checked_add(x)?;
        let monitor_y =
            u32::try_from(i64::from(display.origin.y) - i64::from(display.metadata.bounds.top))
                .ok()?
                .checked_add(y)?;
        display.metadata.local_to_desktop(monitor_x, monitor_y)
    }

    /// Convert a tightly packed 32-bit GDI DIB (BGRA byte order) into the
    /// opaque RGBA frame used by Slint and the crop/export path.
    pub fn from_bgra32(width: u32, height: u32, mut bgra: Vec<u8>) -> Result<Self, &'static str> {
        if width == 0 || height == 0 {
            return Err("frame dimensions must be positive");
        }
        let expected = (width as usize)
            .checked_mul(height as usize)
            .and_then(|pixels| pixels.checked_mul(4))
            .ok_or("frame dimensions overflow addressable memory")?;
        if bgra.len() != expected {
            return Err("BGRA buffer length does not match frame dimensions");
        }
        for pixel in bgra.chunks_exact_mut(4) {
            pixel.swap(0, 2);
            pixel[3] = 255;
        }
        Self::new(width, height, bgra)
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn pixels(&self) -> &[u8] {
        &self.rgba
    }

    /// Copy a non-empty physical-pixel crop from the original capture.
    pub fn crop(&self, rect: PixelRect) -> Result<Self, &'static str> {
        let right = rect
            .x
            .checked_add(rect.width)
            .ok_or("crop bounds overflow")?;
        let bottom = rect
            .y
            .checked_add(rect.height)
            .ok_or("crop bounds overflow")?;
        if rect.width == 0 || rect.height == 0 || right > self.width || bottom > self.height {
            return Err("crop is empty or outside the captured frame");
        }

        let row_bytes = rect.width as usize * 4;
        let mut output = Vec::with_capacity(row_bytes * rect.height as usize);
        for y in rect.y..bottom {
            let start = (y as usize * self.width as usize + rect.x as usize) * 4;
            output.extend_from_slice(&self.rgba[start..start + row_bytes]);
        }
        let mut crop = Self::new(rect.width, rect.height, output)?;
        if let Some(display) = &self.display {
            let crop_x = i32::try_from(rect.x).map_err(|_| "crop origin exceeds desktop limits")?;
            let crop_y = i32::try_from(rect.y).map_err(|_| "crop origin exceeds desktop limits")?;
            crop.display = Some(FrameDisplay {
                metadata: display.metadata.clone(),
                origin: PhysicalPoint {
                    x: display
                        .origin
                        .x
                        .checked_add(crop_x)
                        .ok_or("crop desktop X origin overflow")?,
                    y: display
                        .origin
                        .y
                        .checked_add(crop_y)
                        .ok_or("crop desktop Y origin overflow")?,
                },
            });
        }
        Ok(crop)
    }

    /// Pixelate a redaction rectangle by averaging each block from the
    /// unmodified source frame, then filling that block with the average color.
    /// Sampling from the immutable source prevents earlier blocks from
    /// affecting later samples.
    pub fn mosaic(&mut self, rect: PixelRect, block_size: u32) -> Result<(), &'static str> {
        let right = rect
            .x
            .checked_add(rect.width)
            .ok_or("mosaic bounds overflow")?;
        let bottom = rect
            .y
            .checked_add(rect.height)
            .ok_or("mosaic bounds overflow")?;
        if rect.width == 0 || rect.height == 0 || right > self.width || bottom > self.height {
            return Err("mosaic is empty or outside the captured frame");
        }
        if block_size == 0 {
            return Err("mosaic block size must be positive");
        }

        let source = self.rgba.clone();
        let mut y = rect.y;
        while y < bottom {
            let block_bottom = y.saturating_add(block_size).min(bottom);
            let mut x = rect.x;
            while x < right {
                let block_right = x.saturating_add(block_size).min(right);
                let mut sums = [0u64; 3];
                let mut count = 0u64;
                for sy in y..block_bottom {
                    for sx in x..block_right {
                        let offset = (sy as usize * self.width as usize + sx as usize) * 4;
                        for (channel, sum) in sums.iter_mut().enumerate() {
                            *sum += source[offset + channel] as u64;
                        }
                        count += 1;
                    }
                }
                let color = sums.map(|sum| (sum / count) as u8);
                for dy in y..block_bottom {
                    for dx in x..block_right {
                        let offset = (dy as usize * self.width as usize + dx as usize) * 4;
                        self.rgba[offset..offset + 3].copy_from_slice(&color);
                    }
                }
                x = block_right;
            }
            y = block_bottom;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{CapturedFrame, PixelRect};

    #[test]
    fn rejects_invalid_frame_dimensions_and_buffer_lengths() {
        assert!(CapturedFrame::new(0, 1, vec![]).is_err());
        assert!(CapturedFrame::new(1, 1, vec![0; 3]).is_err());
    }

    #[test]
    fn associates_monitor_metadata_and_maps_frame_pixels_to_desktop() {
        use crate::core::display::{DisplayMetadata, PhysicalBounds, PhysicalPoint};

        let metadata = DisplayMetadata::new(
            "DISPLAY2".into(),
            PhysicalBounds::new(-2, 3, 2, 6).unwrap(),
            144,
            false,
        )
        .unwrap();
        let frame = CapturedFrame::new(4, 3, vec![0; 4 * 3 * 4])
            .unwrap()
            .with_display_metadata(metadata.clone())
            .unwrap();

        assert_eq!(frame.display_metadata(), Some(&metadata));
        assert_eq!(frame.desktop_origin(), Some(PhysicalPoint { x: -2, y: 3 }));
        assert_eq!(
            frame.local_to_desktop(3, 2),
            Some(PhysicalPoint { x: 1, y: 5 })
        );
        let mismatched = DisplayMetadata::new(
            "DISPLAY2".into(),
            PhysicalBounds::new(0, 0, 8, 3).unwrap(),
            96,
            false,
        )
        .unwrap();
        assert!(CapturedFrame::new(4, 3, vec![0; 4 * 3 * 4])
            .unwrap()
            .with_display_metadata(mismatched)
            .is_err());
    }

    #[test]
    fn monitor_metadata_survives_crop_and_crop_origin_tracks_desktop_pixels() {
        use crate::core::display::{DisplayMetadata, PhysicalBounds, PhysicalPoint};

        let metadata = DisplayMetadata::new(
            "DISPLAY2".into(),
            PhysicalBounds::new(-4, -3, 4, 3).unwrap(),
            120,
            false,
        )
        .unwrap();
        let frame = CapturedFrame::new(8, 6, vec![0; 8 * 6 * 4])
            .unwrap()
            .with_display_metadata(metadata.clone())
            .unwrap();
        let crop = frame
            .crop(PixelRect {
                x: 2,
                y: 1,
                width: 3,
                height: 2,
            })
            .unwrap();

        assert_eq!(crop.display_metadata(), Some(&metadata));
        assert_eq!(crop.desktop_origin(), Some(PhysicalPoint { x: -2, y: -2 }));
        assert_eq!(
            crop.local_to_desktop(1, 1),
            Some(PhysicalPoint { x: -1, y: -1 })
        );
    }

    #[test]
    fn crops_physical_rgba_pixels_without_resampling() {
        let pixels = (0..16).collect::<Vec<u8>>();
        let frame = CapturedFrame::new(2, 2, pixels).unwrap();
        let crop = frame
            .crop(PixelRect {
                x: 1,
                y: 0,
                width: 1,
                height: 2,
            })
            .unwrap();
        assert_eq!((crop.width(), crop.height()), (1, 2));
        assert_eq!(crop.pixels(), &[4, 5, 6, 7, 12, 13, 14, 15]);
    }

    #[test]
    fn converts_bgra_channel_order_and_makes_alpha_opaque() {
        let frame = CapturedFrame::from_bgra32(2, 1, vec![3, 2, 1, 0, 30, 20, 10, 7]).unwrap();
        assert_eq!(frame.pixels(), &[1, 2, 3, 255, 10, 20, 30, 255]);
    }

    #[test]
    fn mosaic_averages_each_block_from_the_original_pixels() {
        let mut frame = CapturedFrame::new(2, 1, vec![0, 20, 40, 255, 100, 120, 140, 255]).unwrap();
        frame
            .mosaic(
                PixelRect {
                    x: 0,
                    y: 0,
                    width: 2,
                    height: 1,
                },
                2,
            )
            .unwrap();
        assert_eq!(frame.pixels(), &[50, 70, 90, 255, 50, 70, 90, 255]);
    }

    #[test]
    fn rejects_crop_and_mosaic_outside_frame() {
        let mut frame = CapturedFrame::new(2, 2, vec![0; 16]).unwrap();
        let outside = PixelRect {
            x: 1,
            y: 1,
            width: 2,
            height: 1,
        };
        assert!(frame.crop(outside).is_err());
        assert!(frame.mosaic(outside, 4).is_err());
        assert!(frame
            .mosaic(
                PixelRect {
                    x: 0,
                    y: 0,
                    width: 1,
                    height: 1
                },
                0
            )
            .is_err());
    }
}
