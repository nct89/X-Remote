//! Wires three pieces together: the control channel, the session channel, and the `login` that opens the screen.
//!
//! Same order as `connect()` in the Python port that already ran against a real machine:
//! open a listen port → login → ID lookup → the peer connects back → send the session
//! `login` → ask for the screen stream (`17`, `4|||<id>`).

use std::net::{IpAddr, TcpListener, UdpSocket};
use std::time::Duration;

use md5::{Digest, Md5};
use rand::Rng;

use crate::control::{build_login, build_login_profile, build_tra, Control, FALLBACK_PROFILES, VERSION};
use crate::session::Session;

use crate::Ident;

/// Meaning of an ID-lookup status code.
pub fn status_text(code: &str) -> &'static str {
    match code {
        "1" => "success",
        "2" => "wrong peer password",
        "3" => "peer refused",
        "4" => "peer busy",
        "11" => "peer not ready (UltraViewer not open)",
        _ => "unknown",
    }
}

/// LAN IP to declare to the server.
pub fn local_ip() -> Option<IpAddr> {
    let s = UdpSocket::bind("0.0.0.0:0").ok()?;
    s.connect("8.8.8.8:80").ok()?;
    s.local_addr().ok().map(|a| a.ip())
}

fn session_password() -> String {
    const CHARS: &[u8] = b"!#$%&*+-./0123456789<=>?@ABCXYZ[]^_abcdefghijklmnopqrstuvwxyz{}~";
    let mut rng = rand::thread_rng();
    (0..13)
        .map(|_| CHARS[rng.gen_range(0..CHARS.len())] as char)
        .collect()
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn md5_hex(data: &[u8]) -> String {
    hex(&Md5::digest(data))
}

/// `login` message that opens a session (ASCII; sent as UTF-16LE).
///
/// `host_name` is shown on the peer in the "who is viewing you" list (it used
/// to always be `HOST-1`, the dev machine name).
pub fn session_login(sid: &str, partner_pw: &str, own_id: &str, host_name: &str) -> String {
    let mut h = Md5::new();
    h.update(b"                                "); // 32 spaces
    h.update(partner_pw.as_bytes());
    let f4 = hex(&h.finalize());
    let mut rng = rand::thread_rng();
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    let token: String = (0..16).map(|_| CHARS[rng.gen_range(0..CHARS.len())] as char).collect();
    format!(
        "login:{sid}:{}:1920:1080:0:15:15:510:120:{own_id}:{host_name}:0:{f4}:S-1-5-21-0-0-0-1000:{token}:2:45:-1:0:0:{VERSION}:0",
        md5_hex(format!("{sid}{partner_pw}").as_bytes())
    )
}

/// Read the control channel until a message of type `kind` (second byte).
fn ctl_wait(c: &mut Control, kind: u8, tries: usize) -> Option<Vec<u8>> {
    for _ in 0..tries {
        match c.recv() {
            Ok(Some(m)) if m.get(1) == Some(&kind) => return Some(m),
            Ok(_) => continue,
            Err(_) => continue,
        }
    }
    None
}

/// A session that is fully set up.
pub struct Connected {
    pub session: Session,
    pub control: Control,
    pub session_pw: String,
    pub peer: String,
}

/// Connect to a peer by UltraViewer ID and password.
pub fn connect(
    partner_id: &str,
    partner_pw: &str,
    own_id: &str,
    own_pw: &str,
    accept_timeout: Duration,
) -> Result<Connected, String> {
    let lan = local_ip().unwrap_or(IpAddr::from([0, 0, 0, 0])).to_string();
    let pub_ip = std::env::var("UV_PUBLIC_IP").unwrap_or_else(|_| lan.clone());

    let log = |s: &str| {
        if std::env::var("UV_DEBUG_INPUT").is_ok() {
            eprintln!("[uv-connect] {s}");
        }
    };

    let listener = TcpListener::bind("0.0.0.0:0").map_err(|e| format!("listen failed: {e}"))?;
    let lport = listener.local_addr().map_err(|e| e.to_string())?.port();
    // Must be non-blocking: in blocking mode `accept()` hangs forever and the
    // deadline below is never checked.
    listener.set_nonblocking(true).ok();
    log(&format!("listen {lport}, LAN {lan}"));

    let session_pw = session_password();
    let id = Ident::current();
    log(&format!("identity: host {} / user {}", id.host_name, id.user_name));
    let mut ctl = Control::connect(Duration::from_secs(15))
        .map_err(|e| format!("UltraViewer server connect failed: {e}"))?;
    let http_n = ctl.http_post().unwrap_or(0);
    log(&format!("TLS + HTTP done ({http_n} bytes)"));

    let mut eff_own_id = own_id.to_string();
    let initial_frame = if let Some(p) = FALLBACK_PROFILES
        .iter()
        .find(|p| p.disk_id == id.disk_id || p.prefer_id == own_id)
    {
        eff_own_id = p.prefer_id.to_string();
        build_login_profile(&id, &lport.to_string(), &lan, "vi", p, 9600)
    } else {
        build_login(&id, lport, &lan, own_id, own_pw, "vi")
    };
    ctl.send(&initial_frame)
        .map_err(|e| format!("login send failed: {e}"))?;
    ctl.set_timeout(Duration::from_secs(3)).ok();
    let mut challenge = ctl.recv().ok().flatten();
    if challenge.as_ref().and_then(|m| m.get(1)) == Some(&b'6') {
        for p in &FALLBACK_PROFILES {
            if let Ok(mut c2) = Control::connect(Duration::from_secs(10)) {
                if c2.http_post().is_ok()
                    && c2
                        .send(&build_login_profile(&id, &lport.to_string(), &lan, "vi", p, 9600))
                        .is_ok()
                {
                    c2.set_timeout(Duration::from_secs(3)).ok();
                    let r = c2.recv().ok().flatten();
                    if r.as_ref().and_then(|m| m.get(1)) == Some(&b'3') {
                        eff_own_id = p.prefer_id.to_string();
                        ctl = c2;
                        challenge = r;
                        break;
                    }
                }
            }
        }
    }
    let challenge_txt = challenge
        .as_ref()
        .map(|m| String::from_utf8_lossy(m).to_string())
        .unwrap_or_else(|| "none".into());
    log(&format!("login sent, challenge: {challenge_txt}"));
    ctl.set_timeout(Duration::from_secs(15)).ok();

    ctl.send(&build_tra(
        partner_id,
        partner_pw,
        &eff_own_id,
        own_pw,
        0,
        &pub_ip,
        lport,
        &lan,
        lport,
        &session_pw,
    ))
    .map_err(|e| format!("ID lookup send failed: {e}"))?;
    let resp = ctl_wait(&mut ctl, b'8', 4).ok_or("no ID lookup reply (control channel silent)")?;
    log(&format!("ID lookup reply: {}", String::from_utf8_lossy(&resp[..resp.len().min(160)])));
    let _ = &resp;
    let txt = String::from_utf8_lossy(&resp[1..]).to_string();
    if !txt.starts_with("8:1:") {
        let code = txt.split(':').nth(1).unwrap_or("?");
        return Err(format!("ID lookup failed ({code}: {}): {}", status_text(code), &txt[..txt.len().min(120)]));
    }

    // Listen port: the peer connects back within a few seconds.
    let deadline = std::time::Instant::now() + accept_timeout;
    let (sock, peer) = loop {
        match listener.accept() {
            Ok(v) => break v,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                if std::time::Instant::now() > deadline {
                    return Err("peer did not connect back before timeout".into());
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => return Err(format!("waiting for peer failed: {e}")),
        }
    };
    let peer = peer.to_string();
    sock.set_nonblocking(false).ok();
    drop(listener);
    log(&format!("peer connected from {peer}"));

    let mut session = Session::new(sock, &session_pw).map_err(|e| e.to_string())?;
    session.set_timeout(Duration::from_millis(500)).map_err(|e| e.to_string())?;

    // The server may send more messages on the control channel.
    ctl.set_timeout(Duration::from_millis(400)).ok();
    for _ in 0..4 {
        if ctl.recv().ok().flatten().is_none() {
            break;
        }
    }

    // Open the session: flag 1 (binary) + the login string as UTF-16LE.
    let sid: u64 = rand::thread_rng().gen_range(10_000_000..99_999_999);
    let login = session_login(&sid.to_string(), partner_pw, &eff_own_id, &id.host_name);
    let mut payload = vec![1u8];
    for u in login.encode_utf16() {
        payload.extend_from_slice(&u.to_le_bytes());
    }
    session.send(&payload).map_err(|e| format!("session login send failed: {e}"))?;

    std::thread::sleep(Duration::from_millis(400));
    session.send(b"\x0017").map_err(|e| e.to_string())?;
    std::thread::sleep(Duration::from_millis(150));
    // The original generates a positive 31-bit number; the peer reads it as i32, so it must not overflow.
    let rid: i32 = rand::thread_rng().gen_range(0..i32::MAX);
    session.send(format!("\x004|||{rid}").as_bytes()).map_err(|e| e.to_string())?;
    // Ask for sharp RGB555 frames up front: the peer's default is more compressed and
    // looks blurry / blocky during motion. 70 = prioritize sharpness, 2 = RGB555.
    session.send(b"\x0031|||70").map_err(|e| e.to_string())?;
    session.send(b"\x0032|||2").map_err(|e| e.to_string())?;
    session.set_timeout(Duration::from_secs(20)).map_err(|e| e.to_string())?;

    Ok(Connected { session, control: ctl, session_pw, peer })
}

/// Active disconnect the controller sends before closing: `00` + "19". Measured 2026-11-06 08:44:
/// UltraViewer on Windows (the controller) sends exactly this, then closes TCP when the user
/// presses X. Without it the peer treats the drop as a network failure and waits out the timeout.
pub const BREAK_ERR: &[u8] = b"\x0019";

/// Read until one frame decodes (used by probe mode).
pub fn next_frame(s: &mut Session, dict: Option<&[u8]>) -> Result<crate::frame::Frame, String> {
    for _ in 0..200 {
        // The peer only captures and pushes a new frame after each `18`. Without it the
        // session goes quiet after the first frame.
        let _ = s.send(b"\x0018");
        let batch = s.recv_batch().map_err(|e| e.to_string())?;
        if batch.is_empty() {
            continue;
        }
        let mut last = None;
        for msg in batch {
            // The first byte is a flag (0 = text, 1 = binary); most frames follow it.
            let body = match msg.first() {
                Some(0) | Some(1) => &msg[1..],
                _ => &msg[..],
            };
            if let Some(parts) = crate::multipart::decode(body) {
                if let Some(f) = crate::frame::parse(&parts, dict) {
                    last = Some(f);
                }
            }
        }
        if let Some(f) = last {
            return Ok(f);
        }
    }
    Err("no frame received".into())
}

/// Send one already-framed AES plaintext (used by tests).
pub fn raw_send(s: &mut Session, plain: &[u8]) -> std::io::Result<()> {
    s.send(plain)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_login_format() {
        let s = session_login("12345678", "test-pw", "999888777", "may-thu");
        assert!(s.starts_with("login:12345678:"));
        assert!(s.ends_with(":0"));
        assert!(s.contains(":1920:1080:0:15:15:510:120:999888777:may-thu:0:"));
        assert!(!s.contains("HOST-1"));
        assert!(s.contains("S-1-5-21-0-0-0-1000:"));
        // 23 fields => 22 ':'
        assert_eq!(s.matches(':').count(), 22, "{s}");
    }

    #[test]
    fn status_codes() {
        assert_eq!(status_text("1"), "success");
        assert_eq!(status_text("11"), "peer not ready (UltraViewer not open)");
        assert_eq!(status_text("99"), "unknown");
    }

    #[test]
    fn session_password_is_13_chars() {
        let p = session_password();
        assert_eq!(p.chars().count(), 13);
        assert!(p.chars().all(|c| c.is_ascii_graphic()));
    }

    #[test]
    fn local_ip_is_found() {
        let ip = local_ip();
        assert!(ip.is_some(), "must read a LAN IP");
    }
}
