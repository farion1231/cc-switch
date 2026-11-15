use super::*;
use axum::http::HeaderValue;
use serde_json::json;

#[test]
fn hyphenated_headers_keep_client_identity_across_requests() {
    for client_format in ["codex", "openai"] {
        for name in ["session-id", "thread-id"] {
            let mut headers = HeaderMap::new();
            headers.insert(
                name,
                "  d937243f-2702-4f20-97b6-c9682235ab81  ".parse().unwrap(),
            );
            for body in [
                json!({"input": "first turn"}),
                json!({"input": "next turn"}),
            ] {
                let result = extract_session_id(&headers, &body, client_format);
                assert_eq!(
                    result.session_id, "codex_d937243f-2702-4f20-97b6-c9682235ab81",
                    "{client_format}: {name}"
                );
                assert_eq!(result.source, SessionIdSource::Header);
                assert!(result.client_provided);
            }
        }
    }
}

#[test]
fn session_headers_precede_thread_fallback_and_metadata() {
    let body = json!({"metadata": {"session_id": "metadata-session-123456789"}});
    let mut headers = HeaderMap::new();
    for (name, value) in [
        ("session_id", "legacy-session-123456789"),
        ("x-session-id", "legacy-x-session-123456789"),
        ("session-id", "native-session-123456789"),
        ("thread-id", "native-thread-123456789"),
    ] {
        headers.insert(name, value.parse().unwrap());
    }

    for (removed, expected) in [
        (None, "codex_legacy-session-123456789"),
        (Some("session_id"), "codex_legacy-x-session-123456789"),
        (Some("x-session-id"), "codex_native-session-123456789"),
        (Some("session-id"), "codex_native-thread-123456789"),
    ] {
        if let Some(name) = removed {
            headers.remove(name);
        }
        let result = extract_session_id(&headers, &body, "codex");
        assert_eq!(result.session_id, expected);
        assert_eq!(result.source, SessionIdSource::Header);
        assert!(result.client_provided);
    }
}

#[test]
fn unusable_hyphenated_headers_fall_back_to_thread_or_metadata() {
    let body = json!({"metadata": {"session_id": "metadata-session-123456789"}});
    for invalid in [
        HeaderValue::from_static(""),
        HeaderValue::from_static("   "),
        HeaderValue::from_static("short"),
        HeaderValue::from_static("12345678901234567890"),
        HeaderValue::from_bytes(&[0xff; 36]).unwrap(),
    ] {
        let mut headers = HeaderMap::new();
        headers.insert("session-id", invalid.clone());
        headers.insert("thread-id", "native-thread-123456789".parse().unwrap());
        let result = extract_session_id(&headers, &body, "codex");
        assert_eq!(result.session_id, "codex_native-thread-123456789");
        assert_eq!(result.source, SessionIdSource::Header);
        assert!(result.client_provided);

        headers.insert("thread-id", invalid);
        let result = extract_session_id(&headers, &body, "codex");
        assert_eq!(result.session_id, "codex_metadata-session-123456789");
        assert_eq!(result.source, SessionIdSource::MetadataSessionId);
        assert!(result.client_provided);
    }
}

#[test]
fn hyphenated_codex_headers_do_not_identify_other_clients() {
    let mut headers = HeaderMap::new();
    headers.insert("session-id", "native-session-123456789".parse().unwrap());
    headers.insert("thread-id", "native-thread-123456789".parse().unwrap());
    for client_format in ["claude", "claude-desktop", "grokbuild", "gemini"] {
        let result = extract_session_id(&headers, &json!({}), client_format);
        assert_eq!(result.source, SessionIdSource::Generated, "{client_format}");
        assert!(!result.client_provided);
    }
}
