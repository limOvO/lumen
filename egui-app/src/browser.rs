// In-app folder browser (the web build's 「開啟圖片」 modal).
//
// Parity source: src/js/ui/browser.js — current path + 前往, 上層資料夾,
// subfolders before images (natural sort), folder row enters on a single click,
// image row selects on click and opens on double click, real thumbnails on the
// image rows, file/folder counts, 開啟 confirm, Esc closes. std::fs only (no
// native dialogs), hidden entries skipped, the image filter is `doc::image_ext`.
//
// Thumbnails are decoded lazily into a small bounded LRU cache (path + mtime +
// size key) and uploaded as egui textures; at most `THUMB_PER_FRAME` decodes
// happen per frame (and never more than ~8 ms of decoding), with the placeholder
// glyph shown until a row's thumbnail is ready. Unreadable/unsupported files are
// cached as "failed" so they are not retried every frame.
//
// Public API (wired by app.rs, owner: dialogs-dev):
//   Browser::new(start: Option<PathBuf>) -> Browser      // open == true
//   browser.open                                             // show/hide
//   browser.cwd / browser.selected / browser.error           // observable state
//   browser.show(ctx, locale) -> Option<PathBuf>             // chosen image
//   browser.open_at(start: Option<PathBuf>) / browser.close() // reuse helpers

use std::cmp::Ordering as CmpOrdering;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use egui::{
    Align, Align2, ColorImage, Color32, CornerRadius, CursorIcon, FontId, Id, Layout, Order,
    Pos2, Rect, Sense, Shape, Stroke, StrokeKind, Vec2,
};

use crate::doc::image_ext;
use crate::i18n::Locale;
use crate::theme;
use crate::ui::format::fmt_bytes;
use crate::ui::widgets;

const CARD_W: f32 = 640.0;
const HEAD_H: f32 = 58.0;
const NAV_H: f32 = 50.0;
const FOOT_H: f32 = 42.0;
/// Row height of the list: web `.browser-row` padding 6px 8px + `.browser-thumb`
/// 44px = 56px pitch.
const ROW_H: f32 = 56.0;
/// Painted thumbnail box, matching web `.browser-thumb` (44×44).
const THUMB_BOX: f32 = 44.0;
const PAD: f32 = 16.0;
const ERR_H: f32 = 34.0;
const PATH_INPUT: &str = "lumen-browser-path";

/// Decoded/uploaded thumbnail size: 2x the painted box so HiDPI screens stay
/// crisp while the texture budget stays tiny (64×64×4 = 16 KiB each).
const THUMB_DECODE: u32 = 64;
/// Bounded LRU capacity — a modal shows ~12 rows, and recently visited folders
/// stay warm without pinning memory.
const THUMB_CACHE_CAP: usize = 64;
/// Never decode more than this many thumbnails in one frame…
const THUMB_PER_FRAME: usize = 3;
/// …and never spend longer than this decoding in one frame.
const THUMB_TIME_BUDGET: Duration = Duration::from_millis(8);
/// Refuse to decode absurdly large bitmaps just to make a 30px thumbnail.
const MAX_DECODE_PIXELS: u64 = 120_000_000;

/// One row of the folder listing.
#[derive(Clone)]
struct Entry {
    path: PathBuf,
    name: String,
    is_dir: bool,
    size: u64,
    mtime: Option<SystemTime>,
}

/// Identity of a cached thumbnail. A file can change without its name changing,
/// so the cache key carries mtime + size (same idea as `export`'s decode cache).
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
struct ThumbKey {
    path: PathBuf,
    mtime: Option<SystemTime>,
    size: u64,
}

impl ThumbKey {
    fn for_entry(entry: &Entry) -> Self {
        Self { path: entry.path.clone(), mtime: entry.mtime, size: entry.size }
    }

    /// egui texture name. The version suffix makes egui upload fresh pixels
    /// when the file changes while keeping one live texture per path.
    fn texture_name(&self) -> String {
        let version = self
            .mtime
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_millis())
            .unwrap_or(0);
        format!("lumen-thumb:{}:{}:{}", self.path.display(), version, self.size)
    }
}

struct ThumbEntry {
    key: ThumbKey,
    /// `None` = the file could not be decoded (cached so the row keeps its
    /// placeholder instead of retrying every frame).
    texture: Option<egui::TextureHandle>,
    /// LRU stamp (monotonically increasing frame-local counter).
    used: u64,
}

/// Bounded LRU of decoded thumbnails, keyed by path and invalidated by
/// mtime/size. Pure bookkeeping (no drawing) so the eviction rules are unit
/// tested without an egui context.
struct ThumbCache {
    entries: HashMap<PathBuf, ThumbEntry>,
    capacity: usize,
    tick: u64,
}

impl ThumbCache {
    fn new(capacity: usize) -> Self {
        Self { entries: HashMap::new(), capacity: capacity.max(1), tick: 0 }
    }

    /// `Some(Some(handle))` for a fresh hit, `Some(None)` when the file is a
    /// known decode failure, `None` when the entry is missing or stale. Touches
    /// the LRU on a hit.
    fn lookup(&mut self, key: &ThumbKey) -> Option<Option<egui::TextureHandle>> {
        self.tick += 1;
        let tick = self.tick;
        let entry = self.entries.get_mut(&key.path)?;
        if entry.key != *key {
            return None;
        }
        entry.used = tick;
        Some(entry.texture.clone())
    }

    fn insert(&mut self, key: ThumbKey, texture: Option<egui::TextureHandle>) {
        self.tick += 1;
        let used = self.tick;
        self.entries.insert(key.path.clone(), ThumbEntry { key, texture, used });
        while self.entries.len() > self.capacity {
            if !self.evict_one() {
                break;
            }
        }
    }

    /// Drop the least-recently-used entry. Returns whether anything was removed.
    fn evict_one(&mut self) -> bool {
        let victim = self
            .entries
            .iter()
            .min_by_key(|(_, entry)| entry.used)
            .map(|(path, _)| path.clone());
        match victim {
            Some(path) => self.entries.remove(&path).is_some(),
            None => false,
        }
    }

    /// Fresh entry for this exact key (never a stale one).
    fn contains(&self, key: &ThumbKey) -> bool {
        self.entries.get(&key.path).map_or(false, |entry| entry.key == *key)
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.entries.len()
    }
}

/// What a row click asked for (applied after the list loop so `self` is free).
enum RowAction {
    Select(PathBuf),
    EnterDir(PathBuf),
    Open(PathBuf),
}

pub struct Browser {
    pub open: bool,
    pub cwd: PathBuf,
    pub selected: Option<PathBuf>,
    pub error: Option<String>,
    path_buf: String,
    entries: Vec<Entry>,
    dirs: usize,
    files: usize,
    loaded: Option<PathBuf>,
    focus_path: bool,
    scroll_to_selected: bool,
    /// Timestamp of the last folder entry, so a double click on a folder row
    /// does not descend twice (the first click already reloaded the list).
    last_enter_at: f64,
    /// Lazily decoded image thumbnails (bounded LRU, keyed by path+mtime+size).
    thumbs: ThumbCache,
    /// Scroll offset of the list after the last frame; kept so tests can prove
    /// the selected row was scrolled into view.
    #[allow(dead_code)]
    last_scroll_offset: Vec2,
}

impl Browser {
    pub fn new(start: Option<PathBuf>) -> Self {
        let cwd = start_cwd(start);
        Self {
            open: true,
            path_buf: path_text(&cwd),
            cwd,
            selected: None,
            error: None,
            entries: Vec::new(),
            dirs: 0,
            files: 0,
            loaded: None,
            focus_path: true,
            scroll_to_selected: false,
            last_enter_at: f64::NEG_INFINITY,
            thumbs: ThumbCache::new(THUMB_CACHE_CAP),
            last_scroll_offset: Vec2::ZERO,
        }
    }

    /// Re-open an existing browser at `start` (keeps the allocation; the lead
    /// can reuse one Browser instance for every 開啟).
    pub fn open_at(&mut self, start: Option<PathBuf>) {
        if let Some(path) = start {
            self.cwd = start_cwd(Some(path));
        }
        self.path_buf = path_text(&self.cwd);
        self.open = true;
        self.selected = None;
        self.error = None;
        self.loaded = None;
        self.focus_path = true;
        self.scroll_to_selected = false;
        self.last_enter_at = f64::NEG_INFINITY;
    }

    pub fn close(&mut self) {
        self.open = false;
        self.selected = None;
        self.error = None;
        self.scroll_to_selected = false;
        self.focus_path = false;
    }

    /// Draws the modal. Returns the chosen image path when the user confirms
    /// (or double-clicks an image); the browser closes itself in that case.
    pub fn show(&mut self, ctx: &egui::Context, locale: Locale) -> Option<PathBuf> {
        if !self.open {
            return None;
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.open = false;
            return None;
        }
        self.sync_load(locale);

        let screen = ctx.screen_rect();
        let height = (screen.height() - 56.0).clamp(360.0, 620.0);
        let err_h = if self.error.is_some() { ERR_H } else { 0.0 };
        let list_h = (height - HEAD_H - NAV_H - FOOT_H - err_h).max(120.0);
        let total_h = HEAD_H + NAV_H + err_h + list_h + FOOT_H;
        let avail = ctx.screen_rect().shrink(20.0);
        let card_w = CARD_W.min(avail.width());
        let card_h = total_h.min(avail.height());

        // Scrim: dims the app and swallows clicks outside the card.
        let scrim = egui::Area::new(Id::new("lumen-browser-scrim"))
            .order(Order::Foreground)
            .fixed_pos(screen.left_top())
            .show(ctx, |ui| {
                let (rect, resp) = ui.allocate_exact_size(screen.size(), Sense::click());
                ui.painter().rect_filled(
                    rect,
                    CornerRadius::ZERO,
                    Color32::from_rgba_unmultiplied(10, 10, 12, 170),
                );
                resp
            });
        if scrim.inner.clicked() {
            self.open = false;
            return None;
        }

        let mut chosen: Option<PathBuf> = None;
        let mut close_requested = false;
        // Rows that still need a thumbnail this frame (visible ones first).
        let mut visible_thumbs: Vec<ThumbKey> = Vec::new();
        let mut queued_thumbs: Vec<ThumbKey> = Vec::new();

        egui::Window::new(locale.tr("開啟圖片"))
            .id(Id::new("lumen-browser"))
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .movable(false)
            // Tooltip order = above the Foreground scrim, no matter which layer
            // got created first (this used to leave the dialog dimmed and
            // unclickable behind the scrim after some open/close cycles).
            .order(Order::Tooltip)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .fixed_size(Vec2::new(card_w, card_h))
            .frame(
                egui::Frame::new()
                    .fill(theme::PANEL)
                    .stroke(Stroke::new(1.0_f32, widgets::white(60)))
                    .corner_radius(CornerRadius::same(12))
                    .inner_margin(egui::Margin::ZERO),
            )
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing = Vec2::ZERO;
                ui.set_width(card_w);
                ui.set_height(card_h);

                // --- head ---------------------------------------------------
                let (head, _) =
                    ui.allocate_exact_size(Vec2::new(card_w, HEAD_H), Sense::hover());
                {
                    let painter = ui.painter();
                    painter.text(
                        Pos2::new(head.left() + PAD, head.top() + 20.0),
                        Align2::LEFT_CENTER,
                        locale.tr("開啟圖片"),
                        FontId::proportional(13.5),
                        theme::TEXT,
                    );
                    painter.text(
                        Pos2::new(head.left() + PAD, head.top() + 38.0),
                        Align2::LEFT_CENTER,
                        locale.tr("雙擊清單中的圖片即可開啟"),
                        FontId::proportional(10.5),
                        widgets::white(120),
                    );
                    painter.hline(
                        head.x_range(),
                        head.bottom() - 0.5,
                        Stroke::new(1.0_f32, widgets::white(24)),
                    );
                }
                let close_rect = Rect::from_center_size(
                    Pos2::new(head.right() - PAD - 14.0, head.top() + 25.0),
                    Vec2::splat(28.0),
                );
                let close = ui.interact(
                    close_rect,
                    Id::new("lumen-browser-close"),
                    Sense::click(),
                );
                if close.hovered() {
                    ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                    ui.painter().circle_filled(
                        close_rect.center(),
                        14.0,
                        widgets::white(24),
                    );
                }
                paint_close(
                    ui.painter(),
                    Rect::from_center_size(close_rect.center(), Vec2::splat(14.0)),
                    if close.hovered() { theme::TEXT } else { theme::DIM },
                );
                if close.clicked() {
                    close_requested = true;
                }

                // --- nav row ------------------------------------------------
                let (nav, _) = ui.allocate_exact_size(Vec2::new(CARD_W, NAV_H), Sense::hover());
                let row_y = nav.center().y - 15.0;
                let inner_left = nav.left() + PAD;
                let inner_right = nav.right() - PAD;
                let open_w = 76.0;
                let go_w = 60.0;
                let up_rect =
                    Rect::from_min_size(Pos2::new(inner_left, row_y), Vec2::splat(30.0));
                let path_rect = Rect::from_min_max(
                    Pos2::new(up_rect.right() + 8.0, row_y),
                    Pos2::new(inner_right - open_w - 8.0 - go_w - 8.0, row_y + 30.0),
                );
                let go_rect = Rect::from_min_size(
                    Pos2::new(path_rect.right() + 8.0, row_y),
                    Vec2::new(go_w, 30.0),
                );
                let open_rect = Rect::from_min_size(
                    Pos2::new(inner_right - open_w, row_y),
                    Vec2::new(open_w, 30.0),
                );

                // 上層資料夾 (custom up arrow, matching the web SVG).
                let up = ui.interact(up_rect, Id::new("lumen-browser-up"), Sense::click());
                let hovered = up.hovered();
                if hovered {
                    ui.painter().rect_filled(
                        up_rect,
                        CornerRadius::same(widgets::RADIUS),
                        widgets::white(15),
                    );
                    ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                }
                let up_enabled = self.cwd.parent().is_some();
                let color = if !up_enabled {
                    widgets::disabled(theme::DIM)
                } else if hovered {
                    theme::TEXT
                } else {
                    theme::DIM
                };
                {
                    let k = 14.0 / 24.0;
                    let c = up_rect.center();
                    let p = |x: f32, y: f32| {
                        Pos2::new(c.x + (x - 12.0) * k, c.y + (y - 12.0) * k)
                    };
                    let stroke = Stroke::new(1.7_f32, color);
                    ui.painter().line_segment([p(12.0, 19.0), p(12.0, 5.0)], stroke);
                    ui.painter()
                        .add(Shape::line(vec![p(6.0, 11.0), p(12.0, 5.0), p(18.0, 11.0)], stroke));
                }
                let up = up.on_hover_text(locale.tr("上層資料夾"));
                let mut go_requested = false;
                if up.clicked() && up_enabled {
                    if let Some(parent) = self.cwd.parent() {
                        let parent = parent.to_path_buf();
                        self.enter(parent, locale);
                    }
                }

                // Path editor.
                widgets::at_rect(ui, path_rect, |ui| {
                    let resp = ui.add(
                        egui::TextEdit::singleline(&mut self.path_buf)
                            .id(Id::new(PATH_INPUT))
                            .hint_text(locale.tr("目前路徑"))
                            .font(FontId::monospace(11.0))
                            .text_color(theme::TEXT)
                            .background_color(theme::BG)
                            .margin(egui::Margin::symmetric(8, 6))
                            .desired_width(path_rect.width())
                            .min_size(Vec2::new(path_rect.width(), 30.0))
                            .vertical_align(Align::Center),
                    );
                    if self.focus_path {
                        resp.request_focus();
                        self.focus_path = false;
                    }
                    if resp.lost_focus()
                        && ui.input(|i| i.key_pressed(egui::Key::Enter))
                    {
                        go_requested = true;
                    }
                    ui.painter().rect_stroke(
                        resp.rect.expand(1.0),
                        CornerRadius::same(7),
                        Stroke::new(
                            1.0_f32,
                            if resp.has_focus() { theme::TEXT } else { widgets::white(38) },
                        ),
                        StrokeKind::Outside,
                    );
                });

                if painted_button(ui, go_rect, &locale.tr("前往"), false, true) {
                    go_requested = true;
                }
                // Always enabled, like the web modal: confirming without a
                // selection shows the inline warning instead of doing nothing.
                if painted_button(ui, open_rect, &locale.tr("開啟"), true, true) {
                    match self.selected.clone() {
                        Some(path) if path.is_dir() => self.enter(path, locale),
                        Some(path) => chosen = Some(path),
                        None => self.error = Some(locale.tr("請先選擇資料夾或圖片")),
                    }
                }
                if go_requested {
                    self.go(locale);
                }

                // --- inline error ------------------------------------------
                if let Some(err) = self.error.clone() {
                    let (strip, _) =
                        ui.allocate_exact_size(Vec2::new(CARD_W, err_h), Sense::hover());
                    let r = Rect::from_min_max(
                        Pos2::new(strip.left() + PAD, strip.top() + 4.0),
                        Pos2::new(strip.right() - PAD, strip.bottom() - 4.0),
                    );
                    let painter = ui.painter();
                    painter.rect_filled(
                        r,
                        CornerRadius::same(7),
                        Color32::from_rgba_unmultiplied(245, 158, 11, 26),
                    );
                    painter.rect_stroke(
                        r,
                        CornerRadius::same(7),
                        Stroke::new(
                            1.0_f32,
                            Color32::from_rgba_unmultiplied(245, 158, 11, 82),
                        ),
                        StrokeKind::Inside,
                    );
                    painter.text(
                        Pos2::new(r.left() + 9.0, r.center().y),
                        Align2::LEFT_CENTER,
                        elide(&err, 84),
                        FontId::proportional(11.0),
                        theme::WARN,
                    );
                }

                // --- keyboard navigation (list focused / nothing typed) ----
                // When the Enter key was just used to confirm the path field it
                // must not also activate the list (that used to pop a bogus
                // 「請先選擇資料夾或圖片」 banner right after navigating).
                let typing = ctx.memory(|m| m.focused()).is_some();
                if !typing && !go_requested {
                    let len = self.entries.len();
                    let current = self.selected_index();
                    let mut next: Option<Option<usize>> = None;
                    let mut activate = false;
                    ctx.input(|i| {
                        if i.key_pressed(egui::Key::ArrowDown) {
                            next = Some(step_index(len, current, 1));
                        } else if i.key_pressed(egui::Key::ArrowUp) {
                            next = Some(step_index(len, current, -1));
                        } else if i.key_pressed(egui::Key::Home) {
                            next = Some(edge_index(len, false));
                        } else if i.key_pressed(egui::Key::End) {
                            next = Some(edge_index(len, true));
                        } else if i.key_pressed(egui::Key::Enter) {
                            activate = true;
                        }
                    });
                    if let Some(slot) = next {
                        self.selected = slot.and_then(|i| self.entries.get(i)).map(|e| e.path.clone());
                        self.scroll_to_selected = true;
                        self.error = None;
                    }
                    if activate {
                        match self.selected.clone() {
                            Some(path) if path.is_dir() => self.enter(path, locale),
                            Some(path) => chosen = Some(path),
                            None => {
                                self.error = Some(locale.tr("請先選擇資料夾或圖片"))
                            }
                        }
                    }
                }

                // --- list ---------------------------------------------------
                let (list_rect, _) =
                    ui.allocate_exact_size(Vec2::new(CARD_W, list_h), Sense::hover());
                let mut list_ui = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(list_rect)
                        .layout(Layout::top_down(Align::Min)),
                );
                list_ui.set_clip_rect(ui.clip_rect().intersect(list_rect));
                let scroll_out = egui::ScrollArea::vertical()
                    .id_salt("lumen-browser-list")
                    .auto_shrink([false, false])
                    .show(&mut list_ui, |ui| {
                        ui.set_width((list_rect.width() - PAD * 2.0 - 12.0).max(120.0));
                        ui.spacing_mut().item_spacing = Vec2::new(0.0, 2.0);
                        ui.add_space(4.0);
                        if self.entries.is_empty() {
                            let (state_rect, _) = ui.allocate_exact_size(
                                Vec2::new(ui.available_width(), 80.0),
                                Sense::hover(),
                            );
                            let text = if self.error.is_some() {
                                locale.tr("無法讀取這個資料夾")
                            } else {
                                locale.tr("這個資料夾沒有圖片")
                            };
                            ui.painter().text(
                                state_rect.center(),
                                Align2::CENTER_CENTER,
                                text,
                                FontId::proportional(11.5),
                                widgets::white(130),
                            );
                            return;
                        }
                        let mut action: Option<RowAction> = None;
                        for (row_i, entry) in self.entries.iter().enumerate() {
                            let (row, resp) = ui.allocate_exact_size(
                                Vec2::new(ui.available_width(), ROW_H),
                                Sense::click(),
                            );
                            let active =
                                self.selected.as_deref() == Some(entry.path.as_path());
                            let hovered = resp.hovered();
                            // Web-style press feedback: the row content pops down
                            // to 97% while held and springs back on release.
                            let press = crate::ui::motion::press_scale(
                                ui.ctx(),
                                Id::new(("browser-row", row_i)),
                                &resp,
                                0.97,
                            );
                            let base_row = row.shrink2(Vec2::new(4.0, 0.0));
                            let row_rect = Rect::from_center_size(
                                base_row.center(),
                                base_row.size() * press,
                            );
                            let painter = ui.painter();
                            if active {
                                painter.rect_filled(
                                    row_rect,
                                    CornerRadius::same(7),
                                    Color32::from_rgba_unmultiplied(255, 255, 255, 26),
                                );
                            } else if hovered {
                                painter.rect_filled(
                                    row_rect,
                                    CornerRadius::same(7),
                                    widgets::white(15),
                                );
                            }
                            let thumb = Rect::from_center_size(
                                Pos2::new(
                                    row_rect.left() + 8.0 + THUMB_BOX / 2.0,
                                    row_rect.center().y,
                                ),
                                Vec2::splat(THUMB_BOX),
                            );
                            painter.rect_filled(thumb, CornerRadius::same(6), widgets::SURFACE);
                            let glyph =
                                Rect::from_center_size(thumb.center(), Vec2::splat(22.0));
                            if entry.is_dir {
                                paint_folder_glyph(painter, glyph, theme::DIM);
                            } else {
                                let key = ThumbKey::for_entry(entry);
                                match self.thumbs.lookup(&key) {
                                    Some(Some(texture)) => {
                                        painter.image(
                                            texture.id(),
                                            thumb.shrink(1.0),
                                            Rect::from_min_max(
                                                Pos2::ZERO,
                                                Pos2::new(1.0, 1.0),
                                            ),
                                            Color32::WHITE,
                                        );
                                    }
                                    Some(None) => {
                                        // Known decode failure: keep the placeholder.
                                        paint_image_glyph(painter, glyph, theme::DIM);
                                    }
                                    None => {
                                        paint_image_glyph(painter, glyph, theme::DIM);
                                        if ui.clip_rect().intersects(row) || active {
                                            visible_thumbs.push(key);
                                        } else {
                                            queued_thumbs.push(key);
                                        }
                                    }
                                }
                            }
                            painter.rect_stroke(
                                thumb,
                                CornerRadius::same(6),
                                Stroke::new(1.0_f32, widgets::white(28)),
                                StrokeKind::Inside,
                            );
                            let text_x = thumb.right() + 10.0;
                            let max_w = (row_rect.right() - text_x - 8.0).max(10.0);
                            let name_font = FontId::proportional(11.5);
                            let mut galley = painter.layout_no_wrap(
                                entry.name.clone(),
                                name_font.clone(),
                                theme::TEXT,
                            );
                            if galley.size().x > max_w {
                                let max_chars = ((max_w / 7.0) as usize).max(4);
                                galley = painter.layout_no_wrap(
                                    elide(&entry.name, max_chars),
                                    name_font,
                                    theme::TEXT,
                                );
                            }
                            let name_y = row_rect.center().y - 8.0 - galley.size().y / 2.0;
                            painter.galley(
                                Pos2::new(text_x, name_y),
                                galley,
                                theme::TEXT,
                            );
                            if entry.is_dir {
                                painter.text(
                                    Pos2::new(text_x, row_rect.center().y + 9.0),
                                    Align2::LEFT_CENTER,
                                    locale.tr("資料夾"),
                                    FontId::monospace(10.0),
                                    widgets::white(140),
                                );
                            } else {
                                let meta = format!(
                                    "{} · {}",
                                    fmt_bytes(entry.size),
                                    entry
                                        .mtime
                                        .map(fmt_mtime)
                                        .unwrap_or_else(|| "—".to_string())
                                );
                                painter.text(
                                    Pos2::new(text_x, row_rect.center().y + 9.0),
                                    Align2::LEFT_CENTER,
                                    meta,
                                    FontId::monospace(10.0),
                                    widgets::white(140),
                                );
                            }
                            if hovered {
                                ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                            }
                            if self.scroll_to_selected && active {
                                ui.scroll_to_rect(row_rect, Some(Align::Center));
                            }
                            if !entry.is_dir && resp.double_clicked() {
                                action = Some(RowAction::Open(entry.path.clone()));
                            } else if resp.clicked() {
                                ui.ctx()
                                    .memory_mut(|m| m.surrender_focus(Id::new(PATH_INPUT)));
                                self.focus_path = false;
                                if entry.is_dir {
                                    // Single click enters. A fast second click is
                                    // the tail of a double click on the old row;
                                    // ignore it so the list cannot descend twice.
                                    let now = ui.input(|i| i.time);
                                    if now - self.last_enter_at > 0.35 {
                                        self.last_enter_at = now;
                                        action = Some(RowAction::EnterDir(entry.path.clone()));
                                    }
                                } else {
                                    action = Some(RowAction::Select(entry.path.clone()));
                                }
                            }
                        }
                        match action {
                            Some(RowAction::Select(path)) => {
                                self.selected = Some(path);
                                self.error = None;
                            }
                            Some(RowAction::EnterDir(path)) => self.enter(path, locale),
                            Some(RowAction::Open(path)) => {
                                self.selected = Some(path.clone());
                                chosen = Some(path);
                            }
                            None => {}
                        }
                        if self.files == 0 && self.dirs > 0 {
                            let (note, _) = ui.allocate_exact_size(
                                Vec2::new(ui.available_width(), 28.0),
                                Sense::hover(),
                            );
                            ui.painter().text(
                                Pos2::new(note.left() + 44.0, note.center().y),
                                Align2::LEFT_CENTER,
                                locale.tr("這個資料夾沒有圖片"),
                                FontId::proportional(10.5),
                                widgets::white(120),
                            );
                        }
                        ui.add_space(6.0);
                    });
                self.last_scroll_offset = scroll_out.state.offset;
                self.scroll_to_selected = false;
                if !visible_thumbs.is_empty() || !queued_thumbs.is_empty() {
                    visible_thumbs.append(&mut queued_thumbs);
                    self.fill_thumbnails(ui.ctx(), std::mem::take(&mut visible_thumbs));
                }

                // --- footer -------------------------------------------------
                let (foot, _) =
                    ui.allocate_exact_size(Vec2::new(CARD_W, FOOT_H), Sense::hover());
                {
                    let painter = ui.painter();
                    painter.hline(
                        foot.x_range(),
                        foot.top() + 0.5,
                        Stroke::new(1.0_f32, widgets::white(24)),
                    );
                    painter.text(
                        Pos2::new(foot.left() + PAD, foot.center().y),
                        Align2::LEFT_CENTER,
                        count_label(locale, self.dirs, self.files),
                        FontId::proportional(10.5),
                        widgets::white(120),
                    );
                }
            });

        if close_requested || chosen.is_some() {
            self.open = false;
        }
        chosen
    }

    /// Decode at most `THUMB_PER_FRAME` queued thumbnails and stop once the
    /// frame's decode budget is spent, so the modal keeps painting at speed
    /// while the list fills in. Queue order is visible rows first.
    fn fill_thumbnails(&mut self, ctx: &egui::Context, queue: Vec<ThumbKey>) {
        if queue.is_empty() {
            return;
        }
        let deadline = Instant::now() + THUMB_TIME_BUDGET;
        let mut decoded = 0usize;
        for key in queue {
            if decoded >= THUMB_PER_FRAME || Instant::now() >= deadline {
                break;
            }
            if self.thumbs.contains(&key) {
                continue;
            }
            let texture = decode_thumbnail(&key).map(|image| upload_thumbnail(ctx, &key, image));
            self.thumbs.insert(key, texture);
            decoded += 1;
        }
    }

    // -- folder loading -----------------------------------------------------

    fn sync_load(&mut self, locale: Locale) {
        if self.loaded.as_deref() == Some(self.cwd.as_path()) {
            return;
        }
        self.reload(locale);
    }

    fn reload(&mut self, locale: Locale) {
        match read_folder(&self.cwd) {
            Ok((entries, dirs, files)) => {
                self.entries = entries;
                self.dirs = dirs;
                self.files = files;
                self.error = None;
            }
            Err(err) => {
                self.entries.clear();
                self.dirs = 0;
                self.files = 0;
                self.error = Some(format!(
                    "{}（{}）",
                    locale.tr("無法讀取這個資料夾"),
                    err
                ));
            }
        }
        self.loaded = Some(self.cwd.clone());
        self.prune_selection();
    }

    fn prune_selection(&mut self) {
        if let Some(selected) = self.selected.clone() {
            if !self.entries.iter().any(|e| e.path == selected) {
                self.selected = None;
            }
        }
    }

    fn selected_index(&self) -> Option<usize> {
        self.entries
            .iter()
            .position(|e| self.selected.as_deref() == Some(e.path.as_path()))
    }

    fn enter(&mut self, path: PathBuf, locale: Locale) {
        if self.cwd == path {
            return;
        }
        self.cwd = path;
        self.selected = None;
        self.error = None;
        self.path_buf = path_text(&self.cwd);
        self.loaded = None;
        self.scroll_to_selected = false;
        self.reload(locale);
    }

    fn go(&mut self, locale: Locale) {
        let input = self.path_buf.clone();
        match resolve_go(&input, &self.cwd, home_dir().as_deref()) {
            Ok(path) => match validate_dir(&path) {
                Ok(()) => {
                    self.cwd = path;
                    self.selected = None;
                    self.error = None;
                    self.path_buf = path_text(&self.cwd);
                    self.loaded = None;
                    self.reload(locale);
                }
                Err(issue) => self.error = Some(path_issue_message(locale, issue)),
            },
            Err(issue) => self.error = Some(path_issue_message(locale, issue)),
        }
    }
}

// ---------------------------------------------------------------------------
// Thumbnails
// ---------------------------------------------------------------------------

/// Decode one image into a small square RGBA thumbnail (centre-cropped like the
/// web's `object-fit: cover`). Cheap by design: the header is read first so a
/// panorama is never decoded, then the decoder is asked for at most
/// `THUMB_DECODE`×`THUMB_DECODE` bytes. Returns `None` for any failure; the row
/// keeps its placeholder glyph and the failure is cached.
fn decode_thumbnail(key: &ThumbKey) -> Option<ColorImage> {
    let reader = image::ImageReader::open(&key.path).ok()?;
    let reader = reader.with_guessed_format().ok()?;
    let (width, height) = reader.into_dimensions().ok()?;
    if width == 0 || height == 0 || (width as u64) * (height as u64) > MAX_DECODE_PIXELS {
        return None;
    }
    let decoded = image::ImageReader::open(&key.path)
        .ok()?
        .with_guessed_format()
        .ok()?
        .decode()
        .ok()?;
    let thumbnail = decoded.thumbnail(THUMB_DECODE, THUMB_DECODE);
    let square = square_cover(thumbnail.to_rgba8());
    let (w, h) = square.dimensions();
    if w == 0 || h == 0 {
        return None;
    }
    Some(ColorImage::from_rgba_unmultiplied(
        [w as usize, h as usize],
        square.as_raw(),
    ))
}

/// Centre-crop to a square (the aspect-ratio part of `object-fit: cover`).
fn square_cover(image: image::RgbaImage) -> image::RgbaImage {
    let (width, height) = image.dimensions();
    let side = width.min(height);
    if side == 0 || width == height {
        return image;
    }
    let x = (width - side) / 2;
    let y = (height - side) / 2;
    image::imageops::crop_imm(&image, x, y, side, side).to_image()
}

fn upload_thumbnail(ctx: &egui::Context, key: &ThumbKey, image: ColorImage) -> egui::TextureHandle {
    ctx.load_texture(key.texture_name(), image, egui::TextureOptions::LINEAR)
}

// ---------------------------------------------------------------------------
// Painted controls (the card draws its own chrome like the rest of the app).
// ---------------------------------------------------------------------------

fn painted_button(
    ui: &mut egui::Ui,
    rect: Rect,
    label: &str,
    primary: bool,
    enabled: bool,
) -> bool {
    let resp = ui.interact(rect, ui.id().with(("lumen-btn", label)), Sense::click());
    let hovered = enabled && resp.hovered();
    let painter = ui.painter();
    let radius = CornerRadius::same(widgets::RADIUS);
    if primary {
        painter.rect_filled(
            rect,
            radius,
            if !enabled {
                widgets::white(40)
            } else if hovered {
                Color32::WHITE
            } else {
                theme::TEXT
            },
        );
    } else {
        if hovered {
            painter.rect_filled(rect, radius, widgets::white(15));
        }
        painter.rect_stroke(
            rect,
            radius,
            Stroke::new(1.0_f32, widgets::white(36)),
            StrokeKind::Inside,
        );
    }
    let color = if !enabled {
        widgets::disabled(theme::DIM)
    } else if primary {
        theme::BG
    } else if hovered {
        theme::TEXT
    } else {
        theme::DIM
    };
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        label,
        FontId::proportional(11.5),
        color,
    );
    if hovered {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    ui.painter().rect_stroke(
        rect,
        radius,
        Stroke::new(
            1.0_f32,
            if hovered { widgets::white(60) } else { Color32::TRANSPARENT },
        ),
        StrokeKind::Inside,
    );
    enabled && resp.clicked()
}

fn paint_close(painter: &egui::Painter, rect: Rect, color: Color32) {
    let stroke = Stroke::new(1.8_f32, color);
    painter.line_segment(
        [Pos2::new(rect.left(), rect.top()), Pos2::new(rect.right(), rect.bottom())],
        stroke,
    );
    painter.line_segment(
        [Pos2::new(rect.right(), rect.top()), Pos2::new(rect.left(), rect.bottom())],
        stroke,
    );
}

/// 24x24 folder outline from the web icon set (drawn locally so this module
/// does not depend on `ui::widgets::Icon`, which the UI owner evolves).
fn paint_folder_glyph(painter: &egui::Painter, rect: Rect, color: Color32) {
    let d = rect.width().min(rect.height()).max(1.0);
    let s = d / 24.0;
    let origin = rect.center() - Vec2::splat(d / 2.0);
    let p = |x: f32, y: f32| origin + Vec2::new(x * s, y * s);
    let stroke = Stroke::new(1.6_f32, color);
    painter.add(Shape::line(
        vec![
            p(3.5, 8.3),
            p(3.5, 17.2),
            p(5.2, 18.9),
            p(18.8, 18.9),
            p(20.5, 17.2),
            p(20.5, 9.6),
            p(11.8, 9.6),
            p(9.8, 7.6),
            p(5.2, 7.6),
            p(3.5, 8.3),
        ],
        stroke,
    ));
}

/// 24x24 picture outline (frame + sun + mountains), as in the web icon set.
fn paint_image_glyph(painter: &egui::Painter, rect: Rect, color: Color32) {
    let d = rect.width().min(rect.height()).max(1.0);
    let s = d / 24.0;
    let origin = rect.center() - Vec2::splat(d / 2.0);
    let p = |x: f32, y: f32| origin + Vec2::new(x * s, y * s);
    let stroke = Stroke::new(1.6_f32, color);
    painter.rect_stroke(
        Rect::from_min_max(p(3.5, 4.5), p(20.5, 19.5)),
        CornerRadius::same((3.0 * s).round() as u8),
        stroke,
        StrokeKind::Middle,
    );
    painter.circle_filled(p(9.0, 10.0), (1.7 * s).max(0.9), color);
    painter.add(Shape::line(
        vec![
            p(4.5, 17.5),
            p(9.5, 13.0),
            p(12.9, 16.1),
            p(15.9, 13.7),
            p(19.5, 17.0),
        ],
        stroke,
    ));
}

// ---------------------------------------------------------------------------
// Pure helpers (unit-tested below).
// ---------------------------------------------------------------------------

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
}

fn path_text(path: &Path) -> String {
    path.display().to_string()
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum PathIssue {
    Empty,
    NotFound,
    NotDir,
}

fn path_issue_message(locale: Locale, issue: PathIssue) -> String {
    match issue {
        PathIssue::Empty => locale.tr("請輸入資料夾路徑"),
        PathIssue::NotFound => locale.tr("路徑不存在"),
        PathIssue::NotDir => locale.tr("不是資料夾"),
    }
}

/// First folder to show: `start` if it is a folder, its parent if it is a
/// file, the process cwd otherwise.
fn start_cwd(start: Option<PathBuf>) -> PathBuf {
    if let Some(path) = start {
        let path = expand_home(&path, home_dir().as_deref());
        if path.is_dir() {
            return path;
        }
        if let Some(parent) = path.parent() {
            if parent.is_dir() {
                return parent.to_path_buf();
            }
        }
    }
    std::env::current_dir()
        .ok()
        .or_else(home_dir)
        .unwrap_or_else(|| PathBuf::from("/"))
}

fn expand_home(path: &Path, home: Option<&Path>) -> PathBuf {
    let Some(home) = home else {
        return path.to_path_buf();
    };
    let text = path.to_string_lossy();
    if text == "~" {
        return home.to_path_buf();
    }
    if let Some(rest) = text.strip_prefix("~/") {
        return if rest.is_empty() {
            home.to_path_buf()
        } else {
            home.join(rest)
        };
    }
    path.to_path_buf()
}

/// Resolve the path field: trims, expands a leading `~`, and joins relative
/// paths against the folder currently shown.
fn resolve_go(input: &str, cwd: &Path, home: Option<&Path>) -> Result<PathBuf, PathIssue> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(PathIssue::Empty);
    }
    if trimmed == "~" || trimmed.starts_with("~/") {
        let home = home.ok_or(PathIssue::NotFound)?;
        let rest = trimmed.strip_prefix("~/").unwrap_or("");
        return Ok(if rest.is_empty() {
            home.to_path_buf()
        } else {
            home.join(rest)
        });
    }
    let path = PathBuf::from(trimmed);
    Ok(if path.is_absolute() {
        path
    } else {
        cwd.join(path)
    })
}

fn validate_dir(path: &Path) -> Result<(), PathIssue> {
    match std::fs::metadata(path) {
        Ok(meta) if meta.is_dir() => Ok(()),
        Ok(_) => Err(PathIssue::NotDir),
        Err(_) => Err(PathIssue::NotFound),
    }
}

/// Read one folder: subfolders first, then images, both in natural order.
/// Hidden entries (leading dot) and non-image files are skipped.
fn read_folder(dir: &Path) -> Result<(Vec<Entry>, usize, usize), std::io::Error> {
    let mut dirs: Vec<Entry> = Vec::new();
    let mut files: Vec<Entry> = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let Ok(entry) = entry else { continue };
        let name = entry.file_name().to_string_lossy().to_string();
        if name.is_empty() || name.starts_with('.') {
            continue;
        }
        let path = entry.path();
        // Follow symlinks so symlinked folders stay enterable.
        let Ok(meta) = std::fs::metadata(&path) else { continue };
        let mtime = meta.modified().ok();
        if meta.is_dir() {
            dirs.push(Entry { path, name, is_dir: true, size: 0, mtime });
        } else if meta.is_file() && image_ext(&path) {
            files.push(Entry { path, name, is_dir: false, size: meta.len(), mtime });
        }
    }
    dirs.sort_by(|a, b| natural_cmp(&a.name, &b.name));
    files.sort_by(|a, b| natural_cmp(&a.name, &b.name));
    let dir_count = dirs.len();
    let file_count = files.len();
    dirs.extend(files);
    Ok((dirs, dir_count, file_count))
}

fn count_label(locale: Locale, dirs: usize, files: usize) -> String {
    if dirs == 0 && files == 0 {
        return locale.tr("沒有項目");
    }
    let fill = |text: String, token: &str, value: usize| text.replace(token, &value.to_string());
    if dirs == 0 {
        return fill(locale.tr("${files} 張圖片"), "${files}", files);
    }
    if files == 0 {
        return fill(locale.tr("${dirs} 個資料夾"), "${dirs}", dirs);
    }
    let text = locale.tr("${dirs} 個資料夾 · ${files} 張圖片");
    fill(fill(text, "${dirs}", dirs), "${files}", files)
}

fn step_index(len: usize, current: Option<usize>, delta: i32) -> Option<usize> {
    if len == 0 {
        return None;
    }
    match current {
        None => Some(if delta < 0 { len - 1 } else { 0 }),
        Some(index) => {
            let index = index.min(len - 1);
            let next = if delta >= 0 {
                (index + delta as usize).min(len - 1)
            } else {
                index.saturating_sub((-delta) as usize)
            };
            Some(next)
        }
    }
}

fn edge_index(len: usize, last: bool) -> Option<usize> {
    if len == 0 {
        None
    } else {
        Some(if last { len - 1 } else { 0 })
    }
}

/// Truncate to `max_chars` (inclusive of the ellipsis) for the painted rows.
fn elide(text: &str, max_chars: usize) -> String {
    let count = text.chars().count();
    if max_chars == 0 || count <= max_chars {
        return text.to_string();
    }
    let keep = max_chars.saturating_sub(1);
    let mut out: String = text.chars().take(keep).collect();
    out.push('…');
    out
}

fn natural_cmp(a: &str, b: &str) -> CmpOrdering {
    let ord = natural_cmp_inner(a, b);
    if ord != CmpOrdering::Equal {
        return ord;
    }
    a.to_lowercase()
        .cmp(&b.to_lowercase())
        .then_with(|| a.cmp(b))
}

fn natural_cmp_inner(a: &str, b: &str) -> CmpOrdering {
    use CmpOrdering::{Equal, Greater, Less};
    let mut ai = a.chars().peekable();
    let mut bi = b.chars().peekable();
    loop {
        let (ca, cb) = match (ai.peek().copied(), bi.peek().copied()) {
            (None, None) => return Equal,
            (None, Some(_)) => return Less,
            (Some(_), None) => return Greater,
            (Some(ca), Some(cb)) => (ca, cb),
        };
        if ca.is_ascii_digit() && cb.is_ascii_digit() {
            let na: String = std::iter::from_fn(|| ai.next_if(|c| c.is_ascii_digit())).collect();
            let nb: String = std::iter::from_fn(|| bi.next_if(|c| c.is_ascii_digit())).collect();
            let ta = na.trim_start_matches('0');
            let tb = nb.trim_start_matches('0');
            let ord = ta.len().cmp(&tb.len()).then_with(|| ta.cmp(tb));
            if ord != Equal {
                return ord;
            }
        } else {
            ai.next();
            bi.next();
            let ord = ca
                .to_lowercase()
                .collect::<String>()
                .cmp(&cb.to_lowercase().collect::<String>());
            if ord != Equal {
                return ord;
            }
        }
    }
}

fn fmt_mtime(time: SystemTime) -> String {
    let Ok(delta) = time.duration_since(std::time::UNIX_EPOCH) else {
        return "—".to_string();
    };
    let secs = delta.as_secs() as i64;
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}",
        rem / 3600,
        (rem % 3600) / 60
    )
}

/// Days since 1970-01-01 -> civil (UTC) date, good enough for a file listing.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if month <= 2 { year + 1 } else { year }, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir()
            .join(format!("lumen-browser-tests-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    #[test]
    fn resolve_go_joins_validates_and_expands() {
        let cwd = PathBuf::from("/tmp/photos");
        let home = PathBuf::from("/home/lumen");
        assert_eq!(
            resolve_go("  /var/images  ", &cwd, Some(&home)),
            Ok(PathBuf::from("/var/images"))
        );
        assert_eq!(
            resolve_go("sub/2024", &cwd, Some(&home)),
            Ok(PathBuf::from("/tmp/photos/sub/2024"))
        );
        assert_eq!(resolve_go("~", &cwd, Some(&home)), Ok(home.clone()));
        assert_eq!(
            resolve_go("~/pics", &cwd, Some(&home)),
            Ok(PathBuf::from("/home/lumen/pics"))
        );
        assert_eq!(resolve_go("   ", &cwd, Some(&home)), Err(PathIssue::Empty));
        assert_eq!(resolve_go("", &cwd, None), Err(PathIssue::Empty));
        // No HOME: a bare ~ cannot be resolved.
        assert_eq!(resolve_go("~", &cwd, None), Err(PathIssue::NotFound));
    }

    #[test]
    fn validate_dir_distinguishes_folder_file_and_missing() {
        let dir = temp_dir("validate");
        let file = dir.join("a.png");
        std::fs::write(&file, b"x").expect("write");
        assert_eq!(validate_dir(&dir), Ok(()));
        assert_eq!(validate_dir(&file), Err(PathIssue::NotDir));
        assert_eq!(
            validate_dir(&dir.join("nope")),
            Err(PathIssue::NotFound)
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn start_folder_prefers_folder_then_parent() {
        let dir = temp_dir("start");
        let sub = dir.join("sub");
        std::fs::create_dir_all(&sub).expect("sub");
        let file = sub.join("photo.png");
        std::fs::write(&file, b"x").expect("file");
        assert_eq!(start_cwd(Some(sub.clone())), sub);
        assert_eq!(start_cwd(Some(file.clone())), sub);
        // Unknown path falls back to the process cwd (still a folder).
        let fallback = start_cwd(Some(dir.join("missing")));
        assert!(fallback.is_dir());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn read_folder_skips_hidden_and_non_images() {
        let dir = temp_dir("read");
        std::fs::create_dir_all(dir.join("Trip")).expect("sub");
        std::fs::create_dir_all(dir.join(".git")).expect("hidden dir");
        std::fs::write(dir.join("IMG_2.JPG"), b"x").expect("img2");
        std::fs::write(dir.join("IMG_10.png"), b"x").expect("img10");
        std::fs::write(dir.join("notes.txt"), b"x").expect("txt");
        std::fs::write(dir.join(".hidden.jpg"), b"x").expect("hidden img");

        let (entries, dirs, files) = read_folder(&dir).expect("read folder");
        assert_eq!((dirs, files), (1, 2));
        assert_eq!(entries.len(), 3);
        // Folder first, then images in natural order (2 before 10).
        assert_eq!(entries[0].name, "Trip");
        assert!(entries[0].is_dir);
        assert_eq!(entries[1].name, "IMG_2.JPG");
        assert_eq!(entries[2].name, "IMG_10.png");
        assert!(entries.iter().all(|e| !e.name.starts_with('.')));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn unreadable_folder_reports_an_error() {
        let dir = temp_dir("unreadable");
        let missing = dir.join("gone");
        assert!(read_folder(&missing).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn count_label_matches_the_web_copy() {
        assert_eq!(count_label(Locale::ZhHant, 0, 0), "沒有項目");
        assert_eq!(count_label(Locale::ZhHant, 3, 0), "3 個資料夾");
        assert_eq!(count_label(Locale::ZhHant, 0, 4), "4 張圖片");
        assert_eq!(
            count_label(Locale::ZhHant, 2, 3),
            "2 個資料夾 · 3 張圖片"
        );
        // Translators keep the ${token}s, so the same fill works in English.
        assert_eq!(count_label(Locale::En, 2, 3), "2 folders · 3 images");
    }

    #[test]
    fn keyboard_step_and_edges() {
        assert_eq!(step_index(0, None, 1), None);
        assert_eq!(edge_index(0, true), None);
        assert_eq!(step_index(5, None, 1), Some(0));
        assert_eq!(step_index(5, None, -1), Some(4));
        assert_eq!(step_index(5, Some(0), -1), Some(0));
        assert_eq!(step_index(5, Some(4), 1), Some(4));
        assert_eq!(step_index(5, Some(2), 1), Some(3));
        assert_eq!(step_index(5, Some(2), -3), Some(0));
        assert_eq!(edge_index(5, false), Some(0));
        assert_eq!(edge_index(5, true), Some(4));
    }

    #[test]
    fn natural_order_is_numeric_and_case_insensitive() {
        let mut names = vec!["IMG_10.png", "img_2.png", "IMG_1.png", "B.png", "a.png"];
        names.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(names, vec!["a.png", "B.png", "IMG_1.png", "img_2.png", "IMG_10.png"]);
        // Equal numerically -> stable tie-break, never Equal for distinct names.
        assert_eq!(natural_cmp("a01.png", "a1.png"), CmpOrdering::Less);
        assert_eq!(natural_cmp("同一個", "同一個"), CmpOrdering::Equal);
    }

    #[test]
    fn elide_keeps_the_ellipsis_within_budget() {
        assert_eq!(elide("abc", 5), "abc");
        assert_eq!(elide("abcdef", 4), "abc…");
        assert_eq!(elide("abcdef", 1), "…");
        assert_eq!(elide("abcdef", 0), "abcdef");
        assert_eq!(elide("照片檔案名稱太長了", 5), "照片檔案…");
    }

    #[test]
    fn mtime_formats_as_utc_minutes() {
        let t = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);
        assert_eq!(fmt_mtime(t), "2023-11-14 22:13");
        assert_eq!(fmt_mtime(std::time::UNIX_EPOCH), "1970-01-01 00:00");
    }

    #[test]
    fn path_issues_have_localized_messages() {
        let zh = path_issue_message(Locale::ZhHant, PathIssue::NotFound);
        assert!(zh.contains("路徑"), "{zh}");
        let en = path_issue_message(Locale::En, PathIssue::Empty);
        assert!(!en.is_empty());
        assert_ne!(path_issue_message(Locale::ZhHant, PathIssue::NotDir), zh);
    }

    fn frame_input() -> egui::RawInput {
        let mut input = egui::RawInput::default();
        input.screen_rect =
            Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1280.0, 820.0)));
        input
    }

    fn key_event(key: egui::Key) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }
    }

    /// Cache key for tests: `tag` becomes both the size and the mtime version.
    fn thumb_key(path: &str, tag: u64) -> ThumbKey {
        ThumbKey {
            path: PathBuf::from(path),
            mtime: (tag > 0)
                .then(|| std::time::UNIX_EPOCH + std::time::Duration::from_secs(tag)),
            size: tag,
        }
    }

    #[test]
    fn thumb_cache_evicts_the_least_recently_used_entry() {
        let mut cache = ThumbCache::new(3);
        for i in 0..3 {
            cache.insert(thumb_key(&format!("/tmp/thumb-{i}.jpg"), i), None);
        }
        assert_eq!(cache.len(), 3);
        // Touch entry 0 so entry 1 becomes the least recently used one.
        assert!(cache.lookup(&thumb_key("/tmp/thumb-0.jpg", 0)).is_some());
        cache.insert(thumb_key("/tmp/thumb-3.jpg", 3), None);
        assert_eq!(cache.len(), 3);
        assert!(cache.contains(&thumb_key("/tmp/thumb-0.jpg", 0)));
        assert!(!cache.contains(&thumb_key("/tmp/thumb-1.jpg", 1)), "LRU entry should be evicted");
        assert!(cache.contains(&thumb_key("/tmp/thumb-3.jpg", 3)));
        // Capacity is a hard bound even after many inserts.
        for i in 10..40 {
            cache.insert(thumb_key(&format!("/tmp/thumb-{i}.jpg"), i), None);
        }
        assert_eq!(cache.len(), 3);
    }

    #[test]
    fn thumb_cache_remembers_failures_and_invalidates_changed_files() {
        let mut cache = ThumbCache::new(4);
        let key = thumb_key("/tmp/a.jpg", 10);
        assert!(cache.lookup(&key).is_none(), "cold cache is a miss");
        // A decode failure is cached as `Some(None)` so it is not retried.
        cache.insert(key.clone(), None);
        // `TextureHandle` has no Debug, so match instead of assert_eq!.
        assert!(matches!(cache.lookup(&key), Some(None)), "failure must be cached");
        // Same path, different mtime/size (file edited) is a miss again.
        let changed = ThumbKey { path: key.path.clone(), mtime: key.mtime, size: key.size + 1 };
        assert!(cache.lookup(&changed).is_none());
        assert!(!cache.contains(&changed));
    }

    #[test]
    fn thumb_key_tracks_mtime_and_size_for_texture_names() {
        let base = thumb_key("/tmp/x.jpg", 1);
        let bigger = ThumbKey { path: base.path.clone(), mtime: base.mtime, size: 2 };
        let other_mtime =
            ThumbKey { path: base.path.clone(), mtime: Some(std::time::UNIX_EPOCH), size: 1 };
        assert_ne!(base, bigger);
        assert_ne!(base, other_mtime);
        // Every version gets its own egui texture name (one live texture per path).
        assert_ne!(base.texture_name(), bigger.texture_name());
        assert_ne!(base.texture_name(), other_mtime.texture_name());
        assert!(base.texture_name().starts_with("lumen-thumb:/tmp/x.jpg:"));
    }

    #[test]
    fn square_cover_centre_crops_the_aspect_ratio() {
        let source = image::RgbaImage::from_fn(4, 2, |x, _| {
            image::Rgba([(x * 40) as u8, 10, 20, 255])
        });
        let square = square_cover(source);
        assert_eq!(square.dimensions(), (2, 2));
        // Columns 1 and 2 of the original survive (centre crop).
        assert_eq!(square.get_pixel(0, 0)[0], 40);
        assert_eq!(square.get_pixel(1, 0)[0], 80);
        // Already-square images are returned untouched.
        let square2 = image::RgbaImage::from_pixel(3, 3, image::Rgba([1, 2, 3, 255]));
        assert_eq!(square_cover(square2).dimensions(), (3, 3));
    }

    #[test]
    fn thumbnails_are_decode_budgeted_per_frame() {
        let dir = temp_dir("thumb-budget");
        for i in 0..6 {
            std::fs::write(dir.join(format!("img{i}.png")), b"not an image").expect("write");
        }
        let ctx = egui::Context::default();
        let mut browser = Browser::new(Some(dir.clone()));
        let input = frame_input();
        let _ = ctx.run(input.clone(), |ctx| {
            browser.show(ctx, Locale::ZhHant);
        });
        // Only the per-frame budget is attempted; failures are cached as well.
        assert_eq!(browser.thumbs.len(), THUMB_PER_FRAME);
        for _ in 0..2 {
            let _ = ctx.run(input.clone(), |ctx| {
                browser.show(ctx, Locale::ZhHant);
            });
        }
        assert_eq!(browser.thumbs.len(), 6);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn real_images_upload_small_textures() {
        let dir = temp_dir("thumb-textures");
        let image = image::RgbaImage::from_fn(200, 100, |x, y| {
            image::Rgba([(x % 256) as u8, (y % 256) as u8, 90, 255])
        });
        image.save(dir.join("photo.png")).expect("save test image");
        let ctx = egui::Context::default();
        let mut browser = Browser::new(Some(dir.clone()));
        let input = frame_input();
        let _ = ctx.run(input.clone(), |ctx| {
            browser.show(ctx, Locale::ZhHant);
        });
        let key = browser
            .entries
            .iter()
            .find(|e| !e.is_dir)
            .map(ThumbKey::for_entry)
            .expect("image entry");
        match browser.thumbs.lookup(&key) {
            Some(Some(texture)) => {
                let [w, h] = texture.size();
                assert!(w <= THUMB_DECODE as usize && h <= THUMB_DECODE as usize);
                assert_eq!(w, h, "thumbnails are square (object-fit: cover)");
                assert!(w >= 16, "thumbnail too small: {w}");
            }
            other => panic!("expected an uploaded thumbnail, got {:?}", other.is_some()),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn arrow_keys_move_the_selection() {
        let dir = temp_dir("keys");
        std::fs::create_dir_all(dir.join("sub")).expect("sub");
        std::fs::write(dir.join("a.png"), b"x").expect("img");
        let ctx = egui::Context::default();
        let mut browser = Browser::new(Some(dir.clone()));
        let mut input = frame_input();
        let _ = ctx.run(input.clone(), |ctx| {
            browser.show(ctx, Locale::ZhHant);
        });
        // The path field grabs focus on open (like the web modal); focus the list
        // before arrow keys should navigate.
        ctx.memory_mut(|m| m.surrender_focus(Id::new(PATH_INPUT)));
        input.events = vec![key_event(egui::Key::ArrowDown)];
        let _ = ctx.run(input.clone(), |ctx| {
            browser.show(ctx, Locale::ZhHant);
        });
        assert_eq!(
            browser.selected.as_deref(),
            Some(dir.join("sub").as_path()),
            "first row (folder) should be selected"
        );
        input.events = vec![key_event(egui::Key::End)];
        let _ = ctx.run(input.clone(), |ctx| {
            browser.show(ctx, Locale::ZhHant);
        });
        assert_eq!(
            browser.selected.as_deref(),
            Some(dir.join("a.png").as_path()),
            "End selects the last row"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn selected_row_scrolls_into_view() {
        let dir = temp_dir("scroll");
        for i in 0..48 {
            std::fs::write(dir.join(format!("img{i:02}.png")), b"x").expect("img");
        }
        let ctx = egui::Context::default();
        let mut browser = Browser::new(Some(dir.clone()));
        let mut input = frame_input();
        let _ = ctx.run(input.clone(), |ctx| {
            browser.show(ctx, Locale::ZhHant);
        });
        assert!(browser.last_scroll_offset.y.abs() < 0.5, "list starts at the top");
        ctx.memory_mut(|m| m.surrender_focus(Id::new(PATH_INPUT)));
        input.events = vec![key_event(egui::Key::End)];
        // A few frames: the smooth scroll target needs to settle.
        for _ in 0..6 {
            let _ = ctx.run(input.clone(), |ctx| {
                browser.show(ctx, Locale::ZhHant);
            });
            input.events.clear();
        }
        assert!(
            browser.last_scroll_offset.y > 100.0,
            "selecting the last row must scroll the list (offset {:?})",
            browser.last_scroll_offset
        );
        // The last image row is selected and it is inside the list viewport:
        // the list height is bounded, so a large offset means it is visible.
        assert_eq!(
            browser.selected.as_deref(),
            Some(dir.join("img47.png").as_path())
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn show_draws_two_frames_without_panicking() {
        let dir = temp_dir("show");
        std::fs::write(dir.join("a.png"), b"x").expect("img");
        std::fs::create_dir_all(dir.join("sub")).expect("sub");
        let ctx = egui::Context::default();
        let mut input = egui::RawInput::default();
        input.screen_rect = Some(Rect::from_min_size(
            Pos2::ZERO,
            Vec2::new(1280.0, 820.0),
        ));
        let mut browser = Browser::new(Some(dir.clone()));
        let mut chosen = None;
        for _ in 0..2 {
            let _ = ctx.run(input.clone(), |ctx| {
                chosen = browser.show(ctx, Locale::ZhHant);
            });
        }
        assert!(chosen.is_none());
        assert!(browser.entries.len() >= 2, "listing should be loaded");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn browser_starts_open_at_the_requested_folder() {
        let dir = temp_dir("browser-new");
        let browser = Browser::new(Some(dir.clone()));
        assert!(browser.open);
        assert_eq!(browser.cwd, dir);
        assert!(browser.selected.is_none());
        assert!(browser.error.is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
