//! Per-model provider routing DAO
//!
//! Stores a mapping of `model_class` (opus/sonnet/haiku) → `provider_id` per
//! app, so the local proxy can route different models to different providers.

use crate::database::{lock_conn, Database};
use crate::error::AppError;
use std::collections::HashMap;

impl Database {
    /// Get all model-class → provider routes configured for an app.
    pub fn get_model_routes(&self, app_type: &str) -> Result<HashMap<String, String>, AppError> {
        let conn = lock_conn!(self.conn);

        let mut stmt = conn
            .prepare(
                "SELECT model_class, provider_id
                 FROM model_provider_routes
                 WHERE app_type = ?1",
            )
            .map_err(|e| AppError::Database(e.to_string()))?;

        let rows = stmt
            .query_map([app_type], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|e| AppError::Database(e.to_string()))?;

        let mut map = HashMap::new();
        for row in rows {
            let (class, provider_id) = row.map_err(|e| AppError::Database(e.to_string()))?;
            map.insert(class, provider_id);
        }

        Ok(map)
    }

    /// Get the routed provider id for a single model class, if configured.
    pub fn get_model_route(
        &self,
        app_type: &str,
        model_class: &str,
    ) -> Result<Option<String>, AppError> {
        let conn = lock_conn!(self.conn);

        conn.query_row(
            "SELECT provider_id FROM model_provider_routes
             WHERE app_type = ?1 AND model_class = ?2",
            rusqlite::params![app_type, model_class],
            |row| row.get::<_, String>(0),
        )
        .map(Some)
        .or_else(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            other => Err(AppError::Database(other.to_string())),
        })
    }

    /// Set (or clear) the route for a model class.
    ///
    /// Passing `None` or an empty `provider_id` removes the route, falling back
    /// to the app's normal current/failover provider selection.
    pub fn set_model_route(
        &self,
        app_type: &str,
        model_class: &str,
        provider_id: Option<&str>,
    ) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);

        match provider_id.filter(|id| !id.is_empty()) {
            Some(provider_id) => {
                conn.execute(
                    "INSERT INTO model_provider_routes (app_type, model_class, provider_id, updated_at)
                     VALUES (?1, ?2, ?3, datetime('now'))
                     ON CONFLICT(app_type, model_class)
                     DO UPDATE SET provider_id = excluded.provider_id, updated_at = datetime('now')",
                    rusqlite::params![app_type, model_class, provider_id],
                )
                .map_err(|e| AppError::Database(e.to_string()))?;
            }
            None => {
                conn.execute(
                    "DELETE FROM model_provider_routes WHERE app_type = ?1 AND model_class = ?2",
                    rusqlite::params![app_type, model_class],
                )
                .map_err(|e| AppError::Database(e.to_string()))?;
            }
        }

        Ok(())
    }
}
