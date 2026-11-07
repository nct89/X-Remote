//! The product UI: an UltraViewer-shaped connect screen, plus the session view.

use eframe::egui;
use egui::{Color32, RichText};
use uvproto::Ctl;

use crate::net::{Conn, Inbound};
use crate::uv::UvLink;

use crate::theme::TEXT;

// One accent blue for actions, neutral gray for secondary text,
// very light borders so white blocks stand off the background.
const GREEN: Color32 = Color32::from_rgb(0x16, 0xA3, 0x4A);
const AMBER: Color32 = Color32::from_rgb(0xD9, 0x77, 0x06);
const RED: Color32 = Color32::from_rgb(0xDC, 0x26, 0x26);

const VK_CTRL: u16 = 0x11;
const VK_ALT: u16 = 0x12;
const VK_SHIFT: u16 = 0x10;
const VK_C: u16 = 0x43;
const VK_V: u16 = 0x56;
const VK_X: u16 = 0x58;
const VK_TAB: u16 = 0x09;
const VK_ESC: u16 = 0x1B;
const VK_LWIN: u16 = 0x5B;
const VK_F4: u16 = 0x73;
const VK_D: u16 = 0x44;
const VK_E: u16 = 0x45;

/// Chat while this machine is the controller: history, peer name, and peer typing.
#[derive(Default)]
struct ChatCtl {
    lines: Vec<crate::hostmode::ChatLine>,
    name: String,
    typing: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum View {
    Connect,
    Session,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tone {
    Ok,
    Warn,
    Err,
}

/// Open channel: in-house agent (JSON) or a real UltraViewer peer.
enum Link {
    Agent(Conn),
    Uv(UvLink),
    /// Test only: record every command; `true` plays the agent.
    #[cfg(test)]
    Rec(Vec<Ctl>, bool),
}

impl Link {
    fn poll(&self) -> (Vec<Inbound>, Option<String>) {
        match self {
            Link::Agent(c) => c.poll(),
            Link::Uv(c) => c.poll(),
            #[cfg(test)]
            Link::Rec(..) => (Vec::new(), None),
        }
    }

    fn send(&mut self, ctl: &Ctl) {
        match self {
            Link::Agent(c) => c.send(ctl),
            Link::Uv(c) => c.send(ctl),
            #[cfg(test)]
            Link::Rec(v, _) => v.push(ctl.clone()),
        }
    }
}

/// One Windows wheel notch (`WHEEL_DELTA`).
const WHEEL_DELTA: i32 = 120;
/// Point `p` on draw rect `draw` (showing region `src` of image `full`) ->
/// normalized 0..1 over the **whole image**, as the UltraViewer/agent stream expects.
fn to_full(
    p: egui::Pos2,
    draw: egui::Rect,
    src: (f32, f32, f32, f32),
    full: (f32, f32),
) -> (f32, f32) {
    let (sx, sy, sw, sh) = src;
    let fx = ((p.x - draw.left()) / draw.width()).clamp(0.0, 1.0);
    let fy = ((p.y - draw.top()) / draw.height()).clamp(0.0, 1.0);
    (
        ((sx + fx * sw) / full.0).clamp(0.0, 1.0),
        ((sy + fy * sh) / full.1).clamp(0.0, 1.0),
    )
}

/// Accumulate egui scroll (points) and split out whole notches for the peer.
///
/// `px_per` is egui `Options::line_scroll_speed` (40 px/notch on native 0.31).
/// The fraction stays in `rem`, so a touchpad still reaches a full notch
/// instead of dropping the remainder.
fn wheel_notches(rem: &mut f32, delta: f32, px_per: f32) -> i32 {
    let step = if px_per > 0.0 { px_per } else { 40.0 };
    *rem += delta;
    let notches = (*rem / step).trunc();
    *rem -= notches * step;
    notches as i32
}

/// Random u64 without extra crates: hash time + pid with std's random key.
fn rand_u64() -> u64 {
    use std::hash::BuildHasher;
    std::collections::hash_map::RandomState::new()
        .hash_one((std::time::SystemTime::now(), std::process::id()))
}

/// 9-digit id (100000000..=999999999), like an UltraViewer id.
fn rand_id() -> String {
    (100_000_000 + rand_u64() % 900_000_000).to_string()
}

/// 5-digit password, new each time the app opens, like UltraViewer.
fn rand_pw() -> String {
    (10_000 + rand_u64() % 90_000).to_string()
}

fn id_ok(s: &str) -> bool {
    s.len() == 9 && s.bytes().all(|b| b.is_ascii_digit()) && !s.starts_with('0')
}

/// Read the saved id from `path`; if missing or bad, mint one and store it.
///
/// Old bug: the default id was hardcoded to the Windows box used in tests, so every
/// install of X-Remote claimed the same id as the peer.
pub fn stored_id(path: &std::path::Path) -> String {
    if let Ok(s) = std::fs::read_to_string(path) {
        let s = s.trim();
        if id_ok(s) {
            return s.to_string();
        }
    }
    let id = rand_id();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(path, format!("{id}\n"));
    id
}

/// Path of this machine's id: `$XDG_CONFIG_HOME/x-remote/own_id` (default `~/.config`),
/// or `%APPDATA%\X-Remote\own_id` on Windows.
pub fn id_file() -> Option<std::path::PathBuf> {
    let env = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(std::path::PathBuf::from);
    if cfg!(windows) {
        return env("APPDATA").map(|d| d.join("X-Remote").join("own_id"));
    }
    env("XDG_CONFIG_HOME")
        .or_else(|| env("HOME").map(|h| h.join(".config")))
        .map(|d| d.join("x-remote").join("own_id"))
}

pub struct App {
    /// Engine endpoint. `None` means no engine is available yet.
    engine: Option<String>,
    view: View,
    /// Next frame on the connect screen: focus the partner id field.
    ask_id_focus: bool,

    peer_id: String,
    peer_pw: String,

    status: String,
    tone: Tone,

    conn: Option<Link>,
    texture: Option<egui::TextureHandle>,
    size: [usize; 2],

    left_down: bool,
    right_down: bool,
    mid_down: bool,
    last_mouse: Option<egui::Pos2>,
    mods: (bool, bool, bool),

    /// Picture quality sent to the peer (30..90); can change mid-session.
    quality: i32,
    /// Fullscreen is on.
    fullscreen: bool,
    /// Skip aspect-lock for a few frames after an explicit resize.
    ratio_skip: u8,
    /// Last minimum size sent, in points. A new command is sent only when it changes.
    min_ratio: [f32; 2],
    /// `set_zoom_factor` takes effect next frame; then pin inner size to the design
    /// so Linux actually redraws (same-size `InnerSize` is ignored there).
    zoom_pending: bool,
    /// Keyboard focus was taken for the picture (so typing works as soon as the session starts).
    got_focus: bool,

    pending_connect: bool,
    shot_path: Option<std::path::PathBuf>,
    shot_requested: bool,
    /// Seconds to wait before the screenshot — a real UltraViewer session needs a first frame.
    shot_delay: u64,
    shot_start: Option<std::time::Instant>,

    /// Real UltraViewer mode: (peer id, peer password, own id, own password).
    /// Rebuilt from the fields each time the user connects (see `pick_uv`).
    uv: Option<(String, String, String, String)>,
    /// Set by `--uv`: always use UltraViewer even if the id is not numeric.
    uv_mode: bool,
    /// This machine's UltraViewer id and password (`--own-id` / `--own-pw`).
    own_id: String,
    own_pw: String,
    /// Wheel remainder that is not yet one notch; carried into the next frame.
    wheel_rem: f32,

    /// Peer monitor layout (UltraViewer) and the monitor being viewed (`None` = all).
    monitors: Vec<uvsession::Monitor>,
    mon_ix: Option<usize>,
    /// Peer command 49: `Some(true)` = view only.
    view_only: Option<bool>,
    /// Quality changed during the session: resend after each reconnect.
    quality_sent: bool,
    /// Hide the peer wallpaper (UltraViewer `23|||0`).
    hide_wall: bool,
    /// Max frame rate chosen for the agent (0 = let the agent decide).
    fps: u16,
    /// Latency probe: pending ping id, send time, and the latest result.
    ping_id: u16,
    ping_wait: Option<(u16, std::time::Instant)>,
    ping_at: std::time::Instant,
    lag_ms: Option<f32>,
    /// UltraViewer session: Ctrl+V types this machine's clipboard instead of
    /// pasting the peer clipboard (UltraViewer clipboard is not decoded yet).
    paste_type: bool,
    /// This Ctrl+V already became `Event::Paste` (sent); ignore the matching V release.
    did_paste: bool,
    /// Saw the C, X, V press (in that order). A release with no press means
    /// egui swallowed the press to do Copy/Cut/Paste.
    saw_cxv: [bool; 3],

    /// Retry after a dropped session: deadline and attempt count.
    reconnect_at: Option<std::time::Instant>,
    reconnect_tries: u32,

    /// Rate numbers shown on the bottom bar.
    fps_now: f32,
    kb_now: f32,
    n_frames: u32,
    n_bytes: u64,
    total_frames: u64,
    rate_at: std::time::Instant,

    /// Settings -> "Always on top".
    on_top: bool,
    /// Settings -> "Type Vietnamese with the Linux IME (fcitx)" for the chat box. Default OFF:
    /// text from the controller already has marks (EVKey -> clipboard); the Linux IME only adds
    /// a preedit highlight in the chat box (user report 08:24).
    linux_ime: bool,

    /// Host mode runs in the background: granted id + status.
    host: crate::hostmode::HostShare,

    /// Chat window with the controller is open.
    chat_open: bool,
    /// The chat window has been created at least once. macOS must not destroy
    /// that window mid-frame (dropping NSWindow inside the event loop hangs the app).
    chat_made: bool,
    /// Previous frame had a session (open/hide chat when a session starts/ends).
    had_sess: bool,
    /// Chat draft.
    draft: String,
    /// Chat lines seen last frame (scroll down when a new line arrives).
    seen_n: usize,
    /// Told the controller "typing" (`29`) and have not yet sent "stopped".
    typing_out: bool,
    /// Last edit of the draft (6 s idle -> send "stopped typing").
    edit_at: std::time::Instant,
    /// Chat panel is collapsed to a tab on the right edge.
    chat_dock: bool,
    /// Folder just picked (picker runs in the background), waiting to be inserted.
    dir_q: crate::attach::DirQueue,
    /// Folder links currently in the draft: (visible label, path).
    link_q: Vec<(String, std::path::PathBuf)>,
    /// Chat panel position after the user dragged it (`None` = docked to the right).
    chat_pos: Option<egui::Pos2>,
    /// Chat for the session where this machine CONTROLS another (UltraViewer).
    chat_ctl: ChatCtl,
    /// Session window title already set (changes once the peer name is known).
    sess_title: String,
    /// Session toolbar is collapsed.
    bar_hide: bool,
    /// History of ids we controlled (newest first) + whether the suggestion list is open.
    hist: Vec<crate::history::HistRec>,
    suggest_on: bool,
    /// Peer OS for the current session (`windows` / `linux` / `macos`).
    peer_os: String,
    /// This session was written to history (written on the first picture).
    hist_wrote: bool,
    /// Latest peer picture (width, height, RGB) — for Screenshot.
    last_pic: Option<(usize, usize, std::sync::Arc<Vec<u8>>)>,
    /// Strip the xfwm shadow on the chat window (X Shape).
    #[cfg(target_os = "linux")]
    shaper: Option<crate::shadow::Shadow>,
    #[cfg(target_os = "linux")]
    shadow_tried: bool,
    /// Files and folders the user picked to send over chat.
    atts: crate::attach::AttList,
    att_seen: usize,
}

impl App {
    pub fn new(engine: Option<String>) -> Self {
        let (status, tone) = match &engine {
            Some(_) => ("Sẵn sàng kết nối".to_string(), Tone::Ok),
            None => ("Sẵn sàng kết nối".to_string(), Tone::Ok),
        };

        // This machine's password: random each launch (like UltraViewer). Set
        // `XREMOTE_HOST_PW` to pin it while testing.
        let own_pw = std::env::var("XREMOTE_HOST_PW")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(rand_pw);
        // Register as a background host so other machines can control this one (like UltraViewer).
        let host = crate::hostmode::start(own_pw.clone());
        if std::env::var_os("XREMOTE_CHAT_DEMO").is_some() {
            crate::hostmode::load_demo_chat(&host);
        }

        Self {
            engine,
            view: View::Connect,
            ask_id_focus: false,
            peer_id: String::new(),
            peer_pw: String::new(),
            status,
            tone,
            conn: None,
            texture: None,
            size: [0, 0],
            left_down: false,
            right_down: false,
            mid_down: false,
            last_mouse: None,
            mods: (false, false, false),
            quality: 60,
            fullscreen: false,
            ratio_skip: 0,
            min_ratio: [0.0, 0.0],
            zoom_pending: false,
            got_focus: false,
            pending_connect: false,
            shot_path: None,
            shot_requested: false,
            shot_delay: 0,
            shot_start: None,
            uv: None,
            uv_mode: false,
            own_id: rand_id(),
            own_pw,
            wheel_rem: 0.0,
            monitors: Vec::new(),
            mon_ix: None,
            view_only: None,
            quality_sent: false,
            hide_wall: false,
            fps: 0,
            ping_id: 0,
            ping_wait: None,
            ping_at: std::time::Instant::now(),
            lag_ms: None,
            paste_type: false,
            did_paste: false,
            saw_cxv: [false; 3],
            reconnect_at: None,
            reconnect_tries: 0,
            fps_now: 0.0,
            kb_now: 0.0,
            n_frames: 0,
            n_bytes: 0,
            total_frames: 0,
            rate_at: std::time::Instant::now(),
            on_top: false,
            linux_ime: false,
            host,
            // Hidden until a session exists; opens when a session starts (see `chat_panel`).
            chat_open: false,
            chat_made: false,
            had_sess: false,
            draft: String::new(),
            seen_n: 0,
            typing_out: false,
            edit_at: std::time::Instant::now(),
            chat_dock: false,
            dir_q: Default::default(),
            link_q: Vec::new(),
            chat_pos: None,
            chat_ctl: ChatCtl::default(),
            sess_title: String::new(),
            hist: crate::history::load(),
            suggest_on: false,
            peer_os: String::new(),
            hist_wrote: false,
            bar_hide: false,
            last_pic: None,
            #[cfg(target_os = "linux")]
            shaper: None,
            #[cfg(target_os = "linux")]
            shadow_tried: false,
            atts: Default::default(),
            att_seen: 0,
        }
    }

    /// Render one frame, save it as JPEG, then quit. Used to verify the UI on a
    /// machine we cannot screenshot (macOS blocks Screen Recording over SSH).
    pub fn screenshot_to(&mut self, path: std::path::PathBuf) {
        self.shot_path = Some(path);
    }

    /// Capture after `secs` seconds so a real UltraViewer session has its first frame.
    pub fn set_shot_delay(&mut self, secs: u64) {
        self.shot_delay = secs;
    }

    /// Turn on real UltraViewer transport; an empty own id keeps the default.
    pub fn set_uv(&mut self, peer_id: String, peer_pw: String, own_id: String, own_pw: String) {
        self.set_own(own_id, own_pw);
        self.peer_id = peer_id;
        self.peer_pw = peer_pw;
        self.uv_mode = true;
        self.uv = self.pick_uv();
    }

    /// Set this machine's id/password; empty keeps the default.
    pub fn set_own(&mut self, own_id: String, own_pw: String) {
        if !own_id.trim().is_empty() {
            self.own_id = own_id.trim().to_string();
        }
        if !own_pw.trim().is_empty() {
            self.own_pw = own_pw.trim().to_string();
        }
    }

    /// UltraViewer peer from the fields as they are right now.
    ///
    /// Called every time the user connects: this tuple used to be built only on
    /// the first connect, so after Disconnect + a new id the app still dialed the old host.
    /// Peer id for connect: drop all whitespace if the rest is digits. The id field
    /// groups digits by threes like UltraViewer ("100 000 002" — picked from suggestions, or pasted).
    /// Whitespace used to fail the "id must be digits" check (report 08:55).
    /// A non-numeric id (agent host name) only has surrounding whitespace trimmed.
    fn clean_id(&self) -> String {
        norm_id(&self.peer_id)
    }

    /// `None` when the id is not digits and `--uv` is off (use the engine).
    fn pick_uv(&self) -> Option<(String, String, String, String)> {
        // With `--agent`, the id (also an UltraViewer id, used by the login macro)
        // must go to the agent; numeric ids used to be forced onto UltraViewer.
        if self.engine.is_some() && !self.uv_mode {
            return None;
        }
        let id = self.clean_id();
        let uv_id = !id.is_empty() && id.len() <= 12 && id.chars().all(|c| c.is_ascii_digit());
        if !(uv_id || (self.uv_mode && !id.is_empty())) {
            return None;
        }
        Some((id, self.peer_pw.clone(), self.own_id.clone(), self.own_pw.clone()))
    }

    fn handle_screenshot(&mut self, ctx: &egui::Context) {
        if self.shot_path.is_none() {
            return;
        }

        if self.shot_delay > 0 {
            let start = *self.shot_start.get_or_insert_with(std::time::Instant::now);
            let waited = start.elapsed().as_secs() >= self.shot_delay;
            // Wait for a real frame before capturing, so the shot is not blank.
            if !waited || self.texture.is_none() {
                ctx.request_repaint_after(std::time::Duration::from_millis(200));
                return;
            }
        }

        if !self.shot_requested {
            self.shot_requested = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
            ctx.request_repaint();
            return;
        }

        let shot = ctx.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });

        if let Some(image) = shot {
            if let Some(path) = self.shot_path.take() {
                save_jpeg(&path, &image);
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        } else {
            ctx.request_repaint();
        }
    }

    /// Prefill peer id/password from the command line without turning on `--uv`.
    pub fn set_peer(&mut self, id: String, pw: String) {
        if !id.trim().is_empty() {
            self.peer_id = id;
        }
        if !pw.is_empty() {
            self.peer_pw = pw;
        }
    }

    pub fn autoconnect(&mut self) {
        if self.engine.is_some() || self.uv.is_some() || !self.peer_id.trim().is_empty() {
            self.pending_connect = true;
        }
    }

    // -- connection -------------------------------------------------------

    fn connect(&mut self, ctx: &egui::Context) {
        self.chat_ctl = ChatCtl::default();
        self.peer_os.clear();
        self.hist_wrote = false;
        self.suggest_on = false;
        if std::env::var("UV_DEBUG_INPUT").is_ok() {
            eprintln!("[ui] connect: uv={} engine={:?} id={:?}", self.uv.is_some(), self.engine, self.peer_id);
        }
        // A numeric id is an UltraViewer id: go straight to UltraViewer, not the
        // in-house engine. Always rebuild from the fields so the previous peer does not stick.
        self.uv = self.pick_uv();
        self.clear_sess();

        // Real UltraViewer mode: talk to the UltraViewer server directly.
        if self.uv.is_some() {
            self.open_uv(ctx);
            return;
        }

        // No agent and the id is not digits: there is no path to connect.
        // (Used to fall into `direct_connect` — an old probe hardcoded the dev machine's
        // Windows IP and host name, blocked the UI on id lookup, and never opened a session.)
        if self.engine.is_none() {
            self.status = if self.peer_id.trim().is_empty() {
                "Nhập ID đối tác (dãy số trên UltraViewer máy cần điều khiển)".into()
            } else {
                "ID đối tác phải là dãy số, như trên UltraViewer máy cần điều khiển".into()
            };
            self.tone = Tone::Err;
            return;
        }

        let Some(addr) = self.engine.clone() else {
            self.status = "Chưa có engine để kết nối".into();
            self.tone = Tone::Warn;
            return;
        };

        match Conn::connect(&addr, ctx) {
            Ok(mut conn) => {
                conn.send(&Ctl {
                    t: "login".into(),
                    id: self.clean_id(),
                    pw: self.peer_pw.clone(),
                    ..Default::default()
                });
                self.conn = Some(Link::Agent(conn));
                self.view = View::Session;
                self.texture = None;
                self.got_focus = false;
                self.status = format!("Đang kết nối tới {}", group_id(&self.clean_id()));
                self.tone = Tone::Ok;
                // Like an UltraViewer session: widen the window, or the session toolbar
                // loses buttons inside the 276 px connect column.
                self.resize_win(ctx, SESS_SIZE[0], SESS_SIZE[1]);
            }
            Err(e) => {
                self.status = format!("Không kết nối được engine: {e}");
                self.tone = Tone::Err;
            }
        }
    }

    /// Open an UltraViewer session to the peer stored in `self.uv`.
    fn open_uv(&mut self, ctx: &egui::Context) {
        if let Some((id, pw, oid, opw)) = self.uv.clone() {
            let link = UvLink::connect(&id, &pw, &oid, &opw, ctx);
            self.conn = Some(Link::Uv(link));
            self.view = View::Session;
            self.texture = None;
            self.got_focus = false;
            // Do not reset the counter here: each retry would go back to 0 and
            // the app would hammer connect instead of backing off.
            self.reconnect_at = None;
            self.status = if self.reconnect_tries > 0 {
                format!("Đang kết nối lại tới {id} (lần {}) …", self.reconnect_tries + 1)
            } else {
                format!("Đang kết nối UltraViewer tới {id} …")
            };
            self.tone = Tone::Ok;
            self.resize_win(ctx, SESS_SIZE[0], SESS_SIZE[1]);
        }
    }

    /// Reopen a dropped session (shared by the agent and UltraViewer).
    fn reconnect(&mut self, ctx: &egui::Context) {
        if self.uv.is_some() {
            // Reconnect to the peer of the session that just dropped; do not reread the fields.
            self.open_uv(ctx);
            return;
        }
        let Some(addr) = self.engine.clone() else {
            return;
        };
        match Conn::connect(&addr, ctx) {
            Ok(mut conn) => {
                conn.send(&Ctl {
                    t: "login".into(),
                    id: self.clean_id(),
                    pw: self.peer_pw.clone(),
                    ..Default::default()
                });
                self.conn = Some(Link::Agent(conn));
                self.status = "Đã kết nối lại".into();
                self.tone = Tone::Ok;
            }
            Err(e) => {
                self.reconnect_tries += 1;
                let backoff = 500 * self.reconnect_tries.min(6) as u64;
                self.reconnect_at =
                    Some(std::time::Instant::now() + std::time::Duration::from_millis(backoff));
                self.status = format!("Chưa kết nối lại được: {e}");
                self.tone = Tone::Warn;
            }
        }
    }

    /// Focus is still on the picture (already gone) or the chat window, so keys
    /// never reach the id and password fields. Return to the main window, drop
    /// any IME preedit, and focus the id field on the next frame.
    fn return_focus(&mut self, ctx: &egui::Context) {
        self.ask_id_focus = true;
        ctx.memory_mut(|m| {
            if let Some(id) = m.focused() {
                m.surrender_focus(id);
            }
        });
        ctx.output_mut(|o| o.ime = None);
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    }

    /// Drop the connection off the UI thread. `Drop` for an UltraViewer session
    /// waits for disconnect word `19`. Doing that inside a frame freezes the
    /// window (a Mac is mid-frame) and the id and password fields stop accepting text.
    fn release_conn(&mut self) {
        if let Some(conn) = self.conn.take() {
            std::thread::Builder::new()
                .name("uv-stop".into())
                .spawn(move || drop(conn))
                .ok();
        }
    }

    /// User pressed Disconnect: close the session and cancel any pending retry.
    ///
    /// `reconnect_at` used to stay set, so Disconnect right as the session dropped
    /// reopened it a few hundred milliseconds later and pulled the user back into the session.
    fn disconnect(&mut self, ctx: &egui::Context) {
        self.release_conn();
        self.texture = None;
        self.view = View::Connect;
        self.left_down = false;
        self.right_down = false;
        self.mid_down = false;
        self.mods = (false, false, false);
        self.last_mouse = None;
        self.wheel_rem = 0.0;
        self.reconnect_at = None;
        self.reconnect_tries = 0;
        self.clear_sess();
        if self.fullscreen {
            self.fullscreen = false;
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
        }
        self.resize_win(ctx, WIN_SIZE[0], WIN_SIZE[1]);
        self.return_focus(ctx);
        self.status = "Đã ngắt kết nối".into();
        self.tone = Tone::Warn;
    }

    /// Resize the window: narrow like UltraViewer's left column while entering info,
    /// wider once a session is open.
    fn resize_win(&mut self, ctx: &egui::Context, width: f32, height: f32) {
        // The next frames must not immediately overwrite this size with the aspect lock.
        self.ratio_skip = 3;
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(width, height)));
    }

    /// Keep the current screen's aspect ratio. Dragging a corner or maximizing
    /// only changes the window size; text and buttons scale with it.
    fn lock_ratio(&mut self, ctx: &egui::Context) {
        if self.fullscreen || ctx.input(|i| i.viewport().fullscreen == Some(true)) {
            return;
        }
        let design = match self.view {
            View::Connect => egui::vec2(WIN_SIZE[0], WIN_SIZE[1]),
            View::Session => egui::vec2(SESS_SIZE[0], SESS_SIZE[1]),
        };
        let zoom = ctx.zoom_factor().max(0.05);
        let monitor = ctx.input(|i| i.viewport().monitor_size);
        let (zoom_min, zoom_max) = zoom_bounds(design, zoom, monitor);
        let min_pts = design * (zoom_min / zoom);
        let min_gui = [min_pts.x, min_pts.y];
        if (self.min_ratio[0] - min_gui[0]).abs() > 0.5 || (self.min_ratio[1] - min_gui[1]).abs() > 0.5
        {
            self.min_ratio = min_gui;
            ctx.send_viewport_cmd(egui::ViewportCommand::MinInnerSize(min_pts));
        }
        if self.ratio_skip > 0 {
            self.ratio_skip -= 1;
            return;
        }

        // Zoom is applied at the start of this pass. Pin inner size to the design
        // in *points* so physical pixels = design × zoom (Linux ignores a no-op resize).
        if self.zoom_pending {
            self.zoom_pending = false;
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(design));
            self.ratio_skip = 2;
            return;
        }

        let Some(rect) = ctx.input(|i| i.viewport().inner_rect) else {
            return;
        };
        let size = rect.size();
        if size.x < 8.0 || size.y < 8.0 {
            return;
        }
        let maximized = ctx.input(|i| i.viewport().maximized == Some(true));
        let (locked, target) = if maximized {
            (design * (zoom_max / zoom), zoom_max)
        } else {
            next_ratio(size, design, zoom, monitor)
        };
        let off = (size.x - locked.x).abs() > 1.5 || (size.y - locked.y).abs() > 1.5;
        if maximized {
            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(false));
            if let Some(mon) = monitor.filter(|m| m.x > 32.0 && m.y > 32.0) {
                let pos = egui::pos2(
                    ((mon.x - locked.x) * 0.5).max(0.0),
                    ((mon.y - locked.y) * 0.5).max(0.0),
                );
                ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(pos));
            }
        }
        if off || maximized {
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(locked));
        }
        if (target - zoom).abs() > 0.02 {
            ctx.set_zoom_factor(target);
            self.zoom_pending = true;
            ctx.request_repaint();
        }
    }

    fn send(&mut self, ctl: Ctl) {
        if let Some(conn) = self.conn.as_mut() {
            conn.send(&ctl);
        }
    }

    fn mouse(&mut self, kind: &str, nx: f32, ny: f32) {
        self.send(Ctl {
            t: "mouse".into(),
            k: kind.into(),
            x: nx,
            y: ny,
            ..Default::default()
        });
    }

    /// Mouse event plus button index: 0 left, 1 middle, 2 right (the peer needs this for
    /// WM_*BUTTON*; "move" does not).
    fn mouse_btn(&mut self, kind: &str, nx: f32, ny: f32, b: u8) {
        self.send(Ctl {
            t: "mouse".into(),
            k: kind.into(),
            x: nx,
            y: ny,
            b,
            ..Default::default()
        });
    }

    /// Name of the selected quality level, shown on the session bar.
    fn quality_name(&self) -> &'static str {
        match self.quality {
            v if v <= 40 => "Thấp",
            v if v >= 80 => "Cao",
            _ => "Vừa",
        }
    }

    /// Send Ctrl+Alt+Del to the peer: three keys down, then up in reverse.
    fn send_cad(&mut self) {
        const VK_CTRL: u16 = 0x11;
        const VK_ALT: u16 = 0x12;
        const VK_DEL: u16 = 0x2E;
        for (vk, down) in [
            (VK_CTRL, true),
            (VK_ALT, true),
            (VK_DEL, true),
            (VK_DEL, false),
            (VK_ALT, false),
            (VK_CTRL, false),
        ] {
            self.send(Ctl {
                t: "key".into(),
                k: "vk".into(),
                v: vk,
                down,
                ..Default::default()
            });
        }
        self.status = "Đã gửi Ctrl+Alt+Del".into();
        self.tone = Tone::Ok;
    }

    /// Control event from the peer (agent or UltraViewer stream).
    fn on_event(&mut self, ctx: &egui::Context, ev: uvproto::Event) {
        match ev.t.as_str() {
            "hello" => {
                self.status = format!("Phiên: {}", ev.title);
                self.tone = Tone::Ok;
                self.ping_wait = None;
                // Peer just connected (or reconnected) and forgot settings: send them again.
                if self.quality_sent {
                    let v = self.quality as u16;
                    self.send(Ctl { t: "quality".into(), v, ..Default::default() });
                }
                if self.hide_wall {
                    self.send(Ctl { t: "wallpaper".into(), v: 0, ..Default::default() });
                }
                if self.fps > 0 {
                    let v = self.fps;
                    self.send(Ctl { t: "fps".into(), v, ..Default::default() });
                }
            }
            "info" => {
                self.status = ev.msg;
                self.tone = Tone::Ok;
            }
            "error" => {
                self.status = format!("Máy đích báo lỗi: {}", ev.msg);
                self.tone = Tone::Err;
            }
            "pong" => {
                if let Some((id, at)) = self.ping_wait {
                    if ev.w == id as u32 {
                        self.lag_ms = Some(at.elapsed().as_secs_f32() * 1000.0);
                        self.ping_wait = None;
                    }
                }
            }
            "clip" => {
                if !ev.s.is_empty() {
                    ctx.copy_text(ev.s);
                    self.status = "Đã chép clipboard từ máy đích sang máy này".into();
                    self.tone = Tone::Ok;
                }
            }
            _ => {}
        }
    }

    /// Talking to the JSON agent (clipboard, fps, ping), not UltraViewer.
    fn is_agent(&self) -> bool {
        match &self.conn {
            Some(Link::Agent(_)) => true,
            #[cfg(test)]
            Some(Link::Rec(_, agent)) => *agent,
            _ => false,
        }
    }

    /// Ping the agent every 2 seconds; drop a ping with no reply after 5 seconds.
    fn ping_tick(&mut self) {
        if !self.is_agent() {
            return;
        }
        if let Some((_, at)) = self.ping_wait {
            if at.elapsed() < std::time::Duration::from_secs(5) {
                return;
            }
            self.ping_wait = None;
            self.lag_ms = None;
        }
        if self.ping_at.elapsed() < std::time::Duration::from_secs(2) {
            return;
        }
        self.ping_at = std::time::Instant::now();
        self.ping_id = self.ping_id.wrapping_add(1);
        let id = self.ping_id;
        self.ping_wait = Some((id, std::time::Instant::now()));
        self.send(Ctl { t: "ping".into(), v: id, ..Default::default() });
    }

    /// Clear state that belongs to one session (monitor layout, rights, latency).
    fn clear_sess(&mut self) {
        self.monitors.clear();
        self.mon_ix = None;
        self.view_only = None;
        self.ping_wait = None;
        self.lag_ms = None;
    }

    /// Visible image rect `(x, y, w, h)` in pixels: one chosen monitor, or the whole image.
    /// A monitor outside the image (layout does not match the canvas) falls back to the whole image, like the Python build.
    fn view_rect(&self, w: usize, h: usize) -> (f32, f32, f32, f32) {
        if let Some(m) = self.mon_ix.and_then(|i| self.monitors.get(i)) {
            let fits = m.x >= 0
                && m.y >= 0
                && m.w > 0
                && m.h > 0
                && (m.x + m.w) as usize <= w
                && (m.y + m.h) as usize <= h;
            if fits {
                return (m.x as f32, m.y as f32, m.w as f32, m.h as f32);
            }
        }
        (0.0, 0.0, w as f32, h as f32)
    }

    /// Press then release a key with Ctrl (Ctrl+C/X/V). egui swallows the press of these
    /// chords to do Copy/Cut/Paste, so both events must be sent to the peer.
    /// On macOS the user holds Cmd, not Ctrl, so wrap Ctrl in as well.
    fn send_ctrl_key(&mut self, vk: u16) {
        let wrap_ctrl = !self.mods.0;
        let mut seq = Vec::new();
        if wrap_ctrl {
            seq.push((VK_CTRL, true));
        }
        seq.push((vk, true));
        seq.push((vk, false));
        if wrap_ctrl {
            seq.push((VK_CTRL, false));
        }
        for (vk, down) in seq {
            self.send(Ctl { t: "key".into(), k: "vk".into(), v: vk, down, ..Default::default() });
        }
    }

    /// User pasted (Ctrl+V / Cmd+V) on the picture.
    fn paste_text(&mut self, text: String) {
        self.did_paste = true;
        if self.is_agent() {
            // The agent sets the peer clipboard first, then receives Ctrl+V (same stream).
            self.send(Ctl { t: "clip".into(), s: text, ..Default::default() });
            self.send_ctrl_key(VK_V);
            self.status = "Đã gửi clipboard và dán trên máy đích".into();
            self.tone = Tone::Ok;
            return;
        }
        if !self.paste_type {
            self.send_ctrl_key(VK_V);
            return;
        }
        // The user is still holding Ctrl (from Ctrl+V) and the peer has Ctrl down too:
        // typing straight through becomes Ctrl+letter (seen on a real session). Release the
        // modifier on the peer, type, then press it again to match the user's hand.
        let held_mods: Vec<u16> = [(self.mods.0, VK_CTRL), (self.mods.1, VK_ALT), (self.mods.2, VK_SHIFT)]
            .into_iter()
            .filter_map(|(held, vk)| held.then_some(vk))
            .collect();
        for &vk in &held_mods {
            self.send(Ctl { t: "key".into(), k: "vk".into(), v: vk, down: false, ..Default::default() });
        }
        let (typed, skipped) = self.type_text(&text);
        for &vk in &held_mods {
            self.send(Ctl { t: "key".into(), k: "vk".into(), v: vk, down: true, ..Default::default() });
        }
        self.status = if skipped > 0 {
            format!("Đã gõ {typed} ký tự từ clipboard, bỏ {skipped} ký tự máy đích chưa nhận")
        } else {
            format!("Đã gõ {typed} ký tự từ clipboard")
        };
        self.tone = if skipped > 0 { Tone::Warn } else { Tone::Ok };
    }

    /// Retype a string on the peer. Returns (chars typed, chars skipped).
    ///
    /// Types every `char_events` code point that encodes (including Vietnamese); newline becomes
    /// Enter. Length is capped so pasting a huge file cannot lock the peer for minutes.
    fn type_text(&mut self, text: &str) -> (usize, usize) {
        const MAX_CHARS: usize = 4000;
        let (mut typed, mut skipped) = (0usize, 0usize);
        for (i, ch) in text.chars().enumerate() {
            if i >= MAX_CHARS {
                skipped += 1;
                continue;
            }
            let ch = match ch {
                '\r' => continue,
                '\n' => '\r',
                c => c,
            };
            if uvsession::input::char_events(ch).is_some() {
                self.send(Ctl { t: "key".into(), k: "char".into(), c: ch.to_string(), ..Default::default() });
                typed += 1;
            } else {
                skipped += 1;
            }
        }
        (typed, skipped)
    }

    /// Press a chord in order, then release it in reverse (Alt+Tab, Win+D, ...).
    fn send_combo(&mut self, name: &str, keys: &[u16]) {
        for &vk in keys {
            self.send(Ctl { t: "key".into(), k: "vk".into(), v: vk, down: true, ..Default::default() });
        }
        for &vk in keys.iter().rev() {
            self.send(Ctl { t: "key".into(), k: "vk".into(), v: vk, down: false, ..Default::default() });
        }
        self.status = format!("Đã gửi {name}");
        self.tone = Tone::Ok;
    }

    fn pump(&mut self, ctx: &egui::Context) {
        let (items, closed) = match self.conn.as_ref() {
            Some(conn) => conn.poll(),
            None => return,
        };

        let mut newest: Option<crate::net::Pic> = None;
        for item in items {
            match item {
                Inbound::Frame(k) => newest = Some(k),
                Inbound::Event(ev) => self.on_event(ctx, ev),
                Inbound::Monitors(m) => {
                    self.monitors = m;
                    if self.mon_ix.is_some_and(|i| i >= self.monitors.len()) {
                        self.mon_ix = None;
                    }
                }
                Inbound::Control(allow) => {
                    self.view_only = Some(!allow);
                    if !allow {
                        self.status = "Máy đích chỉ cho xem — chuột và phím sẽ bị bỏ qua".into();
                        self.tone = Tone::Warn;
                    }
                }
                Inbound::Chat(chat_ev) => {
                    use uvsession::chat::HostEvent as S;
                    match chat_ev {
                        S::Msg(body) => {
                            self.chat_ctl.lines.push(crate::hostmode::ChatLine {
                                mine: false,
                                clock: crate::hostmode::hhmm(),
                                body,
                                dirs: Vec::new(),
                            });
                            self.chat_ctl.typing = false;
                            // New message -> open chat (like UltraViewer).
                            self.chat_open = true;
                            self.chat_dock = false;
                        }
                        S::Typing(g) => self.chat_ctl.typing = g,
                        S::Clipboard(_) => {} // already set this machine's clipboard on the session thread
                    }
                    ctx.request_repaint();
                }
                Inbound::HostName(name) => {
                    self.chat_ctl.name = name;
                    self.save_hist(true);
                }
                Inbound::Os(h) => {
                    self.peer_os = h;
                    self.save_hist(true);
                }
                Inbound::Closed(_) => {}
            }
        }

        if let Some(k) = newest {
            // A picture means the session is alive again: reset the retry counter.
            self.reconnect_tries = 0;
            let image = egui::ColorImage::from_rgb([k.w as usize, k.h as usize], &k.rgb);
            self.last_pic = Some((k.w as usize, k.h as usize, k.rgb.clone()));
            // A picture means connect succeeded -> write the id history once per session.
            self.save_hist(false);
            self.size = [k.w as usize, k.h as usize];
            self.n_frames += 1;
            self.n_bytes += k.byte as u64;
            self.total_frames += 1;
            let dt = self.rate_at.elapsed().as_secs_f32();
            if dt >= 1.0 {
                self.fps_now = self.n_frames as f32 / dt;
                self.kb_now = self.n_bytes as f32 / 1024.0 / dt;
                self.n_frames = 0;
                self.n_bytes = 0;
                self.rate_at = std::time::Instant::now();
            }
            match self.texture.as_mut() {
                Some(tex) => tex.set(image, egui::TextureOptions::LINEAR),
                None => {
                    self.texture =
                        Some(ctx.load_texture("screen", image, egui::TextureOptions::LINEAR));
                }
            }
        }

        if let Some(why) = closed {
            self.release_conn();
            self.texture = None;
            self.ping_wait = None;
            self.lag_ms = None;
            if self.engine.is_some() || self.uv.is_some() {
                // Keep the session screen and retry with growing delay, instead of
                // throwing the user back to the home screen.
                self.reconnect_tries += 1;
                let backoff = 500 * self.reconnect_tries.min(10) as u64;
                self.reconnect_at =
                    Some(std::time::Instant::now() + std::time::Duration::from_millis(backoff));
                self.status = format!(
                    "Mất kết nối ({why}) — thử lại sau {:.1} s (lần {})",
                    backoff as f32 / 1000.0,
                    self.reconnect_tries
                );
                self.tone = Tone::Warn;
            } else {
                self.view = View::Connect;
                self.resize_win(ctx, WIN_SIZE[0], WIN_SIZE[1]);
                self.return_focus(ctx);
                self.status = format!("Mất kết nối: {why}");
                self.tone = Tone::Err;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// widgets
// ---------------------------------------------------------------------------

// ---- One vertical column, in UltraViewer's visual language ----
// Pale blue menu bar, gradient title band + blue text, gray description,
// left label and right field on one axis, Start button with a blue arrow.
// Every block sits on an 8 px grid and shares column width `COL_W`.

/// Connect-screen window size (main.rs uses this too).
pub const WIN_SIZE: [f32; 2] = [COL_W + 32.0, 456.0];

/// Logical size of the session window. Resizing does not change this ratio.
pub const SESS_SIZE: [f32; 2] = [1000.0, 740.0];

/// Smallest and largest zoom relative to the design size, so the window fits the monitor.
fn zoom_bounds(design: egui::Vec2, zoom: f32, monitor: Option<egui::Vec2>) -> (f32, f32) {
    let fit = monitor
        .filter(|m| m.x > 32.0 && m.y > 32.0)
        .map(|m| {
            let fx = m.x * zoom * 0.94 / design.x;
            let fy = m.y * zoom * 0.94 / design.y;
            fx.min(fy)
        })
        .unwrap_or(8.0)
        .clamp(0.35, 8.0);
    let zoom_min = fit.min(1.0);
    let zoom_max = fit.max(zoom_min);
    (zoom_min, zoom_max)
}

/// Logical size to apply, and the new zoom.
///
/// `size` is the current size in points. The window locks to the shorter side
/// of the rectangle the user dragged, so the aspect ratio stays put. The zoom
/// brings the layout back to the design size on the next frame.
fn next_ratio(
    size: egui::Vec2,
    design: egui::Vec2,
    zoom: f32,
    monitor: Option<egui::Vec2>,
) -> (egui::Vec2, f32) {
    let zoom = zoom.max(0.05);
    let (zoom_min, zoom_max) = zoom_bounds(design, zoom, monitor);
    let s_raw = (size.x / design.x).min(size.y / design.y);
    let target = (zoom * s_raw).clamp(zoom_min, zoom_max);
    let s = target / zoom;
    (design * s, target)
}

/// Column fits the form: left margin 4 + label + gap + field + right margin 4.
pub const COL_W: f32 = 4.0 + FORM_W + 4.0;
const LBL_W: f32 = 70.0;
const GAP: f32 = 6.0;
/// Fits "100 000 001" (~95 px) + 9 px margin + room for the copy icon,
/// and fits the Start button label.
const BOX_W: f32 = 160.0;
/// Width of the label + field block; the block is left-aligned in the column.
const FORM_W: f32 = LBL_W + GAP + BOX_W;
const BOX_H: f32 = 30.0;
/// FIXED height of one description line (12 px font). Keeps layout from shifting
/// when the description or status text changes.
const HINT_H: f32 = 16.0;
/// Shared corner radius for the title band, fields, and buttons.
const RADIUS: f32 = 5.0;
/// Menu bar and status bar are the same height.
pub const BAR_H: f32 = 30.0;

const UV_BLUE: Color32 = Color32::from_rgb(0x1E, 0x6F, 0xC0);
const UV_INK: Color32 = Color32::from_rgb(0x1A, 0x1A, 0x1A);
const UV_LBL: Color32 = Color32::from_rgb(0x33, 0x37, 0x3E);
const UV_HINT: Color32 = Color32::from_rgb(0x50, 0x57, 0x63);
const UV_EDGE: Color32 = Color32::from_rgb(0xC5, 0xC9, 0xD2);
const UV_EDGE_HOVER: Color32 = Color32::from_rgb(0x8F, 0xB3, 0xDC);
const UV_EDGE_FOCUS: Color32 = Color32::from_rgb(0x2E, 0x7B, 0xC4);
const UV_RO: Color32 = Color32::from_rgb(0xEF, 0xF0, 0xF3);
const UV_MENU: Color32 = Color32::from_rgb(0xD3, 0xE2, 0xF4);
const UV_BAND_TOP: Color32 = Color32::from_rgb(0xF3, 0xF8, 0xFE);
const UV_BAND_BOT: Color32 = Color32::from_rgb(0xD9, 0xE8, 0xF8);
const UV_BAND_EDGE: Color32 = Color32::from_rgb(0xB4, 0xCE, 0xEC);
const UV_RULE: Color32 = Color32::from_rgb(0xDD, 0xE1, 0xE8);
const UV_BTN_TOP: Color32 = Color32::from_rgb(0xFF, 0xFF, 0xFF);
const UV_BTN_BOT: Color32 = Color32::from_rgb(0xE6, 0xE9, 0xEF);
const UV_BTN_EDGE: Color32 = Color32::from_rgb(0xA9, 0xB0, 0xBC);
const UV_OFF: Color32 = Color32::from_rgb(0x9A, 0xA0, 0xAB);

fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()))
}

/// Vertical gradient (top -> bottom) on a rect with corner radius `rad` px (convex, so a triangle fan from the center is enough).
fn vgrad(painter: &egui::Painter, r: egui::Rect, rad: f32, top_c: Color32, bot_c: Color32) {
    use std::f32::consts::PI;
    let rad = rad.min(r.width() / 2.0).min(r.height() / 2.0);
    let ang = [
        (egui::pos2(r.right() - rad, r.top() + rad), -PI / 2.0),
        (egui::pos2(r.right() - rad, r.bottom() - rad), 0.0),
        (egui::pos2(r.left() + rad, r.bottom() - rad), PI / 2.0),
        (egui::pos2(r.left() + rad, r.top() + rad), PI),
    ];
    let color = |p: egui::Pos2| mix(top_c, bot_c, ((p.y - r.top()) / r.height()).clamp(0.0, 1.0));
    let mut m = egui::Mesh::default();
    m.colored_vertex(r.center(), color(r.center()));
    for (ctr, a0) in ang {
        for i in 0..=6 {
            let a = a0 + (PI / 2.0) * i as f32 / 6.0;
            let p = ctr + egui::vec2(a.cos(), a.sin()) * rad;
            m.colored_vertex(p, color(p));
        }
    }
    let n = m.vertices.len() as u32 - 1;
    for i in 1..=n {
        m.add_triangle(0, i, if i == n { 1 } else { i + 1 });
    }
    painter.add(egui::Shape::mesh(m));
}

/// Arc from angle `a0` to `a1` (radians, 0 = right, clockwise).
fn arc_pts(ctr: egui::Pos2, r: f32, a0: f32, a1: f32) -> Vec<egui::Pos2> {
    (0..=12)
        .map(|i| {
            let a = a0 + (a1 - a0) * i as f32 / 12.0;
            ctr + egui::vec2(a.cos(), a.sin()) * r
        })
        .collect()
}

/// Single-line text, vertically centered in `r`, left pad `pad`.
fn text_mid(painter: &egui::Painter, r: egui::Rect, pad: f32, text: &str, sz: f32, color: Color32) {
    let g = painter.layout_no_wrap(text.to_owned(), egui::FontId::proportional(sz), color);
    let y = (r.center().y - g.size().y / 2.0).round();
    painter.galley(egui::pos2(r.left() + pad, y), g, color);
}

/// "Allow control" icon: a monitor sending a signal.
fn icon_share(painter: &egui::Painter, o: egui::Rect) {
    let stroke = egui::Stroke::new(1.6_f32, UV_BLUE);
    let m = egui::Rect::from_min_size(o.min + egui::vec2(1.0, 7.0), egui::vec2(14.0, 10.0));
    painter.rect_stroke(m, 1.5, stroke, egui::StrokeKind::Inside);
    let cx = m.center().x;
    painter.line_segment([egui::pos2(cx, m.bottom()), egui::pos2(cx, m.bottom() + 3.0)], stroke);
    painter.line_segment(
        [egui::pos2(cx - 4.0, m.bottom() + 3.0), egui::pos2(cx + 4.0, m.bottom() + 3.0)],
        stroke,
    );
    // Waves at the top-right corner.
    let ctr = egui::pos2(o.left() + 15.0, o.top() + 7.0);
    painter.circle_filled(ctr, 1.8, UV_BLUE);
    use std::f32::consts::PI;
    painter.add(egui::Shape::line(arc_pts(ctr, 4.5, -PI / 2.0, 0.0), stroke));
    painter.add(egui::Shape::line(arc_pts(ctr, 8.0, -PI / 2.0, 0.0), stroke));
}

/// "Control another computer" icon: a monitor with a mouse cursor.
fn icon_remote(painter: &egui::Painter, o: egui::Rect) {
    let stroke = egui::Stroke::new(1.6_f32, UV_BLUE);
    let m = egui::Rect::from_min_size(o.min + egui::vec2(1.0, 3.0), egui::vec2(20.0, 14.0));
    painter.rect_stroke(m, 1.5, stroke, egui::StrokeKind::Inside);
    let cx = m.center().x;
    painter.line_segment([egui::pos2(cx, m.bottom()), egui::pos2(cx, m.bottom() + 3.0)], stroke);
    painter.line_segment(
        [egui::pos2(cx - 5.0, m.bottom() + 3.0), egui::pos2(cx + 5.0, m.bottom() + 3.0)],
        stroke,
    );
    // Solid cursor, white outline so it shows on the monitor frame.
    let g = egui::pos2(m.left() + 9.0, m.top() + 3.5);
    let cursor = vec![
        g,
        g + egui::vec2(0.0, 11.0),
        g + egui::vec2(3.0, 8.0),
        g + egui::vec2(5.5, 12.5),
        g + egui::vec2(7.5, 11.5),
        g + egui::vec2(5.2, 7.2),
        g + egui::vec2(9.0, 7.2),
    ];
    painter.add(egui::Shape::convex_polygon(
        cursor,
        UV_BLUE,
        egui::Stroke::new(1.0_f32, Color32::WHITE),
    ));
}

/// Group title band: pale blue gradient, blue border, icon + blue text.
fn band_title(ui: &mut egui::Ui, r: egui::Rect, control: bool, text: &str) {
    let p = ui.painter();
    vgrad(p, r, RADIUS, UV_BAND_TOP, UV_BAND_BOT);
    p.rect_stroke(r, RADIUS, egui::Stroke::new(1.0_f32, UV_BAND_EDGE), egui::StrokeKind::Inside);
    let o = egui::Rect::from_min_size(
        egui::pos2(r.left() + 9.0, r.center().y - 11.0),
        egui::vec2(22.0, 22.0),
    );
    if control {
        icon_remote(p, o);
    } else {
        icon_share(p, o);
    }
    text_mid(p, r, 40.0, text, 15.0, UV_BLUE);
}

/// Gray description, wraps; returns the height used.
fn hint(ui: &mut egui::Ui, x: f32, y: f32, text: &str) -> f32 {
    let p = ui.painter();
    let g = p.layout(
        text.to_owned(),
        egui::FontId::proportional(12.0),
        UV_HINT,
        COL_W - 8.0,
    );
    let h = g.size().y;
    p.galley(egui::pos2(x + 4.0, y), g, UV_HINT);
    h
}

/// Collapse a string to one line (a wrap would shove the layout). Keep about 46 chars.
fn one_line(s: &str) -> String {
    const MAX: usize = 46;
    if s.chars().count() <= MAX {
        return s.to_string();
    }
    let mut r: String = s.chars().take(MAX - 1).collect();
    r.push('…');
    r
}

/// Left label of a row.
fn label_left(ui: &mut egui::Ui, row: egui::Rect, text: &str) {
    let r = egui::Rect::from_min_size(row.min, egui::vec2(LBL_W, row.height()));
    text_mid(ui.painter(), r, 0.0, text, 12.5, UV_LBL);
}

fn box_of(row: egui::Rect) -> egui::Rect {
    egui::Rect::from_min_size(
        egui::pos2(row.left() + LBL_W + GAP, row.top()),
        egui::vec2(BOX_W, row.height()),
    )
}

/// Read-only gray field (this machine's id / password).
fn ro_box(ui: &mut egui::Ui, o: egui::Rect, value: &str, sz: f32) {
    let p = ui.painter();
    p.rect_filled(o, RADIUS, UV_RO);
    p.rect_stroke(o, RADIUS, egui::Stroke::new(1.0_f32, UV_EDGE), egui::StrokeKind::Inside);
    text_mid(p, o, 9.0, value, sz, UV_INK);
}

/// Icon drawn at the start of a button.
#[derive(Clone, Copy)]
enum IconKind {
    Arrow,
    Copy,
    Key,
}

/// Draw a 14x14 icon centered on `c`; `bg` is the button fill there.
fn paint_icon(p: &egui::Painter, kind: IconKind, c: egui::Pos2, color: Color32, bg: Color32) {
    match kind {
        IconKind::Arrow => {
            p.rect_filled(
                egui::Rect::from_min_max(c + egui::vec2(-7.0, -2.0), c + egui::vec2(1.0, 2.0)),
                0.0,
                color,
            );
            p.add(egui::Shape::convex_polygon(
                vec![c + egui::vec2(0.0, -6.0), c + egui::vec2(7.0, 0.0), c + egui::vec2(0.0, 6.0)],
                color,
                egui::Stroke::NONE,
            ));
        }
        IconKind::Key => {
            // 3/4 arc + arrow (refresh / change-password icon).
            let stroke = egui::Stroke::new(1.4_f32, color);
            let ctr = c + egui::vec2(0.0, 0.5);
            let mut pts = Vec::new();
            let start_a = -0.5_f32;
            let sweep = std::f32::consts::TAU * 0.75;
            for i in 0..=30 {
                let ang = start_a + sweep * (i as f32 / 30.0);
                pts.push(ctr + egui::vec2(ang.cos(), ang.sin()) * 5.0);
            }
            let tip = *pts.last().unwrap();
            p.add(egui::Shape::line(pts, stroke));
            p.add(egui::Shape::convex_polygon(
                vec![
                    tip + egui::vec2(3.0, -1.5),
                    tip + egui::vec2(0.2, 1.8),
                    tip + egui::vec2(-1.0, -2.6),
                ],
                color,
                egui::Stroke::NONE,
            ));
        }
        IconKind::Copy => {
            // Two overlapping sheets.
            let stroke = egui::Stroke::new(1.4_f32, color);
            let back = egui::Rect::from_min_size(c + egui::vec2(-6.5, -7.0), egui::vec2(9.0, 11.0));
            let front = egui::Rect::from_min_size(c + egui::vec2(-2.5, -3.5), egui::vec2(9.0, 11.0));
            p.rect_stroke(back, 1.5, stroke, egui::StrokeKind::Inside);
            p.rect_filled(front, 1.5, bg);
            p.rect_stroke(front, 1.5, stroke, egui::StrokeKind::Inside);
        }
    }
}

/// White text field; border changes on hover / focus.
fn edit_box(ui: &mut egui::Ui, o: egui::Rect, text: &mut String, secret: bool, pad_right: f32) -> egui::Response {
    ui.painter().rect_filled(o, RADIUS, Color32::WHITE);
    // `pad_right`: width on the right that is NOT part of the text (e.g. the id-history arrow) — hovering it
    // does not show the caret, and clicking it does not focus the text.
    let text_r = egui::Rect::from_min_max(o.min, egui::pos2(o.max.x - pad_right, o.max.y));
    let edit = egui::TextEdit::singleline(text)
        .frame(false)
        .margin(egui::Margin::symmetric(9, 0))
        .vertical_align(egui::Align::Center)
        .font(egui::FontId::proportional(13.0))
        .password(secret);
    let resp = ui.put(text_r, edit);
    if resp.has_focus() {
        // Id / password fields do not need Vietnamese: turn the IME (fcitx5) off while they are focused.
        // With Unikey on, text sits in preedit (highlighted); Tab to the next field drops the
        // preedit — the id is left as "1" and the field sticks (user report 22:50, reproduced
        // the highlight). Clearing the frame's `ime` makes egui-winit call set_ime_allowed(false).
        ui.ctx().output_mut(|out| out.ime = None);
    }
    let edge = if resp.has_focus() {
        UV_EDGE_FOCUS
    } else if resp.hovered() {
        UV_EDGE_HOVER
    } else {
        UV_EDGE
    };
    ui.painter()
        .rect_stroke(o, RADIUS, egui::Stroke::new(1.0_f32, edge), egui::StrokeKind::Inside);
    resp
}

/// "Copy" icon on the right inside a field; returns `true` when clicked.
fn copy_btn(ui: &mut egui::Ui, o: egui::Rect, id: &str, tip: &str) -> bool {
    let r = egui::Rect::from_center_size(
        egui::pos2(o.right() - 39.0, o.center().y),
        egui::vec2(22.0, 22.0),
    );
    let resp = ui
        .interact(r, ui.id().with(id), egui::Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(tip);
    let p = ui.painter();
    let bg = if resp.hovered() { Color32::from_rgb(0xDC, 0xE8, 0xF6) } else { UV_RO };
    if resp.hovered() {
        p.rect_filled(r, 4.0, bg);
    }
    let color = if resp.hovered() { UV_BLUE } else { Color32::from_rgb(0x4A, 0x86, 0xC8) };
    paint_icon(p, IconKind::Copy, r.center(), color, bg);
    resp.clicked()
}

/// "Change password" button (key icon) next to the copy button in the password field.
fn pw_btn(ui: &mut egui::Ui, o: egui::Rect, id: &str) -> bool {
    let r = egui::Rect::from_center_size(
        egui::pos2(o.right() - 15.0, o.center().y),
        egui::vec2(22.0, 22.0),
    );
    let resp = ui
        .interact(r, ui.id().with(id), egui::Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text("Đổi mật khẩu");
    let p = ui.painter();
    let bg = if resp.hovered() { Color32::from_rgb(0xDC, 0xE8, 0xF6) } else { UV_RO };
    if resp.hovered() {
        p.rect_filled(r, 4.0, bg);
    }
    let color = if resp.hovered() { UV_BLUE } else { Color32::from_rgb(0x4A, 0x86, 0xC8) };
    paint_icon(p, IconKind::Key, r.center(), color, bg);
    resp.clicked()
}

/// UltraViewer-style button: white -> gray gradient, blue icon + text,
/// the cluster centered in the button.
fn btn(
    ui: &mut egui::Ui,
    r: egui::Rect,
    id: &str,
    kind: IconKind,
    label: &str,
    on: bool,
) -> egui::Response {
    let resp = ui.interact(r, ui.id().with(id), egui::Sense::click());
    let resp = if on { resp.on_hover_cursor(egui::CursorIcon::PointingHand) } else { resp };
    let p = ui.painter();
    let (top_c, bot_c) = if !on {
        (Color32::from_rgb(0xF7, 0xF8, 0xFA), Color32::from_rgb(0xF1, 0xF2, 0xF5))
    } else if resp.is_pointer_button_down_on() {
        (Color32::from_rgb(0xD9, 0xE6, 0xF5), Color32::from_rgb(0xE9, 0xF0, 0xF9))
    } else if resp.hovered() {
        (Color32::from_rgb(0xF5, 0xF9, 0xFE), Color32::from_rgb(0xDB, 0xE8, 0xF7))
    } else {
        (UV_BTN_TOP, UV_BTN_BOT)
    };
    vgrad(p, r, RADIUS, top_c, bot_c);
    let edge = if on && resp.hovered() { UV_EDGE_HOVER } else if on { UV_BTN_EDGE } else { UV_EDGE };
    p.rect_stroke(r, RADIUS, egui::Stroke::new(1.0_f32, edge), egui::StrokeKind::Inside);

    // Icon (14 px) + 9 px gap + text: the cluster is centered in the button.
    let color = if on { UV_BLUE } else { UV_OFF };
    let ink = if on { UV_INK } else { UV_OFF };
    let g = p.layout_no_wrap(label.to_owned(), egui::FontId::proportional(13.0), ink);
    let cluster = 14.0 + 9.0 + g.size().x;
    let x0 = (r.center().x - cluster / 2.0).round();
    let c = egui::pos2(x0 + 7.0, r.center().y);
    paint_icon(p, kind, c, color, mix(top_c, bot_c, 0.6));
    let y = (r.center().y - g.size().y / 2.0).round();
    p.galley(egui::pos2(x0 + 23.0, y), g, ink);
    resp
}

/// Text for "Your ID". A saved id is shown at once; otherwise "registering"
/// on a machine that can host, and "not supported" on one that cannot.
fn host_id_label(id: Option<&str>, supported: bool) -> String {
    match id.map(str::trim).filter(|s| !s.is_empty()) {
        Some(id) => group_id(id),
        None if !supported => "— chưa hỗ trợ —".into(),
        None => "— đang đăng ký —".into(),
    }
}

/// "100000001" -> "100 000 001", grouped the way UltraViewer shows an id.
fn group_id(id: &str) -> String {
    let digits: Vec<char> = id.chars().filter(|c| !c.is_whitespace()).collect();
    let mut out = String::new();
    for (i, c) in digits.iter().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(' ');
        }
        out.push(*c);
    }
    out
}

fn dot(ui: &mut egui::Ui, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
    ui.painter().circle_filled(rect.center(), 4.5, color);
}

// ---------------------------------------------------------------------------

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.handle_screenshot(ctx);

        if self.pending_connect {
            self.pending_connect = false;
            self.connect(ctx);
        }
        self.pump(ctx);
        self.ping_tick();
        if self.is_agent() {
            // Ping needs a steady frame even when the picture is still.
            ctx.request_repaint_after(std::time::Duration::from_millis(500));
        }

        if self.conn.is_none() {
            if let Some(at) = self.reconnect_at {
                if std::time::Instant::now() >= at {
                    self.reconnect_at = None;
                    self.reconnect(ctx);
                }
            }
        }

        egui::TopBottomPanel::bottom("status")
            .exact_height(BAR_H)
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_rgb(0xF4, 0xF5, 0xF7))
                    .inner_margin(egui::Margin::symmetric(12, 0)),
            )
            .show(ctx, |ui| {
                // Place the meters from the right first; the status line takes what is left and
                // ellipsizes, so the two texts never overlap.
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if self.texture.is_some() {
                        {
                            let mut stats = format!(
                                "{} khung · {:.0} hình/giây · {:.0} KB/giây",
                                self.total_frames, self.fps_now, self.kb_now
                            );
                            if let Some(ms) = self.lag_ms {
                                stats.push_str(&format!(" · trễ {ms:.0} ms"));
                            }
                            if self.view_only == Some(true) {
                                stats.push_str(" · chỉ xem");
                            }
                            ui.label(
                                RichText::new(stats)
                                    .size(11.0)
                                    .color(Color32::from_rgb(0x5A, 0x63, 0x70)),
                            );
                        }
                        ui.add_space(12.0);
                    }
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        // Session is up but chat was closed (x) -> bubble button reopens it.
                        if !self.chat_open && self.had_sess {
                            let (r, resp) = ui.allocate_exact_size(egui::vec2(24.0, 20.0), egui::Sense::click());
                            if resp.hovered() {
                                ui.painter().rect_filled(r, 4.0, chat_color::HOVER);
                            }
                            chat_icon::bubble(ui.painter(), r.shrink2(egui::vec2(5.0, 4.0)), chat_color::ACCENT);
                            if resp.on_hover_text("Mở khung chat").clicked() {
                                self.chat_open = true;
                            }
                        }
                        // One status line, changes with the situation (like UltraViewer):
                        //   Ready -> Connecting... -> Being controlled -> Connection lost.
                        let (tt, tone) = {
                            let host = self.host.lock().unwrap();
                            if host.serving {
                                ("Đang bị điều khiển".to_string(), Tone::Ok)
                            } else if host.status.contains("Handshak") || host.status.starts_with("Server is not issuing") {
                                (host.status.clone(), Tone::Warn)
                            } else if host.status.contains("failed") || host.status.contains("Could not") {
                                (host.status.clone(), Tone::Err)
                            } else if let Some(c) = &host.alert {
                                (c.clone(), Tone::Warn)
                            } else {
                                (self.status.clone(), self.tone)
                            }
                        };
                        let color = match tone {
                            Tone::Ok => GREEN,
                            Tone::Warn => AMBER,
                            Tone::Err => RED,
                        };
                        dot(ui, color);
                        ui.add(
                            egui::Label::new(RichText::new(&tt).size(12.0).color(TEXT))
                                .truncate(),
                        )
                        .on_hover_text(&tt);
                    });
                });
            });

        match self.view {
            View::Connect => self.connect_view(ctx),
            View::Session => self.session_view(ctx),
        }
        self.chat_panel(ctx);
        self.lock_ratio(ctx);
    }
}

/// Chat palette (light background; dark text on light, white text only on deep blue).
mod chat_color {
    use eframe::egui::Color32;
    /// Accent blue (icons, send button). About 6.6:1 on white.
    pub const ACCENT: Color32 = Color32::from_rgb(0x1F, 0x5F, 0xA8);
    pub const BG: Color32 = Color32::from_rgb(0xF3, 0xF5, 0xF8);
    pub const CARD: Color32 = Color32::WHITE;
    pub const EDGE: Color32 = Color32::from_rgb(0xDD, 0xE2, 0xE9);
    pub const MUTED: Color32 = Color32::from_rgb(0x5A, 0x63, 0x70);
    pub const LBL: Color32 = Color32::from_rgb(0x6B, 0x74, 0x80);
    /// Bubble fill for the peer / for me.
    pub const BUBBLE_PEER: Color32 = Color32::from_rgb(0xF0, 0xF3, 0xF7);
    pub const BUBBLE_MINE: Color32 = Color32::from_rgb(0xE1, 0xEE, 0xFB);
    pub const NAME_PEER: Color32 = Color32::from_rgb(0x1F, 0x5F, 0xA8);
    pub const NAME_MINE: Color32 = Color32::from_rgb(0x4A, 0x55, 0x63);
    pub const OK_GREEN: Color32 = Color32::from_rgb(0x16, 0xA3, 0x4A);
    pub const GRAY: Color32 = Color32::from_rgb(0xA0, 0xA7, 0xB1);
    /// Outer border of the chat panel / collapse button (no system window frame).
    pub const EDGE_STRONG: Color32 = Color32::from_rgb(0xB8, 0xC0, 0xCC);
    /// Fill when hovering a flat button.
    pub const HOVER: Color32 = Color32::from_rgb(0xEA, 0xF1, 0xFA);
}

/// Hand-drawn chat icons (Be Vietnam Pro has no emoji glyphs — measured
/// at 22:00 they rendered as empty boxes).
mod chat_icon {
    use eframe::egui::{self, pos2, vec2, Color32, Painter, Pos2, Rect, Stroke};

    /// Chevron: `right = true` draws a right chevron, otherwise a left one.
    pub fn chevron(p: &Painter, ctr: Pos2, sz: f32, right: bool, color: Color32) {
        let d = if right { 1.0 } else { -1.0 };
        let w = sz * 0.32;
        let h = sz * 0.55;
        let pts = vec![pos2(ctr.x - d * w, ctr.y - h), pos2(ctr.x + d * w, ctr.y), pos2(ctr.x - d * w, ctr.y + h)];
        p.add(egui::Shape::line(pts, Stroke::new(2.4_f32, color)));
    }

    /// Plus sign.
    pub fn plus_mark(p: &Painter, ctr: Pos2, sz: f32, color: Color32) {
        let s = Stroke::new(2.2_f32, color);
        p.line_segment([pos2(ctr.x - sz, ctr.y), pos2(ctr.x + sz, ctr.y)], s);
        p.line_segment([pos2(ctr.x, ctr.y - sz), pos2(ctr.x, ctr.y + sz)], s);
    }

    /// Flat paper plane (send button): two triangles sharing an edge, nose to the right.
    pub fn paper_plane(p: &Painter, r: Rect, color: Color32) {
        let (l, t, rr, b) = (r.left(), r.top(), r.right(), r.bottom());
        let mid = r.center().y;
        let fold = pos2(l + r.width() * 0.34, mid + r.height() * 0.04);
        let nose = pos2(rr, t + r.height() * 0.08);
        // Upper and lower wings; the fold is the line from `fold` to the nose.
        p.add(egui::Shape::convex_polygon(vec![pos2(l, mid - r.height() * 0.05), nose, fold], color, Stroke::NONE));
        p.add(egui::Shape::convex_polygon(
            vec![fold, nose, pos2(l + r.width() * 0.46, b)],
            color.gamma_multiply(0.78),
            Stroke::NONE,
        ));
    }

    /// File (page with a folded corner) or image (frame with mountains + sun).
    pub fn file_icon(p: &Painter, r: Rect, is_image: bool, color: Color32) {
        let s = Stroke::new(1.6_f32, color);
        if is_image {
            p.rect_stroke(r, 3.0, s, egui::StrokeKind::Inside);
            p.circle_filled(pos2(r.left() + r.width() * 0.32, r.top() + r.height() * 0.32), r.width() * 0.09, color);
            p.add(egui::Shape::convex_polygon(
                vec![
                    pos2(r.left() + 3.0, r.bottom() - 3.0),
                    pos2(r.left() + r.width() * 0.42, r.top() + r.height() * 0.52),
                    pos2(r.right() - 3.0, r.bottom() - 3.0),
                ],
                color,
                Stroke::NONE,
            ));
        } else {
            let g = r.width() * 0.34;
            let pts = vec![
                r.left_top(),
                pos2(r.right() - g, r.top()),
                pos2(r.right(), r.top() + g),
                r.right_bottom(),
                r.left_bottom(),
                r.left_top(),
            ];
            p.add(egui::Shape::line(pts, s));
            p.add(egui::Shape::line(
                vec![pos2(r.right() - g, r.top()), pos2(r.right() - g, r.top() + g), pos2(r.right(), r.top() + g)],
                s,
            ));
            for i in 0..3 {
                let y = r.top() + r.height() * (0.5 + 0.15 * i as f32);
                p.line_segment([pos2(r.left() + 3.0, y), pos2(r.right() - 3.0, y)], Stroke::new(1.2_f32, color));
            }
        }
    }

    /// Vertical chevron: `down = true` points down, otherwise up.
    pub fn chevron_v(p: &Painter, ctr: Pos2, sz: f32, down: bool, color: Color32) {
        let d = if down { 1.0 } else { -1.0 };
        let pts = vec![
            pos2(ctr.x - sz, ctr.y - d * sz * 0.5),
            pos2(ctr.x, ctr.y + d * sz * 0.5),
            pos2(ctr.x + sz, ctr.y - d * sz * 0.5),
        ];
        p.add(egui::Shape::line(pts, Stroke::new(1.8_f32, color)));
    }

    /// "Actions": white bolt on an orange circle (like UltraViewer).
    pub fn bolt(p: &Painter, r: Rect) {
        p.circle_filled(r.center(), r.width() * 0.5, Color32::from_rgb(0xF5, 0x9E, 0x0B));
        let c = r.center();
        let k = r.width() * 0.5;
        p.add(egui::Shape::convex_polygon(
            vec![pos2(c.x + k * 0.18, c.y - k * 0.62), pos2(c.x - k * 0.34, c.y + k * 0.08), pos2(c.x + k * 0.02, c.y + k * 0.08)],
            Color32::WHITE,
            Stroke::NONE,
        ));
        p.add(egui::Shape::convex_polygon(
            vec![pos2(c.x - k * 0.02, c.y - k * 0.08), pos2(c.x + k * 0.34, c.y - k * 0.08), pos2(c.x - k * 0.18, c.y + k * 0.62)],
            Color32::WHITE,
            Stroke::NONE,
        ));
    }

    /// "Settings": a gear.
    pub fn gear(p: &Painter, r: Rect, color: Color32) {
        let c = r.center();
        let rr = r.width() * 0.32;
        for i in 0..8 {
            let g = i as f32 * std::f32::consts::TAU / 8.0;
            let (sn, cs) = g.sin_cos();
            p.line_segment(
                [pos2(c.x + cs * rr, c.y + sn * rr), pos2(c.x + cs * rr * 1.5, c.y + sn * rr * 1.5)],
                Stroke::new(2.6_f32, color),
            );
        }
        p.circle_stroke(c, rr, Stroke::new(2.0_f32, color));
        p.circle_stroke(c, rr * 0.42, Stroke::new(1.4_f32, color));
    }

    /// "Screenshot": a camera.
    pub fn camera(p: &Painter, r: Rect, color: Color32) {
        let body = Rect::from_min_max(pos2(r.left() + 1.0, r.top() + r.height() * 0.3), pos2(r.right() - 1.0, r.bottom() - 2.0));
        p.rect_filled(body, 3.0, color);
        p.rect_filled(
            Rect::from_min_max(pos2(r.left() + r.width() * 0.32, r.top() + r.height() * 0.14), pos2(r.left() + r.width() * 0.66, body.top() + 1.0)),
            1.5,
            color,
        );
        p.circle_filled(body.center(), body.height() * 0.32, Color32::WHITE);
        p.circle_filled(body.center(), body.height() * 0.18, color);
    }

    /// OS icon for the id list: Windows (four blue tiles), Linux (penguin),
    /// macOS (apple). Unknown -> a generic monitor.
    pub fn os_badge(p: &Painter, r: Rect, os: &str) {
        match os {
            "windows" => {
                let blue = Color32::from_rgb(0x00, 0x78, 0xD4);
                let k = 1.2;
                let (cx, cy) = (r.center().x, r.center().y);
                for (x0, y0, x1, y1) in [
                    (r.left() + 1.0, r.top() + 1.0, cx - k, cy - k),
                    (cx + k, r.top() + 1.0, r.right() - 1.0, cy - k),
                    (r.left() + 1.0, cy + k, cx - k, r.bottom() - 1.0),
                    (cx + k, cy + k, r.right() - 1.0, r.bottom() - 1.0),
                ] {
                    p.rect_filled(Rect::from_min_max(pos2(x0, y0), pos2(x1, y1)), 1.0, blue);
                }
            }
            "linux" => {
                let ink = Color32::from_rgb(0x22, 0x25, 0x2A);
                let orange = Color32::from_rgb(0xF5, 0xA6, 0x23);
                let c = r.center();
                let w = r.width();
                // Body + white belly + eyes + beak + feet.
                p.add(egui::Shape::ellipse_filled(pos2(c.x, c.y + w * 0.06), vec2(w * 0.34, w * 0.46), ink));
                p.add(egui::Shape::ellipse_filled(pos2(c.x, c.y + w * 0.16), vec2(w * 0.22, w * 0.3), Color32::WHITE));
                for dx in [-0.1, 0.1] {
                    p.circle_filled(pos2(c.x + w * dx, c.y - w * 0.2), w * 0.065, Color32::WHITE);
                    p.circle_filled(pos2(c.x + w * dx, c.y - w * 0.19), w * 0.03, ink);
                }
                p.add(egui::Shape::convex_polygon(
                    vec![pos2(c.x - w * 0.09, c.y - w * 0.1), pos2(c.x + w * 0.09, c.y - w * 0.1), pos2(c.x, c.y - w * 0.01)],
                    orange,
                    Stroke::NONE,
                ));
                for dx in [-0.16, 0.16] {
                    p.add(egui::Shape::ellipse_filled(pos2(c.x + w * dx, c.y + w * 0.47), vec2(w * 0.13, w * 0.06), orange));
                }
            }
            "macos" => {
                let gray = Color32::from_rgb(0x55, 0x5B, 0x63);
                let c = r.center();
                let w = r.width();
                p.circle_filled(pos2(c.x - w * 0.12, c.y + w * 0.08), w * 0.27, gray);
                p.circle_filled(pos2(c.x + w * 0.12, c.y + w * 0.08), w * 0.27, gray);
                p.circle_filled(pos2(c.x, c.y + w * 0.2), w * 0.24, gray);
                // Bite on the right + leaf.
                p.circle_filled(pos2(c.x + w * 0.42, c.y + w * 0.02), w * 0.14, Color32::WHITE);
                p.add(egui::Shape::ellipse_filled(pos2(c.x + w * 0.05, c.y - w * 0.34), vec2(w * 0.08, w * 0.15), gray));
            }
            _ => {
                let gray = Color32::from_rgb(0x6B, 0x74, 0x80);
                let screen = Rect::from_min_max(pos2(r.left() + 1.0, r.top() + 2.0), pos2(r.right() - 1.0, r.bottom() - 6.0));
                p.rect_filled(screen, 2.0, gray);
                p.rect_filled(screen.shrink(2.0), 1.0, Color32::from_rgb(0xDD, 0xE6, 0xF2));
                p.rect_filled(Rect::from_min_max(pos2(r.center().x - 4.0, r.bottom() - 4.0), pos2(r.center().x + 4.0, r.bottom() - 2.0)), 1.0, gray);
            }
        }
    }

    /// Chat bubble (button that reopens chat).
    pub fn bubble(p: &Painter, r: Rect, color: Color32) {
        let body = Rect::from_min_max(r.left_top(), pos2(r.right(), r.bottom() - r.height() * 0.25));
        p.rect_filled(body, 3.0, color);
        p.add(egui::Shape::convex_polygon(
            vec![
                pos2(body.left() + body.width() * 0.22, body.bottom() - 0.5),
                pos2(body.left() + body.width() * 0.48, body.bottom() - 0.5),
                pos2(body.left() + body.width() * 0.2, r.bottom()),
            ],
            color,
            Stroke::NONE,
        ));
    }

    /// Folder (opened from a link).
    pub fn folder(p: &Painter, r: Rect, color: Color32) {
        let tab = Rect::from_min_size(r.left_top(), vec2(r.width() * 0.42, r.height() * 0.22));
        p.rect_filled(tab, 2.0, color);
        p.rect_filled(Rect::from_min_max(pos2(r.left(), r.top() + r.height() * 0.16), r.right_bottom()), 2.0, color);
    }
}

impl App {
    /// Write the current UltraViewer session into id history. `refresh = true`: write again only when
    /// the session was already stored (name/OS arrive after the first frame).
    fn save_hist(&mut self, refresh: bool) {
        if self.uv.is_none() || self.peer_id.trim().is_empty() {
            return;
        }
        if refresh != self.hist_wrote {
            return;
        }
        self.hist = crate::history::store(&self.peer_id, &self.chat_ctl.name, &self.peer_os);
        self.hist_wrote = true;
    }

    /// Suggestion list of ids we connected to, under the partner-id field like UltraViewer:
    /// OS icon, id, relative time, machine name. Filtered by the digits being typed; click a row to fill the id.
    fn id_popup(&mut self, ui: &mut egui::Ui, o: &egui::Response, full_box: egui::Rect) {
        use chat_color as m;
        // Dropdown arrow: its own 26 px strip on the right, outside the text (`edit_box(.., 26.0)`). `Sense::CLICK`
        // without FOCUSABLE -> Tab goes ID -> Password (Tab used to stop on the arrow and
        // the suggestion rows, measured 08:24).
        let arrow_r = egui::Rect::from_min_max(egui::pos2(full_box.right() - 26.0, full_box.top()), full_box.right_bottom());
        let arrow = ui.interact(arrow_r, ui.id().with("mo-goi-y-id"), egui::Sense::CLICK);
        if arrow.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        chat_icon::chevron_v(ui.painter(), arrow_r.center(), 4.0, true, if arrow.hovered() || self.suggest_on { m::ACCENT } else { m::MUTED });
        if arrow.clicked() {
            self.suggest_on = !self.suggest_on;
        }
        if o.clicked() || o.gained_focus() {
            self.suggest_on = true;
        }
        let hits = crate::history::match_id(&self.hist, &self.peer_id);
        if !self.suggest_on || hits.is_empty() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                self.suggest_on = false;
            }
            return;
        }
        // Align both edges with the id field.
        let width = full_box.width();
        let x_left = full_box.left();
        let mut picked: Option<String> = None;
        // Use only the space under the id field (the main window is short): extra rows scroll. Do not
        // let egui move the area up — 4 rows used to cover the id field (measured 00:30).
        let below = ui.ctx().screen_rect().bottom() - full_box.bottom() - 12.0;
        let scroll_h = (42.0 * 5.0_f32).min(below.max(42.0));
        let popup = egui::Area::new(ui.id().with("goi-y-id"))
            .fixed_pos(egui::pos2(x_left, full_box.bottom() + 2.0))
            .constrain(false)
            .order(egui::Order::Foreground)
            .show(ui.ctx(), |ui| {
                egui::Frame::new()
                    .fill(m::CARD)
                    .stroke(egui::Stroke::new(1.0_f32, m::EDGE_STRONG))
                    .corner_radius(egui::CornerRadius::same(6))
                    .inner_margin(egui::Margin::same(4))
                    .show(ui, |ui| {
                        ui.set_width(width - 8.0);
                        egui::ScrollArea::vertical().max_height(scroll_h).show(ui, |ui| {
                            // Rows sit flush: a gap used to swallow clicks.
                            ui.spacing_mut().item_spacing.y = 0.0;
                            for item in &hits {
                                let (r, resp) = ui.allocate_exact_size(egui::vec2(width - 8.0, 40.0), egui::Sense::CLICK);
                                if resp.hovered() {
                                    ui.painter().rect_filled(r, 4.0, m::HOVER);
                                }
                                let p = ui.painter();
                                chat_icon::os_badge(
                                    p,
                                    egui::Rect::from_center_size(egui::pos2(r.left() + 16.0, r.center().y), egui::vec2(20.0, 20.0)),
                                    &item.os,
                                );
                                let x = r.left() + 32.0;
                                p.text(
                                    egui::pos2(x, r.top() + 12.0),
                                    egui::Align2::LEFT_CENTER,
                                    group_id(&item.id),
                                    egui::FontId::proportional(13.0),
                                    TEXT,
                                );
                                // The list is only as wide as the id field: time is the short form, full text on hover.
                                let when_full = crate::history::ago_now(item.at);
                                let when = p.text(
                                    egui::pos2(r.right() - 4.0, r.top() + 29.0),
                                    egui::Align2::RIGHT_CENTER,
                                    crate::history::short_ago(&when_full),
                                    egui::FontId::proportional(10.5),
                                    m::GRAY,
                                );
                                // Machine name is ellipsized so it does not cover the time.
                                let name = if item.name.is_empty() { "—".to_string() } else { item.name.clone() };
                                let max_px = (when.left() - 6.0 - x).max(20.0);
                                let name = fit_text(ui, &name, 11.5, max_px);
                                ui.painter().text(
                                    egui::pos2(x, r.top() + 29.0),
                                    egui::Align2::LEFT_CENTER,
                                    name,
                                    egui::FontId::proportional(11.5),
                                    m::MUTED,
                                );
                                let tip = format!(
                                    "{} · {} · {when_full}",
                                    match item.os.as_str() {
                                        "windows" => "Windows",
                                        "linux" => "Linux",
                                        "macos" => "macOS",
                                        _ => "Chưa rõ hệ điều hành",
                                    },
                                    if item.name.is_empty() { "—" } else { item.name.as_str() }
                                );
                                if resp.on_hover_text(tip).clicked() {
                                    picked = Some(item.id.clone());
                                }
                            }
                        });
                    });
            });
        if let Some(id) = picked {
            self.peer_id = group_id(&id);
            self.suggest_on = false;
        } else {
            // Click outside the list and outside the id field -> close.
            let clicked_out = ui.input(|i| {
                i.pointer.any_pressed()
                    && i.pointer.interact_pos().is_some_and(|p| !popup.response.rect.contains(p) && !full_box.contains(p))
            });
            if clicked_out {
                self.suggest_on = false;
            }
        }
    }

    /// Tell the other side "typing" / "stopped" in the direction of this session.
    fn set_typing(&mut self, as_ctrl: bool, typing: bool) {
        if as_ctrl {
            self.send(Ctl { t: "chat_go".into(), down: typing, ..Default::default() });
        } else {
            crate::hostmode::set_typing(&self.host, typing);
        }
    }
}

/// Radio row in a menu with a visible border. egui's menu (`set_menu_style`) drops `bg_stroke`, so the
/// radio is only a pale gray fill, almost invisible (user report 08:24: last row of the Settings menu looked offset).
fn menu_radio(ui: &mut egui::Ui, picked: bool, text: impl Into<egui::WidgetText>) -> egui::Response {
    ui.scope(|ui| {
        menu_box(ui);
        ui.radio(picked, text)
    })
    .inner
}

/// Checkbox in a menu with a visible border (see [`menu_radio`]). Read-only; the caller toggles on click.
fn menu_check(ui: &mut egui::Ui, on: bool, text: impl Into<egui::WidgetText>) -> egui::Response {
    ui.scope(|ui| {
        menu_box(ui);
        let mut b = on;
        ui.checkbox(&mut b, text)
    })
    .inner
}

fn menu_box(ui: &mut egui::Ui) {
    let v = &mut ui.visuals_mut().widgets;
    let stroke = egui::Stroke::new(1.0_f32, Color32::from_rgb(0x8A, 0x93, 0x9E));
    v.inactive.bg_stroke = stroke;
    v.inactive.bg_fill = Color32::WHITE;
    v.hovered.bg_stroke = egui::Stroke::new(1.0_f32, chat_color::ACCENT);
    v.hovered.bg_fill = Color32::WHITE;
    v.active.bg_stroke = egui::Stroke::new(1.0_f32, chat_color::ACCENT);
    // Theme corner radius 5 px on every widget made a 14 px checkbox nearly round and easy to confuse with a radio (measured 08:50).
    let rad = egui::CornerRadius::same(2);
    v.inactive.corner_radius = rad;
    v.hovered.corner_radius = rad;
    v.active.corner_radius = rad;
}

/// See [`App::clean_id`].
fn norm_id(id: &str) -> String {
    let joined: String = id.chars().filter(|c| !c.is_whitespace()).collect();
    if !joined.is_empty() && joined.chars().all(|c| c.is_ascii_digit()) {
        joined
    } else {
        id.trim().to_string()
    }
}

/// Ellipsize `s` to fit `max_px` at font size `sz`.
fn fit_text(ui: &egui::Ui, s: &str, sz: f32, max_px: f32) -> String {
    let width = |t: &str| ui.fonts(|f| f.layout_no_wrap(t.to_string(), egui::FontId::proportional(sz), TEXT).size().x);
    if width(s) <= max_px {
        return s.to_string();
    }
    let mut v: Vec<char> = s.chars().collect();
    while !v.is_empty() {
        v.pop();
        let t: String = v.iter().collect::<String>() + "…";
        if width(&t) <= max_px {
            return t;
        }
    }
    "…".into()
}

/// Directory for peer screenshots: `~/Pictures/X-Remote` (created if missing).
fn shot_dir() -> Result<std::path::PathBuf, String> {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).ok_or("không biết thư mục nhà")?;
    let d = std::path::PathBuf::from(home).join("Pictures").join("X-Remote");
    std::fs::create_dir_all(&d).map_err(|e| format!("không tạo được {}: {e}", d.display()))?;
    Ok(d)
}

/// X11 window type for the chat panel and the collapse button: Utility (hidden from Alt-Tab). The xfwm
/// shadow does NOT follow the type (measured normal/utility/popup/dropdown/notification) — see
/// [`crate::shadow`].
const CHAT_X11: egui::viewport::X11WindowType = egui::viewport::X11WindowType::Utility;

/// Close the chat panel and the collapse button. Called when a session ends (Linux).
fn close_chat_viewports(ctx: &egui::Context) {
    for id in ["x-remote-chat", "x-remote-chat-nut"] {
        ctx.send_viewport_cmd_to(
            egui::ViewportId::from_hash_of(id),
            egui::ViewportCommand::Close,
        );
    }
}

/// macOS: keep the chat window but hide it. Destroying NSWindow inside this
/// frame stalls the event loop — after the session ends the id and password
/// fields no longer accept keys.
fn hide_chat_viewports(ctx: &egui::Context) {
    for id in ["x-remote-chat-nut", "x-remote-chat"] {
        let id = egui::ViewportId::from_hash_of(id);
        let b = egui::ViewportBuilder::default()
            .with_title("Chat")
            .with_visible(false)
            .with_decorations(false)
            .with_inner_size([1.0, 1.0]);
        ctx.show_viewport_immediate(id, b, |ctx, _| {
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        });
    }
}

impl App {
    /// Chat with the controller — layout follows UltraViewer chat (peer, history,
    /// input), docked on the RIGHT, with a square collapse button outside the left
    /// of the title bar. Opens itself when a message arrives.
    fn chat_panel(&mut self, ctx: &egui::Context) {
        use chat_color as m;
        crate::hostmode::set_ctx(ctx);
        // Controlling another machine (UltraViewer session) -> that session's chat; otherwise the
        // host-mode chat. Only the latter used to exist, so when Linux controlled
        // Windows the chat showed nobody / disconnected and could not send (22:58).
        let as_ctrl = self.view == View::Session && self.conn.is_some() && self.uv.is_some();
        let (lines, name, in_sess, typing) = if as_ctrl {
            let name = if self.chat_ctl.name.is_empty() { group_id(&self.clean_id()) } else { self.chat_ctl.name.clone() };
            (self.chat_ctl.lines.clone(), name, true, self.chat_ctl.typing)
        } else {
            let mut g = self.host.lock().unwrap();
            if g.chat_unread {
                g.chat_unread = false;
                self.chat_open = true;
                // New message while collapsed -> open the panel (like UltraViewer).
                self.chat_dock = false;
            }
            (g.chat.clone(), g.peer_name.clone(), g.serving, g.peer_typing)
        };
        // Chat exists only while a session does (being controlled or controlling): session start ->
        // open; session end -> hide (user request 08:50).
        if in_sess != self.had_sess {
            let ended = self.had_sess && !in_sess;
            self.had_sess = in_sess;
            self.chat_open = in_sess;
            if in_sess {
                self.chat_dock = false;
            }
            // Linux: close the chat window. macOS must not destroy NSWindow in
            // this frame (it stalls the event loop) — hide it in the branch below.
            if ended && !cfg!(target_os = "macos") {
                close_chat_viewports(ctx);
            }
        }
        if !self.chat_open {
            if cfg!(target_os = "macos") && self.chat_made {
                hide_chat_viewports(ctx);
            }
            return;
        }
        self.chat_made = true;
        // A folder just picked (background picker) -> insert its name into the draft as a link.
        for p in std::mem::take(&mut *self.dir_q.lock().unwrap()) {
            let dir_name = p.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| p.display().to_string());
            let label = format!("[{dir_name}]");
            if !self.draft.is_empty() && !self.draft.ends_with(' ') {
                self.draft.push(' ');
            }
            self.draft.push_str(&label);
            self.draft.push(' ');
            self.link_q.push((label, p));
            self.edit_at = std::time::Instant::now();
        }
        let show_name = if name.is_empty() { "Đối tác".to_string() } else { name.clone() };

        // Geometry: panel docked to the right; the square button's side equals the title-bar height, sitting
        // outside the left of the title bar when open, and on the right edge when collapsed.
        const HEAD_H: f32 = 44.0;
        let screen = ctx.input(|i| i.viewport().monitor_size).unwrap_or(egui::vec2(1920.0, 1080.0));
        let panel = egui::vec2(320.0, 520.0);
        // Leave ~64 px at the bottom for the taskbar (measured on XFCE 1080p).
        // Default is docked right; if the user dragged the panel, use that position.
        let (panel_x, panel_y) = match self.chat_pos {
            Some(p) => (p.x, p.y),
            None => ((screen.x - panel.x).max(0.0), (screen.y - panel.y - 64.0).max(0.0)),
        };
        let btn_pos = |docked: bool| {
            if docked {
                egui::pos2(screen.x - HEAD_H, panel_y)
            } else {
                egui::pos2(panel_x - HEAD_H, panel_y)
            }
        };
        let docked = self.chat_dock;
        let wkind = CHAT_X11;

        let mut out_msg: Option<String> = None;
        let mut close_chat = false;
        let mut edited = false;
        let mut toggle_dock = false;
        let mut picked: Option<crate::attach::Pick> = None;
        let mut dropped: Vec<std::path::PathBuf> = Vec::new();
        let mut drop_ix: Option<usize> = None;
        let mut open_path: Option<std::path::PathBuf> = None;
        let mut real_pos: Option<egui::Pos2> = None;
        let use_ime = self.linux_ime;
        let files = self.atts.lock().unwrap().clone();
        let new_files = files.len() != self.att_seen;
        self.att_seen = files.len();
        let new_lines = lines.len() != self.seen_n;
        self.seen_n = lines.len();

        // -- Square collapse button (its own window, no frame, no shadow) --
        let btn_vp = egui::ViewportId::from_hash_of("x-remote-chat-nut");
        let btn_b = egui::ViewportBuilder::default()
            .with_title("Chat — thu/hiện")
            .with_decorations(false)
            .with_taskbar(false)
            .with_resizable(false)
            .with_inner_size([HEAD_H, HEAD_H])
            .with_min_inner_size([HEAD_H, HEAD_H])
            .with_position(btn_pos(docked))
            .with_window_level(egui::WindowLevel::AlwaysOnTop)
            .with_window_type(wkind);
        ctx.show_viewport_immediate(btn_vp, btn_b, |ctx, _class| {
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
            egui::CentralPanel::default().frame(egui::Frame::new().fill(m::CARD)).show(ctx, |ui| {
                let r = ui.max_rect();
                let resp = ui.interact(r, ui.id().with("nut-thu-hien"), egui::Sense::click());
                if resp.hovered() {
                    ui.painter().rect_filled(r, 0.0, m::HOVER);
                }
                // Open: chevron points right (push the panel to the right edge); collapsed: points left (pull it out).
                chat_icon::chevron(ui.painter(), r.center(), 22.0, !docked, m::ACCENT);
                ui.painter().rect_stroke(r, 0.0, egui::Stroke::new(1.0_f32, m::EDGE_STRONG), egui::StrokeKind::Inside);
                if resp.on_hover_text(if docked { "Mở khung chat" } else { "Thu gọn khung chat vào mép phải" }).clicked() {
                    toggle_dock = true;
                }
            });
        });

        // -- Chat panel --
        if !docked {
            let input = &mut self.draft;
            let id = egui::ViewportId::from_hash_of("x-remote-chat");
            let builder = egui::ViewportBuilder::default()
                .with_title(format!("Chat — {show_name}"))
                .with_decorations(false)
                .with_taskbar(false)
                .with_inner_size(panel)
                .with_resizable(false)
                .with_position(egui::pos2(panel_x, panel_y))
                .with_window_level(egui::WindowLevel::AlwaysOnTop)
                .with_window_type(wkind);
            ctx.show_viewport_immediate(id, builder, |ctx, _class| {
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                if ctx.input(|i| i.viewport().close_requested()) {
                    close_chat = true;
                }
                // Real panel position (changes when the user drags the title bar) — the square button follows.
                real_pos = ctx.input(|i| i.viewport().outer_rect).map(|r| r.min);
                if in_sess {
                    dropped = ctx.input(|i| i.raw.dropped_files.iter().filter_map(|f| f.path.clone()).collect());
                }

                // Title bar: name + close (drag to move). The collapse button sits outside, on the left.
                egui::TopBottomPanel::top("chat-dau")
                    .exact_height(HEAD_H)
                    // The default separator is a heavy stroke; a frame `stroke` makes the panel 2 px taller
                    // and shows a black gap (pixel check 21:33) — draw a 1 px line underneath instead.
                    .show_separator_line(false)
                    .frame(egui::Frame::new().fill(m::CARD).inner_margin(egui::Margin::symmetric(12, 0)))
                    .show(ctx, |ui| {
                        let r = ui.max_rect();
                        ui.painter().hline(
                            (r.left() - 12.0)..=(r.right() + 12.0),
                            r.bottom() - 0.5,
                            egui::Stroke::new(1.0_f32, m::EDGE),
                        );
                        let drag = ui.interact(r, ui.id().with("keo-chat"), egui::Sense::click_and_drag());
                        if drag.drag_started() {
                            ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                        }
                        ui.horizontal_centered(|ui| {
                            // Title-bar text: not selectable, does not take the pointer — drag on the text
                            // still moves the window (measured 21:49: the default label selected text instead of dragging).
                            let text = |ui: &mut egui::Ui, t: RichText| {
                                ui.add(egui::Label::new(t).selectable(false).sense(egui::Sense::hover()));
                            };
                            // Peer name only (do not write the word Chat).
                            let head_name = if in_sess || !name.is_empty() { show_name.as_str() } else { "Chưa có ai kết nối" };
                            text(ui, RichText::new(head_name).size(14.0).color(TEXT));
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui
                                    .add(
                                        egui::Button::new(RichText::new("×").size(17.0).color(m::MUTED))
                                            .frame(false)
                                            .min_size(egui::vec2(28.0, 28.0)),
                                    )
                                    .on_hover_text("Đóng khung chat")
                                    .clicked()
                                {
                                    close_chat = true;
                                }
                            });
                        });
                    });

                // Input row: [+] [text] [paper plane].
                egui::TopBottomPanel::bottom("chat-nhap")
                    .show_separator_line(false)
                    .frame(
                        egui::Frame::new()
                            .fill(m::CARD)
                            .inner_margin(egui::Margin { left: 10, right: 10, top: 10, bottom: 12 }),
                    )
                    .show(ctx, |ui| {
                        let r = ui.max_rect();
                        ui.painter().hline(
                            (r.left() - 10.0)..=(r.right() + 10.0),
                            r.top() - 10.0 + 0.5,
                            egui::Stroke::new(1.0_f32, m::EDGE),
                        );
                        let height = 34.0;
                        let sq = egui::vec2(height, height);
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 6.0;
                            // "+" opens File / Image / Folder.
                            ui.add_enabled_ui(in_sess, |ui| {
                                let btn = egui::Button::new("").frame(false).min_size(sq);
                                let menu = egui::menu::menu_custom_button(ui, btn, |ui| {
                                    ui.set_min_width(160.0);
                                    if ui.button("Tệp…").clicked() {
                                        picked = Some(crate::attach::Pick::File);
                                        ui.close_menu();
                                    }
                                    if ui.button("Hình ảnh…").clicked() {
                                        picked = Some(crate::attach::Pick::Image);
                                        ui.close_menu();
                                    }
                                    if ui.button("Thư mục (chèn liên kết)…").clicked() {
                                        picked = Some(crate::attach::Pick::Dir);
                                        ui.close_menu();
                                    }
                                });
                                let rr = menu.response.rect;
                                if menu.response.hovered() {
                                    ui.painter().rect_filled(rr, 6.0, m::HOVER);
                                }
                                chat_icon::plus_mark(ui.painter(), rr.center(), 7.5, if in_sess { m::ACCENT } else { m::GRAY });
                                menu.response.on_hover_text("Đính kèm tệp, hình ảnh hoặc thư mục — hoặc kéo thả vào đây");
                            });
                            let o = ui.add_enabled(
                                in_sess,
                                egui::TextEdit::singleline(input)
                                    // Hint color is set on its own: the theme overrides text color, so the default hint was as dark as real text.
                                    .hint_text(
                                        RichText::new(if in_sess { "Nhập tin nhắn, Enter để gửi" } else { "Chưa có ai điều khiển" })
                                            .color(m::GRAY),
                                    )
                                    .margin(egui::Margin::symmetric(8, 8))
                                    .min_size(egui::vec2(0.0, height))
                                    // `desired_width` does not include the inner margin (2 x 8 px).
                                    .desired_width(ui.available_width() - height - 6.0 - 16.0),
                            );
                            edited = o.changed();
                            if o.has_focus() && !use_ime {
                                // Turn the Linux IME off for the chat box (see `linux_ime`).
                                ctx.output_mut(|out| out.ime = None);
                            }
                            let enter = o.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                            let has_text = !input.trim().is_empty();
                            let on = in_sess && has_text;
                            let send_r = ui.add_enabled(on, egui::Button::new("").frame(false).min_size(sq));
                            if send_r.hovered() && on {
                                ui.painter().rect_filled(send_r.rect, 6.0, m::HOVER);
                            }
                            chat_icon::paper_plane(
                                ui.painter(),
                                send_r.rect.shrink2(egui::vec2(8.0, 9.0)),
                                if on { m::ACCENT } else { m::GRAY },
                            );
                            let hit = send_r.on_hover_text("Gửi (Enter)").clicked();
                            if (enter || hit) && has_text {
                                out_msg = Some(std::mem::take(input));
                                o.request_focus();
                            }
                        });
                    });

                // Body: peer + chat history.
                egui::CentralPanel::default()
                    .frame(egui::Frame::new().fill(m::BG).inner_margin(egui::Margin::symmetric(12, 10)))
                    .show(ctx, |ui| {
                        let section = |ui: &mut egui::Ui, s: &str| {
                            ui.label(RichText::new(s).size(11.0).color(m::LBL));
                            ui.add_space(4.0);
                        };
                        let card = || {
                            egui::Frame::new()
                                .fill(m::CARD)
                                .stroke(egui::Stroke::new(1.0_f32, m::EDGE))
                                .corner_radius(egui::CornerRadius::same(8))
                        };

                        section(ui, if as_ctrl { "Bạn đang điều khiển" } else { "Đang điều khiển máy này" });
                        card().inner_margin(egui::Margin::symmetric(12, 10)).show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal(|ui| {
                                let (r, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                                ui.painter().circle_filled(r.center(), 4.5, if in_sess { m::OK_GREEN } else { m::GRAY });
                                let card_name = if in_sess || !name.is_empty() { show_name.as_str() } else { "Chưa có ai" };
                                ui.label(RichText::new(card_name).size(13.0).color(TEXT));
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    ui.label(
                                        RichText::new(if in_sess { "Đang kết nối" } else { "Đã ngắt" })
                                            .size(11.0)
                                            .color(if in_sess { m::OK_GREEN } else { m::MUTED }),
                                    );
                                });
                            });
                        });
                        ui.add_space(12.0);

                        section(ui, "Lịch sử chat");
                        // Reserve a fixed row for "typing" so the card does not resize.
                        let type_h = 18.0;
                        let card_h = (ui.available_height() - type_h - 6.0).max(80.0);
                        card().inner_margin(egui::Margin::same(10)).show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.set_height(card_h - 22.0);
                            if lines.is_empty() && files.is_empty() {
                                ui.centered_and_justified(|ui| {
                                    ui.label(
                                        RichText::new("Chưa có tin nhắn.\nTin từ người điều khiển sẽ hiện ở đây.")
                                            .size(12.0)
                                            .color(m::MUTED),
                                    );
                                });
                                return;
                            }
                            egui::ScrollArea::vertical().auto_shrink([false, false]).stick_to_bottom(true).show(ui, |ui| {
                                let width = ui.available_width();
                                let bubble_w = (width * 0.82).max(120.0);
                                let mut prev: Option<(bool, &str)> = None;
                                for d in &lines {
                                    // Merge adjacent messages from the same person in the same minute: name/time once.
                                    let group_head = prev != Some((d.mine, d.clock.as_str()));
                                    prev = Some((d.mine, d.clock.as_str()));
                                    let side = if d.mine {
                                        egui::Layout::right_to_left(egui::Align::TOP)
                                    } else {
                                        egui::Layout::left_to_right(egui::Align::TOP)
                                    };
                                    if group_head {
                                        ui.add_space(4.0);
                                        ui.with_layout(side, |ui| {
                                            let (who, color) =
                                                if d.mine { ("Tôi", m::NAME_MINE) } else { (show_name.as_str(), m::NAME_PEER) };
                                            ui.label(RichText::new(format!("{who} · {}", d.clock)).size(11.0).color(color));
                                        });
                                        ui.add_space(2.0);
                                    }
                                    ui.with_layout(side, |ui| {
                                        egui::Frame::new()
                                            .fill(if d.mine { m::BUBBLE_MINE } else { m::BUBBLE_PEER })
                                            .corner_radius(egui::CornerRadius::same(10))
                                            .inner_margin(egui::Margin::symmetric(10, 6))
                                            .show(ui, |ui| {
                                                ui.set_max_width(bubble_w - 20.0);
                                                // Text in the bubble is left-aligned (halign; do not wrap a vertical layout —
                                                // a vertical layout makes a short bubble as wide as the row).
                                                ui.add(
                                                    egui::Label::new(RichText::new(&d.body).size(13.0).color(TEXT))
                                                        .halign(egui::Align::LEFT)
                                                        .wrap(),
                                                );
                                            });
                                    });
                                    // Folder link: a small card under the bubble (inside the bubble, right-to-left layout
                                    // placed it beside the text — measured 22:22). Click to open that folder.
                                    for (label, p) in &d.dirs {
                                        ui.add_space(2.0);
                                        ui.with_layout(side, |ui| {
                                            egui::Frame::new()
                                                .fill(m::CARD)
                                                .stroke(egui::Stroke::new(1.0_f32, m::EDGE_STRONG))
                                                .corner_radius(egui::CornerRadius::same(8))
                                                .inner_margin(egui::Margin::symmetric(8, 4))
                                                .show(ui, |ui| {
                                                    ui.spacing_mut().item_spacing.x = 5.0;
                                                    let icon_fn = |ui: &mut egui::Ui| {
                                                        let (o, _) = ui.allocate_exact_size(egui::vec2(14.0, 12.0), egui::Sense::hover());
                                                        chat_icon::folder(ui.painter(), o, m::ACCENT);
                                                    };
                                                    let link_lbl = RichText::new(label.trim_matches(|c| c == '[' || c == ']'))
                                                        .size(13.0)
                                                        .color(m::ACCENT);
                                                    // The icon always sits BEFORE the name: with right-to-left layout, add it after.
                                                    if !d.mine {
                                                        icon_fn(ui);
                                                    }
                                                    let links = ui.add(egui::Link::new(link_lbl));
                                                    if d.mine {
                                                        icon_fn(ui);
                                                    }
                                                    if links.on_hover_text(format!("Mở thư mục {}", p.display())).clicked() {
                                                        open_path = Some(p.clone());
                                                    }
                                                });
                                        });
                                    }
                                    ui.add_space(3.0);
                                }
                                // My attachments: a card on the right, showing the real state.
                                for (i, t) in files.iter().enumerate() {
                                    ui.add_space(4.0);
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
                                        ui.label(RichText::new(format!("Tôi · {}", t.clock)).size(11.0).color(m::NAME_MINE));
                                    });
                                    ui.add_space(2.0);
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
                                        egui::Frame::new()
                                            .fill(m::CARD)
                                            .stroke(egui::Stroke::new(1.0_f32, m::EDGE_STRONG))
                                            .corner_radius(egui::CornerRadius::same(10))
                                            .inner_margin(egui::Margin::symmetric(10, 8))
                                            .show(ui, |ui| {
                                                ui.set_max_width(bubble_w - 20.0);
                                                ui.horizontal(|ui| {
                                                    let (ir, _) = ui.allocate_exact_size(egui::vec2(22.0, 26.0), egui::Sense::hover());
                                                    chat_icon::file_icon(ui.painter(), ir.shrink2(egui::vec2(2.0, 2.0)), t.is_image, m::ACCENT);
                                                    ui.vertical(|ui| {
                                                        ui.set_max_width(bubble_w - 80.0);
                                                        ui.add(
                                                            egui::Label::new(RichText::new(&t.name).size(13.0).color(TEXT))
                                                                .halign(egui::Align::LEFT)
                                                                .wrap(),
                                                        );
                                                        let (text, color) = match &t.state {
                                                            crate::attach::FileState::Ready => (
                                                                format!(
                                                                    "{} · chưa gửi được: đang hoàn thiện gửi tệp",
                                                                    crate::attach::human_size(t.size)
                                                                ),
                                                                m::MUTED,
                                                            ),
                                                            crate::attach::FileState::Bad(e) => (format!("Lỗi: {e}"), RED),
                                                        };
                                                        ui.add(egui::Label::new(RichText::new(text).size(11.0).color(color)).wrap());
                                                    });
                                                    if ui
                                                        .add(egui::Button::new(RichText::new("×").size(14.0).color(m::MUTED)).frame(false))
                                                        .on_hover_text("Bỏ tệp này")
                                                        .clicked()
                                                    {
                                                        drop_ix = Some(i);
                                                    }
                                                });
                                            });
                                    });
                                }
                                if new_lines || new_files {
                                    ui.scroll_to_cursor(Some(egui::Align::BOTTOM));
                                }
                            });
                        });
                        ui.add_space(4.0);
                        ui.allocate_ui_with_layout(
                            egui::vec2(ui.available_width(), type_h),
                            egui::Layout::left_to_right(egui::Align::Center),
                            |ui| {
                                if in_sess && typing {
                                    ui.label(RichText::new(format!("{show_name} đang gõ…")).size(11.0).italics().color(m::MUTED));
                                }
                            },
                        );
                    });

                // Thin border around the panel — the window has no system frame and needs to separate from the background.
                ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("vien-chat"))).rect_stroke(
                    ctx.screen_rect(),
                    0.0,
                    egui::Stroke::new(1.0_f32, m::EDGE_STRONG),
                    egui::StrokeKind::Inside,
                );
            });
        }

        // Drop the window-manager shadow on the panel + button (new window, or just reopened).
        #[cfg(target_os = "linux")]
        {
            if !self.shadow_tried {
                self.shadow_tried = true;
                self.shaper = crate::shadow::Shadow::new();
            }
            if let Some(b) = self.shaper.as_mut() {
                b.scan("Chat — ");
            }
            // A new window needs one scan after it is shown.
            ctx.request_repaint_after(std::time::Duration::from_millis(350));
        }

        // Panel was dragged -> remember the position and move the square button with it (outside, left of the title bar).
        // Ignore a shift of 1 px or less (shadow's 1 px resize).
        if let Some(p) = real_pos {
            let was = egui::pos2(panel_x, panel_y);
            if (p - was).length() > 1.5 {
                self.chat_pos = Some(p);
                ctx.send_viewport_cmd_to(btn_vp, egui::ViewportCommand::OuterPosition(egui::pos2(p.x - HEAD_H, p.y)));
                ctx.request_repaint();
            }
        }

        // Collapse / expand: the panel closes and reopens itself (immediate window); the button moves.
        if toggle_dock {
            self.chat_dock = !self.chat_dock;
            ctx.send_viewport_cmd_to(btn_vp, egui::ViewportCommand::OuterPosition(btn_pos(self.chat_dock)));
            ctx.request_repaint();
        }
        if let Some(kind) = picked {
            let c = ctx.clone();
            crate::attach::pick(
                kind,
                self.atts.clone(),
                self.dir_q.clone(),
                crate::hostmode::hhmm(),
                move || c.request_repaint(),
            );
        }
        for p in dropped {
            // Drag-and-drop: a folder becomes a link in the draft; a file becomes an attachment card.
            if p.is_dir() {
                self.dir_q.lock().unwrap().push(p);
                ctx.request_repaint();
            } else {
                crate::attach::add_file(&self.atts, &p, &crate::hostmode::hhmm());
            }
        }
        if let Some(i) = drop_ix {
            let mut g = self.atts.lock().unwrap();
            if i < g.len() {
                g.remove(i);
            }
        }
        if let Some(p) = open_path {
            crate::attach::open_dir(&p);
        }

        // Tell the controller typing: 29 when typing starts, 30 when the draft is cleared or idle for 6 s.
        let has_text = !self.draft.trim().is_empty();
        if edited {
            self.edit_at = std::time::Instant::now();
        }
        if let Some(t) = out_msg {
            // Folder links: show the name on this machine, send the full path to the controller.
            let links: Vec<(String, std::path::PathBuf)> =
                self.link_q.iter().filter(|(n, _)| t.contains(n.as_str())).cloned().collect();
            let mut wire_body = t.clone();
            for (n, p) in &links {
                wire_body = wire_body.replace(n.as_str(), &format!("[{}]", p.display()));
            }
            let sent = if as_ctrl {
                self.send(Ctl { t: "chat".into(), c: wire_body.clone(), ..Default::default() });
                self.chat_ctl.lines.push(crate::hostmode::ChatLine {
                    mine: true,
                    clock: crate::hostmode::hhmm(),
                    body: t.trim().to_string(),
                    dirs: links,
                });
                true
            } else {
                crate::hostmode::send_chat_links(&self.host, &t, &wire_body, links)
            };
            if sent {
                self.typing_out = false; // the message already included "stopped typing"
                self.link_q.clear();
            } else {
                self.draft = t; // no session -> keep what was typed
            }
            ctx.request_repaint();
        } else if has_text && edited && !self.typing_out {
            self.set_typing(as_ctrl, true);
            self.typing_out = true;
        } else if self.typing_out
            && (!has_text || self.edit_at.elapsed() >= std::time::Duration::from_secs(6))
        {
            self.set_typing(as_ctrl, false);
            self.typing_out = false;
        }
        if self.typing_out {
            ctx.request_repaint_after(std::time::Duration::from_millis(500));
        }
        if close_chat {
            self.chat_open = false;
            if self.typing_out {
                self.set_typing(as_ctrl, false);
                self.typing_out = false;
            }
        }
    }
}

impl App {
    /// UltraViewer-style menu bar: File, Settings, Help.
    fn menu_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("menu")
            .exact_height(BAR_H)
            .frame(
                egui::Frame::new()
                    .fill(UV_MENU)
                    .inner_margin(egui::Margin::symmetric(6, 0)),
            )
            .show(ctx, |ui| {
              ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.style_mut().visuals.widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
                egui::menu::bar(ui, |ui| {
                    ui.spacing_mut().item_spacing.x = 16.0;
                    ui.add_space(4.0);
                    ui.menu_button(RichText::new("Tệp tin").size(12.5), |ui| {
                        if ui.button("Thoát").clicked() {
                            ui.close_menu();
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    });
                    ui.menu_button(RichText::new("Thiết lập").size(12.5), |ui| {
                        ui.checkbox(&mut self.linux_ime, "Gõ tiếng Việt bằng bộ gõ Linux (fcitx) trong chat")
                            .on_hover_text(
                                "Bật khi ngồi gõ trực tiếp trên máy này. Tắt (mặc định) khi gõ từ máy điều                                  khiển: chữ có dấu đã tới sẵn, bộ gõ Linux chỉ thêm chữ đang ghép bôi xanh.",
                            );
                        if ui.checkbox(&mut self.on_top, "Luôn ở trên cùng").changed() {
                            let item = if self.on_top {
                                egui::WindowLevel::AlwaysOnTop
                            } else {
                                egui::WindowLevel::Normal
                            };
                            ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(item));
                            ui.close_menu();
                        }
                    });
                    ui.menu_button(RichText::new("Trợ giúp").size(12.5), |ui| {
                        if ui.button("Giới thiệu").clicked() {
                            ui.close_menu();
                            self.status = format!(
                                "X-Remote {} — client UltraViewer cho Linux/macOS",
                                env!("CARGO_PKG_VERSION")
                            );
                            self.tone = Tone::Ok;
                        }
                    });
                });
              });
            });
    }

    fn connect_view(&mut self, ctx: &egui::Context) {
        self.menu_bar(ctx);

        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(Color32::WHITE)
                    .inner_margin(egui::Margin { left: 16, right: 16, top: 14, bottom: 12 }),
            )
            .show(ctx, |ui| {
                let own_pw = self.own_pw.clone();

                // Hand layout in a fixed column, centered when the window is wider.
                let area = ui.max_rect();
                let width = COL_W.min(area.width());
                let x = (area.center().x - width / 2.0).round();
                let mut y = area.top();
                let form_w = FORM_W.min(width);
                // Label + field block is left-aligned, flush with the description (x + 4).
                let xf = x + 4.0;
                let row = |y: f32| egui::Rect::from_min_size(egui::pos2(xf, y), egui::vec2(form_w, BOX_H));
                let mut start = false;

                // ---- Allow control ----
                band_title(ui, egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(width, 32.0)), false, "Cho phép điều khiển");
                y += 32.0 + 10.0;
                // Description height is FIXED (1 line) so the panel below does not jump when the text changes.
                hint(ui, x, y, "Gửi thông tin cho đối tác hỗ trợ");
                y += HINT_H + 12.0;

                // Background host status: granted id + status line.
                let host_id = self.host.lock().unwrap().granted_id.clone();
                // A saved id is shown at once (grouped by 3). "đang đăng ký" only
                // when there is no host_id file yet.
                let shown_id = host_id_label(host_id.as_deref(), crate::hostmode::HOST_OK);
                if host_id.is_none() && crate::hostmode::HOST_OK {
                    ctx.request_repaint_after(std::time::Duration::from_millis(200));
                }

                let d = row(y);
                label_left(ui, d, "ID của bạn");
                ro_box(ui, box_of(d), &shown_id, 15.0);
                y += BOX_H + 8.0;

                let d = row(y);
                label_left(ui, d, "Mật khẩu");
                ro_box(ui, box_of(d), &own_pw, 14.0);
                // One icon in the password field: copy both the id and
                // the password, ready to paste to the peer.
                if pw_btn(ui, box_of(d), "doi-mat-khau") {
                    let new = rand_pw();
                    self.own_pw = new.clone();
                    crate::hostmode::set_password(new);
                    self.status = "Đã đổi mật khẩu — đang đăng ký lại với máy chủ".into();
                    self.tone = Tone::Ok;
                }
                if copy_btn(ui, box_of(d), "chep-id-pw", "Sao chép ID và mật khẩu") {
                    if let Some(id) = &host_id {
                        ctx.copy_text(format!("ID của bạn: {id}\nMật khẩu: {own_pw}"));
                        self.status = "Đã sao chép ID và mật khẩu".into();
                        self.tone = Tone::Ok;
                    } else {
                        self.status = if crate::hostmode::HOST_OK {
                            "Chưa có ID — đang đăng ký với máy chủ".into()
                        } else {
                            "Nhận điều khiển (bị điều khiển) chưa hỗ trợ trên máy này".into()
                        };
                        self.tone = Tone::Warn;
                    }
                }
                y += BOX_H + 16.0;

                // Rule between the two groups.
                let rule_y = y.round() + 0.5;
                ui.painter().hline(
                    (x + 24.0)..=(x + width - 24.0),
                    rule_y,
                    egui::Stroke::new(1.0_f32, UV_RULE),
                );
                y += 20.0;

                // ---- Control another computer ----
                band_title(ui, egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(width, 32.0)), true, "Điều khiển máy tính khác");
                y += 32.0 + 10.0;
                y += hint(
                    ui,
                    x,
                    y,
                    "Nhập thông tin máy cần điều khiển",
                );
                y += 12.0;

                let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));

                let d = row(y);
                label_left(ui, d, "ID đối tác");
                let id_box = box_of(d);
                let id_resp = edit_box(ui, id_box, &mut self.peer_id, false, 26.0);
                if self.ask_id_focus {
                    id_resp.request_focus();
                    self.ask_id_focus = false;
                }
                self.id_popup(ui, &id_resp, id_box);
                y += BOX_H + 8.0;

                let d = row(y);
                label_left(ui, d, "Mật khẩu");
                let pw_resp = edit_box(ui, box_of(d), &mut self.peer_pw, true, 0.0);
                y += BOX_H + 16.0;

                let ready = !self.peer_id.trim().is_empty();
                if enter && ready && (id_resp.lost_focus() || pw_resp.lost_focus()) {
                    start = true;
                }

                let btn_rect = box_of(egui::Rect::from_min_size(egui::pos2(xf, y), egui::vec2(form_w, 32.0)));
                let b = btn(ui, btn_rect, "nut-bat-dau", IconKind::Arrow, "Bắt đầu điều khiển", ready);
                if b.clicked() && ready {
                    start = true;
                }
                if !ready {
                    b.on_hover_text("Nhập ID đối tác trước");
                }
                y += 32.0;

                // Tell egui how much was used so it can compute the minimum size.
                ui.advance_cursor_after_rect(egui::Rect::from_min_max(
                    area.min,
                    egui::pos2(area.right(), y),
                ));

                if start {
                    self.connect(ctx);
                }
            });
    }

    /// Keys menu: system chords that this machine would swallow if typed here.
    fn key_menu(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("Phím", |ui| {
            let item: [(&str, &str, &[u16]); 6] = [
                ("Alt+Tab", "Alt+Tab — đổi cửa sổ", &[VK_ALT, VK_TAB]),
                ("phím Windows", "Phím Windows — mở Start", &[VK_LWIN]),
                ("Win+D", "Win+D — hiện màn hình nền", &[VK_LWIN, VK_D]),
                ("Win+E", "Win+E — mở File Explorer", &[VK_LWIN, VK_E]),
                ("Ctrl+Shift+Esc", "Ctrl+Shift+Esc — Task Manager", &[VK_CTRL, VK_SHIFT, VK_ESC]),
                ("Alt+F4", "Alt+F4 — đóng cửa sổ", &[VK_ALT, VK_F4]),
            ];
            for (name, label, keys) in item {
                if ui.button(label).clicked() {
                    ui.close_menu();
                    self.send_combo(name, keys);
                }
            }
            if self.uv.is_some() {
                ui.separator();
                ui.checkbox(&mut self.paste_type, "Ctrl+V gõ chữ từ clipboard máy này")
                    .on_hover_text(
                        "UltraViewer chưa chuyển được clipboard. Bật mục này thì Ctrl+V \
                         gõ lại chữ trong clipboard máy này sang máy đích; tắt thì Ctrl+V \
                         dán clipboard của chính máy đích.",
                    );
            }
        });
    }

    /// Control session: the peer picture fills the window, a white toolbar floats
    /// at the top center like UltraViewer (Actions, Settings, Screenshot, Chat).
    fn session_view(&mut self, ctx: &egui::Context) {
        // Window title like UltraViewer: "<machine name> (<id>) - X-Remote".
        let title_s = if self.chat_ctl.name.is_empty() {
            format!("{} - X-Remote", group_id(&self.clean_id()))
        } else {
            format!("{} ({}) - X-Remote", self.chat_ctl.name, group_id(&self.clean_id()))
        };
        if self.sess_title != title_s {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title_s.clone()));
            self.sess_title = title_s;
        }

        self.sess_picture(ctx);
        self.sess_bar(ctx);
    }

    /// Floating toolbar (egui `Area`, above the picture so the picture uses the full height).
    fn sess_bar(&mut self, ctx: &egui::Context) {
        use chat_color as m;
        let screen = ctx.screen_rect();
        if self.bar_hide {
            // Collapsed: a small down-chevron tab at the top center to show the bar again.
            egui::Area::new(egui::Id::new("thanh-phien-an"))
                .fixed_pos(egui::pos2(screen.center().x - 22.0, screen.top()))
                .order(egui::Order::Foreground)
                .show(ctx, |ui| {
                    let (r, resp) = ui.allocate_exact_size(egui::vec2(44.0, 16.0), egui::Sense::click());
                    let bg = if resp.hovered() { m::HOVER } else { m::CARD };
                    ui.painter().rect(
                        r,
                        egui::CornerRadius { nw: 0, ne: 0, sw: 6, se: 6 },
                        bg,
                        egui::Stroke::new(1.0_f32, m::EDGE_STRONG),
                        egui::StrokeKind::Inside,
                    );
                    chat_icon::chevron_v(ui.painter(), r.center(), 9.0, true, m::ACCENT);
                    if resp.on_hover_text("Hiện thanh công cụ").clicked() {
                        self.bar_hide = false;
                    }
                });
            return;
        }

        let mut set_quality: Option<i32> = None;
        let mut set_mon: Option<Option<usize>> = None;
        let mut set_wall = false;
        let mut set_fps: Option<u16> = None;
        let mut set_full = false;
        let mut ctrl_alt_del = false;
        let mut do_stop = false;
        let mut do_shot = false;
        let mut open_shots = false;
        let mut toggle_chat = false;
        let mut hide_bar = false;

        let resp = egui::Area::new(egui::Id::new("thanh-phien"))
            .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 0.0))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(m::CARD)
                    .stroke(egui::Stroke::new(1.0_f32, m::EDGE_STRONG))
                    .corner_radius(egui::CornerRadius { nw: 0, ne: 0, sw: 8, se: 8 })
                    .inner_margin(egui::Margin { left: 6, right: 6, top: 4, bottom: 4 })
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 2.0;
                            // Menu button: icon on the left, label, dropdown arrow on the right (28 px pad both sides).
                            let menu_btn = |ui: &mut egui::Ui,
                                            text: &str,
                                            draw_icon: &dyn Fn(&egui::Painter, egui::Rect),
                                            has_menu: bool,
                                            body: &mut dyn FnMut(&mut egui::Ui)|
                             -> egui::Response {
                                // A `frame(false)` button ignores `button_padding` (measured 00:04: icon over text,
                                // buttons stuck together) -> compute width: text + 30 px left (icon) + 24 px
                                // right (dropdown arrow) or 12 px with no menu. Centering the label with spaces does not
                                // work — draw the text by hand and leave the button empty.
                                let text_w = ui.fonts(|f| {
                                    f.layout_no_wrap(text.to_string(), egui::FontId::proportional(13.0), TEXT).size().x
                                });
                                let x_left = 30.0;
                                let pad_r = if has_menu { 24.0 } else { 12.0 };
                                let btn = egui::Button::new("").frame(false).min_size(egui::vec2(x_left + text_w + pad_r, 30.0));
                                let r = if has_menu {
                                    egui::menu::menu_custom_button(ui, btn, |ui| {
                                        ui.set_min_width(210.0);
                                        body(ui)
                                    })
                                    .response
                                } else {
                                    ui.add(btn)
                                };
                                if r.hovered() {
                                    ui.painter().rect_filled(r.rect, 5.0, m::HOVER);
                                }
                                let o = egui::Rect::from_center_size(
                                    egui::pos2(r.rect.left() + 15.0, r.rect.center().y),
                                    egui::vec2(18.0, 18.0),
                                );
                                draw_icon(ui.painter(), o);
                                ui.painter().text(
                                    egui::pos2(r.rect.left() + x_left, r.rect.center().y),
                                    egui::Align2::LEFT_CENTER,
                                    text,
                                    egui::FontId::proportional(13.0),
                                    TEXT,
                                );
                                if has_menu {
                                    chat_icon::chevron_v(
                                        ui.painter(),
                                        egui::pos2(r.rect.right() - 12.0, r.rect.center().y),
                                        4.0,
                                        true,
                                        m::MUTED,
                                    );
                                }
                                r
                            };

                            // -- Actions --
                            menu_btn(ui, "Thao tác", &|p, o| chat_icon::bolt(p, o), true, &mut |ui| {
                                if ui.button("Gửi Ctrl+Alt+Del").clicked() {
                                    ctrl_alt_del = true;
                                    ui.close_menu();
                                }
                                self.key_menu(ui);
                                ui.separator();
                                if ui.button("Ngắt kết nối").clicked() {
                                    do_stop = true;
                                    ui.close_menu();
                                }
                            });

                            // -- Settings --
                            menu_btn(ui, "Cấu hình", &|p, o| chat_icon::gear(p, o, m::MUTED), true, &mut |ui| {
                                ui.label(RichText::new("Chất lượng hình").size(11.0).color(m::LBL));
                                for (v, name) in [(35i32, "Thấp — nhẹ mạng"), (60, "Vừa"), (85, "Cao — nét")] {
                                    if menu_radio(ui, self.quality == v, name).clicked() {
                                        set_quality = Some(v);
                                        ui.close_menu();
                                    }
                                }
                                if self.monitors.len() > 1 {
                                    ui.separator();
                                    ui.label(RichText::new("Màn hình").size(11.0).color(m::LBL));
                                    if menu_radio(ui, self.mon_ix.is_none(), "Tất cả màn hình").clicked() {
                                        set_mon = Some(None);
                                        ui.close_menu();
                                    }
                                    for (i, mh) in self.monitors.iter().enumerate() {
                                        let name = format!("Màn {} — {}×{}", i + 1, mh.w, mh.h);
                                        if menu_radio(ui, self.mon_ix == Some(i), name).clicked() {
                                            set_mon = Some(Some(i));
                                            ui.close_menu();
                                        }
                                    }
                                }
                                ui.separator();
                                if self.is_agent() {
                                    ui.label(RichText::new("Tốc độ tối đa").size(11.0).color(m::LBL));
                                    for v in [5u16, 10, 20, 30, 60] {
                                        if menu_radio(ui, self.fps == v, format!("{v} hình/giây")).clicked() {
                                            set_fps = Some(v);
                                            ui.close_menu();
                                        }
                                    }
                                    ui.separator();
                                } else if self.uv.is_some() {
                                    if menu_check(ui, self.hide_wall, "Ẩn hình nền máy đích").clicked() {
                                        set_wall = true;
                                        ui.close_menu();
                                    }
                                }
                                if menu_check(ui, self.fullscreen, "Toàn màn hình").clicked() {
                                    set_full = true;
                                    ui.close_menu();
                                }
                            });

                            // -- Screenshot --
                            menu_btn(ui, "Chụp hình", &|p, o| chat_icon::camera(p, o, m::ACCENT), true, &mut |ui| {
                                if ui.add_enabled(self.last_pic.is_some(), egui::Button::new("Lưu ảnh màn hình máy đích")).clicked() {
                                    do_shot = true;
                                    ui.close_menu();
                                }
                                if ui.button("Mở thư mục ảnh đã chụp").clicked() {
                                    open_shots = true;
                                    ui.close_menu();
                                }
                            });

                            // -- Chat --
                            let r = menu_btn(
                                ui,
                                "Chat",
                                &|p, o| chat_icon::bubble(p, o.shrink2(egui::vec2(1.0, 2.0)), m::ACCENT),
                                false,
                                &mut |_| {},
                            );
                            if r.on_hover_text("Mở / đóng khung chat").clicked() {
                                toggle_chat = true;
                            }

                            ui.add_space(4.0);
                            ui.separator();
                            // Collapse the toolbar like UltraViewer.
                            let (rr, rresp) = ui.allocate_exact_size(egui::vec2(24.0, 28.0), egui::Sense::click());
                            if rresp.hovered() {
                                ui.painter().rect_filled(rr, 5.0, m::HOVER);
                            }
                            chat_icon::chevron_v(ui.painter(), rr.center(), 5.0, false, m::MUTED);
                            if rresp.on_hover_text("Thu gọn thanh công cụ").clicked() {
                                hide_bar = true;
                            }
                        });
                    });
            });
        let _ = resp;

        if let Some(v) = set_quality {
            if v != self.quality {
                self.quality = v;
                self.quality_sent = true;
                self.send(Ctl { t: "quality".into(), v: v as u16, ..Default::default() });
                self.status = format!("Chất lượng hình: {}", self.quality_name());
                self.tone = Tone::Ok;
            }
        }
        if let Some(c) = set_mon {
            self.mon_ix = c;
            self.status = match c {
                Some(i) => format!("Đang xem màn {}", i + 1),
                None => "Đang xem tất cả màn hình".into(),
            };
            self.tone = Tone::Ok;
        }
        if let Some(f) = set_fps {
            self.fps = f;
            self.send(Ctl { t: "fps".into(), v: f, ..Default::default() });
            self.status = format!("Tốc độ tối đa: {f} hình/giây");
            self.tone = Tone::Ok;
        }
        if set_wall {
            self.hide_wall = !self.hide_wall;
            let v = if self.hide_wall { 0 } else { 1 };
            self.send(Ctl { t: "wallpaper".into(), v, ..Default::default() });
            self.status =
                if self.hide_wall { "Đã ẩn hình nền máy đích".into() } else { "Đã hiện lại hình nền máy đích".into() };
            self.tone = Tone::Ok;
        }
        if set_full {
            self.fullscreen = !self.fullscreen;
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(self.fullscreen));
        }
        if ctrl_alt_del {
            self.send_cad();
        }
        if do_shot {
            match self.save_shot() {
                Ok(p) => {
                    self.status = format!("Đã lưu ảnh: {}", p.display());
                    self.tone = Tone::Ok;
                }
                Err(e) => {
                    self.status = format!("Lưu ảnh hỏng: {e}");
                    self.tone = Tone::Err;
                }
            }
        }
        if open_shots {
            if let Ok(d) = shot_dir() {
                crate::attach::open_dir(&d);
            }
        }
        if toggle_chat {
            if self.chat_open && !self.chat_dock {
                self.chat_open = false;
            } else {
                self.chat_open = true;
                self.chat_dock = false;
            }
        }
        if hide_bar {
            self.bar_hide = true;
        }
        if do_stop {
            self.disconnect(ctx);
        }
    }

    /// Save the latest peer frame as JPEG in the pictures folder.
    fn save_shot(&self) -> Result<std::path::PathBuf, String> {
        let (w, h, rgb) = self.last_pic.as_ref().ok_or("chưa có hình")?;
        let d = shot_dir()?;
        let name = format!("X-Remote_{}_{}.jpg", self.clean_id(), crate::hostmode::stamp());
        let p = d.join(name);
        let f = std::fs::File::create(&p).map_err(|e| format!("không tạo được {}: {e}", p.display()))?;
        jpeg_encoder::Encoder::new(std::io::BufWriter::new(f), 92)
            .encode(rgb, *w as u16, *h as u16, jpeg_encoder::ColorType::Rgb)
            .map_err(|e| e.to_string())?;
        Ok(p)
    }

    /// Peer picture area (mouse and keys go through here).
    fn sess_picture(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(Color32::from_rgb(0x1E, 0x1E, 0x1E)))
            .show(ctx, |ui| {
                let avail = ui.available_size();
                let (rect, response) =
                    ui.allocate_exact_size(avail, egui::Sense::click_and_drag());

                // Take keyboard focus as soon as the session starts: do not make the user
                // click the picture once before they can type.
                // There is no text field on the session screen, so when no widget holds
                // focus (menu just closed, click on the toolbar, ...) give focus back to
                // the picture — otherwise the user had to click the picture again (that click
                // leaked to the peer) before typing worked.
                let no_focus = ui.ctx().memory(|m| m.focused().is_none());
                if !self.got_focus || no_focus {
                    response.request_focus();
                    self.got_focus = true;
                }

                let Some(tex) = &self.texture else {
                    ui.painter().text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        "Đang chờ hình từ máy khách…",
                        egui::FontId::proportional(14.0),
                        Color32::from_gray(0x88),
                    );
                    return;
                };

                let [w, h] = self.size;
                if w == 0 || h == 0 {
                    return;
                }
                // Draw only the selected monitor (or the whole image); mouse coordinates still map to the whole image
                // so the UltraViewer/agent stream scales to the right point on the peer.
                let (sx, sy, sw, sh) = self.view_rect(w, h);
                let (fw, fh) = (w as f32, h as f32);
                let img = egui::vec2(sw, sh);
                let scale = (rect.width() / img.x).min(rect.height() / img.y);
                let draw = egui::Rect::from_center_size(rect.center(), img * scale);

                ui.painter().image(
                    tex.id(),
                    draw,
                    egui::Rect::from_min_max(
                        egui::pos2(sx / fw, sy / fh),
                        egui::pos2((sx + sw) / fw, (sy + sh) / fh),
                    ),
                    Color32::WHITE,
                );

                let to_norm = |p: egui::Pos2| -> (f32, f32) {
                    to_full(p, draw, (sx, sy, sw, sh), (fw, fh))
                };

                if response.hovered() {
                    if let Some(p) = response.hover_pos() {
                        if self.last_mouse != Some(p) {
                            self.last_mouse = Some(p);
                            let (nx, ny) = to_norm(p);
                            self.mouse("move", nx, ny);
                        }
                    }
                }

                // Three mouse buttons: left, middle, right. The peer tells them apart by button number.
                // Read press/release events, not the state each frame: a touchpad click
                // presses and releases in the same frame and used to be dropped entirely.
                let evs = ui.input(|i| i.events.clone());
                let mut held = [self.left_down, self.mid_down, self.right_down];
                let cmds = ptr_cmds(&evs, response.hovered(), rect, &mut held);
                [self.left_down, self.mid_down, self.right_down] = held;
                for (btn, down, p) in cmds {
                    let (nx, ny) = to_norm(p);
                    self.mouse_btn(if down { "down" } else { "up" }, nx, ny, btn);
                }

                if response.hovered() {
                    let scroll = ui.input(|i| i.raw_scroll_delta.y);
                    // Send WHEEL_DELTA (120) notches at the cursor, like the
                    // Python build. It used to send egui's raw points and coordinates (0, 0).
                    let step = ui.ctx().options(|o| o.line_scroll_speed);
                    let notches = wheel_notches(&mut self.wheel_rem, scroll, step);
                    if notches != 0 {
                        let p = response
                            .hover_pos()
                            .or(self.last_mouse)
                            .unwrap_or(rect.center());
                        let (nx, ny) = to_norm(p);
                        self.send(Ctl {
                            t: "mouse".into(),
                            k: "wheel".into(),
                            x: nx,
                            y: ny,
                            d: (notches * WHEEL_DELTA) as f32,
                            ..Default::default()
                        });
                    }
                }

                if response.clicked() {
                    response.request_focus();
                }
                if response.has_focus() {
                    self.forward_keys(ui);
                }
            });
    }

    fn forward_keys(&mut self, ui: &mut egui::Ui) {
        let (events, modifiers) = ui.input(|i| (i.events.clone(), i.modifiers));
        if std::env::var("UV_DEBUG_INPUT").is_ok() {
            eprintln!("[ui] keys: {} events, focused={}", events.len(), ui.ctx().input(|i| i.focused));
        }

        let now_mods = (modifiers.ctrl, modifiers.alt, modifiers.shift);
        if now_mods != self.mods {
            let pairs = [
                (VK_CTRL, now_mods.0, self.mods.0),
                (VK_ALT, now_mods.1, self.mods.1),
                (VK_SHIFT, now_mods.2, self.mods.2),
            ];
            for (vk, now, before) in pairs {
                if now != before {
                    self.send(Ctl {
                        t: "key".into(),
                        k: "vk".into(),
                        v: vk,
                        down: now,
                        ..Default::default()
                    });
                }
            }
            self.mods = now_mods;
        }

        let debug = std::env::var("UV_DEBUG_INPUT").is_ok();
        for event in events {
            if debug
                && matches!(
                    event,
                    egui::Event::Key { .. }
                        | egui::Event::Text(_)
                        | egui::Event::Paste(_)
                        | egui::Event::Copy
                        | egui::Event::Cut
                )
            {
                let hint = match &event {
                    egui::Event::Paste(s) => format!("Paste({} ký tự)", s.chars().count()),
                    e => format!("{e:?}"),
                };
                eprintln!("[ui] event: {hint}");
            }
            match event {
                egui::Event::Text(text) => {
                    for ch in text.chars() {
                        self.send(Ctl {
                            t: "key".into(),
                            k: "char".into(),
                            c: ch.to_string(),
                            ..Default::default()
                        });
                    }
                }
                // egui-winit turns a Ctrl/Cmd+C/X/V press into Copy/Cut/Paste and does not
                // emit a Key press for them, so the peer used to miss
                // those three chords. Send the press/release pair here.
                egui::Event::Copy => self.send_ctrl_key(VK_C),
                egui::Event::Cut => self.send_ctrl_key(VK_X),
                egui::Event::Paste(text) => self.paste_text(text),
                egui::Event::Key { key, pressed, modifiers, .. } => {
                    // C/X/V: egui swallows the Ctrl/Cmd+C/X/V press to emit
                    // Copy/Cut/Paste (already sent above). A release with no press is the release
                    // of that chord — do not send it again. Do not trust the Ctrl flag on release:
                    // a real session showed the flag can already be off before V is released.
                    if let Some(i) = cxv_ix(key) {
                        if pressed {
                            self.saw_cxv[i] = true;
                        } else if !std::mem::take(&mut self.saw_cxv[i]) {
                            // If this machine's clipboard is empty, egui emits no Paste: still send
                            // Ctrl+V so the peer pastes its own clipboard.
                            if key == egui::Key::V && !std::mem::take(&mut self.did_paste) {
                                self.send_ctrl_key(VK_V);
                            }
                            continue;
                        }
                    }
                    // Function keys are always sent; letters/digits are sent as key codes only while
                    // Ctrl/Alt is held (Ctrl+Z, Ctrl+A, ...). Plain typing is handled by the Text
                    // event below — sending both repeats the character.
                    let vk = vk_of(key).or_else(|| {
                        if modifiers.ctrl || modifiers.alt {
                            vk_letter(key)
                        } else {
                            None
                        }
                    });
                    if let Some(vk) = vk {
                        self.send(Ctl {
                            t: "key".into(),
                            k: "vk".into(),
                            v: vk,
                            down: pressed,
                            ..Default::default()
                        });
                    }
                }
                _ => {}
            }
        }
    }
}

/// Turn egui pointer-button events into commands (button, down?, position) for the peer.
///
/// A press counts only when the cursor is over the picture (`on_pic` = `response.hovered()`,
/// so a click on an open menu over the picture does not leak to the peer); a release is always sent if
/// the press was sent, even after the cursor leaves, so the peer does not stick a button.
/// `held` = sent state of the left, middle, and right buttons.
fn ptr_cmds(
    events: &[egui::Event],
    on_pic: bool,
    area: egui::Rect,
    held: &mut [bool; 3],
) -> Vec<(u8, bool, egui::Pos2)> {
    let mut out = Vec::new();
    for e in events {
        let egui::Event::PointerButton { pos, button, pressed, .. } = e else { continue };
        let btn: u8 = match button {
            egui::PointerButton::Primary => 0,
            egui::PointerButton::Middle => 1,
            egui::PointerButton::Secondary => 2,
            _ => continue,
        };
        let held_btn = &mut held[btn as usize];
        if *pressed {
            if *held_btn || !on_pic || !area.contains(*pos) {
                continue;
            }
            *held_btn = true;
        } else {
            if !*held_btn {
                continue;
            }
            *held_btn = false;
        }
        out.push((btn, *pressed, *pos));
    }
    out
}

/// Index of C, X, V in `saw_cxv`; `None` for any other key.
fn cxv_ix(key: egui::Key) -> Option<usize> {
    match key {
        egui::Key::C => Some(0),
        egui::Key::X => Some(1),
        egui::Key::V => Some(2),
        _ => None,
    }
}

fn save_jpeg(path: &std::path::Path, image: &egui::ColorImage) {
    let (w, h) = (image.size[0] as u16, image.size[1] as u16);
    let mut rgb = Vec::with_capacity(image.pixels.len() * 3);
    for px in &image.pixels {
        rgb.extend_from_slice(&[px.r(), px.g(), px.b()]);
    }
    let mut out = Vec::new();
    let encoder = jpeg_encoder::Encoder::new(&mut out, 92);
    if encoder
        .encode(&rgb, w, h, jpeg_encoder::ColorType::Rgb)
        .is_ok()
    {
        let _ = std::fs::write(path, out);
    }
}

fn vk_letter(key: egui::Key) -> Option<u16> {
    use egui::Key::*;
    Some(match key {
        A => 0x41, B => 0x42, C => 0x43, D => 0x44, E => 0x45, F => 0x46, G => 0x47,
        H => 0x48, I => 0x49, J => 0x4A, K => 0x4B, L => 0x4C, M => 0x4D, N => 0x4E,
        O => 0x4F, P => 0x50, Q => 0x51, R => 0x52, S => 0x53, T => 0x54, U => 0x55,
        V => 0x56, W => 0x57, X => 0x58, Y => 0x59, Z => 0x5A,
        Num0 => 0x30, Num1 => 0x31, Num2 => 0x32, Num3 => 0x33, Num4 => 0x34,
        Num5 => 0x35, Num6 => 0x36, Num7 => 0x37, Num8 => 0x38, Num9 => 0x39,
        Minus => 0xBD,
        Equals | Plus => 0xBB,
        OpenBracket => 0xDB,
        CloseBracket => 0xDD,
        Backslash | Pipe => 0xDC,
        Semicolon | Colon => 0xBA,
        Quote => 0xDE,
        Comma => 0xBC,
        Period => 0xBE,
        Slash | Questionmark => 0xBF,
        Backtick => 0xC0,
        Space => 0x20,
        _ => return None,
    })
}

fn vk_of(key: egui::Key) -> Option<u16> {
    use egui::Key::*;
    Some(match key {
        ArrowDown => 0x28,
        ArrowLeft => 0x25,
        ArrowRight => 0x27,
        ArrowUp => 0x26,
        Backspace => 0x08,
        Delete => 0x2E,
        End => 0x23,
        Enter => 0x0D,
        Escape => 0x1B,
        Home => 0x24,
        PageDown => 0x22,
        PageUp => 0x21,
        Tab => 0x09,
        // No Space: egui already emits `Event::Text(" ")`; also sending the key code makes the
        // peer see two spaces (real session 04/11/2026). Space with
        // Ctrl/Alt goes through `vk_letter`.
        F1 => 0x70,
        F2 => 0x71,
        F3 => 0x72,
        F4 => 0x73,
        F5 => 0x74,
        F6 => 0x75,
        F7 => 0x76,
        F8 => 0x77,
        F9 => 0x78,
        F10 => 0x79,
        F11 => 0x7A,
        F12 => 0x7B,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app_with(id: &str, pw: &str) -> App {
        let mut app = App::new(None);
        app.peer_id = id.into();
        app.peer_pw = pw.into();
        app
    }

    #[test]
    fn new_id_after_disconnect_targets_new_host() {
        // Old bug: the `uv` tuple was built once on the first connect and kept forever.
        let mut app = app_with("111111111", "aaa");
        app.uv = app.pick_uv();
        app.disconnect(&egui::Context::default());

        app.peer_id = " 222222222 ".into();
        app.peer_pw = "bbb".into();
        let target = app.pick_uv().expect("a numeric id must use UltraViewer");
        assert_eq!(target.0, "222222222");
        assert_eq!(target.1, "bbb");
    }

    #[test]
    fn grouped_id_still_connects() {
        // Picking from suggestions fills "100 000 002" (grouped like UltraViewer).
        let app = app_with("100 000 002", "24680");
        let target = app.pick_uv().expect("grouped digits are still an UltraViewer id");
        assert_eq!(target.0, "100000002");
        // Non-breaking space / tab when pasted from elsewhere.
        assert_eq!(norm_id("100\u{a0}000\t002 "), "100000002");
        // A host name (agent path) is kept, with only surrounding whitespace trimmed.
        assert_eq!(norm_id("  may lan  "), "may lan");
    }

    #[test]
    fn non_numeric_id_skips_uv_unless_flag() {
        let mut app = app_with("may-lan", "");
        assert!(app.pick_uv().is_none());
        app.uv_mode = true;
        assert_eq!(app.pick_uv().map(|d| d.0), Some("may-lan".to_string()));
        app.peer_id = "   ".into();
        assert!(app.pick_uv().is_none(), "a blank id is not a target");
    }

    #[test]
    fn numeric_id_goes_to_agent_when_set() {
        let mut app = App::new(Some("127.0.0.1:7801".into()));
        app.peer_id = "100000001".into();
        assert!(app.pick_uv().is_none(), "--agent: a numeric id must go to the agent");
        app.uv_mode = true;
        assert!(app.pick_uv().is_some(), "--uv wins when set explicitly");
    }

    #[test]
    fn set_uv_uses_cli_own_id() {
        let mut app = App::new(None);
        app.set_uv("123".into(), "pw".into(), "999".into(), "888".into());
        let target = app.uv.clone().unwrap();
        assert_eq!((target.2.as_str(), target.3.as_str()), ("999", "888"));
        // Empty keeps the default.
        let mut app = App::new(None);
        let (id, pw) = (app.own_id.clone(), app.own_pw.clone());
        app.set_own(" ".into(), String::new());
        assert_eq!((app.own_id.as_str(), app.own_pw.as_str()), (id.as_str(), pw.as_str()));
    }

    #[test]
    fn default_id_not_hardcoded_and_persists() {
        // Old bug: the default was the Windows machine's id from testing (100000001).
        let app = App::new(None);
        assert!(id_ok(&app.own_id), "default id must be 9 digits: {}", app.own_id);
        assert_ne!(app.own_id, "100000001");
        assert!(app.own_pw.len() == 5 && app.own_pw.bytes().all(|b| b.is_ascii_digit()));

        let dir = std::env::temp_dir().join(format!("xr-id-{}", std::process::id()));
        let path = dir.join("x-remote").join("own_id");
        let _ = std::fs::remove_dir_all(&dir);
        let first = stored_id(&path);
        assert!(id_ok(&first));
        assert_eq!(stored_id(&path), first, "later open must keep the saved id");
        std::fs::write(&path, "hong").unwrap();
        assert!(id_ok(&stored_id(&path)), "a bad file gets a new id");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn saved_id_shows_immediately() {
        assert_eq!(host_id_label(Some("136674901"), true), "136 674 901");
        assert_eq!(host_id_label(Some("  "), true), "— đang đăng ký —");
        assert_eq!(host_id_label(None, true), "— đang đăng ký —");
        assert_eq!(host_id_label(None, false), "— chưa hỗ trợ —");
    }

    #[test]
    fn resize_keeps_aspect() {
        let design = egui::vec2(200.0, 400.0);
        let monitor = Some(egui::vec2(1600.0, 1000.0));
        // Drag evenly to 1.5×: the window keeps that shape and the content zooms to 1.5.
        let (locked, zoom) = next_ratio(design * 1.5, design, 1.0, monitor);
        assert!((locked.x - 300.0).abs() < 0.1 && (locked.y - 600.0).abs() < 0.1);
        assert!((zoom - 1.5).abs() < 0.01);
        // A skewed drag uses the shorter side and returns to 1:2.
        let (locked, zoom) = next_ratio(egui::vec2(500.0, 480.0), design, 1.0, monitor);
        assert!((locked.x / locked.y - 0.5).abs() < 0.01);
        assert!((zoom - 1.2).abs() < 0.01);
        // Already at 2× with the layout back at the design size: stay there.
        let (locked, zoom) = next_ratio(design, design, 2.0, monitor);
        assert!((locked.x - design.x).abs() < 0.1 && (locked.y - design.y).abs() < 0.1);
        assert!((zoom - 2.0).abs() < 0.01);
        // Smaller than the original size snaps back; text does not shrink below the size at launch.
        let (locked, zoom) = next_ratio(design * 0.6, design, 1.0, monitor);
        assert!((locked.x - design.x).abs() < 0.1 && (zoom - 1.0).abs() < 0.01);
        // A monitor smaller than the design: zoom down to fit, and do not overflow.
        let small = Some(egui::vec2(150.0, 280.0));
        let (locked, zoom) = next_ratio(egui::vec2(150.0, 280.0), design, 1.0, small);
        assert!(zoom < 1.0);
        assert!(locked.x <= 150.0 * 0.94 + 1.0);
        assert!(locked.y <= 280.0 * 0.94 + 1.0);
        assert!((locked.x / locked.y - design.x / design.y).abs() < 0.02);
        // Maximize: use as much of the monitor as the ratio allows.
        let (_zmin, zmax) = zoom_bounds(design, 1.0, monitor);
        assert!((zmax - (1000.0 * 0.94 / 400.0)).abs() < 0.02);
    }

    #[test]
    fn disconnect_cancels_pending_retry() {
        // Old bug: Disconnect right as the session dropped left `reconnect_at` set, and the app reopened the session.
        let mut app = app_with("111111111", "aaa");
        app.view = View::Session;
        app.reconnect_tries = 3;
        app.reconnect_at = Some(std::time::Instant::now());
        app.right_down = true;
        app.mods = (true, false, true);
        app.wheel_rem = 17.0;
        app.disconnect(&egui::Context::default());
        assert!(app.reconnect_at.is_none());
        assert_eq!(app.reconnect_tries, 0);
        assert!(app.view == View::Connect);
        assert!(app.ask_id_focus, "Disconnect must focus the id field");
        assert!(!app.right_down && app.mods == (false, false, false));
        assert_eq!(app.wheel_rem, 0.0);
    }

    /// App is in a session; every command sent is recorded.
    fn app_rec(agent: bool) -> App {
        let mut app = app_with("111111111", "aaa");
        if !agent {
            app.uv = app.pick_uv();
        }
        app.view = View::Session;
        app.conn = Some(Link::Rec(Vec::new(), agent));
        app
    }

    fn sent(app: &App) -> Vec<Ctl> {
        match &app.conn {
            Some(Link::Rec(v, _)) => v.clone(),
            _ => Vec::new(),
        }
    }

    /// Reduce a vk key command to (code, down?) for comparison.
    fn keys(v: &[Ctl]) -> Vec<(u16, bool)> {
        v.iter().filter(|c| c.t == "key" && c.k == "vk").map(|c| (c.v, c.down)).collect()
    }

    #[test]
    fn copy_wraps_ctrl_when_not_held() {
        // Old bug: egui swallowed the Ctrl+C press, so the peer received nothing.
        let mut app = app_rec(false);
        app.send_ctrl_key(VK_C);
        assert_eq!(
            keys(&sent(&app)),
            vec![(VK_CTRL, true), (VK_C, true), (VK_C, false), (VK_CTRL, false)]
        );
        // Ctrl is already held (Linux): send only C; Ctrl was synced on its own.
        let mut app = app_rec(false);
        app.mods = (true, false, false);
        app.send_ctrl_key(VK_X);
        assert_eq!(keys(&sent(&app)), vec![(VK_X, true), (VK_X, false)]);
    }

    #[test]
    fn agent_paste_sends_clip_then_ctrl_v() {
        let mut app = app_rec(true);
        app.mods = (true, false, false);
        app.paste_text("xin chào".into());
        let v = sent(&app);
        assert_eq!(v[0].t, "clip");
        assert_eq!(v[0].s, "xin chào");
        assert_eq!(keys(&v[1..]), vec![(VK_V, true), (VK_V, false)]);
    }

    #[test]
    fn uv_paste_is_ctrl_v_unless_type_mode() {
        let mut app = app_rec(false);
        app.mods = (true, false, false);
        app.paste_text("ab".into());
        let v = sent(&app);
        assert!(v.iter().all(|c| c.t != "clip"), "UltraViewer has no clip command");
        assert_eq!(keys(&v), vec![(VK_V, true), (VK_V, false)]);

        let mut app = app_rec(false);
        app.paste_type = true;
        app.paste_text("a\r\nb\u{1EC7}\u{1F600}".into());
        let text: Vec<String> = sent(&app).iter().filter(|c| c.k == "char").map(|c| c.c.clone()).collect();
        assert_eq!(
            text,
            vec!["a", "\r", "b", "\u{1EC7}"],
            "newline becomes Enter, accented letters are typed, emoji is dropped"
        );
        assert!(app.status.contains("bỏ 1"), "must report the dropped char: {}", app.status);
    }

    fn ptr_evt(button: egui::PointerButton, pressed: bool, x: f32, y: f32) -> egui::Event {
        egui::Event::PointerButton {
            pos: egui::pos2(x, y),
            button,
            pressed,
            modifiers: egui::Modifiers::default(),
        }
    }

    #[test]
    fn fast_click_in_one_frame_sends_both() {
        // Old bug (found on a real session): reading button state each frame dropped a
        // press+release in the same frame.
        use egui::PointerButton::*;
        let area = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(100.0, 100.0));
        let mut held = [false; 3];
        let ev = [ptr_evt(Primary, true, 10.0, 10.0), ptr_evt(Primary, false, 10.0, 10.0)];
        let cmds = ptr_cmds(&ev, true, area, &mut held);
        assert_eq!(cmds.iter().map(|l| (l.0, l.1)).collect::<Vec<_>>(), vec![(0, true), (0, false)]);
        assert_eq!(held, [false; 3]);

        // Press outside the picture / while a menu covers it: ignore, and ignore the matching release.
        let ev = [ptr_evt(Secondary, true, 150.0, 10.0), ptr_evt(Secondary, false, 10.0, 10.0)];
        assert!(ptr_cmds(&ev, true, area, &mut held).is_empty());
        let ev = [ptr_evt(Primary, true, 10.0, 10.0)];
        assert!(ptr_cmds(&ev, false, area, &mut held).is_empty());

        // Already held, then dragged outside: still send the release so the peer does not stick the button.
        let ev = [ptr_evt(Middle, true, 10.0, 10.0)];
        assert_eq!(ptr_cmds(&ev, true, area, &mut held).len(), 1);
        let ev = [ptr_evt(Middle, false, 500.0, 500.0)];
        assert_eq!(ptr_cmds(&ev, false, area, &mut held), vec![(1, false, egui::pos2(500.0, 500.0))]);
    }

    /// Run `forward_keys` in a real egui frame with the given events.
    fn run_keys(app: &mut App, events: Vec<egui::Event>, modifiers: egui::Modifiers) {
        let ctx = egui::Context::default();
        let raw = egui::RawInput { events, modifiers, ..Default::default() };
        let _ = ctx.run(raw, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| app.forward_keys(ui));
        });
    }

    fn up_v(m: egui::Modifiers) -> egui::Event {
        egui::Event::Key { key: egui::Key::V, physical_key: None, pressed: false, repeat: false, modifiers: m }
    }

    #[test]
    fn empty_clip_ctrl_v_still_reaches_peer() {
        // egui emits Paste only when the clipboard has text; an empty clipboard leaves only the V release.
        let ctrl = egui::Modifiers { ctrl: true, command: true, ..Default::default() };
        let mut app = app_rec(false);
        run_keys(&mut app, vec![up_v(ctrl)], ctrl);
        let v: Vec<_> = keys(&sent(&app)).into_iter().filter(|p| p.0 == VK_V).collect();
        assert_eq!(v, vec![(VK_V, true), (VK_V, false)]);

        // With Paste: send one pair; the later release is not sent again.
        let mut app = app_rec(false);
        run_keys(&mut app, vec![egui::Event::Paste("x".into()), up_v(ctrl)], ctrl);
        let v: Vec<_> = keys(&sent(&app)).into_iter().filter(|p| p.0 == VK_V).collect();
        assert_eq!(v, vec![(VK_V, true), (VK_V, false)]);
        assert!(!app.did_paste);
    }

    #[test]
    fn space_sent_once() {
        // Old bug: Space sent both key code 0x20 and the character " " -> the peer saw two spaces.
        let space = |pressed| egui::Event::Key {
            key: egui::Key::Space,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: egui::Modifiers::default(),
        };
        let mut app = app_rec(false);
        run_keys(
            &mut app,
            vec![space(true), egui::Event::Text(" ".into()), space(false)],
            egui::Modifiers::default(),
        );
        let v = sent(&app);
        assert_eq!(v.iter().filter(|c| c.k == "char" && c.c == " ").count(), 1);
        assert!(keys(&v).is_empty(), "plain Space must not send a vk: {:?}", keys(&v));
        // With Ctrl, still send the key code (Ctrl+Space).
        assert_eq!(vk_letter(egui::Key::Space), Some(0x20));
    }

    fn key_v(pressed: bool, m: egui::Modifiers) -> egui::Event {
        egui::Event::Key { key: egui::Key::V, physical_key: None, pressed, repeat: false, modifiers: m }
    }

    #[test]
    fn type_paste_releases_ctrl_first() {
        // Real session: Ctrl-down was already sent, and 21 typed chars came out as Ctrl+letter, showing nothing.
        let mut app = app_rec(false);
        app.paste_type = true;
        app.mods = (true, false, false);
        app.paste_text("ab".into());
        let v = sent(&app);
        let order: Vec<String> = v
            .iter()
            .filter(|c| c.t == "key")
            .map(|c| if c.k == "char" { c.c.clone() } else { format!("{}{}", c.v, if c.down { "↓" } else { "↑" }) })
            .collect();
        assert_eq!(order, vec!["17↑", "a", "b", "17↓"]);
    }

    #[test]
    fn fast_ctrl_v_release_still_sent() {
        // Real session: the V release arrived with Modifiers::NONE because Ctrl was released first.
        let mut app = app_rec(false);
        run_keys(&mut app, vec![key_v(false, egui::Modifiers::NONE)], egui::Modifiers::NONE);
        let v: Vec<_> = keys(&sent(&app)).into_iter().filter(|p| p.0 == VK_V).collect();
        assert_eq!(v, vec![(VK_V, true), (VK_V, false)]);
    }

    #[test]
    fn plain_v_is_not_ctrl_v() {
        // Plain "v": both press and release are seen -> do not send Ctrl+V, do not send a key code.
        let mut app = app_rec(false);
        let m = egui::Modifiers::NONE;
        run_keys(&mut app, vec![key_v(true, m), egui::Event::Text("v".into()), key_v(false, m)], m);
        let v = sent(&app);
        assert!(keys(&v).is_empty(), "no vk keys: {:?}", keys(&v));
        assert_eq!(v.iter().filter(|c| c.k == "char").count(), 1);
    }

    #[test]
    fn ctrl_c_release_not_resent() {
        // Copy already sent Ctrl+C; the later C release (no press seen) is skipped.
        let ctrl = egui::Modifiers { ctrl: true, command: true, ..Default::default() };
        let up_c = egui::Event::Key { key: egui::Key::C, physical_key: None, pressed: false, repeat: false, modifiers: ctrl };
        let mut app = app_rec(false);
        run_keys(&mut app, vec![egui::Event::Copy, up_c], ctrl);
        let c: Vec<_> = keys(&sent(&app)).into_iter().filter(|p| p.0 == VK_C).collect();
        assert_eq!(c, vec![(VK_C, true), (VK_C, false)]);
    }

    #[test]
    fn combo_down_forward_up_reverse() {
        let mut app = app_rec(false);
        app.send_combo("Ctrl+Shift+Esc", &[VK_CTRL, VK_SHIFT, VK_ESC]);
        assert_eq!(
            keys(&sent(&app)),
            vec![
                (VK_CTRL, true),
                (VK_SHIFT, true),
                (VK_ESC, true),
                (VK_ESC, false),
                (VK_SHIFT, false),
                (VK_CTRL, false)
            ]
        );
    }

    #[test]
    fn reconnect_resends_changed_settings() {
        let mut app = app_rec(false);
        let ctx = egui::Context::default();
        // Nothing changed: send no settings.
        app.on_event(&ctx, uvproto::Event { t: "hello".into(), ..Default::default() });
        assert!(sent(&app).is_empty());

        app.quality = 85;
        app.quality_sent = true;
        app.hide_wall = true;
        app.on_event(&ctx, uvproto::Event { t: "hello".into(), ..Default::default() });
        let v = sent(&app);
        assert!(v.iter().any(|c| c.t == "quality" && c.v == 85));
        assert!(v.iter().any(|c| c.t == "wallpaper" && c.v == 0));
    }

    #[test]
    fn matching_pong_sets_latency() {
        let mut app = app_rec(true);
        app.ping_at = std::time::Instant::now() - std::time::Duration::from_secs(3);
        app.ping_tick();
        let ping = sent(&app).into_iter().find(|c| c.t == "ping").expect("must send a ping");
        let ctx = egui::Context::default();
        app.on_event(&ctx, uvproto::Event { t: "pong".into(), w: ping.v as u32 + 1, ..Default::default() });
        assert!(app.lag_ms.is_none(), "a pong with the wrong id is ignored");
        app.on_event(&ctx, uvproto::Event { t: "pong".into(), w: ping.v as u32, ..Default::default() });
        assert!(app.lag_ms.is_some());
        // An UltraViewer session has no ping.
        let mut uv = app_rec(false);
        uv.ping_at = std::time::Instant::now() - std::time::Duration::from_secs(3);
        uv.ping_tick();
        assert!(sent(&uv).is_empty());
    }

    #[test]
    fn one_monitor_maps_onto_full_image() {
        let mut app = App::new(None);
        app.monitors = vec![
            uvsession::Monitor { x: 0, y: 0, w: 1920, h: 1080 },
            uvsession::Monitor { x: 1920, y: 0, w: 1280, h: 1024 },
        ];
        assert_eq!(app.view_rect(3200, 1080), (0.0, 0.0, 3200.0, 1080.0));
        app.mon_ix = Some(1);
        let src = app.view_rect(3200, 1080);
        assert_eq!(src, (1920.0, 0.0, 1280.0, 1024.0));
        // Middle of monitor 2's draw rect -> middle of monitor 2 on the whole image.
        let draw = egui::Rect::from_min_size(egui::pos2(10.0, 20.0), egui::vec2(640.0, 512.0));
        let (nx, ny) = to_full(draw.center(), draw, src, (3200.0, 1080.0));
        assert!((nx - 2560.0 / 3200.0).abs() < 1e-4, "nx={nx}");
        assert!((ny - 512.0 / 1080.0).abs() < 1e-4, "ny={ny}");
        // A monitor outside the canvas (layout mismatch) -> fall back to the whole image.
        assert_eq!(app.view_rect(1920, 1080), (0.0, 0.0, 1920.0, 1080.0));
    }

    #[test]
    fn disconnect_clears_layout_and_rights() {
        let mut app = app_rec(false);
        app.monitors = vec![uvsession::Monitor { x: 0, y: 0, w: 1920, h: 1080 }; 2];
        app.mon_ix = Some(1);
        app.view_only = Some(true);
        app.disconnect(&egui::Context::default());
        assert!(app.monitors.is_empty() && app.mon_ix.is_none() && app.view_only.is_none());
    }

    #[test]
    fn wheel_splits_notches_keeps_fraction() {
        let mut rem = 0.0;
        assert_eq!(wheel_notches(&mut rem, 40.0, 40.0), 1);
        assert_eq!(wheel_notches(&mut rem, -40.0, 40.0), -1);
        assert_eq!(wheel_notches(&mut rem, 100.0, 40.0), 2);
        assert!((rem - 20.0).abs() < 1e-4);
        // Touchpad: tiny steps still add up to a full notch.
        let mut rem = 0.0;
        let notches: i32 = (0..8).map(|_| wheel_notches(&mut rem, 15.0, 40.0)).sum();
        assert_eq!(notches, 3);
        // Reversing direction eats the fraction; it does not jump a notch the other way.
        let mut rem = 30.0;
        assert_eq!(wheel_notches(&mut rem, -20.0, 40.0), 0);
        // Step 0 (bad config) does not divide by zero.
        let mut rem = 0.0;
        assert_eq!(wheel_notches(&mut rem, 80.0, 0.0), 2);
    }
}
