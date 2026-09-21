//! End-to-end encryption for sync operation payloads.
//!
//! Payloads are encrypted with ChaCha20-Poly1305 (AEAD) using a 256-bit key.
//! The key is derived from a user-provided site encryption secret with SHA-256
//! so any string can be used as input.
//!
//! Ciphertext format: nonce (12 bytes) || ciphertext || tag (16 bytes).

use chacha20poly1305::{
    aead::{Aead, KeyInit},
    ChaCha20Poly1305, Nonce,
};
use error::{Result, RuntimeError};

const NONCE_LEN: usize = 12;

/// Derive a 32-byte encryption key from an arbitrary site secret.
pub fn derive_key(secret: &str) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(secret.as_bytes());
    hasher.finalize().into()
}

/// Encrypt a plaintext payload string. Returns bytes ready for `encrypted_payload`.
pub fn encrypt_payload(secret: &str, plaintext: &str) -> Result<Vec<u8>> {
    let key = derive_key(secret);
    let cipher = ChaCha20Poly1305::new_from_slice(&key)
        .map_err(|e| RuntimeError::Validation(format!("invalid cipher key: {e}")))?;
    let nonce_bytes: [u8; NONCE_LEN] = rand::random();
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher
        .encrypt(nonce, plaintext.as_bytes())
        .map_err(|e| RuntimeError::Validation(format!("encryption failed: {e}")))?;

    let mut out = Vec::with_capacity(NONCE_LEN + ciphertext.len());
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&ciphertext);
    Ok(out)
}

/// Decrypt an `encrypted_payload` blob back to the original plaintext string.
pub fn decrypt_payload(secret: &str, ciphertext: &[u8]) -> Result<String> {
    if ciphertext.len() < NONCE_LEN + 1 {
        return Err(RuntimeError::Validation(
            "ciphertext too short to contain nonce".into(),
        ));
    }
    let key = derive_key(secret);
    let cipher = ChaCha20Poly1305::new_from_slice(&key)
        .map_err(|e| RuntimeError::Validation(format!("invalid cipher key: {e}")))?;
    let nonce = Nonce::from_slice(&ciphertext[..NONCE_LEN]);
    let plaintext = cipher
        .decrypt(nonce, &ciphertext[NONCE_LEN..])
        .map_err(|e| RuntimeError::Validation(format!("decryption failed: {e}")))?;
    String::from_utf8(plaintext)
        .map_err(|e| RuntimeError::Validation(format!("invalid utf-8 after decrypt: {e}")))
}

/// Generate a random 256-bit site encryption secret as a hex string.
pub fn generate_secret() -> String {
    let bytes: [u8; 32] = rand::random();
    hex::encode(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_payload() {
        let secret = generate_secret();
        let plaintext = r#"{"full_name":"Alice","email":"alice@example.com"}"#;
        let encrypted = encrypt_payload(&secret, plaintext).unwrap();
        assert_ne!(encrypted, plaintext.as_bytes());
        let decrypted = decrypt_payload(&secret, &encrypted).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn wrong_secret_fails() {
        let secret = generate_secret();
        let plaintext = "sensitive data";
        let encrypted = encrypt_payload(&secret, plaintext).unwrap();
        let wrong_secret = generate_secret();
        assert!(decrypt_payload(&wrong_secret, &encrypted).is_err());
    }
}
