# Architecture spikes

Throwaway shells used to measure what Lumen would look like **without the WebView**.
Both open an image, fit/zoom, pan and step through the folder with ← →.

| spike | stack | binary | small photo (1400×934) | 12 MP photo (4000×3000) |
|---|---|---|---|---|
| `gtk3-viewer` | Rust + GTK3 + cairo / gdk-pixbuf | 0.4 MB | window 54 ms · photo 54 ms · PSS **24 MB** | 104 ms · 104 ms · **95 MB** |
| `egui-viewer` | Rust + wgpu + egui (eframe) | 12 MB | 71 ms · 71 ms · **116 MB** | 129 ms · 129 ms · **155 MB** |
| (reference) current Lumen | Tauri 2 + WebKitGTK | 4 MB | 81 ms · 447 ms · 263 MB | 86 ms · 469 ms · 269 MB |
| (reference) Loupe 51.2 | GTK4 / libadwaita | 6.8 MB | 146 ms · 212 ms · 113 MB | 148 ms · 339 ms · 137 MB |

Measured on CachyOS, i9-13900F, Xvfb 1440×900 with software rendering, median of 4 runs;
window/photo = first frame with the window / the picture. `egui-viewer` used `WGPU_BACKEND=gl`
(llvmpipe) because the headless session has no Vulkan device — on a real GPU its numbers differ.

## Build / run

```bash
# GTK3 spike (uses the crates already cached by the main app)
cd spikes/gtk3-viewer
CARGO_HOME="$PWD/../../.cache/cargo-home" cargo build --release --offline
DISPLAY=:0 ./target/release/lumen-native /path/to/photo.jpg

# wgpu + egui spike (fetches eframe/wgpu from crates.io)
cd spikes/egui-viewer
CARGO_HOME="$PWD/../../.cache/cargo-home" cargo build --release
DISPLAY=:0 ./target/release/lumen-egui /path/to/photo.jpg
```

Keys in both: `←`/`→` previous/next, `+`/`-` zoom, `0` fit, `q` quit.
