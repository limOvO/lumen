// Central application state with a compact undo/redo history.
// Every mutation goes through a helper so the renderer and panels stay in
// sync; nothing else in the app mutates `state` fields directly.

export const state = {
  doc: null,
  files: [],
  index: -1,
  tool: 'move',
  style: { color: '#ff5c5c', size: 8, opacity: 1, fill: false, dashed: false, fontSize: 36 },
  objects: [],
  selectedId: null,
  crop: null,
  cropCommitted: false,
  view: { scale: 1, tx: 0, ty: 0, fitMode: 'fit' },
  output: { format: 'auto', quality: 0.85, maxDim: null, targetKB: null },
  dirty: false,
  busy: false,
  compare: false,
  filmstrip: true,
  /** 'preview' = 乾淨看圖；'edit' = 工具列/面板/底片條全開 */
  phase: 'preview',
  /** 預覽階段用：以浮動卡片顯示資訊 */
  peekInfo: false,
};

const listeners = new Set();
const MAX_HISTORY = 80;

/** @type {{objects:any[], crop:any, selectedId:string|null, label:string}[]} */
const undoStack = [];
const redoStack = [];
/** Snapshot of the state as of the last commit — undo restores THIS, not the
 *  post-mutation snapshot (otherwise the first undo looks like a no-op). */
let baseline = null;

function snapshot(label) {
  return {
    objects: structuredClone(state.objects),
    crop: state.crop ? { ...state.crop } : null,
    selectedId: state.selectedId,
    label,
  };
}

function restore(entry) {
  state.objects = structuredClone(entry.objects);
  state.crop = entry.crop ? { ...entry.crop } : null;
  state.selectedId = entry.selectedId ?? null;
}

export function subscribe(fn) {
  listeners.add(fn);
  return () => listeners.delete(fn);
}

let emitScheduled = null;
export function emit(scope = 'all') {
  for (const fn of listeners) {
    try {
      fn(scope);
    } catch (err) {
      console.error('[state] listener failed', err);
    }
  }
  if (scope === 'objects' || scope === 'crop' || scope === 'all') {
    state.dirty = state.objects.length > 0 || state.crop !== null;
  }
  // Keep the native title in step with the dirty flag.
  if (emitScheduled) cancelAnimationFrame(emitScheduled);
  emitScheduled = requestAnimationFrame(() => {
    emitScheduled = null;
    window.dispatchEvent(new CustomEvent('lumen:title'));
  });
}

export function commit(label = '編輯') {
  if (!baseline) baseline = snapshot('初始');
  // The state *before* this operation becomes the undo entry.
  undoStack.push(baseline);
  if (undoStack.length > MAX_HISTORY) undoStack.shift();
  baseline = snapshot(label);
  redoStack.length = 0;
  state.dirty = true;
  emit('objects');
  emit('undo');
}

export function undo() {
  if (!undoStack.length) return false;
  redoStack.push(baseline || snapshot('重做'));
  baseline = undoStack.pop();
  restore(baseline);
  state.dirty = undoStack.length > 0 || state.objects.length > 0;
  emit('objects');
  emit('crop');
  emit('undo');
  return true;
}

export function redo() {
  if (!redoStack.length) return false;
  undoStack.push(baseline || snapshot('復原'));
  baseline = redoStack.pop();
  restore(baseline);
  emit('objects');
  emit('crop');
  emit('undo');
  return true;
}

export function canUndo() {
  return undoStack.length > 0;
}
export function canRedo() {
  return redoStack.length > 0;
}
export function historyDepth() {
  return undoStack.length;
}

/** Clear history when a new document is opened. */
export function resetHistory() {
  undoStack.length = 0;
  redoStack.length = 0;
  state.objects = [];
  state.crop = null;
  state.selectedId = null;
  state.dirty = false;
  baseline = snapshot('初始');
  emit('objects');
  emit('crop');
  emit('undo');
}

export function setPhase(phase) {
  const next = phase === 'edit' ? 'edit' : 'preview';
  const changed = state.phase !== next;
  state.phase = next;
  if (next === 'preview') {
    state.selectedId = null;
    state.peekInfo = false;
  } else {
    state.peekInfo = false;
  }
  if (changed) emit('phase');
}

export function setPeekInfo(on) {
  const next = !!on;
  if (state.peekInfo === next) return;
  state.peekInfo = next;
  emit('phase');
}

export function setTool(tool) {
  if (state.tool === tool) return;
  state.tool = tool;
  if (tool !== 'crop') state.selectedId = tool === 'move' ? state.selectedId : null;
  emit('tool');
}

export function setStyle(patch) {
  Object.assign(state.style, patch);
  emit('style');
}

export function addObject(obj) {
  state.objects.push(obj);
  emit('objects');
}

export function updateObject(obj) {
  const i = state.objects.findIndex((o) => o.id === obj.id);
  if (i >= 0) {
    state.objects[i] = obj;
    emit('objects');
  }
}

export function removeObject(id) {
  const i = state.objects.findIndex((o) => o.id === id);
  if (i < 0) return;
  state.objects.splice(i, 1);
  if (state.selectedId === id) state.selectedId = null;
  emit('objects');
}

export function clearObjects() {
  if (!state.objects.length) return;
  state.objects = [];
  state.selectedId = null;
  emit('objects');
}

export function selectObject(id) {
  if (state.selectedId === id) return;
  state.selectedId = id;
  emit('objects');
}

export function setView(patch) {
  Object.assign(state.view, patch);
  emit('view');
}

export function setCrop(rect) {
  state.crop = rect;
  if (!rect) state.cropCommitted = false;
  emit('crop');
}

export function setOutput(patch) {
  Object.assign(state.output, patch);
  emit('output');
}

export function setDirty(v) {
  state.dirty = v;
  emit('dirty');
}

export function markSaved() {
  state.dirty = false;
  undoStack.length = 0;
  redoStack.length = 0;
  baseline = snapshot('已儲存');
  emit('undo');
  emit('dirty');
}

export function setBusy(v) {
  state.busy = v;
  emit('busy');
}
