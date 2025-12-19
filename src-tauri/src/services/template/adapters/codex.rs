//! Codex 应用适配器
//!
//! 部分支持：
//! - Agent → `~/.codex/agents/{name}.md`
//! - Command → `~/.codex/commands/{name}.md`
//! - MCP → 合并到 `~/.codex/config.toml` 的 [mcp_servers] 表
//! - Setting/Hook → 不支持（Codex 不支持这些功能）

use anyhow::{bail, Context, Result};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

use super::AppAdapter;
use crate::codex_config::get_codex_config_dir;
use crate::config::{atomic_write, write_text_file};

/// Codex 应用适配器
pub struct CodexAdapter {
    config_dir: PathBuf,
}

impl CodexAdapter {
    /// 创建新的 Codex 适配器实例
    pub fn new() -> Self {
        Self {
            config_dir: get_codex_config_dir(),
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

        log::info!("已安装 Codex {}: {}", subdir, file_path.display());
        Ok(file_path)
    }

    /// 获取 Codex config.toml 路径
    fn get_config_toml_path(&self) -> PathBuf {
        crate::codex_config::get_codex_config_path()
    }

    /// 读取 TOML 配置文件
    fn read_toml_file(path: &PathBuf) -> Result<toml::Table> {
        if !path.exists() {
            return Ok(toml::Table::new());
        }
        let content = fs::read_to_string(path)
            .with_context(|| format!("读取配置文件失败: {}", path.display()))?;
        let table: toml::Table = toml::from_str(&content)
            .with_context(|| format!("解析 TOML 失败: {}", path.display()))?;
        Ok(table)
    }

    /// 写入 TOML 配置文件（原子写入）
    fn write_toml_file(path: &Path, table: &toml::Table) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("创建目录失败: {}", parent.display()))?;
        }

        let toml_string = toml::to_string_pretty(table).context("序列化 TOML 失败")?;

        write_text_file(path, &toml_string)
            .with_context(|| format!("写入配置文件失败: {}", path.display()))?;

        Ok(())
    }

    /// 将 JSON MCP 配置转换为 TOML 格式
    fn json_mcp_to_toml(json_config: &Value) -> Result<toml::Table> {
        let mut mcp_servers = toml::Table::new();

        if let Some(obj) = json_config.as_object() {
            for (server_id, server_spec) in obj {
                let mut server_table = toml::Table::new();

                if let Some(spec_obj) = server_spec.as_object() {
                    // type 字段（默认 stdio）
                    let server_type = spec_obj
                        .get("type")
                        .and_then(|v| v.as_str())
                        .unwrap_or("stdio");
                    server_table.insert(
                        "type".to_string(),
                        toml::Value::String(server_type.to_string()),
                    );

                    match server_type {
                        "stdio" => {
                            // command 字段（必需）
                            if let Some(cmd) = spec_obj.get("command").and_then(|v| v.as_str()) {
                                server_table.insert(
                                    "command".to_string(),
                                    toml::Value::String(cmd.to_string()),
                                );
                            }

                            // args 字段（可选）
                            if let Some(args) = spec_obj.get("args").and_then(|v| v.as_array()) {
                                let toml_args: Vec<toml::Value> = args
                                    .iter()
                                    .filter_map(|v| v.as_str())
                                    .map(|s| toml::Value::String(s.to_string()))
                                    .collect();
                                if !toml_args.is_empty() {
                                    server_table
                                        .insert("args".to_string(), toml::Value::Array(toml_args));
                                }
                            }

                            // env 字段（可选）
                            if let Some(env) = spec_obj.get("env").and_then(|v| v.as_object()) {
                                let mut env_table = toml::Table::new();
                                for (key, value) in env {
                                    if let Some(val_str) = value.as_str() {
                                        env_table.insert(
                                            key.clone(),
                                            toml::Value::String(val_str.to_string()),
                                        );
                                    }
                                }
                                if !env_table.is_empty() {
                                    server_table
                                        .insert("env".to_string(), toml::Value::Table(env_table));
                                }
                            }

                            // cwd 字段（可选）
                            if let Some(cwd) = spec_obj.get("cwd").and_then(|v| v.as_str()) {
                                server_table.insert(
                                    "cwd".to_string(),
                                    toml::Value::String(cwd.to_string()),
                                );
                            }
                        }
                        "http" | "sse" => {
                            // url 字段（必需）
                            if let Some(url) = spec_obj.get("url").and_then(|v| v.as_str()) {
                                server_table.insert(
                                    "url".to_string(),
                                    toml::Value::String(url.to_string()),
                                );
                            }

                            // http_headers 字段（可选）
                            if let Some(headers) =
                                spec_obj.get("http_headers").and_then(|v| v.as_object())
                            {
                                let mut headers_table = toml::Table::new();
                                for (key, value) in headers {
                                    if let Some(val_str) = value.as_str() {
                                        headers_table.insert(
                                            key.clone(),
                                            toml::Value::String(val_str.to_string()),
                                        );
                                    }
                                }
                                if !headers_table.is_empty() {
                                    server_table.insert(
                                        "http_headers".to_string(),
                                        toml::Value::Table(headers_table),
                                    );
                                }
                            }
                        }
                        _ => {}
                    }
                }

                mcp_servers.insert(server_id.clone(), toml::Value::Table(server_table));
            }
        }

        Ok(mcp_servers)
    }
}

impl AppAdapter for CodexAdapter {
    fn install_agent(&self, content: &str, name: &str) -> Result<PathBuf> {
        self.install_markdown_file(content, "agents", name)
    }

    fn install_command(&self, content: &str, name: &str) -> Result<PathBuf> {
        self.install_markdown_file(content, "commands", name)
    }

    fn install_mcp(&self, mcp_config: &Value) -> Result<()> {
        let config_path = self.get_config_toml_path();

        // 读取现有 TOML 配置
        let mut current = Self::read_toml_file(&config_path)?;

        // 确保 mcp_servers 表存在
        if !current.contains_key("mcp_servers") {
            current.insert(
                "mcp_servers".to_string(),
                toml::Value::Table(toml::Table::new()),
            );
        }

        // 转换 JSON MCP 配置到 TOML
        let new_mcp_servers = Self::json_mcp_to_toml(mcp_config)?;

        // 合并 MCP 服务器配置
        if let Some(mcp_servers) = current
            .get_mut("mcp_servers")
            .and_then(|v| v.as_table_mut())
        {
            for (server_id, server_config) in new_mcp_servers {
                mcp_servers.insert(server_id, server_config);
            }
        }

        // 写回配置文件
        Self::write_toml_file(&config_path, &current)?;

        log::info!("已安装 Codex MCP 配置到: {}", config_path.display());
        Ok(())
    }

    fn install_setting(&self, _setting_config: &Value) -> Result<()> {
        bail!("Codex 不支持 Setting 配置")
    }

    fn install_hook(&self, _hook_config: &Value) -> Result<()> {
        bail!("Codex 不支持 Hook 配置")
    }

    fn uninstall(&self, component_type: &str, name: &str) -> Result<()> {
        match component_type.to_lowercase().as_str() {
            "agent" => {
                let path = self.config_dir.join("agents").join(format!("{name}.md"));
                if path.exists() {
                    fs::remove_file(&path)
                        .with_context(|| format!("删除 Agent 文件失败: {}", path.display()))?;
                    log::info!("已卸载 Codex Agent: {}", path.display());
                }
            }
            "command" => {
                let path = self.config_dir.join("commands").join(format!("{name}.md"));
                if path.exists() {
                    fs::remove_file(&path)
                        .with_context(|| format!("删除 Command 文件失败: {}", path.display()))?;
                    log::info!("已卸载 Codex Command: {}", path.display());
                }
            }
            "mcp" => {
                let config_path = self.get_config_toml_path();
                let mut current = Self::read_toml_file(&config_path)?;

                if let Some(mcp_servers) = current
                    .get_mut("mcp_servers")
                    .and_then(|v| v.as_table_mut())
                {
                    mcp_servers.remove(name);
                    Self::write_toml_file(&config_path, &current)?;
                    log::info!("已卸载 Codex MCP 服务器: {name}");
                }
            }
            "setting" | "hook" => {
                bail!("Codex 不支持 {component_type} 组件类型")
            }
            _ => bail!("不支持的组件类型: {component_type}"),
        }

        Ok(())
    }

    fn config_dir(&self) -> PathBuf {
        self.config_dir.clone()
    }

    fn supports_component_type(&self, component_type: &str) -> bool {
        matches!(
            component_type.to_lowercase().as_str(),
            "agent" | "command" | "mcp"
        )
    }
}

impl Default for CodexAdapter {
    fn default() -> Self {
        Self::new()
    }
}
