use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::Value;

use super::protocol::{
    decode_frame, write_frame, CancelRequest, Frame, FrameKind, Hello, HelloAck, ProtocolError,
    RpcRequest, RpcResponse,
};

type PendingResponse = Result<RpcResponse, RemoteClientError>;
type PendingRegistry = Arc<Mutex<HashMap<String, Sender<PendingResponse>>>>;

pub struct RemoteSession<R, W> {
    // 握手同步读取；第一条请求注册 pending 后，reader 才移交后台线程，避免预置
    // 响应或极快服务端在注册前回包造成丢失。
    reader: Mutex<Option<R>>,
    writer: Mutex<W>,
    pending: PendingRegistry,
    event_receiver: Mutex<Receiver<Frame>>,
    event_sender: Sender<Frame>,
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
        let (event_sender, event_receiver) = mpsc::channel();
        Ok(Self {
            reader: Mutex::new(Some(reader)),
            writer: Mutex::new(writer),
            pending: Arc::new(Mutex::new(HashMap::new())),
            event_receiver: Mutex::new(event_receiver),
            event_sender,
            hello_ack,
        })
    }

    pub fn hello_ack(&self) -> &HelloAck {
        &self.hello_ack
    }

    /// 测试和受控 transport 可取回尚未移交的 reader 与 writer；一旦请求启动后台
    /// reader，返回的 reader 为 `None`，writer 仍完整保留所有客户端帧。
    pub fn into_parts(self) -> (Option<R>, W) {
        let reader = self
            .reader
            .into_inner()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let writer = self
            .writer
            .into_inner()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        (reader, writer)
    }
}

impl<R, W> RemoteSession<R, W>
where
    R: Read + Send + 'static,
    W: Write + Send,
{
    pub fn invoke_with_id(
        &self,
        request_id: &str,
        operation_id: &str,
        command: &str,
        args: Value,
        timeout_ms: u64,
    ) -> Result<Value, RemoteClientError> {
        let (sender, receiver) = mpsc::channel();
        {
            let mut pending = self
                .pending
                .lock()
                .map_err(|_| RemoteClientError::StatePoisoned("pending"))?;
            match pending.entry(request_id.to_string()) {
                std::collections::hash_map::Entry::Occupied(_) => {
                    return Err(RemoteClientError::DuplicateRequestId(
                        request_id.to_string(),
                    ));
                }
                std::collections::hash_map::Entry::Vacant(entry) => {
                    entry.insert(sender);
                }
            }
        }
        if let Err(error) = self.start_reader() {
            self.remove_pending(request_id);
            return Err(error);
        }

        let request = RpcRequest {
            id: request_id.to_string(),
            command: command.to_string(),
            args,
            timeout_ms,
            operation_id: Some(operation_id.to_string()),
        };
        let request_frame = match Frame::from_json(FrameKind::Request, &request) {
            Ok(frame) => frame,
            Err(error) => {
                self.remove_pending(request_id);
                return Err(RemoteClientError::Protocol(error));
            }
        };
        if let Err(error) = self.write(&request_frame) {
            self.remove_pending(request_id);
            return Err(error);
        }

        match receiver.recv_timeout(Duration::from_millis(timeout_ms)) {
            Ok(response) => response.and_then(response_value),
            Err(RecvTimeoutError::Timeout) => {
                self.remove_pending(request_id);
                let cancel = CancelRequest {
                    request_id: request_id.to_string(),
                    operation_id: operation_id.to_string(),
                };
                // 调用方主错误已经确定为超时；Cancel 写失败通常意味着连接同时断开，
                // 不应把稳定超时码替换成偶发 I/O 文本。
                if let Ok(frame) = Frame::from_json(FrameKind::Cancel, &cancel) {
                    let _ = self.write(&frame);
                }
                Err(RemoteClientError::Timeout {
                    request_id: request_id.to_string(),
                    operation_id: operation_id.to_string(),
                })
            }
            Err(RecvTimeoutError::Disconnected) => {
                Err(RemoteClientError::Offline("远端响应通道已关闭".to_string()))
            }
        }
    }

    pub fn try_recv_event(&self) -> Result<Option<Frame>, RemoteClientError> {
        let receiver = self
            .event_receiver
            .lock()
            .map_err(|_| RemoteClientError::StatePoisoned("events"))?;
        match receiver.try_recv() {
            Ok(frame) => Ok(Some(frame)),
            Err(mpsc::TryRecvError::Empty | mpsc::TryRecvError::Disconnected) => Ok(None),
        }
    }

    fn start_reader(&self) -> Result<(), RemoteClientError> {
        let reader = self
            .reader
            .lock()
            .map_err(|_| RemoteClientError::StatePoisoned("reader"))?
            .take();
        let Some(reader) = reader else {
            return Ok(());
        };
        let pending = Arc::clone(&self.pending);
        let events = self.event_sender.clone();
        std::thread::spawn(move || reader_loop(reader, pending, events));
        Ok(())
    }

    fn write(&self, frame: &Frame) -> Result<(), RemoteClientError> {
        let mut writer = self
            .writer
            .lock()
            .map_err(|_| RemoteClientError::StatePoisoned("writer"))?;
        write_frame(&mut *writer, frame).map_err(RemoteClientError::from)
    }

    fn remove_pending(&self, request_id: &str) {
        if let Ok(mut pending) = self.pending.lock() {
            pending.remove(request_id);
        }
    }
}

fn response_value(response: RpcResponse) -> Result<Value, RemoteClientError> {
    if let Some(error) = response.error {
        return Err(RemoteClientError::Remote {
            code: error.code,
            message: error.message,
        });
    }
    response.result.ok_or(RemoteClientError::MissingResult)
}

fn reader_loop<R: Read>(mut reader: R, pending: PendingRegistry, events: Sender<Frame>) {
    loop {
        let frame = match decode_frame(&mut reader) {
            Ok(frame) => frame,
            Err(error) => {
                fail_pending(&pending, format!("远端连接读取失败: {error}"));
                return;
            }
        };
        match frame.kind {
            FrameKind::Response => match frame.json::<RpcResponse>() {
                Ok(response) => {
                    let sender = pending
                        .lock()
                        .ok()
                        .and_then(|mut pending| pending.remove(&response.id));
                    if let Some(sender) = sender {
                        let _ = sender.send(Ok(response));
                    }
                    // 无 pending 的响应属于超时后的迟到包或未知 ID，必须丢弃，不能
                    // 把它当作另一个并发调用的协议错误。
                }
                Err(error) => {
                    fail_pending(&pending, format!("远端响应解析失败: {error}"));
                    return;
                }
            },
            FrameKind::Event => {
                let _ = events.send(frame);
            }
            other => {
                fail_pending(&pending, format!("远端返回了非预期帧: {other:?}"));
                return;
            }
        }
    }
}

fn fail_pending(pending: &PendingRegistry, message: String) {
    let senders = pending
        .lock()
        .map(|mut pending| {
            pending
                .drain()
                .map(|(_, sender)| sender)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    for sender in senders {
        let _ = sender.send(Err(RemoteClientError::Offline(message.clone())));
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RemoteClientError {
    #[error(transparent)]
    Protocol(#[from] ProtocolError),
    #[error("远端返回了非预期帧: {0:?}")]
    UnexpectedFrame(FrameKind),
    #[error("远端响应缺少 result")]
    MissingResult,
    #[error("远端命令失败 [{code}]: {message}")]
    Remote { code: String, message: String },
    #[error("远端操作超时: request={request_id}, operation={operation_id}")]
    Timeout {
        request_id: String,
        operation_id: String,
    },
    #[error("远端连接离线: {0}")]
    Offline(String),
    #[error("远端请求 ID 重复: {0}")]
    DuplicateRequestId(String),
    #[error("远端客户端状态锁已损坏: {0}")]
    StatePoisoned(&'static str),
}

impl RemoteClientError {
    pub fn code(&self) -> &str {
        match self {
            Self::Timeout { .. } => "REMOTE_OPERATION_TIMEOUT",
            Self::Offline(_) => "REMOTE_OFFLINE",
            Self::Remote { code, .. } => code,
            _ => "REMOTE_CONNECTION_ERROR",
        }
    }
}
