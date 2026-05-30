use serde::{Deserialize, Serialize};
use std::fs;
#[cfg(unix)]
use std::io::Write;
use std::path::PathBuf;
use std::sync::{OnceLock, RwLock};

use crate::app_config::AppType;
use crate::error::AppError;
use crate::services::skill::{SkillStorageLocation, SyncMethod};

/// 自定义端点配置（历史兼容，实际存储在 provider.meta.custom_endpoints）
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

/// 主页面显示的应用配置
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VisibleApps {
    #[serde(default = "default_true")]
    pub claude: bool,
    #[serde(
        rename = "claude-desktop",
        alias = "claudeDesktop",
        alias = "claude_desktop",
        default = "default_true"
    )]
    pub claude_desktop: bool,
    #[serde(default = "default_true")]
    pub codex: bool,
    #[serde(default = "default_true")]
    pub gemini: bool,
    #[serde(default = "default_true")]
    pub opencode: bool,
    #[serde(default = "default_true")]
    pub openclaw: bool,
    #[serde(default)]
    pub hermes: bool,
}

impl Default for VisibleApps {
    fn default() -> Self {
        Self {
            claude: true,
            claude_desktop: true,
            codex: true,
            gemini: true,
            opencode: true,
            openclaw: true,
            hermes: false, // 默认不显示，需用户手动启用
        }
    }
}

impl VisibleApps {
    /// Check if the specified app is visible
    pub fn is_visible(&self, app: &AppType) -> bool {
        match app {
            AppType::Claude => self.claude,
            AppType::ClaudeDesktop => self.claude_desktop,
            AppType::Codex => self.codex,
            AppType::Gemini => self.gemini,
            AppType::OpenCode => self.opencode,
            AppType::OpenClaw => self.openclaw,
            AppType::Hermes => self.hermes,
        }
    }
}

/// WebDAV 同步状态（持久化同步进度信息）
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

/// WebDAV 同步设置
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
    pub fn validate(&self) -> Result<(), crate::error::AppError> {
        if self.base_url.trim().is_empty() {
            return Err(crate::error::AppError::localized(
                "webdav.base_url.required",
                "WebDAV 地址不能为空",
                "WebDAV URL is required.",
            ));
        }
        if self.username.trim().is_empty() {
            return Err(crate::error::AppError::localized(
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

    /// Returns true if all credential fields are blank (no config to persist).
    fn is_empty(&self) -> bool {
        self.base_url.is_empty() && self.username.is_empty() && self.password.is_empty()
    }
}

/// 本机环境配置档。
///
/// Profile 保存每个环境自己的目录覆盖和当前供应商选择；切换时再把这些值
/// 拷贝回 AppSettings 的单值字段，让现有读写路径继续复用原逻辑。
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    pub id: String,
    pub label: String,

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
    pub hermes_config_dir: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_provider_claude: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_provider_claude_desktop: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_provider_codex: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_provider_gemini: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_provider_opencode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_provider_openclaw: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_provider_hermes: Option<String>,
}

fn normalize_optional_string(value: &mut Option<String>) {
    *value = value
        .as_ref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
}

impl Profile {
    fn from_settings(
        id: impl Into<String>,
        label: impl Into<String>,
        settings: &AppSettings,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            claude_config_dir: settings.claude_config_dir.clone(),
            codex_config_dir: settings.codex_config_dir.clone(),
            current_provider_claude: settings.current_provider_claude.clone(),
            current_provider_codex: settings.current_provider_codex.clone(),
            ..Default::default()
        }
    }

    fn normalize(&mut self) {
        self.id = self.id.trim().to_string();
        self.label = self.label.trim().to_string();
        normalize_optional_string(&mut self.claude_config_dir);
        normalize_optional_string(&mut self.codex_config_dir);
        normalize_optional_string(&mut self.gemini_config_dir);
        normalize_optional_string(&mut self.opencode_config_dir);
        normalize_optional_string(&mut self.openclaw_config_dir);
        normalize_optional_string(&mut self.hermes_config_dir);
        normalize_optional_string(&mut self.current_provider_claude);
        normalize_optional_string(&mut self.current_provider_claude_desktop);
        normalize_optional_string(&mut self.current_provider_codex);
        normalize_optional_string(&mut self.current_provider_gemini);
        normalize_optional_string(&mut self.current_provider_opencode);
        normalize_optional_string(&mut self.current_provider_openclaw);
        normalize_optional_string(&mut self.current_provider_hermes);
        if self.label.is_empty() {
            self.label = self.id.clone();
        }
    }
}

/// 本机自动迁移状态。
///
/// 这里记录的是本机启动时执行过的一次性迁移；标记不随数据库同步。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LocalMigrations {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codex_third_party_history_provider_bucket_v1:
        Option<CodexThirdPartyHistoryProviderBucketMigration>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codex_provider_template_v1: Option<CodexProviderTemplateMigration>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexThirdPartyHistoryProviderBucketMigration {
    pub completed_at: String,
    pub target_provider_id: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_provider_ids: Vec<String>,
    #[serde(default)]
    pub migrated_jsonl_files: usize,
    #[serde(default)]
    pub migrated_state_rows: usize,
    #[serde(default)]
    pub scanned_history_files: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexProviderTemplateMigration {
    pub completed_at: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub migrated_provider_ids: Vec<String>,
}

/// 应用设置结构
///
/// 存储设备级别设置，保存在本地 `~/.cc-switch/settings.json`，不随数据库同步。
/// 这确保了云同步场景下多设备可以独立运作。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    // ===== 设备级 UI 设置 =====
    #[serde(default = "default_show_in_tray")]
    pub show_in_tray: bool,
    #[serde(default = "default_minimize_to_tray_on_close")]
    pub minimize_to_tray_on_close: bool,
    #[serde(default)]
    pub use_app_window_controls: bool,
    /// 是否启用 Claude 插件联动
    #[serde(default)]
    pub enable_claude_plugin_integration: bool,
    /// 是否跳过 Claude Code 初次安装确认
    #[serde(default)]
    pub skip_claude_onboarding: bool,
    /// 是否开机自启
    #[serde(default)]
    pub launch_on_startup: bool,
    /// 静默启动（程序启动时不显示主窗口，仅托盘运行）
    #[serde(default)]
    pub silent_startup: bool,
    /// 是否在主页面启用本地代理功能（默认关闭）
    #[serde(default)]
    pub enable_local_proxy: bool,
    /// User has confirmed the local proxy first-run notice
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proxy_confirmed: Option<bool>,
    /// User has confirmed the usage query first-run notice
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage_confirmed: Option<bool>,
    /// User has confirmed the stream check first-run notice
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream_check_confirmed: Option<bool>,
    /// Whether to show the failover toggle independently on the main page
    #[serde(default)]
    pub enable_failover_toggle: bool,
    /// User has confirmed the failover toggle first-run notice
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failover_confirmed: Option<bool>,
    /// User has confirmed the first-run welcome notice
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_run_notice_confirmed: Option<bool>,
    /// User has confirmed the common config first-run notice
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub common_config_confirmed: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,

    // ===== 主页面显示的应用 =====
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visible_apps: Option<VisibleApps>,

    // ===== 设备级目录覆盖 =====
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
    pub hermes_config_dir: Option<String>,

    // ===== 当前供应商 ID（设备级）=====
    /// 当前 Claude 供应商 ID（本地存储，优先于数据库 is_current）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_provider_claude: Option<String>,
    /// 当前 Claude Desktop 供应商 ID（本地存储，优先于数据库 is_current）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_provider_claude_desktop: Option<String>,
    /// 当前 Codex 供应商 ID（本地存储，优先于数据库 is_current）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_provider_codex: Option<String>,
    /// 当前 Gemini 供应商 ID（本地存储，优先于数据库 is_current）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_provider_gemini: Option<String>,
    /// 当前 OpenCode 供应商 ID（本地存储，对 OpenCode 可能无意义，但保持结构一致）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_provider_opencode: Option<String>,
    /// 当前 OpenClaw 供应商 ID（本地存储，对 OpenClaw 可能无意义，但保持结构一致）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_provider_openclaw: Option<String>,
    /// 当前 Hermes 供应商 ID（本地存储，保持结构一致）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_provider_hermes: Option<String>,

    // ===== 环境配置档（设备级）=====
    /// 多环境配置档。切换时会同步到上面的单值字段以保持旧逻辑兼容。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub profiles: Vec<Profile>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_profile_id: Option<String>,

    // ===== Skill 同步设置 =====
    /// Skill 同步方式：auto（默认，优先 symlink）、symlink、copy
    #[serde(default)]
    pub skill_sync_method: SyncMethod,
    /// Skill 存储位置：cc_switch（默认）或 unified（~/.agents/skills/）
    #[serde(default)]
    pub skill_storage_location: SkillStorageLocation,

    // ===== WebDAV 同步设置 =====
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub webdav_sync: Option<WebDavSyncSettings>,

    // ===== WebDAV 备份设置（旧版，保留向后兼容）=====
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub webdav_backup: Option<serde_json::Value>,

    // ===== 备份策略设置 =====
    /// Auto-backup interval in hours (default 24, 0 = disabled)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backup_interval_hours: Option<u32>,
    /// Maximum number of backup files to retain (default 10)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backup_retain_count: Option<u32>,

    // ===== 终端设置 =====
    /// 首选终端应用（可选，默认使用系统默认终端）
    /// - macOS: "terminal" | "iterm2" | "warp" | "alacritty" | "kitty" | "ghostty" | "wezterm" | "kaku"
    /// - Windows: "cmd" | "powershell" | "wt" (Windows Terminal)
    /// - Linux: "gnome-terminal" | "konsole" | "xfce4-terminal" | "alacritty" | "kitty" | "ghostty"
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preferred_terminal: Option<String>,

    // ===== 本机自动迁移状态 =====
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_migrations: Option<LocalMigrations>,
}

fn default_show_in_tray() -> bool {
    true
}

fn default_minimize_to_tray_on_close() -> bool {
    true
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            show_in_tray: true,
            minimize_to_tray_on_close: true,
            use_app_window_controls: false,
            enable_claude_plugin_integration: false,
            skip_claude_onboarding: false,
            launch_on_startup: false,
            silent_startup: false,
            enable_local_proxy: false,
            proxy_confirmed: None,
            usage_confirmed: None,
            stream_check_confirmed: None,
            enable_failover_toggle: false,
            failover_confirmed: None,
            first_run_notice_confirmed: None,
            common_config_confirmed: None,
            language: None,
            visible_apps: None,
            claude_config_dir: None,
            codex_config_dir: None,
            gemini_config_dir: None,
            opencode_config_dir: None,
            openclaw_config_dir: None,
            hermes_config_dir: None,
            current_provider_claude: None,
            current_provider_claude_desktop: None,
            current_provider_codex: None,
            current_provider_gemini: None,
            current_provider_opencode: None,
            current_provider_openclaw: None,
            current_provider_hermes: None,
            profiles: Vec::new(),
            active_profile_id: None,
            skill_sync_method: SyncMethod::default(),
            skill_storage_location: SkillStorageLocation::default(),
            webdav_sync: None,
            webdav_backup: None,
            backup_interval_hours: None,
            backup_retain_count: None,
            preferred_terminal: None,
            local_migrations: None,
        }
    }
}

impl AppSettings {
    fn settings_path() -> Option<PathBuf> {
        // settings.json 保留用于旧版本迁移和无数据库场景
        Some(
            crate::config::get_home_dir()
                .join(".cc-switch")
                .join("settings.json"),
        )
    }

    fn normalize_paths(&mut self) {
        normalize_optional_string(&mut self.claude_config_dir);
        normalize_optional_string(&mut self.codex_config_dir);
        normalize_optional_string(&mut self.gemini_config_dir);
        normalize_optional_string(&mut self.opencode_config_dir);
        normalize_optional_string(&mut self.openclaw_config_dir);
        normalize_optional_string(&mut self.hermes_config_dir);
        normalize_optional_string(&mut self.current_provider_claude);
        normalize_optional_string(&mut self.current_provider_claude_desktop);
        normalize_optional_string(&mut self.current_provider_codex);
        normalize_optional_string(&mut self.current_provider_gemini);
        normalize_optional_string(&mut self.current_provider_opencode);
        normalize_optional_string(&mut self.current_provider_openclaw);
        normalize_optional_string(&mut self.current_provider_hermes);

        self.language = self
            .language
            .as_ref()
            .map(|s| s.trim())
            .filter(|s| matches!(*s, "en" | "zh" | "zh-TW" | "ja"))
            .map(|s| s.to_string());

        if let Some(sync) = &mut self.webdav_sync {
            sync.normalize();
            if sync.is_empty() {
                self.webdav_sync = None;
            }
        }

        self.normalize_profiles();
    }

    fn normalize_for_save(&mut self) {
        self.normalize_paths();
        self.save_single_fields_to_active_profile();
        self.normalize_profiles();
    }

    fn normalize_profiles(&mut self) {
        normalize_optional_string(&mut self.active_profile_id);

        let mut seen = std::collections::HashSet::new();
        let mut normalized_profiles = Vec::with_capacity(self.profiles.len());
        for mut profile in std::mem::take(&mut self.profiles) {
            profile.normalize();
            if profile.id.is_empty() || !seen.insert(profile.id.clone()) {
                continue;
            }
            normalized_profiles.push(profile);
        }
        self.profiles = normalized_profiles;

        if let Some(active_id) = self.active_profile_id.clone() {
            if !self.profiles.iter().any(|profile| profile.id == active_id) {
                self.active_profile_id = None;
            }
        }

        if self.active_profile_id.is_none() && !self.profiles.is_empty() {
            self.active_profile_id = self.profiles.first().map(|profile| profile.id.clone());
        }

        if self.profiles.is_empty() && self.has_profile_managed_values() {
            let profile = Profile::from_settings("default", "Default", self);
            self.profiles.push(profile);
            self.active_profile_id = Some("default".to_string());
        }
    }

    fn has_profile_managed_values(&self) -> bool {
        self.claude_config_dir.is_some()
            || self.codex_config_dir.is_some()
            || self.current_provider_claude.is_some()
            || self.current_provider_codex.is_some()
    }

    fn save_single_fields_to_active_profile(&mut self) {
        let Some(active_id) = self.active_profile_id.clone() else {
            return;
        };
        let snapshot = Profile::from_settings(active_id.clone(), active_id.clone(), self);
        if let Some(profile) = self
            .profiles
            .iter_mut()
            .find(|profile| profile.id == active_id)
        {
            profile.claude_config_dir = snapshot.claude_config_dir;
            profile.codex_config_dir = snapshot.codex_config_dir;
            profile.current_provider_claude = snapshot.current_provider_claude;
            profile.current_provider_codex = snapshot.current_provider_codex;
        }
    }

    fn apply_active_profile_to_single_fields(&mut self) {
        let Some(active_id) = self.active_profile_id.as_deref() else {
            return;
        };
        let Some(profile) = self
            .profiles
            .iter()
            .find(|profile| profile.id == active_id)
            .cloned()
        else {
            return;
        };

        self.claude_config_dir = profile.claude_config_dir;
        self.codex_config_dir = profile.codex_config_dir;
        self.current_provider_claude = profile.current_provider_claude;
        self.current_provider_codex = profile.current_provider_codex;
    }

    fn load_from_file() -> Self {
        let Some(path) = Self::settings_path() else {
            return Self::default();
        };
        if let Ok(content) = fs::read_to_string(&path) {
            match serde_json::from_str::<AppSettings>(&content) {
                Ok(mut settings) => {
                    settings.normalize_paths();
                    settings.apply_active_profile_to_single_fields();
                    settings
                }
                Err(err) => {
                    log::warn!(
                        "解析设置文件失败，将使用默认设置。路径: {}, 错误: {}",
                        path.display(),
                        err
                    );
                    Self::default()
                }
            }
        } else {
            Self::default()
        }
    }
}

fn save_settings_file(settings: &AppSettings) -> Result<(), AppError> {
    let mut normalized = settings.clone();
    normalized.normalize_for_save();
    let Some(path) = AppSettings::settings_path() else {
        return Err(AppError::Config("无法获取用户主目录".to_string()));
    };

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
    }

    let json = serde_json::to_string_pretty(&normalized)
        .map_err(|e| AppError::JsonSerialize { source: e })?;
    #[cfg(unix)]
    {
        use std::fs::OpenOptions;
        use std::os::unix::fs::OpenOptionsExt;

        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .mode(0o600)
            .open(&path)
            .map_err(|e| AppError::io(&path, e))?;
        file.write_all(json.as_bytes())
            .map_err(|e| AppError::io(&path, e))?;
    }

    #[cfg(not(unix))]
    {
        fs::write(&path, json).map_err(|e| AppError::io(&path, e))?;
    }

    Ok(())
}

static SETTINGS_STORE: OnceLock<RwLock<AppSettings>> = OnceLock::new();

fn settings_store() -> &'static RwLock<AppSettings> {
    SETTINGS_STORE.get_or_init(|| RwLock::new(AppSettings::load_from_file()))
}

fn resolve_override_path(raw: &str) -> PathBuf {
    if raw == "~" {
        if let Some(home) = dirs::home_dir() {
            return home;
        }
    } else if let Some(stripped) = raw.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(stripped);
        }
    } else if let Some(stripped) = raw.strip_prefix("~\\") {
        if let Some(home) = dirs::home_dir() {
            return home.join(stripped);
        }
    }

    PathBuf::from(raw)
}

pub fn get_settings() -> AppSettings {
    settings_store()
        .read()
        .unwrap_or_else(|e| {
            log::warn!("设置锁已毒化，使用恢复值: {e}");
            e.into_inner()
        })
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
    new_settings.normalize_for_save();
    save_settings_file(&new_settings)?;

    let mut guard = settings_store().write().unwrap_or_else(|e| {
        log::warn!("设置锁已毒化，使用恢复值: {e}");
        e.into_inner()
    });
    *guard = new_settings;
    Ok(())
}

pub fn update_settings_from_frontend(
    mut new_settings: AppSettings,
    profiles_are_authoritative: bool,
) -> Result<(), AppError> {
    if profiles_are_authoritative {
        new_settings.normalize_paths();
        new_settings.apply_active_profile_to_single_fields();
    }
    update_settings(new_settings)
}

fn mutate_settings<F>(mutator: F) -> Result<(), AppError>
where
    F: FnOnce(&mut AppSettings),
{
    let mut guard = settings_store().write().unwrap_or_else(|e| {
        log::warn!("设置锁已毒化，使用恢复值: {e}");
        e.into_inner()
    });
    let mut next = guard.clone();
    mutator(&mut next);
    next.normalize_for_save();
    save_settings_file(&next)?;
    *guard = next;
    Ok(())
}

/// 切换环境配置档，并把旧环境的当前单值字段保存回 profile。
pub fn switch_profile(profile_id: &str) -> Result<AppSettings, AppError> {
    let profile_id = profile_id.trim();
    if profile_id.is_empty() {
        return Err(AppError::Message("Profile ID 不能为空".to_string()));
    }

    let mut guard = settings_store().write().unwrap_or_else(|e| {
        log::warn!("设置锁已毒化，使用恢复值: {e}");
        e.into_inner()
    });

    let mut next = guard.clone();
    next.normalize_paths();
    next.save_single_fields_to_active_profile();
    let target_profile = next
        .profiles
        .iter()
        .find(|profile| profile.id == profile_id)
        .cloned()
        .ok_or_else(|| AppError::Message(format!("Profile {profile_id} 不存在")))?;

    next.active_profile_id = Some(profile_id.to_string());
    next.claude_config_dir = target_profile.claude_config_dir;
    next.codex_config_dir = target_profile.codex_config_dir;
    next.current_provider_claude = target_profile.current_provider_claude;
    next.current_provider_codex = target_profile.current_provider_codex;
    next.normalize_for_save();

    save_settings_file(&next)?;
    *guard = next.clone();
    Ok(next)
}

pub fn is_codex_third_party_history_provider_bucket_migrated() -> bool {
    get_settings()
        .local_migrations
        .as_ref()
        .and_then(|migrations| {
            migrations
                .codex_third_party_history_provider_bucket_v1
                .as_ref()
        })
        .is_some_and(|m| m.scanned_history_files)
}

pub fn mark_codex_third_party_history_provider_bucket_migrated(
    migration: CodexThirdPartyHistoryProviderBucketMigration,
) -> Result<(), AppError> {
    mutate_settings(|settings| {
        let migrations = settings
            .local_migrations
            .get_or_insert_with(Default::default);
        migrations.codex_third_party_history_provider_bucket_v1 = Some(migration);
    })
}

pub fn is_codex_provider_template_migrated() -> bool {
    get_settings()
        .local_migrations
        .as_ref()
        .and_then(|migrations| migrations.codex_provider_template_v1.as_ref())
        .is_some()
}

pub fn mark_codex_provider_template_migrated(
    migration: CodexProviderTemplateMigration,
) -> Result<(), AppError> {
    mutate_settings(|settings| {
        let migrations = settings
            .local_migrations
            .get_or_insert_with(Default::default);
        migrations.codex_provider_template_v1 = Some(migration);
    })
}

/// 从文件重新加载设置到内存缓存
/// 用于导入配置等场景，确保内存缓存与文件同步
pub fn reload_settings() -> Result<(), AppError> {
    let fresh_settings = AppSettings::load_from_file();
    let mut guard = settings_store().write().unwrap_or_else(|e| {
        log::warn!("设置锁已毒化，使用恢复值: {e}");
        e.into_inner()
    });
    *guard = fresh_settings;
    Ok(())
}

pub fn get_claude_override_dir() -> Option<PathBuf> {
    let settings = settings_store().read().ok()?;
    settings
        .claude_config_dir
        .as_ref()
        .map(|p| resolve_override_path(p))
}

pub fn get_codex_override_dir() -> Option<PathBuf> {
    let settings = settings_store().read().ok()?;
    settings
        .codex_config_dir
        .as_ref()
        .map(|p| resolve_override_path(p))
}

pub fn get_gemini_override_dir() -> Option<PathBuf> {
    let settings = settings_store().read().ok()?;
    settings
        .gemini_config_dir
        .as_ref()
        .map(|p| resolve_override_path(p))
}

pub fn get_opencode_override_dir() -> Option<PathBuf> {
    let settings = settings_store().read().ok()?;
    settings
        .opencode_config_dir
        .as_ref()
        .map(|p| resolve_override_path(p))
}

pub fn get_openclaw_override_dir() -> Option<PathBuf> {
    let settings = settings_store().read().ok()?;
    settings
        .openclaw_config_dir
        .as_ref()
        .map(|p| resolve_override_path(p))
}

pub fn get_hermes_override_dir() -> Option<PathBuf> {
    let settings = settings_store().read().ok()?;
    settings
        .hermes_config_dir
        .as_ref()
        .map(|p| resolve_override_path(p))
}

// ===== 当前供应商管理函数 =====

/// 获取指定应用类型的当前供应商 ID（从本地 settings 读取）
///
/// 这是设备级别的设置，不随数据库同步。
/// 如果本地没有设置，调用者应该 fallback 到数据库的 `is_current` 字段。
pub fn get_current_provider(app_type: &AppType) -> Option<String> {
    let settings = settings_store().read().ok()?;
    match app_type {
        AppType::Claude => settings.current_provider_claude.clone(),
        AppType::ClaudeDesktop => settings.current_provider_claude_desktop.clone(),
        AppType::Codex => settings.current_provider_codex.clone(),
        AppType::Gemini => settings.current_provider_gemini.clone(),
        AppType::OpenCode => settings.current_provider_opencode.clone(),
        AppType::OpenClaw => settings.current_provider_openclaw.clone(),
        AppType::Hermes => settings.current_provider_hermes.clone(),
    }
}

/// 设置指定应用类型的当前供应商 ID（保存到本地 settings）
///
/// 这是设备级别的设置，不随数据库同步。
/// 传入 `None` 会清除当前供应商设置。
pub fn set_current_provider(app_type: &AppType, id: Option<&str>) -> Result<(), AppError> {
    let id_owned = id.map(|s| s.to_string());
    mutate_settings(|settings| match app_type {
        AppType::Claude => settings.current_provider_claude = id_owned.clone(),
        AppType::ClaudeDesktop => settings.current_provider_claude_desktop = id_owned.clone(),
        AppType::Codex => settings.current_provider_codex = id_owned.clone(),
        AppType::Gemini => settings.current_provider_gemini = id_owned.clone(),
        AppType::OpenCode => settings.current_provider_opencode = id_owned.clone(),
        AppType::OpenClaw => settings.current_provider_openclaw = id_owned.clone(),
        AppType::Hermes => settings.current_provider_hermes = id_owned.clone(),
    })
}

/// 获取有效的当前供应商 ID（验证存在性）
///
/// 逻辑：
/// 1. 从本地 settings 读取当前供应商 ID
/// 2. 验证该 ID 在数据库中存在
/// 3. 如果不存在则清理本地 settings，fallback 到数据库的 is_current
///
/// 这确保了返回的 ID 一定是有效的（在数据库中存在）。
/// 多设备云同步场景下，配置导入后本地 ID 可能失效，此函数会自动修复。
pub fn get_effective_current_provider(
    db: &crate::database::Database,
    app_type: &AppType,
) -> Result<Option<String>, AppError> {
    // 1. 从本地 settings 读取
    if let Some(local_id) = get_current_provider(app_type) {
        // 2. 验证该 ID 在数据库中存在
        let providers = db.get_all_providers(app_type.as_str())?;
        if providers.contains_key(&local_id) {
            // 存在，直接返回
            return Ok(Some(local_id));
        }

        // 3. 不存在，清理本地 settings
        log::warn!(
            "本地 settings 中的供应商 {} ({}) 在数据库中不存在，将清理并 fallback 到数据库",
            local_id,
            app_type.as_str()
        );
        let _ = set_current_provider(app_type, None);
    }

    // Fallback 到数据库的 is_current
    db.get_current_provider(app_type.as_str())
}

// ===== Skill 同步方式管理函数 =====

/// 获取 Skill 同步方式配置
pub fn get_skill_sync_method() -> SyncMethod {
    settings_store()
        .read()
        .unwrap_or_else(|e| {
            log::warn!("设置锁已毒化，使用恢复值: {e}");
            e.into_inner()
        })
        .skill_sync_method
}

// ===== Skill 存储位置管理函数 =====

/// 获取 Skill 存储位置配置
pub fn get_skill_storage_location() -> SkillStorageLocation {
    settings_store()
        .read()
        .unwrap_or_else(|e| {
            log::warn!("设置锁已毒化，使用恢复值: {e}");
            e.into_inner()
        })
        .skill_storage_location
}

/// 设置 Skill 存储位置
pub fn set_skill_storage_location(location: SkillStorageLocation) -> Result<(), AppError> {
    mutate_settings(|s| {
        s.skill_storage_location = location;
    })
}

// ===== 备份策略管理函数 =====

/// Get the effective auto-backup interval in hours (default 24)
pub fn effective_backup_interval_hours() -> u32 {
    settings_store()
        .read()
        .unwrap_or_else(|e| {
            log::warn!("设置锁已毒化，使用恢复值: {e}");
            e.into_inner()
        })
        .backup_interval_hours
        .unwrap_or(24)
}

/// Get the effective backup retain count (default 10, minimum 1)
pub fn effective_backup_retain_count() -> usize {
    settings_store()
        .read()
        .unwrap_or_else(|e| {
            log::warn!("设置锁已毒化，使用恢复值: {e}");
            e.into_inner()
        })
        .backup_retain_count
        .map(|n| (n as usize).max(1))
        .unwrap_or(10)
}

// ===== 终端设置管理函数 =====

/// 获取首选终端应用
pub fn get_preferred_terminal() -> Option<String> {
    settings_store()
        .read()
        .unwrap_or_else(|e| {
            log::warn!("设置锁已毒化，使用恢复值: {e}");
            e.into_inner()
        })
        .preferred_terminal
        .clone()
}

// ===== WebDAV 同步设置管理函数 =====

/// 获取 WebDAV 同步设置
pub fn get_webdav_sync_settings() -> Option<WebDavSyncSettings> {
    settings_store().read().ok()?.webdav_sync.clone()
}

/// 保存 WebDAV 同步设置
pub fn set_webdav_sync_settings(settings: Option<WebDavSyncSettings>) -> Result<(), AppError> {
    mutate_settings(|current| {
        current.webdav_sync = settings;
    })
}

/// 仅更新 WebDAV 同步状态，避免覆写 credentials/root/profile 等字段
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
    use crate::app_config::AppType;

    #[test]
    fn visible_apps_old_settings_default_claude_desktop_visible() {
        let visible: VisibleApps = serde_json::from_value(serde_json::json!({
            "claude": true,
            "codex": true,
            "gemini": true,
            "opencode": true,
            "openclaw": true,
            "hermes": true
        }))
        .expect("visible apps");

        assert!(visible.is_visible(&AppType::ClaudeDesktop));
    }

    #[test]
    fn visible_apps_accepts_claude_desktop_aliases() {
        let visible: VisibleApps = serde_json::from_value(serde_json::json!({
            "claude": true,
            "claudeDesktop": false,
            "codex": true,
            "gemini": true,
            "opencode": true,
            "openclaw": true,
            "hermes": true
        }))
        .expect("visible apps");

        assert!(!visible.is_visible(&AppType::ClaudeDesktop));
    }

    #[test]
    fn profile_from_settings_snapshots_only_claude_code_and_codex() {
        let settings = AppSettings {
            claude_config_dir: Some("/profiles/claude".to_string()),
            codex_config_dir: Some("/profiles/codex".to_string()),
            gemini_config_dir: Some("/profiles/gemini".to_string()),
            opencode_config_dir: Some("/profiles/opencode".to_string()),
            openclaw_config_dir: Some("/profiles/openclaw".to_string()),
            hermes_config_dir: Some("/profiles/hermes".to_string()),
            current_provider_claude: Some("claude-provider".to_string()),
            current_provider_claude_desktop: Some("desktop-provider".to_string()),
            current_provider_codex: Some("codex-provider".to_string()),
            current_provider_gemini: Some("gemini-provider".to_string()),
            current_provider_opencode: Some("opencode-provider".to_string()),
            current_provider_openclaw: Some("openclaw-provider".to_string()),
            current_provider_hermes: Some("hermes-provider".to_string()),
            ..AppSettings::default()
        };

        let profile = Profile::from_settings("work", "Work", &settings);

        assert_eq!(
            profile.claude_config_dir.as_deref(),
            Some("/profiles/claude")
        );
        assert_eq!(profile.codex_config_dir.as_deref(), Some("/profiles/codex"));
        assert_eq!(
            profile.current_provider_claude.as_deref(),
            Some("claude-provider")
        );
        assert_eq!(
            profile.current_provider_codex.as_deref(),
            Some("codex-provider")
        );
        assert_eq!(profile.gemini_config_dir, None);
        assert_eq!(profile.opencode_config_dir, None);
        assert_eq!(profile.openclaw_config_dir, None);
        assert_eq!(profile.hermes_config_dir, None);
        assert_eq!(profile.current_provider_claude_desktop, None);
        assert_eq!(profile.current_provider_gemini, None);
        assert_eq!(profile.current_provider_opencode, None);
        assert_eq!(profile.current_provider_openclaw, None);
        assert_eq!(profile.current_provider_hermes, None);
    }

    #[test]
    fn applying_active_profile_preserves_unmanaged_app_fields() {
        let mut settings = AppSettings {
            claude_config_dir: Some("/old/claude".to_string()),
            codex_config_dir: Some("/old/codex".to_string()),
            gemini_config_dir: Some("/keep/gemini".to_string()),
            opencode_config_dir: Some("/keep/opencode".to_string()),
            openclaw_config_dir: Some("/keep/openclaw".to_string()),
            hermes_config_dir: Some("/keep/hermes".to_string()),
            current_provider_claude: Some("old-claude".to_string()),
            current_provider_claude_desktop: Some("keep-desktop".to_string()),
            current_provider_codex: Some("old-codex".to_string()),
            current_provider_gemini: Some("keep-gemini".to_string()),
            current_provider_opencode: Some("keep-opencode".to_string()),
            current_provider_openclaw: Some("keep-openclaw".to_string()),
            current_provider_hermes: Some("keep-hermes".to_string()),
            profiles: vec![Profile {
                id: "work".to_string(),
                label: "Work".to_string(),
                claude_config_dir: Some("/new/claude".to_string()),
                codex_config_dir: Some("/new/codex".to_string()),
                gemini_config_dir: Some("/ignored/gemini".to_string()),
                opencode_config_dir: Some("/ignored/opencode".to_string()),
                openclaw_config_dir: Some("/ignored/openclaw".to_string()),
                hermes_config_dir: Some("/ignored/hermes".to_string()),
                current_provider_claude: Some("new-claude".to_string()),
                current_provider_claude_desktop: Some("ignored-desktop".to_string()),
                current_provider_codex: Some("new-codex".to_string()),
                current_provider_gemini: Some("ignored-gemini".to_string()),
                current_provider_opencode: Some("ignored-opencode".to_string()),
                current_provider_openclaw: Some("ignored-openclaw".to_string()),
                current_provider_hermes: Some("ignored-hermes".to_string()),
            }],
            active_profile_id: Some("work".to_string()),
            ..AppSettings::default()
        };

        settings.apply_active_profile_to_single_fields();

        assert_eq!(settings.claude_config_dir.as_deref(), Some("/new/claude"));
        assert_eq!(settings.codex_config_dir.as_deref(), Some("/new/codex"));
        assert_eq!(
            settings.current_provider_claude.as_deref(),
            Some("new-claude")
        );
        assert_eq!(
            settings.current_provider_codex.as_deref(),
            Some("new-codex")
        );
        assert_eq!(settings.gemini_config_dir.as_deref(), Some("/keep/gemini"));
        assert_eq!(
            settings.opencode_config_dir.as_deref(),
            Some("/keep/opencode")
        );
        assert_eq!(
            settings.openclaw_config_dir.as_deref(),
            Some("/keep/openclaw")
        );
        assert_eq!(settings.hermes_config_dir.as_deref(), Some("/keep/hermes"));
        assert_eq!(
            settings.current_provider_claude_desktop.as_deref(),
            Some("keep-desktop")
        );
        assert_eq!(
            settings.current_provider_gemini.as_deref(),
            Some("keep-gemini")
        );
        assert_eq!(
            settings.current_provider_opencode.as_deref(),
            Some("keep-opencode")
        );
        assert_eq!(
            settings.current_provider_openclaw.as_deref(),
            Some("keep-openclaw")
        );
        assert_eq!(
            settings.current_provider_hermes.as_deref(),
            Some("keep-hermes")
        );
    }

    #[test]
    fn saving_single_fields_updates_only_active_profile_managed_fields() {
        let mut settings = AppSettings {
            claude_config_dir: Some("/active/claude".to_string()),
            codex_config_dir: Some("/active/codex".to_string()),
            gemini_config_dir: Some("/single/gemini".to_string()),
            current_provider_claude: Some("active-claude".to_string()),
            current_provider_codex: Some("active-codex".to_string()),
            current_provider_gemini: Some("single-gemini".to_string()),
            profiles: vec![Profile {
                id: "active".to_string(),
                label: "Active".to_string(),
                claude_config_dir: Some("/profile/claude".to_string()),
                codex_config_dir: Some("/profile/codex".to_string()),
                gemini_config_dir: Some("/profile/gemini".to_string()),
                current_provider_claude: Some("profile-claude".to_string()),
                current_provider_codex: Some("profile-codex".to_string()),
                current_provider_gemini: Some("profile-gemini".to_string()),
                ..Profile::default()
            }],
            active_profile_id: Some("active".to_string()),
            ..AppSettings::default()
        };

        settings.save_single_fields_to_active_profile();
        let profile = settings.profiles.first().expect("active profile");

        assert_eq!(profile.claude_config_dir.as_deref(), Some("/active/claude"));
        assert_eq!(profile.codex_config_dir.as_deref(), Some("/active/codex"));
        assert_eq!(
            profile.current_provider_claude.as_deref(),
            Some("active-claude")
        );
        assert_eq!(
            profile.current_provider_codex.as_deref(),
            Some("active-codex")
        );
        assert_eq!(
            profile.gemini_config_dir.as_deref(),
            Some("/profile/gemini")
        );
        assert_eq!(
            profile.current_provider_gemini.as_deref(),
            Some("profile-gemini")
        );
    }

    #[test]
    fn saving_then_applying_active_profile_uses_latest_single_fields() {
        let mut settings = AppSettings {
            claude_config_dir: Some("/latest/claude".to_string()),
            codex_config_dir: Some("/latest/codex".to_string()),
            current_provider_claude: Some("latest-claude".to_string()),
            current_provider_codex: Some("latest-codex".to_string()),
            profiles: vec![Profile {
                id: "active".to_string(),
                label: "Active".to_string(),
                claude_config_dir: Some("/stale/claude".to_string()),
                codex_config_dir: Some("/stale/codex".to_string()),
                current_provider_claude: Some("stale-claude".to_string()),
                current_provider_codex: Some("stale-codex".to_string()),
                ..Profile::default()
            }],
            active_profile_id: Some("active".to_string()),
            ..AppSettings::default()
        };

        settings.save_single_fields_to_active_profile();
        settings.apply_active_profile_to_single_fields();

        assert_eq!(
            settings.claude_config_dir.as_deref(),
            Some("/latest/claude")
        );
        assert_eq!(settings.codex_config_dir.as_deref(), Some("/latest/codex"));
        assert_eq!(
            settings.current_provider_claude.as_deref(),
            Some("latest-claude")
        );
        assert_eq!(
            settings.current_provider_codex.as_deref(),
            Some("latest-codex")
        );
    }
}
