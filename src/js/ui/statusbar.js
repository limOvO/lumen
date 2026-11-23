// 底部狀態列：左邊檔案資訊、中間訊息（錯誤/提示）、右邊縮放與張數。

import { subscribe } from '../core/state.js';
import { $, fmtBytes, rafThrottle } from './dom.js';

let messageTimer = 0;

function formatOf(doc) {
  if (!doc) return '';
  if (doc.format) return String(doc.format).toUpperCase();
  const ext = (doc.name || '').split('.').pop();
  return ext ? ext.toUpperCase() : '';
}

export function init(ctx) {
  const info = $('status-info');
  const right = $('status-right');
  const msg = $('status-msg');

  const render = rafThrottle(() => {
    const { state } = ctx;
    const doc = state.doc;
    if (info) {
      if (doc) {
        const parts = [`${doc.width} × ${doc.height}`];
        const format = formatOf(doc);
        if (format) parts.push(format);
        if (doc.size) parts.push(fmtBytes(doc.size));
        info.textContent = parts.join(' · ');
        info.title = doc.path || '';
      } else {
        info.textContent = '—';
        info.title = '';
      }
    }
    if (right) {
      if (doc) {
        const scale = Number(state.view?.scale);
        const pct = Number.isFinite(scale) ? `${Math.round(scale * 100)}%` : '—';
        const files = Array.isArray(state.files) ? state.files : [];
        const index = Number.isInteger(state.index) && state.index >= 0 ? state.index + 1 : 1;
        right.textContent = `${pct} · ${index} / ${Math.max(files.length, 1)}`;
      } else {
        right.textContent = '—';
      }
    }
    if (msg && !msg.textContent) msg.hidden = true;
  });

  subscribe((scope) => {
    if (
      scope === 'all' ||
      scope === 'doc' ||
      scope === 'view' ||
      scope === 'files' ||
      scope === 'output' ||
      scope === 'busy' ||
      scope === 'dirty'
    ) {
      render();
    }
  });

  render();
}

/**
 * 顯示狀態列訊息。kind: info|ok|warn|err
 * 空白字串會立即清空；訊息會在數秒後自動淡出（錯誤留久一點）。
 */
export function message(text, kind = 'info') {
  const el = $('status-msg');
  if (!el) return;
  if (messageTimer) {
    clearTimeout(messageTimer);
    messageTimer = 0;
  }
  const value = text == null ? '' : String(text).trim();
  el.textContent = value;
  el.hidden = !value;
  if (!value) {
    delete el.dataset.kind;
    return;
  }
  if (kind === 'err' || kind === 'error') el.dataset.kind = 'err';
  else if (kind === 'warn' || kind === 'warning') el.dataset.kind = 'warn';
  else if (kind === 'ok' || kind === 'success') el.dataset.kind = 'ok';
  else delete el.dataset.kind;

  const lifetime = el.dataset.kind === 'err' ? 6000 : 4500;
  messageTimer = window.setTimeout(() => {
    messageTimer = 0;
    el.textContent = '';
    el.hidden = true;
    delete el.dataset.kind;
  }, lifetime);
}

export const show = message;

export function clear() {
  message('', 'info');
}
