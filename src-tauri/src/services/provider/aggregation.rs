//! 全局聚合模式（Aggregation / Stack）与跨应用供应商兼容层。
//!
//! 职责：
//! 1. 聚合页总览（`get_aggregation_overview`）：汇总所有统一供应商、各应用自有供应商以及各应用的兼容/接入状态；
//! 2. 跨应用兼容投影：把任意非官方供应商（通用供应商或任意应用下的供应商）转换为其他 14 个兼容应用所需的配置格式；
//! 3. 全局聚合开关（`set_global_aggregation_mode`）：统一控制 `enable_stack_mode`，关闭时自动把处于 Stack 模式的代理应用退回直连；
//! 4. 代理模式切换与成员管理：在聚合页直接控制支持本地代理的应用进入/退出聚合（Stack / Proxy）并刷新托盘。

use std::collections::{HashMap, HashSet};

use serde::Serialize;
use serde_json::{json, Value};

use crate::app_config::AppType;
use crate::codex_config::{is_anthropic_wire_api, is_chat_wire_api};
use crate::error::AppError;
use crate::provider::{
    ClaudeDesktopMode, ClaudeDesktopModelRoute, ClaudeStackModel, Provider, ProviderMeta,
    UniversalProvider, UniversalProviderApps,
};
use crate::services::ProviderService;
use crate::store::AppState;

/// 通用/聚合供应商生成的子条目 ID 前缀
const UNIVERSAL_PREFIX: &str = "universal-";
const AGG_SYNC_PREFIX: &str = "agg-";

/// 聚合页展示的单个应用内原生供应商（不含跨应用同步生成的子条目）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AggregationAppProviderItem {
    pub app_type: String,
    pub provider: Provider,
    pub base_url: Option<String>,
    pub api_format: String,
    pub models: Vec<String>,
    pub is_official: bool,
    pub can_sync: bool,
    pub is_current_in_app: bool,
    pub in_stack: bool,
    /// 已同步兼容到哪些目标应用
    pub synced_apps: Vec<String>,
}

/// 聚合页展示的每个应用接入状态。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AggregationAppState {
    pub app_type: String,
    pub supports_local_proxy: bool,
    pub supports_stack_catalog: bool,
    pub additive: bool,
    /// `"direct" | "route" | "stack" | "additive" | "launch"`
    pub mode: String,
    pub attached: bool,
    pub route_provider_id: Option<String>,
    pub stack_member_count: usize,
    pub synced_provider_count: usize,
}

/// 聚合页总览数据。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AggregationOverview {
    pub enabled: bool,
    pub proxy_running: bool,
    pub proxy_address: String,
    pub proxy_port: u16,
    pub universal_providers: Vec<UniversalProvider>,
    pub app_providers: Vec<AggregationAppProviderItem>,
    pub app_states: Vec<AggregationAppState>,
}

/// 同步结果摘要。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncAllResult {
    pub synced_apps: Vec<String>,
    pub warnings: Vec<String>,
}

/// 跨应用兼容投影所需的标准化供应商描述。
#[derive(Debug, Clone)]
pub struct NormalizedProviderSpec {
    pub name: String,
    pub base_url: String,
    pub api_key: String,
    /// `"anthropic" | "openai_chat" | "openai_responses" | "gemini_native"`
    pub api_format: String,
    pub models: Vec<String>,
    pub website_url: Option<String>,
    pub notes: Option<String>,
    pub icon: Option<String>,
    pub icon_color: Option<String>,
    pub created_at: Option<i64>,
    pub sort_index: Option<usize>,
    pub reasoning_effort: Option<String>,
}

/// 从 `UniversalProvider` 推导标准协议格式。
pub fn infer_universal_api_format(u: &UniversalProvider) -> String {
    if let Some(fmt) = u
        .api_format
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        return normalize_api_format(fmt);
    }
    let url_lower = u.base_url.to_ascii_lowercase();
    if url_lower.contains("generativelanguage.googleapis.com")
        || url_lower.contains("aistudio.google")
    {
        return "gemini_native".to_string();
    }
    if url_lower.contains("api.anthropic.com") || u.provider_type.eq_ignore_ascii_case("anthropic")
    {
        return "anthropic".to_string();
    }
    if u.provider_type.eq_ignore_ascii_case("openai_responses") {
        return "openai_responses".to_string();
    }
    "openai_chat".to_string()
}

fn normalize_api_format(raw: &str) -> String {
    match raw.trim().to_ascii_lowercase().as_str() {
        "anthropic" | "anthropic-messages" | "@ai-sdk/anthropic" => "anthropic".to_string(),
        "openai_responses" | "openai-responses" | "responses" | "codex_responses"
        | "@ai-sdk/openai" => "openai_responses".to_string(),
        "gemini_native" | "gemini" | "google-generative-ai" | "google-genai" | "@ai-sdk/google" => {
            "gemini_native".to_string()
        }
        _ => "openai_chat".to_string(),
    }
}

/// 从通用供应商构造标准描述。
pub fn spec_from_universal(u: &UniversalProvider) -> NormalizedProviderSpec {
    let api_format = infer_universal_api_format(u);
    let mut models = Vec::new();
    let mut push_unique = |candidate: Option<&str>| {
        if let Some(m) = candidate.map(str::trim).filter(|s| !s.is_empty()) {
            if !models.iter().any(|existing: &String| existing == m) {
                models.push(m.to_string());
            }
        }
    };

    match api_format.as_str() {
        "anthropic" => {
            push_unique(u.models.claude.as_ref().and_then(|c| c.model.as_deref()));
            push_unique(u.models.codex.as_ref().and_then(|c| c.model.as_deref()));
            push_unique(u.models.gemini.as_ref().and_then(|c| c.model.as_deref()));
        }
        "gemini_native" => {
            push_unique(u.models.gemini.as_ref().and_then(|c| c.model.as_deref()));
            push_unique(u.models.claude.as_ref().and_then(|c| c.model.as_deref()));
            push_unique(u.models.codex.as_ref().and_then(|c| c.model.as_deref()));
        }
        _ => {
            push_unique(u.models.codex.as_ref().and_then(|c| c.model.as_deref()));
            push_unique(u.models.claude.as_ref().and_then(|c| c.model.as_deref()));
            push_unique(u.models.gemini.as_ref().and_then(|c| c.model.as_deref()));
        }
    }
    for extra in &u.extra_models {
        push_unique(Some(extra));
    }
    if models.is_empty() {
        models.push(default_model_for_format(&api_format).to_string());
    }

    NormalizedProviderSpec {
        name: u.name.clone(),
        base_url: u.base_url.trim().trim_end_matches('/').to_string(),
        api_key: u.api_key.trim().to_string(),
        api_format,
        models,
        website_url: u.website_url.clone(),
        notes: u.notes.clone(),
        icon: u.icon.clone(),
        icon_color: u.icon_color.clone(),
        created_at: u.created_at,
        sort_index: u.sort_index,
        reasoning_effort: u
            .models
            .codex
            .as_ref()
            .and_then(|c| c.reasoning_effort.clone()),
    }
}

fn default_model_for_format(api_format: &str) -> &'static str {
    match api_format {
        "anthropic" => "claude-sonnet-4-20250514",
        "gemini_native" => "gemini-2.5-pro",
        "openai_responses" => "gpt-4o",
        _ => "gpt-4o",
    }
}

/// 判断一个供应商是否为官方/托管 OAuth 账号（不能直接作为普通第三方端点跨应用分发）。
pub fn is_official_provider(app: &AppType, provider: &Provider) -> bool {
    if provider.has_official_category() || provider.uses_managed_account_auth() {
        return true;
    }
    match app {
        AppType::Codex => crate::proxy::providers::is_codex_official_provider(provider),
        AppType::Copilot => crate::services::provider::copilot_direct::is_official(provider),
        AppType::WorkBuddy => crate::workbuddy_config::is_official_provider(provider),
        AppType::Kimi => crate::kimi_config::is_official_provider(&provider.settings_config),
        _ => false,
    }
}

fn extract_header_api_key(settings: &Value) -> Option<String> {
    for ptr in ["/options/headers", "/headers"] {
        let Some(headers) = settings.pointer(ptr).and_then(Value::as_object) else {
            continue;
        };
        for (k, v) in headers {
            let Some(raw) = v.as_str().map(str::trim).filter(|s| !s.is_empty()) else {
                continue;
            };
            if k.eq_ignore_ascii_case("authorization") {
                let token = raw
                    .strip_prefix("Bearer ")
                    .or_else(|| raw.strip_prefix("bearer "))
                    .unwrap_or(raw)
                    .trim();
                if !token.is_empty() {
                    return Some(token.to_string());
                }
            } else if k.eq_ignore_ascii_case("x-api-key") {
                return Some(raw.to_string());
            }
        }
    }
    None
}

fn is_local_endpoint(base_url: &str) -> bool {
    let lower = base_url.trim().to_ascii_lowercase();
    lower.starts_with("http://localhost")
        || lower.starts_with("https://localhost")
        || lower.starts_with("http://127.0.0.1")
        || lower.starts_with("https://127.0.0.1")
        || lower.starts_with("http://[::1]")
        || lower.starts_with("http://0.0.0.0")
}

/// 从任意应用的 `Provider` 提取连接地址、Key、接口格式和模型列表。
pub fn inspect_app_provider(
    app: &AppType,
    provider: &Provider,
) -> (Option<String>, Option<String>, String, Vec<String>) {
    let settings = &provider.settings_config;
    let env = settings.get("env");

    // 复用 Provider::resolve_usage_credentials 覆盖全部应用的常规及兜底凭据字段
    let (resolved_url, resolved_key) = provider.resolve_usage_credentials(app);

    let mut api_key = Some(resolved_key.trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| {
            env.and_then(|e| {
                ["OPENAI_API_KEY", "API_KEY", "CLAUDE_CODE_OAUTH_TOKEN"]
                    .iter()
                    .find_map(|k| {
                        e.get(*k)
                            .and_then(Value::as_str)
                            .map(str::trim)
                            .filter(|s| !s.is_empty())
                    })
                    .map(str::to_string)
            })
        })
        .or_else(|| {
            ["apiKey", "api_key", "authToken", "token"]
                .iter()
                .find_map(|k| {
                    settings
                        .get(*k)
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                })
                .map(str::to_string)
        })
        .or_else(|| extract_header_api_key(settings))
        .or_else(|| {
            if matches!(app, AppType::Codex | AppType::GrokBuild) {
                let cfg = settings.get("config").and_then(Value::as_str);
                if let Some(cfg) = cfg {
                    if let Some(env_var) = extract_toml_string(cfg, "env_key") {
                        let env_var = env_var.trim();
                        if let Some(val) = settings
                            .get("auth")
                            .and_then(|a| a.get(env_var))
                            .and_then(Value::as_str)
                            .map(str::trim)
                            .filter(|s| !s.is_empty())
                        {
                            return Some(val.to_string());
                        }
                        if let Ok(val) = std::env::var(env_var) {
                            let trimmed = val.trim().to_string();
                            if !trimmed.is_empty() {
                                return Some(trimmed);
                            }
                        }
                    }
                    if let Ok(parsed) = toml::from_str::<toml::Value>(cfg) {
                        let no_auth = parsed
                            .get("requires_openai_auth")
                            .and_then(toml::Value::as_bool)
                            == Some(false)
                            || parsed
                                .get("model_providers")
                                .and_then(toml::Value::as_table)
                                .is_some_and(|t| {
                                    t.values().any(|p| {
                                        p.get("requires_openai_auth").and_then(toml::Value::as_bool)
                                            == Some(false)
                                    })
                                });
                        if no_auth {
                            return Some("local-no-key".to_string());
                        }
                    }
                }
                if let Some(auth_obj) = settings.get("auth").and_then(Value::as_object) {
                    for (k, v) in auth_obj {
                        if matches!(k.as_str(), "auth_mode" | "last_refresh") {
                            continue;
                        }
                        if let Some(val) = v.as_str().map(str::trim).filter(|s| !s.is_empty()) {
                            return Some(val.to_string());
                        }
                    }
                }
            }
            None
        });

    let mut base_url = Some(resolved_url.trim().trim_end_matches('/').to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| {
            ["base_url", "baseURL", "baseUrl"]
                .iter()
                .find_map(|k| settings.get(*k).and_then(Value::as_str))
                .or_else(|| settings.pointer("/options/baseUrl").and_then(Value::as_str))
                .map(|s| s.trim().trim_end_matches('/').to_string())
                .filter(|s| !s.is_empty())
        });

    if base_url.is_none() && api_key.is_some() {
        base_url = match app {
            AppType::Claude | AppType::ClaudeDesktop => {
                Some("https://api.anthropic.com".to_string())
            }
            AppType::Codex => Some("https://api.openai.com/v1".to_string()),
            AppType::Gemini => Some("https://generativelanguage.googleapis.com".to_string()),
            _ => None,
        };
    }

    if api_key.is_none() && base_url.as_deref().is_some_and(is_local_endpoint) {
        api_key = Some("local-no-key".to_string());
    }

    let api_format = if is_official_provider(app, provider) {
        "official".to_string()
    } else if let Some(fmt) = provider
        .meta
        .as_ref()
        .and_then(|m| m.api_format.as_deref())
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        normalize_api_format(fmt)
    } else {
        match app {
            AppType::Claude | AppType::ClaudeDesktop => {
                crate::proxy::providers::get_claude_api_format(provider).to_string()
            }
            AppType::Codex | AppType::GrokBuild => {
                if let Some(cfg) = settings.get("config").and_then(Value::as_str) {
                    if let Some(wire) = extract_toml_string(cfg, "wire_api") {
                        if is_anthropic_wire_api(&wire) {
                            "anthropic".to_string()
                        } else if is_chat_wire_api(&wire) {
                            "openai_chat".to_string()
                        } else {
                            "openai_responses".to_string()
                        }
                    } else {
                        "openai_responses".to_string()
                    }
                } else {
                    "openai_responses".to_string()
                }
            }
            AppType::Gemini | AppType::Agy => "gemini_native".to_string(),
            AppType::OpenCode => settings
                .get("npm")
                .and_then(Value::as_str)
                .map(normalize_api_format)
                .unwrap_or_else(|| "openai_chat".to_string()),
            AppType::OpenClaw | AppType::Pi | AppType::Omp | AppType::Mcode | AppType::Dsh => {
                settings
                    .get("api")
                    .and_then(Value::as_str)
                    .map(normalize_api_format)
                    .unwrap_or_else(|| {
                        if matches!(app, AppType::Mcode) {
                            "anthropic".to_string()
                        } else {
                            "openai_chat".to_string()
                        }
                    })
            }
            AppType::Hermes => settings
                .get("api_mode")
                .and_then(Value::as_str)
                .map(normalize_api_format)
                .unwrap_or_else(|| "openai_chat".to_string()),
            AppType::Kimi => settings
                .pointer("/provider/type")
                .and_then(Value::as_str)
                .map(normalize_api_format)
                .unwrap_or_else(|| "openai_chat".to_string()),
            AppType::Copilot => settings
                .pointer("/provider/type")
                .and_then(Value::as_str)
                .map(normalize_api_format)
                .unwrap_or_else(|| "openai_chat".to_string()),
            AppType::WorkBuddy => settings
                .get("protocol")
                .and_then(Value::as_str)
                .map(normalize_api_format)
                .unwrap_or_else(|| "openai_chat".to_string()),
        }
    };

    let models = extract_models_for_app(app, provider);
    (base_url, api_key, api_format, models)
}

fn extract_models_for_app(app: &AppType, provider: &Provider) -> Vec<String> {
    let mut out = Vec::new();
    let mut push = |m: &str| {
        let trimmed = m.trim();
        if !trimmed.is_empty() && !out.iter().any(|existing: &String| existing == trimmed) {
            out.push(trimmed.to_string());
        }
    };

    if let Some(stack_models) = provider
        .meta
        .as_ref()
        .and_then(|m| m.stack_models.as_deref())
    {
        for entry in stack_models {
            push(&entry.model);
        }
    }

    let settings = &provider.settings_config;
    match app {
        AppType::Claude | AppType::ClaudeDesktop => {
            if let Some(env) = settings.get("env") {
                for key in [
                    "ANTHROPIC_MODEL",
                    "ANTHROPIC_DEFAULT_SONNET_MODEL",
                    "ANTHROPIC_DEFAULT_OPUS_MODEL",
                    "ANTHROPIC_DEFAULT_HAIKU_MODEL",
                ] {
                    if let Some(val) = env.get(key).and_then(Value::as_str) {
                        push(val);
                    }
                }
            }
            if let Some(meta) = provider.meta.as_ref() {
                for route_entry in meta.claude_desktop_model_routes.values() {
                    push(&route_entry.model);
                }
            }
        }
        AppType::Codex => {
            if let Some(cfg) = settings.get("config").and_then(Value::as_str) {
                if let Some(model) = extract_toml_string(cfg, "model") {
                    push(&model);
                }
            }
            if let Some(catalog) = settings.get("modelCatalog") {
                let entries = catalog
                    .get("models")
                    .and_then(Value::as_array)
                    .or_else(|| catalog.as_array());
                if let Some(arr) = entries {
                    for entry in arr {
                        if let Some(m) = entry.get("model").and_then(Value::as_str) {
                            push(m);
                        }
                    }
                }
            }
        }
        AppType::GrokBuild => {
            if let Some(cfg) = settings.get("config").and_then(Value::as_str) {
                if let Some(model_cfg) = crate::grok_config::extract_model_config(cfg) {
                    push(&model_cfg.model);
                }
                if let Ok(parsed) = cfg.parse::<toml::Value>() {
                    if let Some(model_tables) = parsed.get("model").and_then(toml::Value::as_table)
                    {
                        for (key, table) in model_tables {
                            if let Some(m) = table.get("model").and_then(toml::Value::as_str) {
                                push(m);
                            } else {
                                push(key);
                            }
                        }
                    }
                }
            }
        }
        AppType::Gemini | AppType::Agy => {
            if let Some(model) = settings
                .pointer("/env/GEMINI_MODEL")
                .and_then(Value::as_str)
            {
                push(model);
            }
        }
        AppType::OpenCode | AppType::Mcode | AppType::Dsh => {
            if let Some(map) = settings.get("models").and_then(Value::as_object) {
                for key in map.keys() {
                    push(key);
                }
            } else if let Some(arr) = settings.get("models").and_then(Value::as_array) {
                for entry in arr {
                    if let Some(id) = entry
                        .get("id")
                        .or_else(|| entry.get("model"))
                        .and_then(Value::as_str)
                    {
                        push(id);
                    } else if let Some(s) = entry.as_str() {
                        push(s);
                    }
                }
            }
        }
        AppType::Hermes => {
            if let Some(map) = settings.get("models").and_then(Value::as_object) {
                for key in map.keys() {
                    push(key);
                }
            } else if let Some(arr) = settings.get("models").and_then(Value::as_array) {
                for entry in arr {
                    if let Some(id) = entry
                        .get("id")
                        .or_else(|| entry.get("model"))
                        .and_then(Value::as_str)
                    {
                        push(id);
                    } else if let Some(s) = entry.as_str() {
                        push(s);
                    }
                }
            }
            if let Some(m) = settings.get("model").and_then(Value::as_str) {
                push(m);
            }
        }
        AppType::OpenClaw | AppType::Pi | AppType::Omp | AppType::Copilot | AppType::WorkBuddy => {
            if let Some(arr) = settings.get("models").and_then(Value::as_array) {
                for entry in arr {
                    if let Some(id) = entry
                        .get("id")
                        .or_else(|| entry.get("model"))
                        .and_then(Value::as_str)
                    {
                        push(id);
                    } else if let Some(s) = entry.as_str() {
                        push(s);
                    }
                }
            }
        }
        AppType::Kimi => {
            if let Some(map) = settings.get("models").and_then(Value::as_object) {
                for val in map.values() {
                    if let Some(m) = val.get("model").and_then(Value::as_str) {
                        push(m);
                    }
                }
            }
        }
    }
    out
}

fn extract_toml_string(cfg: &str, key: &str) -> Option<String> {
    let parsed: toml::Value = toml::from_str(cfg).ok()?;
    if let Some(s) = parsed.get(key).and_then(toml::Value::as_str) {
        return Some(s.to_string());
    }
    if let Some(providers) = parsed
        .get("model_providers")
        .and_then(toml::Value::as_table)
    {
        for table in providers.values() {
            if let Some(s) = table.get(key).and_then(toml::Value::as_str) {
                return Some(s.to_string());
            }
        }
    }
    None
}

pub fn spec_from_app_provider(
    app: &AppType,
    provider: &Provider,
) -> Option<NormalizedProviderSpec> {
    if is_official_provider(app, provider) {
        return None;
    }
    let (base_url, api_key, api_format, mut models) = inspect_app_provider(app, provider);
    let base_url = base_url?;
    let api_key = api_key?;
    if models.is_empty() {
        models.push(default_model_for_format(&api_format).to_string());
    }
    Some(NormalizedProviderSpec {
        name: provider.name.clone(),
        base_url,
        api_key,
        api_format,
        models,
        website_url: provider.website_url.clone(),
        notes: provider.notes.clone(),
        icon: provider.icon.clone(),
        icon_color: provider.icon_color.clone(),
        created_at: provider.created_at,
        sort_index: provider.sort_index,
        reasoning_effort: None,
    })
}

/// 生成合法的小写连字符 ID（Dsh 要求：`[a-z0-9-]`，不含 `--`，首尾不为 `-`）。
fn sanitize_hyphen_id(raw: &str, fallback_prefix: &str) -> String {
    let mut out = String::new();
    for c in raw.chars() {
        let lower = c.to_ascii_lowercase();
        if lower.is_ascii_lowercase() || lower.is_ascii_digit() {
            out.push(lower);
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    let trimmed = out.trim_matches('-').to_string();
    if trimmed.is_empty() || trimmed == "deepseek-official" {
        format!("{fallback_prefix}-provider")
    } else {
        trimmed
    }
}

/// 生成合法的 `MCode` ID（`[a-zA-Z0-9_-]`）。
fn sanitize_alnum_id(raw: &str) -> String {
    let mut out: String = raw
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_') {
                c
            } else {
                '-'
            }
        })
        .collect();
    if out.is_empty() {
        out = "agg-provider".to_string();
    }
    out
}

/// 生成合法的 `Copilot` ID（不含 `/`，空白或控制字符，最多 64 字符）。
fn sanitize_copilot_id(raw: &str) -> String {
    let out: String = raw
        .chars()
        .filter(|c| *c != '/' && !c.is_whitespace() && !c.is_control())
        .take(64)
        .collect();
    if out.is_empty() {
        "agg-provider".to_string()
    } else {
        out
    }
}

fn ensure_v1_url(base_url: &str) -> String {
    let base_trimmed = base_url.trim().trim_end_matches('/');
    let origin_only = match base_trimmed.split_once("://") {
        Some((_scheme, rest)) => !rest.contains('/'),
        None => !base_trimmed.contains('/'),
    };
    if base_trimmed.ends_with("/v1") {
        base_trimmed.to_string()
    } else if origin_only {
        format!("{base_trimmed}/v1")
    } else {
        base_trimmed.to_string()
    }
}

/// 生成目标应用可直接使用、可通过 `ProviderService::validate_provider_settings` 的 `Provider`。
pub fn build_compatible_provider(
    spec: &NormalizedProviderSpec,
    target_app: &AppType,
    target_id_seed: &str,
) -> Option<Provider> {
    let primary_model = spec
        .models
        .first()
        .cloned()
        .unwrap_or_else(|| default_model_for_format(&spec.api_format).to_string());
    let all_models: Vec<String> = if spec.models.is_empty() {
        vec![primary_model.clone()]
    } else {
        spec.models.clone()
    };

    let stack_models: Vec<ClaudeStackModel> = all_models
        .iter()
        .map(|m| ClaudeStackModel {
            model: m.clone(),
            display_name: None,
            one_m: false,
        })
        .collect();

    let api_format_meta = Some(spec.api_format.clone());

    match target_app {
        AppType::Claude => {
            let settings_config = json!({
                "env": {
                    "ANTHROPIC_BASE_URL": spec.base_url,
                    "ANTHROPIC_AUTH_TOKEN": spec.api_key,
                    "ANTHROPIC_MODEL": primary_model,
                    "ANTHROPIC_DEFAULT_HAIKU_MODEL": primary_model,
                    "ANTHROPIC_DEFAULT_SONNET_MODEL": primary_model,
                    "ANTHROPIC_DEFAULT_OPUS_MODEL": primary_model,
                }
            });
            let meta = Some(ProviderMeta {
                api_format: api_format_meta,
                stack_models: Some(stack_models),
                ..Default::default()
            });
            Some(make_provider(
                target_id_seed.to_string(),
                spec,
                settings_config,
                meta,
            ))
        }
        AppType::ClaudeDesktop => {
            let settings_config = json!({
                "env": {
                    "ANTHROPIC_BASE_URL": spec.base_url,
                    "ANTHROPIC_AUTH_TOKEN": spec.api_key,
                    "ANTHROPIC_MODEL": primary_model,
                    "ANTHROPIC_DEFAULT_HAIKU_MODEL": primary_model,
                    "ANTHROPIC_DEFAULT_SONNET_MODEL": primary_model,
                    "ANTHROPIC_DEFAULT_OPUS_MODEL": primary_model,
                }
            });
            let mut routes = HashMap::new();
            for m in &all_models {
                routes.insert(
                    format!("claude-{m}"),
                    ClaudeDesktopModelRoute {
                        model: m.clone(),
                        label_override: None,
                        supports_1m: None,
                    },
                );
            }
            let meta = Some(ProviderMeta {
                api_format: api_format_meta,
                claude_desktop_mode: Some(ClaudeDesktopMode::Proxy),
                claude_desktop_model_routes: routes,
                stack_models: Some(stack_models),
                ..Default::default()
            });
            Some(make_provider(
                target_id_seed.to_string(),
                spec,
                settings_config,
                meta,
            ))
        }
        AppType::Codex => {
            let wire_api = match spec.api_format.as_str() {
                "openai_chat" => "chat",
                "anthropic" => "anthropic",
                _ => "responses",
            };
            let codex_base = ensure_v1_url(&spec.base_url);
            let effort = spec.reasoning_effort.as_deref().unwrap_or("high");
            let config_toml = format!(
                "model_provider = \"custom\"\nmodel = \"{primary_model}\"\nmodel_reasoning_effort = \"{effort}\"\n\n[model_providers.custom]\nname = \"CCSwitchAgg\"\nbase_url = \"{codex_base}\"\nwire_api = \"{wire_api}\"\nrequires_openai_auth = true"
            );
            let mut settings_map = serde_json::Map::new();
            settings_map.insert(
                "auth".to_string(),
                json!({
                    "OPENAI_API_KEY": spec.api_key
                }),
            );
            settings_map.insert("config".to_string(), json!(config_toml));
            if all_models.len() > 1 {
                if let Some(catalog) = crate::provider::build_codex_model_catalog(None, &all_models)
                {
                    settings_map.insert("modelCatalog".to_string(), catalog);
                }
            }
            let settings_config = serde_json::Value::Object(settings_map);
            let meta = Some(ProviderMeta {
                api_format: api_format_meta,
                ..Default::default()
            });
            Some(make_provider(
                target_id_seed.to_string(),
                spec,
                settings_config,
                meta,
            ))
        }
        AppType::GrokBuild => {
            let api_backend = match spec.api_format.as_str() {
                "openai_chat" => "chat_completions",
                "anthropic" => "anthropic",
                _ => crate::grok_config::DEFAULT_API_BACKEND,
            };
            let grok_base = ensure_v1_url(&spec.base_url);
            let model_toml = toml::Value::String(primary_model.clone()).to_string();
            let base_toml = toml::Value::String(grok_base).to_string();
            let name_toml = toml::Value::String(spec.name.clone()).to_string();
            let key_toml = toml::Value::String(spec.api_key.clone()).to_string();
            let mut config_toml = format!("[models]\ndefault = {model_toml}\n");
            for m in &all_models {
                let entry_toml = toml::Value::String(m.clone()).to_string();
                config_toml.push_str(&format!(
                    "\n[model.{entry_toml}]\nmodel = {entry_toml}\nbase_url = {base_toml}\nname = {name_toml}\napi_key = {key_toml}\napi_backend = \"{api_backend}\"\ncontext_window = {}\n",
                    crate::grok_config::DEFAULT_CONTEXT_WINDOW,
                ));
            }
            let settings_config = json!({
                "config": config_toml
            });
            let meta = Some(ProviderMeta {
                api_format: api_format_meta,
                ..Default::default()
            });
            Some(make_provider(
                target_id_seed.to_string(),
                spec,
                settings_config,
                meta,
            ))
        }
        AppType::Gemini => {
            let settings_config = json!({
                "env": {
                    "GOOGLE_GEMINI_BASE_URL": spec.base_url,
                    "GEMINI_API_KEY": spec.api_key,
                    "GEMINI_MODEL": primary_model,
                }
            });
            let meta = Some(ProviderMeta {
                api_format: api_format_meta,
                ..Default::default()
            });
            Some(make_provider(
                target_id_seed.to_string(),
                spec,
                settings_config,
                meta,
            ))
        }
        AppType::OpenCode => {
            let npm = match spec.api_format.as_str() {
                "anthropic" => "@ai-sdk/anthropic",
                "openai_responses" => "@ai-sdk/openai",
                "gemini_native" => "@ai-sdk/google",
                _ => "@ai-sdk/openai-compatible",
            };
            let mut models_map = serde_json::Map::new();
            for m in &all_models {
                models_map.insert(m.clone(), json!({ "name": m }));
            }
            let base = if matches!(spec.api_format.as_str(), "openai_chat" | "openai_responses") {
                ensure_v1_url(&spec.base_url)
            } else {
                spec.base_url.clone()
            };
            let settings_config = json!({
                "npm": npm,
                "name": spec.name,
                "options": {
                    "baseURL": base,
                    "apiKey": spec.api_key,
                },
                "models": models_map
            });
            Some(make_provider(
                sanitize_alnum_id(target_id_seed),
                spec,
                settings_config,
                None,
            ))
        }
        AppType::OpenClaw => {
            let api = match spec.api_format.as_str() {
                "anthropic" => "anthropic-messages",
                "openai_responses" => "openai-responses",
                "gemini_native" => "google-generative-ai",
                _ => "openai-completions",
            };
            let base = if matches!(spec.api_format.as_str(), "openai_chat" | "openai_responses") {
                ensure_v1_url(&spec.base_url)
            } else {
                spec.base_url.clone()
            };
            let models_arr: Vec<Value> = all_models
                .iter()
                .map(|m| json!({ "id": m, "name": m }))
                .collect();
            let settings_config = json!({
                "baseUrl": base,
                "apiKey": spec.api_key,
                "api": api,
                "models": models_arr
            });
            Some(make_provider(
                sanitize_alnum_id(target_id_seed),
                spec,
                settings_config,
                None,
            ))
        }
        AppType::Hermes => {
            let api_mode = match spec.api_format.as_str() {
                "anthropic" => "anthropic_messages",
                "openai_responses" => "codex_responses",
                _ => "chat_completions",
            };
            let base = if matches!(spec.api_format.as_str(), "openai_chat" | "openai_responses") {
                ensure_v1_url(&spec.base_url)
            } else {
                spec.base_url.clone()
            };
            let mut models_map = serde_json::Map::new();
            for m in &all_models {
                models_map.insert(m.clone(), json!({}));
            }
            let settings_config = json!({
                "name": spec.name,
                "base_url": base,
                "api_key": spec.api_key,
                "api_mode": api_mode,
                "models": models_map
            });
            Some(make_provider(
                sanitize_alnum_id(target_id_seed),
                spec,
                settings_config,
                None,
            ))
        }
        AppType::Pi | AppType::Omp => {
            let api = match spec.api_format.as_str() {
                "anthropic" => "anthropic-messages",
                "openai_responses" => "openai-responses",
                "gemini_native" => "google-generative-ai",
                _ => "openai-completions",
            };
            let base = if matches!(spec.api_format.as_str(), "openai_chat" | "openai_responses") {
                ensure_v1_url(&spec.base_url)
            } else {
                spec.base_url.clone()
            };
            let models_arr: Vec<Value> = all_models
                .iter()
                .map(|m| json!({ "id": m, "name": m }))
                .collect();
            let settings_config = json!({
                "baseUrl": base,
                "apiKey": spec.api_key,
                "api": api,
                "models": models_arr
            });
            Some(make_provider(
                sanitize_alnum_id(target_id_seed),
                spec,
                settings_config,
                None,
            ))
        }
        AppType::Mcode => {
            let api = match spec.api_format.as_str() {
                "anthropic" => "anthropic-messages",
                "openai_responses" => "openai-responses",
                _ => "openai-completions",
            };
            let base = if matches!(spec.api_format.as_str(), "openai_chat" | "openai_responses") {
                ensure_v1_url(&spec.base_url)
            } else {
                spec.base_url.clone()
            };
            let mut models_map = serde_json::Map::new();
            for m in &all_models {
                models_map.insert(m.clone(), json!({ "name": m }));
            }
            let settings_config = json!({
                "api": api,
                "options": {
                    "baseURL": base,
                    "apiKey": spec.api_key,
                },
                "models": models_map
            });
            Some(make_provider(
                sanitize_alnum_id(target_id_seed),
                spec,
                settings_config,
                None,
            ))
        }
        AppType::Kimi => {
            let id = sanitize_hyphen_id(target_id_seed, "agg-kimi");
            let provider_type = match spec.api_format.as_str() {
                "anthropic" => "anthropic",
                "openai_responses" => "openai_responses",
                "gemini_native" => "google-genai",
                _ => "openai",
            };
            let base = if matches!(spec.api_format.as_str(), "openai_chat" | "openai_responses") {
                ensure_v1_url(&spec.base_url)
            } else {
                spec.base_url.clone()
            };
            let mut models_map = serde_json::Map::new();
            for m in &all_models {
                models_map.insert(
                    format!("{id}/{m}"),
                    json!({
                        "provider": id,
                        "model": m,
                        "max_context_size": 128000,
                    }),
                );
            }
            let settings_config = json!({
                "provider": {
                    "type": provider_type,
                    "base_url": base,
                    "api_key": spec.api_key,
                },
                "models": models_map
            });
            Some(make_provider(id, spec, settings_config, None))
        }
        AppType::Dsh => {
            let id = sanitize_hyphen_id(target_id_seed, "agg-dsh");
            let api = match spec.api_format.as_str() {
                "anthropic" => "anthropic-messages",
                "openai_responses" => "openai-responses",
                _ => "openai-completions",
            };
            let base = if matches!(spec.api_format.as_str(), "openai_chat" | "openai_responses") {
                ensure_v1_url(&spec.base_url)
            } else {
                spec.base_url.clone()
            };
            let mut models_map = serde_json::Map::new();
            for m in &all_models {
                models_map.insert(m.clone(), json!({ "name": m }));
            }
            let settings_config = json!({
                "api": api,
                "options": {
                    "baseURL": base,
                    "apiKey": spec.api_key,
                },
                "models": models_map
            });
            Some(make_provider(id, spec, settings_config, None))
        }
        AppType::Copilot => {
            let id = sanitize_copilot_id(target_id_seed);
            let provider_type = match spec.api_format.as_str() {
                "anthropic" => "anthropic",
                _ => "openai",
            };
            let models_arr: Vec<Value> = all_models
                .iter()
                .map(|m| json!({ "id": m, "name": m }))
                .collect();
            let settings_config = json!({
                "provider": {
                    "type": provider_type,
                    "baseUrl": spec.base_url,
                    "apiKey": spec.api_key,
                },
                "models": models_arr
            });
            Some(make_provider(id, spec, settings_config, None))
        }
        AppType::WorkBuddy => {
            let protocol = match spec.api_format.as_str() {
                "anthropic" => "anthropic",
                _ => "openai",
            };
            let models_arr: Vec<Value> = all_models
                .iter()
                .map(|m| {
                    json!({
                        "id": m,
                        "name": m,
                        "url": spec.base_url,
                        "apiKey": spec.api_key,
                        "protocol": protocol,
                    })
                })
                .collect();
            let settings_config = json!({
                "url": spec.base_url,
                "apiKey": spec.api_key,
                "protocol": protocol,
                "models": models_arr
            });
            Some(make_provider(
                sanitize_alnum_id(target_id_seed),
                spec,
                settings_config,
                None,
            ))
        }
        AppType::Agy => None,
    }
}

fn make_provider(
    id: String,
    spec: &NormalizedProviderSpec,
    settings_config: Value,
    meta: Option<ProviderMeta>,
) -> Provider {
    Provider {
        id,
        name: spec.name.clone(),
        settings_config,
        website_url: spec.website_url.clone(),
        category: Some("aggregator".to_string()),
        created_at: spec.created_at,
        sort_index: spec.sort_index,
        notes: spec.notes.clone(),
        meta,
        icon: spec.icon.clone(),
        icon_color: spec.icon_color.clone(),
        in_failover_queue: false,
    }
}

fn is_generated_sync_id(id: &str) -> bool {
    id.starts_with(UNIVERSAL_PREFIX) || id.starts_with(AGG_SYNC_PREFIX)
}

pub fn synced_child_id_for_universal(app: &AppType, universal_id: &str) -> String {
    let raw = format!("{UNIVERSAL_PREFIX}{}-{universal_id}", app.as_str());
    match app {
        AppType::Dsh | AppType::Kimi => sanitize_hyphen_id(&raw, "agg"),
        AppType::Copilot => sanitize_copilot_id(&raw),
        AppType::OpenCode
        | AppType::OpenClaw
        | AppType::Hermes
        | AppType::Pi
        | AppType::Omp
        | AppType::Mcode
        | AppType::WorkBuddy => sanitize_alnum_id(&raw),
        _ => raw,
    }
}

fn synced_child_id_for_app_provider(
    target_app: &AppType,
    source_app: &AppType,
    source_id: &str,
) -> String {
    let raw = format!(
        "{AGG_SYNC_PREFIX}{}-{}-{source_id}",
        target_app.as_str(),
        source_app.as_str()
    );
    match target_app {
        AppType::Dsh | AppType::Kimi => sanitize_hyphen_id(&raw, "agg"),
        AppType::Copilot => sanitize_copilot_id(&raw),
        AppType::OpenCode
        | AppType::OpenClaw
        | AppType::Hermes
        | AppType::Pi
        | AppType::Omp
        | AppType::Mcode
        | AppType::WorkBuddy => sanitize_alnum_id(&raw),
        _ => raw,
    }
}

/// 读取当前在「应用」页开启显示的应用配置。
pub fn current_visible_apps() -> crate::settings::VisibleApps {
    crate::settings::get_settings()
        .visible_apps
        .unwrap_or_default()
}

/// 将通用供应商的应用开关屏蔽掉未在「应用」页开启显示的应用（即不配置也不显示）。
pub fn mask_universal_apps_to_visible(
    apps: &mut UniversalProviderApps,
    visible: &crate::settings::VisibleApps,
) {
    for app in UniversalProviderApps::COMPATIBLE_APPS {
        if !visible.is_visible(&app) {
            apps.set_enabled_for(&app, false);
        }
    }
}

/// 获取聚合页总览（全部通用供应商、已开启显示的各应用原生供应商及应用状态）。
pub async fn get_aggregation_overview(state: &AppState) -> Result<AggregationOverview, String> {
    let settings = crate::settings::get_settings();
    let visible_apps = settings.visible_apps.unwrap_or_default();
    let proxy_status = state.proxy_service.get_status().await?;

    let state_clone = state.clone();
    tauri::async_runtime::spawn_blocking(move || {
        build_aggregation_overview_sync(
            &state_clone,
            settings.enable_stack_mode,
            proxy_status,
            &visible_apps,
        )
    })
    .await
    .map_err(|e| format!("读取聚合总览失败: {e}"))?
}

fn build_aggregation_overview_sync(
    state: &AppState,
    enabled: bool,
    proxy_status: crate::proxy::types::ProxyStatus,
    visible_apps: &crate::settings::VisibleApps,
) -> Result<AggregationOverview, String> {
    let universal_map = state
        .db
        .get_all_universal_providers()
        .map_err(|e| e.to_string())?;
    let mut universal_providers: Vec<UniversalProvider> = universal_map.into_values().collect();
    universal_providers.sort_by(|a, b| {
        a.sort_index
            .unwrap_or(usize::MAX)
            .cmp(&b.sort_index.unwrap_or(usize::MAX))
            .then_with(|| a.created_at.unwrap_or(0).cmp(&b.created_at.unwrap_or(0)))
            .then_with(|| a.id.cmp(&b.id))
    });

    let store = crate::live::engine::DeviceStore::for_device();

    // 预加载全部应用的供应商，便于统计 synced_apps
    let mut all_by_app: HashMap<String, indexmap::IndexMap<String, Provider>> = HashMap::new();
    let mut current_by_app: HashMap<String, Option<String>> = HashMap::new();
    let mut stack_members_by_app: HashMap<String, HashSet<String>> = HashMap::new();

    for app in AppType::all() {
        let providers = state
            .db
            .get_all_providers(&app)
            .map_err(|e| e.to_string())?;
        let current_id = if app.spec().route_mode {
            crate::mode::current::provider_for(
                &state.db,
                &app,
                crate::mode::current::Purpose::InUse,
            )
            .unwrap_or(None)
        } else {
            state.db.get_current_provider(&app).ok().flatten()
        };
        let stack_set: HashSet<String> = if app.spec().stack {
            crate::mode::state::stack(&store, app.as_str())
                .map(|s| s.members.into_iter().collect())
                .unwrap_or_default()
        } else {
            HashSet::new()
        };

        all_by_app.insert(app.as_str().to_string(), providers);
        current_by_app.insert(app.as_str().to_string(), current_id);
        stack_members_by_app.insert(app.as_str().to_string(), stack_set);
    }

    let mut app_providers = Vec::new();
    for app in AppType::all() {
        if !visible_apps.is_visible(&app) {
            continue;
        }
        let app_key = app.as_str();
        let Some(providers) = all_by_app.get(app_key) else {
            continue;
        };
        let current_id = current_by_app.get(app_key).and_then(|v| v.as_deref());
        let stack_set = stack_members_by_app.get(app_key);

        for (id, provider) in providers {
            if is_generated_sync_id(id) {
                continue;
            }
            let (base_url, api_key, api_format, models) = inspect_app_provider(&app, provider);
            let official = is_official_provider(&app, provider);
            let can_sync = !official && base_url.is_some() && api_key.is_some();
            let mut synced_apps = Vec::new();
            if !official {
                for target in UniversalProviderApps::COMPATIBLE_APPS {
                    if !visible_apps.is_visible(&target) {
                        continue;
                    }
                    if target == app {
                        synced_apps.push(target.as_str().to_string());
                        continue;
                    }
                    let expected_id = synced_child_id_for_app_provider(&target, &app, id);
                    if all_by_app
                        .get(target.as_str())
                        .is_some_and(|map| map.contains_key(&expected_id))
                    {
                        synced_apps.push(target.as_str().to_string());
                    }
                }
            } else {
                synced_apps.push(app_key.to_string());
            }

            app_providers.push(AggregationAppProviderItem {
                app_type: app_key.to_string(),
                provider: provider.clone(),
                base_url,
                api_format,
                models,
                is_official: official,
                can_sync,
                is_current_in_app: current_id == Some(id.as_str()),
                in_stack: stack_set.is_some_and(|set| set.contains(id)),
                synced_apps,
            });
        }
    }

    let mut app_states = Vec::new();
    for app in UniversalProviderApps::COMPATIBLE_APPS {
        if !visible_apps.is_visible(&app) {
            continue;
        }
        let spec = app.spec();
        let app_key = app.as_str();
        let synced_count = all_by_app
            .get(app_key)
            .map(|map| map.keys().filter(|id| is_generated_sync_id(id)).count())
            .unwrap_or(0);
        let stack_count = stack_members_by_app
            .get(app_key)
            .map(|s| s.len())
            .unwrap_or(0);

        let (mode, attached, route_provider_id) = if spec.route_mode {
            let mode_state = crate::mode::current::mode_state(&app);
            let stack_enabled = spec.stack
                && crate::mode::state::stack(&store, app_key)
                    .map(|s| s.enabled)
                    .unwrap_or(false);
            let mode_str = if !mode_state.is_proxy() {
                "direct"
            } else if stack_enabled {
                "stack"
            } else {
                "route"
            };
            (
                mode_str.to_string(),
                mode_state.attached,
                mode_state.proxy_route,
            )
        } else if spec.additive {
            ("additive".to_string(), enabled && synced_count > 0, None)
        } else {
            ("launch".to_string(), false, None)
        };

        app_states.push(AggregationAppState {
            app_type: app_key.to_string(),
            supports_local_proxy: spec.route_mode,
            supports_stack_catalog: spec.stack,
            additive: spec.additive,
            mode,
            attached,
            route_provider_id,
            stack_member_count: stack_count,
            synced_provider_count: synced_count,
        });
    }

    Ok(AggregationOverview {
        enabled,
        proxy_running: proxy_status.running,
        proxy_address: proxy_status.address,
        proxy_port: proxy_status.port,
        universal_providers,
        app_providers,
        app_states,
    })
}

/// 切换全局聚合模式开关（持久化 `enable_stack_mode`；关闭时让仍在 Stack 模式的应用退回直连）。
pub async fn set_global_aggregation_mode(
    state: &AppState,
    app_handle: &tauri::AppHandle,
    enabled: bool,
) -> Result<AggregationOverview, String> {
    let mut settings = crate::settings::get_settings();
    settings.enable_stack_mode = enabled;
    crate::settings::update_settings(settings).map_err(|e| e.to_string())?;

    if !enabled {
        let _ = crate::mode::controller::exit_apps_in_mode(state, true).await?;
    } else {
        // 开启聚合模式时：
        // 1. 同步已保存的通用供应商；
        // 2. 跨应用同步所有可见应用内就绪的非生成供应商；
        // 3. 自动为可见路由/聚合应用启用代理接入与 Stack，刷新 `/model` 目录。
        let visible_apps = crate::settings::get_settings()
            .visible_apps
            .unwrap_or_default();
        let _ = sync_all_universal_providers_inner(state).await;
        let _ = sync_all_ready_app_providers_inner(state, &visible_apps).await;
        let _ = activate_aggregation_for_visible_apps(state).await;
    }

    crate::tray::refresh_tray_menu(app_handle);
    get_aggregation_overview(state).await
}

/// 同步单个 `UniversalProvider` 到全部启用的可见兼容应用（含代理类与累加式应用）。
pub async fn sync_universal_to_compatible_apps(
    state: &AppState,
    universal: &UniversalProvider,
) -> Result<SyncAllResult, String> {
    let visible_apps = current_visible_apps();
    let spec = spec_from_universal(universal);
    let mut synced_apps = Vec::new();
    let mut warnings = Vec::new();

    for target_app in UniversalProviderApps::COMPATIBLE_APPS {
        if !visible_apps.is_visible(&target_app) {
            continue;
        }
        if !universal.apps.is_enabled_for(&target_app) {
            let child_id = synced_child_id_for_universal(&target_app, &universal.id);
            let _ = remove_synced_child_for_app(state, &target_app, &child_id).await;
            let legacy_id = format!("universal-{}", universal.id);
            if legacy_id != child_id {
                let _ = remove_synced_child_for_app(state, &target_app, &legacy_id).await;
            }
            continue;
        }

        let child_id = synced_child_id_for_universal(&target_app, &universal.id);
        // 对 Claude/Codex/Gemini 保持 `universal-<app>-<id>` 向后兼容命名并优先调用原有构造方法
        let mut candidate = if target_app == AppType::Claude {
            universal.to_claude_provider()
        } else if target_app == AppType::Codex {
            universal.to_codex_provider()
        } else if target_app == AppType::Gemini {
            universal.to_gemini_provider()
        } else {
            build_compatible_provider(&spec, &target_app, &child_id)
        };

        let Some(mut provider) = candidate.take() else {
            continue;
        };

        if target_app.spec().route_mode {
            if let Some(u_meta) = universal.meta.as_ref() {
                let p_meta = provider.meta.get_or_insert_with(Default::default);
                if p_meta.is_full_url.is_none() {
                    p_meta.is_full_url = u_meta.is_full_url;
                }
                if p_meta.api_key_field.is_none() {
                    p_meta.api_key_field = u_meta.api_key_field.clone();
                }
                if p_meta.custom_user_agent.is_none() {
                    p_meta.custom_user_agent = u_meta.custom_user_agent.clone();
                }
            }
        }

        match save_synced_provider_for_app(state, &target_app, provider).await {
            Ok(warn) => {
                synced_apps.push(target_app.as_str().to_string());
                if let Some(w) = warn {
                    warnings.push(w);
                }
            }
            Err(e) => warnings.push(format!("{}: {e}", target_app.as_str())),
        }
    }

    if crate::settings::get_settings().enable_stack_mode {
        warnings.extend(activate_aggregation_for_visible_apps(state).await);
    }

    Ok(SyncAllResult {
        synced_apps,
        warnings,
    })
}

async fn sync_all_universal_providers_inner(state: &AppState) -> Result<SyncAllResult, String> {
    let universal_map = state
        .db
        .get_all_universal_providers()
        .map_err(|e| e.to_string())?;
    let mut all_synced = HashSet::new();
    let mut all_warnings = Vec::new();
    for provider in universal_map.values() {
        let res = sync_universal_to_compatible_apps(state, provider).await?;
        all_synced.extend(res.synced_apps);
        all_warnings.extend(res.warnings);
    }
    Ok(SyncAllResult {
        synced_apps: all_synced.into_iter().collect(),
        warnings: all_warnings,
    })
}

/// 把任意来源供应商（`source_app == None` 时为 `UniversalProvider`，否则为某应用的原生 `Provider`）
/// 投影并同步到所有已开启显示的兼容应用。
pub async fn sync_provider_to_all_apps(
    state: &AppState,
    app_handle: &tauri::AppHandle,
    source_app: Option<AppType>,
    provider_id: &str,
) -> Result<SyncAllResult, String> {
    let visible_apps = current_visible_apps();
    let result = match source_app {
        None => {
            let mut universal = state
                .db
                .get_universal_provider(provider_id)
                .map_err(|e| e.to_string())?
                .ok_or_else(|| format!("通用供应商不存在: {provider_id}"))?;
            let mut next_apps = UniversalProviderApps::all_compatible();
            mask_universal_apps_to_visible(&mut next_apps, &visible_apps);
            universal.apps = next_apps;
            state
                .db
                .save_universal_provider(&universal)
                .map_err(|e| e.to_string())?;
            sync_universal_to_compatible_apps(state, &universal).await?
        }
        Some(src_app) => {
            if !visible_apps.is_visible(&src_app) {
                return Err(format!("应用 {} 未在应用页面开启显示", src_app.as_str()));
            }
            let mut source_provider = state
                .db
                .get_provider_by_id(provider_id, &src_app)
                .map_err(|e| e.to_string())?
                .ok_or_else(|| format!("供应商不存在: {provider_id}"))?;
            // 用户主动兼容时清除之前的取消标记
            if source_provider
                .meta
                .as_ref()
                .and_then(|m| m.aggregation_disabled)
                == Some(true)
            {
                if let Some(meta) = source_provider.meta.as_mut() {
                    meta.aggregation_disabled = None;
                }
                state
                    .db
                    .save_provider(&src_app, &source_provider)
                    .map_err(|e| e.to_string())?;
            }
            let spec = spec_from_app_provider(&src_app, &source_provider).ok_or_else(|| {
                format!(
                    "供应商 {} 是官方登录/托管账号或缺少端点凭据，无法跨应用转换",
                    source_provider.name
                )
            })?;
            sync_spec_to_target_apps(state, &spec, Some(&src_app), provider_id, &visible_apps).await
        }
    };

    if crate::settings::get_settings().enable_stack_mode {
        let mut result = result;
        result
            .warnings
            .extend(activate_aggregation_for_visible_apps(state).await);
        crate::tray::refresh_tray_menu(app_handle);
        return Ok(result);
    }

    crate::tray::refresh_tray_menu(app_handle);
    Ok(result)
}

/// 新增原生供应商后（当全局聚合模式开启且源应用已开启显示时）自动同步兼容到其余可见应用。
pub async fn auto_sync_added_provider_if_enabled(
    state: &AppState,
    source_app: &AppType,
    provider_id: &str,
) {
    let settings = crate::settings::get_settings();
    if !settings.enable_stack_mode {
        return;
    }
    let visible_apps = settings.visible_apps.unwrap_or_default();
    if !visible_apps.is_visible(source_app) {
        return;
    }
    if is_generated_sync_id(provider_id) {
        return;
    }
    let Ok(Some(provider)) = state.db.get_provider_by_id(provider_id, source_app) else {
        return;
    };
    if provider.meta.as_ref().and_then(|m| m.aggregation_disabled) == Some(true) {
        return;
    }
    let Some(spec) = spec_from_app_provider(source_app, &provider) else {
        return;
    };
    let res =
        sync_spec_to_target_apps(state, &spec, Some(source_app), provider_id, &visible_apps).await;
    let _ = activate_aggregation_for_visible_apps(state).await;
    if !res.warnings.is_empty() {
        log::debug!(
            "[Aggregation] Auto-sync warnings for {}/{provider_id}: {:?}",
            source_app.as_str(),
            res.warnings
        );
    }
}

async fn sync_spec_to_target_apps(
    state: &AppState,
    spec: &NormalizedProviderSpec,
    source_app: Option<&AppType>,
    source_id: &str,
    visible_apps: &crate::settings::VisibleApps,
) -> SyncAllResult {
    let mut synced_apps = Vec::new();
    let mut warnings = Vec::new();

    for target_app in UniversalProviderApps::COMPATIBLE_APPS {
        if !visible_apps.is_visible(&target_app) {
            continue;
        }
        if source_app == Some(&target_app) {
            // 源应用本身若支持 Stack，直接把它加入自己的 Stack 名单
            if target_app.spec().stack {
                let _ = ensure_in_stack(state, &target_app, source_id).await;
            }
            synced_apps.push(target_app.as_str().to_string());
            continue;
        }
        let Some(src) = source_app else {
            continue;
        };
        let child_id = synced_child_id_for_app_provider(&target_app, src, source_id);
        let Some(provider) = build_compatible_provider(spec, &target_app, &child_id) else {
            continue;
        };
        match save_synced_provider_for_app(state, &target_app, provider).await {
            Ok(warn) => {
                synced_apps.push(target_app.as_str().to_string());
                if let Some(w) = warn {
                    warnings.push(w);
                }
            }
            Err(e) => warnings.push(format!("{}: {e}", target_app.as_str())),
        }
    }

    SyncAllResult {
        synced_apps,
        warnings,
    }
}

/// 将兼容投影的 `Provider` 存入目标应用数据库；支持 Stack 的应用自动加入其 Stack 名单；
/// 当前正使用该子条目的应用（或已接管 live 的累加式应用）自动刷新 live 配置。
async fn save_synced_provider_for_app(
    state: &AppState,
    target_app: &AppType,
    mut provider: Provider,
) -> Result<Option<String>, String> {
    if let Err(e) = ProviderService::validate_provider_settings(target_app, &provider) {
        return Err(e.to_string());
    }

    let existing = state
        .db
        .get_provider_by_id(&provider.id, target_app)
        .map_err(|e| e.to_string())?;
    let was_live_managed = existing
        .as_ref()
        .and_then(|p| p.meta.as_ref())
        .and_then(|m| m.live_config_managed)
        .unwrap_or(false);

    if let Some(existing) = existing {
        // 保留已有应用内排序和用户自定义元数据（如 endpoints / usage_script）
        provider.created_at = existing.created_at.or(provider.created_at);
        provider.sort_index = existing.sort_index.or(provider.sort_index);
        provider.in_failover_queue = existing.in_failover_queue;
        if let Some(mut existing_meta) = existing.meta {
            if let Some(new_meta) = provider.meta.take() {
                if new_meta.api_format.is_some() {
                    existing_meta.api_format = new_meta.api_format;
                }
                if new_meta.stack_models.is_some() {
                    existing_meta.stack_models = new_meta.stack_models;
                }
                if new_meta.claude_desktop_mode.is_some() {
                    existing_meta.claude_desktop_mode = new_meta.claude_desktop_mode;
                }
                if !new_meta.claude_desktop_model_routes.is_empty() {
                    existing_meta.claude_desktop_model_routes =
                        new_meta.claude_desktop_model_routes;
                }
                existing_meta.is_full_url = new_meta.is_full_url;
                existing_meta.api_key_field = new_meta.api_key_field;
                existing_meta.custom_user_agent = new_meta.custom_user_agent;
            }
            provider.meta = Some(existing_meta);
        }
    }

    let provider_id = provider.id.clone();
    state
        .db
        .save_provider(target_app, &provider)
        .map_err(|e| e.to_string())?;

    let mut warning = None;
    if target_app.spec().stack {
        if let Err(e) = ensure_in_stack(state, target_app, &provider_id).await {
            warning = Some(format!("{} 加入聚合名单失败: {e}", target_app.as_str()));
        }
    }

    // 正在使用或处于聚合模式开启状态的累加式应用，自动启用并写入 live 配置
    if target_app.spec().additive {
        let agg_enabled = crate::settings::get_settings().enable_stack_mode;
        if agg_enabled || (was_live_managed && additive_app_has_live_config(target_app)) {
            let mut managed_provider = provider.clone();
            if agg_enabled
                && managed_provider
                    .meta
                    .as_ref()
                    .and_then(|m| m.live_config_managed)
                    != Some(true)
            {
                managed_provider
                    .meta
                    .get_or_insert_with(Default::default)
                    .live_config_managed = Some(true);
                if let Err(e) = state.db.save_provider(target_app, &managed_provider) {
                    warning = Some(format!("{} 更新启用状态失败: {e}", target_app.as_str()));
                }
            }
            if let Err(e) = sync_additive_child_live(state, target_app, &managed_provider) {
                warning = Some(format!("{} 更新配置文件失败: {e}", target_app.as_str()));
            }
        }
    } else {
        let is_current = crate::mode::current::provider_for(
            &state.db,
            target_app,
            crate::mode::current::Purpose::InUse,
        )
        .ok()
        .flatten()
        .as_deref()
            == Some(provider_id.as_str());
        if is_current {
            if let Err(e) =
                ProviderService::sync_current_provider_for_app(state, target_app.clone())
            {
                warning = Some(format!("{} 刷新当前配置失败: {e}", target_app.as_str()));
            }
        }
    }

    Ok(warning)
}

async fn ensure_in_stack(state: &AppState, app: &AppType, provider_id: &str) -> Result<(), String> {
    let store = crate::live::engine::DeviceStore::for_device();
    let already = crate::mode::state::stack(&store, app.as_str())
        .map(|s| s.is_member(provider_id))
        .unwrap_or(false);
    let mode = crate::mode::current::mode_state(app);
    // 已经是成员且当前没接上 Stack 时无需重写；已接上 Stack 时调用 set_stack_member 同步刷新目录
    if already && !mode.is_proxy() {
        return Ok(());
    }
    crate::mode::controller::set_stack_member(state, app, provider_id, true)
        .await
        .map(|_| ())
        .map_err(|e| e.message)
}

fn additive_app_has_live_config(app: &AppType) -> bool {
    match app {
        AppType::OpenCode => {
            crate::opencode_config::get_opencode_config_path().is_ok_and(|p| p.exists())
        }
        AppType::OpenClaw => crate::openclaw_config::get_openclaw_config_path().exists(),
        AppType::Hermes => crate::hermes_config::get_hermes_config_path().exists(),
        AppType::Pi => crate::pi_config::get_pi_models_path().is_ok_and(|p| p.exists()),
        AppType::Omp => crate::omp_config::get_omp_models_path().is_ok_and(|p| p.exists()),
        AppType::Kimi => crate::kimi_config::config_path().exists(),
        AppType::Copilot => crate::copilot_config::providers_path().exists(),
        AppType::Mcode => crate::mcode_config::config_path().exists(),
        AppType::Dsh => crate::dsh_config::config_path().exists(),
        AppType::WorkBuddy => crate::workbuddy_config::config_path().exists(),
        _ => false,
    }
}

fn sync_additive_child_live(
    state: &AppState,
    app: &AppType,
    provider: &Provider,
) -> Result<(), AppError> {
    match app {
        AppType::Pi => super::pi::upsert_live(state, provider),
        AppType::Omp => super::omp::upsert_live(state, provider),
        AppType::Kimi => crate::kimi_config::save_and_commit(
            &provider.id,
            Some(provider.settings_config.clone()),
            true,
            || Ok(()),
        ),
        AppType::Copilot => crate::copilot_config::save_and_commit(
            &provider.id,
            Some(&provider.settings_config),
            true,
            || Ok(()),
        ),
        _ => crate::services::provider::live::sync_additive_app_to_live(state, app),
    }
}

/// 将所有可见应用内就绪的非生成供应商跨应用同步兼容（开启聚合开关时调用）。
async fn sync_all_ready_app_providers_inner(
    state: &AppState,
    visible_apps: &crate::settings::VisibleApps,
) -> SyncAllResult {
    let mut synced_set = std::collections::BTreeSet::new();
    let mut warnings = Vec::new();

    for app in &UniversalProviderApps::COMPATIBLE_APPS {
        if !visible_apps.is_visible(app) {
            continue;
        }
        let Ok(providers) = state.db.get_all_providers(app) else {
            continue;
        };
        let mut ids: Vec<String> = providers.keys().cloned().collect();
        ids.sort();
        for pid in ids {
            if is_generated_sync_id(&pid) {
                continue;
            }
            let Some(provider) = providers.get(&pid) else {
                continue;
            };
            if provider.meta.as_ref().and_then(|m| m.aggregation_disabled) == Some(true) {
                continue;
            }
            let Some(spec) = spec_from_app_provider(app, provider) else {
                continue;
            };
            let res = sync_spec_to_target_apps(state, &spec, Some(app), &pid, visible_apps).await;
            synced_set.extend(res.synced_apps);
            warnings.extend(res.warnings);
        }
    }

    SyncAllResult {
        synced_apps: synced_set.into_iter().collect(),
        warnings,
    }
}

/// 为路由/聚合型应用挑选可用的非官方路由供应商 ID。
fn select_aggregation_route_id<'a>(
    current_route: Option<&str>,
    active_id: Option<&str>,
    providers: impl IntoIterator<Item = (&'a String, &'a Provider)>,
) -> Option<String> {
    let map: std::collections::BTreeMap<&str, &Provider> = providers
        .into_iter()
        .map(|(k, v)| (k.as_str(), v))
        .collect();
    let is_routable = |id: &str| {
        map.get(id)
            .map(|p| p.category.as_deref() != Some("official"))
            .unwrap_or(false)
    };

    if let Some(route) = current_route.filter(|id| is_routable(id)) {
        return Some(route.to_string());
    }
    if let Some(active) = active_id.filter(|id| is_routable(id)) {
        return Some(active.to_string());
    }

    map.iter()
        .find(|(_, p)| p.category.as_deref() != Some("official"))
        .map(|(id, _)| (*id).to_string())
}

/// 当全局聚合模式开启时，为所有可见的路由/聚合应用自动加入 Stack、启用聚合/代理模式并刷新 `/model` 目录。
pub async fn activate_aggregation_for_visible_apps(state: &AppState) -> Vec<String> {
    let settings = crate::settings::get_settings();
    if !settings.enable_stack_mode {
        return Vec::new();
    }
    let visible_apps = settings.visible_apps.unwrap_or_default();
    let mut warnings = Vec::new();

    for app in &UniversalProviderApps::COMPATIBLE_APPS {
        if !visible_apps.is_visible(app) || !app.spec().route_mode {
            continue;
        }
        let Ok(providers) = state.db.get_all_providers(app) else {
            continue;
        };
        if providers.is_empty() {
            continue;
        }

        if app.spec().stack {
            let mut ids: Vec<&String> = providers
                .iter()
                .filter(|(_, p)| p.category.as_deref() != Some("official"))
                .map(|(id, _)| id)
                .collect();
            ids.sort();
            for pid in ids {
                if let Err(e) = ensure_in_stack(state, app, pid).await {
                    warnings.push(format!("{} 加入聚合名单失败: {e}", app.as_str()));
                }
            }
        }

        let current_route = crate::mode::current::mode_state(app).proxy_route;
        let active_id = crate::mode::current::provider_for(
            &state.db,
            app,
            crate::mode::current::Purpose::InUse,
        )
        .ok()
        .flatten();
        let Some(route_id) = select_aggregation_route_id(
            current_route.as_deref(),
            active_id.as_deref(),
            providers.iter(),
        ) else {
            continue;
        };

        if let Err(err) =
            crate::mode::controller::enter_with_route(state, app, app.spec().stack, Some(&route_id))
                .await
        {
            warnings.push(format!("{} 自动启用聚合模式失败: {err}", app.as_str()));
        }
    }

    warnings
}

/// 安全移除某个同步生成的兼容子供应商（`agg-*` / `universal-*`）：
/// 1) 累加/启动器应用直接走 `ProviderService::delete`（连带清理其 live 文件）；
/// 2) 路由/聚合型应用先移出 Stack，并在当前代理路由或直连指针指向它时切换回其他可用供应商或退出代理，最后删除 DB 记录。
async fn remove_synced_child_for_app(
    state: &AppState,
    target_app: &AppType,
    child_id: &str,
) -> Result<bool, String> {
    let existing = state
        .db
        .get_provider_by_id(child_id, target_app)
        .map_err(|e| e.to_string())?;
    if existing.is_none() {
        return Ok(false);
    }

    if target_app.spec().additive || matches!(target_app, AppType::Agy) {
        ProviderService::delete(state, target_app.clone(), child_id).map_err(|e| e.to_string())?;
        return Ok(true);
    }

    if target_app.spec().stack
        && crate::mode::stack::is_member(target_app, child_id).unwrap_or(false)
    {
        crate::mode::controller::set_stack_member(state, target_app, child_id, false)
            .await
            .map_err(|e| e.message)?;
    }

    let mode = crate::mode::current::mode_state(target_app);
    let direct_ptr = crate::mode::current::provider_for(
        &state.db,
        target_app,
        crate::mode::current::Purpose::Direct,
    )
    .ok()
    .flatten();
    let is_routed = mode.routes_to(child_id);
    let is_direct = direct_ptr.as_deref() == Some(child_id);

    if is_routed || is_direct {
        let all = state
            .db
            .get_all_providers(target_app)
            .map_err(|e| e.to_string())?;
        let remaining_routable = select_aggregation_route_id(
            None,
            None,
            all.iter().filter(|(k, _)| k.as_str() != child_id),
        );
        let remaining_any = remaining_routable
            .clone()
            .or_else(|| all.keys().filter(|k| k.as_str() != child_id).min().cloned());

        if is_routed {
            if let Some(next_route) = remaining_routable.as_deref() {
                crate::mode::controller::enter_with_route(
                    state,
                    target_app,
                    target_app.spec().stack,
                    Some(next_route),
                )
                .await?;
            } else {
                crate::mode::controller::exit(state, target_app).await?;
            }
        }

        if is_direct {
            if let Some(fb) = remaining_any {
                if !crate::mode::current::mode_state(target_app).is_proxy() {
                    let _ = ProviderService::switch(state, target_app.clone(), &fb);
                } else {
                    let _ = crate::settings::set_current_provider(target_app, Some(&fb));
                    let _ = state.db.set_current_provider(target_app, &fb);
                }
            } else {
                let _ = crate::settings::set_current_provider(target_app, None);
                let _ = state.db.set_current_provider(target_app, "");
                let _ = crate::mode::controller::exit(state, target_app).await;
            }
        }
    }

    state
        .db
        .delete_provider(target_app, child_id)
        .map_err(|e| e.to_string())?;
    Ok(true)
}

/// 取消指定供应商的跨应用兼容同步：
/// - 若 `source_app == None`（通用供应商）：关闭所有目标应用开关并清理生成的 `universal-*` 子配置；
/// - 若 `source_app == Some(app)`（原生应用供应商）：标记 `meta.aggregation_disabled = Some(true)`，并清理所有其他应用内生成的 `agg-*` 子配置。
pub async fn unsync_provider_from_all_apps(
    state: &AppState,
    app_handle: &tauri::AppHandle,
    source_app: Option<AppType>,
    provider_id: &str,
) -> Result<SyncAllResult, String> {
    let mut removed_apps = Vec::new();
    let mut warnings = Vec::new();

    match source_app {
        None => {
            let mut universal = state
                .db
                .get_universal_provider(provider_id)
                .map_err(|e| e.to_string())?
                .ok_or_else(|| format!("通用供应商不存在: {provider_id}"))?;
            universal.apps = UniversalProviderApps::default();
            state
                .db
                .save_universal_provider(&universal)
                .map_err(|e| e.to_string())?;

            for target_app in UniversalProviderApps::COMPATIBLE_APPS {
                let child_id = synced_child_id_for_universal(&target_app, provider_id);
                let mut removed = false;
                match remove_synced_child_for_app(state, &target_app, &child_id).await {
                    Ok(true) => removed = true,
                    Ok(false) => {}
                    Err(e) => warnings.push(format!("{}: {e}", target_app.as_str())),
                }
                let legacy_id = format!("universal-{provider_id}");
                if legacy_id != child_id {
                    match remove_synced_child_for_app(state, &target_app, &legacy_id).await {
                        Ok(true) => removed = true,
                        Ok(false) => {}
                        Err(e) => warnings.push(format!("{}: {e}", target_app.as_str())),
                    }
                }
                if removed {
                    removed_apps.push(target_app.as_str().to_string());
                }
            }
        }
        Some(src_app) => {
            let mut source_provider = state
                .db
                .get_provider_by_id(provider_id, &src_app)
                .map_err(|e| e.to_string())?
                .ok_or_else(|| format!("供应商不存在: {provider_id}"))?;
            source_provider
                .meta
                .get_or_insert_with(Default::default)
                .aggregation_disabled = Some(true);
            state
                .db
                .save_provider(&src_app, &source_provider)
                .map_err(|e| e.to_string())?;

            for target_app in UniversalProviderApps::COMPATIBLE_APPS {
                if target_app == src_app {
                    continue;
                }
                let child_id = synced_child_id_for_app_provider(&target_app, &src_app, provider_id);
                match remove_synced_child_for_app(state, &target_app, &child_id).await {
                    Ok(true) => removed_apps.push(target_app.as_str().to_string()),
                    Ok(false) => {}
                    Err(e) => warnings.push(format!("{}: {e}", target_app.as_str())),
                }
            }
        }
    }

    if crate::settings::get_settings().enable_stack_mode {
        warnings.extend(activate_aggregation_for_visible_apps(state).await);
    }

    crate::tray::refresh_tray_menu(app_handle);
    Ok(SyncAllResult {
        synced_apps: removed_apps,
        warnings,
    })
}

/// 在全局聚合页内一键让某个应用接入或退出聚合（代理模式）。
pub async fn set_aggregation_app_mode(
    state: &AppState,
    app_handle: &tauri::AppHandle,
    app: AppType,
    enabled: bool,
    route_provider_id: Option<String>,
) -> Result<(), String> {
    if !app.spec().route_mode {
        return Err(format!("{} 不支持本地代理模式", app.as_str()));
    }
    if enabled && !current_visible_apps().is_visible(&app) {
        return Err(format!("{} 未在应用页面开启显示", app.as_str()));
    }
    if enabled {
        let use_stack = app.spec().stack;
        crate::mode::controller::enter_with_route(
            state,
            &app,
            use_stack,
            route_provider_id.as_deref(),
        )
        .await?;
    } else {
        crate::mode::controller::exit(state, &app).await?;
    }
    crate::tray::refresh_tray_menu(app_handle);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{UniversalProviderApps, UniversalProviderModels};

    #[test]
    fn builds_valid_compatible_provider_for_all_15_apps() {
        let u = UniversalProvider {
            id: "u1".to_string(),
            name: "Test Universal".to_string(),
            provider_type: "custom_openai".to_string(),
            apps: UniversalProviderApps::all_compatible(),
            base_url: "https://api.example.com".to_string(),
            api_key: "sk-test-123".to_string(),
            models: UniversalProviderModels::default(),
            api_format: Some("openai_chat".to_string()),
            extra_models: vec!["gpt-4o".to_string(), "claude-sonnet-4-20250514".to_string()],
            website_url: None,
            notes: None,
            icon: None,
            icon_color: None,
            meta: None,
            created_at: Some(100),
            sort_index: Some(0),
        };
        let spec = spec_from_universal(&u);
        assert_eq!(spec.api_format, "openai_chat");
        assert_eq!(spec.models.len(), 2);

        for app in UniversalProviderApps::COMPATIBLE_APPS {
            let built = build_compatible_provider(&spec, &app, "universal-u1")
                .unwrap_or_else(|| panic!("expected provider for {:?}", app));
            assert!(
                ProviderService::validate_provider_settings(&app, &built).is_ok(),
                "validation failed for {:?}: {:?}",
                app,
                ProviderService::validate_provider_settings(&app, &built)
            );
        }
        assert!(build_compatible_provider(&spec, &AppType::Agy, "universal-u1").is_none());
    }

    #[test]
    fn masks_hidden_apps_from_universal_apps_and_overview() {
        let mut apps = UniversalProviderApps::all_compatible();
        let mut visible = crate::settings::VisibleApps::default();
        visible.codex = false;
        visible.pi = false;
        visible.kimi = false;

        mask_universal_apps_to_visible(&mut apps, &visible);
        assert!(apps.claude);
        assert!(!apps.codex);
        assert!(!apps.pi);
        assert!(!apps.kimi);

        let db = std::sync::Arc::new(crate::database::Database::memory().expect("in-memory db"));
        let state = AppState::new(db);

        let claude_p = Provider::with_id(
            "c1".to_string(),
            "Claude Custom".to_string(),
            json!({
                "env": {
                    "ANTHROPIC_BASE_URL": "https://relay.example.com",
                    "ANTHROPIC_AUTH_TOKEN": "sk-1"
                }
            }),
            None,
        );
        let codex_p = Provider::with_id(
            "x1".to_string(),
            "Codex Hidden".to_string(),
            json!({
                "auth": { "OPENAI_API_KEY": "sk-2" },
                "config": "base_url = \"https://relay.example.com/v1\"\nmodel = \"gpt-4o\""
            }),
            None,
        );
        state.db.save_provider(&AppType::Claude, &claude_p).unwrap();
        state.db.save_provider(&AppType::Codex, &codex_p).unwrap();

        let overview = build_aggregation_overview_sync(
            &state,
            true,
            crate::proxy::types::ProxyStatus::default(),
            &visible,
        )
        .unwrap();

        assert!(overview
            .app_providers
            .iter()
            .any(|p| p.app_type == "claude"));
        assert!(!overview.app_providers.iter().any(|p| p.app_type == "codex"));
        assert!(overview.app_states.iter().any(|s| s.app_type == "claude"));
        assert!(!overview.app_states.iter().any(|s| s.app_type == "codex"));
        assert!(!overview.app_states.iter().any(|s| s.app_type == "pi"));
    }

    #[test]
    fn inspects_custom_auth_default_provider_and_sets_can_sync_accurately() {
        // Claude default with only ANTHROPIC_AUTH_TOKEN (official endpoint inferred)
        let claude_no_url = Provider::with_id(
            "default".to_string(),
            "default".to_string(),
            json!({
                "env": { "ANTHROPIC_AUTH_TOKEN": "sk-ant-123" }
            }),
            None,
        );
        let (url, key, _, _) = inspect_app_provider(&AppType::Claude, &claude_no_url);
        assert_eq!(url.as_deref(), Some("https://api.anthropic.com"));
        assert_eq!(key.as_deref(), Some("sk-ant-123"));

        // Codex default with custom auth key name
        let codex_custom_auth = Provider::with_id(
            "default".to_string(),
            "default".to_string(),
            json!({
                "auth": { "CUSTOM_GATEWAY_KEY": "sk-codex-custom" },
                "config": "base_url = \"https://relay.example.com/v1\"\nmodel = \"gpt-4o\""
            }),
            None,
        );
        let (url, key, _, _) = inspect_app_provider(&AppType::Codex, &codex_custom_auth);
        assert_eq!(url.as_deref(), Some("https://relay.example.com/v1"));
        assert_eq!(key.as_deref(), Some("sk-codex-custom"));

        // Local endpoint without API key gets local-no-key fallback
        let local_ollama = Provider::with_id(
            "default".to_string(),
            "default".to_string(),
            json!({
                "env": { "ANTHROPIC_BASE_URL": "http://localhost:11434" }
            }),
            None,
        );
        let (url, key, _, _) = inspect_app_provider(&AppType::Claude, &local_ollama);
        assert_eq!(url.as_deref(), Some("http://localhost:11434"));
        assert_eq!(key.as_deref(), Some("local-no-key"));

        // Remote URL missing key sets can_sync = false in overview
        let db = std::sync::Arc::new(crate::database::Database::memory().expect("in-memory db"));
        let state = AppState::new(db);
        let missing_key = Provider::with_id(
            "default".to_string(),
            "default".to_string(),
            json!({ "env": { "ANTHROPIC_BASE_URL": "https://remote.example.com" } }),
            None,
        );
        state
            .db
            .save_provider(&AppType::Claude, &missing_key)
            .unwrap();
        let overview = build_aggregation_overview_sync(
            &state,
            false,
            crate::proxy::types::ProxyStatus::default(),
            &crate::settings::VisibleApps::default(),
        )
        .unwrap();
        let item = overview
            .app_providers
            .iter()
            .find(|p| p.provider.id == "default")
            .unwrap();
        assert!(!item.can_sync);
    }

    #[test]
    fn codex_multi_model_catalog_includes_all_extra_models() {
        let mut apps = UniversalProviderApps::all_compatible();
        apps.claude = false;
        apps.gemini = false;
        let up = UniversalProvider {
            id: "u1".to_string(),
            name: "Uni Multi".to_string(),
            provider_type: "custom".to_string(),
            apps,
            base_url: "https://relay.example.com".to_string(),
            api_key: "sk-uni".to_string(),
            models: UniversalProviderModels {
                codex: Some(crate::provider::CodexModelConfig {
                    model: Some("gpt-5.4".to_string()),
                    reasoning_effort: Some("high".to_string()),
                }),
                ..Default::default()
            },
            website_url: None,
            notes: None,
            meta: None,
            icon: None,
            icon_color: None,
            created_at: None,
            sort_index: None,
            extra_models: vec!["gpt-4.1".to_string(), "o3".to_string()],
            api_format: Some("openai_responses".to_string()),
        };

        let codex_from_up = up.to_codex_provider().expect("codex provider");
        assert!(
            ProviderService::validate_provider_settings(&AppType::Codex, &codex_from_up).is_ok()
        );
        let up_models: Vec<&str> = codex_from_up
            .settings_config
            .get("modelCatalog")
            .and_then(|c| c.get("models"))
            .and_then(|v| v.as_array())
            .expect("modelCatalog.models array on to_codex_provider")
            .iter()
            .filter_map(|item| item.get("model").and_then(|m| m.as_str()))
            .collect();
        assert_eq!(up_models, vec!["gpt-5.4", "gpt-4.1", "o3"]);

        let spec = spec_from_universal(&up);
        let codex_from_spec =
            build_compatible_provider(&spec, &AppType::Codex, "agg-claude-c1").unwrap();
        assert!(
            ProviderService::validate_provider_settings(&AppType::Codex, &codex_from_spec).is_ok()
        );
        let spec_models: Vec<&str> = codex_from_spec
            .settings_config
            .get("modelCatalog")
            .and_then(|c| c.get("models"))
            .and_then(|v| v.as_array())
            .expect("modelCatalog.models array on build_compatible_provider")
            .iter()
            .filter_map(|item| item.get("model").and_then(|m| m.as_str()))
            .collect();
        assert_eq!(spec_models, vec!["gpt-5.4", "gpt-4.1", "o3"]);

        // Codex 原生供应商的 modelCatalog.models 能完整保留并跨应用分发到 Claude / GrokBuild / Pi
        let (_, _, _, inspected_models) = inspect_app_provider(&AppType::Codex, &codex_from_up);
        assert_eq!(inspected_models, vec!["gpt-5.4", "gpt-4.1", "o3"]);

        let app_spec = spec_from_app_provider(&AppType::Codex, &codex_from_up).unwrap();
        let claude_proj =
            build_compatible_provider(&app_spec, &AppType::Claude, "agg-codex-u1").unwrap();
        let claude_models: Vec<&str> = claude_proj
            .meta
            .as_ref()
            .and_then(|m| m.stack_models.as_deref())
            .unwrap()
            .iter()
            .map(|m| m.model.as_str())
            .collect();
        assert_eq!(claude_models, vec!["gpt-5.4", "gpt-4.1", "o3"]);

        let pi_proj = build_compatible_provider(&app_spec, &AppType::Pi, "agg-codex-u1").unwrap();
        let pi_models: Vec<&str> = pi_proj
            .settings_config
            .get("models")
            .and_then(|v| v.as_array())
            .unwrap()
            .iter()
            .filter_map(|e| e.get("id").and_then(|v| v.as_str()))
            .collect();
        assert_eq!(pi_models, vec!["gpt-5.4", "gpt-4.1", "o3"]);

        let grok_proj =
            build_compatible_provider(&app_spec, &AppType::GrokBuild, "agg-codex-u1").unwrap();
        let (_, _, _, grok_models) = inspect_app_provider(&AppType::GrokBuild, &grok_proj);
        assert_eq!(grok_models, vec!["gpt-5.4", "gpt-4.1", "o3"]);

        // Hermes UI 数组形态与 YAML 对象形态均能完整提取多模型，并支持带点号源 ID 的跨应用 ID 对齐
        let hermes_ui = Provider::with_id(
            "relay.v1".to_string(),
            "Hermes UI".to_string(),
            json!({
                "base_url": "https://relay.example.com/v1",
                "api_key": "sk-hermes",
                "models": [{ "id": "m1" }, { "id": "m2" }]
            }),
            Some("custom".to_string()),
        );
        let (_, _, _, hermes_models) = inspect_app_provider(&AppType::Hermes, &hermes_ui);
        assert_eq!(hermes_models, vec!["m1", "m2"]);

        let opencode_id =
            synced_child_id_for_app_provider(&AppType::OpenCode, &AppType::Hermes, "relay.v1");
        let built_opencode = build_compatible_provider(
            &app_spec,
            &AppType::OpenCode,
            "agg-opencode-hermes-relay.v1",
        )
        .unwrap();
        assert_eq!(built_opencode.id, opencode_id);
    }

    #[test]
    fn select_aggregation_route_id_skips_official_and_picks_routable_provider() {
        let mut providers = indexmap::IndexMap::new();
        let mut official = Provider::with_id(
            "default".to_string(),
            "Official".to_string(),
            json!({}),
            Some("official".to_string()),
        );
        official.category = Some("official".to_string());
        providers.insert("default".to_string(), official);

        assert_eq!(
            select_aggregation_route_id(Some("default"), Some("default"), providers.iter()),
            None
        );

        let agg = Provider::with_id(
            "agg-claude-c1".to_string(),
            "Agg".to_string(),
            json!({}),
            Some("aggregator".to_string()),
        );
        let custom = Provider::with_id(
            "custom-1".to_string(),
            "Custom".to_string(),
            json!({}),
            Some("custom".to_string()),
        );
        providers.insert("agg-claude-c1".to_string(), agg);
        providers.insert("custom-1".to_string(), custom);

        // Skips official current_route/active_id and deterministically selects routable provider
        assert_eq!(
            select_aggregation_route_id(Some("default"), Some("default"), providers.iter()),
            Some("agg-claude-c1".to_string())
        );
        // Preserves existing routable proxy_route / active_id
        assert_eq!(
            select_aggregation_route_id(Some("custom-1"), Some("default"), providers.iter()),
            Some("custom-1".to_string())
        );
    }

    #[tokio::test]
    async fn sync_all_ready_app_providers_skips_aggregation_disabled() {
        let db = std::sync::Arc::new(crate::database::Database::memory().expect("in-memory db"));
        let state = AppState::new(db);

        let mut opt_out = Provider::with_id(
            "opt-out-1".to_string(),
            "Opt Out Codex".to_string(),
            json!({
                "auth": { "OPENAI_API_KEY": "sk-opt-out" },
                "config": "base_url = \"https://relay.example.com/v1\"\nmodel = \"gpt-5.4\""
            }),
            Some("custom".to_string()),
        );
        opt_out
            .meta
            .get_or_insert_with(Default::default)
            .aggregation_disabled = Some(true);
        state.db.save_provider(&AppType::Codex, &opt_out).unwrap();

        let enabled_claude = Provider::with_id(
            "ready-1".to_string(),
            "Ready Claude".to_string(),
            json!({
                "env": {
                    "ANTHROPIC_BASE_URL": "https://relay.example.com",
                    "ANTHROPIC_AUTH_TOKEN": "sk-ready"
                }
            }),
            Some("custom".to_string()),
        );
        state
            .db
            .save_provider(&AppType::Claude, &enabled_claude)
            .unwrap();

        // Only enable Claude, Codex, and Gemini so test runs cleanly in-memory without touching live additive files
        let visible = crate::settings::VisibleApps {
            claude: true,
            codex: true,
            gemini: true,
            claude_desktop: false,
            grokbuild: false,
            opencode: false,
            openclaw: false,
            hermes: false,
            pi: false,
            omp: false,
            mcode: false,
            kimi: false,
            dsh: false,
            copilot: false,
            workbuddy: false,
            agy: false,
        };

        let _ = sync_all_ready_app_providers_inner(&state, &visible).await;

        // Opt-out Codex provider must NOT be synced to Gemini
        let opt_out_gemini_id =
            synced_child_id_for_app_provider(&AppType::Gemini, &AppType::Codex, "opt-out-1");
        assert!(state
            .db
            .get_provider_by_id(&opt_out_gemini_id, &AppType::Gemini)
            .unwrap()
            .is_none());

        // Ready Claude provider MUST be synced to Gemini
        let ready_gemini_id =
            synced_child_id_for_app_provider(&AppType::Gemini, &AppType::Claude, "ready-1");
        assert!(state
            .db
            .get_provider_by_id(&ready_gemini_id, &AppType::Gemini)
            .unwrap()
            .is_some());

        // Removing synced child deletes it cleanly from target DB
        assert!(
            remove_synced_child_for_app(&state, &AppType::Gemini, &ready_gemini_id)
                .await
                .unwrap()
        );
        assert!(state
            .db
            .get_provider_by_id(&ready_gemini_id, &AppType::Gemini)
            .unwrap()
            .is_none());
    }
}
