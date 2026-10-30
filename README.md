# Carambola 🌟

> A modern, lightning-fast, lightweight, cross-platform desktop screenshot, crop, and annotation application built with **Tauri v2**, **Rust**, **React**, and **TypeScript**.

---

## Features

- **Global Hotkey Activation:** Instant capture via `Command+Shift+X` (macOS) or `PrintScreen` (Windows).
- **System Tray Integration:** Runs discreetly in the menu bar / system tray with instant capture and clean exit options.
- **Hardware-Accelerated Screen Capture:** Powered by `xcap` with GPU-level capture pipelines (ScreenCaptureKit on macOS, DXGI on Windows).
- **Zero-Flicker Full-Screen Overlay:** Frameless, transparent, always-on-top window presenting the captured screen.
- **Interactive Region Cropping:** Click-and-drag selection box with 8-point resize handles and live pixel dimensions.
- **4x Magnifier Loupe:** Pixel-level precision crosshair showing live RGB Hex colors and coordinate tracking.
- **Non-Destructive Annotation Tools:**
  - ↗️ **Arrows:** Directional vector arrows with crisp arrowheads.
  - 🔲 **Rectangles:** Outlined bounding boxes.
  - ✏️ **Smooth Pen:** Freehand drawing interpolated with Catmull-Rom splines.
  - 🔤 **Text Annotations:** In-place typography with high-contrast background strokes.
  - 🪟 **Blur & Mosaic:** Privacy redaction filter for sensitive data and credentials.
- **Output Pipelines:**
  - 📋 **Copy to Clipboard:** Composites crop + annotations in native Rust via `arboard`.
  - 💾 **Save to Disk:** High-resolution PNGs timestamped in `~/Pictures/Carambola`.
  - 🔍 **Native Offline OCR:** Text extraction via Windows Media OCR (`Windows.Media.Ocr`) and Apple Vision.

---

## Project Architecture & Documentation

- [**`ARCHITECTURE.md`**](./ARCHITECTURE.md): Complete system overview, process model, multi-monitor coordinate alignment, and latency budgets.
- [**`TECHNICAL_DECISIONS.md`**](./TECHNICAL_DECISIONS.md): Evaluated trade-offs (Tauri v2 vs. Pure Rust, 15–30 MB RAM vs. <50 ms latency, screen capture backends).
- [**`PROJECT_STRUCTURE.md`**](./PROJECT_STRUCTURE.md): Detailed module layouts and separation-of-concerns boundary rules.
- [**`IMPLEMENTATION_PLAN.md`**](./IMPLEMENTATION_PLAN.md): Phase-by-phase roadmap and testing strategies.

---

## Development Setup

### Prerequisites
1. **Node.js** (v18+) & **npm**
2. **Rust** (v1.75+) with `cargo`

### Quick Start

```bash
# 1. Install frontend dependencies
npm install

# 2. Build frontend assets
npm run build

# 3. Compile and run in development mode
npm run tauri dev
```

