use axum::{routing::post, Json, Router};
use serde::Deserialize;
use tracing::{debug, error, info, warn};

use crate::web::models::ApiResponse;

/// Hard limits to keep this public, unauthenticated endpoint from being abused
/// as a log-spam / memory-pressure vector.
const MAX_ENTRIES: usize = 100;
const MAX_FIELD_LEN: usize = 4000;

#[derive(Debug, Deserialize)]
pub struct ClientLogEntry {
    pub level: Option<String>,
    pub message: Option<String>,
    /// Where the log came from in the web UI (e.g. "console.error", "window.onerror", "web-client").
    pub source: Option<String>,
    pub stack: Option<String>,
    pub url: Option<String>,
    pub context: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
pub struct ClientLogBatch {
    pub entries: Vec<ClientLogEntry>,
}

pub fn routes() -> Router {
    Router::new().route("/", post(ingest_logs))
}

/// Truncate a string to at most `max` bytes without splitting a UTF-8 char.
fn truncate_field(mut value: String, max: usize) -> String {
    if value.len() <= max {
        return value;
    }
    let mut end = max;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    value.truncate(end);
    value.push('…');
    value
}

/// Receive a batch of browser-side log entries and re-emit them through the
/// server `tracing` pipeline under the `web_client` target so they show up in
/// the normal `RUST_LOG` output alongside backend logs.
async fn ingest_logs(Json(batch): Json<ClientLogBatch>) -> Json<ApiResponse<()>> {
    let total = batch.entries.len();
    for entry in batch.entries.into_iter().take(MAX_ENTRIES) {
        let level = entry.level.unwrap_or_else(|| "info".to_string());
        let message = truncate_field(entry.message.unwrap_or_default(), MAX_FIELD_LEN);
        let source = entry.source.unwrap_or_else(|| "web".to_string());
        let url = entry.url.unwrap_or_default();
        let stack = truncate_field(entry.stack.unwrap_or_default(), MAX_FIELD_LEN);
        let context = entry
            .context
            .map(|c| truncate_field(c.to_string(), MAX_FIELD_LEN))
            .unwrap_or_default();

        match level.as_str() {
            "error" => error!(
                target: "web_client",
                source = %source,
                url = %url,
                context = %context,
                stack = %stack,
                "{}",
                message
            ),
            "warn" => warn!(
                target: "web_client",
                source = %source,
                url = %url,
                context = %context,
                "{}",
                message
            ),
            "debug" | "trace" => debug!(
                target: "web_client",
                source = %source,
                url = %url,
                "{}",
                message
            ),
            _ => info!(
                target: "web_client",
                source = %source,
                url = %url,
                context = %context,
                "{}",
                message
            ),
        }
    }

    if total > MAX_ENTRIES {
        warn!(
            target: "web_client",
            "dropped {} client log entries over batch limit ({})",
            total - MAX_ENTRIES,
            MAX_ENTRIES
        );
    }

    Json(ApiResponse::success(()))
}
