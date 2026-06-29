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

    #[test]
    fn strips_version_root_for_official_base() {
        let url = build_gemini_native_url(
            "https://generativelanguage.googleapis.com/v1beta",
            "/v1beta/models/gemini-2.5-pro:generateContent",
        );

        assert_eq!(
            url,
            "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-pro:generateContent"
        );
    }

    #[test]
    fn strips_openai_compat_path_for_official_base() {
        let url = build_gemini_native_url(
            "https://generativelanguage.googleapis.com/v1beta/openai/chat/completions",
            "/v1beta/models/gemini-2.5-pro:generateContent",
        );

        assert_eq!(
            url,
            "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-pro:generateContent"
        );
    }

    #[test]
    fn preserves_custom_proxy_prefix_while_stripping_openai_suffix() {
        let url = build_gemini_native_url(
            "https://proxy.example.com/google/v1beta/openai/chat/completions",
            "/v1beta/models/gemini-2.5-flash:streamGenerateContent?alt=sse",
        );

        assert_eq!(
            url,
            "https://proxy.example.com/google/v1beta/models/gemini-2.5-flash:streamGenerateContent?alt=sse"
        );
    }

    #[test]
    fn strips_model_method_path_from_full_url_base() {
        let url = build_gemini_native_url(
            "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-pro:generateContent",
            "/v1beta/models/gemini-2.5-flash:streamGenerateContent?alt=sse",
        );

        assert_eq!(
            url,
            "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-flash:streamGenerateContent?alt=sse"
        );
    }

    #[test]
    fn resolves_structured_full_url_by_normalizing_to_requested_method() {
        let url = resolve_gemini_native_url(
            "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-pro:generateContent",
            "/v1beta/models/gemini-2.5-flash:streamGenerateContent?alt=sse",
            true,
        );

        assert_eq!(
            url,
            "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-flash:streamGenerateContent?alt=sse"
        );
    }

    #[test]
    fn resolves_opaque_full_url_without_appending_gemini_models_path() {
        let url = resolve_gemini_native_url(
            "https://relay.example/custom/generate-content",
            "/v1beta/models/gemini-2.5-flash:streamGenerateContent?alt=sse",
            true,
        );

        assert_eq!(url, "https://relay.example/custom/generate-content?alt=sse");
    }

    #[test]
    fn preserves_cloudflare_vertex_ai_full_url_with_action() {
        let url = resolve_gemini_native_url(
            "https://gateway.ai.cloudflare.com/v1/account/gateway/google-vertex-ai/v1/projects/project/locations/us-central1/publishers/google/models/gemini-3.1-pro-preview:streamGenerateContent",
            "/v1beta/models/gemini-2.5-flash:streamGenerateContent?alt=sse",
            true,
        );

        assert_eq!(
            url,
            "https://gateway.ai.cloudflare.com/v1/account/gateway/google-vertex-ai/v1/projects/project/locations/us-central1/publishers/google/models/gemini-3.1-pro-preview:streamGenerateContent?alt=sse"
        );
    }

    #[test]
    fn preserves_opaque_full_url_containing_models_segment() {
        let url = resolve_gemini_native_url(
            "https://relay.example/custom/models/invoke",
            "/v1beta/models/gemini-2.5-flash:streamGenerateContent?alt=sse",
            true,
        );

        assert_eq!(url, "https://relay.example/custom/models/invoke?alt=sse");
    }

    /// Regression: a relay whose fixed path starts with `/v1/models/` is an
    /// opaque route, not a Gemini-structured endpoint. The previous
    /// heuristic matched any `contains("/v1/models/")` and rewrote it to
    /// `/v1beta/models/{model}:generateContent`, dropping the relay's own
    /// route component (`/invoke`) and sending traffic to the wrong place.
    #[test]
    fn preserves_opaque_full_url_with_v1_models_prefix() {
        let url = resolve_gemini_native_url(
            "https://relay.example/v1/models/invoke",
            "/v1beta/models/gemini-2.5-flash:streamGenerateContent?alt=sse",
            true,
        );

        assert_eq!(url, "https://relay.example/v1/models/invoke?alt=sse");
    }

    /// Same regression, `/v1beta/models/` variant — relays may use Google's
    /// path layout defensively for routing while still being opaque.
    #[test]
    fn preserves_opaque_full_url_with_v1beta_models_prefix() {
        let url = resolve_gemini_native_url(
            "https://relay.example/v1beta/models/route",
            "/v1beta/models/gemini-2.5-flash:streamGenerateContent?alt=sse",
            true,
        );

        assert_eq!(url, "https://relay.example/v1beta/models/route?alt=sse");
    }

    /// Counter-case: a full URL that *does* carry a genuine Gemini method
    /// segment (`:generateContent`) under `/v1/models/` should still be
    /// recognized as structured and normalized to the requested model.
    #[test]
    fn normalizes_structured_v1_models_path_with_method_segment() {
        let url = resolve_gemini_native_url(
            "https://relay.example/v1/models/gemini-2.5-pro:generateContent",
            "/v1beta/models/gemini-2.5-flash:streamGenerateContent?alt=sse",
            true,
        );

        assert_eq!(
            url,
            "https://relay.example/v1beta/models/gemini-2.5-flash:streamGenerateContent?alt=sse"
        );
    }

    // ------------------------------------------------------------------
    // Google-host whitelist tests (generic REST suffix handling)
    //
    // Generic REST conventions like `/v1`, `/models`, `/openai` legitimately
    // appear on opaque relays. `should_normalize_gemini_full_url` only
    // treats these as structured Gemini endpoints when the host itself is
    // Google's Gemini or Vertex AI endpoint.
    // ------------------------------------------------------------------

    /// Regression: a relay whose fixed path ends with `/v1` (a ubiquitous
    /// REST convention) used to be rewritten to
    /// `/v1beta/models/{model}:generateContent`, dropping the relay's own
    /// `/v1` endpoint.
    #[test]
    fn preserves_opaque_full_url_with_v1_suffix() {
        let url = resolve_gemini_native_url(
            "https://relay.example/custom/v1",
            "/v1beta/models/gemini-2.5-flash:streamGenerateContent?alt=sse",
            true,
        );

        assert_eq!(url, "https://relay.example/custom/v1?alt=sse");
    }

    /// Companion case: bare `/models` suffix on a non-Google host is a
    /// generic REST path, not a Gemini-structured endpoint.
    #[test]
    fn preserves_opaque_full_url_with_models_suffix() {
        let url = resolve_gemini_native_url(
            "https://relay.example/custom/models",
            "/v1beta/models/gemini-2.5-flash:streamGenerateContent?alt=sse",
            true,
        );

        assert_eq!(url, "https://relay.example/custom/models?alt=sse");
    }

    /// Companion case: `/v1/models` — same ambiguity as `/models`, with the
    /// version prefix. Must stay as-is on non-Google hosts.
    #[test]
    fn preserves_opaque_full_url_with_v1_models_suffix() {
        let url = resolve_gemini_native_url(
            "https://relay.example/custom/v1/models",
            "/v1beta/models/gemini-2.5-flash:streamGenerateContent?alt=sse",
            true,
        );

        assert_eq!(url, "https://relay.example/custom/v1/models?alt=sse");
    }

    /// Companion case: a relay that exposes an `/openai` compatibility
    /// surface without the deep `/openai/chat/completions` path. Must stay
    /// as-is on non-Google hosts.
    #[test]
    fn preserves_opaque_full_url_with_openai_suffix_on_non_google_host() {
        let url = resolve_gemini_native_url(
            "https://relay.example/custom/openai",
            "/v1beta/models/gemini-2.5-flash:streamGenerateContent?alt=sse",
            true,
        );

        assert_eq!(url, "https://relay.example/custom/openai?alt=sse");
    }

    /// Counter-case: `/v1` on the official Gemini host must still be
    /// normalized to the full `/v1beta/models/...` endpoint — users who
    /// paste `https://generativelanguage.googleapis.com/v1` as their base
    /// URL expect the proxy to resolve the method path.
    #[test]
    fn normalizes_google_host_with_v1_suffix() {
        let url = resolve_gemini_native_url(
            "https://generativelanguage.googleapis.com/v1",
            "/v1beta/models/gemini-2.5-flash:streamGenerateContent?alt=sse",
            true,
        );

        assert_eq!(
            url,
            "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-flash:streamGenerateContent?alt=sse"
        );
    }

    /// Counter-case: `/models` on the official Gemini host is recognized
    /// and normalized.
    #[test]
    fn normalizes_google_host_with_models_suffix() {
        let url = resolve_gemini_native_url(
            "https://generativelanguage.googleapis.com/models",
            "/v1beta/models/gemini-2.5-flash:streamGenerateContent?alt=sse",
            true,
        );

        assert_eq!(
            url,
            "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-flash:streamGenerateContent?alt=sse"
        );
    }

    /// Counter-case: Vertex AI regional endpoints live under
    /// `*-aiplatform.googleapis.com`. Those should also be treated as
    /// Google-host for the whitelist.
    #[test]
    fn normalizes_vertex_aiplatform_host_with_v1_suffix() {
        let url = resolve_gemini_native_url(
            "https://us-central1-aiplatform.googleapis.com/v1",
            "/v1beta/models/gemini-2.5-flash:streamGenerateContent?alt=sse",
            true,
        );

        assert_eq!(
            url,
            "https://us-central1-aiplatform.googleapis.com/v1beta/models/gemini-2.5-flash:streamGenerateContent?alt=sse"
        );
    }

    /// Safety: the Google-host whitelist must do an exact/suffix match, not
    /// a `contains`. A lookalike host like `aiplatform.example.com` must
    /// NOT be treated as Google.
    #[test]
    fn preserves_non_google_aiplatform_lookalike_host() {
        let url = resolve_gemini_native_url(
            "https://aiplatform.example.com/v1",
            "/v1beta/models/gemini-2.5-flash:streamGenerateContent?alt=sse",
            true,
        );

        assert_eq!(url, "https://aiplatform.example.com/v1?alt=sse");
    }

    /// Regression: `/v1beta` by itself is Google-conventional but not
    /// literally impossible on other hosts. An opaque relay fronting a
    /// non-Gemini service at `https://relay.example/custom/v1beta` would
    /// be silently rewritten if `/v1beta` were classified as unconditional
    /// structured Gemini. Require the Google-host whitelist instead.
    #[test]
    fn preserves_opaque_full_url_with_bare_v1beta_suffix() {
        let url = resolve_gemini_native_url(
            "https://relay.example/custom/v1beta",
            "/v1beta/models/gemini-2.5-flash:streamGenerateContent?alt=sse",
            true,
        );

        assert_eq!(url, "https://relay.example/custom/v1beta?alt=sse");
    }

    /// Companion case: `/v1beta/models` (no method segment) on a non-Google
    /// host stays as-is too.
    #[test]
    fn preserves_opaque_full_url_with_v1beta_models_suffix_no_method() {
        let url = resolve_gemini_native_url(
            "https://relay.example/custom/v1beta/models",
            "/v1beta/models/gemini-2.5-flash:streamGenerateContent?alt=sse",
            true,
        );

        assert_eq!(url, "https://relay.example/custom/v1beta/models?alt=sse");
    }

    /// Counter-case: `/v1beta` on the official Gemini host must still
    /// normalize — this is the canonical base URL shape users paste from
    /// AI Studio documentation.
    #[test]
    fn normalizes_google_host_with_v1beta_suffix() {
        let url = resolve_gemini_native_url(
            "https://generativelanguage.googleapis.com/v1beta",
            "/v1beta/models/gemini-2.5-flash:streamGenerateContent?alt=sse",
            true,
        );

        assert_eq!(
            url,
            "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-flash:streamGenerateContent?alt=sse"
        );
    }

    /// Regression guard: in non-full-URL mode, a versioned third-party
    /// relay base must have its `/v1beta` suffix **stripped** so the
    /// appended standard endpoint (`/v1beta/models/{model}:method`) does
    /// not produce a doubled `/v1beta/v1beta/models/...` path. Non-full
    /// mode's contract is "base URL + cc-switch appends the canonical
    /// Gemini endpoint" — a user who wants a relay's custom namespace
    /// (e.g. `/v1/models/...`) must use full-URL mode instead.
    ///
    /// This test pins the intentional asymmetry with
    /// `preserves_opaque_full_url_with_bare_v1beta_suffix` (full-URL
    /// preserves, non-full strips) so nobody "fixes" one side into
    /// breaking the other.
    #[test]
    fn strips_versioned_relay_base_suffix_in_non_full_url_mode() {
        let url = resolve_gemini_native_url(
            "https://relay.example/custom/v1beta",
            "/v1beta/models/gemini-2.5-flash:streamGenerateContent?alt=sse",
            false,
        );

        assert_eq!(
            url,
            "https://relay.example/custom/v1beta/models/gemini-2.5-flash:streamGenerateContent?alt=sse"
        );
    }

    /// Companion case: `/v1` base suffix also stripped in non-full-URL
    /// mode regardless of host.
    #[test]
    fn strips_v1_relay_base_suffix_in_non_full_url_mode() {
        let url = resolve_gemini_native_url(
            "https://relay.example/custom/v1",
            "/v1beta/models/gemini-2.5-flash:streamGenerateContent?alt=sse",
            false,
        );

        assert_eq!(
            url,
            "https://relay.example/custom/v1beta/models/gemini-2.5-flash:streamGenerateContent?alt=sse"
        );
    }

    // ------------------------------------------------------------------
    // Model ID normalization tests.
    //
    // Gemini SDKs and documentation commonly surface model identifiers as
    // `models/gemini-2.5-pro` (resource-name form). If that value flows
    // straight into our URL builder, the format string
    // `/v1beta/models/{model}:generateContent` produces a doubled prefix
    // `/v1beta/models/models/gemini-2.5-pro:generateContent`, which is
    // rejected upstream. `normalize_gemini_model_id` is the single source
    // of truth callers should run the model through first.
    // ------------------------------------------------------------------

    #[test]
    fn normalize_model_id_strips_models_prefix() {
        assert_eq!(
            normalize_gemini_model_id("models/gemini-2.5-pro"),
            "gemini-2.5-pro"
        );
    }

    #[test]
    fn normalize_model_id_leaves_bare_id_unchanged() {
        assert_eq!(
            normalize_gemini_model_id("gemini-2.5-pro"),
            "gemini-2.5-pro"
        );
    }

    #[test]
    fn normalize_model_id_preserves_nested_slashes_after_prefix() {
        // e.g. tuned model resource like `models/gemini-2.5-pro/tunedModels/xxx`
        // — the caller asked for a specific tuned model resource, keep its
        // identity intact after stripping only the single leading prefix.
        assert_eq!(
            normalize_gemini_model_id("models/tunedModels/my-tuned"),
            "tunedModels/my-tuned"
        );
    }

    #[test]
    fn normalize_model_id_tolerates_leading_slash() {
        assert_eq!(
            normalize_gemini_model_id("/models/gemini-2.5-flash"),
            "gemini-2.5-flash"
        );
    }

    #[test]
    fn normalize_model_id_preserves_empty_input() {
        // Edge: caller has no model at all. Pass through so the URL error
        // surfaces at the request layer rather than producing a misleading
        // empty segment here.
        assert_eq!(normalize_gemini_model_id(""), "");
    }
}
