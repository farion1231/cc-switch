//! Check actual columns, not just user_version (branches can share version numbers).
use super::Database;
use crate::error::AppError;
use rusqlite::Connection;

// Additive fields only: main can ignore these without changing its table contracts.
const COMPATIBILITY_COLUMNS: &[(&str, &str, &str)] = &[
    ("skills", "enabled_mcode", "BOOLEAN NOT NULL DEFAULT 0"),
    ("mcp_servers", "enabled_mcode", "BOOLEAN NOT NULL DEFAULT 0"),
    ("session_log_sync", "last_byte_offset", "INTEGER"),
    ("session_log_sync", "last_tail_fingerprint", "INTEGER"),
    (
        "proxy_config",
        "live_takeover_active",
        "INTEGER NOT NULL DEFAULT 0",
    ),
    ("proxy_request_logs", "cost_known", "INTEGER"),
    (
        "usage_daily_rollups",
        "cost_incomplete",
        "INTEGER NOT NULL DEFAULT 1",
    ),
];

impl Database {
    pub(crate) fn ensure_compatibility_columns(conn: &Connection) -> Result<(), AppError> {
        for &(table, column, definition) in COMPATIBILITY_COLUMNS {
            if Self::table_exists(conn, table)? {
                Self::add_column_if_missing(conn, table, column, definition)?;
            }
        }
        Ok(())
    }

    pub(crate) fn missing_schema_columns(
        conn: &Connection,
    ) -> Result<Vec<(String, String)>, AppError> {
        let expected = Connection::open_in_memory()?;
        Self::create_tables_on_conn(&expected)?;
        let mut stmt = expected.prepare("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name")?;
        let tables = stmt
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        let mut missing = Vec::new();
        for table in tables {
            if !Self::table_exists(conn, &table)? {
                missing.push((table, String::new()));
                continue;
            }
            let quoted = table.replace('"', "\"\"");
            let mut stmt = expected.prepare(&format!("PRAGMA table_info(\"{quoted}\")"))?;
            let columns = stmt
                .query_map([], |row| row.get::<_, String>(1))?
                .collect::<Result<Vec<_>, _>>()?;
            for column in columns {
                if !Self::has_column(conn, &table, &column)? {
                    missing.push((table.clone(), column));
                }
            }
        }
        Ok(missing)
    }

    pub(crate) fn repair_schema_columns(conn: &Connection) -> Result<(), AppError> {
        let missing = Self::missing_schema_columns(conn)?;
        let mut repairs = Vec::new();
        for (table, column) in missing {
            // Absent tables are created from canonical DDL by create_tables_on_conn.
            if column.is_empty() {
                continue;
            }
            let definition = COMPATIBILITY_COLUMNS.iter()
                .find(|(t, c, _)| *t == table && *c == column)
                .map(|(_, _, definition)| *definition)
                .ok_or_else(|| AppError::Database(format!(
                    "数据库结构不完整：{table}.{column} 缺失，无法自动修复；原数据已保留，请从备份恢复或升级应用"
                )))?;
            repairs.push((table, column, definition));
        }
        // Validate all missing fields before making any changes.
        for (table, column, definition) in repairs {
            conn.execute_batch(&format!(
                "ALTER TABLE \"{table}\" ADD COLUMN \"{column}\" {definition}"
            ))?;
            log::info!("Repaired database column {table}.{column}");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_version_drift_repairs_without_losing_rows() -> Result<(), AppError> {
        let db = Database::memory()?;
        let conn = db.conn.lock().unwrap();
        conn.execute_batch("INSERT INTO settings(key,value) VALUES ('health-test','preserve'); ALTER TABLE skills DROP COLUMN enabled_mcode; ALTER TABLE mcp_servers DROP COLUMN enabled_mcode; ALTER TABLE session_log_sync DROP COLUMN last_byte_offset; ALTER TABLE session_log_sync DROP COLUMN last_tail_fingerprint;")?;
        Database::set_user_version(&conn, super::super::SCHEMA_VERSION)?;
        assert_eq!(Database::missing_schema_columns(&conn)?.len(), 4);
        Database::repair_schema_columns(&conn)?;
        Database::repair_schema_columns(&conn)?;
        assert!(Database::missing_schema_columns(&conn)?.is_empty());
        assert_eq!(
            conn.query_row(
                "SELECT value FROM settings WHERE key='health-test'",
                [],
                |r| r.get::<_, String>(0)
            )?,
            "preserve"
        );
        Ok(())
    }

    #[test]
    fn refuses_to_invent_required_data() -> Result<(), AppError> {
        let db = Database::memory()?;
        let conn = db.conn.lock().unwrap();
        conn.execute_batch(
            "ALTER TABLE skills DROP COLUMN enabled_mcode; ALTER TABLE providers DROP COLUMN name",
        )?;
        let error = Database::repair_schema_columns(&conn)
            .unwrap_err()
            .to_string();
        assert!(error.contains("providers.name"));
        assert!(!Database::has_column(&conn, "skills", "enabled_mcode")?);
        Ok(())
    }
    #[test]
    fn main_version_adds_extensions_without_bumping_version_or_resetting_costs(
    ) -> Result<(), AppError> {
        let db = Database::memory()?;
        let conn = db.conn.lock().unwrap();
        conn.execute_batch("DROP INDEX idx_request_logs_cost_pending;
            ALTER TABLE proxy_request_logs DROP COLUMN cost_known;
            ALTER TABLE usage_daily_rollups DROP COLUMN cost_incomplete;
            ALTER TABLE proxy_config DROP COLUMN live_takeover_active;
            INSERT INTO proxy_request_logs(request_id,provider_id,app_type,model,total_cost_usd,latency_ms,status_code,created_at)
            VALUES ('retained','p1','claude','test','2.5',100,200,1);")?;
        Database::set_user_version(&conn, 22)?;
        Database::repair_schema_columns(&conn)?;
        Database::apply_schema_migrations_on_conn(&conn)?;
        assert_eq!(Database::get_user_version(&conn)?, 22);
        assert!(Database::missing_schema_columns(&conn)?.is_empty());
        conn.execute(
            "UPDATE proxy_request_logs SET cost_known=1 WHERE request_id='retained'",
            [],
        )?;
        Database::apply_schema_migrations_on_conn(&conn)?;
        let saved: (String, bool) = conn.query_row(
            "SELECT total_cost_usd,cost_known FROM proxy_request_logs WHERE request_id='retained'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        assert_eq!(saved, ("2.5".to_string(), true));
        assert_eq!(Database::get_user_version(&conn)?, 22);
        Ok(())
    }
}
