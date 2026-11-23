// Experimental native preview shell: GTK3 + cairo, no WebView.
// Measures the "native" architecture before migrating the full UI:
// open, fit/zoom, pan, ← →, file info in the status bar.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gtk::prelude::*;
use gtk::{gdk, gdk_pixbuf, glib};
use gdk::prelude::*;

struct View {
    pixbuf: Option<gdk_pixbuf::Pixbuf>,
    surface: Option<cairo::ImageSurface>,
    path: Option<PathBuf>,
    scale: f64,
    tx: f64,
    ty: f64,
    fit: bool,
}

fn image_ext(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|e| e.to_str())
            .map(|s| s.to_ascii_lowercase())
            .as_deref(),
        Some("jpg") | Some("jpeg") | Some("png") | Some("webp") | Some("gif") | Some("bmp")
            | Some("tif") | Some("tiff") | Some("avif") | Some("svg") | Some("ico")
    )
}

fn siblings(path: &Path) -> Vec<PathBuf> {
    let dir = match path.parent() {
        Some(d) => d,
        None => return vec![path.to_path_buf()],
    };
    let mut list: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.path())
                .filter(|p| p.is_file() && image_ext(p))
                .collect()
        })
        .unwrap_or_default();
    list.sort();
    if list.is_empty() {
        list.push(path.to_path_buf());
    }
    list
}

fn to_surface(pb: &gdk_pixbuf::Pixbuf) -> Option<cairo::ImageSurface> {
    let w = pb.width();
    let h = pb.height();
    let stride = cairo::Format::ARgb32.stride_for_width(w as u32).ok()? as usize;
    let mut data = vec![0u8; stride * h as usize];
    let src = unsafe { pb.pixels() };
    let src_stride = pb.rowstride() as usize;
    let channels = pb.n_channels() as usize;
    let bpp = if channels == 4 { 4 } else { 3 };
    for y in 0..h as usize {
        let srow = &src[y * src_stride..y * src_stride + (w as usize) * bpp];
        let drow = &mut data[y * stride..y * stride + (w as usize) * 4];
        for x in 0..w as usize {
            let o = x * bpp;
            let (r, g, b) = (srow[o], srow[o + 1], srow[o + 2]);
            let a = if bpp == 4 { srow[o + 3] } else { 255 };
            let aa = a as u32;
            drow[x * 4] = ((b as u32 * aa) / 255) as u8;
            drow[x * 4 + 1] = ((g as u32 * aa) / 255) as u8;
            drow[x * 4 + 2] = ((r as u32 * aa) / 255) as u8;
            drow[x * 4 + 3] = a;
        }
    }
    cairo::ImageSurface::create_for_data(
        data,
        cairo::Format::ARgb32,
        w,
        h,
        stride as i32,
    )
    .ok()
}

fn fit_view(state: &Rc<RefCell<View>>, area: &gtk::DrawingArea) {
    let mut st = state.borrow_mut();
    let (w, h) = match st.pixbuf.as_ref() {
        Some(pb) => (pb.width() as f64, pb.height() as f64),
        None => return,
    };
    let aw = area.allocated_width() as f64;
    let ah = area.allocated_height() as f64;
    if aw < 2.0 || ah < 2.0 {
        return;
    }
    st.scale = (aw / w).min(ah / h).min(1.0);
    st.tx = (aw - w * st.scale) / 2.0;
    st.ty = (ah - h * st.scale) / 2.0;
    st.fit = true;
    drop(st);
    area.queue_draw();
}

fn zoom(state: &Rc<RefCell<View>>, area: &gtk::DrawingArea, factor: f64) {
    let mut st = state.borrow_mut();
    let aw = area.allocated_width() as f64;
    let ah = area.allocated_height() as f64;
    let (cx, cy) = (aw / 2.0, ah / 2.0);
    let old = st.scale;
    let next = (old * factor).clamp(0.02, 16.0);
    st.tx = cx - (cx - st.tx) * (next / old);
    st.ty = cy - (cy - st.ty) * (next / old);
    st.scale = next;
    st.fit = false;
    drop(st);
    area.queue_draw();
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let start = args.get(1).cloned();

    let app = gtk::Application::new(
        Some("dev.limm.lumen.native"),
        gtk::gio::ApplicationFlags::NON_UNIQUE,
    );
    app.connect_activate(move |app| {
        glib::set_prgname(Some("lumen-native"));
        let window = gtk::ApplicationWindow::new(app);
        window.set_default_size(1280, 820);
        window.set_title("Lumen Native");

        let area = gtk::DrawingArea::new();
        let status = gtk::Label::new(None);
        status.set_xalign(0.0);
        status.set_margin_top(4);
        status.set_margin_bottom(4);
        status.set_margin_start(10);
        status.set_margin_end(10);

        let vbox = gtk::Box::new(gtk::Orientation::Vertical, 0);
        vbox.pack_start(&area, true, true, 0);
        vbox.pack_start(&status, false, false, 0);
        window.add(&vbox);

        let state = Rc::new(RefCell::new(View {
            pixbuf: None,
            surface: None,
            path: None,
            scale: 1.0,
            tx: 0.0,
            ty: 0.0,
            fit: true,
        }));

        {
            let state = state.clone();
            let area_draw = area.clone();
            area.connect_draw(move |_, ctx| {
                let st = state.borrow();
                ctx.set_source_rgb(0.063, 0.063, 0.071);
                ctx.paint().ok();
                if let Some(surf) = st.surface.as_ref() {
                    ctx.save();
                    ctx.translate(st.tx, st.ty);
                    ctx.scale(st.scale, st.scale);
                    ctx.set_source_surface(surf, 0.0, 0.0).ok();
                    ctx.paint().ok();
                    ctx.restore();
                }
                let _ = &area_draw;
                glib::Propagation::Stop
            });
        }

        let load: Rc<dyn Fn(PathBuf)> = {
            let state = state.clone();
            let area = area.clone();
            let status = status.clone();
            let window = window.clone();
            Rc::new(move |path: PathBuf| {
                let pb = match gdk_pixbuf::Pixbuf::from_file(&path) {
                    Ok(pb) => pb,
                    Err(err) => {
                        eprintln!("cannot open {}: {err}", path.display());
                        return;
                    }
                };
                let surface = to_surface(&pb);
                {
                    let mut st = state.borrow_mut();
                    st.pixbuf = Some(pb);
                    st.surface = surface;
                    st.path = Some(path.clone());
                    st.fit = true;
                }
                let name = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("?")
                    .to_string();
                window.set_title(&format!("{name} — Lumen Native"));
                fit_view(&state, &area);
                let st = state.borrow();
                let (w, h) = st
                    .pixbuf
                    .as_ref()
                    .map(|p| (p.width(), p.height()))
                    .unwrap_or((0, 0));
                let bytes = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                status.set_text(&format!(
                    "{name}   {w} × {h}   {:.1} KB   {:.0}%",
                    bytes as f64 / 1024.0,
                    st.scale * 100.0
                ));
                area.queue_draw();
            })
        };

        if let Some(path) = start.clone() {
            load(PathBuf::from(path));
        }

        area.add_events(
            gdk::EventMask::KEY_PRESS_MASK
                | gdk::EventMask::BUTTON_PRESS_MASK
                | gdk::EventMask::BUTTON_RELEASE_MASK
                | gdk::EventMask::POINTER_MOTION_MASK
                | gdk::EventMask::SCROLL_MASK,
        );
        area.set_can_focus(true);

        {
            let state = state.clone();
            let area_in = area.clone();
            let load = load.clone();
            area.connect_key_press_event(move |_, ev| {
                let key = ev.keyval();
                let keyname = key.name().map(|n| n.to_string()).unwrap_or_default();
                match key.to_unicode() {
                    Some('q') => std::process::exit(0),
                    Some('+') | Some('=') => zoom(&state, &area_in, 1.25),
                    Some('-') => zoom(&state, &area_in, 0.8),
                    Some('0') => fit_view(&state, &area_in),
                    _ => {
                        let step: i64 = if keyname == "Left" {
                            -1
                        } else if keyname == "Right" {
                            1
                        } else {
                            return glib::Propagation::Proceed;
                        };
                        let cur = state.borrow().path.clone();
                        if let Some(cur) = cur {
                            let list = siblings(&cur);
                            if let Some(i) = list.iter().position(|p| p == &cur) {
                                let n = list.len() as i64;
                                let next = &list[((i as i64 + step + n) % n) as usize];
                                load(next.clone());
                            }
                        }
                    }
                }
                glib::Propagation::Stop
            });
        }

        {
            let state = state.clone();
            let area_in = area.clone();
            area.connect_scroll_event(move |_, ev| {
                let (_, dy) = ev.scroll_deltas().unwrap_or((0.0, 0.0));
                let factor = if dy < 0.0 { 1.12 } else { 1.0 / 1.12 };
                zoom(&state, &area_in, factor);
                glib::Propagation::Stop
            });
        }

        {
            let dragging = Rc::new(RefCell::new(false));
            let last = Rc::new(RefCell::new((0.0_f64, 0.0_f64)));
            let state_p = state.clone();
            let area_p = area.clone();
            let drag_p = dragging.clone();
            let last_p = last.clone();
            area.connect_button_press_event(move |_, ev| {
                area_p.grab_focus();
                if ev.button() == 1 {
                    *drag_p.borrow_mut() = true;
                    *last_p.borrow_mut() = ev.position();
                }
                let _ = &state_p;
                glib::Propagation::Proceed
            });
            let state_m = state.clone();
            let area_m = area.clone();
            let drag_m = dragging.clone();
            let last_m = last.clone();
            area.connect_motion_notify_event(move |_, ev| {
                if !*drag_m.borrow() {
                    return glib::Propagation::Proceed;
                }
                let (lx, ly) = *last_m.borrow();
                let (x, y) = ev.position();
                {
                    let mut st = state_m.borrow_mut();
                    st.tx += x - lx;
                    st.ty += y - ly;
                    st.fit = false;
                }
                *last_m.borrow_mut() = (x, y);
                area_m.queue_draw();
                glib::Propagation::Stop
            });
            let area_r = area.clone();
            area.connect_button_release_event(move |_, _| {
                *dragging.borrow_mut() = false;
                let _ = &area_r;
                glib::Propagation::Proceed
            });
        }

        {
            let state = state.clone();
            let area_in = area.clone();
            area.connect_size_allocate(move |_, alloc| {
                let mut st = state.borrow_mut();
                if !st.fit {
                    return;
                }
                let (w, h) = match st.pixbuf.as_ref() {
                    Some(pb) => (pb.width() as f64, pb.height() as f64),
                    None => return,
                };
                st.scale = (alloc.width() as f64 / w).min(alloc.height() as f64 / h).min(1.0);
                st.tx = (alloc.width() as f64 - w * st.scale) / 2.0;
                st.ty = (alloc.height() as f64 - h * st.scale) / 2.0;
                drop(st);
                area_in.queue_draw();
            });
        }

        area.set_can_focus(true);
        window.show_all();
        area.grab_focus();
    });

    app.run_with_args::<&str>(&[]);
    std::process::exit(0);
}
