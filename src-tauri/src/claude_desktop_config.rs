use serde::Serialize;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

use crate::app_config::AppType;
#[cfg(any(target_os = "macos", windows, target_os = "linux"))]
use crate::config::get_home_dir;
use crate::config::read_json_file;
use crate::database::Database;
use crate::database::CLAUDE_DESKTOP_OFFICIAL_PROVIDER_ID;
use crate::error::AppError;
use crate::live::engine::{lock_app, DeviceStore, LiveFile};
use crate::live::floor;
use crate::live::patch::json::{self as patch_json, ClearScope, JsonPatch};
use crate::live::patch::{KeyPath, LivePatch, LiveWriteError};
use crate::mode::operation::{self, FileChange};
use crate::mode::stack::DesktopStackModel;
use crate::mode::state::{self, PendingTarget};
use crate::provider::{ClaudeDesktopMode, Provider};

pub const PROFILE_ID: &str = "00000000-0000-4000-8000-000000157210";
pub const PROFILE_NAME: &str = "CC Switch";

#[cfg(any(target_os = "macos", windows, target_os = "linux", test))]
const CONFIG_FILE: &str = "claude_desktop_config.json";
#[cfg(any(target_os = "macos", windows, target_os = "linux", test))]
const CONFIG_LIBRARY_DIR: &str = "configLibrary";
const GATEWAY_TOKEN_SETTING_KEY: &str = "claude_desktop_gateway_token";
const CLAUDE_DESKTOP_PROXY_PREFIX: &str = "/claude-desktop";
pub const DEFAULT_CREATED_AT: &str = "2024-01-01T00:00:00Z";
/// `inferenceModels` 的上限（Desktop 的 schema 写着 `maxItems: 200`）。
const MAX_INFERENCE_MODELS: usize = 200;
const MIMO_REDACTED_THINKING_PLACEHOLDER: &str = "[redacted thinking]";
const MIMO_TOOL_CALL_THINKING_PLACEHOLDER: &str = "tool call";

/// Claude Desktop 模型菜单识别的 route ID 前缀。
pub const CLAUDE_ROUTE_PREFIX: &str = "claude-";
/// 替代前缀（与前端 `ANTHROPIC_CLAUDE_ROUTE_PREFIX` 一致）。
pub const ANTHROPIC_CLAUDE_ROUTE_PREFIX: &str = "anthropic/claude-";
/// Claude Code env 中通过 `[1M]` 后缀声明 1M 上下文能力（匹配用 `eq_ignore_ascii_case`）。
/// Claude Desktop schema 不接受此后缀，import 边界翻译为 `supports1m` 字段。
pub const ONE_M_CONTEXT_MARKER: &str = "[1m]";

const CURRENT_OPUS_ROUTE_ID: &str = "claude-opus-5";
const LEGACY_OPUS_ROUTE_ID: &str = "claude-opus-4-8";

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeDesktopDefaultRoute {
    pub route_id: &'static str,
    pub env_key: &'static str,
    #[serde(rename = "supports1m")]
    pub supports_1m: bool,
}

pub const DEFAULT_PROXY_ROUTES: &[ClaudeDesktopDefaultRoute] = &[
    ClaudeDesktopDefaultRoute {
        route_id: "claude-sonnet-5",
        env_key: "ANTHROPIC_DEFAULT_SONNET_MODEL",
        supports_1m: true,
    },
    ClaudeDesktopDefaultRoute {
        route_id: CURRENT_OPUS_ROUTE_ID,
        env_key: "ANTHROPIC_DEFAULT_OPUS_MODEL",
        supports_1m: true,
    },
    ClaudeDesktopDefaultRoute {
        route_id: "claude-haiku-4-5",
        env_key: "ANTHROPIC_DEFAULT_HAIKU_MODEL",
        supports_1m: true,
    },
    // fable 置于末尾：next_catalog_safe_route_id 给非安全品牌 route 借用合法
    // 角色名时仍按 sonnet→opus→haiku 顺序分配（向后兼容既有 catalog），不会把
    // 无关品牌模型借用成 fable 顶配档名。UI 行序由前端 ROLE_ORDER 独立控制为
    // Sonnet/Opus/Fable/Haiku（所有 proxy 路径都经 normalizeProxyRows 重排），
    // 与此处物理顺序无关。
    ClaudeDesktopDefaultRoute {
        route_id: "claude-fable-5",
        env_key: "ANTHROPIC_DEFAULT_FABLE_MODEL",
        supports_1m: true,
    },
];

#[derive(Debug, Clone)]
struct ClaudeDesktopPaths {
    normal_config_path: PathBuf,
    threep_config_path: PathBuf,
    config_library_path: PathBuf,
    profile_path: PathBuf,
    meta_path: PathBuf,
    /// 写前意图和首写备份所在的设备目录。
    device: DeviceStore,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectGatewayCredentials {
    pub base_url: String,
    pub api_key: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeDesktopStatus {
    pub supported: bool,
    /// 本机装了 Claude Desktop（推断，见 `detect_installed`）；接入了也算
    pub installed: bool,
    pub configured: bool,
    pub applied_id: Option<String>,
    pub profile_path: Option<String>,
    pub config_library_path: Option<String>,
    pub mode: Option<ClaudeDesktopMode>,
    pub expected_base_url: Option<String>,
    pub actual_base_url: Option<String>,
    pub proxy_running: bool,
    pub stale_raw_models: bool,
    pub missing_route_mappings: bool,
    pub gateway_token_configured: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedModelRoute {
    pub route_id: String,
    pub upstream_model: String,
    pub label_override: Option<String>,
    pub supports_1m: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct InferenceModelSpec {
    name: String,
    label_override: Option<String>,
    supports_1m: bool,
}

/// 直连模式下把 `provider` 写进 Desktop 的配置（同步、编辑当前那家）。模型映射卡要经路由，
/// 直连写不进去（见 [`needs_routing`]）。
pub fn apply_provider(db: &Database, provider: &Provider) -> Result<(), AppError> {
    write_direct(
        db,
        state::op::APPLY,
        Some(provider),
        PendingTarget::default(),
    )
}

/// 写进 Desktop 配置的内容。
pub enum DesktopTarget<'a> {
    /// 官方：两个配置文件改回 `1p`，摘掉 CC Switch 的 profile。
    Official,
    /// 第三方网关 profile（直连那家的，或指向本地路由的）。
    Profile(&'a Value),
}

/// 直连模式下要写的东西：`None` 和官方卡写回官方。
pub fn direct_target(provider: Option<&Provider>) -> Result<Option<Value>, AppError> {
    match provider {
        Some(provider) if !is_official_provider(provider) => direct_profile(provider).map(Some),
        _ => Ok(None),
    }
}

/// 直连写入：`provider` 为 `None` 或官方卡时写回官方。文件和 `target`（指针、模式状态）在
/// 同一个操作里提交。
pub fn write_direct(
    db: &Database,
    op: &str,
    provider: Option<&Provider>,
    target: PendingTarget,
) -> Result<(), AppError> {
    let profile = direct_target(provider)?;
    let write = match &profile {
        Some(profile) => DesktopTarget::Profile(profile),
        None => DesktopTarget::Official,
    };
    run(db, op, Some(write), target)
}

/// 写 Desktop 的配置文件，和 `target` 在同一个操作里提交。`write` 为 `None` 时只落定
/// `target`（契约没变，客户端文件不用动）。
pub fn run(
    db: &Database,
    op: &str,
    write: Option<DesktopTarget<'_>>,
    target: PendingTarget,
) -> Result<(), AppError> {
    if write.is_none() {
        // 不碰文件，平台不支持时也能落定模式。
        return operation::commit_target(
            db,
            &DeviceStore::for_device(),
            AppType::ClaudeDesktop.as_str(),
            &target,
        );
    }
    let paths = current_platform_paths()?;
    run_at_paths(db, &paths, op, write, target)
}

fn run_at_paths(
    db: &Database,
    paths: &ClaudeDesktopPaths,
    op: &str,
    write: Option<DesktopTarget<'_>>,
    target: PendingTarget,
) -> Result<(), AppError> {
    match write {
        None => write_desktop_files(db, paths, op, &[], target),
        Some(DesktopTarget::Official) => restore_official_at_paths(db, paths, op, target),
        Some(DesktopTarget::Profile(profile)) => {
            write_profile_at_paths(db, paths, op, profile.clone(), target)
        }
    }
}

pub fn get_status(db: &Database, proxy_running: bool) -> Result<ClaudeDesktopStatus, AppError> {
    if !is_supported_platform() {
        return Ok(ClaudeDesktopStatus {
            supported: false,
            installed: false,
            configured: false,
            applied_id: None,
            profile_path: None,
            config_library_path: None,
            mode: None,
            expected_base_url: None,
            actual_base_url: None,
            proxy_running,
            stale_raw_models: false,
            missing_route_mappings: false,
            gateway_token_configured: false,
        });
    }

    let paths = current_platform_paths()?;
    let applied_id = read_applied_id(&paths.meta_path);
    let profile = read_json_or_empty(&paths.profile_path).unwrap_or_else(|_| json!({}));
    // 切回官方后 profile 文件保留、只清关键字段，所以不能按文件在不在判断。
    let configured =
        meta_has_profile_entry(&paths.meta_path) || profile.get("inferenceProvider").is_some();
    let actual_base_url = profile
        .get("inferenceGatewayBaseUrl")
        .and_then(Value::as_str)
        .map(str::to_string);
    let stale_raw_models = profile
        .get("inferenceModels")
        .and_then(Value::as_array)
        .map(|models| {
            models.iter().any(|item| {
                item.as_str()
                    .or_else(|| item.get("name").and_then(Value::as_str))
                    .is_some_and(|model| !is_claude_safe_model_id(model))
            })
        })
        .unwrap_or(false);
    let gateway_token_configured = db
        .get_setting(GATEWAY_TOKEN_SETTING_KEY)
        .ok()
        .flatten()
        .is_some_and(|token| !token.trim().is_empty());
    // 路由模式下看路由那家，直连模式下看直连那家。
    let routed = crate::mode::current::is_proxy(&AppType::ClaudeDesktop);
    let current_provider = crate::mode::current::provider_in_use(db, &AppType::ClaudeDesktop)
        .ok()
        .flatten()
        .filter(|provider| !is_official_provider(provider));
    let mode = if routed {
        Some(ClaudeDesktopMode::Proxy)
    } else {
        current_provider.as_ref().map(|_| ClaudeDesktopMode::Direct)
    };
    let expected_base_url = match mode {
        Some(ClaudeDesktopMode::Proxy) => proxy_gateway_base_url_from_db(db).ok(),
        Some(ClaudeDesktopMode::Direct) => current_provider
            .as_ref()
            .and_then(|provider| direct_gateway_credentials(provider).ok())
            .map(|credentials| credentials.base_url),
        None => None,
    };
    let missing_route_mappings = routed
        && current_provider
            .as_ref()
            .is_some_and(|provider| proxy_model_routes(provider).is_err());

    Ok(ClaudeDesktopStatus {
        supported: true,
        installed: configured || detect_installed(&paths, &macos_app_bundle_candidates()),
        configured,
        applied_id,
        profile_path: Some(paths.profile_path.display().to_string()),
        config_library_path: Some(paths.config_library_path.display().to_string()),
        mode,
        expected_base_url,
        actual_base_url,
        proxy_running,
        stale_raw_models,
        missing_route_mappings,
        gateway_token_configured,
    })
}

pub fn get_config_library_path() -> Result<PathBuf, AppError> {
    Ok(current_platform_paths()?.config_library_path)
}

pub fn default_proxy_routes() -> Vec<ClaudeDesktopDefaultRoute> {
    DEFAULT_PROXY_ROUTES.to_vec()
}

pub fn is_compatible_direct_provider(provider: &Provider) -> bool {
    validate_direct_provider(provider).is_ok()
}

pub fn is_official_provider(provider: &Provider) -> bool {
    provider.id == CLAUDE_DESKTOP_OFFICIAL_PROVIDER_ID
}

/// 这张卡在直连模式下写不进去，要经本地路由：模型映射卡（上游模型名 Desktop 不认）、托管
/// OAuth（凭据由路由注入）。和前端 `providerNeedsRouting`、托盘的 `provider_needs_routing`
/// 是同一条规则。卡片上的 `claudeDesktopMode` 只说明这张卡是哪种，不再决定 Desktop 的模式
/// （模式在 `live-state.json`，见 `mode::controller`）。
pub fn needs_routing(provider: &Provider) -> bool {
    !is_official_provider(provider)
        && (matches!(provider_mode(provider), ClaudeDesktopMode::Proxy)
            || is_managed_oauth_proxy_provider(provider))
}

/// 升级前旧版的状态：直连指针是模型映射卡（旧版按卡片决定模式，这张卡生效时 Desktop 指着
/// 本地路由）。启动时据此把 Desktop 定为路由模式、路由到这张卡。
pub fn legacy_mapping_card(db: &Database) -> Option<Provider> {
    if current_platform_paths().is_err() {
        return None;
    }
    crate::mode::current::direct_provider(db, &AppType::ClaudeDesktop)
        .ok()
        .flatten()
        .filter(|provider| {
            !is_official_provider(provider)
                && matches!(provider_mode(provider), ClaudeDesktopMode::Proxy)
        })
}

pub fn provider_mode(provider: &Provider) -> ClaudeDesktopMode {
    provider
        .meta
        .as_ref()
        .and_then(|meta| meta.claude_desktop_mode.clone())
        .unwrap_or(ClaudeDesktopMode::Direct)
}

pub fn is_claude_safe_model_id(model: &str) -> bool {
    let normalized = model.trim().to_ascii_lowercase();
    if normalized.contains(ONE_M_CONTEXT_MARKER) {
        return false;
    }

    let Some(route_tail) = normalized
        .strip_prefix(ANTHROPIC_CLAUDE_ROUTE_PREFIX)
        .or_else(|| normalized.strip_prefix(CLAUDE_ROUTE_PREFIX))
    else {
        return false;
    };

    // 角色前缀后必须还有实际模型标识，拒绝 claude-sonnet- 这类退化值
    // （否则会写入 profile 并触发 Claude Desktop fail-all 拒收整组）。
    // Claude Desktop 1.12603.1+ 的 fail-all validator 角色白名单已纳入 fable
    // （app.asar 内 ["sonnet","opus","haiku","fable","mythos"]），故 claude-fable-*
    // 可安全写入 profile。mythos 官方未公开发布，暂不暴露给用户。
    ["sonnet-", "opus-", "haiku-", "fable-"]
        .iter()
        .any(|prefix| {
            route_tail
                .strip_prefix(prefix)
                .is_some_and(|rest| !rest.is_empty())
        })
}

fn inference_model_json(spec: &InferenceModelSpec) -> Value {
    if spec.supports_1m || spec.label_override.is_some() {
        let mut item = json!({ "name": spec.name });
        if let Some(label_override) = spec.label_override.as_deref() {
            item["labelOverride"] = json!(label_override);
        }
        if spec.supports_1m {
            item["supports1m"] = json!(true);
            // 只有 `supports1m` 时 Desktop 每个模型列两行（原名 + 「1M context window」）。
            // 加上 `prefer1m`（1.21459.0 起）只列原名，交给内嵌 CLI 时自动补 `[1m]`。
            item["prefer1m"] = json!(true);
        }
        item
    } else {
        Value::String(spec.name.clone())
    }
}

pub fn get_or_create_gateway_token(db: &Database) -> Result<String, AppError> {
    if let Some(token) = db.get_setting(GATEWAY_TOKEN_SETTING_KEY)? {
        let trimmed = token.trim();
        if !trimmed.is_empty() {
            return Ok(trimmed.to_string());
        }
    }

    let token = format!("ccs-{}", uuid::Uuid::new_v4().simple());
    db.set_setting(GATEWAY_TOKEN_SETTING_KEY, &token)?;
    Ok(token)
}

pub fn direct_gateway_credentials(
    provider: &Provider,
) -> Result<DirectGatewayCredentials, AppError> {
    let env = provider
        .settings_config
        .get("env")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            AppError::localized(
                "claude_desktop.provider.env_missing",
                "Claude Desktop 直连供应商缺少 env 配置",
                "Claude Desktop direct provider is missing env configuration",
            )
        })?;

    let base_url = env
        .get("ANTHROPIC_BASE_URL")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            AppError::localized(
                "claude_desktop.provider.base_url_missing",
                "Claude Desktop 直连供应商缺少 ANTHROPIC_BASE_URL",
                "Claude Desktop direct provider is missing ANTHROPIC_BASE_URL",
            )
        })?
        .to_string();

    let api_key = env
        .get("ANTHROPIC_AUTH_TOKEN")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            AppError::localized(
                "claude_desktop.provider.auth_token_missing",
                "Claude Desktop 直连供应商缺少 ANTHROPIC_AUTH_TOKEN（Bearer Token）",
                "Claude Desktop direct provider is missing ANTHROPIC_AUTH_TOKEN (Bearer Token)",
            )
        })?
        .to_string();

    Ok(DirectGatewayCredentials { base_url, api_key })
}

pub fn validate_direct_provider(provider: &Provider) -> Result<(), AppError> {
    if is_official_provider(provider) {
        return Ok(());
    }

    if !provider.settings_config.is_object() {
        return Err(AppError::localized(
            "claude_desktop.provider.settings_not_object",
            "Claude Desktop 直连供应商配置必须是 JSON 对象",
            "Claude Desktop direct provider configuration must be a JSON object",
        ));
    }

    if let Some(meta) = provider.meta.as_ref() {
        if let Some(api_format) = meta.api_format.as_deref() {
            if !api_format.trim().is_empty() && api_format != "anthropic" {
                return Err(AppError::localized(
                    "claude_desktop.provider.api_format_unsupported",
                    "Claude Desktop 第一阶段只支持原生 Anthropic Messages API",
                    "Claude Desktop phase 1 only supports native Anthropic Messages API",
                ));
            }
        }

        if matches!(
            meta.claude_desktop_mode.as_ref(),
            Some(ClaudeDesktopMode::Proxy)
        ) {
            return Err(AppError::localized(
                "claude_desktop.provider.mode_unsupported",
                "该供应商是 Claude Desktop 本地路由模式，不能按直连模式写入",
                "This Claude Desktop provider uses proxy mode and cannot be written as direct mode",
            ));
        }

        if matches!(
            meta.provider_type.as_deref(),
            Some("github_copilot") | Some("codex_oauth") | Some("xai_oauth")
        ) {
            return Err(AppError::localized(
                "claude_desktop.provider.type_unsupported",
                "Claude Desktop 直连模式不支持需要本地代理转换的供应商",
                "Claude Desktop direct mode does not support providers that require local proxy conversion",
            ));
        }

        if meta.is_full_url == Some(true) {
            return Err(AppError::localized(
                "claude_desktop.provider.full_url_unsupported",
                "Claude Desktop 直连模式不支持完整 URL 端点配置",
                "Claude Desktop direct mode does not support full URL endpoint configuration",
            ));
        }
    }

    direct_inference_model_specs(provider)?;
    direct_gateway_credentials(provider)?;
    Ok(())
}

pub fn validate_proxy_provider(provider: &Provider) -> Result<(), AppError> {
    if is_official_provider(provider) {
        return Ok(());
    }

    if !provider.settings_config.is_object() {
        return Err(AppError::localized(
            "claude_desktop.provider.settings_not_object",
            "Claude Desktop 本地路由供应商配置必须是 JSON 对象",
            "Claude Desktop proxy provider configuration must be a JSON object",
        ));
    }

    if let Some(meta) = provider.meta.as_ref() {
        if let Some(api_format) = meta.api_format.as_deref() {
            if !matches!(
                api_format,
                "" | "anthropic" | "openai_chat" | "openai_responses" | "gemini_native"
            ) {
                return Err(AppError::localized(
                    "claude_desktop.provider.api_format_unsupported",
                    format!("Claude Desktop 本地路由模式不支持 API 格式: {api_format}"),
                    format!("Claude Desktop proxy mode does not support API format: {api_format}"),
                ));
            }
        }
    }

    proxy_model_routes(provider)?;

    if !has_proxy_base_url_and_key(provider) {
        return Err(AppError::localized(
            "claude_desktop.provider.credentials_missing",
            "Claude Desktop 本地路由供应商缺少 Base URL 或 API Key",
            "Claude Desktop proxy provider is missing Base URL or API key",
        ));
    }

    Ok(())
}

fn has_proxy_base_url_and_key(provider: &Provider) -> bool {
    let env = provider.settings_config.get("env");
    let has_base_url = env
        .and_then(|value| value.get("ANTHROPIC_BASE_URL"))
        .or_else(|| provider.settings_config.get("base_url"))
        .or_else(|| provider.settings_config.get("baseURL"))
        .or_else(|| provider.settings_config.get("apiEndpoint"))
        .and_then(Value::as_str)
        .map(str::trim)
        .is_some_and(|value| !value.is_empty());

    if is_managed_oauth_proxy_provider(provider) {
        return has_base_url;
    }

    let has_key = env
        .and_then(|value| {
            [
                "ANTHROPIC_AUTH_TOKEN",
                "ANTHROPIC_API_KEY",
                "OPENROUTER_API_KEY",
                "OPENAI_API_KEY",
                "GEMINI_API_KEY",
            ]
            .into_iter()
            .find_map(|key| value.get(key))
        })
        .or_else(|| provider.settings_config.get("apiKey"))
        .or_else(|| provider.settings_config.get("api_key"))
        .and_then(Value::as_str)
        .map(str::trim)
        .is_some_and(|value| !value.is_empty());

    has_base_url && has_key
}

fn is_managed_oauth_proxy_provider(provider: &Provider) -> bool {
    provider
        .meta
        .as_ref()
        .and_then(|meta| meta.provider_type.as_deref())
        .is_some_and(|provider_type| {
            matches!(
                provider_type,
                "github_copilot" | "codex_oauth" | "xai_oauth"
            )
        })
}

pub fn validate_provider(provider: &Provider) -> Result<(), AppError> {
    if is_official_provider(provider) {
        return Ok(());
    }

    match provider_mode(provider) {
        ClaudeDesktopMode::Direct => validate_direct_provider(provider),
        ClaudeDesktopMode::Proxy => validate_proxy_provider(provider),
    }
}

fn direct_inference_model_specs(provider: &Provider) -> Result<Vec<InferenceModelSpec>, AppError> {
    let Some(routes) = provider
        .meta
        .as_ref()
        .map(|meta| &meta.claude_desktop_model_routes)
    else {
        return Ok(Vec::new());
    };

    let mut result = Vec::new();
    for (route_id, route) in routes {
        let supports_1m = route.supports_1m.unwrap_or(false);
        let route_id = route_id.trim();
        if route_id.is_empty() {
            continue;
        }
        if !is_claude_safe_model_id(route_id) {
            return Err(AppError::localized(
                "claude_desktop.provider.route_invalid",
                format!(
                    "Claude Desktop 直连模型必须使用 claude-* 或 anthropic/claude-* 名称: {route_id}"
                ),
                format!(
                    "Claude Desktop direct model must use a claude-* or anthropic/claude-* name: {route_id}"
                ),
            ));
        }
        let upstream_model = route.model.trim();
        if !upstream_model.is_empty() && upstream_model != route_id {
            return Err(AppError::localized(
                "claude_desktop.provider.direct_mapping_unsupported",
                format!(
                    "Claude Desktop 直连模式不能映射模型: {route_id} -> {upstream_model}；非 Claude 官方模型请使用本地路由模式"
                ),
                format!(
                    "Claude Desktop direct mode cannot map models: {route_id} -> {upstream_model}; use proxy mode for non-Claude official models"
                ),
            ));
        }
        result.push(InferenceModelSpec {
            name: route_id.to_string(),
            label_override: route
                .label_override
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string),
            supports_1m,
        });
    }

    // Sort supports_1m=true first within each name so the subsequent dedup_by
    // (which keeps the first occurrence) preserves the 1M-capable variant.
    result.sort_by(|a, b| {
        a.name
            .cmp(&b.name)
            .then_with(|| b.supports_1m.cmp(&a.supports_1m))
    });
    result.dedup_by(|a, b| a.name == b.name);
    Ok(result)
}

fn uses_default_proxy_routes(provider: &Provider) -> bool {
    !is_official_provider(provider)
        && !needs_routing(provider)
        && provider
            .meta
            .as_ref()
            .is_none_or(|meta| meta.claude_desktop_model_routes.is_empty())
}

pub fn proxy_model_routes(provider: &Provider) -> Result<Vec<ResolvedModelRoute>, AppError> {
    // 原生直连卡允许留空模型列表。先用同名角色路由进入代理，之后才能保存模型映射，
    // 否则「先开路由」和「先存映射」会互相阻挡。显式映射卡仍须配置模型。
    if uses_default_proxy_routes(provider) {
        validate_direct_provider(provider)?;
        let mut routes = DEFAULT_PROXY_ROUTES
            .iter()
            .map(|route| ResolvedModelRoute {
                route_id: route.route_id.to_string(),
                upstream_model: route.route_id.to_string(),
                label_override: None,
                supports_1m: false,
            })
            .collect::<Vec<_>>();
        routes.sort_by(|a, b| a.route_id.cmp(&b.route_id));
        return Ok(routes);
    }

    configured_proxy_model_routes(provider)
}

/// 聚合成员只能发布用户配置过的模型，不能把路由引导用的默认角色当成上游目录。
pub(crate) fn configured_proxy_model_routes(
    provider: &Provider,
) -> Result<Vec<ResolvedModelRoute>, AppError> {
    let routes = provider
        .meta
        .as_ref()
        .map(|meta| &meta.claude_desktop_model_routes)
        .ok_or_else(|| {
            AppError::localized(
                "claude_desktop.provider.routes_missing",
                "Claude Desktop 本地路由模式缺少模型路由映射",
                "Claude Desktop proxy mode is missing model route mappings",
            )
        })?;

    let reserved_route_ids = routes
        .keys()
        .map(|route_id| route_id.trim())
        .filter(|route_id| is_claude_safe_model_id(route_id))
        .map(str::to_string)
        .collect::<std::collections::HashSet<_>>();
    let mut result = Vec::new();
    let mut entries = routes.iter().collect::<Vec<_>>();
    entries.sort_by_key(|(left, _)| *left);
    for (route_id, route) in entries {
        let supports_1m = route.supports_1m.unwrap_or(false);
        let route_id = route_id.trim();
        let upstream_model = route.model.trim();
        if route_id.is_empty() || upstream_model.is_empty() {
            continue;
        }
        let repaired_route_id = if is_claude_safe_model_id(route_id) {
            route_id.to_string()
        } else {
            next_catalog_safe_route_id(&result, &reserved_route_ids)
        };
        result.push(ResolvedModelRoute {
            route_id: published_route_id(repaired_route_id, upstream_model, &reserved_route_ids),
            upstream_model: upstream_model.to_string(),
            label_override: route
                .label_override
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .or_else(|| {
                    (!is_claude_safe_model_id(route_id)).then(|| upstream_model.to_string())
                }),
            supports_1m,
        });
    }

    result.sort_by(|a, b| a.route_id.cmp(&b.route_id));
    result.dedup_by(|a, b| a.route_id == b.route_id);

    if result.is_empty() {
        return Err(AppError::localized(
            "claude_desktop.provider.routes_missing",
            "Claude Desktop 本地路由模式至少需要一个模型路由映射",
            "Claude Desktop proxy mode requires at least one model route mapping",
        ));
    }

    Ok(result)
}

/// 发布给 Desktop 的模型名。Desktop 按这个名字决定请求参数：映射表里的 Opus / Sonnet 键固定是
/// `claude-opus-5` / `claude-sonnet-5`，目标是 5.5 时照写，5.5 上游会拒收按 5 组的请求（#8082）。
/// 和 Claude Code 的路由契约一样换成目录里的 5.5 名字；映射表里的键不变，旧 profile 发来的
/// 原键仍按键查到目标（见 [`legacy_raw_route_upstream_model`]）。用户自己配了这个名字的路由
/// 时不换，免得两条撞名。
fn published_route_id(
    route_id: String,
    upstream_model: &str,
    reserved: &std::collections::HashSet<String>,
) -> String {
    crate::live::project::claude::point_release_client_id(&route_id, upstream_model)
        .filter(|catalog_id| !reserved.contains(*catalog_id))
        .map_or(route_id, str::to_string)
}

fn next_catalog_safe_route_id(
    existing: &[ResolvedModelRoute],
    reserved: &std::collections::HashSet<String>,
) -> String {
    if let Some(default_route) = DEFAULT_PROXY_ROUTES
        .iter()
        .map(|route| route.route_id)
        .find(|route_id| {
            !reserved.contains(*route_id)
                && !existing.iter().any(|route| route.route_id == *route_id)
        })
    {
        return default_route.to_string();
    }

    let mut index = 2usize;
    loop {
        let route_id = format!("{}-r{index}", DEFAULT_PROXY_ROUTES[0].route_id);
        if !reserved.contains(&route_id) && !existing.iter().any(|route| route.route_id == route_id)
        {
            return route_id;
        }
        index += 1;
    }
}

pub fn model_list_response(provider: &Provider) -> Result<Value, AppError> {
    let routes = proxy_model_routes(provider)?;
    let data: Vec<Value> = routes
        .iter()
        .map(|route| {
            let model_id = route.route_id.clone();
            let mut item = json!({
                "type": "model",
                "id": model_id,
                "created_at": DEFAULT_CREATED_AT,
            });
            if route.supports_1m {
                item["supports1m"] = json!(true);
            }
            item
        })
        .collect();
    let first_id = data
        .first()
        .and_then(|item| item.get("id"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let last_id = data
        .last()
        .and_then(|item| item.get("id"))
        .and_then(Value::as_str)
        .map(str::to_string);

    Ok(json!({
        "data": data,
        "has_more": false,
        "first_id": first_id,
        "last_id": last_id,
    }))
}

pub fn map_proxy_request_model(mut body: Value, provider: &Provider) -> Result<Value, AppError> {
    let requested_raw = body
        .get("model")
        .and_then(Value::as_str)
        .map(str::trim)
        .map(str::to_string)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            AppError::localized(
                "claude_desktop.provider.model_missing",
                "Claude Desktop 请求缺少 model 字段",
                "Claude Desktop request is missing the model field",
            )
        })?;
    let requested = strip_one_m_suffix_for_route_lookup(&requested_raw);

    let routes = proxy_model_routes(provider)?;
    // 默认列表只提供进入路由时的角色菜单，不把原生的日期后缀或版本别名改成默认 ID。
    if uses_default_proxy_routes(provider) && is_claude_safe_model_id(requested) {
        body["model"] = json!(requested);
        return Ok(normalize_upstream_request(body, provider));
    }
    let upstream_model = routes
        .iter()
        .find(|r| r.route_id == requested)
        .or_else(|| {
            routes
                .iter()
                .find(|r| is_compatible_opus_route_alias(&r.route_id, requested))
        })
        .map(|route| route.upstream_model.clone())
        .or_else(|| legacy_raw_route_upstream_model(provider, requested))
        .or_else(|| {
            // 角色关键词回落:Claude Desktop 的部分调用(如子 agent)会请求带发布
            // 日期后缀的完整官方名(claude-haiku-4-5-20251001),与 manifest 暴露的
            // 简短 route_id(claude-haiku-4-5)不精确相等。按 opus/haiku/fable/sonnet
            // 归类到同档已配置路由,对齐 Claude Code model_mapper 的宽松匹配。
            // 匹配前已剥离本地 [1m] 标记；这里仍只对 Claude Desktop 认可的
            // 安全模型名回落，避免非 Claude route 被误映射。
            if !is_claude_safe_model_id(requested) {
                return None;
            }
            let role = claude_role_keyword(requested)?;
            routes
                .iter()
                .find(|route| claude_role_keyword(&route.route_id) == Some(role))
                // 老用户只配了 Sonnet/Opus/Haiku 三档时，fable 请求降级到 opus 档，
                // 与官方安全分类器的降级方向一致，避免 route_unknown 硬错误。
                // 用户一旦显式配置 fable 档，上面的精确角色匹配会优先命中。
                .or_else(|| {
                    (role == "fable")
                        .then(|| {
                            routes
                                .iter()
                                .find(|route| claude_role_keyword(&route.route_id) == Some("opus"))
                        })
                        .flatten()
                })
                .map(|route| route.upstream_model.clone())
        })
        .ok_or_else(|| {
            AppError::localized(
                "claude_desktop.provider.route_unknown",
                format!("Claude Desktop 模型路由未配置: {requested_raw}"),
                format!("Claude Desktop model route is not configured: {requested_raw}"),
            )
        })?;

    body["model"] = json!(upstream_model);
    Ok(normalize_upstream_request(body, provider))
}

/// 请求体的 `model` 已经是上游名（路由映射过，或聚合模型直接解出了上游名）之后，按上游的
/// 怪癖做的整理：目前只有 MiMo 的 Anthropic 接口要补 thinking 历史。
pub fn normalize_upstream_request(mut body: Value, provider: &Provider) -> Value {
    let upstream_model = body
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    if should_normalize_mimo_anthropic_thinking_history(provider, &upstream_model) {
        normalize_mimo_anthropic_thinking_history(&mut body);
    }
    body
}

fn strip_one_m_suffix_for_route_lookup(model: &str) -> &str {
    let trimmed = model.trim();
    let marker = ONE_M_CONTEXT_MARKER.as_bytes();
    let bytes = trimmed.as_bytes();
    if bytes.len() >= marker.len()
        && bytes[bytes.len() - marker.len()..].eq_ignore_ascii_case(marker)
    {
        return trimmed[..trimmed.len() - marker.len()].trim_end();
    }
    trimmed
}

fn legacy_raw_route_upstream_model(provider: &Provider, requested: &str) -> Option<String> {
    provider
        .meta
        .as_ref()?
        .claude_desktop_model_routes
        .iter()
        .find(|(route_id, _)| route_id.trim() == requested)
        .and_then(|(_, route)| {
            let upstream_model = route.model.trim();
            (!upstream_model.is_empty()).then(|| upstream_model.to_string())
        })
}

fn is_compatible_opus_route_alias(route_id: &str, requested: &str) -> bool {
    matches!(
        (route_id, requested),
        (CURRENT_OPUS_ROUTE_ID, LEGACY_OPUS_ROUTE_ID)
            | (LEGACY_OPUS_ROUTE_ID, CURRENT_OPUS_ROUTE_ID)
    )
}

/// 按角色关键词(opus / haiku / fable / sonnet)归类一个 Claude 模型名/route_id。
/// 仅在命中明确角色词时返回 Some,未知模型返回 None(不回落,保持精确报错语义)。
/// 与前端 `routeRoleFromId` 同序(opus → haiku → fable → sonnet)。
fn claude_role_keyword(model: &str) -> Option<&'static str> {
    let normalized = model.to_ascii_lowercase();
    if normalized.contains("opus") {
        Some("opus")
    } else if normalized.contains("haiku") {
        Some("haiku")
    } else if normalized.contains("fable") {
        Some("fable")
    } else if normalized.contains("sonnet") {
        Some("sonnet")
    } else {
        None
    }
}

fn should_normalize_mimo_anthropic_thinking_history(
    provider: &Provider,
    upstream_model: &str,
) -> bool {
    if !provider_uses_anthropic_messages_format(provider) {
        return false;
    }

    is_mimo_identifier(upstream_model) || provider_has_mimo_endpoint(provider)
}

fn provider_uses_anthropic_messages_format(provider: &Provider) -> bool {
    let api_format = provider
        .meta
        .as_ref()
        .and_then(|meta| meta.api_format.as_deref())
        .or_else(|| {
            provider
                .settings_config
                .get("api_format")
                .and_then(Value::as_str)
        })
        .map(str::trim)
        .unwrap_or("anthropic");

    api_format.is_empty() || api_format == "anthropic"
}

fn provider_has_mimo_endpoint(provider: &Provider) -> bool {
    let settings = &provider.settings_config;
    [
        settings
            .get("env")
            .and_then(|env| env.get("ANTHROPIC_BASE_URL"))
            .and_then(Value::as_str),
        settings.get("base_url").and_then(Value::as_str),
        settings.get("baseURL").and_then(Value::as_str),
        settings.get("apiEndpoint").and_then(Value::as_str),
    ]
    .into_iter()
    .flatten()
    .any(is_mimo_identifier)
}

fn is_mimo_identifier(value: &str) -> bool {
    let value = value.to_ascii_lowercase();
    value.contains("mimo") || value.contains("xiaomimimo")
}

fn normalize_mimo_anthropic_thinking_history(body: &mut Value) {
    let Some(messages) = body.get_mut("messages").and_then(Value::as_array_mut) else {
        return;
    };

    for message in messages {
        if message.get("role").and_then(Value::as_str) != Some("assistant") {
            continue;
        }

        let Some(content) = message.get_mut("content").and_then(Value::as_array_mut) else {
            continue;
        };
        if !content
            .iter()
            .any(|block| block.get("type").and_then(Value::as_str) == Some("tool_use"))
        {
            continue;
        }

        let mut has_thinking = false;
        for block in content.iter_mut() {
            match block.get("type").and_then(Value::as_str) {
                Some("thinking") => {
                    let has_non_empty_thinking = block
                        .get("thinking")
                        .and_then(Value::as_str)
                        .is_some_and(|value| !value.trim().is_empty());
                    if let Some(obj) = block.as_object_mut() {
                        obj.remove("signature");
                    }
                    if has_non_empty_thinking {
                        has_thinking = true;
                    } else if let Some(obj) = block.as_object_mut() {
                        obj.insert(
                            "thinking".to_string(),
                            json!(MIMO_TOOL_CALL_THINKING_PLACEHOLDER),
                        );
                        has_thinking = true;
                    }
                }
                Some("redacted_thinking") => {
                    *block = json!({
                        "type": "thinking",
                        "thinking": MIMO_REDACTED_THINKING_PLACEHOLDER
                    });
                    has_thinking = true;
                }
                _ => {}
            }
        }

        if !has_thinking {
            content.insert(
                0,
                json!({
                    "type": "thinking",
                    "thinking": MIMO_TOOL_CALL_THINKING_PLACEHOLDER
                }),
            );
        }
    }
}

pub fn proxy_gateway_base_url_from_db(db: &Database) -> Result<String, AppError> {
    let (host, port) = db.get_proxy_client_endpoint_sync();
    if port == 0 {
        return Err(AppError::Config(
            "Claude Desktop 代理地址需要真实监听端口；请先启动本地代理或使用固定端口".to_string(),
        ));
    }
    Ok(format!(
        "{}{}",
        crate::services::proxy::proxy_origin(&host, port),
        CLAUDE_DESKTOP_PROXY_PREFIX
    ))
}

/// 直连那家的网关 profile：地址和 Key 直接写进去。模型映射卡、托管 OAuth 要经路由，拒绝。
fn direct_profile(provider: &Provider) -> Result<Value, AppError> {
    validate_direct_provider(provider)?;
    let credentials = direct_gateway_credentials(provider)?;
    let model_specs = direct_inference_model_specs(provider)?;
    Ok(build_gateway_profile(
        &credentials.base_url,
        &credentials.api_key,
        (!model_specs.is_empty()).then_some(model_specs.as_slice()),
    ))
}

/// 本地路由的 `/claude-desktop` 入口。`proxy_url` 是路由服务的地址（不带路径）。
pub fn gateway_base_url(proxy_url: &str) -> String {
    format!(
        "{}{}",
        proxy_url.trim_end_matches('/'),
        CLAUDE_DESKTOP_PROXY_PREFIX
    )
}

/// 路由模式的网关 profile：指向本地路由、带 gateway token，模型列表是路由那家的模型表
/// （模型映射卡的映射表，直连卡的模型列表）。换路由那家，模型列表跟着变，要重写文件、
/// 重启 Desktop。
///
/// 聚合模式下 `stack` 是其他几家发布的模型，排在路由那家后面（第一项是默认模型，仍是路由
/// 那家的），并关掉模型发现：列表完全由这里决定。Desktop 最多收 200 条，多出的不写。
pub fn route_profile(
    db: &Database,
    route: &Provider,
    proxy_url: &str,
    stack: &[DesktopStackModel],
) -> Result<Value, AppError> {
    if is_official_provider(route) {
        return Err(AppError::localized(
            "claude_desktop.provider.official_not_routable",
            "Claude Desktop 官方账号不经过本地路由",
            "The Claude Desktop official account does not go through local routing",
        ));
    }
    validate_proxy_provider(route)?;
    let mut model_specs = proxy_model_routes(route)?
        .into_iter()
        .map(|route| InferenceModelSpec {
            name: route.route_id,
            label_override: route.label_override,
            supports_1m: route.supports_1m,
        })
        .chain(stack.iter().map(|model| InferenceModelSpec {
            name: model.id.clone(),
            label_override: Some(model.label.clone()),
            supports_1m: model.supports_1m,
        }))
        .collect::<Vec<_>>();
    if model_specs.len() > MAX_INFERENCE_MODELS {
        log::warn!(
            "Claude Desktop 最多 {MAX_INFERENCE_MODELS} 个模型，聚合的模型共 {} 个，多出的不写入",
            model_specs.len()
        );
        model_specs.truncate(MAX_INFERENCE_MODELS);
    }
    let mut profile = build_gateway_profile(
        &gateway_base_url(proxy_url),
        &get_or_create_gateway_token(db)?,
        Some(model_specs.as_slice()),
    );
    if !stack.is_empty() {
        profile["modelDiscoveryEnabled"] = json!(false);
    }
    Ok(profile)
}

fn write_profile_at_paths(
    db: &Database,
    paths: &ClaudeDesktopPaths,
    op: &str,
    profile: Value,
    target: PendingTarget,
) -> Result<(), AppError> {
    let deployment = deployment_mode_patch("3p");
    let profile = gateway_profile_patch(profile);
    let meta = MetaPatch { applied: true };
    write_desktop_files(
        db,
        paths,
        op,
        &[
            FileChange {
                file: LiveFile::shared(&paths.normal_config_path),
                patch: &deployment,
            },
            FileChange {
                file: LiveFile::shared(&paths.threep_config_path),
                patch: &deployment,
            },
            FileChange {
                file: LiveFile::private(&paths.profile_path),
                patch: &profile,
            },
            FileChange {
                file: LiveFile::shared(&paths.meta_path),
                patch: &meta,
            },
        ],
        target,
    )
}

/// 切回官方：两个配置文件改回 `1p`，清掉旧版写进 `enterpriseConfig` 的网关设置，
/// 把 CC Switch 的 profile 从 `_meta.json` 里摘掉。
///
/// profile 文件本身保留、只清关键字段：用户在 Desktop 里对这个 profile 改的设置
/// （#4774 的 auto 模式）下次切回第三方时还在，Key 也从磁盘上清掉了。Desktop 只按
/// `_meta.json` 的条目读 profile，摘掉条目后这个文件不会被列出。
fn restore_official_at_paths(
    db: &Database,
    paths: &ClaudeDesktopPaths,
    op: &str,
    target: PendingTarget,
) -> Result<(), AppError> {
    let normal = deployment_mode_patch("1p");
    let mut threep = deployment_mode_patch("1p");
    threep.remove = LEGACY_ENTERPRISE_GATEWAY_KEYS
        .iter()
        .map(|key| KeyPath::new(&["enterpriseConfig", key]))
        .collect();
    let threep = DropEmptyEnterpriseConfig(threep);
    let profile = JsonPatch {
        clear: vec![ClearScope {
            parent: KeyPath::root(),
            is_floor: floor::desktop_profile_floor,
        }],
        ..JsonPatch::default()
    };
    let meta = MetaPatch { applied: false };

    let mut changes = vec![
        FileChange {
            file: LiveFile::shared(&paths.normal_config_path),
            patch: &normal,
        },
        FileChange {
            file: LiveFile::shared(&paths.threep_config_path),
            patch: &threep,
        },
        FileChange {
            file: LiveFile::shared(&paths.meta_path),
            patch: &meta,
        },
    ];
    if paths.profile_path.exists() {
        changes.push(FileChange {
            file: LiveFile::private(&paths.profile_path),
            patch: &profile,
        });
    }
    write_desktop_files(db, paths, op, &changes, target)
}

/// Desktop 的几个文件作为一次操作写入，`target`（直连指针、模式状态）在文件都写完之后
/// 落定：任何一个解析失败都不写；写到一半失败或崩溃，下次写入或启动时按写前意图补完。
fn write_desktop_files(
    db: &Database,
    paths: &ClaudeDesktopPaths,
    op: &str,
    changes: &[FileChange<'_>],
    target: PendingTarget,
) -> Result<(), AppError> {
    let app = AppType::ClaudeDesktop.as_str();
    let guard = lock_app(app);
    operation::run(&paths.device, &guard, op, changes, target, &|target| {
        operation::commit_target(db, &paths.device, app, target)
    })?;
    Ok(())
}

fn deployment_mode_patch(mode: &str) -> JsonPatch {
    JsonPatch {
        set: vec![(KeyPath::new(&["deploymentMode"]), json!(mode))],
        ..JsonPatch::default()
    }
}

/// 旧版写进 `claude_desktop_config.json` 的 `enterpriseConfig` 的网关设置。
const LEGACY_ENTERPRISE_GATEWAY_KEYS: &[&str] = &[
    "disableDeploymentModeChooser",
    "inferenceGatewayApiKey",
    "inferenceGatewayAuthScheme",
    "inferenceGatewayBaseUrl",
    "inferenceProvider",
];

/// 清完旧网关设置后，`enterpriseConfig` 空了就整个删掉。
struct DropEmptyEnterpriseConfig(JsonPatch);

impl LivePatch for DropEmptyEnterpriseConfig {
    fn apply(&self, path: &Path, pre: Option<&[u8]>) -> Result<Vec<u8>, LiveWriteError> {
        let (mut doc, style) = patch_json::parse(path, pre)?;
        self.0.apply_to(path, &mut doc)?;
        if let Some(obj) = doc.as_object_mut() {
            if obj
                .get("enterpriseConfig")
                .and_then(Value::as_object)
                .is_some_and(|enterprise| enterprise.is_empty())
            {
                obj.shift_remove("enterpriseConfig");
            }
        }
        patch_json::serialize(path, &doc, &style)
    }
}

/// `build_gateway_profile` 的结果拆成补丁：关键字段替换，策略键缺失时才写。
fn gateway_profile_patch(profile: Value) -> JsonPatch {
    let mut patch = JsonPatch {
        clear: vec![ClearScope {
            parent: KeyPath::root(),
            is_floor: floor::desktop_profile_floor,
        }],
        ..JsonPatch::default()
    };
    if let Value::Object(fields) = profile {
        for (key, value) in fields {
            let key_path = KeyPath::new(&[&key]);
            if floor::DESKTOP_PROFILE_SEED.contains(&key.as_str()) {
                patch.seed.push((key_path, value));
            } else {
                patch.set.push((key_path, value));
            }
        }
    }
    patch
}

/// `configLibrary/_meta.json`：`entries` 里登记 CC Switch 的 profile，`appliedId`
/// 指向当前生效的那个。已有条目原位更新，不挪位置。
struct MetaPatch {
    applied: bool,
}

impl LivePatch for MetaPatch {
    fn apply(&self, path: &Path, pre: Option<&[u8]>) -> Result<Vec<u8>, LiveWriteError> {
        let (mut doc, style) = patch_json::parse(path, pre)?;
        let obj = doc.as_object_mut().expect("parse guarantees an object");
        let not_an_array = || LiveWriteError::Shape {
            path: path.to_path_buf(),
            key_path: KeyPath::new(&["entries"]),
            expected: "数组",
        };
        let is_ours = |entry: &Value| entry.get("id").and_then(Value::as_str) == Some(PROFILE_ID);

        if self.applied {
            let entries = obj
                .entry("entries")
                .or_insert_with(|| Value::Array(Vec::new()))
                .as_array_mut()
                .ok_or_else(not_an_array)?;
            match entries.iter_mut().find(|entry| is_ours(entry)) {
                Some(Value::Object(entry)) => {
                    entry.insert("name".to_string(), json!(PROFILE_NAME));
                }
                _ => entries.push(json!({ "id": PROFILE_ID, "name": PROFILE_NAME })),
            }
            match obj.get_mut("appliedId") {
                Some(slot) => *slot = json!(PROFILE_ID),
                None => {
                    obj.insert("appliedId".to_string(), json!(PROFILE_ID));
                }
            }
        } else {
            let next_id = match obj.get_mut("entries") {
                Some(entries) => {
                    let entries = entries.as_array_mut().ok_or_else(not_an_array)?;
                    entries.retain(|entry| !is_ours(entry));
                    entries
                        .iter()
                        .find_map(|entry| entry.get("id").and_then(Value::as_str))
                        .map(str::to_string)
                }
                None => None,
            };
            if obj.get("appliedId").and_then(Value::as_str) == Some(PROFILE_ID) {
                match next_id {
                    Some(next_id) => {
                        obj.insert("appliedId".to_string(), json!(next_id));
                    }
                    None => {
                        obj.shift_remove("appliedId");
                    }
                }
            }
        }

        patch_json::serialize(path, &doc, &style)
    }
}

fn build_gateway_profile(
    base_url: &str,
    api_key: &str,
    model_specs: Option<&[InferenceModelSpec]>,
) -> Value {
    let mut profile = json!({
        "coworkEgressAllowedHosts": ["*"],
        "disableDeploymentModeChooser": true,
        "inferenceGatewayApiKey": api_key,
        "inferenceGatewayAuthScheme": "bearer",
        "inferenceGatewayBaseUrl": base_url,
        "inferenceProvider": "gateway"
    });

    if let Some(model_specs) = model_specs {
        profile["inferenceModels"] =
            Value::Array(model_specs.iter().map(inference_model_json).collect());
    }

    profile
}

fn read_json_or_empty(path: &Path) -> Result<Value, AppError> {
    let value = if path.exists() {
        read_json_file(path)?
    } else {
        json!({})
    };

    if value.is_object() {
        Ok(value)
    } else {
        Ok(json!({}))
    }
}

fn read_applied_id(path: &Path) -> Option<String> {
    read_json_or_empty(path).ok().and_then(|value| {
        value
            .get("appliedId")
            .and_then(Value::as_str)
            .map(str::to_string)
    })
}

fn meta_has_profile_entry(path: &Path) -> bool {
    read_json_or_empty(path)
        .ok()
        .and_then(|value| value.get("entries").and_then(Value::as_array).cloned())
        .is_some_and(|entries| {
            entries
                .iter()
                .any(|entry| entry.get("id").and_then(Value::as_str) == Some(PROFILE_ID))
        })
}

/// Claude Desktop 没有可调的命令，只能推断装没装：它第一次启动就会建自己的数据目录
/// （`claude_desktop_config.json` 所在的 `Claude`，不看 CC Switch 也会写的 `Claude-3p`），
/// macOS 上另认应用包。
fn detect_installed(paths: &ClaudeDesktopPaths, app_bundles: &[PathBuf]) -> bool {
    paths.normal_config_path.parent().is_some_and(Path::is_dir)
        || app_bundles.iter().any(|bundle| bundle.is_dir())
}

fn macos_app_bundle_candidates() -> Vec<PathBuf> {
    if cfg!(target_os = "macos") {
        vec![
            PathBuf::from("/Applications/Claude.app"),
            get_home_dir().join("Applications").join("Claude.app"),
        ]
    } else {
        Vec::new()
    }
}

fn is_supported_platform() -> bool {
    cfg!(any(target_os = "macos", windows, target_os = "linux"))
}

#[allow(clippy::needless_return)]
fn current_platform_paths() -> Result<ClaudeDesktopPaths, AppError> {
    #[cfg(target_os = "macos")]
    {
        return Ok(macos_paths_from_home(&get_home_dir()));
    }

    #[cfg(windows)]
    {
        let local_app_data = windows_local_app_data_dir();
        return Ok(windows_paths_from_local_app_data(&local_app_data));
    }

    #[cfg(target_os = "linux")]
    {
        return Ok(linux_paths_from_config_dir(&linux_config_dir()));
    }

    #[cfg(not(any(target_os = "macos", windows, target_os = "linux")))]
    {
        Err(unsupported_platform_error())
    }
}

/// Flatpak keeps an app's XDG_CONFIG_HOME inside its sandbox. Claude Desktop
/// installed natively uses the host's ~/.config by default, which must be
/// exposed through the Flatpak filesystem permissions.
///
/// This is intentionally the host *default* configuration directory. We do
/// not expose a directory override or attempt to recover a host-custom
/// XDG_CONFIG_HOME: Flatpak replaces that variable with its private path, so
/// its original host value is not available reliably from the sandbox. Users
/// with a custom host XDG_CONFIG_HOME should run the native CC Switch package.
#[cfg(target_os = "linux")]
fn linux_config_dir() -> PathBuf {
    let xdg_config_home = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from);
    linux_config_dir_from_home(&get_home_dir(), xdg_config_home.as_deref(), is_flatpak())
}

#[cfg(any(target_os = "linux", all(test, unix)))]
fn linux_config_dir_from_home(
    home: &Path,
    xdg_config_home: Option<&Path>,
    running_in_flatpak: bool,
) -> PathBuf {
    if running_in_flatpak {
        return home.join(".config");
    }

    xdg_config_home
        .filter(|path| path.is_absolute())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| home.join(".config"))
}

#[cfg(target_os = "linux")]
fn is_flatpak() -> bool {
    Path::new("/.flatpak-info").is_file()
}

#[cfg(target_os = "linux")]
fn linux_paths_from_config_dir(config_dir: &Path) -> ClaudeDesktopPaths {
    paths_from_dirs(config_dir.join("Claude"), config_dir.join("Claude-3p"))
}

#[cfg(target_os = "macos")]
fn macos_paths_from_home(home: &Path) -> ClaudeDesktopPaths {
    let app_support = home.join("Library").join("Application Support");
    paths_from_dirs(app_support.join("Claude"), app_support.join("Claude-3p"))
}

#[cfg(windows)]
fn windows_local_app_data_dir() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| get_home_dir().join("AppData").join("Local"))
}

#[cfg(windows)]
fn windows_paths_from_local_app_data(local_app_data: &Path) -> ClaudeDesktopPaths {
    let normal_dir = pick_windows_claude_dir(local_app_data, false)
        .unwrap_or_else(|| local_app_data.join("Claude"));
    let threep_dir = pick_windows_claude_dir(local_app_data, true)
        .unwrap_or_else(|| local_app_data.join("Claude-3p"));
    paths_from_dirs(normal_dir, threep_dir)
}

#[cfg(windows)]
fn pick_windows_claude_dir(local_app_data: &Path, threep: bool) -> Option<PathBuf> {
    let exact_name = if threep { "Claude-3p" } else { "Claude" };
    let exact = local_app_data.join(exact_name);
    if exact.exists() {
        return Some(exact);
    }

    let mut candidates: Vec<PathBuf> = std::fs::read_dir(local_app_data)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .filter(|path| {
            let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
                return false;
            };
            let starts = name.starts_with("Claude");
            let is_threep = name.contains("-3p");
            starts && is_threep == threep
        })
        .collect();
    candidates.sort();
    candidates.into_iter().next()
}

#[cfg(any(target_os = "macos", windows, target_os = "linux", test))]
fn paths_from_dirs(normal_dir: PathBuf, threep_dir: PathBuf) -> ClaudeDesktopPaths {
    let config_library_path = threep_dir.join(CONFIG_LIBRARY_DIR);
    let profile_path = config_library_path.join(format!("{PROFILE_ID}.json"));
    let meta_path = config_library_path.join("_meta.json");

    ClaudeDesktopPaths {
        normal_config_path: normal_dir.join(CONFIG_FILE),
        threep_config_path: threep_dir.join(CONFIG_FILE),
        config_library_path,
        profile_path,
        meta_path,
        device: DeviceStore::for_device(),
    }
}

#[cfg(not(any(target_os = "macos", windows, target_os = "linux")))]
fn unsupported_platform_error() -> AppError {
    AppError::localized(
        "claude_desktop.unsupported_platform",
        "当前平台暂不支持 Claude Desktop 3P 配置。支持的平台：macOS、Windows 和 Linux。",
        "Claude Desktop 3P configuration is not supported on this platform yet. Supported platforms: macOS, Windows, and Linux.",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::write_json_file;
    use crate::database::Database;
    use crate::provider::{ClaudeDesktopModelRoute, ProviderMeta};
    use serde_json::json;
    use std::fs;
    use tempfile::TempDir;

    fn test_paths(home: &Path) -> ClaudeDesktopPaths {
        let mut paths = paths_from_dirs(
            home.join("Library")
                .join("Application Support")
                .join("Claude"),
            home.join("Library")
                .join("Application Support")
                .join("Claude-3p"),
        );
        paths.device = DeviceStore::at(home.join(".cc-switch"));
        paths
    }

    /// 按卡片写一次：模型映射卡写成路由 profile（路由模式下路由到它），其余直连。
    fn apply_provider_to_paths(
        db: &Database,
        provider: &Provider,
        paths: &ClaudeDesktopPaths,
    ) -> Result<(), AppError> {
        let profile = if is_official_provider(provider) {
            None
        } else if needs_routing(provider) {
            let gateway = proxy_gateway_base_url_from_db(db)?;
            let origin = gateway
                .strip_suffix(CLAUDE_DESKTOP_PROXY_PREFIX)
                .unwrap_or(&gateway);
            Some(route_profile(db, provider, origin, &[])?)
        } else {
            Some(direct_profile(provider)?)
        };
        let write = match &profile {
            Some(profile) => DesktopTarget::Profile(profile),
            None => DesktopTarget::Official,
        };
        run_at_paths(
            db,
            paths,
            state::op::APPLY,
            Some(write),
            PendingTarget::default(),
        )
    }

    fn restore_official(paths: &ClaudeDesktopPaths) -> Result<(), AppError> {
        restore_official_at_paths(
            &test_db(),
            paths,
            state::op::APPLY,
            PendingTarget::default(),
        )
    }

    #[cfg(any(target_os = "linux", all(test, unix)))]
    #[test]
    fn linux_config_dir_uses_absolute_xdg_config_home_outside_flatpak() {
        let home = Path::new("/home/tester");
        let xdg = Path::new("/mnt/config");

        assert_eq!(
            linux_config_dir_from_home(home, Some(xdg), false),
            PathBuf::from("/mnt/config")
        );
    }

    #[cfg(any(target_os = "linux", all(test, unix)))]
    #[test]
    fn linux_config_dir_falls_back_for_missing_or_relative_xdg_config_home() {
        let home = Path::new("/home/tester");

        assert_eq!(
            linux_config_dir_from_home(home, None, false),
            PathBuf::from("/home/tester/.config")
        );
        assert_eq!(
            linux_config_dir_from_home(home, Some(Path::new("relative/config")), false),
            PathBuf::from("/home/tester/.config")
        );
    }

    #[cfg(any(target_os = "linux", all(test, unix)))]
    #[test]
    fn linux_config_dir_uses_host_config_when_cc_switch_runs_in_flatpak() {
        let home = Path::new("/home/tester");
        let private_xdg = Path::new("/home/tester/.var/app/com.ccswitch.desktop/config");

        assert_eq!(
            linux_config_dir_from_home(home, Some(private_xdg), true),
            PathBuf::from("/home/tester/.config")
        );
    }

    fn test_db() -> Database {
        Database::memory().expect("memory db")
    }

    fn set_proxy_port(db: &Database, port: u16) {
        let config = crate::proxy::types::ProxyConfig {
            listen_port: port,
            ..Default::default()
        };
        futures::executor::block_on(db.update_proxy_config(config)).expect("update proxy config");
    }

    fn direct_provider(id: &str) -> Provider {
        let mut provider = Provider::with_id(
            id.to_string(),
            "Direct".to_string(),
            json!({
                "env": {
                    "ANTHROPIC_BASE_URL": "https://gateway.example.com",
                    "ANTHROPIC_AUTH_TOKEN": "test-token",
                    "ANTHROPIC_MODEL": "ignored-by-desktop"
                }
            }),
            Some("https://example.com".to_string()),
        );
        provider.meta = Some(ProviderMeta {
            api_format: Some("anthropic".to_string()),
            ..Default::default()
        });
        provider
    }

    #[test]
    fn proxy_gateway_base_url_rejects_unresolved_ephemeral_port() {
        let db = test_db();
        set_proxy_port(&db, 0);

        let err = proxy_gateway_base_url_from_db(&db)
            .expect_err("unresolved ephemeral port should not produce a :0 URL");
        assert!(
            err.to_string().contains("真实监听端口"),
            "unexpected error: {err}"
        );
    }

    fn official_provider() -> Provider {
        let mut provider = Provider::with_id(
            CLAUDE_DESKTOP_OFFICIAL_PROVIDER_ID.to_string(),
            "Claude Desktop Official".to_string(),
            json!({"env": {}}),
            Some("https://claude.ai/download".to_string()),
        );
        provider.category = Some("official".to_string());
        provider
    }

    fn proxy_provider(id: &str) -> Provider {
        let mut provider = direct_provider(id);
        provider.name = "Proxy".to_string();
        provider.meta = Some(ProviderMeta {
            claude_desktop_mode: Some(ClaudeDesktopMode::Proxy),
            api_format: Some("openai_chat".to_string()),
            claude_desktop_model_routes: std::collections::HashMap::from([(
                "claude-sonnet-4-6".to_string(),
                ClaudeDesktopModelRoute {
                    model: "kimi-k2".to_string(),
                    label_override: Some("Kimi K2".to_string()),
                    supports_1m: Some(true),
                },
            )]),
            ..Default::default()
        });
        provider
    }

    fn mimo_anthropic_proxy_provider(id: &str) -> Provider {
        let mut provider = direct_provider(id);
        provider.name = "MiMo Proxy".to_string();
        provider.settings_config = json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://api.xiaomimimo.com/anthropic",
                "ANTHROPIC_AUTH_TOKEN": "test-token"
            }
        });
        provider.meta = Some(ProviderMeta {
            claude_desktop_mode: Some(ClaudeDesktopMode::Proxy),
            api_format: Some("anthropic".to_string()),
            claude_desktop_model_routes: std::collections::HashMap::from([(
                "claude-sonnet-4-6".to_string(),
                ClaudeDesktopModelRoute {
                    model: "mimo-v2.5-pro".to_string(),
                    label_override: Some("MiMo v2.5 Pro".to_string()),
                    supports_1m: Some(true),
                },
            )]),
            ..Default::default()
        });
        provider
    }

    fn oauth_proxy_provider(id: &str, provider_type: &str, api_format: &str) -> Provider {
        let mut provider = Provider::with_id(
            id.to_string(),
            "OAuth Proxy".to_string(),
            json!({
                "env": {
                    "ANTHROPIC_BASE_URL": "https://oauth-upstream.example.com"
                }
            }),
            Some("https://example.com".to_string()),
        );
        provider.meta = Some(ProviderMeta {
            claude_desktop_mode: Some(ClaudeDesktopMode::Proxy),
            api_format: Some(api_format.to_string()),
            provider_type: Some(provider_type.to_string()),
            claude_desktop_model_routes: std::collections::HashMap::from([(
                "claude-sonnet-4-6".to_string(),
                ClaudeDesktopModelRoute {
                    model: "gpt-5.4".to_string(),
                    label_override: Some("GPT-5.4".to_string()),
                    supports_1m: Some(false),
                },
            )]),
            ..Default::default()
        });
        provider
    }

    fn direct_provider_with_models(id: &str) -> Provider {
        let mut provider = direct_provider(id);
        provider.meta = Some(ProviderMeta {
            claude_desktop_mode: Some(ClaudeDesktopMode::Direct),
            api_format: Some("anthropic".to_string()),
            claude_desktop_model_routes: std::collections::HashMap::from([(
                "claude-sonnet-4-6".to_string(),
                ClaudeDesktopModelRoute {
                    model: "claude-sonnet-4-6".to_string(),
                    label_override: None,
                    supports_1m: Some(true),
                },
            )]),
            ..Default::default()
        });
        provider
    }

    #[test]
    fn detect_installed_reads_data_dir_or_app_bundle() {
        let temp = TempDir::new().expect("tempdir");
        let paths = test_paths(temp.path());
        let bundle = temp.path().join("Claude.app");
        assert!(!detect_installed(&paths, std::slice::from_ref(&bundle)));

        // 只有 CC Switch 写的 Claude-3p 不算
        fs::create_dir_all(paths.threep_config_path.parent().unwrap()).unwrap();
        assert!(!detect_installed(&paths, std::slice::from_ref(&bundle)));

        fs::create_dir_all(&bundle).unwrap();
        assert!(detect_installed(&paths, std::slice::from_ref(&bundle)));

        fs::create_dir_all(paths.normal_config_path.parent().unwrap()).unwrap();
        assert!(detect_installed(&paths, &[]));
    }

    #[test]
    fn claude_desktop_apply_writes_3p_profile_and_meta() {
        let temp = TempDir::new().expect("tempdir");
        let paths = test_paths(temp.path());
        let provider = direct_provider("direct");
        let db = test_db();

        apply_provider_to_paths(&db, &provider, &paths).expect("apply provider");

        let normal: Value = read_json_file(&paths.normal_config_path).expect("read normal config");
        let threep: Value = read_json_file(&paths.threep_config_path).expect("read 3p config");
        let profile: Value = read_json_file(&paths.profile_path).expect("read profile");
        let meta: Value = read_json_file(&paths.meta_path).expect("read meta");

        assert_eq!(normal["deploymentMode"], json!("3p"));
        assert_eq!(threep["deploymentMode"], json!("3p"));
        assert_eq!(profile["inferenceProvider"], json!("gateway"));
        assert_eq!(
            profile["inferenceGatewayBaseUrl"],
            json!("https://gateway.example.com")
        );
        assert_eq!(profile["inferenceGatewayApiKey"], json!("test-token"));
        assert_eq!(profile["inferenceGatewayAuthScheme"], json!("bearer"));
        assert_eq!(profile["disableDeploymentModeChooser"], json!(true));
        assert_eq!(profile["coworkEgressAllowedHosts"], json!(["*"]));
        assert!(profile.get("inferenceModels").is_none());
        assert_eq!(meta["appliedId"], json!(PROFILE_ID));
        assert!(meta["entries"]
            .as_array()
            .expect("entries")
            .iter()
            .any(|entry| entry["id"] == json!(PROFILE_ID) && entry["name"] == json!(PROFILE_NAME)));
    }

    #[test]
    fn claude_desktop_direct_can_write_optional_safe_model_ids() {
        let temp = TempDir::new().expect("tempdir");
        let paths = test_paths(temp.path());
        let provider = direct_provider_with_models("direct-models");
        let db = test_db();

        apply_provider_to_paths(&db, &provider, &paths).expect("apply provider");

        let profile: Value = read_json_file(&paths.profile_path).expect("read profile");
        assert_eq!(
            profile["inferenceGatewayBaseUrl"],
            json!("https://gateway.example.com")
        );
        assert_eq!(
            profile["inferenceModels"],
            json!([{ "name": "claude-sonnet-4-6", "supports1m": true, "prefer1m": true }])
        );
    }

    #[test]
    fn claude_desktop_direct_rejects_model_mapping_to_non_claude_upstream() {
        let mut provider = direct_provider_with_models("direct-non-claude");
        provider
            .meta
            .as_mut()
            .expect("meta")
            .claude_desktop_model_routes
            .get_mut("claude-sonnet-4-6")
            .expect("route")
            .model = "mimo-v2.5-pro".to_string();

        let err = validate_provider(&provider).expect_err("direct mapping should fail");
        assert!(err.to_string().contains("本地路由模式"));
    }

    #[test]
    fn claude_desktop_native_provider_without_catalog_can_start_routing() {
        for with_meta in [false, true] {
            let mut provider = direct_provider("native");
            if !with_meta {
                provider.meta = None;
            } else {
                provider.meta.as_mut().unwrap().claude_desktop_mode =
                    Some(ClaudeDesktopMode::Direct);
            }
            let before = serde_json::to_value(&provider).expect("provider snapshot");
            let db = test_db();
            let profile = route_profile(&db, &provider, "http://127.0.0.1:15721", &[])
                .expect("native provider should not need a pre-existing mapping");

            assert_eq!(
                profile["inferenceGatewayBaseUrl"],
                json!("http://127.0.0.1:15721/claude-desktop")
            );
            assert_eq!(
                profile["inferenceModels"],
                json!([
                    "claude-fable-5",
                    "claude-haiku-4-5",
                    "claude-opus-5",
                    "claude-sonnet-5"
                ])
            );
            let models = model_list_response(&provider).expect("local model discovery");
            configured_proxy_model_routes(&provider)
                .expect_err("bootstrap defaults must not become a configured catalog");
            assert_eq!(models["data"].as_array().expect("models").len(), 4);
            assert!(models["data"]
                .as_array()
                .unwrap()
                .iter()
                .all(|model| model.get("supports1m").is_none()));
            assert_eq!(serde_json::to_value(&provider).unwrap(), before);
            assert!(direct_profile(&provider)
                .expect("direct profile remains valid")
                .get("inferenceModels")
                .is_none());
        }
    }

    #[test]
    fn claude_desktop_native_default_routes_preserve_requested_model_versions() {
        let provider = direct_provider("native");
        for model in [
            "claude-haiku-4-5-20251001",
            "claude-opus-4-8",
            "anthropic/claude-sonnet-4-6",
        ] {
            let mapped = map_proxy_request_model(
                json!({"model": format!("{model} [1M]"), "messages": []}),
                &provider,
            )
            .expect("native model should pass through without a version rewrite");
            assert_eq!(mapped["model"], json!(model));
        }
        map_proxy_request_model(json!({"model": "unconfigured-model"}), &provider)
            .expect_err("default native routes must not accept arbitrary model names");
    }

    #[test]
    fn claude_desktop_mapping_provider_still_requires_explicit_routes() {
        let mut provider = proxy_provider("mapping");
        provider
            .meta
            .as_mut()
            .expect("meta")
            .claude_desktop_model_routes
            .clear();

        let error = validate_proxy_provider(&provider).expect_err("mapping is required");
        assert!(error.to_string().contains("至少需要一个模型路由映射"));
        route_profile(&test_db(), &provider, "http://127.0.0.1:15721", &[])
            .expect_err("an empty mapping must not silently become native defaults");
    }

    #[test]
    fn claude_desktop_default_routes_do_not_bypass_native_provider_validation() {
        let mut provider = direct_provider("invalid-native");
        provider.meta.as_mut().expect("meta").api_format = Some("openai_chat".to_string());
        proxy_model_routes(&provider).expect_err("native defaults require native API format");

        provider.meta.as_mut().expect("meta").api_format = Some("anthropic".to_string());
        provider.settings_config["env"]["ANTHROPIC_AUTH_TOKEN"] = json!("");
        proxy_model_routes(&provider).expect_err("native defaults require credentials");
    }

    #[test]
    fn claude_desktop_proxy_apply_writes_local_gateway_profile_with_safe_models() {
        let temp = TempDir::new().expect("tempdir");
        let paths = test_paths(temp.path());
        let provider = proxy_provider("proxy");
        let db = test_db();

        apply_provider_to_paths(&db, &provider, &paths).expect("apply proxy provider");

        let profile: Value = read_json_file(&paths.profile_path).expect("read profile");
        assert_eq!(
            profile["inferenceGatewayBaseUrl"],
            json!("http://127.0.0.1:15721/claude-desktop")
        );
        assert_eq!(profile["inferenceGatewayAuthScheme"], json!("bearer"));
        assert_eq!(profile["coworkEgressAllowedHosts"], json!(["*"]));
        assert_ne!(profile["inferenceGatewayApiKey"], json!("test-token"));
        assert!(profile["inferenceGatewayApiKey"]
            .as_str()
            .expect("gateway token")
            .starts_with("ccs-"));
        assert_eq!(
            profile["inferenceModels"],
            json!([{ "name": "claude-sonnet-4-6", "labelOverride": "Kimi K2", "supports1m": true, "prefer1m": true }])
        );
        assert!(!profile.to_string().contains("kimi-k2"));
    }

    #[test]
    fn claude_desktop_proxy_accepts_managed_oauth_providers_without_static_key() {
        for (provider_type, api_format) in [
            ("github_copilot", "openai_chat"),
            ("codex_oauth", "openai_responses"),
            ("xai_oauth", "openai_responses"),
        ] {
            let provider = oauth_proxy_provider(provider_type, provider_type, api_format);
            validate_proxy_provider(&provider).expect("oauth proxy provider should validate");

            let temp = TempDir::new().expect("tempdir");
            let paths = test_paths(temp.path());
            let db = test_db();
            apply_provider_to_paths(&db, &provider, &paths).expect("apply oauth proxy provider");

            let profile: Value = read_json_file(&paths.profile_path).expect("read profile");
            assert_eq!(
                profile["inferenceGatewayBaseUrl"],
                json!("http://127.0.0.1:15721/claude-desktop")
            );
            assert_eq!(
                profile["inferenceModels"],
                json!([{ "name": "claude-sonnet-4-6", "labelOverride": "GPT-5.4" }])
            );
        }
    }

    #[test]
    fn claude_desktop_proxy_maps_known_route_and_rejects_unknown_route() {
        let provider = proxy_provider("proxy");

        let mapped = map_proxy_request_model(
            json!({"model": "claude-sonnet-4-6", "messages": []}),
            &provider,
        )
        .expect("map route");
        assert_eq!(mapped["model"], json!("kimi-k2"));

        let models = model_list_response(&provider).expect("model list");
        assert_eq!(models["data"][0]["id"], json!("claude-sonnet-4-6"));
        assert_eq!(models["data"][0]["supports1m"], json!(true));

        let err = map_proxy_request_model(json!({"model": "claude-opus-4-8"}), &provider)
            .expect_err("unknown route should fail");
        assert!(err.to_string().contains("claude-opus-4-8"));
    }

    #[test]
    fn claude_desktop_proxy_publishes_catalog_ids_for_point_release_targets() {
        let route = |model: &str| ClaudeDesktopModelRoute {
            model: model.to_string(),
            label_override: None,
            supports_1m: None,
        };
        let mut provider = proxy_provider("proxy");
        provider
            .meta
            .as_mut()
            .expect("meta")
            .claude_desktop_model_routes = std::collections::HashMap::from([
            ("claude-opus-5".to_string(), route("claude-opus-5.5")),
            (
                "claude-sonnet-5".to_string(),
                route("anthropic/claude-sonnet-5-5-20261001"),
            ),
            ("claude-haiku-4-5".to_string(), route("glm-5")),
        ]);

        let names = |provider: &Provider| -> Vec<String> {
            proxy_model_routes(provider)
                .expect("routes")
                .into_iter()
                .map(|route| route.route_id)
                .collect()
        };
        assert_eq!(
            names(&provider),
            ["claude-haiku-4-5", "claude-opus-5-5", "claude-sonnet-5-5"]
        );
        let models = model_list_response(&provider).expect("model list");
        assert_eq!(models["data"][1]["id"], json!("claude-opus-5-5"));

        // 新名字、旧 profile 里的原键、带日期的全名都落到同一个目标。
        for (requested, upstream) in [
            ("claude-opus-5-5", "claude-opus-5.5"),
            ("claude-opus-5", "claude-opus-5.5"),
            ("claude-opus-5-5-20261001[1m]", "claude-opus-5.5"),
            ("claude-sonnet-5-5", "anthropic/claude-sonnet-5-5-20261001"),
            ("claude-sonnet-5", "anthropic/claude-sonnet-5-5-20261001"),
            ("claude-haiku-4-5", "glm-5"),
        ] {
            let mapped =
                map_proxy_request_model(json!({ "model": requested }), &provider).expect(requested);
            assert_eq!(mapped["model"], json!(upstream), "{requested}");
        }

        // 用户自己配了 claude-opus-5-5 这条路由时，原键不换名，两条各走各的。
        provider
            .meta
            .as_mut()
            .expect("meta")
            .claude_desktop_model_routes
            .insert(
                "claude-opus-5-5".to_string(),
                route("claude-opus-5-5-thinking"),
            );
        assert_eq!(
            names(&provider),
            [
                "claude-haiku-4-5",
                "claude-opus-5",
                "claude-opus-5-5",
                "claude-sonnet-5-5"
            ]
        );
        let mapped = map_proxy_request_model(json!({ "model": "claude-opus-5" }), &provider)
            .expect("original key");
        assert_eq!(mapped["model"], json!("claude-opus-5.5"));
    }

    #[test]
    fn claude_desktop_proxy_maps_dated_role_alias_via_keyword() {
        // 复现反馈：Claude Desktop 子 agent 请求带发布日期后缀的完整官方名
        // （claude-haiku-4-5-20251001），与 manifest 的简短 route_id（claude-haiku-4-5）
        // 不精确相等，旧逻辑会报 route_unknown。角色关键词回落应将其映射到 Haiku 档。
        let mut provider = proxy_provider("proxy");
        provider
            .meta
            .as_mut()
            .expect("meta")
            .claude_desktop_model_routes = std::collections::HashMap::from([
            (
                "claude-sonnet-4-6".to_string(),
                ClaudeDesktopModelRoute {
                    model: "deepseek-v4-pro".to_string(),
                    label_override: None,
                    supports_1m: Some(true),
                },
            ),
            (
                "claude-opus-4-8".to_string(),
                ClaudeDesktopModelRoute {
                    model: "deepseek-v4-pro".to_string(),
                    label_override: None,
                    supports_1m: Some(true),
                },
            ),
            (
                "claude-haiku-4-5".to_string(),
                ClaudeDesktopModelRoute {
                    model: "deepseek-v4-flash".to_string(),
                    label_override: None,
                    supports_1m: Some(true),
                },
            ),
        ]);

        let mapped = map_proxy_request_model(
            json!({"model": "claude-haiku-4-5-20251001", "messages": []}),
            &provider,
        )
        .expect("dated Haiku alias should map via role keyword");
        assert_eq!(mapped["model"], json!("deepseek-v4-flash"));

        let mapped_sonnet = map_proxy_request_model(
            json!({"model": "claude-sonnet-4-5-20250101", "messages": []}),
            &provider,
        )
        .expect("dated Sonnet alias should map via role keyword");
        assert_eq!(mapped_sonnet["model"], json!("deepseek-v4-pro"));

        // 不含任何角色关键词的模型仍然报错，避免被误映射。
        let err = map_proxy_request_model(json!({"model": "gpt-5"}), &provider)
            .expect_err("model without a role keyword should still fail");
        assert!(err.to_string().contains("gpt-5"));
    }

    #[test]
    fn claude_desktop_proxy_maps_fable_to_opus_tier() {
        // issue #4026/#4049：老用户只配 Sonnet/Opus/Haiku 三档、未显式配置
        // fable 档时，fable 请求按官方分类器降级方向回落到 opus 档兜底。
        let mut provider = proxy_provider("proxy");
        provider
            .meta
            .as_mut()
            .expect("meta")
            .claude_desktop_model_routes = std::collections::HashMap::from([
            (
                "claude-opus-4-8".to_string(),
                ClaudeDesktopModelRoute {
                    model: "upstream-opus".to_string(),
                    label_override: None,
                    supports_1m: Some(true),
                },
            ),
            (
                "claude-sonnet-4-6".to_string(),
                ClaudeDesktopModelRoute {
                    model: "upstream-sonnet".to_string(),
                    label_override: None,
                    supports_1m: Some(true),
                },
            ),
        ]);

        let mapped = map_proxy_request_model(
            json!({"model": "claude-fable-5", "messages": []}),
            &provider,
        )
        .expect("fable should fall back to the opus tier");
        assert_eq!(mapped["model"], json!("upstream-opus"));

        // 带 [1m] 标记与日期后缀的形态也应命中同一回落。
        let mapped_one_m = map_proxy_request_model(
            json!({"model": "claude-fable-5[1m]", "messages": []}),
            &provider,
        )
        .expect("fable with [1m] marker should fall back to the opus tier");
        assert_eq!(mapped_one_m["model"], json!("upstream-opus"));

        let mapped_dated = map_proxy_request_model(
            json!({"model": "claude-fable-5-20260609", "messages": []}),
            &provider,
        )
        .expect("dated fable alias should fall back to the opus tier");
        assert_eq!(mapped_dated["model"], json!("upstream-opus"));
    }

    #[test]
    fn claude_desktop_proxy_fable_without_opus_route_still_errors() {
        // 没有 opus 档可回落时保持精确报错语义，不静默落到其他档。
        let mut provider = proxy_provider("proxy");
        provider
            .meta
            .as_mut()
            .expect("meta")
            .claude_desktop_model_routes = std::collections::HashMap::from([(
            "claude-sonnet-4-6".to_string(),
            ClaudeDesktopModelRoute {
                model: "upstream-sonnet".to_string(),
                label_override: None,
                supports_1m: Some(true),
            },
        )]);

        let err = map_proxy_request_model(
            json!({"model": "claude-fable-5", "messages": []}),
            &provider,
        )
        .expect_err("fable without an opus route should fail");
        assert!(err.to_string().contains("claude-fable-5"));
    }

    #[test]
    fn claude_desktop_proxy_maps_fable_to_dedicated_route() {
        // Desktop 1.12603.1+ fail-all 校验已放行 claude-fable-5，用户可显式配置
        // 独立 fable 档；此时 fable 请求精确命中 fable 档，不再降级到 opus。
        let mut provider = proxy_provider("proxy");
        provider
            .meta
            .as_mut()
            .expect("meta")
            .claude_desktop_model_routes = std::collections::HashMap::from([
            (
                "claude-opus-4-8".to_string(),
                ClaudeDesktopModelRoute {
                    model: "upstream-opus".to_string(),
                    label_override: None,
                    supports_1m: Some(true),
                },
            ),
            (
                "claude-fable-5".to_string(),
                ClaudeDesktopModelRoute {
                    model: "upstream-fable".to_string(),
                    label_override: None,
                    supports_1m: Some(true),
                },
            ),
        ]);

        // 精确匹配优先命中 fable 档
        let mapped = map_proxy_request_model(
            json!({"model": "claude-fable-5", "messages": []}),
            &provider,
        )
        .expect("explicit fable route should match");
        assert_eq!(mapped["model"], json!("upstream-fable"));

        // 带日期后缀经角色关键词回落仍归 fable 档，而非降级 opus
        let mapped_dated = map_proxy_request_model(
            json!({"model": "claude-fable-5-20260609", "messages": []}),
            &provider,
        )
        .expect("dated fable alias should map via fable role keyword");
        assert_eq!(mapped_dated["model"], json!("upstream-fable"));
    }

    #[test]
    fn claude_desktop_proxy_accepts_opus_4_7_4_8_alias_during_rollout() {
        let mut provider = proxy_provider("proxy");
        let current_routes = std::collections::HashMap::from([(
            "claude-opus-4-8".to_string(),
            ClaudeDesktopModelRoute {
                model: "upstream-opus-new".to_string(),
                label_override: None,
                supports_1m: Some(true),
            },
        )]);
        provider
            .meta
            .as_mut()
            .expect("meta")
            .claude_desktop_model_routes = current_routes;

        let mapped = map_proxy_request_model(
            json!({"model": "claude-opus-4-7", "messages": []}),
            &provider,
        )
        .expect("legacy Opus route should map to current route");
        assert_eq!(mapped["model"], json!("upstream-opus-new"));

        let legacy_routes = std::collections::HashMap::from([(
            "claude-opus-4-7".to_string(),
            ClaudeDesktopModelRoute {
                model: "upstream-opus-legacy".to_string(),
                label_override: None,
                supports_1m: Some(true),
            },
        )]);
        provider
            .meta
            .as_mut()
            .expect("meta")
            .claude_desktop_model_routes = legacy_routes;

        let mapped = map_proxy_request_model(
            json!({"model": "claude-opus-4-8", "messages": []}),
            &provider,
        )
        .expect("current Opus route should map to legacy saved route");
        assert_eq!(mapped["model"], json!("upstream-opus-legacy"));
    }

    #[test]
    fn claude_desktop_mimo_anthropic_rewrites_redacted_thinking_for_tool_history() {
        let provider = mimo_anthropic_proxy_provider("mimo");

        let mapped = map_proxy_request_model(
            json!({
                "model": "claude-sonnet-4-6",
                "messages": [{
                    "role": "assistant",
                    "content": [
                        {"type": "redacted_thinking", "data": "opaque"},
                        {"type": "tool_use", "id": "call_1", "name": "read_file", "input": {"path": "README.md"}}
                    ]
                }]
            }),
            &provider,
        )
        .expect("map MiMo route");

        assert_eq!(mapped["model"], json!("mimo-v2.5-pro"));
        assert_eq!(
            mapped["messages"][0]["content"][0]["type"],
            json!("thinking")
        );
        assert_eq!(
            mapped["messages"][0]["content"][0]["thinking"],
            json!("[redacted thinking]")
        );
        assert_eq!(
            mapped["messages"][0]["content"][1]["type"],
            json!("tool_use")
        );
    }

    #[test]
    fn claude_desktop_mimo_anthropic_injects_thinking_for_tool_history_without_one() {
        let provider = mimo_anthropic_proxy_provider("mimo");

        let mapped = map_proxy_request_model(
            json!({
                "model": "claude-sonnet-4-6",
                "messages": [{
                    "role": "assistant",
                    "content": [
                        {"type": "tool_use", "id": "call_1", "name": "read_file", "input": {"path": "README.md"}}
                    ]
                }]
            }),
            &provider,
        )
        .expect("map MiMo route");

        assert_eq!(
            mapped["messages"][0]["content"][0]["type"],
            json!("thinking")
        );
        assert_eq!(
            mapped["messages"][0]["content"][0]["thinking"],
            json!("tool call")
        );
        assert_eq!(
            mapped["messages"][0]["content"][1]["type"],
            json!("tool_use")
        );
    }

    #[test]
    fn claude_desktop_mimo_anthropic_keeps_thinking_text_but_drops_signature() {
        let provider = mimo_anthropic_proxy_provider("mimo");

        let mapped = map_proxy_request_model(
            json!({
                "model": "claude-sonnet-4-6",
                "messages": [{
                    "role": "assistant",
                    "content": [
                        {"type": "thinking", "thinking": "Need to inspect the file.", "signature": "anthropic-signature"},
                        {"type": "tool_use", "id": "call_1", "name": "read_file", "input": {"path": "README.md"}}
                    ]
                }]
            }),
            &provider,
        )
        .expect("map MiMo route");

        assert_eq!(
            mapped["messages"][0]["content"][0]["thinking"],
            json!("Need to inspect the file.")
        );
        assert!(mapped["messages"][0]["content"][0]
            .get("signature")
            .is_none());
    }

    #[test]
    fn claude_desktop_proxy_repairs_legacy_unsafe_route_without_colliding() {
        let mut provider = proxy_provider("proxy");
        provider.meta = Some(ProviderMeta {
            claude_desktop_mode: Some(ClaudeDesktopMode::Proxy),
            api_format: Some("openai_chat".to_string()),
            claude_desktop_model_routes: std::collections::HashMap::from([
                (
                    "claude-deepseek-v4-pro".to_string(),
                    ClaudeDesktopModelRoute {
                        model: "deepseek-v4-pro".to_string(),
                        label_override: None,
                        supports_1m: Some(true),
                    },
                ),
                (
                    "claude-old".to_string(),
                    ClaudeDesktopModelRoute {
                        model: "legacy-upstream".to_string(),
                        label_override: None,
                        supports_1m: Some(false),
                    },
                ),
                (
                    "claude-sonnet-5".to_string(),
                    ClaudeDesktopModelRoute {
                        model: "claude-sonnet-5".to_string(),
                        label_override: None,
                        supports_1m: Some(false),
                    },
                ),
            ]),
            ..Default::default()
        });

        let routes = proxy_model_routes(&provider).expect("routes");
        assert_eq!(routes.len(), 3);
        let repaired = routes
            .iter()
            .find(|route| route.upstream_model == "deepseek-v4-pro")
            .expect("repaired route");
        assert_eq!(repaired.route_id, "claude-opus-5");
        assert_eq!(repaired.label_override.as_deref(), Some("deepseek-v4-pro"));
        assert!(repaired.supports_1m);
        let repaired_old = routes
            .iter()
            .find(|route| route.upstream_model == "legacy-upstream")
            .expect("legacy route should be repaired");
        assert_eq!(repaired_old.route_id, "claude-haiku-4-5");
        assert_eq!(
            repaired_old.label_override.as_deref(),
            Some("legacy-upstream")
        );

        let mapped = map_proxy_request_model(
            json!({"model": "claude-opus-4-8", "messages": []}),
            &provider,
        )
        .expect("map repaired route");
        assert_eq!(mapped["model"], json!("deepseek-v4-pro"));

        let legacy_mapped =
            map_proxy_request_model(json!({"model": "claude-old", "messages": []}), &provider)
                .expect("map stale profile route");
        assert_eq!(legacy_mapped["model"], json!("legacy-upstream"));
    }

    #[test]
    fn claude_desktop_proxy_strips_1m_suffix_before_route_lookup() {
        let mut provider = proxy_provider("proxy");
        provider
            .meta
            .as_mut()
            .expect("meta")
            .claude_desktop_model_routes = std::collections::HashMap::from([
            (
                "claude-sonnet-4-6".to_string(),
                ClaudeDesktopModelRoute {
                    model: "upstream-sonnet".to_string(),
                    label_override: None,
                    supports_1m: Some(true),
                },
            ),
            (
                "claude-opus-4-8".to_string(),
                ClaudeDesktopModelRoute {
                    model: "upstream-opus".to_string(),
                    label_override: None,
                    supports_1m: Some(true),
                },
            ),
        ]);

        let mapped = map_proxy_request_model(
            json!({"model": "claude-opus-4-8[1m]", "messages": []}),
            &provider,
        )
        .expect("compact 1M suffix should map to Opus route");
        assert_eq!(mapped["model"], json!("upstream-opus"));

        let mapped = map_proxy_request_model(
            json!({"model": "claude-sonnet-4-6 [1M]", "messages": []}),
            &provider,
        )
        .expect("spaced uppercase 1M suffix should map to Sonnet route");
        assert_eq!(mapped["model"], json!("upstream-sonnet"));

        let err = map_proxy_request_model(json!({"model": "gpt-5[1m]", "messages": []}), &provider)
            .expect_err("non-Claude route should still fail after stripping 1M suffix");
        assert!(err.to_string().contains("gpt-5[1m]"));
    }

    #[test]
    fn claude_desktop_rejects_1m_suffix_as_model_id() {
        assert!(!is_claude_safe_model_id("claude-sonnet-4-6 [1m]"));
        assert!(!is_claude_safe_model_id("  claude-sonnet-4-6  [1M]  "));
        assert!(!is_claude_safe_model_id("claude-old"));
        assert!(!is_claude_safe_model_id("claude-3-5-sonnet-20241022"));
        assert!(!is_claude_safe_model_id("claude-deepseek-v4-pro"));
        assert!(!is_claude_safe_model_id("claude-gpt-5-4"));
        assert!(!is_claude_safe_model_id("claude-"));
        assert!(!is_claude_safe_model_id("anthropic/claude-"));
        assert!(!is_claude_safe_model_id("sonnet"));
        assert!(!is_claude_safe_model_id("sonnet-"));
        // 角色前缀后无实际标识的退化值必须拒绝
        assert!(!is_claude_safe_model_id("claude-sonnet-"));
        assert!(!is_claude_safe_model_id("claude-opus-"));
        assert!(!is_claude_safe_model_id("anthropic/claude-haiku-"));
        assert!(is_claude_safe_model_id("  claude-sonnet-4-6  "));
        assert!(is_claude_safe_model_id("anthropic/claude-opus-4-8"));
    }

    #[test]
    fn claude_desktop_apply_rolls_back_when_profile_write_fails() {
        let temp = TempDir::new().expect("tempdir");
        let paths = test_paths(temp.path());
        let provider = direct_provider("direct");
        let db = test_db();

        write_json_file(
            &paths.normal_config_path,
            &json!({"deploymentMode": "1p", "normal": true}),
        )
        .expect("write normal");
        write_json_file(
            &paths.threep_config_path,
            &json!({"deploymentMode": "1p", "threep": true}),
        )
        .expect("write 3p");
        fs::write(&paths.config_library_path, "not a directory").expect("block profile parent");

        apply_provider_to_paths(&db, &provider, &paths).expect_err("apply should fail");

        let normal: Value = read_json_file(&paths.normal_config_path).expect("read normal config");
        let threep: Value = read_json_file(&paths.threep_config_path).expect("read 3p config");

        assert_eq!(normal, json!({"deploymentMode": "1p", "normal": true}));
        assert_eq!(threep, json!({"deploymentMode": "1p", "threep": true}));
        assert!(!paths.profile_path.exists());
    }

    #[test]
    fn claude_desktop_refuses_a_meta_file_it_cannot_understand() {
        let temp = TempDir::new().expect("tempdir");
        let paths = test_paths(temp.path());
        let db = test_db();
        if let Some(parent) = paths.meta_path.parent() {
            fs::create_dir_all(parent).expect("create parent");
        }
        fs::write(&paths.meta_path, "[]").expect("write invalid meta shape");

        apply_provider_to_paths(&db, &direct_provider("direct"), &paths)
            .expect_err("a meta file that is not an object must not be overwritten");

        assert_eq!(fs::read_to_string(&paths.meta_path).unwrap(), "[]");
        assert!(!paths.profile_path.exists());
        assert!(!paths.normal_config_path.exists());
    }

    #[test]
    fn claude_desktop_restore_switches_to_1p_and_clears_the_cc_switch_profile() {
        let temp = TempDir::new().expect("tempdir");
        let paths = test_paths(temp.path());
        let provider = direct_provider("direct");
        let db = test_db();

        apply_provider_to_paths(&db, &provider, &paths).expect("apply provider");
        restore_official(&paths).expect("restore official");

        let normal: Value = read_json_file(&paths.normal_config_path).expect("read normal config");
        let threep: Value = read_json_file(&paths.threep_config_path).expect("read 3p config");
        let meta: Value = read_json_file(&paths.meta_path).expect("read meta");
        let profile: Value = read_json_file(&paths.profile_path).expect("profile is kept");

        assert_eq!(normal["deploymentMode"], json!("1p"));
        assert_eq!(threep["deploymentMode"], json!("1p"));
        for key in floor::DESKTOP_PROFILE_FLOOR {
            assert!(profile.get(*key).is_none(), "{key} must be cleared");
        }
        assert!(meta.get("appliedId").is_none());
        assert!(!meta["entries"]
            .as_array()
            .expect("entries")
            .iter()
            .any(|entry| entry["id"] == json!(PROFILE_ID)));
    }

    /// #4774：用户在 Desktop 里给 CC Switch 的 profile 打开的设置，切换供应商、
    /// 切回官方再切回来都不能丢；用户收紧过的出站白名单也不能被改回 `*`。
    #[test]
    fn claude_desktop_switches_keep_the_users_own_profile_settings() {
        let temp = TempDir::new().expect("tempdir");
        let paths = test_paths(temp.path());
        let db = test_db();

        apply_provider_to_paths(&db, &direct_provider("a"), &paths).expect("apply a");
        let mut profile: Value = read_json_file(&paths.profile_path).expect("read profile");
        profile["userAutoMode"] = json!(true);
        profile["coworkEgressAllowedHosts"] = json!(["corp.example"]);
        fs::write(
            &paths.profile_path,
            serde_json::to_string_pretty(&profile).unwrap(),
        )
        .unwrap();

        apply_provider_to_paths(&db, &direct_provider_with_models("b"), &paths).expect("apply b");
        restore_official(&paths).expect("restore official");
        apply_provider_to_paths(&db, &direct_provider("a"), &paths).expect("apply a again");

        let profile: Value = read_json_file(&paths.profile_path).expect("read profile");
        assert_eq!(profile["userAutoMode"], json!(true));
        assert_eq!(profile["coworkEgressAllowedHosts"], json!(["corp.example"]));
        assert_eq!(profile["disableDeploymentModeChooser"], json!(true));
        assert_eq!(profile["inferenceProvider"], json!("gateway"));
        assert!(
            profile.get("inferenceModels").is_none(),
            "the previous provider's models must not linger"
        );
    }

    #[test]
    fn claude_desktop_keeps_other_meta_entries_in_place() {
        let temp = TempDir::new().expect("tempdir");
        let paths = test_paths(temp.path());
        let db = test_db();
        write_json_file(
            &paths.meta_path,
            &json!({
                "entries": [
                    {"id": "other-1", "name": "Mine"},
                    {"id": PROFILE_ID, "name": "Old name", "note": "kept"},
                    {"id": "other-2", "name": "Also mine"}
                ],
                "appliedId": "other-1"
            }),
        )
        .expect("seed meta");

        apply_provider_to_paths(&db, &direct_provider("a"), &paths).expect("apply");
        let meta: Value = read_json_file(&paths.meta_path).expect("read meta");
        assert_eq!(
            meta["entries"],
            json!([
                {"id": "other-1", "name": "Mine"},
                {"id": PROFILE_ID, "name": PROFILE_NAME, "note": "kept"},
                {"id": "other-2", "name": "Also mine"}
            ])
        );
        assert_eq!(meta["appliedId"], json!(PROFILE_ID));

        restore_official(&paths).expect("restore");
        let meta: Value = read_json_file(&paths.meta_path).expect("read meta");
        assert_eq!(
            meta["entries"],
            json!([{"id": "other-1", "name": "Mine"}, {"id": "other-2", "name": "Also mine"}])
        );
        assert_eq!(meta["appliedId"], json!("other-1"));
    }

    #[cfg(unix)]
    #[test]
    fn claude_desktop_profile_with_the_gateway_key_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let temp = TempDir::new().expect("tempdir");
        let paths = test_paths(temp.path());
        apply_provider_to_paths(&test_db(), &direct_provider("a"), &paths).expect("apply");
        let mode = fs::metadata(&paths.profile_path)
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[test]
    fn claude_desktop_official_provider_restores_1p_mode() {
        let temp = TempDir::new().expect("tempdir");
        let paths = test_paths(temp.path());
        let direct = direct_provider("direct");
        let db = test_db();

        apply_provider_to_paths(&db, &direct, &paths).expect("apply direct provider");
        apply_provider_to_paths(&db, &official_provider(), &paths)
            .expect("restore official provider");

        let normal: Value = read_json_file(&paths.normal_config_path).expect("read normal config");
        let threep: Value = read_json_file(&paths.threep_config_path).expect("read 3p config");
        let meta: Value = read_json_file(&paths.meta_path).expect("read meta");

        assert_eq!(normal["deploymentMode"], json!("1p"));
        assert_eq!(threep["deploymentMode"], json!("1p"));
        let profile: Value = read_json_file(&paths.profile_path).expect("profile is kept");
        assert!(profile.get("inferenceGatewayApiKey").is_none());
        assert!(meta.get("appliedId").is_none());
    }

    #[test]
    fn claude_desktop_compatibility_filters_non_direct_providers() {
        let direct = direct_provider("direct");
        assert!(is_compatible_direct_provider(&direct));

        let mut claude_official = Provider::with_id(
            "claude-official".to_string(),
            "Claude Official".to_string(),
            json!({"env": {}}),
            Some("https://www.anthropic.com/claude-code".to_string()),
        );
        claude_official.category = Some("official".to_string());
        assert!(!is_compatible_direct_provider(&claude_official));

        let mut openai_format = direct_provider("openai");
        openai_format.meta = Some(ProviderMeta {
            api_format: Some("openai_chat".to_string()),
            ..Default::default()
        });
        assert!(!is_compatible_direct_provider(&openai_format));

        let mut copilot = direct_provider("copilot");
        copilot.meta = Some(ProviderMeta {
            provider_type: Some("github_copilot".to_string()),
            ..Default::default()
        });
        assert!(!is_compatible_direct_provider(&copilot));

        let mut full_url = direct_provider("full_url");
        full_url.meta = Some(ProviderMeta {
            is_full_url: Some(true),
            ..Default::default()
        });
        assert!(!is_compatible_direct_provider(&full_url));

        let missing_bearer = Provider::with_id(
            "x-api-key".to_string(),
            "x-api-key".to_string(),
            json!({
                "env": {
                    "ANTHROPIC_BASE_URL": "https://gateway.example.com",
                    "ANTHROPIC_API_KEY": "sk-ant"
                }
            }),
            None,
        );
        assert!(!is_compatible_direct_provider(&missing_bearer));
    }
}
