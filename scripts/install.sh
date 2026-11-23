#!/usr/bin/env bash
# Install Lumen for the current user — no sudo, no system files touched.
#
#   scripts/install.sh                 # ~/.local/bin + icons + app-menu entry
#   scripts/install.sh --desktop-shortcut  # ...and put a launcher on the desktop
#   scripts/install.sh --set-default   # ...and make Lumen the default image viewer
#                                      # (alias: --default-viewer)
#   scripts/install.sh --no-default    # explicit: leave existing default alone
#
# Layout (all under $HOME/.local, override with PREFIX=...):
#   bin/lumen
#   share/icons/hicolor/<size>/apps/lumen.png
#   share/applications/lumen.desktop
#
# Re-running is safe: files are overwritten in place, caches refreshed.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PREFIX="${PREFIX:-$HOME/.local}"
BIN_DIR="$PREFIX/bin"
APP_DIR="$PREFIX/share/applications"
ICON_ROOT="$PREFIX/share/icons/hicolor"

SET_DEFAULT=0
DESKTOP_SHORTCUT=0
for arg in "$@"; do
  case "$arg" in
    --set-default|--default-viewer|--mime) SET_DEFAULT=1 ;;
    --no-default) SET_DEFAULT=0 ;;
    --desktop-shortcut|--shortcut) DESKTOP_SHORTCUT=1 ;;
    -h|--help)
      sed -n '2,13p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
      exit 0
      ;;
    *) echo "install.sh: 未知參數 $arg" >&2; exit 2 ;;
  esac
done

# --- locate the binary -----------------------------------------------------
SOURCE_BIN=""
for candidate in "$ROOT/dist/lumen" "$ROOT/src-tauri/target/release/lumen"; do
  if [ -x "$candidate" ]; then
    SOURCE_BIN="$candidate"
    break
  fi
done
if [ -z "$SOURCE_BIN" ]; then
  echo "install.sh: 找不到已建置的執行檔。" >&2
  echo "            先執行 scripts/build.sh，或 cargo build --release。" >&2
  exit 1
fi

echo "▸ 安裝 Lumen"
echo "  來源：$SOURCE_BIN"
echo "  目標前綴：$PREFIX"

# --- binary ----------------------------------------------------------------
mkdir -p "$BIN_DIR"
install -m 0755 "$SOURCE_BIN" "$BIN_DIR/lumen"
echo "  ✓ 執行檔：$BIN_DIR/lumen"

# --- icons -----------------------------------------------------------------
missing_icons=0
if [ -d "$ROOT/packaging/icons/hicolor" ]; then
  while IFS= read -r -d '' icon; do
    size="$(basename "$(dirname "$(dirname "$icon")")")"
    dest="$ICON_ROOT/$size/apps"
    mkdir -p "$dest"
    install -m 0644 "$icon" "$dest/lumen.png"
  done < <(find "$ROOT/packaging/icons/hicolor" -mindepth 3 -maxdepth 3 -name 'lumen.png' -print0)
  echo "  ✓ 圖示：$ICON_ROOT/<size>/apps/lumen.png"
else
  missing_icons=1
  echo "  ! 找不到 packaging/icons/hicolor（略過圖示），可先跑 scripts/gen-icons.sh" >&2
fi

# 絕對不要自己產生 $ICON_ROOT/index.theme。
# 使用者層級的 hicolor index.theme 會「蓋掉」系統的 /usr/share/icons/hicolor/index.theme
# （GTK 依 XDG 搜尋順序只讀第一個找到的）。若那份 index.theme 只有 Directories=
# 而沒有每個目錄的 [<size>/apps] Size= 區段，GTK 會判定所有 hicolor 目錄無效，
# 於是桌面／應用程式清單裡幾乎每個 app 圖示都退成通用佔位圖示。
# 放進使用者目錄的圖示不需要 index.theme 也能被找到，快取只是加速，少了沒關係。
if [ -f "$ICON_ROOT/index.theme" ] && ! grep -q '^Size=' "$ICON_ROOT/index.theme"; then
  echo "  ! 清掉不完整的 $ICON_ROOT/index.theme（會遮蔽系統 hicolor 設定）" >&2
  rm -f "$ICON_ROOT/index.theme" "$ICON_ROOT/icon-theme.cache"
fi
if command -v gtk-update-icon-cache >/dev/null && [ -f "$ICON_ROOT/index.theme" ]; then
  gtk-update-icon-cache -f -t "$ICON_ROOT" >/dev/null 2>&1 || true
  echo "  ✓ 已更新圖示快取"
fi

# --- desktop entry ---------------------------------------------------------
mkdir -p "$APP_DIR"
# Absolute Exec/TryExec: ~/.local/bin is not always on the session PATH seen by
# the desktop shell. Fall back to the packaged portable file if it is missing.
if [ -f "$ROOT/packaging/lumen.desktop" ]; then
  sed -e "s|^Exec=lumen|Exec=$BIN_DIR/lumen|" \
      -e "s|^TryExec=lumen|TryExec=$BIN_DIR/lumen|" \
      "$ROOT/packaging/lumen.desktop" > "$APP_DIR/lumen.desktop"
  chmod 0644 "$APP_DIR/lumen.desktop"
  echo "  ✓ 桌面捷徑：$APP_DIR/lumen.desktop"
else
  echo "  ! 找不到 packaging/lumen.desktop（略過桌面捷徑）" >&2
fi

if command -v update-desktop-database >/dev/null; then
  update-desktop-database "$APP_DIR" >/dev/null 2>&1 || true
  echo "  ✓ 已更新 desktop database"
fi

# --- optional: launcher icon on the desktop --------------------------------
desktop_dir() {
  if command -v xdg-user-dir >/dev/null; then
    local d
    d="$(xdg-user-dir DESKTOP 2>/dev/null || true)"
    if [ -n "$d" ] && [ "$d" != "$HOME" ] && [ -d "$d" ]; then
      printf '%s\n' "$d"
      return
    fi
  fi
  local dirs_file="$HOME/.config/user-dirs.dirs"
  if [ -f "$dirs_file" ]; then
    local line val
    line="$(grep -E '^XDG_DESKTOP_DIR=' "$dirs_file" 2>/dev/null | head -1 || true)"
    if [ -n "$line" ]; then
      val="${line#*=}"
      val="${val%\"}"; val="${val#\"}"
      val="${val/\$HOME/$HOME}"
      if [ -d "$val" ]; then printf '%s\n' "$val"; return; fi
    fi
  fi
  for candidate in "$HOME/Desktop" "$HOME/桌面"; do
    if [ -d "$candidate" ]; then printf '%s\n' "$candidate"; return; fi
  done
}

if [ "$DESKTOP_SHORTCUT" = 1 ]; then
  ddir="$(desktop_dir || true)"
  if [ -n "${ddir:-}" ] && [ -f "$APP_DIR/lumen.desktop" ]; then
    install -m 0755 "$APP_DIR/lumen.desktop" "$ddir/Lumen.desktop"
    # GNOME/Nautilus only launches desktop files that are marked trusted.
    if command -v gio >/dev/null; then
      gio set "$ddir/Lumen.desktop" metadata::trusted true >/dev/null 2>&1 || true
    fi
    echo "  ✓ 桌面捷徑：$ddir/Lumen.desktop"
  else
    echo "  ! 找不到桌面資料夾（或缺少 .desktop），略過桌面捷徑" >&2
  fi
fi

# --- optional: default image viewer ---------------------------------------
if [ "$SET_DEFAULT" = 1 ]; then
  if command -v xdg-mime >/dev/null && [ -f "$APP_DIR/lumen.desktop" ]; then
    for mime in image/png image/jpeg image/webp image/gif image/bmp image/tiff image/avif image/svg+xml; do
      xdg-mime default lumen.desktop "$mime" 2>/dev/null || true
    done
    echo "  ✓ 已設為預設開圖程式（xdg-mime，可用 --no-default 不設定）"
  else
    echo "  ! 找不到 xdg-mime，略過預設程式設定" >&2
  fi
fi

echo
echo "✓ Lumen 已安裝"
echo "  執行：$BIN_DIR/lumen [圖片...]"
case ":$PATH:" in
  *":$BIN_DIR:"*) ;;
  *) echo "  提示：$BIN_DIR 不在 PATH 中，可加入 shell 設定檔。" ;;
esac
echo "  移除：scripts/uninstall.sh"
