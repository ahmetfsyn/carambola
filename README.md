# Carambola

**Native Windows app:** The Slint-based v1.1.0 release candidate is documented in
[native-app/README.md](native-app/README.md). It is undergoing packaged-app
verification. The existing public v1.0.0 download is the earlier Tauri app;
the instructions below describe that release.

Carambola is a lightweight, tray-first screenshot and annotation application for the desktop. Press a global shortcut, select any part of the screen, mark it up, then copy it to the clipboard or save it as a PNG. It is built with Tauri v2, Rust, React, and TypeScript.

Carambola runs in the background with no main window. A system-tray menu and a configurable global shortcut are the entry points, so the app stays out of the way until it is needed.

Carambola v1.0.0 is a staged production rollout. This release targets Windows x86_64 only. macOS and Linux artifacts will be added to the same GitHub Release only after separate production-readiness testing.

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
- Optional native screenshot notifications, enabled by default, after a screenshot is successfully copied or saved.
- Signed in-app updates for Windows, with a startup check and a manual **Check for Updates** action in Settings.
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
| Windows 10/11 | x86_64 | Initial v1.0.0 production release target. The per-user NSIS installer and signed updater artifact are verified during release. Installer is not Authenticode-signed. |
| macOS | Apple Silicon | Not included in v1.0.0. An earlier `.app` and DMG exist, but production build/runtime verification, Developer ID signing, and notarization remain pending. |
| macOS | Intel | Not included in v1.0.0. Build and runtime verification remain pending. |
| Linux | x86_64 | Not included in v1.0.0. Packaging and runtime behavior have not been verified. Native Wayland capture is not verified. See the Linux section below. |

The Windows 0.1.2 checks cover the production build and installation, plus runtime tests for launch, tray startup, shortcut capture, overlay geometry, crop selection, clipboard copy, and local save. Pen, vector, text, and mosaic annotation tools and multi-monitor behavior still need interactive QA. Linux has not been built or tested here.

## Installation

### v1.0.0 Windows release

Install v1.0.0 manually from the Windows installer on the [Releases page](https://github.com/ahmetfsyn/carambola/releases). Older releases without the updater cannot update themselves. After installing v1.0.0, Carambola checks for signed Windows updates at startup and offers them before installation. Use **Settings → Check for Updates** for a manual check. An update installs only after choosing **Update now**.

The initial workflow builds and publishes only Windows x64. Windows installers are not Authenticode-signed. macOS is not included; its existing ad hoc configuration is not Developer ID signing or notarization. Platform expansion uses a separate workflow that updates this same release and merges its verified entry into `latest.json` while preserving the Windows entry.

### Windows

The Windows x64 installer uses the Tauri NSIS bundler and installs for the current user by default, so administrator rights are normally not required.

1. Download the **Carambola v1.0.0 Windows x64** installer from GitHub Releases.
2. Run the installer and follow the wizard.
3. Launch **Carambola** from the Start Menu. The app starts in the background and adds a tray icon; there is no main window.
4. Right-click the tray icon for **Capture Screen**, **Settings...**, and **Quit Carambola**.

The installer is unsigned; Windows SmartScreen may show an unrecognized-publisher warning. Release notes and QA claims must match the actual v1.0.0 verification results.

**WebView2 requirement:** Carambola uses the Microsoft Edge WebView2 runtime. The installer downloads its bootstrapper if the runtime is missing, so internet access is needed in that case. Offline installation requires WebView2 to be installed separately first.

**Portable executable:** after a local x64 release build, `src-tauri/target/x86_64-pc-windows-msvc/release/carambola.exe` can be run directly without installation. It still requires WebView2. It is not a replacement for the installer: it has no Start Menu entry or uninstaller, and moving the executable changes the path used by Start on Boot.

### macOS

A `.app` and Apple Silicon `.dmg` were built in an earlier checkout. Packaged-app capture, settings, notification delivery, and the v0.1.3 updater still need runtime QA on a Mac.

1. Open the versioned `.dmg` from the [Releases page](https://github.com/ahmetfsyn/carambola/releases) and drag **Carambola** to **Applications**.
2. The app has an ad hoc signature, not a Developer ID signature, and is not notarized. Gatekeeper may block it; use the macOS privacy/security controls only after obtaining the build from a trusted source.
3. Grant **Screen Recording** permission when prompted during a capture, or add Carambola manually under **System Settings → Privacy & Security → Screen & System Audio Recording**. Restart after changing this permission.
4. Launch Carambola; it runs as a menu-bar/tray application without a Dock icon.

Carambola does not capture the desktop at launch. Screen Recording permission is used only after you start a capture. The current global shortcut does not request Accessibility permission. After a successful Copy or Save, macOS requests notification permission if its status has not been decided. If permission was denied, enable Carambola under **System Settings → Notifications**; macOS does not repeatedly show the authorization prompt.

Screen capture may produce blank or failed frames until Screen Recording permission is granted.

### Linux

A Linux build may produce `.deb`, `.rpm`, and AppImage packages with the Tauri bundler, but none of these packages has been built or installed in this checkout.

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

Linux installation and runtime behaviour are **NOT TESTED**.

Native notifications require a working desktop notification service. Linux behavior varies by desktop environment and is **NOT TESTED**.

## Using Carambola

### Start a capture

Carambola starts hidden. Trigger a capture with:

- the global shortcut (see [Keyboard shortcuts](#keyboard-shortcuts)),
- **Capture Screen** in the tray menu.

Each activation freezes a fresh screenshot before showing the overlay. While a capture session is active, further activations are ignored, so you cannot accidentally open two sessions.

On Windows, the monitor under the mouse pointer is captured. On macOS, Carambola chooses the display containing the frontmost ordinary application window, then falls back to the pointer display and primary display. Other platforms use the primary monitor.

On macOS, xcap reports display bounds in logical points while the captured screenshot uses Retina pixels. Carambola positions and sizes its overlay in points and keeps crop dimensions in captured pixels. The native overlay joins all Spaces, joins full-screen Spaces, stays above ordinary windows, and explicitly receives mouse events through transparent regions. These settings address the Retina geometry and click-through causes found in source inspection; full input-isolation behavior still requires interactive runtime verification.

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

### Screenshot Notifications

**Screenshot Notifications** is enabled by default, including for existing configuration files. Change it in Settings; the preference takes effect immediately and persists alongside the shortcut and Start on Boot choices. A native notification appears after Copy or Save succeeds, at most once per capture session. Opening the overlay, cancelling it, or a failed export does not send a notification.

The operating system may ask once for notification permission after the first successful export. If permission is declined, enable Carambola in the system notification settings. On Windows, the Tauri notification plugin works only for installed apps; development uses a different notification identity.

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

This Windows-specific interaction has not been re-tested in this stabilization pass.

The capture shortcut can be changed in **Settings**. Presets are `PrintScreen`, `Cmd/Ctrl+Shift+X`, `Alt+A`, `Cmd/Ctrl+Shift+S`, `F10`, and `Ctrl+Alt+S`; custom combinations can also be recorded. The recorder uses physical `KeyboardEvent.code` values for letters and punctuation so Turkish Q/F layouts do not turn a key into a different shortcut based on its printed character. Command and Control are distinct on macOS. The choice is persisted in the config file (see below).

If the configured capture shortcut is already registered by another application, Carambola keeps `Alt+A` active for that run and shows a macOS alert explaining the fallback. The saved shortcut is left unchanged so it can be replaced in Settings. On all platforms the registration failure is logged.

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

- **First-activation opacity effect (unverified):** the first-versus-subsequent capture comparison was not completed on a permission-enabled macOS session, so the reported effect remains unresolved.
- **Windows 11 PrintScreen interception:** the default `PrintScreen` shortcut can be consumed by the Snipping Tool. See the [Windows 11 note](#keyboard-shortcuts) for workarounds.
- **Shortcut conflicts:** a persisted shortcut that another app owns cannot be registered; startup uses `Alt+A` for that run and macOS explains the fallback. Choose a different combination in Settings.
- **Mosaic strength is fixed:** redaction always uses 32 CSS px blocks; there is no strength slider.
- **OCR is Windows-only.** On macOS and Linux the OCR action reports that OCR is unavailable.
- **No code signing or notarization.** Windows SmartScreen and macOS Gatekeeper warnings are expected.
- **macOS runtime QA is partial.** In DMG builds launched directly from a terminal, the user confirmed foreground content, input isolation, Escape, the updated transition, and a visible system notification after Copy. macOS attributed capture permission for that launch to VS Code; independent app authorization and PNG export remain unverified. Linux is not runtime-verified.
- **macOS shortcut QA is incomplete.** Unit checks cover separate modifiers, ordering, and physical punctuation mapping. Packaged runtime activation while another app is active and shortcut persistence across restarts remain unverified.
- **Multi-monitor behaviour is untested.** Capture targets the monitor under the cursor on Windows. macOS selects the display containing the frontmost ordinary app window, with pointer and primary display fallbacks; display selection and coordinate behavior still need runtime validation.
- **Display scaling:** 100%, 125%, and 150% scaling were not runtime-tested in this stabilization pass.
- **No history, gallery, or cloud sync.**
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

Carambola checks the running process's permission before capture and offers to open the relevant settings when access is unavailable. Other capture errors are reported separately. A switch in System Settings alone does not establish that the current binary is authorized: the QA machine's TCC log showed that a saved grant's code requirement did not match the installed ad hoc build. Quit and relaunch after changing permission; if a rebuilt ad hoc app is not recognized, grant access to that build through System Settings without resetting TCC automatically.

The bundle defaults to an ad hoc signature so its code-signing identifier matches its bundle identifier. Ad hoc builds have a designated requirement tied to their code hash, so a rebuild can invalidate an earlier grant. For consistent development testing, use one installed app location and sign builds with the same suitable development identity; distribution builds require a consistent Developer ID Application identity and notarization. Set `APPLE_SIGNING_IDENTITY` to override the local ad hoc setting. No certificate identity is configured on the QA machine. The current macOS capture uses xcap's CoreGraphics `CGWindowListCreateImage`, which Apple marks obsolete on macOS 15 and later. A user confirmed foreground content in the overlay when the DMG app was launched directly from a terminal; independent app authorization and exported image contents remain unverified.

## Development setup

Prerequisites:

- Node.js 18 or newer and npm.
- Rust stable with Cargo.
- The Tauri v2 system prerequisites for your platform: <https://v2.tauri.app/start/prerequisites/>
- On Windows: Microsoft Edge WebView2 Runtime (normally present on Windows 11) and the MSVC build tools.

Install dependencies and start the development app:

```sh
npm install
npm run tauri -- dev
```

The dev command starts Vite on `http://localhost:1420` and launches the Rust backend with a debug build. The overlay window is hidden until a capture is triggered. Developer tools are available in development builds only.

> **Note:** the `tauri` npm script in `package.json` prepends `%USERPROFILE%\.cargo\bin` to `PATH` using Windows `cmd` syntax, so `npm run tauri ...` only works on Windows. On macOS and Linux use `npx tauri dev` / `npx tauri build` instead, with `cargo` already on your `PATH`.

Run the project checks:

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
npm run tauri -- build --bundles nsis
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

### macOS DMG

On an Apple Silicon Mac, build a drag-to-Applications installer with:

```sh
npx tauri build --bundles dmg
```

The DMG is written under `src-tauri/target/release/bundle/dmg/`. The verified artifact is `Carambola_0.1.0_aarch64.dmg` (5,223,425 bytes; SHA-256 `846a0cc107f75ceaf8e9a761d7e55b53af20499ac34589a3df9d53ac978f1df0`); its `.app` uses `src-tauri/icons/icon.icns` and bundle identifier `io.carambola.desktop`. The DMG checksum was verified and the app was launched from the mounted image. This build is ad hoc signed, not Developer ID signed or notarized, so Gatekeeper may block it. A public release should be signed and notarized before distribution.

Capture-to-overlay latency and memory behavior were not measured. The native overlay input, selection, annotation, export, and notification flows remain unverified.

### Release artifacts and Git

Build outputs live under `src-tauri/target/`, `dist/`, and `node_modules/` and are ignored by Git. Do not commit installers or compiled binaries unless the repository explicitly starts tracking release artifacts.

### Publishing staged platform updates

1. For the initial v1.0.0 release, build and verify Windows x64 only. Tag `v1.0.0` once; do not replace or recreate this tag during platform expansion.
2. After macOS Apple Silicon, macOS Intel, or Linux AppImage passes separate production-readiness testing, run **Actions → Expand v1.0.0 platforms** and select the existing `v1.0.0` tag and one platform.
3. The expansion workflow uploads that platform's actual artifact and signature to the existing release, merges its entry into `latest.json`, confirms the existing Windows entry is unchanged, verifies the new signature, and records SHA-256 hashes.
4. Keep the bundle identifier `io.carambola.desktop` and updater key pair unchanged. Never publish placeholder platform entries or recreate the v1.0.0 tag.

The only required Actions secret is `TAURI_SIGNING_PRIVATE_KEY`; GitHub supplies `GITHUB_TOKEN` automatically. Keep the local private key at `%USERPROFILE%\.tauri\carambola-updater.key`. Do not commit, replace, or delete the private key, change the configured public key, edit signatures by hand, overwrite prior version tags, or delete old release assets. macOS Developer ID signing and notarization are not configured.

## Technology stack

- **Tauri v2** (`tauri` 2.x) with the tray-icon feature.
- **Rust** (edition 2021) backend.
- **React 18** and **TypeScript 5.6** frontend.
- **Vite 6** and **Tailwind CSS 3** for the UI build.
- **xcap** for screen capture, **image** for PNG encoding, **arboard** for the clipboard.
- **tauri-plugin-global-shortcut**, **tauri-plugin-autostart**, **tauri-plugin-single-instance**, and **tauri-plugin-notification**.
- **Windows.Media.Ocr** (via the `windows` crate) for Windows OCR only.

## License

This repository does not contain a `LICENSE` file. **A license needs to be chosen before Carambola is distributed publicly.** Until a license is added, the default copyright rules apply and no permission is granted to use, copy, modify, or redistribute the project. The project author must select and add a license; this document does not choose one on their behalf.
