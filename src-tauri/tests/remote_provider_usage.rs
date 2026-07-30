use std::io::Read;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::time::{Duration, Instant};

use cc_switch_lib::remote::runtime::documented_error_codes;
use cc_switch_protocol::protocol::{
    decode_frame, write_frame, Frame, FrameKind, Hello, HelloAck, RpcRequest, RpcResponse,
};
use serde_json::{json, Value};

fn start_agent(home: &std::path::Path) -> (Child, ChildStdin, ChildStdout) {
    // 通过 quiet cargo run 让测试在 clean target 上也能独立启动真实 Agent；Cargo 的构建
    // 日志只走 stderr，stdout 仍严格保留给二进制协议，-j 1 避免 Windows 构建峰值过高。
    let mut child = Command::new(env!("CARGO"))
        .args([
            "run",
            "--quiet",
            "-j",
            "1",
            "-p",
            "cc-switch-agent",
            "--",
            "--stdio",
        ])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .env("CC_SWITCH_TEST_HOME", home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("启动真实 Agent 进程");
    let stdin = child.stdin.take().expect("Agent stdin");
    let stdout = child.stdout.take().expect("Agent stdout");
    (child, stdin, stdout)
}

fn handshake(stdin: &mut ChildStdin, stdout: &mut ChildStdout) -> HelloAck {
    write_frame(
        stdin,
        &Frame::from_json(
            FrameKind::Hello,
            &Hello {
                app_version: env!("CARGO_PKG_VERSION").to_string(),
                protocol_minor: 0,
            },
        )
        .expect("编码 hello"),
    )
    .expect("写入 hello");
    let frame = decode_frame(stdout).expect("读取 hello_ack");
    assert_eq!(frame.kind, FrameKind::HelloAck);
    frame.json().expect("解析 hello_ack")
}

fn request(
    stdin: &mut ChildStdin,
    stdout: &mut ChildStdout,
    id: &str,
    command: &str,
    args: Value,
) -> RpcResponse {
    write_frame(
        stdin,
        &Frame::from_json(
            FrameKind::Request,
            &RpcRequest {
                id: id.to_string(),
                command: command.to_string(),
                args,
                timeout_ms: 30_000,
                operation_id: Some(format!("operation-{id}")),
            },
        )
        .expect("编码 RPC 请求"),
    )
    .expect("写入 RPC 请求");
    let frame = decode_frame(stdout).expect("读取 RPC 响应");
    assert_eq!(frame.kind, FrameKind::Response);
    let response: RpcResponse = frame.json().expect("解析 RPC 响应");
    assert_eq!(response.id, id);
    response
}

fn successful(response: RpcResponse) -> Value {
    assert!(response.error.is_none(), "RPC 失败: {:?}", response.error);
    response.result.expect("成功响应必须携带 result")
}

fn stop_agent(mut child: Child, stdin: ChildStdin, stdout: ChildStdout) {
    // stdin EOF 是 SSH 会话结束信号；进程退出后才能确认 worker 与数据库句柄均已释放。
    drop(stdin);
    drop(stdout);
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if let Some(status) = child.try_wait().expect("轮询 Agent 退出") {
            assert!(status.success(), "Agent 退出状态: {status}");
            return;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let mut stderr = String::new();
            if let Some(mut pipe) = child.stderr.take() {
                let _ = pipe.read_to_string(&mut stderr);
            }
            panic!("stdin EOF 后 Agent 未退出: {stderr}");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn agent_process_exposes_provider_and_usage_parity_without_leaking_errors() {
    let home = tempfile::tempdir().expect("创建隔离 HOME");
    let session_dir = home.path().join(".claude/projects/remote-provider-usage");
    std::fs::create_dir_all(&session_dir).expect("创建 Claude 会话目录");
    std::fs::write(
        session_dir.join("session.jsonl"),
        json!({
            "type": "assistant",
            "sessionId": "remote-session",
            "timestamp": "2026-07-30T12:00:00Z",
            "message": {
                "id": "remote-message",
                "model": "claude-sonnet-4-5",
                "usage": { "input_tokens": 9, "output_tokens": 4 }
            }
        })
        .to_string(),
    )
    .expect("写入 Usage fixture");

    let (child, mut stdin, mut stdout) = start_agent(home.path());
    let ack = handshake(&mut stdin, &mut stdout);
    for capability in [
        "provider.list",
        "provider.switch",
        "usage.summary",
        "usage.logs",
        "usage.pricing.update",
    ] {
        assert!(
            ack.capabilities.iter().any(|item| item == capability),
            "Agent 缺少能力: {capability}"
        );
    }

    for (id, name) in [("remote-a", "Remote A"), ("remote-b", "Remote B")] {
        assert_eq!(
            successful(request(
                &mut stdin,
                &mut stdout,
                &format!("add-{id}"),
                "provider.add",
                json!({
                    "app": "claude",
                    "addToLive": false,
                    "provider": {
                        "id": id,
                        "name": name,
                        "settingsConfig": {
                            "env": {
                                "ANTHROPIC_BASE_URL": "https://api.example.test",
                                "ANTHROPIC_AUTH_TOKEN": "fixture-secret-must-not-leak"
                            }
                        }
                    }
                }),
            )),
            Value::Bool(true)
        );
    }
    assert_eq!(
        successful(request(
            &mut stdin,
            &mut stdout,
            "switch-provider",
            "provider.switch",
            json!({ "app": "claude", "id": "remote-b" }),
        )),
        json!({ "warnings": [] })
    );
    let providers = successful(request(
        &mut stdin,
        &mut stdout,
        "list-providers",
        "provider.list",
        json!({ "app": "claude" }),
    ));
    assert_eq!(providers["remote-a"]["name"], "Remote A");
    assert_eq!(providers["remote-b"]["name"], "Remote B");
    assert_eq!(
        successful(request(
            &mut stdin,
            &mut stdout,
            "current-provider",
            "provider.current",
            json!({ "app": "claude" }),
        )),
        json!("remote-b")
    );

    let sync = successful(request(
        &mut stdin,
        &mut stdout,
        "sync-sessions",
        "usage.session_sync",
        json!({}),
    ));
    assert_eq!(sync["imported"], 1);
    let summary = successful(request(
        &mut stdin,
        &mut stdout,
        "usage-summary",
        "usage.summary",
        json!({}),
    ));
    assert_eq!(summary["totalRequests"], 1);
    assert_eq!(summary["realTotalTokens"], 13);
    let logs = successful(request(
        &mut stdin,
        &mut stdout,
        "usage-logs",
        "usage.logs",
        json!({ "filters": {}, "page": 0, "pageSize": 20 }),
    ));
    assert_eq!(logs["total"], 1);

    assert_eq!(
        successful(request(
            &mut stdin,
            &mut stdout,
            "pricing-update",
            "usage.pricing.update",
            json!({
                "modelId": "process-priced-model",
                "displayName": "Process Priced Model",
                "inputCost": "1",
                "outputCost": "2",
                "cacheReadCost": "0.1",
                "cacheCreationCost": "0.2"
            }),
        )),
        Value::Bool(true)
    );
    assert_eq!(
        successful(request(
            &mut stdin,
            &mut stdout,
            "pricing-delete",
            "usage.pricing.delete",
            json!({ "modelId": "process-priced-model" }),
        )),
        Value::Bool(true)
    );

    let denied = request(
        &mut stdin,
        &mut stdout,
        "unknown-command",
        "secrets.dump",
        json!({}),
    )
    .error
    .expect("未注册命令必须被拒绝");
    assert_eq!(denied.code, "COMMAND_NOT_EXPOSED");
    assert!(!denied.message.contains("fixture-secret-must-not-leak"));
    assert!(denied.message.chars().count() <= 4_096);
    assert!(
        denied
            .message
            .chars()
            .all(|character| !character.is_control() || matches!(character, '\n' | '\t')),
        "错误消息不得携带终端控制字符"
    );

    stop_agent(child, stdin, stdout);
}

#[test]
fn remote_error_contract_documents_every_cross_layer_code() {
    let documented = documented_error_codes();
    for code in [
        "DATABASE_INCOMPATIBLE",
        "DATABASE_BUSY",
        "COMMAND_NOT_EXPOSED",
        "CAPABILITY_UNAVAILABLE",
        "STALE_RUNTIME",
        "LIVE_WRITE_FAILED",
        "REMOTE_PERMISSION_DENIED",
        "REMOTE_OPERATION_TIMEOUT",
        "REMOTE_OPERATION_CANCELLED",
    ] {
        assert!(documented.contains(&code), "稳定错误码未登记: {code}");
    }
}
