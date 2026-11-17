//! Outbound header policy: auth checks, fingerprint stripping and local overrides.

use super::*;

pub(super) fn codex_bearer_access_token(headers: &http::HeaderMap) -> Option<&str> {
    let authorization = headers
        .get(http::header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .trim();
    let mut parts = authorization.split_whitespace();
    let scheme = parts.next()?;
    let token = parts.next()?;
    if !scheme.eq_ignore_ascii_case("bearer") || token.is_empty() || parts.next().is_some() {
        return None;
    }
    Some(token)
}

pub(super) fn validate_codex_official_authorization(
    headers: &http::HeaderMap,
    provider: &Provider,
    expected_chatgpt_account_id: Option<&str>,
    managed_session_matches: Option<bool>,
) -> Result<(), ProxyError> {
    let authorization = headers
        .get(http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .map(str::trim);
    match authorization {
        None | Some("") => Err(ProxyError::AuthError(
            "Codex 官方登录不可用，请先在 Codex 中完成 ChatGPT 登录".to_string(),
        )),
        // 带占位 Key 的请求有两种来源：没重载配置的 Codex 进程；或切换前用第三方供应商建的
        // 旧会话（Codex 按会话记下的 provider 走休眠的 custom 表，重启也还是它）。
        Some(value) if value.contains(PROXY_AUTH_PLACEHOLDER) => Err(ProxyError::AuthError(
            "已切换到 OpenAI 官方供应商，但这个请求仍按第三方供应商的配置发出。如果这是切换前用第三方供应商创建的旧会话，它会一直沿用原供应商，重启也无效，请新建会话继续；否则请重启 Codex 以加载官方登录配置".to_string(),
        )),
        Some(_) => {
            let managed_account_id = provider
                .meta
                .as_ref()
                .and_then(|meta| meta.managed_account_id_for("codex_oauth"))
                .map(|account_id| account_id.trim().to_string())
                .filter(|account_id| !account_id.is_empty());
            if managed_account_id.is_some() {
                let request_account_id = headers
                    .get("chatgpt-account-id")
                    .and_then(|value| value.to_str().ok())
                    .map(str::trim)
                    .filter(|account_id| !account_id.is_empty());
                if request_account_id != expected_chatgpt_account_id
                    || managed_session_matches != Some(true)
                {
                    return Err(ProxyError::AuthError(
                        "当前 Codex 会话未加载所选 ChatGPT 账号，请重启 Codex 或新建会话后重试"
                            .to_string(),
                    ));
                }
            }
            Ok(())
        }
    }
}

/// Claude Code client fingerprint (used for Codex→Anthropic emulation to pass a
/// gateway's "Claude Code only" check).
pub(super) const CLAUDE_CODE_USER_AGENT: &str = "claude-cli/1.0.119 (external, cli)";

pub(super) fn is_codex_client_fingerprint_header(key_str: &str) -> bool {
    matches!(
        key_str,
        "originator"
            | "session_id"
            | "session-id"
            | "thread-id"
            | "conversation_id"
            | "chatgpt-account-id"
            | "x-client-request-id"
            | "openai-beta"
            | "openai-organization"
            | "openai-project"
            // Codex 内置 openai 的固定请求头（值是 Codex 版本号）。
            | "version"
    ) || key_str.starts_with("x-stainless-")
        || key_str.starts_with("x-codex-")
        // ChatGPT 登录态下 Codex 每个请求都带：`x-oai-attestation`（设备证明）、
        // `x-openai-fedramp`、`x-openai-account-routing-override`、`x-openai-memgen-request`、
        // `x-openai-internal-codex-responses-lite`、`x-responsesapi-include-timing-metrics`。
        || key_str.starts_with("x-oai-")
        || key_str.starts_with("x-openai-")
        || key_str.starts_with("x-responsesapi-")
}

/// Decide whether a Codex fingerprint header must be removed before the request
/// reaches an upstream.
///
/// Stack requests to a third party still must not carry ChatGPT session or
/// account identity. A native Responses third party, however, may use the
/// non-identity `x-codex-beta-features` marker to recognize the Codex protocol
/// and reject requests without it. Keep only that marker; all other Codex and
/// OpenAI identity headers remain stripped. Converted Chat/Anthropic requests
/// continue to receive a clean client fingerprint.
///
/// Any other Codex request to a third party (`codex_unmanaged_upstream`: Codex and
/// not a CC Switch-managed ChatGPT account) drops only the ChatGPT account identity
/// ([`is_chatgpt_identity_header`]): the client may be the built-in `openai`
/// provider carrying the user's ChatGPT login. Session and client headers stay,
/// so requests from a plain custom table are unchanged.
pub(super) fn should_strip_codex_client_fingerprint_header(
    key_str: &str,
    endpoint: &str,
    codex_responses_to_anthropic: bool,
    codex_responses_to_chat: bool,
    codex_stack_request: bool,
    codex_official_auth_passthrough: bool,
    codex_unmanaged_upstream: bool,
) -> bool {
    if codex_responses_to_anthropic {
        return is_codex_client_fingerprint_header(key_str);
    }

    if codex_stack_request && !codex_official_auth_passthrough {
        let native_third_party_responses = !codex_responses_to_chat
            && crate::proxy::providers::is_codex_responses_endpoint(endpoint);
        return is_codex_client_fingerprint_header(key_str)
            && !(native_third_party_responses && key_str == "x-codex-beta-features");
    }

    codex_unmanaged_upstream
        && !codex_official_auth_passthrough
        && is_chatgpt_identity_header(key_str)
}

/// 只有带着 ChatGPT 登录的内置 openai 才会发的请求头：账号、组织 / 项目、设备证明和
/// 内置 openai 的固定版本头。会话、客户端标识（`session_id`、`originator`、`x-codex-*`）
/// 任何 Codex 客户端都发，不在其中。
pub(super) fn is_chatgpt_identity_header(key_str: &str) -> bool {
    matches!(
        key_str,
        "chatgpt-account-id" | "openai-organization" | "openai-project" | "version"
    ) || key_str.starts_with("x-oai-")
        || key_str.starts_with("x-openai-")
        || key_str.starts_with("x-responsesapi-")
}

pub(super) fn build_codex_oauth_session_headers(
    session_id: &str,
) -> Vec<(http::HeaderName, http::HeaderValue)> {
    let session_id = session_id.trim();
    if session_id.is_empty() {
        return Vec::new();
    }

    let mut headers = Vec::new();
    if let Ok(value) = http::HeaderValue::from_str(session_id) {
        headers.push((http::HeaderName::from_static("session_id"), value.clone()));
        headers.push((http::HeaderName::from_static("x-client-request-id"), value));
    }

    let window_id = format!("{session_id}:0");
    if let Ok(value) = http::HeaderValue::from_str(&window_id) {
        headers.push((http::HeaderName::from_static("x-codex-window-id"), value));
    }

    headers
}

pub(super) fn reject_proxy_placeholder_for_managed_account_upstream(
    url: &str,
    headers: &http::HeaderMap,
) -> Result<(), ProxyError> {
    if !is_managed_account_upstream_url(url) || !headers_contain_proxy_placeholder(headers) {
        return Ok(());
    }

    Err(ProxyError::AuthError(
        "Managed account proxy auth was not resolved; PROXY_MANAGED must not be sent upstream"
            .to_string(),
    ))
}

pub(super) fn is_managed_account_upstream_url(url: &str) -> bool {
    let Ok(uri) = url.parse::<http::Uri>() else {
        return false;
    };

    let Some(host) = uri.host().map(str::to_ascii_lowercase) else {
        return false;
    };

    host == "githubcopilot.com"
        || host.ends_with(".githubcopilot.com")
        || (host == "chatgpt.com" && uri.path().starts_with("/backend-api/codex"))
        || (host == "api.x.ai" && uri.path().starts_with("/v1/"))
}

pub(super) fn headers_contain_proxy_placeholder(headers: &http::HeaderMap) -> bool {
    headers.values().any(|value| {
        value
            .to_str()
            .map(|value| value.contains(PROXY_AUTH_PLACEHOLDER))
            .unwrap_or(false)
    })
}

pub(super) fn should_preserve_exact_header_case(
    adapter_name: &str,
    provider: &Provider,
    resolved_claude_api_format: Option<&str>,
    is_copilot: bool,
) -> bool {
    if matches!(adapter_name, "Codex" | "Gemini") {
        return false;
    }

    if is_copilot || provider.is_codex_oauth() || provider.is_xai_oauth() {
        return false;
    }

    matches!(resolved_claude_api_format, None | Some("anthropic"))
}

#[cfg(test)]
pub(super) fn should_force_identity_encoding(
    endpoint: &str,
    body: &Value,
    headers: &axum::http::HeaderMap,
) -> bool {
    is_streaming_request(endpoint, body, headers)
}

pub(super) fn apply_local_proxy_header_overrides(
    headers: &mut http::HeaderMap,
    overrides: Option<&LocalProxyRequestOverrides>,
    is_copilot: bool,
) {
    if is_copilot {
        return;
    }

    let Some(header_overrides) = overrides.map(|overrides| &overrides.headers) else {
        return;
    };

    for (raw_name, raw_value) in header_overrides {
        let header_name = raw_name.trim().to_ascii_lowercase();
        if header_name.is_empty() {
            log::warn!("[LocalProxyOverrides] Ignoring header override with empty name");
            continue;
        }

        let Ok(name) = http::HeaderName::from_bytes(header_name.as_bytes()) else {
            log::warn!("[LocalProxyOverrides] Ignoring invalid header override name: {raw_name}");
            continue;
        };

        if is_protected_local_proxy_override_header(&name) {
            log::debug!(
                "[LocalProxyOverrides] Ignoring protected header override: {}",
                name.as_str()
            );
            continue;
        }

        let Ok(value) = http::HeaderValue::from_str(raw_value) else {
            log::warn!(
                "[LocalProxyOverrides] Ignoring invalid header override value for {}",
                name.as_str()
            );
            continue;
        };

        headers.insert(name, value);
    }
}

pub(super) fn is_protected_local_proxy_override_header(name: &http::HeaderName) -> bool {
    matches!(
        name.as_str(),
        "host"
            | "content-length"
            | "transfer-encoding"
            | "connection"
            | "proxy-authorization"
            | "proxy-authenticate"
            | "te"
            | "trailer"
            | "upgrade"
            | "accept-encoding"
            | "content-type"
            | "authorization"
            | "x-api-key"
            | "x-goog-api-key"
            | "chatgpt-account-id"
            | "session_id"
            | "x-client-request-id"
            | "x-codex-window-id"
            | "x-forwarded-host"
            | "x-forwarded-port"
            | "x-forwarded-proto"
            | "forwarded"
            | "cf-connecting-ip"
            | "cf-ipcountry"
            | "cf-ray"
            | "cf-visitor"
            | "true-client-ip"
            | "fastly-client-ip"
            | "x-azure-clientip"
            | "x-azure-fdid"
            | "x-azure-ref"
            | "akamai-origin-hop"
            | "x-akamai-config-log-detail"
            | "x-request-id"
            | "x-correlation-id"
            | "x-trace-id"
            | "x-amzn-trace-id"
            | "x-b3-traceid"
            | "x-b3-spanid"
            | "x-b3-parentspanid"
            | "x-b3-sampled"
            | "traceparent"
            | "tracestate"
    )
}
