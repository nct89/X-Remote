//! Drop xfwm4 shadows on the chat panel and the show/hide button (Linux/X11 only).
//!
//! xfwm4 shadows every managed window (`show_frame_shadow=true`) no matter the
//! `_NET_WM_WINDOW_TYPE`, and it ignores `_GTK_FRAME_EXTENTS` set late. It does
//! not shadow a window with an X Shape: set a rectangle equal to the window,
//! nudge the width by 1 px, then restore so xfwm recomputes (checked 2026-11-05).

use std::collections::HashMap;
use std::time::{Duration, Instant};

use x11rb::connection::{Connection, RequestConnection};
use x11rb::protocol::shape::{self, ConnectionExt as _};
use x11rb::protocol::xproto::{AtomEnum, ClipOrdering, ConfigureWindowAux, ConnectionExt as _, Rectangle, Window};
use x11rb::rust_connection::RustConnection;

pub struct Shadow {
    conn: RustConnection,
    root: Window,
    client_list: u32,
    wm_pid: u32,
    net_wm_name: u32,
    utf8: u32,
    /// Window already handled → size at that time (a resize is done again).
    done: HashMap<Window, (u16, u16)>,
    last: Option<Instant>,
}

impl Shadow {
    pub fn new() -> Option<Shadow> {
        let (conn, screen) = x11rb::connect(None).ok()?;
        conn.extension_information(shape::X11_EXTENSION_NAME).ok()??;
        let root = conn.setup().roots[screen].root;
        let atom = |t: &str| conn.intern_atom(false, t.as_bytes()).ok()?.reply().ok().map(|r| r.atom);
        Some(Shadow {
            client_list: atom("_NET_CLIENT_LIST")?,
            wm_pid: atom("_NET_WM_PID")?,
            net_wm_name: atom("_NET_WM_NAME")?,
            utf8: atom("UTF8_STRING")?,
            conn,
            root,
            done: HashMap::new(),
            last: None,
        })
    }

    /// Scan at most 3 times a second. Drop the shadow on this process's windows
    /// whose title starts with `prefix`, if new or just resized.
    pub fn scan(&mut self, prefix: &str) {
        if self.last.is_some_and(|t| t.elapsed() < Duration::from_millis(300)) {
            return;
        }
        self.last = Some(Instant::now());
        let pid = std::process::id();
        let Some(prop) = self
            .conn
            .get_property(false, self.root, self.client_list, AtomEnum::WINDOW, 0, 4096)
            .ok()
            .and_then(|c| c.reply().ok())
        else {
            return;
        };
        let wins: Vec<Window> = prop.value32().map(|v| v.collect()).unwrap_or_default();
        self.done.retain(|w, _| wins.contains(w));
        for w in wins {
            let ours = self
                .conn
                .get_property(false, w, self.wm_pid, AtomEnum::CARDINAL, 0, 1)
                .ok()
                .and_then(|c| c.reply().ok())
                .and_then(|r| r.value32().and_then(|mut v| v.next()))
                == Some(pid);
            if !ours {
                continue;
            }
            let title = self
                .conn
                .get_property(false, w, self.net_wm_name, self.utf8, 0, 256)
                .ok()
                .and_then(|c| c.reply().ok())
                .map(|r| String::from_utf8_lossy(&r.value).into_owned())
                .unwrap_or_default();
            if !title.starts_with(prefix) {
                continue;
            }
            let Some(geo) = self.conn.get_geometry(w).ok().and_then(|c| c.reply().ok()) else { continue };
            if self.done.get(&w) == Some(&(geo.width, geo.height)) {
                continue;
            }
            let ok = self
                .conn
                .shape_rectangles(
                    shape::SO::SET,
                    shape::SK::BOUNDING,
                    ClipOrdering::UNSORTED,
                    w,
                    0,
                    0,
                    &[Rectangle { x: 0, y: 0, width: geo.width, height: geo.height }],
                )
                .is_ok();
            // Nudge width by 1 px and restore so xfwm recomputes the shadow from the new shape.
            let _ = self.conn.configure_window(w, &ConfigureWindowAux::new().width(u32::from(geo.width) + 1));
            let _ = self.conn.configure_window(w, &ConfigureWindowAux::new().width(u32::from(geo.width)));
            let _ = self.conn.flush();
            if ok {
                self.done.insert(w, (geo.width, geo.height));
            }
        }
    }
}
