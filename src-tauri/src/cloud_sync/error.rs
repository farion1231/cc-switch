use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Serialize, Deserialize)]
pub enum CloudSyncError {
    // Network and API errors
    Network(String),
    Api(String),
    Authentication(String),
    RateLimit(String),
    NotFound(String),

    // Encryption errors
    Encryption(String),
    Decryption(String),

    // File system and data errors
    Io(String),
    Parse(String),

    // Configuration and validation
    Configuration(String),
    Validation(String),
    Conflict(String),

    // Operation control
    OperationCancelled,
    Unknown(String),
}

impl fmt::Display for CloudSyncError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Network(msg) => write!(f, "Network error: {}", msg),
            Self::Api(msg) => write!(f, "API error: {}", msg),
            Self::Authentication(msg) => write!(f, "Authentication error: {}", msg),
            Self::RateLimit(msg) => write!(f, "Rate limit error: {}", msg),
            Self::NotFound(msg) => write!(f, "Not found: {}", msg),
            Self::Encryption(msg) => write!(f, "Encryption failed: {}", msg),
            Self::Decryption(msg) => write!(f, "Decryption failed: {}", msg),
            Self::Io(msg) => write!(f, "IO error: {}", msg),
            Self::Parse(msg) => write!(f, "Parse error: {}", msg),
            Self::Configuration(msg) => write!(f, "Configuration error: {}", msg),
            Self::Validation(msg) => write!(f, "Validation error: {}", msg),
            Self::Conflict(msg) => write!(f, "Conflict: {}", msg),
            Self::OperationCancelled => write!(f, "Operation was cancelled"),
            Self::Unknown(msg) => write!(f, "Unknown error: {}", msg),
        }
    }
}

impl std::error::Error for CloudSyncError {}

impl From<std::io::Error> for CloudSyncError {
    fn from(err: std::io::Error) -> Self {
        CloudSyncError::Io(err.to_string())
    }
}

impl From<reqwest::Error> for CloudSyncError {
    fn from(err: reqwest::Error) -> Self {
        CloudSyncError::Network(err.to_string())
    }
}

impl From<serde_json::Error> for CloudSyncError {
    fn from(err: serde_json::Error) -> Self {
        CloudSyncError::Parse(err.to_string())
    }
}