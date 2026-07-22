//! Local at-rest encryption for sensitive DB blobs (provider settings_config).
//!
//! Current format (`ccs1:` prefix): AES-256-GCM with a machine-local key file
//! under `~/.cc-switch/db-secrets.key`.
//!
//! Legacy format (pre-secrets.rs / `crypto.rs`): base64(`CCS1` + nonce + ct)
//! with a hostname-derived key. Still accepted on read and migrated to the
//! current format on encrypt/migrate passes so we never double-wrap it.

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use rand::RngCore;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::PathBuf;
use std::sync::OnceLock;

use crate::config::get_app_config_dir;
use crate::error::AppError;

const PREFIX: &str = "ccs1:";
const KEY_FILE: &str = "db-secrets.key";
const KEY_LEN: usize = 32;
const NONCE_LEN: usize = 12;
/// Legacy magic from the old `crypto.rs` module (bytes, not the text prefix).
const LEGACY_MAGIC: &[u8; 4] = b"CCS1";
const LEGACY_APP_SALT: &[u8] = b"cc-switch-encryption-v1-salt-2024";

static CIPHER: OnceLock<Aes256Gcm> = OnceLock::new();

fn key_path() -> PathBuf {
    get_app_config_dir().join(KEY_FILE)
}

fn load_or_create_key() -> Result<[u8; KEY_LEN], AppError> {
    let path = key_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
    }

    if path.exists() {
        let bytes = fs::read(&path).map_err(|e| AppError::io(&path, e))?;
        if bytes.len() != KEY_LEN {
            return Err(AppError::localized(
                "secrets.key_invalid",
                format!("本地密钥文件长度无效: {}", path.display()),
                format!("Local secrets key file has invalid length: {}", path.display()),
            ));
        }
        let mut key = [0u8; KEY_LEN];
        key.copy_from_slice(&bytes);
        return Ok(key);
    }

    let mut key = [0u8; KEY_LEN];
    rand::thread_rng().fill_bytes(&mut key);

    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(&path)
            .map_err(|e| AppError::io(&path, e))?;
        file.write_all(&key)
            .map_err(|e| AppError::io(&path, e))?;
    }

    #[cfg(not(unix))]
    {
        fs::write(&path, key).map_err(|e| AppError::io(&path, e))?;
    }

    Ok(key)
}

fn cipher() -> Result<&'static Aes256Gcm, AppError> {
    if let Some(c) = CIPHER.get() {
        return Ok(c);
    }
    let key = load_or_create_key()?;
    let aes = Aes256Gcm::new_from_slice(&key).map_err(|e| {
        AppError::localized(
            "secrets.cipher_init_failed",
            format!("初始化加密器失败: {e}"),
            format!("Failed to initialize cipher: {e}"),
        )
    })?;
    let _ = CIPHER.set(aes);
    CIPHER.get().ok_or_else(|| {
        AppError::localized(
            "secrets.cipher_unavailable",
            "加密器不可用",
            "Cipher unavailable",
        )
    })
}

fn machine_hostname() -> String {
    std::env::var_os("COMPUTERNAME")
        .or_else(|| std::env::var_os("HOSTNAME"))
        .and_then(|v| v.into_string().ok())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown-host".to_string())
}

fn legacy_derive_key() -> [u8; KEY_LEN] {
    let hostname = machine_hostname();
    let os_info = format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH);
    let mut hasher = Sha256::new();
    hasher.update(hostname.as_bytes());
    hasher.update(b":");
    hasher.update(os_info.as_bytes());
    hasher.update(b":");
    hasher.update(LEGACY_APP_SALT);
    let result = hasher.finalize();
    let mut key = [0u8; KEY_LEN];
    key.copy_from_slice(&result);
    key
}

/// Detect the legacy `crypto.rs` payload: base64(`CCS1` + nonce + ciphertext).
fn is_legacy_encrypted(data: &str) -> bool {
    let Ok(bytes) = B64.decode(data.trim()) else {
        return false;
    };
    bytes.starts_with(LEGACY_MAGIC) && bytes.len() >= 4 + NONCE_LEN + 16
}

fn decrypt_legacy(data: &str) -> Result<String, AppError> {
    let bytes = B64.decode(data.trim()).map_err(|e| {
        AppError::localized(
            "secrets.legacy_decode_failed",
            format!("旧版密文 Base64 解码失败: {e}"),
            format!("Failed to decode legacy ciphertext: {e}"),
        )
    })?;
    if !bytes.starts_with(LEGACY_MAGIC) || bytes.len() < 4 + NONCE_LEN + 16 {
        return Err(AppError::localized(
            "secrets.legacy_invalid",
            "旧版密文格式无效",
            "Invalid legacy ciphertext format",
        ));
    }

    let key = legacy_derive_key();
    let aes = Aes256Gcm::new_from_slice(&key).map_err(|e| {
        AppError::localized(
            "secrets.legacy_cipher_init_failed",
            format!("初始化旧版加密器失败: {e}"),
            format!("Failed to initialize legacy cipher: {e}"),
        )
    })?;
    let nonce = Nonce::from_slice(&bytes[4..4 + NONCE_LEN]);
    let ciphertext = &bytes[4 + NONCE_LEN..];
    let plaintext = aes.decrypt(nonce, ciphertext).map_err(|_| {
        AppError::localized(
            "secrets.legacy_decrypt_failed",
            "旧版密文解密失败：机器密钥可能不匹配或数据已损坏",
            "Legacy decryption failed: machine key mismatch or corrupted data",
        )
    })?;
    String::from_utf8(plaintext).map_err(|e| {
        AppError::localized(
            "secrets.legacy_utf8_failed",
            format!("旧版解密结果不是合法 UTF-8: {e}"),
            format!("Legacy decrypted data is not valid UTF-8: {e}"),
        )
    })
}

fn encrypt_with_current(plaintext: &str) -> Result<String, AppError> {
    let cipher = cipher()?;
    let mut nonce_bytes = [0u8; NONCE_LEN];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher.encrypt(nonce, plaintext.as_bytes()).map_err(|e| {
        AppError::localized(
            "secrets.encrypt_failed",
            format!("加密失败: {e}"),
            format!("Encryption failed: {e}"),
        )
    })?;

    let mut packed = Vec::with_capacity(NONCE_LEN + ciphertext.len());
    packed.extend_from_slice(&nonce_bytes);
    packed.extend_from_slice(&ciphertext);
    Ok(format!("{PREFIX}{}", B64.encode(packed)))
}

/// Normalize any stored blob to plaintext JSON (or other UTF-8 payload).
///
/// Handles:
/// - current `ccs1:` envelopes (including accidental double-wrap of legacy blobs)
/// - legacy base64(`CCS1`…) payloads
/// - plaintext passthrough
fn to_plaintext(data: &str) -> Result<String, AppError> {
    if let Some(encoded) = data.strip_prefix(PREFIX) {
        let packed = B64.decode(encoded).map_err(|e| {
            AppError::localized(
                "secrets.decode_failed",
                format!("密文 Base64 解码失败: {e}"),
                format!("Failed to decode ciphertext: {e}"),
            )
        })?;
        if packed.len() <= NONCE_LEN {
            return Err(AppError::localized(
                "secrets.ciphertext_too_short",
                "密文过短",
                "Ciphertext too short",
            ));
        }

        let (nonce_bytes, ciphertext) = packed.split_at(NONCE_LEN);
        let cipher = cipher()?;
        let nonce = Nonce::from_slice(nonce_bytes);
        let inner = cipher.decrypt(nonce, ciphertext).map_err(|_| {
            AppError::localized(
                "secrets.decrypt_failed",
                "解密失败：本地密钥可能不匹配或数据已损坏",
                "Decryption failed: local key mismatch or corrupted data",
            )
        })?;
        let inner_text = String::from_utf8(inner).map_err(|e| {
            AppError::localized(
                "secrets.utf8_failed",
                format!("解密结果不是合法 UTF-8: {e}"),
                format!("Decrypted data is not valid UTF-8: {e}"),
            )
        })?;

        // New encrypt accidentally wrapped a legacy blob → unwrap one more layer.
        if is_legacy_encrypted(&inner_text) {
            return decrypt_legacy(&inner_text);
        }
        return Ok(inner_text);
    }

    if is_legacy_encrypted(data) {
        return decrypt_legacy(data);
    }

    Ok(data.to_string())
}

/// Decrypt only the current `ccs1:` envelope (no legacy unwrapping).
fn decrypt_current_layer(data: &str) -> Result<String, AppError> {
    let Some(encoded) = data.strip_prefix(PREFIX) else {
        return Err(AppError::localized(
            "secrets.not_current_format",
            "不是当前密文格式",
            "Not current ciphertext format",
        ));
    };
    let packed = B64.decode(encoded).map_err(|e| {
        AppError::localized(
            "secrets.decode_failed",
            format!("密文 Base64 解码失败: {e}"),
            format!("Failed to decode ciphertext: {e}"),
        )
    })?;
    if packed.len() <= NONCE_LEN {
        return Err(AppError::localized(
            "secrets.ciphertext_too_short",
            "密文过短",
            "Ciphertext too short",
        ));
    }
    let (nonce_bytes, ciphertext) = packed.split_at(NONCE_LEN);
    let cipher = cipher()?;
    let plaintext = cipher
        .decrypt(Nonce::from_slice(nonce_bytes), ciphertext)
        .map_err(|_| {
            AppError::localized(
                "secrets.decrypt_failed",
                "解密失败：本地密钥可能不匹配或数据已损坏",
                "Decryption failed: local key mismatch or corrupted data",
            )
        })?;
    String::from_utf8(plaintext).map_err(|e| {
        AppError::localized(
            "secrets.utf8_failed",
            format!("解密结果不是合法 UTF-8: {e}"),
            format!("Decrypted data is not valid UTF-8: {e}"),
        )
    })
}

/// Encrypt a UTF-8 blob for DB storage. Idempotent for current + legacy formats.
pub fn encrypt_blob(value: &str) -> Result<String, AppError> {
    if value.starts_with(PREFIX) {
        // Keep a clean current envelope as-is; migrate double-wrapped / legacy inners.
        match decrypt_current_layer(value) {
            Ok(inner)
                if !inner.starts_with(PREFIX) && !is_legacy_encrypted(&inner) =>
            {
                return Ok(value.to_string());
            }
            _ => {
                let plain = to_plaintext(value)?;
                return encrypt_with_current(&plain);
            }
        }
    }

    if is_legacy_encrypted(value) {
        let plain = decrypt_legacy(value)?;
        return encrypt_with_current(&plain);
    }

    encrypt_with_current(value)
}

/// Decrypt a blob. Values without a known envelope are returned unchanged.
pub fn decrypt_blob(data: &str) -> Result<String, AppError> {
    to_plaintext(data)
}

/// Encrypt all plaintext / legacy `providers.settings_config` rows in-place.
pub fn encrypt_provider_settings_in_conn(conn: &rusqlite::Connection) -> Result<usize, AppError> {
    let mut stmt = conn
        .prepare("SELECT rowid, settings_config FROM providers")
        .map_err(|e| AppError::Database(e.to_string()))?;
    let rows: Vec<(i64, String)> = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .map_err(|e| AppError::Database(e.to_string()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| AppError::Database(e.to_string()))?;

    let mut updated = 0usize;
    for (rowid, value) in rows {
        let encrypted = encrypt_blob(&value)?;
        if encrypted == value {
            continue;
        }
        conn.execute(
            "UPDATE providers SET settings_config = ?1 WHERE rowid = ?2",
            rusqlite::params![encrypted, rowid],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        updated += 1;
    }
    Ok(updated)
}

/// Decrypt all `providers.settings_config` rows in-place for sync export (portable plaintext).
pub fn decrypt_provider_settings_in_conn(conn: &rusqlite::Connection) -> Result<usize, AppError> {
    let mut stmt = conn
        .prepare("SELECT rowid, settings_config FROM providers")
        .map_err(|e| AppError::Database(e.to_string()))?;
    let rows: Vec<(i64, String)> = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .map_err(|e| AppError::Database(e.to_string()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| AppError::Database(e.to_string()))?;

    let mut updated = 0usize;
    for (rowid, value) in rows {
        if !value.starts_with(PREFIX) && !is_legacy_encrypted(&value) {
            continue;
        }
        let plain = decrypt_blob(&value)?;
        conn.execute(
            "UPDATE providers SET settings_config = ?1 WHERE rowid = ?2",
            rusqlite::params![plain, rowid],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        updated += 1;
    }
    Ok(updated)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use std::env;
    use tempfile::TempDir;

    struct TempHome {
        _dir: TempDir,
        original: Option<String>,
    }

    impl TempHome {
        fn new() -> Self {
            let dir = TempDir::new().unwrap();
            let original = env::var("CC_SWITCH_TEST_HOME").ok();
            env::set_var("CC_SWITCH_TEST_HOME", dir.path());
            Self {
                _dir: dir,
                original,
            }
        }
    }

    impl Drop for TempHome {
        fn drop(&mut self) {
            match &self.original {
                Some(v) => env::set_var("CC_SWITCH_TEST_HOME", v),
                None => env::remove_var("CC_SWITCH_TEST_HOME"),
            }
        }
    }

    #[test]
    #[serial]
    fn roundtrip_and_plaintext_passthrough() {
        let _home = TempHome::new();
        let plain = r#"{"env":{"ANTHROPIC_API_KEY":"sk-test"}}"#;
        let passthrough = decrypt_blob(plain).unwrap();
        assert_eq!(passthrough, plain);

        if CIPHER.get().is_none() {
            let enc = encrypt_blob(plain).unwrap();
            assert!(enc.starts_with(PREFIX));
            assert_eq!(decrypt_blob(&enc).unwrap(), plain);
            assert_eq!(encrypt_blob(&enc).unwrap(), enc);
        }
    }

    #[test]
    fn legacy_detection_rejects_plain_json() {
        assert!(!is_legacy_encrypted(r#"{"a":1}"#));
        assert!(!is_legacy_encrypted(""));
    }
}
