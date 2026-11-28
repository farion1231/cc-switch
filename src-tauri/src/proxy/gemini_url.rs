//! Gemini Native URL helpers.
//!
//! Normalizes legacy Gemini/OpenAI-compatible base URLs into the canonical
//! Gemini Native `models/*:generateContent` endpoints.

/// Normalize a Gemini model identifier to its bare form, stripping an
/// optional leading `models/` (and any leading `/`) so that the value can
/// be safely interpolated into a URL template like
/// `/v1beta/models/{model}:generateContent`.
///
/// Gemini SDKs and documentation commonly surface model ids as
/// `models/gemini-2.5-pro` (the resource-name form). Passing that value
/// through to the format string would otherwise yield a doubled prefix
/// like `/v1beta/models/models/gemini-2.5-pro:generateContent`, which is
/// rejected by the upstream API and turns any health check or live
/// request into a false negative.
pub fn normalize_gemini_model_id(model: &str) -> &str {
    let trimmed = model.strip_prefix('/').unwrap_or(model);
    trimmed.strip_prefix("models/").unwrap_or(trimmed)
}

pub fn resolve_gemini_native_url(base_url: &str, endpoint: &str, is_full_url: bool) -> String {
    if !is_full_url || should_normalize_gemini_full_url(base_url) {
        return build_gemini_native_url(base_url, endpoint);
    }

    let base_url = base_url
        .split_once('#')
        .map_or(base_url, |(base, _)| base)
        .trim_end_matches('/');
    let (base_without_query, base_query) = split_query(base_url);
    let (_, endpoint_query) = split_query(endpoint);

    let mut url = base_without_query.to_string();
    if let Some(query) = merge_queries(base_query, endpoint_query) {
        url.push('?');
        url.push_str(&query);
    }

    url
}

pub fn build_gemini_native_url(base_url: &str, endpoint: &str) -> String {
    let base_url = base_url
        .split_once('#')
        .map_or(base_url, |(base, _)| base)
        .trim_end_matches('/');
    let (base_without_query, base_query) = split_query(base_url);
    let (endpoint_without_query, endpoint_query) = split_query(endpoint);

    let endpoint_path = format!("/{}", endpoint_without_query.trim_start_matches('/'));
    let (origin, raw_path) = split_origin_and_path(base_without_query);
    let prefix_path = normalize_gemini_base_path(raw_path);

    let mut url = if prefix_path.is_empty() {
        format!("{origin}{endpoint_path}")
    } else {
        format!("{origin}{prefix_path}{endpoint_path}")
    };

    if let Some(query) = merge_queries(base_query, endpoint_query) {
        url.push('?');
        url.push_str(&query);
    }

    url
}

fn should_normalize_gemini_full_url(base_url: &str) -> bool {
    let base_url = base_url
        .split_once('#')
        .map_or(base_url, |(base, _)| base)
        .trim_end_matches('/');
    let (base_without_query, _) = split_query(base_url);
    let (origin, path) = split_origin_and_path(base_without_query);

    if path.is_empty() || path == "/" {
        return true;
    }

    let path = path.trim_end_matches('/');
    let on_google_host = is_google_gemini_host(extract_host(origin));

    if matches_vertex_ai_publisher_model_path(path) {
        return false;
    }

    // Unconditional layer: only paths whose grammar is *intrinsically*
    // Gemini-specific — the `/models/...:generateContent` method-call
    // shape and the deep OpenAI-compat endpoints (`/openai/chat/completions`,
    // `/openai/responses`) that are implausibly used as a relay's fixed
    // terminal path — get rewritten regardless of host.
    if matches_structured_gemini_models_path(path)
        || path.ends_with("/v1beta/openai/chat/completions")
        || path.ends_with("/v1beta/openai/responses")
        || path.ends_with("/openai/chat/completions")
        || path.ends_with("/openai/responses")
        || path.ends_with("/v1/openai/chat/completions")
        || path.ends_with("/v1/openai/responses")
    {
        return true;
    }

    // All other version / resource-root suffixes — `/v1beta`, `/v1`,
    // `/models`, `/openai`, and variants — could legitimately be an
    // opaque relay's fixed endpoint (`https://relay.example/custom/v1beta`
    // is a real deployment shape, even if uncommon outside Google's
    // ecosystem). Only rewrite when the host itself is Google's Gemini
    // or Vertex AI endpoint.
    if on_google_host
        && (path.ends_with("/v1beta")
            || path.ends_with("/v1beta/models")
            || path.ends_with("/v1beta/openai")
            || path.ends_with("/v1")
            || path.ends_with("/v1/models")
            || path.ends_with("/models")
            || path.ends_with("/v1/openai")
            || path.ends_with("/openai"))
    {
        return true;
    }

    false
}

/// Extract the host portion of an origin like `https://host:port` or
/// `https://host`. Returns an empty string if no host can be found (e.g.
/// bare `http://`).
fn extract_host(origin: &str) -> &str {
    let after_scheme = origin.split_once("://").map_or(origin, |(_, rest)| rest);
    // authority may carry credentials (`user:pass@host`) and a port
    // (`host:port`). Strip userinfo first, then port.
    let without_userinfo = after_scheme
        .rsplit_once('@')
        .map_or(after_scheme, |(_, h)| h);
    let without_port = without_userinfo
        .split_once(':')
        .map_or(without_userinfo, |(h, _)| h);
    // Strip trailing `/` defensively (split_origin_and_path already handled
    // it, but this helper may be reused elsewhere).
    without_port.trim_end_matches('/')
}

/// Returns true when `host` is one of Google's Gemini / Vertex AI endpoints.
/// Case-insensitive. Requires exact match or a real `-aiplatform.googleapis.com`
/// subdomain suffix — not a substring match, so lookalikes like
/// `aiplatform.example.com` are rejected.
fn is_google_gemini_host(host: &str) -> bool {
    if host.is_empty() {
        return false;
    }
    let host_lower = host.to_ascii_lowercase();
    host_lower == "generativelanguage.googleapis.com"
        || host_lower == "aiplatform.googleapis.com"
        || host_lower.ends_with("-aiplatform.googleapis.com")
}

fn split_query(input: &str) -> (&str, Option<&str>) {
    input
        .split_once('?')
        .map_or((input, None), |(path, query)| (path, Some(query)))
}

fn split_origin_and_path(base_url: &str) -> (&str, &str) {
    let Some(scheme_sep) = base_url.find("://") else {
        return (base_url, "");
    };
    let authority_start = scheme_sep + 3;
    let Some(path_start_rel) = base_url[authority_start..].find('/') else {
        return (base_url, "");
    };
    let path_start = authority_start + path_start_rel;
    (&base_url[..path_start], &base_url[path_start..])
}

fn normalize_gemini_base_path(path: &str) -> String {
    let path = path.trim_end_matches('/');
    if path.is_empty() || path == "/" {
        return String::new();
    }

    for marker in ["/v1beta/models/", "/v1/models/", "/models/"] {
        if let Some(index) = path.find(marker) {
            return normalize_prefix(&path[..index]);
        }
    }

    for suffix in [
        "/v1beta/openai/chat/completions",
        "/v1/openai/chat/completions",
        "/openai/chat/completions",
        "/v1beta/openai/responses",
        "/v1/openai/responses",
        "/openai/responses",
        "/v1beta/openai",
        "/v1/openai",
        "/openai",
        "/v1beta/models",
        "/v1/models",
        "/models",
        "/v1beta",
        "/v1",
    ] {
        if path == suffix {
            return String::new();
        }
        if let Some(prefix) = path.strip_suffix(suffix) {
            return normalize_prefix(prefix);
        }
    }

    path.to_string()
}

fn normalize_prefix(prefix: &str) -> String {
    let prefix = prefix.trim_end_matches('/');
    if prefix.is_empty() || prefix == "/" {
        String::new()
    } else {
        prefix.to_string()
    }
}

/// Returns true when `path` contains a `/models/` segment followed by a
/// canonical Gemini method call (`*:generateContent` or
/// `*:streamGenerateContent`). The `/models/` segment alone is not enough:
/// opaque relay routes such as `/v1/models/invoke` or `/custom/models/foo`
/// also contain `/models/` but are not Gemini-structured and must not be
/// rewritten.
fn matches_structured_gemini_models_path(path: &str) -> bool {
    let mut cursor = path;
    while let Some(idx) = cursor.find("/models/") {
        let after = &cursor[idx + "/models/".len()..];
        if after.contains(":generateContent") || after.contains(":streamGenerateContent") {
            return true;
        }
        // Advance past this `/models/` occurrence so a later Gemini-style
        // segment in the same path (unusual but cheap to handle) can still
        // match.
        cursor = &cursor[idx + "/models/".len()..];
    }
    false
}

/// Vertex AI endpoint paths include project/location/publisher routing before
/// `models/*:generateContent`; in full-URL mode that routing is user-provided
/// and must not be collapsed into the public Gemini `/v1beta/models/*` shape.
fn matches_vertex_ai_publisher_model_path(path: &str) -> bool {
    let Some(projects_index) = path.find("/projects/") else {
        return false;
    };
    let Some(publisher_models_index) = path.find("/publishers/google/models/") else {
        return false;
    };

    if projects_index >= publisher_models_index
        || !path[projects_index..publisher_models_index].contains("/locations/")
    {
        return false;
    }

    let after_model = &path[publisher_models_index + "/publishers/google/models/".len()..];
    after_model.contains(":generateContent") || after_model.contains(":streamGenerateContent")
}

fn merge_queries(base_query: Option<&str>, endpoint_query: Option<&str>) -> Option<String> {
    let parts: Vec<&str> = [base_query, endpoint_query]
        .into_iter()
        .flatten()
        .flat_map(|query| query.split('&'))
        .filter(|part| !part.is_empty())
        .collect();

    if parts.is_empty() {
        None
    } else {
        Some(parts.join("&"))
    }
}

#[cfg(test)]
mod tests {
    use super::{build_gemini_native_url, normalize_gemini_model_id, resolve_gemini_native_url};

    const STREAM_ENDPOINT: &str = "/v1beta/models/gemini-2.5-flash:streamGenerateContent?alt=sse";

    #[test]
    fn build_gemini_native_url_cases() {
        for (case, base, endpoint, expected) in [
            (
                "strips_version_root_for_official_base",
                "https://generativelanguage.googleapis.com/v1beta",
                "/v1beta/models/gemini-2.5-pro:generateContent",
                "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-pro:generateContent",
            ),
            (
                "strips_openai_compat_path_for_official_base",
                "https://generativelanguage.googleapis.com/v1beta/openai/chat/completions",
                "/v1beta/models/gemini-2.5-pro:generateContent",
                "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-pro:generateContent",
            ),
            (
                "preserves_custom_proxy_prefix_while_stripping_openai_suffix",
                "https://proxy.example.com/google/v1beta/openai/chat/completions",
                STREAM_ENDPOINT,
                "https://proxy.example.com/google/v1beta/models/gemini-2.5-flash:streamGenerateContent?alt=sse",
            ),
            (
                "strips_model_method_path_from_full_url_base",
                "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-pro:generateContent",
                STREAM_ENDPOINT,
                "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-flash:streamGenerateContent?alt=sse",
            ),
        ] {
            assert_eq!(build_gemini_native_url(base, endpoint), expected, "{case}");
        }
    }

    #[test]
    fn resolve_gemini_native_url_normalizes_structured_and_non_full_urls() {
        for (case, base, is_full_url, expected) in [
            (
                "structured_v1beta_models_method_full_url",
                "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-pro:generateContent",
                true,
                "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-flash:streamGenerateContent?alt=sse",
            ),
            (
                "structured_v1_models_method_on_relay_full_url",
                "https://relay.example/v1/models/gemini-2.5-pro:generateContent",
                true,
                "https://relay.example/v1beta/models/gemini-2.5-flash:streamGenerateContent?alt=sse",
            ),
            (
                "google_host_v1_suffix",
                "https://generativelanguage.googleapis.com/v1",
                true,
                "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-flash:streamGenerateContent?alt=sse",
            ),
            (
                "google_host_models_suffix",
                "https://generativelanguage.googleapis.com/models",
                true,
                "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-flash:streamGenerateContent?alt=sse",
            ),
            (
                "google_host_v1beta_suffix",
                "https://generativelanguage.googleapis.com/v1beta",
                true,
                "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-flash:streamGenerateContent?alt=sse",
            ),
            (
                "vertex_regional_aiplatform_v1_suffix",
                "https://us-central1-aiplatform.googleapis.com/v1",
                true,
                "https://us-central1-aiplatform.googleapis.com/v1beta/models/gemini-2.5-flash:streamGenerateContent?alt=sse",
            ),
            // Non-full-URL mode strips version suffixes even on non-Google relays
            // (intentional asymmetry with opaque full-URL mode).
            (
                "non_full_url_strips_v1beta_relay_suffix",
                "https://relay.example/custom/v1beta",
                false,
                "https://relay.example/custom/v1beta/models/gemini-2.5-flash:streamGenerateContent?alt=sse",
            ),
            (
                "non_full_url_strips_v1_relay_suffix",
                "https://relay.example/custom/v1",
                false,
                "https://relay.example/custom/v1beta/models/gemini-2.5-flash:streamGenerateContent?alt=sse",
            ),
        ] {
            assert_eq!(
                resolve_gemini_native_url(base, STREAM_ENDPOINT, is_full_url),
                expected,
                "{case}"
            );
        }
    }

    #[test]
    fn resolve_gemini_native_url_preserves_opaque_and_vertex_full_urls() {
        for (case, base, expected) in [
            (
                "opaque_custom_path",
                "https://relay.example/custom/generate-content",
                "https://relay.example/custom/generate-content?alt=sse",
            ),
            (
                "cloudflare_vertex_ai_publisher_path",
                "https://gateway.ai.cloudflare.com/v1/account/gateway/google-vertex-ai/v1/projects/project/locations/us-central1/publishers/google/models/gemini-3.1-pro-preview:streamGenerateContent",
                "https://gateway.ai.cloudflare.com/v1/account/gateway/google-vertex-ai/v1/projects/project/locations/us-central1/publishers/google/models/gemini-3.1-pro-preview:streamGenerateContent?alt=sse",
            ),
            (
                "opaque_models_segment",
                "https://relay.example/custom/models/invoke",
                "https://relay.example/custom/models/invoke?alt=sse",
            ),
            (
                "opaque_v1_models_prefix",
                "https://relay.example/v1/models/invoke",
                "https://relay.example/v1/models/invoke?alt=sse",
            ),
            (
                "opaque_v1beta_models_prefix",
                "https://relay.example/v1beta/models/route",
                "https://relay.example/v1beta/models/route?alt=sse",
            ),
            (
                "opaque_v1_suffix",
                "https://relay.example/custom/v1",
                "https://relay.example/custom/v1?alt=sse",
            ),
            (
                "opaque_models_suffix",
                "https://relay.example/custom/models",
                "https://relay.example/custom/models?alt=sse",
            ),
            (
                "opaque_v1_models_suffix",
                "https://relay.example/custom/v1/models",
                "https://relay.example/custom/v1/models?alt=sse",
            ),
            (
                "opaque_openai_suffix_non_google",
                "https://relay.example/custom/openai",
                "https://relay.example/custom/openai?alt=sse",
            ),
            // Exact/suffix Google-host check: lookalike host must not be normalized.
            (
                "non_google_aiplatform_lookalike",
                "https://aiplatform.example.com/v1",
                "https://aiplatform.example.com/v1?alt=sse",
            ),
            (
                "opaque_bare_v1beta_suffix",
                "https://relay.example/custom/v1beta",
                "https://relay.example/custom/v1beta?alt=sse",
            ),
            (
                "opaque_v1beta_models_suffix_no_method",
                "https://relay.example/custom/v1beta/models",
                "https://relay.example/custom/v1beta/models?alt=sse",
            ),
        ] {
            assert_eq!(
                resolve_gemini_native_url(base, STREAM_ENDPOINT, true),
                expected,
                "{case}"
            );
        }
    }

    #[test]
    fn normalize_model_id_cases() {
        for (case, input, expected) in [
            (
                "normalize_model_id_strips_models_prefix",
                "models/gemini-2.5-pro",
                "gemini-2.5-pro",
            ),
            (
                "normalize_model_id_leaves_bare_id_unchanged",
                "gemini-2.5-pro",
                "gemini-2.5-pro",
            ),
            // e.g. tuned model resource like `models/gemini-2.5-pro/tunedModels/xxx`
            // — the caller asked for a specific tuned model resource, keep its
            // identity intact after stripping only the single leading prefix.
            (
                "normalize_model_id_preserves_nested_slashes_after_prefix",
                "models/tunedModels/my-tuned",
                "tunedModels/my-tuned",
            ),
            (
                "normalize_model_id_tolerates_leading_slash",
                "/models/gemini-2.5-flash",
                "gemini-2.5-flash",
            ),
            // Edge: caller has no model at all. Pass through so the URL error
            // surfaces at the request layer rather than producing a misleading
            // empty segment here.
            ("normalize_model_id_preserves_empty_input", "", ""),
        ] {
            assert_eq!(normalize_gemini_model_id(input), expected, "{case}");
        }
    }
}
