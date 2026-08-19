//! Request/response detail capture queries
//!
//! Query raw body from proxy_request_log_details table for display in detail panel.

use crate::error::AppError;
use serde::{Deserialize, Serialize};

/// Request/response detail payload
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestLogDetailPayload {
    pub request_id: String,
    pub request_headers: Option<String>,
    pub request_body: Option<String>,
    pub response_headers: Option<String>,
    pub response_body: Option<String>,
}

/// Get request/response detail payload
///
/// Tries full schema columns first (*_json / *_full), falls back to simplified 5-column schema.
#[tauri::command]
pub fn get_request_detail_payload(
    state: tauri::State<'_, crate::AppState>,
    request_id: String,
) -> Result<Option<RequestLogDetailPayload>, AppError> {
    let conn = crate::database::lock_conn!(state.db.conn);

    // 1) Full schema (my-ccs-dev / production)
    let full = conn.query_row(
        "SELECT request_id,
                request_headers_json,
                COALESCE(request_body_full, request_body_preview),
                response_headers_json,
                COALESCE(response_body_full, response_body_preview)
         FROM proxy_request_log_details
         WHERE request_id = ?",
        [&request_id],
        |row| {
            Ok(RequestLogDetailPayload {
                request_id: row.get(0)?,
                request_headers: row.get(1)?,
                request_body: row.get(2)?,
                response_headers: row.get(3)?,
                response_body: row.get(4)?,
            })
        },
    );
    match full {
        Ok(payload) => return Ok(Some(payload)),
        Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(None),
        Err(_) => {}
    }

    // 2) Simplified 5-column schema (early branch version)
    let simple = conn.query_row(
        "SELECT request_id, request_headers, request_body, response_headers, response_body
         FROM proxy_request_log_details
         WHERE request_id = ?",
        [&request_id],
        |row| {
            Ok(RequestLogDetailPayload {
                request_id: row.get(0)?,
                request_headers: row.get(1)?,
                request_body: row.get(2)?,
                response_headers: row.get(3)?,
                response_body: row.get(4)?,
            })
        },
    );
    match simple {
        Ok(payload) => Ok(Some(payload)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(AppError::Database(e.to_string())),
    }
}
