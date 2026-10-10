//! Codex 官方做路由、又发布了 Stack 模型时，目录里的官方模型行。
//!
//! 写了 `model_catalog_json` 之后 Codex 只认文件里的模型，也不再刷新自己的模型列表
//! （codex-rs `StaticModelsManager` 忽略刷新策略），所以要把官方模型写全。来源依次是：
//!
//! 1. 自己拉的官方列表（`chatgpt.com/backend-api/codex/models`）：唯一在写了目录之后还能
//!    更新的来源，身份和版本都能核对。按「工作区 ID + 用户身份」分条缓存在
//!    `~/.cc-switch/codex-official-models.json`；
//! 2. 本机 Codex 自带的列表（`codex debug models --bundled`）：和账号无关，可能缺账号专属
//!    的模型。
//!
//! 不用 `~/.codex/models_cache.json`：它是哪个账号、哪个版本写的，CC Switch 证明不了。
//!
//! 服务端按 `client_version` 裁剪列表，版本取本机在用的 Codex 里最新的那个（见
//! [`newest_codex_version`]）。一个来源里没有一条能在模型选择器里选的模型（`visibility`
//! 都不是 `list`）就不算合格：照写的话官方模型全被藏起来，选择器里只剩第三方的（#8014）。
//!
//! 凭据取自操作之后 Codex 实际会用的登录（目标登录，见 `codex_direct`）。托管账号的
//! token 归 CC Switch 管，照常取；其余登录只读、绝不刷新：refresh token 会轮换，CC Switch
//! 刷新了，Codex 手里那份就作废，用户会被登出。token 过期或被拒时退回自带列表，等 Codex
//! 自己刷新。
//!
//! 缓存分两个问题：**能不能用**看身份和本机 Codex 版本；**新不新**看 `fetched_at` 是否在
//! 6 小时内。切换时能用而过期的条目先照用，同时在后台刷新，不在切换锁里等网络；后台另有
//! 启动时和每 15 分钟一次的检查（`controller::check_codex_official_models`）。
//!
//! 刷新存进了变化的列表之后，客户端文件要按它重写。重写可能失败（登录恰好变了、
//! `config.toml` 暂时解析不了），而缓存这时已经是新的，之后的刷新只会得到 304 或同样的
//! 列表。所以「客户端落后于缓存」单独记一个标记，重写成功才清掉，每个检查点都看它。

use std::cmp::Ordering as CmpOrdering;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::codex_config::{
    extract_codex_auth_user_identity, get_codex_config_dir, load_codex_bundled_models,
    normalize_codex_native_rows, read_codex_config_text,
};
use crate::codex_state_db::{codex_state_db_is_lockable, codex_state_db_paths};
use crate::database::Database;
use crate::live::engine::DeviceStore;
use crate::services::subscription::{
    parse_codex_credentials_json, CodexKeychainLogin, CredentialStatus,
};

pub(crate) const CACHE_FILENAME: &str = "codex-official-models.json";
/// 离 `fetched_at` 不到这么久算新的。目录只在 Codex 启动时读，刷新得再勤也要重启才看得到。
const FRESH_FOR: Duration = Duration::from_secs(6 * 60 * 60);
const FETCH_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_BODY_BYTES: usize = 16 * 1024 * 1024;
/// 后台检查的间隔（只读缓存文件；过期了才联网）。
pub(crate) const CHECK_EVERY: Duration = Duration::from_secs(15 * 60);
/// 取版本时看 Codex 最近创建的这么多个线程：够覆盖最近用过的各个客户端，又不让早已
/// 升级掉的版本混进来。
const RECENT_THREADS: u32 = 20;

/// 取官方列表用的登录。
#[derive(Clone)]
pub(crate) struct OfficialLogin {
    /// `<工作区 ID>|<用户身份>`。`tokens.account_id` 是 ChatGPT 工作区 ID，同一工作区的
    /// 不同用户共用它，单靠它分不出人；用户身份取 id_token 的 `sub`，跨 token 刷新不变。
    pub identity: String,
    access_token: String,
    account_id: String,
}

impl OfficialLogin {
    /// 从登录 JSON 取：要有有效的 access token、工作区 ID 和稳定的用户身份。取不到稳定
    /// 用户身份（没有 id_token，或里面没有 `sub`）时不拉取，和 Codex「身份不可用就不复用
    /// 缓存」同一取向。
    pub fn of(auth: &Value) -> Option<Self> {
        let (access_token, account_id, status, _) = parse_codex_credentials_json(&auth.to_string());
        if !matches!(status, CredentialStatus::Valid) {
            return None;
        }
        let access_token = access_token.filter(|token| !token.trim().is_empty())?;
        let account_id = account_id
            .map(|id| id.trim().to_string())
            .filter(|id| !id.is_empty())?;
        let user = extract_codex_auth_user_identity(auth)?;
        Some(Self {
            identity: format!("{account_id}|{user}"),
            access_token,
            account_id,
        })
    }
}

/// 一次拉取的结果。
pub(crate) enum Fetch {
    Models {
        models: Vec<Value>,
        etag: Option<String>,
    },
    /// 304：列表没变。
    NotModified,
    Failed(String),
}

/// 拉一次官方列表：登录、Codex 版本、上次的 etag。
pub(crate) type FetchFn = dyn Fn(&OfficialLogin, &str, Option<&str>) -> Fetch + Send + Sync;

/// 外部依赖：本机 Codex 版本、官方接口、Codex 自带的列表、钥匙串、时钟。测试里换成假的。
pub(crate) struct Env {
    pub codex_version: Box<dyn Fn() -> Option<String> + Send + Sync>,
    pub fetch: Box<FetchFn>,
    pub bundled: Box<dyn Fn() -> Option<Vec<Value>> + Send + Sync>,
    pub keychain: Box<dyn Fn() -> CodexKeychainLogin + Send + Sync>,
    /// Unix 秒。
    pub now: Box<dyn Fn() -> u64 + Send + Sync>,
}

impl Env {
    fn real() -> Self {
        Self {
            codex_version: Box::new(|| {
                newest_codex_version(
                    crate::commands::local_tool_version("codex"),
                    recent_thread_versions(),
                )
            }),
            fetch: Box::new(fetch_models),
            bundled: Box::new(load_codex_bundled_models),
            keychain: Box::new(crate::services::subscription::read_codex_keychain_login),
            now: Box::new(|| {
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|elapsed| elapsed.as_secs())
                    .unwrap_or_default()
            }),
        }
    }
}

fn env_slot() -> &'static Mutex<Option<Arc<Env>>> {
    static SLOT: OnceLock<Mutex<Option<Arc<Env>>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

fn env() -> Arc<Env> {
    let mut slot = env_slot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    slot.get_or_insert_with(|| Arc::new(Env::real())).clone()
}

#[cfg(test)]
pub(crate) fn set_test_env(env: Env) {
    *env_slot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(Arc::new(env));
}

#[cfg(test)]
pub(crate) fn reset_test_env() {
    *env_slot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
    CLIENT_BEHIND.store(false, Ordering::SeqCst);
}

/// 服务端按版本过滤列表：报低了少模型（`0.60.0` 返回 0 条），报高了会列出本机驱动不了的
/// 模型。拿不到版本就不请求，不编一个默认值。
fn usable_version(version: &str) -> bool {
    let version = version.trim();
    let core = version.split(['-', '+']).next().unwrap_or_default();
    !core.is_empty()
        && version.len() <= 64
        && core
            .split('.')
            .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
        && core.split('.').any(|part| part.bytes().any(|b| b != b'0'))
}

/// 本机在用的 Codex 里最新的版本：PATH 上的 `codex`，和 Codex 最近创建的线程记下的版本
/// （每个线程带创建它的客户端的 `cli_version`）。桌面版、编辑器插件自带的 Codex 不在 PATH
/// 上，只装桌面版的人 PATH 上又常留着早年装的旧 CLI：只看它，`0.140` 及更早拉到的官方
/// 模型全是隐藏的。目录是各个客户端共用的，按最新的拉；更旧的客户端读到驱动不了的模型时
/// 该升级。
fn newest_codex_version(path: Option<String>, threads: Vec<String>) -> Option<String> {
    path.into_iter()
        .chain(threads)
        .map(|version| version.trim().to_string())
        .filter(|version| usable_version(version))
        .max_by(|a, b| compare_versions(a, b))
}

/// 按 semver 比较 Codex 版本：先比数字段，同一版本的预发布（`-alpha.2`）比正式版旧，
/// 预发布之间逐段比（数字段按数值）。`+` 之后的构建信息不参与比较。
fn compare_versions(a: &str, b: &str) -> CmpOrdering {
    fn split(version: &str) -> (Vec<u64>, Option<&str>) {
        let version = version.split('+').next().unwrap_or_default();
        let (core, pre) = match version.split_once('-') {
            Some((core, pre)) => (core, Some(pre)),
            None => (version, None),
        };
        let core = core
            .split('.')
            .map(|part| part.parse().unwrap_or(0))
            .collect();
        (core, pre)
    }
    fn compare_pre(a: &str, b: &str) -> CmpOrdering {
        let mut a = a.split('.');
        let mut b = b.split('.');
        loop {
            match (a.next(), b.next()) {
                (None, None) => return CmpOrdering::Equal,
                (None, Some(_)) => return CmpOrdering::Less,
                (Some(_), None) => return CmpOrdering::Greater,
                (Some(x), Some(y)) => {
                    let order = match (x.parse::<u64>(), y.parse::<u64>()) {
                        (Ok(x), Ok(y)) => x.cmp(&y),
                        (Ok(_), Err(_)) => CmpOrdering::Less,
                        (Err(_), Ok(_)) => CmpOrdering::Greater,
                        (Err(_), Err(_)) => x.cmp(y),
                    };
                    if order != CmpOrdering::Equal {
                        return order;
                    }
                }
            }
        }
    }
    let (core_a, pre_a) = split(a);
    let (core_b, pre_b) = split(b);
    core_a.cmp(&core_b).then_with(|| match (pre_a, pre_b) {
        (None, None) => CmpOrdering::Equal,
        (None, Some(_)) => CmpOrdering::Greater,
        (Some(_), None) => CmpOrdering::Less,
        (Some(a), Some(b)) => compare_pre(a, b),
    })
}

/// Codex 最近创建的线程记下的客户端版本（各个 state DB 合起来）。读不了就是空的。
fn recent_thread_versions() -> Vec<String> {
    let config_text = read_codex_config_text().unwrap_or_default();
    codex_state_db_paths(&get_codex_config_dir(), &config_text)
        .into_iter()
        .filter(|path| path.exists() && codex_state_db_is_lockable(path))
        .flat_map(|path| thread_versions(&path))
        .collect()
}

fn thread_versions(db_path: &Path) -> Vec<String> {
    let read = || -> rusqlite::Result<Vec<String>> {
        let conn = rusqlite::Connection::open_with_flags(
            db_path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        // Codex 运行时一直开着这个库，写的时候不等一会儿就读不到。
        conn.busy_timeout(Duration::from_secs(2))?;
        let has_columns = Database::has_column(&conn, "threads", "cli_version").unwrap_or(false)
            && Database::has_column(&conn, "threads", "created_at").unwrap_or(false);
        if !has_columns {
            return Ok(Vec::new());
        }
        let mut stmt = conn.prepare(
            "SELECT cli_version FROM threads WHERE cli_version <> '' \
             ORDER BY created_at DESC LIMIT ?1",
        )?;
        let versions = stmt
            .query_map([RECENT_THREADS], |row| row.get::<_, String>(0))?
            .collect();
        versions
    };
    read().unwrap_or_else(|error| {
        log::debug!(
            "读取 Codex 线程的客户端版本失败 {}: {error}",
            db_path.display()
        );
        Vec::new()
    })
}

/// 系统钥匙串里 Codex 的登录（`cli_auth_credentials_store` 为 keyring / auto 时）。
pub(crate) fn keychain_login() -> CodexKeychainLogin {
    (env().keychain)()
}

fn fetch_models(login: &OfficialLogin, version: &str, etag: Option<&str>) -> Fetch {
    let version = version.to_string();
    let access_token = login.access_token.clone();
    let account_id = login.account_id.clone();
    let etag = etag.map(str::to_string);
    let run = move || {
        tauri::async_runtime::block_on(async move {
            let mut request = crate::proxy::http_client::get()
                .get(crate::services::codex_oauth_models::CODEX_OAUTH_MODELS_URL)
                .query(&[("client_version", version.as_str())])
                .header("Authorization", format!("Bearer {access_token}"))
                .header("ChatGPT-Account-Id", account_id)
                .header("Accept", "application/json")
                .timeout(FETCH_TIMEOUT);
            if let Some(etag) = etag {
                request = request.header("If-None-Match", etag);
            }
            let response = match request.send().await {
                Ok(response) => response,
                Err(error) => return Fetch::Failed(format!("网络错误: {error}")),
            };
            // 被重定向到别处（登录页之类）不算数。
            if response.url().host_str() != Some("chatgpt.com") {
                return Fetch::Failed(format!("被重定向到 {}", response.url()));
            }
            let status = response.status();
            if status == reqwest::StatusCode::NOT_MODIFIED {
                return Fetch::NotModified;
            }
            if !status.is_success() {
                return Fetch::Failed(format!("HTTP {status}"));
            }
            let etag = response
                .headers()
                .get(reqwest::header::ETAG)
                .and_then(|value| value.to_str().ok())
                .map(str::to_string);
            let bytes = match response.bytes().await {
                Ok(bytes) if bytes.len() <= MAX_BODY_BYTES => bytes,
                Ok(_) => return Fetch::Failed("响应过大".to_string()),
                Err(error) => return Fetch::Failed(format!("读取响应失败: {error}")),
            };
            match serde_json::from_slice::<Value>(&bytes)
                .ok()
                .and_then(|body| body.get("models").and_then(Value::as_array).cloned())
            {
                Some(models) => Fetch::Models { models, etag },
                None => Fetch::Failed("响应里没有 models".to_string()),
            }
        })
    };
    std::thread::spawn(run)
        .join()
        .unwrap_or_else(|_| Fetch::Failed("拉取线程异常退出".to_string()))
}

/// 缓存里一个身份的条目。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Entry {
    /// Unix 秒：只按数据本身的年龄判断新旧，和 CC Switch 运行了多久无关。
    fetched_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    etag: Option<String>,
    client_version: String,
    /// 服务端返回的原样（用的时候再补字段、校验）。
    models: Vec<Value>,
}

type Cache = BTreeMap<String, Entry>;

/// 缓存文件的读改写只在一个线程里做（切换和后台刷新会同时写）。
fn cache_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn read_cache() -> Cache {
    let path = DeviceStore::for_device().file(CACHE_FILENAME);
    let Ok(bytes) = std::fs::read(&path) else {
        return Cache::new();
    };
    serde_json::from_slice(&bytes).unwrap_or_else(|error| {
        log::warn!(
            "Codex 官方模型缓存 {} 无法解析，当作空的: {error}",
            path.display()
        );
        Cache::new()
    })
}

fn store_entry(identity: &str, entry: Entry) {
    let _guard = cache_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut cache = read_cache();
    cache.insert(identity.to_string(), entry);
    let path = DeviceStore::for_device().file(CACHE_FILENAME);
    if let Err(error) = crate::config::write_json_file_private(&path, &cache) {
        log::warn!("写入 Codex 官方模型缓存失败: {error}");
    }
}

/// 这个身份、这个版本能用的条目。
fn usable_entry(identity: &str, version: &str) -> Option<Entry> {
    read_cache()
        .remove(identity)
        .filter(|entry| entry.client_version == version)
}

fn is_fresh(entry: &Entry, now: u64) -> bool {
    now.saturating_sub(entry.fetched_at) < FRESH_FOR.as_secs()
}

/// 目录里官方行的来源。
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum NativeRows {
    /// 官方列表。`identity` 是取它用的登录：拿写锁后要和实际的目标登录核对。
    Fetched { identity: String, rows: Vec<Value> },
    /// Codex 自带的列表：官方列表暂未取到。
    Bundled { rows: Vec<Value> },
    /// 两个来源都没有合格数据：不写目录，Stack 模型暂不可用。
    Unavailable,
}

impl NativeRows {
    pub fn rows(&self) -> Option<&[Value]> {
        match self {
            Self::Fetched { rows, .. } | Self::Bundled { rows } => Some(rows),
            Self::Unavailable => None,
        }
    }

    pub fn identity(&self) -> Option<&str> {
        match self {
            Self::Fetched { identity, .. } => Some(identity),
            _ => None,
        }
    }
}

/// 最近一次切换用的是哪种来源（界面提示用）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeSource {
    Fetched,
    Bundled,
    Unavailable,
    /// 拉到的官方列表里没有一条能选的模型：本机 Codex 太旧，服务端只回了隐藏条目。官方
    /// 行退回自带列表（也没有能选的就不写目录），界面提示升级 Codex。
    Outdated,
}

fn last_source_slot() -> &'static Mutex<Option<NativeSource>> {
    static SLOT: OnceLock<Mutex<Option<NativeSource>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

pub(crate) fn last_source() -> Option<NativeSource> {
    *last_source_slot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// 切换时（拿着切换锁、在写锁之前）按目标登录取官方行。能用而新的条目直接用；能用而
/// 过期的先照用、后台刷新；没有能用的就联网拉一次（10 秒），拉不到退回自带列表。
pub(crate) fn rows_for_switch(login: Option<&OfficialLogin>) -> NativeRows {
    let env = env();
    let (rows, source) = match login.and_then(|login| official_rows(&env, login)) {
        Some(Official::Listed(rows)) => (rows, NativeSource::Fetched),
        Some(Official::NothingListed) => (bundled_rows(&env), NativeSource::Outdated),
        None => {
            let rows = bundled_rows(&env);
            let source = match &rows {
                NativeRows::Unavailable => NativeSource::Unavailable,
                _ => NativeSource::Bundled,
            };
            (rows, source)
        }
    };
    *last_source_slot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(source);
    rows
}

/// 能用的官方列表。
enum Official {
    Listed(NativeRows),
    /// 列表合格，但没有一条能在模型选择器里选（版本太旧时服务端只回隐藏条目）。
    NothingListed,
}

/// 至少有一行能在模型选择器里选（Codex 的 `show_in_picker` 就是 `visibility == list`）。
/// 全是隐藏行的列表写进目录，官方模型在选择器里一个都看不到。
fn has_listed_model(rows: &[Value]) -> bool {
    rows.iter()
        .any(|row| row.get("visibility").and_then(Value::as_str) == Some("list"))
}

fn official_rows(env: &Env, login: &OfficialLogin) -> Option<Official> {
    let version = (env.codex_version)()?;
    let now = (env.now)();
    let listed = |rows: Vec<Value>| {
        if has_listed_model(&rows) {
            Official::Listed(NativeRows::Fetched {
                identity: login.identity.clone(),
                rows,
            })
        } else {
            log::warn!("Codex {version} 拉到的官方模型列表里没有可选的模型（Codex 版本过旧），改用自带列表");
            Official::NothingListed
        }
    };
    if let Some(entry) = usable_entry(&login.identity, &version) {
        let fresh = is_fresh(&entry, now);
        if let Some(rows) = normalize_codex_native_rows(entry.models) {
            if !fresh {
                // 同一身份、同一版本的旧列表最多少几个新模型，比自带列表准。刷新带的是
                // 这次的目标登录：切换还没落定，事后重新预测会得到切换前的登录。
                spawn_refresh(login.clone(), version.clone());
            }
            return Some(listed(rows));
        }
        log::warn!("Codex 官方模型缓存里的列表不合格，重新拉取");
    }
    match (env.fetch)(login, &version, None) {
        Fetch::Models { models, etag } => {
            let rows = normalize_codex_native_rows(models.clone()).or_else(|| {
                log::warn!("Codex 官方模型列表不合格（缺必填字段或指令字段），改用自带列表");
                None
            })?;
            // 没有可选模型的列表也存：同一版本再拉还是这样，不必每次切换都等网络。
            store_entry(
                &login.identity,
                Entry {
                    fetched_at: now,
                    etag,
                    client_version: version.clone(),
                    models,
                },
            );
            Some(listed(rows))
        }
        Fetch::NotModified => None,
        Fetch::Failed(error) => {
            log::warn!("拉取 Codex 官方模型列表失败，改用自带列表: {error}");
            None
        }
    }
}

fn bundled_rows(env: &Env) -> NativeRows {
    match (env.bundled)()
        .and_then(normalize_codex_native_rows)
        .filter(|rows| has_listed_model(rows))
    {
        Some(rows) => NativeRows::Bundled { rows },
        None => NativeRows::Unavailable,
    }
}

/// 刷新一个身份的条目：一定联网，带 `If-None-Match`。304 只更新 `fetched_at`；失败保留
/// 旧条目和旧的 `fetched_at`，等下一个检查点再试。返回列表有没有变；变了同时记下客户端
/// 落后于缓存（见 [`take_client_behind`]）。
pub(crate) fn refresh(login: &OfficialLogin, version: &str) -> Result<bool, String> {
    let env = env();
    let now = (env.now)();
    let current = usable_entry(&login.identity, version);
    let etag = current.as_ref().and_then(|entry| entry.etag.clone());
    match (env.fetch)(login, version, etag.as_deref()) {
        Fetch::NotModified => {
            if let Some(entry) = current {
                store_entry(
                    &login.identity,
                    Entry {
                        fetched_at: now,
                        ..entry
                    },
                );
            }
            Ok(false)
        }
        Fetch::Models { models, etag } => {
            if normalize_codex_native_rows(models.clone()).is_none() {
                return Err("官方模型列表不合格".to_string());
            }
            let changed = current.as_ref().map(|entry| &entry.models) != Some(&models);
            store_entry(
                &login.identity,
                Entry {
                    fetched_at: now,
                    etag,
                    client_version: version.to_string(),
                    models,
                },
            );
            if changed {
                mark_client_behind();
            }
            Ok(changed)
        }
        Fetch::Failed(error) => Err(error),
    }
}

/// 后台检查要不要刷新：没有能用的条目，或者条目过期了。只读文件，不联网。
pub(crate) fn needs_refresh(login: &OfficialLogin) -> Option<String> {
    let env = env();
    let version = (env.codex_version)()?;
    match usable_entry(&login.identity, &version) {
        Some(entry) if is_fresh(&entry, (env.now)()) => None,
        _ => Some(version),
    }
}

/// 切换时发现条目过期：后台刷新，列表变了就重写 Codex 的客户端文件。
fn spawn_refresh(login: OfficialLogin, version: String) {
    std::thread::spawn(move || match refresh(&login, &version) {
        Ok(true) => after_refresh(),
        Ok(false) => {}
        Err(error) => log::warn!("后台刷新 Codex 官方模型列表失败: {error}"),
    });
}

fn state_slot() -> &'static OnceLock<crate::store::AppState> {
    static SLOT: OnceLock<crate::store::AppState> = OnceLock::new();
    &SLOT
}

/// 刷新存进了变化的列表，客户端文件还没按它重写成功。只记在进程里：CC Switch 重启时
/// 接上会强制重写，用的就是新缓存。
static CLIENT_BEHIND: AtomicBool = AtomicBool::new(false);

pub(crate) fn mark_client_behind() {
    CLIENT_BEHIND.store(true, Ordering::SeqCst);
}

/// 取走「客户端落后于缓存」的标记。取走后重写失败，用 [`mark_client_behind`] 放回去，
/// 下一个检查点再试；重写期间又刷新出新列表时标记重新立起，不会丢。
pub(crate) fn take_client_behind() -> bool {
    CLIENT_BEHIND.swap(false, Ordering::SeqCst)
}

/// 列表变了：按新列表重算契约，变了就重写目录（用户重启 Codex 后看到新模型）。后台检查
/// 还没开始时（CC Switch 正在启动）先不写，标记留给第一次检查。
pub(crate) fn after_refresh() {
    let Some(state) = state_slot().get().cloned() else {
        return;
    };
    tauri::async_runtime::spawn(async move {
        crate::mode::controller::resync_codex_if_behind(&state).await;
    });
}

/// CC Switch 启动时开始后台检查：先查一次，之后每 15 分钟一次。不是「每 6 小时」的
/// 定时器：从进程启动开始计时的话，每次只运行几小时的用户永远等不到刷新。
pub(crate) fn start_background_checks(state: crate::store::AppState) {
    let _ = state_slot().set(state.clone());
    tauri::async_runtime::spawn(async move {
        loop {
            crate::mode::controller::check_codex_official_models(&state).await;
            tokio::time::sleep(CHECK_EVERY).await;
        }
    });
}

#[cfg(test)]
pub(crate) mod testing {
    use super::*;

    /// 调用记录：拉取时用的身份、版本、带的 etag。
    pub(crate) type Calls = Arc<Mutex<Vec<(String, String, Option<String>)>>>;

    pub(crate) struct Fake {
        pub version: Option<String>,
        pub responses: Arc<Mutex<Vec<Fetch>>>,
        pub bundled: Option<Vec<Value>>,
        /// 钥匙串里的登录（测试可以中途换掉）。
        pub keychain: Arc<Mutex<CodexKeychainLogin>>,
        /// 读了几次钥匙串。
        pub keychain_reads: Arc<std::sync::atomic::AtomicUsize>,
        pub now: Arc<Mutex<u64>>,
        pub calls: Calls,
        /// 拉取时顺带做的事（模拟拿锁前后之间登录变了、通知测试拉取发生了）。
        pub on_fetch: Option<Box<dyn Fn() + Send + Sync>>,
    }

    impl Fake {
        pub fn install(self) {
            let Fake {
                version,
                responses,
                bundled,
                keychain,
                keychain_reads,
                now,
                calls,
                on_fetch,
            } = self;
            set_test_env(Env {
                codex_version: Box::new(move || version.clone()),
                fetch: Box::new(move |login, version, etag| {
                    calls.lock().unwrap().push((
                        login.identity.clone(),
                        version.to_string(),
                        etag.map(str::to_string),
                    ));
                    let response = {
                        let mut responses = responses.lock().unwrap();
                        if responses.is_empty() {
                            Fetch::Failed("no scripted response".to_string())
                        } else {
                            responses.remove(0)
                        }
                    };
                    if let Some(on_fetch) = &on_fetch {
                        on_fetch();
                    }
                    response
                }),
                bundled: Box::new(move || bundled.clone()),
                keychain: Box::new(move || {
                    keychain_reads.fetch_add(1, Ordering::SeqCst);
                    keychain.lock().unwrap().clone()
                }),
                now: Box::new(move || *now.lock().unwrap()),
            });
        }
    }

    /// 一份新近刷新过的 ChatGPT 登录。
    pub(crate) fn login(account: &str, sub: &str) -> Value {
        serde_json::json!({
            "auth_mode": "chatgpt",
            "OPENAI_API_KEY": null,
            "tokens": {
                "id_token": crate::codex_config::test_codex_id_token(sub),
                "access_token": format!("access-{sub}"),
                "refresh_token": format!("refresh-{sub}"),
                "account_id": account,
            },
            "last_refresh": chrono::Utc::now().to_rfc3339(),
        })
    }

    /// 合格的官方行（只有新的指令字段，和 0.158 的缓存一样）。
    pub(crate) fn native_models(slugs: &[(&str, i64)]) -> Vec<Value> {
        slugs
            .iter()
            .map(|(slug, priority)| {
                serde_json::json!({
                    "slug": slug,
                    "priority": priority,
                    "visibility": "list",
                    "comp_hash": "3000",
                    "model_messages": { "instructions_template": "T" },
                    "supports_reasoning_summaries": true,
                    "supports_parallel_tool_calls": true,
                })
            })
            .collect()
    }

    pub(crate) fn cache_fetched_at(identity: &str) -> Option<u64> {
        read_cache().get(identity).map(|entry| entry.fetched_at)
    }

    pub(crate) fn seed_cache(identity: &str, fetched_at: u64, version: &str, models: Vec<Value>) {
        store_entry(
            identity,
            Entry {
                fetched_at,
                etag: Some("\"etag-1\"".to_string()),
                client_version: version.to_string(),
                models,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::testing::*;
    use super::*;
    use serde_json::json;
    use serial_test::serial;
    use std::sync::mpsc;

    /// 临时 home（缓存文件落在这里），结束时换回假依赖之前的状态。
    struct Scope {
        _dir: tempfile::TempDir,
        saved: Option<std::ffi::OsString>,
    }

    impl Scope {
        fn new() -> Self {
            let dir = tempfile::TempDir::new().unwrap();
            let saved = std::env::var_os("CC_SWITCH_TEST_HOME");
            std::env::set_var("CC_SWITCH_TEST_HOME", dir.path());
            Self { _dir: dir, saved }
        }
    }

    impl Drop for Scope {
        fn drop(&mut self) {
            reset_test_env();
            match self.saved.take() {
                Some(value) => std::env::set_var("CC_SWITCH_TEST_HOME", value),
                None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
            }
        }
    }

    fn models(slugs: &[&str]) -> Vec<Value> {
        slugs
            .iter()
            .map(|slug| {
                json!({
                    "slug": slug,
                    "priority": 1,
                    "visibility": "list",
                    "model_messages": { "instructions_template": "T" },
                    "supports_reasoning_summaries": true,
                    "supports_parallel_tool_calls": true,
                })
            })
            .collect()
    }

    fn slugs(rows: &NativeRows) -> Vec<String> {
        rows.rows()
            .unwrap_or_default()
            .iter()
            .map(|row| row["slug"].as_str().unwrap().to_string())
            .collect()
    }

    const HOUR: u64 = 3600;
    const T0: u64 = 1_800_000_000;

    struct Setup {
        calls: Calls,
        now: Arc<Mutex<u64>>,
        responses: Arc<Mutex<Vec<Fetch>>>,
    }

    fn install(version: Option<&str>, responses: Vec<Fetch>, bundled: Option<Vec<Value>>) -> Setup {
        install_with(version, responses, bundled, None)
    }

    fn install_with(
        version: Option<&str>,
        responses: Vec<Fetch>,
        bundled: Option<Vec<Value>>,
        notify: Option<mpsc::Sender<()>>,
    ) -> Setup {
        let calls: Calls = Arc::default();
        let now = Arc::new(Mutex::new(T0));
        let responses = Arc::new(Mutex::new(responses));
        Fake {
            version: version.map(str::to_string),
            responses: responses.clone(),
            bundled,
            keychain: Arc::new(Mutex::new(CodexKeychainLogin::Missing)),
            keychain_reads: Arc::default(),
            now: now.clone(),
            calls: calls.clone(),
            on_fetch: notify.map(|notify| -> Box<dyn Fn() + Send + Sync> {
                Box::new(move || {
                    let _ = notify.send(());
                })
            }),
        }
        .install();
        Setup {
            calls,
            now,
            responses,
        }
    }

    fn fetched(slugs: &[&str]) -> Fetch {
        Fetch::Models {
            models: models(slugs),
            etag: Some("\"etag-2\"".to_string()),
        }
    }

    #[test]
    fn logins_are_told_apart_by_workspace_and_user() {
        let alice = OfficialLogin::of(&login("ws-1", "alice")).unwrap();
        let bob = OfficialLogin::of(&login("ws-1", "bob")).unwrap();
        assert_ne!(
            alice.identity, bob.identity,
            "same workspace, different people"
        );
        assert_eq!(alice.identity, "ws-1|sub:alice");

        // 没有 id_token：身份不稳定，不拉取。
        let mut no_id = login("ws-1", "alice");
        no_id["tokens"].as_object_mut().unwrap().remove("id_token");
        assert!(OfficialLogin::of(&no_id).is_none());
        // 很久没刷新的 token 不用，等 Codex 自己刷新。
        let mut stale = login("ws-1", "alice");
        stale["last_refresh"] = json!("2026-01-01T00:00:00Z");
        assert!(OfficialLogin::of(&stale).is_none());
        // API Key 登录没有官方列表。
        assert!(OfficialLogin::of(&json!({ "OPENAI_API_KEY": "sk" })).is_none());
    }

    #[test]
    #[serial]
    fn a_missing_entry_is_fetched_once_and_reused_while_fresh() {
        let _scope = Scope::new();
        let setup = install(Some("0.158.0"), vec![fetched(&["gpt-6-sol"])], None);
        let login = OfficialLogin::of(&login("ws", "alice")).unwrap();

        let rows = rows_for_switch(Some(&login));
        assert_eq!(slugs(&rows), vec!["gpt-6-sol"]);
        assert_eq!(rows.identity(), Some("ws|sub:alice"));
        assert_eq!(
            setup.calls.lock().unwrap().clone(),
            vec![("ws|sub:alice".to_string(), "0.158.0".to_string(), None)]
        );
        assert_eq!(last_source(), Some(NativeSource::Fetched));

        // 一小时后：新的，不联网。
        *setup.now.lock().unwrap() = T0 + HOUR;
        assert_eq!(slugs(&rows_for_switch(Some(&login))), vec!["gpt-6-sol"]);
        assert_eq!(setup.calls.lock().unwrap().len(), 1);
        assert!(needs_refresh(&login).is_none());
    }

    #[test]
    #[serial]
    fn entries_for_another_codex_version_or_person_are_not_used() {
        let _scope = Scope::new();
        seed_cache("ws|sub:alice", T0, "0.157.1", models(&["old-version"]));
        seed_cache("ws|sub:bob", T0, "0.158.0", models(&["bobs"]));
        install(Some("0.158.0"), vec![fetched(&["alices"])], None);
        let alice = OfficialLogin::of(&login("ws", "alice")).unwrap();
        assert_eq!(slugs(&rows_for_switch(Some(&alice))), vec!["alices"]);
        // 各存各的，互不覆盖。
        assert_eq!(cache_fetched_at("ws|sub:bob"), Some(T0));
    }

    #[test]
    #[serial]
    fn a_stale_entry_is_used_now_and_refreshed_in_the_background() {
        let _scope = Scope::new();
        seed_cache(
            "ws|sub:alice",
            T0 - 7 * HOUR,
            "0.158.0",
            models(&["cached"]),
        );
        let (tx, rx) = mpsc::channel();
        let setup = install_with(
            Some("0.158.0"),
            vec![fetched(&["cached", "new-model"])],
            None,
            Some(tx),
        );
        let login = OfficialLogin::of(&login("ws", "alice")).unwrap();

        // 这次先用旧条目完成切换，不等网络。
        assert_eq!(slugs(&rows_for_switch(Some(&login))), vec!["cached"]);
        rx.recv_timeout(Duration::from_secs(5))
            .expect("background refresh");
        // 刷新带的是这次的目标登录和旧条目的 etag。
        assert_eq!(
            setup.calls.lock().unwrap()[0],
            (
                "ws|sub:alice".to_string(),
                "0.158.0".to_string(),
                Some("\"etag-1\"".to_string())
            )
        );
        for _ in 0..50 {
            if cache_fetched_at("ws|sub:alice") == Some(T0) {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(cache_fetched_at("ws|sub:alice"), Some(T0));
        assert_eq!(
            slugs(&rows_for_switch(Some(&login))),
            vec!["cached", "new-model"]
        );
        // 后台检查还没开始（CC Switch 正在启动）时刷新完了：不重写，标记留给第一次检查。
        for _ in 0..50 {
            if CLIENT_BEHIND.load(Ordering::SeqCst) {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(take_client_behind());
    }

    #[test]
    #[serial]
    fn a_refresh_always_asks_and_keeps_the_old_entry_when_it_fails() {
        let _scope = Scope::new();
        seed_cache(
            "ws|sub:alice",
            T0 - 7 * HOUR,
            "0.158.0",
            models(&["cached"]),
        );
        let setup = install(
            Some("0.158.0"),
            vec![
                Fetch::Failed("offline".to_string()),
                Fetch::NotModified,
                fetched(&["cached"]),
                fetched(&["cached", "new-model"]),
            ],
            None,
        );
        let login = OfficialLogin::of(&login("ws", "alice")).unwrap();
        assert_eq!(needs_refresh(&login).as_deref(), Some("0.158.0"));

        // 失败：旧条目和旧的 fetched_at 都留着。
        assert!(refresh(&login, "0.158.0").is_err());
        assert_eq!(cache_fetched_at("ws|sub:alice"), Some(T0 - 7 * HOUR));
        // 304：只更新 fetched_at，列表没变。
        assert_eq!(refresh(&login, "0.158.0"), Ok(false));
        assert_eq!(cache_fetched_at("ws|sub:alice"), Some(T0));
        // 条目新了也照样联网（刷新不看新旧）；列表相同不算变化。
        assert_eq!(refresh(&login, "0.158.0"), Ok(false));
        assert!(!take_client_behind(), "nothing changed yet");
        assert_eq!(refresh(&login, "0.158.0"), Ok(true));
        // 列表变了：客户端落后于缓存，标记取走一次就没了。
        assert!(take_client_behind());
        assert!(!take_client_behind());
        let etags: Vec<Option<String>> = setup
            .calls
            .lock()
            .unwrap()
            .iter()
            .map(|call| call.2.clone())
            .collect();
        assert_eq!(etags[0].as_deref(), Some("\"etag-1\""));
        assert!(setup.responses.lock().unwrap().is_empty());
    }

    #[test]
    #[serial]
    fn without_a_usable_official_list_the_bundled_list_is_used() {
        let _scope = Scope::new();
        let login = OfficialLogin::of(&login("ws", "alice")).unwrap();
        let bundled = Some(models(&["bundled"]));

        // 拿不到版本：不请求。
        let setup = install(None, vec![fetched(&["x"])], bundled.clone());
        assert_eq!(slugs(&rows_for_switch(Some(&login))), vec!["bundled"]);
        assert!(setup.calls.lock().unwrap().is_empty());
        assert_eq!(last_source(), Some(NativeSource::Bundled));

        // 没有能用的登录、请求失败、列表不合格：都退回自带列表。
        install(Some("0.158.0"), Vec::new(), bundled.clone());
        assert_eq!(slugs(&rows_for_switch(None)), vec!["bundled"]);
        install(
            Some("0.158.0"),
            vec![Fetch::Failed("401".to_string())],
            bundled.clone(),
        );
        assert_eq!(slugs(&rows_for_switch(Some(&login))), vec!["bundled"]);
        let invalid = Fetch::Models {
            models: vec![json!({ "slug": "no-instructions" })],
            etag: None,
        };
        install(Some("0.158.0"), vec![invalid], bundled);
        assert_eq!(slugs(&rows_for_switch(Some(&login))), vec!["bundled"]);
        assert!(
            cache_fetched_at("ws|sub:alice").is_none(),
            "an invalid list is not cached"
        );

        // 两个来源都没有：不可用。
        install(Some("0.158.0"), Vec::new(), None);
        assert_eq!(rows_for_switch(Some(&login)), NativeRows::Unavailable);
        assert_eq!(last_source(), Some(NativeSource::Unavailable));
    }

    fn hidden(rows: Vec<Value>) -> Vec<Value> {
        rows.into_iter()
            .map(|mut row| {
                row["visibility"] = json!("hide");
                row
            })
            .collect()
    }

    /// #8014：旧版本拉到的官方列表只有隐藏条目，写进目录后官方模型在选择器里全看不到。
    #[test]
    #[serial]
    fn a_source_without_a_listed_model_is_not_used() {
        let _scope = Scope::new();
        let login = OfficialLogin::of(&login("ws", "alice")).unwrap();
        let only_hidden = Fetch::Models {
            models: hidden(models(&["gpt-5.5", "codex-auto-review"])),
            etag: None,
        };

        // 官方列表没有能选的：退回自带列表，提示升级。列表照样缓存，同一版本不再联网。
        let setup = install(
            Some("0.130.0"),
            vec![only_hidden],
            Some(models(&["bundled"])),
        );
        assert_eq!(slugs(&rows_for_switch(Some(&login))), vec!["bundled"]);
        assert_eq!(last_source(), Some(NativeSource::Outdated));
        assert!(cache_fetched_at("ws|sub:alice").is_some());
        assert_eq!(slugs(&rows_for_switch(Some(&login))), vec!["bundled"]);
        assert_eq!(setup.calls.lock().unwrap().len(), 1);
        assert_eq!(last_source(), Some(NativeSource::Outdated));

        // 自带列表也没有能选的：不可用，提示的仍是升级。
        install(
            Some("0.130.0"),
            Vec::new(),
            Some(hidden(models(&["bundled"]))),
        );
        assert_eq!(rows_for_switch(Some(&login)), NativeRows::Unavailable);
        assert_eq!(last_source(), Some(NativeSource::Outdated));

        // 没有登录、只有全隐藏的自带列表：不可用。
        assert_eq!(rows_for_switch(None), NativeRows::Unavailable);
        assert_eq!(last_source(), Some(NativeSource::Unavailable));

        // 有一行能选就算合格，隐藏的行照样保留（官方自己就这样发）。
        let mut mixed = hidden(models(&["codex-auto-review"]));
        mixed.extend(models(&["gpt-6-sol"]));
        install(
            Some("0.160.0"),
            vec![Fetch::Models {
                models: mixed,
                etag: None,
            }],
            None,
        );
        assert_eq!(
            slugs(&rows_for_switch(Some(&login))),
            vec!["codex-auto-review", "gpt-6-sol"]
        );
        assert_eq!(last_source(), Some(NativeSource::Fetched));
    }

    #[test]
    fn versions_compare_like_semver() {
        let ordered = [
            "0.60.0",
            "0.140.0",
            "0.159.0-alpha.2",
            "0.159.0-alpha.12.1",
            "0.159.0-beta",
            "0.159.0",
            "0.159.2",
            "0.162.0-alpha.2",
            "0.162.0",
        ];
        for pair in ordered.windows(2) {
            assert_eq!(
                compare_versions(pair[0], pair[1]),
                CmpOrdering::Less,
                "{} < {}",
                pair[0],
                pair[1]
            );
        }
        assert_eq!(
            compare_versions("0.160.0+build.7", "0.160.0"),
            CmpOrdering::Equal
        );
    }

    /// 只装桌面版的人 PATH 上常留着旧 CLI：取 Codex 线程里记下的更新的版本。
    #[test]
    fn the_newest_codex_in_use_wins() {
        let threads = |versions: &[&str]| versions.iter().map(|v| v.to_string()).collect();
        assert_eq!(
            newest_codex_version(
                Some("0.128.0".to_string()),
                threads(&["0.162.0-alpha.2", "0.159.2", ""])
            ),
            Some("0.162.0-alpha.2".to_string())
        );
        assert_eq!(
            newest_codex_version(Some("0.162.0".to_string()), threads(&["0.162.0-alpha.2"])),
            Some("0.162.0".to_string())
        );
        // 没有 CLI 也行；认不出的版本号不算。
        assert_eq!(
            newest_codex_version(None, threads(&["0.160.0", "dev", "0.0.0"])),
            Some("0.160.0".to_string())
        );
        assert_eq!(newest_codex_version(None, Vec::new()), None);
    }

    #[test]
    fn thread_versions_come_from_the_most_recently_created_threads() {
        if crate::config::sqlite_unsupported_in_temp_dir() {
            return;
        }
        let dir = tempfile::TempDir::new().unwrap();
        let db = dir.path().join("state_5.sqlite");
        let conn = rusqlite::Connection::open(&db).unwrap();
        conn.execute_batch(
            "CREATE TABLE threads (id TEXT PRIMARY KEY, created_at INTEGER NOT NULL, \
             cli_version TEXT NOT NULL DEFAULT '');",
        )
        .unwrap();
        // 最老的一个是 0.99.0，排在最近的 RECENT_THREADS 个之后，不算；没记版本的不算。
        conn.execute("INSERT INTO threads VALUES ('old', 0, '0.99.0')", [])
            .unwrap();
        for i in 1..=RECENT_THREADS {
            conn.execute(
                "INSERT INTO threads VALUES (?1, ?2, '0.160.0')",
                rusqlite::params![format!("t{i}"), i],
            )
            .unwrap();
        }
        conn.execute("INSERT INTO threads VALUES ('blank', 999, '')", [])
            .unwrap();
        drop(conn);
        let versions = thread_versions(&db);
        assert_eq!(versions.len(), RECENT_THREADS as usize);
        assert!(versions.iter().all(|v| v == "0.160.0"));

        // 旧版 Codex 的库没有这一列：读不到，不报错。
        let old = dir.path().join("old.sqlite");
        rusqlite::Connection::open(&old)
            .unwrap()
            .execute_batch("CREATE TABLE threads (id TEXT, created_at INTEGER);")
            .unwrap();
        assert!(thread_versions(&old).is_empty());
        assert!(thread_versions(&dir.path().join("missing.sqlite")).is_empty());
    }

    #[test]
    #[serial]
    fn codex_s_own_models_cache_is_never_read() {
        let scope = Scope::new();
        let codex_dir = scope._dir.path().join(".codex");
        std::fs::create_dir_all(&codex_dir).unwrap();
        std::fs::write(
            codex_dir.join("models_cache.json"),
            json!({ "client_version": "0.158.0", "models": models(&["someone-elses"]) })
                .to_string(),
        )
        .unwrap();
        install(Some("0.158.0"), Vec::new(), None);
        let login = OfficialLogin::of(&login("ws", "alice")).unwrap();
        assert_eq!(rows_for_switch(Some(&login)), NativeRows::Unavailable);
    }
}
