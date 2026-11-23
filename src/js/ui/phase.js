// 預覽／編輯雙階段 UI：預覽 chrome、張數/檔名、換頁箭頭、資訊浮卡狀態與提示。
// 階段切換本身由 main.js 負責（body.phase-preview / body.phase-edit）。

import { subscribe } from '../core/state.js';
import { $, on } from './dom.js';

let moduleCtx = null;
let observer = null;

function hasPreviewChrome() {
  return (
    document.body.classList.contains('phase-preview') &&
    !document.body.classList.contains('no-doc')
  );
}

function syncPeek() {
  const pressed = document.body.classList.contains('peek-info');
  const button = $('preview-info');
  if (button) {
    button.setAttribute('aria-pressed', pressed ? 'true' : 'false');
    button.title = pressed ? '關閉資訊浮卡（I）' : '顯示資訊浮卡（I）';
  }
  if (pressed) {
    // 若屬性面板先前被收合，開浮卡前先展開，否則浮卡會是 inert 的空殼。
    const inspector = $('inspector');
    if (inspector && inspector.classList.contains('is-collapsed')) {
      $('btn-insp-collapse')?.click();
    }
    // 浮卡只給資訊用，確保切到資訊分頁（分頁列在預覽模式隱藏）。
    const infoTab = $('tab-info');
    if (infoTab && !infoTab.classList.contains('is-active')) infoTab.click();
  }
}

function render() {
  if (!moduleCtx) return;
  const state = moduleCtx.state;
  const doc = state.doc || null;
  const files = Array.isArray(state.files) ? state.files : [];

  const shouldNoDoc = !doc;
  if (document.body.classList.contains('no-doc') !== shouldNoDoc) {
    document.body.classList.toggle('no-doc', shouldNoDoc);
  }

  const chrome = $('preview-chrome');
  if (chrome) chrome.setAttribute('aria-hidden', doc ? 'false' : 'true');

  const caption = $('preview-caption');
  if (caption) {
    if (doc) {
      caption.textContent = `${doc.name} · ${doc.width}×${doc.height}`;
      caption.title = `${doc.name}${doc.path ? `\n${doc.path}` : ''}`;
    } else {
      caption.textContent = '—';
      caption.title = '';
    }
  }

  const counter = $('preview-counter');
  if (counter) {
    if (doc) {
      const index = Number.isInteger(state.index) && state.index >= 0 ? state.index + 1 : 1;
      counter.textContent = `${index} / ${Math.max(files.length, 1)}`;
    } else {
      counter.textContent = '— / —';
    }
  }

  const previewButton = $('btn-preview');
  if (previewButton) previewButton.disabled = !doc;

  const canSwitch = doc && files.length > 1;
  for (const id of ['preview-nav-prev', 'preview-nav-next']) {
    const button = $(id);
    if (button) button.disabled = !canSwitch;
  }

  syncPeek();
}

export function init(ctx) {
  moduleCtx = ctx;

  on($('preview-edit'), 'click', () => ctx.actions.setPhase?.('edit'));
  on($('btn-preview'), 'click', () => ctx.actions.setPhase?.('preview'));
  on($('preview-nav-prev'), 'click', () => ctx.actions.prev?.());
  on($('preview-nav-next'), 'click', () => ctx.actions.next?.());

  // 預覽中的按鈕用 Enter/Space 操作時，不要讓全域快捷鍵攔走按鈕的預設啟用行為。
  for (const id of ['preview-info', 'preview-edit', 'preview-nav-prev', 'preview-nav-next']) {
    on($(id), 'keydown', (event) => {
      if (event.key === 'Enter' || event.key === ' ') event.stopPropagation();
    });
  }

  on($('preview-info'), 'click', () => {
    const next = !document.body.classList.contains('peek-info');
    ctx.actions.peekInfo?.(next);
    // main 會同步 body class；若尚未更新，下一個 microtask 再對一次。
    queueMicrotask(syncPeek);
  });

  // 點擊資訊浮卡以外的任何地方就收起（點 dock／資訊鈕本身除外）。
  on(document, 'pointerdown', (event) => {
    if (!document.body.classList.contains('peek-info')) return;
    const target = event.target;
    if (!(target instanceof Element)) return;
    if (target.closest('#inspector')) return;
    if (target.closest('#preview-info')) return;
    if (target.closest('.pc-dock')) return;
    ctx.actions.peekInfo?.(false);
  });

  // 滑鼠靠近左右邊緣才顯示換頁箭頭。
  const area = $('canvas-area');
  on(area, 'mousemove', (event) => {
    const chrome = $('preview-chrome');
    if (!chrome) return;
    if (!hasPreviewChrome()) {
      chrome.classList.remove('nav-visible');
      return;
    }
    const rect = area.getBoundingClientRect();
    const edge = Math.min(120, Math.max(64, rect.width * 0.12));
    const nearEdge = event.clientX - rect.left < edge || rect.right - event.clientX < edge;
    chrome.classList.toggle('nav-visible', nearEdge);
  });

  on(area, 'mouseleave', () => {
    $('preview-chrome')?.classList.remove('nav-visible');
  });

  subscribe((scope) => {
    if (
      scope === 'all' ||
      scope === 'phase' ||
      scope === 'doc' ||
      scope === 'files' ||
      scope === 'view'
    ) {
      render();
    }
  });

  // main 直接切 body class 時也同步（含 peek-info）。
  if (window.MutationObserver) {
    observer = new window.MutationObserver(() => {
      syncPeek();
      render();
    });
    observer.observe(document.body, { attributes: true, attributeFilter: ['class'] });
  }

  render();
}
