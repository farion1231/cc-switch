//! Live configuration operations
//!
//! Handles reading and writing live configuration files for Claude, Codex, and Gemini.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};
use toml_edit::{DocumentMut, Item, TableLike};

use rusqlite::{params, OptionalExtension, TransactionBehavior};

use crate::app_config::AppType;
use crate::claude_desktop_config::CLAUDE_MODEL_ENV_KEYS;
use crate::codex_config::{get_codex_auth_path, get_codex_config_path};
use crate::config::{delete_file, get_claude_settings_path, read_json_file, write_json_file};
use crate::database::Database;
use crate::error::AppError;
use crate::provider::Provider;
use crate::proxy::providers::codex_oauth_auth::CodexOAuthManager;
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
const CLAUDE_SUBAGENT_ONE_M_MARKER: &str = "[1M]";

pub(crate) fn provider_env_targets_gpt56(
    provider_env: Option<&serde_json::Map<String, Value>>,
) -> bool {
    let Some(env) = provider_env else {
        return false;
    };
    let mut saw_model = false;
    for key in CLAUDE_MODEL_ENV_KEYS {
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

pub(crate) fn is_kimi_for_coding_provider(provider: &Provider) -> bool {
    provider
        .settings_config
        .pointer("/env/ANTHROPIC_BASE_URL")
        .and_then(Value::as_str)
        .map(str::trim)
        .map(|url| url.trim_end_matches('/'))
        == Some("https://api.kimi.com/coding")
}

/// Kimi / Codex OAuth 静态回退窗口的单一来源（F6/D3）：
/// 返回 (ACW, MAX) 字符串对，5 处调用点统一引用，行为由维护者 P1-2 固定：
/// Kimi For Coding 262144、Codex OAuth（env 全部指向 gpt-5.6）372000。
pub(crate) fn static_context_window_fallback(
    provider: &Provider,
) -> Option<(&'static str, &'static str)> {
    let provider_env = provider
        .settings_config
        .get("env")
        .and_then(Value::as_object);
    if provider.is_codex_oauth() && provider_env_targets_gpt56(provider_env) {
        Some((
            CODEX_OAUTH_CLAUDE_AUTO_COMPACT_WINDOW,
            CODEX_OAUTH_CLAUDE_MAX_CONTEXT_TOKENS,
        ))
    } else if is_kimi_for_coding_provider(provider) {
        Some((
            KIMI_FOR_CODING_CONTEXT_TOKENS,
            KIMI_FOR_CODING_CONTEXT_TOKENS,
        ))
    } else {
        None
    }
}

/// 获取或创建 settings 顶层 autoSyncState 的 &mut Value（统一 get-or-create 样板）。
pub(crate) fn auto_sync_state_value_mut(settings: &mut Value) -> Option<&mut Value> {
    let obj = settings.as_object_mut()?;
    let state = obj
        .entry("autoSyncState".to_string())
        .or_insert_with(|| json!({}));
    if !state.is_object() {
        *state = json!({});
    }
    Some(state)
}

/// 获取或创建 settings 顶层 autoSyncState 对象。
pub(crate) fn auto_sync_state_mut(
    settings: &mut Value,
) -> Option<&mut serde_json::Map<String, Value>> {
    auto_sync_state_value_mut(settings)?.as_object_mut()
}

fn record_static_injected(settings: &mut Value, acw: &str, max: &str) {
    if let Some(state) = auto_sync_state_mut(settings) {
        state.insert(
            "staticInjected".to_string(),
            json!({ "ACW": acw, "MAX": max }),
        );
    }
}

fn clear_static_injected(settings: &mut Value) {
    if let Some(state) = settings
        .get_mut("autoSyncState")
        .and_then(Value::as_object_mut)
    {
        state.remove("staticInjected");
    }
}

/// Claude Code assigns unknown non-Claude model ids a 200K context window.
/// Codex OAuth deliberately exposes GPT ids through Claude Code, so enrich the
/// effective live settings for both newly-created and already-saved providers.
/// Explicit user values always win; the static defaults (Kimi 262144 / Codex
/// OAuth gpt-5.6 372000) are only injected when the provider kind applies.
/// 非静态供应商直接返回，不碰 apply_context_window_defaults 已写好的 staticInjected。
fn apply_static_context_defaults(settings: &mut Value, provider: &Provider) {
    if !provider.is_codex_oauth() && !is_kimi_for_coding_provider(provider) {
        return;
    }

    let fallback = static_context_window_fallback(provider);
    let provider_env = provider
        .settings_config
        .get("env")
        .and_then(Value::as_object);
    let (has_explicit, injected_count) = {
        let Some(env) = settings.get_mut("env").and_then(Value::as_object_mut) else {
            return;
        };
        let mut has_explicit = false;
        let mut injected_count = 0;
        for (key, default_value) in [
            (
                "CLAUDE_CODE_MAX_CONTEXT_TOKENS",
                fallback.map(|(_acw, max)| max),
            ),
            (
                "CLAUDE_CODE_AUTO_COMPACT_WINDOW",
                fallback.map(|(acw, _max)| acw),
            ),
        ] {
            if let Some(value) = provider_env.and_then(|provider_env| provider_env.get(key)) {
                // 显式 provider 值优先，覆盖 common config 的注入值
                env.insert(key.to_string(), value.clone());
                has_explicit = true;
            } else if let Some(default_value) = default_value {
                env.insert(key.to_string(), Value::String(default_value.to_string()));
                injected_count += 1;
            }
        }
        (has_explicit, injected_count)
    };

    if has_explicit {
        clear_static_injected(settings);
    } else if injected_count == 2 {
        if let Some((acw, max)) = fallback {
            record_static_injected(settings, acw, max);
        }
    }
}

/// 从 contextWindows 取所有角色 key 的最大窗口；旧模型后缀会先迁移成 contextWindows。
/// ACW = 窗口 × provider 的压缩比例，MAX = 窗口（复用 watcher 的 build_env_writes 保持一致）。
fn claude_context_window_target(provider: &Provider) -> Option<u64> {
    let mut migrated = provider.settings_config.clone();
    crate::claude_desktop_config::migrate_legacy_suffix_to_context_windows(&mut migrated);
    context_window_target_from_settings(&migrated)
}

fn context_window_target_from_settings(settings: &Value) -> Option<u64> {
    let windows = settings.get("contextWindows").and_then(Value::as_object)?;
    CLAUDE_MODEL_ENV_KEYS
        .iter()
        .filter_map(|key| {
            windows
                .get(*key)
                .and_then(Value::as_u64)
                .filter(|window| *window > 0)
        })
        .max()
}

fn apply_context_window_defaults(settings: &mut Value, provider: &Provider) {
    let Some(target) = context_window_target_from_settings(settings)
        .or_else(|| context_window_target_from_settings(&provider.settings_config))
    else {
        clear_static_injected(settings);
        return;
    };
    let writes = crate::claude_settings_watcher::build_env_writes(
        target,
        crate::claude_settings_watcher::provider_compact_ratio(provider),
    );
    let has_explicit_acw;
    let has_explicit_max;
    let mut injected_writes = Vec::new();
    {
        let Some(env) = settings.get_mut("env").and_then(Value::as_object_mut) else {
            return;
        };
        let provider_env = provider
            .settings_config
            .get("env")
            .and_then(Value::as_object);
        has_explicit_acw = provider_env
            .and_then(|env| env.get("CLAUDE_CODE_AUTO_COMPACT_WINDOW"))
            .is_some();
        has_explicit_max = provider_env
            .and_then(|env| env.get("CLAUDE_CODE_MAX_CONTEXT_TOKENS"))
            .is_some();
        for (key, value) in &writes {
            if !env.contains_key(*key) {
                env.insert(key.to_string(), Value::String(value.clone()));
                injected_writes.push((*key, value.clone()));
            }
        }
    }

    if has_explicit_acw || has_explicit_max {
        clear_static_injected(settings);
    } else if injected_writes.len() == 2 {
        let acw = injected_writes
            .iter()
            .find(|(key, _)| *key == "CLAUDE_CODE_AUTO_COMPACT_WINDOW")
            .map(|(_, value)| value.as_str())
            .expect("ACW write present");
        let max = injected_writes
            .iter()
            .find(|(key, _)| *key == "CLAUDE_CODE_MAX_CONTEXT_TOKENS")
            .map(|(_, value)| value.as_str())
            .expect("MAX write present");
        record_static_injected(settings, acw, max);
    }
}

/// proxy 接管需要 subagent 的本地 1M 标记；直连写 live 前再由
/// `strip_legacy_suffixes_from_claude_models` 统一剥离。
fn restore_claude_subagent_local_marker_for_effective(settings: &mut Value, provider: &Provider) {
    let Some(window) = crate::claude_desktop_config::resolve_context_window(
        &provider.settings_config,
        "CLAUDE_CODE_SUBAGENT_MODEL",
    ) else {
        return;
    };
    if window < 1_000_000 {
        return;
    }

    let Some(env) = settings.get_mut("env").and_then(Value::as_object_mut) else {
        return;
    };
    let Some(model) = env
        .get("CLAUDE_CODE_SUBAGENT_MODEL")
        .and_then(Value::as_str)
    else {
        return;
    };

    if crate::claude_desktop_config::parse_context_window_suffix(model)
        .1
        .is_some()
    {
        return;
    }

    env.insert(
        "CLAUDE_CODE_SUBAGENT_MODEL".to_string(),
        Value::String(format!("{model}{CLAUDE_SUBAGENT_ONE_M_MARKER}")),
    );
}

pub(crate) fn strip_legacy_suffixes_from_claude_models(settings: &mut Value) {
    let Some(env) = settings.get_mut("env").and_then(Value::as_object_mut) else {
        return;
    };
    for key in CLAUDE_MODEL_ENV_KEYS {
        let Some(value) = env.get(key) else {
            continue;
        };
        let Some(model) = value.as_str() else {
            continue;
        };
        let cleaned = crate::proxy::model_mapper::strip_one_m_suffix_for_upstream(model);
        if cleaned != model {
            env.insert(key.to_string(), Value::String(cleaned.to_string()));
        }
    }
}

fn merge_auto_sync_state_into_provider(provider_settings: &mut Value, effective_settings: &Value) {
    let Some(effective_state) = effective_settings
        .get("autoSyncState")
        .and_then(Value::as_object)
    else {
        return;
    };
    let Some(state) = auto_sync_state_mut(provider_settings) else {
        return;
    };
    if let Some(static_injected) = effective_state.get("staticInjected") {
        state.insert("staticInjected".to_string(), static_injected.clone());
    } else {
        state.remove("staticInjected");
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
        // cc-switch 专用字段：watcher 从 provider.settings_config 读取，
        // 不应泄露到 Claude Code 的 settings.json
        obj.remove("autoSyncContextWindow");
        obj.remove("autoSyncCompactRatio");
        obj.remove("contextWindows");
        obj.remove("contextWindowTokens");
        obj.remove("autoSyncState");
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
        AppType::Pi => crate::pi_config::pi_provider_exists(provider_id),
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
        | AppType::Pi
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
        | AppType::Pi
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
        | AppType::Pi
        | AppType::ClaudeDesktop => Ok(settings.clone()),
    }
}

fn merge_watcher_auto_sync_state(stored_settings: &mut Value, watcher_settings: &Value) {
    let Some(watcher_state) = watcher_settings.get("autoSyncState") else {
        return;
    };
    let Some(stored_state) = auto_sync_state_value_mut(stored_settings) else {
        return;
    };
    json_deep_merge(stored_state, watcher_state);
}

/// watcher 应使用写入 live 时的 effective settings 快照，而不是原始 provider 配置。
fn watcher_provider_from_effective(provider: &Provider, effective_settings: &Value) -> Provider {
    let mut watcher_provider = provider.clone();
    watcher_provider.settings_config = effective_settings.clone();
    watcher_provider
}

fn claude_watcher_persist_callback(
    db: &Database,
    app_type: &AppType,
    provider_id: &str,
) -> crate::claude_settings_watcher::PersistSettingsCallback {
    // notify 后台线程要求回调 'static，Database 不是 Clone；这里从当前连接拿到
    // DB 文件路径，回调内打开同一文件并在事务里只合并 autoSyncState 后更新。
    let db_path = db
        .conn
        .lock()
        .ok()
        .and_then(|conn| conn.path().map(std::path::PathBuf::from))
        .filter(|path| !path.as_os_str().is_empty());
    let app_type_str = app_type.as_str().to_string();
    let provider_id = provider_id.to_string();
    std::sync::Arc::new(move |settings| {
        let Some(db_path) = db_path.as_ref() else {
            return Err("cannot persist watcher state to in-memory database".to_string());
        };
        let mut conn = rusqlite::Connection::open(db_path).map_err(|e| e.to_string())?;
        // watcher 线程可能与应用主连接并发写同一 DB，避免立刻 SQLITE_BUSY 失败。
        conn.busy_timeout(Duration::from_secs(5))
            .map_err(|e| format!("set watcher DB busy_timeout: {e}"))?;
        // 用 BEGIN IMMEDIATE 事务包住 read-modify-write，避免与应用主连接并发更新时互相覆盖。
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
        let stored_settings: Option<String> = tx
            .query_row(
                "SELECT settings_config FROM providers WHERE id = ?1 AND app_type = ?2",
                params![provider_id, app_type_str],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        let Some(stored_settings) = stored_settings else {
            return Err(format!(
                "provider {provider_id} not found while persisting watcher state"
            ));
        };
        let mut stored_settings: Value =
            serde_json::from_str(&stored_settings).map_err(|e| e.to_string())?;
        merge_watcher_auto_sync_state(&mut stored_settings, &settings);
        tx.execute(
            "UPDATE providers SET settings_config = ?1 WHERE id = ?2 AND app_type = ?3",
            params![
                serde_json::to_string(&stored_settings).map_err(|e| e.to_string())?,
                provider_id,
                app_type_str
            ],
        )
        .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())
    })
}

/// 写 live 成功后启动/替换 Claude settings watcher；失败只记录日志，不阻断写配置。
pub(crate) fn ensure_claude_settings_watcher(
    db: &Database,
    app_type: &AppType,
    provider: &Provider,
    effective_settings: &Value,
) {
    if !matches!(app_type, AppType::Claude) {
        return;
    }
    #[cfg(test)]
    {
        // 测试默认不 spawn；仅显式设置 CC_SWITCH_TEST_WATCHER=1 时才走真实 spawn。
        if std::env::var("CC_SWITCH_TEST_WATCHER").as_deref() != Ok("1") {
            return;
        }
    }
    // 测试/调试隔离：设置该变量时不要创建目录或 spawn，避免触碰真实 ~/.claude。
    if std::env::var("CC_SWITCH_DISABLE_WATCHER").as_deref() == Ok("1") {
        return;
    }

    let settings_path = get_claude_settings_path();
    // 确保父目录存在：fresh 安装时 ~/.claude 可能尚未创建。
    if let Some(parent) = settings_path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            log::warn!(
                "[ClaudeSettingsWatcher] failed to create {}: {e}",
                parent.display()
            );
        }
    }
    if !settings_path.parent().map(|p| p.exists()).unwrap_or(false) {
        return;
    }

    let persist = claude_watcher_persist_callback(db, app_type, &provider.id);
    let watcher_provider = watcher_provider_from_effective(provider, effective_settings);
    let provider_arc = std::sync::Arc::new(std::sync::Mutex::new(watcher_provider));
    match crate::claude_settings_watcher::spawn_claude_settings_watcher(
        settings_path,
        provider_arc,
        persist,
    ) {
        // Ok(watcher) 必须交给 replace_watcher 存进进程单例，
        // 否则返回值在 match 表达式结束时被 Drop，notify 监听线程退出。
        Ok(watcher) => crate::claude_settings_watcher::replace_watcher(watcher),
        Err(e) => log::warn!("[ClaudeSettingsWatcher] spawn failed: {e}"),
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
        normalize_claude_models_in_value(&mut effective_settings);
        crate::claude_desktop_config::migrate_legacy_suffix_to_context_windows(
            &mut effective_settings,
        );
        restore_claude_subagent_local_marker_for_effective(&mut effective_settings, provider);
        // contextWindows 先注入；Kimi/Codex OAuth 固定兜底后写，避免 staticInjected 被误判为显式清掉。
        apply_context_window_defaults(&mut effective_settings, provider);
        apply_static_context_defaults(&mut effective_settings, provider);
    }

    Ok(effective_settings)
}

pub(crate) fn write_live_with_common_config_for_state(
    state: &AppState,
    app_type: &AppType,
    provider: &Provider,
) -> Result<(), AppError> {
    write_live_with_common_config_for_codex_oauth_manager(
        state.db.as_ref(),
        app_type,
        provider,
        &state.codex_oauth_manager,
    )
}

/// 历史兼容入口：不携带 Codex OAuth 管理器的 live 写，等价于合并前的
/// `write_live_with_common_config(db, ...)`；仅本文件测试使用。
#[cfg(test)]
pub(crate) fn write_live_with_common_config(
    db: &Database,
    app_type: &AppType,
    provider: &Provider,
) -> Result<(), AppError> {
    let effective_settings = build_effective_settings_with_common_config(db, app_type, provider)?;
    let mut effective_provider = provider.clone();
    effective_provider.settings_config = effective_settings;
    write_live_with_effective_provider(db, app_type, provider, effective_provider)
}

pub(crate) fn write_live_with_common_config_for_codex_oauth_manager(
    db: &Database,
    app_type: &AppType,
    provider: &Provider,
    codex_oauth_manager: &Arc<CodexOAuthManager>,
) -> Result<(), AppError> {
    let effective_provider = build_effective_provider_for_live_with_codex_oauth_manager(
        db,
        app_type,
        provider,
        codex_oauth_manager,
    )?;
    write_live_with_effective_provider(db, app_type, provider, effective_provider)
}

fn write_live_with_effective_provider(
    db: &Database,
    app_type: &AppType,
    provider: &Provider,
    mut effective_provider: Provider,
) -> Result<(), AppError> {
    if matches!(app_type, AppType::ClaudeDesktop) {
        crate::claude_desktop_config::apply_provider(db, &effective_provider)?;
        log::info!(
            "Claude Desktop 3P profile '{}' written for provider '{}'",
            crate::claude_desktop_config::PROFILE_ID,
            effective_provider.id
        );
        return Ok(());
    }

    if matches!(app_type, AppType::Claude) {
        strip_legacy_suffixes_from_claude_models(&mut effective_provider.settings_config);
    }

    write_live_snapshot(app_type, &effective_provider)?;

    if matches!(app_type, AppType::Claude) {
        let mut updated_config = provider.settings_config.clone();
        normalize_claude_models_in_value(&mut updated_config);
        crate::claude_desktop_config::migrate_legacy_suffix_to_context_windows(&mut updated_config);
        merge_auto_sync_state_into_provider(
            &mut updated_config,
            &effective_provider.settings_config,
        );
        db.update_provider_settings_config(app_type.as_str(), &provider.id, &updated_config)?;
    }

    if matches!(app_type, AppType::Claude) {
        ensure_claude_settings_watcher(db, app_type, provider, &effective_provider.settings_config);
    }

    Ok(())
}

pub(crate) fn build_effective_provider_for_live_with_codex_oauth_manager(
    db: &Database,
    app_type: &AppType,
    provider: &Provider,
    codex_oauth_manager: &Arc<CodexOAuthManager>,
) -> Result<Provider, AppError> {
    let mut effective_provider = provider.clone();
    effective_provider.settings_config =
        build_effective_settings_with_common_config(db, app_type, provider)?;
    apply_codex_managed_oauth_auth(app_type, &mut effective_provider, Some(codex_oauth_manager))?;
    Ok(effective_provider)
}

fn apply_codex_managed_oauth_auth(
    app_type: &AppType,
    provider: &mut Provider,
    codex_oauth_manager: Option<&Arc<CodexOAuthManager>>,
) -> Result<(), AppError> {
    if !matches!(app_type, AppType::Codex) || provider.category.as_deref() != Some("official") {
        return Ok(());
    }

    let Some(account_id) = provider
        .meta
        .as_ref()
        .and_then(|meta| meta.managed_account_id_for("codex_oauth"))
        .map(|id| id.trim().to_string())
        .filter(|id| !id.is_empty())
    else {
        return Ok(());
    };

    let Some(manager) = codex_oauth_manager else {
        return Err(AppError::Message(
            "Codex OAuth 托管账号不可用，请重启应用后重试".to_string(),
        ));
    };

    let auth = get_codex_managed_oauth_live_auth_value(manager.clone(), account_id.clone())?;

    let Some(settings_obj) = provider.settings_config.as_object_mut() else {
        return Err(AppError::Config(
            "Codex 供应商配置必须是 JSON 对象".to_string(),
        ));
    };

    settings_obj.insert("auth".to_string(), auth);
    Ok(())
}

/// 构建写入托管 Codex `auth.json` 的完整可刷新 auth（含 refresh_token + last_refresh）。
///
/// 步骤：
/// 1. **读回**：若 Codex CLI 已自行刷新并轮换 refresh_token，先采纳盘上最新值，避免
///    用陈腐 refresh_token 覆盖 CLI 的有效登录（反复切换场景）。
/// 2. 取有效 token 束（必要时刷新 access_token）。
/// 3. 按原生浏览器登录形状生成完整 auth。
///
/// 不再持有外层锁：manager 内部按账号加锁刷新，网络阻塞不会波及其他账号操作或
/// token 读取。
fn get_codex_managed_oauth_live_auth_value(
    manager: Arc<CodexOAuthManager>,
    account_id: String,
) -> Result<Value, AppError> {
    std::thread::spawn(move || {
        tauri::async_runtime::block_on(async move {
            if let Some((refresh_token, id_token, last_refresh_ms)) =
                crate::codex_config::read_codex_live_auth_refresh_for_account(&account_id)
            {
                if let Err(err) = manager
                    .adopt_account_refresh_token(
                        &account_id,
                        refresh_token,
                        id_token,
                        last_refresh_ms,
                    )
                    .await
                {
                    log::warn!(
                        "读回 Codex CLI 轮换后的 refresh_token 失败（account={account_id}）: {err}"
                    );
                }
            }

            let bundle = manager
                .get_valid_token_bundle_for_account(&account_id)
                .await
                .map_err(|err| {
                    format!(
                        "Codex OAuth 账号 {account_id} 认证失败，请重新登录 ChatGPT 账号: {err}"
                    )
                })?;
            let id_token = bundle
                .id_token
                .as_deref()
                .filter(|token| !token.trim().is_empty())
                .ok_or_else(|| {
                    format!(
                        "Codex OAuth 账号 {account_id} 缺少 id_token，请在认证中心重新登录后再保存"
                    )
                })?;

            Ok::<Value, String>(codex_managed_oauth_live_auth(
                &account_id,
                &bundle.access_token,
                Some(id_token),
                &bundle.refresh_token,
                &bundle.last_refresh,
            ))
        })
    })
    .join()
    .map_err(|_| AppError::Message("Codex OAuth token 获取线程异常退出".to_string()))?
    .map_err(AppError::Message)
}

/// Before replacing an outgoing managed account's live auth, adopt any Codex
/// CLI-rotated refresh generation and return the exact disk refresh token for
/// a compare-before-write check.
pub(crate) fn prepare_codex_managed_oauth_live_auth_switch_away(
    manager: Arc<CodexOAuthManager>,
    account_id: String,
) -> Result<Option<String>, AppError> {
    std::thread::spawn(move || {
        tauri::async_runtime::block_on(async move {
            manager
                .prepare_live_auth_for_account_switch_away(&account_id)
                .await
                .map_err(|error| error.to_string())
        })
    })
    .join()
    .map_err(|_| AppError::Message("Codex OAuth live 凭据采纳线程异常退出".to_string()))?
    .map_err(AppError::Message)
}

pub(crate) fn codex_managed_oauth_live_auth(
    account_id: &str,
    access_token: &str,
    id_token: Option<&str>,
    refresh_token: &str,
    last_refresh: &str,
) -> Value {
    // 与原生 Codex 浏览器登录的形状对齐：tokens 字段顺序 id_token、access_token、
    // refresh_token、account_id，并带顶层 last_refresh。**必须**包含 refresh_token，
    // 否则 Codex CLI 在 access_token 过期后无法自刷新（“裸跑 codex” 会静默失效）。
    crate::codex_config::codex_managed_oauth_auth_value(
        account_id,
        access_token,
        id_token,
        refresh_token,
        last_refresh,
    )
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
fn strip_injected_static_context_defaults(settings: &mut Value, provider: &Provider) {
    let Some((acw, max)) = static_context_window_fallback(provider) else {
        return;
    };
    let provider_env = provider
        .settings_config
        .get("env")
        .and_then(Value::as_object);
    let Some(env) = settings.get_mut("env").and_then(Value::as_object_mut) else {
        return;
    };
    for (key, default_value) in [
        ("CLAUDE_CODE_MAX_CONTEXT_TOKENS", max),
        ("CLAUDE_CODE_AUTO_COMPACT_WINDOW", acw),
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

/// 与 `apply_context_window_defaults` 对称：模型后缀自动注入的 ACW/MAX 只应活在
/// live，切走回填时必须剥掉；用户显式存储或手改过的 live 值保留。
fn strip_injected_model_suffix_context_defaults(settings: &mut Value, provider: &Provider) {
    let Some(target_window) =
        crate::claude_settings_watcher::resolve_active_model_window(settings, provider)
            .map(|active| active.window)
            .or_else(|| claude_context_window_target(provider))
    else {
        return;
    };
    let provider_env = provider
        .settings_config
        .get("env")
        .and_then(Value::as_object);
    let Some(env) = settings.get_mut("env").and_then(Value::as_object_mut) else {
        return;
    };
    let writes = crate::claude_settings_watcher::build_env_writes(
        target_window,
        crate::claude_settings_watcher::provider_compact_ratio(provider),
    );
    for (key, value) in writes {
        let stored_explicit = provider_env.is_some_and(|e| e.contains_key(key));
        if stored_explicit {
            continue;
        }
        if env.get(key).and_then(Value::as_str) == Some(value.as_str()) {
            env.remove(key);
        }
    }
}

fn provider_env_has_explicit_value(provider: &Provider, env_key: &str) -> bool {
    provider
        .settings_config
        .get("env")
        .and_then(Value::as_object)
        .and_then(|env| env.get(env_key))
        .is_some_and(|value| value.is_string() || value.is_number())
}

fn auto_sync_ledger_value_relevant(value: &Value) -> bool {
    !value.is_null() && value.as_bool() != Some(false)
}

fn has_auto_source_record(state: &serde_json::Map<String, Value>, short_key: &str) -> bool {
    ["lastWritten", "staticInjected"].iter().any(|source| {
        state
            .get(*source)
            .and_then(Value::as_object)
            .is_some_and(|source_obj| {
                source_obj
                    .get(short_key)
                    .is_some_and(auto_sync_ledger_value_relevant)
            })
    })
}

/// 任一自动来源（lastWritten / staticInjected）的该短键值是否等于 expected
/// （字符串严格相等，对齐 Rust as_str 语义）。
pub(crate) fn auto_source_value_matches(
    state: &serde_json::Map<String, Value>,
    short_key: &str,
    expected: &str,
) -> bool {
    ["lastWritten", "staticInjected"].iter().any(|source| {
        state
            .get(*source)
            .and_then(Value::as_object)
            .is_some_and(|source_obj| {
                source_obj.get(short_key).and_then(Value::as_str) == Some(expected)
            })
    })
}

fn backfill_auto_source_value(
    state: &serde_json::Map<String, Value>,
    short_key: &str,
    live_value: &str,
) -> bool {
    auto_source_value_matches(state, short_key, live_value)
}

fn strip_legacy_context_defaults_for_backfill(settings: &mut Value, provider: &Provider) {
    strip_injected_static_context_defaults(settings, provider);
    strip_injected_model_suffix_context_defaults(settings, provider);
}

/// 只对单个 env key 套用旧兜底逻辑；隔离该键避免误删账本已覆盖的另一键。
fn strip_legacy_context_default_for_backfill(
    settings: &mut Value,
    provider: &Provider,
    env_key: &str,
) {
    let mut isolated = settings.clone();
    if let Some(env) = isolated.get_mut("env").and_then(Value::as_object_mut) {
        let value = env.get(env_key).cloned();
        env.clear();
        if let Some(value) = value {
            env.insert(env_key.to_string(), value);
        }
    }
    strip_legacy_context_defaults_for_backfill(&mut isolated, provider);
    let removed = isolated
        .get("env")
        .and_then(Value::as_object)
        .map(|env| !env.contains_key(env_key))
        .unwrap_or(true);
    if removed {
        if let Some(env) = settings.get_mut("env").and_then(Value::as_object_mut) {
            env.remove(env_key);
        }
    }
}

/// 优先按 autoSyncState 逐键判定：provider env 显式键保留，
/// lastWritten/staticInjected 匹配时删除；该键没有任何自动来源记录时退回旧的窗口剥离逻辑。
/// provider env 中显式存在的字符串键也视为用户存储，始终保留。
fn strip_auto_synced_context_defaults(settings: &mut Value, provider: &Provider) {
    let Some(state) = provider
        .settings_config
        .get("autoSyncState")
        .and_then(Value::as_object)
    else {
        strip_legacy_context_defaults_for_backfill(settings, provider);
        return;
    };
    let mut remove_keys = Vec::new();
    let mut legacy_keys = Vec::new();
    {
        let Some(env) = settings.get("env").and_then(Value::as_object) else {
            return;
        };
        for (env_key, short_key) in crate::claude_desktop_config::CLAUDE_CONTEXT_WINDOW_LEDGER_KEYS
        {
            let Some(live_value) = env.get(env_key).and_then(Value::as_str) else {
                continue;
            };
            if provider_env_has_explicit_value(provider, env_key) {
                continue;
            }
            if backfill_auto_source_value(state, short_key, live_value) {
                remove_keys.push(env_key);
            } else if !has_auto_source_record(state, short_key) {
                legacy_keys.push(env_key);
            }
        }
    }
    if let Some(env) = settings.get_mut("env").and_then(Value::as_object_mut) {
        for env_key in remove_keys {
            env.remove(env_key);
        }
    }
    for env_key in legacy_keys {
        strip_legacy_context_default_for_backfill(settings, provider, env_key);
    }
}

fn restore_claude_internal_fields_for_backfill(settings: &mut Value, provider: &Provider) {
    // provider 可能仍只有 legacy 模型后缀，写 live 时 sanitize 会把迁移后的
    // contextWindows 剥掉；回填时必须基于迁移结果恢复窗口信息。
    let mut migrated = provider.settings_config.clone();
    crate::claude_desktop_config::migrate_legacy_suffix_to_context_windows(&mut migrated);

    let provider_env = migrated.get("env").and_then(Value::as_object);
    let live_env = settings.get("env").and_then(Value::as_object);
    let mut restore_models = Vec::new();
    for key in CLAUDE_MODEL_ENV_KEYS {
        let Some(stored_model) = provider_env
            .and_then(|e| e.get(key))
            .and_then(Value::as_str)
        else {
            continue;
        };
        let Some(live_model) = live_env.and_then(|e| e.get(key)).and_then(Value::as_str) else {
            continue;
        };
        let stripped = crate::proxy::model_mapper::strip_one_m_suffix_for_upstream(stored_model);
        if stripped == live_model {
            restore_models.push((key.to_string(), stripped.to_string()));
        }
    }
    let Some(obj) = settings.as_object_mut() else {
        return;
    };
    if let Some(env) = obj.get_mut("env").and_then(Value::as_object_mut) {
        for (key, value) in restore_models {
            env.insert(key, Value::String(value));
        }
    }
    if let Some(value) = provider.settings_config.get("autoSyncContextWindow") {
        obj.insert("autoSyncContextWindow".to_string(), value.clone());
    }
    if let Some(value) = provider.settings_config.get("autoSyncCompactRatio") {
        obj.insert("autoSyncCompactRatio".to_string(), value.clone());
    }
    if let Some(value) = migrated.get("contextWindows") {
        obj.insert("contextWindows".to_string(), value.clone());
    }
    // display-only K/M 原样串：live 经 sanitize 不携带，直连切走回填时必须从
    // DB 恢复，否则每切一次供应商就把上一个供应商的原样串清掉。
    if let Some(value) = provider.settings_config.get("contextWindowTokens") {
        obj.insert("contextWindowTokens".to_string(), value.clone());
    }
    if let Some(value) = provider.settings_config.get("autoSyncState") {
        obj.insert("autoSyncState".to_string(), value.clone());
    }
}

fn restore_live_settings_for_provider_backfill(
    app_type: &AppType,
    provider: &Provider,
    live_settings: Value,
) -> Value {
    if matches!(app_type, AppType::Claude) {
        let mut settings = live_settings;
        restore_claude_internal_fields_for_backfill(&mut settings, provider);
        strip_auto_synced_context_defaults(&mut settings, provider);
        strip_legacy_suffixes_from_claude_models(&mut settings);
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

    strip_codex_managed_oauth_auth_for_backfill(provider, &mut settings);

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

/// 回填（backfill）托管 Codex 官方 provider 时，剥离 live 的 `auth`。
///
/// 托管 provider 的存储配置**永远不应**持久化真实 OAuth token：token 由
/// `CodexOAuthManager` 按账号集中保管，provider 配置只保留绑定与占位 auth。
///
/// 因此无论 live 里当前是什么（我们写入的托管 auth、被 CLI 轮换过的 token、
/// 还是用户自己浏览器登录的原生 auth，后者含真实 access/refresh_token），
/// 都统一替换为 provider 存储的占位 auth，避免把真实凭据回填进 DB 配置。
fn strip_codex_managed_oauth_auth_for_backfill(provider: &Provider, settings: &mut Value) {
    let is_managed = provider
        .meta
        .as_ref()
        .and_then(|meta| meta.managed_account_id_for("codex_oauth"))
        .is_some();
    if !is_managed {
        return;
    }

    // live 里没有 auth 就无需处理。
    if settings.get("auth").is_none() {
        return;
    }

    let stored_auth = provider
        .settings_config
        .get("auth")
        .filter(|auth| auth.is_object())
        .cloned()
        .unwrap_or_else(|| json!({}));
    if let Some(obj) = settings.as_object_mut() {
        obj.insert("auth".to_string(), stored_auth);
    }
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
            if provider
                .meta
                .as_ref()
                .and_then(|meta| meta.managed_account_id_for("codex_oauth"))
                .is_some()
            {
                crate::codex_config::record_codex_managed_oauth_live_auth(auth)?;
            }
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
        AppType::Pi => {
            return Err(AppError::InvalidInput(
                "Pi providers use the Pi provider service".to_string(),
            ));
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

        if let Err(e) = write_live_with_common_config_for_state(state, app_type, provider) {
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
            write_live_with_common_config_for_state(state, app_type, provider)?;
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
            write_live_with_common_config_for_state(state, app_type, provider)?;
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

    write_live_with_common_config_for_state(state, app_type, provider)
}

/// Sync current provider to live configuration
///
/// 使用有效的当前供应商 ID（验证过存在性）。
/// 优先从本地 settings 读取，验证后 fallback 到数据库的 is_current 字段。
/// 这确保了配置导入后无效 ID 会自动 fallback 到数据库。
///
/// For additive mode apps (OpenCode), all providers are synced instead of just the current one.
pub fn sync_current_to_live(state: &AppState) -> Result<(), AppError> {
    let mut failures = Vec::new();

    // Sync providers based on mode
    for app_type in AppType::all() {
        if matches!(app_type, AppType::Pi) {
            continue;
        }
        let result = if app_type.is_additive_mode() {
            // Additive mode: sync ALL providers
            sync_all_providers_to_live(state, &app_type)
        } else {
            // Switch mode: sync only current provider. During proxy takeover,
            // update the restore backup instead of rewriting the taken-over
            // live file.
            sync_current_provider_for_app_respecting_takeover(state, &app_type)
        };

        if let Err(error) = result {
            log::warn!("同步 Provider 到 {app_type:?} 失败: {error}");
            failures.push(format!("provider/{}: {error}", app_type.as_str()));
        }
    }

    // MCP sync is already best-effort per application. Preserve its aggregate
    // error while continuing with Skills.
    if let Err(error) = McpService::sync_all_enabled(state) {
        failures.push(format!("mcp: {error}"));
    }

    // Skill sync
    for app_type in AppType::all() {
        if let Err(e) = crate::services::skill::SkillService::sync_to_app(&state.db, &app_type) {
            log::warn!("同步 Skill 到 {app_type:?} 失败: {e}");
            failures.push(format!("skill/{}: {e}", app_type.as_str()));
        }
    }

    if failures.is_empty() {
        Ok(())
    } else {
        Err(AppError::Message(format!(
            "部分 live 配置同步失败: {}",
            failures.join("; ")
        )))
    }
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
        AppType::Pi => Err(AppError::InvalidInput(
            "Pi providers are read from Pi's native models file".to_string(),
        )),
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
        AppType::OpenCode | AppType::OpenClaw | AppType::Hermes | AppType::Pi => {
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
    use crate::provider::{AuthBinding, AuthBindingSource, ProviderMeta};
    use serde_json::json;
    use serial_test::serial;
    use std::env;
    use tempfile::TempDir;

    struct TempHome {
        #[allow(dead_code)]
        dir: TempDir,
        original_home: Option<String>,
        original_userprofile: Option<String>,
        original_test_home: Option<String>,
    }

    impl TempHome {
        fn new() -> Self {
            let dir = TempDir::new().expect("failed to create temp home");
            let original_home = env::var("HOME").ok();
            let original_userprofile = env::var("USERPROFILE").ok();
            let original_test_home = env::var("CC_SWITCH_TEST_HOME").ok();

            env::set_var("HOME", dir.path());
            env::set_var("USERPROFILE", dir.path());
            env::set_var("CC_SWITCH_TEST_HOME", dir.path());

            Self {
                dir,
                original_home,
                original_userprofile,
                original_test_home,
            }
        }
    }

    impl Drop for TempHome {
        fn drop(&mut self) {
            match &self.original_home {
                Some(value) => env::set_var("HOME", value),
                None => env::remove_var("HOME"),
            }
            match &self.original_userprofile {
                Some(value) => env::set_var("USERPROFILE", value),
                None => env::remove_var("USERPROFILE"),
            }
            match &self.original_test_home {
                Some(value) => env::set_var("CC_SWITCH_TEST_HOME", value),
                None => env::remove_var("CC_SWITCH_TEST_HOME"),
            }
        }
    }

    struct EnvVarGuard {
        key: &'static str,
        previous: Option<std::ffi::OsString>,
    }

    impl EnvVarGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let previous = env::var_os(key);
            env::set_var(key, value);
            Self { key, previous }
        }

        fn remove(key: &'static str) -> Self {
            let previous = env::var_os(key);
            env::remove_var(key);
            Self { key, previous }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            match self.previous.take() {
                Some(value) => env::set_var(self.key, value),
                None => env::remove_var(self.key),
            }
        }
    }

    struct WatcherSlotGuard;

    impl Drop for WatcherSlotGuard {
        fn drop(&mut self) {
            crate::claude_settings_watcher::clear_watcher_slot_for_tests();
        }
    }

    #[test]
    #[serial]
    fn build_effective_settings_does_not_spawn_watcher_in_tests() {
        let _home = TempHome::new();
        crate::claude_settings_watcher::clear_watcher_slot_for_tests();
        let db = Database::memory().expect("create memory db");
        let provider =
            Provider::with_id("p".to_string(), "P".to_string(), json!({ "env": {} }), None);
        let settings_path = get_claude_settings_path();
        assert!(!settings_path.parent().unwrap().exists());
        let _settings =
            build_effective_settings_with_common_config(&db, &AppType::Claude, &provider)
                .expect("build effective settings");
        assert!(
            crate::claude_settings_watcher::watcher_slot_is_empty_for_tests(),
            "settings builder must not spawn or replace the Claude watcher"
        );
        assert!(
            !settings_path.parent().unwrap().exists(),
            "settings builder must not create the watcher parent directory"
        );
    }

    #[test]
    #[serial]
    fn write_live_with_common_config_respects_disable_watcher_env() {
        let _home = TempHome::new();
        let _disable_watcher = EnvVarGuard::set("CC_SWITCH_DISABLE_WATCHER", "1");
        let _test_watcher = EnvVarGuard::set("CC_SWITCH_TEST_WATCHER", "1");
        let _slot_guard = WatcherSlotGuard;
        crate::claude_settings_watcher::clear_watcher_slot_for_tests();
        let db = Database::memory().expect("create memory db");
        let provider = Provider::with_id(
            "disabled-watcher".to_string(),
            "Disabled Watcher".to_string(),
            json!({ "env": {} }),
            None,
        );
        db.save_provider(AppType::Claude.as_str(), &provider)
            .expect("save provider");
        write_live_with_common_config(&db, &AppType::Claude, &provider).expect("write live");
        assert!(get_claude_settings_path().exists());
        assert!(
            crate::claude_settings_watcher::watcher_slot_is_empty_for_tests(),
            "CC_SWITCH_DISABLE_WATCHER=1 must prevent watcher spawn"
        );
    }

    #[test]
    #[serial]
    fn write_live_with_common_config_does_not_spawn_watcher_by_default_in_tests() {
        let _home = TempHome::new();
        let _test_watcher_unset = EnvVarGuard::remove("CC_SWITCH_TEST_WATCHER");
        crate::claude_settings_watcher::clear_watcher_slot_for_tests();
        let db = Database::memory().expect("create memory db");
        let provider = Provider::with_id(
            "default-no-watcher".to_string(),
            "Default No Watcher".to_string(),
            json!({ "env": {} }),
            None,
        );
        db.save_provider(AppType::Claude.as_str(), &provider)
            .expect("save provider");
        write_live_with_common_config(&db, &AppType::Claude, &provider).expect("write live");
        assert!(
            crate::claude_settings_watcher::watcher_slot_is_empty_for_tests(),
            "tests must not spawn or replace the Claude watcher by default"
        );
    }

    #[test]
    #[serial]
    fn write_live_with_common_config_starts_watcher_with_test_opt_in() {
        let _home = TempHome::new();
        let _test_watcher = EnvVarGuard::set("CC_SWITCH_TEST_WATCHER", "1");
        let _slot_guard = WatcherSlotGuard;
        let _disable_watcher_removed = EnvVarGuard::remove("CC_SWITCH_DISABLE_WATCHER");
        crate::claude_settings_watcher::clear_watcher_slot_for_tests();
        let db = Database::memory().expect("create memory db");
        let provider = Provider::with_id(
            "opt-in-watcher".to_string(),
            "Opt In Watcher".to_string(),
            json!({ "env": {} }),
            None,
        );
        db.save_provider(AppType::Claude.as_str(), &provider)
            .expect("save provider");
        write_live_with_common_config(&db, &AppType::Claude, &provider).expect("write live");
        assert!(
            !crate::claude_settings_watcher::watcher_slot_is_empty_for_tests(),
            "CC_SWITCH_TEST_WATCHER=1 must allow watcher spawn after write live"
        );
    }

    #[test]
    fn watcher_provider_snapshot_uses_effective_settings_and_keeps_provider_fields() {
        let mut provider = Provider::with_id(
            "p".to_string(),
            "P".to_string(),
            json!({ "env": { "ANTHROPIC_MODEL": "raw-model" } }),
            Some("https://example.com".to_string()),
        );
        provider.meta = Some(crate::provider::ProviderMeta {
            provider_type: Some("codex_oauth".to_string()),
            ..Default::default()
        });
        let effective = json!({
            "env": {
                "ANTHROPIC_MODEL": "gpt-5.6",
                "CLAUDE_CODE_AUTO_COMPACT_WINDOW": "372000",
                "CLAUDE_CODE_MAX_CONTEXT_TOKENS": "372000"
            },
            "autoSyncState": { "staticInjected": { "ACW": "372000", "MAX": "372000" } }
        });

        let snapshot = watcher_provider_from_effective(&provider, &effective);

        assert_eq!(snapshot.id, "p");
        assert_eq!(snapshot.name, "P");
        assert_eq!(snapshot.website_url.as_deref(), Some("https://example.com"));
        assert_eq!(
            snapshot
                .meta
                .as_ref()
                .and_then(|meta| meta.provider_type.as_deref()),
            Some("codex_oauth")
        );
        assert_eq!(snapshot.settings_config, effective);
        assert_eq!(
            provider.settings_config["env"]["ANTHROPIC_MODEL"],
            "raw-model"
        );
    }

    #[test]
    #[serial]
    fn claude_watcher_persist_callback_writes_auto_sync_state_to_db() {
        let _home = TempHome::new();
        let db = Database::init().expect("file database");
        let provider =
            Provider::with_id("p".to_string(), "P".to_string(), json!({ "env": {} }), None);
        db.save_provider(AppType::Claude.as_str(), &provider)
            .expect("save provider");
        let persist = claude_watcher_persist_callback(&db, &AppType::Claude, &provider.id);
        let settings = json!({
            "env": {},
            "autoSyncState": {
                "lastWritten": { "ACW": "1", "MAX": "2" },
                "userExplicit": { "ACW": "1", "MAX": "2" }
            }
        });
        persist(settings).expect("persist settings");
        let stored = db
            .get_provider_by_id("p", AppType::Claude.as_str())
            .expect("get provider")
            .expect("stored provider exists");
        assert_eq!(
            stored.settings_config["autoSyncState"]["lastWritten"],
            json!({ "ACW": "1", "MAX": "2" })
        );
        assert_eq!(
            stored.settings_config["autoSyncState"]["userExplicit"],
            json!({ "ACW": "1", "MAX": "2" })
        );
    }

    #[test]
    #[serial]
    fn claude_watcher_persist_callback_merges_only_auto_sync_state_into_db() {
        let _home = TempHome::new();
        let db = Database::init().expect("file database");
        let provider = Provider::with_id(
            "p".to_string(),
            "P".to_string(),
            json!({
                "env": {
                    "ANTHROPIC_AUTH_TOKEN": "db-token",
                    "ANTHROPIC_MODEL": "db-model"
                },
                "autoSyncContextWindow": true,
                "contextWindows": { "ANTHROPIC_DEFAULT_SONNET_MODEL": 200000 },
                "autoSyncState": {
                    "extraState": { "keep": true },
                    "lastWritten": { "ACW": "1", "MAX": "2" }
                }
            }),
            None,
        );
        db.save_provider(AppType::Claude.as_str(), &provider)
            .expect("save provider");
        let persist = claude_watcher_persist_callback(&db, &AppType::Claude, &provider.id);
        let watcher_settings = json!({
            "env": {
                "ANTHROPIC_AUTH_TOKEN": "stale-token",
                "ANTHROPIC_MODEL": "stale-model"
            },
            "autoSyncContextWindow": false,
            "contextWindows": {},
            "autoSyncState": {
                "lastWritten": { "ACW": "160000", "MAX": "200000" },
                "userExplicit": {}
            }
        });
        persist(watcher_settings).expect("persist settings");
        let stored = db
            .get_provider_by_id("p", AppType::Claude.as_str())
            .expect("get provider")
            .expect("stored provider exists");
        assert_eq!(
            stored.settings_config["env"]["ANTHROPIC_AUTH_TOKEN"],
            "db-token"
        );
        assert_eq!(stored.settings_config["env"]["ANTHROPIC_MODEL"], "db-model");
        assert_eq!(stored.settings_config["autoSyncContextWindow"], json!(true));
        assert_eq!(
            stored.settings_config["contextWindows"]["ANTHROPIC_DEFAULT_SONNET_MODEL"],
            200000
        );
        assert_eq!(
            stored.settings_config["autoSyncState"]["lastWritten"],
            json!({ "ACW": "160000", "MAX": "200000" })
        );
        assert_eq!(
            stored.settings_config["autoSyncState"]["userExplicit"],
            json!({})
        );
        assert_eq!(
            stored.settings_config["autoSyncState"]["extraState"],
            json!({ "keep": true })
        );
    }

    #[test]
    fn direct_live_uses_context_windows_and_keeps_model_clean() {
        let provider = Provider::with_id(
            "p".into(),
            "P".into(),
            json!({
                "env": {
                    "ANTHROPIC_MODEL": "deepseek-v4-pro",
                    "ANTHROPIC_DEFAULT_SONNET_MODEL": "glm-5.2"
                },
                "contextWindows": {
                    "ANTHROPIC_DEFAULT_SONNET_MODEL": 200000
                }
            }),
            None,
        );
        let db = Database::memory().unwrap();
        let effective =
            build_effective_settings_with_common_config(&db, &AppType::Claude, &provider).unwrap();
        assert_eq!(effective["env"]["CLAUDE_CODE_MAX_CONTEXT_TOKENS"], "200000");
        assert_eq!(
            effective["env"]["ANTHROPIC_DEFAULT_SONNET_MODEL"],
            "glm-5.2"
        );
    }

    #[test]
    fn restore_subagent_local_marker_only_for_one_million_window() {
        let provider_200k = Provider::with_id(
            "p-200k".to_string(),
            "P 200k".to_string(),
            json!({
                "env": {
                    "CLAUDE_CODE_SUBAGENT_MODEL": "subagent-model[200k]"
                },
                "contextWindows": { "CLAUDE_CODE_SUBAGENT_MODEL": 200000 }
            }),
            None,
        );
        let mut effective_200k = json!({
            "env": { "CLAUDE_CODE_SUBAGENT_MODEL": "subagent-model" }
        });
        restore_claude_subagent_local_marker_for_effective(&mut effective_200k, &provider_200k);
        assert_eq!(
            effective_200k["env"]["CLAUDE_CODE_SUBAGENT_MODEL"],
            "subagent-model"
        );

        let provider_1m = Provider::with_id(
            "p-1m".to_string(),
            "P 1M".to_string(),
            json!({
                "env": {
                    "CLAUDE_CODE_SUBAGENT_MODEL": "subagent-model[1M]"
                },
                "contextWindows": { "CLAUDE_CODE_SUBAGENT_MODEL": 1000000 }
            }),
            None,
        );
        let mut effective_1m = json!({
            "env": { "CLAUDE_CODE_SUBAGENT_MODEL": "subagent-model" }
        });
        restore_claude_subagent_local_marker_for_effective(&mut effective_1m, &provider_1m);
        assert_eq!(
            effective_1m["env"]["CLAUDE_CODE_SUBAGENT_MODEL"],
            "subagent-model[1M]"
        );
    }

    #[test]
    fn restore_subagent_local_marker_uses_context_windows_when_env_is_clean() {
        let provider_200k = Provider::with_id(
            "p-clean-200k".to_string(),
            "P Clean 200k".to_string(),
            json!({
                "env": { "CLAUDE_CODE_SUBAGENT_MODEL": "subagent-model" },
                "contextWindows": { "CLAUDE_CODE_SUBAGENT_MODEL": 200000 }
            }),
            None,
        );
        let mut effective_200k = json!({
            "env": { "CLAUDE_CODE_SUBAGENT_MODEL": "subagent-model" }
        });
        restore_claude_subagent_local_marker_for_effective(&mut effective_200k, &provider_200k);
        assert_eq!(
            effective_200k["env"]["CLAUDE_CODE_SUBAGENT_MODEL"],
            "subagent-model"
        );

        let provider_1m = Provider::with_id(
            "p-clean-1m".to_string(),
            "P Clean 1M".to_string(),
            json!({
                "env": { "CLAUDE_CODE_SUBAGENT_MODEL": "subagent-model" },
                "contextWindows": { "CLAUDE_CODE_SUBAGENT_MODEL": 1000000 }
            }),
            None,
        );
        let mut effective_1m = json!({
            "env": { "CLAUDE_CODE_SUBAGENT_MODEL": "subagent-model" }
        });
        restore_claude_subagent_local_marker_for_effective(&mut effective_1m, &provider_1m);
        assert_eq!(
            effective_1m["env"]["CLAUDE_CODE_SUBAGENT_MODEL"],
            "subagent-model[1M]"
        );
    }

    #[test]
    #[serial]
    fn direct_live_writes_clean_models_and_persists_static_injected() {
        let _home = TempHome::new();
        let db = Database::memory().expect("create memory db");
        let provider = Provider::with_id(
            "p".to_string(),
            "P".to_string(),
            json!({
                "env": {
                    "ANTHROPIC_MODEL": "fallback-model[1M]",
                    "ANTHROPIC_DEFAULT_HAIKU_MODEL": "haiku-model",
                    "ANTHROPIC_DEFAULT_SONNET_MODEL": "glm-5.2[200k]",
                    "ANTHROPIC_DEFAULT_OPUS_MODEL": "opus-model",
                    "ANTHROPIC_DEFAULT_FABLE_MODEL": "fable-model",
                    "CLAUDE_CODE_SUBAGENT_MODEL": "subagent-model[1M]"
                },
                "contextWindows": {
                    "ANTHROPIC_DEFAULT_SONNET_MODEL": 200000,
                    "CLAUDE_CODE_SUBAGENT_MODEL": 1000000
                },
                "autoSyncState": {
                    "lastWritten": { "model": "sonnet" },
                    "userExplicit": { "legacy": true }
                }
            }),
            None,
        );
        db.save_provider(AppType::Claude.as_str(), &provider)
            .expect("save provider");
        write_live_with_common_config(&db, &AppType::Claude, &provider).expect("write live");

        let live: Value = read_json_file(&get_claude_settings_path()).expect("read live");
        assert_eq!(live["env"]["ANTHROPIC_MODEL"], "fallback-model");
        assert_eq!(live["env"]["ANTHROPIC_DEFAULT_SONNET_MODEL"], "glm-5.2");
        assert_eq!(live["env"]["CLAUDE_CODE_SUBAGENT_MODEL"], "subagent-model");
        assert_eq!(live["env"]["CLAUDE_CODE_MAX_CONTEXT_TOKENS"], "1000000");
        assert_eq!(live["env"]["CLAUDE_CODE_AUTO_COMPACT_WINDOW"], "950000");
        assert!(live.get("contextWindows").is_none());
        assert!(live.get("autoSyncState").is_none());

        let stored = db
            .get_provider_by_id("p", AppType::Claude.as_str())
            .expect("get stored provider")
            .expect("stored provider exists");
        assert_eq!(
            stored.settings_config["env"]["ANTHROPIC_MODEL"],
            "fallback-model"
        );
        assert_eq!(
            stored.settings_config["autoSyncState"]["lastWritten"],
            json!({ "model": "sonnet" })
        );
        assert_eq!(
            stored.settings_config["autoSyncState"]["userExplicit"],
            json!({ "legacy": true })
        );
        assert_eq!(
            stored.settings_config["autoSyncState"]["staticInjected"],
            json!({ "ACW": "950000", "MAX": "1000000" })
        );
        assert_eq!(
            stored.settings_config["env"]["ANTHROPIC_DEFAULT_SONNET_MODEL"],
            "glm-5.2"
        );
        assert_eq!(
            stored.settings_config["env"]["CLAUDE_CODE_SUBAGENT_MODEL"],
            "subagent-model"
        );
        assert_eq!(
            stored.settings_config["contextWindows"]["ANTHROPIC_MODEL"],
            json!(1000000)
        );
        assert_eq!(
            stored.settings_config["contextWindows"]["ANTHROPIC_DEFAULT_SONNET_MODEL"],
            json!(200000)
        );
        assert_eq!(
            stored.settings_config["contextWindows"]["CLAUDE_CODE_SUBAGENT_MODEL"],
            json!(1000000)
        );
    }

    #[test]
    #[serial]
    fn write_live_with_common_config_persists_legacy_suffix_migration_to_db() {
        let _home = TempHome::new();
        let _test_watcher_unset = EnvVarGuard::remove("CC_SWITCH_TEST_WATCHER");
        crate::claude_settings_watcher::clear_watcher_slot_for_tests();
        let db = Database::memory().expect("create memory db");
        let provider = Provider::with_id(
            "legacy-db".to_string(),
            "Legacy DB".to_string(),
            json!({
                "env": {
                    "ANTHROPIC_MODEL": "fallback-model[1M]",
                    "ANTHROPIC_DEFAULT_SONNET_MODEL": "glm-5.2[200k]"
                }
            }),
            None,
        );
        db.save_provider(AppType::Claude.as_str(), &provider)
            .expect("save provider");

        write_live_with_common_config(&db, &AppType::Claude, &provider).expect("write live");

        let stored = db
            .get_provider_by_id("legacy-db", AppType::Claude.as_str())
            .expect("get provider")
            .expect("stored provider exists");
        assert_eq!(
            stored.settings_config["env"]["ANTHROPIC_MODEL"],
            "fallback-model"
        );
        assert_eq!(
            stored.settings_config["env"]["ANTHROPIC_DEFAULT_SONNET_MODEL"],
            "glm-5.2"
        );
        assert_eq!(
            stored.settings_config["contextWindows"]["ANTHROPIC_MODEL"],
            json!(1000000)
        );
        assert_eq!(
            stored.settings_config["contextWindows"]["ANTHROPIC_DEFAULT_SONNET_MODEL"],
            json!(200000)
        );
    }

    #[test]
    #[serial]
    fn write_live_with_common_config_does_not_materialize_auto_sync_from_ledger() {
        // 方案 A：provider 同时带 contextWindows（触发静态注入记 staticInjected）与
        // autoSyncState.lastWritten 账本，正是旧 F5 物化 bug 的两个触发源；开关字段
        // 缺失即保持缺失，防止推断逻辑被未来重构重新引入。
        let _home = TempHome::new();
        let _test_watcher_unset = EnvVarGuard::remove("CC_SWITCH_TEST_WATCHER");
        crate::claude_settings_watcher::clear_watcher_slot_for_tests();
        let db = Database::memory().expect("create memory db");
        let provider = Provider::with_id(
            "no-materialize".to_string(),
            "No Materialize".to_string(),
            json!({
                "env": { "ANTHROPIC_DEFAULT_SONNET_MODEL": "glm-5.2" },
                "contextWindows": { "ANTHROPIC_DEFAULT_SONNET_MODEL": 200000 },
                "autoSyncState": {
                    "lastWritten": { "ACW": "190000", "MAX": "200000" }
                }
            }),
            None,
        );
        // ① 写入前开关字段缺失
        assert!(provider
            .settings_config
            .get("autoSyncContextWindow")
            .is_none());
        db.save_provider(AppType::Claude.as_str(), &provider)
            .expect("save provider");

        // ② 写 live 成功
        write_live_with_common_config(&db, &AppType::Claude, &provider).expect("write live");

        // ③ 读回 DB，开关字段仍缺失，未被物化成 true
        let stored = db
            .get_provider_by_id("no-materialize", AppType::Claude.as_str())
            .expect("get provider")
            .expect("stored provider exists");
        assert!(
            stored
                .settings_config
                .get("autoSyncContextWindow")
                .is_none(),
            "contextWindows + lastWritten 账本不应把 autoSyncContextWindow 物化成 true"
        );
    }

    #[test]
    #[serial]
    fn write_live_with_common_config_normalizes_small_fast_legacy_suffix() {
        let _home = TempHome::new();
        let _test_watcher_unset = EnvVarGuard::remove("CC_SWITCH_TEST_WATCHER");
        crate::claude_settings_watcher::clear_watcher_slot_for_tests();
        let db = Database::memory().expect("create memory db");
        let provider = Provider::with_id(
            "small-fast-legacy".to_string(),
            "Small Fast Legacy".to_string(),
            json!({
                "env": { "ANTHROPIC_SMALL_FAST_MODEL": "fallback-model[1M]" }
            }),
            None,
        );
        db.save_provider(AppType::Claude.as_str(), &provider)
            .expect("save provider");

        write_live_with_common_config(&db, &AppType::Claude, &provider).expect("write live");

        let live: Value = read_json_file(&get_claude_settings_path()).expect("read live");
        assert!(live["env"].get("ANTHROPIC_SMALL_FAST_MODEL").is_none());
        assert_eq!(
            live["env"]["ANTHROPIC_DEFAULT_HAIKU_MODEL"],
            "fallback-model"
        );
        assert_eq!(
            live["env"]["ANTHROPIC_DEFAULT_SONNET_MODEL"],
            "fallback-model"
        );
        assert_eq!(
            live["env"]["ANTHROPIC_DEFAULT_OPUS_MODEL"],
            "fallback-model"
        );
        assert_eq!(live["env"]["CLAUDE_CODE_MAX_CONTEXT_TOKENS"], "1000000");

        let stored = db
            .get_provider_by_id("small-fast-legacy", AppType::Claude.as_str())
            .expect("get provider")
            .expect("stored provider exists");
        assert!(stored.settings_config["env"]
            .get("ANTHROPIC_SMALL_FAST_MODEL")
            .is_none());
        assert_eq!(
            stored.settings_config["env"]["ANTHROPIC_DEFAULT_SONNET_MODEL"],
            "fallback-model"
        );
        assert_eq!(
            stored.settings_config["contextWindows"]["ANTHROPIC_DEFAULT_HAIKU_MODEL"],
            json!(1000000)
        );
        assert_eq!(
            stored.settings_config["contextWindows"]["ANTHROPIC_DEFAULT_SONNET_MODEL"],
            json!(1000000)
        );
        assert_eq!(
            stored.settings_config["contextWindows"]["ANTHROPIC_DEFAULT_OPUS_MODEL"],
            json!(1000000)
        );
    }

    #[test]
    fn merge_auto_sync_state_into_provider_preserves_db_metadata() {
        let mut stored = json!({
            "env": { "ANTHROPIC_MODEL": "fallback-model[1M]" },
            "autoSyncState": {
                "lastWritten": { "model": "sonnet" },
                "userExplicit": { "CLAUDE_CODE_MAX_CONTEXT_TOKENS": true }
            }
        });
        let effective = json!({
            "autoSyncState": {
                "staticInjected": { "ACW": "800000", "MAX": "1000000" }
            }
        });
        merge_auto_sync_state_into_provider(&mut stored, &effective);
        assert_eq!(
            stored["autoSyncState"]["lastWritten"],
            json!({ "model": "sonnet" })
        );
        assert_eq!(
            stored["autoSyncState"]["userExplicit"],
            json!({ "CLAUDE_CODE_MAX_CONTEXT_TOKENS": true })
        );
        assert_eq!(
            stored["autoSyncState"]["staticInjected"],
            json!({ "ACW": "800000", "MAX": "1000000" })
        );
    }

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
        // 用户显式 ACW 保留；MAX 缺省时仍补静态默认 262144
        assert_eq!(
            effective["env"]["CLAUDE_CODE_MAX_CONTEXT_TOKENS"],
            json!("262144")
        );
        assert_eq!(
            effective["env"]["CLAUDE_CODE_AUTO_COMPACT_WINDOW"],
            json!("262144")
        );
        assert!(
            effective["autoSyncState"].get("staticInjected").is_none(),
            "user explicit ACW must suppress staticInjected"
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
        assert!(
            effective["autoSyncState"].get("staticInjected").is_none(),
            "user explicit ACW/MAX must not be marked as staticInjected"
        );
    }

    #[test]
    fn kimi_restores_static_262144_injection() {
        let db = Database::memory().expect("create memory db");
        let provider = Provider::with_id(
            "kimi".to_string(),
            "Kimi".to_string(),
            json!({
                "env": { "ANTHROPIC_BASE_URL": "https://api.kimi.com/coding/" }
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
        assert_eq!(
            effective["autoSyncState"]["staticInjected"],
            json!({ "ACW": "262144", "MAX": "262144" })
        );
    }

    #[test]
    fn kimi_static_injection_wins_over_context_windows() {
        let db = Database::memory().expect("create memory db");
        let provider = Provider::with_id(
            "kimi-with-context-windows".to_string(),
            "Kimi With Context Windows".to_string(),
            json!({
                "env": {
                    "ANTHROPIC_BASE_URL": "https://api.kimi.com/coding/",
                    "ANTHROPIC_MODEL": "kimi-for-coding"
                },
                "contextWindows": { "ANTHROPIC_MODEL": 800000 }
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
        assert_eq!(
            effective["autoSyncState"]["staticInjected"],
            json!({ "ACW": "262144", "MAX": "262144" })
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
        assert_eq!(
            effective["autoSyncState"]["staticInjected"],
            json!({ "ACW": "372000", "MAX": "372000" })
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
        assert!(
            effective["autoSyncState"].get("staticInjected").is_none(),
            "user explicit ACW/MAX must not be marked as staticInjected"
        );
    }

    #[test]
    fn codex_oauth_restores_static_372000_injection() {
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
        assert_eq!(
            effective["autoSyncState"]["staticInjected"],
            json!({ "ACW": "372000", "MAX": "372000" })
        );
    }

    #[test]
    fn codex_oauth_static_injection_wins_over_context_windows() {
        let db = Database::memory().expect("create memory db");
        let mut provider = Provider::with_id(
            "codex-oauth-context-windows".to_string(),
            "Codex With Context Windows".to_string(),
            json!({
                "env": { "ANTHROPIC_MODEL": "gpt-5.6" },
                "contextWindows": { "ANTHROPIC_MODEL": 800000 }
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
        assert_eq!(
            effective["autoSyncState"]["staticInjected"],
            json!({ "ACW": "372000", "MAX": "372000" })
        );
    }

    #[test]
    fn codex_oauth_context_defaults_override_legacy_common_config_values() {
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
        assert_eq!(
            effective["autoSyncState"]["staticInjected"],
            json!({ "ACW": "372000", "MAX": "372000" })
        );
    }

    #[test]
    fn codex_oauth_context_defaults_preserve_non_gpt56_models_and_common_values() {
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
        assert_eq!(
            effective["env"]["CLAUDE_CODE_MAX_CONTEXT_TOKENS"],
            json!("262144")
        );
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
        assert_eq!(
            live["env"]["CLAUDE_CODE_AUTO_COMPACT_WINDOW"],
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
    fn codex_managed_oauth_live_auth_matches_codex_cli_shape() {
        assert_eq!(
            codex_managed_oauth_live_auth(
                "acct-managed",
                "access-token",
                Some("id-token"),
                "refresh-token",
                "2026-01-02T03:04:05.000000000Z",
            ),
            json!({
                "auth_mode": "chatgpt",
                "OPENAI_API_KEY": null,
                "tokens": {
                    "id_token": "id-token",
                    "access_token": "access-token",
                    "refresh_token": "refresh-token",
                    "account_id": "acct-managed"
                },
                "last_refresh": "2026-01-02T03:04:05.000000000Z"
            }),
            "managed live auth must carry refresh_token + last_refresh so the Codex CLI can self-refresh"
        );
    }

    #[test]
    fn codex_managed_oauth_live_auth_without_id_token_omits_it() {
        assert_eq!(
            codex_managed_oauth_live_auth(
                "acct-managed",
                "access-token",
                None,
                "refresh-token",
                "2026-01-02T03:04:05.000000000Z",
            ),
            json!({
                "auth_mode": "chatgpt",
                "OPENAI_API_KEY": null,
                "tokens": {
                    "access_token": "access-token",
                    "refresh_token": "refresh-token",
                    "account_id": "acct-managed"
                },
                "last_refresh": "2026-01-02T03:04:05.000000000Z"
            }),
            "without a stored id_token the field is omitted rather than written as null"
        );
    }

    #[test]
    fn codex_official_managed_oauth_binding_replaces_auth_with_selected_account_token() {
        let temp = tempfile::tempdir().expect("tempdir");
        let manager = Arc::new(CodexOAuthManager::new(temp.path().to_path_buf()));
        tauri::async_runtime::block_on(async {
            manager
                .add_test_account_with_access_token(
                    "acct-managed",
                    "managed-token",
                    Some("managed-id-token"),
                )
                .await
                .expect("seed managed account");
        });

        let mut provider = Provider::with_id(
            "openai-official".to_string(),
            "OpenAI Official".to_string(),
            json!({
                "auth": {
                    "OPENAI_API_KEY": "stale-key"
                },
                "config": ""
            }),
            None,
        );
        provider.category = Some("official".to_string());
        provider.meta = Some(ProviderMeta {
            auth_binding: Some(AuthBinding {
                source: AuthBindingSource::ManagedAccount,
                auth_provider: Some("codex_oauth".to_string()),
                account_id: Some("acct-managed".to_string()),
            }),
            ..Default::default()
        });

        apply_codex_managed_oauth_auth(&AppType::Codex, &mut provider, Some(&manager))
            .expect("apply managed OAuth auth");

        // last_refresh 是写入时刻的时间戳（非确定），因此逐字段断言而非整体等值。
        let auth = provider.settings_config.get("auth").expect("auth written");
        assert_eq!(
            auth.get("auth_mode").and_then(|v| v.as_str()),
            Some("chatgpt")
        );
        assert!(auth.get("OPENAI_API_KEY").is_some_and(|v| v.is_null()));
        let tokens = auth
            .get("tokens")
            .and_then(|v| v.as_object())
            .expect("tokens object");
        assert_eq!(
            tokens.get("account_id").and_then(|v| v.as_str()),
            Some("acct-managed")
        );
        assert_eq!(
            tokens.get("access_token").and_then(|v| v.as_str()),
            Some("managed-token")
        );
        assert_eq!(
            tokens.get("id_token").and_then(|v| v.as_str()),
            Some("managed-id-token")
        );
        assert_eq!(
            tokens.get("refresh_token").and_then(|v| v.as_str()),
            Some("test-refresh-token"),
            "managed live auth must carry the account's refresh_token so the Codex CLI can self-refresh"
        );
        assert!(
            auth.get("last_refresh")
                .and_then(|v| v.as_str())
                .is_some_and(|s| s.ends_with('Z')),
            "managed live auth must include an RFC3339 last_refresh timestamp"
        );
    }

    #[test]
    fn codex_official_without_binding_does_not_fall_back_to_managed_default_account() {
        let temp = tempfile::tempdir().expect("tempdir");
        let manager = Arc::new(CodexOAuthManager::new(temp.path().to_path_buf()));
        tauri::async_runtime::block_on(async {
            manager
                .add_test_account_with_access_token("acct-managed", "managed-token", None)
                .await
                .expect("seed managed account");
        });

        let original_auth = json!({
            "auth_mode": "chatgpt",
            "OPENAI_API_KEY": null,
            "tokens": {
                "access_token": "native-codex-token",
                "account_id": "acct-native"
            }
        });
        let mut provider = Provider::with_id(
            "openai-official".to_string(),
            "OpenAI Official".to_string(),
            json!({
                "auth": original_auth.clone(),
                "config": ""
            }),
            None,
        );
        provider.category = Some("official".to_string());

        apply_codex_managed_oauth_auth(&AppType::Codex, &mut provider, Some(&manager))
            .expect("no binding should be a no-op");

        assert_eq!(
            provider.settings_config.get("auth"),
            Some(&original_auth),
            "unbound official providers must keep native Codex auth instead of using the managed default account"
        );
    }

    #[test]
    fn backfill_never_persists_native_tokens_into_managed_provider_config() {
        // 托管 provider 的存储配置以占位 auth 表示；但 live 里此刻是用户自己
        // 浏览器登录的原生 auth（含真实 refresh_token）。backfill 必须把 live
        // auth 换回存储占位，绝不把原生 access/refresh_token 回填进 DB 配置。
        let mut provider = Provider::with_id(
            "openai-official".to_string(),
            "OpenAI Official".to_string(),
            json!({
                "auth": {},
                "config": ""
            }),
            None,
        );
        provider.category = Some("official".to_string());
        provider.meta = Some(ProviderMeta {
            auth_binding: Some(AuthBinding {
                source: AuthBindingSource::ManagedAccount,
                auth_provider: Some("codex_oauth".to_string()),
                account_id: Some("acct-managed".to_string()),
            }),
            ..Default::default()
        });

        let mut live_settings = json!({
            "auth": {
                "OPENAI_API_KEY": null,
                "tokens": {
                    "id_token": "native-id",
                    "access_token": "native-access-secret",
                    "refresh_token": "native-refresh-secret",
                    "account_id": "acct-native"
                },
                "last_refresh": "2026-01-01T00:00:00Z"
            },
            "config": ""
        });

        strip_codex_managed_oauth_auth_for_backfill(&provider, &mut live_settings);

        assert_eq!(
            live_settings.get("auth"),
            Some(&json!({})),
            "managed provider backfill must reset live auth to the stored placeholder"
        );
        let serialized = live_settings.to_string();
        assert!(
            !serialized.contains("native-refresh-secret"),
            "native refresh_token must not leak into a managed provider's backfilled config"
        );
        assert!(
            !serialized.contains("native-access-secret"),
            "native access_token must not leak into a managed provider's backfilled config"
        );
    }

    #[test]
    fn backfill_leaves_non_managed_provider_auth_untouched() {
        // 非托管 provider 不受此剥离影响：其 auth 就该原样保留。
        let mut provider = Provider::with_id(
            "custom".to_string(),
            "Custom".to_string(),
            json!({ "auth": { "OPENAI_API_KEY": "user-key" }, "config": "" }),
            None,
        );
        provider.category = Some("custom".to_string());

        let mut live_settings = json!({
            "auth": { "OPENAI_API_KEY": "user-key" },
            "config": ""
        });
        let before = live_settings.clone();
        strip_codex_managed_oauth_auth_for_backfill(&provider, &mut live_settings);
        assert_eq!(
            live_settings, before,
            "non-managed provider auth must be left untouched by managed-oauth backfill stripping"
        );
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

    #[test]
    fn context_window_suffix_injection_matrix() {
        let db = Database::memory().expect("create memory db");
        // (env 配置, 期望 MAX/ACW；None = 不注入)
        let cases: Vec<(Value, Option<(&str, &str)>)> = vec![
            (
                json!({
                    "ANTHROPIC_BASE_URL": "https://example.com",
                    "ANTHROPIC_DEFAULT_SONNET_MODEL": "deepseek-v4-pro[1m]",
                    "ANTHROPIC_DEFAULT_OPUS_MODEL": "glm-5.2[200k]"
                }),
                Some(("1000000", "950000")),
            ),
            (
                json!({
                    "ANTHROPIC_BASE_URL": "https://example.com",
                    "ANTHROPIC_DEFAULT_SONNET_MODEL": "deepseek-v4-pro"
                }),
                None,
            ),
        ];
        for (env, expected) in cases {
            let provider = Provider::with_id(
                "test".to_string(),
                "Test".to_string(),
                json!({ "env": env }),
                None,
            );
            let effective =
                build_effective_settings_with_common_config(&db, &AppType::Claude, &provider)
                    .expect("build effective settings");
            // 后缀迁移后模型名保持干净
            for key in CLAUDE_MODEL_ENV_KEYS {
                if let Some(model) = effective["env"].get(key).and_then(Value::as_str) {
                    assert!(!model.ends_with(']'), "{key} kept legacy suffix: {model}");
                }
            }
            match expected {
                Some((max, acw)) => {
                    assert_eq!(effective["env"]["CLAUDE_CODE_MAX_CONTEXT_TOKENS"], max);
                    assert_eq!(effective["env"]["CLAUDE_CODE_AUTO_COMPACT_WINDOW"], acw);
                    assert_eq!(
                        effective["autoSyncState"]["staticInjected"],
                        json!({ "ACW": acw, "MAX": max })
                    );
                }
                None => {
                    assert!(effective["env"]
                        .get("CLAUDE_CODE_MAX_CONTEXT_TOKENS")
                        .is_none());
                    assert!(effective["env"]
                        .get("CLAUDE_CODE_AUTO_COMPACT_WINDOW")
                        .is_none());
                }
            }
        }
    }

    #[test]
    fn sanitize_strips_all_internal_fields_and_keeps_env() {
        let settings = json!({
            "env": { "ANTHROPIC_MODEL": "test" },
            "autoSyncContextWindow": true,
            "autoSyncCompactRatio": 0.8,
            "contextWindows": { "ANTHROPIC_MODEL": 200000 },
            "contextWindowTokens": { "ANTHROPIC_MODEL": "200k" },
            "autoSyncState": { "lastWritten": {} }
        });
        let clean = sanitize_claude_settings_for_live(&settings);
        let obj = clean.as_object().unwrap();
        for key in [
            "autoSyncContextWindow",
            "autoSyncCompactRatio",
            "contextWindows",
            "contextWindowTokens",
            "autoSyncState",
        ] {
            assert!(!obj.contains_key(key), "{key} leaked into live settings");
        }
        // 其他字段保留
        assert_eq!(clean["env"]["ANTHROPIC_MODEL"], json!("test"));
    }

    #[test]
    fn context_window_defaults_max_window_matrix() {
        let db = Database::memory().expect("create memory db");
        // (env 配置, 期望 MAX/ACW；None = 不注入)
        let cases: Vec<(Value, Option<(&str, &str)>)> = vec![
            (
                json!({
                    "ANTHROPIC_MODEL": "fallback-model[1M]",
                    "ANTHROPIC_DEFAULT_SONNET_MODEL": "sonnet-model[200k]"
                }),
                Some(("1000000", "950000")),
            ),
            (
                json!({
                    "ANTHROPIC_MODEL": "fallback-model",
                    "ANTHROPIC_DEFAULT_SONNET_MODEL": "sonnet-model[1M]",
                    "ANTHROPIC_DEFAULT_HAIKU_MODEL": "haiku-model[30k]"
                }),
                Some(("1000000", "950000")),
            ),
            (
                json!({
                    "ANTHROPIC_MODEL": "fallback-model[200k]",
                    "ANTHROPIC_DEFAULT_SONNET_MODEL": "sonnet-model[1M]"
                }),
                Some(("1000000", "950000")),
            ),
            (
                json!({
                    "ANTHROPIC_MODEL": "fallback-model",
                    "ANTHROPIC_DEFAULT_SONNET_MODEL": "sonnet-model"
                }),
                None,
            ),
        ];
        for (env, expected) in cases {
            let provider = Provider::with_id(
                "test".to_string(),
                "Test".to_string(),
                json!({ "env": env }),
                None,
            );
            let effective =
                build_effective_settings_with_common_config(&db, &AppType::Claude, &provider)
                    .expect("build effective settings");
            match expected {
                Some((max, acw)) => {
                    assert_eq!(effective["env"]["CLAUDE_CODE_MAX_CONTEXT_TOKENS"], max);
                    assert_eq!(effective["env"]["CLAUDE_CODE_AUTO_COMPACT_WINDOW"], acw);
                }
                None => {
                    assert!(effective["env"]
                        .get("CLAUDE_CODE_MAX_CONTEXT_TOKENS")
                        .is_none());
                    assert!(effective["env"]
                        .get("CLAUDE_CODE_AUTO_COMPACT_WINDOW")
                        .is_none());
                }
            }
        }
    }

    #[test]
    fn apply_context_window_defaults_remove_stale_static_injected_when_no_target() {
        let provider = Provider::with_id(
            "test".to_string(),
            "Test".to_string(),
            json!({
                "env": { "ANTHROPIC_MODEL": "fallback-model" },
                "contextWindows": {}
            }),
            None,
        );

        let mut settings = json!({
            "env": { "ANTHROPIC_MODEL": "fallback-model" },
            "contextWindows": {},
            "autoSyncState": {
                "lastWritten": { "model": "sonnet" },
                "staticInjected": { "ACW": "1000000", "MAX": "1000000" }
            }
        });
        apply_context_window_defaults(&mut settings, &provider);
        assert_eq!(
            settings["autoSyncState"]["lastWritten"],
            json!({ "model": "sonnet" })
        );
        assert!(
            settings["autoSyncState"].get("staticInjected").is_none(),
            "empty contextWindows must clear stale staticInjected"
        );
    }

    #[test]
    fn apply_context_window_defaults_ignores_zero_context_windows() {
        let provider = Provider::with_id(
            "test-zero".to_string(),
            "Test Zero".to_string(),
            json!({
                "env": { "ANTHROPIC_MODEL": "fallback-model" },
                "contextWindows": { "ANTHROPIC_MODEL": 0 }
            }),
            None,
        );

        let mut settings = json!({
            "env": { "ANTHROPIC_MODEL": "fallback-model" },
            "contextWindows": { "ANTHROPIC_MODEL": 0 },
            "autoSyncState": {
                "lastWritten": { "model": "sonnet" },
                "staticInjected": { "ACW": "1000000", "MAX": "1000000" }
            }
        });
        apply_context_window_defaults(&mut settings, &provider);
        assert!(
            settings["env"]
                .get("CLAUDE_CODE_MAX_CONTEXT_TOKENS")
                .is_none(),
            "zero contextWindows must not inject MAX"
        );
        assert!(
            settings["env"]
                .get("CLAUDE_CODE_AUTO_COMPACT_WINDOW")
                .is_none(),
            "zero contextWindows must not inject ACW"
        );
        assert!(
            settings["autoSyncState"].get("staticInjected").is_none(),
            "zero contextWindows must clear stale staticInjected"
        );
    }

    #[test]
    fn apply_context_window_defaults_keeps_kimi_static_injected() {
        let provider = Provider::with_id(
            "kimi".to_string(),
            "Kimi For Coding".to_string(),
            json!({
                "env": {
                    "ANTHROPIC_BASE_URL": "https://api.kimi.com/coding/",
                    "ANTHROPIC_MODEL": "kimi-for-coding"
                },
                "contextWindows": { "ANTHROPIC_MODEL": 800000 }
            }),
            None,
        );

        let mut settings = json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://api.kimi.com/coding/",
                "ANTHROPIC_MODEL": "kimi-for-coding"
            },
            "contextWindows": { "ANTHROPIC_MODEL": 800000 },
            "autoSyncState": {}
        });

        apply_static_context_defaults(&mut settings, &provider);
        apply_context_window_defaults(&mut settings, &provider);

        assert_eq!(
            settings["env"]["CLAUDE_CODE_AUTO_COMPACT_WINDOW"],
            json!("262144")
        );
        assert_eq!(
            settings["env"]["CLAUDE_CODE_MAX_CONTEXT_TOKENS"],
            json!("262144")
        );
        assert_eq!(
            settings["autoSyncState"]["staticInjected"],
            json!({ "ACW": "262144", "MAX": "262144" })
        );
    }

    #[test]
    #[serial]
    fn write_live_with_common_config_clears_stale_static_injected_in_db() {
        let _home = TempHome::new();
        let db = Database::memory().expect("create memory db");
        let provider = Provider::with_id(
            "clear-stale-db".to_string(),
            "Clear Stale DB".to_string(),
            json!({
                "env": { "ANTHROPIC_MODEL": "fallback-model" },
                "contextWindows": {},
                "autoSyncState": {
                    "lastWritten": { "model": "sonnet" },
                    "staticInjected": { "ACW": "1000000", "MAX": "1000000" }
                }
            }),
            None,
        );
        db.save_provider(AppType::Claude.as_str(), &provider)
            .expect("save provider");
        write_live_with_common_config(&db, &AppType::Claude, &provider).expect("write live");

        let stored = db
            .get_provider_by_id("clear-stale-db", AppType::Claude.as_str())
            .expect("get stored provider")
            .expect("stored provider exists");
        assert_eq!(
            stored.settings_config["autoSyncState"]["lastWritten"],
            json!({ "model": "sonnet" })
        );
        assert!(
            stored.settings_config["autoSyncState"]
                .get("staticInjected")
                .is_none(),
            "empty contextWindows must remove stale staticInjected from DB"
        );
    }

    #[test]
    fn context_window_defaults_skip_when_auto_sync_disabled() {
        let db = Database::memory().expect("create memory db");
        let provider = Provider::with_id(
            "test".to_string(),
            "Test".to_string(),
            json!({
                "env": { "ANTHROPIC_MODEL": "fallback-model[1M]" },
                "autoSyncContextWindow": false
            }),
            None,
        );

        let effective =
            build_effective_settings_with_common_config(&db, &AppType::Claude, &provider)
                .expect("build effective settings");
        // 静态注入来自 contextWindows，不受 watcher 开关影响
        assert_eq!(effective["env"]["ANTHROPIC_MODEL"], "fallback-model");
        assert_eq!(
            effective["env"]["CLAUDE_CODE_MAX_CONTEXT_TOKENS"],
            "1000000"
        );
        assert_eq!(
            effective["env"]["CLAUDE_CODE_AUTO_COMPACT_WINDOW"],
            "950000"
        );
    }

    #[test]
    fn auto_sync_enabled_does_not_inject_model_suffix_context_window_fields() {
        let db = Database::memory().expect("create memory db");
        let provider = Provider::with_id(
            "test-on".to_string(),
            "Test On".to_string(),
            json!({
                "env": { "ANTHROPIC_MODEL": "fallback-model[1M]" },
                "autoSyncContextWindow": true
            }),
            None,
        );

        let effective =
            build_effective_settings_with_common_config(&db, &AppType::Claude, &provider)
                .expect("build effective settings");
        // 静态注入仍按 contextWindows 写入，watcher 只负责后续模型切换
        assert_eq!(effective["env"]["ANTHROPIC_MODEL"], "fallback-model");
        assert_eq!(
            effective["env"]["CLAUDE_CODE_MAX_CONTEXT_TOKENS"],
            "1000000"
        );
        assert_eq!(
            effective["env"]["CLAUDE_CODE_AUTO_COMPACT_WINDOW"],
            "950000"
        );
    }

    #[test]
    fn auto_sync_disabled_preserves_common_config_context_window_fields() {
        let db = Database::memory().expect("create memory db");
        db.set_config_snippet(
            AppType::Claude.as_str(),
            Some(
                json!({
                    "env": {
                        "CLAUDE_CODE_MAX_CONTEXT_TOKENS": "500000",
                        "CLAUDE_CODE_AUTO_COMPACT_WINDOW": "400000"
                    }
                })
                .to_string(),
            ),
        )
        .expect("save common config");
        let mut provider = Provider::with_id(
            "test-off-common".to_string(),
            "Test Off Common".to_string(),
            json!({
                "env": { "ANTHROPIC_MODEL": "fallback-model[1M]" },
                "autoSyncContextWindow": false
            }),
            None,
        );
        provider.meta = Some(crate::provider::ProviderMeta {
            common_config_enabled: Some(true),
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
            json!("400000")
        );
    }

    #[test]
    fn claude_backfill_migrates_legacy_suffix_only_provider_context_windows() {
        let db = Database::memory().expect("create memory db");
        let provider = Provider::with_id(
            "test".to_string(),
            "Test".to_string(),
            json!({ "env": { "ANTHROPIC_MODEL": "fallback-model[1M]" } }),
            None,
        );

        // live 写盘时会 sanitize 掉 contextWindows，且模型名已被剥离后缀。
        let live = sanitize_claude_settings_for_live(
            &build_effective_settings_with_common_config(&db, &AppType::Claude, &provider)
                .expect("build effective settings"),
        );
        assert!(live.get("contextWindows").is_none());
        assert_eq!(live["env"]["ANTHROPIC_MODEL"], "fallback-model");

        let backfilled =
            strip_common_config_from_live_settings(&db, &AppType::Claude, &provider, live);
        // provider 只有 legacy 后缀时，回填也必须从迁移结果恢复窗口信息。
        assert_eq!(backfilled["contextWindows"]["ANTHROPIC_MODEL"], 1000000);
        assert_eq!(backfilled["env"]["ANTHROPIC_MODEL"], "fallback-model");
    }

    #[test]
    fn claude_backfill_preserves_context_window_tokens_and_sanitize_strips_them() {
        // display-only K/M 原样串：live 写盘 sanitize 会剥掉，直连切走回填必须从 DB
        // 恢复，否则每切一次供应商就把上一个供应商的原样串清掉。
        let db = Database::memory().expect("create memory db");
        let provider = Provider::with_id(
            "test".to_string(),
            "Test".to_string(),
            json!({
                "env": { "ANTHROPIC_MODEL": "fallback-model" },
                "contextWindows": { "ANTHROPIC_MODEL": 200000 },
                "contextWindowTokens": { "ANTHROPIC_MODEL": "200k" }
            }),
            None,
        );

        let live = sanitize_claude_settings_for_live(
            &build_effective_settings_with_common_config(&db, &AppType::Claude, &provider)
                .expect("build effective settings"),
        );
        assert!(
            live.get("contextWindowTokens").is_none(),
            "contextWindowTokens must not leak into live settings.json"
        );

        let backfilled =
            strip_common_config_from_live_settings(&db, &AppType::Claude, &provider, live);
        assert_eq!(backfilled["contextWindowTokens"]["ANTHROPIC_MODEL"], "200k");
    }

    #[test]
    fn claude_backfill_strips_all_legacy_suffixes_from_stored_models() {
        let provider = Provider::with_id(
            "test".to_string(),
            "Test".to_string(),
            json!({
                "env": {
                    "ANTHROPIC_MODEL": "fallback-model[200k]",
                    "ANTHROPIC_DEFAULT_SONNET_MODEL": "sonnet-model[1M]",
                    "CLAUDE_CODE_SUBAGENT_MODEL": "subagent-model[1M]"
                },
                "contextWindows": {
                    "ANTHROPIC_MODEL": 200000,
                    "ANTHROPIC_DEFAULT_SONNET_MODEL": 1000000,
                    "CLAUDE_CODE_SUBAGENT_MODEL": 1000000
                }
            }),
            None,
        );
        let live = json!({
            "env": {
                "ANTHROPIC_MODEL": "fallback-model",
                "ANTHROPIC_DEFAULT_SONNET_MODEL": "sonnet-model",
                "CLAUDE_CODE_SUBAGENT_MODEL": "subagent-model"
            }
        });
        let backfilled =
            restore_live_settings_for_provider_backfill(&AppType::Claude, &provider, live);
        assert_eq!(backfilled["env"]["ANTHROPIC_MODEL"], "fallback-model");
        assert_eq!(
            backfilled["env"]["ANTHROPIC_DEFAULT_SONNET_MODEL"],
            "sonnet-model"
        );
        assert_eq!(
            backfilled["env"]["CLAUDE_CODE_SUBAGENT_MODEL"],
            "subagent-model"
        );
    }

    #[test]
    fn claude_backfill_preserves_all_internal_fields() {
        let db = Database::memory().expect("create memory db");
        let provider = Provider::with_id(
            "test".to_string(),
            "Test".to_string(),
            json!({
                "env": { "ANTHROPIC_MODEL": "fallback-model[1M]" },
                "autoSyncContextWindow": false,
                "autoSyncCompactRatio": 0.8,
                "contextWindows": { "fallback-model[1M]": 1000000 },
                "autoSyncState": { "lastWritten": {} }
            }),
            None,
        );

        // 写 live 时四个内部字段都会被 sanitize 移除，切走回填时必须从 provider 恢复；
        // legacy 后缀会迁移出 role-key contextWindows，旧 key 也保留。
        let live = sanitize_claude_settings_for_live(
            &build_effective_settings_with_common_config(&db, &AppType::Claude, &provider)
                .expect("build effective settings"),
        );
        for key in [
            "autoSyncContextWindow",
            "autoSyncCompactRatio",
            "contextWindows",
            "autoSyncState",
        ] {
            assert!(live.get(key).is_none(), "{key} leaked into live settings");
        }

        let backfilled =
            strip_common_config_from_live_settings(&db, &AppType::Claude, &provider, live);
        assert_eq!(backfilled["autoSyncContextWindow"], json!(false));
        assert_eq!(backfilled["autoSyncCompactRatio"], json!(0.8));
        assert_eq!(
            backfilled["contextWindows"],
            json!({
                "fallback-model[1M]": 1000000,
                "ANTHROPIC_MODEL": 1000000
            })
        );
        assert_eq!(backfilled["autoSyncState"], json!({ "lastWritten": {} }));
        assert_eq!(
            backfilled["env"]["ANTHROPIC_MODEL"],
            json!("fallback-model")
        );
    }

    #[test]
    fn claude_backfill_preserves_auto_sync_context_window_flag() {
        let db = Database::memory().expect("create memory db");
        let provider = Provider::with_id(
            "test".to_string(),
            "Test".to_string(),
            json!({
                "env": { "ANTHROPIC_MODEL": "fallback-model[1M]" },
                "autoSyncContextWindow": false,
                "autoSyncCompactRatio": 0.8
            }),
            None,
        );

        // 写 live 时内部字段会被 sanitize 移除，模拟 settings.json 里没有该键
        let live = sanitize_claude_settings_for_live(
            &build_effective_settings_with_common_config(&db, &AppType::Claude, &provider)
                .expect("build effective settings"),
        );
        assert!(live.get("autoSyncContextWindow").is_none());

        let backfilled =
            strip_common_config_from_live_settings(&db, &AppType::Claude, &provider, live);
        // 切走回填后开关状态必须保留，否则再切回来会回到默认开
        assert_eq!(backfilled["autoSyncContextWindow"], json!(false));
        assert_eq!(backfilled["autoSyncCompactRatio"], json!(0.8));
    }

    #[test]
    fn claude_backfill_strips_model_suffix_injected_context_defaults() {
        let db = Database::memory().expect("create memory db");
        let provider = Provider::with_id(
            "test".to_string(),
            "Test".to_string(),
            json!({ "env": { "ANTHROPIC_MODEL": "fallback-model[1M]" } }),
            None,
        );

        let live = build_effective_settings_with_common_config(&db, &AppType::Claude, &provider)
            .expect("build effective settings");
        // live 会注入 ACW/MAX，并剥离旧后缀
        assert_eq!(live["env"]["ANTHROPIC_MODEL"], "fallback-model");
        assert_eq!(live["env"]["CLAUDE_CODE_MAX_CONTEXT_TOKENS"], "1000000");
        assert_eq!(live["env"]["CLAUDE_CODE_AUTO_COMPACT_WINDOW"], "950000");

        // 注入产物只应活在 live，切走回填后必须剥掉
        let backfilled =
            strip_common_config_from_live_settings(&db, &AppType::Claude, &provider, live);
        assert!(backfilled["env"]
            .get("CLAUDE_CODE_MAX_CONTEXT_TOKENS")
            .is_none());
        assert!(backfilled["env"]
            .get("CLAUDE_CODE_AUTO_COMPACT_WINDOW")
            .is_none());
        assert_eq!(backfilled["env"]["ANTHROPIC_MODEL"], "fallback-model");
    }

    #[test]
    fn claude_backfill_strips_defaults_using_provider_compact_ratio() {
        let db = Database::memory().expect("create memory db");
        let provider = Provider::with_id(
            "test".to_string(),
            "Test".to_string(),
            json!({
                "env": { "ANTHROPIC_MODEL": "fallback-model[200k]" },
                "autoSyncContextWindow": true,
                "autoSyncCompactRatio": 0.5
            }),
            None,
        );

        let live = json!({
            "env": {
                "ANTHROPIC_MODEL": "fallback-model[200k]",
                "CLAUDE_CODE_MAX_CONTEXT_TOKENS": "200000",
                "CLAUDE_CODE_AUTO_COMPACT_WINDOW": "100000"
            }
        });
        let backfilled =
            strip_common_config_from_live_settings(&db, &AppType::Claude, &provider, live);
        assert!(backfilled["env"]
            .get("CLAUDE_CODE_MAX_CONTEXT_TOKENS")
            .is_none());
        assert!(backfilled["env"]
            .get("CLAUDE_CODE_AUTO_COMPACT_WINDOW")
            .is_none());
        assert_eq!(backfilled["autoSyncCompactRatio"], json!(0.5));
    }
    #[test]
    fn claude_backfill_keeps_user_edited_live_context_window_values() {
        let db = Database::memory().expect("create memory db");
        let provider = Provider::with_id(
            "test".to_string(),
            "Test".to_string(),
            json!({ "env": { "ANTHROPIC_MODEL": "fallback-model[1M]" } }),
            None,
        );

        // 手改过 live 的值不等于按模型后缀算出的注入值，必须保留
        let live = json!({
            "env": {
                "ANTHROPIC_MODEL": "fallback-model[1M]",
                "CLAUDE_CODE_MAX_CONTEXT_TOKENS": "1234567",
                "CLAUDE_CODE_AUTO_COMPACT_WINDOW": "765432"
            }
        });
        let backfilled =
            strip_common_config_from_live_settings(&db, &AppType::Claude, &provider, live);
        assert_eq!(
            backfilled["env"]["CLAUDE_CODE_MAX_CONTEXT_TOKENS"],
            json!("1234567")
        );
        assert_eq!(
            backfilled["env"]["CLAUDE_CODE_AUTO_COMPACT_WINDOW"],
            json!("765432")
        );
    }

    #[test]
    fn backfill_removes_static_injected_values_when_not_user_explicit() {
        let mut settings = json!({
            "env": {
                "CLAUDE_CODE_AUTO_COMPACT_WINDOW": "262144",
                "CLAUDE_CODE_MAX_CONTEXT_TOKENS": "262144"
            }
        });
        let provider = Provider::with_id(
            "p".to_string(),
            "P".to_string(),
            json!({
                "autoSyncState": {
                    "lastWritten": { "ACW": "160000", "MAX": "200000" },
                    "staticInjected": { "ACW": "262144", "MAX": "262144" },
                    "userExplicit": {}
                }
            }),
            None,
        );
        strip_auto_synced_context_defaults(&mut settings, &provider);
        assert!(settings["env"]
            .get("CLAUDE_CODE_AUTO_COMPACT_WINDOW")
            .is_none());
        assert!(settings["env"]
            .get("CLAUDE_CODE_MAX_CONTEXT_TOKENS")
            .is_none());
    }
    #[test]
    fn codex_oauth_context_defaults_inject_when_auto_sync_disabled() {
        let db = Database::memory().expect("create memory db");
        let mut provider = Provider::with_id(
            "codex-oauth".to_string(),
            "Codex".to_string(),
            json!({
                "env": { "ANTHROPIC_MODEL": "gpt-5.6" },
                "autoSyncContextWindow": false
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
        // 静态兜底不依赖 watcher 开关
        assert_eq!(
            effective["env"]["CLAUDE_CODE_MAX_CONTEXT_TOKENS"],
            json!("372000")
        );
        assert_eq!(
            effective["env"]["CLAUDE_CODE_AUTO_COMPACT_WINDOW"],
            json!("372000")
        );
        assert_eq!(
            effective["autoSyncState"]["staticInjected"],
            json!({ "ACW": "372000", "MAX": "372000" })
        );
    }

    #[test]
    fn kimi_for_coding_context_defaults_inject_when_auto_sync_disabled() {
        let db = Database::memory().expect("create memory db");
        let provider = Provider::with_id(
            "kimi-for-coding".to_string(),
            "Kimi For Coding".to_string(),
            json!({
                "env": {
                    "ANTHROPIC_BASE_URL": "https://api.kimi.com/coding/",
                    "ANTHROPIC_MODEL": "kimi-for-coding"
                },
                "autoSyncContextWindow": false
            }),
            None,
        );

        let effective =
            build_effective_settings_with_common_config(&db, &AppType::Claude, &provider)
                .expect("build effective settings");
        // 静态兜底不依赖 watcher 开关
        assert_eq!(
            effective["env"]["CLAUDE_CODE_MAX_CONTEXT_TOKENS"],
            json!("262144")
        );
        assert_eq!(
            effective["env"]["CLAUDE_CODE_AUTO_COMPACT_WINDOW"],
            json!("262144")
        );
        assert_eq!(
            effective["autoSyncState"]["staticInjected"],
            json!({ "ACW": "262144", "MAX": "262144" })
        );
    }

    #[test]
    fn backfill_falls_back_per_key_when_ledger_has_only_other_key() {
        let mut settings = json!({
            "env": {
                "CLAUDE_CODE_AUTO_COMPACT_WINDOW": "950000",
                "CLAUDE_CODE_MAX_CONTEXT_TOKENS": "250000"
            }
        });
        let provider = Provider::with_id(
            "p".to_string(),
            "P".to_string(),
            json!({
                "env": { "ANTHROPIC_MODEL": "fallback-model[1M]" },
                "autoSyncState": {
                    "userExplicit": { "MAX": "250000" }
                }
            }),
            None,
        );
        strip_auto_synced_context_defaults(&mut settings, &provider);
        assert!(settings["env"]
            .get("CLAUDE_CODE_AUTO_COMPACT_WINDOW")
            .is_none());
        assert_eq!(
            settings["env"]["CLAUDE_CODE_MAX_CONTEXT_TOKENS"],
            json!("250000")
        );
    }

    #[test]
    fn backfill_keeps_provider_env_explicit_value_matching_last_written() {
        let mut settings = json!({
            "env": { "CLAUDE_CODE_MAX_CONTEXT_TOKENS": "250000" }
        });
        let provider = Provider::with_id(
            "p".to_string(),
            "P".to_string(),
            json!({
                "env": { "CLAUDE_CODE_MAX_CONTEXT_TOKENS": "250000" },
                "autoSyncState": {
                    "lastWritten": { "MAX": "250000" }
                }
            }),
            None,
        );
        strip_auto_synced_context_defaults(&mut settings, &provider);
        assert_eq!(
            settings["env"]["CLAUDE_CODE_MAX_CONTEXT_TOKENS"],
            json!("250000")
        );
    }

    #[test]
    fn backfill_keeps_numeric_provider_env_explicit_value_matching_last_written() {
        let mut settings = json!({
            "env": { "CLAUDE_CODE_MAX_CONTEXT_TOKENS": "250000" }
        });
        let provider = Provider::with_id(
            "p".to_string(),
            "P".to_string(),
            json!({
                "env": { "CLAUDE_CODE_MAX_CONTEXT_TOKENS": 250000 },
                "autoSyncState": {
                    "lastWritten": { "MAX": "250000" }
                }
            }),
            None,
        );
        strip_auto_synced_context_defaults(&mut settings, &provider);
        assert_eq!(
            settings["env"]["CLAUDE_CODE_MAX_CONTEXT_TOKENS"],
            json!("250000")
        );
    }

    #[test]
    fn backfill_without_ledger_uses_active_model_window_when_roles_differ() {
        let provider = Provider::with_id(
            "p".to_string(),
            "P".to_string(),
            json!({
                "env": {
                    "ANTHROPIC_DEFAULT_HAIKU_MODEL": "haiku-model[30k]",
                    "ANTHROPIC_DEFAULT_OPUS_MODEL": "opus-model[1M]"
                },
                "autoSyncCompactRatio": 0.8
            }),
            None,
        );
        let live = json!({
            "model": "haiku",
            "env": {
                "ANTHROPIC_DEFAULT_HAIKU_MODEL": "haiku-model",
                "ANTHROPIC_DEFAULT_OPUS_MODEL": "opus-model",
                "CLAUDE_CODE_AUTO_COMPACT_WINDOW": "24000",
                "CLAUDE_CODE_MAX_CONTEXT_TOKENS": "30000"
            }
        });

        let backfilled =
            restore_live_settings_for_provider_backfill(&AppType::Claude, &provider, live);
        assert!(backfilled["env"]
            .get("CLAUDE_CODE_AUTO_COMPACT_WINDOW")
            .is_none());
        assert!(backfilled["env"]
            .get("CLAUDE_CODE_MAX_CONTEXT_TOKENS")
            .is_none());
        assert_eq!(
            backfilled["env"]["ANTHROPIC_DEFAULT_HAIKU_MODEL"],
            "haiku-model"
        );
        assert_eq!(
            backfilled["env"]["ANTHROPIC_DEFAULT_OPUS_MODEL"],
            "opus-model"
        );
    }
}
