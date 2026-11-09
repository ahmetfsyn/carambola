# Project structure

The repository contains the native Windows application and a separate static
marketing website.

```text
carambola/
├── .github/workflows/
│   ├── deploy-website.yml       # Static website deployment
│   ├── release-native.yml      # Tagged native Windows x64 release
│   └── windows-native-tests.yml
├── native-app/
│   ├── assets/                 # Windows icon and compiled version resource input
│   ├── src/core/               # Image, geometry, annotation, settings, OCR state
│   ├── src/platform/           # Windows and experimental OS adapters
│   ├── tools/                  # Release metadata checks and user-facing tools
│   ├── ui/                     # Slint overlay
│   ├── vendor/winit-0.30.13/   # Cargo-patched Winit source
│   ├── Cargo.toml              # Authoritative application version
│   └── Cargo.lock
├── website/                    # Independent React/Vite marketing site
├── ARCHITECTURE.md
├── IMPLEMENTATION_PLAN.md
├── NATIVE_MIGRATION_PLAN.md
├── PROJECT_STRUCTURE.md
├── README.md
├── RELEASE_NOTES.md
└── TECHNICAL_DECISIONS.md
```

Rust commands run from `native-app/`. Website commands run from `website/` and
are used only by the GitHub Pages workflow. Build outputs, dependency folders,
and release artifacts are ignored rather than committed.
