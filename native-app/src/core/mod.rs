pub mod annotation_render;
pub mod annotations;
pub mod atomic_file;
pub mod clipboard;
pub mod display;
pub mod export;
pub mod frame;
pub mod geometry;
pub mod hotkey;
pub mod lifecycle;
pub mod notification_preview;
pub mod ocr;
pub mod selection;
pub mod session;
pub mod settings;
#[cfg(target_os = "windows")]
pub mod updater;
#[cfg(test)]
mod workflow_test;
