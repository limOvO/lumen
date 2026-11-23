// Lumen — application shell.
// Owns: Tauri bridge, document/folder lifecycle, render loop, actions,
// global keyboard, and wiring between the canvas engine and the UI modules.

import {
  state, subscribe, emit, commit, undo, redo, canUndo, canRedo,
  resetHistory, setTool, setStyle, setView, setCrop, setOutput,
  setBusy, markSaved, selectObject, removeObject, clearObjects,
  setPhase as setPhaseState, setPeekInfo,
} from './core/state.js';

import * as loader from './core/loader.js';
import * as viewport from './core/viewport.js';
import * as annotate from './core/annotate.js';
import * as exporter from './core/export.js';

const tauriCore = window.__TAURI__?.core;
const isTauri = !!(tauriCore && typeof tauriCore.invoke === 'function');

/* ------------------------------------------------------------------ *
 * Bridge (real IPC, or a small in-page mock so the frontend can be
 * developed/debugged in a plain browser as well)
 * ------------------------------------------------------------------ */

const DEMO_FILES = [
  { path: 'samples/coast.jpg', name: 'coast.jpg' },
  { path: 'samples/studio.jpg', name: 'studio.jpg' },
  { path: 'samples/street.jpg', name: 'street.jpg' },
];

function demoList() {
  return DEMO_FILES.map((f, i) => ({ ...f, size: 0, mtime: 1700000000 + i }));
}

async function demoBytes(path) {
  const url = new URL(path, location.href).href;
  const res = await fetch(url);
  if (!res.ok) throw new Error('無法讀取示範圖片：' + path);
  return new Uint8Array(await res.arrayBuffer());
}

const mockInvoke = async (cmd, args = {}) => {
  switch (cmd) {
    case 'read_image_file':
      return demoBytes(args.path);
    case 'list_images':
      return demoList();
    case 'inspect_image': {
      const bytes = await demoBytes(args.path);
      const file = DEMO_FILES.find((f) => f.path === args.path) || {
        path: args.path,
        name: args.path.split('/').pop(),
      };
      return {
        ...file,
        dir: 'samples',
        size: bytes.length,
        mtime: 1700000000,
        width: 0,
        height: 0,
        format: (file.name.split('.').pop() || '').toUpperCase(),
        exif: [],
      };
    }
    case 'startup_files':
      return ['samples/coast.jpg'];
    case 'home_pictures':
      return 'samples';
    default:
      return null;
  }
};

export const bridge = {
  isTauri,
  invoke: isTauri ? tauriCore.invoke.bind(tauriCore) : mockInvoke,
  convertFileSrc: (p) =>
    isTauri && tauriCore.convertFileSrc ? tauriCore.convertFileSrc(p) : new URL(p, location.href).href,
  listen: (event, handler) =>
    window.__TAURI__?.event?.listen ? window.__TAURI__.event.listen(event, handler) : Promise.resolve(() => {}),
};

/* ------------------------------------------------------------------ *
 * Elements
 * ------------------------------------------------------------------ */

const $ = (id) => document.getElementById(id);
const els = {
  stage: $('stage'),
  board: $('board'),
  overlay: $('overlay'),
  toolrail: $('toolrail'),
  filmstrip: $('filmstrip'),
  textEditor: $('textEditor'),
};

/* ------------------------------------------------------------------ *
 * Phase chrome (preview <-> edit)
 * ------------------------------------------------------------------ */

let phaseChangedAt = 0;
let phaseAnimTimer = 0;
let viewTween = 0;
/** Preview image switch: {canvas, start, duration} — snapshot fading out. */
let switchFade = null;
const SWITCH_FADE_MS = 260;
const SWITCH_SLIDE = 0.14; // fraction of the stage width
let phaseTransitioning = false;
let lastAppliedPhase = '';
let phaseEndTimer = 0;

/** CSS custom property as a number (px values only). */
function cssPx(name, fallback) {
  const raw = getComputedStyle(document.documentElement).getPropertyValue(name).trim();
  const n = parseFloat(raw);
  return Number.isFinite(n) ? n : fallback;
}

/** Stage size once the layout transition has finished (used to compute the
 *  animation target up-front, so the picture follows one straight eased path
 *  instead of chasing a moving target). */
function finalStageSize() {
  const app = document.getElementById('app');
  const rect = app ? app.getBoundingClientRect() : { width: cssW, height: cssH };
  if (state.phase === 'preview') {
    return { w: Math.max(1, Math.round(rect.width)), h: Math.max(1, Math.round(rect.height)) };
  }
  const rail = cssPx('--rail-w', 52);
  const insp = cssPx('--inspector-w', 300);
  const top = cssPx('--topbar-h', 46);
  const status = cssPx('--statusbar-h', 24);
  return {
    w: Math.max(1, Math.round(rect.width - rail - insp)),
    h: Math.max(1, Math.round(rect.height - top - status)),
  };
}

/** Both directions land on a fitted, centred picture. */
function phaseTargetView() {
  const { w, h } = finalStageSize();
  if (!state.doc) return state.view;
  return fitViewFor(w, h);
}

function stopViewTween() {
  if (viewTween) {
    cancelAnimationFrame(viewTween);
    viewTween = 0;
  }
}

/**
 * Phase switches: panels/grid animate for 260ms, so the picture eases toward the
 * per-frame target instead of snapping. Entering the viewer always re-centres
 * (and re-fits) even if it was panned/zoomed in the editor.
 */
function startPhaseViewTween(duration = 340, originDx = 0, originDy = 0) {
  stopViewTween();
  if (!state.doc) return;
  // Rebase the start view onto the new canvas origin: the layout snapped, so the
  // same tx/ty would otherwise draw the picture shifted by the origin delta.
  const from = { ...state.view, tx: state.view.tx + originDx, ty: state.view.ty + originDy };
  const target = phaseTargetView();
  const t0 = performance.now();
  const step = () => {
    if (!state.doc) {
      viewTween = 0;
      return;
    }
    const p = Math.min(1, (performance.now() - t0) / duration);
    const e = 1 - Math.pow(1 - p, 3);
    // One fixed target + one eased parameter => both axes move together.
    state.view = {
      scale: from.scale + (target.scale - from.scale) * e,
      tx: from.tx + (target.tx - from.tx) * e,
      ty: from.ty + (target.ty - from.ty) * e,
      fitMode: 'fit',
    };
    render();
    if (p < 1) {
      viewTween = requestAnimationFrame(step);
    } else {
      viewTween = 0;
      state.view = target;
      emit('view');
      render();
    }
  };
  viewTween = requestAnimationFrame(step);
}

function applyPhaseChrome() {
  const body = document.body;
  const hasDoc = !!state.doc;
  // Where the canvas sits *before* the layout switches, so the first animation
  // frame can keep the picture visually in place instead of jumping.
  const stageRectBefore = els.stage ? els.stage.getBoundingClientRect() : null;
  body.classList.toggle('phase-preview', hasDoc && state.phase === 'preview');
  body.classList.toggle('phase-edit', hasDoc && state.phase === 'edit');
  body.classList.toggle('peek-info', hasDoc && state.phase === 'preview' && state.peekInfo);
  body.classList.toggle('no-doc', !hasDoc);
  // Only a real preview <-> edit switch gets the glide; opening the first
  // document must fit immediately.
  const phaseKey = hasDoc ? state.phase : 'none';
  const prevPhase = lastAppliedPhase;
  lastAppliedPhase = phaseKey;
  const isSwitch =
    hasDoc && (prevPhase === 'preview' || prevPhase === 'edit') && prevPhase !== phaseKey;
  if (!isSwitch) return;

  phaseChangedAt = performance.now();
  phaseTransitioning = true;
  document.body.classList.add('phase-anim');
  if (phaseAnimTimer) clearTimeout(phaseAnimTimer);
  phaseAnimTimer = window.setTimeout(() => {
    phaseAnimTimer = 0;
    document.body.classList.remove('phase-anim');
  }, 380);
  if (phaseEndTimer) clearTimeout(phaseEndTimer);
  phaseEndTimer = window.setTimeout(() => {
    phaseEndTimer = 0;
    phaseTransitioning = false;
  }, 950);

  // The grid columns/rows animate via CSS; tween the view against the layout so
  // the picture moves (and re-centres when entering the viewer) naturally.
  syncCanvasSize();
  const stageRectAfter = els.stage ? els.stage.getBoundingClientRect() : null;
  const originDx = stageRectBefore && stageRectAfter ? stageRectBefore.left - stageRectAfter.left : 0;
  const originDy = stageRectBefore && stageRectAfter ? stageRectBefore.top - stageRectAfter.top : 0;
  startPhaseViewTween(340, originDx, originDy);
}

/* ------------------------------------------------------------------ *
 * Canvas sizing / render loop
 * ------------------------------------------------------------------ */

let dpr = 1;
let cssW = 0;
let cssH = 0;
let renderQueued = false;
let interaction = null;

function measureStage() {
  if (!els.stage) return { w: 0, h: 0 };
  const r = els.stage.getBoundingClientRect();
  return { w: Math.max(1, Math.round(r.width)), h: Math.max(1, Math.round(r.height)) };
}

function syncCanvasSize() {
  const { w, h } = measureStage();
  dpr = Math.min(window.devicePixelRatio || 1, 2);
  if (w === cssW && h === cssH && els.board.width === Math.round(w * dpr)) return false;
  cssW = w;
  cssH = h;
  for (const c of [els.board, els.overlay]) {
    if (!c) continue;
    c.width = Math.round(w * dpr);
    c.height = Math.round(h * dpr);
  }
  return true;
}

/** Zoom floor: the image may never become smaller than "fit to window". */
function currentMinScale() {
  if (!state.doc) return 0.02;
  const { w, h } = stageSize();
  if (typeof viewport.minScale === 'function') return viewport.minScale(state.doc, w, h);
  return viewport.fitScale(
    state.doc.width,
    state.doc.height,
    Math.max(32, w - 64),
    Math.max(32, h - 64)
  );
}

function clampView(v) {
  if (!state.doc) return v;
  // Preview keeps the picture inside the viewport; the editor lets you drag it
  // past the edges on purpose.
  if (state.phase !== 'preview') return v;
  const { w, h } = stageSize();
  if (typeof viewport.clampPan === 'function') return viewport.clampPan(v, state.doc, w, h);
  return v;
}

function setSaveStatus(status) {
  ui.saveStatus?.set?.(status);
}

function statusMessage(msg, kind = 'info') {
  const fn = ui.statusbar?.message || ui.statusbar?.show;
  if (typeof fn === 'function') fn.call(ui.statusbar, msg, kind);
  else console.warn(`[${kind}]`, msg);
}

export function requestRender() {
  if (renderQueued) return;
  renderQueued = true;
  requestAnimationFrame(() => {
    renderQueued = false;
    render();
  });
}

function docHasAlpha(doc) {
  if (!doc) return false;
  if (typeof doc.hasAlpha === 'boolean') return doc.hasAlpha;
  return /png|webp|gif|svg|ico|avif/i.test(doc.mime || '');
}

/**
 * Stage backdrop: a flat dark surface like a photo viewer. The checkerboard is
 * only painted *behind the image rectangle* and only when the image can have
 * transparency, so an opaque photo never looks like a drawing canvas.
 */
function paintBackdrop(ctx) {
  const bg =
    getComputedStyle(document.documentElement).getPropertyValue('--bg').trim() || '#0e1013';
  ctx.fillStyle = bg;
  ctx.fillRect(0, 0, cssW, cssH);
  if (!state.doc || !docHasAlpha(state.doc)) return;
  const scale = state.view.scale;
  const x = state.view.tx;
  const y = state.view.ty;
  const w = state.doc.width * scale;
  const h = state.doc.height * scale;
  if (w <= 0 || h <= 0) return;
  ctx.save();
  ctx.beginPath();
  ctx.rect(x, y, w, h);
  ctx.clip();
  viewport.drawBackground(ctx, cssW, cssH, 1);
  ctx.restore();
}

export function snapshotBoard() {
  if (!els.board || els.board.width < 2) return null;
  const snap = document.createElement('canvas');
  snap.width = els.board.width;
  snap.height = els.board.height;
  const ctx = snap.getContext('2d');
  if (!ctx) return null;
  ctx.drawImage(els.board, 0, 0);
  return snap;
}

function render() {
  syncCanvasSize();
  const board = els.board;
  if (!board) return;
  const ctx = board.getContext('2d', { alpha: true });
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.clearRect(0, 0, cssW, cssH);

  paintBackdrop(ctx);

  if (state.doc) {
    const transition = switchFade
      ? (() => {
          const p = Math.min(1, (performance.now() - switchFade.start) / SWITCH_FADE_MS);
          return { p, e: 1 - Math.pow(1 - p, 3), dir: switchFade.dir || 0 };
        })()
      : null;
    ctx.save();
    if (transition && transition.dir) {
      // New picture enters from the side the arrow points away from.
      ctx.translate((1 - transition.e) * transition.dir * cssW * SWITCH_SLIDE, 0);
    }
    if (state.compare) {
      viewport.drawDoc(ctx, { ...state.doc }, state.view, 1);
    } else {
      viewport.drawDoc(ctx, state.doc, state.view, 1);
      annotate.drawObjects(ctx, state.objects, state.view, 1, {
        selectedId: state.selectedId,
        showHandles: false,
      });
      const scale = state.view.scale;
      const ix = state.view.tx;
      const iy = state.view.ty;
      const iw = state.doc.width * scale;
      const ih = state.doc.height * scale;
      ctx.strokeStyle = 'rgba(255,255,255,0.10)';
      ctx.lineWidth = 1;
      ctx.strokeRect(Math.round(ix) + 0.5, Math.round(iy) + 0.5, Math.round(iw), Math.round(ih));
    }
    ctx.restore();
  }

  if (switchFade) {
    const p = Math.min(1, (performance.now() - switchFade.start) / SWITCH_FADE_MS);
    const eased = 1 - Math.pow(1 - p, 3);
    const dir = switchFade.dir || 0;
    ctx.save();
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.globalAlpha = 1 - eased;
    if (dir) ctx.translate(-eased * dir * cssW * SWITCH_SLIDE * dpr, 0);
    ctx.drawImage(switchFade.canvas, 0, 0);
    ctx.restore();
    if (p < 1) requestRender();
    else switchFade = null;
  }

  if (interaction) interaction.paintOverlay();
}

/* ------------------------------------------------------------------ *
 * View helpers
 * ------------------------------------------------------------------ */

function stageSize() {
  return { w: cssW || measureStage().w, h: cssH || measureStage().h };
}

function fitViewFor(w, h) {
  const vw = Math.max(1, w);
  const vh = Math.max(1, h);
  const scale = viewport.fitScale(
    state.doc.width,
    state.doc.height,
    Math.max(32, vw - 64),
    Math.max(32, vh - 64)
  );
  const base = {
    scale,
    tx: (vw - state.doc.width * scale) / 2,
    ty: (vh - state.doc.height * scale) / 2,
    fitMode: 'fit',
  };
  // Clamp against the *target* viewport: at the moment a phase switch starts the
  // canvas still has the old size, and clamping against it would zero tx/ty.
  if (typeof viewport.clampPan === 'function') {
    return viewport.clampPan(base, state.doc, vw, vh);
  }
  return base;
}

function fitTarget() {
  const { w, h } = stageSize();
  const scale = viewport.fitScale(
    state.doc.width,
    state.doc.height,
    Math.max(32, w - 64),
    Math.max(32, h - 64)
  );
  return clampView({
    scale,
    tx: (w - state.doc.width * scale) / 2,
    ty: (h - state.doc.height * scale) / 2,
    fitMode: 'fit',
  });
}

export function fitView({ silent = false } = {}) {
  if (phaseTransitioning) return;
  if (!state.doc) return;
  state.view = fitTarget();
  if (!silent) emit('view');
  requestRender();
}

function zoomTo(scale, anchor) {
  if (!state.doc) return;
  stopViewTween();
  const { w, h } = stageSize();
  const ax = anchor?.x ?? w / 2;
  const ay = anchor?.y ?? h / 2;
  const min = currentMinScale();
  const target = Math.max(min, Math.min(viewport.MAX_SCALE || 16, scale));
  const v = viewport.zoomAt(state.view, ax, ay, target / state.view.scale);
  const clamped = clampView(v);
  state.view = {
    ...clamped,
    fitMode: target <= min + 1e-4 ? 'fit' : 'manual',
  };
  emit('view');
  requestRender();
}

/* ------------------------------------------------------------------ *
 * Document / folder lifecycle
 * ------------------------------------------------------------------ */

function dirOf(path) {
  const i = Math.max(path.lastIndexOf('/'), path.lastIndexOf('\\'));
  return i > 0 ? path.slice(0, i) : path;
}

export async function refreshFolder(dir) {
  try {
    const files = await bridge.invoke('list_images', { dir });
    state.files = Array.isArray(files) ? files : [];
  } catch (err) {
    state.files = [];
  }
  emit('files');
}

export async function openPath(
  path,
  { preserveFolder = false, preloaded = null, direction = 0 } = {}
) {
  if (!path) return;
  const hadDoc = !!state.doc;
  const fadingInPreview = hadDoc && state.phase === 'preview';
  const previousFrame = fadingInPreview ? snapshotBoard() : null;
  const switchDir = direction !== 0 ? Math.sign(direction) : 0;
  setBusy(true);
  if (els.stage && !fadingInPreview) els.stage.classList.add('is-loading');
  try {
    const doc =
      preloaded && preloaded.path === path ? preloaded : await loader.loadDocument(path);
    if (state.doc && state.doc !== doc) loader.disposeDoc?.(state.doc);
    state.doc = doc;
    // Kick off the folder listing without blocking the first paint — the
    // picture can show immediately and the sibling list lands a beat later.
    const folderPromise = preserveFolder
      ? Promise.resolve()
      : refreshFolder(doc.dir || dirOf(path));
    state.index = Math.max(0, state.files.findIndex((f) => f.path === path));
    if (!preserveFolder && state.files.length && state.index < 0) state.index = 0;
    resetHistory();
    state.crop = null;
    state.cropCommitted = false;
    state.selectedId = null;
    state.compare = false;
    if (!hadDoc) setPhaseState('preview');
    emit('doc');
    applyPhaseChrome();
    fitView({ silent: true });
    updateTitle();
    emit('view');
    emit('objects');
    actions.refreshPanels();
    if (previousFrame) {
      switchFade = {
        canvas: previousFrame,
        start: performance.now(),
        dir: switchDir,
      };
    }
    requestRender();
    requestAnimationFrame(() => els.stage && els.stage.classList.remove('is-loading'));
    await folderPromise;
    state.index = Math.max(0, state.files.findIndex((f) => f.path === path));
    emit('files');
  } catch (err) {
    console.error(err);
    switchFade = null;
    if (els.stage) els.stage.classList.remove('is-loading');
    actions.toast(err?.message || '無法開啟圖片', 'err');
  } finally {
    setBusy(false);
  }
}

export async function openBrowser(dir) {
  const mod = ui.browser;
  const start = dir || state.doc?.dir || (await bridge.invoke('home_pictures'));
  if (mod?.open) await mod.open(start);
  else if (mod?.openBrowser) await mod.openBrowser(start);
}

async function browseTo(dir) {
  const mod = ui.browser;
  if (mod?.browseTo) await mod.browseTo(dir);
  else if (mod?.open) await mod.open(dir);
}

function updateTitle() {
  const name = state.doc?.name || 'Lumen';
  const title = state.doc ? `${name}${state.dirty ? ' •' : ''} — Lumen` : 'Lumen';
  document.title = title;
  bridge.invoke('set_window_title', { title }).catch(() => {});
}

window.addEventListener('lumen:title', updateTitle);

function nextFile(delta) {
  if (!state.files.length) return;
  const i = state.index < 0 ? 0 : state.index;
  const n = (i + delta + state.files.length) % state.files.length;
  const f = state.files[n];
  if (f) openPath(f.path, { preserveFolder: true, direction: delta });
}

/* ------------------------------------------------------------------ *
 * Export / save
 * ------------------------------------------------------------------ */

function normalizeFormat(fmt, doc) {
  if (fmt && fmt !== 'auto') return fmt;
  const name = doc?.name || '';
  const ext = name.split('.').pop()?.toLowerCase();
  if (doc?.mime) {
    if (doc.mime.includes('png')) return 'png';
    if (doc.mime.includes('webp')) return 'webp';
    if (doc.mime.includes('jpeg')) return 'jpeg';
    if (doc.mime.includes('gif')) return 'png';
    if (doc.mime.includes('svg')) return 'png';
    if (doc.mime.includes('bmp')) return 'png';
    if (doc.mime.includes('tiff')) return 'png';
  }
  if (ext === 'jpg' || ext === 'jpeg') return 'jpeg';
  if (ext === 'webp') return 'webp';
  return 'png';
}

async function buildExport(overrides = {}) {
  const doc = state.doc;
  if (!doc) throw new Error('沒有開啟的圖片');
  const out = { ...state.output, ...overrides };
  const format = normalizeFormat(out.format, doc);
  const flat = await exporter.flatten(doc, state.objects, {
    crop: state.crop,
    maxDim: out.maxDim,
    scale: out.scale,
  });
  let encoded;
  if (out.targetKB) {
    encoded = await exporter.encodeToTarget(flat.canvas, format, out.targetKB);
  } else {
    encoded = await exporter.encode(flat.canvas, format, out.quality);
  }
  return {
    ...encoded,
    format,
    width: flat.width,
    height: flat.height,
    canvas: flat.canvas,
  };
}

async function writeBytes(path, bytes) {
  // Prefer base64 (small payload, no huge JSON arrays). Fall back to a plain
  // byte array when the shell is older than the base64 command.
  try {
    let bin = '';
    const chunk = 0x8000;
    for (let i = 0; i < bytes.length; i += chunk) {
      bin += String.fromCharCode.apply(null, bytes.subarray(i, i + chunk));
    }
    const b64 = btoa(bin);
    return await bridge.invoke('write_file_b64', { path, data: b64 });
  } catch (err) {
    return await bridge.invoke('write_file', { path, data: Array.from(bytes) });
  }
}

export async function save() {
  const doc = state.doc;
  if (!doc) return;
  const format = normalizeFormat(state.output.format, doc);
  const ext = exporter.outputExtension(format, doc);
  const currentExt = doc.path.split('.').pop()?.toLowerCase();
  const sameKind =
    (ext === 'jpg' && (currentExt === 'jpg' || currentExt === 'jpeg')) || ext === currentExt;
  if (!sameKind) {
    actions.toast('輸出格式與原檔不同，請使用「另存」', 'warn');
    return openSaveDialog();
  }
  setBusy(true);
  setSaveStatus('saving');
  try {
    const res = await buildExport();
    await writeBytes(doc.path, new Uint8Array(await res.blob.arrayBuffer()));
    await openPath(doc.path, { preserveFolder: true });
    markSaved();
    setSaveStatus('saved');
  } catch (err) {
    console.error(err);
    setSaveStatus('error');
    statusMessage(err?.message || '儲存失敗', 'err');
  } finally {
    setBusy(false);
  }
}

export async function saveAs(opts = {}) {
  const doc = state.doc;
  if (!doc) return;
  setBusy(true);
  setSaveStatus('saving');
  try {
    const res = await buildExport(opts);
    const path = opts.path || exporter.suggestedName(doc, res.format, doc.path);
    const bytes = new Uint8Array(await res.blob.arrayBuffer());
    await writeBytes(path, bytes);
    await openPath(path);
    markSaved();
    setSaveStatus('saved');
  } catch (err) {
    console.error(err);
    setSaveStatus('error');
    statusMessage(err?.message || '另存失敗', 'err');
  } finally {
    setBusy(false);
  }
}

async function openSaveDialog() {
  const doc = state.doc;
  if (!doc) return;
  const format = normalizeFormat(state.output.format, doc);
  const defaults = {
    path: exporter.suggestedName(doc, format, doc.path),
    format,
    quality: state.output.quality,
    maxDim: state.output.maxDim,
    targetKB: state.output.targetKB,
    bytes: 0,
    width: doc.width,
    height: doc.height,
  };
  const opener =
    ui.saveModal?.open ||
    ui.saveModal?.openSaveDialog ||
    window.__lumenUI?.openSaveModal ||
    window.__lumenUI?.openSaveDialog;
  if (typeof opener === 'function') {
    const choice = await opener(defaults);
    if (choice) await saveAs(choice);
  } else {
    actions.toast('另存對話框尚未就緒', 'warn');
  }
}

function fmtBytes(n) {
  if (n == null) return '—';
  if (n < 1024) return n + ' B';
  if (n < 1024 * 1024) return (n / 1024).toFixed(1) + ' KB';
  return (n / (1024 * 1024)).toFixed(2) + ' MB';
}

/* ------------------------------------------------------------------ *
 * UI modules
 * ------------------------------------------------------------------ */

const ui = {};

async function initUI() {
  const ctx = { state, actions, els, bridge, requestRender };
  const mods = [
    ['topbar', './ui/topbar.js'],
    ['toolrail', './ui/toolrail.js'],
    ['panels', './ui/panels.js'],
    ['filmstrip', './ui/filmstrip.js'],
    ['browser', './ui/browser.js'],
    ['saveModal', './ui/save-modal.js'],
    ['statusbar', './ui/statusbar.js'],
    ['saveStatus', './ui/save-status.js'],
    ['shortcuts', './ui/shortcuts.js'],
    ['textEditor', './ui/text-editor.js'],
    ['phase', './ui/phase.js'],
  ];
  const loaded = await Promise.all(
    mods.map(async ([key, spec]) => {
      try {
        return [key, await import(spec)];
      } catch (err) {
        console.warn(`[ui] ${key} 未載入：`, err);
        return null;
      }
    })
  );
  for (const item of loaded) {
    if (!item) continue;
    const [key, mod] = item;
    ui[key] = mod;
    if (typeof mod.init === 'function') mod.init(ctx);
  }
}

/* ------------------------------------------------------------------ *
 * Actions
 * ------------------------------------------------------------------ */

export const actions = {
  openPath,
  openBrowser,
  browseTo,
  openDroppedFiles(fileList) {
    const paths = [];
    for (const f of fileList || []) {
      const p = f.path || f.name;
      if (p) paths.push(p);
    }
    if (paths.length) openPath(paths[0]);
  },
  next: () => nextFile(1),
  prev: () => nextFile(-1),
  save,
  saveAs,
  openSaveDialog,
  undo() {
    if (undo()) {
      emit('objects');
      requestRender();
    }
  },
  redo() {
    if (redo()) {
      emit('objects');
      requestRender();
    }
  },
  setTool(tool) {
    const prev = state.tool;
    setTool(tool);
    // Crop frame: appear as soon as the tool is picked, disappear when leaving
    // the tool (unless the user pressed 套用裁切, which keeps it for export).
    if (tool === 'crop' && state.doc && !state.crop) {
      const d = state.doc;
      const w = Math.round(d.width * 0.8);
      const h = Math.round(d.height * 0.8);
      setCrop({
        x: Math.round((d.width - w) / 2),
        y: Math.round((d.height - h) / 2),
        w,
        h,
        ratio: null,
      });
    } else if (prev === 'crop' && tool !== 'crop' && state.crop && !state.cropCommitted) {
      setCrop(null);
    }
    requestRender();
  },
  setStyle(patch) {
    setStyle(patch);
    requestRender();
  },
  deleteSelection() {
    if (state.selectedId) {
      removeObject(state.selectedId);
      commit('刪除標註');
    } else if (state.crop && state.tool === 'crop') {
      setCrop(null);
      commit('清除裁切');
    }
    requestRender();
  },
  clearObjects() {
    if (!state.objects.length) return;
    clearObjects();
    commit('清除全部標註');
    requestRender();
  },
  clearSelection() {
    selectObject(null);
    requestRender();
  },
  setCrop(rect) {
    setCrop(rect);
    requestRender();
  },
  clearCrop() {
    setCrop(null);
    commit('清除裁切');
    requestRender();
  },
  setOutput(patch) {
    setOutput(patch);
    requestRender();
  },
  async estimateSize(overrides) {
    try {
      const doc = state.doc;
      if (!doc) return null;
      const out = { ...state.output, ...overrides };
      const format = normalizeFormat(out.format, doc);
      const res = await exporter.estimateSize(doc, state.objects, { ...out, format }, state.crop);
      return { ...res, format };
    } catch (err) {
      console.warn(err);
      return null;
    }
  },
  setPhase(phase) {
    if (!state.doc) {
      actions.toast('先開啟一張圖片', 'info');
      return;
    }
    setPhaseState(phase);
  },
  togglePhase() {
    if (!state.doc) return;
    setPhaseState(state.phase === 'preview' ? 'edit' : 'preview');
  },
  peekInfo(on) {
    if (!state.doc || state.phase !== 'preview') {
      if (on === false) setPeekInfo(false);
      return;
    }
    setPeekInfo(on === undefined ? !state.peekInfo : on);
  },
  zoomIn() {
    zoomTo(state.view.scale * 1.25);
  },
  zoomOut() {
    zoomTo(Math.max(currentMinScale(), state.view.scale / 1.25));
  },
  zoomFit() {
    fitView();
  },
  zoomActual() {
    zoomTo(1);
  },
  setZoom(s) {
    zoomTo(s);
  },
  toggleFilmstrip() {
    state.filmstrip = !state.filmstrip;
    document.body.classList.toggle('filmstrip-hidden', !state.filmstrip);
    emit('files');
    requestRender();
  },
  toggleCompare(on) {
    state.compare = on ?? !state.compare;
    emit('view');
    requestRender();
  },
  render,
  requestRender,
  /** Kept for older UI modules: routes messages to the status bar. */
  toast(msg, kind = 'info') {
    statusMessage(msg, kind);
  },
  refreshPanels() {
    emit('doc');
    emit('objects');
    emit('crop');
    emit('output');
    emit('view');
  },
};

/* ------------------------------------------------------------------ *
 * Keyboard
 * ------------------------------------------------------------------ */

const KEY_TOOLS = {
  v: 'move',
  b: 'brush',
  r: 'rect',
  o: 'ellipse',
  a: 'arrow',
  l: 'line',
  t: 'text',
  c: 'crop',
};

function isTypingTarget(t) {
  if (!t || !(t instanceof HTMLElement)) return false;
  return !!t.closest('input, textarea, select, [contenteditable="true"], [contenteditable=""]');
}

function onKeyDown(e) {
  const mod = e.ctrlKey || e.metaKey;
  if (isTypingTarget(e.target)) return;
  const modalOpen = document.body.classList.contains('modal-open');

  // Phase switching is intentionally button-only (right-top 「編輯」 / 「預覽」).
  if (state.doc && !modalOpen) {
    if (e.key === 'Home' || e.key === 'End') {
      e.preventDefault();
      const target = e.key === 'Home' ? 0 : state.files.length - 1;
      const file = state.files[target];
      if (file) openPath(file.path, { preserveFolder: true });
      return;
    }
  }

  if (mod) {
    switch (e.key.toLowerCase()) {
      case 'z':
        e.preventDefault();
        e.shiftKey ? actions.redo() : actions.undo();
        return;
      case 'y':
        e.preventDefault();
        actions.redo();
        return;
      case 's':
        e.preventDefault();
        e.shiftKey ? openSaveDialog() : save();
        return;
      case 'o':
        e.preventDefault();
        openBrowser();
        return;
      case '0':
        e.preventDefault();
        actions.zoomActual();
        return;
      case '=':
      case '+':
        e.preventDefault();
        actions.zoomIn();
        return;
      case '-':
        e.preventDefault();
        actions.zoomOut();
        return;
      default:
        return;
    }
  }

  switch (e.key) {
    case 'ArrowRight':
    case 'ArrowDown':
      e.preventDefault();
      nextFile(1);
      return;
    case 'ArrowLeft':
    case 'ArrowUp':
      e.preventDefault();
      nextFile(-1);
      return;
    case '+':
    case '=':
      actions.zoomIn();
      return;
    case '-':
      actions.zoomOut();
      return;
    case '0':
      actions.zoomFit();
      return;
    case 'Escape':
      if (document.body.classList.contains('modal-open')) return;
      if (state.phase === 'preview') {
        if (state.peekInfo) setPeekInfo(false);
        return;
      }
      if (state.selectedId) {
        state.selectedId = null;
        emit('objects');
        requestRender();
        return;
      }
      if (state.crop) {
        setCrop(null);
        commit('清除裁切');
        requestRender();
        return;
      }
      setPhaseState('preview');
      return;
    case 'Delete':
    case 'Backspace':
      if (!state.selectedId) return;
      e.preventDefault();
      actions.deleteSelection();
      return;
    case '?':
      (ui.shortcuts?.toggle || ui.shortcuts?.open || window.__lumenUI?.openShortcuts)?.();
      return;
    case 'i':
    case 'I':
      if (state.phase === 'preview') {
        setPeekInfo(!state.peekInfo);
        return;
      }
      document.getElementById('tab-info')?.click();
      return;
    case 'f':
    case 'F':
      if (document.fullscreenElement) document.exitFullscreen();
      else document.documentElement.requestFullscreen?.().catch(() => {});
      return;
    default:
      break;
  }

  if (state.phase !== 'edit') return;
  const tool = KEY_TOOLS[e.key.toLowerCase()];
  if (tool) {
    actions.setTool(tool);
  }
}

/* ------------------------------------------------------------------ *
 * Drag & drop
 * ------------------------------------------------------------------ */

function initDrop() {
  const stage = els.stage;
  if (!stage) return;
  stage.addEventListener('dragover', (e) => {
    e.preventDefault();
    stage.classList.add('drop-active');
  });
  stage.addEventListener('dragleave', () => stage.classList.remove('drop-active'));
  stage.addEventListener('drop', (e) => {
    e.preventDefault();
    stage.classList.remove('drop-active');
    const files = Array.from(e.dataTransfer?.files || []);
    if (files.length) actions.openDroppedFiles(files);
  });

  bridge.listen('tauri://drag-drop', (event) => {
    const paths = event?.payload?.paths || [];
    if (paths.length) openPath(paths[0]);
  });
}

/* ------------------------------------------------------------------ *
 * Boot
 * ------------------------------------------------------------------ */

async function boot() {
  // The shell reveals the window in setup() (already painted in the dark
  // background colour); no round-trip is needed here.
  applyPhaseChrome();
  syncCanvasSize();

  // Decode the startup image while the UI modules are still loading, so the
  // picture is ready the moment the shell is.
  const preloadPromise = bridge
    .invoke('startup_files')
    .catch(() => [])
    .then(async (files) => {
      if (!Array.isArray(files) || !files.length) return { files, doc: null };
      try {
        return { files, doc: await loader.loadDocument(files[0]) };
      } catch (err) {
        return { files, doc: null };
      }
    });

  const [, preload] = await Promise.all([initUI(), preloadPromise]);
  applyPhaseChrome();

  try {
    const interactionMod = await import('./core/interaction.js');
    interaction = interactionMod.attachInteraction({
      stage: els.stage,
      board: els.board,
      overlay: els.overlay,
      requestRender,
      onCursor: (c) => {
        if (els.stage) els.stage.style.cursor = c;
      },
      showTextEditor: ui.textEditor,
    });
  } catch (err) {
    console.error('[interaction] 未載入', err);
  }

  subscribe((scope) => {
    if (scope === 'phase' || scope === 'doc') applyPhaseChrome();
    if (scope === 'doc' || scope === 'view' || scope === 'objects' || scope === 'crop' || scope === 'style') {
      requestRender();
    }
    updateTitle();
  });

  window.addEventListener('lumen:enter-edit', () => {
    if (state.doc) actions.setPhase('edit');
  });

  window.addEventListener('keydown', onKeyDown);
  initDrop();

  if (window.ResizeObserver && els.stage) {
    new ResizeObserver(() => {
      syncCanvasSize();
      if (!state.doc) {
        render();
        return;
      }
      if (viewTween) {
        render();
        return;
      }
      const min = currentMinScale();
      if (state.view.fitMode === 'fit') {
        // Live: the picture keeps fitting the viewport while the window (or the
        // inspector splitter, or the phase transition) is animating.
        state.view = fitTarget();
        emit('view');
      } else if (state.view.scale < min - 1e-4) {
        state.view = { ...clampView({ ...state.view, scale: min }), fitMode: 'fit' };
        emit('view');
      }
      // Paint synchronously: resizing the canvas clears it, so waiting for the
      // next frame would flash.
      render();
    }).observe(els.stage);
  } else {
    window.addEventListener('resize', () => {
      syncCanvasSize();
      if (state.view.fitMode === 'fit') fitView({ silent: true });
      else requestRender();
    });
  }

  bridge.listen('lumen://open', (event) => {
    const p = event?.payload;
    if (typeof p === 'string' && p) openPath(p);
  });

  try {
    const startup = preload?.files || [];
    if (Array.isArray(startup) && startup.length) {
      // Opened from the file manager: land straight in the picture, never on
      // the editor shell.
      await openPath(startup[0], { preloaded: preload?.doc || null });
    } else {
      applyPhaseChrome();
      emit('doc');
      requestRender();
    }

    // Give the browser one short beat to paint the final layout before the
    // splash fades (setTimeout, not rAF: the window may still be hidden here).
    await new Promise((resolve) => setTimeout(resolve, 16));
  } finally {
    document.body.classList.remove('booting');
    document.body.classList.add('ready');
  }
}

boot();
