//! `live-state.json`：这台设备上每个应用的客户端文件状态。
//!
//! - `mode`、`attached`、`proxy_route`、`contract`：直连 / 代理模式（`mode::controller`）；
//! - `written`：CC Switch 上次写进客户端文件、之后要按记录删掉的东西（Grok 的模型表）；
//! - `stack`：代理模式的 Stack 模型（`mode::stack`）；
//! - `pending`：一次写客户端文件的操作在发布前写下的意图，按文件记录写前、写后的
//!   hash 和已备好的临时文件，崩溃后据此前滚或丢弃（`mode::operation`）。
//!
//! 不认识的字段读写时原样保留。
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Direct,
    Proxy,
}

/// 进入代理时写进客户端文件的契约。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Contract {
    pub version: u32,
    /// 契约内容的摘要：切换路由时摘要相同，客户端文件就不用动。
    pub key: String,
    /// 契约写进客户端的独有字段。退出代理时按它删除（值相同才删）：路由供应商的行之后可能
    /// 被编辑过，不能到时再按行重新计算。
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub exclusive: Map<String, Value>,
}

/// 一个应用的模式状态。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ModeState {
    /// 没有值：这台设备还没运行过有双模式的版本，启动时按旧版遗留的接管状态定下来。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<Mode>,
    /// 客户端文件当前是否指向代理。退出 CC Switch 时分离、下次启动再接上。
    #[serde(default, skip_serializing_if = "is_false")]
    pub attached: bool,
    /// 代理模式下路由到的供应商。和直连指针互相独立，退出代理时保留。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proxy_route: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contract: Option<Contract>,
}

fn is_false(value: &bool) -> bool {
    !*value
}

impl ModeState {
    pub fn is_proxy(&self) -> bool {
        self.mode == Some(Mode::Proxy)
    }

    /// 代理模式下路由到的就是 `id` 这家。
    pub fn routes_to(&self, id: &str) -> bool {
        self.is_proxy() && self.proxy_route.as_deref() == Some(id)
    }
}

/// CC Switch 上次写进客户端文件、切走时要按记录删掉的东西。
///
/// 不能按 live 现在的内容去找：客户端自己会改。Grok 的 `/settings` 会把 `models.default`
/// 改成内置模型，按它找表就会漏删上一家的表；而 CC Switch 默认的表名 `grok-4.5` 正好是
/// 内置模型 ID，留下的表会覆盖内置模型，把官方请求连同第三方 Key 发到第三方地址。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Written {
    /// Grok Build `config.toml` 里 CC Switch 写的 `[model."<名称>"]` 表。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tables: Vec<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// 代理模式的 Stack 模型：这些供应商的模型以带前缀的 id 发布给客户端，选中后请求直达那一家
/// （`mode::stack`）。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct StackState {
    /// Stack 模式：代理模式下发布 Stack 模型、不做故障转移（界面上和路由模式二选一，见
    /// `controller::enter`）。只在代理模式下有意义：每次进入代理时按用户选的模式写定，
    /// 退出代理时不动，名单也留着。
    #[serde(default, skip_serializing_if = "is_false")]
    pub enabled: bool,
    /// 当前 Stack 里的供应商 id，按加入顺序。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub members: Vec<String>,
    /// key 登记簿：key → 供应商 id。一经分配永久归这家，移除成员、删除供应商都不回收：
    /// 客户端会一直带着选中过的 id，key 改了指向，旧 id 就会被悄悄发到另一家。
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub keys: BTreeMap<String, String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl StackState {
    pub fn is_empty(&self) -> bool {
        !self.enabled && self.members.is_empty() && self.keys.is_empty() && self.extra.is_empty()
    }

    /// 这家在登记簿里的 key。
    pub fn key_of(&self, provider_id: &str) -> Option<&str> {
        self.keys
            .iter()
            .find(|(_, id)| id.as_str() == provider_id)
            .map(|(key, _)| key.as_str())
    }

    pub fn is_member(&self, provider_id: &str) -> bool {
        self.members.iter().any(|id| id == provider_id)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AppLiveState {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<Mode>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub attached: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proxy_route: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contract: Option<Contract>,
    /// 没有值：这台设备上还没有新版写过这个应用的文件（升级前旧版写的，按行推断）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub written: Option<Written>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending: Option<Pending>,
    #[serde(default, skip_serializing_if = "StackState::is_empty")]
    pub stack: StackState,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl AppLiveState {
    fn is_empty(&self) -> bool {
        self.pending.is_none()
            && self.written.is_none()
            && self.stack.is_empty()
            && self.mode_state() == ModeState::default()
            && self.extra.is_empty()
    }

    pub fn mode_state(&self) -> ModeState {
        ModeState {
            mode: self.mode,
            attached: self.attached,
            proxy_route: self.proxy_route.clone(),
            contract: self.contract.clone(),
        }
    }

    pub fn set_mode_state(&mut self, state: ModeState) {
        self.mode = state.mode;
        self.attached = state.attached;
        self.proxy_route = state.proxy_route;
        self.contract = state.contract;
    }
}

/// 操作名。用字符串而不是枚举，旧版本读到新版本写的操作名也能照常前滚或丢弃。
pub mod op {
    /// 切换供应商（会改指针）。
    pub const SWITCH: &str = "switch";
    /// 把当前供应商重新写进客户端文件（编辑、同步等）。
    pub const APPLY: &str = "apply";
    /// 进入代理模式：客户端文件写成代理契约。
    pub const ENTER: &str = "enter";
    /// 退出代理模式：客户端文件写回直连供应商。
    pub const EXIT: &str = "exit";
    /// 退出 CC Switch 时把客户端指回直连，模式不变。
    pub const DETACH: &str = "detach";
    /// 启动时把客户端重新指向代理。
    pub const ATTACH: &str = "attach";
    /// 代理模式下换路由（契约变了时同一操作里先改写客户端）。
    pub const ROUTE: &str = "route";
    /// 增删 Stack 模型（契约变了时同一操作里先改写客户端）。
    pub const STACK: &str = "stack";
    /// Codex 改用 CC Switch 生成的模型目录（用户在 Stack 提示上点的）：一律重写客户端。
    pub const CATALOG: &str = "catalog";
}

/// 一次操作的写前意图。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Pending {
    pub op: String,
    pub files: Vec<PendingFile>,
    #[serde(default)]
    pub target: PendingTarget,
    /// 已经开始发布：换进第一个文件之前记下，所以只要有文件发布过它就一定在。发布过的
    /// 文件之后可能又被客户端改掉（Codex 刷新登录），单看文件内容就分不出发布开始过没有，
    /// 恢复时靠它决定前滚还是丢弃。
    #[serde(default, skip_serializing_if = "is_false")]
    pub published: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PendingFile {
    pub path: PathBuf,
    /// 写前内容的 hash；`None` 表示写前文件不存在。
    pub pre: Option<String>,
    /// 写后内容的 hash；`None` 表示这个操作要删掉它。
    #[serde(default)]
    pub planned: Option<String>,
    /// 已写好写后内容、等着 rename 的临时文件；删文件时没有。
    #[serde(default)]
    pub staged: Option<PathBuf>,
}

/// 文件都写完之后要落定的状态。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PendingTarget {
    /// 直连指针：切换成功后当前供应商是谁。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pointer: Option<String>,
    /// 模式状态：有值时整体替换这个应用的 mode、attached、proxy_route、contract。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<ModeState>,
    /// 写入记录：有值时整体替换。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub written: Option<Written>,
    /// Stack 模型：有值时整体替换这个应用的 `stack`（成员和登记簿一起）。不放进 `state`：
    /// `state` 会整体替换，不认识 `stack` 的版本写下的 pending 前滚时就会把名单清空。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stack: Option<StackState>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl PendingTarget {
    /// 只改直连指针（`None` 表示不改）。
    pub fn pointer(pointer: Option<String>) -> Self {
        Self {
            pointer,
            ..Self::default()
        }
    }

    /// 只落定模式状态。
    pub fn mode(state: ModeState) -> Self {
        Self {
            state: Some(state),
            ..Self::default()
        }
    }

    pub fn is_empty(&self) -> bool {
        self.pointer.is_none()
            && self.state.is_none()
            && self.written.is_none()
            && self.stack.is_none()
            && self.extra.is_empty()
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

/// 这个应用的模式状态。
/// 几个应用的模式状态，状态文件只读一次。
pub fn mode_states<const N: usize>(
    store: &DeviceStore,
    apps: [&str; N],
) -> Result<[ModeState; N], AppError> {
    let _guard = state_lock().lock().unwrap_or_else(|e| e.into_inner());
    let state = load(store)?;
    Ok(apps.map(|app| {
        state
            .apps
            .get(app)
            .map(AppLiveState::mode_state)
            .unwrap_or_default()
    }))
}

pub fn mode_state(store: &DeviceStore, app: &str) -> Result<ModeState, AppError> {
    let [mode] = mode_states(store, [app])?;
    Ok(mode)
}

/// 这个应用的写入记录；`None` 表示新版还没写过。
pub fn written(store: &DeviceStore, app: &str) -> Result<Option<Written>, AppError> {
    let _guard = state_lock().lock().unwrap_or_else(|e| e.into_inner());
    Ok(load(store)?
        .apps
        .get(app)
        .and_then(|state| state.written.clone()))
}

/// 这个应用的 Stack 模型（成员和 key 登记簿）。
pub fn stack(store: &DeviceStore, app: &str) -> Result<StackState, AppError> {
    let _guard = state_lock().lock().unwrap_or_else(|e| e.into_inner());
    Ok(load(store)?
        .apps
        .get(app)
        .map(|state| state.stack.clone())
        .unwrap_or_default())
}

/// 这个应用在 Stack 模式（代理模式且 Stack 模式开着），状态文件只读一次。
pub fn stack_mode(store: &DeviceStore, app: &str) -> Result<bool, AppError> {
    let _guard = state_lock().lock().unwrap_or_else(|e| e.into_inner());
    Ok(load(store)?
        .apps
        .get(app)
        .is_some_and(|state| state.mode == Some(Mode::Proxy) && state.stack.enabled))
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
                planned: Some("abc".to_string()),
                staged: Some(PathBuf::from("/tmp/settings.json.tmp.1")),
            }],
            target: PendingTarget::pointer(Some("p1".to_string())),
            published: false,
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
