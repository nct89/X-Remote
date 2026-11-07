//! Byte-for-byte check against the proven Python port (vectors from `tools/uv_native.py`).

fn hex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

#[test]
fn decrypts_python_ciphertext() {
    // python: aes_encrypt("testpw1234567", b"\x01hello world")
    let ct = hex("ac64809345801e56e3cb88ecef6d8351bf4a715080f105f3586ed8134fe0d0f6");
    let pt = uvsession::aes_decrypt("testpw1234567", &ct);
    assert_eq!(pt, b"\x01hello world");
}

#[test]
fn tdes_frame_matches_python() {
    // python: frame_tdes(b"abc").hex()
    let f = uvsession::crypto::frame_tdes(b"abc");
    assert_eq!(hex("0a000000010036972903508474c1"), f);
}

#[test]
fn rust_ciphertext_round_trips() {
    // Ciphertext produced by Rust must decrypt with the same rules (both directions).
    let ct = uvsession::aes_encrypt("Abc!23xyz", b"\x00mot hai ba");
    let out = uvsession::aes_decrypt("Abc!23xyz", &ct);
    assert_eq!(out, b"\x00mot hai ba");
    assert_eq!(ct.len(), 16 + 16, "IV + 1 padded block");
}

/// Mouse: ASCII `\x008|||msg|||wparam|||lo|||hi` must match Python.
#[test]
fn mouse_frames_match_python() {
    use uvsession::input as inp;
    // python: mouse_frame(0x200, 400, 300).hex()
    assert_eq!(hex("00387c7c7c3531327c7c7c307c7c7c3430307c7c7c333030"), inp::mouse_move(400, 300));
    // python: mouse_frame(0x200, 5, 7).hex()
    assert_eq!(hex("00387c7c7c3531327c7c7c307c7c7c357c7c7c37"), inp::mouse_move(5, 7));
    // python: mouse_button_frame(513, 400, 300) — packed lParam
    assert_eq!(
        hex("00387c7c7c3531337c7c7c307c7c7c3139363631323030"),
        inp::mouse_button(inp::WM_LBUTTONDOWN, 400, 300)
    );
    assert_eq!(
        hex("00387c7c7c3531347c7c7c307c7c7c3139363631323030"),
        inp::mouse_button(inp::WM_LBUTTONUP, 400, 300)
    );
    // python: mouse_wheel_frame(-120, 400, 300) — signed wParam (-7864320). The
    // unsigned value 4287102976 makes UltraViewer 6.6.124 overflow and stall input (2026-11-04).
    assert_eq!(
        hex("00387c7c7c3532327c7c7c2d373836343332307c7c7c3139363631323030"),
        inp::mouse_wheel(-120, 400, 300)
    );
}

/// Binary 11-byte keyboard (without the 0x01 flag) must match Python.
#[test]
fn key_frames_match_python() {
    use uvsession::input as inp;

    // python: key_events("a") → down then up
    let a = inp::char_events('a').expect("must have a code for 'a'");
    assert_eq!(hex("1e7c7c7c01411e00610000"), a[0]);
    assert_eq!(hex("1e7c7c7c02411e00610000"), a[1]);

    // 'Z' sends the upper code, caps flag = 1
    let z = inp::char_events('Z').expect("must have a code for 'Z'");
    assert_eq!(hex("1e7c7c7c015a2c005a0000"), z[0]);
    assert_eq!(hex("1e7c7c7c025a2c005a0000"), z[1]);

    // '1' is a normal key, kind 1/2
    let one = inp::char_events('1').expect("must have a code for '1'");
    assert_eq!(hex("1e7c7c7c01310200310000"), one[0]);
    assert_eq!(hex("1e7c7c7c02310200310000"), one[1]);
}

/// System keys (Esc) use kind 3/4 instead of 1/2.
#[test]
fn system_key_uses_kind_3_4() {
    use uvsession::input as inp;
    // python: key_payload(3, 0x1B, 0x01) → 1e7c7c7c031b0100000000
    let esc_down = inp::special_events("escape", true).expect("must have Esc");
    assert_eq!(hex("1e7c7c7c031b0100000000"), esc_down);
    let esc_up = inp::special_events("escape", false).expect("must have Esc");
    assert_eq!(hex("1e7c7c7c041b0100000000"), esc_up);
}
