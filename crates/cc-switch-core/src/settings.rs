//! Application settings

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::{OnceLock, RwLock};

use crate::app_config::AppType;
use crate::error::AppError;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum SyncMethod {
    #[default]
    Auto,
    Symlink,
    Copy,
}

impl FromStr for SyncMethod {
    type Err = AppError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "auto" => Ok(SyncMethod::Auto),
            "symlink" => Ok(SyncMethod::Symlink),
            "copy" => Ok(SyncMethod::Copy),
            _ => Ok(SyncMethod::Auto),
        }
    }
}

impl std::fmt::Display for SyncMethod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SyncMethod::Auto => write!(f, "auto"),
            SyncMethod::Symlink => write!(f, "symlink"),
            SyncMethod::Copy => write!(f, "copy"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomEndpoint {
    pub url: String,
    pub added_at: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_used: Option<i64>,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct VisibleApps {
    #[serde(default = "default_true")]
    pub claude: bool,
    #[serde(default = "default_true")]
    pub codex: bool,
    #[serde(default = "default_true")]
    pub gemini: bool,
    #[serde(default = "default_true")]
    pub opencode: bool,
    #[serde(default = "default_true")]
    pub openclaw: bool,
}

impl Default for VisibleApps {
    fn default() -> Self {
        Self {
            claude: true,
            codex: true,
            gemini: true,
            opencode: true,
            openclaw: true,
        }
    }
}

impl VisibleApps {
    pub fn is_visible(&self, app: &AppType) -> bool {
        match app {
            AppType::Claude => self.claude,
            AppType::Codex => self.codex,
            AppType::Gemini => self.gemini,
            AppType::OpenCode => self.opencode,
            AppType::OpenClaw => self.openclaw,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct WebDavSyncStatus {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_sync_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error_source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_remote_etag: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_local_manifest_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_remote_manifest_hash: Option<String>,
}

fn default_remote_root() -> String {
    "cc-switch-sync".to_string()
}

fn default_profile() -> String {
    "default".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebDavSyncSettings {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub auto_sync: bool,
    #[serde(default)]
    pub base_url: String,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub password: String,
    #[serde(default = "default_remote_root")]
    pub remote_root: String,
    #[serde(default = "default_profile")]
    pub profile: String,
    #[serde(default)]
    pub status: WebDavSyncStatus,
}

impl Default for WebDavSyncSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            auto_sync: false,
            base_url: String::new(),
            username: String::new(),
            password: String::new(),
            remote_root: default_remote_root(),
            profile: default_profile(),
            status: WebDavSyncStatus::default(),
        }
    }
}

impl WebDavSyncSettings {
    pub fn validate(&self) -> Result<(), AppError> {
        if self.base_url.trim().is_empty() {
            return Err(AppError::localized(
                "webdav.base_url.required",
                "WebDAV 地址不能为空",
                "WebDAV URL is required.",
            ));
        }
        if self.username.trim().is_empty() {
            return Err(AppError::localized(
                "webdav.username.required",
                "WebDAV 用户名不能为空",
                "WebDAV username is required.",
            ));
        }
        Ok(())
    }

    pub fn normalize(&mut self) {
        self.base_url = self.base_url.trim().to_string();
        self.username = self.username.trim().to_string();
        self.remote_root = self.remote_root.trim().to_string();
        self.profile = self.profile.trim().to_string();
        if self.remote_root.is_empty() {
            self.remote_root = default_remote_root();
        }
        if self.profile.is_empty() {
            self.profile = default_profile();
        }
    }

    fn is_empty(&self) -> bool {
        self.base_url.is_empty() && self.username.is_empty() && self.password.is_empty()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    #[serde(default = "default_true")]
    pub show_in_tray: bool,
    #[serde(default = "default_true")]
    pub minimize_to_tray_on_close: bool,
    #[serde(default)]
    pub enable_claude_plugin_integration: bool,
    #[serde(default)]
    pub skip_claude_onboarding: bool,
    #[serde(default)]
    pub launch_on_startup: bool,
    #[serde(default)]
    pub silent_startup: bool,
    #[serde(default)]
    pub enable_local_proxy: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proxy_confirmed: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage_confirmed: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream_check_confirmed: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visible_apps: Option<VisibleApps>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claude_config_dir: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codex_config_dir: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gemini_config_dir: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opencode_config_dir: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub openclaw_config_dir: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_provider_claude: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_provider_codex: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_provider_gemini: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_provider_opencode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_provider_openclaw: Option<String>,
    #[serde(default)]
    pub skill_sync_method: SyncMethod,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub webdav_sync: Option<WebDavSyncSettings>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub webdav_backup: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backup_interval_hours: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backup_retain_count: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preferred_terminal: Option<String>,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            show_in_tray: true,
            minimize_to_tray_on_close: true,
            enable_claude_plugin_integration: false,
            skip_claude_onboarding: false,
            launch_on_startup: false,
            silent_startup: false,
            enable_local_proxy: false,
            proxy_confirmed: None,
            usage_confirmed: None,
            stream_check_confirmed: None,
            language: None,
            visible_apps: None,
            claude_config_dir: None,
            codex_config_dir: None,
            gemini_config_dir: None,
            opencode_config_dir: None,
            openclaw_config_dir: None,
            current_provider_claude: None,
            current_provider_codex: None,
            current_provider_gemini: None,
            current_provider_opencode: None,
            current_provider_openclaw: None,
            skill_sync_method: SyncMethod::default(),
            webdav_sync: None,
            webdav_backup: None,
            backup_interval_hours: None,
            backup_retain_count: None,
            preferred_terminal: None,
        }
    }
}

impl AppSettings {
    fn normalize(&mut self) {
        self.language = self
            .language
            .as_ref()
            .map(|s| s.trim())
            .filter(|s| matches!(*s, "en" | "zh" | "ja"))
            .map(|s| s.to_string());

        self.claude_config_dir = normalize_optional_string(self.claude_config_dir.take());
        self.codex_config_dir = normalize_optional_string(self.codex_config_dir.take());
        self.gemini_config_dir = normalize_optional_string(self.gemini_config_dir.take());
        self.opencode_config_dir = normalize_optional_string(self.opencode_config_dir.take());
        self.openclaw_config_dir = normalize_optional_string(self.openclaw_config_dir.take());

        self.current_provider_claude =
            normalize_optional_string(self.current_provider_claude.take());
        self.current_provider_codex = normalize_optional_string(self.current_provider_codex.take());
        self.current_provider_gemini =
            normalize_optional_string(self.current_provider_gemini.take());
        self.current_provider_opencode =
            normalize_optional_string(self.current_provider_opencode.take());
        self.current_provider_openclaw =
            normalize_optional_string(self.current_provider_openclaw.take());
        if let Some(sync) = &mut self.webdav_sync {
            sync.normalize();
            if sync.is_empty() {
                self.webdav_sync = None;
            }
        }
        self.preferred_terminal = normalize_optional_string(self.preferred_terminal.take());
    }

    fn load_from_file() -> Self {
        let path = settings_path();
        let Ok(content) = std::fs::read_to_string(&path) else {
            return Self::default();
        };

        match serde_json::from_str::<AppSettings>(&content) {
            Ok(mut settings) => {
                settings.normalize();
                settings
            }
            Err(err) => {
                log::warn!(
                    "Failed to parse settings file {}, using defaults: {}",
                    path.display(),
                    err
                );
                Self::default()
            }
        }
    }
}

fn normalize_optional_string(value: Option<String>) -> Option<String> {
    value
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

static SETTINGS_STORE: OnceLock<RwLock<AppSettings>> = OnceLock::new();

fn settings_store() -> &'static RwLock<AppSettings> {
    SETTINGS_STORE.get_or_init(|| RwLock::new(AppSettings::load_from_file()))
}

pub fn settings_path() -> PathBuf {
    crate::config::settings_path()
}

pub fn resolve_override_path(raw: &str) -> PathBuf {
    if raw == "~" {
        return crate::config::get_home_dir();
    }
    if let Some(stripped) = raw.strip_prefix("~/") {
        return crate::config::get_home_dir().join(stripped);
    }
    if let Some(stripped) = raw.strip_prefix("~\\") {
        return crate::config::get_home_dir().join(stripped);
    }
    PathBuf::from(raw)
}

fn save_settings_file(settings: &AppSettings) -> Result<(), AppError> {
    let mut normalized = settings.clone();
    normalized.normalize();
    let json = serde_json::to_string_pretty(&normalized)
        .map_err(|e| AppError::JsonSerialize { source: e })?;
    crate::config::atomic_write(&settings_path(), json.as_bytes())
}

pub fn get_settings() -> AppSettings {
    settings_store()
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}

pub fn get_settings_for_frontend() -> AppSettings {
    let mut settings = get_settings();
    if let Some(sync) = &mut settings.webdav_sync {
        sync.password.clear();
    }
    settings.webdav_backup = None;
    settings
}

pub fn update_settings(mut new_settings: AppSettings) -> Result<(), AppError> {
    new_settings.normalize();
    save_settings_file(&new_settings)?;
    let mut guard = settings_store().write().unwrap_or_else(|e| e.into_inner());
    *guard = new_settings;
    Ok(())
}

fn mutate_settings<F>(mutator: F) -> Result<(), AppError>
where
    F: FnOnce(&mut AppSettings),
{
    let mut guard = settings_store().write().unwrap_or_else(|e| e.into_inner());
    let mut next = guard.clone();
    mutator(&mut next);
    next.normalize();
    save_settings_file(&next)?;
    *guard = next;
    Ok(())
}

pub fn reload_settings() -> Result<(), AppError> {
    let fresh = AppSettings::load_from_file();
    let mut guard = settings_store().write().unwrap_or_else(|e| e.into_inner());
    *guard = fresh;
    Ok(())
}

pub fn get_claude_override_dir() -> Option<PathBuf> {
    get_settings()
        .claude_config_dir
        .as_deref()
        .map(resolve_override_path)
}

pub fn get_codex_override_dir() -> Option<PathBuf> {
    get_settings()
        .codex_config_dir
        .as_deref()
        .map(resolve_override_path)
}

pub fn get_gemini_override_dir() -> Option<PathBuf> {
    get_settings()
        .gemini_config_dir
        .as_deref()
        .map(resolve_override_path)
}

pub fn get_opencode_override_dir() -> Option<PathBuf> {
    get_settings()
        .opencode_config_dir
        .as_deref()
        .map(resolve_override_path)
}

pub fn get_openclaw_override_dir() -> Option<PathBuf> {
    get_settings()
        .openclaw_config_dir
        .as_deref()
        .map(resolve_override_path)
}

pub fn get_current_provider(app_type: &AppType) -> Option<String> {
    let settings = get_settings();
    match app_type {
        AppType::Claude => settings.current_provider_claude,
        AppType::Codex => settings.current_provider_codex,
        AppType::Gemini => settings.current_provider_gemini,
        AppType::OpenCode => settings.current_provider_opencode,
        AppType::OpenClaw => settings.current_provider_openclaw,
    }
}

pub fn set_current_provider(app_type: &AppType, id: Option<&str>) -> Result<(), AppError> {
    let mut settings = get_settings();
    let next = id.map(|value| value.to_string());
    match app_type {
        AppType::Claude => settings.current_provider_claude = next,
        AppType::Codex => settings.current_provider_codex = next,
        AppType::Gemini => settings.current_provider_gemini = next,
        AppType::OpenCode => settings.current_provider_opencode = next,
        AppType::OpenClaw => settings.current_provider_openclaw = next,
    }
    update_settings(settings)
}

pub fn get_effective_current_provider(
    db: &crate::database::Database,
    app_type: &AppType,
) -> Result<Option<String>, AppError> {
    if app_type.is_additive_mode() {
        return Ok(None);
    }

    if let Some(local_id) = get_current_provider(app_type) {
        let providers = db.get_all_providers(app_type.as_str())?;
        if providers.contains_key(&local_id) {
            return Ok(Some(local_id));
        }
        log::warn!(
            "Current provider '{}' for {} no longer exists, clearing local override",
            local_id,
            app_type.as_str()
        );
        let _ = set_current_provider(app_type, None);
    }

    db.get_current_provider(app_type.as_str())
}

pub fn get_skill_sync_method() -> SyncMethod {
    get_settings().skill_sync_method
}

pub fn get_preferred_terminal() -> Option<String> {
    get_settings().preferred_terminal
}

pub fn effective_backup_interval_hours() -> u32 {
    get_settings().backup_interval_hours.unwrap_or(24)
}

pub fn effective_backup_retain_count() -> usize {
    get_settings()
        .backup_retain_count
        .map(|value| (value as usize).max(1))
        .unwrap_or(10)
}

pub fn get_webdav_sync_settings() -> Option<WebDavSyncSettings> {
    get_settings().webdav_sync
}

pub fn set_webdav_sync_settings(settings: Option<WebDavSyncSettings>) -> Result<(), AppError> {
    mutate_settings(|current| {
        current.webdav_sync = settings;
    })
}

pub fn update_webdav_sync_status(status: WebDavSyncStatus) -> Result<(), AppError> {
    mutate_settings(|current| {
        if let Some(sync) = current.webdav_sync.as_mut() {
            sync.status = status;
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;
    use crate::provider::Provider;
    use serial_test::serial;
    use tempfile::tempdir;

    #[test]
    #[serial]
    fn update_and_reload_settings_persist_openclaw_fields() -> Result<(), AppError> {
        let temp = tempdir().expect("tempdir");
        std::env::set_var("CC_SWITCH_TEST_HOME", temp.path());

        update_settings(AppSettings {
            openclaw_config_dir: Some("~/custom-openclaw".to_string()),
            current_provider_openclaw: Some("provider-openclaw".to_string()),
            ..AppSettings::default()
        })?;

        reload_settings()?;
        let settings = get_settings();

        assert_eq!(
            settings.openclaw_config_dir.as_deref(),
            Some("~/custom-openclaw")
        );
        assert_eq!(
            settings.current_provider_openclaw.as_deref(),
            Some("provider-openclaw")
        );
        assert_eq!(
            get_openclaw_override_dir(),
            Some(temp.path().join("custom-openclaw"))
        );

        Ok(())
    }

    #[test]
    #[serial]
    fn effective_current_provider_falls_back_to_database_when_local_id_is_stale(
    ) -> Result<(), AppError> {
        let temp = tempdir().expect("tempdir");
        std::env::set_var("CC_SWITCH_TEST_HOME", temp.path());

        let db = Database::memory()?;
        let provider = Provider::with_id(
            "provider-a".to_string(),
            "Provider A".to_string(),
            serde_json::json!({
                "env": {
                    "ANTHROPIC_AUTH_TOKEN": "token"
                }
            }),
            None,
        );
        db.save_provider("claude", &provider)?;
        db.set_current_provider("claude", "provider-a")?;

        set_current_provider(&AppType::Claude, Some("missing"))?;
        assert_eq!(
            get_effective_current_provider(&db, &AppType::Claude)?,
            Some("provider-a".to_string())
        );
        assert_eq!(get_current_provider(&AppType::Claude), None);

        Ok(())
    }

    #[test]
    #[serial]
    fn frontend_settings_redacts_webdav_password() -> Result<(), AppError> {
        let temp = tempdir().expect("tempdir");
        std::env::set_var("CC_SWITCH_TEST_HOME", temp.path());

        update_settings(AppSettings {
            webdav_sync: Some(WebDavSyncSettings {
                base_url: "https://dav.example.com".to_string(),
                username: "alice".to_string(),
                password: "secret".to_string(),
                ..WebDavSyncSettings::default()
            }),
            ..AppSettings::default()
        })?;

        let frontend = get_settings_for_frontend();
        assert_eq!(
            frontend
                .webdav_sync
                .as_ref()
                .map(|item| item.password.as_str()),
            Some("")
        );

        let stored = get_settings();
        assert_eq!(
            stored
                .webdav_sync
                .as_ref()
                .map(|item| item.password.as_str()),
            Some("secret")
        );

        Ok(())
    }

    #[test]
    #[serial]
    fn webdav_status_update_preserves_credentials() -> Result<(), AppError> {
        let temp = tempdir().expect("tempdir");
        std::env::set_var("CC_SWITCH_TEST_HOME", temp.path());

        set_webdav_sync_settings(Some(WebDavSyncSettings {
            enabled: true,
            base_url: "https://dav.example.com".to_string(),
            username: "alice".to_string(),
            password: "secret".to_string(),
            ..WebDavSyncSettings::default()
        }))?;

        update_webdav_sync_status(WebDavSyncStatus {
            last_sync_at: Some(123),
            last_error: None,
            last_error_source: None,
            last_remote_etag: Some("etag".to_string()),
            last_local_manifest_hash: Some("local".to_string()),
            last_remote_manifest_hash: Some("remote".to_string()),
        })?;

        let saved = get_webdav_sync_settings().expect("webdav settings");
        assert_eq!(saved.password, "secret");
        assert_eq!(saved.status.last_sync_at, Some(123));
        assert_eq!(saved.status.last_remote_etag.as_deref(), Some("etag"));

        Ok(())
    }
}
