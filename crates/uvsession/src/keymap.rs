//! Windows virtual-key table, generated from `tools/uv_native.py`.
//!
//! Each key: virtual-key (VK), scan code, unshifted character, and Shift character.

/// One key on a US keyboard.
pub struct KeyDef {
    pub vk: u8,
    pub scan: u16,
    pub plain: char,
    pub shifted: char,
}

/// Keys that produce a character (letters, digits, punctuation).
pub static KEYS: &[KeyDef] = &[
    KeyDef { vk: 0x41, scan: 0x1E, plain: 'a', shifted: 'A' },
    KeyDef { vk: 0x42, scan: 0x30, plain: 'b', shifted: 'B' },
    KeyDef { vk: 0x43, scan: 0x2E, plain: 'c', shifted: 'C' },
    KeyDef { vk: 0x44, scan: 0x20, plain: 'd', shifted: 'D' },
    KeyDef { vk: 0x45, scan: 0x12, plain: 'e', shifted: 'E' },
    KeyDef { vk: 0x46, scan: 0x21, plain: 'f', shifted: 'F' },
    KeyDef { vk: 0x47, scan: 0x22, plain: 'g', shifted: 'G' },
    KeyDef { vk: 0x48, scan: 0x23, plain: 'h', shifted: 'H' },
    KeyDef { vk: 0x49, scan: 0x17, plain: 'i', shifted: 'I' },
    KeyDef { vk: 0x4A, scan: 0x24, plain: 'j', shifted: 'J' },
    KeyDef { vk: 0x4B, scan: 0x25, plain: 'k', shifted: 'K' },
    KeyDef { vk: 0x4C, scan: 0x26, plain: 'l', shifted: 'L' },
    KeyDef { vk: 0x4D, scan: 0x32, plain: 'm', shifted: 'M' },
    KeyDef { vk: 0x4E, scan: 0x31, plain: 'n', shifted: 'N' },
    KeyDef { vk: 0x4F, scan: 0x18, plain: 'o', shifted: 'O' },
    KeyDef { vk: 0x50, scan: 0x19, plain: 'p', shifted: 'P' },
    KeyDef { vk: 0x51, scan: 0x10, plain: 'q', shifted: 'Q' },
    KeyDef { vk: 0x52, scan: 0x13, plain: 'r', shifted: 'R' },
    KeyDef { vk: 0x53, scan: 0x1F, plain: 's', shifted: 'S' },
    KeyDef { vk: 0x54, scan: 0x14, plain: 't', shifted: 'T' },
    KeyDef { vk: 0x55, scan: 0x16, plain: 'u', shifted: 'U' },
    KeyDef { vk: 0x56, scan: 0x2F, plain: 'v', shifted: 'V' },
    KeyDef { vk: 0x57, scan: 0x11, plain: 'w', shifted: 'W' },
    KeyDef { vk: 0x58, scan: 0x2D, plain: 'x', shifted: 'X' },
    KeyDef { vk: 0x59, scan: 0x15, plain: 'y', shifted: 'Y' },
    KeyDef { vk: 0x5A, scan: 0x2C, plain: 'z', shifted: 'Z' },
    KeyDef { vk: 0x30, scan: 0x0B, plain: '0', shifted: ')' },
    KeyDef { vk: 0x31, scan: 0x02, plain: '1', shifted: '!' },
    KeyDef { vk: 0x32, scan: 0x03, plain: '2', shifted: '@' },
    KeyDef { vk: 0x33, scan: 0x04, plain: '3', shifted: '#' },
    KeyDef { vk: 0x34, scan: 0x05, plain: '4', shifted: '$' },
    KeyDef { vk: 0x35, scan: 0x06, plain: '5', shifted: '%' },
    KeyDef { vk: 0x36, scan: 0x07, plain: '6', shifted: '^' },
    KeyDef { vk: 0x37, scan: 0x08, plain: '7', shifted: '&' },
    KeyDef { vk: 0x38, scan: 0x09, plain: '8', shifted: '*' },
    KeyDef { vk: 0x39, scan: 0x0A, plain: '9', shifted: '(' },
    KeyDef { vk: 0x20, scan: 0x39, plain: ' ', shifted: ' ' },
    KeyDef { vk: 0x0D, scan: 0x1C, plain: '\u{d}', shifted: '\u{d}' },
    KeyDef { vk: 0x09, scan: 0x0F, plain: '\u{9}', shifted: '\u{9}' },
    KeyDef { vk: 0x08, scan: 0x0E, plain: '\u{8}', shifted: '\u{8}' },
    KeyDef { vk: 0x1B, scan: 0x01, plain: '\u{1b}', shifted: '\u{1b}' },
    KeyDef { vk: 0xBD, scan: 0x0C, plain: '-', shifted: '_' },
    KeyDef { vk: 0xBB, scan: 0x0D, plain: '=', shifted: '+' },
    KeyDef { vk: 0xDB, scan: 0x1A, plain: '[', shifted: '{' },
    KeyDef { vk: 0xDD, scan: 0x1B, plain: ']', shifted: '}' },
    KeyDef { vk: 0xDC, scan: 0x2B, plain: '\\', shifted: '|' },
    KeyDef { vk: 0xBA, scan: 0x27, plain: ';', shifted: ':' },
    KeyDef { vk: 0xDE, scan: 0x28, plain: '\'', shifted: '"' },
    KeyDef { vk: 0xC0, scan: 0x29, plain: '`', shifted: '~' },
    KeyDef { vk: 0xBC, scan: 0x33, plain: ',', shifted: '<' },
    KeyDef { vk: 0xBE, scan: 0x34, plain: '.', shifted: '>' },
    KeyDef { vk: 0xBF, scan: 0x35, plain: '/', shifted: '?' },
];

/// Special keys: name → (VK, scan).
pub static SPECIAL: &[(&str, u8, u16)] = &[
    ("enter", 0x0D, 0x1C),
    ("tab", 0x09, 0x0F),
    ("backspace", 0x08, 0x0E),
    ("escape", 0x1B, 0x01),
    ("space", 0x20, 0x39),
    ("shift", 0x10, 0x2A),
    ("shift_r", 0x10, 0x36),
    ("ctrl", 0x11, 0x1D),
    ("ctrl_r", 0x11, 0x1D),
    ("alt", 0x12, 0x38),
    ("alt_r", 0x12, 0x38),
    ("win", 0x5B, 0x5B),
    ("win_r", 0x5C, 0x5C),
    ("caps", 0x14, 0x3A),
    ("num", 0x90, 0x45),
    ("scroll", 0x91, 0x46),
    ("insert", 0x2D, 0x52),
    ("delete", 0x2E, 0x53),
    ("home", 0x24, 0x47),
    ("end", 0x23, 0x4F),
    ("pageup", 0x21, 0x49),
    ("pagedown", 0x22, 0x51),
    ("left", 0x25, 0x4B),
    ("right", 0x27, 0x4D),
    ("up", 0x26, 0x48),
    ("down", 0x28, 0x50),
    ("f1", 0x70, 0x3B),
    ("f2", 0x71, 0x3C),
    ("f3", 0x72, 0x3D),
    ("f4", 0x73, 0x3E),
    ("f5", 0x74, 0x3F),
    ("f6", 0x75, 0x40),
    ("f7", 0x76, 0x41),
    ("f8", 0x77, 0x42),
    ("f9", 0x78, 0x43),
    ("f10", 0x79, 0x44),
    ("f11", 0x7A, 0x57),
    ("f12", 0x7B, 0x58),
];


/// Virtual-key codes of system keys (sent as kind 3/4 instead of 1/2).
const SYSTEM_VKS: &[u8] = &[
    0x09, 0x10, 0x11, 0x12, 0x14, 0x1B, 0x20, 0x25, 0x26, 0x27, 0x28, 0x5B, 0x5C, 0x5D, 0x90,
    0x91, 0x2D, 0x2E, 0x24, 0x23, 0x21, 0x22,
];

/// Key for a character, plus whether Shift is required.
///
/// For example `'A'` → key `a` with `shift = true`; `'!'` → key `1` with `shift = true`.
pub fn lookup(ch: char) -> Option<(&'static KeyDef, bool)> {
    if let Some(k) = KEYS.iter().find(|k| k.plain == ch) {
        return Some((k, false));
    }
    KEYS.iter().find(|k| k.shifted == ch).map(|k| (k, true))
}

/// Special key by name (`enter`, `tab`, `f5`, `left`, …).
pub fn special(name: &str) -> Option<(u8, u16)> {
    SPECIAL
        .iter()
        .find(|(n, _, _)| *n == name)
        .map(|(_, vk, scan)| (*vk, *scan))
}

/// Scan code for a virtual-key code (0 when it is not in the table).
pub fn scan_of(vk: u8) -> u16 {
    SPECIAL
        .iter()
        .find(|(_, v, _)| *v == vk)
        .map(|(_, _, scan)| *scan)
        .unwrap_or(0)
}

pub fn is_system(vk: u8) -> bool {
    SYSTEM_VKS.contains(&vk)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_key_for_lower_and_upper() {
        let (a, shift) = lookup('a').unwrap();
        assert_eq!((a.vk, a.scan, shift), (0x41, 0x1E, false));
        let (a2, shift2) = lookup('A').unwrap();
        assert_eq!((a2.vk, a2.scan, shift2), (0x41, 0x1E, true));
    }

    #[test]
    fn punctuation_needs_shift() {
        let (k, shift) = lookup('!').unwrap();
        assert_eq!((k.vk, k.scan, shift), (0x31, 0x02, true));
        assert_eq!(lookup('.').map(|(k, s)| (k.vk, k.scan, s)), Some((0xBE, 0x34, false)));
    }

    #[test]
    fn special_keys() {
        assert_eq!(special("enter"), Some((0x0D, 0x1C)));
        assert_eq!(special("f5"), Some((0x74, 0x3F)));
        assert_eq!(special("khong-co"), None);
    }

    #[test]
    fn scan_code_for_vk() {
        assert_eq!(scan_of(0x10), 0x2A); // Shift
        assert_eq!(scan_of(0x11), 0x1D); // Ctrl
        assert_eq!(scan_of(0x12), 0x38); // Alt
        assert_eq!(scan_of(0x0D), 0x1C); // Enter
        assert_eq!(scan_of(0x5B), 0x5B); // Win
    }

    #[test]
    fn system_and_plain() {
        assert!(is_system(0x1B)); // Esc
        assert!(!is_system(0x0D)); // Enter is a normal key
        assert!(!is_system(0x41));
    }
}
