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
            | "copilot-session-token"
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

/// The `anthropic-beta` value to send. A native Anthropic route always
/// carries the `claude-code` marker (prepended to the client's value when it is
/// missing); Codex→Anthropic gets the emulation and `[1m]` markers it needs.
pub(super) fn anthropic_beta_value(
    headers: &http::HeaderMap,
    should_send_anthropic_headers: bool,
    codex_impersonate_claude_code: bool,
    codex_anthropic_one_m: bool,
) -> Option<String> {
    if should_send_anthropic_headers {
        const CLAUDE_CODE_BETA: &str = "claude-code-20250219";
        Some(if let Some(beta) = headers.get("anthropic-beta") {
            if let Ok(beta_str) = beta.to_str() {
                if beta_str.contains(CLAUDE_CODE_BETA) {
                    beta_str.to_string()
                } else {
                    format!("{CLAUDE_CODE_BETA},{beta_str}")
                }
            } else {
                CLAUDE_CODE_BETA.to_string()
            }
        } else {
            CLAUDE_CODE_BETA.to_string()
        })
    } else if codex_impersonate_claude_code || codex_anthropic_one_m {
        // Codex→Anthropic: emulation injects the claude-code marker; a [1m]
        // model injects the context-1m marker.
        let mut betas: Vec<&str> = Vec::new();
        if codex_impersonate_claude_code {
            betas.push("claude-code-20250219");
        }
        if codex_anthropic_one_m {
            betas.push("context-1m-2025-08-07");
        }
        Some(betas.join(","))
    } else {
        None
    }
}

/// Copilot fingerprint headers. `get_auth_headers` injects them for a Copilot
/// provider, so the client's copies are dropped.
const COPILOT_FINGERPRINT_HEADERS: &[&str] = &[
    "user-agent",
    "editor-version",
    "editor-plugin-version",
    "copilot-integration-id",
    "x-github-api-version",
    "openai-intent",
    // 新增 headers
    "x-initiator",
    "x-interaction-type",
    "x-interaction-id",
    "x-vscode-user-agent-library-version",
    "x-request-id",
    "x-agent-task-id",
];

/// What `forward` decided about one outbound request that shapes its headers.
pub(super) struct OutboundHeaderPlan<'a> {
    /// Authority of the upstream URL; replaces the client's `host` in place.
    pub(super) upstream_host: Option<String>,
    pub(super) auth_headers: &'a [(http::HeaderName, http::HeaderValue)],
    pub(super) codex_official_auth_passthrough: bool,
    pub(super) codex_impersonate_claude_code: bool,
    pub(super) codex_responses_to_anthropic: bool,
    pub(super) codex_responses_to_chat: bool,
    pub(super) codex_stack_request: bool,
    /// A Codex request whose upstream is not a managed ChatGPT account.
    pub(super) codex_unmanaged_upstream: bool,
    pub(super) effective_endpoint: &'a str,
    pub(super) force_identity_encoding: bool,
    pub(super) is_copilot: bool,
    pub(super) custom_user_agent: Option<http::HeaderValue>,
    pub(super) opencode_fallback_user_agent: Option<http::HeaderValue>,
    pub(super) anthropic_beta_value: Option<String>,
    pub(super) should_send_anthropic_headers: bool,
    pub(super) codex_oauth_session_headers: Vec<(http::HeaderName, http::HeaderValue)>,
    /// A session token resolved for this managed Copilot Auto request.
    pub(super) copilot_auto_session_token: Option<http::HeaderValue>,
}

impl OutboundHeaderPlan<'_> {
    /// The outbound headers in the client's order: managed headers replace the
    /// client's copy in place, and the ones the client did not send are appended.
    pub(super) fn build(self, headers: &http::HeaderMap) -> http::HeaderMap {
        let mut ordered_headers = http::HeaderMap::new();
        let mut saw_auth = false;
        let mut saw_accept_encoding = false;
        let mut saw_accept = false;
        let mut saw_user_agent = false;
        let mut saw_anthropic_beta = false;
        let mut saw_anthropic_version = false;

        for (key, value) in headers {
            let key_str = key.as_str();

            if self.is_copilot && key_str == "copilot-session-token" {
                continue;
            }

            // --- host — 原位替换为上游 host（保持客户端原始位置） ---
            if key_str.eq_ignore_ascii_case("host") {
                if let Some(ref host_val) = self.upstream_host {
                    if let Ok(hv) = http::HeaderValue::from_str(host_val) {
                        ordered_headers.append(key.clone(), hv);
                    }
                }
                continue;
            }

            // --- 连接 / 追踪 / CDN 类 — 无条件跳过 ---
            if matches!(
                key_str,
                "content-length"
                    | "transfer-encoding"
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
            ) {
                continue;
            }

            // --- 认证类 — 用 adapter 提供的认证头替换（在原始位置） ---
            if key_str.eq_ignore_ascii_case("authorization")
                || key_str.eq_ignore_ascii_case("x-api-key")
                || key_str.eq_ignore_ascii_case("x-goog-api-key")
            {
                // Codex official account cards deliberately keep credentials
                // out of provider storage. `requires_openai_auth = true` makes
                // Codex send the active ChatGPT authorization, which must reach
                // the official upstream unchanged. Other credential headers
                // are still discarded.
                if self.codex_official_auth_passthrough
                    && key_str.eq_ignore_ascii_case("authorization")
                {
                    saw_auth = true;
                    ordered_headers.append(key.clone(), value.clone());
                    continue;
                }
                if !saw_auth {
                    saw_auth = true;
                    for (ah_name, ah_value) in self.auth_headers {
                        ordered_headers.append(ah_name.clone(), ah_value.clone());
                    }
                }
                continue;
            }

            // --- x-app — during Codex→Anthropic emulation, `cli` is injected uniformly below ---
            if self.codex_impersonate_claude_code && key_str.eq_ignore_ascii_case("x-app") {
                continue;
            }

            // --- Codex/OpenAI fingerprint headers — never leak to an Anthropic upstream ---
            // These are client/session identifiers from the incoming Codex request,
            // not Anthropic protocol headers. Forwarding them both leaks identity and
            // can defeat strict gateway fingerprint checks.
            // The full set lives in `is_codex_client_fingerprint_header` so it stays in one
            // place. (HeaderName is lowercased by the http crate, so a direct match is safe.)
            // Codex Stack 请求发往第三方时同样剥掉：官方做路由时 Codex 每个请求都带着 ChatGPT
            // 身份（`chatgpt-account-id` 等），它们只能发往官方上游。Stack 目标不会是官方账号，
            // 这里仍按上游判断，防止以后放宽。只限 Codex：Claude Code 自己的 `x-stainless-*`
            // 是 Anthropic SDK 的正常请求头，Stack 请求照常转发。
            // 路由那家是第三方时 Codex 也可能以内置 openai 身份、带着 ChatGPT 登录发请求
            // （`codex_direct` 的 `openai_route`），身份头同样只能发往官方或 ChatGPT 托管账号。
            if should_strip_codex_client_fingerprint_header(
                key_str,
                self.effective_endpoint,
                self.codex_responses_to_anthropic,
                self.codex_responses_to_chat,
                self.codex_stack_request,
                self.codex_official_auth_passthrough,
                self.codex_unmanaged_upstream,
            ) {
                continue;
            }

            // --- accept — force application/json on the Codex→Anthropic path ---
            // The Codex CLI sends `Accept: text/event-stream`, whereas a native
            // Anthropic client sends `application/json` (streaming is driven by
            // the body's stream:true). Strict Anthropic gateways return 406 Not
            // Acceptable for an event-stream Accept, so normalize it here.
            if self.codex_responses_to_anthropic && key_str.eq_ignore_ascii_case("accept") {
                if !saw_accept {
                    saw_accept = true;
                    ordered_headers.append(
                        http::header::ACCEPT,
                        http::HeaderValue::from_static("application/json"),
                    );
                }
                continue;
            }

            // --- accept-encoding — transform / SSE 路径强制 identity，其余保留原值 ---
            if key_str.eq_ignore_ascii_case("accept-encoding") {
                if !saw_accept_encoding {
                    saw_accept_encoding = true;
                    if self.force_identity_encoding {
                        ordered_headers.append(
                            http::header::ACCEPT_ENCODING,
                            http::HeaderValue::from_static("identity"),
                        );
                    } else {
                        ordered_headers.append(key.clone(), value.clone());
                    }
                }
                continue;
            }

            // --- user-agent: provider-level override for local proxy routing ---
            if !self.is_copilot && key_str.eq_ignore_ascii_case("user-agent") {
                if !saw_user_agent {
                    saw_user_agent = true;
                    if let Some(ref ua) = self.custom_user_agent {
                        ordered_headers.append(http::header::USER_AGENT, ua.clone());
                    } else {
                        ordered_headers.append(key.clone(), value.clone());
                    }
                }
                continue;
            }

            // --- anthropic-beta — 用重建值替换（确保含 claude-code 标记） ---
            if key_str.eq_ignore_ascii_case("anthropic-beta") {
                if !saw_anthropic_beta {
                    saw_anthropic_beta = true;
                    if let Some(ref beta_val) = self.anthropic_beta_value {
                        if let Ok(hv) = http::HeaderValue::from_str(beta_val) {
                            ordered_headers.append("anthropic-beta", hv);
                        }
                    }
                }
                continue;
            }

            // --- anthropic-version — 透传客户端值 ---
            if key_str.eq_ignore_ascii_case("anthropic-version") {
                if self.should_send_anthropic_headers {
                    saw_anthropic_version = true;
                    ordered_headers.append(key.clone(), value.clone());
                }
                continue;
            }

            // --- Copilot 指纹头 — 跳过（由 auth_headers 提供） ---
            if self.is_copilot
                && COPILOT_FINGERPRINT_HEADERS
                    .iter()
                    .any(|h| key_str.eq_ignore_ascii_case(h))
            {
                continue;
            }

            // --- 默认：透传 ---
            ordered_headers.append(key.clone(), value.clone());
        }

        if let Some(token) = self.copilot_auto_session_token {
            ordered_headers.insert("copilot-session-token", token);
        }

        // 如果原始请求中没有认证头，在末尾追加
        if !saw_auth && !self.auth_headers.is_empty() {
            for (ah_name, ah_value) in self.auth_headers {
                ordered_headers.append(ah_name.clone(), ah_value.clone());
            }
        }

        // transform / SSE 路径在缺失时补 identity；普通透传不主动补 accept-encoding
        if !saw_accept_encoding && self.force_identity_encoding {
            ordered_headers.append(
                http::header::ACCEPT_ENCODING,
                http::HeaderValue::from_static("identity"),
            );
        }

        // On the Codex→Anthropic path, add application/json when Accept is missing (matching a native Anthropic client).
        if self.codex_responses_to_anthropic && !saw_accept {
            ordered_headers.append(
                http::header::ACCEPT,
                http::HeaderValue::from_static("application/json"),
            );
        }

        // Codex→Anthropic emulation: inject Claude Code's x-app: cli
        if self.codex_impersonate_claude_code {
            ordered_headers.append("x-app", http::HeaderValue::from_static("cli"));
        }

        if !saw_user_agent {
            if let Some(ua) = self
                .custom_user_agent
                .as_ref()
                .or(self.opencode_fallback_user_agent.as_ref())
            {
                ordered_headers.append(http::header::USER_AGENT, ua.clone());
            }
        }

        // 如果原始请求中没有 anthropic-beta 且有值需要添加，追加
        if !saw_anthropic_beta {
            if let Some(ref beta_val) = self.anthropic_beta_value {
                if let Ok(hv) = http::HeaderValue::from_str(beta_val) {
                    ordered_headers.append("anthropic-beta", hv);
                }
            }
        }

        // anthropic-version: add the default only when it is missing.
        // The Codex→Anthropic path also needs this header. Note this is independent
        // of anthropic-beta: the Claude Code-specific beta is only sent when
        // impersonation is on (handled above); on the plain Codex→Anthropic path
        // (impersonation off) anthropic-version is still required but no beta is sent.
        if (self.should_send_anthropic_headers || self.codex_responses_to_anthropic)
            && !saw_anthropic_version
        {
            ordered_headers.append(
                "anthropic-version",
                http::HeaderValue::from_static("2023-06-01"),
            );
        }

        // Codex OAuth 反代尽量对齐官方 Codex CLI 的会话路由信号。
        // 只发送客户端提供的 session_id；生成的 UUID 每次不同，反而会破坏前缀缓存。
        for (name, value) in self.codex_oauth_session_headers {
            ordered_headers.insert(name, value);
        }

        ordered_headers
    }
}
