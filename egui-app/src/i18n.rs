// Locale + translations. The string tables are generated from the same JSON
// dictionaries the web frontend uses (scripts/gen-i18n-rs.py).

use std::collections::HashMap;
use std::sync::OnceLock;

use crate::i18n_data::{EN, ZH_HANS};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Locale {
    ZhHant,
    ZhHans,
    En,
}

fn table(locale: Locale) -> Option<&'static HashMap<&'static str, &'static str>> {
    static HANS: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    static ENT: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    match locale {
        Locale::ZhHant => None,
        Locale::ZhHans => Some(HANS.get_or_init(|| ZH_HANS.iter().copied().collect())),
        Locale::En => Some(ENT.get_or_init(|| EN.iter().copied().collect())),
    }
}

impl Locale {
    pub fn detect() -> Self {
        // LUMEN_LANG wins, then the usual POSIX locale variables.
        let raw = std::env::var("LUMEN_LANG")
            .ok()
            .filter(|v| !v.trim().is_empty())
            .or_else(|| std::env::var("LC_ALL").ok().filter(|v| !v.trim().is_empty()))
            .or_else(|| std::env::var("LC_MESSAGES").ok().filter(|v| !v.trim().is_empty()))
            .or_else(|| std::env::var("LANG").ok())
            .unwrap_or_default();
        let tag = raw.to_lowercase().replace('_', "-");
        if tag.starts_with("zh") {
            if tag.contains("hans") || tag.contains("cn") || tag.contains("sg") || tag.contains("my")
            {
                return Locale::ZhHans;
            }
            return Locale::ZhHant;
        }
        Locale::En
    }

    pub fn tr(self, source: &str) -> String {
        match table(self) {
            None => source.to_string(),
            Some(map) => map.get(source).map(|s| s.to_string()).unwrap_or_else(|| source.to_string()),
        }
    }

    /// `tr` with placeholders `{0}`, `{1}`….
    pub fn trf(self, source: &str, args: &[&str]) -> String {
        let mut out = self.tr(source);
        for (i, arg) in args.iter().enumerate() {
            out = out.replace(&format!("{{{i}}}"), arg);
        }
        out
    }

    pub fn cjk_index(self) -> u32 {
        match self {
            Locale::ZhHans => 2, // Noto Sans CJK SC
            _ => 3,              // Noto Sans CJK TC
        }
    }
}
