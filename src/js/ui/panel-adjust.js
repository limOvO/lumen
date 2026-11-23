// 調整分頁：尺寸／比例鎖、百分比、長邊上限、格式、品質、目標大小、估算、匯出。

import { subscribe } from '../core/state.js';
import { $, debounce, fmtBytes, on } from './dom.js';
import { createSelect } from './select.js';

function effectiveFormat(output, doc) {
  if (output.format && output.format !== 'auto') return output.format;
  const mime = doc?.mime || '';
  if (mime.includes('png')) return 'png';
  if (mime.includes('webp')) return 'webp';
  if (mime.includes('jpeg')) return 'jpeg';
  const ext = (doc?.name || '').split('.').pop()?.toLowerCase();
  if (ext === 'png') return 'png';
  if (ext === 'webp') return 'webp';
  if (ext === 'jpg' || ext === 'jpeg') return 'jpeg';
  return 'png';
}

function baseDims(state) {
  const crop = state.crop;
  if (crop && crop.w > 0 && crop.h > 0) return { w: crop.w, h: crop.h };
  if (state.doc && state.doc.width > 0 && state.doc.height > 0) {
    return { w: state.doc.width, h: state.doc.height };
  }
  return null;
}

function outputDims(base, maxDim, extraScale) {
  if (!base) return null;
  const extra = Number(extraScale) > 0 ? Number(extraScale) : 1;
  let w = base.w * extra;
  let h = base.h * extra;
  const long = Math.max(w, h);
  if (maxDim > 0 && long > maxDim) {
    const factor = maxDim / long;
    w *= factor;
    h *= factor;
  }
  return { w: Math.max(1, Math.round(w)), h: Math.max(1, Math.round(h)) };
}

export function initAdjust(ctx) {
  const { state, actions } = ctx;
  const els = {
    width: $('adj-width'),
    height: $('adj-height'),
    lock: $('adj-lock'),
    percents: $('adj-percents'),
    maxDim: $('adj-maxdim'),
    maxDimClear: $('adj-maxdim-clear'),
    format: $('adj-format'),
    quality: $('adj-quality'),
    qualityOut: $('adj-quality-out'),
    qualityHint: $('adj-quality-hint'),
    target: $('adj-target'),
    targetHint: $('adj-target-hint'),
    estimate: $('adj-estimate'),
    estimateOut: $('adj-estimate-out'),
    exportBtn: $('adj-export'),
  };

  let lockRatio = true;
  let estimating = false;

  const formatSelect = createSelect(els.format, {
    ariaLabel: '輸出格式',
    options: [
      { value: 'auto', label: '原格式' },
      { value: 'jpeg', label: 'JPEG' },
      { value: 'png', label: 'PNG' },
      { value: 'webp', label: 'WebP' },
    ],
    value: state.output.format || 'auto',
    onChange: (value) => actions.setOutput({ format: value }),
  });

  on(els.lock, 'click', () => {
    lockRatio = !lockRatio;
    els.lock.classList.toggle('is-on', lockRatio);
    els.lock.setAttribute('aria-pressed', lockRatio ? 'true' : 'false');
    els.lock.title = lockRatio ? '已鎖定長寬比' : '已解除長寬比鎖定';
  });

  function applyDimension(which, raw) {
    const value = Math.round(Number(raw));
    if (!Number.isFinite(value) || value <= 0) return;
    const base = baseDims(state);
    if (!base) return;
    const baseSide = which === 'w' ? base.w : base.h;
    const long = Math.max(base.w, base.h);
    const scale = value / baseSide;
    const targetLong = long * scale;
    if (targetLong > long) {
      // 放大：export.flatten 支援 opts.scale（main.js 會帶入），用倍率而不是上限。
      actions.setOutput({ maxDim: null, scale });
    } else {
      actions.setOutput({ maxDim: Math.max(1, Math.round(targetLong)), scale: null });
    }
    const out = outputDims(base, state.output.maxDim, state.output.scale);
    if (!out) return;
    if (lockRatio) {
      if (which === 'w' && document.activeElement !== els.height) els.height.value = String(out.h);
      if (which === 'h' && document.activeElement !== els.width) els.width.value = String(out.w);
    }
  }

  on(els.width, 'input', () => applyDimension('w', els.width.value));
  on(els.height, 'input', () => applyDimension('h', els.height.value));
  on(els.width, 'blur', () => render());
  on(els.height, 'blur', () => render());

  if (els.percents) {
    const chips = Array.from(els.percents.querySelectorAll('[data-pct]'));
    for (const chip of chips) {
      chip.addEventListener('click', () => {
        const base = baseDims(state);
        if (!base) return;
        const pct = Number(chip.dataset.pct);
        if (!Number.isFinite(pct) || pct <= 0) return;
        if (pct === 100) {
          actions.setOutput({ maxDim: null, scale: null });
          return;
        }
        const long = Math.max(base.w, base.h);
        if (pct > 100) {
          actions.setOutput({ maxDim: null, scale: pct / 100 });
        } else {
          actions.setOutput({ maxDim: Math.max(1, Math.round((long * pct) / 100)), scale: null });
        }
      });
    }
  }

  on(els.maxDim, 'input', () => {
    const value = Math.round(Number(els.maxDim.value));
    actions.setOutput({ maxDim: Number.isFinite(value) && value > 0 ? value : null, scale: null });
  });

  on(els.maxDimClear, 'click', () => actions.setOutput({ maxDim: null }));

  on(els.quality, 'input', () => {
    const value = Math.min(100, Math.max(1, Number(els.quality.value)));
    actions.setOutput({ quality: value / 100 });
  });

  on(els.target, 'input', () => {
    const value = Math.round(Number(els.target.value));
    actions.setOutput({ targetKB: Number.isFinite(value) && value > 0 ? value : null });
  });

  // 「估算」按下後做一次全尺寸估算；400ms debounce 避免連續點擊。
  const runEstimate = debounce(async () => {
    if (!state.doc || estimating) return;
    estimating = true;
    if (els.estimate) els.estimate.disabled = true;
    if (els.estimateOut) els.estimateOut.textContent = '估算中…';
    try {
      const result = await actions.estimateSize();
      if (els.estimateOut) {
        if (result && result.bytes != null) {
          els.estimateOut.textContent = `${fmtBytes(result.bytes)} · ${result.width} × ${result.height}`;
          els.estimateOut.title = `${result.bytes.toLocaleString('zh-TW')} bytes`;
        } else {
          els.estimateOut.textContent = '無法估算';
          els.estimateOut.title = '';
        }
      }
    } finally {
      estimating = false;
      render();
    }
  }, 400);

  on(els.estimate, 'click', runEstimate);

  on(els.exportBtn, 'click', () => {
    if (typeof actions.openSaveDialog === 'function') actions.openSaveDialog();
    else window.__lumenUI?.openSaveModal?.();
  });

  const render = () => {
    const doc = state.doc;
    const base = baseDims(state);
    const has = !!doc && !!base;
    const out = has ? outputDims(base, state.output.maxDim, state.output.scale) : null;
    const format = effectiveFormat(state.output, doc);

    for (const control of [els.width, els.height, els.lock, els.maxDim, els.maxDimClear, els.quality, els.target]) {
      if (control) control.disabled = !has;
    }
    if (els.percents) {
      for (const chip of els.percents.querySelectorAll('[data-pct]')) chip.disabled = !has;
    }

    if (out) {
      if (document.activeElement !== els.width) els.width.value = String(out.w);
      if (document.activeElement !== els.height) els.height.value = String(out.h);
    } else {
      els.width.value = '';
      els.height.value = '';
    }

    if (els.maxDim && document.activeElement !== els.maxDim) {
      els.maxDim.value = state.output.maxDim ? String(state.output.maxDim) : '';
    }

    const pct = out && base ? Math.round((Math.max(out.w, out.h) / Math.max(base.w, base.h)) * 100) : null;
    if (els.percents) {
      for (const chip of els.percents.querySelectorAll('[data-pct]')) {
        const value = Number(chip.dataset.pct);
        const active = pct != null && (value === 100 ? pct === 100 : Math.abs(pct - value) <= 1);
        chip.classList.toggle('is-active', active);
      }
    }

    const qualityPct = Math.round((state.output.quality ?? 0.85) * 100);
    if (els.quality) els.quality.value = String(qualityPct);
    if (els.qualityOut) els.qualityOut.textContent = String(qualityPct);
    if (els.quality) {
      els.quality.disabled = !has || format === 'png' || !!state.output.targetKB;
    }
    if (els.qualityHint) els.qualityHint.hidden = !(has && format === 'png');

    if (els.target && document.activeElement !== els.target) {
      els.target.value = state.output.targetKB ? String(state.output.targetKB) : '';
    }
    if (els.targetHint) els.targetHint.hidden = !state.output.targetKB;

    formatSelect?.setValue(state.output.format || 'auto');
    formatSelect?.setDisabled(!has);

    if (els.exportBtn) els.exportBtn.disabled = !has || state.busy;
    if (els.estimate) els.estimate.disabled = !has || estimating || state.busy;
  };

  subscribe((scope) => {
    if (scope === 'all' || scope === 'doc' || scope === 'crop' || scope === 'output' || scope === 'busy') {
      render();
    }
    if ((scope === 'doc' || scope === 'crop' || scope === 'output') && !estimating && els.estimateOut) {
      els.estimateOut.textContent = '—';
      els.estimateOut.title = '';
    }
  });

  render();
}
