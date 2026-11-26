use std::sync::Arc;
use std::time::{Duration, Instant};

use cc_switch_lib::{AppState, AppType, Provider, ProviderMeta, ProviderService};
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::{oneshot, Notify};

use crate::support::{create_test_state, ensure_test_home, reset_test_fs, test_mutex};

fn claude_provider(id: &str, name: &str, base_url: &str, api_format: Option<&str>) -> Provider {
    let mut provider = Provider::with_id(
        id.to_string(),
        name.to_string(),
        json!({
            "env": {
                "ANTHROPIC_BASE_URL": base_url,
                "ANTHROPIC_AUTH_TOKEN": "sk-e2e-test-token"
            }
        }),
        None,
    );
    if let Some(fmt) = api_format {
        provider.meta = Some(ProviderMeta {
            api_format: Some(fmt.to_string()),
            ..Default::default()
        });
    }
    provider
}

async fn start_proxy_on(
    state: &AppState,
    listen_address: &str,
    auto_failover_enabled: bool,
    max_retries: u32,
    streaming_first_byte_timeout: u32,
) -> (String, u16) {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let mut global_cfg = state
        .db
        .get_global_proxy_config()
        .await
        .expect("get global proxy config");
    global_cfg.proxy_enabled = true;
    global_cfg.listen_address = listen_address.to_string();
    global_cfg.listen_port = 0;
    global_cfg.enable_logging = true;
    state
        .db
        .update_global_proxy_config(global_cfg)
        .await
        .expect("update global proxy config");

    let mut claude_cfg = state
        .db
        .get_proxy_config_for_app(&AppType::Claude)
        .await
        .expect("get claude proxy config");
    claude_cfg.enabled = true;
    claude_cfg.auto_failover_enabled = auto_failover_enabled;
    claude_cfg.max_retries = max_retries;
    claude_cfg.streaming_first_byte_timeout = streaming_first_byte_timeout;
    claude_cfg.streaming_idle_timeout = streaming_first_byte_timeout;
    claude_cfg.non_streaming_timeout = 10;
    state
        .db
        .update_proxy_config_for_app(claude_cfg)
        .await
        .expect("update claude proxy config");

    let info = state
        .proxy_service
        .start()
        .await
        .expect("start real proxy server");
    let connect_host = if listen_address == "0.0.0.0" {
        "127.0.0.1"
    } else {
        listen_address
    };
    (format!("http://{connect_host}:{}", info.port), info.port)
}

async fn read_http_request_head(socket: &mut tokio::net::TcpStream) {
    let mut buf = Vec::with_capacity(4096);
    let mut tmp = [0u8; 1024];
    loop {
        let n = socket.read(&mut tmp).await.unwrap_or(0);
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&tmp[..n]);
        if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&buf[..pos]);
            let content_len = head
                .lines()
                .find_map(|l| {
                    let (k, v) = l.split_once(':')?;
                    if k.eq_ignore_ascii_case("content-length") {
                        v.trim().parse::<usize>().ok()
                    } else {
                        None
                    }
                })
                .unwrap_or(0);
            let body_have = buf.len() - (pos + 4);
            if body_have < content_len {
                let mut remain = vec![0u8; content_len - body_have];
                let _ = socket.read_exact(&mut remain).await;
            }
            break;
        }
    }
}

fn open_test_db_readonly() -> rusqlite::Connection {
    let db_path = ensure_test_home().join(".cc-switch").join("cc-switch.db");
    rusqlite::Connection::open_with_flags(db_path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .expect("open test cc-switch.db")
}

fn query_latest_request_log() -> Option<(String, i64, i64, i64)> {
    let conn = open_test_db_readonly();
    conn.query_row(
        "SELECT provider_id, input_tokens, output_tokens, status_code \
         FROM proxy_request_logs ORDER BY created_at DESC, rowid DESC LIMIT 1",
        [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
    )
    .ok()
}

fn count_request_logs() -> i64 {
    let conn = open_test_db_readonly();
    conn.query_row("SELECT COUNT(*) FROM proxy_request_logs", [], |row| {
        row.get(0)
    })
    .expect("count proxy_request_logs")
}

/// 1. 真实 SSE 极端分片（逐 2 字节跨 UTF-8/边界）+ 单包大批量事件（>256 KiB）
///    端到端验证：HTTP 流式输出完整、SQLite 用量落库准确。
#[allow(clippy::await_holding_lock)]
#[tokio::test]
async fn e2e_sse_trickle_and_batched_events_stream_and_log_usage() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    let _home = ensure_test_home();

    let state = create_test_state().expect("create test state");

    // 恶意上游 1：逐 2 字节慢速切分 SSE（跨多字节 UTF-8 中文和 \n\n 事件边界）
    let trickle_listener = TcpListener::bind("127.0.0.1:0").await.expect("bind trickle");
    let trickle_addr = trickle_listener.local_addr().unwrap();
    let trickle_task = tokio::spawn(async move {
        let (mut socket, _) = trickle_listener.accept().await.expect("accept trickle");
        read_http_request_head(&mut socket).await;

        let sse_body = concat!(
            "event: message_start\n",
            "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_trickle\",\"type\":\"message\",\"role\":\"assistant\",\"model\":\"claude-sonnet-4-5\",\"content\":[],\"usage\":{\"input_tokens\":42,\"output_tokens\":0}}}\n\n",
            "event: content_block_start\n",
            "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
            "event: content_block_delta\n",
            "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"你好世界-trickle\"}}\n\n",
            "event: content_block_stop\n",
            "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
            "event: message_delta\n",
            "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":17}}\n\n",
            "event: message_stop\n",
            "data: {\"type\":\"message_stop\"}\n\n"
        );
        let headers = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: close\r\n\r\n";
        socket.write_all(headers.as_bytes()).await.unwrap();
        socket.flush().await.unwrap();

        for chunk in sse_body.as_bytes().chunks(2) {
            socket.write_all(chunk).await.unwrap();
            socket.flush().await.unwrap();
        }
        let _ = socket.shutdown().await;
    });

    // 恶意上游 2：把 90 条正常大小 SSE 事件（每条 ~3.5 KiB，总计 > 310 KiB）合并成一个 TCP 巨包一次写出
    let burst_listener = TcpListener::bind("127.0.0.1:0").await.expect("bind burst");
    let burst_addr = burst_listener.local_addr().unwrap();
    let burst_task = tokio::spawn(async move {
        let (mut socket, _) = burst_listener.accept().await.expect("accept burst");
        read_http_request_head(&mut socket).await;

        let mut sse_body = String::with_capacity(340 * 1024);
        sse_body.push_str(
            "event: message_start\n\
             data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_burst\",\"type\":\"message\",\"role\":\"assistant\",\"model\":\"claude-sonnet-4-5\",\"content\":[],\"usage\":{\"input_tokens\":120,\"output_tokens\":0}}}\n\n\
             event: content_block_start\n\
             data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
        );
        let pad = "x".repeat(3500);
        for i in 0..90 {
            sse_body.push_str(&format!(
                "event: content_block_delta\ndata: {{\"type\":\"content_block_delta\",\"index\":0,\"delta\":{{\"type\":\"text_delta\",\"text\":\"chunk-{i:02}-{pad}\"}}}}\n\n"
            ));
        }
        sse_body.push_str(
            "event: content_block_stop\n\
             data: {\"type\":\"content_block_stop\",\"index\":0}\n\n\
             event: message_delta\n\
             data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":99}}\n\n\
             event: message_stop\n\
             data: {\"type\":\"message_stop\"}\n\n",
        );
        assert!(
            sse_body.len() > 300 * 1024,
            "burst payload must exceed 256 KiB pending threshold"
        );

        let headers = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            sse_body.len()
        );
        socket.write_all(headers.as_bytes()).await.unwrap();
        socket.write_all(sse_body.as_bytes()).await.unwrap();
        socket.flush().await.unwrap();
        let _ = socket.shutdown().await;
    });

    let p_trickle = claude_provider(
        "p-trickle",
        "Trickle",
        &format!("http://{trickle_addr}"),
        None,
    );
    let p_burst = claude_provider("p-burst", "Burst", &format!("http://{burst_addr}"), None);
    state
        .db
        .save_provider(&AppType::Claude, &p_trickle)
        .expect("save p_trickle");
    state
        .db
        .save_provider(&AppType::Claude, &p_burst)
        .expect("save p_burst");
    state
        .db
        .set_current_provider(&AppType::Claude, "p-trickle")
        .expect("set current p-trickle");

    let (proxy_base, _) = start_proxy_on(&state, "127.0.0.1", false, 0, 5).await;
    let client = reqwest::Client::builder()
        .no_proxy()
        .build()
        .expect("build reqwest");

    // A) 验证逐 2 字节分片流式响应 + SQLite 用量落库
    let resp = client
        .post(format!("{proxy_base}/v1/messages"))
        .json(&json!({
            "model": "claude-sonnet-4-5",
            "max_tokens": 64,
            "stream": true,
            "messages": [{"role": "user", "content": "hi"}]
        }))
        .send()
        .await
        .expect("send trickle request");
    assert_eq!(resp.status(), 200);
    let body = resp.text().await.expect("read trickle body");
    assert!(
        body.contains("你好世界-trickle"),
        "expected full UTF-8 text in trickle stream, got: {body}"
    );
    assert!(body.contains("message_stop"), "missing message_stop");

    // 等待异步用量日志落库
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut logged = None;
    while Instant::now() < deadline {
        if count_request_logs() >= 1 {
            logged = query_latest_request_log();
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    let (pid, in_tok, out_tok, status) = logged.expect("usage log for trickle stream");
    assert_eq!(pid, "p-trickle");
    assert_eq!(status, 200);
    assert_eq!(in_tok, 42);
    assert_eq!(out_tok, 17);

    // B) 切换到 burst 上游，验证 >256 KiB 单包多事件不被错误截断，且用量依然落库
    state
        .db
        .set_current_provider(&AppType::Claude, "p-burst")
        .expect("set current p-burst");

    let resp2 = client
        .post(format!("{proxy_base}/v1/messages"))
        .json(&json!({
            "model": "claude-sonnet-4-5",
            "max_tokens": 64,
            "stream": true,
            "messages": [{"role": "user", "content": "burst"}]
        }))
        .send()
        .await
        .expect("send burst request");
    assert_eq!(resp2.status(), 200);
    let body2 = resp2.text().await.expect("read burst body");
    assert!(
        body2.contains("chunk-89-"),
        "burst stream dropped tail events"
    );
    assert!(
        body2.contains("message_stop"),
        "burst stream missing message_stop"
    );

    let deadline2 = Instant::now() + Duration::from_secs(3);
    let mut logged2 = None;
    while Instant::now() < deadline2 {
        if count_request_logs() >= 2 {
            logged2 = query_latest_request_log();
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    let (pid2, in_tok2, out_tok2, status2) =
        logged2.expect("usage log for >256KiB coalesced SSE burst");
    assert_eq!(pid2, "p-burst");
    assert_eq!(status2, 200);
    assert_eq!(
        in_tok2, 120,
        "coalesced >256KiB SSE chunk must not break usage parsing (input_tokens)"
    );
    assert_eq!(
        out_tok2, 99,
        "coalesced >256KiB SSE chunk must not break usage parsing (output_tokens)"
    );

    let _ = state.proxy_service.stop().await;
    let _ = trickle_task.await;
    let _ = burst_task.await;
}

/// 2. 并发竞态不变量：旧请求在飞行途中发生故障转移时，不得覆盖用户已手动切换的新路由。
#[allow(clippy::await_holding_lock)]
#[tokio::test]
async fn e2e_stale_failover_does_not_overwrite_manual_provider_switch() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    let _home = ensure_test_home();

    let state = create_test_state().expect("create test state");

    let p1_entered = Arc::new(Notify::new());
    let (p1_release_tx, p1_release_rx) = oneshot::channel::<()>();

    // p1: 收到请求后挂起，等用户手动切走后再返回 500 触发故障转移
    let p1_listener = TcpListener::bind("127.0.0.1:0").await.expect("bind p1");
    let p1_addr = p1_listener.local_addr().unwrap();
    let p1_entered_srv = p1_entered.clone();
    let p1_task = tokio::spawn(async move {
        let (mut socket, _) = p1_listener.accept().await.expect("accept p1");
        read_http_request_head(&mut socket).await;
        p1_entered_srv.notify_one();
        let _ = p1_release_rx.await;
        let body = r#"{"type":"error","error":{"type":"api_error","message":"p1 boom"}}"#;
        let resp = format!(
            "HTTP/1.1 500 Internal Server Error\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        let _ = socket.write_all(resp.as_bytes()).await;
        let _ = socket.shutdown().await;
    });

    // p2: 故障转移队列里的备选节点（健康，返回 from-p2）
    let p2_listener = TcpListener::bind("127.0.0.1:0").await.expect("bind p2");
    let p2_addr = p2_listener.local_addr().unwrap();
    let p2_task = tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = p2_listener.accept().await else {
                break;
            };
            tokio::spawn(async move {
                read_http_request_head(&mut socket).await;
                let body = r#"{"id":"msg_p2","type":"message","role":"assistant","model":"claude-sonnet-4-5","content":[{"type":"text","text":"from-p2"}],"usage":{"input_tokens":5,"output_tokens":3}}"#;
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = socket.write_all(resp.as_bytes()).await;
                let _ = socket.shutdown().await;
            });
        }
    });

    // p3: 用户手动切换的目标节点（健康，返回 from-p3）
    let p3_listener = TcpListener::bind("127.0.0.1:0").await.expect("bind p3");
    let p3_addr = p3_listener.local_addr().unwrap();
    let p3_task = tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = p3_listener.accept().await else {
                break;
            };
            tokio::spawn(async move {
                read_http_request_head(&mut socket).await;
                let body = r#"{"id":"msg_p3","type":"message","role":"assistant","model":"claude-sonnet-4-5","content":[{"type":"text","text":"from-p3"}],"usage":{"input_tokens":5,"output_tokens":3}}"#;
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = socket.write_all(resp.as_bytes()).await;
                let _ = socket.shutdown().await;
            });
        }
    });

    for (id, name, addr) in [
        ("p1", "Primary-Failing", p1_addr),
        ("p2", "Failover-Backup", p2_addr),
        ("p3", "Manual-Target", p3_addr),
    ] {
        state
            .db
            .save_provider(
                &AppType::Claude,
                &claude_provider(id, name, &format!("http://{addr}"), None),
            )
            .expect("save provider");
    }
    state
        .db
        .set_current_provider(&AppType::Claude, "p1")
        .expect("set p1 current");
    state
        .db
        .add_to_failover_queue(&AppType::Claude, "p1")
        .expect("queue p1");
    state
        .db
        .add_to_failover_queue(&AppType::Claude, "p2")
        .expect("queue p2");
    state
        .db
        .add_to_failover_queue(&AppType::Claude, "p3")
        .expect("queue p3");

    let (proxy_base, _) = start_proxy_on(&state, "127.0.0.1", true, 1, 5).await;

    let client = reqwest::Client::builder()
        .no_proxy()
        .build()
        .expect("build reqwest");

    // 1) 发出旧请求，命中 p1 后阻塞
    let proxy_base_clone = proxy_base.clone();
    let client_clone = client.clone();
    let stale_req = tokio::spawn(async move {
        client_clone
            .post(format!("{proxy_base_clone}/v1/messages"))
            .json(&json!({
                "model": "claude-sonnet-4-5",
                "max_tokens": 32,
                "stream": false,
                "messages": [{"role": "user", "content": "stale"}]
            }))
            .send()
            .await
            .expect("stale request send")
    });

    // 2) 等旧请求确实进入 p1 后，模拟用户在飞行途中关闭自动故障转移并手动切到 p3
    tokio::time::timeout(Duration::from_secs(3), p1_entered.notified())
        .await
        .expect("p1 should receive in-flight request");
    let mut manual_cfg = state
        .db
        .get_proxy_config_for_app(&AppType::Claude)
        .await
        .expect("get claude proxy cfg");
    manual_cfg.auto_failover_enabled = false;
    state
        .db
        .update_proxy_config_for_app(manual_cfg)
        .await
        .expect("disable auto failover");
    ProviderService::switch(&state, AppType::Claude, "p3").expect("manual switch to p3");

    // 3) 放行 p1 返回 500，旧请求故障转移到 p2 并完成
    let _ = p1_release_tx.send(());
    let stale_resp = stale_req.await.expect("join stale req");
    assert_eq!(stale_resp.status(), 200);
    let stale_body = stale_resp.text().await.unwrap();
    assert!(
        stale_body.contains("from-p2"),
        "stale request should fail over to p2, got: {stale_body}"
    );

    // 给异步 failover switch 任务留出落地时间
    tokio::time::sleep(Duration::from_millis(250)).await;

    // 4) 核心不变量：DB 当前供应商与后续真实请求都必须停留在用户手选的 p3，绝不能被旧请求改写回 p2
    let current = state
        .db
        .get_current_provider(&AppType::Claude)
        .expect("read current provider");
    assert_eq!(
        current.as_deref(),
        Some("p3"),
        "stale failover overwrote manual switch to p3"
    );

    let follow_up = client
        .post(format!("{proxy_base}/v1/messages"))
        .json(&json!({
            "model": "claude-sonnet-4-5",
            "max_tokens": 32,
            "stream": false,
            "messages": [{"role": "user", "content": "after-manual-switch"}]
        }))
        .send()
        .await
        .expect("follow-up request");
    assert_eq!(follow_up.status(), 200);
    let follow_up_body = follow_up.text().await.unwrap();
    assert!(
        follow_up_body.contains("from-p3"),
        "follow-up request was hijacked away from p3, got: {follow_up_body}"
    );

    let _ = state.proxy_service.stop().await;
    p1_task.abort();
    p2_task.abort();
    p3_task.abort();
}

/// 3. 超时活性不变量：流式请求遇到只建连不回响应头的黑洞上游时，必须在首字超时内快速失败。
#[allow(clippy::await_holding_lock)]
#[tokio::test]
async fn e2e_streaming_first_byte_timeout_bounds_hung_header_upstream() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    let _home = ensure_test_home();

    let state = create_test_state().expect("create test state");

    let blackhole = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind blackhole");
    let blackhole_addr = blackhole.local_addr().unwrap();
    let blackhole_task = tokio::spawn(async move {
        if let Ok((mut socket, _)) = blackhole.accept().await {
            read_http_request_head(&mut socket).await;
            // 故意不返回任何响应头，挂死 15 秒
            tokio::time::sleep(Duration::from_secs(15)).await;
        }
    });

    state
        .db
        .save_provider(
            &AppType::Claude,
            &claude_provider(
                "p-hang",
                "Blackhole",
                &format!("http://{blackhole_addr}"),
                None,
            ),
        )
        .expect("save p-hang");
    state
        .db
        .set_current_provider(&AppType::Claude, "p-hang")
        .expect("set p-hang current");

    state
        .db
        .add_to_failover_queue(&AppType::Claude, "p-hang")
        .expect("queue p-hang");

    let (proxy_base, _) = start_proxy_on(&state, "127.0.0.1", true, 0, 1).await;

    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(6))
        .build()
        .expect("build reqwest");

    let started = Instant::now();
    let resp = client
        .post(format!("{proxy_base}/v1/messages"))
        .json(&json!({
            "model": "claude-sonnet-4-5",
            "max_tokens": 32,
            "stream": true,
            "messages": [{"role": "user", "content": "hang"}]
        }))
        .send()
        .await
        .expect("proxy should return timeout HTTP error rather than hanging client");
    let elapsed = started.elapsed();

    assert!(
        resp.status().is_server_error(),
        "expected 5xx timeout response, got {} in {:?}",
        resp.status(),
        elapsed
    );
    assert!(
        elapsed < Duration::from_secs(4),
        "streaming first-byte timeout took {:?} (expected < 4s for 1s config)",
        elapsed
    );

    let _ = state.proxy_service.stop().await;
    blackhole_task.abort();
}

/// 4. 安全边界不变量：监听 0.0.0.0 时，必须按连接校验 Host 头并拒绝伪造 Host（防 DNS Rebinding）。
#[allow(clippy::await_holding_lock)]
#[tokio::test]
async fn e2e_non_loopback_proxy_rejects_forged_host_header() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    let _home = ensure_test_home();

    let state = create_test_state().expect("create test state");
    let (_proxy_base, port) = start_proxy_on(&state, "0.0.0.0", false, 0, 5).await;

    // 原始 TCP 发送伪造 Host 头，模拟 DNS Rebinding 攻击
    let send_raw = |host_header: String| async move {
        let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .expect("connect proxy");
        let req = format!(
            "GET /health HTTP/1.1\r\nHost: {host_header}\r\nConnection: close\r\n\r\n"
        );
        stream.write_all(req.as_bytes()).await.unwrap();
        let mut resp = String::new();
        let _ = stream.read_to_string(&mut resp).await;
        resp
    };

    let forged = send_raw("evil.attacker.example".to_string()).await;
    assert!(
        forged.starts_with("HTTP/1.1 403"),
        "non-loopback proxy MUST reject forged Host header with 403, got: {}",
        forged.lines().next().unwrap_or("<empty>")
    );

    let legit = send_raw(format!("127.0.0.1:{port}")).await;
    assert!(
        legit.starts_with("HTTP/1.1 200"),
        "legitimate loopback Host header should succeed with 200, got: {}",
        legit.lines().next().unwrap_or("<empty>")
    );

    let _ = state.proxy_service.stop().await;
}

/// 5. 协议转换不变量：OpenAI Chat 上游返回结构化分段 content 数组（thinking + text）时，
///    Claude `/v1/messages` 流式桥接不得静默丢弃内容。
#[allow(clippy::await_holding_lock)]
#[tokio::test]
async fn e2e_chat_upstream_chunked_thinking_and_text_streams_end_to_end() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    let _home = ensure_test_home();

    let state = create_test_state().expect("create test state");

    let chat_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind chat upstream");
    let chat_addr = chat_listener.local_addr().unwrap();
    let chat_task = tokio::spawn(async move {
        let (mut socket, _) = chat_listener.accept().await.expect("accept chat");
        read_http_request_head(&mut socket).await;

        let sse_body = concat!(
            "data: {\"id\":\"chatcmpl_e2e\",\"object\":\"chat.completion.chunk\",\"created\":1700000000,\"model\":\"mistral-large-4\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":[{\"type\":\"thinking\",\"thinking\":[{\"type\":\"text\",\"text\":\"step-by-step-thought\"}]},{\"type\":\"text\",\"text\":\"final-visible-answer\"}]},\"finish_reason\":null}]}\n\n",
            "data: {\"id\":\"chatcmpl_e2e\",\"object\":\"chat.completion.chunk\",\"created\":1700000000,\"model\":\"mistral-large-4\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":15,\"completion_tokens\":9}}\n\n",
            "data: [DONE]\n\n"
        );
        let headers = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            sse_body.len()
        );
        socket.write_all(headers.as_bytes()).await.unwrap();
        socket.write_all(sse_body.as_bytes()).await.unwrap();
        socket.flush().await.unwrap();
        let _ = socket.shutdown().await;
    });

    let provider = claude_provider(
        "p-chat-mistral",
        "Mistral-Chat",
        &format!("http://{chat_addr}"),
        Some("openai_chat"),
    );
    state
        .db
        .save_provider(&AppType::Claude, &provider)
        .expect("save openai_chat provider");
    state
        .db
        .set_current_provider(&AppType::Claude, "p-chat-mistral")
        .expect("set openai_chat current");

    let (proxy_base, _) = start_proxy_on(&state, "127.0.0.1", false, 0, 5).await;
    let client = reqwest::Client::builder()
        .no_proxy()
        .build()
        .expect("build reqwest");

    let resp = client
        .post(format!("{proxy_base}/v1/messages"))
        .json(&json!({
            "model": "mistral-large-4",
            "max_tokens": 128,
            "stream": true,
            "messages": [{"role": "user", "content": "think and answer"}]
        }))
        .send()
        .await
        .expect("send openai_chat stream request");
    assert_eq!(resp.status(), 200);
    let body = resp.text().await.expect("read translated sse");

    // 解析回来的 Anthropic SSE 里必须同时保留 thinking 与最终 text，不能因为 delta.content 是数组而静默丢弃
    let mut saw_thinking = false;
    let mut saw_text = false;
    for line in body.lines() {
        if let Some(data) = line.strip_prefix("data: ") {
            if let Ok(val) = serde_json::from_str::<Value>(data) {
                if let Some(delta) = val.get("delta") {
                    if delta
                        .get("thinking")
                        .and_then(|v| v.as_str())
                        .is_some_and(|s| s.contains("step-by-step-thought"))
                    {
                        saw_thinking = true;
                    }
                    if delta
                        .get("text")
                        .and_then(|v| v.as_str())
                        .is_some_and(|s| s.contains("final-visible-answer"))
                    {
                        saw_text = true;
                    }
                }
            }
        }
    }
    assert!(
        saw_thinking,
        "translated stream dropped structured thinking chunk, body:\n{body}"
    );
    assert!(
        saw_text,
        "translated stream dropped structured text chunk, body:\n{body}"
    );

    let _ = state.proxy_service.stop().await;
    let _ = chat_task.await;
}
