use crate::services::uninstall::UninstallReport;

/// 一键卸载 Claude Code
///
/// # 参数
/// - `dry_run`: 为 true 时不执行破坏性操作，仅返回各步骤的"将会做什么"说明
#[tauri::command]
pub async fn uninstall_claude_code(dry_run: bool) -> Result<UninstallReport, String> {
    log::info!("开始卸载 Claude Code (dry_run={})", dry_run);
    let report = crate::services::uninstall::uninstall_claude_code(dry_run)
        .await
        .map_err(|e| {
            log::error!("卸载失败: {}", e);
            format!("卸载失败: {}", e)
        })?;
    log::info!("卸载完成: {:?}", report.overall);
    Ok(report)
}
