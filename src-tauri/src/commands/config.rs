#![allow(non_snake_case)]

use tauri::AppHandle;
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;

use crate::app_config::AppType;
use crate::codex_config;
use crate::config::{self, get_claude_settings_path, ConfigStatus};
use crate::settings;

/// 获取 Claude Code 配置状态
#[tauri::command]
pub async fn get_claude_config_status() -> Result<ConfigStatus, String> {
    Ok(config::get_claude_config_status())
}

use std::str::FromStr;

fn invalid_json_format_error(error: serde_json::Error) -> String {
    let lang = settings::get_settings()
        .language
        .unwrap_or_else(|| "zh".to_string());

    match lang.as_str() {
        "en" => format!("Invalid JSON format: {error}"),
        "ja" => format!("JSON形式が無効です: {error}"),
        _ => format!("无效的 JSON 格式: {error}"),
    }
}

#[tauri::command]
pub async fn get_config_status(app: String) -> Result<ConfigStatus, String> {
    match AppType::from_str(&app).map_err(|e| e.to_string())? {
        AppType::Claude => Ok(config::get_claude_config_status()),
        AppType::Codex => {
            let auth_path = codex_config::get_codex_auth_path();
            let exists = auth_path.exists();
            let path = codex_config::get_codex_config_dir()
                .to_string_lossy()
                .to_string();

            Ok(ConfigStatus { exists, path })
        }
        AppType::Gemini => {
            let env_path = crate::gemini_config::get_gemini_env_path();
            let exists = env_path.exists();
            let path = crate::gemini_config::get_gemini_dir()
                .to_string_lossy()
                .to_string();

            Ok(ConfigStatus { exists, path })
        }
    }
}

/// 获取 Claude Code 配置文件路径
#[tauri::command]
pub async fn get_claude_code_config_path() -> Result<String, String> {
    Ok(get_claude_settings_path().to_string_lossy().to_string())
}

/// 获取当前生效的配置目录
#[tauri::command]
pub async fn get_config_dir(app: String) -> Result<String, String> {
    let dir = match AppType::from_str(&app).map_err(|e| e.to_string())? {
        AppType::Claude => config::get_claude_config_dir(),
        AppType::Codex => codex_config::get_codex_config_dir(),
        AppType::Gemini => crate::gemini_config::get_gemini_dir(),
    };

    Ok(dir.to_string_lossy().to_string())
}

/// 打开配置文件夹
#[tauri::command]
pub async fn open_config_folder(handle: AppHandle, app: String) -> Result<bool, String> {
    let config_dir = match AppType::from_str(&app).map_err(|e| e.to_string())? {
        AppType::Claude => config::get_claude_config_dir(),
        AppType::Codex => codex_config::get_codex_config_dir(),
        AppType::Gemini => crate::gemini_config::get_gemini_dir(),
    };

    if !config_dir.exists() {
        std::fs::create_dir_all(&config_dir).map_err(|e| format!("创建目录失败: {e}"))?;
    }

    handle
        .opener()
        .open_path(config_dir.to_string_lossy().to_string(), None::<String>)
        .map_err(|e| format!("打开文件夹失败: {e}"))?;

    Ok(true)
}

/// 弹出系统目录选择器并返回用户选择的路径
#[tauri::command]
pub async fn pick_directory(
    app: AppHandle,
    #[allow(non_snake_case)] defaultPath: Option<String>,
) -> Result<Option<String>, String> {
    let initial = defaultPath
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty());

    let result = tauri::async_runtime::spawn_blocking(move || {
        let mut builder = app.dialog().file();
        if let Some(path) = initial {
            builder = builder.set_directory(path);
        }
        builder.blocking_pick_folder()
    })
    .await
    .map_err(|e| format!("弹出目录选择器失败: {e}"))?;

    match result {
        Some(file_path) => {
            let resolved = file_path
                .simplified()
                .into_path()
                .map_err(|e| format!("解析选择的目录失败: {e}"))?;
            Ok(Some(resolved.to_string_lossy().to_string()))
        }
        None => Ok(None),
    }
}

/// 获取应用配置文件路径
#[tauri::command]
pub async fn get_app_config_path() -> Result<String, String> {
    let config_path = config::get_app_config_path();
    Ok(config_path.to_string_lossy().to_string())
}

/// 打开应用配置文件夹
#[tauri::command]
pub async fn open_app_config_folder(handle: AppHandle) -> Result<bool, String> {
    let config_dir = config::get_app_config_dir();

    if !config_dir.exists() {
        std::fs::create_dir_all(&config_dir).map_err(|e| format!("创建目录失败: {e}"))?;
    }

    handle
        .opener()
        .open_path(config_dir.to_string_lossy().to_string(), None::<String>)
        .map_err(|e| format!("打开文件夹失败: {e}"))?;

    Ok(true)
}

/// 获取 Claude 通用配置片段（已废弃，使用 get_common_config_snippet）
#[tauri::command]
pub async fn get_claude_common_config_snippet(
    state: tauri::State<'_, crate::store::AppState>,
) -> Result<Option<String>, String> {
    state
        .db
        .get_config_snippet("claude")
        .map_err(|e| e.to_string())
}

/// 设置 Claude 通用配置片段（已废弃，使用 set_common_config_snippet）
#[tauri::command]
pub async fn set_claude_common_config_snippet(
    snippet: String,
    state: tauri::State<'_, crate::store::AppState>,
) -> Result<(), String> {
    // 验证是否为有效的 JSON（如果不为空）
    if !snippet.trim().is_empty() {
        serde_json::from_str::<serde_json::Value>(&snippet).map_err(invalid_json_format_error)?;
    }

    let value = if snippet.trim().is_empty() {
        None
    } else {
        Some(snippet)
    };

    state
        .db
        .set_config_snippet("claude", value)
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// 获取通用配置片段（统一接口）
#[tauri::command]
pub async fn get_common_config_snippet(
    app_type: String,
    state: tauri::State<'_, crate::store::AppState>,
) -> Result<Option<String>, String> {
    state
        .db
        .get_config_snippet(&app_type)
        .map_err(|e| e.to_string())
}

/// 设置通用配置片段（统一接口）
#[tauri::command]
pub async fn set_common_config_snippet(
    app_type: String,
    snippet: String,
    state: tauri::State<'_, crate::store::AppState>,
) -> Result<(), String> {
    // 验证格式（根据应用类型）
    if !snippet.trim().is_empty() {
        match app_type.as_str() {
            "claude" | "gemini" => {
                // 验证 JSON 格式
                serde_json::from_str::<serde_json::Value>(&snippet)
                    .map_err(invalid_json_format_error)?;
            }
            "codex" => {
                // TOML 格式暂不验证（或可使用 toml crate）
                // 注意：TOML 验证较为复杂，暂时跳过
            }
            _ => {}
        }
    }

    let value = if snippet.trim().is_empty() {
        None
    } else {
        Some(snippet)
    };

    state
        .db
        .set_config_snippet(&app_type, value)
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// 提取通用配置片段
///
/// 优先从 `settingsConfig`（编辑器当前内容）提取；若未提供，则从当前激活供应商提取。
///
/// 提取时会自动排除差异化字段（API Key、模型配置、端点等），返回可复用的通用配置片段。
#[tauri::command]
pub async fn extract_common_config_snippet(
    appType: String,
    settingsConfig: Option<String>,
    state: tauri::State<'_, crate::store::AppState>,
) -> Result<String, String> {
    let app = AppType::from_str(&appType).map_err(|e| e.to_string())?;

    if let Some(settings_config) = settingsConfig.filter(|s| !s.trim().is_empty()) {
        let settings: serde_json::Value =
            serde_json::from_str(&settings_config).map_err(invalid_json_format_error)?;

        return crate::services::provider::ProviderService::extract_common_config_snippet_from_settings(
            app,
            &settings,
        )
        .map_err(|e| e.to_string());
    }

    crate::services::provider::ProviderService::extract_common_config_snippet(&state, app)
        .map_err(|e| e.to_string())
}
