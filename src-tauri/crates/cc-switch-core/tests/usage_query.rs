use cc_switch_core::{HeadlessState, LogFilters, UsageScope, UsageService};
use rusqlite::params;

#[test]
fn usage_queries_match_dashboard_semantics() {
    let home = tempfile::tempdir().expect("创建临时 HOME");
    let state = HeadlessState::memory(home.path()).expect("创建 canonical Usage 数据库");
    seed_usage_fixture(&state);
    let scope = UsageScope::all();

    let summary = UsageService::summary(&state, scope.clone()).expect("查询汇总");
    assert_eq!(summary.total_requests, 3);
    assert_eq!(summary.total_cost, "0.042000");
    assert_eq!(summary.total_input_tokens, 840);
    assert_eq!(summary.real_total_tokens, 1_500);
    assert!((summary.success_rate - 66.666_664).abs() < 0.001);

    let by_app = UsageService::summary_by_app(&state, scope.clone()).expect("按应用汇总");
    assert_eq!(by_app.len(), 3);
    assert_eq!(by_app[0].app_type, "codex");
    assert_eq!(by_app[0].summary.real_total_tokens, 1_100);

    let trends = UsageService::trends(
        &state,
        UsageScope {
            start_date: Some(1_699_999_900),
            end_date: Some(1_700_000_300),
            ..UsageScope::all()
        },
    )
    .expect("趋势");
    assert_eq!(trends.iter().map(|item| item.request_count).sum::<u64>(), 3);
    assert_eq!(
        trends.iter().map(|item| item.total_tokens).sum::<u64>(),
        1_010
    );

    let providers = UsageService::provider_stats(&state, scope.clone()).expect("Provider 统计");
    assert_eq!(providers[0].provider_name, "Codex (Session)");
    assert!(providers
        .iter()
        .any(|item| item.provider_name == "Configured Claude"));

    let models = UsageService::model_stats(&state, scope).expect("模型统计");
    assert_eq!(models.len(), 3);
    assert_eq!(models[0].model, "gpt-5");
    assert_eq!(models[0].total_cost, "0.020000");

    let logs = UsageService::logs(&state, LogFilters::default(), 0, 2).expect("分页日志");
    assert_eq!(logs.total, 3);
    assert_eq!(logs.data.len(), 2);
    assert_eq!(logs.data[0].request_id, "codex-session");
    assert_eq!(
        logs.data[0].provider_name.as_deref(),
        Some("Codex (Session)")
    );

    let detail = UsageService::detail(&state, "gemini-session")
        .expect("日志详情")
        .expect("详情存在");
    assert_eq!(detail.provider_name.as_deref(), Some("Gemini (Session)"));
    assert_eq!(detail.data_source.as_deref(), Some("gemini_session"));

    let sources = UsageService::data_sources(&state).expect("数据来源");
    assert_eq!(sources.len(), 3);
    assert!(sources
        .iter()
        .any(|item| item.data_source == "codex_session" && item.total_cost_usd == "0.020000"));

    let pricing = UsageService::pricing(&state).expect("模型定价");
    assert_eq!(pricing.len(), 1);
    assert_eq!(pricing[0].model_id, "gpt-5");
}

#[test]
fn usage_scope_and_log_filters_apply_provider_name_fallback() {
    let home = tempfile::tempdir().expect("创建临时 HOME");
    let state = HeadlessState::memory(home.path()).expect("创建 canonical Usage 数据库");
    seed_usage_fixture(&state);

    let scope = UsageScope {
        provider_name: Some("Codex (Session)".to_string()),
        ..UsageScope::all()
    };
    assert_eq!(
        UsageService::summary(&state, scope)
            .expect("筛选汇总")
            .total_requests,
        1
    );

    let logs = UsageService::logs(
        &state,
        LogFilters {
            provider_name: Some("Gemini (Session)".to_string()),
            ..LogFilters::default()
        },
        0,
        20,
    )
    .expect("筛选日志");
    assert_eq!(logs.total, 1);
    assert_eq!(logs.data[0].request_id, "gemini-session");
}

#[test]
fn usage_queries_deduplicate_matching_session_and_proxy_rows() {
    let home = tempfile::tempdir().expect("创建临时 HOME");
    let state = HeadlessState::memory(home.path()).expect("创建 canonical Usage 数据库");
    state
        .with_connection(|connection| {
            insert_log(
                connection,
                "proxy-copy",
                "codex-provider",
                "codex",
                "gpt-5",
                1_000,
                100,
                300,
                100,
                1,
                "0.020000",
                200,
                1_700_000_200,
                "proxy",
            )?;
            insert_log(
                connection,
                "session-copy",
                "_codex_session",
                "codex",
                "gpt-5",
                1_000,
                100,
                300,
                0,
                1,
                "0.020000",
                200,
                1_700_000_210,
                "codex_session",
            )?;
            Ok(())
        })
        .expect("写入跨源重复 fixture");

    let summary = UsageService::summary(&state, UsageScope::all()).expect("去重汇总");
    assert_eq!(summary.total_requests, 1);
    let logs = UsageService::logs(&state, LogFilters::default(), 0, 20).expect("去重日志");
    assert_eq!(logs.total, 1);
    assert_eq!(logs.data[0].request_id, "proxy-copy");
}

#[test]
fn usage_aggregates_include_historical_daily_rollups() {
    let home = tempfile::tempdir().expect("创建临时 HOME");
    let state = HeadlessState::memory(home.path()).expect("创建 canonical Usage 数据库");
    state
        .with_connection(|connection| {
            connection.execute(
                "INSERT INTO providers (
                    id, app_type, name, settings_config, meta, is_current, in_failover_queue
                 ) VALUES ('historical', 'claude', 'Historical Claude', '{}', '{}', 1, 0)",
                [],
            )?;
            connection.execute(
                "INSERT INTO usage_daily_rollups (
                    date, app_type, provider_id, model, request_model, pricing_model,
                    request_count, success_count, input_tokens, output_tokens,
                    cache_read_tokens, cache_creation_tokens, input_token_semantics,
                    total_cost_usd, avg_latency_ms
                 ) VALUES (
                    '2023-11-13', 'claude', 'historical', 'claude-old', '', '',
                    2, 2, 300, 100, 40, 20, 2, '0.030000', 250
                 )",
                [],
            )?;
            Ok(())
        })
        .expect("写入 rollup fixture");

    let scope = UsageScope::all();
    let summary = UsageService::summary(&state, scope.clone()).expect("rollup 汇总");
    assert_eq!(summary.total_requests, 2);
    assert_eq!(summary.real_total_tokens, 460);
    assert_eq!(summary.total_cost, "0.030000");
    assert_eq!(
        UsageService::summary_by_app(&state, scope.clone()).expect("按应用")[0].app_type,
        "claude"
    );
    assert_eq!(
        UsageService::trends(
            &state,
            UsageScope {
                start_date: Some(1_699_747_200),
                end_date: Some(1_700_006_340),
                ..UsageScope::all()
            },
        )
        .expect("趋势")
        .iter()
        .find(|item| item.request_count > 0)
        .expect("历史 rollup 趋势桶")
        .request_count,
        2
    );
    assert_eq!(
        UsageService::provider_stats(&state, scope.clone()).expect("Provider")[0].provider_name,
        "Historical Claude"
    );
    assert_eq!(
        UsageService::model_stats(&state, scope).expect("模型")[0].model,
        "claude-old"
    );
}

#[test]
fn usage_service_accepts_a_borrowed_desktop_connection() {
    let home = tempfile::tempdir().expect("创建临时 HOME");
    let state = HeadlessState::memory(home.path()).expect("创建 canonical Usage 数据库");
    seed_usage_fixture(&state);

    // 桌面适配层已经持有自己的数据库锁；Core 必须借用该连接，不能另开连接绕过锁与事务边界。
    let total_requests = state
        .with_connection(|connection| {
            Ok(UsageService::summary(connection, UsageScope::all())?.total_requests)
        })
        .expect("通过桌面连接查询 Usage");

    assert_eq!(total_requests, 3);
}

#[test]
fn provider_stats_keep_same_provider_id_isolated_by_app() {
    let home = tempfile::tempdir().expect("创建临时 HOME");
    let state = HeadlessState::memory(home.path()).expect("创建 canonical Usage 数据库");
    state
        .with_connection(|connection| {
            connection.execute_batch(
                "INSERT INTO providers (
                    id, app_type, name, settings_config, meta, is_current, in_failover_queue
                 ) VALUES
                    ('shared', 'claude', 'Shared Claude', '{}', '{}', 1, 0),
                    ('shared', 'codex', 'Shared Codex', '{}', '{}', 1, 0);
                 INSERT INTO usage_daily_rollups (
                    date, app_type, provider_id, model, request_count, success_count,
                    input_tokens, output_tokens, total_cost_usd, avg_latency_ms
                 ) VALUES
                    ('2023-11-13', 'claude', 'shared', 'claude-old', 2, 2, 200, 20, '0.020000', 100),
                    ('2023-11-13', 'codex', 'shared', 'gpt-old', 3, 3, 300, 30, '0.030000', 200);",
            )?;
            insert_log(
                connection,
                "claude-current",
                "shared",
                "claude",
                "claude-new",
                100,
                10,
                0,
                0,
                2,
                "0.010000",
                200,
                1_700_000_000,
                "proxy",
            )?;
            insert_log(
                connection,
                "codex-current",
                "shared",
                "codex",
                "gpt-new",
                100,
                10,
                0,
                0,
                2,
                "0.010000",
                200,
                1_700_000_001,
                "proxy",
            )?;
            Ok(())
        })
        .expect("写入跨应用同 ID fixture");

    // DTO 不暴露 app_type，因此名称与请求数必须共同证明内部合并键没有跨应用串行。
    let stats = UsageService::provider_stats(&state, UsageScope::all()).expect("Provider 统计");
    let claude = stats
        .iter()
        .find(|item| item.provider_name == "Shared Claude")
        .expect("Claude Provider 行");
    let codex = stats
        .iter()
        .find(|item| item.provider_name == "Shared Codex")
        .expect("Codex Provider 行");
    assert_eq!(claude.request_count, 3);
    assert_eq!(codex.request_count, 4);
}

/// fixture 显式写入三种 input token 语义，防止聚合层再次把缓存 token 重复计入 fresh input。
fn seed_usage_fixture(state: &HeadlessState) {
    state
        .with_connection(|connection| {
            connection.execute(
                "INSERT INTO providers (
                    id, app_type, name, settings_config, meta, is_current, in_failover_queue
                 ) VALUES ('claude-a', 'claude', 'Configured Claude', '{}', '{}', 1, 0)",
                [],
            )?;
            connection.execute(
                "INSERT INTO model_pricing (
                    model_id, display_name, input_cost_per_million, output_cost_per_million,
                    cache_read_cost_per_million, cache_creation_cost_per_million
                 ) VALUES ('gpt-5', 'GPT-5', '1', '2', '0.1', '1.25')",
                [],
            )?;
            insert_log(
                connection,
                "claude-proxy",
                "claude-a",
                "claude",
                "claude-sonnet",
                100,
                50,
                20,
                10,
                2,
                "0.010000",
                200,
                1_700_000_000,
                "proxy",
            )?;
            insert_log(
                connection,
                "gemini-session",
                "_gemini_session",
                "gemini",
                "gemini-pro",
                200,
                20,
                50,
                10,
                1,
                "0.012000",
                500,
                1_700_000_100,
                "gemini_session",
            )?;
            insert_log(
                connection,
                "codex-session",
                "_codex_session",
                "codex",
                "gpt-5",
                1_000,
                100,
                300,
                100,
                1,
                "0.020000",
                200,
                1_700_000_200,
                "codex_session",
            )?;
            Ok(())
        })
        .expect("写入 Usage fixture");
}

#[allow(clippy::too_many_arguments)]
fn insert_log(
    connection: &rusqlite::Connection,
    request_id: &str,
    provider_id: &str,
    app_type: &str,
    model: &str,
    input_tokens: i64,
    output_tokens: i64,
    cache_read_tokens: i64,
    cache_creation_tokens: i64,
    input_token_semantics: i64,
    total_cost: &str,
    status_code: i64,
    created_at: i64,
    data_source: &str,
) -> Result<(), rusqlite::Error> {
    connection.execute(
        "INSERT INTO proxy_request_logs (
            request_id, provider_id, app_type, model, request_model, pricing_model,
            input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
            input_token_semantics, input_cost_usd, output_cost_usd,
            cache_read_cost_usd, cache_creation_cost_usd, total_cost_usd,
            latency_ms, first_token_ms, duration_ms, status_code, error_message,
            session_id, provider_type, is_streaming, cost_multiplier, created_at, data_source
         ) VALUES (
            ?1, ?2, ?3, ?4, ?4, ?4, ?5, ?6, ?7, ?8, ?9,
            '0', '0', '0', '0', ?10, 100, 25, 120, ?11, NULL,
            NULL, NULL, 0, '1.0', ?12, ?13
         )",
        params![
            request_id,
            provider_id,
            app_type,
            model,
            input_tokens,
            output_tokens,
            cache_read_tokens,
            cache_creation_tokens,
            input_token_semantics,
            total_cost,
            status_code,
            created_at,
            data_source,
        ],
    )?;
    Ok(())
}
