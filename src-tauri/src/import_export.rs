use chrono::Utc;
use serde_json::{json, Value};
use std::fs;
use std::path::PathBuf;

// 默认仅保留最近 10 份备份，避免目录无限膨胀
const MAX_BACKUPS: usize = 10;

/// 创建配置文件备份
pub fn create_backup(config_path: &PathBuf) -> Result<String, String> {
    if !config_path.exists() {
        return Ok(String::new());
    }

    let timestamp = Utc::now().format("%Y%m%d_%H%M%S");
    let backup_id = format!("backup_{}", timestamp);

    let backup_dir = config_path
        .parent()
        .ok_or("Invalid config path")?
        .join("backups");

    // 创建备份目录
    fs::create_dir_all(&backup_dir)
        .map_err(|e| format!("Failed to create backup directory: {}", e))?;

    let backup_path = backup_dir.join(format!("{}.json", backup_id));

    // 复制配置文件到备份
    fs::copy(config_path, backup_path).map_err(|e| format!("Failed to create backup: {}", e))?;

    // 备份完成后清理旧的备份文件（仅保留最近 MAX_BACKUPS 份）
    cleanup_old_backups(&backup_dir, MAX_BACKUPS)?;

    Ok(backup_id)
}

fn cleanup_old_backups(backup_dir: &PathBuf, retain: usize) -> Result<(), String> {
    if retain == 0 {
        return Ok(());
    }

    let mut entries: Vec<_> = match fs::read_dir(backup_dir) {
        Ok(iter) => iter
            .filter_map(|entry| entry.ok())
            .filter(|entry| {
                entry
                    .path()
                    .extension()
                    .map(|ext| ext == "json")
                    .unwrap_or(false)
            })
            .collect(),
        Err(_) => return Ok(()),
    };

    if entries.len() <= retain {
        return Ok(());
    }

    let remove_count = entries.len().saturating_sub(retain);

    entries.sort_by(|a, b| {
        let a_time = a.metadata().and_then(|m| m.modified()).ok();
        let b_time = b.metadata().and_then(|m| m.modified()).ok();
        a_time.cmp(&b_time)
    });

    for entry in entries.into_iter().take(remove_count) {
        if let Err(err) = fs::remove_file(entry.path()) {
            log::warn!(
                "Failed to remove old backup {}: {}",
                entry.path().display(),
                err
            );
        }
    }

    Ok(())
}

/// 导出配置文件
#[tauri::command]
pub async fn export_config_to_file(file_path: String) -> Result<Value, String> {
    // 读取当前配置文件
    let config_path = crate::config::get_app_config_path();
    let config_content = fs::read_to_string(&config_path)
        .map_err(|e| format!("Failed to read configuration: {}", e))?;

    // 写入到指定文件
    fs::write(&file_path, &config_content).map_err(|e| format!("Failed to write file: {}", e))?;

    Ok(json!({
        "success": true,
        "message": "Configuration exported successfully",
        "filePath": file_path
    }))
}

/// 从文件导入配置
#[tauri::command]
pub async fn import_config_from_file(
    file_path: String,
    state: tauri::State<'_, crate::store::AppState>,
) -> Result<Value, String> {
    // 读取导入的文件
    let import_content =
        fs::read_to_string(&file_path).map_err(|e| format!("Failed to read import file: {}", e))?;

    // 验证并解析为配置对象
    let new_config: crate::app_config::MultiAppConfig = serde_json::from_str(&import_content)
        .map_err(|e| format!("Invalid configuration file: {}", e))?;

    // 备份当前配置
    let config_path = crate::config::get_app_config_path();
    let backup_id = create_backup(&config_path)?;

    // 写入新配置到磁盘
    fs::write(&config_path, &import_content)
        .map_err(|e| format!("Failed to write configuration: {}", e))?;

    // 更新内存中的状态
    {
        let mut config_state = state
            .config
            .lock()
            .map_err(|e| format!("Failed to lock config: {}", e))?;
        *config_state = new_config;
    }

    Ok(json!({
        "success": true,
        "message": "Configuration imported successfully",
        "backupId": backup_id
    }))
}

/// 保存文件对话框
#[tauri::command]
pub async fn save_file_dialog<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    default_name: String,
) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;

    let dialog = app.dialog();
    let result = dialog
        .file()
        .add_filter("JSON", &["json"])
        .set_file_name(&default_name)
        .blocking_save_file();

    Ok(result.map(|p| p.to_string()))
}

/// 打开文件对话框
#[tauri::command]
pub async fn open_file_dialog<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;

    let dialog = app.dialog();
    let result = dialog
        .file()
        .add_filter("JSON", &["json"])
        .blocking_pick_file();

    Ok(result.map(|p| p.to_string()))
}
