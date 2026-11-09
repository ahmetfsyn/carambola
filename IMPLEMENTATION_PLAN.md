# Native application status and follow-up

Carambola's active desktop application is the Rust and Slint project in
`native-app/`. The earlier Tauri implementation plan is obsolete; no Tauri
frontend, backend, updater, or release job is part of the active application.

## Current release scope

- Windows 10/11 x64 portable executable.
- Tray-first capture, crop selection, annotations, clipboard, PNG export,
  background OCR, settings, and optional Windows notifications.
- Automated formatting, Clippy, Rust tests, Windows Cargo checks, and a fresh
  optimized build in GitHub Actions.
- Installer, auto-update, code signing, and non-Windows releases are out of
  scope until separately implemented and verified.

## Open follow-up

`TODO.md` preserves a user-reported issue that the notification banner does not
appear. It remains unresolved pending a reproduction and supporting context.
The deferred notification-area flicker investigation is not part of this
release cleanup.
