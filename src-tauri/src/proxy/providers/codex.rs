//! Codex (OpenAI) Provider Adapter
//!
//! 仅透传模式，支持直连 OpenAI API
//!
//! ## 客户端检测
//! 支持检测官方 Codex 客户端 (codex_vscode, codex_cli_rs)

use super::{AuthInfo, AuthStrategy, ProviderAdapter};
use crate::provider::{
    AuthBinding, AuthBindingSource, CodexChatReasoningConfig, Provider, ProviderMeta,
};
use crate::proxy::error::ProxyError;
use regex::Regex;
use serde_json::{Map, Value as JsonValue};
use std::collections::HashSet;
use std::sync::LazyLock;
use toml::Value as TomlValue;

/// 官方 Codex 客户端 User-Agent 正则
#[allow(dead_code)]
static CODEX_CLIENT_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(codex_vscode|codex_cli_rs)/[\d.]+").unwrap());

/// Codex 适配器
pub struct CodexAdapter;

/// Whether this Codex provider's real upstream should be called through
/// OpenAI Chat Completions, even if the local Codex client is talking to CC
/// Switch through the Responses API.
pub fn codex_provider_uses_chat_completions(provider: &Provider) -> bool {
    if let Some(api_format) = provider
        .meta
        .as_ref()
        .and_then(|meta| meta.api_format.as_deref())
        .or_else(|| {
            provider
                .settings_config
                .get("api_format")
                .and_then(|v| v.as_str())
        })
        .or_else(|| {
            provider
                .settings_config
                .get("apiFormat")
                .and_then(|v| v.as_str())
        })
    {
        return is_chat_wire_api(api_format);
    }

    if let Some(wire_api) = provider
        .settings_config
        .get("config")
        .and_then(|v| v.as_str())
        .and_then(extract_codex_wire_api_from_toml)
    {
        return is_chat_wire_api(&wire_api);
    }

    if let Some(base_url) = provider
        .settings_config
        .get("base_url")
        .or_else(|| provider.settings_config.get("baseURL"))
        .and_then(|v| v.as_str())
    {
        return is_chat_completions_url(base_url);
    }

    provider
        .settings_config
        .get("config")
        .and_then(|v| v.as_str())
        .and_then(extract_codex_base_url_from_toml)
        .map(|url| is_chat_completions_url(&url))
        .unwrap_or(false)
}

pub fn should_convert_codex_responses_to_chat(provider: &Provider, endpoint: &str) -> bool {
    let path = endpoint
        .split_once('?')
        .map_or(endpoint, |(path, _query)| path);

    matches!(
        path,
        "/responses" | "/v1/responses" | "/responses/compact" | "/v1/responses/compact"
    ) && codex_provider_uses_chat_completions(provider)
}

pub fn should_convert_codex_responses_to_messages(provider: &Provider, endpoint: &str) -> bool {
    let path = endpoint
        .split_once('?')
        .map_or(endpoint, |(path, _query)| path);

    if !matches!(
        path,
        "/responses" | "/v1/responses" | "/responses/compact" | "/v1/responses/compact"
    ) {
        return false;
    }

    let wire_api = provider
        .meta
        .as_ref()
        .and_then(|meta| meta.api_format.as_deref())
        .or_else(|| {
            provider
                .settings_config
                .get("api_format")
                .and_then(|v| v.as_str())
        })
        .or_else(|| {
            provider
                .settings_config
                .get("apiFormat")
                .and_then(|v| v.as_str())
        })
        .map(str::to_string)
        .or_else(|| {
            provider
                .settings_config
                .get("config")
                .and_then(|v| v.as_str())
                .and_then(extract_codex_wire_api_from_toml)
        })
        .unwrap_or_else(|| "openai_responses".to_string());

    is_openai_messages_wire_api(&wire_api)
}

/// 根据 Codex 请求体里的 `model` 字段，把复合 provider 解析成本次真实上游 provider。
///
/// 新 schema 使用 `settings_config.codexRouting`；旧的 `codexModelRoutes` / `modelRoutes`
/// 仍然只读兼容，便于本地旧配置在 UI 保存前继续可用。函数不访问数据库，也不改变当前
/// CC Switch provider，避免聊天窗口切模型时反向触发 GUI 当前供应商切换。
pub fn resolve_codex_model_routed_provider(
    provider: &Provider,
    body: &JsonValue,
) -> Option<Provider> {
    let request_model = body
        .get("model")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|model| !model.is_empty())?;

    let route = resolve_codex_route(provider, request_model)?;

    Some(build_codex_routed_provider(provider, route, request_model))
}

/// 判断当前 effective Codex provider 是否声明为 text-only 输入。
///
/// 该信息由 route resolver 写入 `codexResolvedCapabilities`，供 Responses -> Chat 转换
/// 在生成 OpenAI Chat `messages` 时决定是否把图片块降级成文本占位。
pub fn codex_provider_text_only_input(provider: &Provider) -> Option<bool> {
    let capabilities = provider.settings_config.get("codexResolvedCapabilities")?;
    if let Some(text_only) = capabilities
        .get("textOnly")
        .or_else(|| capabilities.get("text_only"))
        .and_then(|value| value.as_bool())
    {
        return Some(text_only);
    }

    capabilities
        .get("inputModalities")
        .or_else(|| capabilities.get("input_modalities"))
        .and_then(|value| value.as_array())
        .map(|modalities| {
            !modalities
                .iter()
                .filter_map(|value| value.as_str())
                .any(|modality| modality.eq_ignore_ascii_case("image"))
        })
}

/// 从新旧配置中挑出本次请求应该使用的 route。
///
/// 新配置允许显式关闭路由，并支持 `defaultRouteId` 兜底；旧配置没有开关语义，只要数组
/// 存在就按旧规则匹配，保证已有本地数据库不会在升级后突然失效。
fn resolve_codex_route<'a>(provider: &'a Provider, request_model: &str) -> Option<&'a JsonValue> {
    if let Some(routing) = provider.settings_config.get("codexRouting") {
        if routing
            .get("enabled")
            .and_then(|value| value.as_bool())
            .is_some_and(|enabled| !enabled)
        {
            return None;
        }

        let routes = routing.get("routes").and_then(|value| value.as_array())?;
        if let Some(route) = routes.iter().find(|route| {
            codex_route_is_enabled(route) && codex_route_matches_model(route, request_model)
        }) {
            return Some(route);
        }

        return routing
            .get("defaultRouteId")
            .or_else(|| routing.get("default_route_id"))
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .and_then(|default_route_id| {
                routes.iter().find(|route| {
                    codex_route_is_enabled(route)
                        && route
                            .get("id")
                            .and_then(|value| value.as_str())
                            .is_some_and(|id| id.eq_ignore_ascii_case(default_route_id))
                })
            });
    }

    provider
        .settings_config
        .get("codexModelRoutes")
        .or_else(|| provider.settings_config.get("modelRoutes"))
        .and_then(|value| value.as_array())
        .and_then(|routes| {
            routes
                .iter()
                .find(|route| codex_route_matches_model(route, request_model))
        })
}

/// 判断 route 是否启用；字段缺省时按启用处理，减少手写配置的必填项。
fn codex_route_is_enabled(route: &JsonValue) -> bool {
    route
        .get("enabled")
        .and_then(|value| value.as_bool())
        .unwrap_or(true)
}

/// 判断单条 Codex route 是否匹配请求模型。
///
/// 新 schema 使用 `match.models` / `match.prefixes`；旧 schema 使用顶层 `models` /
/// `modelPrefixes`。两套字段都按大小写不敏感处理，避免 UI 显示大小写差异导致误路由。
pub(crate) fn codex_route_matches_model(route: &JsonValue, request_model: &str) -> bool {
    let request_model_lower = request_model.to_ascii_lowercase();

    let match_config = route.get("match").unwrap_or(route);

    let exact_match = match_config
        .get("models")
        .and_then(|value| value.as_array())
        .into_iter()
        .flatten()
        .filter_map(|model| model.as_str())
        .any(|model| model.trim().eq_ignore_ascii_case(request_model));
    if exact_match {
        return true;
    }

    match_config
        .get("prefixes")
        .or_else(|| match_config.get("modelPrefixes"))
        .or_else(|| match_config.get("model_prefixes"))
        .or_else(|| route.get("modelPrefixes"))
        .or_else(|| route.get("model_prefixes"))
        .and_then(|prefixes| prefixes.as_array())
        .into_iter()
        .flatten()
        .filter_map(|prefix| prefix.as_str())
        .map(str::trim)
        .filter(|prefix| !prefix.is_empty())
        .any(|prefix| request_model_lower.starts_with(&prefix.to_ascii_lowercase()))
}

/// 从 route 配置构造本次请求实际使用的 provider。
///
/// 保留原 provider 的 `modelCatalog` 等 UI 元数据，只覆盖上游连接必需字段。这样 Chat
/// 转换时仍能识别下拉框中的模型，避免把 `deepseek-v4-flash` 覆盖回 provider 默认模型。
fn build_codex_routed_provider(
    provider: &Provider,
    route: &JsonValue,
    request_model: &str,
) -> Provider {
    let mut routed = provider.clone();
    let upstream = route.get("upstream").unwrap_or(route);

    let route_id = route
        .get("id")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .unwrap_or(request_model);
    routed.id = format!("{}::route::{}", provider.id, route_id);

    if let Some(name) = route
        .get("label")
        .or_else(|| route.get("name"))
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|name| !name.is_empty())
    {
        routed.name = name.to_string();
    }

    let mut settings = provider
        .settings_config
        .as_object()
        .cloned()
        .unwrap_or_else(Map::new);

    if let Some(base_url) = upstream
        .get("baseUrl")
        .or_else(|| upstream.get("base_url"))
        .or_else(|| route.get("baseUrl"))
        .or_else(|| route.get("baseURL"))
        .or_else(|| route.get("base_url"))
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|url| !url.is_empty())
    {
        settings.insert(
            "base_url".to_string(),
            JsonValue::String(base_url.to_string()),
        );
    }

    let upstream_model = upstream
        .get("modelMap")
        .or_else(|| upstream.get("model_map"))
        .and_then(|value| value.as_object())
        .and_then(|map| map.get(request_model))
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|model| !model.is_empty())
        .or_else(|| {
            upstream
                .get("upstreamModel")
                .or_else(|| upstream.get("upstream_model"))
                .or_else(|| upstream.get("model"))
                .and_then(|value| value.as_str())
                .map(str::trim)
                .filter(|model| !model.is_empty())
        })
        .or_else(|| {
            route
                .get("upstreamModel")
                .or_else(|| route.get("upstream_model"))
                .or_else(|| route.get("model"))
                .and_then(|value| value.as_str())
                .map(str::trim)
                .filter(|model| !model.is_empty())
        })
        .unwrap_or(request_model);
    settings.insert(
        "model".to_string(),
        JsonValue::String(upstream_model.to_string()),
    );

    if codex_route_uses_managed_codex_oauth(upstream, route) {
        // 托管 Codex OAuth route 不能继承外层 provider 的 Bearer key，否则会覆盖 managed account 注入链路。
        settings.remove("auth");
        settings.remove("apiKey");
        settings.remove("api_key");
    }
    apply_codex_route_auth(upstream, route, &mut settings);

    if let Some(wire_api) = codex_route_api_format(upstream, route) {
        settings.insert(
            "apiFormat".to_string(),
            JsonValue::String(wire_api.to_string()),
        );
    }
    if let Some(capabilities) = route.get("capabilities").cloned() {
        settings.insert("codexResolvedCapabilities".to_string(), capabilities);
    }
    settings.insert(
        "codexResolvedRouteId".to_string(),
        JsonValue::String(route_id.to_string()),
    );

    routed.settings_config = JsonValue::Object(settings);

    let mut meta = routed.meta.clone().unwrap_or_else(ProviderMeta::default);
    if let Some(wire_api) = codex_route_api_format(upstream, route) {
        meta.api_format = Some(wire_api.to_string());
    }
    if codex_route_uses_managed_codex_oauth(upstream, route) {
        meta.provider_type = Some("codex_oauth".to_string());
    } else if let Some(provider_type) = upstream
        .get("providerType")
        .or_else(|| upstream.get("provider_type"))
        .or_else(|| route.get("providerType"))
        .or_else(|| route.get("provider_type"))
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|provider_type| !provider_type.is_empty())
    {
        meta.provider_type = Some(provider_type.to_string());
    }
    if let Some(auth_binding) = codex_route_auth_binding(upstream, route) {
        meta.auth_binding = Some(auth_binding);
    } else if let Some(auth_binding) = upstream
        .get("authBinding")
        .or_else(|| upstream.get("auth_binding"))
        .or_else(|| route.get("authBinding"))
    {
        if let Ok(binding) = serde_json::from_value(auth_binding.clone()) {
            meta.auth_binding = Some(binding);
        }
    }
    routed.meta = Some(meta);

    routed
}

/// 解析 route 的上游 API 格式，并归一化到 provider meta 使用的枚举字符串。
fn codex_route_api_format<'a>(upstream: &'a JsonValue, route: &'a JsonValue) -> Option<&'a str> {
    upstream
        .get("wire_api")
        .or_else(|| upstream.get("wireApi"))
        .or_else(|| upstream.get("apiFormat"))
        .or_else(|| route.get("wire_api"))
        .or_else(|| route.get("wireApi"))
        .or_else(|| route.get("apiFormat"))
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|wire_api| !wire_api.is_empty())
        .map(|wire_api| match wire_api {
            "responses" => "openai_responses",
            "chat" => "openai_chat",
            "messages" => "openai_messages",
            other => other,
        })
}

/// 根据 route 的 auth source 写入 effective provider 认证信息。
///
/// `provider_config` 支持 route 自带 API key；`managed_account` / `managed_codex_oauth`
/// 只设置 meta，让现有 Codex OAuth adapter 继续负责 token 注入。
fn apply_codex_route_auth(
    upstream: &JsonValue,
    route: &JsonValue,
    settings: &mut Map<String, JsonValue>,
) {
    let auth_source = upstream
        .get("auth")
        .or_else(|| route.get("auth"))
        .and_then(|auth| auth.get("source"))
        .and_then(|value| value.as_str())
        .map(str::trim);

    if let Some(auth) = upstream.get("auth").or_else(|| route.get("auth")) {
        let mut should_insert_auth = true;
        if let Some(source) = auth_source {
            if matches!(source, "managed_account" | "managed_codex_oauth") {
                return;
            }
            if source == "provider_config" {
                let has_inline_key = auth
                    .get("OPENAI_API_KEY")
                    .or_else(|| auth.get("apiKey"))
                    .or_else(|| auth.get("api_key"))
                    .and_then(|value| value.as_str())
                    .map(str::trim)
                    .is_some_and(|key| !key.is_empty());
                if !has_inline_key {
                    // provider_config 是 route 对现有 provider 鉴权的引用声明；没有内联 key 时不能覆盖原 auth。
                    should_insert_auth = false;
                }
            }
        }
        if should_insert_auth {
            settings.insert("auth".to_string(), auth.clone());
        }
    }
    if let Some(env) = upstream.get("env").or_else(|| route.get("env")).cloned() {
        settings.insert("env".to_string(), env);
    }
    if auth_source.is_some_and(|source| source != "provider_config") {
        // 托管账号 route 的鉴权必须由 meta/auth_binding 注入；忽略残留 apiKey，避免 UI 切换 auth source 后误走 Bearer。
        return;
    }
    if let Some(api_key) = upstream
        .get("apiKey")
        .or_else(|| upstream.get("api_key"))
        .or_else(|| route.get("apiKey"))
        .or_else(|| route.get("api_key"))
        .cloned()
    {
        if api_key
            .as_str()
            .map(str::trim)
            .is_some_and(|key| !key.is_empty())
        {
            let mut auth = Map::new();
            auth.insert(
                "OPENAI_API_KEY".to_string(),
                JsonValue::String(api_key.as_str().unwrap_or_default().to_string()),
            );
            settings.insert("auth".to_string(), JsonValue::Object(auth));
        }
        settings.insert("apiKey".to_string(), api_key);
    }
}

/// 判断 route 是否声明使用 CC Switch 托管的 Codex OAuth 账号。
fn codex_route_uses_managed_codex_oauth(upstream: &JsonValue, route: &JsonValue) -> bool {
    upstream
        .get("auth")
        .or_else(|| route.get("auth"))
        .and_then(|auth| auth.get("source"))
        .and_then(|value| value.as_str())
        .map(str::trim)
        .is_some_and(|source| matches!(source, "managed_account" | "managed_codex_oauth"))
}

/// 把 route 内联 auth 声明转换成 ProviderMeta 的托管账号绑定。
///
/// `managed_account` 使用标准 `AuthBinding` 字段；`managed_codex_oauth` 是 UI 友好的简写，
/// 自动归一化为 `authProvider = "codex_oauth"`。
fn codex_route_auth_binding(upstream: &JsonValue, route: &JsonValue) -> Option<AuthBinding> {
    let auth = upstream.get("auth").or_else(|| route.get("auth"))?;
    let source = auth
        .get("source")
        .and_then(|value| value.as_str())
        .map(str::trim)?;

    if source == "managed_account" {
        return serde_json::from_value(auth.clone()).ok();
    }

    if source == "managed_codex_oauth" {
        return Some(AuthBinding {
            source: AuthBindingSource::ManagedAccount,
            auth_provider: Some("codex_oauth".to_string()),
            account_id: auth
                .get("accountId")
                .or_else(|| auth.get("account_id"))
                .and_then(|value| value.as_str())
                .map(str::trim)
                .filter(|account_id| !account_id.is_empty())
                .map(ToString::to_string),
        });
    }

    None
}
/// Extract the real upstream model configured for a Codex provider.
pub fn codex_provider_upstream_model(provider: &Provider) -> Option<String> {
    provider
        .settings_config
        .get("model")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|model| !model.is_empty())
        .map(ToString::to_string)
        .or_else(|| {
            provider
                .settings_config
                .get("config")
                .and_then(|v| v.as_str())
                .and_then(extract_codex_model_from_toml)
        })
}

fn codex_provider_catalog_model_ids(provider: &Provider) -> HashSet<String> {
    provider
        .settings_config
        .get("modelCatalog")
        .and_then(|catalog| catalog.get("models"))
        .and_then(|models| models.as_array())
        .map(|models| {
            models
                .iter()
                .filter_map(|model| model.get("model").and_then(|value| value.as_str()))
                .map(str::trim)
                .filter(|model| !model.is_empty())
                .map(ToString::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// For Codex Chat providers, ensure the request uses the configured upstream
/// model before converting the request to Chat Completions.
pub fn apply_codex_chat_upstream_model(
    provider: &Provider,
    body: &mut JsonValue,
) -> Option<String> {
    if !codex_provider_uses_chat_completions(provider) {
        return None;
    }

    let catalog_model_ids = codex_provider_catalog_model_ids(provider);
    if let Some(request_model) = body
        .get("model")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|model| !model.is_empty())
    {
        if catalog_model_ids.contains(request_model) {
            return Some(request_model.to_string());
        }
    }

    let upstream_model = codex_provider_upstream_model(provider)?;
    body["model"] = JsonValue::String(upstream_model.clone());
    Some(upstream_model)
}

pub fn resolve_codex_chat_reasoning_config(
    provider: &Provider,
    body: &JsonValue,
) -> Option<CodexChatReasoningConfig> {
    if let Some(config) = provider
        .meta
        .as_ref()
        .and_then(|meta| meta.codex_chat_reasoning.clone())
    {
        return Some(normalize_codex_chat_reasoning_config(config));
    }

    infer_codex_chat_reasoning_config(provider, body)
}

fn normalize_codex_chat_reasoning_config(
    mut config: CodexChatReasoningConfig,
) -> CodexChatReasoningConfig {
    if config.supports_effort.unwrap_or(false) && config.supports_thinking.is_none() {
        config.supports_thinking = Some(true);
    }
    config
}

fn infer_codex_chat_reasoning_config(
    provider: &Provider,
    body: &JsonValue,
) -> Option<CodexChatReasoningConfig> {
    let model = body
        .get("model")
        .and_then(|value| value.as_str())
        .map(ToString::to_string)
        .or_else(|| codex_provider_upstream_model(provider))
        .unwrap_or_default()
        .to_ascii_lowercase();
    let base_url = provider
        .settings_config
        .get("base_url")
        .or_else(|| provider.settings_config.get("baseURL"))
        .and_then(|v| v.as_str())
        .map(ToString::to_string)
        .or_else(|| {
            provider
                .settings_config
                .get("config")
                .and_then(|v| v.as_str())
                .and_then(extract_codex_base_url_from_toml)
        })
        .unwrap_or_default()
        .to_ascii_lowercase();
    let name = provider.name.to_ascii_lowercase();

    // 平台优先：聚合 / 托管平台的 reasoning 接口由平台的推理框架决定，而非模型官方实现，
    // 因此先按平台标识（仅 name + base_url，不含 model 名）判定并覆盖模型规则。
    if let Some(config) = infer_aggregator_platform_config(&name, &base_url) {
        return Some(config);
    }

    let haystack = format!("{name} {base_url} {model}");

    if haystack.contains("deepseek") {
        return Some(CodexChatReasoningConfig {
            supports_thinking: Some(true),
            supports_effort: Some(true),
            thinking_param: Some("thinking".to_string()),
            effort_param: Some("reasoning_effort".to_string()),
            effort_value_mode: Some("deepseek".to_string()),
            output_format: Some("reasoning_content".to_string()),
        });
    }

    // StepFun：仅 step-3.5-flash-2603 这一版支持 reasoning effort（low/high 两档），
    // 其余 step 模型不暴露 effort，故 supports_effort 仅对含 "2603" 的模型置真。
    // 第二个 OR 分支覆盖「经中转/聚合跑该模型、但平台 name/base_url 不含 stepfun」的情况。
    if haystack.contains("stepfun") || haystack.contains("step-3.5-flash-2603") {
        return Some(CodexChatReasoningConfig {
            supports_thinking: Some(true),
            supports_effort: Some(model.contains("2603")),
            thinking_param: Some("none".to_string()),
            effort_param: Some("reasoning_effort".to_string()),
            effort_value_mode: Some("low_high".to_string()),
            output_format: Some("reasoning".to_string()),
        });
    }

    if haystack.contains("kimi") || haystack.contains("moonshot") {
        return Some(CodexChatReasoningConfig {
            supports_thinking: Some(true),
            supports_effort: Some(false),
            thinking_param: Some("thinking".to_string()),
            effort_param: Some("none".to_string()),
            effort_value_mode: None,
            output_format: Some("reasoning_content".to_string()),
        });
    }

    if haystack.contains("glm") || haystack.contains("zhipu") || haystack.contains("z.ai") {
        return Some(CodexChatReasoningConfig {
            supports_thinking: Some(true),
            supports_effort: Some(false),
            thinking_param: Some("thinking".to_string()),
            effort_param: Some("none".to_string()),
            effort_value_mode: None,
            output_format: Some("reasoning_content".to_string()),
        });
    }

    if haystack.contains("qwen") || haystack.contains("dashscope") || haystack.contains("bailian") {
        return Some(CodexChatReasoningConfig {
            supports_thinking: Some(true),
            supports_effort: Some(false),
            thinking_param: Some("enable_thinking".to_string()),
            effort_param: Some("none".to_string()),
            effort_value_mode: None,
            output_format: Some("reasoning_content".to_string()),
        });
    }

    if haystack.contains("minimax") {
        return Some(CodexChatReasoningConfig {
            supports_thinking: Some(true),
            supports_effort: Some(false),
            thinking_param: Some("reasoning_split".to_string()),
            effort_param: Some("none".to_string()),
            effort_value_mode: None,
            output_format: Some("reasoning_details".to_string()),
        });
    }

    if haystack.contains("mimo") {
        return Some(CodexChatReasoningConfig {
            supports_thinking: Some(true),
            supports_effort: Some(false),
            thinking_param: Some("thinking".to_string()),
            effort_param: Some("none".to_string()),
            effort_value_mode: None,
            output_format: Some("reasoning_content".to_string()),
        });
    }

    None
}

/// 聚合 / 托管平台的 reasoning 接口由平台决定：同一个模型在不同平台参数可能完全不同
/// （DeepSeek 官方用 `thinking:{type}`、SiliconFlow 用 `enable_thinking`、
/// OpenRouter 用原生 `reasoning:{effort}` 对象）。仅以平台标识（name / base_url）判定，
/// 绝不掺入 model 名——model 名属于模型厂商，会把托管平台误判成模型官方接口。
fn infer_aggregator_platform_config(
    name: &str,
    base_url: &str,
) -> Option<CodexChatReasoningConfig> {
    let platform = format!("{name} {base_url}");

    // OpenRouter：用原生归一化对象 `reasoning: { effort }`（由 OpenRouter 翻译成各底层
    // 模型的正确推理参数，比顶层 OpenAI 别名 reasoning_effort 覆盖面更全）。effort 走
    // "openrouter" 值映射：枚举为 xhigh|high|medium|low|minimal，无 max——max 会触发
    // `400 reasoning_effort: Invalid option`（见 openclaw#77350），故钳到 xhigh。
    // 安全降级：不发 `thinking:{type}`（OpenRouter 不认该字段），避免误配导致请求被拒。
    if platform.contains("openrouter") {
        return Some(CodexChatReasoningConfig {
            supports_thinking: Some(false),
            supports_effort: Some(true),
            thinking_param: Some("none".to_string()),
            effort_param: Some("reasoning.effort".to_string()),
            effort_value_mode: Some("openrouter".to_string()),
            output_format: Some("auto".to_string()),
        });
    }

    // SiliconFlow：平台级统一 `enable_thinking`，思维回传 reasoning_content。
    // 安全降级：不按 reasoning_effort 发 effort（平台用 thinking_budget 控制深度，
    // 发 reasoning_effort 反而可能不被接受）。
    if platform.contains("siliconflow") {
        return Some(CodexChatReasoningConfig {
            supports_thinking: Some(true),
            supports_effort: Some(false),
            thinking_param: Some("enable_thinking".to_string()),
            effort_param: Some("none".to_string()),
            effort_value_mode: None,
            output_format: Some("reasoning_content".to_string()),
        });
    }

    None
}

fn is_chat_wire_api(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "chat"
            | "chat_completions"
            | "chat-completions"
            | "openai_chat"
            | "openai-chat"
            | "openai_chat_completions"
    )
}

/// 判断是否为 OpenAI 的 Messages 风格 API：
/// `messages`/`openai_messages` 需要把 Responses 转换为 Chat 请求中的 `messages`。
fn is_openai_messages_wire_api(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "messages" | "openai_messages" | "openai-messages"
    )
}

fn is_chat_completions_url(value: &str) -> bool {
    value
        .trim_end_matches('/')
        .to_ascii_lowercase()
        .ends_with("/chat/completions")
}

/// `scheme://host` 之后没有路径段的纯 origin 形式。`build_url` 在这种情况下
/// 会自动补 `/v1`；Stream Check 等同步生产路径的代码也需要同一判定。
pub fn is_origin_only_url(value: &str) -> bool {
    let trimmed = value.trim_end_matches('/');
    match trimmed.split_once("://") {
        Some((_scheme, rest)) => !rest.contains('/'),
        None => !trimmed.contains('/'),
    }
}

fn extract_codex_wire_api_from_toml(config_text: &str) -> Option<String> {
    let doc = config_text.parse::<TomlValue>().ok()?;

    if let Some(active_provider) = doc.get("model_provider").and_then(|v| v.as_str()) {
        if let Some(wire_api) = doc
            .get("model_providers")
            .and_then(|providers| providers.get(active_provider))
            .and_then(|provider| provider.get("wire_api"))
            .and_then(|v| v.as_str())
        {
            return Some(wire_api.to_string());
        }
    }

    doc.get("wire_api")
        .and_then(|v| v.as_str())
        .map(ToString::to_string)
}

fn extract_codex_model_from_toml(config_text: &str) -> Option<String> {
    let doc = config_text.parse::<TomlValue>().ok()?;

    doc.get("model")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|model| !model.is_empty())
        .map(ToString::to_string)
}

fn extract_codex_base_url_from_toml(config_text: &str) -> Option<String> {
    // Canonical parser lives in codex_config; keep this thin alias so the
    // proxy hot path and the usage-credential resolver share one implementation.
    crate::codex_config::extract_codex_base_url(config_text)
}

impl CodexAdapter {
    pub fn new() -> Self {
        Self
    }

    /// 检测是否为官方 Codex 客户端
    ///
    /// 匹配 User-Agent 模式: `^(codex_vscode|codex_cli_rs)/[\d.]+`
    #[allow(dead_code)]
    pub fn is_official_client(user_agent: &str) -> bool {
        CODEX_CLIENT_REGEX.is_match(user_agent)
    }

    /// 从 Provider 配置中提取 API Key
    fn extract_key(&self, provider: &Provider) -> Option<String> {
        // 1. 尝试从 env 中获取
        if let Some(env) = provider.settings_config.get("env") {
            if let Some(key) = env
                .get("OPENAI_API_KEY")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|key| !key.is_empty())
            {
                return Some(key.to_string());
            }
        }

        // 2. 尝试从 auth 中获取 (Codex CLI 格式)
        if let Some(auth) = provider.settings_config.get("auth") {
            if let Some(key) = crate::codex_config::extract_codex_auth_api_key(auth) {
                return Some(key.to_string());
            }
        }

        // 3. 尝试直接获取
        if let Some(key) = provider
            .settings_config
            .get("apiKey")
            .or_else(|| provider.settings_config.get("api_key"))
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|key| !key.is_empty())
        {
            return Some(key.to_string());
        }

        // 4. 尝试从 config 对象中获取
        if let Some(config) = provider.settings_config.get("config") {
            if let Some(key) = config
                .get("api_key")
                .or_else(|| config.get("apiKey"))
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|key| !key.is_empty())
            {
                return Some(key.to_string());
            }

            if let Some(config_str) = config.as_str() {
                if let Some(key) =
                    crate::codex_config::extract_codex_experimental_bearer_token(config_str)
                {
                    return Some(key);
                }
            }
        }

        None
    }
}

impl Default for CodexAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl ProviderAdapter for CodexAdapter {
    fn name(&self) -> &'static str {
        "Codex"
    }

    fn extract_base_url(&self, provider: &Provider) -> Result<String, ProxyError> {
        // Codex v2 路由到 ChatGPT OAuth 时仍然固定使用 CodexAdapter；
        // 这里补齐托管账号 provider 的 base_url 语义，避免走普通 OpenAI 兼容配置解析。
        if provider
            .meta
            .as_ref()
            .and_then(|meta| meta.provider_type.as_deref())
            == Some("codex_oauth")
        {
            return Ok("https://chatgpt.com/backend-api/codex".to_string());
        }

        // 1. 尝试直接获取 base_url 字段
        if let Some(url) = provider
            .settings_config
            .get("base_url")
            .and_then(|v| v.as_str())
        {
            return Ok(url.trim_end_matches('/').to_string());
        }

        // 2. 尝试 baseURL
        if let Some(url) = provider
            .settings_config
            .get("baseURL")
            .and_then(|v| v.as_str())
        {
            return Ok(url.trim_end_matches('/').to_string());
        }

        // 3. 尝试从 config 对象中获取
        if let Some(config) = provider.settings_config.get("config") {
            if let Some(url) = config.get("base_url").and_then(|v| v.as_str()) {
                return Ok(url.trim_end_matches('/').to_string());
            }

            // 尝试解析 TOML 字符串格式
            if let Some(config_str) = config.as_str() {
                if let Some(start) = config_str.find("base_url = \"") {
                    let rest = &config_str[start + 12..];
                    if let Some(end) = rest.find('"') {
                        return Ok(rest[..end].trim_end_matches('/').to_string());
                    }
                }
                if let Some(start) = config_str.find("base_url = '") {
                    let rest = &config_str[start + 12..];
                    if let Some(end) = rest.find('\'') {
                        return Ok(rest[..end].trim_end_matches('/').to_string());
                    }
                }
            }
        }

        Err(ProxyError::ConfigError(
            "Codex Provider 缺少 base_url 配置".to_string(),
        ))
    }

    fn extract_auth(&self, provider: &Provider) -> Option<AuthInfo> {
        // ChatGPT Codex OAuth 的真实 access_token 由 forwarder 动态换取；
        // adapter 这里只返回策略占位，保持和 ClaudeAdapter 的托管账号语义一致。
        if provider
            .meta
            .as_ref()
            .and_then(|meta| meta.provider_type.as_deref())
            == Some("codex_oauth")
        {
            return Some(AuthInfo::new(
                "codex_oauth_placeholder".to_string(),
                AuthStrategy::CodexOAuth,
            ));
        }

        self.extract_key(provider)
            .map(|key| AuthInfo::new(key, AuthStrategy::Bearer))
    }

    fn build_url(&self, base_url: &str, endpoint: &str) -> String {
        let base_trimmed = base_url.trim_end_matches('/');
        let endpoint_trimmed = endpoint.trim_start_matches('/');

        // OpenAI/Codex 的 base_url 可能是：
        // - 纯 origin: https://api.openai.com  (需要自动补 /v1)
        // - 已含 /v1: https://api.openai.com/v1 (直接拼接)
        // - 自定义前缀: https://xxx/openai (不添加 /v1，直接拼接)

        // 检查 base_url 是否已经包含 /v1
        let already_has_v1 = base_trimmed.ends_with("/v1");
        let origin_only = is_origin_only_url(base_trimmed);

        let mut url = if already_has_v1 {
            // 已经有 /v1，直接拼接
            format!("{base_trimmed}/{endpoint_trimmed}")
        } else if origin_only {
            // 纯 origin，添加 /v1
            format!("{base_trimmed}/v1/{endpoint_trimmed}")
        } else {
            // 自定义前缀，不添加 /v1，直接拼接
            format!("{base_trimmed}/{endpoint_trimmed}")
        };

        // 去除重复的 /v1/v1（可能由 base_url 与 endpoint 都带版本导致）
        while url.contains("/v1/v1") {
            url = url.replace("/v1/v1", "/v1");
        }

        url
    }

    fn get_auth_headers(
        &self,
        auth: &AuthInfo,
    ) -> Result<Vec<(http::HeaderName, http::HeaderValue)>, ProxyError> {
        use super::adapter::auth_header_value;
        let bearer = format!("Bearer {}", auth.api_key);
        match auth.strategy {
            AuthStrategy::CodexOAuth => Ok(vec![
                (
                    http::HeaderName::from_static("authorization"),
                    auth_header_value(&bearer)?,
                ),
                (
                    http::HeaderName::from_static("originator"),
                    http::HeaderValue::from_static("cc-switch"),
                ),
            ]),
            _ => Ok(vec![(
                http::HeaderName::from_static("authorization"),
                auth_header_value(&bearer)?,
            )]),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn create_provider(config: serde_json::Value) -> Provider {
        Provider {
            id: "test".to_string(),
            name: "Test Codex".to_string(),
            settings_config: config,
            website_url: None,
            category: Some("codex".to_string()),
            created_at: None,
            sort_index: None,
            notes: None,
            meta: None,
            icon: None,
            icon_color: None,
            in_failover_queue: false,
        }
    }

    #[test]
    fn test_codex_model_route_resolves_deepseek_chat_provider() {
        let provider = create_provider(json!({
            "modelCatalog": {
                "models": [
                    { "model": "deepseek-v4-flash" },
                    { "model": "gpt-5.5" }
                ]
            },
            "codexModelRoutes": [
                {
                    "id": "deepseek",
                    "name": "DeepSeek",
                    "models": ["deepseek-v4-flash", "deepseek-v4-pro"],
                    "base_url": "https://api.deepseek.com",
                    "wire_api": "chat",
                    "auth": { "OPENAI_API_KEY": "sk-deepseek" }
                }
            ]
        }));

        let routed = resolve_codex_model_routed_provider(
            &provider,
            &json!({ "model": "deepseek-v4-flash" }),
        )
        .expect("deepseek route");

        assert_eq!(routed.name, "DeepSeek");
        assert_eq!(
            routed.settings_config["base_url"],
            "https://api.deepseek.com"
        );
        assert_eq!(
            routed
                .meta
                .as_ref()
                .and_then(|meta| meta.api_format.as_deref()),
            Some("openai_chat")
        );
        assert!(should_convert_codex_responses_to_chat(
            &routed,
            "/responses"
        ));
    }

    #[test]
    fn test_codex_model_route_supports_prefix_matching() {
        let provider = create_provider(json!({
            "modelRoutes": [
                {
                    "id": "qwen",
                    "name": "Qwen",
                    "modelPrefixes": ["qwen3."],
                    "base_url": "https://www.matrixminecraft.cn:24443/vllm/v1",
                    "wireApi": "chat",
                    "auth": { "OPENAI_API_KEY": "vllm-local" }
                }
            ]
        }));

        let routed = resolve_codex_model_routed_provider(&provider, &json!({ "model": "qwen3.6" }))
            .expect("qwen route");

        assert_eq!(routed.name, "Qwen");
        assert_eq!(routed.settings_config["model"], "qwen3.6");
        assert_eq!(
            routed.settings_config["base_url"],
            "https://www.matrixminecraft.cn:24443/vllm/v1"
        );
    }

    #[test]
    fn test_codex_model_route_uses_codex_routing_first() {
        let provider = create_provider(json!({
            "codexRouting": {
                "routes": [{
                    "id": "routing-deepseek",
                    "match": {
                        "models": ["deepseek-v4-flash"]
                    },
                    "label": "DeepSeek Routing",
                    "baseUrl": "https://routing.deepseek.example",
                    "apiFormat": "chat",
                    "upstream": {
                        "modelMap": {
                            "deepseek-v4-flash": "deepseek-upstream-v4-flash"
                        }
                    },
                    "capabilities": {
                        "textOnly": true,
                        "image": {
                            "supported": false
                        }
                    }
                }],
                "enabled": true
            },
            "codexModelRoutes": [{
                "id": "legacy",
                "name": "Legacy DeepSeek",
                "models": ["deepseek-v4-flash"],
                "base_url": "https://legacy.deepseek.example",
                "wire_api": "chat"
            }]
        }));

        let routed = resolve_codex_model_routed_provider(
            &provider,
            &json!({ "model": "deepseek-v4-flash" }),
        )
        .expect("routing should resolve");

        assert_eq!(routed.name, "DeepSeek Routing");
        assert_eq!(routed.id, "test::route::routing-deepseek");
        assert_eq!(
            routed.settings_config["base_url"],
            "https://routing.deepseek.example"
        );
        assert_eq!(
            routed.settings_config["model"],
            "deepseek-upstream-v4-flash"
        );
        assert_eq!(routed.settings_config["apiFormat"], "openai_chat");
        assert_eq!(
            codex_provider_text_only_input(&routed),
            Some(true),
            "route-level textOnly should be preserved in routed provider settings"
        );
        assert_eq!(
            routed
                .meta
                .as_ref()
                .and_then(|meta| meta.api_format.as_deref()),
            Some("openai_chat")
        );
    }

    #[test]
    fn test_codex_route_default_route_is_used_when_no_match() {
        let provider = create_provider(json!({
            "codexRouting": {
                "defaultRouteId": "fallback",
                "routes": [
                    {
                        "id": "fallback",
                        "enabled": true,
                        "match": { "prefixes": ["qwen"] },
                        "label": "Qwen Fallback",
                        "base_url": "https://fallback.example"
                    },
                    {
                        "id": "disabled",
                        "enabled": false,
                        "match": { "models": ["does-not-match"] },
                        "base_url": "https://disabled.example"
                    }
                ],
                "enabled": true
            }
        }));

        let routed = resolve_codex_model_routed_provider(
            &provider,
            &json!({ "model": "deepseek-v4-flash" }),
        )
        .expect("default fallback route");

        assert_eq!(routed.id, "test::route::fallback");
        assert_eq!(routed.name, "Qwen Fallback");
        assert_eq!(
            routed.settings_config["base_url"],
            "https://fallback.example"
        );
    }

    #[test]
    fn test_codex_route_skips_disabled_matches() {
        let provider = create_provider(json!({
            "codexRouting": {
                "routes": [
                    {
                        "id": "disabled",
                        "enabled": false,
                        "match": { "models": ["deepseek-v4-flash"] },
                        "base_url": "https://disabled.example"
                    },
                    {
                        "id": "enabled",
                        "match": { "models": ["deepseek-v4-flash"] },
                        "base_url": "https://enabled.example"
                    }
                ],
                "enabled": true
            }
        }));

        let routed = resolve_codex_model_routed_provider(
            &provider,
            &json!({ "model": "deepseek-v4-flash" }),
        )
        .expect("fallback to enabled route");

        assert_eq!(routed.id, "test::route::enabled");
        assert_eq!(
            routed.settings_config["base_url"],
            "https://enabled.example"
        );
    }

    #[test]
    fn test_codex_route_managed_codex_oauth_keeps_auth_in_meta() {
        let mut provider = create_provider(json!({
            "codexRouting": {
                "routes": [{
                    "id": "codex_oauth",
                    "label": "ChatGPT OAuth Route",
                    "match": { "models": ["gpt-5.5"] },
                    "auth": {
                        "source": "managed_codex_oauth",
                        "account_id": "acct_123"
                    },
                    "base_url": "https://chatgpt.com/backend-api/codex"
                }],
                "enabled": true
            }
        }));
        provider.meta = Some(ProviderMeta::default());

        let routed = resolve_codex_model_routed_provider(&provider, &json!({ "model": "gpt-5.5" }))
            .expect("managed route");

        let meta = routed.meta.as_ref().expect("meta");
        assert_eq!(meta.provider_type.as_deref(), Some("codex_oauth"));
        assert_eq!(
            meta.auth_binding
                .as_ref()
                .and_then(|binding| binding.auth_provider.as_deref()),
            Some("codex_oauth")
        );
        assert!(routed
            .meta
            .as_ref()
            .and_then(|m| m.auth_binding.as_ref())
            .is_some());
        assert!(
            routed.settings_config.get("auth").is_none(),
            "managed auth route should not inline raw auth into settings"
        );
    }

    #[test]
    fn test_codex_route_managed_auth_ignores_stale_api_key() {
        let adapter = CodexAdapter::new();
        let mut provider = create_provider(json!({
            "auth": {
                "OPENAI_API_KEY": "sk-provider-key"
            },
            "codexRouting": {
                "routes": [{
                    "id": "codex_oauth",
                    "match": { "models": ["gpt-5.5"] },
                    "upstream": {
                        "baseUrl": "https://chatgpt.com/backend-api/codex",
                        "apiFormat": "responses",
                        "auth": {
                            "source": "managed_codex_oauth",
                            "accountId": "acct_123"
                        },
                        "apiKey": "sk-stale-route-key"
                    }
                }],
                "enabled": true
            }
        }));
        provider.meta = Some(ProviderMeta::default());

        let routed = resolve_codex_model_routed_provider(&provider, &json!({ "model": "gpt-5.5" }))
            .expect("managed route");
        let auth = adapter
            .extract_auth(&routed)
            .expect("managed route should use Codex OAuth auth strategy");

        assert_eq!(auth.strategy, AuthStrategy::CodexOAuth);
        assert_ne!(auth.api_key, "sk-stale-route-key");
        assert_eq!(routed.settings_config.get("apiKey"), None);
        assert_eq!(routed.settings_config.get("auth"), None);
    }

    #[test]
    fn test_codex_route_provider_config_auth_preserves_provider_key() {
        let adapter = CodexAdapter::new();
        let provider = create_provider(json!({
            "auth": {
                "OPENAI_API_KEY": "sk-provider-key"
            },
            "codexRouting": {
                "routes": [{
                    "id": "deepseek",
                    "match": { "models": ["deepseek-v4-flash"] },
                    "upstream": {
                        "baseUrl": "https://api.deepseek.example",
                        "apiFormat": "chat",
                        "auth": { "source": "provider_config" }
                    }
                }],
                "enabled": true
            }
        }));

        let routed = resolve_codex_model_routed_provider(
            &provider,
            &json!({ "model": "deepseek-v4-flash" }),
        )
        .expect("provider_config route");
        let auth = adapter
            .extract_auth(&routed)
            .expect("provider auth should remain usable");

        assert_eq!(auth.api_key, "sk-provider-key");
        assert_eq!(auth.strategy, AuthStrategy::Bearer);
        assert_eq!(
            routed.settings_config.get("auth"),
            provider.settings_config.get("auth")
        );
    }

    #[test]
    fn test_codex_route_provider_config_api_key_overrides_provider_key() {
        let adapter = CodexAdapter::new();
        let provider = create_provider(json!({
            "auth": {
                "OPENAI_API_KEY": "sk-provider-key"
            },
            "codexRouting": {
                "routes": [{
                    "id": "deepseek",
                    "match": { "models": ["deepseek-v4-flash"] },
                    "upstream": {
                        "baseUrl": "https://api.deepseek.example",
                        "apiFormat": "chat",
                        "auth": { "source": "provider_config" },
                        "apiKey": "sk-route-key"
                    }
                }],
                "enabled": true
            }
        }));

        let routed = resolve_codex_model_routed_provider(
            &provider,
            &json!({ "model": "deepseek-v4-flash" }),
        )
        .expect("provider_config route");
        let auth = adapter
            .extract_auth(&routed)
            .expect("route api key should be usable");

        assert_eq!(auth.api_key, "sk-route-key");
        assert_eq!(auth.strategy, AuthStrategy::Bearer);
    }

    #[test]
    fn test_codex_adapter_supports_routed_codex_oauth_provider() {
        let adapter = CodexAdapter::new();
        let mut provider = create_provider(json!({
            "codexModelRoutes": [
                {
                    "id": "openai",
                    "models": ["gpt-5.5"],
                    "wire_api": "openai_responses",
                    "providerType": "codex_oauth"
                }
            ]
        }));
        provider.meta = Some(ProviderMeta::default());

        let routed = resolve_codex_model_routed_provider(&provider, &json!({ "model": "gpt-5.5" }))
            .expect("openai route");
        let auth = adapter.extract_auth(&routed).expect("codex oauth auth");

        assert_eq!(
            adapter.extract_base_url(&routed).unwrap(),
            "https://chatgpt.com/backend-api/codex"
        );
        assert_eq!(auth.strategy, AuthStrategy::CodexOAuth);
        assert!(!should_convert_codex_responses_to_chat(
            &routed,
            "/responses"
        ));
    }

    #[test]
    fn test_extract_base_url_direct() {
        let adapter = CodexAdapter::new();
        let provider = create_provider(json!({
            "base_url": "https://api.openai.com/v1"
        }));

        let url = adapter.extract_base_url(&provider).unwrap();
        assert_eq!(url, "https://api.openai.com/v1");
    }

    #[test]
    fn test_extract_auth_from_auth_field() {
        let adapter = CodexAdapter::new();
        let provider = create_provider(json!({
            "auth": {
                "OPENAI_API_KEY": "sk-test-key-12345678"
            }
        }));

        let auth = adapter.extract_auth(&provider).unwrap();
        assert_eq!(auth.api_key, "sk-test-key-12345678");
        assert_eq!(auth.strategy, AuthStrategy::Bearer);
    }

    #[test]
    fn test_extract_auth_falls_back_to_config_bearer_when_auth_key_empty() {
        let adapter = CodexAdapter::new();
        let provider = create_provider(json!({
            "auth": {
                "OPENAI_API_KEY": ""
            },
            "config": r#"model_provider = "custom"

[model_providers.custom]
experimental_bearer_token = "sk-config-key"
"#
        }));

        let auth = adapter.extract_auth(&provider).unwrap();
        assert_eq!(auth.api_key, "sk-config-key");
        assert_eq!(auth.strategy, AuthStrategy::Bearer);
    }

    #[test]
    fn test_extract_auth_from_env() {
        let adapter = CodexAdapter::new();
        let provider = create_provider(json!({
            "env": {
                "OPENAI_API_KEY": "sk-env-key-12345678"
            }
        }));

        let auth = adapter.extract_auth(&provider).unwrap();
        assert_eq!(auth.api_key, "sk-env-key-12345678");
    }

    #[test]
    fn test_build_url() {
        let adapter = CodexAdapter::new();
        let url = adapter.build_url("https://api.openai.com/v1", "/responses");
        assert_eq!(url, "https://api.openai.com/v1/responses");
    }

    #[test]
    fn test_build_url_origin_adds_v1() {
        let adapter = CodexAdapter::new();
        let url = adapter.build_url("https://api.openai.com", "/responses");
        assert_eq!(url, "https://api.openai.com/v1/responses");
    }

    #[test]
    fn test_build_url_custom_prefix_no_v1() {
        let adapter = CodexAdapter::new();
        let url = adapter.build_url("https://example.com/openai", "/responses");
        assert_eq!(url, "https://example.com/openai/responses");
    }

    #[test]
    fn test_build_url_dedup_v1() {
        let adapter = CodexAdapter::new();
        // base_url 已包含 /v1，endpoint 也包含 /v1
        let url = adapter.build_url("https://www.packyapi.com/v1", "/v1/responses");
        assert_eq!(url, "https://www.packyapi.com/v1/responses");
    }

    // 官方客户端检测测试
    #[test]
    fn test_is_official_client_vscode() {
        assert!(CodexAdapter::is_official_client("codex_vscode/1.0.0"));
        assert!(CodexAdapter::is_official_client("codex_vscode/2.3.4"));
        assert!(CodexAdapter::is_official_client("codex_vscode/0.1"));
    }

    #[test]
    fn test_is_official_client_cli() {
        assert!(CodexAdapter::is_official_client("codex_cli_rs/1.0.0"));
        assert!(CodexAdapter::is_official_client("codex_cli_rs/0.5.2"));
    }

    #[test]
    fn test_is_not_official_client() {
        assert!(!CodexAdapter::is_official_client("Mozilla/5.0"));
        assert!(!CodexAdapter::is_official_client("curl/7.68.0"));
        assert!(!CodexAdapter::is_official_client("python-requests/2.25.1"));
        assert!(!CodexAdapter::is_official_client("codex_other/1.0.0"));
        assert!(!CodexAdapter::is_official_client(""));
    }

    #[test]
    fn test_is_official_client_partial_match() {
        // 必须从开头匹配
        assert!(!CodexAdapter::is_official_client("some codex_vscode/1.0.0"));
        assert!(!CodexAdapter::is_official_client(
            "prefix_codex_cli_rs/1.0.0"
        ));
    }

    #[test]
    fn test_codex_provider_uses_chat_completions_from_active_wire_api() {
        let provider = create_provider(json!({
            "config": r#"
model_provider = "chat_only"
model = "gpt-5"

[model_providers.chat_only]
name = "Chat Only"
base_url = "https://example.com/v1"
wire_api = "chat"
"#
        }));

        assert!(codex_provider_uses_chat_completions(&provider));
        assert!(should_convert_codex_responses_to_chat(
            &provider,
            "/responses?stream=true"
        ));
        assert!(!should_convert_codex_responses_to_chat(
            &provider,
            "/chat/completions"
        ));
    }

    #[test]
    fn test_codex_provider_uses_chat_completions_from_full_chat_url() {
        let provider = create_provider(json!({
            "base_url": "https://example.com/v1/chat/completions"
        }));

        assert!(codex_provider_uses_chat_completions(&provider));
        assert!(should_convert_codex_responses_to_chat(
            &provider,
            "/v1/responses/compact"
        ));
    }

    #[test]
    fn test_codex_provider_uses_chat_completions_from_meta_api_format_for_compact() {
        let mut provider = create_provider(json!({
            "base_url": "https://example.com/v1"
        }));
        provider.meta = Some(crate::provider::ProviderMeta {
            api_format: Some("openai_chat".to_string()),
            ..Default::default()
        });

        assert!(codex_provider_uses_chat_completions(&provider));
        assert!(should_convert_codex_responses_to_chat(
            &provider,
            "/responses/compact?stream=true"
        ));
    }

    #[test]
    fn test_codex_provider_uses_chat_completions_from_meta_api_format_for_responses() {
        let mut provider = create_provider(json!({
            "base_url": "https://api.deepseek.com/v1"
        }));
        provider.meta = Some(crate::provider::ProviderMeta {
            api_format: Some("openai_chat".to_string()),
            ..Default::default()
        });

        assert!(should_convert_codex_responses_to_chat(
            &provider,
            "/v1/responses"
        ));
    }

    #[test]
    fn test_apply_codex_chat_upstream_model_uses_provider_config_model() {
        let mut provider = create_provider(json!({
            "config": r#"
model_provider = "deepseek"
model = "deepseek-v4-flash"

[model_providers.deepseek]
name = "DeepSeek"
base_url = "https://api.deepseek.com/v1"
wire_api = "responses"
"#
        }));
        provider.meta = Some(crate::provider::ProviderMeta {
            api_format: Some("openai_chat".to_string()),
            ..Default::default()
        });
        let mut body = json!({
            "model": "placeholder-client-model",
            "input": "ping"
        });

        let upstream_model = apply_codex_chat_upstream_model(&provider, &mut body);

        assert_eq!(upstream_model.as_deref(), Some("deepseek-v4-flash"));
        assert_eq!(
            body.get("model").and_then(|v| v.as_str()),
            Some("deepseek-v4-flash")
        );
    }

    #[test]
    fn test_apply_codex_chat_upstream_model_preserves_catalog_model_selection() {
        let mut provider = create_provider(json!({
            "config": r#"
model_provider = "deepseek"
model = "deepseek-v4-flash"

[model_providers.deepseek]
name = "DeepSeek"
base_url = "https://api.deepseek.com/v1"
wire_api = "responses"
"#,
            "modelCatalog": {
                "models": [
                    { "model": "deepseek-v4-flash" },
                    { "model": "kimi-k2" }
                ]
            }
        }));
        provider.meta = Some(crate::provider::ProviderMeta {
            api_format: Some("openai_chat".to_string()),
            ..Default::default()
        });
        let mut body = json!({
            "model": "kimi-k2",
            "input": "ping"
        });

        let upstream_model = apply_codex_chat_upstream_model(&provider, &mut body);

        assert_eq!(upstream_model.as_deref(), Some("kimi-k2"));
        assert_eq!(body.get("model").and_then(|v| v.as_str()), Some("kimi-k2"));
    }

    #[test]
    fn test_resolve_codex_chat_reasoning_infers_deepseek_effort_support() {
        let provider = create_provider(json!({
            "config": r#"
model_provider = "deepseek"
model = "deepseek-v4-pro"

[model_providers.deepseek]
name = "DeepSeek"
base_url = "https://api.deepseek.com"
wire_api = "chat"
"#
        }));

        let config =
            resolve_codex_chat_reasoning_config(&provider, &json!({ "model": "deepseek-v4-pro" }))
                .unwrap();

        assert_eq!(config.supports_thinking, Some(true));
        assert_eq!(config.supports_effort, Some(true));
        assert_eq!(config.effort_value_mode.as_deref(), Some("deepseek"));
    }

    #[test]
    fn test_resolve_codex_chat_reasoning_explicit_meta_overrides_inference() {
        let mut provider = create_provider(json!({
            "config": r#"
model_provider = "deepseek"
model = "deepseek-v4-pro"

[model_providers.deepseek]
name = "DeepSeek"
base_url = "https://api.deepseek.com"
wire_api = "chat"
"#
        }));
        provider.meta = Some(crate::provider::ProviderMeta {
            codex_chat_reasoning: Some(CodexChatReasoningConfig {
                supports_thinking: Some(false),
                supports_effort: Some(false),
                thinking_param: Some("none".to_string()),
                effort_param: Some("none".to_string()),
                effort_value_mode: None,
                output_format: Some("auto".to_string()),
            }),
            ..Default::default()
        });

        let config =
            resolve_codex_chat_reasoning_config(&provider, &json!({ "model": "deepseek-v4-pro" }))
                .unwrap();

        assert_eq!(config.supports_thinking, Some(false));
        assert_eq!(config.supports_effort, Some(false));
        assert_eq!(config.thinking_param.as_deref(), Some("none"));
    }

    #[test]
    fn test_resolve_codex_chat_reasoning_openrouter_platform_overrides_model() {
        let provider = create_provider(json!({
            "config": r#"
model_provider = "openrouter"
model = "deepseek/deepseek-chat-v3.1"

[model_providers.openrouter]
name = "OpenRouter"
base_url = "https://openrouter.ai/api/v1"
wire_api = "chat"
"#
        }));

        // 模型名含 "deepseek"，但平台是 OpenRouter —— 平台规则必须覆盖模型规则。
        let config = resolve_codex_chat_reasoning_config(
            &provider,
            &json!({ "model": "deepseek/deepseek-chat-v3.1" }),
        )
        .unwrap();

        assert_eq!(config.thinking_param.as_deref(), Some("none"));
        assert_eq!(config.effort_param.as_deref(), Some("reasoning.effort"));
        assert_eq!(config.effort_value_mode.as_deref(), Some("openrouter"));
        assert_eq!(config.supports_effort, Some(true));
    }

    #[test]
    fn test_resolve_codex_chat_reasoning_siliconflow_platform_overrides_minimax() {
        let provider = create_provider(json!({
            "config": r#"
model_provider = "siliconflow"
model = "MiniMaxAI/MiniMax-M2.7"

[model_providers.siliconflow]
name = "SiliconFlow"
base_url = "https://api.siliconflow.cn/v1"
wire_api = "chat"
"#
        }));

        // 模型是 MiniMax（官方用 reasoning_split），但平台是 SiliconFlow —— 应走平台的 enable_thinking。
        let config = resolve_codex_chat_reasoning_config(
            &provider,
            &json!({ "model": "MiniMaxAI/MiniMax-M2.7" }),
        )
        .unwrap();

        assert_eq!(config.thinking_param.as_deref(), Some("enable_thinking"));
        assert_eq!(config.supports_effort, Some(false));
        assert_eq!(config.output_format.as_deref(), Some("reasoning_content"));
    }
}
