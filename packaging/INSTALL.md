# 安裝與建置（Lumen）

Lumen 是 Tauri 2 應用程式：Rust 外殼 + 原生 HTML/CSS/ESM 前端（無 bundler、無 npm 依賴）。
以下步驟全部安裝在**使用者家目錄**，不需要 `sudo`，也不會改動系統套件或系統路徑。

## 0. 環境需求

| 項目 | 版本／說明 |
|---|---|
| 作業系統 | Linux（GNOME Wayland 已驗證）；CachyOS / Arch 適用 |
| Rust | 1.99 以上（`cargo`、`rustc`） |
| 系統函式庫 | `webkit2gtk-4.1`、`gtk3`（Tauri 2 Linux 依賴） |
| 選用工具 | ImageMagick（重新產生圖示）、`desktop-file-validate`、`xdg-mime` |

Arch 系發行版安裝依賴：

```bash
sudo pacman -S --needed rust webkit2gtk-4.1 gtk3 imagemagick
```

## 1. 建置

```bash
git clone <repo> lumen && cd lumen
scripts/build.sh
```

`scripts/build.sh` 會：

1. 以專案內的 `CARGO_HOME=.cache/cargo-home` 執行
   `cargo build --release --manifest-path src-tauri/Cargo.toml`
   （這台機器 `~/.cargo` 唯讀、`/tmp` 不持久，所以 cargo home 放在專案內；
   可用 `CARGO_HOME=/path cargo build ...` 覆寫，但**不要**用 `/tmp`）
2. 把產物複製到 `dist/lumen`
3. 印出檔案大小與 `file` 型別
4. 跑 smoke test：`dist/lumen --version`（應印出 `Lumen 0.1.0`）

只想要型別檢查、不等連結：

```bash
scripts/build.sh --check
```

第一次建置會下載並編譯全部 crate（數分鐘）；之後增量建置約 30 秒。

## 2. 安裝（使用者層級）

```bash
scripts/install.sh                # 安裝，不改預設開圖程式
scripts/install.sh --set-default  # 同時設為預設開圖程式（xdg-mime）
```

安裝內容（前綴預設 `~/.local`，可用 `PREFIX=...` 覆寫）：

```
~/.local/bin/lumen                                  # 執行檔（0755）
~/.local/share/icons/hicolor/<16..512>/apps/lumen.png
~/.local/share/applications/lumen.desktop           # Exec/TryExec 已改為絕對路徑
```

安裝時會順帶更新 desktop database 與 icon cache。`install.sh` 是**冪等**的：
重複執行只會覆蓋同名檔案，不會產生重複項目。

`--set-default` 會用 `xdg-mime default lumen.desktop ...` 把以下 MIME 指向 Lumen：
`image/png`、`image/jpeg`、`image/webp`、`image/gif`、`image/bmp`、
`image/tiff`、`image/avif`、`image/svg+xml`。
不想改就別加參數（或明確加 `--no-default`）。之後想還原，改用桌面環境的
「設定 → 預設應用程式」把「影像檢視器」換回 Loupe 即可。

若 `~/.local/bin` 不在 `PATH`，把下面一行加進 `~/.bashrc` / `~/.zshrc`：

```bash
export PATH="$HOME/.local/bin:$PATH"
```

## 3. 執行

```bash
lumen                       # 開空視窗
lumen ~/Pictures/a.jpg      # 直接開圖
lumen ~/Pictures            # 開資料夾中的第一張圖
lumen file:///path/a.png    # 檔案管理器傳入的 file:// URL 也接受
lumen --help                # 說明
lumen --version             # 版本
```

### 單一實例行為

Lumen 是單一實例應用：已經開著時，從檔案管理器再開一張圖不會啟動第二個視窗，
而是把路徑送進既有視窗（Rust 端透過 `$XDG_RUNTIME_DIR/lumen-$USER.sock` 轉送，
再以 `lumen://open` 事件交給前端）。若 runtime 目錄不可寫（沙盒環境），
會自動退回 `$TMPDIR`。第二次啟動的行程會立即結束（exit 0）。

## 4. 解除安裝

```bash
scripts/uninstall.sh            # 移除執行檔、圖示、桌面捷徑
scripts/uninstall.sh --keep-icons
```

只刪除 `scripts/install.sh` 放進 `~/.local` 的檔案；**不會**動到任何圖片、
編輯結果或設定檔。移除後會一併更新 desktop database 與 icon cache。
重複執行也安全（沒有東西可刪時會直接說明）。

## 5. 重新產生圖示（選用）

圖示是純向量繪製，再用 ImageMagick 輸出各尺寸，不需要外部圖檔：

```bash
scripts/gen-icons.sh
```

輸出：

- `src-tauri/icons/icon.png`（512 母版）＋ `16/32/48/64/128/256/512`、`128x128@2x`
- `packaging/icons/hicolor/<size>/apps/lumen.png`（安裝用）
- `.cache/icons/contact-sheet.png`（各尺寸對照，不隨發佈）

## 6. 疑難排解

- **`dconf-CRITICAL ... 唯讀的檔案系統`**：在受限沙盒裡 GNOME 設定後端不可寫，
  只是警告，不影響功能。
- **`Failed to setup app: 唯讀的檔案系統`**：`$HOME` 或 `XDG_*` 目錄不可寫。
  Lumen 啟動時會自動把不可寫的 `XDG_DATA_HOME` / `XDG_CACHE_HOME` /
  `XDG_CONFIG_HOME` / `XDG_STATE_HOME` 換到可寫的暫存目錄；
  若仍失敗，請確認 `$XDG_RUNTIME_DIR` 或 `$TMPDIR` 可寫。
- **建置很慢**：首次要編譯 Tauri 全部依賴。之後 `scripts/build.sh --check` 很快。
- **沒有桌面圖示**：確認 `packaging/icons/hicolor/` 存在，或先跑
  `scripts/gen-icons.sh`，再重新 `scripts/install.sh`。
- **整個桌面所有 app 圖示都變成灰底佔位圖（不只 Lumen）**：`$HOME/.local/share/icons/hicolor/`
  裡有一份**殘缺的 `index.theme`**（只有 `Directories=`、沒有 `<size>/apps` 的 `Size=`
  區段）。使用者層級的 hicolor `index.theme` 會蓋掉 `/usr/share/icons/hicolor/index.theme`，
  GTK 因此判定所有 hicolor 目錄無效，桌面、應用程式清單、dock 全體退成通用圖示。
  修法：`rm -f ~/.local/share/icons/hicolor/index.theme ~/.local/share/icons/hicolor/icon-theme.cache`
  後重新登入（新版本 `install.sh` / `uninstall.sh` 已會自動清掉這種檔案，也不再自行產生）。

## 7. 開發時的檔案歸屬

- `src-tauri/**`、`packaging/**`、`scripts/**`：原生外殼與打包
- `src/**`：前端（`src/index.html` 由 Tauri 直接以 `frontendDist` 載入，
  改完重新整理視窗即可，不需要 bundler）
