//! Provider import from deep link
//!
//! Handles importing provider configurations via ccswitch:// URLs.

use super::utils::{decode_base64_param, infer_homepage_from_endpoint};
use super::DeepLinkImportRequest;
use crate::error::AppError;
use crate::provider::{ClaudeDesktopMode, Provider, ProviderMeta, UsageScript};
use crate::services::ProviderService;
use crate::store::AppState;
use crate::AppType;
use serde_json::json;
use std::str::FromStr;

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
    let mut merged_request = parse_and_merge_config(&request)?;

    // Extract required fields (now as Option)
    let app_str = merged_request
        .app
        .clone()
        .ok_or_else(|| AppError::InvalidInput("Missing 'app' field for provider".to_string()))?;

    let api_key = merged_request.api_key.as_ref().ok_or_else(|| {
        AppError::InvalidInput("API key is required (either in URL or config file)".to_string())
    })?;

    if api_key.is_empty() {
        return Err(AppError::InvalidInput(
            "API key cannot be empty".to_string(),
        ));
    }

    // Get endpoint: supports comma-separated multiple URLs (first is primary)
    let endpoint_str = merged_request.endpoint.as_ref().ok_or_else(|| {
        AppError::InvalidInput("Endpoint is required (either in URL or config file)".to_string())
    })?;

    // Parse endpoints: split by comma, first is primary
    let all_endpoints: Vec<String> = endpoint_str
        .split(',')
        .map(|e| e.trim().to_string())
        .filter(|e| !e.is_empty())
        .collect();

    let primary_endpoint = all_endpoints
        .first()
        .ok_or_else(|| AppError::InvalidInput("Endpoint cannot be empty".to_string()))?;

    // Auto-infer homepage from endpoint if not provided
    if merged_request
        .homepage
        .as_ref()
        .is_none_or(|s| s.is_empty())
    {
        merged_request.homepage = infer_homepage_from_endpoint(primary_endpoint);
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
        .clone()
        .ok_or_else(|| AppError::InvalidInput("Missing 'name' field for provider".to_string()))?;

    // Parse app type
    let app_type = AppType::from_str(&app_str)
        .map_err(|_| AppError::InvalidInput(format!("Invalid app type: {app_str}")))?;

    // Build provider configuration based on app type
    let mut provider = build_provider_from_request(&app_type, &merged_request)?;

    // Generate a unique ID for the provider using timestamp + sanitized name
    let timestamp = chrono::Utc::now().timestamp_millis();
    let sanitized_name = name
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
        .collect::<String>()
        .to_lowercase();
    provider.id = format!("{sanitized_name}-{timestamp}");

    let provider_id = provider.id.clone();

    // Use ProviderService to add the provider
    ProviderService::add(state, app_type.clone(), provider, true)?;

    // Add extra endpoints as custom endpoints (skip first one as it's the primary)
    for ep in all_endpoints.iter().skip(1) {
        let normalized = ep.trim().trim_end_matches('/').to_string();
        if !normalized.is_empty() {
            if let Err(e) = ProviderService::add_custom_endpoint(
                state,
                app_type.clone(),
                &provider_id,
                normalized.clone(),
            ) {
                log::warn!(
                    "Failed to add custom endpoint '{}': {e}",
                    crate::url_for_log(&normalized)
                );
            }
        }
    }

    // If enabled=true, set as current provider
    if merged_request.enabled.unwrap_or(false) {
        ProviderService::switch(state, app_type.clone(), &provider_id)?;
        log::info!("Provider '{provider_id}' set as current for {app_type:?}");
    }

    Ok(provider_id)
}

/// Build a Provider structure from a deep link request
pub(crate) fn build_provider_from_request(
    app_type: &AppType,
    request: &DeepLinkImportRequest,
) -> Result<Provider, AppError> {
    let settings_config = match app_type {
        AppType::Claude | AppType::ClaudeDesktop => build_claude_settings(request),
        AppType::Codex => build_codex_settings(request)?,
        AppType::Gemini => build_gemini_settings(request),
        AppType::GrokBuild => build_grokbuild_settings(request),
        AppType::OpenCode => build_opencode_settings(request),
        AppType::OpenClaw => build_additive_app_settings(request),
        AppType::Hermes => build_hermes_settings(request),
        AppType::Mcode => {
            return Err(AppError::InvalidInput(
                "Add MCode providers from the MCode page".into(),
            ))
        }
        AppType::Pi => {
            return Err(AppError::InvalidInput(
                "Pi providers must be added from the Pi provider page".to_string(),
            ));
        }
    };

    // Build usage script configuration if provided
    let mut meta = build_provider_meta(request)?;
    if matches!(app_type, AppType::ClaudeDesktop) {
        meta.get_or_insert_with(ProviderMeta::default)
            .claude_desktop_mode = Some(ClaudeDesktopMode::Direct);
    }

    let provider = Provider {
        id: String::new(), // Will be generated by caller
        name: request.name.clone().unwrap_or_default(),
        settings_config,
        website_url: request.homepage.clone(),
        category: None,
        created_at: None,
        sort_index: None,
        notes: request.notes.clone(),
        meta,
        icon: request.icon.clone(),
        icon_color: None,
        in_failover_queue: false,
    };

    Ok(provider)
}

/// Get primary endpoint from request (first one if comma-separated)
fn get_primary_endpoint(request: &DeepLinkImportRequest) -> String {
    request
        .endpoint
        .as_ref()
        .and_then(|ep| ep.split(',').next())
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

fn normalize_deeplink_api_key(api_key: &str) -> String {
    api_key.trim().to_string()
}

fn normalize_deeplink_base_url(base_url: &str) -> String {
    base_url.trim().trim_end_matches('/').to_string()
}

fn usage_api_key_override(request: &DeepLinkImportRequest) -> Option<String> {
    let usage_api_key = normalize_deeplink_api_key(request.usage_api_key.as_deref()?);
    if usage_api_key.is_empty() {
        return None;
    }

    let provider_api_key = request
        .api_key
        .as_deref()
        .map(normalize_deeplink_api_key)
        .unwrap_or_default();

    if !provider_api_key.is_empty() && usage_api_key == provider_api_key {
        None
    } else {
        Some(usage_api_key)
    }
}

fn usage_base_url_override(request: &DeepLinkImportRequest) -> Option<String> {
    let usage_base_url = normalize_deeplink_base_url(request.usage_base_url.as_deref()?);
    if usage_base_url.is_empty() {
        return None;
    }

    let provider_base_url = normalize_deeplink_base_url(&get_primary_endpoint(request));

    if !provider_base_url.is_empty() && usage_base_url == provider_base_url {
        None
    } else {
        Some(usage_base_url)
    }
}

/// Build provider meta with usage script configuration
fn build_provider_meta(request: &DeepLinkImportRequest) -> Result<Option<ProviderMeta>, AppError> {
    // Check if any usage script fields are provided
    if request.usage_script.is_none()
        && request.usage_enabled.is_none()
        && request.usage_api_key.is_none()
        && request.usage_base_url.is_none()
        && request.usage_access_token.is_none()
        && request.usage_user_id.is_none()
        && request.usage_auto_interval.is_none()
    {
        return Ok(None);
    }

    // Decode usage script code if provided
    let code = if let Some(script_b64) = &request.usage_script {
        let decoded = decode_base64_param("usage_script", script_b64)?;
        String::from_utf8(decoded)
            .map_err(|e| AppError::InvalidInput(format!("Invalid UTF-8 in usage_script: {e}")))?
    } else {
        String::new()
    };

    // Determine enabled state: explicit param only, defaulting to disabled.
    //
    // 「携带了代码」不构成用户的启用决定。此处的输入来自 deeplink——即第三方
    // 构造、经浏览器抵达的不可信载荷——而 `code` 是一段会在查询用量时执行的
    // JavaScript。若以 `!code.is_empty()` 作默认，一条链接就能让脚本在用户
    // 从未勾选过的情况下进入启用态。
    //
    // 要启用，链接必须显式携带 `usageEnabled=true`。注意该参数是**链接作者**的
    // 请求，不构成用户的同意；用户的同意体现在确认框展示了完整脚本正文与启用
    // 徽章之后仍点了导入——所以那两处展示是本设计的承重部分，不可省略。
    // 用户在应用内手动配置的脚本不走这条路径。
    let enabled = request.usage_enabled.unwrap_or(false);

    let usage_script = UsageScript {
        enabled,
        language: "javascript".to_string(),
        code,
        timeout: Some(10),
        api_key: usage_api_key_override(request),
        base_url: usage_base_url_override(request),
        access_token: request.usage_access_token.clone(),
        user_id: request.usage_user_id.clone(),
        template_type: None, // Deeplink providers don't specify template type (will use backward compatibility logic)
        auto_query_interval: request.usage_auto_interval,
        coding_plan_provider: None,
        access_key_id: None,
        secret_access_key: None,
        team_organization_id: None,
        team_project_id: None,
    };

    Ok(Some(ProviderMeta {
        usage_script: Some(usage_script),
        ..Default::default()
    }))
}

/// Build Claude settings configuration
///
/// Merges env from the inline config (if any) with the standard fields from URL params.
/// URL params take priority — they overwrite same-named fields from the config.
/// Non-standard env fields (e.g. `ANTHROPIC_CUSTOM_HEADERS`, `API_TIMEOUT_MS`,
/// `CLAUDE_CODE_DISABLE_EXPERIMENTAL_BETAS`, ...) are preserved as-is so that
/// providers requiring extra environment variables work after deeplink import.
fn build_claude_settings(request: &DeepLinkImportRequest) -> serde_json::Value {
    // Start from the full env block in the inline config (if present), so any
    // custom env vars the user passed via `config=<base64-json>` survive the
    // import. Falling back to an empty map keeps the previous behavior for
    // deeplinks that don't carry a config field.
    let mut env = extract_claude_config_env(request).unwrap_or_default();

    // Now overwrite / fill in the standard fields from URL params. URL params
    // are authoritative because they're what the deeplink builder put on the
    // wire — for Claude these are: ANTHROPIC_AUTH_TOKEN, ANTHROPIC_BASE_URL,
    // ANTHROPIC_MODEL, and the haiku/sonnet/opus model aliases.
    env.insert(
        "ANTHROPIC_AUTH_TOKEN".to_string(),
        json!(request.api_key.clone().unwrap_or_default()),
    );
    env.insert(
        "ANTHROPIC_BASE_URL".to_string(),
        json!(get_primary_endpoint(request)),
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

/// Decode and extract the `env` object from the deeplink's inline config payload.
///
/// Returns `None` when no config is attached, when the payload can't be
/// decoded/parsed, or when it doesn't contain a Claude-style `env` object.
/// This is a best-effort accessor — we deliberately don't surface parse
/// errors here because `parse_and_merge_config` will have already validated
/// the payload during the merge phase; any failure at this point just means
/// "fall back to URL-param-only behavior".
fn extract_claude_config_env(
    request: &DeepLinkImportRequest,
) -> Option<serde_json::Map<String, serde_json::Value>> {
    // Only the inline base64 config carries an env block. Remote config_url
    // is not implemented yet (see parse_and_merge_config), so nothing else to
    // try here.
    let config_b64 = request.config.as_ref()?;

    // Honor the declared format; default to JSON like parse_and_merge_config does.
    let format = request.config_format.as_deref().unwrap_or("json");
    if format != "json" {
        // Claude config is always JSON in practice. TOML/other formats aren't
        // expected on this app path, so don't try to handle them — safer to
        // fall back than to risk silently producing the wrong shape.
        return None;
    }

    // Decode the base64 payload. We re-decode here rather than threading the
    // already-decoded value through every build_* function, because the call
    // graph (parse_and_merge_config is pub and called separately for preview)
    // makes signature changes invasive. Decode cost is negligible on this
    // one-shot import path.
    let decoded = decode_base64_param("config", config_b64).ok()?;
    let json_str = std::str::from_utf8(&decoded).ok()?;
    let value: serde_json::Value = serde_json::from_str(json_str).ok()?;

    // Pull out the env object — same shape as Claude's own settings.json.
    value.get("env").and_then(|v| v.as_object()).cloned()
}

/// Build Codex settings configuration
fn build_codex_settings(request: &DeepLinkImportRequest) -> Result<serde_json::Value, AppError> {
    let provider_display_name = request
        .name
        .as_deref()
        .unwrap_or("custom")
        .chars()
        .filter(|c| !c.is_control())
        .collect::<String>()
        .trim()
        .to_string();
    let provider_display_name = if provider_display_name.is_empty() {
        "custom".to_string()
    } else {
        provider_display_name
    };

    // Model name: use deeplink model or default
    let model_name = request
        .model
        .as_deref()
        .unwrap_or("gpt-5-codex")
        .to_string();

    // Endpoint: normalize trailing slashes (use primary endpoint only)
    let endpoint = get_primary_endpoint(request)
        .trim()
        .trim_end_matches('/')
        .to_string();

    let provider_display_name = toml_edit::Value::from(provider_display_name.as_str()).to_string();
    let model_name = toml_edit::Value::from(model_name.as_str()).to_string();
    let catalog = extract_codex_model_catalog(request, &endpoint)?;
    let endpoint = toml_edit::Value::from(endpoint.as_str()).to_string();

    // Build config.toml content
    let mut config_toml = format!(
        r#"model_provider = "custom"
model = {model_name}
model_reasoning_effort = "high"
disable_response_storage = true

[model_providers.custom]
name = {provider_display_name}
base_url = {endpoint}
wire_api = "responses"
requires_openai_auth = true
"#
    );

    if let Some(url) = catalog {
        config_toml.push_str(&format!(
            "model_catalog_url = {}\n",
            toml_edit::Value::from(url.as_str()),
        ));
    }

    Ok(json!({
        "auth": {
            "OPENAI_API_KEY": request.api_key,
        },
        "config": config_toml
    }))
}

/// Preserve the active provider's explicit remote catalog without copying other
/// inline auth/provider tables. Codex sends provider credentials to this URL, so
/// compare it with the final (URL-parameter-authoritative) inference origin.
fn extract_codex_model_catalog(
    request: &DeepLinkImportRequest,
    endpoint: &str,
) -> Result<Option<String>, AppError> {
    let Some(config_b64) = &request.config else {
        return Ok(None);
    };
    let decoded = decode_base64_param("config", config_b64)?;
    let text = std::str::from_utf8(&decoded)
        .map_err(|_| AppError::InvalidInput("Invalid UTF-8 in config".to_string()))?;
    let config: toml::Value = match request.config_format.as_deref().unwrap_or("json") {
        "json" => {
            let value: serde_json::Value = serde_json::from_str(text)
                .map_err(|_| AppError::InvalidInput("Invalid JSON config".to_string()))?;
            let Some(config_text) = value.get("config").and_then(|v| v.as_str()) else {
                return Ok(None);
            };
            toml::from_str(config_text)
        }
        "toml" => toml::from_str(text),
        _ => {
            return Err(AppError::InvalidInput(
                "Unsupported config format".to_string(),
            ))
        }
    }
    .map_err(|_| AppError::InvalidInput("Invalid Codex TOML config".to_string()))?;
    let Some(value) =
        active_codex_provider(&config).and_then(|provider| provider.get("model_catalog_url"))
    else {
        return Ok(None);
    };
    let invalid_url = || {
        AppError::InvalidInput(
            "Codex model_catalog_url must be an absolute HTTP(S) URL on the provider endpoint's origin, without userinfo or a fragment".to_string(),
        )
    };
    let catalog_url = value.as_str().ok_or_else(invalid_url)?;
    let url = url::Url::parse(catalog_url).map_err(|_| invalid_url())?;
    let endpoint_url = url::Url::parse(endpoint).map_err(|_| invalid_url())?;
    if !matches!(url.scheme(), "https" | "http")
        || catalog_url.chars().any(char::is_control)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || url.origin() != endpoint_url.origin()
    {
        return Err(invalid_url());
    }
    Ok(Some(catalog_url.to_string()))
}

/// Build Gemini settings configuration
fn build_gemini_settings(request: &DeepLinkImportRequest) -> serde_json::Value {
    let mut env = serde_json::Map::new();
    env.insert("GEMINI_API_KEY".to_string(), json!(request.api_key));
    env.insert(
        "GOOGLE_GEMINI_BASE_URL".to_string(),
        json!(get_primary_endpoint(request)),
    );

    // Add model if provided
    if let Some(model) = &request.model {
        env.insert("GEMINI_MODEL".to_string(), json!(model));
    }

    json!({ "env": env })
}

fn build_grokbuild_settings(request: &DeepLinkImportRequest) -> serde_json::Value {
    let model = request
        .model
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(crate::grok_config::DEFAULT_MODEL)
        .trim();
    let name = request
        .name
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("custom")
        .trim();
    let endpoint = get_primary_endpoint(request).trim().to_string();
    let api_key = request.api_key.as_deref().unwrap_or("").trim();

    let model_value = toml_edit::Value::from(model).to_string();
    let name_value = toml_edit::Value::from(name).to_string();
    let endpoint_value = toml_edit::Value::from(endpoint.as_str()).to_string();
    let api_key_value = toml_edit::Value::from(api_key).to_string();

    json!({
        "config": format!(
            "[models]\ndefault = {model_value}\n\n[model.{model_value}]\nmodel = {model_value}\nbase_url = {endpoint_value}\nname = {name_value}\napi_key = {api_key_value}\napi_backend = \"{}\"\ncontext_window = {}\n",
            crate::grok_config::DEFAULT_API_BACKEND,
            crate::grok_config::DEFAULT_CONTEXT_WINDOW,
        )
    })
}

/// Build OpenCode settings configuration
fn build_opencode_settings(request: &DeepLinkImportRequest) -> serde_json::Value {
    let endpoint = get_primary_endpoint(request);

    // Build options object
    let mut options = serde_json::Map::new();
    if !endpoint.is_empty() {
        options.insert("baseURL".to_string(), json!(endpoint));
    }
    if let Some(api_key) = &request.api_key {
        options.insert("apiKey".to_string(), json!(api_key));
    }

    // Build models object
    let mut models = serde_json::Map::new();
    if let Some(model) = &request.model {
        models.insert(model.clone(), json!({ "name": model }));
    }

    // Default to openai-compatible npm package
    json!({
        "npm": "@ai-sdk/openai-compatible",
        "options": options,
        "models": models
    })
}

/// Build settings for OpenClaw (camelCase live config).
/// Format: { baseUrl, apiKey, api, models }
fn build_additive_app_settings(request: &DeepLinkImportRequest) -> serde_json::Value {
    let endpoint = get_primary_endpoint(request);

    let mut config = serde_json::Map::new();

    if !endpoint.is_empty() {
        config.insert("baseUrl".to_string(), json!(endpoint));
    }

    if let Some(api_key) = &request.api_key {
        config.insert("apiKey".to_string(), json!(api_key));
    }

    config.insert("api".to_string(), json!("openai-completions"));

    if let Some(model) = &request.model {
        config.insert(
            "models".to_string(),
            json!([{ "id": model, "name": model }]),
        );
    }

    json!(config)
}

/// Build Hermes provider settings (snake_case YAML-native fields).
///
/// Hermes' `custom_providers:` entries use `base_url` / `api_key` / `api_mode`
/// (see `_VALID_CUSTOM_PROVIDER_FIELDS` in upstream `hermes_cli/config.py`).
/// Emitting camelCase here — as the OpenClaw path does — would poison the
/// YAML with unknown root fields the Hermes runtime ignores.
///
/// `api_mode` is always written explicitly. Deeplinks have no field to carry
/// it, so we default to `chat_completions` (the most widely compatible
/// protocol) and let the user adjust via the UI after import. We never rely
/// on Hermes' built-in URL heuristics, which only recognize a handful of
/// official endpoints.
fn build_hermes_settings(request: &DeepLinkImportRequest) -> serde_json::Value {
    let endpoint = get_primary_endpoint(request);

    let mut config = serde_json::Map::new();

    if let Some(name) = request.name.as_deref().filter(|s| !s.is_empty()) {
        config.insert("name".to_string(), json!(name));
    }

    if !endpoint.is_empty() {
        config.insert("base_url".to_string(), json!(endpoint));
    }

    if let Some(api_key) = &request.api_key {
        config.insert("api_key".to_string(), json!(api_key));
    }

    config.insert("api_mode".to_string(), json!("chat_completions"));

    if let Some(model) = &request.model {
        config.insert(
            "models".to_string(),
            json!([{ "id": model, "name": model }]),
        );
    }

    json!(config)
}

// =============================================================================
// Config Merge Logic
// =============================================================================

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

    // MCP, Skill and other resource types don't need config merging
    if request.resource != "provider" {
        return Ok(merged);
    }

    match request.app.as_deref().unwrap_or("") {
        "claude" => merge_claude_config(&mut merged, &config_value)?,
        "codex" => merge_codex_config(&mut merged, &config_value)?,
        "gemini" => merge_gemini_config(&mut merged, &config_value)?,
        "grokbuild" => merge_grokbuild_config(&mut merged, &config_value)?,
        // Additive mode apps use JSON config directly; pass through as-is
        "openclaw" | "opencode" | "hermes" => {
            merge_additive_config(&mut merged, &config_value)?;
        }
        "" => {
            // No app specified, skip merging
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
    if request.api_key.as_ref().is_none_or(|s| s.is_empty()) {
        if let Some(token) = env.get("ANTHROPIC_AUTH_TOKEN").and_then(|v| v.as_str()) {
            request.api_key = Some(token.to_string());
        }
    }

    // Auto-fill endpoint if not provided in URL
    if request.endpoint.as_ref().is_none_or(|s| s.is_empty()) {
        if let Some(base_url) = env.get("ANTHROPIC_BASE_URL").and_then(|v| v.as_str()) {
            request.endpoint = Some(base_url.to_string());
        }
    }

    // Auto-fill homepage from endpoint if not provided
    if request.homepage.as_ref().is_none_or(|s| s.is_empty()) {
        if let Some(endpoint) = request.endpoint.as_ref().filter(|s| !s.is_empty()) {
            request.homepage = infer_homepage_from_endpoint(endpoint);
            if request.homepage.is_none() {
                request.homepage = Some("https://anthropic.com".to_string());
            }
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
    // Auto-fill API key from auth.OPENAI_API_KEY or Codex mobile-compatible bearer token.
    if request.api_key.as_ref().is_none_or(|s| s.is_empty()) {
        let config_str = config.get("config").and_then(|v| v.as_str());
        if let Some(api_key) =
            crate::codex_config::extract_codex_api_key(config.get("auth"), config_str)
        {
            request.api_key = Some(api_key.to_string());
        }
    }

    // Auto-fill endpoint and model from config string
    if let Some(config_str) = config.get("config").and_then(|v| v.as_str()) {
        // Parse TOML config string to extract base_url and model
        if let Ok(toml_value) = toml::from_str::<toml::Value>(config_str) {
            // Extract base_url from model_providers section
            if request.endpoint.as_ref().is_none_or(|s| s.is_empty()) {
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
    if request.homepage.as_ref().is_none_or(|s| s.is_empty()) {
        if let Some(endpoint) = request.endpoint.as_ref().filter(|s| !s.is_empty()) {
            request.homepage = infer_homepage_from_endpoint(endpoint);
            if request.homepage.is_none() {
                request.homepage = Some("https://openai.com".to_string());
            }
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
    if request.api_key.as_ref().is_none_or(|s| s.is_empty()) {
        if let Some(api_key) = config.get("GEMINI_API_KEY").and_then(|v| v.as_str()) {
            request.api_key = Some(api_key.to_string());
        }
    }

    if request.endpoint.as_ref().is_none_or(|s| s.is_empty()) {
        if let Some(base_url) = config
            .get("GOOGLE_GEMINI_BASE_URL")
            .or_else(|| config.get("GEMINI_BASE_URL"))
            .and_then(|v| v.as_str())
        {
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
    if request.homepage.as_ref().is_none_or(|s| s.is_empty()) {
        if let Some(endpoint) = request.endpoint.as_ref().filter(|s| !s.is_empty()) {
            request.homepage = infer_homepage_from_endpoint(endpoint);
            if request.homepage.is_none() {
                request.homepage = Some("https://ai.google.dev".to_string());
            }
        }
    }

    Ok(())
}

fn merge_grokbuild_config(
    request: &mut DeepLinkImportRequest,
    config: &serde_json::Value,
) -> Result<(), AppError> {
    let config_toml = if let Some(config_toml) = config.get("config").and_then(|v| v.as_str()) {
        config_toml.to_string()
    } else {
        let toml_value: toml::Value = serde_json::from_value(config.clone()).map_err(|error| {
            AppError::InvalidInput(format!("Invalid Grok Build config: {error}"))
        })?;
        toml::to_string(&toml_value).map_err(|error| {
            AppError::InvalidInput(format!("Invalid Grok Build config: {error}"))
        })?
    };
    let model = crate::grok_config::extract_model_config(&config_toml).ok_or_else(|| {
        AppError::InvalidInput("Invalid Grok Build config.toml model profile".to_string())
    })?;

    if request
        .api_key
        .as_ref()
        .is_none_or(|value| value.is_empty())
    {
        // Only inline an explicitly-declared `api_key`. Do NOT resolve `env_key`
        // (or any process env var) into a plaintext value here: a deeplink is
        // untrusted input, and resolving+inlining would silently persist the
        // victim's environment secret into the imported provider's config.toml
        // and ship it to whatever `base_url` the link declares. `env_key` is an
        // indirection that must stay a name, not a resolved secret, on import.
        request.api_key = model.api_key;

        // An `env_key`-only link is not importable at all, and saying so beats
        // falling through to the generic "API key is required" (which reads like
        // a malformed link and invites a "just carry the name over" fix).
        // Carrying the name over is exactly what must not happen: the forwarder
        // and the usage query both resolve `env_key` at request time, so the
        // victim's environment secret would still reach the link's `base_url`
        // — the same leak, merely deferred.
        if request
            .api_key
            .as_ref()
            .is_none_or(|value| value.is_empty())
            && model.env_key.is_some()
        {
            return Err(AppError::InvalidInput(
                "This link supplies its API key indirectly through `env_key`, which cannot be \
                 imported from an untrusted link. Add the provider manually and enter the key \
                 yourself."
                    .to_string(),
            ));
        }
    }
    if request
        .endpoint
        .as_ref()
        .is_none_or(|value| value.is_empty())
    {
        request.endpoint = Some(model.base_url);
    }
    if request.model.is_none() {
        request.model = Some(model.model);
    }
    if request
        .homepage
        .as_ref()
        .is_none_or(|value| value.is_empty())
    {
        if let Some(endpoint) = request.endpoint.as_deref() {
            request.homepage = infer_homepage_from_endpoint(endpoint);
        }
    }

    Ok(())
}

/// Merge configuration for additive mode apps (OpenClaw, OpenCode)
///
/// These apps use JSON config directly, so we only extract common fields
/// (api_key, endpoint, model) from the config if not already set in URL params.
fn merge_additive_config(
    request: &mut DeepLinkImportRequest,
    config: &serde_json::Value,
) -> Result<(), AppError> {
    // Extract api_key from config if not provided in URL
    if request.api_key.as_ref().is_none_or(|s| s.is_empty()) {
        if let Some(api_key) = config
            .get("apiKey")
            .or_else(|| config.get("api_key"))
            .and_then(|v| v.as_str())
        {
            request.api_key = Some(api_key.to_string());
        }
    }

    // Extract endpoint from config if not provided in URL
    if request.endpoint.as_ref().is_none_or(|s| s.is_empty()) {
        if let Some(base_url) = config
            .get("baseUrl")
            .or_else(|| config.get("base_url"))
            .or_else(|| config.get("options").and_then(|o| o.get("baseURL")))
            .and_then(|v| v.as_str())
        {
            request.endpoint = Some(base_url.to_string());
        }
    }

    // Auto-fill homepage from endpoint
    if request.homepage.as_ref().is_none_or(|s| s.is_empty()) {
        if let Some(endpoint) = request.endpoint.as_ref().filter(|s| !s.is_empty()) {
            request.homepage = infer_homepage_from_endpoint(endpoint);
        }
    }

    Ok(())
}

/// Extract base_url from Codex TOML config
fn extract_codex_base_url(toml_value: &toml::Value) -> Option<String> {
    active_codex_provider(toml_value)?
        .get("base_url")?
        .as_str()
        .map(str::to_string)
}

/// Inference and catalog must select the same provider. Partial config payloads
/// conventionally use custom; never fall back to an unrelated provider table.
fn active_codex_provider(config: &toml::Value) -> Option<&toml::value::Table> {
    let provider_id = config
        .get("model_provider")
        .and_then(|v| v.as_str())
        .unwrap_or("custom");
    config.get("model_providers")?.get(provider_id)?.as_table()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hermes_request() -> DeepLinkImportRequest {
        DeepLinkImportRequest {
            resource: "provider".to_string(),
            app: Some("hermes".to_string()),
            name: Some("MyHermes".to_string()),
            endpoint: Some("https://api.example.com/v1".to_string()),
            api_key: Some("sk-test".to_string()),
            model: Some("anthropic/claude-opus-4-8".to_string()),
            ..Default::default()
        }
    }

    /// deeplink 同时声明 `api_key` 与 `env_key` 时，导入结果只保留用户可见的
    /// `api_key`：既不能带上解析后的明文环境变量，也不能把 `env_key` 这个间接
    /// 引用本身写进供应商配置。
    ///
    /// 后者容易被误当成「应该补上的功能」，但恰恰不能补：deeplink 是不可信输入，
    /// 攻击者可以让 `env_key` 指向 `XAI_API_KEY`、`base_url` 指向自己的服务器。
    /// 密钥虽然导入时没落盘，却会在转发/用量查询调用 `extract_credentials` 时
    /// 被现场解析并发给攻击者——等于把已修掉的泄露换成延迟触发的版本。
    /// 手工建供应商的表单路径不受影响，那里的 env_key 是用户自己输入的。
    #[test]
    #[serial_test::serial]
    fn grokbuild_deeplink_never_carries_env_key_or_resolved_secret() {
        let original = std::env::var_os("CC_SWITCH_DEEPLINK_ENV_PROBE");
        std::env::set_var("CC_SWITCH_DEEPLINK_ENV_PROBE", "secret-must-not-leak");

        let mut request = DeepLinkImportRequest {
            resource: "provider".to_string(),
            app: Some("grokbuild".to_string()),
            name: Some("Attacker".to_string()),
            ..Default::default()
        };
        let config = serde_json::json!({
            "config": concat!(
                "[models]\ndefault = \"grok-env\"\n\n",
                "[model.\"grok-env\"]\nmodel = \"grok-4.5\"\n",
                "base_url = \"https://attacker.example/v1\"\nname = \"Attacker\"\n",
                "api_key = \"sk-declared-by-link\"\n",
                "env_key = \"CC_SWITCH_DEEPLINK_ENV_PROBE\"\n",
                "api_backend = \"responses\"\ncontext_window = 500000\n",
            )
        });

        merge_grokbuild_config(&mut request, &config).expect("merge should succeed");
        let settings = build_grokbuild_settings(&request);
        let rendered = settings["config"].as_str().expect("config string");

        assert!(
            !rendered.contains("secret-must-not-leak"),
            "the environment secret must never be inlined: {rendered}"
        );
        assert!(
            !rendered.contains("env_key"),
            "the env_key indirection must not be carried over from an untrusted link: {rendered}"
        );
        assert!(
            rendered.contains("api_key = \"sk-declared-by-link\""),
            "only the explicitly declared api_key should survive: {rendered}"
        );

        match original {
            Some(value) => std::env::set_var("CC_SWITCH_DEEPLINK_ENV_PROBE", value),
            None => std::env::remove_var("CC_SWITCH_DEEPLINK_ENV_PROBE"),
        }
    }

    /// `env_key` 独苗的链接必须在**公开入口**上被明确拒绝。
    ///
    /// 这条走 `parse_and_merge_config` 而不是内部 helper：真实失败路径在这里，
    /// 而且报错必须指名 `env_key`——否则用户只看到泛化的 "API key is required"，
    /// 读起来像链接坏了，下一步就会有人「顺手把 env_key 透传过去」，把泄露改成
    /// 延迟触发的版本。
    #[test]
    #[serial_test::serial]
    fn grokbuild_env_key_only_link_is_rejected_at_the_public_entry() {
        use base64::prelude::*;

        // 探针变量必须**真的设上**。若它在环境里不存在，旧代码的
        // `extract_credentials` 回退也解析不出东西，api_key 同样留空、同样触发
        // 拒绝——测试就退化成只覆盖"没有 key 时报错"，对"不得解析环境变量"这条
        // 真正的安全属性零覆盖。设上之后，一旦有人恢复回退解析，api_key 会变成
        // 非空、拒绝不再触发，这条断言立刻变红。
        let original = std::env::var_os("CC_SWITCH_DEEPLINK_ENV_PROBE");
        std::env::set_var("CC_SWITCH_DEEPLINK_ENV_PROBE", "secret-must-not-leak");

        let config_toml = concat!(
            "[models]\ndefault = \"grok-env\"\n\n",
            "[model.\"grok-env\"]\nmodel = \"grok-4.5\"\n",
            "base_url = \"https://attacker.example/v1\"\nname = \"Attacker\"\n",
            "env_key = \"CC_SWITCH_DEEPLINK_ENV_PROBE\"\n",
            "api_backend = \"responses\"\ncontext_window = 500000\n",
        );
        let request = DeepLinkImportRequest {
            resource: "provider".to_string(),
            app: Some("grokbuild".to_string()),
            name: Some("Attacker".to_string()),
            config: Some(BASE64_STANDARD.encode(config_toml)),
            config_format: Some("toml".to_string()),
            ..Default::default()
        };

        let result = parse_and_merge_config(&request);

        match original {
            Some(value) => std::env::set_var("CC_SWITCH_DEEPLINK_ENV_PROBE", value),
            None => std::env::remove_var("CC_SWITCH_DEEPLINK_ENV_PROBE"),
        }

        let err = result.expect_err(
            "an env_key-only link must not be importable, and its env var must never be resolved",
        );
        assert!(
            err.to_string().contains("env_key"),
            "the rejection must name env_key so it is not mistaken for a malformed link: {err}"
        );
    }

    #[test]
    fn build_hermes_settings_emits_snake_case() {
        let settings = build_hermes_settings(&hermes_request());
        let obj = settings.as_object().expect("settings must be object");

        assert_eq!(obj.get("name").unwrap(), "MyHermes");
        assert_eq!(obj.get("base_url").unwrap(), "https://api.example.com/v1");
        assert_eq!(obj.get("api_key").unwrap(), "sk-test");

        // camelCase and legacy fields must NOT be present
        assert!(obj.get("baseUrl").is_none(), "no camelCase baseUrl");
        assert!(obj.get("apiKey").is_none(), "no camelCase apiKey");
        assert!(obj.get("api").is_none(), "no legacy 'api' field");

        // models array with the deeplink model id
        let models = obj.get("models").unwrap().as_array().unwrap();
        assert_eq!(models.len(), 1);
        assert_eq!(models[0]["id"], "anthropic/claude-opus-4-8");
    }

    #[test]
    fn build_hermes_settings_writes_default_api_mode() {
        let settings = build_hermes_settings(&hermes_request());
        assert_eq!(
            settings.as_object().unwrap().get("api_mode").unwrap(),
            "chat_completions",
            "api_mode must be written explicitly so Hermes never falls back to URL auto-detection"
        );
    }

    #[test]
    fn build_hermes_settings_skips_missing_optional_fields() {
        let request = DeepLinkImportRequest {
            resource: "provider".to_string(),
            app: Some("hermes".to_string()),
            name: Some("Minimal".to_string()),
            endpoint: None,
            api_key: None,
            model: None,
            ..Default::default()
        };
        let settings = build_hermes_settings(&request);
        let obj = settings.as_object().unwrap();

        assert_eq!(obj.get("name").unwrap(), "Minimal");
        assert!(obj.get("base_url").is_none());
        assert!(obj.get("api_key").is_none());
        assert!(obj.get("models").is_none());
        assert_eq!(obj.get("api_mode").unwrap(), "chat_completions");
    }

    #[test]
    fn build_codex_settings_uses_custom_key_and_preserves_display_name() {
        let request = DeepLinkImportRequest {
            resource: "provider".to_string(),
            app: Some("codex".to_string()),
            name: Some("My \"Relay\"".to_string()),
            endpoint: Some("https://api.example.com/v1/".to_string()),
            api_key: Some("sk-test".to_string()),
            model: Some("gpt-5-codex".to_string()),
            ..Default::default()
        };

        let settings = build_codex_settings(&request).expect("build Codex settings");
        let config_text = settings
            .get("config")
            .and_then(|value| value.as_str())
            .expect("config text");
        let parsed: toml::Value = toml::from_str(config_text).expect("valid Codex config");

        assert_eq!(
            parsed
                .get("model_provider")
                .and_then(|value| value.as_str()),
            Some("custom")
        );
        let custom_provider = parsed
            .get("model_providers")
            .and_then(|value| value.get("custom"))
            .expect("custom model provider");
        assert_eq!(
            custom_provider.get("name").and_then(|value| value.as_str()),
            Some("My \"Relay\"")
        );
        assert_eq!(
            custom_provider
                .get("base_url")
                .and_then(|value| value.as_str()),
            Some("https://api.example.com/v1")
        );
    }

    #[test]
    fn codex_deeplink_preserves_remote_catalog_from_active_provider() {
        use base64::prelude::*;

        let catalog_url = "https://api.example.com/v1/models?format=codex&label=%22test%22";
        let config = format!(
            r#"model_provider = "relay"
model = "ignored-model"
model_catalog_json = "/private/local-models.json"
[model_providers.inactive]
model_catalog_url = "https://inactive.example/models"
[model_providers.relay]
base_url = "https://api.example.com/old"
model_catalog_url = "{catalog_url}"
experimental_bearer_token = "must-not-copy"
[mcp_servers.untrusted]
command = "must-not-copy"
"#
        );
        let request = DeepLinkImportRequest {
            app: Some("codex".to_string()),
            resource: "provider".to_string(),
            name: Some("Relay".to_string()),
            endpoint: Some("https://api.example.com/v1/,https://backup.example/v1".to_string()),
            model: Some("selected-model".to_string()),
            api_key: Some("sk-query-key".to_string()),
            config: Some(BASE64_STANDARD.encode(json!({ "config": config }).to_string())),
            config_format: Some("json".to_string()),
            ..Default::default()
        };
        let merged = parse_and_merge_config(&request).expect("merge inline config");
        let settings = build_codex_settings(&merged).expect("build catalog config");
        let text = settings["config"].as_str().expect("config TOML");
        let doc: toml::Value = toml::from_str(text).expect("valid TOML");
        assert_eq!(doc["model_provider"].as_str(), Some("custom"));
        assert_eq!(doc["model"].as_str(), Some("selected-model"));
        assert_eq!(settings["auth"]["OPENAI_API_KEY"], "sk-query-key");
        assert_eq!(
            doc["model_providers"]["custom"]["base_url"].as_str(),
            Some("https://api.example.com/v1")
        );
        assert_eq!(
            doc["model_providers"]["custom"]["model_catalog_url"].as_str(),
            Some(catalog_url)
        );
        assert!(!text.contains("must-not-copy"));
        assert!(!text.contains("model_catalog_json"));
        assert!(doc["model_providers"].get("inactive").is_none());
    }

    #[test]
    fn codex_deeplink_preserves_catalog_in_partial_json_and_toml_configs() {
        use base64::prelude::*;

        for format in ["json", "toml"] {
            let text = "[model_providers.custom]\nmodel_catalog_url = \"http://127.0.0.1:8000/catalog?format=codex\"\n".to_string();
            let config = if format == "json" {
                json!({ "config": text }).to_string()
            } else {
                text
            };
            let request = DeepLinkImportRequest {
                endpoint: Some("http://127.0.0.1:8000/v1".to_string()),
                config: Some(BASE64_STANDARD.encode(config)),
                config_format: Some(format.to_string()),
                ..Default::default()
            };
            let settings = build_codex_settings(&request).expect("build remote catalog");
            let doc: toml::Value = toml::from_str(settings["config"].as_str().unwrap()).unwrap();
            assert_eq!(
                doc["model_providers"]["custom"]["model_catalog_url"].as_str(),
                Some("http://127.0.0.1:8000/catalog?format=codex")
            );
        }
    }

    #[test]
    fn codex_deeplink_infers_endpoint_from_the_catalogs_provider() {
        use base64::prelude::*;

        for selector in [Some("relay"), None] {
            let provider_id = selector.unwrap_or("custom");
            let selector_line = selector
                .map(|id| format!("model_provider = \"{id}\"\n"))
                .unwrap_or_default();
            let text = format!(
                "{selector_line}model = \"custom-model\"\n\n[model_providers.{provider_id}]\nbase_url = \"https://api.example.com/v1\"\nmodel_catalog_url = \"https://api.example.com/v1/models?format=codex\"\n\n[model_providers.aaa_inactive]\nbase_url = \"https://inactive.example.com/v1\"\n"
            );
            let request = DeepLinkImportRequest {
                resource: "provider".to_string(),
                app: Some("codex".to_string()),
                api_key: Some("sk-test".to_string()),
                config: Some(BASE64_STANDARD.encode(json!({ "config": text }).to_string())),
                ..Default::default()
            };
            let merged = parse_and_merge_config(&request).expect("infer endpoint and model");
            assert_eq!(
                merged.endpoint.as_deref(),
                Some("https://api.example.com/v1")
            );
            assert_eq!(merged.model.as_deref(), Some("custom-model"));
            let settings =
                build_codex_settings(&merged).expect("matching inference and catalog origin");
            let config: toml::Value = toml::from_str(settings["config"].as_str().unwrap()).unwrap();
            assert_eq!(
                config["model_providers"]["custom"]["base_url"].as_str(),
                Some("https://api.example.com/v1")
            );
            assert_eq!(
                config["model_providers"]["custom"]["model_catalog_url"].as_str(),
                Some("https://api.example.com/v1/models?format=codex")
            );
        }
    }

    #[test]
    fn codex_deeplink_does_not_infer_an_inactive_endpoint() {
        let config: toml::Value = toml::from_str(
            "model_provider = \"relay\"\n[model_providers.relay]\nmodel_catalog_url = \"https://api.example.com/models\"\n[model_providers.aaa_inactive]\nbase_url = \"https://inactive.example.com/v1\"\n",
        ).unwrap();
        assert_eq!(extract_codex_base_url(&config), None);
    }

    #[test]
    fn codex_deeplink_rejects_invalid_or_cross_origin_catalogs() {
        use base64::prelude::*;

        for value in [
            json!("https://different.example/models"),
            json!("http://api.example.com/models"),
            json!("https://api.example.com:8443/models"),
            json!("https://user:password@api.example.com/models"),
            json!("https://api.example.com/models#fragment"),
            json!("file:///tmp/catalog.json"),
            json!("/models"),
            json!("https://api.example.com/models\n"),
            json!(42),
        ] {
            let config = json!({ "config": format!("[model_providers.custom]\nmodel_catalog_url = {value}\n") });
            let request = DeepLinkImportRequest {
                endpoint: Some("https://api.example.com/v1".to_string()),
                config: Some(BASE64_STANDARD.encode(config.to_string())),
                ..Default::default()
            };
            let error = build_codex_settings(&request).expect_err("reject unsafe catalog URL");
            assert!(error.to_string().contains("model_catalog_url"));
            assert!(!error.to_string().contains("password"));
        }
    }

    #[test]
    fn codex_deeplink_without_catalog_keeps_legacy_template() {
        use base64::prelude::*;

        let mut request = DeepLinkImportRequest {
            name: Some("Relay".to_string()),
            endpoint: Some("https://api.example.com/v1".to_string()),
            model: Some("gpt-test".to_string()),
            api_key: Some("sk-test".to_string()),
            ..Default::default()
        };
        let expected = "model_provider = \"custom\"\nmodel = \"gpt-test\"\nmodel_reasoning_effort = \"high\"\ndisable_response_storage = true\n\n[model_providers.custom]\nname = \"Relay\"\nbase_url = \"https://api.example.com/v1\"\nwire_api = \"responses\"\nrequires_openai_auth = true\n";
        assert_eq!(build_codex_settings(&request).unwrap()["config"], expected);
        // An inactive provider's catalog does not enable discovery implicitly.
        request.config = Some(BASE64_STANDARD.encode(json!({ "config": "model_provider = \"active\"\n[model_providers.active]\nbase_url = \"https://api.example.com/v1\"\n[model_providers.other]\nmodel_catalog_url = \"https://api.example.com/models\"\n" }).to_string()));
        assert_eq!(build_codex_settings(&request).unwrap()["config"], expected);
    }

    #[test]
    fn openclaw_still_uses_camel_case() {
        // OpenClaw's live config natively uses camelCase; guard against a
        // refactor accidentally flipping it to snake_case.
        let request = DeepLinkImportRequest {
            resource: "provider".to_string(),
            app: Some("openclaw".to_string()),
            name: Some("c".to_string()),
            endpoint: Some("https://api.example.com".to_string()),
            api_key: Some("k".to_string()),
            ..Default::default()
        };
        let settings = build_additive_app_settings(&request);
        let obj = settings.as_object().unwrap();
        assert!(obj.contains_key("baseUrl"));
        assert!(obj.contains_key("apiKey"));
    }
}
