// 頂欄：檔名／未存點、開啟／儲存／另存、復原／重做、比較原圖、快捷鍵入口。

import { canRedo, canUndo, subscribe } from '../core/state.js';
import { $, on, rafThrottle } from './dom.js';
import { initZoomHud } from './zoom-hud.js';

export function init(ctx) {
  const { state, actions } = ctx;

  const els = {
    fileName: $('file-name'),
    dirtyDot: $('dirty-dot'),
    open: $('btn-open'),
    save: $('btn-save'),
    saveAs: $('btn-saveas'),
    undo: $('btn-undo'),
    redo: $('btn-redo'),
    compare: $('btn-compare'),
    busy: $('busy-indicator'),
  };

  on(els.open, 'click', () => actions.openBrowser());
  on(els.save, 'click', () => actions.save());
  on(els.saveAs, 'click', () => {
    if (typeof actions.openSaveDialog === 'function') actions.openSaveDialog();
    else window.__lumenUI?.openSaveModal?.();
  });
  on(els.undo, 'click', () => actions.undo());
  on(els.redo, 'click', () => actions.redo());
  on(els.compare, 'click', () => actions.toggleCompare());

  const render = rafThrottle(() => {
    const doc = state.doc;
    const name = doc?.name || '未開啟圖片';
    if (els.fileName) {
      els.fileName.textContent = name;
      els.fileName.title = doc?.path || '目前沒有開啟的圖片';
    }
    if (els.dirtyDot) els.dirtyDot.hidden = !(doc && state.dirty);

    if (els.save) els.save.disabled = !doc || state.busy;
    if (els.saveAs) els.saveAs.disabled = !doc || state.busy;
    if (els.compare) {
      els.compare.disabled = !doc;
      els.compare.setAttribute('aria-pressed', state.compare ? 'true' : 'false');
    }
    if (els.undo) els.undo.disabled = !canUndo();
    if (els.redo) els.redo.disabled = !canRedo();
    if (els.busy) els.busy.hidden = !state.busy;
    document.body.classList.toggle('is-busy', !!state.busy);
  });

  subscribe((scope) => {
    if (
      scope === 'all' ||
      scope === 'doc' ||
      scope === 'dirty' ||
      scope === 'undo' ||
      scope === 'view' ||
      scope === 'busy' ||
      scope === 'objects' ||
      scope === 'crop'
    ) {
      render();
    }
  });

  initZoomHud(ctx);
  render();
}
