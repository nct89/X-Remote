//! Chat on the UltraViewer session channel.
//!
//! Measured on a real session (2026-11-05 19:59, UltraViewer 6.6.124 controller
//! → X-Remote host): every chat message is flag `01` + UTF-16LE (no BOM, no trailing NUL):
//!
//! | String | Meaning (inferred from the order seen) |
//! |---|---|
//! | `29` | controller opens / starts the chat frame |
//! | `5|||<text>` | NOT chat: **clipboard sync** (see [`clip_from_ctrl`]) |
//! | `12|||<text>` | **one message** |
//! | `30` | end of a typing / send turn |
//!
//! **Host → controller** is different (measured 2026-11-05 20:20 from real UltraViewer
//! host `100000001` with `examples/chat_probe`): flag `00` + **multipart**, part0 = `u16` command:
//!
//! | Multipart | Meaning |
//! |---|---|
//! | `[u16 29]` | start typing |
//! | `[u16 5][UTF-16LE preview]` | live typing preview |
//! | `[u16 12][UTF-16LE text]` | **message** |
//! | `[u16 30]` | stop typing |
//!
//! Sending the controller string `01`+`12|||…` from the host makes the controller
//! report `F1.ManipulatePack_Error: Type mismatch` (measured 20:14).

/// Chat event parsed from one session message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatEvent {
    /// Controller chat message.
    Msg(String),
    /// Other chat-frame control (`29`, `30`, `5|||…`); the string is kept.
    Ctrl(String),
}

/// Chat message prefix.
const MSG_PFX: &str = "12|||";

/// Decode the UTF-16LE text of a flag-`01` message. `None` if it is not that form.
fn utf16_text(msg: &[u8]) -> Option<String> {
    if msg.first() != Some(&1) || msg.len() < 3 || (msg.len() - 1) % 2 != 0 {
        return None;
    }
    let u: Vec<u16> = msg[1..].chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
    String::from_utf16(&u).ok()
}

/// Recognize a chat message (flag byte still present). Not chat → `None`.
pub fn parse_event(msg: &[u8]) -> Option<ChatEvent> {
    let s = utf16_text(msg)?;
    if let Some(t) = s.strip_prefix(MSG_PFX) {
        return Some(ChatEvent::Msg(t.to_string()));
    }
    if s == "29" || s == "30" {
        return Some(ChatEvent::Ctrl(s));
    }
    None
}

/// Clipboard the controller sends: `01` + UTF-16LE(`5|||<text>`). Measured 22:28: EVKey
/// composes accents via the clipboard, so the controller sends `5|||` plus the accented
/// text and then Ctrl+V. The host must set its own clipboard before that Ctrl+V, or the
/// accent is lost. Also seen when the controller copies (`5|||100000002` after copying an ID).
pub fn clip_from_ctrl(msg: &[u8]) -> Option<String> {
    utf16_text(msg)?.strip_prefix("5|||").map(str::to_string)
}

/// Chat message **controller → host**: `01` + UTF-16LE(`12|||<text>`).
pub fn controller_msg(text: &str) -> Vec<u8> {
    let mut out = vec![1u8];
    for u in MSG_PFX.encode_utf16().chain(text.encode_utf16()) {
        out.extend_from_slice(&u.to_le_bytes());
    }
    out
}

/// Multipart host → controller: `00` + `[u16 command]` (+ a UTF-16LE part when present).
fn host_parts(cmd: u16, text: Option<&str>) -> Vec<u8> {
    let l = cmd.to_le_bytes();
    let u: Vec<u8> = text.map(|s| s.encode_utf16().flat_map(|c| c.to_le_bytes()).collect()).unwrap_or_default();
    let body = match text {
        Some(_) => crate::multipart::encode(&[&l, &u]),
        None => crate::multipart::encode(&[&l]),
    };
    let mut out = Vec::with_capacity(1 + body.len());
    out.push(0u8);
    out.extend_from_slice(&body);
    out
}

/// Chat message **host → controller**: `00` + multipart `[u16 12][UTF-16LE text]`.
pub fn chat_msg(text: &str) -> Vec<u8> {
    host_parts(12, Some(text))
}

/// "Stop typing" host → controller (`[u16 30]`). A real host sends this right after each message.
pub fn type_end() -> Vec<u8> {
    host_parts(30, None)
}

/// "Start typing" host → controller (`[u16 29]`). The controller shows "typing"
/// (VB6 string "Send typing to" / `showtyping`).
pub fn type_start() -> Vec<u8> {
    host_parts(29, None)
}

/// Chat or clipboard message a HOST sends (used when this machine is the controller).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostEvent {
    /// `[u16 12][UTF-16]` — a message.
    Msg(String),
    /// `[u16 29]` start typing / `[u16 30]` stop typing.
    Typing(bool),
    /// `[u16 5][UTF-16]` — the host clipboard (seen when the host user copies / EVKey).
    Clipboard(String),
}

/// Parse a multipart message from the host (parts already split). Not chat/clipboard → `None`.
pub fn parse_from_host(parts: &[Vec<u8>]) -> Option<HostEvent> {
    let cmd = parts.first().filter(|p| p.len() >= 2).map(|p| u16::from_le_bytes([p[0], p[1]]))?;
    let text_of = || {
        let p = parts.get(1)?;
        let u: Vec<u16> = p.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        String::from_utf16(&u).ok()
    };
    match cmd {
        12 => text_of().map(HostEvent::Msg),
        29 => Some(HostEvent::Typing(true)),
        30 => Some(HostEvent::Typing(false)),
        5 => text_of().map(HostEvent::Clipboard),
        _ => None,
    }
}

/// "Typing" / "stopped typing" controller → host: `01` + UTF-16 `29` / `30` (measured 19:59).
pub fn controller_typing(typing: bool) -> Vec<u8> {
    let mut out = vec![1u8];
    for u in (if typing { "29" } else { "30" }).encode_utf16() {
        out.extend_from_slice(&u.to_le_bytes());
    }
    out
}

/// What the host needs to send to the controller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatOut {
    /// One message (also sends "stop typing", like a real host).
    Msg(String),
    /// The host user started (`true`) or stopped (`false`) typing.
    Typing(bool),
}

impl ChatOut {
    /// Session messages for this item, in send order.
    pub fn packets(&self) -> Vec<Vec<u8>> {
        match self {
            ChatOut::Msg(t) => vec![chat_msg(t), type_end()],
            ChatOut::Typing(true) => vec![type_start()],
            ChatOut::Typing(false) => vec![type_end()],
        }
    }
}

/// Chat link between the session loop and the UI.
pub trait ChatChannel {
    /// A chat event arrived from the controller.
    fn on_in(&mut self, event: ChatEvent);
    /// Take one item to send (call until `None`).
    fn poll_out(&mut self) -> Option<ChatOut>;
}

/// Empty channel: drop chat (for example the CLI).
pub struct NoChat;

impl ChatChannel for NoChat {
    fn on_in(&mut self, _event: ChatEvent) {}
    fn poll_out(&mut self) -> Option<ChatOut> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Captured host message (accented plaintext, kept as escapes so the bytes stay identical).
    const HOST_TEXT: &str = "xin ch\u{00e0}o \u{0111}\u{00e3} th\u{1ea5}y n\u{1ed9}i dung";

    fn from_hex(h: &str) -> Vec<u8> {
        h.split_whitespace().map(|b| u8::from_str_radix(b, 16).unwrap()).collect()
    }

    #[test]
    fn parse_real_controller_msg() {
        // Cleartext measured 2026-11-05 19:59 when the user sent "xin chao".
        let msg = from_hex(
            "01 31 00 32 00 7c 00 7c 00 7c 00 78 00 69 00 6e 00 20 00 63 00 68 00 61 00 6f 00",
        );
        assert_eq!(parse_event(&msg), Some(ChatEvent::Msg("xin chao".into())));
        assert_eq!(parse_event(&from_hex("01 32 00 39 00")), Some(ChatEvent::Ctrl("29".into())));
        // `5|||` is clipboard, not chat.
        assert_eq!(parse_event(&from_hex("01 35 00 7c 00 7c 00 7c 00 6f 00")), None);
        assert_eq!(clip_from_ctrl(&from_hex("01 35 00 7c 00 7c 00 7c 00 6f 00")), Some("o".into()));
        let mut raw = vec![1u8];
        for u in "5|||\u{00e0}o".encode_utf16() {
            raw.extend_from_slice(&u.to_le_bytes());
        }
        assert_eq!(clip_from_ctrl(&raw), Some("\u{00e0}o".into()));
        assert_eq!(clip_from_ctrl(&from_hex("01 32 00 39 00")), None);
    }

    #[test]
    fn parse_real_host_msg() {
        // Cleartext a real UltraViewer host (100000001) sent the controller, measured 20:20.
        let msg = from_hex(
            "00 02 00 00 00 0c 00 32 00 00 00 78 00 69 00 6e 00 20 00 63 00 68 00 e0 00 6f 00 20 00 \
             11 01 e3 00 20 00 74 00 68 00 a5 1e 79 00 20 00 6e 00 d9 1e 69 00 20 00 64 00 75 00 6e 00 67 00",
        );
        let parts = crate::multipart::decode(&msg[1..]).unwrap();
        assert_eq!(parse_from_host(&parts), Some(HostEvent::Msg(HOST_TEXT.into())));
        let p = |h: &str| crate::multipart::decode(&from_hex(h)[1..]).unwrap();
        assert_eq!(parse_from_host(&p("00 02 00 00 00 1d 00")), Some(HostEvent::Typing(true)));
        assert_eq!(parse_from_host(&p("00 02 00 00 00 1e 00")), Some(HostEvent::Typing(false)));
        assert_eq!(
            parse_from_host(&p("00 02 00 00 00 05 00 04 00 00 00 a5 1e 79 00")),
            Some(HostEvent::Clipboard("\u{1ea5}y".into()))
        );
        // A picture frame (command 1) is not chat.
        assert_eq!(parse_from_host(&p("00 02 00 00 00 01 00")), None);
        assert_eq!(controller_typing(true), from_hex("01 32 00 39 00"));
        assert_eq!(controller_typing(false), from_hex("01 33 00 30 00"));
    }

    #[test]
    fn ignores_non_chat() {
        assert_eq!(parse_event(b"\x0018"), None, "flag 0 is not chat");
        assert_eq!(parse_event(b"\x0020"), None);
        assert_eq!(parse_event(b"\x008|||512|||0|||1|||2"), None);
        // Session login (flag 1, UTF-16) is not chat.
        let mut login = vec![1u8];
        for u in "login:1:2".encode_utf16() {
            login.extend_from_slice(&u.to_le_bytes());
        }
        assert_eq!(parse_event(&login), None);
    }

    #[test]
    fn controller_msg_roundtrip() {
        let text = "Ch\u{00e0}o b\u{1ea1}n \u{2014} \u{1ed5}n \u{2713}";
        let m = controller_msg(text);
        assert_eq!(m[0], 1);
        assert_eq!(parse_event(&m), Some(ChatEvent::Msg(text.into())));
        // Byte-for-byte the form the controller sends.
        assert_eq!(
            controller_msg("xin chao"),
            from_hex("01 31 00 32 00 7c 00 7c 00 7c 00 78 00 69 00 6e 00 20 00 63 00 68 00 61 00 6f 00")
        );
    }

    #[test]
    fn host_msg_matches_bytes() {
        // Cleartext a real UltraViewer host sent for HOST_TEXT (measured 20:20).
        let captured = from_hex(
            "00 02 00 00 00 0c 00 32 00 00 00 78 00 69 00 6e 00 20 00 63 00 68 00 e0 00 6f 00 20 00 \
             11 01 e3 00 20 00 74 00 68 00 a5 1e 79 00 20 00 6e 00 d9 1e 69 00 20 00 64 00 75 00 6e 00 67 00",
        );
        assert_eq!(chat_msg(HOST_TEXT), captured);
        assert_eq!(type_end(), from_hex("00 02 00 00 00 1e 00"));
        // Real host "start typing": `00 02 00 00 00 1d 00` (measured 20:20, 28.4 s).
        assert_eq!(type_start(), from_hex("00 02 00 00 00 1d 00"));
        assert_eq!(ChatOut::Msg("x".into()).packets(), vec![chat_msg("x"), type_end()]);
        assert_eq!(ChatOut::Typing(true).packets(), vec![type_start()]);
        // The host must NOT send the controller's string form (that causes "Type mismatch").
        assert_ne!(chat_msg("x")[0], 1);
    }
}
