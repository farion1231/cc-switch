use std::io::{Read, Write};
use std::path::PathBuf;

use cc_switch_core::{dispatch_provider_command, HeadlessState, SCHEMA_VERSION};
use cc_switch_protocol::capabilities::CommandCapabilityRegistry;
use cc_switch_protocol::protocol::{
    decode_frame, write_frame, Frame, FrameKind, Hello, HelloAck, ProtocolError, RpcError,
    RpcRequest, RpcResponse,
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
    let stdout = std::io::stdout();
    run_session(stdin.lock(), stdout.lock(), &state)
}

pub fn run_session<R: Read, W: Write>(
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
    let mut capabilities = CommandCapabilityRegistry::provider_phase()
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

    while let Some(frame) = read_next(&mut reader)? {
        match frame.kind {
            FrameKind::Request => {
                let request: RpcRequest = frame.json()?;
                let response =
                    match dispatch_provider_command(state, &request.command, request.args) {
                        Ok(result) => RpcResponse {
                            id: request.id,
                            result: Some(result),
                            error: None,
                        },
                        Err(error) => RpcResponse {
                            id: request.id,
                            result: None,
                            error: Some(RpcError {
                                code: error.code().to_string(),
                                message: sanitize_error(&error.to_string()),
                            }),
                        },
                    };
                write_frame(
                    &mut writer,
                    &Frame::from_json(FrameKind::Response, &response)?,
                )?;
            }
            FrameKind::Ping => write_frame(
                &mut writer,
                &Frame {
                    kind: FrameKind::Pong,
                    major: frame.major,
                    payload: frame.payload,
                },
            )?,
            FrameKind::Cancel => {
                // Provider 命令当前均为短同步操作；保留取消帧兼容性，未来长任务接入时再绑定 operationId。
            }
            other => write_protocol_error(
                &mut writer,
                "UNEXPECTED_FRAME",
                &format!("握手后不接受帧类型: {other:?}"),
            )?,
        }
    }
    Ok(())
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
    #[error("无法确定远程用户 HOME")]
    HomeUnavailable,
    #[error(transparent)]
    Core(#[from] cc_switch_core::CoreError),
    #[error(transparent)]
    Protocol(#[from] ProtocolError),
}
