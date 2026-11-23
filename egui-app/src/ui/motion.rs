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
    (nominal * scale).min(MAX_DURATION)
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
    fn fade_alpha_is_bounded() {
        let c = Color32::from_rgb(200, 100, 50);
        assert_eq!(faded(c, 0.0), Color32::TRANSPARENT);
        assert_eq!(faded(c, 1.0), c);
        // Halfway: alpha halves, rgb is premultiplied so the result stays valid.
        let half = faded(c, 0.5);
        assert!(half.a() > 0 && half.a() < 255);
    }
}
