//! 数据库备份和恢复
//!
//! 提供 SQL 导出/导入和二进制快照备份功能。

use super::{lock_conn, Database};
use crate::config::get_app_config_dir;
use crate::error::AppError;
use chrono::{Local, Utc};
use rusqlite::backup::Backup;
use rusqlite::types::ValueRef;
use rusqlite::{params, Connection, OptionalExtension};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::time::Duration;
use tempfile::NamedTempFile;

const CC_SWITCH_SQL_EXPORT_HEADER: &str = "-- CC Switch SQLite 导出";

/// Bound combined INSERT batches while still amortizing statement parsing.
/// A row larger than this cap is emitted alone because it cannot be split.
const INSERT_BATCH_MAX_ROWS: usize = 200;
const INSERT_BATCH_MAX_BYTES: usize = 1024 * 1024;

/// `dump_sql` 会写出的 PRAGMA。其余 PRAGMA 一律拒绝——`temp_store_directory`
/// 能把临时文件重定向到任意目录，`writable_schema` 能绕过 schema 完整性检查。
const IMPORT_ALLOWED_PRAGMAS: &[&str] = &["foreign_keys", "user_version"];

/// 执行外部 SQL 期间的 authorizer：拒绝一切能**离开临时数据库文件**的动作。
///
/// 头部校验（`validate_cc_switch_sql_export`）只比较一个注释前缀，任何人都能在
/// 合法前缀后面接着写别的语句。`ATTACH DATABASE '/path/x.db'` 的副作用发生在
/// `validate_basic_state` 之前，导入即使最终失败，文件也已经被创建；而 `settings`
/// 表不在 `SYNC_SKIP_TABLES` / `SYNC_PRESERVE_TABLES` 之列，WebDAV/S3 同步会走
/// 同一条 `import_sql_string_inner`，所以这条路径的输入不可信。
///
/// 为什么是 authorizer 而不是「扫描 ATTACH 关键字」：字符串扫描会被 `/*x*/ATTACH`、
/// 大小写、换行绕过，还漏掉 `VACUUM INTO`。authorizer 在 prepare 阶段按**解析结果**
/// 回调，绕不过语法层。
///
/// 为什么是「拒绝越界动作」而不是「只放行 dump_sql 的语句」：这段 SQL 跑在
/// `NamedTempFile` 建的一次性库上，而那个库的全部内容本来就由这份 SQL 决定。
/// 因此 `DELETE` / `DROP` / `UPDATE` 给不了攻击者任何新东西——**唯一有意义的边界
/// 是那个临时文件本身**。按 dump_sql 的产物做严格白名单只会带来误伤风险（用户
/// 库里出现一种没预料到的对象就恢复不了备份），却不多挡任何攻击。
///
/// 越界动作是实测出来的，不是推断的：
/// - `ATTACH DATABASE 'x'`、`VACUUM INTO 'x'`、裸 `VACUUM` **三者都**报
///   `AuthAction::Attach`，所以拒 `Attach` 一条即可覆盖
/// - 文件后端的虚拟表模块（`csvfile`、`zipfile` 等）能读写任意路径 → 拒 vtable
/// - `Unknown` 是 rusqlite 对未识别动作码的兜底 → 未知即拒，将来 SQLite 新增的
///   跨文件语句会默认落进这里，不依赖有人记得回来补名单
fn import_authorizer(context: rusqlite::hooks::AuthContext<'_>) -> rusqlite::hooks::Authorization {
    use rusqlite::hooks::{AuthAction, Authorization};

    let escapes_temp_db = match context.action {
        AuthAction::Attach { .. } | AuthAction::Detach { .. } => true,
        AuthAction::CreateVtable { .. } | AuthAction::DropVtable { .. } => true,
        AuthAction::Unknown { .. } => true,
        AuthAction::Pragma { pragma_name, .. } => !IMPORT_ALLOWED_PRAGMAS
            .iter()
            .any(|allowed| pragma_name.eq_ignore_ascii_case(allowed)),
        _ => false,
    };

    if escapes_temp_db {
        // SQLite 只会回一句 "not authorized"，不记日志就无从知道是哪条语句被拦。
        log::warn!("SQL 导入拒绝了越界语句: {:?}", context.action);
        Authorization::Deny
    } else {
        Authorization::Allow
    }
}

/// Tables whose data rows are skipped when exporting for WebDAV sync.
const SYNC_SKIP_TABLES: &[&str] = &[
    "proxy_request_logs",
    "stream_check_logs",
    "provider_health",
    "proxy_live_backup",
    "usage_daily_rollups",
];

/// Tables whose local data is preserved (restored from local snapshot) during WebDAV import.
/// Excludes ephemeral tables like provider_health that can safely rebuild at runtime.
const SYNC_PRESERVE_TABLES: &[&str] = &[
    "proxy_request_logs",
    "stream_check_logs",
    "proxy_live_backup",
    "usage_daily_rollups",
];

/// A database backup entry for the UI
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupEntry {
    pub filename: String,
    pub size_bytes: u64,
    pub created_at: String, // ISO 8601
}

/// Category-level restore impact. Values and keys are intentionally omitted so
/// previewing an untrusted backup cannot expose credentials or provider details.
#[derive(Debug, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RestoreImpactPreview {
    pub providers_changed: bool,
    pub common_config_changed: bool,
    pub proxy_config_changed: bool,
    pub other_data_changed: bool,
    pub current_provider_count: u64,
    pub backup_provider_count: u64,
    pub restore_token: String,
}

#[derive(Debug, Default)]
struct LocalRestorePreferences {
    common_config: BTreeMap<String, Option<String>>,
    common_config_cleared: BTreeMap<String, Option<String>>,
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
        self.import_sql_string_inner(sql_raw, &[])
    }

    /// Import SQL generated for sync, then restore local-only tables from the
    /// current device snapshot before replacing the main database.
    pub(crate) fn import_sql_string_for_sync(&self, sql_raw: &str) -> Result<String, AppError> {
        self.import_sql_string_inner(sql_raw, SYNC_PRESERVE_TABLES)
    }

    fn import_sql_string_inner(
        &self,
        sql_raw: &str,
        preserve_tables: &[&str],
    ) -> Result<String, AppError> {
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
        let temp_conn =
            Connection::open(&temp_path).map_err(|e| AppError::Database(e.to_string()))?;

        // authorizer 只覆盖外部 SQL，执行完立刻摘掉：紧随其后的
        // `create_tables_on_conn` / `apply_schema_migrations_on_conn` 是本程序自己的
        // schema 维护语句，不属于需要设防的输入，没必要让它们也过一遍守卫。
        temp_conn.authorizer(Some(import_authorizer));
        let batch_result = temp_conn.execute_batch(sql_content);
        temp_conn.authorizer(
            None::<fn(rusqlite::hooks::AuthContext<'_>) -> rusqlite::hooks::Authorization>,
        );
        batch_result.map_err(|e| AppError::Database(format!("执行 SQL 导入失败: {e}")))?;

        // 补齐缺失表/索引并进行基础校验
        Self::create_tables_on_conn(&temp_conn)?;
        Self::apply_schema_migrations_on_conn(&temp_conn)?;
        Self::validate_basic_state(&temp_conn)?;
        if let Some(local_snapshot) = local_snapshot.as_ref() {
            Self::restore_tables(local_snapshot, &temp_conn, preserve_tables)?;
        }

        // 使用 Backup 将临时库原子写回主库
        {
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
        // 整批复原放进一个事务：旧实现每行一条隐式自动提交的 INSERT，
        // 目标是磁盘上的暂存库，等于每行一次 fsync——2.6 万行实测 119 秒。
        // 合并成单事务后只剩最后一次提交；中途失败整体回滚，
        // 也不会留下“半张表”的中间状态。
        let tx = target_conn
            .unchecked_transaction()
            .map_err(|e| AppError::Database(format!("开启恢复事务失败: {e}")))?;

        for table in tables {
            if !Self::table_exists(source_conn, table)? || !Self::table_exists(&tx, table)? {
                continue;
            }

            let columns = Self::get_table_columns(source_conn, table)?;
            if columns.is_empty() {
                continue;
            }

            let quoted_table = Self::quote_identifier(table);
            let quoted_columns = columns
                .iter()
                .map(|column| Self::quote_identifier(column))
                .collect::<Vec<_>>()
                .join(", ");

            tx.execute(&format!("DELETE FROM {quoted_table}"), [])
                .map_err(|e| AppError::Database(format!("清空表 {table} 失败: {e}")))?;

            let placeholders = (1..=columns.len())
                .map(|idx| format!("?{idx}"))
                .collect::<Vec<_>>()
                .join(", ");
            let insert_sql =
                format!("INSERT INTO {quoted_table} ({quoted_columns}) VALUES ({placeholders})");

            // INSERT 语句每表只 prepare 一次，不再逐行重复解析。
            let mut insert_stmt = tx
                .prepare(&insert_sql)
                .map_err(|e| AppError::Database(format!("准备表 {table} 插入语句失败: {e}")))?;

            let mut stmt = source_conn
                .prepare(&format!("SELECT {quoted_columns} FROM {quoted_table}"))
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

                insert_stmt
                    .execute(rusqlite::params_from_iter(values.iter()))
                    .map_err(|e| AppError::Database(format!("恢复表 {table} 数据失败: {e}")))?;
            }
        }

        tx.commit()
            .map_err(|e| AppError::Database(format!("提交恢复事务失败: {e}")))?;
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

    fn copy_database(source: &Connection, destination: &mut Connection) -> Result<(), AppError> {
        let backup =
            Backup::new(source, destination).map_err(|e| AppError::Database(e.to_string()))?;
        backup
            .run_to_completion(100, Duration::from_millis(10), None)
            .map_err(|e| AppError::Database(e.to_string()))
    }

    fn unique_backup_path(backup_dir: &Path) -> PathBuf {
        let base_id = format!("db_backup_{}", Local::now().format("%Y%m%d_%H%M%S"));
        for counter in 0.. {
            let backup_id = if counter == 0 {
                base_id.clone()
            } else {
                format!("{base_id}_{counter}")
            };
            let backup_path = backup_dir.join(format!("{backup_id}.db"));
            if !backup_path.exists() {
                return backup_path;
            }
        }
        unreachable!("backup filename counter is unbounded")
    }

    fn persist_backup(source: &Connection, backup_dir: &Path) -> Result<PathBuf, AppError> {
        let target_path = Self::unique_backup_path(backup_dir);
        let temp_file = NamedTempFile::new_in(backup_dir).map_err(|e| AppError::IoContext {
            context: "创建临时备份文件失败".to_string(),
            source: e,
        })?;
        let mut temp_conn =
            Connection::open(temp_file.path()).map_err(|e| AppError::Database(e.to_string()))?;
        Self::copy_database(source, &mut temp_conn)?;
        drop(temp_conn);
        temp_file
            .persist_noclobber(&target_path)
            .map_err(|e| AppError::io(&target_path, e.error))?;
        Ok(target_path)
    }

    /// 生成一致性快照备份，返回备份文件路径（不存在主库时返回 None）
    pub(crate) fn backup_database_file(&self) -> Result<Option<PathBuf>, AppError> {
        let db_path = get_app_config_dir().join("cc-switch.db");
        if !db_path.exists() {
            return Ok(None);
        }

        let backup_dir = Self::backup_directory()?;
        let backup_path = {
            let conn = lock_conn!(self.conn);
            Self::persist_backup(&conn, &backup_dir)?
        };
        Self::cleanup_db_backups(&backup_dir, None)?;
        Ok(Some(backup_path))
    }

    fn backup_directory() -> Result<PathBuf, AppError> {
        let db_path = get_app_config_dir().join("cc-switch.db");
        let backup_dir = db_path
            .parent()
            .ok_or_else(|| AppError::Config("无效的数据库路径".to_string()))?
            .join("backups");
        fs::create_dir_all(&backup_dir).map_err(|e| AppError::io(&backup_dir, e))?;
        Ok(backup_dir)
    }

    /// 清理旧的数据库备份，保留最新的 N 个。
    /// `protected_path` 用于确保刚创建的恢复安全备份不会被同一轮清理删除。
    fn cleanup_db_backups(dir: &Path, protected_path: Option<&Path>) -> Result<(), AppError> {
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

        let mut removed = 0;
        for entry in sorted {
            if removed >= remove_count {
                break;
            }
            if protected_path.is_some_and(|protected| entry.path() == protected) {
                continue;
            }
            if let Err(err) = fs::remove_file(entry.path()) {
                log::warn!("删除旧数据库备份失败 {}: {}", entry.path().display(), err);
            } else {
                removed += 1;
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
        let mut triggers = Vec::new();
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

            if obj_type == "trigger" {
                triggers.push(sql);
                continue;
            }

            output.push_str(&sql);
            output.push_str(";\n");
            if obj_type == "table" {
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

            // 每行一条 INSERT 是导入慢的根源：恢复侧要为每条语句单独
            // 解析/准备/收尾，2 万行实测 21 秒（内存库上一样慢，说明是
            // 纯 CPU 而非 I/O）。合并成多行 VALUES 后同样数据 <100ms。
            // SQLite 从 3.7.11（2012）起支持多行 VALUES，且导入侧是通用
            // execute_batch，新旧两种格式都能读——向后兼容无忧。
            let quoted_table = Self::quote_identifier(&table);
            let quoted_columns = columns
                .iter()
                .map(|column| Self::quote_identifier(column))
                .collect::<Vec<_>>()
                .join(", ");
            let insert_prefix = format!("INSERT INTO {quoted_table} ({quoted_columns}) VALUES ");

            let mut stmt = conn
                .prepare(&format!("SELECT {quoted_columns} FROM {quoted_table}"))
                .map_err(|e| AppError::Database(e.to_string()))?;
            let mut rows = stmt
                .query([])
                .map_err(|e| AppError::Database(e.to_string()))?;

            let mut pending_rows = 0usize;
            let mut batch = String::new();
            while let Some(row) = rows.next().map_err(|e| AppError::Database(e.to_string()))? {
                let mut values = Vec::with_capacity(columns.len());
                for idx in 0..columns.len() {
                    let value = row
                        .get_ref(idx)
                        .map_err(|e| AppError::Database(e.to_string()))?;
                    values.push(Self::format_sql_value(value)?);
                }

                let row_sql = format!("({})", values.join(", "));
                let separator_bytes = usize::from(pending_rows > 0);
                if pending_rows > 0
                    && batch.len() + separator_bytes + row_sql.len() + 2 > INSERT_BATCH_MAX_BYTES
                {
                    batch.push_str(";\n");
                    output.push_str(&batch);
                    pending_rows = 0;
                }

                if pending_rows == 0 {
                    batch.clear();
                    batch.push_str(&insert_prefix);
                } else {
                    batch.push(',');
                }
                batch.push_str(&row_sql);
                pending_rows += 1;

                if pending_rows >= INSERT_BATCH_MAX_ROWS {
                    batch.push_str(";\n");
                    output.push_str(&batch);
                    pending_rows = 0;
                }
            }
            if pending_rows > 0 {
                batch.push_str(";\n");
                output.push_str(&batch);
            }
        }

        // Triggers must be created after loading table data so they cannot
        // change dump rows or abandon the remainder of a multi-row INSERT.
        for sql in triggers {
            output.push_str(&sql);
            output.push_str(";\n");
        }

        output.push_str("COMMIT;\nPRAGMA foreign_keys=ON;\n");
        Ok(output)
    }

    fn quote_identifier(identifier: &str) -> String {
        format!("\"{}\"", identifier.replace('"', "\"\""))
    }

    /// 获取表的列名列表
    fn get_table_columns(conn: &Connection, table: &str) -> Result<Vec<String>, AppError> {
        let quoted_table = Self::quote_identifier(table);
        let mut stmt = conn
            .prepare(&format!("PRAGMA table_info({quoted_table})"))
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

    fn validated_backup_path(filename: &str) -> Result<PathBuf, AppError> {
        let path = Path::new(filename);
        if path.components().count() != 1
            || !matches!(path.components().next(), Some(Component::Normal(_)))
            || !filename.ends_with(".db")
            || (cfg!(windows) && filename.contains(':'))
        {
            return Err(AppError::InvalidInput(
                "Invalid backup filename".to_string(),
            ));
        }

        let backup_path = get_app_config_dir().join("backups").join(path);
        if !backup_path.exists() {
            return Err(AppError::InvalidInput(format!(
                "Backup file not found: {filename}"
            )));
        }
        Ok(backup_path)
    }

    fn connection_digest(conn: &Connection) -> Result<String, AppError> {
        let tables = Self::table_names(conn)?;
        let mut hasher = Sha256::new();
        let mut schema_stmt = conn
            .prepare(
                "SELECT type, name, tbl_name, COALESCE(sql, '')
                 FROM sqlite_master
                 WHERE name NOT LIKE 'sqlite_%'
                 ORDER BY type, name",
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
        let schema_rows = schema_stmt
            .query_map([], |row| {
                Ok(format!(
                    "{}\u{0}{}\u{0}{}\u{0}{}",
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?
                ))
            })
            .map_err(|e| AppError::Database(e.to_string()))?;
        for schema in schema_rows {
            hasher.update(
                schema
                    .map_err(|e| AppError::Database(e.to_string()))?
                    .as_bytes(),
            );
            hasher.update([0]);
        }
        for table in tables {
            hasher.update(table.as_bytes());
            hasher.update([0]);
            for row in Self::table_fingerprint_excluding(conn, &table, &[])? {
                hasher.update(row.as_bytes());
                hasher.update([0]);
            }
        }
        let user_version: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .map_err(|e| AppError::Database(e.to_string()))?;
        hasher.update(user_version.to_le_bytes());
        Ok(format!("{:x}", hasher.finalize()))
    }

    fn table_fingerprint_excluding(
        conn: &Connection,
        table: &str,
        excluded_columns: &[&str],
    ) -> Result<Vec<String>, AppError> {
        if !Self::table_exists(conn, table)? {
            return Ok(Vec::new());
        }

        let columns = {
            let mut stmt = conn
                .prepare(&format!("PRAGMA table_info({table})"))
                .map_err(|e| AppError::Database(e.to_string()))?;
            let rows = stmt
                .query_map([], |row| row.get::<_, String>(1))
                .map_err(|e| AppError::Database(e.to_string()))?;
            rows.collect::<Result<Vec<_>, _>>()
                .map_err(|e| AppError::Database(e.to_string()))?
                .into_iter()
                .filter(|column| !excluded_columns.contains(&column.as_str()))
                .collect::<Vec<_>>()
        };
        if columns.is_empty() {
            return Ok(Vec::new());
        }

        let quoted_columns = columns
            .iter()
            .map(|column| format!("\"{}\"", column.replace('"', "\"\"")))
            .collect::<Vec<_>>()
            .join(", ");
        let mut stmt = conn
            .prepare(&format!("SELECT {quoted_columns} FROM \"{table}\""))
            .map_err(|e| AppError::Database(e.to_string()))?;
        let rows = stmt
            .query_map([], |row| {
                let encode_bytes = |value: &[u8]| {
                    value
                        .iter()
                        .map(|byte| format!("{byte:02x}"))
                        .collect::<String>()
                };
                let values = (0..columns.len())
                    .map(|index| match row.get_ref(index)? {
                        ValueRef::Null => Ok("null".to_string()),
                        ValueRef::Integer(value) => Ok(format!("i:{value}")),
                        ValueRef::Real(value) => Ok(format!("r:{value}")),
                        ValueRef::Text(value) => Ok(format!("t:{}", encode_bytes(value))),
                        ValueRef::Blob(value) => Ok(format!("b:{}", encode_bytes(value))),
                    })
                    .collect::<Result<Vec<_>, rusqlite::Error>>()?;
                Ok(values.join("|"))
            })
            .map_err(|e| AppError::Database(e.to_string()))?;
        let mut result = rows
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| AppError::Database(e.to_string()))?;
        result.sort();
        Ok(result)
    }

    fn common_config_fingerprint(conn: &Connection) -> Result<Vec<String>, AppError> {
        if !Self::table_exists(conn, "settings")? {
            return Ok(Vec::new());
        }
        let keys = crate::app_config::AppType::all()
            .flat_map(|app| {
                let app_type = app.as_str();
                [
                    format!("common_config_{app_type}"),
                    format!("common_config_{app_type}_cleared"),
                ]
            })
            .collect::<Vec<_>>();
        let placeholders = std::iter::repeat_n("?", keys.len())
            .collect::<Vec<_>>()
            .join(", ");
        let mut stmt = conn
            .prepare(&format!(
                "SELECT key, value FROM settings
                 WHERE key IN ({placeholders}) ORDER BY key"
            ))
            .map_err(|e| AppError::Database(e.to_string()))?;
        let rows = stmt
            .query_map(rusqlite::params_from_iter(&keys), |row| {
                let key: String = row.get(0)?;
                let value: Option<String> = row.get(1)?;
                Ok(format!("{key}\u{0}{}", value.unwrap_or_default()))
            })
            .map_err(|e| AppError::Database(e.to_string()))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| AppError::Database(e.to_string()))
    }

    fn provider_count(conn: &Connection) -> Result<u64, AppError> {
        if !Self::table_exists(conn, "providers")? {
            return Ok(0);
        }
        conn.query_row("SELECT COUNT(*) FROM providers", [], |row| row.get(0))
            .map_err(|e| AppError::Database(e.to_string()))
    }

    fn table_names(conn: &Connection) -> Result<Vec<String>, AppError> {
        let mut stmt = conn
            .prepare(
                "SELECT name FROM sqlite_master
                 WHERE type = 'table' AND name NOT LIKE 'sqlite_%'
                 ORDER BY name",
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
        let rows = stmt
            .query_map([], |row| row.get(0))
            .map_err(|e| AppError::Database(e.to_string()))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| AppError::Database(e.to_string()))
    }

    fn validate_backup_database(conn: &Connection) -> Result<(), AppError> {
        let integrity: String = conn
            .query_row("PRAGMA integrity_check", [], |row| row.get(0))
            .map_err(|e| AppError::Database(e.to_string()))?;
        if integrity != "ok" {
            return Err(AppError::InvalidInput(
                "Backup database failed integrity check".to_string(),
            ));
        }
        if !Self::table_exists(conn, "providers")? || !Self::table_exists(conn, "settings")? {
            return Err(AppError::InvalidInput(
                "Backup is not a valid CC Switch database".to_string(),
            ));
        }
        Ok(())
    }

    /// Preview category-level restore impact without returning database values.
    pub fn preview_restore_from_backup(
        &self,
        filename: &str,
    ) -> Result<RestoreImpactPreview, AppError> {
        const CATEGORIZED_TABLES: &[&str] = &["providers", "provider_endpoints", "proxy_config"];

        let backup_path = Self::validated_backup_path(filename)?;
        let backup_conn =
            Connection::open_with_flags(&backup_path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
                .map_err(|e| AppError::Database(e.to_string()))?;
        Self::validate_backup_database(&backup_conn)?;
        let restore_token = Self::connection_digest(&backup_conn)?;

        // Run the same compatibility work as restore so the confirmation dialog
        // never enables an operation that is known to be incompatible.
        let preview_file = NamedTempFile::new().map_err(|e| AppError::IoContext {
            context: "创建预览数据库文件失败".to_string(),
            source: e,
        })?;
        let mut preview_conn =
            Connection::open(preview_file.path()).map_err(|e| AppError::Database(e.to_string()))?;
        Self::copy_database(&backup_conn, &mut preview_conn)?;
        Self::create_tables_on_conn(&preview_conn)?;
        Self::apply_schema_migrations_on_conn(&preview_conn)?;
        Self::ensure_model_pricing_seeded_on_conn(&preview_conn)?;

        let current_conn = lock_conn!(self.conn);
        let current_provider_count = Self::provider_count(&current_conn)?;
        let backup_provider_count = Self::provider_count(&preview_conn)?;
        let common_config_changed = Self::common_config_fingerprint(&current_conn)?
            != Self::common_config_fingerprint(&preview_conn)?;
        let mut current_for_compare =
            Connection::open_in_memory().map_err(|e| AppError::Database(e.to_string()))?;
        Self::copy_database(&current_conn, &mut current_for_compare)?;
        drop(current_conn);
        Self::preserve_local_proxy_runtime(&current_for_compare, &preview_conn)?;
        let local_preferences = Self::capture_local_restore_preferences(&current_for_compare)?;
        Self::restore_local_preferences(&mut preview_conn, local_preferences)?;

        let mut other_tables = Self::table_names(&current_for_compare)?;
        other_tables.extend(Self::table_names(&preview_conn)?);
        other_tables.sort();
        other_tables.dedup();
        other_tables.retain(|table| !CATEGORIZED_TABLES.contains(&table.as_str()));
        // Common config has its own category and is overlaid when preservation is
        // selected; do not double-report it through the catch-all settings table.
        current_for_compare.execute("DELETE FROM settings WHERE key LIKE 'common_config_%'", [])?;
        preview_conn.execute("DELETE FROM settings WHERE key LIKE 'common_config_%'", [])?;
        let other_data_changed = other_tables
            .iter()
            .map(|table| {
                Ok(
                    Self::table_fingerprint_excluding(&current_for_compare, table, &[])?
                        != Self::table_fingerprint_excluding(&preview_conn, table, &[])?,
                )
            })
            .collect::<Result<Vec<bool>, AppError>>()?
            .into_iter()
            .any(|changed| changed);
        Ok(RestoreImpactPreview {
            providers_changed: Self::table_fingerprint_excluding(
                &current_for_compare,
                "providers",
                &[],
            )? != Self::table_fingerprint_excluding(
                &preview_conn,
                "providers",
                &[],
            )? || Self::table_fingerprint_excluding(
                &current_for_compare,
                "provider_endpoints",
                &[],
            )? != Self::table_fingerprint_excluding(
                &preview_conn,
                "provider_endpoints",
                &[],
            )?,
            common_config_changed,
            proxy_config_changed: Self::table_fingerprint_excluding(
                &current_for_compare,
                "proxy_config",
                &[],
            )? != Self::table_fingerprint_excluding(
                &preview_conn,
                "proxy_config",
                &[],
            )?,
            other_data_changed,
            current_provider_count,
            backup_provider_count,
            restore_token,
        })
    }

    fn setting_value(conn: &Connection, key: &str) -> Result<Option<String>, AppError> {
        conn.query_row("SELECT value FROM settings WHERE key = ?1", [key], |row| {
            row.get::<_, Option<String>>(0)
        })
        .optional()
        .map(|value| value.flatten())
        .map_err(|e| AppError::Database(e.to_string()))
    }

    fn capture_local_restore_preferences(
        conn: &Connection,
    ) -> Result<LocalRestorePreferences, AppError> {
        let mut preferences = LocalRestorePreferences::default();
        if Self::table_exists(conn, "settings")? {
            for app_type in crate::app_config::AppType::all() {
                let app_type = app_type.as_str();
                let config_key = format!("common_config_{app_type}");
                let cleared_key = format!("common_config_{app_type}_cleared");
                preferences
                    .common_config
                    .insert(config_key.clone(), Self::setting_value(conn, &config_key)?);
                preferences.common_config_cleared.insert(
                    cleared_key.clone(),
                    Self::setting_value(conn, &cleared_key)?,
                );
            }
        }
        Ok(preferences)
    }

    fn restore_setting_values(
        tx: &rusqlite::Transaction<'_>,
        values: BTreeMap<String, Option<String>>,
    ) -> Result<(), AppError> {
        for (key, value) in values {
            match value {
                Some(value) => {
                    tx.execute(
                        "INSERT OR REPLACE INTO settings (key, value) VALUES (?1, ?2)",
                        params![key, value],
                    )
                    .map_err(|e| AppError::Database(e.to_string()))?;
                }
                None => {
                    tx.execute("DELETE FROM settings WHERE key = ?1", [key])
                        .map_err(|e| AppError::Database(e.to_string()))?;
                }
            }
        }
        Ok(())
    }

    fn restore_local_preferences(
        conn: &mut Connection,
        preferences: LocalRestorePreferences,
    ) -> Result<(), AppError> {
        let tx = conn
            .transaction()
            .map_err(|e| AppError::Database(e.to_string()))?;
        Self::restore_setting_values(&tx, preferences.common_config)?;
        Self::restore_setting_values(&tx, preferences.common_config_cleared)?;
        tx.commit().map_err(|e| AppError::Database(e.to_string()))
    }

    fn preserve_local_proxy_runtime(
        current: &Connection,
        staged: &Connection,
    ) -> Result<(), AppError> {
        Self::restore_tables(current, staged, &["proxy_live_backup"])?;
        if !Self::table_exists(current, "proxy_config")?
            || !Self::table_exists(staged, "proxy_config")?
        {
            return Ok(());
        }

        let mut stmt = current
            .prepare(
                "SELECT app_type, proxy_enabled, enabled, listen_address, listen_port
                 FROM proxy_config",
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, i64>(4)?,
                ))
            })
            .map_err(|e| AppError::Database(e.to_string()))?;
        for row in rows {
            let (app_type, proxy_enabled, enabled, listen_address, listen_port) =
                row.map_err(|e| AppError::Database(e.to_string()))?;
            staged
                .execute(
                    "UPDATE proxy_config
                     SET proxy_enabled = ?2, enabled = ?3,
                         listen_address = ?4, listen_port = ?5
                     WHERE app_type = ?1",
                    params![
                        app_type,
                        proxy_enabled,
                        enabled,
                        listen_address,
                        listen_port
                    ],
                )
                .map_err(|e| AppError::Database(e.to_string()))?;
        }
        Ok(())
    }

    /// Restore database from a backup file. Returns the safety backup ID.
    pub fn restore_from_backup(
        &self,
        filename: &str,
        restore_token: &str,
        preserve_local_preferences: bool,
    ) -> Result<String, AppError> {
        let backup_path = Self::validated_backup_path(filename)?;
        let source_conn =
            Connection::open_with_flags(&backup_path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
                .map_err(|e| AppError::Database(e.to_string()))?;
        Self::validate_backup_database(&source_conn)?;
        if Self::connection_digest(&source_conn)? != restore_token {
            return Err(AppError::InvalidInput(
                "Backup changed after preview. Preview it again before restoring.".to_string(),
            ));
        }

        // Prepare and migrate a temporary copy first. The active database is not
        // touched until every compatibility and preference step succeeds.
        let temp_file = NamedTempFile::new().map_err(|e| AppError::IoContext {
            context: "创建临时数据库文件失败".to_string(),
            source: e,
        })?;
        let mut staged_conn =
            Connection::open(temp_file.path()).map_err(|e| AppError::Database(e.to_string()))?;
        Self::copy_database(&source_conn, &mut staged_conn)?;
        Self::create_tables_on_conn(&staged_conn)?;
        Self::apply_schema_migrations_on_conn(&staged_conn)?;
        Self::ensure_model_pricing_seeded_on_conn(&staged_conn)?;

        // Re-capture device-local values under the same mutex used for the safety
        // snapshot and final copy, so a concurrent setting change cannot be lost.
        let backup_dir = Self::backup_directory()?;
        let mut main_conn = lock_conn!(self.conn);
        Self::preserve_local_proxy_runtime(&main_conn, &staged_conn)?;
        if preserve_local_preferences {
            let local_preferences = Self::capture_local_restore_preferences(&main_conn)?;
            Self::restore_local_preferences(&mut staged_conn, local_preferences)?;
        }
        // Force the canonical Gemini cleanup to run after restore. That routine
        // removes any historical credential copies from the snippet, providers,
        // and the preserved proxy Live backup together.
        staged_conn
            .execute(
                "DELETE FROM settings
                 WHERE key = 'gemini_common_config_credentials_scrubbed_v1'",
                [],
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
        let safety_path = Self::persist_backup(&main_conn, &backup_dir)?;
        let safety_id = safety_path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        Self::copy_database(&staged_conn, &mut main_conn)?;
        drop(main_conn);
        Self::cleanup_db_backups(&backup_dir, Some(&safety_path))?;

        log::info!(
            "Database restored from backup: {filename}, safety backup: {safety_id}, preserve local preferences: {preserve_local_preferences}"
        );
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
    use super::Database;
    use crate::error::AppError;
    use crate::settings::{update_settings, AppSettings};
    use rusqlite::Connection;
    use serial_test::serial;

    struct TestHomeGuard {
        previous_test_home: Option<std::ffi::OsString>,
        temp_dir: tempfile::TempDir,
    }

    impl TestHomeGuard {
        fn new() -> Self {
            let temp_dir = tempfile::tempdir().expect("create isolated test home");
            let previous_test_home = std::env::var_os("CC_SWITCH_TEST_HOME");
            std::env::set_var("CC_SWITCH_TEST_HOME", temp_dir.path());
            // Prevent the Windows legacy-HOME fallback without mutating HOME:
            // an existing default DB keeps get_app_config_dir() anchored under
            // CC_SWITCH_TEST_HOME and makes import exercise its safety backup.
            let config_dir = temp_dir.path().join(".cc-switch");
            std::fs::create_dir_all(&config_dir).expect("create isolated config directory");
            std::fs::File::create(config_dir.join("cc-switch.db"))
                .expect("create isolated database sentinel");
            let guard = Self {
                previous_test_home,
                temp_dir,
            };
            let resolved = crate::config::get_app_config_dir();
            assert!(
                resolved.starts_with(guard.temp_dir.path()),
                "isolated test home resolved outside its temp directory: {}",
                resolved.display()
            );
            guard
        }

        fn path(&self) -> &std::path::Path {
            self.temp_dir.path()
        }
    }

    impl Drop for TestHomeGuard {
        fn drop(&mut self) {
            match self.previous_test_home.as_ref() {
                Some(previous) => std::env::set_var("CC_SWITCH_TEST_HOME", previous),
                None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
            }
        }
    }

    fn write_backup_fixture(
        db: &Database,
        filename: &str,
        provider_name: &str,
        common_config: &str,
    ) -> Result<(), AppError> {
        {
            let conn = crate::database::lock_conn!(db.conn);
            conn.execute(
                "INSERT INTO providers
                 (id, app_type, name, settings_config, meta, is_current)
                 VALUES ('provider-1', 'claude', ?1, '{}', '{}', 1)",
                [provider_name],
            )?;
            conn.execute(
                "INSERT INTO settings (key, value) VALUES ('common_config_claude', ?1)",
                [common_config],
            )?;
        }
        let backup_dir = crate::config::get_app_config_dir().join("backups");
        std::fs::create_dir_all(&backup_dir).map_err(|e| AppError::io(&backup_dir, e))?;
        let mut destination = Connection::open(backup_dir.join(filename))?;
        let source = crate::database::lock_conn!(db.conn);
        let backup = rusqlite::backup::Backup::new(&source, &mut destination)?;
        backup.step(-1)?;
        Ok(())
    }

    #[test]
    #[serial]
    fn restore_preview_reports_categories_without_values() -> Result<(), AppError> {
        let _test_home = TestHomeGuard::new();
        let current = Database::memory()?;
        write_backup_fixture(&current, "preview.db", "Current", r#"{"model":"sonnet"}"#)?;
        {
            let conn = crate::database::lock_conn!(current.conn);
            conn.execute(
                "UPDATE providers SET name = 'Changed' WHERE id = 'provider-1'",
                [],
            )?;
            conn.execute(
                "UPDATE settings SET value = ?1 WHERE key = 'common_config_claude'",
                [r#"{"model":"opus"}"#],
            )?;
        }

        let preview = current.preview_restore_from_backup("preview.db")?;
        assert!(preview.providers_changed);
        assert!(preview.common_config_changed);
        assert!(!preview.restore_token.is_empty());
        assert_eq!(preview.current_provider_count, 1);
        assert_eq!(preview.backup_provider_count, 1);
        Ok(())
    }

    #[test]
    #[serial]
    fn restore_rejects_backup_changed_after_preview() -> Result<(), AppError> {
        let _test_home = TestHomeGuard::new();
        let db = Database::init()?;
        write_backup_fixture(&db, "changed.db", "Backup", r#"{"model":"opus"}"#)?;
        let backup_path = crate::config::get_app_config_dir()
            .join("backups")
            .join("changed.db");
        let preview = db.preview_restore_from_backup("changed.db")?;
        {
            let conn = Connection::open(&backup_path)?;
            conn.execute(
                "CREATE TRIGGER unpreviewed AFTER INSERT ON settings
                 BEGIN DELETE FROM providers; END",
                [],
            )?;
        }

        let result = db.restore_from_backup("changed.db", &preview.restore_token, false);
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    #[serial]
    fn restore_can_preserve_common_config_and_existing_current_provider() -> Result<(), AppError> {
        let _test_home = TestHomeGuard::new();
        let db = Database::init()?;
        write_backup_fixture(&db, "preserve.db", "Backup", r#"{"model":"opus"}"#)?;
        {
            let conn = crate::database::lock_conn!(db.conn);
            conn.execute(
                "UPDATE settings SET value = ?1 WHERE key = 'common_config_claude'",
                [r#"{"model":"sonnet"}"#],
            )?;
            conn.execute(
                "UPDATE providers SET name = 'Current' WHERE id = 'provider-1'",
                [],
            )?;
        }

        let restore_token = {
            let conn = Connection::open(
                crate::config::get_app_config_dir()
                    .join("backups")
                    .join("preserve.db"),
            )?;
            Database::connection_digest(&conn)?
        };
        db.restore_from_backup("preserve.db", &restore_token, true)?;
        let conn = crate::database::lock_conn!(db.conn);
        let common: String = conn.query_row(
            "SELECT value FROM settings WHERE key = 'common_config_claude'",
            [],
            |row| row.get(0),
        )?;
        let provider: (String, bool) = conn.query_row(
            "SELECT name, is_current FROM providers WHERE id = 'provider-1'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        assert_eq!(common, r#"{"model":"sonnet"}"#);
        assert_eq!(provider, ("Backup".to_string(), true));
        Ok(())
    }

    #[test]
    #[serial]
    fn full_restore_replaces_common_config() -> Result<(), AppError> {
        let _test_home = TestHomeGuard::new();
        let db = Database::init()?;
        write_backup_fixture(&db, "full.db", "Backup", r#"{"model":"opus"}"#)?;
        {
            let conn = crate::database::lock_conn!(db.conn);
            conn.execute(
                "UPDATE settings SET value = ?1 WHERE key = 'common_config_claude'",
                [r#"{"model":"sonnet"}"#],
            )?;
        }

        let restore_token = {
            let conn = Connection::open(
                crate::config::get_app_config_dir()
                    .join("backups")
                    .join("full.db"),
            )?;
            Database::connection_digest(&conn)?
        };
        db.restore_from_backup("full.db", &restore_token, false)?;
        let conn = crate::database::lock_conn!(db.conn);
        let common: String = conn.query_row(
            "SELECT value FROM settings WHERE key = 'common_config_claude'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(common, r#"{"model":"opus"}"#);
        Ok(())
    }

    #[test]
    #[serial]
    fn restore_preserves_local_proxy_runtime_state() -> Result<(), AppError> {
        let _test_home = TestHomeGuard::new();
        let db = Database::init()?;
        write_backup_fixture(&db, "proxy-local.db", "Backup", r#"{"model":"opus"}"#)?;
        {
            let conn = crate::database::lock_conn!(db.conn);
            conn.execute(
                "UPDATE proxy_config
                 SET proxy_enabled = 1, enabled = 1,
                     listen_address = '127.0.0.2', listen_port = 18080
                 WHERE app_type = 'claude'",
                [],
            )?;
            conn.execute(
                "INSERT OR REPLACE INTO proxy_live_backup
                 (app_type, original_config, backed_up_at)
                 VALUES ('claude', '{\"local\":true}', '2026-08-09')",
                [],
            )?;
        }

        let preview = db.preview_restore_from_backup("proxy-local.db")?;
        db.restore_from_backup("proxy-local.db", &preview.restore_token, false)?;
        let conn = crate::database::lock_conn!(db.conn);
        let runtime: (i64, i64, String, i64) = conn.query_row(
            "SELECT proxy_enabled, enabled, listen_address, listen_port
             FROM proxy_config WHERE app_type = 'claude'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )?;
        assert_eq!(runtime, (1, 1, "127.0.0.2".to_string(), 18080));
        let live_backup: String = conn.query_row(
            "SELECT original_config FROM proxy_live_backup WHERE app_type = 'claude'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(live_backup, r#"{"local":true}"#);
        Ok(())
    }

    #[test]
    #[serial]
    fn import_rejects_cross_file_statements_and_leaves_no_file_behind() -> Result<(), AppError> {
        let test_home = TestHomeGuard::new();
        // `VACUUM INTO` 是关键字扫描方案最容易漏的一条：它不含 "ATTACH" 字样，
        // 却和 ATTACH 一样落到 `AuthAction::Attach`（实测），因此同一条规则挡住两者。
        let cases: [(&str, &str); 2] = [
            ("attach", "ATTACH DATABASE '{path}' AS evil;"),
            ("vacuum-into", "VACUUM INTO '{path}';"),
        ];

        for (label, template) in cases {
            let target = test_home
                .path()
                .join(format!("cc-switch-authorizer-{label}.sqlite"));

            // 合法的导出头 + 越界语句。头部校验只比前缀，这份输入过得了它，
            // 真正拦下来的必须是 authorizer。
            let malicious = format!(
                "{}\n{}\n",
                super::CC_SWITCH_SQL_EXPORT_HEADER,
                template.replace("{path}", &target.to_string_lossy().replace('\'', "''"))
            );

            let db = Database::memory()?;
            let result = db.import_sql_string(&malicious);

            let error = result.expect_err("越界 SQL 必须被拒绝");
            assert!(
                error.to_string().to_ascii_lowercase().contains("authoriz"),
                "{label} 必须由 authorizer 拒绝，实际错误: {error}"
            );
            // 光报错不够：文件创建发生在 prepare 之后、`validate_basic_state` 之前，
            // 守卫若失效，即便导入整体失败，文件也已经躺在磁盘上了。
            assert!(
                !target.exists(),
                "被拒绝的 {label} 不得在磁盘上留下文件: {}",
                target.display()
            );
        }
        Ok(())
    }

    #[test]
    #[serial]
    fn import_still_accepts_a_genuine_export() -> Result<(), AppError> {
        let _test_home = TestHomeGuard::new();
        // 白名单收得紧，必须有一条回归防线证明它没误伤自家导出格式——
        // 这条测试红了就说明 dump_sql 写出了白名单没覆盖的语句。
        let source = Database::memory()?;
        {
            let conn = crate::database::lock_conn!(source.conn);
            conn.execute(
                "INSERT INTO providers (id, app_type, name, settings_config, meta)
                 VALUES ('p1', 'claude', 'Provider One', '{}', '{}')",
                [],
            )?;
        }
        let exported = source.export_sql_string()?;

        let target = Database::memory()?;
        target.import_sql_string(&exported)?;

        let conn = crate::database::lock_conn!(target.conn);
        let name: String = conn.query_row(
            "SELECT name FROM providers WHERE id = 'p1' AND app_type = 'claude'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(name, "Provider One");
        Ok(())
    }

    #[test]
    #[serial]
    fn sql_file_api_round_trips_existing_export_behavior() -> Result<(), AppError> {
        let test_home = TestHomeGuard::new();
        let source = Database::memory()?;
        {
            let conn = crate::database::lock_conn!(source.conn);
            conn.execute_batch(
                "INSERT INTO providers (id, app_type, name, settings_config, meta)
                 VALUES ('file-provider', 'claude', 'File Provider', '{}', '{}');
                 INSERT INTO proxy_request_logs (
                     request_id, provider_id, app_type, model,
                     input_tokens, output_tokens, total_cost_usd,
                     latency_ms, status_code, created_at
                 ) VALUES ('file-request', 'file-provider', 'claude', 'claude-file', 5, 3, '0', 10, 200, 1);",
            )?;
        }

        let backup_path = test_home.path().join("round-trip.sql");
        source.export_sql(&backup_path)?;

        let target = Database::memory()?;
        {
            let conn = crate::database::lock_conn!(target.conn);
            conn.execute(
                "INSERT INTO providers (id, app_type, name, settings_config, meta)
                 VALUES ('target-sentinel', 'claude', 'Must Be Replaced', '{}', '{}')",
                [],
            )?;
        }
        target.import_sql(&backup_path)?;

        let conn = crate::database::lock_conn!(target.conn);
        let providers = conn
            .prepare("SELECT id FROM providers ORDER BY id")?
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        assert_eq!(providers, vec!["file-provider"]);
        let request_exists: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM proxy_request_logs WHERE request_id = 'file-request')",
            [],
            |row| row.get(0),
        )?;
        assert!(request_exists, "文件 API 必须完整恢复导出数据");
        Ok(())
    }

    #[test]
    #[serial]
    fn failed_sql_import_keeps_the_existing_database_unchanged() -> Result<(), AppError> {
        let _test_home = TestHomeGuard::new();
        let target = Database::memory()?;
        {
            let conn = crate::database::lock_conn!(target.conn);
            conn.execute(
                "INSERT INTO providers (id, app_type, name, settings_config, meta)
                 VALUES ('sentinel', 'claude', 'Existing Provider', '{}', '{}')",
                [],
            )?;
        }

        let invalid_sql = format!(
            "{}\nBEGIN TRANSACTION;\nCREATE TABLE partial (id INTEGER);\nTHIS IS NOT SQL;\n",
            super::CC_SWITCH_SQL_EXPORT_HEADER
        );
        assert!(target.import_sql_string(&invalid_sql).is_err());

        let conn = crate::database::lock_conn!(target.conn);
        let provider: (i64, String, String) = conn.query_row(
            "SELECT COUNT(*), MIN(id), MIN(name) FROM providers",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        assert_eq!(provider, (1, "sentinel".into(), "Existing Provider".into()));
        let partial_exists: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'partial')",
            [],
            |row| row.get(0),
        )?;
        assert!(!partial_exists, "失败导入的临时对象不得进入主库");
        Ok(())
    }

    #[test]
    #[serial]
    fn import_still_accepts_legacy_single_row_insert_exports() -> Result<(), AppError> {
        let _test_home = TestHomeGuard::new();
        // This schema is copied from the v3.8.3 tag. Its data statements use
        // the historical one-row-per-INSERT format and omit all newer columns.
        let legacy = format!(
            "{}\nPRAGMA foreign_keys=OFF;\nPRAGMA user_version=1;\nBEGIN TRANSACTION;\n{}
             INSERT INTO providers (
                 id, app_type, name, settings_config, meta, is_current
             ) VALUES (
                 'legacy-provider', 'claude', 'Legacy Provider',
                 '{{\"anthropicApiKey\":\"sk-old\"}}', '{{}}', 1
             );
             INSERT INTO skills (key, installed, installed_at)
             VALUES ('claude:legacy-skill', 1, 1700000000);
             COMMIT;\nPRAGMA foreign_keys=ON;\n",
            super::CC_SWITCH_SQL_EXPORT_HEADER,
            crate::database::tests::V3_8_SCHEMA_V1_SQL,
        );

        let target = Database::memory()?;
        target.import_sql_string(&legacy)?;

        let conn = crate::database::lock_conn!(target.conn);
        let user_version: i32 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        assert_eq!(user_version, crate::database::SCHEMA_VERSION);
        let provider: (String, String) = conn.query_row(
            "SELECT name, settings_config FROM providers WHERE id = 'legacy-provider'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        assert_eq!(
            provider,
            (
                "Legacy Provider".into(),
                "{\"anthropicApiKey\":\"sk-old\"}".into()
            )
        );
        let cost_multiplier: String = conn.query_row(
            "SELECT cost_multiplier FROM providers WHERE id = 'legacy-provider'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(cost_multiplier, "1.0");
        let skill_snapshot: String = conn.query_row(
            "SELECT value FROM settings WHERE key = 'skills_ssot_migration_snapshot'",
            [],
            |row| row.get(0),
        )?;
        assert!(
            skill_snapshot.contains("legacy-skill"),
            "重建 skills 表时必须保留旧数据迁移快照"
        );
        Ok(())
    }

    #[test]
    #[serial]
    fn dump_sql_batches_rows_into_multi_row_inserts() -> Result<(), AppError> {
        let _test_home = TestHomeGuard::new();
        // 每行一条 INSERT 是导入慢的根源（恢复侧逐条解析，2 万行实测 21s）。
        // 这条测试钉死批量格式：450 行必须合并成 ceil(450/200) = 3 条语句。
        // 一旦退回到逐行导出，这里立刻变红。
        let db = Database::memory()?;
        {
            let conn = crate::database::lock_conn!(db.conn);
            for i in 0..450 {
                conn.execute(
                    "INSERT INTO providers (id, app_type, name, settings_config, meta)
                     VALUES (?1, 'claude', 'p', '{}', '{}')",
                    [format!("p{i}")],
                )?;
            }
        }

        let sql = db.export_sql_string()?;
        let insert_count = sql.matches("INSERT INTO \"providers\"").count();
        assert_eq!(
            insert_count, 3,
            "450 行应合并为 3 条多行 INSERT（每批 200 行），实际 {insert_count} 条"
        );

        let target = Database::memory()?;
        target.import_sql_string(&sql)?;
        let conn = crate::database::lock_conn!(target.conn);
        let row_count: i64 =
            conn.query_row("SELECT COUNT(*) FROM providers", [], |row| row.get(0))?;
        assert_eq!(row_count, 450, "批次边界不得漏行或重复行");
        for boundary in [0, 199, 200, 399, 400, 449] {
            let exists: bool = conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM providers WHERE id = ?1)",
                [format!("p{boundary}")],
                |row| row.get(0),
            )?;
            assert!(exists, "批次边界行 p{boundary} 必须完整恢复");
        }
        Ok(())
    }

    #[test]
    fn dump_sql_splits_large_rows_by_statement_bytes() -> Result<(), AppError> {
        let source = Connection::open_in_memory()?;
        source.execute(
            "CREATE TABLE large_rows (id INTEGER PRIMARY KEY, payload TEXT NOT NULL)",
            [],
        )?;

        // Each row fits below the byte cap, while any pair exceeds it.
        let payload = "x".repeat(super::INSERT_BATCH_MAX_BYTES / 2 + 1024);
        for id in 1..=3 {
            source.execute(
                "INSERT INTO large_rows (id, payload) VALUES (?1, ?2)",
                rusqlite::params![id, payload],
            )?;
        }

        let sql = Database::dump_sql(&source, &[])?;
        let inserts = sql
            .lines()
            .filter(|line| line.starts_with("INSERT INTO \"large_rows\""))
            .collect::<Vec<_>>();
        assert_eq!(inserts.len(), 3, "超大字段应按 SQL 字节数提前切批");
        assert!(
            inserts
                .iter()
                .all(|statement| statement.len() <= super::INSERT_BATCH_MAX_BYTES),
            "每条可独立容纳的 INSERT 都应保持在字节上限内"
        );

        let target = Connection::open_in_memory()?;
        target.execute_batch(&sql)?;
        let (count, min_len, max_len): (i64, i64, i64) = target.query_row(
            "SELECT COUNT(*), MIN(length(payload)), MAX(length(payload)) FROM large_rows",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        assert_eq!(count, 3);
        assert_eq!(min_len, payload.len() as i64);
        assert_eq!(max_len, payload.len() as i64);
        Ok(())
    }

    #[test]
    fn dump_sql_round_trips_generated_columns_and_quoted_identifiers() -> Result<(), AppError> {
        let source = Connection::open_in_memory()?;
        source.execute_batch(
            r#"
            CREATE TABLE "generated""values" (
                "a" TEXT NOT NULL,
                "computed" TEXT GENERATED ALWAYS AS ("a" || '-generated') STORED,
                "b""tail" TEXT NOT NULL
            );
            INSERT INTO "generated""values" ("a", "b""tail")
            VALUES ('source', 'ordinary-tail');
            "#,
        )?;

        let sql = Database::dump_sql(&source, &[])?;
        assert!(sql.contains("INSERT INTO \"generated\"\"values\" (\"a\", \"b\"\"tail\") VALUES"));

        let target = Connection::open_in_memory()?;
        target.execute_batch(&sql)?;
        let values: (String, String, String) = target.query_row(
            "SELECT \"a\", \"computed\", \"b\"\"tail\" FROM \"generated\"\"values\"",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        assert_eq!(
            values,
            (
                "source".to_string(),
                "source-generated".to_string(),
                "ordinary-tail".to_string()
            )
        );
        Ok(())
    }

    #[test]
    fn restore_tables_reads_only_insertable_columns() -> Result<(), AppError> {
        let source = Connection::open_in_memory()?;
        let target = Connection::open_in_memory()?;
        for conn in [&source, &target] {
            conn.execute_batch(
                r#"
                CREATE TABLE generated_values (
                    a TEXT NOT NULL,
                    computed TEXT GENERATED ALWAYS AS (a || '-generated') STORED,
                    "b""tail" TEXT NOT NULL
                );
                "#,
            )?;
        }
        source.execute(
            "INSERT INTO generated_values (a, \"b\"\"tail\") VALUES ('new', 'new-tail')",
            [],
        )?;
        target.execute(
            "INSERT INTO generated_values (a, \"b\"\"tail\") VALUES ('old', 'old-tail')",
            [],
        )?;

        Database::restore_tables(&source, &target, &["generated_values"])?;

        let values: (String, String, String) = target.query_row(
            "SELECT a, computed, \"b\"\"tail\" FROM generated_values",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        assert_eq!(
            values,
            (
                "new".to_string(),
                "new-generated".to_string(),
                "new-tail".to_string()
            )
        );
        Ok(())
    }

    #[test]
    fn restore_tables_rolls_back_all_tables_on_late_failure() -> Result<(), AppError> {
        let source = Connection::open_in_memory()?;
        source.execute_batch(
            "CREATE TABLE first_table (value TEXT NOT NULL);
             CREATE TABLE second_table (value INTEGER NOT NULL);
             INSERT INTO first_table VALUES ('replacement');
             INSERT INTO second_table VALUES (-1);",
        )?;

        let target = Connection::open_in_memory()?;
        target.execute_batch(
            "CREATE TABLE first_table (value TEXT NOT NULL);
             CREATE TABLE second_table (value INTEGER NOT NULL CHECK (value >= 0));
             INSERT INTO first_table VALUES ('sentinel-first');
             INSERT INTO second_table VALUES (7);",
        )?;

        let result = Database::restore_tables(&source, &target, &["first_table", "second_table"]);
        assert!(result.is_err(), "第二张表的约束错误必须终止恢复");

        let first: String =
            target.query_row("SELECT value FROM first_table", [], |row| row.get(0))?;
        let second: i64 =
            target.query_row("SELECT value FROM second_table", [], |row| row.get(0))?;
        assert_eq!(first, "sentinel-first", "第一张表必须随事务整体回滚");
        assert_eq!(second, 7, "失败表的 DELETE 也必须回滚");
        Ok(())
    }

    #[test]
    fn dump_sql_loads_rows_before_creating_triggers() -> Result<(), AppError> {
        let source = Connection::open_in_memory()?;
        source.execute_batch(
            "CREATE TABLE triggered_rows (seq INTEGER PRIMARY KEY);
             INSERT INTO triggered_rows VALUES (1), (2), (3);
             CREATE TRIGGER ignore_second_row
             BEFORE INSERT ON triggered_rows
             WHEN NEW.seq = 2
             BEGIN
                 SELECT RAISE(IGNORE);
             END;",
        )?;

        let sql = Database::dump_sql(&source, &[])?;
        let data_pos = sql.find("INSERT INTO \"triggered_rows\"").unwrap();
        let trigger_pos = sql.find("CREATE TRIGGER ignore_second_row").unwrap();
        assert!(data_pos < trigger_pos, "触发器必须在数据恢复完成后创建");

        let target = Connection::open_in_memory()?;
        target.execute_batch(&sql)?;
        let rows = target
            .prepare("SELECT seq FROM triggered_rows ORDER BY seq")?
            .query_map([], |row| row.get::<_, i64>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        assert_eq!(rows, vec![1, 2, 3]);
        let trigger_exists: bool = target.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'trigger' AND name = 'ignore_second_row')",
            [],
            |row| row.get(0),
        )?;
        assert!(trigger_exists, "触发器本身仍必须随备份恢复");
        Ok(())
    }

    #[test]
    fn dump_sql_preserves_indexes_and_views() -> Result<(), AppError> {
        let source = Connection::open_in_memory()?;
        source.execute_batch(
            "CREATE TABLE indexed_rows (id INTEGER PRIMARY KEY, value TEXT NOT NULL);
             CREATE UNIQUE INDEX indexed_rows_value_idx ON indexed_rows(value);
             CREATE VIEW indexed_rows_view AS
                 SELECT id, value FROM indexed_rows WHERE value LIKE 'kept%';
             CREATE TRIGGER a_insert_indexed_rows_view
             INSTEAD OF INSERT ON indexed_rows_view
             BEGIN
                 INSERT INTO indexed_rows (id, value) VALUES (NEW.id, NEW.value);
             END;
             INSERT INTO indexed_rows VALUES (1, 'kept-value'), (2, 'hidden-value');",
        )?;

        let sql = Database::dump_sql(&source, &[])?;
        let target = Connection::open_in_memory()?;
        target.execute_batch(&sql)?;

        for (object_type, object_name) in [
            ("index", "indexed_rows_value_idx"),
            ("view", "indexed_rows_view"),
            ("trigger", "a_insert_indexed_rows_view"),
        ] {
            let exists: bool = target.query_row(
                "SELECT EXISTS(
                    SELECT 1 FROM sqlite_master WHERE type = ?1 AND name = ?2
                )",
                [object_type, object_name],
                |row| row.get(0),
            )?;
            assert!(exists, "{object_type} {object_name} 必须随 SQL dump 恢复");
        }

        target.execute(
            "INSERT INTO indexed_rows_view (id, value) VALUES (3, 'kept-via-trigger')",
            [],
        )?;
        let view_rows = target
            .prepare("SELECT id, value FROM indexed_rows_view ORDER BY id")?
            .query_map([], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        assert_eq!(
            view_rows,
            vec![
                (1, "kept-value".to_string()),
                (3, "kept-via-trigger".to_string()),
            ]
        );
        Ok(())
    }

    #[test]
    #[serial]
    fn multi_row_dump_round_trips_special_values() -> Result<(), AppError> {
        let _test_home = TestHomeGuard::new();
        // 多行 VALUES 的转义面比单行宽：单引号、换行、英文逗号（列分隔符）、
        // 中文、emoji、BLOB、NULL——任何一个处理错都会让整批语法崩掉或数据变形。
        let source = Database::memory()?;
        {
            let conn = crate::database::lock_conn!(source.conn);
            conn.execute(
                "INSERT INTO providers (id, app_type, name, settings_config, meta)
                 VALUES ('special', 'claude', ?1, ?2, '{}')",
                rusqlite::params!["O'Brien,\n第二行 \"quoted\" 😀", "{\"key\": \"it's, ok\"}"],
            )?;
            conn.execute(
                "INSERT INTO providers (id, app_type, name, settings_config, meta)
                 VALUES ('with-blob', 'claude', 'blob', X'00FF10', '{}')",
                [],
            )?;
            conn.execute(
                "INSERT INTO providers (id, app_type, name, settings_config, meta, category)
                 VALUES ('with-null', 'claude', 'nullcat', '{}', '{}', NULL)",
                [],
            )?;
        }

        let sql = source.export_sql_string()?;
        let target = Database::memory()?;
        target.import_sql_string(&sql)?;

        let conn = crate::database::lock_conn!(target.conn);
        let name: String = conn.query_row(
            "SELECT name FROM providers WHERE id = 'special'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(name, "O'Brien,\n第二行 \"quoted\" 😀");
        let cfg: String = conn.query_row(
            "SELECT settings_config FROM providers WHERE id = 'special'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(cfg, "{\"key\": \"it's, ok\"}");

        let blob_type: String = conn.query_row(
            "SELECT typeof(settings_config) FROM providers WHERE id = 'with-blob'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(blob_type, "blob", "BLOB 存储类型必须在往返后保留");
        let blob: Vec<u8> = conn.query_row(
            "SELECT settings_config FROM providers WHERE id = 'with-blob'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(blob, vec![0x00, 0xFF, 0x10]);

        let category: Option<String> = conn.query_row(
            "SELECT category FROM providers WHERE id = 'with-null'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(category, None, "NULL 必须在往返后保留");
        Ok(())
    }

    #[test]
    #[serial]
    fn sync_import_preserves_local_only_tables() -> Result<(), AppError> {
        let _test_home = TestHomeGuard::new();
        let remote_db = Database::memory()?;
        {
            let conn = crate::database::lock_conn!(remote_db.conn);
            conn.execute_batch(
                "INSERT INTO providers (id, app_type, name, settings_config, meta)
                 VALUES ('remote-provider', 'claude', 'Remote Provider', '{}', '{}');
                 INSERT INTO proxy_request_logs (
                     request_id, provider_id, app_type, model,
                     input_tokens, output_tokens, total_cost_usd,
                     latency_ms, status_code, created_at
                 ) VALUES ('remote-request', 'remote-provider', 'claude', 'remote-model', 1, 1, '1', 1, 200, 1);
                 INSERT INTO usage_daily_rollups (
                     date, app_type, provider_id, model, request_count, success_count,
                     input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                     total_cost_usd, avg_latency_ms
                 ) VALUES ('2099-01-01', 'claude', 'remote-provider', 'remote-model', 1, 1, 1, 1, 0, 0, '1', 1);
                 INSERT INTO stream_check_logs (
                     provider_id, provider_name, app_type, status, success, message,
                     response_time_ms, http_status, model_used, retry_count, tested_at
                 ) VALUES ('remote-provider', 'Remote Provider', 'claude', 'failed', 0, 'remote', 1, 500, 'remote-model', 0, 1);
                 INSERT INTO proxy_live_backup (app_type, original_config, backed_up_at)
                 VALUES ('claude', 'remote-live', '2099-01-01');
                 INSERT INTO provider_health (
                     provider_id, app_type, is_healthy, consecutive_failures, updated_at
                 ) VALUES ('remote-provider', 'claude', 0, 9, '2099-01-01');",
            )?;
        }
        let remote_sql = remote_db.export_sql_string_for_sync()?;
        let exported = Connection::open_in_memory()?;
        exported.execute_batch(&remote_sql)?;
        let skipped_counts: (i64, i64, i64, i64, i64) = exported.query_row(
            "SELECT
                (SELECT COUNT(*) FROM proxy_request_logs),
                (SELECT COUNT(*) FROM stream_check_logs),
                (SELECT COUNT(*) FROM provider_health),
                (SELECT COUNT(*) FROM proxy_live_backup),
                (SELECT COUNT(*) FROM usage_daily_rollups)",
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )?;
        assert_eq!(skipped_counts, (0, 0, 0, 0, 0));

        let local_db = Database::memory()?;
        {
            let conn = crate::database::lock_conn!(local_db.conn);
            conn.execute_batch(
                "INSERT INTO providers (id, app_type, name, settings_config, meta)
                 VALUES ('local-provider', 'claude', 'Local Provider', '{}', '{}');
                 INSERT INTO proxy_request_logs (
                     request_id, provider_id, app_type, model,
                     input_tokens, output_tokens, total_cost_usd,
                     latency_ms, status_code, created_at
                 ) VALUES ('req-1', 'local-provider', 'claude', 'claude-3', 100, 50, '0.01', 120, 200, 1000);
                 INSERT INTO usage_daily_rollups (
                     date, app_type, provider_id, model, request_count, success_count,
                     input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                     total_cost_usd, avg_latency_ms
                 ) VALUES ('2026-03-01', 'claude', 'local-provider', 'claude-3', 7, 7, 700, 350, 0, 0, '0.07', 120);
                 INSERT INTO stream_check_logs (
                     provider_id, provider_name, app_type, status, success, message,
                     response_time_ms, http_status, model_used, retry_count, tested_at
                 ) VALUES ('local-provider', 'Local Provider', 'claude', 'operational', 1, 'local-ok', 42, 200, 'claude-3', 0, 1000);
                 INSERT INTO proxy_live_backup (app_type, original_config, backed_up_at)
                 VALUES ('claude', '{\"local\":true}', '2026-03-01');
                 INSERT INTO provider_health (
                     provider_id, app_type, is_healthy, consecutive_failures, updated_at
                 ) VALUES ('local-provider', 'claude', 1, 0, '2026-03-01');",
            )?;
        }

        local_db.import_sql_string_for_sync(&remote_sql)?;

        let conn = crate::database::lock_conn!(local_db.conn);
        let providers = conn
            .prepare("SELECT id FROM providers ORDER BY id")?
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        assert_eq!(providers, vec!["remote-provider"]);

        let preserved_counts: (i64, i64, i64, i64) = conn.query_row(
            "SELECT
                (SELECT COUNT(*) FROM proxy_request_logs),
                (SELECT COUNT(*) FROM stream_check_logs),
                (SELECT COUNT(*) FROM proxy_live_backup),
                (SELECT COUNT(*) FROM usage_daily_rollups)",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )?;
        assert_eq!(
            preserved_counts,
            (1, 1, 1, 1),
            "同步导入必须替换配置，同时保留本机日志与 Live 备份"
        );

        let preserved_values: (String, String, i64, String, i64, String, i64) = conn.query_row(
            "SELECT
                (SELECT request_id FROM proxy_request_logs),
                (SELECT model FROM proxy_request_logs),
                (SELECT input_tokens FROM proxy_request_logs),
                (SELECT date FROM usage_daily_rollups),
                (SELECT request_count FROM usage_daily_rollups),
                (SELECT message FROM stream_check_logs),
                (SELECT response_time_ms FROM stream_check_logs)",
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                ))
            },
        )?;
        assert_eq!(
            preserved_values,
            (
                "req-1".into(),
                "claude-3".into(),
                100,
                "2026-03-01".into(),
                7,
                "local-ok".into(),
                42,
            )
        );

        let live_backup: (String, String) = conn.query_row(
            "SELECT original_config, backed_up_at FROM proxy_live_backup WHERE app_type = 'claude'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        assert_eq!(
            live_backup,
            ("{\"local\":true}".into(), "2026-03-01".into())
        );
        let provider_health_count: i64 =
            conn.query_row("SELECT COUNT(*) FROM provider_health", [], |row| row.get(0))?;
        assert_eq!(
            provider_health_count, 0,
            "同步导入应清除可重建的本地 provider_health 状态"
        );
        Ok(())
    }

    #[test]
    #[serial]
    fn periodic_maintenance_runs_even_when_auto_backup_disabled() -> Result<(), AppError> {
        let _test_home = TestHomeGuard::new();

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

        Ok(())
    }

    /// 性能基准（不是回归测试）：用接近重度代理用户的行数测量
    /// 导出 / 本地文件导入 / 同步导入三条路径的耗时与产物大小。
    ///
    /// 手动运行：`cargo test --lib perf_backup -- --ignored --nocapture`
    #[test]
    #[ignore = "perf harness, run explicitly"]
    #[serial]
    fn perf_backup_export_import_paths() -> Result<(), AppError> {
        use std::time::Instant;

        const LOG_ROWS: usize = 20_000;
        const STREAM_ROWS: usize = 5_000;
        const ROLLUP_ROWS: usize = 1_000;

        let _test_home = TestHomeGuard::new();

        fn populate(
            db: &Database,
            log_rows: usize,
            stream_rows: usize,
            rollup_rows: usize,
        ) -> Result<(), AppError> {
            let mut conn = crate::database::lock_conn!(db.conn);
            let tx = conn.transaction()?;
            for i in 0..50 {
                tx.execute(
                    "INSERT INTO providers (id, app_type, name, settings_config, meta)
                     VALUES (?1, 'claude', ?2, '{}', '{}')",
                    rusqlite::params![format!("p{i}"), format!("Provider {i}")],
                )?;
            }
            for i in 0..log_rows {
                tx.execute(
                    "INSERT INTO proxy_request_logs (
                        request_id, provider_id, app_type, model,
                        input_tokens, output_tokens, total_cost_usd,
                        latency_ms, status_code, created_at
                    ) VALUES (?1, 'p1', 'claude', 'claude-3', 100, 50, '0.01', 120, 200, 1000)",
                    [format!("req-{i}")],
                )?;
            }
            for i in 0..stream_rows {
                tx.execute(
                    "INSERT INTO stream_check_logs (
                        provider_id, provider_name, app_type, status, success, message,
                        response_time_ms, http_status, model_used, retry_count, tested_at
                    ) VALUES ('p1', 'Provider 1', 'claude', 'operational', 1, 'ok', 42, 200, 'claude-3', 0, ?1)",
                    [1000i64 + i as i64],
                )?;
            }
            for i in 0..rollup_rows {
                // (date, app_type, provider_id, model, request_model, pricing_model)
                // 上有 UNIQUE 约束，日期必须逐行唯一。
                let date = format!(
                    "{:04}-{:02}-{:02}",
                    2025 + i / 336,
                    i / 28 % 12 + 1,
                    i % 28 + 1
                );
                tx.execute(
                    "INSERT INTO usage_daily_rollups (
                        date, app_type, provider_id, model, request_count, success_count,
                        input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                        total_cost_usd, avg_latency_ms
                    ) VALUES (?1, 'claude', 'p1', 'claude-3', 7, 7, 700, 350, 0, 0, '0.07', 120)",
                    [date],
                )?;
            }
            tx.commit()?;
            Ok(())
        }

        let source = Database::memory()?;
        populate(&source, LOG_ROWS, STREAM_ROWS, ROLLUP_ROWS)?;

        let t = Instant::now();
        let full_sql = source.export_sql_string()?;
        println!(
            "export_sql_string (full): {:?}, {} bytes",
            t.elapsed(),
            full_sql.len()
        );

        let t = Instant::now();
        let import_target = Database::memory()?;
        import_target.import_sql_string(&full_sql)?;
        println!("import_sql_string (local file path): {:?}", t.elapsed());
        {
            let conn = crate::database::lock_conn!(import_target.conn);
            let counts: (i64, i64, i64, i64) = conn.query_row(
                "SELECT
                    (SELECT COUNT(*) FROM providers),
                    (SELECT COUNT(*) FROM proxy_request_logs),
                    (SELECT COUNT(*) FROM stream_check_logs),
                    (SELECT COUNT(*) FROM usage_daily_rollups)",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )?;
            assert_eq!(
                counts,
                (50, LOG_ROWS as i64, STREAM_ROWS as i64, ROLLUP_ROWS as i64)
            );
        }

        let sync_sql = source.export_sql_string_for_sync()?;
        println!("sync payload: {} bytes", sync_sql.len());

        // 同步导入的耗时大头在“保留本机日志表”——本机库必须带同样规模的日志行。
        let local = Database::memory()?;
        populate(&local, LOG_ROWS, STREAM_ROWS, ROLLUP_ROWS)?;
        let t = Instant::now();
        local.import_sql_string_for_sync(&sync_sql)?;
        println!(
            "import_sql_string_for_sync ({} preserved log rows): {:?}",
            LOG_ROWS + STREAM_ROWS + ROLLUP_ROWS,
            t.elapsed()
        );
        {
            let conn = crate::database::lock_conn!(local.conn);
            let counts: (i64, i64, i64) = conn.query_row(
                "SELECT
                    (SELECT COUNT(*) FROM proxy_request_logs),
                    (SELECT COUNT(*) FROM stream_check_logs),
                    (SELECT COUNT(*) FROM usage_daily_rollups)",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )?;
            assert_eq!(
                counts,
                (LOG_ROWS as i64, STREAM_ROWS as i64, ROLLUP_ROWS as i64)
            );
        }
        Ok(())
    }

    /// 分阶段拆解 import_sql_string 的耗时，定位慢在哪一步。
    ///
    /// 手动运行：`cargo test --lib perf_import_phases -- --ignored --nocapture`
    #[test]
    #[ignore = "perf diagnostic, run explicitly"]
    fn perf_import_phases() -> Result<(), AppError> {
        use rusqlite::Connection;
        use std::time::Instant;
        use tempfile::NamedTempFile;

        const LOG_ROWS: usize = 20_000;

        let source = Database::memory()?;
        {
            let mut conn = crate::database::lock_conn!(source.conn);
            let tx = conn.transaction()?;
            for i in 0..50 {
                tx.execute(
                    "INSERT INTO providers (id, app_type, name, settings_config, meta)
                     VALUES (?1, 'claude', ?2, '{}', '{}')",
                    rusqlite::params![format!("p{i}"), format!("Provider {i}")],
                )?;
            }
            for i in 0..LOG_ROWS {
                tx.execute(
                    "INSERT INTO proxy_request_logs (
                        request_id, provider_id, app_type, model,
                        input_tokens, output_tokens, total_cost_usd,
                        latency_ms, status_code, created_at
                    ) VALUES (?1, 'p1', 'claude', 'claude-3', 100, 50, '0.01', 120, 200, 1000)",
                    [format!("req-{i}")],
                )?;
            }
            tx.commit()?;
        }
        let sql = source.export_sql_string()?;
        println!("payload: {} bytes, {LOG_ROWS} log rows", sql.len());

        let temp_file = NamedTempFile::new().expect("temp file");
        let temp_conn = Connection::open(temp_file.path()).expect("open temp conn");

        let t = Instant::now();
        temp_conn
            .execute_batch(&sql)
            .expect("execute_batch should succeed");
        println!("phase execute_batch: {:?}", t.elapsed());

        let t = Instant::now();
        Database::create_tables_on_conn(&temp_conn)?;
        Database::apply_schema_migrations_on_conn(&temp_conn)?;
        println!("phase schema+migrations: {:?}", t.elapsed());

        let t = Instant::now();
        let target = Database::memory()?;
        {
            let mut main_conn = crate::database::lock_conn!(target.conn);
            let backup =
                rusqlite::backup::Backup::new(&temp_conn, &mut main_conn).expect("backup init");
            backup.step(-1).expect("backup step");
        }
        println!("phase backup-to-main: {:?}", t.elapsed());

        // 对照组：同样的语句但临时库关掉 journal / synchronous。
        let temp_file2 = NamedTempFile::new().expect("temp file 2");
        let temp_conn2 = Connection::open(temp_file2.path()).expect("open temp conn 2");
        temp_conn2
            .execute_batch("PRAGMA journal_mode=MEMORY; PRAGMA synchronous=OFF;")
            .expect("pragmas");
        let t = Instant::now();
        temp_conn2
            .execute_batch(&sql)
            .expect("execute_batch should succeed");
        println!(
            "phase execute_batch (journal=MEMORY, sync=OFF): {:?}",
            t.elapsed()
        );

        // 对照组 B：同一份脚本跑在内存库上，区分“纯 CPU/解析”还是“文件 I/O”。
        let mem_conn = Connection::open_in_memory().expect("open mem conn");
        let t = Instant::now();
        mem_conn
            .execute_batch(&sql)
            .expect("execute_batch mem should succeed");
        println!("phase execute_batch (in-memory): {:?}", t.elapsed());

        // 对照组 C：同样的数据改成多行 VALUES（每 200 行一条 INSERT），
        // 验证“每行一条语句”的解析开销占比。
        let mut batched = String::from("PRAGMA foreign_keys=OFF;\nBEGIN TRANSACTION;\n");
        batched.push_str(
            "CREATE TABLE bench_logs (
                request_id TEXT, provider_id TEXT, app_type TEXT, model TEXT,
                input_tokens INTEGER, output_tokens INTEGER, total_cost_usd TEXT,
                latency_ms INTEGER, status_code INTEGER, created_at INTEGER
            );\n",
        );
        const BATCH: usize = 200;
        for chunk_start in (0..LOG_ROWS).step_by(BATCH) {
            batched.push_str("INSERT INTO bench_logs VALUES ");
            for i in chunk_start..(chunk_start + BATCH).min(LOG_ROWS) {
                if i > chunk_start {
                    batched.push(',');
                }
                batched.push_str(&format!(
                    "('req-{i}','p1','claude','claude-3',100,50,'0.01',120,200,1000)"
                ));
            }
            batched.push_str(";\n");
        }
        batched.push_str("COMMIT;\n");
        let mem_conn2 = Connection::open_in_memory().expect("open mem conn 2");
        let t = Instant::now();
        mem_conn2
            .execute_batch(&batched)
            .expect("batched should succeed");
        println!(
            "phase execute_batch (in-memory, multi-row VALUES x{BATCH}): {:?}",
            t.elapsed()
        );

        Ok(())
    }
}
