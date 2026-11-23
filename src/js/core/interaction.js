// Canvas gestures: pan/zoom, brush painting, shape dragging, text placement,
// object move/resize and the crop frame with its eight handles.
//
// Contract notes:
// - Screen <-> image conversion only via viewport helpers.
// - The board canvas is owned by main.js (it draws background + image +
//   committed annotations). This module only ever paints the overlay
//   (in-progress stroke, in-progress shape, selection frame, crop UI).
// - Committed annotations live in state.objects and are mutated through the
//   state helpers; `commit()` is called once per finished gesture, never
//   during a drag.
// - main.js already owns global keys (Delete, Escape, +/-, tool keys), so we
//   deliberately do not bind those again; we add Space-pan and Ctrl+wheel.
// - `state.phase` splits the app in two modes. In 'preview' the canvas is a
//   viewer: wheel zooms around the viewport center (never below "fit"), the
//   image can be dragged once zoomed in past fit, and the overlay stays empty
//   (editing is entered from the toolbar button). In 'edit' every gesture
//   below behaves as before; with the move tool a drag that hits no annotation
//   pans the canvas from anywhere (not just "empty" space), and middle-drag
//   pans in every tool. Only the preview phase clamps panning/zooming so the
//   image cannot be dragged out of the viewport; in edit mode the image can be
//   dragged (and zoomed) freely past the edges.

import {
  state, commit, addObject, updateObject, removeObject, selectObject, setCrop, setView, subscribe,
} from './state.js';
import {
  screenToImage, imageToScreen, zoomAt, panBy, applyView, clampScale, centeredView,
  minScale, clampPan,
} from './viewport.js';
import {
  createObject, drawObjects, drawBrush, hitTest, objectBounds, handlesFor,
  moveObject, resizeObject, arrowPoints, setArrowPoint,
} from './annotate.js';

const PAN_THRESHOLD = 3; // 螢幕 px：小於此位移視為點擊，避免點一下就被當成拖曳
const ARROW_CLICK_MIN = 3; // 第二次點擊需離起點至少這麼多螢幕 px
const ARROW_DRAG_MIN = 6; // 拖曳建立箭頭：放開時位移需大於此值
const MIN_ARROW_LEN = 2; // 圖原 px：端點之間的最小長度，避免拖到另一端時翻轉抖動
const HANDLE_SIZE = 8;
const HANDLE_HIT = 11;
const DBLCLICK_MS = 380;
const MIN_SHAPE_SCREEN = 3;
const MIN_CROP_SCREEN = 8;
const HIT_TOL_PX = 6;

const HANDLE_CURSOR = {
  n: 'ns-resize',
  s: 'ns-resize',
  e: 'ew-resize',
  w: 'ew-resize',
  ne: 'nesw-resize',
  sw: 'nesw-resize',
  nw: 'nwse-resize',
  se: 'nwse-resize',
  start: 'crosshair',
  end: 'crosshair',
};

const SHAPE_LABEL = { rect: '矩形', ellipse: '橢圓', arrow: '箭頭' };

const CROP_HANDLES = ['nw', 'n', 'ne', 'e', 'se', 's', 'sw', 'w'];

function clamp(v, lo, hi) {
  return Math.min(hi, Math.max(lo, v));
}

function isTypingTarget(target) {
  if (!target || typeof target.closest !== 'function') return false;
  return !!target.closest('input, textarea, select, [contenteditable="true"], [contenteditable=""]');
}

export function attachInteraction({ stage, board, overlay, requestRender, onCursor, showTextEditor } = {}) {
  if (!stage || !overlay) {
    // Nothing to attach to: keep main.js alive with a no-op overlay painter.
    return { paintOverlay() {}, detach() {} };
  }

  let drag = null;
  let brushDraft = null;
  let shapeDraft = null;
  let arrowPending = null; // 兩點式箭頭：第一次點擊後的未完成狀態 {obj,startScreen,startImage}
  let hoverScreen = null; // 最近一次游標位置（端點把手 hover 放大用）
  let spaceHeld = false;
  let lastCursor = '';
  let accentCache = '';

  stage.style.touchAction = 'none';
  overlay.style.touchAction = 'none';
  stage.style.userSelect = 'none';
  stage.style.webkitUserSelect = 'none';

  // -------------------------------------------------------------------------
  // Helpers
  // -------------------------------------------------------------------------

  function accent() {
    if (accentCache) return accentCache;
    try {
      const cs = getComputedStyle(document.documentElement);
      const v = cs.getPropertyValue('--accent').trim();
      accentCache = v || '#5b9dff';
    } catch (err) {
      accentCache = '#5b9dff';
    }
    return accentCache;
  }

  function setCursor(cursor) {
    if (!cursor || cursor === lastCursor) return;
    lastCursor = cursor;
    if (typeof onCursor === 'function') onCursor(cursor);
    else stage.style.cursor = cursor;
  }

  function isPreview() {
    return state.phase === 'preview';
  }

  function stageCssSize() {
    const r = stage.getBoundingClientRect();
    return { w: Math.max(1, Math.round(r.width)), h: Math.max(1, Math.round(r.height)) };
  }

  /** 縮放下限＝「適應視窗」，與 main 的 fitView 同一個算式。 */
  function minZoom() {
    if (!state.doc) return clampScale(0);
    const { w, h } = stageCssSize();
    return minScale(state.doc, w, h);
  }

  /**
   * 預覽階段：平移/縮放後夾在可視範圍內，圖片不會被拖出畫面。
   * 編輯階段：不加限制，圖片可以拖到畫布外（使用者要求「到處都能拖」）。
   */
  function clampToViewport(view) {
    if (!state.doc || !isPreview()) return view;
    const { w, h } = stageCssSize();
    return clampPan(view, state.doc, w, h);
  }

  /** 預覽階段：只有放大超過適應大小時才能拖曳。 */
  function canPanPreview() {
    if (!state.doc) return false;
    return (state.view.scale || 1) > minZoom() + 0.001;
  }

  /** 套用一個 view；數字完全沒變就不 emit（fit 時被 clamp 成無位移不會抖動或改 fitMode）。 */
  /** Push a computed view into state, but only when it actually changed.
   *  (Named distinctly from viewport.applyView(ctx, view), which composes the
   *  transform onto a canvas context — the two used to collide, which wiped
   *  state.view every time a brush stroke repainted.) */
  function pushView(v) {
    const cur = state.view;
    if (cur.scale === v.scale && cur.tx === v.tx && cur.ty === v.ty) return;
    setView({ scale: v.scale, tx: v.tx, ty: v.ty, fitMode: v.fitMode || 'manual' });
  }

  /** 縮放（任何 zoomAt 路徑）：下限夾在 fit、上限沿用 clampScale，之後再夾平移。 */
  function applyZoom(view, sx, sy, factor) {
    const base = view.scale > 0 ? view.scale : 1;
    const target = Math.max(minZoom(), clampScale(base * factor));
    const zoomed = zoomAt(view, sx, sy, target / base);
    return clampToViewport(zoomed);
  }

  function defaultCursor() {
    if (isPreview()) return canPanPreview() ? 'grab' : 'default';
    if (spaceHeld) return 'grab';
    switch (state.tool) {
      case 'move':
        return 'default';
      case 'text':
        return 'text';
      default:
        return 'crosshair';
    }
  }

  function clientToStage(e) {
    const r = stage.getBoundingClientRect();
    return { x: e.clientX - r.left, y: e.clientY - r.top };
  }

  function toImage(screenPt) {
    return screenToImage(state.view, screenPt.x, screenPt.y);
  }

  function findSelected() {
    if (!state.selectedId) return null;
    return state.objects.find((o) => o.id === state.selectedId) || null;
  }

  /** 階段切換時把手勢、草稿與選取框一次收乾淨，不留半截筆畫。 */
  function cancelActiveGesture() {
    const d = drag;
    drag = null;
    brushDraft = null;
    shapeDraft = null;
    arrowPending = null;
    if (!d) return;
    try {
      if (stage.hasPointerCapture?.(d.pointerId)) stage.releasePointerCapture(d.pointerId);
    } catch (err) {
      /* 已釋放 */
    }
    if (d.mode === 'moveObj' || d.mode === 'resize') {
      if (d.moved) commit(d.mode === 'moveObj' ? '移動物件' : '調整大小');
    } else if (d.mode === 'crop-new' || d.mode === 'crop-move' || d.mode === 'crop-resize') {
      // 拖到一半切階段：回到拖曳前的裁切框，不留下未完成的框。
      setCrop(d.prevCrop);
    }
    // brush / shape 的草稿直接丟棄，pointerup 不會再被呼叫。
  }

  /** 丟掉未完成的箭頭（不 commit、不進 undo）。 */
  function cancelArrowPending() {
    if (!arrowPending) return;
    if (drag && drag.mode === 'arrow-drag') {
      try {
        if (stage.hasPointerCapture?.(drag.pointerId)) stage.releasePointerCapture(drag.pointerId);
      } catch (err) {
        /* 已釋放 */
      }
      drag = null;
    }
    arrowPending = null;
    setCursor(defaultCursor());
    paintOverlay();
  }

  /** 箭頭端點拖曳：離另一端太近時夾在 MIN_ARROW_LEN，避免翻轉抖動。 */
  function clampArrowEndpoint(obj, handle, imgPt) {
    const pts = arrowPoints(obj);
    const fixed = handle === 'start' ? pts[1] : pts[0];
    const dx = imgPt.x - fixed.x;
    const dy = imgPt.y - fixed.y;
    const len = Math.hypot(dx, dy);
    if (len >= MIN_ARROW_LEN) return imgPt;
    if (len < 1e-6) return { x: fixed.x + MIN_ARROW_LEN, y: fixed.y };
    return { x: fixed.x + (dx / len) * MIN_ARROW_LEN, y: fixed.y + (dy / len) * MIN_ARROW_LEN };
  }

  function overlayCtx() {
    const ctx = overlay.getContext('2d');
    if (!ctx) return null;
    const cssW = Math.max(1, Math.round(stage.getBoundingClientRect().width));
    const cssH = Math.max(1, Math.round(stage.getBoundingClientRect().height));
    const dpr = overlay.width > 0 ? Math.max(1, overlay.width / cssW) : 1;
    return { ctx, cssW, cssH, dpr };
  }

  function capture(e) {
    try {
      stage.setPointerCapture(e.pointerId);
    } catch (err) {
      /* 取消失敗（例如指標已離開）時仍可繼續，事件只是不再被捕捉 */
    }
  }

  function releaseCapture(e) {
    try {
      if (stage.hasPointerCapture && stage.hasPointerCapture(e.pointerId)) {
        stage.releasePointerCapture(e.pointerId);
      }
    } catch (err) {
      /* 已釋放 */
    }
  }

  function screenHandlesFor(rect) {
    const a = imageToScreen(state.view, rect.x, rect.y);
    const b = imageToScreen(state.view, rect.x + rect.w, rect.y + rect.h);
    const x = Math.min(a.x, b.x);
    const y = Math.min(a.y, b.y);
    const w = Math.abs(b.x - a.x);
    const h = Math.abs(b.y - a.y);
    const cx = x + w / 2;
    const cy = y + h / 2;
    const pos = {
      nw: [x, y], n: [cx, y], ne: [x + w, y], e: [x + w, cy],
      se: [x + w, y + h], s: [cx, y + h], sw: [x, y + h], w: [x, cy],
    };
    return CROP_HANDLES.map((name) => ({ name, x: pos[name][0], y: pos[name][1] }));
  }

  function objectHandleAt(screenPt, obj) {
    // 取最近的一個把手：箭頭兩端重疊時（被夾到最短長度）才不會永遠選到起點。
    let best = null;
    let bestDist = Infinity;
    for (const h of handlesFor(obj)) {
      const p = imageToScreen(state.view, h.x, h.y);
      const dx = Math.abs(screenPt.x - p.x);
      const dy = Math.abs(screenPt.y - p.y);
      if (dx <= HANDLE_HIT && dy <= HANDLE_HIT) {
        const dist = Math.hypot(dx, dy);
        if (dist < bestDist) {
          bestDist = dist;
          best = h.name;
        }
      }
    }
    return best;
  }

  function cropHandleAt(screenPt) {
    const c = state.crop;
    if (!c || c.w < 1 || c.h < 1) return null;
    for (const h of screenHandlesFor(c)) {
      if (Math.abs(screenPt.x - h.x) <= HANDLE_HIT + 2 && Math.abs(screenPt.y - h.y) <= HANDLE_HIT + 2) {
        return h.name;
      }
    }
    return null;
  }

  // -------------------------------------------------------------------------
  // Overlay painting (main.js calls this after every board render)
  // -------------------------------------------------------------------------

  function drawHandle(ctx, x, y, size) {
    const half = size / 2;
    ctx.fillStyle = '#ffffff';
    ctx.strokeStyle = accent();
    ctx.lineWidth = 1;
    ctx.beginPath();
    ctx.rect(Math.round(x - half) + 0.5, Math.round(y - half) + 0.5, size, size);
    ctx.fill();
    ctx.stroke();
  }

  function drawArrowHandle(ctx, x, y, radius) {
    ctx.beginPath();
    ctx.arc(x, y, radius, 0, Math.PI * 2);
    ctx.fillStyle = accent();
    ctx.fill();
    ctx.lineWidth = 1;
    ctx.strokeStyle = 'rgba(255, 255, 255, 0.9)';
    ctx.stroke();
  }

  /** 箭頭選取：只有兩個圓形端點把手，沒有外框。 */
  function paintArrowSelection(ctx, obj) {
    ctx.save();
    for (const hp of handlesFor(obj)) {
      const sp = imageToScreen(state.view, hp.x, hp.y);
      const hovered = !!hoverScreen && Math.hypot(hoverScreen.x - sp.x, hoverScreen.y - sp.y) <= 9;
      drawArrowHandle(ctx, sp.x, sp.y, hovered ? 6 : 4.5);
    }
    ctx.restore();
  }

  function paintSelection(ctx) {
    const obj = findSelected();
    if (!obj) return;
    if (obj.type === 'arrow' || obj.type === 'line') {
      paintArrowSelection(ctx, obj);
      return;
    }
    const b = objectBounds(obj);
    const p0 = imageToScreen(state.view, b.x, b.y);
    const p1 = imageToScreen(state.view, b.x + b.w, b.y + b.h);
    const x = Math.min(p0.x, p1.x);
    const y = Math.min(p0.y, p1.y);
    const w = Math.abs(p1.x - p0.x);
    const h = Math.abs(p1.y - p0.y);
    ctx.save();
    ctx.strokeStyle = accent();
    ctx.lineWidth = 1;
    ctx.setLineDash([]);
    ctx.strokeRect(Math.round(x) + 0.5, Math.round(y) + 0.5, Math.max(1, Math.round(w)), Math.max(1, Math.round(h)));
    for (const hp of handlesFor(obj)) {
      const sp = imageToScreen(state.view, hp.x, hp.y);
      drawHandle(ctx, sp.x, sp.y, HANDLE_SIZE);
    }
    ctx.restore();
  }

  function paintCrop(ctx, cssW, cssH) {
    let rect = state.crop;
    if (drag && drag.rect && drag.mode.startsWith('crop')) rect = drag.rect;
    if (!rect || rect.w < 1 || rect.h < 1) return;
    const a = imageToScreen(state.view, rect.x, rect.y);
    const b = imageToScreen(state.view, rect.x + rect.w, rect.y + rect.h);
    const x = Math.min(a.x, b.x);
    const y = Math.min(a.y, b.y);
    const w = Math.abs(b.x - a.x);
    const h = Math.abs(b.y - a.y);

    ctx.save();
    // 框外壓暗
    ctx.fillStyle = 'rgba(0, 0, 0, 0.55)';
    ctx.fillRect(0, 0, cssW, Math.max(0, y));
    ctx.fillRect(0, y + h, cssW, Math.max(0, cssH - (y + h)));
    ctx.fillRect(0, y, Math.max(0, x), Math.max(0, h));
    ctx.fillRect(x + w, y, Math.max(0, cssW - (x + w)), Math.max(0, h));

    // 三分線
    ctx.strokeStyle = 'rgba(255, 255, 255, 0.34)';
    ctx.lineWidth = 1;
    ctx.beginPath();
    for (const t of [1 / 3, 2 / 3]) {
      ctx.moveTo(Math.round(x + w * t) + 0.5, y);
      ctx.lineTo(Math.round(x + w * t) + 0.5, y + h);
      ctx.moveTo(x, Math.round(y + h * t) + 0.5);
      ctx.lineTo(x + w, Math.round(y + h * t) + 0.5);
    }
    ctx.stroke();

    // 邊框 + 把手
    ctx.strokeStyle = 'rgba(255, 255, 255, 0.92)';
    ctx.strokeRect(Math.round(x) + 0.5, Math.round(y) + 0.5, Math.max(1, Math.round(w)), Math.max(1, Math.round(h)));
    for (const hp of screenHandlesFor(rect)) drawHandle(ctx, hp.x, hp.y, HANDLE_SIZE + 2);
    ctx.restore();
  }

  function paintOverlay() {
    const geom = overlayCtx();
    if (!geom) return;
    const { ctx, cssW, cssH, dpr } = geom;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, cssW, cssH);
    if (isPreview()) {
      // 預覽階段：已提交的註譯只由 board 顯示，overlay 保持空白。
      brushDraft = null;
      shapeDraft = null;
      arrowPending = null;
      return;
    }
    if (!state.doc) return;

    if (brushDraft) {
      ctx.save();
      applyView(ctx, state.view);
      ctx.globalAlpha = brushDraft.style?.opacity ?? 1;
      drawBrush(ctx, brushDraft, 1);
      ctx.restore();
    }

    if (arrowPending) {
      // 起點小圓點 + 跟隨游標的預覽線（第一次點擊後、第二次點擊前）。
      drawObjects(ctx, [], state.view, 1, { preview: arrowPending.obj });
      const anchor = imageToScreen(state.view, arrowPending.startImage.x, arrowPending.startImage.y);
      ctx.save();
      ctx.beginPath();
      ctx.arc(anchor.x, anchor.y, 4, 0, Math.PI * 2);
      ctx.fillStyle = accent();
      ctx.fill();
      ctx.strokeStyle = 'rgba(255, 255, 255, 0.85)';
      ctx.lineWidth = 1;
      ctx.stroke();
      ctx.restore();
    } else if (shapeDraft) {
      drawObjects(ctx, [], state.view, 1, { preview: shapeDraft });
    }

    if (state.tool === 'crop') {
      paintCrop(ctx, cssW, cssH);
    } else if (state.tool === 'move' && state.selectedId) {
      paintSelection(ctx);
    }
  }

  // -------------------------------------------------------------------------
  // Brush
  // -------------------------------------------------------------------------

  function brushPaintFresh() {
    const geom = overlayCtx();
    if (!geom) return;
    const { ctx, cssW, cssH, dpr } = geom;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, cssW, cssH);
    if (!brushDraft) return;
    ctx.save();
    applyView(ctx, state.view);
    ctx.globalAlpha = brushDraft.style?.opacity ?? 1;
    drawBrush(ctx, brushDraft, 1);
    ctx.restore();
  }

  /** Append only the newly added curve segment to the overlay. */
  function brushPaintIncremental(from) {
    if (!brushDraft) return;
    const geom = overlayCtx();
    if (!geom) return;
    const { ctx, dpr } = geom;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.save();
    applyView(ctx, state.view);
    ctx.globalAlpha = brushDraft.style?.opacity ?? 1;
    drawBrush(ctx, brushDraft, from);
    ctx.restore();
  }

  function startBrush(e, screenPt, imgPt) {
    const obj = createObject('brush', imgPt, { ...state.style });
    brushDraft = obj;
    drag = {
      mode: 'brush',
      pointerId: e.pointerId,
      lastScreen: screenPt,
      lastTime: e.timeStamp || performance.now(),
      lastDrawn: 0,
    };
    capture(e);
    brushPaintFresh();
    setCursor('crosshair');
    e.preventDefault();
  }

  function brushAdd(screenPt, time) {
    if (!brushDraft || !drag) return;
    const pts = brushDraft.points;
    const last = pts[pts.length - 1];
    const dist = Math.hypot(screenPt.x - drag.lastScreen.x, screenPt.y - drag.lastScreen.y);
    if (dist < 0.7) return;
    const dt = Math.max(4, time - drag.lastTime);
    const velocity = dist / dt;
    const size = brushDraft.style?.size || 8;
    // 速度 → 線寬：快則細、慢則粗，再平滑過渡到上一個點。
    const target = size * (1 - Math.min(1, velocity / 1.5) * 0.82);
    const prevW = last.w;
    const w = Math.max((0.65 * size) / 8, prevW + (target - prevW) * 0.38);
    const imgPt = toImage(screenPt);
    pts.push({ x: imgPt.x, y: imgPt.y, w });
    drag.lastScreen = screenPt;
    drag.lastTime = time;
    brushPaintIncremental(drag.lastDrawn + 1);
    drag.lastDrawn = pts.length - 1;
  }

  function finishBrush() {
    const obj = brushDraft;
    brushDraft = null;
    if (!obj || !obj.points?.length) return;
    addObject(obj);
    commit('筆刷');
    requestRender?.();
  }

  // -------------------------------------------------------------------------
  // Arrow (two-point creation: click start, click end — or drag once)
  // -------------------------------------------------------------------------

  function finalizeArrow() {
    if (!arrowPending) return;
    const obj = arrowPending.obj;
    arrowPending = null;
    addObject(obj);
    commit(obj.type === 'line' ? '直線' : '箭頭');
    requestRender?.();
  }

  function startArrow(e, screenPt, imgPt) {
    const linearType = state.tool === 'line' ? 'line' : 'arrow';
    if (arrowPending) {
      // 第二次點擊：定終點（同一點附近忽略，保留未完成狀態）。
      const dist = Math.hypot(screenPt.x - arrowPending.startScreen.x, screenPt.y - arrowPending.startScreen.y);
      if (dist >= ARROW_CLICK_MIN) {
        setArrowPoint(arrowPending.obj, 'end', imgPt);
        finalizeArrow();
      }
      e.preventDefault();
      return;
    }
    // 第一次點擊：只記起點與預覽，不建立物件、不進 undo。
    const obj = createObject(linearType, { points: [{ ...imgPt }, { ...imgPt }] }, {
      ...state.style,
    });
    arrowPending = { obj, startScreen: screenPt, startImage: { ...imgPt } };
    drag = {
      mode: 'arrow-drag',
      pointerId: e.pointerId,
      startScreen: screenPt,
      startImage: { ...imgPt },
      moved: false,
    };
    capture(e);
    paintOverlay();
    setCursor('crosshair');
    e.preventDefault();
  }

  function finishArrowDrag(d, screenPt) {
    if (!arrowPending) return;
    const dist = Math.hypot(screenPt.x - d.startScreen.x, screenPt.y - d.startScreen.y);
    if (dist < ARROW_DRAG_MIN) return; // 拖太短：當成第一次點擊，等第二次點擊完成
    setArrowPoint(arrowPending.obj, 'end', toImage(screenPt));
    finalizeArrow();
  }

  // -------------------------------------------------------------------------
  // Shapes (rect / ellipse)
  // -------------------------------------------------------------------------

  function startShape(e, screenPt, imgPt) {
    const type = state.tool;
    const obj = createObject(type, imgPt, { ...state.style });
    shapeDraft = obj;
    drag = {
      mode: 'shape',
      type,
      pointerId: e.pointerId,
      start: imgPt,
      startScreen: screenPt,
      obj,
    };
    capture(e);
    paintOverlay();
    setCursor('crosshair');
    e.preventDefault();
    void screenPt;
  }

  function updateShape(e, screenPt) {
    if (!drag || !drag.obj) return;
    const imgPt = toImage(screenPt);
    let dx = imgPt.x - drag.start.x;
    let dy = imgPt.y - drag.start.y;
    if (e.shiftKey) {
      if (drag.type === 'arrow') {
        const len = Math.hypot(dx, dy);
        const step = Math.PI / 4;
        const angle = Math.round(Math.atan2(dy, dx) / step) * step;
        dx = Math.cos(angle) * len;
        dy = Math.sin(angle) * len;
      } else {
        const m = Math.max(Math.abs(dx), Math.abs(dy));
        dx = (dx < 0 ? -1 : 1) * m;
        dy = (dy < 0 ? -1 : 1) * m;
      }
    }
    const obj = drag.obj;
    if (drag.type === 'arrow') {
      obj.x = drag.start.x;
      obj.y = drag.start.y;
      obj.w = dx;
      obj.h = dy;
    } else {
      obj.x = dx < 0 ? drag.start.x + dx : drag.start.x;
      obj.y = dy < 0 ? drag.start.y + dy : drag.start.y;
      obj.w = Math.abs(dx);
      obj.h = Math.abs(dy);
    }
    // 只有 overlay 內容變了，底圖不用重畫。
    paintOverlay();
  }

  function finishShape(d, screenPt) {
    const obj = d.obj;
    shapeDraft = null;
    const scale = state.view.scale || 1;
    const size = Math.hypot(obj.w, obj.h) * scale;
    const moved = Math.hypot(screenPt.x - d.startScreen.x, screenPt.y - d.startScreen.y);
    if (size < MIN_SHAPE_SCREEN || moved < MIN_SHAPE_SCREEN) {
      requestRender?.();
      return;
    }
    addObject(obj);
    commit(SHAPE_LABEL[d.type] || '標註');
    requestRender?.();
  }

  // -------------------------------------------------------------------------
  // Text
  // -------------------------------------------------------------------------

  function openTextEditor(obj, clientPt, isNew) {
    const finish = (text) => {
      const value = typeof text === 'string' ? text.replace(/\r/g, '').trim() : '';
      const live = state.objects.find((o) => o.id === obj.id);
      if (!live) return;
      if (!value) {
        if (isNew) {
          removeObject(obj.id);
          requestRender?.();
        }
        return;
      }
      updateObject({ ...live, text: value });
      commit(isNew ? '新增文字' : '編輯文字');
      selectObject(obj.id);
      requestRender?.();
    };

    const cancel = () => {
      if (!isNew) return;
      if (state.objects.some((o) => o.id === obj.id)) removeObject(obj.id);
      requestRender?.();
    };

    const fontSize = Math.max(8, (obj.style?.fontSize || 36) * (state.view.scale || 1));
    const mod = showTextEditor;
    if (mod && typeof mod.show === 'function') {
      mod.show({
        screenX: clientPt.clientX,
        screenY: clientPt.clientY,
        fontSize,
        color: obj.style?.color,
        text: obj.text || '',
        onCommit: finish,
        onCancel: cancel,
      });
      return;
    }

    // UI 模組尚未載入時的降級：先放一個可雙擊再編輯的預設文字物件。
    if (isNew) {
      updateObject({ ...obj, text: '文字' });
      commit('新增文字');
      requestRender?.();
    }
  }

  function startText(e, screenPt, imgPt) {
    const obj = createObject('text', imgPt, { ...state.style });
    obj.text = '';
    addObject(obj);
    selectObject(obj.id);
    openTextEditor(obj, { clientX: e.clientX, clientY: e.clientY }, true);
    e.preventDefault();
    void screenPt;
  }

  function editExistingText(obj, clientPt) {
    openTextEditor(obj, clientPt, false);
  }

  let lastClick = { time: 0, x: 0, y: 0 };
  function isDoubleClick(e, screenPt) {
    const now = e.timeStamp || performance.now();
    const near = Math.hypot(screenPt.x - lastClick.x, screenPt.y - lastClick.y) < 8;
    const quick = now - lastClick.time < DBLCLICK_MS;
    lastClick = { time: now, x: screenPt.x, y: screenPt.y };
    return near && quick;
  }

  // -------------------------------------------------------------------------
  // Move tool: select, drag, resize, pan
  // -------------------------------------------------------------------------

  function startMove(e, screenPt, imgPt) {
    const sel = findSelected();
    if (sel) {
      const handle = objectHandleAt(screenPt, sel);
      if (handle) {
        const snapshot = structuredClone(sel);
        drag = { mode: 'resize', pointerId: e.pointerId, handle, obj: snapshot, moved: false };
        capture(e);
        setCursor(HANDLE_CURSOR[handle] || 'default');
        e.preventDefault();
        return;
      }
    }

    // 命中優先序：端點把手（上面已處理）> 箭頭線身 > 其他物件。
    const arrows = state.objects.filter((o) => o.type === 'arrow' || o.type === 'line');
    const hit = (arrows.length ? hitTest(arrows, imgPt, HIT_TOL_PX, state.view) : null) ||
      hitTest(state.objects, imgPt, HIT_TOL_PX, state.view);
    if (hit) {
      const dbl = isDoubleClick(e, screenPt);
      if (state.selectedId !== hit.id) selectObject(hit.id);
      if (dbl && hit.type === 'text') {
        const b = objectBounds(hit);
        const p = imageToScreen(state.view, b.x, b.y);
        const r = stage.getBoundingClientRect();
        editExistingText(hit, { clientX: r.left + p.x, clientY: r.top + p.y });
        e.preventDefault();
        return;
      }
      drag = {
        mode: 'moveObj',
        pointerId: e.pointerId,
        obj: structuredClone(hit),
        origin: imgPt,
        moved: false,
      };
      capture(e);
      setCursor('move');
      e.preventDefault();
      return;
    }

    // 沒有命中任何物件：在畫布任何位置（含圖片上）拖曳都直接平移畫面。
    // 選取只在「真的只是點一下」時清掉，拖曳平移不影響選取。
    startPan(e, screenPt, { deselectOnClick: true });
  }

  function updateMove(screenPt) {
    if (!drag) return;
    const imgPt = toImage(screenPt);
    const dx = imgPt.x - drag.origin.x;
    const dy = imgPt.y - drag.origin.y;
    const screenDist = Math.hypot(dx, dy) * (state.view.scale || 1);
    if (!drag.moved && screenDist < 2) return;
    drag.moved = true;
    const next = structuredClone(drag.obj);
    moveObject(next, dx, dy);
    updateObject(next);
  }

  function updateResize(e, screenPt) {
    if (!drag) return;
    const imgPt = toImage(screenPt);
    const next = structuredClone(drag.obj);
    if ((next.type === 'arrow' || next.type === 'line') && (drag.handle === 'start' || drag.handle === 'end')) {
      resizeObject(next, drag.handle, clampArrowEndpoint(next, drag.handle, imgPt), false);
    } else {
      resizeObject(next, drag.handle, imgPt, e.shiftKey);
    }
    drag.moved = true;
    updateObject(next);
  }

  function startPan(e, screenPt, opts = {}) {
    const { w, h } = stageCssSize();
    drag = {
      mode: 'pan',
      pointerId: e.pointerId,
      start: screenPt,
      view0: { ...state.view },
      bounds: { w, h },
      moved: false,
      deselectOnClick: !!opts.deselectOnClick,
    };
    capture(e);
    setCursor('grabbing');
    e.preventDefault();
  }

  // -------------------------------------------------------------------------
  // Crop
  // -------------------------------------------------------------------------

  function startCrop(e, screenPt, imgPt) {
    const prevCrop = state.crop ? { ...state.crop } : null;
    const ratio = Number.isFinite(state.crop?.ratio) && state.crop.ratio > 0 ? state.crop.ratio : null;

    if (prevCrop && prevCrop.w >= 1 && prevCrop.h >= 1) {
      const handle = cropHandleAt(screenPt);
      if (handle) {
        drag = {
          mode: 'crop-resize',
          pointerId: e.pointerId,
          handle,
          rect0: { ...prevCrop },
          prevCrop,
          ratio,
          img0: imgPt,
          moved: false,
          rect: { ...prevCrop },
        };
        capture(e);
        setCursor(HANDLE_CURSOR[handle] || 'crosshair');
        e.preventDefault();
        return;
      }
      if (imgPt.x >= prevCrop.x && imgPt.x <= prevCrop.x + prevCrop.w &&
          imgPt.y >= prevCrop.y && imgPt.y <= prevCrop.y + prevCrop.h) {
        drag = {
          mode: 'crop-move',
          pointerId: e.pointerId,
          rect0: { ...prevCrop },
          prevCrop,
          ratio,
          img0: imgPt,
          moved: false,
          rect: { ...prevCrop },
        };
        capture(e);
        setCursor('move');
        e.preventDefault();
        return;
      }
    }

    drag = {
      mode: 'crop-new',
      pointerId: e.pointerId,
      img0: { x: clamp(imgPt.x, 0, state.doc?.width || 0), y: clamp(imgPt.y, 0, state.doc?.height || 0) },
      rect0: null,
      prevCrop,
      ratio,
      moved: false,
      rect: { x: imgPt.x, y: imgPt.y, w: 0, h: 0 },
    };
    capture(e);
    setCursor('crosshair');
    e.preventDefault();
  }

  function updateCrop(e, screenPt) {
    if (!drag || !state.doc) return;
    const imgPt = toImage(screenPt);
    const W = state.doc.width;
    const H = state.doc.height;
    const ratio = drag.ratio ?? (e.shiftKey ? 1 : null);
    let rect = drag.rect || { x: 0, y: 0, w: 0, h: 0 };

    if (drag.mode === 'crop-new') {
      const x0 = drag.img0.x;
      const y0 = drag.img0.y;
      let x1 = clamp(imgPt.x, 0, W);
      let y1 = clamp(imgPt.y, 0, H);
      if (ratio) {
        const dx = x1 - x0;
        const dy = y1 - y0;
        if (Math.abs(dx) >= Math.abs(dy) * ratio) {
          y1 = clamp(y0 + (dy < 0 ? -1 : 1) * (Math.abs(dx) / ratio), 0, H);
        } else {
          x1 = clamp(x0 + (dx < 0 ? -1 : 1) * (Math.abs(dy) * ratio), 0, W);
        }
      }
      rect = {
        x: Math.min(x0, x1),
        y: Math.min(y0, y1),
        w: Math.abs(x1 - x0),
        h: Math.abs(y1 - y0),
      };
    } else if (drag.mode === 'crop-move') {
      const dx = imgPt.x - drag.img0.x;
      const dy = imgPt.y - drag.img0.y;
      rect = { ...drag.rect0 };
      rect.x = clamp(rect.x + dx, 0, Math.max(0, W - rect.w));
      rect.y = clamp(rect.y + dy, 0, Math.max(0, H - rect.h));
    } else {
      rect = resizeCropRect(drag.rect0, drag.handle, imgPt, ratio, W, H);
    }

    rect.w = Math.max(1, Math.min(rect.w, W));
    rect.h = Math.max(1, Math.min(rect.h, H));
    rect.x = clamp(rect.x, 0, Math.max(0, W - rect.w));
    rect.y = clamp(rect.y, 0, Math.max(0, H - rect.h));
    rect.ratio = drag.ratio;
    drag.rect = rect;
    drag.moved = true;
    // 拖動中只更新狀態（面板會跟著更新），放開滑鼠才 commit。
    setCrop(rect);
  }

  function resizeCropRect(base, handle, imgPt, ratio, W, H) {
    const dw = handle.includes('w');
    const de = handle.includes('e');
    const dn = handle.includes('n');
    const ds = handle.includes('s');
    let left = base.x;
    let top = base.y;
    let width = base.w;
    let height = base.h;

    if (dw) {
      const right = base.x + base.w;
      left = clamp(imgPt.x, 0, right - 1);
      width = right - left;
    } else if (de) {
      width = Math.max(1, clamp(imgPt.x, 0, W) - base.x);
    }
    if (dn) {
      const bottom = base.y + base.h;
      top = clamp(imgPt.y, 0, bottom - 1);
      height = bottom - top;
    } else if (ds) {
      height = Math.max(1, clamp(imgPt.y, 0, H) - base.y);
    }

    if (ratio) {
      if (dw || de) {
        height = width / ratio;
        if (dn) top = base.y + base.h - height;
        else if (!ds) top = base.y;
      } else if (dn || ds) {
        width = height * ratio;
        if (dw) left = base.x + base.w - width;
        else if (!de) left = base.x;
      }
    }

    width = Math.max(1, Math.min(width, W));
    height = Math.max(1, Math.min(height, H));
    left = clamp(left, 0, Math.max(0, W - width));
    top = clamp(top, 0, Math.max(0, H - height));
    return { x: left, y: top, w: width, h: height };
  }

  function finishCrop(d, screenPt) {
    const mode = d.mode;
    const rect = d.rect;
    const prevCrop = d.prevCrop;
    const ratio = d.ratio;
    const scale = state.view.scale || 1;

    if (mode === 'crop-new') {
      const tooSmall = !rect || rect.w * scale < MIN_CROP_SCREEN || rect.h * scale < MIN_CROP_SCREEN;
      if (tooSmall) {
        setCrop(prevCrop);
        requestRender?.();
        return;
      }
      setCrop({ ...rect, ratio });
      commit('裁切框');
      requestRender?.();
      return;
    }

    if (!d.moved || !rect) {
      setCrop(prevCrop);
      requestRender?.();
      return;
    }
    setCrop({ ...rect, ratio });
    commit(mode === 'crop-move' ? '移動裁切框' : '調整裁切框');
    requestRender?.();
    void screenPt;
  }

  // -------------------------------------------------------------------------
  // Pointer handling
  // -------------------------------------------------------------------------

  function onPointerDown(e) {
    if (!stage) return;
    if (e.pointerType === 'mouse' && e.button === 2) return;
    if (e.button !== 0 && e.button !== 1) return;
    if (state.busy) return;
    const screenPt = clientToStage(e);

    if (isPreview()) {
      // 預覽階段：放大超過「適應」才可拖曳平移；fit 時左/中鍵完全沒反應。
      if (canPanPreview()) startPan(e, screenPt);
      return;
    }

    if (e.button === 1 || spaceHeld) {
      startPan(e, screenPt);
      return;
    }
    if (!state.doc) return;

    switch (state.tool) {
      case 'crop':
        startCrop(e, screenPt, toImage(screenPt));
        return;
      case 'move':
        startMove(e, screenPt, toImage(screenPt));
        return;
      case 'text':
        startText(e, screenPt, toImage(screenPt));
        return;
      case 'brush':
        startBrush(e, screenPt, toImage(screenPt));
        return;
      case 'arrow':
      case 'line':
        startArrow(e, screenPt, toImage(screenPt));
        return;
      default:
        startShape(e, screenPt, toImage(screenPt));
    }
  }

  function onPointerMove(e) {
    if (!stage) return;
    const screenPt = clientToStage(e);
    if (!drag) {
      hoverScreen = screenPt;
      updateHoverCursor(screenPt);
      if (arrowPending) {
        // 未完成的箭頭跟著游標拉預覽線。
        setArrowPoint(arrowPending.obj, 'end', toImage(screenPt));
        paintOverlay();
      }
      return;
    }
    if (e.pointerId !== drag.pointerId) return;
    e.preventDefault();

    if (drag.mode === 'brush') {
      const events = typeof e.getCoalescedEvents === 'function' ? e.getCoalescedEvents() : [e];
      for (const ce of events) {
        brushAdd(clientToStage(ce), ce.timeStamp || performance.now());
      }
      return;
    }

    if (drag.mode === 'arrow-drag') {
      if (arrowPending) {
        setArrowPoint(arrowPending.obj, 'end', toImage(screenPt));
        drag.moved = true;
        paintOverlay();
      }
      return;
    }

    switch (drag.mode) {
      case 'pan': {
        const dx = screenPt.x - drag.start.x;
        const dy = screenPt.y - drag.start.y;
        if (!drag.moved) {
          // 3px 門檻：單純點擊不要被當成拖曳（也不改 fitMode / 不清選取）。
          if (Math.hypot(dx, dy) < PAN_THRESHOLD) return;
          drag.moved = true;
        }
        const raw = {
          ...drag.view0,
          tx: drag.view0.tx + dx,
          ty: drag.view0.ty + dy,
          fitMode: 'manual',
        };
        // 只有預覽階段才把畫面夾在邊界內；編輯階段可自由拖出畫布。
        const v = isPreview() && state.doc && drag.bounds
          ? clampPan(raw, state.doc, drag.bounds.w, drag.bounds.h)
          : raw;
        pushView(v);
        return;
      }
      case 'moveObj':
        updateMove(screenPt);
        return;
      case 'resize':
        updateResize(e, screenPt);
        return;
      case 'shape':
        updateShape(e, screenPt);
        return;
      case 'crop-new':
      case 'crop-move':
      case 'crop-resize':
        updateCrop(e, screenPt);
        return;
      default:
        break;
    }
  }

  function onPointerUp(e) {
    if (!drag) return;
    if (e.pointerId !== drag.pointerId) return;
    const screenPt = clientToStage(e);
    const d = drag;
    drag = null;
    releaseCapture(e);

    switch (d.mode) {
      case 'pan':
        if (d.deselectOnClick && !d.moved) selectObject(null);
        setCursor(spaceHeld ? 'grab' : defaultCursor());
        break;
      case 'moveObj':
        if (d.moved) commit('移動物件');
        setCursor('move');
        break;
      case 'resize':
        if (d.moved) commit(d.handle === 'start' || d.handle === 'end' ? '調整箭頭' : '調整大小');
        setCursor('default');
        break;
      case 'brush':
        finishBrush();
        setCursor(defaultCursor());
        break;
      case 'shape':
        finishShape(d, screenPt);
        setCursor(defaultCursor());
        break;
      case 'arrow-drag':
        finishArrowDrag(d, screenPt);
        setCursor(defaultCursor());
        break;
      case 'crop-new':
      case 'crop-move':
      case 'crop-resize':
        finishCrop(d, screenPt);
        setCursor(defaultCursor());
        break;
      default:
        break;
    }
    updateHoverCursor(screenPt);
  }

  function onPointerCancel(e) {
    if (!drag) return;
    const d = drag;
    drag = null;
    releaseCapture(e);
    if (d.mode === 'brush') finishBrush();
    if (d.mode === 'shape') {
      shapeDraft = null;
      requestRender?.();
    }
    if (d.mode === 'arrow-drag') {
      arrowPending = null;
      paintOverlay();
    }
    if (d.mode === 'crop-new' || d.mode === 'crop-move' || d.mode === 'crop-resize') {
      setCrop(d.prevCrop);
      requestRender?.();
    }
    setCursor(defaultCursor());
  }

  function updateHoverCursor(screenPt) {
    hoverScreen = screenPt;
    if (!stage || !state.doc) {
      setCursor(defaultCursor());
      return;
    }
    if (spaceHeld) {
      setCursor('grab');
      return;
    }
    if (isPreview()) {
      setCursor(canPanPreview() ? 'grab' : 'default');
      return;
    }
    const tool = state.tool;
    const imgPt = toImage(screenPt);
    if (tool === 'move') {
      const sel = findSelected();
      if (sel) {
        const handle = objectHandleAt(screenPt, sel);
        if (handle) {
          setCursor(HANDLE_CURSOR[handle] || 'default');
          return;
        }
      }
      const arrows = state.objects.filter((o) => o.type === 'arrow' || o.type === 'line');
      const hit = (arrows.length ? hitTest(arrows, imgPt, HIT_TOL_PX, state.view) : null) ||
        hitTest(state.objects, imgPt, HIT_TOL_PX, state.view);
      setCursor(hit ? 'move' : 'default');
      return;
    }
    if (tool === 'crop') {
      const c = state.crop;
      if (c && c.w >= 1 && c.h >= 1) {
        const handle = cropHandleAt(screenPt);
        if (handle) {
          setCursor(HANDLE_CURSOR[handle] || 'crosshair');
          return;
        }
        if (imgPt.x >= c.x && imgPt.x <= c.x + c.w && imgPt.y >= c.y && imgPt.y <= c.y + c.h) {
          setCursor('move');
          return;
        }
      }
      setCursor('crosshair');
      return;
    }
    setCursor(tool === 'text' ? 'text' : 'crosshair');
  }

  // -------------------------------------------------------------------------
  // Wheel / keyboard
  // -------------------------------------------------------------------------

  function onWheel(e) {
    if (!state.doc) return;
    if (isPreview()) {
      if (!e.deltaY) return;
      e.preventDefault();
      // 預覽階段滾輪即縮放（單一滾輪刻度約 10–15%），以畫布中心為錨點，
      // 縮放下限停在「適應視窗」，縮放後保持置中並夾在可視範圍內。
      const factor = clamp(Math.exp(-e.deltaY * 0.0013), 0.86, 1.15);
      const { w: cssW, h: cssH } = stageCssSize();
      const min = minScale(state.doc, cssW, cssH);
      const next = Math.max(min, clampScale((state.view.scale || 1) * factor));
      const centered = centeredView(state.doc.width, state.doc.height, cssW, cssH, next, 'manual');
      const v = clampPan(centered, state.doc, cssW, cssH);
      pushView(v);
      setCursor(canPanPreview() ? 'grab' : 'default');
      return;
    }
    const screenPt = clientToStage(e);
    if (e.ctrlKey || e.metaKey) {
      e.preventDefault();
      const factor = Math.exp(-e.deltaY * 0.0015);
      const v = applyZoom(state.view, screenPt.x, screenPt.y, factor);
      pushView(v);
      return;
    }
    if (e.deltaX || e.deltaY) {
      e.preventDefault();
      const v = clampToViewport(panBy(state.view, -e.deltaX, -e.deltaY));
      pushView(v);
    }
  }

  /** Space held on the canvas temporarily becomes the hand tool. */
  function isCanvasTarget(target) {
    return target === stage || target === board || target === overlay ||
      target === document.body || target === document.documentElement;
  }

  function onKeyDown(e) {
    if (e.key === 'Escape' && arrowPending) {
      // 取消未完成的箭頭（不 commit），也不要讓 main 的 Esc 順便退選/退出工具。
      e.preventDefault();
      if (typeof e.stopImmediatePropagation === 'function') e.stopImmediatePropagation();
      cancelArrowPending();
      return;
    }
    if (e.code !== 'Space' && e.key !== ' ') return;
    if (isPreview()) return; // 預覽階段畫布不可拖曳，Space 不作用
    if (isTypingTarget(e.target) || !isCanvasTarget(e.target)) return;
    if (e.repeat) return;
    if (drag) return;
    spaceHeld = true;
    setCursor('grab');
    e.preventDefault();
  }

  function onKeyUp(e) {
    if (e.code !== 'Space' && e.key !== ' ') return;
    if (!spaceHeld) return;
    spaceHeld = false;
    setCursor(defaultCursor());
  }

  function onWindowBlur() {
    spaceHeld = false;
    arrowPending = null;
    if (drag && drag.mode === 'brush') {
      drag = null;
      finishBrush();
    } else if (drag && drag.mode === 'shape') {
      drag = null;
      shapeDraft = null;
    } else {
      drag = null;
    }
    setCursor(defaultCursor());
    paintOverlay();
  }

  function onPointerLeave() {
    hoverScreen = null;
    if (state.tool === 'move' && state.selectedId) paintOverlay();
  }

  function onContextMenu(e) {
    e.preventDefault();
  }

  // -------------------------------------------------------------------------
  // Wire up
  // -------------------------------------------------------------------------

  stage.addEventListener('pointerdown', onPointerDown);
  stage.addEventListener('pointermove', onPointerMove);
  stage.addEventListener('pointerup', onPointerUp);
  stage.addEventListener('pointercancel', onPointerCancel);
  stage.addEventListener('wheel', onWheel, { passive: false });
  stage.addEventListener('pointerleave', onPointerLeave);
  stage.addEventListener('contextmenu', onContextMenu);
  window.addEventListener('keydown', onKeyDown);
  window.addEventListener('keyup', onKeyUp);
  window.addEventListener('blur', onWindowBlur);

  // 階段切換（不論從哪裡發起）都要清掉進行中的手勢與 overlay 殘留；
  // 切換工具或換圖時也要丟掉未完成的箭頭。
  let lastPhase = state.phase;
  const unsubState = subscribe((scope) => {
    if (arrowPending && (scope === 'tool' || scope === 'doc')) {
      cancelArrowPending();
    }
    if (state.phase !== lastPhase) {
      lastPhase = state.phase;
      cancelActiveGesture();
      setCursor(defaultCursor());
      paintOverlay();
    }
  });

  setCursor(defaultCursor());
  paintOverlay();

  function detach() {
    stage.removeEventListener('pointerdown', onPointerDown);
    stage.removeEventListener('pointermove', onPointerMove);
    stage.removeEventListener('pointerup', onPointerUp);
    stage.removeEventListener('pointercancel', onPointerCancel);
    stage.removeEventListener('wheel', onWheel);
    stage.removeEventListener('pointerleave', onPointerLeave);
    stage.removeEventListener('contextmenu', onContextMenu);
    if (typeof unsubState === 'function') unsubState();
    window.removeEventListener('keydown', onKeyDown);
    window.removeEventListener('keyup', onKeyUp);
    window.removeEventListener('blur', onWindowBlur);
  }

  return { paintOverlay, detach };
}
