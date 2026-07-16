use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::provider::Provider;
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};

impl Database {
    pub fn get_provider_revision(
        &self,
        app_type: &str,
        provider_id: &str,
    ) -> Result<Option<i64>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn
            .prepare("SELECT revision FROM providers WHERE id = ?1 AND app_type = ?2")
            .map_err(|e| AppError::Database(format!("prepare provider revision query: {e}")))?;
        let mut rows = stmt
            .query(params![provider_id, app_type])
            .map_err(|e| AppError::Database(format!("query provider revision: {e}")))?;
        rows.next()
            .map_err(|e| AppError::Database(format!("read provider revision: {e}")))?
            .map(|row| {
                row.get(0)
                    .map_err(|e| AppError::Database(format!("decode provider revision: {e}")))
            })
            .transpose()
    }

    pub fn update_provider_cas(
        &self,
        app_type: &str,
        provider: &Provider,
        expected_revision: i64,
    ) -> Result<Option<i64>, AppError> {
        let mut conn = lock_conn!(self.conn);
        let tx = conn
            .transaction()
            .map_err(|e| AppError::Database(format!("begin provider CAS: {e}")))?;
        let revision =
            Self::update_provider_cas_on_conn(&tx, app_type, provider, expected_revision)?;
        tx.commit()
            .map_err(|e| AppError::Database(format!("commit provider CAS: {e}")))?;
        Ok(revision)
    }

    pub(crate) fn update_provider_cas_on_conn(
        conn: &Connection,
        app_type: &str,
        provider: &Provider,
        expected_revision: i64,
    ) -> Result<Option<i64>, AppError> {
        let mut meta = provider.meta.clone();
        let endpoints = meta
            .as_mut()
            .map(|value| std::mem::take(&mut value.custom_endpoints))
            .unwrap_or_default();
        let settings = serde_json::to_string(&provider.settings_config)
            .map_err(|e| AppError::Database(format!("serialize provider settings: {e}")))?;
        let meta = serde_json::to_string(&meta)
            .map_err(|e| AppError::Database(format!("serialize provider metadata: {e}")))?;
        let sort_index = provider.sort_index.map(|value| value as i64);

        let changed = conn
            .execute(
                "UPDATE providers SET
                    name = ?1, settings_config = ?2, website_url = ?3, category = ?4,
                    created_at = ?5, sort_index = ?6, notes = ?7, icon = ?8,
                    icon_color = ?9, meta = ?10, in_failover_queue = ?11,
                    revision = revision + 1
                 WHERE id = ?12 AND app_type = ?13 AND revision = ?14",
                params![
                    provider.name,
                    settings,
                    provider.website_url,
                    provider.category,
                    provider.created_at,
                    sort_index,
                    provider.notes,
                    provider.icon,
                    provider.icon_color,
                    meta,
                    provider.in_failover_queue,
                    provider.id,
                    app_type,
                    expected_revision,
                ],
            )
            .map_err(|e| AppError::Database(format!("update provider with CAS: {e}")))?;
        if changed == 0 {
            return Ok(None);
        }

        conn.execute(
            "DELETE FROM provider_endpoints WHERE provider_id = ?1 AND app_type = ?2",
            params![provider.id, app_type],
        )
        .map_err(|e| AppError::Database(format!("replace provider endpoints: {e}")))?;
        for (url, endpoint) in endpoints {
            conn.execute(
                "INSERT INTO provider_endpoints (provider_id, app_type, url, added_at)
                 VALUES (?1, ?2, ?3, ?4)",
                params![provider.id, app_type, url, endpoint.added_at],
            )
            .map_err(|e| AppError::Database(format!("insert provider endpoint: {e}")))?;
        }

        Ok(Some(expected_revision + 1))
    }

    pub fn rename_provider_cas(
        &self,
        app_type: &str,
        original_id: &str,
        provider: &Provider,
        expected_revision: i64,
    ) -> Result<Option<i64>, AppError> {
        let mut conn = lock_conn!(self.conn);
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|e| AppError::Database(format!("begin provider rename CAS: {e}")))?;

        let original_state = tx
            .query_row(
                "SELECT revision, is_current, in_failover_queue
                 FROM providers WHERE id = ?1 AND app_type = ?2",
                params![original_id, app_type],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, bool>(1)?,
                        row.get::<_, bool>(2)?,
                    ))
                },
            )
            .optional()
            .map_err(|e| AppError::Database(format!("read provider rename state: {e}")))?;
        let Some((current_revision, is_current, in_failover_queue)) = original_state else {
            return Ok(None);
        };
        if current_revision != expected_revision {
            return Ok(None);
        }

        let target_exists = tx
            .query_row(
                "SELECT 1 FROM providers WHERE id = ?1 AND app_type = ?2",
                params![provider.id, app_type],
                |_| Ok(()),
            )
            .optional()
            .map_err(|e| AppError::Database(format!("check provider rename target: {e}")))?
            .is_some();
        if target_exists {
            return Err(AppError::InvalidInput(format!(
                "provider '{}' already exists",
                provider.id
            )));
        }

        let mut meta = provider.meta.clone().unwrap_or_default();
        let endpoints = std::mem::take(&mut meta.custom_endpoints);
        let settings = serde_json::to_string(&provider.settings_config)
            .map_err(|e| AppError::Database(format!("serialize provider settings: {e}")))?;
        let meta = serde_json::to_string(&meta)
            .map_err(|e| AppError::Database(format!("serialize provider metadata: {e}")))?;
        let next_revision = expected_revision + 1;

        tx.execute(
            "INSERT INTO providers (
                id, app_type, name, settings_config, website_url, category,
                created_at, sort_index, notes, icon, icon_color, meta,
                is_current, in_failover_queue, revision
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
            params![
                provider.id,
                app_type,
                provider.name,
                settings,
                provider.website_url,
                provider.category,
                provider.created_at,
                provider.sort_index.map(|value| value as i64),
                provider.notes,
                provider.icon,
                provider.icon_color,
                meta,
                is_current,
                in_failover_queue,
                next_revision,
            ],
        )
        .map_err(|e| AppError::Database(format!("insert renamed provider: {e}")))?;

        for (url, endpoint) in endpoints {
            tx.execute(
                "INSERT INTO provider_endpoints (provider_id, app_type, url, added_at)
                 VALUES (?1, ?2, ?3, ?4)",
                params![provider.id, app_type, url, endpoint.added_at],
            )
            .map_err(|e| AppError::Database(format!("insert renamed provider endpoint: {e}")))?;
        }
        tx.execute(
            "DELETE FROM providers
             WHERE id = ?1 AND app_type = ?2 AND revision = ?3",
            params![original_id, app_type, expected_revision],
        )
        .map_err(|e| AppError::Database(format!("delete original provider after rename: {e}")))?;

        tx.commit()
            .map_err(|e| AppError::Database(format!("commit provider rename CAS: {e}")))?;
        Ok(Some(next_revision))
    }

    pub fn update_provider_sort_index(
        &self,
        app_type: &str,
        provider_id: &str,
        sort_index: usize,
    ) -> Result<bool, AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "UPDATE providers SET sort_index = ?1 WHERE id = ?2 AND app_type = ?3",
            params![sort_index as i64, provider_id, app_type],
        )
        .map(|changed| changed == 1)
        .map_err(|e| AppError::Database(format!("update provider sort index: {e}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::CustomEndpoint;
    use serde_json::json;
    use std::collections::HashMap;

    fn provider(id: &str, name: &str) -> Provider {
        let mut provider = Provider::with_id(
            id.to_string(),
            name.to_string(),
            json!({"apiKey": "sk-test"}),
            None,
        );
        let mut endpoints = HashMap::new();
        endpoints.insert(
            "https://api.example/v1".to_string(),
            CustomEndpoint {
                url: "https://api.example/v1".to_string(),
                added_at: 123,
                last_used: None,
            },
        );
        provider.meta.get_or_insert_default().custom_endpoints = endpoints;
        provider
    }

    #[test]
    fn rename_provider_cas_is_atomic_and_preserves_endpoints() -> Result<(), AppError> {
        let db = Database::memory()?;
        db.save_provider("openclaw", &provider("old", "Old"))?;

        let renamed = provider("new", "New");
        assert_eq!(
            db.rename_provider_cas("openclaw", "old", &renamed, 1)?,
            Some(2)
        );
        assert!(db.get_provider_by_id("old", "openclaw")?.is_none());
        assert_eq!(
            db.get_all_providers("openclaw")?
                .get("new")
                .expect("renamed provider")
                .meta
                .as_ref()
                .expect("metadata")
                .custom_endpoints
                .len(),
            1
        );
        assert_eq!(db.get_provider_revision("openclaw", "new")?, Some(2));
        Ok(())
    }

    #[test]
    fn rename_provider_cas_rejects_stale_revision_without_changes() -> Result<(), AppError> {
        let db = Database::memory()?;
        db.save_provider("openclaw", &provider("old", "Old"))?;

        assert_eq!(
            db.rename_provider_cas("openclaw", "old", &provider("new", "New"), 0)?,
            None
        );
        assert!(db.get_provider_by_id("old", "openclaw")?.is_some());
        assert!(db.get_provider_by_id("new", "openclaw")?.is_none());
        Ok(())
    }
}
