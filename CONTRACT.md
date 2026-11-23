# Lumen — 介面契約 (CONTRACT)

輕量級圖片檢視／標註器，取代 GNOME 預裝 Loupe。Tauri 2 + 原生 HTML/CSS/ESM（無打包器）。
**所有像素工作在 WebView 內完成；Rust 只負責檔案、EXIF、視窗。**

## 0. 鐵則

- 不使用框架、不使用 bundler、不引入任何 npm 依賴。原生 ESM，瀏覽器直接載入。
- 任何模組都只能改自己名下的檔案（見 §9 檔案歸屬）。跨檔需求→寫進 CONTRACT 或留訊息給 Lead。
- 座標系：**標註物件一律存在「原圖像素座標」**（與縮放、裁切無關）。螢幕座標只存在 viewport 層。
- 所有 DOM id 與 class 由 `ui-shell` 決定，但 §5 列出的 id 是**契約**，不得改名。
- 顏色／尺寸全部走 CSS 變數（`src/css/tokens.css`），不要在 JS 寫死色值。
- 完成後自檢：`grep` 確認沒有 `TODO`、沒有 `throw new Error('TODO')`。

## 1. 檔案結構

```
src/
  index.html            # ui-shell
  css/tokens.css        # ui-shell
  css/app.css           # ui-shell
  js/main.js            # lead    （開機、Tauri 橋接、渲染排程、actions）
  js/core/state.js      # lead    （狀態容器 + undo/redo + 訂閱）
  js/core/loader.js     # editor-core
  js/core/viewport.js   # editor-core
  js/core/annotate.js   # editor-core
  js/core/interaction.js# editor-core（畫布上的所有指標互動）
  js/core/export.js     # editor-core
  js/ui/*.js            # ui-shell
src-tauri/              # desktop
packaging/              # desktop
```

## 2. state（`core/state.js`，由 Lead 實作，其他人只讀／透過 API 改）

```js
state = {
  doc: null,        // Doc（見 §3）目前載入的圖片
  files: [],        // [{path,name,size,mtime}] 同資料夾圖片，已自然排序
  index: -1,
  tool: 'move',     // 'move'|'brush'|'rect'|'ellipse'|'arrow'|'text'|'crop'
  style: { color:'#ff4d4f', size:8, opacity:1, fill:false, fontSize:36 },
  objects: [],      // 標註物件（§4）
  selectedId: null,
  crop: null,       // {x,y,w,h,ratio} 原圖 px；ratio 為 null=自由
  view: { scale:1, tx:0, ty:0, fitMode:'fit' }, // fitMode:'fit'|'actual'|'manual'
  output: {         // 匯出設定（右側面板用）
    format:'auto',  // 'auto'（沿用來源）| 'jpeg'|'png'|'webp'
    quality:0.85,
    maxDim:null,    // 長邊上限 px，null=不縮
    targetKB:null,  // 目標檔案大小，null=用 quality
    scale:1,        // 直接倍率（200% 放大用；maxDim 只會縮小）
  },
  dirty:false,      // 有未儲存編輯
  busy:false,       // 載入/匯出中
  compare:false,    // 按壓比較原圖
}
```

API（Lead 提供，其他人直接 import）：

```js
import { state, subscribe, emit, commit, undo, redo, canUndo, canRedo,
         setTool, setStyle, addObject, updateObject, removeObject, clearObjects,
         setView, setCrop, setOutput, setDirty, markSaved } from './state.js';
```

- `commit(label)`：把目前 `objects`/`crop` 快照推入 undo 堆疊並 `dirty=true`；**每次完成一筆操作後呼叫一次**（不是每個 pointermove）。
- `subscribe(fn)`：狀態變更回呼，`fn(scope)`，`scope ∈ 'all'|'doc'|'view'|'objects'|'tool'|'style'|'crop'|'output'|'busy'|'files'|'undo'`。Lead 的 main.js 已訂閱並負責重繪，UI 模組也自行訂閱需要的部分。
- `emit(scope)` 由 state 內部呼叫；模組不要自己 emit。

## 3. loader（`core/loader.js`）

```js
export async function loadDocument(path) -> Doc
export async function loadFromBytes(bytes, name, path) -> Doc   // 拖放／剪貼簿
export function sniffMime(bytes) -> string    // 由 magic bytes 判斷
export async function readFileBytes(path) -> Uint8Array  // 走 invoke('read_image_file')
export const IMAGE_EXTS

// Doc = {
//   path:string, name:string, dir:string, size:number, mtime:number,
//   width:number, height:number,          // EXIF 方向校正後的顯示尺寸
//   format:string,                        // 'JPEG'|'PNG'|'WebP'...
//   exif:[{key,label,value}],
//   bitmap:ImageBitmap,                   // 已套用 EXIF Orientation 的點陣
//   blob:Blob, objectUrl:string,          // 供 <img> 或 canvas 用
//   mime:string,
// }
```

- 讀檔用 `invoke('read_image_file', { path })`（回傳 ArrayBuffer/Uint8Array）。
- 解碼用 `createImageBitmap(blob, { imageOrientation:'from-image' })`；SVG 走 `<img>` + `drawImage`，`width/height` 取 natural。
- 巨圖保護：`width*height > 80e6` 時回退到 `createImageBitmap(blob,{resizeWidth})` 並把 `doc.decodedScale` 記下（其餘模組不需理會，只影響 `doc.bitmap` 解析度；標註仍用原圖座標 → 繪製時乘 `doc.decodedScale`）。
- 一定要 `revokeObjectURL` 舊 doc（`disposeDoc(doc)`）。
- 失敗要 throw 帶人話的 Error（main.js 會 toast）。

## 4. annotate（`core/annotate.js`）

物件格式（**原圖像素座標**）：

```js
{ id, type:'brush'|'rect'|'ellipse'|'arrow'|'text',
  // 幾何
  points:[{x,y,w?}],        // brush：帶速度寬度 w
  x,y,w,h,                  // rect/ellipse/arrow/text 的外框（arrow: (x,y)->(x+w,y+h)）
  text:string,              // text
  // 樣式快照
  style:{ color, size, opacity, fill, fontSize } }
```

```js
export function createObject(type, p, style) -> obj          // p 為原圖座標
export function drawObjects(ctx, objects, view, dpr, opts)   // opts:{preview,selectedId,showHandles}
export function hitTest(objects, imgPt, tolPx) -> obj|null   // tolPx 以「螢幕」像素給，內部換算
export function objectBounds(obj) -> {x,y,w,h}
export function handlesFor(obj) -> [{name:'nw'|'n'|'ne'|'e'|'se'|'s'|'sw'|'w', x,y}]
export function moveObject(obj, dx, dy)                      // dx/dy 原圖 px
export function resizeObject(obj, handle, imgPt, keepRatio)
export function cloneObject(obj) -> obj
```

- brush 沿用知識庫「手绘笔刷画布」的畫法：三點中點二次貝茲、`lineWidth` 取**起點**的 `w`、`lineCap/lineJoin='round'`、速度→線寬（`target = size*(1-min(1,v/1.5)*0.82)`、`w = prev + (target-prev)*0.38`、下限 0.65*size/8）。每一筆存點陣列，不做像素烘焙。
- 效能：objects 變更後才全量重繪；正在畫的筆畫由 interaction 增量畫在 overlay canvas，不要每 move 全量重繪。三次貝茲／文字在 `ctx` 上以 `view.scale` 換算。
- `drawObjects` 只畫，不處理選取框（選取框由 interaction 用 `handlesFor` 畫，樣式：1px 亮色 + 8 個 8px 方形把手，隨縮放保持螢幕尺寸）。
- 顏色／透明度：`globalAlpha = style.opacity`，填滿僅當 `style.fill`（brush/arrow 忽略）。

## 5. viewport（`core/viewport.js`）+ 畫布契約

```js
export function fitScale(imgW, imgH, viewW, viewH) -> number
export function clampScale(s) -> number            // 0.02 .. 16
export function screenToImage(view, sx, sy) -> {x,y}
export function imageToScreen(view, sx, sy) -> {x,y}
export function drawBackground(ctx, w, h, dpr)     // 棋盤格（CSS 變數 --checker-a/--checker-b）
export function drawDoc(ctx, doc, view, dpr)       // 只畫圖片本體
export function zoomAt(view, sx, sy, factor) -> view  // 以指標為錨點縮放
export function panBy(view, dx, dy) -> view
```

畫布 DOM（`ui-shell` 必須提供，`editor-core` 依賴）：

```html
<div id="stage">                     <!-- 黑底中央區，position:relative, overflow:hidden -->
  <canvas id="board"></canvas>       <!-- 主畫布：背景+圖片+已提交標註 -->
  <canvas id="overlay"></canvas>     <!-- 互動層：當前筆畫、選取框、裁切框 -->
</div>
```

- 兩張 canvas 都 `position:absolute; inset:0; width:100%; height:100%`，DPR 上限 2。
- `main.js` 負責 resize 與 `render()`；`interaction.js` 只畫 overlay 的臨時內容。
- 圖片在 stage 內置中：`view.tx/ty` 是「圖片左上角在 canvas CSS px 的位置」。

## 6. interaction（`core/interaction.js`）

```js
export function attachInteraction({ stage, board, overlay, requestRender, onCursor })
```

負責全部畫布內手勢，並在需要時呼叫 `commit()` / `addObject()` / `setCrop()`：

| 情境 | 行為 |
|---|---|
| `tool==='move'` | 空白處拖曳＝平移；點物件＝選取；拖選取物件＝移動；把手＝縮放；雙擊文字＝編輯 |
| `tool==='brush'` | pointerdown 建筆、move 增量畫、up `addObject`+`commit` |
| `rect/ellipse/arrow` | 拖出外框，up 才建立物件（<3px 丟棄）；Shift 鎖正方/45° |
| `text` | 點擊建立文字物件並進入行內輸入（`contenteditable` overlay，由 ui-shell 提供 #textEditor） |
| `tool==='crop'` | 拖出新裁切框；8 把手調整；框內拖曳移動；Shift 鎖比例；框外壓暗；三分線 |
| 任何工具 | `Ctrl/Cmd+滾輪` 或 `+/-` 縮放；`Space` 按住暫變平移；中鍵拖曳平移 |
| Delete/Backspace | 刪除選取物件（crop 模式下＝清除裁切框） |

- 全部用 Pointer Events + `setPointerCapture`；`touch-action:none`。
- `requestRender()` 由 main.js 注入（throttle 到 rAF）。
- 游標：`onCursor(cssCursor)` 設定 `stage.style.cursor`。

## 7. export（`core/export.js`）

```js
export async function flatten(doc, objects, { crop, maxDim }) -> { canvas, width, height }
export async function encode(canvas, format, quality) -> { blob, bytes, mime }
export async function estimateSize(doc, objects, output, crop) -> { bytes, width, height }
export async function saveDoc({ path, blob }) -> number      // invoke('write_file')
export function outputExtension(format, doc) -> 'jpg'|'png'|'webp'
export function suggestedName(doc, format) -> string         // 例 IMG_1234-edit.jpg
```

- `flatten`：新 canvas → drawImage(圖片，套用 crop+maxDim) → 標註按同一比例重畫（`ctx.scale`）→ 保持原圖比例。
- **JPEG 必須先鋪白底**（沒有 alpha）。PNG/WebP 保留透明。
- `estimateSize` 用 0.5 倍預覽 canvas 估（速度快），回傳實際全尺寸 bytes 的估算值不準時，允許在面板按下「估算」時做一次全尺寸 encode（debounce 400ms，由 ui-shell 控制）。
- 目標大小模式（`output.targetKB`）：質量二分搜尋最多 8 次，回傳 `{blob, quality}`；提供 `export async function encodeToTarget(canvas, format, targetKB) -> {blob, quality, bytes}`。

## 8. Tauri 指令（已實作於 `src-tauri/src/main.rs`，前端直接 invoke）

| 指令 | 參數 | 回傳 |
|---|---|---|
| `read_image_file` | `{path}` | `ArrayBuffer` |
| `inspect_image` | `{path}` | `{path,name,dir,size,mtime,width,height,format,exif[]}` |
| `list_images` | `{dir}` | `[{path,name,size,mtime}]` |
| `write_file` | `{path,data:number[]}` | `bytes` |
| `path_exists` | `{path}` | `bool` |
| `unique_path` | `{path}` | `string` |
| `open_external` | `{path}` | – |
| `startup_files` | – | `string[]` |
| `home_pictures` | – | `string` |
| `set_window_title` | `{title}` | – |
| `write_file_b64` | `{path, data}` | `bytes`（data 為 base64 字串） |
| `list_dirs` | `{dir}` | `[{path,name,size,mtime}]`（只回子資料夾） |

- 呼叫：`const invoke = window.__TAURI__?.core?.invoke`；**在純瀏覽器開發時必須有降級**（`main.js` 已提供 `bridge`，見下）。
- `main.js` 匯出 `bridge`：`{ invoke, isTauri }`，UI 模組一律用 `bridge.invoke`。

## 9. 檔案歸屬（嚴格）

| 擁有者 | 檔案 |
|---|---|
| lead | `src/js/main.js`, `src/js/core/state.js`, `CONTRACT.md`, `README.md` |
| editor-core | `src/js/core/loader.js`, `viewport.js`, `annotate.js`, `interaction.js`, `export.js` |
| ui-shell | `src/index.html`, `src/css/tokens.css`, `src/css/app.css`, `src/js/ui/*.js` |
| desktop | `src-tauri/src/main.rs`(可擴充), `src-tauri/icons/*`, `src-tauri/tauri.conf.json`, `packaging/*`, `scripts/*` |

只准改自己那幾行；需要別人的檔就發訊息給 Lead。

## 10. 必須實作的 UI 元素 id（契約）

```
#topbar  #btn-open  #btn-save  #btn-saveas  #btn-undo  #btn-redo
#file-name  #dirty-dot
#toolrail  #t-move #t-brush #t-rect #t-ellipse #t-arrow #t-text #t-crop
#stage #board #overlay #checker
#panel-tabs  #tab-info #tab-adjust #tab-crop
#panel-info  #panel-adjust  #panel-crop
#filmstrip  #film-items
#zoom-hud  #zoom-out #zoom-in #zoom-fit #zoom-actual #zoom-label
#browser-modal  #browser-list  #browser-path  #browser-up  #browser-open
#save-modal  #save-fields  #save-confirm  #save-cancel
#toasts
#textEditor   (絕對定位的 contenteditable，預設 display:none)
#empty-state  （沒有圖片時的引導）
```

## 11. 視覺規格（ui-shell 依此實作，先查 ui_design 知識庫）

- 深色工具型 App，整體 `#0e1013`；面板 `#16191f`；卡片/輸入框 `#1d2129`；分隔線 `rgba(255,255,255,.08)`
- 主色 `#ff5c5c`（標註/強調），次色 `#5b9dff`（選取/連結），成功 `#3ddc97`，警告 `#ffb457`
- 工具列 52px 寬直排；頂欄 46px；右側面板 300px；底片條 92px（可收合）；畫布棋盤格 `#141518`/`#191c22`
- 圓角 10px；按下 `scale(.97)`；面板/抽屜 180–240ms `cubic-bezier(.22,.61,.36,1)`
- 字體系統 UI 堆疊；數字（尺寸/大小/比例）用 `font-variant-numeric: tabular-nums`
- 鍵盤可達：所有按鈕 `:focus-visible` 有 2px 主色外框
- 小而精：不要陰影堆疊、不要玻璃擬態、不要漸層裝飾

## 12. 快捷鍵（ui-shell 顯示於「?」面板，interaction 與 main 實作）

`Ctrl+O` 開檔 · `←/→` 上/下一張 · **`V` 移動 / `B` 畫筆 / `R` 矩形 / `O` 圓框 / `A` 箭頭 / `T` 文字 / `C` 裁切** · `Ctrl+Z / Ctrl+Shift+Z` 復原/重做 · `Ctrl+S` 儲存 · `Ctrl+Shift+S` 另存 · `+/-` 縮放 · `0` 適應 · `Ctrl+0` 實際大小 · `F` 全螢幕 · `Esc` 取消選取/退出裁切 · `Delete` 刪除 · `Space` 平移 · `I` 資訊分頁

## 13. actions（`main.js` 提供，UI 模組一律透過它改狀態）

```js
// main.js 匯出
export const actions = {
  // 檔案
  openPath(path),                 // 載入並切換檔案（自動載入同資料夾清單）
  openBrowser(dir),               // 開資料夾瀏覽器（dir 省略=目前資料夾）
  browseTo(dir),                  // 瀏覽器內切換目錄
  openDroppedFiles(fileList),
  next(), prev(),
  save(),                         // 存回原檔（格式沿用輸出設定；覆寫需覆寫原檔）
  saveAs(opts),                   // {path, format, quality, maxDim, targetKB}
  // 編輯
  undo(), redo(),
  setTool(t), setStyle(patch),
  deleteSelection(), clearObjects(),
  setCrop(rect), clearCrop(),
  setOutput(patch), estimateSize(),   // 回傳 {bytes,width,height}
  // 視圖
  zoomIn(), zoomOut(), zoomFit(), zoomActual(), setZoom(scale),
  toggleFilmstrip(), toggleCompare(),
  render(), requestRender(),
  // 雜項
  toast(message, kind = 'info'),      // kind: info|ok|warn|err
  refreshPanels(),
}
```

## 14. UI 模組契約（ui-shell 實作，main.js 直接 import）

`src/index.html` 必須以 `<script type="module" src="js/main.js"></script>` 載入（manifest 不要複製到別處）。每個 ui 模組輸出名稱固定如下，message ctx 由 main.js 提供：

```js
// 所有 ui 模組：export function init(ctx)
// ctx = { state, actions, els, bridge, requestRender }
// els = { topbar, btnOpen, ..., stage, board, overlay, textEditor } 依 §10 id 轉 camelCase
export function init(ctx) {}
```

| 檔案 | export | 責任 |
|---|---|---|
| `ui/topbar.js` | `init` | 頂欄按鈕、檔名/未存點、工具提示 |
| `ui/toolrail.js` | `init` | 左工具列選中態、點擊→`actions.setTool` |
| `ui/panels.js` | `init` | 三個分頁 + 標註樣式區，訂閱 state |
| `ui/filmstrip.js` | `init` | 底片條縮圖、當前檔高亮、點擊切換、收合 |
| `ui/browser.js` | `init` | `#browser-modal` 資料夾瀏覽器，`openBrowser(dir)` 由 main 呼叫 |
| `ui/save-modal.js` | `init` | `#save-modal` 另存；提供 `openSaveDialog(defaults)` |
| `ui/toasts.js` | `init` + `toast(msg, kind)` | 通知堆疊 |
| `ui/shortcuts.js` | `init` | `?` 說明面板的顯示/隱藏（**不要**自己攔截全域快捷鍵，主鍵盤由 main.js 處理） |
| `ui/text-editor.js` | `show({ screenX, screenY, fontSize, color, text, onCommit, onCancel })`, `hide()`, `isActive()` | `#textEditor` 行內文字輸入 |

`interaction.js` 的 `attachInteraction(ctx)` 額外約定回傳 `{ paintOverlay() }`：main.js 每次 render 後呼叫它（overlay 由它負責清空與重畫）。

## 15. 階段（preview / edit）

```js
state.phase   // 'preview' | 'edit'，開圖預設 'preview'
state.peekInfo // 預覽階段是否用浮動卡片顯示資訊
setPhase(p) / setPeekInfo(on)   // core/state.js
```

- `main.js` 在 `<body>` 上加 `phase-preview` / `phase-edit` / `peek-info` / `no-doc`。
- `ui-phase` CSS 只依這些 class 顯示/隱藏外殼；不得自行改 `state.phase`（一律走 `actions.setPhase` / `actions.togglePhase` / `actions.peekInfo`）。
- 預覽階段：`#topbar`、`#toolrail`、`#panel`、`#filmstrip`、`#style-panel` 隱藏；`#zoom-hud` 保留。
  手勢：**滾輪縮放（以畫面中心為錨點，圖片永遠置中）**；**不可拖曳平移、不可雙擊**；
  進編輯只能按右上「編輯」按鈕（沒有 Tab／E／Enter 快捷鍵）。
- 背景：整個 stage 是純深色 `--bg`；棋盤格**只在圖片矩形內**、且僅當 `doc.hasAlpha` 為真時繪製
  （`loader.js` 提供 `doc.hasAlpha`），避免看圖時出現一大片繪圖畫布。
- 編輯階段：既有版面；`#btn-preview` 回預覽。
- 階段切換只走按鈕（右上「編輯」／頂欄「預覽」）；鍵盤只保留 `Esc` 回預覽（先清選取/裁切）與 `I` 預覽開資訊浮卡。
- 預覽階段的 `ui/phase.js` 只負責 chrome 與顯示，`?` 面板新增「階段」分組。


## 16. UI v2（2026-11 重新排布）

新增/變更：
- 頂欄：`#save-status`（`data-s="idle|saving|saved|error"`，`#save-status-text`）取代存檔 toast。
- 右側屬性面板：`#inspector > #panel`；拖寬把手 `#insp-sep > #insp-grip`（0 寬分隔、16×38 膠囊、240–420px、←→/Home/End、`localStorage['lumen.inspector-width']`）；收合 `#btn-insp-collapse`。
- 底部狀態列 `#statusbar`：`#status-info`（尺寸/格式/大小）、`#status-msg`（錯誤/提示，不再是浮動 toast）、`#status-right`（縮放% · 張數）。
- 標註樣式改為畫布底部浮動工具條 `#style-dock`（只有 brush/rect/ellipse/arrow/text 時顯示；與 `#zoom-hud` 垂直錯開，`bottom:56px`；寬度 `min(600px, 100%-96px)` 內可換行）。
- `#toasts` 與 `ui/toasts.js` 已移除；`actions.toast()` 仍存在但只寫入 `#status-msg`。
- 底片條 `#filmstrip` 橫跨 canvas＋inspector（`grid-area: film`），高度 76px、縮圖 52px；`#toolrail` 延伸到 filmstrip 列。

縮放/平移規則（`viewport.minScale` / `viewport.clampPan`）：
- 縮放下限＝`fitScale(doc, cssW-64, cssH-64)`；任何縮小路徑（滾輪、+/-、HUD、setZoom）都不可低於此值。
- 平移一律 `clampPan`：圖片小於視窗時該軸置中，否則夾在邊界內（不留空白）。
- 預覽：`scale > minScale` 時左鍵拖曳＝平移（cursor grab），在 fit 時完全 inert。


## 17. 修正與新行為（task-7）

- `interaction.js` 曾用本地函式 `applyView(view)` 覆蓋 `viewport.applyView(ctx, view)`，導致畫筆重繪時把 `state.view` 的 `scale/tx/ty` 寫成 `undefined`（畫面跳到 100%、註標位置錯亂）。已改名為 `pushView(view)`；**view 一律走 `setView`，canvas transform 走 `viewport.applyView(ctx, view)`**。
- 復原歷史改為 baseline 式：`commit()` 推入「操作前」快照，`undo/redo` 交換 baseline，修正「第一次 undo 沒反應」。
- 註譯取消填色：`#style-fill` 移除、`style.fill` 恆為 `false`，矩形／圓框一律描邊。
- 浮動樣式條閒置行為：`#style-dock.is-idle` → `opacity .42` + `translateY(58%)`，滑鼠進入/繪圖即喚回。
- 版面 grid 修正為 `"rail canvas sep inspector"`（分隔把手必須在畫布／面板交界，而不是視窗最右緣）。
- 編輯模式平移：移動工具可在任何位置拖曳平移（3px 門檻）；其他工具中鍵拖曳平移；所有平移皆 `clampPan`。


## 18. 修正（task-8）

- 版面不再有底片條：`body.phase-edit #filmstrip, body.no-doc #filmstrip { display:none }`；換圖走 `←/→` 或「開啟」。底片條 DOM/JS 保留但不在版面中。
- 分隔把手方向修正：右側面板向左拖變寬（`applyWidth(startWidth - dx)`），`ArrowLeft` 加寬、`ArrowRight` 變窄。
- 拖曳分隔把手不再閃爍：`ResizeObserver` 只同步畫布尺寸並**同步重繪**，`fitView` 以 140ms debounce 在尺寸穩定後才重跑。
- 浮動樣式條閒置行為：`syncTool()` 顯示 dock 時呼叫 `wakeDock()` 起算 2.2s，逾時加 `.is-idle`（`opacity .42` + `translateY(58%)`）。
- **注意**：`syncTool()` 曾引用未定義的 `wakeDock`，導致 `panels.js` 初始化中斷（空狀態不消失、資訊面板空白）。任何在 init 內新增的函式呼叫都要先確認宣告位置。


## 19. 兩點式箭頭（task-9）

- 箭頭資料：`points:[{x,y},{x,y}]` 為權威，`x/y/w/h` 鏡射（起點 + signed delta）；舊資料由 `arrowPoints()` 推算。
- `annotate.js` 匯出 `arrowPoints(obj)` / `setArrowPoint(obj, 'start'|'end', pt)`；繪製、`handlesFor`（僅 `start`/`end` 兩個）、`objectBounds`、`moveObject`、`resizeObject` 全部以 points 為準。
- `interaction.js`：箭頭工具為兩點式（第一次點擊只建 pending 與預覽、第二次點擊才 `addObject`+`commit`；拖曳 >6px 也可一筆完成；`Esc`／切工具取消 pending）。選取箭頭只畫兩顆圓形端點把手（9px，hover 12px），拖端點只動該端（最小長度 2px）；其他形狀維持 8 個方塊把手。


## 20. 手勢與樣式條行為（task-10）

- **編輯模式取消平移夾制**：`interaction.clampToViewport()` 與 `pan` 分支只在 `phase === 'preview'` 時 `clampPan`；編輯模式可把圖片拖出畫布。`main.clampView()` 同樣只在預覽階段生效（`zoomTo`／resize 共用）。
- 預覽規則不變：fit 時 inert、放大才可拖、拖到邊界停住、縮放下限＝fit。
- **樣式條喚醒策略**：`#style-dock` 顯示時預設 `.is-idle`（半透明＋下移 58%）。
  喚醒區＝**「醒著時的 dock 矩形」∪「睡著時的 dock 矩形」**（各向外 6px），也就是只有游標真的進入這條帶狀區域才 `wakeDock()`；
  `event.buttons !== 0`（正在畫圖／拖曳）一律不喚醒；離開 dock 420ms 或喚醒後 2.2s 無互動 → 回到 idle。


## 21. 縮放控制（task-11）

- `#zoom-hud` 不再是固定右下角浮層：`ui/zoom-hud.js` 依 `state.phase` 移動節點——
  編輯模式 append 到 `#toolrail`（`margin:auto auto 6px` 貼底、`flex-direction: column`，寬 38px、按鈕 30×27），
  預覽模式 append 回 `#canvas-area`（右下角、橫排）。
- 按鈕順序：`+` → 比例（點擊＝適應視窗）→ `−` → 分隔線 → 適應（⛶）。
- **移除 `#zoom-actual`（1:1）**：DOM、JS 綁定、快捷鍵面板列與 README 均已移除。


## 22. 移除預覽提示（task-12）

- 刪除 `#preview-hint`（`.pc-hint`）與 `phase.js` 的 `showHint/hideHint/maybeShowHint` 及相關 CSS；
  預覽階段只留 chips、換頁箭頭與縮放控制，不再出現任何浮動提示條。


## 23. 視覺翻新 v2（task-13）

- tokens：`--bg #151517`、`--panel #232324`、`--canvas #101012`、`--rail/--status #1C1C1E`、
  `--surface mix(#FFF 6%, #232324)`、`--text #F9FAFB`、`--text-dim #9CA3AF`、
  `--line rgba(255,255,255,.14)`、`--line-strong mix(#F9FAFB 24%, transparent)`、
  `--brand #F9FAFB`（近白主色）、`--tool #FF5C5C`（僅工具語意）、語意 `#22C55E/#F59E0B/#EF4444`。
- 主按鈕（header 儲存、匯出／套用、預覽「編輯」chip）＝近白底 + `#151517` 字。
- 分層只用表面＋邊框；所有陰影歸零、CSS 漸層清空。
- 動效：階段切換外殼分批進出（topbar 220 / rail 240+30 / inspector 260+40 / statusbar 220+30，畫布 scale .996→1）、
  回預覽 180–220ms、chips 180ms、按鈕 120ms + active scale(.97)、工具指示條 180ms、
  分頁 160ms + 指示條 200ms、樣式條 260ms、影像載入 `#stage.is-loading` 260ms。
- `main.js`：階段切換時用 `animateToFit(280)` 從當前視圖緩動到 fit（rAF + easeOutCubic），
  phase 變更 900ms 內的 ResizeObserver 不搶著 snap；畫面外加 1px `rgba(255,255,255,.10)` 邊框。


## 24. 預覽 dock（task-14）

- 右上四顆 chip（張數／開啟／資訊／編輯）合併為單一 `#preview-chrome .pc-dock`：
  `#preview-counter`（數字，非按鈕）＋分隔線＋`#preview-info`（ⓘ 圖示）＋`#preview-edit`（✎ 近白圓形主按鈕），全部 icon-only、無文字。
- **移除 `#preview-open`**：`phase.js` 的綁定與 Enter/Space 防護清單同步移除；開檔改走 `Ctrl+O`／拖放。
- 樣式：dock `rgba(21,21,23,.86)` + 1px `--line` + pill 38px；`.pc-icon` 30×30、hover 白 8%、`:active` scale(.94)；`.pc-primary` 近白底深字。


## 26. 啟動效能修正（task-16）

- **不要用 `requestAnimationFrame` 當「視窗顯示前」的等待條件**：視窗隱藏時 WebView 會節流 rAF，
  導致 `app_ready` 遲遲不送、卡到 2.5s fallback（使用者回報「啟動特別慢」的根因）。
- 顯示策略：Rust `on_page_load(Finished)` 直接 show（此時視窗底色與 inline critical CSS 已是深色，不會白屏），
  前端 boot 一開始仍無條件 `invoke('app_ready')`（idempotent），最後改用 `setTimeout(16)` 而非 rAF。
- `openPath`：資料夾清單 `refreshFolder()` 不再阻塞第一次繪製（先顯示照片，清單稍後 `emit('files')`）。

## 25. 啟動體驗（task-15）

- `tauri.conf.json`：`windows[0].visible=false`、`backgroundColor="#101012"`；視窗由前端 `app_ready` 指令顯示
  （Rust `on_page_load` 不再自行 show，另有 2.5s fallback 顯示）。
- `src/index.html`：`<head>` 內嵌 critical CSS（`html,body{background:#101012}`）與 `#boot` splash
  （logo＋LUMEN 字樣＋1.15s 光條），`<body class="booting">`；`body.booting #app{visibility:hidden}`。
- `main.js`：boot 一開始等兩個 rAF 後 `invoke('app_ready')`（視窗帶著 splash 出現）；
  有 startup 檔案時 `await openPath()`，之後再等兩個 rAF 才 `booting→ready`（splash 淡出 0.3s），
  確保畫面已經是「預覽階段的 viewer」而不是編輯器外殼。


## 27. 極速啟動（task-17）

- Rust `setup()` 直接 `show()`（視窗底色 `#101012`），實測程序啟動 → 視窗出現 **≈55ms**；
  `on_page_load(Finished)` 顯示仍保留（idempotent），2.5s fallback 不變。
- `main.js` boot：`startup_files` → `loader.loadDocument()` 與 `initUI()` **同時進行**（Promise.all），
  解好的 doc 用 `openPath(path, { preloaded })` 直接使用；`initUI` 的 11 個模組改成 `Promise.all` 並行 import。
- `loader.buildDoc`：`inspect_image`（EXIF）與 `createImageBitmap` **並行**。
- 實測（instrumented，三次）：視窗 54/54/56ms；frontend ready（含解碼與首帧）≈232–238ms。


## 28. 過場動效修正（task-18）

- 資訊浮卡：`#inspector` 在預覽階段一律 `position: fixed`、以 `opacity/transform` 開關（不再用 `width:0` 瞬間收合），
  開 200ms、關 200–220ms；`ui/phase.js` 在 document 上監聽 pointerdown，點 `#inspector`／`#preview-info`／`.pc-dock` 以外就 `peekInfo(false)`。
- 圖片在階段切換時的縮放動畫：
  - `applyPhaseChrome()` 只在「preview ↔ edit 真正互換」時啟動；`phaseTransitioning` 950ms 內 `fitView()` 直接 return、
    ResizeObserver 直接 return，避免任何路徑搶先 snap。
  - 300ms 後執行 `animateToFit(320)`（rAF + easeOutCubic），實測比例 0.813 → 0.618 有 16 個中間值。
  - 修掉兩個 bug：①`boot()` 內重複宣告 `phaseChangedAt / phaseTransitioning / viewTween`（shadow 掉模組層變數，導致守衛永遠 false、RO 提前 snap）；
    ②`#inspector` 的 `width:0` 收合規則讓浮卡關閉時瞬間壓縮。


## 29. 版面動畫與即時適應（task-19）

- `#app` 的 `grid-template-columns` 改用 `var(--inspector-w)`（`ui/inspector.js` 在改寬度時同步
  `documentElement.style.setProperty('--inspector-w', ...)`），欄／列才能是 px 對 px 的可插值動畫。
- 轉場只在 `body.phase-anim`（由 `main.applyPhaseChrome` 在真實 preview↔edit 互換時加、380ms 後移除）
  期間套用 `grid-template-columns/rows 260ms`；平常視窗縮放不加 transition，避免拖視窗時延遲。
- `ResizeObserver` 改成**每一帧即時 fit**（`state.view = fitTarget()`＋同步 `render()`），
  所以拖分隔把手、縮放視窗、階段轉場時圖片都跟著變形；只有 `phaseTransitioning`（950ms）內
  仍封鎖外部 `fitView()`，避免搶先 snap。


## 30. 預覽換圖過渡（task-20）

- `main.openPath()`：若目前是預覽階段且已有文件，先在 `render()` 前用 `snapshotBoard()` 把 board 畫布複製成離屏快照，
  載入新圖並完成第一帧後設定 `switchFade = {canvas, start}`，由 `render()` 在最後以 `globalAlpha = 1 - easeOutQuad(t/220ms)`
  疊畫快照並持續 `requestRender()`，220ms 後清除。
- 此時不加 `.is-loading`（避免淡入淡出之外又閃一下 blur）；載入失敗會清掉 `switchFade`。


## 31. 換圖方向感 + dock 首幀（task-21）

- 預覽換圖：`openPath(path, { direction })`，`nextFile(delta)` 傳入 `direction: delta`；`render()` 在 `switchFade` 期間
  把新圖 `translateX((1-e)*dir*cssW*0.14)` 滑入、舊快照 `translateX(-e*dir*cssW*0.14)` 滑出並淡出，260ms easeOutCubic。
- dock 首幀閃動根因：`#style-dock:not([hidden]) { animation: dock-awake 260ms }` 只要從 hidden 變可見就會
  從 opacity 0 播到 1（看起來「先醒」），動畫結束才套 `.is-idle`（「瞬間閒置」）。
  修正：動畫改成 `:not([hidden]):not(.is-idle)`（只演醒來那一次），keyframes 起點改為 opacity .5 / translateY(30%)；
  首次顯示時以 inline style 直接釘住睡著狀態，下一帧再交還 CSS。


## 32. Dock 進出場（task-22）

- `style-panel.js` 的 `syncTool()` 改用 `showDock()` / `hideDock()`：
  - 進場：`hidden=false` → inline 起始狀態 `opacity:0; translateY(calc(100% + 18px))` → 下一帧過渡到
    `.is-idle` 的 `opacity .42 / translateY(58%)`，240ms cubic-bezier(.2,.8,.25,1)；280ms 後清 inline。
  - 退場：inline 過渡到 `opacity:0; translateY(calc(100% + 18px))`（180ms），190ms 後設 `hidden` 並清 inline。
  - 兩個計時器（`dockAnimTimer`）在快速切換工具時會互相取消，不會卡在中間狀態。
- CSS 的 `dock-awake` 動畫維持只在「醒來」時播放（`:not(.is-idle)`），所以進場不會先亮後暗。


## 33. 直線／虛線／裁切／收合動效（task-23）

- **直線工具**（`L`）：`annotate` 新增 `type:'line'`，與 arrow 共用兩點式資料（`points[2]`、`arrowPoints`／`setArrowPoint`），
  `handlesFor` 回 2 端點、`resizeObject` 單端拖曳、`hitTest` 線段。互動走 `startArrow()` 同一套 click-click / drag 流程。
- **虛線**：`state.style.dashed`；`normStyle` 帶入、`applyDash()` 依 `size` 產生 `[size*2.4, size*1.6]`，
  brush／rect／ellipse／line／arrow（line 本身）都套用，箭頭頭部保持實心。dock 新增 `#style-dashed`。
  樣式輸入元件在 change 後 `blur()`，避免搶走 V/B/L 等快捷鍵。
- **裁切**：`actions.setTool()` 進入 crop 且無裁切框時建立 80% 置中框；離開 crop 且未「套用」時清除；
  `state.cropCommitted` 在套用裁切時設 true（保留供匯出，但 overlay 只在 crop 工具顯示）。
- **動畫**：`body.insp-anim`（收合／拖寬 240ms grid 過渡＋面板 opacity）；`.modal.is-closing` 160ms 退場，
  `ui/shortcuts.js` 延後 `hidden`。
- 畫筆圖示重繪（`#t-brush` 的 SVG path）。


## 35. 雙向 fit 與單一路徑過渡（task-25）

- `phaseTargetView()` 改為以 **`finalStageSize()`**（`app` 尺寸 − rail/inspector/topbar/statusbar 的 CSS 變數）
  先算出動畫結束後的畫布尺寸，兩個方向都回傳 `fitViewFor(w,h)` —— 進 Editor 也重新 fit＋置中。
- `startPhaseViewTween()` 改成「固定起點 → 固定終點、單一 easeOutCubic」的插值（不再每帧追移動中的目標），
  所以 x/y 同時移動，不會出現「先上下再左右」；`ResizeObserver` 在 tween 期間只重繪。
- **回中修正（task-26）**：`fitViewFor(w,h)` 原本用 `clampView()`（以「目前畫布」尺寸夾制），
  但階段切換開始時畫布還是舊尺寸，會把 `tx/ty` 夾成 0（圖片貼左／貼上）。
  現在改為以**目標畫布尺寸**呼叫 `viewport.clampPan(base, doc, w, h)`，因此目標就是真正的置中 fit。
- **路徑彎折修正（task-27）**：`body.phase-anim #app` 的 grid 過渡已移除。原因：grid 動畫期間畫布
  原點與尺寸一直在變，圖片位置 = 畫布原點 + view 位移，兩段不同曲線相加就會「轉彎」。
  現在版面尺寸**瞬間切換**（畫布原點固定），只有 view 以單一 easeOutCubic 走直線；外框仍用各自的
  opacity/transform 過場（220–260ms）。實測移動 ~200px 的最大垂直偏差 1.2px（量測噪聲）。

## 34. 虛線圖示／回中動畫／裁切比例（task-24）

- 虛線改為色票列最後一顆圖示按鈕 `#style-dashed.swatch-toggle`（`aria-pressed`＋`.is-active`），
  切換時 `actions.setStyle({dashed})`；`syncTool()` 在文字工具時 `hidden`。
- 階段切換的回中動畫：新增 `phaseTargetView()`（預覽→永遠 fit；編輯→保持 zoom 但把圖片中心對齊畫布中心）與
  `startPhaseViewTween(340)`（每帧以 k=0.3 指數逼近目標，跟隨 grid 動畫），`ResizeObserver` 在 tween 期間只重繪。
  `zoomTo()` 會取消 tween。`fitViewFor(w,h)` 供指定尺寸計算 fit。
- 裁切比例：`panel-crop.js` 換比例時以當前框中心重算 `w/h`（16px 最小、限制在圖片內），不再只改 `ratio` 欄位。


## 36. 切換瞬間不跳動（task-28）

- 版面在切換該帧就跳到最終尺寸，但 `#stage` 的原點也跟著移動（預覽 vs 編輯差 `--rail-w`／`--topbar-h`）。
  若直接沿用舊 `view.tx/ty`，第一帧圖片就會被畫在位移後的位置 —— 看起來像「瞬移一下」。
- 修正：`applyPhaseChrome()` 在切換前後各量一次 `#stage.getBoundingClientRect()`，把 `dx/dy` 傳給
  `startPhaseViewTween(duration, originDx, originDy)`，補償進起始 view：
  `from.tx += stageLeftBefore - stageLeftAfter`（ty 同理），之後才開始插值到置中 fit 目標。
- 實測（60fps 追蹤字標）：切換前 (873,606) → 第一帧 (861,602)，無跳動；兩個方向皆如此。


## 37. 多語系（task-29）

- 原始文案是繁體中文；`scripts/gen-i18n.py` 讀 `scripts/i18n/keys.json` + `en.json`，
  簡中用 `opencc -c t2s.json` 轉換，輸出 `src/js/core/i18n-data.js`（`TRANSLATIONS['zh-Hans'|'en']`）。
- `src/js/core/i18n.js`：`detectLocale()`（`navigator.languages`；`zh` + `hant/tw/hk/mo` → `zh-Hant`，
  `hans/cn/sg/my` → `zh-Hans`，其餘一律 `en`；可用 `LUMEN_LANG` 覆寫），`tr()`（精確比對 →
  帶 `${…}` 的樣板用 regex 比對並遞迴翻譯捕獲值），`applyDom()`（文字節點＋`title/aria-label/placeholder/data-tip/alt`），
  `MutationObserver`（40ms debounce）讓動態插入的字串也自動翻譯。
- Rust 新增 `ui_locale` 指令（讀 `LUMEN_LANG`），`main.js` 在 `boot()` 一開始 `await` 後呼叫 `initI18n({override})`；
  `body.booting` 仍在，所以翻譯在第一帧之前完成，不會先閃中文。
