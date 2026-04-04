#[derive(Debug, Clone)]
pub struct BaseUrlInfo<'a> {
    pub already_has_v1: bool,
    /// `true` when `base_trimmed` is a bare origin with no path component.
    pub origin_only: bool,
    /// `base_trimmed` with a trailing `/v1` stripped (if present).
    pub copilot_base: &'a str,
}

impl<'a> BaseUrlInfo<'a> {
    pub fn new(base_trimmed: &'a str) -> Self {
        let already_has_v1 = base_trimmed.ends_with("/v1");
        let origin_only = match base_trimmed.split_once("://") {
            Some((_scheme, rest)) => !rest.contains('/'),
            None => !base_trimmed.contains('/'),
        };
        let copilot_base = base_trimmed.strip_suffix("/v1").unwrap_or(base_trimmed);

        Self {
            already_has_v1,
            origin_only,
            copilot_base,
        }
    }
}

pub fn is_openai_compat_endpoint(endpoint_trimmed: &str) -> bool {
    endpoint_trimmed.starts_with("chat/completions")
        || endpoint_trimmed.starts_with("responses")
        || endpoint_trimmed.starts_with("v1/chat/completions")
        || endpoint_trimmed.starts_with("v1/responses")
}

pub fn dedup_v1(url: &mut String) {
    while url.contains("/v1/v1") {
        *url = url.replace("/v1/v1", "/v1");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── BaseUrlInfo ──────────────────────────────────────────────

    #[test]
    fn test_origin_only_https() {
        let info = BaseUrlInfo::new("https://api.openai.com");
        assert!(info.origin_only);
        assert!(!info.already_has_v1);
        assert_eq!(info.copilot_base, "https://api.openai.com");
    }

    #[test]
    fn test_already_has_v1() {
        let info = BaseUrlInfo::new("https://api.openai.com/v1");
        assert!(!info.origin_only);
        assert!(info.already_has_v1);
        assert_eq!(info.copilot_base, "https://api.openai.com");
    }

    #[test]
    fn test_custom_prefix() {
        let info = BaseUrlInfo::new("https://relay.example/openai");
        assert!(!info.origin_only);
        assert!(!info.already_has_v1);
        assert_eq!(info.copilot_base, "https://relay.example/openai");
    }

    #[test]
    fn test_copilot_url() {
        let info = BaseUrlInfo::new("https://api.githubcopilot.com");
        assert!(info.origin_only);
        assert!(!info.already_has_v1);
        assert_eq!(info.copilot_base, "https://api.githubcopilot.com");
    }

    #[test]
    fn test_copilot_url_with_v1() {
        let info = BaseUrlInfo::new("https://api.githubcopilot.com/v1");
        assert!(!info.origin_only);
        assert!(info.already_has_v1);
        assert_eq!(info.copilot_base, "https://api.githubcopilot.com");
    }

    #[test]
    fn test_openrouter_api() {
        let info = BaseUrlInfo::new("https://openrouter.ai/api");
        assert!(!info.origin_only);
        assert!(!info.already_has_v1);
        assert_eq!(info.copilot_base, "https://openrouter.ai/api");
    }

    #[test]
    fn test_openrouter_api_v1() {
        let info = BaseUrlInfo::new("https://openrouter.ai/api/v1");
        assert!(!info.origin_only);
        assert!(info.already_has_v1);
        assert_eq!(info.copilot_base, "https://openrouter.ai/api");
    }

    #[test]
    fn test_no_scheme() {
        let info = BaseUrlInfo::new("localhost:8080");
        assert!(info.origin_only);
        assert!(!info.already_has_v1);
    }

    #[test]
    fn test_no_scheme_with_path() {
        let info = BaseUrlInfo::new("localhost:8080/api");
        assert!(!info.origin_only);
        assert!(!info.already_has_v1);
    }

    // ── is_openai_compat_endpoint ────────────────────────────────

    #[test]
    fn test_endpoint_chat_completions() {
        assert!(is_openai_compat_endpoint("chat/completions"));
    }

    #[test]
    fn test_endpoint_responses() {
        assert!(is_openai_compat_endpoint("responses"));
    }

    #[test]
    fn test_endpoint_v1_chat_completions() {
        assert!(is_openai_compat_endpoint("v1/chat/completions"));
    }

    #[test]
    fn test_endpoint_v1_responses() {
        assert!(is_openai_compat_endpoint("v1/responses"));
    }

    #[test]
    fn test_endpoint_messages_not_compat() {
        assert!(!is_openai_compat_endpoint("messages"));
    }

    #[test]
    fn test_endpoint_v1_messages_not_compat() {
        assert!(!is_openai_compat_endpoint("v1/messages"));
    }

    // ── dedup_v1 ─────────────────────────────────────────────────

    #[test]
    fn test_dedup_single() {
        let mut url = "https://api.example.com/v1/v1/chat/completions".to_string();
        dedup_v1(&mut url);
        assert_eq!(url, "https://api.example.com/v1/chat/completions");
    }

    #[test]
    fn test_dedup_double() {
        let mut url = "https://api.example.com/v1/v1/v1/chat/completions".to_string();
        dedup_v1(&mut url);
        assert_eq!(url, "https://api.example.com/v1/chat/completions");
    }

    #[test]
    fn test_dedup_noop() {
        let mut url = "https://api.example.com/v1/chat/completions".to_string();
        dedup_v1(&mut url);
        assert_eq!(url, "https://api.example.com/v1/chat/completions");
    }
}
