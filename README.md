# Carambola

Carambola is a lightweight Windows screenshot tool. Capture a monitor, select a
region, annotate it, then copy the result or save it as a PNG. The current
desktop application is written in Rust and Slint and runs from the Windows
notification area.

## Download and run

Download the latest **Windows x64** release from
[GitHub Releases](https://github.com/ahmetfsyn/carambola/releases/latest). Releases
provide a standalone executable, a portable ZIP, and `SHA256SUMS.txt`.

For the portable ZIP, extract it to a folder you control and run
`carambola-native.exe`. Keep the folder in a stable location: Windows toast
registration uses the executable's current path. The standalone download has
the same native executable. There is no installer, uninstaller, or automatic
updater. The executable is not Authenticode-signed, so Windows may show a
publisher warning.

The supported release target is Windows 10 or 11, x64. macOS and Linux source
adapters are experimental and are not supported release targets. The app does
not require WebView2.

## Features

- Start a capture with the global shortcut or the notification-area menu.
- Capture the monitor under the pointer and select, move, or resize a crop.
- Annotate with a pen, arrow, rectangle, triangle, ellipse, text, and mosaic
  redaction; choose colors and stroke widths, and undo or redo edits.
- Copy the rendered crop as an image or save it as a PNG.
- Run OCR on a background worker using Windows' installed OCR language packs.
- Configure the capture shortcut, save folder, and notification preference in
  Settings.
- Show optional Windows notifications after successful image export and OCR.

## Use Carambola

1. Launch Carambola. It starts in the notification area without a main window.
2. Press **Ctrl+Shift+S** or choose **New Screenshot** from the tray menu.
3. Drag to select an area. Use the floating tools to annotate or redact it.
4. Choose **Copy** or **Save**, or press **Escape** to cancel.

While the capture overlay is open, **Ctrl+C** copies the image, **Ctrl+S** saves
it to the configured folder, and **Ctrl+Shift+S** opens Save As. Text editing
keeps keyboard focus on the editor.

### OCR behavior

OCR runs in the background; it does not show a centered busy popup. A nonempty
recognized result is copied to the clipboard. Empty results and errors leave
the existing clipboard contents unchanged. Every OCR outcome closes the
overlay, then reports the result through a native notification when
notifications are enabled.

Install the desired language pack through Windows language settings if OCR
cannot find a suitable language. Recognition uses Windows.Media.Ocr and does
not bundle a model.

### Settings and files

Settings are stored at `%APPDATA%\Carambola\settings.json` (with a per-user
Roaming-folder fallback if `APPDATA` is unavailable). The default save folder
is the user's Pictures directory. Save As writes to the path you select.

Captures and OCR are processed locally. Carambola does not upload images or
recognized text. Copy places data on the shared system clipboard. Enabled
notification previews are temporary local PNG files retained while Windows may
display the toast; toast setup also creates a per-user Start Menu shortcut.
Optional window diagnostics can record window titles, paths, bounds, and native
handles when explicitly enabled with a diagnostics build or environment
variable. Do not enable them when those local details should not be logged.

## Build and validate

Use Windows with a stable Rust MSVC toolchain and the Windows SDK. From
`native-app` run:

```powershell
cargo fmt --all -- --check
cargo clippy --locked --all-targets
cargo test --locked --all-targets
cargo check --locked
cargo build --locked --release
.\tools\Test-ReleaseMetadata.ps1
```

The executable is `target\release\carambola-native.exe`. Cargo downloads are
needed the first time; add `--offline` after dependencies are present locally.
The release workflow runs the same checks, builds into a fresh runner directory,
and validates the release tag against the Cargo package version before it
creates a release.

The native app implementation is under `native-app/src`, and the Slint overlay
is in `native-app/ui/overlay.slint`. See
[native-app/WINDOWS_MANUAL_REGRESSION.md](native-app/WINDOWS_MANUAL_REGRESSION.md)
for hardware-dependent checks. Automated tests do not replace physical Windows
verification.

## Technology

- Rust, Cargo, and Slint for the application and interface.
- Winit for the Windows event loop and window integration.
- Windows APIs for display capture, clipboard access, dialogs, OCR, notifications,
  tray operations, and global shortcuts.
- GitHub Actions for Windows checks, release packaging, checksums, and website
  deployment.

## Contributing

Keep changes scoped to the native application or its independent marketing
website. Before a pull request, run formatting, Clippy, and all native Rust
tests. Include the Windows version, display scaling, monitor arrangement, and
reproduction steps for Windows-specific issues. Keep screenshots, OCR output,
and diagnostic logs out of commits unless they are intentionally reviewed test
fixtures.

## Limitations and license

Only Windows x64 is a supported release target. No installer, updater, code
signature, or other platform release is provided. Display layouts, OCR language
availability, and notification behavior depend on the Windows environment.
Some display and tray lifecycle cases still require manual verification.

There is no top-level `LICENSE` file. No particular license or reuse right is
implied by the public repository. Dependencies retain their own licenses.
