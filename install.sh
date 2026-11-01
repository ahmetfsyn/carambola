#!/usr/bin/env sh
#
# Install a Carambola package that was built locally with the Tauri bundler.
# This script never downloads or executes remote content.
#
# Usage: bash install.sh
#
# Expected inputs (produced by `npm run tauri build` on Linux):
#   src-tauri/target/release/bundle/deb/*.deb
#   src-tauri/target/release/bundle/rpm/*.rpm
#   src-tauri/target/release/bundle/appimage/*.AppImage

set -eu

ROOT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
BUNDLE_DIR="$ROOT_DIR/src-tauri/target/release/bundle"

fail() {
    printf 'Error: %s\n' "$*" >&2
    exit 1
}

info() {
    printf '%s\n' "$*"
}

if [ "$(uname -s)" != "Linux" ]; then
    fail "install.sh is for Linux only. Windows users should run the generated Carambola NSIS installer, and macOS users should use the .dmg/.app."
fi

if [ ! -d "$BUNDLE_DIR" ]; then
    fail "No bundle directory found at $BUNDLE_DIR. Build the packages first with: npm run tauri build"
fi

find_one() {
    # $1 = subdirectory, $2 = glob
    find "$BUNDLE_DIR/$1" -maxdepth 1 -type f -name "$2" 2>/dev/null | sort | head -n 1
}

DEB=$(find_one deb "*.deb")
RPM=$(find_one rpm "*.rpm")
APPIMAGE=$(find_one appimage "*.AppImage")

if [ -z "$DEB" ] && [ -z "$RPM" ] && [ -z "$APPIMAGE" ]; then
    fail "No .deb, .rpm, or AppImage found under $BUNDLE_DIR. Run 'npm run tauri build' on Linux first."
fi

install_deb() {
    command -v apt-get >/dev/null 2>&1 || fail "A .deb package was found, but apt-get is not available. Install it manually with your distribution's package manager: $DEB"
    info "Installing Debian package (apt-get will resolve runtime dependencies):"
    info "  $DEB"
    run_privileged apt-get install -y "$DEB"
    info "Carambola installed. Launch it from your application menu; it runs in the background with a tray icon."
}

install_rpm() {
    if command -v dnf >/dev/null 2>&1; then
        info "Installing RPM package with dnf:"
        info "  $RPM"
        run_privileged dnf install -y "$RPM"
    elif command -v zypper >/dev/null 2>&1; then
        info "Installing RPM package with zypper:"
        info "  $RPM"
        run_privileged zypper --non-interactive install "$RPM"
    elif command -v rpm >/dev/null 2>&1; then
        info "Installing RPM package with rpm (dependencies must already be installed):"
        info "  $RPM"
        run_privileged rpm -Uvh "$RPM"
    else
        fail "An .rpm package was found, but no RPM package manager is available. Install it manually: $RPM"
    fi
    info "Carambola installed. Launch it from your application menu; it runs in the background with a tray icon."
}

run_privileged() {
    if [ "$(id -u)" -eq 0 ]; then
        "$@"
    else
        command -v sudo >/dev/null 2>&1 || fail "sudo is required to install this package. Run as root or install it manually."
        sudo "$@"
    fi
}

check_appimage_runtime() {
    command -v ldconfig >/dev/null 2>&1 || fail "ldconfig is required to verify AppImage runtime dependencies."
    ldconfig -p 2>/dev/null | grep -q 'libwebkit2gtk-4.1\.so' \
        || fail "WebKitGTK 4.1 is required (libwebkit2gtk-4.1-0 / webkit2gtk4.1). Install it with your package manager, then retry."
    ldconfig -p 2>/dev/null | grep -q 'libgtk-3\.so' \
        || fail "GTK 3 is required (libgtk-3-0 / gtk3). Install it with your package manager, then retry."
    if ! ldconfig -p 2>/dev/null | grep -Eq 'libayatana-appindicator3\.so|libappindicator3\.so'; then
        info "Warning: no AppIndicator library was detected. The tray icon may not appear on this desktop."
    fi
}

install_appimage() {
    check_appimage_runtime
    BIN_DIR="${XDG_BIN_HOME:-$HOME/.local/bin}"
    TARGET="$BIN_DIR/carambola.AppImage"
    mkdir -p "$BIN_DIR"
    cp "$APPIMAGE" "$TARGET"
    chmod 0755 "$TARGET"
    info "AppImage copied to: $TARGET"
    info "Ensure $BIN_DIR is on your PATH, then run: carambola.AppImage"
    info "If FUSE is unavailable, run it with: $TARGET --appimage-extract-and-run"
    info "This script does not create a desktop entry or Start on Boot registration for the AppImage."
}

if [ -n "$DEB" ] && command -v apt-get >/dev/null 2>&1; then
    install_deb
elif [ -n "$RPM" ] && { command -v dnf >/dev/null 2>&1 || command -v zypper >/dev/null 2>&1 || command -v rpm >/dev/null 2>&1; }; then
    install_rpm
elif [ -n "$APPIMAGE" ]; then
    install_appimage
elif [ -n "$DEB" ]; then
    fail "A .deb was found, but apt-get is unavailable. Install it manually with a compatible package manager: $DEB"
elif [ -n "$RPM" ]; then
    fail "An .rpm was found, but no compatible RPM package manager is available. Install it manually: $RPM"
else
    fail "No package format matches this environment. Generate a compatible .deb/.rpm or use the AppImage under $BUNDLE_DIR."
fi
