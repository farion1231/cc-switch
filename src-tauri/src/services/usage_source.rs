use crate::database::Database;
use crate::error::AppError;
use crate::proxy::types::UsageStatsSource;
use crate::services::proxy::ProxyService;
use crate::services::session_usage::{sync_claude_session_logs_with_mode, SessionSyncResult};
use crate::services::session_usage_codex::sync_codex_usage_with_mode;
use crate::services::session_usage_gemini::sync_gemini_usage_with_mode;

pub async fn resolve_effective_usage_source(
    db: &Database,
    proxy_running: bool,
    app_type: &str,
) -> Result<UsageStatsSource, AppError> {
    if !proxy_running {
        return Ok(UsageStatsSource::Session);
    }

    let config = db.get_proxy_config_for_app(app_type).await?;
    if config.enabled {
        Ok(config.usage_stats_source)
    } else {
        Ok(UsageStatsSource::Session)
    }
}

pub async fn should_record_proxy_usage(db: &Database, app_type: &str) -> Result<bool, AppError> {
    let config = db.get_proxy_config_for_app(app_type).await?;
    Ok(!(config.enabled && config.usage_stats_source == UsageStatsSource::Session))
}

pub async fn should_record_session_usage(
    db: &Database,
    proxy_running: bool,
    app_type: &str,
) -> Result<bool, AppError> {
    // - 代理未接管时：仍按 Session 日志补录
    // - 代理已接管且 source=proxy：Session 只推进游标，不入库
    // - 代理已接管且 source=session：Session 正常入库
    Ok(
        resolve_effective_usage_source(db, proxy_running, app_type).await?
            == UsageStatsSource::Session,
    )
}

pub async fn sync_session_usage_by_policy(
    db: &Database,
    proxy_service: &ProxyService,
) -> Result<SessionSyncResult, AppError> {
    let proxy_running = proxy_service.is_running().await;
    sync_session_usage_by_policy_with_proxy_running(db, proxy_running).await
}

async fn sync_session_usage_by_policy_with_proxy_running(
    db: &Database,
    proxy_running: bool,
) -> Result<SessionSyncResult, AppError> {
    let mut result = SessionSyncResult {
        imported: 0,
        skipped: 0,
        files_scanned: 0,
        errors: Vec::new(),
    };

    let claude_record_usage = match should_record_session_usage(db, proxy_running, "claude").await {
        Ok(value) => value,
        Err(e) => {
            result.errors.push(format!("Claude 策略解析失败: {e}"));
            true
        }
    };
    merge_sync_result(
        &mut result,
        sync_claude_session_logs_with_mode(db, claude_record_usage),
        "Claude",
    );

    let codex_record_usage = match should_record_session_usage(db, proxy_running, "codex").await {
        Ok(value) => value,
        Err(e) => {
            result.errors.push(format!("Codex 策略解析失败: {e}"));
            true
        }
    };
    merge_sync_result(
        &mut result,
        sync_codex_usage_with_mode(db, codex_record_usage),
        "Codex",
    );

    let gemini_record_usage = match should_record_session_usage(db, proxy_running, "gemini").await {
        Ok(value) => value,
        Err(e) => {
            result.errors.push(format!("Gemini 策略解析失败: {e}"));
            true
        }
    };
    merge_sync_result(
        &mut result,
        sync_gemini_usage_with_mode(db, gemini_record_usage),
        "Gemini",
    );

    Ok(result)
}

fn merge_sync_result(
    aggregate: &mut SessionSyncResult,
    next: Result<SessionSyncResult, AppError>,
    app_label: &str,
) {
    match next {
        Ok(sync_result) => {
            aggregate.imported += sync_result.imported;
            aggregate.skipped += sync_result.skipped;
            aggregate.files_scanned += sync_result.files_scanned;
            aggregate.errors.extend(sync_result.errors);
        }
        Err(e) => {
            aggregate.errors.push(format!("{app_label} 同步失败: {e}"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proxy::types::UsageStatsSource;
    use std::ffi::OsString;
    use std::fs;
    use std::sync::{Mutex, OnceLock};
    use tempfile::tempdir;

    fn test_env_guard() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|err| err.into_inner())
    }

    struct TestHomeScope {
        _guard: std::sync::MutexGuard<'static, ()>,
        temp: tempfile::TempDir,
        old_test_home: Option<OsString>,
        old_home: Option<OsString>,
    }

    impl TestHomeScope {
        fn new() -> Self {
            let guard = test_env_guard();
            let temp = tempdir().expect("tempdir");
            let old_test_home = std::env::var_os("CC_SWITCH_TEST_HOME");
            let old_home = std::env::var_os("HOME");
            std::env::set_var("CC_SWITCH_TEST_HOME", temp.path());
            std::env::set_var("HOME", temp.path());

            Self {
                _guard: guard,
                temp,
                old_test_home,
                old_home,
            }
        }

        fn root(&self) -> &std::path::Path {
            self.temp.path()
        }
    }

    impl Drop for TestHomeScope {
        fn drop(&mut self) {
            match &self.old_test_home {
                Some(value) => std::env::set_var("CC_SWITCH_TEST_HOME", value),
                None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
            }
            match &self.old_home {
                Some(value) => std::env::set_var("HOME", value),
                None => std::env::remove_var("HOME"),
            }
        }
    }

    #[tokio::test]
    async fn effective_usage_source_falls_back_to_session_when_proxy_not_running(
    ) -> Result<(), AppError> {
        // 代理未运行时，用量统计始终回退到 Session 日志。
        let db = Database::memory()?;

        let source = resolve_effective_usage_source(&db, false, "claude").await?;
        assert_eq!(source, UsageStatsSource::Session);
        Ok(())
    }

    #[tokio::test]
    async fn effective_usage_source_uses_config_when_takeover_enabled() -> Result<(), AppError> {
        // 本地路由/代理接管生效时，以 app 级别配置的统计来源为准。
        let db = Database::memory()?;
        let mut config = db.get_proxy_config_for_app("claude").await?;
        config.enabled = true;
        config.usage_stats_source = UsageStatsSource::Session;
        db.update_proxy_config_for_app(config).await?;

        let source = resolve_effective_usage_source(&db, true, "claude").await?;
        assert_eq!(source, UsageStatsSource::Session);
        Ok(())
    }

    #[tokio::test]
    async fn effective_usage_source_falls_back_to_session_when_app_not_taken_over(
    ) -> Result<(), AppError> {
        // 未被接管的 app 即使配置为上游统计，也仍然回退到 Session 日志。
        let db = Database::memory()?;
        let mut config = db.get_proxy_config_for_app("codex").await?;
        config.enabled = false;
        config.usage_stats_source = UsageStatsSource::Proxy;
        db.update_proxy_config_for_app(config).await?;

        let source = resolve_effective_usage_source(&db, true, "codex").await?;
        assert_eq!(source, UsageStatsSource::Session);
        Ok(())
    }

    #[tokio::test]
    async fn should_record_proxy_usage_only_blocks_session_mode_during_takeover(
    ) -> Result<(), AppError> {
        // 只有“已接管且明确选择 Session 日志”的 app，才会跳过代理侧用量写入。
        let db = Database::memory()?;
        let mut config = db.get_proxy_config_for_app("codex").await?;
        config.enabled = true;
        config.usage_stats_source = UsageStatsSource::Session;
        db.update_proxy_config_for_app(config).await?;

        assert!(!should_record_proxy_usage(&db, "codex").await?);

        let mut proxy_mode = db.get_proxy_config_for_app("gemini").await?;
        proxy_mode.enabled = true;
        proxy_mode.usage_stats_source = UsageStatsSource::Proxy;
        db.update_proxy_config_for_app(proxy_mode).await?;
        assert!(should_record_proxy_usage(&db, "gemini").await?);

        Ok(())
    }

    #[tokio::test]
    async fn should_record_session_usage_when_proxy_not_running() -> Result<(), AppError> {
        // 代理完全未运行时，Session 日志同步应保持开启。
        let db = Database::memory()?;

        assert!(should_record_session_usage(&db, false, "claude").await?);

        Ok(())
    }

    #[tokio::test]
    async fn should_record_session_usage_uses_session_as_fallback_when_not_taken_over(
    ) -> Result<(), AppError> {
        // 接管策略不应影响未实际接管的 app，其 Session 同步仍应作为回退路径。
        let db = Database::memory()?;
        let mut config = db.get_proxy_config_for_app("codex").await?;
        config.enabled = false;
        config.usage_stats_source = UsageStatsSource::Proxy;
        db.update_proxy_config_for_app(config).await?;

        assert!(should_record_session_usage(&db, true, "codex").await?);

        Ok(())
    }

    #[tokio::test]
    async fn should_record_session_usage_respects_takeover_source() -> Result<(), AppError> {
        // 接管生效后，是否记录 Session 用量取决于每个 app 选择的统计来源。
        let db = Database::memory()?;

        let mut proxy_mode = db.get_proxy_config_for_app("gemini").await?;
        proxy_mode.enabled = true;
        proxy_mode.usage_stats_source = UsageStatsSource::Proxy;
        db.update_proxy_config_for_app(proxy_mode).await?;
        assert!(!should_record_session_usage(&db, true, "gemini").await?);

        let mut session_mode = db.get_proxy_config_for_app("claude").await?;
        session_mode.enabled = true;
        session_mode.usage_stats_source = UsageStatsSource::Session;
        db.update_proxy_config_for_app(session_mode).await?;
        assert!(should_record_session_usage(&db, true, "claude").await?);

        Ok(())
    }

    #[tokio::test]
    async fn sync_session_usage_by_policy_applies_per_app_recording_modes() -> Result<(), AppError>
    {
        // 编排层端到端测试：只有最终解析为 Session 模式的 app 才应导入用量。
        let scope = TestHomeScope::new();
        let home = scope.root();

        // Claude 保持上游统计模式，因此只扫描日志文件，不导入用量。
        let claude_path = home
            .join(".claude")
            .join("projects")
            .join("project-a")
            .join("session.jsonl");
        fs::create_dir_all(claude_path.parent().expect("claude parent"))
            .expect("create claude dirs");
        fs::write(
            &claude_path,
            "{\"type\":\"assistant\",\"message\":{\"id\":\"msg_1\",\"model\":\"claude-sonnet-4-5\",\"usage\":{\"input_tokens\":12,\"output_tokens\":6},\"stop_reason\":\"end_turn\"},\"timestamp\":\"2026-04-05T12:00:00Z\",\"sessionId\":\"session-1\"}\n",
        )
        .expect("write claude session log");

        // Codex 在接管开启时显式选择 Session 日志统计。
        let codex_path = home
            .join(".codex")
            .join("sessions")
            .join("2026")
            .join("03")
            .join("06")
            .join("codex.jsonl");
        fs::create_dir_all(codex_path.parent().expect("codex parent")).expect("create codex dirs");
        fs::write(
            &codex_path,
            concat!(
                "{\"timestamp\":\"2026-03-06T21:50:12Z\",\"type\":\"session_meta\",\"payload\":{\"id\":\"session-1\"}}\n",
                "{\"timestamp\":\"2026-03-06T21:50:13Z\",\"type\":\"turn_context\",\"payload\":{\"model\":\"gpt-5.4\"}}\n",
                "{\"timestamp\":\"2026-03-06T21:50:14Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"info\":{\"model\":\"gpt-5.4\",\"total_token_usage\":{\"input_tokens\":100,\"cached_input_tokens\":20,\"output_tokens\":30}}}}\n"
            ),
        )
        .expect("write codex session log");

        // Gemini 未被接管，因此会回退到 Session 日志统计。
        let gemini_path = home
            .join(".gemini")
            .join("tmp")
            .join("project-hash")
            .join("chats")
            .join("session-1.json");
        fs::create_dir_all(gemini_path.parent().expect("gemini parent"))
            .expect("create gemini dirs");
        fs::write(
            &gemini_path,
            serde_json::json!({
                "sessionId": "gemini-session-1",
                "messages": [
                    {
                        "id": "msg-1",
                        "type": "gemini",
                        "model": "gemini-2.5-pro",
                        "timestamp": "2026-04-05T12:00:00Z",
                        "tokens": {
                            "input": 50,
                            "output": 10,
                            "cached": 5,
                            "thoughts": 2
                        }
                    }
                ]
            })
            .to_string(),
        )
        .expect("write gemini session");

        let db = Database::memory()?;

        let mut claude_config = db.get_proxy_config_for_app("claude").await?;
        claude_config.enabled = true;
        claude_config.usage_stats_source = UsageStatsSource::Proxy;

        let mut codex_config = db.get_proxy_config_for_app("codex").await?;
        codex_config.enabled = true;
        codex_config.usage_stats_source = UsageStatsSource::Session;

        let mut gemini_config = db.get_proxy_config_for_app("gemini").await?;
        gemini_config.enabled = false;
        gemini_config.usage_stats_source = UsageStatsSource::Proxy;

        db.update_proxy_config_for_app(claude_config).await?;
        db.update_proxy_config_for_app(codex_config).await?;
        db.update_proxy_config_for_app(gemini_config).await?;

        let result = sync_session_usage_by_policy_with_proxy_running(&db, true).await?;

        // 最终只有 Codex 和 Gemini 会写入用量，但三个文件的游标都应推进。
        assert_eq!(result.imported, 2);
        assert_eq!(result.skipped, 0);
        assert_eq!(result.files_scanned, 3);
        assert!(result.errors.is_empty());

        let conn = crate::database::lock_conn!(db.conn);
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM proxy_request_logs", [], |row| {
                row.get(0)
            })
            .expect("count imported logs");
        assert_eq!(count, 2);

        let data_sources: Vec<String> = {
            let mut stmt = conn
                .prepare("SELECT data_source FROM proxy_request_logs ORDER BY data_source")
                .expect("prepare data sources");
            stmt.query_map([], |row| row.get::<_, String>(0))
                .expect("query data sources")
                .collect::<Result<Vec<_>, _>>()
                .expect("collect data sources")
        };
        assert_eq!(
            data_sources,
            vec!["codex_session".to_string(), "gemini_session".to_string()]
        );

        let sync_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM session_log_sync", [], |row| {
                row.get(0)
            })
            .expect("count sync cursors");
        assert_eq!(sync_count, 3);

        Ok(())
    }
}
