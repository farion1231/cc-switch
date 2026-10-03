//! Optional, read-only enrichment from Codex's local telemetry. Missing or
//! ambiguous telemetry stays unknown; tool execution is never a request timer.
use rusqlite::{Connection, OpenFlags};
use std::path::Path;

#[derive(Debug, Clone)]
pub(super) struct RequestStart {
    pub timestamp_ms: i64,
    pub turn_id: String,
}

pub(super) fn load_request_starts(codex_dir: &Path, thread_id: &str) -> Vec<RequestStart> {
    let read = || -> rusqlite::Result<Vec<RequestStart>> {
        let conn = Connection::open_with_flags(
            codex_dir.join("logs_2.sqlite"),
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        conn.busy_timeout(std::time::Duration::from_millis(100))?;
        let mut stmt = conn.prepare(
            "SELECT ts * 1000 + ts_nanos / 1000000, feedback_log_body
             FROM logs WHERE thread_id = ?1 AND target = 'feedback_tags'
             AND feedback_log_body LIKE '%responses_websocket.stream_request%'
             AND feedback_log_body LIKE '%websocket.warmup=false%'
             ORDER BY ts, ts_nanos, id",
        )?;
        let rows = stmt.query_map([thread_id], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })?;
        let mut starts = Vec::new();
        for row in rows {
            let (timestamp_ms, body) = row?;
            // The turn ID is in the sampling span, not an arbitrary message.
            if let Some(rest) = body.split("try_run_sampling_request{turn_id=").nth(1) {
                if let Some(turn_id) = rest
                    .split(|c: char| c.is_whitespace() || c == '}')
                    .next()
                    .filter(|id| !id.is_empty())
                {
                    starts.push(RequestStart {
                        timestamp_ms,
                        turn_id: turn_id.to_owned(),
                    });
                }
            }
        }
        Ok(starts)
    };
    read().unwrap_or_default()
}

pub(super) fn request_timing(
    starts: &[RequestStart],
    turn_id: &str,
    previous_end_ms: Option<i64>,
    end_ms: i64,
    first_output_ms: Option<i64>,
) -> Option<(i64, Option<i64>, Option<i64>)> {
    let candidates: Vec<_> = starts
        .iter()
        .filter(|start| {
            start.turn_id == turn_id
                && start.timestamp_ms < end_ms
                && previous_end_ms.is_none_or(|previous| start.timestamp_ms > previous)
        })
        .collect();
    // Retries/multiple requests cannot be assigned safely without a response ID.
    if candidates.len() != 1 {
        return None;
    }
    let start = candidates[0].timestamp_ms;
    if first_output_ms.is_some_and(|first| first < start || first > end_ms) {
        return None;
    }
    let first = first_output_ms.filter(|first| *first >= start && *first <= end_ms);
    Some((
        end_ms - start,
        first.map(|first| first - start),
        first.map(|first| end_ms - first),
    ))
}

pub(super) fn pricing_model(model: &str, input: u32, service_tier: &str) -> String {
    let tier = match service_tier {
        "priority" | "fast" => "fast",
        "flex" => "flex",
        "default" | "" => "",
        _ => return format!("{model}-{service_tier}"),
    };
    if !matches!(model, "gpt-6.1-sol" | "gpt-6-astra" | "gpt-6-luna") {
        return if tier.is_empty() {
            model.to_owned()
        } else {
            format!("{model}-{tier}")
        };
    }
    let long = input > 272_000;
    match (tier, long) {
        ("", false) => model.to_owned(),
        ("", true) => format!("{model}-long"),
        (_, false) => format!("{model}-{tier}"),
        (_, true) => format!("{model}-{tier}-long"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn telemetry_is_optional_read_only_and_scoped_to_thread() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load_request_starts(dir.path(), "thread").is_empty());
        assert!(!dir.path().join("logs_2.sqlite").exists());
        let conn = Connection::open(dir.path().join("logs_2.sqlite")).unwrap();
        conn.execute_batch("CREATE TABLE logs(ts INTEGER, ts_nanos INTEGER, id INTEGER, thread_id TEXT, target TEXT, feedback_log_body TEXT);").unwrap();
        for (id, thread, warmup) in [
            (1, "thread", false),
            (2, "other", false),
            (3, "thread", true),
        ] {
            conn.execute("INSERT INTO logs VALUES(100,500000000,?1,?2,'feedback_tags',?3)", rusqlite::params![id,thread,
                format!("try_run_sampling_request{{turn_id=turn model=gpt-6.1-sol}}:stream_request:websocket.warmup={warmup}:responses_websocket.stream_request")]).unwrap();
        }
        let starts = load_request_starts(dir.path(), "thread");
        assert_eq!(starts.len(), 1);
        assert_eq!(starts[0].timestamp_ms, 100500);
        assert_eq!(starts[0].turn_id, "turn");
    }
    #[test]
    fn timing_excludes_tools_and_rejects_ambiguous_requests() {
        let mut starts = vec![RequestStart {
            timestamp_ms: 1200,
            turn_id: "turn".into(),
        }];
        assert_eq!(
            request_timing(&starts, "turn", Some(1000), 2500, Some(1500)),
            Some((1300, Some(300), Some(1000)))
        );
        assert_eq!(
            request_timing(&starts, "other", None, 2500, Some(1500)),
            None
        );
        assert_eq!(
            request_timing(&starts, "turn", Some(1300), 2500, Some(1500)),
            None
        );
        starts.push(RequestStart {
            timestamp_ms: 1400,
            turn_id: "turn".into(),
        });
        assert_eq!(
            request_timing(&starts, "turn", Some(1000), 2500, Some(1500)),
            None
        );
        assert_eq!(request_timing(&[], "turn", None, 2500, None), None);
    }
    #[test]
    fn pricing_preserves_model_and_context_boundary() {
        assert_eq!(
            pricing_model("gpt-6.1-sol", 272000, "default"),
            "gpt-6.1-sol"
        );
        assert_eq!(
            pricing_model("gpt-6.1-sol", 272001, "priority"),
            "gpt-6.1-sol-fast-long"
        );
        assert_eq!(
            pricing_model("gpt-6.1-sol", 200000, "flex"),
            "gpt-6.1-sol-flex"
        );
        // Auto may resolve to a project-specific tier absent from rollout metadata.
        assert_eq!(
            pricing_model("gpt-6.1-sol", 100, "auto"),
            "gpt-6.1-sol-auto"
        );
        assert_eq!(pricing_model("unknown", 100, "priority"), "unknown-fast");
    }
}
