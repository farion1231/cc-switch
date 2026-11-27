use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;

use crate::app_config::AppType;
use crate::codex_config;
use crate::config::{self, ConfigStatus};
use crate::store::AppState;

use std::str::FromStr;

#[tauri::command]
pub async fn get_config_status(
    state: State<'_, AppState>,
    app: String,
) -> Result<ConfigStatus, String> {
    match AppType::from_str(&app)? {
        AppType::Claude => Ok(config::get_claude_config_status()),
        AppType::ClaudeDesktop => {
            let status = crate::claude_desktop_config::get_status(
                state.db.as_ref(),
                state.proxy_service.is_running().await,
            )?;
            Ok(ConfigStatus {
                exists: status.configured,
                path: status.config_library_path.unwrap_or_default(),
            })
        }
        AppType::Codex => {
            let auth_path = codex_config::get_codex_auth_path();
            let config_text = codex_config::read_codex_config_text().unwrap_or_default();
            let exists = auth_path.exists() || !config_text.trim().is_empty();
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
        AppType::GrokBuild => {
            let config_path = crate::grok_config::get_grok_config_path();
            let exists = config_path.exists();
            let path = crate::grok_config::get_grok_config_dir()
                .to_string_lossy()
                .to_string();

            Ok(ConfigStatus { exists, path })
        }
        AppType::OpenCode => {
            let config_path = crate::opencode_config::get_opencode_config_path()?;
            let exists = config_path.try_exists().map_err(|e| e.to_string())?;
            let path = crate::opencode_config::get_opencode_dir()
                .to_string_lossy()
                .to_string();

            Ok(ConfigStatus { exists, path })
        }
        AppType::OpenClaw => {
            let config_path = crate::openclaw_config::get_openclaw_config_path();
            let exists = config_path.exists();
            let path = crate::openclaw_config::get_openclaw_dir()
                .to_string_lossy()
                .to_string();

            Ok(ConfigStatus { exists, path })
        }
        AppType::Hermes => {
            let config_path = crate::hermes_config::get_hermes_config_path();
            let exists = config_path.exists();
            let path = crate::hermes_config::get_hermes_dir()
                .to_string_lossy()
                .to_string();

            Ok(ConfigStatus { exists, path })
        }
        AppType::Dsh => {
            let file = crate::dsh_config::config_path();
            Ok(ConfigStatus {
                exists: file.exists(),
                path: file.to_string_lossy().to_string(),
            })
        }
        AppType::Kimi => {
            let file = crate::kimi_config::config_path();
            Ok(ConfigStatus {
                exists: file.exists(),
                path: file.to_string_lossy().to_string(),
            })
        }
        AppType::Mcode => {
            let file = crate::mcode_config::config_path();
            Ok(ConfigStatus {
                exists: file.exists(),
                path: file.parent().unwrap().to_string_lossy().into_owned(),
            })
        }
        AppType::Pi => {
            let config_path = crate::pi_config::get_pi_models_path()?;
            let path = crate::pi_config::get_pi_agent_dir()?
                .to_string_lossy()
                .to_string();
            Ok(ConfigStatus {
                exists: config_path.exists(),
                path,
            })
        }
        AppType::Agy => {
            let dir = crate::agy_config::agy_dir();
            Ok(ConfigStatus {
                exists: crate::agy_config::settings_path().exists(),
                path: dir.to_string_lossy().into_owned(),
            })
        }
        AppType::Omp => {
            let config_path = crate::omp_config::get_omp_models_path()?;
            let path = crate::omp_config::get_omp_agent_dir()?
                .to_string_lossy()
                .to_string();
            Ok(ConfigStatus {
                exists: config_path.exists(),
                path,
            })
        }
        AppType::Copilot => {
            let dir = crate::copilot_config::copilot_dir();
            Ok(ConfigStatus {
                exists: dir.exists(),
                path: dir.to_string_lossy().into_owned(),
            })
        }
        AppType::WorkBuddy => Ok(ConfigStatus {
            exists: crate::workbuddy_config::config_path().exists(),
            path: crate::workbuddy_config::data_dir()
                .to_string_lossy()
                .into_owned(),
        }),
        AppType::Codebuddy => Ok(ConfigStatus {
            exists: crate::codebuddy_config::config_path().exists()
                || crate::codebuddy_config::settings_path().exists(),
            path: crate::codebuddy_config::data_dir()
                .to_string_lossy()
                .into_owned(),
        }),
        AppType::ZCode => {
            let provider_config = crate::zcode_config::get_zcode_provider_config_path();
            Ok(ConfigStatus {
                exists: provider_config.exists(),
                path: crate::zcode_config::get_zcode_dir()
                    .to_string_lossy()
                    .into_owned(),
            })
        }
    }
}

#[tauri::command]
pub async fn get_config_dir(app: String) -> Result<String, String> {
    Ok(app_config_dir(&app)?.to_string_lossy().to_string())
}

/// 每个应用的配置目录：设置页显示的路径和「打开文件夹」打开的是同一个。
fn app_config_dir(app: &str) -> Result<std::path::PathBuf, String> {
    let dir = match AppType::from_str(app)? {
        AppType::Claude => config::get_claude_config_dir(),
        AppType::ClaudeDesktop => crate::claude_desktop_config::get_config_library_path()?,
        AppType::Codex => codex_config::get_codex_config_dir(),
        AppType::Gemini => crate::gemini_config::get_gemini_dir(),
        AppType::GrokBuild => crate::grok_config::get_grok_config_dir(),
        AppType::OpenCode => crate::opencode_config::get_opencode_dir(),
        AppType::OpenClaw => crate::openclaw_config::get_openclaw_dir(),
        AppType::Hermes => crate::hermes_config::get_hermes_dir(),
        AppType::Pi => crate::pi_config::get_pi_agent_dir()?,
        AppType::Dsh => crate::dsh_config::data_dir(),
        AppType::Kimi => crate::kimi_config::data_dir(),
        AppType::Omp => crate::omp_config::get_omp_agent_dir()?,
        AppType::Mcode => crate::mcode_config::config_path()
            .parent()
            .unwrap()
            .to_path_buf(),
        AppType::Agy => crate::agy_config::agy_dir(),
        AppType::Copilot => crate::copilot_config::copilot_dir(),
        AppType::WorkBuddy => crate::workbuddy_config::data_dir(),
        AppType::Codebuddy => crate::codebuddy_config::data_dir(),
        AppType::ZCode => crate::zcode_config::get_zcode_dir(),
    };

    Ok(dir)
}

#[tauri::command]
pub async fn open_config_folder(handle: AppHandle, app: String) -> Result<bool, String> {
    let config_dir = app_config_dir(&app)?;

    if !config_dir.exists() {
        std::fs::create_dir_all(&config_dir).map_err(|e| format!("创建目录失败: {e}"))?;
    }

    crate::host_open::open_path(&handle, &config_dir)
        .map_err(|e| format!("打开文件夹失败: {e}"))?;

    Ok(true)
}

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

#[tauri::command]
pub async fn open_app_config_folder(handle: AppHandle) -> Result<bool, String> {
    let config_dir = config::get_app_config_dir();

    if !config_dir.exists() {
        std::fs::create_dir_all(&config_dir).map_err(|e| format!("创建目录失败: {e}"))?;
    }

    crate::host_open::open_path(&handle, &config_dir)
        .map_err(|e| format!("打开文件夹失败: {e}"))?;

    Ok(true)
}

/// 在文件管理器中定位应用诊断日志 cc-switch.log；文件还没生成时打开日志目录
#[tauri::command]
pub async fn reveal_app_log_file() -> Result<bool, String> {
    let log_dir = crate::panic_hook::get_log_dir();
    let log_file = log_dir.join("cc-switch.log");

    let target = if log_file.exists() {
        log_file
    } else {
        std::fs::create_dir_all(&log_dir).map_err(|e| format!("创建目录失败: {e}"))?;
        log_dir
    };

    tauri_plugin_opener::reveal_item_in_dir(&target).map_err(|e| format!("打开文件夹失败: {e}"))?;

    Ok(true)
}
