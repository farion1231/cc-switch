//! Claude Code 那边的文件：它的锁、`.claude.json` 的账号资料，以及账号槽位的备份目录。

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{Map, Value};

use super::store::Slot;
use super::ACCOUNT_CACHE_KEYS;
use crate::error::AppError;
use crate::services::claude_account::cc_lock::{CcLock, LockSpec};

// Claude Code 的锁（proper-lockfile 目录锁）。stale 照抄它的设定，wait 是我们愿意等的时长。
const REFRESH_LOCK: LockSpec = LockSpec {
    stale: Duration::from_secs(60),
    wait: Duration::from_secs(10),
};
const STORAGE_LOCK: LockSpec = LockSpec {
    stale: Duration::from_secs(15),
    wait: Duration::from_secs(10),
};
const CONFIG_LOCK: LockSpec = LockSpec {
    stale: Duration::from_secs(10),
    wait: Duration::from_secs(8),
};

pub(super) struct RefreshLocks {
    _a: CcLock,
    _b: CcLock,
}

/// Claude Code 刷新令牌时拿的两把锁：`<目录>/.oauth_refresh.lock` 和 `<realpath(目录)>.lock`。
/// 不可重入：同一个槽位别在外层已经拿着时再拿。
pub(super) fn refresh_locks(slot: &Slot) -> Result<RefreshLocks, AppError> {
    let dir = slot.secure_dir();
    let a = CcLock::acquire(&dir.join(".oauth_refresh.lock"), REFRESH_LOCK)?;
    let real = std::fs::canonicalize(&dir).unwrap_or(dir);
    let mut legacy = real.into_os_string();
    legacy.push(".lock");
    let b = CcLock::acquire(Path::new(&legacy), REFRESH_LOCK)?;
    Ok(RefreshLocks { _a: a, _b: b })
}

/// Claude Code 写凭据时的串行锁。
pub(super) fn storage_lock(slot: &Slot) -> Result<CcLock, AppError> {
    CcLock::acquire(&slot.secure_dir().join(".storage-write.lock"), STORAGE_LOCK)
}

pub(super) fn read_profile(path: &Path) -> Option<Value> {
    let bytes = std::fs::read(path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// 在 Claude Code 的配置锁里改 `.claude.json`：换 `oauthAccount`（`None` 是删掉）、删账号
/// 缓存，其余不碰。
pub(super) fn write_profile(path: &Path, account: Option<&Value>) -> Result<(), AppError> {
    let mut lock_path = path.as_os_str().to_owned();
    lock_path.push(".lock");
    let _lock = CcLock::acquire(Path::new(&lock_path), CONFIG_LOCK)?;
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => "{}".to_string(),
        Err(source) => return Err(AppError::io(path, source)),
    };
    let mut doc: Map<String, Value> = match serde_json::from_str(&text) {
        Ok(Value::Object(map)) => map,
        _ => {
            return Err(AppError::Config(format!(
                "{} 不是 JSON 对象，没有改动",
                path.display()
            )))
        }
    };
    match account {
        Some(account) => {
            doc.insert("oauthAccount".to_string(), account.clone());
        }
        None => {
            doc.remove("oauthAccount");
        }
    }
    for key in ACCOUNT_CACHE_KEYS {
        doc.remove(*key);
    }
    doc.entry("hasCompletedOnboarding")
        .or_insert(Value::Bool(true));
    let bytes = serde_json::to_vec_pretty(&Value::Object(doc))
        .map_err(|source| AppError::JsonSerialize { source })?;
    crate::config::atomic_write_private(path, &bytes)
}

/// 账号目录 `dir` 的备份槽位目录（与它并列的 `<名字>.prev`）。删除账号时一起删。
pub fn backup_dir(dir: &Path) -> PathBuf {
    let mut name = dir.file_name().unwrap_or_default().to_os_string();
    name.push(".prev");
    dir.with_file_name(name)
}

pub(super) fn backup_slot(dir: &Path) -> Slot {
    Slot::Account(backup_dir(dir))
}
