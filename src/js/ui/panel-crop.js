// 裁切分頁：長寬比預設、目前裁切尺寸、套用裁切（切到匯出流程）、重設。

import { subscribe } from '../core/state.js';
import { $, clamp, on } from './dom.js';

const RATIO_LABELS = ['free', 'orig', '1:1', '4:3', '3:2', '16:9'];

function ratioFor(label, doc) {
  if (!doc) return null;
  if (label === 'free') return null;
  if (label === 'orig') return doc.width / doc.height;
  const [a, b] = label.split(':').map(Number);
  return a && b ? a / b : null;
}

function centeredRect(doc, ratio) {
  const W = doc.width;
  const H = doc.height;
  if (!ratio) {
    return {
      x: Math.round(W * 0.1),
      y: Math.round(H * 0.1),
      w: Math.round(W * 0.8),
      h: Math.round(H * 0.8),
      ratio: null,
    };
  }
  let w = W * 0.8;
  let h = w / ratio;
  if (h > H * 0.8) {
    h = H * 0.8;
    w = h * ratio;
  }
  return {
    x: Math.round((W - w) / 2),
    y: Math.round((H - h) / 2),
    w: Math.round(w),
    h: Math.round(h),
    ratio,
  };
}

function matchesRatio(value, target) {
  if (!Number.isFinite(value) || !Number.isFinite(target)) return false;
  return Math.abs(value - target) < 0.012;
}

export function initCrop(ctx, helpers = {}) {
  const { state, actions } = ctx;
  const activateTab = helpers.activateTab || (() => {});

  const els = {
    ratios: $('crop-ratios'),
    size: $('crop-size'),
    apply: $('crop-apply'),
    reset: $('crop-reset'),
  };
  const chips = els.ratios ? Array.from(els.ratios.querySelectorAll('[data-ratio]')) : [];

  for (const chip of chips) {
    on(chip, 'click', () => {
      const doc = state.doc;
      if (!doc) return;
      const label = chip.dataset.ratio;
      const ratio = ratioFor(label, doc);
      const current = state.crop;
      if (!current) {
        actions.setCrop(centeredRect(doc, ratio));
      } else if (!ratio) {
        actions.setCrop({ ...current, ratio: null });
      } else {
        // Keep the crop centre, but rebuild w/h so it matches the new ratio.
        const cx = current.x + current.w / 2;
        const cy = current.y + current.h / 2;
        let w = Math.min(doc.width, Math.max(16, current.w));
        let h = w / ratio;
        if (h > doc.height) {
          h = doc.height;
          w = h * ratio;
        }
        const x = clamp(cx - w / 2, 0, doc.width - w);
        const y = clamp(cy - h / 2, 0, doc.height - h);
        actions.setCrop({
          x: Math.round(x),
          y: Math.round(y),
          w: Math.round(w),
          h: Math.round(h),
          ratio,
        });
      }
      if (state.tool !== 'crop') actions.setTool('crop');
    });
  }

  on(els.apply, 'click', () => {
    const doc = state.doc;
    if (!doc) return;
    const crop = state.crop;
    if (!crop) {
      actions.toast('請先在畫布上拖曳出裁切框', 'warn');
      return;
    }
    const x = Math.round(clamp(crop.x, 0, doc.width - 1));
    const y = Math.round(clamp(crop.y, 0, doc.height - 1));
    const w = Math.round(clamp(crop.w, 1, doc.width - x));
    const h = Math.round(clamp(crop.h, 1, doc.height - y));
    actions.setCrop({ x, y, w, h, ratio: crop.ratio ?? null });
    state.cropCommitted = true;
    activateTab('tab-adjust');
    actions.toast(`裁切已套用：${w} × ${h} px`, 'ok');
    window.setTimeout(() => $('adj-export')?.focus({ preventScroll: true }), 40);
  });

  on(els.reset, 'click', () => {
    if (!state.doc) return;
    actions.clearCrop();
    actions.toast('已重設裁切', 'info');
  });

  function render() {
    const doc = state.doc;
    const crop = state.crop;
    const has = !!doc;

    for (const chip of chips) chip.disabled = !has;
    if (els.apply) els.apply.disabled = !has;
    if (els.reset) els.reset.disabled = !has;

    if (els.size) {
      if (!doc) els.size.textContent = '—';
      else if (crop) {
        els.size.textContent = `${Math.round(crop.w)} × ${Math.round(crop.h)} px`;
        els.size.title = `位置 ${Math.round(crop.x)}, ${Math.round(crop.y)}`;
      } else {
        els.size.textContent = `整張 ${doc.width} × ${doc.height} px`;
        els.size.title = '目前未設定裁切框';
      }
    }

    let activeLabel = 'free';
    if (crop && Number.isFinite(crop.ratio)) {
      for (const label of RATIO_LABELS) {
        if (label === 'free') continue;
        const target = ratioFor(label, doc);
        if (target != null && matchesRatio(crop.ratio, target)) {
          activeLabel = label;
          break;
        }
      }
    }

    for (const chip of chips) {
      const active = chip.dataset.ratio === activeLabel;
      chip.classList.toggle('is-active', active);
      chip.setAttribute('aria-pressed', active ? 'true' : 'false');
    }
  }

  subscribe((scope) => {
    if (scope === 'all' || scope === 'doc' || scope === 'crop' || scope === 'tool') render();
  });

  render();
}
