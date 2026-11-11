# Carambola 1.2

Carambola 1.2 is a production release of the unified Rust and Slint desktop
application for Windows 10/11 x64.

## What's new

- The text annotation editor receives keyboard focus when it opens, so typing
  can begin immediately.
- Installed copies can check for compatible stable updates from Settings.
  Downloads use HTTPS, verify the published SHA-256 checksum, and require
  confirmation before opening the normal per-user installer.

## Included

- Monitor-under-pointer capture with a movable and resizable selection.
- Pen, arrow, rectangle, triangle, ellipse, text, and mosaic annotations, with
  undo and redo for annotation edits.
- Clipboard copy, PNG save, background Windows OCR, tray controls, and a
  configurable global capture shortcut.
- Per-user `Carambola-Setup-1.2.exe` installer with Apps & Features
  registration, Start Menu shortcuts, and an optional desktop shortcut.
- Standalone `Carambola.exe` and `Carambola-1.2-Windows-x64.zip` containing
  `Carambola.exe` and `README.txt`.
- Upgrades and uninstall retain settings and diagnostics under
  `%APPDATA%\Carambola`; close the app before upgrading or uninstalling.
- Windows file and product metadata use the name Carambola and public version
  `1.2`.

## Requirements and limitations

- Windows 10/11 x64. OCR requires a suitable installed Windows OCR language.
- The installer and portable EXE/ZIP are not Authenticode-signed.
- Updates are manual and available only in installed copies. Portable copies
  must be updated by replacing their files with the new portable download.
- macOS and Linux are not supported release targets.
- The repository has no top-level project license file.
- Interactive physical Windows regression checks have not been completed.
