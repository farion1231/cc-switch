//! 数据库备份和恢复
//!
//! 提供 SQL 导出/导入和二进制快照备份功能。

use super::{lock_conn, Database};
use crate::config::get_app_config_dir;
use crate::error::AppError;
use chrono::{Local, Utc};
use rusqlite::backup::Backup;
use rusqlite::types::ValueRef;
use rusqlite::Connection;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::NamedTempFile;

const CC_SWITCH_SQL_EXPORT_HEADER: &str = "-- CC Switch SQLite 导出";

/// Tables whose data rows are skipped when exporting for WebDAV sync.
const SYNC_SKIP_TABLES: &[&str] = &[
    "proxy_request_logs",
    "stream_check_logs",
    "provider_health",
    "proxy_live_backup",
    "usage_daily_rollups",
    // Device-local security / workbench state — never leave this machine via cloud sync.
    "codex_reasoning_rounds",
    "provider_credential_audit",
    "provider_rollback_snapshots",
    "app_configuration_state",
];

/// Tables whose local data is preserved (restored from local snapshot) during WebDAV import.
/// Excludes ephemeral tables like provider_health that can safely rebuild at runtime.
const SYNC_PRESERVE_TABLES: &[&str] = &[
    "proxy_request_logs",
    "stream_check_logs",
    "proxy_live_backup",
    "usage_daily_rollups",
    // Keep local security / recovery history across cloud restore.
    "codex_reasoning_rounds",
    "provider_credential_audit",
    "provider_rollback_snapshots",
    "app_configuration_state",
];

/// A database backup entry for the UI
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupEntry {
    pub filename: String,
    pub size_bytes: u64,
    pub created_at: String, // ISO 8601
}

/// Per-provider opt-in to take remote credential fields during cloud apply.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteCredentialSelection {
    pub app_type: String,
    pub provider_id: String,
    pub use_remote_api_key: bool,
    pub use_remote_base_url: bool,
}

/// Non-destructive restore impact summary. Preview never mutates the live DB.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestorePreview {
    pub preview_id: String,
    pub new_provider_count: u32,
    pub existing_provider_count: u32,
    pub credential_conflicts: Vec<crate::services::provider_security::CredentialDiff>,
    /// Number of credential fields (apiKey/baseUrl) that would change under exact restore.
    pub exact_restore_credential_field_count: u32,
}

impl Database {
    /// 导出为 SQLite 兼容的 SQL 文本（内存字符串，完整导出）
    pub fn export_sql_string(&self) -> Result<String, AppError> {
        let snapshot = self.snapshot_to_memory()?;
        Self::dump_sql(&snapshot, &[])
    }

    /// Export SQL for sync (WebDAV), skipping local-only tables' data
    pub fn export_sql_string_for_sync(&self) -> Result<String, AppError> {
        let snapshot = self.snapshot_to_memory()?;
        Self::dump_sql(&snapshot, SYNC_SKIP_TABLES)
    }

    /// 导出为 SQLite 兼容的 SQL 文本
    pub fn export_sql(&self, target_path: &Path) -> Result<(), AppError> {
        let dump = self.export_sql_string()?;

        if let Some(parent) = target_path.parent() {
            fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
        }

        crate::config::atomic_write(target_path, dump.as_bytes())
    }

    /// 从 SQL 文件导入，返回生成的备份 ID（若无备份则为空字符串）
    pub fn import_sql(&self, source_path: &Path) -> Result<String, AppError> {
        if !source_path.exists() {
            return Err(AppError::InvalidInput(format!(
                "SQL 文件不存在: {}",
                source_path.display()
            )));
        }

        let sql_raw = fs::read_to_string(source_path).map_err(|e| AppError::io(source_path, e))?;
        let sql_content = sql_raw.trim_start_matches('\u{feff}');
        self.import_sql_string(sql_content)
    }

    /// 从 SQL 字符串导入，返回生成的备份 ID（若无备份则为空字符串）
    pub fn import_sql_string(&self, sql_raw: &str) -> Result<String, AppError> {
        self.import_sql_string_inner(sql_raw, &[], |_| Ok(()))
    }

    /// Import SQL generated for sync, then restore local-only tables from the
    /// current device snapshot before replacing the main database.
    ///
    /// Default cloud-restore policy: keep local `api_key`/`base_url` for providers
    /// that already exist on this device, while accepting remote non-credential fields.
    /// Cloud restore with default policy: keep local credentials on existing providers.
    pub(crate) fn import_sql_string_for_sync(&self, sql_raw: &str) -> Result<String, AppError> {
        self.import_sql_string_for_sync_with_selections(sql_raw, &[])
    }

    /// Cloud restore with optional per-provider remote credential opt-ins.
    ///
    /// By default, local `apiKey`/`baseUrl` win for providers that already exist.
    /// Entries in `remote_selections` can opt into remote values for specific fields.
    pub(crate) fn import_sql_string_for_sync_with_selections(
        &self,
        sql_raw: &str,
        remote_selections: &[RemoteCredentialSelection],
    ) -> Result<String, AppError> {
        let local_credentials = self.snapshot_local_provider_credentials()?;
        self.import_sql_string_inner(sql_raw, SYNC_PRESERVE_TABLES, |imported_db| {
            imported_db.merge_local_provider_credentials(&local_credentials, remote_selections)
        })
    }

    /// Capture per-provider settings so credential fields can be reapplied after a
    /// whole-table cloud import. Keyed by `(app_type, provider_id)`.
    fn snapshot_local_provider_credentials(
        &self,
    ) -> Result<Vec<(String, String, serde_json::Value)>, AppError> {
        use crate::app_config::AppType;

        let mut snapshots = Vec::new();
        for app_type in AppType::all() {
            let providers = self.get_all_providers(app_type.as_str())?;
            for (id, provider) in providers {
                snapshots.push((app_type.as_str().to_string(), id, provider.settings_config));
            }
        }
        Ok(snapshots)
    }

    /// Re-apply local credential fields onto providers that still exist after a
    /// cloud restore. Non-credential remote fields (name, notes, etc.) stay.
    fn merge_local_provider_credentials(
        &self,
        local_credentials: &[(String, String, serde_json::Value)],
        remote_selections: &[RemoteCredentialSelection],
    ) -> Result<(), AppError> {
        use crate::app_config::AppType;
        use crate::services::provider_security::{
            apply_selected_credentials, extract_provider_credentials,
        };
        use std::collections::BTreeSet;
        use std::str::FromStr;

        for (app_type_str, provider_id, local_settings) in local_credentials {
            let Ok(app_type) = AppType::from_str(app_type_str) else {
                continue;
            };
            let Some(mut remote_provider) = self.get_provider_by_id(provider_id, app_type_str)?
            else {
                // Provider only existed locally; whole-table import dropped it.
                continue;
            };

            let selection = remote_selections
                .iter()
                .find(|s| s.app_type == *app_type_str && s.provider_id == *provider_id);
            let use_remote_api_key = selection.map(|s| s.use_remote_api_key).unwrap_or(false);
            let use_remote_base_url = selection.map(|s| s.use_remote_base_url).unwrap_or(false);

            let mut local_provider = remote_provider.clone();
            local_provider.settings_config = local_settings.clone();
            let local_creds = extract_provider_credentials(&local_provider, &app_type);

            // Default: re-apply local credential fields. Opt-in remote fields are left
            // as imported (remote) by simply not including them in the confirmed set.
            let mut confirmed = BTreeSet::new();
            if local_creds.api_key.is_some() && !use_remote_api_key {
                confirmed.insert("apiKey".to_string());
            }
            if local_creds.base_url.is_some() && !use_remote_base_url {
                confirmed.insert("baseUrl".to_string());
            }
            if confirmed.is_empty() {
                continue;
            }

            apply_selected_credentials(
                &mut remote_provider,
                local_settings,
                &app_type,
                &confirmed,
            )?;
            self.save_provider(app_type_str, &remote_provider)?;
        }
        Ok(())
    }

    /// Build a non-destructive preview of an exact SQL restore.
    ///
    /// The live database is never mutated. Credential field counts describe
    /// how many `apiKey`/`baseUrl` values would change if the remote SQL were
    /// applied verbatim (exact restore).
    pub fn preview_exact_restore(&self, sql_raw: &str) -> Result<RestorePreview, AppError> {
        use crate::app_config::AppType;
        use crate::services::provider_security::{
            base_urls_equivalent, credential_fingerprint, extract_provider_credentials,
            mask_credential, CredentialDiff,
        };

        // Import into a throwaway in-memory DB so the live connection is untouched.
        let remote_db = Database::memory()?;
        remote_db.import_sql_string(sql_raw)?;

        let mut new_provider_count = 0u32;
        let mut existing_provider_count = 0u32;
        let mut exact_restore_credential_field_count = 0u32;
        let mut credential_conflicts: Vec<CredentialDiff> = Vec::new();

        for app_type in AppType::all() {
            let app_type_str = app_type.as_str();
            let remote_providers = remote_db.get_all_providers(app_type_str)?;
            for (provider_id, remote_provider) in remote_providers {
                match self.get_provider_by_id(&provider_id, app_type_str)? {
                    None => {
                        new_provider_count = new_provider_count.saturating_add(1);
                    }
                    Some(local_provider) => {
                        existing_provider_count = existing_provider_count.saturating_add(1);
                        let local_creds = extract_provider_credentials(&local_provider, &app_type);
                        let remote_creds =
                            extract_provider_credentials(&remote_provider, &app_type);

                        if local_creds.api_key != remote_creds.api_key {
                            exact_restore_credential_field_count =
                                exact_restore_credential_field_count.saturating_add(1);
                            credential_conflicts.push(CredentialDiff {
                                field: "apiKey".to_string(),
                                stored_masked: local_creds.api_key.as_deref().map(mask_credential),
                                live_masked: remote_creds.api_key.as_deref().map(mask_credential),
                                stored_fingerprint: local_creds
                                    .api_key
                                    .as_deref()
                                    .map(|v| credential_fingerprint("apiKey", v)),
                                live_fingerprint: remote_creds
                                    .api_key
                                    .as_deref()
                                    .map(|v| credential_fingerprint("apiKey", v)),
                            });
                        }
                        if !base_urls_equivalent(
                            local_creds.base_url.as_deref(),
                            remote_creds.base_url.as_deref(),
                        )? {
                            exact_restore_credential_field_count =
                                exact_restore_credential_field_count.saturating_add(1);
                            credential_conflicts.push(CredentialDiff {
                                field: "baseUrl".to_string(),
                                stored_masked: local_creds.base_url.as_deref().map(mask_credential),
                                live_masked: remote_creds.base_url.as_deref().map(mask_credential),
                                stored_fingerprint: local_creds
                                    .base_url
                                    .as_deref()
                                    .map(|v| credential_fingerprint("baseUrl", v)),
                                live_fingerprint: remote_creds
                                    .base_url
                                    .as_deref()
                                    .map(|v| credential_fingerprint("baseUrl", v)),
                            });
                        }
                    }
                }
            }
        }

        Ok(RestorePreview {
            preview_id: uuid::Uuid::new_v4().to_string(),
            new_provider_count,
            existing_provider_count,
            credential_conflicts,
            exact_restore_credential_field_count,
        })
    }

    fn import_sql_string_inner<F>(
        &self,
        sql_raw: &str,
        preserve_tables: &[&str],
        finalize_import: F,
    ) -> Result<String, AppError>
    where
        F: FnOnce(&Database) -> Result<(), AppError>,
    {
        let sql_content = sql_raw.trim_start_matches('\u{feff}');
        Self::validate_cc_switch_sql_export(sql_content)?;

        // 导入前备份现有数据库
        let backup_path = self.backup_database_file()?;

        let local_snapshot = if preserve_tables.is_empty() {
            None
        } else {
            Some(self.snapshot_to_memory()?)
        };

        // 在临时数据库执行导入，确保失败不会污染主库
        let temp_file = NamedTempFile::new().map_err(|e| AppError::IoContext {
            context: "创建临时数据库文件失败".to_string(),
            source: e,
        })?;
        let temp_path = temp_file.path().to_path_buf();
        let temp_db = Database {
            conn: std::sync::Mutex::new(
                Connection::open(&temp_path).map_err(|e| AppError::Database(e.to_string()))?,
            ),
        };

        {
            let temp_conn = lock_conn!(temp_db.conn);
            temp_conn
                .execute_batch(sql_content)
                .map_err(|e| AppError::Database(format!("执行 SQL 导入失败: {e}")))?;

            // 补齐缺失表/索引并恢复本机数据，全部在临时库中完成。
            Self::create_tables_on_conn(&temp_conn)?;
            Self::apply_schema_migrations_on_conn(&temp_conn)?;
            if let Some(local_snapshot) = local_snapshot.as_ref() {
                Self::restore_tables(local_snapshot, &temp_conn, preserve_tables)?;
            }
        }

        // Sync-specific credential merge must also succeed before the main DB changes.
        finalize_import(&temp_db)?;

        // 使用 Backup 将临时库原子写回主库
        {
            let temp_conn = lock_conn!(temp_db.conn);
            Self::validate_basic_state(&temp_conn)?;
            let mut main_conn = lock_conn!(self.conn);
            let backup = Backup::new(&temp_conn, &mut main_conn)
                .map_err(|e| AppError::Database(e.to_string()))?;
            backup
                .step(-1)
                .map_err(|e| AppError::Database(e.to_string()))?;
        }

        let backup_id = backup_path
            .and_then(|p| p.file_stem().map(|s| s.to_string_lossy().to_string()))
            .unwrap_or_default();

        Ok(backup_id)
    }

    /// 创建内存快照以避免长时间持有数据库锁
    pub(crate) fn snapshot_to_memory(&self) -> Result<Connection, AppError> {
        let conn = lock_conn!(self.conn);
        let mut snapshot =
            Connection::open_in_memory().map_err(|e| AppError::Database(e.to_string()))?;

        {
            let backup =
                Backup::new(&conn, &mut snapshot).map_err(|e| AppError::Database(e.to_string()))?;
            backup
                .step(-1)
                .map_err(|e| AppError::Database(e.to_string()))?;
        }

        Ok(snapshot)
    }

    fn validate_cc_switch_sql_export(sql: &str) -> Result<(), AppError> {
        let trimmed = sql.trim_start();
        if trimmed.starts_with(CC_SWITCH_SQL_EXPORT_HEADER) {
            return Ok(());
        }

        Err(AppError::localized(
            "backup.sql.invalid_format",
            "仅支持导入由 CC Switch 导出的 SQL 备份文件。",
            "Only SQL backups exported by CC Switch are supported.",
        ))
    }

    fn restore_tables(
        source_conn: &Connection,
        target_conn: &Connection,
        tables: &[&str],
    ) -> Result<(), AppError> {
        for table in tables {
            if !Self::table_exists(source_conn, table)? || !Self::table_exists(target_conn, table)?
            {
                continue;
            }

            let columns = Self::get_table_columns(source_conn, table)?;
            if columns.is_empty() {
                continue;
            }

            target_conn
                .execute(&format!("DELETE FROM \"{table}\""), [])
                .map_err(|e| AppError::Database(format!("清空表 {table} 失败: {e}")))?;

            let placeholders = (1..=columns.len())
                .map(|idx| format!("?{idx}"))
                .collect::<Vec<_>>()
                .join(", ");
            let cols = columns
                .iter()
                .map(|column| format!("\"{column}\""))
                .collect::<Vec<_>>()
                .join(", ");
            let insert_sql = format!("INSERT INTO \"{table}\" ({cols}) VALUES ({placeholders})");

            let mut stmt = source_conn
                .prepare(&format!("SELECT * FROM \"{table}\""))
                .map_err(|e| AppError::Database(format!("读取表 {table} 失败: {e}")))?;
            let mut rows = stmt
                .query([])
                .map_err(|e| AppError::Database(format!("查询表 {table} 数据失败: {e}")))?;

            while let Some(row) = rows.next().map_err(|e| AppError::Database(e.to_string()))? {
                let mut values = Vec::with_capacity(columns.len());
                for idx in 0..columns.len() {
                    values.push(
                        row.get::<_, rusqlite::types::Value>(idx)
                            .map_err(|e| AppError::Database(e.to_string()))?,
                    );
                }

                target_conn
                    .execute(&insert_sql, rusqlite::params_from_iter(values.iter()))
                    .map_err(|e| AppError::Database(format!("恢复表 {table} 数据失败: {e}")))?;
            }
        }

        Ok(())
    }

    /// Periodic backup: create a new backup if the latest one is older than the configured interval
    pub(crate) fn periodic_backup_if_needed(&self) -> Result<(), AppError> {
        let interval_hours = crate::settings::effective_backup_interval_hours();
        if interval_hours > 0 {
            let backup_dir = get_app_config_dir().join("backups");
            if !backup_dir.exists() {
                self.backup_database_file()?;
            } else {
                let latest = fs::read_dir(&backup_dir).ok().and_then(|entries| {
                    entries
                        .filter_map(|e| e.ok())
                        .filter(|e| e.path().extension().map(|ext| ext == "db").unwrap_or(false))
                        .filter_map(|e| e.metadata().ok().and_then(|m| m.modified().ok()))
                        .max()
                });

                let interval_secs = u64::from(interval_hours) * 3600;
                let needs_backup = match latest {
                    None => true,
                    Some(last_modified) => {
                        last_modified.elapsed().unwrap_or_default()
                            > std::time::Duration::from_secs(interval_secs)
                    }
                };

                if needs_backup {
                    log::info!(
                        "Periodic backup: latest backup is older than {interval_hours} hours, creating new backup"
                    );
                    self.backup_database_file()?;
                }
            }
        }

        // Periodic maintenance is always enabled, regardless of auto-backup settings.
        let mut reclaimed_rows = 0u64;
        match self.cleanup_old_stream_check_logs(7) {
            Ok(deleted) => {
                reclaimed_rows += deleted;
            }
            Err(e) => {
                log::warn!("Periodic stream_check_logs cleanup failed: {e}");
            }
        }
        match self.rollup_and_prune(30) {
            Ok(deleted) => {
                reclaimed_rows += deleted;
            }
            Err(e) => {
                log::warn!("Periodic rollup_and_prune failed: {e}");
            }
        }
        if reclaimed_rows > 0 {
            let conn = lock_conn!(self.conn);
            if let Err(e) = conn.execute_batch("PRAGMA incremental_vacuum;") {
                log::warn!("Periodic incremental vacuum failed: {e}");
            }
        }

        Ok(())
    }

    /// 生成一致性快照备份，返回备份文件路径（不存在主库时返回 None）
    pub(crate) fn backup_database_file(&self) -> Result<Option<PathBuf>, AppError> {
        let db_path = get_app_config_dir().join("cc-switch.db");
        if !db_path.exists() {
            return Ok(None);
        }

        let backup_dir = db_path
            .parent()
            .ok_or_else(|| AppError::Config("无效的数据库路径".to_string()))?
            .join("backups");

        fs::create_dir_all(&backup_dir).map_err(|e| AppError::io(&backup_dir, e))?;

        let base_id = format!("db_backup_{}", Local::now().format("%Y%m%d_%H%M%S"));
        let mut backup_id = base_id.clone();
        let mut backup_path = backup_dir.join(format!("{backup_id}.db"));
        let mut counter = 1;
        while backup_path.exists() {
            backup_id = format!("{base_id}_{counter}");
            backup_path = backup_dir.join(format!("{backup_id}.db"));
            counter += 1;
        }

        {
            let conn = lock_conn!(self.conn);
            let mut dest_conn =
                Connection::open(&backup_path).map_err(|e| AppError::Database(e.to_string()))?;
            let backup = Backup::new(&conn, &mut dest_conn)
                .map_err(|e| AppError::Database(e.to_string()))?;
            backup
                .step(-1)
                .map_err(|e| AppError::Database(e.to_string()))?;
        }

        Self::cleanup_db_backups(&backup_dir)?;
        Ok(Some(backup_path))
    }

    /// 清理旧的数据库备份，保留最新的 N 个
    fn cleanup_db_backups(dir: &Path) -> Result<(), AppError> {
        let retain = crate::settings::effective_backup_retain_count();
        let entries = match fs::read_dir(dir) {
            Ok(iter) => iter
                .filter_map(|entry| entry.ok())
                .filter(|entry| {
                    entry
                        .path()
                        .extension()
                        .map(|ext| ext == "db")
                        .unwrap_or(false)
                })
                .collect::<Vec<_>>(),
            Err(_) => return Ok(()),
        };

        if entries.len() <= retain {
            return Ok(());
        }

        let remove_count = entries.len().saturating_sub(retain);
        let mut sorted = entries;
        sorted.sort_by_key(|entry| entry.metadata().and_then(|m| m.modified()).ok());

        for entry in sorted.into_iter().take(remove_count) {
            if let Err(err) = fs::remove_file(entry.path()) {
                log::warn!("删除旧数据库备份失败 {}: {}", entry.path().display(), err);
            }
        }
        Ok(())
    }

    /// 基础状态校验
    fn validate_basic_state(conn: &Connection) -> Result<(), AppError> {
        let provider_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM providers", [], |row| row.get(0))
            .map_err(|e| AppError::Database(e.to_string()))?;
        let mcp_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM mcp_servers", [], |row| row.get(0))
            .map_err(|e| AppError::Database(e.to_string()))?;

        if provider_count == 0 && mcp_count == 0 {
            return Err(AppError::Config(
                "导入的 SQL 未包含有效的供应商或 MCP 数据".to_string(),
            ));
        }
        Ok(())
    }

    /// 导出数据库为 SQL 文本
    fn dump_sql(conn: &Connection, skip_tables: &[&str]) -> Result<String, AppError> {
        let mut output = String::new();
        let timestamp = Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
        let user_version: i64 = conn
            .query_row("PRAGMA user_version;", [], |row| row.get(0))
            .unwrap_or(0);

        output.push_str(&format!(
            "-- CC Switch SQLite 导出\n-- 生成时间: {timestamp}\n-- user_version: {user_version}\n"
        ));
        output.push_str("PRAGMA foreign_keys=OFF;\n");
        output.push_str(&format!("PRAGMA user_version={user_version};\n"));
        output.push_str("BEGIN TRANSACTION;\n");

        // 导出 schema
        let mut stmt = conn
            .prepare(
                "SELECT type, name, tbl_name, sql
                 FROM sqlite_master
                 WHERE sql NOT NULL AND type IN ('table','index','trigger','view')
                 ORDER BY type='table' DESC, name",
            )
            .map_err(|e| AppError::Database(e.to_string()))?;

        let mut tables = Vec::new();
        let mut rows = stmt
            .query([])
            .map_err(|e| AppError::Database(e.to_string()))?;
        while let Some(row) = rows.next().map_err(|e| AppError::Database(e.to_string()))? {
            let obj_type: String = row.get(0).map_err(|e| AppError::Database(e.to_string()))?;
            let name: String = row.get(1).map_err(|e| AppError::Database(e.to_string()))?;
            let sql: String = row.get(3).map_err(|e| AppError::Database(e.to_string()))?;

            // 跳过 SQLite 内部对象（如 sqlite_sequence）
            if name.starts_with("sqlite_") {
                continue;
            }

            output.push_str(&sql);
            output.push_str(";\n");

            if obj_type == "table" && !name.starts_with("sqlite_") {
                tables.push(name);
            }
        }

        // 导出数据
        for table in tables {
            if skip_tables.iter().any(|t| *t == table) {
                continue;
            }
            let columns = Self::get_table_columns(conn, &table)?;
            if columns.is_empty() {
                continue;
            }

            let mut stmt = conn
                .prepare(&format!("SELECT * FROM \"{table}\""))
                .map_err(|e| AppError::Database(e.to_string()))?;
            let mut rows = stmt
                .query([])
                .map_err(|e| AppError::Database(e.to_string()))?;

            while let Some(row) = rows.next().map_err(|e| AppError::Database(e.to_string()))? {
                let mut values = Vec::with_capacity(columns.len());
                for idx in 0..columns.len() {
                    let value = row
                        .get_ref(idx)
                        .map_err(|e| AppError::Database(e.to_string()))?;
                    values.push(Self::format_sql_value(value)?);
                }

                let cols = columns
                    .iter()
                    .map(|c| format!("\"{c}\""))
                    .collect::<Vec<_>>()
                    .join(", ");
                output.push_str(&format!(
                    "INSERT INTO \"{table}\" ({cols}) VALUES ({});\n",
                    values.join(", ")
                ));
            }
        }

        output.push_str("COMMIT;\nPRAGMA foreign_keys=ON;\n");
        Ok(output)
    }

    /// 获取表的列名列表
    fn get_table_columns(conn: &Connection, table: &str) -> Result<Vec<String>, AppError> {
        let mut stmt = conn
            .prepare(&format!("PRAGMA table_info(\"{table}\")"))
            .map_err(|e| AppError::Database(e.to_string()))?;
        let iter = stmt
            .query_map([], |row| row.get::<_, String>(1))
            .map_err(|e| AppError::Database(e.to_string()))?;

        let mut columns = Vec::new();
        for col in iter {
            columns.push(col.map_err(|e| AppError::Database(e.to_string()))?);
        }
        Ok(columns)
    }

    /// 格式化 SQL 值
    fn format_sql_value(value: ValueRef<'_>) -> Result<String, AppError> {
        match value {
            ValueRef::Null => Ok("NULL".to_string()),
            ValueRef::Integer(i) => Ok(i.to_string()),
            ValueRef::Real(f) => Ok(f.to_string()),
            ValueRef::Text(t) => {
                let text = std::str::from_utf8(t)
                    .map_err(|e| AppError::Database(format!("文本字段不是有效的 UTF-8: {e}")))?;
                let escaped = text.replace('\'', "''");
                Ok(format!("'{escaped}'"))
            }
            ValueRef::Blob(bytes) => {
                let mut s = String::from("X'");
                for b in bytes {
                    use std::fmt::Write;
                    let _ = write!(&mut s, "{b:02X}");
                }
                s.push('\'');
                Ok(s)
            }
        }
    }

    /// List all database backup files, sorted by creation time (newest first)
    pub fn list_backups() -> Result<Vec<BackupEntry>, AppError> {
        let backup_dir = get_app_config_dir().join("backups");
        if !backup_dir.exists() {
            return Ok(vec![]);
        }

        let mut entries: Vec<BackupEntry> = fs::read_dir(&backup_dir)
            .map_err(|e| AppError::io(&backup_dir, e))?
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().map(|ext| ext == "db").unwrap_or(false))
            .filter_map(|e| {
                let metadata = e.metadata().ok()?;
                let filename = e.file_name().to_string_lossy().to_string();
                let size_bytes = metadata.len();
                let created_at = metadata
                    .modified()
                    .ok()
                    .map(|t| {
                        let dt: chrono::DateTime<Utc> = t.into();
                        dt.to_rfc3339()
                    })
                    .unwrap_or_default();
                Some(BackupEntry {
                    filename,
                    size_bytes,
                    created_at,
                })
            })
            .collect();

        // Sort by created_at descending (newest first)
        entries.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(entries)
    }

    /// Restore database from a backup file. Returns the safety backup ID.
    pub fn restore_from_backup(&self, filename: &str) -> Result<String, AppError> {
        // Security: validate filename to prevent path traversal
        if filename.contains("..")
            || filename.contains('/')
            || filename.contains('\\')
            || !filename.ends_with(".db")
        {
            return Err(AppError::InvalidInput(
                "Invalid backup filename".to_string(),
            ));
        }

        let backup_dir = get_app_config_dir().join("backups");
        let backup_path = backup_dir.join(filename);

        if !backup_path.exists() {
            return Err(AppError::InvalidInput(format!(
                "Backup file not found: {filename}"
            )));
        }

        // Step 1: Create safety backup of current database
        let safety_backup = self.backup_database_file()?;
        let safety_id = safety_backup
            .and_then(|p| p.file_stem().map(|s| s.to_string_lossy().to_string()))
            .unwrap_or_default();

        // Step 2: Open the backup file and restore it to the main database
        let source_conn =
            Connection::open(&backup_path).map_err(|e| AppError::Database(e.to_string()))?;

        {
            let mut main_conn = lock_conn!(self.conn);
            let backup = Backup::new(&source_conn, &mut main_conn)
                .map_err(|e| AppError::Database(e.to_string()))?;
            backup
                .step(-1)
                .map_err(|e| AppError::Database(e.to_string()))?;
        }

        // Step 3: Run schema migrations (backup may be from an older version)
        self.create_tables()?;
        self.apply_schema_migrations()?;
        self.ensure_model_pricing_seeded()?;

        log::info!("Database restored from backup: {filename}, safety backup: {safety_id}");
        Ok(safety_id)
    }

    /// Rename a backup file. Returns the new filename.
    pub fn rename_backup(old_filename: &str, new_name: &str) -> Result<String, AppError> {
        // Validate old filename (path traversal + .db suffix)
        if old_filename.contains("..")
            || old_filename.contains('/')
            || old_filename.contains('\\')
            || !old_filename.ends_with(".db")
        {
            return Err(AppError::InvalidInput(
                "Invalid backup filename".to_string(),
            ));
        }

        // Clean new name
        let trimmed = new_name.trim();
        if trimmed.is_empty() {
            return Err(AppError::InvalidInput(
                "New name cannot be empty".to_string(),
            ));
        }

        // Length limit (without .db suffix)
        let name_part = trimmed.strip_suffix(".db").unwrap_or(trimmed);
        if name_part.len() > 100 {
            return Err(AppError::InvalidInput(
                "Name too long (max 100 characters)".to_string(),
            ));
        }

        // Prevent path traversal in new name
        if name_part.contains("..")
            || name_part.contains('/')
            || name_part.contains('\\')
            || name_part.contains('\0')
        {
            return Err(AppError::InvalidInput(
                "Invalid characters in new name".to_string(),
            ));
        }

        let new_filename = format!("{name_part}.db");

        let backup_dir = get_app_config_dir().join("backups");
        let old_path = backup_dir.join(old_filename);
        let new_path = backup_dir.join(&new_filename);

        if !old_path.exists() {
            return Err(AppError::InvalidInput(format!(
                "Backup file not found: {old_filename}"
            )));
        }

        if new_path.exists() {
            return Err(AppError::InvalidInput(format!(
                "A backup named '{new_filename}' already exists"
            )));
        }

        fs::rename(&old_path, &new_path).map_err(|e| AppError::io(&old_path, e))?;
        log::info!("Renamed backup: {old_filename} -> {new_filename}");
        Ok(new_filename)
    }

    /// Delete a backup file permanently.
    pub fn delete_backup(filename: &str) -> Result<(), AppError> {
        // Validate filename (path traversal + .db suffix)
        if filename.contains("..")
            || filename.contains('/')
            || filename.contains('\\')
            || !filename.ends_with(".db")
        {
            return Err(AppError::InvalidInput(
                "Invalid backup filename".to_string(),
            ));
        }

        let backup_path = get_app_config_dir().join("backups").join(filename);
        if !backup_path.exists() {
            return Err(AppError::InvalidInput(format!(
                "Backup file not found: {filename}"
            )));
        }

        fs::remove_file(&backup_path).map_err(|e| AppError::io(&backup_path, e))?;
        log::info!("Deleted backup: {filename}");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{Database, RemoteCredentialSelection};
    use crate::error::AppError;
    use crate::settings::{update_settings, AppSettings};
    use serial_test::serial;

    #[test]
    fn sync_import_preserves_local_only_tables() -> Result<(), AppError> {
        let remote_db = Database::memory()?;
        {
            let conn = crate::database::lock_conn!(remote_db.conn);
            conn.execute(
                "INSERT INTO providers (id, app_type, name, settings_config, meta)
                 VALUES ('remote-provider', 'claude', 'Remote Provider', '{}', '{}')",
                [],
            )?;
        }
        let remote_sql = remote_db.export_sql_string_for_sync()?;

        let local_db = Database::memory()?;
        {
            let conn = crate::database::lock_conn!(local_db.conn);
            conn.execute(
                "INSERT INTO providers (id, app_type, name, settings_config, meta)
                 VALUES ('local-provider', 'claude', 'Local Provider', '{}', '{}')",
                [],
            )?;
            conn.execute(
                "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model,
                    input_tokens, output_tokens, total_cost_usd,
                    latency_ms, status_code, created_at
                ) VALUES ('req-1', 'local-provider', 'claude', 'claude-3', 100, 50, '0.01', 120, 200, 1000)",
                [],
            )?;
            conn.execute(
                "INSERT INTO usage_daily_rollups (
                    date, app_type, provider_id, model, request_count, success_count,
                    input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                    total_cost_usd, avg_latency_ms
                ) VALUES ('2026-03-01', 'claude', 'local-provider', 'claude-3', 7, 7, 700, 350, 0, 0, '0.07', 120)",
                [],
            )?;
            conn.execute(
                "INSERT INTO stream_check_logs (
                    provider_id, provider_name, app_type, status, success, message,
                    response_time_ms, http_status, model_used, retry_count, tested_at
                ) VALUES ('local-provider', 'Local Provider', 'claude', 'operational', 1, 'ok', 42, 200, 'claude-3', 0, 1000)",
                [],
            )?;
        }

        local_db.import_sql_string_for_sync(&remote_sql)?;

        let remote_provider_exists: i64 = {
            let conn = crate::database::lock_conn!(local_db.conn);
            conn.query_row(
                "SELECT COUNT(*) FROM providers WHERE id = 'remote-provider' AND app_type = 'claude'",
                [],
                |row| row.get(0),
            )?
        };
        assert_eq!(
            remote_provider_exists, 1,
            "remote config should be imported"
        );

        let (request_logs, rollups, stream_logs): (i64, i64, i64) = {
            let conn = crate::database::lock_conn!(local_db.conn);
            let request_logs =
                conn.query_row("SELECT COUNT(*) FROM proxy_request_logs", [], |row| {
                    row.get(0)
                })?;
            let rollups =
                conn.query_row("SELECT COUNT(*) FROM usage_daily_rollups", [], |row| {
                    row.get(0)
                })?;
            let stream_logs =
                conn.query_row("SELECT COUNT(*) FROM stream_check_logs", [], |row| {
                    row.get(0)
                })?;
            (request_logs, rollups, stream_logs)
        };
        assert_eq!(request_logs, 1, "local request logs should be preserved");
        assert_eq!(rollups, 1, "local rollups should be preserved");
        assert_eq!(
            stream_logs, 1,
            "local stream check logs should be preserved"
        );

        Ok(())
    }

    #[test]
    fn cloud_restore_preserves_existing_local_credentials_by_default() -> Result<(), AppError> {
        use crate::app_config::AppType;
        use crate::services::provider_security::extract_provider_credentials;

        let remote_settings = serde_json::json!({
            "env": {
                "ANTHROPIC_AUTH_TOKEN": "sk-remote",
                "ANTHROPIC_BASE_URL": "https://remote.example"
            }
        });
        let local_settings = serde_json::json!({
            "env": {
                "ANTHROPIC_AUTH_TOKEN": "sk-local",
                "ANTHROPIC_BASE_URL": "https://local.example"
            }
        });

        let remote_db = Database::memory()?;
        {
            let conn = crate::database::lock_conn!(remote_db.conn);
            conn.execute(
                "INSERT INTO providers (id, app_type, name, settings_config, meta)
                 VALUES ('p1', 'claude', 'Remote Renamed Provider', ?1, '{}')",
                rusqlite::params![remote_settings.to_string()],
            )?;
        }
        let remote_sql = remote_db.export_sql_string_for_sync()?;

        let local_db = Database::memory()?;
        {
            let conn = crate::database::lock_conn!(local_db.conn);
            conn.execute(
                "INSERT INTO providers (id, app_type, name, settings_config, meta)
                 VALUES ('p1', 'claude', 'Local Provider', ?1, '{}')",
                rusqlite::params![local_settings.to_string()],
            )?;
        }

        local_db.import_sql_string_for_sync(&remote_sql)?;

        let provider = local_db
            .get_provider_by_id("p1", "claude")?
            .expect("provider should exist after cloud restore");
        assert_eq!(
            provider.name, "Remote Renamed Provider",
            "non-credential remote fields should be accepted"
        );
        let creds = extract_provider_credentials(&provider, &AppType::Claude);
        assert_eq!(
            creds.api_key.as_deref(),
            Some("sk-local"),
            "local api key must be preserved by default"
        );
        assert_eq!(
            creds.base_url.as_deref(),
            Some("https://local.example"),
            "local base url must be preserved by default"
        );
        Ok(())
    }

    #[test]
    fn failed_credential_merge_does_not_replace_local_database() -> Result<(), AppError> {
        use crate::app_config::AppType;
        use crate::services::provider_security::extract_provider_credentials;

        let remote_settings = serde_json::json!({
            "env": {
                "ANTHROPIC_AUTH_TOKEN": "sk-remote",
                "ANTHROPIC_BASE_URL": "https://remote.example"
            }
        });
        let remote_db = Database::memory()?;
        {
            let conn = crate::database::lock_conn!(remote_db.conn);
            conn.execute(
                "INSERT INTO providers (id, app_type, name, settings_config, meta)
                 VALUES ('p1', 'claude', 'Remote Provider', ?1, '{}')",
                rusqlite::params![remote_settings.to_string()],
            )?;
            conn.execute_batch(
                "CREATE TRIGGER reject_provider_merge
                 BEFORE UPDATE ON providers
                 BEGIN
                     SELECT RAISE(ABORT, 'reject provider merge');
                 END;",
            )?;
        }
        let remote_sql = remote_db.export_sql_string_for_sync()?;

        let local_settings = serde_json::json!({
            "env": {
                "ANTHROPIC_AUTH_TOKEN": "sk-local",
                "ANTHROPIC_BASE_URL": "https://local.example"
            }
        });
        let local_db = Database::memory()?;
        {
            let conn = crate::database::lock_conn!(local_db.conn);
            conn.execute(
                "INSERT INTO providers (id, app_type, name, settings_config, meta)
                 VALUES ('p1', 'claude', 'Local Provider', ?1, '{}')",
                rusqlite::params![local_settings.to_string()],
            )?;
        }

        local_db
            .import_sql_string_for_sync(&remote_sql)
            .expect_err("credential merge should fail");

        let provider = local_db
            .get_provider_by_id("p1", "claude")?
            .expect("local provider must remain after failed import");
        assert_eq!(provider.name, "Local Provider");
        let credentials = extract_provider_credentials(&provider, &AppType::Claude);
        assert_eq!(credentials.api_key.as_deref(), Some("sk-local"));
        assert_eq!(
            credentials.base_url.as_deref(),
            Some("https://local.example")
        );
        Ok(())
    }

    #[test]
    #[serial]
    fn cloud_restore_uses_remote_credentials_when_explicitly_selected() -> Result<(), AppError> {
        use crate::app_config::AppType;
        use crate::services::provider_security::extract_provider_credentials;

        let local_db = Database::memory()?;
        let local_settings = serde_json::json!({
            "env": {
                "ANTHROPIC_AUTH_TOKEN": "sk-local",
                "ANTHROPIC_BASE_URL": "https://local.example"
            }
        });
        {
            let conn = crate::database::lock_conn!(local_db.conn);
            conn.execute(
                "INSERT INTO providers (id, app_type, name, settings_config, meta)
                 VALUES ('p1', 'claude', 'Local Provider', ?1, '{}')",
                rusqlite::params![local_settings.to_string()],
            )?;
        }

        let remote_db = Database::memory()?;
        let remote_settings = serde_json::json!({
            "env": {
                "ANTHROPIC_AUTH_TOKEN": "sk-remote",
                "ANTHROPIC_BASE_URL": "https://remote.example"
            }
        });
        {
            let conn = crate::database::lock_conn!(remote_db.conn);
            conn.execute(
                "INSERT INTO providers (id, app_type, name, settings_config, meta)
                 VALUES ('p1', 'claude', 'Remote Renamed Provider', ?1, '{}')",
                rusqlite::params![remote_settings.to_string()],
            )?;
        }
        let remote_sql = remote_db.export_sql_string_for_sync()?;

        local_db.import_sql_string_for_sync_with_selections(
            &remote_sql,
            &[RemoteCredentialSelection {
                app_type: "claude".to_string(),
                provider_id: "p1".to_string(),
                use_remote_api_key: true,
                use_remote_base_url: false,
            }],
        )?;

        let provider = local_db
            .get_provider_by_id("p1", "claude")?
            .expect("provider should exist after cloud restore");
        assert_eq!(provider.name, "Remote Renamed Provider");
        let creds = extract_provider_credentials(&provider, &AppType::Claude);
        assert_eq!(
            creds.api_key.as_deref(),
            Some("sk-remote"),
            "explicit remote api key selection must win"
        );
        assert_eq!(
            creds.base_url.as_deref(),
            Some("https://local.example"),
            "unselected base url must stay local"
        );
        Ok(())
    }

    #[test]
    #[serial]
    fn exact_restore_preview_counts_credential_changes_without_applying() -> Result<(), AppError> {
        use crate::app_config::AppType;
        use crate::services::provider_security::extract_provider_credentials;

        let local_db = Database::memory()?;
        let local_settings = serde_json::json!({
            "env": {
                "ANTHROPIC_AUTH_TOKEN": "sk-local",
                "ANTHROPIC_BASE_URL": "https://local.example"
            }
        });
        {
            let conn = crate::database::lock_conn!(local_db.conn);
            conn.execute(
                "INSERT INTO providers (id, app_type, name, settings_config, meta)
                 VALUES ('p1', 'claude', 'Local Provider', ?1, '{}')",
                rusqlite::params![local_settings.to_string()],
            )?;
        }

        let remote_db = Database::memory()?;
        let remote_settings = serde_json::json!({
            "env": {
                "ANTHROPIC_AUTH_TOKEN": "sk-remote",
                "ANTHROPIC_BASE_URL": "https://remote.example"
            }
        });
        {
            let conn = crate::database::lock_conn!(remote_db.conn);
            conn.execute(
                "INSERT INTO providers (id, app_type, name, settings_config, meta)
                 VALUES ('p1', 'claude', 'Remote Renamed Provider', ?1, '{}')",
                rusqlite::params![remote_settings.to_string()],
            )?;
            conn.execute(
                "INSERT INTO providers (id, app_type, name, settings_config, meta)
                 VALUES ('remote-only', 'claude', 'Remote Only', '{}', '{}')",
                [],
            )?;
        }
        let remote_sql = remote_db.export_sql_string()?;

        let preview = local_db.preview_exact_restore(&remote_sql)?;
        assert_eq!(
            preview.new_provider_count, 1,
            "remote-only should count as new"
        );
        assert_eq!(
            preview.existing_provider_count, 1,
            "p1 should count as existing"
        );
        assert_eq!(
            preview.exact_restore_credential_field_count, 2,
            "apiKey + baseUrl should both count as changes"
        );
        assert!(
            !preview.preview_id.is_empty(),
            "preview must carry a non-empty id"
        );

        // Preview must not mutate local credentials.
        let provider = local_db
            .get_provider_by_id("p1", "claude")?
            .expect("local provider must still exist");
        assert_eq!(provider.name, "Local Provider");
        let creds = extract_provider_credentials(&provider, &AppType::Claude);
        assert_eq!(creds.api_key.as_deref(), Some("sk-local"));
        assert_eq!(creds.base_url.as_deref(), Some("https://local.example"));

        // And remote-only must not have been imported by preview.
        assert!(
            local_db
                .get_provider_by_id("remote-only", "claude")?
                .is_none(),
            "preview must not apply remote providers"
        );
        Ok(())
    }

    #[test]
    #[serial]
    fn periodic_maintenance_runs_even_when_auto_backup_disabled() -> Result<(), AppError> {
        let old_test_home = std::env::var_os("CC_SWITCH_TEST_HOME");
        let test_home =
            std::env::temp_dir().join("cc-switch-periodic-maintenance-backup-disabled-test");
        let _ = std::fs::remove_dir_all(&test_home);
        std::fs::create_dir_all(&test_home).expect("create test home");
        std::env::set_var("CC_SWITCH_TEST_HOME", &test_home);

        let settings = AppSettings {
            backup_interval_hours: Some(0),
            ..AppSettings::default()
        };
        update_settings(settings).expect("disable auto backup");

        let db = Database::memory()?;
        let now = chrono::Utc::now().timestamp();
        let old_ts = now - 40 * 86400;
        let old_stream_ts = now - 8 * 86400;

        {
            let conn = crate::database::lock_conn!(db.conn);
            conn.execute(
                "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model,
                    input_tokens, output_tokens, total_cost_usd,
                    latency_ms, status_code, created_at
                ) VALUES ('old-req', 'p1', 'claude', 'claude-3', 100, 50, '0.01', 100, 200, ?1)",
                [old_ts],
            )?;
            conn.execute(
                "INSERT INTO stream_check_logs (
                    provider_id, provider_name, app_type, status, success, message,
                    response_time_ms, http_status, model_used, retry_count, tested_at
                ) VALUES ('p1', 'Provider 1', 'claude', 'operational', 1, 'ok', 42, 200, 'claude-3', 0, ?1)",
                [old_stream_ts],
            )?;
        }

        db.periodic_backup_if_needed()?;

        let (remaining_request_logs, stream_logs, rollups): (i64, i64, i64) = {
            let conn = crate::database::lock_conn!(db.conn);
            let remaining_request_logs =
                conn.query_row("SELECT COUNT(*) FROM proxy_request_logs", [], |row| {
                    row.get(0)
                })?;
            let stream_logs =
                conn.query_row("SELECT COUNT(*) FROM stream_check_logs", [], |row| {
                    row.get(0)
                })?;
            let rollups =
                conn.query_row("SELECT COUNT(*) FROM usage_daily_rollups", [], |row| {
                    row.get(0)
                })?;
            (remaining_request_logs, stream_logs, rollups)
        };

        assert_eq!(
            remaining_request_logs, 0,
            "old request logs should still be pruned when auto backup is disabled"
        );
        assert_eq!(
            stream_logs, 0,
            "old stream check logs should still be pruned when auto backup is disabled"
        );
        assert_eq!(rollups, 1, "old request logs should be rolled up");

        match old_test_home {
            Some(value) => std::env::set_var("CC_SWITCH_TEST_HOME", value),
            None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
        }

        Ok(())
    }
}
