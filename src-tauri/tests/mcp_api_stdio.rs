//! Real subprocess contract. No GUI, database, user config or external API.
use cc_switch_lib::mcp_api::{encode_profile, ApiProfile, ApiTool, CONFIG_ENV, HEADERS_ENV};
use serde_json::{json, Value};
use std::{
    io::{BufRead, BufReader, Write},
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc::{channel, Receiver},
    time::{Duration, Instant},
};

struct McpProcess {
    child: Child,
    input: Option<ChildStdin>,
    output: Receiver<Value>,
}
impl McpProcess {
    fn start(tools: Vec<ApiTool>) -> Self {
        let encoded = encode_profile(&ApiProfile { version: 1, tools }).unwrap();
        let mut child = Command::new(env!("CARGO_BIN_EXE_cc-switch"))
            .arg("--mcp-api")
            .env(CONFIG_ENV, encoded)
            .env(HEADERS_ENV, r#"{"X-API-Key":"synthetic-test-key"}"#)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let input = child.stdin.take();
        let stdout = child.stdout.take().unwrap();
        let (sender, output) = channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let message = serde_json::from_str(&line.unwrap()).unwrap();
                if sender.send(message).is_err() {
                    break;
                }
            }
        });
        Self {
            child,
            input,
            output,
        }
    }
    fn send(&mut self, value: Value) {
        let input = self.input.as_mut().unwrap();
        serde_json::to_writer(&mut *input, &value).unwrap();
        input.write_all(b"\n").unwrap();
        input.flush().unwrap();
    }
    fn receive(&self) -> Value {
        self.output
            .recv_timeout(Duration::from_secs(5))
            .expect("MCP subprocess response")
    }
    fn initialize(&mut self) {
        self.send(json!({"jsonrpc":"2.0","id":"init","method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"contract-test","version":"1"}}}));
        assert_eq!(
            self.receive()["result"]["serverInfo"]["name"],
            "cc-switch-api"
        );
        self.send(json!({"jsonrpc":"2.0","method":"notifications/initialized"}));
    }
    fn eof(&mut self) {
        self.input.take();
        let start = Instant::now();
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                assert!(status.success());
                break;
            }
            assert!(
                start.elapsed() < Duration::from_secs(3),
                "MCP process must exit after EOF"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
impl Drop for McpProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
fn tool(name: &str, url: String) -> ApiTool {
    ApiTool {
        read_only: None,
        name: name.into(),
        description: name.into(),
        method: "GET".into(),
        url,
        input_schema: json!({"type":"object","properties":{},"additionalProperties":false}),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stdio_process_initializes_lists_calls_and_exits_without_gui() {
    use axum::{http::HeaderMap, routing::get, Json, Router};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new().route(
                "/echo",
                get(|headers: HeaderMap| async move {
                    Json(json!({"key":headers["x-api-key"].to_str().unwrap()}))
                }),
            ),
        )
        .await
        .unwrap();
    });
    let mut process = McpProcess::start(vec![tool("echo", format!("http://{address}/echo"))]);
    process.initialize();
    process.send(json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}));
    let tools = process.receive();
    assert_eq!(tools["result"]["tools"][0]["name"], "echo");
    assert_eq!(
        tools["result"]["tools"][0]["inputSchema"],
        json!({"type":"object","properties":{},"additionalProperties":false})
    );
    assert!(!tools.to_string().contains("synthetic-test-key"));
    process.send(json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"echo","arguments":{}}}));
    let response = process.receive();
    assert_eq!(response["result"]["isError"], false);
    let content: Value =
        serde_json::from_str(response["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(content["body"]["key"], "synthetic-test-key");
    process.eof();
    server.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stdio_process_bounds_concurrency_keeps_ping_responsive_and_cancels() {
    use axum::{routing::get, Router};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new()
                .route(
                    "/slow",
                    get(|| async {
                        tokio::time::sleep(Duration::from_secs(60)).await;
                        "slow"
                    }),
                )
                .route("/ok", get(|| async { "ok" })),
        )
        .await
        .unwrap();
    });
    let mut process = McpProcess::start(vec![
        tool("slow", format!("http://{address}/slow")),
        tool("ok", format!("http://{address}/ok")),
    ]);
    process.initialize();
    for id in 0..8 {
        process
            .send(json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":"slow"}}));
    }
    process.send(json!({"jsonrpc":"2.0","id":99,"method":"tools/call","params":{"name":"slow"}}));
    process.send(json!({"jsonrpc":"2.0","id":100,"method":"ping"}));
    assert_eq!(process.receive()["error"]["code"], -32000);
    assert_eq!(
        process.receive(),
        json!({"jsonrpc":"2.0","id":100,"result":{}})
    );
    for id in 0..8 {
        process.send(
            json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":id}}),
        );
    }
    for _ in 0..8 {
        assert_eq!(process.receive()["error"]["code"], -32800);
    }
    // Cancellation replies must mean all eight slots are available again.
    // Retrying overload errors here would hide an acknowledgement/cleanup race.
    for id in 200..208 {
        process.send(json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":"ok"}}));
    }
    let mut completed = std::collections::HashSet::new();
    for _ in 0..8 {
        let response = process.receive();
        assert_eq!(
            response["result"]["isError"], false,
            "Cancellation must release call slots before replying: {response}"
        );
        let id = response["id"].as_u64().unwrap();
        assert!((200..208).contains(&id));
        assert!(completed.insert(id));
    }
    assert_eq!(completed.len(), 8);
    process.eof();
    server.abort();
}
