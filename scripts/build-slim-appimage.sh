#!/usr/bin/env bash
set -e

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$DIR"

echo "==> Building frontend..."
npm run build

echo "==> Building release binary..."
cargo build --release --manifest-path src-tauri/Cargo.toml

APPDIR="$DIR/build/AppDir"
OUT_DIR="$DIR/build"
mkdir -p "$APPDIR/usr/bin" "$OUT_DIR"

echo "==> Preparing slim AppDir..."
cp "$DIR/src-tauri/target/release/madlinux" "$APPDIR/usr/bin/madlinux"
strip --strip-all "$APPDIR/usr/bin/madlinux"

cat << 'EOF' > "$APPDIR/AppRun"
#!/bin/sh
HERE="$(dirname "$(readlink -f "$0")")"
exec "$HERE/usr/bin/madlinux" "$@"
EOF
chmod +x "$APPDIR/AppRun"

cp "$DIR/src-tauri/icons/128x128.png" "$APPDIR/madlinux.png"

cat << 'EOF' > "$APPDIR/madlinux.desktop"
[Desktop Entry]
Name=MadLinux
Exec=madlinux
Icon=madlinux
Type=Application
Categories=Game;Development;Utility;
EOF

echo "==> Generating ultra-compact AppImage..."
APPIMAGE_PLUGIN="$HOME/.cache/tauri/linuxdeploy-plugin-appimage.AppImage"
if [ ! -f "$APPIMAGE_PLUGIN" ]; then
    mkdir -p "$HOME/.cache/tauri"
    curl -L --fail -o "$APPIMAGE_PLUGIN" "https://github.com/linuxdeploy/linuxdeploy-plugin-appimage/releases/download/continuous/linuxdeploy-plugin-appimage-x86_64.AppImage"
    chmod +x "$APPIMAGE_PLUGIN"
fi

cd "$OUT_DIR"
"$APPIMAGE_PLUGIN" --appdir="$APPDIR"

SLIM_APPIMAGE=$(ls -t "$OUT_DIR"/MadLinux*.AppImage | head -n 1)
cp -f "$SLIM_APPIMAGE" "$DIR/MadLinux_0.1.0_amd64.AppImage"
cp -f "$SLIM_APPIMAGE" "$HOME/Downloads/MadLinux_0.1.0_amd64.AppImage"

echo "==> Done! Lightweight AppImage generated at:"
ls -lh "$DIR/MadLinux_0.1.0_amd64.AppImage"
