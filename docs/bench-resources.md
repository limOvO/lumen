# Resource comparison — Lumen (egui rewrite) vs Loupe vs the Tauri build

Measured on this machine (i9-13900F, 32 threads) under Xvfb 1440x900 with
**software GL (llvmpipe)** and `LIBGL_ALWAYS_SOFTWARE=1` for all three apps, so
the numbers are comparable with each other but pessimistic in absolute terms
(no GPU driver memory/threads). Process-tree totals include helper processes
(WebKit web/network processes for the Tauri build, image workers for Loupe).

Reproduce: `scripts/bench-resources.sh 3` (image: `/tmp/qa-big.jpg`, 4000x3000).

| Metric (median of 3) | **Lumen (egui)** | Lumen (Tauri/WebKit) | Loupe 51.2 |
|---|---|---|---|
| Process-tree PSS | **147 MB** | 435 MB | 229 MB |
| Process-tree RSS | **201 MB** | 708 MB | 348 MB |
| Anonymous (Pss_Anon) | 77 MB | 244 MB | **70 MB** |
| Peak RSS (VmHWM) | **395 MB** | 798 MB | 644 MB |
| Threads | **68** | 231 | 147 |
| Open file descriptors | **16** | 122 | 98 |
| Idle CPU (viewer, no input) | **0.0 %** | 2.7 % | 5.5 % |
| Idle CPU (editor) | **0.0 %** | 2.5 % | 2.8 % |
| Time to window | 81 ms | 85 ms | 166 ms |
| Time to photo | **81 ms** | 482 ms | 232 ms (4000x3000: 347 ms) |
| Binary on disk | 15 MB | 4 MB + system WebKit | 7 MB + system GTK |

Notes:
- The egui build wins on every memory metric in this harness, and is completely
  idle when nothing happens (event-driven: no timer loop), which is why idle CPU
  is 0.0 % instead of the 2–6 % the other two keep burning.
- Anonymous memory is essentially tied with Loupe (77 vs 70 MB); our extra PSS is
  mostly the software-GL llvmpipe libraries and their worker threads, which a
  real GPU run does not carry.
- The Tauri build's totals include a WebKitWebProcess and NetworkProcess (the
  egui build has neither).
- Time-to-photo: egui 81 ms vs Loupe 232 ms (1.3 MP) / 347 ms (12 MP) vs Tauri
  482/515 ms. Photoload benchmark: `scripts/bench-egui.sh`, `scripts/bench-vs-loupe.sh`.
