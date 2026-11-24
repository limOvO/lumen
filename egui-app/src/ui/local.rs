// UI-only state that does not belong in AppState (active inspector tab,
// inspector collapse, animation bookkeeping). Stored in the egui context so it
// survives frames without touching Lead-owned modules.

use egui::{Id, Rect};

use crate::state::Tool;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum PanelTab {
    #[default]
    Info,
    Adjust,
    Crop,
}

impl PanelTab {
    pub const ALL: [PanelTab; 3] = [PanelTab::Info, PanelTab::Adjust, PanelTab::Crop];

    pub fn label(self) -> &'static str {
        match self {
            PanelTab::Info => "資訊",
            PanelTab::Adjust => "調整",
            PanelTab::Crop => "裁切",
        }
    }

    pub fn index(self) -> usize {
        match self {
            PanelTab::Info => 0,
            PanelTab::Adjust => 1,
            PanelTab::Crop => 2,
        }
    }

}

#[derive(Clone)]
pub struct Local {
    pub tab: PanelTab,
    pub last_tool: Option<Tool>,
    pub inspector_collapsed: bool,
    /// 調整 panel: proportional-resize lock (on by default, like the web build).
    pub lock_ratio: bool,
    /// Inspector width in points (web: 240–420, default 300).
    pub inspector_width: f32,
    /// Screen rect of the floating preview dock (so the peek card can ignore
    /// presses on the button that opens/closes it).
    pub preview_dock: Rect,
    /// Screen rect of the floating annotation style dock (last painted).
    pub style_dock: Rect,
    /// The same dock rect translated back to the fully-awake position, so the
    /// hover band covers both ends of the slide (prevents wake/sleep jitter).
    pub style_dock_rest: Rect,
    /// True while the pointer is inside the wake band (hysteresis latch).
    pub dock_wake_locked: bool,
    /// Path of the document the UI last saw (drives web-style `openDoc` resets).
    pub last_doc_path: String,
}

impl Default for Local {
    fn default() -> Self {
        Self {
            tab: PanelTab::default(),
            last_tool: None,
            inspector_collapsed: false,
            lock_ratio: true,
            inspector_width: crate::theme::PANEL_W,
            preview_dock: Rect::NOTHING,
            style_dock: Rect::NOTHING,
            style_dock_rest: Rect::NOTHING,
            dock_wake_locked: false,
            last_doc_path: String::new(),
        }
    }
}

/// The web build resets compare only in `openDoc()` (a different file), not on
/// phase changes — keep that rule in one testable place.
pub fn should_reset_compare(previous_doc: &str, current_doc: &str) -> bool {
    previous_doc != current_doc && !current_doc.is_empty()
}

fn id() -> Id {
    Id::new("lumen.ui.local")
}

pub fn get<R>(ctx: &egui::Context, f: impl FnOnce(&Local) -> R) -> R {
    let local: Local = ctx.data(|d| d.get_temp(id())).unwrap_or_default();
    f(&local)
}

pub fn with<R>(ctx: &egui::Context, f: impl FnOnce(&mut Local) -> R) -> R {
    let mut local: Local = ctx.data(|d| d.get_temp(id())).unwrap_or_default();
    let out = f(&mut local);
    ctx.data_mut(|d| d.insert_temp(id(), local));
    out
}

#[cfg(test)]
mod tests {
    use super::PanelTab;

    #[test]
    fn tab_labels_and_roundtrip() {
        assert_eq!(PanelTab::ALL.len(), 3);
        for (i, tab) in PanelTab::ALL.into_iter().enumerate() {
            assert_eq!(tab.index(), i);
            assert!(!tab.label().is_empty());
        }
    }

    #[test]
    fn compare_resets_only_when_a_new_document_loads() {
        assert!(!super::should_reset_compare("", ""));
        assert!(!super::should_reset_compare("/a.png", "/a.png"));
        assert!(super::should_reset_compare("/a.png", "/b.png"));
        assert!(super::should_reset_compare("", "/a.png"));
        // An unloaded document must not clear compare.
        assert!(!super::should_reset_compare("/a.png", ""));
    }

    #[test]
    fn default_tab_is_info() {
        assert_eq!(PanelTab::default(), PanelTab::Info);
    }
}
