//! Provider API Keys DAO
//!
//! CRUD on the `provider_api_keys` child table introduced in v12. Each
//! provider can have N keys (drag-reorderable, per-key on/off, tags, cooldown).
//! One key per provider is flagged `is_active=1` to be the one written into
//! live settings.json when proxy is off; the rest stay in the pool for
//! rotation by the proxy's `KeyRing`.
//!
//! Runtime state (`cooldown_until`, `failure_count`, `last_used_at`,
//! `last_error`) is persisted in the same row so process restart doesn't
//! lose in-flight cooldowns. The `KeyRing` service reads/writes via
//! these methods and flushes dirty changes on a 30s tick.

use crate::database::{lock_conn, Database};
use crate::error::AppError;
use rusqlite::{params, OptionalExtension};

/// One row of `provider_api_keys`, mirrored on the JS side as `ApiKeyDto`.
///
/// `api_key` is plaintext on disk to match the rest of the app's storage
/// (Provider.settings_config.api_key). A later secret-store migration would
/// touch both columns at once.
#[derive(Debug, Clone)]
pub struct ProviderApiKey {
    pub id: String,
    pub provider_id: String,
    pub app_type: String,
    pub label: String,
    pub api_key: String,
    /// JSON array of strings. Empty Vec = no tags.
    pub tags: Vec<String>,
    pub notes: String,
    pub enabled: bool,
    pub sort_index: i64,
    pub is_active: bool,
    pub cooldown_until: i64,
    pub failure_count: i64,
    pub last_used_at: Option<i64>,
    pub last_error: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

impl Database {
    /// List all keys for one (provider_id, app_type), ordered by `sort_index ASC`.
    /// Used by the form view (`ApiKeyListSection`) and by the proxy's
    /// `KeyRing::reload` to hydrate its in-memory pool.
    pub fn list_api_keys(
        &self,
        provider_id: &str,
        app_type: &str,
    ) -> Result<Vec<ProviderApiKey>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn
            .prepare(
                "SELECT id, provider_id, app_type, label, api_key, tags, notes,
                        enabled, sort_index, is_active, cooldown_until, failure_count,
                        last_used_at, last_error, created_at, updated_at
                 FROM provider_api_keys
                 WHERE provider_id = ?1 AND app_type = ?2
                 ORDER BY sort_index ASC, created_at ASC, id ASC",
            )
            .map_err(|e| AppError::Database(e.to_string()))?;

        let rows = stmt
            .query_map(params![provider_id, app_type], row_to_api_key)
            .map_err(|e| AppError::Database(e.to_string()))?;
        let mut keys = Vec::new();
        for r in rows {
            keys.push(r.map_err(|e| AppError::Database(e.to_string()))?);
        }
        Ok(keys)
    }

    /// Look up a single key by its primary key id. Returns None if not found
    /// (e.g. concurrent delete). Used by the active-key selector and the
    /// KeyRing hot path.
    pub fn get_api_key(&self, key_id: &str) -> Result<Option<ProviderApiKey>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn
            .prepare(
                "SELECT id, provider_id, app_type, label, api_key, tags, notes,
                        enabled, sort_index, is_active, cooldown_until, failure_count,
                        last_used_at, last_error, created_at, updated_at
                 FROM provider_api_keys WHERE id = ?1",
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
        stmt.query_row(params![key_id], row_to_api_key)
            .optional()
            .map_err(|e| AppError::Database(e.to_string()))
    }

    /// Look up the currently-active key for a provider. There should be at
    /// most one row with `is_active=1` per provider; ties are broken by
    /// the lowest `sort_index`. Returns None if no key is active (e.g.
    /// the migration's `is_active=0` for a fresh pool that the user
    /// hasn't selected yet).
    pub fn get_active_api_key(
        &self,
        provider_id: &str,
        app_type: &str,
    ) -> Result<Option<ProviderApiKey>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn
            .prepare(
                "SELECT id, provider_id, app_type, label, api_key, tags, notes,
                        enabled, sort_index, is_active, cooldown_until, failure_count,
                        last_used_at, last_error, created_at, updated_at
                 FROM provider_api_keys
                 WHERE provider_id = ?1 AND app_type = ?2 AND is_active = 1
                 ORDER BY sort_index ASC LIMIT 1",
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
        stmt.query_row(params![provider_id, app_type], row_to_api_key)
            .optional()
            .map_err(|e| AppError::Database(e.to_string()))
    }

    /// Insert a new key. Returns the persisted row (with timestamps filled
    /// by the DB side or by the caller — we generate them here so callers
    /// don't need to round-trip).
    pub fn insert_api_key(&self, key: &ProviderApiKey) -> Result<ProviderApiKey, AppError> {
        let conn = lock_conn!(self.conn);
        // UNIQUE(provider_id, app_type, label) 唯一约束——前检查给出友好错误，
        // 否则 raw rusqlite constraint violation 会泄露到 UI toast。
        let existing: Option<String> = conn
            .query_row(
                "SELECT id FROM provider_api_keys
                 WHERE provider_id = ?1 AND app_type = ?2 AND label = ?3",
                params![key.provider_id, key.app_type, key.label],
                |row| row.get(0),
            )
            .ok();
        if existing.is_some() {
            return Err(AppError::Database(format!(
                "同一 provider 下已存在标签为 {:?} 的 key（label 必须在 provider 内唯一）",
                key.label
            )));
        }
        let tags_json =
            serde_json::to_string(&key.tags).map_err(|e| AppError::Database(e.to_string()))?;
        conn.execute(
            "INSERT INTO provider_api_keys (
                id, provider_id, app_type, label, api_key, tags, notes,
                enabled, sort_index, is_active, cooldown_until, failure_count,
                last_used_at, last_error, created_at, updated_at
             ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16
             )",
            params![
                key.id,
                key.provider_id,
                key.app_type,
                key.label,
                key.api_key,
                tags_json,
                key.notes,
                key.enabled as i64,
                key.sort_index,
                key.is_active as i64,
                key.cooldown_until,
                key.failure_count,
                key.last_used_at,
                key.last_error,
                key.created_at,
                key.updated_at,
            ],
        )
        .map_err(|e| AppError::Database(format!("insert_api_key 失败: {e}")))?;
        Ok(key.clone())
    }

    /// Update mutable fields on a key. Only fields callers may legally
    /// change are exposed as `UpdateApiKey` to keep the SQL tight (avoid
    /// accidentally writing `id` or `provider_id` mid-flight).
    pub fn update_api_key_fields(
        &self,
        key_id: &str,
        label: Option<&str>,
        api_key: Option<&str>,
        tags: Option<&Vec<String>>,
        notes: Option<&str>,
        enabled: Option<bool>,
        sort_index: Option<i64>,
        is_active: Option<bool>,
        updated_at: i64,
    ) -> Result<ProviderApiKey, AppError> {
        // Build dynamic SET clause — only include fields the caller actually
        // changed. This avoids clobbering runtime fields (`cooldown_until`,
        // `failure_count`, `last_used_at`, `last_error`) when the UI only
        // edited label / tags.
        let mut sets: Vec<String> = Vec::new();
        let mut bind: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        if let Some(v) = label {
            sets.push("label = ?".into());
            bind.push(Box::new(v.to_string()));
        }
        if let Some(v) = api_key {
            sets.push("api_key = ?".into());
            bind.push(Box::new(v.to_string()));
        }
        if let Some(v) = tags {
            let json = serde_json::to_string(v).map_err(|e| AppError::Database(e.to_string()))?;
            sets.push("tags = ?".into());
            bind.push(Box::new(json));
        }
        if let Some(v) = notes {
            sets.push("notes = ?".into());
            bind.push(Box::new(v.to_string()));
        }
        if let Some(v) = enabled {
            sets.push("enabled = ?".into());
            bind.push(Box::new(v as i64));
        }
        if let Some(v) = sort_index {
            sets.push("sort_index = ?".into());
            bind.push(Box::new(v));
        }
        if let Some(v) = is_active {
            sets.push("is_active = ?".into());
            bind.push(Box::new(v as i64));
        }
        if sets.is_empty() {
            // Nothing to update; return the current row so callers can chain.
            return self
                .get_api_key(key_id)?
                .ok_or_else(|| AppError::Database(format!("api_key {key_id} 不存在")));
        }
        sets.push("updated_at = ?".into());
        bind.push(Box::new(updated_at));

        let sql = format!(
            "UPDATE provider_api_keys SET {} WHERE id = ?",
            sets.join(", ")
        );
        bind.push(Box::new(key_id.to_string()));
        // 在持锁期间执行 UPDATE：避免与下面的 SELECT 在同一 Mutex 上 deadlock。
        // 关键：在锁释放之前不能让 Database 上其他方法重新获取锁。
        {
            let conn = lock_conn!(self.conn);
            let refs: Vec<&dyn rusqlite::ToSql> = bind.iter().map(|b| b.as_ref()).collect();
            let updated = conn.execute(&sql, refs.as_slice()).map_err(|e| {
                AppError::Database(format!("update_api_key_fields 失败: {e}"))
            })?;
            if updated == 0 {
                return Err(AppError::Database(format!("api_key {key_id} 不存在")));
            }
        } // 锁在这里释放
        self.get_api_key(key_id)?
            .ok_or_else(|| AppError::Database(format!("api_key {key_id} 读取失败（刚更新过）")))
    }

    /// Persist the runtime cooldown / failure stats for a single key.
    /// Called by `KeyRing::flush_dirty` every 30s.
    pub fn write_api_key_runtime(
        &self,
        key_id: &str,
        cooldown_until: i64,
        failure_count: i64,
        last_used_at: Option<i64>,
        last_error: Option<&str>,
        updated_at: i64,
    ) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        let updated = conn
            .execute(
                "UPDATE provider_api_keys
                 SET cooldown_until = ?1,
                     failure_count = ?2,
                     last_used_at = ?3,
                     last_error = ?4,
                     updated_at = ?5
                 WHERE id = ?6",
                params![
                    cooldown_until,
                    failure_count,
                    last_used_at,
                    last_error,
                    updated_at,
                    key_id,
                ],
            )
            .map_err(|e| AppError::Database(format!("write_api_key_runtime 失败: {e}")))?;
        if updated == 0 {
            return Err(AppError::Database(format!(
                "write_api_key_runtime: api_key {key_id} 不存在"
            )));
        }
        Ok(())
    }

    /// 原子性切换 active key：关闭旧 active + 打开新 active（同事务）+ 同步
    /// provider.settings_config 中指定的字段路径为新 key 的 raw value。
    ///
    /// 在同一事务里：
    /// 1. 关闭同 provider 下所有 key 的 `is_active=1`
    /// 2. 把目标 key 的 `is_active=1`
    /// 3. 把 provider.settings_config 里 `app_type` 对应字段同步到新 key 的 raw value
    ///
    /// 任一步骤失败整体回滚——避免"is_active 已切但 settings_config 还是旧 key"
    /// 的脏状态（Review finding #16 关心的半失败场景）。
    ///
    /// 字段位置由 `AppType::api_key_settings_path` 决定——与 `Provider::set_api_key`
    /// / `adapter::inject_key_into_provider` 走同一份路径表。
    pub fn set_active_api_key_with_settings(
        &self,
        provider_id: &str,
        app_type: &crate::app_config::AppType,
        key_id: &str,
        new_api_key: &str,
        updated_at: i64,
    ) -> Result<(), AppError> {
        let app_type_str = app_type.as_str();
        let mut conn = lock_conn!(self.conn);
        let tx = conn
            .transaction()
            .map_err(|e| AppError::Database(format!("开启 set_active 事务失败: {e}")))?;

        // Step 1: 关掉所有旧的 active key
        tx.execute(
            "UPDATE provider_api_keys SET is_active = 0, updated_at = ?1
             WHERE provider_id = ?2 AND app_type = ?3 AND is_active = 1",
            params![updated_at, provider_id, app_type_str],
        )
        .map_err(|e| AppError::Database(format!("清除旧 active key 失败: {e}")))?;

        // Step 2: 把目标 key 标 active
        let n = tx
            .execute(
                "UPDATE provider_api_keys SET is_active = 1, updated_at = ?1
                 WHERE id = ?2 AND provider_id = ?3 AND app_type = ?4",
                params![updated_at, key_id, provider_id, app_type_str],
            )
            .map_err(|e| AppError::Database(format!("设置新 active key 失败: {e}")))?;
        if n == 0 {
            return Err(AppError::Database(format!(
                "set_active_api_key_with_settings: 找不到 key_id={key_id} under (provider={provider_id}, app={app_type_str})"
            )));
        }

        // Step 3: 同步 provider.settings_config 到新 key 的 raw value。
        // 字段路径由 `AppType::api_key_settings_path()` 决定——与 Provider::set_api_key
        // / adapter::inject_key_into_provider 走同一份路径表。
        let mut stmt = tx.prepare(
            "SELECT settings_config FROM providers WHERE id = ?1 AND app_type = ?2",
        )?;
        let raw: String = stmt.query_row(params![provider_id, app_type_str], |row| row.get(0))?;
        drop(stmt);

        let mut parsed: serde_json::Value = serde_json::from_str(&raw)
            .map_err(|e| AppError::Database(format!("解析 settings_config 失败: {e}")))?;
        {
            let root = parsed.as_object_mut().ok_or_else(|| {
                AppError::Database("settings_config 不是 object（无法原子更新）".to_string())
            })?;
            // 优先保留用户原本的字段——同 Provider::set_api_key 的 first-existing 语义：
            // paths 里第一个已存在的字段会被更新；都不存在时创建第一个。
            // 写入与读取位置严格对称（resolve_usage_credentials 同顺序）。
            let paths = app_type.api_key_settings_path();
            let mut written = false;
            for (i, (parent, child)) in paths.iter().enumerate() {
                // 任意 path 里已存在这个 child 字段 → 命中；或这是第一个被检查的 path 且
                // 都不存在 → 创建第一个
                let is_first_path = i == 0;
                let earlier_has_field = paths[..i]
                    .iter()
                    .any(|(p, c)| root.get(*p).and_then(|o| o.get(*c)).is_some());
                let parent_obj =
                    root.entry(parent.to_string())
                        .or_insert_with(|| serde_json::Value::Object(Default::default()));
                let next = match parent_obj.as_object_mut() {
                    Some(o) => o,
                    None => {
                        *parent_obj = serde_json::Value::Object(Default::default());
                        parent_obj
                            .as_object_mut()
                            .expect("刚刚插入")
                    }
                };
                let already_present = next.contains_key(*child);
                if already_present || (is_first_path && !earlier_has_field) {
                    next.insert(
                        child.to_string(),
                        serde_json::Value::String(new_api_key.to_string()),
                    );
                    written = true;
                    break;
                }
            }
            if !written {
                // 兜底：所有 path 都不存在也未命中，强制写第一个。
                // 不会发生（is_first_path && !earlier_has_field 总会为 true），
                // 但保留以防 paths 退化为空数组。
                let (parent, child) = paths[0];
                let entry = root
                    .entry(parent.to_string())
                    .or_insert_with(|| serde_json::Value::Object(Default::default()));
                let next = entry
                    .as_object_mut()
                    .expect("parent 必须是 object");
                next.insert(
                    child.to_string(),
                    serde_json::Value::String(new_api_key.to_string()),
                );
            }
        }

        tx.execute(
            "UPDATE providers SET settings_config = ?1
             WHERE id = ?2 AND app_type = ?3",
            params![
                serde_json::to_string(&parsed)
                    .map_err(|e| AppError::Database(format!("序列化 settings_config 失败: {e}")))?,
                provider_id,
                app_type_str,
            ],
        )
        .map_err(|e| AppError::Database(format!("更新 provider.settings_config 失败: {e}")))?;

        tx.commit()
            .map_err(|e| AppError::Database(format!("提交 set_active 事务失败: {e}")))?;
        Ok(())
    }

    /// Atomically pick a new active key for a provider: clear `is_active`
    /// on all current keys for `(provider_id, app_type)`, then set it on
    /// the target `key_id`. Done in a single transaction so a crash
    /// mid-way can't leave two active keys (the existing UNIQUE invariant
    /// is per `(provider_id, app_type, label)`, not `is_active`).
    pub fn set_active_api_key(
        &self,
        provider_id: &str,
        app_type: &str,
        key_id: &str,
        updated_at: i64,
    ) -> Result<(), AppError> {
        let mut conn = lock_conn!(self.conn);
        let tx = conn
            .transaction()
            .map_err(|e| AppError::Database(format!("开启 set_active 事务失败: {e}")))?;
        tx.execute(
            "UPDATE provider_api_keys SET is_active = 0, updated_at = ?1
             WHERE provider_id = ?2 AND app_type = ?3 AND is_active = 1",
            params![updated_at, provider_id, app_type],
        )
        .map_err(|e| AppError::Database(format!("清除旧 active key 失败: {e}")))?;
        let n = tx
            .execute(
                "UPDATE provider_api_keys SET is_active = 1, updated_at = ?1
                 WHERE id = ?2 AND provider_id = ?3 AND app_type = ?4",
                params![updated_at, key_id, provider_id, app_type],
            )
            .map_err(|e| AppError::Database(format!("设置新 active key 失败: {e}")))?;
        if n == 0 {
            return Err(AppError::Database(format!(
                "set_active_api_key: 找不到 key_id={key_id} under (provider={provider_id}, app={app_type})"
            )));
        }
        tx.commit()
            .map_err(|e| AppError::Database(format!("提交 set_active 事务失败: {e}")))?;
        Ok(())
    }

    /// Remove a key. Returns the deleted row id for caller logging; returns
    /// `AppError::Database("not found")` if the key didn't exist.
    pub fn delete_api_key(&self, key_id: &str) -> Result<String, AppError> {
        let conn = lock_conn!(self.conn);
        let n = conn
            .execute("DELETE FROM provider_api_keys WHERE id = ?1", params![key_id])
            .map_err(|e| AppError::Database(format!("delete_api_key 失败: {e}")))?;
        if n == 0 {
            return Err(AppError::Database(format!(
                "delete_api_key: api_key {key_id} 不存在"
            )));
        }
        Ok(key_id.to_string())
    }

    /// Bulk update sort_index for an entire pool. Called when the user
    /// drag-reorders the key list. The caller supplies the full ordered
    /// list of key_ids; we just write `sort_index = i` for each.
    pub fn reorder_api_keys(
        &self,
        app_type: &str,
        ordered_ids: &[String],
        updated_at: i64,
    ) -> Result<(), AppError> {
        let mut conn = lock_conn!(self.conn);
        let tx = conn
            .transaction()
            .map_err(|e| AppError::Database(format!("开启 reorder 事务失败: {e}")))?;
        for (i, key_id) in ordered_ids.iter().enumerate() {
            tx.execute(
                "UPDATE provider_api_keys
                 SET sort_index = ?1, updated_at = ?2
                 WHERE id = ?3 AND app_type = ?4",
                params![i as i64, updated_at, key_id, app_type],
            )
            .map_err(|e| AppError::Database(format!("reorder 行 {i} 失败: {e}")))?;
        }
        tx.commit()
            .map_err(|e| AppError::Database(format!("提交 reorder 事务失败: {e}")))?;
        Ok(())
    }

    /// Cascade-delete all keys for a provider. Called when the provider
    /// itself is deleted (FK ON DELETE CASCADE would do this too, but
    /// explicit is easier to test).
    #[allow(dead_code)]
    pub fn delete_api_keys_for_provider(
        &self,
        provider_id: &str,
        app_type: &str,
    ) -> Result<usize, AppError> {
        let conn = lock_conn!(self.conn);
        let n = conn
            .execute(
                "DELETE FROM provider_api_keys WHERE provider_id = ?1 AND app_type = ?2",
                params![provider_id, app_type],
            )
            .map_err(|e| AppError::Database(format!("delete_api_keys_for_provider 失败: {e}")))?;
        Ok(n)
    }
}

/// Decode one row of `provider_api_keys` into the struct.
/// `tags` is a JSON string; we tolerate malformed values (empty Vec) rather
/// than surface a DB error — these come from legacy/manual writes.
fn row_to_api_key(row: &rusqlite::Row<'_>) -> rusqlite::Result<ProviderApiKey> {
    let tags_str: String = row.get(5)?;
    let tags: Vec<String> = serde_json::from_str(&tags_str).unwrap_or_default();
    Ok(ProviderApiKey {
        id: row.get(0)?,
        provider_id: row.get(1)?,
        app_type: row.get(2)?,
        label: row.get(3)?,
        api_key: row.get(4)?,
        tags,
        notes: row.get(6)?,
        enabled: row.get::<_, i64>(7)? != 0,
        sort_index: row.get(8)?,
        is_active: row.get::<_, i64>(9)? != 0,
        cooldown_until: row.get(10)?,
        failure_count: row.get(11)?,
        last_used_at: row.get(12)?,
        last_error: row.get(13)?,
        created_at: row.get(14)?,
        updated_at: row.get(15)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;

    /// 创建一个最小可用的 providers 行（FK 目标）让 api_key 行能插入。
    /// 测试不验证 providers 本身内容，只关心 FK 不报错。
    fn seed_provider(db: &Database, provider_id: &str, app_type: &str) -> Result<(), AppError> {
        let conn = lock_conn!(db.conn);
        conn.execute(
            "INSERT OR IGNORE INTO providers (id, app_type, name, settings_config)
             VALUES (?1, ?2, ?3, '{}')",
            rusqlite::params![provider_id, app_type, format!("P-{provider_id}")],
        )
        .expect("insert provider for FK");
        drop(conn);
        Ok(())
    }

    fn seed_key(id: &str, label: &str, sort_index: i64) -> ProviderApiKey {
        let now = chrono::Utc::now().timestamp();
        ProviderApiKey {
            id: id.to_string(),
            provider_id: "p1".to_string(),
            app_type: "claude".to_string(),
            label: label.to_string(),
            api_key: format!("sk-{id}"),
            tags: vec!["prod".to_string()],
            notes: String::new(),
            enabled: true,
            sort_index,
            is_active: false,
            cooldown_until: 0,
            failure_count: 0,
            last_used_at: None,
            last_error: None,
            created_at: now,
            updated_at: now,
        }
    }

    #[test]
    fn insert_and_list_round_trip() -> Result<(), AppError> {
        let db = Database::memory()?;
        seed_provider(&db, "p1", "claude")?;
        db.insert_api_key(&seed_key("k1", "Primary", 0))?;
        db.insert_api_key(&seed_key("k2", "Backup", 1))?;
        let keys = db.list_api_keys("p1", "claude")?;
        assert_eq!(keys.len(), 2);
        assert_eq!(keys[0].label, "Primary");
        assert_eq!(keys[1].label, "Backup");
        assert_eq!(keys[0].api_key, "sk-k1");
        assert_eq!(keys[0].tags, vec!["prod".to_string()]);
        Ok(())
    }

    #[test]
    fn insert_rejects_duplicate_label() -> Result<(), AppError> {
        // 同一 provider 下 label 必须唯一——UNIQUE(provider_id, app_type, label) 约束。
        // DAO 前置检查给友好错误，绕过裸 rusqlite constraint 信息。
        let db = Database::memory()?;
        seed_provider(&db, "p1", "claude")?;
        db.insert_api_key(&seed_key("k1", "Primary", 0))?;

        let dup = seed_key("k2", "Primary", 1);
        let err = db.insert_api_key(&dup).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("唯一") || msg.contains("已存在"),
            "expected unique-label error, got: {msg}");
        // 第二个 k1 确实没插进去
        assert!(db.get_api_key("k2")?.is_none());
        Ok(())
    }

    #[test]
    fn update_fields_only_touches_provided_columns() -> Result<(), AppError> {
        let db = Database::memory()?;
        seed_provider(&db, "p1", "claude")?;
        db.insert_api_key(&seed_key("k1", "Primary", 0))?;

        // 仅改 label + enabled
        db.update_api_key_fields(
            "k1",
            Some("Renamed"),
            None,
            None,
            None,
            Some(false),
            None,
            None,
            chrono::Utc::now().timestamp(),
        )?;

        let got = db.get_api_key("k1")?.expect("k1 still exists");
        assert_eq!(got.label, "Renamed");
        assert!(!got.enabled);
        // api_key 未改
        assert_eq!(got.api_key, "sk-k1");
        // cooldown_until 仍为 0
        assert_eq!(got.cooldown_until, 0);
        Ok(())
    }

    #[test]
    fn set_active_api_key_atomic_swap() -> Result<(), AppError> {
        let db = Database::memory()?;
        seed_provider(&db, "p1", "claude")?;
        db.insert_api_key(&seed_key("k1", "A", 0))?;
        db.insert_api_key(&seed_key("k2", "B", 1))?;

        // 初始两把都不是 active
        assert!(db.get_active_api_key("p1", "claude")?.is_none());

        // 把 k1 设为 active
        db.set_active_api_key("p1", "claude", "k1", chrono::Utc::now().timestamp())?;
        let active = db.get_active_api_key("p1", "claude")?.expect("active now set");
        assert_eq!(active.id, "k1");

        // 切到 k2
        db.set_active_api_key("p1", "claude", "k2", chrono::Utc::now().timestamp())?;
        let active = db.get_active_api_key("p1", "claude")?.expect("active now k2");
        assert_eq!(active.id, "k2");
        // k1 仍存在但 is_active=0
        let k1 = db.get_api_key("k1")?.expect("k1 still exists");
        assert!(!k1.is_active);
        Ok(())
    }

    #[test]
    fn set_active_with_settings_writes_both_atomically() -> Result<(), AppError> {
        // 验证 set_active_api_key_with_settings 把 active 翻转 + settings_config
        // 字段同步发生在同一事务里（任一失败整体回滚 —— Review finding #16 修复）。
        let db = Database::memory()?;
        // 用预先存在的 settings_config（env 已有 ANTHROPIC_AUTH_TOKEN=old）
        {
            let conn = lock_conn!(db.conn);
            conn.execute(
                "INSERT INTO providers (id, app_type, name, settings_config)
                 VALUES ('p1', 'claude', 'P1', '{\"env\":{\"ANTHROPIC_AUTH_TOKEN\":\"sk-old\"}}')",
                [],
            )?;
        }
        let mut k1 = seed_key("k1", "A", 0);
        k1.api_key = "sk-NEW-real-key".to_string();
        db.insert_api_key(&k1)?;

        // 调用：原子切换 active + 同步 env.ANTHROPIC_AUTH_TOKEN
        let now = chrono::Utc::now().timestamp();
        db.set_active_api_key_with_settings(
            "p1",
            &crate::app_config::AppType::Claude,
            "k1",
            "sk-NEW-real-key",
            now,
        )?;

        // is_active 已切
        let active = db.get_active_api_key("p1", "claude")?.expect("active");
        assert_eq!(active.id, "k1");
        assert!(active.is_active);

        // settings_config 已被同步覆盖
        let conn = lock_conn!(db.conn);
        let raw: String = conn.query_row(
            "SELECT settings_config FROM providers WHERE id = 'p1' AND app_type = 'claude'",
            [],
            |row| row.get(0),
        )?;
        let parsed: serde_json::Value = serde_json::from_str(&raw)
            .map_err(|e| AppError::Database(format!("parse settings_config: {e}")))?;
        assert_eq!(
            parsed["env"]["ANTHROPIC_AUTH_TOKEN"].as_str(),
            Some("sk-NEW-real-key"),
            "settings_config 必须在同事务里被同步到新 key"
        );
        Ok(())
    }

    #[test]
    fn set_active_with_settings_creates_missing_path() -> Result<(), AppError> {
        // 当 settings_config 没有 env / auth 等顶层 key 时，函数应自动创建。
        let db = Database::memory()?;
        {
            let conn = lock_conn!(db.conn);
            conn.execute(
                "INSERT INTO providers (id, app_type, name, settings_config)
                 VALUES ('p1', 'codex', 'P1', '{}')",
                [],
            )?;
        }
        let mut k1 = seed_key("k1", "A", 0);
        k1.provider_id = "p1".to_string();
        k1.app_type = "codex".to_string();
        k1.api_key = "sk-codex-key".to_string();
        db.insert_api_key(&k1)?;

        let now = chrono::Utc::now().timestamp();
        db.set_active_api_key_with_settings(
            "p1",
            &crate::app_config::AppType::Codex,
            "k1",
            "sk-codex-key",
            now,
        )?;

        let conn = lock_conn!(db.conn);
        let raw: String = conn.query_row(
            "SELECT settings_config FROM providers WHERE id = 'p1' AND app_type = 'codex'",
            [],
            |row| row.get(0),
        )?;
        let parsed: serde_json::Value = serde_json::from_str(&raw)
            .map_err(|e| AppError::Database(format!("parse settings_config: {e}")))?;
        assert_eq!(
            parsed["auth"]["OPENAI_API_KEY"].as_str(),
            Some("sk-codex-key"),
            "缺失的 auth 顶层必须被自动创建"
        );
        Ok(())
    }

    #[test]
    fn write_runtime_state_persists_across_reopen() -> Result<(), AppError> {
        let db = Database::memory()?;
        seed_provider(&db, "p1", "claude")?;
        db.insert_api_key(&seed_key("k1", "A", 0))?;

        let now = chrono::Utc::now().timestamp();
        db.write_api_key_runtime(
            "k1",
            now + 300,
            3,
            Some(now),
            Some("rate limited"),
            now,
        )?;

        let got = db.get_api_key("k1")?.expect("k1 exists");
        assert_eq!(got.cooldown_until, now + 300);
        assert_eq!(got.failure_count, 3);
        assert_eq!(got.last_used_at, Some(now));
        assert_eq!(got.last_error.as_deref(), Some("rate limited"));
        Ok(())
    }

    #[test]
    fn reorder_assigns_sequential_indices() -> Result<(), AppError> {
        let db = Database::memory()?;
        seed_provider(&db, "p1", "claude")?;
        db.insert_api_key(&seed_key("a", "A", 5))?; // 故意写乱
        db.insert_api_key(&seed_key("b", "B", 9))?;
        db.insert_api_key(&seed_key("c", "C", 1))?;

        // 重新排序为 [b, a, c]
        db.reorder_api_keys(
            "claude",
            &["b".to_string(), "a".to_string(), "c".to_string()],
            chrono::Utc::now().timestamp(),
        )?;

        let keys = db.list_api_keys("p1", "claude")?;
        let order: Vec<&str> = keys.iter().map(|k| k.id.as_str()).collect();
        assert_eq!(order, vec!["b", "a", "c"]);
        // sort_index 现在是 0, 1, 2
        assert_eq!(keys.iter().map(|k| k.sort_index).collect::<Vec<_>>(), vec![0, 1, 2]);
        Ok(())
    }

    #[test]
    fn delete_cascades() -> Result<(), AppError> {
        let db = Database::memory()?;
        seed_provider(&db, "p1", "claude")?;
        db.insert_api_key(&seed_key("k1", "A", 0))?;
        db.insert_api_key(&seed_key("k2", "B", 1))?;
        db.delete_api_key("k1")?;
        assert!(db.get_api_key("k1")?.is_none());
        // k2 仍存在
        assert!(db.get_api_key("k2")?.is_some());
        Ok(())
    }

    #[test]
    fn delete_unknown_key_errors() {
        let db = Database::memory().unwrap();
        let err = db.delete_api_key("nope").unwrap_err();
        assert!(err.to_string().contains("不存在"));
    }

    #[test]
    fn delete_api_keys_for_provider_wipes_pool() -> Result<(), AppError> {
        let db = Database::memory()?;
        seed_provider(&db, "p1", "claude")?;
        db.insert_api_key(&seed_key("k1", "A", 0))?;
        db.insert_api_key(&seed_key("k2", "B", 1))?;
        let n = db.delete_api_keys_for_provider("p1", "claude")?;
        assert_eq!(n, 2);
        assert!(db.list_api_keys("p1", "claude")?.is_empty());
        Ok(())
    }

    #[test]
    fn list_filters_by_provider_and_app() -> Result<(), AppError> {
        let db = Database::memory()?;
        seed_provider(&db, "p1", "claude")?;
        seed_provider(&db, "p1", "codex")?;
        let mut k1 = seed_key("k1", "A", 0);
        k1.provider_id = "p1".to_string();
        k1.app_type = "claude".to_string();
        let mut k2 = seed_key("k2", "B", 0);
        k2.provider_id = "p1".to_string();
        k2.app_type = "codex".to_string();
        db.insert_api_key(&k1)?;
        db.insert_api_key(&k2)?;
        assert_eq!(db.list_api_keys("p1", "claude")?.len(), 1);
        assert_eq!(db.list_api_keys("p1", "codex")?.len(), 1);
        assert_eq!(db.list_api_keys("p2", "claude")?.len(), 0);
        Ok(())
    }

    #[test]
    fn get_active_returns_lowest_sort_index_among_active() -> Result<(), AppError> {
        // 防御性测试：正常路径下 active=1 是唯一的；如果（用户手动 SQL）出现多条，
        // 取 sort_index 最低的那条作为代表
        let db = Database::memory()?;
        seed_provider(&db, "p1", "claude")?;
        let mut k1 = seed_key("k1", "B", 0);
        k1.is_active = true;
        let mut k2 = seed_key("k2", "A", 1);
        k2.is_active = true;
        db.insert_api_key(&k1)?; // B 先插入，sort_index 0，is_active 1
        db.insert_api_key(&k2)?; // A 之后，sort_index 1，is_active 1
        let active = db.get_active_api_key("p1", "claude")?.expect("some active");
        assert_eq!(active.id, "k1");
        Ok(())
    }
}
