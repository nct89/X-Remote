//! Session channel: rendezvous (get the P2P address) + raw TCP + AES.

use crate::aes_decrypt;
use crate::aes_encrypt;
use rustls::pki_types::ServerName;
use rustls::{ClientConfig, ClientConnection, StreamOwned};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::Arc;

/// Frame `[u32 len][u8 type=1][body]` (used for rendezvous messages).
pub fn frame8(body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(5 + body.len());
    out.extend_from_slice(&((body.len() + 1) as u32).to_le_bytes());
    out.push(1u8);
    out.extend_from_slice(body);
    out
}

fn no_verify_config() -> Arc<ClientConfig> {
    let config = ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(crate::net::NoVerify))
        .with_no_client_auth();
    Arc::new(config)
}

/// Controller-side rendezvous: connect the P2P relay -> POST -> send "0" + nonce -> read command 19.
/// Returns the command 19 body (like `0:<port>:<ip>:4`).
pub fn rendezvous(relay_ip: &str, nonce: &str) -> std::io::Result<Vec<u8>> {
    let host = "server200001976.ultraviewer.net";
    let tcp = TcpStream::connect((relay_ip, 443))?;
    let name = ServerName::try_from(host.to_string())
        .map_err(|_| std::io::Error::other("invalid server name"))?;
    let conn = ClientConnection::new(no_verify_config(), name)
        .map_err(|_| std::io::Error::other("tls config"))?;
    let mut tls = StreamOwned::new(conn, tcp);
    tls.write_all(
        format!(
            "POST /UVService/ HTTP/1.1\r\nUser-Agent: Mozilla/4.0\r\nContent-Type: application/octet-stream\r\nConnection: Keep-Alive\r\nContent-length: 8589934590\r\nHost: {host}\r\n\r\n"
        )
        .as_bytes(),
    )?;
    let mut buf = [0u8; 4096];
    let _ = tls.read(&mut buf)?;
    tls.write_all(&frame8(b"0"))?;
    tls.write_all(&frame8(nonce.as_bytes()))?;
    // read command 19
    let mut hdr = [0u8; 4];
    tls.read_exact(&mut hdr)?;
    let plen = u32::from_le_bytes(hdr) as usize;
    let mut body = vec![0u8; plen];
    tls.read_exact(&mut body)?;
    Ok(body[1..].to_vec())
}

/// Raw TCP + AES session channel.
pub struct Session {
    sock: TcpStream,
    password: String,
}

impl Session {
    pub fn connect(host: &str, port: u16, password: &str) -> std::io::Result<Self> {
        let sock = TcpStream::connect((host, port))?;
        sock.set_nodelay(true)?;
        Ok(Self {
            sock,
            password: password.to_string(),
        })
    }

    /// Send plaintext: AES-encrypt + frame `[u32 len][cipher]`.
    pub fn send(&mut self, plain: &[u8]) -> std::io::Result<()> {
        let ct = aes_encrypt(&self.password, plain);
        let mut frame = Vec::with_capacity(4 + ct.len());
        frame.extend_from_slice(&(ct.len() as u32).to_le_bytes());
        frame.extend_from_slice(&ct);
        self.sock.write_all(&frame)
    }

    /// Receive and AES-decrypt.
    pub fn recv(&mut self) -> std::io::Result<Option<Vec<u8>>> {
        let mut hdr = [0u8; 4];
        match self.sock.read_exact(&mut hdr) {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
            Err(e) => return Err(e),
        }
        let n = u32::from_le_bytes(hdr) as usize;
        let mut data = vec![0u8; n];
        self.sock.read_exact(&mut data)?;
        Ok(Some(aes_decrypt(&self.password, &data)))
    }
}
