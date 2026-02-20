//! Claude 插件文件监听服务
//!
//! 监听 `~/.claude/plugins/installed_plugins.json` 的变化，
//! 自动同步到 SQLite plugin_states 表。

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use notify_debouncer_mini::{new_debouncer, notify::RecursiveMode};
use serde::Deserialize;
use tauri::{AppHandle, Emitter};

use crate::database::Database;
use crate::error::AppError;

/// installed_plugins.json 的数据结构
#[derive(Debug, Deserialize)]
struct InstalledPlugins {
    plugins: std::collections::HashMap<String, Vec<InstalledPluginEntry>>,
}

#[derive(Debug, Deserialize)]
struct InstalledPluginEntry {
    scope: String,
    #[serde(rename = "installPath")]
    install_path: String,
    #[serde(default)]
    version: Option<String>,
    #[serde(rename = "gitCommitSha", default)]
    git_commit_sha: Option<String>,
}

/// 从 JSON 字符串同步插件到数据库（纯函数，便于测试）
pub fn sync_plugins_from_json(json: &str, db: &Arc<Database>) -> Result<(), AppError> {
    let installed: InstalledPlugins = serde_json::from_str(json)
        .map_err(|e| AppError::Config(format!("解析 installed_plugins.json 失败: {e}")))?;

    // 取当前 DB 中的插件 ID 集合
    let existing_ids: std::collections::HashSet<String> = db
        .get_all_plugin_states()?
        .into_iter()
        .map(|s| s.plugin_id)
        .collect();

    // 新插件集合
    let mut new_ids = std::collections::HashSet::new();

    for (plugin_id, entries) in &installed.plugins {
        if let Some(entry) = entries.first() {
            new_ids.insert(plugin_id.clone());
            let version = entry.version.as_deref().or(entry.git_commit_sha.as_deref());
            db.upsert_plugin_state(plugin_id, &entry.install_path, version, &entry.scope)?;
        }
    }

    // 删除已卸载的插件
    for old_id in existing_ids.difference(&new_ids) {
        db.remove_plugin_state(old_id)?;
    }

    Ok(())
}

/// 从文件路径同步（不存在时静默返回 Ok）
pub fn sync_plugins_from_file_path(path: &Path, db: &Arc<Database>) -> Result<(), AppError> {
    if !path.exists() {
        return Ok(());
    }

    let content = std::fs::read_to_string(path).map_err(|e| AppError::io(path, e))?;

    sync_plugins_from_json(&content, db)
}

/// 启动文件监听服务（在后台线程中运行）
pub fn start_watcher(db: Arc<Database>, app_handle: AppHandle) {
    let installed_plugins_path = crate::config::get_claude_config_dir()
        .join("plugins")
        .join("installed_plugins.json");

    // 启动时先同步一次
    if let Err(e) = sync_plugins_from_file_path(&installed_plugins_path, &db) {
        log::warn!("插件初始同步失败: {e}");
    }

    let watch_path = installed_plugins_path.clone();

    tauri::async_runtime::spawn_blocking(move || {
        let (tx, rx) = std::sync::mpsc::channel();

        let mut debouncer = match new_debouncer(Duration::from_millis(500), tx) {
            Ok(d) => d,
            Err(e) => {
                log::warn!("创建插件文件监听器失败（降级为启动时同步）: {e}");
                return;
            }
        };

        // 监听父目录（文件可能还不存在）
        let watch_dir = watch_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf();

        if let Err(e) = debouncer
            .watcher()
            .watch(&watch_dir, RecursiveMode::NonRecursive)
        {
            log::warn!("监听插件目录失败（降级为启动时同步）: {e}");
            return;
        }

        log::info!("✓ 插件文件监听已启动: {}", watch_dir.display());

        for result in rx {
            match result {
                Ok(events) => {
                    let relevant = events.iter().any(|e| {
                        e.path
                            .file_name()
                            .map(|f| f == "installed_plugins.json")
                            .unwrap_or(false)
                    });

                    if relevant {
                        log::debug!("检测到 installed_plugins.json 变化，开始同步...");
                        if let Err(e) = sync_plugins_from_file_path(&watch_path, &db) {
                            log::warn!("同步插件失败: {e}");
                        } else {
                            log::info!("✓ 插件同步完成");
                        }
                        // 通知前端刷新
                        if let Err(e) = app_handle.emit("plugins://changed", ()) {
                            log::warn!("发送插件变化事件失败: {e}");
                        }
                    }
                }
                Err(errors) => {
                    log::warn!("插件文件监听错误: {errors:?}");
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;
    use std::sync::Arc;

    #[test]
    fn test_sync_new_plugin_added_to_db() {
        let db = Arc::new(Database::memory().unwrap());
        let json = r#"{
            "version": 2,
            "plugins": {
                "foo@bar": [{"scope":"user","installPath":"/tmp/foo","version":"1.0","installedAt":"2026-01-01T00:00:00Z","lastUpdated":"2026-01-01T00:00:00Z","gitCommitSha":"abc"}]
            }
        }"#;
        sync_plugins_from_json(json, &db).unwrap();
        let states = db.get_all_plugin_states().unwrap();
        assert_eq!(states.len(), 1);
        assert_eq!(states[0].plugin_id, "foo@bar");
        assert!(states[0].enabled);
    }

    #[test]
    fn test_sync_removed_plugin_deleted_from_db() {
        let db = Arc::new(Database::memory().unwrap());
        db.upsert_plugin_state("old@reg", "/old", None, "user")
            .unwrap();
        let json = r#"{"version":2,"plugins":{}}"#;
        sync_plugins_from_json(json, &db).unwrap();
        let states = db.get_all_plugin_states().unwrap();
        assert!(states.is_empty());
    }

    #[test]
    fn test_sync_invalid_json_returns_error() {
        let db = Arc::new(Database::memory().unwrap());
        let result = sync_plugins_from_json("not json", &db);
        assert!(result.is_err());
    }

    #[test]
    fn test_sync_missing_file_returns_ok() {
        let db = Arc::new(Database::memory().unwrap());
        let result =
            sync_plugins_from_file_path(Path::new("/nonexistent/path/installed_plugins.json"), &db);
        assert!(result.is_ok());
    }

    #[test]
    fn test_sync_entry_with_only_git_commit_sha() {
        let db = Arc::new(Database::memory().unwrap());
        let json = r#"{
            "version": 2,
            "plugins": {
                "foo@bar": [{"scope":"user","installPath":"/tmp/foo","gitCommitSha":"deadbeef","installedAt":"2026-01-01T00:00:00Z","lastUpdated":"2026-01-01T00:00:00Z"}]
            }
        }"#;
        sync_plugins_from_json(json, &db).unwrap();
        let states = db.get_all_plugin_states().unwrap();
        assert_eq!(states.len(), 1);
        assert_eq!(states[0].version.as_deref(), Some("deadbeef"));
    }
}
