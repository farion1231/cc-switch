use crate::database::{lock_conn, Database};
use crate::error::AppError;
use rusqlite::params;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SessionOverrideKey {
    pub provider_id: String,
    pub session_id: String,
    pub source_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionTitleOverride {
    pub key: SessionOverrideKey,
    pub custom_title: String,
}

impl Database {
    pub fn set_session_custom_title(
        &self,
        provider_id: &str,
        session_id: &str,
        source_path: &str,
        custom_title: Option<&str>,
    ) -> Result<(), AppError> {
        let provider_id = provider_id.trim();
        let session_id = session_id.trim();
        let source_path = source_path.trim();

        if provider_id.is_empty() || session_id.is_empty() || source_path.is_empty() {
            return Err(AppError::Database(
                "session override key fields cannot be empty".to_string(),
            ));
        }

        let conn = lock_conn!(self.conn);

        match custom_title.map(str::trim).filter(|value| !value.is_empty()) {
            Some(title) => {
                conn.execute(
                    "INSERT INTO session_overrides (
                        provider_id,
                        session_id,
                        source_path,
                        custom_title,
                        updated_at
                    ) VALUES (?1, ?2, ?3, ?4, ?5)
                    ON CONFLICT(provider_id, session_id, source_path)
                    DO UPDATE SET custom_title = excluded.custom_title, updated_at = excluded.updated_at",
                    params![provider_id, session_id, source_path, title, chrono::Utc::now().timestamp_millis()],
                )
                .map_err(|e| AppError::Database(e.to_string()))?;
            }
            None => {
                conn.execute(
                    "DELETE FROM session_overrides
                     WHERE provider_id = ?1 AND session_id = ?2 AND source_path = ?3",
                    params![provider_id, session_id, source_path],
                )
                .map_err(|e| AppError::Database(e.to_string()))?;
            }
        }

        Ok(())
    }

    pub fn get_session_custom_title(
        &self,
        provider_id: &str,
        session_id: &str,
        source_path: &str,
    ) -> Result<Option<String>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn
            .prepare(
                "SELECT custom_title
                 FROM session_overrides
                 WHERE provider_id = ?1 AND session_id = ?2 AND source_path = ?3",
            )
            .map_err(|e| AppError::Database(e.to_string()))?;

        let mut rows = stmt
            .query(params![provider_id, session_id, source_path])
            .map_err(|e| AppError::Database(e.to_string()))?;

        if let Some(row) = rows.next().map_err(|e| AppError::Database(e.to_string()))? {
            Ok(Some(
                row.get(0).map_err(|e| AppError::Database(e.to_string()))?,
            ))
        } else {
            Ok(None)
        }
    }

    pub fn list_session_title_overrides(&self) -> Result<Vec<SessionTitleOverride>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn
            .prepare(
                "SELECT provider_id, session_id, source_path, custom_title
                 FROM session_overrides",
            )
            .map_err(|e| AppError::Database(e.to_string()))?;

        let rows = stmt
            .query_map([], |row| {
                Ok(SessionTitleOverride {
                    key: SessionOverrideKey {
                        provider_id: row.get(0)?,
                        session_id: row.get(1)?,
                        source_path: row.get(2)?,
                    },
                    custom_title: row.get(3)?,
                })
            })
            .map_err(|e| AppError::Database(e.to_string()))?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| AppError::Database(e.to_string()))
    }
}
