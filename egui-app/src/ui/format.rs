// Pure presentation helpers shared by the panels: byte/size formatting, aspect
// ratio labels, the crop ratio presets and the output-dimension maths. Kept in
// one testable place so inspector/preview/status agree.

use crate::i18n::Locale;

pub fn fmt_bytes(bytes: u64) -> String {
    let b = bytes as f64;
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", b / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.2} MB", b / (1024.0 * 1024.0))
    } else {
        format!("{:.2} GB", b / (1024.0 * 1024.0 * 1024.0))
    }
}

fn gcd(a: u32, b: u32) -> u32 {
    let (mut x, mut y) = (a, b);
    while y != 0 {
        let t = y;
        y = x % y;
        x = t;
    }
    x.max(1)
}

/// Human readable aspect ratio, mirroring `ratioLabel()` in src/js/ui/dom.js.
pub fn ratio_label(width: u32, height: u32) -> String {
    if width == 0 || height == 0 {
        return "—".to_string();
    }
    let d = gcd(width, height);
    let (w, h) = (width / d, height / d);
    if w > 40 || h > 40 {
        let r = width as f64 / height as f64;
        const COMMON: [(u32, u32); 8] =
            [(1, 1), (4, 3), (3, 2), (16, 9), (5, 4), (3, 4), (2, 3), (9, 16)];
        for (cw, ch) in COMMON {
            if (r - cw as f64 / ch as f64).abs() < 0.01 {
                return format!("{cw}:{ch}");
            }
        }
        return format!("{r:.2}:1");
    }
    format!("{w}:{h}")
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RatioPreset {
    Free,
    Original,
    R1x1,
    R4x3,
    R3x2,
    R16x9,
}

impl RatioPreset {
    pub const ALL: [RatioPreset; 6] = [
        RatioPreset::Free,
        RatioPreset::Original,
        RatioPreset::R1x1,
        RatioPreset::R4x3,
        RatioPreset::R3x2,
        RatioPreset::R16x9,
    ];

    /// The dictionary key (Traditional Chinese source string).
    pub fn label(self) -> &'static str {
        match self {
            RatioPreset::Free => "自由",
            RatioPreset::Original => "原始",
            RatioPreset::R1x1 => "1:1",
            RatioPreset::R4x3 => "4:3",
            RatioPreset::R3x2 => "3:2",
            RatioPreset::R16x9 => "16:9",
        }
    }

    pub fn label_localized(self, locale: Locale) -> String {
        locale.tr(self.label())
    }

    /// Locked numeric ratio, or `None` for free-form cropping.
    pub fn value(self, doc_w: u32, doc_h: u32) -> Option<f32> {
        match self {
            RatioPreset::Free => None,
            RatioPreset::Original => {
                if doc_w == 0 || doc_h == 0 {
                    None
                } else {
                    Some(doc_w as f32 / doc_h as f32)
                }
            }
            RatioPreset::R1x1 => Some(1.0),
            RatioPreset::R4x3 => Some(4.0 / 3.0),
            RatioPreset::R3x2 => Some(3.0 / 2.0),
            RatioPreset::R16x9 => Some(16.0 / 9.0),
        }
    }

    /// Which chip should look active for a crop carrying `ratio`.
    pub fn from_ratio(ratio: Option<f32>, doc_w: u32, doc_h: u32) -> Self {
        let Some(ratio) = ratio else { return RatioPreset::Free };
        if !ratio.is_finite() || ratio <= 0.0 {
            return RatioPreset::Free;
        }
        for preset in RatioPreset::ALL.into_iter().skip(1) {
            if let Some(target) = preset.value(doc_w, doc_h) {
                if (ratio - target).abs() < 0.012 {
                    return preset;
                }
            }
        }
        RatioPreset::Free
    }
}

/// Output dimensions after a long-edge cap (`max_dim`), 1px minimum.
pub fn output_dims(w: u32, h: u32, max_dim: Option<u32>) -> (u32, u32) {
    if w == 0 || h == 0 {
        return (0, 0);
    }
    match max_dim {
        Some(max) if max > 0 && w.max(h) > max => {
            let factor = max as f64 / w.max(h) as f64;
            (
                ((w as f64 * factor).round() as u32).max(1),
                ((h as f64 * factor).round() as u32).max(1),
            )
        }
        _ => (w, h),
    }
}

/// Format bytes for the status slot: "—" when unavailable.
pub fn format_estimate(bytes: Option<u64>, width: u32, height: u32) -> String {
    match bytes {
        Some(b) if b > 0 => format!("{} · {} × {}", fmt_bytes(b), width, height),
        _ => "—".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::Locale;

    #[test]
    fn bytes_format_matches_web_precision() {
        assert_eq!(fmt_bytes(0), "0 B");
        assert_eq!(fmt_bytes(1023), "1023 B");
        assert_eq!(fmt_bytes(1024), "1.0 KB");
        assert_eq!(fmt_bytes(1536), "1.5 KB");
        assert_eq!(fmt_bytes(1024 * 1024), "1.00 MB");
        assert_eq!(fmt_bytes(3 * 1024 * 1024 * 1024), "3.00 GB");
    }

    #[test]
    fn ratio_label_reduces_and_approximates() {
        assert_eq!(ratio_label(1920, 1080), "16:9");
        assert_eq!(ratio_label(800, 600), "4:3");
        assert_eq!(ratio_label(1000, 1000), "1:1");
        // Prime-ish dimensions fall back to an approximation.
        assert_eq!(ratio_label(4032, 3024), "4:3");
        assert_eq!(ratio_label(0, 10), "—");
    }

    #[test]
    fn ratio_presets_resolve_and_detect() {
        assert_eq!(RatioPreset::Free.value(100, 50), None);
        assert_eq!(RatioPreset::Original.value(100, 50), Some(2.0));
        assert_eq!(RatioPreset::R16x9.value(1, 1), Some(16.0 / 9.0));
        assert_eq!(RatioPreset::label_localized(RatioPreset::Free, Locale::ZhHant), "自由");
        assert_eq!(RatioPreset::label_localized(RatioPreset::R4x3, Locale::ZhHant), "4:3");
        assert_eq!(RatioPreset::label_localized(RatioPreset::Free, Locale::En), "Free");

        assert_eq!(RatioPreset::from_ratio(None, 1920, 1080), RatioPreset::Free);
        // A 16:9 document lights "原始" first (it matches the doc ratio).
        assert_eq!(
            RatioPreset::from_ratio(Some(16.0 / 9.0), 1920, 1080),
            RatioPreset::Original
        );
        assert_eq!(
            RatioPreset::from_ratio(Some(16.0 / 9.0), 1920, 1200),
            RatioPreset::R16x9
        );
        assert_eq!(
            RatioPreset::from_ratio(Some(4.0 / 3.0), 1920, 1200),
            RatioPreset::R4x3
        );
        // A hand-dragged ratio does not light up a chip.
        assert_eq!(RatioPreset::from_ratio(Some(2.71), 1920, 1080), RatioPreset::Free);
    }

    #[test]
    fn output_dims_caps_the_long_edge() {
        assert_eq!(output_dims(4000, 3000, None), (4000, 3000));
        assert_eq!(output_dims(4000, 3000, Some(2000)), (2000, 1500));
        assert_eq!(output_dims(3000, 4000, Some(2000)), (1500, 2000));
        // A cap larger than the image leaves it untouched.
        assert_eq!(output_dims(800, 600, Some(4000)), (800, 600));
        // Degenerate inputs never produce a zero edge.
        assert_eq!(output_dims(0, 600, Some(100)), (0, 0));
        assert_eq!(output_dims(10_000, 1, Some(100)), (100, 1));
    }

    #[test]
    fn estimate_label_falls_back_to_dash() {
        assert_eq!(format_estimate(None, 100, 50), "—");
        assert_eq!(format_estimate(Some(0), 100, 50), "—");
        assert_eq!(format_estimate(Some(2048), 100, 50), "2.0 KB · 100 × 50");
    }
}
