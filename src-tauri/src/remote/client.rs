use std::io::{Read, Write};

use serde_json::Value;

use super::protocol::{
    decode_frame, write_frame, Frame, FrameKind, Hello, HelloAck, ProtocolError, RpcRequest,
    RpcResponse,
};

pub struct RemoteSession<R, W> {
    reader: R,
    writer: W,
    hello_ack: HelloAck,
}

impl<R: Read, W: Write> RemoteSession<R, W> {
    pub fn connect(
        mut reader: R,
        mut writer: W,
        app_version: &str,
    ) -> Result<Self, RemoteClientError> {
        let hello = Hello {
            app_version: app_version.to_string(),
            protocol_minor: 0,
        };
        write_frame(&mut writer, &Frame::from_json(FrameKind::Hello, &hello)?)?;

        let frame = decode_frame(&mut reader)?;
        if frame.kind != FrameKind::HelloAck {
            return Err(RemoteClientError::UnexpectedFrame(frame.kind));
        }
        let hello_ack = frame.json()?;
        Ok(Self {
            reader,
            writer,
            hello_ack,
        })
    }

    pub fn hello_ack(&self) -> &HelloAck {
        &self.hello_ack
    }

    pub fn invoke_with_id(
        &mut self,
        request_id: &str,
        command: &str,
        args: Value,
        timeout_ms: u64,
    ) -> Result<Value, RemoteClientError> {
        let request = RpcRequest {
            id: request_id.to_string(),
            command: command.to_string(),
            args,
            timeout_ms,
            operation_id: None,
        };
        write_frame(
            &mut self.writer,
            &Frame::from_json(FrameKind::Request, &request)?,
        )?;

        loop {
            let frame = decode_frame(&mut self.reader)?;
            match frame.kind {
                FrameKind::Response => {
                    let response: RpcResponse = frame.json()?;
                    if response.id != request_id {
                        return Err(RemoteClientError::ResponseIdMismatch {
                            expected: request_id.to_string(),
                            actual: response.id,
                        });
                    }
                    if let Some(error) = response.error {
                        return Err(RemoteClientError::Remote {
                            code: error.code,
                            message: error.message,
                        });
                    }
                    return response.result.ok_or(RemoteClientError::MissingResult);
                }
                FrameKind::Event => {
                    // 第一阶段没有独立事件订阅者；读取请求响应时先忽略事件帧，
                    // 后续连接管理器会将其转发为 Tauri event。
                }
                other => return Err(RemoteClientError::UnexpectedFrame(other)),
            }
        }
    }

    pub fn into_parts(self) -> (R, W) {
        (self.reader, self.writer)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RemoteClientError {
    #[error(transparent)]
    Protocol(#[from] ProtocolError),
    #[error("远端返回了非预期帧: {0:?}")]
    UnexpectedFrame(FrameKind),
    #[error("远端响应 ID 不匹配，期望 {expected}，实际 {actual}")]
    ResponseIdMismatch { expected: String, actual: String },
    #[error("远端响应缺少 result")]
    MissingResult,
    #[error("远端命令失败 [{code}]: {message}")]
    Remote { code: String, message: String },
}
