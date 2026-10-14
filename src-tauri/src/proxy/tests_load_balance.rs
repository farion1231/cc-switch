//! 会话粘性负载均衡集成测试：双 mock 上游 + 真实 ProviderRouter/RequestForwarder。
//!
//! 覆盖设计第 6 节的四个集成场景：
//! ① 多会话分摊到两账号 ② 同会话恒定同账号
//! ③ 杀一号后会话迁移（failover 重映射） ④ 429 后新会话避开冷却账号
//!
//! mock 上游用手写 HTTP/1.1 应答（不引新 dev-dependency），按序消费 canned
//! 响应，耗尽后回 500。

use super::forwarder::RequestForwarder;
use super::provider_router::ProviderRouter;
use super::types::{CopilotOptimizerConfig, OptimizerConfig, ProxyStatus, RectifierConfig};
use super::{failover_switch::FailoverSwitchManager, providers};
use crate::app_config::AppType;
use crate::database::Database;
use crate::provider::Provider;
use axum::http::HeaderMap;
use http::Extensions;
use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::RwLock;

use providers::codex_chat_history::CodexChatHistoryStore;
use providers::gemini_shadow::GeminiShadowStore;

/// 一条 canned 响应；按序消费
struct MockResponse {
    status: u16,
    retry_after: Option<&'static str>,
    body: String,
}

/// 极简 HTTP mock 上游：读完请求头+body，回 canned 响应。
/// 返回 (地址, 请求计数)。响应耗尽后回 500。
async fn spawn_mock_upstream(responses: Vec<MockResponse>) -> (SocketAddr, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let count = Arc::new(AtomicUsize::new(0));
    let counter = count.clone();
    let queue = Arc::new(std::sync::Mutex::new(
        responses.into_iter().collect::<VecDeque<MockResponse>>(),
    ));
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else {
                break;
            };
            let queue = queue.clone();
            let counter = counter.clone();
            tokio::spawn(async move {
                let mut acc = Vec::new();
                let mut buf = [0u8; 8192];
                // 读到请求头结束且 body 读满（简化实现：无 100-continue）
                loop {
                    let n = sock.read(&mut buf).await.unwrap_or(0);
                    if n == 0 {
                        return;
                    }
                    acc.extend_from_slice(&buf[..n]);
                    if let Some(pos) = find_header_end(&acc) {
                        let content_length = extract_content_length(&acc[..pos]);
                        if acc.len() >= pos + 4 + content_length {
                            break;
                        }
                    }
                }
                counter.fetch_add(1, Ordering::SeqCst);
                let (status, retry_after, body) = {
                    let mut queue = queue.lock().unwrap();
                    match queue.pop_front() {
                        Some(r) => (r.status, r.retry_after, r.body),
                        None => (500u16, None, "{\"error\":\"exhausted\"}".to_string()),
                    }
                };
                let reason = match status {
                    200 => "OK",
                    429 => "Too Many Requests",
                    _ => "Internal Server Error",
                };
                let mut head = format!(
                    "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n",
                    body.len()
                );
                if let Some(ra) = retry_after {
                    head.push_str(&format!("Retry-After: {ra}\r\n"));
                }
                head.push_str("\r\n");
                let _ = sock.write_all(head.as_bytes()).await;
                let _ = sock.write_all(body.as_bytes()).await;
                let _ = sock.shutdown().await;
            });
        }
    });
    (addr, count)
}

fn find_header_end(acc: &[u8]) -> Option<usize> {
    acc.windows(4).position(|w| w == b"\r\n\r\n")
}

fn extract_content_length(headers: &[u8]) -> usize {
    let text = String::from_utf8_lossy(headers).to_lowercase();
    text.lines()
        .find_map(|l| l.strip_prefix("content-length:"))
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(0)
}

/// 带唯一响应 id 的 OK 响应体。Claude 应用用量日志的去重键回退到响应体 id，
/// 每条 canned 响应必须各持唯一 id，否则同 provider 的多行日志会被去重覆盖。
fn ok_body(id: &str) -> String {
    format!(
        "{{\"id\":\"{id}\",\"model\":\"claude-3-5-sonnet\",\"role\":\"assistant\",\"type\":\"message\",\"content\":[{{\"type\":\"text\",\"text\":\"hi\"}}],\"stop_reason\":\"end_turn\",\"usage\":{{\"input_tokens\":8,\"output_tokens\":2}}}}"
    )
}

/// HOME 隔离（与 provider_router 测试同款），防止 settings 落到真实用户目录
struct TempHome {
    #[allow(dead_code)]
    dir: tempfile::TempDir,
}

/// 安装 rustls CryptoProvider（应用在 tauri setup 里装；测试进程需自行装，
/// 否则 reqwest 初始化 TLS 时 panic）
fn ensure_crypto_provider() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

impl TempHome {
    fn new() -> Self {
        ensure_crypto_provider();
        use std::env;
        let dir = tempfile::TempDir::new().expect("temp home");
        env::set_var("HOME", dir.path());
        env::set_var("USERPROFILE", dir.path());
        env::set_var("CC_SWITCH_TEST_HOME", dir.path());
        crate::settings::reload_settings().expect("reload settings");
        Self { dir }
    }
}

impl Drop for TempHome {
    fn drop(&mut self) {
        std::env::remove_var("CC_SWITCH_TEST_HOME");
    }
}

fn claude_provider(id: &str, sort: usize, base: &SocketAddr) -> Provider {
    let mut p = Provider::with_id(
        id.to_string(),
        format!("Provider-{id}"),
        json!({
            "env": {
                "ANTHROPIC_BASE_URL": format!("http://{base}"),
                "ANTHROPIC_AUTH_TOKEN": "sk-test"
            }
        }),
        None,
    );
    p.sort_index = Some(sort);
    p
}

/// 双账号 session 模式环境：a(队首) + b，故障转移开启
async fn seed_session_mode(db: &Arc<Database>, a: &SocketAddr, b: &SocketAddr) {
    let provider_a = claude_provider("a", 1, a);
    let provider_b = claude_provider("b", 2, b);
    db.save_provider("claude", &provider_a).unwrap();
    db.save_provider("claude", &provider_b).unwrap();
    db.add_to_failover_queue("claude", "a").unwrap();
    db.add_to_failover_queue("claude", "b").unwrap();
    let mut config = db.get_proxy_config_for_app("claude").await.unwrap();
    config.auto_failover_enabled = true;
    config.load_balance_mode = "session".to_string();
    db.update_proxy_config_for_app(config).await.unwrap();
}

fn make_forwarder(
    router: Arc<ProviderRouter>,
    session_id: &str,
    client_provided: bool,
) -> RequestForwarder {
    let db = Arc::new(Database::memory().unwrap());
    RequestForwarder::new(
        router,
        5, // non_streaming_timeout（秒）
        Arc::new(RwLock::new(ProxyStatus::default())),
        Arc::new(RwLock::new(HashMap::new())),
        Arc::new(GeminiShadowStore::new()),
        Arc::new(CodexChatHistoryStore::default()),
        Arc::new(FailoverSwitchManager::new()),
        None,
        "a".to_string(),
        session_id.to_string(),
        client_provided,
        5, // streaming_first_byte_timeout（秒）
        0,
        RectifierConfig::default(),
        OptimizerConfig::default(),
        CopilotOptimizerConfig::default(),
        2, // max_retries → max_attempts=3
    )
}

fn request_body() -> Value {
    json!({
        "model": "claude-3-5-sonnet",
        "max_tokens": 16,
        "messages": [{"role": "user", "content": "hi"}]
    })
}

async fn forward_once(router: &Arc<ProviderRouter>, session_id: &str, client_provided: bool) {
    let providers = router
        .select_providers_for_session("claude", session_id, client_provided, "claude-3-5-sonnet")
        .await
        .unwrap();
    let forwarder = make_forwarder(router.clone(), session_id, client_provided);
    let result = forwarder
        .forward_with_retry(
            &AppType::Claude,
            http::Method::POST,
            "/v1/messages",
            request_body(),
            HeaderMap::new(),
            Extensions::new(),
            providers,
        )
        .await;
    if let Err(e) = result {
        panic!("forward failed: {}", e.error);
    }
}

/// 场景①：多会话在两账号间轮询分摊；粘性映射各归各账号
#[tokio::test(flavor = "multi_thread")]
#[serial_test::serial]
async fn multiple_sessions_distribute_across_accounts() {
    let _home = TempHome::new();
    let (addr_a, count_a) = spawn_mock_upstream(vec![
        MockResponse {
            status: 200,
            retry_after: None,
            body: ok_body("msg_1"),
        },
        MockResponse {
            status: 200,
            retry_after: None,
            body: ok_body("msg_2"),
        },
        MockResponse {
            status: 200,
            retry_after: None,
            body: ok_body("msg_3"),
        },
    ])
    .await;
    let (addr_b, count_b) = spawn_mock_upstream(vec![
        MockResponse {
            status: 200,
            retry_after: None,
            body: ok_body("msg_4"),
        },
        MockResponse {
            status: 200,
            retry_after: None,
            body: ok_body("msg_5"),
        },
    ])
    .await;

    let db = Arc::new(Database::memory().unwrap());
    seed_session_mode(&db, &addr_a, &addr_b).await;
    let router = Arc::new(ProviderRouter::new(db));

    for session in ["s1", "s2", "s3"] {
        forward_once(&router, session, true).await;
    }

    // 轮询分摊：a 收 2 次（s1/s3），b 收 1 次（s2）
    assert_eq!(count_a.load(Ordering::SeqCst), 2);
    assert_eq!(count_b.load(Ordering::SeqCst), 1);
    // 粘性映射
    let lb = router.load_balancer();
    assert_eq!(lb.lookup_sticky("claude", "s1"), Some("a".to_string()));
    assert_eq!(lb.lookup_sticky("claude", "s2"), Some("b".to_string()));
    assert_eq!(lb.lookup_sticky("claude", "s3"), Some("a".to_string()));
}

/// 场景②：同一会话恒定同一账号
#[tokio::test(flavor = "multi_thread")]
#[serial_test::serial]
async fn same_session_hits_same_upstream() {
    let _home = TempHome::new();
    let (addr_a, count_a) = spawn_mock_upstream(vec![
        MockResponse {
            status: 200,
            retry_after: None,
            body: ok_body("msg_6"),
        },
        MockResponse {
            status: 200,
            retry_after: None,
            body: ok_body("msg_7"),
        },
    ])
    .await;
    let (addr_b, count_b) = spawn_mock_upstream(vec![]).await;

    let db = Arc::new(Database::memory().unwrap());
    seed_session_mode(&db, &addr_a, &addr_b).await;
    let router = Arc::new(ProviderRouter::new(db));

    forward_once(&router, "s1", true).await;
    forward_once(&router, "s1", true).await;

    assert_eq!(count_a.load(Ordering::SeqCst), 2);
    assert_eq!(count_b.load(Ordering::SeqCst), 0);
}

/// 场景③：首选账号 5xx → failover 到幸存者，会话重映射并固定到幸存者
#[tokio::test(flavor = "multi_thread")]
#[serial_test::serial]
async fn failover_remaps_session_to_survivor() {
    let _home = TempHome::new();
    let (addr_a, count_a) = spawn_mock_upstream(vec![MockResponse {
        status: 500,
        retry_after: None,
        body: "{\"error\":\"boom\"}".to_string(),
    }])
    .await;
    let (addr_b, count_b) = spawn_mock_upstream(vec![
        MockResponse {
            status: 200,
            retry_after: None,
            body: ok_body("msg_8"),
        },
        MockResponse {
            status: 200,
            retry_after: None,
            body: ok_body("msg_9"),
        },
    ])
    .await;

    let db = Arc::new(Database::memory().unwrap());
    seed_session_mode(&db, &addr_a, &addr_b).await;
    let router = Arc::new(ProviderRouter::new(db));

    // 第一发：路由选中 a（游标起点），a 500 → failover b 成功 → 重映射
    forward_once(&router, "s1", true).await;
    assert_eq!(count_a.load(Ordering::SeqCst), 1);
    assert_eq!(count_b.load(Ordering::SeqCst), 1);
    assert_eq!(
        router.load_balancer().lookup_sticky("claude", "s1"),
        Some("b".to_string())
    );

    // 第二发：粘性命中 b，直接成功，不再打 a
    forward_once(&router, "s1", true).await;
    assert_eq!(count_a.load(Ordering::SeqCst), 1);
    assert_eq!(count_b.load(Ordering::SeqCst), 2);
}

/// 场景④：a 返回 429（Retry-After 120）→ 进冷却；新会话选号避开 a
#[tokio::test(flavor = "multi_thread")]
#[serial_test::serial]
async fn rate_limited_account_entering_cooldown_is_skipped() {
    let _home = TempHome::new();
    let (addr_a, _count_a) = spawn_mock_upstream(vec![MockResponse {
        status: 429,
        retry_after: Some("120"),
        body: "{\"error\":{\"message\":\"rate limit exceeded\"}}".to_string(),
    }])
    .await;
    let (addr_b, count_b) = spawn_mock_upstream(vec![MockResponse {
        status: 200,
        retry_after: None,
        body: ok_body("msg_10"),
    }])
    .await;

    let db = Arc::new(Database::memory().unwrap());
    seed_session_mode(&db, &addr_a, &addr_b).await;
    let router = Arc::new(ProviderRouter::new(db));

    // s1：路由选中 a，a 429 → 冷却记录 → failover b 成功
    forward_once(&router, "s1", true).await;
    assert_eq!(count_b.load(Ordering::SeqCst), 1);
    // a 进入冷却（Retry-After 120 生效）
    let lb = router.load_balancer();
    assert!(lb.is_cooling("claude", "a"));
    assert!(!lb.is_cooling("claude", "b"));

    // 新会话 s2：选号避开冷却中的 a
    let selected = router
        .select_providers_for_session("claude", "s2", true, "claude-3-5-sonnet")
        .await
        .unwrap();
    assert_eq!(selected[0].id, "b");
}

/// 端到端（设计第 6 节手工验收的自动化等价）：起真实代理 HTTP 服务器，
/// 模拟 Claude Code 多会话请求，验证 ①多会话分摊两账号 ②同会话恒定同账号
/// ③用量日志两账号同涨（代理页用量的数据层）。
///
/// 真实 Claude Code 客户端与上游 prompt cache 命中属环境依赖，无法在
/// 单测覆盖——上游侧由 mock 的请求计数与粘性映射间接验证。
#[tokio::test(flavor = "multi_thread")]
#[serial_test::serial]
async fn e2e_proxy_server_distributes_sessions_and_logs_usage() {
    let _home = TempHome::new();
    ensure_crypto_provider();
    // a 收 s1/s3/s1 追加共 3 次；b 收 s2 一次
    let (addr_a, count_a) = spawn_mock_upstream(vec![
        MockResponse {
            status: 200,
            retry_after: None,
            body: ok_body("msg_11"),
        },
        MockResponse {
            status: 200,
            retry_after: None,
            body: ok_body("msg_12"),
        },
        MockResponse {
            status: 200,
            retry_after: None,
            body: ok_body("msg_13"),
        },
    ])
    .await;
    let (addr_b, count_b) = spawn_mock_upstream(vec![MockResponse {
        status: 200,
        retry_after: None,
        body: ok_body("msg_14"),
    }])
    .await;

    let db = Arc::new(Database::memory().unwrap());
    seed_session_mode(&db, &addr_a, &addr_b).await;

    let server = super::server::ProxyServer::new(
        super::types::ProxyConfig {
            listen_address: "127.0.0.1".to_string(),
            listen_port: 0, // 临时端口，避免与本机运行中的实例冲突
            ..Default::default()
        },
        db.clone(),
        None,
    );
    let info = server.start().await.expect("start proxy server");

    let url = format!("http://127.0.0.1:{}/v1/messages", info.port);
    let client = reqwest::Client::new();
    let send = |session: &'static str| {
        let client = client.clone();
        let url = url.clone();
        async move {
            let resp = client
                .post(&url)
                .header("x-claude-code-session-id", session)
                .header("anthropic-version", "2023-06-01")
                .json(&request_body())
                .send()
                .await
                .expect("send request");
            let status = resp.status();
            let body = resp.text().await.expect("read body");
            assert_eq!(status, 200, "proxy should return 200, body: {body}");
        }
    };

    // 三个客户端会话 + s1 追加一轮
    for session in ["s1", "s2", "s3", "s1"] {
        send(session).await;
    }

    server.stop().await.expect("stop proxy server");

    // ① 分摊：a=3（s1 两轮 + s3），b=1（s2）
    assert_eq!(count_a.load(Ordering::SeqCst), 3, "上游 a 请求计数");
    assert_eq!(count_b.load(Ordering::SeqCst), 1, "上游 b 请求计数");

    // ② 同会话恒定：s1 两轮都落 a（上游计数即证明）
    // ③ 用量同涨：两 provider 的 proxy_request_logs 均有行（异步落库，轮询等待）
    let mut logs = db.count_request_logs_by_provider("claude");
    for _ in 0..40 {
        if logs.iter().map(|(_, n)| n).sum::<i64>() >= 4 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
        logs = db.count_request_logs_by_provider("claude");
    }
    let by_provider: std::collections::HashMap<String, i64> = logs.into_iter().collect();
    assert_eq!(
        by_provider.get("a"),
        Some(&3),
        "provider a 用量行数: {by_provider:?}"
    );
    assert_eq!(
        by_provider.get("b"),
        Some(&1),
        "provider b 用量行数: {by_provider:?}"
    );
}
