//! Codex runtime state machine.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CodexRuntimeState {
    Stopped,
    Launching,
    Injecting,
    Running,
    OrdinaryRunning,
    Degraded,
    StaleLock,
    Unsupported,
}

impl Default for CodexRuntimeState {
    fn default() -> Self {
        Self::Stopped
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CodexRuntimeSnapshot {
    pub state: CodexRuntimeState,
    pub pid: Option<u32>,
    pub cdp_port: Option<u16>,
    pub bridge_port: Option<u16>,
    pub instance_id: Option<String>,
    pub message: Option<String>,
}

impl Default for CodexRuntimeSnapshot {
    fn default() -> Self {
        Self {
            state: CodexRuntimeState::Stopped,
            pid: None,
            cdp_port: None,
            bridge_port: None,
            instance_id: None,
            message: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_state_serializes_snake_case() {
        let json = serde_json::to_string(&CodexRuntimeState::OrdinaryRunning).unwrap();
        assert_eq!(json, "\"ordinary_running\"");
    }
}
