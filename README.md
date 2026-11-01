# Carambola

Carambola is a lightweight, tray-first screenshot and annotation application for the desktop. Press a global shortcut, select any part of the screen, mark it up, then copy it to the clipboard or save it as a PNG. It is built with Tauri v2, Rust, React, and TypeScript.

Carambola runs in the background with no main window. A system-tray menu and a configurable global shortcut are the entry points, so the app stays out of the way until it is needed.

## Features

- Global shortcut to start a new capture session.
- Frozen full-screen overlay with desktop dimming and a resizable selection.
- Selection can be moved or resized with edge/corner handles.
- Annotation tools: freehand pen, arrow, rectangle, triangle, circle, text, and pixelation redaction.
- Eight annotation colours and stroke widths from 1 px to 20 px.
- Undo and redo for annotations.
- Copy the annotated selection to the clipboard as a PNG.
- Save the annotated selection as a PNG under the user's Pictures folder.
- OCR on Windows (Windows.Media.Ocr); extracted text is copied to the clipboard.
- Settings screen for the capture shortcut and Start on Boot.
- Single-instance behaviour: launching Carambola again while it is running only brings up the existing instance (it does not create a second tray icon).
- Tray-only background operation on Windows, macOS, and Linux.

Carambola does not upload screenshots anywhere. Captures stay on the local machine unless the user copies or saves them.

## Screenshots

Real screenshots are not checked into this repository yet. The placeholders below describe the intended images so they can be added later without changing the documentation structure.

| Screen | Placeholder |
| --- | --- |
| Overlay with a selection and the floating toolbox | `docs/screenshots/overlay-selection.png` — not included yet |
| Annotation toolbox with colours and stroke width controls | `docs/screenshots/toolbox.png` — not included yet |
| Settings screen (shortcut and Start on Boot) | `docs/screenshots/settings.png` — not included yet |
| Tray menu | `docs/screenshots/tray-menu.png` — not included yet |

## Supported platforms

| Platform | Architecture | Status |
| --- | --- | --- |
| Windows 10/11 | x86_64 | Built and runtime-tested on Windows 11 x64 in this repository's QA pass. Single 1920×1080 monitor at 100% display scaling. A production NSIS installer is produced. |
| macOS | Apple Silicon / Intel | Code paths are present, but macOS has **not** been runtime-tested in this QA pass. No signing or notarization is configured. Screen Recording permission is required. |
| Linux | x86_64 | Code paths are present, but Linux has **not** been runtime-tested in this QA pass. Screen capture uses X11 APIs; native Wayland capture is not verified. See the Linux section below. |

Do not assume a platform is supported merely because it appears in this table. Only the Windows x86_64 row was exercised at runtime for this release.

## Installation

### Windows

The Windows build uses the Tauri NSIS bundler and installs for the current user by default, so administrator rights are normally not required.

1. Obtain `Carambola_<version>_x64-setup.exe` from the repository's Releases page when a release is published, or build it locally (see [Build instructions](#build-instructions)).
2. Run the installer and follow the wizard.
3. Launch **Carambola** from the Start Menu. The app starts in the background and adds a tray icon; there is no main window.
4. Right-click the tray icon for **Capture Screen**, **Settings...**, and **Quit Carambola**.

The installer is not code-signed. Windows SmartScreen may show a warning the first time it is run; use **More info → Run anyway** only if you obtained the file from a source you trust.

**WebView2 requirement:** Carambola uses the Microsoft Edge WebView2 runtime. Windows 11 and most up-to-date Windows 10 systems already have it. If it is missing, the Tauri installer's bootstrapper downloads and installs it, which requires an internet connection during installation. On an offline machine, install the Evergreen WebView2 Runtime from Microsoft first, then run the Carambola installer.

**Portable executable:** after a local release build, `src-tauri/target/release/carambola.exe` can be run directly without installation. It still requires WebView2. It is not a replacement for the installer: it has no Start Menu entry or uninstaller, and moving the executable changes the path used by Start on Boot.

### macOS

A `.app`/`.dmg` can be produced by running the Tauri build on macOS, but no macOS package is included or verified here.

1. Open the `.dmg` and drag **Carambola** to **Applications**.
2. The app is not signed or notarized. On first launch, macOS may block it; open it with **Control-click → Open**, or allow it in **System Settings → Privacy & Security**.
3. Grant **Screen Recording** permission when prompted, or add Carambola manually under **System Settings → Privacy & Security → Screen Recording**. macOS requires the app to be restarted after this permission is granted.
4. Launch Carambola; it runs as a menu-bar/tray application without a Dock icon.

Screen capture will produce blank or failed frames until Screen Recording permission is granted.

### Linux

A Linux build can produce `.deb`, `.rpm`, and AppImage packages with the Tauri bundler, but none of these packages was built or installed during this QA pass.

Build on the target Linux machine, then use `install.sh` at the repository root:

```sh
npm run tauri build
bash install.sh
```

`install.sh` only installs a package that already exists under `src-tauri/target/release/bundle/`. It does not download anything. It detects `.deb`, `.rpm`, or AppImage output and uses the system package manager where possible. If no package is found, it exits with an explanatory message. Installation via `.deb`/`.rpm` normally requires `sudo`.

Runtime requirements for the Linux build:

- GTK 3 (`libgtk-3-0` on Debian/Ubuntu, `gtk3` on Fedora).
- WebKitGTK 4.1 (`libwebkit2gtk-4.1-0` / `webkit2gtk4.1`).
- An AppIndicator-compatible tray host (`libayatana-appindicator3-1` or `libappindicator3-1`). GNOME users may need the AppIndicator extension.
- A working X11 display for screen capture. On Wayland, run under XWayland or use an X11 session; native Wayland capture is not verified and may fail or return a black frame.
- A global-shortcut backend. The app uses X11 global shortcuts; on Wayland through XWayland this may work, otherwise bind the shortcut in the desktop environment's keyboard settings or use an X11 session.

Linux installation and runtime behaviour are **NOT TESTED** in this repository's QA pass.

## Using Carambola

### Start a capture

Carambola starts hidden. Trigger a capture with:

- the global shortcut (see [Keyboard shortcuts](#keyboard-shortcuts)),
- **Capture Screen** in the tray menu.

Each activation freezes a fresh screenshot before showing the overlay. While a capture session is active, further activations are ignored, so you cannot accidentally open two sessions.

On Windows, the monitor under the mouse pointer is captured. On other platforms the primary monitor is used.

### Select an area

1. Drag anywhere on the dimmed desktop to create a selection.
2. Drag the selection's interior to move it.
3. Drag an edge or corner handle to resize it.
4. The floating toolbox appears next to the selection and is clamped to stay inside the screen.

Press **Escape** to cancel the session and return Carambola to the tray. Escape also cancels the session if the Settings screen is open; Settings will not reappear during the next capture.

### Annotate

The toolbox provides:

- **Select** — create, move, and resize the selection.
- **Pen** — freehand drawing.
- **Arrow** — drag from the tail to the head; reverse dragging works too.
- **Shape** — click the button repeatedly to cycle Rectangle → Triangle → Circle, then drag to draw.
- **Text** — click to place a text cursor, type, and press Enter to commit.
- **Mosaic** — drag over an area to apply destructive pixelation (redaction).
- **OCR** — Windows only; extracts text from the selection and copies it to the clipboard.
- **Undo / Redo** — step through annotation history.
- **Copy** — copy the composited selection to the clipboard.
- **Save** — save the composited selection as a PNG.
- **Settings** — open shortcut and Start on Boot settings.
- **Close** — cancel the session.

Annotation colours: Black, White, Red, Orange, Yellow, Green, Blue, Purple. Stroke widths: 1–20 px.

The **Mosaic** tool pixelates the exported pixels; it does not merely draw a translucent shape. The original detail under the mosaic is destroyed in the copied/saved image. Mosaic block size is fixed at 32 CSS pixels; small selections may therefore show coarse blocks. Always inspect a sensitive capture before sharing it.

### Copy and save

- **Copy:** press **Enter** or **Ctrl/Cmd+C**, or click the copy button. The composite (screenshot + annotations, cropped to the selection) is written to the clipboard as PNG. The overlay closes shortly afterwards.
- **Save:** press **Ctrl/Cmd+S**, or click the save button. The PNG is written to:
  - Windows: `%USERPROFILE%\Pictures\Carambola\Carambola_YYYY-MM-DD_HHMMSS.png`
  - macOS/Linux: the system Pictures directory, in a `Carambola` subfolder.
- **Cancel:** press **Escape** or click Close. Nothing is copied or saved.

If the clipboard is unavailable or the Pictures folder cannot be written, the error is logged and no success feedback is shown; the overlay remains usable so the operation can be retried.

### Start on Boot

On first run, Start on Boot is enabled by default. Change it in **Settings → Start on Boot** (tray → **Settings...**). The toggle reflects the real OS registration:

- Windows: `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`, value `Carambola`, with `--background`.
- macOS: a LaunchAgent (`tauri-plugin-autostart`).
- Linux: an XDG autostart `.desktop` entry.

Startup launches the app in the background; no overlay is shown until the shortcut or tray action is used. The choice is stored in the config file and survives restarts.

### Quit

Choose **Quit Carambola** from the tray menu. This removes the tray icon and stops the background process. Closing an overlay with Escape does not quit the app.

## Keyboard shortcuts

| Action | Default |
| --- | --- |
| Start a capture | Windows/Linux: `PrintScreen` · macOS: `Cmd+Shift+X` (`CommandOrControl+Shift+X`) |
| Copy selection | `Enter` or `Ctrl/Cmd+C` |
| Save selection | `Ctrl/Cmd+S` |
| Undo | `Ctrl/Cmd+Z` |
| Redo | `Ctrl/Cmd+Shift+Z` or `Ctrl+Y` |
| Cancel capture | `Escape` |

**Windows 11 note:** Windows 11 routes `PrintScreen` to the Snipping Tool by default, using a low-level keyboard hook that runs before application hotkeys. When that option is enabled, registering `PrintScreen` succeeds but the key never reaches Carambola, so pressing it appears to do nothing. Either:

- turn off **Settings → Accessibility → Keyboard → Use the Print Screen key to open Snipping Tool**, or
- choose a different shortcut in Carambola Settings (for example `F10`, `Ctrl+Alt+S`, or `Alt+F10`).

This was reproduced on Windows 11 during QA. `Alt+F10` was verified to open the overlay through the real global shortcut.

The capture shortcut can be changed in **Settings**. Presets are `PrintScreen`, `Cmd/Ctrl+Shift+X`, `Alt+A`, `Cmd/Ctrl+Shift+S`, `F10`, and `Ctrl+Alt+S`; custom combinations can also be recorded. The choice is persisted in the config file (see below).

If the configured capture shortcut is already registered by another application, Carambola falls back to `Alt+A` at startup and only writes a warning to its log; the Settings screen does not currently surface that fallback. Choose a different combination in that case.

When changing the shortcut in Settings, Carambola tries to register the new combination before releasing the old one. If the new combination is unavailable, an error is shown and the previous shortcut keeps working.

### Config file locations

- Windows: `%APPDATA%\carambola\config.json`
- macOS: `~/Library/Application Support/carambola/config.json`
- Linux: `~/.config/carambola/config.json`

Example:

```json
{
  "shortcut": "PrintScreen",
  "autostart": true
}
```

Edit the file only while Carambola is not running. Invalid JSON is ignored and defaults are used.

## Known limitations

- **First-activation opacity effect (unfixed):** in development mode, the very first capture after launching shows the overlay fading in over roughly 200 ms instead of appearing instantly. Subsequent captures are immediate. Root cause was isolated to the Windows DWM window-open animation on the overlay's first show; no fix has been applied yet. See the QA investigation notes for the proposed fix.
- **Windows 11 PrintScreen interception:** the default `PrintScreen` shortcut can be consumed by the Snipping Tool. See the [Windows 11 note](#keyboard-shortcuts) for workarounds.
- **Shortcut conflicts:** a persisted shortcut that another app owns is not registered; the app falls back to `Alt+A` and does not warn in the UI.
- **Mosaic strength is fixed:** redaction always uses 32 CSS px blocks; there is no strength slider.
- **OCR is Windows-only.** On macOS and Linux the OCR action reports that OCR is unavailable.
- **No code signing or notarization.** Windows SmartScreen and macOS Gatekeeper warnings are expected.
- **macOS and Linux are not runtime-verified** for this release. Do not treat them as tested.
- **Multi-monitor behaviour is untested.** Windows selects the monitor under the cursor; other platforms use the primary monitor.
- **Display scaling:** only 100% scaling was exercised in this QA pass. 125%/150% scaling and mixed-DPI multi-monitor setups were not re-tested.
- **No auto-update, history, gallery, or cloud sync.**
- **No license has been selected.** See [License](#license).

## Linux X11 / Wayland requirements

- Screen capture uses X11 APIs. Native Wayland sessions may not allow the required screen access; use an X11 session or XWayland.
- Global shortcuts use X11 hotkey registration. On Wayland, the compositor may intercept or reject the key; bind the capture action in the desktop's keyboard settings, or use an X11 session.
- The tray icon requires an AppIndicator/StatusNotifier host. GNOME needs the AppIndicator extension; KDE and most other desktops include support.
- Clipboard image transfer uses `arboard` with Wayland data-control support when built with the Linux feature; behaviour on native Wayland is not verified.
- OCR is unavailable on Linux.
- No Linux package has been built or installed during this QA pass.

## macOS screen-recording permission

macOS blocks screen capture until the app is authorised:

1. Open **System Settings → Privacy & Security → Screen Recording**.
2. Enable **Carambola**.
3. Quit and relaunch Carambola.

Without this permission, capture returns no usable frame; Carambola keeps the overlay hidden rather than showing an empty one. The macOS build is unsigned and unnotarized, so Gatekeeper may also block the first launch. These macOS behaviours were not runtime-tested in this QA pass.

## Development setup

Prerequisites:

- Node.js 18 or newer (Vite 5 requirement) and npm.
- Rust stable with Cargo.
- The Tauri v2 system prerequisites for your platform: <https://v2.tauri.app/start/prerequisites/>
- On Windows: Microsoft Edge WebView2 Runtime (normally present on Windows 11) and the MSVC build tools.

Install dependencies and start the development app:

```sh
npm install
npm run tauri dev
```

The dev command starts Vite on `http://localhost:1420` and launches the Rust backend with a debug build. The overlay window is hidden until a capture is triggered. Developer tools are available in development builds only.

> **Note:** the `tauri` npm script in `package.json` prepends `%USERPROFILE%\.cargo\bin` to `PATH` using Windows `cmd` syntax, so `npm run tauri ...` only works on Windows. On macOS and Linux use `npx tauri dev` / `npx tauri build` instead, with `cargo` already on your `PATH`.

Run the checks used for this QA pass:

```sh
npm run build
cargo check --all-targets --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml
```

There is no separate ESLint or frontend unit-test configuration in this repository; `npm run build` runs TypeScript type-checking followed by the Vite production build.

## Build instructions

### Windows installer (NSIS)

From the repository root, with the Rust toolchain and npm dependencies installed:

```sh
npm run tauri build -- --bundles nsis
```

Outputs:

- Portable executable: `src-tauri/target/release/carambola.exe`
- Installer: `src-tauri/target/release/bundle/nsis/Carambola_<version>_x64-setup.exe`

The installer is configured for a per-user install (`currentUser`) with a Start Menu folder named `Carambola`. It includes the normal Tauri uninstaller. The build is not code-signed.

### Other platforms

On macOS and Linux, run the Tauri CLI directly from the repository root:

```sh
npx tauri build
```

Tauri's platform bundler decides which package formats are available:

- macOS: `.app` and `.dmg`.
- Linux: `.deb`, `.rpm`, and AppImage, subject to the installed tooling.

Cross-compiling a production bundle is not supported by this project's instructions; build on the target OS.

### Release artifacts and Git

Build outputs live under `src-tauri/target/`, `dist/`, and `node_modules/` and are ignored by Git. Do not commit installers or compiled binaries unless the repository explicitly starts tracking release artifacts.

## Technology stack

- **Tauri v2** (`tauri` 2.x) with the tray-icon feature.
- **Rust** (edition 2021) backend.
- **React 18** and **TypeScript 5.6** frontend.
- **Vite 5** and **Tailwind CSS 3** for the UI build.
- **xcap** for screen capture, **image** for PNG encoding, **arboard** for the clipboard.
- **tauri-plugin-global-shortcut**, **tauri-plugin-autostart**, and **tauri-plugin-single-instance**.
- **Windows.Media.Ocr** (via the `windows` crate) for Windows OCR only.

## License

This repository does not contain a `LICENSE` file. **A license needs to be chosen before Carambola is distributed publicly.** Until a license is added, the default copyright rules apply and no permission is granted to use, copy, modify, or redistribute the project. The project author must select and add a license; this document does not choose one on their behalf.
