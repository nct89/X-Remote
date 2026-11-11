//! Host-side mouse/keyboard injection into X11 (Linux only) via the XTEST extension.
//!
//! Commands are parsed in [`crate::input_cmd`]; the UltraViewer controller sends (see [`crate::input`]):
//! * mouse — text `8|||<msg>|||<wparam>|||<lparam>` (flag 0);
//! * key   — 11 bytes `1e 7c 7c 7c kind vk scan_lo scan_hi ch_lo ch_hi caps` (flag 0).
//!
//! This module decodes those and posts synthetic events. Any character is injected by
//! temporarily mapping its keysym onto a spare keycode, then pressing it ("remap keycode").

use x11rb::connection::Connection;
use x11rb::protocol::xproto::{ConnectionExt as _, Keysym};
use x11rb::protocol::xtest::ConnectionExt as _;
use x11rb::rust_connection::RustConnection;

pub use crate::input_cmd::{parse_cmd, InputCmd};

/// Release mouse buttons and keys still held when a host session ends.
/// X11 does not remember that state (XTEST). macOS does — this function
/// exists so `hostmode` can call it on both platforms.
pub fn release_all(_inj: &InputInjector) {}

const MOTION_NOTIFY: u8 = 6;
const BUTTON_PRESS: u8 = 4;
const BUTTON_RELEASE: u8 = 5;
const KEY_PRESS: u8 = 2;
const KEY_RELEASE: u8 = 3;

/// Input injector bound to one X11 screen.
pub struct InputInjector {
    conn: RustConnection,
    root: u32,
    /// Spare keycode used to temporarily map a keysym when typing a character.
    spare: u8,
    min_keycode: u8,
    /// keysym (column 0) → real keyboard keycode. Press/release uses the real keycode
    /// so held modifiers (Ctrl, Shift, …) combine with the next key; only keysyms
    /// missing from the keyboard borrow `spare`.
    keysym_kc: std::collections::HashMap<Keysym, u8>,
}

impl InputInjector {
    pub fn new() -> Result<InputInjector, String> {
        let (conn, screen_num) = x11rb::connect(None).map_err(|e| format!("X11 connect failed: {e}"))?;
        let root = conn.setup().roots[screen_num].root;
        let min_keycode = conn.setup().min_keycode;
        let max_keycode = conn.setup().max_keycode;
        // Find a keycode whose keysyms are all NoSymbol (0) to use as `spare`.
        let count = max_keycode - min_keycode + 1;
        let map = conn
            .get_keyboard_mapping(min_keycode, count)
            .map_err(|e| format!("GetKeyboardMapping failed: {e}"))?
            .reply()
            .map_err(|e| format!("GetKeyboardMapping reply failed: {e}"))?;
        let per = map.keysyms_per_keycode as usize;
        let mut spare = max_keycode; // default: highest keycode
        let mut find_spare = true;
        let mut keysym_kc = std::collections::HashMap::new();
        for kc in 0..count as usize {
            let s = &map.keysyms[kc * per..(kc + 1) * per];
            if find_spare && s.iter().all(|&k| k == 0) {
                spare = min_keycode + kc as u8;
                find_spare = false;
            }
            if let Some(&k0) = s.first() {
                if k0 != 0 {
                    keysym_kc.entry(k0).or_insert(min_keycode + kc as u8);
                }
            }
        }
        keysym_kc.remove(&0);
        Ok(InputInjector { conn, root, spare, min_keycode, keysym_kc })
    }

    fn fake(&self, kind: u8, detail: u8, x: i16, y: i16) -> Result<(), String> {
        self.conn
            .xtest_fake_input(kind, detail, 0, self.root, x, y, 0)
            .map_err(|e| format!("XTEST send failed: {e}"))?;
        self.conn.flush().map_err(|e| format!("flush failed: {e}"))?;
        Ok(())
    }

    /// Move the pointer to absolute (x, y).
    pub fn mouse_move(&self, x: i32, y: i32) -> Result<(), String> {
        self.fake(MOTION_NOTIFY, 0, x as i16, y as i16)
    }

    /// Current pointer position (for tests).
    pub fn mouse_pos(&self) -> Result<(i32, i32), String> {
        let p = self
            .conn
            .query_pointer(self.root)
            .map_err(|e| format!("QueryPointer failed: {e}"))?
            .reply()
            .map_err(|e| format!("QueryPointer reply failed: {e}"))?;
        Ok((p.root_x as i32, p.root_y as i32))
    }

    /// Press/release a mouse button (1 = left, 2 = middle, 3 = right).
    pub fn mouse_btn(&self, button: u8, down: bool) -> Result<(), String> {
        self.fake(if down { BUTTON_PRESS } else { BUTTON_RELEASE }, button, 0, 0)
    }

    /// Scroll: delta > 0 up (button 4), < 0 down (button 5). Each 120 units = 1 step.
    pub fn mouse_wheel(&self, delta: i32) -> Result<(), String> {
        let btn = if delta > 0 { 4 } else { 5 };
        let steps = (delta.abs() / 120).max(1);
        for _ in 0..steps {
            self.fake(BUTTON_PRESS, btn, 0, 0)?;
            self.fake(BUTTON_RELEASE, btn, 0, 0)?;
        }
        Ok(())
    }

    /// Press then release a keysym (temporarily mapped onto the spare keycode).
    pub fn type_sym(&self, keysym: Keysym) -> Result<(), String> {
        if keysym == 0 {
            return Ok(());
        }
        // Map the keysym onto the spare keycode.
        self.conn
            .change_keyboard_mapping(1, self.spare, 1, &[keysym])
            .map_err(|e| format!("ChangeKeyboardMapping failed: {e}"))?;
        self.conn.flush().ok();
        self.fake(KEY_PRESS, self.spare, 0, 0)?;
        self.fake(KEY_RELEASE, self.spare, 0, 0)?;
        Ok(())
    }

    /// Type one Unicode character (keysym 0x01000000 + codepoint).
    pub fn type_char(&self, ch: char) -> Result<(), String> {
        let cp = ch as u32;
        // Printable ASCII: keysym = code; otherwise the X Unicode range.
        let keysym = if (0x20..0x7f).contains(&cp) { cp } else { 0x0100_0000 + cp };
        self.type_sym(keysym)
    }

    /// Press/release a single keysym (for special keys).
    pub fn key_sym(&self, keysym: Keysym, down: bool) -> Result<(), String> {
        if keysym == 0 {
            return Ok(());
        }
        if let Some(&kc) = self.keysym_kc.get(&keysym) {
            return self.fake(if down { KEY_PRESS } else { KEY_RELEASE }, kc, 0, 0);
        }
        self.conn
            .change_keyboard_mapping(1, self.spare, 1, &[keysym])
            .map_err(|e| format!("ChangeKeyboardMapping failed: {e}"))?;
        self.conn.flush().ok();
        self.fake(if down { KEY_PRESS } else { KEY_RELEASE }, self.spare, 0, 0)?;
        let _ = self.min_keycode;
        Ok(())
    }
}

/// Windows virtual-key → X11 keysym for common special keys.
pub fn vk_to_keysym(vk: u8) -> Option<Keysym> {
    Some(match vk {
        0x08 => 0xff08, // Backspace
        0x09 => 0xff09, // Tab
        0x0d => 0xff0d, // Enter
        0x10 => 0xffe1, // Shift
        0x11 => 0xffe3, // Ctrl
        0x12 => 0xffe9, // Alt
        0x14 => 0xffe5, // CapsLock
        0x1b => 0xff1b, // Esc
        0x20 => 0x0020, // Space
        0x21 => 0xff55, // PageUp
        0x22 => 0xff56, // PageDown
        0x23 => 0xff57, // End
        0x24 => 0xff50, // Home
        0x25 => 0xff51, // Left
        0x26 => 0xff52, // Up
        0x27 => 0xff53, // Right
        0x28 => 0xff54, // Down
        0x2d => 0xff63, // Insert
        0x2e => 0xffff, // Delete
        0x70..=0x7b => 0xffbe + (vk - 0x70) as u32, // F1..F12
        0x30..=0x39 => vk as u32,                    // 0..9
        0x41..=0x5a => (vk + 0x20) as u32,           // A..Z → lowercase keysym
        0x5b => 0xffeb,                              // left Win (Super_L)
        0x5c => 0xffec,                              // right Win
        0x5d => 0xff67,                              // Menu
        0x60..=0x69 => 0xffb0 + (vk - 0x60) as u32,  // NumPad 0..9
        0x6a => 0xffaa,                              // NumPad *
        0x6b => 0xffab,                              // NumPad +
        0x6d => 0xffad,                              // NumPad -
        0x6e => 0xffae,                              // NumPad .
        0x6f => 0xffaf,                              // NumPad /
        0x90 => 0xff7f,                              // NumLock
        0x91 => 0xff14,                              // ScrollLock
        0x2c => 0xff61,                              // PrintScreen
        0x13 => 0xff13,                              // Pause
        0xa0 => 0xffe1,                              // left Shift
        0xa1 => 0xffe2,                              // right Shift
        0xa2 => 0xffe3,                              // left Ctrl
        0xa3 => 0xffe4,                              // right Ctrl
        0xa4 => 0xffe9,                              // left Alt
        0xa5 => 0xffea,                              // right Alt
        0xba => 0x3b,                                // ;
        0xbb => 0x3d,                                // =
        0xbc => 0x2c,                                // ,
        0xbd => 0x2d,                                // -
        0xbe => 0x2e,                                // .
        0xbf => 0x2f,                                // /
        0xc0 => 0x60,                                // `
        0xdb => 0x5b,                                // [
        0xdc => 0x5c,                                // \
        0xdd => 0x5d,                                // ]
        0xde => 0x27,                                // '
        _ => return None,
    })
}

/// Apply one input command to the screen.
///
/// Printable keys (ch ≥ 0x20): type press+release on the DOWN event (skip UP).
/// Special keys (Enter, arrows, modifiers, …): press on DOWN, release on UP via [`vk_to_keysym`].
pub fn apply_input(inj: &InputInjector, cmd: &InputCmd) -> Result<(), String> {
    match cmd {
        InputCmd::Move { x, y } => inj.mouse_move(*x, *y),
        InputCmd::Btn { button, down, .. } => {
            // Click at the current pointer, NOT at lParam: the real UltraViewer
            // sends button lParam in the scaled view (measured 05/11 19:40:
            // remote click (1700,900) → lParam (1566,829) = ×0.9214 when a 1769 px
            // window shows a 1920 px screen), while `512` mouse moves carry true
            // coordinates and always arrive first. Moving by lParam shifts every
            // click toward the top-left.
            inj.mouse_btn(*button, *down)
        }
        InputCmd::Wheel { delta } => inj.mouse_wheel(*delta),
        InputCmd::Key { kind, vk, ch } => {
            let down = *kind == 1 || *kind == 3;
            if *ch >= 0x20 && *ch != 0x7f {
                if down {
                    if let Some(c) = char::from_u32(*ch as u32) {
                        inj.type_char(c)?;
                    }
                }
                Ok(())
            } else if let Some(ks) = vk_to_keysym(*vk) {
                inj.key_sym(ks, down)
            } else {
                Ok(())
            }
        }
        InputCmd::Other => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vk_maps_to_x11_keysym() {
        // Measured 05/11 19:30: left Ctrl (162), left Shift (160), 'S' (83).
        assert_eq!(vk_to_keysym(162), Some(0xffe3));
        assert_eq!(vk_to_keysym(160), Some(0xffe1));
        assert_eq!(vk_to_keysym(83), Some(b's' as u32));
        assert_eq!(vk_to_keysym(0x37), Some(b'7' as u32));
    }
}
