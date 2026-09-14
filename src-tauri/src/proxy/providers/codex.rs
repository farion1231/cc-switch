//! Codex (OpenAI) Provider Adapter
//!
//! 仅透传模式，支持直连 OpenAI API
//!
//! ## 客户端检测
//! 支持检测官方 Codex 客户端 (codex_vscode, codex_cli_rs)

use super::{AuthInfo, AuthStrategy, ProviderAdapter};
use crate::provider::{CodexChatReasoningConfig, Provider};
use crate::proxy::error::ProxyError;
use regex::Regex;
use serde_json::Value as JsonValue;
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

/// Whether a converted Codex Responses request may send `prompt_cache_key` to
/// its Chat Completions upstream. Unknown OpenAI-compatible gateways default to
/// false because many reject unsupported request fields with HTTP 400.
pub fn should_send_codex_chat_prompt_cache_key(provider: &Provider) -> bool {
    match provider
        .meta
        .as_ref()
        .and_then(|meta| meta.prompt_cache_routing.as_deref())
        .unwrap_or("auto")
    {
        "enabled" => return true,
        "disabled" => return false,
        _ => {}
    }

    let base_url = provider
        .settings_config
        .get("base_url")
        .or_else(|| provider.settings_config.get("baseURL"))
        .and_then(|value| value.as_str())
        .map(ToString::to_string)
        .or_else(|| {
            provider
                .settings_config
                .get("config")
                .and_then(|value| value.as_str())
                .and_then(extract_codex_base_url_from_toml)
        });

    let Some(base_url) = base_url else {
        return false;
    };
    let Ok(url) = url::Url::parse(&base_url) else {
        return false;
    };

    match url.host_str() {
        Some("api.openai.com") => true,
        Some("api.kimi.com") => {
            let path = url.path().trim_end_matches('/');
            path == "/coding" || path.starts_with("/coding/")
        }
        _ => false,
    }
}

/// Add a stable cache-routing key after Responses -> Chat conversion. An
/// explicit client key wins; otherwise only a real client-provided session ID
/// is eligible. Generated per-request UUIDs must never be used here.
pub fn inject_codex_chat_prompt_cache_key(
    provider: &Provider,
    chat_body: &mut JsonValue,
    explicit_key: Option<&str>,
    client_session_id: Option<&str>,
) -> bool {
    if !should_send_codex_chat_prompt_cache_key(provider) {
        return false;
    }

    let key = explicit_key
        .map(str::trim)
        .filter(|key| !key.is_empty())
        .or_else(|| {
            client_session_id
                .map(str::trim)
                .filter(|session_id| !session_id.is_empty())
        });
    let Some(key) = key else {
        return false;
    };

    chat_body["prompt_cache_key"] = JsonValue::String(key.to_string());
    true
}

/// Whether this Codex provider's real upstream speaks the native Anthropic
/// Messages protocol (`/v1/messages`). The local Codex client always talks to CC
/// Switch through the Responses API, so CC Switch bridges Responses ⇄ Anthropic.
///
/// Determined solely from explicit config (apiFormat / wire_api); no base_url
/// guessing — Anthropic gateway addresses vary widely and guessing easily misfires.
pub fn codex_provider_uses_anthropic(provider: &Provider) -> bool {
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
        return is_anthropic_wire_api(api_format);
    }

    provider
        .settings_config
        .get("config")
        .and_then(|v| v.as_str())
        .and_then(extract_codex_wire_api_from_toml)
        .map(|wire_api| is_anthropic_wire_api(&wire_api))
        .unwrap_or(false)
}

pub fn should_convert_codex_responses_to_anthropic(provider: &Provider, endpoint: &str) -> bool {
    let path = endpoint
        .split_once('?')
        .map_or(endpoint, |(path, _query)| path);

    matches!(
        path,
        "/responses" | "/v1/responses" | "/responses/compact" | "/v1/responses/compact"
    ) && codex_provider_uses_anthropic(provider)
}

/// Whether a native-Responses Codex upstream needs Codex `namespace`/plugin
/// tool declarations flattened before forwarding, plus xAI schema sanitization.
///
/// Codex 0.142+ emits ChatGPT-backend-private `{"type":"namespace",…}` tool
/// shapes that strict third-party Responses gateways reject with
/// `422 unknown variant "namespace"`. xAI also rejects root `oneOf`/`anyOf`
/// function schemas (notably `mcp__codex_app__automation_update`). The
/// Chat/Anthropic transform paths already unwrap namespaces, so this only
/// fires on native Responses passthrough.
///
/// Covers managed xAI OAuth *and* API-key providers whose live upstream is
/// `api.x.ai` with `wire_api = "responses"`. See farion1231/cc-switch#6815.
pub fn provider_needs_responses_namespace_flatten(provider: &Provider) -> bool {
    provider.is_xai_oauth() || provider_is_xai_native_responses(provider)
}

/// True when this Codex provider talks native Responses to first-party xAI
/// (`api.x.ai`), including API-key Grok cards that are not `xai_oauth`.
fn provider_is_xai_native_responses(provider: &Provider) -> bool {
    let config_text = provider
        .settings_config
        .get("config")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let Some(wire_api) = extract_codex_wire_api_from_toml(config_text) else {
        return false;
    };
    if !wire_api.eq_ignore_ascii_case("responses") {
        return false;
    }

    extract_codex_base_url_from_toml(config_text)
        .map(|url| url.to_ascii_lowercase())
        .is_some_and(|url| url.contains("api.x.ai"))
}

fn has_explicit_codex_third_party_upstream(provider: &Provider) -> bool {
    let non_empty_setting = |key: &str| {
        provider
            .settings_config
            .get(key)
            .and_then(JsonValue::as_str)
            .is_some_and(|value| !value.trim().is_empty())
    };
    let config = provider
        .settings_config
        .get("config")
        .and_then(JsonValue::as_str)
        .map(|text| {
            crate::codex_config::strip_codex_unified_session_bucket(text)
                .unwrap_or_else(|_| text.to_string())
        });
    let config = config.as_deref();

    ["baseUrl", "baseURL", "base_url"]
        .into_iter()
        .any(non_empty_setting)
        || config
            .and_then(crate::codex_config::extract_codex_experimental_bearer_token)
            .is_some()
        || config
            .and_then(crate::codex_config::extract_codex_base_url)
            .is_some()
        || config
            .and_then(|text| text.parse::<TomlValue>().ok())
            .and_then(|doc| {
                doc.get("model_provider")
                    .and_then(TomlValue::as_str)
                    .map(str::trim)
                    .filter(|provider_id| !provider_id.is_empty())
                    .map(str::to_string)
            })
            // Exact match, mirroring upstream: the built-in lookup is
            // case-sensitive, so `OpenAI` routes to a custom table — a
            // third-party upstream, not the official provider.
            .is_some_and(|provider_id| provider_id != "openai")
}

/// Codex Official ChatGPT cards receive authentication from the calling Codex
/// client (`requires_openai_auth = true`). Unbound cards with a stored API key
/// stay on the direct OpenAI API path instead of being sent to the ChatGPT
/// backend. The fixed legacy card keeps its existing behavior.
pub fn is_codex_official_provider(provider: &Provider) -> bool {
    let is_fixed_official_id = provider.id == crate::database::CODEX_OFFICIAL_PROVIDER_ID;
    if is_fixed_official_id && provider.category.as_deref() == Some("official") {
        return true;
    }

    let has_auth_object = provider
        .settings_config
        .get("auth")
        .is_some_and(JsonValue::is_object);
    let has_valid_config_shape = provider
        .settings_config
        .get("config")
        .is_none_or(|config| config.is_null() || config.is_string());
    if !has_auth_object || !has_valid_config_shape {
        return false;
    }

    if has_explicit_codex_third_party_upstream(provider) {
        return false;
    }

    let has_managed_account = provider
        .meta
        .as_ref()
        .and_then(|meta| meta.managed_account_id_for("codex_oauth"))
        .is_some_and(|account_id| !account_id.trim().is_empty());
    if has_managed_account {
        return true;
    }

    let has_stored_api_key = provider
        .settings_config
        .get("auth")
        .and_then(|auth| auth.get("OPENAI_API_KEY"))
        .and_then(JsonValue::as_str)
        .is_some_and(|key| !key.trim().is_empty());
    if has_stored_api_key {
        return false;
    }

    is_fixed_official_id || provider.category.as_deref() == Some("official")
}

/// Vendors whose OFFICIAL Codex integration is a native `/responses` gateway that
/// rejects Codex's freeform custom tools (`apply_patch` with `type: "custom"`,
/// #6944). Same vendor set as `CODEX_WEB_SEARCH_REJECT_HOSTS` in `codex_config`
/// (kept separate: that list also gates aggregators by model brand). Matched on
/// host labels via `codex_url_host_matches_any`, never by substring.
const CODEX_NATIVE_RESPONSES_HOSTS: &[&str] = &[
    "bigmodel.cn",
    "z.ai",
    "xiaomimimo.com",
    "minimaxi.com",
    "minimax.io",
    "longcat.chat",
];

/// Path markers of a listed vendor's OpenAI *Chat Completions* endpoint, which is
/// NOT its Responses endpoint. Zhipu documents three separate base URLs per site
/// (Anthropic `/api/anthropic`, Chat `/api/coding/paas/v4` + pay-as-you-go
/// `/api/paas/v4`, Responses `/api/v1`) and warns that the wrong one cannot use
/// Coding Plan quota. A stored provider still pointing at a Chat path is a
/// pre-2026-09 Chat-route record: it keeps its `ProxyChat` catalog (the proxy
/// route converts it correctly; direct connect fails loudly with the #6944 400
/// until the preset is re-imported) instead of being silently steered onto the
/// wrong endpoint with a native catalog.
const CODEX_NATIVE_RESPONSES_CHAT_PATH_MARKERS: &[&str] = &["/paas/v4"];

/// Whether `base_url` points at a listed vendor's native Responses gateway, so a
/// provider whose stored `apiFormat` predates the preset's switch to
/// `openai_responses` still gets the `NativeResponses` catalog without a re-save.
pub fn is_codex_native_responses_url(base_url: &str) -> bool {
    if !crate::codex_config::codex_url_host_matches_any(base_url, CODEX_NATIVE_RESPONSES_HOSTS) {
        return false;
    }
    if is_chat_completions_url(base_url) {
        return false;
    }
    let lower = base_url.to_ascii_lowercase();
    !CODEX_NATIVE_RESPONSES_CHAT_PATH_MARKERS
        .iter()
        .any(|marker| lower.contains(marker))
}

/// Resolve the model-catalog tool profile for a Codex provider using the SAME
/// Anthropic detection as the proxy router ([`codex_provider_uses_anthropic`]), so the
/// generated catalog never disagrees with the routed transform. A provider whose
/// Anthropic upstream is declared only via settings `apiFormat` or TOML `wire_api`
/// (not `meta.api_format`) would otherwise get a `ProxyChat` catalog and emit the
/// freeform `apply_patch` tool that the Anthropic transform then silently drops.
/// Non-Anthropic providers keep the existing `meta.api_format` classification.
pub fn resolve_codex_catalog_tool_profile(
    provider: &Provider,
) -> crate::codex_config::CodexCatalogToolProfile {
    use crate::codex_config::CodexCatalogToolProfile;
    if is_codex_official_provider(provider) {
        return CodexCatalogToolProfile::NativeResponses;
    }
    // xAI OAuth pins the native Responses profile regardless of editable
    // api_format, mirroring the Claude-side managed-provider invariant.
    if provider.is_xai_oauth() {
        return CodexCatalogToolProfile::NativeResponses;
    }
    if codex_provider_uses_anthropic(provider) {
        return CodexCatalogToolProfile::Anthropic;
    }

    // Defensive fallback for providers saved in SQLite before their preset
    // switched to `openai_responses` (the #6944 reporter reinstalled to no
    // effect precisely because the stale `apiFormat` lives in the DB row): a
    // base_url on a listed vendor's native Responses gateway forces the
    // NativeResponses catalog. Chat-endpoint paths are deliberately excluded —
    // see `CODEX_NATIVE_RESPONSES_CHAT_PATH_MARKERS`.
    if let Some(base_url) = provider
        .settings_config
        .get("config")
        .and_then(|v| v.as_str())
        .and_then(extract_codex_base_url_from_toml)
        .or_else(|| {
            provider
                .settings_config
                .get("base_url")
                .or_else(|| provider.settings_config.get("baseURL"))
                .and_then(|v| v.as_str())
                .map(ToString::to_string)
        })
    {
        if is_codex_native_responses_url(&base_url) {
            return CodexCatalogToolProfile::NativeResponses;
        }
    }

    let api_format = provider
        .meta
        .as_ref()
        .and_then(|m| m.api_format.as_deref())
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
        });
    CodexCatalogToolProfile::from_api_format(api_format)
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
                .and_then(|config| {
                    crate::grok_config::extract_model_config(config)
                        .map(|model| model.model)
                        .or_else(|| extract_codex_model_from_toml(config))
                })
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
    apply_codex_upstream_model(provider, body)
}

/// Same model-substitution logic as `apply_codex_chat_upstream_model`, but without
/// the chat gating check. Reused by the anthropic conversion path (the forwarder has
/// already confirmed this provider uses anthropic).
pub fn apply_codex_upstream_model(provider: &Provider, body: &mut JsonValue) -> Option<String> {
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
    let mut config = if let Some(config) = provider
        .meta
        .as_ref()
        .and_then(|meta| meta.codex_chat_reasoning.clone())
    {
        normalize_codex_chat_reasoning_config(config)
    } else {
        infer_codex_chat_reasoning_config(provider, body)?
    };

    // zen 的合法 effort 档位是逐模型的（models.dev：glm-5.2 仅 high|max、
    // kimi-k3 仅 max、qwen/glm-5.1 等为 toggle 型无 effort），opencode 客户端
    // 也严格按模型声明发值。按请求模型从 modelCatalog 的 reasoningLevels
    // （#6228 引入的逐模型声明）查表附上；查不到（模型未收录 / 条目未声明
    // effort）→ None，转换层将完全不发 reasoning_effort。
    if config.effort_value_mode.as_deref() == Some("zen") {
        config.effort_levels = zen_catalog_effort_levels(provider, body);
    }

    Some(config)
}

/// 按请求模型从供应商 modelCatalog 查 Zen 合法 effort 档位（逐模型数据镜像
/// models.dev 的 reasoning_options effort values）。仅做档位查表，不参与平台
/// 判定——平台身份仍只由 name/base_url 决定（见 infer_aggregator_platform_config）。
/// DB SSOT 为 camelCase，手写/旧数据可能为 snake_case，双格式兼容（与表单加载侧一致）。
fn zen_catalog_effort_levels(provider: &Provider, body: &JsonValue) -> Option<Vec<String>> {
    let model = body.get("model")?.as_str()?.trim();
    if model.is_empty() {
        return None;
    }
    let entries = provider
        .settings_config
        .get("modelCatalog")?
        .get("models")?
        .as_array()?;
    let entry = entries.iter().find(|entry| {
        entry
            .get("model")
            .and_then(|value| value.as_str())
            .is_some_and(|name| name.eq_ignore_ascii_case(model))
    })?;
    let levels_value = entry
        .get("reasoningLevels")
        .or_else(|| entry.get("reasoning_levels"))?;
    let levels: Vec<String> = levels_value
        .as_array()?
        .iter()
        .filter_map(|level| level.as_str().map(str::to_string))
        .collect();
    (!levels.is_empty()).then_some(levels)
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
            effort_levels: None,
        });
    }

    // StepFun：官方 reasoning 指南与两站模型页（2026-08-15 盘点）——
    // step-3.5-flash-2603 支持 low/high 两档；step-3.7-flash 支持
    // low/medium/high 三档（官方默认 medium）；其余 step 模型（含无后缀
    // step-3.5-flash）不暴露 effort。2603 沿用 low_high 收敛映射；
    // 3.7-flash 必须 passthrough——套 low_high 会把 medium 塌成 high，
    // 造出 wire 上无差异的假档位。全系无思考开关（thinking_param 恒 none）。
    // 第二个 OR 分支覆盖「经中转/聚合跑该模型、但平台 name/base_url 不含 stepfun」的情况。
    if haystack.contains("stepfun") || haystack.contains("step-3.5-flash-2603") {
        return Some(CodexChatReasoningConfig {
            supports_thinking: Some(true),
            supports_effort: Some(model.contains("2603") || model.contains("step-3.7-flash")),
            thinking_param: Some("none".to_string()),
            effort_param: Some("reasoning_effort".to_string()),
            effort_value_mode: Some(
                if model.contains("2603") {
                    "low_high"
                } else {
                    "passthrough"
                }
                .to_string(),
            ),
            output_format: Some("reasoning".to_string()),
            effort_levels: None,
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
            effort_levels: None,
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
            effort_levels: None,
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
            effort_levels: None,
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
            effort_levels: None,
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
            effort_levels: None,
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
            effort_levels: None,
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
            effort_levels: None,
        });
    }

    // ModelScope 魔搭 API-Inference：与 SiliconFlow 同构——平台级统一
    // `enable_thinking` 布尔（官方模型页范例 extra_body {"enable_thinking": bool}，
    // OpenAI SDK 的 extra_body 合并进请求体顶层），思维回传 reasoning_content。
    // 智谱风格 thinking:{type} 是模型厂商自家方言，平台文档零出现——没有这条
    // 分支时挂 GLM 的 ModelScope 供应商会被下方 glm 模型规则错误注入该形态。
    if platform.contains("modelscope") {
        return Some(CodexChatReasoningConfig {
            supports_thinking: Some(true),
            supports_effort: Some(false),
            thinking_param: Some("enable_thinking".to_string()),
            effort_param: Some("none".to_string()),
            effort_value_mode: None,
            output_format: Some("reasoning_content".to_string()),
            effort_levels: None,
        });
    }

    // OpenCode Zen（opencode.ai 网关，issue #6112）：其自家客户端对该传输发顶层
    // `reasoning_effort`（provider/transform.ts），平台归一参数；不发厂商原生
    // thinking 形状（glm 模型走 zen 时套智谱 thinking:{type} 网关不认）。
    // 合法档位逐模型（models.dev 的 reasoning_options，opencode 客户端同样严格
    // 按模型声明发值）：具体档位表见供应商 modelCatalog 各条目的 reasoningLevels，
    // 代理由此按请求模型查表钳制（resolve 处附上 effort_levels），无表不发字段。
    // 匹配域名而非裸 "opencode"，避免误伤名字含 opencode 的无关供应商。
    if platform.contains("opencode.ai") {
        return Some(CodexChatReasoningConfig {
            supports_thinking: Some(true),
            supports_effort: Some(true),
            thinking_param: Some("none".to_string()),
            effort_param: Some("reasoning_effort".to_string()),
            effort_value_mode: Some("zen".to_string()),
            output_format: Some("reasoning_content".to_string()),
            effort_levels: None,
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

fn is_anthropic_wire_api(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "anthropic" | "anthropic_messages" | "anthropic-messages" | "claude" | "messages"
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
                if let Some((_, key)) = crate::grok_config::extract_credentials(config_str) {
                    return Some(key);
                }
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
        if is_codex_official_provider(provider) {
            return Ok(super::CHATGPT_CODEX_BASE_URL.to_string());
        }

        // xAI OAuth: ignore editable provider base URLs and always use the xAI
        // API origin associated with the managed token.
        if provider.is_xai_oauth() {
            return Ok(super::XAI_API_BASE_URL.to_string());
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
                if let Some(url) = crate::grok_config::extract_base_url(config_str) {
                    return Ok(url.trim_end_matches('/').to_string());
                }
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
        // xAI OAuth (Grok subscription): placeholder credentials only; the real
        // access_token is resolved per-request by the forwarder via XaiOAuthManager.
        if provider.is_xai_oauth() {
            return Some(AuthInfo::new(
                "xai_oauth_placeholder".to_string(),
                AuthStrategy::XaiOAuth,
            ));
        }

        // Anthropic upstream: the auth field is chosen by the user in the UI (meta.apiKeyField).
        //   ANTHROPIC_API_KEY    → x-api-key (AuthStrategy::Anthropic)
        //   ANTHROPIC_AUTH_TOKEN → Authorization: Bearer (default, AuthStrategy::Bearer)
        // The two are mutually exclusive to avoid a 401 from the gateway receiving
        // both auth headers at once. All other Codex upstreams stay pure Bearer.
        let strategy = if codex_provider_uses_anthropic(provider) {
            let uses_x_api_key = provider
                .meta
                .as_ref()
                .and_then(|meta| meta.api_key_field.as_deref())
                .map(|field| field.eq_ignore_ascii_case("ANTHROPIC_API_KEY"))
                .unwrap_or(false);
            if uses_x_api_key {
                AuthStrategy::Anthropic
            } else {
                AuthStrategy::Bearer
            }
        } else {
            AuthStrategy::Bearer
        };
        self.extract_key(provider)
            .map(|key| AuthInfo::new(key, strategy))
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
        // Anthropic gateway: send only x-api-key (anthropic-version is filled in by
        // the forwarder). Mutually exclusive with Bearer to avoid a 401 from the
        // gateway receiving both auth headers at once.
        if auth.strategy == AuthStrategy::Anthropic {
            return Ok(vec![(
                http::HeaderName::from_static("x-api-key"),
                auth_header_value(&auth.api_key)?,
            )]);
        }
        Ok(vec![(
            http::HeaderName::from_static("authorization"),
            auth_header_value(&bearer)?,
        )])
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
    fn grok_build_toml_exposes_upstream_credentials_and_model() {
        let adapter = CodexAdapter::new();
        let provider = create_provider(json!({
            "config": r#"
[models]
default = "grok-4.5"

[model."grok-4.5"]
model = "upstream-grok-model"
base_url = "https://relay.example.com/v1/"
name = "Example Relay"
api_key = "grok-secret"
api_backend = "responses"
context_window = 500000
"#
        }));

        assert_eq!(
            adapter.extract_base_url(&provider).unwrap(),
            "https://relay.example.com/v1"
        );
        let auth = adapter.extract_auth(&provider).unwrap();
        assert_eq!(auth.api_key, "grok-secret");
        assert_eq!(auth.strategy, AuthStrategy::Bearer);
        assert_eq!(
            codex_provider_upstream_model(&provider).as_deref(),
            Some("upstream-grok-model")
        );
    }

    #[test]
    fn explicit_codex_official_cards_use_chatgpt_backend() {
        let mut provider = create_provider(json!({
            "auth": {
                "auth_mode": "chatgpt",
                "OPENAI_API_KEY": null,
                "tokens": { "refresh_token": "legacy-live-only-token" }
            },
            "config": ""
        }));
        provider.id = "unbound-official-account".to_string();
        provider.category = Some("official".to_string());
        assert!(is_codex_official_provider(&provider));

        let mut native = provider.clone();
        native.id = crate::database::CODEX_OFFICIAL_PROVIDER_ID.to_string();
        assert!(is_codex_official_provider(&native));

        provider.id = "managed-official-account".to_string();
        provider.meta = Some(crate::provider::ProviderMeta {
            provider_type: Some("codex_oauth".to_string()),
            auth_binding: Some(crate::provider::AuthBinding {
                source: crate::provider::AuthBindingSource::ManagedAccount,
                auth_provider: Some("codex_oauth".to_string()),
                account_id: Some("acct-managed".to_string()),
            }),
            ..Default::default()
        });
        let adapter = CodexAdapter::new();

        assert!(is_codex_official_provider(&provider));
        assert_eq!(
            adapter
                .extract_base_url(&provider)
                .expect("official base url"),
            "https://chatgpt.com/backend-api/codex"
        );
        assert!(adapter.extract_auth(&provider).is_none());
        assert_eq!(
            adapter.build_url(
                "https://chatgpt.com/backend-api/codex",
                "/responses/compact"
            ),
            "https://chatgpt.com/backend-api/codex/responses/compact"
        );

        let mut official_api_key = create_provider(json!({
            "auth": { "OPENAI_API_KEY": "sk-official" },
            "config": ""
        }));
        official_api_key.category = Some("official".to_string());
        assert!(!is_codex_official_provider(&official_api_key));

        let mut stored_bearer = create_provider(json!({
            "auth": {},
            "config": "experimental_bearer_token = \"sk-legacy\""
        }));
        stored_bearer.category = Some("official".to_string());
        assert!(!is_codex_official_provider(&stored_bearer));

        let mut unmarked_custom = create_provider(json!({
            "auth": {},
            "config": "model_provider = \"custom\"\n[model_providers.custom]\nbase_url = \"https://example.com/v1\""
        }));
        unmarked_custom.category = Some("official".to_string());
        assert!(!is_codex_official_provider(&unmarked_custom));

        let mut category_less_fixed_custom = unmarked_custom.clone();
        category_less_fixed_custom.id = crate::database::CODEX_OFFICIAL_PROVIDER_ID.to_string();
        category_less_fixed_custom.category = None;
        assert!(!is_codex_official_provider(&category_less_fixed_custom));

        let mut category_less_fixed_api_key = official_api_key.clone();
        category_less_fixed_api_key.id = crate::database::CODEX_OFFICIAL_PROVIDER_ID.to_string();
        category_less_fixed_api_key.category = None;
        assert!(!is_codex_official_provider(&category_less_fixed_api_key));

        let mut explicit_openai = create_provider(json!({
            "auth": { "OPENAI_API_KEY": "sk-official" },
            "config": "model_provider = \"openai\""
        }));
        explicit_openai.category = Some("official".to_string());
        assert!(!is_codex_official_provider(&explicit_openai));

        let mut managed_with_null_config = provider.clone();
        managed_with_null_config.category = None;
        managed_with_null_config.settings_config["config"] = JsonValue::Null;
        assert!(is_codex_official_provider(&managed_with_null_config));

        let mut unified_session = create_provider(json!({
            "auth": {},
            "config": crate::codex_config::inject_codex_unified_session_bucket("")
                .expect("inject unified session route")
        }));
        unified_session.category = Some("official".to_string());
        assert!(is_codex_official_provider(&unified_session));

        let mut implicit_custom = create_provider(json!({
            "auth": {},
            "config": "model_provider = \"ollama\""
        }));
        implicit_custom.category = Some("official".to_string());
        assert!(!is_codex_official_provider(&implicit_custom));

        let mut grok_official = create_provider(json!({ "config": "" }));
        grok_official.id = crate::database::GROKBUILD_OFFICIAL_PROVIDER_ID.to_string();
        grok_official.category = Some("official".to_string());
        assert!(!is_codex_official_provider(&grok_official));
    }

    #[test]
    fn prompt_cache_routing_auto_enables_known_upstreams_only() {
        let kimi = create_provider(json!({
            "config": r#"
model_provider = "custom"
[model_providers.custom]
base_url = "https://api.kimi.com/coding/v1"
wire_api = "responses"
"#
        }));
        let openai = create_provider(json!({
            "base_url": "https://api.openai.com/v1"
        }));
        let unknown = create_provider(json!({
            "base_url": "https://strict.example.com/v1"
        }));

        assert!(should_send_codex_chat_prompt_cache_key(&kimi));
        assert!(should_send_codex_chat_prompt_cache_key(&openai));
        assert!(!should_send_codex_chat_prompt_cache_key(&unknown));
    }

    #[test]
    fn prompt_cache_routing_user_override_wins_over_auto_detection() {
        let mut kimi = create_provider(json!({
            "base_url": "https://api.kimi.com/coding/v1"
        }));
        kimi.meta = Some(crate::provider::ProviderMeta {
            prompt_cache_routing: Some("disabled".to_string()),
            ..Default::default()
        });
        assert!(!should_send_codex_chat_prompt_cache_key(&kimi));

        let mut unknown = create_provider(json!({
            "base_url": "https://strict.example.com/v1"
        }));
        unknown.meta = Some(crate::provider::ProviderMeta {
            prompt_cache_routing: Some("enabled".to_string()),
            ..Default::default()
        });
        assert!(should_send_codex_chat_prompt_cache_key(&unknown));
    }

    #[test]
    fn prompt_cache_key_prefers_explicit_key_then_real_session() {
        let provider = create_provider(json!({
            "base_url": "https://api.kimi.com/coding/v1"
        }));
        let mut explicit_body = json!({ "model": "kimi-for-coding" });
        assert!(inject_codex_chat_prompt_cache_key(
            &provider,
            &mut explicit_body,
            Some("request-key"),
            Some("session-key"),
        ));
        assert_eq!(explicit_body["prompt_cache_key"], "request-key");

        let mut session_body = json!({ "model": "kimi-for-coding" });
        assert!(inject_codex_chat_prompt_cache_key(
            &provider,
            &mut session_body,
            None,
            Some("session-key"),
        ));
        assert_eq!(session_body["prompt_cache_key"], "session-key");
    }

    #[test]
    fn prompt_cache_key_is_not_injected_without_real_session_or_support() {
        let kimi = create_provider(json!({
            "base_url": "https://api.kimi.com/coding/v1"
        }));
        let mut no_session_body = json!({ "model": "kimi-for-coding" });
        assert!(!inject_codex_chat_prompt_cache_key(
            &kimi,
            &mut no_session_body,
            None,
            None,
        ));
        assert!(no_session_body.get("prompt_cache_key").is_none());

        let unknown = create_provider(json!({
            "base_url": "https://strict.example.com/v1"
        }));
        let mut unsupported_body = json!({ "model": "other" });
        assert!(!inject_codex_chat_prompt_cache_key(
            &unknown,
            &mut unsupported_body,
            Some("request-key"),
            Some("session-key"),
        ));
        assert!(unsupported_body.get("prompt_cache_key").is_none());
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

    // ==================== anthropic upstream detection ====================

    #[test]
    fn test_uses_anthropic_from_settings_api_format() {
        let provider = create_provider(json!({ "apiFormat": "anthropic" }));
        assert!(codex_provider_uses_anthropic(&provider));

        let provider = create_provider(json!({ "api_format": "anthropic_messages" }));
        assert!(codex_provider_uses_anthropic(&provider));
    }

    #[test]
    fn test_uses_anthropic_from_meta_api_format() {
        let mut provider = create_provider(json!({}));
        provider.meta = Some(crate::provider::ProviderMeta {
            api_format: Some("anthropic".to_string()),
            ..Default::default()
        });
        assert!(codex_provider_uses_anthropic(&provider));
    }

    #[test]
    fn test_uses_anthropic_from_toml_wire_api() {
        let provider = create_provider(json!({
            "config": r#"model_provider = "custom"

[model_providers.custom]
wire_api = "anthropic"
"#
        }));
        assert!(codex_provider_uses_anthropic(&provider));
    }

    #[test]
    fn test_anthropic_false_for_chat_and_responses() {
        let chat = create_provider(json!({ "apiFormat": "openai_chat" }));
        assert!(!codex_provider_uses_anthropic(&chat));
        let responses = create_provider(json!({ "apiFormat": "openai_responses" }));
        assert!(!codex_provider_uses_anthropic(&responses));
    }

    #[test]
    fn test_anthropic_and_chat_are_mutually_exclusive() {
        let anth = create_provider(json!({ "apiFormat": "anthropic" }));
        assert!(codex_provider_uses_anthropic(&anth));
        assert!(!codex_provider_uses_chat_completions(&anth));

        let chat = create_provider(json!({ "apiFormat": "openai_chat" }));
        assert!(codex_provider_uses_chat_completions(&chat));
        assert!(!codex_provider_uses_anthropic(&chat));
    }

    #[test]
    fn test_should_convert_responses_to_anthropic_path_guard() {
        let provider = create_provider(json!({ "apiFormat": "anthropic" }));
        assert!(should_convert_codex_responses_to_anthropic(
            &provider,
            "/responses"
        ));
        assert!(should_convert_codex_responses_to_anthropic(
            &provider,
            "/v1/responses/compact"
        ));
        assert!(should_convert_codex_responses_to_anthropic(
            &provider,
            "/responses?x=1"
        ));
        assert!(!should_convert_codex_responses_to_anthropic(
            &provider,
            "/chat/completions"
        ));
    }

    #[test]
    fn test_resolve_catalog_profile_matches_router() {
        use crate::codex_config::CodexCatalogToolProfile;

        // Anthropic declared only via TOML wire_api (no meta.api_format) must still
        // resolve to the Anthropic catalog profile — this is the routing/catalog
        // divergence that let apply_patch leak through.
        let toml_anthropic = create_provider(json!({
            "config": r#"model_provider = "custom"

[model_providers.custom]
wire_api = "anthropic"
"#
        }));
        assert_eq!(
            resolve_codex_catalog_tool_profile(&toml_anthropic),
            CodexCatalogToolProfile::Anthropic
        );

        // Anthropic via settings apiFormat.
        let settings_anthropic = create_provider(json!({ "apiFormat": "anthropic" }));
        assert_eq!(
            resolve_codex_catalog_tool_profile(&settings_anthropic),
            CodexCatalogToolProfile::Anthropic
        );

        // Native openai_responses (meta) → NativeResponses; chat → ProxyChat.
        let mut native = create_provider(json!({}));
        native.meta = Some(crate::provider::ProviderMeta {
            api_format: Some("openai_responses".to_string()),
            ..Default::default()
        });
        assert_eq!(
            resolve_codex_catalog_tool_profile(&native),
            CodexCatalogToolProfile::NativeResponses
        );

        let chat = create_provider(json!({ "apiFormat": "openai_chat" }));
        assert_eq!(
            resolve_codex_catalog_tool_profile(&chat),
            CodexCatalogToolProfile::ProxyChat
        );

        // Host fallback (#6944): a DB row saved while the Zhipu preset was still
        // `openai_chat` but whose base_url is the vendor's native Responses
        // gateway (`/api/v1`) resolves to NativeResponses without a re-save —
        // whether the URL lives in the TOML or in settings `baseURL`.
        let legacy_chat_toml = |base_url: &str| {
            create_provider(json!({
                "apiFormat": "openai_chat",
                "config": format!(
                    "model_provider = \"custom\"\n\n[model_providers.custom]\nname = \"zhipu_glm\"\nbase_url = \"{base_url}\"\nwire_api = \"responses\"\n"
                )
            }))
        };
        for base_url in [
            "https://open.bigmodel.cn/api/v1",
            "https://api.z.ai/api/v1",
            "https://Open.BigModel.cn/api/v1/",
        ] {
            assert_eq!(
                resolve_codex_catalog_tool_profile(&legacy_chat_toml(base_url)),
                CodexCatalogToolProfile::NativeResponses,
                "{base_url}"
            );
        }
        let legacy_chat_settings = create_provider(json!({
            "apiFormat": "openai_chat",
            "baseURL": "https://api.z.ai/api/v1"
        }));
        assert_eq!(
            resolve_codex_catalog_tool_profile(&legacy_chat_settings),
            CodexCatalogToolProfile::NativeResponses
        );

        // The vendor's Chat Completions endpoints are NOT its Responses gateway
        // (Zhipu: `/api/coding/paas/v4` Coding Plan, `/api/paas/v4` pay-as-you-go)
        // — a stale row there keeps ProxyChat so it is never steered onto the
        // endpoint Zhipu documents as unable to use Coding Plan quota.
        for base_url in [
            "https://open.bigmodel.cn/api/coding/paas/v4",
            "https://api.z.ai/api/coding/paas/v4",
            "https://open.bigmodel.cn/api/paas/v4",
            "https://open.bigmodel.cn/api/paas/v4/chat/completions",
        ] {
            assert_eq!(
                resolve_codex_catalog_tool_profile(&legacy_chat_toml(base_url)),
                CodexCatalogToolProfile::ProxyChat,
                "{base_url}"
            );
        }

        // Host matching is on DNS labels: `xyz.ai` must not be captured by the
        // 4-char `z.ai` entry (it would lose apply_patch and web_search).
        for base_url in [
            "https://api.xyz.ai/v1",
            "https://viz.ai/v1",
            "https://z.ai.example.com/v1",
        ] {
            assert_eq!(
                resolve_codex_catalog_tool_profile(&legacy_chat_toml(base_url)),
                CodexCatalogToolProfile::ProxyChat,
                "{base_url}"
            );
        }

        // An explicit `openai_responses` on an unlisted host is untouched by the
        // fallback (it only ever widens toward NativeResponses for listed hosts).
        let explicit_native = create_provider(json!({
            "apiFormat": "openai_responses",
            "baseURL": "https://api.xyz.ai/v1"
        }));
        assert_eq!(
            resolve_codex_catalog_tool_profile(&explicit_native),
            CodexCatalogToolProfile::NativeResponses
        );
    }

    #[test]
    fn native_responses_url_guard_matches_gateway_not_chat_paths() {
        for url in [
            "https://open.bigmodel.cn/api/v1",
            "https://api.z.ai/api/v1",
            "https://api.xiaomimimo.com/v1",
            "https://token-plan-cn.xiaomimimo.com/v1",
            "https://api.minimaxi.com/v1",
            "https://api.minimax.io/v1",
            "https://api.longcat.chat/openai/v1",
        ] {
            assert!(is_codex_native_responses_url(url), "{url}");
        }
        for url in [
            "https://open.bigmodel.cn/api/coding/paas/v4",
            "https://api.z.ai/api/coding/paas/v4",
            "https://open.bigmodel.cn/api/paas/v4",
            "https://api.minimaxi.com/v1/chat/completions",
            "https://api.xyz.ai/v1",
            "https://api.deepseek.com",
            "",
        ] {
            assert!(!is_codex_native_responses_url(url), "{url}");
        }
    }

    #[test]
    fn test_apply_codex_upstream_model_preserves_one_m_catalog_model() {
        // Regression for the [1m] path: a request model carrying the [1m] marker must
        // match its catalog entry and be preserved (not overridden by the provider
        // default) so the transform can later strip [1m] and emit the context-1m beta.
        // This only works because the forwarder no longer strips [1m] before this call
        // on the Anthropic path.
        let provider = create_provider(json!({
            "config": r#"model_provider = "custom"
model = "claude-opus-4-1"

[model_providers.custom]
wire_api = "anthropic"
"#,
            "modelCatalog": {
                "models": [
                    { "model": "claude-opus-4-1[1m]" }
                ]
            }
        }));
        let mut body = json!({ "model": "claude-opus-4-1[1m]", "input": "hi" });
        let result = apply_codex_upstream_model(&provider, &mut body);
        assert_eq!(result.as_deref(), Some("claude-opus-4-1[1m]"));
        assert_eq!(
            body.get("model").and_then(|v| v.as_str()),
            Some("claude-opus-4-1[1m]")
        );
    }

    #[test]
    fn test_anthropic_auth_defaults_to_bearer() {
        // No meta.apiKeyField (defaults to ANTHROPIC_AUTH_TOKEN) → Authorization: Bearer only
        let adapter = CodexAdapter::new();
        let provider = create_provider(json!({
            "apiFormat": "anthropic",
            "auth": { "OPENAI_API_KEY": "sk-anthropic-key-123" }
        }));

        let auth = adapter.extract_auth(&provider).unwrap();
        assert_eq!(auth.strategy, AuthStrategy::Bearer);

        let headers = adapter.get_auth_headers(&auth).unwrap();
        let names: Vec<String> = headers
            .iter()
            .map(|(name, _)| name.as_str().to_string())
            .collect();
        assert_eq!(names, vec!["authorization".to_string()]);
    }

    #[test]
    fn test_anthropic_auth_x_api_key_when_selected() {
        // meta.apiKeyField = ANTHROPIC_API_KEY → x-api-key only
        let adapter = CodexAdapter::new();
        let mut provider = create_provider(json!({
            "apiFormat": "anthropic",
            "auth": { "OPENAI_API_KEY": "sk-anthropic-key-123" }
        }));
        provider.meta = Some(crate::provider::ProviderMeta {
            api_format: Some("anthropic".to_string()),
            api_key_field: Some("ANTHROPIC_API_KEY".to_string()),
            ..Default::default()
        });

        let auth = adapter.extract_auth(&provider).unwrap();
        assert_eq!(auth.strategy, AuthStrategy::Anthropic);

        let headers = adapter.get_auth_headers(&auth).unwrap();
        let names: Vec<String> = headers
            .iter()
            .map(|(name, _)| name.as_str().to_string())
            .collect();
        assert_eq!(names, vec!["x-api-key".to_string()]);
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
                effort_levels: None,
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
model = "MiniMaxAI/MiniMax-M2.5"

[model_providers.siliconflow]
name = "SiliconFlow"
base_url = "https://api.siliconflow.cn/v1"
wire_api = "chat"
"#
        }));

        // 模型是 MiniMax（官方用 reasoning_split），但平台是 SiliconFlow —— 应走平台的 enable_thinking。
        let config = resolve_codex_chat_reasoning_config(
            &provider,
            &json!({ "model": "MiniMaxAI/MiniMax-M2.5" }),
        )
        .unwrap();

        assert_eq!(config.thinking_param.as_deref(), Some("enable_thinking"));
        assert_eq!(config.supports_effort, Some(false));
        assert_eq!(config.output_format.as_deref(), Some("reasoning_content"));
    }

    #[test]
    fn test_resolve_codex_chat_reasoning_modelscope_platform_overrides_glm() {
        let provider = create_provider(json!({
            "config": r#"
model_provider = "modelscope"
model = "ZhipuAI/GLM-5.2"

[model_providers.modelscope]
name = "ModelScope"
base_url = "https://api-inference.modelscope.cn/v1"
wire_api = "chat"
"#
        }));

        // 模型是 GLM（智谱自家用 thinking:{type}），但平台是 ModelScope ——
        // 应走平台级 enable_thinking，而不是被 glm 模型规则注入智谱方言。
        let config =
            resolve_codex_chat_reasoning_config(&provider, &json!({ "model": "ZhipuAI/GLM-5.2" }))
                .unwrap();

        assert_eq!(config.thinking_param.as_deref(), Some("enable_thinking"));
        assert_eq!(config.supports_effort, Some(false));
        assert_eq!(config.output_format.as_deref(), Some("reasoning_content"));
    }

    #[test]
    fn test_resolve_codex_chat_reasoning_opencode_zen_platform_overrides_model_vendor() {
        let provider = create_provider(json!({
            "config": r#"
model_provider = "opencode"
model = "glm-5.2"

[model_providers.opencode]
name = "OpenCode Go"
base_url = "https://opencode.ai/zen/go/v1"
wire_api = "chat"
"#
        }));

        // 模型是 GLM（智谱自家用 thinking:{type} 方言），但平台是 OpenCode Zen ——
        // 必须走平台配置：顶层 reasoning_effort + reasoning_content，
        // 不得注入智谱 thinking 形状。
        let config =
            resolve_codex_chat_reasoning_config(&provider, &json!({ "model": "glm-5.2" })).unwrap();

        assert_eq!(config.supports_thinking, Some(true));
        assert_eq!(config.supports_effort, Some(true));
        assert_eq!(config.thinking_param.as_deref(), Some("none"));
        assert_eq!(config.effort_param.as_deref(), Some("reasoning_effort"));
        assert_eq!(config.effort_value_mode.as_deref(), Some("zen"));
        assert_eq!(config.output_format.as_deref(), Some("reasoning_content"));
    }

    #[test]
    fn test_resolve_codex_chat_reasoning_zen_attaches_per_model_effort_levels() {
        let provider = create_provider(json!({
            "config": r#"
model_provider = "opencode"
model = "glm-5.2"

[model_providers.opencode]
name = "OpenCode Go"
base_url = "https://opencode.ai/zen/go/v1"
wire_api = "chat"
"#,
            "modelCatalog": {
                "models": [
                    { "model": "glm-5.2", "reasoningLevels": ["high", "max"] },
                    { "model": "deepseek-v4-flash", "reasoningLevels": ["low", "high", "max"] },
                    { "model": "glm-5.1" }
                ]
            }
        }));

        // 逐模型查表：声明了 reasoningLevels 的模型附上各自档位（模型名大小写不敏感）。
        let config =
            resolve_codex_chat_reasoning_config(&provider, &json!({ "model": "GLM-5.2" })).unwrap();
        assert_eq!(
            config.effort_levels,
            Some(vec!["high".to_string(), "max".to_string()])
        );

        let config = resolve_codex_chat_reasoning_config(
            &provider,
            &json!({ "model": "deepseek-v4-flash" }),
        )
        .unwrap();
        assert_eq!(
            config.effort_levels,
            Some(vec![
                "low".to_string(),
                "high".to_string(),
                "max".to_string()
            ])
        );

        // toggle 型模型（目录条目未声明 reasoningLevels）→ None：转换层不发 effort 字段。
        let config =
            resolve_codex_chat_reasoning_config(&provider, &json!({ "model": "glm-5.1" })).unwrap();
        assert_eq!(config.effort_value_mode.as_deref(), Some("zen"));
        assert!(config.effort_levels.is_none());

        // 目录未收录的模型 → 同样 None。
        let config =
            resolve_codex_chat_reasoning_config(&provider, &json!({ "model": "kimi-k3" })).unwrap();
        assert!(config.effort_levels.is_none());
    }

    #[test]
    fn test_resolve_codex_chat_reasoning_zen_levels_attach_on_explicit_meta_too() {
        let mut provider = create_provider(json!({
            "config": r#"
model_provider = "opencode"
model = "deepseek-v4-flash"

[model_providers.opencode]
name = "OpenCode Go"
base_url = "https://opencode.ai/zen/go/v1"
wire_api = "chat"
"#,
            "modelCatalog": {
                "models": [
                    { "model": "deepseek-v4-flash", "reasoning_levels": ["low", "high", "max"] }
                ]
            }
        }));
        // 显式 meta（表单手选 zen 模式）同样要在 resolve 末端按请求模型附表；
        // 且手写/旧数据可能是 snake_case 的 reasoning_levels（加载侧双格式兼容）。
        provider.meta = Some(crate::provider::ProviderMeta {
            codex_chat_reasoning: Some(CodexChatReasoningConfig {
                supports_thinking: Some(true),
                supports_effort: Some(true),
                thinking_param: Some("none".to_string()),
                effort_param: Some("reasoning_effort".to_string()),
                effort_value_mode: Some("zen".to_string()),
                output_format: Some("reasoning_content".to_string()),
                effort_levels: None,
            }),
            ..Default::default()
        });

        let config = resolve_codex_chat_reasoning_config(
            &provider,
            &json!({ "model": "deepseek-v4-flash" }),
        )
        .unwrap();

        assert_eq!(config.effort_value_mode.as_deref(), Some("zen"));
        assert_eq!(
            config.effort_levels,
            Some(vec![
                "low".to_string(),
                "high".to_string(),
                "max".to_string()
            ])
        );
    }

    #[test]
    fn test_infer_codex_chat_reasoning_stepfun_per_model_effort() {
        let provider = create_provider(json!({
            "config": r#"
model_provider = "stepfun"
model = "step-3.7-flash"

[model_providers.stepfun]
name = "StepFun"
base_url = "https://api.stepfun.com/step_plan/v1"
wire_api = "chat"
"#
        }));

        // step-3.7-flash：官方三档 low/medium/high —— 必须 passthrough，
        // 套 low_high 会把 medium 塌成 high（假差异档）
        let config =
            resolve_codex_chat_reasoning_config(&provider, &json!({ "model": "step-3.7-flash" }))
                .unwrap();
        assert_eq!(config.supports_effort, Some(true));
        assert_eq!(config.effort_value_mode.as_deref(), Some("passthrough"));

        // step-3.5-flash-2603：官方两档 low/high，沿用收敛映射
        let config = resolve_codex_chat_reasoning_config(
            &provider,
            &json!({ "model": "step-3.5-flash-2603" }),
        )
        .unwrap();
        assert_eq!(config.supports_effort, Some(true));
        assert_eq!(config.effort_value_mode.as_deref(), Some("low_high"));

        // 无后缀 step-3.5-flash：官方未暴露 effort，不下发
        let config =
            resolve_codex_chat_reasoning_config(&provider, &json!({ "model": "step-3.5-flash" }))
                .unwrap();
        assert_eq!(config.supports_effort, Some(false));
        assert_eq!(config.thinking_param.as_deref(), Some("none"));
    }

    #[test]
    fn xai_oauth_invariants_ignore_editable_base_url_and_auth() {
        let adapter = CodexAdapter::new();
        let mut provider = create_provider(json!({
            "auth": { "OPENAI_API_KEY": "user-edited" },
            "config": r#"
model = "grok-4.5"

[model_providers.custom]
name = "xai"
base_url = "https://attacker.example/v1"
wire_api = "responses"
"#
        }));
        provider.meta = Some(crate::provider::ProviderMeta {
            provider_type: Some("xai_oauth".to_string()),
            ..Default::default()
        });

        // 可编辑字段（base_url / auth key）不得影响托管路由：
        // 端点硬定向 api.x.ai，凭据是占位符（真 token 由 forwarder 注入）。
        assert_eq!(
            adapter.extract_base_url(&provider).unwrap(),
            super::super::XAI_API_BASE_URL
        );
        let auth = adapter
            .extract_auth(&provider)
            .expect("managed auth placeholder");
        assert_eq!(auth.api_key, "xai_oauth_placeholder");
        assert_eq!(auth.strategy, AuthStrategy::XaiOAuth);
    }

    #[test]
    fn xai_oauth_pins_native_responses_catalog_profile() {
        let mut provider = create_provider(json!({ "auth": {}, "config": "" }));
        provider.meta = Some(crate::provider::ProviderMeta {
            provider_type: Some("xai_oauth".to_string()),
            // 即使 api_format 被改成 anthropic，catalog 画像也必须钉死原生 Responses
            api_format: Some("anthropic".to_string()),
            ..Default::default()
        });

        assert!(matches!(
            resolve_codex_catalog_tool_profile(&provider),
            crate::codex_config::CodexCatalogToolProfile::NativeResponses
        ));
    }

    #[test]
    fn namespace_flatten_gate_fires_for_xai_oauth_and_api_xai_responses() {
        let mut xai = create_provider(json!({ "auth": {}, "config": "" }));
        xai.meta = Some(crate::provider::ProviderMeta {
            provider_type: Some("xai_oauth".to_string()),
            ..Default::default()
        });
        assert!(provider_needs_responses_namespace_flatten(&xai));

        // API-key Grok cards (no xai_oauth meta) still talk to api.x.ai Responses.
        let grok_key = create_provider(json!({
            "auth": { "OPENAI_API_KEY": "sk-x" },
            "config": r#"
model_provider = "custom"
model = "grok-4.6"

[model_providers.custom]
name = "xai"
base_url = "https://api.x.ai/v1"
wire_api = "responses"
"#
        }));
        assert!(provider_needs_responses_namespace_flatten(&grok_key));

        // A non-xAI Responses provider must not be flattened.
        let other = create_provider(json!({
            "auth": { "OPENAI_API_KEY": "sk-x" },
            "config": r#"
[model_providers.custom]
base_url = "https://api.deepseek.com"
wire_api = "responses"
"#
        }));
        assert!(!provider_needs_responses_namespace_flatten(&other));
    }
}
