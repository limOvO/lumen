// Annotation objects: creation, rendering, hit testing and geometry edits.
//
// Every object is stored in SOURCE IMAGE pixels (the space of `doc.width` x
// `doc.height`). Rendering composes the viewport transform so the same object
// renders correctly on screen at any zoom, and identically when flattened for
// export. Nothing in this module knows about DPR or screen coordinates.

import { imageToScreen } from './viewport.js';

const DEFAULTS = { color: '#ff5c5c', size: 8, opacity: 1, fill: false, fontSize: 36 };

let seq = 0;
function newId() {
  seq += 1;
  return 'o' + seq.toString(36) + Math.random().toString(36).slice(2, 7);
}

function normStyle(style) {
  const s = style || {};
  const num = (v, d) => (Number.isFinite(+v) && +v > 0 ? +v : d);
  return {
    color: typeof s.color === 'string' && s.color ? s.color : DEFAULTS.color,
    size: num(s.size, DEFAULTS.size),
    opacity: Number.isFinite(+s.opacity) ? Math.min(1, Math.max(0, +s.opacity)) : DEFAULTS.opacity,
    fill: !!s.fill,
    dashed: !!s.dashed,
    fontSize: num(s.fontSize, DEFAULTS.fontSize),
  };
}

/** Dash pattern derived from stroke width so it stays proportional. */
function applyDash(ctx, st) {
  if (st && st.dashed) {
    const w = Math.max(1, st.size);
    ctx.setLineDash([w * 2.4, w * 1.6]);
  } else {
    ctx.setLineDash([]);
  }
}

function normRect(x, y, w, h) {
  return {
    x: w < 0 ? x + w : x,
    y: h < 0 ? y + h : y,
    w: Math.abs(w),
    h: Math.abs(h),
  };
}

// --- Arrow geometry -------------------------------------------------------
// Arrows are defined by exactly two points (start, end) in image pixels.
// `points` is authoritative; x/y/w/h are kept in sync for bounds/hit/selection
// compatibility and for documents written by older versions (x/y + signed w/h).

/** Two endpoints of an arrow: [start, end] in image pixels. */
export function arrowPoints(obj) {
  if (obj && Array.isArray(obj.points) && obj.points.length >= 2) {
    const a = obj.points[0] || {};
    const b = obj.points[1] || {};
    return [
      { x: Number(a.x) || 0, y: Number(a.y) || 0 },
      { x: Number(b.x) || 0, y: Number(b.y) || 0 },
    ];
  }
  const x = Number(obj?.x) || 0;
  const y = Number(obj?.y) || 0;
  return [
    { x, y },
    { x: x + (Number(obj?.w) || 0), y: y + (Number(obj?.h) || 0) },
  ];
}

/** Keep x/y/w/h mirroring the two endpoints (x/y = start, w/h = signed delta). */
function syncArrowBounds(obj) {
  const pts = arrowPoints(obj);
  obj.x = pts[0].x;
  obj.y = pts[0].y;
  obj.w = pts[1].x - pts[0].x;
  obj.h = pts[1].y - pts[0].y;
  return obj;
}

/** Move one endpoint of an arrow and refresh the mirrored bounds. */
export function setArrowPoint(obj, which, pt) {
  if (!obj || (obj.type !== 'arrow' && obj.type !== 'line') || !pt) return;
  const pts = arrowPoints(obj);
  const next = { x: Number(pt.x) || 0, y: Number(pt.y) || 0 };
  if (which === 'start') pts[0] = next;
  else if (which === 'end') pts[1] = next;
  else return;
  obj.points = pts;
  syncArrowBounds(obj);
}

/** Create an object at an image-space point (or rect for shapes). */
export function createObject(type, p, style) {
  const st = normStyle(style);
  const pt = { x: Number(p?.x) || 0, y: Number(p?.y) || 0 };
  if (type === 'brush') {
    const size = st.size;
    return {
      id: newId(),
      type: 'brush',
      points: [{ x: pt.x, y: pt.y, w: Math.max(0.65 * size / 8, size * 0.82) }],
      style: st,
    };
  }
  if (type === 'text') {
    return {
      id: newId(),
      type: 'text',
      x: pt.x,
      y: pt.y,
      w: Math.max(0, Number(p?.w) || 0),
      h: Math.max(0, Number(p?.h) || 0),
      text: typeof p?.text === 'string' ? p.text : '',
      style: st,
    };
  }
  const w = Number(p?.w) || 0;
  const h = Number(p?.h) || 0;
  const id = newId();
  if (type === 'arrow' || type === 'line') {
    const pts = Array.isArray(p?.points) && p.points.length >= 2
      ? [
        { x: Number(p.points[0]?.x) || 0, y: Number(p.points[0]?.y) || 0 },
        { x: Number(p.points[1]?.x) || 0, y: Number(p.points[1]?.y) || 0 },
      ]
      : [{ x: pt.x, y: pt.y }, { x: pt.x + w, y: pt.y + h }];
    return syncArrowBounds({ id, type, points: pts, style: st });
  }
  const r = normRect(pt.x, pt.y, w, h);
  return { id, type, x: r.x, y: r.y, w: r.w, h: r.h, style: st };
}

// ---------------------------------------------------------------------------
// Brush rendering — three-point midpoint quadratic curves, width from the
// starting point of each segment (matches the "手绘笔刷画布" design), so the
// stroke keeps round caps/joins and no visible corners.
// ---------------------------------------------------------------------------

function mid(a, b) {
  return { x: (a.x + b.x) / 2, y: (a.y + b.y) / 2 };
}

/**
 * Paint a brush object from segment `from` onwards. `from` is 1-based to match
 * `paintStroke(cur, cur.lastDrawn + 1)` incremental drawing; pass 1 for a full
 * repaint. Assumes the caller already set fill/stroke alpha and the transform.
 */
export function drawBrush(ctx, obj, from = 1) {
  const pts = obj && obj.points;
  if (!pts || !pts.length) return;
  const color = obj.style?.color || DEFAULTS.color;
  ctx.strokeStyle = color;
  ctx.fillStyle = color;
  ctx.lineCap = 'round';
  ctx.lineJoin = 'round';
  applyDash(ctx, obj.style);
  if (pts.length === 1) {
    ctx.beginPath();
    ctx.arc(pts[0].x, pts[0].y, Math.max(0.7, pts[0].w / 2), 0, Math.PI * 2);
    ctx.fill();
    return;
  }
  for (let i = Math.max(1, from); i < pts.length; i += 1) {
    const p0 = i === 1 ? pts[0] : mid(pts[i - 2], pts[i - 1]);
    const p1 = mid(pts[i - 1], pts[i]);
    ctx.beginPath();
    ctx.moveTo(p0.x, p0.y);
    ctx.quadraticCurveTo(pts[i - 1].x, pts[i - 1].y, p1.x, p1.y);
    ctx.lineWidth = Math.max(0.6, pts[i - 1].w);
    ctx.stroke();
  }
}

function textFont(obj) {
  const size = normStyle(obj.style).fontSize;
  // Values are image-space px; the view transform scales them on screen.
  return `600 ${size}px "Noto Sans CJK TC", "PingFang TC", "Microsoft JhengHei", system-ui, sans-serif`;
}

const measureCache = new Map();
let measureCtx = null;

function measureText(obj) {
  const text = obj.text || '';
  const lines = text.split('\n');
  const font = textFont(obj);
  const key = font + '\u0000' + text;
  let cached = measureCache.get(key);
  if (!cached) {
    if (!measureCtx) {
      // Text metrics do not need a visible canvas; this never touches the DOM
      // layout, only an offscreen 2D context.
      const c = document.createElement('canvas');
      measureCtx = c.getContext('2d');
    }
    measureCtx.font = font;
    let width = 0;
    for (const line of lines) width = Math.max(width, measureCtx.measureText(line).width);
    const size = normStyle(obj.style).fontSize;
    cached = { w: Math.max(width, size * 0.5), h: Math.max(lines.length * size * 1.28, size) };
    if (measureCache.size > 400) measureCache.clear();
    measureCache.set(key, cached);
  }
  return { ...cached, lines, lineHeight: normStyle(obj.style).fontSize * 1.28 };
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

function drawOne(ctx, obj) {
  if (!obj || !obj.type) return;
  const st = normStyle(obj.style);
  ctx.save();
  ctx.globalAlpha = st.opacity;
  ctx.strokeStyle = st.color;
  ctx.fillStyle = st.color;
  ctx.lineCap = 'round';
  ctx.lineJoin = 'round';

  switch (obj.type) {
    case 'brush': {
      drawBrush(ctx, obj, 1);
      break;
    }
    case 'rect': {
      const r = normRect(obj.x, obj.y, obj.w, obj.h);
      if (st.fill) ctx.fillRect(r.x, r.y, r.w, r.h);
      ctx.lineWidth = st.size;
      applyDash(ctx, st);
      ctx.strokeRect(r.x, r.y, r.w, r.h);
      ctx.setLineDash([]);
      break;
    }
    case 'ellipse': {
      const r = normRect(obj.x, obj.y, obj.w, obj.h);
      ctx.beginPath();
      ctx.ellipse(r.x + r.w / 2, r.y + r.h / 2, Math.max(0.5, r.w / 2), Math.max(0.5, r.h / 2), 0, 0, Math.PI * 2);
      if (st.fill) ctx.fill();
      ctx.lineWidth = st.size;
      applyDash(ctx, st);
      ctx.stroke();
      ctx.setLineDash([]);
      break;
    }
    case 'line': {
      const pts = arrowPoints(obj);
      const len = Math.hypot(pts[1].x - pts[0].x, pts[1].y - pts[0].y);
      if (len < 0.01) break;
      ctx.lineWidth = st.size;
      applyDash(ctx, st);
      ctx.beginPath();
      ctx.moveTo(pts[0].x, pts[0].y);
      ctx.lineTo(pts[1].x, pts[1].y);
      ctx.stroke();
      ctx.setLineDash([]);
      break;
    }
    case 'arrow': {
      // 方向一律由兩個端點決定，w/h 只鏡射用。
      const pts = arrowPoints(obj);
      const x0 = pts[0].x;
      const y0 = pts[0].y;
      const x1 = pts[1].x;
      const y1 = pts[1].y;
      const dx = x1 - x0;
      const dy = y1 - y0;
      const len = Math.hypot(dx, dy);
      if (len < 0.01) break;
      const ux = dx / len;
      const uy = dy / len;
      const head = Math.max(st.size * 3.2, st.size + 6);
      const neck = Math.min(len * 0.6, head * 0.85);
      ctx.lineWidth = st.size;
      applyDash(ctx, st);
      ctx.beginPath();
      ctx.moveTo(x0, y0);
      ctx.lineTo(x1 - ux * neck, y1 - uy * neck);
      ctx.stroke();
      ctx.setLineDash([]);
      const half = head * 0.42;
      ctx.beginPath();
      ctx.moveTo(x1, y1);
      ctx.lineTo(x1 - ux * head + uy * half, y1 - uy * head - ux * half);
      ctx.lineTo(x1 - ux * head - uy * half, y1 - uy * head + ux * half);
      ctx.closePath();
      ctx.fill();
      break;
    }
    case 'text': {
      const m = measureText(obj);
      ctx.font = textFont(obj);
      ctx.textAlign = 'left';
      ctx.textBaseline = 'top';
      let y = obj.y;
      for (const line of m.lines) {
        ctx.fillText(line, obj.x, y);
        y += m.lineHeight;
      }
      break;
    }
    default:
      break;
  }
  ctx.restore();
}

/**
 * Draw annotations. The ctx is expected to already carry the DPR transform;
 * this only composes view translate/scale. `opts.preview` draws one extra
 * object last (the in-progress shape), `opts.selectedId`/`showHandles` are
 * accepted for API compatibility — the selection frame and handles belong to
 * interaction.js's overlay pass so moving an object never repaints the image.
 */
export function drawObjects(ctx, objects, view, dpr, opts = {}) {
  if (!ctx) return;
  ctx.save();
  ctx.translate(view?.tx || 0, view?.ty || 0);
  const scale = view && view.scale > 0 ? view.scale : 1;
  ctx.scale(scale, scale);
  if (Array.isArray(objects)) {
    for (const obj of objects) drawOne(ctx, obj);
  }
  if (opts && opts.preview) drawOne(ctx, opts.preview);
  ctx.restore();
  void dpr;
  void opts?.selectedId;
  void opts?.showHandles;
}

// ---------------------------------------------------------------------------
// Geometry
// ---------------------------------------------------------------------------

/** Axis-aligned bounds in image pixels. */
export function objectBounds(obj) {
  if (!obj || !obj.type) return { x: 0, y: 0, w: 0, h: 0 };
  if (obj.type === 'brush') {
    const pts = obj.points || [];
    if (!pts.length) return { x: 0, y: 0, w: 0, h: 0 };
    let minX = Infinity;
    let minY = Infinity;
    let maxX = -Infinity;
    let maxY = -Infinity;
    for (const p of pts) {
      const half = Math.max(0.35, p.w / 2);
      if (p.x - half < minX) minX = p.x - half;
      if (p.y - half < minY) minY = p.y - half;
      if (p.x + half > maxX) maxX = p.x + half;
      if (p.y + half > maxY) maxY = p.y + half;
    }
    return { x: minX, y: minY, w: Math.max(1, maxX - minX), h: Math.max(1, maxY - minY) };
  }
  if (obj.type === 'text') {
    const m = measureText(obj);
    return { x: obj.x, y: obj.y, w: m.w, h: m.h };
  }
  if (obj.type === 'arrow' || obj.type === 'line') {
    const pts = arrowPoints(obj);
    const x = Math.min(pts[0].x, pts[1].x);
    const y = Math.min(pts[0].y, pts[1].y);
    return {
      x,
      y,
      w: Math.max(1, Math.abs(pts[1].x - pts[0].x)),
      h: Math.max(1, Math.abs(pts[1].y - pts[0].y)),
    };
  }
  return normRect(obj.x, obj.y, obj.w, obj.h);
}

const HANDLE_NAMES = ['nw', 'n', 'ne', 'e', 'se', 's', 'sw', 'w'];

/** Resize handles in image pixels: 2 endpoints for arrows, 8 box handles otherwise. */
export function handlesFor(obj) {
  if (obj && (obj.type === 'arrow' || obj.type === 'line')) {
    const pts = arrowPoints(obj);
    return [
      { name: 'start', x: pts[0].x, y: pts[0].y },
      { name: 'end', x: pts[1].x, y: pts[1].y },
    ];
  }
  const b = objectBounds(obj);
  const cx = b.x + b.w / 2;
  const cy = b.y + b.h / 2;
  const pos = {
    nw: [b.x, b.y],
    n: [cx, b.y],
    ne: [b.x + b.w, b.y],
    e: [b.x + b.w, cy],
    se: [b.x + b.w, b.y + b.h],
    s: [cx, b.y + b.h],
    sw: [b.x, b.y + b.h],
    w: [b.x, cy],
  };
  return HANDLE_NAMES.map((name) => ({ name, x: pos[name][0], y: pos[name][1] }));
}

export function moveObject(obj, dx, dy) {
  if (!obj) return;
  if (obj.type === 'brush') {
    for (const p of obj.points || []) {
      p.x += dx;
      p.y += dy;
    }
    return;
  }
  if (obj.type === 'arrow' || obj.type === 'line') {
    const pts = arrowPoints(obj);
    pts[0].x += dx;
    pts[0].y += dy;
    pts[1].x += dx;
    pts[1].y += dy;
    obj.points = pts;
    syncArrowBounds(obj);
    return;
  }
  obj.x += dx;
  obj.y += dy;
}

/**
 * Resize an object by dragging `handle` to `imgPt` (image pixels).
 * `keepRatio` keeps the original aspect ratio; anchored at the opposite edge.
 */
export function resizeObject(obj, handle, imgPt, keepRatio = false) {
  if (!obj || !imgPt) return;
  if ((obj.type === 'arrow' || obj.type === 'line') && (handle === 'start' || handle === 'end')) {
    // 箭頭只有兩個控制點：拖哪一端就只動那一端（比例鎖定無意義）。
    setArrowPoint(obj, handle, imgPt);
    return;
  }
  const b = objectBounds(obj);
  const baseW = Math.max(1, b.w);
  const baseH = Math.max(1, b.h);
  const dw = handle.includes('w');
  const de = handle.includes('e');
  const dn = handle.includes('n');
  const ds = handle.includes('s');

  let left = b.x;
  let top = b.y;
  let width = baseW;
  let height = baseH;

  if (dw) {
    const right = b.x + baseW;
    left = Math.min(imgPt.x, right - 1);
    width = right - left;
  } else if (de) {
    width = Math.max(1, imgPt.x - b.x);
  }
  if (dn) {
    const bottom = b.y + baseH;
    top = Math.min(imgPt.y, bottom - 1);
    height = bottom - top;
  } else if (ds) {
    height = Math.max(1, imgPt.y - b.y);
  }

  if (keepRatio) {
    const ratio = baseW / baseH;
    const isCorner = (dw || de) && (dn || ds);
    if (isCorner) {
      // Corner: follow the larger relative change so the pointer stays close.
      const s = Math.max(width / baseW, height / baseH);
      width = baseW * s;
      height = baseH * s;
      if (dw) left = b.x + baseW - width;
      if (dn) top = b.y + baseH - height;
    } else if (dn || ds) {
      width = height * ratio;
      left = dw ? b.x + baseW - width : b.x;
    } else if (dw || de) {
      height = width / ratio;
      top = dn ? b.y + baseH - height : b.y;
    }
  }

  width = Math.max(1, width);
  height = Math.max(1, height);

  if (obj.type === 'brush') {
    const sx = width / baseW;
    const sy = height / baseH;
    const sw = (sx + sy) / 2;
    for (const p of obj.points || []) {
      p.x = left + (p.x - b.x) * sx;
      p.y = top + (p.y - b.y) * sy;
      p.w = Math.max(0.65, p.w * sw);
    }
    return;
  }
  obj.x = left;
  obj.y = top;
  obj.w = width;
  obj.h = height;
  if (obj.type === 'text') {
    obj.style.fontSize = Math.max(4, normStyle(obj.style).fontSize * (height / baseH));
  }
}

/** Deep copy with a fresh id (styles and points are cloned). */
export function cloneObject(obj) {
  if (!obj) return null;
  const copy = JSON.parse(JSON.stringify(obj));
  copy.id = newId();
  if (!copy.style) copy.style = normStyle(copy.style);
  return copy;
}

// ---------------------------------------------------------------------------
// Hit testing
// ---------------------------------------------------------------------------

function pointSegDistance(px, py, ax, ay, bx, by) {
  const dx = bx - ax;
  const dy = by - ay;
  const len2 = dx * dx + dy * dy;
  if (len2 === 0) return Math.hypot(px - ax, py - ay);
  let t = ((px - ax) * dx + (py - ay) * dy) / len2;
  t = Math.max(0, Math.min(1, t));
  return Math.hypot(px - (ax + t * dx), py - (ay + t * dy));
}

function hitObject(obj, pt, tol) {
  if (!obj) return false;
  const st = normStyle(obj.style);
  if (obj.type === 'brush') {
    const pts = obj.points || [];
    if (!pts.length) return false;
    if (pts.length === 1) {
      return Math.hypot(pt.x - pts[0].x, pt.y - pts[0].y) <= tol + pts[0].w / 2;
    }
    for (let i = 1; i < pts.length; i += 1) {
      const a = pts[i - 1];
      const b = pts[i];
      const r = tol + Math.max(a.w, b.w) / 2;
      if (pointSegDistance(pt.x, pt.y, a.x, a.y, b.x, b.y) <= r) return true;
    }
    return false;
  }
  if (obj.type === 'text') {
    const b = objectBounds(obj);
    const pad = tol;
    return pt.x >= b.x - pad && pt.x <= b.x + b.w + pad && pt.y >= b.y - pad && pt.y <= b.y + b.h + pad;
  }
  if (obj.type === 'rect') {
    const b = normRect(obj.x, obj.y, obj.w, obj.h);
    const pad = tol + st.size / 2;
    const insideX = pt.x >= b.x - pad && pt.x <= b.x + b.w + pad;
    const insideY = pt.y >= b.y - pad && pt.y <= b.y + b.h + pad;
    if (!insideX || !insideY) return false;
    if (st.fill) return true;
    const inCoreX = pt.x >= b.x + pad && pt.x <= b.x + b.w - pad;
    const inCoreY = pt.y >= b.y + pad && pt.y <= b.y + b.h - pad;
    return !(inCoreX && inCoreY);
  }
  if (obj.type === 'ellipse') {
    const b = normRect(obj.x, obj.y, obj.w, obj.h);
    const rx = Math.max(0.5, b.w / 2);
    const ry = Math.max(0.5, b.h / 2);
    const nx = (pt.x - (b.x + rx)) / rx;
    const ny = (pt.y - (b.y + ry)) / ry;
    const r = Math.hypot(nx, ny);
    if (r <= 1 && st.fill) return true;
    const band = (tol + st.size / 2) / Math.max(0.5, Math.min(rx, ry));
    return Math.abs(r - 1) <= band;
  }
  if (obj.type === 'arrow' || obj.type === 'line') {
    const pts = arrowPoints(obj);
    const x0 = pts[0].x;
    const y0 = pts[0].y;
    const x1 = pts[1].x;
    const y1 = pts[1].y;
    const d = pointSegDistance(pt.x, pt.y, x0, y0, x1, y1);
    if (d <= tol + st.size / 2) return true;
    if (obj.type !== 'arrow') return false;
    const head = Math.max(st.size * 3.2, st.size + 6);
    return Math.hypot(pt.x - x1, pt.y - y1) <= head * 0.55;
  }
  return false;
}

/**
 * Topmost object under an image-space point.
 *
 * `tolPx` is given in SCREEN pixels; pass the current `view` so it is converted
 * to image pixels internally. Without a view (pure geometry tests) `tolPx` is
 * treated as image pixels.
 */
export function hitTest(objects, imgPt, tolPx = 6, view) {
  if (!Array.isArray(objects) || !imgPt) return null;
  const scale = view && view.scale > 0 ? view.scale : 1;
  const tol = Math.max(0.5, (Number(tolPx) || 0) / scale);
  for (let i = objects.length - 1; i >= 0; i -= 1) {
    if (hitObject(objects[i], imgPt, tol)) return objects[i];
  }
  return null;
}

// Exported for interaction's hover cursor logic (image px -> screen px).
export function screenBounds(obj, view) {
  const b = objectBounds(obj);
  const p0 = imageToScreen(view, b.x, b.y);
  const p1 = imageToScreen(view, b.x + b.w, b.y + b.h);
  return { x: p0.x, y: p0.y, w: p1.x - p0.x, h: p1.y - p0.y };
}
