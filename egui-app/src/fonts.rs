// Font setup. The UI ships a 390-glyph subset of Noto Sans CJK; annotation text
// can contain arbitrary characters, so the full system collection is loaded
// lazily the first time the text tool is used.

use std::sync::atomic::{AtomicBool, Ordering};

use crate::i18n::Locale;

pub(crate) static CJK_TC: &[u8] = include_bytes!("../assets/NotoSansCJKtc-subset.otf");
pub(crate) static CJK_SC: &[u8] = include_bytes!("../assets/NotoSansCJKsc-subset.otf");
// Weight-600 companion for exported annotation text (the web canvas uses
// font-weight 600; the screen keeps the regular face for performance).
pub(crate) static CJK_TC_BOLD: &[u8] =
    include_bytes!("../assets/NotoSansCJKtc-Bold-subset.otf");
pub(crate) static CJK_SC_BOLD: &[u8] =
    include_bytes!("../assets/NotoSansCJKsc-Bold-subset.otf");

// Wide fallback faces: every hanzi encodable in GB2312/GBK/Big5, so arbitrary
// typed annotation text and file names never render as tofu. The small subsets
// above still win for UI strings (correct regional forms).
static CJK_FALLBACK: &[u8] = include_bytes!("../assets/NotoSansCJKfallback-regular.otf");
static CJK_FALLBACK_BOLD: &[u8] = include_bytes!("../assets/NotoSansCJKfallback-bold.otf");
static FULL_LOADED: AtomicBool = AtomicBool::new(false);

const SYSTEM_TTC: &str = "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc";

/// Weight-600 annotation family (web canvas uses font-weight 600).
pub const SEMIBOLD_FAMILY: &str = "cjk-semibold";

fn apply(ctx: &egui::Context, locale: Locale, full: Option<&'static [u8]>) {
    let mut fonts = egui::FontDefinitions::default();
    let subset: &'static [u8] = match locale {
        Locale::ZhHans => CJK_SC,
        _ => CJK_TC,
    };
    let bold: &'static [u8] = match locale {
        Locale::ZhHans => CJK_SC_BOLD,
        _ => CJK_TC_BOLD,
    };
    fonts.font_data.insert(
        "cjk".to_owned(),
        std::sync::Arc::new(egui::FontData::from_static(subset)),
    );
    if let Some(bytes) = full {
        fonts.font_data.insert(
            "cjk-full".to_owned(),
            std::sync::Arc::new(egui::FontData {
                font: bytes.into(),
                index: locale.cjk_index(),
                tweak: egui::FontTweak::default(),
            }),
        );
    }
    fonts.font_data.insert(
        "cjk-semibold".to_owned(),
        std::sync::Arc::new(egui::FontData::from_static(bold)),
    );
    fonts.font_data.insert(
        "cjk-fallback".to_owned(),
        std::sync::Arc::new(egui::FontData::from_static(CJK_FALLBACK)),
    );
    fonts.font_data.insert(
        "cjk-fallback-bold".to_owned(),
        std::sync::Arc::new(egui::FontData::from_static(CJK_FALLBACK_BOLD)),
    );
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        let list = fonts.families.entry(family).or_default();
        list.push("cjk".to_owned());
        list.push("cjk-fallback".to_owned());
        if full.is_some() {
            list.push("cjk-full".to_owned());
        }
    }
    let semibold = egui::FontFamily::Name(SEMIBOLD_FAMILY.into());
    let semibold_list = fonts.families.entry(semibold).or_default();
    semibold_list.push("cjk-semibold".to_owned());
    semibold_list.push("cjk-fallback-bold".to_owned());
    if full.is_some() {
        semibold_list.push("cjk-full".to_owned());
    }
    ctx.set_fonts(fonts);
}

pub fn install(ctx: &egui::Context, locale: Locale) {
    // Embedded subsets cover every UI string (including the Simplified Chinese
    // translations). The full system collection previously loaded here caused
    // crashes with the TTC face index under some drivers, so it is no longer
    // loaded at startup.
    apply(ctx, locale, None);
    FULL_LOADED.store(false, Ordering::Relaxed);
}

/// Kept for callers that used to trigger the lazy load; now a no-op because the
/// full collection is installed at startup.
pub fn ensure_annotation_font(_ctx: &egui::Context, _locale: Locale) {}


#[cfg(test)]
mod tests {
    use super::*;
    use ab_glyph::Font as _;

    fn covers(font: &'static [u8], text: &str) -> Vec<char> {
        let parsed = ab_glyph::FontRef::try_from_slice(font).expect("embedded font parses");
        text.chars()
            .filter(|c| !c.is_whitespace() && !c.is_control())
            .filter(|c| parsed.glyph_id(*c) == ab_glyph::GlyphId(0))
            .collect()
    }

    #[test]
    fn ui_charset_has_glyphs() {
        let chars = include_str!("../../scripts/i18n/subset-chars.txt");
        for font in [CJK_TC, CJK_SC, CJK_TC_BOLD, CJK_SC_BOLD] {
            let missing = covers(font, chars);
            assert!(missing.is_empty(), "missing UI glyphs: {:?}", missing);
        }
    }

    #[test]
    fn every_common_hanzi_has_a_fallback_glyph() {
        // typed annotation text / file names may be any common Chinese char
        let chars = include_str!("../../scripts/i18n/cjk-common.txt");
        for font in [CJK_FALLBACK, CJK_FALLBACK_BOLD] {
            let missing = covers(font, chars);
            if !missing.is_empty() {
                eprintln!(
                    "MISSING {} U+{:04X} {:?}",
                    missing.len(),
                    missing[0] as u32,
                    &missing[..missing.len().min(20)]
                );
            }
            assert!(
                missing.is_empty(),
                "fallback font misses {} chars, e.g. {:?}",
                missing.len(),
                &missing[..missing.len().min(12)]
            );
        }
    }
}
