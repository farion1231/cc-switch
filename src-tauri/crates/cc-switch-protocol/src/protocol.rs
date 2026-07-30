use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::io::{Read, Write};

/// 协议魔数用于尽早识别 stdout 中混入的日志或非 CC Switch 数据。
pub const FRAME_MAGIC: [u8; 4] = *b"CCS1";
pub const PROTOCOL_MAJOR: u16 = 1;
pub const MAX_FRAME_BYTES: usize = 16 * 1024 * 1024;
const HEADER_BYTES: usize = 11;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum FrameKind {
    Hello = 1,
    HelloAck = 2,
    Request = 3,
    Response = 4,
    Event = 5,
    Cancel = 6,
    Ping = 7,
    Pong = 8,
    Chunk = 9,
    ChunkAck = 10,
    ProtocolError = 11,
}

impl TryFrom<u8> for FrameKind {
    type Error = ProtocolError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::Hello),
            2 => Ok(Self::HelloAck),
            3 => Ok(Self::Request),
            4 => Ok(Self::Response),
            5 => Ok(Self::Event),
            6 => Ok(Self::Cancel),
            7 => Ok(Self::Ping),
            8 => Ok(Self::Pong),
            9 => Ok(Self::Chunk),
            10 => Ok(Self::ChunkAck),
            11 => Ok(Self::ProtocolError),
            other => Err(ProtocolError::UnknownFrameKind(other)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub kind: FrameKind,
    pub major: u16,
    pub payload: Vec<u8>,
}

impl Frame {
    pub fn from_json<T: Serialize>(kind: FrameKind, value: &T) -> Result<Self, ProtocolError> {
        let payload = serde_json::to_vec(value)?;
        validate_payload_size(payload.len())?;
        Ok(Self {
            kind,
            major: PROTOCOL_MAJOR,
            payload,
        })
    }

    pub fn json_payload<T: DeserializeOwned>(&self) -> Result<T, ProtocolError> {
        serde_json::from_slice(&self.payload).map_err(ProtocolError::Json)
    }

    /// 保留简短别名，调用端读取控制帧时更易扫描。
    pub fn json<T: DeserializeOwned>(&self) -> Result<T, ProtocolError> {
        self.json_payload()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RpcRequest {
    pub id: String,
    pub command: String,
    #[serde(default)]
    pub args: serde_json::Value,
    pub timeout_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation_id: Option<String>,
}

/// request ID 关联一次响应，operation ID 跨超时与取消生命周期定位 worker；
/// 两者同时携带可防止迟到 Cancel 误伤其他请求。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CancelRequest {
    pub request_id: String,
    pub operation_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Hello {
    pub app_version: String,
    pub protocol_minor: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HelloAck {
    pub agent_version: String,
    pub protocol_minor: u16,
    pub schema_version: i32,
    pub platform: String,
    pub architecture: String,
    pub capabilities: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RpcResponse {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RpcError {
    pub code: String,
    pub message: String,
}

#[derive(Debug, thiserror::Error)]
pub enum ProtocolError {
    #[error("协议帧魔数无效")]
    InvalidMagic,
    #[error("不支持的协议主版本: {0}")]
    UnsupportedMajor(u16),
    #[error("未知帧类型: {0}")]
    UnknownFrameKind(u8),
    #[error("协议负载过大: {actual} bytes")]
    PayloadTooLarge { actual: usize },
    #[error("协议 I/O 失败: {0}")]
    Io(#[from] std::io::Error),
    #[error("协议 JSON 失败: {0}")]
    Json(#[from] serde_json::Error),
}

fn validate_payload_size(actual: usize) -> Result<(), ProtocolError> {
    if actual > MAX_FRAME_BYTES {
        return Err(ProtocolError::PayloadTooLarge { actual });
    }
    Ok(())
}

pub fn encode_frame(frame: &Frame) -> Result<Vec<u8>, ProtocolError> {
    validate_payload_size(frame.payload.len())?;
    let payload_len =
        u32::try_from(frame.payload.len()).map_err(|_| ProtocolError::PayloadTooLarge {
            actual: frame.payload.len(),
        })?;

    let mut bytes = Vec::with_capacity(HEADER_BYTES + frame.payload.len());
    bytes.extend_from_slice(&FRAME_MAGIC);
    bytes.push(frame.kind as u8);
    bytes.extend_from_slice(&frame.major.to_be_bytes());
    bytes.extend_from_slice(&payload_len.to_be_bytes());
    bytes.extend_from_slice(&frame.payload);
    Ok(bytes)
}

pub fn write_frame<W: Write>(writer: &mut W, frame: &Frame) -> Result<(), ProtocolError> {
    let bytes = encode_frame(frame)?;
    writer.write_all(&bytes)?;
    writer.flush()?;
    Ok(())
}

pub fn decode_frame<R: Read>(reader: &mut R) -> Result<Frame, ProtocolError> {
    let mut header = [0_u8; HEADER_BYTES];
    reader.read_exact(&mut header)?;
    if header[..4] != FRAME_MAGIC {
        return Err(ProtocolError::InvalidMagic);
    }

    let kind = FrameKind::try_from(header[4])?;
    let major = u16::from_be_bytes([header[5], header[6]]);
    if major != PROTOCOL_MAJOR {
        return Err(ProtocolError::UnsupportedMajor(major));
    }

    let payload_len = u32::from_be_bytes([header[7], header[8], header[9], header[10]]) as usize;
    validate_payload_size(payload_len)?;
    let mut payload = vec![0_u8; payload_len];
    reader.read_exact(&mut payload)?;

    Ok(Frame {
        kind,
        major,
        payload,
    })
}
