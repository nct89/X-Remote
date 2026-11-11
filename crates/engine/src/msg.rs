//! Control-channel message builders: login (cmd 3) and id-lookup (cmd 8), 3DES.

use crate::tdes_encrypt;
use base64::Engine;
use md5::{Digest, Md5};
use rand::Rng;

const VERSION: &str = "6.6.124";

fn md5_hex(s: &[u8]) -> String {
    Md5::digest(s)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// .NET DateTime.UtcNow.Ticks (100ns since 0001-01-01).
fn net_ticks() -> i64 {
    let unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    (unix + 62_135_596_800) * 10_000_000
}

/// Frame a control-channel message: `[u32 len][u16 type=1][3DES body]`.
pub fn frame_tdes(body: &[u8]) -> Vec<u8> {
    let ct = tdes_encrypt(body);
    let mut out = Vec::with_capacity(6 + ct.len());
    out.extend_from_slice(&((ct.len() + 2) as u32).to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&ct);
    out
}

/// Login, command 3 (3DES).
#[allow(clippy::too_many_arguments)]
pub fn build_login(
    machine_id: &str,
    port: &str,
    local_ip: &str,
    machine_name: &str,
    username: &str,
    own_id: &str,
    _own_pw: &str,
) -> Vec<u8> {
    let ts = net_ticks();
    let token: String = rand::thread_rng()
        .sample_iter(rand::distributions::Alphanumeric)
        .take(16)
        .map(char::from)
        .collect();
    let b64 = base64::engine::general_purpose::STANDARD.encode(machine_name.as_bytes());
    // MAC-style hardware id derived from the machine id.
    let hw: String = machine_id.chars().take(12).collect();
    let body = format!(
        "3:{}:{}:0:{}:0:{}:{}:{hw}:{hw}-{}-{}:9600|6|0|3|0|0::::0:vi:{}:1:1:{}:0:1:{}:{}:{}",
        machine_id, port, local_ip, b64, VERSION, ts, token, username, own_id,
        ts + 10_000_000, ts, ts + 1_000_000_000
    );
    frame_tdes(body.as_bytes())
}

/// Id lookup, command 8 (3DES).
#[allow(clippy::too_many_arguments)]
pub fn build_tra(
    partner_id: &str,
    partner_pw: &str,
    own_id: &str,
    own_pw: &str,
    counter: i32,
    public_ip: &str,
    public_port: i32,
    local_ip: &str,
    local_port: i32,
) -> Vec<u8> {
    let f3 = md5_hex(format!("{}{}", " ".repeat(32), partner_pw).as_bytes());
    let f8 = md5_hex(format!("{own_id}{own_pw}").as_bytes());
    let token: Vec<u8> = (0..10).map(|_| rand::thread_rng().gen::<u8>()).collect();
    let mut body = format!(
        "8:{}:{}:{}:::{}:{}:-1:{}:0:0:0:420:1::{}|{}|{}|{}|",
        partner_id, f3, VERSION, own_id, f8, counter, public_ip, public_port, local_ip, local_port
    )
    .into_bytes();
    body.extend_from_slice(&token);
    frame_tdes(&body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_hashes_are_stable() {
        // Field 3 = md5(32 spaces ++ partner_pw); stable for a fixed input.
        let f3 = md5_hex(format!("{}{}", " ".repeat(32), "11111111").as_bytes());
        assert_eq!(f3, "f2ecd41d2a7eafec299fa922e73d3746");
        // Field 8 = md5(own_id ++ own_pw).
        let f8 = md5_hex(b"12345678material");
        assert_eq!(f8.len(), 32);
    }
}
