use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::provider::Provider;
use rusqlite::{params, Connection};

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
