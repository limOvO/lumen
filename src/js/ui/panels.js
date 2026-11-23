// 面板總控：三個分頁、標註樣式、空狀態。
// 各分頁的細節在 panel-info / panel-adjust / panel-crop / style-panel。

import { subscribe } from '../core/state.js';
import { $, on } from './dom.js';
import { initAdjust } from './panel-adjust.js';
import { initInspector } from './inspector.js';
import { initCrop } from './panel-crop.js';
import { initInfo } from './panel-info.js';
import { initStyle } from './style-panel.js';

const TABS = [
  { tab: 'tab-info', panel: 'panel-info' },
  { tab: 'tab-adjust', panel: 'panel-adjust' },
  { tab: 'tab-crop', panel: 'panel-crop' },
];

export function init(ctx) {
  const { state, actions } = ctx;

  const tabButtons = TABS.map((entry) => $(entry.tab)).filter(Boolean);
  const panels = TABS.map((entry) => $(entry.panel)).filter(Boolean);

  function activate(tabId, { focus = false } = {}) {
    const index = TABS.findIndex((entry) => entry.tab === tabId);
    if (index < 0) return;
    for (let i = 0; i < TABS.length; i += 1) {
      const button = $(TABS[i].tab);
      const panel = $(TABS[i].panel);
      const active = i === index;
      if (button) {
        button.classList.toggle('is-active', active);
        button.setAttribute('aria-selected', active ? 'true' : 'false');
        button.tabIndex = active ? 0 : -1;
        if (active && focus) button.focus({ preventScroll: true });
      }
      if (panel) panel.hidden = !active;
    }
  }

  for (const button of tabButtons) {
    button.addEventListener('click', () => activate(button.id));
  }

  const tablist = $('panel-tabs');
  on(tablist, 'keydown', (event) => {
    const current = tabButtons.indexOf(document.activeElement);
    if (current < 0) return;
    let next = -1;
    if (event.key === 'ArrowRight' || event.key === 'ArrowDown') next = (current + 1) % tabButtons.length;
    else if (event.key === 'ArrowLeft' || event.key === 'ArrowUp') next = (current - 1 + tabButtons.length) % tabButtons.length;
    else if (event.key === 'Home') next = 0;
    else if (event.key === 'End') next = tabButtons.length - 1;
    if (next < 0) return;
    event.preventDefault();
    activate(tabButtons[next].id, { focus: true });
  });

  initInspector(ctx);
  initStyle(ctx);
  initInfo(ctx);
  initAdjust(ctx, { activateTab: activate });
  initCrop(ctx, { activateTab: activate });

  // 空狀態
  const empty = $('empty-state');
  on($('empty-open'), 'click', () => actions.openBrowser());
  on($('empty-shortcuts'), 'click', () => $('btn-shortcuts')?.click());

  const syncEmpty = () => {
    if (empty) empty.hidden = !!state.doc;
  };
  subscribe((scope) => {
    if (scope === 'doc' || scope === 'all') syncEmpty();
  });
  syncEmpty();

  // 切到裁切工具時自動顯示裁切分頁
  subscribe((scope) => {
    if (scope !== 'tool' && scope !== 'all') return;
    if (state.tool === 'crop') activate('tab-crop');
  });

  activate('tab-info');
}
