//! Claude 应用适配器
//!
//! 完整支持所有组件类型：
//! - Agent → `~/.claude/agents/{name}.md`
//! - Command → `~/.claude/commands/{name}.md`
//! - MCP → 合并到 `~/.claude.json` 的 mcpServers 字段
//! - Setting → 合并到 `~/.claude/settings.json` 的 permissions 字段
//! - Hook → 合并到 `~/.claude/settings.json` 的 hooks 字段

use anyhow::{Context, Result};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

use super::AppAdapter;
use crate::config::{atomic_write, get_claude_config_dir, get_claude_mcp_path};

/// Claude 应用适配器
pub struct ClaudeAdapter {
    config_dir: PathBuf,
}

impl ClaudeAdapter {
    /// 创建新的 Claude 适配器实例
    pub fn new() -> Self {
        Self {
            config_dir: get_claude_config_dir(),
        }
    }

    /// 读取 JSON 配置文件
    fn read_json_file(path: &PathBuf) -> Result<Value> {
        if !path.exists() {
            return Ok(serde_json::json!({}));
        }
        let content = fs::read_to_string(path)
            .with_context(|| format!("读取配置文件失败: {}", path.display()))?;
        let value: Value = serde_json::from_str(&content)
            .with_context(|| format!("解析 JSON 失败: {}", path.display()))?;
        Ok(value)
    }

    /// 写入 JSON 配置文件（原子写入）
    fn write_json_file(path: &Path, value: &Value) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("创建目录失败: {}", parent.display()))?;
        }

        let json = serde_json::to_string_pretty(value).context("序列化 JSON 失败")?;

        atomic_write(path, json.as_bytes())
            .with_context(|| format!("写入配置文件失败: {}", path.display()))?;

        Ok(())
    }

    /// 合并两个 JSON 对象（深度合并）
    fn merge_json(base: &mut Value, overlay: &Value) {
        if let (Some(base_obj), Some(overlay_obj)) = (base.as_object_mut(), overlay.as_object()) {
            for (key, value) in overlay_obj {
                if let Some(base_value) = base_obj.get_mut(key) {
                    // 如果两边都是对象，递归合并
                    if base_value.is_object() && value.is_object() {
                        Self::merge_json(base_value, value);
                    } else {
                        // 否则直接覆盖
                        *base_value = value.clone();
                    }
                } else {
                    // 键不存在，直接插入
                    base_obj.insert(key.clone(), value.clone());
                }
            }
        }
    }

    /// 安装 Markdown 文件（通用）
    fn install_markdown_file(&self, content: &str, subdir: &str, name: &str) -> Result<PathBuf> {
        let dir = self.config_dir.join(subdir);
        fs::create_dir_all(&dir).with_context(|| format!("创建目录失败: {}", dir.display()))?;

        let filename = if name.ends_with(".md") {
            name.to_string()
        } else {
            format!("{name}.md")
        };

        let file_path = dir.join(&filename);

        atomic_write(&file_path, content.as_bytes())
            .with_context(|| format!("写入文件失败: {}", file_path.display()))?;

        log::info!("已安装 Claude {}: {}", subdir, file_path.display());
        Ok(file_path)
    }

    /// 获取 Claude settings.json 路径
    fn get_settings_path(&self) -> PathBuf {
        crate::config::get_claude_settings_path()
    }
}

impl AppAdapter for ClaudeAdapter {
    fn install_agent(&self, content: &str, name: &str) -> Result<PathBuf> {
        self.install_markdown_file(content, "agents", name)
    }

    fn install_command(&self, content: &str, name: &str) -> Result<PathBuf> {
        self.install_markdown_file(content, "commands", name)
    }

    fn install_mcp(&self, mcp_config: &Value) -> Result<()> {
        let mcp_path = get_claude_mcp_path();

        // 读取现有 MCP 配置
        let mut current = Self::read_json_file(&mcp_path)?;

        // 确保 mcpServers 字段存在
        if !current.is_object() {
            current = serde_json::json!({});
        }
        if current.get("mcpServers").is_none() {
            current["mcpServers"] = serde_json::json!({});
        }

        // 合并新的 MCP 服务器配置
        if let Some(mcp_servers) = current.get_mut("mcpServers") {
            Self::merge_json(mcp_servers, mcp_config);
        }

        // 写回配置文件
        Self::write_json_file(&mcp_path, &current)?;

        log::info!("已安装 Claude MCP 配置到: {}", mcp_path.display());
        Ok(())
    }

    fn install_setting(&self, setting_config: &Value) -> Result<()> {
        let settings_path = self.get_settings_path();

        // 读取现有配置
        let mut current = Self::read_json_file(&settings_path)?;

        // 确保 permissions 字段存在
        if !current.is_object() {
            current = serde_json::json!({});
        }
        if current.get("permissions").is_none() {
            current["permissions"] = serde_json::json!({});
        }

        // 合并新的 permissions 配置
        if let Some(permissions) = current.get_mut("permissions") {
            Self::merge_json(permissions, setting_config);
        }

        // 写回配置文件
        Self::write_json_file(&settings_path, &current)?;

        log::info!("已安装 Claude Setting 配置到: {}", settings_path.display());
        Ok(())
    }

    fn install_hook(&self, hook_config: &Value) -> Result<()> {
        let settings_path = self.get_settings_path();

        // 读取现有配置
        let mut current = Self::read_json_file(&settings_path)?;

        // 确保 hooks 字段存在
        if !current.is_object() {
            current = serde_json::json!({});
        }
        if current.get("hooks").is_none() {
            current["hooks"] = serde_json::json!({});
        }

        // 合并新的 hooks 配置
        if let Some(hooks) = current.get_mut("hooks") {
            Self::merge_json(hooks, hook_config);
        }

        // 写回配置文件
        Self::write_json_file(&settings_path, &current)?;

        log::info!("已安装 Claude Hook 配置到: {}", settings_path.display());
        Ok(())
    }

    fn uninstall(&self, component_type: &str, name: &str) -> Result<()> {
        match component_type.to_lowercase().as_str() {
            "agent" => {
                let path = self.config_dir.join("agents").join(format!("{name}.md"));
                if path.exists() {
                    fs::remove_file(&path)
                        .with_context(|| format!("删除 Agent 文件失败: {}", path.display()))?;
                    log::info!("已卸载 Claude Agent: {}", path.display());
                }
            }
            "command" => {
                let path = self.config_dir.join("commands").join(format!("{name}.md"));
                if path.exists() {
                    fs::remove_file(&path)
                        .with_context(|| format!("删除 Command 文件失败: {}", path.display()))?;
                    log::info!("已卸载 Claude Command: {}", path.display());
                }
            }
            "mcp" => {
                let mcp_path = get_claude_mcp_path();
                let mut current = Self::read_json_file(&mcp_path)?;

                if let Some(mcp_servers) = current
                    .get_mut("mcpServers")
                    .and_then(|v| v.as_object_mut())
                {
                    mcp_servers.remove(name);
                    Self::write_json_file(&mcp_path, &current)?;
                    log::info!("已卸载 Claude MCP 服务器: {name}");
                }
            }
            "setting" => {
                let settings_path = self.get_settings_path();
                let mut current = Self::read_json_file(&settings_path)?;

                if let Some(permissions) = current
                    .get_mut("permissions")
                    .and_then(|v| v.as_object_mut())
                {
                    permissions.remove(name);
                    Self::write_json_file(&settings_path, &current)?;
                    log::info!("已卸载 Claude Setting: {name}");
                }
            }
            "hook" => {
                let settings_path = self.get_settings_path();
                let mut current = Self::read_json_file(&settings_path)?;

                if let Some(hooks) = current.get_mut("hooks").and_then(|v| v.as_object_mut()) {
                    hooks.remove(name);
                    Self::write_json_file(&settings_path, &current)?;
                    log::info!("已卸载 Claude Hook: {name}");
                }
            }
            _ => anyhow::bail!("不支持的组件类型: {component_type}"),
        }

        Ok(())
    }

    fn config_dir(&self) -> PathBuf {
        self.config_dir.clone()
    }

    fn supports_component_type(&self, component_type: &str) -> bool {
        matches!(
            component_type.to_lowercase().as_str(),
            "agent" | "command" | "mcp" | "setting" | "hook"
        )
    }
}

impl Default for ClaudeAdapter {
    fn default() -> Self {
        Self::new()
    }
}
