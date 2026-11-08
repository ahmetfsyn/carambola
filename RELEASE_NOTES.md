# Carambola v1.0.0

The initial production release supports **Windows x86_64** only.

## Included

- Per-user Windows NSIS installer.
- Signed in-app update artifact and Windows-only `latest.json` entry.
- Screenshot capture, selection, annotation, clipboard export, PNG saving, tray controls, and configurable capture shortcut.
- Optional screenshot notifications and update status in Settings.

## Upgrade

Install v1.0.0 manually from the Windows installer. Signed in-app updates are available from this release onward for supported Windows x86_64 installations.

## Platform status

- Windows x86_64: production release target. See the release page for verified installer and updater assets.
- macOS Apple Silicon and Intel: not included in this release; they will be added to this same GitHub Release after separate production-readiness testing.
- Linux x86_64: not included in this release; it will be added to this same GitHub Release after separate production-readiness testing.

The Windows installer is not Authenticode-signed. The updater artifact uses Carambola's existing Tauri updater key. macOS and Linux are not included and are not claimed as supported by this release.
