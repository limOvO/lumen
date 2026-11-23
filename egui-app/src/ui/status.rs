// Editor status bar: file info on the left, transient message in the centre,
// zoom + position on the right (26px, full window width like the web build).
//
// The real layout is reserved by a bottom panel; the visible bar is painted as
// a full-width overlay so it also covers the left rail / inspector tails (those
// panels are created before this one in app.rs and span the full window).

use egui::{Align2, FontId, Id, Order, Pos2, Rect, Sense, Stroke, Vec2};

use crate::state::{AppState, Phase, StatusKind};
use crate::theme;

use super::format::fmt_bytes;
use super::widgets;

pub fn show(ctx: &egui::Context, state: &mut AppState) {
    // body.no-doc keeps the status bar visible (with dashes).
    if state.phase != Phase::Edit && state.doc.is_some() {
        return;
    }
    egui::TopBottomPanel::bottom("statusbar")
        .exact_height(theme::STATUS_H)
        .show_separator_line(false)
        .frame(
            egui::Frame::new()
                .fill(widgets::RAIL)
                .inner_margin(egui::Margin::ZERO),
        )
        .show(ctx, |_ui| {});

    let screen = ctx.screen_rect();
    let bar = Rect::from_min_size(
        Pos2::new(screen.left(), screen.bottom() - theme::STATUS_H),
        Vec2::new(screen.width(), theme::STATUS_H),
    );
    // Auto-clear expired status messages before painting the overlay.
    if let Some(msg) = state.status.clone() {
        let now = ctx.input(|i| i.time);
        let lifetime = if msg.kind == StatusKind::Err { 6.0 } else { 4.5 };
        if now - msg.born > lifetime {
            state.status = None;
        }
    }
    let message = state.status.clone();
    let left = state
        .doc
        .as_ref()
        .map(|doc| {
            format!(
                "{} × {} · {} · {}",
                doc.width,
                doc.height,
                doc.format,
                fmt_bytes(doc.bytes)
            )
        })
        .unwrap_or_else(|| "—".to_string());
    let right = if state.doc.is_some() {
        let pct = format!("{:.0}%", state.view.scale * 100.0);
        let count = format!("{} / {}", state.index + 1, state.siblings.len().max(1));
        format!("{pct} · {count}")
    } else {
        "—".to_string()
    };
    let font = FontId::proportional(10.5);

    egui::Area::new(Id::new("statusbar-overlay"))
        .order(Order::Middle)
        .fixed_pos(bar.left_top())
        .show(ctx, |ui| {
            let (rect, _) = ui.allocate_exact_size(bar.size(), Sense::hover());
            let painter = ui.painter();
            painter.rect_filled(rect, 0.0, widgets::RAIL);
            painter.hline(
                rect.x_range(),
                rect.top() + 0.5,
                Stroke::new(1.0_f32, widgets::white(22)),
            );
            let cy = rect.center().y;
            painter.text(
                Pos2::new(rect.left() + 12.0, cy),
                Align2::LEFT_CENTER,
                left,
                font.clone(),
                widgets::white(150),
            );
            if let Some(msg) = &message {
                let color = match msg.kind {
                    StatusKind::Info => theme::DIM,
                    StatusKind::Ok => theme::OK,
                    StatusKind::Warn => theme::WARN,
                    StatusKind::Err => theme::ERR,
                };
                painter.text(
                    Pos2::new(rect.center().x, cy),
                    Align2::CENTER_CENTER,
                    msg.text.clone(),
                    font.clone(),
                    color,
                );
            }
            painter.text(
                Pos2::new(rect.right() - 12.0, cy),
                Align2::RIGHT_CENTER,
                right,
                font,
                widgets::white(150),
            );
        });
}
