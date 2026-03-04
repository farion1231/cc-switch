use crate::database::{lock_conn, Database};
use crate::error::AppError;
use rusqlite::params;
use std::time::SystemTime;

#[derive(Debug, Clone)]
pub struct SessionPromptCaptureRecord {
    pub system_prompt: String,
    pub updated_at: i64,
}

impl Database {
    pub fn upsert_session_system_prompt(
        &self,
        app_type: &str,
        session_id: &str,
        system_prompt: &str,
    ) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        conn.execute(
            "INSERT INTO session_prompt_capture (
                app_type, session_id, system_prompt, created_at, updated_at
            ) VALUES (?1, ?2, ?3, ?4, ?4)
            ON CONFLICT(app_type, session_id) DO UPDATE SET
                system_prompt = excluded.system_prompt,
                updated_at = excluded.updated_at",
            params![app_type, session_id, system_prompt, now],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        Ok(())
    }

    pub fn get_session_system_prompt(
        &self,
        app_type: &str,
        session_id: &str,
    ) -> Result<Option<SessionPromptCaptureRecord>, AppError> {
        let conn = lock_conn!(self.conn);
        let result = conn.query_row(
            "SELECT system_prompt, updated_at
             FROM session_prompt_capture
             WHERE app_type = ?1 AND session_id = ?2",
            params![app_type, session_id],
            |row| {
                Ok(SessionPromptCaptureRecord {
                    system_prompt: row.get(0)?,
                    updated_at: row.get(1)?,
                })
            },
        );

        match result {
            Ok(row) => Ok(Some(row)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(AppError::Database(e.to_string())),
        }
    }
}
