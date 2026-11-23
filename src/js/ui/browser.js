// 資料夾瀏覽器：目前路徑、上層、子資料夾（先）＋圖片（後）、
// 延遲載入縮圖、資料夾單擊/雙擊進入、圖片雙擊開啟。

import {
  $,
  clamp,
  dirname,
  fmtBytes,
  fmtDate,
  ICONS,
  iconNode,
  naturalCompare,
  on,
} from './dom.js';

let moduleCtx = null;
let currentDir = '';
let items = []; // { type:'dir'|'file', path, name, size, mtime }
let selectedIndex = -1;
let loadToken = 0;
let lastFocus = null;
let isOpen = false;

function stateNode(text) {
  const node = document.createElement('div');
  node.className = 'list-state';
  node.textContent = text;
  return node;
}

function setOpen(open) {
  const modal = $('browser-modal');
  if (!modal) return;
  modal.hidden = !open;
  isOpen = open;
  document.body.classList.toggle('modal-open', open);
  if (open) {
    lastFocus = document.activeElement;
  } else if (lastFocus && typeof lastFocus.focus === 'function') {
    lastFocus.focus({ preventScroll: true });
    lastFocus = null;
  }
}

function sortByName(list) {
  return list.slice().sort((a, b) => naturalCompare(a.name || '', b.name || ''));
}

async function listCommand(command, dir) {
  try {
    const result = await moduleCtx.bridge.invoke(command, { dir });
    return Array.isArray(result) ? result : [];
  } catch {
    return null;
  }
}

function directoryCount() {
  return items.filter((item) => item.type === 'dir').length;
}

function fileCount() {
  return items.filter((item) => item.type === 'file').length;
}

function updateCount() {
  const count = $('browser-count');
  if (!count) return;
  const dirs = directoryCount();
  const files = fileCount();
  if (!dirs && !files) count.textContent = '沒有項目';
  else if (!dirs) count.textContent = `${files} 張圖片`;
  else if (!files) count.textContent = `${dirs} 個資料夾`;
  else count.textContent = `${dirs} 個資料夾 · ${files} 張圖片`;
}

function updateSelection({ scroll = false } = {}) {
  const list = $('browser-list');
  if (!list) return;
  const rows = Array.from(list.querySelectorAll('.browser-row'));
  rows.forEach((row, index) => {
    const active = index === selectedIndex;
    row.classList.toggle('is-active', active);
    row.setAttribute('aria-selected', active ? 'true' : 'false');
    if (active && scroll) row.scrollIntoView({ block: 'nearest' });
  });
}

function makeThumb(entry) {
  const thumb = document.createElement('span');
  if (entry.type === 'dir') {
    thumb.className = 'browser-thumb browser-thumb-dir';
    thumb.append(iconNode(ICONS.folder));
    return thumb;
  }

  thumb.className = 'browser-thumb';
  const img = document.createElement('img');
  img.alt = '';
  img.loading = 'lazy';
  img.decoding = 'async';
  try {
    img.src = moduleCtx.bridge.convertFileSrc(entry.path);
  } catch {
    img.src = '';
  }
  img.addEventListener('error', () => {
    img.remove();
    if (!thumb.firstElementChild) thumb.append(iconNode(ICONS.image));
  });
  thumb.append(img);
  return thumb;
}

function makeRow(entry, index) {
  const row = document.createElement('button');
  row.type = 'button';
  row.className = entry.type === 'dir' ? 'browser-row browser-row-dir' : 'browser-row';
  row.setAttribute('role', 'option');
  row.setAttribute('aria-selected', 'false');
  row.dataset.path = entry.path || '';
  row.dataset.type = entry.type;
  row.dataset.index = String(index);

  const info = document.createElement('span');
  info.className = 'browser-info';
  const name = document.createElement('span');
  name.className = 'browser-name';
  name.textContent = entry.name || entry.path || '';
  const meta = document.createElement('span');
  meta.className = 'browser-meta';
  if (entry.type === 'dir') {
    meta.textContent = '資料夾';
  } else {
    meta.textContent = `${fmtBytes(entry.size)} · ${fmtDate(entry.mtime)}`;
  }
  info.append(name, meta);
  row.append(makeThumb(entry), info);

  if (entry.type === 'dir') {
    // 資料夾：單擊即進入（雙擊也走同一條路徑，enterDir 會忽略重複）。
    row.addEventListener('click', () => {
      selectedIndex = index;
      updateSelection();
      enterDir(entry.path);
    });
    row.addEventListener('dblclick', () => enterDir(entry.path));
  } else {
    row.addEventListener('click', () => {
      selectedIndex = index;
      updateSelection();
    });
    row.addEventListener('dblclick', () => {
      selectedIndex = index;
      openItem();
    });
  }
  return row;
}

function renderList() {
  const list = $('browser-list');
  if (!list) return;
  list.textContent = '';
  list.removeAttribute('aria-busy');

  if (!items.length) {
    list.append(stateNode('這個資料夾沒有圖片'));
    updateCount();
    return;
  }

  const fragment = document.createDocumentFragment();
  items.forEach((entry, index) => fragment.append(makeRow(entry, index)));
  list.append(fragment);

  // 只有子資料夾、沒有圖片時，補一行說明但保留資料夾列。
  const files = items.filter((item) => item.type === 'file');
  if (!files.length && directoryCount() > 0) {
    const note = document.createElement('p');
    note.className = 'list-state list-state-inline';
    note.textContent = '這個資料夾沒有圖片';
    list.append(note);
  }

  if (selectedIndex >= items.length) selectedIndex = items.length - 1;
  updateSelection();
  updateCount();
}

async function loadDir(dir) {
  const target = String(dir || '').trim();
  const pathInput = $('browser-path');
  const list = $('browser-list');
  if (!target) {
    if (list) {
      list.textContent = '';
      list.append(stateNode('請輸入資料夾路徑'));
    }
    return;
  }

  currentDir = target;
  if (pathInput) pathInput.value = target;
  selectedIndex = -1;
  const token = ++loadToken;
  if (list) {
    list.textContent = '';
    list.setAttribute('aria-busy', 'true');
    list.append(stateNode('載入中…'));
  }

  const [images, dirs] = await Promise.all([
    listCommand('list_images', target),
    listCommand('list_dirs', target),
  ]);
  if (token !== loadToken) return;

  let imageList = images;
  let failed = false;
  if (imageList === null) {
    // 瀏覽器環境或權限受限時，退回目前資料夾清單。
    if (target === moduleCtx.state.doc?.dir && Array.isArray(moduleCtx.state.files)) {
      imageList = moduleCtx.state.files;
    } else {
      imageList = [];
      failed = true;
    }
  }
  const dirList = dirs === null ? [] : dirs;

  items = [
    ...sortByName(dirList).map((entry) => ({
      type: 'dir',
      path: entry.path,
      name: entry.name || entry.path,
      size: 0,
      mtime: entry.mtime || 0,
    })),
    ...sortByName(imageList).map((entry) => ({
      type: 'file',
      path: entry.path,
      name: entry.name,
      size: entry.size,
      mtime: entry.mtime,
    })),
  ];

  if (failed && !items.length) {
    if (list) {
      list.textContent = '';
      list.append(stateNode('無法讀取這個資料夾'));
    }
    moduleCtx.actions.toast('無法讀取資料夾', 'warn');
    updateCount();
    return;
  }

  renderList();
}

function enterDir(path) {
  if (!path || path === currentDir) return;
  if (typeof moduleCtx.actions.browseTo === 'function') moduleCtx.actions.browseTo(path);
  else loadDir(path);
}

function openItem() {
  const entry = items[selectedIndex];
  if (!entry) {
    moduleCtx.actions.toast('請先選擇資料夾或圖片', 'warn');
    return;
  }
  if (entry.type === 'dir') {
    enterDir(entry.path);
    return;
  }
  if (!entry.path) return;
  close();
  moduleCtx.actions.openPath(entry.path);
}

function go(dir) {
  if (!dir) return;
  loadDir(dir);
}

function goUp() {
  const parent = dirname(currentDir);
  if (!parent || parent === currentDir) return;
  go(parent);
}

export function init(ctx) {
  moduleCtx = ctx;
  const modal = $('browser-modal');
  if (!modal) return;

  for (const el of modal.querySelectorAll('[data-close]')) {
    el.addEventListener('click', close);
  }

  on($('browser-up'), 'click', goUp);
  on($('browser-open'), 'click', openItem);

  const pathInput = $('browser-path');
  on(pathInput, 'keydown', (event) => {
    if (event.key === 'Enter') {
      event.preventDefault();
      go(pathInput.value);
    }
  });

  const list = $('browser-list');
  on(list, 'keydown', (event) => {
    if (!items.length) return;
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault();
      const delta = event.key === 'ArrowDown' ? 1 : -1;
      const next = selectedIndex < 0 ? 0 : clamp(selectedIndex + delta, 0, items.length - 1);
      selectedIndex = next;
      updateSelection({ scroll: true });
      list.querySelectorAll('.browser-row')[next]?.focus({ preventScroll: true });
      return;
    }
    if (event.key === 'Home' || event.key === 'End') {
      event.preventDefault();
      selectedIndex = event.key === 'Home' ? 0 : items.length - 1;
      updateSelection({ scroll: true });
      list.querySelectorAll('.browser-row')[selectedIndex]?.focus({ preventScroll: true });
      return;
    }
    if (event.key === 'Enter') {
      event.preventDefault();
      openItem();
    }
  });

  document.addEventListener('keydown', (event) => {
    if (!isOpen) return;
    if (event.key === 'Escape') {
      event.preventDefault();
      event.stopPropagation();
      close();
    }
  });
}

export async function open(dir) {
  if (!moduleCtx) return;
  if (isOpen) {
    await loadDir(dir || currentDir);
    return;
  }
  lastFocus = document.activeElement;
  const modal = $('browser-modal');
  if (modal) modal.hidden = false;
  isOpen = true;
  document.body.classList.add('modal-open');

  const start = String(dir || moduleCtx.state.doc?.dir || '');
  await loadDir(start);
  const pathInput = $('browser-path');
  if (pathInput) {
    pathInput.focus({ preventScroll: true });
    pathInput.select();
  }
}

export function browseTo(dir) {
  return loadDir(dir);
}

export function close() {
  setOpen(false);
}

export function isBrowserOpen() {
  return isOpen;
}
