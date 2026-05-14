use serde::Serialize;
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};

#[cfg(any(target_os = "macos", windows))]
use crate::config::get_home_dir;
use crate::config::{atomic_write, delete_file, read_json_file, write_json_file};
use crate::database::Database;
use crate::database::CLAUDE_DESKTOP_OFFICIAL_PROVIDER_ID;
use crate::error::AppError;
use crate::provider::{ClaudeDesktopMode, Provider};

pub const PROFILE_ID: &str = "00000000-0000-4000-8000-000000157210";
pub const PROFILE_NAME: &str = "CC Switch";

#[cfg(any(target_os = "macos", windows, test))]
const CONFIG_FILE: &str = "claude_desktop_config.json";
#[cfg(any(target_os = "macos", windows, test))]
const CONFIG_LIBRARY_DIR: &str = "configLibrary";
const GATEWAY_TOKEN_SETTING_KEY: &str = "claude_desktop_gateway_token";
const CLAUDE_DESKTOP_PROXY_PREFIX: &str = "/claude-desktop";
const DEFAULT_CREATED_AT: &str = "2024-01-01T00:00:00Z";

/// Claude Desktop 模型菜单识别的 route ID 前缀。
pub const CLAUDE_ROUTE_PREFIX: &str = "claude-";
/// 替代前缀（与前端 `ANTHROPIC_CLAUDE_ROUTE_PREFIX` 一致）。
pub const ANTHROPIC_CLAUDE_ROUTE_PREFIX: &str = "anthropic/claude-";
/// Claude Code env 中通过 `[1M]` 后缀声明 1M 上下文能力（匹配用 `eq_ignore_ascii_case`）。
/// Claude Desktop schema 不接受此后缀，import 边界翻译为 `supports1m` 字段。
pub const ONE_M_CONTEXT_MARKER: &str = "[1m]";

const NON_ANTHROPIC_ROUTE_MARKERS: &[&str] = &[
    "ark-code",
    "astron",
    "command-r",
    "deepseek",
    "doubao",
    "gemini",
    "gemma",
    "glm",
    "gpt",
    "grok",
    "hermes",
    "hy3",
    "kimi",
    "lfm",
    "llama",
    "longcat",
    "mimo",
    "minimax",
    "mistral",
    "mixtral",
    "moonshot",
    "nemotron",
    "openai",
    "qianfan",
    "qwen",
    "stepfun",
    "seed-",
    "hunyuan",
    "nova-",
    "ernie",
    "codex",
    "abab",
    "jamba",
    "arctic",
    "solar",
    "mercury",
];

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
        route_id: "claude-sonnet-4-6",
        env_key: "ANTHROPIC_DEFAULT_SONNET_MODEL",
        supports_1m: true,
    },
    ClaudeDesktopDefaultRoute {
        route_id: "claude-opus-4-7",
        env_key: "ANTHROPIC_DEFAULT_OPUS_MODEL",
        supports_1m: true,
    },
    ClaudeDesktopDefaultRoute {
        route_id: "claude-haiku-4-5",
        env_key: "ANTHROPIC_DEFAULT_HAIKU_MODEL",
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectGatewayCredentials {
    pub base_url: String,
    pub api_key: String,
}

#[derive(Debug, Clone)]
struct FileSnapshot {
    path: PathBuf,
    content: Option<Vec<u8>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeDesktopStatus {
    pub supported: bool,
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

pub fn apply_provider(db: &Database, provider: &Provider) -> Result<(), AppError> {
    let paths = current_platform_paths()?;
    apply_provider_to_paths(db, provider, &paths)
}

pub fn get_status(db: &Database, proxy_running: bool) -> Result<ClaudeDesktopStatus, AppError> {
    if !is_supported_platform() {
        return Ok(ClaudeDesktopStatus {
            supported: false,
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
    let configured = paths.profile_path.exists() || meta_has_profile_entry(&paths.meta_path);
    let profile = read_json_or_empty(&paths.profile_path).unwrap_or_else(|_| json!({}));
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
    let current_provider = crate::settings::get_effective_current_provider(
        db,
        &crate::app_config::AppType::ClaudeDesktop,
    )
    .ok()
    .flatten()
    .and_then(|id| db.get_provider_by_id(&id, "claude-desktop").ok().flatten());
    let mode = current_provider.as_ref().map(provider_mode);
    let expected_base_url = match mode {
        Some(ClaudeDesktopMode::Proxy) => proxy_gateway_base_url_from_db(db).ok(),
        Some(ClaudeDesktopMode::Direct) => current_provider
            .as_ref()
            .and_then(|provider| direct_gateway_credentials(provider).ok())
            .map(|credentials| credentials.base_url),
        None => None,
    };
    let missing_route_mappings = current_provider.as_ref().is_some_and(|provider| {
        matches!(provider_mode(provider), ClaudeDesktopMode::Proxy)
            && proxy_model_routes(provider).is_err()
    });

    Ok(ClaudeDesktopStatus {
        supported: true,
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

pub fn provider_mode(provider: &Provider) -> ClaudeDesktopMode {
    provider
        .meta
        .as_ref()
        .and_then(|meta| meta.claude_desktop_mode.clone())
        .unwrap_or(ClaudeDesktopMode::Direct)
}

pub fn is_claude_safe_model_id(model: &str) -> bool {
    let normalized = model.trim().to_ascii_lowercase();
    let has_allowed_shape = (normalized.starts_with(CLAUDE_ROUTE_PREFIX)
        && normalized.len() > CLAUDE_ROUTE_PREFIX.len())
        || (normalized.starts_with(ANTHROPIC_CLAUDE_ROUTE_PREFIX)
            && normalized.len() > ANTHROPIC_CLAUDE_ROUTE_PREFIX.len())
        || matches!(normalized.as_str(), "sonnet" | "opus" | "haiku")
        || (normalized.starts_with("sonnet-") && normalized.len() > "sonnet-".len())
        || (normalized.starts_with("opus-") && normalized.len() > "opus-".len())
        || (normalized.starts_with("haiku-") && normalized.len() > "haiku-".len());
    has_allowed_shape
        && !normalized.contains(ONE_M_CONTEXT_MARKER)
        && !NON_ANTHROPIC_ROUTE_MARKERS
            .iter()
            .any(|marker| normalized.contains(marker))
}

fn inference_model_json(spec: &InferenceModelSpec) -> Value {
    if spec.supports_1m || spec.label_override.is_some() {
        let mut item = json!({ "name": spec.name });
        if let Some(label_override) = spec.label_override.as_deref() {
            item["labelOverride"] = json!(label_override);
        }
        if spec.supports_1m {
            item["supports1m"] = json!(true);
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
            Some("github_copilot") | Some("codex_oauth")
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
        .is_some_and(|provider_type| matches!(provider_type, "github_copilot" | "codex_oauth"))
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

pub fn proxy_model_routes(provider: &Provider) -> Result<Vec<ResolvedModelRoute>, AppError> {
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
            route_id: repaired_route_id,
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
    let requested = body
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

    let routes = proxy_model_routes(provider)?;
    let route = routes.iter().find(|r| r.route_id == requested);
    let Some(route) = route else {
        return Err(AppError::localized(
            "claude_desktop.provider.route_unknown",
            format!("Claude Desktop 模型路由未配置: {requested}"),
            format!("Claude Desktop model route is not configured: {requested}"),
        ));
    };

    body["model"] = json!(route.upstream_model);
    Ok(body)
}

pub fn proxy_gateway_base_url_from_db(db: &Database) -> Result<String, AppError> {
    // get_proxy_config is async-tagged but its body is fully synchronous (rusqlite
    // under a Mutex), so block_on cannot deadlock the calling thread.
    let config = futures::executor::block_on(db.get_proxy_config())?;
    Ok(format!(
        "{}{}",
        proxy_origin_from_parts(&config.listen_address, config.listen_port),
        CLAUDE_DESKTOP_PROXY_PREFIX
    ))
}

fn apply_provider_to_paths(
    db: &Database,
    provider: &Provider,
    paths: &ClaudeDesktopPaths,
) -> Result<(), AppError> {
    if is_official_provider(provider) {
        return restore_official_at_paths(paths);
    }

    validate_provider(provider)?;
    with_rollback(paths, |paths| {
        apply_provider_to_paths_inner(db, provider, paths)
    })
}

fn restore_official_at_paths(paths: &ClaudeDesktopPaths) -> Result<(), AppError> {
    with_rollback(paths, restore_official_at_paths_inner)
}

fn with_rollback<F>(paths: &ClaudeDesktopPaths, op: F) -> Result<(), AppError>
where
    F: FnOnce(&ClaudeDesktopPaths) -> Result<(), AppError>,
{
    let snapshots = snapshot_files(paths)?;
    match op(paths) {
        Ok(()) => Ok(()),
        Err(err) => match restore_snapshots(&snapshots) {
            Ok(()) => Err(err),
            Err(rollback_err) => {
                log::error!("Failed to rollback Claude Desktop config after error: {rollback_err}");
                Err(AppError::Message(format!(
                    "{err}; rollback failed: {rollback_err}"
                )))
            }
        },
    }
}

fn apply_provider_to_paths_inner(
    db: &Database,
    provider: &Provider,
    paths: &ClaudeDesktopPaths,
) -> Result<(), AppError> {
    let profile = match provider_mode(provider) {
        ClaudeDesktopMode::Direct => {
            let credentials = direct_gateway_credentials(provider)?;
            let model_specs = direct_inference_model_specs(provider)?;
            build_gateway_profile(
                &credentials.base_url,
                &credentials.api_key,
                (!model_specs.is_empty()).then_some(model_specs.as_slice()),
            )
        }
        ClaudeDesktopMode::Proxy => {
            let base_url = proxy_gateway_base_url_from_db(db)?;
            let api_key = get_or_create_gateway_token(db)?;
            let routes = proxy_model_routes(provider)?;
            let model_specs = routes
                .iter()
                .map(|route| InferenceModelSpec {
                    name: route.route_id.clone(),
                    label_override: route.label_override.clone(),
                    supports_1m: route.supports_1m,
                })
                .collect::<Vec<_>>();
            build_gateway_profile(&base_url, &api_key, Some(model_specs.as_slice()))
        }
    };

    write_deployment_mode(&paths.normal_config_path, "3p")?;
    write_deployment_mode(&paths.threep_config_path, "3p")?;
    write_json_file(&paths.profile_path, &profile)?;
    write_meta(&paths.meta_path, Some(PROFILE_ID))?;

    Ok(())
}

fn restore_official_at_paths_inner(paths: &ClaudeDesktopPaths) -> Result<(), AppError> {
    write_deployment_mode(&paths.normal_config_path, "1p")?;
    write_deployment_mode(&paths.threep_config_path, "1p")?;
    remove_cc_switch_enterprise_config(&paths.threep_config_path)?;

    if paths.profile_path.exists() {
        delete_file(&paths.profile_path)?;
    }
    write_meta(&paths.meta_path, None)?;

    Ok(())
}

fn build_gateway_profile(
    base_url: &str,
    api_key: &str,
    model_specs: Option<&[InferenceModelSpec]>,
) -> Value {
    let mut profile = json!({
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

fn snapshot_files(paths: &ClaudeDesktopPaths) -> Result<Vec<FileSnapshot>, AppError> {
    [
        &paths.normal_config_path,
        &paths.threep_config_path,
        &paths.profile_path,
        &paths.meta_path,
    ]
    .into_iter()
    .map(|path| {
        let content = if path.exists() {
            Some(fs::read(path).map_err(|e| AppError::io(path, e))?)
        } else {
            None
        };
        Ok(FileSnapshot {
            path: path.clone(),
            content,
        })
    })
    .collect()
}

fn restore_snapshots(snapshots: &[FileSnapshot]) -> Result<(), AppError> {
    for snapshot in snapshots {
        match &snapshot.content {
            Some(content) => {
                if let Some(parent) = snapshot.path.parent() {
                    fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
                }
                atomic_write(&snapshot.path, content)?;
            }
            None => {
                delete_file(&snapshot.path)?;
            }
        }
    }
    Ok(())
}

fn write_deployment_mode(path: &Path, mode: &str) -> Result<(), AppError> {
    let mut value = read_json_or_empty(path)?;
    if !value.is_object() {
        value = json!({});
    }
    if let Some(obj) = value.as_object_mut() {
        obj.insert(
            "deploymentMode".to_string(),
            Value::String(mode.to_string()),
        );
    }
    write_json_file(path, &value)
}

fn remove_cc_switch_enterprise_config(path: &Path) -> Result<(), AppError> {
    if !path.exists() {
        return Ok(());
    }

    let mut value = read_json_or_empty(path)?;
    let Some(obj) = value.as_object_mut() else {
        return Ok(());
    };
    let Some(enterprise) = obj
        .get_mut("enterpriseConfig")
        .and_then(Value::as_object_mut)
    else {
        return Ok(());
    };

    for key in [
        "disableDeploymentModeChooser",
        "inferenceGatewayApiKey",
        "inferenceGatewayAuthScheme",
        "inferenceGatewayBaseUrl",
        "inferenceProvider",
    ] {
        enterprise.remove(key);
    }

    if enterprise.is_empty() {
        obj.remove("enterpriseConfig");
    }

    write_json_file(path, &value)
}

fn write_meta(path: &Path, applied_profile_id: Option<&str>) -> Result<(), AppError> {
    let mut value = read_json_or_empty(path)?;
    if !value.is_object() {
        value = json!({});
    }

    let obj = value.as_object_mut().expect("just normalized to object");
    let mut entries = obj
        .get("entries")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    entries.retain(|entry| entry.get("id").and_then(Value::as_str) != Some(PROFILE_ID));

    match applied_profile_id {
        Some(id) => {
            entries.push(json!({
                "id": PROFILE_ID,
                "name": PROFILE_NAME
            }));
            obj.insert("appliedId".to_string(), Value::String(id.to_string()));
        }
        None => {
            let should_clear_applied = obj
                .get("appliedId")
                .and_then(Value::as_str)
                .is_some_and(|id| id == PROFILE_ID);
            if should_clear_applied {
                if let Some(next_id) = entries
                    .iter()
                    .find_map(|entry| entry.get("id").and_then(Value::as_str))
                {
                    obj.insert("appliedId".to_string(), Value::String(next_id.to_string()));
                } else {
                    obj.remove("appliedId");
                }
            }
        }
    }

    obj.insert("entries".to_string(), Value::Array(entries));
    write_json_file(path, &value)
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

fn is_supported_platform() -> bool {
    cfg!(any(target_os = "macos", windows))
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

    #[cfg(not(any(target_os = "macos", windows)))]
    {
        Err(unsupported_platform_error())
    }
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

#[cfg(any(target_os = "macos", windows, test))]
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
    }
}

fn proxy_origin_from_parts(listen_address: &str, listen_port: u16) -> String {
    let connect_host = match listen_address {
        "0.0.0.0" => "127.0.0.1",
        "::" => "::1",
        value => value,
    };
    let connect_host_for_url = if connect_host.contains(':') && !connect_host.starts_with('[') {
        format!("[{connect_host}]")
    } else {
        connect_host.to_string()
    };

    format!("http://{}:{}", connect_host_for_url, listen_port)
}

#[cfg(not(any(target_os = "macos", windows)))]
fn unsupported_platform_error() -> AppError {
    AppError::localized(
        "claude_desktop.unsupported_platform",
        "当前平台暂不支持 Claude Desktop 3P 配置。第一阶段仅支持 macOS 和 Windows。",
        "Claude Desktop 3P configuration is not supported on this platform yet. Phase 1 only supports macOS and Windows.",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;
    use crate::provider::{ClaudeDesktopModelRoute, ProviderMeta};
    use serde_json::json;
    use tempfile::TempDir;

    fn test_paths(home: &Path) -> ClaudeDesktopPaths {
        paths_from_dirs(
            home.join("Library")
                .join("Application Support")
                .join("Claude"),
            home.join("Library")
                .join("Application Support")
                .join("Claude-3p"),
        )
    }

    fn test_db() -> Database {
        Database::memory().expect("memory db")
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
            json!([{ "name": "claude-sonnet-4-6", "supports1m": true }])
        );
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
        assert_ne!(profile["inferenceGatewayApiKey"], json!("test-token"));
        assert!(profile["inferenceGatewayApiKey"]
            .as_str()
            .expect("gateway token")
            .starts_with("ccs-"));
        assert_eq!(
            profile["inferenceModels"],
            json!([{ "name": "claude-sonnet-4-6", "labelOverride": "Kimi K2", "supports1m": true }])
        );
        assert!(!profile.to_string().contains("kimi-k2"));
    }

    #[test]
    fn claude_desktop_proxy_accepts_managed_oauth_providers_without_static_key() {
        for (provider_type, api_format) in [
            ("github_copilot", "openai_chat"),
            ("codex_oauth", "openai_responses"),
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

        let err = map_proxy_request_model(json!({"model": "claude-opus-4-7"}), &provider)
            .expect_err("unknown route should fail");
        assert!(err.to_string().contains("claude-opus-4-7"));
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
                    "claude-sonnet-4-6".to_string(),
                    ClaudeDesktopModelRoute {
                        model: "claude-sonnet-4-6".to_string(),
                        label_override: None,
                        supports_1m: Some(false),
                    },
                ),
            ]),
            ..Default::default()
        });

        let routes = proxy_model_routes(&provider).expect("routes");
        assert_eq!(routes.len(), 2);
        let repaired = routes
            .iter()
            .find(|route| route.upstream_model == "deepseek-v4-pro")
            .expect("repaired route");
        assert_eq!(repaired.route_id, "claude-opus-4-7");
        assert_eq!(repaired.label_override.as_deref(), Some("deepseek-v4-pro"));
        assert!(repaired.supports_1m);

        let mapped = map_proxy_request_model(
            json!({"model": "claude-opus-4-7", "messages": []}),
            &provider,
        )
        .expect("map repaired route");
        assert_eq!(mapped["model"], json!("deepseek-v4-pro"));
    }

    #[test]
    fn claude_desktop_proxy_rejects_1m_suffix_route() {
        let provider = proxy_provider("proxy");

        let err = map_proxy_request_model(
            json!({"model": "claude-sonnet-4-6 [1M]", "messages": []}),
            &provider,
        )
        .expect_err("1M suffix route should not be accepted");
        assert!(err.to_string().contains("claude-sonnet-4-6 [1M]"));
    }

    #[test]
    fn claude_desktop_rejects_1m_suffix_as_model_id() {
        assert!(!is_claude_safe_model_id("claude-sonnet-4-6 [1m]"));
        assert!(!is_claude_safe_model_id("  claude-sonnet-4-6  [1M]  "));
        assert!(!is_claude_safe_model_id("claude-deepseek-v4-pro"));
        assert!(!is_claude_safe_model_id("claude-gpt-5-4"));
        assert!(!is_claude_safe_model_id("claude-"));
        assert!(!is_claude_safe_model_id("anthropic/claude-"));
        assert!(!is_claude_safe_model_id("sonnet-"));
        assert!(is_claude_safe_model_id("  claude-sonnet-4-6  "));
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
    fn claude_desktop_write_meta_recovers_non_object_meta_file() {
        let temp = TempDir::new().expect("tempdir");
        let paths = test_paths(temp.path());
        if let Some(parent) = paths.meta_path.parent() {
            fs::create_dir_all(parent).expect("create parent");
        }
        fs::write(&paths.meta_path, "[]").expect("write invalid meta shape");

        write_meta(&paths.meta_path, Some(PROFILE_ID)).expect("write meta");

        let meta: Value = read_json_file(&paths.meta_path).expect("read meta");
        assert_eq!(meta["appliedId"], json!(PROFILE_ID));
        assert!(meta["entries"].as_array().is_some());
    }

    #[test]
    fn claude_desktop_restore_switches_to_1p_and_removes_cc_switch_profile() {
        let temp = TempDir::new().expect("tempdir");
        let paths = test_paths(temp.path());
        let provider = direct_provider("direct");
        let db = test_db();

        apply_provider_to_paths(&db, &provider, &paths).expect("apply provider");
        restore_official_at_paths(&paths).expect("restore official");

        let normal: Value = read_json_file(&paths.normal_config_path).expect("read normal config");
        let threep: Value = read_json_file(&paths.threep_config_path).expect("read 3p config");
        let meta: Value = read_json_file(&paths.meta_path).expect("read meta");

        assert_eq!(normal["deploymentMode"], json!("1p"));
        assert_eq!(threep["deploymentMode"], json!("1p"));
        assert!(!paths.profile_path.exists());
        assert!(meta.get("appliedId").is_none());
        assert!(!meta["entries"]
            .as_array()
            .expect("entries")
            .iter()
            .any(|entry| entry["id"] == json!(PROFILE_ID)));
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
        assert!(!paths.profile_path.exists());
        assert!(meta.get("appliedId").is_none());
    }

    #[test]
    fn claude_desktop_compatibility_filters_non_direct_providers() {
        let direct = direct_provider("direct");
        assert!(is_compatible_direct_provider(&direct));

        let claude_official = Provider::with_id(
            "claude-official".to_string(),
            "Claude Official".to_string(),
            json!({"env": {}}),
            Some("https://www.anthropic.com/claude-code".to_string()),
        );
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
