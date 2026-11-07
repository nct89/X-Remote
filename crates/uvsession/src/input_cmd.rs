//! Decode input messages from an UltraViewer controller (shared by the Linux and macOS hosts):
//! * mouse — text `8|||<msg>|||<wparam>|||<lparam>` or `8|||<msg>|||<wparam>|||x|||y`;
//! * keys  — 11 bytes `1e 7c 7c 7c kind vk scan_lo scan_hi ch_lo ch_hi caps`, or text
//!   `8|||256/257/260/261|||<vk>|||-1|||-1`.
//!
//! The OS injector ([`crate::inject`]) receives the decoded [`InputCmd`].

/// Input command decoded from a controller session message.
#[derive(Debug, PartialEq)]
pub enum InputCmd {
    Move { x: i32, y: i32 },
    Btn { button: u8, down: bool, x: i32, y: i32 },
    Wheel { delta: i32 },
    /// 11-byte key: (kind, vk, UTF-16 char, caps).
    Key { kind: u8, vk: u8, ch: u16 },
    /// Not an input command (frame, keepalive, …).
    Other,
}

/// Decode one session message (leading flag byte already removed) into an input command.
pub fn parse_cmd(body: &[u8]) -> InputCmd {
    // Binary key: 1e 7c 7c 7c …
    if body.len() >= 11 && body[0] == 0x1e && body[1] == 0x7c && body[2] == 0x7c && body[3] == 0x7c {
        let kind = body[4];
        let vk = body[5];
        let ch = u16::from_le_bytes([body[8], body[9]]);
        return InputCmd::Key { kind, vk, ch };
    }
    // Mouse text: 8|||<msg>|||<wparam>|||<lparam>
    if let Ok(s) = std::str::from_utf8(body) {
        let p: Vec<&str> = s.split("|||").collect();
        if p.first() == Some(&"8") && p.len() >= 3 {
            let msg: u32 = p[1].parse().unwrap_or(0);
            use crate::input::*;
            // Position: 5-field form `…|||x|||y` (real UltraViewer) or a packed lParam.
            let coords = || -> (i32, i32) {
                if p.len() >= 5 {
                    (p[3].parse().unwrap_or(0), p[4].parse().unwrap_or(0))
                } else {
                    unpack_lparam(p.get(3))
                }
            };
            return match msg {
                // Text key: `8|||256|||<vk>|||-1|||-1` (up is 257, Alt+… is 260/261).
                0x100 | 0x101 | 0x104 | 0x105 => {
                    let vk: u32 = p[2].parse().unwrap_or(0);
                    if vk == 0 || vk > 0xff {
                        return InputCmd::Other;
                    }
                    let kind = match msg {
                        0x100 => 1,
                        0x101 => 2,
                        0x104 => 3,
                        _ => 4,
                    };
                    InputCmd::Key { kind, vk: vk as u8, ch: 0 }
                }
                WM_MOUSEMOVE => {
                    // Two forms: "…|0|x|y" (5 fields) or "…|0|<packed lparam>" (4 fields).
                    if p.len() >= 5 {
                        InputCmd::Move { x: p[3].parse().unwrap_or(0), y: p[4].parse().unwrap_or(0) }
                    } else {
                        let (x, y) = unpack_lparam(p.get(3));
                        InputCmd::Move { x, y }
                    }
                }
                WM_LBUTTONDOWN | WM_LBUTTONUP | WM_RBUTTONDOWN | WM_RBUTTONUP
                | WM_MBUTTONDOWN | WM_MBUTTONUP | WM_LBUTTONDBLCLK | WM_RBUTTONDBLCLK
                | WM_MBUTTONDBLCLK => {
                    let (x, y) = coords();
                    let (button, down) = match msg {
                        // Windows double-click: DOWN, UP, DBLCLK, UP — DBLCLK is the second press.
                        WM_LBUTTONDOWN | WM_LBUTTONDBLCLK => (1, true),
                        WM_RBUTTONDBLCLK => (3, true),
                        WM_MBUTTONDBLCLK => (2, true),
                        WM_LBUTTONUP => (1, false),
                        WM_RBUTTONDOWN => (3, true),
                        WM_RBUTTONUP => (3, false),
                        WM_MBUTTONDOWN => (2, true),
                        _ => (2, false),
                    };
                    InputCmd::Btn { button, down, x, y }
                }
                WM_MOUSEWHEEL => {
                    let wparam: i32 = p.get(2).and_then(|v| v.parse().ok()).unwrap_or(0);
                    InputCmd::Wheel { delta: wparam >> 16 }
                }
                _ => InputCmd::Other,
            };
        }
    }
    InputCmd::Other
}

/// Unpack a packed lParam (LOWORD = x, HIWORD = y) from one numeric field.
fn unpack_lparam(field: Option<&&str>) -> (i32, i32) {
    let v: u32 = field.and_then(|s| s.parse().ok()).unwrap_or(0);
    ((v & 0xffff) as i32, ((v >> 16) & 0xffff) as i32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn move_both_forms() {
        assert_eq!(parse_cmd(b"8|||512|||0|||100|||200"), InputCmd::Move { x: 100, y: 200 });
        let lparam = 100u32 | (200u32 << 16);
        assert_eq!(parse_cmd(format!("8|||512|||0|||{lparam}").as_bytes()), InputCmd::Move { x: 100, y: 200 });
    }

    #[test]
    fn button_and_wheel() {
        let lparam = 10u32 | (20u32 << 16);
        assert_eq!(parse_cmd(format!("8|||513|||0|||{lparam}").as_bytes()), InputCmd::Btn { button: 1, down: true, x: 10, y: 20 });
        assert_eq!(parse_cmd(b"8|||517|||0|||0"), InputCmd::Btn { button: 3, down: false, x: 0, y: 0 });
        // wheel wparam = 120<<16 → delta 120.
        assert_eq!(parse_cmd(b"8|||522|||7864320|||0"), InputCmd::Wheel { delta: 120 });
        assert_eq!(parse_cmd(b"8|||522|||-7864320|||0"), InputCmd::Wheel { delta: -120 });
    }

    #[test]
    fn binary_key() {
        let p = crate::input::key_payload(crate::input::KEY_DOWN, 0x41, 0x1e, 0x61, false);
        assert_eq!(parse_cmd(&p), InputCmd::Key { kind: 1, vk: 0x41, ch: 0x61 });
    }

    #[test]
    fn five_field_button_keeps_y() {
        // 5-field form: y is field 4, not the high half of lParam.
        assert_eq!(
            parse_cmd(b"8|||513|||1|||640|||480"),
            InputCmd::Btn { button: 1, down: true, x: 640, y: 480 }
        );
        assert_eq!(
            parse_cmd(b"8|||517|||0|||1900|||1070"),
            InputCmd::Btn { button: 3, down: false, x: 1900, y: 1070 }
        );
        // Double-click: 515 is the second left press (real log `8|||515|||1|||34669129`).
        assert_eq!(
            parse_cmd(b"8|||515|||1|||34669129"),
            InputCmd::Btn { button: 1, down: true, x: 585, y: 529 }
        );
        assert_eq!(parse_cmd(b"8|||518|||2|||0"), InputCmd::Btn { button: 3, down: true, x: 0, y: 0 });
        // Packed lParam still works.
        let lp = 640u32 | (480u32 << 16);
        assert_eq!(
            parse_cmd(format!("8|||513|||1|||{lp}").as_bytes()),
            InputCmd::Btn { button: 1, down: true, x: 640, y: 480 }
        );
    }

    #[test]
    fn real_controller_text_key() {
        // Measured 2026-11-05 19:30: left Ctrl (162), left Shift (160), 'S' (83).
        assert_eq!(parse_cmd(b"8|||256|||162|||-1|||-1"), InputCmd::Key { kind: 1, vk: 162, ch: 0 });
        assert_eq!(parse_cmd(b"8|||257|||83|||-1|||-1"), InputCmd::Key { kind: 2, vk: 83, ch: 0 });
        assert_eq!(parse_cmd(b"8|||260|||115|||-1|||-1"), InputCmd::Key { kind: 3, vk: 115, ch: 0 });
    }

    #[test]
    fn non_input_frame() {
        assert_eq!(parse_cmd(b"18"), InputCmd::Other);
    }
}
