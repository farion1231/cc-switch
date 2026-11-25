use super::*;
use crate::database::Database;
use crate::provider::LocalProxyRequestOverrides;
use axum::http::header::{HeaderValue, ACCEPT};
use axum::http::HeaderMap;
use bytes::Bytes;
use http::StatusCode;
use serde_json::json;
use std::collections::HashMap;
use std::time::Duration;

#[test]
fn opencode_upstream_host_matching() {
    // With or without a port, and subdomains, count as OpenCode; other
    // upstreams must not (they would otherwise gain a stray header).
    for host in ["opencode.ai", "opencode.ai:443", "zen.opencode.ai"] {
        assert!(
            is_opencode_upstream(Some(host)),
            "{host} should be recognised as OpenCode"
        );
    }
    for host in [
        "api.anthropic.com",
        "notopencode.ai",
        "example.com",
        "127.0.0.1:15721",
    ] {
        assert!(
            !is_opencode_upstream(Some(host)),
            "{host} must not be recognised as OpenCode"
        );
    }
    assert!(!is_opencode_upstream(None));
}

#[test]
fn conversation_fingerprint_is_stable_per_conversation() {
    let turn1 = json!({
        "system": "You are an assistant.",
        "messages": [{"role": "user", "content": "first question"}]
    });
    // Later turns of the same conversation append messages; system and the
    // first user message stay put.
    let turn2 = json!({
        "system": "You are an assistant.",
        "messages": [
            {"role": "user", "content": "first question"},
            {"role": "assistant", "content": "answer"},
            {"role": "user", "content": "follow-up"}
        ]
    });
    let other = json!({
        "system": "You are an assistant.",
        "messages": [{"role": "user", "content": "another conversation"}]
    });

    let a = conversation_fingerprint(&turn1).expect("fingerprint");
    let b = conversation_fingerprint(&turn2).expect("fingerprint");
    let c = conversation_fingerprint(&other).expect("fingerprint");
    assert_eq!(a, b, "one conversation must keep a single session id");
    assert_ne!(a, c, "different conversations must not share a session id");
    assert!(
        a.starts_with("ccsw-"),
        "prefix keeps the origin recognisable: {a}"
    );
}

#[test]
fn conversation_fingerprint_requires_conversation_content() {
    assert!(conversation_fingerprint(&json!({"model": "x"})).is_none());
}

#[test]
fn conversation_fingerprint_ignores_cache_control() {
    // Clients move `cache_control` between messages as a conversation grows
    // (Claude Code puts it on the first user message, then on the last one);
    // the session id must not change with it.
    let first = json!({
        "system": "You are an assistant.",
        "messages": [{
            "role": "user",
            "content": [
                {"type": "text", "text": "hello", "cache_control": {"type": "ephemeral"}}
            ]
        }]
    });
    let later = json!({
        "system": "You are an assistant.",
        "messages": [
            {"role": "user", "content": [{"type": "text", "text": "hello"}]},
            {"role": "assistant", "content": "hi"},
            {"role": "user", "content": [
                {"type": "text", "text": "again", "cache_control": {"type": "ephemeral"}}
            ]}
        ]
    });
    assert_eq!(
        conversation_fingerprint(&first),
        conversation_fingerprint(&later),
        "cache_control placement must not change the session id"
    );
}

#[test]
fn opencode_session_precedence() {
    // native client header > client-provided id > fingerprint > generated id
    assert_eq!(
        pick_opencode_session(Some("native"), Some("client"), Some("fp"), "gen").as_deref(),
        Some("native")
    );
    assert_eq!(
        pick_opencode_session(None, Some("client"), Some("fp"), "gen").as_deref(),
        Some("client")
    );
    assert_eq!(
        pick_opencode_session(None, None, Some("fp"), "gen").as_deref(),
        Some("fp")
    );
    assert_eq!(
        pick_opencode_session(None, None, None, "gen").as_deref(),
        Some("gen")
    );
    // Blank candidates are skipped, and nothing is sent when all are blank.
    assert_eq!(
        pick_opencode_session(Some("   "), None, None, "gen").as_deref(),
        Some("gen")
    );
    assert_eq!(pick_opencode_session(None, None, None, "  "), None);
}

#[test]
fn native_session_header_recognises_real_client_headers() {
    // Claude Code and Codex each send one of these; unknown headers do not count.
    for name in [
        "x-claude-code-session-id",
        "session-id",
        "session_id",
        "x-session-id",
    ] {
        let mut headers = http::HeaderMap::new();
        headers.insert(
            http::HeaderName::from_static(name),
            http::HeaderValue::from_static("abc123"),
        );
        assert_eq!(native_session_header(&headers), Some("abc123"), "{name}");
    }

    let mut blank = http::HeaderMap::new();
    blank.insert(
        http::HeaderName::from_static("session_id"),
        http::HeaderValue::from_static("  "),
    );
    assert_eq!(native_session_header(&blank), None);

    let mut other = http::HeaderMap::new();
    other.insert(
        http::HeaderName::from_static("x-request-id"),
        http::HeaderValue::from_static("ignored"),
    );
    assert_eq!(native_session_header(&other), None);
    assert_eq!(native_session_header(&http::HeaderMap::new()), None);
}

fn test_provider_with_type(provider_type: Option<&str>) -> Provider {
    Provider {
        id: "provider-1".to_string(),
        name: "Provider 1".to_string(),
        settings_config: json!({}),
        website_url: None,
        category: None,
        created_at: None,
        sort_index: None,
        notes: None,
        meta: provider_type.map(|value| crate::provider::ProviderMeta {
            provider_type: Some(value.to_string()),
            ..Default::default()
        }),
        icon: None,
        icon_color: None,
        in_failover_queue: false,
    }
}

fn test_forwarder(
    non_streaming_timeout: Duration,
    streaming_first_byte_timeout: Duration,
) -> RequestForwarder {
    let db = Arc::new(Database::memory().expect("memory db"));

    RequestForwarder {
        router: Arc::new(ProviderRouter::new(db.clone())),
        status: Arc::new(RwLock::new(ProxyStatus::default())),
        current_providers: Arc::new(RwLock::new(HashMap::new())),
        gemini_shadow: Arc::new(GeminiShadowStore::new()),
        codex_chat_history: Arc::new(CodexChatHistoryStore::default()),
        failover_manager: Arc::new(FailoverSwitchManager::new()),
        app_handle: None,
        current_provider_id_at_start: String::new(),
        route_revision: None,
        session_id: String::new(),
        session_client_provided: false,
        rectifier_config: RectifierConfig::default(),
        optimizer_config: OptimizerConfig::default(),
        copilot_optimizer_config: CopilotOptimizerConfig::default(),
        non_streaming_timeout,
        streaming_first_byte_timeout,
        max_attempts: 1,
        stack_request: false,
        failover_chain: false,
        last_outbound_model: std::sync::Mutex::new(None),
        privacy_filter: PrivacyFilterConfig::default(),
    }
}

/// 流式请求在 hyper 路径上必须用 `streaming_first_byte_timeout` 等响应头，
/// 不能沿用非流式超时：否则 Claude 供应商会卡到 10 分钟才触发故障转移。
#[tokio::test]
async fn streaming_header_wait_uses_first_byte_timeout_on_hyper_path() {
    // 应用启动时会安装 rustls provider；测试进程里手动装一次，hyper-util 客户端
    // 才能构建 TLS connector（http:// 目标本身不走 TLS）。
    let _ = rustls::crypto::ring::default_provider().install_default();
    // 接受连接、读完请求后一直不写响应头，直到客户端超时断开。
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    let server = std::thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let mut buf = [0u8; 2048];
            let _ = std::io::Read::read(&mut stream, &mut buf);
            // 不写响应头，并且保持连接超过非流式超时，确保「拿不到响应头」时只能由
            // 首包超时结束，而不是服务端主动断开。连接可能被客户端连接池保活，
            // 读不到 EOF，所以有界等待后退出。
            std::thread::sleep(Duration::from_secs(15));
        }
    });

    let forwarder = test_forwarder(Duration::from_secs(10), Duration::from_millis(300));
    let started = std::time::Instant::now();
    let error = tokio::time::timeout(
        Duration::from_secs(12),
        forwarder.send_upstream(
            &format!("http://{addr}/v1/messages"),
            "stalled upstream",
            &http::Method::POST,
            http::HeaderMap::new(),
            &http::Extensions::new(),
            b"{}".to_vec(),
            true, // streaming
            true, // preserve_exact_header_case → hyper path
        ),
    )
    .await
    .expect("send_upstream must not hang")
    .err()
    .expect("stalled upstream must time out");
    assert!(
        matches!(error, ProxyError::Timeout(_)),
        "expected timeout, got {error:?}"
    );
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "streaming header wait should use the 300ms first-byte timeout, took {:?}",
        started.elapsed()
    );
    // 不 join：服务端连接可能仍被客户端连接池保活，有界等待结束后线程自行退出。
    drop(server);
}

#[test]
fn codex_failover_model_mapping_skips_standalone_endpoints() {
    let mut forwarder = test_forwarder(Duration::ZERO, Duration::ZERO);
    assert!(!forwarder.maps_codex_failover_model(&AppType::Codex, "/responses"));

    forwarder.failover_chain = true;
    for endpoint in ["/responses", "/responses/compact", "/chat/completions"] {
        assert!(
            forwarder.maps_codex_failover_model(&AppType::Codex, endpoint),
            "{endpoint}"
        );
    }
    for endpoint in [
        "/images/generations",
        "/images/edits",
        "/images/generations?client_version=0.145.0",
        "/alpha/search",
    ] {
        assert!(
            !forwarder.maps_codex_failover_model(&AppType::Codex, endpoint),
            "{endpoint}"
        );
    }
    assert!(!forwarder.maps_codex_failover_model(&AppType::GrokBuild, "/responses"));
}

/// Copilot 清洗会把孤儿 tool_result 降级成用户文本。按 forward() 用的同一组函数、同一顺序：
/// 客户端那一遍（子开关关闭，工具结果保留）→ 清洗 / 合并并再过滤 → 确定性请求 ID
/// （对已过滤正文算 SHA-256）。只差在被遮盖口令上的两个请求，ID 相同，正文里也没有口令。
#[test]
fn copilot_orphan_tool_results_are_redacted_before_the_request_id() {
    let fwd = test_forwarder(Duration::ZERO, Duration::ZERO).privacy_filter(PrivacyFilterConfig {
        enabled: true,
        filter_tool_results: false,
    });
    let id = |password: &str| {
        let mut body = json!({
            "metadata": { "session_id": "known-session" },
            "messages": [
                { "role": "assistant", "content": [{ "type": "text", "text": "previous turn" }] },
                { "role": "user", "content": [
                    { "type": "tool_result", "tool_use_id": "missing",
                      "content": format!("password = {password}") }
                ]}
            ]
        });
        fwd.apply_privacy_filter(&AppType::Claude, &mut body, "client");
        // 子开关关闭：客户端那一遍不碰工具结果
        assert!(body.to_string().contains(password), "{body}");
        let body = fwd.copilot_rewrite_tool_results(&AppType::Claude, body);
        assert!(!body.to_string().contains(password), "{body}");
        super::super::copilot_optimizer::deterministic_request_id(&body, "known-session")
    };
    assert_eq!(id("4937"), id("4938"));
}

#[tokio::test]
async fn copilot_success_bookkeeping_preserves_actual_upstream_format() {
    for format in [
        Some(CodexUpstreamFormat::NativeResponses),
        Some(CodexUpstreamFormat::ChatCompletions),
        Some(CodexUpstreamFormat::Anthropic),
        None,
    ] {
        let mut forwarder = test_forwarder(Duration::ZERO, Duration::ZERO);
        let mut provider = test_provider_with_type(Some("github_copilot"));
        provider.meta.as_mut().unwrap().api_format = Some("openai_chat".to_string());
        forwarder.current_provider_id_at_start = provider.id.clone();
        forwarder.status.write().await.total_requests = 1;
        let response =
            ProxyResponse::buffered(StatusCode::OK, HeaderMap::new(), Bytes::from_static(b"{}"));

        let result = forwarder
            .finish_success(
                &provider,
                &AppType::Codex,
                false,
                (response, None, Some("resolved-model".to_string())),
                format,
            )
            .await;

        assert_eq!(result.codex_upstream_format, format);
        assert_eq!(result.outbound_model.as_deref(), Some("resolved-model"));
        assert_eq!(result.provider.id, provider.id);
        assert_eq!(result.response.status(), StatusCode::OK);
        let status = forwarder.status.read().await;
        assert_eq!(status.success_requests, 1);
        assert_eq!(status.failover_count, 0);
    }
}

#[test]
fn single_provider_retryable_log_uses_single_provider_code() {
    let error = ProxyError::UpstreamError {
        status: 429,
        body: Some(r#"{"error":{"message":"rate limit exceeded"}}"#.to_string()),
    };

    let (code, message) = build_retryable_failure_log("PackyCode-response", 1, 1, &error);

    assert_eq!(code, log_fwd::SINGLE_PROVIDER_FAILED);
    assert!(message.contains("Provider PackyCode-response 请求失败"));
    assert!(message.contains("上游 HTTP 429"));
    // 上游错误消息保留(截断)，用于诊断失败原因。
    assert!(message.contains("rate limit exceeded"));
    assert!(!message.contains("切换下一个"));
}

#[test]
fn multi_provider_retryable_log_keeps_failover_wording() {
    let error = ProxyError::Timeout("upstream timed out after 30s".to_string());

    let (code, message) = build_retryable_failure_log("primary", 1, 3, &error);

    assert_eq!(code, log_fwd::PROVIDER_FAILED_RETRY);
    assert!(message.contains("继续尝试下一个 (1/3)"));
    assert!(message.contains("请求超时"));
}

#[test]
fn single_provider_has_no_terminal_all_failed_log() {
    assert!(build_terminal_failure_log(1, 1, None).is_none());
}

#[test]
fn multi_provider_terminal_log_contains_last_error_summary() {
    let error = ProxyError::ForwardFailed("connection reset by peer".to_string());

    let (code, message) =
        build_terminal_failure_log(2, 2, Some(&error)).expect("expected terminal log");

    assert_eq!(code, log_fwd::ALL_PROVIDERS_FAILED);
    assert!(message.contains("已尝试 2/2 个 Provider，均失败"));
    assert!(message.contains("connection reset by peer"));
}

#[test]
fn summarize_text_for_log_collapses_whitespace_and_truncates() {
    let summary = summarize_text_for_log("line1\n\n line2   line3", 12);

    assert_eq!(summary, "line1 line2...");
}

#[test]
fn canonical_json_sorts_object_keys_for_cache_trace_hashes() {
    let left = json!({
        "tools": [
            {
                "parameters": {
                    "properties": {
                        "b": {"type": "string"},
                        "a": {"type": "number"}
                    },
                    "type": "object"
                },
                "name": "lookup"
            }
        ]
    });
    let right = json!({
        "tools": [
            {
                "name": "lookup",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "a": {"type": "number"},
                        "b": {"type": "string"}
                    }
                }
            }
        ]
    });

    assert_eq!(
        crate::proxy::json_canonical::canonical_json_string(&left),
        crate::proxy::json_canonical::canonical_json_string(&right)
    );
    assert_eq!(
        short_value_hash(Some(&left)),
        short_value_hash(Some(&right))
    );
}

#[test]
fn prepare_upstream_request_body_filters_private_fields_and_canonicalizes_order() {
    let body = json!({
        "z": 1,
        "_internal": "drop",
        "tools": [
            {
                "name": "lookup",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "_id": {
                            "_private_note": "drop",
                            "type": "string"
                        },
                        "b": {"type": "number"},
                        "a": {"type": "string"}
                    }
                }
            }
        ],
        "a": 2
    });

    let prepared = prepare_upstream_request_body(body);

    assert!(prepared.get("_internal").is_none());
    assert!(prepared["tools"][0]["parameters"]["properties"]
        .get("_id")
        .is_some());
    assert!(prepared["tools"][0]["parameters"]["properties"]["_id"]
        .get("_private_note")
        .is_none());
    assert_eq!(
        serde_json::to_string(&prepared).unwrap(),
        r#"{"a":2,"tools":[{"name":"lookup","parameters":{"properties":{"_id":{"type":"string"},"a":{"type":"string"},"b":{"type":"number"}},"type":"object"}}],"z":1}"#
    );
}

#[test]
fn local_proxy_body_overrides_deep_merge_final_body_without_stream() {
    let mut body = json!({
        "model": "before",
        "stream": false,
        "metadata": {
            "keep": true,
            "temperature": 1
        },
        "messages": [{ "role": "user", "content": "hello" }]
    });
    let overrides = LocalProxyRequestOverrides {
        headers: HashMap::new(),
        body: Some(json!({
            "model": "after",
            "stream": true,
            "metadata": {
                "temperature": 0.2,
                "top_p": 0.9
            },
            "messages": []
        })),
    };

    assert!(apply_local_proxy_body_overrides(&mut body, &overrides));

    assert_eq!(body["model"], "after");
    assert_eq!(body["stream"], false);
    assert_eq!(body["metadata"]["keep"], true);
    assert_eq!(body["metadata"]["temperature"], 0.2);
    assert_eq!(body["metadata"]["top_p"], 0.9);
    assert_eq!(body["messages"], json!([]));
}

#[test]
fn local_proxy_header_overrides_replace_allowed_headers_only() {
    let mut headers = http::HeaderMap::new();
    headers.insert(
        http::header::USER_AGENT,
        http::HeaderValue::from_static("original"),
    );
    headers.insert(
        http::header::AUTHORIZATION,
        http::HeaderValue::from_static("Bearer good"),
    );
    headers.insert(
        http::header::CONTENT_TYPE,
        http::HeaderValue::from_static("application/json"),
    );

    let overrides = LocalProxyRequestOverrides {
        headers: HashMap::from([
            ("User-Agent".to_string(), "custom".to_string()),
            ("X-Test".to_string(), "ok".to_string()),
            ("Authorization".to_string(), "Bearer bad".to_string()),
            ("Content-Type".to_string(), "text/plain".to_string()),
            ("X-Bad".to_string(), "bad\nvalue".to_string()),
        ]),
        body: None,
    };

    apply_local_proxy_header_overrides(&mut headers, Some(&overrides), false);

    assert_eq!(
        headers
            .get(http::header::USER_AGENT)
            .and_then(|value| value.to_str().ok()),
        Some("custom")
    );
    assert_eq!(
        headers
            .get(http::header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok()),
        Some("Bearer good")
    );
    assert_eq!(
        headers
            .get(http::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok()),
        Some("application/json")
    );
    assert_eq!(
        headers.get("x-test").and_then(|value| value.to_str().ok()),
        Some("ok")
    );
    assert!(headers.get("x-bad").is_none());
}

#[test]
fn local_proxy_header_overrides_are_skipped_for_copilot() {
    let mut headers = http::HeaderMap::new();
    headers.insert(
        http::header::USER_AGENT,
        http::HeaderValue::from_static("copilot"),
    );
    let overrides = LocalProxyRequestOverrides {
        headers: HashMap::from([("User-Agent".to_string(), "custom".to_string())]),
        body: None,
    };

    apply_local_proxy_header_overrides(&mut headers, Some(&overrides), true);

    assert_eq!(
        headers
            .get(http::header::USER_AGENT)
            .and_then(|value| value.to_str().ok()),
        Some("copilot")
    );
}

#[tokio::test]
async fn non_streaming_success_is_buffered_before_marking_provider_successful() {
    let forwarder = test_forwarder(Duration::from_secs(1), Duration::from_secs(1));
    let response = ProxyResponse::streamed(
        StatusCode::OK,
        HeaderMap::new(),
        futures::stream::once(async {
            tokio::time::sleep(Duration::from_millis(10)).await;
            Ok::<Bytes, std::io::Error>(Bytes::from_static(b"{\"ok\":true}"))
        }),
    );

    let prepared = forwarder
        .prepare_success_response_for_failover(response, false)
        .await
        .expect("response should be buffered");

    assert_eq!(
        prepared
            .bytes_with_limit(MAX_RESPONSE_BODY_BYTES)
            .await
            .unwrap(),
        Bytes::from_static(b"{\"ok\":true}")
    );
}

#[tokio::test]
async fn non_streaming_body_read_error_is_retryable_before_success_record() {
    let forwarder = test_forwarder(Duration::from_secs(1), Duration::from_secs(1));
    let response = ProxyResponse::streamed(
        StatusCode::OK,
        HeaderMap::new(),
        futures::stream::once(async {
            Err::<Bytes, std::io::Error>(std::io::Error::other("body boom"))
        }),
    );

    let err = match forwarder
        .prepare_success_response_for_failover(response, false)
        .await
    {
        Ok(_) => panic!("body read errors should fail the attempt"),
        Err(err) => err,
    };

    assert!(matches!(err, ProxyError::ForwardFailed(_)));
}

#[tokio::test]
async fn streaming_success_primes_first_chunk_and_replays_it() {
    let forwarder = test_forwarder(Duration::from_secs(1), Duration::from_secs(1));
    let response = ProxyResponse::streamed(
        StatusCode::OK,
        HeaderMap::new(),
        futures::stream::iter(vec![
            Ok::<Bytes, std::io::Error>(Bytes::from_static(b"first")),
            Ok::<Bytes, std::io::Error>(Bytes::from_static(b"second")),
        ]),
    );

    let prepared = forwarder
        .prepare_success_response_for_failover(response, true)
        .await
        .expect("stream should be primed");

    assert_eq!(
        prepared
            .bytes_with_limit(MAX_RESPONSE_BODY_BYTES)
            .await
            .unwrap(),
        Bytes::from_static(b"firstsecond")
    );
}

#[tokio::test]
async fn streaming_first_chunk_error_is_retryable_before_success_record() {
    let forwarder = test_forwarder(Duration::from_secs(1), Duration::from_secs(1));
    let response = ProxyResponse::streamed(
        StatusCode::OK,
        HeaderMap::new(),
        futures::stream::once(async {
            Err::<Bytes, std::io::Error>(std::io::Error::other("first chunk boom"))
        }),
    );

    let err = match forwarder
        .prepare_success_response_for_failover(response, true)
        .await
    {
        Ok(_) => panic!("first chunk errors should fail the attempt"),
        Err(err) => err,
    };

    assert!(matches!(err, ProxyError::ForwardFailed(_)));
}

#[test]
fn codex_oauth_session_headers_match_codex_cache_identity() {
    let headers = build_codex_oauth_session_headers("session-123");
    let mut map = HeaderMap::new();
    for (name, value) in headers {
        map.insert(name, value);
    }

    assert_eq!(
        map.get("session_id"),
        Some(&HeaderValue::from_static("session-123"))
    );
    assert_eq!(
        map.get("x-client-request-id"),
        Some(&HeaderValue::from_static("session-123"))
    );
    assert_eq!(
        map.get("x-codex-window-id"),
        Some(&HeaderValue::from_static("session-123:0"))
    );
}

#[test]
fn managed_account_upstream_rejects_proxy_managed_placeholder_header() {
    let mut headers = HeaderMap::new();
    headers.insert(
        "authorization",
        HeaderValue::from_static("Bearer PROXY_MANAGED"),
    );

    let err = reject_proxy_placeholder_for_managed_account_upstream(
        "https://api.githubcopilot.com/chat/completions",
        &headers,
    )
    .expect_err("placeholder should be rejected before upstream");

    assert!(matches!(
        err,
        ProxyError::AuthError(message) if message.contains("PROXY_MANAGED")
    ));

    let xai_err = reject_proxy_placeholder_for_managed_account_upstream(
        "https://api.x.ai/v1/responses",
        &headers,
    )
    .expect_err("xAI placeholder should be rejected before upstream");
    assert!(matches!(
        xai_err,
        ProxyError::AuthError(message) if message.contains("PROXY_MANAGED")
    ));
}

#[test]
fn codex_oauth_upstream_rejects_proxy_managed_placeholder_header() {
    let mut headers = HeaderMap::new();
    headers.insert(
        "authorization",
        HeaderValue::from_static("Bearer PROXY_MANAGED"),
    );

    let err = reject_proxy_placeholder_for_managed_account_upstream(
        "https://chatgpt.com/backend-api/codex/responses",
        &headers,
    )
    .expect_err("placeholder should be rejected before upstream");

    assert!(matches!(
        err,
        ProxyError::AuthError(message) if message.contains("PROXY_MANAGED")
    ));
}

#[test]
fn non_managed_upstream_allows_proxy_managed_placeholder_guard() {
    let mut headers = HeaderMap::new();
    headers.insert(
        "authorization",
        HeaderValue::from_static("Bearer PROXY_MANAGED"),
    );

    reject_proxy_placeholder_for_managed_account_upstream(
        "https://api.example.com/v1/messages",
        &headers,
    )
    .expect("guard is scoped to managed-account upstreams");
}

#[test]
fn exact_header_case_preserved_for_native_claude_only() {
    let provider = test_provider_with_type(None);

    assert!(should_preserve_exact_header_case(
        "Claude",
        &provider,
        Some("anthropic"),
        false
    ));
    assert!(!should_preserve_exact_header_case(
        "Claude",
        &provider,
        Some("openai_responses"),
        false
    ));
    assert!(!should_preserve_exact_header_case(
        "Codex", &provider, None, false
    ));
    assert!(!should_preserve_exact_header_case(
        "Gemini", &provider, None, false
    ));
}

#[test]
fn exact_header_case_skipped_for_codex_oauth_and_copilot() {
    let codex_oauth = test_provider_with_type(Some("codex_oauth"));
    let copilot = test_provider_with_type(Some("github_copilot"));

    assert!(!should_preserve_exact_header_case(
        "Claude",
        &codex_oauth,
        Some("openai_responses"),
        false
    ));
    assert!(!should_preserve_exact_header_case(
        "Claude",
        &copilot,
        Some("openai_chat"),
        true
    ));
}

#[test]
fn rewrite_claude_transform_endpoint_strips_beta_for_chat_completions() {
    let (endpoint, passthrough_query) = rewrite_claude_transform_endpoint(
        "/v1/messages?beta=true&foo=bar",
        "openai_chat",
        false,
        &json!({ "model": "gpt-5.4" }),
    );

    assert_eq!(endpoint, "/v1/chat/completions?foo=bar");
    assert_eq!(passthrough_query.as_deref(), Some("foo=bar"));
}

#[test]
fn rewrite_claude_transform_endpoint_strips_beta_for_responses() {
    let (endpoint, passthrough_query) = rewrite_claude_transform_endpoint(
        "/claude/v1/messages?beta=true&x-id=1",
        "openai_responses",
        false,
        &json!({ "model": "gpt-5.4" }),
    );

    assert_eq!(endpoint, "/v1/responses?x-id=1");
    assert_eq!(passthrough_query.as_deref(), Some("x-id=1"));
}

#[test]
fn rewrite_codex_responses_endpoint_to_chat_preserves_query() {
    let (endpoint, passthrough_query) =
        rewrite_codex_responses_endpoint_to_chat("/v1/responses?foo=bar");

    assert_eq!(endpoint, "/chat/completions?foo=bar");
    assert_eq!(passthrough_query.as_deref(), Some("foo=bar"));
}

#[test]
fn prepend_claude_code_system_prompt_from_string() {
    let mut body = json!({ "system": "You are a Codex agent." });
    prepend_claude_code_system_prompt(&mut body);
    let system = body["system"].as_array().unwrap();
    assert_eq!(system[0]["text"], CLAUDE_CODE_SYSTEM_IDENTITY);
    assert_eq!(system[1]["text"], "You are a Codex agent.");
}

#[test]
fn prepend_claude_code_system_prompt_when_absent() {
    let mut body = json!({});
    prepend_claude_code_system_prompt(&mut body);
    let system = body["system"].as_array().unwrap();
    assert_eq!(system.len(), 1);
    assert_eq!(system[0]["text"], CLAUDE_CODE_SYSTEM_IDENTITY);
}

#[test]
fn prepend_claude_code_system_prompt_is_idempotent() {
    let mut body = json!({ "system": "orig" });
    prepend_claude_code_system_prompt(&mut body);
    prepend_claude_code_system_prompt(&mut body);
    let system = body["system"].as_array().unwrap();
    assert_eq!(system.len(), 2);
    assert_eq!(system[0]["text"], CLAUDE_CODE_SYSTEM_IDENTITY);
    assert_eq!(system[1]["text"], "orig");
}

#[test]
fn rewrite_codex_responses_endpoint_to_anthropic_preserves_query() {
    let (endpoint, passthrough_query) =
        rewrite_codex_responses_endpoint_to_anthropic("/responses?x=1");
    assert_eq!(endpoint, "/v1/messages?x=1");
    assert_eq!(passthrough_query.as_deref(), Some("x=1"));

    let (endpoint, _) = rewrite_codex_responses_endpoint_to_anthropic("/v1/responses");
    assert_eq!(endpoint, "/v1/messages");
}

#[test]
fn codex_anthropic_full_endpoint_guard_avoids_double_messages() {
    // On the Codex→Anthropic path a base URL already ending in `/v1/messages` (switch
    // off) must be treated as a full endpoint by the real `base_url_is_full_endpoint`.

    // Without the guard, build_url would concatenate the pasted endpoint with the
    // rewritten `/v1/messages` target, producing a broken double suffix.
    use super::super::providers::ProviderAdapter;
    let doubled = super::super::providers::CodexAdapter::new()
        .build_url("https://host.example/v1/messages", "/v1/messages");
    assert_eq!(doubled, "https://host.example/v1/messages/v1/messages");

    // With the guard, the pasted URL is used verbatim (plus preserved query). Includes
    // query/fragment/whitespace suffixes, which must not hide the endpoint (fix: a base
    // like `.../v1/messages?beta=true` previously evaded the suffix check).
    for base in [
        "https://host.example/v1/messages",
        "https://host.example/v1/messages/",
        "https://host.example/api/v1/messages", // prefixed gateway
        "https://host.example/v1/messages?beta=true",
        "https://host.example/v1/messages/?beta=true",
        "https://host.example/v1/messages#frag",
        "  https://host.example/v1/messages  ",
    ] {
        assert!(
            base_url_is_full_endpoint(base, "/v1/messages"),
            "expected full-endpoint match: {base:?}"
        );
    }
    assert_eq!(
        append_query_to_full_url("https://host.example/v1/messages", Some("x=1")),
        "https://host.example/v1/messages?x=1"
    );
    // A base URL that already carries its own query is preserved verbatim (no double
    // `/v1/messages`, query kept).
    assert_eq!(
        append_query_to_full_url("https://host.example/v1/messages?beta=true", None),
        "https://host.example/v1/messages?beta=true"
    );

    // A non-endpoint base (origin/prefix) must NOT match, so build_url still appends.
    assert!(!base_url_is_full_endpoint(
        "https://host.example",
        "/v1/messages"
    ));
    assert!(!base_url_is_full_endpoint(
        "https://host.example/v1",
        "/v1/messages"
    ));
    // The shared helper also backs the Chat path's `/chat/completions` guard.
    assert!(base_url_is_full_endpoint(
        "https://host.example/v1/chat/completions?api-version=2024",
        "/chat/completions"
    ));
}

#[test]
fn codex_client_fingerprint_headers_are_dropped_for_anthropic_upstreams() {
    // Codex/OpenAI fingerprints a native Claude Code client never sends → must drop.
    for header in [
        "originator",
        "session_id",
        "session-id",
        "thread-id",
        "conversation_id",
        "chatgpt-account-id",
        "x-openai-subagent",
        "x-client-request-id",
        "x-codex-window-id",
        "openai-beta",
        "openai-organization",
        "openai-project",
        "version",
        "x-stainless-lang",
        "x-stainless-runtime",
        "x-codex-turn-id",
        // ChatGPT 登录态下的身份头，Stack 请求发往第三方时同样不能带出去。
        "x-oai-attestation",
        "x-openai-fedramp",
        "x-openai-account-routing-override",
        "x-openai-memgen-request",
        "x-openai-internal-codex-responses-lite",
        "x-responsesapi-include-timing-metrics",
    ] {
        assert!(
            is_codex_client_fingerprint_header(header),
            "expected {header} to be dropped while impersonating Claude Code"
        );
    }

    // Headers a real Claude Code client sends (or that the forwarder rebuilds) must
    // NOT be caught by the denylist.
    for header in [
        "anthropic-version",
        "anthropic-beta",
        "user-agent",
        "accept",
        "content-type",
        "x-app",
    ] {
        assert!(
            !is_codex_client_fingerprint_header(header),
            "{header} must be preserved while impersonating Claude Code"
        );
    }
}

#[test]
fn codex_anthropic_2xx_error_envelope_is_detected_for_failover() {
    let body = br#"{"type":"error","error":{"type":"overloaded_error","message":"busy"}}"#;
    assert_eq!(
        codex_anthropic_error_envelope_message(body).as_deref(),
        Some("overloaded_error: busy")
    );
    assert!(
        codex_anthropic_error_envelope_message(br#"{"type":"message","content":[]}"#).is_none()
    );
}

#[test]
fn responses_2xx_failure_is_detected_for_failover() {
    assert_eq!(
        responses_error_envelope_message(
            br#"{"status":"failed","error":{"type":"server_error","message":"busy"},"output":[]}"#
        )
        .as_deref(),
        Some("server_error: busy")
    );
    assert_eq!(
        responses_error_envelope_message(br#"{"status":"cancelled","output":[]}"#).as_deref(),
        Some("cancelled: response generation was cancelled")
    );
    assert!(responses_error_envelope_message(
        br#"{"status":"incomplete","incomplete_details":{"reason":"max_output_tokens"},"output":[]}"#
    )
    .is_none());
    assert!(responses_error_envelope_message(
        br#"{"status":"completed","error":null,"output":[]}"#
    )
    .is_none());
}

#[test]
fn responses_stream_start_semantic_failure_is_retryable() {
    let created = concat!(
        "event: response.created\n",
        "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_1\"}}"
    );
    assert!(inspect_responses_start_event(created).is_none());

    let failed = concat!(
        "event: response.failed\n",
        "data: {\"type\":\"response.failed\",\"response\":{\"error\":{\"type\":\"server_error\",\"message\":\"boom\"}}}"
    );
    assert!(matches!(
        inspect_responses_start_event(failed),
        Some(Err(ProxyError::TransformError(message))) if message.contains("boom")
    ));

    let delta = concat!(
        "event: response.output_text.delta\n",
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"hi\"}"
    );
    assert!(matches!(inspect_responses_start_event(delta), Some(Ok(()))));
}

/// 输出前说验不了密文（#9424）：改成上游 400，回到重试循环做密文整流，不当转换错误换家。
#[test]
fn responses_stream_start_opaque_rejection_becomes_upstream_error() {
    let foreign_resource = concat!(
        "event: response.failed\n",
        "data: {\"type\":\"response.failed\",\"response\":{\"status\":\"failed\",\"error\":{\"type\":\"invalid_request_error\",\"message\":\"The requested item was created under a different Azure OpenAI resource.\"}}}"
    );
    let Some(Err(ProxyError::UpstreamError {
        status: 400,
        body: Some(body),
    })) = inspect_responses_start_event(foreign_resource)
    else {
        panic!("expected an upstream 400");
    };
    assert!(body.contains("different Azure OpenAI resource"));

    let error_event = concat!(
        "event: error\n",
        "data: {\"type\":\"error\",\"error\":{\"code\":\"invalid_encrypted_content\",\"message\":\"bad\"}}"
    );
    assert!(matches!(
        inspect_responses_start_event(error_event),
        Some(Err(ProxyError::UpstreamError { status: 400, .. }))
    ));

    assert!(matches!(
        inspect_responses_json_document(
            r#"{"status":"failed","error":{"code":"invalid_encrypted_content","message":"bad"}}"#
        ),
        Some(Err(ProxyError::UpstreamError { status: 400, .. }))
    ));
}

#[test]
fn responses_stream_start_accepts_unlabelled_whole_json() {
    assert!(matches!(
        inspect_responses_json_document(
            r#"{
                "status": "completed",

                "output": []
            }"#
        ),
        Some(Ok(()))
    ));
    assert!(inspect_responses_json_document(r#"{"status":"completed""#).is_none());

    let failed = inspect_responses_json_document(
        r#"{"status":"failed","error":{"message":"backend unavailable"}}"#,
    );
    assert!(
        matches!(failed, Some(Err(ProxyError::TransformError(message))) if message.contains("backend unavailable"))
    );
}

#[test]
fn codex_anthropic_cache_is_default_on_but_honors_sub_switch() {
    let default = codex_anthropic_cache_config(&OptimizerConfig::default());
    assert!(default.enabled);
    assert!(default.cache_injection);

    let disabled = codex_anthropic_cache_config(&OptimizerConfig {
        cache_injection: false,
        ..OptimizerConfig::default()
    });
    assert!(disabled.enabled);
    assert!(!disabled.cache_injection);
}

#[test]
fn invalid_client_history_is_not_retryable() {
    let forwarder = test_forwarder(Duration::ZERO, Duration::ZERO);
    let provider = test_provider_with_type(None);
    assert_eq!(
        forwarder.categorize_proxy_error(
            &ProxyError::InvalidRequest("invalid historical tool arguments".to_string()),
            &provider,
        ),
        ErrorCategory::NonRetryable
    );
}

#[test]
fn official_codex_failures_are_not_retryable() {
    let forwarder = test_forwarder(Duration::ZERO, Duration::ZERO);
    let mut provider = test_provider_with_type(None);
    provider.id = "codex-official".to_string();
    provider.category = Some("official".to_string());

    for error in [
        ProxyError::AuthError("restart Codex".to_string()),
        ProxyError::UpstreamError {
            status: 401,
            body: None,
        },
        ProxyError::UpstreamError {
            status: 403,
            body: None,
        },
        ProxyError::UpstreamError {
            status: 429,
            body: None,
        },
        ProxyError::Timeout("timeout".to_string()),
    ] {
        assert_eq!(
            forwarder.categorize_proxy_error(&error, &provider),
            ErrorCategory::NonRetryable
        );
    }
}

#[test]
fn xai_oauth_token_auth_failures_are_not_retryable() {
    let forwarder = test_forwarder(Duration::ZERO, Duration::ZERO);
    let provider = test_provider_with_type(Some("xai_oauth"));

    // 本地取 token 失败 = 账号级问题（需重新登录），failover 无济于事
    assert_eq!(
        forwarder.categorize_proxy_error(
            &ProxyError::AuthError("xAI OAuth 认证失败".to_string()),
            &provider,
        ),
        ErrorCategory::NonRetryable
    );
    // 上游 401/403 保持 Retryable：换 provider 可能持有可用的 key
    assert_eq!(
        forwarder.categorize_proxy_error(
            &ProxyError::UpstreamError {
                status: 401,
                body: None,
            },
            &provider,
        ),
        ErrorCategory::Retryable
    );
}

#[test]
fn official_codex_rejects_stale_proxy_placeholder_with_restart_hint() {
    let mut headers = HeaderMap::new();
    headers.insert(
        http::header::AUTHORIZATION,
        HeaderValue::from_static("Bearer PROXY_MANAGED"),
    );
    let mut provider = test_provider_with_type(None);
    provider.id = "codex-official".to_string();
    provider.category = Some("official".to_string());
    let error = validate_codex_official_authorization(&headers, &provider, None, None)
        .expect_err("stale placeholder must be rejected");
    assert!(matches!(
        error,
        ProxyError::AuthError(message)
            if message.contains("重启 Codex") && message.contains("新建会话")
    ));
}

#[test]
fn managed_codex_official_rejects_a_different_session_account() {
    let mut provider = test_provider_with_type(Some("codex_oauth"));
    provider.category = Some("official".to_string());
    provider.meta.as_mut().expect("provider meta").auth_binding =
        Some(crate::provider::AuthBinding {
            source: crate::provider::AuthBindingSource::ManagedAccount,
            auth_provider: Some("codex_oauth".to_string()),
            account_id: Some("local-account-b".to_string()),
        });

    let mut headers = HeaderMap::new();
    headers.insert(
        http::header::AUTHORIZATION,
        HeaderValue::from_static("Bearer account-a-token"),
    );
    headers.insert(
        "chatgpt-account-id",
        HeaderValue::from_static("workspace-shared"),
    );
    let error = validate_codex_official_authorization(
        &headers,
        &provider,
        Some("workspace-shared"),
        Some(false),
    )
    .expect_err("another user's bearer in the same workspace must be rejected");
    assert!(matches!(error, ProxyError::AuthError(message) if message.contains("重启 Codex")));

    validate_codex_official_authorization(
        &headers,
        &provider,
        Some("workspace-shared"),
        Some(true),
    )
    .expect("the selected account may pass through");
}

#[test]
fn rewrite_codex_responses_compact_endpoint_to_chat_preserves_query() {
    let (endpoint, passthrough_query) =
        rewrite_codex_responses_endpoint_to_chat("/v1/responses/compact?foo=bar");

    assert_eq!(endpoint, "/chat/completions?foo=bar");
    assert_eq!(passthrough_query.as_deref(), Some("foo=bar"));
}

#[test]
fn rewrite_claude_transform_endpoint_uses_copilot_path() {
    let (endpoint, passthrough_query) = rewrite_claude_transform_endpoint(
        "/v1/messages?beta=true&x-id=1",
        "anthropic",
        true,
        &json!({ "model": "claude-sonnet-4-6" }),
    );

    assert_eq!(endpoint, "/chat/completions?x-id=1");
    assert_eq!(passthrough_query.as_deref(), Some("x-id=1"));
}

#[test]
fn rewrite_claude_transform_endpoint_uses_copilot_responses_path() {
    let (endpoint, passthrough_query) = rewrite_claude_transform_endpoint(
        "/v1/messages?beta=true&x-id=1",
        "openai_responses",
        true,
        &json!({ "model": "gpt-5.4" }),
    );

    assert_eq!(endpoint, "/v1/responses?x-id=1");
    assert_eq!(passthrough_query.as_deref(), Some("x-id=1"));
}

#[test]
fn rewrite_claude_transform_endpoint_maps_gemini_generate_content() {
    let (endpoint, passthrough_query) = rewrite_claude_transform_endpoint(
        "/v1/messages?beta=true&x-id=1",
        "gemini_native",
        false,
        &json!({ "model": "gemini-2.5-pro" }),
    );

    assert_eq!(
        endpoint,
        "/v1beta/models/gemini-2.5-pro:generateContent?x-id=1"
    );
    assert_eq!(passthrough_query.as_deref(), Some("x-id=1"));
}

/// Regression: body.model arriving as the resource-name form
/// `models/gemini-2.5-pro` must not produce a doubled
/// `/v1beta/models/models/...` path.
#[test]
fn rewrite_claude_transform_endpoint_strips_gemini_model_resource_prefix() {
    let (endpoint, _) = rewrite_claude_transform_endpoint(
        "/v1/messages",
        "gemini_native",
        false,
        &json!({ "model": "models/gemini-2.5-pro" }),
    );

    assert_eq!(endpoint, "/v1beta/models/gemini-2.5-pro:generateContent");
}

#[test]
fn rewrite_claude_transform_endpoint_maps_gemini_streaming() {
    let (endpoint, passthrough_query) = rewrite_claude_transform_endpoint(
        "/v1/messages?beta=true",
        "gemini_native",
        false,
        &json!({ "model": "gemini-2.5-flash", "stream": true }),
    );

    assert_eq!(
        endpoint,
        "/v1beta/models/gemini-2.5-flash:streamGenerateContent?alt=sse"
    );
    assert_eq!(passthrough_query.as_deref(), Some("alt=sse"));
}

#[test]
fn append_query_to_full_url_preserves_existing_query_string() {
    let url = append_query_to_full_url("https://relay.example/api?foo=bar", Some("x-id=1"));

    assert_eq!(url, "https://relay.example/api?foo=bar&x-id=1");
}

#[test]
fn alpha_search_rewrites_known_full_responses_urls() {
    let cases = [
        (
            "https://relay.example/Gateway/%2F/v1/Responses/Compact/?api-version=CaseValue#fragment",
            "https://relay.example/Gateway/%2F/v1/alpha/search?api-version=CaseValue&client_version=0.144.6",
        ),
        (
            "https://relay.example/v1/responses",
            "https://relay.example/v1/alpha/search?client_version=0.144.6",
        ),
        (
            "https://relay.example/backend-api/codex/responses/compact/",
            "https://relay.example/backend-api/codex/alpha/search?client_version=0.144.6",
        ),
        (
            "https://relay.example/custom/%2F/v1/responses?api-version=2026-07",
            "https://relay.example/custom/%2F/v1/alpha/search?api-version=2026-07&client_version=0.144.6",
        ),
    ];

    for (base_url, expected) in cases {
        assert_eq!(
            rewrite_codex_standalone_full_url(
                base_url,
                Some("client_version=0.144.6"),
                CodexStandaloneEndpoint::AlphaSearch,
            )
            .expect("known Responses full URL should be rewritable"),
            expected
        );
    }
}

#[test]
fn images_generations_rewrites_known_full_codex_urls() {
    let cases = [
        (
            "https://relay.example/Gateway/v1/Images/Edits/?api-version=CaseValue#fragment",
            "https://relay.example/Gateway/v1/images/generations?api-version=CaseValue&client_version=0.145.0",
        ),
        (
            "https://relay.example/v1/responses",
            "https://relay.example/v1/images/generations?client_version=0.145.0",
        ),
        (
            "https://relay.example/backend-api/codex/responses/compact/",
            "https://relay.example/backend-api/codex/images/generations?client_version=0.145.0",
        ),
        (
            "https://relay.example/custom/%2F/v1/responses?api-version=2026-07",
            "https://relay.example/custom/%2F/v1/images/generations?api-version=2026-07&client_version=0.145.0",
        ),
        (
            "https://relay.example/v1/chat/completions?api-version=2026-07",
            "https://relay.example/v1/images/generations?api-version=2026-07&client_version=0.145.0",
        ),
        (
            "https://relay.example/v1/images/edits",
            "https://relay.example/v1/images/generations?client_version=0.145.0",
        ),
    ];

    for (base_url, expected) in cases {
        assert_eq!(
            rewrite_codex_standalone_full_url(
                base_url,
                Some("client_version=0.145.0"),
                CodexStandaloneEndpoint::ImagesGenerations,
            )
            .expect("known Codex full URL should be rewritable"),
            expected
        );
    }
}

#[test]
fn images_generations_preserves_existing_full_images_url() {
    let url = rewrite_codex_standalone_full_url(
        "https://relay.example/v1/images/generations?api-version=2026-07",
        Some("client_version=0.145.0"),
        CodexStandaloneEndpoint::ImagesGenerations,
    )
    .expect("full Images URL should be preserved");

    assert_eq!(
        url,
        "https://relay.example/v1/images/generations?api-version=2026-07&client_version=0.145.0"
    );
}

#[test]
fn images_generations_rejects_opaque_full_url_instead_of_misrouting_payload() {
    let error = rewrite_codex_standalone_full_url(
        "https://relay.example/custom/rpc-endpoint",
        Some("client_version=0.145.0"),
        CodexStandaloneEndpoint::ImagesGenerations,
    )
    .expect_err("opaque endpoint must fail closed");

    assert!(matches!(
        error,
        ProxyError::ConfigError(message)
            if message.contains("cannot derive /images/generations")
    ));
}

#[test]
fn codex_standalone_endpoint_recognizes_images_edits() {
    assert!(matches!(
        CodexStandaloneEndpoint::from_effective_endpoint("/images/edits?client_version=0.145.0"),
        Some(CodexStandaloneEndpoint::ImagesEdits)
    ));
    // Codex ImageGen never calls the variations route; keep it unrouted.
    assert!(CodexStandaloneEndpoint::from_effective_endpoint("/images/variations").is_none());
}

#[test]
fn images_edits_rewrites_known_full_codex_urls() {
    let cases = [
        (
            "https://relay.example/Gateway/v1/Chat/Completions/?api-version=CaseValue#fragment",
            "https://relay.example/Gateway/v1/images/edits?api-version=CaseValue&client_version=0.145.0",
        ),
        (
            "https://relay.example/v1/responses",
            "https://relay.example/v1/images/edits?client_version=0.145.0",
        ),
        (
            "https://relay.example/backend-api/codex/responses/compact/",
            "https://relay.example/backend-api/codex/images/edits?client_version=0.145.0",
        ),
        (
            "https://relay.example/v1/chat/completions?api-version=2026-07",
            "https://relay.example/v1/images/edits?api-version=2026-07&client_version=0.145.0",
        ),
        (
            "https://relay.example/v1/images/generations?api-version=2026-07",
            "https://relay.example/v1/images/edits?api-version=2026-07&client_version=0.145.0",
        ),
    ];

    for (base_url, expected) in cases {
        assert_eq!(
            rewrite_codex_standalone_full_url(
                base_url,
                Some("client_version=0.145.0"),
                CodexStandaloneEndpoint::ImagesEdits,
            )
            .expect("known Codex full URL should be rewritable"),
            expected
        );
    }
}

#[test]
fn images_edits_preserves_existing_full_edits_url() {
    let url = rewrite_codex_standalone_full_url(
        "https://relay.example/v1/images/edits?api-version=2026-07",
        Some("client_version=0.145.0"),
        CodexStandaloneEndpoint::ImagesEdits,
    )
    .expect("full Images edits URL should be preserved");

    assert_eq!(
        url,
        "https://relay.example/v1/images/edits?api-version=2026-07&client_version=0.145.0"
    );
}

#[test]
fn images_edits_rejects_opaque_full_url_instead_of_misrouting_payload() {
    let error = rewrite_codex_standalone_full_url(
        "https://relay.example/custom/rpc-endpoint",
        Some("client_version=0.145.0"),
        CodexStandaloneEndpoint::ImagesEdits,
    )
    .expect_err("opaque endpoint must fail closed");

    assert!(matches!(
        error,
        ProxyError::ConfigError(message)
            if message.contains("cannot derive /images/edits")
    ));
}

#[test]
fn alpha_search_rejects_opaque_full_url_instead_of_misrouting_payload() {
    let error = rewrite_codex_standalone_full_url(
        "https://relay.example/custom/rpc-endpoint",
        Some("client_version=0.144.6"),
        CodexStandaloneEndpoint::AlphaSearch,
    )
    .expect_err("opaque endpoint must fail closed");

    assert!(matches!(
        error,
        ProxyError::ConfigError(message)
            if message.contains("cannot derive /alpha/search")
    ));
}

#[test]
fn build_gemini_native_url_uses_origin_when_base_ends_with_v1beta() {
    let url = crate::proxy::gemini_url::build_gemini_native_url(
        "https://generativelanguage.googleapis.com/v1beta",
        "/v1beta/models/gemini-2.5-pro:generateContent",
    );

    assert_eq!(
        url,
        "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-pro:generateContent"
    );
}

#[test]
fn build_gemini_native_url_uses_origin_when_base_already_contains_models_prefix() {
    let url = crate::proxy::gemini_url::build_gemini_native_url(
        "https://generativelanguage.googleapis.com/v1beta/models",
        "/v1beta/models/gemini-2.5-flash:streamGenerateContent?alt=sse",
    );

    assert_eq!(
        url,
        "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-flash:streamGenerateContent?alt=sse"
    );
}

#[test]
fn resolve_gemini_native_url_keeps_opaque_full_url_as_is() {
    let url = crate::proxy::gemini_url::resolve_gemini_native_url(
        "https://relay.example/custom/generate-content",
        "/v1beta/models/gemini-2.5-flash:streamGenerateContent?alt=sse",
        true,
    );

    assert_eq!(url, "https://relay.example/custom/generate-content?alt=sse");
}

#[test]
fn force_identity_for_stream_flag_requests() {
    let headers = HeaderMap::new();

    assert!(should_force_identity_encoding(
        "/v1/responses",
        &json!({ "stream": true }),
        &headers
    ));
}

#[test]
fn force_identity_for_gemini_stream_endpoints() {
    let headers = HeaderMap::new();

    assert!(should_force_identity_encoding(
        "/v1beta/models/gemini-2.5-pro:streamGenerateContent?alt=sse",
        &json!({ "model": "gemini-2.5-pro" }),
        &headers
    ));
}

#[test]
fn streaming_request_detects_gemini_sse_without_body_stream_flag() {
    let headers = HeaderMap::new();

    assert!(is_streaming_request(
        "/v1beta/models/gemini-2.5-pro:streamGenerateContent?alt=sse",
        &json!({ "model": "gemini-2.5-pro" }),
        &headers
    ));
}

#[test]
fn force_identity_for_sse_accept_header() {
    let mut headers = HeaderMap::new();
    headers.insert(ACCEPT, HeaderValue::from_static("text/event-stream"));

    assert!(should_force_identity_encoding(
        "/v1/responses",
        &json!({ "model": "gpt-5" }),
        &headers
    ));
}

#[test]
fn non_streaming_requests_allow_automatic_compression() {
    let headers = HeaderMap::new();

    assert!(!should_force_identity_encoding(
        "/v1/responses",
        &json!({ "model": "gpt-5" }),
        &headers
    ));
}

// ==================== Copilot 动态 endpoint 路由相关测试 ====================

fn codex_copilot_headers(forwarder: &RequestForwarder, body: &Value) -> HeaderMap {
    let mut headers =
        super::super::providers::copilot_auth::build_copilot_request_headers("test-token").unwrap();
    let request_id = headers
        .iter()
        .find(|(name, _)| name.as_str() == "x-request-id")
        .unwrap()
        .1
        .clone();
    if let Some(context) = forwarder.codex_copilot_request_context(true, body) {
        context
            .apply_headers(
                &mut headers,
                forwarder.copilot_optimizer_config.request_classification,
            )
            .unwrap();
    }
    let headers: HeaderMap = headers.into_iter().collect();
    assert_eq!(headers["x-request-id"], request_id);
    assert_eq!(headers["x-agent-task-id"], request_id);
    headers
}

#[test]
fn codex_copilot_tool_continuations_receive_agent_and_session_headers() {
    let mut forwarder = test_forwarder(Duration::ZERO, Duration::ZERO);
    forwarder.session_id = "codex_12345678-1234-1234-1234-123456789abc".to_string();
    forwarder.session_client_provided = true;
    for item_type in [
        "function_call_output",
        "custom_tool_call_output",
        "tool_search_output",
    ] {
        for stream in [false, true] {
            let body = json!({
                "input": [{"type": item_type, "call_id": "call-1", "output": "done"}],
                "stream": stream
            });
            let headers = codex_copilot_headers(&forwarder, &body);
            assert_eq!(headers["x-initiator"], "agent", "{body}");
            assert_eq!(
                headers["x-interaction-id"].to_str().unwrap(),
                super::super::copilot_optimizer::deterministic_interaction_id(
                    &forwarder.session_id
                )
                .unwrap()
            );
            assert_eq!(headers["x-interaction-type"], "conversation-agent");
        }
    }
}

#[test]
fn codex_copilot_headers_reuse_client_session_across_turns() {
    let first = json!({"input": "Read the file"});
    let continuation = json!({
        "input": [{"type": "function_call_output", "call_id": "call-1", "output": "done"}]
    });
    for source in [
        "session_id",
        "x-session-id",
        "session-id",
        "thread-id",
        "metadata",
    ] {
        let mut headers = HeaderMap::new();
        let mut body = first.clone();
        let session_id = "12345678-1234-1234-1234-123456789abc";
        if source == "metadata" {
            body["metadata"] = json!({"session_id": session_id});
        } else {
            headers.insert(source, HeaderValue::from_static(session_id));
        }
        let session = super::super::session::extract_session_id(&headers, &body, "codex");
        let mut forwarder = test_forwarder(Duration::ZERO, Duration::ZERO);
        forwarder.session_id = session.session_id;
        forwarder.session_client_provided = session.client_provided;
        let first_headers = codex_copilot_headers(&forwarder, &first);
        let next_headers = codex_copilot_headers(&forwarder, &continuation);
        assert_eq!(first_headers["x-initiator"], "user");
        assert_eq!(next_headers["x-initiator"], "agent");
        assert_eq!(
            first_headers["x-interaction-id"], next_headers["x-interaction-id"],
            "{source}"
        );
    }
}

#[test]
fn codex_copilot_headers_honor_switches_and_do_not_invent_sessions() {
    let body = json!({
        "previous_response_id": "resp-not-a-session",
        "input": [{"type": "function_call_output", "call_id": "call-1", "output": "done"}]
    });
    let session = super::super::session::extract_session_id(&HeaderMap::new(), &body, "codex");
    let mut forwarder = test_forwarder(Duration::ZERO, Duration::ZERO);
    forwarder.session_id = session.session_id;
    forwarder.session_client_provided = session.client_provided;
    let headers = codex_copilot_headers(&forwarder, &body);
    assert_eq!(headers["x-initiator"], "agent");
    assert!(!headers.contains_key("x-interaction-id"));

    forwarder.session_client_provided = true;
    forwarder.copilot_optimizer_config.request_classification = false;
    let headers = codex_copilot_headers(&forwarder, &body);
    assert_eq!(headers["x-initiator"], "user");
    assert!(headers.contains_key("x-interaction-id"));

    forwarder.copilot_optimizer_config.enabled = false;
    let headers = codex_copilot_headers(&forwarder, &body);
    assert_eq!(headers["x-initiator"], "user");
    assert!(!headers.contains_key("x-interaction-id"));

    forwarder.copilot_optimizer_config.enabled = true;
    assert!(forwarder
        .codex_copilot_request_context(false, &body)
        .is_none());
}

#[test]
fn codex_copilot_new_user_input_is_not_a_replayed_tool_continuation() {
    let forwarder = test_forwarder(Duration::ZERO, Duration::ZERO);
    let body = json!({
        "input": [
            {"type": "function_call_output", "call_id": "call-1", "output": "done"},
            {"role": "assistant", "content": "The file is updated"},
            {"role": "user", "content": "Explain a different file"}
        ]
    });
    assert_eq!(
        codex_copilot_headers(&forwarder, &body)["x-initiator"],
        "user"
    );
}

#[test]
fn copilot_request_context_preserves_claude_header_overrides() {
    let mut headers =
        super::super::providers::copilot_auth::build_copilot_request_headers("test-token").unwrap();
    let context = CopilotRequestContext {
        classification: super::super::copilot_optimizer::CopilotClassification {
            initiator: "agent",
            is_warmup: false,
            is_compact: false,
            is_subagent: true,
        },
        request_id: Some("12345678-1234-1234-1234-123456789abc".to_string()),
        interaction_id: Some("abcdef12-1234-1234-1234-123456789abc".to_string()),
    };
    context.apply_headers(&mut headers, true).unwrap();
    let headers: HeaderMap = headers.into_iter().collect();
    assert_eq!(headers["x-initiator"], "agent");
    assert_eq!(headers["x-interaction-type"], "conversation-subagent");
    assert_eq!(
        headers["x-request-id"],
        context.request_id.as_deref().unwrap()
    );
    assert_eq!(headers["x-agent-task-id"], headers["x-request-id"]);
    assert_eq!(
        headers["x-interaction-id"],
        context.interaction_id.as_deref().unwrap()
    );
}

#[test]
fn anthropic_rectifiers_cover_copilot_native_messages() {
    for provider_type in [
        ProviderType::Claude,
        ProviderType::ClaudeAuth,
        ProviderType::GitHubCopilot,
    ] {
        assert!(uses_anthropic_rectifiers(Some(provider_type)));
    }
    for provider_type in [ProviderType::OpenRouter, ProviderType::CodexOAuth] {
        assert!(!uses_anthropic_rectifiers(Some(provider_type)));
    }
    assert!(!uses_anthropic_rectifiers(None));
}

#[test]
fn copilot_transport_selects_protocol_by_vendor_and_endpoints() {
    let messages = vec!["/v1/messages".to_string(), "/chat/completions".to_string()];
    let chat_only = vec!["/chat/completions".to_string()];
    assert_eq!(
        copilot_api_format_for_model(Some("OpenAI"), &chat_only),
        "openai_responses"
    );
    assert_eq!(
        copilot_api_format_for_model(Some("Anthropic"), &messages),
        "anthropic"
    );
    assert_eq!(
        copilot_api_format_for_model(Some("Anthropic"), &chat_only),
        "openai_chat"
    );
    assert_eq!(
        copilot_api_format_for_model(Some("Anthropic"), &[]),
        "openai_chat"
    );
    assert_eq!(copilot_api_format_for_model(None, &messages), "openai_chat");
    for responses_only in [["/responses"], ["/v1/responses"]] {
        let endpoints = responses_only.map(String::from);
        assert_eq!(
            copilot_api_format_for_model(Some("xAI"), &endpoints),
            "openai_responses"
        );
    }
    let both = ["/v1/chat/completions", "/responses"].map(String::from);
    assert_eq!(
        copilot_api_format_for_model(Some("xAI"), &both),
        "openai_chat"
    );
}

#[test]
fn copilot_unversioned_url_does_not_add_v1_prefix() {
    assert_eq!(
        build_copilot_unversioned_url(
            "https://api.githubcopilot.com/",
            "/chat/completions?client_version=0.149"
        ),
        "https://api.githubcopilot.com/chat/completions?client_version=0.149"
    );
    assert_eq!(
        build_copilot_unversioned_url(
            "https://api.githubcopilot.com",
            "/responses?client_version=0.149"
        ),
        "https://api.githubcopilot.com/responses?client_version=0.149"
    );
}

#[test]
fn codex_copilot_lookup_errors_preserve_cause_and_allow_failover() {
    let forwarder = test_forwarder(Duration::ZERO, Duration::ZERO);
    let copilot = test_provider_with_type(Some("github_copilot"));
    for (cause, expected_kind) in [
        (CopilotAuthError::DeviceFlowNotStarted, "auth"),
        (CopilotAuthError::AuthorizationPending, "auth"),
        (CopilotAuthError::AccessDenied, "auth"),
        (CopilotAuthError::ExpiredToken, "auth"),
        (CopilotAuthError::GitHubTokenInvalid, "auth"),
        (CopilotAuthError::NoCopilotSubscription, "auth"),
        (CopilotAuthError::AccountNotFound("missing".into()), "auth"),
        (
            CopilotAuthError::NetworkError("connection reset".into()),
            "upstream",
        ),
        (
            CopilotAuthError::CopilotTokenFetchFailed("models HTTP 503".into()),
            "upstream",
        ),
        (
            CopilotAuthError::ParseError("invalid models JSON".into()),
            "config",
        ),
        (
            CopilotAuthError::IoError("permission denied".into()),
            "config",
        ),
        (
            CopilotAuthError::InvalidDomain("invalid.example/path".into()),
            "config",
        ),
    ] {
        let cause_text = cause.to_string();
        let error = codex_copilot_lookup_error("gpt-5.5", cause);
        let (kind, message) = match &error {
            ProxyError::AuthError(message) => ("auth", message),
            ProxyError::ForwardFailed(message) => ("upstream", message),
            ProxyError::ConfigError(message) => ("config", message),
            _ => panic!("unexpected capability error: {error}"),
        };
        assert_eq!(kind, expected_kind, "{error}");
        assert!(message.contains(&cause_text), "{message}");
        assert!(message.contains("gpt-5.5"), "{message}");
        assert!(!message.contains("unavailable"), "{message}");
        assert_eq!(
            forwarder.categorize_proxy_error(&error, &copilot),
            ErrorCategory::Retryable
        );
    }
}

#[tokio::test]
async fn codex_copilot_validates_client_model_before_runtime_lookup() {
    let forwarder = test_forwarder(Duration::ZERO, Duration::ZERO);
    let copilot = test_provider_with_type(Some("github_copilot"));
    let adapter = get_adapter(&AppType::Codex).unwrap();
    for body in [
        json!({}),
        json!({"model": null}),
        json!({"model": 42}),
        json!({"model": ""}),
        json!({"model": " \t "}),
        json!({"model": "gpt-5.5"}),
    ] {
        let mut codex_upstream_format = None;
        let error = match forwarder
            .forward(
                &Attempt {
                    app_type: &AppType::Codex,
                    method: &http::Method::POST,
                    provider: &copilot,
                    endpoint: "/v1/responses",
                    headers: &HeaderMap::new(),
                    extensions: &Extensions::new(),
                    adapter: adapter.as_ref(),
                },
                body.clone(),
                &mut codex_upstream_format,
            )
            .await
        {
            Err(error) => error,
            Ok(_) => panic!("expected local validation failure: {body}"),
        };
        if body["model"] == "gpt-5.5" {
            assert!(matches!(&error, ProxyError::ConfigError(message)
                if message.contains("AppHandle") && message.contains("gpt-5.5")));
            assert_eq!(
                forwarder.categorize_proxy_error(&error, &copilot),
                ErrorCategory::Retryable
            );
        } else {
            assert!(matches!(&error, ProxyError::InvalidRequest(message)
                if message.contains("model")));
            assert_eq!(
                forwarder.categorize_proxy_error(&error, &copilot),
                ErrorCategory::NonRetryable
            );
        }
    }
}

#[test]
fn codex_copilot_unavailable_model_is_provider_scoped() {
    let forwarder = test_forwarder(Duration::ZERO, Duration::ZERO);
    let copilot = test_provider_with_type(Some("github_copilot"));
    for api_format in [
        CodexCopilotApiFormat::Auto,
        CodexCopilotApiFormat::OpenaiResponses,
        CodexCopilotApiFormat::OpenaiChat,
    ] {
        let mut body = json!({"model": "gpt-6-astra"});
        let error = apply_codex_copilot_model(&mut body, None, api_format).unwrap_err();
        assert!(matches!(&error, ProxyError::ConfigError(message)
            if message.contains("gpt-6-astra") && message.contains("unavailable")));
        assert_eq!(
            forwarder.categorize_proxy_error(&error, &copilot),
            ErrorCategory::Retryable
        );
        assert_eq!(body["model"], "gpt-6-astra");
    }
}

#[tokio::test]
async fn codex_copilot_capability_failure_tries_healthy_later_provider() {
    let captured = Arc::new(tokio::sync::Mutex::new(Vec::<Value>::new()));
    let app = axum::Router::new().route(
        "/v1/responses",
        axum::routing::post({
            let captured = captured.clone();
            move |axum::Json(body): axum::Json<Value>| {
                let captured = captured.clone();
                async move {
                    captured.lock().await.push(body);
                    axum::Json(json!({"status": "completed", "output": []}))
                }
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = shutdown_rx.await;
            })
            .await
            .unwrap();
    });
    let mut forwarder = test_forwarder(Duration::from_secs(5), Duration::from_secs(5));
    forwarder.max_attempts = 2;
    // Avoid persisting a provider switch outside this in-memory test.
    forwarder.current_provider_id_at_start = "healthy".to_string();
    let copilot = test_provider_with_type(Some("github_copilot"));
    let mut healthy = test_provider_with_type(None);
    healthy.id = "healthy".to_string();
    healthy.settings_config = json!({
        "base_url": format!("http://{addr}/v1"),
        "auth": {"OPENAI_API_KEY": "test-key"}
    });
    let body = json!({"model": "gpt-5.5", "input": "Hello", "stream": false});

    let result = forwarder
        .forward_with_retry(
            &AppType::Codex,
            http::Method::POST,
            "/v1/responses",
            wire_body(body.clone()),
            HeaderMap::new(),
            Extensions::new(),
            &[copilot, healthy],
        )
        .await;

    shutdown_tx.send(()).unwrap();
    server.await.unwrap();
    let result = result.unwrap_or_else(|failure| {
        panic!(
            "Copilot capability failure blocked failover: {}",
            failure.error
        )
    });
    assert_eq!(result.provider.id, "healthy");
    assert_eq!(result.response.status(), StatusCode::OK);
    assert_eq!(
        result.codex_upstream_format,
        Some(CodexUpstreamFormat::NativeResponses)
    );
    assert_eq!(*captured.lock().await, vec![body]);
    assert_eq!(forwarder.status.read().await.success_requests, 1);
}

#[test]
fn native_responses_rewrites_call_id_less_outputs_for_non_xai_providers() {
    let provider = test_provider_with_type(None);
    for call_id in [None, Some(json!(null)), Some(json!("")), Some(json!("  "))] {
        let mut receipt = json!({
            "type": "function_call_output", "id": "fco_receipt",
            "name": "send_message_to_thread", "namespace": "codex_app",
            "output": "<codex_delegation>Review the diff.</codex_delegation>"
        });
        if let Some(call_id) = call_id {
            receipt["call_id"] = call_id;
        }
        let mut body = json!({
            "model": "gpt-example", "previous_response_id": "resp_previous",
            "input": [receipt, {
                "type": "function_call_output", "call_id": "call_previous", "output": "kept"
            }]
        });
        let previous_output = body["input"][1].clone();
        apply_native_responses_request_compat(&provider, &mut body, false).unwrap();
        assert_eq!(body["input"][0]["type"], "message");
        assert_eq!(
            body["input"][0]["content"][0]["text"],
            "[tool_result without a matching tool_call]\n<codex_delegation>Review the diff.</codex_delegation>"
        );
        assert_eq!(body["input"][1], previous_output);
        assert_eq!(body["model"], "gpt-example");
        assert_eq!(body["previous_response_id"], "resp_previous");
        let once = body.clone();
        apply_native_responses_request_compat(&provider, &mut body, false).unwrap();
        assert_eq!(body, once);
    }
}

#[test]
fn native_responses_keeps_official_call_id_less_outputs() {
    let mut provider = test_provider_with_type(None);
    provider.category = Some("official".to_string());
    provider.settings_config = json!({"auth": {}, "config": ""});
    assert!(super::super::providers::is_codex_official_provider(
        &provider
    ));
    let original = json!({
        "model": "gpt-example",
        "input": [{"type": "function_call_output", "name": "automation_update", "output": "receipt"}]
    });
    let mut body = original.clone();
    apply_native_responses_request_compat(&provider, &mut body, false).unwrap();
    assert_eq!(body, original);
}

#[test]
fn native_responses_keeps_hosted_search_and_async_history() {
    let provider = test_provider_with_type(None);
    let original = json!({"input": [
        {"type": "tool_search_output", "execution": "server", "call_id": null,
         "status": "completed", "tools": [{"type": "function", "name": "lookup"}]},
        {"type": "function_call", "call_id": "slow", "name": "shell", "arguments": "{}", "async": true},
        {"type": "message", "role": "assistant", "content": "Still working."},
        {"type": "message", "role": "user", "content": "Query the flight as well."},
        {"type": "function_call", "call_id": "fast", "name": "lookup", "arguments": "{}"},
        {"type": "function_call_output", "call_id": "fast", "output": "flight"},
        {"type": "function_call_output", "call_id": "slow", "output": "done"}
    ]});
    let mut body = original.clone();
    apply_native_responses_request_compat(&provider, &mut body, false).unwrap();
    assert_eq!(body, original);
}

#[test]
fn copilot_grok_native_request_reuses_xai_compat_without_model_fallback() {
    let mut copilot = test_provider_with_type(Some("github_copilot"));
    copilot.settings_config = json!({
        "config": "model = \"gpt-6.1-sol\"",
        "modelCatalog": {"models": [{"model": "gpt-6.1-sol"}]}
    });
    let mut body = grok_desktop_request("grok-4.7");
    let reasoning = body["reasoning"].clone();
    apply_native_responses_request_compat(&copilot, &mut body, false).unwrap();

    assert_eq!(body["model"], "grok-4.7");
    assert_eq!(body["reasoning"], reasoning);
    assert!(body.get("safety_identifier").is_none());
    assert!(body.get("prompt_cache_retention").is_none());
    assert_eq!(body["tools"].as_array().unwrap().len(), 1);
    assert_eq!(body["tools"][0]["type"], "function");
    assert_eq!(body["tools"][0]["name"], "diagnostic__ping");
    assert_eq!(body["tools"][0]["parameters"]["type"], "object");
    assert!(body["tools"][0]["parameters"].get("anyOf").is_none());
    assert!(body["tools"][0].get("external_web_access").is_none());
    assert_eq!(body["input"][0]["name"], "diagnostic__ping");
    assert!(body["input"][0].get("namespace").is_none());
    assert_eq!(body["input"][0]["call_id"], "call_1");
    assert_eq!(body["input"][1]["output"], "pong");
    assert_eq!(body["input"][2]["type"], "message");
    assert_eq!(body["input"][2]["content"][0]["text"], "Continue.");

    let sanitized = body.clone();
    apply_native_responses_request_compat(&copilot, &mut body, false).unwrap();
    assert_eq!(body, sanitized);
}

fn grok_desktop_request(model: &str) -> Value {
    json!({
        "model": model,
        "stream": true,
        "reasoning": {"effort": "high"},
        "safety_identifier": "diagnostic",
        "prompt_cache_retention": "24h",
        "tools": [
            {"type": "namespace", "name": "diagnostic", "tools": [
                {"type": "function", "name": "ping", "external_web_access": false,
                 "parameters": {"anyOf": [
                     {"type": "object", "properties": {"count": {"type": "integer"}}},
                     {"type": "null"}
                 ]}}
            ]},
            {"type": "tool_search"}
        ],
        "input": [
            {"type": "function_call", "namespace": "diagnostic", "name": "ping",
             "call_id": "call_1", "arguments": "{\"count\":1}"},
            {"type": "function_call_output", "call_id": "call_1", "output": "pong"},
            {"type": "agent_message", "content": [{"type": "input_text", "text": "Continue."}]}
        ]
    })
}

#[test]
fn native_grok_compat_leaves_other_copilot_models_and_providers_unchanged() {
    for provider_type in [Some("github_copilot"), None] {
        let provider = test_provider_with_type(provider_type);
        for model in ["gpt-6.1-sol", "claude-sonnet-4.6", "grok-4.7"] {
            if provider_type == Some("github_copilot") && model == "grok-4.7" {
                continue;
            }
            let original = grok_desktop_request(model);
            let mut body = original.clone();
            apply_native_responses_request_compat(&provider, &mut body, false).unwrap();
            assert_eq!(body, original, "{provider_type:?}: {model}");
        }
    }
}

#[test]
fn direct_xai_model_fallback_and_stack_preservation_are_unchanged() {
    let mut xai = test_provider_with_type(Some("xai_oauth"));
    xai.settings_config = json!({"config": "model = \"grok-4.5\""});
    for keep_resolved_model in [false, true] {
        let mut body = grok_desktop_request("gpt-subagent");
        body["presence_penalty"] = json!(1);
        apply_native_responses_request_compat(&xai, &mut body, keep_resolved_model).unwrap();
        assert_eq!(body["tools"][0]["type"], "function");
        if keep_resolved_model {
            assert_eq!(body["model"], "gpt-subagent");
            assert_eq!(body["presence_penalty"], 1);
        } else {
            assert_eq!(body["model"], "grok-4.5");
            assert!(body.get("presence_penalty").is_none());
        }
    }
}

#[test]
fn copilot_grok_namespace_collisions_keep_the_existing_error() {
    let copilot = test_provider_with_type(Some("github_copilot"));
    let mut body = grok_desktop_request("grok-4.7");
    body["tools"].as_array_mut().unwrap().push(json!({
        "type": "function", "name": "diagnostic__ping",
        "parameters": {"type": "object", "properties": {}}
    }));
    let original = body.clone();
    let error = apply_native_responses_request_compat(&copilot, &mut body, false).unwrap_err();
    assert!(matches!(error, ProxyError::TransformError(message) if message.contains("collides")));
    assert_eq!(body, original);
}

#[test]
fn codex_copilot_metadata_selects_advertised_transport_at_forwarding_seam() {
    use super::super::providers::copilot_auth::CopilotModel;
    use super::super::providers::copilot_model_map::resolve_model_with_format;
    use crate::provider::ProviderMeta;

    let forwarder = test_forwarder(Duration::ZERO, Duration::ZERO);
    let copilot = test_provider_with_type(Some("github_copilot"));
    for (metadata, endpoints, expected) in [
        (
            json!({}),
            vec!["/responses", "/chat/completions"],
            Some((CopilotProtocol::Responses, "/responses")),
        ),
        (
            json!({"apiFormat": "openai_chat"}),
            vec!["/v1/chat/completions", "/v1/responses"],
            Some((CopilotProtocol::Responses, "/v1/responses")),
        ),
        (
            json!({"apiFormat": "openai_responses"}),
            vec!["/chat/completions"],
            Some((CopilotProtocol::Chat, "/chat/completions")),
        ),
        (
            json!({"codexCopilotApiFormat": "auto"}),
            vec!["/chat/completions", "/responses"],
            Some((CopilotProtocol::Responses, "/responses")),
        ),
        (
            json!({"codexCopilotApiFormat": "auto"}),
            vec!["/v1/messages", "/v1/chat/completions"],
            Some((CopilotProtocol::Chat, "/v1/chat/completions")),
        ),
        (
            json!({"codexCopilotApiFormat": "openai_chat"}),
            vec!["/responses", "/chat/completions"],
            Some((CopilotProtocol::Chat, "/chat/completions")),
        ),
        (
            json!({"apiFormat": "openai_responses", "codexCopilotApiFormat": "openai_chat"}),
            vec!["/v1/responses", "/v1/chat/completions"],
            Some((CopilotProtocol::Chat, "/v1/chat/completions")),
        ),
        (
            json!({"codexCopilotApiFormat": "openai_responses"}),
            vec!["/chat/completions", "/responses"],
            Some((CopilotProtocol::Responses, "/responses")),
        ),
        (
            json!({"apiFormat": "openai_chat", "codexCopilotApiFormat": "openai_responses"}),
            vec!["/v1/chat/completions", "/v1/responses/"],
            Some((CopilotProtocol::Responses, "/v1/responses")),
        ),
        (
            json!({"codexCopilotApiFormat": "openai_chat"}),
            vec!["/responses"],
            None,
        ),
        (
            json!({"codexCopilotApiFormat": "openai_responses"}),
            vec!["/v1/chat/completions"],
            None,
        ),
        (
            json!({"codexCopilotApiFormat": "auto"}),
            vec!["/v1/messages"],
            None,
        ),
        (
            json!({"codexCopilotApiFormat": "openai_chat"}),
            vec!["/v1/messages"],
            None,
        ),
        (
            json!({"codexCopilotApiFormat": "openai_responses"}),
            vec![],
            None,
        ),
    ] {
        let meta: ProviderMeta = serde_json::from_value(metadata.clone()).unwrap();
        let api_format = CodexCopilotApiFormat::from_meta(Some(&meta));
        let models = [CopilotModel {
            id: "gpt-5.6".to_string(),
            name: "GPT-5.6".to_string(),
            vendor: "OpenAI".to_string(),
            model_picker_enabled: false,
            context_window: Some(400_000),
            supported_endpoints: endpoints.into_iter().map(str::to_string).collect(),
            supports_parallel_tool_calls: None,
            supports_vision: None,
            reasoning_effort: None,
        }];
        let mut body = json!({"model": "GPT-5.6", "input": "Hello"});
        let resolved = resolve_model_with_format("GPT-5.6", &models, api_format);
        let result = apply_codex_copilot_model(&mut body, resolved, api_format);
        if let Some((protocol, endpoint)) = expected {
            let transport = result.unwrap();
            assert_eq!(transport.protocol, protocol, "{metadata}");
            assert_eq!(transport.endpoint, endpoint, "{metadata}");
            assert_eq!(body["model"], "gpt-5.6");
            assert_eq!(body["input"], "Hello");
            let expected_compact = if protocol == CopilotProtocol::Responses {
                "/compact"
            } else {
                ""
            };
            assert_eq!(
                rewrite_codex_endpoint_for_copilot(
                    "/v1/responses/compact?x=1",
                    &transport.endpoint
                ),
                (
                    format!("{endpoint}{expected_compact}?x=1"),
                    Some("x=1".to_string())
                ),
                "{metadata}"
            );
        } else {
            let error = result.unwrap_err();
            assert_eq!(
                forwarder.categorize_proxy_error(&error, &copilot),
                ErrorCategory::Retryable,
                "{metadata}: {error}"
            );
            let ProxyError::ConfigError(message) = error else {
                panic!("expected provider ConfigError for {metadata}");
            };
            assert!(message.contains("gpt-5.6"), "{message}");
            assert!(message.contains("does not advertise"), "{message}");
            if api_format != CodexCopilotApiFormat::Auto {
                assert!(
                    message.contains(metadata["codexCopilotApiFormat"].as_str().unwrap()),
                    "{message}"
                );
            }
            assert_eq!(body["model"], "GPT-5.6");
        }
    }
}

#[test]
fn copilot_endpoint_rewrite_preserves_advertised_version_and_compact() {
    assert_eq!(
        rewrite_codex_endpoint_for_copilot("/v1/responses?x=1", "/responses"),
        ("/responses?x=1".to_string(), Some("x=1".to_string()))
    );
    assert_eq!(
        rewrite_codex_endpoint_for_copilot("/responses", "/v1/responses"),
        ("/v1/responses".to_string(), None)
    );
    assert_eq!(
        rewrite_codex_endpoint_for_copilot("/v1/responses/compact", "/responses"),
        ("/responses/compact".to_string(), None)
    );
}

/// 验证 is_copilot 检测逻辑：通过 provider_type 判断
#[test]
fn copilot_detection_via_provider_type() {
    let provider = test_provider_with_type(Some("github_copilot"));
    assert!(is_managed_copilot_request(
        &AppType::Codex,
        &provider,
        "https://copilot-api.corp.example.com"
    ));
}

/// Claude 保留旧 URL 识别；Codex 必须有显式托管身份，避免接管自建中转。
#[test]
fn copilot_detection_via_base_url() {
    let ordinary = test_provider_with_type(None);
    assert!(is_managed_copilot_request(
        &AppType::Claude,
        &ordinary,
        "https://api.githubcopilot.com"
    ));
    assert!(!is_managed_copilot_request(
        &AppType::Codex,
        &ordinary,
        "https://api.githubcopilot.com"
    ));
    assert!(!is_managed_copilot_request(
        &AppType::Codex,
        &ordinary,
        "https://copilot-api.example.com"
    ));
    assert!(!is_managed_copilot_request(
        &AppType::Claude,
        &ordinary,
        "https://api.anthropic.com"
    ));
}

/// 请求路径只看 providerType 和解析后的请求 URL：旧数据的 env.ANTHROPIC_BASE_URL
/// 让 `Provider::is_github_copilot` 为真，但这里不算。
#[test]
fn copilot_request_detection_ignores_legacy_env_url() {
    let mut legacy = test_provider_with_type(None);
    legacy.settings_config =
        json!({ "env": { "ANTHROPIC_BASE_URL": "https://api.githubcopilot.com" } });
    assert!(legacy.is_github_copilot());
    for app_type in [AppType::Claude, AppType::Codex] {
        assert!(!is_managed_copilot_request(
            &app_type,
            &legacy,
            "https://relay.example.com"
        ));
    }
}

/// 验证动态 endpoint 替换条件
#[test]
fn dynamic_endpoint_replacement_conditions() {
    // 条件：is_copilot && !is_full_url
    let test_cases = [
        (true, false, true, "Copilot + 非 full_url 应该替换"),
        (true, true, false, "Copilot + full_url 不应替换"),
        (false, false, false, "非 Copilot 不应替换"),
        (false, true, false, "非 Copilot + full_url 不应替换"),
    ];

    for (is_copilot, is_full_url, should_replace, desc) in test_cases {
        let will_replace = is_copilot && !is_full_url;
        assert_eq!(will_replace, should_replace, "{desc}");
    }
}

// ===== P3: forwarder 层 media 开关回归测试 =====
// 验证 gate 在 forwarder 这一层的"接线"，而非 media_sanitizer 纯函数本身。

fn forwarder_with_rectifier(config: RectifierConfig) -> RequestForwarder {
    let mut fwd = test_forwarder(Duration::from_secs(1), Duration::from_secs(1));
    fwd.rectifier_config = config;
    fwd
}

fn provider_with_settings(settings_config: Value) -> Provider {
    let mut p = test_provider_with_type(Some("anthropic"));
    p.settings_config = settings_config;
    p
}

fn body_with_image(model: &str) -> Value {
    json!({
        "model": model,
        "messages": [{
            "role": "user",
            "content": [
                { "type": "image", "source": { "type": "base64", "media_type": "image/png", "data": "abc" } }
            ]
        }]
    })
}

fn body_with_codex_input_image(model: &str) -> Value {
    json!({
        "model": model,
        "input": [{
            "role": "user",
            "content": [
                { "type": "input_image", "image_url": "data:image/png;base64,abc" }
            ]
        }]
    })
}

fn body_with_codex_tool_output_image(stringified: bool) -> Value {
    let output = json!({
        "content": [{
            "type": "input_image",
            "image_url": "data:image/png;base64,TOOL_OUTPUT_SENTINEL"
        }]
    });
    json!({
        "model": "any-model",
        "input": [{
            "type": "function_call_output",
            "call_id": "call_1",
            "output": if stringified {
                Value::String(output.to_string())
            } else {
                output
            }
        }]
    })
}

fn body_with_stringified_chat_tool_image() -> Value {
    let content = json!({
        "content": [{
            "type": "image",
            "mimeType": "image/png",
            "data": "CHAT_TOOL_SENTINEL"
        }]
    })
    .to_string();
    json!({
        "model": "any-model",
        "messages": [{
            "role": "tool",
            "tool_call_id": "call_1",
            "content": content
        }]
    })
}

fn body_with_gemini_image() -> Value {
    json!({
        "contents": [{
            "role": "user",
            "parts": [{
                "inlineData": {
                    "mimeType": "image/png",
                    "data": "GEMINI_SENTINEL"
                }
            }]
        }]
    })
}

fn image_unsupported_error() -> ProxyError {
    ProxyError::UpstreamError {
        status: 400,
        body: Some(
            r#"{"error":{"message":"This model does not support image input"}}"#.to_string(),
        ),
    }
}
#[test]
fn prevention_replaces_when_all_switches_on_and_model_in_heuristic_list() {
    let fwd = forwarder_with_rectifier(RectifierConfig::default());
    let provider = provider_with_settings(json!({}));
    let mut body = body_with_image("qwen3-coder-plus");

    let replaced = fwd.apply_media_prevention(&mut body, &provider);

    assert_eq!(replaced, 1, "默认全开 + 名单内模型应预替换");
    assert_eq!(body["messages"][0]["content"][0]["type"], "text");
}

#[test]
fn prevention_skipped_when_media_fallback_off() {
    // 关闭 request_media_fallback：即使名单命中也不预替换。
    let fwd = forwarder_with_rectifier(RectifierConfig {
        request_media_fallback: false,
        ..RectifierConfig::default()
    });
    let provider = provider_with_settings(json!({}));
    let mut body = body_with_image("qwen3-coder-plus");

    let replaced = fwd.apply_media_prevention(&mut body, &provider);

    assert_eq!(replaced, 0);
    assert_eq!(body["messages"][0]["content"][0]["type"], "image");
}

#[test]
fn prevention_skipped_when_master_switch_off() {
    let fwd = forwarder_with_rectifier(RectifierConfig {
        enabled: false,
        ..RectifierConfig::default()
    });
    let provider = provider_with_settings(json!({}));
    let mut body = body_with_image("qwen3-coder-plus");

    assert_eq!(fwd.apply_media_prevention(&mut body, &provider), 0);
    assert_eq!(body["messages"][0]["content"][0]["type"], "image");
}

#[test]
fn prevention_heuristic_off_skips_list_but_keeps_explicit_text_only() {
    // 关闭 request_media_heuristic：名单预测失效，但显式声明 text-only 仍预替换。
    let fwd = forwarder_with_rectifier(RectifierConfig {
        request_media_heuristic: false,
        ..RectifierConfig::default()
    });

    // (a) 名单内模型、无显式声明 → 不再预替换
    let bare_provider = provider_with_settings(json!({}));
    let mut list_body = body_with_image("qwen3-coder-plus");
    assert_eq!(
        fwd.apply_media_prevention(&mut list_body, &bare_provider),
        0,
        "heuristic 关闭后名单模型不应被预替换"
    );
    assert_eq!(list_body["messages"][0]["content"][0]["type"], "image");

    // (b) 显式声明 text-only → 仍预替换（声明驱动，不受 heuristic 开关影响）
    let declared_provider = provider_with_settings(json!({
        "models": [ { "id": "some-text-model", "input": ["text"] } ]
    }));
    let mut declared_body = body_with_image("some-text-model");
    assert_eq!(
        fwd.apply_media_prevention(&mut declared_body, &declared_provider),
        1,
        "显式 text-only 即使关闭 heuristic 也应预替换"
    );
    assert_eq!(declared_body["messages"][0]["content"][0]["type"], "text");
}

#[test]
fn reactive_triggers_when_all_switches_on() {
    let fwd = forwarder_with_rectifier(RectifierConfig::default());
    let body = body_with_image("any-model");
    assert!(fwd.media_retry_should_trigger(
        "Claude",
        false,
        Some(&body),
        &image_unsupported_error()
    ));
}

#[test]
fn reactive_triggers_for_codex_image_url_deserialize_errors() {
    let fwd = forwarder_with_rectifier(RectifierConfig::default());
    let body = body_with_codex_input_image("deepseek-v4-flash");
    let error = ProxyError::UpstreamError {
        status: 400,
        body: Some(
            r#"{"error":{"message":"Failed to deserialize the JSON body into the target type: messages[11]: unknown variant image_url, expected text"}}"#
                .to_string(),
        ),
    };

    assert!(fwd.media_retry_should_trigger("Codex", false, Some(&body), &error));
}

#[test]
fn reactive_triggers_for_openrouter_no_image_endpoint_404() {
    let fwd = forwarder_with_rectifier(RectifierConfig::default());
    let body = body_with_codex_input_image("deepseek/deepseek-v4-flash-0731");
    let error = ProxyError::UpstreamError {
        status: 404,
        body: Some(
            r#"{"error":{"message":"No endpoints found that support image input"}}"#.to_string(),
        ),
    };

    assert!(fwd.media_retry_should_trigger("Codex", false, Some(&body), &error));
}

#[test]
fn reactive_triggers_for_structured_and_stringified_codex_tool_images() {
    let fwd = forwarder_with_rectifier(RectifierConfig::default());

    for stringified in [false, true] {
        let body = body_with_codex_tool_output_image(stringified);
        assert!(
            fwd.media_retry_should_trigger("Codex", false, Some(&body), &image_unsupported_error()),
            "tool-output image should trigger retry (stringified={stringified})"
        );
    }
}

#[test]
fn reactive_triggers_for_chat_tool_and_gemini_images() {
    let fwd = forwarder_with_rectifier(RectifierConfig::default());

    assert!(fwd.media_retry_should_trigger(
        "Claude",
        false,
        Some(&body_with_stringified_chat_tool_image()),
        &image_unsupported_error()
    ));
    assert!(fwd.media_retry_should_trigger(
        "Claude",
        false,
        Some(&body_with_gemini_image()),
        &image_unsupported_error()
    ));
}

#[test]
fn reactive_does_not_treat_context_limit_as_image_rejection() {
    let fwd = forwarder_with_rectifier(RectifierConfig::default());
    let body = body_with_codex_tool_output_image(false);
    let context_error = ProxyError::UpstreamError {
        status: 400,
        body: Some(r#"{"error":{"message":"maximum context length exceeded"}}"#.to_string()),
    };

    assert!(!fwd.media_retry_should_trigger("Codex", false, Some(&body), &context_error));
}

#[test]
fn reactive_skipped_when_media_fallback_off() {
    // 关闭 request_media_fallback：上游报图片错误也不触发兜底重试。
    let fwd = forwarder_with_rectifier(RectifierConfig {
        request_media_fallback: false,
        ..RectifierConfig::default()
    });
    let body = body_with_image("any-model");
    assert!(!fwd.media_retry_should_trigger(
        "Claude",
        false,
        Some(&body),
        &image_unsupported_error()
    ));
}

#[test]
fn reactive_skipped_when_master_switch_off() {
    let fwd = forwarder_with_rectifier(RectifierConfig {
        enabled: false,
        ..RectifierConfig::default()
    });
    let body = body_with_image("any-model");
    assert!(!fwd.media_retry_should_trigger(
        "Claude",
        false,
        Some(&body),
        &image_unsupported_error()
    ));
}

#[test]
fn reactive_unaffected_by_heuristic_switch() {
    // 关闭 request_media_heuristic 不影响反应式兜底——它是上游实测错误后的恢复，不是预测。
    let fwd = forwarder_with_rectifier(RectifierConfig {
        request_media_heuristic: false,
        ..RectifierConfig::default()
    });
    let body = body_with_image("any-model");
    assert!(fwd.media_retry_should_trigger(
        "Claude",
        false,
        Some(&body),
        &image_unsupported_error()
    ));
}

/// Codex Stack 请求的转发改写：身份头、模型名、托管 web_search。
/// 在本机一个空闲端口上起假上游 `app`，返回它的地址。
async fn serve_upstream(app: axum::Router) -> String {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("bind upstream");
    let addr = listener.local_addr().expect("upstream address");
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve upstream");
    });
    format!("http://{addr}")
}

mod codex_stack {
    use super::*;
    use tokio::sync::Mutex;

    struct Seen {
        headers: HeaderMap,
        body: Value,
    }

    struct Upstream {
        base_url: String,
        seen: Arc<Mutex<Vec<Seen>>>,
    }

    /// 什么都回 200 的假上游，记下请求头和请求体。
    async fn upstream() -> Upstream {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let app = {
            let seen = seen.clone();
            axum::Router::new().fallback(move |headers: HeaderMap, body: Bytes| {
                let seen = seen.clone();
                async move {
                    seen.lock().await.push(Seen {
                        headers,
                        body: serde_json::from_slice(&body).unwrap_or(Value::Null),
                    });
                    (
                        StatusCode::OK,
                        [(http::header::CONTENT_TYPE, "application/json")],
                        json!({ "id": "r1", "object": "response", "output": [] }).to_string(),
                    )
                }
            })
        };
        Upstream {
            base_url: serve_upstream(app).await,
            seen,
        }
    }

    /// 一家 Codex 第三方供应商：行里的模型是 `row-model`，目录里只有 `listed`。
    fn provider(upstream: &Upstream, api_format: &str) -> Provider {
        let mut provider = test_provider_with_type(None);
        provider.id = format!("p-{api_format}");
        provider.settings_config = json!({
            "auth": { "OPENAI_API_KEY": "sk-third-party" },
            "config": format!(
                "model_provider = \"custom\"\nmodel = \"row-model\"\n\n[model_providers.custom]\nname = \"custom\"\nbase_url = \"{}/v1\"\nwire_api = \"responses\"\n",
                upstream.base_url
            ),
            "modelCatalog": { "models": [{ "model": "listed" }] },
        });
        provider.meta = Some(crate::provider::ProviderMeta {
            api_format: Some(api_format.to_string()),
            ..Default::default()
        });
        provider
    }

    fn forwarder(stack: bool) -> RequestForwarder {
        let _ = rustls::crypto::ring::default_provider().install_default();
        test_forwarder(Duration::from_secs(5), Duration::from_secs(5)).stack_request(stack)
    }

    /// 官方做路由时 Codex 每个请求都带的 ChatGPT 身份。
    fn chatgpt_headers() -> HeaderMap {
        let mut headers = HeaderMap::new();
        for (name, value) in [
            ("content-type", "application/json"),
            ("authorization", "Bearer chatgpt-access-token"),
            ("chatgpt-account-id", "acct-workspace"),
            ("originator", "codex_cli_rs"),
            ("session_id", "session-1"),
            ("x-codex-beta-features", "remote_compaction_v2"),
            ("x-codex-turn-state", "state"),
            ("openai-beta", "responses=experimental"),
            ("x-oai-attestation", "device-proof"),
            ("x-openai-fedramp", "true"),
        ] {
            headers.insert(
                http::HeaderName::from_static(name),
                HeaderValue::from_static(value),
            );
        }
        headers
    }

    fn body(model: &str, tools: Value) -> Value {
        json!({
            "model": model,
            "input": [{ "role": "user", "content": [{ "type": "input_text", "text": "hi" }] }],
            "tools": tools,
            "stream": false
        })
    }

    async fn send(
        forwarder: &RequestForwarder,
        upstream: &Upstream,
        provider: Provider,
        endpoint: &str,
        body: Value,
    ) -> Seen {
        forwarder
            .forward_with_retry(
                &AppType::Codex,
                http::Method::POST,
                endpoint,
                wire_body(body.clone()),
                chatgpt_headers(),
                Extensions::new(),
                &[provider],
            )
            .await
            .map_err(|error| error.error)
            .expect("forward");
        upstream.seen.lock().await.pop().expect("upstream request")
    }

    /// 隐私过滤在转发器出口执行：转换成哪种上游格式都过滤，CC Switch 自己包装的
    /// 压缩摘要也过滤；工具结果仍跟随子开关。工具定义的说明文字不碰（Claude Code 的
    /// Bash 工具说明里有 `noreply@anthropic.com`，模型会照抄进提交信息）。
    #[tokio::test]
    async fn privacy_filter_runs_on_the_final_upstream_body() {
        const SECRET: &str = "Hunter2xyz";
        let upstream = upstream().await;
        let request = json!({
            "model": "listed",
            "input": [
                { "role": "user", "content": [
                    { "type": "input_text", "text": format!("我的密码是 {SECRET}") }
                ]},
                { "type": "compaction", "encrypted_content":
                    crate::proxy::providers::codex_compaction::encode_compaction_summary(
                        &format!("earlier: password = {SECRET}")
                    ) },
                { "type": "function_call", "call_id": "c1", "name": "lookup", "arguments": "{}" },
                { "type": "function_call_output", "call_id": "c1",
                  "output": format!("password: {SECRET}") }
            ],
            "tools": [{
                "type": "function",
                "name": "lookup",
                "description": "Co-Authored-By: Claude <noreply@anthropic.com>",
                "parameters": { "type": "object", "properties": {} }
            }],
            "stream": false
        });
        let config = |filter_tool_results| PrivacyFilterConfig {
            enabled: true,
            filter_tool_results,
        };
        for api_format in ["openai_responses", "openai_chat", "anthropic"] {
            // 总开关打开、子开关关闭：只剩工具结果里的原文
            let seen = send(
                &forwarder(false).privacy_filter(config(false)),
                &upstream,
                provider(&upstream, api_format),
                "/responses",
                request.clone(),
            )
            .await;
            let sent = seen.body.to_string();
            let decoded = decoded_compaction_summaries(&seen.body).join("\n");
            assert_eq!(
                format!("{sent}{decoded}").matches(SECRET).count(),
                1,
                "{api_format}: only the tool result keeps the secret: {sent} {decoded}"
            );
            assert!(
                sent.contains(&format!("password: {SECRET}")),
                "{api_format}: {sent}"
            );
            assert!(
                sent.contains("<noreply@anthropic.com>"),
                "{api_format}: tool descriptions are left alone: {sent}"
            );

            // 子开关打开：一处都不剩
            let seen = send(
                &forwarder(false).privacy_filter(config(true)),
                &upstream,
                provider(&upstream, api_format),
                "/responses",
                request.clone(),
            )
            .await;
            let sent = seen.body.to_string();
            let decoded = decoded_compaction_summaries(&seen.body).join("\n");
            assert!(!sent.contains(SECRET), "{api_format}: {sent}");
            assert!(!decoded.contains(SECRET), "{api_format}: {decoded}");
            assert!(
                sent.contains("<noreply@anthropic.com>"),
                "{api_format}: {sent}"
            );
        }

        // 开关关闭：三处原文都到了上游。第三方原生 Responses 收到的压缩摘要已被展开成
        // 明文用户消息（`scrub_ccswitch_state_for_official` 之前的转换），
        // 这正是旧版在 handler 入口过滤时漏掉的内容。
        let seen = send(
            &forwarder(false),
            &upstream,
            provider(&upstream, "openai_responses"),
            "/responses",
            request.clone(),
        )
        .await;
        let sent = seen.body.to_string();
        let decoded = decoded_compaction_summaries(&seen.body).join("\n");
        assert_eq!(
            format!("{sent}{decoded}").matches(SECRET).count(),
            3,
            "{sent}"
        );
    }

    /// 供应商配置的本地 body 覆写在出口之前合并，覆写进去的内容同样要过滤
    #[tokio::test]
    async fn privacy_filter_covers_local_body_overrides() {
        let upstream = upstream().await;
        for api_format in ["openai_responses", "openai_chat", "anthropic"] {
            let mut provider = provider(&upstream, api_format);
            provider
                .meta
                .as_mut()
                .unwrap()
                .local_proxy_request_overrides =
                Some(crate::provider::LocalProxyRequestOverrides {
                    headers: Default::default(),
                    body: Some(json!({ "instructions": "我的密码是 Hunter2xyz" })),
                });
            let seen = send(
                &forwarder(false).privacy_filter(PrivacyFilterConfig {
                    enabled: true,
                    filter_tool_results: false,
                }),
                &upstream,
                provider,
                "/responses",
                body("listed", json!([])),
            )
            .await;
            let sent = seen.body.to_string();
            assert!(!sent.contains("Hunter2xyz"), "{api_format}: {sent}");
            // 占位符跟随系统语言，只断言结构
            let instructions = seen.body["instructions"].as_str().unwrap_or_default();
            assert!(
                instructions.starts_with("我的密码是 ["),
                "{api_format}: {sent}"
            );
        }
    }

    /// 请求体里 CC Switch 包装的压缩摘要解开后的正文
    fn decoded_compaction_summaries(body: &Value) -> Vec<String> {
        body["input"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|item| item["encrypted_content"].as_str())
            .filter_map(crate::proxy::providers::codex_compaction::decode_compaction_summary)
            .collect()
    }

    #[tokio::test]
    async fn stacked_requests_drop_chatgpt_identity_but_keep_native_responses_beta_marker() {
        let upstream = upstream().await;
        for api_format in ["openai_responses", "openai_chat", "anthropic"] {
            let seen = send(
                &forwarder(true),
                &upstream,
                provider(&upstream, api_format),
                "/responses",
                body("listed", json!([])),
            )
            .await;
            for name in [
                "chatgpt-account-id",
                "originator",
                "session_id",
                "x-codex-turn-state",
                "openai-beta",
                "x-oai-attestation",
                "x-openai-fedramp",
            ] {
                assert!(
                    !seen.headers.contains_key(name),
                    "{api_format}: {name} reached a third party"
                );
            }
            assert_eq!(
                seen.headers.contains_key("x-codex-beta-features"),
                api_format == "openai_responses",
                "{api_format}: protocol marker handling"
            );
            let auth = seen
                .headers
                .get("authorization")
                .or_else(|| seen.headers.get("x-api-key"))
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default()
                .to_string();
            assert!(auth.contains("sk-third-party"), "{api_format}: {auth}");
            assert!(!auth.contains("chatgpt"), "{api_format}: {auth}");
        }

        // Routing requests to a third party keep the session and client headers
        // (cache keys, protocol marker) but never the ChatGPT account identity:
        // the client may be the built-in `openai` provider carrying the login.
        for api_format in ["openai_responses", "openai_chat"] {
            let seen = send(
                &forwarder(false),
                &upstream,
                provider(&upstream, api_format),
                "/responses",
                body("listed", json!([])),
            )
            .await;
            for name in ["session_id", "originator", "x-codex-turn-state"] {
                assert!(seen.headers.contains_key(name), "{api_format}: {name}");
            }
            for name in [
                "chatgpt-account-id",
                "x-oai-attestation",
                "x-openai-fedramp",
            ] {
                assert!(
                    !seen.headers.contains_key(name),
                    "{api_format}: {name} reached a third party"
                );
            }
            let auth = seen
                .headers
                .get("authorization")
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default();
            assert!(auth.contains("sk-third-party"), "{api_format}: {auth}");
        }
        let seen = send(
            &forwarder(false),
            &upstream,
            provider(&upstream, "openai_responses"),
            "/responses",
            body("listed", json!([])),
        )
        .await;
        assert!(seen.headers.contains_key("x-codex-beta-features"));
    }

    #[test]
    fn chatgpt_identity_reaches_only_official_or_managed_account_upstreams() {
        let strip = |key: &str, passthrough: bool, unmanaged: bool| {
            should_strip_codex_client_fingerprint_header(
                key,
                "/responses",
                false,
                false,
                false,
                passthrough,
                unmanaged,
            )
        };
        for key in ["chatgpt-account-id", "x-oai-attestation", "version"] {
            assert!(strip(key, false, true), "{key} to a third party");
            assert!(!strip(key, true, true), "{key} to the official upstream");
            assert!(
                !strip(key, false, false),
                "{key} to a managed ChatGPT account"
            );
        }
        for key in [
            "session_id",
            "originator",
            "x-codex-turn-state",
            "openai-beta",
        ] {
            assert!(!strip(key, false, true), "{key} is not account identity");
        }
    }

    #[tokio::test]
    async fn native_responses_forwarder_preserves_call_less_receipt_content() {
        let upstream = upstream().await;
        let mut request = body("listed", json!([]));
        request["input"] = json!([
            {"type": "function_call_output", "id": "fco_page", "namespace": "agent",
             "name": "codex_apps_open_page", "output": "{\"page_id\":null}"},
            {"type": "function_call_output", "call_id": " ", "output": [
                {"type": "input_text", "text": "page selection"},
                {"type": "input_image", "image_url": "data:image/png;base64,AAAA"}
            ]},
            {"role": "user", "content": "Continue."}
        ]);
        for stack in [false, true] {
            let seen = send(
                &forwarder(stack),
                &upstream,
                provider(&upstream, "openai_responses"),
                "/responses",
                request.clone(),
            )
            .await;
            let input = seen.body["input"].as_array().unwrap();
            assert_eq!(input.len(), 3);
            assert_eq!(input[0]["type"], "message");
            assert_eq!(
                input[0]["content"][0]["text"],
                "[tool_result without a matching tool_call]\n{\"page_id\":null}"
            );
            assert_eq!(input[1]["type"], "message");
            assert_eq!(input[1]["content"][1], request["input"][1]["output"][0]);
            assert_eq!(input[1]["content"][2], request["input"][1]["output"][1]);
            assert_eq!(input[2], request["input"][2]);
        }
    }

    /// Native Responses third parties cannot consume Codex's private compaction
    /// trigger. Rewrite it into a summary turn that prevents new tool calls
    /// while preserving tool definitions and historical tool-call references.
    #[tokio::test]
    async fn native_third_party_compaction_becomes_summary_turn() {
        use crate::proxy::providers::codex_compaction::{
            encode_compaction_summary, COMPACT_PROMPT, SUMMARY_PREFIX,
        };
        let upstream = upstream().await;
        let mut request = body(
            "listed",
            json!([{ "type": "function", "name": "shell", "parameters": { "type": "object" } }]),
        );
        request["input"] = json!([
            { "type": "compaction", "encrypted_content": "gAAAA-openai" },
            { "type": "compaction", "encrypted_content": encode_compaction_summary("prior work") },
            { "role": "user", "content": [{ "type": "input_text", "text": "hi" }] },
            { "type": "compaction_trigger" }
        ]);
        let seen = send(
            &forwarder(true),
            &upstream,
            provider(&upstream, "openai_responses"),
            "/responses",
            request,
        )
        .await;
        assert_eq!(
            seen.body["tools"],
            json!([{ "type": "function", "name": "shell", "parameters": { "type": "object" } }])
        );
        assert_eq!(seen.body["tool_choice"], "none");
        let input = seen.body["input"].as_array().unwrap();
        assert_eq!(
            input[0],
            json!({ "type": "compaction", "encrypted_content": "gAAAA-openai" })
        );
        assert_eq!(
            input[1]["content"][0]["text"],
            format!("{SUMMARY_PREFIX}\nprior work")
        );
        assert!(input
            .iter()
            .all(|item| item["type"] != "compaction_trigger"));
        assert_eq!(input.last().unwrap()["content"][0]["text"], COMPACT_PROMPT);

        // 同一家上游自己签发的压缩密文：请求逐字节不改，不能把已压缩的历史换成占位文字。
        let mut own_history = body("listed", json!([]));
        own_history["input"] = json!([
            { "type": "compaction", "id": "cmp_1", "encrypted_content": "gAAAA-issued-here" },
            { "role": "user", "content": [{ "type": "input_text", "text": "go on" }] }
        ]);
        let seen = send(
            &forwarder(false),
            &upstream,
            provider(&upstream, "openai_responses"),
            "/responses",
            own_history.clone(),
        )
        .await;
        assert_eq!(seen.body["input"], own_history["input"]);

        // 对照：普通请求逐字节不改。
        let plain = body(
            "listed",
            json!([{ "type": "function", "name": "shell", "parameters": { "type": "object" } }]),
        );
        let seen = send(
            &forwarder(true),
            &upstream,
            provider(&upstream, "openai_responses"),
            "/responses",
            plain.clone(),
        )
        .await;
        assert_eq!(seen.body["tools"], plain["tools"]);
        assert_eq!(seen.body["input"], plain["input"]);
    }

    /// 剥身份头只针对 Codex：Claude Code 的 Stack 请求照常带着 Anthropic SDK 的请求头。
    #[tokio::test]
    async fn claude_stacked_requests_keep_sdk_headers() {
        let upstream = upstream().await;
        let mut provider = test_provider_with_type(None);
        provider.settings_config = json!({
            "env": {
                "ANTHROPIC_BASE_URL": upstream.base_url,
                "ANTHROPIC_AUTH_TOKEN": "sk-third-party",
            }
        });
        let mut headers = HeaderMap::new();
        for (name, value) in [
            ("content-type", "application/json"),
            ("x-stainless-lang", "js"),
            ("x-stainless-package-version", "0.60.0"),
        ] {
            headers.insert(
                http::HeaderName::from_static(name),
                HeaderValue::from_static(value),
            );
        }
        forwarder(true)
            .forward_with_retry(
                &AppType::Claude,
                http::Method::POST,
                "/v1/messages",
                wire_body(json!({
                    "model": "stacked-model",
                    "max_tokens": 16,
                    "messages": [{ "role": "user", "content": "hi" }]
                })),
                headers,
                Extensions::new(),
                &[provider],
            )
            .await
            .map_err(|error| error.error)
            .expect("forward");
        let seen = upstream.seen.lock().await.pop().expect("upstream request");
        assert_eq!(seen.headers["x-stainless-lang"], "js");
        assert_eq!(seen.headers["x-stainless-package-version"], "0.60.0");
        assert_eq!(seen.body["model"], "stacked-model");
    }

    #[tokio::test]
    async fn stacked_requests_are_not_rewritten_to_the_rows_model() {
        let upstream = upstream().await;
        for api_format in ["openai_responses", "openai_chat", "anthropic"] {
            let seen = send(
                &forwarder(true),
                &upstream,
                provider(&upstream, api_format),
                "/responses",
                body("not-listed", json!([])),
            )
            .await;
            assert_eq!(seen.body["model"], "not-listed", "{api_format}");
        }

        // 对照：路由请求发往 Chat 转换时，目录外的模型换成行里的模型。
        let seen = send(
            &forwarder(false),
            &upstream,
            provider(&upstream, "openai_chat"),
            "/responses",
            body("not-listed", json!([])),
        )
        .await;
        assert_eq!(seen.body["model"], "row-model");
    }

    #[tokio::test]
    async fn stacked_requests_drop_hosted_web_search_only_where_it_is_rejected() {
        let upstream = upstream().await;
        let function = json!({ "type": "function", "name": "shell", "parameters": {} });
        let tools = json!([function.clone(), { "type": "web_search" }]);
        let with_choice = |model: &str| {
            let mut body = body(model, tools.clone());
            body["tool_choice"] = json!({ "type": "web_search" });
            body
        };

        // GLM 的原生 Responses 网关不认托管 web_search：去掉工具和指向它的 tool_choice。
        for endpoint in ["/responses", "/responses/compact"] {
            let seen = send(
                &forwarder(true),
                &upstream,
                provider(&upstream, "openai_responses"),
                endpoint,
                with_choice("glm-5.2"),
            )
            .await;
            assert_eq!(seen.body["tools"], json!([function.clone()]), "{endpoint}");
            assert!(seen.body.get("tool_choice").is_none(), "{endpoint}");
        }

        // 只剩它一个工具时整个 `tools` 删掉。
        let seen = send(
            &forwarder(true),
            &upstream,
            provider(&upstream, "openai_responses"),
            "/responses",
            body("glm-5.2", json!([{ "type": "web_search" }])),
        )
        .await;
        assert!(seen.body.get("tools").is_none(), "{}", seen.body);

        // 对照：支持的上游、路由请求都原样转发。
        for (stack, model) in [(true, "deepseek-v4-pro"), (false, "glm-5.2")] {
            let seen = send(
                &forwarder(stack),
                &upstream,
                provider(&upstream, "openai_responses"),
                "/responses",
                with_choice(model),
            )
            .await;
            assert_eq!(seen.body["tools"], tools, "stack={stack} {model}");
            assert_eq!(seen.body["tool_choice"]["type"], "web_search");
        }
    }

    /// Codex 0.163+ 给每个请求带 `include: ["reasoning.encrypted_content",
    /// "output.encrypted_content"]`。后一个是 ChatGPT 后端私有能力，严格的第三方与本地
    /// 原生 Responses 网关会拿它整条 400（#9042）。
    #[tokio::test]
    async fn third_party_requests_drop_codex_private_include_markers() {
        let upstream = upstream().await;
        let with_include = |include: Value| {
            let mut body = body("listed", json!([]));
            body["include"] = include;
            body
        };

        // 只掉私有那个，其余标记原样留着。
        let seen = send(
            &forwarder(true),
            &upstream,
            provider(&upstream, "openai_responses"),
            "/responses",
            with_include(json!([
                "reasoning.encrypted_content",
                "output.encrypted_content"
            ])),
        )
        .await;
        assert_eq!(seen.body["include"], json!(["reasoning.encrypted_content"]));

        // 只剩私有标记时整个键去掉，别留一个空数组。
        let seen = send(
            &forwarder(true),
            &upstream,
            provider(&upstream, "openai_responses"),
            "/responses",
            with_include(json!(["output.encrypted_content"])),
        )
        .await;
        assert!(seen.body.get("include").is_none(), "{}", seen.body);

        // 路由请求直连本地网关时一样会中招，同样要掉。
        let seen = send(
            &forwarder(false),
            &upstream,
            provider(&upstream, "openai_responses"),
            "/responses",
            with_include(json!(["output.encrypted_content"])),
        )
        .await;
        assert!(seen.body.get("include").is_none(), "{}", seen.body);
    }

    /// 第三方路由写成内置 openai 后，Codex 把只发给 OpenAI 的字段也带进第三方请求：
    /// 条目上的 `internal_chat_message_metadata_passthrough`、非推理条目的密文、
    /// `configuration_update` 条目、`stream_options` 与 `access_programs`（#9042）。
    /// 去掉后应与 Codex 对自定义表发的一样，推理与压缩的密文、其余字段原样留着。
    #[tokio::test]
    async fn third_party_requests_drop_openai_only_fields() {
        let upstream = upstream().await;
        let metadata = json!({ "turn_id": "t1" });
        let mut request = body("listed", json!([]));
        request["input"] = json!([
                {
                    "type": "message",
                    "role": "user",
                    "content": [{ "type": "input_text", "text": "hi" }],
                    "internal_chat_message_metadata_passthrough": metadata
                },
                { "type": "configuration_update", "reasoning": { "effort": "high" } },
                {
                    "type": "reasoning",
                    "summary": [],
                    "encrypted_content": "gAAAA-reasoning",
                    "internal_chat_message_metadata_passthrough": metadata
                },
                {
                    "type": "message",
                    "role": "assistant",
                    "content": [{ "type": "output_text", "text": "ok" }],
                    "encrypted_content": "gAAAA-message"
                },
                {
                    "type": "function_call",
                    "call_id": "c1",
                    "name": "spawn_agent",
                    "arguments": "{}",
                    "encrypted_content": "gAAAA-call",
                    "encrypted_function_args": ["message"]
                },
                { "type": "function_call_output", "call_id": "c1", "output": "done" }
        ]);
        request["include"] = json!(["reasoning.encrypted_content", "output.encrypted_content"]);
        request["stream_options"] = json!({ "reasoning_summary_delivery": "sequential_cutoff" });
        request["access_programs"] = json!({ "cyber": "standard" });

        let seen = send(
            &forwarder(false),
            &upstream,
            provider(&upstream, "openai_responses"),
            "/responses",
            request,
        )
        .await;

        assert_eq!(seen.body["include"], json!(["reasoning.encrypted_content"]));
        assert!(seen.body.get("stream_options").is_none(), "{}", seen.body);
        assert!(seen.body.get("access_programs").is_none(), "{}", seen.body);
        assert_eq!(
            seen.body["input"],
            json!([
                {
                    "type": "message",
                    "role": "user",
                    "content": [{ "type": "input_text", "text": "hi" }]
                },
                { "type": "reasoning", "summary": [], "encrypted_content": "gAAAA-reasoning" },
                {
                    "type": "message",
                    "role": "assistant",
                    "content": [{ "type": "output_text", "text": "ok" }]
                },
                { "type": "function_call", "call_id": "c1", "name": "spawn_agent", "arguments": "{}" },
                { "type": "function_call_output", "call_id": "c1", "output": "done" }
            ])
        );
    }

    /// 官方 ChatGPT 后端认这些字段；转 Chat / Anthropic 的路径本来就不带它们。都跳过。
    /// 官方那条路实测只能打到 chatgpt.com，这里直接验门控。
    #[test]
    fn openai_only_fields_gate_skips_official_and_converted_requests() {
        let third_party = provider(
            &Upstream {
                base_url: "https://relay.example.com".to_string(),
                seen: Arc::new(Mutex::new(Vec::new())),
            },
            "openai_responses",
        );
        let mut official = third_party.clone();
        official.id = crate::database::CODEX_OFFICIAL_PROVIDER_ID.to_string();
        official.category = Some("official".to_string());

        assert!(should_strip_codex_openai_only_fields(
            &AppType::Codex,
            &third_party,
            false,
            false
        ));
        assert!(!should_strip_codex_openai_only_fields(
            &AppType::Codex,
            &official,
            false,
            false
        ));
        // ChatGPT 托管账号发往 ChatGPT 后端，同样认这些字段。
        let mut managed = third_party.clone();
        managed.meta = Some(crate::provider::ProviderMeta {
            provider_type: Some("codex_oauth".to_string()),
            ..Default::default()
        });
        assert!(!should_strip_codex_openai_only_fields(
            &AppType::Codex,
            &managed,
            false,
            false
        ));
        // 转 Chat / 转 Anthropic 的路径不带原生 Responses 字段。
        assert!(!should_strip_codex_openai_only_fields(
            &AppType::Codex,
            &third_party,
            true,
            false
        ));
        assert!(!should_strip_codex_openai_only_fields(
            &AppType::Codex,
            &third_party,
            false,
            true
        ));
        // Grok Build 走 xAI 自己的清洗，不归这里管。
        assert!(!should_strip_codex_openai_only_fields(
            &AppType::GrokBuild,
            &third_party,
            false,
            false
        ));
    }

    /// 按脚本依次回应的假上游，记下请求头和请求体。
    async fn scripted_upstream(script: Vec<(u16, Value)>) -> Upstream {
        let script = Arc::new(Mutex::new(std::collections::VecDeque::from(script)));
        let seen = Arc::new(Mutex::new(Vec::new()));
        let app = {
            let seen = seen.clone();
            axum::Router::new().fallback(move |headers: HeaderMap, body: Bytes| {
                let script = script.clone();
                let seen = seen.clone();
                async move {
                    seen.lock().await.push(Seen {
                        headers,
                        body: serde_json::from_slice(&body).unwrap_or(Value::Null),
                    });
                    let (status, body) = script
                        .lock()
                        .await
                        .pop_front()
                        .expect("upstream script exhausted");
                    (
                        StatusCode::from_u16(status).expect("status"),
                        [(http::header::CONTENT_TYPE, "application/json")],
                        body.to_string(),
                    )
                }
            })
        };
        Upstream {
            base_url: serve_upstream(app).await,
            seen,
        }
    }

    fn blob_rejection() -> (u16, Value) {
        (
            400,
            json!({ "error": {
                "type": "invalid_request_error",
                "code": "invalid_encrypted_content",
                "message": "The encrypted content could not be verified."
            } }),
        )
    }

    fn response_ok() -> (u16, Value) {
        (
            200,
            json!({ "id": "r1", "object": "response", "output": [] }),
        )
    }

    /// 上一轮是别家原生 Responses 上游，历史里留着它签发的推理密文。
    fn history_with_foreign_reasoning() -> Value {
        let mut request = body("listed", json!([]));
        request["input"] = json!([
            { "role": "user", "content": [{ "type": "input_text", "text": "hi" }] },
            { "type": "reasoning", "id": "rs_1", "summary": [], "encrypted_content": "gAAAA-other-org" },
            { "type": "message", "id": "msg_1", "role": "assistant",
              "content": [{ "type": "output_text", "text": "a" }] },
            { "role": "user", "content": [{ "type": "input_text", "text": "again" }] }
        ]);
        request
    }

    async fn forward_codex(
        forwarder: &RequestForwarder,
        provider: Provider,
    ) -> Result<ForwardResult, ForwardError> {
        forwarder
            .forward_with_retry(
                &AppType::Codex,
                http::Method::POST,
                "/responses",
                wire_body(history_with_foreign_reasoning()),
                chatgpt_headers(),
                Extensions::new(),
                &[provider],
            )
            .await
    }

    fn carries_reasoning(seen: &Seen) -> bool {
        seen.body["input"]
            .as_array()
            .is_some_and(|input| input.iter().any(|item| item["type"] == "reasoning"))
    }

    /// 上游明确说验不了别家签发的密文：去掉推理条目，对同一家只重试一次。
    #[tokio::test]
    async fn foreign_blob_rejection_is_retried_once_without_reasoning() {
        for stack in [true, false] {
            let upstream = scripted_upstream(vec![blob_rejection(), response_ok()]).await;
            let result =
                forward_codex(&forwarder(stack), provider(&upstream, "openai_responses")).await;
            assert!(result.is_ok(), "stack={stack}");
            let seen = upstream.seen.lock().await;
            assert_eq!(seen.len(), 2, "stack={stack}");
            assert!(carries_reasoning(&seen[0]));
            assert!(!carries_reasoning(&seen[1]));
            assert_eq!(seen[1].body["input"].as_array().unwrap().len(), 3);
        }

        // 重试仍被拒：不再重试，把第二次的错误交回客户端。
        let upstream = scripted_upstream(vec![blob_rejection(), blob_rejection()]).await;
        let error = forward_codex(&forwarder(true), provider(&upstream, "openai_responses"))
            .await
            .err()
            .expect("still rejected");
        assert!(matches!(
            error.error,
            ProxyError::UpstreamError { status: 400, .. }
        ));
        assert_eq!(upstream.seen.lock().await.len(), 2);
    }

    /// Stack 模式下官方压缩过、再切到别家原生 Responses 模型：官方的压缩密文原样发出，
    /// 那家网关不认识、报错措辞又不在清单里时，只换掉压缩条目对同一家重试一次，
    /// 这家自己的推理条目照旧发。
    #[tokio::test]
    async fn unrecognized_compaction_is_replaced_after_a_native_third_party_rejects_it() {
        use crate::proxy::providers::codex_compaction::OPAQUE_COMPACTION_NOTE;
        let unsupported = (
            400,
            json!({ "error": { "message": "unsupported input item type: compaction" } }),
        );
        let upstream = scripted_upstream(vec![unsupported, response_ok()]).await;
        let mut request = history_with_foreign_reasoning();
        request["input"].as_array_mut().unwrap().insert(
            0,
            json!({ "type": "compaction", "encrypted_content": "gAAAA-openai" }),
        );
        forwarder(true)
            .forward_with_retry(
                &AppType::Codex,
                http::Method::POST,
                "/responses",
                wire_body(request.clone()),
                chatgpt_headers(),
                Extensions::new(),
                &[provider(&upstream, "openai_responses")],
            )
            .await
            .map_err(|error| error.error)
            .expect("retried");

        let seen = upstream.seen.lock().await;
        assert_eq!(seen.len(), 2);
        assert_eq!(seen[0].body["input"][0]["type"], "compaction");
        let retried = seen[1].body["input"].as_array().unwrap();
        assert_eq!(retried.len(), 5);
        assert_eq!(retried[0]["content"][0]["text"], OPAQUE_COMPACTION_NOTE);
        assert!(carries_reasoning(&seen[1]));
    }

    /// Kimi For Coding 不收 `tool_choice: "none"`，只回「Invalid request Error」：压缩回合
    /// 被拒后去掉 `tool_choice` 对同一家重试一次，其余改写照旧。
    #[tokio::test]
    async fn compaction_rejected_for_tool_choice_none_retries_without_it() {
        use crate::proxy::providers::codex_compaction::COMPACT_PROMPT;
        let rejected = (
            400,
            json!({ "error": { "message": "Invalid request Error", "type": "invalid_request_error" } }),
        );
        let upstream = scripted_upstream(vec![rejected, response_ok()]).await;
        let mut request = body(
            "listed",
            json!([{ "type": "function", "name": "shell", "parameters": { "type": "object" } }]),
        );
        request["input"] = json!([
            { "role": "user", "content": [{ "type": "input_text", "text": "hi" }] },
            { "type": "compaction_trigger" }
        ]);
        forwarder(true)
            .forward_with_retry(
                &AppType::Codex,
                http::Method::POST,
                "/responses",
                wire_body(request.clone()),
                chatgpt_headers(),
                Extensions::new(),
                &[provider(&upstream, "openai_responses")],
            )
            .await
            .map_err(|error| error.error)
            .expect("retried");

        let seen = upstream.seen.lock().await;
        assert_eq!(seen.len(), 2);
        assert_eq!(seen[0].body["tool_choice"], "none");
        assert!(
            seen[1].body.get("tool_choice").is_none(),
            "{}",
            seen[1].body
        );
        assert!(seen[1].body.get("tools").is_some());
        let input = seen[1].body["input"].as_array().unwrap();
        assert!(input
            .iter()
            .all(|item| item["type"] != "compaction_trigger"));
        assert_eq!(input.last().unwrap()["content"][0]["text"], COMPACT_PROMPT);
    }

    /// 去掉 `tool_choice` 的重试只给 Codex 原生 Responses 第三方的压缩回合、且只在 400 时。
    #[test]
    fn compaction_tool_choice_retry_is_limited_to_rejected_third_party_compactions() {
        let upstream = Upstream {
            base_url: "https://relay.example".to_string(),
            seen: Arc::new(tokio::sync::Mutex::new(Vec::new())),
        };
        let third_party = provider(&upstream, "openai_responses");
        let mut official = third_party.clone();
        official.id = crate::database::CODEX_OFFICIAL_PROVIDER_ID.to_string();
        official.category = Some("official".to_string());
        let compaction = json!({
            "tools": [{ "type": "function", "name": "shell" }],
            "input": [{ "type": "compaction_trigger" }]
        });
        let turn = json!({
            "tools": [{ "type": "function", "name": "shell" }],
            "input": [{ "role": "user", "content": [] }]
        });
        let rejected = ProxyError::UpstreamError {
            status: 400,
            body: Some("Invalid request Error".to_string()),
        };
        let overloaded = ProxyError::UpstreamError {
            status: 529,
            body: None,
        };
        let forwarder = forwarder(false);
        let native = Some(CodexUpstreamFormat::NativeResponses);
        let gate = |app: AppType, provider: &Provider, format, request: &Value, error| {
            forwarder
                .compaction_tool_choice_retry_body(&app, provider, format, request, error)
                .is_some()
        };

        assert!(gate(
            AppType::Codex,
            &third_party,
            native,
            &compaction,
            &rejected
        ));
        assert!(!gate(
            AppType::Codex,
            &official,
            native,
            &compaction,
            &rejected
        ));
        assert!(!gate(
            AppType::GrokBuild,
            &third_party,
            native,
            &compaction,
            &rejected
        ));
        assert!(!gate(
            AppType::Codex,
            &third_party,
            Some(CodexUpstreamFormat::ChatCompletions),
            &compaction,
            &rejected
        ));
        assert!(!gate(
            AppType::Codex,
            &third_party,
            native,
            &turn,
            &rejected
        ));
        assert!(!gate(
            AppType::Codex,
            &third_party,
            native,
            &compaction,
            &overloaded
        ));
    }

    /// 措辞不在清单里的兜底只给 Codex 的原生 Responses 第三方：官方的压缩密文只在错误
    /// 点名时才换，Grok Build 的压缩密文是 xAI 自己的。
    #[tokio::test]
    async fn generic_compaction_retry_is_limited_to_codex_third_parties() {
        let upstream = upstream().await;
        let third_party = provider(&upstream, "openai_responses");
        let mut official = third_party.clone();
        official.id = crate::database::CODEX_OFFICIAL_PROVIDER_ID.to_string();
        official.category = Some("official".to_string());
        let request = json!({ "input": [
            { "type": "compaction", "encrypted_content": "gAAAA-openai" }
        ] });
        let unsupported = ProxyError::UpstreamError {
            status: 400,
            body: Some("unsupported input item".to_string()),
        };
        let forwarder = forwarder(false);
        let gate = |app_type: AppType, provider: &Provider| {
            forwarder.opaque_state_retry_rejection(
                &app_type,
                provider,
                Some(CodexUpstreamFormat::NativeResponses),
                false,
                Some(&request),
                &unsupported,
            )
        };

        assert_eq!(
            gate(AppType::Codex, &third_party),
            Some(OpaqueStateRejection {
                reasoning: false,
                compaction: true,
                item_ids: false,
            })
        );
        assert_eq!(gate(AppType::Codex, &official), None);
        assert_eq!(gate(AppType::GrokBuild, &third_party), None);
    }

    #[test]
    fn copilot_opaque_retry_uses_actual_transport_not_legacy_static_format() {
        let forwarder = forwarder(false);
        let mut provider = provider(
            &Upstream {
                base_url: "https://api.githubcopilot.com".to_string(),
                seen: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            },
            "openai_chat",
        );
        provider.meta = Some(crate::provider::ProviderMeta {
            provider_type: Some("github_copilot".to_string()),
            api_format: Some("openai_chat".to_string()),
            ..Default::default()
        });
        let request = json!({ "input": [
            { "type": "compaction", "encrypted_content": "gAAAA-openai" }
        ] });
        let unsupported = ProxyError::UpstreamError {
            status: 400,
            body: Some("unsupported input item".to_string()),
        };

        assert_eq!(
            forwarder.opaque_state_retry_rejection(
                &AppType::Codex,
                &provider,
                Some(CodexUpstreamFormat::NativeResponses),
                false,
                Some(&request),
                &unsupported,
            ),
            Some(OpaqueStateRejection {
                reasoning: false,
                compaction: true,
                item_ids: false,
            })
        );
        assert_eq!(
            forwarder.opaque_state_retry_rejection(
                &AppType::Codex,
                &provider,
                Some(CodexUpstreamFormat::ChatCompletions),
                false,
                Some(&request),
                &unsupported,
            ),
            None
        );
    }

    /// Claude Code 转 Responses（#9424）：上一轮的推理条目藏在 thinking 签名里回放，中转
    /// 把会话分到另一个 Azure 资源、或换了一家解不开密文时，去掉这些 thinking 块对同一家
    /// 只重试一次，正文照旧。
    #[tokio::test]
    async fn claude_responses_replay_is_retried_once_without_reasoning() {
        use crate::proxy::providers::reasoning_bridge::encode_openai_reasoning_item;
        let envelope = encode_openai_reasoning_item(&json!({
            "type": "reasoning", "id": "rs_1", "summary": [],
            "encrypted_content": "gAAAA-resource-a"
        }))
        .unwrap();
        let request = json!({
            "model": "gpt-6.1-sol",
            "max_tokens": 64,
            "stream": false,
            "messages": [
                { "role": "user", "content": "hi" },
                { "role": "assistant", "content": [
                    { "type": "thinking", "thinking": "", "signature": envelope },
                    { "type": "text", "text": "a" }
                ] },
                { "role": "user", "content": "again" }
            ]
        });
        let foreign_resource = (
            400,
            json!({ "error": { "type": "invalid_request_error", "param": "", "code": null,
                "message": "OpenAI Responses bad request: The requested item was created under a different Azure OpenAI resource. Use the same resource that created the item to access it. [trace_id=x]" } }),
        );
        let claude_provider = |upstream: &Upstream| {
            let mut provider = test_provider_with_type(None);
            provider.settings_config = json!({ "env": {
                "ANTHROPIC_BASE_URL": upstream.base_url,
                "ANTHROPIC_AUTH_TOKEN": "sk-third-party",
            } });
            provider.meta = Some(crate::provider::ProviderMeta {
                api_format: Some("openai_responses".to_string()),
                ..Default::default()
            });
            provider
        };
        let send = |upstream: &Upstream| {
            let provider = claude_provider(upstream);
            let request = request.clone();
            async move {
                forwarder(false)
                    .forward_with_retry(
                        &AppType::Claude,
                        http::Method::POST,
                        "/v1/messages",
                        wire_body(request.clone()),
                        HeaderMap::new(),
                        Extensions::new(),
                        &[provider],
                    )
                    .await
            }
        };

        for rejection in [foreign_resource.clone(), blob_rejection()] {
            let upstream = scripted_upstream(vec![rejection, response_ok()]).await;
            send(&upstream)
                .await
                .map_err(|error| error.error)
                .expect("retried");
            let seen = upstream.seen.lock().await;
            assert_eq!(seen.len(), 2);
            assert!(carries_reasoning(&seen[0]));
            assert_eq!(seen[0].body["input"][1]["id"], "rs_1");
            assert!(!carries_reasoning(&seen[1]));
            let retried = seen[1].body["input"].as_array().unwrap();
            assert_eq!(retried.len(), 3);
            assert_eq!(retried[1]["role"], "assistant");
        }

        // 先回 200、在 Response 对象里说验不了密文：同样整流重试一次。
        let failed_ok = (
            200,
            json!({ "id": "r0", "object": "response", "status": "failed",
                "error": { "code": "invalid_encrypted_content", "message": "The encrypted content gAAAA could not be verified." } }),
        );
        let upstream = scripted_upstream(vec![failed_ok, response_ok()]).await;
        send(&upstream)
            .await
            .map_err(|error| error.error)
            .expect("retried after a 2xx failure");
        let seen = upstream.seen.lock().await;
        assert_eq!(seen.len(), 2);
        assert!(carries_reasoning(&seen[0]));
        assert!(!carries_reasoning(&seen[1]));
        drop(seen);

        // 重试仍被拒：不再重试，把第二次的错误交回客户端。
        let upstream = scripted_upstream(vec![foreign_resource.clone(), foreign_resource]).await;
        assert!(send(&upstream).await.is_err());
        assert_eq!(upstream.seen.lock().await.len(), 2);

        // 措辞同时命中签名整流的「invalid request」兜底：也只重试这一次，不发第三次。
        let both = (
            400,
            json!({ "error": { "type": "invalid_request_error", "code": "invalid_encrypted_content",
                "message": "Invalid request: invalid encrypted content" } }),
        );
        let upstream = scripted_upstream(vec![both.clone(), both]).await;
        assert!(send(&upstream).await.is_err());
        assert_eq!(upstream.seen.lock().await.len(), 2);
    }

    /// 其他 400、转换成 Chat 的上游都不触发。
    #[tokio::test]
    async fn blob_retry_only_follows_self_identified_rejections_on_native_upstreams() {
        let unrelated = (
            400,
            json!({ "error": { "type": "invalid_request_error", "message": "Invalid value for 'model'." } }),
        );
        let upstream = scripted_upstream(vec![unrelated]).await;
        assert!(
            forward_codex(&forwarder(true), provider(&upstream, "openai_responses"))
                .await
                .is_err()
        );
        assert_eq!(upstream.seen.lock().await.len(), 1);

        let upstream = scripted_upstream(vec![blob_rejection()]).await;
        assert!(
            forward_codex(&forwarder(true), provider(&upstream, "openai_chat"))
                .await
                .is_err()
        );
        assert_eq!(upstream.seen.lock().await.len(), 1);
    }

    /// 按脚本依次回应的假上游，每一步自己定 content-type 和原始响应体（流式用）。
    async fn raw_upstream(script: Vec<(u16, &'static str, String)>) -> Upstream {
        let script = Arc::new(Mutex::new(std::collections::VecDeque::from(script)));
        let seen = Arc::new(Mutex::new(Vec::new()));
        let app = {
            let seen = seen.clone();
            axum::Router::new().fallback(move |headers: HeaderMap, body: Bytes| {
                let script = script.clone();
                let seen = seen.clone();
                async move {
                    seen.lock().await.push(Seen {
                        headers,
                        body: serde_json::from_slice(&body).unwrap_or(Value::Null),
                    });
                    let (status, content_type, body) = script
                        .lock()
                        .await
                        .pop_front()
                        .expect("upstream script exhausted");
                    (
                        StatusCode::from_u16(status).expect("status"),
                        [(http::header::CONTENT_TYPE, content_type)],
                        body,
                    )
                }
            })
        };
        Upstream {
            base_url: serve_upstream(app).await,
            seen,
        }
    }

    const ENCRYPTED_TASK_REJECTION: &str =
        "Encrypted function output content could not be decrypted or decoded.";

    fn sse(events: &[Value]) -> (u16, &'static str, String) {
        let body = events
            .iter()
            .map(|event| {
                format!(
                    "event: {}\ndata: {event}\n\n",
                    event["type"].as_str().unwrap()
                )
            })
            .collect::<String>();
        (200, "text/event-stream", body)
    }

    fn failed_stream(message: &str) -> (u16, &'static str, String) {
        sse(&[
            json!({ "type": "response.created", "response": { "id": "r1", "status": "in_progress" } }),
            json!({ "type": "response.failed", "response": { "id": "r1", "status": "failed",
                     "error": { "code": "server_error", "message": message } } }),
        ])
    }

    fn completed_stream() -> (u16, &'static str, String) {
        sse(&[
            json!({ "type": "response.created", "response": { "id": "r1", "status": "in_progress" } }),
            json!({ "type": "response.output_item.added", "output_index": 0,
                     "item": { "type": "message", "id": "msg_1", "role": "assistant", "content": [] } }),
            json!({ "type": "response.completed", "response": { "id": "r1", "status": "completed", "output": [] } }),
        ])
    }

    /// 子 agent 的第一个请求：末尾是主 agent 那家签发的任务密文。`history` 放在它前面。
    fn sub_agent_request(history: Vec<Value>) -> Value {
        let mut input = history;
        input.push(json!({
            "type": "agent_message",
            "author": "/root",
            "recipient": "/root/worker",
            "content": [
                { "type": "input_text", "text": "Message Type: NEW_TASK\nTask name: /root/worker\nSender: /root\nPayload:\n" },
                { "type": "encrypted_content", "encrypted_content": "gAAAAB-task-from-main" }
            ]
        }));
        json!({ "model": "listed", "input": input, "stream": true })
    }

    async fn forward_sub_agent(
        upstream: &Upstream,
        request: Value,
    ) -> Result<ForwardResult, ForwardError> {
        forwarder(true)
            .forward_with_retry(
                &AppType::Codex,
                http::Method::POST,
                "/responses",
                wire_body(request.clone()),
                chatgpt_headers(),
                Extensions::new(),
                &[provider(upstream, "openai_responses")],
            )
            .await
    }

    async fn body_text(result: ForwardResult) -> String {
        let chunks: Vec<_> = result.response.bytes_stream().collect().await;
        let bytes: Vec<u8> = chunks
            .into_iter()
            .flat_map(|chunk| chunk.expect("chunk").to_vec())
            .collect();
        String::from_utf8(bytes).expect("utf8")
    }

    fn is_unreadable_task_error(error: &ProxyError) -> bool {
        let ProxyError::UpstreamError {
            status: 400,
            body: Some(body),
        } = error
        else {
            return false;
        };
        serde_json::from_str::<Value>(body)
            .is_ok_and(|body| body["error"]["code"] == "unreadable_encrypted_agent_task")
    }

    fn task_ciphertext(seen: &Seen) -> &Value {
        &seen.body["input"].as_array().unwrap().last().unwrap()["content"][1]["encrypted_content"]
    }

    /// 子 agent 换了一家：上游在流里、输出之前说解不开。不把空任务交给子 agent，也不让
    /// Codex 对着同一个错重试，直接说明原因。
    #[tokio::test]
    async fn sub_agent_task_rejected_in_stream_returns_an_explanation() {
        let upstream = raw_upstream(vec![failed_stream(ENCRYPTED_TASK_REJECTION)]).await;
        let error = forward_sub_agent(&upstream, sub_agent_request(Vec::new()))
            .await
            .err()
            .expect("rejected");
        assert!(is_unreadable_task_error(&error.error), "{:?}", error.error);
        // 请求里没有别家的历史状态可去：只发一次。
        assert_eq!(upstream.seen.lock().await.len(), 1);
    }

    /// 历史里也有别家的状态时，先去掉历史里的、任务原样保留重试一次；还被拒才说明原因。
    #[tokio::test]
    async fn sub_agent_retry_strips_history_but_keeps_the_task() {
        let history = vec![
            json!({ "type": "reasoning", "id": "rs_1", "summary": [], "encrypted_content": "gAAAA-other-org" }),
            json!({ "type": "agent_message", "author": "/root", "recipient": "/root/worker",
                     "content": [{ "type": "encrypted_content", "encrypted_content": "gAAAA-older-task" }] }),
        ];
        let upstream = raw_upstream(vec![
            failed_stream(ENCRYPTED_TASK_REJECTION),
            failed_stream(ENCRYPTED_TASK_REJECTION),
        ])
        .await;
        let error = forward_sub_agent(&upstream, sub_agent_request(history.clone()))
            .await
            .err()
            .expect("rejected");
        assert!(is_unreadable_task_error(&error.error), "{:?}", error.error);
        let seen = upstream.seen.lock().await;
        assert_eq!(seen.len(), 2);
        assert!(carries_reasoning(&seen[0]));
        assert!(!carries_reasoning(&seen[1]));
        assert_eq!(seen[1].body["input"][0]["content"][0]["type"], "input_text");
        assert_eq!(task_ciphertext(&seen[1]), "gAAAAB-task-from-main");
        drop(seen);

        // 历史里的状态才是问题：重试成功，任务照样原样到达上游。
        let upstream = raw_upstream(vec![
            failed_stream(ENCRYPTED_TASK_REJECTION),
            completed_stream(),
        ])
        .await;
        assert!(forward_sub_agent(&upstream, sub_agent_request(history))
            .await
            .is_ok());
        let seen = upstream.seen.lock().await;
        assert_eq!(seen.len(), 2);
        assert_eq!(task_ciphertext(&seen[1]), "gAAAAB-task-from-main");
    }

    /// 普通会话换了一家续接（#9476）：历史里有别家签发的推理和函数输出密文，上游先回 200、
    /// 再在输出前说解不开。压住流改成上游错误，整流后对同一家重试一次，不把失败流交给
    /// Codex（它会拿同一段历史重连到底）。
    #[tokio::test]
    async fn resumed_thread_rejected_in_stream_is_rectified_and_retried() {
        let request = json!({
            "model": "listed",
            "stream": true,
            "input": [
                { "type": "message", "role": "user", "content": [{ "type": "input_text", "text": "hi" }] },
                { "type": "reasoning", "id": "rs_1", "summary": [], "encrypted_content": "gAAAA-provider-a" },
                { "type": "function_call", "call_id": "call_1", "name": "shell", "arguments": "{}" },
                { "type": "function_call_output", "call_id": "call_1", "output": [
                    { "type": "input_text", "text": "ok" },
                    { "type": "encrypted_content", "encrypted_content": "enc-provider-a" }
                ] },
                { "type": "message", "role": "user", "content": [{ "type": "input_text", "text": "again" }] }
            ]
        });
        // 中转转述时加了自己的前缀，照样认。
        let relayed = format!("OpenAI Responses bad request: {ENCRYPTED_TASK_REJECTION}");
        let upstream = raw_upstream(vec![failed_stream(&relayed), completed_stream()]).await;
        let result = forward_sub_agent(&upstream, request)
            .await
            .map_err(|error| error.error)
            .expect("retried");
        assert!(body_text(result).await.contains("response.completed"));

        let seen = upstream.seen.lock().await;
        assert_eq!(seen.len(), 2);
        assert!(carries_reasoning(&seen[0]));
        assert!(!carries_reasoning(&seen[1]));
        let output = &seen[1].body["input"][2]["output"];
        assert_eq!(output[0]["text"], "ok");
        assert_eq!(output[1]["type"], "input_text");
    }

    /// 请求里没有别家签发的状态（新会话）：不压流，失败流原样交给客户端，只发一次。
    #[tokio::test]
    async fn fresh_thread_streams_are_not_held() {
        let request = json!({
            "model": "listed",
            "stream": true,
            "input": [{ "type": "message", "role": "user", "content": [{ "type": "input_text", "text": "hi" }] }]
        });
        let step = failed_stream(ENCRYPTED_TASK_REJECTION);
        let expected = step.2.clone();
        let upstream = raw_upstream(vec![step]).await;
        let result = forward_sub_agent(&upstream, request)
            .await
            .map_err(|error| error.error)
            .expect("committed");
        assert_eq!(body_text(result).await, expected);
        assert_eq!(upstream.seen.lock().await.len(), 1);
    }

    /// HTTP 报错同样处理。
    #[tokio::test]
    async fn sub_agent_task_rejected_over_http_returns_an_explanation() {
        let rejection = json!({ "error": { "message": ENCRYPTED_TASK_REJECTION } }).to_string();
        let upstream = raw_upstream(vec![(502, "application/json", rejection)]).await;
        let error = forward_sub_agent(&upstream, sub_agent_request(Vec::new()))
            .await
            .err()
            .expect("rejected");
        assert!(is_unreadable_task_error(&error.error), "{:?}", error.error);
        assert_eq!(upstream.seen.lock().await.len(), 1);
    }

    /// 缓冲阶段只有一个截止时间：上游在输出前一直发心跳，也不能无限期压着流不交给客户端。
    #[tokio::test]
    async fn sub_agent_hold_has_one_deadline_despite_heartbeats() {
        const HEARTBEATS: u32 = 40;
        let created = "event: response.created\ndata: {\"type\":\"response.created\",\"response\":{\"id\":\"r1\"}}\n\n";
        let completed = "event: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"id\":\"r1\",\"status\":\"completed\",\"output\":[]}}\n\n";
        let app = axum::Router::new().fallback(move || async move {
            let stream = futures::stream::unfold(0u32, move |n| async move {
                if n > HEARTBEATS {
                    return None;
                }
                if n > 0 {
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
                let chunk = match n {
                    0 => created,
                    HEARTBEATS => completed,
                    _ => ": keepalive\n\n",
                };
                Some((
                    Ok::<_, std::io::Error>(Bytes::from_static(chunk.as_bytes())),
                    n + 1,
                ))
            });
            (
                [(http::header::CONTENT_TYPE, "text/event-stream")],
                axum::body::Body::from_stream(stream),
            )
        });
        let upstream = Upstream {
            base_url: serve_upstream(app).await,
            seen: Arc::new(Mutex::new(Vec::new())),
        };
        let _ = rustls::crypto::ring::default_provider().install_default();
        let forwarder =
            test_forwarder(Duration::from_secs(5), Duration::from_millis(100)).stack_request(true);

        let started = std::time::Instant::now();
        let result = forwarder
            .forward_with_retry(
                &AppType::Codex,
                http::Method::POST,
                "/responses",
                wire_body(sub_agent_request(Vec::new())),
                chatgpt_headers(),
                Extensions::new(),
                &[provider(&upstream, "openai_responses")],
            )
            .await
            .map_err(|error| error.error)
            .expect("committed");
        let handed_over = started.elapsed();
        // 心跳要发 800ms；截止时间 100ms，交出流不该等到心跳发完。
        assert!(
            handed_over < Duration::from_millis(500),
            "held for {handed_over:?}"
        );
        let text = body_text(result).await;
        assert!(text.starts_with(created), "{text}");
        assert!(text.ends_with(completed), "{text}");
        assert_eq!(
            text.matches(": keepalive").count(),
            (HEARTBEATS - 1) as usize
        );
    }

    /// 上游解得开（同一家）或者是别的失败：客户端收到的流和上游发的逐字节一样。
    #[tokio::test]
    async fn sub_agent_streams_pass_through_unchanged_otherwise() {
        for step in [completed_stream(), failed_stream("backend exploded")] {
            let expected = step.2.clone();
            let upstream = raw_upstream(vec![step]).await;
            let result = forward_sub_agent(&upstream, sub_agent_request(Vec::new()))
                .await
                .map_err(|error| error.error)
                .expect("committed");
            assert_eq!(body_text(result).await, expected);
            assert_eq!(upstream.seen.lock().await.len(), 1);
        }
    }
}

/// 一次请求结束后留下的记账：代理统计、「正在使用」、熔断器。每条出口路径各断言一次，
/// 锁住成功、失败、不计入熔断三种收尾对路由状态的影响。
mod bookkeeping {
    use super::*;
    use std::collections::VecDeque;
    use tokio::sync::Mutex;

    struct Upstream {
        base_url: String,
        requests: Arc<Mutex<Vec<Value>>>,
    }

    /// 按脚本依次返回响应的假上游，记下收到的请求体。
    async fn upstream(script: Vec<(u16, Value)>) -> Upstream {
        let script = Arc::new(Mutex::new(VecDeque::from(script)));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let app = {
            let requests = requests.clone();
            axum::Router::new().fallback(move |body: Bytes| {
                let script = script.clone();
                let requests = requests.clone();
                async move {
                    requests
                        .lock()
                        .await
                        .push(serde_json::from_slice(&body).unwrap_or(Value::Null));
                    let (status, body) = script
                        .lock()
                        .await
                        .pop_front()
                        .expect("upstream script exhausted");
                    (
                        StatusCode::from_u16(status).expect("status"),
                        [(http::header::CONTENT_TYPE, "application/json")],
                        body.to_string(),
                    )
                }
            })
        };
        Upstream {
            base_url: serve_upstream(app).await,
            requests,
        }
    }

    fn provider(id: &str, upstream: &Upstream) -> Provider {
        let mut provider = test_provider_with_type(None);
        provider.id = id.to_string();
        provider.name = format!("Provider {id}");
        provider.settings_config = json!({
            "env": {
                "ANTHROPIC_BASE_URL": upstream.base_url,
                "ANTHROPIC_AUTH_TOKEN": "sk-test",
            }
        });
        provider
    }

    fn forwarder(max_attempts: usize, provider_at_start: &str) -> RequestForwarder {
        // 真正发请求要用 HTTP 客户端；应用启动时装的 rustls 后端测试里没有（同 lib.rs）。
        let _ = rustls::crypto::ring::default_provider().install_default();
        let mut forwarder = test_forwarder(Duration::from_secs(5), Duration::from_secs(5));
        forwarder.max_attempts = max_attempts;
        forwarder.current_provider_id_at_start = provider_at_start.to_string();
        forwarder
    }

    async fn send(
        forwarder: &RequestForwarder,
        providers: Vec<Provider>,
        body: Value,
    ) -> Result<ForwardResult, ForwardError> {
        send_as(&AppType::Claude, forwarder, providers, body).await
    }

    async fn send_as(
        app_type: &AppType,
        forwarder: &RequestForwarder,
        providers: Vec<Provider>,
        body: Value,
    ) -> Result<ForwardResult, ForwardError> {
        let mut headers = HeaderMap::new();
        headers.insert(
            http::header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        );
        forwarder
            .forward_with_retry(
                app_type,
                http::Method::POST,
                "/v1/messages",
                wire_body(body.clone()),
                headers,
                Extensions::new(),
                &providers,
            )
            .await
    }

    fn ok() -> (u16, Value) {
        (
            200,
            json!({
                "id": "msg_1",
                "type": "message",
                "role": "assistant",
                "model": "claude-sonnet-4-5",
                "content": [{ "type": "text", "text": "ok" }],
                "stop_reason": "end_turn",
                "usage": { "input_tokens": 1, "output_tokens": 1 }
            }),
        )
    }

    fn error(status: u16, message: &str) -> (u16, Value) {
        (
            status,
            json!({
                "type": "error",
                "error": { "type": "invalid_request_error", "message": message }
            }),
        )
    }

    const SIGNATURE_ERROR: &str = "messages.1.content.0: Invalid `signature` in `thinking` block";
    const BUDGET_ERROR: &str =
        "thinking.budget_tokens: Input should be greater than or equal to 1024";

    fn plain_body() -> Value {
        json!({
            "model": "claude-sonnet-4-5",
            "max_tokens": 1000,
            "messages": [{ "role": "user", "content": "hi" }]
        })
    }

    fn body_with_thinking_signature() -> Value {
        json!({
            "model": "claude-sonnet-4-5",
            "max_tokens": 1000,
            "messages": [
                { "role": "user", "content": "hi" },
                { "role": "assistant", "content": [
                    { "type": "thinking", "thinking": "t", "signature": "sig" },
                    { "type": "text", "text": "a" }
                ]},
                { "role": "user", "content": "again" }
            ]
        })
    }

    fn body_with_thinking(thinking: Value) -> Value {
        let mut body = plain_body();
        body["thinking"] = thinking;
        body
    }

    #[derive(Debug, PartialEq)]
    struct Books {
        total: u64,
        success: u64,
        failed: u64,
        success_rate: f32,
        failover_count: u64,
        last_error: Option<String>,
        /// 最后一次尝试的是哪家（`ProxyStatus.current_provider_id`）。
        attempting: Option<String>,
        /// 「正在使用」（`current_providers`），只在成功时写。
        in_use: Option<String>,
    }

    async fn books(forwarder: &RequestForwarder) -> Books {
        let status = forwarder.status.read().await;
        Books {
            total: status.total_requests,
            success: status.success_requests,
            failed: status.failed_requests,
            success_rate: status.success_rate,
            failover_count: status.failover_count,
            last_error: status.last_error.clone(),
            attempting: status.current_provider_id.clone(),
            in_use: forwarder
                .current_providers
                .read()
                .await
                .get("claude")
                .map(|(id, _)| id.clone()),
        }
    }

    /// 熔断器记下的 (总数, 失败数)；从没建过熔断器时为 `None`。成功是异步记的，等它落下。
    async fn breaker(forwarder: &RequestForwarder, provider_id: &str) -> Option<(u32, u32)> {
        for _ in 0..200 {
            if let Some(stats) = forwarder
                .router
                .get_circuit_breaker_stats(provider_id, &AppType::Claude)
                .await
            {
                if stats.total_requests > 0 {
                    return Some((stats.total_requests, stats.failed_requests));
                }
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        forwarder
            .router
            .get_circuit_breaker_stats(provider_id, &AppType::Claude)
            .await
            .map(|stats| (stats.total_requests, stats.failed_requests))
    }

    fn expect_err(result: Result<ForwardResult, ForwardError>) -> ForwardError {
        match result {
            Ok(_) => panic!("expected the request to fail"),
            Err(err) => err,
        }
    }

    #[tokio::test]
    async fn success_on_the_first_provider() {
        let up = upstream(vec![ok()]).await;
        let fwd = forwarder(1, "p1");

        let result = send(&fwd, vec![provider("p1", &up)], plain_body()).await;

        assert_eq!(result.ok().map(|r| r.provider.id), Some("p1".into()));
        assert_eq!(
            books(&fwd).await,
            Books {
                total: 1,
                success: 1,
                failed: 0,
                success_rate: 100.0,
                failover_count: 0,
                last_error: None,
                attempting: Some("p1".into()),
                in_use: Some("p1".into()),
            }
        );
        assert_eq!(breaker(&fwd, "p1").await, Some((1, 0)));
    }

    #[tokio::test]
    async fn ordinary_errors_replay_only_when_trying_another_provider() {
        for app_type in [AppType::Claude, AppType::Codex] {
            for status in [401, 429, 500, 502, 503] {
                for failover in [false, true] {
                    let failed = upstream(vec![error(status, "ordinary upstream failure")]).await;
                    let healthy = upstream(vec![ok()]).await;
                    let fwd = forwarder(2, "p1");
                    let mut providers = vec![provider("p1", &failed)];
                    if failover {
                        providers.push(provider("p2", &healthy));
                    }
                    let endpoint = if matches!(app_type, AppType::Codex) {
                        for (provider, up) in providers.iter_mut().zip([&failed, &healthy]) {
                            provider.settings_config = json!({
                                "auth": {"OPENAI_API_KEY": "sk-test"},
                                "config": format!("model_provider = \"custom\"\n[model_providers.custom]\nbase_url = \"{}\"\nwire_api = \"responses\"\n", up.base_url)
                            });
                        }
                        "/responses"
                    } else {
                        "/v1/messages"
                    };
                    let mut request = plain_body();
                    request["messages"][0]["content"] = json!("x".repeat(1024 * 1024));
                    let body = wire_body(request);
                    let replay_count = body.replay_count.clone();
                    let result = fwd
                        .forward_with_retry(
                            &app_type,
                            http::Method::POST,
                            endpoint,
                            body,
                            HeaderMap::new(),
                            Extensions::new(),
                            &providers,
                        )
                        .await;

                    assert_eq!(result.is_ok(), failover, "{app_type:?}: {status}");
                    assert_eq!(
                        replay_count.load(std::sync::atomic::Ordering::Relaxed),
                        usize::from(failover),
                        "{app_type:?}: {status}, failover={failover}"
                    );
                    assert_eq!(failed.requests.lock().await.len(), 1);
                    assert_eq!(healthy.requests.lock().await.len(), usize::from(failover));
                }
            }
        }
    }

    #[tokio::test]
    async fn overlapping_rectifier_checks_share_one_replay() {
        // Signature matching sees "invalid request" but cannot modify this body;
        // budget rectification then uses the same prepared replay and succeeds.
        let up = upstream(vec![
            error(400, &format!("Invalid request: {BUDGET_ERROR}")),
            ok(),
        ])
        .await;
        let fwd = forwarder(1, "p1");
        let body = wire_body(plain_body());
        let replay_count = body.replay_count.clone();
        let result = fwd
            .forward_with_retry(
                &AppType::Claude,
                http::Method::POST,
                "/v1/messages",
                body,
                HeaderMap::new(),
                Extensions::new(),
                &[provider("p1", &up)],
            )
            .await;
        assert!(result.is_ok());
        assert_eq!(replay_count.load(std::sync::atomic::Ordering::Relaxed), 1);
        let requests = up.requests.lock().await;
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[1]["thinking"]["budget_tokens"], 32000);
    }

    #[tokio::test]
    async fn timeout_replays_only_for_failover() {
        let slow_url = serve_upstream(axum::Router::new().fallback(|| async {
            tokio::time::sleep(Duration::from_secs(5)).await;
            StatusCode::OK
        }))
        .await;
        let slow = Upstream {
            base_url: slow_url,
            requests: Arc::new(Mutex::new(Vec::new())),
        };
        for failover in [false, true] {
            let healthy = upstream(vec![ok()]).await;
            let mut fwd = forwarder(2, "p1");
            fwd.non_streaming_timeout = Duration::from_millis(250);
            let mut providers = vec![provider("p1", &slow)];
            if failover {
                providers.push(provider("p2", &healthy));
            }
            let body = wire_body(plain_body());
            let replay_count = body.replay_count.clone();
            let result = fwd
                .forward_with_retry(
                    &AppType::Claude,
                    http::Method::POST,
                    "/v1/messages",
                    body,
                    HeaderMap::new(),
                    Extensions::new(),
                    &providers,
                )
                .await;
            if failover {
                assert!(result.is_ok());
            } else {
                assert!(matches!(expect_err(result).error, ProxyError::Timeout(_)));
            }
            assert_eq!(
                replay_count.load(std::sync::atomic::Ordering::Relaxed),
                usize::from(failover)
            );
        }
    }

    #[tokio::test]
    async fn failover_to_the_second_provider_counts_a_switch() {
        let up1 = upstream(vec![error(500, "boom")]).await;
        let up2 = upstream(vec![ok()]).await;
        let fwd = forwarder(2, "p1");

        let result = send(
            &fwd,
            vec![provider("p1", &up1), provider("p2", &up2)],
            plain_body(),
        )
        .await;

        assert_eq!(result.ok().map(|r| r.provider.id), Some("p2".into()));
        assert_eq!(
            books(&fwd).await,
            Books {
                total: 1,
                success: 1,
                failed: 0,
                success_rate: 100.0,
                failover_count: 1,
                last_error: None,
                attempting: Some("p2".into()),
                in_use: Some("p2".into()),
            }
        );
        assert_eq!(breaker(&fwd, "p1").await, Some((1, 1)));
        assert_eq!(breaker(&fwd, "p2").await, Some((1, 0)));
    }

    /// 原生 Anthropic 供应商走 hyper 发送；第一家 500 后换第二家重发，两次发送都经过过滤，
    /// 第二家的本地 body 覆写注入的内容也过滤。工具结果跟随子开关（关闭）原样保留。
    #[tokio::test]
    async fn privacy_filter_runs_on_every_attempt_over_hyper() {
        let up1 = upstream(vec![error(500, "boom")]).await;
        let up2 = upstream(vec![ok()]).await;
        let first_provider = provider("p1", &up1);
        for format in [None, Some("anthropic")] {
            assert!(should_preserve_exact_header_case(
                "Claude",
                &first_provider,
                format,
                false
            ));
        }
        let mut second_provider = provider("p2", &up2);
        second_provider.meta = Some(crate::provider::ProviderMeta {
            local_proxy_request_overrides: Some(crate::provider::LocalProxyRequestOverrides {
                headers: Default::default(),
                body: Some(json!({ "system": "我的密码是 Hunter2xyz" })),
            }),
            ..Default::default()
        });
        let fwd = forwarder(2, "p1").privacy_filter(PrivacyFilterConfig {
            enabled: true,
            filter_tool_results: false,
        });
        let body = json!({
            "model": "claude-sonnet-4-5",
            "max_tokens": 1000,
            "messages": [
                { "role": "user", "content": "邮箱 alice@example.com" },
                { "role": "assistant", "content": [
                    { "type": "tool_use", "id": "t1", "name": "read", "input": {} }
                ]},
                { "role": "user", "content": [
                    { "type": "tool_result", "tool_use_id": "t1", "content": "password: Hunter2xyz" },
                    { "type": "text", "text": "password = Qwer1234" }
                ]}
            ]
        });

        let result = send(&fwd, vec![first_provider, second_provider], body).await;
        assert_eq!(result.ok().map(|r| r.provider.id), Some("p2".into()));

        assert_eq!(up1.requests.lock().await.len(), 1);
        assert_eq!(up2.requests.lock().await.len(), 1);
        let first = up1.requests.lock().await.pop().expect("first attempt");
        let second = up2.requests.lock().await.pop().expect("second attempt");
        for (attempt, sent) in [("p1", &first), ("p2", &second)] {
            let text = sent.to_string();
            assert!(!text.contains("alice@example.com"), "{attempt}: {text}");
            assert!(!text.contains("Qwer1234"), "{attempt}: {text}");
            assert!(
                sent["messages"][0].to_string().contains("邮箱 ["),
                "{attempt}: {text}"
            );
            assert_eq!(
                sent["messages"].as_array().map(Vec::len),
                Some(3),
                "{attempt}"
            );
            assert_eq!(sent["messages"][1]["content"][0]["id"], "t1", "{attempt}");
            assert_eq!(
                sent["messages"][2]["content"][0]["tool_use_id"], "t1",
                "{attempt}"
            );
            let user_text = &sent["messages"][2]["content"][1];
            assert_eq!(user_text["type"], "text", "{attempt}: {text}");
            assert!(
                user_text["text"]
                    .as_str()
                    .is_some_and(|t| t.starts_with("password = [")),
                "{attempt}: {text}"
            );
            assert!(
                sent["messages"][2]["content"][0]
                    .to_string()
                    .contains("password: Hunter2xyz"),
                "{attempt}: tool result should be kept: {text}"
            );
        }
        assert!(first.get("system").is_none(), "{first}");
        let system = second["system"].to_string();
        assert!(system.contains("我的密码是 ["), "{system}");
        assert!(!system.contains("Hunter2xyz"), "{system}");
    }

    #[tokio::test]
    async fn non_retryable_error_is_neutral_for_the_breaker() {
        let up = upstream(vec![error(400, "bad request")]).await;
        let fwd = forwarder(1, "p1");

        let err = expect_err(send(&fwd, vec![provider("p1", &up)], plain_body()).await);

        assert_eq!(err.provider.map(|p| p.id), Some("p1".into()));
        assert_eq!(
            books(&fwd).await,
            Books {
                total: 1,
                success: 0,
                failed: 1,
                success_rate: 0.0,
                failover_count: 0,
                last_error: Some(err.error.to_string()),
                attempting: Some("p1".into()),
                in_use: None,
            }
        );
        assert_eq!(breaker(&fwd, "p1").await, None);
    }

    /// 失败时带出映射后的上游模型名，错误日志才不会把客户端别名（claude-haiku-*）
    /// 记成「调了没配置的模型」。429 走中性失败、500 走全部失败两条出口。
    #[tokio::test]
    async fn failed_request_reports_the_mapped_upstream_model() {
        for status in [429, 500] {
            let up = upstream(vec![error(status, "upstream failed")]).await;
            let fwd = forwarder(1, "p1");
            let mut mapped = provider("p1", &up);
            mapped.settings_config["env"]["ANTHROPIC_DEFAULT_HAIKU_MODEL"] = json!("grok-4.7");
            let mut body = plain_body();
            body["model"] = json!("claude-haiku-4-5-20251001");

            let err = expect_err(send(&fwd, vec![mapped], body).await);

            assert_eq!(
                err.outbound_model.as_deref(),
                Some("grok-4.7"),
                "status {status}"
            );
        }
    }

    #[tokio::test]
    async fn every_provider_failing_records_each_failure() {
        let up1 = upstream(vec![error(500, "boom")]).await;
        let up2 = upstream(vec![error(503, "busy")]).await;
        let fwd = forwarder(2, "p1");

        let err = expect_err(
            send(
                &fwd,
                vec![provider("p1", &up1), provider("p2", &up2)],
                plain_body(),
            )
            .await,
        );

        assert_eq!(err.provider.map(|p| p.id), Some("p2".into()));
        assert_eq!(
            books(&fwd).await,
            Books {
                total: 1,
                success: 0,
                failed: 1,
                success_rate: 0.0,
                failover_count: 0,
                last_error: Some("所有供应商都失败".into()),
                attempting: Some("p2".into()),
                in_use: None,
            }
        );
        assert_eq!(breaker(&fwd, "p1").await, Some((1, 1)));
        assert_eq!(breaker(&fwd, "p2").await, Some((1, 1)));
    }

    #[tokio::test]
    async fn repeated_classifier_errors_stop_main_requests_before_any_upstream_call() {
        let up = upstream(vec![ok()]).await;
        let fwd = forwarder(2, "p1");
        let mut body = plain_body();
        body["tools"] = json!([{"name":"Bash","input_schema":{"type":"object"}}]);
        for n in 0..3 {
            body["messages"].as_array_mut().unwrap().extend([
                json!({"role":"assistant","content":[{"type":"tool_use","id":format!("tool_{n}"),"name":"Bash","input":{}}]}),
                json!({"role":"user","content":[{"type":"tool_result","tool_use_id":format!("tool_{n}"),"is_error":true,"content":"claude-opus-5[1m] is temporarily unavailable, so auto mode cannot determine the safety of Bash right now."}]}),
            ]);
        }
        for _ in 0..10 {
            let err = expect_err(send(&fwd, vec![provider("p1", &up)], body.clone()).await);
            assert!(matches!(err.error, ProxyError::InvalidRequest(_)));
        }
        assert_eq!(up.requests.lock().await.len(), 0);
        for _ in 0..100 {
            if fwd.status.read().await.active_connections == 0 {
                break;
            }
            tokio::task::yield_now().await;
        }
        assert_eq!(fwd.status.read().await.active_connections, 0);
        body["messages"]
            .as_array_mut()
            .unwrap()
            .push(json!({"role":"user","content":"I fixed the mapping; continue."}));
        send(&fwd, vec![provider("p1", &up)], body)
            .await
            .map_err(|e| e.error)
            .expect("new user turn resumes");
        assert_eq!(up.requests.lock().await.len(), 1);
    }

    #[tokio::test]
    async fn open_breakers_reject_every_provider_without_sending() {
        let up1 = upstream(vec![]).await;
        let up2 = upstream(vec![]).await;
        let fwd = forwarder(2, "p1");
        for id in ["p1", "p2"] {
            for _ in 0..20 {
                let _ = fwd
                    .router
                    .record_result(id, &AppType::Claude, false, false, Some("seed".into()))
                    .await;
            }
        }

        let err = expect_err(
            send(
                &fwd,
                vec![provider("p1", &up1), provider("p2", &up2)],
                plain_body(),
            )
            .await,
        );

        assert!(matches!(err.error, ProxyError::NoAvailableProvider));
        assert_eq!(
            books(&fwd).await,
            Books {
                total: 1,
                success: 0,
                failed: 1,
                success_rate: 0.0,
                failover_count: 0,
                last_error: Some("所有供应商暂时不可用（熔断器限制）".into()),
                attempting: None,
                in_use: None,
            }
        );
        assert!(up1.requests.lock().await.is_empty());
        assert!(up2.requests.lock().await.is_empty());
    }

    #[tokio::test]
    async fn signature_rectifier_retry_success() {
        let up = upstream(vec![error(400, SIGNATURE_ERROR), ok()]).await;
        let fwd = forwarder(1, "p1");

        let result = send(
            &fwd,
            vec![provider("p1", &up)],
            body_with_thinking_signature(),
        )
        .await;

        assert!(result.is_ok());
        assert_eq!(
            books(&fwd).await,
            Books {
                total: 1,
                success: 1,
                failed: 0,
                success_rate: 100.0,
                failover_count: 0,
                last_error: None,
                attempting: Some("p1".into()),
                in_use: Some("p1".into()),
            }
        );
        assert_eq!(breaker(&fwd, "p1").await, Some((1, 0)));
        let requests = up.requests.lock().await;
        assert_eq!(requests.len(), 2);
        assert!(!requests[1].to_string().contains("\"signature\""));
    }

    #[tokio::test]
    async fn signature_rectifier_retry_client_error_is_neutral() {
        let up = upstream(vec![error(400, SIGNATURE_ERROR), error(400, "still bad")]).await;
        let fwd = forwarder(1, "p1");

        let err = expect_err(
            send(
                &fwd,
                vec![provider("p1", &up)],
                body_with_thinking_signature(),
            )
            .await,
        );

        assert_eq!(
            books(&fwd).await,
            Books {
                total: 1,
                success: 0,
                failed: 1,
                success_rate: 0.0,
                failover_count: 0,
                last_error: Some(err.error.to_string()),
                attempting: Some("p1".into()),
                in_use: None,
            }
        );
        assert_eq!(breaker(&fwd, "p1").await, None);
        assert_eq!(up.requests.lock().await.len(), 2);
    }

    #[tokio::test]
    async fn signature_rectifier_retry_provider_error_counts_against_the_breaker() {
        let up = upstream(vec![error(400, SIGNATURE_ERROR), error(502, "gateway")]).await;
        let fwd = forwarder(1, "p1");

        let err = expect_err(
            send(
                &fwd,
                vec![provider("p1", &up)],
                body_with_thinking_signature(),
            )
            .await,
        );

        assert_eq!(err.provider.map(|p| p.id), Some("p1".into()));
        assert_eq!(
            books(&fwd).await,
            Books {
                total: 1,
                success: 0,
                failed: 1,
                success_rate: 0.0,
                failover_count: 0,
                last_error: Some("所有供应商都失败".into()),
                attempting: Some("p1".into()),
                in_use: None,
            }
        );
        assert_eq!(breaker(&fwd, "p1").await, Some((1, 1)));
        assert_eq!(up.requests.lock().await.len(), 2);
    }

    #[tokio::test]
    async fn signature_error_without_anything_to_rectify_is_neutral() {
        let up = upstream(vec![error(400, SIGNATURE_ERROR)]).await;
        let fwd = forwarder(1, "p1");

        let err = expect_err(send(&fwd, vec![provider("p1", &up)], plain_body()).await);

        assert_eq!(
            books(&fwd).await,
            Books {
                total: 1,
                success: 0,
                failed: 1,
                success_rate: 0.0,
                failover_count: 0,
                last_error: Some(err.error.to_string()),
                attempting: Some("p1".into()),
                in_use: None,
            }
        );
        assert_eq!(breaker(&fwd, "p1").await, None);
        assert_eq!(up.requests.lock().await.len(), 1);
    }

    #[tokio::test]
    async fn budget_rectifier_retry_success() {
        let up = upstream(vec![error(400, BUDGET_ERROR), ok()]).await;
        let fwd = forwarder(1, "p1");

        let result = send(
            &fwd,
            vec![provider("p1", &up)],
            body_with_thinking(json!({ "type": "enabled", "budget_tokens": 500 })),
        )
        .await;

        assert!(result.is_ok());
        assert_eq!(
            books(&fwd).await,
            Books {
                total: 1,
                success: 1,
                failed: 0,
                success_rate: 100.0,
                failover_count: 0,
                last_error: None,
                attempting: Some("p1".into()),
                in_use: Some("p1".into()),
            }
        );
        assert_eq!(breaker(&fwd, "p1").await, Some((1, 0)));
        let requests = up.requests.lock().await;
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[1]["thinking"]["budget_tokens"], 32000);
    }

    const DISABLED_ERROR: &str = r#"invalid params, model "MiniMax-M3.1-Flash" requires adaptive thinking; thinking.type="disabled" (including reasoning.effort=none) is not allowed (2013)"#;

    async fn assert_thinking_disabled_rejection_retries(app_type: AppType) {
        let up = upstream(vec![error(400, DISABLED_ERROR), ok()]).await;
        let fwd = forwarder(1, "p1");

        let result = send_as(
            &app_type,
            &fwd,
            vec![provider("p1", &up)],
            body_with_thinking(json!({ "type": "disabled" })),
        )
        .await;

        assert!(result.is_ok());
        if app_type == AppType::Claude {
            // 熔断记录只按 claude 查（Claude Desktop 没有故障转移队列）
            assert_eq!(breaker(&fwd, "p1").await, Some((1, 0)));
        }
        let requests = up.requests.lock().await;
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0]["thinking"]["type"], "disabled");
        assert!(requests[1].get("thinking").is_none());
        assert_eq!(requests[1]["max_tokens"], requests[0]["max_tokens"]);
    }

    #[tokio::test]
    async fn thinking_disabled_rejection_retries_without_thinking() {
        // Issue #8887：auto mode 分类器带 thinking disabled，MiniMax-M3.1-Flash 拒绝
        assert_thinking_disabled_rejection_retries(AppType::Claude).await;
    }

    #[tokio::test]
    async fn thinking_disabled_rejection_retries_on_claude_desktop_route() {
        // Issue #9671：Claude Desktop 路由（/claude-desktop）同样整流
        assert_thinking_disabled_rejection_retries(AppType::ClaudeDesktop).await;
    }

    #[tokio::test]
    async fn thinking_disabled_rejection_without_disabled_thinking_is_not_retried() {
        let up = upstream(vec![error(400, DISABLED_ERROR), ok()]).await;
        let fwd = forwarder(1, "p1");

        let result = send(
            &fwd,
            vec![provider("p1", &up)],
            body_with_thinking(json!({ "type": "adaptive" })),
        )
        .await;

        // 不是 disabled：没有可整流的内容，原样报错、不重试
        assert!(result.is_err());
        assert_eq!(up.requests.lock().await.len(), 1);
    }

    #[tokio::test]
    async fn budget_error_without_anything_to_rectify_is_neutral() {
        let up = upstream(vec![error(400, BUDGET_ERROR)]).await;
        let fwd = forwarder(1, "p1");

        let err = expect_err(
            send(
                &fwd,
                vec![provider("p1", &up)],
                body_with_thinking(json!({ "type": "adaptive" })),
            )
            .await,
        );

        assert_eq!(
            books(&fwd).await,
            Books {
                total: 1,
                success: 0,
                failed: 1,
                success_rate: 0.0,
                failover_count: 0,
                last_error: Some(err.error.to_string()),
                attempting: Some("p1".into()),
                in_use: None,
            }
        );
        assert_eq!(breaker(&fwd, "p1").await, None);
        assert_eq!(up.requests.lock().await.len(), 1);
    }

    #[tokio::test]
    async fn media_retry_success_on_the_second_provider_counts_a_switch() {
        let up1 = upstream(vec![error(500, "boom")]).await;
        let up2 = upstream(vec![error(400, "Model only supports text input"), ok()]).await;
        let fwd = forwarder(2, "p1");

        let result = send(
            &fwd,
            vec![provider("p1", &up1), provider("p2", &up2)],
            body_with_image("claude-sonnet-4-5"),
        )
        .await;

        assert_eq!(result.ok().map(|r| r.provider.id), Some("p2".into()));
        assert_eq!(
            books(&fwd).await,
            Books {
                total: 1,
                success: 1,
                failed: 0,
                success_rate: 100.0,
                failover_count: 1,
                last_error: None,
                attempting: Some("p2".into()),
                in_use: Some("p2".into()),
            }
        );
        assert_eq!(breaker(&fwd, "p1").await, Some((1, 1)));
        assert_eq!(breaker(&fwd, "p2").await, Some((1, 0)));
        let requests = up2.requests.lock().await;
        assert_eq!(requests.len(), 2);
        assert!(requests[1]
            .to_string()
            .contains(crate::proxy::media_sanitizer::UNSUPPORTED_IMAGE_MARKER));
    }

    // ---- Stack 模型：不读也不写任何路由状态 ----

    fn untouched() -> Books {
        Books {
            total: 0,
            success: 0,
            failed: 0,
            success_rate: 0.0,
            failover_count: 0,
            last_error: None,
            attempting: None,
            in_use: None,
        }
    }

    /// 熔断器从没为这家建过记录（成功是异步记的，给它时间落下）。
    async fn breaker_untouched(forwarder: &RequestForwarder, provider_id: &str) -> bool {
        tokio::time::sleep(Duration::from_millis(50)).await;
        forwarder
            .router
            .get_circuit_breaker_stats(provider_id, &AppType::Claude)
            .await
            .is_none()
    }

    fn stack_forwarder() -> RequestForwarder {
        // 请求开始时的「当前供应商」是别家：普通请求成功后会记一次故障转移并切路由。
        forwarder(4, "route").stack_request(true)
    }

    #[tokio::test]
    async fn stacked_requests_leave_every_routing_state_alone() {
        for script in [
            vec![ok()],
            vec![error(503, "overloaded")],
            vec![error(400, "bad request")],
        ] {
            let status = script[0].0;
            let up = upstream(script).await;
            let fwd = stack_forwarder();

            let result = send(&fwd, vec![provider("kimi", &up)], plain_body()).await;

            assert_eq!(result.is_ok(), status == 200, "{status}");
            if let Ok(result) = &result {
                assert!(result.connection_guard.is_none(), "no active connection");
            }
            assert_eq!(books(&fwd).await, untouched(), "{status}");
            assert_eq!(fwd.status.read().await.active_connections, 0);
            assert!(breaker_untouched(&fwd, "kimi").await, "{status}");
            assert_eq!(up.requests.lock().await.len(), 1, "{status}");
        }
    }

    #[tokio::test]
    async fn stacked_requests_keep_their_model() {
        let up = upstream(vec![ok(), ok()]).await;
        let mut kimi = provider("kimi", &up);
        kimi.settings_config["env"]["ANTHROPIC_MODEL"] = json!("kimi-k2");
        let mut body = plain_body();
        body["model"] = json!("kimi-k3");

        // 对照：普通请求按角色映射成行里配置的模型。
        send(&forwarder(1, "kimi"), vec![kimi.clone()], body.clone())
            .await
            .map_err(|e| e.error)
            .expect("routed");
        send(&stack_forwarder(), vec![kimi], body)
            .await
            .map_err(|e| e.error)
            .expect("stack mode");

        let requests = up.requests.lock().await;
        assert_eq!(requests[0]["model"], "kimi-k2");
        assert_eq!(requests[1]["model"], "kimi-k3");
    }

    #[tokio::test]
    async fn claude_desktop_stacked_requests_skip_the_mapping_table() {
        let up = upstream(vec![ok(), ok()]).await;
        let mut kimi = provider("kimi", &up);
        kimi.meta = Some(crate::provider::ProviderMeta {
            claude_desktop_model_routes: [(
                "claude-sonnet-5".to_string(),
                crate::provider::ClaudeDesktopModelRoute {
                    model: "kimi-k2".to_string(),
                    label_override: None,
                    supports_1m: None,
                },
            )]
            .into_iter()
            .collect(),
            ..Default::default()
        });
        let send_desktop = |fwd: RequestForwarder, model: &'static str| {
            let kimi = kimi.clone();
            async move {
                let mut body = plain_body();
                body["model"] = json!(model);
                fwd.forward_with_retry(
                    &AppType::ClaudeDesktop,
                    http::Method::POST,
                    "/v1/messages",
                    wire_body(body.clone()),
                    HeaderMap::new(),
                    Extensions::new(),
                    &[kimi],
                )
                .await
                .map_err(|e| e.error)
            }
        };

        // 对照：路由请求按映射表换成上游名；聚合请求体里已经是上游名，映射表里没有它也照发。
        send_desktop(forwarder(1, "kimi"), "claude-sonnet-5")
            .await
            .expect("routed");
        send_desktop(stack_forwarder(), "kimi-k3")
            .await
            .expect("stack mode");

        let requests = up.requests.lock().await;
        assert_eq!(requests[0]["model"], "kimi-k2");
        assert_eq!(requests[1]["model"], "kimi-k3");
    }

    #[tokio::test]
    async fn stacked_requests_stay_stacked_through_rectifier_retries() {
        let up = upstream(vec![error(400, SIGNATURE_ERROR), ok()]).await;
        let mut kimi = provider("kimi", &up);
        kimi.settings_config["env"]["ANTHROPIC_MODEL"] = json!("kimi-k2");
        let mut body = body_with_thinking_signature();
        body["model"] = json!("kimi-k3");
        let fwd = stack_forwarder();

        send(&fwd, vec![kimi], body)
            .await
            .map_err(|e| e.error)
            .expect("retried");

        let requests = up.requests.lock().await;
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[1]["model"], "kimi-k3");
        assert!(!requests[1].to_string().contains("\"signature\""));
        assert_eq!(books(&fwd).await, untouched());
        assert!(breaker_untouched(&fwd, "kimi").await);
    }
}

fn wire_body(body: Value) -> ForwardBody {
    let bytes = Bytes::from(serde_json::to_vec(&body).unwrap());
    ForwardBody::from_parsed(body, bytes)
}

#[test]
fn owned_attempt_moves_history_and_replay_restores_the_resolved_model() {
    let original = json!({"model": "stack-alias", "messages": [{"role":"user", "content": "x".repeat(1024 * 1024)}]});
    let bytes = Bytes::from(serde_json::to_vec(&original).unwrap());
    let mut resolved = original.clone();
    resolved["model"] = json!("upstream-model");
    let pointer = resolved["messages"][0]["content"]
        .as_str()
        .unwrap()
        .as_ptr();
    let mut replay = ForwardBody::from_parsed(resolved, bytes);
    let mut first = replay.next().unwrap();
    assert_eq!(
        first["messages"][0]["content"].as_str().unwrap().as_ptr(),
        pointer,
        "the first attempt must take the original allocation"
    );
    first["messages"][0]["content"] = json!("rewritten for the first provider");
    first["model"] = json!("provider-specific-model");
    let retried = replay.next().unwrap();
    assert_eq!(retried["model"], "upstream-model");
    assert_eq!(retried["messages"], original["messages"]);
    let mut empty = ForwardBody::from_parsed(Value::Null, Bytes::new());
    assert_eq!(empty.next().unwrap(), Value::Null);
    assert_eq!(empty.next().unwrap(), Value::Null);
}

#[test]
fn no_op_copilot_rewrites_do_not_require_another_privacy_scan() {
    use super::super::copilot_optimizer::{
        merge_tool_results_with_changes, sanitize_orphan_tool_results_with_changes,
    };
    for body in [
        json!({"messages": [{"role":"user", "content":"hello"}, {"role":"assistant", "content":"hi"}]}),
        json!({"messages": [
            {"role": "user", "content": "Run two tools"},
            {"role": "assistant", "content": [{"type": "tool_use", "id": "one", "name": "read", "input": {}}]},
            {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "one", "content": "first result"}]},
            {"role": "assistant", "content": [{"type": "tool_use", "id": "two", "name": "read", "input": {}}]},
            {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "two", "content": "second result"}]}
        ]}),
    ] {
        let (sanitized, changed) = sanitize_orphan_tool_results_with_changes(body.clone());
        assert!(!changed);
        let (merged, changed) = merge_tool_results_with_changes(sanitized);
        assert!(!changed);
        assert_eq!(merged, body);
    }
}
