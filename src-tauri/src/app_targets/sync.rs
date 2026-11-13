//! 逐目标写入：主环境由现有链路写（指针、模式、代理都按原样），跟随环境在各自的作用域
//! 里复用同一套直连写入、MCP、提示词和 Skills 投影。
//!
//! - 先主环境，成功后再并发处理跟随环境；每个目标独立备份、独立提交，不跨目标回滚；
//! - 每个跟随目标有自己的写锁（`<app>@<目标>`）和操作代次：超时后旧写入仍可能完成，
//!   但只有代次仍是最新时才记结果；重试用当时最新的选择重新生成补丁；
//! - 托管登录（Claude 绑号官方卡、Codex 官方登录）只写 Windows 主环境，跟随环境保持原配置。

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::{mpsc, Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::{json, Map, Value};

use super::model::{self, AppTargetsConfig, ConfigSource, TargetId, TargetSpec};
use super::scope::{self, TargetScope};
use super::status::{self, TargetRecord, TargetStatus};
use super::wsl;
use crate::app_config::AppType;
use crate::error::AppError;
use crate::provider::Provider;
use crate::store::AppState;

const WSL_TIMEOUT: Duration = Duration::from_secs(10);
const WINDOWS_TIMEOUT: Duration = Duration::from_secs(30);

/// 一次同步写哪些东西。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncKind {
    /// 供应商（及随它重投影的 MCP）：切换、编辑当前供应商。
    Provider,
    /// 供应商、MCP、提示词、Skills：新目标、立即同步、重试。
    Full,
    Mcp,
    Prompt,
    Skills,
}

impl SyncKind {
    fn writes_provider(self) -> bool {
        matches!(self, Self::Provider | Self::Full)
    }
    fn writes_mcp(self) -> bool {
        matches!(self, Self::Provider | Self::Full | Self::Mcp)
    }
    fn writes_prompt(self) -> bool {
        matches!(self, Self::Full | Self::Prompt)
    }
    fn writes_skills(self) -> bool {
        matches!(self, Self::Full | Self::Skills)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetReport {
    pub target: String,
    #[serde(flatten)]
    pub record: TargetRecord,
}

fn legacy_dir(settings: &crate::settings::AppSettings, app: &AppType) -> Option<String> {
    match app {
        AppType::Claude => settings.claude_config_dir.clone(),
        AppType::Codex => settings.codex_config_dir.clone(),
        AppType::Gemini => settings.gemini_config_dir.clone(),
        AppType::OpenCode => settings.opencode_config_dir.clone(),
        _ => None,
    }
}

/// 这个应用的有效目标配置。
pub fn app_config(app: &AppType) -> (AppTargetsConfig, ConfigSource) {
    let settings = crate::settings::get_settings();
    model::effective(
        settings.app_targets.as_ref(),
        app,
        legacy_dir(&settings, app).as_deref(),
    )
}

/// 有没有需要跟随写入的目标（只读设置，不碰文件）。
pub fn has_followers(app: &AppType) -> bool {
    if !model::supports(app) {
        return false;
    }
    let (config, source) = app_config(app);
    source == ConfigSource::Saved && !config.followers().is_empty()
}

pub fn store_root(app: &AppType, id: &TargetId) -> PathBuf {
    crate::config::get_home_dir()
        .join(".cc-switch")
        .join("targets")
        .join(app.as_str())
        .join(id.slug())
}

pub fn scope_for(app: &AppType, id: &TargetId, dir: Option<PathBuf>) -> TargetScope {
    TargetScope {
        app: app.clone(),
        key: id.key(),
        dir,
        store_root: store_root(app, id),
        wsl: matches!(id, TargetId::Wsl(_)),
    }
}

fn home_unresolved(distro: &str) -> AppError {
    AppError::localized(
        "app_targets.home_unresolved",
        format!("未获取到 WSL 发行版 {distro} 的用户目录，请填写自定义目录"),
        format!(
            "Could not resolve the home directory of WSL distro {distro}; enter a custom directory"
        ),
    )
}

/// 目标目录。Windows 没有自定义目录时为 `None`（用应用现有的默认解析，包括它认的环境
/// 变量）；WSL 用自定义目录或按发行版 HOME 推导的自动目录，推导不出来时报错，不回落到
/// Windows HOME。`allow_start` 为假时不为了读 HOME 启动发行版。
pub fn resolve_dir(
    app: &AppType,
    spec: &TargetSpec,
    allow_start: bool,
) -> Result<Option<PathBuf>, AppError> {
    match &spec.id {
        TargetId::Windows => Ok(spec
            .dir
            .as_deref()
            .map(crate::settings::resolve_override_path)),
        TargetId::Wsl(distro) => {
            if let Some(dir) = &spec.dir {
                return Ok(Some(PathBuf::from(dir)));
            }
            let env = wsl::resolve_env(distro, allow_start)?;
            let linux =
                model::linux_default_dir(app, &env).ok_or_else(|| home_unresolved(distro))?;
            Ok(Some(PathBuf::from(model::wsl_unc_for(distro, &linux))))
        }
    }
}

/// 作用域里这个应用的配置目录（走应用自己的解析器）。
fn app_dir(app: &AppType) -> PathBuf {
    match app {
        AppType::Claude => crate::config::get_claude_config_dir(),
        AppType::Codex => crate::codex_config::get_codex_config_dir(),
        AppType::Gemini => crate::gemini_config::get_gemini_dir(),
        AppType::OpenCode => crate::opencode_config::get_opencode_dir(),
        _ => PathBuf::new(),
    }
}

/// 目标实际写入的目录（显示、查重用）；解析不出来时为 `None`。
pub fn display_dir(app: &AppType, spec: &TargetSpec, allow_start: bool) -> Option<String> {
    match resolve_dir(app, spec, allow_start) {
        Ok(Some(dir)) => Some(dir.to_string_lossy().into_owned()),
        Ok(None) => {
            let (dir, _) = scope::run(scope_for(app, &spec.id, None), || app_dir(app));
            Some(dir.to_string_lossy().into_owned())
        }
        Err(_) => None,
    }
}

/// 每个目标解析出的目录。只为 `probe` 里的目标启动发行版。
pub fn resolved_dirs(
    app: &AppType,
    config: &AppTargetsConfig,
    probe: &dyn Fn(&TargetSpec) -> bool,
) -> BTreeMap<String, String> {
    config
        .targets()
        .iter()
        .filter_map(|spec| display_dir(app, spec, probe(spec)).map(|dir| (spec.id.key(), dir)))
        .collect()
}

// ===== 托管字段的身份与指纹 =====

fn short_hash(text: &str) -> String {
    crate::live::engine::sha256_hex(text.as_bytes())[..12].to_string()
}

fn text_field(value: Option<&Value>) -> Value {
    value
        .and_then(Value::as_str)
        .map(|s| s.trim().trim_end_matches('/'))
        .filter(|s| !s.is_empty())
        .map(|s| json!(s))
        .unwrap_or(Value::Null)
}

/// 凭据只留短摘要，用于比较，不出现在对话框或日志里。
fn secret_field(value: Option<&Value>) -> Value {
    value
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(|s| json!(short_hash(s)))
        .unwrap_or(Value::Null)
}

fn non_empty(object: Map<String, Value>) -> Option<Value> {
    object
        .values()
        .any(|v| !v.is_null())
        .then_some(Value::Object(object))
}

/// 从「供应商的 settings_config」或「同形状的 live 内容」里取出切换时会被替换的关键字段，
/// 用来判断两边是不是同一家。OpenCode 是累加式，没有「当前那家」，不比较。
pub fn identity_from_settings(app: &AppType, settings: &Value) -> Option<Value> {
    let mut out = Map::new();
    match app {
        AppType::Claude => {
            let env = settings.get("env");
            let get = |key: &str| env.and_then(|env| env.get(key));
            out.insert("baseUrl".into(), text_field(get("ANTHROPIC_BASE_URL")));
            out.insert(
                "authToken".into(),
                secret_field(get("ANTHROPIC_AUTH_TOKEN")),
            );
            out.insert("apiKey".into(), secret_field(get("ANTHROPIC_API_KEY")));
            out.insert("model".into(), text_field(get("ANTHROPIC_MODEL")));
            out.insert("bedrock".into(), text_field(get("CLAUDE_CODE_USE_BEDROCK")));
            out.insert("vertex".into(), text_field(get("CLAUDE_CODE_USE_VERTEX")));
            out.insert(
                "apiKeyHelper".into(),
                secret_field(settings.get("apiKeyHelper")),
            );
        }
        AppType::Codex => {
            let auth = settings.get("auth");
            let config: toml::Value = settings
                .get("config")
                .and_then(Value::as_str)
                .and_then(|text| toml::from_str(text).ok())
                .unwrap_or(toml::Value::Table(Default::default()));
            let provider = config.get("model_provider").and_then(toml::Value::as_str);
            let base_url = provider
                .and_then(|name| config.get("model_providers")?.get(name)?.get("base_url"))
                .and_then(toml::Value::as_str)
                .map(|s| json!(s));
            out.insert("baseUrl".into(), text_field(base_url.as_ref()));
            out.insert(
                "model".into(),
                text_field(
                    config
                        .get("model")
                        .and_then(toml::Value::as_str)
                        .map(|s| json!(s))
                        .as_ref(),
                ),
            );
            out.insert(
                "apiKey".into(),
                secret_field(auth.and_then(|a| a.get("OPENAI_API_KEY"))),
            );
        }
        AppType::Gemini => {
            let env = settings.get("env");
            let get = |key: &str| env.and_then(|env| env.get(key));
            out.insert("baseUrl".into(), text_field(get("GOOGLE_GEMINI_BASE_URL")));
            out.insert("apiKey".into(), secret_field(get("GEMINI_API_KEY")));
            out.insert("model".into(), text_field(get("GEMINI_MODEL")));
            let selected = settings
                .get("config")
                .and_then(|c| c.pointer("/security/auth/selectedType"));
            out.insert("authType".into(), text_field(selected));
        }
        _ => return None,
    }
    non_empty(out)
}

/// 作用域里读目标的 live，取出关键字段。没有配置文件时为 `None`。
fn read_live_shaped(app: &AppType) -> Result<Option<Value>, AppError> {
    use crate::config::read_json_file;
    match app {
        AppType::Claude => {
            let path = crate::config::get_claude_settings_path();
            if !path.exists() {
                return Ok(None);
            }
            Ok(Some(read_json_file(&path)?))
        }
        AppType::Codex => {
            let auth_path = crate::codex_config::get_codex_auth_path();
            let config_path = crate::codex_config::get_codex_config_path();
            if !auth_path.exists() && !config_path.exists() {
                return Ok(None);
            }
            let auth: Value = if auth_path.exists() {
                read_json_file(&auth_path)?
            } else {
                json!({})
            };
            let config = if config_path.exists() {
                std::fs::read_to_string(&config_path).map_err(|e| AppError::io(&config_path, e))?
            } else {
                String::new()
            };
            Ok(Some(json!({ "auth": auth, "config": config })))
        }
        AppType::Gemini => {
            let env_path = crate::gemini_config::get_gemini_env_path();
            let settings_path = crate::gemini_config::get_gemini_settings_path();
            if !env_path.exists() && !settings_path.exists() {
                return Ok(None);
            }
            let env = crate::gemini_config::read_gemini_env()?;
            let config: Value = if settings_path.exists() {
                read_json_file(&settings_path)?
            } else {
                json!({})
            };
            Ok(Some(json!({ "env": env, "config": config })))
        }
        AppType::OpenCode => {
            let path = crate::opencode_config::get_opencode_config_path()?;
            if !path.exists() {
                return Ok(None);
            }
            let config = crate::opencode_config::read_opencode_config_from_path(&path)?;
            Ok(Some(
                json!({ "provider": config.get("provider").cloned().unwrap_or(Value::Null) }),
            ))
        }
        _ => Ok(None),
    }
}

/// 作用域里目标托管字段的指纹（发现外部修改用）。
pub fn fingerprint_in_scope(app: &AppType) -> Option<String> {
    let live = read_live_shaped(app).ok().flatten()?;
    let basis = match app {
        AppType::OpenCode => live.get("provider").cloned().unwrap_or(Value::Null),
        _ => identity_from_settings(app, &live).unwrap_or(Value::Null),
    };
    Some(short_hash(&basis.to_string()))
}

fn host_of(identity: &Value) -> Option<String> {
    let url = identity.get("baseUrl")?.as_str()?;
    let without_scheme = url.split("://").nth(1).unwrap_or(url);
    without_scheme
        .split(['/', '?', '#'])
        .next()
        .map(|host| host.rsplit('@').next().unwrap_or(host).to_string())
        .filter(|host| !host.is_empty())
}

/// 托管登录：本阶段只写 Windows 主环境，不跨环境复制登录材料。
pub fn is_managed_login(app: &AppType, provider: &Provider) -> bool {
    match app {
        AppType::Claude => {
            crate::services::provider::claude_official::bound_account(provider).is_some()
        }
        AppType::Codex => crate::services::provider::codex_direct::is_official(provider),
        _ => false,
    }
}

// ===== 冲突、导入、备份 =====

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetConflict {
    pub target: String,
    pub dir: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub existing_provider_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub existing_provider_name: Option<String>,
    /// 原配置的服务主机（不含路径和凭据）；没有自定义地址时为空。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub existing_host: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub incoming_provider_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub incoming_provider_name: Option<String>,
}

fn current_provider(state: &AppState, app: &AppType) -> Result<Option<Provider>, AppError> {
    let Some(id) =
        crate::mode::current::provider_for(&state.db, app, crate::mode::current::Purpose::Direct)?
    else {
        return Ok(None);
    };
    Ok(state.db.get_all_providers(app.as_str())?.get(&id).cloned())
}

/// 新目标里已有和当前选择不同的托管字段时返回冲突；没有配置、或就是当前这家时为 `None`。
pub fn inspect_target(
    state: &AppState,
    app: &AppType,
    spec: &TargetSpec,
    dir: Option<PathBuf>,
) -> Result<Option<TargetConflict>, AppError> {
    if matches!(app, AppType::OpenCode) {
        return Ok(None);
    }
    let Some(current) = current_provider(state, app)? else {
        return Ok(None);
    };
    if is_managed_login(app, &current) && spec.id != TargetId::Windows {
        // 托管登录不写 WSL：保持原配置，不需要处理冲突。
        return Ok(None);
    }
    let (live, _) = scope::run(scope_for(app, &spec.id, dir.clone()), || {
        read_live_shaped(app)
    });
    let Some(live) = live? else {
        return Ok(None);
    };
    let Some(existing) = identity_from_settings(app, &live) else {
        return Ok(None);
    };
    if identity_from_settings(app, &current.settings_config).as_ref() == Some(&existing) {
        return Ok(None);
    }
    let providers = state.db.get_all_providers(app.as_str())?;
    let matched = providers
        .values()
        .find(|p| identity_from_settings(app, &p.settings_config).as_ref() == Some(&existing));
    let shown_dir = match dir {
        Some(dir) => dir.to_string_lossy().into_owned(),
        None => display_dir(app, spec, false).unwrap_or_default(),
    };
    Ok(Some(TargetConflict {
        target: spec.id.key(),
        dir: shown_dir,
        existing_provider_id: matched.map(|p| p.id.clone()),
        existing_provider_name: matched.map(|p| p.name.clone()),
        existing_host: host_of(&existing),
        incoming_provider_id: Some(current.id.clone()),
        incoming_provider_name: Some(current.name.clone()),
    }))
}

pub fn env_label(id: &TargetId) -> String {
    match id {
        TargetId::Windows => "Windows".to_string(),
        TargetId::Wsl(distro) => format!("WSL · {distro}"),
    }
}

/// 把目标里的配置导入成供应商卡（关键字段和已有卡相同时不重复创建）。返回卡的 ID。
pub fn import_target(
    state: &AppState,
    app: &AppType,
    spec: &TargetSpec,
    dir: Option<PathBuf>,
) -> Result<Option<String>, AppError> {
    let (live, _) = scope::run(scope_for(app, &spec.id, dir), || read_live_shaped(app));
    let Some(live) = live? else {
        return Ok(None);
    };
    let Some(identity) = identity_from_settings(app, &live) else {
        return Ok(None);
    };
    let providers = state.db.get_all_providers(app.as_str())?;
    if let Some(existing) = providers
        .values()
        .find(|p| identity_from_settings(app, &p.settings_config).as_ref() == Some(&identity))
    {
        return Ok(Some(existing.id.clone()));
    }
    let settings_config = match app {
        AppType::Claude => {
            let mut config = Map::new();
            config.insert(
                "env".into(),
                live.get("env").cloned().unwrap_or_else(|| json!({})),
            );
            if let Some(helper) = live.get("apiKeyHelper") {
                config.insert("apiKeyHelper".into(), helper.clone());
            }
            Value::Object(config)
        }
        AppType::Codex => json!({ "auth": live["auth"].clone(), "config": live["config"].clone() }),
        AppType::Gemini => json!({ "env": live["env"].clone(), "config": live["config"].clone() }),
        _ => return Ok(None),
    };
    let name = match host_of(&identity) {
        Some(host) => format!("{} · {host}", env_label(&spec.id)),
        None => env_label(&spec.id),
    };
    let id = uuid::Uuid::new_v4().to_string();
    let provider = Provider::with_id(id.clone(), name, settings_config, None);
    state.db.save_provider(app.as_str(), &provider)?;
    log::info!(
        "[app-targets] 从 {} 导入了 {} 供应商卡 {id}",
        spec.id.key(),
        app.as_str()
    );
    Ok(Some(id))
}

fn managed_files(app: &AppType) -> Vec<PathBuf> {
    match app {
        AppType::Claude => vec![crate::config::get_claude_settings_path()],
        AppType::Codex => vec![
            crate::codex_config::get_codex_config_path(),
            crate::codex_config::get_codex_auth_path(),
        ],
        AppType::Gemini => vec![
            crate::gemini_config::get_gemini_env_path(),
            crate::gemini_config::get_gemini_settings_path(),
        ],
        AppType::OpenCode => crate::opencode_config::get_opencode_config_path()
            .into_iter()
            .collect(),
        _ => Vec::new(),
    }
}

/// 写入前备份目标的托管文件到这个目标自己的状态目录。返回备份目录。
pub fn backup_target(
    app: &AppType,
    spec: &TargetSpec,
    dir: Option<PathBuf>,
) -> Result<PathBuf, AppError> {
    let stamp = chrono::Utc::now().format("%Y%m%dT%H%M%S%3fZ").to_string();
    let backup_dir = store_root(app, &spec.id).join("backups").join(stamp);
    let (result, _) = scope::run(scope_for(app, &spec.id, dir), || {
        for file in managed_files(app) {
            if !file.exists() {
                continue;
            }
            let bytes = std::fs::read(&file).map_err(|e| AppError::io(&file, e))?;
            let name = file
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "file".to_string());
            crate::config::atomic_write_private(&backup_dir.join(name), &bytes)?;
        }
        Ok::<_, AppError>(())
    });
    result?;
    Ok(backup_dir)
}

// ===== 跟随写入 =====

fn generations() -> &'static Mutex<HashMap<String, u64>> {
    static GENERATIONS: OnceLock<Mutex<HashMap<String, u64>>> = OnceLock::new();
    GENERATIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn bump_generation(app: &AppType, key: &str) -> u64 {
    let mut map = generations().lock().unwrap_or_else(|e| e.into_inner());
    let entry = map.entry(format!("{}@{key}", app.as_str())).or_insert(0);
    *entry += 1;
    *entry
}

fn is_latest(app: &AppType, key: &str, generation: u64) -> bool {
    generations()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&format!("{}@{key}", app.as_str()))
        .is_none_or(|latest| *latest == generation)
}

/// 记一个目标的结果并返回报告。
pub fn record(app: &AppType, key: &str, record: TargetRecord) -> TargetReport {
    status::set(app.as_str(), key, record.clone());
    TargetReport {
        target: key.to_string(),
        record: status::get(app.as_str(), key).unwrap_or(record),
    }
}

/// 错误信息里不带凭据：只留第一行，截断过长的部分。
fn error_detail(error: &AppError) -> String {
    let text = error.to_string();
    let line = text.lines().next().unwrap_or_default();
    line.chars().take(300).collect()
}

struct Job {
    key: String,
    timeout: Duration,
    rx: mpsc::Receiver<TargetReport>,
    done: Arc<Mutex<bool>>,
    generation: u64,
    dir: Option<String>,
}

/// 在作用域里写一个目标；返回（托管字段指纹, 提示）。
fn write_in_scope(
    state: &AppState,
    app: &AppType,
    kind: SyncKind,
    current: Option<&Provider>,
    prev: Option<&Provider>,
) -> Result<(Option<String>, Vec<String>), AppError> {
    let mut warnings = Vec::new();
    if kind.writes_provider() {
        crate::services::provider::apply_to_follower(state, app, current, prev)?;
    }
    if kind.writes_mcp() {
        if let Err(err) = crate::services::McpService::sync_enabled_for_app(state, app) {
            if kind == SyncKind::Mcp {
                return Err(err);
            }
            warnings.push(format!("mcp_failed:{}", error_detail(&err)));
        }
    }
    if kind.writes_prompt() {
        if let Err(err) = crate::services::PromptService::sync_to_live(state, app.clone()) {
            if kind == SyncKind::Prompt {
                return Err(err);
            }
            warnings.push(format!("prompt_failed:{}", error_detail(&err)));
        }
    }
    if kind.writes_skills() {
        if let Err(err) = crate::services::skill::SkillService::sync_to_app(&state.db, app) {
            let err = AppError::Message(err.to_string());
            if kind == SyncKind::Skills {
                return Err(err);
            }
            warnings.push(format!("skills_failed:{}", error_detail(&err)));
        }
    }
    Ok((fingerprint_in_scope(app), warnings))
}

/// 把当前选择写进跟随环境。`only` 限定目标（重试、新目标），`None` 为全部跟随目标。
/// 主环境不在这里写。
pub fn sync_followers(
    state: &AppState,
    app: &AppType,
    only: Option<&[TargetId]>,
    kind: SyncKind,
) -> Vec<TargetReport> {
    if !model::supports(app) || scope::active() {
        return Vec::new();
    }
    let (config, source) = app_config(app);
    if source != ConfigSource::Saved {
        return Vec::new();
    }
    let followers: Vec<TargetSpec> = config
        .followers()
        .into_iter()
        .filter(|spec| only.is_none_or(|only| only.iter().any(|id| id.same(&spec.id))))
        .collect();
    if followers.is_empty() {
        return Vec::new();
    }

    let mut reports = Vec::new();
    if kind.writes_provider() && crate::mode::current::is_proxy(app) {
        // 路由模式下主环境写的是代理契约；跟随环境的接管和恢复尚未接入，暂不写。
        for spec in &followers {
            reports.push(record(
                app,
                &spec.id.key(),
                TargetRecord::new(TargetStatus::Deferred).reason("proxy_mode"),
            ));
        }
        return reports;
    }

    let current = match current_provider(state, app) {
        Ok(current) => current,
        Err(err) => {
            for spec in &followers {
                reports.push(record(
                    app,
                    &spec.id.key(),
                    TargetRecord::new(TargetStatus::Error)
                        .reason("write_failed")
                        .detail(error_detail(&err)),
                ));
            }
            return reports;
        }
    };
    let providers = state.db.get_all_providers(app.as_str()).unwrap_or_default();

    let mut jobs = Vec::new();
    for spec in followers {
        let key = spec.id.key();
        if kind.writes_provider() && current.as_ref().is_some_and(|p| is_managed_login(app, p)) {
            reports.push(record(
                app,
                &key,
                TargetRecord::new(TargetStatus::Skipped).reason("managed_login"),
            ));
            continue;
        }
        let generation = bump_generation(app, &key);
        let dir = match resolve_dir(app, &spec, true) {
            Ok(dir) => dir,
            Err(err) => {
                reports.push(record(
                    app,
                    &key,
                    TargetRecord::new(TargetStatus::Error)
                        .reason("home_unresolved")
                        .detail(error_detail(&err)),
                ));
                continue;
            }
        };
        let shown_dir = dir
            .as_ref()
            .map(|d| d.to_string_lossy().into_owned())
            .or_else(|| display_dir(app, &spec, false));
        let previous = status::get(app.as_str(), &key);
        let prev = previous
            .as_ref()
            .and_then(|r| r.provider_id.as_ref())
            .and_then(|id| providers.get(id))
            .cloned();
        let (tx, rx) = mpsc::channel();
        let done = Arc::new(Mutex::new(false));
        let worker_done = done.clone();
        let worker_state = state.clone();
        let worker_app = app.clone();
        let worker_current = current.clone();
        let worker_key = key.clone();
        let worker_dir = shown_dir.clone();
        let worker_scope = scope_for(app, &spec.id, dir);
        let timeout = if matches!(spec.id, TargetId::Wsl(_)) {
            WSL_TIMEOUT
        } else {
            WINDOWS_TIMEOUT
        };
        std::thread::spawn(move || {
            let (result, notes) = scope::run(worker_scope, || {
                write_in_scope(
                    &worker_state,
                    &worker_app,
                    kind,
                    worker_current.as_ref(),
                    prev.as_ref(),
                )
            });
            let previous = status::get(worker_app.as_str(), &worker_key);
            let record = match result {
                Ok((fingerprint, mut warnings)) => {
                    warnings.extend(notes);
                    if kind.writes_provider() {
                        TargetRecord {
                            provider_id: worker_current.as_ref().map(|p| p.id.clone()),
                            fingerprint,
                            dir: worker_dir,
                            warnings,
                            ..TargetRecord::new(TargetStatus::Ok)
                        }
                    } else {
                        // 只写 MCP / 提示词 / Skills：供应商状态不变，只更新提示。
                        let mut record = previous.unwrap_or_else(|| {
                            TargetRecord::new(TargetStatus::Deferred).reason("not_synced")
                        });
                        record.warnings = warnings;
                        record.at = chrono::Utc::now().timestamp();
                        record
                    }
                }
                Err(err) => {
                    log::warn!(
                        "[app-targets] 写入 {} 的 {worker_key} 失败: {}",
                        worker_app.as_str(),
                        crate::error_for_log(&err.to_string())
                    );
                    if kind.writes_provider() {
                        TargetRecord {
                            dir: worker_dir,
                            ..TargetRecord::new(TargetStatus::Error)
                                .reason("write_failed")
                                .detail(error_detail(&err))
                        }
                    } else {
                        let mut record = previous.unwrap_or_else(|| {
                            TargetRecord::new(TargetStatus::Deferred).reason("not_synced")
                        });
                        record.warnings = vec![format!(
                            "{}_failed:{}",
                            match kind {
                                SyncKind::Mcp => "mcp",
                                SyncKind::Prompt => "prompt",
                                _ => "skills",
                            },
                            error_detail(&err)
                        )];
                        record
                    }
                }
            };
            let mut finished = worker_done.lock().unwrap_or_else(|e| e.into_inner());
            let report = if is_latest(&worker_app, &worker_key, generation) {
                record_report(&worker_app, &worker_key, record)
            } else {
                // 已有更新的操作：这次的结果过期了，不记。
                TargetReport {
                    target: worker_key.clone(),
                    record: TargetRecord::new(TargetStatus::Deferred).reason("superseded"),
                }
            };
            *finished = true;
            drop(finished);
            let _ = tx.send(report);
        });
        jobs.push(Job {
            key,
            timeout,
            rx,
            done,
            generation,
            dir: shown_dir,
        });
    }

    let started = Instant::now();
    for job in jobs {
        let remaining = job.timeout.saturating_sub(started.elapsed());
        match job.rx.recv_timeout(remaining) {
            Ok(report) => reports.push(report),
            Err(_) => {
                let finished = job.done.lock().unwrap_or_else(|e| e.into_inner());
                if *finished {
                    drop(finished);
                    if let Ok(report) = job.rx.recv() {
                        reports.push(report);
                    }
                    continue;
                }
                // 超时：后台写入可能稍后完成（代次没变时它会更新结果）。
                let report = if is_latest(app, &job.key, job.generation) {
                    record_report(
                        app,
                        &job.key,
                        TargetRecord {
                            dir: job.dir.clone(),
                            ..TargetRecord::new(TargetStatus::Error).reason("timeout")
                        },
                    )
                } else {
                    TargetReport {
                        target: job.key.clone(),
                        record: TargetRecord::new(TargetStatus::Deferred).reason("superseded"),
                    }
                };
                drop(finished);
                reports.push(report);
            }
        }
    }
    reports
}

fn record_report(app: &AppType, key: &str, record: TargetRecord) -> TargetReport {
    self::record(app, key, record)
}

/// 主环境之外的变化之后（编辑当前供应商、MCP / 提示词 / Skills 改动），尽力同步跟随环境。
pub fn after_change(state: &AppState, app: &AppType, kind: SyncKind) -> Vec<TargetReport> {
    if !has_followers(app) || scope::active() {
        return Vec::new();
    }
    let reports = sync_followers(state, app, None, kind);
    let failed: Vec<&str> = reports
        .iter()
        .filter(|r| r.record.status == TargetStatus::Error)
        .map(|r| r.target.as_str())
        .collect();
    if !failed.is_empty() {
        log::warn!(
            "[app-targets] {} 跟随环境同步未完成: {}",
            app.as_str(),
            failed.join(", ")
        );
    }
    reports
}

/// 检查已同步的跟随目标是否被外部修改（托管字段指纹变了）。不启动发行版：
/// `can_read` 为假的目标跳过。
pub fn refresh_external(
    app: &AppType,
    config: &AppTargetsConfig,
    can_read: &dyn Fn(&TargetSpec) -> bool,
) {
    for spec in config.followers() {
        let key = spec.id.key();
        let Some(record) = status::get(app.as_str(), &key) else {
            continue;
        };
        if !matches!(record.status, TargetStatus::Ok | TargetStatus::External) {
            continue;
        }
        let Some(expected) = record.fingerprint.clone() else {
            continue;
        };
        if !can_read(&spec) {
            continue;
        }
        let Ok(dir) = resolve_dir(app, &spec, false) else {
            continue;
        };
        let (actual, _) = scope::run(scope_for(app, &spec.id, dir), || fingerprint_in_scope(app));
        let Some(actual) = actual else {
            continue;
        };
        if actual != expected && record.status == TargetStatus::Ok {
            status::mark(
                app.as_str(),
                &key,
                TargetStatus::External,
                Some("managed_fields_changed"),
            );
        } else if actual == expected && record.status == TargetStatus::External {
            status::mark(app.as_str(), &key, TargetStatus::Ok, None);
        }
    }
}

/// 供应商新增、编辑、删除之后：累加式的 OpenCode 总要同步；切换式应用只在改的是当前
/// 那家时才需要（别的卡不在客户端文件里）。
pub fn after_provider_edit(state: &AppState, app: &AppType, provider_id: Option<&str>) {
    if !has_followers(app) {
        return;
    }
    if !matches!(app, AppType::OpenCode) {
        let current = crate::mode::current::provider_for(
            &state.db,
            app,
            crate::mode::current::Purpose::Direct,
        )
        .ok()
        .flatten();
        if provider_id.is_none() || current.as_deref() != provider_id {
            return;
        }
    }
    after_change(state, app, SyncKind::Provider);
}

/// 在后台尽力同步跟随环境（MCP、提示词、Skills 改动后），不让界面等 WSL。
pub fn spawn_after_change(state: &AppState, apps: Vec<AppType>, kind: SyncKind) {
    let apps: Vec<AppType> = apps.into_iter().filter(has_followers).collect();
    if apps.is_empty() || scope::active() {
        return;
    }
    let state = state.clone();
    std::thread::spawn(move || {
        for app in apps {
            after_change(&state, &app, kind);
        }
    });
}

/// 四个支持多环境的应用。
pub fn all_supported() -> Vec<AppType> {
    model::supported_apps().to_vec()
}
