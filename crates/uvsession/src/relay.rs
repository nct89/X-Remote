//! P2P channel through a **relay** — host (controlled) side, rebuilt from the real DLL.
//!
//! Checked against:
//! * `RemoteControl.P2P_TCPSocket.BindAndGetPort_Thread` — open the channel, ask
//!   for the "public IP", store `LocalEndPoint` as "LAN ip|port";
//! * `docs/03 §16` — the peer sends the **command 17 response (3DES)** on the
//!   relay channel (94 bytes = `[u32][u16=1][3DES]`), not on the control channel;
//! * Real UltraViewer **listens on no port** (measured on Windows 05/11/2026), so
//!   dialing the address inside command 17 always times out.
//!
//! The relay only speaks HTTPS 443 (every relay IP returns `200` for `POST /UVService/`),
//! so the P2P channel runs **on the same TLS connection** used for rendezvous: after
//! the command-17 handshake, session AES frames continue on it.

use std::io::{Read, Write};
use std::time::Duration;

use crate::crypto::frame_tdes;
use crate::session::{Chan, Session};

/// Chosen relay: IP plus the relay port assigned to the session.
#[derive(Debug, Clone)]
pub struct RelayEndpoint {
    pub ip: String,
    pub port: u16,
    /// Our own public IP as the relay sees it (cmd19 field 2).
    pub public_hint: String,
}

/// Rendezvous frame: `[u32 len][0x01][body]`.
fn frame8(body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(5 + body.len());
    out.extend_from_slice(&((body.len() + 1) as u32).to_le_bytes());
    out.push(1);
    out.extend_from_slice(body);
    out
}

fn read_exact(s: &mut impl Read, n: usize) -> std::io::Result<Vec<u8>> {
    let mut v = vec![0u8; n];
    s.read_exact(&mut v)?;
    Ok(v)
}

/// Split `0:<port>:<IP>:4`.
fn split_addr(s: &str) -> Option<(String, u16)> {
    let f: Vec<&str> = s.split(':').collect();
    if f.len() < 3 {
        return None;
    }
    let port: u16 = f[1].trim().parse().ok()?;
    let ip = f[2].trim().to_string();
    if ip.is_empty() || port == 0 {
        return None;
    }
    Some((ip, port))
}

/// Command 17 handshake body:
/// `17:<sid>:<token>:<relay_ip>:<relay port>:<public ip>:<port>:<lan_ip>|<lan port>`
pub fn hs_body(
    session_id: &str,
    token: &str,
    ep: &RelayEndpoint,
    public_ip: &str,
    public_port: u16,
    lan_ip: &str,
    lan_port: u16,
) -> String {
    format!(
        "17:{session_id}:{token}:{}:{}:{public_ip}:{public_port}:{lan_ip}|{lan_port}",
        ep.ip, ep.port
    )
}

/// Result of one relay connection.
pub struct RelayLink {
    /// Session channel (TLS to the relay, handshake done).
    pub chan: Chan,
    pub endpoint: RelayEndpoint,
    pub public_ip: String,
    pub public_port: u16,
    pub lan_ip: String,
    pub lan_port: u16,
    pub note: String,
}

/// Try every relay in one `command 17`.
pub fn via_relay(
    session_id: &str,
    token: &str,
    session_pw: &str,
    relay_ips: &[String],
    relay_hosts: &[String],
    lan_port: u16,
    timeout: Duration,
) -> Result<RelayLink, String> {
    if relay_ips.is_empty() {
        return Err("command 17 has no relay ip".into());
    }
    let mut err = String::new();
    for (i, ip) in relay_ips.iter().enumerate() {
        let host = relay_hosts
            .get(i)
            .or_else(|| relay_hosts.first())
            .cloned()
            .unwrap_or_else(|| "server200001976.ultraviewer.net".to_string());
        // 1) Raw TCP: same as the real DLL (BindAndGetPort_Thread).
        match one_relay_raw(session_id, token, session_pw, ip, lan_port, timeout) {
            Ok(k) => return Ok(k),
            Err(e) => err = format!("relay {ip} raw: {e}"),
        }
        // 2) TLS rendezvous (fallback).
        match one_relay(session_id, token, session_pw, ip, &host, lan_port, timeout) {
            Ok(k) => return Ok(k),
            Err(e) => err = format!("{err}; relay {ip} TLS: {e}"),
        }
    }
    Err(err)
}

fn one_relay(
    session_id: &str,
    token: &str,
    session_pw: &str,
    relay_ip: &str,
    relay_host: &str,
    lan_port: u16,
    timeout: Duration,
) -> Result<RelayLink, String> {
    let sock = open_tcp(relay_ip, lan_port, timeout)
        .map_err(|e| format!("connect relay from port {lan_port} failed: {e}"))?;
    let mut tls = crate::control::tls_wrap_any(sock, relay_host)
        .map_err(|e| format!("TLS failed: {e}"))?;

    let _ = relay_host;
    // TCPControl(useSSL): after TLS the next bytes are an AES frame, NOT an HTTP POST.
    let local = tls.sock.local_addr().map_err(|e| e.to_string())?;
    let lan_ip = local.ip().to_string();
    let ep = RelayEndpoint {
        ip: relay_ip.to_string(),
        port: local.port(),
        public_hint: String::new(),
    };
    // Send the command 17 handshake (3DES) directly — the relay pairs on sid/token.
    let body = hs_body(
        session_id,
        token,
        &ep,
        &lan_ip,
        lan_port,
        &lan_ip,
        lan_port,
    );
    // The TCPControl relay channel is AES (not the 3DES of the control channel).
    let ct = crate::crypto::aes_encrypt(session_pw, body.as_bytes());
    let mut fr = Vec::with_capacity(4 + ct.len());
    fr.extend_from_slice(&(ct.len() as u32).to_le_bytes());
    fr.extend_from_slice(&ct);
    tls.write_all(&fr).map_err(|e| format!("send AES handshake failed: {e}"))?;
    tls.flush().ok();
    tls.sock.set_read_timeout(Some(Duration::from_secs(20))).ok();

    let note = format!(
        "relay {relay_ip}:443 (source port {}, lan |{lan_port})",
        local.port()
    );
    Ok(RelayLink {
        chan: Chan::Tls(Session::wrap(tls, session_pw)),
        endpoint: ep,
        public_ip: lan_ip.clone(),
        public_port: lan_port,
        lan_ip,
        lan_port,
        note,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_addr_cmd19() {
        assert_eq!(split_addr("0:61434:203.0.113.10:4"), Some(("203.0.113.10".into(), 61434)));
        assert_eq!(split_addr("0:57993:1.2.3.4:4"), Some(("1.2.3.4".into(), 57993)));
        assert_eq!(split_addr("junk"), None);
        assert_eq!(split_addr("0:0:1.2.3.4:4"), None);
    }

    #[test]
    fn handshake_layout() {
        let ep = RelayEndpoint { ip: "203.0.113.20".into(), port: 57993, public_hint: "203.0.113.10".into() };
        let s = hs_body("100000001", "AABBCCDDEEFF", &ep, "203.0.113.10", 26, "198.51.100.36", 57993);
        assert_eq!(
            s,
            "17:100000001:AABBCCDDEEFF:203.0.113.20:57993:203.0.113.10:26:198.51.100.36|57993"
        );
    }
}

/// **Raw TCP** path, same as `P2P_TCPSocket.BindAndGetPort_Thread`:
/// connect `relay_ip:443` (no TLS) → read the "get public IP" command under AES
/// (key = session token) → send the command 17 handshake as 3DES on that socket.
fn one_relay_raw(
    session_id: &str,
    token: &str,
    session_pw: &str,
    relay_ip: &str,
    lan_port: u16,
    timeout: Duration,
) -> Result<RelayLink, String> {
    use crate::crypto::aes_encrypt;
    use std::io::Read;

    let sock = std::net::TcpStream::connect_timeout(
        &format!("{relay_ip}:443").parse().map_err(|e| format!("bad address: {e}"))?,
        timeout,
    )
    .map_err(|e| format!("raw connect {relay_ip}:443 failed: {e}"))?;
    sock.set_nodelay(true).ok();
    sock.set_read_timeout(Some(timeout)).ok();
    sock.set_write_timeout(Some(timeout)).ok();
    let local = sock.local_addr().map_err(|e| e.to_string())?;

    let ct = aes_encrypt(session_pw, &[0x01, 0x30]);
    let mut fr = Vec::with_capacity(4 + ct.len());
    fr.extend_from_slice(&(ct.len() as u32).to_le_bytes());
    fr.extend_from_slice(&ct);
    (&sock).write_all(&fr).map_err(|e| format!("send 0x0130 failed: {e}"))?;

    let mut hdr = [0u8; 4];
    (&sock).read_exact(&mut hdr).map_err(|e| format!("read public ip failed: {e}"))?;
    let n = u32::from_le_bytes(hdr) as usize;
    if n == 0 || n > 4096 {
        return Err(format!("public-ip reply length invalid: {n}"));
    }
    let mut body = vec![0u8; n];
    (&sock).read_exact(&mut body).map_err(|e| format!("read ip body failed: {e}"))?;
    let text = String::from_utf8_lossy(&body).to_string();
    let (public_ip, public_port) =
        split_addr(&text).ok_or_else(|| format!("unexpected public-ip reply: {text:?}"))?;

    let ep = RelayEndpoint { ip: relay_ip.to_string(), port: 443, public_hint: public_ip.clone() };
    let body = hs_body(
        session_id,
        token,
        &ep,
        &public_ip,
        public_port,
        &local.ip().to_string(),
        lan_port,
    );
    (&sock)
        .write_all(&frame_tdes(body.as_bytes()))
        .map_err(|e| format!("send raw handshake failed: {e}"))?;

    sock.set_read_timeout(Some(Duration::from_secs(20))).ok();
    let note = format!(
        "raw TCP {relay_ip}:443 -> public {public_ip}:{public_port}, lan {}|{lan_port}",
        local.ip()
    );
    Ok(RelayLink {
        chan: Chan::Tcp(Session::new(sock, session_pw).map_err(|e| e.to_string())?),
        endpoint: ep,
        public_ip,
        public_port,
        lan_ip: local.ip().to_string(),
        lan_port,
        note,
    })
}

/// TCP connect **from a chosen source port** — same as real UltraViewer: it binds
/// the P2P port (declared in login) and connects to the relay from that port, so
/// NAT/relay can route the return path (Windows socket log 05/11/2026).
/// IPv4 `sockaddr_in` (`s_addr` is network order, `port` is host order). Zero the
/// struct, then set fields: macOS/BSD also have `sin_len` (omitting it breaks the
/// macOS build, measured 06/11); Linux does not.
#[cfg(unix)]
fn sockaddr_ipv4(s_addr: u32, port: u16) -> libc::sockaddr_in {
    let mut a: libc::sockaddr_in = unsafe { std::mem::zeroed() };
    #[cfg(any(target_os = "macos", target_os = "ios", target_os = "freebsd", target_os = "openbsd", target_os = "netbsd"))]
    {
        a.sin_len = std::mem::size_of::<libc::sockaddr_in>() as u8;
    }
    a.sin_family = libc::AF_INET as libc::sa_family_t;
    a.sin_port = port.to_be();
    a.sin_addr = libc::in_addr { s_addr };
    a
}

#[cfg(unix)]
fn connect_port(dest: &str, src_port: u16, timeout: Duration) -> std::io::Result<std::net::TcpStream> {
    use std::os::unix::io::FromRawFd;
    let addr: std::net::SocketAddr =
        dest.parse().map_err(|_| std::io::Error::other("bad address"))?;
    let ip = match addr.ip() {
        std::net::IpAddr::V4(v) => v.octets(),
        _ => return Err(std::io::Error::other("IPv4 only")),
    };
    unsafe {
        let fd = libc::socket(libc::AF_INET, libc::SOCK_STREAM, 0);
        if fd < 0 {
            return Err(std::io::Error::last_os_error());
        }
        let opt: libc::c_int = 1;
        for name in [libc::SO_REUSEADDR, libc::SO_REUSEPORT] {
            libc::setsockopt(
                fd,
                libc::SOL_SOCKET,
                name,
                &opt as *const _ as *const libc::c_void,
                std::mem::size_of::<libc::c_int>() as libc::socklen_t,
            );
        }
        let bind_addr = sockaddr_ipv4(0, src_port);
        if libc::bind(fd, &bind_addr as *const _ as *const libc::sockaddr, std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t) != 0 {
            let e = std::io::Error::last_os_error();
            libc::close(fd);
            return Err(e);
        }
        let dest_in = sockaddr_ipv4(u32::from_ne_bytes(ip), addr.port());
        if libc::connect(fd, &dest_in as *const _ as *const libc::sockaddr, std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t) != 0 {
            let e = std::io::Error::last_os_error();
            libc::close(fd);
            return Err(e);
        }
        let s = std::net::TcpStream::from_raw_fd(fd);
        s.set_read_timeout(Some(timeout))?;
        s.set_write_timeout(Some(timeout))?;
        s.set_nodelay(true)?;
        Ok(s)
    }
}

/// Open TCP to the relay, preferring source port = the P2P port declared in login.
fn open_tcp(ip: &str, src_port: u16, timeout: Duration) -> std::io::Result<std::net::TcpStream> {
    #[cfg(unix)]
    {
        if src_port != 0 {
            return connect_port(&format!("{ip}:443"), src_port, timeout);
        }
    }
    let s = std::net::TcpStream::connect_timeout(
        &format!("{ip}:443").parse().map_err(|e| std::io::Error::other(format!("{e}")))?,
        timeout,
    )?;
    s.set_read_timeout(Some(timeout))?;
    s.set_write_timeout(Some(timeout))?;
    s.set_nodelay(true)?;
    Ok(s)
}

/// Connect **in parallel** to every relay in command 17 — same as the real target
/// (Windows log 15:04:43: one P2P port opens 3 TLS connections to 3 relays at once).
/// Whichever relay gets the session is the channel the caller then reads login from.
pub fn connect_all(
    session_id: &str,
    token: &str,
    session_pw: &str,
    relay_ips: &[String],
    relay_hosts: &[String],
    lan_port: u16,
    timeout: Duration,
) -> Vec<RelayLink> {
    use std::sync::mpsc::channel;
    let (tx, rx) = channel();
    let mut n = 0;
    for (i, ip) in relay_ips.iter().enumerate() {
        let host = relay_hosts
            .get(i)
            .or_else(|| relay_hosts.first())
            .cloned()
            .unwrap_or_else(|| "server200001976.ultraviewer.net".to_string());
        let (sid, tok, pw, ip2) = (
            session_id.to_string(),
            token.to_string(),
            session_pw.to_string(),
            ip.clone(),
        );
        let tx2 = tx.clone();
        n += 1;
        std::thread::spawn(move || {
            let r = one_relay(&sid, &tok, &pw, &ip2, &host, lan_port, timeout);
            let _ = tx2.send(r);
        });
    }
    drop(tx);
    let mut out = Vec::new();
    for _ in 0..n {
        if let Ok(Ok(k)) = rx.recv() {
            out.push(k);
        }
    }
    out
}
