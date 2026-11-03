//! Claude Code 存登录的地方（「槽位」）：读、写、删整包凭据。
//!
//! - 默认槽位是 `~/.claude` 对应的那一个；账号槽位是 `<数据目录>/claude-accounts/<id>/config`。
//! - macOS：钥匙串 generic password，service 见 [`Slot::keychain_service`]，内容是凭据 JSON；
//! - 其他平台：槽位目录里的 `.credentials.json`（0600）。
//!
//! 凭据只在内存里经过（[`Secret`] 不实现可打印的 `Debug`），不落到 CC Switch 自己的任何文件。

use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use crate::error::AppError;
use crate::services::claude_account::layout;

/// 一个槽位。
#[derive(Clone, PartialEq, Eq)]
pub enum Slot {
    /// 用户的 `~/.claude`：不设 `CLAUDE_CONFIG_DIR` 时 Claude Code 用的那一个（`home` 是用户目录）
    Default { home: PathBuf },
    /// 某个账号目录（`CLAUDE_CONFIG_DIR` 的原字符串）
    Account(PathBuf),
}

impl std::fmt::Debug for Slot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Slot::Default { home } => write!(f, "Slot::Default({})", home.display()),
            Slot::Account(dir) => write!(f, "Slot::Account({})", dir.display()),
        }
    }
}

impl Slot {
    /// 本机用户的默认槽位。
    pub fn default_slot() -> Slot {
        Slot::Default {
            home: crate::config::get_home_dir(),
        }
    }

    /// 放凭据文件和锁的目录。
    pub fn secure_dir(&self) -> PathBuf {
        match self {
            Slot::Default { home } => home.join(".claude"),
            Slot::Account(dir) => dir.clone(),
        }
    }

    /// 这个槽位的 `.claude.json`（账号资料 `oauthAccount` 在这里）。和 Claude Code 一样：
    /// 目录里有非空的旧版 `.config.json` 时用它。
    pub fn global_config(&self) -> PathBuf {
        let legacy = self.secure_dir().join(".config.json");
        if std::fs::metadata(&legacy).is_ok_and(|meta| meta.is_file() && meta.len() > 0) {
            return legacy;
        }
        match self {
            Slot::Default { home } => home.join(".claude.json"),
            Slot::Account(dir) => dir.join(".claude.json"),
        }
    }

    pub fn credentials_file(&self) -> PathBuf {
        self.secure_dir().join(".credentials.json")
    }

    /// macOS 钥匙串的 service：默认槽位没有后缀，账号目录按 Claude Code 的规则加后缀。
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub fn keychain_service(&self) -> String {
        match self {
            Slot::Default { .. } => "Claude Code-credentials".to_string(),
            Slot::Account(dir) => layout::keychain_service(dir),
        }
    }
}

/// 整包凭据。不能打印，比较用 [`Secret::fingerprint`]。
#[derive(Clone, PartialEq)]
pub struct Secret(pub Map<String, Value>);

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let keys: Vec<&String> = self.0.keys().collect();
        write!(f, "Secret(keys={keys:?})")
    }
}

impl Secret {
    pub fn parse(bytes: &[u8]) -> Result<Secret, AppError> {
        match serde_json::from_slice::<Value>(bytes) {
            Ok(Value::Object(map)) => Ok(Secret(map)),
            // 不把解析错误原文带出去：里面可能夹着内容片段
            _ => Err(AppError::localized(
                "claude.snapshot.unreadable",
                "Claude Code 的登录信息格式无法识别，没有做任何改动",
                "Claude Code's sign-in data has an unrecognized format. Nothing was changed.",
            )),
        }
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(&Value::Object(self.0.clone())).unwrap_or_default()
    }

    /// 内容的 sha256（只用来比较前后是不是同一份，单向、不可还原）。
    pub fn fingerprint(&self) -> String {
        use sha2::{Digest, Sha256};
        let digest = Sha256::digest(self.to_bytes());
        digest.iter().map(|byte| format!("{byte:02x}")).collect()
    }
}

/// 读、写、删一个槽位的整包凭据。测试里换成内存实现。
pub trait CredentialStore: Send + Sync {
    /// 没有登录时为 `None`。
    fn read(&self, slot: &Slot) -> Result<Option<Secret>, AppError>;
    fn write(&self, slot: &Slot, secret: &Secret) -> Result<(), AppError>;
    /// 不在时也算成功。
    fn delete(&self, slot: &Slot) -> Result<(), AppError>;
}

/// 本平台 Claude Code 用的那种存法。
pub fn platform_store() -> Box<dyn CredentialStore> {
    #[cfg(target_os = "macos")]
    {
        Box::new(keychain::KeychainStore)
    }
    #[cfg(not(target_os = "macos"))]
    {
        Box::new(FileStore)
    }
}

/// `.credentials.json`（Linux / Windows；macOS 上是钥匙串失败时的后备）。
pub struct FileStore;

impl CredentialStore for FileStore {
    fn read(&self, slot: &Slot) -> Result<Option<Secret>, AppError> {
        read_file(&slot.credentials_file())
    }

    fn write(&self, slot: &Slot, secret: &Secret) -> Result<(), AppError> {
        let path = slot.credentials_file();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| AppError::io(parent, source))?;
        }
        crate::config::atomic_write_private(&path, &secret.to_bytes())
    }

    fn delete(&self, slot: &Slot) -> Result<(), AppError> {
        let path = slot.credentials_file();
        match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(source) => Err(AppError::io(&path, source)),
        }
    }
}

fn read_file(path: &Path) -> Result<Option<Secret>, AppError> {
    match std::fs::read(path) {
        Ok(bytes) if bytes.iter().all(u8::is_ascii_whitespace) => Ok(None),
        Ok(bytes) => Secret::parse(&bytes).map(Some),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(AppError::io(path, source)),
    }
}

#[cfg(target_os = "macos")]
mod keychain;

#[cfg(test)]
pub mod memory {
    //! 测试用的内存槽位。
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    #[derive(Default)]
    pub struct MemoryStore {
        pub slots: Mutex<HashMap<String, Secret>>,
    }

    fn key(slot: &Slot) -> String {
        format!("{slot:?}")
    }

    impl MemoryStore {
        pub fn get(&self, slot: &Slot) -> Option<Secret> {
            self.slots.lock().unwrap().get(&key(slot)).cloned()
        }
        pub fn put(&self, slot: &Slot, secret: Secret) {
            self.slots.lock().unwrap().insert(key(slot), secret);
        }
    }

    impl CredentialStore for MemoryStore {
        fn read(&self, slot: &Slot) -> Result<Option<Secret>, AppError> {
            Ok(self.get(slot))
        }
        fn write(&self, slot: &Slot, secret: &Secret) -> Result<(), AppError> {
            self.put(slot, secret.clone());
            Ok(())
        }
        fn delete(&self, slot: &Slot) -> Result<(), AppError> {
            self.slots.lock().unwrap().remove(&key(slot));
            Ok(())
        }
    }
}
