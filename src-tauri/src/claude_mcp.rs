use serde_json::{Map, Value};
use std::fs;
use std::path::{Path, PathBuf};

use crate::config::{atomic_write, get_claude_mcp_path, is_wsl_path};
use crate::error::AppError;

/// 需要在 Windows 上用 cmd /c 包装的命令
/// 这些命令在 Windows 上实际是 .cmd 批处理文件，需要通过 cmd /c 来执行
#[cfg(windows)]
const WINDOWS_WRAP_COMMANDS: &[&str] = &["npx", "npm", "yarn", "pnpm", "node", "bun", "deno"];

/// Windows 平台：将 `npx args...` 转换为 `cmd /c npx args...`
/// 解决 Claude Code /doctor 报告的 "Windows requires 'cmd /c' wrapper to execute npx" 警告
#[cfg(windows)]
fn wrap_command_for_windows(obj: &mut Map<String, Value>) {
    // 只处理 stdio 类型（默认或显式）
    let server_type = obj.get("type").and_then(|v| v.as_str()).unwrap_or("stdio");
    if server_type != "stdio" {
        return;
    }

    let Some(cmd) = obj.get("command").and_then(|v| v.as_str()) else {
        return;
    };

    // 已经是 cmd 的不重复包装
    if cmd.eq_ignore_ascii_case("cmd") || cmd.eq_ignore_ascii_case("cmd.exe") {
        return;
    }

    // 提取命令名（去掉 .cmd 后缀和路径）
    let cmd_name = Path::new(cmd)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(cmd);

    let needs_wrap = WINDOWS_WRAP_COMMANDS
        .iter()
        .any(|&c| cmd_name.eq_ignore_ascii_case(c));

    if !needs_wrap {
        return;
    }

    // 构建新的 args: ["/c", "原命令", ...原args]
    let original_args = obj
        .get("args")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let mut new_args = vec![Value::String("/c".into()), Value::String(cmd.into())];
    new_args.extend(original_args);

    obj.insert("command".into(), Value::String("cmd".into()));
    obj.insert("args".into(), Value::Array(new_args));
}

/// 非 Windows 平台无需处理
#[cfg(not(windows))]
fn wrap_command_for_windows(_obj: &mut Map<String, Value>) {
    // 非 Windows 平台不做任何处理
}

fn user_config_path() -> PathBuf {
    get_claude_mcp_path()
}

fn read_json_value(path: &Path) -> Result<Value, AppError> {
    if !path.exists() {
        return Ok(serde_json::json!({}));
    }
    let content = fs::read_to_string(path).map_err(|e| AppError::io(path, e))?;
    let value: Value = serde_json::from_str(&content).map_err(|e| AppError::json(path, e))?;
    Ok(value)
}

fn write_json_value(path: &Path, value: &Value) -> Result<(), AppError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
    }
    let json =
        serde_json::to_string_pretty(value).map_err(|e| AppError::JsonSerialize { source: e })?;
    atomic_write(path, json.as_bytes())
}

pub fn read_mcp_json() -> Result<Option<String>, AppError> {
    let path = user_config_path();
    if !path.exists() {
        return Ok(None);
    }
    let content = fs::read_to_string(&path).map_err(|e| AppError::io(&path, e))?;
    Ok(Some(content))
}

/// 在 ~/.claude.json 根对象写入 hasCompletedOnboarding=true（用于跳过 Claude Code 初次安装确认）
/// 仅增量写入该字段，其他字段保持不变
pub fn set_has_completed_onboarding() -> Result<bool, AppError> {
    let path = user_config_path();
    let mut root = if path.exists() {
        read_json_value(&path)?
    } else {
        serde_json::json!({})
    };

    let obj = root
        .as_object_mut()
        .ok_or_else(|| AppError::Config("~/.claude.json 根必须是对象".into()))?;

    let already = obj
        .get("hasCompletedOnboarding")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if already {
        return Ok(false);
    }

    obj.insert("hasCompletedOnboarding".into(), Value::Bool(true));
    write_json_value(&path, &root)?;
    Ok(true)
}

/// 删除 ~/.claude.json 根对象的 hasCompletedOnboarding 字段（恢复 Claude Code 初次安装确认）
/// 仅增量删除该字段，其他字段保持不变
pub fn clear_has_completed_onboarding() -> Result<bool, AppError> {
    let path = user_config_path();
    if !path.exists() {
        return Ok(false);
    }

    let mut root = read_json_value(&path)?;
    let obj = root
        .as_object_mut()
        .ok_or_else(|| AppError::Config("~/.claude.json 根必须是对象".into()))?;

    let existed = obj.remove("hasCompletedOnboarding").is_some();
    if !existed {
        return Ok(false);
    }

    write_json_value(&path, &root)?;
    Ok(true)
}

/// 读取 ~/.claude.json 中的 mcpServers 映射
pub fn read_mcp_servers_map() -> Result<std::collections::HashMap<String, Value>, AppError> {
    let path = user_config_path();
    if !path.exists() {
        return Ok(std::collections::HashMap::new());
    }

    let root = read_json_value(&path)?;
    let servers = root
        .get("mcpServers")
        .and_then(|v| v.as_object())
        .map(|obj| obj.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
        .unwrap_or_default();

    Ok(servers)
}

/// 将给定的启用 MCP 服务器映射写入到用户级 ~/.claude.json 的 mcpServers 字段
/// 仅覆盖 mcpServers，其他字段保持不变
pub fn set_mcp_servers_map(
    servers: &std::collections::HashMap<String, Value>,
) -> Result<(), AppError> {
    let path = user_config_path();
    let mut root = if path.exists() {
        read_json_value(&path)?
    } else {
        serde_json::json!({})
    };

    // 构建 mcpServers 对象：移除 UI 辅助字段（enabled/source），仅保留实际 MCP 规范
    // 检测目标路径是否为 WSL，若是则跳过 cmd /c 包装
    let is_wsl_target = is_wsl_path(&path);
    if is_wsl_target {
        log::info!("检测到 WSL 路径，跳过 cmd /c 包装: {}", path.display());
    }
    let mut out: Map<String, Value> = Map::new();
    for (id, spec) in servers.iter() {
        let mut obj = if let Some(map) = spec.as_object() {
            map.clone()
        } else {
            return Err(AppError::McpValidation(format!(
                "MCP 服务器 '{id}' 不是对象"
            )));
        };

        if let Some(server_val) = obj.remove("server") {
            let server_obj = server_val.as_object().cloned().ok_or_else(|| {
                AppError::McpValidation(format!("MCP 服务器 '{id}' server 字段不是对象"))
            })?;
            obj = server_obj;
        }

        obj.remove("enabled");
        obj.remove("source");
        obj.remove("id");
        obj.remove("name");
        obj.remove("description");
        obj.remove("tags");
        obj.remove("homepage");
        obj.remove("docs");

        // Windows 平台自动包装 npx/npm 等命令为 cmd /c 格式（WSL 路径除外）
        if !is_wsl_target {
            wrap_command_for_windows(&mut obj);
        }

        out.insert(id.clone(), Value::Object(obj));
    }

    {
        let obj = root
            .as_object_mut()
            .ok_or_else(|| AppError::Config("~/.claude.json 根必须是对象".into()))?;
        obj.insert("mcpServers".into(), Value::Object(out));
    }

    write_json_value(&path, &root)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// 测试 Windows 命令包装功能
    /// 由于使用条件编译，在非 Windows 平台上测试的是空函数
    #[test]
    fn test_wrap_command_for_windows_npx() {
        let mut obj = json!({"command": "npx", "args": ["-y", "@upstash/context7-mcp"]})
            .as_object()
            .unwrap()
            .clone();
        wrap_command_for_windows(&mut obj);

        #[cfg(windows)]
        {
            assert_eq!(obj["command"], "cmd");
            assert_eq!(
                obj["args"],
                json!(["/c", "npx", "-y", "@upstash/context7-mcp"])
            );
        }

        #[cfg(not(windows))]
        {
            // 非 Windows 平台不做任何处理
            assert_eq!(obj["command"], "npx");
        }
    }

    #[test]
    fn test_wrap_command_for_windows_npm() {
        let mut obj = json!({"command": "npm", "args": ["run", "start"]})
            .as_object()
            .unwrap()
            .clone();
        wrap_command_for_windows(&mut obj);

        #[cfg(windows)]
        {
            assert_eq!(obj["command"], "cmd");
            assert_eq!(obj["args"], json!(["/c", "npm", "run", "start"]));
        }
    }

    #[test]
    fn test_wrap_command_for_windows_already_cmd() {
        // 已经是 cmd 的不应该重复包装
        let mut obj = json!({"command": "cmd", "args": ["/c", "npx", "-y", "foo"]})
            .as_object()
            .unwrap()
            .clone();
        wrap_command_for_windows(&mut obj);

        assert_eq!(obj["command"], "cmd");
        // args 应该保持不变，不会变成 ["/c", "cmd", "/c", "npx", ...]
        assert_eq!(obj["args"], json!(["/c", "npx", "-y", "foo"]));
    }

    #[test]
    fn test_wrap_command_for_windows_http_type_skipped() {
        // http 类型不应该被处理
        let mut obj = json!({"type": "http", "url": "https://example.com/mcp"})
            .as_object()
            .unwrap()
            .clone();
        wrap_command_for_windows(&mut obj);

        assert!(!obj.contains_key("command"));
        assert_eq!(obj["url"], "https://example.com/mcp");
    }

    #[test]
    fn test_wrap_command_for_windows_other_command_skipped() {
        // 非目标命令（如 python）不应该被包装
        let mut obj = json!({"command": "python", "args": ["server.py"]})
            .as_object()
            .unwrap()
            .clone();
        wrap_command_for_windows(&mut obj);

        // python 不在 WINDOWS_WRAP_COMMANDS 列表中，不应该被包装
        assert_eq!(obj["command"], "python");
        assert_eq!(obj["args"], json!(["server.py"]));
    }

    #[test]
    fn test_wrap_command_for_windows_no_args() {
        // 没有 args 的情况
        let mut obj = json!({"command": "npx"}).as_object().unwrap().clone();
        wrap_command_for_windows(&mut obj);

        #[cfg(windows)]
        {
            assert_eq!(obj["command"], "cmd");
            assert_eq!(obj["args"], json!(["/c", "npx"]));
        }
    }

    #[test]
    fn test_wrap_command_for_windows_with_cmd_suffix() {
        // 处理 npx.cmd 格式
        let mut obj = json!({"command": "npx.cmd", "args": ["-y", "foo"]})
            .as_object()
            .unwrap()
            .clone();
        wrap_command_for_windows(&mut obj);

        #[cfg(windows)]
        {
            assert_eq!(obj["command"], "cmd");
            assert_eq!(obj["args"], json!(["/c", "npx.cmd", "-y", "foo"]));
        }
    }

    #[test]
    fn test_wrap_command_for_windows_case_insensitive() {
        // 大小写不敏感
        let mut obj = json!({"command": "NPX", "args": ["-y", "foo"]})
            .as_object()
            .unwrap()
            .clone();
        wrap_command_for_windows(&mut obj);

        #[cfg(windows)]
        {
            assert_eq!(obj["command"], "cmd");
            assert_eq!(obj["args"], json!(["/c", "NPX", "-y", "foo"]));
        }
    }

    /// 测试 WSL 路径检测功能
    #[test]
    fn test_is_wsl_path_wsl_dollar() {
        // wsl$ 格式 - 各种发行版
        #[cfg(windows)]
        {
            assert!(is_wsl_path(Path::new(r"\\wsl$\Ubuntu\home\user\.claude")));
            assert!(is_wsl_path(Path::new(r"\\wsl$\Debian\home\user\.claude")));
            assert!(is_wsl_path(Path::new(
                r"\\wsl$\openSUSE-Leap-15.2\home\user"
            )));
            assert!(is_wsl_path(Path::new(r"\\wsl$\kali-linux\home\user")));
            assert!(is_wsl_path(Path::new(r"\\wsl$\Arch\home\user")));
            assert!(is_wsl_path(Path::new(r"\\wsl$\Alpine\home\user")));
            assert!(is_wsl_path(Path::new(r"\\wsl$\Fedora\home\user")));
        }

        #[cfg(not(windows))]
        {
            // 非 Windows 平台始终返回 false
            assert!(!is_wsl_path(Path::new(r"\\wsl$\Ubuntu\home\user\.claude")));
        }
    }

    #[test]
    fn test_is_wsl_path_wsl_localhost() {
        // wsl.localhost 格式
        #[cfg(windows)]
        {
            assert!(is_wsl_path(Path::new(
                r"\\wsl.localhost\Ubuntu\home\user\.claude"
            )));
            assert!(is_wsl_path(Path::new(r"\\wsl.localhost\Debian\home\user")));
            assert!(is_wsl_path(Path::new(
                r"\\wsl.localhost\openSUSE-Leap-15.2\home\user"
            )));
        }
    }

    #[test]
    fn test_is_wsl_path_case_insensitive() {
        // 大小写不敏感
        #[cfg(windows)]
        {
            assert!(is_wsl_path(Path::new(r"\\WSL$\Ubuntu\home\user")));
            assert!(is_wsl_path(Path::new(r"\\Wsl$\Ubuntu\home\user")));
            assert!(is_wsl_path(Path::new(r"\\WSL.LOCALHOST\Ubuntu\home\user")));
            assert!(is_wsl_path(Path::new(r"\\Wsl.Localhost\Ubuntu\home\user")));
        }
    }

    #[test]
    fn test_is_wsl_path_non_wsl() {
        // 非 WSL 路径
        assert!(!is_wsl_path(Path::new(r"C:\Users\user\.claude")));
        assert!(!is_wsl_path(Path::new(r"D:\Workspace\project")));
        #[cfg(windows)]
        {
            assert!(!is_wsl_path(Path::new(r"\\server\share\path")));
            assert!(!is_wsl_path(Path::new(r"\\localhost\c$\Users")));
            assert!(!is_wsl_path(Path::new(r"\\192.168.1.1\share")));
        }
    }
}
