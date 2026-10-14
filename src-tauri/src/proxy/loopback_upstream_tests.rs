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
    referer: Option<String>,
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
    let referer = get(&axum::http::header::REFERER);
    let x_api_key = get(&axum::http::HeaderName::from_static("x-api-key"));
    let body = axum::body::to_bytes(body, 10 * 1024 * 1024)
        .await
        .map(|b| b.to_vec())
        .unwrap_or_default();
    record.lock().unwrap().push(RecReq {
        method,
        uri,
        authorization,
        referer,
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

/// Mock server that pops its reply from a queue per request, with a fixed delay
/// applied before each reply (used to probe request-level timeout deadlines).
async fn spawn_mock_queue_delayed(
    record: Arc<Mutex<Vec<RecReq>>>,
    queue: Arc<Mutex<VecDeque<MockReply>>>,
    delay: Duration,
) -> (std::net::SocketAddr, tokio::task::JoinHandle<()>) {
    let app = axum::Router::new().fallback(move |req: axum::extract::Request| {
        let record = record.clone();
        let queue = queue.clone();
        async move {
            record_request(req, record).await;
            tokio::time::sleep(delay).await;
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
        .expect("bind delayed mock queue server");
    let addr = listener
        .local_addr()
        .expect("delayed mock queue server addr");
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
    seed_provider_with_format(db, base_url, "openai_chat");
}

fn seed_provider_with_format(db: &Database, base_url: String, api_format: &str) {
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
        serde_json::from_value::<ProviderMeta>(json!({ "apiFormat": api_format }))
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

// ===========================================================================
// R2: one logical redirect budget / request-level deadline / Referer contract
// ===========================================================================

/// R2 budget contract 1: exactly 10 redirects are followed (11 HTTP requests in
/// total: 1 initial + 10 followed), then the final 200 is returned.
#[tokio::test]
async fn r2_redirect_budget_allows_exactly_ten_follows() {
    let home = tempfile::tempdir().expect("temp home");
    std::env::set_var("CC_SWITCH_TEST_HOME", home.path());

    let a_rec: Arc<Mutex<Vec<RecReq>>> = Arc::new(Mutex::new(Vec::new()));
    let a_queue = Arc::new(Mutex::new(VecDeque::new()));
    let (a_addr, a_handle) = spawn_mock_queue(a_rec.clone(), a_queue.clone()).await;
    for i in 0..10 {
        a_queue.lock().unwrap().push_back(MockReply::Status(
            307,
            Some(format!("http://127.0.0.1:{}/r{i}", a_addr.port())),
        ));
    }
    a_queue.lock().unwrap().push_back(MockReply::Json200);

    let db = Arc::new(Database::init().expect("init isolated database"));
    seed_provider(&db, format!("http://127.0.0.1:{}/v1", a_addr.port()));
    let _ = http_client::init(None);
    http_client::apply_proxy(None).expect("no explicit proxy");
    let (info, server) = start_server(db.clone()).await;
    let resp = send_messages(info.port).await;

    assert_eq!(resp.status(), 200, "a 10-redirect chain must succeed");
    assert_eq!(
        a_rec.lock().unwrap().len(),
        11,
        "exactly 10 redirects followed => 11 HTTP requests total"
    );
    server.stop().await.expect("stop proxy server");
    a_handle.abort();
}

/// R2 budget contract 2: the 11th redirect is rejected; the chain stops after
/// exactly 11 HTTP requests and never sends a 12th.
#[tokio::test]
async fn r2_redirect_budget_rejects_eleventh() {
    let home = tempfile::tempdir().expect("temp home");
    std::env::set_var("CC_SWITCH_TEST_HOME", home.path());

    let a_rec: Arc<Mutex<Vec<RecReq>>> = Arc::new(Mutex::new(Vec::new()));
    let a_queue = Arc::new(Mutex::new(VecDeque::new()));
    let (a_addr, a_handle) = spawn_mock_queue(a_rec.clone(), a_queue.clone()).await;
    for i in 0..12 {
        a_queue.lock().unwrap().push_back(MockReply::Status(
            307,
            Some(format!("http://127.0.0.1:{}/r{i}", a_addr.port())),
        ));
    }

    let db = Arc::new(Database::init().expect("init isolated database"));
    seed_provider(&db, format!("http://127.0.0.1:{}/v1", a_addr.port()));
    let _ = http_client::init(None);
    http_client::apply_proxy(None).expect("no explicit proxy");
    let (info, server) = start_server(db.clone()).await;
    let resp = send_messages(info.port).await;

    let status = resp.status();
    assert!(
        !status.is_success() && !status.is_redirection(),
        "11th redirect must be rejected with a proxy error, got {status}"
    );
    assert_eq!(
        a_rec.lock().unwrap().len(),
        11,
        "the logical budget must stop after 1 initial + 10 followed requests"
    );
    let body = resp.text().await.unwrap_or_default();
    assert!(
        body.contains("重定向") || body.to_lowercase().contains("redirect"),
        "rejection must be reported as a redirect-limit error: {body}"
    );
    server.stop().await.expect("stop proxy server");
    a_handle.abort();
}

/// R2 budget contract 3: the budget is shared across transport classes —
/// alternating loopback <-> external hops must NOT get a fresh budget per hop.
/// 12 alternating redirects stop at 1 initial + 10 followed requests
/// (loopback mock A = 6, env-proxy mock P = 5).
#[tokio::test]
async fn r2_redirect_budget_shared_across_classes() {
    let home = tempfile::tempdir().expect("temp home");
    std::env::set_var("CC_SWITCH_TEST_HOME", home.path());

    let a_rec: Arc<Mutex<Vec<RecReq>>> = Arc::new(Mutex::new(Vec::new()));
    let a_queue = Arc::new(Mutex::new(VecDeque::new()));
    let (a_addr, a_handle) = spawn_mock_queue(a_rec.clone(), a_queue.clone()).await;

    let proxy_rec: Arc<Mutex<Vec<RecReq>>> = Arc::new(Mutex::new(Vec::new()));
    let proxy_queue = Arc::new(Mutex::new(VecDeque::new()));
    let (proxy_addr, proxy_handle) = spawn_mock_queue(proxy_rec.clone(), proxy_queue.clone()).await;
    let _env = EnvProxyGuard::set(&format!("http://127.0.0.1:{}", proxy_addr.port()));

    // Alternating chain: A -> external (via env proxy) -> A -> external -> ...
    for _ in 0..6 {
        a_queue.lock().unwrap().push_back(MockReply::Status(
            307,
            Some("http://192.0.2.1/ext".to_string()),
        ));
    }
    for _ in 0..5 {
        proxy_queue.lock().unwrap().push_back(MockReply::Status(
            307,
            Some(format!("http://127.0.0.1:{}/back", a_addr.port())),
        ));
    }

    let db = Arc::new(Database::init().expect("init isolated database"));
    seed_provider(&db, format!("http://127.0.0.1:{}/v1", a_addr.port()));
    let _ = http_client::init(None);
    http_client::apply_proxy(None).expect("no explicit proxy");
    let (info, server) = start_server(db.clone()).await;
    let resp = send_messages(info.port).await;

    let status = resp.status();
    assert!(
        !status.is_success() && !status.is_redirection(),
        "the shared budget must reject the 11th redirect, got {status}"
    );
    let a = a_rec.lock().unwrap().len();
    let p = proxy_rec.lock().unwrap().len();
    assert_eq!(
        (a, p),
        (6, 5),
        "loopback/external hops must share ONE budget: expected A=6, P=5 (11 total)"
    );
    server.stop().await.expect("stop proxy server");
    a_handle.abort();
    proxy_handle.abort();
}

/// R2 timeout contract: the redirect chain shares ONE request-level deadline.
/// A 3s non-streaming timeout against a mock that delays every reply by 1.5s
/// must stop near the deadline (<= 3 requests), not walk the whole chain.
#[tokio::test]
async fn r2_redirect_timeout_is_request_level_deadline() {
    let home = tempfile::tempdir().expect("temp home");
    std::env::set_var("CC_SWITCH_TEST_HOME", home.path());

    let a_rec: Arc<Mutex<Vec<RecReq>>> = Arc::new(Mutex::new(Vec::new()));
    let a_queue = Arc::new(Mutex::new(VecDeque::new()));
    let (a_addr, a_handle) =
        spawn_mock_queue_delayed(a_rec.clone(), a_queue.clone(), Duration::from_millis(1500)).await;
    for i in 0..15 {
        a_queue.lock().unwrap().push_back(MockReply::Status(
            307,
            Some(format!("http://127.0.0.1:{}/slow{i}", a_addr.port())),
        ));
    }

    let db = Arc::new(Database::init().expect("init isolated database"));
    seed_provider(&db, format!("http://127.0.0.1:{}/v1", a_addr.port()));
    // Timeouts are loaded from the per-app DB row and are bypassed while
    // auto-failover is disabled: enable the 3s non-streaming budget in the row.
    let row = db
        .get_proxy_config_for_app("claude")
        .await
        .expect("read proxy config row");
    let row = crate::proxy::types::AppProxyConfig {
        auto_failover_enabled: true,
        non_streaming_timeout: 3,
        ..row
    };
    db.update_proxy_config_for_app(row)
        .await
        .expect("update proxy config row");
    let _ = http_client::init(None);
    http_client::apply_proxy(None).expect("no explicit proxy");
    let config = ProxyConfig {
        non_streaming_timeout: 3,
        ..fast_config()
    };
    let server = ProxyServer::new(config, db.clone(), None);
    let info = server.start().await.expect("start proxy server");

    let started = std::time::Instant::now();
    let resp = send_messages(info.port).await;
    let elapsed = started.elapsed();

    assert!(!resp.status().is_success(), "the slow chain must time out");
    assert!(
        elapsed < Duration::from_secs(9),
        "one logical deadline must cap total wall time (no per-hop reset), took {elapsed:?}"
    );
    let n = a_rec.lock().unwrap().len();
    assert!(
        n <= 3,
        "the deadline must stop the chain near 3s (<= 3 delayed requests), made {n}"
    );
    server.stop().await.expect("stop proxy server");
    a_handle.abort();
}

/// R2 Referer contract (cross-class hop): the manually resumed hop must carry
/// `Referer` = the previous hop's URL, mirroring reqwest's automatic behavior.
#[tokio::test]
async fn r2_referer_contract_cross_class_hop() {
    let home = tempfile::tempdir().expect("temp home");
    std::env::set_var("CC_SWITCH_TEST_HOME", home.path());

    let a_rec: Arc<Mutex<Vec<RecReq>>> = Arc::new(Mutex::new(Vec::new()));
    let (a_addr, a_handle) = spawn_mock(
        a_rec.clone(),
        MockReply::Status(307, Some("http://192.0.2.1/next".to_string())),
    )
    .await;

    let proxy_rec: Arc<Mutex<Vec<RecReq>>> = Arc::new(Mutex::new(Vec::new()));
    let (proxy_addr, proxy_handle) = spawn_mock(proxy_rec.clone(), MockReply::Json200).await;
    let _env = EnvProxyGuard::set(&format!("http://127.0.0.1:{}", proxy_addr.port()));

    let db = Arc::new(Database::init().expect("init isolated database"));
    seed_provider(&db, format!("http://127.0.0.1:{}/v1", a_addr.port()));
    let _ = http_client::init(None);
    http_client::apply_proxy(None).expect("no explicit proxy");
    let (info, server) = start_server(db.clone()).await;
    let resp = send_messages(info.port).await;

    assert_eq!(resp.status(), 200, "cross-class redirect must succeed");
    let p = proxy_rec.lock().unwrap().clone();
    assert_eq!(p.len(), 1, "second hop must go through the inherited proxy");
    assert_eq!(
        p[0].referer.as_deref(),
        Some(format!("http://127.0.0.1:{}/v1/chat/completions", a_addr.port()).as_str()),
        "manually resumed hop must carry Referer = previous hop URL (reqwest parity)"
    );
    server.stop().await.expect("stop proxy server");
    a_handle.abort();
    proxy_handle.abort();
}

/// R2 Referer contract (same-class hop): reqwest-parity behavior already
/// applies within one send and must be preserved.
#[tokio::test]
async fn r2_referer_contract_same_class_hop() {
    let home = tempfile::tempdir().expect("temp home");
    std::env::set_var("CC_SWITCH_TEST_HOME", home.path());

    let a_rec: Arc<Mutex<Vec<RecReq>>> = Arc::new(Mutex::new(Vec::new()));
    let a_queue = Arc::new(Mutex::new(VecDeque::new()));
    let (a_addr, a_handle) = spawn_mock_queue(a_rec.clone(), a_queue.clone()).await;
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

    assert_eq!(resp.status(), 200, "same-class redirect must succeed");
    let a = a_rec.lock().unwrap().clone();
    assert_eq!(a.len(), 2);
    assert_eq!(
        a[1].referer.as_deref(),
        Some(format!("http://127.0.0.1:{}/v1/chat/completions", a_addr.port()).as_str()),
        "same-class hop must carry Referer = previous hop URL"
    );
    server.stop().await.expect("stop proxy server");
    a_handle.abort();
}

// ===========================================================================
// R2: forwarder-level route-decision atomicity (ONE HOP = ONE SNAPSHOT)
// ===========================================================================

/// F-1 (deterministic barrier, None -> explicit HTTP): a hop paused between
/// route resolution and send must consume the DECISION it resolved — a hot
/// `apply_proxy` during the pause must not retroactively re-route that hop.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn r2_route_decision_none_to_http_deterministic() {
    use crate::proxy::test_hooks;
    let home = tempfile::tempdir().expect("temp home");
    std::env::set_var("CC_SWITCH_TEST_HOME", home.path());

    let env_rec: Arc<Mutex<Vec<RecReq>>> = Arc::new(Mutex::new(Vec::new()));
    let (env_addr, env_handle) = spawn_mock(env_rec.clone(), MockReply::Json200).await;
    let _env = EnvProxyGuard::set(&format!("http://127.0.0.1:{}", env_addr.port()));

    let a_rec: Arc<Mutex<Vec<RecReq>>> = Arc::new(Mutex::new(Vec::new()));
    let (a_addr, a_handle) = spawn_mock(a_rec.clone(), MockReply::Json200).await;

    let q_rec: Arc<Mutex<Vec<RecReq>>> = Arc::new(Mutex::new(Vec::new()));
    let (q_addr, q_handle) = spawn_mock(q_rec.clone(), MockReply::Json200).await;

    let db = Arc::new(Database::init().expect("init isolated database"));
    seed_provider(&db, format!("http://127.0.0.1:{}/v1", a_addr.port()));
    let _ = http_client::init(None);
    http_client::apply_proxy(None).expect("no explicit proxy");
    let (info, server) = start_server(db.clone()).await;

    let gate = test_hooks::new_gate();
    gate.arm();
    test_hooks::set_gate(Some(gate.clone()));
    let handle = tokio::spawn(async move { send_messages(info.port).await });
    assert!(
        gate.wait_entered(Duration::from_secs(10)),
        "forwarder must reach the route-resolution pause point"
    );
    http_client::apply_proxy(Some(&format!("http://127.0.0.1:{}", q_addr.port())))
        .expect("apply explicit proxy mid-hop");
    gate.release();
    test_hooks::set_gate(None);
    let resp = handle.await.expect("request task");

    assert_eq!(
        resp.status(),
        200,
        "paused hop must complete via its own decision"
    );
    assert_eq!(
        a_rec.lock().unwrap().len(),
        1,
        "hop must use the pre-switch decision (loopback direct), not a re-read"
    );
    assert_eq!(
        q_rec.lock().unwrap().len(),
        0,
        "mid-hop explicit proxy must not serve this hop (no second read)"
    );
    assert_eq!(
        env_rec.lock().unwrap().len(),
        0,
        "inherited proxy not involved"
    );

    server.stop().await.expect("stop proxy server");
    a_handle.abort();
    q_handle.abort();
    env_handle.abort();
    let _ = http_client::apply_proxy(None);
}

/// F-2 (deterministic barrier, explicit -> None): with an explicit proxy
/// configured, a hop paused at route resolution must still use that explicit
/// proxy even if `apply_proxy(None)` lands during the pause (an explicit-proxy
/// snapshot effective for the hop is never bypassed).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn r2_route_decision_explicit_to_none_deterministic() {
    use crate::proxy::test_hooks;
    let home = tempfile::tempdir().expect("temp home");
    std::env::set_var("CC_SWITCH_TEST_HOME", home.path());

    let a_rec: Arc<Mutex<Vec<RecReq>>> = Arc::new(Mutex::new(Vec::new()));
    let (a_addr, a_handle) = spawn_mock(a_rec.clone(), MockReply::Json200).await;

    let q_rec: Arc<Mutex<Vec<RecReq>>> = Arc::new(Mutex::new(Vec::new()));
    let (q_addr, q_handle) = spawn_mock(q_rec.clone(), MockReply::Json200).await;

    let db = Arc::new(Database::init().expect("init isolated database"));
    seed_provider(&db, format!("http://127.0.0.1:{}/v1", a_addr.port()));
    let _ = http_client::init(None);
    http_client::apply_proxy(Some(&format!("http://127.0.0.1:{}", q_addr.port())))
        .expect("explicit proxy");
    let (info, server) = start_server(db.clone()).await;

    let gate = test_hooks::new_gate();
    gate.arm();
    test_hooks::set_gate(Some(gate.clone()));
    let handle = tokio::spawn(async move { send_messages(info.port).await });
    assert!(
        gate.wait_entered(Duration::from_secs(10)),
        "forwarder must reach the route-resolution pause point"
    );
    http_client::apply_proxy(None).expect("remove explicit proxy mid-hop");
    gate.release();
    test_hooks::set_gate(None);
    let resp = handle.await.expect("request task");

    assert_eq!(
        resp.status(),
        200,
        "paused hop must complete via its own decision"
    );
    assert_eq!(
        q_rec.lock().unwrap().len(),
        1,
        "the hop must honor the explicit-proxy snapshot effective at its resolution"
    );
    assert_eq!(
        a_rec.lock().unwrap().len(),
        0,
        "no re-read => loopback must not be forced direct after the switch"
    );

    server.stop().await.expect("stop proxy server");
    a_handle.abort();
    q_handle.abort();
    let _ = http_client::apply_proxy(None);
}

/// F-3 (deterministic barrier, SOCKS5): a hop resolved while an explicit SOCKS5
/// proxy is configured must consume the SOCKS5 decision (and fail against the
/// dead port) instead of silently falling back to the direct path after a
/// concurrent `apply_proxy(None)`.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn r2_route_decision_socks5_never_falls_back_deterministic() {
    use crate::proxy::test_hooks;
    let home = tempfile::tempdir().expect("temp home");
    std::env::set_var("CC_SWITCH_TEST_HOME", home.path());

    let a_rec: Arc<Mutex<Vec<RecReq>>> = Arc::new(Mutex::new(Vec::new()));
    let (a_addr, a_handle) = spawn_mock(a_rec.clone(), MockReply::Json200).await;

    // Reserve a loopback port, then drop the listener: guaranteed-refused port.
    let dead_port = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").expect("bind temp port");
        l.local_addr().expect("addr").port()
    };

    let db = Arc::new(Database::init().expect("init isolated database"));
    seed_provider(&db, format!("http://127.0.0.1:{}/v1", a_addr.port()));
    let _ = http_client::init(None);
    http_client::apply_proxy(Some(&format!("socks5://127.0.0.1:{dead_port}")))
        .expect("socks5 explicit proxy");
    let (info, server) = start_server(db.clone()).await;

    let gate = test_hooks::new_gate();
    gate.arm();
    test_hooks::set_gate(Some(gate.clone()));
    let handle = tokio::spawn(async move { send_messages(info.port).await });
    assert!(
        gate.wait_entered(Duration::from_secs(10)),
        "forwarder must reach the route-resolution pause point"
    );
    http_client::apply_proxy(None).expect("remove explicit proxy mid-hop");
    gate.release();
    test_hooks::set_gate(None);
    let resp = handle.await.expect("request task");

    assert!(
        !resp.status().is_success(),
        "socks5-resolved hop must not silently succeed by falling back to direct"
    );
    assert_eq!(
        a_rec.lock().unwrap().len(),
        0,
        "the loopback mock must NOT be reached (no re-read to a direct decision)"
    );

    server.stop().await.expect("stop proxy server");
    a_handle.abort();
    let _ = http_client::apply_proxy(None);
}

/// F-4 (stress, supplementary): deliberate `apply_proxy` churn across
/// None / HTTP / SOCKS5 while readers resolve both loopback and external URLs.
/// Every recorded resolution must be consistent with the *published snapshot
/// generation* it consumed: `direct == (url_is_loopback && explicit.is_none())`.
#[tokio::test]
async fn r2_route_decision_stress_no_mixed_generations() {
    use crate::proxy::test_hooks;
    use std::sync::atomic::{AtomicBool, Ordering};

    let _ = http_client::init(None);
    test_hooks::clear_publish_log();
    test_hooks::clear_resolution_log();
    http_client::apply_proxy(None).expect("re-publish baseline snapshot");
    test_hooks::set_gate(None);

    let stop = Arc::new(AtomicBool::new(false));
    let mut handles = Vec::new();
    for w in 0..3usize {
        let stop = stop.clone();
        handles.push(std::thread::spawn(move || {
            let mut i = 0usize;
            while !stop.load(Ordering::SeqCst) {
                let url = match (w + i) % 3 {
                    0 => Some("http://127.0.0.1:59888".to_string()),
                    1 => Some("socks5://127.0.0.1:59889".to_string()),
                    _ => None,
                };
                let _ = http_client::apply_proxy(url.as_deref());
                i += 1;
            }
        }));
    }
    for r in 0..4usize {
        let stop = stop.clone();
        handles.push(std::thread::spawn(move || {
            while !stop.load(Ordering::SeqCst) {
                let url = if r % 2 == 0 {
                    "http://127.0.0.1:8080/v1"
                } else {
                    "http://192.0.2.1/v1"
                };
                let _ = http_client::resolve_route_for_url(url);
            }
        }));
    }
    std::thread::sleep(Duration::from_millis(500));
    stop.store(true, Ordering::SeqCst);
    for handle in handles {
        handle.join().expect("stress worker");
    }

    let resolutions = test_hooks::resolutions();
    assert!(
        resolutions.len() > 100,
        "stress must record resolutions, got {}",
        resolutions.len()
    );
    let mut violations = Vec::new();
    for (generation, direct, url_is_loopback) in &resolutions {
        if let Some(published) = test_hooks::published_explicit_proxy(*generation) {
            let expected = *url_is_loopback && published.is_none();
            if *direct != expected {
                violations.push(format!(
                    "gen={generation} direct={direct} url_loopback={url_is_loopback} published={published:?}"
                ));
            }
        }
        // generation 0 = fallback snapshot (not published); ignore.
    }
    assert!(
        violations.is_empty(),
        "route decisions must never mix generations under churn: {violations:?}"
    );
    let _ = http_client::apply_proxy(None);
}

// ===========================================================================
// R2: differential contract — forwarder redirect handling vs plain reqwest
// ===========================================================================

/// Drives identical redirect scenarios through (a) the forwarder path and
/// (b) a plain reqwest client using the *default* (upstream) redirect policy,
/// then compares per-hop method / body presence / authorization / referer
/// presence. This is the differential proof that the unified explicit state
/// machine matches reqwest's own redirect semantics (review finding MINOR-2
/// plus the method/body/header contract).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn r2_differential_forwarder_matches_plain_reqwest() {
    #[derive(Debug, PartialEq)]
    struct Hop {
        method: String,
        has_body: bool,
        has_auth: bool,
        has_referer: bool,
    }
    fn hops(rec: &Arc<Mutex<Vec<RecReq>>>) -> Vec<Hop> {
        rec.lock()
            .unwrap()
            .iter()
            .map(|r| Hop {
                method: r.method.clone(),
                has_body: !r.body.is_empty(),
                has_auth: r.authorization.is_some(),
                has_referer: r.referer.is_some(),
            })
            .collect()
    }

    for status in [302u16, 303, 307] {
        // ---- (a) plain reqwest baseline (default redirect policy) ----
        let plain_rec: Arc<Mutex<Vec<RecReq>>> = Arc::new(Mutex::new(Vec::new()));
        let plain_q = Arc::new(Mutex::new(VecDeque::new()));
        let (plain_addr, plain_handle) = spawn_mock_queue(plain_rec.clone(), plain_q.clone()).await;
        plain_q.lock().unwrap().push_back(MockReply::Status(
            status,
            Some(format!("http://127.0.0.1:{}/hop2", plain_addr.port())),
        ));
        plain_q.lock().unwrap().push_back(MockReply::Json200);
        let plain_client = reqwest::Client::builder()
            .no_proxy()
            .build()
            .expect("plain client");
        let resp = plain_client
            .request(
                reqwest::Method::POST,
                format!("http://127.0.0.1:{}/hop1", plain_addr.port()),
            )
            .header("authorization", "Bearer differential")
            .header("content-type", "application/json")
            .body("hello-body")
            .send()
            .await
            .expect("plain send");
        assert_eq!(resp.status(), 200);
        let plain_hops = hops(&plain_rec);
        plain_handle.abort();

        // ---- (b) forwarder path (isolated proxy) ----
        let a_rec: Arc<Mutex<Vec<RecReq>>> = Arc::new(Mutex::new(Vec::new()));
        let a_q = Arc::new(Mutex::new(VecDeque::new()));
        let (a_addr, a_handle) = spawn_mock_queue(a_rec.clone(), a_q.clone()).await;
        a_q.lock().unwrap().push_back(MockReply::Status(
            status,
            Some(format!("http://127.0.0.1:{}/hop2", a_addr.port())),
        ));
        a_q.lock().unwrap().push_back(MockReply::Json200);
        let home = tempfile::tempdir().expect("temp home");
        std::env::set_var("CC_SWITCH_TEST_HOME", home.path());
        let db = Arc::new(Database::init().expect("init isolated database"));
        seed_provider(&db, format!("http://127.0.0.1:{}/v1", a_addr.port()));
        let _ = http_client::init(None);
        http_client::apply_proxy(None).expect("no explicit proxy");
        let (info, server) = start_server(db.clone()).await;
        let resp = send_messages(info.port).await;
        assert_eq!(
            resp.status(),
            200,
            "status {status}: forwarder must succeed"
        );
        let fwd_hops = hops(&a_rec);
        server.stop().await.expect("stop");
        a_handle.abort();
        let _ = http_client::apply_proxy(None);

        // ---- compare ----
        assert_eq!(
            fwd_hops.len(),
            plain_hops.len(),
            "status {status}: hop count must match plain reqwest"
        );
        for (i, (f, p)) in fwd_hops.iter().zip(plain_hops.iter()).enumerate() {
            assert_eq!(f.method, p.method, "status {status} hop{i}: method");
            assert_eq!(
                f.has_body, p.has_body,
                "status {status} hop{i}: body presence"
            );
            assert_eq!(
                f.has_auth, p.has_auth,
                "status {status} hop{i}: authorization"
            );
            assert_eq!(
                f.has_referer, p.has_referer,
                "status {status} hop{i}: referer presence"
            );
        }
    }
}

/// R2: the raw hyper path (exact header-case preservation, native anthropic
/// format) must drive redirects through the SAME logical budget / deadline /
/// semantics state machine as the reqwest path. 12 self-redirects stop after
/// 1 initial + 10 followed requests and the 11th redirect is rejected.
#[tokio::test]
async fn r2_redirect_budget_raw_hyper_path() {
    let home = tempfile::tempdir().expect("temp home");
    std::env::set_var("CC_SWITCH_TEST_HOME", home.path());

    let a_rec: Arc<Mutex<Vec<RecReq>>> = Arc::new(Mutex::new(Vec::new()));
    let a_queue = Arc::new(Mutex::new(VecDeque::new()));
    let (a_addr, a_handle) = spawn_mock_queue(a_rec.clone(), a_queue.clone()).await;
    for _ in 0..12 {
        a_queue.lock().unwrap().push_back(MockReply::Status(
            307,
            Some(format!("http://127.0.0.1:{}/next", a_addr.port())),
        ));
    }
    a_queue.lock().unwrap().push_back(MockReply::Json200);

    let db = Arc::new(Database::init().expect("init isolated database"));
    // Native anthropic format (no OAuth adapter) selects the raw write path
    // with exact header-case preservation.
    seed_provider_with_format(
        &db,
        format!("http://127.0.0.1:{}/v1", a_addr.port()),
        "anthropic",
    );
    let _ = http_client::init(None);
    http_client::apply_proxy(None).expect("no explicit proxy");
    let (info, server) = start_server(db.clone()).await;
    let resp = send_messages(info.port).await;

    let status = resp.status();
    assert!(
        !status.is_success() && !status.is_redirection(),
        "raw hyper path must reject the 11th redirect, got {status}"
    );
    let recs = a_rec.lock().unwrap();
    assert_eq!(
        recs.len(),
        11,
        "raw hyper path must share the same 10-follow budget (1 initial + 10)"
    );
    let hop1_uri = recs[0].uri.clone();
    let expected_referer = if hop1_uri.starts_with("http") {
        hop1_uri
    } else {
        format!("http://127.0.0.1:{}{}", a_addr.port(), hop1_uri)
    };
    assert_eq!(
        recs[1].referer.as_deref(),
        Some(expected_referer.as_str()),
        "followed hops on the raw path must carry Referer = previous hop URL"
    );
    drop(recs);
    server.stop().await.expect("stop");
    a_handle.abort();
}

/// R2: proxy-kind classification must match the client builder's URL-scheme
/// acceptance (case-insensitive, SOCKS family incl. uppercase `SOCKS5://`).
/// A mis-classification would hand a SOCKS URL to hyper's HTTP-CONNECT.
#[test]
fn r2_proxy_kind_matches_builder_scheme_acceptance() {
    let _ = http_client::init(None);
    let _ = http_client::apply_proxy(None);
    assert_eq!(
        http_client::resolve_route_for_url("http://127.0.0.1:1").proxy_kind,
        http_client::RouteProxyKind::None,
        "no explicit proxy => None kind"
    );
    let _ = http_client::apply_proxy(Some("SOCKS5://127.0.0.1:1"));
    let decision = http_client::resolve_route_for_url("http://192.0.2.1/");
    assert_eq!(
        decision.proxy_kind,
        http_client::RouteProxyKind::Socks5,
        "uppercase SOCKS scheme must classify as SOCKS (builder accepts it)"
    );
    assert!(
        !decision.loopback_direct,
        "explicit proxy must win even for loopback targets"
    );
    let _ = http_client::apply_proxy(None);
}

// ===========================================================================
// R2: Referer helper edges (mirrors reqwest make_referer exactly)
// ===========================================================================

#[test]
fn r2_referer_helper_matches_reqwest_edges() {
    let next_http = reqwest::Url::parse("http://b.example/z").expect("url");
    let next_https = reqwest::Url::parse("https://b.example/z").expect("url");

    // http -> http: Referer = previous URL, fragment stripped.
    let r = super::forwarder::redirect_referer("http://a.example/x?y=1#frag", &next_http);
    assert_eq!(
        r.as_ref().and_then(|v| v.to_str().ok()),
        Some("http://a.example/x?y=1")
    );

    // https -> https: Referer = previous URL; credentials stripped.
    let r = super::forwarder::redirect_referer("https://u:p@a.example/x", &next_https);
    assert_eq!(
        r.as_ref().and_then(|v| v.to_str().ok()),
        Some("https://a.example/x")
    );

    // https -> http (security downgrade): NOT sent (reqwest parity).
    let r = super::forwarder::redirect_referer("https://a.example/x", &next_http);
    assert!(r.is_none(), "https->http must not send Referer");
}
