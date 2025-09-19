use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::config::{copy_file, get_app_config_dir, get_app_config_path, write_json_file};
use crate::provider::ProviderManager;

/// 应用类型
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AppType {
    Claude,
    Codex,
}

impl AppType {
    pub fn as_str(&self) -> &str {
        match self {
            AppType::Claude => "claude",
            AppType::Codex => "codex",
        }
    }
}

impl From<&str> for AppType {
    fn from(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "codex" => AppType::Codex,
            _ => AppType::Claude, // 默认为 Claude
        }
    }
}

/// 应用设置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    /// 是否在系统托盘显示图标
    #[serde(default = "default_show_in_tray")]
    pub show_in_tray: bool,
    /// 点击关闭按钮时是否最小化到托盘而不是关闭应用
    #[serde(default = "default_minimize_to_tray_on_close")]
    pub minimize_to_tray_on_close: bool,
}

fn default_show_in_tray() -> bool {
    true
}

fn default_minimize_to_tray_on_close() -> bool {
    true
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            show_in_tray: true,
            minimize_to_tray_on_close: true,
        }
    }
}

/// 多应用配置结构（向后兼容）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultiAppConfig {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default)]
    pub settings: Settings,
    #[serde(flatten)]
    pub apps: HashMap<String, ProviderManager>,
}

fn default_version() -> u32 {
    2
}

impl Default for MultiAppConfig {
    fn default() -> Self {
        let mut apps = HashMap::new();
        apps.insert("claude".to_string(), ProviderManager::default());
        apps.insert("codex".to_string(), ProviderManager::default());

        Self { 
            version: 2, 
            settings: Settings::default(),
            apps 
        }
    }
}

impl MultiAppConfig {
    /// 从文件加载配置（处理v1到v2的迁移）
    pub fn load() -> Result<Self, String> {
        let config_path = get_app_config_path();

        if !config_path.exists() {
            log::info!("配置文件不存在，创建新的多应用配置");
            return Ok(Self::default());
        }

        // 尝试读取文件
        let content = std::fs::read_to_string(&config_path)
            .map_err(|e| format!("读取配置文件失败: {}", e))?;

        // 检查是否是旧版本格式（v1）
        if let Ok(v1_config) = serde_json::from_str::<ProviderManager>(&content) {
            log::info!("检测到v1配置，自动迁移到v2");

            // 迁移到新格式
            let mut apps = HashMap::new();
            apps.insert("claude".to_string(), v1_config);
            apps.insert("codex".to_string(), ProviderManager::default());

            let config = Self { 
                version: 2, 
                settings: Settings::default(),
                apps 
            };

            // 迁移前备份旧版(v1)配置文件
            let backup_dir = get_app_config_dir();
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let backup_path = backup_dir.join(format!("config.v1.backup.{}.json", ts));

            match copy_file(&config_path, &backup_path) {
                Ok(()) => log::info!(
                    "已备份旧版配置文件: {} -> {}",
                    config_path.display(),
                    backup_path.display()
                ),
                Err(e) => log::warn!("备份旧版配置文件失败: {}", e),
            }

            // 保存迁移后的配置
            config.save()?;
            return Ok(config);
        }

        // 尝试读取v2格式
        serde_json::from_str::<Self>(&content).map_err(|e| format!("解析配置文件失败: {}", e))
    }

    /// 保存配置到文件
    pub fn save(&self) -> Result<(), String> {
        let config_path = get_app_config_path();
        // 先备份旧版（若存在）到 ~/.cc-switch/config.json.bak，再写入新内容
        if config_path.exists() {
            let backup_path = get_app_config_dir().join("config.json.bak");
            if let Err(e) = copy_file(&config_path, &backup_path) {
                log::warn!("备份 config.json 到 .bak 失败: {}", e);
            }
        }

        write_json_file(&config_path, self)?;
        Ok(())
    }

    /// 获取指定应用的管理器
    pub fn get_manager(&self, app: &AppType) -> Option<&ProviderManager> {
        self.apps.get(app.as_str())
    }

    /// 获取指定应用的管理器（可变引用）
    pub fn get_manager_mut(&mut self, app: &AppType) -> Option<&mut ProviderManager> {
        self.apps.get_mut(app.as_str())
    }

    /// 确保应用存在
    pub fn ensure_app(&mut self, app: &AppType) {
        if !self.apps.contains_key(app.as_str()) {
            self.apps
                .insert(app.as_str().to_string(), ProviderManager::default());
        }
    }
}
