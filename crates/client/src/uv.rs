//! Real UltraViewer transport for the egui UI.
//!
//! Crate `uvsession` owns the protocol. This file ties it to the UI lifetime:
//! a background thread holds the session, pushes frames decoded to RGB into the
//! UI over `Inbound`, and sends mouse/key commands from the UI back to the remote.

use std::io::Write;
use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};

use eframe::egui;
use uvproto::{Ctl, Event};

use crate::net::Inbound;

pub struct UvLink {
    tx: Sender<Ctl>,
    rx: Receiver<Inbound>,
    /// Session thread has sent the disconnect word `19` (see `Drop`).
    stopped: Receiver<()>,
}

/// Leave the session (Disconnect, or the window/app closing): send disconnect word
/// `19` like a real UltraViewer controller, then close. Without it the remote Windows
/// host treats the drop as a dead network ("Force disconnect because no response"),
/// waits, then fires `tmDisconnectCallBack_error` (seen 2026-11-05 08:24; a real
/// capture at 08:44 shows UltraViewer sends `\x0019` before closing TCP).
/// Wait briefly so `19` can leave when the process exits. Do not wait long:
/// the session thread may be sending a frame (a slow Mac capture fills the
/// TCP window). Waiting on the UI thread freezes the window, and the id
/// field stops accepting keys. Disconnect moves this `Drop` onto its own thread.
impl Drop for UvLink {
    fn drop(&mut self) {
        let _ = self.tx.send(Ctl { t: "stop".into(), ..Default::default() });
        let _ = self.stopped.recv_timeout(Duration::from_millis(80));
    }
}

impl UvLink {
    pub fn connect(
        host_id: &str,
        host_pw: &str,
        own_id: &str,
        own_pw: &str,
        ctx: &egui::Context,
    ) -> UvLink {
        let (tx, rx_cmd) = channel::<Ctl>();
        let (tx_in, rx_in) = channel::<Inbound>();
        let (tx_stop, rx_stop) = channel::<()>();
        let (id, pw) = (host_id.to_string(), host_pw.to_string());
        let (oid, opw) = (own_id.to_string(), own_pw.to_string());
        let ctx = ctx.clone();
        thread::spawn(move || run(id, pw, oid, opw, rx_cmd, tx_in, ctx, tx_stop));
        UvLink { tx, rx: rx_in, stopped: rx_stop }
    }

    pub fn send(&mut self, ctl: &Ctl) {
        let _ = self.tx.send(ctl.clone());
    }

    /// Take everything the background thread produced since the last call.
    pub fn poll(&self) -> (Vec<Inbound>, Option<String>) {
        let mut items = Vec::new();
        let mut closed = None;
        loop {
            match self.rx.try_recv() {
                Ok(Inbound::Closed(why)) => {
                    closed = Some(why);
                    break;
                }
                Ok(item) => items.push(item),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    closed = Some("receive channel closed".to_string());
                    break;
                }
            }
        }
        (items, closed)
    }
}

fn run(
    host_id: String,
    host_pw: String,
    own_id: String,
    own_pw: String,
    cmds: Receiver<Ctl>,
    out: Sender<Inbound>,
    ctx: egui::Context,
    stopped: Sender<()>,
) {
    let bao = |why: String| {
        if std::env::var("UV_DEBUG_INPUT").is_ok() {
            eprintln!("[uv] closed: {why}");
        }
        let _ = out.send(Inbound::Closed(why));
        ctx.request_repaint();
    };

    let mut c = match uvsession::client::connect(
        &host_id,
        &host_pw,
        &own_id,
        &own_pw,
        Duration::from_secs(25),
    ) {
        Ok(c) => c,
        Err(e) => return bao(e),
    };
    // 50 ms: `18` about 20 times a second — smooth, without asking the remote to capture too often.
    let _ = c.session.set_timeout(Duration::from_millis(50));
    // A slow Mac capture can miss the read. With no write timeout, `send`
    // stalls for tens of seconds and the disconnect word `19` never leaves —
    // the UI is already back on the id screen and still frozen.
    let _ = c.session.set_write_timeout(Duration::from_secs(2));
    if std::env::var("UV_DEBUG_INPUT").is_ok() {
        eprintln!("[uv] connected, peer={:?}", c.peer);
    }

    let _ = out.send(Inbound::Event(Event {
        t: "hello".into(),
        title: format!("{host_id} via UltraViewer"),
        ..Default::default()
    }));
    ctx.request_repaint();

    // First frame has no size yet: guess a common screen, then fix it when the
    // remote sends the init message.
    let mut w = 1920usize;
    let mut h = 1080usize;
    // Buffer shared with the UI; copy only while the UI still holds the old one.
    let mut canvas: std::sync::Arc<Vec<u8>> = std::sync::Arc::new(vec![0u8; w * h * 3]);
    let mut rstate = RState::new();
    let mut nbytes = 0usize;
    let mut sent_at: Option<Instant> = None;
    let debug = std::env::var("UV_DEBUG_INPUT").is_ok();
    let mut vong: u64 = 0;
    let mut da_co_bo_cuc = false;
    // Clipboard owner (opened the first time the remote sends a clipboard).
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    let mut clip: Option<uvsession::clipboard::ClipboardOwner> = None;

    loop {
        vong += 1;
        if debug && vong % 20 == 1 {
            eprintln!("[uv] loop {vong}");
        }
        // 1. Commands from the UI.
        loop {
            match cmds.try_recv() {
                Ok(ctl) if ctl.t == "stop" => {
                    // Active disconnect from the UltraViewer controller (see `Drop for UvLink`).
                    let _ = c.session.send(uvsession::client::BREAK_ERR);
                    let _ = stopped.send(());
                    return;
                }
                Ok(ctl) => {
                    if let Err(e) = send_cmd(&mut c.session, &ctl, w, h) {
                        return bao(format!("send command failed: {e}"));
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    let _ = c.session.send(uvsession::client::BREAK_ERR);
                    return;
                }
            }
        }

        // 2. Beat `18`: the remote captures and pushes a new frame only after each one.
        let _ = c.session.send(b"\x0018");

        // 3. Frames from the remote.
        if debug && vong % 20 == 1 {
            eprintln!("[uv] loop {vong}: waiting for frame");
        }
        let batch = match c.session.recv_batch() {
            Ok(b) => b,
            Err(e) => return bao(format!("connection lost: {e}")),
        };
        if debug && vong % 20 == 1 {
            eprintln!("[uv] loop {vong}: got {} messages", batch.len());
        }

        let mut have_pic = false;
        for msg in batch {
            if msg.is_empty() {
                continue;
            }
            // Monitor layout rides in a small text message; only the first one matters.
            if msg.len() < uvsession::layout::MAX_LAYOUT_MSG {
                let mons = uvsession::parse_monitors(&msg);
                if !mons.is_empty() {
                    da_co_bo_cuc = true;
                    if debug {
                        eprintln!("[uv] monitor layout: {mons:?}");
                    }
                    let _ = out.send(Inbound::Monitors(mons));
                    ctx.request_repaint();
                }
            }
            let body = match msg[0] {
                0 | 1 => &msg[1..],
                _ => &msg[..],
            };
            let Some(parts) = uvsession::decode_multipart(body) else { continue };
            match parts.first().and_then(|p| p.first().copied()) {
                Some(0) => {
                    // Init: two u32 fields are the remote screen size.
                    let doc = |i: usize| -> Option<usize> {
                        let p = parts.get(i)?;
                        (p.len() >= 4).then(|| {
                            u32::from_le_bytes([p[0], p[1], p[2], p[3]]) as usize
                        })
                    };
                    // Part 16 = OS tag of an X-Remote host; missing → UltraViewer (Windows).
                    let _ = out.send(Inbound::Os(uvsession::frame::os_from_init(&parts)));
                    // Part 15 = remote machine name (UTF-16LE) — chat sender name.
                    if let Some(p) = parts.get(15) {
                        let u: Vec<u16> = p.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
                        let ten = String::from_utf16_lossy(&u).trim_matches('\0').trim().to_string();
                        if !ten.is_empty() {
                            let _ = out.send(Inbound::HostName(ten));
                        }
                    }
                    if let (Some(nw), Some(nh)) = (doc(1), doc(2)) {
                        if (nw, nh) != (w, h) && nw > 0 && nh > 0 && nw <= 8192 && nh <= 8192 {
                            w = nw;
                            h = nh;
                            canvas = std::sync::Arc::new(vec![0u8; w * h * 3]);
                            have_pic = true;
                        }
                    }
                }
                Some(12) | Some(29) | Some(30) | Some(5) => {
                    if let Some(sk) = uvsession::chat::parse_from_host(&parts) {
                        // Remote clipboard → this machine (copy there, paste here).
                        #[cfg(any(target_os = "linux", target_os = "macos"))]
                        if let uvsession::chat::HostEvent::Clipboard(chu) = &sk {
                            if clip.is_none() {
                                clip = uvsession::clipboard::ClipboardOwner::new().ok();
                            }
                            if let Some(c) = clip.as_ref() {
                                c.set(chu);
                            }
                        }
                        let _ = out.send(Inbound::Chat(sk));
                        ctx.request_repaint();
                    }
                }
                Some(49) => {
                    // 49|||0 = control allowed, 49|||1 = view only (remote ignores mouse/keys).
                    let duoc = parts.get(1).and_then(|p| p.first().copied()) == Some(0);
                    let _ = out.send(Inbound::Control(duoc));
                    ctx.request_repaint();
                }
                Some(1) => {
                    if let Some(f) = uvsession::frame::parse(&parts, uvsession::frame::screen_dict()) {
                        nbytes = f.pixels.len() + f.jpeg.len();
                        // Several monitors: a region may sit outside the first guess.
                        // Grow the canvas to cover every region, like Python `canvas_size`.
                        let (nw, nh) = uvsession::layout::canvas_bounds(
                            f.rects.iter().chain(f.aux.iter()).map(|r| (r.x, r.y, r.w, r.h)),
                            w,
                            h,
                        );
                        if (nw, nh) != (w, h) && nw <= 8192 && nh <= 8192 {
                            canvas = std::sync::Arc::new(grow(&canvas, w, h, nw, nh));
                            w = nw;
                            h = nh;
                        }
                        let dst = std::sync::Arc::make_mut(&mut canvas);
                        render_frame(dst.as_mut_slice(), w, h, &mut rstate, &f);
                        have_pic = true;
                    }
                }
                _ => {}
            }
        }

        // 4. Send the picture to the UI, but not faster than ~20 fps.
        if have_pic {
            let can_send = sent_at.map(|t| t.elapsed() >= Duration::from_millis(50)).unwrap_or(true);
            if can_send {
                if std::env::var("UV_DEBUG_INPUT").is_ok() {
                    eprintln!("[uv] send frame {}x{} ({} bytes)", w, h, canvas.len());
                }
                sent_at = Some(Instant::now());
                // `Arc` shares the buffer with the UI instead of copying 2-6 MB per frame.
                let pic = Inbound::Frame(crate::net::Pic {
                    w: w as u16,
                    h: h as u16,
                    rgb: canvas.clone(),
                    byte: nbytes,
                });
                if out.send(pic).is_err() {
                    return;
                }
                ctx.request_repaint();
            }
        }
    }
}

/// New RGB canvas of size `nw×nh`, old pixels kept in the top-left corner.
fn grow(prev: &[u8], w: usize, h: usize, nw: usize, nh: usize) -> Vec<u8> {
    let mut new = vec![0u8; nw * nh * 3];
    let span = w.min(nw) * 3;
    for y in 0..h.min(nh) {
        let a = y * w * 3;
        let b = y * nw * 3;
        if a + span <= prev.len() {
            new[b..b + span].copy_from_slice(&prev[a..a + span]);
        }
    }
    new
}

/// Paint RGB555 rects onto the UI's RGB image (leave the rest untouched).
#[cfg(test)]
fn paint_frame(dst: &mut [u8], w: usize, h: usize, rects: &[uvsession::Region], pixels: &[u8]) {
    let mut off = 0usize;
    for r in rects {
        if r.w <= 0 || r.h <= 0 {
            continue;
        }
        let can = r.w as usize * r.h as usize * 2;
        if off + can > pixels.len() {
            break;
        }
        for row in 0..r.h as usize {
            let y = r.y + row as i32;
            if y < 0 || y >= h as i32 {
                continue;
            }
            let mut i = off + row * r.w as usize * 2;
            for col in 0..r.w as usize {
                let x = r.x + col as i32;
                if x >= 0 && x < w as i32 {
                    let v = u16::from_le_bytes([pixels[i], pixels[i + 1]]);
                    let p = uvsession::frame::rgb555_pixel(v);
                    let d = (y as usize * w + x as usize) * 3;
                    dst[d] = p[0];
                    dst[d + 1] = p[1];
                    dst[d + 2] = p[2];
                }
                i += 2;
            }
        }
        off += can;
    }
    let _ = std::io::sink().flush();
}

/// Turn one UI command into a message for the remote.
fn send_cmd(
    session: &mut uvsession::Session,
    ctl: &Ctl,
    w: usize,
    h: usize,
) -> std::io::Result<()> {
    use uvsession::input as inp;
    if std::env::var("UV_DEBUG_INPUT").is_ok() {
        eprintln!(
            "[gui] t={} k={} x={:.3} y={:.3} d={:.0} v={} c={:?} down={}",
            ctl.t, ctl.k, ctl.x, ctl.y, ctl.d, ctl.v, ctl.c, ctl.down
        );
    }
    match ctl.t.as_str() {
        "mouse" => {
            let x = (ctl.x.clamp(0.0, 1.0) * w as f32).round() as i32;
            let y = (ctl.y.clamp(0.0, 1.0) * h as f32).round() as i32;
            let frame = match ctl.k.as_str() {
                "move" => inp::mouse_move(x, y),
                "down" => inp::mouse_button(
                    match ctl.b {
                        1 => inp::WM_MBUTTONDOWN,
                        2 => inp::WM_RBUTTONDOWN,
                        _ => inp::WM_LBUTTONDOWN,
                    },
                    x,
                    y,
                ),
                "up" => inp::mouse_button(
                    match ctl.b {
                        1 => inp::WM_MBUTTONUP,
                        2 => inp::WM_RBUTTONUP,
                        _ => inp::WM_LBUTTONUP,
                    },
                    x,
                    y,
                ),
                "wheel" => inp::mouse_wheel(ctl.d.round() as i32, x, y),
                _ => return Ok(()),
            };
            session.send(&frame)
        }
        "key" => match ctl.k.as_str() {
            "char" => {
                for ch in ctl.c.chars() {
                    if let Some(evs) = inp::char_events(ch) {
                        for e in evs {
                            session.send(&inp::key_frame(&e))?;
                        }
                    }
                }
                Ok(())
            }
            "vk" => session.send(&inp::key_frame(&inp::vk_events(ctl.v as u8, ctl.down))),
            _ => Ok(()),
        },
        "quality" => session.send(&inp::quality_frame(ctl.v as i32)),
        // Controller → host chat: `01` + UTF-16 `12|||<text>`; typing `29` / stop typing `30`.
        "chat" => {
            session.send(&uvsession::chat::controller_msg(&ctl.c))?;
            session.send(&uvsession::chat::controller_typing(false))
        }
        "chat_go" => session.send(&uvsession::chat::controller_typing(ctl.down)),
        // v = 1 show wallpaper, 0 hide.
        "wallpaper" => session.send(&inp::wallpaper_frame(ctl.v != 0)),
        // No equivalent in the UltraViewer protocol yet.
        _ => Ok(()),
    }
}

/// Blit a 32×N JPEG atlas (part 7) onto the aux regions (part 8).
///
/// The atlas is a vertical strip of 32×32 tiles. A region wider or taller than
/// one tile takes the next tiles in row-major order, the same order the remote
/// packed them. Returns how many tiles were blitted, for tests.
fn blit_jpeg_atlas(
    dst: &mut [u8],
    w: usize,
    h: usize,
    jpeg: &[u8],
    aux: &[uvsession::Region],
) -> usize {
    const T: usize = 32;
    if jpeg.is_empty() || aux.is_empty() || dst.len() < w * h * 3 {
        return 0;
    }
    let mut dec = jpeg_decoder::Decoder::new(std::io::Cursor::new(jpeg));
    let anh = match dec.decode() {
        Ok(a) => a,
        Err(_) => return 0,
    };
    let info = match dec.info() {
        Some(i) => i,
        None => return 0,
    };
    let (aw, ah) = (info.width as usize, info.height as usize);
    if aw == 0 || ah < T {
        return 0;
    }
    // Column-major tile walk + bottom-up rows, matching the verified MacViewer
    // reference. The atlas is a strip of 32x32 tiles, wrapped into the next column
    // when one column fills; its rows are addressed bottom-up.
    let mut column = 0usize;
    let mut sy = 0usize;
    let mut da_dan = 0usize;
    for r in aux {
        if r.w <= 0 || r.h <= 0 {
            continue;
        }
        let (rw, rh) = (r.w as usize, r.h as usize);
        for dx in (0..rw).step_by(T) {
            for dy in (0..rh).step_by(T) {
                let (tw, th) = (T.min(rw - dx), T.min(rh - dy));
                if sy + th > ah {
                    sy = 0;
                    column += 1;
                }
                let sx = column * T;
                if sx + tw > aw || sy + th > ah {
                    return da_dan;
                }
                let gx = r.x + dx as i32;
                let gy = r.y + dy as i32;
                for row in 0..th {
                    let yy = gy + row as i32;
                    if yy < 0 || yy >= h as i32 {
                        continue;
                    }
                    let src_row = ah - 1 - (sy + row);
                    let so = (src_row * aw + sx) * 3;
                    for col in 0..tw {
                        let xx = gx + col as i32;
                        if xx < 0 || xx >= w as i32 {
                            continue;
                        }
                        let di = (yy as usize * w + xx as usize) * 3;
                        let si = so + col * 3;
                        dst[di] = anh[si];
                        dst[di + 1] = anh[si + 1];
                        dst[di + 2] = anh[si + 2];
                    }
                }
                sy += th;
                da_dan += 1;
            }
        }
    }
    da_dan
}

// ---------------------------------------------------------------------------
// Delta-frame renderer: cache + scroll + copy. Ported from the verified
// MacViewer reference (DeanOpen/MacViewer, screen/renderer.rs), adapted to a
// top-down RGB (3-byte) canvas instead of a bottom-up RGB555 DIB.
// ---------------------------------------------------------------------------

struct CachedTile {
    w: usize,
    h: usize,
    px: Vec<u8>, // tightly packed top-down RGB
}

struct RState {
    w: usize,
    h: usize,
    temp: Vec<u8>, // top-down RGB w*h*3
    cache: std::collections::HashMap<i32, CachedTile>,
    cache_order: Vec<i32>, // oldest first
}

impl RState {
    fn new() -> Self {
        RState {
            w: 0,
            h: 0,
            temp: Vec::new(),
            cache: std::collections::HashMap::new(),
            cache_order: Vec::new(),
        }
    }

    fn set_size(&mut self, w: usize, h: usize) {
        if self.w != w || self.h != h {
            self.w = w;
            self.h = h;
            self.temp = vec![0u8; w * h * 3];
            self.cache.clear();
            self.cache_order.clear();
        }
    }
}

fn blit_rect(
    dst: &mut [u8],
    src: &[u8],
    w: usize,
    h: usize,
    sx: i32,
    sy: i32,
    dx: i32,
    dy: i32,
    cw: i32,
    ch: i32,
) {
    let (wi, hi) = (w as i64, h as i64);
    let left = 0i64.max(-(sx as i64)).max(-(dx as i64));
    let top = 0i64.max(-(sy as i64)).max(-(dy as i64));
    let right = (cw as i64).min(wi - sx as i64).min(wi - dx as i64);
    let bottom = (ch as i64).min(hi - sy as i64).min(hi - dy as i64);
    if left >= right || top >= bottom {
        return;
    }
    let n = (right - left) as usize * 3;
    for row in 0..(bottom - top) as usize {
        let so = (((sy as i64 + top + row as i64) as usize) * w + (sx as i64 + left) as usize) * 3;
        let d = (((dy as i64 + top + row as i64) as usize) * w + (dx as i64 + left) as usize) * 3;
        dst[d..d + n].copy_from_slice(&src[so..so + n]);
    }
}

fn blit_tile(dst: &mut [u8], w: usize, h: usize, px: &[u8], tw: usize, th: usize, dx: i32, dy: i32) {
    let sx0 = if dx < 0 { (-dx) as usize } else { 0 };
    let sy0 = if dy < 0 { (-dy) as usize } else { 0 };
    let dx0 = dx.max(0) as usize;
    let dy0 = dy.max(0) as usize;
    let cw = tw.saturating_sub(sx0).min(w.saturating_sub(dx0));
    let ch = th.saturating_sub(sy0).min(h.saturating_sub(dy0));
    for row in 0..ch {
        let so = ((sy0 + row) * tw + sx0) * 3;
        let d = ((dy0 + row) * w + dx0) * 3;
        dst[d..d + cw * 3].copy_from_slice(&px[so..so + cw * 3]);
    }
}

fn blit_rgb555(dst: &mut [u8], w: usize, h: usize, r: &uvsession::Region, pixels: &[u8], off: &mut usize) {
    if r.w <= 0 || r.h <= 0 {
        return;
    }
    let need = r.w as usize * r.h as usize * 2;
    if *off + need > pixels.len() {
        return;
    }
    for row in 0..r.h as usize {
        let y = r.y + row as i32;
        if y < 0 || y >= h as i32 {
            continue;
        }
        let mut i = *off + row * r.w as usize * 2;
        for col in 0..r.w as usize {
            let x = r.x + col as i32;
            if x >= 0 && x < w as i32 {
                let v = u16::from_le_bytes([pixels[i], pixels[i + 1]]);
                let p = uvsession::frame::rgb555_pixel(v);
                let d = (y as usize * w + x as usize) * 3;
                dst[d] = p[0];
                dst[d + 1] = p[1];
                dst[d + 2] = p[2];
            }
            i += 2;
        }
    }
    *off += need;
}

fn extract_rgb(src: &[u8], w: usize, h: usize, r: &uvsession::Region) -> Vec<u8> {
    let rw = r.w.max(0) as usize;
    let rh = r.h.max(0) as usize;
    let mut out = vec![0u8; rw * rh * 3];
    for row in 0..rh {
        let y = r.y + row as i32;
        if y < 0 || y >= h as i32 {
            continue;
        }
        for col in 0..rw {
            let x = r.x + col as i32;
            if x < 0 || x >= w as i32 {
                continue;
            }
            let so = (y as usize * w + x as usize) * 3;
            let d = (row * rw + col) * 3;
            out[d..d + 3].copy_from_slice(&src[so..so + 3]);
        }
    }
    out
}

fn render_frame(main: &mut [u8], w: usize, h: usize, st: &mut RState, f: &uvsession::frame::Frame) {
    st.set_size(w, h);

    if let Some(sc) = &f.scroll {
        blit_rect(
            &mut st.temp,
            main,
            w,
            h,
            sc.capture.x,
            sc.capture.y,
            sc.capture.x,
            sc.capture.y - sc.delta,
            sc.capture.w,
            sc.capture.h,
        );
        for r in &sc.copy_back {
            blit_rect(main, &st.temp, w, h, r.x, r.y, r.x, r.y, r.w, r.h);
        }
    }

    for r in &f.cache_refs {
        if let Some(t) = st.cache.get(&r.hash) {
            blit_tile(&mut st.temp, w, h, &t.px, t.w, t.h, r.x, r.y);
        }
    }

    for r in &f.copy {
        blit_rect(main, &st.temp, w, h, r.x, r.y, r.x, r.y, r.w, r.h);
    }

    blit_jpeg_atlas(main, w, h, &f.jpeg, &f.aux);

    let mut off = 0usize;
    for r in &f.rects {
        blit_rgb555(main, w, h, r, &f.pixels, &mut off);
    }

    for a in &f.cache_adds {
        blit_rect(&mut st.temp, main, w, h, a.r.x, a.r.y, a.r.x, a.r.y, a.r.w, a.r.h);
        let px = extract_rgb(&st.temp, w, h, &a.r);
        st.cache.insert(a.hash, CachedTile { w: a.r.w as usize, h: a.r.h as usize, px });
        st.cache_order.retain(|&x| x != a.hash);
        st.cache_order.push(a.hash);
    }
    while st.cache_order.len() > 1000 {
        if let Some(h0) = st.cache_order.first().copied() {
            st.cache.remove(&h0);
        }
        st.cache_order.remove(0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uvsession::Region;

    #[test]
    fn paint_color_and_pos() {
        let (w, h) = (4usize, 3usize);
        let mut dst = vec![0u8; w * h * 3];
        // 2x1 rect at (1,1): white pixel (0x7FFF) and red (0x7C00)
        let px = [0xFFu8, 0x7F, 0x00, 0x7C];
        paint_frame(&mut dst, w, h, &[Region { x: 1, y: 1, w: 2, h: 1 }], &px);
        let i = (1 * w + 1) * 3;
        assert_eq!(&dst[i..i + 3], &[255, 255, 255]);
        let j = (1 * w + 2) * 3;
        assert_eq!(&dst[j..j + 3], &[255, 0, 0]);
        // outside the rect stays black
        assert_eq!(&dst[0..3], &[0, 0, 0]);
    }

    #[test]
    fn grow_keeps_old_pixels() {
        // 2×1: red, green → grow to 3×2.
        let prev = [255u8, 0, 0, 0, 255, 0];
        let new = grow(&prev, 2, 1, 3, 2);
        assert_eq!(new.len(), 3 * 2 * 3);
        assert_eq!(&new[0..6], &prev);
        assert!(new[6..].iter().all(|&b| b == 0));
    }

    #[test]
    fn paint_clips_at_edge() {
        let (w, h) = (2usize, 2usize);
        let mut dst = vec![0u8; w * h * 3];
        let px = [0xFFu8; 8];
        // 2x2 rect starting at (-1,-1): only one pixel lands in the image
        paint_frame(&mut dst, w, h, &[Region { x: -1, y: -1, w: 2, h: 2 }], &px);
        assert_eq!(&dst[0..3], &[255, 255, 255]);
        assert_eq!(&dst[3..6], &[0, 0, 0]);
    }

    /// 32×64 bottom-up atlas of two tiles: tile 0 red (rows 32..64), tile 1 blue (rows 0..32).
    fn two_tile_atlas() -> Vec<u8> {
        let (aw, ah) = (32usize, 64usize);
        let mut rgb = vec![0u8; aw * ah * 3];
        for y in 0..ah {
            for x in 0..aw {
                let i = (y * aw + x) * 3;
                if y >= 32 {
                    rgb[i] = 255;
                } else {
                    rgb[i + 2] = 255;
                }
            }
        }
        let mut out = Vec::new();
        jpeg_encoder::Encoder::new(&mut out, 95)
            .encode(&rgb, aw as u16, ah as u16, jpeg_encoder::ColorType::Rgb)
            .expect("encoded atlas");
        out
    }

    #[test]
    fn blit_atlas_place_and_order() {
        let (w, h) = (80usize, 60usize);
        let mut dst = vec![0u8; w * h * 3];
        let jpeg = two_tile_atlas();
        let aux = [
            Region { x: 8, y: 4, w: 32, h: 32 },
            Region { x: 48, y: 8, w: 32, h: 32 },
        ];
        let n = blit_jpeg_atlas(&mut dst, w, h, &jpeg, &aux);
        assert_eq!(n, 2, "must blit exactly two tiles");
        // tile 0 (red) sits in the first region
        let i = (4 * w + 8) * 3;
        assert!(dst[i] > 200 && dst[i + 2] < 60, "tile 0 must be red: {:?}", &dst[i..i + 3]);
        // tile 1 (blue) sits in the second region
        let j = (8 * w + 48) * 3;
        assert!(dst[j + 2] > 200 && dst[j] < 60, "tile 1 must be blue: {:?}", &dst[j..j + 3]);
        // outside both regions stays black
        let k = (40 * w + 70) * 3;
        assert_eq!(&dst[k..k + 3], &[0, 0, 0]);
    }

    #[test]
    fn missing_atlas_does_nothing() {
        let (w, h) = (8usize, 8usize);
        let mut dst = vec![0u8; w * h * 3];
        assert_eq!(blit_jpeg_atlas(&mut dst, w, h, &[], &[Region { x: 0, y: 0, w: 32, h: 32 }]), 0);
        assert_eq!(blit_jpeg_atlas(&mut dst, w, h, b"not-a-jpeg", &[Region { x: 0, y: 0, w: 32, h: 32 }]), 0);
        assert!(dst.iter().all(|&b| b == 0));
    }
}
