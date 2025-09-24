use crate::cloud_sync::{CloudSyncResult, CloudSyncError};
use ring::aead::{Aad, LessSafeKey, Nonce, UnboundKey, AES_256_GCM};
use ring::pbkdf2::{self, PBKDF2_HMAC_SHA256};
use ring::rand::{SecureRandom, SystemRandom};
use std::num::NonZeroU32;

const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 12;
const TAG_LEN: usize = 16;
const PBKDF2_ITERATIONS: u32 = 100_000;

pub struct CryptoService {
    rng: SystemRandom,
}

impl CryptoService {
    pub fn new() -> Self {
        Self {
            rng: SystemRandom::new(),
        }
    }

    pub fn encrypt(&self, data: &[u8], password: &str) -> CloudSyncResult<Vec<u8>> {
        // Generate random salt and nonce
        let mut salt = [0u8; SALT_LEN];
        let mut nonce_bytes = [0u8; NONCE_LEN];

        self.rng.fill(&mut salt)
            .map_err(|_| CloudSyncError::Encryption("Failed to generate salt".into()))?;
        self.rng.fill(&mut nonce_bytes)
            .map_err(|_| CloudSyncError::Encryption("Failed to generate nonce".into()))?;

        // Derive key from password using PBKDF2
        let mut key_bytes = [0u8; 32];
        pbkdf2::derive(
            PBKDF2_HMAC_SHA256,
            NonZeroU32::new(PBKDF2_ITERATIONS).unwrap(),
            &salt,
            password.as_bytes(),
            &mut key_bytes,
        );

        // Create encryption key
        let unbound_key = UnboundKey::new(&AES_256_GCM, &key_bytes)
            .map_err(|_| CloudSyncError::Encryption("Failed to create encryption key".into()))?;
        let key = LessSafeKey::new(unbound_key);

        // Encrypt data
        let nonce = Nonce::assume_unique_for_key(nonce_bytes);
        let mut encrypted = data.to_vec();

        key.seal_in_place_append_tag(nonce, Aad::empty(), &mut encrypted)
            .map_err(|_| CloudSyncError::Encryption("Failed to encrypt data".into()))?;

        // Build output: salt + nonce + encrypted_data_with_tag
        let mut result = Vec::with_capacity(SALT_LEN + NONCE_LEN + encrypted.len());
        result.extend_from_slice(&salt);
        result.extend_from_slice(&nonce_bytes);
        result.extend_from_slice(&encrypted);

        Ok(result)
    }

    pub fn decrypt(&self, data: &[u8], password: &str) -> CloudSyncResult<Vec<u8>> {
        // Validate minimum length
        if data.len() < SALT_LEN + NONCE_LEN + TAG_LEN {
            return Err(CloudSyncError::Decryption("Invalid encrypted data format".into()));
        }

        // Extract components
        let (salt, rest) = data.split_at(SALT_LEN);
        let (nonce_bytes, encrypted_with_tag) = rest.split_at(NONCE_LEN);

        // Derive key from password
        let mut key_bytes = [0u8; 32];
        pbkdf2::derive(
            PBKDF2_HMAC_SHA256,
            NonZeroU32::new(PBKDF2_ITERATIONS).unwrap(),
            salt,
            password.as_bytes(),
            &mut key_bytes,
        );

        // Create decryption key
        let unbound_key = UnboundKey::new(&AES_256_GCM, &key_bytes)
            .map_err(|_| CloudSyncError::Decryption("Failed to create decryption key".into()))?;
        let key = LessSafeKey::new(unbound_key);

        // Decrypt data
        let nonce = Nonce::assume_unique_for_key(*Nonce::try_assume_unique_for_key(nonce_bytes)
            .map_err(|_| CloudSyncError::Decryption("Invalid nonce".into()))?);

        let mut decrypted = encrypted_with_tag.to_vec();
        let plaintext = key.open_in_place(nonce, Aad::empty(), &mut decrypted)
            .map_err(|_| CloudSyncError::Decryption("Failed to decrypt data or invalid password".into()))?;

        Ok(plaintext.to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        let service = CryptoService::new();
        let original = b"This is sensitive configuration data";
        let password = "test_password_123";

        let encrypted = service.encrypt(original, password).unwrap();
        assert!(encrypted.len() > original.len() + SALT_LEN + NONCE_LEN);

        let decrypted = service.decrypt(&encrypted, password).unwrap();
        assert_eq!(decrypted, original);
    }

    #[test]
    fn test_decrypt_with_wrong_password_fails() {
        let service = CryptoService::new();
        let original = b"Secret data";
        let password = "correct_password";
        let wrong_password = "wrong_password";

        let encrypted = service.encrypt(original, password).unwrap();
        let result = service.decrypt(&encrypted, wrong_password);

        assert!(result.is_err());
        if let Err(CloudSyncError::Decryption(msg)) = result {
            assert!(msg.contains("Failed to decrypt"));
        } else {
            panic!("Expected decryption error");
        }
    }

    #[test]
    fn test_encrypted_data_is_different_each_time() {
        let service = CryptoService::new();
        let data = b"Same data";
        let password = "password";

        let encrypted1 = service.encrypt(data, password).unwrap();
        let encrypted2 = service.encrypt(data, password).unwrap();

        assert_ne!(encrypted1, encrypted2);
    }
}