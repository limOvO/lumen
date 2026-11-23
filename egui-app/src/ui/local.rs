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

    pub fn from_index(index: usize) -> Self {
        Self::ALL[index.min(Self::ALL.len() - 1)]
    }
}

#[derive(Clone)]
pub struct Local {
    pub tab: PanelTab,
    pub last_tool: Option<Tool>,
    pub inspector_collapsed: bool,
    /// Screen rect of the floating preview dock (so the peek card can ignore
    /// presses on the button that opens/closes it).
    pub preview_dock: Rect,
    /// Path currently loaded, used to reset per-document UI state.
    pub doc_key: String,
    /// Screen rect of the floating annotation style dock (hover/idle test).
    pub style_dock: Rect,
}

impl Default for Local {
    fn default() -> Self {
        Self {
            tab: PanelTab::default(),
            last_tool: None,
            inspector_collapsed: false,
            preview_dock: Rect::NOTHING,
            doc_key: String::new(),
            style_dock: Rect::NOTHING,
        }
    }
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
            assert_eq!(PanelTab::from_index(i), tab);
            assert!(!tab.label().is_empty());
        }
        // Out-of-range index clamps instead of panicking.
        assert_eq!(PanelTab::from_index(99), PanelTab::Crop);
    }

    #[test]
    fn default_tab_is_info() {
        assert_eq!(PanelTab::default(), PanelTab::Info);
    }
}
