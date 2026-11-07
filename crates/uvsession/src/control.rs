//! Control channel: TLS to `functions3.ultraviewer.net:443`, 3DES frames.
//!
//! Sequence (same as the checked Python client):
//! 1. open TLS, send `POST /UVService/` so the server switches to binary mode;
//! 2. `login` declares our ID and password;
//! 3. `tra` asks for the remote (session address, session password);
//! 4. wait for the remote to connect back.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::Arc;
use std::time::Duration;

use md5::{Digest, Md5};

use crate::crypto::frame_tdes;

pub const CTRL_HOST: &str = "functions3.ultraviewer.net";
pub const CTRL_PORT: u16 = 443;
/// Client version declared to the server; keep the value that was checked.
pub const VERSION: &str = "6.6.124";

/// Windows-style ticks (100 ns since 0001-01-01).
pub fn net_ticks() -> i64 {
    let unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    (unix + 62_135_596_800) * 10_000_000
}

// Stable machine id (field 10 = disk id) lives in `Ident::disk_id`
// (see `ident.rs`). The old form (16-char UID at the tail of `<hw>-<ticks>-<UID>`)
// is gone: a 06/11 MITM of real UltraViewer showed field 10 is the disk
// `model|serial|firmware`, with no UID, and the server pins the ID to that string.

/// Standard base64 (with `=` padding) — used only for the host name.
pub fn base64(data: &[u8]) -> String {
    const B64: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let b = [c[0], *c.get(1).unwrap_or(&0), *c.get(2).unwrap_or(&0)];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        for i in 0..4 {
            if i <= c.len() {
                out.push(B64[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// `login` message (command 3) — identity of the client (the controlling side).
///
/// Field 3 here is the listen port (controller). The *controlled* side uses
/// [`build_login_host`] (field 3 is the password).
pub fn build_login(
    dt: &crate::Ident,
    port: u16,
    local_ip: &str,
    own_id: &str,
    own_pw: &str,
    lang: &str,
) -> Vec<u8> {
    let _ = own_pw;
    frame_tdes(login_body(dt, &port.to_string(), local_ip, own_id, lang, port, net_ticks(), "").as_bytes())
}

/// **Host** `login` (the controlled side): field 3 is the **password**.
///
/// Found 05/11/2026 (Frida on real UltraViewer): the host registers with this
/// login and the password in field 3 (not a port). The server **assigns an ID**
/// in the reply `3:<port>:<assigned ID>:…`; when a controller dials that ID the
/// server pushes `command 17` down this connection. See docs/03 §22.
pub fn build_login_host(
    dt: &crate::Ident,
    password: &str,
    local_ip: &str,
    own_id: &str,
    lang: &str,
    listen_port: u16,
    captcha: &str,
) -> Vec<u8> {
    frame_tdes(
        login_body(dt, password, local_ip, own_id, lang, listen_port, net_ticks(), captcha)
            .as_bytes(),
    )
}

/// Login body. `field3` is the port (controller) or the password (host).
///
/// Field 10 is the **disk id** `model|serial|firmware` ([`crate::Ident::disk_id`]);
/// the server pins the ID to that string (06/11 MITM of real UltraViewer — see docs/03).
/// An earlier build sent `{hw}-{ts}-{token}` (timestamp changed every time) and did not match UltraViewer.
fn login_body(
    dt: &crate::Ident,
    field3: &str,
    local_ip: &str,
    own_id: &str,
    lang: &str,
    listen_port: u16,
    ts: i64,
    captcha: &str,
) -> String {
    let (machine_id, hw, user) = (&dt.machine_id, &dt.hw_id, &dt.user_name);
    let name = base64(dt.host_name.as_bytes());
    let disk = &dt.disk_id;
    // The last field stays empty until the user solves captcha command `60`. The server
    // reads the token from the last field of the next registration (same as client 6.6).
    let mut s = format!(
        "3:{machine_id}:{field3}:0:{local_ip}:0:{name}:{VERSION}:{hw}:{disk}:{listen_port}|6|0|3|0|0::::0:{lang}:{user}:1:1:{own_id}:0:1:{}:{ts}:{}",
        ts + 10_000_000,
        ts + 1_000_000_000
    );
    if !captcha.is_empty() {
        s.push(':');
        s.push_str(captcha);
    }
    s
}

/// `tra` message (command 8) — ask how to reach the remote.
#[allow(clippy::too_many_arguments)]
pub fn build_tra(
    partner_id: &str,
    partner_pw: &str,
    own_id: &str,
    own_pw: &str,
    counter: i64,
    public_ip: &str,
    public_port: u16,
    local_ip: &str,
    local_port: u16,
    session_pw: &str,
) -> Vec<u8> {
    let mut h = Md5::new();
    h.update(b"                                "); // 32 spaces
    h.update(partner_pw.as_bytes());
    let f3 = hex(&h.finalize_reset());
    h.update(format!("{own_id}{own_pw}").as_bytes());
    let f8 = hex(&h.finalize());
    let body = format!(
        "8:{partner_id}:{f3}:{VERSION}:::{own_id}:{f8}:-1:{counter}:0:0:0:420:1::{public_ip}|{public_port}|{local_ip}|{local_port}|{session_pw}"
    );
    frame_tdes(body.as_bytes())
}

fn hex(b: &[u8]) -> String {
    let mut s = String::with_capacity(b.len() * 2);
    for x in b {
        s.push_str(&format!("{x:02x}"));
    }
    s
}

/// Wrapped TLS connection, read/write as frames `[u32 length][bytes]`.
pub struct Control {
    stream: rustls::StreamOwned<rustls::ClientConnection, TcpStream>,
    /// Bytes already read from TLS but not yet consumed (tail of the HTTP reply).
    buf: Vec<u8>,
}

#[derive(Debug)]
struct AcceptAll(Arc<rustls::crypto::CryptoProvider>);

impl rustls::client::danger::ServerCertVerifier for AcceptAll {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}

/// Open TLS to **any IP** with SNI `name` (used for the P2P relay).
///
/// The relay presents a certificate we cannot verify (same as the control
/// server), so this shares the accept-all verifier above.
pub fn tls_connect_any(
    ip: &str,
    port: u16,
    name: &str,
    timeout: Duration,
) -> std::io::Result<rustls::StreamOwned<rustls::ClientConnection, TcpStream>> {
    let sock = TcpStream::connect_timeout(
        &format!("{ip}:{port}")
            .parse()
            .map_err(|e| std::io::Error::other(format!("bad address: {e}")))?,
        timeout,
    )?;
    sock.set_read_timeout(Some(timeout))?;
    sock.set_write_timeout(Some(timeout))?;
    sock.set_nodelay(true)?;
    tls_wrap_any(sock, name)
}

/// Wrap TLS around an already connected `TcpStream` (used when the source port must be bound).
pub fn tls_wrap_any(
    sock: TcpStream,
    name: &str,
) -> std::io::Result<rustls::StreamOwned<rustls::ClientConnection, TcpStream>> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = rustls::ClientConfig::builder_with_provider(provider.clone())
        .with_safe_default_protocol_versions()
        .map_err(|e| std::io::Error::other(e.to_string()))?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(AcceptAll(provider)))
        .with_no_client_auth();
    let server_name =
        rustls::pki_types::ServerName::try_from(name.to_string()).map_err(|e| std::io::Error::other(e.to_string()))?;
    let conn = rustls::ClientConnection::new(Arc::new(config), server_name)
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    Ok(rustls::StreamOwned::new(conn, sock))
}

impl Control {
    pub fn connect(timeout: Duration) -> std::io::Result<Control> {
        let sock = TcpStream::connect((CTRL_HOST, CTRL_PORT))?;
        sock.set_read_timeout(Some(timeout))?;
        sock.set_write_timeout(Some(timeout))?;
        sock.set_nodelay(true)?;

        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let config = rustls::ClientConfig::builder_with_provider(provider.clone())
            .with_safe_default_protocol_versions()
            .map_err(|e| std::io::Error::other(e.to_string()))?
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(AcceptAll(provider)))
            .with_no_client_auth();

        let name = rustls::pki_types::ServerName::try_from(CTRL_HOST)
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        let conn = rustls::ClientConnection::new(Arc::new(config), name)
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        Ok(Control { stream: rustls::StreamOwned::new(conn, sock), buf: Vec::new() })
    }

    pub fn set_timeout(&mut self, t: Duration) -> std::io::Result<()> {
        self.stream.sock.set_read_timeout(Some(t))?;
        self.stream.sock.set_write_timeout(Some(t))
    }

    /// Read more TLS bytes into the buffer; return how many were new.
    fn fill(&mut self) -> std::io::Result<usize> {
        let mut chunk = [0u8; 8192];
        let n = self.stream.read(&mut chunk)?;
        self.buf.extend_from_slice(&chunk[..n]);
        Ok(n)
    }

    /// Make sure the buffer holds at least `n` bytes.
    fn need(&mut self, n: usize) -> std::io::Result<bool> {
        while self.buf.len() < n {
            if self.fill()? == 0 {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// Take `n` bytes off the front of the buffer.
    fn take(&mut self, n: usize) -> Vec<u8> {
        self.buf.drain(..n).collect()
    }

    /// Switch the server into binary mode.
    ///
    /// Read the whole HTTP reply (through the body given by `Content-Length`) and drop it;
    /// leftover bytes belong to the binary stream and stay in the buffer.
    pub fn http_post(&mut self) -> std::io::Result<usize> {
        let req = b"POST /UVService/ HTTP/1.1\r\nUser-Agent: Mozilla/4.0\r\n\
Content-Type: application/octet-stream\r\nConnection: Keep-Alive\r\n\
Content-length: 8589934590\r\nHost: functions3.ultraviewer.net\r\n\r\n";
        self.stream.write_all(req)?;
        self.stream.flush()?;

        let mut head_end = None;
        let mut total = 0usize;
        while head_end.is_none() {
            if self.fill()? == 0 {
                break;
            }
            total = self.buf.len();
            head_end = self.buf.windows(4).position(|w| w == b"\r\n\r\n").map(|p| p + 4);
        }
        let Some(head_end) = head_end else { return Ok(total) };

        let head = String::from_utf8_lossy(&self.buf[..head_end]).to_lowercase();
        let body_len: usize = head
            .lines()
            .find(|l| l.starts_with("content-length:"))
            .and_then(|l| l.split(':').nth(1))
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(0);
        // The server echoes `Content-length: 8589934590` and then switches
        // straight to the binary stream, with no HTTP body. Waiting for that
        // number adds ~15 s (read timeout) per connect and can swallow the
        // start of the binary stream, so only wait when the body is real and small.
        let has_body = body_len > 0 && body_len <= (1 << 20);
        let want = if has_body { head_end + body_len } else { head_end };
        if has_body {
            let _ = self.need(want);
        }
        let dropped = self.take(self.buf.len().min(want));
        Ok(dropped.len())
    }

    pub fn send(&mut self, frame: &[u8]) -> std::io::Result<()> {
        self.stream.write_all(frame)?;
        self.stream.flush()
    }

    /// Read one frame; `None` on timeout or a closed connection.
    pub fn recv(&mut self) -> std::io::Result<Option<Vec<u8>>> {
        if !self.need(4)? {
            return Ok(None);
        }
        let hdr = [self.buf[0], self.buf[1], self.buf[2], self.buf[3]];
        let n = u32::from_le_bytes(hdr) as usize;
        if n == 0 || n > 8 << 20 {
            return Ok(None);
        }
        if !self.need(4 + n)? {
            return Ok(None);
        }
        self.take(4);
        Ok(Some(self.take(n)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_std() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foob"), "Zm9vYg==");
        assert_eq!(base64(b"HOST-1"), "SE9TVC0x");
    }

    #[test]
    fn net_ticks_increases() {
        let a = net_ticks();
        assert!(a > 0);
        // 2026-01-01 in Windows ticks
        assert!(a > 639_000_000_000_000_000, "{a}");
    }

    #[test]
    fn login_is_framed() {
        let dt = crate::Ident::from_seed("g", "may-thu", "ban");
        let f = build_login(&dt, 0, "10.0.0.2", "987654321", "11111", "vi");
        let n = u32::from_le_bytes([f[0], f[1], f[2], f[3]]) as usize;
        assert_eq!(n, f.len() - 4);
        assert_eq!(u16::from_le_bytes([f[4], f[5]]), 1);
        assert_eq!((f.len() - 6) % 8, 0);
        assert!(f.len() > 60, "login message too short: {}", f.len());
    }

    #[test]
    fn login_uses_ident_not_hardcoded() {
        let dt = crate::Ident::from_seed("g", "may-thu", "ban");
        let s = login_body(&dt, "5000", "10.0.0.2", "987654321", "vi", 5000, 639_000_000_000_000_000, "");
        let f: Vec<&str> = s.split(':').collect();
        assert_eq!(f[1], dt.machine_id);
        assert_eq!(f[6], base64(b"may-thu"));
        assert_eq!(f[8], dt.hw_id);
        // Field 10 = disk id `model|serial|firmware` (stable per machine).
        assert_eq!(f[9], dt.disk_id);
        assert!(f[9].contains('|') && f[9].matches('|').count() == 2, "field 10 must look like model|serial|firmware: {}", f[9]);
        assert!(s.contains(":vi:ban:1:1:987654321:"), "{s}");
        // Placeholders only. "000000000000" is not used here: it is a substring of the test timestamp.
        for bad in ["00112233445566778899AABBCCDDEEFF", "AABBCCDDEE00", "dev", "SE9TVC0x"] {
            assert!(!s.contains(bad), "still has hardcoded value {bad}: {s}");
        }
    }

    #[test]
    fn login_host_field3_is_password() {
        let dt = crate::Ident::from_seed("g", "may-thu", "ban");
        // Host: field 3 must be the password, not a port.
        let s = login_body(&dt, "test-pw", "10.0.0.2", "100000001", "vi", 9600, 639_000_000_000_000_000, "");
        let f: Vec<&str> = s.split(':').collect();
        assert_eq!(f[2], "test-pw", "field 3 must be the password: {s}");
        assert!(!s.ends_with(":TOK"), "no captcha means no extra field: {s}");
        let with_captcha = login_body(&dt, "test-pw", "10.0.0.2", "100000001", "vi", 9600, 639_000_000_000_000_000, "TOK");
        assert!(with_captcha.ends_with(":TOK"), "{with_captcha}");
        assert_eq!(with_captcha.split(':').count(), s.split(':').count() + 1);
        // And build_login_host still frames it.
        let fr = build_login_host(&dt, "test-pw", "10.0.0.2", "100000001", "vi", 9600, "");
        assert_eq!((fr.len() - 6) % 8, 0);
        assert_eq!(u16::from_le_bytes([fr[4], fr[5]]), 1);
    }

    #[test]
    fn tra_frame_shape() {
        let f = build_tra("100000001", "test-pw", "987654321", "11111", 7, "1.2.3.4", 7801, "198.51.100.14", 5000, "abc123");
        let n = u32::from_le_bytes([f[0], f[1], f[2], f[3]]) as usize;
        assert_eq!(n, f.len() - 4);
        assert_eq!(u16::from_le_bytes([f[4], f[5]]), 1);
    }
}
