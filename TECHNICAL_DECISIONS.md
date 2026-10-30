# Carambola — Technical Decisions & Trade-Off Analysis

This document records the foundational architectural decisions, comparative evaluations, performance benchmarks, and feasibility analyses for **Carambola**.

---

## 1. Architectural Paradigm Evaluation: Tauri v2 vs. Pure Rust GUI

The core architectural dilemma is whether to build Carambola with **Tauri v2 (Rust backend + TypeScript/Canvas frontend)** or with a **Pure Rust GUI framework (e.g., Slint, egui, or custom winit + wgpu)**.

### Comparative Evaluation Matrix

| Criterion | Tauri v2 (Rust + React/Canvas) | Pure Rust: Slint | Pure Rust: egui | Custom winit + wgpu |
| :--- | :--- | :--- | :--- | :--- |
| **Idle RAM (Background)** | 45–60 MB (macOS) / 70–95 MB (Win)* | 15–25 MB | 20–35 MB | 12–20 MB |
| **Hotkey-to-Overlay Latency** | 25–45 ms (pre-warmed) / 180–320 ms (cold) | 10–20 ms | 15–25 ms | 5–15 ms |
| **Annotation Vector Engine** | **High** (HTML5 Canvas 2D, Bezier curves, rich stroke smoothing) | **Low–Medium** (Custom drawing primitives limited) | **Medium** (Immediate mode path drawing) | **Very Low** (Must build complete 2D path renderer from scratch) |
| **Text Editing & System IME** | **Native Webview** (Full IME, emojis, font fallbacks, bi-directional text) | **Basic** (Improving, but complex IME edge cases) | **Basic** (CJK/IME input is notoriously brittle) | **None** (Requires custom glyph atlas & text layout engine) |
| **UI Polish / Controls** | **Extremely High** (Tailwind, icons, animations, toolbars) | **Medium** (Slint DSL, styling constraints) | **Low** (Distinct immediate-mode look and feel) | **Extremely Low** (Must build every button and slider) |
| **Binary Size** | ~10–18 MB | ~8–15 MB | ~12–20 MB | ~5–10 MB |
| **Ecosystem & Cloud/OCR** | Vast web/JS + Rust ecosystem | Rust-only | Rust-only | Rust-only |
| **Developer Velocity & Maintainability** | **High** (Separation of concerns: OS/Capture in Rust, Vector UI in Canvas) | **Medium** (Slint markup language + Rust callbacks) | **Medium** (Immediate-mode state synchronization) | **Very Low** (Massive boilerplate and maintenance burden) |

*\*Note: If the background daemon runs without a pre-warmed webview, Tauri idles at 10–18 MB (matching pure Rust), but sacrifices <50 ms activation.*

### Analysis & Resolution of the 15–30 MB Idle RAM Target

The prompt sets two ambitious performance goals:
1. **Overlay activation under 50 ms.**
2. **Idle RAM consumption of 15–30 MB.**

#### Feasibility Verdict:
With a WebView-based architecture (Tauri v2), **these two targets cannot be met simultaneously in a single static mode**:
- **On Windows**, Microsoft WebView2 spawns multi-process Chromium helpers (`msedgewebview2.exe` GPU process, renderer process, crashpad). Even with an empty DOM, the baseline resident set size (RSS) is **60–90 MB**.
- **On macOS**, WebKit launches `com.apple.WebKit.WebContent` and `com.apple.WebKit.Networking`, consuming **35–55 MB** of baseline RSS.
- If we destroy the WebView when idle and only keep the Rust daemon in RAM, the idle footprint is **12–18 MB** (comfortably beating the 15–30 MB target). However, initializing and bootstrapping a new WebView on hotkey press takes **180–320 ms on Windows** and **120–220 ms on macOS**, which fails the <50 ms activation requirement.

#### The Architectural Decision:
We select **Tauri v2 (Rust Backend + React/Canvas Frontend)** with an **Adaptive Dual-Mode Lifecycle**:
1. **Turbo Mode (Default):** The overlay window is pre-warmed at startup and kept hidden (`hide()`). On hotkey press, activation takes **25–40 ms**. Idle RAM is ~50 MB on macOS and ~75 MB on Windows.
2. **Eco Mode (Configurable via Settings):** The overlay window is destroyed after a screenshot is completed. The application idles as a pure Rust daemon using **12–18 MB RAM**. On hotkey press, the window is created on demand (latency ~200 ms).
3. **Adaptive Idle Purge:** In Turbo Mode, if no screenshot is taken for 30 minutes, the overlay webview is suspended/purged to reclaim memory. The next invocation performs a warm start.

#### Why Pure Rust Was Rejected for the Core Application:
While Slint or egui could achieve 20 MB RAM and 15 ms activation simultaneously, building a commercial-grade screenshot editor requires:
- Non-destructive vector editing (moving, scaling, deleting arrows, rectangles, freehand lines).
- In-place text editing with native OS IME support (essential for Asian languages, accents, emojis).
- Magnifier loupe, color pickers, and blur/mosaic pixel transformations.
Building these in immediate-mode Rust or Slint requires thousands of lines of low-level geometry math, text layout engines (cosmic-text), and custom widgets, substantially increasing bug density and maintenance overhead. The HTML5 Canvas API and Web platform provide a battle-tested vector rendering engine with native GPU compositing.

---

## 2. Screen Capture Strategy Evaluation

Screen capture is the most performance-critical backend operation.

### Candidate Technologies

1. **`xcap` (v0.0.9+):**
   - *How it works:* Actively maintained Rust crate that wraps modern native capture APIs.
   - *macOS:* Uses `ScreenCaptureKit` on macOS 12.3+ (high-performance GPU capture) with fallback to `CoreGraphics` (`CGDisplayCreateImage`).
   - *Windows:* Uses DXGI Desktop Duplication API (DirectX GPU surface) with fallback to Windows Graphics Capture (WGC) and GDI.
   - *Pros:* Cross-platform, thread-safe, active maintenance, supports per-monitor and per-window capture.
   - *Cons:* Heavy dependency tree on Linux (not our target), occasional multi-monitor offset coordinate bugs on mixed-DPI Windows if not carefully mapped.

2. **`scrap`:**
   - *Status:* Deprecated and unmaintained for several years. Uses legacy Direct3D 9 / GDI on Windows and deprecated `CGDisplayStream` on macOS.
   - *Verdict:* **Rejected.**

3. **Direct Native Implementations (`windows-rs` / `objc2`):**
   - *macOS (`ScreenCaptureKit` via `objc2`):* Optimal performance (<15 ms capture for 4K), but requires macOS 12.3+ and extensive Objective-C runtime boilerplate.
   - *Windows (DXGI via `windows-rs`):* Direct access to `IDXGIOutputDuplication::AcquireNextFrame`, sub-10 ms capture on discrete GPUs.

### Selected Approach: Layered Screen Capture Engine
We will use **`xcap` as the primary capture engine**, encapsulated behind a custom Rust trait:

```rust
pub trait ScreenCapturer: Send + Sync {
    fn capture_all_monitors(&self) -> Result<Vec<CapturedDisplay>, CaptureError>;
    fn capture_monitor(&self, monitor_id: u32) -> Result<CapturedDisplay, CaptureError>;
}
```

#### Fallback Hierarchy:
- **macOS:**
  1. `ScreenCaptureKit` via `xcap` (Hardware-accelerated, respects modern macOS security).
  2. Legacy `CoreGraphics` fallback (`CGWindowListCreateImage` / `CGDisplayCreateImage`) if running on macOS 11 or if ScreenCaptureKit returns an unrecoverable stream error.
- **Windows:**
  1. `DXGI Desktop Duplication` via `xcap` (Direct GPU surface lock, <10 ms).
  2. `GDI` (`BitBlt` from Desktop DC) as a safety fallback if DXGI fails (e.g., when running on hybrid GPU laptops with switchable graphics where Desktop Duplication can return `DXGI_ERROR_UNSUPPORTED`).

---

## 3. Overlay Window Strategy: Per-Monitor vs. Virtual Desktop Spanning

### Option A: Single Virtual Desktop Spanning Window
A single massive window whose bounds equal the bounding box of all displays combined (e.g. `(min_x, min_y)` to `(max_x, max_y)`).

**Fatal Platform Limitations:**
1. **macOS Spaces Limitation:** By default, macOS has *"Displays have separate Spaces"* enabled in Mission Control. Under this setting, an `NSWindow` **cannot cross physical display boundaries**. If a window is positioned across two screens, macOS clips it completely to the screen containing the window's center of mass.
2. **Windows Mixed DPI Scaling:** If Monitor 1 is 4K at 200% scaling and Monitor 2 is 1080p at 100% scaling, a single spanning WebView2 window will choose one DPI context. On the other monitor, content will either be rendered at half resolution (blurry) or scaled incorrectly by the DWM compositor.

### Option B: Per-Monitor Overlay Windows (Selected Architecture)
When the capture hotkey is pressed:
1. All active displays are captured synchronously into distinct frame buffers.
2. The Rust backend positions **one frameless, transparent, always-on-top window per physical monitor**, exactly matching that monitor's logical coordinates and native aspect ratio.
3. Each window loads the local monitor's screenshot slice.
4. An inter-window event bus coordinates cursor movement and multi-monitor selection across boundaries.

#### Benefits:
- Completely bypasses macOS Spaces clipping.
- Respects native DPI and Retina scaling (`window.devicePixelRatio`) per display without interpolation blur.
- Memory isolation: if a secondary monitor is unplugged, its window is torn down cleanly.

---

## 4. Image Data Transfer Strategy: Rust Backend to Frontend

A 4K display at native resolution is $3840 \times 2160$ pixels. In 32-bit RGBA, this is **~33.2 MB** of raw uncompressed pixel data per monitor. Transferring this across the IPC boundary is the biggest potential latency bottleneck.

### Evaluation of Transfer Channels

1. **Base64 over JSON IPC (Standard Tauri Command):**
   - Rust must encode 33 MB of bytes into a 44 MB Base64 string.
   - JSON serialization and parsing costs ~40–80 ms of CPU time.
   - Frontend must decode the Base64 string into a Blob.
   - *Verdict:* **Rejected.** Violates the 50 ms budget completely.

2. **Tauri v2 Binary IPC (`tauri::ipc::Response::new` / Channels):**
   - Tauri v2 allows sending raw byte buffers (`Vec<u8>`) directly across the webview IPC as a native JavaScript `ArrayBuffer`.
   - Latency: ~3–6 ms for 33 MB.
   - Frontend creates an `ImageData` or `ImageBitmap` from the `ArrayBuffer`.

3. **In-Memory Custom URI Protocol (`carambola://`):**
   - Rust registers a custom protocol scheme (`carambola://screenshot/{monitor_id}?t={timestamp}`).
   - When the frontend requests this URL, Rust serves the raw PNG/BMP/RGBA stream directly from an in-memory buffer without file disk I/O.
   - The browser's native C++ image decoder decodes the image asynchronously via hardware acceleration.

### Selected Approach: Dual-Path Transfer
- **Primary:** Rust stores captured frames in an in-memory cache protected by an `Arc<RwLock<FrameCache>>`. The frontend overlay requests its display frame via the custom protocol `carambola://frame/{monitor_id}` and renders it using HTML5 `<canvas>` via `createImageBitmap(blob)`.
- **Secondary (Raw Pixels):** For operations requiring direct pixel manipulation (magnifier loupe, color picker, OCR), Rust exposes a command returning a typed raw `ArrayBuffer` slice of the region of interest.

---

## 5. Canvas and Annotation Engine Architecture

### Evaluated Frontend Libraries

1. **Heavy Canvas Frameworks (Fabric.js, Konva.js):**
   - *Pros:* Complete scene graph, object selection, transform handles.
   - *Cons:* Significant bundle size (100–300 KB), heavy DOM-like overhead, sluggish with thousands of freehand points, difficult to implement real-time custom pixel shaders (mosaic/blur).

2. **Custom Layered HTML5 Canvas Engine (Selected Architecture):**
   - We implement a bespoke, lightweight, high-performance canvas engine tailored specifically for screenshot annotations.
   - **4-Layer Compositing Architecture:**
     1. **Background Layer (Static):** Displays the raw captured screenshot. Never redrawn during user interaction.
     2. **Dimmer / Mask Layer:** Renders the dark semi-transparent overlay ($rgba(0, 0, 0, 0.4)$) with a `globalCompositeOperation = 'destination-out'` cutout for the selected crop rectangle.
     3. **Annotation Vector Layer:** Renders user-drawn vectors (arrows, boxes, lines, text) within the crop boundary.
     4. **Interaction & UI Layer:** Renders selection handles, dimensions tooltip ($1920 \times 1080$), active drawing previews, magnifier loupe, and floating toolbar.

#### Non-Destructive Editing & History
- All annotations are stored as pure immutable data objects:
  ```typescript
  type AnnotationElement =
    | { type: 'arrow'; start: Point; end: Point; color: string; strokeWidth: number }
    | { type: 'rect'; bounds: Rect; color: string; strokeWidth: number; fill?: string }
    | { type: 'pen'; points: Point[]; color: string; strokeWidth: number }
    | { type: 'text'; position: Point; text: string; fontSize: number; color: string }
    | { type: 'blur'; bounds: Rect; radius: number }
    | { type: 'mosaic'; bounds: Rect; blockSize: number };
  ```
- History is managed via a standard command stack (`undoStack` and `redoStack`), enabling unlimited non-destructive undo/redo.

---

## 6. OCR (Optical Character Recognition) Engine Selection

### Evaluated Alternatives

1. **`tesseract-rs` (Bundled Tesseract OCR):**
   - *Pros:* Cross-platform.
   - *Cons:* Requires bundling C++ Tesseract libraries and large language training data (`eng.traineddata` is ~25 MB per language). Slow initialization, high memory usage, cold execution takes 400–1200 ms.

2. **Cloud OCR APIs (Google Cloud Vision, AWS Rekognition):**
   - *Pros:* High accuracy.
   - *Cons:* Requires internet connectivity, API keys, subscription cost, privacy violation (sending user screen content to remote cloud).

3. **Platform-Native OCR Engines (Selected Architecture):**
   - **macOS:** Apple **Vision Framework** (`VNRecognizeTextRequest`).
     - Built into macOS 10.15+. Zero bundle size overhead.
     - Utilizes the Apple Neural Engine (ANE) on Apple Silicon and Metal GPU on Intel.
     - Typical execution time on a 1080p crop: **15–40 ms**.
     - Accessible from Rust via `objc2-vision` / `apple_vision`.
   - **Windows:** **Windows.Media.Ocr** (WinRT).
     - Built into Windows 10 (1511+) and Windows 11. Zero bundle size overhead.
     - Uses installed Windows language packs.
     - Typical execution time: **30–70 ms**.
     - Accessible from Rust via the official `windows` crate (`windows::Media::Ocr`).

#### Decision:
We implement a unified Rust trait `OcrProvider` backed by **Platform-Native OCR** engines on both operating systems. This delivers instant, 100% offline, privacy-first OCR with zero binary bloat.

---

## 7. Global Keyboard Shortcut Strategy

### Evaluated Technologies
- `tauri-plugin-global-shortcut` (wraps the Rust `global-hotkey` crate).

### Platform Nuances & Conflicts:
- **Windows:**
  - Default shortcut: `PrintScreen` (or `Ctrl + Shift + S` / `Alt + A`).
  - *Conflict Notice:* Windows 11 (22H2+) introduced a setting that maps `PrintScreen` to the native Snipping Tool by default. Registering `PrintScreen` may fail with `ERROR_HOTKEY_ALREADY_REGISTERED`.
  - *Resolution:* Carambola will attempt to register `PrintScreen`. If it fails, it alerts the user and falls back to `Alt + A` or a user-defined combination, providing guidance on how to disable the Windows Snipping Tool override in *Settings > Accessibility > Keyboard*.
- **macOS:**
  - Default shortcut: `Cmd + Shift + X` (avoids conflict with native macOS `Cmd + Shift + 3/4/5`).
  - *Permission:* Global shortcut listeners using Carbon `RegisterEventHotKey` do not require Accessibility permissions for non-intercepted combinations. However, if using low-level event taps (`CGEventTap`), macOS requires **Accessibility permissions** (`AXIsProcessTrusted`).

---

## 8. Summary of Architectural Decisions

| Area | Decision | Primary Rationale |
| :--- | :--- | :--- |
| **App Framework** | Tauri v2 (Rust + React + Canvas) | Rich vector UI ecosystem, native IME, robust cross-platform tray/window APIs. |
| **Window Strategy** | Pre-warmed Per-Monitor Overlays | Bypasses macOS Spaces clipping, ensures crisp multi-DPI rendering, achieves <50 ms latency. |
| **Screen Capture** | `xcap` with Native API Fallbacks | DXGI Desktop Duplication on Windows (<10ms), ScreenCaptureKit on macOS (<15ms). |
| **Image Transfer** | In-Memory Protocol (`carambola://`) | Zero-copy / stream-based loading, avoids Base64 CPU/memory penalties. |
| **Annotation Engine** | Custom Layered 2D Canvas | Lightweight, high-DPI scaling, smooth freehand interpolation, non-destructive undo/redo. |
| **OCR Engine** | Platform-Native (Vision / WinRT OCR) | 100% offline, privacy-safe, hardware-accelerated, zero binary weight. |
| **Clipboard** | Native Rust Clipboard (`arboard`) | Reliable cross-platform bitmap and text clipboard transfers without webview limitations. |

