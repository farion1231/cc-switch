//! Codex (OpenAI) Provider Adapter
//!
//! 仅透传模式，支持直连 OpenAI API
//!
//! ## 客户端检测
//! 支持检测官方 Codex 客户端 (codex_vscode, codex_cli_rs)

use super::{AuthInfo, AuthStrategy, ProviderAdapter};
use crate::codex_config::{
    active_provider_str, extract_codex_base_url, is_anthropic_wire_api, is_chat_wire_api,
};
use crate::provider::{CodexChatReasoningConfig, Provider};
use crate::proxy::error::ProxyError;
use serde_json::Value as JsonValue;
use std::collections::HashSet;
use toml::Value as TomlValue;

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
        .and_then(extract_codex_base_url)
        .map(|url| is_chat_completions_url(&url))
        .unwrap_or(false)
}

pub fn is_codex_responses_endpoint(endpoint: &str) -> bool {
    let path = endpoint
        .split_once('?')
        .map_or(endpoint, |(path, _query)| path);

    matches!(
        path,
        "/responses" | "/v1/responses" | "/responses/compact" | "/v1/responses/compact"
    )
}

pub fn should_convert_codex_responses_to_chat(provider: &Provider, endpoint: &str) -> bool {
    is_codex_responses_endpoint(endpoint) && codex_provider_uses_chat_completions(provider)
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
                .and_then(extract_codex_base_url)
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
/// `api.x.ai` with `wire_api = "responses"`, plus Grok models served by managed
/// Copilot. Call only on native Responses paths, with the resolved outbound
/// model: a Copilot card can serve both Grok and non-Grok models.
/// See farion1231/cc-switch#6815.
pub fn provider_needs_responses_namespace_flatten(
    provider: &Provider,
    outbound_model: Option<&str>,
) -> bool {
    provider.is_xai_oauth()
        || provider_is_xai_native_responses(provider)
        || (provider.is_github_copilot()
            && outbound_model.is_some_and(|model| {
                model
                    .trim()
                    .as_bytes()
                    .get(..5)
                    .is_some_and(|prefix| prefix.eq_ignore_ascii_case(b"grok-"))
            }))
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

    extract_codex_base_url(config_text)
        .is_some_and(|url| crate::codex_config::codex_url_host_matches_any(&url, &["api.x.ai"]))
}

/// 原生 Responses 透传的响应要不要补迟到的函数调用参数（见 `responses_late_arguments`）：
/// 官方以外的上游都补。触发条件是协议违规本身（结束事件参数为空、增量排在后面，实测
/// MiniMax），不按厂商名：转发 MiniMax 原生流的中转站同样会带过来。顺序正常的流原样放行。
///
/// 只在转 Chat / 转 Anthropic / xAI 改写之外的原生透传分支里调用。
pub fn provider_needs_responses_late_arguments_repair(provider: &Provider) -> bool {
    !is_codex_official_provider(provider)
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
    if is_fixed_official_id && provider.has_official_category() {
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

    is_fixed_official_id || provider.has_official_category()
}

/// Vendors whose OFFICIAL Codex integration is a native `/responses` gateway that
/// rejects Codex's freeform custom tools (`apply_patch` with `type: "custom"`,
/// #6944). This is intentionally separate from `CODEX_WEB_SEARCH_REJECT_HOSTS`:
/// web-search compatibility alone must not change a stored Chat provider's
/// protocol or catalog. Matched on
/// host labels via `codex_url_host_matches_any`, never by substring.
const CODEX_NATIVE_RESPONSES_HOSTS: &[&str] = &[
    "bigmodel.cn",
    "z.ai",
    "xiaomimimo.com",
    "minimaxi.com",
    "minimax.cn",
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
    if provider.is_github_copilot() {
        return CodexCatalogToolProfile::Copilot;
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
        .and_then(extract_codex_base_url)
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

/// Stack 请求的上游拒收 Codex 的托管 `web_search`：按这家的地址和模型品牌判断，和它做
/// 路由时写 `web_search = "disabled"` 的依据相同（归一化后的配置，旧形态的行也认得出
/// 地址）；另看这次请求的模型：行里配了多个模型时，选中的不一定是行的 `model`。
pub fn codex_stack_upstream_rejects_web_search(
    provider: &Provider,
    request_model: Option<&str>,
) -> bool {
    if provider.is_github_copilot() {
        return true;
    }

    let projected =
        crate::live::project::codex::CodexProjection::of(&crate::live::project::codex::RowInput {
            settings: &provider.settings_config,
            official: false,
            proxy_injected_oauth: provider.uses_proxy_injected_oauth(),
        })
        .map(|projection| projection.catalog_input_text());
    let config_text = match &projected {
        Ok(text) => text.as_str(),
        Err(_) => provider
            .settings_config
            .get("config")
            .and_then(|value| value.as_str())
            .unwrap_or(""),
    };
    crate::codex_config::codex_native_gateway_rejects_web_search(config_text)
        || request_model.is_some_and(crate::codex_config::codex_model_rejects_web_search)
}

/// 去掉 Responses 请求里托管的 `web_search` 工具：`tools` 去完为空时整个键删掉；指向它
/// 的 `tool_choice` 一并删掉，`tools` 整个没了时 `tool_choice` 也删（上游对没有工具的
/// `tool_choice` 报 400）。其余字段不动。返回是否改了请求。
pub fn strip_codex_hosted_web_search(body: &mut JsonValue) -> bool {
    let Some(obj) = body.as_object_mut() else {
        return false;
    };
    let is_web_search =
        |tool: &JsonValue| tool.get("type").and_then(|value| value.as_str()) == Some("web_search");
    let mut changed = false;
    let mut tools_gone = false;
    if let Some(JsonValue::Array(tools)) = obj.get_mut("tools") {
        let before = tools.len();
        tools.retain(|tool| !is_web_search(tool));
        changed = tools.len() != before;
        tools_gone = changed && tools.is_empty();
    }
    if tools_gone {
        obj.remove("tools");
    }
    let choice_is_web_search = obj.get("tool_choice").is_some_and(is_web_search);
    if choice_is_web_search || (tools_gone && obj.contains_key("tool_choice")) {
        obj.remove("tool_choice");
        changed = true;
    }
    changed
}

/// Codex 客户端私有、第三方原生 Responses 网关普遍不认识的 `include` 标记。Codex 0.163+
/// 起随请求下发（#8996 / #9042），ChatGPT 后端之外的网关会拿它整条 400。只删这一项，
/// 其余标记原样保留。
const CODEX_PRIVATE_INCLUDE_MARKERS: &[&str] = &["output.encrypted_content"];

/// 去掉 `include` 里的 Codex 私有标记：去完为空时整个键删掉；缺键、非数组、非对象
/// 请求一律不动；数组里的非字符串项原样留着。返回是否改了请求。和 xAI 侧的白名单各管
/// 一段，重复删无副作用。
pub fn strip_codex_private_include_markers(body: &mut JsonValue) -> bool {
    let Some(obj) = body.as_object_mut() else {
        return false;
    };
    let mut include_gone = false;
    let changed = match obj.get_mut("include").and_then(JsonValue::as_array_mut) {
        Some(include) => {
            let before = include.len();
            include.retain(|marker| {
                !marker
                    .as_str()
                    .is_some_and(|marker| CODEX_PRIVATE_INCLUDE_MARKERS.contains(&marker))
            });
            let changed = include.len() != before;
            include_gone = changed && include.is_empty();
            changed
        }
        None => false,
    };
    if include_gone {
        obj.remove("include");
    }
    changed
}

/// Codex 对非 OpenAI provider 会清掉 `encrypted_content` 的条目类型：codex-rs
/// `ResponseItem::clear_encrypted_content` 覆盖的类型，去掉它另外放行的推理与压缩条目。
/// 按正向名单照抄，Codex 以后新加的条目类型不会被误删。
const CODEX_OPENAI_ONLY_ENCRYPTED_CONTENT_ITEM_TYPES: &[&str] = &[
    "message",
    "local_shell_call",
    "function_call",
    "tool_search_call",
    "custom_tool_call",
    "web_search_call",
    "image_generation_call",
];

/// 去掉 Codex 只在 provider 是内置 openai 时才发的请求内容，让第三方上游收到的请求和
/// Codex 自己对非 OpenAI provider 发的一样。照 codex-rs `core/src/client.rs`
/// `build_responses_request` 里 `is_openai()`（`name == "OpenAI"`）的分支：
///
/// - `include` 里的 `output.encrypted_content`（`output_token_replay` 只对 OpenAI 开）；
/// - `stream_options.reasoning_summary_delivery`（同上，`stream_options` 只有这一个字段）；
/// - `input` 里的 `configuration_update` 条目（推理档位更新只对 OpenAI 开，否则整条滤掉）；
/// - 每个条目的 `internal_chat_message_metadata_passthrough`；
/// - 推理、压缩以外条目的 `encrypted_content`，以及 `function_call` 的 `encrypted_function_args`。
///
/// 另加顶层 `access_programs`：Codex 按登录方式而非 provider 决定发不发，但只有带着
/// ChatGPT 登录才会出现，自定义表（第三方 Key）永远不发。
///
/// 第三方路由写成内置 openai（有 ChatGPT 登录时）、聚合模式经内置 openai 走第三方成员，
/// Codex 都以为自己在跟 OpenAI 说话，这些字段就会到达第三方，严格网关会整条 400
/// （#9042）。每次删得一样，不影响缓存前缀；请求里没有这些字段时逐字节不变。返回是否
/// 改了请求。
pub fn strip_codex_openai_only_request_fields(body: &mut JsonValue) -> bool {
    let mut changed = strip_codex_private_include_markers(body);
    let Some(obj) = body.as_object_mut() else {
        return changed;
    };

    let mut stream_options_gone = false;
    if let Some(stream_options) = obj
        .get_mut("stream_options")
        .and_then(JsonValue::as_object_mut)
    {
        if stream_options
            .remove("reasoning_summary_delivery")
            .is_some()
        {
            changed = true;
            stream_options_gone = stream_options.is_empty();
        }
    }
    if stream_options_gone {
        obj.remove("stream_options");
    }

    changed |= obj.remove("access_programs").is_some();

    if let Some(input) = obj.get_mut("input").and_then(JsonValue::as_array_mut) {
        let before = input.len();
        input.retain(|item| {
            item.get("type").and_then(JsonValue::as_str) != Some("configuration_update")
        });
        changed |= input.len() != before;

        for item in input.iter_mut() {
            let Some(item) = item.as_object_mut() else {
                continue;
            };
            changed |= item
                .remove("internal_chat_message_metadata_passthrough")
                .is_some();
            let item_type = item.get("type").and_then(JsonValue::as_str);
            let drops_encrypted_content = item_type
                .is_some_and(|t| CODEX_OPENAI_ONLY_ENCRYPTED_CONTENT_ITEM_TYPES.contains(&t));
            let is_function_call = item_type == Some("function_call");
            if drops_encrypted_content {
                changed |= item.remove("encrypted_content").is_some();
            }
            if is_function_call {
                changed |= item.remove("encrypted_function_args").is_some();
            }
        }
    }

    changed
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
    if config.supports_effort.unwrap_or(false) {
        if config.supports_thinking.is_none() {
            config.supports_thinking = Some(true);
        }
        if config.effort_param.as_deref().is_none_or(|value| {
            value.trim().is_empty() || value.trim().eq_ignore_ascii_case("none")
        }) {
            config.effort_param = Some("reasoning_effort".to_string());
        }
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
        .unwrap_or_default();
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
                .and_then(extract_codex_base_url)
        })
        .unwrap_or_default();
    infer_chat_reasoning_config(&provider.name, &base_url, &model)
}

/// 按供应商名、上游地址、上游模型名推断 Chat 兼容上游的 reasoning 接口。
/// Codex Responses → Chat 与 Claude 路由（经 Chat / Responses 转发）共用这份判定。
pub(crate) fn infer_chat_reasoning_config(
    name: &str,
    base_url: &str,
    model: &str,
) -> Option<CodexChatReasoningConfig> {
    let name = name.to_ascii_lowercase();
    let base_url = base_url.to_ascii_lowercase();
    let model = model.to_ascii_lowercase();

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

    // Mistral 官方 API（#9507）：Chat 请求体不收额外字段，它托管的 deepseek / glm 名模型
    // 会被下方模型规则注入 `thinking:{type}`，整条 422。与 Codex 的 Mistral 预设、Claude
    // 侧（`claude_reasoning`）共用 `is_mistral_api`，不下发推理参数。
    // 要发档位的用户在卡上显式配置 codexChatReasoning。
    if super::is_mistral_api(base_url) {
        return Some(CodexChatReasoningConfig {
            supports_thinking: Some(false),
            supports_effort: Some(false),
            thinking_param: Some("none".to_string()),
            effort_param: Some("none".to_string()),
            effort_value_mode: None,
            output_format: Some("auto".to_string()),
            effort_levels: None,
        });
    }

    None
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
    active_provider_str(&doc, "wire_api").map(str::to_string)
}

fn extract_codex_model_from_toml(config_text: &str) -> Option<String> {
    let doc = config_text.parse::<TomlValue>().ok()?;

    doc.get("model")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|model| !model.is_empty())
        .map(ToString::to_string)
}

impl CodexAdapter {
    pub fn new() -> Self {
        Self
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

        if provider.is_github_copilot() {
            return Ok("https://api.githubcopilot.com".to_string());
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
                // The table `model_provider` selects, read the way Codex reads it. The text
                // scans below remain for a top-level `openai_base_url` and for TOML that
                // does not parse.
                if let Some(url) = extract_codex_base_url(config_str) {
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
        if provider.is_github_copilot() {
            return Some(AuthInfo::new(
                "copilot_placeholder".to_string(),
                AuthStrategy::GitHubCopilot,
            ));
        }

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
        if auth.strategy == AuthStrategy::GitHubCopilot {
            return super::copilot_auth::build_copilot_request_headers(&auth.api_key);
        }
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
mod tests;
