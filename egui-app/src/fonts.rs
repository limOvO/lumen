// Font setup. The UI ships a 390-glyph subset of Noto Sans CJK; annotation text
// can contain arbitrary characters, so the full system collection is loaded
// lazily the first time the text tool is used.

use std::sync::atomic::{AtomicBool, Ordering};

use crate::i18n::Locale;

pub(crate) static CJK_TC: &[u8] = include_bytes!("../assets/NotoSansCJKtc-subset.otf");
pub(crate) static CJK_SC: &[u8] = include_bytes!("../assets/NotoSansCJKsc-subset.otf");
static FULL_LOADED: AtomicBool = AtomicBool::new(false);

const SYSTEM_TTC: &str = "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc";

fn apply(ctx: &egui::Context, locale: Locale, full: Option<&'static [u8]>) {
    let mut fonts = egui::FontDefinitions::default();
    let subset: &'static [u8] = match locale {
        Locale::ZhHans => CJK_SC,
        _ => CJK_TC,
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
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        let list = fonts.families.entry(family).or_default();
        list.push("cjk".to_owned());
        if full.is_some() {
            list.push("cjk-full".to_owned());
        }
    }
    ctx.set_fonts(fonts);
}

pub fn install(ctx: &egui::Context, locale: Locale) {
    apply(ctx, locale, None);
}

/// Load the full CJK collection once so arbitrary annotation text has glyphs.
pub fn ensure_annotation_font(ctx: &egui::Context, locale: Locale) {
    if FULL_LOADED.load(Ordering::Relaxed) {
        return;
    }
    let Ok(bytes) = std::fs::read(SYSTEM_TTC) else { return };
    let leaked: &'static [u8] = Box::leak(bytes.into_boxed_slice());
    apply(ctx, locale, Some(leaked));
    FULL_LOADED.store(true, Ordering::Relaxed);
}
