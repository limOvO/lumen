// Canvas gestures: pan/zoom, annotation creation/selection, crop manipulation.
// Owner: annotations teammate. `app.rs` calls `handle_canvas` once per frame.
//
// Parity reference: src/js/core/interaction.js. Every gesture the web build
// supports is implemented here on top of egui's raw pointer input:
//   preview: wheel zoom at the cursor (min = fit), drag pan only when zoomed,
//            double-click fits;
//   edit:    brush (velocity -> width), line/arrow (two-click or drag, Esc
//            cancels, Shift snaps 45 deg), rect/ellipse (Shift = square/circle),
//            text (inline egui editor), crop (new/move/8-handle resize, Shift
//            locks 1:1), selection (2 handles for line/arrow, 8 otherwise) with
//            move/resize, and pan from empty space (or Space/middle anywhere).
//
// History contract: `state.commit()` runs exactly once before the first
// mutation of each gesture, so one undo step restores the pre-gesture state.

use std::cell::RefCell;

use egui::{Color32, CursorIcon, Id, Key, PointerButton, Pos2, Rect, Sense, Stroke, Vec2};

use crate::annotate::{brush_width, hit_topmost, Annot, AnnotKind, Handle, Style};
use crate::crop::{CropHandle, CropRect};
use crate::state::{AppState, Phase, Tool};
use crate::theme;

// Screen-pixel thresholds (parity with the web build).
const PAN_THRESHOLD: f32 = 3.0;
const ARROW_CLICK_MIN: f32 = 3.0;
const ARROW_DRAG_MIN: f32 = 6.0;
const HANDLE_SIZE: f32 = 8.0;
const HANDLE_HIT: f32 = 11.0;
const DBLCLICK_SECS: f64 = 0.380;
const MIN_SHAPE_SCREEN: f32 = 3.0;
const MIN_CROP_SCREEN: f32 = 8.0;
const HIT_TOL_PX: f32 = 6.0;
const MIN_SCALE: f32 = 0.02;
const MAX_SCALE: f32 = 16.0;

pub struct CanvasView {
    pub rect: Rect,
    /// image px -> screen point
    pub to_screen: Box<dyn Fn(Pos2) -> Pos2>,
    /// screen point -> image px
    pub to_image: Box<dyn Fn(Pos2) -> Pos2>,
    pub zoom: f32,
}

// ---------------------------------------------------------------------------
// Gesture state (lives across frames; one window per process).
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum PanButton {
    Primary,
    Middle,
    Space,
}

enum Drag {
    Pan {
        start: Pos2,
        offset0: Vec2,
        moved: bool,
        deselect_on_click: bool,
        button: PanButton,
    },
    Brush {
        last_screen: Pos2,
        last_time: f64,
    },
    Shape {
        start_img: Pos2,
        start_screen: Pos2,
    },
    ArrowDrag {
        start_screen: Pos2,
        start_img: Pos2,
    },
    MoveObj {
        obj: Annot,
        origin_img: Pos2,
        moved: bool,
        committed: bool,
    },
    Resize {
        obj: Annot,
        handle: Handle,
        start_screen: Pos2,
        moved: bool,
        committed: bool,
    },
    CropNew {
        img0: Pos2,
        start_screen: Pos2,
        prev: Option<CropRect>,
        ratio: Option<f32>,
        committed: bool,
    },
    CropMove {
        rect0: CropRect,
        img0: Pos2,
        start_screen: Pos2,
        prev: Option<CropRect>,
        moved: bool,
        committed: bool,
    },
    CropResize {
        rect0: CropRect,
        handle: CropHandle,
        start_screen: Pos2,
        prev: Option<CropRect>,
        ratio: Option<f32>,
        moved: bool,
        committed: bool,
    },
}

struct PendingArrow {
    obj: Annot,
    start_screen: Pos2,
}

struct TextEditor {
    screen_pos: Pos2,
    image_pos: Pos2,
    buffer: String,
    style: Style,
    existing: Option<u64>,
    focused: bool,
}

#[derive(Default)]
struct GestureState {
    drag: Option<Drag>,
    brush: Option<Annot>,
    shape: Option<Annot>,
    arrow: Option<PendingArrow>,
    editor: Option<TextEditor>,
    hover: Option<Pos2>,
    last_click: (f64, Pos2),
    last_phase: Option<Phase>,
    last_tool: Option<Tool>,
    last_doc: Option<(u32, u32, String)>,
}

thread_local! {
    static GESTURE: RefCell<GestureState> = RefCell::new(GestureState::default());
}

/// True while a gesture owns the Escape key (pending arrow or text editor).
/// `app.rs` should skip its own Escape handling in that case.
pub fn wants_escape() -> bool {
    GESTURE.with(|c| {
        let g = c.borrow();
        g.editor.is_some() || g.arrow.is_some()
    })
}

/// True while the inline text editor has the keyboard: `app.rs` must not treat
/// Delete/Backspace/Ctrl+Z as canvas shortcuts.
pub fn text_editing() -> bool {
    GESTURE.with(|c| c.borrow().editor.is_some())
}

// ---------------------------------------------------------------------------
// Pure helpers (unit-tested without an egui context).
// ---------------------------------------------------------------------------

fn doc_size(state: &AppState) -> Option<(f32, f32)> {
    state.doc.as_ref().map(|d| (d.width as f32, d.height as f32))
}

fn doc_dims(state: &AppState) -> Option<(u32, u32)> {
    state.doc.as_ref().map(|d| (d.width, d.height))
}

/// Fit scale, same formula the canvas paints with (small images stay at 100%).
fn fit_scale_of(doc: Vec2, viewport: Vec2) -> f32 {
    if doc.x <= 0.0 || doc.y <= 0.0 {
        return 1.0;
    }
    (viewport.x / doc.x).min(viewport.y / doc.y).min(1.0).clamp(MIN_SCALE, MAX_SCALE)
}

/// Preview pan clamp: keep the image inside the viewport (centred when it is
/// smaller on an axis). `offset` is relative to the viewport centre.
fn clamp_offset_to_view(offset: Vec2, viewport: Vec2, content: Vec2) -> Vec2 {
    let mut out = offset;
    if content.x <= viewport.x {
        out.x = 0.0;
    } else {
        let limit = (content.x - viewport.x) / 2.0;
        out.x = out.x.clamp(-limit, limit);
    }
    if content.y <= viewport.y {
        out.y = 0.0;
    } else {
        let limit = (content.y - viewport.y) / 2.0;
        out.y = out.y.clamp(-limit, limit);
    }
    out
}

/// Snap `end` to the nearest 45 degrees around `start`, keeping the length.
fn snap_angle(start: Pos2, end: Pos2) -> Pos2 {
    let d = end - start;
    let len = d.length();
    if len < 1e-6 {
        return end;
    }
    let step = std::f32::consts::FRAC_PI_4;
    let angle = (d.y.atan2(d.x) / step).round() * step;
    start + Vec2::new(angle.cos(), angle.sin()) * len
}

/// Square/circle snap: use the larger axis for both signs.
fn snap_square(start: Pos2, end: Pos2) -> Pos2 {
    let d = end - start;
    let m = d.x.abs().max(d.y.abs());
    start + Vec2::new(if d.x < 0.0 { -m } else { m }, if d.y < 0.0 { -m } else { m })
}

fn handle_cursor(handle: Handle) -> CursorIcon {
    match handle {
        Handle::N | Handle::S => CursorIcon::ResizeVertical,
        Handle::E | Handle::W => CursorIcon::ResizeHorizontal,
        Handle::Ne | Handle::Sw => CursorIcon::ResizeNeSw,
        Handle::Nw | Handle::Se => CursorIcon::ResizeNwSe,
        Handle::Start | Handle::End => CursorIcon::Crosshair,
    }
}

fn crop_cursor(handle: CropHandle) -> CursorIcon {
    match handle {
        CropHandle::N | CropHandle::S => CursorIcon::ResizeVertical,
        CropHandle::E | CropHandle::W => CursorIcon::ResizeHorizontal,
        CropHandle::Ne | CropHandle::Sw => CursorIcon::ResizeNeSw,
        CropHandle::Nw | CropHandle::Se => CursorIcon::ResizeNwSe,
    }
}

// ---------------------------------------------------------------------------
// Input snapshot
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct InputSnap {
    pos: Option<Pos2>,
    primary_pressed: bool,
    primary_down: bool,
    middle_pressed: bool,
    middle_down: bool,
    shift: bool,
    command: bool,
    esc: bool,
    space: bool,
    time: f64,
    scroll: Vec2,
}

fn read_input(ui: &egui::Ui) -> InputSnap {
    ui.input(|i| InputSnap {
        pos: i.pointer.interact_pos().or_else(|| i.pointer.latest_pos()),
        primary_pressed: i.pointer.primary_pressed(),
        primary_down: i.pointer.primary_down(),
        middle_pressed: i.pointer.button_pressed(PointerButton::Middle),
        middle_down: i.pointer.button_down(PointerButton::Middle),
        shift: i.modifiers.shift,
        command: i.modifiers.command,
        esc: i.key_pressed(Key::Escape),
        space: i.key_down(Key::Space),
        time: i.time,
        scroll: i.raw_scroll_delta,
    })
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

pub fn handle_canvas(ui: &egui::Ui, state: &mut AppState, view: &CanvasView) {
    let response = ui.interact(view.rect, ui.id().with("canvas"), Sense::click_and_drag());
    GESTURE.with(|cell| {
        let mut g = cell.borrow_mut();
        sync_lifecycle(&mut g, state);

        let input = read_input(ui);
        let mut editor_open = false;
        if g.editor.is_some() {
            edit_text_overlay(ui, state, view, &mut g);
            editor_open = true;
        }

        if !editor_open {
            if input.esc && g.arrow.is_some() {
                g.arrow = None;
            }
            update_pointer(state, view, &mut g, &response, &input);
            update_wheel(state, view, &response, &input);
            update_hover(&mut g, view, &response, &input);
        }

        update_cursor(ui, state, view, &g, &response, &input);
        paint_overlay(ui, state, view, &g);
    });
}

// ---------------------------------------------------------------------------
// Lifecycle: drop stale gestures when phase/tool/document change.
// ---------------------------------------------------------------------------

fn sync_lifecycle(g: &mut GestureState, state: &mut AppState) {
    let doc_key = state.doc.as_ref().map(|d| (d.width, d.height, d.name.clone()));
    let doc_changed = g.last_doc != doc_key;
    let phase_changed = g.last_phase != Some(state.phase);
    let tool_changed = g.last_tool != Some(state.tool);
    if doc_changed {
        g.editor = None;
        cancel_gesture(g, state);
        // A new document has no crop: never restore the previous one.
        state.crop = None;
        g.arrow = None;
        g.hover = None;
    } else if phase_changed || tool_changed {
        if g.editor.is_some() {
            commit_editor(g, state);
        }
        cancel_gesture(g, state);
        g.arrow = None;
    }
    g.last_doc = doc_key;
    g.last_phase = Some(state.phase);
    g.last_tool = Some(state.tool);
}

fn cancel_gesture(g: &mut GestureState, state: &mut AppState) {
    if let Some(drag) = g.drag.take() {
        match drag {
            Drag::Pan { .. } | Drag::Brush { .. } | Drag::Shape { .. } | Drag::ArrowDrag { .. } => {}
            Drag::MoveObj { .. } | Drag::Resize { .. } => {}
            Drag::CropNew { prev, .. }
            | Drag::CropMove { prev, .. }
            | Drag::CropResize { prev, .. } => {
                // Half-finished crop gestures roll back to the rectangle they
                // started from.
                state.crop = prev;
            }
        }
    }
    g.brush = None;
    g.shape = None;
}

// ---------------------------------------------------------------------------
// Pointer processing
// ---------------------------------------------------------------------------

fn update_pointer(
    state: &mut AppState,
    view: &CanvasView,
    g: &mut GestureState,
    response: &egui::Response,
    input: &InputSnap,
) {
    let over = response.contains_pointer() || response.hovered();

    if let Some(mut drag) = g.drag.take() {
        let finished = update_drag(state, view, g, &mut drag, input);
        if finished {
            finish_drag(state, view, g, drag, input);
        } else {
            g.drag = Some(drag);
        }
        return;
    }

    if input.middle_pressed && over {
        if let Some(pos) = input.pos {
            start_pan(state, g, pos, false, PanButton::Middle);
        }
        return;
    }
    if input.primary_pressed && over {
        if let Some(pos) = input.pos {
            begin_primary(state, view, g, pos, input);
        }
    }
}

fn start_pan(state: &mut AppState, g: &mut GestureState, pos: Pos2, deselect: bool, button: PanButton) {
    let _ = state;
    g.drag = Some(Drag::Pan {
        start: pos,
        offset0: state.view.offset,
        moved: false,
        deselect_on_click: deselect,
        button,
    });
}

fn begin_primary(
    state: &mut AppState,
    view: &CanvasView,
    g: &mut GestureState,
    pos: Pos2,
    input: &InputSnap,
) {
    let img = (view.to_image)(pos);

    if state.phase == Phase::Preview {
        let double = is_double_click(g, input.time, pos);
        if double {
            state.view.fit = true;
            return;
        }
        if can_pan_preview(state, view) {
            start_pan(state, g, pos, false, PanButton::Primary);
        }
        return;
    }

    if state.doc.is_none() {
        return;
    }
    if input.space {
        start_pan(state, g, pos, false, PanButton::Space);
        return;
    }

    let tol = (HIT_TOL_PX / view.zoom.max(0.0001)).max(0.5);
    match state.tool {
        Tool::Move => begin_move(state, view, g, pos, img, tol, input),
        Tool::Brush => {
            let draft = Annot::brush(img, state.style);
            g.brush = Some(draft);
            g.drag = Some(Drag::Brush { last_screen: pos, last_time: input.time });
        }
        Tool::Line | Tool::Arrow => begin_linear(state, g, pos, img, input),
        Tool::Rect | Tool::Ellipse => {
            let kind = if state.tool == Tool::Rect { AnnotKind::Rect } else { AnnotKind::Ellipse };
            let draft = Annot::new(kind, vec![img, img], state.style);
            g.shape = Some(draft);
            g.drag = Some(Drag::Shape { start_img: img, start_screen: pos });
        }
        Tool::Text => open_new_text_editor(g, pos, img, state.style),
        Tool::Crop => begin_crop(state, view, g, pos, img),
    }
}

fn begin_move(
    state: &mut AppState,
    view: &CanvasView,
    g: &mut GestureState,
    pos: Pos2,
    img: Pos2,
    tol: f32,
    input: &InputSnap,
) {
    // 1. A handle of the current selection wins (resize).
    if let Some(selected) = state.selected_object().cloned() {
        if let Some(handle) = object_handle_at(view, &selected, pos) {
            g.drag = Some(Drag::Resize {
                obj: selected,
                handle,
                start_screen: pos,
                moved: false,
                committed: false,
            });
            return;
        }
    }

    // 2. Endpoint handles / arrows beat everything else on the line body.
    let hit = hit_linear_topmost(&state.objects, img, tol)
        .or_else(|| hit_topmost(&state.objects, img, tol));

    if let Some(index) = hit {
        let (id, kind) = {
            let obj = &state.objects[index];
            (obj.id, obj.kind)
        };
        if state.selected != Some(id) {
            state.selected = Some(id);
        }
        let double = is_double_click(g, input.time, pos);
        if double && kind == AnnotKind::Text {
            open_existing_text_editor(state, view, g, id);
            return;
        }
        let snapshot = state.objects[index].clone();
        g.drag = Some(Drag::MoveObj {
            obj: snapshot,
            origin_img: img,
            moved: false,
            committed: false,
        });
        return;
    }

    // 3. Nothing hit: dragging anywhere pans the canvas.
    start_pan(state, g, pos, true, PanButton::Primary);
}

fn begin_linear(
    state: &mut AppState,
    g: &mut GestureState,
    pos: Pos2,
    img: Pos2,
    input: &InputSnap,
) {
    if let Some(pending) = g.arrow.as_ref() {
        // Second click: fix the end point (ignore clicks on the start point).
        let dist = (pos - pending.start_screen).length();
        if dist >= ARROW_CLICK_MIN {
            let mut obj = pending.obj.clone();
            let end = if input.shift { snap_angle(obj.points[0], img) } else { img };
            obj.points[1] = crate::annotate::clamp_arrow_endpoint(obj.points[0], end);
            g.arrow = None;
            state.add_annot(obj);
        }
        return;
    }
    let kind = if state.tool == Tool::Line { AnnotKind::Line } else { AnnotKind::Arrow };
    let obj = Annot::new(kind, vec![img, img], state.style);
    g.arrow = Some(PendingArrow { obj, start_screen: pos });
    g.drag = Some(Drag::ArrowDrag { start_screen: pos, start_img: img });
}

fn begin_crop(state: &mut AppState, view: &CanvasView, g: &mut GestureState, pos: Pos2, img: Pos2) {
    let prev = state.crop;
    let ratio = prev.and_then(|c| c.ratio);
    if let Some(c) = prev {
        if c.w >= 1.0 && c.h >= 1.0 {
            if let Some(handle) = crop_handle_at(view, c, pos) {
                g.drag = Some(Drag::CropResize {
                    rect0: c,
                    handle,
                    start_screen: pos,
                    prev: Some(c),
                    ratio,
                    moved: false,
                    committed: false,
                });
                return;
            }
            if c.contains(img) {
                g.drag = Some(Drag::CropMove {
                    rect0: c,
                    img0: img,
                    start_screen: pos,
                    prev: Some(c),
                    moved: false,
                    committed: false,
                });
                return;
            }
        }
    }
    let Some((doc_w, doc_h)) = doc_size(state) else { return };
    let img0 = Pos2::new(img.x.clamp(0.0, doc_w), img.y.clamp(0.0, doc_h));
    g.drag = Some(Drag::CropNew {
        img0,
        start_screen: pos,
        prev,
        ratio,
        committed: false,
    });
}

/// Returns true when the gesture should finish (`update_pointer` then calls
/// `finish_drag`).
fn update_drag(
    state: &mut AppState,
    view: &CanvasView,
    g: &mut GestureState,
    drag: &mut Drag,
    input: &InputSnap,
) -> bool {
    let Some(pos) = input.pos else {
        return !input.primary_down && !input.middle_down;
    };
    match drag {
        Drag::Pan { start, offset0, moved, button, .. } => {
            let released = match button {
                PanButton::Primary => !input.primary_down,
                PanButton::Middle => !input.middle_down,
                PanButton::Space => !input.primary_down || !input.space,
            };
            let d = pos - *start;
            if !*moved {
                if d.length() < PAN_THRESHOLD {
                    return released;
                }
                *moved = true;
            }
            let mut offset = *offset0 + d;
            if state.phase == Phase::Preview {
                if let Some((w, h)) = doc_size(state) {
                    let content = Vec2::new(w, h) * state.view.scale;
                    offset = clamp_offset_to_view(offset, view.rect.size(), content);
                }
            }
            state.view.offset = offset;
            state.view.fit = false;
            released
        }
        Drag::Brush { last_screen, last_time } => {
            if let Some(draft) = g.brush.as_mut() {
                let dist = (pos - *last_screen).length();
                if dist >= 0.7 {
                    let dt_ms = ((input.time - *last_time) * 1000.0).max(4.0) as f32;
                    let velocity = dist / dt_ms;
                    let size = draft.style.size;
                    let prev_w = draft.point_width(draft.points.len().saturating_sub(1));
                    let width = brush_width(prev_w, size, velocity);
                    let img = (view.to_image)(pos);
                    draft.push_brush_point(img, width);
                    *last_screen = pos;
                    *last_time = input.time;
                }
            }
            !input.primary_down
        }
        Drag::Shape { start_img, start_screen } => {
            let img = (view.to_image)(pos);
            let end = if input.shift { snap_square(*start_img, img) } else { img };
            if let Some(draft) = g.shape.as_mut() {
                draft.points = vec![*start_img, end];
            }
            let _ = start_screen;
            !input.primary_down
        }
        Drag::ArrowDrag { start_screen, start_img } => {
            if let Some(pending) = g.arrow.as_mut() {
                let img = (view.to_image)(pos);
                pending.obj.points[1] =
                    if input.shift { snap_angle(*start_img, img) } else { img };
            }
            let _ = start_screen;
            !input.primary_down
        }
        Drag::MoveObj { obj, origin_img, moved, committed } => {
            let img = (view.to_image)(pos);
            let delta = img - *origin_img;
            if !*moved && delta.length() * view.zoom < 2.0 {
                return !input.primary_down;
            }
            *moved = true;
            if !*committed {
                state.commit();
                *committed = true;
            }
            let mut next = obj.clone();
            next.move_by(delta);
            replace_object(state, next);
            !input.primary_down
        }
        Drag::Resize { obj, handle, start_screen, moved, committed } => {
            let img = (view.to_image)(pos);
            if !*moved && (pos - *start_screen).length() < 0.5 {
                return !input.primary_down;
            }
            *moved = true;
            if !*committed {
                state.commit();
                *committed = true;
            }
            let mut next = obj.clone();
            next.resize(*handle, img, input.shift);
            replace_object(state, next);
            !input.primary_down
        }
        Drag::CropNew { img0, start_screen, ratio, committed, .. } => {
            if (pos - *start_screen).length() < 0.5 {
                return !input.primary_down;
            }
            let Some((doc_w, doc_h)) = doc_dims(state) else { return true };
            let img = (view.to_image)(pos);
            let lock = ratio.or(if input.shift { Some(1.0) } else { None });
            let mut rect = CropRect::from_drag(*img0, img, lock, doc_w, doc_h);
            // The model keeps the crop's own ratio; Shift is a transient lock.
            rect.ratio = *ratio;
            if !*committed {
                state.commit();
                *committed = true;
            }
            state.crop = Some(rect);
            !input.primary_down
        }
        Drag::CropMove { rect0, img0, start_screen, moved, committed, .. } => {
            if !*moved && (pos - *start_screen).length() < 0.5 {
                return !input.primary_down;
            }
            *moved = true;
            let Some((doc_w, doc_h)) = doc_dims(state) else { return true };
            let img = (view.to_image)(pos);
            let rect = rect0.moved(Vec2::new(img.x - img0.x, img.y - img0.y), doc_w, doc_h);
            if !*committed {
                state.commit();
                *committed = true;
            }
            state.crop = Some(rect);
            !input.primary_down
        }
        Drag::CropResize { rect0, handle, start_screen, ratio, moved, committed, .. } => {
            if !*moved && (pos - *start_screen).length() < 0.5 {
                return !input.primary_down;
            }
            *moved = true;
            let Some((doc_w, doc_h)) = doc_dims(state) else { return true };
            let img = (view.to_image)(pos);
            let lock = ratio.or(if input.shift { Some(1.0) } else { None });
            let mut rect = rect0.resize_with_ratio(*handle, img, doc_w, doc_h, lock);
            // Transient Shift lock must not be persisted into the model.
            rect.ratio = rect0.ratio;
            if !*committed {
                state.commit();
                *committed = true;
            }
            state.crop = Some(rect);
            !input.primary_down
        }
    }
}

fn finish_drag(
    state: &mut AppState,
    view: &CanvasView,
    g: &mut GestureState,
    drag: Drag,
    input: &InputSnap,
) {
    match drag {
        Drag::Pan { moved, deselect_on_click, .. } => {
            if deselect_on_click && !moved {
                state.selected = None;
            }
        }
        Drag::Brush { .. } => {
            if let Some(draft) = g.brush.take() {
                if !draft.points.is_empty() {
                    state.add_annot(draft);
                }
            }
        }
        Drag::Shape { start_screen, .. } => {
            if let Some(draft) = g.shape.take() {
                if draft.points.len() >= 2 {
                    let size_img = (draft.points[1] - draft.points[0]).length();
                    let released = input
                        .pos
                        .map(|p| (p - start_screen).length())
                        .unwrap_or(0.0);
                    if size_img * view.zoom >= MIN_SHAPE_SCREEN
                        && released >= MIN_SHAPE_SCREEN
                    {
                        state.add_annot(draft);
                    }
                }
            }
        }
        Drag::ArrowDrag { start_screen, .. } => {
            let released = input.pos.map(|p| (p - start_screen).length()).unwrap_or(0.0);
            if released >= ARROW_DRAG_MIN {
                if let Some(pending) = g.arrow.take() {
                    state.add_annot(pending.obj);
                }
            }
            // Otherwise keep the pending arrow waiting for its second click.
        }
        Drag::MoveObj { .. } | Drag::Resize { .. } => {}
        Drag::CropNew { prev, .. } => {
            let too_small = state
                .crop
                .map(|c| c.w * view.zoom < MIN_CROP_SCREEN || c.h * view.zoom < MIN_CROP_SCREEN)
                .unwrap_or(true);
            if too_small {
                state.crop = prev;
            }
        }
        Drag::CropMove { prev, moved, .. } | Drag::CropResize { prev, moved, .. } => {
            if !moved {
                state.crop = prev;
            }
        }
    }
}

fn replace_object(state: &mut AppState, obj: Annot) {
    if let Some(slot) = state.objects.iter_mut().find(|o| o.id == obj.id) {
        *slot = obj;
    }
}

fn is_double_click(g: &mut GestureState, time: f64, pos: Pos2) -> bool {
    let prev = g.last_click;
    let quick = time - prev.0 <= DBLCLICK_SECS;
    let near = (pos - prev.1).length() < 8.0;
    g.last_click = (time, pos);
    quick && near
}

// ---------------------------------------------------------------------------
// Hit helpers
// ---------------------------------------------------------------------------

fn object_handle_at(view: &CanvasView, obj: &Annot, screen: Pos2) -> Option<Handle> {
    let mut best: Option<(f32, Handle)> = None;
    for (handle, p) in obj.handles() {
        let sp = (view.to_screen)(p);
        let dx = (screen.x - sp.x).abs();
        let dy = (screen.y - sp.y).abs();
        if dx <= HANDLE_HIT && dy <= HANDLE_HIT {
            let dist = dx.hypot(dy);
            if best.map(|(d, _)| dist < d).unwrap_or(true) {
                best = Some((dist, handle));
            }
        }
    }
    best.map(|(_, h)| h)
}

fn crop_handle_at(view: &CanvasView, crop: CropRect, screen: Pos2) -> Option<CropHandle> {
    let img = (view.to_image)(screen);
    crop.hit_handle(img, (HANDLE_HIT + 2.0) / view.zoom.max(0.0001))
}

fn hit_linear_topmost(objects: &[Annot], pt: Pos2, tol: f32) -> Option<usize> {
    objects
        .iter()
        .enumerate()
        .rev()
        .find(|(_, o)| o.kind.is_linear() && o.hit(pt, tol))
        .map(|(i, _)| i)
}

fn can_pan_preview(state: &AppState, view: &CanvasView) -> bool {
    let Some((w, h)) = doc_size(state) else { return false };
    state.view.scale > fit_scale_of(Vec2::new(w, h), view.rect.size()) + 0.001
}

// ---------------------------------------------------------------------------
// Wheel zoom / pan
// ---------------------------------------------------------------------------

fn update_wheel(
    state: &mut AppState,
    view: &CanvasView,
    response: &egui::Response,
    input: &InputSnap,
) {
    if !response.contains_pointer() || state.doc.is_none() {
        return;
    }
    let anchor = input.pos.unwrap_or_else(|| view.rect.center());
    if state.phase == Phase::Preview {
        if input.scroll.y.abs() < 0.001 {
            return;
        }
        let factor = (input.scroll.y * 0.0013).exp().clamp(0.86, 1.15);
        let Some((w, h)) = doc_size(state) else { return };
        let min = fit_scale_of(Vec2::new(w, h), view.rect.size());
        let next = (state.view.scale * factor).clamp(min, MAX_SCALE);
        zoom_to(state, view, next, anchor);
        let content = Vec2::new(w, h) * state.view.scale;
        state.view.offset =
            clamp_offset_to_view(state.view.offset, view.rect.size(), content);
        return;
    }
    if input.command {
        if input.scroll.y.abs() < 0.001 {
            return;
        }
        let factor = (input.scroll.y * 0.0015).exp();
        let next = (state.view.scale * factor).clamp(MIN_SCALE, MAX_SCALE);
        zoom_to(state, view, next, anchor);
        return;
    }
    if input.scroll != Vec2::ZERO {
        state.view.offset += input.scroll;
    }
}

/// Zoom to `next` around `anchor` (screen px), keeping the anchor image point
/// pinned. Matches `app.rs::zoom_at` / viewport.js `zoomAt`.
fn zoom_to(state: &mut AppState, view: &CanvasView, next: f32, anchor: Pos2) {
    let old = state.view.scale.max(0.0001);
    if (next - old).abs() <= 1e-6 {
        return;
    }
    let rel = anchor - view.rect.center();
    state.view.offset = (state.view.offset - rel) * (next / old) + rel;
    state.view.scale = next;
    state.view.fit = false;
}

fn update_hover(
    g: &mut GestureState,
    view: &CanvasView,
    response: &egui::Response,
    input: &InputSnap,
) {
    if response.contains_pointer() {
        g.hover = input.pos;
        // Keep the pending arrow's preview endpoint glued to the pointer.
        if let Some(pending) = g.arrow.as_mut() {
            if let Some(pos) = input.pos {
                let img = (view.to_image)(pos);
                pending.obj.points[1] =
                    if input.shift { snap_angle(pending.obj.points[0], img) } else { img };
            }
        }
    } else if g.drag.is_none() {
        g.hover = None;
    }
}

// ---------------------------------------------------------------------------
// Cursor
// ---------------------------------------------------------------------------

fn update_cursor(
    ui: &egui::Ui,
    state: &AppState,
    view: &CanvasView,
    g: &GestureState,
    response: &egui::Response,
    input: &InputSnap,
) {
    let dragging = g.drag.is_some();
    if !dragging && !response.contains_pointer() {
        return;
    }
    let pos = input.pos.unwrap_or_else(|| view.rect.center());
    let icon = if let Some(drag) = g.drag.as_ref() {
        match drag {
            Drag::Pan { .. } => CursorIcon::Grabbing,
            Drag::Brush { .. } | Drag::Shape { .. } | Drag::ArrowDrag { .. } => {
                CursorIcon::Crosshair
            }
            Drag::MoveObj { .. } => CursorIcon::Move,
            Drag::Resize { handle, .. } => handle_cursor(*handle),
            Drag::CropNew { .. } => CursorIcon::Crosshair,
            Drag::CropMove { .. } => CursorIcon::Move,
            Drag::CropResize { handle, .. } => crop_cursor(*handle),
        }
    } else if input.space {
        CursorIcon::Grab
    } else if state.phase == Phase::Preview {
        if can_pan_preview(state, view) {
            CursorIcon::Grab
        } else {
            CursorIcon::Default
        }
    } else if state.doc.is_none() {
        CursorIcon::Default
    } else {
        match state.tool {
            Tool::Move => {
                let mut icon = CursorIcon::Default;
                if let Some(selected) = state.selected_object() {
                    if let Some(handle) = object_handle_at(view, selected, pos) {
                        icon = handle_cursor(handle);
                    }
                }
                if icon == CursorIcon::Default {
                    let img = (view.to_image)(pos);
                    let tol = (HIT_TOL_PX / view.zoom.max(0.0001)).max(0.5);
                    let hit = hit_linear_topmost(&state.objects, img, tol)
                        .or_else(|| hit_topmost(&state.objects, img, tol));
                    if hit.is_some() {
                        icon = CursorIcon::Move;
                    }
                }
                icon
            }
            Tool::Crop => {
                let mut icon = CursorIcon::Crosshair;
                if let Some(crop) = state.crop {
                    if crop.w >= 1.0 && crop.h >= 1.0 {
                        if let Some(handle) = crop_handle_at(view, crop, pos) {
                            icon = crop_cursor(handle);
                        } else if crop.contains((view.to_image)(pos)) {
                            icon = CursorIcon::Move;
                        }
                    }
                }
                icon
            }
            Tool::Text => CursorIcon::Text,
            _ => CursorIcon::Crosshair,
        }
    };
    ui.ctx().set_cursor_icon(icon);
}

// ---------------------------------------------------------------------------
// Overlay painting (drafts, selection frame/handles, crop shading)
// ---------------------------------------------------------------------------

fn paint_overlay(
    ui: &egui::Ui,
    state: &AppState,
    view: &CanvasView,
    g: &GestureState,
) {
    let painter = ui.painter_at(view.rect);
    let to_screen = &*view.to_screen;

    if let Some(draft) = g.brush.as_ref() {
        draft.draw(&painter, to_screen, view.zoom);
    }
    if let Some(draft) = g.shape.as_ref() {
        draft.draw(&painter, to_screen, view.zoom);
    }
    if let Some(pending) = g.arrow.as_ref() {
        pending.obj.draw(&painter, to_screen, view.zoom);
        let anchor = to_screen(pending.obj.points[0]);
        painter.circle_filled(anchor, 4.0, theme::ACCENT);
        painter.circle_stroke(
            anchor,
            4.0,
            Stroke::new(1.0_f32, Color32::from_rgba_premultiplied(255, 255, 255, 217)),
        );
    }

    if state.phase != Phase::Edit {
        return;
    }
    if state.tool == Tool::Move {
        if let Some(selected) = state.selected_object() {
            paint_selection(&painter, view, selected, g.hover);
        }
    }
}

fn paint_selection(
    painter: &egui::Painter,
    view: &CanvasView,
    obj: &Annot,
    hover: Option<Pos2>,
) {
    if obj.kind.is_linear() {
        for (_, p) in obj.handles() {
            let sp = (view.to_screen)(p);
            let hovered =
                hover.map(|h| (h - sp).length() <= 9.0).unwrap_or(false);
            let radius = if hovered { 6.0 } else { 4.5 };
            painter.circle_filled(sp, radius, theme::ACCENT);
            painter.circle_stroke(
                sp,
                radius,
                Stroke::new(1.0_f32, Color32::from_rgba_premultiplied(255, 255, 255, 230)),
            );
        }
        return;
    }

    let b = obj.bounds();
    if !b.is_finite() || b.width() <= 0.0 || b.height() <= 0.0 {
        return;
    }
    let p0 = (view.to_screen)(b.left_top());
    let p1 = (view.to_screen)(b.right_bottom());
    let frame = Rect::from_two_pos(p0, p1);
    painter.rect_stroke(frame, 0.0, Stroke::new(1.0_f32, theme::ACCENT), egui::StrokeKind::Middle);
    for (_, p) in obj.handles() {
        let sp = (view.to_screen)(p);
        let rect = Rect::from_center_size(sp, Vec2::splat(HANDLE_SIZE));
        painter.rect_filled(rect, 0.0, Color32::WHITE);
        painter.rect_stroke(
            rect,
            0.0,
            Stroke::new(1.0_f32, theme::ACCENT),
            egui::StrokeKind::Middle,
        );
    }
}

// ---------------------------------------------------------------------------
// Inline text editor
// ---------------------------------------------------------------------------

fn open_new_text_editor(g: &mut GestureState, screen: Pos2, img: Pos2, style: Style) {
    g.editor = Some(TextEditor {
        screen_pos: screen,
        image_pos: img,
        buffer: String::new(),
        style,
        existing: None,
        focused: false,
    });
}

fn open_existing_text_editor(
    state: &mut AppState,
    view: &CanvasView,
    g: &mut GestureState,
    id: u64,
) {
    let Some(obj) = state.objects.iter().find(|o| o.id == id) else { return };
    let b = obj.bounds();
    let screen = (view.to_screen)(Pos2::new(b.left(), b.top()));
    g.editor = Some(TextEditor {
        screen_pos: screen,
        image_pos: Pos2::new(b.left(), b.top()),
        buffer: obj.text.clone(),
        style: obj.style,
        existing: Some(id),
        focused: false,
    });
}

/// Draw the inline editor and apply commit/cancel. Returns true when the editor
/// is (or was) open, so the caller swallows the same frame's canvas click.
fn edit_text_overlay(
    ui: &egui::Ui,
    state: &mut AppState,
    view: &CanvasView,
    g: &mut GestureState,
) -> bool {
    let Some(_) = g.editor.as_ref() else { return false };
    let ctx = ui.ctx().clone();
    let hint = state.locale.tr("文字");
    let esc = ctx.input(|i| i.key_pressed(Key::Escape));
    let commit_keys = ctx.input(|i| i.modifiers.command && i.key_pressed(Key::Enter));
    let pressed = ctx.input(|i| i.pointer.primary_pressed());
    let pointer = ctx.input(|i| i.pointer.interact_pos());

    let mut commit = false;
    let area_rect: Rect;
    {
        let editor = g.editor.as_mut().expect("editor checked above");
        let font_px = (editor.style.font_size * view.zoom).clamp(8.0, 72.0);
        let area = egui::Area::new(Id::new("lumen_text_editor"))
            .order(egui::Order::Foreground)
            .fixed_pos(editor.screen_pos)
            .show(&ctx, |ui| {
                egui::Frame::new()
                    .fill(Color32::from_rgba_premultiplied(24, 24, 26, 235))
                    .stroke(Stroke::new(1.0_f32, theme::ACCENT))
                    .corner_radius(4.0)
                    .inner_margin(egui::Margin::same(6))
                    .show(ui, |ui| {
                        let resp = ui.add(
                            egui::TextEdit::multiline(&mut editor.buffer)
                                .hint_text(hint)
                                .font(egui::FontId::proportional(font_px))
                                .desired_width(240.0)
                                .desired_rows(1)
                                .text_color(theme::TEXT),
                        );
                        if !editor.focused {
                            resp.request_focus();
                            editor.focused = true;
                        }
                        if resp.lost_focus() {
                            commit = true;
                        }
                    });
            });
        area_rect = area.response.rect;
    }

    let clicked_outside =
        pressed && pointer.map(|p| !area_rect.expand(2.0).contains(p)).unwrap_or(false);

    if esc {
        g.editor = None;
    } else if commit_keys || commit || clicked_outside {
        commit_editor(g, state);
    }
    true
}

fn commit_editor(g: &mut GestureState, state: &mut AppState) {
    let Some(editor) = g.editor.take() else { return };
    let text = editor.buffer.replace('\r', "");
    let text = text.trim().to_string();
    match editor.existing {
        Some(id) => {
            if !text.is_empty() {
                state.commit();
                if let Some(obj) = state.objects.iter_mut().find(|o| o.id == id) {
                    obj.text = text;
                }
                state.selected = Some(id);
            }
        }
        None => {
            if !text.is_empty() {
                let obj = Annot::text(editor.image_pos, text, editor.style);
                state.add_annot(obj);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests for the pure gesture maths
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f32, b: f32) {
        assert!((a - b).abs() < 1e-4, "{a} != {b}");
    }

    #[test]
    fn fit_scale_never_blows_up_small_images() {
        approx(fit_scale_of(Vec2::new(2000.0, 1000.0), Vec2::new(1000.0, 800.0)), 0.5);
        approx(fit_scale_of(Vec2::new(200.0, 100.0), Vec2::new(1000.0, 800.0)), 1.0);
        approx(fit_scale_of(Vec2::new(0.0, 0.0), Vec2::new(100.0, 100.0)), 1.0);
    }

    #[test]
    fn preview_offset_clamps_or_centres() {
        // content larger than viewport: pan clamped to half the overflow
        let off = clamp_offset_to_view(
            Vec2::new(500.0, -500.0),
            Vec2::new(1000.0, 800.0),
            Vec2::new(1200.0, 600.0),
        );
        approx(off.x, 100.0);
        approx(off.y, 0.0);
        // content smaller than viewport: centred
        let off = clamp_offset_to_view(
            Vec2::new(50.0, 50.0),
            Vec2::new(1000.0, 800.0),
            Vec2::new(400.0, 300.0),
        );
        approx(off.x, 0.0);
        approx(off.y, 0.0);
    }

    #[test]
    fn shift_snap_45_degrees_keeps_length() {
        let start = Pos2::new(100.0, 100.0);
        let snapped = snap_angle(start, Pos2::new(160.0, 103.0));
        approx((snapped - start).length(), (Pos2::new(160.0, 103.0) - start).length());
        approx(snapped.y, 100.0); // near-horizontal snaps to 0 deg
        let snapped = snap_angle(start, Pos2::new(163.0, 157.0));
        approx(snapped.x, 100.0 + (snapped - start).length() * std::f32::consts::FRAC_1_SQRT_2);
        approx(snapped.y, 100.0 + (snapped - start).length() * std::f32::consts::FRAC_1_SQRT_2);
    }

    #[test]
    fn shift_snap_square_uses_the_larger_axis() {
        let start = Pos2::new(10.0, 10.0);
        let p = snap_square(start, Pos2::new(60.0, 30.0));
        approx(p.x, 60.0);
        approx(p.y, 60.0);
        let p = snap_square(start, Pos2::new(-40.0, 0.0));
        approx(p.x, -40.0);
        approx(p.y, -40.0);
    }
}
