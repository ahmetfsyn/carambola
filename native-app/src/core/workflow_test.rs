//! End-to-end core test: logical pointer drag -> physical crop -> source pixels.

use super::{frame::CapturedFrame, geometry::ViewportToFrame, selection::Selection};

#[test]
fn capture_to_crop_preserves_exact_pixels_at_125_percent_dpi() {
    let mut rgba = Vec::new();
    for y in 0..6u8 {
        for x in 0..8u8 {
            rgba.extend_from_slice(&[x, y, x.wrapping_add(y), 255]);
        }
    }
    let frame = CapturedFrame::new(8, 6, rgba).unwrap();
    // At 125% DPI the logical viewport is smaller than the physical capture.
    let small_transform = ViewportToFrame {
        viewport_width: 6.4,
        viewport_height: 4.8,
        scale_factor: 1.25,
        frame_width: 8,
        frame_height: 6,
    };
    let mut small_selection = Selection::default();
    small_selection.begin(2.4, 1.6, 6.4, 4.8);
    small_selection.update(0.8, 0.8, 6.4, 4.8);
    let (small_rect, _) = small_selection.finish(small_transform).unwrap().unwrap();
    let crop = frame.crop(small_rect).unwrap();

    assert_eq!((crop.width(), crop.height()), (2, 1));
    assert_eq!(crop.pixels(), &[1, 1, 2, 255, 2, 1, 3, 255]);
}
