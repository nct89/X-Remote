//! uvengine — crypto + protocol framing for the control/session channels.
//!
//! Two cipher layers:
//!   - control channel (login / id-lookup / handshake): 3DES-CBC, constant key + IV.
//!   - session channel (screen / keys): AES-256-CBC, key = MD5(password) overlapped by 1 byte, IV=0.

use aes::cipher::{block_padding::Pkcs7, BlockDecryptMut, BlockEncryptMut, KeyIvInit};
use aes::Aes256;
use cbc::{Decryptor, Encryptor};
use des::TdesEde3;
use digest::Digest;
use md5::Md5;
use sha1::Sha1;

pub mod msg;
pub mod net;
pub mod session;

type Aes256CbcEnc = Encryptor<Aes256>;
type Aes256CbcDec = Decryptor<Aes256>;
type TdesCbcEnc = Encryptor<TdesEde3>;
type TdesCbcDec = Decryptor<TdesEde3>;

/// 3DES key + IV for the control channel (constant).
pub fn tdes_key_iv() -> ([u8; 24], [u8; 8]) {
    // salt = UTF-16LE of {5,10,15,20,25}
    let salt: Vec<u8> = [5u16, 10, 15, 20, 25]
        .iter()
        .flat_map(|x| x.to_le_bytes())
        .collect();
    let sha: [u8; 20] = Sha1::digest(&salt).into();
    let mut key = [0u8; 24];
    key[..20].copy_from_slice(&sha); // + 4 zero bytes

    let iv_sha: [u8; 20] = Sha1::digest([]).into();
    let mut iv = [0u8; 8];
    iv.copy_from_slice(&iv_sha[..8]); // first 8 bytes of SHA1("")
    (key, iv)
}

/// 3DES-CBC encrypt (control channel).
pub fn tdes_encrypt(plain: &[u8]) -> Vec<u8> {
    let (key, iv) = tdes_key_iv();
    TdesCbcEnc::new(&key.into(), &iv.into()).encrypt_padded_vec_mut::<Pkcs7>(plain)
}

/// 3DES-CBC decrypt.
pub fn tdes_decrypt(cipher: &[u8]) -> Vec<u8> {
    let (key, iv) = tdes_key_iv();
    TdesCbcDec::new(&key.into(), &iv.into())
        .decrypt_padded_vec_mut::<Pkcs7>(cipher)
        .expect("3DES decrypt")
}

/// AES-256 key from a password:
/// key[0..16]=md5, key[15..31]=md5 (1-byte overlap), key[31]=0.
pub fn aes_key_from_password(password: &str) -> [u8; 32] {
    let md5: [u8; 16] = Md5::digest(password.as_bytes()).into();
    let mut key = [0u8; 32];
    key[..16].copy_from_slice(&md5);
    key[15..31].copy_from_slice(&md5);
    key
}

/// AES-256-CBC encrypt (session channel), IV = 0.
pub fn aes_encrypt(password: &str, plain: &[u8]) -> Vec<u8> {
    let key = aes_key_from_password(password);
    Aes256CbcEnc::new(&key.into(), &[0u8; 16].into()).encrypt_padded_vec_mut::<Pkcs7>(plain)
}

/// AES-256-CBC decrypt.
pub fn aes_decrypt(password: &str, cipher: &[u8]) -> Vec<u8> {
    let key = aes_key_from_password(password);
    Aes256CbcDec::new(&key.into(), &[0u8; 16].into())
        .decrypt_padded_vec_mut::<Pkcs7>(cipher)
        .expect("AES decrypt")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tdes_key_is_usable() {
        let (key, iv) = tdes_key_iv();
        assert_eq!(key.len(), 24);
        assert_eq!(iv.len(), 8);
        assert_ne!(key, [0u8; 24]);
    }

    #[test]
    fn aes_key_is_stable() {
        // aes_key_from_password("11111111")
        assert_eq!(
            hex(&aes_key_from_password("11111111")),
            "1bbd886460827015e5d605ed4425221bbd886460827015e5d605ed4425225100"
        );
    }

    #[test]
    fn roundtrip() {
        let p = b"hello";
        assert_eq!(tdes_decrypt(&tdes_encrypt(p)), p);
        assert_eq!(aes_decrypt("testpw", &aes_encrypt("testpw", p)), p);
    }

    fn hex(b: &[u8]) -> String {
        b.iter().map(|x| format!("{x:02x}")).collect()
    }
}
