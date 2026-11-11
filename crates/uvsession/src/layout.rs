//! Screen layout of the peer.
//!
//! A peer with several monitors sends a small text message
//! `x:y:x2:y2:id|...` (for example `0:0:1920:1080:-1|272:1080:2192:2160:0|`), sometimes
//! as UTF-16 (with zero bytes interleaved). Ported from `parse_monitors` in `uvviewer.py`.

/// One monitor, in peer coordinates (also the coordinates on the combined canvas).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Monitor {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

/// A message larger than this is a picture frame, not a layout.
pub const MAX_LAYOUT_MSG: usize = 500;

/// Read `\d{1,5}` at `i`. Returns (value, index after the number).
fn read_num(b: &[u8], i: usize, max_len: usize) -> Option<(i64, usize)> {
    let mut j = i;
    let mut v: i64 = 0;
    while j < b.len() && b[j].is_ascii_digit() && j - i < max_len {
        v = v * 10 + (b[j] - b'0') as i64;
        j += 1;
    }
    (j > i).then_some((v, j))
}

/// Match `x:y:a:b:-?id` starting exactly at `i`. Returns the four numbers and the end index.
fn match_at(b: &[u8], i: usize) -> Option<([i64; 4], usize)> {
    let mut out = [0i64; 4];
    let mut j = i;
    for o in out.iter_mut() {
        let (v, end) = read_num(b, j, 5)?;
        *o = v;
        j = end;
        if b.get(j) != Some(&b':') {
            return None;
        }
        j += 1;
    }
    if b.get(j) == Some(&b'-') {
        j += 1;
    }
    let (_, end) = read_num(b, j, usize::MAX)?;
    Some((out, end))
}

fn scan(b: &[u8]) -> Vec<Monitor> {
    let mut found: Vec<Monitor> = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let Some(([x1, y1, a, c], end)) = match_at(b, i) else {
            i += 1;
            continue;
        };
        i = end;
        // Two forms: absolute x2/y2, or w/h directly.
        let (w, h) = if a > x1 && c > y1 { (a - x1, c - y1) } else { (a, c) };
        if !(640..=7680).contains(&w) || !(480..=4320).contains(&h) {
            continue;
        }
        let m = Monitor { x: x1 as i32, y: y1 as i32, w: w as i32, h: h as i32 };
        if !found.contains(&m) {
            found.push(m);
        }
    }
    found
}

/// Find a monitor layout in a message. Empty when there is none.
///
/// Results are sorted by position (top to bottom, left to right) so "Screen 1, Screen 2…"
/// stays stable across sessions. Measured 2026-11-05: the same peer, after UltraViewer was
/// reopened, sent the monitors in reverse order (`272:1080…` before `0:0…`).
pub fn parse_monitors(blob: &[u8]) -> Vec<Monitor> {
    if blob.is_empty() {
        return Vec::new();
    }
    let mut found = scan(blob);
    if found.is_empty() && blob.len() < MAX_LAYOUT_MSG {
        let stripped: Vec<u8> = blob.iter().copied().filter(|&c| c != 0).collect();
        found = scan(&stripped);
    }
    found.sort_by_key(|m| (m.y, m.x));
    found
}

/// Canvas size needed to hold every rect `(x, y, w, h)`, not smaller than the current size.
pub fn canvas_bounds(rects: impl IntoIterator<Item = (i32, i32, i32, i32)>, w: usize, h: usize) -> (usize, usize) {
    let (mut cw, mut ch) = (w as i64, h as i64);
    for (x, y, rw, rh) in rects {
        if rw <= 0 || rh <= 0 {
            continue;
        }
        cw = cw.max(x as i64 + rw as i64);
        ch = ch.max(y as i64 + rh as i64);
    }
    (cw.max(0) as usize, ch.max(0) as usize)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_monitors_absolute() {
        let m = parse_monitors(b"0:0:1920:1080:-1|272:1080:2192:2160:0|");
        assert_eq!(
            m,
            vec![
                Monitor { x: 0, y: 0, w: 1920, h: 1080 },
                Monitor { x: 272, y: 1080, w: 1920, h: 1080 },
            ]
        );
    }

    #[test]
    fn order_stable_when_peer_reverses() {
        let fwd = parse_monitors(b"0:0:1920:1080:-1|272:1080:2192:2160:0|");
        let rev = parse_monitors(b"272:1080:2192:2160:0|0:0:1920:1080:-1|");
        assert_eq!(fwd, rev);
        assert_eq!(rev[0], Monitor { x: 0, y: 0, w: 1920, h: 1080 });
        // Same row: left before right.
        let row = parse_monitors(b"1920:0:3200:1024:0|0:0:1920:1080:-1|");
        assert_eq!(row[0].x, 0);
    }

    #[test]
    fn utf16_with_zero_bytes() {
        let s = "\u{1}0:0:1920:1080:-1|1920:0:1280:1024:0|";
        let mut b = Vec::new();
        for u in s.encode_utf16() {
            b.extend_from_slice(&u.to_le_bytes());
        }
        let m = parse_monitors(&b);
        assert_eq!(m.len(), 2);
        assert_eq!(m[1], Monitor { x: 1920, y: 0, w: 1280, h: 1024 });
    }

    #[test]
    fn skip_bad_size_and_dupes() {
        assert!(parse_monitors(b"0:0:10:10:-1|").is_empty());
        assert_eq!(parse_monitors(b"0:0:1920:1080:-1|0:0:1920:1080:-1|").len(), 1);
        assert!(parse_monitors(b"").is_empty());
        assert!(parse_monitors(b"khong co gi").is_empty());
    }

    #[test]
    fn canvas_grows_to_outer_rect() {
        assert_eq!(canvas_bounds([(0, 0, 1920, 1080)], 1920, 1080), (1920, 1080));
        assert_eq!(canvas_bounds([(1920, 0, 1280, 1024)], 1920, 1080), (3200, 1080));
        assert_eq!(canvas_bounds([(272, 1080, 1920, 1080), (0, 0, -1, 5)], 1920, 1080), (2192, 2160));
    }
}
