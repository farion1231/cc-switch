//! Git 托管站点访问凭据。
//!
//! 分两层存放：**元数据**（id / host / 名称 / 时间）落 SQLite 表 `git_credentials`，
//! **token 本体**只进系统密钥链，条目名 `git:{host}:{id}`。元数据落库是为了能列表、
//! 能排序、能在删除时连带清掉引用它的仓库；密钥材料则始终不落库、不进导出与
//! WebDAV/S3 同步、不进日志。
//!
//! 下载路径通过 [`crate::services::skill::RepoAuth`] 显式拿到 token，不去读环境变量
//! 或全局状态——上游 PR #6924 正是把 `GITHUB_TOKEN` 隐式接进通用下载函数而被拒的：
//! 桌面应用从启动环境继承的 token 可能是用户给别的工具或 CI 用的高权限凭据。

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};

use crate::error::format_skill_error;

/// 密钥链里的 service 名。
const KEYRING_SERVICE: &str = "cc-switch";

/// 目前只支持 GitHub。凭据按 host 分条存放，为将来接入其他托管站点留位。
pub const GITHUB_HOST: &str = "github.com";

/// token 长度上限。GitHub 的 PAT 远短于此，超过基本可判定是粘错了内容。
const MAX_SECRET_LEN: usize = 512;

/// 一枚凭据的元数据。**不含 token 本体**——本体只存在密钥链里，也从不回传前端。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitCredential {
    pub id: String,
    pub host: String,
    /// 用户起的名字，用于在下拉框里区分多枚令牌。
    pub name: String,
    #[serde(default)]
    pub created_at: i64,
    #[serde(default)]
    pub updated_at: i64,
}

impl GitCredential {
    /// 新建一枚凭据的元数据（时间戳由调用方给出，便于测试）。
    pub fn new(id: &str, host: &str, name: &str, now: i64) -> Self {
        Self {
            id: id.to_string(),
            host: host.to_string(),
            name: name.trim().to_string(),
            created_at: now,
            updated_at: now,
        }
    }
}

/// 校验新建凭据的 token。
///
/// token 只接受单个可打印 ASCII 串：带换行或其它控制字符的值会被 reqwest 的 header
/// 校验拒绝，在这里拦下能给出可行动的提示，而不是把 header 解析错误原样抛给用户。
/// 前导/尾随空白先裁掉——从终端或网页复制时很容易带上。
///
/// 名称不在这里校验：它只是下拉框里的标签，必填与长度由对话框把关。两件事塞进
/// 同一个错误码，只会让"名字太长"看起来像"令牌格式不对"。
pub fn validate_new_credential(secret: &str) -> Result<()> {
    let secret = secret.trim();
    let secret_ok = !secret.is_empty()
        && secret.len() <= MAX_SECRET_LEN
        && secret.bytes().all(|byte| byte.is_ascii_graphic());
    if secret_ok {
        return Ok(());
    }
    Err(anyhow!(format_skill_error(
        "CREDENTIAL_INVALID",
        &[],
        Some("reenterCredential"),
    )))
}

/// token 本体的读写接口。
///
/// 抽成 trait 是为了让不依赖系统密钥链的测试（含 CI）能注入内存实现。
pub trait SecretStore: Send + Sync {
    /// 未配置（或已被删除）时返回 `Ok(None)`，而不是错误。
    fn get(&self, host: &str, id: &str) -> Result<Option<String>>;
    fn set(&self, host: &str, id: &str, secret: &str) -> Result<()>;
    /// 删除不存在的条目视为成功——"撤销"要能被重复执行。
    fn delete(&self, host: &str, id: &str) -> Result<()>;
}

/// 系统密钥链实现：Windows Credential Manager / macOS Keychain / Linux Secret Service。
pub struct KeyringSecretStore;

impl KeyringSecretStore {
    /// 条目名带 `git:` 前缀，避免与将来其它用途的条目撞名。
    fn account(host: &str, id: &str) -> String {
        format!("git:{host}:{id}")
    }

    fn entry(host: &str, id: &str) -> Result<keyring::Entry> {
        keyring::Entry::new(KEYRING_SERVICE, &Self::account(host, id))
            .map_err(|error| keyring_unavailable(id, &error))
    }
}

impl SecretStore for KeyringSecretStore {
    fn get(&self, host: &str, id: &str) -> Result<Option<String>> {
        match Self::entry(host, id)?.get_password() {
            Ok(secret) => Ok(Some(secret)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(keyring_unavailable(id, &error)),
        }
    }

    fn set(&self, host: &str, id: &str, secret: &str) -> Result<()> {
        Self::entry(host, id)?
            .set_password(secret.trim())
            .map_err(|error| keyring_unavailable(id, &error))
    }

    fn delete(&self, host: &str, id: &str) -> Result<()> {
        match Self::entry(host, id)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(keyring_unavailable(id, &error)),
        }
    }
}

/// 密钥链不可用时给出可行动的报错。
///
/// 不降级到明文存储：维护者的验收清单要求"系统密钥链保存"，静默落库会把一枚可
/// 访问私有仓库的凭据变成可被云同步带走的明文。底层错误只进日志——它不含 token，
/// 但可能带平台路径等细节，不适合直接抛给用户。
fn keyring_unavailable(id: &str, error: &keyring::Error) -> anyhow::Error {
    log::warn!("系统密钥链不可用（凭据 {id}）: {error}");
    anyhow!(format_skill_error(
        "KEYRING_UNAVAILABLE",
        &[("host", GITHUB_HOST)],
        Some("checkKeyring"),
    ))
}

#[cfg(test)]
pub mod testing {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    /// 不依赖系统密钥链的实现，供单测与 CI 使用。
    #[derive(Default)]
    pub struct InMemorySecretStore {
        entries: Mutex<HashMap<String, String>>,
        /// 置位后所有操作都报"密钥链不可用"，用来覆盖降级路径。
        pub unavailable: bool,
        /// 删除总是失败。用来验证"密钥链删不掉时，元数据必须还在"这条顺序约束。
        pub delete_fails: bool,
    }

    impl InMemorySecretStore {
        pub fn new() -> Self {
            Self::default()
        }

        pub fn unavailable() -> Self {
            Self {
                entries: Mutex::new(HashMap::new()),
                unavailable: true,
                delete_fails: false,
            }
        }

        fn key(host: &str, id: &str) -> String {
            format!("{host}:{id}")
        }
    }

    impl SecretStore for InMemorySecretStore {
        fn get(&self, host: &str, id: &str) -> Result<Option<String>> {
            if self.unavailable {
                return Err(keyring_unavailable(id, &keyring::Error::NoEntry));
            }
            Ok(self
                .entries
                .lock()
                .unwrap()
                .get(&Self::key(host, id))
                .cloned())
        }

        fn set(&self, host: &str, id: &str, secret: &str) -> Result<()> {
            if self.unavailable {
                return Err(keyring_unavailable(id, &keyring::Error::NoEntry));
            }
            self.entries
                .lock()
                .unwrap()
                .insert(Self::key(host, id), secret.to_string());
            Ok(())
        }

        fn delete(&self, host: &str, id: &str) -> Result<()> {
            if self.unavailable || self.delete_fails {
                return Err(keyring_unavailable(id, &keyring::Error::NoEntry));
            }
            self.entries.lock().unwrap().remove(&Self::key(host, id));
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::testing::InMemorySecretStore;
    use super::*;

    #[test]
    fn accepts_a_realistic_credential() {
        assert!(validate_new_credential("github_pat_11ABCDEFG0").is_ok());
        // 从终端或网页复制时常带上的空白由校验容忍
        assert!(validate_new_credential("  ghp_abc123\n").is_ok());
    }

    #[test]
    fn rejects_tokens_that_would_break_a_request_header() {
        // 内嵌换行是 header 注入的形状：必须在进 reqwest 之前拦下
        for secret in [
            "",
            "   ",
            "ghp_abc

X-Injected: 1",
            "ghp_abc def",
            "ghp_中文",
        ] {
            let error = validate_new_credential(secret).unwrap_err();
            let parsed: serde_json::Value = serde_json::from_str(&error.to_string()).unwrap();
            assert_eq!(parsed["code"], "CREDENTIAL_INVALID", "secret={secret:?}");
        }
        let oversized = "g".repeat(MAX_SECRET_LEN + 1);
        assert!(validate_new_credential(&oversized).is_err());
    }

    #[test]
    fn error_messages_never_carry_the_secret() {
        // 报错会被前端原样展示、也会进日志：token 不能出现在任何一条路径上
        // 内嵌换行会让校验失败，报错里也不能把整串 token 带出来
        let secret = "ghp_supersecrettoken\nX-Injected: 1";
        let error = validate_new_credential(secret).unwrap_err();
        assert!(!error.to_string().contains("ghp_supersecrettoken"));
    }

    #[test]
    fn in_memory_store_round_trips_and_deletes_idempotently() {
        let store = InMemorySecretStore::new();
        assert!(store.get(GITHUB_HOST, "c1").unwrap().is_none());

        store.set(GITHUB_HOST, "c1", "ghp_abc123").unwrap();
        assert_eq!(
            store.get(GITHUB_HOST, "c1").unwrap().as_deref(),
            Some("ghp_abc123")
        );
        // 不同凭据互不干扰
        assert!(store.get(GITHUB_HOST, "c2").unwrap().is_none());

        store.delete(GITHUB_HOST, "c1").unwrap();
        assert!(store.get(GITHUB_HOST, "c1").unwrap().is_none());
        // 撤销要能被重复执行
        store.delete(GITHUB_HOST, "c1").unwrap();
    }

    #[test]
    fn unavailable_store_reports_a_keyring_error() {
        let store = InMemorySecretStore::unavailable();
        let error = store.get(GITHUB_HOST, "c1").unwrap_err();
        let parsed: serde_json::Value = serde_json::from_str(&error.to_string()).unwrap();
        assert_eq!(parsed["code"], "KEYRING_UNAVAILABLE");
    }
}
