use std::sync::Arc;
use std::time::Duration;

use rusqlite::params;

use crate::cursor::runtime::CursorRuntimeService;
use crate::cursor::types::CursorUsageEvent;
use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::services::sql_helpers::{INPUT_TOKEN_SEMANTICS_FRESH, INPUT_TOKEN_SEMANTICS_TOTAL};

const CURSOR_USAGE_CURSOR_KEY: &str = "cursor_sidecar_usage_cursor";
const CURSOR_USAGE_SOURCE: &str = "cursor_sidecar";
const PAGE_SIZE: usize = 200;

pub async fn sync_usage(
    service: &CursorRuntimeService,
    db: &Arc<Database>,
) -> Result<u64, AppError> {
    let mut cursor = db
        .get_setting(CURSOR_USAGE_CURSOR_KEY)?
        .and_then(|value| value.parse::<i64>().ok())
        .unwrap_or(0);
    let mut imported = 0_u64;

    loop {
        let page = service.usage_events(cursor, PAGE_SIZE).await?;
        if page.events.is_empty() {
            break;
        }
        {
            let mut conn = lock_conn!(db.conn);
            let transaction = conn
                .transaction()
                .map_err(|error| AppError::Database(error.to_string()))?;
            for event in &page.events {
                upsert_usage_event(&transaction, event)?;
            }
            transaction
                .execute(
                    "INSERT OR REPLACE INTO settings(key, value) VALUES (?1, ?2)",
                    params![CURSOR_USAGE_CURSOR_KEY, page.next_cursor.to_string()],
                )
                .map_err(|error| AppError::Database(error.to_string()))?;
            transaction
                .commit()
                .map_err(|error| AppError::Database(error.to_string()))?;
        }
        imported += page.events.len() as u64;
        cursor = page.next_cursor;
        if !page.has_more {
            break;
        }
    }

    if imported > 0 {
        let models = {
            let conn = lock_conn!(db.conn);
            let mut statement = conn
                .prepare(
                    "SELECT DISTINCT COALESCE(NULLIF(pricing_model, ''), model)
                     FROM proxy_request_logs
                     WHERE app_type = 'cursor' AND data_source = ?1",
                )
                .map_err(|error| AppError::Database(error.to_string()))?;
            let rows = statement
                .query_map([CURSOR_USAGE_SOURCE], |row| row.get::<_, String>(0))
                .map_err(|error| AppError::Database(error.to_string()))?;
            rows.filter_map(Result::ok).collect::<Vec<_>>()
        };
        for model in models {
            if let Err(error) = db.backfill_missing_usage_costs_for_model(&model) {
                log::warn!("Cursor usage 成本回填失败 model={model}: {error}");
            }
        }
        crate::usage_events::notify_log_recorded();
    }
    Ok(imported)
}

fn upsert_usage_event(
    transaction: &rusqlite::Transaction<'_>,
    event: &CursorUsageEvent,
) -> Result<(), AppError> {
    if event.event_id.trim().is_empty()
        || event.source_provider_id.trim().is_empty()
        || event.model.trim().is_empty()
    {
        return Err(AppError::InvalidInput(
            "Cursor usage 事件缺少 eventId、Provider ID 或模型".to_string(),
        ));
    }
    let created_at = event.at.timestamp();
    let status_code = if event.status_code > 0 {
        event.status_code
    } else if event.status == "completed" {
        200
    } else {
        502
    };
    let input_semantics = if event.provider_type == "openai" {
        INPUT_TOKEN_SEMANTICS_TOTAL
    } else {
        INPUT_TOKEN_SEMANTICS_FRESH
    };
    transaction
        .execute(
            "INSERT INTO proxy_request_logs (
                request_id, provider_id, provider_name_snapshot, app_type,
                model, request_model, pricing_model,
                input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                input_token_semantics,
                input_cost_usd, output_cost_usd, cache_read_cost_usd, cache_creation_cost_usd,
                total_cost_usd, latency_ms, first_token_ms, duration_ms,
                status_code, error_message, provider_type, is_streaming,
                cost_multiplier, created_at, data_source
            ) VALUES (
                ?1, ?2, ?3, 'cursor', ?4, ?5, ?6,
                ?7, ?8, ?9, ?10, ?11,
                '0', '0', '0', '0', '0', ?12, ?13, ?14,
                ?15, ?16, ?17, ?18, '1', ?19, ?20
            )
            ON CONFLICT(request_id) DO UPDATE SET
                provider_id = excluded.provider_id,
                provider_name_snapshot = excluded.provider_name_snapshot,
                model = excluded.model,
                request_model = excluded.request_model,
                pricing_model = excluded.pricing_model,
                input_tokens = excluded.input_tokens,
                output_tokens = excluded.output_tokens,
                cache_read_tokens = excluded.cache_read_tokens,
                cache_creation_tokens = excluded.cache_creation_tokens,
                input_token_semantics = excluded.input_token_semantics,
                latency_ms = excluded.latency_ms,
                first_token_ms = excluded.first_token_ms,
                duration_ms = excluded.duration_ms,
                status_code = excluded.status_code,
                error_message = excluded.error_message,
                provider_type = excluded.provider_type,
                is_streaming = excluded.is_streaming,
                created_at = excluded.created_at,
                data_source = excluded.data_source",
            params![
                event.event_id,
                event.source_provider_id,
                event.source_provider_name,
                event.model,
                event.request_model,
                event.pricing_model,
                event.input_tokens.max(0),
                event.output_tokens.max(0),
                event.cache_read_tokens.max(0),
                event.cache_write_tokens.max(0),
                input_semantics,
                event.latency_ms.max(0),
                (event.first_token_ms > 0).then_some(event.first_token_ms),
                (event.duration_ms > 0).then_some(event.duration_ms),
                status_code,
                (!event.error.trim().is_empty()).then_some(event.error.as_str()),
                event.provider_type,
                i64::from(event.is_streaming),
                created_at,
                CURSOR_USAGE_SOURCE,
            ],
        )
        .map_err(|error| AppError::Database(format!("写入 Cursor usage 失败: {error}")))?;
    Ok(())
}

pub fn spawn_usage_sync(service: CursorRuntimeService, db: Arc<Database>) {
    if !service.try_begin_usage_sync() {
        return;
    }
    tauri::async_runtime::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(2));
        loop {
            interval.tick().await;
            if !service.is_running().await {
                service.finish_usage_sync();
                return;
            }
            if let Err(error) = sync_usage(&service, &db).await {
                log::warn!("同步 Cursor usage 失败: {error}");
            }
        }
    });
}
