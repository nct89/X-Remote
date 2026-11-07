//! **Controlled** side (host): register with the UltraViewer server, then wait
//! for `command 17` when a controller dials in.
//!
//! Checked on a real machine 05/11/2026 (see docs/03 §22):
//! 1. open TLS + `POST /UVService/` (same as the controller);
//! 2. send [`build_login_host`](crate::control::build_login_host) — **field 3 is
//!    the password**, not a port;
//! 3. the server **assigns an ID** in the reply `3:<port>:<assigned ID>:<public ip>…`;
//! 4. keep the connection alive with a periodic keepalive `2`;
//! 5. when a controller dials that **assigned ID**, the server pushes `command 17`.
//!
//! A random ID the client invents **cannot** receive a connection — the real
//! address is the ID assigned in step 3. The UI must show that ID.

use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::control::{build_login_host, Control};
use crate::crypto::frame_tdes;
use crate::session::{Chan, Session};
use crate::Ident;

/// Control connection registered as a host.
pub struct HostControl {
    ctl: Control,
    /// ID the server assigned to this host (the address a controller dials).
    pub assigned_id: String,
    /// Public IP as seen by the server.
    pub public_ip: String,
    /// P2P port the server assigned (field 2 of the login reply). The remote must
    /// **listen** on this port — the controller dials `LAN_IP:port` (docs/03 §19).
    pub p2p_port: u16,
    last_keepalive: Instant,
}

/// `command 17` — the server says "a controller wants to connect".
#[derive(Debug, Clone)]
pub struct Cmd17 {
    /// Session/request ID (checked during the relay handshake).
    pub session_id: String,
    /// Session token (hex).
    pub token: String,
    /// P2P relay IPs (split from the `;` list).
    pub relay_ips: Vec<String>,
    /// Relay hostnames (`Host:` label when POSTing to a relay).
    pub relay_hosts: Vec<String>,
    /// Controller LAN address (direct connect on the same network).
    pub controller_lan: Option<(String, u16)>,
    /// Controller public address.
    pub controller_pub: Option<(String, u16)>,
    /// Session password (the controller generates it; the server forwards it as the last field).
    /// AES key for the session channel.
    pub session_pw: String,
    /// Every field left after the relay list (debug).
    pub tail: String,
    /// Raw message text (debug).
    pub raw: String,
}

/// File that stores the host ID the server assigned (so the next login asks for
/// that SAME id, like UltraViewer's PreferID): `$XDG_CONFIG_HOME/x-remote/host_id`
/// (default `~/.config`), or `%APPDATA%\X-Remote\host_id` on Windows.
pub fn host_id_path() -> Option<PathBuf> {
    let env = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
    if cfg!(windows) {
        return env("APPDATA").map(|d| d.join("X-Remote").join("host_id"));
    }
    env("XDG_CONFIG_HOME")
        .or_else(|| env("HOME").map(|h| h.join(".config")))
        .map(|d| d.join("x-remote").join("host_id"))
}

/// Read the saved host ID (empty if none).
pub fn saved_host_id() -> String {
    host_id_path()
        .and_then(|t| std::fs::read_to_string(t).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
        .unwrap_or_default()
}

/// Save the host ID the server just assigned, to request it again next time.
pub fn save_host_id(id: &str) {
    if let Some(t) = host_id_path() {
        if let Some(dir) = t.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(t, format!("{id}\n"));
    }
}

/// Characters allowed in a `:`-separated field. Empty is allowed.
fn field_before_colon(t: &str) -> bool {
    t.len() <= 4096 && t.bytes().all(|b| (0x20..0x7f).contains(&b) && b != b':')
}

/// Captcha token: printable ASCII, non-empty, no `:`.
pub fn captcha_ok(t: &str) -> bool {
    !t.is_empty() && field_before_colon(t)
}

/// Percent-encode (unreserved bytes stay; others become uppercase `%XX`).
fn pct(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            o.push(b as char);
        } else {
            o.push_str(&format!("%{b:02X}"));
        }
    }
    o
}

/// Official captcha page. `esc`/`extra` come from a `60:` reply.
pub fn url_captcha(esc: &str, extra: &str) -> String {
    format!(
        "https://www.ultraviewer.net/api/enter_captcha.aspx?lang=vi&version={}&extra={}&esc={}",
        crate::VERSION,
        pct(extra),
        pct(esc)
    )
}

/// Split `60:<esc>:<extra>`. `extra` may be empty. `None` if this is not a captcha.
pub fn split_captcha(body: &str) -> Option<(String, String)> {
    let body = body.trim_end_matches(['\0', '\r', '\n', ' ']);
    if body.len() > 4096 {
        return None;
    }
    let mut it = body.split(':');
    if it.next()? != "60" {
        return None;
    }
    let esc = it.next()?.to_string();
    let extra = it.next().unwrap_or("").to_string();
    if !captcha_ok(&esc) || !field_before_colon(&extra) {
        return None;
    }
    Some((esc, extra))
}

/// Registration error when the server demands a captcha. `read_captcha_err` reads this pair back.
pub fn captcha_fail(esc: &str, extra: &str) -> String {
    format!("CAPTCHA\t{esc}\t{extra}")
}

/// Read an error built by [`captcha_fail`]. `None` if this is not a captcha error.
pub fn read_captcha_err(e: &str) -> Option<(String, String)> {
    let rest = e.strip_prefix("CAPTCHA\t")?;
    let mut it = rest.splitn(2, '\t');
    let esc = it.next()?.to_string();
    let extra = it.next().unwrap_or("").to_string();
    if !captcha_ok(&esc) || !field_before_colon(&extra) {
        return None;
    }
    Some((esc, extra))
}

#[cfg(target_os = "macos")]
const PLIST_CAPTCHA: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleExecutable</key><string>captcha_mac</string>
<key>CFBundleIdentifier</key><string>net.xremote.captcha</string>
<key>CFBundleName</key><string>Xac nhan UltraViewer</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleShortVersionString</key><string>1.0</string>
<key>CFBundleVersion</key><string>1</string>
<key>LSMinimumSystemVersion</key><string>11.0</string>
<key>NSHighResolutionCapable</key><true/>
</dict></plist>
"#;

/// Compile the WKWebView helper (once, when the source changes) and return the binary path.
#[cfg(target_os = "macos")]
pub fn prep_captcha_mac() -> Result<std::path::PathBuf, String> {
    let cfg = host_id_path()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .ok_or_else(|| "no config directory".to_string())?;
    std::fs::create_dir_all(&cfg).map_err(|e| format!("create config dir: {e}"))?;
    let src = cfg.join("captcha_mac.swift");
    let src_text = include_str!("../../../scripts/captcha_mac.swift");
    let app = cfg.join("Captcha.app");
    let bin = app.join("Contents/MacOS/captcha_mac");
    let prev = std::fs::read_to_string(&src).unwrap_or_default();
    if prev != src_text || !bin.is_file() {
        std::fs::write(&src, src_text).map_err(|e| format!("write captcha source: {e}"))?;
        if let Some(dir) = bin.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("create captcha bundle: {e}"))?;
        }
        std::fs::write(app.join("Contents/Info.plist"), PLIST_CAPTCHA)
            .map_err(|e| format!("write captcha Info.plist: {e}"))?;
        let out = std::process::Command::new("swiftc")
            .args(["-O", "-framework", "Cocoa", "-framework", "WebKit", "-o"])
            .arg(&bin)
            .arg(&src)
            .output()
            .map_err(|e| format!("swiftc failed to start: {e}"))?;
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr);
            let tail: String = err.chars().rev().take(600).collect::<String>().chars().rev().collect();
            return Err(format!("swiftc error: {tail}"));
        }
        let cs = std::process::Command::new("codesign")
            .args(["-s", "-", "--force"])
            .arg(&app)
            .status()
            .map_err(|e| format!("codesign failed to start: {e}"))?;
        if !cs.success() {
            return Err("captcha codesign failed".into());
        }
    }
    Ok(bin)
}

#[cfg(not(target_os = "macos"))]
pub fn prep_captcha_mac() -> Result<std::path::PathBuf, String> {
    Err("this machine cannot open a captcha window".into())
}

/// Write the Python/WebKit2GTK helper into the config dir (once, when the source
/// changes) and return its path. Needs `python3-gobject` + `webkit2gtk4.1`;
/// if those are missing, [`open_captcha`] reports the error.
#[cfg(target_os = "linux")]
pub fn prep_captcha_linux() -> Result<std::path::PathBuf, String> {
    let cfg = host_id_path()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .ok_or_else(|| "no config directory".to_string())?;
    std::fs::create_dir_all(&cfg).map_err(|e| format!("create config dir: {e}"))?;
    let script = cfg.join("captcha_linux.py");
    let src_text = include_str!("../../../scripts/captcha_linux.py");
    let prev = std::fs::read_to_string(&script).unwrap_or_default();
    if prev != src_text {
        std::fs::write(&script, src_text).map_err(|e| format!("write captcha helper: {e}"))?;
    }
    Ok(script)
}

/// Wait until the captcha helper prints a token on stdout (first line), up to 10 minutes.
/// `Ok(None)` if the window closes or time runs out with no token; `Err` if the helper exits with an error.
#[cfg(any(target_os = "macos", target_os = "linux"))]
fn wait_proc_token(mut child: std::process::Child) -> Result<Option<String>, String> {
    let deadline = Instant::now() + Duration::from_secs(600);
    loop {
        match child.try_wait() {
            Ok(Some(st)) => {
                let mut out = String::new();
                if let Some(mut p) = child.stdout.take() {
                    use std::io::Read;
                    let _ = p.read_to_string(&mut out);
                }
                let tok = out.lines().next().unwrap_or("").trim();
                if captcha_ok(tok) {
                    return Ok(Some(tok.to_string()));
                }
                if st.success() {
                    return Ok(None);
                }
                let mut err_out = String::new();
                if let Some(mut p) = child.stderr.take() {
                    use std::io::Read;
                    let _ = p.read_to_string(&mut err_out);
                }
                let tail: String = err_out.chars().rev().take(300).collect::<String>().chars().rev().collect();
                return Err(format!(
                    "captcha window exited {}: {tail}",
                    st.code().unwrap_or(-1)
                ));
            }
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Ok(None);
                }
                std::thread::sleep(Duration::from_millis(200));
            }
            Err(e) => return Err(format!("wait captcha window: {e}")),
        }
    }
}

/// Open the captcha page and wait for the user to enter the code (up to 10 minutes).
/// `Ok(None)` if the window closes or time runs out with no token.
#[cfg(target_os = "macos")]
pub fn open_captcha(url: &str) -> Result<Option<String>, String> {
    if !url.starts_with("https://www.ultraviewer.net/api/enter_captcha.aspx?") {
        return Err("captcha url is not the UltraViewer page".into());
    }
    let bin = prep_captcha_mac()?;
    let child = std::process::Command::new(&bin)
        .arg(url)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("cannot open captcha window: {e}"))?;
    wait_proc_token(child)
}

/// Linux: open the captcha page with the Python/WebKit2GTK helper (`captcha_linux.py`).
#[cfg(target_os = "linux")]
pub fn open_captcha(url: &str) -> Result<Option<String>, String> {
    if !url.starts_with("https://www.ultraviewer.net/api/enter_captcha.aspx?") {
        return Err("captcha url is not the UltraViewer page".into());
    }
    let script = prep_captcha_linux()?;
    let child = std::process::Command::new("python3")
        .arg(&script)
        .arg(url)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("cannot start python3 for captcha: {e}"))?;
    wait_proc_token(child)
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub fn open_captcha(_url: &str) -> Result<Option<String>, String> {
    Err("this machine cannot open a captcha window".into())
}

impl HostControl {
    /// Register as a host: login (field 3 = password), then read the assigned ID.
    ///
    /// `password` is the password the host wants to use (the controller must type it).
    /// `local_ip` is the host's LAN IP.
    pub fn register(
        preferred_id: &str,
        password: &str,
        local_ip: &str,
        listen_port: u16,
        captcha: &str,
        timeout: Duration,
    ) -> Result<HostControl, String> {
        let dt = Ident::current();
        let mut ctl = Control::connect(timeout).map_err(|e| format!("cannot connect to server: {e}"))?;
        ctl.http_post().map_err(|e| format!("HTTP POST failed: {e}"))?;
        // Send a previously assigned ID (if any) so the server returns that SAME id —
        // like UltraViewer PreferID. Otherwise send "0" to request a new ID.
        let prefer = if preferred_id.chars().all(|c| c.is_ascii_digit()) && !preferred_id.is_empty() {
            preferred_id
        } else {
            "0"
        };
        ctl.send(&build_login_host(&dt, password, local_ip, prefer, "vi", listen_port, captcha))
            .map_err(|e| format!("send host login failed: {e}"))?;

        // Reply: `3:<port>:<assigned ID>:<public ip>:…` when an ID is assigned,
        // or `60:<esc>:<extra>` when the server wants a captcha before a new ID
        // (page enter_captcha.aspx). Must be distinguished from a silent server.
        let resp = Self::wait_login_reply(&mut ctl, Duration::from_secs(6))
            .ok_or("server sent no login reply (silent)")?;
        let body = String::from_utf8_lossy(&resp[1..]).to_string();
        if std::env::var_os("XREMOTE_LOG_LOGIN").is_some() {
            let preview: String = body.chars().take(240).collect();
            eprintln!("login-reply: {preview}");
        }
        if let Some((esc, extra)) = split_captcha(&body) {
            return Err(captcha_fail(&esc, &extra));
        }
        if body.starts_with("60:") || body.starts_with("60\0") {
            return Err("server sent captcha (60) but the session code was unreadable".into());
        }
        let f: Vec<&str> = body.split(':').collect();
        if f.first() != Some(&"3") || f.len() < 4 {
            return Err(format!("unexpected login reply: {body}"));
        }
        let assigned_id = f[2].to_string();
        let public_ip = f[3].to_string();
        let p2p_port: u16 = f[1].trim().parse().unwrap_or(0);
        if assigned_id.is_empty() || !assigned_id.bytes().all(|b| b.is_ascii_digit()) {
            return Err(format!("server assigned no valid id: {body}"));
        }
        // Save it so the next login asks for this same ID (fixed ID, like UltraViewer).
        save_host_id(&assigned_id);
        Ok(HostControl { ctl, assigned_id, public_ip, p2p_port, last_keepalive: Instant::now() })
    }

    /// Read the login reply frame: a frame starting with `'3'` (ID assigned) or `'6'`
    /// (the `60:` challenge). Skip keepalive replies `'2'`. `None` if the server
    /// stays silent until the deadline.
    fn wait_login_reply(ctl: &mut Control, timeout: Duration) -> Option<Vec<u8>> {
        let deadline = Instant::now() + timeout;
        ctl.set_timeout(Duration::from_millis(500)).ok();
        while Instant::now() < deadline {
            match ctl.recv() {
                // Control frame: [u32 len][byte 0][kind byte][body]; kind is at index 1.
                Ok(Some(m)) if matches!(m.get(1), Some(b'3') | Some(b'6')) => return Some(m),
                Ok(Some(_)) => continue,
                Ok(None) => continue,
                Err(_) => continue,
            }
        }
        None
    }

    /// Wait for `command 17`. `Ok(Some(cmd17))` when a controller dials in,
    /// `Ok(None)` if not yet (keepalive was sent when due), `Err` if the link drops.
    ///
    /// Call this in a loop for the life of the host; each call blocks for about `poll`.
    pub fn poll(&mut self, poll: Duration) -> Result<Option<Cmd17>, String> {
        self.ctl.set_timeout(Duration::from_millis(500)).ok();
        let deadline = Instant::now() + poll;
        loop {
            // Keepalive every 20s so the server keeps the host online.
            if self.last_keepalive.elapsed() >= Duration::from_secs(20) {
                self.ctl.send(&frame_tdes(b"2")).map_err(|e| format!("keepalive failed: {e}"))?;
                self.last_keepalive = Instant::now();
            }
            match self.ctl.recv() {
                Ok(Some(m)) => {
                    let body = &m[1..];
                    if body.starts_with(b"17:") {
                        return Ok(Some(parse_cmd17(&String::from_utf8_lossy(body))));
                    }
                    // '2:<port>' is a keepalive reply — ignore.
                }
                Ok(None) => {}
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock || e.kind() == std::io::ErrorKind::TimedOut => {}
                Err(e) => return Err(format!("control channel read failed: {e}")),
            }
            if Instant::now() >= deadline {
                return Ok(None);
            }
        }
    }

    /// Borrow the control channel (for the rendezvous handshake after command 17).
    pub fn control_mut(&mut self) -> &mut Control {
        &mut self.ctl
    }
}

/// Host session already connected to a controller.
pub struct HostSession {
    /// AES session channel (raw TCP or TLS via a relay).
    pub session: Chan,
    /// The `login:…` message the controller sends at the start (UTF-16LE decoded).
    pub controller_login: String,
    /// Address/channel that connected (for logs).
    pub peer: String,
}

impl Cmd17 {
    /// Reply to command 17 on the **control channel**: report the address we are
    /// listening on so the server can hand it to the controller (ID lookup) and
    /// the controller dials us directly.
    pub fn reply_17(
        &self,
        ctl: &mut Control,
        pub_ip: &str,
        pub_port: u16,
        lan_ip: &str,
        lan_port: u16,
    ) -> Result<String, String> {
        let relay = self.relay_ips.first().cloned().unwrap_or_default();
        // The real message has 5 pipe fields at the end: pub_ip|pub_port|lan_ip|lan_port|session_token.
        // A missing 5th field makes VB6 report "ManipulatePack_Error: Subscript out of range".
        let body = format!(
            "17:{}:{}:{}:{}:{pub_ip}:{pub_port}:{lan_ip}|{lan_port}|{}",
            self.session_id, self.token, relay, lan_port, self.session_pw
        );
        ctl.send(&frame_tdes(body.as_bytes()))
            .map_err(|e| format!("send 17 on control failed: {e}"))?;
        Ok(body)
    }

    /// Use the socket the controller just accepted on the remote's listen port to open the session.
    pub fn from_socket(&self, sock: TcpStream, timeout: Duration) -> Result<HostSession, String> {
        if self.session_pw.is_empty() {
            return Err("command 17 has no session password".into());
        }
        sock.set_nodelay(true).ok();
        sock.set_read_timeout(Some(timeout)).ok();
        sock.set_write_timeout(Some(timeout)).ok();
        let peer = sock.peer_addr().map(|a| a.to_string()).unwrap_or_default();
        let mut chan = Chan::Tcp(Session::new(sock, &self.session_pw).map_err(|e| e.to_string())?);
        let login = read_login(&mut chan)?;
        Ok(HostSession { session: chan, controller_login: login, peer })
    }

    pub fn connect_controller(
        &self,
        ctl: &mut Control,
        lan_port: u16,
        timeout: Duration,
    ) -> Result<HostSession, String> {
        if self.session_pw.is_empty() {
            return Err("command 17 has no session password".into());
        }

        // 1) Relay: connect to EVERY relay in parallel (the real target does this).
        //    Use the channel that receives a session login.
        let mut relay_err = String::from("no relay");
        if !self.relay_ips.is_empty() {
            let mut links = crate::relay::connect_all(
                &self.session_id,
                &self.token,
                &self.session_pw,
                &self.relay_ips,
                &self.relay_hosts,
                lan_port,
                timeout,
            );
            if links.is_empty() {
                relay_err = "no relay connected".into();
            } else {
                eprintln!("[uv-host] opened {} parallel relay channels", links.len());
                // Reply to command 17 on the CONTROL channel: the server waits for
                // this before it returns the target address to the controller (pending ID lookup).
                let k0 = &links[0];
                let body = crate::relay::hs_body(
                    &self.session_id,
                    &self.token,
                    &k0.endpoint,
                    &k0.public_ip,
                    k0.public_port,
                    &k0.lan_ip,
                    k0.lan_port,
                );
                match ctl.send(&frame_tdes(body.as_bytes())) {
                    Ok(()) => eprintln!("[uv-host] replied 17 on control channel: {body}"),
                    Err(e) => eprintln!("[uv-host] send 17 on control failed: {e}"),
                }
                for link in &links {
                    link.chan.set_timeout(Duration::from_millis(250)).ok();
                }
                let deadline = Instant::now() + Duration::from_secs(40);
                while Instant::now() < deadline {
                    let mut got = None;
                    for i in 0..links.len() {
                        if let Ok(Some(m)) = links[i].chan.recv() {
                            got = Some((i, m));
                            break;
                        }
                    }
                    if let Some((i, m)) = got {
                        let link = links.swap_remove(i);
                        let login = parse_login(&m);
                        eprintln!("[uv-host] got login via {}", link.note);
                        return Ok(HostSession {
                            session: link.chan,
                            controller_login: login,
                            peer: link.note,
                        });
                    }
                }
                relay_err = format!("{} relay channels but no login within 40s", links.len());
            }
        }
        // 2) Direct connect (X-Remote-style controller that is listening).
        let dirs: Vec<(String, u16)> = [self.controller_lan.clone(), self.controller_pub.clone()]
            .into_iter()
            .flatten()
            .collect();
        if dirs.is_empty() {
            return Err(format!("relay failed ({relay_err}); command 17 has no controller address"));
        }
        let mut err = String::new();
        for (ip, port) in &dirs {
            let addr: SocketAddr = match format!("{ip}:{port}").parse() {
                Ok(a) => a,
                Err(e) => {
                    err = format!("{ip}:{port} bad: {e}");
                    continue;
                }
            };
            match TcpStream::connect_timeout(&addr, timeout) {
                Ok(sock) => {
                    sock.set_nodelay(true).ok();
                    sock.set_read_timeout(Some(Duration::from_secs(10))).ok();
                    let mut chan = Chan::Tcp(Session::new(sock, &self.session_pw).map_err(|e| e.to_string())?);
                    let login = read_login(&mut chan)?;
                    return Ok(HostSession {
                        session: chan,
                        controller_login: login,
                        peer: format!("{ip}:{port}"),
                    });
                }
                Err(e) => err = format!("{ip}:{port}: {e}"),
            }
        }
        Err(format!("relay failed ({relay_err}); direct connect failed ({err})"))
    }
}

/// Decode one decrypted frame into a `login:…` string (drop the flag byte, UTF-16LE).
fn parse_login(m: &[u8]) -> String {
    let body = if m.first() == Some(&1) { &m[1..] } else { &m[..] };
    let u16s: Vec<u16> = body.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
    String::from_utf16_lossy(&u16s)
}

/// Read the session `login` message: the first frame is `[flag 1][UTF-16LE string]`.
fn read_login(chan: &mut Chan) -> Result<String, String> {
    let m = chan
        .recv()
        .map_err(|e| format!("session login read failed: {e}"))?
        .ok_or("controller sent no session login")?;
    Ok(parse_login(&m))
}

/// Split `command 17` into fields.
///
/// Shape (captured 05/11/2026):
/// `17:<sid>:<token>:<ip1;ip2;ip3>:<host1;host2;host3>:-1:-1:0:0:<ctrl_pub>|<port>|<ctrl_lan>|<port>|<session_token>`
pub fn parse_cmd17(s: &str) -> Cmd17 {
    let f: Vec<&str> = s.splitn(6, ':').collect();
    let split_list = |i: usize| f.get(i).map(|x| x.split(';').map(|t| t.to_string()).collect()).unwrap_or_default();
    let tail = f.get(5).unwrap_or(&"").to_string();
    // The `|` part: <pub_ip>|<pub_port>|<lan_ip>|<lan_port>|<session password>.
    // It sits at the end (after "…0:0:"), split from `tail` at the last ':' before '|'.
    let pipe = tail
        .rsplit(':')
        .find(|seg| seg.contains('|'))
        .unwrap_or("");
    let g: Vec<&str> = pipe.split('|').collect();
    let port_at = |i: usize| g.get(i).and_then(|v| v.parse::<u16>().ok());
    let addr_at = |ip: usize, port: usize| match (g.get(ip), port_at(port)) {
        (Some(ip), Some(p)) if !ip.is_empty() => Some((ip.to_string(), p)),
        _ => None,
    };
    Cmd17 {
        session_id: f.get(1).unwrap_or(&"").to_string(),
        token: f.get(2).unwrap_or(&"").to_string(),
        relay_ips: split_list(3),
        relay_hosts: split_list(4),
        controller_pub: addr_at(0, 1),
        controller_lan: addr_at(2, 3),
        session_pw: g.get(4).unwrap_or(&"").to_string(),
        tail,
        raw: s.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_login_from_fake_controller() {
        use std::net::TcpListener;
        // Fake controller: listen locally, send a session login AES-encrypted with the session password.
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let pw = "sesskey123456";
        let pw2 = pw.to_string();
        let sv = std::thread::spawn(move || {
            let (sock, _) = l.accept().unwrap();
            let mut s = Session::new(sock, &pw2).unwrap();
            let mut payload = vec![1u8]; // binary flag
            for u in "login:777:deadbeef:1920:1080".encode_utf16() {
                payload.extend_from_slice(&u.to_le_bytes());
            }
            s.send(&payload).unwrap();
            std::thread::sleep(Duration::from_millis(100));
        });
        let sock = TcpStream::connect(("127.0.0.1", port)).unwrap();
        sock.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        let mut chan = Chan::Tcp(Session::new(sock, pw).unwrap());
        let login = read_login(&mut chan).expect("session login");
        assert!(login.starts_with("login:777:deadbeef:"), "{login}");
        sv.join().unwrap();
    }

    #[test]
    fn parse_cmd17_fields() {
        let s = "17:2399618:C80F155AF757:103.107.183.14;103.153.74.136;207.148.118.137:\
                 server200001837.ultraviewer.net;server200001951.ultraviewer.net:-1:-1:0:0:\
                 203.0.113.10|61185|198.51.100.36|61185|BINTOKEN";
        let c = parse_cmd17(s);
        assert_eq!(c.session_id, "2399618");
        assert_eq!(c.token, "C80F155AF757");
        assert_eq!(c.relay_ips, ["103.107.183.14", "103.153.74.136", "207.148.118.137"]);
        assert_eq!(c.relay_hosts.len(), 2);
        assert_eq!(c.controller_pub, Some(("203.0.113.10".into(), 61185)));
        assert_eq!(c.controller_lan, Some(("198.51.100.36".into(), 61185)));
        assert_eq!(c.session_pw, "BINTOKEN");
    }

    #[test]
    fn captcha_60_esc_and_url() {
        let raw = "60:mE78OnkVclIAnT8hqmYkFQ==:";
        let (esc, extra) = split_captcha(raw).unwrap();
        assert_eq!(esc, "mE78OnkVclIAnT8hqmYkFQ==");
        assert_eq!(extra, "");
        let url = url_captcha(&esc, &extra);
        assert!(url.contains("esc=mE78OnkVclIAnT8hqmYkFQ%3D%3D"), "{url}");
        assert!(url.contains("extra=&") || url.contains("extra=&esc="), "{url}");
        assert!(url.starts_with("https://www.ultraviewer.net/api/enter_captcha.aspx?"));
        let err = captcha_fail(&esc, &extra);
        assert_eq!(read_captcha_err(&err), Some((esc, extra)));
        assert!(split_captcha("3:1:2").is_none());
        assert!(!captcha_ok(""));
        assert!(!captcha_ok("a:b"));
    }
}
