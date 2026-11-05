//! Immutable screenshot pixels and crop operations in physical frame pixels.

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
        })
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
        Self::new(rect.width, rect.height, output)
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
