use super::*;

fn local_ts(year: i32, month: u32, day: u32, hour: u32, minute: u32, second: u32) -> i64 {
    match Local.with_ymd_and_hms(year, month, day, hour, minute, second) {
        chrono::LocalResult::Single(dt) => dt.timestamp(),
        chrono::LocalResult::Ambiguous(earliest, _) => earliest.timestamp(),
        chrono::LocalResult::None => panic!("valid local datetime"),
    }
}

#[allow(clippy::too_many_arguments)]
fn insert_usage_log(
    conn: &Connection,
    request_id: &str,
    app_type: &str,
    provider_id: &str,
    model: &str,
    data_source: &str,
    created_at: i64,
    input_tokens: i64,
    output_tokens: i64,
    cache_read_tokens: i64,
    cache_creation_tokens: i64,
    status_code: i64,
    total_cost_usd: &str,
) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO proxy_request_logs (
                request_id, provider_id, app_type, model, request_model,
                input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                input_cost_usd, output_cost_usd, cache_read_cost_usd, cache_creation_cost_usd,
                total_cost_usd, latency_ms, status_code, created_at, data_source
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, '0', '0', '0', '0', ?, 100, ?, ?, ?)",
        params![
            request_id,
            provider_id,
            app_type,
            model,
            model,
            input_tokens,
            output_tokens,
            cache_read_tokens,
            cache_creation_tokens,
            total_cost_usd,
            status_code,
            created_at,
            data_source
        ],
    )?;
    Ok(())
}

fn create_legacy_nullable_logs_table(conn: &Connection) -> Result<(), AppError> {
    conn.execute(
        "CREATE TABLE proxy_request_logs (
                request_id TEXT PRIMARY KEY,
                app_type TEXT NOT NULL,
                model TEXT NOT NULL,
                request_model TEXT,
                input_tokens INTEGER NOT NULL,
                output_tokens INTEGER NOT NULL,
                cache_read_tokens INTEGER NOT NULL,
                cache_creation_tokens INTEGER NOT NULL,
                status_code INTEGER NOT NULL,
                created_at INTEGER NOT NULL,
                data_source TEXT,
                input_token_semantics INTEGER NOT NULL DEFAULT 0
            )",
        [],
    )?;
    Ok(())
}

#[test]
fn test_effective_filter_keeps_legacy_null_data_source_proxy_rows() -> Result<(), AppError> {
    let conn = Connection::open_in_memory()?;
    create_legacy_nullable_logs_table(&conn)?;
    conn.execute(
        "INSERT INTO proxy_request_logs (
                request_id, app_type, model, input_tokens, output_tokens,
                cache_read_tokens, cache_creation_tokens, status_code, created_at, data_source
            ) VALUES ('legacy-proxy', 'codex', 'gpt-5.5', 10, 2, 1, 0, 200, 1000, NULL)",
        [],
    )?;

    let filter = effective_usage_log_filter("l");
    let sql = format!("SELECT COUNT(*) FROM proxy_request_logs l WHERE {filter}");
    let count: i64 = conn.query_row(&sql, [], |row| row.get(0))?;
    assert_eq!(count, 1);

    Ok(())
}

/// 日志总数缓存：没有写入时复用；插入、删除或结束时间推后有新行时都要重数。
#[test]
fn test_request_log_total_cache_invalidates_on_writes() -> Result<(), AppError> {
    let db = Database::memory()?;
    let insert = |id: &str, created_at: i64| -> Result<(), AppError> {
        let conn = lock_conn!(db.conn);
        insert_usage_log(
            &conn,
            id,
            "claude",
            "anthropic",
            "claude-sonnet-4-5",
            "proxy",
            created_at,
            10,
            2,
            0,
            0,
            200,
            "0.01",
        )
    };
    insert("a", 1_000)?;
    insert("b", 2_000)?;

    let mut filters = LogFilters {
        start_date: Some(0),
        end_date: Some(5_000),
        ..LogFilters::default()
    };
    assert_eq!(db.get_request_logs(&filters, 0, 10)?.total, 2);
    // 没有写入：复用缓存，结果不变
    assert_eq!(db.get_request_logs(&filters, 0, 10)?.total, 2);

    // 插入后必须重数
    insert("c", 3_000)?;
    assert_eq!(db.get_request_logs(&filters, 0, 10)?.total, 3);

    // 删除后也要重数
    {
        let conn = lock_conn!(db.conn);
        conn.execute("DELETE FROM proxy_request_logs WHERE request_id = 'a'", [])?;
    }
    assert_eq!(db.get_request_logs(&filters, 0, 10)?.total, 2);

    // 结束时间往后推：新增区间里没有行就复用，有行（之前就存在的未来时间行）就重数
    filters.end_date = Some(6_000);
    assert_eq!(db.get_request_logs(&filters, 0, 10)?.total, 2);
    insert("d", 9_000)?;
    filters.end_date = Some(5_000);
    assert_eq!(db.get_request_logs(&filters, 0, 10)?.total, 2);
    filters.end_date = Some(10_000);
    assert_eq!(db.get_request_logs(&filters, 0, 10)?.total, 3);

    Ok(())
}

/// 范围版去重条件不管选哪种写法，结果都必须和逐行 EXISTS 的原写法一致。
#[test]
fn test_range_filter_matches_original_dedup() -> Result<(), AppError> {
    let conn = Connection::open_in_memory()?;
    create_legacy_nullable_logs_table(&conn)?;
    // request_id, app_type, model, input, output, cache_read, cache_creation,
    // status_code, created_at, data_source
    type LogRow = (
        &'static str,
        &'static str,
        &'static str,
        i64,
        i64,
        i64,
        i64,
        i64,
        i64,
        Option<&'static str>,
    );
    let rows: &[LogRow] = &[
        // 和 proxy-1 重复（应被去掉）
        (
            "p1",
            "claude",
            "opus",
            10,
            2,
            1,
            5,
            200,
            1000,
            Some("proxy"),
        ),
        (
            "s1",
            "claude",
            "opus",
            10,
            2,
            1,
            5,
            200,
            1100,
            Some("session_log"),
        ),
        // claude-desktop 的代理行也能去掉 claude 会话行
        (
            "p2",
            "claude-desktop",
            "sonnet",
            20,
            3,
            0,
            0,
            200,
            5000,
            Some("proxy"),
        ),
        (
            "s2",
            "claude",
            "SONNET",
            20,
            3,
            0,
            0,
            200,
            5300,
            Some("session_log"),
        ),
        // codex 会话没有 cache_creation：0 视作未知
        ("p3", "codex", "gpt", 30, 4, 2, 9, 200, 9000, Some("proxy")),
        (
            "s3",
            "codex",
            "gpt",
            30,
            4,
            2,
            0,
            200,
            9100,
            Some("codex_session"),
        ),
        // 超出 10 分钟窗口：保留
        (
            "s4",
            "claude",
            "opus",
            10,
            2,
            1,
            5,
            200,
            1000 + 601,
            Some("session_log"),
        ),
        // 代理行失败：不参与去重，会话行保留
        (
            "p5",
            "claude",
            "haiku",
            7,
            7,
            7,
            7,
            500,
            20000,
            Some("proxy"),
        ),
        (
            "s5",
            "claude",
            "haiku",
            7,
            7,
            7,
            7,
            200,
            20010,
            Some("session_log"),
        ),
        // 模型名 unknown 也算匹配
        ("p6", "claude", "unknown", 8, 8, 8, 8, 200, 30000, None),
        (
            "s6",
            "claude",
            "opus",
            8,
            8,
            8,
            8,
            200,
            30010,
            Some("session_log"),
        ),
        // 不相关的会话行，让会话行多于代理行
        (
            "s7",
            "claude",
            "opus",
            1,
            1,
            1,
            1,
            200,
            40000,
            Some("session_log"),
        ),
        (
            "s8",
            "claude",
            "opus",
            2,
            2,
            2,
            2,
            200,
            40001,
            Some("session_log"),
        ),
        (
            "s9",
            "claude",
            "opus",
            3,
            3,
            3,
            3,
            200,
            40002,
            Some("session_log"),
        ),
    ];
    for r in rows {
        conn.execute(
            "INSERT INTO proxy_request_logs (
                    request_id, app_type, model, input_tokens, output_tokens,
                    cache_read_tokens, cache_creation_tokens, status_code, created_at, data_source
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![r.0, r.1, r.2, r.3, r.4, r.5, r.6, r.7, r.8, r.9],
        )?;
    }

    let ids = |filter: &str, start: i64, end: i64| -> Result<Vec<String>, AppError> {
        let sql = format!(
            "SELECT request_id FROM proxy_request_logs l
                 WHERE l.created_at BETWEEN ?1 AND ?2 AND {filter}
                 ORDER BY request_id"
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt
            .query_map(params![start, end], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    };

    let original = effective_usage_log_filter("l");
    // 全量（代理少于会话 → NOT IN）、只含代理行、只含会话行（直接跳过）
    for (start, end) in [(0, 50000), (900, 1050), (40000, 40002), (1100, 9100)] {
        let ranged = effective_usage_log_filter_for_range(&conn, "l", Some(start), Some(end))?;
        assert_eq!(
            ids(&ranged, start, end)?,
            ids(&original, start, end)?,
            "range {start}..{end}"
        );
    }
    let all = effective_usage_log_filter_for_range(&conn, "l", None, None)?;
    assert!(all.contains("NOT IN"));
    assert_eq!(ids(&all, 0, 50000)?, ids(&original, 0, 50000)?);
    assert_eq!(
        ids(&all, 0, 50000)?,
        vec!["p1", "p2", "p3", "p5", "p6", "s4", "s5", "s7", "s8", "s9"]
    );

    Ok(())
}

#[test]
fn test_matching_proxy_log_treats_legacy_null_data_source_as_proxy() -> Result<(), AppError> {
    let conn = Connection::open_in_memory()?;
    create_legacy_nullable_logs_table(&conn)?;
    conn.execute(
        "INSERT INTO proxy_request_logs (
                request_id, app_type, model, input_tokens, output_tokens,
                cache_read_tokens, cache_creation_tokens, status_code, created_at, data_source
            ) VALUES ('legacy-proxy', 'codex', 'gpt-5.5', 10, 2, 1, 0, 200, 1000, NULL)",
        [],
    )?;

    let key = DedupKey {
        app_type: "codex",
        data_source: "codex_session",
        model: "gpt-5.5",
        input_tokens: 10,
        output_tokens: 2,
        cache_read_tokens: 1,
        cache_creation_tokens: 0,
        created_at: 1000,
    };
    assert!(has_matching_proxy_usage_log(&conn, &key)?);

    Ok(())
}

#[test]
fn test_matching_proxy_log_matches_codex_request_model_alias() -> Result<(), AppError> {
    let db = Database::memory()?;
    let conn = lock_conn!(db.conn);
    insert_usage_log(
        &conn,
        "codex-proxy-alias",
        "codex",
        "provider-1",
        "gpt-5.6-sol",
        "proxy",
        1_000,
        100,
        20,
        10,
        0,
        200,
        "0.10",
    )?;
    conn.execute(
        "UPDATE proxy_request_logs SET request_model = ?1 WHERE request_id = ?2",
        params!["gpt-5.6-sol-joybuilder", "codex-proxy-alias"],
    )?;

    let key = DedupKey {
        app_type: "codex",
        data_source: "codex_session",
        model: "GPT-5.6-Sol-JoyBuilder",
        input_tokens: 100,
        output_tokens: 20,
        cache_read_tokens: 10,
        cache_creation_tokens: 0,
        created_at: 1_060,
    };
    assert!(has_matching_proxy_usage_log(&conn, &key)?);
    assert!(should_skip_session_insert(
        &conn,
        "codex-session-alias",
        &key
    )?);

    conn.execute(
        "UPDATE proxy_request_logs SET request_model = ?1 WHERE request_id = ?2",
        params![" gpt-5.6-sol-joybuilder ", "codex-proxy-alias"],
    )?;
    assert!(has_matching_proxy_usage_log(&conn, &key)?);
    conn.execute(
        "UPDATE proxy_request_logs SET request_model = ?1 WHERE request_id = ?2",
        params!["gpt-5.6-sol-joybuilder", "codex-proxy-alias"],
    )?;

    let mut outside_window = key;
    outside_window.created_at = 1_601;
    assert!(!has_matching_proxy_usage_log(&conn, &outside_window)?);

    let mut different_tokens = key;
    different_tokens.input_tokens += 1;
    assert!(!has_matching_proxy_usage_log(&conn, &different_tokens)?);

    let mut different_app = key;
    different_app.app_type = "gemini";
    assert!(!has_matching_proxy_usage_log(&conn, &different_app)?);

    let mut mismatched_codex_source = key;
    mismatched_codex_source.app_type = "gemini";
    mismatched_codex_source.data_source = "codex_session";
    assert!(!has_matching_proxy_usage_log(
        &conn,
        &mismatched_codex_source
    )?);

    let mut non_codex_session = key;
    non_codex_session.data_source = "session_log";
    assert!(!has_matching_proxy_usage_log(&conn, &non_codex_session)?);

    for invalid_request_model in [
        None,
        Some(""),
        Some("  "),
        Some("unknown"),
        Some(" UNKNOWN "),
    ] {
        conn.execute(
            "UPDATE proxy_request_logs SET request_model = ?1 WHERE request_id = ?2",
            params![invalid_request_model, "codex-proxy-alias"],
        )?;
        assert!(!has_matching_proxy_usage_log(&conn, &key)?);
    }

    conn.execute(
        "UPDATE proxy_request_logs SET request_model = ?1, status_code = 400 WHERE request_id = ?2",
        params!["gpt-5.6-sol-joybuilder", "codex-proxy-alias"],
    )?;
    assert!(!has_matching_proxy_usage_log(&conn, &key)?);

    Ok(())
}

#[test]
fn test_matching_proxy_log_matches_codex_prefix_model() -> Result<(), AppError> {
    let db = Database::memory()?;
    let conn = lock_conn!(db.conn);
    insert_usage_log(
        &conn,
        "codex-proxy-prefix",
        "codex",
        "provider-cpa",
        "antigravity/gemini-3.8-flash-high",
        "proxy",
        1_000,
        2_341,
        228,
        116_000,
        0,
        200,
        "0.08",
    )?;
    conn.execute(
        "UPDATE proxy_request_logs SET request_model = ?1 WHERE request_id = ?2",
        params!["antigravity/gemini-3.8-flash-high", "codex-proxy-prefix"],
    )?;

    // 会话日志经过 normalize_codex_model 剥离了前缀
    let key = DedupKey {
        app_type: "codex",
        data_source: "codex_session",
        model: "gemini-3.8-flash-high",
        input_tokens: 2_341,
        output_tokens: 228,
        cache_read_tokens: 116_000,
        cache_creation_tokens: 0,
        created_at: 1_001,
    };

    assert!(has_matching_proxy_usage_log(&conn, &key)?);
    assert!(should_skip_session_insert(
        &conn,
        "codex-session-prefix",
        &key
    )?);

    // 如果代理的 request_model 带有前缀，而实际 model 被路由改写，也能识别
    conn.execute(
        "UPDATE proxy_request_logs SET model = ?1, request_model = ?2 WHERE request_id = ?3",
        params![
            "upstream-routed-model",
            "antigravity/gemini-3.8-flash-high",
            "codex-proxy-prefix"
        ],
    )?;
    assert!(has_matching_proxy_usage_log(&conn, &key)?);

    // 多级前缀在 request_model 回退分支中也能正确识别
    conn.execute(
        "UPDATE proxy_request_logs SET model = ?1, request_model = ?2 WHERE request_id = ?3",
        params![
            "upstream-routed-model",
            "cpa/antigravity/gemini-3.8-flash-high",
            "codex-proxy-prefix"
        ],
    )?;
    assert!(has_matching_proxy_usage_log(&conn, &key)?);

    // request_model 剥离前缀后为 unknown 时不放宽匹配
    conn.execute(
        "UPDATE proxy_request_logs SET model = ?1, request_model = ?2 WHERE request_id = ?3",
        params!["upstream-routed-model", "cpa/unknown", "codex-proxy-prefix"],
    )?;
    assert!(!has_matching_proxy_usage_log(&conn, &key)?);

    // 多级前缀（如 cpa/antigravity/gemini-3.8-flash-high）同样与最后一个 '/' 之后的模型对齐
    conn.execute(
        "UPDATE proxy_request_logs SET model = ?1, request_model = ?1 WHERE request_id = ?2",
        params![
            "cpa/antigravity/gemini-3.8-flash-high",
            "codex-proxy-prefix"
        ],
    )?;
    assert!(has_matching_proxy_usage_log(&conn, &key)?);
    assert!(should_skip_session_insert(
        &conn,
        "codex-session-prefix",
        &key
    )?);

    // 模型名不同则不误伤
    let mut different_model = key;
    different_model.model = "gemini-2.5-pro";
    assert!(!has_matching_proxy_usage_log(&conn, &different_model)?);

    Ok(())
}

#[test]
fn test_effective_filter_dedups_codex_prefix_model() -> Result<(), AppError> {
    let db = Database::memory()?;
    let conn = lock_conn!(db.conn);
    insert_usage_log(
        &conn,
        "codex-proxy-prefix",
        "codex",
        "provider-cpa",
        "antigravity/gemini-3.8-flash-high",
        "proxy",
        1_000,
        2_341,
        228,
        116_000,
        0,
        200,
        "0.08",
    )?;
    conn.execute(
        "UPDATE proxy_request_logs SET request_model = ?1 WHERE request_id = ?2",
        params!["antigravity/gemini-3.8-flash-high", "codex-proxy-prefix"],
    )?;
    insert_usage_log(
        &conn,
        "codex-session-prefix",
        "codex",
        "_codex_session",
        "gemini-3.8-flash-high",
        "codex_session",
        1_001,
        2_341,
        228,
        116_000,
        0,
        200,
        "0.08",
    )?;

    let filter = effective_usage_log_filter("l");
    let sql =
        format!("SELECT request_id FROM proxy_request_logs l WHERE {filter} ORDER BY request_id");
    let request_ids = conn
        .prepare(&sql)?
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(request_ids, vec!["codex-proxy-prefix".to_string()]);

    // range filter (JOIN 路径) 同样正确去重
    let range_filter = effective_usage_log_filter_for_range(&conn, "l", None, None)?;
    let range_sql = format!(
        "SELECT request_id FROM proxy_request_logs l WHERE {range_filter} ORDER BY request_id"
    );
    let range_ids = conn
        .prepare(&range_sql)?
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(range_ids, vec!["codex-proxy-prefix".to_string()]);

    // 多级前缀在单行过滤与 range JOIN 中也能正确去重
    conn.execute(
        "UPDATE proxy_request_logs SET model = ?1, request_model = ?1 WHERE request_id = ?2",
        params![
            "cpa/antigravity/gemini-3.8-flash-high",
            "codex-proxy-prefix"
        ],
    )?;
    let filter = effective_usage_log_filter("l");
    let sql =
        format!("SELECT request_id FROM proxy_request_logs l WHERE {filter} ORDER BY request_id");
    let request_ids = conn
        .prepare(&sql)?
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(request_ids, vec!["codex-proxy-prefix".to_string()]);

    let range_filter = effective_usage_log_filter_for_range(&conn, "l", None, None)?;
    let range_sql = format!(
        "SELECT request_id FROM proxy_request_logs l WHERE {range_filter} ORDER BY request_id"
    );
    let range_ids = conn
        .prepare(&range_sql)?
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(range_ids, vec!["codex-proxy-prefix".to_string()]);

    // 当代理的 model 被上游路由改写、依赖多级 request_model 回退去重时，单行过滤与 range JOIN 同样正确去重
    conn.execute(
        "UPDATE proxy_request_logs SET model = ?1, request_model = ?2 WHERE request_id = ?3",
        params![
            "upstream-routed-model",
            "cpa/antigravity/gemini-3.8-flash-high",
            "codex-proxy-prefix"
        ],
    )?;
    let filter = effective_usage_log_filter("l");
    let sql =
        format!("SELECT request_id FROM proxy_request_logs l WHERE {filter} ORDER BY request_id");
    let request_ids = conn
        .prepare(&sql)?
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(request_ids, vec!["codex-proxy-prefix".to_string()]);

    let range_filter = effective_usage_log_filter_for_range(&conn, "l", None, None)?;
    let range_sql = format!(
        "SELECT request_id FROM proxy_request_logs l WHERE {range_filter} ORDER BY request_id"
    );
    let range_ids = conn
        .prepare(&range_sql)?
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(range_ids, vec!["codex-proxy-prefix".to_string()]);

    Ok(())
}

#[test]
fn test_effective_filter_dedups_only_codex_request_model_alias() -> Result<(), AppError> {
    let db = Database::memory()?;
    let conn = lock_conn!(db.conn);
    insert_usage_log(
        &conn,
        "codex-proxy-alias",
        "codex",
        "provider-1",
        "gpt-5.6-sol",
        "proxy",
        1_000,
        100,
        20,
        10,
        0,
        200,
        "0.10",
    )?;
    insert_usage_log(
        &conn,
        "codex-session-alias",
        "codex",
        "_codex_session",
        "gpt-5.6-sol-joybuilder",
        "codex_session",
        1_060,
        100,
        20,
        10,
        0,
        200,
        "0.10",
    )?;
    insert_usage_log(
        &conn,
        "gemini-proxy-alias",
        "gemini",
        "provider-2",
        "gemini-2.5-pro",
        "proxy",
        2_000,
        200,
        40,
        30,
        0,
        200,
        "0.20",
    )?;
    insert_usage_log(
        &conn,
        "gemini-session-alias",
        "gemini",
        "_gemini_session",
        "gemini-logical-alias",
        "gemini_session",
        2_060,
        200,
        40,
        30,
        0,
        200,
        "0.20",
    )?;
    conn.execute(
        "UPDATE proxy_request_logs SET request_model = ?1 WHERE request_id = ?2",
        params![" gpt-5.6-sol-joybuilder ", "codex-proxy-alias"],
    )?;
    conn.execute(
        "UPDATE proxy_request_logs SET request_model = ?1 WHERE request_id = ?2",
        params!["gemini-logical-alias", "gemini-proxy-alias"],
    )?;

    let filter = effective_usage_log_filter("l");
    let sql =
        format!("SELECT request_id FROM proxy_request_logs l WHERE {filter} ORDER BY request_id");
    let request_ids = conn
        .prepare(&sql)?
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(
        request_ids,
        vec![
            "codex-proxy-alias".to_string(),
            "gemini-proxy-alias".to_string(),
            "gemini-session-alias".to_string(),
        ]
    );

    Ok(())
}

#[test]
fn test_matching_proxy_log_matches_claude_desktop_for_claude_session() -> Result<(), AppError> {
    let conn = Connection::open_in_memory()?;
    create_legacy_nullable_logs_table(&conn)?;
    conn.execute(
        "INSERT INTO proxy_request_logs (
                request_id, app_type, model, input_tokens, output_tokens,
                cache_read_tokens, cache_creation_tokens, status_code, created_at, data_source
            ) VALUES ('desktop-proxy', 'claude-desktop', 'claude-sonnet-4-5', 100, 20, 10, 5, 200, 1000, 'proxy')",
        [],
    )?;

    let key = DedupKey {
        app_type: "claude",
        data_source: "session_log",
        model: "claude-sonnet-4-5",
        input_tokens: 100,
        output_tokens: 20,
        cache_read_tokens: 10,
        cache_creation_tokens: 5,
        created_at: 1060,
    };
    assert!(has_matching_proxy_usage_log(&conn, &key)?);

    let mut outside_window = key;
    outside_window.created_at = 1_601;
    assert!(!has_matching_proxy_usage_log(&conn, &outside_window)?);

    let mut different_model = key;
    different_model.model = "claude-opus-4-5";
    assert!(!has_matching_proxy_usage_log(&conn, &different_model)?);

    let mut different_input = key;
    different_input.input_tokens += 1;
    assert!(!has_matching_proxy_usage_log(&conn, &different_input)?);

    let mut different_cache_creation = key;
    different_cache_creation.cache_creation_tokens += 1;
    assert!(!has_matching_proxy_usage_log(
        &conn,
        &different_cache_creation
    )?);

    Ok(())
}

#[test]
fn test_effective_filter_dedups_claude_session_against_desktop_proxy() -> Result<(), AppError> {
    let conn = Connection::open_in_memory()?;
    create_legacy_nullable_logs_table(&conn)?;
    conn.execute_batch(
        "INSERT INTO proxy_request_logs (
                request_id, app_type, model, input_tokens, output_tokens,
                cache_read_tokens, cache_creation_tokens, status_code, created_at, data_source
            ) VALUES
                ('desktop-proxy', 'claude-desktop', 'claude-sonnet-4-5', 100, 20, 10, 5, 200, 1000, 'proxy'),
                ('claude-session', 'claude', 'claude-sonnet-4-5', 100, 20, 10, 5, 200, 1060, 'session_log');",
    )?;

    let filter = effective_usage_log_filter("l");
    let sql = format!("SELECT request_id FROM proxy_request_logs l WHERE {filter}");
    let request_ids = conn
        .prepare(&sql)?
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(request_ids, vec!["desktop-proxy"]);

    Ok(())
}

#[test]
fn test_claude_desktop_folds_into_claude_for_display() -> Result<(), AppError> {
    let db = Database::memory()?;
    let ts = local_ts(2026, 6, 10, 12, 0, 0);

    {
        let conn = lock_conn!(db.conn);
        // 一条 Claude Code 行 + 一条 Claude Desktop 网关行，同一时间窗。
        insert_usage_log(
            &conn,
            "cc-1",
            "claude",
            "p-claude",
            "claude-sonnet-4-5",
            "proxy",
            ts,
            100,
            10,
            0,
            0,
            200,
            "0.5",
        )?;
        insert_usage_log(
            &conn,
            "cd-1",
            "claude-desktop",
            "p-desktop",
            "claude-opus-4-8",
            "proxy",
            ts,
            200,
            20,
            0,
            0,
            200,
            "1.5",
        )?;
    }

    // ① 分应用汇总：desktop 折叠进 claude，不再单列 claude-desktop 桶。
    let by_app = db.get_usage_summary_by_app(None, None, None, None)?;
    assert_eq!(by_app.len(), 1, "应只剩一个合并后的 claude 桶");
    assert_eq!(by_app[0].app_type, "claude");
    assert_eq!(by_app[0].summary.total_requests, 2, "两条行都计入 claude");
    assert!(
        !by_app.iter().any(|a| a.app_type == "claude-desktop"),
        "不应再出现 claude-desktop 桶"
    );

    // ② 选中 claude 过滤：汇总应同时覆盖 desktop 行。
    let claude_summary = db.get_usage_summary(None, None, Some("claude"), None, None)?;
    assert_eq!(claude_summary.total_requests, 2);

    // ③ 请求日志按 claude 过滤返回两行，且 desktop 行投影仍是原始 app_type。
    let logs = db.get_request_logs(
        &LogFilters {
            app_type: Some("claude".to_string()),
            ..Default::default()
        },
        0, // 页码从 0 开始
        50,
    )?;
    assert_eq!(logs.total, 2, "claude 过滤含 desktop 行");
    assert!(
        logs.data.iter().any(|r| r.app_type == "claude-desktop"),
        "详情面板需要看到真实入口，行投影不可被折叠"
    );

    // ④ 折叠不外溢：codex 过滤为空。
    let codex_summary = db.get_usage_summary(None, None, Some("codex"), None, None)?;
    assert_eq!(codex_summary.total_requests, 0);

    Ok(())
}

#[test]
fn test_backfill_missing_usage_costs_uses_new_gpt_5_5_pricing() -> Result<(), AppError> {
    let db = Database::memory()?;

    {
        let conn = lock_conn!(db.conn);
        insert_usage_log(
            &conn,
            "codex-gpt-5-5-zero-cost",
            "codex",
            "_codex_session",
            "gpt-5.5",
            "codex_session",
            1000,
            1_000_000,
            1_000_000,
            0,
            0,
            200,
            "0",
        )?;
    }

    assert_eq!(db.backfill_missing_usage_costs()?, 1);

    let conn = lock_conn!(db.conn);
    let (input_cost, output_cost, total_cost): (String, String, String) = conn.query_row(
        "SELECT input_cost_usd, output_cost_usd, total_cost_usd
             FROM proxy_request_logs WHERE request_id = 'codex-gpt-5-5-zero-cost'",
        [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;
    assert_eq!(input_cost, "5.000000");
    assert_eq!(output_cost, "30.000000");
    assert_eq!(total_cost, "35.000000");

    Ok(())
}

#[test]
fn test_backfill_new_model_pricing_after_upgrade() -> Result<(), AppError> {
    let db = Database::memory()?;
    let cases = [
        (
            "anthropic/claude-opus-5.5",
            "claude",
            1_000_000,
            ["4.000000", "20.000000", "0.200000", "5.000000", "29.200000"],
        ),
        (
            "OpenAI/GPT-6-SOL@HIGH",
            "codex",
            3_000_000,
            ["2.000000", "10.000000", "0.200000", "2.500000", "14.700000"],
        ),
        (
            "OpenAI/GPT-6.1-SOL@HIGH",
            "codex",
            3_000_000,
            ["2.000000", "10.000000", "0.100000", "2.500000", "14.600000"],
        ),
        (
            "gpt-6-luna",
            "codex",
            3_000_000,
            ["0.100000", "0.500000", "0.010000", "0.125000", "0.735000"],
        ),
        (
            "gpt-5.6-cyber",
            "codex",
            3_000_000,
            [
                "12.500000",
                "75.000000",
                "1.250000",
                "15.625000",
                "104.375000",
            ],
        ),
        // Pro 系列无缓存折扣，缓存列记 0
        (
            "gpt-5.5-pro",
            "codex",
            3_000_000,
            [
                "30.000000",
                "180.000000",
                "0.000000",
                "0.000000",
                "210.000000",
            ],
        ),
        // 剥日期后缀后精确命中 gpt-4o-mini，不会落到更短的 gpt-4o
        (
            "gpt-4o-mini-2024-07-18",
            "codex",
            3_000_000,
            ["0.150000", "0.600000", "0.075000", "0.000000", "0.825000"],
        ),
        (
            "qwen/qwen4-max",
            "claude",
            1_000_000,
            ["1.500000", "4.500000", "0.075000", "1.875000", "7.950000"],
        ),
    ];
    {
        let conn = lock_conn!(db.conn);
        // Simulate an existing database with unpriced usage before the update.
        conn.execute(
            "DELETE FROM model_pricing WHERE model_id IN
                 ('claude-opus-5-5', 'gpt-6-sol', 'gpt-6.1-sol', 'gpt-6-luna', 'gpt-5.6-cyber',
                  'gpt-5.5-pro', 'gpt-4o-mini', 'qwen4-max')",
            [],
        )?;
        for (model, app, input, _) in &cases {
            insert_usage_log(
                &conn, model, app, "p1", model, "proxy", 1000, *input, 1_000_000, 1_000_000,
                1_000_000, 200, "0",
            )?;
        }
        conn.execute(
            "UPDATE proxy_request_logs SET input_token_semantics = ?1",
            [INPUT_TOKEN_SEMANTICS_TOTAL],
        )?;
    }
    assert_eq!(db.backfill_missing_usage_costs()?, 0);
    db.ensure_model_pricing_seeded()?;
    assert_eq!(db.backfill_missing_usage_costs()?, cases.len() as u64);

    let conn = lock_conn!(db.conn);
    for (model, _, _, expected) in cases {
        let costs: [String; 5] = conn.query_row(
            "SELECT input_cost_usd, output_cost_usd, cache_read_cost_usd,
                        cache_creation_cost_usd, total_cost_usd
                 FROM proxy_request_logs WHERE request_id = ?1",
            [model],
            |row| {
                Ok([
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ])
            },
        )?;
        assert_eq!(costs, expected, "{model}");
    }
    Ok(())
}

#[test]
fn test_backfill_distinguishes_legacy_and_total_cache_semantics() -> Result<(), AppError> {
    let db = Database::memory()?;

    {
        let conn = lock_conn!(db.conn);
        // v12 mirror row: input = fresh + read; creation was reported separately.
        insert_usage_log(
            &conn,
            "legacy-cache-semantics",
            "codex",
            "p1",
            "gpt-5.5",
            "proxy",
            1000,
            800_000,
            0,
            600_000,
            200_000,
            200,
            "0",
        )?;
        // v13 proxy row: input = fresh + read + creation.
        insert_usage_log(
            &conn,
            "total-cache-semantics",
            "codex",
            "p1",
            "gpt-5.5",
            "proxy",
            1001,
            1_000_000,
            0,
            600_000,
            200_000,
            200,
            "0",
        )?;
        conn.execute(
            "UPDATE proxy_request_logs
                 SET input_token_semantics = ?1
                 WHERE request_id = 'total-cache-semantics'",
            [INPUT_TOKEN_SEMANTICS_TOTAL],
        )?;
    }

    assert_eq!(db.backfill_missing_usage_costs()?, 2);

    let conn = lock_conn!(db.conn);
    let mut stmt = conn.prepare(
        "SELECT request_id, input_cost_usd
             FROM proxy_request_logs
             WHERE request_id IN ('legacy-cache-semantics', 'total-cache-semantics')
             ORDER BY request_id",
    )?;
    let rows = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(
        rows,
        vec![
            ("legacy-cache-semantics".to_string(), "1.000000".to_string()),
            ("total-cache-semantics".to_string(), "1.000000".to_string()),
        ]
    );

    Ok(())
}

#[test]
fn test_request_logs_report_fresh_input_like_the_summary() -> Result<(), AppError> {
    let db = Database::memory()?;
    {
        let conn = lock_conn!(db.conn);
        for (request_id, app_type) in [
            ("codex-legacy", "codex"),
            ("codex-total", "codex"),
            ("claude", "claude"),
        ] {
            insert_usage_log(
                &conn,
                request_id,
                app_type,
                "p1",
                "test-model",
                "proxy",
                1000,
                1000,
                50,
                300,
                600,
                200,
                "0.01",
            )?;
        }
        conn.execute(
            "UPDATE proxy_request_logs
                 SET input_token_semantics = ?1
                 WHERE request_id = 'codex-total'",
            [INPUT_TOKEN_SEMANTICS_TOTAL],
        )?;
    }

    let logs = db.get_request_logs(&LogFilters::default(), 0, 10)?;
    let fresh: HashMap<&str, u32> = logs
        .data
        .iter()
        .map(|log| (log.request_id.as_str(), log.fresh_input_tokens))
        .collect();
    assert_eq!(
        fresh,
        HashMap::from([
            ("codex-legacy", 700),
            ("codex-total", 100),
            ("claude", 1000)
        ])
    );
    let summary = db.get_usage_summary(None, None, None, None, None)?;
    assert_eq!(
        fresh.values().map(|&tokens| u64::from(tokens)).sum::<u64>(),
        summary.total_input_tokens
    );

    let detail = db
        .get_request_detail("codex-total")?
        .expect("codex-total row");
    assert_eq!(
        serde_json::to_value(&detail).unwrap()["freshInputTokens"],
        100
    );
    Ok(())
}

#[test]
fn test_backfill_deducts_cache_read_for_grokbuild_total_rows() -> Result<(), AppError> {
    // 回归：回填侧的 cache-inclusive 判定曾硬编码 codex|gemini 漏掉
    // grokbuild，导致 TOTAL 行按全量 input 计价、cache_read 双算。
    // 判定收敛到 sql_helpers::is_cache_inclusive_app 后按 450 fresh 计价。
    let db = Database::memory()?;
    {
        let conn = lock_conn!(db.conn);
        insert_usage_log(
            &conn,
            "grokbuild-total-backfill",
            "grokbuild",
            "_grok_session",
            "grok-4.5",
            "grok_session",
            1000,
            700,
            100,
            250,
            0,
            200,
            "0",
        )?;
        conn.execute(
            "UPDATE proxy_request_logs
                 SET input_token_semantics = ?1
                 WHERE request_id = 'grokbuild-total-backfill'",
            [INPUT_TOKEN_SEMANTICS_TOTAL],
        )?;
    }

    assert_eq!(db.backfill_missing_usage_costs()?, 1);

    let conn = lock_conn!(db.conn);
    let (input_cost, cache_read_cost, total_cost): (String, String, String) = conn.query_row(
        "SELECT input_cost_usd, cache_read_cost_usd, total_cost_usd
             FROM proxy_request_logs WHERE request_id = 'grokbuild-total-backfill'",
        [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;
    // grok-4.5 定价 2/6/0.30：input = (700-250)×2/1M，cache_read = 250×0.3/1M
    assert_eq!(input_cost, "0.000900");
    assert_eq!(cache_read_cost, "0.000075");
    assert_eq!(total_cost, "0.001575");
    Ok(())
}

#[test]
fn test_backfill_missing_usage_costs_uses_stored_multiplier() -> Result<(), AppError> {
    let db = Database::memory()?;

    {
        let conn = lock_conn!(db.conn);
        insert_usage_log(
            &conn,
            "codex-gpt-5-5-multiplier",
            "codex",
            "_codex_session",
            "gpt-5.5",
            "codex_session",
            1000,
            1_000_000,
            0,
            0,
            0,
            200,
            "0",
        )?;
        conn.execute(
            "UPDATE proxy_request_logs
                 SET cost_multiplier = '1.5'
                 WHERE request_id = 'codex-gpt-5-5-multiplier'",
            [],
        )?;
    }

    assert_eq!(db.backfill_missing_usage_costs()?, 1);

    let conn = lock_conn!(db.conn);
    let (input_cost, total_cost): (String, String) = conn.query_row(
        "SELECT input_cost_usd, total_cost_usd
             FROM proxy_request_logs WHERE request_id = 'codex-gpt-5-5-multiplier'",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    assert_eq!(input_cost, "5.000000");
    assert_eq!(total_cost, "7.500000");

    Ok(())
}

#[test]
fn test_backfill_skips_zero_multiplier_rows() -> Result<(), AppError> {
    let db = Database::memory()?;

    {
        let conn = lock_conn!(db.conn);
        insert_usage_log(
            &conn,
            "codex-gpt-5-5-free",
            "codex",
            "free-relay",
            "gpt-5.5",
            "proxy",
            1000,
            1_000_000,
            0,
            0,
            0,
            200,
            "0",
        )?;
        conn.execute(
            "UPDATE proxy_request_logs
                 SET cost_multiplier = '0', input_cost_usd = '1.000000'
                 WHERE request_id = 'codex-gpt-5-5-free'",
            [],
        )?;
    }

    // 已按 0 倍率计过价的行不是缺价行：回填不得重写它，也不得每次读取都 UPDATE。
    assert_eq!(db.backfill_missing_usage_costs()?, 0);
    let detail = db
        .get_request_detail("codex-gpt-5-5-free")?
        .expect("row exists");
    assert_eq!(detail.input_cost_usd, "1.000000");
    assert_eq!(detail.total_cost_usd, "0");

    Ok(())
}

#[test]
fn test_backfill_keeps_sub_micro_dollar_costs() -> Result<(), AppError> {
    let db = Database::memory()?;

    {
        let conn = lock_conn!(db.conn);
        conn.execute(
            "INSERT OR REPLACE INTO model_pricing
                 (model_id, display_name, input_cost_per_million, output_cost_per_million)
                 VALUES ('tiny-model', 'Tiny', '2', '0')",
            [],
        )?;
        insert_usage_log(
            &conn,
            "tiny-request",
            "claude",
            "relay",
            "tiny-model",
            "proxy",
            1000,
            3,
            0,
            0,
            0,
            200,
            "0",
        )?;
        conn.execute(
            "UPDATE proxy_request_logs SET cost_multiplier = '0.07'
                 WHERE request_id = 'tiny-request'",
            [],
        )?;
    }

    // 3 token × $2/M × 0.07 = $0.00000042：按 6 位小数存会变成 0，下次读取又被当成缺价重算。
    assert_eq!(db.backfill_missing_usage_costs()?, 1);
    let detail = db.get_request_detail("tiny-request")?.expect("row exists");
    assert_eq!(detail.input_cost_usd, "0.000006");
    assert_eq!(detail.total_cost_usd, "0.00000042");
    assert_eq!(db.backfill_missing_usage_costs()?, 0);

    Ok(())
}

#[test]
fn test_backfill_missing_usage_costs_falls_back_to_request_model() -> Result<(), AppError> {
    let db = Database::memory()?;

    {
        let conn = lock_conn!(db.conn);
        conn.execute(
            "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model, request_model,
                    input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                    input_cost_usd, output_cost_usd, cache_read_cost_usd, cache_creation_cost_usd,
                    total_cost_usd, latency_ms, status_code, created_at, data_source
                ) VALUES (
                    'codex-request-model-fallback', '_codex_session', 'codex', 'unknown', 'gpt-5.5',
                    1000000, 0, 0, 0,
                    '0', '0', '0', '0',
                    '0', 100, 200, 1000, 'codex_session'
                )",
            [],
        )?;
    }

    assert_eq!(db.backfill_missing_usage_costs()?, 1);

    let conn = lock_conn!(db.conn);
    let total_cost: String = conn.query_row(
        "SELECT total_cost_usd
             FROM proxy_request_logs WHERE request_id = 'codex-request-model-fallback'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(total_cost, "5.000000");

    Ok(())
}

#[test]
fn test_backfill_skips_request_model_fallback_for_real_unpriced_model() -> Result<(), AppError> {
    let db = Database::memory()?;

    {
        let conn = lock_conn!(db.conn);
        // 路由接管场景：model 是上游回显的真实模型（缺定价），request_model
        // 是客户端别名（有定价）。回填不得按别名定价，必须保持 0 成本等待补价。
        conn.execute(
            "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model, request_model,
                    input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                    input_cost_usd, output_cost_usd, cache_read_cost_usd, cache_creation_cost_usd,
                    total_cost_usd, latency_ms, status_code, created_at, data_source
                ) VALUES (
                    'takeover-unpriced-model', 'provider-1', 'claude',
                    'takeover-real-model-unpriced', 'claude-sonnet-4-6',
                    1000000, 0, 0, 0,
                    '0', '0', '0', '0',
                    '0', 100, 200, 1000, 'proxy'
                )",
            [],
        )?;
    }

    // request_model（claude-sonnet-4-6）有定价，但 model 是真实模型名：不得回退
    assert_eq!(db.backfill_missing_usage_costs()?, 0);

    {
        let conn = lock_conn!(db.conn);
        let total_cost: String = conn.query_row(
            "SELECT total_cost_usd
                 FROM proxy_request_logs WHERE request_id = 'takeover-unpriced-model'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(total_cost, "0");

        // 补上真实模型定价后，回填必须按真实模型价格修复（0 成本行未被污染固化）
        conn.execute(
            "INSERT INTO model_pricing (model_id, display_name, input_cost_per_million, output_cost_per_million)
                 VALUES ('takeover-real-model-unpriced', 'Takeover Real Model', '0.6', '2.5')",
            [],
        )?;
    }

    assert_eq!(db.backfill_missing_usage_costs()?, 1);

    let conn = lock_conn!(db.conn);
    let total_cost: String = conn.query_row(
        "SELECT total_cost_usd
             FROM proxy_request_logs WHERE request_id = 'takeover-unpriced-model'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(total_cost, "0.600000");

    Ok(())
}

#[test]
fn test_backfill_uses_persisted_pricing_model() -> Result<(), AppError> {
    let db = Database::memory()?;

    {
        let conn = lock_conn!(db.conn);
        // request 计价模式 + 接管：写入时锚定出站模型 kimi-k2-novel（当时缺价），
        // 但上游回显了别名 → model/request_model 都是 claude-sonnet-4-6（有定价）。
        // 回填必须按落库的 pricing_model 重算，不得换用 model 列的别名价格。
        conn.execute(
            "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model, request_model, pricing_model,
                    input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                    input_cost_usd, output_cost_usd, cache_read_cost_usd, cache_creation_cost_usd,
                    total_cost_usd, latency_ms, status_code, created_at, data_source
                ) VALUES (
                    'persisted-pricing-model', 'provider-1', 'claude',
                    'claude-sonnet-4-6', 'claude-sonnet-4-6', 'kimi-k2-novel',
                    1000000, 0, 0, 0,
                    '0', '0', '0', '0',
                    '0', 100, 200, 1000, 'proxy'
                )",
            [],
        )?;
    }

    // pricing_model（kimi-k2-novel）缺价：不得回退到 model 列的别名价格
    assert_eq!(db.backfill_missing_usage_costs()?, 0);

    {
        let conn = lock_conn!(db.conn);
        conn.execute(
            "INSERT INTO model_pricing (model_id, display_name, input_cost_per_million, output_cost_per_million)
                 VALUES ('kimi-k2-novel', 'Kimi K2 Novel', '0.6', '2.5')",
            [],
        )?;
    }

    // 按 pricing_model 也能定位到该行（model/request_model 都不是 kimi-k2-novel）
    assert_eq!(
        db.backfill_missing_usage_costs_for_model("kimi-k2-novel")?,
        1
    );

    let conn = lock_conn!(db.conn);
    let total_cost: String = conn.query_row(
        "SELECT total_cost_usd
             FROM proxy_request_logs WHERE request_id = 'persisted-pricing-model'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(total_cost, "0.600000");

    Ok(())
}

#[test]
fn test_scoped_backfill_matches_raw_alias_rows() -> Result<(), AppError> {
    let db = Database::memory()?;

    {
        let conn = lock_conn!(db.conn);
        // 代理日志按上游原文落库：带路由前缀和 :free 后缀的别名形式。
        // 精准回填的筛选必须归一化后匹配，否则这类行要等全量回填才更新。
        insert_usage_log(
            &conn,
            "openrouter-alias-zero-cost",
            "claude",
            "provider-1",
            "openrouter/moonshot/kimi-k2-novel:free",
            "proxy",
            1000,
            1_000_000,
            0,
            0,
            0,
            200,
            "0",
        )?;
    }

    // 定价缺失时不应回填
    assert_eq!(db.backfill_missing_usage_costs()?, 0);

    {
        let conn = lock_conn!(db.conn);
        conn.execute(
            "INSERT INTO model_pricing (model_id, display_name, input_cost_per_million, output_cost_per_million)
                 VALUES ('kimi-k2-novel', 'Kimi K2 Novel', '0.6', '2.5')",
            [],
        )?;
    }

    // 按归一化 ID 精准回填，应命中以原始别名落库的行
    assert_eq!(
        db.backfill_missing_usage_costs_for_model("kimi-k2-novel")?,
        1
    );

    let conn = lock_conn!(db.conn);
    let total_cost: String = conn.query_row(
        "SELECT total_cost_usd
             FROM proxy_request_logs WHERE request_id = 'openrouter-alias-zero-cost'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(total_cost, "0.600000");

    Ok(())
}

#[test]
fn test_backfill_missing_usage_costs_keeps_claude_fresh_input() -> Result<(), AppError> {
    let db = Database::memory()?;

    {
        let conn = lock_conn!(db.conn);
        insert_usage_log(
            &conn,
            "claude-cache-fresh-input",
            "claude",
            "_session",
            "claude-haiku-4-5",
            "session_log",
            1000,
            100,
            0,
            200,
            0,
            200,
            "0",
        )?;
    }

    assert_eq!(db.backfill_missing_usage_costs()?, 1);

    let conn = lock_conn!(db.conn);
    let (input_cost, cache_read_cost, total_cost): (String, String, String) = conn.query_row(
        "SELECT input_cost_usd, cache_read_cost_usd, total_cost_usd
             FROM proxy_request_logs WHERE request_id = 'claude-cache-fresh-input'",
        [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;
    assert_eq!(input_cost, "0.000100");
    assert_eq!(cache_read_cost, "0.000020");
    assert_eq!(total_cost, "0.000120");

    Ok(())
}

#[test]
fn test_get_usage_summary() -> Result<(), AppError> {
    let db = Database::memory()?;

    // 插入测试数据
    {
        let conn = lock_conn!(db.conn);
        conn.execute(
            "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model,
                    input_tokens, output_tokens, total_cost_usd,
                    latency_ms, status_code, created_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            params!["req1", "p1", "claude", "claude-3", 100, 50, "0.01", 100, 200, 1000],
        )?;
        conn.execute(
            "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model,
                    input_tokens, output_tokens, total_cost_usd,
                    latency_ms, status_code, created_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            params!["req2", "p1", "claude", "claude-3", 200, 100, "0.02", 150, 200, 2000],
        )?;
    }

    let summary = db.get_usage_summary(None, None, None, None, None)?;
    assert_eq!(summary.total_requests, 2);
    assert_eq!(summary.success_rate, 100.0);

    Ok(())
}

#[test]
fn test_get_session_usage_summary_counts_only_session_log_rows() -> Result<(), AppError> {
    let db = Database::memory()?;

    {
        let conn = lock_conn!(db.conn);
        // (request_id, app_type, session_id, data_source, input, output, cache_read, cost)
        let rows = [
            (
                "s1",
                "claude",
                "sess-a",
                "session_log",
                10,
                200,
                5000,
                "0.10",
            ),
            (
                "s2",
                "claude-desktop",
                "sess-a",
                "session_log",
                20,
                300,
                7000,
                "0.20",
            ),
            // 代理行即便会话 ID 相同也不数，避免与会话日志重复
            ("p1", "claude", "sess-a", "proxy", 10, 200, 5000, "0.10"),
            ("s3", "claude", "sess-b", "session_log", 1, 1, 1, "9.00"),
            ("s4", "codex", "sess-a", "codex_session", 1, 1, 0, "9.00"),
        ];
        for (id, app, session, source, input, output, cache_read, cost) in rows {
            conn.execute(
                "INSERT INTO proxy_request_logs (
                        request_id, provider_id, app_type, model,
                        input_tokens, output_tokens, cache_read_tokens, total_cost_usd,
                        latency_ms, status_code, created_at, session_id, data_source
                    ) VALUES (?, 'p1', ?, 'claude-x', ?, ?, ?, ?, 0, 200, 1000, ?, ?)",
                params![id, app, input, output, cache_read, cost, session, source],
            )?;
        }
    }

    let summary = db.get_session_usage_summary("claude", "sess-a")?;
    assert_eq!(summary.total_requests, 2);
    assert_eq!(summary.real_total_tokens, 10 + 200 + 5000 + 20 + 300 + 7000);
    assert_eq!(summary.total_cost, "0.300000");

    let empty = db.get_session_usage_summary("claude", "missing")?;
    assert_eq!(empty.total_requests, 0);

    Ok(())
}

/// 最早日期：明细和日聚合取较早的一天，筛选与 Dashboard 同口径（claude 包含 claude-desktop）。
#[test]
fn test_get_first_usage_date_spans_rollups_and_logs() -> Result<(), AppError> {
    let db = Database::memory()?;
    assert_eq!(db.get_first_usage_date(None, None, None)?, None);

    {
        let conn = lock_conn!(db.conn);
        conn.execute(
            "INSERT INTO usage_daily_rollups (
                    date, app_type, provider_id, model,
                    request_count, success_count, input_tokens, output_tokens,
                    cache_read_tokens, cache_creation_tokens, total_cost_usd, avg_latency_ms
                ) VALUES ('2024-03-05', 'claude', 'p1', 'claude-3', 1, 1, 10, 5, 0, 0, '0', 100)",
            [],
        )?;
        insert_usage_log(
            &conn,
            "codex-1",
            "codex",
            "p2",
            "gpt-5.5",
            "proxy",
            local_ts(2025, 1, 2, 10, 0, 0),
            10,
            5,
            0,
            0,
            200,
            "0",
        )?;
        insert_usage_log(
            &conn,
            "desktop-1",
            "claude-desktop",
            "p3",
            "claude-3",
            "proxy",
            local_ts(2023, 12, 31, 23, 30, 0),
            10,
            5,
            0,
            0,
            200,
            "0",
        )?;
    }

    assert_eq!(
        db.get_first_usage_date(None, None, None)?.as_deref(),
        Some("2023-12-31")
    );
    assert_eq!(
        db.get_first_usage_date(Some("claude"), None, None)?
            .as_deref(),
        Some("2023-12-31")
    );
    assert_eq!(
        db.get_first_usage_date(Some("codex"), None, None)?
            .as_deref(),
        Some("2025-01-02")
    );
    assert_eq!(
        db.get_first_usage_date(None, None, Some("claude-3"))?
            .as_deref(),
        Some("2023-12-31")
    );
    assert_eq!(db.get_first_usage_date(Some("gemini"), None, None)?, None);

    Ok(())
}

#[test]
fn test_get_usage_summary_excludes_partial_rollup_boundary_days() -> Result<(), AppError> {
    let db = Database::memory()?;
    let start = local_ts(2024, 1, 1, 12, 0, 0);
    let end = local_ts(2024, 1, 3, 12, 0, 0);

    {
        let conn = lock_conn!(db.conn);
        conn.execute(
            "INSERT INTO usage_daily_rollups (
                    date, app_type, provider_id, model,
                    request_count, success_count, input_tokens, output_tokens,
                    cache_read_tokens, cache_creation_tokens, total_cost_usd, avg_latency_ms
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            params![
                "2024-01-01",
                "claude",
                "p1",
                "claude-3",
                10,
                10,
                1000,
                500,
                0,
                0,
                "1.00",
                100
            ],
        )?;
        conn.execute(
            "INSERT INTO usage_daily_rollups (
                    date, app_type, provider_id, model,
                    request_count, success_count, input_tokens, output_tokens,
                    cache_read_tokens, cache_creation_tokens, total_cost_usd, avg_latency_ms
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            params![
                "2024-01-02",
                "claude",
                "p1",
                "claude-3",
                20,
                19,
                2000,
                1000,
                0,
                0,
                "2.00",
                120
            ],
        )?;
        conn.execute(
            "INSERT INTO usage_daily_rollups (
                    date, app_type, provider_id, model,
                    request_count, success_count, input_tokens, output_tokens,
                    cache_read_tokens, cache_creation_tokens, total_cost_usd, avg_latency_ms
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            params![
                "2024-01-03",
                "claude",
                "p1",
                "claude-3",
                30,
                29,
                3000,
                1500,
                0,
                0,
                "3.00",
                140
            ],
        )?;
    }

    let summary = db.get_usage_summary(Some(start), Some(end), Some("claude"), None, None)?;
    assert_eq!(summary.total_requests, 20);
    assert_eq!(summary.total_input_tokens, 2000);
    assert_eq!(summary.total_output_tokens, 1000);

    Ok(())
}

#[test]
fn test_get_request_detail_reads_proxy_and_session_rows() -> Result<(), AppError> {
    let db = Database::memory()?;
    let ts = local_ts(2026, 6, 10, 12, 0, 0);

    {
        let conn = lock_conn!(db.conn);
        // providers 也有 created_at / cost_multiplier 列：详情查询的列必须带表别名，
        // 否则 SQLite 报 ambiguous column name，整个命令失败。
        conn.execute(
            "INSERT INTO providers (id, app_type, name, settings_config, created_at) VALUES
                 ('prov-a', 'claude', 'Packy', '{}', 1)",
            [],
        )?;
        insert_usage_log(
            &conn,
            "a-1",
            "claude",
            "prov-a",
            "claude-sonnet-4-6",
            "proxy",
            ts,
            100,
            10,
            0,
            0,
            200,
            "1.0",
        )?;
        insert_usage_log(
            &conn,
            "session:msg_1",
            "claude",
            "_session",
            "claude-sonnet-4-6",
            "session_log",
            ts,
            999,
            99,
            0,
            0,
            200,
            "0.5",
        )?;
    }

    let proxy = db.get_request_detail("a-1")?.expect("proxy row");
    assert_eq!(proxy.provider_name.as_deref(), Some("Packy"));
    assert_eq!(proxy.created_at, ts);
    assert_eq!(proxy.input_tokens, 100);

    let session = db
        .get_request_detail("session:msg_1")?
        .expect("session row");
    assert_eq!(session.provider_name.as_deref(), Some("Claude (Session)"));
    assert_eq!(session.created_at, ts);

    assert!(db.get_request_detail("missing")?.is_none());

    Ok(())
}

#[test]
fn test_provider_and_model_filters_cover_detail_and_rollup() -> Result<(), AppError> {
    let db = Database::memory()?;
    let detail_ts = local_ts(2026, 6, 10, 12, 0, 0);

    {
        let conn = lock_conn!(db.conn);
        conn.execute(
            "INSERT INTO providers (id, app_type, name, settings_config) VALUES
                 ('prov-a', 'claude', 'Packy', '{}'),
                 ('prov-b', 'claude', 'DeepSeek', '{}')",
            [],
        )?;

        insert_usage_log(
            &conn,
            "a-1",
            "claude",
            "prov-a",
            "claude-sonnet-4-6",
            "proxy",
            detail_ts,
            100,
            10,
            0,
            0,
            200,
            "1.0",
        )?;
        insert_usage_log(
            &conn,
            "b-1",
            "claude",
            "prov-b",
            "deepseek-v3",
            "proxy",
            detail_ts,
            200,
            20,
            0,
            0,
            200,
            "2.0",
        )?;
        // 会话占位行：providers 表无此 id，展示名走 CASE 映射。
        insert_usage_log(
            &conn,
            "s-1",
            "claude",
            "_session",
            "claude-sonnet-4-6",
            "session_log",
            detail_ts,
            999,
            99,
            0,
            0,
            200,
            "0.5",
        )?;
        // 计价模型与请求模型不同的行：模型筛选必须按有效计价模型命中。
        insert_usage_log(
            &conn,
            "a-2",
            "claude",
            "prov-a",
            "alias-model",
            "proxy",
            detail_ts,
            50,
            5,
            0,
            0,
            200,
            "0.3",
        )?;
        conn.execute(
            "UPDATE proxy_request_logs SET pricing_model = 'real-model' WHERE request_id = 'a-2'",
            [],
        )?;

        // rollup 历史日行：无范围过滤时全部计入。
        conn.execute(
            "INSERT INTO usage_daily_rollups (
                    date, app_type, provider_id, model,
                    request_count, success_count, input_tokens, output_tokens,
                    cache_read_tokens, cache_creation_tokens, total_cost_usd, avg_latency_ms
                ) VALUES
                ('2026-06-08', 'claude', 'prov-a', 'claude-sonnet-4-6', 5, 5, 500, 50, 0, 0, '5.0', 100),
                ('2026-06-08', 'claude', 'prov-b', 'deepseek-v3', 7, 7, 700, 70, 0, 0, '7.0', 100)",
            [],
        )?;
    }

    // ① 汇总按 Provider 展示名过滤：明细 + rollup 都命中。
    let packy = db.get_usage_summary(None, None, None, Some("Packy"), None)?;
    assert_eq!(packy.total_requests, 7, "a-1 + a-2 + rollup 5");

    // ② 汇总按模型过滤（有效计价模型口径）。
    let deepseek = db.get_usage_summary(None, None, None, None, Some("deepseek-v3"))?;
    assert_eq!(deepseek.total_requests, 8, "b-1 + rollup 7");

    // ③ pricing_model 优先于 model：alias-model 查不到，real-model 查得到。
    let by_alias = db.get_usage_summary(None, None, None, None, Some("alias-model"))?;
    assert_eq!(by_alias.total_requests, 0);
    let by_real = db.get_usage_summary(None, None, None, None, Some("real-model"))?;
    assert_eq!(by_real.total_requests, 1);

    // ④ 会话占位行可按可读名选中。
    let session = db.get_usage_summary(None, None, None, Some("Claude (Session)"), None)?;
    assert_eq!(session.total_requests, 1);

    // ⑤ Provider 统计 + 模型过滤：只剩 DeepSeek 一行。
    let provider_stats = db.get_provider_stats(None, None, None, None, Some("deepseek-v3"))?;
    assert_eq!(provider_stats.len(), 1);
    assert_eq!(provider_stats[0].provider_name, "DeepSeek");
    assert_eq!(provider_stats[0].request_count, 8);

    // ⑥ 模型统计 + Provider 过滤：只剩 Packy 名下的模型。
    let model_stats = db.get_model_stats(None, None, None, Some("Packy"), None)?;
    let models: Vec<&str> = model_stats.iter().map(|m| m.model.as_str()).collect();
    assert!(models.contains(&"claude-sonnet-4-6"));
    assert!(models.contains(&"real-model"));
    assert!(!models.contains(&"deepseek-v3"));

    // ⑦ 分应用汇总（Hero 卡片数据源）同样受过滤影响。
    let by_app = db.get_usage_summary_by_app(None, None, Some("Packy"), None)?;
    assert_eq!(by_app.len(), 1);
    assert_eq!(by_app[0].app_type, "claude");
    assert_eq!(by_app[0].summary.total_requests, 7);

    // ⑧ 趋势（>24h 走天分桶 + rollup 分支）。
    let t_start = local_ts(2026, 6, 8, 0, 0, 0);
    let t_end = local_ts(2026, 6, 10, 23, 59, 0);
    let trends = db.get_daily_trends(Some(t_start), Some(t_end), None, Some("Packy"), None)?;
    let total_req: u64 = trends.iter().map(|d| d.request_count).sum();
    assert_eq!(total_req, 7, "明细 2 + rollup 5");

    // ⑨ 趋势 ≤24h 走小时分桶分支（?1/?2/?3 编号参数与追加过滤混用的路径），
    //    同时验证 Provider + 模型组合过滤。
    let h_start = local_ts(2026, 6, 10, 0, 0, 0);
    let h_end = local_ts(2026, 6, 10, 20, 0, 0);
    let hourly = db.get_daily_trends(
        Some(h_start),
        Some(h_end),
        None,
        Some("Packy"),
        Some("claude-sonnet-4-6"),
    )?;
    let hourly_req: u64 = hourly.iter().map(|d| d.request_count).sum();
    assert_eq!(hourly_req, 1, "仅 a-1 命中（a-2 计价模型不同）");

    // ⑩ 请求日志列表与下拉同口径：精确名 + 有效计价模型。
    let logs = db.get_request_logs(
        &LogFilters {
            provider_name: Some("Packy".to_string()),
            model: Some("real-model".to_string()),
            ..Default::default()
        },
        0,
        10,
    )?;
    assert_eq!(logs.total, 1);
    assert_eq!(logs.data[0].request_id, "a-2");

    Ok(())
}

#[test]
fn test_get_usage_summary_includes_end_day_rollup_for_minute_precision_end_time(
) -> Result<(), AppError> {
    let db = Database::memory()?;
    let start = local_ts(2024, 1, 1, 0, 0, 0);
    let end = local_ts(2024, 1, 2, 23, 59, 0);

    {
        let conn = lock_conn!(db.conn);
        conn.execute(
            "INSERT INTO usage_daily_rollups (
                    date, app_type, provider_id, model,
                    request_count, success_count, input_tokens, output_tokens,
                    cache_read_tokens, cache_creation_tokens, total_cost_usd, avg_latency_ms
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            params![
                "2024-01-01",
                "claude",
                "p1",
                "claude-3",
                10,
                10,
                1000,
                500,
                0,
                0,
                "1.00",
                100
            ],
        )?;
        conn.execute(
            "INSERT INTO usage_daily_rollups (
                    date, app_type, provider_id, model,
                    request_count, success_count, input_tokens, output_tokens,
                    cache_read_tokens, cache_creation_tokens, total_cost_usd, avg_latency_ms
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            params![
                "2024-01-02",
                "claude",
                "p1",
                "claude-3",
                20,
                19,
                2000,
                1000,
                0,
                0,
                "2.00",
                120
            ],
        )?;
    }

    let summary = db.get_usage_summary(Some(start), Some(end), Some("claude"), None, None)?;
    assert_eq!(summary.total_requests, 30);
    assert_eq!(summary.total_input_tokens, 3000);
    assert_eq!(summary.total_output_tokens, 1500);

    Ok(())
}

#[test]
fn test_effective_usage_dedup_prefers_proxy_for_session_sources() -> Result<(), AppError> {
    let db = Database::memory()?;

    {
        let conn = lock_conn!(db.conn);
        insert_usage_log(
            &conn,
            "codex-proxy",
            "codex",
            "openai",
            "GPT-5.4",
            "proxy",
            10_000,
            100,
            20,
            10,
            7,
            200,
            "0.10",
        )?;
        insert_usage_log(
            &conn,
            "codex-session-dup",
            "codex",
            "_codex_session",
            "gpt-5.4",
            "codex_session",
            10_060,
            100,
            20,
            10,
            0,
            200,
            "0.10",
        )?;
        insert_usage_log(
            &conn,
            "claude-proxy",
            "claude",
            "openai-compatible",
            "claude-sonnet-4-5",
            "proxy",
            25_000,
            300,
            60,
            20,
            5,
            200,
            "0.30",
        )?;
        insert_usage_log(
            &conn,
            "claude-session-dup",
            "claude",
            "_session",
            "claude-sonnet-4-5",
            "session_log",
            25_060,
            300,
            60,
            20,
            5,
            200,
            "0.30",
        )?;
        insert_usage_log(
            &conn,
            "gemini-proxy",
            "gemini",
            "google",
            "gemini-2.5-pro",
            "proxy",
            20_000,
            200,
            40,
            30,
            0,
            200,
            "0.20",
        )?;
        insert_usage_log(
            &conn,
            "gemini-session-dup",
            "gemini",
            "_gemini_session",
            "gemini-2.5-pro",
            "gemini_session",
            20_060,
            200,
            40,
            30,
            0,
            200,
            "0.20",
        )?;
        insert_usage_log(
            &conn,
            "codex-session-only",
            "codex",
            "_codex_session",
            "gpt-5.4",
            "codex_session",
            30_000,
            50,
            5,
            0,
            0,
            200,
            "0.02",
        )?;
    }

    let summary = db.get_usage_summary(None, None, None, None, None)?;
    assert_eq!(summary.total_requests, 4);
    // codex-proxy contributes 100-10=90; gemini-proxy contributes 200-30=170
    // (both cache-inclusive providers). claude-proxy=300, codex-session-only=50.
    // 90 + 170 + 300 + 50 = 610.
    assert_eq!(summary.total_input_tokens, 610);
    assert_eq!(summary.total_output_tokens, 125);
    assert_eq!(summary.total_cache_read_tokens, 60);
    assert_eq!(summary.total_cache_creation_tokens, 12);
    // real_total = fresh_input(610) + output(125) + cache_create(12) + cache_read(60) = 807
    assert_eq!(summary.real_total_tokens, 807);
    // hit_rate = 60 / (610 + 12 + 60) = 60 / 682
    let expected_hit_rate = 60.0_f64 / 682.0_f64;
    assert!((summary.cache_hit_rate - expected_hit_rate).abs() < 1e-9);

    let trends = db.get_daily_trends(Some(0), Some(40_000), None, None, None)?;
    assert_eq!(trends.iter().map(|stat| stat.request_count).sum::<u64>(), 4);

    let provider_stats = db.get_provider_stats(None, None, None, None, None)?;
    assert_eq!(
        provider_stats
            .iter()
            .map(|stat| stat.request_count)
            .sum::<u64>(),
        4
    );
    assert!(provider_stats
        .iter()
        .any(|stat| stat.provider_id == "_codex_session" && stat.request_count == 1));
    // 各供应商命中率的分子分母加起来等于指标卡：60 / 682
    assert_eq!(
        provider_stats
            .iter()
            .map(|stat| stat.cache_read_tokens)
            .sum::<u64>(),
        60
    );
    assert_eq!(
        provider_stats
            .iter()
            .map(|stat| stat.cacheable_input_tokens)
            .sum::<u64>(),
        682
    );
    assert!(!provider_stats
        .iter()
        .any(|stat| stat.provider_id == "_gemini_session"));
    assert!(!provider_stats
        .iter()
        .any(|stat| stat.provider_id == "_session"));

    let model_stats = db.get_model_stats(None, None, None, None, None)?;
    assert_eq!(
        model_stats
            .iter()
            .map(|stat| stat.request_count)
            .sum::<u64>(),
        4
    );
    // 各模型命中率的分子分母加起来等于指标卡：60 / 682
    assert_eq!(
        model_stats
            .iter()
            .map(|stat| stat.cache_read_tokens)
            .sum::<u64>(),
        60
    );
    assert_eq!(
        model_stats
            .iter()
            .map(|stat| stat.cacheable_input_tokens)
            .sum::<u64>(),
        682
    );

    let logs = db.get_request_logs(&LogFilters::default(), 0, 10)?;
    let request_ids: Vec<&str> = logs
        .data
        .iter()
        .map(|log| log.request_id.as_str())
        .collect();
    assert_eq!(logs.total, 4);
    assert!(request_ids.contains(&"codex-proxy"));
    assert!(request_ids.contains(&"claude-proxy"));
    assert!(request_ids.contains(&"gemini-proxy"));
    assert!(request_ids.contains(&"codex-session-only"));
    assert!(!request_ids.contains(&"codex-session-dup"));
    assert!(!request_ids.contains(&"claude-session-dup"));
    assert!(!request_ids.contains(&"gemini-session-dup"));

    Ok(())
}

#[test]
fn test_effective_usage_dedup_keeps_non_matching_session_rows() -> Result<(), AppError> {
    let db = Database::memory()?;

    {
        let conn = lock_conn!(db.conn);
        insert_usage_log(
            &conn,
            "proxy-base",
            "codex",
            "openai",
            "gpt-5.4",
            "proxy",
            10_000,
            100,
            20,
            10,
            0,
            200,
            "0.10",
        )?;
        insert_usage_log(
            &conn,
            "session-outside-window",
            "codex",
            "_codex_session",
            "gpt-5.4",
            "codex_session",
            10_601,
            100,
            20,
            10,
            0,
            200,
            "0.10",
        )?;
        insert_usage_log(
            &conn,
            "session-token-mismatch",
            "codex",
            "_codex_session",
            "gpt-5.4",
            "codex_session",
            10_060,
            101,
            20,
            10,
            0,
            200,
            "0.10",
        )?;
        insert_usage_log(
            &conn,
            "session-app-mismatch",
            "gemini",
            "_gemini_session",
            "gpt-5.4",
            "gemini_session",
            10_060,
            100,
            20,
            10,
            0,
            200,
            "0.10",
        )?;
        insert_usage_log(
            &conn,
            "session-model-mismatch",
            "codex",
            "_codex_session",
            "different-model",
            "codex_session",
            10_060,
            100,
            20,
            10,
            0,
            200,
            "0.10",
        )?;
        insert_usage_log(
            &conn,
            "proxy-error",
            "codex",
            "openai",
            "gpt-5.4",
            "proxy",
            20_000,
            300,
            60,
            0,
            0,
            500,
            "0.00",
        )?;
        insert_usage_log(
            &conn,
            "session-matches-error-proxy",
            "codex",
            "_codex_session",
            "gpt-5.4",
            "codex_session",
            20_060,
            300,
            60,
            0,
            0,
            200,
            "0.30",
        )?;
        insert_usage_log(
            &conn,
            "claude-proxy-cache-creation",
            "claude",
            "anthropic",
            "claude-sonnet-4-5",
            "proxy",
            30_000,
            100,
            20,
            10,
            5,
            200,
            "0.10",
        )?;
        insert_usage_log(
            &conn,
            "claude-session-cache-creation-mismatch",
            "claude",
            "_session",
            "claude-sonnet-4-5",
            "session_log",
            30_060,
            100,
            20,
            10,
            0,
            200,
            "0.10",
        )?;
    }

    let summary = db.get_usage_summary(None, None, None, None, None)?;
    assert_eq!(summary.total_requests, 9);

    let logs = db.get_request_logs(&LogFilters::default(), 0, 10)?;
    let request_ids: Vec<&str> = logs
        .data
        .iter()
        .map(|log| log.request_id.as_str())
        .collect();
    assert_eq!(logs.total, 9);
    assert!(request_ids.contains(&"session-outside-window"));
    assert!(request_ids.contains(&"session-token-mismatch"));
    assert!(request_ids.contains(&"session-app-mismatch"));
    assert!(request_ids.contains(&"session-model-mismatch"));
    assert!(request_ids.contains(&"session-matches-error-proxy"));
    assert!(request_ids.contains(&"claude-session-cache-creation-mismatch"));

    Ok(())
}

#[test]
fn test_effective_usage_dedup_matches_legacy_codex_proxy_without_cache_writes(
) -> Result<(), AppError> {
    // v3.17.0 之前的代理不记录 Codex 缓存写入；新语义代理行仍要求写入量一致。
    let db = Database::memory()?;
    {
        let conn = lock_conn!(db.conn);
        for (request_id, provider_id, data_source, created_at, cache_creation) in [
            ("legacy-proxy", "openai", "proxy", 10_000, 0),
            (
                "session-vs-legacy",
                "_codex_session",
                "codex_session",
                10_060,
                600,
            ),
            ("total-proxy", "openai", "proxy", 20_000, 0),
            (
                "session-vs-total",
                "_codex_session",
                "codex_session",
                20_060,
                600,
            ),
        ] {
            insert_usage_log(
                &conn,
                request_id,
                "codex",
                provider_id,
                "gpt-5.6",
                data_source,
                created_at,
                1000,
                50,
                300,
                cache_creation,
                200,
                "0.01",
            )?;
        }
        conn.execute(
            "UPDATE proxy_request_logs SET input_token_semantics = ?1
                 WHERE request_id != 'legacy-proxy'",
            [INPUT_TOKEN_SEMANTICS_TOTAL],
        )?;
    }

    // 逐行 EXISTS 和按窗口的 rowid NOT IN 两种写法都要去掉 session-vs-legacy。
    let conn = lock_conn!(db.conn);
    let ranged = effective_usage_log_filter_for_range(&conn, "l", None, None)?;
    assert!(ranged.contains("NOT IN"));
    for filter in [effective_usage_log_filter("l"), ranged] {
        let mut stmt = conn.prepare(&format!(
            "SELECT request_id FROM proxy_request_logs l WHERE {filter} ORDER BY request_id"
        ))?;
        let request_ids = stmt
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        assert_eq!(
            request_ids,
            ["legacy-proxy", "session-vs-total", "total-proxy"]
        );
    }

    Ok(())
}

#[test]
fn test_get_model_stats() -> Result<(), AppError> {
    let db = Database::memory()?;

    // 插入测试数据
    {
        let conn = lock_conn!(db.conn);
        conn.execute(
            "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model,
                    input_tokens, output_tokens, total_cost_usd,
                    latency_ms, status_code, created_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            params![
                "req1",
                "p1",
                "claude",
                "claude-3-sonnet",
                100,
                50,
                "0.01",
                100,
                200,
                1000
            ],
        )?;
    }

    let stats = db.get_model_stats(None, None, None, None, None)?;
    assert_eq!(stats.len(), 1);
    assert_eq!(stats[0].model, "claude-3-sonnet");
    assert_eq!(stats[0].request_count, 1);

    Ok(())
}

#[test]
fn test_get_model_stats_success_rate_and_speed_per_model() -> Result<(), AppError> {
    let db = Database::memory()?;

    {
        let conn = lock_conn!(db.conn);
        let insert = |id: &str,
                      model: &str,
                      output: i64,
                      latency: i64,
                      first: Option<i64>,
                      status: i64,
                      source: &str| {
            conn.execute(
                "INSERT INTO proxy_request_logs (
                        request_id, provider_id, app_type, model,
                        input_tokens, output_tokens, total_cost_usd,
                        latency_ms, first_token_ms, status_code, created_at, data_source
                    ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                params![
                    id, "p1", "claude", model, 10, output, "0", latency, first, status, 1000,
                    source
                ],
            )
        };
        // fast：精确速度 1000 token / 10000 ms，另有一条失败请求
        insert("fast-ok", "fast", 1_000, 11_000, Some(1_000), 200, "proxy")?;
        insert("fast-err", "fast", 0, 500, None, 500, "proxy")?;
        // slow：只有会话日志，估算速度 400 token / 8000 ms
        insert("slow-a", "slow", 400, 8_000, None, 200, "session_log")?;
    }

    let stats = db.get_model_stats(None, None, None, None, None)?;
    let fast = stats.iter().find(|s| s.model == "fast").expect("fast");
    assert_eq!(fast.request_count, 2);
    assert!((fast.success_rate - 50.0).abs() < f32::EPSILON);
    assert_eq!(fast.speed_output_tokens, 1_000);
    assert_eq!(fast.speed_generation_ms, 10_000);
    assert_eq!(fast.est_speed_output_tokens, 0);

    let slow = stats.iter().find(|s| s.model == "slow").expect("slow");
    assert!((slow.success_rate - 100.0).abs() < f32::EPSILON);
    assert_eq!(slow.speed_output_tokens, 0);
    assert_eq!(slow.est_speed_output_tokens, 400);
    assert_eq!(slow.est_speed_duration_ms, 8_000);

    Ok(())
}

#[test]
fn test_stats_first_token_median_only_counts_successful_proxy_streams() -> Result<(), AppError> {
    let db = Database::memory()?;

    {
        let conn = lock_conn!(db.conn);
        let insert = |id: &str,
                      provider: &str,
                      model: &str,
                      first: Option<i64>,
                      status: i64,
                      source: &str| {
            conn.execute(
                "INSERT INTO proxy_request_logs (
                        request_id, provider_id, app_type, model,
                        input_tokens, output_tokens, total_cost_usd,
                        latency_ms, first_token_ms, status_code, created_at, data_source
                    ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                params![
                    id,
                    provider,
                    "codex",
                    model,
                    10,
                    // 会话日志的用量和路由的一样会被当成重复记录滤掉，错开
                    if source == "proxy" { 500 } else { 777 },
                    "0",
                    20_000,
                    first,
                    status,
                    1000,
                    source
                ],
            )
        };
        // p1 / m1：奇数个样本取中间那个，长尾不拉高
        insert("a", "p1", "m1", Some(1_000), 200, "proxy")?;
        insert("b", "p1", "m1", Some(2_000), 200, "proxy")?;
        insert("c", "p1", "m1", Some(15_000), 200, "proxy")?;
        // 不计：失败、非流式、会话日志
        insert("err", "p1", "m1", Some(100), 500, "proxy")?;
        insert("non-stream", "p1", "m1", None, 200, "proxy")?;
        insert("session", "p1", "m1", Some(50), 200, "codex_session")?;
        // 会话日志推出来的首字：单独取中位数
        insert("est-a", "p1", "m1", Some(3_000), 200, "codex_session")?;
        // p2 / m2：偶数个样本取中间两个的平均
        insert("d", "p2", "m2", Some(1_000), 200, "proxy")?;
        insert("e", "p2", "m2", Some(3_000), 200, "proxy")?;
        // p3 / m3：没有可计的请求
        insert("f", "p3", "m3", None, 200, "proxy")?;
    }

    let providers = db.get_provider_stats(None, None, None, None, None)?;
    let median_of = |id: &str| {
        providers
            .iter()
            .find(|s| s.provider_id == id)
            .expect(id)
            .first_token_median_ms
    };
    assert_eq!(median_of("p1"), Some(2_000));
    assert_eq!(median_of("p2"), Some(2_000));
    assert_eq!(median_of("p3"), None);
    let p1 = providers.iter().find(|s| s.provider_id == "p1").unwrap();
    // 会话日志两条：50 和 3000
    assert_eq!(p1.est_first_token_median_ms, Some(1_525));
    // 会话日志推出来的首字不算精确速度：进速度的只有路由的 a、b、c、err 四条
    assert_eq!(p1.speed_output_tokens, 2_000);

    let models = db.get_model_stats(None, None, None, None, None)?;
    let median_of = |m: &str| {
        models
            .iter()
            .find(|s| s.model == m)
            .expect(m)
            .first_token_median_ms
    };
    assert_eq!(median_of("m1"), Some(2_000));
    assert_eq!(median_of("m2"), Some(2_000));
    assert_eq!(median_of("m3"), None);

    // 带筛选条件时 WHERE 要接上，不能拼出两个 WHERE
    let filtered = db.get_provider_stats(None, None, Some("codex"), None, Some("m2"))?;
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].first_token_median_ms, Some(2_000));

    Ok(())
}

#[test]
fn test_get_provider_stats_with_time_filter() -> Result<(), AppError> {
    let db = Database::memory()?;

    {
        let conn = lock_conn!(db.conn);
        conn.execute(
            "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model,
                    input_tokens, output_tokens, total_cost_usd,
                    latency_ms, status_code, created_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            params!["old", "p1", "claude", "claude-3", 100, 50, "0.01", 100, 200, 1000],
        )?;
        conn.execute(
            "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model,
                    input_tokens, output_tokens, total_cost_usd,
                    latency_ms, status_code, created_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            params!["new", "p1", "claude", "claude-3", 200, 75, "0.02", 120, 200, 2000],
        )?;
    }

    let stats = db.get_provider_stats(Some(1500), Some(2500), Some("claude"), None, None)?;
    assert_eq!(stats.len(), 1);
    assert_eq!(stats[0].provider_id, "p1");
    assert_eq!(stats[0].app_type, "claude");
    assert_eq!(stats[0].request_count, 1);
    assert_eq!(stats[0].total_tokens, 275);

    Ok(())
}

#[test]
fn test_get_provider_stats_preserves_app_type_identity_across_apps() -> Result<(), AppError> {
    let db = Database::memory()?;

    {
        let conn = lock_conn!(db.conn);
        conn.execute(
            "INSERT INTO providers (id, app_type, name, settings_config) VALUES (?, ?, ?, '{}')",
            params!["shared-p1", "claude", "Shared Provider"],
        )?;
        conn.execute(
            "INSERT INTO providers (id, app_type, name, settings_config) VALUES (?, ?, ?, '{}')",
            params!["shared-p1", "claude-desktop", "Shared Provider"],
        )?;

        // Detail log under claude
        conn.execute(
            "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model,
                    input_tokens, output_tokens, total_cost_usd,
                    latency_ms, status_code, created_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            params![
                "req-1",
                "shared-p1",
                "claude",
                "m1",
                100,
                50,
                "0.01",
                100,
                200,
                1000
            ],
        )?;

        // Rollup under claude-desktop
        conn.execute(
            "INSERT INTO usage_daily_rollups (
                    date, app_type, provider_id, model,
                    request_count, input_tokens, output_tokens,
                    cache_creation_tokens, cache_read_tokens, total_cost_usd,
                    success_count, avg_latency_ms
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            params![
                "2026-10-08",
                "claude-desktop",
                "shared-p1",
                "m1",
                10,
                1000,
                500,
                0,
                0,
                "0.10",
                10,
                120
            ],
        )?;
    }

    // Folded query for "claude" matches both claude and claude-desktop
    let stats = db.get_provider_stats(None, None, Some("claude"), None, None)?;
    assert_eq!(stats.len(), 2);

    let claude_stat = stats
        .iter()
        .find(|s| s.app_type == "claude")
        .expect("claude stat");
    assert_eq!(claude_stat.provider_id, "shared-p1");
    assert_eq!(claude_stat.provider_name, "Shared Provider");
    assert_eq!(claude_stat.request_count, 1);

    let desktop_stat = stats
        .iter()
        .find(|s| s.app_type == "claude-desktop")
        .expect("claude-desktop stat");
    assert_eq!(desktop_stat.provider_id, "shared-p1");
    assert_eq!(desktop_stat.provider_name, "Shared Provider");
    assert_eq!(desktop_stat.request_count, 10);

    Ok(())
}

#[test]
fn test_get_provider_stats_speed_sums_only_eligible_requests() -> Result<(), AppError> {
    let db = Database::memory()?;

    {
        let conn = lock_conn!(db.conn);
        let insert = |id: &str, output: i64, latency: i64, first: Option<i64>| {
            conn.execute(
                "INSERT INTO proxy_request_logs (
                        request_id, provider_id, app_type, model,
                        input_tokens, output_tokens, total_cost_usd,
                        latency_ms, first_token_ms, status_code, created_at
                    ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                params![id, "p1", "claude", "m", 10, output, "0", latency, first, 200, 1000],
            )
        };
        // 计入：1000 token / (11000 - 1000) ms
        insert("ok-a", 1000, 11_000, Some(1_000))?;
        // 计入：300 token / (4000 - 1000) ms
        insert("ok-b", 300, 4_000, Some(1_000))?;
        // 不计：输出不到 100
        insert("short", 50, 2_000, Some(100))?;
        // 不计：没有首字（会话日志 / 非流式）
        insert("no-ttft", 5_000, 9_000, None)?;
        // 不计：耗时不大于首字
        insert("zero-gen", 500, 1_000, Some(1_000))?;
        // 不计：生成窗口不到 100ms，是传输突发
        insert("burst", 500, 1_050, Some(1_000))?;
    }

    let stats = db.get_provider_stats(None, None, None, None, None)?;
    assert_eq!(stats.len(), 1);
    assert_eq!(stats[0].request_count, 6);
    assert_eq!(stats[0].speed_output_tokens, 1_300);
    assert_eq!(stats[0].speed_generation_ms, 13_000);
    // 路由服务的行没有首字（非流式）也不算估算速度
    assert_eq!(stats[0].est_speed_output_tokens, 0);
    assert_eq!(stats[0].est_speed_duration_ms, 0);

    Ok(())
}

#[test]
fn test_get_provider_stats_estimated_speed_sums_session_rows() -> Result<(), AppError> {
    let db = Database::memory()?;

    {
        let conn = lock_conn!(db.conn);
        let insert = |id: &str, output: i64, latency: i64, source: &str| {
            conn.execute(
                "INSERT INTO proxy_request_logs (
                        request_id, provider_id, app_type, model,
                        input_tokens, output_tokens, total_cost_usd,
                        latency_ms, status_code, created_at, data_source
                    ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                params![id, "_session", "claude", "m", 10, output, "0", latency, 200, 1000, source],
            )
        };
        // 计入：2000 token / 20000 ms
        insert("ok-a", 2_000, 20_000, "session_log")?;
        // 计入：500 token / 5000 ms
        insert("ok-b", 200, 5_000, "session_log")?;
        // 不计：输出不到 200
        insert("short", 199, 5_000, "session_log")?;
        // 不计：没估出耗时
        insert("no-timing", 3_000, 0, "session_log")?;
        // 不计：耗时不到 1 秒
        insert("too-fast", 800, 900, "session_log")?;
    }

    let stats = db.get_provider_stats(None, None, None, None, None)?;
    assert_eq!(stats.len(), 1);
    assert_eq!(stats[0].est_speed_output_tokens, 2_200);
    assert_eq!(stats[0].est_speed_duration_ms, 25_000);
    // 估算的不混进精确口径
    assert_eq!(stats[0].speed_output_tokens, 0);
    assert_eq!(stats[0].speed_generation_ms, 0);

    Ok(())
}

#[test]
fn test_agy_session_includes_first_token_in_speed_duration() -> Result<(), AppError> {
    let db = Database::memory()?;

    {
        let conn = lock_conn!(db.conn);
        let insert = |id: &str,
                      provider: &str,
                      app: &str,
                      model: &str,
                      output: i64,
                      latency: i64,
                      first: Option<i64>,
                      source: &str| {
            conn.execute(
                "INSERT INTO proxy_request_logs (
                        request_id, provider_id, app_type, model,
                        input_tokens, output_tokens, total_cost_usd,
                        latency_ms, first_token_ms, status_code, created_at, data_source
                    ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                params![
                    id, provider, app, model, 10, output, "0", latency, first, 200, 1000, source
                ],
            )
        };
        // Antigravity 原生 output_tokens 包含隐藏思考 token，而 first_token_ms 包含整段思考时间；
        // 速度必须按完整 latency_ms（含首字时间）计算，否则思考后仅极短流式窗口会让速度严重虚高。
        insert(
            "agy-thinking",
            "_agy_session",
            "agy",
            "gemini-4-neon",
            12_000,
            100_000,
            Some(99_850),
            "agy_session",
        )?;
        insert(
            "agy-short",
            "_agy_session",
            "agy",
            "gemini-4-neon",
            150,
            38_000,
            Some(36_500),
            "agy_session",
        )?;
        insert(
            "agy-untimed",
            "_agy_session",
            "agy",
            "gemini-4-neon",
            1_000,
            0,
            None,
            "agy_session",
        )?;
        insert(
            "codex-est",
            "_codex_session",
            "codex",
            "gpt-5.4",
            500,
            10_000,
            Some(3_300),
            "codex_session",
        )?;
    }

    let providers = db.get_provider_stats(None, None, None, None, None)?;
    let agy_provider = providers
        .iter()
        .find(|s| s.provider_id == "_agy_session")
        .expect("_agy_session");
    assert_eq!(agy_provider.speed_output_tokens, 0);
    assert_eq!(agy_provider.speed_generation_ms, 0);
    assert_eq!(agy_provider.first_token_median_ms, None);
    // 只有 agy-thinking (>=200 output tokens 且 >=1000ms) 计入估算速度，分母为完整 latency_ms (100_000ms -> 120 tok/s，而非 80_000 tok/s)
    assert_eq!(agy_provider.est_speed_output_tokens, 12_000);
    assert_eq!(agy_provider.est_speed_duration_ms, 100_000);
    assert_eq!(agy_provider.est_first_token_median_ms, Some(68_175));

    let models = db.get_model_stats(None, None, None, None, None)?;
    let neon = models
        .iter()
        .find(|s| s.model == "gemini-4-neon")
        .expect("gemini-4-neon");
    assert_eq!(neon.speed_output_tokens, 0);
    assert_eq!(neon.speed_generation_ms, 0);
    assert_eq!(neon.first_token_median_ms, None);
    assert_eq!(neon.est_speed_output_tokens, 12_000);
    assert_eq!(neon.est_speed_duration_ms, 100_000);
    assert_eq!(neon.est_first_token_median_ms, Some(68_175));

    Ok(())
}

#[test]
fn test_get_provider_stats_labels_opencode_session_provider() -> Result<(), AppError> {
    let db = Database::memory()?;

    {
        let conn = lock_conn!(db.conn);
        insert_usage_log(
            &conn,
            "opencode-session",
            "opencode",
            "_opencode_session",
            "opencode-model",
            "opencode_session",
            1000,
            100,
            50,
            0,
            0,
            200,
            "0.01",
        )?;
    }

    let stats = db.get_provider_stats(None, None, Some("opencode"), None, None)?;
    assert_eq!(stats.len(), 1);
    assert_eq!(stats[0].provider_id, "_opencode_session");
    assert_eq!(stats[0].provider_name, "OpenCode (Session)");

    Ok(())
}

#[test]
fn test_get_provider_stats_excludes_partial_rollup_boundary_days() -> Result<(), AppError> {
    let db = Database::memory()?;
    let start = local_ts(2024, 2, 1, 12, 0, 0);
    let end = local_ts(2024, 2, 3, 12, 0, 0);

    {
        let conn = lock_conn!(db.conn);
        conn.execute(
            "INSERT INTO usage_daily_rollups (
                    date, app_type, provider_id, model,
                    request_count, success_count, input_tokens, output_tokens,
                    cache_read_tokens, cache_creation_tokens, total_cost_usd, avg_latency_ms
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            params![
                "2024-02-01",
                "claude",
                "p-rollup",
                "claude-3",
                5,
                5,
                500,
                250,
                0,
                0,
                "0.50",
                100
            ],
        )?;
        conn.execute(
            "INSERT INTO usage_daily_rollups (
                    date, app_type, provider_id, model,
                    request_count, success_count, input_tokens, output_tokens,
                    cache_read_tokens, cache_creation_tokens, total_cost_usd, avg_latency_ms
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            params![
                "2024-02-02",
                "claude",
                "p-rollup",
                "claude-3",
                8,
                7,
                800,
                400,
                0,
                0,
                "0.80",
                120
            ],
        )?;
        conn.execute(
            "INSERT INTO usage_daily_rollups (
                    date, app_type, provider_id, model,
                    request_count, success_count, input_tokens, output_tokens,
                    cache_read_tokens, cache_creation_tokens, total_cost_usd, avg_latency_ms
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            params![
                "2024-02-03",
                "claude",
                "p-rollup",
                "claude-3",
                12,
                11,
                1200,
                600,
                0,
                0,
                "1.20",
                140
            ],
        )?;
    }

    let stats = db.get_provider_stats(Some(start), Some(end), Some("claude"), None, None)?;
    assert_eq!(stats.len(), 1);
    assert_eq!(stats[0].provider_id, "p-rollup");
    assert_eq!(stats[0].request_count, 8);
    assert_eq!(stats[0].total_tokens, 1200);
    // 日汇总也计入命中率分母：新增输入 800，无缓存
    assert_eq!(stats[0].cacheable_input_tokens, 800);
    assert_eq!(stats[0].cache_read_tokens, 0);

    Ok(())
}

#[test]
fn test_get_daily_trends_respects_shorter_than_24_hours() -> Result<(), AppError> {
    let db = Database::memory()?;

    {
        let conn = lock_conn!(db.conn);
        conn.execute(
            "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model,
                    input_tokens, output_tokens, total_cost_usd,
                    latency_ms, status_code, created_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            params![
                "req-short",
                "p1",
                "claude",
                "claude-3",
                100,
                50,
                "0.01",
                100,
                200,
                10_800
            ],
        )?;
    }

    let stats = db.get_daily_trends(Some(0), Some(15 * 60 * 60), Some("claude"), None, None)?;
    assert_eq!(stats.len(), 15);
    assert_eq!(stats[3].request_count, 1);

    Ok(())
}

#[test]
fn test_get_daily_trends_groups_ranges_longer_than_24_hours_by_local_day() -> Result<(), AppError> {
    let db = Database::memory()?;
    let start = local_ts(2024, 3, 1, 12, 0, 0);
    let end = local_ts(2024, 3, 3, 12, 0, 0);

    {
        let conn = lock_conn!(db.conn);
        conn.execute(
            "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model,
                    input_tokens, output_tokens, total_cost_usd,
                    latency_ms, status_code, created_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            params![
                "day-1-detail",
                "p1",
                "claude",
                "claude-3",
                100,
                50,
                "0.01",
                100,
                200,
                local_ts(2024, 3, 1, 13, 0, 0)
            ],
        )?;
        conn.execute(
            "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model,
                    input_tokens, output_tokens, total_cost_usd,
                    latency_ms, status_code, created_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            params![
                "day-3-detail",
                "p1",
                "claude",
                "claude-3",
                200,
                75,
                "0.02",
                110,
                200,
                local_ts(2024, 3, 3, 10, 0, 0)
            ],
        )?;
        conn.execute(
            "INSERT INTO usage_daily_rollups (
                    date, app_type, provider_id, model,
                    request_count, success_count, input_tokens, output_tokens,
                    cache_read_tokens, cache_creation_tokens, total_cost_usd, avg_latency_ms
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            params![
                "2024-03-02",
                "claude",
                "p1",
                "claude-3",
                4,
                4,
                400,
                200,
                0,
                0,
                "0.40",
                120
            ],
        )?;
    }

    let stats = db.get_daily_trends(Some(start), Some(end), Some("claude"), None, None)?;
    assert_eq!(stats.len(), 3);
    assert_eq!(stats[0].request_count, 1);
    assert_eq!(stats[0].total_tokens, 150);
    assert_eq!(stats[1].request_count, 4);
    assert_eq!(stats[1].total_tokens, 600);
    assert_eq!(stats[2].request_count, 1);
    assert_eq!(stats[2].total_tokens, 275);

    Ok(())
}

#[test]
fn test_provider_and_model_stats_tokens_sum_to_summary_real_total() -> Result<(), AppError> {
    let db = Database::memory()?;

    {
        let conn = lock_conn!(db.conn);
        // Claude：input 不含缓存。真实消耗 = 100 + 200 + 1000 + 5000
        insert_usage_log(
            &conn,
            "claude-1",
            "claude",
            "p-claude",
            "claude-x",
            "session_log",
            1000,
            100,
            200,
            5000,
            1000,
            200,
            "0.10",
        )?;
        // Codex：input 已含缓存命中 600。真实消耗 = (1000 - 600) + 50 + 600
        insert_usage_log(
            &conn,
            "codex-1",
            "codex",
            "p-codex",
            "gpt-x",
            "codex_session",
            1000,
            1000,
            50,
            600,
            0,
            200,
            "0.05",
        )?;
        // 日汇总行同样要带上缓存。真实消耗 = (900 - 300) + 40 + 300 + 0
        conn.execute(
            "INSERT INTO usage_daily_rollups (
                    date, app_type, provider_id, model,
                    request_count, success_count, input_tokens, output_tokens,
                    cache_read_tokens, cache_creation_tokens, total_cost_usd, avg_latency_ms
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            params![
                "2020-01-01",
                "codex",
                "p-codex",
                "gpt-x",
                3,
                3,
                900,
                40,
                300,
                0,
                "0.30",
                100
            ],
        )?;
    }

    let summary = db.get_usage_summary(None, None, None, None, None)?;
    assert_eq!(summary.real_total_tokens, 6300 + 1050 + 940);

    let providers = db.get_provider_stats(None, None, None, None, None)?;
    let tokens_of = |id: &str| {
        providers
            .iter()
            .find(|s| s.provider_id == id)
            .map(|s| s.total_tokens)
    };
    assert_eq!(tokens_of("p-claude"), Some(6300));
    assert_eq!(tokens_of("p-codex"), Some(1050 + 940));
    let provider_sum: u64 = providers.iter().map(|s| s.total_tokens).sum();
    assert_eq!(provider_sum, summary.real_total_tokens);

    let models = db.get_model_stats(None, None, None, None, None)?;
    let model_sum: u64 = models.iter().map(|s| s.total_tokens).sum();
    assert_eq!(model_sum, summary.real_total_tokens);

    Ok(())
}

#[test]
fn test_get_model_stats_excludes_partial_rollup_boundary_days() -> Result<(), AppError> {
    let db = Database::memory()?;
    let start = local_ts(2024, 4, 1, 12, 0, 0);
    let end = local_ts(2024, 4, 3, 12, 0, 0);

    {
        let conn = lock_conn!(db.conn);
        conn.execute(
            "INSERT INTO usage_daily_rollups (
                    date, app_type, provider_id, model,
                    request_count, success_count, input_tokens, output_tokens,
                    cache_read_tokens, cache_creation_tokens, total_cost_usd, avg_latency_ms
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            params![
                "2024-04-01",
                "claude",
                "p1",
                "claude-3-haiku",
                6,
                6,
                600,
                300,
                0,
                0,
                "0.60",
                100
            ],
        )?;
        conn.execute(
            "INSERT INTO usage_daily_rollups (
                    date, app_type, provider_id, model,
                    request_count, success_count, input_tokens, output_tokens,
                    cache_read_tokens, cache_creation_tokens, total_cost_usd, avg_latency_ms
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            params![
                "2024-04-02",
                "claude",
                "p1",
                "claude-3-haiku",
                9,
                8,
                900,
                450,
                0,
                0,
                "0.90",
                110
            ],
        )?;
        conn.execute(
            "INSERT INTO usage_daily_rollups (
                    date, app_type, provider_id, model,
                    request_count, success_count, input_tokens, output_tokens,
                    cache_read_tokens, cache_creation_tokens, total_cost_usd, avg_latency_ms
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            params![
                "2024-04-03",
                "claude",
                "p1",
                "claude-3-haiku",
                12,
                11,
                1200,
                600,
                0,
                0,
                "1.20",
                130
            ],
        )?;
    }

    let stats = db.get_model_stats(Some(start), Some(end), Some("claude"), None, None)?;
    assert_eq!(stats.len(), 1);
    assert_eq!(stats[0].model, "claude-3-haiku");
    assert_eq!(stats[0].request_count, 9);
    assert_eq!(stats[0].total_tokens, 1350);
    // 日汇总也计入命中率分母：新增输入 900，无缓存
    assert_eq!(stats[0].cacheable_input_tokens, 900);
    assert_eq!(stats[0].cache_read_tokens, 0);

    Ok(())
}

#[test]
fn test_strip_model_date_suffix_is_utf8_safe() {
    assert_eq!(
        strip_model_date_suffix("模型-2026-05-14").as_deref(),
        Some("模型")
    );
    assert_eq!(strip_model_date_suffix("abc🚀12345678"), None);
}

#[test]
fn test_strip_model_date_suffix_handles_six_digit_yymmdd() {
    // 火山方舟 6 位 YYMMDD 后缀应被剥离（doubao 全系都用这种格式）。
    assert_eq!(
        strip_model_date_suffix("doubao-seed-2-1-pro-260628").as_deref(),
        Some("doubao-seed-2-1-pro")
    );
    assert_eq!(
        strip_model_date_suffix("doubao-seed-1-6-250615").as_deref(),
        Some("doubao-seed-1-6")
    );
    // 8 位 YYYYMMDD 仍照旧剥离。
    assert_eq!(
        strip_model_date_suffix("claude-3-5-sonnet-20241022").as_deref(),
        Some("claude-3-5-sonnet")
    );
    // 月/日非法的 6 位尾巴（版本号等）不剥离，避免误伤。
    assert_eq!(strip_model_date_suffix("foo-bar-123456"), None); // 月=34
    assert_eq!(strip_model_date_suffix("widget-209900"), None); // 月=99
    assert_eq!(strip_model_date_suffix("gizmo-251200"), None); // 日=00
}

#[test]
fn test_pricing_resolves_volcengine_dated_model_to_bare_seed_row() -> Result<(), AppError> {
    // 回归：火山真实用量带 6 位日期后缀（doubao-seed-2-1-pro-260628），
    // 必须能归一化命中定价表里的裸名 seed 行（doubao-seed-2-1-pro），否则成本显示 $0。
    let db = Database::memory()?;
    let conn = lock_conn!(db.conn);

    conn.execute(
        "INSERT OR REPLACE INTO model_pricing (
                model_id, display_name, input_cost_per_million, output_cost_per_million,
                cache_read_cost_per_million, cache_creation_cost_per_million
            ) VALUES ('doubao-seed-2-1-pro', 'Doubao Seed 2.1 Pro', '0.84', '4.2', '0.17', '0')",
        [],
    )?;

    let row = find_model_pricing_row(&conn, "doubao-seed-2-1-pro-260628")?;
    assert!(
        row.is_some(),
        "带日期的火山模型应通过 6 位日期剥离命中裸名定价行"
    );
    let (input, output, ..) = row.unwrap();
    assert_eq!(input, "0.84");
    assert_eq!(output, "4.2");

    Ok(())
}

#[test]
fn test_pricing_lookup_matches_custom_rows_with_uppercase_model_id() -> Result<(), AppError> {
    // 回归 #8413：自定义定价常按界面显示名原样入库（如 GLM-5.3-Flash-EXL3），
    // 而查价候选会被 clean_model_id_for_pricing 小写归一化；若精确匹配大小写
    // 敏感，这类行永远命中不了，成本恒为 0。查价必须大小写不敏感。
    let db = Database::memory()?;
    let conn = lock_conn!(db.conn);

    conn.execute(
        "INSERT OR REPLACE INTO model_pricing (
                model_id, display_name, input_cost_per_million, output_cost_per_million,
                cache_read_cost_per_million, cache_creation_cost_per_million
            ) VALUES ('GLM-5.3-Flash-EXL3', 'GLM-5.3-Flash-EXL3', '1.4', '4.4', '0.26', '0')",
        [],
    )?;

    let row = find_model_pricing_row(&conn, "GLM-5.3-Flash-EXL3")?;
    assert!(row.is_some(), "小写候选应能命中按大写入库的自定义定价行");
    let (input, output, ..) = row.unwrap();
    assert_eq!(input, "1.4");
    assert_eq!(output, "4.4");

    Ok(())
}

#[test]
fn test_prefix_pricing_does_not_match_short_base_model_to_variant() -> Result<(), AppError> {
    let db = Database::memory()?;
    let conn = lock_conn!(db.conn);

    conn.execute("DELETE FROM model_pricing WHERE model_id LIKE 'gpt-5%'", [])?;
    for (model_id, display_name) in [("gpt-5-mini", "GPT-5 Mini"), ("gpt-5-pro", "GPT-5 Pro")] {
        conn.execute(
            "INSERT INTO model_pricing (
                    model_id, display_name, input_cost_per_million, output_cost_per_million,
                    cache_read_cost_per_million, cache_creation_cost_per_million
                ) VALUES (?1, ?2, '1', '2', '0', '0')",
            params![model_id, display_name],
        )?;
    }

    let result = find_model_pricing_row(&conn, "gpt-5")?;
    assert!(
        result.is_none(),
        "缺少 gpt-5 基础定价时，不应前缀误匹配到 gpt-5-mini/gpt-5-pro"
    );

    Ok(())
}

#[test]
fn test_hydrogen_orchestrator_pricing_preserves_exact_overrides() -> Result<(), AppError> {
    let db = Database::memory()?;
    let conn = lock_conn!(db.conn);
    let base = find_model_pricing_row(&conn, "gemini-4-hydrogen")?;
    assert!(base.is_some());
    for model in [
        "gemini-4-hydrogen-orchestrator",
        "google/GEMINI-4-HYDROGEN-ORCHESTRATOR",
        "google.gemini-4-hydrogen-orchestrator:free",
        "gemini-4-hydrogen-orchestrator-high",
    ] {
        assert_eq!(find_model_pricing_row(&conn, model)?, base, "{model}");
    }
    // Orchestrator is a known Hydrogen variant, not a suffix to strip from any model.
    for model in [
        "gemini-4-carbon-orchestrator",
        "gemini-4-hydrogen-orchestrator-preview",
        "orchestrator-carbon",
    ] {
        assert!(find_model_pricing_row(&conn, model)?.is_none(), "{model}");
    }

    conn.execute(
        "INSERT INTO model_pricing VALUES ('gemini-4-hydrogen-orchestrator',
             'Custom Hydrogen Orchestrator', '3', '12', '0.2', '0')",
        [],
    )?;
    assert_eq!(
        find_model_pricing_row(&conn, "gemini-4-hydrogen-orchestrator")?,
        Some(("3".into(), "12".into(), "0.2".into(), "0".into()))
    );
    // A custom per-effort entry must also win over both fallback prices.
    conn.execute(
        "INSERT INTO model_pricing VALUES ('gemini-4-hydrogen-orchestrator-high',
             'Custom Hydrogen Orchestrator High', '4', '16', '0.3', '0')",
        [],
    )?;
    assert_eq!(
        find_model_pricing_row(&conn, "gemini-4-hydrogen-orchestrator-high")?,
        Some(("4".into(), "16".into(), "0.3".into(), "0".into()))
    );
    Ok(())
}

#[test]
fn test_hydrogen_tiered_pricing_preserves_exact_overrides() -> Result<(), AppError> {
    let db = Database::memory()?;
    let conn = lock_conn!(db.conn);
    let base = find_model_pricing_row(&conn, "gemini-4-hydrogen")?;
    assert!(base.is_some());
    for model in [
        "gemini-4-hydrogen-tiered",
        "gemini-4-hydrogen-tiered-high",
        "gemini-4-hydrogen-tiered-medium",
        "gemini-4-hydrogen-tiered-low",
        "gemini-4-hydrogen-tiered-xhigh",
        "models/gemini-4-hydrogen-tiered-high",
        "google/GEMINI-4-HYDROGEN-TIERED-HIGH",
        "google.gemini-4-hydrogen-tiered-high:free",
        "claude-gemini-4-hydrogen-tiered-high",
        "gemini-4-hydrogen-tiered-high-2026-11-18",
    ] {
        assert_eq!(find_model_pricing_row(&conn, model)?, base, "{model}");
    }
    for model in [
        "gemini-4-hydrogen-tiered-preview",
        "gemini-4-carbon-tiered-high",
        "gemini-4-argon-tiered-high",
        "custom-tiered-high",
    ] {
        assert!(find_model_pricing_row(&conn, model)?.is_none(), "{model}");
    }

    conn.execute(
        "INSERT INTO model_pricing VALUES ('gemini-4-hydrogen-tiered-high',
             'Custom Hydrogen Tiered High', '3', '12', '0.2', '0')",
        [],
    )?;
    for model in [
        "gemini-4-hydrogen-tiered-high",
        "google.gemini-4-hydrogen-tiered-high",
        "claude-gemini-4-hydrogen-tiered-high",
    ] {
        assert_eq!(
            find_model_pricing_row(&conn, model)?,
            Some(("3".into(), "12".into(), "0.2".into(), "0".into())),
            "{model}"
        );
    }
    conn.execute(
        "INSERT INTO model_pricing VALUES ('gemini-4-hydrogen-tiered',
             'Custom Hydrogen Tiered', '2', '10', '0.1', '0')",
        [],
    )?;
    assert_eq!(
        find_model_pricing_row(&conn, "gemini-4-hydrogen-tiered-high")?,
        Some(("3".into(), "12".into(), "0.2".into(), "0".into()))
    );
    assert_eq!(
        find_model_pricing_row(&conn, "gemini-4-hydrogen-tiered-medium")?,
        Some(("2".into(), "10".into(), "0.1".into(), "0".into()))
    );
    conn.execute(
        "DELETE FROM model_pricing WHERE model_id IN
             ('gemini-4-hydrogen', 'gemini-4-hydrogen-tiered', 'gemini-4-hydrogen-tiered-high')",
        [],
    )?;
    conn.execute(
        "INSERT INTO model_pricing VALUES ('gemini-4-hydrogen-preview',
             'Custom Hydrogen Preview', '4', '16', '0.3', '0')",
        [],
    )?;
    conn.execute(
        "INSERT INTO model_pricing VALUES ('gemini-4-hydrogen-tiered-low',
             'Custom Hydrogen Tiered Low', '1', '5', '0.1', '0')",
        [],
    )?;
    // A missing exact base must not borrow a preview or another tier's price.
    assert!(find_model_pricing_row(&conn, "gemini-4-hydrogen-tiered-high")?.is_none());
    Ok(())
}

#[test]
fn test_hydrogen_tiered_backfills_without_transcripts() -> Result<(), AppError> {
    let db = Database::memory()?;
    {
        let conn = lock_conn!(db.conn);
        for (id, model, cost) in [
            ("tiered-history", "gemini-4-hydrogen-tiered-high", "0"),
            ("unresolved-mode", "orchestrator-carbon", "0"),
            ("already-priced", "gemini-4-hydrogen-tiered-high", "9"),
        ] {
            insert_usage_log(
                &conn,
                id,
                "agy",
                "_agy_session",
                model,
                "agy_session",
                1000,
                1_000_000,
                100_000,
                1_000_000,
                0,
                200,
                cost,
            )?;
        }
        conn.execute(
            "UPDATE proxy_request_logs SET request_model=model, input_token_semantics=?1",
            [INPUT_TOKEN_SEMANTICS_FRESH],
        )?;
    }
    assert_eq!(
        db.backfill_missing_usage_costs_for_model("gemini-4-hydrogen")?,
        1
    );
    assert_eq!(db.backfill_missing_usage_costs()?, 0);
    let conn = lock_conn!(db.conn);
    let row: (String, String, String) = conn.query_row(
        "SELECT model, request_model, total_cost_usd FROM proxy_request_logs
             WHERE request_id='tiered-history'",
        [],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    assert_eq!(
        row,
        (
            "gemini-4-hydrogen-tiered-high".into(),
            "gemini-4-hydrogen-tiered-high".into(),
            "2.325000".into(),
        )
    );
    for (id, expected) in [("unresolved-mode", "0"), ("already-priced", "9")] {
        let cost: String = conn.query_row(
            "SELECT total_cost_usd FROM proxy_request_logs WHERE request_id=?1",
            [id],
            |r| r.get(0),
        )?;
        assert_eq!(cost, expected);
    }
    let totals: (i64, i64, i64, i64) = conn.query_row(
        "SELECT COUNT(*),SUM(input_tokens),SUM(output_tokens),SUM(cache_read_tokens)
             FROM proxy_request_logs",
        [],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    )?;
    assert_eq!(totals, (3, 3_000_000, 300_000, 3_000_000));
    Ok(())
}

#[test]
fn test_claude_thinking_pricing_matches_base_model() -> Result<(), AppError> {
    let db = Database::memory()?;
    let conn = lock_conn!(db.conn);
    let opus = find_model_pricing_row(&conn, "claude-opus-4-6")?;
    let sonnet = find_model_pricing_row(&conn, "claude-sonnet-4-6")?;
    assert!(opus.is_some() && sonnet.is_some());
    for model in [
        "claude-opus-4.6-thinking",
        "claude-opus-4-6-thinking",
        "models/claude-opus-4.6-thinking",
        "anthropic.claude-opus-4-6-thinking",
    ] {
        assert_eq!(find_model_pricing_row(&conn, model)?, opus, "{model}");
    }
    for model in ["claude-sonnet-4.6-thinking", "claude-sonnet-4-6-thinking"] {
        assert_eq!(find_model_pricing_row(&conn, model)?, sonnet, "{model}");
    }
    // A thinking variant resolves exactly like its base ID, including bases that only
    // have a dated row (short ID -> dated row is the normal Claude lookup).
    for base in [
        "claude-opus-4-1",
        "claude-opus-4-5",
        "claude-opus-4-6",
        "claude-opus-4-8",
        "claude-opus-5-5",
        "claude-sonnet-4-5",
        "claude-sonnet-4-6",
        "claude-haiku-4-5",
    ] {
        let expected = find_model_pricing_row(&conn, base)?;
        assert!(expected.is_some(), "{base}");
        let dotted = format!("{}.{}", &base[..base.len() - 2], &base[base.len() - 1..]);
        for model in [format!("{base}-thinking"), format!("{dotted}-thinking")] {
            assert_eq!(find_model_pricing_row(&conn, &model)?, expected, "{model}");
        }
    }

    // Other `-thinking` models are separately priced; never strip them to a base ID.
    let kimi_thinking = query_model_pricing_exact(&conn, "kimi-k2-thinking")?;
    assert!(kimi_thinking.is_some());
    assert_ne!(kimi_thinking, query_model_pricing_exact(&conn, "kimi-k2")?);
    for model in ["kimi-k2-thinking", "claude-kimi-k2-thinking"] {
        assert_eq!(
            find_model_pricing_row(&conn, model)?,
            kimi_thinking,
            "{model}"
        );
    }
    assert_eq!(
        strip_claude_thinking_suffix("claude-kimi-k2-thinking"),
        None
    );
    assert_eq!(strip_claude_thinking_suffix("claude-thinking"), None);

    // An exact custom thinking price still wins over the base estimate.
    conn.execute(
        "INSERT INTO model_pricing VALUES ('claude-opus-4-6-thinking',
             'Custom Opus 4.6 Thinking', '9', '45', '0.9', '11.25')",
        [],
    )?;
    assert_eq!(
        find_model_pricing_row(&conn, "claude-opus-4.6-thinking")?,
        Some(("9".into(), "45".into(), "0.9".into(), "11.25".into()))
    );

    // Unknown Claude versions stay unpriced instead of borrowing another family's price.
    assert!(find_model_pricing_row(&conn, "claude-opus-9-9-thinking")?.is_none());
    Ok(())
}

#[test]
fn test_claude_thinking_backfills_without_transcripts() -> Result<(), AppError> {
    let db = Database::memory()?;
    {
        let conn = lock_conn!(db.conn);
        for (id, model, cost) in [
            ("thinking-history", "claude-opus-4.6-thinking", "0"),
            ("already-priced", "claude-opus-4.6-thinking", "9"),
        ] {
            insert_usage_log(
                &conn,
                id,
                "agy",
                "_agy_session",
                model,
                "agy_session",
                1000,
                1_000_000,
                100_000,
                1_000_000,
                0,
                200,
                cost,
            )?;
        }
        conn.execute(
            "UPDATE proxy_request_logs SET request_model='Claude Opus 4.6 (Thinking)',
                 input_token_semantics=?1",
            [INPUT_TOKEN_SEMANTICS_FRESH],
        )?;
    }
    // Updating the base Opus 4.6 price must reach the retained thinking row.
    assert_eq!(
        db.backfill_missing_usage_costs_for_model("claude-opus-4-6")?,
        1
    );
    assert_eq!(db.backfill_missing_usage_costs()?, 0);
    let conn = lock_conn!(db.conn);
    for (id, expected) in [("thinking-history", "8.000000"), ("already-priced", "9")] {
        let row: (String, String, String) = conn.query_row(
            "SELECT model, request_model, total_cost_usd FROM proxy_request_logs
                 WHERE request_id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        assert_eq!(
            row,
            (
                "claude-opus-4.6-thinking".into(),
                "Claude Opus 4.6 (Thinking)".into(),
                expected.into(),
            ),
            "{id}"
        );
    }
    Ok(())
}

#[test]
fn test_hydrogen_orchestrator_backfills_without_transcripts() -> Result<(), AppError> {
    let db = Database::memory()?;
    {
        let conn = lock_conn!(db.conn);
        for (id, model, cost) in [
            ("hydrogen-history", "gemini-4-hydrogen-orchestrator", "0"),
            ("unresolved-mode", "orchestrator-carbon", "0"),
            ("already-priced", "gemini-4-hydrogen-orchestrator", "9"),
        ] {
            insert_usage_log(
                &conn,
                id,
                "agy",
                "_agy_session",
                model,
                "agy_session",
                1000,
                1_000_000,
                100_000,
                1_000_000,
                0,
                200,
                cost,
            )?;
        }
        conn.execute(
            "UPDATE proxy_request_logs SET request_model='Orchestrator (Carbon)',
                 input_token_semantics=?1",
            [INPUT_TOKEN_SEMANTICS_FRESH],
        )?;
    }
    // Updating the base price must find retained variant rows without reading JSONL/SQLite.
    assert_eq!(
        db.backfill_missing_usage_costs_for_model("gemini-4-hydrogen")?,
        1
    );
    assert_eq!(db.backfill_missing_usage_costs()?, 0);
    let conn = lock_conn!(db.conn);
    let row: (String, String, String, String, String) = conn.query_row(
        "SELECT model,request_model,input_cost_usd,output_cost_usd,cache_read_cost_usd
             FROM proxy_request_logs WHERE request_id='hydrogen-history'",
        [],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
    )?;
    assert_eq!(
        row,
        (
            "gemini-4-hydrogen-orchestrator".into(),
            "Orchestrator (Carbon)".into(),
            "1.500000".into(),
            "0.750000".into(),
            "0.075000".into()
        )
    );
    for (id, expected) in [
        ("hydrogen-history", "2.325000"),
        ("unresolved-mode", "0"),
        ("already-priced", "9"),
    ] {
        let cost: String = conn.query_row(
            "SELECT total_cost_usd FROM proxy_request_logs WHERE request_id=?1",
            [id],
            |r| r.get(0),
        )?;
        assert_eq!(cost, expected);
    }
    let totals: (i64, i64, i64, i64) = conn.query_row(
        "SELECT COUNT(*),SUM(input_tokens),SUM(output_tokens),SUM(cache_read_tokens) FROM proxy_request_logs",
        [], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?)),
    )?;
    assert_eq!(totals, (3, 3_000_000, 300_000, 3_000_000));
    Ok(())
}

#[test]
fn test_gemini_flash_tiered_pricing_preserves_exact_overrides() -> Result<(), AppError> {
    let db = Database::memory()?;
    let conn = lock_conn!(db.conn);
    let base_38 = find_model_pricing_row(&conn, "gemini-3.8-flash")?;
    let base_37 = find_model_pricing_row(&conn, "gemini-3.7-flash")?;
    assert!(base_38.is_some());
    assert!(base_37.is_some());
    // Built-in seeds contain `gemini-3.1-flash-lite` (no `gemini-3.1-flash`) and
    // `gemini-3-flash-preview` (no `gemini-3-flash`); verify they exist before
    // asserting that `-tiered` never prefix-matches across sub-tiers or previews.
    assert!(find_model_pricing_row(&conn, "gemini-3.1-flash-lite")?.is_some());
    assert!(find_model_pricing_row(&conn, "gemini-3-flash-preview")?.is_some());

    for model in [
        "gemini-3.8-flash-tiered",
        "models/gemini-3.8-flash-tiered",
        "google/GEMINI-3.8-FLASH-TIERED",
        "google.gemini-3.8-flash-tiered:free",
        "gemini-3.8-flash@tiered",
        "gemini-3.8-flash-tiered-high",
        "gemini-3.8-flash-tiered-2026-09-02",
        "claude-gemini-3.8-flash-tiered",
    ] {
        assert_eq!(find_model_pricing_row(&conn, model)?, base_38, "{model}");
    }
    for model in ["gemini-3.7-flash-tiered", "models/gemini-3.7-flash-tiered"] {
        assert_eq!(find_model_pricing_row(&conn, model)?, base_37, "{model}");
    }

    // `-tiered` is a Gemini Flash endpoint suffix, not a generic suffix for other families,
    // and must not prefix-match `gemini-3.1-flash-lite` or `-preview` when the exact base
    // `gemini-<ver>-flash` entry is absent.
    for model in [
        "gemini-3.1-flash-tiered",
        "models/gemini-3.1-flash-tiered",
        "claude-gemini-3.1-flash-tiered",
        "gemini-3.1-flash-tiered-high",
        "gemini-3.1-flash-tiered-2026-09-02",
        "gemini-3-flash-tiered",
        "gemini-4-argon-tiered",
        "gemini-3.1-pro-tiered",
        "gemini-3.5-flash-lite-tiered",
        "gemini-3.8-flash-tiered-preview",
        "gemini-pro-flash-tiered",
        "gemini-.8-flash-tiered",
        "gemini-3.-flash-tiered",
        "gemini--flash-tiered",
        "gemini-3..8-flash-tiered",
        "step-3.7-flash-tiered",
        "custom-flash-tiered",
        "gemini-3.8-flash-n",
    ] {
        assert!(find_model_pricing_row(&conn, model)?.is_none(), "{model}");
    }

    // Deleting an existing base Flash row or adding only a sub-tier row must also
    // stay unpriced rather than falling into `LIKE 'gemini-<ver>-flash-%'`.
    conn.execute(
        "DELETE FROM model_pricing WHERE model_id = 'gemini-3.5-flash'",
        [],
    )?;
    conn.execute(
        "INSERT INTO model_pricing VALUES ('gemini-3.9-flash-low',
             'Custom Gemini 3.9 Flash Low', '0.2', '1.0', '0.02', '0')",
        [],
    )?;
    conn.execute(
        "INSERT INTO model_pricing VALUES ('gemini-3.9-flash-tiered-low',
             'Custom Gemini 3.9 Flash Tiered Low', '0.4', '2.0', '0.04', '0')",
        [],
    )?;
    assert!(find_model_pricing_row(&conn, "gemini-3.5-flash-lite")?.is_some());
    assert!(find_model_pricing_row(&conn, "gemini-3.5-flash-tiered")?.is_none());
    assert!(find_model_pricing_row(&conn, "gemini-3.9-flash-tiered")?.is_none());
    assert!(find_model_pricing_row(&conn, "models/gemini-3.9-flash-tiered")?.is_none());
    assert!(find_model_pricing_row(&conn, "gemini-3.9-flash-tiered-high")?.is_none());
    assert_eq!(
        find_model_pricing_row(&conn, "gemini-3.9-flash-tiered-low")?,
        Some(("0.4".into(), "2.0".into(), "0.04".into(), "0".into()))
    );

    // Single-integer major versions still match when an exact `gemini-<major>-flash` row exists.
    conn.execute(
        "INSERT INTO model_pricing VALUES ('gemini-4-flash',
             'Custom Gemini 4 Flash', '1', '5', '0.1', '0')",
        [],
    )?;
    assert_eq!(
        find_model_pricing_row(&conn, "gemini-4-flash-tiered")?,
        Some(("1".into(), "5".into(), "0.1".into(), "0".into()))
    );

    conn.execute(
        "INSERT INTO model_pricing VALUES ('gemini-3.8-flash-tiered',
             'Custom Gemini 3.8 Flash Tiered', '2', '8', '0.2', '0')",
        [],
    )?;
    for model in [
        "gemini-3.8-flash-tiered",
        "models/gemini-3.8-flash-tiered",
        "google.gemini-3.8-flash-tiered:free",
        "claude-gemini-3.8-flash-tiered",
        "gemini-3.8-flash-tiered-high",
        "google.gemini-3.8-flash-tiered-high",
    ] {
        assert_eq!(
            find_model_pricing_row(&conn, model)?,
            Some(("2".into(), "8".into(), "0.2".into(), "0".into())),
            "{model}"
        );
    }
    assert_eq!(
        find_model_pricing_row(&conn, "gemini-3.7-flash-tiered")?,
        base_37
    );

    conn.execute(
        "INSERT INTO model_pricing VALUES ('gemini-3.8-flash-tiered-high',
             'Custom Gemini 3.8 Flash Tiered High', '3', '12', '0.3', '0')",
        [],
    )?;
    for model in [
        "gemini-3.8-flash-tiered-high",
        "models/gemini-3.8-flash-tiered-high",
        "google.gemini-3.8-flash-tiered-high:free",
        "claude-gemini-3.8-flash-tiered-high",
        "bedrock.google.gemini-3.8-flash-tiered-high-v1:0",
    ] {
        assert_eq!(
            find_model_pricing_row(&conn, model)?,
            Some(("3".into(), "12".into(), "0.3".into(), "0".into())),
            "{model}"
        );
    }
    assert_eq!(
        find_model_pricing_row(&conn, "google.gemini-3.8-flash-tiered-low")?,
        Some(("2".into(), "8".into(), "0.2".into(), "0".into()))
    );
    Ok(())
}

#[test]
fn test_gemini_flash_tiered_backfills_without_transcripts() -> Result<(), AppError> {
    let db = Database::memory()?;
    {
        let conn = lock_conn!(db.conn);
        for (id, model, cost) in [
            ("browser-38", "gemini-3.8-flash-tiered", "0"),
            ("browser-37", "gemini-3.7-flash-tiered", "0"),
            ("unpriced-31", "gemini-3.1-flash-tiered", "0"),
            ("unpriced-n", "gemini-3.8-flash-n", "0"),
            ("already-priced", "gemini-3.8-flash-tiered", "5"),
        ] {
            insert_usage_log(
                &conn,
                id,
                "agy",
                "_agy_session",
                model,
                "agy_session",
                1000,
                1_000_000,
                100_000,
                1_000_000,
                0,
                200,
                cost,
            )?;
        }
        conn.execute(
            "UPDATE proxy_request_logs SET input_token_semantics=?1",
            [INPUT_TOKEN_SEMANTICS_FRESH],
        )?;
    }

    // Scoped Flash Lite updates and full backfills must never misprice `gemini-3.1-flash-tiered`.
    assert_eq!(
        db.backfill_missing_usage_costs_for_model("gemini-3.1-flash-lite")?,
        0
    );
    // Scoped base-model updates must only backfill matching Flash version rows.
    assert_eq!(
        db.backfill_missing_usage_costs_for_model("gemini-3.8-flash")?,
        1
    );
    assert_eq!(
        db.backfill_missing_usage_costs_for_model("gemini-3.7-flash")?,
        1
    );
    assert_eq!(db.backfill_missing_usage_costs()?, 0);

    {
        let conn = lock_conn!(db.conn);
        for (id, expected_model) in [
            ("browser-38", "gemini-3.8-flash-tiered"),
            ("browser-37", "gemini-3.7-flash-tiered"),
        ] {
            let row: (String, String, String, String, String) = conn.query_row(
                "SELECT model,input_cost_usd,output_cost_usd,cache_read_cost_usd,total_cost_usd
                     FROM proxy_request_logs WHERE request_id=?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )?;
            assert_eq!(
                row,
                (
                    expected_model.into(),
                    "0.750000".into(),
                    "0.375000".into(),
                    "0.075000".into(),
                    "1.200000".into(),
                )
            );
        }
        for (id, expected) in [
            ("unpriced-31", "0"),
            ("unpriced-n", "0"),
            ("already-priced", "5"),
        ] {
            let cost: String = conn.query_row(
                "SELECT total_cost_usd FROM proxy_request_logs WHERE request_id=?1",
                [id],
                |r| r.get(0),
            )?;
            assert_eq!(cost, expected, "{id}");
        }
        conn.execute(
            "INSERT INTO model_pricing VALUES ('gemini-3.1-flash-tiered-high',
                 'Custom Gemini 3.1 Flash Tiered High', '3', '12', '0.3', '0')",
            [],
        )?;
    }

    // Adding a custom per-effort `-tiered-high` entry must not misprice bare `-tiered` rows
    // and permanently lock them out of a later base-model backfill.
    assert_eq!(
        db.backfill_missing_usage_costs_for_model("gemini-3.1-flash-tiered-high")?,
        0
    );
    assert_eq!(db.backfill_missing_usage_costs()?, 0);

    {
        let conn = lock_conn!(db.conn);
        conn.execute(
            "INSERT INTO model_pricing VALUES ('gemini-3.1-flash',
                 'Custom Gemini 3.1 Flash', '0.5', '2.5', '0.05', '0')",
            [],
        )?;
    }

    // Once an exact base `gemini-3.1-flash` entry is added, scoped backfill reprices it.
    assert_eq!(
        db.backfill_missing_usage_costs_for_model("gemini-3.1-flash")?,
        1
    );
    let conn = lock_conn!(db.conn);
    let cost_31: String = conn.query_row(
        "SELECT total_cost_usd FROM proxy_request_logs WHERE request_id='unpriced-31'",
        [],
        |r| r.get(0),
    )?;
    assert_eq!(cost_31, "0.800000");
    let totals: (i64, i64, i64, i64) = conn.query_row(
        "SELECT COUNT(*),SUM(input_tokens),SUM(output_tokens),SUM(cache_read_tokens) FROM proxy_request_logs",
        [],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    )?;
    assert_eq!(totals, (5, 5_000_000, 500_000, 5_000_000));
    Ok(())
}

#[test]
fn test_orchestrator_cli_worker_alias_pricing_and_orphan_backfill() -> Result<(), AppError> {
    let db = Database::memory()?;
    {
        let conn = lock_conn!(db.conn);
        let base = find_model_pricing_row(&conn, "gemini-4-hydrogen")?;
        assert!(base.is_some());
        for model in [
            "gemini-flash-worker-agy",
            "google/GEMINI-FLASH-WORKER-AGY",
            "google.gemini-flash-worker-agy:free",
            "gemini-flash-worker-agy-high",
        ] {
            assert_eq!(find_model_pricing_row(&conn, model)?, base, "{model}");
        }
        for model in [
            "gemini-flash",
            "gemini-flash-worker",
            "gemini-flash-worker-agy-preview",
            "gemini-pro-orchestrator",
        ] {
            assert!(find_model_pricing_row(&conn, model)?.is_none(), "{model}");
        }

        conn.execute(
            "INSERT INTO model_pricing VALUES ('gemini-flash-worker-agy',
                 'Custom Hydrogen Worker', '3', '12', '0.2', '0')",
            [],
        )?;
        for model in [
            "gemini-flash-worker-agy",
            "google.gemini-flash-worker-agy:free",
            "claude-gemini-flash-worker-agy",
            "gemini-flash-worker-agy-high",
            "google.gemini-flash-worker-agy-high",
        ] {
            assert_eq!(
                find_model_pricing_row(&conn, model)?,
                Some(("3".into(), "12".into(), "0.2".into(), "0".into())),
                "{model}"
            );
        }
        conn.execute(
            "INSERT INTO model_pricing VALUES ('gemini-flash-worker-agy-high',
                 'Custom Hydrogen Worker High', '4', '16', '0.3', '0')",
            [],
        )?;
        for model in [
            "gemini-flash-worker-agy-high",
            "models/gemini-flash-worker-agy-high",
            "google.gemini-flash-worker-agy-high:free",
            "claude-gemini-flash-worker-agy-high",
        ] {
            assert_eq!(
                find_model_pricing_row(&conn, model)?,
                Some(("4".into(), "16".into(), "0.3".into(), "0".into())),
                "{model}"
            );
        }
        assert_eq!(
            find_model_pricing_row(&conn, "google.gemini-flash-worker-agy-low")?,
            Some(("3".into(), "12".into(), "0.2".into(), "0".into()))
        );
        conn.execute(
            "DELETE FROM model_pricing WHERE model_id LIKE 'gemini-flash-worker-agy%'",
            [],
        )?;

        // When base `gemini-4-hydrogen` is absent, neither `gemini-flash-worker-agy` nor its
        // base fallback may prefix-match sibling `gemini-4-hydrogen-*` or `-high` entries.
        conn.execute(
            "DELETE FROM model_pricing WHERE model_id = 'gemini-4-hydrogen'",
            [],
        )?;
        conn.execute(
            "INSERT INTO model_pricing VALUES ('gemini-4-hydrogen-orchestrator',
                 'Custom Hydrogen Orchestrator', '5', '20', '0.5', '0')",
            [],
        )?;
        conn.execute(
            "INSERT INTO model_pricing VALUES ('gemini-flash-worker-agy-high',
                 'Custom Hydrogen Worker High', '4', '16', '0.3', '0')",
            [],
        )?;
        assert!(find_model_pricing_row(&conn, "gemini-flash-worker-agy")?.is_none());
        assert!(find_model_pricing_row(&conn, "google.gemini-flash-worker-agy")?.is_none());

        for (id, model, cost) in [
            ("worker-orphan", "gemini-flash-worker-agy", "0"),
            ("unresolved-planner", "gemini-pro-orchestrator", "0"),
            ("already-priced", "gemini-flash-worker-agy", "7"),
        ] {
            insert_usage_log(
                &conn,
                id,
                "agy",
                "_agy_session",
                model,
                "agy_session",
                1000,
                1_000_000,
                100_000,
                1_000_000,
                0,
                200,
                cost,
            )?;
        }
        conn.execute(
            "UPDATE proxy_request_logs SET input_token_semantics=?1",
            [INPUT_TOKEN_SEMANTICS_FRESH],
        )?;
    }

    // Sibling custom entries must never misprice bare `gemini-flash-worker-agy` rows when
    // base `gemini-4-hydrogen` is absent.
    assert_eq!(
        db.backfill_missing_usage_costs_for_model("gemini-flash-worker-agy-high")?,
        0
    );
    assert_eq!(
        db.backfill_missing_usage_costs_for_model("gemini-4-hydrogen-orchestrator")?,
        0
    );
    assert_eq!(db.backfill_missing_usage_costs()?, 0);

    {
        let conn = lock_conn!(db.conn);
        conn.execute(
            "DELETE FROM model_pricing
                 WHERE model_id IN ('gemini-4-hydrogen-orchestrator', 'gemini-flash-worker-agy-high')",
            [],
        )?;
        conn.execute(
            "INSERT INTO model_pricing VALUES ('gemini-4-hydrogen',
                 'Gemini 4 Hydrogen', '1.5', '7.5', '0.075', '0')",
            [],
        )?;
    }

    // Retained worker rows whose transcripts are gone still reprice on Hydrogen updates.
    assert_eq!(
        db.backfill_missing_usage_costs_for_model("gemini-4-hydrogen")?,
        1
    );
    assert_eq!(db.backfill_missing_usage_costs()?, 0);

    let conn = lock_conn!(db.conn);
    let row: (String, String, String, String, String) = conn.query_row(
        "SELECT model,request_model,input_cost_usd,output_cost_usd,cache_read_cost_usd
             FROM proxy_request_logs WHERE request_id='worker-orphan'",
        [],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
    )?;
    assert_eq!(
        row,
        (
            "gemini-flash-worker-agy".into(),
            "gemini-flash-worker-agy".into(),
            "1.500000".into(),
            "0.750000".into(),
            "0.075000".into(),
        )
    );
    for (id, expected) in [
        ("worker-orphan", "2.325000"),
        ("unresolved-planner", "0"),
        ("already-priced", "7"),
    ] {
        let cost: String = conn.query_row(
            "SELECT total_cost_usd FROM proxy_request_logs WHERE request_id=?1",
            [id],
            |r| r.get(0),
        )?;
        assert_eq!(cost, expected, "{id}");
    }
    Ok(())
}

#[test]
fn test_model_pricing_matching() -> Result<(), AppError> {
    let db = Database::memory()?;
    let conn = lock_conn!(db.conn);

    // 准备额外定价数据，覆盖前缀/后缀清洗场景
    conn.execute(
        "INSERT OR REPLACE INTO model_pricing (
                model_id, display_name, input_cost_per_million, output_cost_per_million,
                cache_read_cost_per_million, cache_creation_cost_per_million
            ) VALUES (?, ?, ?, ?, ?, ?)",
        params![
            "claude-haiku-4.5",
            "Claude Haiku 4.5",
            "1.0",
            "2.0",
            "0.0",
            "0.0"
        ],
    )?;

    // 测试精确匹配（seed_model_pricing 已预置 claude-sonnet-4-5-20250929）
    let result = find_model_pricing_row(&conn, "claude-sonnet-4-5-20250929")?;
    assert!(
        result.is_some(),
        "应该能精确匹配 claude-sonnet-4-5-20250929"
    );

    // 清洗：去除前缀和冒号后缀
    let result = find_model_pricing_row(&conn, "anthropic/claude-haiku-4.5")?;
    assert!(
        result.is_some(),
        "带前缀的模型 anthropic/claude-haiku-4.5 应能匹配到 claude-haiku-4.5"
    );
    let result = find_model_pricing_row(&conn, "moonshotai/kimi-k2-0905:exa")?;
    assert!(
        result.is_some(),
        "带前缀+冒号后缀的模型应清洗后匹配到 kimi-k2-0905"
    );

    // 清洗：@ 替换为 -（seed_model_pricing 已预置 gpt-5.2-codex-low）
    let result = find_model_pricing_row(&conn, "gpt-5.2-codex@low")?;
    assert!(
        result.is_some(),
        "带 @ 分隔符的模型 gpt-5.2-codex@low 应能匹配到 gpt-5.2-codex-low"
    );
    let result = find_model_pricing_row(&conn, "OpenAI/GPT-5.5@HIGH")?;
    assert!(
        result.is_some(),
        "大小写混合的 GPT-5.5 模型应能归一化匹配到 gpt-5.5-high"
    );
    let result = find_model_pricing_row(&conn, "OpenAI/GPT-5.5-2026-05-14")?;
    assert!(
        result.is_some(),
        "OpenAI 日期后缀模型应能回退到 gpt-5.5 基础定价"
    );
    let result = find_model_pricing_row(&conn, "google/gemini-3-pro-preview-20260514")?;
    assert!(
        result.is_some(),
        "Gemini 日期后缀模型应能回退到 gemini-3-pro-preview 基础定价"
    );

    // Claude Desktop route 短 ID：应通过前缀匹配到带日期的定价
    let result = find_model_pricing_row(&conn, "claude-haiku-4-5")?;
    assert!(
        result.is_some(),
        "Claude Desktop 短路由 claude-haiku-4-5 应能匹配到 claude-haiku-4-5-20251001"
    );
    let result = find_model_pricing_row(&conn, "anthropic/claude-opus-4.8")?;
    assert!(
        result.is_some(),
        "聚合商点号格式 anthropic/claude-opus-4.8 应能匹配到 claude-opus-4-8"
    );

    // Claude Desktop 旧版/异常包装的非 Anthropic route：claude-gpt-5.5 → gpt-5.5
    let result = find_model_pricing_row(&conn, "claude-gpt-5.5")?;
    assert!(
        result.is_some(),
        "带 claude- 包装的非 Anthropic 模型应能剥离后匹配到真实模型定价"
    );

    // Bedrock/Vertex 常见形态：provider 前缀 + -vN 后缀 + :0 修饰
    let result = find_model_pricing_row(&conn, "global.anthropic.claude-haiku-4-5-20251001-v1:0")?;
    assert!(
        result.is_some(),
        "Bedrock/Vertex 风格 Claude 模型 ID 应能归一化到基础 Claude 模型定价"
    );
    let result = find_model_pricing_row(&conn, "global.anthropic.claude-opus-4-8-v1:0")?;
    assert!(
        result.is_some(),
        "Bedrock 风格 Claude Opus 4.8 模型 ID 应能归一化到基础 Claude 模型定价"
    );
    let result = find_model_pricing_row(&conn, "claude-opus-4-8@20260527")?;
    assert!(
        result.is_some(),
        "Vertex 风格 Claude Opus 4.8 模型 ID 应能归一化到基础 Claude 模型定价"
    );

    // Reasoning effort 后缀：没有专门价格时回退到基础模型
    let result = find_model_pricing_row(&conn, "gpt-5.4@low")?;
    assert!(
        result.is_some(),
        "缺少专门 effort 价格时应回退到 gpt-5.4 基础模型定价"
    );

    // Kimi Code 是订阅/额度模型，不应伪装成公开按 token 计费模型
    let result = find_model_pricing_row(&conn, "kimi-for-coding")?;
    assert!(result.is_none(), "kimi-for-coding 没有固定 token 单价");

    // 测试不存在的模型
    let result = find_model_pricing_row(&conn, "unknown-model-123")?;
    assert!(result.is_none(), "不应该匹配不存在的模型");

    Ok(())
}

#[test]
fn test_range_filter_dedups_codex_request_model_alias() -> Result<(), AppError> {
    // 会话行不少于代理行时走 JOIN 写法，同样要认 Codex 的请求模型别名。
    let db = Database::memory()?;
    {
        let conn = lock_conn!(db.conn);
        insert_usage_log(
            &conn,
            "proxy",
            "codex",
            "provider-1",
            "glm-5.3",
            "proxy",
            10_000,
            19_472,
            165,
            16_704,
            0,
            200,
            "0.10",
        )?;
        conn.execute(
            "UPDATE proxy_request_logs SET request_model = 'ccswitch-live-alpha'
                 WHERE request_id = 'proxy'",
            [],
        )?;
        insert_usage_log(
            &conn,
            "session-alias",
            "codex",
            "_codex_session",
            "ccswitch-live-alpha",
            "codex_session",
            10_001,
            19_472,
            165,
            16_704,
            0,
            200,
            "0.10",
        )?;
        insert_usage_log(
            &conn,
            "session-other-alias",
            "codex",
            "_codex_session",
            "other-alias",
            "codex_session",
            10_002,
            19_472,
            165,
            16_704,
            0,
            200,
            "0.10",
        )?;

        let filter = effective_usage_log_filter_for_range(&conn, "l", None, None)?;
        assert!(filter.contains("rowid NOT IN"), "应走 JOIN 写法: {filter}");
        let kept: Vec<String> = conn
            .prepare(&format!(
                "SELECT request_id FROM proxy_request_logs l WHERE {filter} ORDER BY request_id"
            ))?
            .query_map([], |row| row.get(0))?
            .collect::<Result<_, _>>()?;
        assert_eq!(kept, ["proxy", "session-other-alias"]);
    }
    assert_eq!(
        db.get_usage_summary(None, None, Some("codex"), None, None)?
            .total_requests,
        2
    );
    Ok(())
}

#[test]
fn test_reasoning_statistics_require_equal_prices_and_known_base() -> Result<(), AppError> {
    let db = Database::memory()?;
    let conn = lock_conn!(db.conn);
    conn.execute(
        "INSERT OR REPLACE INTO model_pricing VALUES
            ('gemini-3.8-flash', 'Flash', '0.75', '3.75', '0.075', '0'),
            ('gemini-3.8-flash-high', 'High', '0.750', '3.750', '0.0750', '0.0'),
            ('gemini-3.8-flash-low', 'Low', '0.75', '3.75', '0.15', '0'),
            ('gemini-3.8-flash-minimal', 'Minimal', '0.75', '3.75', '0.075', '1'),
            ('gemini-9-pro-low', 'Nine Low', '1', '5', '0.1', '0'),
            ('gemini-9-pro-high', 'Nine High', '2', '10', '0.2', '0'),
            ('claude-opus-5-5', 'Opus', '4', '20', '0.2', '5')",
        [],
    )?;

    for (model, expected) in [
        ("gemini-3.8-flash-high", "gemini-3.8-flash"),
        ("gemini-3.8-flash-medium", "gemini-3.8-flash"),
        ("gemini-3.8-flash-tiered", "gemini-3.8-flash"),
        ("gemini-3.8-flash-tiered-high", "gemini-3.8-flash"),
        ("models/gemini-3.8-flash-tiered", "models/gemini-3.8-flash"),
        ("google/GEMINI-3.8-FLASH-TIERED", "google/GEMINI-3.8-FLASH"),
        ("gemini-4-hydrogen-high", "gemini-4-hydrogen"),
        ("gemini-4-hydrogen-tiered", "gemini-4-hydrogen"),
        ("gemini-4-hydrogen-tiered-high", "gemini-4-hydrogen"),
        (
            "models/gemini-4-hydrogen-tiered-HIGH",
            "models/gemini-4-hydrogen",
        ),
        ("gemini-4-neon-high", "gemini-4-neon"),
        ("gemini-4-neon-xhigh", "gemini-4-neon"),
        ("claude-opus-5.5-medium", "claude-opus-5.5"),
        ("GPT-5.5-HIGH", "GPT-5.5"),
    ] {
        assert_eq!(statistics_model(&conn, model)?, expected, "{model}");
    }

    // 缺失基础模型定价（gemini-9-pro、devstral、magistral、gemini-3.1-flash）绝不能通过前缀匹配合并；
    // 自定义差价、Flash Lite 与未知模型同样保持独立。
    for model in [
        "gemini-3.8-flash-low",
        "gemini-3.8-flash-minimal",
        "gemini-9-pro-low",
        "gemini-9-pro-medium",
        "gemini-9-pro-high",
        "devstral-medium",
        "magistral-medium",
        "missing-high",
        "gemini-3.5-flash-lite",
        "gemini-3.1-flash-tiered",
        "custom-tiered",
    ] {
        assert_eq!(statistics_model(&conn, model)?, model, "{model}");
    }

    // 输入或输出差价同样不能合并。
    for column in ["input_cost_per_million", "output_cost_per_million"] {
        conn.execute(
            &format!(
                "UPDATE model_pricing SET {column} = '99' WHERE model_id = 'gemini-3.8-flash-high'"
            ),
            [],
        )?;
        assert_eq!(
            statistics_model(&conn, "gemini-3.8-flash-high")?,
            "gemini-3.8-flash-high"
        );
        conn.execute(
            "UPDATE model_pricing SET input_cost_per_million = '0.75', output_cost_per_million = '3.75' WHERE model_id = 'gemini-3.8-flash-high'",
            [],
        )?;
    }

    // 为 `-tiered` 单独配置差价后，`-tiered` 保持独立，`-tiered-high` 只合并到 `-tiered`。
    conn.execute(
        "INSERT OR REPLACE INTO model_pricing VALUES
            ('gemini-4-hydrogen-tiered', 'Custom Hydrogen Tiered', '2', '10', '0.1', '0')",
        [],
    )?;
    assert_eq!(
        statistics_model(&conn, "gemini-4-hydrogen-tiered")?,
        "gemini-4-hydrogen-tiered"
    );
    assert_eq!(
        statistics_model(&conn, "gemini-4-hydrogen-tiered-high")?,
        "gemini-4-hydrogen-tiered"
    );
    Ok(())
}

#[test]
fn test_flash_and_hydrogen_tiered_pricing_backfill_and_grouping() -> Result<(), AppError> {
    let db = Database::memory()?;
    {
        let conn = lock_conn!(db.conn);
        for (id, model) in [
            ("flash-tiered", "gemini-3.8-flash-tiered"),
            ("flash-medium", "gemini-3.8-flash-medium"),
            ("h-tiered-high", "gemini-4-hydrogen-tiered-high"),
            ("h-low", "gemini-4-hydrogen-low"),
            ("neon-high", "gemini-4-neon-high"),
        ] {
            insert_usage_log(
                &conn,
                id,
                "agy",
                "_agy_session",
                model,
                "agy_session",
                1000,
                100,
                200,
                0,
                0,
                200,
                "0",
            )?;
        }
    }
    assert_eq!(
        db.backfill_missing_usage_costs_for_model("gemini-3.8-flash")?,
        2
    );
    assert_eq!(
        db.backfill_missing_usage_costs_for_model("gemini-4-hydrogen")?,
        2
    );
    assert_eq!(
        db.backfill_missing_usage_costs_for_model("gemini-4-neon")?,
        1
    );

    let flash_stats =
        db.get_model_stats(None, None, Some("agy"), None, Some("gemini-3.8-flash"))?;
    assert_eq!(flash_stats.len(), 1);
    assert_eq!(flash_stats[0].model, "gemini-3.8-flash");
    assert_eq!(flash_stats[0].request_count, 2);
    assert_eq!(flash_stats[0].total_cost, "0.001650");

    let hydrogen_stats =
        db.get_model_stats(None, None, Some("agy"), None, Some("gemini-4-hydrogen"))?;
    assert_eq!(hydrogen_stats.len(), 1);
    assert_eq!(hydrogen_stats[0].model, "gemini-4-hydrogen");
    assert_eq!(hydrogen_stats[0].request_count, 2);
    assert_eq!(hydrogen_stats[0].total_cost, "0.003300");

    let neon_stats = db.get_model_stats(None, None, Some("agy"), None, Some("gemini-4-neon"))?;
    assert_eq!(neon_stats.len(), 1);
    assert_eq!(neon_stats[0].model, "gemini-4-neon");
    assert_eq!(neon_stats[0].request_count, 1);
    assert_eq!(neon_stats[0].total_cost, "0.003300");

    let detail = db
        .get_request_detail("flash-tiered")?
        .expect("tiered request");
    assert_eq!(detail.model, "gemini-3.8-flash-tiered");
    assert_eq!(detail.total_cost_usd, "0.000825");
    Ok(())
}

#[test]
fn test_merged_reasoning_models_keep_totals_filters_and_pooled_medians() -> Result<(), AppError> {
    let db = Database::memory()?;
    let ts = local_ts(2026, 6, 10, 12, 0, 0);
    {
        let conn = lock_conn!(db.conn);
        // 样本分布在三个档位：base=[10]、high=[20, 30]、medium=[1000, 2000, 3000]。
        // 全样本合并中位数 = (30 + 1000) / 2 = 515；
        // 各档位中位数 [10, 25, 2000] 的中位数 = 25、均值 = 678、加权均值 = 1010，互不相同。
        for (id, model, first, cost) in [
            ("base", "gemini-4-argon", 10, "1"),
            ("high-1", "gemini-4-argon-high", 20, "2"),
            ("high-2", "gemini-4-argon-HIGH", 30, "3"),
            ("medium-1", "gemini-4-argon-medium", 1000, "4"),
            ("medium-2", "gemini-4-argon-medium", 2000, "5"),
            ("medium-3", "gemini-4-argon-medium", 3000, "6"),
        ] {
            insert_usage_log(
                &conn,
                id,
                "agy",
                "_agy_session",
                model,
                "proxy",
                ts,
                100,
                200,
                0,
                0,
                200,
                cost,
            )?;
            conn.execute(
                "UPDATE proxy_request_logs SET first_token_ms = ?1, latency_ms = ?1 + 1000 WHERE request_id = ?2",
                params![first, id],
            )?;
        }
        conn.execute(
            "UPDATE proxy_request_logs SET model = 'routed-alias', pricing_model = 'gemini-4-argon-medium' WHERE request_id = 'medium-1'",
            [],
        )?;
        conn.execute(
            "INSERT INTO usage_daily_rollups (
                    date, app_type, provider_id, model, request_count, success_count,
                    input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                    total_cost_usd, avg_latency_ms
                ) VALUES ('2026-06-08', 'agy', '_agy_session', 'gemini-4-argon-high', 2, 1, 200, 400, 0, 0, '5', 1000)",
            [],
        )?;
        // 带前缀的 `-tiered` 也要支持合并及筛选回查。
        insert_usage_log(
            &conn,
            "prefixed-tiered",
            "agy",
            "_agy_session",
            "models/gemini-3.8-flash-tiered",
            "agy_session",
            ts,
            100,
            200,
            0,
            0,
            200,
            "1",
        )?;
    }

    let stats = db.get_model_stats(None, None, Some("agy"), None, None)?;
    assert_eq!(stats.len(), 2);
    let stat = stats
        .iter()
        .find(|s| s.model == "gemini-4-argon")
        .expect("merged argon stat");
    assert_eq!(stat.request_count, 8);
    assert_eq!(stat.total_tokens, 2400);
    assert_eq!(stat.total_cost, "26.000000");
    assert_eq!(stat.avg_cost_per_request, "3.250000");
    assert!((stat.success_rate - 700.0 / 8.0).abs() < 0.001);
    assert_eq!(stat.speed_output_tokens, 1200);
    assert_eq!(stat.speed_generation_ms, 6000);
    assert_eq!(stat.first_token_median_ms, Some(515));

    let model = Some("gemini-4-argon");
    let summary = db.get_usage_summary(None, None, Some("agy"), None, model)?;
    assert_eq!(summary.total_requests, 8);
    assert_eq!(summary.total_cost, "26.000000");
    assert_eq!(
        db.get_usage_summary_by_app(None, None, None, model)?[0]
            .summary
            .total_requests,
        8
    );
    assert_eq!(
        db.get_provider_stats(None, None, Some("agy"), None, model)?[0].request_count,
        8
    );
    assert_eq!(
        db.get_model_stats(None, None, Some("agy"), None, model)?[0].request_count,
        8
    );
    assert_eq!(
        db.get_first_usage_date(Some("agy"), None, model)?
            .as_deref(),
        Some("2026-06-08")
    );
    let trends = db.get_daily_trends(
        Some(local_ts(2026, 6, 8, 0, 0, 0)),
        Some(ts + 1),
        Some("agy"),
        None,
        model,
    )?;
    assert_eq!(trends.iter().map(|t| t.request_count).sum::<u64>(), 8);
    let hourly = db.get_daily_trends(Some(ts - 3600), Some(ts + 1), Some("agy"), None, model)?;
    assert_eq!(hourly.iter().map(|t| t.request_count).sum::<u64>(), 6);
    let logs = db.get_request_logs(
        &LogFilters {
            model: model.map(str::to_string),
            ..Default::default()
        },
        0,
        10,
    )?;
    assert_eq!(logs.total, 6);
    assert!(logs.data.iter().any(|l| l.model == "gemini-4-argon-high"));
    assert!(logs.data.iter().any(|l| l.model == "gemini-4-argon-HIGH"));
    assert!(logs.data.iter().any(|l| l.model == "routed-alias"));

    // `models/` 前缀的 `-tiered` 合并与筛选闭环。
    let prefixed = db.get_model_stats(
        None,
        None,
        Some("agy"),
        None,
        Some("models/gemini-3.8-flash"),
    )?;
    assert_eq!(prefixed.len(), 1);
    assert_eq!(prefixed[0].model, "models/gemini-3.8-flash");
    assert_eq!(prefixed[0].request_count, 1);
    Ok(())
}

#[test]
fn selection_median_matches_sort_for_duplicates_and_extreme_values() {
    for len in 1..100 {
        let input: Vec<_> = (0..len)
            .map(|i| ((i * 7919 + len * 13) % 37) as u64)
            .collect();
        let mut sorted = input.clone();
        sorted.sort_unstable();
        let middle = len / 2;
        let expected = if len % 2 == 0 {
            (sorted[middle - 1] + sorted[middle]) / 2
        } else {
            sorted[middle]
        };
        assert_eq!(select_median(&mut input.clone()), expected);
    }
    assert_eq!(select_median(&mut [u64::MAX, u64::MAX - 2]), u64::MAX - 1);
}

#[test]
fn app_and_pricing_filters_use_matching_indexes() -> Result<(), AppError> {
    let db = Database::memory()?;
    let conn = lock_conn!(db.conn);
    let filter = UsageFilter {
        app_type: Some("claude"),
        provider_name: None,
        model: None,
        pricing: OnceCell::new(),
    };
    let mut conditions = SqlWhere::default();
    filter.push_to(&conn, &mut conditions, "l", "p")?;
    conditions.push("l.created_at >= ?", 100i64);
    conditions.push("l.created_at <= ?", 200i64);
    let (clause, params) = conditions.into_where();
    let refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let mut stmt = conn.prepare(&format!(
        "EXPLAIN QUERY PLAN SELECT l.request_id FROM proxy_request_logs l {clause}"
    ))?;
    let plans = stmt
        .query_map(refs.as_slice(), |row| row.get::<_, String>(3))?
        .collect::<Result<Vec<_>, _>>()?;
    assert!(
        plans
            .iter()
            .any(|plan| plan.contains("idx_request_logs_app_created_at")
                && plan.contains("app_type=?")),
        "{plans:?}"
    );
    let plan: String = conn.query_row("EXPLAIN QUERY PLAN SELECT input_cost_per_million FROM model_pricing WHERE model_id = ? COLLATE NOCASE", ["GPT-5"], |row| row.get(3))?;
    assert!(plan.contains("idx_model_pricing_nocase"), "{plan}");
    Ok(())
}

#[test]
fn pricing_snapshot_matches_sql_and_refreshes_after_mutations() -> Result<(), AppError> {
    let db = Database::memory()?;
    let conn = lock_conn!(db.conn);
    conn.execute("INSERT INTO model_pricing (model_id, display_name, input_cost_per_million, output_cost_per_million) VALUES ('gpt-under_score-a', 'test', '1', '2'), ('gpt-underXscore-b', 'test', '3', '4'), ('gpt-50%-a', 'test', '5', '6'), ('gpt-500-a', 'test', '7', '8')", [])?;
    // Preserve the original rowid tie break when a user has case variants.
    conn.execute("INSERT INTO model_pricing (model_id, display_name, input_cost_per_million, output_cost_per_million) VALUES ('gpt-CASE-a', 'test', '11', '12'), ('GPT-case-A', 'test', '13', '14')", [])?;
    let catalog = PricingCatalog::load(&conn)?;
    assert_eq!(catalog.exact("gpt-case-a")?.unwrap().0, "11");
    assert_eq!(
        query_model_pricing_exact(&conn, "gpt-case-a")?.unwrap().0,
        "11"
    );
    let models = {
        let mut stmt = conn.prepare("SELECT model_id FROM model_pricing")?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        rows.collect::<Result<Vec<_>, _>>()?
    };
    for model in models {
        for candidate in [
            model.clone(),
            model.to_ascii_uppercase(),
            format!("{model}-high"),
            format!("models/{model}"),
        ] {
            assert_eq!(
                lookup_model_pricing(&catalog, &candidate)?,
                find_model_pricing_row(&conn, &candidate)?,
                "{candidate}"
            );
        }
    }
    for (base, expected) in [("gpt-under_score", "1"), ("gpt-50%", "5")] {
        let row = query_model_pricing_prefix(&conn, base)?.unwrap();
        assert_eq!(row.0, expected);
        assert_eq!(catalog.prefix(base)?, Some(row));
    }
    conn.execute(
        "UPDATE model_pricing SET input_cost_per_million='9' WHERE model_id='gpt-under_score-a'",
        [],
    )?;
    assert_eq!(catalog.exact("GPT-UNDER_SCORE-A")?.unwrap().0, "1");
    assert_eq!(
        PricingCatalog::load(&conn)?
            .exact("GPT-UNDER_SCORE-A")?
            .unwrap()
            .0,
        "9"
    );
    conn.execute_batch("BEGIN; DELETE FROM model_pricing; ROLLBACK;")?;
    assert_eq!(
        PricingCatalog::load(&conn)?
            .exact("GPT-UNDER_SCORE-A")?
            .unwrap()
            .0,
        "9"
    );
    Ok(())
}
