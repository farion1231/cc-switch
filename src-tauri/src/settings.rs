use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{OnceLock, RwLock};

/// 自定义端点配置
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomEndpoint {
    pub url: String,
    pub added_at: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_used: Option<i64>,
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
    pub language: Option<String>,
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
            language: None,
            custom_endpoints_claude: HashMap::new(),
            custom_endpoints_codex: HashMap::new(),
        }
    }
}

impl AppSettings {
    fn settings_path() -> PathBuf {
        // settings.json 必须使用固定路径，不能被 app_config_dir 覆盖
        // 否则会造成循环依赖：读取 settings 需要知道路径，但路径在 settings 中
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

        self.language = self
            .language
            .as_ref()
            .map(|s| s.trim())
            .filter(|s| matches!(*s, "en" | "zh"))
            .map(|s| s.to_string());
    }

    pub fn load() -> Self {
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

    pub fn save(&self) -> Result<(), String> {
        let mut normalized = self.clone();
        normalized.normalize_paths();
        let path = Self::settings_path();

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("创建设置目录失败: {}", e))?;
        }

        let json = serde_json::to_string_pretty(&normalized)
            .map_err(|e| format!("序列化设置失败: {}", e))?;
        fs::write(&path, json).map_err(|e| format!("写入设置失败: {}", e))?;
        Ok(())
    }
}

fn settings_store() -> &'static RwLock<AppSettings> {
    static STORE: OnceLock<RwLock<AppSettings>> = OnceLock::new();
    STORE.get_or_init(|| RwLock::new(AppSettings::load()))
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

pub fn update_settings(mut new_settings: AppSettings) -> Result<(), String> {
    new_settings.normalize_paths();
    new_settings.save()?;

    let mut guard = settings_store().write().expect("写入设置锁失败");
    *guard = new_settings;
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