# Carambola

Carambola is a lightweight Windows screenshot tool. Capture a monitor, select a
region, annotate it, then copy the result or save it as a PNG. The current
desktop application is written in Rust and Slint and runs from the Windows
notification area.

## Download and run

Download the latest **Windows x64** release from
[GitHub Releases](https://github.com/ahmetfsyn/carambola/releases/latest). Releases
provide `Carambola-Setup-1.2.exe`, a portable ZIP, a standalone executable, and
`SHA256SUMS.txt`.

The per-user installer places Carambola under
`%LOCALAPPDATA%\Programs\Carambola`, adds a Start Menu shortcut, and offers an
optional desktop shortcut. Close Carambola before upgrading or uninstalling;
the installer will not terminate it automatically. Settings and diagnostics
under `%APPDATA%\Carambola` are retained by upgrades and uninstall.

For the portable ZIP, extract it to a folder you control and run
`Carambola.exe`. Keep the folder in a stable location: Windows toast
registration uses the executable's current path. The standalone download
contains the same application. The standalone executable and installer are not
Authenticode-signed, so Windows may show a publisher warning.

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
- Manually check for updates from Settings in an installed copy of Carambola.
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

Cargo builds `target\release\carambola.exe`; distribution packages present it
as `Carambola.exe`. Cargo stores a three-component technical version such as
`1.2.0` because its package metadata and Windows numeric version resource need
that form. The public Carambola version is the first two components (`1.2`),
and the matching release tag is `v1.2`. The patch component must stay zero;
increment the public minor version for each new release. Cargo downloads are
needed the first time; add `--offline` after dependencies are present locally.

The release workflow runs the same checks, builds into a fresh runner directory,
and validates the two-part public tag against the Cargo package version before
it creates a release. It uses a pinned Inno Setup command-line compiler to add
the per-user installer alongside the standalone EXE, portable ZIP, and SHA-256
checksums. To build the installer locally, run this from the repository root:

```powershell
.\scripts\build-windows-installer.ps1
```

The script builds the Windows release executable when needed. Install Inno
Setup 7 from its official download page if `ISCC.exe` is not found; the script
does not download or run third-party installers.

An installed copy can check GitHub Releases from **Settings → Check updates**.
The updater considers only newer stable two-part versions with the current
Windows x64 EXE, portable ZIP, installer, and checksum manifest; legacy tag
formats and incomplete or prerelease releases are skipped. It downloads over
HTTPS, verifies the installer SHA-256 against `SHA256SUMS.txt`, and asks before
downloading. A small handoff waits for Carambola to exit before opening the
normal per-user Setup program, which retains `%APPDATA%\Carambola`. Updates are
manual and never install silently. Portable copies do not run the installer
updater; replace their files using the portable release download.

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
- GitHub Actions for Windows checks, release packaging, and checksums.
- Cloudflare Workers for the independently deployed marketing website.

## Contributing

Keep changes scoped to the native application or its independent marketing
website. Before a pull request, run formatting, Clippy, and all native Rust
tests. Include the Windows version, display scaling, monitor arrangement, and
reproduction steps for Windows-specific issues. Keep screenshots, OCR output,
and diagnostic logs out of commits unless they are intentionally reviewed test
fixtures.

## Limitations and license

Only Windows x64 is a supported release target. There is no Authenticode code
signature, and no other platform release is provided. Display layouts, OCR language
availability, and notification behavior depend on the Windows environment.
Some display and tray lifecycle cases still require manual verification.

There is no top-level `LICENSE` file. No particular license or reuse right is
implied by the public repository. Dependencies retain their own licenses.
