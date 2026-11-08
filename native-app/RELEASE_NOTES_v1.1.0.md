# Carambola native v1.1.0 — release candidate

This is the first portable Windows x64 build of the Rust and Slint application.
It is separate from the existing Tauri v1.0.0 release.

## Included

- Monitor-under-pointer capture, crop movement and eight-edge/corner resizing.
- Pen, arrow, rectangle, triangle, ellipse, text, and mosaic annotation tools.
- Undo, redo, clear annotations, image Copy, PNG Save, and Save As.
- Windows on-device OCR with text clipboard output and result notifications.
- Configurable global capture shortcut, tray controls, Settings persistence,
  notification preference, and screenshot-preview Windows toasts.
- One native process per Windows session; a second launch exits without adding
  another tray icon or competing for the global hotkey.
- Existing Carambola icon and v1.1.0 metadata embedded in the executable.

## Requirements and limitations

- Windows 10/11 x64. OCR requires a suitable installed Windows OCR language.
- Portable EXE and ZIP only; no native installer or automatic updater.
- Executable is not Authenticode-signed. WebView2 is not needed.
- Complete physical regression testing on multi-monitor and mixed-DPI setups,
  and interactive verification of the final packaged Copy, Save, OCR, and toast
  flows are pending. Do not describe those as verified until tested.
- The repository has no project license file. Do not assume open-source terms.

See [the native app README](README.md) for installation, usage, build steps, and
privacy details.
