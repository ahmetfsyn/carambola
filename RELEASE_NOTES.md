# Carambola 1.0

Carambola 1.0 is the first official release of the unified Rust and Slint
desktop application for Windows 10/11 x64.

## Included

- Monitor-under-pointer capture with a movable and resizable selection.
- Toolbar fades out while resizing and returns when the resize finishes.
- Pen, arrow, rectangle, triangle, ellipse, text, and mosaic annotations.
- Ctrl+Z undo and Ctrl+Y redo for annotation edits while a selection is active.
- Clipboard copy, PNG save, background Windows OCR, tray controls, and a
  configurable global capture shortcut.
- Per-user `Carambola-Setup-1.0.exe` installer with Apps & Features
  registration, Start Menu shortcuts, and an optional desktop shortcut.
- Standalone `Carambola.exe` and `Carambola-1.0-Windows-x64.zip` with
  `Carambola.exe` and `README.txt`.
- Upgrades and uninstall retain settings and diagnostics under
  `%APPDATA%\Carambola`; close the app before upgrading or uninstalling.
- Windows file and product metadata use the name Carambola and public version
  `1.0`.

## Requirements and limitations

- Windows 10/11 x64. OCR requires a suitable installed Windows OCR language.
- The installer and portable EXE/ZIP are not Authenticode-signed. There is no
  automatic updater.
- macOS and Linux are not supported release targets.
- The repository has no top-level project license file.
- Interactive physical Windows regression checks are still needed; this
  release note does not claim that they have been completed.
