//! Decode an UltraViewer video frame.
//!
//! Command 1 has 11 parts:
//! 0 = command, 1 = main rects (4 × u16), 2 = zstd RGB555 (optional dictionary),
//! 4 = frame number (i32), 5 = copy, 6 = scroll, 7 = extra JPEG atlas,
//! 8 = atlas rects (4 × i16), 9/10 = cache.

/// One rectangle on the remote screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Region {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Region {
    pub fn area(&self) -> i64 {
        self.w.max(0) as i64 * self.h.max(0) as i64
    }
}

/// One decoded video frame.
#[derive(Debug, Default, Clone)]
pub struct Frame {
    pub rects: Vec<Region>,
    /// RGB555 little-endian pixels, in `rects` order.
    pub pixels: Vec<u8>,
    pub frame_no: i32,
    /// 32×N JPEG atlas of the extra regions.
    pub jpeg: Vec<u8>,
    pub aux: Vec<Region>,
}

/// Read 8-byte rects: `u16` for main regions, `i16` for extra regions.
pub fn regions(data: &[u8], signed: bool) -> Vec<Region> {
    let mut out = Vec::with_capacity(data.len() / 8);
    for chunk in data.chunks_exact(8) {
        let v: [i32; 4] = if signed {
            let g = |i: usize| i16::from_le_bytes([chunk[i], chunk[i + 1]]) as i32;
            [g(0), g(2), g(4), g(6)]
        } else {
            let g = |i: usize| u16::from_le_bytes([chunk[i], chunk[i + 1]]) as i32;
            [g(0), g(2), g(4), g(6)]
        };
        out.push(Region { x: v[0], y: v[1], w: v[2], h: v[3] });
    }
    out
}

/// Optional zstd dictionary for screen frames, loaded at runtime from the path in
/// `XREMOTE_SCREEN_DICT`. Not bundled (that dictionary is third-party). Without it,
/// frames use plain zstd (fine peer-to-peer; only needed to interop with other clients).
pub fn screen_dict() -> Option<&'static [u8]> {
    use std::sync::OnceLock;
    static D: OnceLock<Option<Vec<u8>>> = OnceLock::new();
    D.get_or_init(|| {
        std::env::var_os("XREMOTE_SCREEN_DICT")
            .and_then(|p| std::fs::read(p).ok())
            .filter(|v| !v.is_empty())
    })
    .as_deref()
}

/// zstd decompress, with dictionary when given; otherwise plain.
pub fn decompress(data: &[u8], dict: Option<&[u8]>) -> Option<Vec<u8>> {
    let mut dec = match dict {
        Some(d) if !d.is_empty() => zstd::bulk::Decompressor::with_dictionary(d).ok()?,
        _ => zstd::bulk::Decompressor::new().ok()?,
    };
    // A 1080p RGB555 frame is at most ~4 MB; allow extra so we do not truncate.
    dec.decompress(data, 64 << 20).ok()
}

/// Decode the parts of one command-1 frame.
pub fn parse(parts: &[Vec<u8>], dict: Option<&[u8]>) -> Option<Frame> {
    if parts.len() < 3 || parts[0].first().copied() != Some(1) {
        return None;
    }
    let rects = regions(&parts[1], false);
    if rects.is_empty() {
        return None;
    }
    let mut pixels = Vec::new();
    for body in [&parts[2][..], parts[2].get(4..).unwrap_or(&[])] {
        if body.is_empty() {
            continue;
        }
        if let Some(v) = decompress(body, dict) {
            pixels = v;
            break;
        }
    }
    if pixels.is_empty() {
        return None;
    }
    let frame_no = parts
        .get(4)
        .filter(|p| p.len() == 4)
        .map(|p| i32::from_le_bytes([p[0], p[1], p[2], p[3]]))
        .unwrap_or(-1);
    Some(Frame {
        rects,
        pixels,
        frame_no,
        jpeg: parts.get(7).cloned().unwrap_or_default(),
        aux: parts.get(8).map(|p| regions(p, true)).unwrap_or_default(),
    })
}

/// zstd compress with a dictionary (the inverse of [`decompress`]).
pub fn compress(data: &[u8], dict: Option<&[u8]>, level: i32) -> Option<Vec<u8>> {
    let mut enc = match dict {
        Some(d) if !d.is_empty() => zstd::bulk::Compressor::with_dictionary(level, d).ok()?,
        _ => zstd::bulk::Compressor::new(level).ok()?,
    };
    enc.compress(data).ok()
}

/// Pack one RGB pixel (8 bits per channel) into the remote's little-endian RGB555
/// (red in the high 5 bits, green in the middle, blue in the low 5). Inverse of
/// [`rgb555_pixel`].
#[inline]
pub fn rgb_to_rgb555_u16(r: u8, g: u8, b: u8) -> u16 {
    ((r as u16 >> 3) << 10) | ((g as u16 >> 3) << 5) | (b as u16 >> 3)
}

/// Convert packed RGB (3 bytes/pixel) to little-endian RGB555 (2 bytes/pixel).
pub fn rgb_to_rgb555(rgb: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(rgb.len() / 3 * 2);
    for px in rgb.chunks_exact(3) {
        out.extend_from_slice(&rgb_to_rgb555_u16(px[0], px[1], px[2]).to_le_bytes());
    }
    out
}

/// Encode rects as 8 bytes each (4 × u16, main regions).
pub fn encode_regions(rects: &[Region]) -> Vec<u8> {
    let mut out = Vec::with_capacity(rects.len() * 8);
    for r in rects {
        out.extend_from_slice(&(r.x as u16).to_le_bytes());
        out.extend_from_slice(&(r.y as u16).to_le_bytes());
        out.extend_from_slice(&(r.w as u16).to_le_bytes());
        out.extend_from_slice(&(r.h as u16).to_le_bytes());
    }
    out
}

/// Build a **command-1 frame** (host → controller):
/// part0 = `[1]`, part1 = rects, part2 = zstd RGB555, part4 = frame number.
///
/// `rgb555` is little-endian RGB555 in `rects` order. The result is the multipart
/// body only (no session flag byte) — see [`frame_message`].
pub fn encode_command1(rects: &[Region], rgb555: &[u8], frame_no: i32, dict: Option<&[u8]>) -> Option<Vec<u8>> {
    let rect_bytes = encode_regions(rects);
    let packed = compress(rgb555, dict, 3)?;
    let fno = frame_no.to_le_bytes();
    // The real client has all 11 parts. Missing part[7..10] makes the controller
    // report "Subscript out of range" while parsing the frame.
    // The real client uses a 2-byte command `01 00`; part[3] is a 1-byte flag;
    // part[4] is the frame number.
    let cmd = 1u16.to_le_bytes();
    let flag = [0u8];
    let parts: [&[u8]; 11] = [
        &cmd,
        &rect_bytes,
        &packed,
        &flag,
        &fno,
        &[],
        &[],
        &[],
        &[],
        &[],
        &[],
    ];
    Some(crate::multipart::encode(&parts))
}

/// **Init** message (command 0) the remote sends right after the session login.
///
/// The real controller builds its canvas from this message. Without it the
/// controller stays on "waiting for connection" even though the session is open.
pub fn init_message(w: u32, h: u32, host_name: &str) -> Vec<u8> {
    // Layout taken from a real UltraViewer Windows capture (16 parts):
    // 0=[0,0] 1=w 2=h 3=16012 4=16170 5=[0,0] 6=version 7=[1]
    // 8=screen layout  9=0 10=0 11=[0] 12=[0,0] 13=u16(1) 14=u16(32) 15=host name UTF-16LE
    let wb = w.to_le_bytes();
    let hb = h.to_le_bytes();
    let p3 = 16012u32.to_le_bytes();
    let p4 = 16170u32.to_le_bytes();
    let zero4 = 0u32.to_le_bytes();
    let zero2 = [0u8, 0u8];
    let one = [1u8];
    let one16 = 1u16.to_le_bytes();
    let tile16 = 32u16.to_le_bytes();
    let layout = format!("0:0:{w}:{h}:-1|");
    let name: Vec<u8> = host_name.encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
    // Part 17 (index 16) — OS tag of an X-Remote host: `XR-OS:linux|macos|windows`.
    // Real UltraViewer only reads indexes 0..15 (a short message is "Subscript out of
    // range"); extra parts are ignored. The X-Remote controller reads this to show
    // the OS in ID history. No tag means a stock UltraViewer host (Windows only).
    let os_tag = format!("{OS_TAG}{}", this_os());
    let parts: [&[u8]; 17] = [
        &[0u8, 0u8],
        &wb,
        &hb,
        &p3,
        &p4,
        &zero2,
        crate::control::VERSION.as_bytes(),
        &one,
        layout.as_bytes(),
        &zero4,
        &zero4,
        &[0u8],
        &zero2,
        &one16,
        &tile16,
        &name,
        os_tag.as_bytes(),
    ];
    let body = crate::multipart::encode(&parts);
    let mut msg = Vec::with_capacity(1 + body.len());
    msg.push(1u8);
    msg.extend_from_slice(&body);
    msg
}

/// OS-tag prefix in part 16 of the init message (see [`init_message`]).
pub const OS_TAG: &str = "XR-OS:";

/// This machine's OS name used in the tag (`linux` / `macos` / `windows`).
pub fn this_os() -> &'static str {
    if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else {
        "linux"
    }
}

/// Remote OS, read from an init message (command 0). An `XR-OS:` tag means an
/// X-Remote host and names the OS; no tag means stock UltraViewer (Windows only).
pub fn os_from_init(parts: &[Vec<u8>]) -> String {
    parts
        .get(16)
        .and_then(|p| std::str::from_utf8(p).ok())
        .and_then(|s| s.strip_prefix(OS_TAG))
        .filter(|s| matches!(*s, "linux" | "macos" | "windows"))
        .unwrap_or("windows")
        .to_string()
}

/// Full session message for one frame: `[binary flag 1] + command-1 multipart`.
/// Pass straight to `Session::send`.
pub fn frame_message(rects: &[Region], rgb555: &[u8], frame_no: i32, dict: Option<&[u8]>) -> Option<Vec<u8>> {
    let body = encode_command1(rects, rgb555, frame_no, dict)?;
    let mut msg = Vec::with_capacity(1 + body.len());
    msg.push(0u8); // video frames use flag 0 (real client)
    msg.extend_from_slice(&body);
    Some(msg)
}

/// 15-byte message `[u16 3][u32 n]`.
///
/// A Windows remote sends this when the controller sends `18` before `4|||<id>`.
/// In a stable real UltraViewer session the host does NOT send it — the
/// "message timeout without received ack" string belongs to `VUDPClient` (UDP),
/// not the ~16 s TCP reset. See [`pong_message`].
pub fn ack_message(n: u32) -> Vec<u8> {
    let cmd = 3u16.to_le_bytes();
    let seq = n.to_le_bytes();
    let parts: [&[u8]; 2] = [&cmd, &seq];
    let body = crate::multipart::encode(&parts);
    let mut msg = Vec::with_capacity(1 + body.len());
    msg.push(0u8);
    msg.extend_from_slice(&body);
    msg
}

/// Reply to the controller ping `20`: `00` + multipart `[u16 20]` (7 bytes).
///
/// From a real UltraViewer pcap (Wine host, Windows controller, 20 minutes):
/// 125 `20` commands, 124 of these replies. If the host stays silent the
/// controller reports "connection lost, waiting for signal" and resets the
/// channel after ~16 seconds.
pub fn pong_message() -> Vec<u8> {
    let cmd = 20u16.to_le_bytes();
    let body = crate::multipart::encode(&[&cmd]);
    let mut msg = Vec::with_capacity(1 + body.len());
    msg.push(0u8);
    msg.extend_from_slice(&body);
    msg
}

/// Merge adjacent tiles on the same row (same `y` and `h`) into one rect,
/// so a dirty frame does not carry thousands of 32×32 rects.
pub fn merge_rows(tiles: &[Region]) -> Vec<Region> {
    let mut out: Vec<Region> = Vec::with_capacity(tiles.len());
    for t in tiles {
        if let Some(last) = out.last_mut() {
            if last.y == t.y && last.h == t.h && last.x + last.w == t.x {
                last.w += t.w;
                continue;
            }
        }
        out.push(*t);
    }
    out
}

/// Pick 32×32 tiles for one dirty frame: tiles that differ from `prev` (none
/// if there is no `prev`) plus tile rows `scan .. scan + rows` of the refresh
/// pass. `scan` advances, and becomes `None` at the bottom of the screen.
/// Result is sorted by (y, x), unique, and never empty (a real host still
/// returns one small tile for each `18` when the screen is idle).
pub fn tiles_due(
    prev: Option<&[u8]>,
    cur: &[u8],
    w: usize,
    h: usize,
    scan: &mut Option<usize>,
    rows: usize,
) -> Vec<Region> {
    let mut tiles = match prev {
        Some(old) => dirty_tiles(old, cur, w, h),
        None => Vec::new(),
    };
    if let Some(r0) = *scan {
        let row_count = h.div_ceil(32);
        let r1 = (r0 + rows.max(1)).min(row_count);
        let (y0, y1) = ((r0 * 32) as i32, (r1 * 32) as i32);
        tiles.extend(all_tiles(w, h).into_iter().filter(|t| t.y >= y0 && t.y < y1));
        *scan = (r1 < row_count).then_some(r1);
    }
    tiles.sort_by_key(|t| (t.y, t.x));
    tiles.dedup();
    if tiles.is_empty() && w > 0 && h > 0 {
        tiles.push(Region { x: 0, y: 0, w: 32.min(w) as i32, h: 32.min(h) as i32 });
    }
    tiles
}

/// Expand one RGB555 pixel (bits 14-10 R, 9-5 G, 4-0 B) to 8-bit RGBA.
#[inline]
pub fn rgb555_pixel(v: u16) -> [u8; 4] {
    // The remote stores red in the high 5 bits and blue in the low 5. Swapping
    // the ends makes the picture orange — checked against a real screenshot.
    let r = ((v >> 10) & 0x1f) as u32;
    let g = ((v >> 5) & 0x1f) as u32;
    let b = (v & 0x1f) as u32;
    // Stretch 5 bits to 8: multiply by 255/31 so white stays white.
    let c = |x: u32| ((x * 255 + 15) / 31) as u8;
    [c(r), c(g), c(b), 255]
}

/// Blit RGB555 rects onto an RGBA image `dst` of size `dst_w × dst_h`.
///
/// Rects outside the image are clipped. If pixel data runs out, stop at the
/// last complete rect.
pub fn blit_rgba(dst: &mut [u8], dst_w: usize, dst_h: usize, pixels: &[u8], rects: &[Region]) {
    let mut off = 0usize;
    for r in rects {
        if r.w <= 0 || r.h <= 0 {
            continue;
        }
        let need = r.w as usize * r.h as usize * 2;
        if off + need > pixels.len() {
            break;
        }
        for row in 0..r.h as usize {
            let y = r.y + row as i32;
            if y < 0 || y >= dst_h as i32 {
                continue;
            }
            for col in 0..r.w as usize {
                let x = r.x + col as i32;
                if x < 0 || x >= dst_w as i32 {
                    continue;
                }
                let i = off + (row * r.w as usize + col) * 2;
                let v = u16::from_le_bytes([pixels[i], pixels[i + 1]]);
                let d = (y as usize * dst_w + x as usize) * 4;
                dst[d..d + 4].copy_from_slice(&rgb555_pixel(v));
            }
        }
        off += need;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regions_signed_and_unsigned() {
        let mut d = Vec::new();
        d.extend_from_slice(&1920u16.to_le_bytes());
        d.extend_from_slice(&1080u16.to_le_bytes());
        d.extend_from_slice(&32u16.to_le_bytes());
        d.extend_from_slice(&16u16.to_le_bytes());
        assert_eq!(regions(&d, false)[0], Region { x: 1920, y: 1080, w: 32, h: 16 });

        let mut s = Vec::new();
        for v in [-8i16, 4, 32, 32] {
            s.extend_from_slice(&v.to_le_bytes());
        }
        assert_eq!(regions(&s, true)[0], Region { x: -8, y: 4, w: 32, h: 32 });
    }

    #[test]
    fn rgb555_channel_order() {
        // 0x7C00 = R=31 in the high 5 bits (checked against a real capture).
        assert_eq!(rgb555_pixel(0x7C00), [255, 0, 0, 255]);
        assert_eq!(rgb555_pixel(0x03E0), [0, 255, 0, 255]);
        assert_eq!(rgb555_pixel(0x001F), [0, 0, 255, 255]);
        assert_eq!(rgb555_pixel(0x0000), [0, 0, 0, 255]);
    }

    #[test]
    fn blit_clips_edges() {
        let mut dst = vec![0u8; 4 * 4 * 4];
        let rects = [Region { x: -1, y: -1, w: 2, h: 2 }];
        let px = [0xFFu8; 8]; // 4 white pixels
        blit_rgba(&mut dst, 4, 4, &px, &rects);
        // only pixel (0,0) is inside the image
        assert_eq!(&dst[0..4], &[255, 255, 255, 255]);
        assert_eq!(&dst[4..8], &[0, 0, 0, 0]);
    }

    #[test]
    fn encode_frame_round_trip() {
        // 4×2 image, one full-frame rect.
        let w = 4i32;
        let h = 2i32;
        let rgb: Vec<u8> = (0..(w * h)).flat_map(|i| [(i * 20) as u8, (i * 10) as u8, (i * 5) as u8]).collect();
        let rgb555 = rgb_to_rgb555(&rgb);
        let rects = [Region { x: 0, y: 0, w, h }];
        let body = encode_command1(&rects, &rgb555, 42, None).expect("encode");
        let parts = crate::multipart::decode(&body).expect("multipart");
        let f = parse(&parts, None).expect("parse frame");
        assert_eq!(f.rects, rects);
        assert_eq!(f.frame_no, 42);
        assert_eq!(f.pixels, rgb555, "RGB555 pixels must round-trip");
    }

    #[test]
    fn rgb_to_rgb555_round_trip() {
        // Pure red/green/blue must come back on the same channel.
        assert_eq!(rgb555_pixel(rgb_to_rgb555_u16(255, 0, 0)), [255, 0, 0, 255]);
        assert_eq!(rgb555_pixel(rgb_to_rgb555_u16(0, 255, 0)), [0, 255, 0, 255]);
        assert_eq!(rgb555_pixel(rgb_to_rgb555_u16(0, 0, 255)), [0, 0, 255, 255]);
    }

    #[test]
    fn zstd_with_dict() {
        let dict = b"zstd dictionary sample text";
        let data = b"zstd dictionary sample text -- repeat -- repeat".repeat(20);
        let mut comp = zstd::bulk::Compressor::with_dictionary(3, dict).unwrap();
        let packed = comp.compress(&data).unwrap();
        assert_eq!(decompress(&packed, Some(dict)).unwrap(), data);
    }

    #[test]
    fn init_carries_os_tag() {
        let m = init_message(1920, 1080, "HOST-1");
        let parts = crate::multipart::decode(&m[1..]).unwrap();
        // The first 16 parts keep the UltraViewer layout; the extra part is the OS tag.
        assert_eq!(parts.len(), 17);
        assert_eq!(u32::from_le_bytes(parts[1][..4].try_into().unwrap()), 1920);
        assert_eq!(parts[6], crate::control::VERSION.as_bytes());
        assert_eq!(os_from_init(&parts), this_os());
        // A real UltraViewer init (16 parts, no tag) means Windows.
        assert_eq!(os_from_init(&parts[..16]), "windows");
        // An unknown tag is ignored.
        let mut p2 = parts.clone();
        p2[16] = b"XR-OS:haiku".to_vec();
        assert_eq!(os_from_init(&p2), "windows");
    }

    #[test]
    fn pong_is_seven_bytes() {
        // Cleartext a real UltraViewer host sends for each `20` (pcap 04/11).
        assert_eq!(pong_message(), vec![0x00, 0x02, 0x00, 0x00, 0x00, 0x14, 0x00]);
    }

    #[test]
    fn merge_adjacent_row_tiles() {
        let t = |x, y| Region { x, y, w: 32, h: 32 };
        let merged = merge_rows(&[t(0, 0), t(32, 0), t(96, 0), t(0, 32), t(32, 32)]);
        assert_eq!(
            merged,
            vec![
                Region { x: 0, y: 0, w: 64, h: 32 },
                Region { x: 96, y: 0, w: 32, h: 32 },
                Region { x: 0, y: 32, w: 64, h: 32 },
            ]
        );
    }

    #[test]
    fn scan_covers_screen_once() {
        // 100×100 → 4×4 tiles; 2 rows/frame → 2 frames cover the screen.
        let (w, h) = (100usize, 100usize);
        let a = vec![0u8; w * h * 3];
        let mut b = a.clone();
        b[(40 * w + 40) * 3] = 9; // one changed pixel in tile (32,32), inside the first pass
        let mut scan = Some(0);
        let k1 = tiles_due(Some(&a), &b, w, h, &mut scan, 2);
        assert_eq!(scan, Some(2));
        assert_eq!(k1.len(), 8, "2 rows × 4 tiles; the dirty tile is not duplicated");
        assert!(k1.windows(2).all(|p| (p[0].y, p[0].x) < (p[1].y, p[1].x)));
        let k2 = tiles_due(Some(&b), &b, w, h, &mut scan, 2);
        assert_eq!(scan, None);
        assert_eq!(k2.len(), 8);
        let mut covered: Vec<Region> = k1.into_iter().chain(k2).collect();
        covered.sort_by_key(|t| (t.y, t.x));
        assert_eq!(covered, all_tiles(w, h), "two frames cover the whole screen");
        // Scan finished and the screen is idle → still one tile.
        let k3 = tiles_due(Some(&b), &b, w, h, &mut scan, 2);
        assert_eq!(k3, vec![Region { x: 0, y: 0, w: 32, h: 32 }]);
        // A change outside the scan → only the dirty tile.
        let mut c = b.clone();
        c[(99 * w + 99) * 3] = 1;
        let k4 = tiles_due(Some(&b), &c, w, h, &mut scan, 2);
        assert_eq!(k4, vec![Region { x: 96, y: 96, w: 4, h: 4 }]);
    }

    #[test]
    fn dirty_frame_round_trips() {
        // A frame that carries only dirty regions (no atlas) must decode to the same rects and pixels.
        let (w, h) = (96usize, 64usize);
        let mut rgb = vec![0u8; w * h * 3];
        for (i, b) in rgb.iter_mut().enumerate() {
            *b = (i * 7 % 251) as u8;
        }
        let rects = merge_rows(&[
            Region { x: 32, y: 0, w: 32, h: 32 },
            Region { x: 64, y: 0, w: 32, h: 32 },
            Region { x: 0, y: 32, w: 32, h: 32 },
        ]);
        let px = pack_regions_rgb555(&rgb, w, &rects);
        assert_eq!(px.len(), (64 * 32 + 32 * 32) * 2);
        let msg = frame_message_ext(&rects, &px, 7, &[], &[], screen_dict()).unwrap();
        let parts = crate::multipart::decode(&msg[1..]).unwrap();
        assert_eq!(parts.len(), 11);
        let f = parse(&parts, screen_dict()).unwrap();
        assert_eq!(f.rects, rects);
        assert_eq!(f.pixels, px);
        assert_eq!(f.frame_no as u32, 0x8000_0007);
    }
}

/// Encode a **32×N JPEG atlas** (part 7) plus one rect per tile (part 8).
///
/// Real UltraViewer draws the screen as 32×32 JPEG tiles stacked in one image
/// (VDrawer: record X,Y → cached JPEG). RGB555 alone is accepted but **draws
/// nothing** (black screen).
pub fn jpeg_tiles(rgb: &[u8], w: usize, h: usize, quality: u8) -> Option<(Vec<u8>, Vec<Region>)> {
    const T: usize = 32;
    if rgb.len() < w * h * 3 || w == 0 || h == 0 {
        return None;
    }
    let cols = w.div_ceil(T);
    let rows = h.div_ceil(T);
    let n = cols * rows;
    if n == 0 || 32 * n > 65500 {
        return None;
    }
    let mut canvas = vec![0u8; T * (T * n) * 3];
    let mut rects = Vec::with_capacity(n);
    for ry in 0..rows {
        for cx in 0..cols {
            let idx = ry * cols + cx;
            let x = cx * T;
            let y = ry * T;
            let tw = T.min(w - x);
            let th = T.min(h - y);
            for yy in 0..th {
                for xx in 0..tw {
                    let si = ((y + yy) * w + (x + xx)) * 3;
                    let di = ((idx * T + yy) * T + xx) * 3;
                    canvas[di..di + 3].copy_from_slice(&rgb[si..si + 3]);
                }
            }
            rects.push(Region { x: x as i32, y: y as i32, w: tw as i32, h: th as i32 });
        }
    }
    let mut jpeg = Vec::new();
    let enc = jpeg_encoder::Encoder::new(&mut jpeg, quality);
    enc.encode(&canvas, T as u16, (T * n) as u16, jpeg_encoder::ColorType::Rgb).ok()?;
    Some((jpeg, rects))
}

/// Full 11-part frame message, with a JPEG atlas (parts 7/8).
pub fn frame_message_ext(
    rects: &[Region],
    rgb555: &[u8],
    frame_no: i32,
    jpeg: &[u8],
    aux: &[Region],
    dict: Option<&[u8]>,
) -> Option<Vec<u8>> {
    let rect_bytes = encode_regions(rects);
    let packed = compress(rgb555, dict, 3)?;
    let fno = (0x8000_0000u32 | (frame_no as u32 & 0x7FFF_FFFF)).to_le_bytes();
    let cmd = 1u16.to_le_bytes();
    let flag = [0u8];
    let aux_bytes: Vec<u8> = aux
        .iter()
        .flat_map(|r| {
            let v = [r.x as i16, r.y as i16, r.w as i16, r.h as i16];
            v.iter().flat_map(|x| x.to_le_bytes()).collect::<Vec<u8>>()
        })
        .collect();
    let parts: [&[u8]; 11] = [
        &cmd,
        &rect_bytes,
        &packed,
        &flag,
        &fno,
        &[],
        &[],
        jpeg,
        &aux_bytes,
        &[],
        &[],
    ];
    let body = crate::multipart::encode(&parts);
    let mut msg = Vec::with_capacity(1 + body.len());
    msg.push(0u8);
    msg.extend_from_slice(&body);
    Some(msg)
}

/// 32×32 tiles that **changed** between two frames.
pub fn dirty_tiles(prev: &[u8], cur: &[u8], w: usize, h: usize) -> Vec<Region> {
    const T: usize = 32;
    let mut out = Vec::new();
    if prev.len() < w * h * 3 || cur.len() < w * h * 3 {
        return out;
    }
    let cols = w.div_ceil(T);
    let rows = h.div_ceil(T);
    for ry in 0..rows {
        for cx in 0..cols {
            let x = cx * T;
            let y = ry * T;
            let tw = T.min(w - x);
            let th = T.min(h - y);
            // Compare each tile row at once (memcmp) — much faster than per byte.
            let differ = (0..th).any(|yy| {
                let a = ((y + yy) * w + x) * 3;
                let b = a + tw * 3;
                prev[a..b] != cur[a..b]
            });
            if differ {
                out.push(Region { x: x as i32, y: y as i32, w: tw as i32, h: th as i32 });
            }
        }
    }
    out
}

/// RGB555 for each rect, packed in `rects` order.
pub fn pack_regions_rgb555(rgb: &[u8], w: usize, rects: &[Region]) -> Vec<u8> {
    let mut out = Vec::new();
    for r in rects {
        if r.w <= 0 || r.h <= 0 {
            continue;
        }
        for yy in 0..r.h as usize {
            let y = r.y as usize + yy;
            if y >= rgb.len() / (w * 3).max(1) { break; }
            for xx in 0..r.w as usize {
                let x = r.x as usize + xx;
                let i = (y * w + x) * 3;
                if i + 2 >= rgb.len() { break; }
                out.extend_from_slice(&rgb_to_rgb555_u16(rgb[i], rgb[i + 1], rgb[i + 2]).to_le_bytes());
            }
        }
    }
    out
}

/// JPEG atlas for a **given tile list** (each tile ≤32×32).
pub fn jpeg_tiles_for(
    rgb: &[u8],
    w: usize,
    h: usize,
    rects: &[Region],
    quality: u8,
) -> Option<(Vec<u8>, Vec<Region>)> {
    const T: usize = 32;
    if rects.is_empty() || rgb.len() < w * h * 3 {
        return None;
    }
    let n = rects.len();
    if 32 * n > 65500 {
        return None;
    }
    let mut canvas = vec![0u8; T * (T * n) * 3];
    for (idx, r) in rects.iter().enumerate() {
        for yy in 0..r.h.max(0) as usize {
            let y = r.y as usize + yy;
            if y >= h {
                break;
            }
            for xx in 0..r.w.max(0) as usize {
                let x = r.x as usize + xx;
                if x >= w {
                    break;
                }
                let si = (y * w + x) * 3;
                let di = ((idx * T + yy) * T + xx) * 3;
                canvas[di..di + 3].copy_from_slice(&rgb[si..si + 3]);
            }
        }
    }
    let mut jpeg = Vec::new();
    jpeg_encoder::Encoder::new(&mut jpeg, quality)
        .encode(&canvas, T as u16, (T * n) as u16, jpeg_encoder::ColorType::Rgb)
        .ok()?;
    Some((jpeg, rects.to_vec()))
}

/// Every 32×32 tile of the screen (first frame).
pub fn all_tiles(w: usize, h: usize) -> Vec<Region> {
    const T: usize = 32;
    let mut out = Vec::new();
    for y in (0..h).step_by(T) {
        for x in (0..w).step_by(T) {
            out.push(Region {
                x: x as i32,
                y: y as i32,
                w: T.min(w - x) as i32,
                h: T.min(h - y) as i32,
            });
        }
    }
    out
}
