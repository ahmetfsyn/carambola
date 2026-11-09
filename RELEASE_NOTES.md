# Carambola Native v1.1.1

This patch release makes the native Rust and Slint Windows application the
single active desktop product and establishes a native-only tagged release
workflow. The existing v1.1.0 release and tag are unchanged.

## Included

- Standalone Windows x64 executable and portable ZIP.
- Windows file and product version metadata generated from the Cargo package
  version.
- Release tag validation, automated checks, uploaded-asset integrity checks,
  and SHA-256 checksums.
- Disabled unused Winit file-drop targets in the native app.
- Updated native-first project documentation and website download status.

## Requirements and limitations

- Windows 10/11 x64. OCR requires a suitable installed Windows OCR language.
- No installer, uninstaller, automatic updater, or Authenticode signature.
- macOS and Linux are not supported release targets.
- The repository has no top-level project license file.
