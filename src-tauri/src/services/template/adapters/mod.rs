//! 应用适配器模块
//!
//! 负责将 Template 组件安装到不同应用的配置目录中。
//! 每个应用有独立的适配器实现，处理各自的配置格式和目录结构。

mod claude;
mod codex;
mod gemini;

use anyhow::Result;
use std::path::PathBuf;

pub use claude::ClaudeAdapter;
pub use codex::CodexAdapter;
pub use gemini::GeminiAdapter;

use crate::app_config::AppType;

/// 应用适配器 trait
///
/// 定义了将 Template 组件安装到应用配置目录的统一接口。
/// 每个应用实现自己的适配器来处理特定的配置格式和目录结构。
#[allow(dead_code)]
pub trait AppAdapter: Send + Sync {
    /// 安装 Agent 到应用配置目录
    ///
    /// # 参数
    /// - `content`: Agent 内容（Markdown 格式）
    /// - `name`: Agent 名称（用作文件名）
    ///
    /// # 返回
    /// 安装后的文件路径
    fn install_agent(&self, content: &str, name: &str) -> Result<PathBuf>;

    /// 安装 Command 到应用配置目录
    ///
    /// # 参数
    /// - `content`: Command 内容（Markdown 格式）
    /// - `name`: Command 名称（用作文件名）
    ///
    /// # 返回
    /// 安装后的文件路径
    fn install_command(&self, content: &str, name: &str) -> Result<PathBuf>;

    /// 安装 MCP 服务器配置
    ///
    /// # 参数
    /// - `mcp_config`: MCP 服务器配置（JSON 对象）
    ///
    /// # 说明
    /// 配置会合并到应用的 MCP 配置文件中，保留现有配置。
    fn install_mcp(&self, mcp_config: &serde_json::Value) -> Result<()>;

    /// 安装 Setting (permissions)
    ///
    /// # 参数
    /// - `setting_config`: Setting 配置（JSON 对象）
    ///
    /// # 说明
    /// 仅 Claude 支持此功能，会合并到 settings.json 的 permissions 字段。
    fn install_setting(&self, setting_config: &serde_json::Value) -> Result<()>;

    /// 安装 Hook
    ///
    /// # 参数
    /// - `hook_config`: Hook 配置（JSON 对象）
    ///
    /// # 说明
    /// 仅 Claude 支持此功能，会合并到 settings.json 的 hooks 字段。
    fn install_hook(&self, hook_config: &serde_json::Value) -> Result<()>;

    /// 卸载组件
    ///
    /// # 参数
    /// - `component_type`: 组件类型（agent/command/mcp/setting/hook）
    /// - `name`: 组件名称或 ID
    fn uninstall(&self, component_type: &str, name: &str) -> Result<()>;

    /// 获取配置目录路径
    fn config_dir(&self) -> PathBuf;

    /// 检查组件类型是否支持
    ///
    /// # 参数
    /// - `component_type`: 组件类型字符串
    ///
    /// # 返回
    /// 如果应用支持该组件类型返回 true，否则返回 false
    #[allow(dead_code)]
    fn supports_component_type(&self, component_type: &str) -> bool;
}

/// 创建应用适配器工厂函数
///
/// # 参数
/// - `app_type`: 应用类型
///
/// # 返回
/// 对应应用的适配器实例
#[allow(dead_code)]
pub fn create_adapter(app_type: &AppType) -> Box<dyn AppAdapter> {
    match app_type {
        AppType::Claude => Box::new(ClaudeAdapter::new()),
        AppType::Codex => Box::new(CodexAdapter::new()),
        AppType::Gemini => Box::new(GeminiAdapter::new()),
    }
}
