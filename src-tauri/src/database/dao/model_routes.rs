//! Per-model provider routing DAO
//!
//! Stores a mapping of `model_class` (opus/sonnet/haiku) → provider for an app,
//! plus an optional `target_model` naming the exact model to use on that
//! provider. The local proxy uses this to send different Claude models to
//! different providers (and, optionally, to a specific upstream model).

use crate::database::{lock_conn, Database};
use crate::error::AppError;
use std::collections::HashMap;

/// A configured route for a single model class.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelRoute {
    /// Provider the model class is routed to.
    pub provider_id: String,
    /// Exact model to request on that provider. `None` means "use the
    /// provider's own model mapping / default model".
    pub target_model: Option<String>,
}

impl Database {
    /// Get all model-class → route entries configured for an app.
    pub fn get_model_routes(
        &self,
        app_type: &str,
    ) -> Result<HashMap<String, ModelRoute>, AppError> {
        let conn = lock_conn!(self.conn);

        let mut stmt = conn
            .prepare(
                "SELECT model_class, provider_id, target_model
                 FROM model_provider_routes
                 WHERE app_type = ?1",
            )
            .map_err(|e| AppError::Database(e.to_string()))?;

        let rows = stmt
            .query_map([app_type], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ))
            })
            .map_err(|e| AppError::Database(e.to_string()))?;

        let mut map = HashMap::new();
        for row in rows {
            let (class, provider_id, target_model) =
                row.map_err(|e| AppError::Database(e.to_string()))?;
            map.insert(
                class,
                ModelRoute {
                    provider_id,
                    target_model: target_model.filter(|s| !s.is_empty()),
                },
            );
        }

        Ok(map)
    }

    /// Get the route for a single model class, if configured.
    pub fn get_model_route(
        &self,
        app_type: &str,
        model_class: &str,
    ) -> Result<Option<ModelRoute>, AppError> {
        let conn = lock_conn!(self.conn);

        conn.query_row(
            "SELECT provider_id, target_model FROM model_provider_routes
             WHERE app_type = ?1 AND model_class = ?2",
            rusqlite::params![app_type, model_class],
            |row| {
                Ok(ModelRoute {
                    provider_id: row.get::<_, String>(0)?,
                    target_model: row.get::<_, Option<String>>(1)?,
                })
            },
        )
        .map(|route| {
            Some(ModelRoute {
                provider_id: route.provider_id,
                target_model: route.target_model.filter(|s| !s.is_empty()),
            })
        })
        .or_else(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            other => Err(AppError::Database(other.to_string())),
        })
    }

    /// Set (or clear) the route for a model class.
    ///
    /// - Passing `None` or an empty `provider_id` removes the route, falling
    ///   back to the app's normal current/failover provider selection.
    /// - `target_model` is the exact model to use on that provider; `None` or
    ///   an empty string stores `NULL` (use the provider's own model mapping).
    pub fn set_model_route(
        &self,
        app_type: &str,
        model_class: &str,
        provider_id: Option<&str>,
        target_model: Option<&str>,
    ) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);

        match provider_id.filter(|id| !id.is_empty()) {
            Some(provider_id) => {
                let target_model = target_model.filter(|m| !m.is_empty());
                conn.execute(
                    "INSERT INTO model_provider_routes (app_type, model_class, provider_id, target_model, updated_at)
                     VALUES (?1, ?2, ?3, ?4, datetime('now'))
                     ON CONFLICT(app_type, model_class)
                     DO UPDATE SET provider_id = excluded.provider_id, target_model = excluded.target_model, updated_at = datetime('now')",
                    rusqlite::params![app_type, model_class, provider_id, target_model],
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::Provider;
    use serde_json::json;

    fn seed_db() -> Database {
        let db = Database::memory().unwrap();
        let provider_a = Provider::with_id("a".to_string(), "A".to_string(), json!({}), None);
        let provider_b = Provider::with_id("b".to_string(), "B".to_string(), json!({}), None);
        db.save_provider("claude", &provider_a).unwrap();
        db.save_provider("claude", &provider_b).unwrap();
        db
    }

    #[test]
    fn set_and_get_route_with_target_model() {
        let db = seed_db();
        db.set_model_route("claude", "opus", Some("a"), Some("gpt-5"))
            .unwrap();

        let route = db.get_model_route("claude", "opus").unwrap();
        assert_eq!(
            route,
            Some(ModelRoute {
                provider_id: "a".to_string(),
                target_model: Some("gpt-5".to_string()),
            })
        );
    }

    #[test]
    fn empty_target_model_is_stored_as_none() {
        let db = seed_db();
        db.set_model_route("claude", "sonnet", Some("a"), Some(""))
            .unwrap();

        let route = db.get_model_route("claude", "sonnet").unwrap().unwrap();
        assert_eq!(route.provider_id, "a");
        assert_eq!(route.target_model, None);
    }

    #[test]
    fn upsert_overwrites_provider_and_target_model() {
        let db = seed_db();
        db.set_model_route("claude", "opus", Some("a"), Some("gpt-5"))
            .unwrap();
        db.set_model_route("claude", "opus", Some("b"), Some("deepseek-chat"))
            .unwrap();

        let route = db.get_model_route("claude", "opus").unwrap().unwrap();
        assert_eq!(route.provider_id, "b");
        assert_eq!(route.target_model.as_deref(), Some("deepseek-chat"));
    }

    #[test]
    fn clearing_provider_removes_route() {
        let db = seed_db();
        db.set_model_route("claude", "opus", Some("a"), Some("gpt-5"))
            .unwrap();
        db.set_model_route("claude", "opus", None, None).unwrap();

        assert_eq!(db.get_model_route("claude", "opus").unwrap(), None);
    }

    #[test]
    fn get_model_routes_returns_all_entries() {
        let db = seed_db();
        db.set_model_route("claude", "opus", Some("a"), Some("gpt-5"))
            .unwrap();
        db.set_model_route("claude", "sonnet", Some("b"), None)
            .unwrap();

        let routes = db.get_model_routes("claude").unwrap();
        assert_eq!(routes.len(), 2);
        assert_eq!(
            routes.get("opus"),
            Some(&ModelRoute {
                provider_id: "a".to_string(),
                target_model: Some("gpt-5".to_string()),
            })
        );
        assert_eq!(
            routes.get("sonnet"),
            Some(&ModelRoute {
                provider_id: "b".to_string(),
                target_model: None,
            })
        );
    }
}
