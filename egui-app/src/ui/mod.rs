// egui chrome. Each module renders one region and mutates AppState only
// through its public API. Owner: UI teammate.
//
// Router (app.rs calls every module every frame; each one no-ops outside its
// phase):
//   topbar      preview chips/dock OR the editor top bar
//   rail        editor tool rail + zoom stack (edit only)
//   inspector   editor right panel with 資訊/調整/裁切 tabs (edit only)
//   dock        floating style dock (edit only, drawing tools)
//   status      bottom status bar (edit only)
//   info        preview peek info card (preview only)
//   shortcuts   shortcut reference overlay (any phase)

pub mod dock;
pub mod empty;
pub mod format;
pub mod info;
pub mod inspector;
pub mod local;
pub mod motion;
pub mod preview;
pub mod rail;
pub mod shortcuts;
pub mod status;
mod tool_icons;
pub mod topbar;
pub mod widgets;
pub mod zoom;
