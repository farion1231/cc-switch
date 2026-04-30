//! Encryption module for sensitive data
//!
//! Provides AES-256-GCM encryption for API keys, tokens, and passwords
//! stored in the SQLite database and settings files.
//!
//! ## Design
//!
//! The encryption key is derived from machine-specific data (hostname + OS info)
//! combined with a static application salt. This provides protection against:
//! - Direct SQLite file exfiltration
//! - Accidental credential exposure in backups
//! - Casual inspection of config files
//!
//! ## Limitations
//!
//! This is NOT a substitute for OS-level keychain. An attacker with code
//! execution on the same machine can reconstruct the key. For production
//! deployments, consider using the system keyring (macOS Keychain,
//! Windows Credential Manager, Linux libsecret).

use aes_gcm::aead::{Aead, OsRng};
use aes_gcm::{Aes256Gcm, Key, KeyInit, Nonce};
use rand::RngCore;
use sha2::{Digest, Sha256};

/// 4-byte magic prefix to identify encrypted data: "CCS1"
const ENCRYPTION_MAGIC: &[u8; 4] = b"CCS1";

/// Returns true if the input starts with the encryption magic prefix.
pub fn is_encrypted(data: &[u8]) -> bool {
    data.len() >= 4 && &data[..4] == ENCRYPTION_MAGIC
}

/// Returns true if the string is base64-encoded ciphertext produced by `encrypt`.
///
/// `encrypt` base64-encodes the magic prefix together with the nonce and
/// ciphertext, so the raw UTF-8 bytes of the returned string never start with
/// the magic. Decode the base64 first, then check the magic.
pub fn is_encrypted_str(data: &str) -> bool {
    decode_base64(data)
        .map(|bytes| is_encrypted(&bytes))
        .unwrap_or(false)
}

/// Derive an AES-256 key from machine-specific context.
///
/// Uses SHA-256 to derive a 32-byte key from:
/// - Machine hostname (fallback: "unknown-host")
/// - OS family and architecture
/// - Static application salt
fn derive_key() -> [u8; 32] {
    let hostname = hostname::get()
        .ok()
        .and_then(|h| h.into_string().ok())
        .unwrap_or_else(|| "unknown-host".to_string());

    let os_info = format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH);

    // Application-specific salt - this is not a secret, it just ensures
    // the derived key is different from other applications on the same machine.
    const APP_SALT: &[u8] = b"cc-switch-encryption-v1-salt-2024";

    let mut hasher = Sha256::new();
    hasher.update(hostname.as_bytes());
    hasher.update(b":");
    hasher.update(os_info.as_bytes());
    hasher.update(b":");
    hasher.update(APP_SALT);

    let result = hasher.finalize();
    let mut key = [0u8; 32];
    key.copy_from_slice(&result);
    key
}

/// Encrypt a plaintext string and return base64-encoded ciphertext with magic prefix.
///
/// Format: `CCS1` + 12-byte nonce + ciphertext (all base64 encoded)
///
/// Returns `None` if the input is empty or encryption fails.
pub fn encrypt(plaintext: &str) -> Option<String> {
    if plaintext.is_empty() {
        return None;
    }

    let key_bytes = derive_key();
    let key = Key::<Aes256Gcm>::from_slice(&key_bytes);
    let cipher = Aes256Gcm::new(key);

    let mut nonce_bytes = [0u8; 12];
    OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, plaintext.as_bytes())
        .ok()?;

    let mut combined = Vec::with_capacity(4 + 12 + ciphertext.len());
    combined.extend_from_slice(ENCRYPTION_MAGIC);
    combined.extend_from_slice(&nonce_bytes);
    combined.extend_from_slice(&ciphertext);

    Some(encode_base64(&combined))
}

/// Decrypt a base64-encoded ciphertext that starts with the magic prefix.
///
/// Returns `None` if the input is empty, not encrypted, or decryption fails.
/// Transparently passes through non-encrypted data unchanged.
pub fn decrypt(data: &str) -> Option<String> {
    if data.is_empty() {
        return None;
    }

    let bytes = decode_base64(data)?;

    if !is_encrypted(&bytes) {
        // Not encrypted - pass through as-is (backward compatibility)
        return Some(data.to_string());
    }

    if bytes.len() < 4 + 12 + 16 {
        // Invalid encrypted data (too short to be valid)
        return None;
    }

    let key_bytes = derive_key();
    let key = Key::<Aes256Gcm>::from_slice(&key_bytes);
    let cipher = Aes256Gcm::new(key);

    let nonce = Nonce::from_slice(&bytes[4..16]);
    let ciphertext = &bytes[16..];

    let plaintext = cipher.decrypt(nonce, ciphertext).ok()?;
    String::from_utf8(plaintext).ok()
}

/// Encrypt if not already encrypted. Returns the original string if already encrypted.
#[allow(dead_code)]
pub fn encrypt_if_needed(plaintext: &str) -> Option<String> {
    if plaintext.is_empty() || is_encrypted_str(plaintext) {
        return Some(plaintext.to_string());
    }
    encrypt(plaintext)
}

/// Decrypt if encrypted. Returns the original string if not encrypted.
#[allow(dead_code)]
pub fn decrypt_if_needed(data: &str) -> Option<String> {
    if data.is_empty() || !is_encrypted_str(data) {
        return Some(data.to_string());
    }
    decrypt(data)
}

fn encode_base64(data: &[u8]) -> String {
    use base64::engine::general_purpose::STANDARD;
    use base64::Engine;
    STANDARD.encode(data)
}

fn decode_base64(encoded: &str) -> Option<Vec<u8>> {
    use base64::engine::general_purpose::STANDARD;
    use base64::Engine;
    STANDARD.decode(encoded).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        let plaintext = "sk-ant-api03-xxxxxxxxxxxxx";
        let encrypted = encrypt(plaintext).expect("encryption failed");
        assert!(is_encrypted_str(&encrypted));

        let decrypted = decrypt(&encrypted).expect("decryption failed");
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_encrypt_different_each_time() {
        let plaintext = "test-api-key-12345";
        let enc1 = encrypt(plaintext).unwrap();
        let enc2 = encrypt(plaintext).unwrap();
        // Same plaintext should produce different ciphertext due to random nonce
        assert_ne!(enc1, enc2);
        // But both should decrypt to the same value
        assert_eq!(decrypt(&enc1).unwrap(), plaintext);
        assert_eq!(decrypt(&enc2).unwrap(), plaintext);
    }

    #[test]
    fn test_decrypt_non_encrypted_passthrough() {
        let plaintext = "sk-regular-api-key-not-encrypted";
        // decrypt on non-encrypted data should pass through
        let result = decrypt(plaintext);
        assert_eq!(result, Some(plaintext.to_string()));
    }

    #[test]
    fn test_empty_string() {
        assert_eq!(encrypt(""), None);
        assert_eq!(decrypt(""), None);
    }

    #[test]
    fn test_encrypt_if_needed() {
        let plain = "my-secret-key";
        let encrypted = encrypt_if_needed(plain).unwrap();
        assert!(is_encrypted_str(&encrypted));

        // Should not double-encrypt
        let encrypted2 = encrypt_if_needed(&encrypted).unwrap();
        assert_eq!(encrypted, encrypted2);

        let decrypted = decrypt_if_needed(&encrypted).unwrap();
        assert_eq!(decrypted, plain);
    }

    #[test]
    fn test_is_encrypted_detection() {
        assert!(!is_encrypted_str("plain-text"));
        assert!(!is_encrypted_str(""));
        let enc = encrypt("test").unwrap();
        assert!(is_encrypted_str(&enc));
    }
}
