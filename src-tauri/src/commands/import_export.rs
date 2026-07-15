#![allow(non_snake_case)]

use serde_json::{json, Value};
use std::ffi::OsString;
use std::io::ErrorKind;
use std::path::{Component, Path, PathBuf};
use tauri::State;
use tauri_plugin_dialog::DialogExt;

use crate::commands::sync_support::{
    post_sync_warning_from_result, run_post_import_sync, success_payload_with_warning,
};
use crate::database::backup::BackupEntry;
use crate::database::Database;
use crate::error::AppError;
use crate::services::provider::ProviderService;
use crate::store::AppState;

// ─── File import/export ──────────────────────────────────────

const PORTABLE_EXPORT_FALLBACK_FILE: &str = "cc-switch-export.sql";

fn is_windows_reserved_file_name(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or_default();
    let upper = stem.to_ascii_uppercase();

    matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || upper
            .strip_prefix("COM")
            .or_else(|| upper.strip_prefix("LPT"))
            .is_some_and(|suffix| suffix.len() == 1 && matches!(suffix.as_bytes()[0], b'1'..=b'9'))
}

fn safe_portable_export_file_name(default_name: &str) -> String {
    let candidate = default_name
        .trim()
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or_default()
        .trim();
    let invalid = candidate.is_empty()
        || matches!(candidate, "." | "..")
        || candidate.len() > 240
        || candidate.ends_with(' ')
        || candidate.ends_with('.')
        || candidate
            .chars()
            .any(|character| character.is_control() || r#"<>:"|?*"#.contains(character))
        || is_windows_reserved_file_name(candidate);

    if invalid {
        PORTABLE_EXPORT_FALLBACK_FILE.to_string()
    } else {
        candidate.to_string()
    }
}

fn normalize_absolute_path(path: &Path) -> Result<PathBuf, String> {
    if !path.is_absolute() {
        return Err("export target must be an absolute path".to_string());
    }

    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => {
                normalized.push(component.as_os_str());
            }
            Component::CurDir => {}
            Component::ParentDir => {
                return Err("export target must not contain parent-directory traversal".to_string());
            }
        }
    }

    Ok(normalized)
}

fn canonicalize_parent_allowing_missing(path: &Path) -> Result<PathBuf, String> {
    let mut existing = path;
    let mut missing: Vec<OsString> = Vec::new();

    while !existing.exists() {
        let name = existing
            .file_name()
            .ok_or_else(|| format!("export target has no existing ancestor: {}", path.display()))?;
        missing.push(name.to_os_string());
        existing = existing
            .parent()
            .ok_or_else(|| format!("export target has no existing ancestor: {}", path.display()))?;
    }

    if !existing.is_dir() {
        return Err(format!(
            "export target parent is not a directory: {}",
            existing.display()
        ));
    }

    let mut resolved = std::fs::canonicalize(existing).map_err(|error| {
        format!(
            "failed to resolve export target parent {}: {error}",
            existing.display()
        )
    })?;
    for component in missing.iter().rev() {
        resolved.push(component);
    }

    Ok(resolved)
}

fn validate_export_target_in_data(target: &Path, data_dir: &Path) -> Result<(), String> {
    let normalized_target = normalize_absolute_path(target)?;
    let target_parent = normalized_target.parent().ok_or_else(|| {
        format!(
            "export target has no parent directory: {}",
            target.display()
        )
    })?;

    let canonical_data = std::fs::canonicalize(data_dir).map_err(|error| {
        format!(
            "failed to resolve portable data directory {}: {error}",
            data_dir.display()
        )
    })?;
    if !canonical_data.is_dir() {
        return Err(format!(
            "portable data path is not a directory: {}",
            data_dir.display()
        ));
    }

    let resolved_parent = canonicalize_parent_allowing_missing(target_parent)?;
    if !resolved_parent.starts_with(&canonical_data) {
        return Err(format!(
            "portable mode only allows exports inside {}",
            data_dir.display()
        ));
    }

    match std::fs::symlink_metadata(&normalized_target) {
        Ok(_) => {
            let resolved_target = std::fs::canonicalize(&normalized_target).map_err(|error| {
                format!(
                    "failed to resolve existing export target {}: {error}",
                    normalized_target.display()
                )
            })?;
            if resolved_target.is_dir() {
                return Err(format!(
                    "export target is a directory: {}",
                    target.display()
                ));
            }
            if !resolved_target.starts_with(&canonical_data) {
                return Err(format!(
                    "portable mode only allows exports inside {}",
                    data_dir.display()
                ));
            }
        }
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => {
            return Err(format!(
                "failed to inspect export target {}: {error}",
                normalized_target.display()
            ));
        }
    }

    Ok(())
}

fn validate_portable_export_target(target: &Path) -> Result<(), String> {
    if let Some(paths) = crate::portable::paths() {
        validate_export_target_in_data(target, paths.data_dir())?;
    }
    Ok(())
}

fn portable_save_target(data_dir: &Path, default_name: &str) -> Result<PathBuf, String> {
    let export_dir = data_dir.join("exports");
    let target = export_dir.join(safe_portable_export_file_name(default_name));

    validate_export_target_in_data(&target, data_dir)?;
    std::fs::create_dir_all(&export_dir).map_err(|error| {
        format!(
            "failed to create portable export directory {}: {error}",
            export_dir.display()
        )
    })?;

    validate_export_target_in_data(&target, data_dir)?;
    Ok(target)
}

/// 导出数据库为 SQL 备份
#[tauri::command]
pub async fn export_config_to_file(
    #[allow(non_snake_case)] filePath: String,
    state: State<'_, AppState>,
) -> Result<Value, String> {
    let db = state.db.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let target_path = PathBuf::from(&filePath);
        validate_portable_export_target(&target_path).map_err(AppError::Message)?;
        db.export_sql(&target_path)?;
        Ok::<_, AppError>(json!({
            "success": true,
            "message": "SQL exported successfully",
            "filePath": filePath
        }))
    })
    .await
    .map_err(|e| format!("导出配置失败: {e}"))?
    .map_err(|e: AppError| e.to_string())
}

/// 从 SQL 备份导入数据库
#[tauri::command]
pub async fn import_config_from_file(
    #[allow(non_snake_case)] filePath: String,
    state: State<'_, AppState>,
) -> Result<Value, String> {
    let db = state.db.clone();
    let db_for_sync = db.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let path_buf = PathBuf::from(&filePath);
        let backup_id = db.import_sql(&path_buf)?;
        let warning = post_sync_warning_from_result(Ok(run_post_import_sync(db_for_sync)));
        if let Some(msg) = warning.as_ref() {
            log::warn!("[Import] post-import sync warning: {msg}");
        }
        Ok::<_, AppError>(success_payload_with_warning(backup_id, warning))
    })
    .await
    .map_err(|e| format!("导入配置失败: {e}"))?
    .map_err(|e: AppError| e.to_string())
}

#[tauri::command]
pub async fn sync_current_providers_live(state: State<'_, AppState>) -> Result<Value, String> {
    let db = state.db.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let app_state = AppState::new(db);
        ProviderService::sync_current_to_live(&app_state)?;
        Ok::<_, AppError>(json!({
            "success": true,
            "message": "Live configuration synchronized"
        }))
    })
    .await
    .map_err(|e| format!("同步当前供应商失败: {e}"))?
    .map_err(|e: AppError| e.to_string())
}

// ─── File dialogs ────────────────────────────────────────────

/// 保存文件对话框
#[tauri::command]
pub async fn save_file_dialog<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    #[allow(non_snake_case)] defaultName: String,
) -> Result<Option<String>, String> {
    if let Some(paths) = crate::portable::paths() {
        let target = portable_save_target(paths.data_dir(), &defaultName)?;
        return Ok(Some(target.to_string_lossy().into_owned()));
    }

    let dialog = app.dialog();
    let result = dialog
        .file()
        .add_filter("SQL", &["sql"])
        .set_file_name(&defaultName)
        .blocking_save_file();

    Ok(result.map(|p| p.to_string()))
}

/// 打开文件对话框
#[tauri::command]
pub async fn open_file_dialog<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
) -> Result<Option<String>, String> {
    let dialog = app.dialog();
    let result = dialog
        .file()
        .add_filter("SQL", &["sql"])
        .blocking_pick_file();

    Ok(result.map(|p| p.to_string()))
}

/// 打开 ZIP 文件选择对话框
#[tauri::command]
pub async fn open_zip_file_dialog<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
) -> Result<Option<String>, String> {
    let dialog = app.dialog();
    let result = dialog
        .file()
        .add_filter("ZIP / Skill", &["zip", "skill"])
        .blocking_pick_file();

    Ok(result.map(|p| p.to_string()))
}

// ─── Database backup management ─────────────────────────────

/// Manually create a database backup
#[tauri::command]
pub async fn create_db_backup(state: State<'_, AppState>) -> Result<String, String> {
    let db = state.db.clone();
    tauri::async_runtime::spawn_blocking(move || match db.backup_database_file()? {
        Some(path) => Ok(path
            .file_name()
            .map(|f| f.to_string_lossy().into_owned())
            .unwrap_or_default()),
        None => Err(AppError::Config(
            "Database file not found, backup skipped".to_string(),
        )),
    })
    .await
    .map_err(|e| format!("Backup failed: {e}"))?
    .map_err(|e: AppError| e.to_string())
}

/// List all database backup files
#[tauri::command]
pub fn list_db_backups() -> Result<Vec<BackupEntry>, String> {
    Database::list_backups().map_err(|e| e.to_string())
}

/// Restore database from a backup file
#[tauri::command]
pub async fn restore_db_backup(
    state: State<'_, AppState>,
    filename: String,
) -> Result<String, String> {
    let db = state.db.clone();
    tauri::async_runtime::spawn_blocking(move || db.restore_from_backup(&filename))
        .await
        .map_err(|e| format!("Restore failed: {e}"))?
        .map_err(|e: AppError| e.to_string())
}

/// Rename a database backup file
#[tauri::command]
pub fn rename_db_backup(
    #[allow(non_snake_case)] oldFilename: String,
    #[allow(non_snake_case)] newName: String,
) -> Result<String, String> {
    Database::rename_backup(&oldFilename, &newName).map_err(|e| e.to_string())
}

/// Delete a database backup file
#[tauri::command]
pub fn delete_db_backup(filename: String) -> Result<(), String> {
    Database::delete_backup(&filename).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::{
        portable_save_target, safe_portable_export_file_name, validate_export_target_in_data,
        PORTABLE_EXPORT_FALLBACK_FILE,
    };
    use std::path::Path;

    #[test]
    fn portable_export_allows_missing_parent_inside_data() {
        let temp = tempfile::tempdir().expect("create temp directory");
        let data = temp.path().join("data");
        std::fs::create_dir_all(&data).expect("create data directory");

        let target = data.join("exports").join("config.sql");
        assert!(validate_export_target_in_data(&target, &data).is_ok());
    }

    #[test]
    fn portable_export_rejects_target_outside_data() {
        let temp = tempfile::tempdir().expect("create temp directory");
        let data = temp.path().join("data");
        std::fs::create_dir_all(&data).expect("create data directory");

        let target = temp.path().join("outside.sql");
        assert!(validate_export_target_in_data(&target, &data).is_err());
    }

    #[test]
    fn portable_export_rejects_parent_directory_escape() {
        let temp = tempfile::tempdir().expect("create temp directory");
        let data = temp.path().join("data");
        std::fs::create_dir_all(&data).expect("create data directory");

        let target = data
            .join("exports")
            .join("..")
            .join("..")
            .join("outside.sql");
        assert!(validate_export_target_in_data(&target, &data).is_err());
    }

    #[test]
    fn portable_export_rejects_parent_traversal_even_when_it_stays_inside_data() {
        let temp = tempfile::tempdir().expect("create temp directory");
        let data = temp.path().join("data");
        std::fs::create_dir_all(&data).expect("create data directory");

        let target = data
            .join("exports")
            .join("nested")
            .join("..")
            .join("config.sql");
        assert!(validate_export_target_in_data(&target, &data).is_err());
    }

    #[test]
    fn portable_export_rejects_relative_target() {
        let temp = tempfile::tempdir().expect("create temp directory");
        let data = temp.path().join("data");
        std::fs::create_dir_all(&data).expect("create data directory");

        assert!(validate_export_target_in_data(Path::new("exports/config.sql"), &data).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn portable_export_rejects_existing_symlink_outside_data() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().expect("create temp directory");
        let data = temp.path().join("data");
        std::fs::create_dir_all(&data).expect("create data directory");
        let outside = temp.path().join("outside.sql");
        std::fs::write(&outside, "outside").expect("create outside file");
        let target = data.join("export.sql");
        symlink(&outside, &target).expect("create target symlink");

        assert!(validate_export_target_in_data(&target, &data).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn portable_export_rejects_dangling_symlink_target() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().expect("create temp directory");
        let data = temp.path().join("data");
        std::fs::create_dir_all(&data).expect("create data directory");
        let target = data.join("export.sql");
        symlink(temp.path().join("missing.sql"), &target).expect("create dangling symlink");

        assert!(validate_export_target_in_data(&target, &data).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn portable_save_target_rejects_linked_export_directory_before_writing() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().expect("create temp directory");
        let data = temp.path().join("data");
        let outside = temp.path().join("outside");
        std::fs::create_dir_all(&data).expect("create data directory");
        std::fs::create_dir_all(&outside).expect("create outside directory");
        symlink(&outside, data.join("exports")).expect("create exports symlink");

        assert!(portable_save_target(&data, "export.sql").is_err());
        assert!(std::fs::read_dir(&outside)
            .expect("read outside directory")
            .next()
            .is_none());
    }

    #[test]
    fn portable_save_target_uses_data_exports_without_requiring_it_to_exist() {
        let temp = tempfile::tempdir().expect("create temp directory");
        let data = temp.path().join("data");
        std::fs::create_dir_all(&data).expect("create data directory");

        let target = portable_save_target(&data, r"C:\Users\tester\Desktop\backup.sql")
            .expect("resolve portable save target");

        assert_eq!(target, data.join("exports").join("backup.sql"));
        assert!(data.join("exports").is_dir());
    }

    #[test]
    fn portable_export_file_name_rejects_unsafe_or_reserved_names() {
        for name in ["", ".", "..", "NUL.sql", "COM1", "bad:name.sql"] {
            assert_eq!(
                safe_portable_export_file_name(name),
                PORTABLE_EXPORT_FALLBACK_FILE
            );
        }

        assert_eq!(
            safe_portable_export_file_name("../nested/backup.sql"),
            "backup.sql"
        );
    }

    #[cfg(windows)]
    #[test]
    fn portable_export_rejects_c_drive_target_outside_data() {
        let temp = tempfile::tempdir().expect("create temp directory");
        let data = temp.path().join("data");
        std::fs::create_dir_all(&data).expect("create data directory");

        assert!(
            validate_export_target_in_data(Path::new(r"C:\cc-switch-export.sql"), &data).is_err()
        );
    }
}
