//! 跨应用复制供应商：以源应用的 base_url + api_key 为中间凭据，
//! 按目标应用的原生配置形状重建供应商。

use std::collections::HashMap;
use std::str::FromStr;

use serde::Serialize;
use serde_json::{json, Map, Value};
use tauri::Manager;

use super::provider::{claude_provider_models_are_claude_safe, suggested_claude_desktop_routes};
use crate::app_config::AppType;
use crate::error::AppError;
use crate::provider::{ClaudeDesktopMode, ClaudeDesktopModelRoute, Provider, ProviderMeta};
use crate::services::ProviderService;
use crate::store::AppState;

/// 跨应用副本最多携带的模型数量。
const SOURCE_MODEL_LIMIT: usize = 8;

/// 复制结果的逐目标状态。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CopyTargetOutcome {
    pub target_app: String,
    pub status: CopyStatus,
    pub new_provider_id: Option<String>,
    pub reason: Option<CopyFailureReason>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CopyStatus {
    Copied,
    Skipped,
    Failed,
}

/// 复制被跳过/失败的原因。`key` 是 i18n 键（`provider.copyToApps.reasons.*`），
/// `params` 供前端插值渲染当前语言，`fallback` 是无翻译时的兜底文案。
#[derive(Debug, Clone, Serialize)]
pub struct CopyFailureReason {
    pub key: String,
    pub params: HashMap<String, String>,
    pub fallback: String,
}

impl CopyFailureReason {
    fn new(key: &str, fallback: String) -> Self {
        Self {
            key: key.to_string(),
            params: HashMap::new(),
            fallback,
        }
    }

    fn with_param(mut self, name: &str, value: impl Into<String>) -> Self {
        self.params.insert(name.to_string(), value.into());
        self
    }
}

/// 从源供应商提取 base_url + api_key 作为跨应用中间凭据。
/// 官方 / 代理注入 OAuth / 托管认证供应商无法提取，返回拒绝原因。
fn source_rejection(source: &Provider, source_app: &AppType) -> Option<AppError> {
    if crate::database::is_official_seed_id(&source.id)
        || source.category.as_deref() == Some("official")
    {
        return Some(AppError::localized(
            "provider.copyToApps.reasons.officialSeed",
            "官方供应商由应用内置管理，无法跨应用复制",
            "Official providers are managed by the app itself and cannot be copied across apps",
        ));
    }

    if source.uses_proxy_injected_oauth() {
        return Some(AppError::localized(
            "provider.copyToApps.reasons.managedAuth",
            "该供应商的凭据由本地代理注入，无法提取为跨应用副本",
            "This provider's credentials are injected by the local proxy and cannot be extracted",
        ));
    }

    let (base_url, api_key) = source.resolve_usage_credentials(source_app);
    if base_url.trim().is_empty() || api_key.trim().is_empty() {
        return Some(AppError::localized(
            "provider.copyToApps.reasons.missingCredentials",
            "无法从该供应商提取 base_url 与 API key，无法跨应用复制",
            "Cannot extract base_url and API key from this provider",
        ));
    }

    None
}

/// 每个源应用一套模型名提取：去重保序、cap 8、空列表不写模型字段。
fn extract_source_model_names(
    app: &AppType,
    settings_config: &Value,
    _meta: Option<&ProviderMeta>,
) -> Vec<String> {
    let mut models: Vec<String> = Vec::new();
    let mut push = |candidate: Option<&str>| {
        let Some(raw) = candidate else {
            return;
        };
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return;
        }
        // Claude 端 env 值可能带 [1M] 后缀（上下文窗口标记）；目标应用无法
        // 消费该后缀，复制边界上剥掉，避免把后缀当模型名写入目标。
        let marker = crate::claude_desktop_config::ONE_M_CONTEXT_MARKER.as_bytes();
        let bytes = trimmed.as_bytes();
        let stripped = if bytes.len() >= marker.len()
            && bytes[bytes.len() - marker.len()..].eq_ignore_ascii_case(marker)
        {
            trimmed[..trimmed.len() - marker.len()].trim_end()
        } else {
            trimmed
        };
        if stripped.is_empty() {
            return;
        }
        if !models.iter().any(|existing| existing == stripped) {
            models.push(stripped.to_string());
        }
    };

    match app {
        AppType::Claude | AppType::ClaudeDesktop => {
            let env = settings_config.get("env").and_then(Value::as_object);
            for key in [
                "ANTHROPIC_MODEL",
                "ANTHROPIC_DEFAULT_SONNET_MODEL",
                "ANTHROPIC_DEFAULT_OPUS_MODEL",
                "ANTHROPIC_DEFAULT_HAIKU_MODEL",
            ] {
                push(env.and_then(|env| env.get(key)).and_then(Value::as_str));
            }
        }
        AppType::Codex => {
            // 真实存储形状：config 是 TOML 文本（codex 官方 seed 同形），
            // 顶层 model = "..."；TOML 解析失败按无模型处理。
            let document = settings_config
                .get("config")
                .and_then(Value::as_str)
                .and_then(|config_toml| config_toml.parse::<toml::Value>().ok());
            if let Some(model) = document
                .as_ref()
                .and_then(|document| document.get("model"))
                .and_then(toml::Value::as_str)
            {
                push(Some(model));
            }
        }
        AppType::Gemini => {
            let env = settings_config.get("env").and_then(Value::as_object);
            push(
                env.and_then(|env| env.get("GEMINI_MODEL"))
                    .and_then(Value::as_str),
            );
        }
        AppType::GrokBuild => {
            // 真实存储形状：config 是 TOML 文本（~/.grok/config.toml 的快照），
            // [models].default 优先；其余 [model."<name>"] 键随后补齐。
            let document = settings_config
                .get("config")
                .and_then(Value::as_str)
                .and_then(|config_toml| config_toml.parse::<toml::Value>().ok());
            if let Some(document) = document {
                if let Some(default) = document
                    .get("models")
                    .and_then(|models| models.get("default"))
                    .and_then(toml::Value::as_str)
                {
                    push(Some(default));
                }
                if let Some(entries) = document.get("model").and_then(toml::Value::as_table) {
                    for name in entries.keys() {
                        push(Some(name));
                    }
                }
            }
        }
        AppType::Hermes | AppType::OpenClaw | AppType::Pi => {
            if let Some(entries) = settings_config.get("models").and_then(Value::as_array) {
                for entry in entries {
                    push(entry.get("id").and_then(Value::as_str));
                }
            }
        }
        AppType::OpenCode => {
            if let Some(entries) = settings_config.get("models").and_then(Value::as_object) {
                for name in entries.keys() {
                    push(Some(name));
                }
            }
        }
    }

    if models.len() > SOURCE_MODEL_LIMIT {
        models.truncate(SOURCE_MODEL_LIMIT);
    }
    models
}

/// OpenClaw live config 的 api 枚举（openclawProviderPresets.ts:91-94，Pi 同款）。
/// 两个目标都原生支持 Anthropic / Responses / Gemini，按源协议映射而非降级；
/// 未知格式回退 openai-completions。
fn derive_openclaw_api(api_format: &str) -> &'static str {
    match api_format {
        "anthropic" => "anthropic-messages",
        "openai_responses" => "openai-responses",
        "gemini_native" => "google-generative-ai",
        _ => "openai-completions",
    }
}

/// Hermes api_mode 枚举（hermesProviderPresets.ts:87-95）。
fn derive_hermes_api_mode(api_format: &str) -> &'static str {
    match api_format {
        "anthropic" => "anthropic_messages",
        "openai_responses" => "codex_responses",
        _ => "chat_completions",
    }
}

/// Pi 目标：直连非代理，api 走 openclaw 同款语义。
fn derive_pi_api(api_format: &str) -> &'static str {
    derive_openclaw_api(api_format)
}

/// openai_chat 等格式复制到 Claude / Codex 时不能丢，否则发错协议格式。
fn derive_source_api_format(source: &Provider, source_app: &AppType) -> String {
    // Claude 源整体走 get_claude_api_format 判定链：codex_oauth/xai_oauth 的
    // 协议不变量（priority 0）优先于 meta.api_format，随后才是
    // settings_config.api_format 与 openrouter_compat_mode；漏掉任何一级都会
    // 把正常工作的供应商误标协议。
    if matches!(source_app, AppType::Claude | AppType::ClaudeDesktop) {
        return crate::proxy::providers::get_claude_api_format(source).to_string();
    }
    if let Some(explicit) = source
        .meta
        .as_ref()
        .and_then(|meta| meta.api_format.as_deref())
        .filter(|value| !value.trim().is_empty())
    {
        return explicit.to_string();
    }

    match source_app {
        // 通常不可达（函数开头已提前返回），保留以保证 AppType 穷尽匹配。
        AppType::Claude | AppType::ClaudeDesktop => {
            crate::proxy::providers::get_claude_api_format(source).to_string()
        }
        AppType::Codex => {
            // 旧协议声明（settings 级 api_format/apiFormat）优先于 TOML wire_api，
            // 优先级链与 codex_provider_uses_anthropic（codex.rs:168-196）一致；
            // 两个枚举之外的值继续落 TOML 判定。
            let declared = source
                .settings_config
                .get("api_format")
                .or_else(|| source.settings_config.get("apiFormat"))
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty());
            if let Some(declared) = declared {
                if crate::proxy::providers::codex::is_anthropic_wire_api(declared) {
                    return "anthropic".to_string();
                }
                if crate::proxy::providers::codex::is_chat_wire_api(declared) {
                    return "openai_chat".to_string();
                }
            }
            // wire_api 是唯一的显式协议声明（与 codex.rs 的源判定同源）：
            // anthropic 系 → Anthropic，chat 系 → Chat，其余/缺省 → Responses。
            let wire_api = source
                .settings_config
                .get("config")
                .and_then(Value::as_str)
                .and_then(crate::proxy::providers::codex::extract_codex_wire_api_from_toml);
            match wire_api.as_deref() {
                Some(wire) if crate::proxy::providers::codex::is_anthropic_wire_api(wire) => {
                    "anthropic".to_string()
                }
                Some(wire) if crate::proxy::providers::codex::is_chat_wire_api(wire) => {
                    "openai_chat".to_string()
                }
                _ => "openai_responses".to_string(),
            }
        }
        AppType::Gemini => "gemini_native".to_string(),
        AppType::OpenClaw | AppType::Pi => {
            let api = source
                .settings_config
                .get("api")
                .and_then(Value::as_str)
                .unwrap_or_default();
            match api {
                "openai-responses" => "openai_responses".to_string(),
                "anthropic-messages" => "anthropic".to_string(),
                "google-generative-ai" => "gemini_native".to_string(),
                _ => "openai_chat".to_string(),
            }
        }
        AppType::Hermes => {
            // api_mode 枚举见 hermesProviderPresets.ts:74-77：Responses 实存
            // 为 codex_responses（"responses" 不是合法存储值）；
            // bedrock_converse 无对应目标协议，按 Chat best-effort。
            let api_mode = source
                .settings_config
                .get("api_mode")
                .and_then(Value::as_str)
                .unwrap_or_default();
            match api_mode {
                "codex_responses" => "openai_responses".to_string(),
                "anthropic_messages" => "anthropic".to_string(),
                _ => "openai_chat".to_string(),
            }
        }
        AppType::OpenCode => {
            // npm → 协议见 opencodeProviderPresets.ts:22-28：
            // @ai-sdk/openai = Responses（AI SDK v5 默认 Responses API），
            // @ai-sdk/anthropic = Anthropic，@ai-sdk/google = Gemini；
            // openai-compatible 与未知包（含 amazon-bedrock）按 Chat best-effort。
            let npm = source
                .settings_config
                .get("npm")
                .and_then(Value::as_str)
                .map(str::trim)
                .unwrap_or_default();
            match npm {
                "@ai-sdk/openai" => "openai_responses".to_string(),
                "@ai-sdk/anthropic" => "anthropic".to_string(),
                "@ai-sdk/google" => "gemini_native".to_string(),
                _ => "openai_chat".to_string(),
            }
        }
        // xAI API 为 OpenAI 兼容；best-effort，复制后可在目标卡调整。
        AppType::GrokBuild => "openai_chat".to_string(),
    }
}

/// 复制显示字段（name/notes/icon/icon_color/website_url/category/created_at/
/// in_failover_queue）；目标 ID 与凭据字段由调用方设置，sort_index 留空走默认追加。
fn apply_copy_metadata(target: &mut Provider, source: &Provider, created_at: i64) {
    target.name = source.name.clone();
    target.notes = source.notes.clone();
    target.icon = source.icon.clone();
    target.icon_color = source.icon_color.clone();
    target.website_url = source.website_url.clone();
    target.category = Some("custom".to_string());
    target.created_at = Some(created_at);
    target.in_failover_queue = false;
}

/// OpenCode 的 @ai-sdk/anthropic 以 baseURL + "/messages" 发请求（vercel/ai
/// packages/anthropic/src/anthropic-language-model.ts），而 Anthropic 系源
/// （Claude Code、OpenClaw/Pi/Hermes anthropic 预设、codex wire_api=anthropic）
/// 的 base 语义是追加 "/v1/messages"——Kimi 即 /coding（Claude 预设）vs
/// /coding/v1（OpenCode 预设）。缺 /v1 的 base 补齐 /v1（与代理层 adapter 的
/// /v1/v1 去重惯例一致）；is_full_url 源是完整端点（…/v1/messages），剥掉尾部
/// /messages 让 AI SDK 重新拼回同一请求 URL。
fn opencode_anthropic_base_url(base_url: &str, is_full_url: bool) -> String {
    let trimmed = base_url.trim_end_matches('/');
    if is_full_url {
        return trimmed
            .strip_suffix("/messages")
            .unwrap_or(trimmed)
            .to_string();
    }
    if trimmed.ends_with("/v1") {
        trimmed.to_string()
    } else {
        format!("{trimmed}/v1")
    }
}

/// OpenCode 的 @ai-sdk/openai 以 baseURL + "/responses" 发请求，而 Codex 源
/// （openai_responses）的 base 语义与 Codex 适配器一致（proxy/providers/
/// codex.rs:1056-1077）：origin 补 /v1，已带 /v1 或自定义前缀保持原样。
/// is_full_url 源是完整端点（…/responses），剥掉尾部 /responses 让 AI SDK
/// 重新拼回同一请求 URL，否则会得到 …/responses/responses。
fn opencode_responses_base_url(base_url: &str, is_full_url: bool) -> String {
    let trimmed = base_url.trim_end_matches('/');
    if is_full_url {
        return trimmed
            .strip_suffix("/responses")
            .unwrap_or(trimmed)
            .to_string();
    }
    if trimmed.ends_with("/v1") {
        trimmed.to_string()
    } else {
        format!("{trimmed}/v1")
    }
}

/// 按目标应用的原生配置形状重建 settings_config（8 个 builder 派发，
/// ClaudeDesktop 走 resolve_claude_desktop_copy 的整份 clone 路径）。
/// api_format 由源应用推导（derive_source_api_format），仅进入副本 meta
/// 供本地代理层转换；写入目标的 wire_api / api 枚举不依赖它。
#[allow(clippy::too_many_arguments)]
fn build_target_settings(
    target: AppType,
    source_app: &AppType,
    base_url: &str,
    api_key: &str,
    source_name: &str,
    models: &[String],
    source: &Provider,
) -> Result<Value, AppError> {
    let api_format = derive_source_api_format(source, source_app);
    let base_url_value = json!(base_url);
    let api_key_value = json!(api_key);
    let settings = match target {
        AppType::Claude => {
            let mut env = Map::new();
            env.insert("ANTHROPIC_BASE_URL".to_string(), base_url_value);
            let auth_key = if source.claude_uses_api_key_field() {
                "ANTHROPIC_API_KEY"
            } else {
                "ANTHROPIC_AUTH_TOKEN"
            };
            env.insert(auth_key.to_string(), api_key_value);
            if let Some(model) = models.first() {
                env.insert("ANTHROPIC_MODEL".to_string(), json!(model));
            }
            json!({ "env": env })
        }
        AppType::Codex => {
            let provider_display_name = if source_name.trim().is_empty() {
                "custom".to_string()
            } else {
                source_name.trim().to_string()
            };
            let model_name = models.first().map(String::as_str).unwrap_or("gpt-5-codex");
            let endpoint = base_url.trim().trim_end_matches('/');
            let provider_display_name =
                toml_edit::Value::from(provider_display_name.as_str()).to_string();
            let model_name = toml_edit::Value::from(model_name).to_string();
            let endpoint = toml_edit::Value::from(endpoint).to_string();
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
            crate::codex_config::validate_config_toml(&config_toml)?;
            json!({
                "auth": { "OPENAI_API_KEY": api_key_value },
                "config": config_toml,
            })
        }
        AppType::Gemini => {
            let mut env = Map::new();
            env.insert("GEMINI_API_KEY".to_string(), api_key_value);
            env.insert("GOOGLE_GEMINI_BASE_URL".to_string(), base_url_value);
            if let Some(model) = models.first() {
                env.insert("GEMINI_MODEL".to_string(), json!(model));
            }
            json!({ "env": env })
        }
        AppType::GrokBuild => {
            let model_value = models
                .first()
                .map(String::as_str)
                .unwrap_or(crate::grok_config::DEFAULT_MODEL);
            let model_value = toml_edit::Value::from(model_value).to_string();
            let name_value = toml_edit::Value::from(source_name).to_string();
            let endpoint_value = toml_edit::Value::from(base_url).to_string();
            let api_key_value = toml_edit::Value::from(api_key).to_string();
            json!({
                "config": format!(
                    "[models]\ndefault = {model_value}\n\n[model.{model_value}]\nmodel = {model_value}\nbase_url = {endpoint_value}\nname = {name_value}\napi_key = {api_key_value}\napi_backend = \"{}\"\ncontext_window = {}\n",
                    crate::grok_config::DEFAULT_API_BACKEND,
                    crate::grok_config::DEFAULT_CONTEXT_WINDOW,
                )
            })
        }
        AppType::OpenCode => {
            // npm 决定 OpenCode 实际发出的协议（opencodeProviderPresets.ts:22-28），
            // 必须跟源协议一致：anthropic 源配 openai-compatible 会发错 Chat 请求。
            let npm = match api_format.as_str() {
                "anthropic" => "@ai-sdk/anthropic",
                "openai_responses" => "@ai-sdk/openai",
                "gemini_native" => "@ai-sdk/google",
                _ => "@ai-sdk/openai-compatible",
            };
            let base_url_value = if api_format == "anthropic" {
                json!(opencode_anthropic_base_url(
                    base_url,
                    source.meta.as_ref().and_then(|meta| meta.is_full_url) == Some(true)
                ))
            } else if api_format == "openai_responses" {
                json!(opencode_responses_base_url(
                    base_url,
                    source.meta.as_ref().and_then(|meta| meta.is_full_url) == Some(true)
                ))
            } else {
                base_url_value
            };
            let mut options = Map::new();
            options.insert("baseURL".to_string(), base_url_value);
            options.insert("apiKey".to_string(), api_key_value);
            let mut model_map = Map::new();
            for model in models {
                model_map.insert(model.clone(), json!({ "name": model }));
            }
            json!({
                "npm": npm,
                "name": source_name,
                "options": options,
                "models": model_map,
            })
        }
        AppType::OpenClaw => {
            let mut config = Map::new();
            config.insert("baseUrl".to_string(), base_url_value);
            config.insert("apiKey".to_string(), api_key_value);
            config.insert("api".to_string(), json!(derive_openclaw_api(&api_format)));
            let models = models
                .iter()
                .map(|model| json!({ "id": model, "name": model }))
                .collect::<Vec<_>>();
            config.insert("models".to_string(), Value::Array(models));
            Value::Object(config)
        }
        AppType::Hermes => {
            let mut config = Map::new();
            if !source_name.trim().is_empty() {
                config.insert("name".to_string(), json!(source_name));
            }
            config.insert("base_url".to_string(), base_url_value);
            config.insert("api_key".to_string(), api_key_value);
            config.insert(
                "api_mode".to_string(),
                json!(derive_hermes_api_mode(&api_format)),
            );
            let models = models
                .iter()
                .map(|model| json!({ "id": model, "name": model }))
                .collect::<Vec<_>>();
            config.insert("models".to_string(), Value::Array(models));
            Value::Object(config)
        }
        AppType::Pi => {
            let mut config = Map::new();
            config.insert("name".to_string(), json!(source_name));
            config.insert("baseUrl".to_string(), base_url_value);
            config.insert("api".to_string(), json!(derive_pi_api(&api_format)));
            config.insert("apiKey".to_string(), api_key_value);
            let models = models
                .iter()
                .map(|model| json!({ "id": model }))
                .collect::<Vec<_>>();
            config.insert("models".to_string(), Value::Array(models));
            Value::Object(config)
        }
        // ClaudeDesktop 目标在外层走整克隆分支，不会进入此 builder。
        AppType::ClaudeDesktop => {
            return Err(AppError::InvalidInput(
                "Claude Desktop copy must clone the source settings, not rebuild them".to_string(),
            ))
        }
    };

    Ok(settings)
}

/// 副本 meta：api_format（协议信息，供本地代理层转换）+ Anthropic 源的
/// 认证字段选择（Codex 代理适配器按 meta.apiKeyField 决定 x-api-key 还是
/// Bearer，见 proxy/providers/codex.rs extract_auth）+ is_full_url（仅
/// Claude/Codex 目标——forwarder 对这两个目标的 codex/anthropic 请求按完整
/// 端点处理，丢掉标记会把 …/v1/responses 再拼一层 /responses）。其余源 meta 字段（usage_script、custom_endpoints、
/// claude_desktop_*、provider_type 等）语义绑定源应用，带不过去；显示
/// 字段由 apply_copy_metadata 复制。
fn build_copy_meta(source: &Provider, source_app: &AppType, target: AppType) -> ProviderMeta {
    let api_format = derive_source_api_format(source, source_app);
    let mut meta = ProviderMeta {
        api_format: Some(api_format.clone()),
        ..ProviderMeta::default()
    };
    if matches!(target, AppType::Claude | AppType::Codex) {
        meta.is_full_url = source.meta.as_ref().and_then(|meta| meta.is_full_url);
    }
    // 表单只为非默认选择（ANTHROPIC_API_KEY）持久化 apiKeyField；None 保持
    // 默认 AUTH_TOKEN 语义。仅 Anthropic 协议副本需要它（x-api-key vs Bearer）。
    if api_format == "anthropic" && source.claude_uses_api_key_field() {
        meta.api_key_field = Some("ANTHROPIC_API_KEY".to_string());
    }
    meta
}

/// P1：复刻 import_claude_desktop_providers_from_claude 的逐供应商决策——
/// 兼容 Direct 且模型全部 Claude-safe → Direct（routes 留空）；
/// 否则能生成建议路由 → Proxy + routes；否则 None（调用方 skip）。
/// 非 Claude 形状的源没有 env，必然落 None。
fn resolve_claude_desktop_copy(
    source: &Provider,
) -> Option<(ClaudeDesktopMode, HashMap<String, ClaudeDesktopModelRoute>)> {
    if crate::claude_desktop_config::is_compatible_direct_provider(source)
        && claude_provider_models_are_claude_safe(source)
    {
        return Some((ClaudeDesktopMode::Direct, HashMap::new()));
    }
    let routes = suggested_claude_desktop_routes(source)?;
    Some((ClaudeDesktopMode::Proxy, routes))
}

fn skipped(target_app: String, key: &str, fallback: &str) -> CopyTargetOutcome {
    CopyTargetOutcome {
        target_app,
        status: CopyStatus::Skipped,
        new_provider_id: None,
        reason: Some(CopyFailureReason::new(key, fallback.to_string())),
    }
}

fn failed(target_app: String, fallback: impl Into<String>) -> CopyTargetOutcome {
    // 具体错误同时放进 params：四语 saveFailed 键以 {{error}} 插值展示真实
    // 原因；fallback 保留原文，供无翻译环境兜底。
    let fallback = fallback.into();
    CopyTargetOutcome {
        target_app,
        status: CopyStatus::Failed,
        new_provider_id: None,
        reason: Some(
            CopyFailureReason::new("saveFailed", fallback.clone()).with_param("error", fallback),
        ),
    }
}

fn copied(target_app: String, provider_id: String) -> CopyTargetOutcome {
    CopyTargetOutcome {
        target_app,
        status: CopyStatus::Copied,
        new_provider_id: Some(provider_id),
        reason: None,
    }
}

/// 已存在预检失败时的统一跳过结果（带 name 参数供 i18n 插值）。
fn skipped_already_exists(target_app: String, provider_name: &str) -> CopyTargetOutcome {
    CopyTargetOutcome {
        target_app,
        status: CopyStatus::Skipped,
        new_provider_id: None,
        reason: Some(
            CopyFailureReason::new(
                "alreadyExists",
                format!(
                    "Target app already has a provider with the same ID '{provider_name}', skipped"
                ),
            )
            .with_param("name", provider_name),
        ),
    }
}

/// 逐目标复制。预检与分支派发按请求顺序执行，任何单目标失败不中断其余目标。
#[allow(clippy::too_many_arguments)]
fn copy_to_single_target(
    state: &AppState,
    source: &Provider,
    source_app: &AppType,
    target_raw: &str,
    base_url: &str,
    api_key: &str,
    models: &[String],
    now: i64,
) -> CopyTargetOutcome {
    let target = match AppType::from_str(target_raw) {
        Ok(app) => app,
        Err(_) => {
            return CopyTargetOutcome {
                target_app: target_raw.to_string(),
                status: CopyStatus::Failed,
                new_provider_id: None,
                reason: Some(CopyFailureReason::new(
                    "unsupportedTarget",
                    format!("Unsupported target app: '{target_raw}'"),
                )),
            };
        }
    };
    let target_app = target.as_str().to_string();

    if &target == source_app {
        return skipped(
            target_app,
            "sameAsSource",
            "Target app is the same as the source app",
        );
    }

    // 已存在预检先于分支派发（所有目标统一）：save_provider 是 upsert，
    // 漏检会静默覆盖目标应用的同 ID 供应商。重试由此幂等。
    match state.db.get_provider_by_id(&source.id, target.as_str()) {
        Ok(Some(_)) => return skipped_already_exists(target_app, &source.name),
        Ok(None) => {}
        Err(error) => return failed(target_app, error.to_string()),
    }
    if target == AppType::Pi {
        // native models.json 含内置键（'anthropic'/'openai'）且 DB 未落行时
        // pi::add 会报错；预检把该路径转成 skipped/alreadyExists。
        match crate::pi_config::pi_provider_exists(&source.id) {
            Ok(true) => return skipped_already_exists(target_app, &source.name),
            Ok(false) => {}
            Err(error) => return failed(target_app, error.to_string()),
        }
    }

    if target == AppType::ClaudeDesktop {
        // P1：任意源统一走导入决策路径（整份 clone settings_config + meta），
        // 不经 builder 重建；非 Claude 源没有 env 形状，必然 skip。
        let Some((mode, routes)) = resolve_claude_desktop_copy(source) else {
            return skipped(
                target_app,
                "incompatibleClaudeDesktop",
                "This provider cannot be converted to a Claude Desktop configuration",
            );
        };
        let mut desktop_provider = source.clone();
        desktop_provider.in_failover_queue = false;
        let meta = desktop_provider
            .meta
            .get_or_insert_with(ProviderMeta::default);
        meta.claude_desktop_mode = Some(mode);
        meta.claude_desktop_model_routes = routes;
        // 裸 save（与导入命令一致）：无校验副作用，不触发 ensure_official_seed。
        match state.db.save_provider(target.as_str(), &desktop_provider) {
            Ok(()) => copied(target_app, source.id.clone()),
            Err(error) => failed(target_app, error.to_string()),
        }
    } else {
        let meta = Some(build_copy_meta(source, source_app, target.clone()));
        let settings = match build_target_settings(
            target.clone(),
            source_app,
            base_url,
            api_key,
            &source.name,
            models,
            source,
        ) {
            Ok(settings) => settings,
            Err(error) => return failed(target_app, error.to_string()),
        };
        let mut provider = Provider {
            id: source.id.clone(),
            name: String::new(),
            settings_config: settings,
            website_url: None,
            category: None,
            created_at: None,
            sort_index: None,
            notes: None,
            meta,
            icon: None,
            icon_color: None,
            in_failover_queue: false,
        };
        apply_copy_metadata(&mut provider, source, now);
        // add_to_live=false：additive 目标仅落库（liveConfigManaged=false）；
        // 切换式目标由 add 既有语义处理（目标为空时副本成为 current 并写 live）。
        match ProviderService::add(state, target.clone(), provider, false) {
            Ok(_) => copied(target_app, source.id.clone()),
            Err(error) => {
                // add 先落库（空目标还会先置 current）再写 live；live 写入失败时
                // 把本次落下的行回滚掉，否则 alreadyExists 预检会让重试永远跳过，
                // 目标卡在无法通过复制修复的半持久化状态。预检保证同 ID 行只可能
                // 来自本次调用，删行同时清除误置的 current。
                let _ = state.db.delete_provider(target.as_str(), &source.id);
                failed(target_app, error.to_string())
            }
        }
    }
}

/// 编排器：拉源 → 源门禁 → 提取中间凭据与模型名 → 按请求顺序逐目标。
/// 源级拒绝（官方 / 代理注入 OAuth / 无法提取凭据）整体报错，无逐目标结果。
pub fn copy_provider_to_apps_internal(
    state: &AppState,
    source_app: AppType,
    provider_id: &str,
    target_apps: &[String],
) -> Result<Vec<CopyTargetOutcome>, AppError> {
    let source = state
        .db
        .get_provider_by_id(provider_id, source_app.as_str())?
        .ok_or_else(|| {
            AppError::localized(
                "provider.notFound",
                format!("供应商 '{provider_id}' 不存在"),
                format!("Provider '{provider_id}' not found"),
            )
        })?;

    if let Some(rejection) = source_rejection(&source, &source_app) {
        return Err(rejection);
    }

    let (base_url, api_key) = source.resolve_usage_credentials(&source_app);
    let models =
        extract_source_model_names(&source_app, &source.settings_config, source.meta.as_ref());
    let now = chrono::Utc::now().timestamp();

    let outcomes = target_apps
        .iter()
        .map(|target_raw| {
            copy_to_single_target(
                state,
                &source,
                &source_app,
                target_raw,
                &base_url,
                &api_key,
                &models,
                now,
            )
        })
        .collect();
    Ok(outcomes)
}

/// 供集成测试绕过 Tauri 运行时直接调编排器。
#[cfg_attr(not(feature = "test-hooks"), doc(hidden))]
pub fn copy_provider_to_apps_test_hook(
    state: &AppState,
    source_app: AppType,
    provider_id: &str,
    target_apps: &[String],
) -> Result<Vec<CopyTargetOutcome>, AppError> {
    copy_provider_to_apps_internal(state, source_app, provider_id, target_apps)
}

/// 跨应用复制供应商（Rust 参数 snake_case，Tauri v2 自动匹配前端 camelCase 调用键）。
#[tauri::command]
pub async fn copy_provider_to_apps(
    app_handle: tauri::AppHandle,
    source_app: String,
    provider_id: String,
    target_apps: Vec<String>,
) -> Result<Vec<CopyTargetOutcome>, String> {
    let source_app = AppType::from_str(&source_app).map_err(|e| e.to_string())?;
    tauri::async_runtime::spawn_blocking(move || {
        let state = app_handle
            .try_state::<AppState>()
            .ok_or_else(|| "应用状态不可用".to_string())?;
        copy_provider_to_apps_internal(state.inner(), source_app, &provider_id, &target_apps)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("跨应用复制任务执行失败: {e}"))?
}

#[cfg(test)]
mod copy_to_apps_tests {
    use super::*;

    fn provider_with_settings(settings: Value) -> Provider {
        Provider::with_id("copy-src".to_string(), "Relay".to_string(), settings, None)
    }

    fn claude_source() -> Provider {
        provider_with_settings(json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://relay.example.com",
                "ANTHROPIC_AUTH_TOKEN": "sk-relay",
                "ANTHROPIC_MODEL": "claude-sonnet-4-5"
            }
        }))
    }

    fn env_of(settings: &Value) -> &Map<String, Value> {
        settings.get("env").and_then(Value::as_object).expect("env")
    }

    #[test]
    fn claude_target_uses_default_auth_token_and_first_model_only() {
        let source = claude_source();
        let settings = build_target_settings(
            AppType::Claude,
            &AppType::Claude,
            "https://relay.example.com",
            "sk-relay",
            &source.name,
            &["claude-sonnet-4-5".to_string()],
            &source,
        )
        .expect("build claude target");

        let env = env_of(&settings);
        assert_eq!(
            env.get("ANTHROPIC_BASE_URL").and_then(Value::as_str),
            Some("https://relay.example.com")
        );
        assert_eq!(
            env.get("ANTHROPIC_AUTH_TOKEN").and_then(Value::as_str),
            Some("sk-relay")
        );
        assert_eq!(
            env.get("ANTHROPIC_MODEL").and_then(Value::as_str),
            Some("claude-sonnet-4-5")
        );
        assert!(!env.contains_key("ANTHROPIC_API_KEY"));
        assert!(!env.contains_key("ANTHROPIC_DEFAULT_SONNET_MODEL"));
    }

    #[test]
    fn claude_target_honors_api_key_field_source() {
        let mut source = claude_source();
        source
            .meta
            .get_or_insert_with(ProviderMeta::default)
            .api_key_field = Some("ANTHROPIC_API_KEY".to_string());

        let settings = build_target_settings(
            AppType::Claude,
            &AppType::Claude,
            "https://relay.example.com",
            "sk-relay",
            &source.name,
            &[],
            &source,
        )
        .expect("build claude target");

        let env = env_of(&settings);
        assert_eq!(
            env.get("ANTHROPIC_API_KEY").and_then(Value::as_str),
            Some("sk-relay")
        );
        assert!(!env.contains_key("ANTHROPIC_AUTH_TOKEN"));
        assert!(!env.contains_key("ANTHROPIC_MODEL"));
    }

    #[test]
    fn codex_target_toml_passes_validation_and_pins_wire_api() {
        let source = claude_source();
        let settings = build_target_settings(
            AppType::Codex,
            &AppType::Claude,
            "https://relay.example.com/v1/",
            "sk-relay",
            &source.name,
            &["gpt-5-codex".to_string()],
            &source,
        )
        .expect("build codex target");

        let config = settings
            .get("config")
            .and_then(Value::as_str)
            .expect("config toml");
        crate::codex_config::validate_config_toml(config).expect("passes codex validation");
        assert!(config.contains("wire_api = \"responses\""));
        assert!(config.contains("model = \"gpt-5-codex\""));
        assert!(config.contains("name = \"Relay\""));
        assert!(config.contains("base_url = \"https://relay.example.com/v1\""));
        assert_eq!(
            settings
                .pointer("/auth/OPENAI_API_KEY")
                .and_then(Value::as_str),
            Some("sk-relay")
        );
    }

    #[test]
    fn codex_target_falls_back_when_source_name_empty() {
        let source = claude_source();
        let settings = build_target_settings(
            AppType::Codex,
            &AppType::Claude,
            "https://relay.example.com",
            "sk-relay",
            "  ",
            &[],
            &source,
        )
        .expect("build codex target");

        let config = settings
            .get("config")
            .and_then(Value::as_str)
            .expect("config toml");
        assert!(config.contains("name = \"custom\""));
        assert!(config.contains("model = \"gpt-5-codex\""));
    }

    #[test]
    fn gemini_target_shape() {
        let source = claude_source();
        let settings = build_target_settings(
            AppType::Gemini,
            &AppType::Claude,
            "https://relay.example.com",
            "sk-relay",
            &source.name,
            &["gemini-2.5-pro".to_string()],
            &source,
        )
        .expect("build gemini target");

        let env = env_of(&settings);
        assert_eq!(
            env.get("GEMINI_API_KEY").and_then(Value::as_str),
            Some("sk-relay")
        );
        assert_eq!(
            env.get("GOOGLE_GEMINI_BASE_URL").and_then(Value::as_str),
            Some("https://relay.example.com")
        );
        assert_eq!(
            env.get("GEMINI_MODEL").and_then(Value::as_str),
            Some("gemini-2.5-pro")
        );
    }

    #[test]
    fn grokbuild_target_round_trips_through_grok_config() {
        let source = claude_source();
        let settings = build_target_settings(
            AppType::GrokBuild,
            &AppType::Claude,
            "https://relay.example.com",
            "sk-relay",
            &source.name,
            &["grok-4".to_string()],
            &source,
        )
        .expect("build grokbuild target");

        let config = settings
            .get("config")
            .and_then(Value::as_str)
            .expect("config toml");
        let model_config =
            crate::grok_config::extract_model_config(config).expect("parses model config");
        assert_eq!(model_config.model, "grok-4");
        assert_eq!(model_config.base_url, "https://relay.example.com");
        assert_eq!(model_config.name, "Relay");
        assert_eq!(model_config.api_key.as_deref(), Some("sk-relay"));
        assert_eq!(
            model_config.api_backend,
            crate::grok_config::DEFAULT_API_BACKEND
        );
        assert_eq!(
            model_config.context_window,
            crate::grok_config::DEFAULT_CONTEXT_WINDOW
        );
        assert_eq!(
            crate::grok_config::extract_credentials(config),
            Some((
                "https://relay.example.com".to_string(),
                "sk-relay".to_string()
            ))
        );
    }

    #[test]
    fn additive_targets_derive_api_from_source_protocol() {
        let anthropic_source = provider_with_settings(json!({
            "baseUrl": "https://relay.example.com",
            "apiKey": "sk-relay",
            "api": "anthropic-messages",
            "models": [{ "id": "m1" }]
        }));

        let openclaw = build_target_settings(
            AppType::OpenClaw,
            &AppType::OpenClaw,
            "https://relay.example.com",
            "sk-relay",
            "Relay",
            &["m1".to_string()],
            &anthropic_source,
        )
        .expect("build openclaw target");
        assert_eq!(
            openclaw.get("api").and_then(Value::as_str),
            Some("anthropic-messages")
        );
        assert_eq!(
            openclaw.get("baseUrl").and_then(Value::as_str),
            Some("https://relay.example.com")
        );

        let hermes = build_target_settings(
            AppType::Hermes,
            &AppType::OpenClaw,
            "https://relay.example.com",
            "sk-relay",
            "Relay",
            &["m1".to_string()],
            &anthropic_source,
        )
        .expect("build hermes target");
        assert_eq!(
            hermes.get("api_mode").and_then(Value::as_str),
            Some("anthropic_messages")
        );

        let pi = build_target_settings(
            AppType::Pi,
            &AppType::OpenClaw,
            "https://relay.example.com",
            "sk-relay",
            "Relay",
            &["m1".to_string()],
            &anthropic_source,
        )
        .expect("build pi target");
        assert_eq!(
            pi.get("api").and_then(Value::as_str),
            Some("anthropic-messages")
        );
        assert!(pi.get("models").and_then(Value::as_array).is_some());

        let responses_source = provider_with_settings(json!({
            "base_url": "https://relay.example.com",
            "api_key": "sk-relay",
            "api_mode": "codex_responses",
            "models": [{ "id": "m1" }]
        }));
        let hermes_responses = build_target_settings(
            AppType::Hermes,
            &AppType::Hermes,
            "https://relay.example.com",
            "sk-relay",
            "Relay",
            &[],
            &responses_source,
        )
        .expect("build hermes target from responses source");
        assert_eq!(
            hermes_responses.get("api_mode").and_then(Value::as_str),
            Some("codex_responses")
        );

        // OpenClaw / Pi 原生支持 Responses 与 Gemini，不得降级为 Chat。
        let responses_openclaw = build_target_settings(
            AppType::OpenClaw,
            &AppType::Hermes,
            "https://relay.example.com",
            "sk-relay",
            "Relay",
            &[],
            &responses_source,
        )
        .expect("build openclaw target from responses source");
        assert_eq!(
            responses_openclaw.get("api").and_then(Value::as_str),
            Some("openai-responses")
        );
        let pi_responses = build_target_settings(
            AppType::Pi,
            &AppType::Hermes,
            "https://relay.example.com",
            "sk-relay",
            "Relay",
            &[],
            &responses_source,
        )
        .expect("build pi target from responses source");
        assert_eq!(
            pi_responses.get("api").and_then(Value::as_str),
            Some("openai-responses")
        );

        let gemini_source = provider_with_settings(json!({
            "baseUrl": "https://relay.example.com",
            "apiKey": "sk-relay",
            "api": "google-generative-ai",
            "models": [{ "id": "m1" }]
        }));
        let openclaw_gemini = build_target_settings(
            AppType::OpenClaw,
            &AppType::OpenClaw,
            "https://relay.example.com",
            "sk-relay",
            "Relay",
            &[],
            &gemini_source,
        )
        .expect("build openclaw target from gemini source");
        assert_eq!(
            openclaw_gemini.get("api").and_then(Value::as_str),
            Some("google-generative-ai")
        );
        let pi_gemini = build_target_settings(
            AppType::Pi,
            &AppType::OpenClaw,
            "https://relay.example.com",
            "sk-relay",
            "Relay",
            &[],
            &gemini_source,
        )
        .expect("build pi target from gemini source");
        assert_eq!(
            pi_gemini.get("api").and_then(Value::as_str),
            Some("google-generative-ai")
        );
    }

    #[test]
    fn opencode_target_shape() {
        let source = claude_source();
        let settings = build_target_settings(
            AppType::OpenCode,
            &AppType::Claude,
            "https://relay.example.com",
            "sk-relay",
            &source.name,
            &["m1".to_string(), "m2".to_string()],
            &source,
        )
        .expect("build opencode target");

        assert_eq!(
            settings.get("npm").and_then(Value::as_str),
            Some("@ai-sdk/anthropic")
        );
        // AI SDK anthropic 只在 baseURL 后追加 /messages，Claude 系源的
        // base 语义是追加 /v1/messages——缺 /v1 时必须补齐。
        assert_eq!(
            settings.pointer("/options/baseURL").and_then(Value::as_str),
            Some("https://relay.example.com/v1")
        );
        assert_eq!(
            settings.pointer("/options/apiKey").and_then(Value::as_str),
            Some("sk-relay")
        );
        let models = settings
            .get("models")
            .and_then(Value::as_object)
            .expect("models map");
        assert!(models.contains_key("m1"));
        assert!(models.contains_key("m2"));

        // Kimi 形状（Claude 预设 /coding）→ /coding/v1（对齐 OpenCode 官方预设）。
        let kimi_settings = build_target_settings(
            AppType::OpenCode,
            &AppType::Claude,
            "https://api.kimi.com/coding",
            "sk-relay",
            "Relay",
            &["m1".to_string()],
            &source,
        )
        .expect("build opencode target from kimi base");
        assert_eq!(
            kimi_settings
                .pointer("/options/baseURL")
                .and_then(Value::as_str),
            Some("https://api.kimi.com/coding/v1")
        );

        // 已含 /v1 的 base 不重复追加。
        let v1_settings = build_target_settings(
            AppType::OpenCode,
            &AppType::Claude,
            "https://relay.example.com/v1",
            "sk-relay",
            "Relay",
            &["m1".to_string()],
            &source,
        )
        .expect("build opencode target from v1 base");
        assert_eq!(
            v1_settings
                .pointer("/options/baseURL")
                .and_then(Value::as_str),
            Some("https://relay.example.com/v1")
        );

        // is_full_url 源是完整端点（…/v1/messages）：剥掉尾部 /messages，
        // AI SDK 重新追加 /messages 后请求 URL 与源端一致。
        let mut full_url_source = claude_source();
        full_url_source
            .meta
            .get_or_insert_with(ProviderMeta::default)
            .is_full_url = Some(true);
        let full_url_settings = build_target_settings(
            AppType::OpenCode,
            &AppType::Claude,
            "https://gateway.example.com/v1/messages",
            "sk-relay",
            "Relay",
            &["m1".to_string()],
            &full_url_source,
        )
        .expect("build opencode target from full url");
        assert_eq!(
            full_url_settings
                .pointer("/options/baseURL")
                .and_then(Value::as_str),
            Some("https://gateway.example.com/v1")
        );

        // npm 跟随源协议：Codex（Responses）→ @ai-sdk/openai，
        // OpenClaw Chat → @ai-sdk/openai-compatible。
        let responses_source = provider_with_settings(json!({
            "auth": { "OPENAI_API_KEY": "k" },
            "config": "model = \"gpt-5-codex\"\n"
        }));
        let responses_settings = build_target_settings(
            AppType::OpenCode,
            &AppType::Codex,
            "https://relay.example.com",
            "sk-relay",
            "Relay",
            &["m1".to_string()],
            &responses_source,
        )
        .expect("build opencode target from responses source");
        assert_eq!(
            responses_settings.get("npm").and_then(Value::as_str),
            Some("@ai-sdk/openai")
        );
        // AI SDK openai 只在 baseURL 后追加 /responses，Codex 源的 base 语义
        // 是补 /v1（与代理层 Codex 适配器一致）——origin 缺 /v1 时必须补齐。
        assert_eq!(
            responses_settings
                .pointer("/options/baseURL")
                .and_then(Value::as_str),
            Some("https://relay.example.com/v1")
        );
        // is_full_url 的 Codex 源是完整端点（…/responses）：剥掉尾部 /responses，
        // AI SDK 重新追加 /responses 后请求 URL 与源端一致。
        let mut responses_full_url = provider_with_settings(json!({
            "auth": { "OPENAI_API_KEY": "k" },
            "config": "model = \"gpt-5-codex\"\n"
        }));
        responses_full_url
            .meta
            .get_or_insert_with(ProviderMeta::default)
            .is_full_url = Some(true);
        let responses_full_url_settings = build_target_settings(
            AppType::OpenCode,
            &AppType::Codex,
            "https://gateway.example.com/v1/responses",
            "sk-relay",
            "Relay",
            &["m1".to_string()],
            &responses_full_url,
        )
        .expect("build opencode target from responses full url");
        assert_eq!(
            responses_full_url_settings
                .pointer("/options/baseURL")
                .and_then(Value::as_str),
            Some("https://gateway.example.com/v1")
        );

        let chat_source = provider_with_settings(json!({
            "baseUrl": "https://relay.example.com",
            "apiKey": "sk-relay",
            "api": "openai-completions",
            "models": [{ "id": "m1" }]
        }));
        let chat_settings = build_target_settings(
            AppType::OpenCode,
            &AppType::OpenClaw,
            "https://relay.example.com",
            "sk-relay",
            "Relay",
            &["m1".to_string()],
            &chat_source,
        )
        .expect("build opencode target from chat source");
        assert_eq!(
            chat_settings.get("npm").and_then(Value::as_str),
            Some("@ai-sdk/openai-compatible")
        );
    }

    #[test]
    fn extract_models_from_each_source_app() {
        // Claude: env 模型 + [1M] 后缀剥离 + 去重
        let claude = provider_with_settings(json!({
            "env": {
                "ANTHROPIC_MODEL": "claude-sonnet-4-5 [1m]",
                "ANTHROPIC_DEFAULT_SONNET_MODEL": "claude-sonnet-4-5",
                "ANTHROPIC_DEFAULT_OPUS_MODEL": "claude-opus-4-8"
            }
        }));
        assert_eq!(
            extract_source_model_names(
                &AppType::Claude,
                &claude.settings_config,
                claude.meta.as_ref()
            ),
            vec![
                "claude-sonnet-4-5".to_string(),
                "claude-opus-4-8".to_string()
            ]
        );

        // Codex：config TOML 文本中的顶层 model（真实存储形状，与官方 seed 一致）
        let codex = provider_with_settings(json!({
            "auth": { "OPENAI_API_KEY": "k" },
            "config": "model = \"gpt-5-codex\"\n"
        }));
        assert_eq!(
            extract_source_model_names(&AppType::Codex, &codex.settings_config, None),
            vec!["gpt-5-codex".to_string()]
        );

        // GrokBuild：[models].default + [model."<name>"] 键
        let grok = provider_with_settings(json!({
            "config": "[models]\ndefault = \"grok-4\"\n\n[model.\"grok-4\"]\nmodel = \"grok-4\"\n"
        }));
        assert_eq!(
            extract_source_model_names(&AppType::GrokBuild, &grok.settings_config, None),
            vec!["grok-4".to_string()]
        );

        // OpenClaw/Hermes/Pi：models[].id
        let openclaw = provider_with_settings(json!({
            "models": [{ "id": "m1" }, { "id": "m2" }]
        }));
        assert_eq!(
            extract_source_model_names(&AppType::OpenClaw, &openclaw.settings_config, None),
            vec!["m1".to_string(), "m2".to_string()]
        );

        // OpenCode：models map 键
        let opencode = provider_with_settings(json!({
            "models": { "m1": { "name": "m1" } }
        }));
        assert_eq!(
            extract_source_model_names(&AppType::OpenCode, &opencode.settings_config, None),
            vec!["m1".to_string()]
        );

        // cap 8：10 个候选只保留前 8 个
        let entries: Vec<Value> = (0..10)
            .map(|index| json!({ "id": format!("m{index}") }))
            .collect();
        let capped = provider_with_settings(json!({ "models": entries }));
        let extracted =
            extract_source_model_names(&AppType::OpenClaw, &capped.settings_config, None);
        assert_eq!(extracted.len(), 8);
        assert_eq!(extracted.first().map(String::as_str), Some("m0"));
        assert_eq!(extracted.last().map(String::as_str), Some("m7"));
    }

    #[test]
    fn derive_source_api_format_table() {
        // Claude / Desktop 源 → anthropic
        assert_eq!(
            derive_source_api_format(&claude_source(), &AppType::Claude),
            "anthropic"
        );
        // Claude 旧格式声明（settings_config.api_format / openrouter_compat_mode）
        // 复用 get_claude_api_format 判定链。
        let claude_legacy_chat = provider_with_settings(json!({
            "env": { "ANTHROPIC_BASE_URL": "https://relay.example.com" },
            "api_format": "openai_chat"
        }));
        assert_eq!(
            derive_source_api_format(&claude_legacy_chat, &AppType::Claude),
            "openai_chat"
        );
        let claude_legacy_compat = provider_with_settings(json!({
            "env": { "ANTHROPIC_BASE_URL": "https://openrouter.ai/api" },
            "openrouter_compat_mode": true
        }));
        assert_eq!(
            derive_source_api_format(&claude_legacy_compat, &AppType::Claude),
            "openai_chat"
        );
        // codex_oauth/xai_oauth 协议不变量（claude.rs priority 0）压过 meta.api_format。
        let mut codex_oauth_source = provider_with_settings(json!({
            "env": { "ANTHROPIC_BASE_URL": "https://api.x.ai/v1" }
        }));
        let codex_oauth_meta = codex_oauth_source
            .meta
            .get_or_insert_with(ProviderMeta::default);
        codex_oauth_meta.provider_type = Some("codex_oauth".to_string());
        codex_oauth_meta.api_format = Some("anthropic".to_string());
        assert_eq!(
            derive_source_api_format(&codex_oauth_source, &AppType::Claude),
            "openai_responses"
        );
        // Codex 旧协议声明（settings 级 api_format/apiFormat）优先于 TOML wire_api。
        let codex_legacy_anthropic = provider_with_settings(json!({
            "auth": { "OPENAI_API_KEY": "k" },
            "api_format": "anthropic",
            "config": "model = \"gpt-5-codex\"\n"
        }));
        assert_eq!(
            derive_source_api_format(&codex_legacy_anthropic, &AppType::Codex),
            "anthropic"
        );
        let codex_legacy_chat = provider_with_settings(json!({
            "auth": { "OPENAI_API_KEY": "k" },
            "apiFormat": "chat",
            "config": "model = \"gpt-5-codex\"\n"
        }));
        assert_eq!(
            derive_source_api_format(&codex_legacy_chat, &AppType::Codex),
            "openai_chat"
        );
        let codex_declared_wins = provider_with_settings(json!({
            "auth": { "OPENAI_API_KEY": "k" },
            "api_format": "chat",
            "config": "model_provider = \"custom\"\n\n[model_providers.custom]\nname = \"c\"\nbase_url = \"https://relay.example.com\"\nwire_api = \"anthropic\"\n"
        }));
        assert_eq!(
            derive_source_api_format(&codex_declared_wins, &AppType::Codex),
            "openai_chat"
        );
        // Codex / Gemini 源：wire_api 缺省按 Responses；显式声明按枚举映射
        let codex = provider_with_settings(json!({
            "auth": { "OPENAI_API_KEY": "k" },
            "config": "model = \"gpt-5-codex\"\n"
        }));
        assert_eq!(
            derive_source_api_format(&codex, &AppType::Codex),
            "openai_responses"
        );
        let codex_chat = provider_with_settings(json!({
            "auth": { "OPENAI_API_KEY": "k" },
            "config": "model_provider = \"custom\"\n\n[model_providers.custom]\nname = \"c\"\nbase_url = \"https://relay.example.com/v1\"\nwire_api = \"chat\"\n"
        }));
        assert_eq!(
            derive_source_api_format(&codex_chat, &AppType::Codex),
            "openai_chat"
        );
        let codex_anthropic = provider_with_settings(json!({
            "auth": { "OPENAI_API_KEY": "k" },
            "config": "model_provider = \"custom\"\n\n[model_providers.custom]\nname = \"c\"\nbase_url = \"https://relay.example.com\"\nwire_api = \"anthropic\"\n"
        }));
        assert_eq!(
            derive_source_api_format(&codex_anthropic, &AppType::Codex),
            "anthropic"
        );
        let gemini = provider_with_settings(json!({
            "env": { "GEMINI_API_KEY": "k" }
        }));
        assert_eq!(
            derive_source_api_format(&gemini, &AppType::Gemini),
            "gemini_native"
        );
        // OpenClaw api 映射
        let mut openclaw_gemini = provider_with_settings(json!({ "api": "google-generative-ai" }));
        assert_eq!(
            derive_source_api_format(&openclaw_gemini, &AppType::OpenClaw),
            "gemini_native"
        );
        // Hermes api_mode 映射（Responses 实存为 codex_responses）
        let hermes = provider_with_settings(json!({ "api_mode": "codex_responses" }));
        assert_eq!(
            derive_source_api_format(&hermes, &AppType::Hermes),
            "openai_responses"
        );
        // OpenCode npm 映射（npm 枚举见 opencodeProviderPresets.ts）
        let opencode_anthropic = provider_with_settings(json!({ "npm": "@ai-sdk/anthropic" }));
        assert_eq!(
            derive_source_api_format(&opencode_anthropic, &AppType::OpenCode),
            "anthropic"
        );
        let opencode_responses = provider_with_settings(json!({ "npm": "@ai-sdk/openai" }));
        assert_eq!(
            derive_source_api_format(&opencode_responses, &AppType::OpenCode),
            "openai_responses"
        );
        let opencode_gemini = provider_with_settings(json!({ "npm": "@ai-sdk/google" }));
        assert_eq!(
            derive_source_api_format(&opencode_gemini, &AppType::OpenCode),
            "gemini_native"
        );
        let opencode_compatible =
            provider_with_settings(json!({ "npm": "@ai-sdk/openai-compatible" }));
        assert_eq!(
            derive_source_api_format(&opencode_compatible, &AppType::OpenCode),
            "openai_chat"
        );
        // GrokBuild best-effort
        assert_eq!(
            derive_source_api_format(&claude_source(), &AppType::GrokBuild),
            "openai_chat"
        );
        // 显式 meta.api_format 优先
        openclaw_gemini
            .meta
            .get_or_insert_with(ProviderMeta::default)
            .api_format = Some("openai_chat".to_string());
        assert_eq!(
            derive_source_api_format(&openclaw_gemini, &AppType::OpenClaw),
            "openai_chat"
        );
    }

    #[test]
    fn copy_meta_carries_is_full_url_for_claude_and_codex_targets() {
        let mut source = claude_source();
        source
            .meta
            .get_or_insert_with(ProviderMeta::default)
            .is_full_url = Some(true);

        let claude_meta = build_copy_meta(&source, &AppType::Claude, AppType::Claude);
        assert_eq!(claude_meta.is_full_url, Some(true));
        assert_eq!(claude_meta.api_format.as_deref(), Some("anthropic"));

        // Codex 目标的代理转发同样按完整端点处理，标记必须带过去，
        // 否则 …/v1/responses 会被再拼一层 /responses。
        let codex_meta = build_copy_meta(&source, &AppType::Claude, AppType::Codex);
        assert_eq!(codex_meta.is_full_url, Some(true));
        assert_eq!(codex_meta.api_format.as_deref(), Some("anthropic"));

        // 其余目标维持丢弃语义。
        let opencode_meta = build_copy_meta(&source, &AppType::Claude, AppType::OpenCode);
        assert_eq!(opencode_meta.is_full_url, None);
    }

    #[test]
    fn copy_meta_carries_anthropic_api_key_field_choice() {
        // 表单显式选择 ANTHROPIC_API_KEY 的 Claude 源 → 副本 meta 保留该选择，
        // Codex 代理适配器才能继续发 x-api-key 而不是退回 Bearer。
        let mut source = claude_source();
        source
            .meta
            .get_or_insert_with(ProviderMeta::default)
            .api_key_field = Some("ANTHROPIC_API_KEY".to_string());
        let meta = build_copy_meta(&source, &AppType::Claude, AppType::Codex);
        assert_eq!(meta.api_key_field.as_deref(), Some("ANTHROPIC_API_KEY"));

        // 默认（AUTH_TOKEN）源不带该字段，None 保持默认语义。
        let default_meta = build_copy_meta(&claude_source(), &AppType::Claude, AppType::Codex);
        assert_eq!(default_meta.api_key_field, None);

        // 非 Anthropic 协议副本不携带。
        let codex_source = provider_with_settings(json!({
            "auth": { "OPENAI_API_KEY": "k" },
            "config": "model = \"gpt-5-codex\"\n"
        }));
        let non_anthropic = build_copy_meta(&codex_source, &AppType::Codex, AppType::Codex);
        assert_eq!(non_anthropic.api_key_field, None);
    }

    #[test]
    fn resolve_claude_desktop_copy_modes() {
        // 兼容且模型 Claude-safe → Direct（无路由）
        let (mode, routes) = resolve_claude_desktop_copy(&claude_source())
            .expect("direct-compatible source resolves");
        assert_eq!(mode, ClaudeDesktopMode::Direct);
        assert!(routes.is_empty());

        // kimi 类模型 → Proxy + 路由指向 kimi
        let mut kimi_source = claude_source();
        kimi_source.settings_config["env"]["ANTHROPIC_MODEL"] = json!("kimi-k2");
        let (mode, routes) =
            resolve_claude_desktop_copy(&kimi_source).expect("proxy-compatible source resolves");
        assert_eq!(mode, ClaudeDesktopMode::Proxy);
        assert!(routes.values().any(|route| route.model == "kimi-k2"));

        // 非 env 形状（Codex 配置）→ None → skip
        let codex = provider_with_settings(json!({
            "auth": { "OPENAI_API_KEY": "k" },
            "config": "model = \"gpt-5-codex\"\n"
        }));
        assert!(resolve_claude_desktop_copy(&codex).is_none());
    }

    #[test]
    fn source_rejection_rules() {
        let app = AppType::Claude;

        // 官方 seed id
        let mut official = claude_source();
        official.id = crate::database::CODEX_OFFICIAL_PROVIDER_ID.to_string();
        assert!(source_rejection(&official, &app).is_some());

        // category == official
        let mut categorized = claude_source();
        categorized.category = Some("official".to_string());
        assert!(source_rejection(&categorized, &app).is_some());

        // 代理注入 OAuth（xai_oauth）
        let mut oauth = claude_source();
        oauth
            .meta
            .get_or_insert_with(ProviderMeta::default)
            .provider_type = Some("xai_oauth".to_string());
        let rejection = source_rejection(&oauth, &app).expect("proxy-injected oauth rejected");
        assert!(
            rejection.to_string().contains("proxy"),
            "managed auth message: {rejection}"
        );

        // 空凭据
        let keyless = provider_with_settings(json!({
            "env": { "ANTHROPIC_BASE_URL": "https://relay.example.com" }
        }));
        let rejection = source_rejection(&keyless, &app).expect("keyless rejected");
        assert!(
            rejection.to_string().contains("base_url") || rejection.to_string().contains("API key")
        );

        // 普通第三方放行
        assert!(source_rejection(&claude_source(), &app).is_none());
    }
}
