# Carambola native Windows app

Carambola captures a screenshot of the monitor under the mouse pointer. Select a
region, annotate it, then copy the image, save a PNG, or scan text with OCR. The
Windows app uses Rust, Slint, Winit, and native Windows APIs. It starts in the
notification area and creates no visible main window at startup.

## Release status and installation

The native v1.1.0 build is a Windows x64 release candidate. The existing public
v1.0.0 GitHub Release contains the older Tauri application. No native installer
or automatic updater is available yet. Do not use the Tauri v1.0.0 updater to
install the native app.

For a portable build, extract the ZIP into a folder and run
`carambola-native.exe`. Keep the executable in a stable location: Windows toast
registration points to its current path. The executable includes the app icon
and UI assets. It does not require WebView2. Windows may warn about its unsigned
publisher. Close Carambola from the tray menu before replacing the executable.

Windows 10 and 11 x64 are the current build target. ARM64 and macOS builds are
not part of this candidate. Other platforms and mixed-DPI display changes have
not completed packaged-app verification.

## Using Carambola

1. Press the default global shortcut, **Ctrl+Shift+S**, or choose **New
   Screenshot** from the tray menu.
2. Drag to select part of the monitor. Move or resize the crop with its edges
   and corners. Use Ctrl while dragging inside it to move the crop when an
   annotation tool is active.
3. Use the floating toolbar for pen, arrow, rectangle, triangle, ellipse,
   text, mosaic, colors, size, undo, redo, clear annotations, Copy, Save, and
   OCR. The clear action preserves the crop.
4. Copy the rendered image with **Ctrl+C**; save to the configured folder with
   **Ctrl+S**; choose a name and location with **Ctrl+Shift+S** while the overlay
   is open. Escape cancels the capture. Text editing keeps focus on its editor
   instead of invoking export shortcuts.

Successful Copy and Save operations close the overlay and can show a Windows
toast with a screenshot preview. OCR scans the selected crop on a worker thread.
If it finds text, it copies the text to the clipboard. A text result, no-text
result, or OCR error closes the overlay and reports the outcome through a native
notification when notifications are enabled. OCR never replaces clipboard
contents for an empty result or recognition failure.

Use **Settings** in the tray menu to choose the save directory, record a capture
shortcut, and enable or disable notifications. Settings are stored at
`%APPDATA%\Carambola\settings.json`. The shortcut may use Ctrl, Shift, Alt, and
supported keys under the active Windows keyboard layout. A conflicting global
shortcut is reported rather than silently substituted.

## OCR and privacy

OCR uses Windows' installed `Windows.Media.Ocr` language packs. Install the
desired language in Windows if OCR reports that no suitable language is
available. Recognition and screenshot processing happen locally. Carambola does
not upload captured images. Copy places image or recognized text on the system
clipboard; Save writes a PNG to the selected local directory. Enabled Windows
notifications may create a temporary local thumbnail that is retained until
the notification is dismissed.

## Build and contribute

Install a current Rust MSVC toolchain and the Windows SDK, then run from
`native-app`:

```powershell
cargo fmt --all -- --check
cargo test --locked --offline
cargo check --locked --offline
cargo build --locked --offline --release
```

The optimized executable is `target\release\carambola-native.exe`. Offline
commands work after Cargo dependencies have been downloaded. Before proposing
changes, run the checks and follow [the Windows manual regression checklist](WINDOWS_MANUAL_REGRESSION.md)
for window, display, tray, and clipboard behavior. Report the Windows version,
display scaling, monitor arrangement, and exact steps for runtime issues.

## Known limitations and license

This candidate has no installer, uninstaller, automatic updater, or code
signature. Some display configurations and the complete packaged capture flow
still need physical Windows verification. The repository has no top-level
project license; public source visibility does not grant a general reuse or
redistribution license. Dependencies retain their own licenses. A project
license decision is needed before describing Carambola as open source.
