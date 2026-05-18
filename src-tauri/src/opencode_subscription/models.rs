use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OpenCodeSubscriptionKind {
    Go,
    Zen,
}

impl OpenCodeSubscriptionKind {
    pub fn provider_type(&self) -> &'static str {
        match self {
            Self::Go => "opencode_go_subscription",
            Self::Zen => "opencode_zen_subscription",
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Go => "OpenCode Go",
            Self::Zen => "OpenCode Zen",
        }
    }
}

impl fmt::Display for OpenCodeSubscriptionKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Go => "go",
            Self::Zen => "zen",
        })
    }
}

impl FromStr for OpenCodeSubscriptionKind {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "go" | "opencode_go_subscription" => Ok(Self::Go),
            "zen" | "opencode_zen_subscription" => Ok(Self::Zen),
            _ => Err(format!("unknown OpenCode subscription kind: {value}")),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveOpenCodeSubscriptionProviderRequest {
    pub provider_id: Option<String>,
    pub name: Option<String>,
    pub subscription_kind: OpenCodeSubscriptionKind,
    pub base_url: String,
    pub api_key: String,
    pub default_model: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenCodeSubscriptionProviderRecord {
    pub id: String,
    pub provider_id: String,
    pub subscription_kind: OpenCodeSubscriptionKind,
    pub base_url: String,
    pub api_key_ref: String,
    pub local_adapter_enabled: bool,
    pub default_model: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenCodeSubscriptionConnectionResult {
    pub success: bool,
    pub provider_id: String,
    pub status: Option<u16>,
    pub latency_ms: u128,
    pub message: String,
    pub models: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenCodeSubscriptionStreamResult {
    pub success: bool,
    pub provider_id: String,
    pub status: Option<u16>,
    pub latency_ms: u128,
    pub first_event: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OpenCodeSubscriptionError {
    pub code: String,
    pub message: String,
    pub suggestion: String,
    pub details: Option<String>,
}

impl OpenCodeSubscriptionError {
    pub fn new(code: &str, message: impl Into<String>, suggestion: impl Into<String>) -> Self {
        Self {
            code: code.to_string(),
            message: message.into(),
            suggestion: suggestion.into(),
            details: None,
        }
    }

    pub fn with_details(mut self, details: impl Into<String>) -> Self {
        self.details = Some(details.into());
        self
    }
}
