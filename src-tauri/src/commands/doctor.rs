use tauri::{AppHandle, State};

#[cfg(target_os = "macos")]
use crate::services::brew_migration::{
    build_migration_preview, execute_migration, MigrationOverallStatus, MigrationPreview,
    MigrationResult,
};
#[cfg(target_os = "macos")]
use crate::services::claude_installer::{
    detect_claude, run_claude_update, run_install_sh, InstallMethod,
};
use crate::services::env_doctor::{DiagnosisIssue, DiagnosisResult, FixResult};
use crate::services::installer::{self, InstallResult};
use crate::services::stream_command::{emit_done, emit_error_line, emit_progress, ProcessRegistry};

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
pub async fn install_tool(
    app: AppHandle,
    registry: State<'_, ProcessRegistry>,
    tool: String,
    channel_id: String,
) -> Result<InstallResult, String> {
    let tool_lower = tool.to_lowercase();
    let cid = channel_id.as_str();

    if tool_lower != "claude" {
        let result = InstallResult {
            success: false,
            message: format!("不支持的工具: {}。当前仅支持: claude", tool),
            installed_version: None,
            action: None,
            already_installed: None,
            verified: Some(false),
            error_code: Some("unsupported_tool".to_string()),
        };
        emit_done(&app, cid, false, None, false);
        return Ok(result);
    }

    let state = registry.begin_session(cid).await;
    emit_progress(&app, cid, "===== 开始安装 Claude Code =====");

    let result = run_install_claude_flow(&app, &registry, &state, cid).await;

    registry.end_session(cid).await;

    let cancelled = result
        .as_ref()
        .ok()
        .and_then(|r| r.error_code.as_deref())
        .map(|c| c == "cancelled")
        .unwrap_or(false);
    let success = result.as_ref().map(|r| r.success).unwrap_or(false);
    emit_done(&app, cid, success, None, cancelled);
    result
}

async fn run_install_claude_flow(
    app: &AppHandle,
    registry: &ProcessRegistry,
    state: &crate::services::stream_command::SessionState,
    cid: &str,
) -> Result<InstallResult, String> {
    let current_version =
        crate::commands::misc::get_tool_versions(Some(vec!["claude".to_string()]), None)
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
                emit_progress(
                    app,
                    cid,
                    format!("Claude Code 已是最新版本 ({})，无需安装", version),
                );
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
        emit_progress(app, cid, "Node.js 未安装，开始安装 Node.js...");
        let nodejs_result = installer::install_nodejs(app, state, cid).await?;

        if !nodejs_result.success {
            emit_error_line(app, cid, "Node.js 安装失败，终止流程");
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
        emit_progress(
            app,
            cid,
            format!("Node.js 安装成功: {:?}", nodejs_result.installed_version),
        );
    } else {
        let version_sufficient = installer::check_nodejs_version_sufficient()
            .map_err(|e| format!("检查 Node.js 版本失败: {}", e))?;

        if !version_sufficient {
            emit_progress(
                app,
                cid,
                "Node.js 版本不满足要求（需要 >= 18.0.0），开始升级...",
            );
            let nodejs_result = installer::install_nodejs(app, state, cid).await?;

            if !nodejs_result.success {
                emit_error_line(app, cid, "Node.js 升级失败，终止流程");
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
            emit_progress(
                app,
                cid,
                format!("Node.js 升级成功: {:?}", nodejs_result.installed_version),
            );
        }
    }

    if registry.is_cancelled(cid).await {
        return Ok(InstallResult {
            success: false,
            message: "已取消".to_string(),
            installed_version: None,
            action: None,
            already_installed: None,
            verified: Some(false),
            error_code: Some("cancelled".to_string()),
        });
    }

    let mut result = installer::install_claude_code(app, state, cid).await?;

    if result.success {
        let verified =
            crate::commands::misc::get_tool_versions(Some(vec!["claude".to_string()]), None)
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
            result.action = Some(if current_version.and_then(|tool| tool.version).is_some() {
                "upgrade".to_string()
            } else {
                "install".to_string()
            });
        }
    }

    if !result.success && result.error_code.as_deref() != Some("cancelled") {
        result.message = "安装未完成，请检查网络或 Node.js 环境后重试。".to_string();
    }

    Ok(result)
}

/// 取消正在执行的安装/卸载流程
///
/// 前端在用户点「取消」按钮时调用：标记会话为已取消并通过 Notify 唤醒所有
/// 正在等待 child.wait() 的 stream_command 任务，让它们走 kill 分支。
#[tauri::command]
pub async fn cancel_install(
    registry: State<'_, ProcessRegistry>,
    channel_id: String,
) -> Result<bool, String> {
    Ok(registry.cancel(&channel_id).await)
}

// ─────────────────────────────────────────────────────────────────────────
// macOS 专属：基于官方 install.sh / binary auto-updater 的安装与升级命令
//
// 设计原则（参见 services/claude_installer.rs 模块文档）：
// - 安装：直接跑官方 install.sh，不再要求 Node / brew 前置（macOS 上
//   Claude Code 是 native binary，与 Node/npm 无关）
// - 升级：调 binary 自带 `update` 子命令，让 Anthropic 自己决定升不升
// - Brew 装的不允许走 native updater（会无效或冲突），需要先迁移到 native
// ─────────────────────────────────────────────────────────────────────────

/// 一键安装 Claude Code（macOS）。
///
/// 直接跑官方 `curl -fsSL https://claude.ai/install.sh | bash`。安装脚本本身
/// 只依赖 bash + curl + 系统工具，无需 Node / brew 前置。
#[cfg(target_os = "macos")]
#[tauri::command]
pub async fn install_claude_code(
    app: AppHandle,
    registry: State<'_, ProcessRegistry>,
    channel_id: String,
) -> Result<InstallResult, String> {
    let cid = channel_id.as_str();
    let state = registry.begin_session(cid).await;
    emit_progress(&app, cid, "===== 开始安装 Claude Code =====");

    let result = run_install_sh(&app, &state, cid).await;

    registry.end_session(cid).await;
    emit_install_done(&app, cid, &result);
    result
}

/// 检查并升级 Claude Code（已装情况下用，仅 native 安装可用）。
///
/// 调用 binary 自带 `update` 子命令；brew 安装不走这条（应先迁移）。
#[cfg(target_os = "macos")]
#[tauri::command]
pub async fn check_claude_update(
    app: AppHandle,
    registry: State<'_, ProcessRegistry>,
    channel_id: String,
) -> Result<InstallResult, String> {
    let cid = channel_id.as_str();
    let state = registry.begin_session(cid).await;
    emit_progress(&app, cid, "===== 检查 Claude Code 更新 =====");

    let result: Result<InstallResult, String> = match detect_claude() {
        None => {
            emit_error_line(&app, cid, "未检测到已安装的 Claude Code");
            Ok(InstallResult {
                success: false,
                message: "未检测到已安装的 Claude Code，请先「一键安装」。".to_string(),
                installed_version: None,
                action: None,
                already_installed: Some(false),
                verified: Some(false),
                error_code: Some("not_installed".to_string()),
            })
        }
        Some(d) if matches!(d.install_method, InstallMethod::Brew { .. }) => {
            // brew 装的不能走 native updater —— binary 的 `update` 子命令会
            // 试图覆盖 brew 管理的文件，要么权限拒绝要么造成 brew 状态不一致。
            // 引导用户走迁移流程或手动 brew upgrade。
            emit_error_line(
                &app,
                cid,
                "当前为 Homebrew 安装，请通过「迁移到官方版」按钮切换，或手动执行 brew upgrade。",
            );
            Ok(InstallResult {
                success: false,
                message: "当前 Claude Code 通过 Homebrew 安装，无法用 native updater 升级。\
                          请使用「迁移到官方版」按钮，或手动执行 \
                          `brew upgrade --cask claude-code`。"
                    .to_string(),
                installed_version: None,
                action: None,
                already_installed: Some(true),
                verified: Some(true),
                error_code: Some("brew_installed".to_string()),
            })
        }
        Some(d) => run_claude_update(&app, &state, cid, &d).await,
    };

    registry.end_session(cid).await;
    emit_install_done(&app, cid, &result);
    result
}

/// 预览 brew → native 迁移（dry-run，不动磁盘）。返回会做的步骤、备份目标、
/// 当前 brew cask 名等，供前端弹"确认迁移"对话框。
#[cfg(target_os = "macos")]
#[tauri::command]
pub async fn preview_brew_migration() -> Result<MigrationPreview, String> {
    build_migration_preview()
}

/// 执行 brew → native 迁移。备份 → brew uninstall → install.sh → 验证；
/// 任意步骤失败自动 brew install 回滚，备份目录始终保留。
#[cfg(target_os = "macos")]
#[tauri::command]
pub async fn migrate_brew_to_native(
    app: AppHandle,
    registry: State<'_, ProcessRegistry>,
    channel_id: String,
) -> Result<MigrationResult, String> {
    let cid = channel_id.as_str();
    let state = registry.begin_session(cid).await;
    emit_progress(
        &app,
        cid,
        "===== 开始迁移 Claude Code: Homebrew → 官方 native =====",
    );

    let result = execute_migration(&app, &state, cid).await;

    registry.end_session(cid).await;
    let success = matches!(
        result.as_ref().map(|r| r.overall),
        Ok(MigrationOverallStatus::Success)
    );
    // 迁移流程内部不区分用户取消和系统失败 —— 都体现为 RolledBack/Failed。
    // emit_done 的 cancelled 字段统一传 false，前端按 result.overall 区分。
    emit_done(&app, cid, success, None, false);
    result
}

/// 内部 helper：把 install/update 类命令的尾声 emit_done 收敛到一处。
/// `error_code == "cancelled"` 才把 cancelled 标记为 true（让前端日志面板
/// 区分"用户取消"与"执行失败"两种 UX）。
#[cfg(target_os = "macos")]
fn emit_install_done(app: &AppHandle, cid: &str, result: &Result<InstallResult, String>) {
    let cancelled = result
        .as_ref()
        .ok()
        .and_then(|r| r.error_code.as_deref())
        .map(|c| c == "cancelled")
        .unwrap_or(false);
    let success = result.as_ref().map(|r| r.success).unwrap_or(false);
    emit_done(app, cid, success, None, cancelled);
}
