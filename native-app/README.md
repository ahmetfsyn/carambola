# Carambola application

The Rust and Slint application runs on Windows 10/11 x64. It starts in the
notification area, captures the monitor under the pointer, and supports crop
selection, annotations, image Copy, PNG Save, background OCR, configurable
shortcuts, settings, and optional Windows notifications.

The project overview, download instructions, feature guide, OCR behavior,
privacy notes, build commands, support boundary, and license status are in the
[repository README](../README.md).

## Build and validate

From this directory, use a stable Rust MSVC toolchain and the Windows SDK:

```powershell
cargo fmt --all -- --check
cargo clippy --locked --all-targets
cargo test --locked --all-targets
cargo check --locked
cargo build --locked --release
./tools/Test-ReleaseMetadata.ps1
```

From the repository root, build the per-user Windows installer with:

```powershell
.\scripts\build-windows-installer.ps1
```

This command builds the release executable if needed, then invokes Inno Setup's
`ISCC.exe` compiler. Install Inno Setup 7 from its official download page when
the compiler is missing; the script never downloads or runs the Inno Setup
installer itself.

Cargo builds `target\release\carambola.exe`; release packages present the
application as `Carambola.exe`. Cargo's technical version uses three numeric
components (for example, `1.0.0`) and maps to the public two-part version `1.0`.
The package metadata is the authoritative source; the release workflow checks
that a two-part tag such as `v1.0` matches it.

See [WINDOWS_MANUAL_REGRESSION.md](WINDOWS_MANUAL_REGRESSION.md) for
hardware-dependent checks. Do not treat automated tests as physical Windows
verification.
