// 標註樣式：顏色色票 + 自訂色、粗細、透明度、文字字級。
// 工具切換時即時反映（文字字級只在文字工具顯示）。

import { subscribe } from '../core/state.js';
import { $, on } from './dom.js';

const DRAW_TOOLS = new Set(['brush', 'rect', 'ellipse', 'arrow', 'line', 'text']);

const TOOL_NAMES = {
  move: '移動',
  brush: '畫筆',
  rect: '矩形',
  ellipse: '圓框',
  arrow: '箭頭',
  line: '直線',
  text: '文字',
  crop: '裁切',
};

function isHex(value) {
  return /^#[0-9a-fA-F]{6}$/.test(value || '');
}

const DOCK_IDLE_MS = 2200;

export function initStyle(ctx) {
  const { state, actions } = ctx;

  const panel = $('style-panel');
  const dock = $('style-dock');
  const toggle = $('style-toggle');
  const body = $('style-body');
  const toolName = $('style-tool-name');
  const colors = $('style-colors');
  const custom = $('style-custom');
  const size = $('style-size');
  const sizeOut = $('style-size-out');
  const opacity = $('style-opacity');
  const opacityOut = $('style-opacity-out');
  const dashed = $('style-dashed');
  const fontRow = $('style-font-row');
  const fontSize = $('style-fontsize');
  const fontSizeOut = $('style-fontsize-out');

  /* The floating style bar sleeps (dim + slid down) by default and only wakes
     up when the pointer actually comes near it — drawing on the canvas must
     never pop it back up. */
  let dockIdleTimer = 0;
  let dockAnimTimer = 0;
  const HIDDEN_TRANSFORM = 'translateX(-50%) translateY(calc(100% + 18px))';
  function clearDockInline() {
    if (!dock) return;
    dock.style.transition = '';
    dock.style.opacity = '';
    dock.style.transform = '';
  }
  function showDock() {
    if (!dock) return;
    if (dockAnimTimer) clearTimeout(dockAnimTimer);
    dock.hidden = false;
    dock.classList.add('is-idle');
    // Start below the stage and transparent, then glide into the sleeping spot.
    dock.style.transition = 'none';
    dock.style.opacity = '0';
    dock.style.transform = HIDDEN_TRANSFORM;
    void dock.offsetWidth;
    dock.style.transition =
      'opacity 220ms cubic-bezier(0.2, 0.8, 0.25, 1), transform 240ms cubic-bezier(0.2, 0.8, 0.25, 1)';
    dock.style.opacity = '';
    dock.style.transform = '';
    dockAnimTimer = window.setTimeout(clearDockInline, 280);
  }
  function hideDock() {
    if (!dock || dock.hidden) return;
    if (dockAnimTimer) clearTimeout(dockAnimTimer);
    dock.style.transition =
      'opacity 160ms ease, transform 180ms cubic-bezier(0.2, 0.8, 0.25, 1)';
    dock.style.opacity = '0';
    dock.style.transform = HIDDEN_TRANSFORM;
    dockAnimTimer = window.setTimeout(() => {
      dock.hidden = true;
      clearDockInline();
    }, 190);
  }
  function markIdle() {
    if (dock) dock.classList.add('is-idle');
  }
  function wakeDock() {
    if (!dock) return;
    dock.classList.remove('is-idle');
    if (dockIdleTimer) clearTimeout(dockIdleTimer);
    dockIdleTimer = window.setTimeout(markIdle, DOCK_IDLE_MS);
  }
  if (dock) {
    for (const evt of ['pointerenter', 'pointermove', 'pointerdown', 'focusin']) {
      dock.addEventListener(evt, wakeDock, { passive: true });
    }
    dock.addEventListener(
      'pointerleave',
      () => {
        if (dockIdleTimer) clearTimeout(dockIdleTimer);
        dockIdleTimer = window.setTimeout(markIdle, 420);
      },
      { passive: true }
    );
    const stageEl = $('stage');
    if (stageEl) {
      // Wake zone = the union of the dock's awake box and its sleeping box
      // (it slides down 58% when idle). Nothing outside that band wakes it.
      const SLEEP_SHIFT = 0.58; // must match #style-dock.is-idle translateY
      const PAD = 6;
      stageEl.addEventListener(
        'pointermove',
        (event) => {
          if (dock.hidden || event.buttons) return; // never while drawing/dragging
          const rect = dock.getBoundingClientRect();
          const asleep = dock.classList.contains('is-idle');
          const awakeTop = asleep ? rect.top - rect.height * SLEEP_SHIFT : rect.top;
          const top = Math.min(awakeTop, rect.top) - PAD;
          const bottom = Math.max(awakeTop + rect.height, rect.bottom) + PAD;
          const left = rect.left - PAD;
          const right = rect.right + PAD;
          const { clientX: x, clientY: y } = event;
          if (x >= left && x <= right && y >= top && y <= bottom) wakeDock();
        },
        { passive: true }
      );
    }
  }

  if (toggle) {
    toggle.addEventListener('click', () => {
      const collapsed = panel.classList.toggle('collapsed');
      toggle.setAttribute('aria-expanded', collapsed ? 'false' : 'true');
      if (body) body.hidden = collapsed;
    });
  }

  const swatches = colors ? Array.from(colors.querySelectorAll('.swatch[data-color]')) : [];
  for (const swatch of swatches) {
    on(swatch, 'click', () => actions.setStyle({ color: swatch.dataset.color }));
  }

  on(custom, 'input', () => actions.setStyle({ color: custom.value }));
  on(custom, 'change', () => custom.blur());
  on(size, 'input', () => actions.setStyle({ size: Number(size.value) }));
  on(size, 'change', () => size.blur());
  on(opacity, 'input', () => actions.setStyle({ opacity: Number(opacity.value) / 100 }));
  on(opacity, 'change', () => opacity.blur());
  on(dashed, 'click', () => {
    actions.setStyle({ dashed: !state.style.dashed });
    dashed.blur();
  });
  on(fontSize, 'input', () => actions.setStyle({ fontSize: Number(fontSize.value) }));
  on(fontSize, 'change', () => fontSize.blur());

  function render() {
    const style = state.style || {};
    const color = isHex(style.color) ? style.color.toLowerCase() : '';
    for (const swatch of swatches) {
      const active = (swatch.dataset.color || '').toLowerCase() === color;
      swatch.classList.toggle('is-active', active);
      swatch.setAttribute('aria-pressed', active ? 'true' : 'false');
    }
    if (custom && color) custom.value = color;

    if (size) size.value = String(style.size ?? 8);
    if (sizeOut) sizeOut.textContent = String(Math.round(style.size ?? 8));
    if (opacity) opacity.value = String(Math.round((style.opacity ?? 1) * 100));
    if (opacityOut) opacityOut.textContent = `${Math.round((style.opacity ?? 1) * 100)}%`;
    if (dashed) {
      dashed.setAttribute('aria-pressed', style.dashed ? 'true' : 'false');
      dashed.classList.toggle('is-active', !!style.dashed);
    }
    if (fontSize) fontSize.value = String(style.fontSize ?? 36);
    if (fontSizeOut) fontSizeOut.textContent = String(Math.round(style.fontSize ?? 36));

    syncTool();
  }

  function syncTool() {
    const tool = state.tool || 'move';
    if (toolName) {
      toolName.textContent = TOOL_NAMES[tool] ? `工具：${TOOL_NAMES[tool]}` : '移動';
    }
    if (fontRow) fontRow.hidden = tool !== 'text';
    // 虛線不適用於文字
    if (dashed) dashed.hidden = tool === 'text';
    if (dock) {
      const shouldShow = !!(state.doc && DRAW_TOOLS.has(tool));
      if (!shouldShow) {
        if (dockIdleTimer) {
          clearTimeout(dockIdleTimer);
          dockIdleTimer = 0;
        }
        hideDock();
      } else if (dock.hidden) {
        if (dockIdleTimer) {
          clearTimeout(dockIdleTimer);
          dockIdleTimer = 0;
        }
        showDock();
      }
    }
  }

  subscribe((scope) => {
    if (scope === 'style' || scope === 'tool' || scope === 'doc' || scope === 'all') render();
    // 工具名稱每次狀態變更都重讀，避免任何 emit 順序造成標籤落後。
    syncTool();
  });

  render();
  syncTool();
}
