# Native Carambola application

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

The executable is `target\release\carambola-native.exe`. Release metadata is
generated from the version in `Cargo.toml`, and the tagged release workflow
checks that the tag matches it.

See [WINDOWS_MANUAL_REGRESSION.md](WINDOWS_MANUAL_REGRESSION.md) for
hardware-dependent checks. Do not treat automated tests as physical Windows
verification.
