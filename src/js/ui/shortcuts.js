// 快捷鍵說明面板：按 ? 或點頂欄「?」開啟；Esc／點空白／✕ 關閉。
// 全域 '?' 由 main.js 攔截後呼叫 toggle()，這裡不重複攔截。

import { $, on } from './dom.js';

let modal = null;
let trigger = null;
let opened = false;
let lastFocus = null;
let closeTimer = 0;

function body() {
  return modal ? modal.querySelector('.shortcuts-body') : null;
}

export function open() {
  if (!modal || opened) return;
  if (closeTimer) {
    clearTimeout(closeTimer);
    closeTimer = 0;
  }
  modal.classList.remove('is-closing');
  lastFocus = document.activeElement;
  modal.hidden = false;
  opened = true;
  document.body.classList.add('modal-open');
  const scroller = body();
  if (scroller) scroller.scrollTop = 0;
  const closeButton = modal.querySelector('.modal-close');
  if (closeButton) closeButton.focus({ preventScroll: true });
}

export function close() {
  if (!modal || !opened) return;
  opened = false;
  document.body.classList.remove('modal-open');
  if (closeTimer) clearTimeout(closeTimer);
  modal.classList.add('is-closing');
  closeTimer = window.setTimeout(() => {
    closeTimer = 0;
    modal.hidden = true;
    modal.classList.remove('is-closing');
  }, 160);
  if (lastFocus && typeof lastFocus.focus === 'function') {
    lastFocus.focus({ preventScroll: true });
  }
  lastFocus = null;
}

export function toggle() {
  if (opened) close();
  else open();
}

export function isOpen() {
  return opened;
}

export function init(ctx) {
  modal = $('shortcuts-modal');
  trigger = $('btn-shortcuts');
  if (!modal) return;

  on(trigger, 'click', toggle);

  for (const el of modal.querySelectorAll('[data-close]')) {
    el.addEventListener('click', close);
  }

  document.addEventListener('keydown', (event) => {
    if (!opened) return;
    if (event.key === 'Escape') {
      event.preventDefault();
      event.stopPropagation();
      close();
    }
  });

  window.__lumenUI = Object.assign(window.__lumenUI || {}, {
    openShortcuts: open,
    toggleShortcuts: toggle,
  });
}
