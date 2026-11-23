// Central mutable state for the egui build. Lead-owned: modules read/write it
// through the small API at the bottom, never by re-declaring their own copies.

use std::path::PathBuf;

use egui::Vec2;

use crate::annotate::{Annot, Style};
use crate::crop::CropRect;
use crate::doc::Doc;
use crate::export::Estimated;
use crate::i18n::Locale;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Preview,
    Edit,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Move,
    Brush,
    Line,
    Rect,
    Ellipse,
    Arrow,
    Text,
    Crop,
}

impl Tool {
    pub const ALL: [Tool; 8] = [
        Tool::Move,
        Tool::Brush,
        Tool::Line,
        Tool::Rect,
        Tool::Ellipse,
        Tool::Arrow,
        Tool::Text,
        Tool::Crop,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Tool::Move => "移動",
            Tool::Brush => "畫筆",
            Tool::Line => "直線",
            Tool::Rect => "矩形",
            Tool::Ellipse => "圓框",
            Tool::Arrow => "箭頭",
            Tool::Text => "文字",
            Tool::Crop => "裁切",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OutputFormat {
    Auto,
    Jpeg,
    Png,
    Webp,
}

impl OutputFormat {
    pub fn label(self) -> &'static str {
        match self {
            OutputFormat::Auto => "原格式",
            OutputFormat::Jpeg => "JPEG",
            OutputFormat::Png => "PNG",
            OutputFormat::Webp => "WEBP",
        }
    }
    pub fn ext(self) -> &'static str {
        match self {
            OutputFormat::Auto => "jpg",
            OutputFormat::Jpeg => "jpg",
            OutputFormat::Png => "png",
            OutputFormat::Webp => "webp",
        }
    }

    /// Resolve `Auto` to the concrete encoder for a document.
    pub fn resolve(self, doc: &Doc) -> OutputFormat {
        if self != OutputFormat::Auto {
            return self;
        }
        let name = doc.name.to_ascii_lowercase();
        let format = doc.format.to_ascii_lowercase();
        if format.contains("png") || name.ends_with(".png") {
            OutputFormat::Png
        } else if format.contains("webp") || name.ends_with(".webp") {
            OutputFormat::Webp
        } else {
            OutputFormat::Jpeg
        }
    }
}

#[derive(Clone)]
pub struct OutputSettings {
    pub format: OutputFormat,
    pub quality: u8,
    pub max_dim: Option<u32>,
    /// Explicit output scale (1.0 = original pixels; >1 upscales, <1 shrinks).
    pub scale: Option<f32>,
    pub target_kb: Option<u32>,
}

impl Default for OutputSettings {
    fn default() -> Self {
        Self { format: OutputFormat::Auto, quality: 90, max_dim: None, scale: None, target_kb: None }
    }
}

/// Intents emitted by the UI chrome and drained by `app.rs` (dialogs, files).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UiAction {
    Open,
    Save,
    SaveAs,
    Export,
    Estimate,
    Prev,
    Next,
    CloseWindow,
    ApplyCrop,
    ResetCrop,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum StatusKind {
    Info,
    Ok,
    Warn,
    Err,
}

#[derive(Clone)]
pub struct StatusMessage {
    pub text: String,
    pub kind: StatusKind,
    pub born: f64,
}

pub struct View {
    pub scale: f32,
    pub offset: Vec2,
    pub fit: bool,
}

impl Default for View {
    fn default() -> Self {
        Self { scale: 1.0, offset: Vec2::ZERO, fit: true }
    }
}

pub struct AppState {
    pub locale: Locale,
    pub doc: Option<Doc>,
    pub error: Option<String>,
    pub view: View,
    pub phase: Phase,
    pub tool: Tool,
    pub style: Style,
    pub objects: Vec<Annot>,
    pub selected: Option<u64>,
    pub crop: Option<CropRect>,
    pub crop_committed: bool,
    pub output: OutputSettings,
    pub show_info: bool,
    pub show_info_card: bool,
    pub show_shortcuts: bool,
    pub show_browser: bool,
    pub compare: bool,
    pending: Vec<UiAction>,
    pub dirty: bool,
    pub fullscreen: bool,
    pub status: Option<StatusMessage>,
    pub estimate: Option<Estimated>,
    pub siblings: Vec<PathBuf>,
    pub index: usize,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
}

#[derive(Clone)]
struct Snapshot {
    objects: Vec<Annot>,
    crop: Option<CropRect>,
    selected: Option<u64>,
}

const MAX_HISTORY: usize = 80;

impl AppState {
    pub fn new(locale: Locale) -> Self {
        Self {
            locale,
            doc: None,
            error: None,
            view: View::default(),
            phase: Phase::Preview,
            tool: Tool::Move,
            style: Style::default(),
            objects: Vec::new(),
            selected: None,
            crop: None,
            crop_committed: false,
            output: OutputSettings::default(),
            show_info: false,
            show_info_card: false,
            show_shortcuts: false,
            show_browser: false,
            compare: false,
            pending: Vec::new(),
            dirty: false,
            fullscreen: false,
            status: None,
            estimate: None,
            siblings: Vec::new(),
            index: 0,
            undo: Vec::new(),
            redo: Vec::new(),
        }
    }

    pub fn doc_size(&self) -> Option<(u32, u32)> {
        self.doc.as_ref().map(|d| (d.width, d.height))
    }

    pub fn reset_for_doc(&mut self) {
        self.view = View::default();
        self.objects.clear();
        self.selected = None;
        self.crop = None;
        self.crop_committed = false;
        self.dirty = false;
        self.compare = false;
        self.undo.clear();
        self.redo.clear();
    }

    /// Push the current edit state before a mutation (80-entry history).
    pub fn commit(&mut self) {
        self.undo.push(Snapshot {
            objects: self.objects.clone(),
            crop: self.crop,
            selected: self.selected,
        });
        if self.undo.len() > MAX_HISTORY {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.dirty = true;
    }

    pub fn undo(&mut self) -> bool {
        let Some(prev) = self.undo.pop() else { return false };
        self.redo.push(Snapshot {
            objects: self.objects.clone(),
            crop: self.crop,
            selected: self.selected,
        });
        self.objects = prev.objects;
        self.crop = prev.crop;
        self.selected = prev.selected;
        self.dirty = true;
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(next) = self.redo.pop() else { return false };
        self.undo.push(Snapshot {
            objects: self.objects.clone(),
            crop: self.crop,
            selected: self.selected,
        });
        self.objects = next.objects;
        self.crop = next.crop;
        self.selected = next.selected;
        self.dirty = true;
        true
    }

    pub fn set_tool(&mut self, tool: Tool) {
        self.tool = tool;
        if tool != Tool::Crop && !self.crop_committed {
            self.crop = None;
        }
        if tool == Tool::Crop && self.crop.is_none() {
            let Some((w, h)) = self.doc_size() else { return };
            self.crop = Some(CropRect::centered(w, h, None));
        }
    }

    pub fn select(&mut self, id: Option<u64>) {
        self.selected = id;
    }

    pub fn selected_object(&self) -> Option<&Annot> {
        let id = self.selected?;
        self.objects.iter().find(|o| o.id == id)
    }

    pub fn selected_object_mut(&mut self) -> Option<&mut Annot> {
        let id = self.selected?;
        self.objects.iter_mut().find(|o| o.id == id)
    }

    pub fn add_annot(&mut self, annot: Annot) {
        self.commit();
        self.selected = Some(annot.id);
        self.objects.push(annot);
    }

    pub fn delete_selected(&mut self) {
        let Some(id) = self.selected else { return };
        self.commit();
        self.objects.retain(|o| o.id != id);
        self.selected = None;
    }

    pub fn push_action(&mut self, action: UiAction) {
        self.pending.push(action);
    }

    pub fn take_actions(&mut self) -> Vec<UiAction> {
        std::mem::take(&mut self.pending)
    }

    pub fn status(&mut self, text: impl Into<String>, kind: StatusKind, now: f64) {
        self.status = Some(StatusMessage { text: text.into(), kind, born: now });
    }
}
