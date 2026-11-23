#!/usr/bin/env bash
# Build Lumen for this machine.
#
#   scripts/build.sh              # release build -> dist/lumen
#   scripts/build.sh --check      # cargo check only (fast feedback)
#
# CARGO_HOME lives inside the project (`.cache/cargo-home`) because ~/.cargo is
# read-only on this machine and /tmp is wiped between shell sessions.
# Override with CARGO_HOME=... if you have a warm cargo home elsewhere.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

export CARGO_HOME="${CARGO_HOME:-$ROOT/.cache/cargo-home}"
mkdir -p "$CARGO_HOME"

MODE="release"
for arg in "$@"; do
  case "$arg" in
    --check) MODE="check" ;;
    --debug) MODE="debug" ;;
    -h|--help)
      sed -n '2,9p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
      exit 0
      ;;
    *) echo "build.sh: 未知參數 $arg" >&2; exit 2 ;;
  esac
done

command -v cargo >/dev/null || { echo "build.sh: 找不到 cargo" >&2; exit 1; }

echo "▸ CARGO_HOME=$CARGO_HOME"
echo "▸ cargo $MODE build"

START=$(date +%s)
if [ "$MODE" = "check" ]; then
  cargo check --release --manifest-path src-tauri/Cargo.toml
  echo "✓ check 通過（$(( $(date +%s) - START ))s）"
  exit 0
fi

PROFILE="$MODE"
cargo build --release --manifest-path src-tauri/Cargo.toml
BIN="src-tauri/target/release/lumen"
[ -x "$BIN" ] || { echo "build.sh: 建置後找不到 $BIN" >&2; exit 1; }

mkdir -p dist
install -m 0755 "$BIN" dist/lumen

SIZE_H=$(du -h dist/lumen | cut -f1)
SIZE_B=$(stat -c %s dist/lumen)
echo
echo "✓ 建置完成（$(( $(date +%s) - START ))s）"
echo "  產物：dist/lumen（$SIZE_H，$SIZE_B bytes）"
file dist/lumen | sed 's/^/  /'

echo
echo "▸ smoke test: dist/lumen --version"
if OUT=$(timeout 15 ./dist/lumen --version 2>&1); then
  echo "  $OUT"
  case "$OUT" in
    *Lumen*) echo "  ✓ 可執行檔可啟動" ;;
    *) echo "  ✗ --version 輸出非預期" >&2; exit 1 ;;
  esac
else
  echo "  ✗ --version 執行失敗" >&2
  exit 1
fi

echo
echo "下一步：scripts/install.sh（安裝到 ~/.local，不需要 sudo）"
