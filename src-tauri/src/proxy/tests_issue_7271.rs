//! 复现测试：Issue #7271 - Claude(Anthropic) 路径未剥离 OpenAI Chat 上游内联的 `<think>` 块
//!
//! 复现 issue「方式 B」：Claude Code 的 provider 经 CC Switch 本地代理指向一个
//! OpenAI Chat 兼容上游（MiniMax M3 / OpenCode Go 形态），上游不返回
//! `reasoning_content` 字段，而是把思考内联在 `message.content` / `delta.content`
//! 文本前部（`<think>…</think>`）。期望代理转成 Anthropic 的 thinking 块；修复前
//! 整段思考被当作正文明文下发（含标签），Claude Code 里看不到折叠块。
//!
//! 测试用 axum 模拟这类上游，经真实 `ProxyServer` 发 HTTP 请求验证端到端行为。

#[cfg(test)]
mod tests {
    use crate::database::Database;
    use crate::provider::Provider;
    use crate::proxy::server::ProxyServer;
    use crate::proxy::types::ProxyConfig;
    use axum::{
        body::Body,
        extract::State,
        http::{header, StatusCode},
        response::{IntoResponse, Response},
        routing::post,
        Json, Router,
    };
    use serde_json::{json, Value};
    use serial_test::serial;
    use std::env;
    use std::sync::{Arc, Mutex};
    use tempfile::TempDir;
    use tokio::net::TcpListener;

    const REASONING: &str = "The bat costs $1.05 more than the ball.";
    const ANSWER: &str = "The ball costs $0.05.";
    /// 上游内联思考的原始文本（与变换后应当出现的两段对应）
    const INLINE_THINK_CONTENT: &str =
        "<think>The bat costs $1.05 more than the ball.</think>\n\nThe ball costs $0.05.";

    #[derive(Clone, Debug)]
    struct CapturedRequest {
        path: String,
        stream: bool,
    }

    /// 模拟 OpenAI Chat 上游：思考内联在 content 里。
    /// 非流式返回整体 JSON，流式（`stream=true`）返回把 `<think>` 标签跨 chunk
    /// 切分的 SSE——上游确实会这么切，转换必须能缓冲前缀。
    async fn mock_inline_think_upstream(
        State(captured): State<Arc<Mutex<Vec<CapturedRequest>>>>,
        req: axum::extract::Request,
    ) -> Response {
        let path = req.uri().path().to_string();
        let body_bytes = axum::body::to_bytes(req.into_body(), usize::MAX)
            .await
            .unwrap();
        let body: Value = serde_json::from_slice(&body_bytes).unwrap_or(json!({}));
        let stream = body
            .get("stream")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        captured
            .lock()
            .unwrap()
            .push(CapturedRequest { path, stream });

        if !stream {
            return Json(json!({
                "id": "chatcmpl-inline-think",
                "object": "chat.completion",
                "model": "minimax-m3",
                "choices": [{
                    "index": 0,
                    "message": {"role": "assistant", "content": INLINE_THINK_CONTENT},
                    "finish_reason": "stop"
                }],
                "usage": {"prompt_tokens": 30, "completion_tokens": 25, "total_tokens": 55}
            }))
            .into_response();
        }

        let chunks = [
            json!({"id":"chatcmpl-inline-think","model":"minimax-m3","choices":[{"delta":{"content":"<thi"}}]}),
            json!({"id":"chatcmpl-inline-think","model":"minimax-m3","choices":[{"delta":{"content":"nk>The bat costs $1.05 more than the ball.</thi"}}]}),
            json!({"id":"chatcmpl-inline-think","model":"minimax-m3","choices":[{"delta":{"content":"nk>The ball costs $0.05."}}]}),
            json!({"id":"chatcmpl-inline-think","model":"minimax-m3","choices":[{"delta":{},"finish_reason":"stop"}],"usage":{"prompt_tokens":30,"completion_tokens":25}}),
        ];
        let mut sse = String::new();
        for chunk in chunks {
            sse.push_str(&format!("data: {chunk}\n\n"));
        }
        sse.push_str("data: [DONE]\n\n");

        Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "text/event-stream")
            .body(Body::from(sse))
            .unwrap()
    }

    /// 启动假上游，返回 (base_url, 收到的请求)
    async fn start_inline_think_upstream() -> (String, Arc<Mutex<Vec<CapturedRequest>>>) {
        let captured = Arc::new(Mutex::new(Vec::new()));
        let app = Router::new()
            .route("/v1/chat/completions", post(mock_inline_think_upstream))
            .route("/chat/completions", post(mock_inline_think_upstream))
            .with_state(Arc::clone(&captured));

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        (format!("http://{addr}"), captured)
    }

    /// 建库（provider 走 OpenAI Chat 协议）+ 启动 CC Switch 代理
    ///
    /// 返回值里必须带上 `ProxyServer` 本身：它持有 shutdown oneshot 的 sender，
    /// 一旦被 drop，服务器 accept 循环会立刻退出（请求表现为连接被重置）。
    async fn start_proxy_with_inline_think_provider(
        mock_base_url: String,
    ) -> (ProxyServer, String) {
        let db = Arc::new(Database::memory().unwrap());
        let provider = Provider::with_id(
            "test-inline-think".to_string(),
            "MiniMax M3 (OpenAI Chat upstream)".to_string(),
            json!({
                "env": {
                    "ANTHROPIC_BASE_URL": mock_base_url,
                    "ANTHROPIC_AUTH_TOKEN": "test-inline-think-key"
                },
                "api_format": "openai_chat"
            }),
            None,
        );
        db.save_provider("claude", &provider).unwrap();
        db.set_current_provider("claude", &provider.id).unwrap();

        let proxy = ProxyServer::new(
            ProxyConfig {
                listen_port: 0,
                enable_logging: false,
                non_streaming_timeout: 10,
                ..ProxyConfig::default()
            },
            db.clone(),
            None,
        );
        let info = proxy.start().await.unwrap();
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        (proxy, format!("http://127.0.0.1:{}", info.port))
    }

    fn anthropic_request_body(stream: bool) -> Value {
        json!({
            "model": "claude-sonnet-4-6",
            "max_tokens": 200,
            "stream": stream,
            "messages": [{
                "role": "user",
                "content": "A bat and ball cost $1.10 total. The bat costs $1.00 more than the ball. How much does the ball cost?"
            }]
        })
    }

    struct TempHome {
        #[allow(dead_code)]
        dir: TempDir,
        original_home: Option<String>,
        original_userprofile: Option<String>,
        original_test_home: Option<String>,
    }

    impl TempHome {
        fn new() -> Self {
            let dir = TempDir::new().expect("failed to create temp home");
            let original_home = env::var("HOME").ok();
            let original_userprofile = env::var("USERPROFILE").ok();
            let original_test_home = env::var("CC_SWITCH_TEST_HOME").ok();

            env::set_var("HOME", dir.path());
            env::set_var("USERPROFILE", dir.path());
            env::set_var("CC_SWITCH_TEST_HOME", dir.path());
            crate::settings::reload_settings().expect("reload settings");

            Self {
                dir,
                original_home,
                original_userprofile,
                original_test_home,
            }
        }
    }

    impl Drop for TempHome {
        fn drop(&mut self) {
            match &self.original_home {
                Some(value) => env::set_var("HOME", value),
                None => env::remove_var("HOME"),
            }
            match &self.original_userprofile {
                Some(value) => env::set_var("USERPROFILE", value),
                None => env::remove_var("USERPROFILE"),
            }
            match &self.original_test_home {
                Some(value) => env::set_var("CC_SWITCH_TEST_HOME", value),
                None => env::remove_var("CC_SWITCH_TEST_HOME"),
            }
        }
    }

    /// 非流式：`/v1/messages` 的响应里思考必须是独立 thinking 块，标签不能泄漏
    #[tokio::test]
    #[serial]
    async fn proxy_converts_inline_think_to_thinking_block_non_streaming() {
        let _home = TempHome::new();
        let (mock_base_url, captured) = start_inline_think_upstream().await;
        let (_proxy, proxy_url) = start_proxy_with_inline_think_provider(mock_base_url).await;

        let response = reqwest::Client::new()
            .post(format!("{proxy_url}/v1/messages"))
            .header("x-api-key", "test-inline-think-key")
            .header("anthropic-version", "2023-06-01")
            .json(&anthropic_request_body(false))
            .send()
            .await
            .unwrap();

        let status = response.status();
        let body_text = response.text().await.unwrap();
        assert_eq!(
            status,
            reqwest::StatusCode::OK,
            "代理应返回 200，实际 {status}: {body_text}"
        );

        let body: Value = serde_json::from_str(&body_text).unwrap();
        assert_eq!(
            body["content"][0]["type"], "thinking",
            "全段思考都应成为独立 thinking 块（修复前是明文 text）: {body_text}"
        );
        assert_eq!(body["content"][0]["thinking"], REASONING);
        assert_eq!(body["content"][1]["type"], "text");
        assert_eq!(body["content"][1]["text"], ANSWER);
        assert_eq!(body["stop_reason"], "end_turn");
        assert!(
            !body_text.contains("<think>") && !body_text.contains("</think>"),
            "内联思考标签不应泄漏到 Anthropic 响应: {body_text}"
        );

        let captured = captured.lock().unwrap();
        assert_eq!(captured.len(), 1, "上游应收到 1 个请求");
        assert!(
            captured[0].path.ends_with("/chat/completions"),
            "provider 声明 openai_chat，上游应收到 Chat Completions 请求，实际 {}",
            captured[0].path
        );
    }

    /// 流式：`<think>` 标签跨 chunk 切分时仍要缓冲判定，输出 thinking_delta +
    /// text_delta，标签同样不能泄漏
    #[tokio::test]
    #[serial]
    async fn proxy_converts_inline_think_to_thinking_block_streaming() {
        let _home = TempHome::new();
        let (mock_base_url, captured) = start_inline_think_upstream().await;
        let (_proxy, proxy_url) = start_proxy_with_inline_think_provider(mock_base_url).await;

        let response = reqwest::Client::new()
            .post(format!("{proxy_url}/v1/messages"))
            .header("x-api-key", "test-inline-think-key")
            .header("anthropic-version", "2023-06-01")
            .json(&anthropic_request_body(true))
            .send()
            .await
            .unwrap();

        let status = response.status();
        let body_text = response.text().await.unwrap();
        assert_eq!(
            status,
            reqwest::StatusCode::OK,
            "代理应返回 200，实际 {status}: {body_text}"
        );

        let events: Vec<Value> = body_text
            .split("\n\n")
            .filter_map(|block| {
                let data = block.lines().find_map(|line| line.strip_prefix("data: "))?;
                serde_json::from_str(data).ok()
            })
            .collect();

        let thinking_delta: String = events
            .iter()
            .filter(|event| event["type"] == "content_block_delta")
            .filter(|event| {
                event.pointer("/delta/type").and_then(|v| v.as_str()) == Some("thinking_delta")
            })
            .filter_map(|event| event.pointer("/delta/thinking").and_then(|v| v.as_str()))
            .collect();
        let text_delta: String = events
            .iter()
            .filter(|event| event["type"] == "content_block_delta")
            .filter(|event| {
                event.pointer("/delta/type").and_then(|v| v.as_str()) == Some("text_delta")
            })
            .filter_map(|event| event.pointer("/delta/text").and_then(|v| v.as_str()))
            .collect();

        assert_eq!(
            thinking_delta, REASONING,
            "思考应以 thinking_delta 下发（修复前整段混在 text_delta 里）: {body_text}"
        );
        assert_eq!(text_delta, ANSWER);
        assert!(
            !body_text.contains("<think>") && !body_text.contains("</think>"),
            "内联思考标签不应泄漏到 SSE: {body_text}"
        );
        assert!(
            events.iter().any(|event| {
                event["type"] == "content_block_start"
                    && event
                        .pointer("/content_block/type")
                        .and_then(|v| v.as_str())
                        == Some("thinking")
            }),
            "应发出 thinking 内容块: {body_text}"
        );

        let captured = captured.lock().unwrap();
        assert_eq!(captured.len(), 1);
        assert!(captured[0].stream, "客户端请求流式时上游也应是流式");
        assert!(
            captured[0].path.ends_with("/chat/completions"),
            "provider 声明 openai_chat，上游应收到 Chat Completions 请求，实际 {}",
            captured[0].path
        );
    }
}
