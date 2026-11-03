# Carambola v0.1.3

## Changes

- Route Windows screenshot notifications through Tauri's native notification backend and report delivery errors.
- Add signed in-app update checks, an update prompt, manual Settings action, download progress, and restart support.
- Preserve the existing macOS notification implementation; macOS notification runtime verification remains pending.

## Upgrade

Users of v0.1.2 must install v0.1.3 manually once. In-app signed updates are available starting with v0.1.3.

## Distribution status

This note describes the intended release. Do not publish it until the Windows and macOS artifacts, updater signatures, and generated `latest.json` have passed the release checks.
