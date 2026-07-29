use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::{
    aead::{Aead, Payload},
    KeyInit, XChaCha20Poly1305, XNonce,
};
use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use rand::{rngs::OsRng, RngCore};
use sha2::{Digest, Sha256};
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

use crate::error::AppError;

const MASTER_KEY_LEN: usize = 32;
const SUBKEY_LEN: usize = 32;
const XNONCE_LEN: usize = 24;
const MIN_SALT_LEN: usize = 16;
const MAX_SALT_LEN: usize = 64;
const KEY_CHECK_LABEL: &[u8] = b"cc-switch/workspace-sync/key-check/v1";

type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KdfParams {
    pub memory_kib: u32,
    pub iterations: u32,
    pub parallelism: u32,
}

impl Default for KdfParams {
    fn default() -> Self {
        Self {
            memory_kib: 65_536,
            iterations: 3,
            parallelism: 1,
        }
    }
}

impl KdfParams {
    #[cfg(test)]
    pub fn test() -> Self {
        Self {
            memory_kib: 32,
            iterations: 1,
            parallelism: 1,
        }
    }

    fn to_argon2_params(self) -> Result<Params, AppError> {
        Params::new(
            self.memory_kib,
            self.iterations,
            self.parallelism,
            Some(MASTER_KEY_LEN),
        )
        .map_err(|_| invalid_input("invalid workspace sync KDF parameters"))
    }
}

#[derive(Zeroize, ZeroizeOnDrop)]
pub struct KeyMaterial {
    manifest_key: [u8; SUBKEY_LEN],
    blob_key: [u8; SUBKEY_LEN],
    object_id_key: [u8; SUBKEY_LEN],
    nonce_key: [u8; SUBKEY_LEN],
    key_check_key: [u8; SUBKEY_LEN],
}

impl KeyMaterial {
    pub fn derive(
        password: impl AsRef<[u8]>,
        salt: &[u8],
        params: KdfParams,
    ) -> Result<Self, AppError> {
        validate_salt(salt)?;
        let argon2_params = params.to_argon2_params()?;
        let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, argon2_params);
        let mut master = Zeroizing::new([0_u8; MASTER_KEY_LEN]);
        argon2
            .hash_password_into(password.as_ref(), salt, master.as_mut())
            .map_err(|_| invalid_input("workspace sync key derivation failed"))?;

        let result = derive_subkeys(&master);
        master.zeroize();
        result
    }

    pub fn object_id(&self, domain: impl AsRef<[u8]>, plaintext: &[u8]) -> String {
        let content_hash = Sha256::digest(plaintext);
        let mut mac = new_hmac(&self.object_id_key);
        mac.update(domain.as_ref());
        mac.update(&[0]);
        mac.update(&content_hash);
        encode_hex(&mac.finalize().into_bytes())
    }

    pub fn encrypt_blob(
        &self,
        object_id: &str,
        plaintext: &[u8],
        aad: &[u8],
    ) -> Result<Vec<u8>, AppError> {
        validate_object_id(object_id)?;
        let object_key = self.derive_object_key(object_id)?;
        let nonce = self.object_nonce(object_id);
        encrypt_with_key(&object_key, &nonce, b"blob", plaintext, aad)
    }

    pub fn decrypt_blob(
        &self,
        object_id: &str,
        ciphertext: &[u8],
        aad: &[u8],
    ) -> Result<Vec<u8>, AppError> {
        validate_object_id(object_id)?;
        let object_key = self.derive_object_key(object_id)?;
        let nonce = self.object_nonce(object_id);
        decrypt_with_key(&object_key, &nonce, b"blob", ciphertext, aad)
    }

    pub fn encrypt_manifest(&self, plaintext: &[u8], aad: &[u8]) -> Result<Vec<u8>, AppError> {
        let mut nonce = [0_u8; XNONCE_LEN];
        OsRng.fill_bytes(&mut nonce);
        let ciphertext = encrypt_with_key(&self.manifest_key, &nonce, b"manifest", plaintext, aad)?;
        let mut output = Vec::with_capacity(XNONCE_LEN + ciphertext.len());
        output.extend_from_slice(&nonce);
        output.extend_from_slice(&ciphertext);
        Ok(output)
    }

    pub fn decrypt_manifest(&self, ciphertext: &[u8], aad: &[u8]) -> Result<Vec<u8>, AppError> {
        let (nonce, encrypted) = ciphertext
            .split_at_checked(XNONCE_LEN)
            .ok_or_else(authentication_failed)?;
        let nonce: &[u8; XNONCE_LEN] = nonce.try_into().map_err(|_| authentication_failed())?;
        decrypt_with_key(&self.manifest_key, nonce, b"manifest", encrypted, aad)
    }

    pub fn key_check(&self) -> [u8; 32] {
        let mut mac = new_hmac(&self.key_check_key);
        mac.update(KEY_CHECK_LABEL);
        mac.finalize().into_bytes().into()
    }

    pub fn verify_key_check(&self, expected: &[u8]) -> bool {
        let mut mac = new_hmac(&self.key_check_key);
        mac.update(KEY_CHECK_LABEL);
        mac.verify_slice(expected).is_ok()
    }

    fn derive_object_key(&self, object_id: &str) -> Result<Zeroizing<[u8; SUBKEY_LEN]>, AppError> {
        let hkdf = Hkdf::<Sha256>::new(Some(object_id.as_bytes()), &self.blob_key);
        let mut key = Zeroizing::new([0_u8; SUBKEY_LEN]);
        hkdf.expand(b"cc-switch/workspace-sync/blob-object/v1", key.as_mut())
            .map_err(|_| encryption_failed())?;
        Ok(key)
    }

    fn object_nonce(&self, object_id: &str) -> [u8; XNONCE_LEN] {
        let mut mac = new_hmac(&self.nonce_key);
        mac.update(object_id.as_bytes());
        let digest = mac.finalize().into_bytes();
        let mut nonce = [0_u8; XNONCE_LEN];
        nonce.copy_from_slice(&digest[..XNONCE_LEN]);
        nonce
    }
}

fn derive_subkeys(master: &[u8; MASTER_KEY_LEN]) -> Result<KeyMaterial, AppError> {
    let hkdf = Hkdf::<Sha256>::new(None, master);
    Ok(KeyMaterial {
        manifest_key: expand_subkey(&hkdf, b"cc-switch/workspace-sync/manifest/v1")?,
        blob_key: expand_subkey(&hkdf, b"cc-switch/workspace-sync/blob/v1")?,
        object_id_key: expand_subkey(&hkdf, b"cc-switch/workspace-sync/object-id/v1")?,
        nonce_key: expand_subkey(&hkdf, b"cc-switch/workspace-sync/nonce/v1")?,
        key_check_key: expand_subkey(&hkdf, b"cc-switch/workspace-sync/key-check/v1")?,
    })
}

fn expand_subkey(hkdf: &Hkdf<Sha256>, label: &[u8]) -> Result<[u8; SUBKEY_LEN], AppError> {
    let mut key = [0_u8; SUBKEY_LEN];
    hkdf.expand(label, &mut key)
        .map_err(|_| encryption_failed())?;
    Ok(key)
}

fn encrypt_with_key(
    key: &[u8; SUBKEY_LEN],
    nonce: &[u8; XNONCE_LEN],
    domain: &[u8],
    plaintext: &[u8],
    aad: &[u8],
) -> Result<Vec<u8>, AppError> {
    let cipher = XChaCha20Poly1305::new(key.into());
    let authenticated_data = domain_aad(domain, aad);
    cipher
        .encrypt(
            XNonce::from_slice(nonce),
            Payload {
                msg: plaintext,
                aad: &authenticated_data,
            },
        )
        .map_err(|_| encryption_failed())
}

fn decrypt_with_key(
    key: &[u8; SUBKEY_LEN],
    nonce: &[u8; XNONCE_LEN],
    domain: &[u8],
    ciphertext: &[u8],
    aad: &[u8],
) -> Result<Vec<u8>, AppError> {
    let cipher = XChaCha20Poly1305::new(key.into());
    let authenticated_data = domain_aad(domain, aad);
    cipher
        .decrypt(
            XNonce::from_slice(nonce),
            Payload {
                msg: ciphertext,
                aad: &authenticated_data,
            },
        )
        .map_err(|_| authentication_failed())
}

fn domain_aad(domain: &[u8], aad: &[u8]) -> Vec<u8> {
    let mut authenticated_data = Vec::with_capacity(domain.len() + 1 + aad.len());
    authenticated_data.extend_from_slice(domain);
    authenticated_data.push(0);
    authenticated_data.extend_from_slice(aad);
    authenticated_data
}

fn validate_salt(salt: &[u8]) -> Result<(), AppError> {
    if !(MIN_SALT_LEN..=MAX_SALT_LEN).contains(&salt.len()) {
        return Err(invalid_input("workspace sync salt must be 16 to 64 bytes"));
    }
    Ok(())
}

fn validate_object_id(object_id: &str) -> Result<(), AppError> {
    if object_id.is_empty()
        || !object_id.is_ascii()
        || !object_id.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(invalid_input(
            "workspace sync object ID must be non-empty hexadecimal ASCII",
        ));
    }
    Ok(())
}

fn new_hmac(key: &[u8]) -> HmacSha256 {
    <HmacSha256 as Mac>::new_from_slice(key).expect("HMAC-SHA256 accepts any key length")
}

fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

fn invalid_input(message: &'static str) -> AppError {
    AppError::InvalidInput(message.to_string())
}

fn encryption_failed() -> AppError {
    AppError::Message("workspace sync encryption failed".to_string())
}

fn authentication_failed() -> AppError {
    AppError::Message("workspace sync authentication failed".to_string())
}

#[cfg(test)]
mod tests {
    use super::{KdfParams, KeyMaterial};
    use sha2::{Digest, Sha256};

    const SALT: &[u8] = b"0123456789abcdef";
    const DOMAIN: &[u8] = b"v1/profile-1/codex/session";

    fn keys(password: &str) -> KeyMaterial {
        KeyMaterial::derive(password.as_bytes(), SALT, KdfParams::test())
            .expect("test key derivation should succeed")
    }

    #[test]
    fn kdf_defaults_and_validation_match_the_protocol() {
        assert_eq!(
            KdfParams::default(),
            KdfParams {
                memory_kib: 65_536,
                iterations: 3,
                parallelism: 1,
            }
        );

        assert!(KeyMaterial::derive(b"password", b"too-short", KdfParams::test()).is_err());
        assert!(KeyMaterial::derive(
            b"password",
            SALT,
            KdfParams {
                memory_kib: 0,
                iterations: 0,
                parallelism: 0,
            },
        )
        .is_err());
    }

    #[test]
    fn blob_round_trip() {
        let keys = keys("correct horse battery staple");
        let plaintext = b"encrypted workspace payload";
        let aad = b"provider=codex;kind=session";
        let object_id = keys.object_id(DOMAIN, plaintext);

        let ciphertext = keys
            .encrypt_blob(&object_id, plaintext, aad)
            .expect("blob encryption should succeed");
        let decrypted = keys
            .decrypt_blob(&object_id, &ciphertext, aad)
            .expect("blob decryption should succeed");

        assert_eq!(decrypted, plaintext);
        assert_ne!(ciphertext, plaintext);
    }

    #[test]
    fn wrong_password_is_rejected() {
        let correct = keys("correct password");
        let wrong = keys("wrong password");
        let plaintext = b"secret";
        let object_id = correct.object_id(DOMAIN, plaintext);
        let ciphertext = correct
            .encrypt_blob(&object_id, plaintext, b"metadata")
            .expect("blob encryption should succeed");

        let error = wrong
            .decrypt_blob(&object_id, &ciphertext, b"metadata")
            .expect_err("wrong password must not decrypt");
        let message = error.to_string();
        assert!(!message.contains("wrong password"));
        assert!(!message.contains("secret"));
    }

    #[test]
    fn tampered_ciphertext_is_rejected() {
        let keys = keys("password");
        let plaintext = b"secret";
        let object_id = keys.object_id(DOMAIN, plaintext);
        let mut ciphertext = keys
            .encrypt_blob(&object_id, plaintext, b"metadata")
            .expect("blob encryption should succeed");
        ciphertext[0] ^= 0x80;

        assert!(keys
            .decrypt_blob(&object_id, &ciphertext, b"metadata")
            .is_err());
    }

    #[test]
    fn different_aad_is_rejected() {
        let keys = keys("password");
        let plaintext = b"secret";
        let object_id = keys.object_id(DOMAIN, plaintext);
        let ciphertext = keys
            .encrypt_blob(&object_id, plaintext, b"expected-aad")
            .expect("blob encryption should succeed");

        assert!(keys
            .decrypt_blob(&object_id, &ciphertext, b"different-aad")
            .is_err());
    }

    #[test]
    fn object_ids_are_stable_domain_separated_and_not_plain_sha256() {
        let keys = keys("password");
        let plaintext = b"same content";
        let first = keys.object_id(DOMAIN, plaintext);
        let second = keys.object_id(DOMAIN, plaintext);
        let other_domain = keys.object_id(b"v1/profile-2/codex/session", plaintext);
        let plain_sha = format!("{:x}", Sha256::digest(plaintext));

        assert_eq!(first, second);
        assert_ne!(first, other_domain);
        assert_ne!(first, plain_sha);
        assert_eq!(first.len(), 64);
        assert!(first.bytes().all(|byte| byte.is_ascii_hexdigit()));
    }

    #[test]
    fn manifest_encryption_uses_fresh_nonces() {
        let keys = keys("password");
        let plaintext = b"manifest";
        let aad = b"workspace-manifest-v1";

        let first = keys
            .encrypt_manifest(plaintext, aad)
            .expect("manifest encryption should succeed");
        let second = keys
            .encrypt_manifest(plaintext, aad)
            .expect("manifest encryption should succeed");

        assert_ne!(first, second);
        assert_eq!(
            keys.decrypt_manifest(&first, aad)
                .expect("first manifest should decrypt"),
            plaintext
        );
        assert_eq!(
            keys.decrypt_manifest(&second, aad)
                .expect("second manifest should decrypt"),
            plaintext
        );
    }

    #[test]
    fn invalid_object_ids_are_rejected() {
        let keys = keys("password");

        for invalid in ["", "not-hex", "密文"] {
            assert!(keys.encrypt_blob(invalid, b"secret", b"aad").is_err());
            assert!(keys.decrypt_blob(invalid, b"ciphertext", b"aad").is_err());
        }
    }

    #[test]
    fn key_check_accepts_matching_key_and_rejects_wrong_key() {
        let correct = keys("correct password");
        let same = keys("correct password");
        let wrong = keys("wrong password");
        let tag = correct.key_check();

        assert!(same.verify_key_check(&tag));
        assert!(!wrong.verify_key_check(&tag));
    }
}
