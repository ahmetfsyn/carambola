# Technical decisions

## Native desktop application

The only supported desktop app is the Windows x64 Rust and Slint implementation
in `native-app/`. The earlier Tauri/React desktop application has been removed
from the active project and release path. Historical Git commits are retained.

Slint provides the overlay UI, with Winit handling Windows windows and events.
The app uses Windows APIs for capture, clipboard, dialogs, OCR, notifications,
the system tray, and global shortcuts. Its OCR path runs in the background and
preserves the current clipboard unless recognition produces nonempty text.

## Release and versioning

`native-app/Cargo.toml` is the single source for the app version. Cargo's
technical SemVer value has three numeric components and must use a zero patch
component (for example, `1.0.0`). The public version is derived as
`MAJOR.MINOR` (`1.0`), and its release tag is `v1.0`. `build.rs` maps the Cargo
version to Windows' four-part numeric resource while storing the public
two-part string in FileVersion and ProductVersion. The release validator
rejects three-part tags, mismatches, and nonzero Cargo patch components.

The workflow checks that the tag is on `master` before building. It creates a
draft, uploads the standalone executable, portable ZIP, and SHA-256 list,
verifies the uploaded files, then publishes.

The app currently ships as a portable executable. There is no installer,
updater, or code-signing setup to reproduce or claim.

## Performance claims

No startup, idle resource, activation latency, or rendering benchmark is part
of the repository's release evidence. Performance changes must include
repeatable measurements before their results are described as improvements.

## Support boundary

Windows 10/11 x64 is the supported desktop target. macOS and Linux adapters are
experimental and do not establish a verified end-to-end application on those
platforms. The website is independent static content and is not embedded in
the desktop application.
