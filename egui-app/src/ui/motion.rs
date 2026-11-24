// Shared motion helpers for the chrome.
//
// Every chrome animation goes through `blend()` so that:
//   * it uses egui's animation manager (which requests a repaint only while
//     0 < t < 1, and never when idle),
//   * it has an ease-out comparable to the web build's
//     `cubic-bezier(.2,.8,.25,1)`,
//   * it honours the app-wide `Style::animation_time`: when that is 0 (the
//     reduced-motion switch) animations become instant, and a faster/slower
//     style scales the nominal duration while staying under `MAX_DURATION`.

use egui::{emath::easing, Color32, Context, Id};

/// Phase-change slide for the left rail / right inspector (web: 220–240 ms).
pub const PANEL: f32 = 0.24;
/// Style dock enter (web: 240 ms fade + rise).
pub const DOCK_IN: f32 = 0.24;
/// Style dock exit (web: 180 ms fade + drop).
pub const DOCK_OUT: f32 = 0.18;
/// Preview peek-card open/close (web: 200 ms fade + slide).
pub const PEEK: f32 = 0.20;

/// egui's default `Style::animation_time`; used to scale nominal durations.
const DEFAULT_STYLE_TIME: f32 = 0.083;
/// Hard ceiling for any chrome animation.
pub const MAX_DURATION: f32 = 0.26;

/// Scale a nominal duration by `Style::animation_time`.
///
/// `style_animation_time <= 0` means "no motion" (reduced motion), and any
/// positive value scales the duration proportionally to the style setting,
/// clamped to `[0.25, 3.0] x nominal` and never above `MAX_DURATION`.
pub fn scaled_duration(style_animation_time: f32, nominal: f32) -> f32 {
    if !(style_animation_time > 0.0) || !(nominal > 0.0) {
        return 0.0;
    }
    let scale = (style_animation_time / DEFAULT_STYLE_TIME).clamp(0.25, 3.0);
    let slow = std::env::var("LUMEN_MOTION_SLOW")
        .ok()
        .and_then(|v| v.parse::<f32>().ok())
        .map(|v| v.clamp(1.0, 10.0))
        .unwrap_or(1.0);
    (nominal * scale).min(MAX_DURATION) * slow
}

/// One-shot intro animation for elements that appear on the very first frame
/// (where `animate_bool` would snap straight to 1). Starts at 0 the first time
/// it is called for `id`, eases to 1 over `nominal`, and requests repaints only
/// while it is still progressing. Call `reset_intro` when the element goes away
/// so a later re-appearance animates again.
pub fn intro(ctx: &Context, id: Id, nominal: f32) -> f32 {
    easing::cubic_out(intro_raw(ctx, id, nominal))
}

/// Drop a stored intro start time (element was hidden / document loaded).
pub fn reset_intro(ctx: &Context, id: Id) {
    ctx.data_mut(|d| d.remove::<f64>(id));
}

/// Pure progress helper for [`intro`] (kept testable without a context).
pub fn intro_progress(start: f64, now: f64, duration: f32) -> f32 {
    if !(duration > 0.0) || !start.is_finite() || !now.is_finite() {
        return 1.0;
    }
    (((now - start) / duration as f64) as f32).clamp(0.0, 1.0)
}

#[derive(Clone, Copy)]
struct Presence {
    last: bool,
    start: f64,
}

/// Toggle-presence animation for pills/rings: returns `(progress, alpha)`
/// where `progress` is the linear 0..1 transition position (for pop curves) and
/// `alpha` fades in on activation / out on deactivation. Requests repaints only
/// while the transition is running, and starts steaedy (no animation on the
/// first frame a widget appears).
pub fn toggle(ctx: &Context, id: Id, active: bool, nominal: f32) -> (f32, f32) {
    let now = ctx.input(|i| i.time);
    let duration = scaled_duration(ctx.style().animation_time, nominal);
    let (progress, was_active) = ctx.data_mut(|d| {
        let p = d.get_temp_mut_or_insert_with(id, || Presence {
            last: active,
            start: now - duration as f64,
        });
        if p.last != active {
            p.last = active;
            p.start = now;
        }
        (intro_progress(p.start, now, duration), p.last)
    });
    if progress < 1.0 {
        ctx.request_repaint();
    }
    let alpha = ring_alpha(was_active, progress);
    (progress, alpha)
}

/// Raw (linear) intro progress. Same timing rules as [`intro`], but returns
/// the un-eased 0..1 value so callers can drive custom spring curves.
pub fn intro_raw(ctx: &Context, id: Id, nominal: f32) -> f32 {
    let now = ctx.input(|i| i.time);
    let start = ctx.data_mut(|d| *d.get_temp_mut_or_insert_with(id, move || now));
    let duration = scaled_duration(ctx.style().animation_time, nominal);
    let progress = intro_progress(start, now, duration);
    if progress < 1.0 {
        ctx.request_repaint();
    }
    progress
}

/// Press-scale factor for clickables: `pressed` while the pointer is held on
/// the widget, springing back over 120 ms. The web uses scale(.94)/(.97).
pub fn press_scale(
    ctx: &Context,
    id: Id,
    response: &egui::Response,
    pressed_scale: f32,
) -> f32 {
    let target = response.is_pointer_button_down_on();
    let t = ctx.animate_bool_with_time_and_easing(
        id,
        target,
        scaled_duration(ctx.style().animation_time, 0.12),
        easing::cubic_out,
    );
    press_factor(t, pressed_scale)
}

/// Pure press-scale interpolation (`t` = eased press amount).
pub fn press_factor(t: f32, pressed_scale: f32) -> f32 {
    1.0 + (pressed_scale - 1.0) * t.clamp(0.0, 1.0)
}

/// Ring "pop" curve used by the colour swatches: 0.8 -> 1.12 -> 1.0 over the
/// 0..1 activation progress (the web overshoots briefly at ~60%).
pub fn pop_scale(progress: f32) -> f32 {
    let p = progress.clamp(0.0, 1.0);
    if p < 0.6 {
        let t = easing::cubic_out(p / 0.6);
        0.8 + (1.12 - 0.8) * t
    } else {
        let t = easing::cubic_out((p - 0.6) / 0.4);
        1.12 + (1.0 - 1.12) * t
    }
}

/// Ring opacity: fades in when selected, fades out when deselected.
pub fn ring_alpha(active: bool, progress: f32) -> f32 {
    let t = easing::cubic_out(progress.clamp(0.0, 1.0));
    if active {
        t
    } else {
        1.0 - t
    }
}

/// Eased 0..1 value for a boolean target. repaints only while moving.
pub fn blend(ctx: &Context, id: Id, target: bool, nominal: f32) -> f32 {
    let duration = scaled_duration(ctx.style().animation_time, nominal);
    ctx.animate_bool_with_time_and_easing(id, target, duration, easing::cubic_out)
}

/// How far a sliding element is from its resting place (0 at t = 1).
pub fn slide_offset(t: f32, distance: f32) -> f32 {
    (1.0 - t.clamp(0.0, 1.0)) * distance
}

/// Fade a colour with the animation value (premultiplied-safe).
pub fn faded(color: Color32, t: f32) -> Color32 {
    color.gamma_multiply(t.clamp(0.0, 1.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_respect_reduced_motion_and_cap() {
        // Reduced motion -> instant.
        assert_eq!(scaled_duration(0.0, PANEL), 0.0);
        assert_eq!(scaled_duration(-1.0, PANEL), 0.0);
        assert_eq!(scaled_duration(f32::NAN, PANEL), 0.0);
        // Default style time -> the nominal duration.
        assert!((scaled_duration(DEFAULT_STYLE_TIME, PANEL) - PANEL).abs() < 1e-6);
        assert!((scaled_duration(DEFAULT_STYLE_TIME, DOCK_OUT) - DOCK_OUT).abs() < 1e-6);
        // Faster style settings shorten the animation.
        let fast = scaled_duration(DEFAULT_STYLE_TIME * 0.5, DOCK_IN);
        assert!(fast > 0.0 && fast < DOCK_IN);
        // Slower settings lengthen it, but everything stays <= MAX_DURATION.
        let slow = scaled_duration(DEFAULT_STYLE_TIME * 8.0, PANEL);
        assert!((slow - MAX_DURATION).abs() < 1e-6);
        for nominal in [PANEL, DOCK_IN, DOCK_OUT, PEEK] {
            for style in [0.02, 0.083, 0.25, 1.0] {
                assert!(scaled_duration(style, nominal) <= MAX_DURATION);
            }
        }
        // Every nominal chrome timing is within the requested 260 ms budget.
        for nominal in [PANEL, DOCK_IN, DOCK_OUT, PEEK] {
            assert!(nominal <= MAX_DURATION);
        }
    }

    #[test]
    fn slide_offset_clamps() {
        assert_eq!(slide_offset(1.0, 18.0), 0.0);
        assert_eq!(slide_offset(0.0, 18.0), 18.0);
        assert_eq!(slide_offset(0.5, 18.0), 9.0);
        assert_eq!(slide_offset(-1.0, 18.0), 18.0);
    }

    #[test]
    fn intro_progress_is_time_based_and_clamped() {
        // Duration 0 (reduced motion) -> instantly finished.
        assert_eq!(intro_progress(1.0, 1.0, 0.0), 1.0);
        assert_eq!(intro_progress(0.0, 0.0, 0.18), 0.0);
        assert!((intro_progress(0.0, 0.09, 0.18) - 0.5).abs() < 1e-6);
        assert_eq!(intro_progress(0.0, 0.5, 0.18), 1.0);
        // Clock going backwards / NaN never panics or overshoots.
        assert_eq!(intro_progress(1.0, 0.0, 0.18), 0.0);
        assert_eq!(intro_progress(f64::NAN, 1.0, 0.18), 1.0);
    }

    #[test]
    fn press_scale_and_pop_curve() {
        assert_eq!(press_factor(0.0, 0.94), 1.0);
        assert!((press_factor(1.0, 0.94) - 0.94).abs() < 1e-6);
        assert!((press_factor(0.5, 0.94) - 0.97).abs() < 1e-6);
        // clamped
        assert!((press_factor(2.0, 0.94) - 0.94).abs() < 1e-6);

        // ring pop: starts at 0.8, overshoots ~1.12, settles to 1.0
        assert!((pop_scale(0.0) - 0.8).abs() < 1e-6);
        assert!((pop_scale(0.6) - 1.12).abs() < 1e-6);
        assert!((pop_scale(1.0) - 1.0).abs() < 1e-6);
        assert!(pop_scale(0.6) > pop_scale(0.9));
        let mut prev = 0.8;
        for i in 0..=60 {
            let p = i as f32 / 60.0;
            let v = pop_scale(p);
            assert!(v >= 0.79 && v <= 1.121, "pop out of range at {p}: {v}");
            if p <= 0.6 {
                assert!(v + 1e-5 >= prev, "pop must rise until the overshoot");
            }
            prev = v;
        }

        // ring alpha
        assert_eq!(ring_alpha(true, 0.0), 0.0);
        assert_eq!(ring_alpha(true, 1.0), 1.0);
        assert_eq!(ring_alpha(false, 0.0), 1.0);
        assert_eq!(ring_alpha(false, 1.0), 0.0);
    }

    #[test]
    fn fade_alpha_is_bounded() {
        let c = Color32::from_rgb(200, 100, 50);
        assert_eq!(faded(c, 0.0), Color32::TRANSPARENT);
        assert_eq!(faded(c, 1.0), c);
        // Halfway: alpha halves, rgb is premultiplied so the result stays valid.
        let half = faded(c, 0.5);
        assert!(half.a() > 0 && half.a() < 255);
    }
}
