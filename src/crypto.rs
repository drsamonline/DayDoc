//! AES-256-GCM for encrypted backups.
//!
//! One file format: `DAYDOC1\n`, a 12-byte nonce, then ciphertext with its tag.
//! A wrong key and a tampered file fail the same way — closed.

use ring::aead;
use ring::rand::{SecureRandom, SystemRandom};

use crate::error::{DayDocError, Result};

pub const MAGIC: &[u8] = b"DAYDOC1\n";
pub const KEY_LEN: usize = 32;
pub const NONCE_LEN: usize = 12;

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

fn unhex(text: &str) -> Option<Vec<u8>> {
    let trimmed = text.trim();
    if trimmed.len() % 2 != 0 {
        return None;
    }
    let mut out = Vec::with_capacity(trimmed.len() / 2);
    let bytes = trimmed.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        let pair = std::str::from_utf8(&bytes[index..index + 2]).ok()?;
        out.push(u8::from_str_radix(pair, 16).ok()?);
        index += 2;
    }
    Some(out)
}

/// A fresh 32-byte key, hex encoded for storage.
pub fn new_key_hex() -> Result<String> {
    let rng = SystemRandom::new();
    let mut key = [0u8; KEY_LEN];
    rng.fill(&mut key)
        .map_err(|_| DayDocError::Config("no system randomness available".to_string()))?;
    Ok(hex(&key))
}

/// Parse a stored key: hex first, then raw bytes for a 32-character secret.
pub fn key_from_text(text: &str) -> Result<[u8; KEY_LEN]> {
    let trimmed = text.trim();
    let bytes = if trimmed.len() == KEY_LEN * 2 {
        unhex(trimmed).ok_or_else(|| {
            DayDocError::Config("the backup key is not valid hex".to_string())
        })?
    } else {
        trimmed.as_bytes().to_vec()
    };
    if bytes.len() != KEY_LEN {
        return Err(DayDocError::Config(format!(
            "the backup key must be {} bytes ({} hex characters)",
            KEY_LEN,
            KEY_LEN * 2
        )));
    }
    let mut key = [0u8; KEY_LEN];
    key.copy_from_slice(&bytes);
    Ok(key)
}

fn less_safe_key(key: &[u8; KEY_LEN]) -> Result<aead::LessSafeKey> {
    let unbound = aead::UnboundKey::new(&aead::AES_256_GCM, key)
        .map_err(|_| DayDocError::Config("invalid AES-256-GCM key".to_string()))?;
    Ok(aead::LessSafeKey::new(unbound))
}

/// Encrypt: magic, nonce, ciphertext+tag.
pub fn encrypt(key: &[u8; KEY_LEN], plaintext: &[u8]) -> Result<Vec<u8>> {
    let sealing = less_safe_key(key)?;
    let rng = SystemRandom::new();
    let mut nonce_bytes = [0u8; NONCE_LEN];
    rng.fill(&mut nonce_bytes)
        .map_err(|_| DayDocError::Config("no system randomness available".to_string()))?;

    let mut in_out = plaintext.to_vec();
    sealing
        .seal_in_place_append_tag(
            aead::Nonce::assume_unique_for_key(nonce_bytes),
            aead::Aad::empty(),
            &mut in_out,
        )
        .map_err(|_| DayDocError::Config("encryption failed".to_string()))?;

    let mut out = Vec::with_capacity(MAGIC.len() + NONCE_LEN + in_out.len());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&in_out);
    Ok(out)
}

/// True if the payload looks encrypted by us.
pub fn is_encrypted(payload: &[u8]) -> bool {
    payload.len() > MAGIC.len() + NONCE_LEN && payload.starts_with(MAGIC)
}

/// Decrypt. A wrong key, a truncated file and a tampered tag all fail here.
pub fn decrypt(key: &[u8; KEY_LEN], payload: &[u8]) -> Result<Vec<u8>> {
    if !is_encrypted(payload) {
        return Err(DayDocError::Config(
            "not an encrypted DayDoc backup".to_string(),
        ));
    }
    let opening = less_safe_key(key)?;
    let mut nonce_bytes = [0u8; NONCE_LEN];
    nonce_bytes.copy_from_slice(&payload[MAGIC.len()..MAGIC.len() + NONCE_LEN]);
    let mut body = payload[MAGIC.len() + NONCE_LEN..].to_vec();
    let plain = opening
        .open_in_place(
            aead::Nonce::assume_unique_for_key(nonce_bytes),
            aead::Aad::empty(),
            &mut body,
        )
        .map_err(|_| {
            DayDocError::Config(
                "could not decrypt: wrong key, or the file has been altered".to_string(),
            )
        })?;
    Ok(plain.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> [u8; KEY_LEN] {
        let mut key = [0u8; KEY_LEN];
        for (index, byte) in key.iter_mut().enumerate() {
            *byte = index as u8;
        }
        key
    }

    #[test]
    fn a_round_trip_returns_the_plaintext() {
        let payload = encrypt(&key(), b"the vault, compressed").expect("encrypt");
        assert!(is_encrypted(&payload));
        assert_ne!(&payload[MAGIC.len()..], b"the vault, compressed");
        let plain = decrypt(&key(), &payload).expect("decrypt");
        assert_eq!(plain, b"the vault, compressed");
    }

    #[test]
    fn a_wrong_key_fails_closed() {
        let payload = encrypt(&key(), b"secret").expect("encrypt");
        let mut other = key();
        other[0] ^= 0xff;
        assert!(decrypt(&other, &payload).is_err());
    }

    #[test]
    fn a_tampered_file_fails_closed() {
        let mut payload = encrypt(&key(), b"secret").expect("encrypt");
        let last = payload.len() - 1;
        payload[last] ^= 0x01;
        assert!(decrypt(&key(), &payload).is_err());
    }

    #[test]
    fn plaintext_is_not_mistaken_for_ciphertext() {
        assert!(!is_encrypted(b"not encrypted at all"));
        assert!(decrypt(&key(), b"plain").is_err());
    }

    #[test]
    fn keys_are_generated_parse_and_validate() {
        let generated = new_key_hex().expect("key");
        assert_eq!(generated.len(), KEY_LEN * 2);
        let parsed = key_from_text(&generated).expect("parse");
        assert_eq!(parsed.len(), KEY_LEN);
        assert!(key_from_text("too-short").is_err());
        assert!(key_from_text(&"z".repeat(KEY_LEN * 2)).is_err());
    }

    #[test]
    fn every_encryption_uses_a_fresh_nonce() {
        let first = encrypt(&key(), b"same").expect("first");
        let second = encrypt(&key(), b"same").expect("second");
        assert_ne!(first, second);
    }
}
