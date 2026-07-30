//! Deep link URL parser
//!
//! Parses ccswitch:// URLs into DeepLinkImportRequest structures.

use super::utils::validate_url;
use super::{DeepLinkImportRequest, DeepLinkRequest, ProviderSwitchRequest};
use crate::error::AppError;
use std::collections::{HashMap, HashSet};
use url::Url;

pub fn parse_deeplink_request_url(url_str: &str) -> Result<DeepLinkRequest, AppError> {
    let url = Url::parse(url_str)
        .map_err(|e| AppError::InvalidInput(format!("Invalid deep link URL: {e}")))?;
    let is_provider_switch = url
        .query_pairs()
        .any(|(key, value)| key == "resource" && value == "provider-switch");

    if !is_provider_switch {
        return parse_deeplink_url(url_str)
            .map(|request| DeepLinkRequest::Import(Box::new(request)));
    }

    if url.scheme() != "ccswitch" {
        return Err(AppError::InvalidInput(
            "Invalid scheme: expected 'ccswitch'".to_string(),
        ));
    }
    if !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || url.fragment().is_some()
    {
        return Err(AppError::InvalidInput(
            "Unsupported URL component in provider-switch request".to_string(),
        ));
    }
    let version = url
        .host_str()
        .ok_or_else(|| AppError::InvalidInput("Missing version in URL host".to_string()))?;
    if version != "v1" {
        return Err(AppError::InvalidInput(
            "Unsupported protocol version".to_string(),
        ));
    }
    if url.path() != "/import" {
        return Err(AppError::InvalidInput(
            "Invalid path: expected '/import'".to_string(),
        ));
    }

    let mut seen = HashSet::new();
    for (key, _) in url.query_pairs() {
        if !matches!(key.as_ref(), "resource" | "app" | "id") {
            return Err(AppError::InvalidInput(
                "Unsupported parameter in provider-switch request".to_string(),
            ));
        }
        if !seen.insert(key.into_owned()) {
            return Err(AppError::InvalidInput(
                "Duplicate parameter in provider-switch request".to_string(),
            ));
        }
    }

    let params: HashMap<String, String> = url.query_pairs().into_owned().collect();
    let app = params
        .get("app")
        .ok_or_else(|| AppError::InvalidInput("Missing 'app' parameter".to_string()))?;
    if app != "codex" {
        return Err(AppError::InvalidInput(
            "Provider switches support only the 'codex' app".to_string(),
        ));
    }
    let id = params
        .get("id")
        .filter(|id| !id.trim().is_empty())
        .ok_or_else(|| AppError::InvalidInput("Missing 'id' parameter".to_string()))?;

    Ok(DeepLinkRequest::ProviderSwitch(ProviderSwitchRequest {
        version: version.to_string(),
        resource: "provider-switch".to_string(),
        app: app.to_string(),
        id: id.to_string(),
    }))
}

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
        "claude" | "codex" | "gemini" | "grokbuild" | "opencode" | "openclaw" | "hermes"
    ) {
        return Err(AppError::InvalidInput(format!(
            "Invalid app type: must be 'claude', 'codex', 'gemini', 'grokbuild', 'opencode', 'openclaw', or 'hermes', got '{app}'"
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
    let notes = params.get("notes").cloned();
    let haiku_model = params.get("haikuModel").cloned();
    let sonnet_model = params.get("sonnetModel").cloned();
    let opus_model = params.get("opusModel").cloned();
    let icon = params
        .get("icon")
        .map(|v| v.trim().to_lowercase())
        .filter(|v| !v.is_empty());
    let config = params.get("config").cloned();
    let config_format = params.get("configFormat").cloned();
    let config_url = params.get("configUrl").cloned();
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

    Ok(DeepLinkImportRequest {
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
        content: None,
        description: None,
        apps: None,
        repo: None,
        directory: None,
        branch: None,
        config,
        config_format,
        config_url,
        usage_enabled,
        usage_script,
        usage_api_key,
        usage_base_url,
        usage_access_token,
        usage_user_id,
        usage_auto_interval,
    })
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
        "claude" | "codex" | "gemini" | "grokbuild" | "opencode" | "openclaw" | "hermes"
    ) {
        return Err(AppError::InvalidInput(format!(
            "Invalid app type: must be 'claude', 'codex', 'gemini', 'grokbuild', 'opencode', 'openclaw', or 'hermes', got '{app}'"
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
        apps: None,
        repo: None,
        directory: None,
        branch: None,
        config: None,
        config_format: None,
        config_url: None,
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
                | "grokbuild"
                | "grok"
                | "opencode"
                | "openclaw"
                | "hermes"
        ) {
            return Err(AppError::InvalidInput(format!(
                "Invalid app in 'apps': must be 'claude', 'codex', 'gemini', 'grokbuild', 'opencode', 'openclaw', or 'hermes', got '{trimmed}'"
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
        content: None,
        description: None,
        repo: None,
        directory: None,
        branch: None,
        config_url: None,
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
        app: Some("claude".to_string()), // Skills are Claude-only
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
        content: None,
        description: None,
        apps: None,
        config: None,
        config_format: None,
        config_url: None,
        usage_enabled: None,
        usage_script: None,
        usage_api_key: None,
        usage_base_url: None,
        usage_access_token: None,
        usage_user_id: None,
        usage_auto_interval: None,
    })
}
