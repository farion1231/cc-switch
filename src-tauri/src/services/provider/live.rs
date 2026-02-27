//! Live configuration operations
//!
//! Handles reading and writing live configuration files for Claude, Codex, and Gemini.

use std::collections::HashMap;

use serde_json::{json, Value};

use crate::app_config::AppType;
use crate::codex_config::{get_codex_auth_path, get_codex_config_path};
use crate::config::{delete_file, get_claude_settings_path, read_json_file, write_json_file};
use crate::error::AppError;
use crate::provider::Provider;
use crate::services::mcp::McpService;
use crate::store::AppState;

use super::gemini_auth::{
    detect_gemini_auth_type, ensure_google_oauth_security_flag, GeminiAuthType,
};
use super::normalize_claude_models_in_value;

pub(crate) fn sanitize_claude_settings_for_live(settings: &Value) -> Value {
    let mut v = settings.clone();
    if let Some(obj) = v.as_object_mut() {
        // Internal-only fields - never write to Claude Code settings.json
        obj.remove("api_format");
        obj.remove("apiFormat");
        obj.remove("openrouter_compat_mode");
        obj.remove("openrouterCompatMode");
    }
    v
}

/// Live configuration snapshot for backup/restore
#[derive(Clone)]
#[allow(dead_code)]
pub(crate) enum LiveSnapshot {
    Claude {
        settings: Option<Value>,
    },
    Codex {
        auth: Option<Value>,
        config: Option<String>,
    },
    Gemini {
        env: Option<HashMap<String, String>>,
        config: Option<Value>,
    },
}

impl LiveSnapshot {
    #[allow(dead_code)]
    pub(crate) fn restore(&self) -> Result<(), AppError> {
        match self {
            LiveSnapshot::Claude { settings } => {
                let path = get_claude_settings_path();
                if let Some(value) = settings {
                    write_json_file(&path, value)?;
                } else if path.exists() {
                    delete_file(&path)?;
                }
            }
            LiveSnapshot::Codex { auth, config } => {
                let auth_path = get_codex_auth_path();
                let config_path = get_codex_config_path();
                if let Some(value) = auth {
                    write_json_file(&auth_path, value)?;
                } else if auth_path.exists() {
                    delete_file(&auth_path)?;
                }

                if let Some(text) = config {
                    crate::config::write_text_file(&config_path, text)?;
                } else if config_path.exists() {
                    delete_file(&config_path)?;
                }
            }
            LiveSnapshot::Gemini { env, .. } => {
                use crate::gemini_config::{
                    get_gemini_env_path, get_gemini_settings_path, write_gemini_env_atomic,
                };
                let path = get_gemini_env_path();
                if let Some(env_map) = env {
                    write_gemini_env_atomic(env_map)?;
                } else if path.exists() {
                    delete_file(&path)?;
                }

                let settings_path = get_gemini_settings_path();
                match self {
                    LiveSnapshot::Gemini {
                        config: Some(cfg), ..
                    } => {
                        write_json_file(&settings_path, cfg)?;
                    }
                    LiveSnapshot::Gemini { config: None, .. } if settings_path.exists() => {
                        delete_file(&settings_path)?;
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    }
}

/// Write live configuration snapshot for a provider
pub(crate) fn write_live_snapshot(app_type: &AppType, provider: &Provider) -> Result<(), AppError> {
    match app_type {
        AppType::Claude => {
            let path = get_claude_settings_path();
            let settings = sanitize_claude_settings_for_live(&provider.settings_config);
            write_json_file(&path, &settings)?;
        }
        AppType::Codex => {
            let obj = provider
                .settings_config
                .as_object()
                .ok_or_else(|| AppError::Config("Codex 供应商配置必须是 JSON 对象".to_string()))?;
            let auth = obj
                .get("auth")
                .ok_or_else(|| AppError::Config("Codex 供应商配置缺少 'auth' 字段".to_string()))?;
            let config_str = obj.get("config").and_then(|v| v.as_str()).ok_or_else(|| {
                AppError::Config("Codex 供应商配置缺少 'config' 字段或不是字符串".to_string())
            })?;

            let auth_path = get_codex_auth_path();
            write_json_file(&auth_path, auth)?;
            let config_path = get_codex_config_path();
            std::fs::write(&config_path, config_str).map_err(|e| AppError::io(&config_path, e))?;
        }
        AppType::Gemini => {
            // Delegate to write_gemini_live which handles env file writing correctly
            write_gemini_live(provider)?;
        }
        AppType::OpenCode => {
            // OpenCode uses additive mode - write provider to config
            use crate::opencode_config;
            use crate::provider::OpenCodeProviderConfig;

            // Defensive check: if settings_config is a full config structure, extract provider fragment
            let config_to_write = if let Some(obj) = provider.settings_config.as_object() {
                // Detect full config structure (has $schema or top-level provider field)
                if obj.contains_key("$schema") || obj.contains_key("provider") {
                    log::warn!(
                        "OpenCode provider '{}' has full config structure in settings_config, attempting to extract fragment",
                        provider.id
                    );
                    // Try to extract from provider.{id}
                    obj.get("provider")
                        .and_then(|p| p.get(&provider.id))
                        .cloned()
                        .unwrap_or_else(|| provider.settings_config.clone())
                } else {
                    provider.settings_config.clone()
                }
            } else {
                provider.settings_config.clone()
            };

            // Convert settings_config to OpenCodeProviderConfig
            let opencode_config_result =
                serde_json::from_value::<OpenCodeProviderConfig>(config_to_write.clone());

            match opencode_config_result {
                Ok(config) => {
                    opencode_config::set_typed_provider(&provider.id, &config)?;
                    log::info!("OpenCode provider '{}' written to live config", provider.id);
                }
                Err(e) => {
                    log::warn!(
                        "Failed to parse OpenCode provider config for '{}': {}",
                        provider.id,
                        e
                    );
                    // Only write if config looks like a valid provider fragment
                    if config_to_write.get("npm").is_some()
                        || config_to_write.get("options").is_some()
                    {
                        opencode_config::set_provider(&provider.id, config_to_write)?;
                        log::info!(
                            "OpenCode provider '{}' written as raw JSON to live config",
                            provider.id
                        );
                    } else {
                        log::error!(
                            "OpenCode provider '{}' has invalid config structure, skipping write",
                            provider.id
                        );
                    }
                }
            }
        }
        AppType::OpenClaw => {
            // OpenClaw uses additive mode - write provider to config
            use crate::openclaw_config;
            use crate::openclaw_config::OpenClawProviderConfig;

            // Convert settings_config to OpenClawProviderConfig
            let openclaw_config_result =
                serde_json::from_value::<OpenClawProviderConfig>(provider.settings_config.clone());

            match openclaw_config_result {
                Ok(config) => {
                    openclaw_config::set_typed_provider(&provider.id, &config)?;
                    log::info!("OpenClaw provider '{}' written to live config", provider.id);
                }
                Err(e) => {
                    log::warn!(
                        "Failed to parse OpenClaw provider config for '{}': {}",
                        provider.id,
                        e
                    );
                    // Try to write as raw JSON if it looks valid
                    if provider.settings_config.get("baseUrl").is_some()
                        || provider.settings_config.get("api").is_some()
                        || provider.settings_config.get("models").is_some()
                    {
                        openclaw_config::set_provider(
                            &provider.id,
                            provider.settings_config.clone(),
                        )?;
                        log::info!(
                            "OpenClaw provider '{}' written as raw JSON to live config",
                            provider.id
                        );
                    } else {
                        log::error!(
                            "OpenClaw provider '{}' has invalid config structure, skipping write",
                            provider.id
                        );
                    }
                }
            }
        }
    }
    Ok(())
}

// ============================================================================
// Key fields definitions for partial merge
// ============================================================================

/// Claude env-level key fields that belong to the provider.
/// When adding a new field here, also update backfill_claude_key_fields().
const CLAUDE_KEY_ENV_FIELDS: &[&str] = &[
    // --- API auth & endpoint ---
    "ANTHROPIC_BASE_URL",
    "ANTHROPIC_AUTH_TOKEN",
    "ANTHROPIC_API_KEY",
    // --- Model selection ---
    "ANTHROPIC_MODEL",
    "ANTHROPIC_REASONING_MODEL",
    "ANTHROPIC_SMALL_FAST_MODEL",
    "ANTHROPIC_DEFAULT_HAIKU_MODEL",
    "ANTHROPIC_DEFAULT_SONNET_MODEL",
    "ANTHROPIC_DEFAULT_OPUS_MODEL",
    "CLAUDE_CODE_SUBAGENT_MODEL",
    // --- AWS Bedrock ---
    "CLAUDE_CODE_USE_BEDROCK",
    "AWS_ACCESS_KEY_ID",
    "AWS_SECRET_ACCESS_KEY",
    "AWS_SESSION_TOKEN",
    "AWS_REGION",
    "AWS_PROFILE",
    "ANTHROPIC_SMALL_FAST_MODEL_AWS_REGION",
    // --- Google Vertex AI ---
    "CLAUDE_CODE_USE_VERTEX",
    "ANTHROPIC_VERTEX_PROJECT_ID",
    "CLOUD_ML_REGION",
    // --- Microsoft Foundry ---
    "CLAUDE_CODE_USE_FOUNDRY",
    // --- Provider behavior ---
    "CLAUDE_CODE_MAX_OUTPUT_TOKENS",
    "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC",
    "API_TIMEOUT_MS",
    "DISABLE_PROMPT_CACHING",
];

/// Claude top-level key fields (legacy + modern format).
/// When adding a new field here, also update backfill_claude_key_fields().
const CLAUDE_KEY_TOP_LEVEL: &[&str] = &[
    "apiBaseUrl",     // legacy
    "primaryModel",   // legacy
    "smallFastModel", // legacy
    "model",          // modern
    "apiKey",         // Bedrock API Key auth
];

/// Codex TOML key fields.
/// When adding a new field here, also update backfill_codex_key_fields().
const CODEX_KEY_TOP_LEVEL: &[&str] = &[
    "model_provider",
    "model",
    "model_reasoning_effort",
    "review_model",
    "plan_mode_reasoning_effort",
];

/// Gemini env-level key fields.
/// When adding a new field here, also update backfill_gemini_key_fields().
const GEMINI_KEY_ENV_FIELDS: &[&str] = &[
    "GOOGLE_GEMINI_BASE_URL",
    "GEMINI_API_KEY",
    "GEMINI_MODEL",
    "GOOGLE_API_KEY",
];

// ============================================================================
// Partial merge: write only key fields to live config
// ============================================================================

/// Write only provider-specific key fields to live configuration,
/// preserving all other user settings in the live file.
///
/// Used for switch-mode apps (Claude, Codex, Gemini) during:
/// - `switch_normal()` — switching providers
/// - `sync_current_to_live()` — startup sync
/// - `add()` / `update()` when the provider is current
pub(crate) fn write_live_partial(app_type: &AppType, provider: &Provider) -> Result<(), AppError> {
    match app_type {
        AppType::Claude => write_claude_live_partial(provider),
        AppType::Codex => write_codex_live_partial(provider),
        AppType::Gemini => write_gemini_live_partial(provider),
        // Additive mode apps still use full snapshot
        AppType::OpenCode | AppType::OpenClaw => write_live_snapshot(app_type, provider),
    }
}

/// Apply a JSON merge patch (RFC 7396) directly to Claude live settings.json.
/// Used for user-level preferences (attribution, thinking, etc.) that are
/// independent of the active provider.
pub fn patch_claude_live(patch: Value) -> Result<(), AppError> {
    let path = get_claude_settings_path();
    let mut live = if path.exists() {
        read_json_file(&path).unwrap_or_else(|_| json!({}))
    } else {
        json!({})
    };
    json_merge_patch(&mut live, &patch);
    let settings = sanitize_claude_settings_for_live(&live);
    write_json_file(&path, &settings)?;
    Ok(())
}

/// RFC 7396 JSON Merge Patch: null deletes, objects merge recursively, rest overwrites.
fn json_merge_patch(target: &mut Value, patch: &Value) {
    if let Some(patch_obj) = patch.as_object() {
        if !target.is_object() {
            *target = json!({});
        }
        let target_obj = target.as_object_mut().unwrap();
        for (key, value) in patch_obj {
            if value.is_null() {
                target_obj.remove(key);
            } else if value.is_object() {
                let entry = target_obj.entry(key.clone()).or_insert(json!({}));
                json_merge_patch(entry, value);
                // Clean up empty container objects
                if entry.as_object().is_some_and(|o| o.is_empty()) {
                    target_obj.remove(key);
                }
            } else {
                target_obj.insert(key.clone(), value.clone());
            }
        }
    }
}

/// Claude: merge only key env and top-level fields into live settings.json
fn write_claude_live_partial(provider: &Provider) -> Result<(), AppError> {
    let path = get_claude_settings_path();

    // 1. Read existing live config (start from empty if file doesn't exist)
    let mut live = if path.exists() {
        read_json_file(&path).unwrap_or_else(|_| json!({}))
    } else {
        json!({})
    };

    // 2. Ensure live.env exists as an object
    if !live.get("env").is_some_and(|v| v.is_object()) {
        live.as_object_mut()
            .unwrap()
            .insert("env".into(), json!({}));
    }

    // 3. Clear key env fields from live, then write from provider
    let live_env = live.get_mut("env").unwrap().as_object_mut().unwrap();
    for key in CLAUDE_KEY_ENV_FIELDS {
        live_env.remove(*key);
    }

    if let Some(provider_env) = provider
        .settings_config
        .get("env")
        .and_then(|v| v.as_object())
    {
        for key in CLAUDE_KEY_ENV_FIELDS {
            if let Some(value) = provider_env.get(*key) {
                live_env.insert(key.to_string(), value.clone());
            }
        }
    }

    // 4. Handle top-level legacy key fields
    let live_obj = live.as_object_mut().unwrap();
    for key in CLAUDE_KEY_TOP_LEVEL {
        live_obj.remove(*key);
    }
    if let Some(provider_obj) = provider.settings_config.as_object() {
        for key in CLAUDE_KEY_TOP_LEVEL {
            if let Some(value) = provider_obj.get(*key) {
                live_obj.insert(key.to_string(), value.clone());
            }
        }
    }

    // 5. Sanitize and write
    let settings = sanitize_claude_settings_for_live(&live);
    write_json_file(&path, &settings)?;
    Ok(())
}

/// Codex: replace auth.json entirely, partially merge config.toml key fields
fn write_codex_live_partial(provider: &Provider) -> Result<(), AppError> {
    let obj = provider
        .settings_config
        .as_object()
        .ok_or_else(|| AppError::Config("Codex 供应商配置必须是 JSON 对象".to_string()))?;

    // auth.json is entirely provider-specific, replace it wholesale
    let auth = obj
        .get("auth")
        .ok_or_else(|| AppError::Config("Codex 供应商配置缺少 'auth' 字段".to_string()))?;

    let provider_config_str = obj.get("config").and_then(|v| v.as_str()).unwrap_or("");

    // Read existing config.toml (or start from empty)
    let config_path = get_codex_config_path();
    let existing_toml = if config_path.exists() {
        std::fs::read_to_string(&config_path).unwrap_or_default()
    } else {
        String::new()
    };

    // Parse both existing and provider TOML
    let mut live_doc = existing_toml
        .parse::<toml_edit::DocumentMut>()
        .unwrap_or_else(|_| toml_edit::DocumentMut::new());

    // Remove key fields from live doc
    let live_root = live_doc.as_table_mut();
    for key in CODEX_KEY_TOP_LEVEL {
        live_root.remove(key);
    }
    live_root.remove("model_providers");

    // Parse provider TOML and extract key fields
    if !provider_config_str.is_empty() {
        if let Ok(provider_doc) = provider_config_str.parse::<toml_edit::DocumentMut>() {
            let provider_root = provider_doc.as_table();

            // Copy key top-level fields from provider
            for key in CODEX_KEY_TOP_LEVEL {
                if let Some(item) = provider_root.get(key) {
                    live_root.insert(key, item.clone());
                }
            }

            // Copy model_providers table from provider
            if let Some(mp) = provider_root.get("model_providers") {
                live_root.insert("model_providers", mp.clone());
            }
        }
    }

    // Write using atomic write
    crate::codex_config::write_codex_live_atomic(auth, Some(&live_doc.to_string()))?;
    Ok(())
}

/// Gemini: merge only key env fields, preserve settings.json (MCP etc.)
fn write_gemini_live_partial(provider: &Provider) -> Result<(), AppError> {
    use crate::gemini_config::{get_gemini_env_path, read_gemini_env, write_gemini_env_atomic};

    let auth_type = detect_gemini_auth_type(provider);

    // 1. Read existing env from live .env file
    let mut env_map = if get_gemini_env_path().exists() {
        read_gemini_env().unwrap_or_default()
    } else {
        HashMap::new()
    };

    // 2. Remove key fields from existing env
    for key in GEMINI_KEY_ENV_FIELDS {
        env_map.remove(*key);
    }

    // 3. Extract key fields from provider and merge
    if let Some(provider_env) = provider
        .settings_config
        .get("env")
        .and_then(|v| v.as_object())
    {
        for key in GEMINI_KEY_ENV_FIELDS {
            if let Some(value) = provider_env.get(*key).and_then(|v| v.as_str()) {
                if !value.is_empty() {
                    env_map.insert(key.to_string(), value.to_string());
                }
            }
        }
    }

    // 4. Handle auth type specific behavior
    match auth_type {
        GeminiAuthType::GoogleOfficial => {
            // Google official uses OAuth, clear all env
            env_map.clear();
            write_gemini_env_atomic(&env_map)?;
        }
        GeminiAuthType::Packycode | GeminiAuthType::Generic => {
            // Validate and write env
            crate::gemini_config::validate_gemini_settings_strict(&provider.settings_config)?;
            write_gemini_env_atomic(&env_map)?;
        }
    }

    // 5. Handle settings.json (same as write_gemini_live — preserve existing MCP etc.)
    use crate::gemini_config::get_gemini_settings_path;
    let settings_path = get_gemini_settings_path();

    if let Some(config_value) = provider.settings_config.get("config") {
        if config_value.is_object() {
            let mut merged = if settings_path.exists() {
                read_json_file::<Value>(&settings_path).unwrap_or_else(|_| json!({}))
            } else {
                json!({})
            };
            if let (Some(merged_obj), Some(config_obj)) =
                (merged.as_object_mut(), config_value.as_object())
            {
                for (k, v) in config_obj {
                    merged_obj.insert(k.clone(), v.clone());
                }
            }
            write_json_file(&settings_path, &merged)?;
        } else if !config_value.is_null() {
            return Err(AppError::localized(
                "gemini.validation.invalid_config",
                "Gemini 配置格式错误: config 必须是对象或 null",
                "Gemini config invalid: config must be an object or null",
            ));
        }
    }

    // 6. Set security flag based on auth type
    match auth_type {
        GeminiAuthType::GoogleOfficial => ensure_google_oauth_security_flag(provider)?,
        GeminiAuthType::Packycode | GeminiAuthType::Generic => {
            crate::gemini_config::write_packycode_settings()?;
        }
    }

    Ok(())
}

// ============================================================================
// Backfill: extract only key fields from live config
// ============================================================================

/// Extract only provider-specific key fields from a live config value.
///
/// Used during backfill to ensure the provider's `settings_config` converges
/// to containing only key fields over time.
pub(crate) fn backfill_key_fields(app_type: &AppType, live_config: &Value) -> Value {
    match app_type {
        AppType::Claude => backfill_claude_key_fields(live_config),
        AppType::Codex => backfill_codex_key_fields(live_config),
        AppType::Gemini => backfill_gemini_key_fields(live_config),
        // Additive mode: return full config (no backfill needed)
        _ => live_config.clone(),
    }
}

fn backfill_claude_key_fields(live: &Value) -> Value {
    let mut result = json!({});
    let result_obj = result.as_object_mut().unwrap();

    // Extract key env fields
    if let Some(live_env) = live.get("env").and_then(|v| v.as_object()) {
        let mut env_obj = serde_json::Map::new();
        for key in CLAUDE_KEY_ENV_FIELDS {
            if let Some(value) = live_env.get(*key) {
                env_obj.insert(key.to_string(), value.clone());
            }
        }
        if !env_obj.is_empty() {
            result_obj.insert("env".to_string(), Value::Object(env_obj));
        }
    }

    // Extract key top-level fields
    if let Some(live_obj) = live.as_object() {
        for key in CLAUDE_KEY_TOP_LEVEL {
            if let Some(value) = live_obj.get(*key) {
                result_obj.insert(key.to_string(), value.clone());
            }
        }
    }

    result
}

fn backfill_codex_key_fields(live: &Value) -> Value {
    let mut result = json!({});
    let result_obj = result.as_object_mut().unwrap();

    // auth is entirely provider-specific — keep it as-is
    if let Some(auth) = live.get("auth") {
        result_obj.insert("auth".to_string(), auth.clone());
    }

    // Extract key TOML fields from config string
    if let Some(config_str) = live.get("config").and_then(|v| v.as_str()) {
        if let Ok(doc) = config_str.parse::<toml_edit::DocumentMut>() {
            let mut new_doc = toml_edit::DocumentMut::new();
            let new_root = new_doc.as_table_mut();

            // Copy key top-level fields
            for key in CODEX_KEY_TOP_LEVEL {
                if let Some(item) = doc.as_table().get(key) {
                    new_root.insert(key, item.clone());
                }
            }

            // Copy model_providers table
            if let Some(mp) = doc.as_table().get("model_providers") {
                new_root.insert("model_providers", mp.clone());
            }

            let toml_str = new_doc.to_string();
            if !toml_str.trim().is_empty() {
                result_obj.insert("config".to_string(), Value::String(toml_str));
            }
        }
    }

    result
}

fn backfill_gemini_key_fields(live: &Value) -> Value {
    let mut result = json!({});
    let result_obj = result.as_object_mut().unwrap();

    // Extract key env fields
    if let Some(live_env) = live.get("env").and_then(|v| v.as_object()) {
        let mut env_obj = serde_json::Map::new();
        for key in GEMINI_KEY_ENV_FIELDS {
            if let Some(value) = live_env.get(*key) {
                env_obj.insert(key.to_string(), value.clone());
            }
        }
        if !env_obj.is_empty() {
            result_obj.insert("env".to_string(), Value::Object(env_obj));
        }
    }

    result
}

/// Sync all providers to live configuration (for additive mode apps)
///
/// Writes all providers from the database to the live configuration file.
/// Used for OpenCode and other additive mode applications.
fn sync_all_providers_to_live(state: &AppState, app_type: &AppType) -> Result<(), AppError> {
    let providers = state.db.get_all_providers(app_type.as_str())?;

    for provider in providers.values() {
        if let Err(e) = write_live_snapshot(app_type, provider) {
            log::warn!(
                "Failed to sync {:?} provider '{}' to live: {e}",
                app_type,
                provider.id
            );
            // Continue syncing other providers, don't abort
        }
    }

    log::info!(
        "Synced {} {:?} providers to live config",
        providers.len(),
        app_type
    );
    Ok(())
}

/// Sync current provider to live configuration
///
/// 使用有效的当前供应商 ID（验证过存在性）。
/// 优先从本地 settings 读取，验证后 fallback 到数据库的 is_current 字段。
/// 这确保了配置导入后无效 ID 会自动 fallback 到数据库。
///
/// For additive mode apps (OpenCode), all providers are synced instead of just the current one.
pub fn sync_current_to_live(state: &AppState) -> Result<(), AppError> {
    // Sync providers based on mode
    for app_type in AppType::all() {
        if app_type.is_additive_mode() {
            // Additive mode: sync ALL providers
            sync_all_providers_to_live(state, &app_type)?;
        } else {
            // Switch mode: sync only current provider
            let current_id =
                match crate::settings::get_effective_current_provider(&state.db, &app_type)? {
                    Some(id) => id,
                    None => continue,
                };

            let providers = state.db.get_all_providers(app_type.as_str())?;
            if let Some(provider) = providers.get(&current_id) {
                write_live_partial(&app_type, provider)?;
            }
            // Note: get_effective_current_provider already validates existence,
            // so providers.get() should always succeed here
        }
    }

    // MCP sync
    McpService::sync_all_enabled(state)?;

    // Skill sync
    for app_type in AppType::all() {
        if let Err(e) = crate::services::skill::SkillService::sync_to_app(&state.db, &app_type) {
            log::warn!("同步 Skill 到 {app_type:?} 失败: {e}");
            // Continue syncing other apps, don't abort
        }
    }

    Ok(())
}

/// Read current live settings for an app type
pub fn read_live_settings(app_type: AppType) -> Result<Value, AppError> {
    match app_type {
        AppType::Codex => {
            let auth_path = get_codex_auth_path();
            if !auth_path.exists() {
                return Err(AppError::localized(
                    "codex.auth.missing",
                    "Codex 配置文件不存在：缺少 auth.json",
                    "Codex configuration missing: auth.json not found",
                ));
            }
            let auth: Value = read_json_file(&auth_path)?;
            let cfg_text = crate::codex_config::read_and_validate_codex_config_text()?;
            Ok(json!({ "auth": auth, "config": cfg_text }))
        }
        AppType::Claude => {
            let path = get_claude_settings_path();
            if !path.exists() {
                return Err(AppError::localized(
                    "claude.live.missing",
                    "Claude Code 配置文件不存在",
                    "Claude settings file is missing",
                ));
            }
            read_json_file(&path)
        }
        AppType::Gemini => {
            use crate::gemini_config::{
                env_to_json, get_gemini_env_path, get_gemini_settings_path, read_gemini_env,
            };

            // Read .env file (environment variables)
            let env_path = get_gemini_env_path();
            if !env_path.exists() {
                return Err(AppError::localized(
                    "gemini.env.missing",
                    "Gemini .env 文件不存在",
                    "Gemini .env file not found",
                ));
            }

            let env_map = read_gemini_env()?;
            let env_json = env_to_json(&env_map);
            let env_obj = env_json.get("env").cloned().unwrap_or_else(|| json!({}));

            // Read settings.json file (MCP config etc.)
            let settings_path = get_gemini_settings_path();
            let config_obj = if settings_path.exists() {
                read_json_file(&settings_path)?
            } else {
                json!({})
            };

            // Return complete structure: { "env": {...}, "config": {...} }
            Ok(json!({
                "env": env_obj,
                "config": config_obj
            }))
        }
        AppType::OpenCode => {
            use crate::opencode_config::{get_opencode_config_path, read_opencode_config};

            let config_path = get_opencode_config_path();
            if !config_path.exists() {
                return Err(AppError::localized(
                    "opencode.config.missing",
                    "OpenCode 配置文件不存在",
                    "OpenCode configuration file not found",
                ));
            }

            let config = read_opencode_config()?;
            Ok(config)
        }
        AppType::OpenClaw => {
            use crate::openclaw_config::{get_openclaw_config_path, read_openclaw_config};

            let config_path = get_openclaw_config_path();
            if !config_path.exists() {
                return Err(AppError::localized(
                    "openclaw.config.missing",
                    "OpenClaw 配置文件不存在",
                    "OpenClaw configuration file not found",
                ));
            }

            let config = read_openclaw_config()?;
            Ok(config)
        }
    }
}

/// Import default configuration from live files
///
/// Returns `Ok(true)` if a provider was actually imported,
/// `Ok(false)` if skipped (providers already exist for this app).
pub fn import_default_config(state: &AppState, app_type: AppType) -> Result<bool, AppError> {
    // Additive mode apps (OpenCode, OpenClaw) should use their dedicated
    // import_xxx_providers_from_live functions, not this generic default config import
    if app_type.is_additive_mode() {
        return Ok(false);
    }

    {
        let providers = state.db.get_all_providers(app_type.as_str())?;
        if !providers.is_empty() {
            return Ok(false); // 已有供应商，跳过
        }
    }

    let settings_config = match app_type {
        AppType::Codex => {
            let auth_path = get_codex_auth_path();
            if !auth_path.exists() {
                return Err(AppError::localized(
                    "codex.live.missing",
                    "Codex 配置文件不存在",
                    "Codex configuration file is missing",
                ));
            }
            let auth: Value = read_json_file(&auth_path)?;
            let config_str = crate::codex_config::read_and_validate_codex_config_text()?;
            json!({ "auth": auth, "config": config_str })
        }
        AppType::Claude => {
            let settings_path = get_claude_settings_path();
            if !settings_path.exists() {
                return Err(AppError::localized(
                    "claude.live.missing",
                    "Claude Code 配置文件不存在",
                    "Claude settings file is missing",
                ));
            }
            let mut v = read_json_file::<Value>(&settings_path)?;
            let _ = normalize_claude_models_in_value(&mut v);
            v
        }
        AppType::Gemini => {
            use crate::gemini_config::{
                env_to_json, get_gemini_env_path, get_gemini_settings_path, read_gemini_env,
            };

            // Read .env file (environment variables)
            let env_path = get_gemini_env_path();
            if !env_path.exists() {
                return Err(AppError::localized(
                    "gemini.live.missing",
                    "Gemini 配置文件不存在",
                    "Gemini configuration file is missing",
                ));
            }

            let env_map = read_gemini_env()?;
            let env_json = env_to_json(&env_map);
            let env_obj = env_json.get("env").cloned().unwrap_or_else(|| json!({}));

            // Read settings.json file (MCP config etc.)
            let settings_path = get_gemini_settings_path();
            let config_obj = if settings_path.exists() {
                read_json_file(&settings_path)?
            } else {
                json!({})
            };

            // Return complete structure: { "env": {...}, "config": {...} }
            json!({
                "env": env_obj,
                "config": config_obj
            })
        }
        // OpenCode and OpenClaw use additive mode and are handled by early return above
        AppType::OpenCode | AppType::OpenClaw => {
            unreachable!("additive mode apps are handled by early return")
        }
    };

    let mut provider = Provider::with_id(
        "default".to_string(),
        "default".to_string(),
        settings_config,
        None,
    );
    provider.category = Some("custom".to_string());

    state.db.save_provider(app_type.as_str(), &provider)?;
    state
        .db
        .set_current_provider(app_type.as_str(), &provider.id)?;

    Ok(true) // 真正导入了
}

/// Write Gemini live configuration with authentication handling
pub(crate) fn write_gemini_live(provider: &Provider) -> Result<(), AppError> {
    use crate::gemini_config::{
        get_gemini_settings_path, json_to_env, validate_gemini_settings_strict,
        write_gemini_env_atomic,
    };

    // One-time auth type detection to avoid repeated detection
    let auth_type = detect_gemini_auth_type(provider);

    let mut env_map = json_to_env(&provider.settings_config)?;

    // Prepare config to write to ~/.gemini/settings.json
    // Behavior:
    // - config is object: use it (merge with existing to preserve mcpServers etc.)
    // - config is null or absent: preserve existing file content
    let settings_path = get_gemini_settings_path();
    let mut config_to_write: Option<Value> = None;

    if let Some(config_value) = provider.settings_config.get("config") {
        if config_value.is_object() {
            // Merge with existing settings to preserve mcpServers and other fields
            let mut merged = if settings_path.exists() {
                read_json_file::<Value>(&settings_path).unwrap_or_else(|_| json!({}))
            } else {
                json!({})
            };

            // Merge provider config into existing settings
            if let (Some(merged_obj), Some(config_obj)) =
                (merged.as_object_mut(), config_value.as_object())
            {
                for (k, v) in config_obj {
                    merged_obj.insert(k.clone(), v.clone());
                }
            }
            config_to_write = Some(merged);
        } else if !config_value.is_null() {
            return Err(AppError::localized(
                "gemini.validation.invalid_config",
                "Gemini 配置格式错误: config 必须是对象或 null",
                "Gemini config invalid: config must be an object or null",
            ));
        }
        // config is null: don't modify existing settings.json (preserve mcpServers etc.)
    }

    // If no config specified or config is null, preserve existing file
    if config_to_write.is_none() && settings_path.exists() {
        config_to_write = Some(read_json_file(&settings_path)?);
    }

    match auth_type {
        GeminiAuthType::GoogleOfficial => {
            // Google official uses OAuth, clear env
            env_map.clear();
            write_gemini_env_atomic(&env_map)?;
        }
        GeminiAuthType::Packycode => {
            // PackyCode provider, uses API Key (strict validation on switch)
            validate_gemini_settings_strict(&provider.settings_config)?;
            write_gemini_env_atomic(&env_map)?;
        }
        GeminiAuthType::Generic => {
            // Generic provider, uses API Key (strict validation on switch)
            validate_gemini_settings_strict(&provider.settings_config)?;
            write_gemini_env_atomic(&env_map)?;
        }
    }

    if let Some(config_value) = config_to_write {
        write_json_file(&settings_path, &config_value)?;
    }

    // Set security.auth.selectedType based on auth type
    // - Google Official: OAuth mode
    // - All others: API Key mode
    match auth_type {
        GeminiAuthType::GoogleOfficial => ensure_google_oauth_security_flag(provider)?,
        GeminiAuthType::Packycode | GeminiAuthType::Generic => {
            crate::gemini_config::write_packycode_settings()?;
        }
    }

    Ok(())
}

/// Remove an OpenCode provider from the live configuration
///
/// This is specific to OpenCode's additive mode - removing a provider
/// from the opencode.json file.
pub(crate) fn remove_opencode_provider_from_live(provider_id: &str) -> Result<(), AppError> {
    use crate::opencode_config;

    // Check if OpenCode config directory exists
    if !opencode_config::get_opencode_dir().exists() {
        log::debug!("OpenCode config directory doesn't exist, skipping removal of '{provider_id}'");
        return Ok(());
    }

    opencode_config::remove_provider(provider_id)?;
    log::info!("OpenCode provider '{provider_id}' removed from live config");

    Ok(())
}

/// Import all providers from OpenCode live config to database
///
/// This imports existing providers from ~/.config/opencode/opencode.json
/// into the CC Switch database. Each provider found will be added to the
/// database with is_current set to false.
pub fn import_opencode_providers_from_live(state: &AppState) -> Result<usize, AppError> {
    use crate::opencode_config;

    let providers = opencode_config::get_typed_providers()?;
    if providers.is_empty() {
        return Ok(0);
    }

    let mut imported = 0;
    let existing = state.db.get_all_providers("opencode")?;

    for (id, config) in providers {
        // Skip if already exists in database
        if existing.contains_key(&id) {
            log::debug!("OpenCode provider '{id}' already exists in database, skipping");
            continue;
        }

        // Convert to Value for settings_config
        let settings_config = match serde_json::to_value(&config) {
            Ok(v) => v,
            Err(e) => {
                log::warn!("Failed to serialize OpenCode provider '{id}': {e}");
                continue;
            }
        };

        // Create provider
        let provider = Provider::with_id(
            id.clone(),
            config.name.clone().unwrap_or_else(|| id.clone()),
            settings_config,
            None,
        );

        // Save to database
        if let Err(e) = state.db.save_provider("opencode", &provider) {
            log::warn!("Failed to import OpenCode provider '{id}': {e}");
            continue;
        }

        imported += 1;
        log::info!("Imported OpenCode provider '{id}' from live config");
    }

    Ok(imported)
}

/// Import all providers from OpenClaw live config to database
///
/// This imports existing providers from ~/.openclaw/openclaw.json
/// into the CC Switch database. Each provider found will be added to the
/// database with is_current set to false.
pub fn import_openclaw_providers_from_live(state: &AppState) -> Result<usize, AppError> {
    use crate::openclaw_config;

    let providers = openclaw_config::get_typed_providers()?;
    if providers.is_empty() {
        return Ok(0);
    }

    let mut imported = 0;
    let existing = state.db.get_all_providers("openclaw")?;

    for (id, config) in providers {
        // Validate: skip entries with empty id or no models
        if id.trim().is_empty() {
            log::warn!("Skipping OpenClaw provider with empty id");
            continue;
        }
        if config.models.is_empty() {
            log::warn!("Skipping OpenClaw provider '{id}': no models defined");
            continue;
        }

        // Skip if already exists in database
        if existing.contains_key(&id) {
            log::debug!("OpenClaw provider '{id}' already exists in database, skipping");
            continue;
        }

        // Convert to Value for settings_config
        let settings_config = match serde_json::to_value(&config) {
            Ok(v) => v,
            Err(e) => {
                log::warn!("Failed to serialize OpenClaw provider '{id}': {e}");
                continue;
            }
        };

        // Determine display name: use first model name if available, otherwise use id
        let display_name = config
            .models
            .first()
            .and_then(|m| m.name.clone())
            .unwrap_or_else(|| id.clone());

        // Create provider
        let provider = Provider::with_id(id.clone(), display_name, settings_config, None);

        // Save to database
        if let Err(e) = state.db.save_provider("openclaw", &provider) {
            log::warn!("Failed to import OpenClaw provider '{id}': {e}");
            continue;
        }

        imported += 1;
        log::info!("Imported OpenClaw provider '{id}' from live config");
    }

    Ok(imported)
}

/// Remove an OpenClaw provider from live config
///
/// This removes a specific provider from ~/.openclaw/openclaw.json
/// without affecting other providers in the file.
pub fn remove_openclaw_provider_from_live(provider_id: &str) -> Result<(), AppError> {
    use crate::openclaw_config;

    // Check if OpenClaw config directory exists
    if !openclaw_config::get_openclaw_dir().exists() {
        log::debug!("OpenClaw config directory doesn't exist, skipping removal of '{provider_id}'");
        return Ok(());
    }

    openclaw_config::remove_provider(provider_id)?;
    log::info!("OpenClaw provider '{provider_id}' removed from live config");

    Ok(())
}
