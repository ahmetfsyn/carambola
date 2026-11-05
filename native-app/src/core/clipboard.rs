//! Windows CF_DIBV5 pixel-buffer conversion, kept independent of Win32 handles.

use super::frame::CapturedFrame;

const BITMAPV5HEADER_SIZE: usize = 124;
const BI_BITFIELDS: u32 = 3;
const LCS_SRGB: u32 = 0x7352_4742;
const LCS_GM_IMAGES: u32 = 4;

/// Build a top-down 32-bit BGRA DIBV5 block from the final RGBA screenshot.
pub fn dib_v5_bytes(frame: &CapturedFrame) -> Result<Vec<u8>, String> {
    let width = i32::try_from(frame.width()).map_err(|_| "Image width exceeds DIB limits")?;
    let height = i32::try_from(frame.height()).map_err(|_| "Image height exceeds DIB limits")?;
    let pixel_bytes = (frame.width() as usize)
        .checked_mul(frame.height() as usize)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or("Clipboard image size overflow")?;
    let pixel_bytes_u32 =
        u32::try_from(pixel_bytes).map_err(|_| "Clipboard image exceeds DIB limits")?;
    let total_size = BITMAPV5HEADER_SIZE
        .checked_add(pixel_bytes)
        .ok_or("Clipboard DIB size overflow")?;

    let mut dib = Vec::with_capacity(total_size);
    push_u32(&mut dib, BITMAPV5HEADER_SIZE as u32);
    push_i32(&mut dib, width);
    push_i32(&mut dib, -height); // Negative height means top-down rows.
    push_u16(&mut dib, 1); // Planes.
    push_u16(&mut dib, 32); // Bits per pixel.
    push_u32(&mut dib, BI_BITFIELDS);
    push_u32(&mut dib, pixel_bytes_u32);
    push_i32(&mut dib, 0); // Horizontal pixels per meter.
    push_i32(&mut dib, 0); // Vertical pixels per meter.
    push_u32(&mut dib, 0); // Colors used.
    push_u32(&mut dib, 0); // Important colors.
    push_u32(&mut dib, 0x00ff_0000); // Red mask.
    push_u32(&mut dib, 0x0000_ff00); // Green mask.
    push_u32(&mut dib, 0x0000_00ff); // Blue mask.
    push_u32(&mut dib, 0xff00_0000); // Alpha mask.
    push_u32(&mut dib, LCS_SRGB);
    dib.extend_from_slice(&[0; 36]); // CIEXYZ endpoints.
    push_u32(&mut dib, 0); // Red gamma.
    push_u32(&mut dib, 0); // Green gamma.
    push_u32(&mut dib, 0); // Blue gamma.
    push_u32(&mut dib, LCS_GM_IMAGES);
    push_u32(&mut dib, 0); // Profile data offset.
    push_u32(&mut dib, 0); // Profile size.
    push_u32(&mut dib, 0); // Reserved.
    debug_assert_eq!(dib.len(), BITMAPV5HEADER_SIZE);

    for pixel in frame.pixels().chunks_exact(4) {
        dib.extend_from_slice(&[pixel[2], pixel[1], pixel[0], pixel[3]]);
    }
    debug_assert_eq!(dib.len(), total_size);
    Ok(dib)
}

fn push_u16(output: &mut Vec<u8>, value: u16) {
    output.extend_from_slice(&value.to_le_bytes());
}

fn push_u32(output: &mut Vec<u8>, value: u32) {
    output.extend_from_slice(&value.to_le_bytes());
}

fn push_i32(output: &mut Vec<u8>, value: i32) {
    output.extend_from_slice(&value.to_le_bytes());
}

#[cfg(test)]
mod tests {
    use super::{dib_v5_bytes, BITMAPV5HEADER_SIZE, BI_BITFIELDS, LCS_SRGB};
    use crate::core::frame::CapturedFrame;

    fn read_u32(bytes: &[u8], offset: usize) -> u32 {
        u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
    }

    #[test]
    fn dib_v5_buffer_has_valid_header_orientation_and_bgra_pixels() {
        let frame = CapturedFrame::new(2, 1, vec![1, 2, 3, 77, 10, 20, 30, 255]).unwrap();
        let dib = dib_v5_bytes(&frame).unwrap();
        assert_eq!(dib.len(), BITMAPV5HEADER_SIZE + 8);
        assert_eq!(read_u32(&dib, 0), BITMAPV5HEADER_SIZE as u32);
        assert_eq!(read_u32(&dib, 4), 2);
        assert_eq!(i32::from_le_bytes(dib[8..12].try_into().unwrap()), -1);
        assert_eq!(u16::from_le_bytes(dib[12..14].try_into().unwrap()), 1);
        assert_eq!(u16::from_le_bytes(dib[14..16].try_into().unwrap()), 32);
        assert_eq!(read_u32(&dib, 16), BI_BITFIELDS);
        assert_eq!(read_u32(&dib, 20), 8);
        assert_eq!(read_u32(&dib, 40), 0x00ff_0000);
        assert_eq!(read_u32(&dib, 44), 0x0000_ff00);
        assert_eq!(read_u32(&dib, 48), 0x0000_00ff);
        assert_eq!(read_u32(&dib, 52), 0xff00_0000);
        assert_eq!(read_u32(&dib, 56), LCS_SRGB);
        assert_eq!(&dib[BITMAPV5HEADER_SIZE..], &[3, 2, 1, 77, 30, 20, 10, 255]);
    }
}
