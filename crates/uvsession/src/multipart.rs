//! Multipart frame: repeated `[u32 len][part]` until the data runs out.
//!
//! Every session-channel message (after decryption) uses this shape. Part 0 is always
//! the command: 0 = screen init, 1 = frame, 49 = control permission.

/// Decode a multipart frame. Returns `None` if the data is invalid.
pub fn decode(mut payload: &[u8]) -> Option<Vec<Vec<u8>>> {
    let mut parts = Vec::new();
    while payload.len() >= 4 {
        let n = u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]) as usize;
        payload = &payload[4..];
        if n > payload.len() {
            return None;
        }
        parts.push(payload[..n].to_vec());
        payload = &payload[n..];
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts)
    }
}

/// Encode a list of parts into a multipart frame.
pub fn encode(parts: &[&[u8]]) -> Vec<u8> {
    let mut out = Vec::with_capacity(parts.iter().map(|p| p.len() + 4).sum());
    for p in parts {
        out.extend_from_slice(&(p.len() as u32).to_le_bytes());
        out.extend_from_slice(p);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let parts: Vec<&[u8]> = vec![b"\x01", b"", b"hello", &[0u8; 300][..]];
        let blob = encode(&parts);
        let back = decode(&blob).expect("must decode");
        assert_eq!(back.len(), 4);
        assert_eq!(back[0], b"\x01");
        assert!(back[1].is_empty());
        assert_eq!(back[2], b"hello");
        assert_eq!(back[3].len(), 300);
    }

    #[test]
    fn corrupt_data_does_not_panic() {
        assert!(decode(b"").is_none());
        assert!(decode(b"\xff\xff\xff\x7f").is_none(), "len exceeds data");
        assert!(decode(b"\x05\x00\x00\x00ab").is_none(), "truncated part");
        // length 0 is still valid
        let ok = decode(b"\x00\x00\x00\x00\x01\x00\x00\x00a").unwrap();
        assert_eq!(ok.len(), 2);
    }
}
