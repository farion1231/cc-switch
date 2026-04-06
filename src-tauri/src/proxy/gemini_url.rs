//! Gemini Native URL helpers.
//!
//! Normalizes legacy Gemini/OpenAI-compatible base URLs into the canonical
//! Gemini Native `models/*:generateContent` endpoints.

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
    let (_, path) = split_origin_and_path(base_without_query);

    if path.is_empty() || path == "/" {
        return true;
    }

    let path = path.trim_end_matches('/');
    path.contains("/v1beta/models/")
        || path.contains("/v1/models/")
        || path.contains("/models/")
        || path.ends_with("/v1beta")
        || path.ends_with("/v1")
        || path.ends_with("/v1beta/models")
        || path.ends_with("/v1/models")
        || path.ends_with("/models")
        || path.ends_with("/v1beta/openai")
        || path.ends_with("/v1/openai")
        || path.ends_with("/openai")
        || path.ends_with("/v1beta/openai/chat/completions")
        || path.ends_with("/v1/openai/chat/completions")
        || path.ends_with("/openai/chat/completions")
        || path.ends_with("/v1beta/openai/responses")
        || path.ends_with("/v1/openai/responses")
        || path.ends_with("/openai/responses")
        || path.contains(":generateContent")
        || path.contains(":streamGenerateContent")
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
    use super::{build_gemini_native_url, resolve_gemini_native_url};

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
}
