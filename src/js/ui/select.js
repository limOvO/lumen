// 自訂下拉選擇面板：觸發按鈕 + 浮層 listbox + 鍵盤操作。
// 遵循設計知識庫「下拉選擇面板」：SVG 箭頭、選中對勾、點外側/Esc 關閉。

const CARET = `<svg class="select-caret" width="12" height="12" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M3 4.6 6 7.4 9 4.6"></path></svg>`;

const TICK = `<svg class="select-tick" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M5.5 12.5l4.2 4.1 8.8-9.6"></path></svg>`;

/**
 * @param {HTMLElement} host 掛載點（.select-host）
 * @param {{options:{value:string,label:string}[], value?:string, onChange?:(v:string)=>void, ariaLabel?:string}} config
 */
export function createSelect(host, config = {}) {
  if (!host) return null;
  const options = Array.isArray(config.options) ? config.options.slice() : [];
  host.classList.add('select-host');
  host.textContent = '';

  const trigger = document.createElement('button');
  trigger.type = 'button';
  trigger.className = 'select-trigger';
  trigger.setAttribute('aria-haspopup', 'listbox');
  trigger.setAttribute('aria-expanded', 'false');
  if (config.ariaLabel) trigger.setAttribute('aria-label', config.ariaLabel);
  trigger.innerHTML = `<span class="select-value"></span>${CARET}`;
  const valueEl = trigger.querySelector('.select-value');
  host.append(trigger);

  const panel = document.createElement('div');
  panel.className = 'select-panel';
  panel.setAttribute('role', 'listbox');
  panel.hidden = true;
  if (config.ariaLabel) panel.setAttribute('aria-label', config.ariaLabel);
  document.body.append(panel);

  let current = config.value ?? options[0]?.value ?? null;
  let open = false;

  const labelOf = (value) => options.find((item) => item.value === value)?.label ?? '';

  function syncTrigger() {
    const label = labelOf(current);
    valueEl.textContent = label || '—';
    trigger.title = label;
  }

  function buildOptions() {
    panel.textContent = '';
    for (const option of options) {
      const row = document.createElement('button');
      row.type = 'button';
      row.className = 'select-option';
      row.setAttribute('role', 'option');
      row.setAttribute('aria-selected', option.value === current ? 'true' : 'false');
      const text = document.createElement('span');
      text.textContent = option.label;
      row.append(text);
      row.insertAdjacentHTML('beforeend', TICK);
      row.addEventListener('click', () => choose(option.value, true));
      panel.append(row);
    }
  }

  function choose(value, fromPointer = false) {
    const changed = value !== current;
    current = value;
    syncTrigger();
    close(fromPointer);
    if (changed && typeof config.onChange === 'function') config.onChange(value);
  }

  function position() {
    panel.style.left = '0px';
    panel.style.top = '0px';
    const rect = trigger.getBoundingClientRect();
    panel.style.minWidth = `${Math.max(168, Math.round(rect.width))}px`;
    const width = panel.offsetWidth;
    const height = panel.offsetHeight;
    let left = Math.min(rect.left, window.innerWidth - width - 12);
    left = Math.max(8, left);
    let top = rect.bottom + 6;
    if (top + height > window.innerHeight - 8) {
      top = Math.max(8, rect.top - height - 6);
    }
    panel.style.left = `${Math.round(left)}px`;
    panel.style.top = `${Math.round(top)}px`;
  }

  function onDocPointerDown(event) {
    if (!open) return;
    if (panel.contains(event.target) || trigger.contains(event.target)) return;
    close(false);
  }

  function onViewportChange() {
    if (open) close(false);
  }

  function optionRows() {
    return Array.from(panel.querySelectorAll('.select-option'));
  }

  function openPanel() {
    if (open || !options.length) return;
    open = true;
    buildOptions();
    panel.hidden = false;
    trigger.setAttribute('aria-expanded', 'true');
    position();
    const selected = panel.querySelector('[aria-selected="true"]');
    (selected || panel.firstElementChild)?.focus({ preventScroll: true });
    document.addEventListener('pointerdown', onDocPointerDown, true);
    window.addEventListener('resize', onViewportChange, true);
    window.addEventListener('scroll', onViewportChange, true);
  }

  function close(restoreFocus = false) {
    if (!open) return;
    open = false;
    panel.hidden = true;
    trigger.setAttribute('aria-expanded', 'false');
    document.removeEventListener('pointerdown', onDocPointerDown, true);
    window.removeEventListener('resize', onViewportChange, true);
    window.removeEventListener('scroll', onViewportChange, true);
    if (restoreFocus) trigger.focus({ preventScroll: true });
  }

  trigger.addEventListener('click', () => {
    if (open) close(true);
    else openPanel();
  });

  trigger.addEventListener('keydown', (event) => {
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp' || event.key === 'Enter' || event.key === ' ') {
      event.preventDefault();
      openPanel();
    }
  });

  panel.addEventListener('keydown', (event) => {
    const rows = optionRows();
    const index = rows.indexOf(document.activeElement);
    if (event.key === 'Escape') {
      event.preventDefault();
      close(true);
      return;
    }
    if (event.key === 'Tab') {
      close(false);
      return;
    }
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault();
      const delta = event.key === 'ArrowDown' ? 1 : -1;
      const next = (index + delta + rows.length) % rows.length;
      rows[next]?.focus({ preventScroll: true });
      return;
    }
    if (event.key === 'Home' || event.key === 'End') {
      event.preventDefault();
      (event.key === 'Home' ? rows[0] : rows[rows.length - 1])?.focus({ preventScroll: true });
      return;
    }
    if (event.key === 'Enter' || event.key === ' ') {
      event.preventDefault();
      const row = rows[index];
      if (row) {
        const selected = options[index];
        if (selected) choose(selected.value, true);
      }
    }
  });

  syncTrigger();

  return {
    get value() {
      return current;
    },
    setValue(value, { emit = false } = {}) {
      current = value;
      syncTrigger();
      if (emit && typeof config.onChange === 'function') config.onChange(value);
    },
    open: openPanel,
    close,
    setDisabled(disabled) {
      trigger.disabled = !!disabled;
      if (disabled) close(false);
    },
    focus() {
      trigger.focus({ preventScroll: true });
    },
    destroy() {
      close(false);
      panel.remove();
      host.textContent = '';
    },
  };
}
