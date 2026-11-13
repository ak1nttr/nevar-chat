//! The overlay window. Over a game it's a see-through panel in a screen corner that clicks
//! pass through: who's talking, new messages. On its shortcut it becomes the quick menu in the
//! middle of the screen. One window does both, so nevar costs a single extra webview, and only
//! once the overlay is turned on or the menu is first opened.
//!
//! It works over windowed and borderless games. Exclusive fullscreen games draw over every
//! window; getting in there means injecting into the game, which anti-cheat treats as a cheat.

use std::sync::Mutex;

use nevar_core::{service::Service, store::Corner};
use serde::Serialize;
use tauri::{
    AppHandle, Emitter, LogicalSize, Manager, Monitor, PhysicalPosition, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder,
};
use tracing::warn;

pub const LABEL: &str = "overlay";

/// Logical sizes. The corner panel's content is anchored to its corner, the rest is clear.
const PANEL: (f64, f64) = (300.0, 360.0);
const MENU: (f64, f64) = (420.0, 560.0);
const MARGIN: f64 = 16.0;

#[derive(Default)]
pub struct Overlay(Mutex<Mode>);

#[derive(Default, Clone, Copy)]
struct Mode {
    menu_open: bool,
    /// The menu has had focus since it opened, so losing it means the user clicked away.
    menu_focused: bool,
    /// The panel has something to show (a call, a fresh message). Reported by the page.
    has_content: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ModePayload {
    menu: bool,
}

fn mode(app: &AppHandle) -> Mode {
    *app.state::<Overlay>().0.lock().unwrap()
}

fn update(app: &AppHandle, f: impl FnOnce(&mut Mode)) {
    f(&mut app.state::<Overlay>().0.lock().unwrap());
    sync(app);
}

pub fn menu_open(app: &AppHandle) -> bool {
    mode(app).menu_open
}

pub fn toggle_menu(app: &AppHandle) {
    update(app, |m| {
        m.menu_open = !m.menu_open;
        m.menu_focused = false;
    });
}

pub fn close_menu(app: &AppHandle) {
    if menu_open(app) {
        update(app, |m| m.menu_open = false);
    }
}

pub fn set_has_content(app: &AppHandle, has: bool) {
    if mode(app).has_content != has {
        update(app, |m| m.has_content = has);
    }
}

/// Focus changes of the overlay window: clicking back into the game closes the menu.
pub fn on_focus(app: &AppHandle, focused: bool) {
    let m = mode(app);
    if !m.menu_open {
        return;
    }
    if focused {
        update(app, |m| m.menu_focused = true);
    } else if m.menu_focused {
        close_menu(app);
    }
}

/// Puts the window where the current mode and settings want it, or hides it.
pub fn sync(app: &AppHandle) {
    let settings = app.state::<Service>().settings().overlay;
    let m = mode(app);
    let show_panel = settings.enabled && m.has_content;

    // While the overlay is on, the page must run to know when it has something to show.
    let window = if m.menu_open || settings.enabled { ensure_window(app) } else { app.get_webview_window(LABEL) };
    let Some(window) = window else { return };
    let _ = app.emit_to(LABEL, "overlay-mode", ModePayload { menu: m.menu_open });

    if !m.menu_open && !show_panel {
        let _ = window.hide();
        return;
    }
    let Some(monitor) = target_monitor(app) else { return };
    let scale = monitor.scale_factor();
    let (origin, size) = (monitor.position(), monitor.size());
    let (w, h) = if m.menu_open { MENU } else { PANEL };
    let (pw, ph) = ((w * scale) as i32, (h * scale) as i32);
    let margin = (MARGIN * scale) as i32;
    let (mw, mh) = (size.width as i32, size.height as i32);
    let (x, y) = if m.menu_open {
        ((mw - pw) / 2, (mh - ph) / 2)
    } else {
        match settings.corner {
            Corner::TopLeft => (margin, margin),
            Corner::TopRight => (mw - pw - margin, margin),
            Corner::BottomLeft => (margin, mh - ph - margin),
            Corner::BottomRight => (mw - pw - margin, mh - ph - margin),
        }
    };
    let _ = window.set_size(LogicalSize::new(w, h));
    let _ = window.set_position(PhysicalPosition::new(origin.x + x, origin.y + y));

    if m.menu_open {
        let _ = window.set_ignore_cursor_events(false);
        let _ = window.set_focusable(true);
        let _ = window.show();
        let _ = window.set_focus();
    } else {
        // Never takes focus from the game, and clicks go through to it.
        let _ = window.set_focusable(false);
        let _ = window.set_ignore_cursor_events(true);
        let _ = window.show();
    }
}

/// The screen the user is on: where the mouse is (a game keeps it on its own screen).
fn target_monitor(app: &AppHandle) -> Option<Monitor> {
    app.cursor_position()
        .ok()
        .and_then(|p| app.monitor_from_point(p.x, p.y).ok().flatten())
        .or_else(|| app.primary_monitor().ok().flatten())
}

fn ensure_window(app: &AppHandle) -> Option<WebviewWindow> {
    if let Some(window) = app.get_webview_window(LABEL) {
        return Some(window);
    }
    let built = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("index.html".into()))
        .title("nevar")
        .inner_size(PANEL.0, PANEL.1)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .focusable(false)
        .focused(false)
        .visible(false)
        .build();
    match built {
        Ok(window) => Some(window),
        Err(e) => {
            warn!("couldn't open the overlay window: {e}");
            None
        }
    }
}
