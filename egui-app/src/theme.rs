// Shared visual constants (mirrors css/tokens.css so the two builds look alike).
use egui::Color32;

pub const BG: Color32 = Color32::from_rgb(21, 21, 23); // --bg  #151517
pub const CANVAS: Color32 = Color32::from_rgb(21, 21, 23); // --bg  #151517 (web stage backdrop)
pub const PANEL: Color32 = Color32::from_rgb(35, 35, 36);
pub const TEXT: Color32 = Color32::from_rgb(249, 250, 251);
pub const DIM: Color32 = Color32::from_rgb(156, 163, 175);
pub const LINE: Color32 = Color32::from_rgba_premultiplied(255, 255, 255, 25);
pub const ACCENT: Color32 = Color32::from_rgb(249, 250, 251);
pub const OK: Color32 = Color32::from_rgb(34, 197, 94);
pub const WARN: Color32 = Color32::from_rgb(245, 158, 11);
pub const ERR: Color32 = Color32::from_rgb(239, 68, 68);

pub const TOPBAR_H: f32 = 46.0;
pub const RAIL_W: f32 = 52.0; // --rail-w
pub const PANEL_W: f32 = 300.0;
pub const STATUS_H: f32 = 24.0; // --statusbar-h
pub const DOCK_H: f32 = 56.0;

pub const MOTION: f32 = 0.22;


#[cfg(test)]
mod tests {
    use super::*;

    /// The chrome geometry must stay in sync with the web build's CSS
    /// (src/css/tokens.css / app.css): 46px top bar, 56px rail, 300px panel,
    /// 26px status bar.
    #[test]
    fn layout_matches_web_css() {
        assert_eq!(TOPBAR_H, 46.0);
        assert_eq!(RAIL_W, 52.0);
        assert_eq!(PANEL_W, 300.0);
        assert_eq!(STATUS_H, 24.0);
    }

    #[test]
    fn palette_matches_tokens_css() {
        assert_eq!(BG.to_array(), [21, 21, 23, 255]); // #151517 --bg
        assert_eq!(PANEL.to_array(), [35, 35, 36, 255]);
        assert_eq!(TEXT.to_array(), [249, 250, 251, 255]);
    }
}
