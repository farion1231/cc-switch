//! Deep link URL parser
//!
//! Parses ccswitch:// URLs into DeepLinkImportRequest structures.

use super::utils::validate_url;
use super::DeepLinkImportRequest;
use crate::error::AppError;
use std::collections::HashMap;
use url::Url;

/// Parse a ccswitch:// URL into a DeepLinkImportRequest
///
/// Expected format:
/// ccswitch://v1/import?resource={type}&...
pub fn parse_deeplink_url(url_str: &str) -> Result<DeepLinkImportRequest, AppError> {
    // Parse URL
    let url = Url::parse(url_str)
        .map_err(|e| AppError::InvalidInput(format!("Invalid deep link URL: {e}")))?;

    // Validate scheme
    let scheme = url.scheme();
    if scheme != "ccswitch" {
        return Err(AppError::InvalidInput(format!(
            "Invalid scheme: expected 'ccswitch', got '{scheme}'"
        )));
    }

    // Extract version from host
    let version = url
        .host_str()
        .ok_or_else(|| AppError::InvalidInput("Missing version in URL host".to_string()))?
        .to_string();

    // Validate version
    if version != "v1" {
        return Err(AppError::InvalidInput(format!(
            "Unsupported protocol version: {version}"
        )));
    }

    // Extract path (should be "/import")
    let path = url.path();
    if path != "/import" {
        return Err(AppError::InvalidInput(format!(
            "Invalid path: expected '/import', got '{path}'"
        )));
    }

    // Parse query parameters
    let params: HashMap<String, String> = url.query_pairs().into_owned().collect();

    // Extract and validate resource type
    let resource = params
        .get("resource")
        .ok_or_else(|| AppError::InvalidInput("Missing 'resource' parameter".to_string()))?
        .clone();

    // Dispatch to appropriate parser based on resource type
    match resource.as_str() {
        "provider" => parse_provider_deeplink(&params, version, resource),
        "prompt" => parse_prompt_deeplink(&params, version, resource),
        "mcp" => parse_mcp_deeplink(&params, version, resource),
        "skill" => parse_skill_deeplink(&params, version, resource),
        _ => Err(AppError::InvalidInput(format!(
            "Unsupported resource type: {resource}"
        ))),
    }
}

/// Parse provider deep link parameters
/// 内置图标名的形状：小写字母、数字、`-`、`_`、`.`，不超过 64 个字符。
/// 只看形状，不核对图标是否存在（前端找不到时显示首字母）。
fn is_builtin_icon_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '-' | '_' | '.'))
}

fn parse_provider_deeplink(
    params: &HashMap<String, String>,
    version: String,
    resource: String,
) -> Result<DeepLinkImportRequest, AppError> {
    let app = params
        .get("app")
        .ok_or_else(|| AppError::InvalidInput("Missing 'app' parameter".to_string()))?
        .clone();

    // Validate app type
    if !matches!(
        app.as_str(),
        "agy"
            | "claude"
            | "codex"
            | "gemini"
            | "grokbuild"
            | "opencode"
            | "openclaw"
            | "hermes"
            | "pi"
    ) {
        return Err(AppError::InvalidInput(format!(
            "Invalid provider app type: '{app}'"
        )));
    }

    let name = params
        .get("name")
        .ok_or_else(|| AppError::InvalidInput("Missing 'name' parameter".to_string()))?
        .clone();

    // Make these optional for config file auto-fill (v3.8+)
    let homepage = params.get("homepage").cloned();
    let endpoint = params.get("endpoint").cloned();
    let api_key = params.get("apiKey").cloned();

    // Validate URLs only if provided
    if let Some(ref hp) = homepage {
        if !hp.is_empty() {
            validate_url(hp, "homepage")?;
        }
    }
    // Validate each endpoint (supports comma-separated multiple URLs)
    if let Some(ref ep) = endpoint {
        for (i, url) in ep.split(',').enumerate() {
            let trimmed = url.trim();
            if !trimmed.is_empty() {
                validate_url(trimmed, &format!("endpoint[{i}]"))?;
            }
        }
    }

    // Extract optional fields
    let model = params.get("model").cloned();
    // Pi 的节点没有模型就选不到任何模型（见 `provider::build_pi_settings`）
    if app == "pi" && model.as_deref().is_none_or(|model| model.trim().is_empty()) {
        return Err(AppError::InvalidInput(
            "Pi providers need a model; add 'model' to the link".to_string(),
        ));
    }
    let notes = params.get("notes").cloned();
    let haiku_model = params.get("haikuModel").cloned();
    let sonnet_model = params.get("sonnetModel").cloned();
    let opus_model = params.get("opusModel").cloned();
    // 在解析阶段就核对，用户当场看到「这个应用不支持这种接口」，而不是确认导入之后才失败
    let api_format = params.get("apiFormat").cloned();
    if let Some(ref format) = api_format {
        super::provider::check_api_format(&app, format)?;
    }
    // icon 只收内置图标名。图片要走下载后重新编码的流程，不能借这个参数直接把
    // data URI 之类的内容塞进库；不合格的值丢掉，退回首字母图标，不让整条链接失败
    let icon = params
        .get("icon")
        .map(|v| v.trim().to_lowercase())
        .filter(|v| is_builtin_icon_name(v));
    // iconUrl 只在确认框里由专门的命令下载（见 `icon_fetch`）。形状不合格的地址同样丢掉，
    // 不让整条链接失败
    let icon_url = params
        .get("iconUrl")
        .and_then(|v| super::icon_fetch::parse_icon_url(v))
        .map(|url| url.to_string());
    let config = params.get("config").cloned();
    let config_format = params.get("configFormat").cloned();
    // 远程配置还没实现。在解析阶段就拒绝，用户当场看到原因，而不是确认导入之后才失败
    if params.contains_key("configUrl") {
        return Err(AppError::InvalidInput(
            "'configUrl' is not supported yet; pass the config inline with 'config' (Base64)"
                .to_string(),
        ));
    }
    let config_url = None;
    let enabled = params.get("enabled").and_then(|v| v.parse::<bool>().ok());

    // Extract usage script fields (v3.9+)
    let usage_enabled = params
        .get("usageEnabled")
        .and_then(|v| v.parse::<bool>().ok());
    let usage_script = params.get("usageScript").cloned();
    let usage_api_key = params.get("usageApiKey").cloned();
    let usage_base_url = params.get("usageBaseUrl").cloned();
    let usage_access_token = params.get("usageAccessToken").cloned();
    let usage_user_id = params.get("usageUserId").cloned();
    let usage_auto_interval = params
        .get("usageAutoInterval")
        .and_then(|v| v.parse::<u64>().ok());

    let request = DeepLinkImportRequest {
        version,
        resource,
        app: Some(app),
        name: Some(name),
        enabled,
        homepage,
        endpoint,
        api_key,
        icon,
        model,
        notes,
        haiku_model,
        sonnet_model,
        opus_model,
        api_format,
        content: None,
        description: None,
        apps: None,
        repo: None,
        directory: None,
        branch: None,
        config,
        config_format,
        config_url,
        icon_url,
        usage_enabled,
        usage_script,
        usage_api_key,
        usage_base_url,
        usage_access_token,
        usage_user_id,
        usage_auto_interval,
    };
    // 累加式应用会把某些写法当成命令、环境变量或文件引用，解析阶段就拒绝
    if let Ok(app_type) = request
        .app
        .as_deref()
        .unwrap_or("")
        .parse::<crate::AppType>()
    {
        super::provider::check_link_values(&app_type, &request)?;
    }
    Ok(request)
}

/// Parse prompt deep link parameters
fn parse_prompt_deeplink(
    params: &HashMap<String, String>,
    version: String,
    resource: String,
) -> Result<DeepLinkImportRequest, AppError> {
    let app = params
        .get("app")
        .ok_or_else(|| AppError::InvalidInput("Missing 'app' parameter for prompt".to_string()))?
        .clone();

    // Validate app type
    if !matches!(
        app.as_str(),
        "claude"
            | "codex"
            | "gemini"
            | "agy"
            | "grokbuild"
            | "opencode"
            | "openclaw"
            | "hermes"
            | "pi"
            | "mcode"
    ) {
        return Err(AppError::InvalidInput(format!(
            "Invalid app type: must be 'claude', 'codex', 'gemini', 'agy', 'grokbuild', 'opencode', 'openclaw', 'hermes', 'pi', or 'mcode', got '{app}'"
        )));
    }

    let name = params
        .get("name")
        .ok_or_else(|| AppError::InvalidInput("Missing 'name' parameter for prompt".to_string()))?
        .clone();

    let content = params
        .get("content")
        .ok_or_else(|| {
            AppError::InvalidInput("Missing 'content' parameter for prompt".to_string())
        })?
        .clone();

    let description = params.get("description").cloned();
    let enabled = params.get("enabled").and_then(|v| v.parse::<bool>().ok());

    Ok(DeepLinkImportRequest {
        version,
        resource,
        app: Some(app),
        name: Some(name),
        enabled,
        content: Some(content),
        description,
        icon: None,
        homepage: None,
        endpoint: None,
        api_key: None,
        model: None,
        notes: None,
        haiku_model: None,
        sonnet_model: None,
        opus_model: None,
        api_format: None,
        apps: None,
        repo: None,
        directory: None,
        branch: None,
        config: None,
        config_format: None,
        config_url: None,
        icon_url: None,
        usage_enabled: None,
        usage_script: None,
        usage_api_key: None,
        usage_base_url: None,
        usage_access_token: None,
        usage_user_id: None,
        usage_auto_interval: None,
    })
}

/// Parse MCP deep link parameters
fn parse_mcp_deeplink(
    params: &HashMap<String, String>,
    version: String,
    resource: String,
) -> Result<DeepLinkImportRequest, AppError> {
    let apps = params
        .get("apps")
        .ok_or_else(|| AppError::InvalidInput("Missing 'apps' parameter for MCP".to_string()))?
        .clone();

    // Validate apps format
    for app in apps.split(',') {
        let trimmed = app.trim();
        if !matches!(
            trimmed,
            "claude"
                | "codex"
                | "gemini"
                | "agy"
                | "grokbuild"
                | "grok"
                | "opencode"
                | "openclaw"
                | "hermes"
                | "pi"
                | "mcode"
        ) {
            return Err(AppError::InvalidInput(format!(
                "Invalid app in 'apps': must be 'claude', 'codex', 'gemini', 'agy', 'grokbuild', 'opencode', 'hermes', 'pi', or 'mcode', got '{trimmed}'"
            )));
        }
    }

    let config = params
        .get("config")
        .ok_or_else(|| AppError::InvalidInput("Missing 'config' parameter for MCP".to_string()))?
        .clone();

    let enabled = params.get("enabled").and_then(|v| v.parse::<bool>().ok());

    Ok(DeepLinkImportRequest {
        version,
        resource,
        apps: Some(apps),
        enabled,
        config: Some(config),
        config_format: Some("json".to_string()), // MCP config is always JSON
        app: None,
        name: None,
        icon: None,
        homepage: None,
        endpoint: None,
        api_key: None,
        model: None,
        notes: None,
        haiku_model: None,
        sonnet_model: None,
        opus_model: None,
        api_format: None,
        content: None,
        description: None,
        repo: None,
        directory: None,
        branch: None,
        config_url: None,
        icon_url: None,
        usage_enabled: None,
        usage_script: None,
        usage_api_key: None,
        usage_base_url: None,
        usage_access_token: None,
        usage_user_id: None,
        usage_auto_interval: None,
    })
}

/// Parse skill deep link parameters
fn parse_skill_deeplink(
    params: &HashMap<String, String>,
    version: String,
    resource: String,
) -> Result<DeepLinkImportRequest, AppError> {
    let repo = params
        .get("repo")
        .ok_or_else(|| AppError::InvalidInput("Missing 'repo' parameter for skill".to_string()))?
        .clone();

    // Validate repo format (should be "owner/name")
    if !repo.contains('/') || repo.split('/').count() != 2 {
        return Err(AppError::InvalidInput(format!(
            "Invalid repo format: expected 'owner/name', got '{repo}'"
        )));
    }

    let directory = params.get("directory").cloned();
    let branch = params.get("branch").cloned();

    Ok(DeepLinkImportRequest {
        version,
        resource,
        repo: Some(repo),
        directory,
        branch,
        icon: None,
        app: None, // Skill links add a shared repository; app installation is a separate action.
        name: None,
        enabled: None,
        homepage: None,
        endpoint: None,
        api_key: None,
        model: None,
        notes: None,
        haiku_model: None,
        sonnet_model: None,
        opus_model: None,
        api_format: None,
        content: None,
        description: None,
        apps: None,
        config: None,
        config_format: None,
        config_url: None,
        icon_url: None,
        usage_enabled: None,
        usage_script: None,
        usage_api_key: None,
        usage_base_url: None,
        usage_access_token: None,
        usage_user_id: None,
        usage_auto_interval: None,
    })
}
