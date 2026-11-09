# Native migration record

This document records the transition from the former Tauri desktop app to the
native Rust and Slint implementation. It is historical context; the current
architecture and release instructions are in [README.md](README.md) and
[ARCHITECTURE.md](ARCHITECTURE.md).

## Current state

The native app is the only active desktop application. Its Windows x64
portable release path, user interface, capture workflow, annotations, PNG
export, clipboard, background OCR, tray integration, settings, and Windows
notifications are maintained under `native-app/`. The previous Tauri source,
frontend, updater scripts, and Tauri release workflows have been removed.

The app has source adapters for some non-Windows APIs, but macOS and Linux do
not have a complete verified capture and packaging path. No cross-platform
release is claimed.

## Historical verification notes

Earlier development sessions recorded automated Rust checks and manual testing
of annotation rendering, export, and portions of the Windows capture flow.
Those notes are not a substitute for the current release workflow or for
retesting hardware-dependent behavior on a target machine. Consult
`native-app/WINDOWS_MANUAL_REGRESSION.md` for the current checklist.

The notification-area and overlay transition investigation was explicitly
deferred. This migration cleanup does not repeat that investigation.
