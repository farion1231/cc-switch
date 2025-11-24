use crate::app_config::{McpApps, McpServer};
/// Deep link import functionality for CC Switch
///
/// This module implements the ccswitch:// protocol for importing provider configurations
/// via deep links. See docs/ccswitch-deeplink-design.md for detailed design.
use crate::error::AppError;
use crate::prompt::Prompt;
use crate::provider::Provider;
use crate::services::skill::SkillRepo;
use crate::services::ProviderService;
use crate::store::AppState;
use crate::AppType;
use base64::prelude::*;
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::str::FromStr;
use url::Url;

/// Deep link import request model
/// Represents a parsed ccswitch:// URL ready for processing
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeepLinkImportRequest {
    /// Protocol version (e.g., "v1")
    pub version: String,
    /// Resource type to import: "provider" | "prompt" | "mcp" | "skill"
    pub resource: String,

    // ============ Common fields ============
    /// Target application (claude/codex/gemini) - for provider, prompt, skill
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app: Option<String>,
    /// Resource name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Whether to enable after import (default: false)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,

    // ============ Provider-specific fields (existing) ============
    /// Provider homepage URL
    #[serde(skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,
    /// API endpoint/base URL
    #[serde(skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
    /// API key
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    /// Optional provider icon name (maps to built-in SVG)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// Optional model name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Optional notes/description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    /// Optional Haiku model (Claude only, v3.7.1+)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub haiku_model: Option<String>,
    /// Optional Sonnet model (Claude only, v3.7.1+)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sonnet_model: Option<String>,
    /// Optional Opus model (Claude only, v3.7.1+)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub opus_model: Option<String>,

    // ============ Prompt-specific fields ============
    /// Base64 encoded Markdown content
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    /// Prompt description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    // ============ MCP-specific fields ============
    /// Target applications for MCP (comma-separated: "claude,codex,gemini")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub apps: Option<String>,

    // ============ Skill-specific fields ============
    /// GitHub repository (format: "owner/name")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repo: Option<String>,
    /// Skill directory name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub directory: Option<String>,
    /// Repository branch (default: "main")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    /// Skills subdirectory path (e.g., "skills")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skills_path: Option<String>,

    // ============ Config file fields (v3.8+) ============
    /// Base64 encoded config content
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config: Option<String>,
    /// Config format (json/toml)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config_format: Option<String>,
    /// Remote config URL
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config_url: Option<String>,
}

/// MCP import result
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpImportResult {
    /// Number of successfully imported MCP servers
    pub imported_count: usize,
    /// IDs of successfully imported MCP servers
    pub imported_ids: Vec<String>,
    /// Failed imports with error messages
    pub failed: Vec<McpImportError>,
}

/// MCP import error
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpImportError {
    /// MCP server ID
    pub id: String,
    /// Error message
    pub error: String,
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
    if app != "claude" && app != "codex" && app != "gemini" {
        return Err(AppError::InvalidInput(format!(
            "Invalid app type: must be 'claude', 'codex', or 'gemini', got '{app}'"
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
    if let Some(ref ep) = endpoint {
        if !ep.is_empty() {
            validate_url(ep, "endpoint")?;
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
        skills_path: None,
        config,
        config_format,
        config_url,
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
    if app != "claude" && app != "codex" && app != "gemini" {
        return Err(AppError::InvalidInput(format!(
            "Invalid app type: must be 'claude', 'codex', or 'gemini', got '{app}'"
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
        skills_path: None,
        config: None,
        config_format: None,
        config_url: None,
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
        if trimmed != "claude" && trimmed != "codex" && trimmed != "gemini" {
            return Err(AppError::InvalidInput(format!(
                "Invalid app in 'apps': must be 'claude', 'codex', or 'gemini', got '{trimmed}'"
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
        skills_path: None,
        config_url: None,
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
    let skills_path = params
        .get("skills_path")
        .or_else(|| params.get("skillsPath"))
        .cloned();

    Ok(DeepLinkImportRequest {
        version,
        resource,
        repo: Some(repo),
        directory,
        branch,
        skills_path,
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
    })
}

/// Validate that a string is a valid HTTP(S) URL
fn validate_url(url_str: &str, field_name: &str) -> Result<(), AppError> {
    let url = Url::parse(url_str)
        .map_err(|e| AppError::InvalidInput(format!("Invalid URL for '{field_name}': {e}")))?;

    let scheme = url.scheme();
    if scheme != "http" && scheme != "https" {
        return Err(AppError::InvalidInput(format!(
            "Invalid URL scheme for '{field_name}': must be http or https, got '{scheme}'"
        )));
    }

    Ok(())
}

/// Import a provider from a deep link request
///
/// This function:
/// 1. Validates the request
/// 2. Merges config file if provided (v3.8+)
/// 3. Converts it to a Provider structure
/// 4. Delegates to ProviderService for actual import
/// 5. Optionally sets as current provider if enabled=true
pub fn import_provider_from_deeplink(
    state: &AppState,
    request: DeepLinkImportRequest,
) -> Result<String, AppError> {
    // Verify this is a provider request
    if request.resource != "provider" {
        return Err(AppError::InvalidInput(format!(
            "Expected provider resource, got '{}'",
            request.resource
        )));
    }

    // Step 1: Merge config file if provided (v3.8+)
    let merged_request = parse_and_merge_config(&request)?;

    // Extract required fields (now as Option)
    let app_str = merged_request
        .app
        .as_ref()
        .ok_or_else(|| AppError::InvalidInput("Missing 'app' field for provider".to_string()))?;

    let api_key = merged_request.api_key.as_ref().ok_or_else(|| {
        AppError::InvalidInput("API key is required (either in URL or config file)".to_string())
    })?;

    if api_key.is_empty() {
        return Err(AppError::InvalidInput(
            "API key cannot be empty".to_string(),
        ));
    }

    let endpoint = merged_request.endpoint.as_ref().ok_or_else(|| {
        AppError::InvalidInput("Endpoint is required (either in URL or config file)".to_string())
    })?;

    if endpoint.is_empty() {
        return Err(AppError::InvalidInput(
            "Endpoint cannot be empty".to_string(),
        ));
    }

    let homepage = merged_request.homepage.as_ref().ok_or_else(|| {
        AppError::InvalidInput("Homepage is required (either in URL or config file)".to_string())
    })?;

    if homepage.is_empty() {
        return Err(AppError::InvalidInput(
            "Homepage cannot be empty".to_string(),
        ));
    }

    let name = merged_request
        .name
        .as_ref()
        .ok_or_else(|| AppError::InvalidInput("Missing 'name' field for provider".to_string()))?;

    // Parse app type
    let app_type = AppType::from_str(app_str)
        .map_err(|_| AppError::InvalidInput(format!("Invalid app type: {app_str}")))?;

    // Build provider configuration based on app type
    let mut provider = build_provider_from_request(&app_type, &merged_request)?;

    // Generate a unique ID for the provider using timestamp + sanitized name
    // This is similar to how frontend generates IDs
    let timestamp = chrono::Utc::now().timestamp_millis();
    let sanitized_name = name
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
        .collect::<String>()
        .to_lowercase();
    provider.id = format!("{sanitized_name}-{timestamp}");

    let provider_id = provider.id.clone();

    // Use ProviderService to add the provider
    ProviderService::add(state, app_type.clone(), provider)?;

    // If enabled=true, set as current provider
    if merged_request.enabled.unwrap_or(false) {
        // Use ProviderService::switch to set as current and sync to live config
        ProviderService::switch(state, app_type.clone(), &provider_id)?;
        log::info!("Provider '{provider_id}' set as current for {app_type:?}");
    }

    Ok(provider_id)
}

/// Build a Provider structure from a deep link request
fn build_provider_from_request(
    app_type: &AppType,
    request: &DeepLinkImportRequest,
) -> Result<Provider, AppError> {
    use serde_json::json;

    let settings_config = match app_type {
        AppType::Claude => {
            // Claude configuration structure
            let mut env = serde_json::Map::new();
            env.insert(
                "ANTHROPIC_AUTH_TOKEN".to_string(),
                json!(request.api_key.clone().unwrap_or_default()),
            );
            env.insert(
                "ANTHROPIC_BASE_URL".to_string(),
                json!(request.endpoint.clone().unwrap_or_default()),
            );

            // Add default model if provided
            if let Some(model) = &request.model {
                env.insert("ANTHROPIC_MODEL".to_string(), json!(model));
            }

            // Add Claude-specific model fields (v3.7.1+)
            if let Some(haiku_model) = &request.haiku_model {
                env.insert(
                    "ANTHROPIC_DEFAULT_HAIKU_MODEL".to_string(),
                    json!(haiku_model),
                );
            }
            if let Some(sonnet_model) = &request.sonnet_model {
                env.insert(
                    "ANTHROPIC_DEFAULT_SONNET_MODEL".to_string(),
                    json!(sonnet_model),
                );
            }
            if let Some(opus_model) = &request.opus_model {
                env.insert(
                    "ANTHROPIC_DEFAULT_OPUS_MODEL".to_string(),
                    json!(opus_model),
                );
            }

            json!({ "env": env })
        }
        AppType::Codex => {
            // Codex configuration structure
            // For Codex, we store auth.json (JSON) and config.toml (TOML string) in settings_config。
            //
            // 这里尽量与前端 `getCodexCustomTemplate` 的默认模板保持一致，
            // 再根据深链接参数注入 base_url / model，避免出现“只有 base_url 行”的极简配置，
            // 让通过 UI 新建和通过深链接导入的 Codex 自定义供应商行为一致。

            // 1. 生成一个适合作为 model_provider 名的安全标识
            //    规则尽量与前端 codexProviderPresets.generateThirdPartyConfig 保持一致：
            //    - 转小写
            //    - 非 [a-z0-9_] 统一替换为下划线
            //    - 去掉首尾下划线
            //    - 若结果为空，则使用 "custom"
            let clean_provider_name = {
                let raw: String = request
                    .name
                    .clone()
                    .unwrap_or_else(|| "custom".to_string())
                    .chars()
                    .filter(|c| !c.is_control())
                    .collect();
                let lower = raw.to_lowercase();
                let mut key: String = lower
                    .chars()
                    .map(|c| match c {
                        'a'..='z' | '0'..='9' | '_' => c,
                        _ => '_',
                    })
                    .collect();

                // 去掉首尾下划线
                while key.starts_with('_') {
                    key.remove(0);
                }
                while key.ends_with('_') {
                    key.pop();
                }

                if key.is_empty() {
                    "custom".to_string()
                } else {
                    key
                }
            };

            // 2. 模型名称：优先使用 deeplink 中的 model，否则退回到 Codex 默认模型
            let model_name = request
                .model
                .as_deref()
                .unwrap_or("gpt-5-codex")
                .to_string();

            // 3. 端点：与 UI 中 Base URL 处理方式保持一致，去掉结尾多余的斜杠
            let endpoint = request
                .endpoint
                .as_deref()
                .unwrap_or("")
                .trim()
                .trim_end_matches('/')
                .to_string();

            // 4. 组装 config.toml 内容
            // 使用 Rust 1.58+ 的内联格式化语法，避免 clippy::uninlined_format_args 警告
            let config_toml = format!(
                r#"model_provider = "{clean_provider_name}"
model = "{model_name}"
model_reasoning_effort = "high"
disable_response_storage = true

[model_providers.{clean_provider_name}]
name = "{clean_provider_name}"
base_url = "{endpoint}"
wire_api = "responses"
requires_openai_auth = true
"#
            );

            json!({
                "auth": {
                    "OPENAI_API_KEY": request.api_key,
                },
                "config": config_toml
            })
        }
        AppType::Gemini => {
            // Gemini configuration structure (.env format)
            let mut env = serde_json::Map::new();
            env.insert("GEMINI_API_KEY".to_string(), json!(request.api_key));
            env.insert(
                "GOOGLE_GEMINI_BASE_URL".to_string(),
                json!(request.endpoint),
            );

            // Add model if provided
            if let Some(model) = &request.model {
                env.insert("GEMINI_MODEL".to_string(), json!(model));
            }

            json!({ "env": env })
        }
    };

    let provider = Provider {
        id: String::new(), // Will be generated by ProviderService
        name: request.name.clone().unwrap_or_default(),
        settings_config,
        website_url: request.homepage.clone(),
        category: None,
        created_at: None,
        sort_index: None,
        notes: request.notes.clone(),
        meta: None,
        icon: request.icon.clone(),
        icon_color: None,
    };

    Ok(provider)
}

/// Parse and merge configuration from Base64 encoded config or remote URL
///
/// Priority: URL params > inline config > remote config
pub fn parse_and_merge_config(
    request: &DeepLinkImportRequest,
) -> Result<DeepLinkImportRequest, AppError> {
    // If no config provided, return original request
    if request.config.is_none() && request.config_url.is_none() {
        return Ok(request.clone());
    }

    // Step 1: Get config content
    let config_content = if let Some(config_b64) = &request.config {
        // Decode Base64 inline config
        let decoded = decode_base64_param("config", config_b64)?;
        String::from_utf8(decoded)
            .map_err(|e| AppError::InvalidInput(format!("Invalid UTF-8 in config: {e}")))?
    } else if let Some(_config_url) = &request.config_url {
        // Fetch remote config (TODO: implement remote fetching in next phase)
        return Err(AppError::InvalidInput(
            "Remote config URL is not yet supported. Use inline config instead.".to_string(),
        ));
    } else {
        return Ok(request.clone());
    };

    // Step 2: Parse config based on format
    let format = request.config_format.as_deref().unwrap_or("json");
    let config_value: serde_json::Value = match format {
        "json" => serde_json::from_str(&config_content)
            .map_err(|e| AppError::InvalidInput(format!("Invalid JSON config: {e}")))?,
        "toml" => {
            let toml_value: toml::Value = toml::from_str(&config_content)
                .map_err(|e| AppError::InvalidInput(format!("Invalid TOML config: {e}")))?;
            // Convert TOML to JSON for uniform processing
            serde_json::to_value(toml_value)
                .map_err(|e| AppError::Message(format!("Failed to convert TOML to JSON: {e}")))?
        }
        _ => {
            return Err(AppError::InvalidInput(format!(
                "Unsupported config format: {format}"
            )))
        }
    };

    // Step 3: Extract values from config based on app type and merge with URL params
    let mut merged = request.clone();

    // MCP, Skill and other resource types don't need config merging (they use config directly)
    // Only provider resource type needs merging
    if request.resource != "provider" {
        return Ok(merged);
    }

    match request.app.as_deref().unwrap_or("") {
        "claude" => merge_claude_config(&mut merged, &config_value)?,
        "codex" => merge_codex_config(&mut merged, &config_value)?,
        "gemini" => merge_gemini_config(&mut merged, &config_value)?,
        "" => {
            // No app specified, skip merging (this is valid for MCP imports)
            return Ok(merged);
        }
        _ => {
            return Err(AppError::InvalidInput(format!(
                "Invalid app type: {:?}",
                request.app
            )))
        }
    }

    Ok(merged)
}

/// Merge Claude configuration from config file
///
/// Priority: URL params override config file values
fn merge_claude_config(
    request: &mut DeepLinkImportRequest,
    config: &serde_json::Value,
) -> Result<(), AppError> {
    let env = config
        .get("env")
        .and_then(|v| v.as_object())
        .ok_or_else(|| {
            AppError::InvalidInput("Claude config must have 'env' object".to_string())
        })?;

    // Auto-fill API key if not provided in URL
    if request.api_key.is_none() || request.api_key.as_ref().unwrap().is_empty() {
        if let Some(token) = env.get("ANTHROPIC_AUTH_TOKEN").and_then(|v| v.as_str()) {
            request.api_key = Some(token.to_string());
        }
    }

    // Auto-fill endpoint if not provided in URL
    if request.endpoint.is_none() || request.endpoint.as_ref().unwrap().is_empty() {
        if let Some(base_url) = env.get("ANTHROPIC_BASE_URL").and_then(|v| v.as_str()) {
            request.endpoint = Some(base_url.to_string());
        }
    }

    // Auto-fill homepage from endpoint if not provided
    if (request.homepage.is_none() || request.homepage.as_ref().unwrap().is_empty())
        && request.endpoint.is_some()
        && !request.endpoint.as_ref().unwrap().is_empty()
    {
        request.homepage = infer_homepage_from_endpoint(request.endpoint.as_ref().unwrap());
        if request.homepage.is_none() {
            request.homepage = Some("https://anthropic.com".to_string());
        }
    }

    // Auto-fill model fields (URL params take priority)
    if request.model.is_none() {
        request.model = env
            .get("ANTHROPIC_MODEL")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
    }
    if request.haiku_model.is_none() {
        request.haiku_model = env
            .get("ANTHROPIC_DEFAULT_HAIKU_MODEL")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
    }
    if request.sonnet_model.is_none() {
        request.sonnet_model = env
            .get("ANTHROPIC_DEFAULT_SONNET_MODEL")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
    }
    if request.opus_model.is_none() {
        request.opus_model = env
            .get("ANTHROPIC_DEFAULT_OPUS_MODEL")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
    }

    Ok(())
}

/// Merge Codex configuration from config file
fn merge_codex_config(
    request: &mut DeepLinkImportRequest,
    config: &serde_json::Value,
) -> Result<(), AppError> {
    // Auto-fill API key from auth.OPENAI_API_KEY
    if request.api_key.is_none() || request.api_key.as_ref().unwrap().is_empty() {
        if let Some(api_key) = config
            .get("auth")
            .and_then(|v| v.get("OPENAI_API_KEY"))
            .and_then(|v| v.as_str())
        {
            request.api_key = Some(api_key.to_string());
        }
    }

    // Auto-fill endpoint and model from config string
    if let Some(config_str) = config.get("config").and_then(|v| v.as_str()) {
        // Parse TOML config string to extract base_url and model
        if let Ok(toml_value) = toml::from_str::<toml::Value>(config_str) {
            // Extract base_url from model_providers section
            if request.endpoint.is_none() || request.endpoint.as_ref().unwrap().is_empty() {
                if let Some(base_url) = extract_codex_base_url(&toml_value) {
                    request.endpoint = Some(base_url);
                }
            }

            // Extract model
            if request.model.is_none() {
                if let Some(model) = toml_value.get("model").and_then(|v| v.as_str()) {
                    request.model = Some(model.to_string());
                }
            }
        }
    }

    // Auto-fill homepage from endpoint
    if (request.homepage.is_none() || request.homepage.as_ref().unwrap().is_empty())
        && request.endpoint.is_some()
        && !request.endpoint.as_ref().unwrap().is_empty()
    {
        request.homepage = infer_homepage_from_endpoint(request.endpoint.as_ref().unwrap());
        if request.homepage.is_none() {
            request.homepage = Some("https://openai.com".to_string());
        }
    }

    Ok(())
}

/// Merge Gemini configuration from config file
fn merge_gemini_config(
    request: &mut DeepLinkImportRequest,
    config: &serde_json::Value,
) -> Result<(), AppError> {
    // Gemini uses flat env structure
    if request.api_key.is_none() || request.api_key.as_ref().unwrap().is_empty() {
        if let Some(api_key) = config.get("GEMINI_API_KEY").and_then(|v| v.as_str()) {
            request.api_key = Some(api_key.to_string());
        }
    }

    if request.endpoint.is_none() || request.endpoint.as_ref().unwrap().is_empty() {
        if let Some(base_url) = config.get("GEMINI_BASE_URL").and_then(|v| v.as_str()) {
            request.endpoint = Some(base_url.to_string());
        }
    }

    if request.model.is_none() {
        request.model = config
            .get("GEMINI_MODEL")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
    }

    // Auto-fill homepage from endpoint
    if (request.homepage.is_none() || request.homepage.as_ref().unwrap().is_empty())
        && request.endpoint.is_some()
        && !request.endpoint.as_ref().unwrap().is_empty()
    {
        request.homepage = infer_homepage_from_endpoint(request.endpoint.as_ref().unwrap());
        if request.homepage.is_none() {
            request.homepage = Some("https://ai.google.dev".to_string());
        }
    }

    Ok(())
}

/// Extract base_url from Codex TOML config
fn extract_codex_base_url(toml_value: &toml::Value) -> Option<String> {
    // Try to find base_url in model_providers section
    if let Some(providers) = toml_value.get("model_providers").and_then(|v| v.as_table()) {
        for (_key, provider) in providers.iter() {
            if let Some(base_url) = provider.get("base_url").and_then(|v| v.as_str()) {
                return Some(base_url.to_string());
            }
        }
    }
    None
}

/// Infer homepage URL from API endpoint
///
/// Examples:
/// - https://api.anthropic.com/v1 → https://anthropic.com
/// - https://api.openai.com/v1 → https://openai.com
/// - https://api-test.company.com/v1 → https://company.com
fn infer_homepage_from_endpoint(endpoint: &str) -> Option<String> {
    let url = Url::parse(endpoint).ok()?;
    let host = url.host_str()?;

    // Remove common API prefixes
    let clean_host = host
        .strip_prefix("api.")
        .or_else(|| host.strip_prefix("api-"))
        .unwrap_or(host);

    Some(format!("https://{clean_host}"))
}

/// 解码 deeplink 里的 Base64 参数，容忍 `+` 被解析为空格、缺少 padding 等常见问题
fn decode_base64_param(field: &str, raw: &str) -> Result<Vec<u8>, AppError> {
    let mut candidates: Vec<String> = Vec::new();
    // 保留空格（用于还原 `+`），但去掉换行符避免复制/粘贴带来的污染
    let trimmed = raw.trim_matches(|c| c == '\r' || c == '\n');

    // 优先尝试将空格还原成 "+"，避免直接解码时被忽略导致内容缺失
    if trimmed.contains(' ') {
        let replaced = trimmed.replace(' ', "+");
        if !replaced.is_empty() && !candidates.contains(&replaced) {
            candidates.push(replaced);
        }
    }

    // 原始值（放在替换版本之后）
    if !trimmed.is_empty() && !candidates.contains(&trimmed.to_string()) {
        candidates.push(trimmed.to_string());
    }

    // 补齐 padding，避免前端去掉结尾 `=`
    let existing = candidates.clone();
    for candidate in existing {
        let mut padded = candidate.clone();
        let remainder = padded.len() % 4;
        if remainder != 0 {
            padded.extend(std::iter::repeat_n('=', 4 - remainder));
        }
        if !candidates.contains(&padded) {
            candidates.push(padded);
        }
    }

    let mut last_error: Option<String> = None;
    for candidate in candidates {
        for engine in [
            &BASE64_STANDARD,
            &BASE64_STANDARD_NO_PAD,
            &BASE64_URL_SAFE,
            &BASE64_URL_SAFE_NO_PAD,
        ] {
            match engine.decode(&candidate) {
                Ok(bytes) => return Ok(bytes),
                Err(err) => last_error = Some(err.to_string()),
            }
        }
    }

    Err(AppError::InvalidInput(format!(
        "{field} 参数 Base64 解码失败：{}。请确认链接参数已用 Base64 编码并经过 URL 转义（尤其是将 '+' 编码为 %2B，或使用 URL-safe Base64）。",
        last_error.unwrap_or_else(|| "未知错误".to_string())
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{store::AppState, Database};
    use std::sync::Arc;

    #[test]
    fn test_parse_valid_claude_deeplink() {
        let url = "ccswitch://v1/import?resource=provider&app=claude&name=Test%20Provider&homepage=https%3A%2F%2Fexample.com&endpoint=https%3A%2F%2Fapi.example.com&apiKey=sk-test-123&icon=claude";

        let request = parse_deeplink_url(url).unwrap();

        assert_eq!(request.version, "v1");
        assert_eq!(request.resource, "provider");
        assert_eq!(request.app, Some("claude".to_string()));
        assert_eq!(request.name, Some("Test Provider".to_string()));
        assert_eq!(request.homepage, Some("https://example.com".to_string()));
        assert_eq!(
            request.endpoint,
            Some("https://api.example.com".to_string())
        );
        assert_eq!(request.api_key, Some("sk-test-123".to_string()));
        assert_eq!(request.icon, Some("claude".to_string()));
    }

    #[test]
    fn test_parse_deeplink_with_notes() {
        let url = "ccswitch://v1/import?resource=provider&app=codex&name=Codex&homepage=https%3A%2F%2Fcodex.com&endpoint=https%3A%2F%2Fapi.codex.com&apiKey=key123&notes=Test%20notes";

        let request = parse_deeplink_url(url).unwrap();

        assert_eq!(request.notes, Some("Test notes".to_string()));
    }

    #[test]
    fn test_parse_invalid_scheme() {
        let url = "https://v1/import?resource=provider&app=claude&name=Test";

        let result = parse_deeplink_url(url);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Invalid scheme"));
    }

    #[test]
    fn test_parse_unsupported_version() {
        let url = "ccswitch://v2/import?resource=provider&app=claude&name=Test";

        let result = parse_deeplink_url(url);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Unsupported protocol version"));
    }

    #[test]
    fn test_parse_missing_required_field() {
        // Name is still required even in v3.8+ (only homepage/endpoint/apiKey are optional)
        let url = "ccswitch://v1/import?resource=provider&app=claude";

        let result = parse_deeplink_url(url);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Missing 'name' parameter"));
    }

    #[test]
    fn test_validate_invalid_url() {
        let result = validate_url("not-a-url", "test");
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_invalid_scheme() {
        let result = validate_url("ftp://example.com", "test");
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("must be http or https"));
    }

    #[test]
    fn test_build_gemini_provider_with_model() {
        let request = DeepLinkImportRequest {
            version: "v1".to_string(),
            resource: "provider".to_string(),
            app: Some("gemini".to_string()),
            name: Some("Test Gemini".to_string()),
            homepage: Some("https://example.com".to_string()),
            endpoint: Some("https://api.example.com".to_string()),
            api_key: Some("test-api-key".to_string()),
            icon: None,
            model: Some("gemini-2.0-flash".to_string()),
            notes: None,
            haiku_model: None,
            sonnet_model: None,
            opus_model: None,
            config: None,
            config_format: None,
            config_url: None,
            apps: None,
            repo: None,
            directory: None,
            branch: None,
            skills_path: None,
            content: None,
            description: None,
            enabled: None,
        };

        let provider = build_provider_from_request(&AppType::Gemini, &request).unwrap();

        // Verify provider basic info
        assert_eq!(provider.name, "Test Gemini");
        assert_eq!(
            provider.website_url,
            Some("https://example.com".to_string())
        );

        // Verify settings_config structure
        let env = provider.settings_config["env"].as_object().unwrap();
        assert_eq!(env["GEMINI_API_KEY"], "test-api-key");
        assert_eq!(env["GOOGLE_GEMINI_BASE_URL"], "https://api.example.com");
        assert_eq!(env["GEMINI_MODEL"], "gemini-2.0-flash");
    }

    #[test]
    fn test_build_gemini_provider_without_model() {
        let request = DeepLinkImportRequest {
            version: "v1".to_string(),
            resource: "provider".to_string(),
            app: Some("gemini".to_string()),
            name: Some("Test Gemini".to_string()),
            homepage: Some("https://example.com".to_string()),
            endpoint: Some("https://api.example.com".to_string()),
            api_key: Some("test-api-key".to_string()),
            icon: None,
            model: None,
            notes: None,
            haiku_model: None,
            sonnet_model: None,
            opus_model: None,
            config: None,
            config_format: None,
            config_url: None,
            apps: None,
            repo: None,
            directory: None,
            branch: None,
            skills_path: None,
            content: None,
            description: None,
            enabled: None,
        };

        let provider = build_provider_from_request(&AppType::Gemini, &request).unwrap();

        // Verify settings_config structure
        let env = provider.settings_config["env"].as_object().unwrap();
        assert_eq!(env["GEMINI_API_KEY"], "test-api-key");
        assert_eq!(env["GOOGLE_GEMINI_BASE_URL"], "https://api.example.com");
        // Model should not be present
        assert!(env.get("GEMINI_MODEL").is_none());
    }

    #[test]
    fn test_infer_homepage() {
        assert_eq!(
            infer_homepage_from_endpoint("https://api.anthropic.com/v1"),
            Some("https://anthropic.com".to_string())
        );
        assert_eq!(
            infer_homepage_from_endpoint("https://api-test.company.com/v1"),
            Some("https://test.company.com".to_string())
        );
        assert_eq!(
            infer_homepage_from_endpoint("https://example.com"),
            Some("https://example.com".to_string())
        );
    }

    #[test]
    fn test_parse_and_merge_config_claude() {
        use base64::prelude::*;

        // Prepare Base64 encoded Claude config
        let config_json = r#"{"env":{"ANTHROPIC_AUTH_TOKEN":"sk-ant-xxx","ANTHROPIC_BASE_URL":"https://api.anthropic.com/v1","ANTHROPIC_MODEL":"claude-sonnet-4.5"}}"#;
        let config_b64 = BASE64_STANDARD.encode(config_json.as_bytes());

        let request = DeepLinkImportRequest {
            version: "v1".to_string(),
            resource: "provider".to_string(),
            app: Some("claude".to_string()),
            name: Some("Test".to_string()),
            homepage: None,
            endpoint: None,
            api_key: None,
            icon: None,
            model: None,
            notes: None,
            haiku_model: None,
            sonnet_model: None,
            opus_model: None,
            config: Some(config_b64),
            config_format: Some("json".to_string()),
            config_url: None,
            apps: None,
            repo: None,
            directory: None,
            branch: None,
            skills_path: None,
            content: None,
            description: None,
            enabled: None,
        };

        let merged = parse_and_merge_config(&request).unwrap();

        // Should auto-fill from config
        assert_eq!(merged.api_key, Some("sk-ant-xxx".to_string()));
        assert_eq!(
            merged.endpoint,
            Some("https://api.anthropic.com/v1".to_string())
        );
        assert_eq!(merged.homepage, Some("https://anthropic.com".to_string()));
        assert_eq!(merged.model, Some("claude-sonnet-4.5".to_string()));
    }

    #[test]
    fn test_parse_and_merge_config_url_override() {
        use base64::prelude::*;

        let config_json = r#"{"env":{"ANTHROPIC_AUTH_TOKEN":"sk-old","ANTHROPIC_BASE_URL":"https://api.anthropic.com/v1"}}"#;
        let config_b64 = BASE64_STANDARD.encode(config_json.as_bytes());

        let request = DeepLinkImportRequest {
            version: "v1".to_string(),
            resource: "provider".to_string(),
            app: Some("claude".to_string()),
            name: Some("Test".to_string()),
            homepage: None,
            endpoint: None,
            api_key: Some("sk-new".to_string()), // URL param should override
            icon: None,
            model: None,
            notes: None,
            haiku_model: None,
            sonnet_model: None,
            opus_model: None,
            config: Some(config_b64),
            config_format: Some("json".to_string()),
            config_url: None,
            apps: None,
            repo: None,
            directory: None,
            branch: None,
            skills_path: None,
            content: None,
            description: None,
            enabled: None,
        };

        let merged = parse_and_merge_config(&request).unwrap();

        // URL param should take priority
        assert_eq!(merged.api_key, Some("sk-new".to_string()));
        // Config file value should be used
        assert_eq!(
            merged.endpoint,
            Some("https://api.anthropic.com/v1".to_string())
        );
    }

    #[test]
    fn test_import_prompt_allows_space_in_base64_content() {
        let url = "ccswitch://v1/import?resource=prompt&app=codex&name=PromptPlus&content=Pj4+";
        let request = parse_deeplink_url(url).unwrap();

        // URL 解码后 content 中的 "+" 会变成空格，确保解码逻辑可以恢复
        assert_eq!(request.content.as_deref(), Some("Pj4 "));

        let db = Arc::new(Database::memory().expect("create memory db"));
        let state = AppState::new(db.clone());

        let prompt_id =
            import_prompt_from_deeplink(&state, request.clone()).expect("import prompt");

        let prompts = state.db.get_prompts("codex").expect("get prompts");
        let prompt = prompts.get(&prompt_id).expect("prompt saved");

        assert_eq!(prompt.content, ">>>");
        assert_eq!(prompt.name, request.name.unwrap());
    }
}

// ============================================
// MCP Server Import Implementation
// ============================================

/// Import MCP servers from deep link request
///
/// This function handles batch import of MCP servers from standard MCP JSON format
pub fn import_mcp_from_deeplink(
    state: &AppState,
    request: DeepLinkImportRequest,
) -> Result<McpImportResult, AppError> {
    // Verify this is an MCP request
    if request.resource != "mcp" {
        return Err(AppError::InvalidInput(format!(
            "Expected mcp resource, got '{}'",
            request.resource
        )));
    }

    // Extract and validate apps parameter
    let apps_str = request
        .apps
        .as_ref()
        .ok_or_else(|| AppError::InvalidInput("Missing 'apps' parameter for MCP".to_string()))?;

    // Parse apps into McpApps struct
    let target_apps = parse_mcp_apps(apps_str)?;

    // Extract config
    let config_b64 = request
        .config
        .as_ref()
        .ok_or_else(|| AppError::InvalidInput("Missing 'config' parameter for MCP".to_string()))?;

    // Decode Base64 config
    let decoded = decode_base64_param("config", config_b64)?;

    let config_str = String::from_utf8(decoded)
        .map_err(|e| AppError::InvalidInput(format!("Invalid UTF-8 in config: {e}")))?;

    // Parse JSON
    let config_json: Value = serde_json::from_str(&config_str)
        .map_err(|e| AppError::InvalidInput(format!("Invalid JSON in MCP config: {e}")))?;

    // Extract mcpServers object
    let mcp_servers = config_json
        .get("mcpServers")
        .and_then(|v| v.as_object())
        .ok_or_else(|| {
            AppError::InvalidInput("MCP config must contain 'mcpServers' object".to_string())
        })?;

    if mcp_servers.is_empty() {
        return Err(AppError::InvalidInput(
            "No MCP servers found in config".to_string(),
        ));
    }

    // Get existing servers to check for duplicates
    let existing_servers = state.db.get_all_mcp_servers()?;

    // Import each MCP server
    let mut imported_ids = Vec::new();
    let mut failed = Vec::new();

    use crate::services::McpService;

    for (id, server_spec) in mcp_servers.iter() {
        // Check if server already exists
        let server = if let Some(existing) = existing_servers.get(id) {
            // Server exists - merge apps only, keep other fields unchanged
            log::info!("MCP server '{id}' already exists, merging apps only");

            let mut merged_apps = existing.apps.clone();
            // Merge new apps into existing apps
            if target_apps.claude {
                merged_apps.claude = true;
            }
            if target_apps.codex {
                merged_apps.codex = true;
            }
            if target_apps.gemini {
                merged_apps.gemini = true;
            }

            McpServer {
                id: existing.id.clone(),
                name: existing.name.clone(),
                server: existing.server.clone(), // Keep existing server config
                apps: merged_apps,               // Merged apps
                description: existing.description.clone(),
                homepage: existing.homepage.clone(),
                docs: existing.docs.clone(),
                tags: existing.tags.clone(),
            }
        } else {
            // New server - create with provided config
            log::info!("Creating new MCP server: {id}");
            McpServer {
                id: id.clone(),
                name: id.clone(),
                server: server_spec.clone(),
                apps: target_apps.clone(),
                description: None,
                homepage: None,
                docs: None,
                tags: vec!["imported".to_string()],
            }
        };

        match McpService::upsert_server(state, server) {
            Ok(_) => {
                imported_ids.push(id.clone());
                log::info!("Successfully imported/updated MCP server: {id}");
            }
            Err(e) => {
                failed.push(McpImportError {
                    id: id.clone(),
                    error: format!("{e}"),
                });
                log::warn!("Failed to import MCP server '{id}': {e}");
            }
        }
    }

    Ok(McpImportResult {
        imported_count: imported_ids.len(),
        imported_ids,
        failed,
    })
}

/// Parse apps string into McpApps struct
fn parse_mcp_apps(apps_str: &str) -> Result<McpApps, AppError> {
    let mut apps = McpApps {
        claude: false,
        codex: false,
        gemini: false,
    };

    for app in apps_str.split(',') {
        match app.trim() {
            "claude" => apps.claude = true,
            "codex" => apps.codex = true,
            "gemini" => apps.gemini = true,
            other => {
                return Err(AppError::InvalidInput(format!(
                    "Invalid app in 'apps': {other}"
                )))
            }
        }
    }

    if apps.is_empty() {
        return Err(AppError::InvalidInput(
            "At least one app must be specified in 'apps'".to_string(),
        ));
    }

    Ok(apps)
}

// ============================================
// Prompt Import Implementation
// ============================================

/// Import a prompt from deep link request
pub fn import_prompt_from_deeplink(
    state: &AppState,
    request: DeepLinkImportRequest,
) -> Result<String, AppError> {
    // Verify this is a prompt request
    if request.resource != "prompt" {
        return Err(AppError::InvalidInput(format!(
            "Expected prompt resource, got '{}'",
            request.resource
        )));
    }

    // Extract required fields
    let app_str = request
        .app
        .as_ref()
        .ok_or_else(|| AppError::InvalidInput("Missing 'app' field for prompt".to_string()))?;

    let name = request
        .name
        .ok_or_else(|| AppError::InvalidInput("Missing 'name' field for prompt".to_string()))?;

    // Parse app type
    let app_type = AppType::from_str(app_str)
        .map_err(|_| AppError::InvalidInput(format!("Invalid app type: {app_str}")))?;

    // Decode content
    let content_b64 = request
        .content
        .as_ref()
        .ok_or_else(|| AppError::InvalidInput("Missing 'content' field for prompt".to_string()))?;

    let content = decode_base64_param("content", content_b64)?;
    let content = String::from_utf8(content)
        .map_err(|e| AppError::InvalidInput(format!("Invalid UTF-8 in content: {e}")))?;

    // Generate ID
    let timestamp = chrono::Utc::now().timestamp_millis();
    let sanitized_name = name
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
        .collect::<String>()
        .to_lowercase();
    let id = format!("{sanitized_name}-{timestamp}");

    // Check if we should enable this prompt
    let should_enable = request.enabled.unwrap_or(false);

    // Create Prompt (initially disabled)
    let prompt = Prompt {
        id: id.clone(),
        name: name.clone(),
        content,
        description: request.description,
        enabled: false, // Always start as disabled, will be enabled later if needed
        created_at: Some(timestamp),
        updated_at: Some(timestamp),
    };

    // Save using PromptService
    use crate::services::PromptService;
    PromptService::upsert_prompt(state, app_type.clone(), &id, prompt)?;

    // If enabled flag is set, enable this prompt (which will disable others)
    if should_enable {
        PromptService::enable_prompt(state, app_type, &id)?;
        log::info!("Successfully imported and enabled prompt '{name}' for {app_str}");
    } else {
        log::info!("Successfully imported prompt '{name}' for {app_str} (disabled)");
    }

    Ok(id)
}

// ============================================
// Skill Import Implementation
// ============================================

/// Import a skill from deep link request
pub fn import_skill_from_deeplink(
    state: &AppState,
    request: DeepLinkImportRequest,
) -> Result<String, AppError> {
    // Verify this is a skill request
    if request.resource != "skill" {
        return Err(AppError::InvalidInput(format!(
            "Expected skill resource, got '{}'",
            request.resource
        )));
    }

    // Parse repo
    let repo_str = request
        .repo
        .ok_or_else(|| AppError::InvalidInput("Missing 'repo' field for skill".to_string()))?;

    let parts: Vec<&str> = repo_str.split('/').collect();
    if parts.len() != 2 {
        return Err(AppError::InvalidInput(format!(
            "Invalid repo format: expected 'owner/name', got '{repo_str}'"
        )));
    }
    let owner = parts[0].to_string();
    let name = parts[1].to_string();

    // Create SkillRepo
    let repo = SkillRepo {
        owner: owner.clone(),
        name: name.clone(),
        branch: request.branch.unwrap_or_else(|| "main".to_string()),
        enabled: request.enabled.unwrap_or(true),
        skills_path: request.skills_path,
    };

    // Save using Database
    state.db.save_skill_repo(&repo)?;

    log::info!("Successfully added skill repo '{owner}/{name}'");

    Ok(format!("{owner}/{name}"))
}

#[cfg(test)]
mod tests_imports {
    use super::*;
    use base64::Engine;

    #[test]
    fn test_parse_mcp_apps() {
        let apps = parse_mcp_apps("claude,codex").unwrap();
        assert!(apps.claude);
        assert!(apps.codex);
        assert!(!apps.gemini);

        let apps = parse_mcp_apps("gemini").unwrap();
        assert!(!apps.claude);
        assert!(!apps.codex);
        assert!(apps.gemini);

        let err = parse_mcp_apps("invalid").unwrap_err();
        assert!(err.to_string().contains("Invalid app"));
    }

    #[test]
    fn test_parse_prompt_deeplink() {
        let content = "Hello World";
        let content_b64 = BASE64_STANDARD.encode(content);
        let url = format!(
            "ccswitch://v1/import?resource=prompt&app=claude&name=test&content={}&description=desc&enabled=true",
            content_b64
        );

        let request = parse_deeplink_url(&url).unwrap();
        assert_eq!(request.resource, "prompt");
        assert_eq!(request.app.unwrap(), "claude");
        assert_eq!(request.name.unwrap(), "test");
        assert_eq!(request.content.unwrap(), content_b64);
        assert_eq!(request.description.unwrap(), "desc");
        assert_eq!(request.enabled.unwrap(), true);
    }

    #[test]
    fn test_parse_mcp_deeplink() {
        let config = r#"{"mcpServers":{"test":{"command":"echo"}}}"#;
        let config_b64 = BASE64_STANDARD.encode(config);
        let url = format!(
            "ccswitch://v1/import?resource=mcp&apps=claude,codex&config={}&enabled=true",
            config_b64
        );

        let request = parse_deeplink_url(&url).unwrap();
        assert_eq!(request.resource, "mcp");
        assert_eq!(request.apps.unwrap(), "claude,codex");
        assert_eq!(request.config.unwrap(), config_b64);
        assert_eq!(request.enabled.unwrap(), true);
    }

    #[test]
    fn test_parse_skill_deeplink() {
        let url = "ccswitch://v1/import?resource=skill&repo=owner/repo&directory=skills&branch=dev&skills_path=src";
        let request = parse_deeplink_url(&url).unwrap();

        assert_eq!(request.resource, "skill");
        assert_eq!(request.repo.unwrap(), "owner/repo");
        assert_eq!(request.directory.unwrap(), "skills");
        assert_eq!(request.branch.unwrap(), "dev");
        assert_eq!(request.skills_path.unwrap(), "src");
    }
}
