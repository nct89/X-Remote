//! **Controlled** (host) mode, running in the background of the UI: register with
//! the UltraViewer servers, show the assigned ID, then serve a controller that dials in.
//!
//! Linux only for the full path (needs X11 to capture and inject input). macOS uses
//! CoreGraphics. On other platforms `start` reports "not supported".

use std::sync::{Arc, Mutex};

use eframe::egui;

/// Host state shared with the UI thread.
#[derive(Default)]
pub struct HostState {
    /// ID assigned by the server (the address a controller dials). `None` until registered.
    pub granted_id: Option<String>,
    /// Status line shown under the ID.
    pub status: String,
    /// A controller is in the session.
    pub serving: bool,
    /// Chat history of the current session (and earlier ones in this run).
    pub chat: Vec<ChatLine>,
    /// Unread message → the UI opens the chat panel.
    pub chat_unread: bool,
    /// Controller machine name (field 11 of the session login), used as the sender.
    pub peer_name: String,
    /// Controller is typing (between `29` and `30`).
    pub peer_typing: bool,
    /// Warning the user must act on (macOS: missing Screen Recording / Accessibility).
    pub alert: Option<String>,
}

/// One chat line.
#[derive(Clone, Debug)]
pub struct ChatLine {
    /// `true` = sent by the user of this machine.
    pub mine: bool,
    pub clock: String,
    pub body: String,
    /// Folder links in the message (label shown, path) — click to open.
    pub dirs: Vec<(String, std::path::PathBuf)>,
}

pub type HostShare = Arc<Mutex<HostState>>;

/// Whether this machine can run controlled mode (Linux: X11; macOS: CoreGraphics —
/// needs Screen Recording + Accessibility).
pub const HOST_OK: bool = cfg!(any(target_os = "linux", target_os = "macos"));

/// Chat the host user typed, waiting for the session loop to send.
fn outbox() -> &'static Mutex<Vec<uvsession::chat::ChatOut>> {
    static S: std::sync::OnceLock<Mutex<Vec<uvsession::chat::ChatOut>>> = std::sync::OnceLock::new();
    S.get_or_init(|| Mutex::new(Vec::new()))
}

/// `XREMOTE_CHAT_DEMO=1`: load a sample chat so the panel can be screenshotted
/// without a real controller. No effect on a normal run.
pub fn load_demo_chat(tt: &HostShare) {
    let mut g = tt.lock().unwrap();
    g.serving = true;
    g.peer_name = "HOST-PC".into();
    g.peer_typing = true;
    g.chat_unread = true;
    let d = |mine: bool, clock: &str, s: &str| ChatLine { mine, clock: clock.into(), body: s.into(), dirs: Vec::new() };
    g.chat = vec![
        d(false, "20:43", "alo"),
        d(false, "20:43", "xin chào, bạn thấy màn hình của tôi chưa?"),
        d(true, "20:43", "chào bạn"),
        d(true, "20:44", "Mình thấy rồi, chuột và bàn phím đều chạy. Bạn cần hỗ trợ phần nào?"),
        d(false, "20:45", "Cài giúp mình bộ gõ tiếng Việt nhé — cảm ơn!"),
    ];
}

/// UI reports the user started (`true`) / stopped (`false`) typing, so the controller
/// shows "typing" like UltraViewer. No session → ignore.
pub fn set_typing(tt: &HostShare, typing: bool) {
    if tt.lock().unwrap().serving {
        outbox().lock().unwrap().push(uvsession::chat::ChatOut::Typing(typing));
    }
}

/// UI calls this when the user sends a chat line. Returns `false` if there is no session.
#[allow(dead_code)]
pub fn send_chat(tt: &HostShare, body: &str) -> bool {
    send_chat_links(tt, body, body, Vec::new())
}

/// Send a line: `shown` is stored in this machine's history (with folder links),
/// `wire` is the text sent to the controller (folder labels already replaced by full paths).
pub fn send_chat_links(
    tt: &HostShare,
    shown: &str,
    wire: &str,
    dirs: Vec<(String, std::path::PathBuf)>,
) -> bool {
    let (shown, wire) = (shown.trim(), wire.trim());
    if shown.is_empty() {
        return false;
    }
    let mut g = tt.lock().unwrap();
    if !g.serving {
        return false;
    }
    g.chat.push(ChatLine { mine: true, clock: hhmm(), body: shown.to_string(), dirs });
    outbox().lock().unwrap().push(uvsession::chat::ChatOut::Msg(wire.to_string()));
    true
}

/// Local hour:minute for a chat line.
pub fn hhmm() -> String {
    let giay = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    #[cfg(unix)]
    {
        let t: libc::time_t = giay as libc::time_t;
        let mut tm: libc::tm = unsafe { std::mem::zeroed() };
        if !unsafe { libc::localtime_r(&t, &mut tm) }.is_null() {
            return format!("{:02}:{:02}", tm.tm_hour, tm.tm_min);
        }
    }
    format!("{:02}:{:02}", (giay / 3600) % 24, (giay / 60) % 60)
}

/// Local timestamp `YYYYMMDD-HHMMSS` for file names.
pub fn stamp() -> String {
    let giay = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    #[cfg(unix)]
    {
        let t: libc::time_t = giay as libc::time_t;
        let mut tm: libc::tm = unsafe { std::mem::zeroed() };
        if !unsafe { libc::localtime_r(&t, &mut tm) }.is_null() {
            return format!(
                "{:04}{:02}{:02}-{:02}{:02}{:02}",
                tm.tm_year + 1900,
                tm.tm_mon + 1,
                tm.tm_mday,
                tm.tm_hour,
                tm.tm_min,
                tm.tm_sec
            );
        }
    }
    format!("{giay}")
}

/// Controller machine name: field 11 of `login:<sid>:<hash>:<w>:<h>:…:<id>:<host name>:…`.
#[cfg_attr(not(any(target_os = "linux", target_os = "macos")), allow(dead_code))] // host mode only (Linux, macOS)
pub fn name_from_login(login: &str) -> String {
    login.split(':').nth(11).filter(|s| !s.is_empty()).unwrap_or("Đối tác").to_string()
}

/// Chat channel between the session loop and the UI state.
#[cfg(any(target_os = "linux", target_os = "macos"))]
struct UiChat {
    tt: HostShare,
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
impl uvsession::chat::ChatChannel for UiChat {
    fn on_in(&mut self, event: uvsession::chat::ChatEvent) {
        use uvsession::chat::ChatEvent;
        let mut g = self.tt.lock().unwrap();
        match event {
            ChatEvent::Msg(body) => {
                g.chat.push(ChatLine { mine: false, clock: hhmm(), body, dirs: Vec::new() });
                g.chat_unread = true;
                g.peer_typing = false;
            }
            // `29`/`30` = start/stop typing (VB6 strings "showtyping"/"hidetyping").
            ChatEvent::Ctrl(s) if s == "29" => g.peer_typing = true,
            ChatEvent::Ctrl(s) if s == "30" => g.peer_typing = false,
            ChatEvent::Ctrl(_) => return,
        }
        drop(g);
        if let Some(ctx) = ui_ctx().lock().unwrap().as_ref() {
            ctx.request_repaint();
        }
    }
    fn poll_out(&mut self) -> Option<uvsession::chat::ChatOut> {
        let mut h = outbox().lock().unwrap();
        (!h.is_empty()).then(|| h.remove(0))
    }
}

/// Wake the UI. A session ends on the host thread; without a repaint the
/// window sits still, so the chat panel and "Connecting" line stay up until
/// the next mouse move.
fn wake_ui() {
    if let Some(ctx) = ui_ctx().lock().unwrap().as_ref() {
        ctx.request_repaint();
    }
}

/// `egui::Context` so the host thread can wake the UI when chat arrives.
fn ui_ctx() -> &'static Mutex<Option<egui::Context>> {
    static S: std::sync::OnceLock<Mutex<Option<egui::Context>>> = std::sync::OnceLock::new();
    S.get_or_init(|| Mutex::new(None))
}

/// UI registers its context once at startup.
pub fn set_ctx(ctx: &egui::Context) {
    let mut g = ui_ctx().lock().unwrap();
    if g.is_none() {
        *g = Some(ctx.clone());
    }
}

/// New password requested by the UI (change password → register again).
fn pw_slot() -> &'static Mutex<Option<String>> {
    static S: std::sync::OnceLock<Mutex<Option<String>>> = std::sync::OnceLock::new();
    S.get_or_init(|| Mutex::new(None))
}

/// UI calls this when the user presses the change-password button.
pub fn set_password(pw: String) {
    *pw_slot().lock().unwrap() = Some(pw);
}

/// Saved host id (`~/.config/x-remote/host_id`). Shown at once in "Your ID",
/// without waiting for the server — otherwise every launch sticks on
/// "— đang đăng ký —".
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn saved_id_shown() -> Option<String> {
    let s = uvsession::host::saved_host_id();
    (!s.is_empty()).then_some(s)
}

/// Start the background host with `password`. Returns the state handle the UI reads.
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub fn start(password: String) -> HostShare {
    let saved_id = saved_id_shown();
    // One machine may have only ONE host process: every login overwrites the
    // password of the same fixed ID, so a second copy shows a password the
    // server is no longer holding.
    if let Some(p) = lock_path() {
        let running = std::fs::read_to_string(&p)
            .ok()
            .and_then(|s| s.trim().parse::<u32>().ok())
            .filter(|pid| *pid != std::process::id() && child_alive(*pid));
        if let Some(pid) = running {
            return Arc::new(Mutex::new(HostState {
                granted_id: saved_id,
                status: format!("Already running in another process (PID {pid})"),
                ..Default::default()
            }));
        }
        if let Some(d) = p.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let _ = std::fs::write(&p, std::process::id().to_string());
    }

    let tt: HostShare = Arc::new(Mutex::new(HostState {
        granted_id: saved_id,
        status: "Registering…".into(),
        ..Default::default()
    }));
    let tt2 = tt.clone();
    std::thread::Builder::new()
        .name("uvhost".into())
        .spawn(move || host_loop(password, tt2))
        .ok();
    tt
}

/// Is process `pid` still alive (macOS has no `/proc`: probe with signal 0).
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn child_alive(pid: u32) -> bool {
    if cfg!(target_os = "linux") {
        return std::path::Path::new(&format!("/proc/{pid}")).exists();
    }
    let r = unsafe { libc::kill(pid as libc::pid_t, 0) };
    r == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

/// Single-process lock file, next to `host_id`.
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn lock_path() -> Option<std::path::PathBuf> {
    uvsession::host::host_id_path().map(|p| p.with_file_name("host.lock"))
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub fn start(_password: String) -> HostShare {
    Arc::new(Mutex::new(HostState {
        status: "Accepting control is only supported on Linux".into(),
        ..Default::default()
    }))
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn host_loop(password: String, tt: HostShare) {
    let mut password = password;
    use std::time::Duration;
    use uvsession::capture::Capturer;
    use uvsession::inject::InputInjector;
    use uvsession::HostControl;

    let set_status = |s: &str| {
        log_line(s);
        tt.lock().unwrap().status = s.to_string();
        wake_ui();
    };

    // macOS: without permission the system captures only the wallpaper (no windows) and
    // silently drops injected mouse/keys — request both as soon as host starts (dialog once).
    #[cfg(target_os = "macos")]
    {
        let rec = uvsession::capture::request_screen_record();
        let ax = uvsession::inject::request_accessibility();
        log_line(&format!("macOS permissions: Screen Recording={rec}, Accessibility={ax}"));
        if !rec || !ax {
            let mut missing = Vec::new();
            if !rec {
                missing.push("Screen Recording");
            }
            if !ax {
                missing.push("Accessibility");
            }
            tt.lock().unwrap().alert = Some(format!(
                "Grant {} to X-Remote in System Settings → Privacy & Security, then reopen the app.",
                missing.join(" and ")
            ));
        }
    }

    // Capture + input injection open once and are reused for every session.
    let cap = match Capturer::new() {
        Ok(m) => m,
        Err(e) => {
            set_status(&format!("Could not open the screen: {e}"));
            return;
        }
    };
    let inj = InputInjector::new().ok();
    log_line(&format!("screen {}x{}", cap.width, cap.height));
    // Back off on errors so login is not hammered. Transient network: 5 → 10 → … → 300 seconds.
    // Command `60:` is captcha (enter_captcha.aspx), not silence. macOS/Linux opens a window
    // for the user to type the code, then resubmits at once. Closing the window, or two
    // tries used up, waits 10 → 30 minutes so the challenge does not stay hot.
    let mut retry_s: u64 = 5;
    let mut id_blocked = false;
    let mut token_captcha = String::new();
    let mut captcha_wins: u32 = 0;

    loop {
        // UI asked for a new password → register again with it.
        if let Some(p) = pw_slot().lock().unwrap().take() {
            password = p;
            log_line(&format!("password change ({} chars) -> re-register", password.len()));
        }
        let lan = uvsession::client::local_ip().map(|i| i.to_string()).unwrap_or_else(|| "0.0.0.0".into());
        let pref = uvsession::host::saved_host_id();
        // Bind the listen port and KEEP it: the server publishes `LAN_IP|port` to the
        // controller, which dials straight here (measured: an ID lookup returned
        // `198.51.100.14|33435`).
        let listener = std::net::TcpListener::bind(("0.0.0.0", 0)).ok();
        let listen_port = listener
            .as_ref()
            .and_then(|l| l.local_addr().ok())
            .map(|a| a.port())
            .unwrap_or(0);
        if let Some(l) = &listener {
            let _ = l.set_nonblocking(true);
        }
        let token = std::mem::take(&mut token_captcha);
        match HostControl::register(&pref, &password, &lan, listen_port, &token, Duration::from_secs(15)) {
            Ok(mut h) => {
                {
                    let mut g = tt.lock().unwrap();
                    g.granted_id = Some(h.assigned_id.clone());
                    g.status = "Ready — waiting for a connection".into();
                }
                // A saved id is shown from the first frame. The server returns
                // the id (or a new one) on this thread — wake the UI, or the
                // field stays on "— đang đăng ký —".
                wake_ui();
                log_line(&format!(
                    "registered: ID {} (requested {}) password {} chars, LAN {lan}, P2P port {}",
                    h.assigned_id,
                    if pref.is_empty() { "0" } else { &pref },
                    password.len(),
                    h.p2p_port
                ));
                // The host must LISTEN on the port the server assigned: the controller connects
                // straight to `LAN_IP:port` (docs/03 §19). No listen → the controller times out.
                log_line(&format!("listen 0.0.0.0:{listen_port} + advertised in login"));
                retry_s = 5;
                id_blocked = false;
                // Wait for a controller to dial.
                loop {
                    if pw_slot().lock().unwrap().is_some() {
                        break;
                    }
                    match h.poll(Duration::from_secs(2)) {
                        Ok(Some(cmd17)) => {
                            log_line(&format!(
                                "COMMAND 17: sid={} token={} relay={:?} hosts={:?} lan={:?} pub={:?}",
                                cmd17.session_id,
                                cmd17.token,
                                cmd17.relay_ips,
                                cmd17.relay_hosts,
                                cmd17.controller_lan,
                                cmd17.controller_pub
                            ));
                            {
                                let mut g = tt.lock().unwrap();
                                g.serving = true;
                                g.status = "Handshaking with the controller…".into();
                            }
                            wake_ui();
                            // EMPTY cmd17 (no controller address and no session password):
                            // the server is still holding an old session. Do not answer an
                            // empty packet (it can break the VB6 parser); register again.
                            if cmd17.session_pw.is_empty()
                                || (cmd17.controller_lan.is_none() && cmd17.controller_pub.is_none())
                            {
                                log_line("empty cmd17 — re-register to reset the session");
                                break;
                            }
                            // 1) Answer command 17 on the control channel so the server
                            //    hands our address to the controller.
                            let pub_ip = h.public_ip.clone();
                            match cmd17.reply_17(h.control_mut(), &pub_ip, listen_port, &lan, listen_port) {
                                Ok(s) => log_line(&format!("reply 17 (control): {s}")),
                                Err(e) => log_line(&format!("reply 17 failed: {e}")),
                            }
                            // 2) Reach the controller: wait for it to dial our listen port (a real
                            //    UltraViewer controller does that) AND at the same time dial the LAN
                            //    address it published (an X-Remote controller waits for the host to
                            //    call back). Dialing only after 25 s of wait + 40 s of relay used to
                            //    miss it — the X-Remote controller had already closed the port, so
                            //    Mac → Linux failed (2026-11-06).
                            let sock = listener
                                .as_ref()
                                .and_then(|l| wait_accept(l, cmd17.controller_lan.clone(), 25));
                            let kq = match sock {
                                Some((s, cach)) => {
                                    log_line(&format!("controller connected ({cach})"));
                                    cmd17.from_socket(s, Duration::from_secs(15))
                                }
                                None => {
                                    log_line("nobody dialed — trying relay");
                                    cmd17.connect_controller(h.control_mut(), 0, Duration::from_secs(6))
                                }
                            };
                            match kq {
                                Ok(mut hs) => {
                                    log_line(&format!("SESSION CHANNEL OPEN: {}", hs.peer));
                                    log_line(&format!("session login: {}", hs.controller_login));
                                    {
                                        let mut g = tt.lock().unwrap();
                                        g.status = "Being controlled".into();
                                        g.peer_name = name_from_login(&hs.controller_login);
                                    }
                                    wake_ui();
                                    outbox().lock().unwrap().clear();
                                    let mut chat = UiChat { tt: tt.clone() };
                                    let kq = uvsession::hostrun::serve_session_chat(
                                        &mut hs, &cap, inj.as_ref(), || false, |_| {}, &mut chat,
                                    );
                                    // The peer (Mac/Linux) can close mid-session and leave a
                                    // modifier or mouse button down. Release them, or this
                                    // machine cannot type into the id and password fields.
                                    if let Some(b) = inj.as_ref() {
                                        uvsession::inject::release_all(b);
                                    }
                                    log_line(&format!("session ended: {kq:?}"));
                                }
                                Err(e) => {
                                    log_line(&format!("CONTROLLER CONNECT FAILED: {e}"));
                                    set_status(&format!("Controller connect failed: {e}"));
                                }
                            }
                            {
                                let mut g = tt.lock().unwrap();
                                g.serving = false;
                                g.peer_typing = false;
                                if !g.status.starts_with("Controller connect failed") {
                                    g.status = "Ready — waiting for a connection".into();
                                }
                            }
                            // Required: the session finished on this thread. Without a
                            // wake, the chat viewport and "Connecting" stay on screen.
                            wake_ui();
                        }
                        Ok(None) => {}
                        Err(_) => break, // control channel dropped → register again
                    }
                }
            }
            Err(e) => {
                if let Some((esc, extra)) = uvsession::host::read_captcha_err(&e) {
                    log_line(&format!("server wants captcha, esc {} chars", esc.len()));
                    if cfg!(any(target_os = "macos", target_os = "linux")) && captcha_wins < 2 {
                        let url = uvsession::host::url_captcha(&esc, &extra);
                        set_status("Enter the code in the window that opened");
                        captcha_wins += 1;
                        match uvsession::host::open_captcha(&url) {
                            Ok(Some(tok)) => {
                                log_line("got confirmation code — resending registration");
                                set_status("Sending the confirmation code…");
                                token_captcha = tok;
                                continue;
                            }
                            Ok(None) => {
                                id_blocked = true;
                                if retry_s < 600 {
                                    retry_s = 600;
                                }
                                let mins = retry_s.div_ceil(60);
                                set_status(&format!("Code not entered — retrying in {mins} min"));
                                log_line(&format!("captcha closed with no code — retry in {retry_s}s"));
                            }
                            Err(loi) => {
                                id_blocked = true;
                                if retry_s < 600 {
                                    retry_s = 600;
                                }
                                let mins = retry_s.div_ceil(60);
                                log_line(&format!("could not open captcha: {loi}"));
                                set_status(&format!(
                                    "Could not open the confirmation window — retrying in {mins} min"
                                ));
                            }
                        }
                    } else {
                        id_blocked = true;
                        if retry_s < 600 {
                            retry_s = 600;
                        }
                        let mins = retry_s.div_ceil(60);
                        log_line(&format!(
                            "captcha — wait {retry_s}s (opened {captcha_wins} windows)"
                        ));
                        set_status(&format!("Server requires a confirmation code — retrying in {mins} min"));
                    }
                } else {
                    // A leftover `(60)` is captcha whose session code could not be read.
                    // Real silence backs off on the short ladder.
                    id_blocked = e.contains("(60)") || e.contains("chặn cấp ID");
                    if id_blocked && retry_s < 600 {
                        retry_s = 600;
                    }
                    log_line(&format!("register failed: {e} — retry in {retry_s}s"));
                    if id_blocked {
                        set_status(&format!(
                            "Server is not issuing a new ID yet — retrying in {} min",
                            retry_s.div_ceil(60)
                        ));
                    } else {
                        set_status("Reconnecting to the server…");
                    }
                }
            }
        }
        std::thread::sleep(Duration::from_secs(retry_s));
        captcha_wins = 0;
        // Hammering retries only makes the server keep the challenge longer. Unsolved
        // captcha: stretch up to 30 minutes; other errors (network/silence): 5 → 10 → … → 5 minutes.
        retry_s = if id_blocked {
            (retry_s * 2).min(1800)
        } else {
            (retry_s * 2).min(300)
        };
    }
}

/// Wait for the controller to connect to the listen port, and every 1.5 s also try
/// dialing `dial` (the LAN address it published in command 17). Returns the socket
/// plus how the connection was made.
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn wait_accept(
    l: &std::net::TcpListener,
    dial: Option<(String, u16)>,
    secs: u64,
) -> Option<(std::net::TcpStream, &'static str)> {
    use std::time::{Duration, Instant};
    let _ = l.set_nonblocking(true);
    let dest: Option<std::net::SocketAddr> = dial.and_then(|(ip, port)| format!("{ip}:{port}").parse().ok());
    let deadline = Instant::now() + Duration::from_secs(secs);
    let mut next_dial = Instant::now();
    while Instant::now() < deadline {
        match l.accept() {
            Ok((s, _)) => {
                let _ = s.set_nonblocking(false);
                return Some((s, "controller dialed the listen port"));
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(_) => return None,
        }
        if let Some(d) = dest {
            if Instant::now() >= next_dial {
                next_dial = Instant::now() + Duration::from_millis(1500);
                if let Ok(s) = std::net::TcpStream::connect_timeout(&d, Duration::from_millis(700)) {
                    return Some((s, "dialed the controller directly"));
                }
            }
        }
        std::thread::sleep(Duration::from_millis(60));
    }
    None
}

/// Append a diagnostic line to `~/.config/x-remote/host.log` (Windows: `%APPDATA%`).
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub fn log_line(s: &str) {
    use std::io::Write;
    let Some(p) = log_path() else { return };
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let moc = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(p) {
        let _ = writeln!(f, "[{moc}] {s}");
    }
}

/// Path of the host log file.
#[cfg_attr(not(any(target_os = "linux", target_os = "macos")), allow(dead_code))] // host mode only (Linux, macOS)
pub fn log_path() -> Option<std::path::PathBuf> {
    let env = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(std::path::PathBuf::from);
    if cfg!(windows) {
        return env("APPDATA").map(|d| d.join("X-Remote").join("host.log"));
    }
    env("XDG_CONFIG_HOME")
        .or_else(|| env("HOME").map(|h| h.join(".config")))
        .map(|d| d.join("x-remote").join("host.log"))
}
