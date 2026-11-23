// Empty-editor state: when no document is loaded the web build still shows the
// full editor chrome (top bar / rail / inspector / status bar / floating zoom
// HUD) plus a centred drop card. This module owns the card and the floating
// HUD; the other chrome modules switch to their edit variants when doc is None.

use egui::{
    Align2, Color32, CornerRadius, FontId, Id, Order, Pos2, Rect, Sense, Stroke, StrokeKind, Ui,
    Vec2,
};

use crate::state::{AppState, UiAction};
use crate::theme;

use super::local;
use super::motion;
use super::widgets::{self, Icon};
use super::zoom;

const CARD_W: f32 = 430.0;
const CARD_PAD_X: f32 = 30.0;
const INTRO: f32 = 0.18;

pub fn show(ctx: &egui::Context, state: &mut AppState) {
    if state.doc.is_some() {
        // Allow the intro to replay after a document was loaded and closed.
        motion::reset_intro(ctx, Id::new("empty-card-intro"));
        return;
    }
    let t = motion::intro(ctx, Id::new("empty-card-intro"), INTRO);
    let screen = ctx.screen_rect();
    let collapsed = local::get(ctx, |l| l.inspector_collapsed);
    let canvas = Rect::from_min_max(
        Pos2::new(screen.left() + theme::RAIL_W, screen.top() + theme::TOPBAR_H),
        Pos2::new(
            screen.right() - if collapsed { 0.0 } else { theme::PANEL_W },
            screen.bottom() - theme::STATUS_H,
        ),
    );

    // Opaque canvas cover: hides the canvas' own "not loaded" placeholder so the
    // card sits on an empty stage exactly like `#empty-state` in the web build.
    egui::Area::new(Id::new("empty-scrim"))
        .order(Order::Foreground)
        .fixed_pos(canvas.left_top())
        .interactable(false)
        .show(ctx, |ui| {
            let (rect, _) = ui.allocate_exact_size(canvas.size(), Sense::hover());
            ui.painter().rect_filled(rect, 0.0, theme::CANVAS);
        });

    // Card (~430px wide, 12px radius, fades/slides in over 180 ms).
    let card_h = 254.0;
    let card_pos = Pos2::new(
        canvas.center().x - CARD_W / 2.0,
        canvas.center().y - card_h / 2.0 + motion::slide_offset(t, 6.0),
    );
    egui::Area::new(Id::new("empty-card"))
        .order(Order::Foreground)
        .fixed_pos(card_pos)
        .show(ctx, |ui| {
            ui.set_opacity(t);
            egui::Frame::new()
                .fill(theme::PANEL)
                .stroke(Stroke::new(1.0_f32, widgets::white(36)))
                .corner_radius(CornerRadius::same(12))
                .inner_margin(egui::Margin::symmetric(CARD_PAD_X as i8, 28))
                .show(ui, |ui| {
                    let content_w = CARD_W - CARD_PAD_X * 2.0;
                    ui.set_width(content_w);
                    empty_card_content(ui, state, t, content_w);
                });
        });

    // Floating zoom HUD in the canvas corner (disabled without a document).
    egui::Area::new(Id::new("empty-zoom"))
        .order(Order::Foreground)
        .fixed_pos(Pos2::new(
            canvas.right() - 14.0 - 140.0,
            canvas.bottom() - 14.0 - 40.0,
        ))
        .show(ctx, |ui| {
            zoom::floating_frame().show(ui, |ui| {
                zoom::stack(ui, state, false);
            });
        });
}

fn empty_card_content(ui: &mut Ui, state: &mut AppState, _t: f32, content_w: f32) {
    let t = state.locale;
    // Explicit add_space() controls the rhythm; no implicit layout gaps so the
    // card height matches the web's 28/24px padding + fixed blocks.
    ui.spacing_mut().item_spacing = Vec2::ZERO;
    ui.vertical_centered(|ui| {
        // Drop icon (56×56 rounded square).
        let (icon_rect, _) = ui.allocate_exact_size(Vec2::splat(56.0), Sense::hover());
        ui.painter().rect_filled(
            icon_rect,
            CornerRadius::same(12),
            widgets::SURFACE,
        );
        widgets::paint_icon(
            ui.painter(),
            Rect::from_center_size(icon_rect.center(), Vec2::splat(30.0)),
            Icon::Brand,
            theme::DIM,
            1.8,
        );
        ui.add_space(12.0);

        // 拖放圖片到這裡
        let (rect, _) = ui.allocate_exact_size(Vec2::new(content_w, 24.0), Sense::hover());
        let painter = ui.painter();
        let heading = t.tr("拖放圖片到這裡");
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            heading.clone(),
            FontId::proportional(16.0),
            theme::TEXT,
        );
        // Faux weight-600 (egui has no bold face for CJK here).
        painter.text(
            rect.center() + Vec2::new(0.4, 0.0),
            Align2::CENTER_CENTER,
            heading,
            FontId::proportional(16.0),
            theme::TEXT,
        );
        ui.add_space(6.0);

        // Subtext, wrapped and centred.
        let sub = t.tr("或從資料夾開啟圖片。支援 JPEG、PNG、WebP、GIF、SVG。");
        let galley = ui.painter().layout(
            sub,
            FontId::proportional(11.5),
            theme::DIM,
            content_w,
        );
        let (sub_rect, _) = ui.allocate_exact_size(
            Vec2::new(content_w, galley.size().y.max(18.0)),
            Sense::hover(),
        );
        ui.painter().galley(
            Pos2::new(sub_rect.center().x - galley.size().x / 2.0, sub_rect.top()),
            galley,
            theme::DIM,
        );
        ui.add_space(16.0);

        // Actions: 開啟圖片… / 查看快捷鍵, centred as a pair.
        let open_label = t.tr("開啟圖片…");
        let shortcuts_label = t.tr("查看快捷鍵");
        let open_w = measure(ui, &open_label, 11.5) + 24.0;
        let shortcuts_w = measure(ui, &shortcuts_label, 11.5) + 24.0;
        let total = open_w + 8.0 + shortcuts_w;
        let (row_rect, _) =
            ui.allocate_exact_size(Vec2::new(content_w, 32.0), Sense::hover());
        let mut x = row_rect.center().x - total / 2.0;
        let open_rect = Rect::from_min_size(Pos2::new(x, row_rect.top()), Vec2::new(open_w, 32.0));
        let open_resp = ui.interact(
            open_rect,
            ui.id().with("empty-open"),
            Sense::click(),
        );
        paint_action(ui, open_rect, &open_label, true, open_resp.hovered());
        x += open_w + 8.0;
        let shortcuts_rect =
            Rect::from_min_size(Pos2::new(x, row_rect.top()), Vec2::new(shortcuts_w, 32.0));
        let shortcuts_resp = ui.interact(
            shortcuts_rect,
            ui.id().with("empty-shortcuts"),
            Sense::click(),
        );
        paint_action(
            ui,
            shortcuts_rect,
            &shortcuts_label,
            false,
            shortcuts_resp.hovered(),
        );
        if open_resp.clicked() {
            state.push_action(UiAction::Open);
        }
        if shortcuts_resp.clicked() {
            state.show_shortcuts = true;
        }
        ui.add_space(16.0);

        // Ctrl + O hint row.
        let hint = t.tr("開啟 · 也可以直接拖放檔案");
        let kbd_ctrl = kbd_width(ui, "Ctrl");
        let plus_w = measure(ui, "+", 10.5);
        let kbd_o = kbd_width(ui, "O");
        let hint_w = measure(ui, &hint, 10.5);
        let total = kbd_ctrl + 4.0 + plus_w + 4.0 + kbd_o + 4.0 + hint_w;
        let (hint_rect, _) =
            ui.allocate_exact_size(Vec2::new(content_w, 22.0), Sense::hover());
        let mut x = hint_rect.center().x - total / 2.0;
        let cy = hint_rect.center().y;
        let ctrl_rect =
            Rect::from_min_size(Pos2::new(x, cy - 10.5), Vec2::new(kbd_ctrl, 21.0));
        paint_kbd(ui, ctrl_rect, "Ctrl");
        x += kbd_ctrl + 4.0;
        ui.painter().text(
            Pos2::new(x, cy),
            Align2::LEFT_CENTER,
            "+",
            FontId::proportional(10.5),
            widgets::white(110),
        );
        x += plus_w + 4.0;
        let o_rect = Rect::from_min_size(Pos2::new(x, cy - 10.5), Vec2::new(kbd_o, 21.0));
        paint_kbd(ui, o_rect, "O");
        x += kbd_o + 4.0;
        ui.painter().text(
            Pos2::new(x, cy),
            Align2::LEFT_CENTER,
            hint,
            FontId::proportional(10.5),
            widgets::white(110),
        );
    });
}

fn measure(ui: &Ui, text: &str, size: f32) -> f32 {
    ui.painter()
        .layout_no_wrap(text.to_owned(), FontId::proportional(size), theme::TEXT)
        .size()
        .x
}

fn kbd_width(ui: &Ui, label: &str) -> f32 {
    measure(ui, label, 9.5).max(15.0) + 12.0
}

fn paint_kbd(ui: &Ui, rect: Rect, label: &str) {
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(5), widgets::SURFACE_2);
    painter.rect_stroke(
        rect,
        CornerRadius::same(5),
        Stroke::new(1.0_f32, widgets::white(46)),
        StrokeKind::Inside,
    );
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        label,
        FontId::monospace(9.5),
        theme::TEXT,
    );
}

fn paint_action(ui: &Ui, rect: Rect, label: &str, primary: bool, hovered: bool) {
    let painter = ui.painter();
    let radius = CornerRadius::same(8);
    let (fill, stroke, color) = if primary {
        (
            if hovered { Color32::WHITE } else { theme::TEXT },
            Stroke::NONE,
            theme::BG,
        )
    } else if hovered {
        (
            widgets::white(15),
            Stroke::new(1.0_f32, widgets::white(52)),
            theme::TEXT,
        )
    } else {
        (
            Color32::TRANSPARENT,
            Stroke::new(1.0_f32, widgets::white(36)),
            theme::DIM,
        )
    };
    painter.rect_filled(rect, radius, fill);
    if stroke != Stroke::NONE {
        painter.rect_stroke(rect, radius, stroke, StrokeKind::Inside);
    }
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        label,
        FontId::proportional(11.5),
        color,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two headless frames of the whole no-document chrome (the same modules
    /// app.rs drives) must paint without panicking and keep producing shapes.
    #[test]
    fn empty_state_renders_two_frames_headless() {
        let ctx = egui::Context::default();
        ctx.set_visuals(egui::Visuals::dark());
        let mut state = AppState::new(crate::i18n::Locale::ZhHant);
        let mut shapes = Vec::new();
        for frame in 0..2 {
            let mut input = egui::RawInput::default();
            input.screen_rect =
                Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1280.0, 820.0)));
            input.time = Some(frame as f64 / 60.0);
            let out = ctx.run(input, |ctx| {
                crate::ui::topbar::show(ctx, &mut state);
                crate::ui::rail::show(ctx, &mut state);
                crate::ui::inspector::show(ctx, &mut state);
                crate::ui::dock::show(ctx, &mut state);
                crate::ui::status::show(ctx, &mut state);
                crate::ui::info::show(ctx, &mut state);
                crate::ui::shortcuts::show(ctx, &mut state);
            });
            shapes.push(out.shapes.len());
        }
        assert!(shapes[0] > 20, "first frame painted too little: {shapes:?}");
        assert!(shapes[1] > 20, "second frame painted too little: {shapes:?}");
        // The state stays in the no-document preview phase.
        assert!(state.doc.is_none());
    }
}
