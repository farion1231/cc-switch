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
        AppType::Codex => build_codex_settings(request),
        AppType::Gemini => build_gemini_settings(request),
        AppType::GrokBuild => build_grokbuild_settings(request),
        AppType::OpenCode => build_opencode_settings(request),
        AppType::OpenClaw => build_additive_app_settings(request),
        AppType::Hermes => build_hermes_settings(request),
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
    // Share-link fidelity path: a `usageScriptConfig` blob carries the full
    // `UsageScript` (templateType, codingPlanProvider, Volcengine AK/SK, Zhipu
    // org/project, ...) verbatim, restoring native usage templates the
    // scattered `usage_*` params would drop. Authoritative when present.
    if let Some(blob_b64) = request.usage_script_config.as_deref().filter(|s| !s.is_empty()) {
        let decoded = decode_base64_param("usage_script_config", blob_b64)?;
        let usage_script: UsageScript = serde_json::from_slice(&decoded).map_err(|e| {
            AppError::InvalidInput(format!("Invalid usage_script_config JSON: {e}"))
        })?;
        return Ok(Some(ProviderMeta {
            usage_script: Some(usage_script),
            api_format: request.api_format.clone(),
            ..Default::default()
        }));
    }

    // Check if any usage script fields are provided. `api_format` is tracked
    // separately so a provider that carries only routing metadata doesn't
    // synthesize a spurious empty `usage_script` here (which would break the
    // share-link fidelity gate).
    let has_usage_fields = request.usage_script.is_some()
        || request.usage_enabled.is_some()
        || request.usage_api_key.is_some()
        || request.usage_base_url.is_some()
        || request.usage_access_token.is_some()
        || request.usage_user_id.is_some()
        || request.usage_auto_interval.is_some();
    if !has_usage_fields && request.api_format.is_none() {
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

    // Determine enabled state: explicit param > has code > false
    let enabled = request.usage_enabled.unwrap_or(!code.is_empty());

    let usage_script = has_usage_fields.then(|| UsageScript {
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
    });

    Ok(Some(ProviderMeta {
        usage_script,
        api_format: request.api_format.clone(),
        ..Default::default()
    }))
}

/// Build Claude settings configuration
///
/// When the deeplink carries an inline config with a Claude-shaped `env`
/// object, the full config object is used as the base (top-level keys like
/// `permissions` survive). Standard env fields are then overwritten by URL
/// params, which stay authoritative per the deeplink protocol.
///
/// Precondition: callers must run the request through `parse_and_merge_config`
/// first — standard env fields are unconditionally overwritten from request
/// fields, so an empty `api_key` would write `""`.
///
/// Note: unlike the Codex passthrough (which ignores URL params except
/// `apiKey`), the Claude path applies all standard URL params over the config
/// base. This asymmetry is deliberate: env JSON is mergeable while TOML is not.
fn build_claude_settings(request: &DeepLinkImportRequest) -> serde_json::Value {
    let mut settings = decoded_config_object(request)
        .filter(|cfg| cfg.get("env").is_some_and(|v| v.is_object()))
        .map(serde_json::Value::Object)
        .unwrap_or_else(|| json!({ "env": {} }));

    let env = settings
        .as_object_mut()
        .expect("settings is always an object")
        .get_mut("env")
        .and_then(|v| v.as_object_mut())
        .expect("env is always an object (guarded above)");

    // Write the credential back to the field the sender used. Presets like
    // PatewayAI / Gemini Native / AiHubMix store the key only as
    // `ANTHROPIC_API_KEY`; writing `ANTHROPIC_AUTH_TOKEN` here would inject a
    // spurious second key and break the verbatim round-trip. Default to
    // `ANTHROPIC_AUTH_TOKEN` (the canonical Claude Code field) when the base
    // env doesn't already pin a credential field.
    let cred_field = if env.contains_key("ANTHROPIC_API_KEY")
        && !env.contains_key("ANTHROPIC_AUTH_TOKEN")
    {
        "ANTHROPIC_API_KEY"
    } else {
        "ANTHROPIC_AUTH_TOKEN"
    };
    env.insert(cred_field.to_string(), json!(request.api_key.clone().unwrap_or_default()));
    env.insert(
        "ANTHROPIC_BASE_URL".to_string(),
        json!(get_primary_endpoint(request)),
    );

    if let Some(model) = &request.model {
        env.insert("ANTHROPIC_MODEL".to_string(), json!(model));
    }
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

    settings
}

/// Decode the inline base64 JSON config into an object, if present and valid.
///
/// Best-effort: any decode/parse failure returns None and the caller falls
/// back to the legacy template-rebuild path. `parse_and_merge_config` has
/// already surfaced hard errors during the merge phase.
fn decoded_config_object(
    request: &DeepLinkImportRequest,
) -> Option<serde_json::Map<String, serde_json::Value>> {
    let config_b64 = request.config.as_ref()?;
    let format = request.config_format.as_deref().unwrap_or("json");
    if format != "json" {
        return None;
    }
    let decoded = decode_base64_param("config", config_b64).ok()?;
    let json_str = std::str::from_utf8(&decoded).ok()?;
    match serde_json::from_str::<serde_json::Value>(json_str).ok()? {
        serde_json::Value::Object(map) => Some(map),
        _ => None,
    }
}

/// Build Codex settings configuration
fn build_codex_settings(request: &DeepLinkImportRequest) -> serde_json::Value {
    // Share-link passthrough: a deeplink carrying the native {auth, config} shape
    // keeps the original TOML verbatim — rebuilding from the template used to
    // drop custom details like wire_api / extra keys.
    // On this path URL params other than apiKey are intentionally ignored.
    if let Some(mut settings) = decoded_config_object(request) {
        let has_native_toml = settings.get("config").is_some_and(|v| v.is_string());
        if has_native_toml {
            let existing_key = crate::codex_config::extract_codex_api_key(
                settings.get("auth"),
                settings.get("config").and_then(|v| v.as_str()),
            );
            // URL apiKey 仅在与 config 自带的 key 不同时覆写 auth（保持
            // bearer-token 型配置的 auth 原样，维持往返保真）。
            if let Some(url_key) = request.api_key.as_deref().filter(|k| !k.is_empty()) {
                if existing_key.as_deref() != Some(url_key) {
                    let auth = settings
                        .entry("auth".to_string())
                        .or_insert_with(|| json!({}));
                    if !auth.is_object() {
                        *auth = json!({});
                    }
                    if let Some(auth_obj) = auth.as_object_mut() {
                        auth_obj.insert("OPENAI_API_KEY".to_string(), json!(url_key));
                    }
                }
            }
            return serde_json::Value::Object(settings);
        }
    }

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
    let endpoint = toml_edit::Value::from(endpoint.as_str()).to_string();

    // Build config.toml content
    let config_toml = format!(
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

    json!({
        "auth": {
            "OPENAI_API_KEY": request.api_key,
        },
        "config": config_toml
    })
}

/// Build Gemini settings configuration
///
/// When the deeplink carries an inline config with a native `{ env: {...} }`
/// shape, the full config object is used as the base so extra env keys like
/// `GOOGLE_CLOUD_PROJECT` survive. Standard env fields are then overwritten
/// by URL params, which stay authoritative per the deeplink protocol.
///
/// Precondition: callers must run the request through `parse_and_merge_config`
/// first — standard env fields are unconditionally overwritten from request
/// fields.
fn build_gemini_settings(request: &DeepLinkImportRequest) -> serde_json::Value {
    let mut settings = decoded_config_object(request)
        .filter(|cfg| cfg.get("env").is_some_and(|v| v.is_object()))
        .map(serde_json::Value::Object)
        .unwrap_or_else(|| json!({ "env": {} }));

    let env = settings
        .as_object_mut()
        .expect("settings is always an object")
        .get_mut("env")
        .and_then(|v| v.as_object_mut())
        .expect("env is always an object (guarded above)");

    env.insert("GEMINI_API_KEY".to_string(), json!(request.api_key));
    env.insert(
        "GOOGLE_GEMINI_BASE_URL".to_string(),
        json!(get_primary_endpoint(request)),
    );
    if let Some(model) = &request.model {
        env.insert("GEMINI_MODEL".to_string(), json!(model));
    }

    settings
}

/// Apply explicit `apiKey`/`endpoint`/`model` URL overrides onto a native
/// GrokBuild TOML, preserving unrelated keys via `toml_edit`.
///
/// Returns `Some(updated_toml)` only when at least one value actually differs
/// from the embedded TOML; returns `None` when nothing changed (so the caller
/// can keep the original verbatim) or when the TOML is too malformed to read
/// the selected model profile. Rewrite failures are logged and skipped
/// best-effort rather than aborting the import.
fn apply_grokbuild_passthrough_overrides(
    toml_str: &str,
    request: &DeepLinkImportRequest,
) -> Option<String> {
    let model_cfg = crate::grok_config::extract_model_config(toml_str)?;
    let mut current = toml_str.to_string();
    let mut changed = false;

    if let Some(url_key) = request
        .api_key
        .as_deref()
        .map(str::trim)
        .filter(|k| !k.is_empty())
    {
        if model_cfg.api_key.as_deref().map(str::trim) != Some(url_key) {
            match crate::grok_config::update_api_key(&current, url_key) {
                Ok(updated) => {
                    current = updated;
                    changed = true;
                }
                Err(e) => log::warn!("grokbuild passthrough: apiKey override failed: {e}"),
            }
        }
    }

    let primary = get_primary_endpoint(request);
    let primary_trimmed = primary.trim().trim_end_matches('/');
    if !primary_trimmed.is_empty() {
        let current_base = model_cfg.base_url.trim_end_matches('/');
        if current_base != primary_trimmed {
            match crate::grok_config::update_selected_model_string(
                &current,
                "base_url",
                primary_trimmed,
            ) {
                Ok(updated) => {
                    current = updated;
                    changed = true;
                }
                Err(e) => log::warn!("grokbuild passthrough: endpoint override failed: {e}"),
            }
        }
    }

    if let Some(url_model) = request
        .model
        .as_deref()
        .map(str::trim)
        .filter(|m| !m.is_empty())
    {
        if model_cfg.model.trim() != url_model {
            match crate::grok_config::update_selected_model_string(&current, "model", url_model) {
                Ok(updated) => {
                    current = updated;
                    changed = true;
                }
                Err(e) => log::warn!("grokbuild passthrough: model override failed: {e}"),
            }
        }
    }

    changed.then_some(current)
}

fn build_grokbuild_settings(request: &DeepLinkImportRequest) -> serde_json::Value {
    // Share-link passthrough: a config whose `config` field is a TOML string is
    // the native GrokBuild shape — keep it verbatim so custom TOML keys survive.
    // Legacy deeplinks without an embedded TOML string fall through to the
    // template rebuild below.
    if let Some(mut settings) = decoded_config_object(request) {
        if settings.get("config").is_some_and(|v| v.is_string()) {
            // Apply explicit URL overrides on top of the embedded TOML. Share
            // links carry no `apiKey`/`endpoint`/`model` params, so
            // `parse_and_merge_config` backfills them from the TOML itself and
            // every value matches -> no rewrite -> verbatim. A hand-crafted
            // link that pairs an embedded TOML with explicit overrides would
            // otherwise silently keep the stale embedded credentials/routing;
            // here we rewrite only the differing values (via toml_edit, which
            // preserves comments/ordering and unrelated keys).
            if let Some(toml_str) = settings
                .get("config")
                .and_then(|v| v.as_str())
                .map(str::to_string)
            {
                if let Some(updated) = apply_grokbuild_passthrough_overrides(&toml_str, request) {
                    settings.insert("config".to_string(), serde_json::Value::String(updated));
                }
            }
            return serde_json::Value::Object(settings);
        }
    }

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
    // Share-link passthrough: keep the native OpenCode provider JSON verbatim;
    // URL params only overwrite the canonical options fields. Legacy flat
    // configs (top-level apiKey/baseUrl) fall through to the template rebuild.
    if let Some(mut settings) = decoded_config_object(request)
        .filter(|cfg| cfg.get("options").is_some_and(|v| v.is_object()))
    {
        let opts = settings
            .get_mut("options")
            .and_then(|v| v.as_object_mut())
            .expect("options is an object (guarded above)");
        if let Some(api_key) = request.api_key.as_deref().filter(|s| !s.is_empty()) {
            opts.insert("apiKey".to_string(), json!(api_key));
        }
        let endpoint = get_primary_endpoint(request);
        if !endpoint.is_empty() {
            opts.insert("baseURL".to_string(), json!(endpoint));
        }
        return serde_json::Value::Object(settings);
    }

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
    // Share-link passthrough: keep the native OpenClaw JSON (api / models /
    // any extra keys) verbatim; URL params overwrite the canonical fields
    // only. Bare legacy configs without the `api` protocol field fall
    // through to the template rebuild.
    if let Some(mut settings) =
        decoded_config_object(request).filter(|cfg| cfg.get("api").is_some_and(|v| v.is_string()))
    {
        if let Some(api_key) = request.api_key.as_deref().filter(|s| !s.is_empty()) {
            settings.insert("apiKey".to_string(), json!(api_key));
        }
        let endpoint = get_primary_endpoint(request);
        if !endpoint.is_empty() {
            settings.insert("baseUrl".to_string(), json!(endpoint));
        }
        return serde_json::Value::Object(settings);
    }

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
///
/// Share links carrying the full native shape (with `api_mode`) bypass the
/// template rebuild entirely — see the passthrough branch below.
fn build_hermes_settings(request: &DeepLinkImportRequest) -> serde_json::Value {
    // Share-link passthrough: keep the native Hermes snake_case YAML-shaped
    // JSON (api_mode / models / name) verbatim; URL params overwrite the
    // canonical fields only. Bare legacy configs without `api_mode` fall
    // through to the template rebuild.
    if let Some(mut settings) = decoded_config_object(request)
        .filter(|cfg| cfg.get("api_mode").is_some_and(|v| v.is_string()))
    {
        if let Some(api_key) = request.api_key.as_deref().filter(|s| !s.is_empty()) {
            settings.insert("api_key".to_string(), json!(api_key));
        }
        let endpoint = get_primary_endpoint(request);
        if !endpoint.is_empty() {
            settings.insert("base_url".to_string(), json!(endpoint));
        }
        return serde_json::Value::Object(settings);
    }

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

    // Auto-fill API key if not provided in URL. Prefer ANTHROPIC_AUTH_TOKEN
    // (canonical), fall back to ANTHROPIC_API_KEY for presets like PatewayAI /
    // Gemini Native / AiHubMix that store the key only under that field.
    if request.api_key.as_ref().is_none_or(|s| s.is_empty()) {
        if let Some(token) = env
            .get("ANTHROPIC_AUTH_TOKEN")
            .or_else(|| env.get("ANTHROPIC_API_KEY"))
            .and_then(|v| v.as_str())
        {
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
    // Gemini 历史 deeplink 用 flat 结构；分享链接携带原生 { env: {...} } 形态
    let flat = match config.get("env").filter(|v| v.is_object()) {
        Some(env) => env,
        None => config,
    };

    if request.api_key.as_ref().is_none_or(|s| s.is_empty()) {
        if let Some(api_key) = flat.get("GEMINI_API_KEY").and_then(|v| v.as_str()) {
            request.api_key = Some(api_key.to_string());
        }
    }

    if request.endpoint.as_ref().is_none_or(|s| s.is_empty()) {
        if let Some(base_url) = flat
            .get("GOOGLE_GEMINI_BASE_URL")
            .or_else(|| flat.get("GEMINI_BASE_URL"))
            .and_then(|v| v.as_str())
        {
            request.endpoint = Some(base_url.to_string());
        }
    }

    if request.model.is_none() {
        request.model = flat
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
        request.api_key = model.api_key.or_else(|| {
            crate::grok_config::extract_credentials(&config_toml).map(|(_, api_key)| api_key)
        });
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

/// Merge configuration for additive mode apps (OpenClaw, OpenCode, Hermes)
///
/// These apps use JSON config directly, so we only extract common fields
/// (api_key, endpoint, model) from the config if not already set in URL
/// params. Besides flat top-level keys (`apiKey`/`api_key`,
/// `baseUrl`/`base_url`), this also extracts OpenCode's nested
/// `options.apiKey` / `options.baseURL`.
fn merge_additive_config(
    request: &mut DeepLinkImportRequest,
    config: &serde_json::Value,
) -> Result<(), AppError> {
    // Extract api_key from config if not provided in URL
    if request.api_key.as_ref().is_none_or(|s| s.is_empty()) {
        if let Some(api_key) = config
            .get("apiKey")
            .or_else(|| config.get("api_key"))
            .or_else(|| config.get("options").and_then(|o| o.get("apiKey")))
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

#[cfg(test)]
mod tests {
    use super::*;
    use base64::prelude::*;

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

        let settings = build_codex_settings(&request);
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

    fn b64_config(config: &serde_json::Value) -> String {
        BASE64_STANDARD.encode(serde_json::to_string(config).unwrap())
    }

    fn codex_custom_config() -> serde_json::Value {
        let toml = r#"model_provider = "custom"
model = "gpt-5.2"

[model_providers.custom]
name = "Relay"
base_url = "https://relay.example.com/v1"
wire_api = "chat"
requires_openai_auth = false
"#;
        json!({ "auth": { "OPENAI_API_KEY": "sk-relay" }, "config": toml })
    }

    #[test]
    fn codex_config_passthrough_preserves_custom_toml() {
        let config = codex_custom_config();
        let request = DeepLinkImportRequest {
            resource: "provider".to_string(),
            app: Some("codex".to_string()),
            name: Some("Relay".to_string()),
            config: Some(b64_config(&config)),
            config_format: Some("json".to_string()),
            ..Default::default()
        };
        let merged = parse_and_merge_config(&request).unwrap();
        let settings = build_codex_settings(&merged);
        // 原 TOML（wire_api="chat" 等自定义细节）与 auth 必须原样保留
        assert_eq!(settings, config);
    }

    #[test]
    fn codex_url_api_key_overrides_config_auth() {
        let config = codex_custom_config();
        let request = DeepLinkImportRequest {
            resource: "provider".to_string(),
            app: Some("codex".to_string()),
            name: Some("Relay".to_string()),
            api_key: Some("sk-url-wins".to_string()),
            config: Some(b64_config(&config)),
            config_format: Some("json".to_string()),
            ..Default::default()
        };
        let merged = parse_and_merge_config(&request).unwrap();
        let settings = build_codex_settings(&merged);
        assert_eq!(settings["auth"]["OPENAI_API_KEY"], "sk-url-wins");
        // TOML 本体不受 apiKey 覆写影响
        assert_eq!(settings["config"], config["config"]);
    }

    #[test]
    fn codex_auth_only_config_falls_back_to_template() {
        let config = json!({ "auth": { "OPENAI_API_KEY": "sk-auth-only" } });
        let request = DeepLinkImportRequest {
            resource: "provider".to_string(),
            app: Some("codex".to_string()),
            name: Some("AuthOnly".to_string()),
            endpoint: Some("https://relay.example.com/v1".to_string()),
            config: Some(b64_config(&config)),
            config_format: Some("json".to_string()),
            ..Default::default()
        };
        let merged = parse_and_merge_config(&request).unwrap();
        let settings = build_codex_settings(&merged);
        let config_text = settings.get("config").and_then(|v| v.as_str()).unwrap();
        assert!(config_text.contains("base_url = \"https://relay.example.com/v1\""));
        assert_eq!(settings["auth"]["OPENAI_API_KEY"], "sk-auth-only");
    }

    #[test]
    fn claude_config_passthrough_preserves_top_level_and_custom_env() {
        let config = json!({
            "env": {
                "ANTHROPIC_AUTH_TOKEN": "sk-team",
                "ANTHROPIC_BASE_URL": "https://relay.example.com",
                "ANTHROPIC_CUSTOM_HEADERS": "X-Team: a",
                "API_TIMEOUT_MS": "600000"
            },
            "permissions": { "allow": ["Bash"] }
        });
        let request = DeepLinkImportRequest {
            resource: "provider".to_string(),
            app: Some("claude".to_string()),
            name: Some("团队 Claude".to_string()),
            config: Some(b64_config(&config)),
            config_format: Some("json".to_string()),
            ..Default::default()
        };
        let merged = parse_and_merge_config(&request).unwrap();
        let settings = build_claude_settings(&merged);
        assert_eq!(settings, config);
    }

    #[test]
    fn claude_url_params_override_config_env_on_passthrough() {
        let config = json!({
            "env": {
                "ANTHROPIC_AUTH_TOKEN": "sk-old",
                "ANTHROPIC_BASE_URL": "https://old.example.com"
            },
            "permissions": { "allow": ["Bash"] }
        });
        let request = DeepLinkImportRequest {
            resource: "provider".to_string(),
            app: Some("claude".to_string()),
            name: Some("C".to_string()),
            api_key: Some("sk-url-wins".to_string()),
            endpoint: Some("https://url.example.com".to_string()),
            config: Some(b64_config(&config)),
            config_format: Some("json".to_string()),
            ..Default::default()
        };
        let merged = parse_and_merge_config(&request).unwrap();
        let settings = build_claude_settings(&merged);
        assert_eq!(settings["env"]["ANTHROPIC_AUTH_TOKEN"], "sk-url-wins");
        assert_eq!(
            settings["env"]["ANTHROPIC_BASE_URL"],
            "https://url.example.com"
        );
        assert_eq!(settings["permissions"], json!({ "allow": ["Bash"] }));
    }

    #[test]
    fn gemini_config_passthrough_supports_env_wrapper() {
        // 分享侧导出的是原生 settings 形态 { env: {...} }，而非历史 flat 形态
        let config = json!({
            "env": {
                "GEMINI_API_KEY": "gk-1",
                "GOOGLE_GEMINI_BASE_URL": "https://g.example.com",
                "GEMINI_MODEL": "gemini-3-pro",
                "GOOGLE_CLOUD_PROJECT": "my-proj"
            }
        });
        let request = DeepLinkImportRequest {
            resource: "provider".to_string(),
            app: Some("gemini".to_string()),
            name: Some("G".to_string()),
            config: Some(b64_config(&config)),
            config_format: Some("json".to_string()),
            ..Default::default()
        };
        let merged = parse_and_merge_config(&request).unwrap();
        assert_eq!(merged.api_key.as_deref(), Some("gk-1"));
        assert_eq!(merged.endpoint.as_deref(), Some("https://g.example.com"));
        let settings = build_gemini_settings(&merged);
        assert_eq!(settings, config);
    }

    #[test]
    fn gemini_flat_config_still_merges() {
        // 历史 deeplink 的 flat 形态必须继续可用
        let config = json!({
            "GEMINI_API_KEY": "gk-flat",
            "GEMINI_BASE_URL": "https://flat.example.com",
            "GEMINI_MODEL": "gemini-3-pro"
        });
        let request = DeepLinkImportRequest {
            resource: "provider".to_string(),
            app: Some("gemini".to_string()),
            name: Some("G".to_string()),
            config: Some(b64_config(&config)),
            config_format: Some("json".to_string()),
            ..Default::default()
        };
        let merged = parse_and_merge_config(&request).unwrap();
        assert_eq!(merged.api_key.as_deref(), Some("gk-flat"));
        assert_eq!(merged.endpoint.as_deref(), Some("https://flat.example.com"));
        assert_eq!(merged.model.as_deref(), Some("gemini-3-pro"));
    }

    #[test]
    fn gemini_url_params_override_config_env_on_passthrough() {
        let config = json!({
            "env": {
                "GEMINI_API_KEY": "gk-old",
                "GOOGLE_GEMINI_BASE_URL": "https://old.example.com",
                "GOOGLE_CLOUD_PROJECT": "my-proj"
            }
        });
        let request = DeepLinkImportRequest {
            resource: "provider".to_string(),
            app: Some("gemini".to_string()),
            name: Some("G".to_string()),
            api_key: Some("gk-url-wins".to_string()),
            endpoint: Some("https://url.example.com".to_string()),
            config: Some(b64_config(&config)),
            config_format: Some("json".to_string()),
            ..Default::default()
        };
        let merged = parse_and_merge_config(&request).unwrap();
        let settings = build_gemini_settings(&merged);
        assert_eq!(settings["env"]["GEMINI_API_KEY"], "gk-url-wins");
        assert_eq!(
            settings["env"]["GOOGLE_GEMINI_BASE_URL"],
            "https://url.example.com"
        );
        assert_eq!(settings["env"]["GOOGLE_CLOUD_PROJECT"], "my-proj");
    }

    #[test]
    fn opencode_config_passthrough_preserves_npm_and_models() {
        let config = json!({
            "npm": "@ai-sdk/anthropic",
            "options": { "baseURL": "https://oc.example.com/v1", "apiKey": "sk-oc" },
            "models": { "claude-opus-4": { "name": "claude-opus-4" } }
        });
        let request = DeepLinkImportRequest {
            resource: "provider".to_string(),
            app: Some("opencode".to_string()),
            name: Some("OC".to_string()),
            config: Some(b64_config(&config)),
            config_format: Some("json".to_string()),
            ..Default::default()
        };
        let merged = parse_and_merge_config(&request).unwrap();
        // options.apiKey 必须能被提取，否则导入端会因缺 key 拒绝
        assert_eq!(merged.api_key.as_deref(), Some("sk-oc"));
        let settings = build_opencode_settings(&merged);
        assert_eq!(settings, config);
    }

    #[test]
    fn opencode_url_params_override_config_options_on_passthrough() {
        let config = json!({
            "npm": "@ai-sdk/anthropic",
            "options": { "baseURL": "https://old.example.com/v1", "apiKey": "sk-old" },
            "models": { "claude-opus-4": { "name": "claude-opus-4" } }
        });
        let request = DeepLinkImportRequest {
            resource: "provider".to_string(),
            app: Some("opencode".to_string()),
            name: Some("OC".to_string()),
            api_key: Some("sk-url-wins".to_string()),
            endpoint: Some("https://url.example.com/v1".to_string()),
            config: Some(b64_config(&config)),
            config_format: Some("json".to_string()),
            ..Default::default()
        };
        let merged = parse_and_merge_config(&request).unwrap();
        let settings = build_opencode_settings(&merged);
        assert_eq!(settings["options"]["apiKey"], "sk-url-wins");
        assert_eq!(settings["options"]["baseURL"], "https://url.example.com/v1");
        assert_eq!(settings["npm"], "@ai-sdk/anthropic");
    }

    #[test]
    fn opencode_flat_config_falls_back_to_template() {
        // 历史 flat 形态（无 options 对象）必须继续走模板重建
        let config = json!({ "baseUrl": "https://flat.example.com/v1", "apiKey": "sk-flat" });
        let request = DeepLinkImportRequest {
            resource: "provider".to_string(),
            app: Some("opencode".to_string()),
            name: Some("OC".to_string()),
            config: Some(b64_config(&config)),
            config_format: Some("json".to_string()),
            ..Default::default()
        };
        let merged = parse_and_merge_config(&request).unwrap();
        let settings = build_opencode_settings(&merged);
        // 模板重建：无顶层 baseUrl/apiKey 残留，配置进入 options
        assert!(settings.get("baseUrl").is_none());
        assert!(settings.get("apiKey").is_none());
        assert_eq!(settings["npm"], "@ai-sdk/openai-compatible");
        assert_eq!(settings["options"]["apiKey"], "sk-flat");
        assert_eq!(
            settings["options"]["baseURL"],
            "https://flat.example.com/v1"
        );
    }

    #[test]
    fn openclaw_config_passthrough_preserves_api_and_models() {
        let config = json!({
            "baseUrl": "https://claw.example.com",
            "apiKey": "sk-claw",
            "api": "anthropic-messages",
            "models": [{ "id": "m1", "name": "m1" }, { "id": "m2", "name": "m2" }]
        });
        let request = DeepLinkImportRequest {
            resource: "provider".to_string(),
            app: Some("openclaw".to_string()),
            name: Some("Claw".to_string()),
            config: Some(b64_config(&config)),
            config_format: Some("json".to_string()),
            ..Default::default()
        };
        let merged = parse_and_merge_config(&request).unwrap();
        let settings = build_additive_app_settings(&merged);
        assert_eq!(settings, config);
    }

    #[test]
    fn hermes_config_passthrough_preserves_api_mode_and_models() {
        let config = json!({
            "name": "H",
            "base_url": "https://h.example.com/v1",
            "api_key": "sk-h",
            "api_mode": "anthropic_messages",
            "models": [{ "id": "opus", "name": "opus" }]
        });
        let request = DeepLinkImportRequest {
            resource: "provider".to_string(),
            app: Some("hermes".to_string()),
            name: Some("H".to_string()),
            config: Some(b64_config(&config)),
            config_format: Some("json".to_string()),
            ..Default::default()
        };
        let merged = parse_and_merge_config(&request).unwrap();
        let settings = build_hermes_settings(&merged);
        assert_eq!(settings, config);
    }

    #[test]
    fn openclaw_bare_config_falls_back_to_template() {
        // 无 api 协议字段的历史 config 必须走模板重建（补 api 字段）
        let config = json!({ "baseUrl": "https://bare.example.com", "apiKey": "sk-bare" });
        let request = DeepLinkImportRequest {
            resource: "provider".to_string(),
            app: Some("openclaw".to_string()),
            name: Some("Claw".to_string()),
            config: Some(b64_config(&config)),
            config_format: Some("json".to_string()),
            ..Default::default()
        };
        let merged = parse_and_merge_config(&request).unwrap();
        let settings = build_additive_app_settings(&merged);
        assert_eq!(settings["api"], "openai-completions");
        assert_eq!(settings["apiKey"], "sk-bare");
        assert_eq!(settings["baseUrl"], "https://bare.example.com");
    }

    #[test]
    fn hermes_bare_config_falls_back_to_template() {
        let config = json!({ "base_url": "https://bare.example.com/v1", "api_key": "sk-bare" });
        let request = DeepLinkImportRequest {
            resource: "provider".to_string(),
            app: Some("hermes".to_string()),
            name: Some("H".to_string()),
            config: Some(b64_config(&config)),
            config_format: Some("json".to_string()),
            ..Default::default()
        };
        let merged = parse_and_merge_config(&request).unwrap();
        let settings = build_hermes_settings(&merged);
        assert_eq!(settings["api_mode"], "chat_completions");
        assert_eq!(settings["api_key"], "sk-bare");
        assert_eq!(settings["base_url"], "https://bare.example.com/v1");
    }

    #[test]
    fn openclaw_url_params_override_config_on_passthrough() {
        let config = json!({
            "baseUrl": "https://old.example.com",
            "apiKey": "sk-old",
            "api": "anthropic-messages",
            "models": [{ "id": "m1", "name": "m1" }]
        });
        let request = DeepLinkImportRequest {
            resource: "provider".to_string(),
            app: Some("openclaw".to_string()),
            name: Some("Claw".to_string()),
            api_key: Some("sk-url-wins".to_string()),
            endpoint: Some("https://url.example.com".to_string()),
            config: Some(b64_config(&config)),
            config_format: Some("json".to_string()),
            ..Default::default()
        };
        let merged = parse_and_merge_config(&request).unwrap();
        let settings = build_additive_app_settings(&merged);
        assert_eq!(settings["apiKey"], "sk-url-wins");
        assert_eq!(settings["baseUrl"], "https://url.example.com");
        // 协议字段与自定义键原样保留
        assert_eq!(settings["api"], "anthropic-messages");
        assert_eq!(settings["models"], config["models"]);
    }

    #[test]
    fn hermes_url_params_override_config_on_passthrough() {
        let config = json!({
            "name": "H",
            "base_url": "https://old.example.com/v1",
            "api_key": "sk-old",
            "api_mode": "anthropic_messages",
            "models": [{ "id": "opus", "name": "opus" }]
        });
        let request = DeepLinkImportRequest {
            resource: "provider".to_string(),
            app: Some("hermes".to_string()),
            name: Some("H".to_string()),
            api_key: Some("sk-url-wins".to_string()),
            endpoint: Some("https://url.example.com/v1".to_string()),
            config: Some(b64_config(&config)),
            config_format: Some("json".to_string()),
            ..Default::default()
        };
        let merged = parse_and_merge_config(&request).unwrap();
        let settings = build_hermes_settings(&merged);
        assert_eq!(settings["api_key"], "sk-url-wins");
        assert_eq!(settings["base_url"], "https://url.example.com/v1");
        assert_eq!(settings["api_mode"], "anthropic_messages");
        assert_eq!(settings["models"], config["models"]);
    }

    #[test]
    fn grokbuild_config_passthrough_preserves_original_toml() {
        let toml = "[models]\ndefault = \"grok-code\"\n\n[model.\"grok-code\"]\nmodel = \"grok-code\"\nbase_url = \"https://grok.example.com/v1\"\nname = \"custom\"\napi_key = \"sk-grok\"\napi_backend = \"anthropic\"\ncontext_window = 256000\ncustom_flag = true\n";
        let config = json!({ "config": toml });
        let request = DeepLinkImportRequest {
            resource: "provider".to_string(),
            app: Some("grokbuild".to_string()),
            name: Some("Grok".to_string()),
            config: Some(b64_config(&config)),
            config_format: Some("json".to_string()),
            ..Default::default()
        };
        let merged = parse_and_merge_config(&request).unwrap();
        let settings = build_grokbuild_settings(&merged);
        // 自定义 TOML 字段 (custom_flag, api_backend="anthropic") 必须原样保留
        assert_eq!(settings, config);
    }

    #[test]
    fn grokbuild_config_without_toml_falls_back_to_template() {
        // config 字段非字符串 → passthrough 门禁不触发，走模板重建
        let config = json!({ "config": { "models": {} } });
        let request = DeepLinkImportRequest {
            resource: "provider".to_string(),
            app: Some("grokbuild".to_string()),
            name: Some("Grok".to_string()),
            endpoint: Some("https://g.example.com/v1".to_string()),
            api_key: Some("sk-g".to_string()),
            config: Some(b64_config(&config)),
            config_format: Some("json".to_string()),
            ..Default::default()
        };
        let settings = build_grokbuild_settings(&request);
        let toml = settings["config"]
            .as_str()
            .expect("config is a toml string");
        assert!(toml.contains("api_backend"));
        assert!(toml.contains("sk-g"));
        assert!(toml.contains("https://g.example.com/v1"));
    }
}
