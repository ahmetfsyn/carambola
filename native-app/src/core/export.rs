//! PNG encoding and file output for rendered screenshot frames.

use image::ImageEncoder;
use std::path::{Path, PathBuf};

use super::{atomic_file, frame::CapturedFrame};

pub fn encode_png(frame: &CapturedFrame) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    image::codecs::png::PngEncoder::new(&mut bytes)
        .write_image(
            frame.pixels(),
            frame.width(),
            frame.height(),
            image::ExtendedColorType::Rgba8,
        )
        .map_err(|error| format!("Could not encode screenshot as PNG: {error}"))?;
    Ok(bytes)
}

pub fn write_png(path: &Path, frame: &CapturedFrame) -> Result<(), String> {
    let bytes = encode_png(frame)?;
    atomic_file::write(path, &bytes)
        .map_err(|error| format!("Could not write PNG at {}: {error}", path.display()))
}

pub fn ensure_png_extension(mut path: PathBuf) -> Result<PathBuf, String> {
    match path.extension() {
        Some(extension) if extension.eq_ignore_ascii_case("png") => Ok(path),
        Some(_) => Err("Choose a filename ending in .png".into()),
        None => {
            path.set_extension("png");
            Ok(path)
        }
    }
}

pub fn default_screenshot_path(
    directory: &Path,
    unix_nanos: u128,
    process_id: u32,
    sequence: u64,
) -> PathBuf {
    directory.join(format!(
        "Carambola Screenshot-{unix_nanos}-{process_id}-{sequence}.png"
    ))
}

#[cfg(test)]
mod tests {
    use super::{encode_png, ensure_png_extension, write_png};
    use crate::core::{
        annotation_render,
        annotations::{
            AnnotationDocument, AnnotationGeometry, ImagePoint, StrokeColor, StrokeStyle,
        },
        frame::CapturedFrame,
    };
    use std::{path::PathBuf, time::SystemTime};

    fn scratch_dir() -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "carambola-native-export-{}-{nonce}",
            std::process::id()
        ))
    }

    #[test]
    fn png_round_trip_preserves_dimensions_and_rgba_pixels() {
        let frame = CapturedFrame::new(2, 1, vec![1, 2, 3, 255, 40, 50, 60, 255]).unwrap();
        let encoded = encode_png(&frame).unwrap();
        let decoded = image::load_from_memory(&encoded).unwrap().to_rgba8();
        assert_eq!(decoded.dimensions(), (2, 1));
        assert_eq!(decoded.into_raw(), frame.pixels());
    }

    #[test]
    fn png_file_write_reports_invalid_parent_directory() {
        let path = scratch_dir().join("missing").join("image.png");
        let frame = CapturedFrame::new(1, 1, vec![1, 2, 3, 255]).unwrap();
        let error = write_png(&path, &frame).unwrap_err();
        assert!(error.contains("Could not write PNG"));
    }

    #[test]
    fn png_write_returns_after_the_complete_file_is_readable() {
        let directory = scratch_dir();
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("image.png");
        let frame = CapturedFrame::new(2, 1, vec![1, 2, 3, 255, 40, 50, 60, 255]).unwrap();

        write_png(&path, &frame).unwrap();

        let decoded = image::open(&path).unwrap().to_rgba8();
        assert_eq!(decoded.dimensions(), (2, 1));
        assert_eq!(decoded.into_raw(), frame.pixels());
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn rendered_png_contains_final_annotation_pixels() {
        let source = CapturedFrame::new(4, 4, vec![0, 0, 255, 255].repeat(16)).unwrap();
        let mut document = AnnotationDocument::default();
        document
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
        let rendered =
            annotation_render::render_composite(&source, document.annotations()).unwrap();
        let encoded = encode_png(&rendered).unwrap();
        let decoded = image::load_from_memory(&encoded).unwrap().to_rgba8();
        assert_eq!(decoded.dimensions(), (4, 4));
        assert!(decoded
            .pixels()
            .any(|pixel| pixel[0] == 255 && pixel[1] == 55 && pixel[2] == 80));
        assert_eq!(source.pixels()[..4], [0, 0, 255, 255]);
    }

    #[test]
    fn save_path_uses_png_extension() {
        assert_eq!(
            ensure_png_extension(PathBuf::from("screenshot")).unwrap(),
            PathBuf::from("screenshot.png")
        );
        assert!(ensure_png_extension(PathBuf::from("screenshot.jpg")).is_err());
        assert_eq!(
            ensure_png_extension(PathBuf::from("screenshot.PNG")).unwrap(),
            PathBuf::from("screenshot.PNG")
        );
    }

    #[test]
    fn default_save_path_is_in_the_configured_directory_and_unique_per_sequence() {
        let directory = PathBuf::from("screenshots");
        let first = super::default_screenshot_path(&directory, 123, 456, 1);
        let second = super::default_screenshot_path(&directory, 123, 456, 2);
        assert_eq!(first.parent(), Some(directory.as_path()));
        assert_eq!(
            first.extension().and_then(|extension| extension.to_str()),
            Some("png")
        );
        assert_ne!(first, second);
        assert!(first
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("Carambola Screenshot-"));
    }
}
