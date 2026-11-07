//! Encode keyboard and mouse for the peer.
//!
//! A peer at version 6.2.13 or newer uses an 11-byte binary message:
//! ```text
//! 30 '|' '|' '|' kind vk scan_lo scan_hi char_lo char_hi caps
//! ```
//! sent with flag 1 (binary). Older peers use `8|||256|||<vk>|||-1|||-1` with flag 0.
//! The mouse is always the text message `8|||<msg>|||<wparam>|||<lparam>`.

pub const KEY_DOWN: u8 = 1;
pub const KEY_UP: u8 = 2;
pub const SYS_KEY_DOWN: u8 = 3;
pub const SYS_KEY_UP: u8 = 4;

pub const WM_MOUSEMOVE: u32 = 512;
pub const WM_LBUTTONDOWN: u32 = 513;
pub const WM_LBUTTONUP: u32 = 514;
pub const WM_RBUTTONDOWN: u32 = 516;
pub const WM_RBUTTONUP: u32 = 517;
pub const WM_MBUTTONDOWN: u32 = 519;
pub const WM_MBUTTONUP: u32 = 520;
pub const WM_MOUSEWHEEL: u32 = 522;
pub const WM_LBUTTONDBLCLK: u32 = 515;
pub const WM_RBUTTONDBLCLK: u32 = 518;
pub const WM_MBUTTONDBLCLK: u32 = 521;

/// Keys Windows treats as "system keys" (sent as kind 3/4).
const SYSTEM_VKS: &[u8] = &[
    0x09, 0x10, 0x11, 0x12, 0x14, 0x1B, 0x20, 0x25, 0x26, 0x27, 0x28, 0x5B, 0x5C, 0x5D, 0x90,
    0x91, 0x2D, 0x2E, 0x24, 0x23, 0x21, 0x22,
];

/// 11-byte payload of one key down or key up.
pub fn key_payload(kind: u8, vk: u8, scan: u16, ch: u16, caps: bool) -> [u8; 11] {
    [
        0x1e,
        0x7c,
        0x7c,
        0x7c,
        kind,
        vk,
        (scan & 0xff) as u8,
        (scan >> 8) as u8,
        (ch & 0xff) as u8,
        (ch >> 8) as u8,
        u8::from(caps),
    ]
}

/// Kind that fits a virtual-key code.
pub fn kind_for(vk: u8, down: bool) -> u8 {
    let sys = SYSTEM_VKS.contains(&vk);
    match (sys, down) {
        (true, true) => SYS_KEY_DOWN,
        (true, false) => SYS_KEY_UP,
        (false, true) => KEY_DOWN,
        (false, false) => KEY_UP,
    }
}

/// Text keyboard message for an old peer (< 6.2.13).
pub fn key_frame_legacy(vk: u8, down: bool) -> Vec<u8> {
    let msg = if down { 256 } else { 257 };
    format!("\x008|||{msg}|||{vk}|||-1|||-1").into_bytes()
}

/// Windows-style `lParam`: LOWORD = x, HIWORD = y.
pub fn packed_position(x: i32, y: i32) -> u32 {
    ((x as u32) & 0xffff) | (((y as u32) & 0xffff) << 16)
}

pub fn mouse_move(x: i32, y: i32) -> Vec<u8> {
    format!("\x008|||{WM_MOUSEMOVE}|||0|||{x}|||{y}").into_bytes()
}

pub fn mouse_button(msg: u32, x: i32, y: i32) -> Vec<u8> {
    format!("\x008|||{msg}|||0|||{}", packed_position(x, y)).into_bytes()
}

/// Mouse wheel: `wParam = delta << 16`, written as a **signed** Int32.
///
/// The old code wrote an unsigned value (-120 → `4287102976`). On a real Windows
/// box on 2026-11-04, UltraViewer 6.6.124 read that as Int32, reported
/// "VCT_PThread error: Arithmetic operation resulted in an overflow", and the
/// input thread stopped accepting mouse and keys for the session.
/// Both scroll-up and scroll-down as signed values ran on a real machine (2026-11-05).
pub fn mouse_wheel(delta: i32, x: i32, y: i32) -> Vec<u8> {
    let delta = delta.clamp(i16::MIN as i32, i16::MAX as i32);
    let wparam: i32 = delta << 16;
    format!("\x008|||{WM_MOUSEWHEEL}|||{wparam}|||{}", packed_position(x, y)).into_bytes()
}

/// Picture-quality command (0..100), already clamped to the valid range.
pub fn quality_frame(value: i32) -> Vec<u8> {
    format!("\x0031|||{}", value.clamp(0, 100)).into_bytes()
}

/// Color mode: 2 = RGB555.
pub fn color_mode_frame(mode: i32) -> Vec<u8> {
    format!("\x0032|||{mode}").into_bytes()
}

/// Show (`23|||-1`) or hide (`23|||0`) the peer wallpaper. Hiding it saves bandwidth.
pub fn wallpaper_frame(visible: bool) -> Vec<u8> {
    format!("\x0023|||{}", if visible { -1 } else { 0 }).into_bytes()
}


/// Wrap an 11-byte key event for the session: flag byte **0**, then the payload.
///
/// Old notes said flag 1. On a real Windows machine (UltraViewer 6.6.x, 2026-11-04)
/// flag 1 was ignored completely; flag 0 (and the old text form) did type.
pub fn key_frame(payload: &[u8; 11]) -> Vec<u8> {
    let mut f = Vec::with_capacity(12);
    f.push(0u8);
    f.extend_from_slice(payload);
    f
}

/// Two events (down + up) for one typeable character.
///
/// An uppercase character is sent with its upper code. Punctuation that needs Shift
/// still sends the character code so the peer sets key state itself — same as the
/// proven Python port.
pub fn char_events(ch: char) -> Option<[[u8; 11]; 2]> {
    use crate::keymap;
    let Some((def, _can_shift)) = keymap::lookup(ch) else {
        // Off the keymap (accented letters, symbols, …): vk 0, scan 0, UTF-16 only.
        // Tried on Windows 6.6.x on 2026-11-04: U+1EC7 U+0110 typed correctly.
        // A character outside the BMP (emoji) needs two UTF-16 units; not supported yet.
        let code = u16::try_from(ch as u32).ok()?;
        if ch.is_control() {
            return None;
        }
        return Some([
            key_payload(KEY_DOWN, 0, 0, code, false),
            key_payload(KEY_UP, 0, 0, code, false),
        ]);
    };
    // The peer receives the character code. Shift/Caps state comes from separate keys.
    let code = ch as u16;
    let (dn, up) = if keymap::is_system(def.vk) {
        (SYS_KEY_DOWN, SYS_KEY_UP)
    } else {
        (KEY_DOWN, KEY_UP)
    };
    Some([
        key_payload(dn, def.vk, def.scan, code, false),
        key_payload(up, def.vk, def.scan, code, false),
    ])
}

/// One event for a special key (`enter`, `tab`, `left`, `f5`, …).
pub fn special_events(name: &str, down: bool) -> Option<[u8; 11]> {
    use crate::keymap;
    let (vk, scan) = keymap::special(name)?;
    Some(key_payload(kind_for(vk, down), vk, scan, 0, false))
}

/// One event for a bare virtual-key code (Shift, Ctrl, Alt, function keys).
///
/// The scan code comes from the keymap. Without a scan code the peer usually ignores the key.
pub fn vk_events(vk: u8, down: bool) -> [u8; 11] {
    key_payload(kind_for(vk, down), vk, crate::keymap::scan_of(vk), 0, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_payload_reference() {
        // From the reference port: keyboard(KEY_DOWN, 0x41, 0x001E, 0x0041, false)
        assert_eq!(
            key_payload(KEY_DOWN, 0x41, 0x001e, 0x0041, false),
            [0x1e, 0x7c, 0x7c, 0x7c, 0x01, 0x41, 0x1e, 0x00, 0x41, 0x00, 0x00]
        );
    }

    #[test]
    fn scan_high_byte_and_caps() {
        let p = key_payload(KEY_DOWN, 0x2d, 0x0152, 0, true);
        assert_eq!(p[6], 0x52);
        assert_eq!(p[7], 0x01);
        assert_eq!(p[10], 1);
    }

    #[test]
    fn system_key_kinds() {
        assert_eq!(kind_for(0x1b, true), SYS_KEY_DOWN); // Esc
        assert_eq!(kind_for(0x1b, false), SYS_KEY_UP);
        assert_eq!(kind_for(0x0d, true), KEY_DOWN); // Enter
        assert_eq!(kind_for(0x41, false), KEY_UP); // 'a'
    }

    #[test]
    fn legacy_key_frame() {
        assert_eq!(key_frame_legacy(16, true), b"\x008|||256|||16|||-1|||-1");
        assert_eq!(key_frame_legacy(16, false), b"\x008|||257|||16|||-1|||-1");
    }

    #[test]
    fn mouse_packs_lparam() {
        assert_eq!(packed_position(10, 20), 10 | (20 << 16));
        assert_eq!(mouse_move(100, 200), b"\x008|||512|||0|||100|||200");
        assert_eq!(
            mouse_button(WM_LBUTTONDOWN, 10, 20),
            format!("\x008|||513|||0|||{}", 10 | (20 << 16)).into_bytes()
        );
        assert_eq!(
            mouse_wheel(120, 0, 0),
            b"\x008|||522|||7864320|||0"
        );
        // Signed: the unsigned value 4287102976 makes UltraViewer overflow and stall input.
        assert_eq!(mouse_wheel(-120, 0, 0), b"\x008|||522|||-7864320|||0");
        assert_eq!(mouse_wheel(-240, 0, 0), b"\x008|||522|||-15728640|||0");
        // A delta past i16 is clamped, so it never overflows Int32.
        assert_eq!(mouse_wheel(-100_000, 0, 0), format!("\x008|||522|||{}|||0", -32768i32 << 16).into_bytes());
    }

    #[test]
    fn quality_commands() {
        assert_eq!(quality_frame(30), b"\x0031|||30");
        assert_eq!(quality_frame(500), b"\x0031|||100");
        assert_eq!(color_mode_frame(2), b"\x0032|||2");
    }

    #[test]
    fn key_frame_uses_flag_0() {
        // Flag 1 was ignored by a real Windows peer (tried 2026-11-04); it must be flag 0.
        let p = key_payload(KEY_DOWN, 0x41, 0x1e, 0x61, false);
        let f = key_frame(&p);
        assert_eq!(f[0], 0);
        assert_eq!(&f[1..], &p);
    }

    #[test]
    fn off_keymap_char_sends_utf16() {
        let [dn, up] = char_events('\u{1ec7}').expect("an accented char must type");
        assert_eq!(dn, key_payload(KEY_DOWN, 0, 0, 0x1EC7, false));
        assert_eq!(up, key_payload(KEY_UP, 0, 0, 0x1EC7, false));
        assert!(char_events('😀').is_none(), "outside the BMP is not supported yet");
        assert!(char_events('\u{7}').is_none(), "a control character is not sent");
        // A character that is in the table still carries a real vk/scan.
        assert_eq!(char_events('a').unwrap()[0][5], 0x41);
    }

    #[test]
    fn wallpaper_matches_python() {
        // Same values as `wallpaper_frame` in app/test_input.py.
        assert_eq!(wallpaper_frame(false), b"\x0023|||0");
        assert_eq!(wallpaper_frame(true), b"\x0023|||-1");
    }
}
