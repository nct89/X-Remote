//! UltraViewer protocol, in Rust.
//!
//! Port of the proven Python engine (`tools/uv_native.py`) so the workspace
//! egui client can call it directly, with no helper process.
//!
//! * [`crypto`] — 3DES on the control channel, AES-256-CBC on the session channel.
//! * [`control`] — TLS to the UltraViewer server: login, ID lookup, wait for the peer.
//! * [`client`] — wires those steps into one session.
//! * [`multipart`] — repeated `[u32 len][part]` frames.
//! * [`frame`] — decode a frame (zstd + RGB555 + extra JPEG atlas).
//! * [`input`] — encode keyboard and mouse for the peer.
//! * [`layout`] — the peer's multi-monitor layout.
//! * [`chat`] — chat messages on the session channel.

// Host mode (the controlled side): OS screen capture and input injection, same API.
#[cfg(target_os = "linux")]
pub mod capture;
#[cfg(target_os = "macos")]
#[path = "capture_mac.rs"]
pub mod capture;
#[cfg(target_os = "linux")]
pub mod clipboard;
#[cfg(target_os = "macos")]
#[path = "clipboard_mac.rs"]
pub mod clipboard;
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub mod hostrun;
#[cfg(target_os = "linux")]
pub mod inject;
#[cfg(target_os = "macos")]
#[path = "inject_mac.rs"]
pub mod inject;
pub mod input_cmd;
pub mod chat;
pub mod client;
pub mod control;
pub mod crypto;
pub mod diag;
pub mod ident;
pub mod frame;
pub mod host;
pub mod input;
pub mod keymap;
pub mod layout;
pub mod multipart;
pub mod relay;
pub mod session;

pub use client::{connect, Connected};
pub use control::{build_login, build_login_host, build_tra, Control, VERSION};
pub use host::{Cmd17, HostControl, HostSession};
pub use crypto::{aes_decrypt, aes_encrypt, tdes_encrypt};
pub use ident::Ident;
pub use frame::{Frame, Region};
pub use layout::{parse_monitors, Monitor};
pub use multipart::{decode as decode_multipart, encode as encode_multipart};
pub use session::Session;
