//! Crypto: 3DES for the control channel, AES-256-CBC for the session channel.
//!
//! * 3DES key fetched from the project key server (see [`load_key_iv`]);
//! * AES key derived from the session password via MD5;
//! * both apply PKCS#7 manually and disable the library padding.
#![cfg_attr(test, allow(dead_code, unused_imports))]

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::{Duration, SystemTime};

use aes::cipher::{block_padding::NoPadding, BlockDecryptMut, BlockEncryptMut, KeyIvInit};
use md5::{Digest, Md5};
use rand::RngCore;

type Aes256CbcEnc = cbc::Encryptor<aes::Aes256>;
type Aes256CbcDec = cbc::Decryptor<aes::Aes256>;
type TdesCbcEnc = cbc::Encryptor<des::TdesEde3>;

/// Key service (primary domain, then the direct host as a fallback).
const KEY_HOSTS: [&str; 2] = ["x-remote-key.xos.li", "dsh.144-91-77-44.sslip.io"];
const KEY_PATH: &str = "/xr/service.env";
/// Re-check the server after this long; a stale cache is still used if offline.
const CACHE_TTL: Duration = Duration::from_secs(6 * 60 * 60);

/// Built-in protocol key/IV (`sha1(utf16le("\u{5}\u{10}\u{15}\u{20}\u{25}")) ++ 4 zero bytes`,
/// `sha1("")[..8]`), used for offline unit tests and as a fallback if the key server is
/// unreachable on first launch.
const DEFAULT_KEY: [u8; 24] = [
    0x40, 0x9a, 0x19, 0x4e, 0xaa, 0x8b, 0x6a, 0x12, 0xb9, 0x5a, 0x60, 0x48, 0x6d, 0x47, 0xc6, 0x50,
    0xaa, 0x81, 0xbc, 0x89, 0x00, 0x00, 0x00, 0x00,
];
const DEFAULT_IV: [u8; 8] = [0xda, 0x39, 0xa3, 0xee, 0x5e, 0x6b, 0x4b, 0x0d];

fn cache_path() -> Option<PathBuf> {
    if cfg!(windows) {
        if let Some(appdata) = std::env::var_os("APPDATA").filter(|v| !v.is_empty()) {
            return Some(PathBuf::from(appdata).join("X-Remote").join("key.cache"));
        }
    }
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .or_else(|| std::env::var_os("USERPROFILE"))
                .filter(|v| !v.is_empty())
                .map(|h| PathBuf::from(h).join(".config"))
        })?;
    Some(base.join("x-remote").join("key.cache"))
}

fn parse_env(text: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        let k = k.trim();
        if k.is_empty() {
            continue;
        }
        let mut v = v.trim().to_string();
        if v.len() >= 2
            && ((v.starts_with('"') && v.ends_with('"')) || (v.starts_with('\'') && v.ends_with('\'')))
        {
            v = v[1..v.len() - 1].to_string();
        }
        out.insert(k.to_string(), v);
    }
    out
}

fn cache_fresh() -> bool {
    let Some(path) = cache_path() else { return false };
    let Ok(meta) = std::fs::metadata(&path) else { return false };
    let Ok(modified) = meta.modified() else { return false };
    SystemTime::now()
        .duration_since(modified)
        .map(|age| age < CACHE_TTL)
        .unwrap_or(false)
}

fn write_cache(text: &str) {
    let Some(path) = cache_path() else { return };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if std::fs::write(&path, text).is_ok() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
        }
    }
}

fn decode_chunked(body: &str) -> Option<String> {
    let mut out = String::new();
    let mut rest = body;
    loop {
        let (size_line, tail) = rest.split_once("\r\n")?;
        let size = usize::from_str_radix(size_line.trim(), 16).ok()?;
        if size == 0 {
            return Some(out);
        }
        if tail.len() < size {
            return None;
        }
        out.push_str(&tail[..size]);
        rest = tail[size..].strip_prefix("\r\n").unwrap_or(&tail[size..]);
    }
}

/// Fetch the key file over TLS. The endpoint shares the accept-any certificate
/// policy of the control channel: a tampered answer can break the connection,
/// but the host is compiled in, so it cannot redirect the client elsewhere.
fn fetch_from(host: &str) -> Option<String> {
    let addr = (host, 443).to_socket_addrs().ok()?.next()?;
    let sock = TcpStream::connect_timeout(&addr, Duration::from_secs(3)).ok()?;
    sock.set_read_timeout(Some(Duration::from_secs(4))).ok()?;
    sock.set_write_timeout(Some(Duration::from_secs(4))).ok()?;
    sock.set_nodelay(true).ok();
    let mut tls = crate::control::tls_wrap_relay(sock, host).ok()?;
    let req = format!(
        "GET {KEY_PATH} HTTP/1.1\r\nHost: {host}\r\nUser-Agent: X-Remote\r\nAccept: text/plain\r\nConnection: close\r\n\r\n"
    );
    tls.write_all(req.as_bytes()).ok()?;
    tls.flush().ok()?;
    let mut raw = Vec::new();
    tls.read_to_end(&mut raw).ok()?;
    let text = String::from_utf8_lossy(&raw).to_string();
    let (head, body) = text.split_once("\r\n\r\n")?;
    let head = head.to_ascii_lowercase();
    if !head.starts_with("http/1.1 200") && !head.starts_with("http/1.0 200") {
        return None;
    }
    if head.contains("transfer-encoding: chunked") {
        decode_chunked(body)
    } else {
        Some(body.to_string())
    }
}

fn fetch_remote() -> Option<String> {
    KEY_HOSTS.iter().find_map(|host| fetch_from(host))
}

fn hex_array<const N: usize>(raw: Option<&str>) -> Option<[u8; N]> {
    let s = raw?.trim();
    if s.len() != N * 2 {
        return None;
    }
    let mut out = [0u8; N];
    for i in 0..N {
        out[i] = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(out)
}

/// Control-channel key/IV: environment, then cache, then the key server, then built-in default.
#[cfg(not(test))]
fn load_key_iv() -> ([u8; 24], [u8; 8]) {
    static CELL: OnceLock<([u8; 24], [u8; 8])> = OnceLock::new();
    *CELL.get_or_init(|| {
        let env = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
        let (env_key, env_iv) = (env("XREMOTE_CTL_KEY"), env("XREMOTE_CTL_IV"));
        if let (Some(k), Some(iv)) = (&env_key, &env_iv) {
            if let (Some(kb), Some(ib)) = (hex_array::<24>(Some(k)), hex_array::<8>(Some(iv))) {
                return (kb, ib);
            }
        }

        let mut map = HashMap::new();
        let cached = cache_path().and_then(|p| std::fs::read_to_string(p).ok());
        if let Some(text) = &cached {
            map.extend(parse_env(text));
        }
        let have = map.contains_key("XR_CTL_KEY") && map.contains_key("XR_CTL_IV");
        if !have || !cache_fresh() {
            if let Some(text) = fetch_remote() {
                write_cache(&text);
                map.extend(parse_env(&text));
            }
        }

        let key = env_key.or_else(|| map.get("XR_CTL_KEY").cloned());
        let iv = env_iv.or_else(|| map.get("XR_CTL_IV").cloned());
        match (hex_array::<24>(key.as_deref()), hex_array::<8>(iv.as_deref())) {
            (Some(kb), Some(ib)) => (kb, ib),
            _ => (DEFAULT_KEY, DEFAULT_IV),
        }
    })
}

#[cfg(test)]
fn load_key_iv() -> ([u8; 24], [u8; 8]) {
    (DEFAULT_KEY, DEFAULT_IV)
}

/// Session AES-256 key derived from the password.
///
/// `md5` fills bytes 0..16, then is copied again into 15..31 (byte 15 is overwritten,
/// matching the original), last byte = 0.
pub fn aes_key_from_password(password: &str) -> [u8; 32] {
    let digest = Md5::digest(password.as_bytes());
    let mut key = [0u8; 32];
    key[0..16].copy_from_slice(&digest);
    key[15..31].copy_from_slice(&digest);
    key
}

fn pkcs7(data: &[u8], block: usize) -> Vec<u8> {
    let n = block - data.len() % block;
    let mut out = Vec::with_capacity(data.len() + n);
    out.extend_from_slice(data);
    out.extend(std::iter::repeat(n as u8).take(n));
    out
}

/// 3DES-CBC encrypt (control channel), block-8 padding included.
pub fn tdes_encrypt(plain: &[u8]) -> Vec<u8> {
    let (key, iv) = load_key_iv();
    let padded = pkcs7(plain, 8);
    TdesCbcEnc::new(&key.into(), &iv.into()).encrypt_padded_vec_mut::<NoPadding>(&padded)
}

/// AES-256-CBC encrypt (session channel): returns `IV || ciphertext`.
pub fn aes_encrypt(password: &str, plain: &[u8]) -> Vec<u8> {
    let mut iv = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut iv);
    let padded = pkcs7(plain, 16);
    let ct = Aes256CbcEnc::new(&aes_key_from_password(password).into(), &iv.into())
        .encrypt_padded_vec_mut::<NoPadding>(&padded);
    let mut out = Vec::with_capacity(16 + ct.len());
    out.extend_from_slice(&iv);
    out.extend_from_slice(&ct);
    out
}

/// AES-256-CBC decrypt: input is `IV || ciphertext`, strips PKCS#7 padding.
pub fn aes_decrypt(password: &str, data: &[u8]) -> Vec<u8> {
    if data.len() < 32 || (data.len() - 16) % 16 != 0 {
        return Vec::new();
    }
    let (iv, ct) = data.split_at(16);
    let mut out = match Aes256CbcDec::new(&aes_key_from_password(password).into(), iv.into())
        .decrypt_padded_vec_mut::<NoPadding>(ct)
    {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    if let Some(&n) = out.last() {
        if (1..=16).contains(&n) && out.len() >= n as usize {
            out.truncate(out.len() - n as usize);
        }
    }
    out
}

/// Control-channel frame: `[u32 len][u16 = 1][3DES]`.
pub fn frame_tdes(body: &[u8]) -> Vec<u8> {
    let ct = tdes_encrypt(body);
    let mut out = Vec::with_capacity(6 + ct.len());
    out.extend_from_slice(&((ct.len() + 2) as u32).to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&ct);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aes_key_matches_original() {
        // md5("11111") = b0baee9d279d34fa1dfd71aadb908c3f
        let d = hex("b0baee9d279d34fa1dfd71aadb908c3f");
        let k = aes_key_from_password("11111");
        assert_eq!(&k[0..15], &d[0..15]);
        // byte 15 is overwritten by md5[0] — matches the original
        assert_eq!(k[15], d[0]);
        assert_eq!(&k[16..31], &d[1..16]);
        assert_eq!(k[31], 0);
    }

    #[test]
    fn aes_roundtrip() {
        for pw in ["11111", "60470", "Abc123xyz!@#"] {
            for len in [0usize, 1, 15, 16, 17, 100] {
                let plain = vec![7u8; len];
                let ct = aes_encrypt(pw, &plain);
                assert_eq!(ct.len() % 16, 0);
                assert_eq!(aes_decrypt(pw, &ct), plain, "pw={pw} len={len}");
            }
        }
    }

    #[test]
    fn aes_wrong_password_differs() {
        let ct = aes_encrypt("11111", b"hello");
        assert_ne!(aes_decrypt("22222", &ct), b"hello".to_vec());
    }

    #[test]
    fn tdes_frame_format() {
        let f = frame_tdes(b"abc");
        let n = u32::from_le_bytes([f[0], f[1], f[2], f[3]]) as usize;
        assert_eq!(n, f.len() - 4);
        assert_eq!(u16::from_le_bytes([f[4], f[5]]), 1);
        assert_eq!((f.len() - 6) % 8, 0, "ciphertext must be a multiple of 8");
    }

    #[test]
    fn chunked_key_file_is_decoded() {
        assert_eq!(decode_chunked("3\r\nabc\r\n0\r\n\r\n").as_deref(), Some("abc"));
    }

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }
}
