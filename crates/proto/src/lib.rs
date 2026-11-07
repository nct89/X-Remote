//! Wire protocol between the Windows agent and the native client.
//!
//! Framing (little endian):
//!
//! ```text
//! [u32 payload_len][u8 kind][payload...]
//! ```
//!
//! `kind` 1 = JSON control message (`Ctl` client->agent, `Event` agent->client)
//! `kind` 2 = video frame: `[u16 w][u16 h][u32 seq][jpeg bytes]`
//!
//! Control messages, client -> agent (`Ctl.t`):
//!   mouse / key / login  — input and the login macro.
//!   quality  — `{t:"quality", v:1..95}` change JPEG quality live.
//!   fps      — `{t:"fps", v:1..60}` change max frame rate live.
//!   ping     — `{t:"ping", v:<id>}` round-trip probe; agent replies pong.
//!   clip     — `{t:"clip", s:"<text>"}` push the client clipboard to the guest.
//!
//! Events, agent -> client (`Event.t`):
//!   hello / info / error — session lifecycle and messages.
//!   pong     — `{t:"pong", w:<id>}` echoes a ping id.
//!   clip     — `{t:"clip", s:"<text>"}` the guest clipboard changed.

use serde::{Deserialize, Serialize};
use std::io::{self, Read, Write};

pub const KIND_CTL: u8 = 1;
pub const KIND_FRAME: u8 = 2;

/// Upper bound on a single message, to stop a hostile peer from OOM-ing us.
pub const MAX_MSG: usize = 32 * 1024 * 1024;

pub fn write_msg<W: Write>(w: &mut W, kind: u8, payload: &[u8]) -> io::Result<()> {
    let mut header = [0u8; 5];
    header[0..4].copy_from_slice(&(payload.len() as u32).to_le_bytes());
    header[4] = kind;
    w.write_all(&header)?;
    w.write_all(payload)?;
    w.flush()
}

pub fn read_msg<R: Read>(r: &mut R) -> io::Result<(u8, Vec<u8>)> {
    let mut header = [0u8; 5];
    r.read_exact(&mut header)?;
    let len = u32::from_le_bytes([header[0], header[1], header[2], header[3]]) as usize;
    if len > MAX_MSG {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("message too large: {len}"),
        ));
    }
    let mut payload = vec![0u8; len];
    r.read_exact(&mut payload)?;
    Ok((header[4], payload))
}

/// Client -> agent control message. One flat struct keeps the JSON small and
/// the parsing trivially backwards compatible when fields are added.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Ctl {
    /// "login" | "mouse" | "key" | "ping"
    pub t: String,
    /// mouse: "move" | "down" | "up" | "wheel"
    /// key:   "char" | "vk"
    #[serde(default)]
    pub k: String,
    /// mouse position, normalized 0..1 inside the captured window
    #[serde(default)]
    pub x: f32,
    #[serde(default)]
    pub y: f32,
    /// mouse button index: 0 left, 1 middle, 2 right
    #[serde(default)]
    pub b: u8,
    /// wheel delta
    #[serde(default)]
    pub d: f32,
    /// character for k="char"
    #[serde(default)]
    pub c: String,
    /// virtual key code for k="vk"
    #[serde(default)]
    pub v: u16,
    #[serde(default)]
    pub down: bool,
    /// login: UltraViewer id / password
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub pw: String,
    /// Generic string payload. Clipboard text for t="clip".
    #[serde(default)]
    pub s: String,
}

/// Agent -> client event.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Event {
    /// "hello" | "error" | "info" | "pong" | "clip"
    pub t: String,
    #[serde(default)]
    pub w: u32,
    #[serde(default)]
    pub h: u32,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub msg: String,
    /// Generic string payload. Clipboard text for t="clip".
    #[serde(default)]
    pub s: String,
}

pub fn encode_frame(w: u16, h: u16, seq: u32, jpeg: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(8 + jpeg.len());
    out.extend_from_slice(&w.to_le_bytes());
    out.extend_from_slice(&h.to_le_bytes());
    out.extend_from_slice(&seq.to_le_bytes());
    out.extend_from_slice(jpeg);
    out
}

/// Returns `(width, height, seq, jpeg)`.
pub fn decode_frame(payload: &[u8]) -> Option<(u16, u16, u32, &[u8])> {
    if payload.len() < 8 {
        return None;
    }
    let w = u16::from_le_bytes([payload[0], payload[1]]);
    let h = u16::from_le_bytes([payload[2], payload[3]]);
    let seq = u32::from_le_bytes([payload[4], payload[5], payload[6], payload[7]]);
    Some((w, h, seq, &payload[8..]))
}
