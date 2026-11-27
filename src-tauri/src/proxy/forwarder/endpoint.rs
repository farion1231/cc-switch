//! Upstream endpoint and URL rewriting for each route mode.

use super::*;

pub(super) fn split_endpoint_and_query(endpoint: &str) -> (&str, Option<&str>) {
    endpoint
        .split_once('?')
        .map_or((endpoint, None), |(path, query)| (path, Some(query)))
}

pub(super) fn strip_beta_query(query: Option<&str>) -> Option<String> {
    let filtered = query.map(|query| {
        query
            .split('&')
            .filter(|pair| !pair.is_empty() && !pair.starts_with("beta="))
            .collect::<Vec<_>>()
            .join("&")
    });

    match filtered.as_deref() {
        Some("") | None => None,
        Some(_) => filtered,
    }
}

pub(super) fn is_claude_messages_path(path: &str) -> bool {
    matches!(path, "/v1/messages" | "/claude/v1/messages")
}

pub(super) fn rewrite_codex_responses_endpoint_to_chat(endpoint: &str) -> (String, Option<String>) {
    let (_path, query) = split_endpoint_and_query(endpoint);
    let passthrough_query = query.map(ToString::to_string);
    let target_path = "/chat/completions";
    let rewritten = match passthrough_query.as_deref() {
        Some(query) if !query.is_empty() => format!("{target_path}?{query}"),
        _ => target_path.to_string(),
    };

    (rewritten, passthrough_query)
}

/// Headers a native Claude Code client never sends but the Codex/OpenAI CLI (and its
/// stainless SDK layer) do. Dropped for every Codex→Anthropic request so the upstream sees a
/// clean Anthropic client fingerprint. Centralized here so the set stays in one place and future
/// additions can't miss a code path. `key_str` is already lowercased by the http crate.
/// Whether `base_url` already ends in `endpoint_suffix` (e.g. `/v1/messages` or
/// `/chat/completions`), ignoring surrounding whitespace, any `?query`/`#fragment`, and a
/// trailing slash. Used to avoid double-appending the endpoint when a user pastes a full
/// URL but leaves the "full URL" switch off (`.../v1/messages` → `.../v1/messages/v1/messages`,
/// a non-retryable 400). `endpoint_suffix` must be lowercase.
pub(super) fn base_url_is_full_endpoint(base_url: &str, endpoint_suffix: &str) -> bool {
    let trimmed = base_url.trim();
    // Match against the path only: a `?query`/`#fragment` on a full endpoint URL must not
    // hide the suffix (`.../v1/messages?beta=true` still ends in the endpoint).
    let path = match trimmed.split_once(['?', '#']) {
        Some((head, _)) => head,
        None => trimmed,
    };
    path.trim_end_matches('/')
        .to_ascii_lowercase()
        .ends_with(endpoint_suffix)
}

/// Rewrite Codex's `/responses` (and variants) to Anthropic's `/v1/messages`, preserving the query.
pub(super) fn rewrite_codex_responses_endpoint_to_anthropic(
    endpoint: &str,
) -> (String, Option<String>) {
    let (_path, query) = split_endpoint_and_query(endpoint);
    let passthrough_query = query.map(ToString::to_string);
    let target_path = "/v1/messages";
    let rewritten = match passthrough_query.as_deref() {
        Some(query) if !query.is_empty() => format!("{target_path}?{query}"),
        _ => target_path.to_string(),
    };

    (rewritten, passthrough_query)
}

pub(super) fn rewrite_codex_endpoint_for_copilot(
    inbound_endpoint: &str,
    supported_endpoint: &str,
) -> (String, Option<String>) {
    let (inbound_path, query) = split_endpoint_and_query(inbound_endpoint);
    let passthrough_query = query.map(ToString::to_string);
    let mut target_path = supported_endpoint.trim_end_matches('/').to_string();
    if inbound_path.ends_with("/responses/compact")
        && matches!(target_path.as_str(), "/responses" | "/v1/responses")
    {
        target_path.push_str("/compact");
    }
    let rewritten = match passthrough_query.as_deref() {
        Some(query) if !query.is_empty() => format!("{target_path}?{query}"),
        _ => target_path,
    };
    (rewritten, passthrough_query)
}

pub(super) fn build_copilot_unversioned_url(base_url: &str, endpoint: &str) -> String {
    format!(
        "{}/{}",
        base_url.trim_end_matches('/'),
        endpoint.trim_start_matches('/')
    )
}

pub(super) fn rewrite_claude_transform_endpoint(
    endpoint: &str,
    api_format: &str,
    is_copilot: bool,
    body: &Value,
) -> (String, Option<String>) {
    let (path, query) = split_endpoint_and_query(endpoint);
    let passthrough_query = if is_claude_messages_path(path) {
        strip_beta_query(query)
    } else {
        query.map(ToString::to_string)
    };

    if !is_claude_messages_path(path) {
        return (endpoint.to_string(), passthrough_query);
    }

    if api_format == "gemini_native" {
        let model = crate::proxy::providers::transform_gemini::extract_gemini_model(body)
            .unwrap_or("unknown");
        // Accept both bare ids (`gemini-2.5-pro`) and the resource-name
        // form (`models/gemini-2.5-pro`) that Gemini SDKs emit. See
        // `normalize_gemini_model_id` for rationale.
        let model = crate::proxy::gemini_url::normalize_gemini_model_id(model);
        let is_stream = body
            .get("stream")
            .and_then(|value| value.as_bool())
            .unwrap_or(false);
        let target_path = if is_stream {
            format!("/v1beta/models/{model}:streamGenerateContent")
        } else {
            format!("/v1beta/models/{model}:generateContent")
        };

        let rewritten_query = merge_query_params(
            passthrough_query.as_deref(),
            if is_stream { Some("alt=sse") } else { None },
        );

        let rewritten = match rewritten_query.as_deref() {
            Some(query) if !query.is_empty() => format!("{target_path}?{query}"),
            _ => target_path,
        };

        return (rewritten, rewritten_query);
    }

    let target_path = if is_copilot && api_format == "openai_responses" {
        "/v1/responses"
    } else if is_copilot {
        "/chat/completions"
    } else if api_format == "openai_responses" {
        "/v1/responses"
    } else {
        "/v1/chat/completions"
    };

    let rewritten = match passthrough_query.as_deref() {
        Some(query) if !query.is_empty() => format!("{target_path}?{query}"),
        _ => target_path.to_string(),
    };

    (rewritten, passthrough_query)
}

pub(super) fn merge_query_params(
    base_query: Option<&str>,
    extra_param: Option<&str>,
) -> Option<String> {
    let mut params: Vec<String> = base_query
        .into_iter()
        .flat_map(|query| query.split('&'))
        .filter(|pair| !pair.is_empty())
        .filter(|pair| !pair.starts_with("alt="))
        .map(ToString::to_string)
        .collect();

    if let Some(extra_param) = extra_param {
        params.push(extra_param.to_string());
    }

    if params.is_empty() {
        None
    } else {
        Some(params.join("&"))
    }
}

pub(super) fn append_query_to_full_url(base_url: &str, query: Option<&str>) -> String {
    match query {
        Some(query) if !query.is_empty() => {
            if base_url.contains('?') {
                format!("{base_url}&{query}")
            } else {
                format!("{base_url}?{query}")
            }
        }
        _ => base_url.to_string(),
    }
}

#[derive(Clone, Copy)]
pub(super) enum CodexStandaloneEndpoint {
    AlphaSearch,
    ImagesGenerations,
    ImagesEdits,
}

impl CodexStandaloneEndpoint {
    pub(super) fn from_effective_endpoint(endpoint: &str) -> Option<Self> {
        match split_endpoint_and_query(endpoint).0 {
            "/alpha/search" => Some(Self::AlphaSearch),
            "/images/generations" => Some(Self::ImagesGenerations),
            "/images/edits" => Some(Self::ImagesEdits),
            _ => None,
        }
    }

    pub(super) fn path(self) -> &'static str {
        match self {
            Self::AlphaSearch => "/alpha/search",
            Self::ImagesGenerations => "/images/generations",
            Self::ImagesEdits => "/images/edits",
        }
    }

    pub(super) fn label(self) -> &'static str {
        match self {
            Self::AlphaSearch => "Codex Alpha Search",
            Self::ImagesGenerations => "Codex Images generations",
            Self::ImagesEdits => "Codex Images edits",
        }
    }

    pub(super) fn full_url_hint(self) -> &'static str {
        match self {
            Self::AlphaSearch => "/responses",
            Self::ImagesGenerations | Self::ImagesEdits => {
                "/responses, /chat/completions, /images/generations, or /images/edits"
            }
        }
    }

    /// Full-URL suffixes that unambiguously locate this endpoint's sibling.
    ///
    /// Order matters: a longer suffix must precede any suffix it ends with
    /// (`/responses/compact` before `/responses`), otherwise the shorter one
    /// wins and the rewrite keeps a stray `/compact` segment.
    pub(super) fn source_suffixes(self) -> &'static [&'static str] {
        match self {
            Self::AlphaSearch => &["/responses/compact", "/responses"],
            // Both Images routes live next to each other, so a full URL pasted
            // for either one is a valid source for the other.
            Self::ImagesGenerations | Self::ImagesEdits => &[
                "/images/generations",
                "/images/edits",
                "/chat/completions",
                "/responses/compact",
                "/responses",
            ],
        }
    }

    pub(super) fn source_suffix(self, parsed_path: &str) -> Option<&'static str> {
        // Match the case-insensitive pasted-endpoint check. Only normalize for
        // matching; the rewrite keeps the original URL prefix and query intact.
        let parsed_path = parsed_path.to_ascii_lowercase();
        self.source_suffixes()
            .iter()
            .copied()
            .find(|suffix| parsed_path.ends_with(suffix))
    }

    /// Whether a base URL (full-URL switch off) already ends in one of this
    /// endpoint's source suffixes, i.e. was pasted as a complete endpoint URL.
    pub(super) fn base_url_is_source_endpoint(self, base_url: &str) -> bool {
        self.source_suffixes()
            .iter()
            .any(|suffix| base_url_is_full_endpoint(base_url, suffix))
    }
}

/// Derive Codex standalone sibling endpoints from a provider configured with a
/// complete Codex-compatible API URL.
///
/// Full-URL mode normally means "use this exact URL". That is correct for the
/// request type it was configured for, but standalone Codex protocols cannot be
/// posted to chat/responses endpoints. Only rewrite URL shapes whose sibling
/// endpoint is unambiguous; opaque full URLs fail closed instead of leaking the
/// payload to an unrelated route.
pub(super) fn rewrite_codex_standalone_full_url(
    base_url: &str,
    request_query: Option<&str>,
    endpoint: CodexStandaloneEndpoint,
) -> Result<String, ProxyError> {
    let trimmed = base_url.trim();
    let parsed = url::Url::parse(trimmed).map_err(|_| {
        ProxyError::ConfigError(format!("{} requires a valid full URL", endpoint.label()))
    })?;

    // Fragments are never sent in HTTP requests. Drop one before splitting the
    // query so an accidental fragment cannot move the incoming query behind `#`.
    let without_fragment = trimmed
        .split_once('#')
        .map_or(trimmed, |(head, _fragment)| head);
    let (url_without_query, base_query) = without_fragment
        .split_once('?')
        .map_or((without_fragment, None), |(head, query)| {
            (head, Some(query))
        });
    let url_without_query = url_without_query.trim_end_matches('/');

    let parsed_path = parsed.path().trim_end_matches('/').to_string();
    let suffix = endpoint.source_suffix(&parsed_path).ok_or_else(|| {
        ProxyError::ConfigError(format!(
            "{} cannot derive {} from an opaque full URL; use a base URL or a full URL ending in {}",
            endpoint.label(),
            endpoint.path(),
            endpoint.full_url_hint()
        ))
    })?;

    let prefix_len = url_without_query
        .len()
        .checked_sub(suffix.len())
        .ok_or_else(|| ProxyError::ConfigError("Invalid Codex full URL".to_string()))?;
    let mut rewritten = format!("{}{}", &url_without_query[..prefix_len], endpoint.path());

    let request_query = request_query.filter(|query| !query.is_empty());
    let base_query = base_query.filter(|query| !query.is_empty());
    match (base_query, request_query) {
        (Some(base), Some(request)) => rewritten.push_str(&format!("?{base}&{request}")),
        (Some(base), None) => rewritten.push_str(&format!("?{base}")),
        (None, Some(request)) => rewritten.push_str(&format!("?{request}")),
        (None, None) => {}
    }

    Ok(rewritten)
}

pub(super) fn is_streaming_request(
    endpoint: &str,
    body: &Value,
    headers: &axum::http::HeaderMap,
) -> bool {
    if body
        .get("stream")
        .and_then(|value| value.as_bool())
        .unwrap_or(false)
    {
        return true;
    }

    if endpoint.contains("streamGenerateContent") || endpoint.contains("alt=sse") {
        return true;
    }

    headers
        .get(axum::http::header::ACCEPT)
        .and_then(|value| value.to_str().ok())
        .map(|accept| accept.contains("text/event-stream"))
        .unwrap_or(false)
}
