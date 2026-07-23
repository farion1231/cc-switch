use std::str::FromStr;

use crate::app_config::AppType;
use crate::prompt_files::prompt_file_path;
use crate::settings::{self, InjectionToggle};

/// 读取指定应用的全局系统提示文件内容
/// 优先使用 custom_file_path（如果设置），否则使用默认路径
#[tauri::command]
pub async fn get_system_prompt_file(app: String) -> Result<String, String> {
    let toggle = settings::get_injection_toggle(&app);
    let path = if let Some(ref custom) = toggle.custom_file_path {
        std::path::PathBuf::from(custom)
    } else {
        let app_type = AppType::from_str(&app).map_err(|e| e.to_string())?;
        prompt_file_path(&app_type).map_err(|e| e.to_string())?
    };
    if path.exists() {
        std::fs::read_to_string(&path).map_err(|e| format!("读取文件失败: {e}"))
    } else {
        Ok(String::new())
    }
}

/// 保存指定应用的全局系统提示文件内容
/// 优先使用 custom_file_path（如果设置），否则使用默认路径
#[tauri::command]
pub async fn save_system_prompt_file(
    app: String,
    content: String,
) -> Result<(), String> {
    let toggle = settings::get_injection_toggle(&app);
    let path = if let Some(ref custom) = toggle.custom_file_path {
        std::path::PathBuf::from(custom)
    } else {
        let app_type = AppType::from_str(&app).map_err(|e| e.to_string())?;
        prompt_file_path(&app_type).map_err(|e| e.to_string())?
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("创建目录失败: {e}"))?;
    }
    std::fs::write(&path, &content).map_err(|e| format!("保存文件失败: {e}"))?;
    Ok(())
}

/// 获取指定应用的注入开关状态
#[tauri::command]
pub async fn get_injection_toggle(app: String) -> Result<InjectionToggle, String> {
    let app_type = AppType::from_str(&app).map_err(|e| e.to_string())?;
    Ok(settings::get_injection_toggle(app_type.as_str()))
}

/// 设置指定应用的注入开关状态
#[tauri::command]
pub async fn set_injection_toggle(
    app: String,
    toggle: InjectionToggle,
) -> Result<(), String> {
    let app_type = AppType::from_str(&app).map_err(|e| e.to_string())?;
    settings::set_injection_toggle(app_type.as_str(), toggle).map_err(|e| e.to_string())
}

/// 获取共享规则内容
#[tauri::command]
pub async fn get_shared_prompt() -> Result<String, String> {
    Ok(settings::load_shared_prompt())
}

/// 保存共享规则内容
#[tauri::command]
pub async fn save_shared_prompt(content: String) -> Result<(), String> {
    settings::save_shared_prompt(&content).map_err(|e| e.to_string())
}

/// 打开文件选择对话框，选择 .md 文件作为自定义提示词路径
#[tauri::command]
pub async fn pick_system_prompt_file(app: tauri::AppHandle) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;
    let path = app
        .dialog()
        .file()
        .add_filter("Markdown", &["md"])
        .blocking_pick_file();
    Ok(path.map(|p| p.to_string()))
}
