//! 供应商聚合端到端测试
//!
//! 启动真实的本地代理实例 + 两个模拟上游，验证：把一个「聚合供应商」设为当前供应商后，
//! 代理按请求体中的模型名把请求路由到对应上游，并支持上游模型名改写。
//! 上游用不同的路径前缀（/a、/b）区分；均声明 anthropic 格式（无需格式转换）。

#[path = "support.rs"]
mod support;
use support::{create_test_state, ensure_test_home, reset_test_fs, test_mutex};

use cc_switch_lib::{Provider, ProviderMeta};
use serde_json::json;
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Clone, Debug)]
struct UpstreamHit {
    path: String,
    model: String,
    authorization: Option<String>,
    x_api_key: Option<String>,
}

async fn start_mock_upstream(hits: Arc<Mutex<Vec<UpstreamHit>>>) -> u16 {
    use axum::{
        body::Bytes,
        http::{HeaderMap, Uri},
        response::IntoResponse,
        Router,
    };

    async fn handler(
        axum::extract::State(hits): axum::extract::State<Arc<Mutex<Vec<UpstreamHit>>>>,
        uri: Uri,
        headers: HeaderMap,
        body: Bytes,
    ) -> impl IntoResponse {
        let model = serde_json::from_slice::<serde_json::Value>(&body)
            .ok()
            .and_then(|v| v.get("model").and_then(|m| m.as_str()).map(String::from))
            .unwrap_or_default();
        hits.lock().unwrap().push(UpstreamHit {
            path: uri.path().to_string(),
            model,
            authorization: headers
                .get("authorization")
                .and_then(|value| value.to_str().ok())
                .map(str::to_string),
            x_api_key: headers
                .get("x-api-key")
                .and_then(|value| value.to_str().ok())
                .map(str::to_string),
        });
        axum::Json(json!({
            "id": "msg_mock", "type": "message", "role": "assistant", "model": "mock",
            "content": [{"type": "text", "text": "ok"}],
            "stop_reason": "end_turn", "stop_sequence": null,
            "usage": {"input_tokens": 1, "output_tokens": 1}
        }))
    }

    let app = Router::new().fallback(handler).with_state(hits);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock upstream");
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("serve mock upstream");
    });
    port
}

async fn send_model(client: &reqwest::Client, proxy: &str, model: &str) -> reqwest::StatusCode {
    client
        .post(proxy)
        .header("content-type", "application/json")
        .header("anthropic-version", "2023-06-01")
        .header("x-api-key", "client-key")
        .json(&json!({
            "model": model, "max_tokens": 16,
            "messages": [{"role": "user", "content": "hi"}]
        }))
        .send()
        .await
        .expect("send request to proxy")
        .status()
}

// 测试使用 Mutex 进行串行化，跨 await 持锁是预期行为
#[allow(clippy::await_holding_lock)]
#[tokio::test]
async fn aggregation_provider_routes_by_model_end_to_end() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let _home = ensure_test_home();

    let hits = Arc::new(Mutex::new(Vec::<UpstreamHit>::new()));
    let mock = start_mock_upstream(hits.clone()).await;

    let state = create_test_state().expect("create test state");

    // 聚合供应商：两条上游(/a、/b)，两条路由(model-a→u1, model-b→u2 + 改写)
    let mut agg = Provider::with_id("agg1".into(), "AGG".into(), json!({}), None);
    agg.meta = Some(ProviderMeta {
        provider_type: Some("aggregation".to_string()),
        aggregation: Some(json!({
                "upstreams": [
                    {"id":"u1","name":"A","baseUrl": format!("http://127.0.0.1:{mock}/a"), "apiKey":"key-a","apiFormat":"anthropic"},
                    {"id":"u2","name":"B","baseUrl": format!("http://127.0.0.1:{mock}/b"), "apiKey":"key-b","apiFormat":"anthropic","apiKeyField":"ANTHROPIC_API_KEY"}
                ],
                "routes": [
                    {"model":"model-a","upstreamId":"u1"},
                    {"model":"model-b","upstreamId":"u2","upstreamModel":"real-upstream-b"}
                ]
        })),
        ..Default::default()
    });
    state.db.save_provider("claude", &agg).expect("save agg");
    state
        .db
        .set_current_provider("claude", "agg1")
        .expect("set current");

    // 启用 claude 接管
    let mut cfg = state
        .db
        .get_proxy_config_for_app("claude")
        .await
        .expect("get app config");
    cfg.enabled = true;
    state
        .db
        .update_proxy_config_for_app(cfg)
        .await
        .expect("enable takeover");

    // 临时端口，避免与本机正式实例冲突
    let mut gc = state
        .db
        .get_global_proxy_config()
        .await
        .expect("get global config");
    gc.listen_port = 0;
    state
        .db
        .update_global_proxy_config(gc)
        .await
        .expect("set ephemeral port");

    let info = state.proxy_service.start().await.expect("start proxy");
    let proxy = format!("http://127.0.0.1:{}/v1/messages", info.port);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap();
    let last = || hits.lock().unwrap().last().cloned();

    // model-a → 上游 A(/a)
    let st = send_model(&client, &proxy, "model-a").await;
    assert!(st.is_success(), "model-a status {st}");
    let h = last().expect("hit a");
    assert!(
        h.path.starts_with("/a"),
        "model-a should route to /a, got {}",
        h.path
    );
    assert_eq!(h.authorization.as_deref(), Some("Bearer key-a"));
    assert_eq!(h.x_api_key, None);

    // model-b → 上游 B(/b)，且模型名被改写为 real-upstream-b
    let st = send_model(&client, &proxy, "model-b").await;
    assert!(st.is_success(), "model-b status {st}");
    let h = last().unwrap();
    assert!(
        h.path.starts_with("/b"),
        "model-b should route to /b, got {}",
        h.path
    );
    assert_eq!(
        h.model, "real-upstream-b",
        "upstream_model rewrite should apply"
    );
    assert_eq!(h.authorization, None);
    assert_eq!(h.x_api_key.as_deref(), Some("key-b"));

    // 未配置的模型 → 400（聚合无匹配路由）
    let st = send_model(&client, &proxy, "model-unconfigured").await;
    assert_eq!(st.as_u16(), 400, "unconfigured model should 400, got {st}");

    let _ = state.proxy_service.stop().await;
}

/// 手工验收真实上游：
/// CC_SWITCH_AGGREGATION_LIVE_BASE_URL=http://host:port \
/// CC_SWITCH_AGGREGATION_LIVE_MODEL=model-id \
/// CC_SWITCH_AGGREGATION_LIVE_API_FORMAT=openai_responses \
/// cargo test --test aggregation_e2e aggregation_provider_routes_to_live_upstream -- --ignored --exact
#[allow(clippy::await_holding_lock)]
#[tokio::test]
#[ignore = "requires an explicitly configured live upstream"]
async fn aggregation_provider_routes_to_live_upstream() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let _home = ensure_test_home();

    let base_url = std::env::var("CC_SWITCH_AGGREGATION_LIVE_BASE_URL")
        .expect("CC_SWITCH_AGGREGATION_LIVE_BASE_URL is required");
    let model = std::env::var("CC_SWITCH_AGGREGATION_LIVE_MODEL")
        .expect("CC_SWITCH_AGGREGATION_LIVE_MODEL is required");
    let api_format = std::env::var("CC_SWITCH_AGGREGATION_LIVE_API_FORMAT")
        .unwrap_or_else(|_| "anthropic".to_string());
    let api_key = std::env::var("CC_SWITCH_AGGREGATION_LIVE_API_KEY").unwrap_or_default();
    let api_key_field = std::env::var("CC_SWITCH_AGGREGATION_LIVE_API_KEY_FIELD")
        .unwrap_or_else(|_| "ANTHROPIC_AUTH_TOKEN".to_string());

    let state = create_test_state().expect("create test state");
    let mut provider = Provider::with_id("agg-live".into(), "AGG Live".into(), json!({}), None);
    provider.meta = Some(ProviderMeta {
        provider_type: Some("aggregation".to_string()),
        aggregation: Some(json!({
            "upstreams": [{
                "id": "live",
                "name": "Live upstream",
                "baseUrl": base_url,
                "apiKey": api_key,
                "apiFormat": api_format,
                "apiKeyField": api_key_field
            }],
            "routes": [{"model": model, "upstreamId": "live"}]
        })),
        ..Default::default()
    });
    state
        .db
        .save_provider("claude", &provider)
        .expect("save live aggregation provider");
    state
        .db
        .set_current_provider("claude", "agg-live")
        .expect("set current provider");

    let mut app_config = state
        .db
        .get_proxy_config_for_app("claude")
        .await
        .expect("get app config");
    app_config.enabled = true;
    state
        .db
        .update_proxy_config_for_app(app_config)
        .await
        .expect("enable takeover");

    let mut global_config = state
        .db
        .get_global_proxy_config()
        .await
        .expect("get global config");
    global_config.listen_port = 0;
    state
        .db
        .update_global_proxy_config(global_config)
        .await
        .expect("set ephemeral port");

    let info = state.proxy_service.start().await.expect("start proxy");
    let proxy = format!("http://127.0.0.1:{}/v1/messages", info.port);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .build()
        .unwrap();

    let status = send_model(&client, &proxy, &model).await;
    assert!(status.is_success(), "live aggregation status {status}");

    let _ = state.proxy_service.stop().await;
}
