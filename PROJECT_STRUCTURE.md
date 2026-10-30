# Carambola — Recommended Project Structure

This document outlines the complete directory layout, file organization, configuration schemas, and separation of concerns for the **Carambola** repository.

---

## 1. Top-Level Repository Layout

```
carambola/
├── .github/                       # CI/CD Workflows, issue templates
│   └── workflows/
│       ├── test.yml               # Automated unit & integration tests
│       ├── lint.yml               # Clippy, Rustfmt, ESLint, Prettier
│       └── release.yml            # Cross-platform builds (macOS & Windows)
├── src-tauri/                     # Rust Backend (Tauri v2 Core & OS Integration)
│   ├── Cargo.toml                 # Rust dependencies and feature flags
│   ├── build.rs                   # Tauri build script
│   ├── tauri.conf.json            # Tauri v2 application configuration
│   ├── icons/                     # Application & tray icons (.icns, .ico, .png)
│   └── src/                       # Rust source modules (see Section 2)
├── src/                           # Frontend (React + TypeScript + Canvas)
│   ├── index.html                 # Overlay entry point HTML
│   ├── main.tsx                   # React root bootstrapping
│   ├── App.tsx                    # Top-level window controller
│   ├── assets/                    # Static SVG icons, logos, brand styling
│   ├── components/                # React UI components (see Section 3)
│   ├── hooks/                     # Custom React hooks (gestures, state, canvas)
│   ├── lib/                       # Pure logic, canvas rendering, geometry, IPC
│   ├── styles/                    # Global Tailwind / CSS styles
│   └── types/                     # Shared TypeScript type definitions
├── tests/                         # Integration and End-to-End Tests
│   ├── backend/                   # Rust integration tests
│   └── frontend/                  # Vitest / Canvas unit tests
├── package.json                   # Frontend dependencies, scripts
├── tsconfig.json                  # TypeScript compiler configuration
├── tsconfig.node.json             # Vite TypeScript configuration
├── vite.config.ts                 # Vite bundler configuration
├── tailwind.config.ts             # Tailwind CSS styling configuration
├── ARCHITECTURE.md                # System Architecture specification
├── IMPLEMENTATION_PLAN.md         # Phased roadmap & milestone breakdown
├── TECHNICAL_DECISIONS.md         # Architectural decisions & benchmarks
├── PROJECT_STRUCTURE.md           # This document
└── README.md                      # Project overview and developer onboarding
```

---

## 2. Rust Backend (`src-tauri/`) Detailed Structure

```
src-tauri/src/
├── main.rs                        # Main executable entry point & initialization
├── lib.rs                         # Tauri application builder & plugin registration
├── app_state.rs                   # Shared, thread-safe application state container
│
├── capture/                       # Screen Capture Subsystem
│   ├── mod.rs                     # Public API: ScreenCapturer trait, errors
│   ├── engine.rs                  # xcap capture implementation
│   ├── frame.rs                   # CapturedFrame, RGBA buffer, display bounds
│   └── platform/                  # OS-specific low-level fallbacks
│       ├── macos.rs               # ScreenCaptureKit / CoreGraphics direct bindings
│       └── windows.rs             # DXGI Desktop Duplication & GDI fallbacks
│
├── window/                        # Window Lifecycle & Multi-Monitor Coordination
│   ├── mod.rs                     # WindowManager trait & public commands
│   ├── pool.rs                    # Pre-warmed window pool (create, hide, show)
│   ├── coordinator.rs             # Display enumeration & coordinate mapping
│   └── native/                    # Low-level OS window tweaks
│       ├── macos.rs               # NSPanel, NSWindowCollectionBehavior, window level
│       └── windows.rs             # Win32 extended styles (WS_EX_TOPMOST, etc.)
│
├── hotkey/                        # Global Keyboard Shortcut Subsystem
│   ├── mod.rs                     # HotkeyManager, default shortcut mappings
│   ├── listener.rs                # tauri-plugin-global-shortcut event handler
│   └── parser.rs                  # Human-readable string <-> Hotkey code mapping
│
├── tray/                          # System Tray & Menu Bar Integration
│   ├── mod.rs                     # Tray menu setup, icons, and event routing
│   └── actions.rs                 # Handlers for "Capture", "Settings", "Quit"
│
├── ocr/                           # Offline Optical Character Recognition
│   ├── mod.rs                     # OcrEngine trait & error types
│   ├── macos.rs                   # Apple Vision Framework (VNRecognizeTextRequest)
│   └── windows.rs                 # Windows.Media.Ocr (WinRT bindings)
│
├── clipboard/                     # Native OS Clipboard Interop
│   ├── mod.rs                     # ClipboardService trait
│   └── arboard.rs                 # arboard implementation for RGBA bitmap & text
│
├── protocol/                      # Custom Protocol Handler ('carambola://')
│   ├── mod.rs                     # URI scheme registration and routing
│   └── handler.rs                 # Streaming in-memory RGBA/PNG buffers to webview
│
├── storage/                       # Local Settings & Screenshot Export
│   ├── mod.rs                     # Public storage API
│   ├── config.rs                  # User preferences JSON (hotkeys, save directory)
│   └── export.rs                  # PNG/JPEG encoding and file saving
│
└── commands/                      # Tauri IPC Command Bindings (Rust -> Frontend)
    ├── mod.rs                     # Command registry
    ├── capture.rs                 # get_displays, trigger_capture
    ├── export.rs                  # copy_to_clipboard, save_screenshot_to_disk
    ├── ocr.rs                     # extract_text_from_region
    └── window.rs                  # hide_overlay, toggle_eco_mode
```

---

## 3. Frontend (`src/`) Detailed Structure

```
src/
├── main.tsx                       # React DOM entry point
├── App.tsx                        # Main router / display manager
│
├── components/
│   ├── overlay/                   # Transparent Screenshot Overlay Components
│   │   ├── OverlayContainer.tsx   # Root wrapper, full-screen bounds, event root
│   │   ├── LayeredCanvas.tsx      # Multi-layer HTML5 Canvas compositing element
│   │   ├── SelectionBox.tsx       # Selection boundary visual & resize handles
│   │   ├── LoupeMagnifier.tsx     # Zoom loupe with sub-pixel crosshair & RGB color
│   │   └── DimensionBadge.tsx     # Pixel dimension indicator (e.g. 1920 × 1080)
│   │
│   ├── toolbar/                   # Floating Annotation Toolbar
│   │   ├── FloatingToolbar.tsx    # Responsive floating container anchored to crop
│   │   ├── ToolGroup.tsx          # Grouped buttons (Vector tools vs. Actions)
│   │   ├── ToolButton.tsx         # Individual tool button with active indicator
│   │   ├── ColorPicker.tsx        # Palette selector (pre-defined swatches + hex)
│   │   ├── StrokeWidthPicker.tsx  # Slider / presets (2px, 4px, 8px)
│   │   └── ActionButtons.tsx      # Copy, Save, OCR, Upload, Cancel buttons
│   │
│   ├── editor/                    # In-Place Vector Editing Helpers
│   │   ├── InlineTextInput.tsx    # ContentEditable element for text annotations
│   │   └── ResizeHandle.tsx       # 8-point interactive resize handles
│   │
│   └── common/                    # Shared UI Primitives
│       ├── Icon.tsx               # Lucide / SVG icon renderer
│       ├── Tooltip.tsx            # Minimal tooltip for keyboard shortcuts
│       └── Toast.tsx              # Transient feedback (e.g., "Copied to clipboard")
│
├── hooks/
│   ├── useCanvasRenderer.ts       # requestAnimationFrame drawing loop & layers
│   ├── useCropGesture.ts          # Mouse drag, selection creation, and boundary clamps
│   ├── useAnnotationHistory.ts    # Undo/redo stack and vector state management
│   ├── useDisplayInfo.ts          # Queries current display bounds and DPI scale
│   ├── useShortcuts.ts            # Local key listeners (Esc, Ctrl+Z, Ctrl+C, Ctrl+S)
│   └── useLoupe.ts                # Real-time pixel sampling for the magnifier
│
├── lib/
│   ├── canvas/                    # Canvas Drawing Utilities
│   │   ├── renderLayers.ts        # Orchestrates background, mask, vector layers
│   │   ├── drawShapes.ts          # Arrow, rectangle, ellipse vector algorithms
│   │   ├── smoothStroke.ts        # Catmull-Rom spline smoother for freehand pen
│   │   ├── pixelShaders.ts        # Offscreen canvas blur and mosaic algorithms
│   │   └── geometry.ts            # Point-in-rect, handle hit-testing, clamping
│   │
│   └── ipc/                       # Tauri v2 Backend Communication
│       ├── commands.ts            # Strongly-typed invoke() command wrappers
│       ├── events.ts              # Tauri event listeners (hotkeys, window show)
│       └── protocol.ts            # carambola:// URL formatting
│
└── types/                         # Shared TypeScript Definitions
    ├── annotations.ts             # Vector elements: Arrow, Rect, Pen, Text, Blur
    ├── crop.ts                    # Rect, Point, CropState, HandleType
    ├── display.ts                 # DisplayMetadata, DisplayBounds, ScaleFactor
    └── config.ts                  # UserPreferences, Theme, ToolType
```

---

## 4. Key Configuration Files Specification

### 4.1 `src-tauri/tauri.conf.json`
Key properties to guarantee high-performance transparent overlays:

```json
{
  "$schema": "https://raw.githubusercontent.com/tauri-apps/tauri/dev/tooling/cli/schema.json",
  "productName": "Carambola",
  "version": "0.1.0",
  "identifier": "io.carambola.app",
  "build": {
    "frontendDist": "../dist",
    "devUrl": "http://localhost:1420",
    "beforeDevCommand": "npm run dev",
    "beforeBuildCommand": "npm run build"
  },
  "app": {
    "macOSPrivateApi": true,
    "windows": [
      {
        "label": "overlay-main",
        "title": "Carambola Overlay",
        "transparent": true,
        "decorations": false,
        "alwaysOnTop": true,
        "skipTaskbar": true,
        "resizable": false,
        "visible": false,
        "fullscreen": false
      }
    ],
    "security": {
      "csp": "default-src 'self' carambola: asset:; img-src 'self' carambola: asset: data: blob:; style-src 'self' 'unsafe-inline';"
    }
  },
  "plugins": {
    "global-shortcut": {
      "registerOnStartup": false
    }
  }
}
```

### 4.2 `src-tauri/Cargo.toml`
Carefully selected, verified dependencies:

```toml
[package]
name = "carambola"
version = "0.1.0"
description = "Lightweight cross-platform screenshot and annotation application"
edition = "2021"

[dependencies]
tauri = { version = "2.0", features = ["tray-icon", "protocol-asset"] }
tauri-plugin-global-shortcut = "2.0"
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
xcap = "0.0.9"
arboard = "3.4"
image = { version = "0.25", default-features = false, features = ["png", "jpeg"] }
log = "0.4"
env_logger = "0.11"
thiserror = "1.0"
parking_lot = "0.12"

[target.'cfg(target_os = "windows")'.dependencies]
windows = { version = "0.58", features = [
    "Win32_UI_WindowsAndMessaging",
    "Win32_Graphics_Gdi",
    "Win32_Graphics_Dxgi",
    "Win32_System_ProcessStatus",
    "Media_Ocr",
    "Graphics_Imaging",
    "Foundation"
] }

[target.'cfg(target_os = "macos")'.dependencies]
objc2 = "0.5"
objc2-foundation = "0.2"
objc2-app-kit = "0.2"
objc2-vision = "0.2"
core-graphics = "0.24"
```

---

## 5. Separation of Concerns & Boundary Rules

1. **Rust Backend Responsibilities:**
   - Owns system permissions, hardware access, and OS APIs (display capture, hotkeys, tray, native clipboard, file I/O, native OCR).
   - Manages window creation, display coordinate mapping, and visibility toggling.
   - Holds the authoritative raw image buffers in memory.
   - Never implements UI styling or direct pixel-by-pixel canvas drawing.

2. **Frontend Responsibilities:**
   - Owns the interactive visual presentation, mouse gesture handling, and crop geometry calculations.
   - Manages non-destructive vector annotations, stroke smoothing, and user palette state.
   - Renders the magnifier loupe and floating toolbars.
   - Emits export commands with crop coordinates and vector trees back to Rust.
   - Never performs direct disk writes or invokes OS APIs directly.

