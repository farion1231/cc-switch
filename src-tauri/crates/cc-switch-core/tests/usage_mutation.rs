use std::io::{Read, Write};
use std::net::TcpListener;

use cc_switch_core::{
    dispatch_command, HeadlessState, OperationCancellation, PricingUpdate, ProviderUsageInput,
    UsageService,
};
use rusqlite::params;

#[test]
fn negative_pricing_is_rejected_before_database_write() {
    let home = tempfile::tempdir().expect("创建临时 HOME");
    let state = HeadlessState::memory(home.path()).expect("创建 canonical Usage 数据库");
    let error = UsageService::update_pricing(
        &state,
        PricingUpdate {
            model_id: "invalid-price".to_string(),
            display_name: "Invalid Price".to_string(),
            input_cost: "-1".to_string(),
            output_cost: "2".to_string(),
            cache_read_cost: "0".to_string(),
            cache_creation_cost: "0".to_string(),
        },
    )
    .expect_err("负价格必须被拒绝");
    assert_eq!(error.code(), "INVALID_ARGUMENT");

    let count = state
        .with_connection(|connection| {
            Ok(connection.query_row(
                "SELECT COUNT(*) FROM model_pricing WHERE model_id = 'invalid-price'",
                [],
                |row| row.get::<_, i64>(0),
            )?)
        })
        .expect("检查定价未落库");
    assert_eq!(count, 0);
}

#[test]
fn pricing_update_backfills_persisted_pricing_model_and_can_be_deleted() {
    let home = tempfile::tempdir().expect("创建临时 HOME");
    let state = HeadlessState::memory(home.path()).expect("创建 canonical Usage 数据库");
    state
        .with_connection(|connection| {
            connection.execute(
                "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model, request_model, pricing_model,
                    input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                    input_token_semantics, total_cost_usd, cost_multiplier,
                    latency_ms, status_code, created_at, data_source
                 ) VALUES (
                    'needs-backfill', 'provider-a', 'claude', 'client-alias', 'client-alias',
                    'priced-upstream', 1000000, 0, 0, 0, 2, '0', '1.5',
                    100, 200, 1700000000, 'proxy'
                 )",
                [],
            )?;
            Ok(())
        })
        .expect("写入零成本历史行");

    UsageService::update_pricing(
        &state,
        PricingUpdate {
            model_id: "priced-upstream".to_string(),
            display_name: "Priced Upstream".to_string(),
            input_cost: "2".to_string(),
            output_cost: "0".to_string(),
            cache_read_cost: "0".to_string(),
            cache_creation_cost: "0".to_string(),
        },
    )
    .expect("更新定价并回填");

    let total_cost = state
        .with_connection(|connection| {
            Ok(connection.query_row(
                "SELECT total_cost_usd FROM proxy_request_logs WHERE request_id = 'needs-backfill'",
                [],
                |row| row.get::<_, String>(0),
            )?)
        })
        .expect("读取回填成本");
    assert_eq!(total_cost, "3.000000");

    UsageService::delete_pricing(&state, "priced-upstream").expect("删除定价");
    assert!(UsageService::pricing(&state)
        .expect("读取定价")
        .iter()
        .all(|item| item.model_id != "priced-upstream"));
}

#[test]
fn pricing_delete_records_target_home_tombstone() {
    let home = tempfile::tempdir().expect("创建临时目标 HOME");
    let state = HeadlessState::open(home.path()).expect("打开目标 HOME 数据库");

    UsageService::update_pricing(
        &state,
        PricingUpdate {
            model_id: "deleted-remote-model".to_string(),
            display_name: "Deleted Remote Model".to_string(),
            input_cost: "1".to_string(),
            output_cost: "2".to_string(),
            cache_read_cost: "0".to_string(),
            cache_creation_cost: "0".to_string(),
        },
    )
    .expect("写入目标主机定价覆盖");
    UsageService::delete_pricing(&state, "deleted-remote-model").expect("删除目标主机定价");

    let file: serde_json::Value = serde_json::from_slice(
        &std::fs::read(home.path().join(".cc-switch/model-pricing.json"))
            .expect("读取目标主机定价覆盖文件"),
    )
    .expect("解析目标主机定价覆盖文件");
    assert_eq!(
        file["deletedModelIds"],
        serde_json::json!(["deleted-remote-model"])
    );
    assert_eq!(file["models"], serde_json::json!([]));
}

#[test]
fn pricing_backfill_normalizes_namespaces_dates_free_and_reasoning_suffixes() {
    let home = tempfile::tempdir().expect("创建临时 HOME");
    let state = HeadlessState::memory(home.path()).expect("创建 canonical Usage 数据库");
    state
        .with_connection(|connection| {
            for (request_id, model) in [
                (
                    "namespaced-model",
                    "openrouter/anthropic/claude-sonnet-4-5-20250929:free",
                ),
                ("reasoning-model", "gpt-5-2025-07-30-high"),
            ] {
                connection.execute(
                    "INSERT INTO proxy_request_logs (
                        request_id, provider_id, app_type, model, request_model,
                        input_tokens, total_cost_usd, latency_ms, status_code,
                        created_at, data_source
                     ) VALUES (?1, 'provider-a', 'codex', ?2, ?2,
                               1000000, '0', 1, 200, 1700000000, 'proxy')",
                    params![request_id, model],
                )?;
            }
            Ok(())
        })
        .expect("写入模型别名历史行");

    UsageService::update_pricing(
        &state,
        PricingUpdate {
            model_id: "claude-sonnet-4-5".to_string(),
            display_name: "Claude Sonnet 4.5".to_string(),
            input_cost: "2".to_string(),
            output_cost: "0".to_string(),
            cache_read_cost: "0".to_string(),
            cache_creation_cost: "0".to_string(),
        },
    )
    .expect("写入 Claude 定价");
    UsageService::update_pricing(
        &state,
        PricingUpdate {
            model_id: "gpt-5".to_string(),
            display_name: "GPT-5".to_string(),
            input_cost: "3".to_string(),
            output_cost: "0".to_string(),
            cache_read_cost: "0".to_string(),
            cache_creation_cost: "0".to_string(),
        },
    )
    .expect("写入 GPT 定价");

    let costs = state
        .with_connection(|connection| {
            let mut statement = connection.prepare(
                "SELECT request_id, total_cost_usd FROM proxy_request_logs ORDER BY request_id",
            )?;
            let rows = statement.query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
        })
        .expect("读取归一化回填结果");
    assert_eq!(
        costs,
        vec![
            ("namespaced-model".to_string(), "2.000000".to_string()),
            ("reasoning-model".to_string(), "3.000000".to_string()),
        ]
    );
}

#[test]
fn provider_limits_include_current_detail_and_rollup_costs() {
    let home = tempfile::tempdir().expect("创建临时 HOME");
    let state = HeadlessState::memory(home.path()).expect("创建 canonical Usage 数据库");
    state
        .with_connection(|connection| {
            connection.execute_batch(
                "INSERT INTO providers (
                    id, app_type, name, settings_config, meta, is_current, in_failover_queue
                 ) VALUES (
                    'limited', 'claude', 'Limited', '{}',
                    '{\"limitDailyUsd\":\"2.50\",\"limitMonthlyUsd\":\"10\"}', 1, 0
                 );
                 INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model, total_cost_usd,
                    latency_ms, status_code, created_at, data_source
                 ) VALUES (
                    'today-detail', 'limited', 'claude', 'model-a', '1.25',
                    100, 200, CAST(strftime('%s', 'now') AS INTEGER), 'proxy'
                 );
                 INSERT INTO usage_daily_rollups (
                    date, app_type, provider_id, model, request_count, success_count,
                    total_cost_usd, avg_latency_ms
                 ) VALUES (
                    date('now', 'localtime'), 'claude', 'limited', 'model-a', 1, 1, '1.75', 100
                 );",
            )?;
            Ok(())
        })
        .expect("写入额度 fixture");

    let status = UsageService::limits(&state, "limited", "claude").expect("查询额度");
    assert_eq!(status.daily_usage, "3.000000");
    assert_eq!(status.monthly_usage, "3.000000");
    assert_eq!(status.daily_limit.as_deref(), Some("2.50"));
    assert!(status.daily_exceeded);
    assert!(!status.monthly_exceeded);
}

#[test]
fn saved_provider_usage_script_executes_against_target_database_configuration() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("绑定本地 Usage fixture");
    let address = listener.local_addr().expect("读取 fixture 地址");
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("接收 Usage 请求");
        let mut request = [0u8; 4096];
        let length = stream.read(&mut request).expect("读取 Usage 请求");
        let request = String::from_utf8_lossy(&request[..length]);
        assert!(request
            .to_ascii_lowercase()
            .contains("x-usage-key: script-key"));
        let body = r#"{"plan":"Remote Plan","remaining":42.5}"#;
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        )
        .expect("返回 Usage 响应");
    });

    let home = tempfile::tempdir().expect("创建临时 HOME");
    let state = HeadlessState::memory(home.path()).expect("创建 canonical Usage 数据库");
    let base_url = format!("http://{address}");
    let settings = serde_json::json!({
        "env": {
            "ANTHROPIC_AUTH_TOKEN": "provider-key",
            "ANTHROPIC_BASE_URL": base_url
        }
    });
    let meta = serde_json::json!({
        "usageScript": {
            "enabled": true,
            "language": "javascript",
            "code": "({ request: { url: '{{baseUrl}}/usage', method: 'GET', headers: { 'X-Usage-Key': '{{apiKey}}' } }, extractor: function(response) { return { planName: response.plan, remaining: response.remaining, unit: 'USD' }; } })",
            "timeout": 5,
            "apiKey": "script-key"
        }
    });
    state
        .with_connection(|connection| {
            connection.execute(
                "INSERT INTO providers (
                    id, app_type, name, settings_config, meta, is_current, in_failover_queue
                 ) VALUES (?1, 'claude', 'Scripted', ?2, ?3, 1, 0)",
                params!["scripted", settings.to_string(), meta.to_string()],
            )?;
            Ok(())
        })
        .expect("写入脚本 Provider");

    let result = UsageService::provider_query(
        &state,
        ProviderUsageInput {
            provider_id: "scripted".to_string(),
            app_type: "claude".to_string(),
        },
    )
    .expect("执行远端 Provider Usage 脚本");
    assert!(result.success);
    let data = result.data.expect("Usage 数据");
    assert_eq!(data[0].plan_name.as_deref(), Some("Remote Plan"));
    assert_eq!(data[0].remaining, Some(42.5));
    server.join().expect("等待本地 Usage fixture");
}

#[test]
fn provider_usage_rpc_reaches_core_script_service() {
    let home = tempfile::tempdir().expect("创建临时 HOME");
    let state = HeadlessState::memory(home.path()).expect("创建 canonical Usage 数据库");
    state
        .with_connection(|connection| {
            connection.execute(
                "INSERT INTO providers (
                    id, app_type, name, settings_config, meta, is_current, in_failover_queue
                 ) VALUES ('no-script', 'claude', 'No Script', '{}', '{}', 1, 0)",
                [],
            )?;
            Ok(())
        })
        .expect("写入无脚本 Provider");

    let error = dispatch_command(
        &state,
        "usage.provider_query",
        serde_json::json!({ "providerId": "no-script", "appType": "claude" }),
    )
    .expect_err("未配置脚本必须返回业务错误");
    assert_eq!(error.code(), "REMOTE_BUSINESS_ERROR");
}

#[test]
fn cancelled_codex_rebuild_stops_before_reset() {
    let home = tempfile::tempdir().expect("创建临时 HOME");
    let state = HeadlessState::open(home.path()).expect("创建磁盘 Usage 数据库");
    seed_codex_usage(&state);
    let cancellation = OperationCancellation::cancelled();

    let error = UsageService::rebuild_codex(&state, &cancellation).expect_err("取消的重建不能继续");
    assert_eq!(error.code(), "REMOTE_OPERATION_CANCELLED");
    assert_eq!(codex_usage_count(&state), 1);
}

#[test]
fn codex_rebuild_preserves_proxy_rows_and_non_session_rollups() {
    let home = tempfile::tempdir().expect("创建临时 HOME");
    let state = HeadlessState::open(home.path()).expect("创建磁盘 Usage 数据库");
    state
        .with_connection(|connection| {
            connection.execute_batch(
                "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model, total_cost_usd,
                    latency_ms, status_code, created_at, data_source
                 ) VALUES
                    ('codex-session', '_codex_session', 'codex', 'gpt-5', '1', 1, 200, 1, 'codex_session'),
                    ('codex-proxy', 'provider-a', 'codex', 'gpt-5', '2', 1, 200, 2, 'proxy'),
                    ('claude-session', '_claude_session', 'claude', 'claude', '3', 1, 200, 3, 'claude_session');
                 INSERT INTO usage_daily_rollups (
                    date, app_type, provider_id, model, total_cost_usd
                 ) VALUES
                    ('2026-07-29', 'codex', '_codex_session', 'gpt-5', '1'),
                    ('2026-07-29', 'codex', 'provider-a', 'gpt-5', '2'),
                    ('2026-07-29', 'claude', '_claude_session', 'claude', '3');",
            )?;
            Ok(())
        })
        .expect("写入重建隔离 fixture");

    UsageService::rebuild_codex(&state, &OperationCancellation::active())
        .expect("执行无会话 Codex 重建");
    let remaining = state
        .with_connection(|connection| {
            let details = connection.query_row(
                "SELECT GROUP_CONCAT(request_id, ',') FROM (
                    SELECT request_id FROM proxy_request_logs ORDER BY request_id
                 )",
                [],
                |row| row.get::<_, String>(0),
            )?;
            let rollups = connection.query_row(
                "SELECT GROUP_CONCAT(provider_id, ',') FROM (
                    SELECT provider_id FROM usage_daily_rollups ORDER BY provider_id
                 )",
                [],
                |row| row.get::<_, String>(0),
            )?;
            Ok((details, rollups))
        })
        .expect("读取重建后数据");
    assert_eq!(remaining.0, "claude-session,codex-proxy");
    assert_eq!(remaining.1, "_claude_session,provider-a");
}

#[test]
fn codex_rebuild_keeps_backup_when_import_fails_after_reset() {
    let home = tempfile::tempdir().expect("创建临时 HOME");
    let state = HeadlessState::open(home.path()).expect("创建磁盘 Usage 数据库");
    seed_codex_usage(&state);
    let sessions_path = home.path().join(".codex").join("sessions");
    std::fs::create_dir_all(sessions_path.parent().expect("sessions 父目录"))
        .expect("创建 Codex 目录");
    std::fs::write(&sessions_path, b"not-a-directory").expect("制造导入失败");

    let error = UsageService::rebuild_codex(&state, &OperationCancellation::active())
        .expect_err("导入失败必须上报");
    assert_eq!(error.code(), "REMOTE_BUSINESS_ERROR");
    let backup_count = std::fs::read_dir(home.path().join(".cc-switch"))
        .expect("读取备份目录")
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with("cc-switch.db.codex-rebuild-")
        })
        .count();
    assert_eq!(backup_count, 1);
}

#[test]
fn session_sync_imports_codex_usage_from_explicit_target_home() {
    let home = tempfile::tempdir().expect("创建临时 HOME");
    let session_dir = home
        .path()
        .join(".codex")
        .join("sessions")
        .join("2026")
        .join("07")
        .join("30");
    std::fs::create_dir_all(&session_dir).expect("创建 Codex sessions");
    let events = [
        serde_json::json!({
            "timestamp": "2026-07-30T10:00:00Z",
            "type": "session_meta",
            "payload": { "id": "remote-thread" }
        }),
        serde_json::json!({
            "timestamp": "2026-07-30T10:00:01Z",
            "type": "turn_context",
            "payload": { "model": "gpt-5.6-sol" }
        }),
        serde_json::json!({
            "timestamp": "2026-07-30T10:00:02Z",
            "type": "event_msg",
            "payload": { "type": "token_count", "info": { "total_token_usage": {
                "input_tokens": 100, "cached_input_tokens": 40, "output_tokens": 10
            }}}
        }),
        serde_json::json!({
            "timestamp": "2026-07-30T10:00:03Z",
            "type": "event_msg",
            "payload": { "type": "token_count", "info": { "total_token_usage": {
                "input_tokens": 250, "cached_input_tokens": 100, "output_tokens": 30
            }}}
        }),
    ];
    let body = events
        .iter()
        .map(serde_json::Value::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(session_dir.join("rollout.jsonl"), body).expect("写入 Codex JSONL");

    let state = HeadlessState::open(home.path()).expect("打开目标 HOME 数据库");
    let result = UsageService::sync_sessions(&state, &OperationCancellation::active())
        .expect("同步目标 HOME 会话");
    assert_eq!(result.files_scanned, 1);
    assert_eq!(result.imported, 2);
    assert_eq!(codex_usage_count(&state), 2);

    let unchanged = UsageService::sync_sessions(&state, &OperationCancellation::active())
        .expect("再次同步未变化 Codex rollout");
    assert_eq!(unchanged.files_scanned, 1);
    assert_eq!(unchanged.imported, 0);
    assert_eq!(unchanged.skipped, 0);
}

#[test]
fn codex_sync_excludes_parent_history_replayed_by_forked_rollout() {
    const PARENT: &str = "00000000-0000-4000-8000-000000000001";
    const CHILD: &str = "00000000-0000-4000-8000-000000000002";
    let home = tempfile::tempdir().expect("创建临时 HOME");
    let sessions = home.path().join(".codex/sessions/2026/07/30");
    std::fs::create_dir_all(&sessions).expect("创建 Codex sessions");

    let meta = |id: &str, parent: Option<&str>, timestamp: &str| {
        serde_json::json!({
            "timestamp": timestamp,
            "type": "session_meta",
            "payload": {
                "id": id,
                "forked_from_id": parent,
                "source": "cli"
            }
        })
    };
    let token = |input: u64, cached: u64, output: u64, timestamp: &str| {
        serde_json::json!({
            "timestamp": timestamp,
            "type": "event_msg",
            "payload": { "type": "token_count", "info": { "total_token_usage": {
                "input_tokens": input,
                "cached_input_tokens": cached,
                "output_tokens": output
            }}}
        })
    };
    let write_jsonl = |name: &str, values: Vec<serde_json::Value>| {
        std::fs::write(
            sessions.join(name),
            values
                .into_iter()
                .map(|value| value.to_string())
                .collect::<Vec<_>>()
                .join("\n"),
        )
        .expect("写入 Codex rollout");
    };
    write_jsonl(
        &format!("rollout-2026-07-30T10-00-00-{PARENT}.jsonl"),
        vec![
            meta(PARENT, None, "2026-07-30T10:00:00Z"),
            serde_json::json!({
                "timestamp": "2026-07-30T10:00:00Z",
                "type": "turn_context",
                "payload": { "model": "gpt-5" }
            }),
            token(1_000, 900, 100, "2026-07-30T10:00:01Z"),
        ],
    );
    write_jsonl(
        &format!("rollout-2026-07-30T10-00-05-{CHILD}.jsonl"),
        vec![
            meta(CHILD, Some(PARENT), "2026-07-30T10:00:05Z"),
            serde_json::json!({
                "timestamp": "2026-07-30T10:00:05Z",
                "type": "turn_context",
                "payload": { "model": "gpt-5" }
            }),
            token(1_000, 900, 100, "2026-07-30T10:00:06Z"),
            token(1_300, 1_050, 150, "2026-07-30T10:00:07Z"),
        ],
    );

    let state = HeadlessState::open(home.path()).expect("打开目标 HOME 数据库");
    let result = UsageService::sync_sessions(&state, &OperationCancellation::active())
        .expect("同步 fork rollout");
    assert_eq!(result.imported, 2);
    assert_eq!(result.skipped, 1);
    let totals = state
        .with_connection(|connection| {
            Ok(connection.query_row(
                "SELECT COUNT(*), SUM(input_tokens), SUM(cache_read_tokens), SUM(output_tokens)
                 FROM proxy_request_logs WHERE data_source = 'codex_session'",
                [],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, i64>(3)?,
                    ))
                },
            )?)
        })
        .expect("读取 fork 导入结果");
    assert_eq!(totals, (2, 1_300, 1_050, 150));
}

#[test]
fn codex_sync_skips_usage_already_recorded_by_proxy() {
    let home = tempfile::tempdir().expect("创建临时 HOME");
    let sessions = home.path().join(".codex/sessions/2026/07/20");
    std::fs::create_dir_all(&sessions).expect("创建 Codex sessions");
    let events = [
        serde_json::json!({
            "timestamp": "2026-07-20T10:00:00Z",
            "type": "session_meta",
            "payload": { "id": "codex-session" }
        }),
        serde_json::json!({
            "timestamp": "2026-07-20T10:00:00Z",
            "type": "turn_context",
            "payload": { "model": "gpt-5" }
        }),
        serde_json::json!({
            "timestamp": "2026-07-20T10:00:01Z",
            "type": "event_msg",
            "payload": { "type": "token_count", "info": { "total_token_usage": {
                "input_tokens": 100,
                "cached_input_tokens": 40,
                "output_tokens": 10
            }}}
        }),
    ];
    std::fs::write(
        sessions.join("rollout.jsonl"),
        events
            .iter()
            .map(serde_json::Value::to_string)
            .collect::<Vec<_>>()
            .join("\n"),
    )
    .expect("写入 Codex fixture");
    let state = HeadlessState::open(home.path()).expect("打开目标 HOME 数据库");
    state
        .with_connection(|connection| {
            connection.execute(
                "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model,
                    input_tokens, output_tokens, cache_read_tokens,
                    latency_ms, status_code, created_at, data_source
                 ) VALUES (
                    'proxy-codex', 'provider-a', 'codex', 'gpt-5', 100, 10, 40,
                    1, 200, CAST(strftime('%s', '2026-07-20T10:00:01Z') AS INTEGER), 'proxy'
                 )",
                [],
            )?;
            Ok(())
        })
        .expect("写入 Codex proxy 权威行");

    let result = UsageService::sync_sessions(&state, &OperationCancellation::active())
        .expect("同步 Codex 会话");
    assert_eq!(result.imported, 0);
    assert_eq!(result.skipped, 1);
    assert_eq!(codex_usage_count(&state), 1);
}

#[test]
fn long_usage_commands_are_reachable_through_core_dispatch() {
    let home = tempfile::tempdir().expect("创建临时 HOME");
    let state = HeadlessState::open(home.path()).expect("打开目标 HOME 数据库");

    let sync = dispatch_command(&state, "usage.session_sync", serde_json::json!({}))
        .expect("Agent 必须能调用会话同步");
    assert_eq!(sync["imported"], 0);
    assert_eq!(sync["filesScanned"], 0);

    let rebuild = dispatch_command(&state, "usage.codex_rebuild", serde_json::json!({}))
        .expect("Agent 必须能调用 Codex 重建");
    assert_eq!(rebuild["imported"], 0);
    assert_eq!(rebuild["filesScanned"], 0);
    assert!(home
        .path()
        .join(".cc-switch")
        .read_dir()
        .expect("读取备份目录")
        .flatten()
        .any(|entry| entry
            .file_name()
            .to_string_lossy()
            .contains("codex-rebuild")));
}

#[test]
fn models_dev_commands_persist_inside_explicit_target_home() {
    let home = tempfile::tempdir().expect("创建临时目标 HOME");
    let state = HeadlessState::open(home.path()).expect("打开目标 HOME 数据库");

    let initial = dispatch_command(&state, "usage.models_dev_sync.get", serde_json::json!({}))
        .expect("读取 models.dev 初始配置");
    assert_eq!(initial["config"]["autoSyncEnabled"], false);
    assert_eq!(
        initial["configPath"],
        home.path()
            .join(".cc-switch")
            .join("model-pricing.json")
            .display()
            .to_string()
    );

    dispatch_command(
        &state,
        "usage.models_dev_sync.save",
        serde_json::json!({
            "config": {
                "autoSyncEnabled": true,
                "includeCommonModels": false,
                "selectedModelKeys": ["openai:gpt-5", "openai:gpt-5"],
                "excludedCommonModelKeys": [],
                "lastSyncAt": null,
                "lastSyncError": null
            }
        }),
    )
    .expect("保存目标主机 models.dev 配置");
    dispatch_command(
        &state,
        "usage.models_dev_sync.record",
        serde_json::json!({ "syncedAt": 123, "error": null }),
    )
    .expect("记录目标主机 models.dev 同步结果");

    let changed = dispatch_command(
        &state,
        "usage.pricing.update_batch",
        serde_json::json!({
            "entries": [{
                "modelId": "remote-batch-model",
                "displayName": "Remote Batch Model",
                "inputCostPerMillion": "1",
                "outputCostPerMillion": "2",
                "cacheReadCostPerMillion": "0.1",
                "cacheCreationCostPerMillion": "0.2"
            }]
        }),
    )
    .expect("批量写入目标主机定价");
    assert_eq!(changed, 1);

    let saved = dispatch_command(&state, "usage.models_dev_sync.get", serde_json::json!({}))
        .expect("重新读取目标主机 models.dev 配置");
    assert_eq!(saved["config"]["autoSyncEnabled"], true);
    assert_eq!(
        saved["config"]["selectedModelKeys"],
        serde_json::json!(["openai:gpt-5"])
    );
    assert_eq!(saved["config"]["lastSyncAt"], 123);

    let pricing = dispatch_command(&state, "usage.pricing.list", serde_json::json!({}))
        .expect("读取目标主机定价");
    assert!(pricing
        .as_array()
        .expect("定价列表")
        .iter()
        .any(|item| item["modelId"] == "remote-batch-model"));
    assert!(home
        .path()
        .join(".cc-switch")
        .join("model-pricing.json")
        .is_file());
}

#[test]
fn session_sync_imports_every_supported_cli_from_explicit_target_home() {
    let home = tempfile::tempdir().expect("创建临时 HOME");

    // fixture 固定使用远端 Linux CLI 的默认目录；Core 只能基于显式 HOME 解析，
    // 不能读取运行测试的 Windows 用户目录，否则本地与远端数据会串线。
    let claude_project = home.path().join(".claude/projects/project-a");
    std::fs::create_dir_all(&claude_project).expect("创建 Claude 会话目录");
    std::fs::write(
        claude_project.join("session.jsonl"),
        serde_json::json!({
            "type": "assistant",
            "sessionId": "claude-session",
            "timestamp": "2026-07-30T11:00:00Z",
            "message": {
                "id": "claude-message",
                "model": "claude-sonnet-4-5",
                "stop_reason": "end_turn",
                "usage": {
                    "input_tokens": 10,
                    "output_tokens": 4,
                    "cache_read_input_tokens": 2,
                    "cache_creation_input_tokens": 1
                }
            }
        })
        .to_string(),
    )
    .expect("写入 Claude fixture");

    let gemini_chats = home.path().join(".gemini/tmp/project-a/chats");
    std::fs::create_dir_all(&gemini_chats).expect("创建 Gemini 会话目录");
    std::fs::write(
        gemini_chats.join("session-one.json"),
        serde_json::json!({
            "sessionId": "gemini-session",
            "messages": [{
                "id": "gemini-message",
                "type": "gemini",
                "model": "gemini-2.5-pro",
                "timestamp": "2026-07-30T11:01:00Z",
                "tokens": { "input": 20, "output": 5, "cached": 3, "thoughts": 2 }
            }]
        })
        .to_string(),
    )
    .expect("写入 Gemini fixture");

    let opencode_path = home.path().join(".local/share/opencode/opencode.db");
    std::fs::create_dir_all(opencode_path.parent().expect("OpenCode 数据目录"))
        .expect("创建 OpenCode 数据目录");
    let opencode = rusqlite::Connection::open(&opencode_path).expect("创建 OpenCode fixture");
    opencode
        .execute_batch(
            "CREATE TABLE session (id TEXT PRIMARY KEY, time_updated INTEGER NOT NULL);
             CREATE TABLE message (
                 id TEXT PRIMARY KEY,
                 session_id TEXT NOT NULL,
                 time_created INTEGER NOT NULL,
                 time_updated INTEGER NOT NULL,
                 data TEXT NOT NULL
             );
             INSERT INTO session VALUES ('opencode-session', 1774868580000);",
        )
        .expect("创建 OpenCode schema");
    opencode
        .execute(
            "INSERT INTO message VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                "opencode-message",
                "opencode-session",
                1774868580000_i64,
                1774868581000_i64,
                serde_json::json!({
                    "role": "assistant",
                    "modelID": "gpt-5",
                    "cost": 0.002,
                    "tokens": {
                        "input": 30,
                        "output": 6,
                        "reasoning": 1,
                        "cache": { "read": 4, "write": 2 }
                    },
                    "time": { "created": 1774868580000_i64, "completed": 1774868581000_i64 }
                })
                .to_string(),
            ],
        )
        .expect("写入 OpenCode message");
    drop(opencode);

    let grok_session = home.path().join(".grok/sessions/project-a/session-a");
    std::fs::create_dir_all(&grok_session).expect("创建 Grok Build 会话目录");
    std::fs::write(
        grok_session.join("updates.jsonl"),
        serde_json::json!({
            "timestamp": "2026-07-20T11:03:00Z",
            "method": "_x.ai/session/update",
            "params": { "update": {
                "sessionUpdate": "turn_completed",
                "prompt_id": "grok-prompt",
                "usage": { "modelUsage": { "grok-4.5-build": {
                    "inputTokens": 40,
                    "outputTokens": 7,
                    "cachedReadTokens": 5,
                    "costUsdTicks": 12300000
                }}}
            }}
        })
        .to_string(),
    )
    .expect("写入 Grok Build fixture");

    let state = HeadlessState::open(home.path()).expect("打开目标 HOME 数据库");
    let result = UsageService::sync_sessions(&state, &OperationCancellation::active())
        .expect("同步全部远端 CLI 会话");
    assert_eq!(result.imported, 4);
    assert_eq!(result.files_scanned, 4);

    let sources = state
        .with_connection(|connection| {
            let mut statement = connection.prepare(
                "SELECT data_source, COUNT(*) FROM proxy_request_logs
                 GROUP BY data_source ORDER BY data_source",
            )?;
            let rows = statement.query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
        })
        .expect("读取导入来源");
    assert_eq!(
        sources,
        vec![
            ("claude_session".to_string(), 1),
            ("gemini_session".to_string(), 1),
            ("grok_session".to_string(), 1),
            ("opencode_session".to_string(), 1),
        ]
    );
}

#[test]
fn session_sync_skips_a_claude_row_already_recorded_by_proxy() {
    let home = tempfile::tempdir().expect("创建临时 HOME");
    let project = home.path().join(".claude/projects/project-a");
    std::fs::create_dir_all(&project).expect("创建 Claude 会话目录");
    std::fs::write(
        project.join("session.jsonl"),
        serde_json::json!({
            "type": "assistant",
            "sessionId": "session-a",
            "timestamp": "2026-07-20T10:00:00Z",
            "message": {
                "id": "message-a",
                "model": "claude-sonnet-4-5",
                "usage": { "input_tokens": 10, "output_tokens": 2 }
            }
        })
        .to_string(),
    )
    .expect("写入 Claude fixture");
    let state = HeadlessState::open(home.path()).expect("打开目标 HOME 数据库");
    state
        .with_connection(|connection| {
            connection.execute(
                "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model, input_tokens, output_tokens,
                    latency_ms, status_code, created_at, data_source
                 ) VALUES ('proxy-a', 'provider-a', 'claude', 'claude-sonnet-4-5',
                           10, 2, 1, 200,
                           CAST(strftime('%s', '2026-07-20T10:00:00Z') AS INTEGER), 'proxy')",
                [],
            )?;
            Ok(())
        })
        .expect("写入 proxy 权威行");

    let result = UsageService::sync_sessions(&state, &OperationCancellation::active())
        .expect("同步 Claude 会话");
    assert_eq!(result.imported, 0);
    assert_eq!(result.skipped, 1);
    let count = state
        .with_connection(|connection| {
            Ok(
                connection.query_row("SELECT COUNT(*) FROM proxy_request_logs", [], |row| {
                    row.get::<_, i64>(0)
                })?,
            )
        })
        .expect("读取去重结果");
    assert_eq!(count, 1);
}

#[test]
fn session_sync_short_circuits_an_unchanged_session_file() {
    let home = tempfile::tempdir().expect("创建临时 HOME");
    let project = home.path().join(".claude/projects/project-a");
    std::fs::create_dir_all(&project).expect("创建 Claude 会话目录");
    std::fs::write(
        project.join("session.jsonl"),
        serde_json::json!({
            "type": "assistant",
            "sessionId": "session-a",
            "timestamp": "2026-07-20T10:00:00Z",
            "message": {
                "id": "message-a",
                "model": "claude-sonnet-4-5",
                "usage": { "input_tokens": 10, "output_tokens": 2 }
            }
        })
        .to_string(),
    )
    .expect("写入 Claude fixture");
    let state = HeadlessState::open(home.path()).expect("打开目标 HOME 数据库");
    let first = UsageService::sync_sessions(&state, &OperationCancellation::active())
        .expect("首次同步 Claude 会话");
    assert_eq!(first.imported, 1);

    let second = UsageService::sync_sessions(&state, &OperationCancellation::active())
        .expect("再次同步未变化会话");
    assert_eq!(second.files_scanned, 1);
    assert_eq!(second.imported, 0);
    assert_eq!(second.skipped, 0);
}

#[test]
fn session_sync_defers_recent_grok_turn_until_usage_settles() {
    let home = tempfile::tempdir().expect("创建临时 HOME");
    let session = home.path().join(".grok/sessions/project-a/session-a");
    std::fs::create_dir_all(&session).expect("创建 Grok Build 会话目录");
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("读取当前时间")
        .as_secs() as i64;
    std::fs::write(
        session.join("updates.jsonl"),
        serde_json::json!({
            "timestamp": now,
            "method": "_x.ai/session/update",
            "params": { "update": {
                "sessionUpdate": "turn_completed",
                "prompt_id": "active-turn",
                "usage": { "modelUsage": { "grok-4.5-build": {
                    "inputTokens": 100,
                    "outputTokens": 10
                }}}
            }}
        })
        .to_string(),
    )
    .expect("写入活跃 Grok fixture");
    let state = HeadlessState::open(home.path()).expect("打开目标 HOME 数据库");

    let result = UsageService::sync_sessions(&state, &OperationCancellation::active())
        .expect("同步活跃 Grok 会话");
    assert_eq!(result.imported, 0);
    assert_eq!(result.deferred_files, 1);
    let count = state
        .with_connection(|connection| {
            Ok(
                connection.query_row("SELECT COUNT(*) FROM proxy_request_logs", [], |row| {
                    row.get::<_, i64>(0)
                })?,
            )
        })
        .expect("读取沉降结果");
    assert_eq!(count, 0);
}

#[test]
fn grok_partial_reported_cost_uses_complete_local_pricing_when_available() {
    let home = tempfile::tempdir().expect("创建临时 HOME");
    let session = home.path().join(".grok/sessions/project-a/session-a");
    std::fs::create_dir_all(&session).expect("创建 Grok Build 会话目录");
    std::fs::write(
        session.join("updates.jsonl"),
        serde_json::json!({
            "timestamp": "2026-07-20T10:00:00Z",
            "method": "_x.ai/session/update",
            "params": { "update": {
                "sessionUpdate": "turn_completed",
                "prompt_id": "partial-turn",
                "usage": {
                    "costIsPartial": true,
                    "modelUsage": { "grok-priced": {
                        "inputTokens": 1_000_000,
                        "outputTokens": 0,
                        "costUsdTicks": 1_000,
                        "costIsPartial": true
                    }}
                }
            }}
        })
        .to_string(),
    )
    .expect("写入部分成本 Grok fixture");
    let state = HeadlessState::open(home.path()).expect("打开目标 HOME 数据库");
    UsageService::update_pricing(
        &state,
        PricingUpdate {
            model_id: "grok-priced".to_string(),
            display_name: "Grok Priced".to_string(),
            input_cost: "2".to_string(),
            output_cost: "0".to_string(),
            cache_read_cost: "0".to_string(),
            cache_creation_cost: "0".to_string(),
        },
    )
    .expect("写入 Grok 定价");

    UsageService::sync_sessions(&state, &OperationCancellation::active())
        .expect("同步部分成本 Grok 会话");
    let total = state
        .with_connection(|connection| {
            Ok(connection.query_row(
                "SELECT total_cost_usd FROM proxy_request_logs
                 WHERE data_source = 'grok_session'",
                [],
                |row| row.get::<_, String>(0),
            )?)
        })
        .expect("读取 Grok 完整成本");
    assert_eq!(total, "2.000000");
}

#[test]
fn session_sync_updates_a_completed_gemini_message_when_tokens_change() {
    let home = tempfile::tempdir().expect("创建临时 HOME");
    let chats = home.path().join(".gemini/tmp/project-a/chats");
    std::fs::create_dir_all(&chats).expect("创建 Gemini 会话目录");
    let path = chats.join("session-one.json");
    let write = |input: u64| {
        std::fs::write(
            &path,
            serde_json::json!({
                "sessionId": "gemini-session",
                "messages": [{
                    "id": "gemini-message",
                    "type": "gemini",
                    "model": "gemini-2.5-pro",
                    "timestamp": "2026-07-20T11:00:00Z",
                    "tokens": { "input": input, "output": 5 }
                }]
            })
            .to_string(),
        )
        .expect("写入 Gemini fixture");
    };
    write(20);
    let state = HeadlessState::open(home.path()).expect("打开目标 HOME 数据库");
    UsageService::sync_sessions(&state, &OperationCancellation::active()).expect("首次同步 Gemini");
    write(30);
    UsageService::sync_sessions(&state, &OperationCancellation::active()).expect("再次同步 Gemini");

    let input = state
        .with_connection(|connection| {
            Ok(connection.query_row(
                "SELECT input_tokens FROM proxy_request_logs
                 WHERE request_id = 'gemini_session:gemini-session:gemini-message'",
                [],
                |row| row.get::<_, i64>(0),
            )?)
        })
        .expect("读取 Gemini 更新值");
    assert_eq!(input, 30);
}

fn seed_codex_usage(state: &HeadlessState) {
    state
        .with_connection(|connection| {
            connection.execute(
                "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model, total_cost_usd,
                    latency_ms, status_code, created_at, data_source
                 ) VALUES (
                    'codex-before-rebuild', '_codex_session', 'codex', 'gpt-5', '1',
                    100, 200, 1700000000, 'codex_session'
                 )",
                [],
            )?;
            Ok(())
        })
        .expect("写入 Codex Usage");
}

fn codex_usage_count(state: &HeadlessState) -> i64 {
    state
        .with_connection(|connection| {
            Ok(connection.query_row(
                "SELECT COUNT(*) FROM proxy_request_logs WHERE app_type = 'codex'",
                [],
                |row| row.get(0),
            )?)
        })
        .expect("读取 Codex Usage 数量")
}
