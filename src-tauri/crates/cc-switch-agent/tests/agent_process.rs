use std::io::Read;
use std::process::{ChildStdin, ChildStdout, Command, Stdio};
use std::time::{Duration, Instant};

use cc_switch_core::ProviderRecord;
use cc_switch_protocol::protocol::{
    decode_frame, write_frame, CancelRequest, Frame, FrameKind, Hello, HelloAck, RpcRequest,
    RpcResponse,
};
use serde_json::{json, Value};

fn provider(id: &str, name: &str) -> ProviderRecord {
    // 进程级 fixture 使用完整 Provider DTO，确保 Agent 的 JSON 边界不会遗漏桌面字段。
    ProviderRecord {
        id: id.to_string(),
        name: name.to_string(),
        settings_config: json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://api.example.com",
                "ANTHROPIC_AUTH_TOKEN": format!("sk-{id}")
            }
        }),
        website_url: None,
        category: Some("custom".to_string()),
        created_at: None,
        sort_index: None,
        notes: None,
        icon: None,
        icon_color: None,
        meta: None,
        in_failover_queue: false,
    }
}

fn request(
    stdin: &mut ChildStdin,
    stdout: &mut ChildStdout,
    id: &str,
    command: &str,
    args: Value,
) -> Value {
    let request = RpcRequest {
        id: id.to_string(),
        command: command.to_string(),
        args,
        timeout_ms: 30_000,
        operation_id: None,
    };
    write_frame(
        stdin,
        &Frame::from_json(FrameKind::Request, &request).expect("编码请求"),
    )
    .expect("写入请求");

    let frame = decode_frame(stdout).expect("读取响应帧");
    assert_eq!(frame.kind, FrameKind::Response);
    let response: RpcResponse = frame.json().expect("解析响应");
    assert_eq!(response.id, id);
    assert!(response.error.is_none(), "RPC 错误: {:?}", response.error);
    response.result.expect("响应结果")
}

#[test]
fn stdio_agent_completes_provider_vertical_slice_and_exits_on_eof() {
    let home = tempfile::tempdir().expect("创建隔离 HOME");
    // 进程 fixture 必须位于 Agent 的显式 HOME；若实现误读桌面 HOME，下面的同步结果会保持 0。
    let claude_project = home.path().join(".claude/projects/remote-project");
    std::fs::create_dir_all(&claude_project).expect("创建远端 Claude 会话目录");
    std::fs::write(
        claude_project.join("remote-session.jsonl"),
        serde_json::json!({
            "type": "assistant",
            "sessionId": "remote-claude-session",
            "timestamp": "2026-07-30T12:00:00Z",
            "message": {
                "id": "remote-claude-message",
                "model": "claude-sonnet-4-5",
                "usage": { "input_tokens": 12, "output_tokens": 3 }
            }
        })
        .to_string(),
    )
    .expect("写入远端 Claude fixture");
    let mut child = Command::new(env!("CARGO_BIN_EXE_cc-switch-agent"))
        .arg("--stdio")
        .env("CC_SWITCH_TEST_HOME", home.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("启动独立 Agent");
    let mut stdin = child.stdin.take().expect("Agent stdin");
    let mut stdout = child.stdout.take().expect("Agent stdout");

    write_frame(
        &mut stdin,
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
    let ack = decode_frame(&mut stdout).expect("读取 hello_ack");
    assert_eq!(ack.kind, FrameKind::HelloAck);
    let ack: HelloAck = ack.json().expect("解析 hello_ack");
    assert!(ack.capabilities.contains(&"provider.add".to_string()));
    assert!(ack.capabilities.contains(&"usage.summary".to_string()));
    assert!(ack
        .capabilities
        .contains(&"usage.codex_rebuild".to_string()));
    assert!(ack
        .capabilities
        .contains(&"usage.pricing.update_batch".to_string()));
    assert!(ack
        .capabilities
        .contains(&"usage.models_dev_sync.get".to_string()));

    let empty_summary = request(
        &mut stdin,
        &mut stdout,
        "usage-summary",
        "usage.summary",
        json!({}),
    );
    assert_eq!(empty_summary["totalRequests"], 0);
    assert_eq!(empty_summary["realTotalTokens"], 0);

    let sync = request(
        &mut stdin,
        &mut stdout,
        "usage-session-sync",
        "usage.session_sync",
        json!({}),
    );
    assert_eq!(sync["imported"], 1);
    assert_eq!(sync["filesScanned"], 1);
    let synced_summary = request(
        &mut stdin,
        &mut stdout,
        "usage-summary-after-sync",
        "usage.summary",
        json!({}),
    );
    assert_eq!(synced_summary["totalRequests"], 1);
    assert_eq!(synced_summary["realTotalTokens"], 15);

    assert_eq!(
        request(
            &mut stdin,
            &mut stdout,
            "pricing-update",
            "usage.pricing.update",
            json!({
                "modelId": "remote-priced-model",
                "displayName": "Remote Priced Model",
                "inputCost": "1",
                "outputCost": "2",
                "cacheReadCost": "0.1",
                "cacheCreationCost": "1.25"
            }),
        ),
        Value::Bool(true)
    );
    assert_eq!(
        request(
            &mut stdin,
            &mut stdout,
            "models-dev-config-save",
            "usage.models_dev_sync.save",
            json!({
                "config": {
                    "autoSyncEnabled": true,
                    "includeCommonModels": false,
                    "selectedModelKeys": ["openai:gpt-5"],
                    "excludedCommonModelKeys": [],
                    "lastSyncAt": null,
                    "lastSyncError": null
                }
            }),
        ),
        Value::Bool(true)
    );
    let models_dev_state = request(
        &mut stdin,
        &mut stdout,
        "models-dev-config-get",
        "usage.models_dev_sync.get",
        json!({}),
    );
    assert_eq!(models_dev_state["config"]["autoSyncEnabled"], true);
    assert_eq!(
        request(
            &mut stdin,
            &mut stdout,
            "pricing-update-batch",
            "usage.pricing.update_batch",
            json!({
                "entries": [{
                    "modelId": "remote-batch-model",
                    "displayName": "Remote Batch Model",
                    "inputCostPerMillion": "1",
                    "outputCostPerMillion": "2",
                    "cacheReadCostPerMillion": "0.1",
                    "cacheCreationCostPerMillion": "0.2"
                }]
            }),
        ),
        serde_json::json!(1)
    );
    let pricing = request(
        &mut stdin,
        &mut stdout,
        "pricing-list",
        "usage.pricing.list",
        json!({}),
    );
    assert!(pricing
        .as_array()
        .expect("定价列表")
        .iter()
        .any(|item| item["modelId"] == "remote-priced-model"));
    assert_eq!(
        request(
            &mut stdin,
            &mut stdout,
            "pricing-delete",
            "usage.pricing.delete",
            json!({ "modelId": "remote-priced-model" }),
        ),
        Value::Bool(true)
    );

    for item in [
        provider("remote-a", "Remote A"),
        provider("remote-b", "Remote B"),
    ] {
        assert_eq!(
            request(
                &mut stdin,
                &mut stdout,
                &format!("add-{}", item.id),
                "provider.add",
                json!({ "app": "claude", "provider": item, "addToLive": false }),
            ),
            Value::Bool(true)
        );
    }
    assert_eq!(
        request(
            &mut stdin,
            &mut stdout,
            "switch",
            "provider.switch",
            json!({ "app": "claude", "id": "remote-b" }),
        ),
        json!({ "warnings": [] })
    );
    assert_eq!(
        request(
            &mut stdin,
            &mut stdout,
            "current",
            "provider.current",
            json!({ "app": "claude" }),
        ),
        json!("remote-b")
    );
    let listed = request(
        &mut stdin,
        &mut stdout,
        "list",
        "provider.list",
        json!({ "app": "claude" }),
    );
    assert_eq!(listed["remote-a"]["name"], "Remote A");
    assert_eq!(listed["remote-b"]["name"], "Remote B");

    // SSH 关闭后 stdin EOF 是会话终止信号；Agent 必须及时退出，不能残留后台进程。
    drop(stdin);
    drop(stdout);
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if let Some(status) = child.try_wait().expect("轮询 Agent 退出") {
            assert!(status.success(), "Agent 退出状态: {status}");
            break;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let mut stderr = String::new();
            if let Some(mut pipe) = child.stderr.take() {
                let _ = pipe.read_to_string(&mut stderr);
            }
            panic!("stdin 关闭后 Agent 未退出: {stderr}");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn cli_modes_have_stable_output_and_exit_codes() {
    let version = Command::new(env!("CARGO_BIN_EXE_cc-switch-agent"))
        .arg("--version")
        .output()
        .expect("运行 --version");
    assert!(version.status.success());
    assert_eq!(
        String::from_utf8_lossy(&version.stdout).trim(),
        concat!("cc-switch-agent ", env!("CARGO_PKG_VERSION"))
    );

    let unsupported = Command::new(env!("CARGO_BIN_EXE_cc-switch-agent"))
        .arg("--unknown")
        .output()
        .expect("运行非法参数");
    assert_eq!(unsupported.status.code(), Some(2));
    assert!(
        unsupported.stdout.is_empty(),
        "非法调用不能污染 stdout 协议通道"
    );
}

#[test]
fn cancelled_usage_operation_keeps_agent_session_available() {
    let home = tempfile::tempdir().expect("创建隔离 HOME");
    let sessions = home.path().join(".codex/sessions/2026/07/30");
    std::fs::create_dir_all(&sessions).expect("创建 Codex sessions");
    // 足够多的文件确保 worker 进入可取消扫描阶段；每个文件都只含非计费元数据，
    // 测试不依赖机器速度制造大数据库。
    for index in 0..512 {
        std::fs::write(
            sessions.join(format!("rollout-{index:04}.jsonl")),
            serde_json::json!({
                "timestamp": "2026-07-30T10:00:00Z",
                "type": "session_meta",
                "payload": { "id": format!("session-{index}") }
            })
            .to_string(),
        )
        .expect("写入 Codex 取消 fixture");
    }
    let mut child = Command::new(env!("CARGO_BIN_EXE_cc-switch-agent"))
        .arg("--stdio")
        .env("CC_SWITCH_TEST_HOME", home.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("启动独立 Agent");
    let mut stdin = child.stdin.take().expect("Agent stdin");
    let mut stdout = child.stdout.take().expect("Agent stdout");
    write_frame(
        &mut stdin,
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
    assert_eq!(
        decode_frame(&mut stdout).expect("读取 hello ack").kind,
        FrameKind::HelloAck
    );

    write_frame(
        &mut stdin,
        &Frame::from_json(
            FrameKind::Request,
            &RpcRequest {
                id: "cancel-request".to_string(),
                command: "usage.session_sync".to_string(),
                args: json!({}),
                timeout_ms: 300_000,
                operation_id: Some("cancel-operation".to_string()),
            },
        )
        .expect("编码长任务请求"),
    )
    .expect("写入长任务请求");
    write_frame(
        &mut stdin,
        &Frame::from_json(
            FrameKind::Cancel,
            &CancelRequest {
                request_id: "cancel-request".to_string(),
                operation_id: "cancel-operation".to_string(),
            },
        )
        .expect("编码取消帧"),
    )
    .expect("写入取消帧");

    let response: RpcResponse = decode_frame(&mut stdout)
        .expect("读取取消响应")
        .json()
        .expect("解析取消响应");
    assert_eq!(response.id, "cancel-request");
    assert_eq!(
        response.error.expect("取消错误").code,
        "REMOTE_OPERATION_CANCELLED"
    );

    write_frame(
        &mut stdin,
        &Frame {
            kind: FrameKind::Ping,
            major: cc_switch_protocol::protocol::PROTOCOL_MAJOR,
            payload: b"still-alive".to_vec(),
        },
    )
    .expect("写入取消后 ping");
    assert_eq!(
        decode_frame(&mut stdout).expect("读取 pong").kind,
        FrameKind::Pong
    );
    drop(stdin);
    drop(stdout);
    assert!(child.wait().expect("等待 Agent 退出").success());
}
