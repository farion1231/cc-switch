use crate::services::env_doctor::{DiagnosisIssue, DiagnosisResult, FixResult};
use crate::services::installer::{self, InstallResult};

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

/// 批量修复环境问题
///
/// 只修复 `auto_fixable = true` 的问题。
/// 每个修复操作前会先备份相关数据。
///
/// 支持的修复类型：
/// - 环境变量冲突：从 shell 配置文件中删除冲突的环境变量（自动备份）
/// - 配置文件损坏：从备份恢复或生成默认配置
/// - 权限不足：修复文件/目录权限（macOS/Linux）
///
/// 不支持的修复类型（需要用户明确触发）：
/// - 安装工具：使用 `install_tool` 命令
/// - 安装 Node.js：使用 `install_tool` 命令
/// - 更新工具：使用 `install_tool` 命令
///
/// # 参数
///
/// - `issues`: 待修复的问题列表（从 `diagnose_environment` 获取）
///
/// # 返回值
///
/// 返回 `FixResult`，包含：
/// - `fixed`: 成功修复的问题 ID 列表
/// - `failed`: 修复失败的问题列表（问题 ID, 错误信息）
///
/// # 错误
///
/// 如果修复过程中发生致命错误，返回错误信息字符串
///
/// # 示例
///
/// ```rust
/// // 1. 先诊断环境
/// let diagnosis = diagnose_environment().await?;
///
/// // 2. 过滤出可自动修复的问题
/// let fixable_issues: Vec<DiagnosisIssue> = diagnosis.issues
///     .into_iter()
///     .filter(|issue| issue.auto_fixable)
///     .collect();
///
/// // 3. 执行修复
/// let result = fix_environment(fixable_issues).await?;
///
/// // 4. 检查修复结果
/// println!("成功修复: {:?}", result.fixed);
/// println!("修复失败: {:?}", result.failed);
/// ```
#[tauri::command]
pub async fn fix_environment(issues: Vec<DiagnosisIssue>) -> Result<FixResult, String> {
    log::info!("开始修复环境问题，共 {} 个问题", issues.len());

    let result = crate::services::env_doctor::fix_environment(issues)
        .await
        .map_err(|e| {
            log::error!("环境修复失败: {}", e);
            format!("环境修复失败: {}", e)
        })?;

    log::info!(
        "环境修复完成，成功: {}, 失败: {}",
        result.fixed.len(),
        result.failed.len()
    );

    if !result.failed.is_empty() {
        for failure in &result.failed {
            log::warn!(
                "修复失败 [{}] code={:?}: {}",
                failure.issue_id,
                failure.error_code,
                failure.message
            );
        }
    }

    Ok(result)
}

/// 安装指定工具
///
/// 当前仅支持安装 `claude`（Claude Code）。
///
/// # 安装流程
///
/// 1. 检查 Node.js 是否已安装
/// 2. 如果 Node.js 未安装或版本不满足要求（< 18.0.0），先安装 Node.js
/// 3. 调用 Claude Code 安装函数
///
/// # 参数
///
/// - `tool`: 工具名称（仅支持 claude）
///
/// # 返回值
///
/// 返回 `InstallResult`，包含：
/// - `success`: 是否安装成功
/// - `message`: 安装结果消息
/// - `installed_version`: 安装后的版本号（如果成功）
///
/// # 错误
///
/// 如果安装过程中发生错误，返回错误信息字符串
#[tauri::command]
pub async fn install_tool(tool: String) -> Result<InstallResult, String> {
    let tool_lower = tool.to_lowercase();

    if tool_lower != "claude" {
        return Ok(InstallResult {
            success: false,
            message: format!("不支持的工具: {}。当前仅支持: claude", tool),
            installed_version: None,
            action: None,
            already_installed: None,
            verified: Some(false),
            error_code: Some("unsupported_tool".to_string()),
        });
    }

    let current_version = crate::commands::misc::get_tool_versions(
        Some(vec!["claude".to_string()]),
        None,
    )
    .await
    .map_err(|e| format!("检查 Claude Code 当前状态失败: {}", e))?
    .into_iter()
    .find(|tool| tool.name == "claude");

    if let Some(tool_version) = current_version.as_ref() {
        if let Some(version) = tool_version.version.clone() {
            let needs_upgrade = tool_version
                .latest_version
                .as_ref()
                .map(|latest| latest != &version)
                .unwrap_or(false);

            if !needs_upgrade {
                return Ok(InstallResult {
                    success: true,
                    message: "Claude Code 已安装，无需重复安装".to_string(),
                    installed_version: Some(version),
                    action: Some("none".to_string()),
                    already_installed: Some(true),
                    verified: Some(true),
                    error_code: None,
                });
            }
        }
    }

    let nodejs_installed =
        installer::check_nodejs_installed().map_err(|e| format!("检查 Node.js 失败: {}", e))?;

    if !nodejs_installed {
        log::info!("Node.js 未安装，开始安装 Node.js...");
        let nodejs_result =
            installer::install_nodejs().map_err(|e| format!("安装 Node.js 失败: {}", e))?;

        if !nodejs_result.success {
            return Ok(InstallResult {
                success: false,
                message: "安装未完成，请检查网络或 Node.js 环境后重试。".to_string(),
                installed_version: None,
                action: Some("install".to_string()),
                already_installed: Some(false),
                verified: Some(false),
                error_code: nodejs_result.error_code,
            });
        }
        log::info!("Node.js 安装成功: {:?}", nodejs_result.installed_version);
    } else {
        let version_sufficient = installer::check_nodejs_version_sufficient()
            .map_err(|e| format!("检查 Node.js 版本失败: {}", e))?;

        if !version_sufficient {
            log::info!("Node.js 版本不满足要求（需要 >= 18.0.0），开始升级...");
            let nodejs_result =
                installer::install_nodejs().map_err(|e| format!("升级 Node.js 失败: {}", e))?;

            if !nodejs_result.success {
                return Ok(InstallResult {
                    success: false,
                    message: "安装未完成，请检查网络或 Node.js 环境后重试。".to_string(),
                    installed_version: None,
                    action: Some("upgrade".to_string()),
                    already_installed: Some(false),
                    verified: Some(false),
                    error_code: nodejs_result.error_code,
                });
            }
            log::info!("Node.js 升级成功: {:?}", nodejs_result.installed_version);
        }
    }

    log::info!("开始安装 Claude Code...");
    let mut result =
        installer::install_claude_code().map_err(|e| format!("安装 Claude Code 失败: {}", e))?;

    if result.success {
        let verified = crate::commands::misc::get_tool_versions(
            Some(vec!["claude".to_string()]),
            None,
        )
        .await
        .map_err(|e| format!("验证 Claude Code 安装结果失败: {}", e))?
        .into_iter()
        .find(|tool| tool.name == "claude")
        .and_then(|tool| tool.version);

        result.verified = Some(verified.is_some());
        if let Some(version) = verified {
            result.installed_version = Some(version);
        }
        if result.action.is_none() {
            result.action = Some(
                if current_version.and_then(|tool| tool.version).is_some() {
                    "upgrade".to_string()
                } else {
                    "install".to_string()
                },
            );
        }
    }

    if !result.success {
        result.message = "安装未完成，请检查网络或 Node.js 环境后重试。".to_string();
    }

    Ok(result)
}
