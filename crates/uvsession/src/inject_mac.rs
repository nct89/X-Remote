//! Host-side mouse/keyboard injection on macOS via CGEvent — same interface as
//! the X11 build (`inject.rs`): `InputInjector::new()`, [`parse_cmd`], [`apply_input`], [`InputCmd`].
//!
//! Unlike X11, CGEvent does not track state. This code keeps:
//! * held mouse buttons → move becomes a *dragged* event (drag-drop, text select);
//! * click count (`kCGMouseEventClickState`) → double/triple click;
//! * held modifiers → flags on every later event (Cmd+C, Shift+click, …).
//!
//! Windows controller keys follow Windows habits:
//! Ctrl → Command (Ctrl+C/V/Z… works as on Windows), Alt → Option, Win → Control.
//! Accessibility permission is required (System Settings → Privacy & Security → Accessibility).
//! Without it macOS silently drops events — see [`request_accessibility`].

use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::time::{Duration, Instant};

pub use crate::input_cmd::{parse_cmd, InputCmd};

#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct CGPoint {
    x: f64,
    y: f64,
}

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGEventSourceCreate(state: i32) -> *mut c_void;
    fn CGEventCreate(src: *mut c_void) -> *mut c_void;
    fn CGEventGetLocation(e: *mut c_void) -> CGPoint;
    fn CGEventCreateMouseEvent(src: *mut c_void, kind: u32, p: CGPoint, button: u32) -> *mut c_void;
    fn CGEventCreateKeyboardEvent(src: *mut c_void, keycode: u16, down: bool) -> *mut c_void;
    fn CGEventCreateScrollWheelEvent2(src: *mut c_void, unit: u32, wheels: u32, w1: i32, w2: i32, w3: i32) -> *mut c_void;
    fn CGEventKeyboardSetUnicodeString(e: *mut c_void, n: usize, s: *const u16);
    fn CGEventSetFlags(e: *mut c_void, flags: u64);
    fn CGEventSetIntegerValueField(e: *mut c_void, field: u32, value: i64);
    fn CGEventPost(tap: u32, e: *mut c_void);
}
#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFRelease(o: *mut c_void);
    fn CFDictionaryCreate(
        alloc: *const c_void,
        keys: *const *const c_void,
        values: *const *const c_void,
        n: isize,
        kcb: *const c_void,
        vcb: *const c_void,
    ) -> *mut c_void;
    static kCFBooleanTrue: *const c_void;
    static kCFTypeDictionaryKeyCallBacks: u8;
    static kCFTypeDictionaryValueCallBacks: u8;
}
#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrusted() -> bool;
    fn AXIsProcessTrustedWithOptions(opts: *const c_void) -> bool;
    static kAXTrustedCheckOptionPrompt: *const c_void;
}

const HID_SRC: i32 = 1; // kCGEventSourceStateHIDSystemState
const TAP_HID: u32 = 0; // kCGHIDEventTap
const CLICK_STATE: u32 = 1; // kCGMouseEventClickState
const SCROLL_LINE: u32 = 1; // kCGScrollEventUnitLine

const FLAG_SHIFT: u64 = 0x0002_0000;
const FLAG_CTRL: u64 = 0x0004_0000;
const FLAG_OPT: u64 = 0x0008_0000;
const FLAG_CMD: u64 = 0x0010_0000;

/// Max gap between two presses that still counts as a double-click.
const DBL_CLICK_GAP: Duration = Duration::from_millis(500);

/// True if Accessibility is granted; otherwise ask macOS to show the Settings prompt (once).
pub fn request_accessibility() -> bool {
    unsafe {
        if AXIsProcessTrusted() {
            return true;
        }
        let k = [kAXTrustedCheckOptionPrompt];
        let v = [kCFBooleanTrue];
        let d = CFDictionaryCreate(
            std::ptr::null(),
            k.as_ptr(),
            v.as_ptr(),
            1,
            &kCFTypeDictionaryKeyCallBacks as *const u8 as *const c_void,
            &kCFTypeDictionaryValueCallBacks as *const u8 as *const c_void,
        );
        let ok = AXIsProcessTrustedWithOptions(d);
        if !d.is_null() {
            CFRelease(d);
        }
        ok
    }
}

/// Input injector for the current login session.
pub struct InputInjector {
    source: *mut c_void,
    /// Pointer in point coordinates (origin: top-left of the main display).
    pos: Cell<(f64, f64)>,
    /// Bits 0/1/2 = left/right/middle button held.
    held_btns: Cell<u8>,
    /// Last press: (button, time, consecutive click count).
    last_press: Cell<Option<(u8, Instant, i64)>>,
    clicks: Cell<i64>,
    /// Held macOS modifiers (bits by left/right keycode; see [`modifier_mac`]).
    held_mods: Cell<u8>,
    /// Non-modifier keycodes currently down (released if the session ends mid-key).
    held_keys: RefCell<Vec<u16>>,
}

impl Drop for InputInjector {
    fn drop(&mut self) {
        self.release_all();
        if !self.source.is_null() {
            unsafe { CFRelease(self.source) };
        }
    }
}

impl InputInjector {
    pub fn new() -> Result<InputInjector, String> {
        let source = unsafe { CGEventSourceCreate(HID_SRC) };
        let pos = unsafe {
            let e = CGEventCreate(std::ptr::null_mut());
            if e.is_null() {
                (0.0, 0.0)
            } else {
                let p = CGEventGetLocation(e);
                CFRelease(e);
                (p.x, p.y)
            }
        };
        Ok(InputInjector {
            source,
            pos: Cell::new(pos),
            held_btns: Cell::new(0),
            last_press: Cell::new(None),
            clicks: Cell::new(1),
            held_mods: Cell::new(0),
            held_keys: RefCell::new(Vec::new()),
        })
    }

    /// Release every mouse button and key still held. CGEvent does not release
    /// them when the socket closes; without this, Command/Shift/the pointer stay
    /// down and the id and password fields stop accepting text.
    pub fn release_all(&self) {
        for (bit, button) in [(1u8, 1u8), (2, 3u8), (4, 2u8)] {
            if self.held_btns.get() & bit != 0 {
                let _ = self.mouse_btn(button, false);
            }
        }
        // One virtual-key per modifier bit (left/right Shift, Command, Option, Control).
        const MOD_VK: [u8; 8] = [0xa0, 0xa1, 0xa2, 0xa3, 0xa4, 0xa5, 0x5b, 0x5c];
        for vk in MOD_VK {
            if let Some((_, bit)) = modifier_mac(vk) {
                if self.held_mods.get() & bit != 0 {
                    let _ = self.key_vk(vk, false);
                }
            }
        }
        let held: Vec<u16> = self.held_keys.borrow().clone();
        self.held_keys.borrow_mut().clear();
        for kc in held {
            let e = unsafe { CGEventCreateKeyboardEvent(self.source, kc, false) };
            let _ = self.post(e, 0);
        }
    }

    fn flags(&self) -> u64 {
        mods_from(self.held_mods.get())
    }

    /// Set modifier flags, post the event, and release it.
    fn post(&self, e: *mut c_void, flags: u64) -> Result<(), String> {
        if e.is_null() {
            return Err("CGEvent create failed".into());
        }
        unsafe {
            CGEventSetFlags(e, flags);
            CGEventPost(TAP_HID, e);
            CFRelease(e);
        }
        Ok(())
    }

    fn post_mouse(&self, kind: u32, btn: u32, clicks: Option<i64>) -> Result<(), String> {
        let (x, y) = self.pos.get();
        let e = unsafe { CGEventCreateMouseEvent(self.source, kind, CGPoint { x, y }, btn) };
        if let (false, Some(n)) = (e.is_null(), clicks) {
            unsafe { CGEventSetIntegerValueField(e, CLICK_STATE, n) };
        }
        self.post(e, self.flags())
    }

    /// Move the pointer to absolute (x, y) in points. A held button makes it a drag.
    pub fn mouse_move(&self, x: i32, y: i32) -> Result<(), String> {
        self.pos.set((x as f64, y as f64));
        let held = self.held_btns.get();
        let (kind, btn) = if held & 1 != 0 {
            (6, 0) // kCGEventLeftMouseDragged
        } else if held & 2 != 0 {
            (7, 1) // kCGEventRightMouseDragged
        } else if held & 4 != 0 {
            (27, 2) // kCGEventOtherMouseDragged
        } else {
            (5, 0) // kCGEventMouseMoved
        };
        self.post_mouse(kind, btn, None)
    }

    /// Current pointer position (for tests).
    pub fn mouse_pos(&self) -> Result<(i32, i32), String> {
        let (x, y) = self.pos.get();
        Ok((x as i32, y as i32))
    }

    /// Press/release a mouse button (1 = left, 2 = middle, 3 = right) at the current pointer.
    pub fn mouse_btn(&self, button: u8, down: bool) -> Result<(), String> {
        let (bit, btn, press, release) = match button {
            1 => (1u8, 0u32, 1u32, 2u32),
            3 => (2, 1, 3, 4),
            _ => (4, 2, 25, 26),
        };
        if down {
            let now = Instant::now();
            let n = match self.last_press.get() {
                Some((b, at, n)) if b == button && now.duration_since(at) <= DBL_CLICK_GAP => (n + 1).min(3),
                _ => 1,
            };
            self.last_press.set(Some((button, now, n)));
            self.clicks.set(n);
            self.held_btns.set(self.held_btns.get() | bit);
            self.post_mouse(press, btn, Some(n))
        } else {
            self.held_btns.set(self.held_btns.get() & !bit);
            self.post_mouse(release, btn, Some(self.clicks.get()))
        }
    }

    /// Scroll: delta > 0 up, < 0 down. Each 120 units = 1 line.
    pub fn mouse_wheel(&self, delta: i32) -> Result<(), String> {
        let lines = if delta.abs() < 120 { delta.signum() } else { delta / 120 };
        let e = unsafe { CGEventCreateScrollWheelEvent2(self.source, SCROLL_LINE, 1, lines, 0, 0) };
        self.post(e, self.flags())
    }

    /// Type one Unicode character (press + release, no modifiers).
    pub fn type_char(&self, ch: char) -> Result<(), String> {
        let mut buf = [0u16; 2];
        let s = ch.encode_utf16(&mut buf);
        for down in [true, false] {
            let e = unsafe { CGEventCreateKeyboardEvent(self.source, 0, down) };
            if !e.is_null() {
                unsafe { CGEventKeyboardSetUnicodeString(e, s.len(), s.as_ptr()) };
            }
            self.post(e, 0)?;
        }
        Ok(())
    }

    /// Press/release a key by Windows virtual-key code.
    pub fn key_vk(&self, vk: u8, down: bool) -> Result<(), String> {
        if let Some((kc, bit)) = modifier_mac(vk) {
            let m = if down { self.held_mods.get() | bit } else { self.held_mods.get() & !bit };
            self.held_mods.set(m);
            let e = unsafe { CGEventCreateKeyboardEvent(self.source, kc, down) };
            return self.post(e, mods_from(m));
        }
        let Some(kc) = vk_to_keycode(vk) else { return Ok(()) };
        {
            let mut held = self.held_keys.borrow_mut();
            if down {
                if !held.contains(&kc) {
                    held.push(kc);
                }
            } else {
                held.retain(|k| *k != kc);
            }
        }
        let e = unsafe { CGEventCreateKeyboardEvent(self.source, kc, down) };
        self.post(e, self.flags())
    }

    /// Command or Control is held (shortcut — send by keycode, not by character).
    fn shortcut_held(&self) -> bool {
        self.flags() & (FLAG_CMD | FLAG_CTRL) != 0
    }
}

/// Bits in `held_mods`: 0/1 left/right Shift, 2/3 Command, 4/5 Option, 6/7 Control.
fn mods_from(m: u8) -> u64 {
    let mut flags = 0;
    if m & 0b0000_0011 != 0 {
        flags |= FLAG_SHIFT;
    }
    if m & 0b0000_1100 != 0 {
        flags |= FLAG_CMD;
    }
    if m & 0b0011_0000 != 0 {
        flags |= FLAG_OPT;
    }
    if m & 0b1100_0000 != 0 {
        flags |= FLAG_CTRL;
    }
    flags
}

/// Windows modifier key → (macOS keycode, bit in `held_mods`).
/// Ctrl → Command, Alt → Option, Win → Control.
fn modifier_mac(vk: u8) -> Option<(u16, u8)> {
    Some(match vk {
        0x10 | 0xa0 => (0x38, 1 << 0), // Shift
        0xa1 => (0x3c, 1 << 1),
        0x11 | 0xa2 => (0x37, 1 << 2), // Ctrl → Command
        0xa3 => (0x36, 1 << 3),
        0x12 | 0xa4 => (0x3a, 1 << 4), // Alt → Option
        0xa5 => (0x3d, 1 << 5),
        0x5b => (0x3b, 1 << 6), // Win → Control
        0x5c => (0x3e, 1 << 7),
        _ => return None,
    })
}

/// Windows virtual-key → macOS keycode (ANSI layout, `kVK_*` in Events.h).
pub fn vk_to_keycode(vk: u8) -> Option<u16> {
    const LETTERS: [u16; 26] = [
        0x00, 0x0b, 0x08, 0x02, 0x0e, 0x03, 0x05, 0x04, 0x22, 0x26, 0x28, 0x25, 0x2e, // A..M
        0x2d, 0x1f, 0x23, 0x0c, 0x0f, 0x01, 0x11, 0x20, 0x09, 0x0d, 0x07, 0x10, 0x06, // N..Z
    ];
    const DIGITS: [u16; 10] = [0x1d, 0x12, 0x13, 0x14, 0x15, 0x17, 0x16, 0x1a, 0x1c, 0x19];
    const NUMPAD: [u16; 10] = [0x52, 0x53, 0x54, 0x55, 0x56, 0x57, 0x58, 0x59, 0x5b, 0x5c];
    const F: [u16; 12] = [0x7a, 0x78, 0x63, 0x76, 0x60, 0x61, 0x62, 0x64, 0x65, 0x6d, 0x67, 0x6f];
    Some(match vk {
        0x41..=0x5a => LETTERS[(vk - 0x41) as usize],
        0x30..=0x39 => DIGITS[(vk - 0x30) as usize],
        0x60..=0x69 => NUMPAD[(vk - 0x60) as usize],
        0x70..=0x7b => F[(vk - 0x70) as usize],
        0x08 => 0x33, // Backspace
        0x09 => 0x30, // Tab
        0x0d => 0x24, // Enter
        0x14 => 0x39, // CapsLock
        0x1b => 0x35, // Esc
        0x20 => 0x31, // Space
        0x21 => 0x74, // PageUp
        0x22 => 0x79, // PageDown
        0x23 => 0x77, // End
        0x24 => 0x73, // Home
        0x25 => 0x7b, // Left
        0x26 => 0x7e, // Up
        0x27 => 0x7c, // Right
        0x28 => 0x7d, // Down
        0x2d => 0x72, // Insert → Help
        0x2e => 0x75, // Delete (forward)
        0x6a => 0x43, // NumPad *
        0x6b => 0x45, // NumPad +
        0x6d => 0x4e, // NumPad -
        0x6e => 0x41, // NumPad .
        0x6f => 0x4b, // NumPad /
        0x90 => 0x47, // NumLock → Clear
        0xba => 0x29, // ;
        0xbb => 0x18, // =
        0xbc => 0x2b, // ,
        0xbd => 0x1b, // -
        0xbe => 0x2f, // .
        0xbf => 0x2c, // /
        0xc0 => 0x32, // `
        0xdb => 0x21, // [
        0xdc => 0x2a, // \
        0xdd => 0x1e, // ]
        0xde => 0x27, // '
        _ => return None,
    })
}

/// Release mouse buttons and keys still held when a host session ends.
pub fn release_all(inj: &InputInjector) {
    inj.release_all();
}

/// Apply one input command to the screen.
///
/// Printable keys (ch ≥ 0x20, Command/Control not held): type the Unicode character
/// on the DOWN event (skip UP) — matches the controller layout and IME. Everything else
/// (special keys, modifiers, shortcuts): press on DOWN, release on UP via keycode.
pub fn apply_input(inj: &InputInjector, cmd: &InputCmd) -> Result<(), String> {
    match cmd {
        InputCmd::Move { x, y } => inj.mouse_move(*x, *y),
        // Same as X11: click at the current pointer (button lParam is in the scaled view).
        InputCmd::Btn { button, down, .. } => inj.mouse_btn(*button, *down),
        InputCmd::Wheel { delta } => inj.mouse_wheel(*delta),
        InputCmd::Key { kind, vk, ch } => {
            let down = *kind == 1 || *kind == 3;
            if *ch >= 0x20 && *ch != 0x7f && !inj.shortcut_held() && modifier_mac(*vk).is_none() {
                if down {
                    if let Some(c) = char::from_u32(*ch as u32) {
                        inj.type_char(c)?;
                    }
                }
                Ok(())
            } else {
                inj.key_vk(*vk, down)
            }
        }
        InputCmd::Other => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vk_maps_to_mac_keycode() {
        assert_eq!(vk_to_keycode(b'A'), Some(0x00));
        assert_eq!(vk_to_keycode(b'C'), Some(0x08));
        assert_eq!(vk_to_keycode(b'V'), Some(0x09));
        assert_eq!(vk_to_keycode(b'Z'), Some(0x06));
        assert_eq!(vk_to_keycode(b'0'), Some(0x1d));
        assert_eq!(vk_to_keycode(b'5'), Some(0x17));
        assert_eq!(vk_to_keycode(0x0d), Some(0x24));
        assert_eq!(vk_to_keycode(0x70), Some(0x7a)); // F1
        assert_eq!(vk_to_keycode(0x7b), Some(0x6f)); // F12
        assert_eq!(vk_to_keycode(0xff), None);
    }

    #[test]
    fn win_ctrl_maps_to_command() {
        // Left Ctrl (162) → Command; Win → Control; Shift unchanged.
        let (kc, bit) = modifier_mac(162).unwrap();
        assert_eq!(kc, 0x37);
        assert_eq!(mods_from(bit), FLAG_CMD);
        assert_eq!(mods_from(modifier_mac(0x5b).unwrap().1), FLAG_CTRL);
        assert_eq!(mods_from(modifier_mac(0x10).unwrap().1 | modifier_mac(0xa4).unwrap().1), FLAG_SHIFT | FLAG_OPT);
        assert_eq!(mods_from(0), 0);
    }
}
