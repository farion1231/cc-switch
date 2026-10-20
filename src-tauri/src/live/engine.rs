//! 写入引擎：CC Switch 改客户端文件的唯一底层路径。
//!
//! 一次写入分几步：读字节并记下 hash → 按格式解析，解析失败就停 → 在内存里改关键字段
//! → 序列化成新字节（[`plan`]）→ 写好临时文件（[`stage`]）→ 发布前对这个文件做一次
//! 字节级备份（每个文件只做一次）→ 最后重读一次比对 hash，没变才 rename，变了就以
//! 新内容为底重算。多文件操作的写前意图和崩溃恢复在 `mode::operation`。
//!
//! 冲突检测是乐观的：CC Switch 自己的写入方共用 [`lock_app`] 这把锁，彼此不会覆盖；
//! Claude Code 这类外部进程不参与这把锁，「重读比对」和 rename 之间仍有一个极小的
//! 窗口，外部写入恰好落在里面时仍会被覆盖。

use std::collections::HashSet;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::{Condvar, Mutex, OnceLock};

use sha2::{Digest, Sha256};

use crate::config::{atomic_write_private, stage_write, StagedWrite};
use crate::error::AppError;

use super::patch::{LivePatch, LiveWriteError};

/// 这台设备自己的状态目录：`live-state.json` 和首写备份都放在这里。
///
/// 路径固定为 `get_home_dir()/.cc-switch`，和 `settings.json` 一样不跟随配置目录
/// 覆盖：覆盖目录可能指向网盘同步目录，而写前意图和备份都是这台设备的事实。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceStore {
    root: PathBuf,
}

impl DeviceStore {
    pub fn for_device() -> Self {
        Self::at(crate::config::get_home_dir().join(".cc-switch"))
    }

    pub fn at(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn state_path(&self) -> PathBuf {
        self.root.join("live-state.json")
    }

    /// 这台设备状态目录下的一个文件（比如 Codex 的登录暂存）。
    pub fn file(&self, name: &str) -> PathBuf {
        self.root.join(name)
    }

    pub fn first_write_backup_dir(&self) -> PathBuf {
        self.root.join("backups").join("live-first-write")
    }
}

/// 一个受引擎管理的客户端文件。`private` 为真时按 0600 写（文件里有 Key）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveFile {
    pub path: PathBuf,
    pub private: bool,
}

impl LiveFile {
    pub fn private(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            private: true,
        }
    }

    pub fn shared(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            private: false,
        }
    }
}

/// 文件内容的 hash；文件不存在是 `None`。
pub fn digest(bytes: Option<&[u8]>) -> Option<String> {
    bytes.map(sha256_hex)
}

/// 十六进制的 SHA-256。
pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// 读当前字节；文件不存在返回 `None`，其他读取错误照报。
pub fn read_current(path: &Path) -> Result<Option<Vec<u8>>, LiveWriteError> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(None),
        Err(source) => Err(LiveWriteError::Io {
            path: path.to_path_buf(),
            source,
        }),
    }
}

/// 在内存里算好的一次写入。
#[derive(Debug, Clone)]
pub struct Planned {
    pub file: LiveFile,
    /// 写前内容的 hash，`None` 表示文件不存在。
    pub pre: Option<String>,
    pre_bytes: Option<Vec<u8>>,
    /// 写后内容的 hash；`None` 表示删掉这个文件。
    pub planned: Option<String>,
    bytes: Option<Vec<u8>>,
}

impl Planned {
    pub fn is_noop(&self) -> bool {
        self.pre == self.planned
    }

    pub fn pre_bytes(&self) -> Option<&[u8]> {
        self.pre_bytes.as_deref()
    }
}

/// 读当前内容并在内存里算出新内容；什么都不写。
pub fn plan(file: &LiveFile, patch: &dyn LivePatch) -> Result<Planned, LiveWriteError> {
    let pre_bytes = read_current(&file.path)?;
    plan_from(file, patch, pre_bytes)
}

pub(crate) fn plan_from(
    file: &LiveFile,
    patch: &dyn LivePatch,
    pre_bytes: Option<Vec<u8>>,
) -> Result<Planned, LiveWriteError> {
    let bytes = patch.apply_file(&file.path, pre_bytes.as_deref())?;
    Ok(Planned {
        file: file.clone(),
        pre: digest(pre_bytes.as_deref()),
        pre_bytes,
        planned: digest(bytes.as_deref()),
        bytes,
    })
}

/// 把新内容写进目标旁边的临时文件（fsync 过，崩溃后可以靠它前滚）。要删文件时没有
/// 临时文件，返回 `None`。
pub fn stage(planned: &Planned) -> Result<Option<StagedWrite>, AppError> {
    let Some(bytes) = planned.bytes.as_deref() else {
        return Ok(None);
    };
    let mode = planned.file.private.then_some(0o600);
    stage_write(&planned.file.path, bytes, mode, true).map(Some)
}

/// 这个文件第一次经引擎写入前，留一份原文件的字节级备份。
///
/// 给第一次用新版的用户一道保险：万一引擎有 bug，还能找回升级前的原文件。以文件的
/// 绝对路径为键，每个文件只备份一次；备份在 `<device>/backups/live-first-write/`，
/// 旁边的 `.source` 记着原路径。写入前文件不存在时只记 `.source`，以后也不再备份
/// （那时的内容是 CC Switch 自己写的）。
pub fn ensure_first_write_backup(
    store: &DeviceStore,
    path: &Path,
    current: Option<&[u8]>,
) -> Result<(), AppError> {
    let dir = store.first_write_backup_dir();
    let key = digest(Some(path.to_string_lossy().as_bytes())).expect("digest");
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file".to_string());
    let backup = dir.join(format!("{}-{name}", &key[..12]));
    let marker = dir.join(format!("{}-{name}.source", &key[..12]));
    if marker.exists() {
        return Ok(());
    }
    if let Some(bytes) = current {
        atomic_write_private(&backup, bytes)?;
    }
    atomic_write_private(&marker, path.to_string_lossy().as_bytes())
}

/// 一个应用的写锁：CC Switch 里改这个应用客户端文件的所有写入方（切换、编辑器、
/// 模式操作、托盘）都要先拿到它。不可重入。
#[derive(Debug)]
pub struct AppWriteGuard {
    app: String,
}

impl AppWriteGuard {
    pub fn app(&self) -> &str {
        &self.app
    }
}

fn lock_table() -> &'static (Mutex<HashSet<String>>, Condvar) {
    static LOCKS: OnceLock<(Mutex<HashSet<String>>, Condvar)> = OnceLock::new();
    LOCKS.get_or_init(|| (Mutex::new(HashSet::new()), Condvar::new()))
}

pub fn lock_app(app: &str) -> AppWriteGuard {
    let (held, released) = lock_table();
    let mut held = held.lock().unwrap_or_else(|e| e.into_inner());
    while held.contains(app) {
        held = released.wait(held).unwrap_or_else(|e| e.into_inner());
    }
    held.insert(app.to_string());
    AppWriteGuard {
        app: app.to_string(),
    }
}

impl Drop for AppWriteGuard {
    fn drop(&mut self) {
        let (held, released) = lock_table();
        held.lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&self.app);
        released.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::live::patch::json::JsonPatch;
    use crate::live::patch::KeyPath;
    use serde_json::json;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    fn set_patch(key: &str, value: &str) -> JsonPatch {
        JsonPatch {
            set: vec![(KeyPath::new(&[key]), json!(value))],
            ..JsonPatch::default()
        }
    }

    #[test]
    fn plan_writes_nothing_and_refuses_broken_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        fs::write(&path, "{ broken").unwrap();
        let before = fs::metadata(&path).unwrap().modified().unwrap();

        let err = plan(&LiveFile::shared(&path), &set_patch("a", "b")).expect_err("refused");
        assert!(matches!(err, LiveWriteError::Parse { .. }));
        assert_eq!(fs::read_to_string(&path).unwrap(), "{ broken");
        assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), before);
        assert_eq!(
            fs::read_dir(dir.path()).unwrap().count(),
            1,
            "no temp files"
        );
    }

    #[test]
    fn plan_reports_noops() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        fs::write(&path, "{\n  \"a\": \"b\"\n}").unwrap();
        let planned = plan(&LiveFile::shared(&path), &set_patch("a", "b")).unwrap();
        assert!(planned.is_noop());
        let planned = plan(&LiveFile::shared(&path), &set_patch("a", "c")).unwrap();
        assert!(!planned.is_noop());
    }

    #[cfg(unix)]
    #[test]
    fn private_files_are_staged_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let planned = plan(&LiveFile::private(&path), &set_patch("a", "b")).unwrap();
        stage(&planned).unwrap().unwrap().commit().unwrap();
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[test]
    fn first_write_backup_keeps_the_original_once() {
        let dir = tempfile::tempdir().unwrap();
        let store = DeviceStore::at(dir.path().join("device"));
        let path = dir.path().join("settings.json");

        ensure_first_write_backup(&store, &path, Some(b"original")).unwrap();
        ensure_first_write_backup(&store, &path, Some(b"later")).unwrap();

        let backups: Vec<_> = fs::read_dir(store.first_write_backup_dir())
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        assert_eq!(backups.len(), 2, "{backups:?}");
        let backup = backups
            .iter()
            .find(|p| !p.to_string_lossy().ends_with(".source"))
            .unwrap();
        assert_eq!(fs::read(backup).unwrap(), b"original");
    }

    #[test]
    fn app_lock_serializes_writers_of_the_same_app() {
        let guard = lock_app("lock-test-app");
        let entered = Arc::new(AtomicBool::new(false));
        let flag = entered.clone();
        let waiter = std::thread::spawn(move || {
            let _other = lock_app("lock-test-app");
            flag.store(true, Ordering::SeqCst);
        });
        let _unrelated = lock_app("lock-test-other-app");
        std::thread::sleep(Duration::from_millis(50));
        assert!(!entered.load(Ordering::SeqCst), "second writer must wait");
        drop(guard);
        waiter.join().unwrap();
        assert!(entered.load(Ordering::SeqCst));
    }
}
