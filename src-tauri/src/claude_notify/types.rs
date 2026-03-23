use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ClaudeNotifyEventType {
    PermissionPrompt,
    IdlePrompt,
    Stop,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeNotifyPayload {
    pub source_app: String,
    pub event_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notification_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeNotifyRuntimeStatus {
    pub listening: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
}

impl ClaudeNotifyPayload {
    pub fn normalized_event_type(&self) -> Option<ClaudeNotifyEventType> {
        match self.event_type.as_str() {
            "permission_prompt" => Some(ClaudeNotifyEventType::PermissionPrompt),
            "idle_prompt" => Some(ClaudeNotifyEventType::IdlePrompt),
            "stop" => Some(ClaudeNotifyEventType::Stop),
            _ => None,
        }
    }
}
