// 行內文字輸入 overlay（#textEditor）。interaction 以 canvas 座標呼叫 show()，
// Enter／失焦完成，Esc 取消；完成時回呼 onCommit(text)，取消回呼 onCancel()。

let editor = null;
let active = false;
let commitCb = null;
let cancelCb = null;

function finish(commit) {
  if (!active) return;
  const raw = editor ? editor.innerText || '' : '';
  const text = raw.replace(/\r/g, '').replace(/\n+$/, '');
  const onCommit = commitCb;
  const onCancel = cancelCb;
  hide();
  if (commit) {
    if (text.trim()) onCommit?.(text.trim());
    else onCancel?.();
  } else {
    onCancel?.();
  }
}

function onKeyDown(event) {
  if (!active) return;
  if (event.key === 'Escape') {
    event.preventDefault();
    event.stopPropagation();
    finish(false);
    return;
  }
  if (event.key === 'Enter' && !event.shiftKey) {
    event.preventDefault();
    event.stopPropagation();
    finish(true);
  }
}

function onBlur() {
  if (active) finish(true);
}

function onPaste(event) {
  if (!active || !editor) return;
  event.preventDefault();
  const text = (event.clipboardData || window.clipboardData)?.getData('text/plain') || '';
  document.execCommand('insertText', false, text);
}

export function init(ctx) {
  editor = document.getElementById('textEditor');
  if (!editor) return;
  editor.hidden = true;
  editor.addEventListener('keydown', onKeyDown);
  editor.addEventListener('blur', onBlur);
  editor.addEventListener('paste', onPaste);
  window.__lumenUI = Object.assign(window.__lumenUI || {}, {
    textEditor: { show, hide, isActive },
  });
}

/**
 * @param {{screenX?:number, screenY?:number, fontSize?:number, color?:string,
 *          text?:string, onCommit?:(text:string)=>void, onCancel?:()=>void}} options
 */
export function show(options = {}) {
  if (!editor) editor = document.getElementById('textEditor');
  if (!editor) return;

  // interaction 傳入的是 client 座標（pointer event 的 clientX/clientY）。
  const fontPx = Math.max(8, Number(options.fontSize) || 36);
  const boxW = 160;
  const boxH = Math.max(28, Math.round(fontPx * 1.6));
  const left = Math.max(0, Math.min(Number(options.screenX) || 0, window.innerWidth - boxW - 8));
  const top = Math.max(0, Math.min(Number(options.screenY) || 0, window.innerHeight - boxH - 8));

  editor.style.left = `${Math.round(left)}px`;
  editor.style.top = `${Math.round(top)}px`;
  editor.style.fontSize = `${fontPx}px`;
  editor.style.color = options.color || '#ff5c5c';
  editor.textContent = String(options.text || '');
  editor.hidden = false;

  commitCb = typeof options.onCommit === 'function' ? options.onCommit : null;
  cancelCb = typeof options.onCancel === 'function' ? options.onCancel : null;
  active = true;

  editor.focus({ preventScroll: true });
  const range = document.createRange();
  range.selectNodeContents(editor);
  const selection = window.getSelection();
  if (selection) {
    selection.removeAllRanges();
    selection.addRange(range);
  }
}

export function hide() {
  active = false;
  commitCb = null;
  cancelCb = null;
  if (!editor) return;
  editor.hidden = true;
  editor.textContent = '';
  if (typeof editor.blur === 'function') editor.blur();
}

export function isActive() {
  return active;
}

export function value() {
  return editor ? editor.innerText || '' : '';
}
