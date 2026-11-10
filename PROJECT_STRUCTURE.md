# Project structure

The repository contains the native Windows application and a separate static
marketing website.

```text
carambola/
├── .github/workflows/
│   ├── release-native.yml      # Tagged native Windows x64 release
│   └── windows-native-tests.yml
├── installer/windows/
│   ├── carambola.iss            # Inno Setup installer definition
│   └── INSTALLATION.txt
├── native-app/
│   ├── assets/                 # Windows icon and compiled version resource input
│   ├── src/core/               # Image, geometry, annotation, settings, OCR state
│   ├── src/platform/           # Windows and experimental OS adapters
│   ├── tools/                  # Release metadata checks and user-facing tools
│   ├── ui/                     # Slint overlay
│   ├── vendor/winit-0.30.13/   # Cargo-patched Winit source
│   ├── Cargo.toml              # Technical version source; zero patch maps to public MAJOR.MINOR
│   └── Cargo.lock
├── website/                    # Independent React/Vite marketing site
├── scripts/                    # Local Windows installer build command
├── ARCHITECTURE.md
├── IMPLEMENTATION_PLAN.md
├── NATIVE_MIGRATION_PLAN.md
├── PROJECT_STRUCTURE.md
├── README.md
├── RELEASE_NOTES.md
└── TECHNICAL_DECISIONS.md
```

Rust commands run from `native-app/`. Website commands run from `website/`;
the marketing site is deployed independently through Cloudflare Workers. Build
outputs, dependency folders, and release artifacts are ignored rather than
committed.
