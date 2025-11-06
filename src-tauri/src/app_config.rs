use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::str::FromStr;

/// MCP 配置：单客户端维度（claude 或 codex 下的一组服务器）
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct McpConfig {
    /// 以 id 为键的服务器定义（宽松 JSON 对象，包含 enabled/source 等 UI 辅助字段）
    #[serde(default)]
    pub servers: HashMap<String, serde_json::Value>,
}

/// MCP 根：按客户端分开维护（无历史兼容压力，直接以 v2 结构落地）
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct McpRoot {
    #[serde(default)]
    pub claude: McpConfig,
    #[serde(default)]
    pub codex: McpConfig,
}

/// Prompt 配置：单客户端维度
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PromptConfig {
    #[serde(default)]
    pub prompts: HashMap<String, crate::prompt::Prompt>,
}

/// Prompt 根：按客户端分开维护
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PromptRoot {
    #[serde(default)]
    pub claude: PromptConfig,
    #[serde(default)]
    pub codex: PromptConfig,
}

use crate::config::{copy_file, get_app_config_dir, get_app_config_path, write_json_file};
use crate::error::AppError;
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

impl FromStr for AppType {
    type Err = AppError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let normalized = s.trim().to_lowercase();
        match normalized.as_str() {
            "claude" => Ok(AppType::Claude),
            "codex" => Ok(AppType::Codex),
            other => Err(AppError::localized(
                "unsupported_app",
                format!("不支持的应用标识: '{other}'。可选值: claude, codex。"),
                format!("Unsupported app id: '{other}'. Allowed: claude, codex."),
            )),
        }
    }
}

/// 多应用配置结构（向后兼容）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultiAppConfig {
    #[serde(default = "default_version")]
    pub version: u32,
    /// 应用管理器（claude/codex）
    #[serde(flatten)]
    pub apps: HashMap<String, ProviderManager>,
    /// MCP 配置（按客户端分治）
    #[serde(default)]
    pub mcp: McpRoot,
    /// Prompt 配置（按客户端分治）
    #[serde(default)]
    pub prompts: PromptRoot,
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
            apps,
            mcp: McpRoot::default(),
            prompts: PromptRoot::default(),
        }
    }
}

impl MultiAppConfig {
    /// 从文件加载配置（仅支持 v2 结构）
    pub fn load() -> Result<Self, AppError> {
        let config_path = get_app_config_path();

        if !config_path.exists() {
            log::info!("配置文件不存在，创建新的多应用配置");
            return Ok(Self::default());
        }

        // 尝试读取文件
        let content =
            std::fs::read_to_string(&config_path).map_err(|e| AppError::io(&config_path, e))?;

        // 先解析为 Value，以便严格判定是否为 v1 结构；
        // 满足：顶层同时包含 providers(object) + current(string)，且不包含 version/apps/mcp 关键键，即视为 v1
        let value: serde_json::Value =
            serde_json::from_str(&content).map_err(|e| AppError::json(&config_path, e))?;
        let is_v1 = value.as_object().map_or(false, |map| {
            let has_providers = map
                .get("providers")
                .map(|v| v.is_object())
                .unwrap_or(false);
            let has_current = map
                .get("current")
                .map(|v| v.is_string())
                .unwrap_or(false);
            // v1 的充分必要条件：有 providers 和 current，且 apps 不存在（version/mcp 可能存在但不作为 v2 判据）
            let has_apps = map.contains_key("apps");
            has_providers && has_current && !has_apps
        });
        if is_v1 {
            return Err(AppError::localized(
                "config.unsupported_v1",
                "检测到旧版 v1 配置格式。当前版本已不再支持运行时自动迁移。\n\n解决方案：\n1. 安装 v3.2.x 版本进行一次性自动迁移\n2. 或手动编辑 ~/.cc-switch/config.json，将顶层结构调整为：\n   {\"version\": 2, \"claude\": {...}, \"codex\": {...}, \"mcp\": {...}}\n\n",
                "Detected legacy v1 config. Runtime auto-migration is no longer supported.\n\nSolutions:\n1. Install v3.2.x for one-time auto-migration\n2. Or manually edit ~/.cc-switch/config.json to adjust the top-level structure:\n   {\"version\": 2, \"claude\": {...}, \"codex\": {...}, \"mcp\": {...}}\n\n",
            ));
        }

        // 解析 v2 结构
        serde_json::from_value::<Self>(value).map_err(|e| AppError::json(&config_path, e))
    }

    /// 保存配置到文件
    pub fn save(&self) -> Result<(), AppError> {
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

    /// 获取指定客户端的 MCP 配置（不可变引用）
    pub fn mcp_for(&self, app: &AppType) -> &McpConfig {
        match app {
            AppType::Claude => &self.mcp.claude,
            AppType::Codex => &self.mcp.codex,
        }
    }

    /// 获取指定客户端的 MCP 配置（可变引用）
    pub fn mcp_for_mut(&mut self, app: &AppType) -> &mut McpConfig {
        match app {
            AppType::Claude => &mut self.mcp.claude,
            AppType::Codex => &mut self.mcp.codex,
        }
    }
}
