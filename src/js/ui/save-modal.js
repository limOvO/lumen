// 另存新檔對話框：路徑＋檔名、格式、品質、目標大小、覆寫提示。
// main.js 以 open(defaults) 取得 { path, format, quality, maxDim, targetKB } 或 null。

import { $, debounce, dirname, extOf, fmtBytes, joinPath, on, stripExt } from './dom.js';
import { createSelect } from './select.js';

const FORMAT_EXT = { jpeg: 'jpg', jpg: 'jpg', png: 'png', webp: 'webp' };

let moduleCtx = null;
let resolveChoice = null;
let isOpen = false;
let lastFocus = null;
let baseOptions = {};
let formatSelect = null;

function extFor(format) {
  return FORMAT_EXT[format] || 'png';
}

function ensureExt(name, format) {
  const base = stripExt(name || '').trim() || '未命名';
  return `${base}.${extFor(format)}`;
}

function splitPath(path, fallbackDir) {
  const value = String(path || '');
  const index = Math.max(value.lastIndexOf('/'), value.lastIndexOf('\\'));
  if (index < 0) return { dir: fallbackDir || '', name: value };
  const dir = index === 0 ? value.slice(0, 1) : value.slice(0, index);
  return { dir, name: value.slice(index + 1) };
}

function normalizeFormat(format, doc) {
  if (format && format !== 'auto') return FORMAT_EXT[format] ? format : 'png';
  const mime = doc?.mime || '';
  if (mime.includes('png')) return 'png';
  if (mime.includes('webp')) return 'webp';
  if (mime.includes('jpeg')) return 'jpeg';
  const ext = extOf(doc?.name || '');
  if (ext === 'png') return 'png';
  if (ext === 'webp') return 'webp';
  if (ext === 'jpg' || ext === 'jpeg') return 'jpeg';
  return 'png';
}

function currentPath() {
  const dir = ($('save-dir')?.value || '').trim();
  const name = ($('save-name')?.value || '').trim();
  if (!name) return '';
  const format = formatSelect?.value || 'png';
  return joinPath(dir, ensureExt(name, format));
}

let estimateToken = 0;

async function runEstimate() {
  const meta = $('save-meta');
  if (!meta || !isOpen) return;
  const token = ++estimateToken;

  const target = Number($('save-target')?.value);
  if (Number.isFinite(target) && target > 0) {
    meta.textContent = `依目標 ${Math.round(target)} KB 自動調整`;
    return;
  }

  const doc = moduleCtx?.state?.doc;
  if (!doc || typeof moduleCtx.actions?.estimateSize !== 'function') {
    meta.textContent = '—';
    return;
  }

  const format = formatSelect?.value || 'png';
  const quality = Math.min(1, Math.max(0.01, Number($('save-quality')?.value || 85) / 100));
  meta.textContent = '估算中…';

  let result = null;
  try {
    result = await moduleCtx.actions.estimateSize({
      format,
      quality,
      targetKB: null,
      maxDim: baseOptions.maxDim ?? moduleCtx.state.output.maxDim ?? null,
    });
  } catch {
    result = null;
  }
  if (token !== estimateToken || !isOpen) return;
  if (result && result.bytes != null) {
    meta.textContent = `${fmtBytes(result.bytes)} · ${result.width} × ${result.height}`;
    meta.title = `${result.bytes.toLocaleString('zh-TW')} bytes`;
  } else {
    meta.textContent = '—';
    meta.title = '';
  }
}

const scheduleEstimate = debounce(() => {
  void runEstimate();
}, 400);

function updateMeta() {
  const meta = $('save-meta');
  const bytes = Number(baseOptions.bytes);
  if (meta && Number.isFinite(bytes) && bytes > 0) meta.textContent = fmtBytes(bytes);
  scheduleEstimate();
}

function syncQualityState() {
  const quality = $('save-quality');
  const out = $('save-quality-out');
  const target = $('save-target');
  if (quality && out) out.textContent = String(Math.round(Number(quality.value)));
  const format = formatSelect?.value || 'png';
  const hasTarget = Number(target?.value) > 0;
  if (quality) quality.disabled = format === 'png' || hasTarget;
  updateMeta();
}

const checkExists = debounce(async () => {
  if (!isOpen) return;
  const warn = $('save-warn');
  const confirm = $('save-confirm');
  if (!warn || !confirm) return;
  const path = currentPath();
  if (!path) {
    warn.hidden = true;
    confirm.textContent = '儲存';
    return;
  }
  let exists = false;
  try {
    exists = !!(await moduleCtx.bridge.invoke('path_exists', { path }));
  } catch {
    exists = false;
  }
  if (!isOpen) return;
  warn.hidden = !exists;
  confirm.textContent = exists ? '覆寫並儲存' : '儲存';
}, 260);

function finish(result) {
  const modal = $('save-modal');
  if (modal) modal.hidden = true;
  document.body.classList.remove('modal-open');
  isOpen = false;
  scheduleEstimate.cancel?.();
  checkExists.cancel?.();
  const resolve = resolveChoice;
  resolveChoice = null;
  if (lastFocus && typeof lastFocus.focus === 'function') {
    lastFocus.focus({ preventScroll: true });
  }
  lastFocus = null;
  if (resolve) resolve(result);
}

function cancel() {
  finish(null);
}

function confirm() {
  const nameInput = $('save-name');
  const dirInput = $('save-dir');
  const name = (nameInput?.value || '').trim();
  if (!name) {
    moduleCtx.actions.toast('請輸入檔名', 'warn');
    nameInput?.focus({ preventScroll: true });
    return;
  }
  const format = formatSelect?.value || 'png';
  const dir = (dirInput?.value || '').trim() || moduleCtx.state.doc?.dir || '';
  const path = joinPath(dir, ensureExt(name, format));
  const quality = Math.min(1, Math.max(0.01, Number($('save-quality')?.value || 85) / 100));
  const targetRaw = Number($('save-target')?.value);
  const targetKB = Number.isFinite(targetRaw) && targetRaw > 0 ? Math.round(targetRaw) : null;
  const maxDim = baseOptions.maxDim ?? moduleCtx.state.output.maxDim ?? null;
  finish({ path, format, quality, maxDim, targetKB });
}

export function init(ctx) {
  moduleCtx = ctx;
  const modal = $('save-modal');
  if (!modal) return;

  formatSelect = createSelect($('save-format'), {
    ariaLabel: '輸出格式',
    options: [
      { value: 'jpeg', label: 'JPEG' },
      { value: 'png', label: 'PNG' },
      { value: 'webp', label: 'WebP' },
    ],
    value: 'jpeg',
    onChange: (format) => {
      const nameInput = $('save-name');
      if (nameInput && nameInput.value.trim()) {
        nameInput.value = ensureExt(nameInput.value, format);
      }
      syncQualityState();
      checkExists();
    },
  });

  for (const el of modal.querySelectorAll('[data-close]')) {
    el.addEventListener('click', cancel);
  }
  on($('save-cancel'), 'click', cancel);
  on($('save-confirm'), 'click', confirm);

  for (const id of ['save-dir', 'save-name']) {
    const input = $(id);
    on(input, 'input', () => {
      updateMeta();
      checkExists();
    });
  }

  on($('save-target'), 'input', () => {
    syncQualityState();
    checkExists();
  });
  on($('save-quality'), 'input', () => syncQualityState());

  on($('save-name'), 'keydown', (event) => {
    if (event.key === 'Enter') {
      event.preventDefault();
      confirm();
    }
  });

  on(modal, 'keydown', (event) => {
    if (event.key === 'Escape') {
      event.preventDefault();
      event.stopPropagation();
      cancel();
      return;
    }
    if (event.key === 'Enter' && event.target instanceof HTMLElement) {
      if (event.target.tagName === 'INPUT') {
        event.preventDefault();
        confirm();
      }
    }
  });

  document.addEventListener('keydown', (event) => {
    if (!isOpen) return;
    if (event.key === 'Escape') {
      event.preventDefault();
      cancel();
    }
  });

  window.__lumenUI = Object.assign(window.__lumenUI || {}, {
    openSaveModal: openSaveDialog,
    openSaveDialog,
  });
}

/**
 * 開啟另存對話框，回傳 Promise<choice|null>。
 * @param {{path?:string, format?:string, quality?:number, maxDim?:number|null, targetKB?:number|null, bytes?:number, width?:number, height?:number}} defaults
 */
export async function openSaveDialog(defaults = {}) {
  if (!moduleCtx) return null;
  if (isOpen) cancel();

  baseOptions = { ...defaults };
  const doc = moduleCtx.state.doc;
  const format = normalizeFormat(defaults.format || moduleCtx.state.output.format, doc);
  let dir = defaults.dir || doc?.dir || '';
  let name = defaults.name || '';
  if (defaults.path) {
    const parts = splitPath(defaults.path, dir);
    if (parts.dir) dir = parts.dir;
    if (parts.name) name = parts.name;
  }
  if (!name) {
    const base = stripExt(doc?.name || '圖片');
    name = `${base}-edit.${extFor(format)}`;
  }
  // 若上層給的是絕對路徑但沒有目錄，補上文件所在資料夾。
  if (!dirname(joinPath(dir, name)) && doc?.dir) dir = doc.dir;

  const dirInput = $('save-dir');
  const nameInput = $('save-name');
  if (dirInput) dirInput.value = dir;
  if (nameInput) nameInput.value = ensureExt(name, format);
  formatSelect?.setValue(format);

  const quality = $('save-quality');
  const qualityOut = $('save-quality-out');
  const qualityPct = Math.round((defaults.quality ?? moduleCtx.state.output.quality ?? 0.85) * 100);
  if (quality) quality.value = String(qualityPct);
  if (qualityOut) qualityOut.textContent = String(qualityPct);
  const target = $('save-target');
  if (target) target.value = defaults.targetKB ? String(defaults.targetKB) : '';

  const meta = $('save-meta');
  if (meta) {
    meta.textContent = '—';
    meta.title = '';
  }
  estimateToken += 1;

  syncQualityState();

  lastFocus = document.activeElement;
  const modal = $('save-modal');
  if (modal) modal.hidden = false;
  isOpen = true;
  document.body.classList.add('modal-open');

  if (nameInput) {
    nameInput.focus({ preventScroll: true });
    const dot = nameInput.value.lastIndexOf('.');
    if (dot > 0) nameInput.setSelectionRange(0, dot);
    else nameInput.select();
  }

  checkExists();

  return new Promise((resolve) => {
    resolveChoice = resolve;
  });
}

// CONTRACT §14 也允許以 open(defaults) 呼叫。
export const open = openSaveDialog;
