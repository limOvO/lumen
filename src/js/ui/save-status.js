// 存檔狀態：idle（無變更）/ saving（儲存中）/ saved（已儲存）/ error（儲存失敗）。
// main.js 透過 ui.saveStatus.set(status) 驅動；saved 兩秒後淡回 idle。

import { subscribe } from '../core/state.js';
import { $ } from './dom.js';

const TEXT = {
  idle: '無變更',
  saving: '儲存中…',
  saved: '已儲存',
  error: '儲存失敗',
};

let timer = 0;
let current = 'idle';
let lastDocPath = null;
let reduceMotion = false;

function set(status, { silent = false } = {}) {
  const next = Object.prototype.hasOwnProperty.call(TEXT, status) ? status : 'idle';
  current = next;
  const el = $('save-status');
  const label = $('save-status-text');
  if (timer) {
    clearTimeout(timer);
    timer = 0;
  }
  if (label) label.textContent = TEXT[next];
  if (!el) return;
  el.dataset.s = next;
  el.title = `儲存狀態：${TEXT[next]}`;

  if (!silent && !reduceMotion && typeof el.animate === 'function') {
    el.animate(
      [{ opacity: 0.45 }, { opacity: 1 }],
      { duration: 180, easing: 'ease-out' }
    );
  }

  // 「已儲存」只停留兩秒，之後回到無變更。
  if (next === 'saved') {
    timer = window.setTimeout(() => {
      timer = 0;
      if (current === 'saved') set('idle', { silent: true });
    }, 2000);
  }
}

export function init(ctx) {
  reduceMotion = !!(
    window.matchMedia && window.matchMedia('(prefers-reduced-motion: reduce)').matches
  );
  lastDocPath = ctx.state.doc?.path || null;
  set('idle', { silent: true });

  subscribe((scope) => {
    if (scope !== 'doc' && scope !== 'dirty') return;
    const path = ctx.state.doc?.path || null;

    if (!ctx.state.doc) {
      lastDocPath = null;
      if (current !== 'idle') set('idle', { silent: true });
      return;
    }

    if (scope === 'doc' && path !== lastDocPath) {
      lastDocPath = path;
      // 換檔（含存檔後重新載入）時先回無變更；main 接著會依結果設 saved/error。
      if (current !== 'saving') set('idle', { silent: true });
      return;
    }

    // 使用者又開始編輯：已儲存狀態不再成立。
    if (scope === 'dirty' && ctx.state.dirty && current === 'saved') {
      set('idle', { silent: true });
    }
  });
}

export { set };
