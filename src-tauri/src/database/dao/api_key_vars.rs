//! API Key 变量 DAO。每次写入后同步更新进程内的值缓存
//! （`services::api_key_vars::resolve`）。

use std::collections::HashMap;
use std::str::FromStr;
use std::sync::atomic::{AtomicI64, Ordering};

use rusqlite::{params, OptionalExtension, Row};

use crate::app_config::AppType;
use crate::database::dao::universal_providers::{
    rewrite_api_key_var_refs_in_tx, UNIVERSAL_PROVIDERS_KEY,
};
use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::provider::UniversalProvider;
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

/// A persisted API-key field that prevents a variable from being deleted.
/// This is intentionally storage-only: callers must not treat it as a live
/// provider activation signal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiKeyVarReference {
    pub app: String,
    pub provider_id: String,
    pub provider_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeleteApiKeyVarOutcome {
    Deleted,
    NotFound,
    InUse(Vec<ApiKeyVarReference>),
}

const COLUMNS: &str = "name, script_dir, enabled, timeout_secs, lead_time_secs, value, state_json, sort_index, created_at, updated_at";

fn db_err(e: rusqlite::Error) -> AppError {
    AppError::Database(e.to_string())
}

fn from_row(row: &Row<'_>) -> rusqlite::Result<ApiKeyVarRecord> {
    let state_json: String = row.get(6)?;
    let timeout: Option<i64> = row.get(3)?;
    let lead: Option<i64> = row.get(4)?;
    let record = ApiKeyVarRecord {
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
    };
    observe_timestamp(record.created_at);
    observe_timestamp(record.updated_at);
    Ok(record)
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

// `updated_at` doubles as the refresh generation. Wall-clock milliseconds alone
// are not sufficient: a user can edit a variable twice during the same tick and
// return its configuration to its original value. Keep timestamps monotonic in
// this process so a refresh snapshot cannot pass that ABA transition.
static LAST_TIMESTAMP: AtomicI64 = AtomicI64::new(0);

fn observe_timestamp(timestamp: i64) {
    let mut previous = LAST_TIMESTAMP.load(Ordering::Relaxed);
    while timestamp > previous {
        match LAST_TIMESTAMP.compare_exchange_weak(
            previous,
            timestamp,
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            Ok(_) => return,
            Err(current) => previous = current,
        }
    }
}

fn next_timestamp(after: i64) -> i64 {
    let mut previous = LAST_TIMESTAMP.load(Ordering::Relaxed);
    loop {
        let next = now_ms()
            .max(after.saturating_add(1))
            .max(previous.saturating_add(1));
        match LAST_TIMESTAMP.compare_exchange_weak(
            previous,
            next,
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            Ok(_) => return next,
            Err(current) => previous = current,
        }
    }
}

fn references_in_tx(
    tx: &rusqlite::Transaction<'_>,
    name: &str,
) -> Result<Vec<ApiKeyVarReference>, AppError> {
    let provider_rows = {
        let mut stmt = tx
            .prepare(
                "SELECT id, app_type, name, settings_config FROM providers ORDER BY app_type, id",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            })
            .map_err(db_err)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(db_err)?
    };
    let mut refs = Vec::new();
    for (id, app_type, provider_name, raw_settings) in provider_rows {
        let Ok(app) = AppType::from_str(&app_type) else {
            continue;
        };
        let settings = serde_json::from_str(&raw_settings).map_err(|e| {
            AppError::Database(format!(
                "Failed to parse settings_config for {app_type}/{id}: {e}"
            ))
        })?;
        if resolve::referenced_vars(&app, &settings)
            .iter()
            .any(|reference| reference == name)
        {
            refs.push(ApiKeyVarReference {
                app: app_type,
                provider_id: id,
                provider_name,
            });
        }
    }

    let raw_universal: Option<String> = tx
        .query_row(
            "SELECT value FROM settings WHERE key = ?1",
            [UNIVERSAL_PROVIDERS_KEY],
            |row| row.get(0),
        )
        .optional()
        .map_err(db_err)?;
    if let Some(raw_universal) = raw_universal {
        let providers: HashMap<String, UniversalProvider> = serde_json::from_str(&raw_universal)
            .map_err(|e| AppError::Database(format!("解析统一供应商数据失败: {e}")))?;
        for provider in providers.values() {
            if resolve::parse_ref(&provider.api_key) == Some(name) {
                refs.push(ApiKeyVarReference {
                    app: "universal".to_string(),
                    provider_id: provider.id.clone(),
                    provider_name: provider.name.clone(),
                });
            }
        }
    }
    Ok(refs)
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
        let lookup_name = original.unwrap_or(name);
        let record = {
            let mut conn = lock_conn!(self.conn);
            let tx = conn.transaction().map_err(db_err)?;
            let existing = tx
                .query_row(
                    &format!("SELECT {COLUMNS} FROM api_key_vars WHERE name = ?1"),
                    params![lookup_name],
                    from_row,
                )
                .optional()
                .map_err(db_err)?;
            let now = next_timestamp(existing.as_ref().map_or(0, |record| record.updated_at));
            let record = match existing {
                Some(mut rec) => {
                    // A different script is a new source of truth. Keep the
                    // last key (and its real expiry for switch-time safety),
                    // but discard the old script's success/schedule markers
                    // so the scheduler runs the replacement immediately.
                    let script_changed = rec.config.script_dir != config.script_dir;
                    if rec.config != *config {
                        rec.state.consecutive_failures = 0;
                        rec.state.needs_attention = false;
                        rec.state.last_error = None;
                    }
                    if script_changed {
                        rec.state.last_success_at = None;
                        rec.state.refresh_after = None;
                        rec.state.resolved_script = None;
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
            if lookup_name != name {
                // Keep every stored template in the same transaction as the
                // variable rename. Updating through the public provider DAO
                // would acquire the connection again and could leave partial
                // rewrites behind on failure.
                let provider_rows = {
                    let mut stmt = tx
                        .prepare(
                            "SELECT id, app_type, settings_config FROM providers ORDER BY app_type, id",
                        )
                        .map_err(db_err)?;
                    let rows = stmt
                        .query_map([], |row| {
                            Ok((
                                row.get::<_, String>(0)?,
                                row.get::<_, String>(1)?,
                                row.get::<_, String>(2)?,
                            ))
                        })
                        .map_err(db_err)?;
                    rows.collect::<Result<Vec<_>, _>>().map_err(db_err)?
                };
                for (id, app_type, raw_settings) in provider_rows {
                    let Ok(app) = AppType::from_str(&app_type) else {
                        continue;
                    };
                    let settings = serde_json::from_str(&raw_settings).map_err(|e| {
                        AppError::Database(format!(
                            "Failed to parse settings_config for {app_type}/{id}: {e}"
                        ))
                    })?;
                    if let Some(rewritten) =
                        resolve::rewrite_refs(&app, &settings, lookup_name, name)
                    {
                        let json = serde_json::to_string(&rewritten).map_err(|e| {
                            AppError::Database(format!(
                                "Failed to serialize settings_config for {app_type}/{id}: {e}"
                            ))
                        })?;
                        tx.execute(
                            "UPDATE providers SET settings_config = ?1 WHERE id = ?2 AND app_type = ?3",
                            params![json, id, app_type],
                        )
                        .map_err(db_err)?;
                    }
                }
                rewrite_api_key_var_refs_in_tx(&tx, lookup_name, name)?;
            }
            tx.commit().map_err(db_err)?;
            if lookup_name != name {
                resolve::cache_rename(lookup_name, name);
            }
            resolve::cache_set(name, record.config.enabled, record.value.as_deref());
            record
        };
        Ok(record)
    }

    pub fn delete_api_key_var(&self, name: &str) -> Result<bool, AppError> {
        let n = {
            let conn = lock_conn!(self.conn);
            let previous = conn
                .query_row(
                    "SELECT created_at, updated_at FROM api_key_vars WHERE name = ?1",
                    params![name],
                    |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
                )
                .optional()
                .map_err(db_err)?;
            if let Some((created_at, updated_at)) = previous {
                observe_timestamp(created_at);
                observe_timestamp(updated_at);
            }
            let deleted = conn
                .execute("DELETE FROM api_key_vars WHERE name = ?1", params![name])
                .map_err(db_err)?;
            resolve::cache_remove(name);
            deleted
        };
        Ok(n > 0)
    }

    /// Delete a variable only when no persisted provider or unified-provider
    /// API-key field refers to it. The reference scan and DELETE share one
    /// transaction, closing the check-then-delete race in the service layer.
    pub fn delete_api_key_var_if_unused(
        &self,
        name: &str,
    ) -> Result<DeleteApiKeyVarOutcome, AppError> {
        let outcome = {
            let mut conn = lock_conn!(self.conn);
            let tx = conn.transaction().map_err(db_err)?;
            let timestamps = tx
                .query_row(
                    "SELECT created_at, updated_at FROM api_key_vars WHERE name = ?1",
                    params![name],
                    |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
                )
                .optional()
                .map_err(db_err)?;
            let Some((created_at, updated_at)) = timestamps else {
                return Ok(DeleteApiKeyVarOutcome::NotFound);
            };
            let references = references_in_tx(&tx, name)?;
            if !references.is_empty() {
                return Ok(DeleteApiKeyVarOutcome::InUse(references));
            }
            tx.execute("DELETE FROM api_key_vars WHERE name = ?1", params![name])
                .map_err(db_err)?;
            tx.commit().map_err(db_err)?;
            observe_timestamp(created_at);
            observe_timestamp(updated_at);
            resolve::cache_remove(name);
            DeleteApiKeyVarOutcome::Deleted
        };
        Ok(outcome)
    }

    /// 写入一次刷新结果（`value` 为 None 时保留旧值）
    pub fn update_api_key_var_result(
        &self,
        name: &str,
        value: Option<&str>,
        state: &ApiKeyRefreshState,
    ) -> Result<(), AppError> {
        {
            let conn = lock_conn!(self.conn);
            let state_json = serde_json::to_string(state).unwrap_or_else(|_| "{}".into());
            conn.execute(
                "UPDATE api_key_vars SET value = COALESCE(?1, value), state_json = ?2, updated_at = ?3 WHERE name = ?4",
                params![value, state_json, next_timestamp(0), name],
            )
            .map_err(db_err)?;
            let enabled = conn
                .query_row(
                    "SELECT enabled FROM api_key_vars WHERE name = ?1",
                    params![name],
                    |r| r.get::<_, i64>(0),
                )
                .optional()
                .map_err(db_err)?;
            if let Some(enabled) = enabled {
                if let Some(v) = value {
                    resolve::cache_set(name, enabled != 0, Some(v));
                }
            }
        }
        Ok(())
    }

    /// Atomically writes a refresh result only when `expected` still identifies
    /// the same, unedited variable. A false return means a running script became
    /// stale (edited, renamed, deleted, or recreated) and must be discarded.
    pub fn update_api_key_var_result_if_current(
        &self,
        expected: &ApiKeyVarRecord,
        value: Option<&str>,
        state: &ApiKeyRefreshState,
    ) -> Result<bool, AppError> {
        let state_json = serde_json::to_string(state).unwrap_or_else(|_| "{}".into());
        let updated = next_timestamp(expected.updated_at);
        let changed = {
            let conn = lock_conn!(self.conn);
            let changed = conn
                .execute(
                    "UPDATE api_key_vars
                 SET value = COALESCE(?1, value), state_json = ?2, updated_at = ?3
                 WHERE name = ?4 AND created_at = ?5 AND updated_at = ?6",
                    params![
                        value,
                        state_json,
                        updated,
                        expected.name,
                        expected.created_at,
                        expected.updated_at,
                    ],
                )
                .map_err(db_err)?
                > 0;
            if changed {
                if let Some(value) = value {
                    resolve::cache_set(&expected.name, expected.config.enabled, Some(value));
                }
            }
            changed
        };
        Ok(changed)
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
    use crate::app_config::AppType;
    use crate::provider::UniversalProvider;
    use crate::services::api_key_vars::runner::next_due_ms;

    fn cfg(dir: &str, enabled: bool) -> ApiKeyRefreshConfig {
        ApiKeyRefreshConfig {
            enabled,
            script_dir: dir.into(),
            timeout_secs: Some(10),
            lead_time_secs: None,
        }
    }

    fn insert_claude_provider(db: &Database, id: &str, key: &str) -> Result<(), AppError> {
        let conn = crate::database::lock_conn!(db.conn);
        conn.execute(
            "INSERT INTO providers (id, app_type, name, settings_config, meta)
             VALUES (?1, ?2, ?3, ?4, '{}')",
            params![
                id,
                AppType::Claude.as_str(),
                id,
                serde_json::json!({"env": {"ANTHROPIC_AUTH_TOKEN": key}}).to_string(),
            ],
        )
        .map_err(db_err)?;
        Ok(())
    }

    fn claude_provider_key(db: &Database, id: &str) -> Result<String, AppError> {
        let conn = crate::database::lock_conn!(db.conn);
        let raw: String = conn
            .query_row(
                "SELECT settings_config FROM providers WHERE id = ?1 AND app_type = ?2",
                params![id, AppType::Claude.as_str()],
                |row| row.get(0),
            )
            .map_err(db_err)?;
        serde_json::from_str::<serde_json::Value>(&raw)
            .map_err(|e| AppError::Database(e.to_string()))?
            .pointer("/env/ANTHROPIC_AUTH_TOKEN")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| AppError::Database("missing test provider key".to_string()))
    }

    fn save_universal_ref(db: &Database, id: &str, key: &str) -> Result<(), AppError> {
        db.save_universal_provider(&UniversalProvider::new(
            id.to_string(),
            id.to_string(),
            "custom".to_string(),
            "https://example.invalid/v1".to_string(),
            key.to_string(),
        ))
    }

    #[test]
    fn rename_rewrites_provider_and_universal_templates_atomically() -> Result<(), AppError> {
        let db = Database::memory()?;
        db.upsert_api_key_var(None, "DAO_T_RENAME_OLD", &cfg("/old", true))?;
        db.update_api_key_var_result(
            "DAO_T_RENAME_OLD",
            Some("test-key-value-old"),
            &ApiKeyRefreshState::default(),
        )?;
        db.update_api_key_var_result(
            "DAO_T_RENAME_OLD",
            Some("test-key-value"),
            &ApiKeyRefreshState::default(),
        )?;
        insert_claude_provider(&db, "rename-provider", "{{DAO_T_RENAME_OLD}}")?;
        save_universal_ref(&db, "rename-universal", "{{DAO_T_RENAME_OLD}}")?;

        db.upsert_api_key_var(
            Some("DAO_T_RENAME_OLD"),
            "DAO_T_RENAME_NEW",
            &cfg("/old", true),
        )?;

        assert_eq!(
            claude_provider_key(&db, "rename-provider")?,
            "{{DAO_T_RENAME_NEW}}"
        );
        assert_eq!(
            db.get_universal_provider("rename-universal")?
                .expect("universal provider")
                .api_key,
            "{{DAO_T_RENAME_NEW}}"
        );
        assert!(db.get_api_key_var("DAO_T_RENAME_OLD")?.is_none());
        assert_eq!(
            resolve::lookup("DAO_T_RENAME_NEW"),
            (resolve::RefStatus::Ok, Some("test-key-value".to_string()))
        );
        assert_eq!(
            resolve::known_values("DAO_T_RENAME_NEW"),
            vec![
                "test-key-value".to_string(),
                "test-key-value-old".to_string()
            ]
        );
        Ok(())
    }

    #[test]
    fn rename_rolls_back_when_provider_rewrite_fails() -> Result<(), AppError> {
        let db = Database::memory()?;
        db.upsert_api_key_var(None, "DAO_T_PROVIDER_ABORT_OLD", &cfg("/old", true))?;
        insert_claude_provider(&db, "provider-abort", "{{DAO_T_PROVIDER_ABORT_OLD}}")?;
        save_universal_ref(
            &db,
            "provider-abort-universal",
            "{{DAO_T_PROVIDER_ABORT_OLD}}",
        )?;
        {
            let conn = crate::database::lock_conn!(db.conn);
            conn.execute_batch(
                "CREATE TRIGGER api_key_var_provider_abort BEFORE UPDATE ON providers
                 BEGIN SELECT RAISE(ABORT, 'provider rewrite rejected'); END;",
            )
            .map_err(db_err)?;
        }

        assert!(db
            .upsert_api_key_var(
                Some("DAO_T_PROVIDER_ABORT_OLD"),
                "DAO_T_PROVIDER_ABORT_NEW",
                &cfg("/old", true),
            )
            .is_err());
        assert!(db.get_api_key_var("DAO_T_PROVIDER_ABORT_OLD")?.is_some());
        assert!(db.get_api_key_var("DAO_T_PROVIDER_ABORT_NEW")?.is_none());
        assert_eq!(
            claude_provider_key(&db, "provider-abort")?,
            "{{DAO_T_PROVIDER_ABORT_OLD}}"
        );
        assert_eq!(
            db.get_universal_provider("provider-abort-universal")?
                .expect("universal provider")
                .api_key,
            "{{DAO_T_PROVIDER_ABORT_OLD}}"
        );
        Ok(())
    }

    #[test]
    fn rename_rolls_back_when_universal_rewrite_fails() -> Result<(), AppError> {
        let db = Database::memory()?;
        db.upsert_api_key_var(None, "DAO_T_UNIVERSAL_ABORT_OLD", &cfg("/old", true))?;
        insert_claude_provider(&db, "universal-abort", "{{DAO_T_UNIVERSAL_ABORT_OLD}}")?;
        save_universal_ref(
            &db,
            "universal-abort-provider",
            "{{DAO_T_UNIVERSAL_ABORT_OLD}}",
        )?;
        {
            let conn = crate::database::lock_conn!(db.conn);
            conn.execute_batch(
                "CREATE TRIGGER api_key_var_universal_abort BEFORE UPDATE ON settings
                 WHEN NEW.key = 'universal_providers'
                 BEGIN SELECT RAISE(ABORT, 'universal rewrite rejected'); END;",
            )
            .map_err(db_err)?;
        }

        assert!(db
            .upsert_api_key_var(
                Some("DAO_T_UNIVERSAL_ABORT_OLD"),
                "DAO_T_UNIVERSAL_ABORT_NEW",
                &cfg("/old", true),
            )
            .is_err());
        assert!(db.get_api_key_var("DAO_T_UNIVERSAL_ABORT_OLD")?.is_some());
        assert!(db.get_api_key_var("DAO_T_UNIVERSAL_ABORT_NEW")?.is_none());
        assert_eq!(
            claude_provider_key(&db, "universal-abort")?,
            "{{DAO_T_UNIVERSAL_ABORT_OLD}}"
        );
        assert_eq!(
            db.get_universal_provider("universal-abort-provider")?
                .expect("universal provider")
                .api_key,
            "{{DAO_T_UNIVERSAL_ABORT_OLD}}"
        );
        Ok(())
    }

    #[test]
    fn guarded_delete_rechecks_provider_and_universal_references() -> Result<(), AppError> {
        let db = Database::memory()?;
        db.upsert_api_key_var(None, "DAO_T_DELETE_GUARD", &cfg("/delete", true))?;
        insert_claude_provider(&db, "delete-guard-provider", "{{DAO_T_DELETE_GUARD}}")?;
        save_universal_ref(&db, "delete-guard-universal", "{{DAO_T_DELETE_GUARD}}")?;

        let DeleteApiKeyVarOutcome::InUse(references) =
            db.delete_api_key_var_if_unused("DAO_T_DELETE_GUARD")?
        else {
            panic!("persisted references must prevent deletion");
        };
        assert_eq!(references.len(), 2);
        assert!(references.iter().any(|reference| {
            reference.app == "claude" && reference.provider_name == "delete-guard-provider"
        }));
        assert!(references.iter().any(|reference| {
            reference.app == "universal" && reference.provider_name == "delete-guard-universal"
        }));
        assert!(db.get_api_key_var("DAO_T_DELETE_GUARD")?.is_some());
        Ok(())
    }

    #[test]
    fn guarded_delete_keeps_cache_when_database_delete_rolls_back() -> Result<(), AppError> {
        let db = Database::memory()?;
        db.upsert_api_key_var(None, "DAO_T_DELETE_ABORT", &cfg("/delete", true))?;
        db.update_api_key_var_result(
            "DAO_T_DELETE_ABORT",
            Some("delete-guard-value"),
            &ApiKeyRefreshState::default(),
        )?;
        {
            let conn = crate::database::lock_conn!(db.conn);
            conn.execute_batch(
                "CREATE TRIGGER api_key_var_delete_abort BEFORE DELETE ON api_key_vars
                 BEGIN SELECT RAISE(ABORT, 'delete rejected'); END;",
            )
            .map_err(db_err)?;
        }

        assert!(db
            .delete_api_key_var_if_unused("DAO_T_DELETE_ABORT")
            .is_err());
        assert!(db.get_api_key_var("DAO_T_DELETE_ABORT")?.is_some());
        assert_eq!(
            resolve::lookup("DAO_T_DELETE_ABORT"),
            (
                resolve::RefStatus::Ok,
                Some("delete-guard-value".to_string())
            )
        );
        Ok(())
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

    #[test]
    fn changing_script_discards_old_schedule_but_keeps_the_last_key_and_expiry(
    ) -> Result<(), AppError> {
        let db = Database::memory()?;
        db.upsert_api_key_var(None, "DAO_T_SCRIPT_CHANGE", &cfg("/old-script", true))?;
        let state = ApiKeyRefreshState {
            expires_at: Some(9_000_000),
            refresh_after: Some(8_000_000),
            last_success_at: Some(7_000_000),
            resolved_script: Some("index.mjs".to_string()),
            ..Default::default()
        };
        db.update_api_key_var_result("DAO_T_SCRIPT_CHANGE", Some("last-good-key"), &state)?;

        let updated = db.upsert_api_key_var(
            Some("DAO_T_SCRIPT_CHANGE"),
            "DAO_T_SCRIPT_CHANGE",
            &cfg("/replacement-script", true),
        )?;

        assert_eq!(updated.value.as_deref(), Some("last-good-key"));
        assert_eq!(updated.state.expires_at, Some(9_000_000));
        assert_eq!(updated.state.last_success_at, None);
        assert_eq!(updated.state.refresh_after, None);
        assert_eq!(updated.state.resolved_script, None);
        // A replacement script must be scheduled now, even when the old key
        // had a distant expiry. `next_due_ms` treats no successful run as due.
        assert_eq!(next_due_ms(&updated.config, &updated.state), 0);
        Ok(())
    }

    #[test]
    fn renaming_without_changing_script_keeps_a_successful_schedule() -> Result<(), AppError> {
        let db = Database::memory()?;
        db.upsert_api_key_var(None, "DAO_T_RENAME_SCHEDULE", &cfg("/same-script", true))?;
        let state = ApiKeyRefreshState {
            expires_at: Some(9_000_000),
            refresh_after: Some(8_000_000),
            last_success_at: Some(7_000_000),
            resolved_script: Some("index.mjs".to_string()),
            ..Default::default()
        };
        db.update_api_key_var_result("DAO_T_RENAME_SCHEDULE", Some("last-good-key"), &state)?;

        let renamed = db.upsert_api_key_var(
            Some("DAO_T_RENAME_SCHEDULE"),
            "DAO_T_RENAME_SCHEDULED",
            &cfg("/same-script", true),
        )?;

        assert_eq!(renamed.value.as_deref(), Some("last-good-key"));
        assert_eq!(renamed.state, state);
        assert_eq!(next_due_ms(&renamed.config, &renamed.state), 8_000_000);
        Ok(())
    }

    #[test]
    fn conditional_refresh_result_rejects_edited_or_recreated_records() -> Result<(), AppError> {
        let db = Database::memory()?;
        db.upsert_api_key_var(None, "DAO_T_GENERATION", &cfg("/original", true))?;
        let original = db.get_api_key_var("DAO_T_GENERATION")?.unwrap();

        db.upsert_api_key_var(
            Some("DAO_T_GENERATION"),
            "DAO_T_GENERATION_RENAMED",
            &cfg("/original", true),
        )?;
        assert!(!db.update_api_key_var_result_if_current(
            &original,
            Some("stale-after-rename"),
            &ApiKeyRefreshState::default(),
        )?);
        db.upsert_api_key_var(
            Some("DAO_T_GENERATION_RENAMED"),
            "DAO_T_GENERATION",
            &cfg("/original", true),
        )?;

        // A configuration edit, including an ABA edit back to the old settings,
        // invalidates the script snapshot.
        db.upsert_api_key_var(
            Some("DAO_T_GENERATION"),
            "DAO_T_GENERATION",
            &cfg("/changed", true),
        )?;
        db.upsert_api_key_var(
            Some("DAO_T_GENERATION"),
            "DAO_T_GENERATION",
            &cfg("/original", true),
        )?;
        assert!(!db.update_api_key_var_result_if_current(
            &original,
            Some("stale-after-edit"),
            &ApiKeyRefreshState::default(),
        )?);
        assert_eq!(
            db.get_api_key_var("DAO_T_GENERATION")?
                .unwrap()
                .value
                .as_deref(),
            None
        );

        let before_delete = db.get_api_key_var("DAO_T_GENERATION")?.unwrap();
        assert!(db.delete_api_key_var("DAO_T_GENERATION")?);
        db.upsert_api_key_var(None, "DAO_T_GENERATION", &cfg("/replacement", true))?;
        db.update_api_key_var_result(
            "DAO_T_GENERATION",
            Some("replacement-value"),
            &ApiKeyRefreshState::default(),
        )?;
        assert!(!db.update_api_key_var_result_if_current(
            &before_delete,
            Some("stale-after-delete"),
            &ApiKeyRefreshState::default(),
        )?);
        let replacement = db.get_api_key_var("DAO_T_GENERATION")?.unwrap();
        assert_eq!(replacement.value.as_deref(), Some("replacement-value"));
        assert_eq!(
            resolve::lookup("DAO_T_GENERATION"),
            (resolve::RefStatus::Ok, Some("replacement-value".into()))
        );
        Ok(())
    }
}
