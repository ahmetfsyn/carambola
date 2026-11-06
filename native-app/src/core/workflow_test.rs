//! End-to-end core test: logical pointer drag -> physical crop -> source pixels.

use super::{
    display::{DisplayMetadata, PhysicalBounds, PhysicalPoint},
    frame::CapturedFrame,
    geometry::{FrameViewport, ViewportToFrame},
    selection::Selection,
};

#[test]
fn capture_to_crop_preserves_exact_pixels_at_125_percent_dpi() {
    let mut rgba = Vec::new();
    for y in 0..6u8 {
        for x in 0..8u8 {
            rgba.extend_from_slice(&[x, y, x.wrapping_add(y), 255]);
        }
    }
    let frame = CapturedFrame::new(8, 6, rgba)
        .unwrap()
        .with_display_metadata(
            DisplayMetadata::new(
                "DISPLAY2".into(),
                PhysicalBounds::new(-4, -3, 4, 3).unwrap(),
                120,
                false,
            )
            .unwrap(),
        )
        .unwrap();
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
    assert_eq!(crop.desktop_origin(), Some(PhysicalPoint { x: -3, y: -2 }));
}

#[test]
fn dpi_change_keeps_physical_capture_metadata_and_crops_exact_pixels() {
    let mut rgba = Vec::new();
    for y in 0..6u8 {
        for x in 0..8u8 {
            rgba.extend_from_slice(&[x, y, x.wrapping_add(y), 255]);
        }
    }
    let display = DisplayMetadata::new(
        "DISPLAY_LEFT".into(),
        PhysicalBounds::new(-8, -6, 0, 0).unwrap(),
        96,
        false,
    )
    .unwrap();
    let frame = CapturedFrame::new(8, 6, rgba)
        .unwrap()
        .with_display_metadata(display.clone())
        .unwrap();
    let original_dimensions = (frame.width(), frame.height());
    let original_pixels = frame.pixels().to_vec();
    let mut viewport = FrameViewport::new(frame.width(), frame.height(), 1.0).unwrap();

    // The captured display metadata remains at 100% while the Overlay's
    // effective DPI changes, as can happen after a monitor configuration
    // transition. Only the logical viewport follows the event scale.
    let old_viewport = viewport.logical_size();
    viewport.synchronize_scale_factor(1.5).unwrap();
    assert!(viewport
        .transform_for(old_viewport.width, old_viewport.height)
        .is_err());
    let logical_viewport = viewport.logical_size();
    let transform = viewport
        .transform_for(logical_viewport.width, logical_viewport.height)
        .unwrap();

    let expected_pixels = crate::core::frame::PixelRect {
        x: 2,
        y: 1,
        width: 3,
        height: 2,
    };
    let mut selection = Selection::default();
    selection.begin(
        f64::from(expected_pixels.x) * logical_viewport.width / f64::from(frame.width()),
        f64::from(expected_pixels.y) * logical_viewport.height / f64::from(frame.height()),
        logical_viewport.width,
        logical_viewport.height,
    );
    selection.update(
        f64::from(expected_pixels.x + expected_pixels.width) * logical_viewport.width
            / f64::from(frame.width()),
        f64::from(expected_pixels.y + expected_pixels.height) * logical_viewport.height
            / f64::from(frame.height()),
        logical_viewport.width,
        logical_viewport.height,
    );
    let (selected_pixels, selected_bounds) = selection.finish(transform).unwrap().unwrap();
    assert_eq!(selected_pixels, expected_pixels);
    assert_eq!(
        viewport.project_pixels(selected_pixels).unwrap(),
        selected_bounds
    );

    let crop = frame.crop(selected_pixels).unwrap();
    assert_eq!((crop.width(), crop.height()), (3, 2));
    assert_eq!(
        crop.pixels(),
        &[2, 1, 3, 255, 3, 1, 4, 255, 4, 1, 5, 255, 2, 2, 4, 255, 3, 2, 5, 255, 4, 2, 6, 255,]
    );
    assert_eq!((frame.width(), frame.height()), original_dimensions);
    assert_eq!(frame.pixels(), original_pixels);
    assert_eq!(frame.display_metadata(), Some(&display));
    assert_eq!(crop.desktop_origin(), Some(PhysicalPoint { x: -6, y: -5 }));
}

#[test]
fn mixed_display_scale_transitions_do_not_rewrite_capture_display_metadata() {
    let captured_display = DisplayMetadata::new(
        "DISPLAY1".into(),
        PhysicalBounds::new(0, 0, 2400, 1350).unwrap(),
        120,
        true,
    )
    .unwrap();
    let other_display = DisplayMetadata::new(
        "DISPLAY2".into(),
        PhysicalBounds::new(2400, 0, 4800, 1350).unwrap(),
        144,
        false,
    )
    .unwrap();
    let frame = CapturedFrame::new(2400, 1350, vec![17; 2400 * 1350 * 4])
        .unwrap()
        .with_display_metadata(captured_display.clone())
        .unwrap();
    let mut viewport = FrameViewport::new(
        frame.width(),
        frame.height(),
        captured_display.scale_factor(),
    )
    .unwrap();

    for scale_factor in [
        other_display.scale_factor(),
        1.0,
        captured_display.scale_factor(),
    ] {
        let logical = viewport.synchronize_scale_factor(scale_factor).unwrap();
        assert_eq!(logical.width, f64::from(frame.width()) / scale_factor);
        assert_eq!(logical.height, f64::from(frame.height()) / scale_factor);
        assert!(viewport
            .transform_for(logical.width, logical.height)
            .is_ok());
        assert_eq!(frame.display_metadata(), Some(&captured_display));
    }
    assert_eq!((frame.width(), frame.height()), (2400, 1350));
}
