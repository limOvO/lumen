// 資訊分頁：檔名、尺寸、格式、大小、時間、比例、色彩、EXIF、複製路徑。

import { subscribe } from '../core/state.js';
import { $, fmtBytes, fmtDate, on, ratioLabel, rafThrottle } from './dom.js';

async function copyText(text) {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    const area = document.createElement('textarea');
    area.value = text;
    area.setAttribute('readonly', '');
    area.style.position = 'fixed';
    area.style.top = '-1000px';
    area.style.opacity = '0';
    document.body.append(area);
    area.select();
    let ok = false;
    try {
      ok = document.execCommand('copy');
    } catch {
      ok = false;
    }
    area.remove();
    return ok;
  }
}

function findColor(doc) {
  const direct = doc?.color || doc?.colorSpace || doc?.colorspace || doc?.profile;
  if (direct) return String(direct);
  const exif = Array.isArray(doc?.exif) ? doc.exif : [];
  const hit = exif.find((entry) => /色|color|icc/i.test(`${entry.key || ''} ${entry.label || ''}`));
  return hit?.value || null;
}

export function initInfo(ctx) {
  const { state, actions } = ctx;

  const els = {
    empty: $('info-empty'),
    content: $('info-content'),
    name: $('info-name'),
    dims: $('info-dims'),
    format: $('info-format'),
    size: $('info-size'),
    mtime: $('info-mtime'),
    ratio: $('info-ratio'),
    colorRow: $('info-color-row'),
    color: $('info-color'),
    exif: $('info-exif'),
    exifCount: $('info-exif-count'),
    copy: $('info-copy-path'),
  };

  on(els.copy, 'click', async () => {
    const path = state.doc?.path;
    if (!path) return;
    const ok = await copyText(path);
    actions.toast(ok ? '已複製檔案路徑' : '無法寫入剪貼簿', ok ? 'ok' : 'warn');
  });

  function renderExif(doc) {
    if (!els.exif) return;
    const entries = Array.isArray(doc?.exif) ? doc.exif : [];
    if (els.exifCount) els.exifCount.textContent = `${entries.length} 項`;
    els.exif.textContent = '';
    if (!entries.length) {
      const empty = document.createElement('p');
      empty.className = 'exif-empty';
      empty.textContent = '儲存後的檔案已移除 EXIF／GPS（隱私保護）';
      els.exif.append(empty);
      return;
    }
    const fragment = document.createDocumentFragment();
    for (const entry of entries) {
      if (!entry) continue;
      const value = entry.value == null || entry.value === '' ? '—' : String(entry.value);
      const row = document.createElement('div');
      row.className = 'exif-row';
      const key = document.createElement('span');
      key.className = 'exif-k';
      key.textContent = entry.label || entry.key || '—';
      key.title = entry.key || '';
      const val = document.createElement('span');
      val.className = 'exif-v';
      val.textContent = value;
      val.title = value;
      row.append(key, val);
      fragment.append(row);
    }
    els.exif.append(fragment);
  }

  const render = rafThrottle(() => {
    const doc = state.doc;
    if (els.empty) els.empty.hidden = !!doc;
    if (els.content) els.content.hidden = !doc;
    if (els.copy) els.copy.disabled = !doc;
    if (!doc) return;

    if (els.name) els.name.textContent = doc.name || '—';
    if (els.dims) els.dims.textContent = `${doc.width} × ${doc.height} px`;
    if (els.format) {
      const format = doc.format || (doc.name || '').split('.').pop()?.toUpperCase();
      els.format.textContent = format || '—';
    }
    if (els.size) els.size.textContent = fmtBytes(doc.size);
    if (els.mtime) els.mtime.textContent = fmtDate(doc.mtime);
    if (els.ratio) els.ratio.textContent = ratioLabel(doc.width, doc.height);

    const color = findColor(doc);
    if (els.colorRow) els.colorRow.hidden = !color;
    if (els.color && color) els.color.textContent = color;

    renderExif(doc);
  });

  subscribe((scope) => {
    if (scope === 'all' || scope === 'doc') render();
  });

  render();
}
