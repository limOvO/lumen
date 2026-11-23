// Viewport maths + scene primitives.
//
// Coordinate contract: `view.tx/ty` is the top-left corner of the image in
// canvas CSS pixels; `view.scale` is screen pixels per source-image pixel.
// Every screen <-> image conversion in the app lives here — tools never do
// their own scaling arithmetic.
//
// Rendering contract: callers (main.js render loop, export.flatten) hand us a
// 2D context that already carries the device-pixel transform
// (`setTransform(dpr,0,0,dpr,0,0)`). We therefore only *compose* view
// translate/scale on top and never touch dpr ourselves, so the same helpers
// work for a DPR-scaled screen canvas and a plain export canvas.

export const MIN_SCALE = 0.02;
export const MAX_SCALE = 16;

const CHECKER_CELL = 12; // CSS px，棋盤格方格邊長

export function clampScale(s) {
  const n = Number(s);
  if (!Number.isFinite(n) || n <= 0) return MIN_SCALE;
  return Math.min(MAX_SCALE, Math.max(MIN_SCALE, n));
}

/**
 * Scale that fits the image inside the given viewport. Small images are not
 * blown up: a 300px thumbnail stays at 100% instead of filling the window.
 */
export function fitScale(imgW, imgH, viewW, viewH) {
  if (!(imgW > 0) || !(imgH > 0)) return 1;
  const w = Math.max(1, Number(viewW) || 1);
  const h = Math.max(1, Number(viewH) || 1);
  return clampScale(Math.min(w / imgW, h / imgH, 1));
}

/**
 * Smallest allowed zoom: "fit the window" computed the same way main.js does
 * (stage size minus 64px of breathing room). Zooming out stops here so the
 * image never shrinks into a tiny thumbnail lost in the viewport.
 */
export function minScale(doc, cssW, cssH) {
  if (!doc) return MIN_SCALE;
  const w = Math.max(32, (Number(cssW) || 0) - 64);
  const h = Math.max(32, (Number(cssH) || 0) - 64);
  return fitScale(doc.width, doc.height, w, h);
}

/**
 * Clamp a view so the image can never be dragged out of the viewport.
 * - displayed image smaller than the viewport on an axis -> center it there
 * - otherwise keep `tx/ty` inside [viewportSize - imageSize, 0]
 * Returns a new view; the input is not mutated.
 */
export function clampPan(view, doc, cssW, cssH) {
  const v = view || {};
  if (!doc) return { ...v };
  const scale = v.scale > 0 ? v.scale : 1;
  const imgW = (doc.width || 0) * scale;
  const imgH = (doc.height || 0) * scale;
  const w = Number(cssW) || 0;
  const h = Number(cssH) || 0;
  let tx = Number.isFinite(v.tx) ? v.tx : 0;
  let ty = Number.isFinite(v.ty) ? v.ty : 0;
  if (imgW <= w) tx = (w - imgW) / 2;
  else tx = Math.min(0, Math.max(w - imgW, tx));
  if (imgH <= h) ty = (h - imgH) / 2;
  else ty = Math.min(0, Math.max(h - imgH, ty));
  return { ...v, scale, tx, ty };
}

/** Screen CSS px -> source image px. */
export function screenToImage(view, sx, sy) {
  const scale = view && view.scale > 0 ? view.scale : 1;
  const tx = view ? view.tx || 0 : 0;
  const ty = view ? view.ty || 0 : 0;
  return { x: (sx - tx) / scale, y: (sy - ty) / scale };
}

/** Source image px -> screen CSS px. */
export function imageToScreen(view, sx, sy) {
  const scale = view && view.scale > 0 ? view.scale : 1;
  const tx = view ? view.tx || 0 : 0;
  const ty = view ? view.ty || 0 : 0;
  return { x: sx * scale + tx, y: sy * scale + ty };
}

/** Compose the view transform onto whatever transform the ctx already has. */
export function applyView(ctx, view) {
  const scale = view && view.scale > 0 ? view.scale : 1;
  const tx = view ? view.tx || 0 : 0;
  const ty = view ? view.ty || 0 : 0;
  ctx.translate(tx, ty);
  ctx.scale(scale, scale);
}

/** Zoom around a screen anchor; returns a new view (never mutates). */
export function zoomAt(view, sx, sy, factor) {
  const scale = view && view.scale > 0 ? view.scale : 1;
  const f = Number(factor) > 0 ? Number(factor) : 1;
  const next = clampScale(scale * f);
  if (next === scale) return { ...view, scale };
  const anchor = screenToImage(view, sx, sy);
  return {
    ...view,
    scale: next,
    tx: sx - anchor.x * next,
    ty: sy - anchor.y * next,
    fitMode: 'manual',
  };
}

/** Convenience: zoom to an absolute scale around a screen anchor. */
export function zoomTo(view, targetScale, sx, sy) {
  const scale = view && view.scale > 0 ? view.scale : 1;
  return zoomAt(view, sx, sy, clampScale(targetScale) / scale);
}

/** Translate the view by screen pixels; returns a new view. */
export function panBy(view, dx, dy) {
  return {
    ...view,
    tx: (view?.tx || 0) + dx,
    ty: (view?.ty || 0) + dy,
    fitMode: 'manual',
  };
}

/** Centered view at `scale` for a viewport of viewW x viewH. */
export function centeredView(imgW, imgH, viewW, viewH, scale, fitMode = 'manual') {
  const s = clampScale(scale);
  return {
    scale: s,
    tx: (viewW - imgW * s) / 2,
    ty: (viewH - imgH * s) / 2,
    fitMode,
  };
}

// ---------------------------------------------------------------------------
// Drawing
// ---------------------------------------------------------------------------

function cssVar(name, fallback) {
  try {
    const v = getComputedStyle(document.documentElement).getPropertyValue(name).trim();
    return v || fallback;
  } catch (err) {
    return fallback;
  }
}

let checkerTile = null;
let checkerKey = '';

function checkerPattern(ctx, a, b) {
  const key = a + '|' + b;
  if (checkerTile && checkerKey === key) return ctx.createPattern(checkerTile, 'repeat');
  const size = CHECKER_CELL * 2;
  const tile = document.createElement('canvas');
  tile.width = size;
  tile.height = size;
  const tctx = tile.getContext('2d');
  tctx.fillStyle = a;
  tctx.fillRect(0, 0, size, size);
  tctx.fillStyle = b;
  tctx.fillRect(0, 0, CHECKER_CELL, CHECKER_CELL);
  tctx.fillRect(CHECKER_CELL, CHECKER_CELL, CHECKER_CELL, CHECKER_CELL);
  checkerTile = tile;
  checkerKey = key;
  return ctx.createPattern(tile, 'repeat');
}

/** Checkerboard backdrop for the transparent parts of the image. */
export function drawBackground(ctx, w, h, dpr) {
  const areaW = Math.max(0, Number(w) || 0);
  const areaH = Math.max(0, Number(h) || 0);
  if (!areaW || !areaH) return;
  ctx.save();
  const a = cssVar('--checker-a', '#141518');
  const b = cssVar('--checker-b', '#191c22');
  ctx.fillStyle = checkerPattern(ctx, a, b);
  ctx.fillRect(0, 0, areaW, areaH);
  ctx.restore();
  void dpr;
}

/** Paint the image body only (background + annotations are separate). */
export function drawDoc(ctx, doc, view, dpr) {
  const bitmap = doc && doc.bitmap;
  if (!bitmap) return;
  const w = doc.width > 0 ? doc.width : bitmap.width;
  const h = doc.height > 0 ? doc.height : bitmap.height;
  if (!(w > 0) || !(h > 0)) return;
  const scale = view && view.scale > 0 ? view.scale : 1;
  ctx.save();
  applyView(ctx, view);
  // Zoomed in past 1.5x the source pixels are worth showing crisply.
  ctx.imageSmoothingEnabled = scale < 1.5;
  if (ctx.imageSmoothingEnabled) ctx.imageSmoothingQuality = 'high';
  // doc.width/height are logical (EXIF-corrected) pixels; the bitmap may be
  // smaller for huge images, so stretch it to the logical box.
  ctx.drawImage(bitmap, 0, 0, bitmap.width, bitmap.height, 0, 0, w, h);
  ctx.restore();
  void dpr;
}
