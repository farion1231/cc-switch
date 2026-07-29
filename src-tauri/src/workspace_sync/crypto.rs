use argon2::{Algorithm, Argon2, Block, Params, Version};
use chacha20poly1305::{
    aead::{Aead, Payload},
    KeyInit, XChaCha20Poly1305, XNonce,
};
use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use rand::{rngs::OsRng, CryptoRng, RngCore};
use sha2::{Digest, Sha256};
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

use crate::error::AppError;

const MASTER_KEY_LEN: usize = 32;
const SUBKEY_LEN: usize = 32;
const XNONCE_LEN: usize = 24;
const MIN_SALT_LEN: usize = 16;
const MAX_SALT_LEN: usize = 64;
const MAX_KDF_MEMORY_KIB: u32 = 1_048_576;
const MAX_KDF_ITERATIONS: u32 = 10;
const MAX_KDF_PARALLELISM: u32 = 16;
const MAX_DOMAIN_LEN: usize = 1_024;
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
            memory_kib: 8,
            iterations: 1,
            parallelism: 1,
        }
    }

    fn to_argon2_params(self) -> Result<Params, AppError> {
        if self.memory_kib == 0
            || self.memory_kib > MAX_KDF_MEMORY_KIB
            || self.iterations == 0
            || self.iterations > MAX_KDF_ITERATIONS
            || self.parallelism == 0
            || self.parallelism > MAX_KDF_PARALLELISM
        {
            return Err(invalid_input("invalid workspace sync KDF parameters"));
        }

        Params::new(
            self.memory_kib,
            self.iterations,
            self.parallelism,
            Some(MASTER_KEY_LEN),
        )
        .map_err(|_| invalid_input("invalid workspace sync KDF parameters"))
    }
}

pub struct SensitivePassword(Zeroizing<Vec<u8>>);

impl SensitivePassword {
    pub fn new(password: Vec<u8>) -> Self {
        Self(Zeroizing::new(password))
    }

    fn as_bytes(&self) -> &[u8] {
        self.0.as_slice()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SealedBlob {
    pub object_id: String,
    pub ciphertext: Vec<u8>,
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
        password: &SensitivePassword,
        salt: &[u8],
        params: KdfParams,
    ) -> Result<Self, AppError> {
        validate_salt(salt)?;
        let argon2_params = params.to_argon2_params()?;
        let block_count = argon2_params.block_count();
        let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, argon2_params);
        let mut work_memory = Vec::new();
        work_memory
            .try_reserve_exact(block_count)
            .map_err(|_| kdf_allocation_failed())?;
        work_memory.resize(block_count, Block::default());
        let mut work_memory = Zeroizing::new(work_memory);
        let mut master = Zeroizing::new([0_u8; MASTER_KEY_LEN]);

        let hash_result = argon2.hash_password_into_with_memory(
            password.as_bytes(),
            salt,
            master.as_mut(),
            work_memory.as_mut_slice(),
        );
        work_memory.zeroize();
        hash_result.map_err(|_| invalid_input("workspace sync key derivation failed"))?;

        let result = derive_subkeys(&master);
        master.zeroize();
        result
    }

    pub fn seal_blob(&self, domain: &[u8], plaintext: &[u8]) -> Result<SealedBlob, AppError> {
        validate_domain(domain)?;
        let object_id = self.object_id(domain, plaintext);
        let object_key = self.derive_object_key(&object_id)?;
        let nonce = self.object_nonce(&object_id);
        let aad = canonical_blob_aad(domain, &object_id);
        let ciphertext = encrypt_with_key(&object_key, &nonce, plaintext, &aad)?;
        Ok(SealedBlob {
            object_id,
            ciphertext,
        })
    }

    pub fn open_blob(
        &self,
        domain: &[u8],
        object_id: &str,
        ciphertext: &[u8],
    ) -> Result<Vec<u8>, AppError> {
        validate_domain(domain)?;
        validate_object_id(object_id)?;
        let object_key = self.derive_object_key(object_id)?;
        let nonce = self.object_nonce(object_id);
        let aad = canonical_blob_aad(domain, object_id);
        let mut plaintext = decrypt_with_key(&object_key, &nonce, ciphertext, &aad)?;

        if !self.verify_object_id(domain, &plaintext, object_id) {
            plaintext.zeroize();
            return Err(authentication_failed());
        }

        Ok(plaintext)
    }

    pub fn encrypt_manifest(&self, plaintext: &[u8], aad: &[u8]) -> Result<Vec<u8>, AppError> {
        let mut rng = OsRng;
        self.encrypt_manifest_with_rng(plaintext, aad, &mut rng)
    }

    fn encrypt_manifest_with_rng<R: RngCore + CryptoRng>(
        &self,
        plaintext: &[u8],
        aad: &[u8],
        rng: &mut R,
    ) -> Result<Vec<u8>, AppError> {
        let mut nonce = [0_u8; XNONCE_LEN];
        rng.try_fill_bytes(&mut nonce)
            .map_err(|_| random_nonce_failed())?;
        let ciphertext = encrypt_with_key(&self.manifest_key, &nonce, plaintext, aad)?;
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
        decrypt_with_key(&self.manifest_key, nonce, encrypted, aad)
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

    fn object_id(&self, domain: &[u8], plaintext: &[u8]) -> String {
        let content_hash = Sha256::digest(plaintext);
        let mut mac = new_hmac(&self.object_id_key);
        mac.update(domain);
        mac.update(&[0]);
        mac.update(&content_hash);
        encode_hex(&mac.finalize().into_bytes())
    }

    fn verify_object_id(&self, domain: &[u8], plaintext: &[u8], object_id: &str) -> bool {
        let Some(provided) = decode_object_id(object_id) else {
            return false;
        };
        let content_hash = Sha256::digest(plaintext);
        let mut mac = new_hmac(&self.object_id_key);
        mac.update(domain);
        mac.update(&[0]);
        mac.update(&content_hash);
        mac.verify_slice(&provided).is_ok()
    }

    fn derive_object_key(&self, object_id: &str) -> Result<Zeroizing<[u8; SUBKEY_LEN]>, AppError> {
        let hkdf = Hkdf::<Sha256>::new(Some(object_id.as_bytes()), &self.blob_key);
        let mut key = Zeroizing::new([0_u8; SUBKEY_LEN]);
        hkdf.expand(b"blob", key.as_mut())
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
        manifest_key: expand_subkey(&hkdf, b"manifest")?,
        blob_key: expand_subkey(&hkdf, b"blob")?,
        object_id_key: expand_subkey(&hkdf, b"object-id")?,
        nonce_key: expand_subkey(&hkdf, b"nonce")?,
        key_check_key: expand_subkey(&hkdf, b"key-check")?,
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
    plaintext: &[u8],
    aad: &[u8],
) -> Result<Vec<u8>, AppError> {
    let cipher = XChaCha20Poly1305::new(key.into());
    cipher
        .encrypt(
            XNonce::from_slice(nonce),
            Payload {
                msg: plaintext,
                aad,
            },
        )
        .map_err(|_| encryption_failed())
}

fn decrypt_with_key(
    key: &[u8; SUBKEY_LEN],
    nonce: &[u8; XNONCE_LEN],
    ciphertext: &[u8],
    aad: &[u8],
) -> Result<Vec<u8>, AppError> {
    let cipher = XChaCha20Poly1305::new(key.into());
    cipher
        .decrypt(
            XNonce::from_slice(nonce),
            Payload {
                msg: ciphertext,
                aad,
            },
        )
        .map_err(|_| authentication_failed())
}

fn canonical_blob_aad(domain: &[u8], object_id: &str) -> Vec<u8> {
    let mut aad = Vec::with_capacity(domain.len() + 1 + object_id.len());
    aad.extend_from_slice(domain);
    aad.push(0);
    aad.extend_from_slice(object_id.as_bytes());
    aad
}

fn validate_domain(domain: &[u8]) -> Result<(), AppError> {
    if domain.is_empty() || domain.len() > MAX_DOMAIN_LEN {
        return Err(invalid_input(
            "workspace sync domain must be between 1 and 1024 bytes",
        ));
    }
    Ok(())
}

fn validate_salt(salt: &[u8]) -> Result<(), AppError> {
    if !(MIN_SALT_LEN..=MAX_SALT_LEN).contains(&salt.len()) {
        return Err(invalid_input("workspace sync salt must be 16 to 64 bytes"));
    }
    Ok(())
}

fn validate_object_id(object_id: &str) -> Result<(), AppError> {
    if object_id.len() != 64
        || !object_id
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
    {
        return Err(invalid_input(
            "workspace sync object ID must be 64 lowercase hexadecimal ASCII characters",
        ));
    }
    Ok(())
}

fn decode_object_id(object_id: &str) -> Option<[u8; 32]> {
    if validate_object_id(object_id).is_err() {
        return None;
    }

    let mut decoded = [0_u8; 32];
    for (index, pair) in object_id.as_bytes().chunks_exact(2).enumerate() {
        decoded[index] = (decode_hex_nibble(pair[0])? << 4) | decode_hex_nibble(pair[1])?;
    }
    Some(decoded)
}

fn decode_hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    }
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

fn kdf_allocation_failed() -> AppError {
    AppError::Message("workspace sync KDF memory allocation failed".to_string())
}

fn random_nonce_failed() -> AppError {
    AppError::Message("workspace sync random nonce generation failed".to_string())
}

fn encryption_failed() -> AppError {
    AppError::Message("workspace sync encryption failed".to_string())
}

fn authentication_failed() -> AppError {
    AppError::Message("workspace sync authentication failed".to_string())
}

#[cfg(test)]
mod tests {
    use super::{
        derive_subkeys, encode_hex, KdfParams, KeyMaterial, SensitivePassword, MAX_DOMAIN_LEN,
        SUBKEY_LEN, XNONCE_LEN,
    };
    use chacha20poly1305::{
        aead::{Aead, Payload},
        KeyInit, XChaCha20Poly1305, XNonce,
    };
    use hkdf::Hkdf;
    use rand::{CryptoRng, Error as RandError, RngCore};
    use sha2::{Digest, Sha256};

    const SALT: &[u8] = b"0123456789abcdef";
    const DOMAIN: &[u8] = b"v1/profile-1/codex/session";

    fn password(value: &str) -> SensitivePassword {
        SensitivePassword::new(value.as_bytes().to_vec())
    }

    fn keys(value: &str) -> KeyMaterial {
        let password = password(value);
        KeyMaterial::derive(&password, SALT, KdfParams::test())
            .expect("test key derivation should succeed")
    }

    fn canonical_aad(domain: &[u8], object_id: &str) -> Vec<u8> {
        let mut aad = Vec::with_capacity(domain.len() + 1 + object_id.len());
        aad.extend_from_slice(domain);
        aad.push(0);
        aad.extend_from_slice(object_id.as_bytes());
        aad
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
        assert_eq!(
            KdfParams::test(),
            KdfParams {
                memory_kib: 8,
                iterations: 1,
                parallelism: 1,
            }
        );

        let password = password("password");
        assert!(KeyMaterial::derive(&password, b"too-short", KdfParams::test()).is_err());
        assert!(KeyMaterial::derive(
            &password,
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
    fn kdf_rejects_resource_limits_before_argon2_allocation() {
        for params in [
            KdfParams {
                memory_kib: 1_048_577,
                iterations: 1,
                parallelism: 1,
            },
            KdfParams {
                memory_kib: 8,
                iterations: 11,
                parallelism: 1,
            },
            KdfParams {
                memory_kib: 128,
                iterations: 1,
                parallelism: 17,
            },
        ] {
            assert!(params.to_argon2_params().is_err());
        }
    }

    #[test]
    fn hkdf_uses_exact_protocol_info_labels() {
        let master = [0x5a; 32];
        let keys = derive_subkeys(&master).expect("subkey derivation should succeed");
        let hkdf = Hkdf::<Sha256>::new(None, &master);

        let cases: [(&[u8; SUBKEY_LEN], &[u8]); 5] = [
            (&keys.manifest_key, b"manifest"),
            (&keys.blob_key, b"blob"),
            (&keys.object_id_key, b"object-id"),
            (&keys.nonce_key, b"nonce"),
            (&keys.key_check_key, b"key-check"),
        ];

        for (actual, label) in cases {
            let mut expected = [0_u8; SUBKEY_LEN];
            hkdf.expand(label, &mut expected)
                .expect("32-byte HKDF expansion should succeed");
            assert_eq!(actual, &expected, "unexpected HKDF info label");
        }
    }

    #[test]
    fn seal_and_open_blob_round_trip_with_canonical_aad() {
        let keys = keys("correct horse battery staple");
        let plaintext = b"encrypted workspace payload";
        let sealed = keys
            .seal_blob(DOMAIN, plaintext)
            .expect("blob sealing should succeed");

        let hkdf = Hkdf::<Sha256>::new(Some(sealed.object_id.as_bytes()), &keys.blob_key);
        let mut object_key = [0_u8; SUBKEY_LEN];
        hkdf.expand(b"blob", &mut object_key)
            .expect("object key derivation should succeed");
        let nonce = keys.object_nonce(&sealed.object_id);
        let cipher = XChaCha20Poly1305::new((&object_key).into());
        let decrypted_by_raw_aead = cipher
            .decrypt(
                XNonce::from_slice(&nonce),
                Payload {
                    msg: &sealed.ciphertext,
                    aad: &canonical_aad(DOMAIN, &sealed.object_id),
                },
            )
            .expect("canonical AAD should interoperate with raw AEAD");

        assert_eq!(decrypted_by_raw_aead, plaintext);
        assert_eq!(
            keys.open_blob(DOMAIN, &sealed.object_id, &sealed.ciphertext)
                .expect("sealed blob should open"),
            plaintext
        );
    }

    #[test]
    fn wrong_password_is_rejected_without_secret_error_text() {
        let correct = keys("correct password");
        let wrong = keys("wrong password");
        let sealed = correct
            .seal_blob(DOMAIN, b"secret")
            .expect("blob sealing should succeed");

        let error = wrong
            .open_blob(DOMAIN, &sealed.object_id, &sealed.ciphertext)
            .expect_err("wrong password must not decrypt");
        let message = error.to_string();
        assert!(!message.contains("wrong password"));
        assert!(!message.contains("secret"));
    }

    #[test]
    fn wrong_domain_is_rejected() {
        let keys = keys("password");
        let sealed = keys
            .seal_blob(DOMAIN, b"secret")
            .expect("blob sealing should succeed");

        assert!(keys
            .open_blob(
                b"v1/profile-2/codex/session",
                &sealed.object_id,
                &sealed.ciphertext,
            )
            .is_err());
    }

    #[test]
    fn path_object_id_that_does_not_match_plaintext_is_rejected() {
        let keys = keys("password");
        let plaintext = b"actual content";
        let mismatched_id = keys.object_id(DOMAIN, b"different content");
        let hkdf = Hkdf::<Sha256>::new(Some(mismatched_id.as_bytes()), &keys.blob_key);
        let mut object_key = [0_u8; SUBKEY_LEN];
        hkdf.expand(b"blob", &mut object_key)
            .expect("object key derivation should succeed");
        let nonce = keys.object_nonce(&mismatched_id);
        let cipher = XChaCha20Poly1305::new((&object_key).into());
        let ciphertext = cipher
            .encrypt(
                XNonce::from_slice(&nonce),
                Payload {
                    msg: plaintext,
                    aad: &canonical_aad(DOMAIN, &mismatched_id),
                },
            )
            .expect("crafted authenticated ciphertext should encrypt");

        assert!(keys.open_blob(DOMAIN, &mismatched_id, &ciphertext).is_err());
    }

    #[test]
    fn tampered_blob_ciphertext_is_rejected() {
        let keys = keys("password");
        let mut sealed = keys
            .seal_blob(DOMAIN, b"secret")
            .expect("blob sealing should succeed");
        sealed.ciphertext[0] ^= 0x80;

        assert!(keys
            .open_blob(DOMAIN, &sealed.object_id, &sealed.ciphertext)
            .is_err());
    }

    #[test]
    fn invalid_domains_are_rejected() {
        let keys = keys("password");
        let oversized = vec![b'x'; MAX_DOMAIN_LEN + 1];
        let valid_id = "a".repeat(64);

        assert!(keys.seal_blob(b"", b"secret").is_err());
        assert!(keys.seal_blob(&oversized, b"secret").is_err());
        assert!(keys.open_blob(b"", &valid_id, b"ciphertext").is_err());
        assert!(keys
            .open_blob(&oversized, &valid_id, b"ciphertext")
            .is_err());
    }

    #[test]
    fn invalid_object_ids_are_rejected_by_open() {
        let keys = keys("password");
        let invalid = [
            String::new(),
            "aa".to_string(),
            "a".repeat(63),
            "A".repeat(64),
            format!("{}g", "a".repeat(63)),
        ];

        for object_id in invalid {
            assert!(keys.open_blob(DOMAIN, &object_id, b"ciphertext").is_err());
        }
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
        assert!(first
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f')));
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
    fn short_or_tampered_manifest_ciphertext_is_rejected() {
        let keys = keys("password");
        let aad = b"workspace-manifest-v1";
        let short_nonce = [0_u8; XNONCE_LEN - 1];
        let nonce_without_tag = [0_u8; XNONCE_LEN];

        assert!(keys.decrypt_manifest(&short_nonce, aad).is_err());
        assert!(keys.decrypt_manifest(&nonce_without_tag, aad).is_err());

        let mut encrypted = keys
            .encrypt_manifest(b"manifest", aad)
            .expect("manifest encryption should succeed");
        let tag_byte = encrypted
            .last_mut()
            .expect("encrypted manifest includes an authentication tag");
        *tag_byte ^= 0x01;
        assert!(keys.decrypt_manifest(&encrypted, aad).is_err());
    }

    struct FailingRng;

    impl RngCore for FailingRng {
        fn next_u32(&mut self) -> u32 {
            0
        }

        fn next_u64(&mut self) -> u64 {
            0
        }

        fn fill_bytes(&mut self, _dest: &mut [u8]) {
            panic!("infallible RNG API must not be used");
        }

        fn try_fill_bytes(&mut self, _dest: &mut [u8]) -> Result<(), RandError> {
            Err(RandError::new(std::io::Error::other("rng unavailable")))
        }
    }

    impl CryptoRng for FailingRng {}

    #[test]
    fn manifest_rng_failure_is_mapped_to_app_error() {
        let keys = keys("password");
        let error = keys
            .encrypt_manifest_with_rng(b"manifest", b"aad", &mut FailingRng)
            .expect_err("fallible RNG failure must be returned");

        assert_eq!(
            error.to_string(),
            "workspace sync random nonce generation failed"
        );
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

    #[test]
    fn protocol_vectors_cover_object_id_and_key_check() {
        let keys = keys("protocol-vector-password");

        assert_eq!(
            keys.object_id(DOMAIN, b"protocol vector payload"),
            "7fdb79d2c07e4637fed397d4cf5999c1ea1e7effbf8b763d5934e7aa49f2ec5b"
        );
        assert_eq!(
            encode_hex(&keys.key_check()),
            "7d1beecd61b8f3df181cebb0c85d2492210fd1d8e9dcbb2220738f55743919ef"
        );
    }
}
