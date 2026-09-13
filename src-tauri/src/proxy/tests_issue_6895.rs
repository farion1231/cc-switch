//! 复现测试：Issue #6895 - Codex wire_api=responses 企业网关 502 回归
//!
//! v3.16.5 → v3.20.0 回归：企业 Codex 提供商（`wire_api = "responses"`）直接 curl
//! 返回 HTTP 200 + 自定义非 OpenAI 信封 JSON；通过 CC Switch 本地代理转发后变成
//! HTTP 502，错误信息为 `"上游错误 (502):"` 带空 cause（`:` 后面什么都没有）。
//!
//! issue 评论者 chufeng 推测两个可能原因：
//! 1. v3.17.0 引入的「Fail Closed on 2xx 失败信封」逻辑（commit 650905af）
//!    误把正常 200 当错误
//! 2. v3.17–3.19 引入的「解压/缓冲」502 逻辑变化
//!
//! 本测试用 axum 模拟企业网关，验证代理的实际行为。

#[cfg(test)]
mod tests {
    use crate::database::Database;
    use crate::provider::Provider;
    use crate::proxy::server::ProxyServer;
    use crate::proxy::types::ProxyConfig;
    use axum::{extract::State, routing::post, Json, Router};
    use serde_json::{json, Value};
    use std::sync::{Arc, Mutex};
    use tokio::net::TcpListener;

    #[derive(Clone, Debug)]
    struct CapturedRequest {
        path_and_query: String,
        authorization: Option<String>,
        body: Value,
    }

    /// 模拟企业网关：返回 HTTP 200 + 自定义非 OpenAI-Responses 信封 JSON
    async fn mock_enterprise_gateway_200_custom_json(
        State(captured): State<Arc<Mutex<Vec<CapturedRequest>>>>,
        req: axum::extract::Request,
    ) -> (axum::http::StatusCode, Json<Value>) {
        let (parts, body) = req.into_parts();
        let path_and_query = parts
            .uri
            .path_and_query()
            .map(|pq| pq.as_str())
            .unwrap_or("")
            .to_string();
        let authorization = parts
            .headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .map(String::from);

        let body_bytes = axum::body::to_bytes(body, usize::MAX).await.unwrap();
        let body_json: Value = serde_json::from_slice(&body_bytes).unwrap_or(json!({}));

        captured.lock().unwrap().push(CapturedRequest {
            path_and_query,
            authorization,
            body: body_json,
        });

        // 企业网关返回的自定义 JSON（非 OpenAI Responses 标准信封）
        let custom_response = json!({
            "success": true,
            "data": {
                "completion": "This is a custom enterprise response format.",
                "tokens_used": 42
            }
        });

        (axum::http::StatusCode::OK, Json(custom_response))
    }

    /// 模拟企业网关：返回真实的 HTTP 502 空 body（上游挂了）
    async fn mock_enterprise_gateway_502_empty(
        State(captured): State<Arc<Mutex<Vec<CapturedRequest>>>>,
        req: axum::extract::Request,
    ) -> (axum::http::StatusCode, String) {
        let (parts, body) = req.into_parts();
        let path_and_query = parts
            .uri
            .path_and_query()
            .map(|pq| pq.as_str())
            .unwrap_or("")
            .to_string();
        let authorization = parts
            .headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .map(String::from);

        let body_bytes = axum::body::to_bytes(body, usize::MAX).await.unwrap();
        let body_json: Value = serde_json::from_slice(&body_bytes).unwrap_or(json!({}));

        captured.lock().unwrap().push(CapturedRequest {
            path_and_query,
            authorization,
            body: body_json,
        });

        // 上游真的挂了，返回 502 + 空 body
        (axum::http::StatusCode::BAD_GATEWAY, String::new())
    }

    #[tokio::test]
    async fn native_responses_provider_passes_through_200_custom_json() {
        // === 启动模拟企业网关（返回 HTTP 200 + 自定义 JSON）===
        let captured = Arc::new(Mutex::new(Vec::new()));
        let app = Router::new()
            .route(
                "/v1/responses",
                post(mock_enterprise_gateway_200_custom_json),
            )
            .with_state(Arc::clone(&captured));

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mock_addr = listener.local_addr().unwrap();
        let mock_base_url = format!("http://{mock_addr}");

        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        // === 创建 wire_api=responses 的测试 Provider ===
        let db = Arc::new(Database::memory().unwrap());
        let provider = Provider::with_id(
            "test-enterprise-responses".to_string(),
            "Enterprise Responses Gateway".to_string(),
            json!({
                "auth": {"OPENAI_API_KEY": "test-enterprise-key-123"},
                "base_url": mock_base_url,
                "config": r#"model = "gpt-4""#
            }),
            None,
        );
        db.save_provider("codex", &provider).unwrap();
        db.set_current_provider("codex", &provider.id).unwrap();

        // === 启动 CC Switch 代理 ===
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

        let proxy_info = proxy.start().await.unwrap();
        let proxy_url = format!("http://127.0.0.1:{}", proxy_info.port);

        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // === 通过代理转发 /v1/responses 请求 ===
        let client = reqwest::Client::new();
        let request_body = json!({
            "model": "gpt-4",
            "prompt": "Hello",
            "max_tokens": 50
        });

        let response = client
            .post(format!("{proxy_url}/v1/responses"))
            .header("Authorization", "Bearer test-enterprise-key-123")
            .json(&request_body)
            .send()
            .await
            .unwrap();

        // === 断言：代理应透传 HTTP 200 + 自定义 JSON，而非返回 502 ===
        let status = response.status();
        let body_text = response.text().await.unwrap();

        assert_eq!(
            status,
            reqwest::StatusCode::OK,
            "代理应透传上游的 200 而非误报 502；实际返回 {status}，body: {body_text}"
        );

        let body_json: Value = serde_json::from_str(&body_text).unwrap();
        assert_eq!(
            body_json.get("success").and_then(|v| v.as_bool()),
            Some(true),
            "响应应保留企业网关的自定义 JSON 结构"
        );
        assert_eq!(
            body_json
                .pointer("/data/completion")
                .and_then(|v| v.as_str()),
            Some("This is a custom enterprise response format."),
            "completion 字段应完整透传"
        );

        // === 确认上游收到了正确的请求 ===
        let captured_reqs = captured.lock().unwrap();
        assert_eq!(captured_reqs.len(), 1, "上游应收到 1 个请求");
        assert_eq!(
            captured_reqs[0].path_and_query, "/v1/responses",
            "base_url 为纯 origin 时，build_url 会补 /v1 前缀"
        );
        assert_eq!(
            captured_reqs[0].authorization.as_deref(),
            Some("Bearer test-enterprise-key-123"),
            "认证头应透传"
        );
        assert_eq!(
            captured_reqs[0].body.get("model").and_then(|v| v.as_str()),
            Some("gpt-4"),
            "请求 body 应透传"
        );
    }

    /// 模拟企业网关：真实 Codex CLI 是流式请求（stream=true + Accept: text/event-stream），
    /// 但企业网关对流式请求也返回 200 + 普通 JSON（非 SSE）。模拟这一形状。
    async fn mock_enterprise_gateway_streaming_request_returns_plain_json(
        State(captured): State<Arc<Mutex<Vec<CapturedRequest>>>>,
        req: axum::extract::Request,
    ) -> (axum::http::StatusCode, axum::http::HeaderMap, String) {
        let (parts, body) = req.into_parts();
        let path_and_query = parts
            .uri
            .path_and_query()
            .map(|pq| pq.as_str())
            .unwrap_or("")
            .to_string();
        let authorization = parts
            .headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .map(String::from);

        let body_bytes = axum::body::to_bytes(body, usize::MAX).await.unwrap();
        let body_json: Value = serde_json::from_slice(&body_bytes).unwrap_or(json!({}));

        captured.lock().unwrap().push(CapturedRequest {
            path_and_query,
            authorization,
            body: body_json,
        });

        let mut headers = axum::http::HeaderMap::new();
        headers.insert("content-type", "application/json".parse().unwrap());
        let custom_response = json!({
            "success": true,
            "data": {
                "completion": "Custom enterprise streaming-shaped response.",
            }
        });
        (
            axum::http::StatusCode::OK,
            headers,
            custom_response.to_string(),
        )
    }

    /// 复现 Issue #6895 的另一种可能：Codex CLI 真实请求是流式的
    /// （stream=true, Accept: text/event-stream），但上游网关返回 200 + 普通 JSON。
    /// 验证 v3.20.0 代理不会把这种 200 误判成 502。
    #[tokio::test]
    async fn native_responses_provider_passes_through_streaming_200_plain_json() {
        let captured = Arc::new(Mutex::new(Vec::new()));
        let app = Router::new()
            .route(
                "/v1/responses",
                post(mock_enterprise_gateway_streaming_request_returns_plain_json),
            )
            .with_state(Arc::clone(&captured));

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mock_addr = listener.local_addr().unwrap();
        let mock_base_url = format!("http://{mock_addr}");

        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        let db = Arc::new(Database::memory().unwrap());
        let provider = Provider::with_id(
            "test-enterprise-streaming".to_string(),
            "Enterprise Streaming Gateway".to_string(),
            json!({
                "auth": {"OPENAI_API_KEY": "test-stream-key-123"},
                "base_url": mock_base_url,
                "config": r#"model = "gpt-4""#
            }),
            None,
        );
        db.save_provider("codex", &provider).unwrap();
        db.set_current_provider("codex", &provider.id).unwrap();

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

        let proxy_info = proxy.start().await.unwrap();
        let proxy_url = format!("http://127.0.0.1:{}", proxy_info.port);

        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // 模拟 Codex CLI 的流式请求形状
        let client = reqwest::Client::new();
        let request_body = json!({
            "model": "gpt-4",
            "input": "Hello",
            "stream": true
        });

        let response = client
            .post(format!("{proxy_url}/v1/responses"))
            .header("Authorization", "Bearer test-stream-key-123")
            .header("Accept", "text/event-stream")
            .header("User-Agent", "codex/0.144.0")
            .json(&request_body)
            .send()
            .await
            .unwrap();

        let status = response.status();
        let body_text = response.text().await.unwrap();

        assert_eq!(
            status,
            reqwest::StatusCode::OK,
            "流式请求 + 200 普通 JSON 应透传而非误报 502；实际返回 {status}，body: {body_text}"
        );

        let body_json: Value = serde_json::from_str(&body_text).unwrap_or(json!({}));
        assert_eq!(
            body_json.get("success").and_then(|v| v.as_bool()),
            Some(true),
            "响应应保留企业网关的自定义 JSON 结构"
        );

        let captured_reqs = captured.lock().unwrap();
        assert_eq!(captured_reqs.len(), 1, "上游应收到 1 个请求");
        assert_eq!(
            captured_reqs[0].authorization.as_deref(),
            Some("Bearer test-stream-key-123"),
            "认证头应透传"
        );
        assert_eq!(
            captured_reqs[0]
                .body
                .get("stream")
                .and_then(|v| v.as_bool()),
            Some(true),
            "stream 字段应透传"
        );
    }

    #[tokio::test]
    async fn native_responses_provider_passes_through_real_502_empty_body() {
        // === 启动模拟企业网关（返回真实 HTTP 502 空 body）===
        let captured = Arc::new(Mutex::new(Vec::new()));
        let app = Router::new()
            .route("/v1/responses", post(mock_enterprise_gateway_502_empty))
            .with_state(Arc::clone(&captured));

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mock_addr = listener.local_addr().unwrap();
        let mock_base_url = format!("http://{mock_addr}");

        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        // === 创建 wire_api=responses 的测试 Provider ===
        let db = Arc::new(Database::memory().unwrap());
        let provider = Provider::with_id(
            "test-enterprise-502".to_string(),
            "Enterprise 502 Gateway".to_string(),
            json!({
                "auth": {"OPENAI_API_KEY": "test-502-key"},
                "config": format!(r#"model_provider = "enterprise"
model = "gpt-4"

[model_providers.enterprise]
name = "Enterprise Gateway"
base_url = "{mock_base_url}"
wire_api = "responses"
"#)
            }),
            None,
        );
        db.save_provider("codex", &provider).unwrap();
        db.set_current_provider("codex", &provider.id).unwrap();

        // === 启动 CC Switch 代理 ===
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

        let proxy_info = proxy.start().await.unwrap();
        let proxy_url = format!("http://127.0.0.1:{}", proxy_info.port);

        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // === 通过代理转发请求 ===
        let client = reqwest::Client::new();
        let request_body = json!({
            "model": "gpt-4",
            "prompt": "Hello",
            "max_tokens": 50
        });

        let response = client
            .post(format!("{proxy_url}/v1/responses"))
            .header("Authorization", "Bearer test-502-key")
            .json(&request_body)
            .send()
            .await
            .unwrap();

        // === 断言：代理应透传上游的真实 502，这是预期行为 ===
        let status = response.status();
        let body_text = response.text().await.unwrap();

        assert_eq!(
            status,
            reqwest::StatusCode::BAD_GATEWAY,
            "代理应透传上游的真实 502"
        );

        // 解析 Codex proxy 错误格式：cause 是拼接进 error.message 里的一段文本
        // （形如 "...; cause: 上游错误 (502):"），不是独立的 JSON 字段
        let body_json: Value = serde_json::from_str(&body_text).unwrap_or(json!({}));
        let message = body_json
            .pointer("/error/message")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        // 上游返回空 body 时，cause 应为 "上游错误 (502):" 带空尾巴（issue 原始症状）
        assert!(
            message.contains("cause: 上游错误 (502):"),
            "message 应包含上游 502 描述，实际: {message}"
        );
        // 这是 issue 报告的精确症状：冒号后空白（后面没有 body 内容）
        assert!(
            message.trim_end().ends_with("cause: 上游错误 (502):"),
            "上游空 body 应产生冒号后空白的 cause（issue #6895 症状），实际: '{message}'"
        );

        // === 确认上游收到了请求 ===
        let captured_reqs = captured.lock().unwrap();
        assert_eq!(captured_reqs.len(), 1, "上游应收到 1 个请求");
    }
}
