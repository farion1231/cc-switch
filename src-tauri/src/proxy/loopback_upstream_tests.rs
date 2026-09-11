//! Loopback upstream direct-connect contract tests (forwarder level, end-to-end).
//!
//! These tests drive the real `ProxyServer` forwarding path against mock
//! endpoints so the loopback transport decision is verified end to end:
//!
//! - loopback target + no explicit proxy -> direct transport (mock upstream sees it)
//! - loopback target + explicit proxy    -> explicit proxy transport
//! - external target + explicit proxy    -> explicit proxy transport
//!
//! They mutate process-global HTTP client state, so they are gated behind the
//! existing `test-hooks` feature (excluded from default `cargo test`/CI) and
//! kept in a single sequential test function. Run them with:
//!
//! `cargo test --features test-hooks loopback_upstream -- --test-threads=1`

use super::{http_client, server::ProxyServer, ProxyConfig};
use crate::database::Database;
use crate::provider::{Provider, ProviderMeta};
use serde_json::json;
use std::sync::{Arc, Mutex};

const OPENAI_CHAT_BODY: &str = r#"{"id":"chatcmpl-test","object":"chat.completion","created":0,"model":"test-model","choices":[{"index":0,"message":{"role":"assistant","content":"ok"},"finish_reason":"stop"}],"usage":{"prompt_tokens":1,"completion_tokens":1,"total_tokens":2}}"#;
const PLACEHOLDER_TOKEN: &str = "test-placeholder-token";

/// Mock server that records every request it receives and always answers
/// `200` + `OPENAI_CHAT_BODY`. Used both as a mock upstream and as a mock
/// explicit proxy.
async fn spawn_recording_server(
    record: Arc<Mutex<Vec<String>>>,
) -> (std::net::SocketAddr, tokio::task::JoinHandle<()>) {
    let app = axum::Router::new().fallback(move |req: axum::extract::Request| {
        let record = record.clone();
        async move {
            record
                .lock()
                .unwrap()
                .push(format!("{} {}", req.method(), req.uri()));
            (
                axum::http::StatusCode::OK,
                [(axum::http::header::CONTENT_TYPE, "application/json")],
                OPENAI_CHAT_BODY,
            )
        }
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock server");
    let addr = listener.local_addr().expect("mock server addr");
    let handle = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    (addr, handle)
}

fn seed_provider(db: &Database, base_url: String) {
    let mut provider = Provider::with_id(
        "loopback-e2e-provider".to_string(),
        "Loopback E2E".to_string(),
        json!({
            "env": {
                "ANTHROPIC_BASE_URL": base_url,
                "ANTHROPIC_AUTH_TOKEN": PLACEHOLDER_TOKEN
            }
        }),
        None,
    );
    provider.meta = Some(
        serde_json::from_value::<ProviderMeta>(json!({ "apiFormat": "openai_chat" }))
            .expect("provider meta"),
    );
    db.save_provider("claude", &provider)
        .expect("save test provider");
    db.set_current_provider("claude", &provider.id)
        .expect("select test provider");
}

async fn send_messages(port: u16) -> reqwest::Response {
    // Test-side client must itself bypass any host-level system proxy so the
    // assertions observe the proxy server's behavior, not the test runner's.
    http_client::get_direct()
        .post(format!("http://127.0.0.1:{port}/v1/messages"))
        .header("x-api-key", PLACEHOLDER_TOKEN)
        .header("anthropic-version", "2023-06-01")
        .json(&json!({
            "model": "test-model",
            "max_tokens": 16,
            "messages": [{"role": "user", "content": "ping"}]
        }))
        .send()
        .await
        .expect("send /v1/messages request")
}

#[tokio::test]
async fn loopback_upstream_direct_connect_contract_end_to_end() {
    let home = tempfile::tempdir().expect("temp home");
    std::env::set_var("CC_SWITCH_TEST_HOME", home.path());

    let db = Arc::new(Database::init().expect("init isolated database"));
    let upstream_rec: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let (upstream_addr, upstream_handle) = spawn_recording_server(upstream_rec.clone()).await;
    let proxy_rec: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let (explicit_proxy_addr, explicit_proxy_handle) =
        spawn_recording_server(proxy_rec.clone()).await;

    seed_provider(&db, format!("http://127.0.0.1:{}", upstream_addr.port()));

    let _ = http_client::init(None);

    // Scenario 1: no explicit proxy -> loopback target must go direct.
    http_client::apply_proxy(None).expect("reset to no explicit proxy");
    let server = ProxyServer::new(
        ProxyConfig {
            listen_port: 0,
            enable_logging: false,
            ..ProxyConfig::default()
        },
        db.clone(),
        None,
    );
    let info = server.start().await.expect("start proxy server");
    let resp = send_messages(info.port).await;
    assert_eq!(resp.status(), 200, "direct loopback request must succeed");
    assert_eq!(
        upstream_rec.lock().unwrap().len(),
        1,
        "loopback provider must be reached directly (no explicit proxy)"
    );
    assert_eq!(
        proxy_rec.lock().unwrap().len(),
        0,
        "no proxy must be involved without an explicit proxy"
    );
    server.stop().await.expect("stop proxy server");

    // Scenario 2: explicit proxy configured -> loopback request must use it
    // (explicit proxy semantics are preserved, not silently overridden).
    let explicit_proxy_url = format!("http://127.0.0.1:{}", explicit_proxy_addr.port());
    http_client::apply_proxy(Some(explicit_proxy_url.as_str())).expect("apply explicit proxy");
    let server = ProxyServer::new(
        ProxyConfig {
            listen_port: 0,
            enable_logging: false,
            ..ProxyConfig::default()
        },
        db.clone(),
        None,
    );
    let info = server
        .start()
        .await
        .expect("start proxy server (explicit proxy)");
    let resp = send_messages(info.port).await;
    assert_eq!(resp.status(), 200, "explicit-proxied request must succeed");
    assert_eq!(
        proxy_rec.lock().unwrap().len(),
        1,
        "loopback request must go through the explicit proxy"
    );
    assert_eq!(
        upstream_rec.lock().unwrap().len(),
        1,
        "loopback provider must not be reached directly while an explicit proxy is set"
    );
    server.stop().await.expect("stop proxy server");

    // Scenario 3: explicit proxy + external target must also use the proxy.
    db.set_current_provider("claude", "loopback-e2e-provider")
        .expect("re-select provider");
    // External (TEST-NET-1, non-routable) target: the mock proxy answers, so no
    // real network traffic is required.
    {
        let mut external = Provider::with_id(
            "external-e2e-provider".to_string(),
            "External E2E".to_string(),
            json!({
                "env": {
                    "ANTHROPIC_BASE_URL": "http://192.0.2.1",
                    "ANTHROPIC_AUTH_TOKEN": PLACEHOLDER_TOKEN
                }
            }),
            None,
        );
        external.meta = Some(
            serde_json::from_value::<ProviderMeta>(json!({ "apiFormat": "openai_chat" }))
                .expect("provider meta"),
        );
        db.save_provider("claude", &external)
            .expect("save external provider");
        db.set_current_provider("claude", "external-e2e-provider")
            .expect("select external provider");
    }
    let server = ProxyServer::new(
        ProxyConfig {
            listen_port: 0,
            enable_logging: false,
            ..ProxyConfig::default()
        },
        db.clone(),
        None,
    );
    let info = server.start().await.expect("start proxy server (external)");
    let resp = send_messages(info.port).await;
    assert_eq!(
        resp.status(),
        200,
        "external request via explicit proxy must succeed"
    );
    let rec = proxy_rec.lock().unwrap().clone();
    assert_eq!(
        rec.len(),
        2,
        "external request must go through the explicit proxy"
    );
    assert!(
        rec.last().unwrap().contains("192.0.2.1"),
        "{}",
        rec.last().unwrap()
    );
    server.stop().await.expect("stop proxy server");

    // Restore global state for any follow-up tests in the same binary.
    let _ = http_client::apply_proxy(None);
    upstream_handle.abort();
    explicit_proxy_handle.abort();
}
