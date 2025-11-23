use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock, RwLock};

use crate::database::Database;
use crate::error::AppError;

/// 自定义端点配置
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomEndpoint {
    pub url: String,
    pub added_at: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_used: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SecurityAuthSettings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SecuritySettings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth: Option<SecurityAuthSettings>,
}

/// 应用设置结构，允许覆盖默认配置目录
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    #[serde(default = "default_show_in_tray")]
    pub show_in_tray: bool,
    #[serde(default = "default_minimize_to_tray_on_close")]
    pub minimize_to_tray_on_close: bool,
    /// 是否启用 Claude 插件联动
    #[serde(default)]
    pub enable_claude_plugin_integration: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claude_config_dir: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codex_config_dir: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gemini_config_dir: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    /// 是否开机自启
    #[serde(default)]
    pub launch_on_startup: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub security: Option<SecuritySettings>,
    /// Claude 自定义端点列表
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub custom_endpoints_claude: HashMap<String, CustomEndpoint>,
    /// Codex 自定义端点列表
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub custom_endpoints_codex: HashMap<String, CustomEndpoint>,
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
            enable_claude_plugin_integration: false,
            claude_config_dir: None,
            codex_config_dir: None,
            gemini_config_dir: None,
            language: None,
            launch_on_startup: false,
            security: None,
            custom_endpoints_claude: HashMap::new(),
            custom_endpoints_codex: HashMap::new(),
        }
    }
}

impl AppSettings {
    fn settings_path() -> PathBuf {
        // settings.json 保留用于旧版本迁移和无数据库场景
        dirs::home_dir()
            .expect("无法获取用户主目录")
            .join(".cc-switch")
            .join("settings.json")
    }

    fn normalize_paths(&mut self) {
        self.claude_config_dir = self
            .claude_config_dir
            .as_ref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());

        self.codex_config_dir = self
            .codex_config_dir
            .as_ref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());

        self.gemini_config_dir = self
            .gemini_config_dir
            .as_ref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());

        self.language = self
            .language
            .as_ref()
            .map(|s| s.trim())
            .filter(|s| matches!(*s, "en" | "zh"))
            .map(|s| s.to_string());
    }

    fn load_from_file() -> Self {
        let path = Self::settings_path();
        if let Ok(content) = fs::read_to_string(&path) {
            match serde_json::from_str::<AppSettings>(&content) {
                Ok(mut settings) => {
                    settings.normalize_paths();
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
    normalized.normalize_paths();
    let path = AppSettings::settings_path();

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
    }

    let json = serde_json::to_string_pretty(&normalized)
        .map_err(|e| AppError::JsonSerialize { source: e })?;
    fs::write(&path, json).map_err(|e| AppError::io(&path, e))?;
    Ok(())
}

static SETTINGS_STORE: OnceLock<RwLock<AppSettings>> = OnceLock::new();

fn settings_store() -> &'static RwLock<AppSettings> {
    SETTINGS_STORE.get_or_init(|| RwLock::new(load_initial_settings()))
}

static SETTINGS_DB: OnceLock<Arc<Database>> = OnceLock::new();
const APP_SETTINGS_KEY: &str = "app_settings";

pub fn bind_db(db: Arc<Database>) {
    if SETTINGS_DB.set(db).is_err() {
        return;
    }

    if let Some(store) = SETTINGS_STORE.get() {
        let mut guard = store.write().expect("写入设置锁失败");
        *guard = load_initial_settings();
    }
}

fn load_initial_settings() -> AppSettings {
    if let Some(db) = SETTINGS_DB.get() {
        if let Some(from_db) = load_from_db(db.as_ref()) {
            return from_db;
        }

        // 从文件迁移一次并写入数据库
        let file_settings = AppSettings::load_from_file();
        if let Err(e) = save_to_db(db.as_ref(), &file_settings) {
            log::warn!("迁移设置到数据库失败，将继续使用内存副本: {e}");
        }
        return file_settings;
    }

    AppSettings::load_from_file()
}

fn load_from_db(db: &Database) -> Option<AppSettings> {
    let raw = db.get_setting(APP_SETTINGS_KEY).ok()??;
    match serde_json::from_str::<AppSettings>(&raw) {
        Ok(mut settings) => {
            settings.normalize_paths();
            Some(settings)
        }
        Err(err) => {
            log::warn!("解析数据库中 app_settings 失败: {err}");
            None
        }
    }
}

fn save_to_db(db: &Database, settings: &AppSettings) -> Result<(), AppError> {
    let mut normalized = settings.clone();
    normalized.normalize_paths();
    let json =
        serde_json::to_string(&normalized).map_err(|e| AppError::JsonSerialize { source: e })?;
    db.set_setting(APP_SETTINGS_KEY, &json)
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
    settings_store().read().expect("读取设置锁失败").clone()
}

pub fn update_settings(mut new_settings: AppSettings) -> Result<(), AppError> {
    new_settings.normalize_paths();
    if let Some(db) = SETTINGS_DB.get() {
        save_to_db(db, &new_settings)?;
    } else {
        save_settings_file(&new_settings)?;
    }

    let mut guard = settings_store().write().expect("写入设置锁失败");
    *guard = new_settings;
    Ok(())
}

pub fn ensure_security_auth_selected_type(selected_type: &str) -> Result<(), AppError> {
    let mut settings = get_settings();
    let current = settings
        .security
        .as_ref()
        .and_then(|sec| sec.auth.as_ref())
        .and_then(|auth| auth.selected_type.as_deref());

    if current == Some(selected_type) {
        return Ok(());
    }

    let mut security = settings.security.unwrap_or_default();
    let mut auth = security.auth.unwrap_or_default();
    auth.selected_type = Some(selected_type.to_string());
    security.auth = Some(auth);
    settings.security = Some(security);

    update_settings(settings)
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
