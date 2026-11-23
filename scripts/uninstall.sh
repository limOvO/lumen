#!/usr/bin/env bash
# Remove a user-local Lumen install created by scripts/install.sh.
#
#   scripts/uninstall.sh              # remove binary, icons, desktop entry
#   scripts/uninstall.sh --keep-icons # keep only the binary/desktop entry? (no, see below)
#
# Only touches $HOME/.local (override with PREFIX=...). No sudo.
set -euo pipefail

PREFIX="${PREFIX:-$HOME/.local}"
BIN_DIR="$PREFIX/bin"
APP_DIR="$PREFIX/share/applications"
ICON_ROOT="$PREFIX/share/icons/hicolor"

KEEP_ICONS=0
for arg in "$@"; do
  case "$arg" in
    --keep-icons) KEEP_ICONS=1 ;;
    -h|--help)
      sed -n '2,7p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
      exit 0
      ;;
    *) echo "uninstall.sh: 未知參數 $arg" >&2; exit 2 ;;
  esac
done

removed=0

if [ -e "$BIN_DIR/lumen" ]; then
  rm -f "$BIN_DIR/lumen" && echo "  ✓ 已移除 $BIN_DIR/lumen"
  removed=1
fi

if [ -e "$APP_DIR/lumen.desktop" ]; then
  rm -f "$APP_DIR/lumen.desktop" && echo "  ✓ 已移除 $APP_DIR/lumen.desktop"
  removed=1
fi

# Desktop launcher (only if install.sh --desktop-shortcut created one).
for ddir in "$(xdg-user-dir DESKTOP 2>/dev/null || true)" "$HOME/Desktop" "$HOME/桌面"; do
  if [ -n "${ddir:-}" ] && [ -f "$ddir/Lumen.desktop" ]; then
    rm -f "$ddir/Lumen.desktop" && echo "  ✓ 已移除 $ddir/Lumen.desktop"
    removed=1
  fi
done

if [ "$KEEP_ICONS" = 0 ] && [ -d "$ICON_ROOT" ]; then
  icon_count=0
  while IFS= read -r -d '' icon; do
    rm -f "$icon"
    icon_count=$((icon_count + 1))
  done < <(find "$ICON_ROOT" -mindepth 3 -maxdepth 3 -name 'lumen.png' -print0)
  if [ "$icon_count" -gt 0 ]; then
    echo "  ✓ 已移除 $icon_count 個 hicolor 圖示"
    removed=1
  fi
fi

# 不完整的 index.theme 會遮蔽系統 hicolor，導致全桌面 app 圖示變成佔位圖。
if [ -f "$ICON_ROOT/index.theme" ] && ! grep -q '^Size=' "$ICON_ROOT/index.theme"; then
  rm -f "$ICON_ROOT/index.theme" "$ICON_ROOT/icon-theme.cache"
fi
if command -v gtk-update-icon-cache >/dev/null && [ -f "$ICON_ROOT/index.theme" ]; then
  gtk-update-icon-cache -f -t "$ICON_ROOT" >/dev/null 2>&1 || true
fi
if command -v update-desktop-database >/dev/null; then
  update-desktop-database "$APP_DIR" >/dev/null 2>&1 || true
fi

echo
if [ "$removed" = 0 ]; then
  echo "· 沒有找到已安裝的 Lumen（$PREFIX），無事可做。"
else
  echo "✓ Lumen 已移除（未刪除任何圖片、設定或專案檔）"
fi
