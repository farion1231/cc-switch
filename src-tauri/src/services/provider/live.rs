//! Live configuration operations
//!
//! Handles reading and writing live configuration files for Claude, Codex, and Gemini.

use std::collections::HashMap;

use serde_json::{json, Value};
use toml_edit::{DocumentMut, Item, TableLike};

use crate::app_config::AppType;
use crate::codex_config::{get_codex_auth_path, get_codex_config_path};
use crate::config::{delete_file, get_claude_settings_path, read_json_file, write_json_file};
use crate::database::Database;
use crate::error::AppError;
use crate::provider::Provider;
use crate::services::mcp::McpService;
use crate::store::AppState;

use super::gemini_auth::{
    detect_gemini_auth_type, ensure_google_oauth_security_flag, GeminiAuthType,
};
use super::normalize_claude_models_in_value;

/// ChatGPT Codex catalogs gpt-5.6 at a 372K context window with a ~353K
/// effective budget (openai/codex#31860), far below the 1.05M API spec.
/// Declare the catalog window for both knobs: Claude Code's built-in output
/// reserve and compact buffer already keep the actual compact trigger
/// (~278K-339K) below the effective budget, so anything lower only wastes
/// usable context.
const CODEX_OAUTH_CLAUDE_MAX_CONTEXT_TOKENS: &str = "372000";
const CODEX_OAUTH_CLAUDE_AUTO_COMPACT_WINDOW: &str = "372000";
const KIMI_FOR_CODING_CONTEXT_TOKENS: &str = "262144";

/// Model env keys Claude Code may route requests through. The defaults above
/// are calibrated against gpt-5.6's Codex catalog, so every configured model
/// must belong to that family before they are injected — gpt-5.5's upstream
/// catalog oscillates between 272K and 372K and must not inherit them.
const CODEX_OAUTH_MODEL_ENV_KEYS: [&str; 6] = [
    "ANTHROPIC_MODEL",
    "ANTHROPIC_DEFAULT_HAIKU_MODEL",
    "ANTHROPIC_DEFAULT_SONNET_MODEL",
    "ANTHROPIC_DEFAULT_OPUS_MODEL",
    "ANTHROPIC_DEFAULT_FABLE_MODEL",
    "CLAUDE_CODE_SUBAGENT_MODEL",
];

fn provider_env_targets_gpt56(provider_env: Option<&serde_json::Map<String, Value>>) -> bool {
    let Some(env) = provider_env else {
        return false;
    };
    let mut saw_model = false;
    for key in CODEX_OAUTH_MODEL_ENV_KEYS {
        let Some(value) = env.get(key) else {
            continue;
        };
        let Some(model) = value.as_str() else {
            return false;
        };
        let model = model.trim();
        if model.is_empty() {
            continue;
        }
        saw_model = true;
        if !model.to_ascii_lowercase().starts_with("gpt-5.6") {
            return false;
        }
    }
    saw_model
}

fn is_kimi_for_coding_provider(provider: &Provider) -> bool {
    provider
        .settings_config
        .pointer("/env/ANTHROPIC_BASE_URL")
        .and_then(Value::as_str)
        .map(str::trim)
        .map(|url| url.trim_end_matches('/'))
        == Some("https://api.kimi.com/coding")
}

/// Claude Code assigns unknown non-Claude model ids a 200K context window.
/// Codex OAuth deliberately exposes GPT ids through Claude Code, so enrich the
/// effective live settings for both newly-created and already-saved providers.
/// Explicit user values always win; the defaults are only injected when every
/// configured model targets gpt-5.6.
fn apply_codex_oauth_claude_context_defaults(settings: &mut Value, provider: &Provider) {
    if !provider.is_codex_oauth() {
        return;
    }

    // Read provider-owned values before mutably borrowing the effective
    // settings. This also deliberately prevents a legacy common-config
    // snippet from overriding model-specific context limits.
    let provider_env = provider
        .settings_config
        .get("env")
        .and_then(Value::as_object);
    let Some(root) = settings.as_object_mut() else {
        return;
    };
    let env = root.entry("env".to_string()).or_insert_with(|| json!({}));
    let Some(env) = env.as_object_mut() else {
        log::warn!(
            "Cannot apply Codex OAuth Claude context defaults for '{}': env is not an object",
            provider.id
        );
        return;
    };

    let inject_defaults = provider_env_targets_gpt56(provider_env);
    for (key, default_value) in [
        (
            "CLAUDE_CODE_MAX_CONTEXT_TOKENS",
            CODEX_OAUTH_CLAUDE_MAX_CONTEXT_TOKENS,
        ),
        (
            "CLAUDE_CODE_AUTO_COMPACT_WINDOW",
            CODEX_OAUTH_CLAUDE_AUTO_COMPACT_WINDOW,
        ),
    ] {
        match provider_env.and_then(|provider_env| provider_env.get(key)) {
            Some(value) => {
                env.insert(key.to_string(), value.clone());
            }
            None if inject_defaults => {
                env.insert(key.to_string(), Value::String(default_value.to_string()));
            }
            // 老模型不注入默认值，同时剥掉遗留共享片段可能带进来的值
            None => {
                env.remove(key);
            }
        }
    }
}

/// Kimi For Coding serves a 256K window, but Claude Code caps unknown models at
/// 200K unless `CLAUDE_CODE_MAX_CONTEXT_TOKENS` is set — and that env is ignored
/// for `claude-`-prefixed ids, so these defaults only bite when the provider also
/// routes the endpoint's `kimi-for-coding` alias (the preset does). Keep the
/// defaults provider-owned so an old shared snippet cannot override them.
fn apply_kimi_for_coding_context_defaults(settings: &mut Value, provider: &Provider) {
    if !is_kimi_for_coding_provider(provider) {
        return;
    }

    let provider_env = provider
        .settings_config
        .get("env")
        .and_then(Value::as_object);
    let Some(env) = settings.get_mut("env").and_then(Value::as_object_mut) else {
        return;
    };

    for key in [
        "CLAUDE_CODE_MAX_CONTEXT_TOKENS",
        "CLAUDE_CODE_AUTO_COMPACT_WINDOW",
    ] {
        let value = provider_env
            .and_then(|provider_env| provider_env.get(key))
            .cloned()
            .unwrap_or_else(|| Value::String(KIMI_FOR_CODING_CONTEXT_TOKENS.to_string()));
        env.insert(key.to_string(), value);
    }
}

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

pub(crate) fn provider_exists_in_live_config(
    app_type: &AppType,
    provider_id: &str,
) -> Result<bool, AppError> {
    match app_type {
        AppType::OpenCode => crate::opencode_config::get_providers()
            .map(|providers| providers.contains_key(provider_id)),
        AppType::OpenClaw => crate::openclaw_config::get_providers()
            .map(|providers| providers.contains_key(provider_id)),
        AppType::Hermes => crate::hermes_config::get_providers()
            .map(|providers| providers.contains_key(provider_id)),
        _ => Ok(false),
    }
}

fn json_is_subset(target: &Value, source: &Value) -> bool {
    match source {
        Value::Object(source_map) => {
            let Some(target_map) = target.as_object() else {
                return false;
            };
            source_map.iter().all(|(key, source_value)| {
                target_map
                    .get(key)
                    .is_some_and(|target_value| json_is_subset(target_value, source_value))
            })
        }
        Value::Array(source_arr) => {
            let Some(target_arr) = target.as_array() else {
                return false;
            };
            json_array_contains_subset(target_arr, source_arr)
        }
        _ => target == source,
    }
}

fn json_array_contains_subset(target_arr: &[Value], source_arr: &[Value]) -> bool {
    let mut matched = vec![false; target_arr.len()];

    source_arr.iter().all(|source_item| {
        if let Some((index, _)) = target_arr.iter().enumerate().find(|(index, target_item)| {
            !matched[*index] && json_is_subset(target_item, source_item)
        }) {
            matched[index] = true;
            true
        } else {
            false
        }
    })
}

fn json_remove_array_items(target_arr: &mut Vec<Value>, source_arr: &[Value]) {
    for source_item in source_arr {
        if let Some(index) = target_arr
            .iter()
            .position(|target_item| json_is_subset(target_item, source_item))
        {
            target_arr.remove(index);
        }
    }
}

fn json_deep_merge(target: &mut Value, source: &Value) {
    match (target, source) {
        (Value::Object(target_map), Value::Object(source_map)) => {
            for (key, source_value) in source_map {
                match target_map.get_mut(key) {
                    Some(target_value) => json_deep_merge(target_value, source_value),
                    None => {
                        target_map.insert(key.clone(), source_value.clone());
                    }
                }
            }
        }
        (target_value, source_value) => {
            *target_value = source_value.clone();
        }
    }
}

fn json_deep_remove(target: &mut Value, source: &Value) {
    let (Some(target_map), Some(source_map)) = (target.as_object_mut(), source.as_object()) else {
        return;
    };

    for (key, source_value) in source_map {
        let mut remove_key = false;

        if let Some(target_value) = target_map.get_mut(key) {
            if source_value.is_object() && target_value.is_object() {
                json_deep_remove(target_value, source_value);
                remove_key = target_value.as_object().is_some_and(|obj| obj.is_empty());
            } else if let (Some(target_arr), Some(source_arr)) =
                (target_value.as_array_mut(), source_value.as_array())
            {
                json_remove_array_items(target_arr, source_arr);
                remove_key = target_arr.is_empty();
            } else if json_is_subset(target_value, source_value) {
                remove_key = true;
            }
        }

        if remove_key {
            target_map.remove(key);
        }
    }
}

fn toml_value_is_subset(target: &toml_edit::Value, source: &toml_edit::Value) -> bool {
    match (target, source) {
        (toml_edit::Value::String(target), toml_edit::Value::String(source)) => {
            target.value() == source.value()
        }
        (toml_edit::Value::Integer(target), toml_edit::Value::Integer(source)) => {
            target.value() == source.value()
        }
        (toml_edit::Value::Float(target), toml_edit::Value::Float(source)) => {
            target.value() == source.value()
        }
        (toml_edit::Value::Boolean(target), toml_edit::Value::Boolean(source)) => {
            target.value() == source.value()
        }
        (toml_edit::Value::Datetime(target), toml_edit::Value::Datetime(source)) => {
            target.value() == source.value()
        }
        (toml_edit::Value::Array(target), toml_edit::Value::Array(source)) => {
            toml_array_contains_subset(target, source)
        }
        (toml_edit::Value::InlineTable(target), toml_edit::Value::InlineTable(source)) => {
            source.iter().all(|(key, source_item)| {
                target
                    .get(key)
                    .is_some_and(|target_item| toml_value_is_subset(target_item, source_item))
            })
        }
        _ => false,
    }
}

fn toml_array_contains_subset(target: &toml_edit::Array, source: &toml_edit::Array) -> bool {
    let mut matched = vec![false; target.len()];
    let target_items: Vec<&toml_edit::Value> = target.iter().collect();

    source.iter().all(|source_item| {
        if let Some((index, _)) = target_items
            .iter()
            .enumerate()
            .find(|(index, target_item)| {
                !matched[*index] && toml_value_is_subset(target_item, source_item)
            })
        {
            matched[index] = true;
            true
        } else {
            false
        }
    })
}

fn toml_remove_array_items(target: &mut toml_edit::Array, source: &toml_edit::Array) {
    for source_item in source.iter() {
        let index = {
            let target_items: Vec<&toml_edit::Value> = target.iter().collect();
            target_items
                .iter()
                .enumerate()
                .find(|(_, target_item)| toml_value_is_subset(target_item, source_item))
                .map(|(index, _)| index)
        };

        if let Some(index) = index {
            target.remove(index);
        }
    }
}

fn toml_item_is_subset(target: &Item, source: &Item) -> bool {
    if let Some(source_table) = source.as_table_like() {
        let Some(target_table) = target.as_table_like() else {
            return false;
        };
        return source_table.iter().all(|(key, source_item)| {
            target_table
                .get(key)
                .is_some_and(|target_item| toml_item_is_subset(target_item, source_item))
        });
    }

    match (target.as_value(), source.as_value()) {
        (Some(target_value), Some(source_value)) => {
            toml_value_is_subset(target_value, source_value)
        }
        _ => false,
    }
}

fn merge_toml_item(target: &mut Item, source: &Item) {
    if let Some(source_table) = source.as_table_like() {
        if let Some(target_table) = target.as_table_like_mut() {
            merge_toml_table_like(target_table, source_table);
            return;
        }
    }

    *target = source.clone();
}

fn merge_toml_table_like(target: &mut dyn TableLike, source: &dyn TableLike) {
    for (key, source_item) in source.iter() {
        match target.get_mut(key) {
            Some(target_item) => merge_toml_item(target_item, source_item),
            None => {
                target.insert(key, source_item.clone());
            }
        }
    }
}

fn remove_toml_item(target: &mut Item, source: &Item) {
    if let Some(source_table) = source.as_table_like() {
        if let Some(target_table) = target.as_table_like_mut() {
            remove_toml_table_like(target_table, source_table);
            if target_table.is_empty() {
                *target = Item::None;
            }
            return;
        }
    }

    if let Some(source_value) = source.as_value() {
        let mut remove_item = false;

        if let Some(target_value) = target.as_value_mut() {
            match (target_value, source_value) {
                (toml_edit::Value::Array(target_arr), toml_edit::Value::Array(source_arr)) => {
                    toml_remove_array_items(target_arr, source_arr);
                    remove_item = target_arr.is_empty();
                }
                (target_value, source_value)
                    if toml_value_is_subset(target_value, source_value) =>
                {
                    remove_item = true;
                }
                _ => {}
            }
        }

        if remove_item {
            *target = Item::None;
        }
    }
}

fn remove_toml_table_like(target: &mut dyn TableLike, source: &dyn TableLike) {
    let keys: Vec<String> = source.iter().map(|(key, _)| key.to_string()).collect();

    for key in keys {
        let mut remove_key = false;
        if let (Some(target_item), Some(source_item)) = (target.get_mut(&key), source.get(&key)) {
            remove_toml_item(target_item, source_item);
            remove_key = target_item.is_none()
                || target_item
                    .as_table_like()
                    .is_some_and(|table_like| table_like.is_empty());
        }

        if remove_key {
            target.remove(&key);
        }
    }
}

/// 前端表单勾选/取消"使用通用配置"时，对编辑器里的 config.toml 文本做
/// 结构化合并/剥离。必须在后端用 toml_edit 做：前端 smol-toml 只能
/// parse → merge → 整文档重序列化，注释全丢、键序重排，还会生成多余的
/// 空父表头（如 `[model_providers]`）。
pub fn update_toml_common_config_snippet(
    config_toml: &str,
    snippet_toml: &str,
    enabled: bool,
) -> Result<String, AppError> {
    let trimmed = snippet_toml.trim();
    if trimmed.is_empty() {
        return Ok(config_toml.to_string());
    }

    let mut target_doc = if config_toml.trim().is_empty() {
        DocumentMut::new()
    } else {
        config_toml
            .parse::<DocumentMut>()
            .map_err(|e| AppError::Message(format!("Invalid Codex config.toml: {e}")))?
    };
    let source_doc = trimmed
        .parse::<DocumentMut>()
        .map_err(|e| AppError::Message(format!("Invalid Codex common config snippet: {e}")))?;

    if enabled {
        merge_toml_table_like(target_doc.as_table_mut(), source_doc.as_table());
    } else {
        remove_toml_table_like(target_doc.as_table_mut(), source_doc.as_table());
    }

    Ok(target_doc.to_string())
}

fn settings_contain_common_config(app_type: &AppType, settings: &Value, snippet: &str) -> bool {
    let trimmed = snippet.trim();
    if trimmed.is_empty() {
        return false;
    }

    match app_type {
        AppType::Claude => match serde_json::from_str::<Value>(trimmed) {
            Ok(source) if source.is_object() => json_is_subset(settings, &source),
            _ => false,
        },
        AppType::Codex => {
            let config_toml = settings.get("config").and_then(Value::as_str).unwrap_or("");
            if config_toml.trim().is_empty() {
                return false;
            }

            let target_doc = match config_toml.parse::<DocumentMut>() {
                Ok(doc) => doc,
                Err(_) => return false,
            };
            let source_doc = match trimmed.parse::<DocumentMut>() {
                Ok(doc) => doc,
                Err(_) => return false,
            };

            toml_item_is_subset(target_doc.as_item(), source_doc.as_item())
        }
        AppType::Gemini => match serde_json::from_str::<Value>(trimmed) {
            Ok(Value::Object(source_map)) => {
                let Some(target_map) = settings.get("env").and_then(Value::as_object) else {
                    return false;
                };
                source_map.iter().all(|(key, source_value)| {
                    target_map
                        .get(key)
                        .is_some_and(|target_value| json_is_subset(target_value, source_value))
                })
            }
            _ => false,
        },
        AppType::GrokBuild
        | AppType::OpenCode
        | AppType::OpenClaw
        | AppType::Hermes
        | AppType::ClaudeDesktop => false,
    }
}

pub(crate) fn provider_uses_common_config(
    app_type: &AppType,
    provider: &Provider,
    snippet: Option<&str>,
) -> bool {
    match provider
        .meta
        .as_ref()
        .and_then(|meta| meta.common_config_enabled)
    {
        Some(explicit) => explicit && snippet.is_some_and(|value| !value.trim().is_empty()),
        None => snippet.is_some_and(|value| {
            settings_contain_common_config(app_type, &provider.settings_config, value)
        }),
    }
}

pub(crate) fn remove_common_config_from_settings(
    app_type: &AppType,
    settings: &Value,
    snippet: &str,
) -> Result<Value, AppError> {
    let trimmed = snippet.trim();
    if trimmed.is_empty() {
        return Ok(settings.clone());
    }

    match app_type {
        AppType::Claude => {
            let source = serde_json::from_str::<Value>(trimmed)
                .map_err(|e| AppError::Message(format!("Invalid Claude common config: {e}")))?;
            let mut result = settings.clone();
            json_deep_remove(&mut result, &source);
            Ok(result)
        }
        AppType::Codex => {
            let mut result = settings.clone();
            let config_toml = settings.get("config").and_then(Value::as_str).unwrap_or("");
            let mut target_doc = if config_toml.trim().is_empty() {
                DocumentMut::new()
            } else {
                config_toml.parse::<DocumentMut>().map_err(|e| {
                    AppError::Message(format!(
                        "Invalid Codex config.toml while removing common config: {e}"
                    ))
                })?
            };
            let source_doc = trimmed.parse::<DocumentMut>().map_err(|e| {
                AppError::Message(format!("Invalid Codex common config snippet: {e}"))
            })?;

            remove_toml_table_like(target_doc.as_table_mut(), source_doc.as_table());
            if let Some(obj) = result.as_object_mut() {
                obj.insert("config".to_string(), Value::String(target_doc.to_string()));
            }
            Ok(result)
        }
        AppType::Gemini => {
            let source = serde_json::from_str::<Value>(trimmed)
                .map_err(|e| AppError::Message(format!("Invalid Gemini common config: {e}")))?;
            let mut result = settings.clone();
            if let Some(env) = result.get_mut("env") {
                json_deep_remove(env, &source);
            }
            Ok(result)
        }
        AppType::GrokBuild
        | AppType::OpenCode
        | AppType::OpenClaw
        | AppType::Hermes
        | AppType::ClaudeDesktop => Ok(settings.clone()),
    }
}

fn apply_common_config_to_settings(
    app_type: &AppType,
    settings: &Value,
    snippet: &str,
) -> Result<Value, AppError> {
    let trimmed = snippet.trim();
    if trimmed.is_empty() {
        return Ok(settings.clone());
    }

    match app_type {
        AppType::Claude => {
            let source = serde_json::from_str::<Value>(trimmed)
                .map_err(|e| AppError::Message(format!("Invalid Claude common config: {e}")))?;
            let mut result = settings.clone();
            json_deep_merge(&mut result, &source);
            Ok(result)
        }
        AppType::Codex => {
            let mut result = settings.clone();
            let config_toml = settings.get("config").and_then(Value::as_str).unwrap_or("");
            let mut target_doc = if config_toml.trim().is_empty() {
                DocumentMut::new()
            } else {
                config_toml.parse::<DocumentMut>().map_err(|e| {
                    AppError::Message(format!(
                        "Invalid Codex config.toml while applying common config: {e}"
                    ))
                })?
            };
            let source_doc = trimmed.parse::<DocumentMut>().map_err(|e| {
                AppError::Message(format!("Invalid Codex common config snippet: {e}"))
            })?;

            merge_toml_table_like(target_doc.as_table_mut(), source_doc.as_table());
            if let Some(obj) = result.as_object_mut() {
                obj.insert("config".to_string(), Value::String(target_doc.to_string()));
            }
            Ok(result)
        }
        AppType::Gemini => {
            let source = serde_json::from_str::<Value>(trimmed)
                .map_err(|e| AppError::Message(format!("Invalid Gemini common config: {e}")))?;
            let mut result = settings.clone();
            if let Some(env) = result.get_mut("env") {
                json_deep_merge(env, &source);
            } else if let Some(obj) = result.as_object_mut() {
                obj.insert("env".to_string(), source);
            }
            Ok(result)
        }
        AppType::GrokBuild
        | AppType::OpenCode
        | AppType::OpenClaw
        | AppType::Hermes
        | AppType::ClaudeDesktop => Ok(settings.clone()),
    }
}

pub(crate) fn build_effective_settings_with_common_config(
    db: &Database,
    app_type: &AppType,
    provider: &Provider,
) -> Result<Value, AppError> {
    let snippet = db.get_config_snippet(app_type.as_str())?;
    let mut effective_settings = provider.settings_config.clone();

    if provider_uses_common_config(app_type, provider, snippet.as_deref()) {
        if let Some(snippet_text) = snippet.as_deref() {
            match apply_common_config_to_settings(app_type, &effective_settings, snippet_text) {
                Ok(settings) => effective_settings = settings,
                Err(err) => {
                    log::warn!(
                        "Failed to apply common config for {} provider '{}': {err}",
                        app_type.as_str(),
                        provider.id
                    );
                }
            }
        }
    }

    if matches!(app_type, AppType::Claude) {
        apply_codex_oauth_claude_context_defaults(&mut effective_settings, provider);
        apply_kimi_for_coding_context_defaults(&mut effective_settings, provider);
    }

    Ok(effective_settings)
}

pub(crate) fn write_live_with_common_config(
    db: &Database,
    app_type: &AppType,
    provider: &Provider,
) -> Result<(), AppError> {
    let mut effective_provider = provider.clone();
    effective_provider.settings_config =
        build_effective_settings_with_common_config(db, app_type, provider)?;

    if matches!(app_type, AppType::ClaudeDesktop) {
        crate::claude_desktop_config::apply_provider(db, &effective_provider)?;
        log::info!(
            "Claude Desktop 3P profile '{}' written for provider '{}'",
            crate::claude_desktop_config::PROFILE_ID,
            effective_provider.id
        );
        return Ok(());
    }

    write_live_snapshot(app_type, &effective_provider)
}

pub(crate) fn strip_common_config_from_live_settings(
    db: &Database,
    app_type: &AppType,
    provider: &Provider,
    live_settings: Value,
) -> Value {
    let snippet = match db.get_config_snippet(app_type.as_str()) {
        Ok(snippet) => snippet,
        Err(err) => {
            log::warn!(
                "Failed to load common config for {} while backfilling '{}': {err}",
                app_type.as_str(),
                provider.id
            );
            return restore_live_settings_for_provider_backfill(app_type, provider, live_settings);
        }
    };

    let backfill_settings = if provider_uses_common_config(app_type, provider, snippet.as_deref()) {
        match snippet.as_deref() {
            Some(snippet_text) => {
                match remove_common_config_from_settings(app_type, &live_settings, snippet_text) {
                    Ok(settings) => settings,
                    Err(err) => {
                        log::warn!(
                            "Failed to strip common config for {} provider '{}': {err}",
                            app_type.as_str(),
                            provider.id
                        );
                        live_settings
                    }
                }
            }
            None => live_settings,
        }
    } else {
        live_settings
    };

    restore_live_settings_for_provider_backfill(app_type, provider, backfill_settings)
}

/// 与 `apply_codex_oauth_claude_context_defaults` 严格对称：注入产物只活在
/// live，切走回填时必须剥掉，否则程序默认值会固化成供应商的"用户显式值"，
/// 之后调整默认值或更换模型时旧值永远压住新默认。仅当"注入会发生且注入的
/// 就是这个值、且存储配置本来没有显式值"时才剥；用户显式存储的值和手改
/// live 成其他数字的值都保留。
fn strip_injected_codex_oauth_context_defaults(settings: &mut Value, provider: &Provider) {
    if !provider.is_codex_oauth() {
        return;
    }
    let provider_env = provider
        .settings_config
        .get("env")
        .and_then(Value::as_object);
    if !provider_env_targets_gpt56(provider_env) {
        return;
    }
    let Some(env) = settings.get_mut("env").and_then(Value::as_object_mut) else {
        return;
    };
    for (key, default_value) in [
        (
            "CLAUDE_CODE_MAX_CONTEXT_TOKENS",
            CODEX_OAUTH_CLAUDE_MAX_CONTEXT_TOKENS,
        ),
        (
            "CLAUDE_CODE_AUTO_COMPACT_WINDOW",
            CODEX_OAUTH_CLAUDE_AUTO_COMPACT_WINDOW,
        ),
    ] {
        let stored_explicit = provider_env.is_some_and(|e| e.contains_key(key));
        if stored_explicit {
            continue;
        }
        if env.get(key).and_then(Value::as_str) == Some(default_value) {
            env.remove(key);
        }
    }
}

fn strip_injected_kimi_for_coding_context_defaults(settings: &mut Value, provider: &Provider) {
    if !is_kimi_for_coding_provider(provider) {
        return;
    }
    let provider_env = provider
        .settings_config
        .get("env")
        .and_then(Value::as_object);
    let Some(env) = settings.get_mut("env").and_then(Value::as_object_mut) else {
        return;
    };
    for key in [
        "CLAUDE_CODE_MAX_CONTEXT_TOKENS",
        "CLAUDE_CODE_AUTO_COMPACT_WINDOW",
    ] {
        if provider_env.is_some_and(|provider_env| provider_env.contains_key(key)) {
            continue;
        }
        if env.get(key).and_then(Value::as_str) == Some(KIMI_FOR_CODING_CONTEXT_TOKENS) {
            env.remove(key);
        }
    }
}

fn restore_live_settings_for_provider_backfill(
    app_type: &AppType,
    provider: &Provider,
    live_settings: Value,
) -> Value {
    if matches!(app_type, AppType::Claude) {
        let mut settings = live_settings;
        strip_injected_codex_oauth_context_defaults(&mut settings, provider);
        strip_injected_kimi_for_coding_context_defaults(&mut settings, provider);
        return settings;
    }
    if matches!(app_type, AppType::GrokBuild) {
        let mut settings = live_settings;
        if let Err(err) = crate::grok_config::strip_grok_mcp_servers_from_settings(&mut settings) {
            log::warn!(
                "Failed to strip Grok Build mcp_servers while backfilling '{}': {err}",
                provider.id
            );
        }
        return settings;
    }
    if !matches!(app_type, AppType::Codex) {
        return live_settings;
    }

    let mut settings = live_settings;
    let restore_provider_token =
        crate::codex_config::should_restore_codex_provider_token_for_backfill(
            provider.category.as_deref(),
            &provider.settings_config,
        );
    if let Err(err) = crate::codex_config::restore_codex_settings_for_backfill(
        &mut settings,
        &provider.settings_config,
        restore_provider_token,
    ) {
        log::warn!(
            "Failed to restore Codex settings while backfilling '{}': {err}",
            provider.id
        );
    }

    // MCP 服务器归 DB mcp_servers 表所有，live 里的 [mcp_servers] 是同步投影；
    // 回填时剥掉，否则已删除的服务器会随供应商快照复活（逐条 reconcile 清不掉孤儿）。
    if let Err(err) = crate::codex_config::strip_codex_mcp_servers_from_settings(&mut settings) {
        log::warn!(
            "Failed to strip mcp_servers while backfilling '{}': {err}",
            provider.id
        );
    }

    // 统一会话开关注入的共享 `custom` 路由只属于 live 配置；切换回填时
    // 必须剥掉，否则官方供应商的存储配置被污染，关闭开关后无法还原。
    if provider.category.as_deref() == Some("official") {
        if let Err(err) =
            crate::codex_config::strip_codex_unified_session_bucket_from_settings(&mut settings)
        {
            log::warn!(
                "Failed to strip unified session bucket while backfilling '{}': {err}",
                provider.id
            );
        }
    }

    // `modelCatalog` is a cc-switch–private field whose SSOT is the DB. Live's
    // `config.toml` only carries a lossy projection (`model_catalog_json` →
    // generated catalog file) that proxy takeover/restore cycles and Codex.app
    // config rewrites can drop, so `read_live_settings` may reconstruct it as
    // absent. Never let a switch-away backfill from Live erase the stored
    // mapping: prefer the DB provider's `modelCatalog`, falling back to whatever
    // Live reconstructed only when the DB has none.
    if let Some(stored_catalog) = provider.settings_config.get("modelCatalog") {
        if let Some(obj) = settings.as_object_mut() {
            obj.insert("modelCatalog".to_string(), stored_catalog.clone());
        }
    }

    settings
}

pub(crate) fn normalize_provider_common_config_for_storage(
    db: &Database,
    app_type: &AppType,
    provider: &mut Provider,
) -> Result<(), AppError> {
    let uses_common_config = provider
        .meta
        .as_ref()
        .and_then(|meta| meta.common_config_enabled)
        .unwrap_or(false);

    if !uses_common_config {
        return Ok(());
    }

    let Some(snippet) = db.get_config_snippet(app_type.as_str())? else {
        return Ok(());
    };

    if snippet.trim().is_empty() {
        return Ok(());
    }

    match remove_common_config_from_settings(app_type, &provider.settings_config, &snippet) {
        Ok(settings) => provider.settings_config = settings,
        Err(err) => {
            log::warn!(
                "Failed to normalize common config before saving {} provider '{}': {err}",
                app_type.as_str(),
                provider.id
            );
        }
    }

    Ok(())
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
        AppType::ClaudeDesktop => {
            return Err(AppError::localized(
                "claude_desktop.live.requires_db_context",
                "Claude Desktop 配置写入需要通过供应商切换流程执行",
                "Claude Desktop configuration must be written through the provider switch flow",
            ));
        }
        AppType::Codex => {
            let obj = provider
                .settings_config
                .as_object()
                .ok_or_else(|| AppError::Config("Codex 供应商配置必须是 JSON 对象".to_string()))?;
            let auth = obj
                .get("auth")
                .ok_or_else(|| AppError::Config("Codex 供应商配置缺少 'auth' 字段".to_string()))?;
            let config_str = obj.get("config").and_then(|v| v.as_str());

            // Native (direct) Responses and Anthropic providers must suppress Codex's
            // freeform apply_patch custom tool via the generated catalog; chat/proxy
            // providers keep the default tool set. Uses the same Anthropic detection as
            // the proxy router (apiFormat meta/settings + TOML wire_api).
            let profile = crate::proxy::providers::resolve_codex_catalog_tool_profile(provider);

            crate::codex_config::write_codex_provider_live_with_catalog(
                &provider.settings_config,
                provider.category.as_deref(),
                auth,
                config_str,
                profile,
            )?;
        }
        AppType::Gemini => {
            // Delegate to write_gemini_live which handles env file writing correctly
            write_gemini_live(provider)?;
        }
        AppType::GrokBuild => {
            crate::grok_config::write_grok_provider_live(provider)?;
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
                        return Err(AppError::Message(format!(
                            "OpenCode provider '{}' has invalid config structure for live config (must contain 'npm' or 'options')",
                            provider.id
                        )));
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
                        return Err(AppError::Message(format!(
                            "OpenClaw provider '{}' has invalid config structure for live config (must contain 'baseUrl', 'api', or 'models')",
                            provider.id
                        )));
                    }
                }
            }
        }
        AppType::Hermes => {
            crate::hermes_config::set_provider(&provider.id, provider.settings_config.clone())?;
            log::debug!("Hermes provider '{}' written to live config", provider.id);
        }
    }
    Ok(())
}

/// Sync all providers to live configuration (for additive mode apps)
///
/// Writes all providers from the database to the live configuration file.
/// Used for OpenCode and other additive mode applications.
fn sync_all_providers_to_live(state: &AppState, app_type: &AppType) -> Result<(), AppError> {
    let providers = state.db.get_all_providers(app_type.as_str())?;
    let mut synced_count = 0usize;

    for provider in providers.values() {
        if provider
            .meta
            .as_ref()
            .and_then(|meta| meta.live_config_managed)
            == Some(false)
        {
            continue;
        }

        if let Err(e) = write_live_with_common_config(state.db.as_ref(), app_type, provider) {
            log::warn!(
                "Failed to sync {:?} provider '{}' to live: {e}",
                app_type,
                provider.id
            );
            continue;
        }
        synced_count += 1;
    }

    log::info!("Synced {synced_count} {app_type:?} providers to live config");
    Ok(())
}

pub(crate) fn sync_current_provider_for_app_to_live(
    state: &AppState,
    app_type: &AppType,
) -> Result<(), AppError> {
    if app_type.is_additive_mode() {
        sync_all_providers_to_live(state, app_type)?;
    } else {
        let current_id = match crate::settings::get_effective_current_provider(&state.db, app_type)?
        {
            Some(id) => id,
            None => return Ok(()),
        };

        let providers = state.db.get_all_providers(app_type.as_str())?;
        if let Some(provider) = providers.get(&current_id) {
            write_live_with_common_config(state.db.as_ref(), app_type, provider)?;
        }
    }

    // 本函数语义是"把这个应用同步到 live"，MCP 重投影也只针对该应用；
    // 全量 sync_all_enabled 会把无关应用的 live 损坏牵连进来。投影失败
    // 上抛（不降级）：这里没有已变更的 DB 状态需要保护，调用方重试即可。
    McpService::sync_enabled_for_app(state, app_type)?;

    Ok(())
}

fn sync_current_provider_for_app_respecting_takeover(
    state: &AppState,
    app_type: &AppType,
) -> Result<(), AppError> {
    let current_id = match crate::settings::get_effective_current_provider(&state.db, app_type)? {
        Some(id) => id,
        None => return Ok(()),
    };

    let providers = state.db.get_all_providers(app_type.as_str())?;
    let Some(provider) = providers.get(&current_id) else {
        return Ok(());
    };

    let has_live_backup = futures::executor::block_on(state.db.get_live_backup(app_type.as_str()))
        .ok()
        .flatten()
        .is_some();
    let live_taken_over = state
        .proxy_service
        .detect_takeover_in_live_config_for_app(app_type);

    // `enabled` is set only after takeover writes complete. During that
    // activation window, backup/live placeholders are the authoritative signal
    // that normal provider sync must not rewrite the managed live file.
    if has_live_backup || live_taken_over {
        if matches!(app_type, AppType::ClaudeDesktop) {
            write_live_with_common_config(state.db.as_ref(), app_type, provider)?;
        } else {
            futures::executor::block_on(
                state
                    .proxy_service
                    .update_live_backup_from_provider(app_type.as_str(), provider),
            )
            .map_err(|e| AppError::Message(format!("更新 Live 备份失败: {e}")))?;
        }
        return Ok(());
    }

    write_live_with_common_config(state.db.as_ref(), app_type, provider)
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
            // Switch mode: sync only current provider. During proxy takeover,
            // update the restore backup instead of rewriting the taken-over
            // live file.
            sync_current_provider_for_app_respecting_takeover(state, &app_type)?;
        }
    }

    // MCP sync（best-effort 逐应用投影，内部已聚合失败）。错误暂存到
    // Skill 同步之后再返回：MCP 的失败不该跳过 Skill 同步，但调用方
    //（配置导入 / 云同步恢复）需要知道结果不完整。
    let mcp_result = McpService::sync_all_enabled(state);

    // Skill sync
    for app_type in AppType::all() {
        if let Err(e) = crate::services::skill::SkillService::sync_to_app(&state.db, &app_type) {
            log::warn!("同步 Skill 到 {app_type:?} 失败: {e}");
            // Continue syncing other apps, don't abort
        }
    }

    mcp_result
}

/// Read current live settings for an app type
pub fn read_live_settings(app_type: AppType) -> Result<Value, AppError> {
    match app_type {
        AppType::Codex => {
            let mut result = crate::codex_config::read_codex_live_settings()?;
            // `modelCatalog` is a cc-switch private field that lives only in
            // the DB SSOT plus the `cc-switch-model-catalog.json` projection
            // file — it is never inlined into `auth.json` or `config.toml`.
            // Reverse-parse the projection so the edit form for the active
            // Codex provider doesn't see an empty mapping table.
            if let Ok(Some(model_catalog)) =
                crate::codex_config::read_codex_model_catalog_simplified_from_live()
            {
                if let Some(obj) = result.as_object_mut() {
                    obj.insert("modelCatalog".to_string(), model_catalog);
                }
            }
            Ok(result)
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
        AppType::ClaudeDesktop => Err(AppError::localized(
            "claude_desktop.live.read_unsupported",
            "Claude Desktop 3P 配置不支持作为通用 live 配置导入，请使用“从 Claude 导入兼容供应商”。",
            "Claude Desktop 3P configuration cannot be imported as a generic live config. Use 'Import compatible providers from Claude' instead.",
        )),
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
        AppType::GrokBuild => crate::grok_config::read_grok_live_settings(),
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
        AppType::Hermes => {
            let config_path = crate::hermes_config::get_hermes_config_path();
            if !config_path.exists() {
                return Err(AppError::localized(
                    "hermes.config.missing",
                    "Hermes 配置文件不存在",
                    "Hermes configuration file not found",
                ));
            }
            let yaml_config = crate::hermes_config::read_hermes_config()?;
            let config = crate::hermes_config::yaml_to_json(&yaml_config)?;
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

    // 允许 "只有官方 seed 预设" 的情况下继续导入 live：
    // - 启动编排顺序是先 import 后 seed，新用户启动时 providers 为空，导入照常
    // - 老用户已有非 seed provider，跳过导入（正确）
    // - 用户手动点 ProviderEmptyState 的导入按钮时，与官方 seed 共存而不被阻塞
    if state.db.has_non_official_seed_provider(app_type.as_str())? {
        return Ok(false);
    }

    // 拒绝把"被代理接管的 Live"导入为供应商：接管期间 Live 里只有
    // PROXY_MANAGED 占位符和本地代理地址，不是用户的真实配置。一旦导入，
    // 它会成为 current provider（SSOT），后续"无备份恢复"路径会把占位符
    // 当真实配置写回 Live，永久卡在已失效的本地代理上。
    // 典型触发场景：代理接管开启时切换 app_config_dir 并重启，新数据库首启导入。
    if state
        .proxy_service
        .detect_takeover_in_live_config_for_app(&app_type)
    {
        return Err(AppError::localized(
            "provider.import.live_taken_over",
            "Live 配置当前处于代理接管状态（包含占位符），不能导入为供应商。请先关闭代理接管或恢复 Live 配置后重试。",
            "The live config is currently taken over by the proxy (contains placeholders) and cannot be imported as a provider. Disable proxy takeover or restore the live config first.",
        ));
    }

    let settings_config = match app_type {
        AppType::Codex => crate::codex_config::read_codex_live_settings()?,
        AppType::GrokBuild => {
            let mut settings = crate::grok_config::read_grok_live_settings()?;
            let config = settings
                .get("config")
                .and_then(Value::as_str)
                .unwrap_or_default();
            // 官方登录态（无自定义模型表）在这里必须报错：本函数也被启动
            // 自动导入调用，而全项目惯例是"启动自动导入只产出 default，
            // 从不产出官方条目"——否则删掉的官方条目每次重启都会复活。
            // 官方态的成功导入（补官方条目并激活）只挂在手动导入的命令层
            // （`import_default_config_internal`）。
            crate::grok_config::validate_config_toml(config)?;
            crate::grok_config::strip_grok_mcp_servers_from_settings(&mut settings)?;
            settings
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
        AppType::ClaudeDesktop => {
            return Err(AppError::localized(
                "claude_desktop.import_unsupported",
                "Claude Desktop 3P 配置不能通过通用导入读取，请使用“从 Claude 导入兼容供应商”。",
                "Claude Desktop 3P config cannot be imported through the generic import flow. Use 'Import compatible providers from Claude' instead.",
            ));
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
        // OpenCode, OpenClaw and Hermes use additive mode and are handled by early return above
        AppType::OpenCode | AppType::OpenClaw | AppType::Hermes => {
            unreachable!("additive mode apps are handled by early return")
        }
    };

    let mut provider = Provider::with_id(
        "default".to_string(),
        "default".to_string(),
        settings_config,
        None,
    );
    provider.category = Some(
        if matches!(app_type, AppType::Codex) {
            let config_text = provider
                .settings_config
                .get("config")
                .and_then(Value::as_str);
            let has_provider_key = crate::codex_config::extract_codex_api_key(
                provider.settings_config.get("auth"),
                config_text,
            )
            .is_some();
            let has_login_material = provider
                .settings_config
                .get("auth")
                .is_some_and(crate::codex_config::codex_auth_has_login_material);

            if has_login_material && !has_provider_key {
                "official"
            } else {
                "custom"
            }
        } else {
            "custom"
        }
        .to_string(),
    );

    state.db.save_provider(app_type.as_str(), &provider)?;
    state
        .db
        .set_current_provider(app_type.as_str(), &provider.id)?;
    crate::settings::set_current_provider(&app_type, Some(provider.id.as_str()))?;

    // 初次导入已有配置时随手补出官方入口，对齐其它应用"首启动 = 导入 default
    // + 播种官方条目"的观感。grokbuild 种子晚于 `official_providers_seeded`
    // flag 引入，存量库的主播种不会再跑，只能挂在导入动作上补。
    // 只在导入成功时执行；live 完全不可导入（文件缺失/语法错误/残缺配置）
    // 不会到达这里。失败只 warn。
    if matches!(app_type, AppType::GrokBuild) {
        if let Err(e) = state.db.ensure_official_seed_by_id(
            crate::database::GROKBUILD_OFFICIAL_PROVIDER_ID,
            AppType::GrokBuild,
        ) {
            log::warn!("Failed to ensure grokbuild-official seed after import: {e}");
        }
    }

    Ok(true) // 真正导入了
}

/// Decide whether startup should auto-import the current live config as `default`.
///
/// This is intentionally stricter than the manual import path:
/// if the app already has any provider row at all (including official seeds),
/// startup must skip auto-import to avoid recreating `default` on each launch.
pub fn should_import_default_config_on_startup(
    state: &AppState,
    app_type: &AppType,
) -> Result<bool, AppError> {
    if app_type.is_additive_mode() {
        return Ok(false);
    }

    Ok(!state.db.has_any_provider_for_app(app_type.as_str())?)
}

/// Write Gemini live configuration with authentication handling
pub(crate) fn write_gemini_live(provider: &Provider) -> Result<(), AppError> {
    use crate::gemini_config::{
        get_gemini_settings_path, json_to_env, validate_gemini_settings_strict,
        write_gemini_env_atomic,
    };

    // One-time auth type detection to avoid repeated detection
    let auth_type = detect_gemini_auth_type(provider);

    let env_map = json_to_env(&provider.settings_config)?;

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
            // Google Official uses OAuth, no API key validation needed.
            // Write user's env vars as-is (e.g. GEMINI_MODEL, custom vars).
            write_gemini_env_atomic(&env_map)?;
        }
        GeminiAuthType::Packycode | GeminiAuthType::Generic => {
            // API Key mode -- require GEMINI_API_KEY
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
    let mut updated = 0;
    let existing_ids = state.db.get_provider_ids("opencode")?;

    for (id, config) in providers {
        // Convert to Value for settings_config
        let settings_config = match serde_json::to_value(&config) {
            Ok(v) => v,
            Err(e) => {
                log::warn!("Failed to serialize OpenCode provider '{id}': {e}");
                continue;
            }
        };

        if existing_ids.contains(&id) {
            match state.db.get_provider_by_id(&id, "opencode") {
                Ok(Some(existing)) => {
                    let display_name = config.name.clone().unwrap_or_else(|| existing.name.clone());
                    if existing.settings_config != settings_config || existing.name != display_name
                    {
                        let mut provider = existing;
                        provider.name = display_name;
                        provider.settings_config = settings_config;
                        if let Err(e) = state.db.save_provider("opencode", &provider) {
                            log::warn!(
                                "Failed to update OpenCode provider '{id}' from live config: {e}"
                            );
                        } else {
                            updated += 1;
                            log::info!("Updated OpenCode provider '{id}' from live config");
                        }
                    }
                }
                Ok(None) => {
                    log::warn!("OpenCode provider '{id}' disappeared while importing live config")
                }
                Err(e) => log::warn!("Failed to look up OpenCode provider '{id}': {e}"),
            }
            continue;
        }

        // Create provider
        let display_name = config.name.clone().unwrap_or_else(|| id.clone());
        let mut provider = Provider::with_id(id.clone(), display_name, settings_config, None);
        provider.meta = Some(crate::provider::ProviderMeta {
            live_config_managed: Some(true),
            ..Default::default()
        });

        // Save to database
        if let Err(e) = state.db.save_provider("opencode", &provider) {
            log::warn!("Failed to import OpenCode provider '{id}': {e}");
            continue;
        }

        imported += 1;
        log::info!("Imported OpenCode provider '{id}' from live config");
    }

    Ok(imported + updated)
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
    let mut updated = 0;
    let existing_ids = state.db.get_provider_ids("openclaw")?;

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

        // Convert to Value for settings_config
        let settings_config = match serde_json::to_value(&config) {
            Ok(v) => v,
            Err(e) => {
                log::warn!("Failed to serialize OpenClaw provider '{id}': {e}");
                continue;
            }
        };

        if existing_ids.contains(&id) {
            match state.db.get_provider_by_id(&id, "openclaw") {
                Ok(Some(existing)) => {
                    if existing.settings_config != settings_config {
                        let mut provider = existing;
                        provider.settings_config = settings_config;
                        if let Err(e) = state.db.save_provider("openclaw", &provider) {
                            log::warn!(
                                "Failed to update OpenClaw provider '{id}' from live config: {e}"
                            );
                        } else {
                            updated += 1;
                            log::info!("Updated OpenClaw provider '{id}' from live config");
                        }
                    }
                }
                Ok(None) => {
                    log::warn!("OpenClaw provider '{id}' disappeared while importing live config")
                }
                Err(e) => log::warn!("Failed to look up OpenClaw provider '{id}': {e}"),
            }
            continue;
        }

        // Determine display name: use first model name if available, otherwise use id
        let display_name = config
            .models
            .first()
            .and_then(|m| m.name.clone())
            .unwrap_or_else(|| id.clone());

        // Create provider
        let mut provider = Provider::with_id(id.clone(), display_name, settings_config, None);
        provider.meta = Some(crate::provider::ProviderMeta {
            live_config_managed: Some(true),
            ..Default::default()
        });

        // Save to database
        if let Err(e) = state.db.save_provider("openclaw", &provider) {
            log::warn!("Failed to import OpenClaw provider '{id}': {e}");
            continue;
        }

        imported += 1;
        log::info!("Imported OpenClaw provider '{id}' from live config");
    }

    Ok(imported + updated)
}

/// Import all providers from Hermes live config to database
///
/// This imports existing providers from ~/.hermes/config.yaml
/// into the CC Switch database. Each provider found will be added to the
/// database with is_current set to false.
pub fn import_hermes_providers_from_live(state: &AppState) -> Result<usize, AppError> {
    use crate::hermes_config;

    let providers = hermes_config::get_providers()?;
    if providers.is_empty() {
        return Ok(0);
    }

    let mut imported = 0;
    let mut updated = 0;
    let existing_ids = state.db.get_provider_ids("hermes")?;

    for (name, config) in providers {
        // Validate: skip entries with empty name
        if name.trim().is_empty() {
            log::warn!("Skipping Hermes provider with empty name");
            continue;
        }

        if existing_ids.contains(&name) {
            match state.db.get_provider_by_id(&name, "hermes") {
                Ok(Some(existing)) => {
                    if existing.settings_config != config {
                        let mut provider = existing;
                        provider.settings_config = config;
                        if let Err(e) = state.db.save_provider("hermes", &provider) {
                            log::warn!(
                                "Failed to update Hermes provider '{name}' from live config: {e}"
                            );
                        } else {
                            updated += 1;
                            log::info!("Updated Hermes provider '{name}' from live config");
                        }
                    }
                }
                Ok(None) => {
                    log::warn!("Hermes provider '{name}' disappeared while importing live config")
                }
                Err(e) => log::warn!("Failed to look up Hermes provider '{name}': {e}"),
            }
            continue;
        }

        // Create provider
        let mut provider = Provider::with_id(name.clone(), name.clone(), config, None);
        provider.meta = Some(crate::provider::ProviderMeta {
            live_config_managed: Some(true),
            ..Default::default()
        });

        // Save to database
        if let Err(e) = state.db.save_provider("hermes", &provider) {
            log::warn!("Failed to import Hermes provider '{name}': {e}");
            continue;
        }

        imported += 1;
        log::info!("Imported Hermes provider '{name}' from live config");
    }

    Ok(imported + updated)
}

/// Remove a Hermes provider from live config
///
/// This removes a specific provider from ~/.hermes/config.yaml
/// without affecting other providers in the file.
pub fn remove_hermes_provider_from_live(provider_id: &str) -> Result<(), AppError> {
    use crate::hermes_config;

    // Check if Hermes config directory exists
    if !hermes_config::get_hermes_dir().exists() {
        log::debug!("Hermes config directory doesn't exist, skipping removal of '{provider_id}'");
        return Ok(());
    }

    hermes_config::remove_provider(provider_id)?;
    log::info!("Hermes provider '{provider_id}' removed from live config");

    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn kimi_for_coding_effective_settings_backfill_256k_context() {
        let db = Database::memory().expect("create memory db");
        let provider = Provider::with_id(
            "kimi-for-coding".to_string(),
            "Kimi For Coding".to_string(),
            json!({
                "env": {
                    "ANTHROPIC_BASE_URL": "https://api.kimi.com/coding/",
                    "ANTHROPIC_MODEL": "kimi-for-coding",
                    "CLAUDE_CODE_AUTO_COMPACT_WINDOW": "262144"
                }
            }),
            None,
        );

        let effective =
            build_effective_settings_with_common_config(&db, &AppType::Claude, &provider)
                .expect("build effective settings");
        assert_eq!(
            effective["env"]["CLAUDE_CODE_MAX_CONTEXT_TOKENS"],
            json!("262144")
        );
        assert_eq!(
            effective["env"]["CLAUDE_CODE_AUTO_COMPACT_WINDOW"],
            json!("262144")
        );
    }

    #[test]
    fn kimi_for_coding_context_defaults_preserve_user_overrides() {
        let db = Database::memory().expect("create memory db");
        let provider = Provider::with_id(
            "kimi-for-coding".to_string(),
            "Kimi For Coding".to_string(),
            json!({
                "env": {
                    "ANTHROPIC_BASE_URL": "https://api.kimi.com/coding",
                    "CLAUDE_CODE_MAX_CONTEXT_TOKENS": "300000",
                    "CLAUDE_CODE_AUTO_COMPACT_WINDOW": "250000"
                }
            }),
            None,
        );

        let effective =
            build_effective_settings_with_common_config(&db, &AppType::Claude, &provider)
                .expect("build effective settings");
        assert_eq!(
            effective["env"]["CLAUDE_CODE_MAX_CONTEXT_TOKENS"],
            json!("300000")
        );
        assert_eq!(
            effective["env"]["CLAUDE_CODE_AUTO_COMPACT_WINDOW"],
            json!("250000")
        );
    }

    #[test]
    fn kimi_for_coding_backfill_strips_only_injected_context_default() {
        let db = Database::memory().expect("create memory db");
        let provider = Provider::with_id(
            "kimi-for-coding".to_string(),
            "Kimi For Coding".to_string(),
            json!({
                "env": {
                    "ANTHROPIC_BASE_URL": "https://api.kimi.com/coding/",
                    "CLAUDE_CODE_AUTO_COMPACT_WINDOW": "262144"
                }
            }),
            None,
        );

        let live = build_effective_settings_with_common_config(&db, &AppType::Claude, &provider)
            .expect("build effective settings");
        let backfilled =
            strip_common_config_from_live_settings(&db, &AppType::Claude, &provider, live);
        assert!(backfilled["env"]
            .get("CLAUDE_CODE_MAX_CONTEXT_TOKENS")
            .is_none());
        assert_eq!(
            backfilled["env"]["CLAUDE_CODE_AUTO_COMPACT_WINDOW"],
            json!("262144")
        );
    }

    #[test]
    fn codex_oauth_effective_settings_backfill_gpt_context_defaults() {
        let db = Database::memory().expect("create memory db");
        let mut provider = Provider::with_id(
            "codex-oauth".to_string(),
            "Codex".to_string(),
            json!({
                "env": {
                    "ANTHROPIC_MODEL": "gpt-5.6"
                }
            }),
            None,
        );
        provider.meta = Some(crate::provider::ProviderMeta {
            provider_type: Some("codex_oauth".to_string()),
            ..Default::default()
        });

        let effective =
            build_effective_settings_with_common_config(&db, &AppType::Claude, &provider)
                .expect("build effective settings");
        assert_eq!(
            effective["env"]["CLAUDE_CODE_MAX_CONTEXT_TOKENS"],
            json!("372000")
        );
        assert_eq!(
            effective["env"]["CLAUDE_CODE_AUTO_COMPACT_WINDOW"],
            json!("372000")
        );
    }

    #[test]
    fn codex_oauth_context_defaults_preserve_user_overrides() {
        let db = Database::memory().expect("create memory db");
        let mut provider = Provider::with_id(
            "codex-oauth".to_string(),
            "Codex".to_string(),
            json!({
                "env": {
                    "CLAUDE_CODE_MAX_CONTEXT_TOKENS": "500000",
                    "CLAUDE_CODE_AUTO_COMPACT_WINDOW": "350000"
                }
            }),
            None,
        );
        provider.meta = Some(crate::provider::ProviderMeta {
            provider_type: Some("codex_oauth".to_string()),
            ..Default::default()
        });

        let effective =
            build_effective_settings_with_common_config(&db, &AppType::Claude, &provider)
                .expect("build effective settings");
        assert_eq!(
            effective["env"]["CLAUDE_CODE_MAX_CONTEXT_TOKENS"],
            json!("500000")
        );
        assert_eq!(
            effective["env"]["CLAUDE_CODE_AUTO_COMPACT_WINDOW"],
            json!("350000")
        );
    }

    #[test]
    fn codex_oauth_context_defaults_ignore_legacy_common_config_values() {
        let db = Database::memory().expect("create memory db");
        db.set_config_snippet(
            AppType::Claude.as_str(),
            Some(
                json!({
                    "env": {
                        "CLAUDE_CODE_MAX_CONTEXT_TOKENS": "262144",
                        "CLAUDE_CODE_AUTO_COMPACT_WINDOW": "262144"
                    }
                })
                .to_string(),
            ),
        )
        .expect("save legacy common config");
        let mut provider = Provider::with_id(
            "codex-oauth".to_string(),
            "Codex".to_string(),
            json!({ "env": { "ANTHROPIC_MODEL": "gpt-5.6" } }),
            None,
        );
        provider.meta = Some(crate::provider::ProviderMeta {
            provider_type: Some("codex_oauth".to_string()),
            common_config_enabled: Some(true),
            ..Default::default()
        });

        let effective =
            build_effective_settings_with_common_config(&db, &AppType::Claude, &provider)
                .expect("build effective settings");
        assert_eq!(
            effective["env"]["CLAUDE_CODE_MAX_CONTEXT_TOKENS"],
            json!("372000")
        );
        assert_eq!(
            effective["env"]["CLAUDE_CODE_AUTO_COMPACT_WINDOW"],
            json!("372000")
        );
    }

    #[test]
    fn codex_oauth_context_defaults_skip_non_gpt56_models() {
        let db = Database::memory().expect("create memory db");
        db.set_config_snippet(
            AppType::Claude.as_str(),
            Some(
                json!({
                    "env": {
                        "CLAUDE_CODE_MAX_CONTEXT_TOKENS": "262144",
                        "CLAUDE_CODE_AUTO_COMPACT_WINDOW": "262144"
                    }
                })
                .to_string(),
            ),
        )
        .expect("save legacy common config");
        let mut provider = Provider::with_id(
            "codex-oauth".to_string(),
            "Codex".to_string(),
            json!({
                "env": {
                    "ANTHROPIC_MODEL": "gpt-5.5",
                    "CLAUDE_CODE_AUTO_COMPACT_WINDOW": "300000"
                }
            }),
            None,
        );
        provider.meta = Some(crate::provider::ProviderMeta {
            provider_type: Some("codex_oauth".to_string()),
            common_config_enabled: Some(true),
            ..Default::default()
        });

        let effective =
            build_effective_settings_with_common_config(&db, &AppType::Claude, &provider)
                .expect("build effective settings");
        // 旧模型不注入 372K 默认值，遗留共享片段带进来的值也要剥掉
        assert!(effective["env"]
            .get("CLAUDE_CODE_MAX_CONTEXT_TOKENS")
            .is_none());
        // 用户显式写在供应商配置里的值仍然生效
        assert_eq!(
            effective["env"]["CLAUDE_CODE_AUTO_COMPACT_WINDOW"],
            json!("300000")
        );
    }

    /// 往返不动点：注入产物只活在 live，切走回灌后存储配置必须与注入前一致，
    /// 否则程序默认值固化成"用户显式值"，之后调默认值永远压不动。
    #[test]
    fn codex_oauth_backfill_strips_injected_context_defaults() {
        let db = Database::memory().expect("create memory db");
        let mut provider = Provider::with_id(
            "codex-oauth".to_string(),
            "Codex".to_string(),
            json!({ "env": { "ANTHROPIC_MODEL": "gpt-5.6" } }),
            None,
        );
        provider.meta = Some(crate::provider::ProviderMeta {
            provider_type: Some("codex_oauth".to_string()),
            ..Default::default()
        });

        // 模拟写 live：注入了两个上下文默认值
        let live = build_effective_settings_with_common_config(&db, &AppType::Claude, &provider)
            .expect("build effective settings");
        assert_eq!(
            live["env"]["CLAUDE_CODE_MAX_CONTEXT_TOKENS"],
            json!("372000")
        );

        // 模拟切走回灌：注入产物被剥掉，其余字段原样保留
        let backfilled =
            strip_common_config_from_live_settings(&db, &AppType::Claude, &provider, live);
        assert!(backfilled["env"]
            .get("CLAUDE_CODE_MAX_CONTEXT_TOKENS")
            .is_none());
        assert!(backfilled["env"]
            .get("CLAUDE_CODE_AUTO_COMPACT_WINDOW")
            .is_none());
        assert_eq!(backfilled["env"]["ANTHROPIC_MODEL"], json!("gpt-5.6"));
    }

    #[test]
    fn codex_oauth_backfill_keeps_user_context_values() {
        let db = Database::memory().expect("create memory db");
        let mut provider = Provider::with_id(
            "codex-oauth".to_string(),
            "Codex".to_string(),
            json!({
                "env": {
                    "ANTHROPIC_MODEL": "gpt-5.6",
                    "CLAUDE_CODE_MAX_CONTEXT_TOKENS": "500000"
                }
            }),
            None,
        );
        provider.meta = Some(crate::provider::ProviderMeta {
            provider_type: Some("codex_oauth".to_string()),
            ..Default::default()
        });

        // live 里：MAX 是用户显式值；ACW 被用户手改成了非默认数字
        let live = json!({
            "env": {
                "ANTHROPIC_MODEL": "gpt-5.6",
                "CLAUDE_CODE_MAX_CONTEXT_TOKENS": "500000",
                "CLAUDE_CODE_AUTO_COMPACT_WINDOW": "300000"
            }
        });
        let backfilled =
            strip_common_config_from_live_settings(&db, &AppType::Claude, &provider, live);
        assert_eq!(
            backfilled["env"]["CLAUDE_CODE_MAX_CONTEXT_TOKENS"],
            json!("500000")
        );
        assert_eq!(
            backfilled["env"]["CLAUDE_CODE_AUTO_COMPACT_WINDOW"],
            json!("300000")
        );
    }

    /// C5 回归锁：前端表单的合并/剥离必须走 toml_edit 文档模型。
    /// smol-toml 的 parse→merge→stringify 整文档重序列化会丢注释、
    /// 按字母序重排键、并为 dotted 表生成多余的空父表头。
    #[test]
    fn update_toml_common_config_snippet_preserves_comments_and_key_order() {
        // 刻意非字母序的键序 + 注释，模拟用户手写格式
        let config = r#"# my precious comment
model = "gpt-5.5"
model_provider = "aprov"
disable_response_storage = true

[model_providers.aprov]
# provider comment
name = "A Prov"
base_url = "https://a.example/v1"
"#;
        let snippet = "[tui]\nnotifications = true\n";

        let merged = update_toml_common_config_snippet(config, snippet, true).unwrap();
        assert!(merged.contains("# my precious comment"));
        assert!(merged.contains("# provider comment"));
        let model_pos = merged.find("model = ").unwrap();
        let provider_pos = merged.find("model_provider = ").unwrap();
        let disable_pos = merged.find("disable_response_storage").unwrap();
        assert!(
            model_pos < provider_pos && provider_pos < disable_pos,
            "merge must not reorder user keys, got: {merged}"
        );
        assert!(merged.contains("[tui]"));
        assert!(merged.contains("notifications = true"));
        assert!(
            !merged.contains("[model_providers]\n"),
            "merge must not synthesize an empty parent table header, got: {merged}"
        );

        let removed = update_toml_common_config_snippet(&merged, snippet, false).unwrap();
        assert!(!removed.contains("[tui]"), "snippet keys must be stripped");
        assert!(removed.contains("# my precious comment"));
        assert!(removed.contains("disable_response_storage = true"));
    }

    /// 合并时标量=片段覆盖供应商值（与 Claude 侧 deepMerge 一致）；
    /// 剥离按值匹配：用户改过的值不删（与 strip 路径的
    /// toml_value_is_subset 语义一致）。
    #[test]
    fn update_toml_common_config_snippet_scalar_override_and_value_matched_removal() {
        let snippet = "[tui]\nnotifications = true\n";

        let merged =
            update_toml_common_config_snippet("[tui]\nnotifications = false\n", snippet, true)
                .unwrap();
        assert!(
            merged.contains("notifications = true"),
            "snippet scalar should override provider value, got: {merged}"
        );

        let removed =
            update_toml_common_config_snippet("[tui]\nnotifications = false\n", snippet, false)
                .unwrap();
        assert!(
            removed.contains("notifications = false"),
            "user-modified value must survive removal, got: {removed}"
        );
    }

    #[test]
    fn claude_common_config_apply_and_remove_roundtrip_for_non_overlapping_fields() {
        let settings = json!({
            "env": {
                "ANTHROPIC_API_KEY": "sk-test"
            }
        });
        let snippet = r#"{
  "includeCoAuthoredBy": false,
  "env": {
    "CLAUDE_CODE_USE_BEDROCK": "1"
  }
}"#;

        let applied =
            apply_common_config_to_settings(&AppType::Claude, &settings, snippet).unwrap();
        assert_eq!(applied["includeCoAuthoredBy"], json!(false));
        assert_eq!(applied["env"]["CLAUDE_CODE_USE_BEDROCK"], json!("1"));

        let stripped =
            remove_common_config_from_settings(&AppType::Claude, &applied, snippet).unwrap();
        assert_eq!(stripped, settings);
    }

    #[test]
    fn codex_common_config_apply_and_remove_roundtrip_for_non_overlapping_fields() {
        let settings = json!({
            "auth": {
                "OPENAI_API_KEY": "sk-test"
            },
            "config": "model_provider = \"openai\"\n[general]\nmodel = \"gpt-5\"\n"
        });
        let snippet = "[shared]\nreasoning = \"medium\"\n";

        let applied = apply_common_config_to_settings(&AppType::Codex, &settings, snippet).unwrap();
        let applied_config = applied["config"].as_str().unwrap_or_default();
        assert!(applied_config.contains("[shared]"));
        assert!(applied_config.contains("reasoning = \"medium\""));

        let stripped =
            remove_common_config_from_settings(&AppType::Codex, &applied, snippet).unwrap();
        assert_eq!(stripped, settings);
    }

    #[test]
    fn explicit_common_config_flag_overrides_legacy_subset_detection() {
        let mut provider = Provider::with_id(
            "claude-test".to_string(),
            "Claude Test".to_string(),
            json!({
                "includeCoAuthoredBy": false
            }),
            None,
        );
        provider.meta = Some(crate::provider::ProviderMeta {
            common_config_enabled: Some(false),
            ..Default::default()
        });

        assert!(
            !provider_uses_common_config(
                &AppType::Claude,
                &provider,
                Some(r#"{ "includeCoAuthoredBy": false }"#),
            ),
            "explicit false should win over legacy subset detection"
        );
    }

    #[test]
    fn claude_common_config_array_subset_detection_and_strip_preserve_extra_items() {
        let settings = json!({
            "allowedTools": ["tool1", "tool2"]
        });
        let snippet = r#"{
  "allowedTools": ["tool1"]
}"#;

        assert!(
            settings_contain_common_config(&AppType::Claude, &settings, snippet),
            "array subset should be detected for legacy providers"
        );

        let stripped =
            remove_common_config_from_settings(&AppType::Claude, &settings, snippet).unwrap();
        assert_eq!(
            stripped,
            json!({
                "allowedTools": ["tool2"]
            })
        );
    }

    #[test]
    fn codex_common_config_array_subset_detection_and_strip_preserve_extra_items() {
        let settings = json!({
            "auth": {},
            "config": "allowed_tools = [\"tool1\", \"tool2\"]\n"
        });
        let snippet = "allowed_tools = [\"tool1\"]\n";

        assert!(
            settings_contain_common_config(&AppType::Codex, &settings, snippet),
            "TOML array subset should be detected for legacy providers"
        );

        let stripped =
            remove_common_config_from_settings(&AppType::Codex, &settings, snippet).unwrap();
        assert_eq!(stripped["auth"], json!({}));
        let stripped_config = stripped["config"].as_str().unwrap_or_default();
        let parsed = stripped_config
            .parse::<DocumentMut>()
            .expect("stripped codex config should remain valid TOML");
        let allowed_tools = parsed["allowed_tools"]
            .as_array()
            .expect("allowed_tools should remain an array");
        let values: Vec<&str> = allowed_tools
            .iter()
            .map(|value| value.as_str().expect("tool id should be string"))
            .collect();
        assert_eq!(values, vec!["tool2"]);
    }

    #[test]
    fn codex_switch_backfill_preserves_stored_model_catalog_when_live_lacks_it() {
        // Reproduces the data-loss bug: switching away from a Codex provider
        // backfills the outgoing provider from Live, but Live's config.toml had
        // already lost its `model_catalog_json` projection (proxy cycle /
        // Codex.app rewrite), so `read_live_settings` reconstructs no catalog.
        // The stored mapping must survive the backfill.
        let mut provider = Provider::with_id(
            "deepseek".to_string(),
            "DeepSeek".to_string(),
            json!({
                "auth": { "OPENAI_API_KEY": "sk-deepseek" },
                "config": "model_provider = \"custom\"\nmodel = \"deepseek-v4-pro\"\n",
                "modelCatalog": {
                    "models": [
                        { "model": "deepseek-v4-pro", "contextWindow": 1_000_000 }
                    ]
                }
            }),
            None,
        );
        provider.category = Some("cn_official".to_string());

        // Live snapshot as captured during switch: no `modelCatalog` field.
        let live_settings = json!({
            "auth": { "OPENAI_API_KEY": "sk-deepseek" },
            "config": "model_provider = \"custom\"\nmodel = \"deepseek-v4-pro\"\n"
        });

        let result =
            restore_live_settings_for_provider_backfill(&AppType::Codex, &provider, live_settings);

        assert_eq!(
            result.get("modelCatalog"),
            provider.settings_config.get("modelCatalog"),
            "switch-away backfill must keep the DB-stored modelCatalog when Live has none"
        );
    }

    #[test]
    fn codex_switch_backfill_keeps_live_catalog_when_db_has_none() {
        // When the DB provider has no stored catalog, a catalog reconstructed
        // from Live (if any) should be left intact — the DB-preference overlay
        // must not wipe it.
        let mut provider = Provider::with_id(
            "deepseek".to_string(),
            "DeepSeek".to_string(),
            json!({
                "auth": { "OPENAI_API_KEY": "sk-deepseek" },
                "config": "model_provider = \"custom\"\nmodel = \"deepseek-v4-pro\"\n"
            }),
            None,
        );
        provider.category = Some("cn_official".to_string());

        let live_settings = json!({
            "auth": { "OPENAI_API_KEY": "sk-deepseek" },
            "config": "model_provider = \"custom\"\nmodel = \"deepseek-v4-pro\"\n",
            "modelCatalog": { "models": [ { "model": "deepseek-v4-pro" } ] }
        });

        let result = restore_live_settings_for_provider_backfill(
            &AppType::Codex,
            &provider,
            live_settings.clone(),
        );

        assert_eq!(
            result.get("modelCatalog"),
            live_settings.get("modelCatalog"),
            "backfill must keep the Live-reconstructed catalog when the DB has none"
        );
    }

    #[test]
    fn codex_switch_backfill_strips_synced_mcp_servers() {
        // Live 里的 [mcp_servers] 是 MCP 同步的投影（SSOT 在 DB 表），
        // 回填进供应商存储配置会让已删除的服务器随快照复活。
        let provider = Provider::with_id(
            "prov".to_string(),
            "Prov".to_string(),
            json!({
                "auth": { "OPENAI_API_KEY": "sk-test" },
                "config": "model = \"gpt-5.5\"\n"
            }),
            None,
        );

        let live_settings = json!({
            "auth": { "OPENAI_API_KEY": "sk-test" },
            "config": "model = \"gpt-5.5\"\n\n[mcp_servers.echo]\ntype = \"stdio\"\ncommand = \"echo\"\n"
        });

        let result =
            restore_live_settings_for_provider_backfill(&AppType::Codex, &provider, live_settings);

        let config_text = result
            .get("config")
            .and_then(|v| v.as_str())
            .expect("config text");
        assert!(
            !config_text.contains("mcp_servers"),
            "backfill must strip synced [mcp_servers] from the stored provider config, got: {config_text}"
        );
        assert!(
            config_text.contains("model = \"gpt-5.5\""),
            "non-MCP content must survive the strip"
        );
    }

    #[test]
    fn grok_switch_backfill_strips_synced_mcp_servers() {
        let provider = Provider::with_id(
            "grok".to_string(),
            "Grok".to_string(),
            json!({
                "config": "[models]\ndefault = \"grok-4.5\"\n\n[model.\"grok-4.5\"]\nmodel = \"grok-4.5\"\nbase_url = \"https://example.com/v1\"\nname = \"Example\"\napi_key = \"secret\"\napi_backend = \"responses\"\ncontext_window = 500000\n"
            }),
            None,
        );
        let live_settings = json!({
            "config": "[models]\ndefault = \"grok-4.5\"\n\n[model.\"grok-4.5\"]\nmodel = \"grok-4.5\"\nbase_url = \"https://example.com/v1\"\nname = \"Example\"\napi_key = \"secret\"\napi_backend = \"responses\"\ncontext_window = 500000\n\n[mcp_servers.echo]\ncommand = \"echo\"\n"
        });

        let result = restore_live_settings_for_provider_backfill(
            &AppType::GrokBuild,
            &provider,
            live_settings,
        );
        let config_text = result
            .get("config")
            .and_then(Value::as_str)
            .expect("config text");

        assert!(!config_text.contains("mcp_servers"));
        assert!(config_text.contains("model = \"grok-4.5\""));
    }
}
