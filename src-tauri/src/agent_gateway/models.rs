use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentInstance {
    pub id: String,
    pub name: String,
    pub runtime: AgentRuntimeKind,
    pub provider_id: String,
    pub provider_name: Option<String>,
    pub model: Option<String>,
    pub launch_mode: AgentLaunchMode,
    pub run_profile_id: String,
    pub port: u16,
    pub cwd: Option<String>,
    pub pid: Option<u32>,
    pub window_title: Option<String>,
    pub session_id: Option<String>,
    pub status: AgentStatus,
    pub created_at: String,
    pub started_at: Option<String>,
    pub stopped_at: Option<String>,
    pub last_error: Option<String>,
    pub deleted_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LaunchAgentRequest {
    pub name: String,
    pub runtime: AgentRuntimeKind,
    pub provider_id: String,
    pub provider_mode: Option<AgentProviderMode>,
    pub model: Option<String>,
    pub claude_entry_model: Option<String>,
    pub upstream_provider_model: Option<String>,
    pub run_profile_id: Option<String>,
    pub cwd: Option<String>,
    pub session_id: Option<String>,
    pub launch_strategy: Option<LaunchStrategy>,
    pub permission_mode: Option<AgentPermissionMode>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentProviderMode {
    SelectedProvider,
    CurrentCcSwitchProvider,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSnapshotRequest {
    pub provider_id: Option<String>,
    pub provider_mode: Option<AgentProviderMode>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderRuntimeSnapshot {
    pub provider_id: String,
    pub provider_name: String,
    pub provider_type: String,
    pub app_type: String,
    pub base_url: String,
    pub redacted_base_url: String,
    pub auth_token_present: bool,
    pub api_format: Option<String>,
    pub upstream_models: Vec<String>,
    pub default_upstream_model: Option<String>,
    pub redacted_settings_config_json: String,
    pub provider_config_hash: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RestartAgentRequest {
    pub launch_strategy: Option<LaunchStrategy>,
    pub permission_mode: Option<AgentPermissionMode>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LaunchStrategy {
    WindowsTerminal,
    PowerShellWindow,
    BackgroundProcess,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentPermissionMode {
    Default,
    Plan,
    AcceptEdits,
    Auto,
    DontAsk,
    BypassPermissions,
}

impl AgentPermissionMode {
    pub fn claude_args(&self) -> Vec<String> {
        match self {
            Self::Default => Vec::new(),
            Self::BypassPermissions => vec!["--dangerously-skip-permissions".to_string()],
            Self::Plan => vec!["--permission-mode".to_string(), "plan".to_string()],
            Self::AcceptEdits => vec!["--permission-mode".to_string(), "acceptEdits".to_string()],
            Self::Auto => vec!["--permission-mode".to_string(), "auto".to_string()],
            Self::DontAsk => vec!["--permission-mode".to_string(), "dontAsk".to_string()],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LaunchCommandPreview {
    pub strategy: LaunchStrategy,
    pub program: String,
    pub args_redacted: Vec<String>,
    pub cwd: Option<String>,
    pub window_title: String,
    pub env_keys: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RunProfile {
    pub id: String,
    pub name: String,
    pub runtime: AgentRuntimeKind,
    pub kind: RunProfileKind,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub allow_custom_profiles: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentLog {
    pub id: String,
    pub agent_id: String,
    pub level: String,
    pub event: String,
    pub message: Option<String>,
    pub payload_json: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentCommandError {
    pub code: String,
    pub message: String,
    pub suggestion: String,
    pub details: Option<String>,
}

impl AgentCommandError {
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentRuntimeKind {
    ClaudeCode,
    Codex,
    OpenCode,
    OpenClaw,
    Gemini,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentLaunchMode {
    New,
    Resume,
}

impl fmt::Display for AgentLaunchMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::New => "new",
            Self::Resume => "resume",
        })
    }
}

impl FromStr for AgentLaunchMode {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "new" | "New" => Ok(Self::New),
            "resume" | "Resume" => Ok(Self::Resume),
            _ => Err(format!("unknown agent launch mode: {value}")),
        }
    }
}

impl fmt::Display for AgentRuntimeKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::ClaudeCode => "claude_code",
            Self::Codex => "codex",
            Self::OpenCode => "opencode",
            Self::OpenClaw => "openclaw",
            Self::Gemini => "gemini",
        })
    }
}

impl FromStr for AgentRuntimeKind {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "claude_code" | "ClaudeCode" => Ok(Self::ClaudeCode),
            "codex" | "Codex" => Ok(Self::Codex),
            "opencode" | "OpenCode" => Ok(Self::OpenCode),
            "openclaw" | "OpenClaw" => Ok(Self::OpenClaw),
            "gemini" | "Gemini" => Ok(Self::Gemini),
            _ => Err(format!("unknown agent runtime: {value}")),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentStatus {
    Created,
    Launching,
    Running,
    Stopping,
    Stopped,
    Failed,
    Exited,
    Killed,
}

impl AgentStatus {
    pub fn can_transition_to(self, next: Self) -> bool {
        use AgentStatus::*;
        matches!(
            (self, next),
            (Created, Launching)
                | (Launching, Running)
                | (Launching, Failed)
                | (Launching, Stopping)
                | (Running, Stopping)
                | (Running, Exited)
                | (Stopping, Stopped)
                | (Stopping, Killed)
                | (Created, Killed)
                | (Launching, Killed)
                | (Running, Killed)
                | (Failed, Killed)
                | (Exited, Killed)
                | (Stopped, Killed)
        )
    }

    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Stopped | Self::Failed | Self::Exited | Self::Killed
        )
    }
}

impl fmt::Display for AgentStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Created => "created",
            Self::Launching => "launching",
            Self::Running => "running",
            Self::Stopping => "stopping",
            Self::Stopped => "stopped",
            Self::Failed => "failed",
            Self::Exited => "exited",
            Self::Killed => "killed",
        })
    }
}

impl FromStr for AgentStatus {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "created" => Ok(Self::Created),
            "launching" => Ok(Self::Launching),
            "running" => Ok(Self::Running),
            "stopping" => Ok(Self::Stopping),
            "stopped" => Ok(Self::Stopped),
            "failed" => Ok(Self::Failed),
            "exited" => Ok(Self::Exited),
            "killed" => Ok(Self::Killed),
            _ => Err(format!("unknown agent status: {value}")),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RunProfileKind {
    Safe,
    Auto,
    Resume,
    Sandbox,
    Custom,
}

impl fmt::Display for RunProfileKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Safe => "safe",
            Self::Auto => "auto",
            Self::Resume => "resume",
            Self::Sandbox => "sandbox",
            Self::Custom => "custom",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::AgentStatus;

    #[test]
    fn status_machine_allows_expected_transitions() {
        assert!(AgentStatus::Created.can_transition_to(AgentStatus::Launching));
        assert!(AgentStatus::Launching.can_transition_to(AgentStatus::Running));
        assert!(AgentStatus::Running.can_transition_to(AgentStatus::Stopping));
        assert!(AgentStatus::Stopping.can_transition_to(AgentStatus::Stopped));
        assert!(AgentStatus::Running.can_transition_to(AgentStatus::Killed));
    }

    #[test]
    fn status_machine_rejects_invalid_transitions() {
        assert!(!AgentStatus::Created.can_transition_to(AgentStatus::Running));
        assert!(!AgentStatus::Stopped.can_transition_to(AgentStatus::Running));
        assert!(!AgentStatus::Failed.can_transition_to(AgentStatus::Running));
    }
}
