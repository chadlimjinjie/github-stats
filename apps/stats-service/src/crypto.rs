//! AES-256-GCM sealing for Spotify refresh tokens at rest and the OAuth `state`
//! (authenticated encryption doubles as the state's signature).
//! Format: base64url(nonce || ciphertext || tag). Key: `STATS_ENCRYPTION_KEY`, 64 hex chars.

use aes_gcm::aead::{Aead, AeadCore, KeyInit, OsRng};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
use base64::Engine;

const NONCE_LEN: usize = 12;

/// `None` when the key is missing or invalid: the service still starts, only Spotify linking fails.
#[derive(Clone)]
pub struct Sealer(Option<Aes256Gcm>);

impl Sealer {
    pub fn new(key_hex: &str) -> Self {
        let key: Option<Vec<u8>> = (key_hex.len() == 64 && key_hex.is_ascii())
            .then(|| (0..64).step_by(2).map(|i| u8::from_str_radix(&key_hex[i..i + 2], 16).ok()).collect())
            .flatten();
        if key.is_none() {
            tracing::warn!("STATS_ENCRYPTION_KEY must be 64 hex chars; Spotify linking is disabled");
        }
        Self(key.and_then(|k| Aes256Gcm::new_from_slice(&k).ok()))
    }

    fn cipher(&self) -> Result<&Aes256Gcm, String> {
        self.0.as_ref().ok_or_else(|| "encryption key not configured".to_string())
    }

    pub fn seal(&self, plaintext: &str) -> Result<String, String> {
        let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
        let mut out = nonce.to_vec();
        out.extend(self.cipher()?.encrypt(&nonce, plaintext.as_bytes()).map_err(|e| e.to_string())?);
        Ok(B64.encode(out))
    }

    pub fn open(&self, sealed: &str) -> Result<String, String> {
        let bytes = B64.decode(sealed).map_err(|e| e.to_string())?;
        if bytes.len() < NONCE_LEN {
            return Err("sealed value too short".into());
        }
        let (nonce, ct) = bytes.split_at(NONCE_LEN);
        let plain = self.cipher()?.decrypt(Nonce::from_slice(nonce), ct).map_err(|_| "tampered or wrong key")?;
        String::from_utf8(plain).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    #[test]
    fn round_trip_and_tamper() {
        let s = Sealer::new(KEY);
        let sealed = s.seal("refresh-token").unwrap();
        assert_eq!(s.open(&sealed).unwrap(), "refresh-token");
        let mut bad = sealed.into_bytes();
        let last = bad.len() - 1;
        bad[last] = if bad[last] == b'A' { b'B' } else { b'A' };
        assert!(s.open(std::str::from_utf8(&bad).unwrap()).is_err());
    }

    #[test]
    fn missing_key_fails_closed() {
        let s = Sealer::new("short");
        assert!(s.seal("x").is_err());
    }
}
