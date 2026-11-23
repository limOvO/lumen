// 右側屬性面板寬度：可拖拽分隔欄（240–420px）、鍵盤調整、收合/展開與寬度記憶。
// 依知識庫《可拖拽分隔欄》：分隔欄 0 寬，只有把手能拖；寬度直接寫 inline style，不加過渡。

import { $, clamp, on } from './dom.js';

export const INSPECTOR_MIN = 240;
export const INSPECTOR_MAX = 420;
export const INSPECTOR_DEFAULT = 300;
export const INSPECTOR_STEP = 16;
export const INSPECTOR_KEY = 'lumen.inspector-width';

function readSavedWidth() {
  try {
    const raw = window.localStorage.getItem(INSPECTOR_KEY);
    if (raw != null && raw !== '') {
      const value = Number(raw);
      if (Number.isFinite(value)) return clamp(Math.round(value), INSPECTOR_MIN, INSPECTOR_MAX);
    }
  } catch {
    /* localStorage 不可用時用預設值 */
  }
  return INSPECTOR_DEFAULT;
}

function saveWidth(width) {
  try {
    window.localStorage.setItem(INSPECTOR_KEY, String(Math.round(width)));
  } catch {
    /* 忽略隱私模式 / 無同源 storage 的錯誤 */
  }
}

export function initInspector(ctx) {
  const inspector = $('inspector');
  const separator = $('insp-sep');
  const grip = $('insp-grip');
  const collapseButton = $('btn-insp-collapse');
  if (!inspector || !grip) return;

  let width = readSavedWidth();
  let lastWidth = width;
  let collapsed = false;
  let dragging = false;
  let pointerId = null;
  let startX = 0;
  let startWidth = 0;

  const clampWidth = (value) => clamp(Math.round(value), INSPECTOR_MIN, INSPECTOR_MAX);

  let animTimer = 0;
  function animateLayout() {
    document.body.classList.add('insp-anim');
    if (animTimer) clearTimeout(animTimer);
    animTimer = window.setTimeout(() => {
      animTimer = 0;
      document.body.classList.remove('insp-anim');
    }, 300);
  }

  function syncAria() {
    grip.setAttribute('aria-valuemin', String(INSPECTOR_MIN));
    grip.setAttribute('aria-valuemax', String(INSPECTOR_MAX));
    grip.setAttribute('aria-valuenow', String(collapsed ? 0 : width));
    inspector.classList.toggle('is-collapsed', collapsed);
    try {
      inspector.inert = collapsed;
    } catch {
      /* 舊 WebView 沒有 inert，用 aria-hidden 降級 */
    }
    if (collapsed) inspector.setAttribute('aria-hidden', 'true');
    else inspector.removeAttribute('aria-hidden');
    if (collapseButton) {
      collapseButton.setAttribute('aria-expanded', collapsed ? 'false' : 'true');
      collapseButton.title = collapsed ? '展開屬性面板' : '收合屬性面板';
      collapseButton.setAttribute('aria-label', collapseButton.title);
      collapseButton.classList.toggle('is-collapsed', collapsed);
    }
  }

  function applyWidth(value, { persist = false } = {}) {
    width = clampWidth(value);
    lastWidth = width;
    collapsed = false;
    animateLayout();
    inspector.style.width = `${width}px`;
    document.documentElement.style.setProperty('--inspector-w', `${width}px`);
    syncAria();
    if (persist) saveWidth(width);
  }

  function collapse() {
    if (collapsed) return;
    animateLayout();
    lastWidth = width;
    collapsed = true;
    inspector.style.width = '0px';
    document.documentElement.style.setProperty('--inspector-w', '0px');
    syncAria();
  }

  function expand() {
    applyWidth(lastWidth || INSPECTOR_DEFAULT);
  }

  function toggleCollapse() {
    if (collapsed) expand();
    else collapse();
  }

  on(collapseButton, 'click', toggleCollapse);

  // 只有把手能拖：分隔線與 0 寬容器都不接收指標事件。
  on(grip, 'pointerdown', (event) => {
    if (event.button != null && event.button !== 0) return;
    event.preventDefault();
    dragging = true;
    pointerId = event.pointerId;
    startX = event.clientX;
    startWidth = inspector.getBoundingClientRect().width;
    separator?.classList.add('dragging');
    document.documentElement.style.userSelect = 'none';
    try {
      grip.setPointerCapture(event.pointerId);
    } catch {
      /* 沒有 pointer capture 時仍可由 move 事件追蹤 */
    }
  });

  on(grip, 'pointermove', (event) => {
    if (!dragging) return;
    // The inspector sits on the right: dragging the grip left widens it.
    applyWidth(startWidth - (event.clientX - startX));
  });

  function endDrag() {
    if (!dragging) return;
    dragging = false;
    separator?.classList.remove('dragging');
    document.documentElement.style.userSelect = '';
    if (pointerId != null) {
      try {
        grip.releasePointerCapture(pointerId);
      } catch {
        /* 已釋放或無 capture */
      }
      pointerId = null;
    }
    saveWidth(width);
  }

  on(grip, 'pointerup', endDrag);
  on(grip, 'pointercancel', endDrag);

  on(grip, 'keydown', (event) => {
    if (event.key === 'ArrowLeft') {
      // Handle moves left -> the right-hand inspector gets wider.
      event.preventDefault();
      applyWidth(width + INSPECTOR_STEP, { persist: true });
    } else if (event.key === 'ArrowRight') {
      event.preventDefault();
      applyWidth(width - INSPECTOR_STEP, { persist: true });
    } else if (event.key === 'Home') {
      event.preventDefault();
      applyWidth(INSPECTOR_MIN, { persist: true });
    } else if (event.key === 'End') {
      event.preventDefault();
      applyWidth(INSPECTOR_MAX, { persist: true });
    }
  });

  // 初始寬度：讀回 localStorage 並 clamp。
  applyWidth(width);

  // 視窗變窄時不讓面板超出可用空間（仍以 240–420 為準）。
  on(window, 'resize', () => {
    if (!dragging && !collapsed) applyWidth(inspector.getBoundingClientRect().width || width);
  });
}
