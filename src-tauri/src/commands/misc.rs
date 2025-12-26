#![allow(non_snake_case)]

use crate::app_config::AppType;
use crate::init_status::InitErrorPayload;
use crate::services::ProviderService;
use tauri::AppHandle;
use tauri::State;
use tauri_plugin_opener::OpenerExt;
use std::str::FromStr;

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;

/// 打开外部链接
#[tauri::command]
pub async fn open_external(app: AppHandle, url: String) -> Result<bool, String> {
    let url = if url.starts_with("http://") || url.starts_with("https://") {
        url
    } else {
        format!("https://{url}")
    };

    app.opener()
        .open_url(&url, None::<String>)
        .map_err(|e| format!("打开链接失败: {e}"))?;

    Ok(true)
}

/// 检查更新
#[tauri::command]
pub async fn check_for_updates(handle: AppHandle) -> Result<bool, String> {
    handle
        .opener()
        .open_url(
            "https://github.com/farion1231/cc-switch/releases/latest",
            None::<String>,
        )
        .map_err(|e| format!("打开更新页面失败: {e}"))?;

    Ok(true)
}

/// 判断是否为便携版（绿色版）运行
#[tauri::command]
pub async fn is_portable_mode() -> Result<bool, String> {
    let exe_path = std::env::current_exe().map_err(|e| format!("获取可执行路径失败: {e}"))?;
    if let Some(dir) = exe_path.parent() {
        Ok(dir.join("portable.ini").is_file())
    } else {
        Ok(false)
    }
}

/// 获取应用启动阶段的初始化错误（若有）。
/// 用于前端在早期主动拉取，避免事件订阅竞态导致的提示缺失。
#[tauri::command]
pub async fn get_init_error() -> Result<Option<InitErrorPayload>, String> {
    Ok(crate::init_status::get_init_error())
}

/// 获取 JSON→SQLite 迁移结果（若有）。
/// 只返回一次 true，之后返回 false，用于前端显示一次性 Toast 通知。
#[tauri::command]
pub async fn get_migration_result() -> Result<bool, String> {
    Ok(crate::init_status::take_migration_success())
}

#[derive(serde::Serialize)]
pub struct ToolVersion {
    name: String,
    version: Option<String>,
    latest_version: Option<String>, // 新增字段：最新版本
    error: Option<String>,
}

#[tauri::command]
pub async fn get_tool_versions() -> Result<Vec<ToolVersion>, String> {
    let tools = vec!["claude", "codex", "gemini"];
    let mut results = Vec::new();

    // 用于获取远程版本的 client
    let client = reqwest::Client::builder()
        .user_agent("cc-switch/1.0")
        .build()
        .map_err(|e| e.to_string())?;

    for tool in tools {
        // 1. 获取本地版本 - 先尝试直接执行，失败则扫描常见路径
        let (local_version, local_error) = {
            // 先尝试直接执行
            let direct_result = try_get_version(tool);

            if direct_result.0.is_some() {
                direct_result
            } else {
                // 扫描常见的 npm 全局安装路径
                scan_cli_version(tool)
            }
        };

        // 2. 获取远程最新版本
        let latest_version = match tool {
            "claude" => fetch_npm_latest_version(&client, "@anthropic-ai/claude-code").await,
            "codex" => fetch_npm_latest_version(&client, "@openai/codex").await,
            "gemini" => fetch_npm_latest_version(&client, "@google/gemini-cli").await,
            _ => None,
        };

        results.push(ToolVersion {
            name: tool.to_string(),
            version: local_version,
            latest_version,
            error: local_error,
        });
    }

    Ok(results)
}

/// Helper function to fetch latest version from npm registry
async fn fetch_npm_latest_version(client: &reqwest::Client, package: &str) -> Option<String> {
    let url = format!("https://registry.npmjs.org/{package}");
    match client.get(&url).send().await {
        Ok(resp) => {
            if let Ok(json) = resp.json::<serde_json::Value>().await {
                json.get("dist-tags")
                    .and_then(|tags| tags.get("latest"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
            } else {
                None
            }
        }
        Err(_) => None,
    }
}

/// 从版本输出中提取纯版本号
fn extract_version(raw: &str) -> String {
    // 匹配 semver 格式: x.y.z 或 x.y.z-xxx
    let re = regex::Regex::new(r"\d+\.\d+\.\d+(-[\w.]+)?").unwrap();
    re.find(raw)
        .map(|m| m.as_str().to_string())
        .unwrap_or_else(|| raw.to_string())
}

/// 尝试直接执行命令获取版本
fn try_get_version(tool: &str) -> (Option<String>, Option<String>) {
    use std::process::Command;

    #[cfg(target_os = "windows")]
    let output = {
        Command::new("cmd")
            .args(["/C", &format!("{tool} --version")])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
    };

    #[cfg(not(target_os = "windows"))]
    let output = {
        Command::new("sh")
            .arg("-c")
            .arg(format!("{tool} --version"))
            .output()
    };

    match output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
            let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
            if out.status.success() {
                let raw = if stdout.is_empty() { &stderr } else { &stdout };
                if raw.is_empty() {
                    (None, Some("未安装或无法执行".to_string()))
                } else {
                    (Some(extract_version(raw)), None)
                }
            } else {
                let err = if stderr.is_empty() { stdout } else { stderr };
                (
                    None,
                    Some(if err.is_empty() {
                        "未安装或无法执行".to_string()
                    } else {
                        err
                    }),
                )
            }
        }
        Err(e) => (None, Some(e.to_string())),
    }
}

/// 扫描常见路径查找 CLI
fn scan_cli_version(tool: &str) -> (Option<String>, Option<String>) {
    use std::process::Command;

    let home = dirs::home_dir().unwrap_or_default();

    // 常见的 npm 全局安装路径
    let mut search_paths: Vec<std::path::PathBuf> = vec![
        home.join(".npm-global/bin"),
        home.join(".local/bin"),
        home.join("n/bin"), // n version manager
    ];

    #[cfg(target_os = "macos")]
    {
        search_paths.push(std::path::PathBuf::from("/opt/homebrew/bin"));
        search_paths.push(std::path::PathBuf::from("/usr/local/bin"));
    }

    #[cfg(target_os = "linux")]
    {
        search_paths.push(std::path::PathBuf::from("/usr/local/bin"));
        search_paths.push(std::path::PathBuf::from("/usr/bin"));
    }

    #[cfg(target_os = "windows")]
    {
        if let Some(appdata) = dirs::data_dir() {
            search_paths.push(appdata.join("npm"));
        }
        search_paths.push(std::path::PathBuf::from("C:\\Program Files\\nodejs"));
    }

    // 扫描 nvm 目录下的所有 node 版本
    let nvm_base = home.join(".nvm/versions/node");
    if nvm_base.exists() {
        if let Ok(entries) = std::fs::read_dir(&nvm_base) {
            for entry in entries.flatten() {
                let bin_path = entry.path().join("bin");
                if bin_path.exists() {
                    search_paths.push(bin_path);
                }
            }
        }
    }

    // 在每个路径中查找工具
    for path in &search_paths {
        let tool_path = if cfg!(target_os = "windows") {
            path.join(format!("{tool}.cmd"))
        } else {
            path.join(tool)
        };

        if tool_path.exists() {
            // 构建 PATH 环境变量，确保 node 可被找到
            let current_path = std::env::var("PATH").unwrap_or_default();
            let new_path = format!("{}:{}", path.display(), current_path);

            #[cfg(target_os = "windows")]
            let output = {
                Command::new(&tool_path)
                    .arg("--version")
                    .env("PATH", &new_path)
                    .creation_flags(CREATE_NO_WINDOW)
                    .output()
            };

            #[cfg(not(target_os = "windows"))]
            let output = {
                Command::new(&tool_path)
                    .arg("--version")
                    .env("PATH", &new_path)
                    .output()
            };

            if let Ok(out) = output {
                let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
                let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
                if out.status.success() {
                    let raw = if stdout.is_empty() { &stderr } else { &stdout };
                    if !raw.is_empty() {
                        return (Some(extract_version(raw)), None);
                    }
                }
            }
        }
    }

    (None, Some("未安装或无法执行".to_string()))
}

/// 打开指定提供商的终端
///
/// 根据提供商配置的环境变量启动一个带有该提供商特定设置的终端
/// 无需检查是否为当前激活的提供商，任何提供商都可以打开终端
#[allow(non_snake_case)]
#[tauri::command]
pub async fn open_provider_terminal(
    state: State<'_, crate::store::AppState>,
    app: String,
    #[allow(non_snake_case)] providerId: String,
) -> Result<bool, String> {
    let app_type = AppType::from_str(&app).map_err(|e| e.to_string())?;

    // 获取提供商配置
    let providers = ProviderService::list(state.inner(), app_type.clone())
        .map_err(|e| format!("获取提供商列表失败: {e}"))?;

    let provider = providers.get(&providerId)
        .ok_or_else(|| format!("提供商 {providerId} 不存在"))?;

    // 从提供商配置中提取环境变量
    let config = &provider.settings_config;
    let env_vars = extract_env_vars_from_config(config, &app_type);

    // 根据平台启动终端，传入提供商ID用于生成唯一的配置文件名
    launch_terminal_with_env(env_vars, &providerId).map_err(|e| format!("启动终端失败: {e}"))?;

    Ok(true)
}

/// 从提供商配置中提取环境变量
fn extract_env_vars_from_config(
    config: &serde_json::Value,
    app_type: &AppType,
) -> Vec<(String, String)> {
    let mut env_vars = Vec::new();

    if let Some(obj) = config.as_object() {
        // Claude 使用 env 字段
        if let Some(env) = obj.get("env").and_then(|v| v.as_object()) {
            for (key, value) in env {
                if let Some(str_val) = value.as_str() {
                    env_vars.push((key.clone(), str_val.to_string()));
                }
            }
        }

        // Codex 使用 auth 字段
        if let Some(auth) = obj.get("auth").and_then(|v| v.as_str()) {
            match app_type {
                AppType::Codex => {
                    env_vars.push(("OPENAI_API_KEY".to_string(), auth.to_string()));
                }
                _ => {}
            }
        }

        // Gemini 使用 API_KEY
        if let Some(api_key) = obj.get("api_key").and_then(|v| v.as_str()) {
            match app_type {
                AppType::Gemini => {
                    env_vars.push(("GOOGLE_API_KEY".to_string(), api_key.to_string()));
                }
                _ => {}
            }
        }

        // 提取 base_url（如果存在）
        if let Some(env) = obj.get("env").and_then(|v| v.as_object()) {
            if let Some(base_url) = env.get("ANTHROPIC_BASE_URL").or_else(|| env.get("GOOGLE_GEMINI_BASE_URL")) {
                if let Some(url_str) = base_url.as_str() {
                    match app_type {
                        AppType::Claude => {
                            env_vars.push(("ANTHROPIC_BASE_URL".to_string(), url_str.to_string()));
                        }
                        AppType::Gemini => {
                            env_vars.push(("GOOGLE_GEMINI_BASE_URL".to_string(), url_str.to_string()));
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    env_vars
}

/// 创建临时配置文件并启动 claude 终端
/// 使用 --settings 参数传入提供商特定的 API 配置
fn launch_terminal_with_env(env_vars: Vec<(String, String)>, provider_id: &str) -> Result<(), String> {
    use std::process::Command;

    // 创建临时配置文件，使用提供商ID和进程ID确保唯一性
    let temp_dir = std::env::temp_dir();
    let config_file = temp_dir.join(format!("claude_{}_{}.json", provider_id, std::process::id()));

    // 构建 claude 配置 JSON 格式
    let mut config_obj = serde_json::Map::new();
    let mut env_obj = serde_json::Map::new();

    for (key, value) in &env_vars {
        env_obj.insert(key.clone(), serde_json::Value::String(value.clone()));
    }

    config_obj.insert("env".to_string(), serde_json::Value::Object(env_obj));

    let config_json = serde_json::to_string_pretty(&config_obj)
        .map_err(|e| format!("序列化配置失败: {e}"))?;

    // 写入临时配置文件
    std::fs::write(&config_file, config_json)
        .map_err(|e| format!("写入配置文件失败: {e}"))?;

    // 转义配置文件路径用于 shell
    let config_path_escaped = config_file.to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('$', "\\$")
        .replace(' ', "\\ ");

    #[cfg(target_os = "macos")]
    {
        // macOS: 使用 Terminal.app 启动 claude
        let mut terminal_cmd = Command::new("osascript");
        terminal_cmd.arg("-e");

        let config_file_for_cleanup = config_file.clone();
        let script = format!(
            r#"tell application "Terminal"
                activate
                do script "echo 'Using provider-specific claude config:' && echo '{}' && claude --settings '{}'; exit"
            end tell"#,
            config_path_escaped, config_path_escaped
        );

        terminal_cmd.arg(&script);

        terminal_cmd
            .spawn()
            .map_err(|e| format!("启动 macOS 终端失败: {e}"))?;

        return Ok(());
    }

    #[cfg(target_os = "linux")]
    {
        // Linux: 尝试使用常见终端
        let terminals = [
            "gnome-terminal", "konsole", "xfce4-terminal",
            "mate-terminal", "lxterminal", "alacritty", "kitty",
        ];

        let mut last_error = String::from("未找到可用的终端");

        for terminal in terminals {
            // 检查终端是否存在
            if Command::new("which").arg(terminal).output().is_err() {
                continue;
            }

            let result = Command::new(terminal)
                .arg("--")
                .arg("sh")
                .arg("-c")
                .arg(&format!(
                    "echo 'Using provider-specific claude config:' && echo '{}' && claude --settings '{}'; $SHELL",
                    config_path_escaped, config_path_escaped
                ))
                .spawn();

            match result {
                Ok(_) => {
                    return Ok(());
                }
                Err(e) => {
                    last_error = format!("启动 {} 失败: {}", terminal, e);
                    continue;
                }
            }
        }

        // 如果所有终端都失败，清理配置文件
        let _ = std::fs::remove_file(&config_file);
        return Err(last_error);
    }

    #[cfg(target_os = "windows")]
    {
        use std::io::Write;

        // Windows: 创建临时批处理文件
        let bat_file = temp_dir.join(format!("cc_switch_claude_{}.bat", std::process::id()));

        let mut content = String::from("@echo off\n");
        content.push_str(&format!("echo Using provider-specific claude config:\n"));
        content.push_str(&format!("echo {}\n", config_file.to_string_lossy().to_string().replace('&', "^&")));
        content.push_str(&format!("claude --settings \"{}\"\n", config_file.to_string_lossy().to_string().replace('&', "^&")));
        content.push_str("if errorlevel 1 (\n");
        content.push_str("    echo.\n");
        content.push_str("    echo Press any key to close...\n");
        content.push_str("    pause >nul\n");
        content.push_str(")\n");

        std::fs::write(&bat_file, content)
            .map_err(|e| format!("写入批处理文件失败: {e}"))?;

        // 启动新的 cmd 窗口执行批处理文件
        Command::new("cmd")
            .args(["/C", "start", "cmd", "/C", &bat_file.to_string_lossy().to_string()])
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map_err(|e| format!("启动 Windows 终端失败: {e}"))?;

        return Ok(());
    }

    // 这个代码在所有支持的平台上都不可达，因为前面的平台特定块都已经返回了
    // 使用 cfg 和 allow 来避免编译器警告和错误
    #[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
    #[allow(unreachable_code)]
    {
        Ok(())
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    Err("不支持的操作系统".to_string())
}
