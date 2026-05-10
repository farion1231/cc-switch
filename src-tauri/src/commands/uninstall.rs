use tauri::{AppHandle, State};

use crate::services::stream_command::{emit_done, ProcessRegistry};
use crate::services::uninstall::UninstallReport;

/// 一键卸载 Claude Code
///
/// # 参数
/// - `dry_run`: 为 true 时不执行破坏性操作，仅返回各步骤的"将会做什么"说明
/// - `channel_id`: 流式日志通道 id，前端通过同一 id 订阅 install-log/install-log-done 事件
#[tauri::command]
pub async fn uninstall_claude_code(
    app: AppHandle,
    registry: State<'_, ProcessRegistry>,
    dry_run: bool,
    channel_id: String,
) -> Result<UninstallReport, String> {
    log::info!(
        "开始卸载 Claude Code (dry_run={}, channel_id={})",
        dry_run,
        channel_id
    );

    let cid = channel_id.as_str();
    let state = registry.begin_session(cid).await;

    let result =
        crate::services::uninstall::uninstall_claude_code(&app, &state, cid, dry_run).await;

    registry.end_session(cid).await;

    match &result {
        Ok(report) => {
            log::info!("卸载完成: {:?}", report.overall);
            let cancelled = report.steps.iter().any(|s| s.message.contains("已取消"));
            let success =
                report.overall == crate::services::uninstall::UninstallOverallStatus::Success;
            emit_done(&app, cid, success, None, cancelled);
        }
        Err(e) => {
            log::error!("卸载失败: {}", e);
            emit_done(&app, cid, false, None, false);
        }
    }

    result.map_err(|e| format!("卸载失败: {}", e))
}
