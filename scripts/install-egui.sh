#!/usr/bin/env bash
# Install the egui rewrite next to the main app (no sudo, ~/.local only).
#   scripts/install-egui.sh [--desktop-shortcut] [--set-default]
set -euo pipefail
PROJ="$(cd "$(dirname "$0")/.." && pwd)"
BIN="$PROJ/dist/lumen-egui"
[ -x "$BIN" ] || { echo "build first: scripts/build-egui.sh"; exit 1; }
BIN_DIR="$HOME/.local/bin"
APP_DIR="$HOME/.local/share/applications"
ICON_DIR="$HOME/.local/share/icons/hicolor"
mkdir -p "$BIN_DIR" "$APP_DIR" "$ICON_DIR/256x256/apps"
install -m 755 "$BIN" "$BIN_DIR/lumen-egui"

SRC_ICON="$PROJ/src-tauri/icons/256x256.png"
[ -f "$SRC_ICON" ] || SRC_ICON="$PROJ/packaging/lumen.png"
if [ -f "$SRC_ICON" ]; then
  install -m 644 "$SRC_ICON" "$ICON_DIR/256x256/apps/lumen-egui.png"
fi

DESKTOP="$APP_DIR/lumen-egui.desktop"
sed -e "s|@BIN@|$BIN_DIR/lumen-egui|" \
    -e "s|@ICON@|lumen-egui|" \
    "$PROJ/packaging/lumen-egui.desktop" > "$DESKTOP"
chmod 644 "$DESKTOP"

update-desktop-database "$APP_DIR" >/dev/null 2>&1 || true
gtk-update-icon-cache -f -t "$ICON_DIR" >/dev/null 2>&1 || true

if [ "${1:-}" = "--desktop-shortcut" ] || [ "${2:-}" = "--desktop-shortcut" ]; then
  DESK="$HOME/桌面"
  [ -d "$DESK" ] || DESK="$HOME/Desktop"
  if [ -d "$DESK" ]; then
    cp "$DESKTOP" "$DESK/lumen-egui.desktop"
    chmod +x "$DESK/lumen-egui.desktop"
    gio set "$DESK/lumen-egui.desktop" metadata::trusted true 2>/dev/null || true
    echo "desktop shortcut: $DESK/lumen-egui.desktop"
  fi
fi

if [ "${1:-}" = "--set-default" ] || [ "${2:-}" = "--set-default" ]; then
  xdg-mime default lumen-egui.desktop image/jpeg image/png image/webp image/gif image/bmp image/tiff image/avif image/svg+xml image/x-icon
  echo "set as default image viewer"
fi
echo "installed: $BIN_DIR/lumen-egui"
echo "  run: lumen-egui path/to/image.jpg"
