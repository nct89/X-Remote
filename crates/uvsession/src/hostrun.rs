//! Serve one controlled session (Linux, macOS): capture the screen and
//! apply mouse/keys from the controller. Shared by `host_listen` and the
//! X-Remote UI.
//!
//! Timing from a real UltraViewer pcap (Wine host, Windows controller,
//! stable for 20 minutes): the controller pulls frames; the host only replies.
//! - `18` → host sends exactly one command-1 frame (27 requests, 27 frames).
//! - `20` → host sends pong `[u16 20]` (125 requests, 124 pongs). Without a
//!   pong the controller shows "connection lost, waiting for signal" and
//!   resets after ~16 seconds.
//! - The host does NOT send ack `3:n` and does NOT push frames. Extra frames
//!   stall the controller so it stops requesting `18`.

use std::time::{Duration, Instant};

use crate::capture::Capturer;
use crate::frame::{self};
use crate::host::HostSession;
use crate::chat::{ChatChannel, NoChat};
use crate::inject::{self, InputInjector, InputCmd};

/// If no `18` arrives within this long after init, send one first frame,
/// in case the controller waits for it before requesting more.
const FIRST_FRAME_WAIT: Duration = Duration::from_secs(2);

/// Extra tile rows (32 px) refreshed in each frame: 1080p is 34 rows, so
/// the screen is covered in 9 frames (~0.5 s at ~20 `18`/s).
const SCAN_ROWS: usize = 4;

/// Fast full scans when the stream opens: pass 1 drops some tiles, pass 2 fills them.
const FAST_PASSES: u32 = 2;

/// After the fast passes, refresh 1 row every [`IDLE_STRIDE`] frames, so the
/// whole screen updates about every 136 frames (~5 s). Repairs tiles the
/// controller dropped or drew wrong without much bandwidth when the screen
/// is idle (measured 19:59: 1 row/frame was ~590 KB/s even with no changes).
const IDLE_ROWS: usize = 1;
const IDLE_STRIDE: u64 = 4;

/// Session-channel messages the host must answer.
#[derive(Debug, PartialEq, Eq)]
pub enum SessCmd {
    /// `18` — request one frame.
    WantFrame,
    /// `20` — ping; must pong.
    Ping,
    /// `19` — controller is disconnecting (sent just before close).
    Stop,
    /// Anything else (mouse/keys, `17`, `4|||n`, login, ...).
    Other,
}

/// Classify a message body (flag byte already removed).
pub fn classify(body: &[u8]) -> SessCmd {
    match body {
        b"18" => SessCmd::WantFrame,
        b"20" => SessCmd::Ping,
        b"19" => SessCmd::Stop,
        _ => SessCmd::Other,
    }
}

/// Serve until the controller disconnects. `halt` returns true to stop early
/// (for example the user turns the host off). `on_frame(n)` runs after each
/// frame is sent (may be a no-op).
pub fn serve_session(
    hs: &mut HostSession,
    cap: &Capturer,
    inj: Option<&InputInjector>,
    halt: impl FnMut() -> bool,
    on_frame: impl FnMut(i32),
) -> Result<(), String> {
    serve_session_chat(hs, cap, inj, halt, on_frame, &mut NoChat)
}

/// Like [`serve_session`], plus a chat channel with the controller.
pub fn serve_session_chat(
    hs: &mut HostSession,
    cap: &Capturer,
    inj: Option<&InputInjector>,
    mut halt: impl FnMut() -> bool,
    mut on_frame: impl FnMut(i32),
    chat: &mut dyn ChatChannel,
) -> Result<(), String> {
    let mut prev: Option<Vec<u8>> = None;
    // Next tile row of the full-screen refresh (`None` = not scanning).
    let mut scan: Option<usize> = Some(0);
    let mut rows = SCAN_ROWS;
    let mut fast_left = FAST_PASSES;
    let clip = match crate::clipboard::ClipboardOwner::new() {
        Ok(c) => Some(c),
        Err(e) => {
            eprintln!("[uv-host] cannot open clipboard: {e}");
            None
        }
    };
    let mut no_scan: Option<usize> = None;
    hs.session.set_timeout(Duration::from_millis(200)).ok();
    // The controller can leave while a frame is being sent. With no write
    // timeout, `send` blocks until TCP gives up (tens of seconds) and the
    // UI stays on "Connecting".
    hs.session.set_write_timeout(Duration::from_secs(2)).ok();
    let mut n = 0i32;

    // Command 0 (init) — the real controller waits for this to build the canvas.
    let host_name = crate::Ident::current().host_name;
    let init = frame::init_message(cap.width as u32, cap.height as u32, &host_name);
    hs.session.send(&init).map_err(|e| format!("send init failed: {e}"))?;
    let started = Instant::now();
    eprintln!("[uv-host] sent init, waiting for controller frame request (18)");
    // A full-screen frame only "counts" when it answers an `18`. A frame pushed
    // before the controller is ready is dropped; if that frame is treated as the
    // baseline, later dirty frames paint only the changes and the rest stays
    // black (measured 05/11 19:20).
    let mut need_full = true;
    let mut sent_first = false;
    let mut n18 = 0u64;
    let mut n20 = 0u64;
    let mut stats = FrameStats::default();

    loop {
        if halt() {
            return Ok(());
        }
        while let Some(out) = chat.poll_out() {
            // Like a real host: text is `[u16 12][chars]` then "stopped typing"
            // `[u16 30]`; typing is `[u16 29]` / `[u16 30]`.
            for b in out.packets() {
                hs.session.send(&b).map_err(|e| format!("send chat failed: {e}"))?;
            }
            eprintln!("[uv-host] chat out: {out:?}");
        }
        match hs.session.recv() {
            Ok(Some(msg)) => {
                // Controller clipboard (`5|||…`): apply NOW, before later keys
                // (EVKey sends accented text as clipboard + Ctrl+V).
                if let Some(text) = crate::chat::clip_from_ctrl(&msg) {
                    let ok = clip.as_ref().map(|c| c.set(&text)).unwrap_or(false);
                    eprintln!("[uv-host] clipboard <- controller ({} chars): {}", text.chars().count(), if ok { "ok" } else { "FAIL" });
                    continue;
                }
                if let Some(ev) = crate::chat::parse_event(&msg) {
                    eprintln!("[uv-host] chat in: {ev:?}");
                    chat.on_in(ev);
                    continue;
                }
                let body = match msg.first() {
                    Some(0) | Some(1) => &msg[1..],
                    _ => &msg[..],
                };
                match classify(body) {
                    SessCmd::Stop => {
                        eprintln!("[uv-host] controller stopped (19)");
                        return Ok(());
                    }
                    SessCmd::Ping => {
                        n20 += 1;
                        hs.session
                            .send(&frame::pong_message())
                            .map_err(|e| format!("send pong failed: {e}"))?;
                    }
                    SessCmd::WantFrame => {
                        n18 += 1;
                        if need_full {
                            scan = Some(0);
                            rows = SCAN_ROWS;
                            fast_left = FAST_PASSES;
                            need_full = false;
                        }
                        // Idle refresh only adds rows every IDLE_STRIDE frames.
                        let do_scan = rows != IDLE_ROWS || n18 % IDLE_STRIDE == 0;
                        let q = if do_scan { &mut scan } else { &mut no_scan };
                        send_frame(hs, cap, &mut prev, q, rows, &mut n, &mut stats)?;
                        if scan.is_none() {
                            // End of a pass: more fast passes rescan (repair tiles the
                            // controller dropped — a black band at the top), then a
                            // sparse idle scan repairs edges that went black when the
                            // controller scaled the image (horizontal stripes, 19:31).
                            fast_left = fast_left.saturating_sub(1);
                            scan = Some(0);
                            rows = if fast_left > 0 { SCAN_ROWS } else { IDLE_ROWS };
                        }
                        on_frame(n);
                        if n18 == 1 || n18 % 200 == 0 {
                            eprintln!(
                                "[uv-host] 18 #{n18} after {:.1}s, ponged {n20} times",
                                started.elapsed().as_secs_f32()
                            );
                        }
                    }
                    SessCmd::Other => {
                        if body == b"17" || body.starts_with(b"4|||") {
                            // Controller opened the screen stream again → next frame must be full.
                            need_full = true;
                        }
                        if let Some(inj) = inj {
                            let cmd = inject::parse_cmd(body);
                            if !matches!(cmd, InputCmd::Other) {
                                if matches!(cmd, InputCmd::Btn { .. } | InputCmd::Key { .. }) {
                                    eprintln!(
                                        "[uv-host] input {} -> {cmd:?}",
                                        String::from_utf8_lossy(&body[..body.len().min(48)])
                                    );
                                }
                                if let Err(e) = inject::apply_input(inj, &cmd) {
                                    eprintln!("[uv-host] apply input failed: {e}");
                                }
                                continue;
                            }
                        }
                        eprintln!("[uv-host] recv unknown: {}", describe_msg(&msg));
                        dump_unknown(&msg);
                    }
                }
            }
            Ok(None) => {
                if n18 == 0 && !sent_first && started.elapsed() >= FIRST_FRAME_WAIT {
                    eprintln!("[uv-host] no 18 yet — sending first frame (not counted as baseline)");
                    sent_first = true;
                    send_frame(hs, cap, &mut prev, &mut scan, rows, &mut n, &mut stats)?;
                    on_frame(n);
                    // `need_full` stays set: the first `18` still gets a full frame.
                }
            }
            Err(e) => return Err(format!("session closed: {e}")),
        }
    }
}

/// Send one dirty frame: 32×32 tiles that changed since the previous frame,
/// plus tile rows in the current refresh pass (`scan`). Do not send one
/// full-screen rect: measured 05/11 19:24 the controller draws dirty frames
/// but does NOT draw one 1920×1080 rect plus a 2040-tile atlas (unchanged
/// areas stay black).
fn send_frame(
    hs: &mut HostSession,
    cap: &Capturer,
    prev: &mut Option<Vec<u8>>,
    scan: &mut Option<usize>,
    rows: usize,
    n: &mut i32,
    stats: &mut FrameStats,
) -> Result<(), String> {
    let t0 = Instant::now();
    let rgb = cap.grab_rgb()?;
    let t1 = Instant::now();
    let w = cap.width as usize;
    let h = cap.height as usize;
    let tiles = frame::tiles_due(prev.as_deref(), &rgb, w, h, scan, rows);
    let t2 = Instant::now();
    let rects = frame::merge_rows(&tiles);
    let rgb555 = frame::pack_regions_rgb555(&rgb, w, &rects);
    let (jpeg, aux) = frame::jpeg_tiles_for(&rgb, w, h, &tiles, 70).unwrap_or_default();
    let msg = frame::frame_message_ext(&rects, &rgb555, *n, &jpeg, &aux, frame::screen_dict())
        .ok_or("frame encode failed")?;
    let t3 = Instant::now();
    *prev = Some(rgb);
    hs.session.send(&msg).map_err(|e| format!("send frame failed: {e}"))?;
    let t4 = Instant::now();
    *n += 1;
    stats.add(t1 - t0, t2 - t1, t3 - t2, t4 - t3, tiles.len(), msg.len());
    Ok(())
}

/// Per-stage frame timing (printed every [`FrameStats::EVERY`] frames).
#[derive(Default)]
pub struct FrameStats {
    n: u32,
    grab: Duration,
    diff: Duration,
    enc: Duration,
    send: Duration,
    tile: usize,
    byte: usize,
    started: Option<Instant>,
}

impl FrameStats {
    const EVERY: u32 = 100;

    fn add(&mut self, grab: Duration, diff: Duration, enc: Duration, send: Duration, tile: usize, byte: usize) {
        let t0 = *self.started.get_or_insert_with(Instant::now);
        self.n += 1;
        self.grab += grab;
        self.diff += diff;
        self.enc += enc;
        self.send += send;
        self.tile += tile;
        self.byte += byte;
        if self.n == Self::EVERY {
            let n = self.n as f64;
            let ms = |d: Duration| d.as_secs_f64() * 1000.0 / n;
            let secs = t0.elapsed().as_secs_f64().max(1e-3);
            eprintln!(
                "[uv-host] stats {} frames: {:.1} fps | grab {:.1}ms diff {:.1}ms enc {:.1}ms send {:.1}ms | {:.0} tiles/frame {:.1} KB/frame {:.0} KB/s",
                self.n,
                n / secs,
                ms(self.grab),
                ms(self.diff),
                ms(self.enc),
                ms(self.send),
                self.tile as f64 / n,
                self.byte as f64 / n / 1024.0,
                self.byte as f64 / secs / 1024.0,
            );
            *self = FrameStats::default();
        }
    }
}

/// `XREMOTE_DUMP_DIR=<dir>`: write each unknown message to `<n>.bin` (for
/// protocol tracing, file transfer, ...). Unset means write nothing.
fn dump_unknown(msg: &[u8]) {
    use std::sync::atomic::{AtomicU32, Ordering};
    static N: AtomicU32 = AtomicU32::new(0);
    if let Some(d) = std::env::var_os("XREMOTE_DUMP_DIR") {
        let d = std::path::PathBuf::from(d);
        let _ = std::fs::create_dir_all(&d);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let _ = std::fs::write(d.join(format!("{n:05}.bin")), msg);
    }
}

/// Hex plus a UTF-16LE guess for an unknown message (chat format tracing).
fn describe_msg(msg: &[u8]) -> String {
    let n = msg.len().min(160);
    let hex: String = msg[..n].iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" ");
    let u16s: Vec<u16> = msg.get(1..).unwrap_or(&[]).chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
    let utf16 = String::from_utf16_lossy(&u16s);
    let utf8 = String::from_utf8_lossy(msg.get(1..).unwrap_or(&[]));
    format!("hex[{n}/{}]={hex} | utf8={utf8:?} | utf16={utf16:?}", msg.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_session_cmds() {
        assert_eq!(classify(b"18"), SessCmd::WantFrame);
        assert_eq!(classify(b"20"), SessCmd::Ping);
        assert_eq!(classify(b"17"), SessCmd::Other);
        assert_eq!(classify(b"4|||0"), SessCmd::Other);
        assert_eq!(classify(b"8|||512|||0|||10|||20"), SessCmd::Other);
        assert_eq!(classify(b"180"), SessCmd::Other);
        assert_eq!(classify(b"19"), SessCmd::Stop);
        // Exact bytes of the real UltraViewer close message (captured 08:44).
        assert_eq!(crate::client::BREAK_ERR, b"\x0019");
        assert_eq!(classify(&crate::client::BREAK_ERR[1..]), SessCmd::Stop);
    }
}
