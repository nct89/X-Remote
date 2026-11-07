//! Host-side X11 clipboard (Linux only): receive the clipboard the controller sends
//! (`5|||<text>`) and own the `CLIPBOARD` selection so Ctrl+V on this machine pastes it.
//!
//! Why: an IME on the controller (clipboard mode) composes diacritics by putting text on
//! the clipboard and sending Ctrl+V. The controller forwards `5|||<text>` then Ctrl+V; if
//! the host drops the `5`, Ctrl+V pastes the old clipboard. No xclip/xsel here, so we hold
//! the selection ourselves with x11rb on a dedicated thread.

use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

use x11rb::connection::Connection;
use x11rb::protocol::xproto::{
    Atom, AtomEnum, ConnectionExt as _, CreateWindowAux, EventMask, PropMode, SelectionNotifyEvent,
    WindowClass, SELECTION_NOTIFY_EVENT,
};
use x11rb::protocol::Event;
use x11rb::wrapper::ConnectionExt as _;
use x11rb::CURRENT_TIME;

/// Background clipboard owner; `set` installs new content (waits until it owns the selection).
pub struct ClipboardOwner {
    tx: Sender<(String, Sender<bool>)>,
}

impl ClipboardOwner {
    pub fn new() -> Result<ClipboardOwner, String> {
        let (tx, rx) = mpsc::channel::<(String, Sender<bool>)>();
        let (ready_tx, ready_rx) = mpsc::channel::<Result<(), String>>();
        std::thread::Builder::new()
            .name("uv-clipboard".into())
            .spawn(move || run(rx, ready_tx))
            .map_err(|e| e.to_string())?;
        ready_rx.recv_timeout(Duration::from_secs(3)).map_err(|_| "clipboard did not start".to_string())??;
        Ok(ClipboardOwner { tx })
    }

    /// Set the clipboard. Returns `true` once it owns the selection (Ctrl+V then pastes this).
    pub fn set(&self, text: &str) -> bool {
        let (ack_tx, ack_rx) = mpsc::channel();
        if self.tx.send((text.to_string(), ack_tx)).is_err() {
            return false;
        }
        ack_rx.recv_timeout(Duration::from_millis(500)).unwrap_or(false)
    }
}

fn run(rx: Receiver<(String, Sender<bool>)>, ready: Sender<Result<(), String>>) {
    let init = || -> Result<_, String> {
        let (conn, screen) = x11rb::connect(None).map_err(|e| format!("cannot open X11: {e}"))?;
        let root = conn.setup().roots[screen].root;
        let win = conn.generate_id().map_err(|e| e.to_string())?;
        conn.create_window(
            0,
            win,
            root,
            -10,
            -10,
            1,
            1,
            0,
            WindowClass::INPUT_ONLY,
            0,
            &CreateWindowAux::new().event_mask(EventMask::PROPERTY_CHANGE),
        )
        .map_err(|e| e.to_string())?;
        let atom = |t: &str| -> Result<Atom, String> {
            Ok(conn.intern_atom(false, t.as_bytes()).map_err(|e| e.to_string())?.reply().map_err(|e| e.to_string())?.atom)
        };
        let a = Atoms {
            clipboard: atom("CLIPBOARD")?,
            utf8: atom("UTF8_STRING")?,
            targets: atom("TARGETS")?,
            text: atom("TEXT")?,
            plain: atom("text/plain;charset=utf-8")?,
        };
        conn.flush().map_err(|e| e.to_string())?;
        Ok((conn, win, a))
    };
    let (conn, win, a) = match init() {
        Ok(v) => {
            let _ = ready.send(Ok(()));
            v
        }
        Err(e) => {
            let _ = ready.send(Err(e));
            return;
        }
    };
    let mut content: Option<Vec<u8>> = None;
    loop {
        // New command from the session serve loop.
        match rx.recv_timeout(Duration::from_millis(15)) {
            Ok((text, ack)) => {
                content = Some(text.into_bytes());
                let _ = conn.set_selection_owner(win, a.clipboard, CURRENT_TIME);
                let _ = conn.flush();
                let owns = conn
                    .get_selection_owner(a.clipboard)
                    .ok()
                    .and_then(|c| c.reply().ok())
                    .map(|r| r.owner == win)
                    .unwrap_or(false);
                let _ = ack.send(owns);
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        // X events: an app asks to paste / we lost ownership.
        while let Ok(Some(ev)) = conn.poll_for_event() {
            match ev {
                Event::SelectionRequest(e) => {
                    let prop = if e.property == x11rb::NONE { e.target } else { e.property };
                    let mut ok = false;
                    if let Some(d) = &content {
                        if e.target == a.targets {
                            let ds = [a.targets, a.utf8, a.text, a.plain, u32::from(AtomEnum::STRING)];
                            ok = conn
                                .change_property32(PropMode::REPLACE, e.requestor, prop, AtomEnum::ATOM, &ds)
                                .is_ok();
                        } else if [a.utf8, a.text, a.plain, u32::from(AtomEnum::STRING)].contains(&e.target) {
                            let kind = if e.target == a.text { a.utf8 } else { e.target };
                            ok = conn.change_property8(PropMode::REPLACE, e.requestor, prop, kind, d).is_ok();
                        }
                    }
                    let tb = SelectionNotifyEvent {
                        response_type: SELECTION_NOTIFY_EVENT,
                        sequence: 0,
                        time: e.time,
                        requestor: e.requestor,
                        selection: e.selection,
                        target: e.target,
                        property: if ok { prop } else { x11rb::NONE },
                    };
                    let _ = conn.send_event(false, e.requestor, EventMask::NO_EVENT, tb);
                    let _ = conn.flush();
                }
                Event::SelectionClear(_) => {
                    // Another app (e.g. a clipboard manager) took ownership — it keeps the content.
                    content = None;
                }
                _ => {}
            }
        }
    }
}

struct Atoms {
    clipboard: Atom,
    utf8: Atom,
    targets: Atom,
    text: Atom,
    plain: Atom,
}
