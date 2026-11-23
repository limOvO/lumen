// 共用工具：DOM 查詢、格式化、路徑處理。純函式，不碰應用狀態。

export const $ = (id) => document.getElementById(id);

export const $$ = (selector, root = document) => Array.from(root.querySelectorAll(selector));

export function on(el, type, handler, options) {
  if (!el) return () => {};
  el.addEventListener(type, handler, options);
  return () => el.removeEventListener(type, handler, options);
}

export function clamp(value, min, max) {
  return Math.min(max, Math.max(min, value));
}

export function fmtBytes(bytes) {
  if (bytes == null || !Number.isFinite(bytes)) return '—';
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / 1024 / 1024).toFixed(2)} MB`;
  return `${(bytes / 1024 / 1024 / 1024).toFixed(2)} GB`;
}

export function fmtDate(mtime) {
  if (!mtime) return '—';
  const ms = mtime < 1e12 ? mtime * 1000 : mtime;
  const date = new Date(ms);
  if (Number.isNaN(date.getTime())) return '—';
  try {
    return new Intl.DateTimeFormat('zh-TW', {
      year: 'numeric',
      month: '2-digit',
      day: '2-digit',
      hour: '2-digit',
      minute: '2-digit',
      hour12: false,
    }).format(date);
  } catch {
    return date.toISOString();
  }
}

export function gcd(a, b) {
  let x = Math.abs(Math.round(a));
  let y = Math.abs(Math.round(b));
  while (y) {
    const t = y;
    y = x % y;
    x = t;
  }
  return x || 1;
}

export function ratioLabel(width, height) {
  if (!width || !height) return '—';
  const d = gcd(width, height);
  let w = width / d;
  let h = height / d;
  // 大型照片的公因數常常是 1，改用常見比例近似比較好讀。
  if (w > 40 || h > 40) {
    const r = width / height;
    const common = [
      [1, 1],
      [4, 3],
      [3, 2],
      [16, 9],
      [5, 4],
      [3, 4],
      [2, 3],
      [9, 16],
    ];
    for (const [cw, ch] of common) {
      if (Math.abs(r - cw / ch) < 0.01) return `${cw}:${ch}`;
    }
    return `${r.toFixed(2)}:1`;
  }
  return `${w}:${h}`;
}

export function ratioValue(label, width, height) {
  if (!label || label === 'free') return null;
  if (label === 'orig') return width && height ? width / height : null;
  const [a, b] = label.split(':').map(Number);
  return a && b ? a / b : null;
}

export function debounce(fn, ms = 250) {
  let timer = 0;
  const wrapped = (...args) => {
    clearTimeout(timer);
    timer = window.setTimeout(() => fn(...args), ms);
  };
  wrapped.cancel = () => clearTimeout(timer);
  return wrapped;
}

export function rafThrottle(fn) {
  let queued = false;
  let lastArgs = [];
  return (...args) => {
    lastArgs = args;
    if (queued) return;
    queued = true;
    requestAnimationFrame(() => {
      queued = false;
      fn(...lastArgs);
    });
  };
}

export function setPressed(el, pressed) {
  if (!el) return;
  el.setAttribute('aria-pressed', pressed ? 'true' : 'false');
}

export function basename(path) {
  if (!path) return '';
  const clean = String(path).replace(/[\\/]+$/, '');
  const i = Math.max(clean.lastIndexOf('/'), clean.lastIndexOf('\\'));
  return i < 0 ? clean : clean.slice(i + 1);
}

export function dirname(path) {
  if (!path) return '';
  const value = String(path);
  const trimmed = value.replace(/[\\/]+$/, '');
  if (!trimmed) return value.slice(0, 1) || '/';
  const i = Math.max(trimmed.lastIndexOf('/'), trimmed.lastIndexOf('\\'));
  if (i < 0) return '';
  if (i === 0) return trimmed.slice(0, 1);
  const dir = trimmed.slice(0, i);
  if (/^[A-Za-z]:$/.test(dir)) return `${dir}\\`;
  return dir;
}

export function joinPath(dir, name) {
  if (!dir) return name;
  if (!name) return dir;
  const sep = dir.includes('\\') && !dir.includes('/') ? '\\' : '/';
  const base = dir.replace(/[\\/]+$/, '');
  if (!base) return `${sep}${name}`;
  return `${base}${sep}${name}`;
}

export function extOf(name) {
  const m = /\.([A-Za-z0-9]+)$/.exec(name || '');
  return m ? m[1].toLowerCase() : '';
}

export function stripExt(name) {
  return String(name || '').replace(/\.[A-Za-z0-9]+$/, '');
}

export function naturalCompare(a, b) {
  return String(a).localeCompare(String(b), 'zh-Hant', {
    numeric: true,
    sensitivity: 'base',
  });
}

/** 由 svg 標記字串建立節點（只用於本模組內的固定圖示）。 */
export function iconNode(markup) {
  const template = document.createElement('template');
  template.innerHTML = markup.trim();
  return template.content.firstElementChild;
}

export const ICONS = {
  info: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><circle cx="12" cy="12" r="8.5"></circle><path d="M12 11v5.5M12 7.8h.01"></path></svg>',
  ok: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.1" stroke-linecap="round" stroke-linejoin="round"><path d="m5 12.5 4.5 4.4L19 7"></path></svg>',
  warn: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 4.5 21 19.5H3z"></path><path d="M12 9.5v4.2M12 17h.01"></path></svg>',
  err: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><circle cx="12" cy="12" r="8.5"></circle><path d="m9 9 6 6M15 9l-6 6"></path></svg>',
  close: '<svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round"><path d="M6 6l12 12M18 6 6 18"></path></svg>',
  image: '<svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><rect x="3.5" y="5" width="17" height="14" rx="2.5"></rect><circle cx="8.4" cy="9.6" r="1.4"></circle><path d="m4.5 16.5 4.5-4.2 3 2.7 2.8-2.2 4.2 3.9"></path></svg>',
  folder: '<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><path d="M3.5 7.5A1.7 1.7 0 0 1 5.2 5.8h3.6l2 2.1h7.5a1.7 1.7 0 0 1 1.7 1.7v7a1.7 1.7 0 0 1-1.7 1.7H5.2a1.7 1.7 0 0 1-1.7-1.7z"></path></svg>',
};
