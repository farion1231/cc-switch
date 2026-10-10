//! API Key 变量 DAO。每次写入后同步更新进程内的值缓存
//! （`services::api_key_vars::resolve`）。

use rusqlite::{params, OptionalExtension, Row};

use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::services::api_key_vars::resolve;
use crate::services::api_key_vars::runner::{ApiKeyRefreshConfig, ApiKeyRefreshState};

/// 数据库中的一条变量记录（包含明文值，绝不直接返回前端）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiKeyVarRecord {
    pub name: String,
    pub config: ApiKeyRefreshConfig,
    pub value: Option<String>,
    pub state: ApiKeyRefreshState,
    pub sort_index: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

const COLUMNS: &str = "name, script_dir, enabled, timeout_secs, lead_time_secs, value, state_json, sort_index, created_at, updated_at";

fn db_err(e: rusqlite::Error) -> AppError {
    AppError::Database(e.to_string())
}

fn from_row(row: &Row<'_>) -> rusqlite::Result<ApiKeyVarRecord> {
    let state_json: String = row.get(6)?;
    let timeout: Option<i64> = row.get(3)?;
    let lead: Option<i64> = row.get(4)?;
    Ok(ApiKeyVarRecord {
        name: row.get(0)?,
        config: ApiKeyRefreshConfig {
            script_dir: row.get(1)?,
            enabled: row.get::<_, i64>(2)? != 0,
            timeout_secs: timeout.map(|v| v.max(0) as u64),
            lead_time_secs: lead.map(|v| v.max(0) as u64),
        },
        value: row.get::<_, Option<String>>(5)?.filter(|v| !v.is_empty()),
        state: serde_json::from_str(&state_json).unwrap_or_default(),
        sort_index: row.get(7)?,
        created_at: row.get(8)?,
        updated_at: row.get(9)?,
    })
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

impl Database {
    pub fn list_api_key_vars(&self) -> Result<Vec<ApiKeyVarRecord>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn
            .prepare(&format!(
                "SELECT {COLUMNS} FROM api_key_vars ORDER BY sort_index, created_at, name"
            ))
            .map_err(db_err)?;
        let rows = stmt.query_map([], from_row).map_err(db_err)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(db_err)
    }

    pub fn get_api_key_var(&self, name: &str) -> Result<Option<ApiKeyVarRecord>, AppError> {
        let conn = lock_conn!(self.conn);
        conn.query_row(
            &format!("SELECT {COLUMNS} FROM api_key_vars WHERE name = ?1"),
            params![name],
            from_row,
        )
        .optional()
        .map_err(db_err)
    }

    /// 新增或更新变量定义（`original` 为改名前的名字）。保留当前值与状态；
    /// 配置变化时清除失败计数与“需要处理”标记。
    pub fn upsert_api_key_var(
        &self,
        original: Option<&str>,
        name: &str,
        config: &ApiKeyRefreshConfig,
    ) -> Result<ApiKeyVarRecord, AppError> {
        let now = now_ms();
        let lookup_name = original.unwrap_or(name);
        let existing = self.get_api_key_var(lookup_name)?;
        let record = {
            let mut conn = lock_conn!(self.conn);
            let tx = conn.transaction().map_err(db_err)?;
            let record = match existing {
                Some(mut rec) => {
                    if rec.config != *config {
                        rec.state.consecutive_failures = 0;
                        rec.state.needs_attention = false;
                        rec.state.last_error = None;
                    }
                    rec.config = config.clone();
                    rec.updated_at = now;
                    tx.execute(
                        "UPDATE api_key_vars SET name = ?1, script_dir = ?2, enabled = ?3,
                            timeout_secs = ?4, lead_time_secs = ?5, state_json = ?6, updated_at = ?7
                         WHERE name = ?8",
                        params![
                            name,
                            config.script_dir,
                            config.enabled as i64,
                            config.timeout_secs.map(|v| v as i64),
                            config.lead_time_secs.map(|v| v as i64),
                            serde_json::to_string(&rec.state).unwrap_or_else(|_| "{}".into()),
                            now,
                            lookup_name,
                        ],
                    )
                    .map_err(db_err)?;
                    rec.name = name.to_string();
                    rec
                }
                None => {
                    let sort_index: i64 = tx
                        .query_row(
                            "SELECT COALESCE(MAX(sort_index), -1) + 1 FROM api_key_vars",
                            [],
                            |r| r.get(0),
                        )
                        .map_err(db_err)?;
                    tx.execute(
                        &format!(
                            "INSERT INTO api_key_vars ({COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, NULL, '{{}}', ?6, ?7, ?7)"
                        ),
                        params![
                            name,
                            config.script_dir,
                            config.enabled as i64,
                            config.timeout_secs.map(|v| v as i64),
                            config.lead_time_secs.map(|v| v as i64),
                            sort_index,
                            now,
                        ],
                    )
                    .map_err(db_err)?;
                    ApiKeyVarRecord {
                        name: name.to_string(),
                        config: config.clone(),
                        value: None,
                        state: ApiKeyRefreshState::default(),
                        sort_index,
                        created_at: now,
                        updated_at: now,
                    }
                }
            };
            tx.commit().map_err(db_err)?;
            record
        };
        if lookup_name != name {
            resolve::cache_rename(lookup_name, name);
        }
        resolve::cache_set(name, record.config.enabled, record.value.as_deref());
        Ok(record)
    }

    pub fn delete_api_key_var(&self, name: &str) -> Result<bool, AppError> {
        let n = {
            let conn = lock_conn!(self.conn);
            conn.execute("DELETE FROM api_key_vars WHERE name = ?1", params![name])
                .map_err(db_err)?
        };
        resolve::cache_remove(name);
        Ok(n > 0)
    }

    /// 写入一次刷新结果（`value` 为 None 时保留旧值）
    pub fn update_api_key_var_result(
        &self,
        name: &str,
        value: Option<&str>,
        state: &ApiKeyRefreshState,
    ) -> Result<(), AppError> {
        let enabled = {
            let conn = lock_conn!(self.conn);
            let state_json = serde_json::to_string(state).unwrap_or_else(|_| "{}".into());
            conn.execute(
                "UPDATE api_key_vars SET value = COALESCE(?1, value), state_json = ?2, updated_at = ?3 WHERE name = ?4",
                params![value, state_json, now_ms(), name],
            )
            .map_err(db_err)?;
            conn.query_row(
                "SELECT enabled FROM api_key_vars WHERE name = ?1",
                params![name],
                |r| r.get::<_, i64>(0),
            )
            .optional()
            .map_err(db_err)?
        };
        if let Some(enabled) = enabled {
            if let Some(v) = value {
                resolve::cache_set(name, enabled != 0, Some(v));
            }
        }
        Ok(())
    }

    /// 启动时把全部变量加载进值缓存
    pub fn load_api_key_var_cache(&self) -> Result<usize, AppError> {
        let vars = self.list_api_key_vars()?;
        let n = vars.len();
        resolve::cache_load(
            vars.into_iter()
                .map(|v| (v.name, v.config.enabled, v.value)),
        );
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(dir: &str, enabled: bool) -> ApiKeyRefreshConfig {
        ApiKeyRefreshConfig {
            enabled,
            script_dir: dir.into(),
            timeout_secs: Some(10),
            lead_time_secs: None,
        }
    }

    #[test]
    fn crud_and_cache() -> Result<(), AppError> {
        let db = Database::memory()?;
        db.upsert_api_key_var(None, "DAO_T_ONE", &cfg("/a", true))?;
        db.upsert_api_key_var(None, "DAO_T_TWO", &cfg("/b", false))?;
        let list = db.list_api_key_vars()?;
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].name, "DAO_T_ONE");
        assert_eq!(list[0].config.timeout_secs, Some(10));

        let st = ApiKeyRefreshState {
            expires_at: Some(123),
            last_success_at: Some(1),
            ..Default::default()
        };
        db.update_api_key_var_result("DAO_T_ONE", Some("sk-dao"), &st)?;
        let rec = db.get_api_key_var("DAO_T_ONE")?.unwrap();
        assert_eq!(rec.value.as_deref(), Some("sk-dao"));
        assert_eq!(rec.state.expires_at, Some(123));
        assert_eq!(
            resolve::lookup("DAO_T_ONE"),
            (resolve::RefStatus::Ok, Some("sk-dao".into()))
        );

        // 失败结果不覆盖值
        db.update_api_key_var_result(
            "DAO_T_ONE",
            None,
            &ApiKeyRefreshState {
                consecutive_failures: 1,
                ..st.clone()
            },
        )?;
        assert_eq!(
            db.get_api_key_var("DAO_T_ONE")?.unwrap().value.as_deref(),
            Some("sk-dao")
        );

        // 改名保留值；改配置清除失败计数
        db.upsert_api_key_var(Some("DAO_T_ONE"), "DAO_T_RENAMED", &cfg("/c", true))?;
        assert!(db.get_api_key_var("DAO_T_ONE")?.is_none());
        let rec = db.get_api_key_var("DAO_T_RENAMED")?.unwrap();
        assert_eq!(rec.value.as_deref(), Some("sk-dao"));
        assert_eq!(rec.state.consecutive_failures, 0);
        assert_eq!(resolve::lookup("DAO_T_ONE").0, resolve::RefStatus::Unknown);
        assert_eq!(resolve::lookup("DAO_T_RENAMED").0, resolve::RefStatus::Ok);

        // 停用后不可解析
        db.upsert_api_key_var(Some("DAO_T_RENAMED"), "DAO_T_RENAMED", &cfg("/c", false))?;
        assert_eq!(
            resolve::lookup("DAO_T_RENAMED").0,
            resolve::RefStatus::Disabled
        );

        assert!(db.delete_api_key_var("DAO_T_RENAMED")?);
        assert_eq!(
            resolve::lookup("DAO_T_RENAMED").0,
            resolve::RefStatus::Unknown
        );
        Ok(())
    }
}
