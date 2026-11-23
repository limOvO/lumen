// 左側工具列：選中態、點擊切換工具、鍵盤上下移動焦點。

import { subscribe } from '../core/state.js';
import { on } from './dom.js';

const TOOL_NAMES = {
  move: '移動',
  brush: '畫筆',
  rect: '矩形',
  ellipse: '圓框',
  arrow: '箭頭',
  text: '文字',
  crop: '裁切',
};

export function init(ctx) {
  const { state, actions } = ctx;
  const rail = document.getElementById('toolrail');
  if (!rail) return;
  const buttons = Array.from(rail.querySelectorAll('.rail-btn'));

  for (const button of buttons) {
    on(button, 'click', () => {
      const tool = button.dataset.tool;
      if (tool) actions.setTool(tool);
    });
  }

  // 工具列方向鍵：上下移動焦點，Home/End 到頭尾。
  on(rail, 'keydown', (event) => {
    const index = buttons.indexOf(document.activeElement);
    if (index < 0) return;
    let next = -1;
    if (event.key === 'ArrowDown') next = (index + 1) % buttons.length;
    else if (event.key === 'ArrowUp') next = (index - 1 + buttons.length) % buttons.length;
    else if (event.key === 'Home') next = 0;
    else if (event.key === 'End') next = buttons.length - 1;
    if (next < 0) return;
    event.preventDefault();
    buttons[next].focus({ preventScroll: true });
  });

  const render = () => {
    for (const button of buttons) {
      const active = state.tool === button.dataset.tool;
      button.setAttribute('aria-pressed', active ? 'true' : 'false');
      if (active) button.dataset.active = 'true';
      else delete button.dataset.active;
    }
  };

  subscribe((scope) => {
    if (scope === 'tool' || scope === 'all') render();
  });

  render();
}

export { TOOL_NAMES };
