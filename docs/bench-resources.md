# Benchmarks — Lumen (egui rewrite) vs Loupe vs the Tauri/WebKit build

All numbers measured on this machine (i9-13900F, 32 threads, CachyOS) under a
headless Xvfb 1440x900 session with **software GL (llvmpipe)** for every app, so
the comparison is fair but absolute numbers are pessimistic (no GPU driver, no
compositor). Memory is a **process-tree total** (includes helper processes:
WebKitWebProcess/NetworkProcess for the Tauri build, image workers for Loupe).

- Time to window / time to photo: `scripts/bench-egui.sh 3`, `scripts/bench-vs-loupe.sh <img> 3`
- Memory / CPU / threads / FDs: `scripts/bench-resources.sh 3`
- Image switching: `scripts/parity/verify.py` check P3
- Binary size: `ls -l dist/lumen-egui`

## Time to photo (4000×3000 and 1400×934 samples, median of 3)

| | 1.3 MP | 12 MP |
|---|---|---|
| **Lumen (egui)** | **87 ms** | **85 ms** |
| Lumen (Tauri/WebKit) | 444 ms | 464 ms |
| Loupe 51.2 | 204 ms | 331 ms |

Window shown: Lumen 85–87 ms, Tauri 85–94 ms, Loupe 138–141 ms.
Switching to the next/previous image: median **3 ms** (neighbour previews are
prefetched and JPEGs decode through libjpeg-turbo's scaled IDCT).

## Memory & CPU (4000×3000 image, median of 3)

| Metric | **Lumen (egui)** | Lumen (Tauri/WebKit) | Loupe 51.2 |
|---|---|---|---|
| PSS (process tree) | **135 MB** | 338 MB | 229 MB |
| RSS (process tree) | **206 MB** | 619 MB | 346 MB |
| Anonymous memory | 78 MB | 166 MB | **67 MB** |
| Peak RSS (VmHWM) | **400 MB** | 697 MB | 644 MB |
| Threads | **68** | 186 | 147 |
| Open file descriptors | **41** | 197 | 122 |
| Idle CPU — viewer | **0.0 %** | 2.2 % | 5.2 % |
| Idle CPU — editor | **0.0 %** | 2.2 % | 2.7 % |
| Binary on disk | 9.6 MB | 4 MB + WebKit 141 MB + GTK 54 MB | 8 MB + GTK 54 MB + glycin 20 MB |

Notes:
- The egui build is event-driven: when nothing happens it does not repaint, so
  idle CPU is literally 0 ticks. The other two keep a compositor/animation timer
  running.
- Anonymous memory (what the program itself owns) is close to Loupe (78 vs
  67 MB); most of our extra PSS is the software-GL llvmpipe libraries and their
  worker threads, which a real GPU run does not carry.
- The Tauri build spawns a full WebKitWebProcess and NetworkProcess; the egui
  build has neither.

## Reproduce

```bash
scripts/bench-egui.sh 3
scripts/bench-vs-loupe.sh /tmp/qa-small.jpg 3
scripts/bench-vs-loupe.sh /tmp/qa-big.jpg 3
scripts/bench-resources.sh 3
python3 scripts/parity/verify.py        # includes P3 (switch latency) + P2 (vs WebView)
```
