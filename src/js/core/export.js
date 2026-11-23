// Export pipeline: crop/scale the document, re-draw annotations at the same
// ratio, then encode to the requested format. Everything happens on canvas in
// the WebView — Rust only writes the resulting bytes.

import { bridge } from '../main.js';
import { drawObjects } from './annotate.js';

const FORMAT_MIME = { jpeg: 'image/jpeg', png: 'image/png', webp: 'image/webp' };

/** Map an output format (possibly 'auto') to 'jpeg' | 'png' | 'webp'. */
export function resolveFormat(format, doc) {
  const f = String(format || '').toLowerCase();
  if (f === 'jpeg' || f === 'jpg') return 'jpeg';
  if (f === 'png' || f === 'webp') return f;
  const hint = String(doc?.mime || doc?.format || '').toLowerCase();
  if (hint.includes('png')) return 'png';
  if (hint.includes('webp')) return 'webp';
  if (hint.includes('jpeg') || hint.includes('jpg')) return 'jpeg';
  return 'png';
}

function clamp(v, lo, hi) {
  return Math.min(hi, Math.max(lo, v));
}

function baseName(path) {
  const s = String(path || '');
  const i = Math.max(s.lastIndexOf('/'), s.lastIndexOf('\\'));
  return i >= 0 ? s.slice(i + 1) : s;
}

function dirName(path) {
  const s = String(path || '');
  const i = Math.max(s.lastIndexOf('/'), s.lastIndexOf('\\'));
  return i > 0 ? s.slice(0, i) : '';
}

/** Clamp the crop rect to the image and resolve the final pixel dimensions. */
function targetDims(doc, crop, maxDim, extraScale) {
  const W = Math.max(1, doc.width || doc.bitmap?.width || 1);
  const H = Math.max(1, doc.height || doc.bitmap?.height || 1);
  let rect = { x: 0, y: 0, w: W, h: H };
  if (crop && crop.w > 0 && crop.h > 0) {
    const x0 = clamp(crop.x, 0, W);
    const y0 = clamp(crop.y, 0, H);
    const x1 = clamp(crop.x + crop.w, 0, W);
    const y1 = clamp(crop.y + crop.h, 0, H);
    rect = { x: x0, y: y0, w: Math.max(1, x1 - x0), h: Math.max(1, y1 - y0) };
  }
  const extra = Number(extraScale) > 0 ? Number(extraScale) : 1;
  let rawW = rect.w * extra;
  let rawH = rect.h * extra;
  const max = Number(maxDim);
  if (max > 0 && Math.max(rawW, rawH) > max) {
    const f = max / Math.max(rawW, rawH);
    rawW *= f;
    rawH *= f;
  }
  const outW = Math.max(1, Math.round(rawW));
  const outH = Math.max(1, Math.round(rawH));
  return { rect, outW, outH, scale: outW / rect.w };
}

/**
 * Draw the crop of the image plus its annotations into a fresh canvas.
 * The bitmap may be downsampled (doc.decodedScale < 1), so the source rect is
 * converted into bitmap pixels while annotations stay in logical pixels.
 */
function renderToCanvas(doc, objects, rect, outW, outH) {
  const canvas = document.createElement('canvas');
  canvas.width = Math.max(1, Math.round(outW));
  canvas.height = Math.max(1, Math.round(outH));
  const ctx = canvas.getContext('2d');
  ctx.imageSmoothingEnabled = true;
  ctx.imageSmoothingQuality = 'high';

  const bitmap = doc.bitmap;
  if (bitmap) {
    const ds = doc.decodedScale > 0 ? doc.decodedScale : 1;
    const bmpW = bitmap.width;
    const bmpH = bitmap.height;
    const sx = clamp(rect.x * ds, 0, Math.max(0, bmpW - 1));
    const sy = clamp(rect.y * ds, 0, Math.max(0, bmpH - 1));
    const sw = clamp(rect.w * ds, 1, Math.max(1, bmpW - sx));
    const sh = clamp(rect.h * ds, 1, Math.max(1, bmpH - sy));
    ctx.drawImage(bitmap, sx, sy, sw, sh, 0, 0, canvas.width, canvas.height);
  }

  if (Array.isArray(objects) && objects.length) {
    const scale = canvas.width / rect.w;
    drawObjects(ctx, objects, {
      scale,
      tx: -rect.x * scale,
      ty: -rect.y * scale,
      fitMode: 'manual',
    }, 1);
  }
  return canvas;
}

/**
 * Flatten the document: crop + optional downscale + annotations.
 * `opts.scale` is an extra output multiplier (applied before maxDim capping).
 */
export async function flatten(doc, objects, { crop, maxDim, scale } = {}) {
  if (!doc) throw new Error('沒有可匯出的圖片');
  if (!doc.bitmap) throw new Error('圖片尚未解碼完成，無法匯出');
  const dims = targetDims(doc, crop, maxDim, scale);
  const canvas = renderToCanvas(doc, objects, dims.rect, dims.outW, dims.outH);
  return { canvas, width: dims.outW, height: dims.outH };
}

function canvasToBlob(canvas, mime, quality) {
  return new Promise((resolve, reject) => {
    try {
      canvas.toBlob(
        (blob) => {
          if (blob) resolve(blob);
          else reject(new Error('圖片編碼失敗'));
        },
        mime,
        quality,
      );
    } catch (err) {
      reject(err);
    }
  });
}

/** JPEG has no alpha: paint the flattened image over an opaque white sheet. */
function whiteBacked(canvas) {
  const out = document.createElement('canvas');
  out.width = canvas.width;
  out.height = canvas.height;
  const ctx = out.getContext('2d');
  ctx.fillStyle = '#ffffff';
  ctx.fillRect(0, 0, out.width, out.height);
  ctx.drawImage(canvas, 0, 0);
  return out;
}

/**
 * Encode a canvas. `bytes` is the encoded size in bytes (number), `blob` the
 * payload. Transparent formats keep alpha; JPEG is composited over white.
 */
export async function encode(canvas, format, quality) {
  if (!canvas) throw new Error('沒有可編碼的畫布');
  let fmt = resolveFormat(format, { mime: `image/${format}` });
  const q = Number.isFinite(+quality) ? clamp(+quality, 0.05, 0.98) : 0.85;
  let source = canvas;
  if (fmt === 'jpeg') source = whiteBacked(canvas);

  let blob = null;
  try {
    blob = await canvasToBlob(source, FORMAT_MIME[fmt], q);
  } catch (err) {
    blob = null;
  }
  if (!blob && fmt !== 'png') {
    // WebView without WebP/JPEG encoding support: PNG always works.
    fmt = 'png';
    try {
      blob = await canvasToBlob(source, FORMAT_MIME.png, q);
    } catch (err) {
      blob = null;
    }
  }
  if (!blob) throw new Error('圖片編碼失敗：此 WebView 不支援所需的編碼格式');
  return { blob, bytes: blob.size, mime: FORMAT_MIME[fmt] };
}

/** Binary-search the highest quality whose encoded size still fits the target. */
export async function encodeToTarget(canvas, format, targetKB) {
  const targetBytes = Math.max(1024, Math.round((Number(targetKB) || 0) * 1024));
  let lo = 0.1;
  let hi = 0.95;
  let best = null;
  let bestQuality = lo;
  let smallest = null;

  for (let i = 0; i < 8; i += 1) {
    const q = (lo + hi) / 2;
    const res = await encode(canvas, format, q);
    if (!smallest || res.bytes <= smallest.bytes) {
      smallest = { ...res, quality: q };
    }
    if (res.bytes <= targetBytes) {
      best = res;
      bestQuality = q;
      lo = q;
    } else {
      hi = q;
    }
    if (hi - lo < 0.02) break;
  }

  if (best) return { blob: best.blob, quality: bestQuality, bytes: best.bytes, mime: best.mime };
  return smallest
    ? { blob: smallest.blob, quality: smallest.quality, bytes: smallest.bytes, mime: smallest.mime }
    : { blob: null, quality: 0, bytes: 0, mime: FORMAT_MIME.png };
}

/**
 * Fast size estimate: render/encode a half-resolution preview and scale the
 * byte count by the pixel ratio. Not exact, but within a few percent for
 * photographic content and instantaneous compared to a full encode.
 */
export async function estimateSize(doc, objects, output, crop) {
  if (!doc || !doc.bitmap) return { bytes: 0, width: 0, height: 0 };
  const out = output || {};
  const fmt = resolveFormat(out.format, doc);
  const dims = targetDims(doc, crop, out.maxDim, out.scale);
  const previewW = Math.max(1, Math.round(dims.outW * 0.5));
  const previewH = Math.max(1, Math.round(dims.outH * 0.5));
  const preview = renderToCanvas(doc, objects, dims.rect, previewW, previewH);
  const quality = Number.isFinite(+out.quality) ? +out.quality : 0.85;
  const res = await encode(preview, fmt, quality);
  const pixelRatio = (dims.outW * dims.outH) / Math.max(1, previewW * previewH);
  return {
    bytes: Math.max(1, Math.round(res.bytes * pixelRatio)),
    width: dims.outW,
    height: dims.outH,
  };
}

/** Write an encoded blob to disk through the shell. Returns bytes written. */
export async function saveDoc({ path, blob } = {}) {
  if (!path) throw new Error('沒有指定儲存路徑');
  if (!blob) throw new Error('沒有可寫入的圖片資料');
  const bytes = new Uint8Array(await blob.arrayBuffer());
  try {
    const written = await bridge.invoke('write_file', { path, data: Array.from(bytes) });
    return Number(written) || bytes.length;
  } catch (err) {
    const msg = typeof err === 'string' ? err : err?.message;
    throw new Error(`寫入檔案失敗：${baseName(path)}${msg ? `（${msg}）` : ''}`);
  }
}

/** File extension for an output format. */
export function outputExtension(format, doc) {
  const fmt = resolveFormat(format, doc);
  return fmt === 'jpeg' ? 'jpg' : fmt;
}

/**
 * Suggested save name. When a source path is known the full path next to the
 * original is returned (main.js uses it as the save-as default), otherwise
 * just the file name, e.g. IMG_1234-edit.jpg.
 */
export function suggestedName(doc, format, sourcePath) {
  const ext = outputExtension(format, doc);
  const path = String(sourcePath || doc?.path || '');
  const name = doc?.name || baseName(path) || 'image';
  const stem = name.replace(/\.[^./\\]+$/, '') || 'image';
  const file = `${stem}-edit.${ext}`;
  const dir = dirName(path);
  if (!dir) return file;
  const sep = dir.includes('\\') && !dir.includes('/') ? '\\' : '/';
  return `${dir}${sep}${file}`;
}
