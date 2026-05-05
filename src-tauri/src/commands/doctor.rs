use crate::services::env_doctor::DiagnosisResult;

/// 执行环境诊断
///
/// 检测项包括：
/// - 工具安装状态（Claude Code、Codex、Gemini CLI、OpenCode）
/// - Node.js 版本（需要 >= 18.0.0）
/// - 环境变量冲突
/// - 配置文件完整性（~/.claude/settings.json）
///
/// # 返回值
///
/// 返回 `DiagnosisResult`，包含：
/// - `overall_status`: 整体健康状态
/// - `issues`: 诊断发现的问题列表
/// - `tools_status`: 各工具的状态
///
/// # 错误
///
/// 如果诊断过程中发生错误，返回错误信息字符串
#[tauri::command]
pub async fn diagnose_environment() -> Result<DiagnosisResult, String> {
    crate::services::env_doctor::diagnose_environment()
        .await
        .map_err(|e| format!("环境诊断失败: {}", e))
}
