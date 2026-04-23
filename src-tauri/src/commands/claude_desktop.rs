#![allow(non_snake_case)]

use std::path::PathBuf;
use std::str::FromStr;

use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;

use crate::claude_desktop::{self, ClaudeDesktopExportFormat};
use crate::store::AppState;

#[cfg(target_os = "macos")]
const MACOS_PROFILE_SETTINGS_URLS: [&str; 2] = [
    "x-apple.systempreferences:com.apple.Profiles-Settings.extension",
    "x-apple.systempreferences:com.apple.systempreferences.GeneralSettings",
];

#[cfg(target_os = "macos")]
fn open_macos_profile_settings(app: &AppHandle) -> Result<bool, String> {
    let opener = app.opener();
    let mut last_error = None;

    for url in MACOS_PROFILE_SETTINGS_URLS {
        match opener.open_url(url, None::<String>) {
            Ok(_) => return Ok(true),
            Err(err) => last_error = Some(err),
        }
    }

    Err(format!(
        "打开 macOS 配置描述文件设置失败: {}",
        last_error
            .map(|err| err.to_string())
            .unwrap_or_else(|| "未知错误".to_string())
    ))
}

#[tauri::command]
pub async fn get_claude_desktop_preview(
    state: State<'_, AppState>,
) -> Result<crate::claude_desktop::ClaudeDesktopPreview, String> {
    claude_desktop::build_preview(state.inner())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_claude_desktop_mode_status(
    state: State<'_, AppState>,
) -> Result<crate::claude_desktop::ClaudeDesktopModeStatus, String> {
    claude_desktop::detect_mode_status(state.inner())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn export_claude_desktop_config(
    state: State<'_, AppState>,
    format: String,
    #[allow(non_snake_case)] filePath: String,
) -> Result<bool, String> {
    let export_format = ClaudeDesktopExportFormat::from_str(&format).map_err(|e| e.to_string())?;
    let path = PathBuf::from(filePath);
    claude_desktop::export_to_path(state.inner(), export_format, &path)
        .await
        .map_err(|e| e.to_string())?;
    Ok(true)
}

#[tauri::command]
pub async fn install_claude_desktop_mobileconfig(
    app: AppHandle,
    #[allow(non_snake_case)] filePath: String,
) -> Result<bool, String> {
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (app, filePath);
        return Err("当前平台不支持自动安装 .mobileconfig".to_string());
    }

    #[cfg(target_os = "macos")]
    {
        let path = PathBuf::from(&filePath);
        if !path.exists() {
            return Err(format!(
                "找不到要安装的 mobileconfig 文件: {}",
                path.display()
            ));
        }
        if path.extension().and_then(|ext| ext.to_str()) != Some("mobileconfig") {
            return Err("只能安装 .mobileconfig 文件".to_string());
        }

        app.opener()
            .open_path(path.to_string_lossy().to_string(), None::<String>)
            .map_err(|e| format!("打开 .mobileconfig 安装器失败: {e}"))?;
        let _ = open_macos_profile_settings(&app);
        Ok(true)
    }
}

#[tauri::command]
pub async fn open_claude_desktop_install_settings(app: AppHandle) -> Result<bool, String> {
    #[cfg(not(target_os = "macos"))]
    {
        let _ = app;
        return Err("当前平台不支持直接打开 macOS 配置描述文件设置".to_string());
    }

    #[cfg(target_os = "macos")]
    {
        open_macos_profile_settings(&app)
    }
}

#[tauri::command]
pub async fn save_claude_desktop_export_dialog<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    format: String,
) -> Result<Option<String>, String> {
    let export_format = ClaudeDesktopExportFormat::from_str(&format).map_err(|e| e.to_string())?;
    let dialog = match export_format {
        ClaudeDesktopExportFormat::Json => app
            .dialog()
            .file()
            .add_filter("JSON", &["json"])
            .set_file_name(export_format.default_filename()),
        ClaudeDesktopExportFormat::Mobileconfig => app
            .dialog()
            .file()
            .add_filter("Configuration Profile", &["mobileconfig"])
            .set_file_name(export_format.default_filename()),
        ClaudeDesktopExportFormat::Reg => app
            .dialog()
            .file()
            .add_filter("Registry", &["reg"])
            .set_file_name(export_format.default_filename()),
    };

    Ok(dialog.blocking_save_file().map(|path| path.to_string()))
}
