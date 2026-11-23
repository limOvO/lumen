// 底片條：同資料夾縮圖、當前檔高亮、點擊切換、可收合。
// 縮圖走 Tauri asset protocol（bridge.convertFileSrc），原生 lazy 載入。

import { subscribe } from '../core/state.js';
import { $, ICONS, iconNode, on } from './dom.js';

export function init(ctx) {
  const { state, actions, bridge } = ctx;
  const strip = $('filmstrip');
  const container = $('film-items');
  const count = $('film-count');
  const toggle = $('film-toggle');
  if (!container) return;

  on(toggle, 'click', () => actions.toggleFilmstrip());

  let signature = '';
  let currentPath = null;

  function fallbackNode() {
    const span = document.createElement('span');
    span.className = 'film-thumb-fallback';
    span.append(iconNode(ICONS.image));
    return span;
  }

  function makeImage(file) {
    const img = document.createElement('img');
    img.alt = '';
    img.loading = 'lazy';
    img.decoding = 'async';
    try {
      img.src = bridge.convertFileSrc(file.path);
    } catch {
      img.src = '';
    }
    img.addEventListener('error', () => {
      img.remove();
      const item = img.closest('.film-item');
      if (item && !item.querySelector('.film-thumb-fallback')) item.prepend(fallbackNode());
    });
    return img;
  }

  function build() {
    container.textContent = '';
    const files = Array.isArray(state.files) ? state.files : [];
    if (!files.length) {
      const empty = document.createElement('span');
      empty.className = 'film-empty';
      empty.textContent = '這個資料夾沒有其他圖片';
      container.append(empty);
      return;
    }
    const fragment = document.createDocumentFragment();
    for (const file of files) {
      const item = document.createElement('button');
      item.type = 'button';
      item.className = 'film-item';
      item.setAttribute('role', 'option');
      item.setAttribute('aria-selected', 'false');
      item.dataset.path = file.path;
      item.title = file.path || file.name || '';
      item.append(makeImage(file));

      const name = document.createElement('span');
      name.className = 'film-name';
      name.textContent = file.name || '';
      item.append(name);

      item.addEventListener('click', () => {
        if (file.path && file.path !== state.doc?.path) actions.openPath(file.path);
      });
      fragment.append(item);
    }
    container.append(fragment);
  }

  function syncSelection() {
    const files = Array.isArray(state.files) ? state.files : [];
    currentPath = state.doc?.path || null;
    const nodes = Array.from(container.querySelectorAll('.film-item'));
    let activeIndex = currentPath == null ? -1 : nodes.findIndex((item) => item.dataset.path === currentPath);
    if (activeIndex < 0 && Number.isInteger(state.index) && state.index >= 0 && state.index < nodes.length) {
      activeIndex = state.index;
    }
    nodes.forEach((item, index) => {
      const active = index === activeIndex;
      item.setAttribute('aria-selected', active ? 'true' : 'false');
      if (active) item.classList.add('is-current');
      else item.classList.remove('is-current');
    });
    const active = nodes[activeIndex];
    if (active) {
      active.scrollIntoView({ block: 'nearest', inline: 'nearest' });
    }
    if (count) count.textContent = `${files.length} 張`;
    if (toggle) toggle.setAttribute('aria-expanded', state.filmstrip === false ? 'false' : 'true');
    // 沒有同資料夾圖片時整條收合，不留空的黑色長條。
    document.body.classList.toggle('filmstrip-empty', files.length === 0);
  }

  function render() {
    const files = Array.isArray(state.files) ? state.files : [];
    const nextSignature = files.map((file) => file.path).join('\n');
    if (nextSignature !== signature) {
      signature = nextSignature;
      build();
    }
    syncSelection();
  }

  subscribe((scope) => {
    if (scope === 'files' || scope === 'doc' || scope === 'all') render();
  });

  render();

  // 水平滾輪：底片條只在有圖片時才需要。
  on(
    strip,
    'wheel',
    (event) => {
      if (Math.abs(event.deltaY) <= Math.abs(event.deltaX)) return;
      if (container.scrollWidth <= container.clientWidth) return;
      event.preventDefault();
      container.scrollLeft += event.deltaY;
    },
    { passive: false }
  );
}
