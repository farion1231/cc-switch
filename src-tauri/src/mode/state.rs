//! `live-state.json`：这台设备上每个应用的客户端文件状态。
//!
//! 目前只有 `pending`：一次写客户端文件的操作在发布前写下的意图，按文件记录写前、
//! 写后的 hash 和已备好的临时文件，崩溃后据此前滚或丢弃（`mode::operation`）。
//! 双模式的 `mode`、`proxy_route`、`contract` 以后加在同一个对象里；不认识的字段
//! 读写时原样保留。
//!
//! 文件是设备本地的（0600，不同步），路径见 [`DeviceStore`]。

use std::collections::BTreeMap;
use std::fs;
use std::io::ErrorKind;
use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::config::stage_write;
use crate::error::AppError;
use crate::live::engine::DeviceStore;

pub const STATE_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LiveState {
    #[serde(default = "state_version")]
    pub version: u32,
    #[serde(default)]
    pub apps: BTreeMap<String, AppLiveState>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

fn state_version() -> u32 {
    STATE_VERSION
}

impl Default for LiveState {
    fn default() -> Self {
        Self {
            version: STATE_VERSION,
            apps: BTreeMap::new(),
            extra: Map::new(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AppLiveState {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending: Option<Pending>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl AppLiveState {
    fn is_empty(&self) -> bool {
        self.pending.is_none() && self.extra.is_empty()
    }
}

/// 操作名。用字符串而不是枚举，旧版本读到新版本写的操作名也能照常前滚或丢弃。
pub mod op {
    /// 切换供应商（会改指针）。
    pub const SWITCH: &str = "switch";
    /// 把当前供应商重新写进客户端文件（编辑、同步等）。
    pub const APPLY: &str = "apply";
}

/// 一次操作的写前意图。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Pending {
    pub op: String,
    pub files: Vec<PendingFile>,
    #[serde(default)]
    pub target: PendingTarget,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PendingFile {
    pub path: PathBuf,
    /// 写前内容的 hash；`None` 表示写前文件不存在。
    pub pre: Option<String>,
    /// 写后内容的 hash。
    pub planned: String,
    /// 已写好写后内容、等着 rename 的临时文件。
    pub staged: PathBuf,
}

/// 文件都写完之后要落定的状态。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PendingTarget {
    /// 直连指针：切换成功后当前供应商是谁。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pointer: Option<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl PendingTarget {
    pub fn is_empty(&self) -> bool {
        self.pointer.is_none() && self.extra.is_empty()
    }
}

/// 状态文件是所有应用共用的，读改写要串行。
fn state_lock() -> &'static Mutex<()> {
    static LOCK: Mutex<()> = Mutex::new(());
    &LOCK
}

/// 读状态文件。不存在时是空状态；内容坏了就挪到一旁（`live-state.json.corrupt-<时间>`）
/// 从空状态开始：这是 CC Switch 自己的文件，里面只有未完成操作的意图。
pub fn load(store: &DeviceStore) -> Result<LiveState, AppError> {
    let path = store.state_path();
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(LiveState::default()),
        Err(source) => return Err(AppError::io(&path, source)),
    };
    match serde_json::from_slice::<LiveState>(&bytes) {
        Ok(state) => Ok(state),
        Err(err) => {
            let stamp = chrono::Utc::now().format("%Y%m%dT%H%M%SZ");
            let aside = path.with_file_name(format!("live-state.json.corrupt-{stamp}"));
            log::warn!(
                "live-state.json 无法解析（{err}），已移到 {} 并从空状态开始",
                aside.display()
            );
            fs::rename(&path, &aside).map_err(|source| AppError::io(&path, source))?;
            Ok(LiveState::default())
        }
    }
}

fn save(store: &DeviceStore, state: &LiveState) -> Result<(), AppError> {
    let bytes =
        serde_json::to_vec_pretty(state).map_err(|source| AppError::JsonSerialize { source })?;
    stage_write(&store.state_path(), &bytes, Some(0o600), true)?.commit()
}

/// 读改写状态文件。
pub fn update<R>(
    store: &DeviceStore,
    change: impl FnOnce(&mut LiveState) -> R,
) -> Result<R, AppError> {
    let _guard = state_lock().lock().unwrap_or_else(|e| e.into_inner());
    let mut state = load(store)?;
    let result = change(&mut state);
    state.apps.retain(|_, app| !app.is_empty());
    save(store, &state)?;
    Ok(result)
}

pub fn pending(store: &DeviceStore, app: &str) -> Result<Option<Pending>, AppError> {
    let _guard = state_lock().lock().unwrap_or_else(|e| e.into_inner());
    Ok(load(store)?
        .apps
        .get(app)
        .and_then(|state| state.pending.clone()))
}

pub fn set_pending(
    store: &DeviceStore,
    app: &str,
    pending: Option<Pending>,
) -> Result<(), AppError> {
    update(store, |state| {
        state.apps.entry(app.to_string()).or_default().pending = pending;
    })
}

/// 有未完成操作的应用。
pub fn apps_with_pending(store: &DeviceStore) -> Result<Vec<String>, AppError> {
    let _guard = state_lock().lock().unwrap_or_else(|e| e.into_inner());
    Ok(load(store)?
        .apps
        .into_iter()
        .filter(|(_, state)| state.pending.is_some())
        .map(|(app, _)| app)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample_pending() -> Pending {
        Pending {
            op: op::SWITCH.to_string(),
            files: vec![PendingFile {
                path: PathBuf::from("/tmp/settings.json"),
                pre: None,
                planned: "abc".to_string(),
                staged: PathBuf::from("/tmp/settings.json.tmp.1"),
            }],
            target: PendingTarget {
                pointer: Some("p1".to_string()),
                extra: Map::new(),
            },
        }
    }

    #[test]
    fn pending_round_trips_and_clears() {
        let dir = tempfile::tempdir().unwrap();
        let store = DeviceStore::at(dir.path());

        assert_eq!(pending(&store, "claude").unwrap(), None);
        set_pending(&store, "claude", Some(sample_pending())).unwrap();
        assert_eq!(pending(&store, "claude").unwrap(), Some(sample_pending()));
        assert_eq!(
            apps_with_pending(&store).unwrap(),
            vec!["claude".to_string()]
        );

        set_pending(&store, "claude", None).unwrap();
        assert_eq!(pending(&store, "claude").unwrap(), None);
        let raw: Value = serde_json::from_slice(&fs::read(store.state_path()).unwrap()).unwrap();
        assert_eq!(raw, json!({"version": 1, "apps": {}}));
    }

    #[test]
    fn unknown_fields_survive_a_rewrite() {
        let dir = tempfile::tempdir().unwrap();
        let store = DeviceStore::at(dir.path());
        fs::write(
            store.state_path(),
            r#"{"version": 2, "future": true, "apps": {"codex": {"mode": "proxy"}}}"#,
        )
        .unwrap();

        set_pending(&store, "claude", Some(sample_pending())).unwrap();
        set_pending(&store, "claude", None).unwrap();

        let raw: Value = serde_json::from_slice(&fs::read(store.state_path()).unwrap()).unwrap();
        assert_eq!(
            raw,
            json!({"version": 2, "future": true, "apps": {"codex": {"mode": "proxy"}}})
        );
    }

    #[test]
    fn a_corrupt_state_file_is_moved_aside() {
        let dir = tempfile::tempdir().unwrap();
        let store = DeviceStore::at(dir.path());
        fs::write(store.state_path(), "{ not json").unwrap();

        assert_eq!(load(&store).unwrap(), LiveState::default());
        let names: Vec<String> = fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert!(
            names
                .iter()
                .any(|name| name.starts_with("live-state.json.corrupt-")),
            "{names:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn state_file_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let store = DeviceStore::at(dir.path());
        set_pending(&store, "claude", Some(sample_pending())).unwrap();
        let mode = fs::metadata(store.state_path())
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
    }
}
