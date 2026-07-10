use crate::{AppError, AppType};
use serde::{Deserialize, Serialize};
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProviderTruthSource {
    Db,
    LiveFile,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProviderLiveSyncPolicy {
    CurrentOnly,
    MultiProviderLive,
    FileTruth,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderRuntimeCapabilities {
    pub has_current_provider: bool,
    pub truth_source: ProviderTruthSource,
    pub live_sync_policy: ProviderLiveSyncPolicy,
    pub supports_import_from_live: bool,
    pub supports_remove_from_live_only: bool,
    pub supports_preview_apply: bool,
    pub supports_rename_after_live_add: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProviderRuntimeApp {
    Claude,
    ClaudeDesktop,
    Codex,
    Gemini,
    OpenCode,
    OpenClaw,
    Hermes,
    Pi,
}

impl ProviderRuntimeApp {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::ClaudeDesktop => "claude-desktop",
            Self::Codex => "codex",
            Self::Gemini => "gemini",
            Self::OpenCode => "opencode",
            Self::OpenClaw => "openclaw",
            Self::Hermes => "hermes",
            Self::Pi => "pi",
        }
    }

    pub fn capabilities(&self) -> ProviderRuntimeCapabilities {
        match self {
            Self::Claude | Self::ClaudeDesktop | Self::Codex | Self::Gemini => {
                ProviderRuntimeCapabilities {
                    has_current_provider: true,
                    truth_source: ProviderTruthSource::Db,
                    live_sync_policy: ProviderLiveSyncPolicy::CurrentOnly,
                    supports_import_from_live: false,
                    supports_remove_from_live_only: false,
                    supports_preview_apply: false,
                    supports_rename_after_live_add: false,
                }
            }
            Self::OpenCode | Self::OpenClaw | Self::Hermes => ProviderRuntimeCapabilities {
                has_current_provider: false,
                truth_source: ProviderTruthSource::Db,
                live_sync_policy: ProviderLiveSyncPolicy::MultiProviderLive,
                supports_import_from_live: true,
                supports_remove_from_live_only: true,
                supports_preview_apply: false,
                supports_rename_after_live_add: false,
            },
            Self::Pi => ProviderRuntimeCapabilities {
                has_current_provider: false,
                truth_source: ProviderTruthSource::LiveFile,
                live_sync_policy: ProviderLiveSyncPolicy::FileTruth,
                supports_import_from_live: false,
                supports_remove_from_live_only: false,
                supports_preview_apply: true,
                supports_rename_after_live_add: false,
            },
        }
    }
}

impl From<AppType> for ProviderRuntimeApp {
    fn from(value: AppType) -> Self {
        match value {
            AppType::Claude => Self::Claude,
            AppType::ClaudeDesktop => Self::ClaudeDesktop,
            AppType::Codex => Self::Codex,
            AppType::Gemini => Self::Gemini,
            AppType::OpenCode => Self::OpenCode,
            AppType::OpenClaw => Self::OpenClaw,
            AppType::Hermes => Self::Hermes,
        }
    }
}

impl TryFrom<ProviderRuntimeApp> for AppType {
    type Error = AppError;

    fn try_from(value: ProviderRuntimeApp) -> Result<Self, Self::Error> {
        match value {
            ProviderRuntimeApp::Claude => Ok(Self::Claude),
            ProviderRuntimeApp::ClaudeDesktop => Ok(Self::ClaudeDesktop),
            ProviderRuntimeApp::Codex => Ok(Self::Codex),
            ProviderRuntimeApp::Gemini => Ok(Self::Gemini),
            ProviderRuntimeApp::OpenCode => Ok(Self::OpenCode),
            ProviderRuntimeApp::OpenClaw => Ok(Self::OpenClaw),
            ProviderRuntimeApp::Hermes => Ok(Self::Hermes),
            ProviderRuntimeApp::Pi => Err(AppError::localized(
                "provider_runtime.pi_has_no_db_app_type",
                "Pi provider runtime 不使用数据库应用类型。",
                "Pi provider runtime does not use a database app type.",
            )),
        }
    }
}

impl FromStr for ProviderRuntimeApp {
    type Err = AppError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_lowercase().as_str() {
            "claude" => Ok(Self::Claude),
            "claude-desktop" | "claude_desktop" | "claudedesktop" => Ok(Self::ClaudeDesktop),
            "codex" => Ok(Self::Codex),
            "gemini" => Ok(Self::Gemini),
            "opencode" => Ok(Self::OpenCode),
            "openclaw" => Ok(Self::OpenClaw),
            "hermes" => Ok(Self::Hermes),
            "pi" => Ok(Self::Pi),
            other => Err(AppError::localized(
                "unsupported_provider_runtime_app",
                format!("不支持的 provider runtime 应用标识: '{other}'。"),
                format!("Unsupported provider runtime app id: '{other}'."),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ProviderLiveSyncPolicy, ProviderRuntimeApp, ProviderTruthSource};

    #[test]
    fn pi_is_live_file_backed_without_current_provider() {
        let caps = ProviderRuntimeApp::Pi.capabilities();

        assert!(!caps.has_current_provider);
        assert_eq!(caps.truth_source, ProviderTruthSource::LiveFile);
        assert_eq!(caps.live_sync_policy, ProviderLiveSyncPolicy::FileTruth);
        assert!(caps.supports_preview_apply);
    }

    #[test]
    fn opencode_family_is_db_backed_multi_provider_live() {
        for app in [
            ProviderRuntimeApp::OpenCode,
            ProviderRuntimeApp::OpenClaw,
            ProviderRuntimeApp::Hermes,
        ] {
            let caps = app.capabilities();
            assert!(
                !caps.has_current_provider,
                "{app:?} should not expose current provider"
            );
            assert_eq!(caps.truth_source, ProviderTruthSource::Db);
            assert_eq!(
                caps.live_sync_policy,
                ProviderLiveSyncPolicy::MultiProviderLive
            );
            assert!(caps.supports_remove_from_live_only);
        }
    }

    #[test]
    fn switch_mode_apps_keep_current_provider_semantics() {
        for app in [
            ProviderRuntimeApp::Claude,
            ProviderRuntimeApp::ClaudeDesktop,
            ProviderRuntimeApp::Codex,
            ProviderRuntimeApp::Gemini,
        ] {
            let caps = app.capabilities();
            assert!(
                caps.has_current_provider,
                "{app:?} should expose current provider"
            );
            assert_eq!(caps.truth_source, ProviderTruthSource::Db);
            assert_eq!(caps.live_sync_policy, ProviderLiveSyncPolicy::CurrentOnly);
        }
    }
}
