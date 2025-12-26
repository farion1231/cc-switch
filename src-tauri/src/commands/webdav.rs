#![allow(non_snake_case)]

use serde_json::{json, Value};
use tauri::State;

use crate::config;
use crate::error::AppError;
use crate::services::provider::ProviderService;
use crate::store::AppState;
use crate::webdav::{export_to_webdav, generate_backup_filename, import_from_webdav, WebDavConfig};

/// 保存 WebDAV 配置
#[tauri::command]
pub async fn save_webdav_config(
    config: WebDavConfig,
    state: State<'_, AppState>,
) -> Result<Value, String> {
    let db = state.db.clone();
    tauri::async_runtime::spawn_blocking(move || {
        // 测试连接
        let test_client = crate::webdav::WebDavClient::new(config.clone())
            .map_err(|e| e.to_string())?;

        tauri::async_runtime::block_on(async {
            test_client
                .test_connection()
                .await
                .map_err(|e| e.to_string())?;
            Ok::<_, String>(())
        })?;

        // 保存配置到数据库
        db.save_webdav_config(&config)
            .map_err(|e| e.to_string())?;

        Ok(json!({
            "success": true,
            "message": "WebDAV 配置已保存"
        }))
    })
    .await
    .map_err(|e| format!("保存 WebDAV 配置失败: {e}"))?
}

/// 获取 WebDAV 配置
#[tauri::command]
pub async fn get_webdav_config(
    state: State<'_, AppState>,
) -> Result<Option<WebDavConfig>, String> {
    let db = state.db.clone();
    tauri::async_runtime::spawn_blocking(move || {
        db.get_webdav_config()
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("获取 WebDAV 配置失败: {e}"))?
}

/// 测试 WebDAV 连接
#[tauri::command]
pub async fn test_webdav_connection(
    config: WebDavConfig,
) -> Result<Value, String> {
    let test_client = crate::webdav::WebDavClient::new(config.clone())
        .map_err(|e| format!("创建 WebDAV 客户端失败: {e}"))?;

    test_client
        .test_connection()
        .await
        .map_err(|e| format!("WebDAV 连接测试失败: {e}"))?;

    Ok(json!({
        "success": true,
        "message": "WebDAV 连接成功"
    }))
}

/// 导出配置到 WebDAV
#[tauri::command]
pub async fn export_config_to_webdav(
    config: WebDavConfig,
    _state: State<'_, AppState>,
) -> Result<Value, String> {
    log::info!("开始导出配置到 WebDAV");
    log::info!("配置: url={}, remote_path={}", config.url, config.remote_path);

    tauri::async_runtime::spawn_blocking(move || {
        // 获取数据库路径
        let db_path = config::get_app_config_dir().join("cc-switch.db");
        if !db_path.exists() {
            return Err(AppError::Config("数据库文件不存在".to_string()).to_string());
        }

        // 生成备份文件名
        let filename = generate_backup_filename("cc-switch-export");
        log::info!("生成备份文件名: {}", filename);

        tauri::async_runtime::block_on(async {
            export_to_webdav(&db_path, &config, &filename)
                .await
                .map_err(|e| {
                    log::error!("导出到 WebDAV 失败: {e}");
                    format!("导出到 WebDAV 失败: {e}")
                })?;
            Ok::<_, String>(())
        })?;

        log::info!("成功导出到 WebDAV: {}", filename);

        Ok(json!({
            "success": true,
            "message": "配置已导出到 WebDAV",
            "filename": filename
        }))
    })
    .await
    .map_err(|e| format!("导出到 WebDAV 失败: {e}"))?
}

/// 从 WebDAV 导入配置
#[tauri::command]
pub async fn import_config_from_webdav(
    config: WebDavConfig,
    filename: String,
    state: State<'_, AppState>,
) -> Result<Value, String> {
    log::info!("开始从 WebDAV 导入配置");
    log::info!("配置: url={}, remote_path={}", config.url, config.remote_path);
    log::info!("文件名: {}", filename);

    let db = state.db.clone();
    tauri::async_runtime::spawn_blocking(move || {
        // 创建临时文件路径
        let temp_dir = config::get_app_config_dir().join("temp");
        let local_path = temp_dir.join(&filename);
        log::info!("临时文件路径: {:?}", local_path);

        tauri::async_runtime::block_on(async {
            // 从 WebDAV 下载文件
            log::info!("开始从 WebDAV 下载文件");
            import_from_webdav(&local_path, &config, &filename)
                .await
                .map_err(|e| {
                    log::error!("从 WebDAV 下载失败: {e}");
                    format!("从 WebDAV 导入失败: {e}")
                })?;
            log::info!("文件下载成功");
            Ok::<_, String>(())
        })?;

        // 检查文件是否存在
        if !local_path.exists() {
            log::error!("下载的文件不存在: {:?}", local_path);
            return Err("下载的文件不存在".to_string());
        }
        log::info!("文件存在，大小: {} 字节", local_path.metadata().map(|m| m.len()).unwrap_or(0));

        // 导入到数据库
        log::info!("开始导入到数据库");
        let backup_id = db
            .import_sql(&local_path)
            .map_err(|e| {
                log::error!("导入数据库失败: {e}");
                format!("导入数据库失败: {e}")
            })?;
        log::info!("数据库导入成功，backup_id: {}", backup_id);

        // 清理临时文件
        let _ = std::fs::remove_file(&local_path);
        log::info!("临时文件已清理");

        // 导入后同步当前供应商到各自的 live 配置
        let app_state = AppState::new(db);
        if let Err(err) = ProviderService::sync_current_to_live(&app_state) {
            log::warn!("导入后同步 live 配置失败: {err}");
        }

        // 重新加载设置到内存缓存，确保导入的设置生效
        if let Err(err) = crate::settings::reload_settings() {
            log::warn!("导入后重载设置失败: {err}");
        }

        log::info!("WebDAV 导入完成");

        Ok(json!({
            "success": true,
            "message": "配置已从 WebDAV 导入",
            "backupId": backup_id
        }))
    })
    .await
    .map_err(|e| {
        log::error!("WebDAV 导入任务失败: {e}");
        format!("从 WebDAV 导入失败: {e}")
    })?
}

/// 列出 WebDAV 服务器上的备份文件
#[tauri::command]
pub async fn list_webdav_backups(
    config: WebDavConfig,
) -> Result<Value, String> {
    log::info!("开始列出 WebDAV 备份文件");
    log::info!("配置: url={}, remote_path={}", config.url, config.remote_path);

    let client = crate::webdav::WebDavClient::new(config.clone())
        .map_err(|e| format!("创建 WebDAV 客户端失败: {e}"))?;

    let files = client
        .list_files()
        .await
        .map_err(|e| {
            log::error!("列出 WebDAV 文件失败: {e}");
            format!("列出 WebDAV 文件失败: {e}")
        })?;

    log::info!("成功获取 {} 个备份文件", files.len());

    Ok(Value::Array(
        files.into_iter().map(Value::String).collect(),
    ))
}

/// 删除 WebDAV 服务器上的备份文件
#[tauri::command]
pub async fn delete_webdav_backup(
    config: WebDavConfig,
    filename: String,
) -> Result<Value, String> {
    let client = crate::webdav::WebDavClient::new(config.clone())
        .map_err(|e| format!("创建 WebDAV 客户端失败: {e}"))?;

    client
        .delete_file(&filename)
        .await
        .map_err(|e| format!("删除 WebDAV 文件失败: {e}"))?;

    Ok(json!({
        "success": true,
        "message": "备份文件已删除"
    }))
}
