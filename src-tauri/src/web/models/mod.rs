use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::services::skill::SkillStorageLocation;

pub mod app_state;

pub type ProviderCategory = String;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Provider {
    pub id: String,
    pub name: String,
    #[serde(rename = "settingsConfig")]
    pub settings_config: serde_json::Value,
    #[serde(rename = "websiteUrl")]
    pub website_url: Option<String>,
    pub category: Option<ProviderCategory>,
    #[serde(rename = "createdAt")]
    pub created_at: Option<i64>,
    #[serde(rename = "sortIndex")]
    pub sort_index: Option<i32>,
    pub notes: Option<String>,
    #[serde(rename = "isPartner")]
    pub is_partner: Option<bool>,
    pub meta: Option<ProviderMeta>,
    pub icon: Option<String>,
    #[serde(rename = "iconColor")]
    pub icon_color: Option<String>,
    #[serde(rename = "inFailoverQueue")]
    pub in_failover_queue: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderMeta {
    #[serde(rename = "custom_endpoints")]
    pub custom_endpoints: Option<HashMap<String, CustomEndpoint>>,
    #[serde(rename = "usage_script")]
    pub usage_script: Option<UsageScript>,
    #[serde(rename = "endpointAutoSelect")]
    pub endpoint_auto_select: Option<bool>,
    #[serde(rename = "isPartner")]
    pub is_partner: Option<bool>,
    #[serde(rename = "partnerPromotionKey")]
    pub partner_promotion_key: Option<String>,
    #[serde(rename = "testConfig")]
    pub test_config: Option<ProviderTestConfig>,
    #[serde(rename = "proxyConfig")]
    pub proxy_config: Option<ProviderProxyConfig>,
    #[serde(rename = "costMultiplier")]
    pub cost_multiplier: Option<String>,
    #[serde(rename = "pricingModelSource")]
    pub pricing_model_source: Option<String>,
    #[serde(rename = "apiFormat")]
    pub api_format: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomEndpoint {
    pub url: String,
    #[serde(rename = "addedAt")]
    pub added_at: i64,
    #[serde(rename = "lastUsed")]
    pub last_used: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageScript {
    pub enabled: bool,
    pub language: String,
    pub code: String,
    pub timeout: Option<u64>,
    #[serde(rename = "templateType")]
    pub template_type: Option<String>,
    #[serde(rename = "apiKey")]
    pub api_key: Option<String>,
    #[serde(rename = "baseUrl")]
    pub base_url: Option<String>,
    #[serde(rename = "accessToken")]
    pub access_token: Option<String>,
    #[serde(rename = "userId")]
    pub user_id: Option<String>,
    #[serde(rename = "autoQueryInterval")]
    pub auto_query_interval: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderTestConfig {
    pub enabled: bool,
    #[serde(rename = "testModel")]
    pub test_model: Option<String>,
    #[serde(rename = "timeoutSecs")]
    pub timeout_secs: Option<u64>,
    #[serde(rename = "testPrompt")]
    pub test_prompt: Option<String>,
    #[serde(rename = "degradedThresholdMs")]
    pub degraded_threshold_ms: Option<u64>,
    #[serde(rename = "maxRetries")]
    pub max_retries: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderProxyConfig {
    pub enabled: bool,
    #[serde(rename = "proxyType")]
    pub proxy_type: Option<String>,
    #[serde(rename = "proxyHost")]
    pub proxy_host: Option<String>,
    #[serde(rename = "proxyPort")]
    pub proxy_port: Option<u16>,
    #[serde(rename = "proxyUsername")]
    pub proxy_username: Option<String>,
    #[serde(rename = "proxyPassword")]
    pub proxy_password: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServer {
    pub id: String,
    pub name: String,
    pub server: McpServerSpec,
    pub apps: McpApps,
    pub description: Option<String>,
    pub tags: Option<Vec<String>>,
    pub homepage: Option<String>,
    pub docs: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerSpec {
    #[serde(rename = "type")]
    pub server_type: Option<String>,
    pub command: Option<String>,
    pub args: Option<Vec<String>>,
    pub env: Option<HashMap<String, String>>,
    pub cwd: Option<String>,
    pub url: Option<String>,
    pub headers: Option<HashMap<String, String>>,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpApps {
    pub claude: bool,
    pub codex: bool,
    pub gemini: bool,
    pub opencode: bool,
    pub openclaw: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    #[serde(rename = "showInTray")]
    pub show_in_tray: bool,
    #[serde(rename = "minimizeToTrayOnClose")]
    pub minimize_to_tray_on_close: bool,
    #[serde(rename = "enableClaudePluginIntegration")]
    pub enable_claude_plugin_integration: Option<bool>,
    #[serde(rename = "skipClaudeOnboarding")]
    pub skip_claude_onboarding: Option<bool>,
    #[serde(rename = "launchOnStartup")]
    pub launch_on_startup: Option<bool>,
    #[serde(rename = "silentStartup")]
    pub silent_startup: Option<bool>,
    #[serde(rename = "enableLocalProxy")]
    pub enable_local_proxy: Option<bool>,
    pub language: Option<String>,
    #[serde(rename = "visibleApps")]
    pub visible_apps: Option<VisibleApps>,
    #[serde(rename = "claudeConfigDir")]
    pub claude_config_dir: Option<String>,
    #[serde(rename = "codexConfigDir")]
    pub codex_config_dir: Option<String>,
    #[serde(rename = "geminiConfigDir")]
    pub gemini_config_dir: Option<String>,
    #[serde(rename = "opencodeConfigDir")]
    pub opencode_config_dir: Option<String>,
    #[serde(rename = "openclawConfigDir")]
    pub openclaw_config_dir: Option<String>,
    #[serde(rename = "currentProviderClaude")]
    pub current_provider_claude: Option<String>,
    #[serde(rename = "currentProviderCodex")]
    pub current_provider_codex: Option<String>,
    #[serde(rename = "currentProviderGemini")]
    pub current_provider_gemini: Option<String>,
    #[serde(rename = "skillSyncMethod")]
    pub skill_sync_method: Option<String>,
    #[serde(rename = "skillStorageLocation")]
    pub skill_storage_location: Option<SkillStorageLocation>,
    #[serde(rename = "webdavSync")]
    pub webdav_sync: Option<WebDavSyncSettings>,
    #[serde(rename = "preferredTerminal")]
    pub preferred_terminal: Option<String>,
    #[serde(rename = "firstRunNoticeConfirmed")]
    pub first_run_notice_confirmed: Option<bool>,
    #[serde(rename = "proxyConfirmed")]
    pub proxy_confirmed: Option<bool>,
    #[serde(rename = "usageConfirmed")]
    pub usage_confirmed: Option<bool>,
    #[serde(rename = "streamCheckConfirmed")]
    pub stream_check_confirmed: Option<bool>,
    #[serde(rename = "enableFailoverToggle")]
    pub enable_failover_toggle: Option<bool>,
    #[serde(rename = "preserveCodexOfficialAuthOnSwitch")]
    pub preserve_codex_official_auth_on_switch: Option<bool>,
    #[serde(rename = "failoverConfirmed")]
    pub failover_confirmed: Option<bool>,
    #[serde(rename = "autoSyncConfirmed")]
    pub auto_sync_confirmed: Option<bool>,
    #[serde(rename = "commonConfigConfirmed")]
    pub common_config_confirmed: Option<bool>,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VisibleApps {
    #[serde(default = "default_true")]
    pub claude: bool,
    // Frontend uses the kebab-case key "claude-desktop"; keep it round-trippable.
    #[serde(rename = "claude-desktop", default = "default_true")]
    pub claude_desktop: bool,
    #[serde(default = "default_true")]
    pub codex: bool,
    #[serde(default = "default_true")]
    pub gemini: bool,
    #[serde(default = "default_true")]
    pub opencode: bool,
    #[serde(default = "default_true")]
    pub openclaw: bool,
    #[serde(default = "default_true")]
    pub hermes: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebDavSyncSettings {
    pub enabled: Option<bool>,
    #[serde(rename = "autoSync")]
    pub auto_sync: Option<bool>,
    #[serde(rename = "baseUrl")]
    pub base_url: Option<String>,
    pub username: Option<String>,
    pub password: Option<String>,
    #[serde(rename = "remoteRoot")]
    pub remote_root: Option<String>,
    pub profile: Option<String>,
    pub status: Option<WebDavSyncStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebDavSyncStatus {
    #[serde(rename = "lastSyncAt")]
    pub last_sync_at: Option<i64>,
    #[serde(rename = "lastError")]
    pub last_error: Option<String>,
    #[serde(rename = "lastErrorSource")]
    pub last_error_source: Option<String>,
    #[serde(rename = "lastRemoteEtag")]
    pub last_remote_etag: Option<String>,
    #[serde(rename = "lastLocalManifestHash")]
    pub last_local_manifest_hash: Option<String>,
    #[serde(rename = "lastRemoteManifestHash")]
    pub last_remote_manifest_hash: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Prompt {
    pub id: String,
    pub name: String,
    pub content: String,
    pub description: Option<String>,
    #[serde(rename = "enabled")]
    pub enabled: bool,
    #[serde(rename = "createdAt")]
    pub created_at: Option<i64>,
    #[serde(rename = "updatedAt")]
    pub updated_at: Option<i64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_serializes_to_frontend_shape() {
        let prompt = Prompt {
            id: "p1".to_string(),
            name: "Default".to_string(),
            content: "# System".to_string(),
            description: Some("desc".to_string()),
            enabled: true,
            created_at: Some(1),
            updated_at: Some(2),
        };
        let json = serde_json::to_value(&prompt).unwrap();
        assert_eq!(json["id"], "p1");
        assert_eq!(json["name"], "Default");
        assert_eq!(json["content"], "# System");
        assert_eq!(json["description"], "desc");
        assert_eq!(json["enabled"], true);
        assert_eq!(json["createdAt"], 1);
        assert_eq!(json["updatedAt"], 2);
        assert!(json.get("isActive").is_none());
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Skill {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    #[serde(rename = "installedAt")]
    pub installed_at: Option<i64>,
    #[serde(rename = "updatedAt")]
    pub updated_at: Option<i64>,
    pub source: Option<String>,
    pub version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    #[serde(rename = "sessionId")]
    pub session_id: String,
    #[serde(rename = "providerId")]
    pub provider_id: String,
    pub title: Option<String>,
    pub summary: Option<String>,
    #[serde(rename = "projectDir")]
    pub project_dir: Option<String>,
    #[serde(rename = "createdAt")]
    pub created_at: Option<i64>,
    #[serde(rename = "lastActiveAt")]
    pub last_active_at: Option<i64>,
    #[serde(rename = "sourcePath")]
    pub source_path: Option<String>,
    #[serde(rename = "resumeCommand")]
    pub resume_command: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiResponse<T> {
    pub success: bool,
    pub data: Option<T>,
    pub error: Option<String>,
}

impl<T> ApiResponse<T> {
    pub fn success(data: T) -> Self {
        Self {
            success: true,
            data: Some(data),
            error: None,
        }
    }

    pub fn error(message: String) -> Self {
        Self {
            success: false,
            data: None,
            error: Some(message),
        }
    }
}
