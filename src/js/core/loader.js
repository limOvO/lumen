// Image loading and decoding.
//
// Rust hands us raw bytes, we sniff the real format from magic bytes (file
// extensions lie), decode with EXIF orientation applied, and keep both an
// ImageBitmap (for canvas drawing) and an object URL (for <img> based paths).
// Very large images are decoded downsampled so the WebView never allocates a
// 500 MB backing store; annotations stay in source-image coordinates because
// `decodedScale` is only meaningful to the renderer.

import { bridge } from '../main.js';

export const IMAGE_EXTS = [
  'jpg', 'jpeg', 'jpe', 'jfif', 'png', 'webp', 'gif', 'bmp', 'avif', 'svg', 'ico', 'tif', 'tiff',
];

/** Decoding above this many pixels gets downsampled. */
const MAX_PIXELS = 80e6;

const MIME_BY_EXT = {
  jpg: 'image/jpeg',
  jpeg: 'image/jpeg',
  jpe: 'image/jpeg',
  jfif: 'image/jpeg',
  png: 'image/png',
  webp: 'image/webp',
  gif: 'image/gif',
  bmp: 'image/bmp',
  avif: 'image/avif',
  svg: 'image/svg+xml',
  ico: 'image/x-icon',
  tif: 'image/tiff',
  tiff: 'image/tiff',
};

// 天生支援 alpha 的格式；main.js 用它決定是否只在圖片範圍內畫棋盤格。
const ALPHA_MIMES = new Set([
  'image/png', 'image/webp', 'image/gif', 'image/svg+xml', 'image/x-icon', 'image/avif',
]);

const FORMAT_BY_MIME = {
  'image/jpeg': 'JPEG',
  'image/png': 'PNG',
  'image/webp': 'WebP',
  'image/gif': 'GIF',
  'image/bmp': 'BMP',
  'image/avif': 'AVIF',
  'image/svg+xml': 'SVG',
  'image/x-icon': 'ICO',
  'image/tiff': 'TIFF',
};

function toU8(bytes) {
  if (!bytes) return new Uint8Array(0);
  if (bytes instanceof Uint8Array) return bytes;
  if (bytes instanceof ArrayBuffer) return new Uint8Array(bytes);
  if (ArrayBuffer.isView(bytes)) return new Uint8Array(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  if (Array.isArray(bytes)) return Uint8Array.from(bytes);
  if (typeof bytes === 'object' && bytes.data) return toU8(bytes.data);
  return new Uint8Array(0);
}

function ascii(b, start, end) {
  let s = '';
  for (let i = start; i < end && i < b.length; i += 1) s += String.fromCharCode(b[i]);
  return s;
}

function extOf(path) {
  const name = String(path || '');
  const i = name.lastIndexOf('.');
  return i >= 0 ? name.slice(i + 1).toLowerCase() : '';
}

function baseName(path) {
  const name = String(path || '');
  const i = Math.max(name.lastIndexOf('/'), name.lastIndexOf('\\'));
  return i >= 0 ? name.slice(i + 1) : name;
}

function dirName(path) {
  const name = String(path || '');
  const i = Math.max(name.lastIndexOf('/'), name.lastIndexOf('\\'));
  return i > 0 ? name.slice(0, i) : '';
}

function stripExt(name) {
  return String(name || '').replace(/\.[^./\\]+$/, '');
}

/**
 * Detect the image MIME type from magic bytes. Returns
 * 'application/octet-stream' when nothing matches.
 */
export function sniffMime(bytes) {
  const b = toU8(bytes);
  if (!b.length) return 'application/octet-stream';
  if (b.length >= 3 && b[0] === 0xff && b[1] === 0xd8 && b[2] === 0xff) return 'image/jpeg';
  if (b.length >= 8 && b[0] === 0x89 && b[1] === 0x50 && b[2] === 0x4e && b[3] === 0x47 &&
      b[4] === 0x0d && b[5] === 0x0a && b[6] === 0x1a && b[7] === 0x0a) return 'image/png';
  if (b.length >= 6 && ascii(b, 0, 3) === 'GIF') return 'image/gif';
  if (b.length >= 12 && ascii(b, 0, 4) === 'RIFF' && ascii(b, 8, 12) === 'WEBP') return 'image/webp';
  if (b.length >= 2 && b[0] === 0x42 && b[1] === 0x4d) return 'image/bmp';
  if (b.length >= 4) {
    const t = ascii(b, 0, 4);
    if (t === 'II*\u0000' || t === 'MM\u0000*') return 'image/tiff';
  }
  if (b.length >= 4 && b[0] === 0x00 && b[1] === 0x00 && b[2] === 0x01 && b[3] === 0x00) return 'image/x-icon';
  if (b.length >= 12 && ascii(b, 4, 8) === 'ftyp') {
    const brand = ascii(b, 8, 12);
    if (brand === 'avif' || brand === 'avis') return 'image/avif';
    if (brand === 'heic' || brand === 'heix' || brand === 'heif' || brand === 'mif1') return 'image/heif';
  }
  // SVG is text: look for a root <svg> in the first 2 KB (skip BOM/XML prolog).
  const head = new TextDecoder('utf-8', { fatal: false }).decode(b.subarray(0, 2048)).replace(/^\uFEFF/, '');
  if (/^\s*(<\?xml|<!--|<!doctype\s+svg|<svg[\s>])/i.test(head) && /<svg[\s>]/i.test(head)) {
    return 'image/svg+xml';
  }
  return 'application/octet-stream';
}

/** Read raw file bytes through the Tauri bridge. */
export async function readFileBytes(path) {
  if (!path) throw new Error('沒有指定圖片路徑');
  let raw;
  try {
    raw = await bridge.invoke('read_image_file', { path });
  } catch (err) {
    const msg = typeof err === 'string' ? err : err?.message;
    throw new Error(`讀取檔案失敗：${baseName(path)}${msg ? `（${msg}）` : ''}`);
  }
  const bytes = toU8(raw);
  if (!bytes.length) throw new Error(`檔案是空的：${baseName(path)}`);
  return bytes;
}

// ---------------------------------------------------------------------------
// Decoding
// ---------------------------------------------------------------------------

async function decodeViaImageElement(blob) {
  const url = URL.createObjectURL(blob);
  try {
    const img = new Image();
    img.decoding = 'async';
    img.src = url;
    if (typeof img.decode === 'function') await img.decode();
    else {
      await new Promise((resolve, reject) => {
        img.onload = resolve;
        img.onerror = () => reject(new Error('圖片解碼失敗'));
      });
    }
    const w = img.naturalWidth;
    const h = img.naturalHeight;
    if (!w || !h) throw new Error('圖片解碼失敗');
    const canvas = document.createElement('canvas');
    canvas.width = w;
    canvas.height = h;
    canvas.getContext('2d').drawImage(img, 0, 0);
    return await createImageBitmap(canvas);
  } finally {
    URL.revokeObjectURL(url);
  }
}

/**
 * Decode a blob to an ImageBitmap. Returns `{ bitmap, decodedScale }` where
 * decodedScale = bitmap pixels / logical pixels (1 unless downsampled).
 */
async function decodeBlob(blob, mime) {
  const isSvg = mime === 'image/svg+xml';
  // SVG 沒有 EXIF 方向，走 <img> + drawImage 最可靠（WebKit 的
  // createImageBitmap 對 SVG blob 支援不一致）。
  if (isSvg) {
    const bitmap = await decodeViaImageElement(blob);
    return { bitmap, decodedScale: 1 };
  }
  let bitmap;
  try {
    // imageOrientation:'from-image' applies the EXIF rotation so width/height
    // are the displayed dimensions, not the sensor dimensions.
    bitmap = await createImageBitmap(blob, { imageOrientation: 'from-image' });
  } catch (err) {
    // Older WebKit builds reject the options bag; <img> decoding still applies
    // EXIF orientation by itself.
    bitmap = await decodeViaImageElement(blob);
  }
  if (!bitmap || !bitmap.width || !bitmap.height) {
    throw new Error('圖片解碼失敗');
  }

  const fullW = bitmap.width;
  const fullH = bitmap.height;
  const area = fullW * fullH;

  if (area > MAX_PIXELS) {
    bitmap.close?.();
    const ratio = Math.sqrt(MAX_PIXELS / area);
    const resizeWidth = Math.max(1, Math.round(fullW * ratio));
    try {
      const small = await createImageBitmap(blob, {
        imageOrientation: 'from-image',
        resizeWidth,
        resizeQuality: 'high',
      });
      // Area ratio is rotation-invariant, so it works even when EXIF swapped
      // width and height.
      const decodedScale = Math.sqrt((small.width * small.height) / area);
      return { bitmap: small, decodedScale };
    } catch (err) {
      // Could not downsample; fall back to the full decode.
      bitmap = await createImageBitmap(blob, { imageOrientation: 'from-image' });
    }
  }
  return { bitmap, decodedScale: 1 };
}

function describeDecodeError(name, mime) {
  if (mime === 'image/tiff') return `此 WebView 不支援 TIFF 解碼：「${name}」。請先轉存為 JPEG 或 PNG。`;
  if (mime === 'image/heif' || mime === 'image/heic') return `此 WebView 不支援 HEIC 解碼：「${name}」。`;
  if (mime === 'image/avif') return `此 WebView 不支援 AVIF 解碼：「${name}」。請先轉存為 JPEG 或 PNG。`;
  return `無法解碼「${name}」，檔案可能已損毀或格式不受支援。`;
}

async function buildDoc({ bytes, name, path, meta }) {
  const sniffed = sniffMime(bytes);
  // 副檔名只在 magic bytes 認不出來時當後備（例如被裁掉開頭的檔案）。
  const extMime = MIME_BY_EXT[extOf(path || name)];
  const mime = sniffed !== 'application/octet-stream' ? sniffed : (extMime || sniffed);
  let resolvedMeta = meta || null;
  const metaPromise =
    !resolvedMeta && path
      ? bridge.invoke('inspect_image', { path }).catch(() => null)
      : Promise.resolve(resolvedMeta);
  if (!mime.startsWith('image/') && mime !== 'application/octet-stream') {
    throw new Error(`不支援的檔案格式：「${name}」`);
  }
  if (mime === 'application/octet-stream') {
    throw new Error(`無法辨識圖片格式：「${name}」。支援 JPEG、PNG、WebP、GIF、BMP、AVIF、SVG、TIFF。`);
  }

  const blob = new Blob([bytes], { type: mime });
  // Start decoding while the shell is still reading EXIF/metadata.
  const decodePromise = decodeBlob(blob, mime).catch((err) => {
    throw new Error(describeDecodeError(name, mime));
  });
  let decoded;
  try {
    [decoded, resolvedMeta] = await Promise.all([decodePromise, metaPromise]);
  } catch (err) {
    throw new Error(describeDecodeError(name, mime));
  }
  resolvedMeta = resolvedMeta || null;
  const { bitmap, decodedScale } = decoded;
  const width = Math.max(1, Math.round(bitmap.width / (decodedScale || 1)));
  const height = Math.max(1, Math.round(bitmap.height / (decodedScale || 1)));
  const objectUrl = URL.createObjectURL(blob);
  const exif = Array.isArray(resolvedMeta?.exif) ? resolvedMeta.exif : [];

  return {
    path: path || '',
    name: resolvedMeta?.name || name || '未命名',
    dir: resolvedMeta?.dir || dirName(path || ''),
    size: Number(resolvedMeta?.size) || bytes.length,
    mtime: Number(resolvedMeta?.mtime) || Math.floor(Date.now() / 1000),
    width,
    height,
    format: resolvedMeta?.format || FORMAT_BY_MIME[mime] || mime.replace('image/', '').toUpperCase(),
    exif,
    bitmap,
    blob,
    objectUrl,
    mime,
    decodedScale: decodedScale || 1,
    hasAlpha: ALPHA_MIMES.has(mime),
  };
}

let currentDoc = null;

/** Load an image from disk (Tauri) and decode it. */
export async function loadDocument(path) {
  if (!path) throw new Error('沒有指定圖片路徑');
  const bytes = await readFileBytes(path);
  const name = baseName(path);
  const doc = await buildDoc({ bytes, name, path });
  if (currentDoc && currentDoc !== doc) disposeDoc(currentDoc);
  currentDoc = doc;
  return doc;
}

/** Load an image from memory (drag & drop / clipboard). */
export async function loadFromBytes(bytes, name, path) {
  const data = toU8(bytes);
  if (!data.length) throw new Error('沒有可用的圖片資料');
  const safeName = name || baseName(path) || '未命名';
  const doc = await buildDoc({ bytes: data, name: safeName, path: path || '' });
  if (currentDoc && currentDoc !== doc) disposeDoc(currentDoc);
  currentDoc = doc;
  return doc;
}

/** Release the bitmap + object URL held by a document. Safe to call twice. */
export function disposeDoc(doc) {
  if (!doc) return;
  try {
    doc.bitmap?.close?.();
  } catch (err) {
    /* 已釋放 */
  }
  try {
    if (doc.objectUrl) URL.revokeObjectURL(doc.objectUrl);
  } catch (err) {
    /* 已釋放 */
  }
  doc.bitmap = null;
  doc.objectUrl = '';
  if (currentDoc === doc) currentDoc = null;
}

export { stripExt };
