//! External OpenAI-compatible API profile.
//!
//! This module owns the sidecar API surface used by third-party agents. It is
//! deliberately separate from Codex current provider, live config, and takeover
//! state.

use crate::app_config::AppType;
use crate::database::Database;
use crate::error::AppError;
use crate::provider::Provider;
use axum::http::HeaderMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::str::FromStr;

const PROFILE_SETTING_KEY: &str = "external_openai_api_profile_v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExternalOpenAiApiBackendType {
    Provider,
    CodexRouterRoute,
}

impl Default for ExternalOpenAiApiBackendType {
    fn default() -> Self {
        Self::Provider
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalOpenAiApiProfile {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub backend_type: ExternalOpenAiApiBackendType,
    #[serde(default)]
    pub app_type: Option<String>,
    #[serde(default)]
    pub provider_id: Option<String>,
    #[serde(default)]
    pub route_id: Option<String>,
    #[serde(default)]
    pub default_model: Option<String>,
    #[serde(default)]
    pub api_key_hash: Option<String>,
    #[serde(default)]
    pub api_key_prefix: Option<String>,
    #[serde(default)]
    pub updated_at: Option<i64>,
}

impl Default for ExternalOpenAiApiProfile {
    fn default() -> Self {
        Self {
            enabled: false,
            backend_type: ExternalOpenAiApiBackendType::Provider,
            app_type: None,
            provider_id: None,
            route_id: None,
            default_model: None,
            api_key_hash: None,
            api_key_prefix: None,
            updated_at: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalOpenAiApiProfileView {
    pub enabled: bool,
    pub backend_type: ExternalOpenAiApiBackendType,
    pub app_type: Option<String>,
    pub provider_id: Option<String>,
    pub route_id: Option<String>,
    pub default_model: Option<String>,
    pub api_key_prefix: Option<String>,
    pub has_api_key: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalOpenAiApiProfileUpdate {
    pub enabled: bool,
    pub backend_type: ExternalOpenAiApiBackendType,
    pub app_type: Option<String>,
    pub provider_id: Option<String>,
    pub route_id: Option<String>,
    pub default_model: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneratedExternalOpenAiApiKey {
    pub profile: ExternalOpenAiApiProfileView,
    #[serde(rename = "apiKey")]
    pub api_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalOpenAiApiBackendOptionView {
    pub key: String,
    pub backend_type: ExternalOpenAiApiBackendType,
    pub app_type: String,
    pub provider_id: String,
    pub route_id: Option<String>,
    pub label: String,
    pub description: String,
    pub models: Vec<String>,
    pub is_managed_oauth: bool,
    pub available: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalOpenAiApiRuntimeStatusView {
    pub profile: ExternalOpenAiApiProfileView,
    pub selected_backend: Option<ExternalOpenAiApiBackendOptionView>,
    pub backend_options: Vec<ExternalOpenAiApiBackendOptionView>,
    pub effective_model: Option<String>,
    pub ready: bool,
    pub issues: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExternalOpenAiApiAuthError {
    Disabled,
    MissingKey,
    InvalidKey,
}

/// 读取第三方 Agent API profile，并兼容旧版 `routerProviderId` 存档。
pub fn load_profile(db: &Database) -> Result<ExternalOpenAiApiProfile, AppError> {
    let Some(raw) = db.get_setting(PROFILE_SETTING_KEY)? else {
        return Ok(ExternalOpenAiApiProfile::default());
    };
    parse_profile(&raw)
}

/// 保存第三方 Agent API profile。
pub fn save_profile(db: &Database, profile: &ExternalOpenAiApiProfile) -> Result<(), AppError> {
    let raw = serde_json::to_string(profile).map_err(|e| {
        AppError::Database(format!(
            "External OpenAI API profile JSON serialize failed: {e}"
        ))
    })?;
    db.set_setting(PROFILE_SETTING_KEY, &raw)
}

/// 生成新的本地访问 key，明文只通过本次返回交给前端。
pub fn regenerate_api_key(db: &Database) -> Result<GeneratedExternalOpenAiApiKey, AppError> {
    let api_key = format!("ccsw_{}", uuid::Uuid::new_v4().simple());
    let mut profile = load_profile(db)?;
    profile.api_key_prefix = Some(api_key.chars().take(12).collect());
    profile.api_key_hash = Some(hash_api_key(&api_key));
    profile.updated_at = Some(chrono::Utc::now().timestamp());
    save_profile(db, &profile)?;
    Ok(GeneratedExternalOpenAiApiKey {
        profile: profile_view(&profile),
        api_key,
    })
}

/// 更新 profile 的后端目标和默认模型；API key 只能通过单独命令生成。
pub fn update_profile(
    db: &Database,
    update: ExternalOpenAiApiProfileUpdate,
) -> Result<ExternalOpenAiApiProfileView, AppError> {
    validate_update(&update)?;
    let mut profile = load_profile(db)?;
    profile.enabled = update.enabled;
    profile.backend_type = update.backend_type;
    profile.app_type = clean_optional(update.app_type);
    profile.provider_id = clean_optional(update.provider_id);
    profile.route_id = clean_optional(update.route_id);
    profile.default_model = clean_optional(update.default_model);
    profile.updated_at = Some(chrono::Utc::now().timestamp());
    save_profile(db, &profile)?;
    Ok(profile_view(&profile))
}

/// 生成前端可展示的脱敏 profile。
pub fn profile_view(profile: &ExternalOpenAiApiProfile) -> ExternalOpenAiApiProfileView {
    ExternalOpenAiApiProfileView {
        enabled: profile.enabled,
        backend_type: profile.backend_type,
        app_type: profile.app_type.clone(),
        provider_id: profile.provider_id.clone(),
        route_id: profile.route_id.clone(),
        default_model: profile.default_model.clone(),
        api_key_prefix: profile.api_key_prefix.clone(),
        has_api_key: profile.api_key_hash.is_some(),
    }
}

/// 读取第三方 Agent API 的运行时视图，作为前端展示的后端事实源。
pub fn runtime_status(db: &Database) -> Result<ExternalOpenAiApiRuntimeStatusView, AppError> {
    let profile = load_profile(db)?;
    let backend_options = list_backend_options(db)?;
    let selected_key = profile_backend_key(&profile);
    let selected_backend = selected_key
        .as_deref()
        .and_then(|key| backend_options.iter().find(|option| option.key == key))
        .cloned();
    let effective_model = profile.default_model.clone().or_else(|| {
        selected_backend
            .as_ref()
            .and_then(|backend| backend.models.first().cloned())
    });

    let mut issues = Vec::new();
    if !profile.enabled {
        issues.push("profile disabled".to_string());
    }
    if profile.api_key_hash.is_none() {
        issues.push("api key not generated".to_string());
    }
    if profile.provider_id.is_none() {
        issues.push("backend not selected".to_string());
    }
    match &selected_backend {
        Some(backend) if !backend.available => issues.push(
            backend
                .error
                .clone()
                .unwrap_or_else(|| "selected backend is unavailable".to_string()),
        ),
        None if profile.provider_id.is_some() => {
            issues.push("selected backend not found".to_string())
        }
        _ => {}
    }
    if effective_model.is_none() {
        issues.push("model not selected".to_string());
    }

    let ready = profile.enabled
        && profile.api_key_hash.is_some()
        && selected_backend
            .as_ref()
            .is_some_and(|backend| backend.available)
        && effective_model.is_some();

    Ok(ExternalOpenAiApiRuntimeStatusView {
        profile: profile_view(&profile),
        selected_backend,
        backend_options,
        effective_model,
        ready,
        issues,
    })
}

/// 列出可作为第三方 Agent API 后端的 provider 和 Codex router route。
pub fn list_backend_options(
    db: &Database,
) -> Result<Vec<ExternalOpenAiApiBackendOptionView>, AppError> {
    let mut options = Vec::new();
    for app_type in AppType::all() {
        let providers = db.get_all_providers(app_type.as_str())?;
        for provider in providers.values() {
            options.push(provider_backend_option(app_type.clone(), provider));
            if app_type == AppType::Codex && is_codex_router_provider(provider) {
                options.extend(router_backend_options(provider));
            }
        }
    }
    Ok(options)
}

/// 校验外部 OpenAI-compatible 请求的本地访问 key。
pub fn validate_request(
    db: &Database,
    headers: &HeaderMap,
) -> Result<ExternalOpenAiApiProfile, ExternalOpenAiApiAuthError> {
    let profile = load_profile(db).map_err(|_| ExternalOpenAiApiAuthError::Disabled)?;
    if !profile.enabled {
        return Err(ExternalOpenAiApiAuthError::Disabled);
    }
    let Some(expected_hash) = profile.api_key_hash.as_deref() else {
        return Err(ExternalOpenAiApiAuthError::MissingKey);
    };
    let Some(api_key) = extract_api_key(headers) else {
        return Err(ExternalOpenAiApiAuthError::MissingKey);
    };
    if hash_api_key(&api_key) != expected_hash {
        return Err(ExternalOpenAiApiAuthError::InvalidKey);
    }
    Ok(profile)
}

/// 判断请求是否显式携带 CC Switch 外部 API key。
///
/// Codex 自身也可能带 User-Agent，因此只要出现 `ccsw_` key，就优先按
/// External API profile 处理，避免第三方 agent 因 UA 命名误入 Codex current provider。
pub fn has_external_api_key(headers: &HeaderMap) -> bool {
    extract_api_key(headers).is_some_and(|api_key| api_key.starts_with("ccsw_"))
}

/// 解析 profile JSON；旧版仅有 routerProviderId 时映射为 Codex router target。
fn parse_profile(raw: &str) -> Result<ExternalOpenAiApiProfile, AppError> {
    let value: serde_json::Value = serde_json::from_str(raw).map_err(|e| {
        AppError::Database(format!("External OpenAI API profile JSON invalid: {e}"))
    })?;
    let mut profile: ExternalOpenAiApiProfile =
        serde_json::from_value(value.clone()).map_err(|e| {
            AppError::Database(format!("External OpenAI API profile JSON invalid: {e}"))
        })?;

    if profile.provider_id.is_none() {
        if let Some(router_provider_id) = value
            .get("routerProviderId")
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            profile.backend_type = ExternalOpenAiApiBackendType::CodexRouterRoute;
            profile.app_type = Some(AppType::Codex.as_str().to_string());
            profile.provider_id = Some(router_provider_id.to_string());
        }
    }

    Ok(profile)
}

/// 校验 profile 更新参数，避免保存无法路由的半成品 target。
fn validate_update(update: &ExternalOpenAiApiProfileUpdate) -> Result<(), AppError> {
    if let Some(app_type) = clean_optional(update.app_type.clone()) {
        AppType::from_str(&app_type)?;
    }
    if update.enabled && clean_optional(update.provider_id.clone()).is_none() {
        return Err(AppError::Config(
            "External OpenAI API enabled profile requires providerId".to_string(),
        ));
    }
    if update.backend_type == ExternalOpenAiApiBackendType::CodexRouterRoute
        && update.enabled
        && clean_optional(update.app_type.clone()).as_deref() != Some(AppType::Codex.as_str())
    {
        return Err(AppError::Config(
            "Codex router route backend must use appType=codex".to_string(),
        ));
    }
    Ok(())
}

/// 生成 profile 对应的稳定后端 key，用于前后端选项匹配。
fn profile_backend_key(profile: &ExternalOpenAiApiProfile) -> Option<String> {
    Some(build_backend_key(
        profile.backend_type,
        profile.app_type.as_deref()?,
        profile.provider_id.as_deref()?,
        profile.route_id.as_deref(),
    ))
}

/// 生成前端和后端共享的后端选项 key。
fn build_backend_key(
    backend_type: ExternalOpenAiApiBackendType,
    app_type: &str,
    provider_id: &str,
    route_id: Option<&str>,
) -> String {
    let backend_type = match backend_type {
        ExternalOpenAiApiBackendType::Provider => "provider",
        ExternalOpenAiApiBackendType::CodexRouterRoute => "codex_router_route",
    };
    format!(
        "{backend_type}::{app_type}::{provider_id}::{}",
        route_id.unwrap_or("")
    )
}

/// 将普通 provider 转成第三方 API 页面可展示的后端选项。
fn provider_backend_option(
    app_type: AppType,
    provider: &Provider,
) -> ExternalOpenAiApiBackendOptionView {
    let is_managed_oauth = provider.uses_managed_account_auth()
        || is_codex_official_managed_oauth_provider(&app_type, provider);
    let mut models = collect_provider_models(provider);
    if is_managed_oauth && models.is_empty() && app_type == AppType::Codex {
        models = default_codex_oauth_models();
    }
    let (base_url, api_key) = provider.resolve_usage_credentials(&app_type);
    let is_openai_compatible = provider_can_be_openai_compatible(&app_type, provider);
    let has_credentials =
        is_managed_oauth || (!base_url.trim().is_empty() && !api_key.trim().is_empty());
    let available = is_openai_compatible && has_credentials;
    ExternalOpenAiApiBackendOptionView {
        key: build_backend_key(
            ExternalOpenAiApiBackendType::Provider,
            app_type.as_str(),
            &provider.id,
            None,
        ),
        backend_type: ExternalOpenAiApiBackendType::Provider,
        app_type: app_type.as_str().to_string(),
        provider_id: provider.id.clone(),
        route_id: None,
        label: format!("{} ({})", provider.name, app_type.as_str()),
        description: if is_managed_oauth {
            "Managed OAuth provider".to_string()
        } else if is_openai_compatible {
            "OpenAI-compatible provider".to_string()
        } else {
            "Native provider".to_string()
        },
        models,
        is_managed_oauth,
        available,
        error: if available {
            None
        } else if !is_openai_compatible {
            Some(
                "provider uses a native protocol that is not safe to expose as OpenAI-compatible"
                    .to_string(),
            )
        } else {
            Some(
                "provider has no usable base URL or credential for OpenAI-compatible forwarding"
                    .to_string(),
            )
        },
    }
}

/// 将 Codex router 的每条 route 展开成独立后端选项。
fn router_backend_options(provider: &Provider) -> Vec<ExternalOpenAiApiBackendOptionView> {
    let mut options = Vec::new();
    let Some(routes) = provider
        .settings_config
        .pointer("/codexRouting/routes")
        .and_then(|routes| routes.as_array())
    else {
        return options;
    };

    for route in routes {
        if route.get("enabled").and_then(|value| value.as_bool()) == Some(false) {
            continue;
        }
        let Some(route_id) = route.get("id").and_then(|value| value.as_str()) else {
            continue;
        };
        let label = route
            .get("label")
            .and_then(|value| value.as_str())
            .unwrap_or(route_id);
        let availability = route_backend_availability(provider, route);
        options.push(ExternalOpenAiApiBackendOptionView {
            key: build_backend_key(
                ExternalOpenAiApiBackendType::CodexRouterRoute,
                AppType::Codex.as_str(),
                &provider.id,
                Some(route_id),
            ),
            backend_type: ExternalOpenAiApiBackendType::CodexRouterRoute,
            app_type: AppType::Codex.as_str().to_string(),
            provider_id: provider.id.clone(),
            route_id: Some(route_id.to_string()),
            label: format!("{} / {}", provider.name, label),
            description: "Codex router route".to_string(),
            models: collect_route_models(route),
            is_managed_oauth: route_uses_managed_oauth(route),
            available: availability.0,
            error: availability.1,
        });
    }
    options
}

/// 判断普通 provider 能否安全作为 OpenAI-compatible 外部 API 后端。
fn provider_can_be_openai_compatible(app_type: &AppType, provider: &Provider) -> bool {
    if provider.uses_managed_account_auth()
        || is_codex_official_managed_oauth_provider(app_type, provider)
    {
        return true;
    }
    match app_type {
        AppType::Codex | AppType::OpenCode | AppType::OpenClaw | AppType::Hermes => true,
        AppType::Claude | AppType::ClaudeDesktop | AppType::Gemini => false,
    }
}

/// 判断 Codex 内置官方源是否应按托管 ChatGPT/Codex OAuth 处理。
///
/// 该 provider 的 config/auth 为空是设计行为，用来表达“走 Codex 官方登录态”，
/// 因此第三方 Agent API 不能按普通 OpenAI-compatible provider 的 base_url/key 规则拦截它。
fn is_codex_official_managed_oauth_provider(app_type: &AppType, provider: &Provider) -> bool {
    app_type == &AppType::Codex && provider.id == "codex-official"
}

/// 给空配置的 Codex 官方 OAuth 源提供可选模型，避免 UI 只能手填模型名。
fn default_codex_oauth_models() -> Vec<String> {
    ["gpt-5.5", "gpt-5.4", "gpt-5.4-mini", "gpt-5.3-codex-spark"]
        .into_iter()
        .map(ToOwned::to_owned)
        .collect()
}

/// 判断 Codex router route 是否有足够信息解析到真实上游。
fn route_backend_availability(provider: &Provider, route: &Value) -> (bool, Option<String>) {
    if route_uses_managed_oauth(route) {
        return (true, None);
    }

    let upstream = route.get("upstream").unwrap_or(route);
    let route_base_url = upstream
        .get("baseUrl")
        .or_else(|| upstream.get("base_url"))
        .or_else(|| route.get("baseUrl"))
        .or_else(|| route.get("base_url"))
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let route_api_key = upstream
        .get("apiKey")
        .or_else(|| upstream.get("api_key"))
        .or_else(|| route.get("apiKey"))
        .or_else(|| route.get("api_key"))
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let (provider_base_url, provider_api_key) = provider.resolve_usage_credentials(&AppType::Codex);
    let has_base_url = route_base_url.is_some() || !provider_base_url.trim().is_empty();
    let has_api_key = route_api_key.is_some() || !provider_api_key.trim().is_empty();

    if has_base_url && has_api_key {
        (true, None)
    } else {
        (
            false,
            Some(
                "route needs managed OAuth, an inline API key, or provider credentials".to_string(),
            ),
        )
    }
}

/// 判断 provider 是否是显式开启的 Codex router。
fn is_codex_router_provider(provider: &Provider) -> bool {
    provider
        .settings_config
        .get("codexRouting")
        .and_then(|routing| routing.get("enabled"))
        .and_then(|enabled| enabled.as_bool())
        .unwrap_or(false)
}

/// 从 provider 配置中提取页面和 /v1/models 可展示的模型 id。
fn collect_provider_models(provider: &Provider) -> Vec<String> {
    let mut ids = BTreeSet::new();
    collect_model_string(&mut ids, provider.settings_config.get("model"));
    collect_model_string(&mut ids, provider.settings_config.get("defaultModel"));
    collect_model_array(&mut ids, provider.settings_config.get("models"));
    collect_model_array(&mut ids, provider.settings_config.get("modelList"));
    collect_model_array(&mut ids, provider.settings_config.get("modelCatalog"));
    ids.into_iter().collect()
}

/// 从 router route 的 match 规则中提取模型 id；prefix 以星号标记展示。
fn collect_route_models(route: &Value) -> Vec<String> {
    let mut ids = BTreeSet::new();
    collect_model_array(&mut ids, route.pointer("/match/models"));
    if ids.is_empty() {
        if let Some(prefixes) = route
            .pointer("/match/prefixes")
            .and_then(|value| value.as_array())
        {
            for prefix in prefixes {
                if let Some(prefix) = prefix
                    .as_str()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                {
                    ids.insert(format!("{prefix}*"));
                }
            }
        }
    }
    ids.into_iter().collect()
}

/// 收集单个模型字段。
fn collect_model_string(ids: &mut BTreeSet<String>, value: Option<&Value>) {
    if let Some(model) = value
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        ids.insert(model.to_string());
    }
}

/// 收集字符串数组或对象数组中的模型字段。
fn collect_model_array(ids: &mut BTreeSet<String>, value: Option<&Value>) {
    let Some(values) = value.and_then(|value| value.as_array()) else {
        return;
    };
    for value in values {
        if let Some(model) = value.as_str() {
            collect_model_string(ids, Some(&Value::String(model.to_string())));
        } else {
            collect_model_string(
                ids,
                value
                    .get("id")
                    .or_else(|| value.get("model"))
                    .or_else(|| value.get("name")),
            );
        }
    }
}

/// 判断 router route 是否交给 CC Switch 管理的 OAuth/账号认证。
fn route_uses_managed_oauth(route: &Value) -> bool {
    matches!(
        route
            .pointer("/upstream/auth/source")
            .and_then(|value| value.as_str()),
        Some("managed_codex_oauth" | "managed_account")
    )
}

/// 从请求头提取 OpenAI SDK 传入的 key。
fn extract_api_key(headers: &HeaderMap) -> Option<String> {
    headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| {
            headers
                .get("x-api-key")
                .and_then(|value| value.to_str().ok())
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
        })
}

/// 计算 API key 的 SHA-256 hex hash。
fn hash_api_key(api_key: &str) -> String {
    let digest = Sha256::digest(api_key.as_bytes());
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// 清理可选字符串配置，空白字符串按未设置处理。
fn clean_optional(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::Provider;
    use axum::http::{HeaderMap, HeaderValue};
    use serde_json::json;

    #[test]
    fn generated_key_is_hashed_and_validates_from_authorization_header() {
        let db = Database::memory().expect("memory db");
        let generated = regenerate_api_key(&db).expect("generate key");
        update_profile(
            &db,
            ExternalOpenAiApiProfileUpdate {
                enabled: true,
                backend_type: ExternalOpenAiApiBackendType::Provider,
                app_type: Some("codex".to_string()),
                provider_id: Some("provider".to_string()),
                route_id: None,
                default_model: Some("gpt-5.4-mini".to_string()),
            },
        )
        .expect("enable profile");

        let stored = load_profile(&db).expect("load profile");
        assert_ne!(
            stored.api_key_hash.as_deref(),
            Some(generated.api_key.as_str())
        );
        assert!(stored
            .api_key_prefix
            .as_deref()
            .is_some_and(|prefix| prefix.starts_with("ccsw_")));

        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {}", generated.api_key)).unwrap(),
        );

        assert!(validate_request(&db, &headers).is_ok());
    }

    #[test]
    fn generated_key_validates_from_x_api_key_header() {
        let db = Database::memory().expect("memory db");
        let generated = regenerate_api_key(&db).expect("generate key");
        update_profile(
            &db,
            ExternalOpenAiApiProfileUpdate {
                enabled: true,
                backend_type: ExternalOpenAiApiBackendType::Provider,
                app_type: Some("codex".to_string()),
                provider_id: Some("provider".to_string()),
                route_id: None,
                default_model: None,
            },
        )
        .expect("enable profile");

        let mut headers = HeaderMap::new();
        headers.insert(
            "x-api-key",
            HeaderValue::from_str(&generated.api_key).unwrap(),
        );

        assert!(validate_request(&db, &headers).is_ok());
    }

    #[test]
    fn invalid_key_is_rejected() {
        let db = Database::memory().expect("memory db");
        regenerate_api_key(&db).expect("generate key");
        update_profile(
            &db,
            ExternalOpenAiApiProfileUpdate {
                enabled: true,
                backend_type: ExternalOpenAiApiBackendType::Provider,
                app_type: Some("codex".to_string()),
                provider_id: Some("provider".to_string()),
                route_id: None,
                default_model: None,
            },
        )
        .expect("enable profile");

        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::AUTHORIZATION,
            HeaderValue::from_static("Bearer ccsw_wrong"),
        );

        let err = validate_request(&db, &headers).unwrap_err();
        assert_eq!(err, ExternalOpenAiApiAuthError::InvalidKey);
    }

    #[test]
    fn disabled_profile_rejects_external_request() {
        let db = Database::memory().expect("memory db");
        let err = validate_request(&db, &HeaderMap::new()).unwrap_err();
        assert_eq!(err, ExternalOpenAiApiAuthError::Disabled);
    }

    #[test]
    fn legacy_router_provider_id_is_mapped_to_codex_router_backend() {
        let raw = r#"{
            "enabled": true,
            "routerProviderId": "codex-openai-router",
            "defaultModel": "gpt-5.4-mini",
            "apiKeyHash": "hash",
            "apiKeyPrefix": "ccsw_old"
        }"#;

        let profile = parse_profile(raw).expect("legacy profile");

        assert_eq!(
            profile.backend_type,
            ExternalOpenAiApiBackendType::CodexRouterRoute
        );
        assert_eq!(profile.app_type.as_deref(), Some("codex"));
        assert_eq!(profile.provider_id.as_deref(), Some("codex-openai-router"));
    }

    #[test]
    fn runtime_status_resolves_selected_provider_backend() {
        let db = Database::memory().expect("memory db");
        let provider = Provider::with_id(
            "hermes-openai".to_string(),
            "Hermes OpenAI".to_string(),
            json!({
                "base_url": "https://example.com/v1",
                "api_key": "sk-placeholder",
                "models": ["gpt-test"]
            }),
            None,
        );
        db.save_provider("hermes", &provider)
            .expect("save provider");
        regenerate_api_key(&db).expect("generate key");
        update_profile(
            &db,
            ExternalOpenAiApiProfileUpdate {
                enabled: true,
                backend_type: ExternalOpenAiApiBackendType::Provider,
                app_type: Some("hermes".to_string()),
                provider_id: Some("hermes-openai".to_string()),
                route_id: None,
                default_model: Some("gpt-test".to_string()),
            },
        )
        .expect("enable profile");

        let status = runtime_status(&db).expect("runtime status");

        assert!(status.ready);
        assert_eq!(status.effective_model.as_deref(), Some("gpt-test"));
        assert_eq!(
            status
                .selected_backend
                .as_ref()
                .map(|backend| backend.key.as_str()),
            Some("provider::hermes::hermes-openai::")
        );
    }

    #[test]
    fn runtime_status_marks_provider_without_credentials_unavailable() {
        let db = Database::memory().expect("memory db");
        let provider = Provider::with_id(
            "empty-provider".to_string(),
            "Empty Provider".to_string(),
            json!({ "models": ["gpt-test"] }),
            None,
        );
        db.save_provider("hermes", &provider)
            .expect("save provider");
        regenerate_api_key(&db).expect("generate key");
        update_profile(
            &db,
            ExternalOpenAiApiProfileUpdate {
                enabled: true,
                backend_type: ExternalOpenAiApiBackendType::Provider,
                app_type: Some("hermes".to_string()),
                provider_id: Some("empty-provider".to_string()),
                route_id: None,
                default_model: Some("gpt-test".to_string()),
            },
        )
        .expect("enable profile");

        let status = runtime_status(&db).expect("runtime status");

        assert!(!status.ready);
        assert_eq!(
            status
                .selected_backend
                .as_ref()
                .map(|backend| backend.available),
            Some(false)
        );
        assert!(status
            .issues
            .iter()
            .any(|issue| issue.contains("no usable base URL")));
    }

    #[test]
    fn runtime_status_marks_native_provider_unavailable_for_openai_api() {
        let db = Database::memory().expect("memory db");
        let provider = Provider::with_id(
            "claude-native".to_string(),
            "Claude Native".to_string(),
            json!({
                "env": {
                    "ANTHROPIC_BASE_URL": "https://api.anthropic.com",
                    "ANTHROPIC_AUTH_TOKEN": "sk-ant-placeholder"
                },
                "models": ["claude-sonnet"]
            }),
            None,
        );
        db.save_provider("claude", &provider)
            .expect("save provider");
        regenerate_api_key(&db).expect("generate key");
        update_profile(
            &db,
            ExternalOpenAiApiProfileUpdate {
                enabled: true,
                backend_type: ExternalOpenAiApiBackendType::Provider,
                app_type: Some("claude".to_string()),
                provider_id: Some("claude-native".to_string()),
                route_id: None,
                default_model: Some("claude-sonnet".to_string()),
            },
        )
        .expect("enable profile");

        let status = runtime_status(&db).expect("runtime status");

        assert!(!status.ready);
        let backend = status.selected_backend.expect("selected backend");
        assert!(!backend.available);
        assert_eq!(backend.description, "Native provider");
        assert!(backend
            .error
            .as_deref()
            .is_some_and(|error| error.contains("native protocol")));
    }

    #[test]
    fn runtime_status_marks_codex_chatgpt_backend_as_managed_oauth() {
        let db = Database::memory().expect("memory db");
        let provider = Provider::with_id(
            "openai-official-backup".to_string(),
            "OpenAI Official Backup".to_string(),
            json!({
                "config": "model_provider = \"custom\"\n\
                           [model_providers.custom]\n\
                           base_url = \"https://chatgpt.com/backend-api/codex\"\n\
                           wire_api = \"responses\"\n",
                "models": ["gpt-5.4-mini"]
            }),
            None,
        );
        db.save_provider("codex", &provider).expect("save provider");

        let options = list_backend_options(&db).expect("backend options");
        let backend = options
            .into_iter()
            .find(|option| option.provider_id == "openai-official-backup")
            .expect("official backup backend option");

        assert!(backend.available);
        assert!(backend.is_managed_oauth);
        assert_eq!(backend.description, "Managed OAuth provider");
    }

    #[test]
    fn runtime_status_marks_empty_codex_official_seed_as_managed_oauth() {
        let db = Database::memory().expect("memory db");
        let provider = Provider::with_id(
            "codex-official".to_string(),
            "OpenAI Official".to_string(),
            json!({ "auth": {}, "config": "" }),
            None,
        );
        db.save_provider("codex", &provider).expect("save provider");

        let options = list_backend_options(&db).expect("backend options");
        let backend = options
            .into_iter()
            .find(|option| option.provider_id == "codex-official")
            .expect("codex official backend option");

        assert!(backend.available);
        assert!(backend.is_managed_oauth);
        assert!(backend.models.iter().any(|model| model == "gpt-5.4-mini"));
        assert_eq!(backend.description, "Managed OAuth provider");
    }

    #[test]
    fn runtime_status_marks_router_route_without_credentials_unavailable() {
        let db = Database::memory().expect("memory db");
        let provider = Provider::with_id(
            "codex-router".to_string(),
            "Codex Router".to_string(),
            json!({
                "codexRouting": {
                    "enabled": true,
                    "routes": [{
                        "id": "empty-route",
                        "match": { "models": ["gpt-empty"] },
                        "upstream": {
                            "apiFormat": "openai_chat",
                            "auth": { "source": "provider_config" }
                        }
                    }]
                }
            }),
            None,
        );
        db.save_provider("codex", &provider).expect("save provider");
        regenerate_api_key(&db).expect("generate key");
        update_profile(
            &db,
            ExternalOpenAiApiProfileUpdate {
                enabled: true,
                backend_type: ExternalOpenAiApiBackendType::CodexRouterRoute,
                app_type: Some("codex".to_string()),
                provider_id: Some("codex-router".to_string()),
                route_id: Some("empty-route".to_string()),
                default_model: Some("gpt-empty".to_string()),
            },
        )
        .expect("enable profile");

        let status = runtime_status(&db).expect("runtime status");

        assert!(!status.ready);
        let backend = status.selected_backend.expect("selected backend");
        assert!(!backend.available);
        assert!(backend
            .error
            .as_deref()
            .is_some_and(|error| error.contains("route needs managed OAuth")));
    }

    #[test]
    fn ccsw_key_header_is_detected_even_with_codex_user_agent() {
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::USER_AGENT,
            HeaderValue::from_static("codex-compatible-agent"),
        );
        headers.insert(
            axum::http::header::AUTHORIZATION,
            HeaderValue::from_static("Bearer ccsw_test"),
        );

        assert!(has_external_api_key(&headers));
    }
}
