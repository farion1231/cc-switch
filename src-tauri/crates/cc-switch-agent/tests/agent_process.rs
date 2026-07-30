use std::io::Read;
use std::process::{ChildStdin, ChildStdout, Command, Stdio};
use std::time::{Duration, Instant};

use cc_switch_core::ProviderRecord;
use cc_switch_protocol::protocol::{
    decode_frame, write_frame, Frame, FrameKind, Hello, HelloAck, RpcRequest, RpcResponse,
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
