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
    PromptClear,
    Skills,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum SyncChannel {
    Provider,
    Mcp,
    Prompt,
    Skills,
}

impl SyncChannel {
    fn covers_warning(self, warning: &str) -> bool {
        match self {
            Self::Provider => false,
            Self::Mcp => warning.starts_with("mcp_"),
            Self::Prompt => warning.starts_with("prompt_"),
            Self::Skills => warning.starts_with("skills_"),
        }
    }
}

impl SyncKind {
    fn writes_provider(self) -> bool {
        matches!(self, Self::Provider | Self::Full)
    }
    fn writes_mcp(self) -> bool {
        matches!(self, Self::Provider | Self::Full | Self::Mcp)
    }
    fn writes_prompt(self) -> bool {
        matches!(self, Self::Full | Self::Prompt | Self::PromptClear)
    }
    fn writes_skills(self) -> bool {
        matches!(self, Self::Full | Self::Skills)
    }
    fn channels(self) -> &'static [SyncChannel] {
        match self {
            Self::Provider => &[SyncChannel::Provider, SyncChannel::Mcp],
            Self::Full => &[
                SyncChannel::Provider,
                SyncChannel::Mcp,
                SyncChannel::Prompt,
                SyncChannel::Skills,
            ],
            Self::Mcp => &[SyncChannel::Mcp],
            Self::Prompt | Self::PromptClear => &[SyncChannel::Prompt],
            Self::Skills => &[SyncChannel::Skills],
        }
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
    source == ConfigSource::Saved
        && config.primary == model::WINDOWS
        && config.windows.enabled
        && !config.followers().is_empty()
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
        direct: None,
        client_host: None,
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
/// Windows HOME。`allow_start` 只允许刷新运行中发行版的文件信息，不执行命令或启动发行版。
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
            #[cfg(windows)]
            if allow_start && !wsl::is_running(distro) {
                return Err(AppError::Message(format!(
                    "WSL · {distro} is not running; start it manually"
                )));
            }
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

/// 托管登录（绑了 CC Switch 托管账号的官方卡）：本阶段只写 Windows 主环境，不跨环境复制
/// 登录材料。没绑账号的官方卡用各环境自己的登录，照常写。
pub fn is_managed_login(app: &AppType, provider: &Provider) -> bool {
    match app {
        AppType::Claude => {
            crate::services::provider::claude_official::bound_account(provider).is_some()
        }
        // 没绑托管账号的官方卡用那个环境自己的登录（auth.json），可以在 WSL 用。
        AppType::Codex => {
            crate::services::provider::ProviderService::managed_codex_oauth_account_id(provider)
                .is_some()
        }
        _ => false,
    }
}

/// 主环境没有 TargetScope，仍需按保存的目标（含旧 UNC 设置）隔离宿主登录。
pub(crate) fn is_wsl_target(app: &AppType) -> bool {
    if scope::active() {
        scope::is_wsl()
    } else {
        matches!(app_config(app).0.primary_id(), Some(TargetId::Wsl(_)))
    }
}

pub(crate) fn ensure_provider_allowed(app: &AppType, provider: &Provider) -> Result<(), AppError> {
    if is_managed_login(app, provider) && is_wsl_target(app) {
        return Err(AppError::localized(
            "app_targets.managed_login_windows_only",
            "绑定了托管账号的官方卡目前只能在 Windows 使用；WSL 可以用没绑账号的官方卡（用它自己的登录）或 API Key 供应商",
            "Official cards bound to a managed account only work on Windows for now; in WSL use an unbound official card (its own sign-in) or an API key provider",
        ));
    }
    Ok(())
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
    Ok(state.db.get_all_providers(app)?.get(&id).cloned())
}

/// 新目标里已有和当前选择不同的托管字段时返回冲突；没有配置、或就是当前这家时为 `None`。
#[cfg(test)]
pub fn inspect_target(
    state: &AppState,
    app: &AppType,
    spec: &TargetSpec,
    dir: Option<PathBuf>,
) -> Result<Option<TargetConflict>, AppError> {
    inspect_target_with(state, app, spec, dir, None)
}

pub fn inspect_target_with(
    state: &AppState,
    app: &AppType,
    spec: &TargetSpec,
    dir: Option<PathBuf>,
    incoming_override: Option<&Provider>,
) -> Result<Option<TargetConflict>, AppError> {
    if matches!(app, AppType::OpenCode) {
        return Ok(None);
    }
    let current = match incoming_override {
        Some(provider) => provider.clone(),
        None => match current_provider(state, app)? {
            Some(provider) => provider,
            None => return Ok(None),
        },
    };
    if is_managed_login(app, &current) && spec.id != TargetId::Windows {
        // 托管登录不写 WSL：保持原配置，不需要处理冲突。
        return Ok(None);
    }
    let (live, _) = scope::run(scope_for(app, &spec.id, dir.clone()), || {
        if state.proxy_service.live_has_proxy_placeholder(app) {
            return Ok(None);
        }
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
    let providers = state.db.get_all_providers(app)?;
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
    let (live, _) = scope::run(scope_for(app, &spec.id, dir), || {
        if state.proxy_service.live_has_proxy_placeholder(app) {
            return Ok(None);
        }
        read_live_shaped(app)
    });
    let Some(live) = live? else {
        return Ok(None);
    };
    let Some(identity) = identity_from_settings(app, &live) else {
        return Ok(None);
    };
    let providers = state.db.get_all_providers(app)?;
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
    let mut provider = Provider::with_id(id.clone(), name, settings_config, None);
    provider.meta = Some(crate::provider::ProviderMeta {
        imported_from_target: Some(spec.id.key()),
        ..Default::default()
    });
    state.db.save_provider(app, &provider)?;
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

type GenerationMap = HashMap<(String, SyncChannel), u64>;

fn generations() -> &'static Mutex<GenerationMap> {
    static GENERATIONS: OnceLock<Mutex<GenerationMap>> = OnceLock::new();
    GENERATIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn generation_target_key(app: &AppType, key: &str) -> String {
    format!("{}@{}", app.as_str(), key.to_ascii_lowercase())
}

fn bump_channels(app: &AppType, key: &str, channels: &[SyncChannel]) -> Vec<(SyncChannel, u64)> {
    let mut map = generations().lock().unwrap_or_else(|e| e.into_inner());
    let target = generation_target_key(app, key);
    channels
        .iter()
        .map(|&ch| {
            let entry = map.entry((target.clone(), ch)).or_insert(0);
            *entry += 1;
            (ch, *entry)
        })
        .collect()
}

fn is_channel_latest(app: &AppType, key: &str, channel: SyncChannel, generation: u64) -> bool {
    let target = generation_target_key(app, key);
    generations()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&(target, channel))
        .is_none_or(|latest| *latest == generation)
}

fn bump_generation(app: &AppType, key: &str) -> u64 {
    bump_channels(app, key, &[SyncChannel::Provider])[0].1
}

fn is_latest(app: &AppType, key: &str, generation: u64) -> bool {
    is_channel_latest(app, key, SyncChannel::Provider, generation)
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
    primary_channel: SyncChannel,
    primary_generation: u64,
    dir: Option<String>,
    previous_provider_id: Option<String>,
    previous_fingerprint: Option<String>,
    previous_warnings: Vec<String>,
    previous_routed: bool,
    writes_provider: bool,
}

/// 在作用域里写一个目标；返回（托管字段指纹, 提示）。
fn write_in_scope(
    state: &AppState,
    app: &AppType,
    kind: SyncKind,
    current: Option<&Provider>,
    prev: Option<&Provider>,
    routed: bool,
    clear_prompt: bool,
) -> Result<(Option<String>, Vec<String>), AppError> {
    let mut warnings = Vec::new();
    // 接在路由上的环境，客户端文件是代理契约：直连供应商等退出路由时再写。
    if kind.writes_provider() && !routed {
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
        // 重试保留清空意图；若期间启用了新提示词，则投影最新内容。
        let prompt_result = (|| {
            if clear_prompt
                && !state
                    .db
                    .get_prompts(app)?
                    .values()
                    .any(|prompt| prompt.enabled)
            {
                crate::services::PromptService::clear_live_in_scope(app)
            } else {
                crate::services::PromptService::sync_to_live(state, app.clone())
            }
        })();
        if let Err(err) = prompt_result {
            if matches!(kind, SyncKind::Prompt | SyncKind::PromptClear) {
                return Err(err);
            }
            warnings.push(format!("prompt_failed:{}", error_detail(&err)));
        }
    }
    if kind.writes_skills() {
        warnings.extend(sync_skills(state, app));
    }
    Ok((fingerprint_in_scope(app), warnings))
}

pub(super) fn sync_skills(state: &AppState, app: &AppType) -> Vec<String> {
    match crate::services::SkillService::sync_to_app_with_failures(&state.db, app) {
        Ok(failed) => failed
            .into_iter()
            .map(|failure| format!("skills_failed:{}: {}", failure.directory, failure.error))
            .collect(),
        Err(err) => vec![format!("skills_failed:{err:#}")],
    }
}

/// 把 Windows 当前的供应商及共享的 MCP / 提示词 / Skills 写入 WSL。
/// `only` 限定目标（重试、新目标），`None` 为全部跟随目标。主环境不在这里写。
pub fn sync_followers(
    state: &AppState,
    app: &AppType,
    only: Option<&[TargetId]>,
    kind: SyncKind,
) -> Vec<TargetReport> {
    sync_followers_internal(state, app, only, kind, None)
}

fn sync_followers_internal(
    state: &AppState,
    app: &AppType,
    only: Option<&[TargetId]>,
    kind: SyncKind,
    prev_override: Option<&Provider>,
) -> Vec<TargetReport> {
    if !model::supports(app) || scope::active() {
        return Vec::new();
    }
    let route_lock = kind.writes_provider().then(|| route_sync_lock(app));
    let _route_guard = route_lock
        .as_ref()
        .map(|lock| lock.lock().unwrap_or_else(|e| e.into_inner()));
    let (config, source) = app_config(app);
    if source != ConfigSource::Saved || config.primary != model::WINDOWS || !config.windows.enabled
    {
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
    let providers = state.db.get_all_providers(app).unwrap_or_default();
    let additive = matches!(app, AppType::OpenCode);

    let mut jobs = Vec::new();
    for spec in followers {
        let key = spec.id.key();
        let tokens = bump_channels(app, &key, kind.channels());
        let (primary_channel, primary_generation) = tokens[0];
        if kind == SyncKind::PromptClear {
            // 解析目录和启动写入前落盘；离线、超时或失败后仍能重试清空。
            status::set_prompt_clear_pending(app.as_str(), &key, true);
        }
        let previous = status::get(app.as_str(), &key);
        let active_id = previous.as_ref().and_then(|r| r.provider_id.clone());
        // 本 PR 只做 Windows → WSL。后续独立供应商选择应新增显式配置，
        // 不要把 TargetRecord.provider_id（最近成功同步结果）当成目标的选择。
        let wanted_id = crate::mode::current::provider_for(
            &state.db,
            app,
            crate::mode::current::Purpose::Direct,
        )
        .ok()
        .flatten();
        let current = match wanted_id.as_deref() {
            Some(id) if !additive => match providers.get(id) {
                Some(provider) => Some(provider.clone()),
                None => {
                    reports.push(record(
                        app,
                        &key,
                        TargetRecord::new(TargetStatus::Error)
                            .reason("provider_missing")
                            .detail(id.to_string()),
                    ));
                    continue;
                }
            },
            _ => None,
        };
        if kind.writes_provider()
            && !follower_routed(app, &spec.id)
            && current.as_ref().is_some_and(|p| is_managed_login(app, p))
        {
            reports.push(record(
                app,
                &key,
                TargetRecord::new(TargetStatus::Skipped).reason("managed_login"),
            ));
            continue;
        }
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
        let prev = prev_override
            .filter(|provider| active_id.as_deref() == Some(provider.id.as_str()))
            .cloned()
            .or_else(|| active_id.as_ref().and_then(|id| providers.get(id)).cloned());
        let routed = follower_routed(app, &spec.id);
        let (tx, rx) = mpsc::channel();
        let done = Arc::new(Mutex::new(false));
        let worker_done = done.clone();
        let worker_state = state.clone();
        let worker_app = app.clone();
        let worker_current = current.clone();
        let worker_key = key.clone();
        let worker_dir = shown_dir.clone();
        let worker_tokens = tokens.clone();
        let mut worker_scope = scope_for(app, &spec.id, dir);
        worker_scope.direct = current.as_ref().map(|p| p.id.clone()).or(active_id.clone());
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
                    routed,
                    status::prompt_clear_pending(worker_app.as_str(), &worker_key),
                )
            });
            let previous = status::get(worker_app.as_str(), &worker_key);
            let channel_is_latest = |ch: SyncChannel| {
                worker_tokens
                    .iter()
                    .find(|(c, _)| *c == ch)
                    .is_some_and(|(_, gen)| is_channel_latest(&worker_app, &worker_key, ch, *gen))
            };
            let primary_is_latest = is_channel_latest(
                &worker_app,
                &worker_key,
                primary_channel,
                primary_generation,
            );
            if kind.writes_prompt()
                && channel_is_latest(SyncChannel::Prompt)
                && result.as_ref().is_ok_and(|(_, warnings)| {
                    !warnings
                        .iter()
                        .any(|warning| warning.starts_with("prompt_failed:"))
                })
            {
                status::set_prompt_clear_pending(worker_app.as_str(), &worker_key, false);
            }
            let retained_warnings: Vec<String> = previous
                .as_ref()
                .map(|r| {
                    r.warnings
                        .iter()
                        .filter(|w| {
                            !worker_tokens.iter().any(|(ch, gen)| {
                                ch.covers_warning(w)
                                    && is_channel_latest(&worker_app, &worker_key, *ch, *gen)
                            })
                        })
                        .cloned()
                        .collect()
                })
                .unwrap_or_default();
            let mut record = match result {
                Ok((fingerprint, mut warnings)) => {
                    warnings.extend(notes);
                    warnings.retain(|w| {
                        let owned_by_stale =
                            [SyncChannel::Mcp, SyncChannel::Prompt, SyncChannel::Skills]
                                .into_iter()
                                .any(|ch| {
                                    ch.covers_warning(w)
                                        && worker_tokens.iter().any(|(c, _)| *c == ch)
                                        && !channel_is_latest(ch)
                                });
                        !owned_by_stale
                    });
                    if kind.writes_provider() && routed {
                        // 接在共享路由上：记录跟随的 Windows 直连选择，退出路由时恢复。
                        TargetRecord {
                            provider_id: worker_current.as_ref().map(|p| p.id.clone()),
                            fingerprint,
                            dir: worker_dir,
                            warnings,
                            routed: true,
                            ..TargetRecord::new(TargetStatus::Ok).reason("routed")
                        }
                    } else if kind.writes_provider() && worker_current.is_none() && !additive {
                        // Windows 尚未选择供应商：只同步共享的 MCP 等。
                        TargetRecord {
                            dir: worker_dir,
                            warnings,
                            ..TargetRecord::new(TargetStatus::Skipped).reason("no_provider")
                        }
                    } else if kind.writes_provider() {
                        TargetRecord {
                            provider_id: worker_current.as_ref().map(|p| p.id.clone()),
                            fingerprint,
                            dir: worker_dir,
                            warnings,
                            ..TargetRecord::new(TargetStatus::Ok)
                        }
                    } else {
                        // 只写共享内容：保留供应商状态与指纹，以及本次没有覆盖的其他失败项。
                        let mut record = previous.unwrap_or_else(|| {
                            TargetRecord::new(TargetStatus::Deferred).reason("not_synced")
                        });
                        if fingerprint.is_some() {
                            record.fingerprint = fingerprint;
                        }
                        if worker_dir.is_some() {
                            record.dir = worker_dir;
                        }
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
                                SyncKind::Prompt | SyncKind::PromptClear => "prompt",
                                _ => "skills",
                            },
                            error_detail(&err)
                        )];
                        record.at = chrono::Utc::now().timestamp();
                        record
                    }
                }
            };
            let mut warnings = retained_warnings;
            warnings.append(&mut record.warnings);
            record = record.with_sync_warnings(warnings);
            let mut finished = worker_done.lock().unwrap_or_else(|e| e.into_inner());
            let report = if primary_is_latest {
                record_report(&worker_app, &worker_key, record)
            } else {
                // 已有更新的同通道操作：这次的结果过期了，不记。
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
            primary_channel,
            primary_generation,
            dir: shown_dir,
            previous_provider_id: previous.as_ref().and_then(|r| r.provider_id.clone()),
            previous_fingerprint: previous.as_ref().and_then(|r| r.fingerprint.clone()),
            previous_warnings: previous.map(|r| r.warnings).unwrap_or_default(),
            previous_routed: routed,
            writes_provider: kind.writes_provider(),
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
                // 超时：后台写入可能稍后完成（同通道代次没变时它会更新结果）。
                let report = if is_channel_latest(
                    app,
                    &job.key,
                    job.primary_channel,
                    job.primary_generation,
                ) {
                    record_report(
                        app,
                        &job.key,
                        TargetRecord {
                            provider_id: if job.writes_provider {
                                None
                            } else {
                                job.previous_provider_id.clone()
                            },
                            fingerprint: if job.writes_provider {
                                None
                            } else {
                                job.previous_fingerprint.clone()
                            },
                            dir: job.dir.clone(),
                            warnings: job.previous_warnings.clone(),
                            routed: job.previous_routed,
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
        .filter(|r| matches!(r.record.status, TargetStatus::Error | TargetStatus::Partial))
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
        if !can_read(&spec) || !external_check_due(app, &key) {
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
                &expected,
                record.status,
                TargetStatus::External,
                Some("managed_fields_changed"),
            );
        } else if actual == expected && record.status == TargetStatus::External {
            status::mark(
                app.as_str(),
                &key,
                &expected,
                record.status,
                TargetStatus::Ok,
                None,
            );
        }
    }
}

/// 供应商新增、编辑、删除之后：累加式的 OpenCode 总要同步全部环境；切换式应用只重写
/// 正在用这家的跟随环境（主环境由现有链路负责）。
pub fn after_provider_edit(state: &AppState, app: &AppType, provider_id: Option<&str>) {
    after_provider_edit_with_prev(state, app, provider_id, None, None);
}

pub fn after_provider_edit_with_prev(
    state: &AppState,
    app: &AppType,
    provider_id: Option<&str>,
    prev_override: Option<&Provider>,
    remove_opencode_id: Option<&str>,
) {
    if !has_followers(app) || scope::active() {
        return;
    }
    if matches!(app, AppType::OpenCode) {
        if let Some(old_id) = remove_opencode_id.map(str::to_string) {
            for_each_follower_in_scope(app, SyncChannel::Provider, move |_| {
                crate::opencode_config::remove_provider(&old_id)
            });
        }
        after_change(state, app, SyncKind::Provider);
        return;
    }
    let Some(provider_id) = provider_id else {
        return;
    };
    let current =
        crate::mode::current::provider_for(&state.db, app, crate::mode::current::Purpose::Direct)
            .ok()
            .flatten();
    if current.as_deref() == Some(provider_id) {
        sync_followers_internal(state, app, None, SyncKind::Provider, prev_override);
    }
}

fn for_each_follower_in_scope<F>(app: &AppType, channel: SyncChannel, op: F)
where
    F: Fn(&TargetSpec) -> Result<(), AppError> + Send + Sync,
{
    let (config, source) = app_config(app);
    if source != ConfigSource::Saved || config.primary != model::WINDOWS || !config.windows.enabled
    {
        return;
    }
    for spec in config.followers() {
        let key = spec.id.key();
        let tokens = bump_channels(app, &key, &[channel]);
        let gen = tokens[0].1;
        let Ok(dir) = resolve_dir(app, &spec, true) else {
            continue;
        };
        let shown_dir = dir
            .as_ref()
            .map(|d| d.to_string_lossy().into_owned())
            .or_else(|| display_dir(app, &spec, false));
        let (res, _) = scope::run(scope_for(app, &spec.id, dir), || {
            op(&spec)?;
            Ok::<_, AppError>(fingerprint_in_scope(app))
        });
        if !is_channel_latest(app, &key, channel, gen) {
            continue;
        }
        match res {
            Ok(fp) => {
                if let Some(mut rec) = status::get(app.as_str(), &key) {
                    if let Some(fp) = fp {
                        rec.fingerprint = Some(fp);
                    }
                    if let Some(dir) = shown_dir {
                        rec.dir = Some(dir);
                    }
                    let warnings: Vec<String> = rec
                        .warnings
                        .iter()
                        .filter(|w| !channel.covers_warning(w))
                        .cloned()
                        .collect();
                    rec = rec.with_sync_warnings(warnings);
                    status::set(app.as_str(), &key, rec);
                }
            }
            Err(err) => {
                log::warn!(
                    "[app-targets] 清理 {} 的 {key} 失败: {}",
                    app.as_str(),
                    crate::error_for_log(&err.to_string())
                );
            }
        }
    }
}

pub fn spawn_opencode_default_model(model: String) {
    if !has_followers(&AppType::OpenCode) || scope::active() {
        return;
    }
    std::thread::spawn(move || {
        for_each_follower_in_scope(&AppType::OpenCode, SyncChannel::Provider, move |_| {
            crate::opencode_config::set_default_model(&model)
        });
    });
}

pub fn spawn_remove_mcp(_state: &AppState, id: String, apps: Vec<AppType>) {
    let apps: Vec<AppType> = apps.into_iter().filter(has_followers).collect();
    if apps.is_empty() || scope::active() {
        return;
    }
    std::thread::spawn(move || {
        for app in apps {
            let server_id = id.clone();
            let app_copy = app.clone();
            for_each_follower_in_scope(&app, SyncChannel::Mcp, move |_| {
                crate::services::McpService::remove_server_from_app(&server_id, &app_copy)
            });
        }
    });
}

pub fn spawn_remove_skill(_state: &AppState, directory: String, apps: Vec<AppType>) {
    if crate::services::SkillService::require_valid_directory(&directory).is_err() {
        return;
    }
    let apps: Vec<AppType> = apps.into_iter().filter(has_followers).collect();
    if apps.is_empty() || scope::active() {
        return;
    }
    std::thread::spawn(move || {
        for app in apps {
            let dir_name = directory.clone();
            let app_copy = app.clone();
            for_each_follower_in_scope(&app, SyncChannel::Skills, move |_| {
                let skills_dir = crate::services::SkillService::get_app_skills_dir(&app_copy)
                    .map_err(|e| AppError::Message(e.to_string()))?;
                let target = skills_dir.join(&dir_name);
                if std::fs::symlink_metadata(&target).is_ok() {
                    crate::services::SkillService::remove_path(&target)
                        .map_err(|e| AppError::Message(e.to_string()))?;
                }
                Ok(())
            });
        }
    });
}

/// 正在用 `provider_id` 的已启用跟随环境。
pub fn followers_using(app: &AppType, provider_id: &str) -> Vec<TargetId> {
    let (config, source) = app_config(app);
    if source != ConfigSource::Saved || config.primary != model::WINDOWS || !config.windows.enabled
    {
        return Vec::new();
    }
    config
        .followers()
        .into_iter()
        .filter(|spec| {
            status::get(app.as_str(), &spec.id.key())
                .and_then(|r| r.provider_id)
                .as_deref()
                == Some(provider_id)
        })
        .map(|spec| spec.id)
        .collect()
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

/// 这个跟随环境的客户端文件现在是不是代理契约（以它自己的模式状态为准）。
pub fn follower_routed(app: &AppType, id: &TargetId) -> bool {
    app.spec().route_mode
        && crate::mode::state::mode_state(
            &crate::live::engine::DeviceStore::at(store_root(app, id)),
            app.as_str(),
        )
        .is_ok_and(|mode| mode.attached)
}

/// 接入路由的 WSL 目标写给客户端的代理主机：保存的优先；否则 mirrored 网络用
/// 127.0.0.1，NAT 用发行版的默认网关（Windows 主机）。Windows 目标沿用全局设置。
pub fn client_host_for(id: &TargetId) -> Result<Option<String>, AppError> {
    client_host_for_fresh(id, false)
}

pub fn client_host_for_fresh(id: &TargetId, force_probe: bool) -> Result<Option<String>, AppError> {
    let TargetId::Wsl(distro) = id else {
        return Ok(None);
    };
    if let Some(host) = crate::settings::get_proxy_wsl_client_host(distro) {
        return Ok(Some(host));
    }
    let env = wsl::resolve_network_env(distro, true, force_probe)?;
    detected_client_host(&env).map(Some).ok_or_else(|| {
        AppError::localized(
            "app_targets.client_host_unresolved",
            format!("未获取到 {distro} 连接 Windows 的地址，请在本地路由设置里填写"),
            format!(
                "Could not find how {distro} reaches Windows; enter it in the routing settings"
            ),
        )
    })
}

/// 当主环境本身是 WSL 发行版时，返回这个发行版连接 Windows 代理的地址；Windows 主环境返回 None。
pub fn primary_client_host(app: &AppType) -> Option<String> {
    if !model::supports(app) {
        return None;
    }
    let (config, source) = app_config(app);
    if source == ConfigSource::LegacyUnresolved {
        return None;
    }
    let primary = config.primary_id()?;
    matches!(primary, TargetId::Wsl(_))
        .then(|| client_host_for(&primary).ok().flatten())
        .flatten()
}

pub fn detected_client_host(env: &model::LinuxEnv) -> Option<String> {
    if env.networking_mode.as_deref() == Some("mirrored") {
        return Some("127.0.0.1".to_string());
    }
    env.gateway.clone()
}

/// WSL 与 Windows 共用路由 / 聚合服务，仅客户端主机地址因网络而异。
/// 退出路由（或 `detach`）时恢复 Windows 当前直连供应商，不保存独立 WSL 选择。
pub fn sync_follower_routes(state: &AppState, app: &AppType, detach: bool) -> Vec<TargetReport> {
    sync_follower_routes_for(state, app, detach, None)
}

/// 只重做指定环境的路由接入 / 退出；重试不能跳过之前失败的代理契约写入。
pub fn sync_follower_routes_for(
    state: &AppState,
    app: &AppType,
    detach: bool,
    only: Option<&[TargetId]>,
) -> Vec<TargetReport> {
    if !app.spec().route_mode || !model::supports(app) || scope::active() {
        return Vec::new();
    }
    let lock = route_sync_lock(app);
    let _serial = lock.lock().unwrap_or_else(|e| e.into_inner());
    let (config, source) = app_config(app);
    if source != ConfigSource::Saved || config.primary != model::WINDOWS || !config.windows.enabled
    {
        return Vec::new();
    }
    let targets = config
        .followers()
        .into_iter()
        .filter(|spec| only.is_none_or(|ids| ids.iter().any(|id| id.same(&spec.id))))
        .collect();
    sync_routes_locked(state, app, targets, detach)
}

/// 调用方持有 route_sync_lock。保存目标时传入旧目录，恢复成功后才可保存新设置。
pub(super) fn sync_routes_locked(
    state: &AppState,
    app: &AppType,
    targets: Vec<TargetSpec>,
    detach: bool,
) -> Vec<TargetReport> {
    if !app.spec().route_mode || !model::supports(app) || scope::active() {
        return Vec::new();
    }
    let main = crate::mode::current::mode_state(app);
    let main_stack = crate::mode::state::stack(
        &crate::live::engine::DeviceStore::for_device(),
        app.as_str(),
    )
    .unwrap_or_default();
    let attach = !detach && main.is_proxy() && main.attached;
    let mut reports = Vec::new();
    let mut jobs: Vec<Job> = Vec::new();
    for spec in targets {
        let key = spec.id.key();
        let join = attach; // WSL 使用 Windows 上同一个路由 / 聚合服务。
        let routed = follower_routed(app, &spec.id);
        if !join && !routed {
            continue;
        }
        let previous = status::get(app.as_str(), &key);
        let generation = bump_generation(app, &key);
        let dir = match resolve_dir(app, &spec, true) {
            Ok(dir) => dir,
            Err(err) => {
                reports.push(record(
                    app,
                    &key,
                    TargetRecord {
                        routed,
                        warnings: previous.map(|r| r.warnings).unwrap_or_default(),
                        ..TargetRecord::new(TargetStatus::Error)
                            .reason("home_unresolved")
                            .detail(error_detail(&err))
                    },
                ));
                continue;
            }
        };
        let client_host = if join {
            match client_host_for(&spec.id) {
                Ok(host) => host,
                Err(err) => {
                    reports.push(record(
                        app,
                        &key,
                        TargetRecord {
                            routed,
                            warnings: previous.map(|r| r.warnings).unwrap_or_default(),
                            ..TargetRecord::new(TargetStatus::Error)
                                .reason("client_host_unresolved")
                                .detail(error_detail(&err))
                        },
                    ));
                    continue;
                }
            }
        } else {
            None
        };
        let active = crate::mode::current::provider_for(
            &state.db,
            app,
            crate::mode::current::Purpose::Direct,
        )
        .ok()
        .flatten();
        let previous_fingerprint = previous.as_ref().and_then(|r| r.fingerprint.clone());
        let previous_warnings = previous.map(|r| r.warnings).unwrap_or_default();
        let shown_dir = dir
            .as_ref()
            .map(|d| d.to_string_lossy().into_owned())
            .or_else(|| display_dir(app, &spec, false));
        let mut scope = scope_for(app, &spec.id, dir);
        // 托管登录不能从 Windows 复制到 WSL。离开共享路由时清除代理契约，
        // 回到 WSL 自己保留的原生登录，而不是尝试把 Windows 账号写进去。
        scope.direct = active.clone().filter(|id| {
            !state
                .db
                .get_provider_by_id(id, app)
                .ok()
                .flatten()
                .is_some_and(|provider| is_managed_login(app, &provider))
        });
        scope.client_host = client_host.clone();
        let (tx, rx) = mpsc::channel();
        let done = Arc::new(Mutex::new(false));
        let worker_done = done.clone();
        let worker_state = state.clone();
        let worker_app = app.clone();
        let worker_key = key.clone();
        let worker_main = main.clone();
        let worker_stack = main_stack.clone();
        let worker_dir = shown_dir.clone();
        std::thread::spawn(move || {
            let (result, notes) = scope::run(scope, || {
                let outcome =
                    tauri::async_runtime::block_on(crate::mode::controller::reconcile_follower(
                        &worker_state,
                        &worker_app,
                        join,
                        &worker_main,
                        worker_stack,
                        detach,
                    ));
                (outcome, fingerprint_in_scope(&worker_app))
            });
            let (outcome, fingerprint) = result;
            let record = match outcome {
                Ok(routed) => {
                    let mut warnings = notes;
                    if routed
                        && listen_is_loopback(&worker_state)
                        && client_host
                            .as_deref()
                            .is_some_and(|host| !is_loopback_host(host))
                    {
                        warnings.push("listen_loopback".to_string());
                    }
                    TargetRecord {
                        fingerprint,
                        dir: worker_dir,
                        warnings,
                        routed,
                        ..TargetRecord::new(TargetStatus::Ok).reason(if routed {
                            "routed"
                        } else {
                            "direct"
                        })
                    }
                }
                Err(err) => {
                    log::warn!(
                        "[app-targets] {} 的 {worker_key} 路由写入失败: {}",
                        worker_app.as_str(),
                        crate::error_for_log(&err)
                    );
                    TargetRecord {
                        dir: worker_dir,
                        routed,
                        ..TargetRecord::new(TargetStatus::Error)
                            .reason("route_failed")
                            .detail(
                                err.lines()
                                    .next()
                                    .unwrap_or_default()
                                    .chars()
                                    .take(300)
                                    .collect::<String>(),
                            )
                    }
                }
            };
            // 路由接入成功不代表此前失败的 MCP / 提示词 / Skills 也已补写。
            let mut warnings: Vec<String> = status::get(worker_app.as_str(), &worker_key)
                .map(|r| {
                    r.warnings
                        .into_iter()
                        .filter(|w| status::is_sync_failure(w))
                        .collect()
                })
                .unwrap_or_default();
            warnings.extend(record.warnings.iter().cloned());
            let record = record.with_sync_warnings(warnings);
            let mut finished = worker_done.lock().unwrap_or_else(|e| e.into_inner());
            let report = if is_latest(&worker_app, &worker_key, generation) {
                record_report(&worker_app, &worker_key, record)
            } else {
                TargetReport {
                    target: worker_key.clone(),
                    record: TargetRecord::new(TargetStatus::Deferred).reason("superseded"),
                }
            };
            *finished = true;
            drop(finished);
            let _ = tx.send(report);
        });
        let timeout = if matches!(spec.id, TargetId::Wsl(_)) {
            WSL_TIMEOUT
        } else {
            WINDOWS_TIMEOUT
        };
        jobs.push(Job {
            key,
            timeout,
            rx,
            done,
            primary_channel: SyncChannel::Provider,
            primary_generation: generation,
            dir: shown_dir,
            previous_provider_id: active,
            previous_fingerprint,
            previous_warnings,
            previous_routed: routed,
            writes_provider: false,
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
                let report = if is_latest(app, &job.key, job.primary_generation) {
                    record_report(
                        app,
                        &job.key,
                        TargetRecord {
                            provider_id: job.previous_provider_id.clone(),
                            fingerprint: job.previous_fingerprint.clone(),
                            dir: job.dir.clone(),
                            warnings: job.previous_warnings.clone(),
                            routed: job.previous_routed,
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

fn is_loopback_host(host: &str) -> bool {
    let host = host.trim_matches(['[', ']']);
    host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
}

fn listen_is_loopback(state: &AppState) -> bool {
    tauri::async_runtime::block_on(state.db.get_listen_config())
        .map(|config| is_loopback_host(&config.listen_address))
        .unwrap_or(false)
}

/// 应用的路由状态变了之后（进入 / 退出 / 换路由 / 聚合名单 / 启动接上），在后台同步跟随
/// 环境，不让界面等 WSL。
pub fn spawn_route_sync(state: &AppState, app: &AppType) {
    if !has_followers(app) || !app.spec().route_mode || scope::active() {
        return;
    }
    let state = state.clone();
    let app = app.clone();
    std::thread::spawn(move || {
        sync_follower_routes(&state, &app, false);
    });
}

pub(super) fn route_sync_lock(app: &AppType) -> Arc<Mutex<()>> {
    static LOCKS: OnceLock<Mutex<HashMap<String, Arc<Mutex<()>>>>> = OnceLock::new();
    LOCKS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .entry(app.as_str().to_string())
        .or_default()
        .clone()
}

/// 外部修改检查限频：概览每 30 秒刷新一次，每次都读 WSL 里的文件会一直重置发行版的空闲
/// 计时，让它停不下来。同一个目标 5 分钟内只读一次。
fn external_check_due(app: &AppType, key: &str) -> bool {
    const INTERVAL: Duration = Duration::from_secs(5 * 60);
    if cfg!(test) {
        return true;
    }
    static LAST: OnceLock<Mutex<HashMap<String, Instant>>> = OnceLock::new();
    let mut last = LAST
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let id = format!("{}@{key}", app.as_str());
    let now = Instant::now();
    if last
        .get(&id)
        .is_some_and(|at| now.duration_since(*at) < INTERVAL)
    {
        return false;
    }
    last.insert(id, now);
    true
}
