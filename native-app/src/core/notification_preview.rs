//! Small physical-pixel previews used by native screenshot notifications.

use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use super::{export::encode_png, frame::CapturedFrame};

const HERO_WIDTH: u32 = 364;
const HERO_HEIGHT: u32 = 180;
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub fn thumbnail(frame: &CapturedFrame) -> Result<CapturedFrame, String> {
    let source = image::RgbaImage::from_raw(frame.width(), frame.height(), frame.pixels().to_vec())
        .ok_or_else(|| {
            "Could not read screenshot pixels for the notification preview".to_owned()
        })?;
    let scale = (f64::from(HERO_WIDTH) / f64::from(frame.width()))
        .min(f64::from(HERO_HEIGHT) / f64::from(frame.height()));
    let fitted_width = (f64::from(frame.width()) * scale)
        .round()
        .clamp(1.0, f64::from(HERO_WIDTH)) as u32;
    let fitted_height = (f64::from(frame.height()) * scale)
        .round()
        .clamp(1.0, f64::from(HERO_HEIGHT)) as u32;
    let fitted = image::imageops::resize(
        &source,
        fitted_width,
        fitted_height,
        image::imageops::FilterType::Lanczos3,
    );
    let mut canvas =
        image::RgbaImage::from_pixel(HERO_WIDTH, HERO_HEIGHT, image::Rgba([15, 23, 42, 255]));
    let offset_x = (HERO_WIDTH - fitted.width()) / 2;
    let offset_y = (HERO_HEIGHT - fitted.height()) / 2;
    image::imageops::overlay(
        &mut canvas,
        &fitted,
        i64::from(offset_x),
        i64::from(offset_y),
    );
    CapturedFrame::new(HERO_WIDTH, HERO_HEIGHT, canvas.into_raw()).map_err(str::to_owned)
}

pub fn encode_thumbnail_png(frame: &CapturedFrame) -> Result<Vec<u8>, String> {
    encode_png(&thumbnail(frame)?)
}

pub struct TempPreviewFile {
    path: PathBuf,
}

impl TempPreviewFile {
    pub fn create(frame: &CapturedFrame) -> Result<Self, String> {
        let directory = std::env::temp_dir().join("Carambola").join("ToastPreviews");
        fs::create_dir_all(&directory).map_err(|error| {
            format!("Could not create the screenshot notification preview directory: {error}")
        })?;
        let bytes = encode_thumbnail_png(frame)?;
        for _ in 0..8 {
            let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            let path = directory.join(format!(
                "carambola-toast-{}-{nanos}-{sequence}.png",
                std::process::id()
            ));
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(mut file) => {
                    if let Err(error) = file.write_all(&bytes).and_then(|()| file.sync_all()) {
                        let _ = fs::remove_file(&path);
                        return Err(format!(
                            "Could not write screenshot notification preview: {error}"
                        ));
                    }
                    return Ok(Self { path });
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => {
                    return Err(format!(
                        "Could not create screenshot notification preview: {error}"
                    ));
                }
            }
        }
        Err("Could not allocate a unique screenshot notification preview file".into())
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempPreviewFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

/// Return an absolute `file:///` URI suitable for a desktop toast image.
pub fn local_file_uri(path: &Path) -> Result<String, String> {
    if !path.is_absolute() {
        return Err("Notification preview path must be absolute".into());
    }
    let normalized = path.to_string_lossy().replace('\\', "/");
    let encoded = normalized
        .bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric()
                || matches!(byte, b'/' | b':' | b'-' | b'.' | b'_' | b'~')
            {
                (byte as char).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect::<String>();
    if encoded.starts_with('/') {
        Ok(format!("file://{encoded}"))
    } else {
        Ok(format!("file:///{encoded}"))
    }
}

#[cfg(test)]
mod tests {
    use super::{encode_thumbnail_png, local_file_uri, thumbnail, TempPreviewFile};
    use crate::core::frame::CapturedFrame;
    use std::path::Path;

    #[test]
    fn notification_thumbnail_fits_the_windows_hero_size_without_cropping() {
        let frame = CapturedFrame::new(2, 1, vec![255, 0, 0, 255, 0, 0, 255, 255]).unwrap();
        let image = thumbnail(&frame).unwrap();
        assert_eq!((image.width(), image.height()), (364, 180));
        assert_eq!(image.pixels()[0..4], [15, 23, 42, 255]);
        assert!(image
            .pixels()
            .chunks_exact(4)
            .any(|pixel| pixel == [255, 0, 0, 255]));
        assert!(image
            .pixels()
            .chunks_exact(4)
            .any(|pixel| pixel == [0, 0, 255, 255]));
        assert!(!encode_thumbnail_png(&frame).unwrap().is_empty());
    }

    #[test]
    fn preview_file_is_removed_when_its_lifetime_guard_is_dropped() {
        let frame = CapturedFrame::new(1, 1, vec![255, 0, 0, 255]).unwrap();
        let file = TempPreviewFile::create(&frame).unwrap();
        let path = file.path().to_owned();
        assert!(path.is_file());
        drop(file);
        assert!(!path.exists());
    }

    #[test]
    fn file_uri_escapes_spaces_and_accepts_only_absolute_paths() {
        #[cfg(windows)]
        let path = Path::new(r"C:\Users\Test User\preview.png");
        #[cfg(not(windows))]
        let path = Path::new("/tmp/Test User/preview.png");
        let uri = local_file_uri(path).unwrap();
        assert!(uri.starts_with("file:///"));
        assert!(uri.contains("Test%20User"));
        assert!(local_file_uri(Path::new("relative.png")).is_err());
    }
}
