use super::*;
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{
        sse::{Event, Sse},
        IntoResponse, Response,
    },
    routing::{get, post},
    Json, Router,
};
use serde_json::{json, Value};
use std::{
    convert::Infallible,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Mutex,
    },
};

#[derive(Clone, Default)]
struct MockServer {
    messages: Arc<Mutex<Vec<Value>>>,
    sse_sender: Arc<Mutex<Option<tokio::sync::mpsc::UnboundedSender<Event>>>>,
    started: Arc<AtomicUsize>,
    stream_response: bool,
    fail: bool,
    hang: bool,
    foreign_endpoint: Option<String>,
}

fn initialize_result(request: &Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": request["id"], "result": {
        "protocolVersion": "2025-03-26", "capabilities": {},
        "serverInfo": { "name": "connectivity-fixture", "version": "1" }
    }})
}

async fn http_message(
    State(state): State<MockServer>,
    headers: HeaderMap,
    Json(message): Json<Value>,
) -> Response {
    assert_eq!(headers["x-mcp-test"], "configured-header");
    state.started.fetch_add(1, Ordering::SeqCst);
    if state.hang {
        return std::future::pending().await;
    }
    state.messages.lock().unwrap().push(message.clone());
    if state.fail {
        return Json(json!({"ok": true})).into_response();
    }
    if message["method"] == "initialize" {
        let result = initialize_result(&message);
        if state.stream_response {
            return Sse::new(stream::once(async move {
                Ok::<_, Infallible>(Event::default().event("message").data(result.to_string()))
            }))
            .into_response();
        }
        return Json(result).into_response();
    }
    StatusCode::ACCEPTED.into_response()
}

async fn sse_stream(State(state): State<MockServer>, headers: HeaderMap) -> impl IntoResponse {
    assert_eq!(headers["x-mcp-test"], "configured-header");
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    tx.send(
        Event::default()
            .event("endpoint")
            .data(state.foreign_endpoint.as_deref().unwrap_or("/messages")),
    )
    .unwrap();
    *state.sse_sender.lock().unwrap() = Some(tx);
    Sse::new(async_stream::stream! {
        while let Some(event) = rx.recv().await { yield Ok::<_, Infallible>(event); }
    })
}

async fn sse_message(
    State(state): State<MockServer>,
    headers: HeaderMap,
    Json(message): Json<Value>,
) -> StatusCode {
    assert_eq!(headers["x-mcp-test"], "configured-header");
    state.messages.lock().unwrap().push(message.clone());
    if message["method"] == "initialize" {
        state
            .sse_sender
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .send(
                Event::default()
                    .event("message")
                    .data(initialize_result(&message).to_string()),
            )
            .unwrap();
    }
    StatusCode::ACCEPTED
}

async fn mock_http(state: MockServer) -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let router = Router::new()
        .route("/mcp", post(http_message))
        .route("/sse", get(sse_stream))
        .route("/messages", post(sse_message))
        .with_state(state);
    let task = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    (url, task)
}

fn http_spec(url: &str, transport: &str) -> Value {
    json!({"type": transport, "url": url, "headers": {"x-mcp-test": "configured-header"}})
}

#[tokio::test]
async fn http_json_and_sse_responses_require_a_real_mcp_handshake() {
    for stream_response in [false, true] {
        let state = MockServer {
            stream_response,
            ..Default::default()
        };
        let (url, task) = mock_http(state.clone()).await;
        let mut spec = http_spec(&format!("{url}/mcp"), "http");
        spec.as_object_mut().unwrap().remove("type"); // URL-only configs are HTTP.
        assert!(check_server(spec, CHECK_TIMEOUT).await);
        let messages = state.messages.lock().unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0]["method"], "initialize");
        assert_eq!(messages[1]["method"], "notifications/initialized");
        task.abort();
    }
}

#[tokio::test]
async fn legacy_sse_uses_configured_headers_and_closes_after_initialize() {
    let state = MockServer::default();
    let (url, task) = mock_http(state.clone()).await;
    assert!(check_server(http_spec(&format!("{url}/sse"), "sse"), CHECK_TIMEOUT).await);
    assert_eq!(state.messages.lock().unwrap().len(), 2);
    task.abort();
}

#[tokio::test]
async fn legacy_sse_does_not_forward_headers_to_a_foreign_endpoint() {
    let foreign = MockServer::default();
    let (foreign_url, foreign_task) = mock_http(foreign.clone()).await;
    let state = MockServer {
        foreign_endpoint: Some(format!("{foreign_url}/messages")),
        ..Default::default()
    };
    let (url, task) = mock_http(state).await;
    assert!(!check_server(http_spec(&format!("{url}/sse"), "sse"), CHECK_TIMEOUT).await);
    assert!(foreign.messages.lock().unwrap().is_empty());
    foreign_task.abort();
    task.abort();
}

#[tokio::test]
async fn http_success_without_mcp_and_timeouts_are_disconnected() {
    for hang in [false, true] {
        let state = MockServer {
            fail: !hang,
            hang,
            ..Default::default()
        };
        let (url, task) = mock_http(state).await;
        assert!(
            !check_server(
                http_spec(&format!("{url}/mcp"), "http"),
                Duration::from_millis(100)
            )
            .await
        );
        task.abort();
    }
    for value in [
        json!({}),
        json!({"type":"unsupported"}),
        json!({"command":"cc-switch-nonexistent-mcp-test-command"}),
    ] {
        assert!(!check_server(value, CHECK_TIMEOUT).await);
    }
}

fn server(id: usize, spec: Value, enabled: bool) -> McpServer {
    serde_json::from_value(json!({"id": id.to_string(), "name": "test", "server": spec,
        "apps": {"claude": enabled}}))
    .unwrap()
}

async fn wait_until(mut condition: impl FnMut() -> bool) {
    tokio::time::timeout(Duration::from_secs(10), async {
        while !condition() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("condition did not become true");
}

#[tokio::test]
async fn batches_skip_empty_selections_limit_concurrency_and_cancel_queued_checks() {
    let state = MockServer {
        hang: true,
        ..Default::default()
    };
    let (url, task) = mock_http(state.clone()).await;
    let spec = http_spec(&format!("{url}/mcp"), "http");
    let disabled = start_checks(vec![], |_| panic!("empty selection checked"));
    wait_until(|| !BATCHES.lock().unwrap().contains_key(&disabled)).await;
    assert_eq!(state.started.load(Ordering::SeqCst), 0);
    let batch = start_checks(
        (0..8).map(|id| server(id, spec.clone(), true)).collect(),
        |_| true,
    );
    wait_until(|| state.started.load(Ordering::SeqCst) == MAX_CONCURRENT_CHECKS).await;
    cancel_checks(&batch);
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(state.started.load(Ordering::SeqCst), MAX_CONCURRENT_CHECKS);
    assert!(!BATCHES.lock().unwrap().contains_key(&batch));
    task.abort();
}

static STDIO_FIXTURE: Lazy<(tempfile::TempDir, PathBuf)> = Lazy::new(|| {
    let directory = tempfile::tempdir().unwrap();
    let binary = directory
        .path()
        .join(format!("mcp-fixture{}", std::env::consts::EXE_SUFFIX));
    let source =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mcp_connectivity_server.rs");
    let mut command = std::process::Command::new("rustc");
    command.arg(source).arg("-o").arg(&binary);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    (directory, binary)
});

fn stdio_spec(marker: &Path, hang: bool) -> Value {
    json!({"command": STDIO_FIXTURE.1, "args": [if hang { "--hang" } else { "argument with spaces & symbols" }],
        "env": {"MCP_TEST_MARKER": marker}, "cwd": marker.parent().unwrap()})
}

async fn assert_process_released_port(marker: &Path) {
    let address = std::fs::read_to_string(marker).unwrap();
    wait_until(|| std::net::TcpListener::bind(&address).is_ok()).await;
}

#[tokio::test]
async fn stdio_honors_args_env_cwd_and_releases_the_process() {
    let dir = tempfile::tempdir().unwrap();
    let marker = dir.path().join("stdio");
    assert!(check_server(stdio_spec(&marker, false), CHECK_TIMEOUT).await);
    // A probe may terminate the process immediately after flushing initialized;
    // the HTTP/SSE tests separately assert receipt of that notification.
    assert_process_released_port(&marker).await;
    assert_process_released_port(&marker.with_extension("child")).await;
}

#[tokio::test]
async fn batches_allow_stdio_cold_starts_longer_than_ten_seconds() {
    let dir = tempfile::tempdir().unwrap();
    let marker = dir.path().join("slow-start");
    let mut spec = stdio_spec(&marker, false);
    spec["env"]["MCP_TEST_STARTUP_DELAY_MS"] = json!("11000");
    // Exercise the shell launcher used by Windows MCP presets as well.
    #[cfg(windows)]
    {
        spec["command"] = json!("cmd");
        spec["args"] = json!([
            "/d",
            "/c",
            STDIO_FIXTURE.1,
            "argument with spaces & symbols"
        ]);
    }
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    start_checks(vec![server(0, spec, true)], move |result| {
        tx.send(result).is_ok()
    });
    let result = tokio::time::timeout(CHECK_TIMEOUT + Duration::from_secs(5), rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert!(
        result.connected,
        "a healthy cold start must not fail after ten seconds"
    );
    assert_process_released_port(&marker).await;
    assert_process_released_port(&marker.with_extension("child")).await;
}

#[tokio::test]
async fn cancellation_and_timeout_release_stdio_descendants() {
    for cancel in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("stdio");
        let spec = stdio_spec(&marker, true);
        if cancel {
            let batch = start_checks(vec![server(0, spec, true)], |_| true);
            wait_until(|| marker.with_extension("child").exists()).await;
            cancel_checks(&batch);
        } else {
            let check = tokio::spawn(check_server(spec, CHECK_TIMEOUT));
            wait_until(|| marker.with_extension("child").exists()).await;
            assert!(!check.is_finished(), "the fixture must wait for initialize");
            // Resolve the login shell PATH and start both processes before
            // expiring the probe, regardless of the CI runner's startup speed.
            tokio::time::pause();
            tokio::time::advance(CHECK_TIMEOUT).await;
            assert!(!check.await.unwrap());
            tokio::time::resume();
        }
        assert_process_released_port(&marker).await;
        assert_process_released_port(&marker.with_extension("child")).await;
    }
}

#[cfg(unix)]
#[tokio::test]
async fn stdio_login_path_finds_bare_launchers_and_configured_path_wins() {
    let dir = tempfile::tempdir().unwrap();
    let launcher = dir.path().join("mcp-path-fixture");
    std::fs::copy("/usr/bin/env", &launcher).unwrap();
    let login_path = format!("{}:/usr/bin:/bin", dir.path().display());
    for configured_path in [None, Some("/explicit/server/bin:/usr/bin")] {
        let mut command = Command::new("mcp-path-fixture");
        command.env_clear();
        apply_login_shell_path(&mut command, Some(&login_path), "/usr/bin:/bin");
        // The launcher is found using the injected login PATH. Use env's output
        // to also verify the PATH seen by launcher subprocesses (e.g. env node).
        let output = command.output().await.unwrap();
        assert!(output.status.success());
        let output = String::from_utf8(output.stdout).unwrap();
        assert!(output
            .lines()
            .any(|line| line == format!("PATH={login_path}")));
        if let Some(explicit) = configured_path {
            let mut command = Command::new(&launcher);
            apply_login_shell_path(&mut command, Some(&login_path), "/usr/bin:/bin");
            command.envs([("PATH", explicit)]);
            let output = command.output().await.unwrap();
            assert!(output.status.success());
            assert!(String::from_utf8(output.stdout)
                .unwrap()
                .lines()
                .any(|line| line == format!("PATH={explicit}")));
        }
    }
}

#[cfg(unix)]
#[tokio::test]
async fn stdio_keeps_inherited_path_when_login_shell_resolution_fails() {
    let mut command = Command::new("/usr/bin/env");
    command.env("PATH", "/inherited/bin:/usr/bin");
    apply_login_shell_path(&mut command, None, "/inherited/bin:/usr/bin");
    let output = command.output().await.unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .any(|line| line == "PATH=/inherited/bin:/usr/bin"));
}
