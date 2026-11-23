# Lumen 流明

[English](README.md) · **繁體中文**

一個夠輕量的 Linux 圖片檢視器，順手還能標註。
單一 4 MB 執行檔、零 runtime 依賴、約 55 ms 出現視窗——可以直接取代系統預裝的 GNOME 圖片檢視器（Loupe）。

![Lumen 操作示範：預覽 → 換圖 → 編輯 → 標註](docs/showcase/demo.gif)

| 預覽 | 編輯 |
|---|---|
| ![預覽](docs/showcase/preview.png) | ![編輯](docs/showcase/edit.png) |

## 為什麼用 Lumen

| | **Lumen** | **Loupe**（GNOME 圖片檢視器） |
|---|---|---|
| **技術** | Tauri 2（Rust ＋ 系統 WebView）＋ 原生 HTML/CSS | GTK4 / libadwaita（Rust） |
| **體積** | 單一 **4 MB** ELF，只需 `webkit2gtk` ＋ GTK3 | 整套 GNOME/GTK4（或 Flatpak runtime） |
| **冷啟動** | 約 55 ms 出視窗、約 350 ms 看到圖 | 很快，但底層較重 |
| **能做什麼** | 看圖、**裁切**、**調整大小**、**壓縮**、**標註** | 看圖、縮放、旋轉、翻轉 |
| **標註** | 畫筆、直線、矩形、圓框、兩點式箭頭、文字；顏色／粗細／透明度／虛線 | — |
| **裁切／調整大小／壓到指定 KB** | ✅ | — |
| **EXIF／GPS 面板** | ✅ 完整 EXIF（相機、鏡頭、快門、光圈、ISO、GPS） | 僅基本檔案資訊 |
| **資料夾瀏覽** | ✅ 內建瀏覽器，`←`/`→` 逛同資料夾 | 一次開一張 |
| **復原** | 80 步，編輯全程非破壞性 | — |
| **存檔隱私** | 存檔時移除 EXIF/GPS | 不適用（沒有存檔） |
| **語言** | 繁中、簡中、英文（跟隨系統） | 所有 GNOME 語言 |
| **桌面整合** | 一支腳本：選單項目、圖示、桌面捷徑、可選設為預設 | GNOME 內建 |

Loupe 本身很好用，身為 GNOME 核心程式整合也更深；Lumen 是給「想要更小、更快、零依賴，
而且能直接裁切／縮放／壓縮／標註，不用再開第二個 App」的情況。

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
