use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use cc_switch_core::{
    dispatch_command_with_cancellation, HeadlessState, OperationCancellation, SCHEMA_VERSION,
};
use cc_switch_protocol::capabilities::CommandCapabilityRegistry;
use cc_switch_protocol::protocol::{
    decode_frame, write_frame, CancelRequest, Frame, FrameKind, Hello, HelloAck, ProtocolError,
    RpcError, RpcRequest, RpcResponse,
};

pub const EXIT_SUCCESS: i32 = 0;
pub const EXIT_SESSION_ERROR: i32 = 1;
pub const EXIT_USAGE: i32 = 2;

/// 解析极小命令行面，避免引入 clap 等额外依赖；参数集合固定后也能让错误退出码长期稳定。
pub fn run_cli(args: impl IntoIterator<Item = String>) -> i32 {
    let args = args.into_iter().collect::<Vec<_>>();
    match args.as_slice() {
        [mode] if mode == "--version" => {
            println!("cc-switch-agent {}", env!("CARGO_PKG_VERSION"));
            EXIT_SUCCESS
        }
        [mode] if mode == "--stdio" => match run_stdio() {
            Ok(()) => EXIT_SUCCESS,
            Err(error) => {
                // stdout 是协议专用通道；任何诊断写入都会导致桌面端误读帧头。
                eprintln!("cc-switch-agent: {error}");
                EXIT_SESSION_ERROR
            }
        },
        _ => {
            eprintln!("usage: cc-switch-agent --stdio | --version");
            EXIT_USAGE
        }
    }
}

pub fn run_stdio() -> Result<(), AgentError> {
    let home = resolve_home()?;
    let state = HeadlessState::open(home)?;
    let stdin = std::io::stdin();
    // `Stdout` 可安全移交 scoped worker；`StdoutLock` 不能跨线程发送。
    run_session(stdin.lock(), std::io::stdout(), &state)
}

#[derive(Clone)]
struct ActiveOperation {
    request_id: String,
    cancellation: OperationCancellation,
}

type SharedWriter<W> = Arc<Mutex<W>>;

pub fn run_session<R: Read, W: Write + Send>(
    mut reader: R,
    mut writer: W,
    state: &HeadlessState,
) -> Result<(), AgentError> {
    let first = match read_next(&mut reader)? {
        Some(frame) => frame,
        None => return Ok(()),
    };
    if first.kind != FrameKind::Hello {
        write_protocol_error(&mut writer, "PROTOCOL_ORDER", "首个协议帧必须是 hello")?;
        return Ok(());
    }

    let _hello: Hello = first.json()?;
    let mut capabilities = CommandCapabilityRegistry::remote_supported()
        .names()
        .map(str::to_string)
        .collect::<Vec<_>>();
    capabilities.sort();
    let ack = HelloAck {
        agent_version: env!("CARGO_PKG_VERSION").to_string(),
        protocol_minor: 0,
        schema_version: SCHEMA_VERSION,
        platform: std::env::consts::OS.to_string(),
        architecture: std::env::consts::ARCH.to_string(),
        capabilities,
    };
    write_frame(&mut writer, &Frame::from_json(FrameKind::HelloAck, &ack)?)?;

    let writer = Arc::new(Mutex::new(writer));
    let operations = Arc::new(Mutex::new(HashMap::new()));
    std::thread::scope(|scope| -> Result<(), AgentError> {
        while let Some(frame) = read_next(&mut reader)? {
            match frame.kind {
                FrameKind::Request => {
                    let request: RpcRequest = frame.json()?;
                    let operation_id = request
                        .operation_id
                        .clone()
                        .unwrap_or_else(|| request.id.clone());
                    let cancellation = OperationCancellation::active();
                    {
                        let mut registry = operations
                            .lock()
                            .map_err(|_| AgentError::StatePoisoned("operations"))?;
                        if registry.contains_key(&operation_id) {
                            let response = RpcResponse {
                                id: request.id,
                                result: None,
                                error: Some(RpcError {
                                    code: "INVALID_ARGUMENT".to_string(),
                                    message: "operationId 正在使用".to_string(),
                                }),
                            };
                            write_shared_response(&writer, &response)?;
                            continue;
                        }
                        registry.insert(
                            operation_id.clone(),
                            ActiveOperation {
                                request_id: request.id.clone(),
                                cancellation: cancellation.clone(),
                            },
                        );
                    }

                    let worker_writer = Arc::clone(&writer);
                    let worker_operations = Arc::clone(&operations);
                    scope.spawn(move || {
                        let response = match dispatch_command_with_cancellation(
                            state,
                            &request.command,
                            request.args,
                            &cancellation,
                        ) {
                            Ok(result) => RpcResponse {
                                id: request.id.clone(),
                                result: Some(result),
                                error: None,
                            },
                            Err(error) => RpcResponse {
                                id: request.id.clone(),
                                result: None,
                                error: Some(RpcError {
                                    code: error.code().to_string(),
                                    message: sanitize_error(&error.to_string()),
                                }),
                            },
                        };
                        if let Ok(mut registry) = worker_operations.lock() {
                            if registry
                                .get(&operation_id)
                                .is_some_and(|active| active.request_id == request.id)
                            {
                                registry.remove(&operation_id);
                            }
                        }
                        // SSH 断线会让写入失败；主 reader 随后收到 EOF 并取消其它 worker，
                        // 此处不能 panic 或污染 stdout。
                        let _ = write_shared_response(&worker_writer, &response);
                    });
                }
                FrameKind::Ping => write_shared_frame(
                    &writer,
                    &Frame {
                        kind: FrameKind::Pong,
                        major: frame.major,
                        payload: frame.payload,
                    },
                )?,
                FrameKind::Cancel => {
                    let cancel: CancelRequest = frame.json()?;
                    let cancellation = operations
                        .lock()
                        .map_err(|_| AgentError::StatePoisoned("operations"))?
                        .get(&cancel.operation_id)
                        .filter(|active| active.request_id == cancel.request_id)
                        .map(|active| active.cancellation.clone());
                    if let Some(cancellation) = cancellation {
                        cancellation.cancel();
                    }
                }
                other => write_shared_protocol_error(
                    &writer,
                    "UNEXPECTED_FRAME",
                    &format!("握手后不接受帧类型: {other:?}"),
                )?,
            }
        }

        // stdin EOF 表示会话结束；先通知所有 worker，再离开 scope 等待其完成，避免
        // 临时 Agent 退出后仍有线程持有数据库或 HOME 文件句柄。
        let cancellations = operations
            .lock()
            .map_err(|_| AgentError::StatePoisoned("operations"))?
            .values()
            .map(|active| active.cancellation.clone())
            .collect::<Vec<_>>();
        for cancellation in cancellations {
            cancellation.cancel();
        }
        Ok(())
    })
}

fn resolve_home() -> Result<PathBuf, AgentError> {
    ["CC_SWITCH_TEST_HOME", "HOME", "USERPROFILE"]
        .into_iter()
        .find_map(std::env::var_os)
        .map(PathBuf::from)
        .ok_or(AgentError::HomeUnavailable)
}

fn read_next<R: Read>(reader: &mut R) -> Result<Option<Frame>, AgentError> {
    match decode_frame(reader) {
        Ok(frame) => Ok(Some(frame)),
        Err(ProtocolError::Io(error)) if error.kind() == std::io::ErrorKind::UnexpectedEof => {
            Ok(None)
        }
        Err(error) => Err(AgentError::Protocol(error)),
    }
}

fn write_shared_response<W: Write>(
    writer: &SharedWriter<W>,
    response: &RpcResponse,
) -> Result<(), AgentError> {
    write_shared_frame(writer, &Frame::from_json(FrameKind::Response, response)?)
}

fn write_shared_frame<W: Write>(writer: &SharedWriter<W>, frame: &Frame) -> Result<(), AgentError> {
    let mut writer = writer
        .lock()
        .map_err(|_| AgentError::StatePoisoned("writer"))?;
    write_frame(&mut *writer, frame).map_err(AgentError::from)
}

fn write_protocol_error<W: Write>(
    writer: &mut W,
    code: &str,
    message: &str,
) -> Result<(), AgentError> {
    let payload = serde_json::json!({
        "code": code,
        "message": sanitize_error(message),
    });
    write_frame(
        writer,
        &Frame::from_json(FrameKind::ProtocolError, &payload)?,
    )?;
    Ok(())
}

fn write_shared_protocol_error<W: Write>(
    writer: &SharedWriter<W>,
    code: &str,
    message: &str,
) -> Result<(), AgentError> {
    let payload = serde_json::json!({
        "code": code,
        "message": sanitize_error(message),
    });
    write_shared_frame(
        writer,
        &Frame::from_json(FrameKind::ProtocolError, &payload)?,
    )
}

/// 错误会展示在本地 UI 和日志中，必须剥离控制字符并限制长度，避免终端转义和日志膨胀。
fn sanitize_error(message: &str) -> String {
    message
        .chars()
        .filter(|character| !character.is_control() || *character == '\n' || *character == '\t')
        .take(4_096)
        .collect()
}

#[derive(Debug, thiserror::Error)]
pub enum AgentError {
    #[error("无法确定远端用户 HOME")]
    HomeUnavailable,
    #[error("Agent 状态锁已损坏: {0}")]
    StatePoisoned(&'static str),
    #[error(transparent)]
    Core(#[from] cc_switch_core::CoreError),
    #[error(transparent)]
    Protocol(#[from] ProtocolError),
}
