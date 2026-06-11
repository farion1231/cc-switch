use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::provider::{ModelRoute, ModelRouteInput};
use chrono::Utc;
use rusqlite::{params, OptionalExtension};
use uuid::Uuid;

impl Database {
    pub fn get_model_routes(&self, app_type: &str) -> Result<Vec<ModelRoute>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn
            .prepare(
                "SELECT id, app_type, pattern, provider_id, priority, enabled, created_at, updated_at
                 FROM model_routes
                 WHERE app_type = ?1
                 ORDER BY enabled DESC, priority DESC, created_at ASC, id ASC",
            )
            .map_err(|e| AppError::Database(e.to_string()))?;

        let routes = stmt
            .query_map(params![app_type], model_route_from_row)
            .map_err(|e| AppError::Database(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| AppError::Database(e.to_string()))?;

        Ok(routes)
    }

    pub fn create_model_route(&self, input: ModelRouteInput) -> Result<ModelRoute, AppError> {
        validate_model_route_input(&input)?;
        self.ensure_route_provider_exists(&input.app_type, &input.provider_id)?;

        let now = Utc::now().to_rfc3339();
        let id = Uuid::new_v4().to_string();

        {
            let conn = lock_conn!(self.conn);
            conn.execute(
                "INSERT INTO model_routes
                 (id, app_type, pattern, provider_id, priority, enabled, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    id,
                    input.app_type,
                    input.pattern,
                    input.provider_id,
                    input.priority,
                    input.enabled,
                    now,
                    now
                ],
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
        }

        self.get_model_route(&id)?.ok_or_else(|| {
            AppError::Database(format!("Model route '{id}' was not found after insert"))
        })
    }

    pub fn update_model_route(
        &self,
        route_id: &str,
        input: ModelRouteInput,
    ) -> Result<ModelRoute, AppError> {
        validate_model_route_input(&input)?;
        self.ensure_route_provider_exists(&input.app_type, &input.provider_id)?;

        let now = Utc::now().to_rfc3339();
        let changed = {
            let conn = lock_conn!(self.conn);
            conn.execute(
                "UPDATE model_routes
                     SET app_type = ?1,
                         pattern = ?2,
                         provider_id = ?3,
                         priority = ?4,
                         enabled = ?5,
                         updated_at = ?6
                     WHERE id = ?7",
                params![
                    input.app_type,
                    input.pattern,
                    input.provider_id,
                    input.priority,
                    input.enabled,
                    now,
                    route_id
                ],
            )
            .map_err(|e| AppError::Database(e.to_string()))?
        };

        if changed == 0 {
            return Err(AppError::InvalidInput(format!(
                "Model route '{route_id}' does not exist"
            )));
        }

        self.get_model_route(route_id)?.ok_or_else(|| {
            AppError::Database(format!(
                "Model route '{route_id}' was not found after update"
            ))
        })
    }

    pub fn delete_model_route(&self, route_id: &str) -> Result<bool, AppError> {
        let conn = lock_conn!(self.conn);
        let changed = conn
            .execute("DELETE FROM model_routes WHERE id = ?1", params![route_id])
            .map_err(|e| AppError::Database(e.to_string()))?;

        Ok(changed > 0)
    }

    pub fn get_model_route(&self, route_id: &str) -> Result<Option<ModelRoute>, AppError> {
        let conn = lock_conn!(self.conn);
        conn.query_row(
            "SELECT id, app_type, pattern, provider_id, priority, enabled, created_at, updated_at
             FROM model_routes
             WHERE id = ?1",
            params![route_id],
            model_route_from_row,
        )
        .optional()
        .map_err(|e| AppError::Database(e.to_string()))
    }

    fn ensure_route_provider_exists(
        &self,
        app_type: &str,
        provider_id: &str,
    ) -> Result<(), AppError> {
        if self.get_provider_by_id(provider_id, app_type)?.is_none() {
            return Err(AppError::InvalidInput(format!(
                "Provider '{provider_id}' does not exist for app '{app_type}'"
            )));
        }
        Ok(())
    }
}

fn validate_model_route_input(input: &ModelRouteInput) -> Result<(), AppError> {
    if input.app_type.trim().is_empty() {
        return Err(AppError::InvalidInput(
            "Model route app_type cannot be empty".to_string(),
        ));
    }
    if input.pattern.trim().is_empty() {
        return Err(AppError::InvalidInput(
            "Model route pattern cannot be empty".to_string(),
        ));
    }
    if input.provider_id.trim().is_empty() {
        return Err(AppError::InvalidInput(
            "Model route provider_id cannot be empty".to_string(),
        ));
    }
    Ok(())
}

fn model_route_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ModelRoute> {
    Ok(ModelRoute {
        id: row.get(0)?,
        app_type: row.get(1)?,
        pattern: row.get(2)?,
        provider_id: row.get(3)?,
        priority: row.get(4)?,
        enabled: row.get(5)?,
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
    })
}
