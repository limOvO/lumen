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

// Arbitrary typed text / file names are covered by the system Noto CJK
// collection at runtime (fc-list shows it on Linux desktops); the tiny embedded
// subsets cover the UI itself and keep the binary small.
static FULL_LOADED: AtomicBool = AtomicBool::new(false);

const SYSTEM_TTC: &str = "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc";

/// Candidate system faces, in preference order (Noto first, then distro CJK
/// fallbacks). TTC files need the locale's regional index; plain OTF/TTF are 0.
const SYSTEM_FACES: &[&str] = &[
    "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/noto-cjk/NotoSansCJKsc-Regular.otf",
    "/usr/share/fonts/noto-cjk/NotoSansCJKtc-Regular.otf",
    "/usr/share/fonts/noto-cjk/NotoSansCJK-Medium.ttc",
    "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc",
    "/usr/share/fonts/truetype/droid/DroidSansFallbackFull.ttf",
];

/// Ask fontconfig for a CJK face (works on any distro with a CJK font).
fn fontconfig_face() -> Option<std::path::PathBuf> {
    let out = std::process::Command::new("fc-match")
        .args(["-f", "%{file}", "Noto Sans CJK TC:lang=zh-tw,Noto Sans CJK SC:lang=zh-cn,sans-serif:lang=zh"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if path.is_empty() {
        None
    } else {
        Some(std::path::PathBuf::from(path))
    }
}

/// Load a system CJK face for the locale. Returns None when the system has no
/// CJK font; the embedded UI subsets still render, only arbitrary typed text
/// would fall back to tofu (and the font coverage test reports that).
fn system_face(locale: Locale) -> Option<egui::FontData> {
    let candidates: Vec<std::path::PathBuf> = SYSTEM_FACES
        .iter()
        .map(std::path::PathBuf::from)
        .chain(fontconfig_face())
        .collect();
    for path in candidates {
        let Ok(bytes) = std::fs::read(&path) else { continue };
        let is_ttc = path
            .extension()
            .map(|e| e.eq_ignore_ascii_case("ttc"))
            .unwrap_or(false);
        let index = if is_ttc { locale.cjk_index() } else { 0 };
        return Some(egui::FontData {
            font: bytes.into(),
            index,
            tweak: egui::FontTweak::default(),
        });
    }
    None
}

/// Weight-600 annotation family (web canvas uses font-weight 600).
pub const SEMIBOLD_FAMILY: &str = "cjk-semibold";

fn apply(ctx: &egui::Context, locale: Locale, system: Option<egui::FontData>) {
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
    let has_system = system.is_some();
    if let Some(data) = system {
        fonts.font_data.insert("cjk-system".to_owned(), std::sync::Arc::new(data));
    }
    fonts.font_data.insert(
        "cjk-semibold".to_owned(),
        std::sync::Arc::new(egui::FontData::from_static(bold)),
    );
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        let list = fonts.families.entry(family).or_default();
        list.push("cjk".to_owned());
        if has_system {
            list.push("cjk-system".to_owned());
        }
    }
    let semibold = egui::FontFamily::Name(SEMIBOLD_FAMILY.into());
    let semibold_list = fonts.families.entry(semibold).or_default();
    semibold_list.push("cjk-semibold".to_owned());
    if has_system {
        semibold_list.push("cjk-system".to_owned());
    }
    ctx.set_fonts(fonts);
}

pub fn install(ctx: &egui::Context, locale: Locale) {
    // System CJK face for arbitrary text, embedded subsets for the UI.
    let system = system_face(locale);
    FULL_LOADED.store(system.is_some(), Ordering::Relaxed);
    apply(ctx, locale, system);
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
    fn every_common_hanzi_has_a_glyph_in_the_system_face() {
        // typed annotation text / file names may be any common Chinese char
        let chars = include_str!("../../scripts/i18n/cjk-common.txt");
        let Ok(bytes) = std::fs::read(SYSTEM_TTC) else {
            eprintln!("system Noto CJK missing; skipping runtime-face coverage test");
            return;
        };
        let parsed = ab_glyph::FontRef::try_from_slice_and_index(&bytes, 3)
            .expect("system TTC parses");
        let missing: Vec<char> = chars
            .chars()
            .filter(|c| !c.is_whitespace() && !c.is_control())
            .filter(|c| parsed.glyph_id(*c) == ab_glyph::GlyphId(0))
            .collect();
        assert!(
            missing.is_empty(),
            "system face misses {} chars, e.g. {:?}",
            missing.len(),
            &missing[..missing.len().min(12)]
        );
    }
}
