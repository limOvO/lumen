# Lumen 流明

[English](README.md) · **繁體中文**

一個夠輕量的 Linux 圖片檢視器，順手還能標註。
單一 4 MB 執行檔、零 runtime 依賴、約 80 ms 出現視窗——可以直接取代系統預裝的 GNOME 圖片檢視器（Loupe）。

![Lumen 操作示範：預覽 → 換圖 → 編輯 → 標註](docs/showcase/demo.gif)

| 預覽 | 編輯 |
|---|---|
| ![預覽](docs/showcase/preview.png) | ![編輯](docs/showcase/edit.png) |

## 為什麼用 Lumen

| | **Lumen** | **影像檢視器**（GNOME，Loupe 51.2） |
|---|---|---|
| **技術** | Tauri 2（Rust ＋ 系統 WebView）＋ 原生 HTML/CSS | GTK4 / libadwaita（Rust） |
| **程式大小** | **4.0 MB** 單一 ELF | 6.8 MB（套件 8.1 MiB） |
| **視窗出現** | **80 ms**（1400×934）· **83 ms**（1200 萬像素） | 146 ms · 151 ms |
| **看到圖片** | 490 ms · 508 ms | **214 ms** · **343 ms** |
| **看圖時記憶體**（PSS） | 266 MB · 310 MB | **115 MB** · **140 MB** |
| **執行期依賴** | webkit2gtk-4.1（141 MiB）＋ GTK3（54 MiB） | GTK4（57 MiB）＋ libadwaita（5.4）＋ glycin（19.6） |
| **功能** | 看圖、**裁切**、**調整大小**、**壓到指定 KB**、**標註** | 看圖、縮放、旋轉、翻轉 |
| **標註** | 畫筆、直線、矩形、圓框、兩點式箭頭、文字；顏色／粗細／透明度／虛線 | — |
| **裁切／縮放／壓縮** | ✅ | — |
| **EXIF／GPS 面板** | ✅ 完整 EXIF（相機、鏡頭、曝光、GPS） | 僅檔案屬性 |
| **資料夾瀏覽** | ✅ 內建瀏覽器，`←`/`→` 逛同資料夾 | 一次一張 |
| **復原** | 80 步，全程非破壞性 | — |
| **存檔隱私** | 存檔時移除 EXIF/GPS | 不適用（沒有存檔） |
| **語言** | 繁中、簡中、英文 | 所有 GNOME 語言 |

「影像檢視器」就是 GNOME 應用程式選單對 Loupe 的稱呼（`org.gnome.Loupe.desktop`、`/usr/bin/loupe`），
也就是這台機器預裝的那個檢視器。

實測環境：CachyOS、13th Gen Intel i9-13900F、Xvfb 1440×900 軟體渲染
（`GSK_RENDERER=cairo`、`WEBKIT_DISABLE_COMPOSITING_MODE=1`），4 次取中位數；
「看到圖片」= 60 fps 錄影中第一個出現圖片的畫格。有 GPU 的桌面上兩者都會更快，
這張表是同等條件下的相對比較。可用 [`scripts/bench-vs-loupe.sh`](scripts/bench-vs-loupe.sh) 重跑（需 Xvfb 與已安裝的 Loupe）。

**老實說**：單就看圖，Loupe 貼圖更快、記憶體更省；Lumen 的價值是
**4 MB 單一執行檔**、視窗快一倍，而且把「裁切、縮放、壓縮到指定大小、標註」
這些原本要再開一個 App 的事都收進來。

## 安裝

**下載執行檔**（Linux x86_64）：

```bash
curl -L -o lumen https://github.com/limOvO/lumen/releases/latest/download/lumen
chmod +x lumen && ./lumen photo.jpg
```

需要 `gtk3` 與 `webkit2gtk-4.1`（Arch：`gtk3`、`webkit2gtk-4.1`）。

**從原始碼建置**（Rust 1.80+）：

```bash
./scripts/build.sh                       # 產出 dist/lumen
./scripts/install.sh                     # 安裝到 ~/.local（不需 sudo）
./scripts/install.sh --desktop-shortcut  # 加桌面捷徑
./scripts/install.sh --set-default       # 設為圖片預設開啟程式
./scripts/uninstall.sh                   # 解除安裝（只動 ~/.local）
```

會安裝 `~/.local/bin/lumen`、應用程式選單項目與圖示；細節見 [packaging/INSTALL.md](packaging/INSTALL.md)。

## 使用

```bash
lumen                 # 空視窗：Ctrl+O 或拖放開檔
lumen photo.jpg       # 直接開一張圖
lumen ~/Pictures      # 開啟資料夾
```

Lumen 分成兩個階段：開檔預設在**預覽**（乾淨畫面、縮放、`←`/`→` 翻頁、`I` 資訊浮卡）。
按右上角「**編輯**」或 `E` 進入編輯（工具列、面板、標註樣式條）；`Esc` 回到預覽。

| 鍵 | 動作 | 鍵 | 動作 |
|---|---|---|---|
| `Ctrl+O` / `Ctrl+S` / `Ctrl+Shift+S` | 開啟／儲存／另存 | `Ctrl+Z` / `Ctrl+Shift+Z` | 復原／重做 |
| `←` `→` | 上一張／下一張 | `Home` `End` | 第一張／最後一張 |
| `+` `-` / `0` | 放大／縮小／適應視窗 | 滾輪、`Ctrl+滾輪` | 以游標為錨點縮放 |
| `V` `B` `R` `O` `A` `L` `T` `C` | 移動／畫筆／矩形／圓框／箭頭／直線／文字／裁切 | `Delete` | 刪除選取標註 |
| `Space`（按住）或中鍵拖曳 | 平移畫布 | `Esc` | 取消選取／回預覽 |
| `I` | 資訊浮卡 | `F` / `?` | 全螢幕／快捷鍵說明 |

**格式**：JPEG、PNG、WebP、GIF（第一格）、BMP、TIFF、AVIF、SVG、ICO；輸出 JPEG（自動鋪白底）、PNG、WebP。

**語言**：跟隨系統——繁中、簡中，其他語言一律英文；可用 `LUMEN_LANG=zh-Hans`（或 `zh-Hant`、`en`）固定。

## 授權

MIT。
