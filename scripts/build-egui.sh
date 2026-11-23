#!/usr/bin/env bash
# Build the Rust + wgpu + egui rewrite into dist/lumen-egui.
set -euo pipefail
PROJ="$(cd "$(dirname "$0")/.." && pwd)"
cd "$PROJ"
export CARGO_HOME="$PROJ/.cache/cargo-home"
cargo build --release --manifest-path egui-app/Cargo.toml
mkdir -p dist
cp egui-app/target/release/lumen-egui-app dist/lumen-egui
./dist/lumen-egui --version >/dev/null 2>&1 || true
echo "built dist/lumen-egui ($(du -h dist/lumen-egui | cut -f1))"
