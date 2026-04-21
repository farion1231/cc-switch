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

    #[tokio::test]
    async fn effective_usage_source_falls_back_to_session_when_proxy_not_running(
    ) -> Result<(), AppError> {
        let db = Database::memory()?;

        let source = resolve_effective_usage_source(&db, false, "claude").await?;
        assert_eq!(source, UsageStatsSource::Session);
        Ok(())
    }

    #[tokio::test]
    async fn effective_usage_source_uses_config_when_takeover_enabled() -> Result<(), AppError> {
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
    async fn should_record_proxy_usage_only_blocks_session_mode_during_takeover(
    ) -> Result<(), AppError> {
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
        let db = Database::memory()?;

        assert!(should_record_session_usage(&db, false, "claude").await?);

        Ok(())
    }

    #[tokio::test]
    async fn should_record_session_usage_uses_session_as_fallback_when_not_taken_over(
    ) -> Result<(), AppError> {
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
}
