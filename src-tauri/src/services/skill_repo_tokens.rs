//! 技能仓库的访问令牌：按主机存，只在本机，不随云同步（#9666）。
//!
//! 哪些主机有令牌记在 `~/.cc-switch/skill-repo-tokens.json`（0600，云同步只带 `db.sql`
//! 和 `skills.zip`，不带它）。令牌本身在 macOS 存钥匙串、Windows 存凭据管理器；其他平台
//! 没有处处可用的钥匙串，直接写在这个文件里。下载时没登记的主机不碰钥匙串，公开仓库的
//! 行为和以前一样。
//!
//! 按主机而不是按仓库存：`skill_repos` 随同步整表覆盖、主键不含主机，同步来的一行可以把
//! 某个 owner/name 的主机改掉。令牌按主机取，就只会发给录入它时填的那台主机。

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};

use crate::error::format_skill_error;
use crate::services::skill::{SkillService, GITHUB_HOST};
use crate::sync_utils::MutexExt;

const INDEX_FILE: &str = "skill-repo-tokens.json";
/// 钥匙串条目的 service（macOS）/ 目标名前缀（Windows），account 是主机名
#[cfg_attr(not(any(target_os = "macos", windows)), allow(dead_code))]
const KEYCHAIN_SERVICE: &str = "cc-switch-skill-repo";
/// GitHub、GitLab、Gitea 的令牌都在百字以内，留足余量；也远小于 Windows 凭据 2560 字节的上限
const MAX_TOKEN_LEN: usize = 1024;

#[derive(Debug, Default, Serialize, Deserialize)]
struct TokenIndex {
    #[serde(default)]
    hosts: BTreeMap<String, HostEntry>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
struct HostEntry {
    /// 只在没有系统钥匙串的平台上有值
    #[serde(default, skip_serializing_if = "Option::is_none")]
    token: Option<String>,
}

/// 系统钥匙串里的那一条：按主机读、写、删。测试换成内存实现，别碰真实钥匙串。
pub(crate) trait SecretBackend: Send + Sync {
    fn read(&self, host: &str) -> Result<Option<String>>;
    fn write(&self, host: &str, token: &str) -> Result<()>;
    /// 条目不存在也算成功
    fn delete(&self, host: &str) -> Result<()>;
}

/// 给界面看的清单：只有主机，没有令牌。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RepoTokenList {
    pub hosts: Vec<String>,
    /// 令牌存在系统钥匙串里；为 false 时存在本机仅自己可读的文件里
    pub secure_storage: bool,
}

pub struct RepoTokenStore {
    index_path: PathBuf,
    /// `None`：令牌直接写在清单文件里
    secrets: Option<Box<dyn SecretBackend>>,
    /// 读改写清单要串行，两次保存别互相覆盖
    lock: Mutex<()>,
}

impl RepoTokenStore {
    /// 本机的那一份：清单在 `~/.cc-switch/` 下（同 settings.json，不受数据目录覆盖影响）。
    pub fn system() -> Self {
        let index_path = crate::config::get_home_dir()
            .join(".cc-switch")
            .join(INDEX_FILE);
        Self::new(index_path, system_backend())
    }

    pub(crate) fn new(index_path: PathBuf, secrets: Option<Box<dyn SecretBackend>>) -> Self {
        Self {
            index_path,
            secrets,
            lock: Mutex::new(()),
        }
    }

    pub fn list(&self) -> Result<RepoTokenList> {
        let _guard = self.lock.lock_recover();
        let index = self.read_index()?;
        Ok(RepoTokenList {
            hosts: index.hosts.into_keys().collect(),
            secure_storage: self.secrets.is_some(),
        })
    }

    /// 下载时用：这台主机的令牌，没设置就是 `None`。
    ///
    /// 清单读不出来只记日志、当作没有令牌：公开仓库不能因为它下载失败。登记过的主机
    /// 钥匙串却读不出来时报错，否则私有仓库会报成「仓库不存在」，把人引到错的方向。
    pub fn get(&self, host: &str) -> Result<Option<String>> {
        let _guard = self.lock.lock_recover();
        let index = match self.read_index() {
            Ok(index) => index,
            Err(e) => {
                log::warn!("读取技能仓库令牌清单失败，按没有令牌处理: {e}");
                return Ok(None);
            }
        };
        let Some(entry) = index.hosts.get(host) else {
            return Ok(None);
        };
        if let Some(token) = &entry.token {
            return Ok(Some(token.clone()));
        }
        let Some(secrets) = &self.secrets else {
            return Ok(None);
        };
        match secrets.read(host) {
            Ok(Some(token)) => Ok(Some(token)),
            Ok(None) => {
                log::warn!("{host} 登记了访问令牌，钥匙串里却没有，按没有令牌处理");
                Ok(None)
            }
            Err(e) => {
                log::error!("读取 {host} 的访问令牌失败: {e}");
                Err(storage_error(host))
            }
        }
    }

    /// 保存（覆盖）一台主机的令牌，返回规范化后的主机名。
    pub fn set(&self, host: &str, token: &str) -> Result<String> {
        let host = normalize_host(host)?;
        let token = validate_token(token)?;
        let _guard = self.lock.lock_recover();
        let mut index = self.read_index().map_err(|e| {
            log::error!("读取技能仓库令牌清单失败: {e}");
            storage_error(&host)
        })?;
        // 先写令牌、再登记：中途失败最多在钥匙串里留一条没人引用的，下次保存会覆盖它
        let entry = match &self.secrets {
            Some(secrets) => {
                secrets.write(&host, token).map_err(|e| {
                    log::error!("写入 {host} 的访问令牌失败: {e}");
                    storage_error(&host)
                })?;
                HostEntry { token: None }
            }
            None => HostEntry {
                token: Some(token.to_string()),
            },
        };
        index.hosts.insert(host.clone(), entry);
        self.write_index(&index, &host)?;
        Ok(host)
    }

    /// 删除一台主机的令牌，返回规范化后的主机名。没有登记也照常清一遍钥匙串。
    pub fn remove(&self, host: &str) -> Result<String> {
        let host = normalize_host(host)?;
        let _guard = self.lock.lock_recover();
        let mut index = self.read_index().map_err(|e| {
            log::error!("读取技能仓库令牌清单失败: {e}");
            storage_error(&host)
        })?;
        // 先删令牌、再撤登记：删令牌失败时登记还在，界面上能看到、能重试
        if let Some(secrets) = &self.secrets {
            secrets.delete(&host).map_err(|e| {
                log::error!("删除 {host} 的访问令牌失败: {e}");
                storage_error(&host)
            })?;
        }
        if index.hosts.remove(&host).is_some() {
            self.write_index(&index, &host)?;
        }
        Ok(host)
    }

    fn read_index(&self) -> Result<TokenIndex> {
        match std::fs::read_to_string(&self.index_path) {
            Ok(text) => Ok(serde_json::from_str(&text)?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(TokenIndex::default()),
            Err(e) => Err(e.into()),
        }
    }

    fn write_index(&self, index: &TokenIndex, host: &str) -> Result<()> {
        let write = || -> Result<()> {
            if let Some(dir) = self.index_path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            let text = serde_json::to_string_pretty(index)?;
            crate::config::atomic_write_private(&self.index_path, text.as_bytes())?;
            Ok(())
        };
        write().map_err(|e| {
            log::error!("写入技能仓库令牌清单失败: {e}");
            storage_error(host)
        })
    }

    #[cfg(test)]
    pub(crate) fn index_path(&self) -> &std::path::Path {
        &self.index_path
    }
}

/// 主机名不分大小写，存小写；只认能当仓库主机的值（同 `skill_repos.host` 的校验）。
pub(crate) fn normalize_host(host: &str) -> Result<String> {
    let host = host.trim().to_ascii_lowercase();
    if host == GITHUB_HOST || SkillService::is_valid_gitlab_host(&host) {
        Ok(host)
    } else {
        Err(anyhow!(format_skill_error(
            "INVALID_TOKEN_HOST",
            &[("host", &host)],
            None,
        )))
    }
}

/// 令牌会进 HTTP 头，macOS 上还要放进 `security -i` 的一行命令里：只收各平台令牌实际
/// 用到的字符，引号、反斜杠、空白一律拒绝。前后的空白（粘贴带的换行）先去掉。
fn validate_token(token: &str) -> Result<&str> {
    let token = token.trim();
    let ok = !token.is_empty()
        && token.len() <= MAX_TOKEN_LEN
        && token.chars().all(|c| {
            c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '~' | '+' | '/' | '=')
        });
    if ok {
        Ok(token)
    } else {
        let limit = MAX_TOKEN_LEN.to_string();
        Err(anyhow!(format_skill_error(
            "INVALID_REPO_TOKEN",
            &[("limit", &limit)],
            None,
        )))
    }
}

fn storage_error(host: &str) -> anyhow::Error {
    anyhow!(format_skill_error(
        "REPO_TOKEN_STORAGE_FAILED",
        &[("host", host)],
        None,
    ))
}

#[cfg(any(target_os = "macos", windows))]
fn system_backend() -> Option<Box<dyn SecretBackend>> {
    Some(Box::new(SystemKeychain))
}

#[cfg(not(any(target_os = "macos", windows)))]
fn system_backend() -> Option<Box<dyn SecretBackend>> {
    None
}

#[cfg(any(target_os = "macos", windows))]
struct SystemKeychain;

#[cfg(target_os = "macos")]
impl SecretBackend for SystemKeychain {
    fn read(&self, host: &str) -> Result<Option<String>> {
        macos::read(KEYCHAIN_SERVICE, host)
    }
    fn write(&self, host: &str, token: &str) -> Result<()> {
        macos::write(KEYCHAIN_SERVICE, host, token)
    }
    fn delete(&self, host: &str) -> Result<()> {
        macos::delete(KEYCHAIN_SERVICE, host)
    }
}

#[cfg(windows)]
impl SecretBackend for SystemKeychain {
    fn read(&self, host: &str) -> Result<Option<String>> {
        wincred::read(&wincred::target(host))
    }
    fn write(&self, host: &str, token: &str) -> Result<()> {
        wincred::write(&wincred::target(host), host, token)
    }
    fn delete(&self, host: &str) -> Result<()> {
        wincred::delete(&wincred::target(host))
    }
}

/// 经 `/usr/bin/security` 读写：条目由它创建，访问控制里信任的也是它，以后读不会弹授权框
/// （同 `agy_account::keychain`）。主机名和令牌都已校验过字符集，可以放进引号里。
#[cfg(target_os = "macos")]
mod macos {
    use std::io::Write;
    use std::process::{Command, Stdio};

    use anyhow::{anyhow, Result};

    const SECURITY: &str = "/usr/bin/security";
    /// 找不到条目时的退出码（errSecItemNotFound）
    const NOT_FOUND_EXIT: i32 = 44;

    pub fn read(service: &str, account: &str) -> Result<Option<String>> {
        let output = Command::new(SECURITY)
            .args(["find-generic-password", "-s", service, "-a", account, "-w"])
            .stdin(Stdio::null())
            .output()?;
        if output.status.code() == Some(NOT_FOUND_EXIT) {
            return Ok(None);
        }
        if !output.status.success() {
            return Err(anyhow!("security 退出码 {:?}", output.status.code()));
        }
        let value = String::from_utf8(output.stdout)?;
        let value = value.trim_end_matches(['\r', '\n']);
        Ok((!value.is_empty()).then(|| value.to_string()))
    }

    /// `-U` 原地更新。令牌经 `security -i` 的标准输入传，不出现在进程参数里。
    pub fn write(service: &str, account: &str, value: &str) -> Result<()> {
        let line =
            format!("add-generic-password -U -s \"{service}\" -a \"{account}\" -w \"{value}\"\n");
        let mut child = Command::new(SECURITY)
            .arg("-i")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()?;
        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(line.as_bytes())?;
        }
        let output = child.wait_with_output()?;
        // `security -i` 出错也可能退出 0，错误只写在 stderr
        let stderr = String::from_utf8_lossy(&output.stderr);
        if !output.status.success() || !stderr.trim().is_empty() {
            return Err(anyhow!(
                "security 退出码 {:?}: {}",
                output.status.code(),
                stderr.trim()
            ));
        }
        Ok(())
    }

    pub fn delete(service: &str, account: &str) -> Result<()> {
        let output = Command::new(SECURITY)
            .args(["delete-generic-password", "-s", service, "-a", account])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .output()?;
        if output.status.success() || output.status.code() == Some(NOT_FOUND_EXIT) {
            Ok(())
        } else {
            Err(anyhow!("security 退出码 {:?}", output.status.code()))
        }
    }
}

/// Windows 凭据管理器：普通凭据，目标名 `cc-switch-skill-repo:<主机>`，用户名是主机。
#[cfg(windows)]
mod wincred {
    use std::ptr;

    use anyhow::{anyhow, Result};
    use windows_sys::Win32::Foundation::{GetLastError, ERROR_NOT_FOUND};
    use windows_sys::Win32::Security::Credentials::{
        CredDeleteW, CredFree, CredReadW, CredWriteW, CREDENTIALW, CRED_PERSIST_LOCAL_MACHINE,
        CRED_TYPE_GENERIC,
    };

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }

    pub fn target(host: &str) -> String {
        format!("{}:{host}", super::KEYCHAIN_SERVICE)
    }

    pub fn read(target: &str) -> Result<Option<String>> {
        let target = wide(target);
        let mut credential: *mut CREDENTIALW = ptr::null_mut();
        // SAFETY: target 以 0 结尾、在调用期间存活；成功时 credential 指向系统分配的结构，下面用完即 CredFree。
        let ok = unsafe { CredReadW(target.as_ptr(), CRED_TYPE_GENERIC, 0, &mut credential) };
        if ok == 0 {
            // SAFETY: 紧跟在失败的系统调用之后取错误码。
            let code = unsafe { GetLastError() };
            if code == ERROR_NOT_FOUND {
                return Ok(None);
            }
            return Err(anyhow!("CredReadW 失败（Windows 错误码 {code}）"));
        }
        // SAFETY: CredReadW 成功，credential 非空；blob 长度由系统给出，复制出来后再释放。
        let bytes = unsafe {
            let blob = &*credential;
            let bytes = if blob.CredentialBlob.is_null() || blob.CredentialBlobSize == 0 {
                Vec::new()
            } else {
                std::slice::from_raw_parts(blob.CredentialBlob, blob.CredentialBlobSize as usize)
                    .to_vec()
            };
            CredFree(credential as *const core::ffi::c_void);
            bytes
        };
        let value = String::from_utf8(bytes)?;
        Ok((!value.is_empty()).then_some(value))
    }

    pub fn write(target: &str, user: &str, value: &str) -> Result<()> {
        let blob = value.as_bytes();
        let mut target = wide(target);
        let mut user = wide(user);
        let credential = CREDENTIALW {
            Type: CRED_TYPE_GENERIC,
            TargetName: target.as_mut_ptr(),
            CredentialBlobSize: blob.len() as u32,
            CredentialBlob: blob.as_ptr() as *mut u8,
            Persist: CRED_PERSIST_LOCAL_MACHINE,
            UserName: user.as_mut_ptr(),
            ..Default::default()
        };
        // SAFETY: 结构里的指针都指向本函数里存活的缓冲区；CredWriteW 只读不写它们。
        let ok = unsafe { CredWriteW(&credential, 0) };
        if ok == 0 {
            // SAFETY: 紧跟在失败的系统调用之后取错误码。
            let code = unsafe { GetLastError() };
            return Err(anyhow!("CredWriteW 失败（Windows 错误码 {code}）"));
        }
        Ok(())
    }

    pub fn delete(target: &str) -> Result<()> {
        let target = wide(target);
        // SAFETY: target 以 0 结尾、在调用期间存活。
        let ok = unsafe { CredDeleteW(target.as_ptr(), CRED_TYPE_GENERIC, 0) };
        if ok == 0 {
            // SAFETY: 紧跟在失败的系统调用之后取错误码。
            let code = unsafe { GetLastError() };
            if code != ERROR_NOT_FOUND {
                return Err(anyhow!("CredDeleteW 失败（Windows 错误码 {code}）"));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Arc;

    /// 内存里的钥匙串；`fail` 置位后读写都报错，模拟钥匙串不可用。
    #[derive(Default, Clone)]
    pub(crate) struct MemorySecrets {
        pub(crate) entries: Arc<Mutex<HashMap<String, String>>>,
        pub(crate) fail: Arc<std::sync::atomic::AtomicBool>,
    }

    impl MemorySecrets {
        fn check(&self) -> Result<()> {
            if self.fail.load(std::sync::atomic::Ordering::SeqCst) {
                Err(anyhow!("钥匙串不可用"))
            } else {
                Ok(())
            }
        }
    }

    impl SecretBackend for MemorySecrets {
        fn read(&self, host: &str) -> Result<Option<String>> {
            self.check()?;
            Ok(self.entries.lock_recover().get(host).cloned())
        }
        fn write(&self, host: &str, token: &str) -> Result<()> {
            self.check()?;
            self.entries
                .lock_recover()
                .insert(host.to_string(), token.to_string());
            Ok(())
        }
        fn delete(&self, host: &str) -> Result<()> {
            self.check()?;
            self.entries.lock_recover().remove(host);
            Ok(())
        }
    }

    pub(crate) fn memory_store(dir: &std::path::Path) -> (RepoTokenStore, MemorySecrets) {
        let secrets = MemorySecrets::default();
        let store = RepoTokenStore::new(dir.join(INDEX_FILE), Some(Box::new(secrets.clone())));
        (store, secrets)
    }

    fn error_code(error: &anyhow::Error) -> String {
        serde_json::from_str::<serde_json::Value>(&error.to_string()).unwrap()["code"]
            .as_str()
            .unwrap()
            .to_string()
    }

    #[test]
    fn keychain_backend_keeps_the_token_out_of_the_index_file() {
        let dir = tempfile::tempdir().unwrap();
        let (store, secrets) = memory_store(dir.path());

        assert_eq!(store.get("gitlab.example.com").unwrap(), None);
        let host = store
            .set(" GitLab.Example.com:8443 ", "glpat-abc_DEF.123\n")
            .unwrap();
        assert_eq!(host, "gitlab.example.com:8443");
        assert_eq!(
            store.get("gitlab.example.com:8443").unwrap().as_deref(),
            Some("glpat-abc_DEF.123")
        );
        assert_eq!(
            store.list().unwrap(),
            RepoTokenList {
                hosts: vec!["gitlab.example.com:8443".into()],
                secure_storage: true,
            }
        );
        let index = std::fs::read_to_string(store.index_path()).unwrap();
        assert!(!index.contains("glpat"), "令牌不该落进清单文件: {index}");

        // 别的主机拿不到这枚令牌
        assert_eq!(store.get("gitlab.example.com").unwrap(), None);

        store.remove("gitlab.example.com:8443").unwrap();
        assert_eq!(store.get("gitlab.example.com:8443").unwrap(), None);
        assert!(store.list().unwrap().hosts.is_empty());
        assert!(secrets.entries.lock_recover().is_empty());
    }

    #[test]
    fn file_backend_stores_the_token_in_a_private_index() {
        let dir = tempfile::tempdir().unwrap();
        let store = RepoTokenStore::new(dir.path().join(INDEX_FILE), None);

        store.set("github.com", "ghp_abc").unwrap();
        assert_eq!(store.get("github.com").unwrap().as_deref(), Some("ghp_abc"));
        assert!(!store.list().unwrap().secure_storage);

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(store.index_path())
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }

        store.remove("github.com").unwrap();
        assert_eq!(store.get("github.com").unwrap(), None);
    }

    #[test]
    fn rejects_bad_hosts_and_tokens_without_writing() {
        let dir = tempfile::tempdir().unwrap();
        let (store, secrets) = memory_store(dir.path());

        for host in [
            "",
            "https://gitlab.example.com",
            "gitlab.example.com/group",
            "user@gitlab.example.com",
        ] {
            let error = store.set(host, "token").unwrap_err();
            assert_eq!(error_code(&error), "INVALID_TOKEN_HOST", "{host}");
        }
        for token in [
            "",
            "   ",
            "has space",
            "quote\"inside",
            "back\\slash",
            "line\nbreak",
            "全角",
        ] {
            let error = store.set("github.com", token).unwrap_err();
            assert_eq!(error_code(&error), "INVALID_REPO_TOKEN", "{token:?}");
        }
        let too_long = "a".repeat(MAX_TOKEN_LEN + 1);
        assert!(store.set("github.com", &too_long).is_err());
        assert!(store.set("github.com", &"a".repeat(MAX_TOKEN_LEN)).is_ok());

        store.remove("github.com").unwrap();
        assert!(secrets.entries.lock_recover().is_empty());
        assert!(store.list().unwrap().hosts.is_empty());
    }

    #[test]
    fn unreadable_index_means_no_token_but_blocks_writes() {
        let dir = tempfile::tempdir().unwrap();
        let (store, _secrets) = memory_store(dir.path());
        std::fs::write(store.index_path(), "{not json").unwrap();

        // 公开仓库照常下载
        assert_eq!(store.get("github.com").unwrap(), None);
        // 写不能把坏文件当成空清单覆盖掉
        let error = store.set("github.com", "ghp_abc").unwrap_err();
        assert_eq!(error_code(&error), "REPO_TOKEN_STORAGE_FAILED");
        assert_eq!(
            std::fs::read_to_string(store.index_path()).unwrap(),
            "{not json"
        );
    }

    #[test]
    fn keychain_failures_surface_only_for_registered_hosts() {
        let dir = tempfile::tempdir().unwrap();
        let (store, secrets) = memory_store(dir.path());
        store.set("gitlab.example.com", "glpat-abc").unwrap();
        secrets
            .fail
            .store(true, std::sync::atomic::Ordering::SeqCst);

        // 没登记的主机不碰钥匙串
        assert_eq!(store.get("github.com").unwrap(), None);
        let error = store.get("gitlab.example.com").unwrap_err();
        assert_eq!(error_code(&error), "REPO_TOKEN_STORAGE_FAILED");

        // 删令牌失败时登记保留，界面上还能看到、能重试
        assert!(store.remove("gitlab.example.com").is_err());
        assert_eq!(store.list().unwrap().hosts, vec!["gitlab.example.com"]);
    }
}
