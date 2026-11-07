//! Crypto: 3DES for the control channel, AES-256-CBC for the session channel.
//!
//! * fixed 3DES key, derived from sha1 of a UTF-16LE salt;
//! * AES key derived from the session password via MD5;
//! * both apply PKCS#7 manually and disable the library padding.

use aes::cipher::{block_padding::NoPadding, BlockDecryptMut, BlockEncryptMut, KeyIvInit};
use md5::{Digest, Md5};
use rand::RngCore;

type Aes256CbcEnc = cbc::Encryptor<aes::Aes256>;
type Aes256CbcDec = cbc::Decryptor<aes::Aes256>;
type TdesCbcEnc = cbc::Encryptor<des::TdesEde3>;

/// `sha1(utf16le("\u{5}\u{10}\u{15}\u{20}\u{25}")) ++ 4 zero bytes`.
const TDES_KEY: [u8; 24] = [
    0x40, 0x9a, 0x19, 0x4e, 0xaa, 0x8b, 0x6a, 0x12, 0xb9, 0x5a, 0x60, 0x48, 0x6d, 0x47, 0xc6, 0x50,
    0xaa, 0x81, 0xbc, 0x89, 0x00, 0x00, 0x00, 0x00,
];
/// `sha1("")[..8]`.
const TDES_IV: [u8; 8] = [0xda, 0x39, 0xa3, 0xee, 0x5e, 0x6b, 0x4b, 0x0d];

/// Session AES-256 key derived from the password.
///
/// `md5` fills bytes 0..16, then is copied again into 15..31 (byte 15 is overwritten,
/// matching the original), last byte = 0.
pub fn aes_key_from_password(password: &str) -> [u8; 32] {
    let digest = Md5::digest(password.as_bytes());
    let mut key = [0u8; 32];
    key[0..16].copy_from_slice(&digest);
    key[15..31].copy_from_slice(&digest);
    key
}

fn pkcs7(data: &[u8], block: usize) -> Vec<u8> {
    let n = block - data.len() % block;
    let mut out = Vec::with_capacity(data.len() + n);
    out.extend_from_slice(data);
    out.extend(std::iter::repeat(n as u8).take(n));
    out
}

/// 3DES-CBC encrypt (control channel), block-8 padding included.
pub fn tdes_encrypt(plain: &[u8]) -> Vec<u8> {
    let padded = pkcs7(plain, 8);
    TdesCbcEnc::new(&TDES_KEY.into(), &TDES_IV.into())
        .encrypt_padded_vec_mut::<NoPadding>(&padded)
}

/// AES-256-CBC encrypt (session channel): returns `IV || ciphertext`.
pub fn aes_encrypt(password: &str, plain: &[u8]) -> Vec<u8> {
    let mut iv = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut iv);
    let padded = pkcs7(plain, 16);
    let ct = Aes256CbcEnc::new(&aes_key_from_password(password).into(), &iv.into())
        .encrypt_padded_vec_mut::<NoPadding>(&padded);
    let mut out = Vec::with_capacity(16 + ct.len());
    out.extend_from_slice(&iv);
    out.extend_from_slice(&ct);
    out
}

/// AES-256-CBC decrypt: input is `IV || ciphertext`, strips PKCS#7 padding.
pub fn aes_decrypt(password: &str, data: &[u8]) -> Vec<u8> {
    if data.len() < 32 || (data.len() - 16) % 16 != 0 {
        return Vec::new();
    }
    let (iv, ct) = data.split_at(16);
    let mut out = match Aes256CbcDec::new(&aes_key_from_password(password).into(), iv.into())
        .decrypt_padded_vec_mut::<NoPadding>(ct)
    {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    if let Some(&n) = out.last() {
        if (1..=16).contains(&n) && out.len() >= n as usize {
            out.truncate(out.len() - n as usize);
        }
    }
    out
}

/// Control-channel frame: `[u32 len][u16 = 1][3DES]`.
pub fn frame_tdes(body: &[u8]) -> Vec<u8> {
    let ct = tdes_encrypt(body);
    let mut out = Vec::with_capacity(6 + ct.len());
    out.extend_from_slice(&((ct.len() + 2) as u32).to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&ct);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aes_key_matches_original() {
        // md5("11111") = b0baee9d279d34fa1dfd71aadb908c3f
        let d = hex("b0baee9d279d34fa1dfd71aadb908c3f");
        let k = aes_key_from_password("11111");
        assert_eq!(&k[0..15], &d[0..15]);
        // byte 15 is overwritten by md5[0] — matches the original
        assert_eq!(k[15], d[0]);
        assert_eq!(&k[16..31], &d[1..16]);
        assert_eq!(k[31], 0);
    }

    #[test]
    fn aes_roundtrip() {
        for pw in ["11111", "60470", "Abc123xyz!@#"] {
            for len in [0usize, 1, 15, 16, 17, 100] {
                let plain = vec![7u8; len];
                let ct = aes_encrypt(pw, &plain);
                assert_eq!(ct.len() % 16, 0);
                assert_eq!(aes_decrypt(pw, &ct), plain, "pw={pw} len={len}");
            }
        }
    }

    #[test]
    fn aes_wrong_password_differs() {
        let ct = aes_encrypt("11111", b"hello");
        assert_ne!(aes_decrypt("22222", &ct), b"hello".to_vec());
    }

    #[test]
    fn tdes_frame_format() {
        let f = frame_tdes(b"abc");
        let n = u32::from_le_bytes([f[0], f[1], f[2], f[3]]) as usize;
        assert_eq!(n, f.len() - 4);
        assert_eq!(u16::from_le_bytes([f[4], f[5]]), 1);
        assert_eq!((f.len() - 6) % 8, 0, "ciphertext must be a multiple of 8");
    }

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }
}
