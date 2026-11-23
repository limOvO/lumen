// 縮放控制：編輯模式時併入左側工具列（直排），預覽模式回到畫布右下角（橫排）。

import { subscribe } from '../core/state.js';
import { $, on, rafThrottle } from './dom.js';

export function initZoomHud(ctx) {
  const { state, actions } = ctx;
  const label = $('zoom-label');
  const out = $('zoom-out');
  const inn = $('zoom-in');
  const fit = $('zoom-fit');
  const hud = $('zoom-hud');
  const rail = $('toolrail');
  const canvasArea = $('canvas-area');

  on(out, 'click', () => actions.zoomOut());
  on(inn, 'click', () => actions.zoomIn());
  on(fit, 'click', () => actions.zoomFit());
  on(label, 'click', () => actions.zoomFit());

  /** The HUD lives in the left rail while editing and floats in the canvas
   *  during preview, so both modes keep a usable zoom control. */
  function place() {
    if (!hud) return;
    const target = state.phase === 'edit' ? rail : canvasArea;
    if (target && hud.parentElement !== target) target.appendChild(hud);
  }

  const render = rafThrottle(() => {
    const hasDoc = !!state.doc;
    const scale = Number(state.view?.scale);
    if (label) {
      label.textContent = hasDoc && Number.isFinite(scale) ? `${Math.round(scale * 100)}%` : '—';
    }
    for (const btn of [out, inn, fit]) {
      if (btn) btn.disabled = !hasDoc;
    }
  });

  subscribe((scope) => {
    if (scope === 'view' || scope === 'doc' || scope === 'phase' || scope === 'all') {
      place();
      render();
    }
  });

  place();
  render();
}
