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
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        let list = fonts.families.entry(family).or_default();
        list.push("cjk".to_owned());
        if full.is_some() {
            list.push("cjk-full".to_owned());
        }
    }
    let semibold = egui::FontFamily::Name(SEMIBOLD_FAMILY.into());
    let semibold_list = fonts.families.entry(semibold).or_default();
    semibold_list.push("cjk-semibold".to_owned());
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
