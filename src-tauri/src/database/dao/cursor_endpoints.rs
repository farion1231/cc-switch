use crate::cursor::types::CursorEndpoint;
use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::provider::Provider;
use rusqlite::params;

impl Database {
    pub fn get_cursor_endpoints(&self) -> Result<Vec<CursorEndpoint>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn
            .prepare(
                "SELECT id, name, provider_type, base_url, api_key, created_at
                 FROM cursor_endpoints ORDER BY created_at, id",
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
        let rows = stmt
            .query_map([], |row| {
                Ok(CursorEndpoint {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    provider_type: row.get(2)?,
                    base_url: row.get(3)?,
                    api_key: row.get(4)?,
                    created_at: row.get(5)?,
                })
            })
            .map_err(|e| AppError::Database(e.to_string()))?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| AppError::Database(e.to_string()))
    }

    pub fn get_cursor_endpoint(&self, id: &str) -> Result<Option<CursorEndpoint>, AppError> {
        let conn = lock_conn!(self.conn);
        let result = conn.query_row(
            "SELECT id, name, provider_type, base_url, api_key, created_at
             FROM cursor_endpoints WHERE id = ?1",
            [id],
            |row| {
                Ok(CursorEndpoint {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    provider_type: row.get(2)?,
                    base_url: row.get(3)?,
                    api_key: row.get(4)?,
                    created_at: row.get(5)?,
                })
            },
        );
        match result {
            Ok(endpoint) => Ok(Some(endpoint)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(error) => Err(AppError::Database(error.to_string())),
        }
    }

    pub fn save_cursor_endpoint_with_provider_changes(
        &self,
        endpoint: &CursorEndpoint,
        upserts: &[Provider],
        deleted_provider_ids: &[String],
    ) -> Result<(), AppError> {
        let mut conn = lock_conn!(self.conn);
        let tx = conn
            .transaction()
            .map_err(|e| AppError::Database(e.to_string()))?;

        tx.execute(
            "INSERT INTO cursor_endpoints
             (id, name, provider_type, base_url, api_key, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(id) DO UPDATE SET
               name = excluded.name,
               provider_type = excluded.provider_type,
               base_url = excluded.base_url,
               api_key = excluded.api_key",
            params![
                endpoint.id,
                endpoint.name,
                endpoint.provider_type,
                endpoint.base_url,
                endpoint.api_key,
                endpoint.created_at,
            ],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        for id in deleted_provider_ids {
            let raw_config: String = tx
                .query_row(
                    "SELECT settings_config FROM providers
                     WHERE id = ?1 AND app_type = 'cursor'",
                    params![id],
                    |row| row.get(0),
                )
                .map_err(|e| AppError::Database(e.to_string()))?;
            let config: serde_json::Value =
                serde_json::from_str(&raw_config).map_err(|e| AppError::Database(e.to_string()))?;
            if config["endpointId"].as_str() != Some(endpoint.id.as_str()) {
                return Err(AppError::Database(format!(
                    "Cursor Provider '{id}' 不属于 Endpoint '{}'",
                    endpoint.id
                )));
            }
            tx.execute(
                "DELETE FROM providers WHERE id = ?1 AND app_type = 'cursor'",
                params![id],
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
        }
        for provider in upserts {
            Database::save_provider_in_transaction(&tx, "cursor", provider)?;
        }

        tx.commit().map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::cursor::types::CursorEndpoint;
    use crate::database::Database;
    use crate::provider::Provider;
    use serde_json::json;

    fn endpoint() -> CursorEndpoint {
        CursorEndpoint {
            id: "endpoint-1".to_string(),
            name: "Endpoint".to_string(),
            provider_type: "openai".to_string(),
            base_url: "https://api.example.com".to_string(),
            api_key: "secret".to_string(),
            created_at: 1,
        }
    }

    fn provider(id: &str) -> Provider {
        Provider::with_id(
            id.to_string(),
            id.to_string(),
            json!({ "endpointId": "endpoint-1", "modelID": id }),
            None,
        )
    }

    #[test]
    fn keeps_endpoint_after_deleting_its_last_provider() {
        let db = Database::memory().expect("memory db");
        db.save_cursor_endpoint_with_provider_changes(&endpoint(), &[provider("model-1")], &[])
            .expect("save endpoint and model");

        db.save_cursor_endpoint_with_provider_changes(&endpoint(), &[], &["model-1".to_string()])
            .expect("delete last model");

        assert!(db
            .get_all_providers("cursor")
            .expect("providers")
            .is_empty());
        let endpoints = db.get_cursor_endpoints().expect("endpoints");
        assert_eq!(endpoints.len(), 1);
        assert_eq!(endpoints[0].id, "endpoint-1");
    }
}
