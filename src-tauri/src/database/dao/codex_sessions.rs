use crate::database::{lock_conn, Database};
use crate::error::AppError;
use rusqlite::params;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CodexSessionProviderLink {
    pub session_id: String,
    pub source_path: String,
    pub provider_id: String,
    pub link_mode: String,
    pub created_at: i64,
    pub updated_at: i64,
}

impl Database {
    pub fn replace_codex_session_provider_links(
        &self,
        session_id: &str,
        source_path: &str,
        provider_ids: &[String],
        link_mode: &str,
    ) -> Result<Vec<CodexSessionProviderLink>, AppError> {
        let mut conn = lock_conn!(self.conn);
        let tx = conn
            .transaction()
            .map_err(|e| AppError::Database(e.to_string()))?;

        tx.execute(
            "DELETE FROM codex_session_provider_links
             WHERE session_id = ?1 AND source_path = ?2",
            params![session_id, source_path],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        let now = chrono::Utc::now().timestamp();
        let mut seen = HashSet::new();
        let mut links = Vec::with_capacity(provider_ids.len());

        for provider_id in provider_ids {
            if !seen.insert(provider_id.as_str()) {
                continue;
            }

            let link = CodexSessionProviderLink {
                session_id: session_id.to_string(),
                source_path: source_path.to_string(),
                provider_id: provider_id.clone(),
                link_mode: link_mode.to_string(),
                created_at: now,
                updated_at: now,
            };

            tx.execute(
                "INSERT INTO codex_session_provider_links (
                    session_id, source_path, provider_id, link_mode, created_at, updated_at
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    &link.session_id,
                    &link.source_path,
                    &link.provider_id,
                    &link.link_mode,
                    link.created_at,
                    link.updated_at
                ],
            )
            .map_err(|e| AppError::Database(e.to_string()))?;

            links.push(link);
        }

        tx.commit().map_err(|e| AppError::Database(e.to_string()))?;
        Ok(links)
    }

    pub fn get_codex_session_provider_links(
        &self,
        session_id: &str,
        source_path: &str,
    ) -> Result<Vec<CodexSessionProviderLink>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn
            .prepare(
                "SELECT session_id, source_path, provider_id, link_mode, created_at, updated_at
                 FROM codex_session_provider_links
                 WHERE session_id = ?1 AND source_path = ?2
                 ORDER BY provider_id ASC",
            )
            .map_err(|e| AppError::Database(e.to_string()))?;

        let rows = stmt
            .query_map(params![session_id, source_path], |row| {
                Ok(CodexSessionProviderLink {
                    session_id: row.get(0)?,
                    source_path: row.get(1)?,
                    provider_id: row.get(2)?,
                    link_mode: row.get(3)?,
                    created_at: row.get(4)?,
                    updated_at: row.get(5)?,
                })
            })
            .map_err(|e| AppError::Database(e.to_string()))?;

        let mut links = Vec::new();
        for row in rows {
            links.push(row.map_err(|e| AppError::Database(e.to_string()))?);
        }
        Ok(links)
    }

    pub fn delete_codex_session_provider_link(
        &self,
        session_id: &str,
        source_path: &str,
        provider_id: &str,
    ) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "DELETE FROM codex_session_provider_links
             WHERE session_id = ?1 AND source_path = ?2 AND provider_id = ?3",
            params![session_id, source_path, provider_id],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }
}
