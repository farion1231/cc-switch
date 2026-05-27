//! SQL fragment helpers shared across usage aggregation queries.
//!
//! Anthropic reports `input_tokens` as fresh (cache reads counted
//! separately); OpenAI Responses API and Google Gemini's
//! `promptTokenCount` both include the cached portion. Any aggregation
//! summing `input_tokens` across providers must route through
//! [`fresh_input_sql`] to recover a consistent semantics.

/// Set of `app_type` values whose stored `input_tokens` already includes
/// `cache_read_tokens`. Aggregations subtract cache reads from these rows
/// to recover the fresh-input semantics used by Claude.
///
/// Why list providers explicitly: new providers default to the
/// Claude-style "input excludes cache" semantics, which is safer if the
/// caller forgets to update this list. The wrong direction (a new OpenAI-
/// style provider not added here) shows up loudly as a too-low cache hit
/// rate, which is easier to catch than the silent over-deduction that
/// would happen with the opposite default.
const CACHE_INCLUSIVE_APP_TYPES: &[&str] = &["codex", "gemini"];

/// Build an SQL expression that returns the cache-normalized `input_tokens`
/// for a single row in `proxy_request_logs` or `usage_daily_rollups`.
///
/// For rows whose `app_type` is in [`CACHE_INCLUSIVE_APP_TYPES`] and
/// `input_tokens >= cache_read_tokens`, returns
/// `input_tokens - cache_read_tokens`. For all other rows the original
/// `input_tokens` is returned unchanged.
///
/// Pass an empty string to reference the columns directly (no alias),
/// or a table alias such as `"l"` to emit `l.input_tokens` style references.
pub fn fresh_input_sql(alias: &str) -> String {
    let prefix = if alias.is_empty() {
        String::new()
    } else {
        format!("{alias}.")
    };
    let app_type_list = CACHE_INCLUSIVE_APP_TYPES
        .iter()
        .map(|t| format!("'{t}'"))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "CASE WHEN {prefix}app_type IN ({app_type_list}) AND {prefix}input_tokens >= {prefix}cache_read_tokens \
              THEN ({prefix}input_tokens - {prefix}cache_read_tokens) \
              ELSE {prefix}input_tokens END"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn setup_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE proxy_request_logs (
                request_id TEXT PRIMARY KEY,
                app_type TEXT NOT NULL,
                input_tokens INTEGER NOT NULL DEFAULT 0,
                output_tokens INTEGER NOT NULL DEFAULT 0,
                cache_read_tokens INTEGER NOT NULL DEFAULT 0,
                cache_creation_tokens INTEGER NOT NULL DEFAULT 0
            );",
        )
        .unwrap();
        conn
    }

    #[test]
    fn fresh_input_with_alias_emits_prefixed_columns() {
        let sql = fresh_input_sql("l");
        assert!(sql.contains("l.app_type"));
        assert!(sql.contains("l.input_tokens"));
        assert!(sql.contains("l.cache_read_tokens"));
    }

    #[test]
    fn fresh_input_without_alias_uses_bare_columns() {
        let sql = fresh_input_sql("");
        assert!(!sql.contains("."));
        assert!(sql.contains("'codex'"));
        assert!(sql.contains("'gemini'"));
    }

    #[test]
    fn fresh_input_subtracts_cache_for_cache_inclusive_providers() {
        let conn = setup_conn();
        // Codex row: OpenAI semantics — input_tokens includes the 600 cached.
        conn.execute(
            "INSERT INTO proxy_request_logs (request_id, app_type, input_tokens, cache_read_tokens)
             VALUES ('codex-1', 'codex', 1000, 600)",
            [],
        )
        .unwrap();
        // Gemini row: Google semantics — promptTokenCount includes cachedContentTokenCount.
        conn.execute(
            "INSERT INTO proxy_request_logs (request_id, app_type, input_tokens, cache_read_tokens)
             VALUES ('gemini-1', 'gemini', 800, 300)",
            [],
        )
        .unwrap();
        // Claude row: Anthropic semantics — input_tokens already excludes cache.
        conn.execute(
            "INSERT INTO proxy_request_logs (request_id, app_type, input_tokens, cache_read_tokens)
             VALUES ('claude-1', 'claude', 200, 5000)",
            [],
        )
        .unwrap();

        let expr = fresh_input_sql("l");
        let sql = format!("SELECT COALESCE(SUM({expr}), 0) FROM proxy_request_logs l");
        let total: i64 = conn.query_row(&sql, [], |r| r.get(0)).unwrap();
        // Codex: 1000-600=400; Gemini: 800-300=500; Claude: 200 unchanged.
        assert_eq!(total, 400 + 500 + 200);
    }

    #[test]
    fn fresh_input_handles_codex_with_cache_exceeding_input() {
        // Defensive: if a malformed Codex row somehow has cache > input,
        // we keep the original value rather than producing a negative number.
        let conn = setup_conn();
        conn.execute(
            "INSERT INTO proxy_request_logs (request_id, app_type, input_tokens, cache_read_tokens)
             VALUES ('codex-broken', 'codex', 100, 999)",
            [],
        )
        .unwrap();
        let expr = fresh_input_sql("l");
        let sql = format!("SELECT {expr} FROM proxy_request_logs l");
        let value: i64 = conn.query_row(&sql, [], |r| r.get(0)).unwrap();
        assert_eq!(value, 100);
    }
}
