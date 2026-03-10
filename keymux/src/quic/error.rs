use thiserror::Error;

#[derive(Debug, Error)]
pub enum QuicError {
    #[error(transparent)]
    Connection(#[from] ConnectionError),
    #[error(transparent)]
    Stream(#[from] StreamError),
    #[error(transparent)]
    Protocol(#[from] ProtocolError),
    #[error(transparent)]
    Transport(#[from] TransportError),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Error)]
pub enum ConnectionError {
    #[error("QUIC connection not established")]
    NotConnected,
    #[error("QUIC connection already closed")]
    ConnectionClosed,
    #[error("Connection flow control blocked: window={window_size}, attempted={attempted}")]
    FlowControlBlocked { window_size: u64, attempted: u64 },
    #[error("QUIC handshake failed: {0:?}")]
    HandshakeFailed(#[source] Option<Box<dyn std::error::Error + Send + Sync>>),
    #[error("Invalid state: {0}")]
    InvalidState(String),
}

#[derive(Debug, Error)]
pub enum StreamError {
    #[error("Stream {stream_id} not found")]
    StreamNotFound { stream_id: u64 },
    #[error("Stream {stream_id} is closed")]
    StreamClosed { stream_id: u64 },
    #[error("Stream {stream_id} flow control blocked: window={window_id}, attempted={attempted}")]
    FlowControlBlocked {
        stream_id: u64,
        window_id: u64,
        attempted: u64,
    },
    #[error("Invalid stream ID: {stream_id}")]
    InvalidStreamId { stream_id: u64 },
    #[error("Maximum number of streams exceeded")]
    StreamLimitExceeded,
}

#[derive(Debug, Error)]
pub enum ProtocolError {
    #[error("Invalid packet: {0}")]
    InvalidPacket(String),
    #[error("QUIC version mismatch: local={local}, remote={remote}")]
    VersionMismatch { local: u64, remote: u64 },
    #[error("Crypto error: {0}")]
    Crypto(
        String,
        #[source] Option<Box<dyn std::error::Error + Send + Sync>>,
    ),
    #[error("Invalid stream ID: {0}")]
    InvalidStreamId(u64),
}

#[derive(Debug, Error)]
pub enum TransportError {
    #[error("Network error: {0}")]
    Network(
        String,
        #[source] Option<Box<dyn std::error::Error + Send + Sync>>,
    ),
    #[error("Packet size {size} exceeds MTU {mtu}")]
    PacketTooLarge { size: usize, mtu: usize },
}
