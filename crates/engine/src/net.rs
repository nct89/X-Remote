//! Control channel: TLS -> HTTP POST -> login (cmd 3) -> id lookup (cmd 8) -> parse response.

use crate::msg::{build_login, build_tra};
use md5::Digest;
use rustls::pki_types::ServerName;
use rustls::{ClientConfig, ClientConnection, StreamOwned};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::Arc;

const CTRL_HOST: &str = "functions3.ultraviewer.net";
const CTRL_PORT: u16 = 443;

/// TLS with a verifier that accepts any cert (relays use certs we cannot validate).
fn client_config() -> Arc<ClientConfig> {
    let config = ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(NoVerify))
        .with_no_client_auth();
    Arc::new(config)
}

#[derive(Debug)]
pub struct NoVerify;

impl rustls::client::danger::ServerCertVerifier for NoVerify {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
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
        use rustls::SignatureScheme::*;
        vec![
            ECDSA_NISTP256_SHA256,
            ECDSA_NISTP384_SHA384,
            ED25519,
            RSA_PSS_SHA256,
            RSA_PSS_SHA384,
            RSA_PSS_SHA512,
            RSA_PKCS1_SHA256,
            RSA_PKCS1_SHA384,
            RSA_PKCS1_SHA512,
        ]
    }
}

/// Control channel: TLS + read/write frames.
pub struct Control {
    tls: StreamOwned<ClientConnection, TcpStream>,
}

impl Control {
    pub fn connect() -> std::io::Result<Self> {
        let tcp = TcpStream::connect((CTRL_HOST, CTRL_PORT))?;
        let name = ServerName::try_from(CTRL_HOST.to_string())
            .map_err(|_| std::io::Error::other("invalid server name"))?;
        let conn = ClientConnection::new(client_config(), name)
            .map_err(|_| std::io::Error::other("tls config"))?;
        let mut tls = StreamOwned::new(conn, tcp);
        tls.write_all(
            b"POST /UVService/ HTTP/1.1\r\nUser-Agent: Mozilla/4.0\r\n\
              Content-Type: application/octet-stream\r\nConnection: Keep-Alive\r\n\
              Content-length: 8589934590\r\nHost: functions3.ultraviewer.net\r\n\r\n",
        )?;
        let mut buf = [0u8; 4096];
        let _n = tls.read(&mut buf)?;
        Ok(Self { tls })
    }

    pub fn send(&mut self, frame: &[u8]) -> std::io::Result<()> {
        self.tls.write_all(frame)
    }

    /// Read a response frame `[u32 len][u8 type][body]`.
    pub fn recv_frame(&mut self) -> std::io::Result<Option<(u8, Vec<u8>)>> {
        let mut hdr = [0u8; 4];
        match self.tls.read_exact(&mut hdr) {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
            Err(e) => return Err(e),
        }
        let plen = u32::from_le_bytes(hdr) as usize;
        let mut data = vec![0u8; plen];
        self.tls.read_exact(&mut data)?;
        Ok(Some((data[0], data[1..].to_vec())))
    }
}

/// Full id lookup: returns the `8:1:0:<addr>` message as a `Vec<String>` (code at index 1).
#[allow(clippy::too_many_arguments)]
pub fn lookup(
    partner_id: &str,
    partner_pw: &str,
    own_id: &str,
    own_pw: &str,
) -> std::io::Result<Vec<String>> {
    // Identity is read from this machine (no hardcoded host/user).
    let lan = std::net::UdpSocket::bind("0.0.0.0:0")
        .and_then(|s| s.connect("8.8.8.8:80").and_then(|_| s.local_addr()))
        .map(|a| a.ip().to_string())
        .unwrap_or_else(|_| "0.0.0.0".into());
    let env = |ks: &[&str], default: &str| {
        ks.iter().find_map(|k| std::env::var(k).ok().filter(|v| !v.trim().is_empty())).unwrap_or_else(|| default.into())
    };
    let host_name = env(&["COMPUTERNAME", "HOSTNAME"], "X-REMOTE");
    let user = env(&["USER", "USERNAME", "LOGNAME"], "user");
    let machine_id: String = md5::Md5::digest(format!("x-remote/machine/{host_name}/{user}").as_bytes())
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect();
    let mut c = Control::connect()?;
    c.send(&build_login(&machine_id, "0", &lan, &host_name, &user, own_id, own_pw))?;
    let _ = c.recv_frame()?;
    c.send(&build_tra(
        partner_id,
        partner_pw,
        own_id,
        own_pw,
        0,
        &lan,
        0,
        &lan,
        0,
    ))?;
    match c.recv_frame()? {
        Some((_, body)) => Ok(String::from_utf8_lossy(&body)
            .split(':')
            .map(|s| s.to_string())
            .collect()),
        None => Ok(vec!["timeout".to_string()]),
    }
}
