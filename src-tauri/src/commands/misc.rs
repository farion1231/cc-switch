#![allow(non_snake_case)]

use crate::init_status::InitErrorPayload;
use tauri::AppHandle;
use tauri_plugin_opener::OpenerExt;

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
    use std::process::Command;

    let tools = vec!["claude", "codex", "gemini"];
    let mut results = Vec::new();

    // 用于获取远程版本的 client
    let client = reqwest::Client::builder()
        .user_agent("cc-switch/1.0")
        .build()
        .map_err(|e| e.to_string())?;

    for tool in tools {
        // 1. 获取本地版本 (保持不变)
        let (local_version, local_error) = {
            let output = if cfg!(target_os = "windows") {
                Command::new("cmd")
                    .args(["/C", &format!("{tool} --version")])
                    .output()
            } else {
                Command::new("sh")
                    .arg("-c")
                    .arg(format!("{tool} --version"))
                    .output()
            };

            match output {
                Ok(out) => {
                    if out.status.success() {
                        (
                            Some(String::from_utf8_lossy(&out.stdout).trim().to_string()),
                            None,
                        )
                    } else {
                        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
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
