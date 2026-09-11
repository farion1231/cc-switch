//! Loopback upstream direct-connect contract tests (forwarder level, end-to-end).
//!
//! These tests drive the real `ProxyServer` forwarding path against mock
//! endpoints so the loopback transport decision is verified end to end:
//!
//! - loopback target + no explicit proxy -> direct transport (mock upstream sees it)
//! - loopback target + explicit proxy    -> explicit proxy transport
//! - external target + explicit proxy    -> explicit proxy transport
//! - cross-class redirect chains (loopback <-> external) re-select transport per
//!   hop: only loopback hops are forced direct, and method/body/sensitive-header
//!   semantics mirror reqwest/tower-http redirect handling
//! - route-state publication is a single atomic snapshot: the explicit-proxy
//!   metadata and its client are always read as one unit, even while
//!   `apply_proxy` hot-updates configuration concurrently
//!
//! They mutate process-global HTTP client state, so they are gated behind the
//! existing `test-hooks` feature (excluded from default `cargo test`/CI) and
//! kept sequential. Run them with:
//!
//! `cargo test --features test-hooks loopback_upstream -- --test-threads=1`

use super::{http_client, server::ProxyServer, ProxyConfig};
use crate::database::Database;
use crate::provider::{Provider, ProviderMeta};
use axum::response::IntoResponse;
use serde_json::json;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;

const OPENAI_CHAT_BODY: &str = r#"{"id":"chatcmpl-test","object":"chat.completion","created":0,"model":"test-model","choices":[{"index":0,"message":{"role":"assistant","content":"ok"},"finish_reason":"stop"}],"usage":{"prompt_tokens":1,"completion_tokens":1,"total_tokens":2}}"#;
const PLACEHOLDER_TOKEN: &str = "test-placeholder-token";

/// A request observed by a mock server (used both as a mock upstream and as a
/// mock proxy).
#[derive(Clone, Debug)]
struct RecReq {
    method: String,
    uri: String,
    authorization: Option<String>,
    x_api_key: Option<String>,
    body: Vec<u8>,
}

impl RecReq {
    fn body_text(&self) -> String {
        String::from_utf8_lossy(&self.body).to_string()
    }

    fn has_credential(&self) -> bool {
        self.authorization.is_some() || self.x_api_key.is_some()
    }
}

#[derive(Clone)]
enum MockReply {
    /// 200 + OPENAI_CHAT_BODY (final upstream response).
    Json200,
    /// Fixed status with optional Location header (redirect hop).
    Status(u16, Option<String>),
}

async fn record_request(req: axum::extract::Request, record: Arc<Mutex<Vec<RecReq>>>) {
    let (parts, body) = req.into_parts();
    let method = parts.method.to_string();
    let uri = parts.uri.to_string();
    let get = |name: &axum::http::HeaderName| {
        parts
            .headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string())
    };
    let authorization = get(&axum::http::header::AUTHORIZATION);
    let x_api_key = get(&axum::http::HeaderName::from_static("x-api-key"));
    let body = axum::body::to_bytes(body, 10 * 1024 * 1024)
        .await
        .map(|b| b.to_vec())
        .unwrap_or_default();
    record.lock().unwrap().push(RecReq {
        method,
        uri,
        authorization,
        x_api_key,
        body,
    });
}

fn build_reply(reply: &MockReply) -> axum::response::Response {
    match reply {
        MockReply::Json200 => (
            axum::http::StatusCode::OK,
            [(axum::http::header::CONTENT_TYPE, "application/json")],
            OPENAI_CHAT_BODY,
        )
            .into_response(),
        MockReply::Status(code, location) => {
            let mut resp = axum::http::StatusCode::from_u16(*code)
                .expect("valid mock status")
                .into_response();
            if let Some(loc) = location {
                resp.headers_mut().insert(
                    axum::http::header::LOCATION,
                    axum::http::HeaderValue::from_str(loc).expect("valid location"),
                );
            }
            resp
        }
    }
}

/// Mock server with a fixed reply; records every request it receives.
async fn spawn_mock(
    record: Arc<Mutex<Vec<RecReq>>>,
    reply: MockReply,
) -> (std::net::SocketAddr, tokio::task::JoinHandle<()>) {
    let app = axum::Router::new().fallback(move |req: axum::extract::Request| {
        let record = record.clone();
        let reply = reply.clone();
        async move {
            record_request(req, record).await;
            build_reply(&reply)
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

/// Mock server that pops its reply from a queue per request; records every
/// request. An empty queue answers `500`.
async fn spawn_mock_queue(
    record: Arc<Mutex<Vec<RecReq>>>,
    queue: Arc<Mutex<VecDeque<MockReply>>>,
) -> (std::net::SocketAddr, tokio::task::JoinHandle<()>) {
    let app = axum::Router::new().fallback(move |req: axum::extract::Request| {
        let record = record.clone();
        let queue = queue.clone();
        async move {
            record_request(req, record).await;
            match queue.lock().unwrap().pop_front() {
                Some(reply) => build_reply(&reply),
                None => (
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    "mock queue exhausted",
                )
                    .into_response(),
            }
        }
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock queue server");
    let addr = listener.local_addr().expect("mock queue server addr");
    let handle = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    (addr, handle)
}

/// Sets HTTP_PROXY/http_proxy for the duration of a test and restores the
/// previous values on drop. The following/inherited client picks these up at
/// `build_client` time (i.e. at `apply_proxy`/`init`).
struct EnvProxyGuard {
    prev_http: Option<String>,
    prev_lower: Option<String>,
}

impl EnvProxyGuard {
    fn set(url: &str) -> Self {
        let prev_http = std::env::var("HTTP_PROXY").ok();
        let prev_lower = std::env::var("http_proxy").ok();
        std::env::set_var("HTTP_PROXY", url);
        std::env::set_var("http_proxy", url);
        Self {
            prev_http,
            prev_lower,
        }
    }
}

impl Drop for EnvProxyGuard {
    fn drop(&mut self) {
        match &self.prev_http {
            Some(v) => std::env::set_var("HTTP_PROXY", v),
            None => std::env::remove_var("HTTP_PROXY"),
        }
        match &self.prev_lower {
            Some(v) => std::env::set_var("http_proxy", v),
            None => std::env::remove_var("http_proxy"),
        }
    }
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

fn fast_config() -> ProxyConfig {
    ProxyConfig {
        listen_port: 0,
        enable_logging: false,
        // Single attempt (no provider failover retries) so failure-path tests are
        // deterministic and bounded.
        max_retries: 0,
        // Bound redirect/failure paths quickly so transport mistakes surface as
        // fast errors instead of multi-minute hangs.
        non_streaming_timeout: 5,
        ..ProxyConfig::default()
    }
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

async fn start_server(db: Arc<Database>) -> (super::ProxyServerInfo, ProxyServer) {
    let server = ProxyServer::new(fast_config(), db, None);
    let info = server.start().await.expect("start proxy server");
    (info, server)
}

#[tokio::test]
async fn loopback_upstream_direct_connect_contract_end_to_end() {
    let home = tempfile::tempdir().expect("temp home");
    std::env::set_var("CC_SWITCH_TEST_HOME", home.path());

    let db = Arc::new(Database::init().expect("init isolated database"));
    let upstream_rec: Arc<Mutex<Vec<RecReq>>> = Arc::new(Mutex::new(Vec::new()));
    let (upstream_addr, upstream_handle) =
        spawn_mock(upstream_rec.clone(), MockReply::Json200).await;
    let proxy_rec: Arc<Mutex<Vec<RecReq>>> = Arc::new(Mutex::new(Vec::new()));
    let (explicit_proxy_addr, explicit_proxy_handle) =
        spawn_mock(proxy_rec.clone(), MockReply::Json200).await;

    seed_provider(&db, format!("http://127.0.0.1:{}/v1", upstream_addr.port()));

    let _ = http_client::init(None);

    // Scenario 1: no explicit proxy -> loopback target must go direct.
    http_client::apply_proxy(None).expect("reset to no explicit proxy");
    let (info, server) = start_server(db.clone()).await;
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
    let (info, server) = start_server(db.clone()).await;
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
    let (info, server) = start_server(db.clone()).await;
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
        rec.last().unwrap().uri.contains("192.0.2.1"),
        "{}",
        rec.last().unwrap().uri
    );
    server.stop().await.expect("stop proxy server");

    // Restore global state for any follow-up tests in the same binary.
    let _ = http_client::apply_proxy(None);
    upstream_handle.abort();
    explicit_proxy_handle.abort();
}

/// Redirect contract 1: loopback -> loopback (same origin) stays direct, keeps
/// method/body (307) and does not touch the inherited proxy.
#[tokio::test]
async fn loopback_upstream_redirect_loopback_to_loopback_stays_direct() {
    let home = tempfile::tempdir().expect("temp home");
    std::env::set_var("CC_SWITCH_TEST_HOME", home.path());
    let proxy_rec: Arc<Mutex<Vec<RecReq>>> = Arc::new(Mutex::new(Vec::new()));
    let (proxy_addr, proxy_handle) = spawn_mock(proxy_rec.clone(), MockReply::Json200).await;
    let _env = EnvProxyGuard::set(&format!("http://127.0.0.1:{}", proxy_addr.port()));

    let a_rec: Arc<Mutex<Vec<RecReq>>> = Arc::new(Mutex::new(Vec::new()));
    let a_queue = Arc::new(Mutex::new(VecDeque::new()));
    let (a_addr, a_handle) = spawn_mock_queue(a_rec.clone(), a_queue.clone()).await;
    // Location is filled in below once we know our own port.
    a_queue.lock().unwrap().push_back(MockReply::Status(
        307,
        Some(format!("http://127.0.0.1:{}/step2", a_addr.port())),
    ));
    a_queue.lock().unwrap().push_back(MockReply::Json200);

    let db = Arc::new(Database::init().expect("init isolated database"));
    seed_provider(&db, format!("http://127.0.0.1:{}/v1", a_addr.port()));

    let _ = http_client::init(None);
    http_client::apply_proxy(None).expect("no explicit proxy");
    let (info, server) = start_server(db.clone()).await;
    let resp = send_messages(info.port).await;

    assert_eq!(
        resp.status(),
        200,
        "loopback->loopback redirect must succeed"
    );
    let a = a_rec.lock().unwrap().clone();
    assert_eq!(
        a.len(),
        2,
        "both hops must reach the loopback mock directly"
    );
    assert_eq!(a[0].method, "POST");
    assert!(
        a[0].has_credential(),
        "loopback hop must carry upstream auth"
    );
    assert!(a[0].body_text().contains("ping"));
    assert!(a[1].uri.contains("/step2"), "{}", a[1].uri);
    // 307 keeps method and body.
    assert_eq!(a[1].method, "POST", "307 must keep the method");
    assert!(a[1].body_text().contains("ping"), "307 must keep the body");
    // Same origin (host+port): credentials must NOT be stripped.
    assert!(
        a[1].has_credential(),
        "same-origin redirect must keep credentials"
    );
    assert_eq!(
        proxy_rec.lock().unwrap().len(),
        0,
        "no hop may touch the inherited proxy for loopback targets"
    );

    server.stop().await.expect("stop proxy server");
    a_handle.abort();
    proxy_handle.abort();
}

/// Redirect contract 2: loopback -> external re-selects transport for the
/// external hop (inherited proxy must be used, not the no-proxy client).
#[tokio::test]
async fn loopback_upstream_redirect_loopback_to_external_reselects_transport() {
    let home = tempfile::tempdir().expect("temp home");
    std::env::set_var("CC_SWITCH_TEST_HOME", home.path());
    let proxy_rec: Arc<Mutex<Vec<RecReq>>> = Arc::new(Mutex::new(Vec::new()));
    let (proxy_addr, proxy_handle) = spawn_mock(proxy_rec.clone(), MockReply::Json200).await;
    let _env = EnvProxyGuard::set(&format!("http://127.0.0.1:{}", proxy_addr.port()));

    let a_rec: Arc<Mutex<Vec<RecReq>>> = Arc::new(Mutex::new(Vec::new()));
    let (a_addr, a_handle) = spawn_mock(
        a_rec.clone(),
        MockReply::Status(307, Some("http://192.0.2.1/step2".to_string())),
    )
    .await;

    let db = Arc::new(Database::init().expect("init isolated database"));
    seed_provider(&db, format!("http://127.0.0.1:{}/v1", a_addr.port()));

    let _ = http_client::init(None);
    http_client::apply_proxy(None).expect("no explicit proxy");
    let (info, server) = start_server(db.clone()).await;
    let resp = send_messages(info.port).await;

    assert_eq!(resp.status(), 200, "redirected external hop must succeed");
    assert_eq!(a_rec.lock().unwrap().len(), 1, "first hop stays direct");
    let p = proxy_rec.lock().unwrap().clone();
    assert_eq!(
        p.len(),
        1,
        "external redirect hop must go through the inherited proxy"
    );
    assert!(p[0].uri.contains("192.0.2.1/step2"), "{}", p[0].uri);
    assert_eq!(p[0].method, "POST", "307 must keep the method");
    assert!(p[0].body_text().contains("ping"), "307 must keep the body");
    assert!(
        p[0].authorization.is_none(),
        "cross-host redirect must strip the Authorization header"
    );

    server.stop().await.expect("stop proxy server");
    a_handle.abort();
    proxy_handle.abort();
}

/// Redirect contract 3: external -> loopback re-selects transport for the
/// loopback hop (direct), with 302 POST->GET method/body semantics.
#[tokio::test]
async fn loopback_upstream_redirect_external_to_loopback_reselects_transport() {
    let home = tempfile::tempdir().expect("temp home");
    std::env::set_var("CC_SWITCH_TEST_HOME", home.path());

    let b_rec: Arc<Mutex<Vec<RecReq>>> = Arc::new(Mutex::new(Vec::new()));
    let (b_addr, b_handle) = spawn_mock(b_rec.clone(), MockReply::Json200).await;

    let proxy_rec: Arc<Mutex<Vec<RecReq>>> = Arc::new(Mutex::new(Vec::new()));
    let proxy_queue = Arc::new(Mutex::new(VecDeque::new()));
    proxy_queue.lock().unwrap().push_back(MockReply::Status(
        302,
        Some(format!("http://127.0.0.1:{}/final", b_addr.port())),
    ));
    let (proxy_addr, proxy_handle) = spawn_mock_queue(proxy_rec.clone(), proxy_queue.clone()).await;
    let _env = EnvProxyGuard::set(&format!("http://127.0.0.1:{}", proxy_addr.port()));

    let db = Arc::new(Database::init().expect("init isolated database"));
    seed_provider(&db, "http://192.0.2.1/start".to_string());

    let _ = http_client::init(None);
    http_client::apply_proxy(None).expect("no explicit proxy");
    let (info, server) = start_server(db.clone()).await;
    let resp = send_messages(info.port).await;

    assert_eq!(resp.status(), 200, "redirected loopback hop must succeed");
    let p = proxy_rec.lock().unwrap().clone();
    assert_eq!(
        p.len(),
        1,
        "only the external first hop may go through the proxy"
    );
    assert!(p[0].uri.contains("192.0.2.1/start"), "{}", p[0].uri);
    let b = b_rec.lock().unwrap().clone();
    assert_eq!(
        b.len(),
        1,
        "loopback redirect hop must be reached directly, not via the proxy"
    );
    assert!(b[0].uri.contains("/final"), "{}", b[0].uri);
    // 302 with POST: method becomes GET and the body is dropped.
    assert_eq!(b[0].method, "GET", "302 must switch POST to GET");
    assert!(b[0].body.is_empty(), "302 must drop the request body");

    server.stop().await.expect("stop proxy server");
    b_handle.abort();
    proxy_handle.abort();
}

/// Redirect contract 4: with an explicit proxy configured, every hop keeps
/// using it (explicit precedence is not overridden by the loopback rule).
#[tokio::test]
async fn loopback_upstream_redirect_explicit_proxy_precedence_preserved() {
    let home = tempfile::tempdir().expect("temp home");
    std::env::set_var("CC_SWITCH_TEST_HOME", home.path());

    let env_rec: Arc<Mutex<Vec<RecReq>>> = Arc::new(Mutex::new(Vec::new()));
    let (env_addr, env_handle) = spawn_mock(env_rec.clone(), MockReply::Json200).await;
    let _env = EnvProxyGuard::set(&format!("http://127.0.0.1:{}", env_addr.port()));

    let explicit_rec: Arc<Mutex<Vec<RecReq>>> = Arc::new(Mutex::new(Vec::new()));
    let explicit_queue = Arc::new(Mutex::new(VecDeque::new()));
    explicit_queue.lock().unwrap().push_back(MockReply::Status(
        302,
        Some("http://192.0.2.1/after".to_string()),
    ));
    explicit_queue.lock().unwrap().push_back(MockReply::Json200);
    let (explicit_addr, explicit_handle) =
        spawn_mock_queue(explicit_rec.clone(), explicit_queue.clone()).await;

    let a_rec: Arc<Mutex<Vec<RecReq>>> = Arc::new(Mutex::new(Vec::new()));
    let (a_addr, a_handle) = spawn_mock(a_rec.clone(), MockReply::Json200).await;

    let db = Arc::new(Database::init().expect("init isolated database"));
    seed_provider(&db, format!("http://127.0.0.1:{}/v1", a_addr.port()));

    let _ = http_client::init(None);
    let explicit_url = format!("http://127.0.0.1:{}", explicit_addr.port());
    http_client::apply_proxy(Some(explicit_url.as_str())).expect("apply explicit proxy");
    let (info, server) = start_server(db.clone()).await;
    let resp = send_messages(info.port).await;

    assert_eq!(
        resp.status(),
        200,
        "explicit-proxied redirect chain must succeed"
    );
    let e = explicit_rec.lock().unwrap().clone();
    assert_eq!(e.len(), 2, "both hops must go through the explicit proxy");
    assert!(
        e[0].uri.contains("127.0.0.1") && e[0].uri.contains("/chat/completions"),
        "first hop (loopback) must go through the explicit proxy: {}",
        e[0].uri
    );
    assert!(e[1].uri.contains("192.0.2.1/after"), "{}", e[1].uri);
    assert_eq!(
        a_rec.lock().unwrap().len(),
        0,
        "loopback upstream must not be reached directly while an explicit proxy is set"
    );
    assert_eq!(
        env_rec.lock().unwrap().len(),
        0,
        "explicit proxy must take precedence over the inherited/env proxy"
    );

    server.stop().await.expect("stop proxy server");
    a_handle.abort();
    explicit_handle.abort();
    env_handle.abort();
}

/// Redirect contract 5: 301/302/303 vs 307/308 method/body semantics and
/// cross-host sensitive-header stripping for redirected hops.
#[tokio::test]
async fn loopback_upstream_redirect_method_and_body_semantics() {
    let home = tempfile::tempdir().expect("temp home");
    std::env::set_var("CC_SWITCH_TEST_HOME", home.path());
    let proxy_rec: Arc<Mutex<Vec<RecReq>>> = Arc::new(Mutex::new(Vec::new()));
    let (proxy_addr, proxy_handle) = spawn_mock(proxy_rec.clone(), MockReply::Json200).await;
    let _env = EnvProxyGuard::set(&format!("http://127.0.0.1:{}", proxy_addr.port()));

    let a_rec: Arc<Mutex<Vec<RecReq>>> = Arc::new(Mutex::new(Vec::new()));
    let a_queue = Arc::new(Mutex::new(VecDeque::new()));
    let (a_addr, a_handle) = spawn_mock_queue(a_rec.clone(), a_queue.clone()).await;
    for (status, path) in [
        (307u16, "r307"),
        (308, "r308"),
        (303, "r303"),
        (301, "r301"),
        (302, "r302"),
    ] {
        a_queue.lock().unwrap().push_back(MockReply::Status(
            status,
            Some(format!("http://192.0.2.1/{path}")),
        ));
    }

    let db = Arc::new(Database::init().expect("init isolated database"));
    seed_provider(&db, format!("http://127.0.0.1:{}/v1", a_addr.port()));

    let _ = http_client::init(None);
    http_client::apply_proxy(None).expect("no explicit proxy");
    let (info, server) = start_server(db.clone()).await;

    for _ in 0..5 {
        let resp = send_messages(info.port).await;
        assert_eq!(resp.status(), 200, "redirect case must succeed");
    }

    let a = a_rec.lock().unwrap().clone();
    assert_eq!(a.len(), 5);
    for req in &a {
        assert!(req.has_credential(), "loopback hop must carry credentials");
    }
    let p = proxy_rec.lock().unwrap().clone();
    assert_eq!(
        p.len(),
        5,
        "every cross-host hop must use the inherited proxy"
    );
    // 307 / 308 keep method + body.
    for (idx, path) in [(0usize, "r307"), (1, "r308")] {
        assert!(p[idx].uri.contains(path), "{}", p[idx].uri);
        assert_eq!(p[idx].method, "POST", "307/308 must keep the method");
        assert!(
            p[idx].body_text().contains("ping"),
            "307/308 must keep the body"
        );
    }
    // 303 / 301 / 302 switch POST -> GET and drop the body.
    for (idx, path) in [(2usize, "r303"), (3, "r301"), (4, "r302")] {
        assert!(p[idx].uri.contains(path), "{}", p[idx].uri);
        assert_eq!(p[idx].method, "GET", "{path} must switch POST to GET");
        assert!(p[idx].body.is_empty(), "{path} must drop the request body");
    }
    // Cross-host hops must not carry the Authorization header.
    for req in &p {
        assert!(
            req.authorization.is_none(),
            "cross-host redirect must strip Authorization"
        );
    }

    server.stop().await.expect("stop proxy server");
    a_handle.abort();
    proxy_handle.abort();
}

/// Redirect contract 6 (safety net): a redirect loop across transports stays
/// bounded and never hangs.
#[tokio::test]
async fn loopback_upstream_redirect_chain_is_bounded() {
    let home = tempfile::tempdir().expect("temp home");
    std::env::set_var("CC_SWITCH_TEST_HOME", home.path());
    let proxy_rec: Arc<Mutex<Vec<RecReq>>> = Arc::new(Mutex::new(Vec::new()));
    let proxy_queue = Arc::new(Mutex::new(VecDeque::new()));
    for i in 0..12 {
        proxy_queue.lock().unwrap().push_back(MockReply::Status(
            307,
            Some(format!("http://192.0.2.1/loop{i}")),
        ));
    }
    let (proxy_addr, proxy_handle) = spawn_mock_queue(proxy_rec.clone(), proxy_queue.clone()).await;
    let _env = EnvProxyGuard::set(&format!("http://127.0.0.1:{}", proxy_addr.port()));

    let a_rec: Arc<Mutex<Vec<RecReq>>> = Arc::new(Mutex::new(Vec::new()));
    let (a_addr, a_handle) = spawn_mock(
        a_rec.clone(),
        MockReply::Status(307, Some("http://192.0.2.1/orig".to_string())),
    )
    .await;

    let db = Arc::new(Database::init().expect("init isolated database"));
    seed_provider(&db, format!("http://127.0.0.1:{}/v1", a_addr.port()));

    let _ = http_client::init(None);
    http_client::apply_proxy(None).expect("no explicit proxy");
    let (info, server) = start_server(db.clone()).await;

    let started = std::time::Instant::now();
    let resp = send_messages(info.port).await;
    let elapsed = started.elapsed();

    assert!(
        elapsed < Duration::from_secs(30),
        "redirect chain must terminate quickly, took {elapsed:?}"
    );
    assert_ne!(
        resp.status(),
        200,
        "a redirect loop must not report success"
    );
    assert_eq!(a_rec.lock().unwrap().len(), 1, "first hop once");
    assert!(
        proxy_rec.lock().unwrap().len() <= 12,
        "redirect hops must stay bounded"
    );

    server.stop().await.expect("stop proxy server");
    a_handle.abort();
    proxy_handle.abort();
}

/// Atomicity contract 1 (deterministic): a selection paused between the proxy
/// metadata read and the transport decision must not pair a stale metadata
/// snapshot with a client from a newer configuration. The decision must be
/// consistent with the *published snapshot* it used (loopback direct iff that
/// snapshot had no explicit proxy).
#[tokio::test]
async fn loopback_upstream_atomic_snapshot_deterministic_pause() {
    use crate::proxy::test_hooks;

    let _ = http_client::init(None);
    test_hooks::clear_publish_log();
    test_hooks::set_gate(None);

    // Direction 1: None -> explicit during the paused selection.
    http_client::apply_proxy(None).expect("baseline None");
    let gate = test_hooks::new_gate();
    gate.arm();
    test_hooks::set_gate(Some(gate.clone()));
    let handle =
        tokio::task::spawn_blocking(|| http_client::select_for_test("http://127.0.0.1:8080/v1"));
    assert!(
        gate.wait_entered(Duration::from_secs(10)),
        "selection must reach the pause point"
    );
    http_client::apply_proxy(Some("http://127.0.0.1:59888")).expect("apply explicit proxy");
    gate.release();
    test_hooks::set_gate(None);
    let (direct, generation) = handle.await.expect("selection task");
    let published = test_hooks::published_explicit_proxy(generation)
        .expect("generation must be a published route-state snapshot");
    assert_eq!(
        direct,
        published.is_none(),
        "None->explicit: selection must be consistent with its snapshot generation \
         (loopback direct iff that snapshot had no explicit proxy)"
    );

    // Direction 2: explicit -> None during the paused selection.
    http_client::apply_proxy(Some("http://127.0.0.1:59888")).expect("baseline explicit");
    let gate = test_hooks::new_gate();
    gate.arm();
    test_hooks::set_gate(Some(gate.clone()));
    let handle =
        tokio::task::spawn_blocking(|| http_client::select_for_test("http://127.0.0.1:8080/v1"));
    assert!(
        gate.wait_entered(Duration::from_secs(10)),
        "selection must reach the pause point"
    );
    http_client::apply_proxy(None).expect("switch back to None");
    gate.release();
    test_hooks::set_gate(None);
    let (direct, generation) = handle.await.expect("selection task");
    let published = test_hooks::published_explicit_proxy(generation)
        .expect("generation must be a published route-state snapshot");
    assert_eq!(
        direct,
        published.is_none(),
        "explicit->None: selection must be consistent with its snapshot generation"
    );

    let _ = http_client::apply_proxy(None);
}

/// Atomicity contract 2 (stress): concurrent `apply_proxy` writers and
/// selection readers never produce a decision inconsistent with the published
/// snapshot generation (no torn metadata/client pair; a loopback request never
/// silently bypasses an already-effective explicit proxy).
#[tokio::test]
async fn loopback_upstream_atomic_snapshot_concurrent_stress() {
    use crate::proxy::test_hooks;
    use std::sync::atomic::{AtomicUsize, Ordering};

    let _ = http_client::init(None);
    test_hooks::clear_publish_log();
    // Re-publish once after clearing so the currently-live generation is present
    // in the log; otherwise readers racing before the first writer publish would
    // look up a generation whose record was just cleared (test-side artifact).
    http_client::apply_proxy(None).expect("re-publish current route state");
    test_hooks::set_gate(None); // no pauses in the stress test

    let violations = Arc::new(AtomicUsize::new(0));
    let details = Arc::new(Mutex::new(Vec::<String>::new()));
    let mut handles = Vec::new();

    for w in 0..3 {
        handles.push(std::thread::spawn(move || {
            for i in 0..30 {
                let url = if (w + i) % 2 == 0 {
                    Some("http://127.0.0.1:59888")
                } else {
                    None
                };
                let _ = http_client::apply_proxy(url);
            }
        }));
    }

    for _ in 0..4 {
        let violations = violations.clone();
        let details = details.clone();
        handles.push(std::thread::spawn(move || {
            for _ in 0..400 {
                let (direct, generation) = http_client::select_for_test("http://127.0.0.1:8080/v1");
                let published = test_hooks::published_explicit_proxy(generation);
                let consistent = match &published {
                    Some(published) => direct == published.is_none(),
                    None => false,
                };
                if !consistent {
                    let n = violations.fetch_add(1, Ordering::SeqCst);
                    if let Ok(mut d) = details.lock() {
                        if d.len() < 8 {
                            d.push(format!(
                                "gen={generation} direct={direct} published={published:?}"
                            ));
                        }
                    }
                    let _ = n;
                }
            }
        }));
    }

    for handle in handles {
        handle.join().expect("worker thread");
    }
    let violation_count = violations.load(Ordering::SeqCst);
    assert_eq!(
        violation_count,
        0,
        "no reader may observe a (decision, generation) pair inconsistent with the published snapshot; details: {:?}",
        details.lock().map(|d| d.clone()).unwrap_or_default()
    );

    let _ = http_client::apply_proxy(None);
}
