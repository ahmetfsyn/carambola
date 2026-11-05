# Carambola Native Rust Migration Plan

Status: Phase A audit complete. Phase B Slint/Winit prototype builds. Phase C
passed capture, exact crop-to-PNG verification, Escape cancellation, and a
subsequent capture/crop at the available 125% scale. Phase 4.1's annotation
foundation and Phase 5's annotation completion are implemented. Phase 5 passed
`cargo fmt --check`, `cargo check --offline`, all 35 `cargo test --offline`
tests, `git diff --check`, and the Windows `cargo build --offline --release`.
The user manually confirmed Text, Mosaic, Undo/Redo, existing annotation tools,
PNG export, lowercase text, painter ordering, and mosaic compositing in the
Windows Release application. Phase 6 export/clipboard implementation passed
`cargo fmt --check`, `cargo check --offline`, all 40 `cargo test --offline`
tests, `git diff --check`, and the Windows Release build. The Release app is
running and awaiting physical Save/Copy testing.
Runtime tests at 100% and 150%, mixed-DPI displays, macOS capture, monitor
selection, and the remaining product features are still open.

Phase 5 annotation completion is being reviewed on
`feat/native-annotation-completion`. The existing
Tauri application remains the behavioral reference and release implementation
until the native application has passed the parity and runtime gates below.
Nothing in this plan authorizes publishing or replacing the v1.0.0 release.

## Phase A — Repository Audit

### Current implementation map

| Area | Current implementation | Migration requirement |
| --- | --- | --- |
| App lifecycle and single instance | `src-tauri/src/lib.rs`, `main.rs`; Tauri single-instance plugin; tray-first setup; diagnostic file log | Rust main process, native single-instance behavior, tray lifetime, diagnostics |
| Tray and capture activation | `src-tauri/src/tray/mod.rs`; `src-tauri/src/hotkey/mod.rs` | Native tray/menu and global hotkey; retain configurable shortcut, platform defaults, and fallback diagnostics |
| Capture and display selection | `src-tauri/src/capture/mod.rs`; `xcap::Monitor`, Windows capture fallback and macOS CoreGraphics paths | Native capture adapters; per-display physical frame dimensions and stable monitor identity |
| Overlay lifecycle and DPI | `src-tauri/src/window/mod.rs`, `src-tauri/src/app_state.rs`; persistent hidden Tauri window, native positioning, per-capture geometry | Native transparent overlay; synchronize capture, monitor, physical geometry, and input coordinates before show |
| Crop and annotations | `src/components/overlay/OverlayContainer.tsx`, `src/lib/canvas/{geometry,renderLayers,drawShapes,composeCrop}.ts`, `src/types/{crop,annotations}.ts` | Rust-owned selection/annotation model and rendering/compositing with pixel-space contracts |
| Annotation controls | `src/components/toolbar/FloatingToolbar.tsx` | Compact native toolbar; selection, arrow, pen, rectangle, triangle, circle, text, mosaic, colors, widths, undo/redo, copy, save, OCR, settings, close |
| Export and clipboard | `src-tauri/src/commands/mod.rs`, `src-tauri/src/clipboard/mod.rs` | Native crop compositing, PNG copy, timestamped save under Pictures/Carambola, text clipboard |
| OCR | `src-tauri/src/ocr/mod.rs`; Windows Media OCR only; macOS explicitly returns unavailable | Preserve Windows OCR; implement and verify Vision OCR on macOS before marking parity |
| Settings and persistence | `src/components/settings/ShortcutModal.tsx`, `src-tauri/src/config.rs`, `src-tauri/src/autostart.rs` | Native settings; preserve existing `carambola/config.json` fields and legacy defaults without reset |
| Notifications | Tauri notification plugin plus native platform code (`src-tauri/src/notification/`) | Native delivery, permission/setting behavior, and existing opt-out preference |
| Updates and packaging | `src/lib/updater.ts`, `src-tauri/tauri.conf.json`, `.github/workflows/{release,publish-release}.yml` | Replace Tauri updater/bundler with independently verified update and installer flows; preserve signature checks and Windows-only production strategy until new targets pass |
| Version/About | `src/lib/appVersion.ts`, `src/components/settings/AboutSection.tsx` | Compile-time version in native About view |

### Existing feature boundary

The repository currently has no cloud upload or cloud sync; `README.md` says
screenshots remain local and that there is no cloud sync. It also has no
user-selectable theme/appearance preference. Those are not current v1.0.0
behaviors to migrate. macOS OCR is also not implemented in the current app.
These discrepancies are recorded rather than treated as completed features.

Current persistence is a JSON object at `dirs::config_dir()/carambola/config.json`
with `shortcut`, optional `autostart`, `screenshot_notifications`, and
`notification_permission_prompted`. First-run startup registration defaults to
enabled. Preserve those fields and semantics; add a versioned migration only if
the native implementation needs a new representation.

### Framework evaluation

The current official Slint documentation describes the Winit backend for
Windows and macOS, transparent window backgrounds, position/size/fullscreen and
visibility APIs, a raw native-window-handle feature, GPU renderers, and an
accessibility API. Slint is the leading candidate for the product UI and first
overlay prototype. The choice remains gated on real overlay tests for mixed DPI,
monitor targeting, transparency, z-order, and repeated show/hide cycles.

| Candidate | Strengths for Carambola | Risks / verification needed |
| --- | --- | --- |
| Slint (primary) | Declarative compiled UI; native Winit backend; physical window position/size APIs; transparent background and frameless windows; GPU and software renderer options; accessibility metadata and OS API integration | Fullscreen-on-specific-monitor behavior, Windows topmost/transparency, macOS Spaces/level, hidden-window warmup, and DPI-change sequencing require prototypes and runtime tests. No trustworthy memory/latency comparison has been measured yet. Licensing choice must be documented: Slint offers GPLv3, royalty-free with attribution, or commercial terms. |
| egui/eframe | MIT OR Apache-2.0; immediate-mode custom painting suits annotations; native viewports expose transparency and monitor selection; eframe enables AccessKit | More hand-built application layout and settings UI; multi-viewport behavior differs by backend; native overlay behavior still needs the same platform tests. No performance measurements are available. |
| Raw winit + renderer | Maximum control of windows, input, swapchain, and drawing | Does not provide the application UI, accessibility, or product widgets; greatly increases custom code and maintenance. Use only as an overlay integration layer if a UI framework fails a required native behavior. |

No framework is presumed to be faster or smaller. Capture latency, frame time,
idle/session memory, and startup measurements must be collected on actual
Windows and macOS builds before performance claims are made.

### Architecture decision under test

Use Slint + Winit as the prototype framework for the tray-adjacent settings/tool
UI and a pre-created, hidden, frameless transparent overlay window. Use a custom Rust
rendering/compositing core and physical-pixel geometry contracts; do not put
screen pixels or application state across a WebView/IPC boundary. Use the
Slint/Winit native handle for focused Win32/AppKit adjustments. If per-monitor
fullscreen, topmost ordering, transparency, or macOS Spaces behavior fails on
either platform, keep Slint for ordinary UI and replace only the overlay host
with a native Win32/AppKit host and a Slint-compatible/custom rendering adapter.

Each capture gets an immutable display/frame geometry record: stable monitor
identity, physical bounds/origin (signed), scale factor, captured bitmap size,
and native overlay client geometry. Physical pixels are the canonical space for
the captured frame and output. Pointer coordinates are transformed through one
tested mapping into that space. DPI/display notifications invalidate cached
geometry; a capture cannot present until actual native and render geometry
matches the current frame. No fixed sleeps are used for synchronization.

### Feature parity checklist

Status values: `[x]` audited in the existing application; `[ ]` migrated and
verified. Audit does not count as migration.

- [x] Global screenshot shortcut, editable binding, platform default, fallback: `src-tauri/src/hotkey/mod.rs`, settings UI.
- [x] Tray/background lifecycle, capture/settings/quit menu, single-instance behavior: `src-tauri/src/lib.rs`, `src-tauri/src/tray/mod.rs`.
- [x] Per-session frozen capture, monitor selection, overlay show/hide lifecycle: `src-tauri/src/capture/`, `src-tauri/src/window/`.
- [x] Selection creation, move, edge/corner resize, dimensions and pointer-coordinate geometry: `src/components/overlay/OverlayContainer.tsx`, `src/lib/canvas/geometry.ts`.
- [x] Pen, arrow, rectangle, triangle, circle, text, mosaic redaction; eight colors; 1–20 px stroke widths: `src/lib/canvas/drawShapes.ts`, `src/components/toolbar/FloatingToolbar.tsx`.
- [x] Annotation undo/redo: `src/components/overlay/OverlayContainer.tsx`.
- [x] Clipboard PNG, local PNG save, OCR text clipboard: `src-tauri/src/commands/mod.rs`, `src-tauri/src/clipboard/`, `src-tauri/src/ocr/`.
- [x] Windows OCR exists; macOS OCR is currently unavailable and is a migration gap if parity requires it.
- [x] Start on Boot preference and OS registration, including first-run default: `src-tauri/src/autostart.rs`, `src-tauri/src/config.rs`.
- [x] Screenshot notifications with opt-out and permission state: settings UI, commands, native notification code.
- [x] Windows signed updater check/install UI and release workflows; target is Windows x64 today.
- [x] Installed version/About view.
- [x] Existing JSON configuration and its legacy defaults.
- [x] Product explicitly has no cloud upload/sync or selectable themes today; no implementation exists to preserve.
- [ ] New Rust-only entry point and lifecycle, running alongside the preserved Tauri reference.
- [ ] Native tray and global shortcut on Windows and macOS.
- [x] Transparent full-screen selection overlay prototype on Windows; macOS remains untested.
- [x] Windows primary-display capture and physical-pixel crop geometry contract; dynamic monitor and mixed-DPI synchronization remain open.
- [x] Phase 4.1 Rust annotation data model, image-space coordinate mapping, five-tool software rendering, Slint preview, and fresh-frame compositing interface.
- [x] Phase 5 Text and Mosaic annotations, shared Undo/Redo history, and common preview/export compositing; offline tests and Windows Release build passed, and the user manually verified Text, Mosaic, Undo/Redo, the existing tools, lowercase text, painter ordering, mosaic compositing, and PNG export.
- [x] Phase 6 Windows PNG Save As and CF_DIBV5 image clipboard implementation; automated tests and Windows Release build passed. Physical Save/Copy runtime verification is pending.
- [ ] Windows OCR and macOS Vision OCR.
- [ ] Settings, notification, startup, config migration, version/About.
- [ ] Single-instance and native update/installer strategy.
- [ ] Windows x64, Windows ARM64, macOS Intel, macOS Apple Silicon builds and installed-runtime verification.
- [ ] Remove Tauri, WebView2/WKWebView, React/TypeScript/Vite, IPC/plugins only after all preceding parity gates pass.

## Controlled implementation phases

1. **A — Audit:** completed in this document. No installed v1.0.0 baseline was
   available for a controlled before/after performance comparison.
2. **B — Foundation (in progress):** added the separate `native-app/` Rust
   crate, Slint settings/overlay prototype, Windows Per-Monitor V2 initialization,
   and pure physical-pixel mapping tests. The Windows prototype build and basic
   transparent fullscreen/close interaction passed. Native raw-handle tuning,
   tray, hotkey, DPI transitions, and the final overlay-host choice are open.
3. **C — Screenshot core (Windows prototype):** added a validated RGBA frame
   contract, GDI primary-display capture, a frozen-frame native selection
   overlay, clamped logical-to-physical coordinate mapping, and crop-to-PNG
   output under the system temporary directory. Selection movement does not
   recapture or copy the frame. Capture runs off the UI thread and completes
   before the transparent overlay is shown, preventing the overlay from being
   included in its own desktop capture. Eleven native-core tests passed at this
   phase, covering capture-session concurrency, transition, cancellation, retry, and error
   cleanup tests. A raw pre-overlay diagnostic frame showed that hiding the
   settings HWND and calling DwmFlush still left its pixels in the GDI capture.
   The settings window is now temporarily marked `WDA_EXCLUDEFROMCAPTURE` before
   hiding; the affinity remains until the window is restored, then resets to
   `WDA_NONE`. This removed the Carambola window from both the independently
   inspected 1920x1080 raw frame and the overlay. The capture path also deselects
   its DIB before `GetDIBits`, as required by GDI, and converts BGRA to opaque
   RGBA with tests. Current desktop geometry was 1536x864 logical against
   1920x1080 physical (125%). A 625x380 selected PNG matched the exact source
   frame crop byte-for-byte; Escape restored settings, and another capture and
   crop passed. Runtime scale changes to 100%/150% were unavailable through the
   desktop UI, so those scales are unit-tested only. Monitor enumeration and
   selection, dynamic display changes, production save behavior, and macOS
   ScreenCaptureKit remain open.
   **Phase 4.1 — Annotation foundation:** added a framework-independent model
   with stable IDs/z-order and selected-crop physical-pixel geometry; viewport
   mapping feeds a separate transparent renderer for pen, arrow, rectangle,
   triangle, and ellipse. The native overlay keeps the crop immutable while
   previewing strokes and composites a fresh output frame on Done.
   **Phase 5 — Native annotation completion:** added Text and Mosaic
   annotations and one shared undo/redo history across all annotation types.
   Preview and PNG export use the same deterministic compositor against the
   original selected crop; mosaic effects and vector/text marks follow document
   painter order. Offline automated checks cover text rendering, mosaic bounds
   and pixelation, painter order, history branching, and source immutability;
   35 tests passed. `cargo fmt --check`, `cargo check --offline`,
   `git diff --check`, and `cargo build --offline --release` also passed.
   The user has physically confirmed Text, Mosaic, Undo/Redo, the existing
   annotation tools, lowercase text, painter ordering, mosaic compositing, and
   PNG export in the Windows Release app.
   **Phase 6 — Export & Clipboard:** added a native Windows PNG Save As dialog,
   PNG encoding/writing of the same `render_composite` output used by preview,
   and CF_DIBV5 clipboard transfer using movable global memory with explicit
   ownership and cleanup. Save and Copy operate on an immutable crop/annotation
   snapshot and leave the annotation session open; canceling Save As leaves it
   unchanged. Implementation files: `native-app/Cargo.toml`,
   `native-app/src/core/{clipboard,export}.rs`,
   `native-app/src/platform/{mod,windows}.rs`, `native-app/src/main.rs`,
   `native-app/ui/{overlay,windows}.slint`, and this plan. Forty offline tests,
   `cargo fmt --check`, `cargo check --offline`, `git diff --check`, and the
   Windows Release build passed. The Release app has been launched, but physical
   Save As and clipboard behavior have NOT YET BEEN VERIFIED by the user.
4. **D — Remaining product features:** migrate OCR, tray/hotkey,
   settings/config, startup, notifications, About, and updater/package flows.
5. **E — Removal:** only after Windows and macOS build/runtime parity passes,
   remove Tauri and frontend tooling and update installers/workflows/docs.

## Verification gates and baseline

Automated build/test and interactive installed-app verification are reported
separately. Required platform matrix: Windows x64 and ARM64 at 100%, 125%, and
150%, including mixed-DPI changes and monitor add/remove/rearrangement; macOS
Intel and Apple Silicon with Retina/non-Retina changes and screen-capture
permission flows. Also exercise repeated sessions, clipboard, startup, hotkeys,
notifications, tray lifetime, export fidelity, and updates.

Measure cold startup, idle RSS, hotkey-to-visible latency, selection frame time,
idle CPU, selection CPU, and screenshot-session memory using the same procedure
before and after migration. The initial native UI-only development run on the
available Windows desktop produced one warm sample after a 5-second warmup:
working set 135 MiB; process CPU 109 ms over the following 10 seconds. This is
not an installed build or a full screenshot session, has no Tauri comparison,
and is only a preliminary runtime observation. Cold-start latency was not
reliably measured (`WaitForInputIdle` is unsupported by this Winit process).
Selection frame time, hotkey activation, selection CPU, and screenshot-session
memory remain unmeasured because those features do not exist in the prototype.

Do not publish a native installer, change the existing production signing keys,
alter v1.0.0 assets/tags, or remove the old application while these gates remain
unchecked.
