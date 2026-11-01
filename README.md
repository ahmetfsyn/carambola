# Carambola

Carambola is a lightweight desktop screenshot and annotation app. Start a capture from the system tray or a keyboard shortcut, select an area, annotate it, then copy or save the result as a PNG.

Carambola runs in the background and has no main window. It is built with Tauri, Rust, React, and TypeScript.

## Key features

- Capture a screen and select, move, or resize a region.
- Annotate with a pen, arrows, shapes, text, and pixelation mosaic.
- Undo and redo annotations; copy or save the finished image.
- Extract text with OCR on Windows.
- Configure the capture shortcut, Start on Boot, and screenshot notifications.
- Use a native system tray menu to capture, open settings, or quit.

## Screenshots

No verified application screenshots are available yet.

## Platform status

| Platform | Current status |
| --- | --- |
| macOS, Apple Silicon | A release app and DMG were built in project QA. Packaged capture, annotation, settings, export, notification, and permission flows are not fully verified. |
| macOS, Intel | Not tested. |
| Windows 10/11, x64 | Build target is configured; runtime QA and installer generation have not been verified. |
| Linux, x64 | Code paths are present; build, packaging, and runtime QA have not been verified. Screen capture and global shortcuts use X11 APIs; native Wayland behavior is not verified. |

These statuses describe available project evidence, not a guarantee of support on every system or configuration. Carambola is not signed or notarized for distribution.

## Installation

The first release provides a macOS Apple Silicon build only. Download `Carambola_0.1.0_aarch64.dmg` from the [v0.1.0 release](https://github.com/ahmetfsyn/carambola/releases/tag/v0.1.0), open it, and drag Carambola to Applications. Windows and Linux installers are not included.

On macOS, screen capture requires Screen Recording permission. Allow Carambola under **System Settings → Privacy & Security → Screen & System Audio Recording**, then restart the app. Permission behavior can vary for locally built, ad hoc signed copies.

On Linux, the capture and shortcut implementations use X11. Native Wayland capture is not verified and may not work; use an X11 session or XWayland.

## Basic usage

Carambola starts in the background. Start a capture with the system tray menu or the configured keyboard shortcut. Drag across the dimmed desktop to select an area. Drag inside the selection to move it, or use its handles to resize it.

The annotation toolbar provides:

- **Select**, **Pen**, **Arrow**, **Shape** (rectangle, triangle, or circle), **Text**, and **Mosaic**.
- **Undo** and **Redo**.
- **OCR** on Windows.
- **Copy**, **Save**, **Settings**, and **Close**.

Mosaic pixelates the exported image and replaces the original pixels in that region. Its block size is fixed at 32 CSS pixels, so inspect a redacted capture before sharing it.

### Keyboard shortcuts

| Action | Shortcut |
| --- | --- |
| Start capture | Windows/Linux: `PrintScreen`; macOS: `Cmd+Shift+X` |
| Copy selection | `Enter` or `Ctrl/Cmd+C` |
| Save selection | `Ctrl/Cmd+S` |
| Undo / redo | `Ctrl/Cmd+Z` / `Ctrl/Cmd+Shift+Z` (`Ctrl+Y` also redoes) |
| Cancel capture | `Escape` |

The capture shortcut can be changed in Settings. If another app owns the configured shortcut, Carambola tries `Alt+A` for that run. On Windows 11, the Snipping Tool may intercept `PrintScreen`; disable its Print Screen setting or choose another shortcut in Carambola.

Saved images go to the system Pictures folder in a `Carambola` subfolder, with a timestamped PNG filename. Copy and Save include the selected screenshot and its annotations.

## Privacy

Screen captures and annotations are processed locally. Carambola has no screenshot upload or cloud-sync feature. Copy sends the result to the system clipboard; Save writes it to the local Pictures folder. OCR text is copied to the clipboard on Windows. The app may use operating-system services for screen capture, startup, shortcuts, clipboard access, and notifications.

## Development

### Prerequisites

- Node.js 18 or newer and npm.
- Rust stable and Cargo.
- The [Tauri v2 prerequisites](https://v2.tauri.app/start/prerequisites/) for your operating system.
- Windows: Microsoft Edge WebView2 Runtime and the MSVC build tools.

Install dependencies and start the development app:

```sh
npm install
npx tauri dev
```

Build for the current operating system:

```sh
npx tauri build
```

On Windows, an NSIS installer can be built with:

```sh
npm run tauri -- build --bundles nsis
```

Build on the target operating system; cross-compiling production bundles is not covered by this project. The `npm run tauri` script uses Windows `cmd` syntax, so use `npx tauri` on macOS and Linux.

The frontend check and production build are run together with:

```sh
npm run build
```

The Rust backend can be checked and tested with:

```sh
cargo check --all-targets --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml
```

## Known limitations

- Windows runtime behavior, the Windows installer, and Linux runtime/package behavior have not been verified in this checkout.
- macOS Intel, multi-monitor behavior, and display scaling have not been tested.
- Linux capture and global shortcuts use X11; native Wayland behavior is not verified.
- OCR is available only on Windows.
- Capture latency and memory use have not been measured.
- The app is not code-signed or notarized for distribution.

## License

No license has been specified for this repository.
