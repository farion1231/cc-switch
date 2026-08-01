use std::sync::Arc;
use std::time::Duration;

use rusqlite::params;

use crate::cursor::runtime::CursorRuntimeService;
use crate::cursor::types::CursorUsageEvent;
use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::services::sql_helpers::INPUT_TOKEN_SEMANTICS_FRESH;

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
        let page_imported = {
            let mut conn = lock_conn!(db.conn);
            persist_usage_page(&mut conn, &page.events, page.next_cursor)?
        };
        imported += page_imported;
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

fn persist_usage_page(
    conn: &mut rusqlite::Connection,
    events: &[CursorUsageEvent],
    next_cursor: i64,
) -> Result<u64, AppError> {
    let mut provider_calls = Vec::with_capacity(events.len());
    for event in events {
        match event.kind.trim() {
            "provider_call" => match validate_provider_call(event) {
                Ok(()) => provider_calls.push(event),
                Err(error) => log::warn!(
                    "跳过无效 Cursor provider_call sequence={} event_id={:?}: {error}",
                    event.sequence,
                    event.event_id
                ),
            },
            "turn_finalized" => {}
            kind => log::warn!(
                "跳过未知 Cursor usage 事件 sequence={} event_id={:?} kind={kind:?}",
                event.sequence,
                event.event_id
            ),
        }
    }

    let imported = provider_calls.len() as u64;
    let transaction = conn
        .transaction()
        .map_err(|error| AppError::Database(error.to_string()))?;
    for event in provider_calls {
        upsert_usage_event(&transaction, event)?;
    }
    transaction
        .execute(
            "INSERT OR REPLACE INTO settings(key, value) VALUES (?1, ?2)",
            params![CURSOR_USAGE_CURSOR_KEY, next_cursor.to_string()],
        )
        .map_err(|error| AppError::Database(error.to_string()))?;
    transaction
        .commit()
        .map_err(|error| AppError::Database(error.to_string()))?;
    Ok(imported)
}

fn validate_provider_call(event: &CursorUsageEvent) -> Result<(), AppError> {
    if event.event_id.trim().is_empty()
        || event.source_provider_id.trim().is_empty()
        || event.model.trim().is_empty()
    {
        return Err(AppError::InvalidInput(
            "Cursor usage 事件缺少 eventId、Provider ID 或模型".to_string(),
        ));
    }
    Ok(())
}

fn upsert_usage_event(
    transaction: &rusqlite::Transaction<'_>,
    event: &CursorUsageEvent,
) -> Result<(), AppError> {
    validate_provider_call(event)?;
    let created_at = event.at.timestamp();
    let status_code = if event.status_code > 0 {
        event.status_code
    } else if event.status == "completed" {
        200
    } else {
        502
    };
    let input_semantics = INPUT_TOKEN_SEMANTICS_FRESH;
    let token_usage_status = match event.usage_status.trim() {
        "reported" | "estimated" | "missing" => event.usage_status.trim(),
        _ if event.usage_present => "reported",
        _ => "missing",
    };
    transaction
        .execute(
            "INSERT INTO proxy_request_logs (
                request_id, provider_id, provider_name_snapshot, app_type,
                model, request_model, pricing_model,
                input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                input_token_semantics, token_usage_status, cache_usage_observed,
                input_cost_usd, output_cost_usd, cache_read_cost_usd, cache_creation_cost_usd,
                total_cost_usd, latency_ms, first_token_ms, duration_ms,
                status_code, error_message, provider_type, is_streaming,
                cost_multiplier, created_at, data_source
            ) VALUES (
                ?1, ?2, ?3, 'cursor', ?4, ?5, ?6,
                ?7, ?8, ?9, ?10, ?11, ?12, ?13,
                '0', '0', '0', '0', '0', ?14, ?15, ?16,
                ?17, ?18, ?19, ?20, '1', ?21, ?22
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
                token_usage_status = excluded.token_usage_status,
                cache_usage_observed = excluded.cache_usage_observed,
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
                token_usage_status,
                i64::from(event.cache_usage_observed && token_usage_status == "reported"),
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

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use rusqlite::Connection;

    use super::{persist_usage_page, upsert_usage_event, CURSOR_USAGE_CURSOR_KEY};
    use crate::cursor::types::CursorUsageEvent;
    use crate::database::Database;
    use crate::services::sql_helpers::INPUT_TOKEN_SEMANTICS_FRESH;

    fn provider_event(sequence: i64, event_id: &str) -> CursorUsageEvent {
        CursorUsageEvent {
            sequence,
            event_id: event_id.to_string(),
            kind: "provider_call".to_string(),
            status: "completed".to_string(),
            source_provider_id: "provider".to_string(),
            source_provider_name: "Provider".to_string(),
            provider_type: "openai".to_string(),
            channel_id: "channel".to_string(),
            request_model: "alias".to_string(),
            model: "model".to_string(),
            pricing_model: "model".to_string(),
            status_code: 200,
            error: String::new(),
            latency_ms: 12,
            first_token_ms: 3,
            duration_ms: 15,
            is_streaming: true,
            at: Utc::now(),
            input_tokens: 120,
            output_tokens: 30,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            usage_present: true,
            usage_status: "reported".to_string(),
            cache_usage_observed: false,
        }
    }

    fn test_connection() -> Connection {
        let conn = Connection::open_in_memory().expect("open database");
        Database::create_tables_on_conn(&conn).expect("create schema");
        conn
    }

    fn stored_request_count(conn: &Connection) -> i64 {
        conn.query_row("SELECT COUNT(*) FROM proxy_request_logs", [], |row| {
            row.get(0)
        })
        .expect("count stored requests")
    }

    fn stored_cursor(conn: &Connection) -> String {
        conn.query_row(
            "SELECT value FROM settings WHERE key = ?1",
            [CURSOR_USAGE_CURSOR_KEY],
            |row| row.get(0),
        )
        .expect("read stored cursor")
    }

    #[test]
    fn upsert_usage_event_persists_estimated_status_and_fresh_input_semantics() {
        let mut conn = test_connection();
        let transaction = conn.transaction().expect("start transaction");
        let event = CursorUsageEvent {
            usage_present: false,
            usage_status: "estimated".to_string(),
            ..provider_event(1, "cursor-estimated")
        };

        upsert_usage_event(&transaction, &event).expect("write usage");
        transaction.commit().expect("commit usage");

        let stored: (i64, i64, String, i64, i64, Option<i64>) = conn
            .query_row(
                "SELECT input_tokens, output_tokens, token_usage_status, input_token_semantics,
                        cache_usage_observed, first_token_ms
                 FROM proxy_request_logs WHERE request_id = 'cursor-estimated'",
                [],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                    ))
                },
            )
            .expect("read usage");
        assert_eq!(
            stored,
            (
                120,
                30,
                "estimated".to_string(),
                INPUT_TOKEN_SEMANTICS_FRESH,
                0,
                Some(3),
            )
        );
    }

    #[test]
    fn mixed_pages_skip_turn_events_at_every_position() {
        for turn_index in 0..3 {
            let mut conn = test_connection();
            let events = (0..3)
                .map(|index| {
                    let mut event =
                        provider_event(index as i64 + 1, &format!("event-{turn_index}-{index}"));
                    if index == turn_index {
                        event.kind = "turn_finalized".to_string();
                        event.source_provider_id.clear();
                        event.model.clear();
                    }
                    event
                })
                .collect::<Vec<_>>();

            let imported = persist_usage_page(&mut conn, &events, 3).expect("persist mixed page");

            assert_eq!(imported, 2, "turn position {turn_index}");
            assert_eq!(stored_request_count(&conn), 2, "turn position {turn_index}");
            assert_eq!(stored_cursor(&conn), "3", "turn position {turn_index}");
        }
    }

    #[test]
    fn invalid_provider_call_does_not_poison_page() {
        let mut conn = test_connection();
        let mut invalid = provider_event(2, "invalid");
        invalid.source_provider_id.clear();
        let events = vec![
            provider_event(1, "valid-first"),
            invalid,
            provider_event(3, "valid-last"),
        ];

        let imported = persist_usage_page(&mut conn, &events, 3).expect("persist valid events");

        assert_eq!(imported, 2);
        assert_eq!(stored_request_count(&conn), 2);
        assert_eq!(stored_cursor(&conn), "3");
    }

    #[test]
    fn retrying_page_is_idempotent() {
        let mut conn = test_connection();
        let events = vec![provider_event(1, "stable-request")];

        assert_eq!(
            persist_usage_page(&mut conn, &events, 1).expect("first import"),
            1
        );
        assert_eq!(
            persist_usage_page(&mut conn, &events, 1).expect("retry import"),
            1
        );

        assert_eq!(stored_request_count(&conn), 1);
        assert_eq!(stored_cursor(&conn), "1");
    }
}
