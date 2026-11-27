//! 数据库备份和恢复
//!
//! 提供 SQL 导出/导入和二进制快照备份功能。

use super::{lock_conn, Database};
use crate::config::get_app_config_dir;
use crate::error::AppError;
use chrono::{Local, Utc};
use rusqlite::backup::{Backup, StepResult};
use rusqlite::types::ValueRef;
use rusqlite::Connection;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};
use tempfile::{Builder, NamedTempFile};

const CC_SWITCH_SQL_EXPORT_HEADER: &str = "-- CC Switch SQLite 导出";

/// SQL 导入允许的最大体积（与云同步产物上限保持一致，防止超大文件触发 OOM）。
const MAX_SQL_IMPORT_BYTES: u64 = crate::services::sync_protocol::MAX_SYNC_ARTIFACT_BYTES;

/// 外部 SQL 批量执行的超时上限，防止恶意 `WITH RECURSIVE` 等语句永久挂死并占住锁。
const SQL_IMPORT_EXECUTION_TIMEOUT: Duration = Duration::from_secs(30);

/// SQLite progress_handler 回调间隔（每 N 条虚拟机指令检查一次截止时间）。
const SQL_IMPORT_PROGRESS_OPCODES: i32 = 10_000;

/// Bound combined INSERT batches while still amortizing statement parsing.
/// A row larger than this cap is emitted alone because it cannot be split.
const INSERT_BATCH_MAX_ROWS: usize = 200;
const INSERT_BATCH_MAX_BYTES: usize = 1024 * 1024;

/// Serialize every operation that observes or mutates the database-backup
/// directory. Always acquire this guard before `Database.conn`.
static BACKUP_FILE_OPERATION_LOCK: Mutex<()> = Mutex::new(());
type BackupFileOperationGuard = MutexGuard<'static, ()>;

fn lock_backup_file_operations() -> BackupFileOperationGuard {
    crate::sync_utils::lock_file_operation(&BACKUP_FILE_OPERATION_LOCK)
}

/// Backups are built under this prefix with a `.tmp` suffix, then published
/// as `*.db`. Only a crashed backup leaves such a file behind.
const BACKUP_TEMP_PREFIX: &str = ".cc-switch-backup-";
/// A backup temp file is written continuously while its backup runs, so one
/// that has not changed for this long belongs to no running backup.
const ORPHAN_BACKUP_TEMP_MIN_AGE: std::time::Duration = std::time::Duration::from_secs(24 * 3600);
/// SQLite files that belong to a database file of the same name.
const SQLITE_SIDECAR_SUFFIXES: &[&str] = &["-journal", "-wal", "-shm"];

/// `dump_sql` 会写出的 PRAGMA。其余 PRAGMA 一律拒绝——`temp_store_directory`
/// 能把临时文件重定向到任意目录，`writable_schema` 能绕过 schema 完整性检查。
const IMPORT_ALLOWED_PRAGMAS: &[&str] = &["foreign_keys", "user_version"];

/// 执行外部 SQL 期间的 authorizer：拒绝一切能**离开临时数据库文件**的动作。
///
/// 头部校验（`validate_cc_switch_sql_export`）只比较一个注释前缀，任何人都能在
/// 合法前缀后面接着写别的语句。`ATTACH DATABASE '/path/x.db'` 的副作用发生在
/// 暂存库的 schema 校验之前，导入即使最终失败，文件也已经被创建；而 `settings`
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
/// - CC Switch 生产库 schema 不包含任何 trigger / view；放行 `CREATE TRIGGER` / `CREATE VIEW`
///   会让恶意备份或云同步包在暂存库植入持久化触发器并被整库复制到主库，在用户后续写入供应商时静默篡改数据 → 拒 trigger / view
/// - `Unknown` 是 rusqlite 对未识别动作码的兜底 → 未知即拒，将来 SQLite 新增的
///   跨文件语句会默认落进这里，不依赖有人记得回来补名单
fn import_authorizer(context: rusqlite::hooks::AuthContext<'_>) -> rusqlite::hooks::Authorization {
    use rusqlite::hooks::{AuthAction, Authorization};

    let disallowed = match context.action {
        AuthAction::Attach { .. } | AuthAction::Detach { .. } => true,
        AuthAction::CreateVtable { .. } | AuthAction::DropVtable { .. } => true,
        AuthAction::CreateTrigger { .. }
        | AuthAction::CreateTempTrigger { .. }
        | AuthAction::DropTrigger { .. }
        | AuthAction::DropTempTrigger { .. }
        | AuthAction::CreateView { .. }
        | AuthAction::CreateTempView { .. }
        | AuthAction::DropView { .. }
        | AuthAction::DropTempView { .. } => true,
        AuthAction::Unknown { .. } => true,
        AuthAction::Pragma { pragma_name, .. } => !IMPORT_ALLOWED_PRAGMAS
            .iter()
            .any(|allowed| pragma_name.eq_ignore_ascii_case(allowed)),
        _ => false,
    };

    if disallowed {
        // SQLite 只会回一句 "not authorized"，不记日志就无从知道是哪条语句被拦。
        log::warn!("SQL 导入拒绝了不允许的语句: {:?}", context.action);
        Authorization::Deny
    } else {
        Authorization::Allow
    }
}

fn validate_sql_import_size(size_bytes: u64, max_bytes: u64) -> Result<(), AppError> {
    if size_bytes > max_bytes {
        let max_mb = max_bytes / 1024 / 1024;
        return Err(AppError::localized(
            "backup.sql.too_large",
            format!("SQL 文件过大，超过 {max_mb} MB 上限"),
            format!("SQL import exceeds the {max_mb} MB size limit"),
        ));
    }
    Ok(())
}

fn read_sql_file_bounded(source_path: &Path, max_bytes: u64) -> Result<String, AppError> {
    let metadata = fs::metadata(source_path).map_err(|e| AppError::io(source_path, e))?;
    validate_sql_import_size(metadata.len(), max_bytes)?;

    let file = fs::File::open(source_path).map_err(|e| AppError::io(source_path, e))?;
    let mut buf = Vec::new();
    file.take(max_bytes.saturating_add(1))
        .read_to_end(&mut buf)
        .map_err(|e| AppError::io(source_path, e))?;
    validate_sql_import_size(buf.len() as u64, max_bytes)?;

    String::from_utf8(buf).map_err(|e| {
        AppError::InvalidInput(format!(
            "SQL 文件不是有效的 UTF-8 编码 ({}): {e}",
            source_path.display()
        ))
    })
}

/// Tables whose data rows are skipped when exporting for WebDAV sync.
const SYNC_SKIP_TABLES: &[&str] = &[
    "proxy_request_logs",
    "stream_check_logs",
    "provider_health",
    "proxy_live_backup",
    "usage_daily_rollups",
    "session_log_sync",
    "session_usage_dedup",
];

/// Tables whose local data is preserved from the live database during WebDAV import.
/// Excludes ephemeral tables like provider_health that can safely rebuild at runtime.
const SYNC_PRESERVE_TABLES: &[&str] = &[
    "proxy_request_logs",
    "stream_check_logs",
    "proxy_live_backup",
    "usage_daily_rollups",
    "session_log_sync",
    "session_usage_dedup",
];

/// A database backup entry for the UI
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupEntry {
    pub filename: String,
    pub size_bytes: u64,
    pub created_at: String, // ISO 8601
}

impl Database {
    fn replace_from_staging(
        &self,
        source: &Connection,
        destination: &mut Connection,
        context: &str,
    ) -> Result<(), AppError> {
        let page_size: i64 = destination.query_row("PRAGMA page_size", [], |row| row.get(0))?;
        let source_page_size: i64 = source.query_row("PRAGMA page_size", [], |row| row.get(0))?;
        if source_page_size != page_size {
            // Rebuild the disposable staging DB, never disable WAL on the live DB.
            source.execute_batch(&format!(
                "PRAGMA journal_mode = DELETE; PRAGMA page_size = {page_size}; VACUUM;"
            ))?;
        }
        let result = {
            let backup =
                Backup::new(source, destination).map_err(|e| AppError::Database(e.to_string()))?;
            Self::complete_backup(&backup, context)
        };
        self.read_cache.invalidate_all();
        result?;
        Self::configure_connection(destination)
    }

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

        crate::config::atomic_write_private(target_path, dump.as_bytes())
    }

    /// 从 SQL 文件导入，返回生成的备份 ID（若无备份则为空字符串）
    pub fn import_sql(&self, source_path: &Path) -> Result<String, AppError> {
        if !source_path.exists() {
            return Err(AppError::InvalidInput(format!(
                "SQL 文件不存在: {}",
                source_path.display()
            )));
        }

        let sql_raw = read_sql_file_bounded(source_path, MAX_SQL_IMPORT_BYTES)?;
        let sql_content = sql_raw.trim_start_matches('\u{feff}');
        self.import_sql_string(sql_content)
    }

    /// 从 SQL 字符串导入，返回生成的备份 ID（若无备份则为空字符串）
    pub fn import_sql_string(&self, sql_raw: &str) -> Result<String, AppError> {
        self.import_sql_string_inner(sql_raw, &[])
    }

    /// Import SQL generated for sync, then restore local-only tables from the
    /// current live database before replacing it.
    pub(crate) fn import_sql_string_for_sync(&self, sql_raw: &str) -> Result<String, AppError> {
        self.import_sql_string_inner(sql_raw, SYNC_PRESERVE_TABLES)
    }

    fn import_sql_string_inner(
        &self,
        sql_raw: &str,
        preserve_tables: &[&str],
    ) -> Result<String, AppError> {
        self.import_sql_string_inner_with_hook(sql_raw, preserve_tables, || Ok(()))
    }

    fn import_sql_string_inner_with_hook<F>(
        &self,
        sql_raw: &str,
        preserve_tables: &[&str],
        on_staging_ready: F,
    ) -> Result<String, AppError>
    where
        F: FnOnce() -> Result<(), AppError>,
    {
        self.import_sql_string_inner_with_options(
            sql_raw,
            preserve_tables,
            SQL_IMPORT_EXECUTION_TIMEOUT,
            on_staging_ready,
        )
    }

    fn import_sql_string_inner_with_options<F>(
        &self,
        sql_raw: &str,
        preserve_tables: &[&str],
        execution_timeout: Duration,
        on_staging_ready: F,
    ) -> Result<String, AppError>
    where
        F: FnOnce() -> Result<(), AppError>,
    {
        validate_sql_import_size(sql_raw.len() as u64, MAX_SQL_IMPORT_BYTES)?;
        let sql_content = sql_raw.trim_start_matches('\u{feff}');
        Self::validate_cc_switch_sql_export(sql_content)?;

        // 在临时数据库执行导入，确保失败不会污染主库
        let temp_file = NamedTempFile::new().map_err(|e| AppError::IoContext {
            context: "创建临时数据库文件失败".to_string(),
            source: e,
        })?;
        let temp_path = temp_file.path().to_path_buf();
        let temp_conn =
            Connection::open(&temp_path).map_err(|e| AppError::Database(e.to_string()))?;
        // SQLite Backup copies the source database header into the destination.
        // Configure the empty staging database before creating any tables so a
        // SQL import cannot downgrade the main DB from incremental vacuum to NONE.
        temp_conn
            .execute("PRAGMA auto_vacuum = INCREMENTAL;", [])
            .map_err(|e| AppError::Database(format!("设置暂存库 auto_vacuum 失败: {e}")))?;

        // authorizer 与 progress_handler 只覆盖外部 SQL，执行完立刻摘掉：紧随其后的
        // `create_tables_on_conn` / `apply_schema_migrations_on_conn` 是本程序自己的
        // schema 维护语句，不属于需要设防的输入，没必要让它们也过一遍守卫。
        let deadline = Instant::now().checked_add(execution_timeout);
        temp_conn.authorizer(Some(import_authorizer));
        if let Some(deadline) = deadline {
            temp_conn.progress_handler(
                SQL_IMPORT_PROGRESS_OPCODES,
                Some(move || Instant::now() > deadline),
            );
        }
        let batch_result = temp_conn.execute_batch(sql_content);
        temp_conn.progress_handler(0, None::<fn() -> bool>);
        temp_conn.authorizer(
            None::<fn(rusqlite::hooks::AuthContext<'_>) -> rusqlite::hooks::Authorization>,
        );
        batch_result.map_err(|e| {
            if matches!(
                e,
                rusqlite::Error::SqliteFailure(
                    rusqlite::ffi::Error {
                        code: rusqlite::ErrorCode::OperationInterrupted,
                        ..
                    },
                    _
                )
            ) {
                AppError::localized(
                    "backup.sql.execution_timeout",
                    "SQL 导入执行超时，文件可能包含异常循环语句",
                    "SQL import timed out; the file may contain an abusive query",
                )
            } else {
                AppError::Database(format!("执行 SQL 导入失败: {e}"))
            }
        })?;
        if !temp_conn.is_autocommit() {
            let _ = temp_conn.execute_batch("ROLLBACK;");
            return Err(AppError::localized(
                "backup.sql.incomplete_transaction",
                "SQL 备份事务未完成，文件可能已截断。",
                "The SQL backup transaction is incomplete; the file may be truncated.",
            ));
        }

        // Validate the schema produced by the input itself before migrations
        // can create missing tables and accidentally make a truncated file look valid.
        Self::validate_imported_schema(&temp_conn)?;

        // 补齐缺失表/索引并执行迁移
        Self::create_tables_on_conn(&temp_conn)?;
        Self::apply_schema_migrations_on_conn(&temp_conn)?;
        // 导入和同步不经过保存时的校验：坏的自定义图标在这里清掉，不带进主库
        crate::services::provider::icon::strip_invalid_custom_icons(&temp_conn)?;
        on_staging_ready()?;

        let backup_file_guard = lock_backup_file_operations();
        // Keep one main-DB guard across the safety snapshot, local-table read,
        // and final replacement so neither the rollback point nor preserved
        // device-local rows can miss writes that arrived during staging.
        let backup_path = {
            let mut main_conn = lock_conn!(self.conn);
            let backup_path =
                Self::backup_database_file_from_conn(&backup_file_guard, &main_conn, &[])?;
            if !preserve_tables.is_empty() {
                Self::restore_tables(&main_conn, &temp_conn, preserve_tables)?;
            }
            self.replace_from_staging(&temp_conn, &mut main_conn, "替换主数据库")?;
            backup_path
        };

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
            Self::complete_backup(&backup, "创建内存数据库快照")?;
        }

        Ok(snapshot)
    }

    fn complete_backup(backup: &Backup<'_, '_>, context: &str) -> Result<(), AppError> {
        let result = backup
            .step(-1)
            .map_err(|e| AppError::Database(format!("{context}失败: {e}")))?;
        match result {
            StepResult::Done => Ok(()),
            StepResult::More | StepResult::Busy | StepResult::Locked => Err(AppError::Database(
                format!("{context}未完成: SQLite Backup 返回 {result:?}"),
            )),
            _ => Err(AppError::Database(format!(
                "{context}未完成: SQLite Backup 返回未知状态"
            ))),
        }
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

        Self::restore_sqlite_sequences(source_conn, &tx, tables)?;

        tx.commit()
            .map_err(|e| AppError::Database(format!("提交恢复事务失败: {e}")))?;
        Ok(())
    }

    fn restore_sqlite_sequences(
        source_conn: &Connection,
        target_conn: &Connection,
        tables: &[&str],
    ) -> Result<(), AppError> {
        if !Self::table_exists(source_conn, "sqlite_sequence")?
            || !Self::table_exists(target_conn, "sqlite_sequence")?
        {
            return Ok(());
        }

        let mut source_stmt = source_conn
            .prepare(
                "SELECT seq FROM sqlite_sequence
                 WHERE name = ?1 ORDER BY rowid DESC LIMIT 1",
            )
            .map_err(|e| AppError::Database(format!("读取 AUTOINCREMENT 序列失败: {e}")))?;
        for table in tables {
            target_conn
                .execute("DELETE FROM sqlite_sequence WHERE name = ?1", [*table])
                .map_err(|e| {
                    AppError::Database(format!("清理表 {table} 的 AUTOINCREMENT 序列失败: {e}"))
                })?;

            let mut rows = source_stmt
                .query([*table])
                .map_err(|e| AppError::Database(format!("查询表 {table} 序列失败: {e}")))?;
            if let Some(row) = rows.next().map_err(|e| AppError::Database(e.to_string()))? {
                let sequence = row
                    .get::<_, rusqlite::types::Value>(0)
                    .map_err(|e| AppError::Database(format!("解析表 {table} 序列失败: {e}")))?;
                target_conn
                    .execute(
                        "INSERT INTO sqlite_sequence (name, seq) VALUES (?1, ?2)",
                        rusqlite::params![table, sequence],
                    )
                    .map_err(|e| {
                        AppError::Database(format!("恢复表 {table} 的 AUTOINCREMENT 序列失败: {e}"))
                    })?;
            }
        }
        Ok(())
    }

    /// Create a backup if the latest one is older than the configured interval.
    fn backup_if_needed(&self) -> Result<(), AppError> {
        let interval_hours = crate::settings::effective_backup_interval_hours();
        if interval_hours > 0 {
            let backup_file_guard = lock_backup_file_operations();
            let backup_dir = get_app_config_dir().join("backups");
            if !backup_dir.exists() {
                self.backup_database_file_locked(&backup_file_guard)?;
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
                    self.backup_database_file_locked(&backup_file_guard)?;
                }
            }
        }

        Ok(())
    }

    /// Startup/daily maintenance. Cleanup also runs when backup is disabled or
    /// fails, so a broken backup destination cannot prevent log retention.
    pub(crate) fn periodic_backup_if_needed(&self) -> Result<(), AppError> {
        let backup_result = self.backup_if_needed();
        // Retention otherwise only runs when a new backup is published. Run it
        // here too, so a size limit the user just picked and temp files left
        // by a crashed backup don't wait for the next backup.
        if let Err(e) = Self::tidy_backup_dir() {
            log::warn!("Periodic backup retention failed: {e}");
        }
        if let Err(e) = self.cleanup_old_stream_check_logs(7) {
            log::warn!("Periodic stream_check_logs cleanup failed: {e}");
        }
        if let Err(e) = self.rollup_and_prune(30) {
            log::warn!("Periodic rollup_and_prune failed: {e}");
        }
        // Also reclaim pages left by deletions before the last shutdown.
        {
            let conn = lock_conn!(self.conn);
            if let Err(e) = Self::incremental_vacuum_on_conn(&conn) {
                log::warn!("Periodic incremental vacuum failed: {e}");
            }
        }

        backup_result
    }

    /// 生成一致性快照备份，返回备份文件路径（不存在主库时返回 None）
    pub(crate) fn backup_database_file(&self) -> Result<Option<PathBuf>, AppError> {
        let backup_file_guard = lock_backup_file_operations();
        self.backup_database_file_locked(&backup_file_guard)
    }

    fn backup_database_file_locked(
        &self,
        backup_file_guard: &BackupFileOperationGuard,
    ) -> Result<Option<PathBuf>, AppError> {
        let conn = lock_conn!(self.conn);
        Self::backup_database_file_from_conn(backup_file_guard, &conn, &[])
    }

    /// Create a safety backup from a connection whose caller already owns both
    /// the backup-file operation guard and the appropriate database guard.
    fn backup_database_file_from_conn(
        backup_file_guard: &BackupFileOperationGuard,
        source_conn: &Connection,
        protected_paths: &[&Path],
    ) -> Result<Option<PathBuf>, AppError> {
        Self::backup_database_file_from_conn_with_hook(
            backup_file_guard,
            source_conn,
            protected_paths,
            |_, _| Ok(()),
        )
    }

    fn backup_database_file_from_conn_with_hook<F>(
        backup_file_guard: &BackupFileOperationGuard,
        source_conn: &Connection,
        protected_paths: &[&Path],
        before_publish: F,
    ) -> Result<Option<PathBuf>, AppError>
    where
        F: FnOnce(&Path, &Path) -> Result<(), AppError>,
    {
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
        let mut next_suffix = 0;
        let mut backup_path =
            Self::next_available_backup_path(&backup_dir, &base_id, &mut next_suffix);

        // Build and validate the backup under a non-.db temporary name. Backup
        // discovery and retention only see the final path after the complete
        // SQLite image has been atomically published.
        let mut temp_path = Builder::new()
            .prefix(BACKUP_TEMP_PREFIX)
            .suffix(".tmp")
            .tempfile_in(&backup_dir)
            .map_err(|e| AppError::io(&backup_dir, e))?
            .into_temp_path();
        let temp_db_path: &Path = temp_path.as_ref();
        let mut dest_conn =
            Connection::open(temp_db_path).map_err(|e| AppError::Database(e.to_string()))?;
        let backup = Backup::new(source_conn, &mut dest_conn)
            .map_err(|e| AppError::Database(e.to_string()))?;
        Self::complete_backup(&backup, "创建数据库安全备份")?;
        drop(backup);
        Self::validate_sqlite_integrity(&dest_conn)?;
        dest_conn
            .close()
            .map_err(|(_, e)| AppError::Database(format!("关闭数据库安全备份失败: {e}")))?;
        before_publish(temp_db_path, &backup_path)?;

        loop {
            match temp_path.persist_noclobber(&backup_path) {
                Ok(()) => break,
                Err(error) if error.error.kind() == std::io::ErrorKind::AlreadyExists => {
                    temp_path = error.path;
                    backup_path =
                        Self::next_available_backup_path(&backup_dir, &base_id, &mut next_suffix);
                }
                Err(error) => return Err(AppError::io(&backup_path, error.error)),
            }
        }

        // The newly created safety backup must never be the cleanup victim.
        // During restore, the selected source is protected as well. If the
        // configured retention is too small to keep both, temporarily exceed
        // it instead of deleting either side of the recovery operation.
        let mut cleanup_protected = Vec::with_capacity(protected_paths.len() + 1);
        cleanup_protected.push(backup_path.as_path());
        cleanup_protected.extend_from_slice(protected_paths);
        Self::cleanup_db_backups(backup_file_guard, &backup_dir, &cleanup_protected)?;
        Ok(Some(backup_path))
    }

    fn next_available_backup_path(
        backup_dir: &Path,
        base_id: &str,
        next_suffix: &mut usize,
    ) -> PathBuf {
        loop {
            let backup_id = if *next_suffix == 0 {
                base_id.to_string()
            } else {
                format!("{base_id}_{}", *next_suffix)
            };
            *next_suffix += 1;
            let backup_path = backup_dir.join(format!("{backup_id}.db"));
            if !backup_path.exists() {
                return backup_path;
            }
        }
    }

    fn same_existing_backup_path(left: &Path, right: &Path) -> bool {
        match (fs::canonicalize(left), fs::canonicalize(right)) {
            (Ok(left), Ok(right)) => left == right,
            _ => left == right,
        }
    }

    /// 清理旧的数据库备份：保留最新的 N 个；用户设置了总大小上限时，再从最旧的
    /// 开始删除，直到备份目录不超过上限。
    ///
    /// 最新的一份备份和 `protected_paths`（刚创建的安全备份、本次恢复源）永远
    /// 不删：宁可暂时超出数量或大小上限，也要留下可用的恢复点。
    fn cleanup_db_backups(
        backup_file_guard: &BackupFileOperationGuard,
        dir: &Path,
        protected_paths: &[&Path],
    ) -> Result<(), AppError> {
        let retain = crate::settings::effective_backup_retain_count();
        let max_total_bytes = crate::settings::effective_backup_max_total_bytes();
        let mut backups = match fs::read_dir(dir) {
            Ok(iter) => iter
                .filter_map(|entry| entry.ok())
                .filter(|entry| {
                    entry
                        .path()
                        .extension()
                        .map(|ext| ext == "db")
                        .unwrap_or(false)
                })
                .map(|entry| {
                    let metadata = entry.metadata().ok();
                    let modified = metadata.as_ref().and_then(|m| m.modified().ok());
                    let len = metadata.map(|m| m.len()).unwrap_or(0);
                    (entry.path(), modified, len)
                })
                .collect::<Vec<_>>(),
            Err(_) => return Ok(()),
        };

        // 最旧的排在前面
        backups.sort_by_key(|(_, modified, _)| *modified);
        let newest = backups.last().map(|(path, _, _)| path.clone());
        let mut kept_count = backups.len();
        let mut kept_bytes: u64 = backups.iter().map(|(_, _, len)| len).sum();

        for (path, _, len) in &backups {
            let over_count = kept_count > retain;
            let over_size = max_total_bytes.is_some_and(|max| kept_bytes > max);
            if !over_count && !over_size {
                break;
            }
            if newest.as_ref() == Some(path)
                || protected_paths
                    .iter()
                    .any(|protected| Self::same_existing_backup_path(path, protected))
            {
                continue;
            }

            match Self::remove_backup_file(backup_file_guard, path) {
                Ok(()) => {
                    kept_count -= 1;
                    kept_bytes = kept_bytes.saturating_sub(*len);
                }
                Err(err) => log::warn!("删除旧数据库备份失败 {}: {}", path.display(), err),
            }
        }
        Ok(())
    }

    /// 删除一份备份，以及 SQLite 留在它旁边的 `-journal`/`-wal`/`-shm` 文件。
    /// 这些文件只属于同名的 `.db`；调用方持有备份文件操作锁，没有进行中的备份
    /// 或恢复会用到它们。`.db` 本身删除失败时（例如正被其他程序打开），旁路
    /// 文件原样保留。
    fn remove_backup_file(
        _backup_file_guard: &BackupFileOperationGuard,
        path: &Path,
    ) -> std::io::Result<()> {
        fs::remove_file(path)?;
        for suffix in SQLITE_SIDECAR_SUFFIXES {
            let mut sidecar = path.as_os_str().to_owned();
            sidecar.push(suffix);
            let sidecar = PathBuf::from(sidecar);
            match fs::remove_file(&sidecar) {
                Ok(()) => {}
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                Err(err) => {
                    log::warn!("删除数据库备份旁路文件失败 {}: {}", sidecar.display(), err)
                }
            }
        }
        Ok(())
    }

    /// 不创建新备份时的备份目录整理：清掉崩溃遗留的临时文件；用户设置了总大小
    /// 上限时再执行一次保留策略。未设置上限时不按数量清理，保持升级前的行为。
    fn tidy_backup_dir() -> Result<(), AppError> {
        let backup_file_guard = lock_backup_file_operations();
        let backup_dir = get_app_config_dir().join("backups");
        if !backup_dir.exists() {
            return Ok(());
        }
        Self::remove_orphan_backup_temp_files(&backup_file_guard, &backup_dir);
        if crate::settings::effective_backup_max_total_bytes().is_some() {
            Self::cleanup_db_backups(&backup_file_guard, &backup_dir, &[])?;
        }
        Ok(())
    }

    /// 删除崩溃遗留的备份临时文件（`.cc-switch-backup-*.tmp` 及其旁路文件）。
    /// 进行中的备份持有备份文件操作锁并持续写入临时文件，所以持锁时只有超过
    /// `ORPHAN_BACKUP_TEMP_MIN_AGE` 未修改的临时文件才会被删除。
    fn remove_orphan_backup_temp_files(_backup_file_guard: &BackupFileOperationGuard, dir: &Path) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.filter_map(|entry| entry.ok()) {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            let is_backup_temp = name.starts_with(BACKUP_TEMP_PREFIX)
                && (name.ends_with(".tmp")
                    || SQLITE_SIDECAR_SUFFIXES
                        .iter()
                        .any(|suffix| name.ends_with(&format!(".tmp{suffix}"))));
            if !is_backup_temp {
                continue;
            }
            let stale = entry
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|modified| modified.elapsed().ok())
                .is_some_and(|age| age >= ORPHAN_BACKUP_TEMP_MIN_AGE);
            if !stale {
                continue;
            }
            let path = entry.path();
            match fs::remove_file(&path) {
                Ok(()) => log::info!("Removed orphaned backup temp file: {}", path.display()),
                Err(err) => log::warn!("删除遗留的备份临时文件失败 {}: {}", path.display(), err),
            }
        }
    }

    fn validate_sqlite_integrity(conn: &Connection) -> Result<(), AppError> {
        let mut stmt = conn
            .prepare("PRAGMA quick_check;")
            .map_err(|e| AppError::Database(format!("检查数据库完整性失败: {e}")))?;
        let results = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|e| AppError::Database(format!("检查数据库完整性失败: {e}")))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| AppError::Database(format!("检查数据库完整性失败: {e}")))?;

        if results.len() == 1 && results[0].eq_ignore_ascii_case("ok") {
            return Ok(());
        }

        Err(AppError::localized(
            "backup.db.integrity_failed",
            format!("数据库备份完整性检查失败: {}", results.join("; ")),
            format!(
                "Database backup integrity check failed: {}",
                results.join("; ")
            ),
        ))
    }

    /// Validate that the external SQL created a recognizable CC Switch schema.
    ///
    /// These tables all existed in the oldest supported SQL-export schema
    /// (v3.8.x). Checking before migrations keeps header-only/truncated files
    /// from being completed by `create_tables_on_conn`, while allowing a valid
    /// backup whose user-owned configuration tables happen to contain no rows.
    fn validate_imported_schema(conn: &Connection) -> Result<(), AppError> {
        const REQUIRED_TABLES: &[&str] = &[
            "providers",
            "provider_endpoints",
            "mcp_servers",
            "prompts",
            "skills",
            "skill_repos",
            "settings",
        ];

        let mut missing = Vec::new();
        for table in REQUIRED_TABLES {
            if !Self::table_exists(conn, table)? {
                missing.push(*table);
            }
        }
        if !missing.is_empty() {
            let names = missing.join(", ");
            return Err(AppError::localized(
                "backup.sql.invalid_schema",
                format!("导入的 SQL 缺少 CC Switch 必需表：{names}"),
                format!("The imported SQL is missing required CC Switch tables: {names}"),
            ));
        }

        // 纵深防御：CC Switch 生产 schema 不含 trigger/view，即便 authorizer 被绕过也不允许落盘
        let disallowed_objects: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type IN ('trigger', 'view')",
                [],
                |row| row.get(0),
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
        if disallowed_objects > 0 {
            return Err(AppError::localized(
                "backup.sql.disallowed_schema_objects",
                "导入的 SQL 包含不允许的触发器或视图对象",
                "The imported SQL contains disallowed trigger or view objects",
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

        Self::dump_sqlite_sequences(conn, skip_tables, &mut output)?;

        // Triggers must be created after loading table data so they cannot
        // change dump rows or abandon the remainder of a multi-row INSERT.
        for sql in triggers {
            output.push_str(&sql);
            output.push_str(";\n");
        }

        output.push_str("COMMIT;\nPRAGMA foreign_keys=ON;\n");
        Ok(output)
    }

    fn dump_sqlite_sequences(
        conn: &Connection,
        skip_tables: &[&str],
        output: &mut String,
    ) -> Result<(), AppError> {
        if !Self::table_exists(conn, "sqlite_sequence")? {
            return Ok(());
        }

        let mut stmt = conn
            .prepare("SELECT name, seq FROM sqlite_sequence ORDER BY name")
            .map_err(|e| AppError::Database(format!("读取 AUTOINCREMENT 序列失败: {e}")))?;
        let mut rows = stmt
            .query([])
            .map_err(|e| AppError::Database(format!("查询 AUTOINCREMENT 序列失败: {e}")))?;
        let mut values = Vec::new();
        while let Some(row) = rows.next().map_err(|e| AppError::Database(e.to_string()))? {
            let table: String = row
                .get(0)
                .map_err(|e| AppError::Database(format!("解析 AUTOINCREMENT 表名失败: {e}")))?;
            if skip_tables.iter().any(|skipped| *skipped == table) {
                continue;
            }
            let sequence = row
                .get_ref(1)
                .map_err(|e| AppError::Database(format!("解析表 {table} 序列失败: {e}")))?;
            values.push(format!(
                "({}, {})",
                Self::format_sql_value(ValueRef::Text(table.as_bytes()))?,
                Self::format_sql_value(sequence)?
            ));
        }

        // Data INSERTs update sqlite_sequence to MAX(rowid), which loses a
        // deleted high-water mark. Replace those derived values with the exact
        // source metadata after all user-table rows have been loaded.
        output.push_str("DELETE FROM sqlite_sequence;\n");
        if !values.is_empty() {
            output.push_str("INSERT INTO sqlite_sequence (name, seq) VALUES ");
            output.push_str(&values.join(","));
            output.push_str(";\n");
        }
        Ok(())
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
            ValueRef::Real(f) => Ok(Self::format_sql_real(f)),
            ValueRef::Text(t) => match std::str::from_utf8(t) {
                // SQLite's SQL parser treats NUL as the end of the statement.
                // Keep readable literals for normal UTF-8, and use a hex cast
                // whenever a TEXT value cannot safely appear in SQL source.
                Ok(text) if !text.contains('\0') => {
                    let escaped = text.replace('\'', "''");
                    Ok(format!("'{escaped}'"))
                }
                _ => Ok(format!("CAST({} AS TEXT)", Self::format_sql_blob(t))),
            },
            ValueRef::Blob(bytes) => Ok(Self::format_sql_blob(bytes)),
        }
    }

    fn format_sql_real(value: f64) -> String {
        if value.is_nan() {
            // SQLite normalizes bound NaN values to NULL as well.
            return "NULL".to_string();
        }
        if value.is_infinite() {
            return if value.is_sign_negative() {
                "-9.0e999".to_string()
            } else {
                "9.0e999".to_string()
            };
        }
        if value == 0.0 && value.is_sign_negative() {
            return "-0.0".to_string();
        }

        let mut literal = value.to_string();
        if !literal.contains(['.', 'e', 'E']) {
            // Without a decimal point/exponent SQLite stores integer-valued
            // REALs as INTEGER in columns without REAL affinity (e.g. STRICT ANY).
            literal.push_str(".0");
        }
        literal
    }

    fn format_sql_blob(bytes: &[u8]) -> String {
        let mut s = String::from("X'");
        for b in bytes {
            use std::fmt::Write;
            let _ = write!(&mut s, "{b:02X}");
        }
        s.push('\'');
        s
    }

    /// List all database backup files, sorted by creation time (newest first)
    pub fn list_backups() -> Result<Vec<BackupEntry>, AppError> {
        let _backup_file_guard = lock_backup_file_operations();
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
        self.restore_from_backup_with_hook(filename, |_| Ok(()))
    }

    /// Validate that a backup filename is a single normal `.db` filename with no path
    /// separators, traversal segments, NULs, or (on Windows) drive prefixes / NTFS stream colons.
    fn validate_backup_filename(filename: &str) -> Result<(), AppError> {
        let mut components = Path::new(filename).components();
        let is_single_normal = matches!(
            (components.next(), components.next()),
            (Some(std::path::Component::Normal(part)), None) if part == filename
        );
        if !is_single_normal
            || filename.contains("..")
            || filename.contains('/')
            || filename.contains('\\')
            || (cfg!(windows) && filename.contains(':'))
            || filename.contains('\0')
            || !filename.ends_with(".db")
            || filename == ".db"
        {
            return Err(AppError::InvalidInput(
                "Invalid backup filename".to_string(),
            ));
        }
        Ok(())
    }

    fn restore_from_backup_with_hook<F>(
        &self,
        filename: &str,
        before_replace: F,
    ) -> Result<String, AppError>
    where
        F: FnOnce(Option<&Path>) -> Result<(), AppError>,
    {
        // Security: validate filename to prevent path traversal and drive-relative paths
        Self::validate_backup_filename(filename)?;

        let backup_file_guard = lock_backup_file_operations();
        let backup_dir = get_app_config_dir().join("backups");
        let backup_path = backup_dir.join(filename);

        if !backup_path.exists() {
            return Err(AppError::InvalidInput(format!(
                "Backup file not found: {filename}"
            )));
        }

        // Open read-only before creating the safety backup. `Connection::open`
        // would recreate a source removed by retention cleanup as an empty DB.
        let source_conn = Connection::open_with_flags(
            &backup_path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        // Stage and fully validate the selected file before touching the live
        // connection. A corrupt/future-schema backup or failed migration must
        // leave the current database unchanged.
        let temp_file = NamedTempFile::new().map_err(|e| AppError::IoContext {
            context: "创建数据库恢复暂存文件失败".to_string(),
            source: e,
        })?;
        let mut staging_conn =
            Connection::open(temp_file.path()).map_err(|e| AppError::Database(e.to_string()))?;
        {
            let backup = Backup::new(&source_conn, &mut staging_conn)
                .map_err(|e| AppError::Database(e.to_string()))?;
            Self::complete_backup(&backup, "读取数据库备份")?;
        }
        drop(source_conn);

        Self::validate_sqlite_integrity(&staging_conn)?;
        Self::validate_imported_schema(&staging_conn)?;
        Self::ensure_incremental_auto_vacuum_on_conn(&staging_conn)?;
        Self::create_tables_on_conn(&staging_conn)?;
        Self::apply_schema_migrations_on_conn(&staging_conn)?;
        Self::ensure_model_pricing_seeded_on_conn(&staging_conn)?;
        Self::validate_sqlite_integrity(&staging_conn)?;

        // Keep one main-DB guard across the safety snapshot and final apply so
        // the safety file exactly represents the state being replaced.
        let safety_backup = {
            let mut main_conn = lock_conn!(self.conn);
            let safety_backup = Self::backup_database_file_from_conn(
                &backup_file_guard,
                &main_conn,
                &[backup_path.as_path()],
            )?;
            before_replace(safety_backup.as_deref())?;
            self.replace_from_staging(&staging_conn, &mut main_conn, "恢复主数据库")?;
            safety_backup
        };
        let safety_id = safety_backup
            .and_then(|p| p.file_stem().map(|s| s.to_string_lossy().to_string()))
            .unwrap_or_default();

        log::info!("Database restored from backup: {filename}, safety backup: {safety_id}");
        Ok(safety_id)
    }

    /// Rename a backup file. Returns the new filename.
    pub fn rename_backup(old_filename: &str, new_name: &str) -> Result<String, AppError> {
        // Validate old filename (path traversal, drive prefix, + .db suffix)
        Self::validate_backup_filename(old_filename)?;

        // Clean new name
        let trimmed = new_name.trim();
        if trimmed.is_empty() {
            return Err(AppError::InvalidInput(
                "New name cannot be empty".to_string(),
            ));
        }

        // Length limit (without .db suffix)
        let name_part = trimmed.strip_suffix(".db").unwrap_or(trimmed);
        if name_part.is_empty() {
            return Err(AppError::InvalidInput(
                "New name cannot be empty".to_string(),
            ));
        }
        if name_part.len() > 100 {
            return Err(AppError::InvalidInput(
                "Name too long (max 100 characters)".to_string(),
            ));
        }

        // Prevent path traversal, drive-relative colons, and NUL bytes in new name
        if name_part.contains("..")
            || name_part.contains('/')
            || name_part.contains('\\')
            || name_part.contains(':')
            || name_part.contains('\0')
        {
            return Err(AppError::InvalidInput(
                "Invalid characters in new name".to_string(),
            ));
        }

        let new_filename = format!("{name_part}.db");
        Self::validate_backup_filename(&new_filename)?;

        let _backup_file_guard = lock_backup_file_operations();
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
        // Validate filename (path traversal, drive prefix, + .db suffix)
        Self::validate_backup_filename(filename)?;

        let backup_file_guard = lock_backup_file_operations();
        let backup_path = get_app_config_dir().join("backups").join(filename);
        if !backup_path.exists() {
            return Err(AppError::InvalidInput(format!(
                "Backup file not found: {filename}"
            )));
        }

        Self::remove_backup_file(&backup_file_guard, &backup_path)
            .map_err(|e| AppError::io(&backup_path, e))?;
        log::info!("Deleted backup: {filename}");
        Ok(())
    }
}

#[cfg(test)]
mod tests;
