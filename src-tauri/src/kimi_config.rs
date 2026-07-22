//! Kimi Code 配置路径模块
//!
//! Kimi Code 的用户级配置目录为 `~/.kimi-code/`（全平台一致）：
//! - `config.toml` — 主配置（Provider/Model 定义，二期适配）
//! - `mcp.json` — MCP 服务器配置（标准 `{"mcpServers": {...}}` 结构）
//! - `skills/` — Skills 目录（每 skill 一子目录含 SKILL.md，与 Claude 约定兼容）
//! - `bin/kimi`（Windows: `kimi.exe`）— 可执行文件

use crate::settings::get_kimi_override_dir;
use std::path::PathBuf;

/// 获取 Kimi Code 配置目录
///
/// 解析顺序：
///   1. CCS 设置 `kimi_config_dir`（显式覆盖）
///   2. `KIMI_CODE_HOME` 环境变量（trim 后非空；按原样，不展开 `~`）
///   3. 平台默认 `~/.kimi-code`
pub fn get_kimi_dir() -> PathBuf {
    if let Some(override_dir) = get_kimi_override_dir() {
        return override_dir;
    }

    if let Some(raw) = std::env::var_os("KIMI_CODE_HOME") {
        let value = raw.to_string_lossy();
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }

    crate::config::get_home_dir().join(".kimi-code")
}

/// 获取 Kimi Code MCP 配置文件路径（~/.kimi-code/mcp.json）
pub fn get_kimi_mcp_path() -> PathBuf {
    get_kimi_dir().join("mcp.json")
}

/// 获取 Kimi Code 主配置文件路径（~/.kimi-code/config.toml）
pub fn get_kimi_config_path() -> PathBuf {
    get_kimi_dir().join("config.toml")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mcp_path_is_under_kimi_dir() {
        let dir = get_kimi_dir();
        let mcp_path = get_kimi_mcp_path();
        assert_eq!(mcp_path, dir.join("mcp.json"));
        assert!(mcp_path.ends_with("mcp.json"));
    }
}
