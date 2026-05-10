use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use chrono::Local;
use serde::{Deserialize, Serialize};
use tauri::AppHandle;

use crate::services::stream_command::{
    emit_error_line, emit_progress, stream_command, SessionState,
};

// ─── 数据结构 ───

/// 卸载报告
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UninstallReport {
    /// 备份目录路径
    pub backup_path: String,
    /// 各步骤记录
    pub steps: Vec<UninstallStep>,
    /// 整体状态
    pub overall: UninstallOverallStatus,
}

/// 单个卸载步骤
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UninstallStep {
    /// 步骤名称
    pub name: String,
    /// 状态
    pub status: UninstallStepStatus,
    /// 详细消息
    pub message: String,
}

/// 步骤状态
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub enum UninstallStepStatus {
    /// 成功
    Success,
    /// 跳过
    Skipped,
    /// 失败
    Failed,
}

/// 整体卸载状态
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub enum UninstallOverallStatus {
    /// 全部成功
    Success,
    /// 部分成功
    Partial,
    /// 失败
    Failed,
}

impl UninstallReport {
    fn determine_overall(steps: &[UninstallStep], backup_failed: bool) -> UninstallOverallStatus {
        if backup_failed {
            return UninstallOverallStatus::Failed;
        }
        if steps
            .iter()
            .all(|s| s.status == UninstallStepStatus::Success)
        {
            UninstallOverallStatus::Success
        } else {
            UninstallOverallStatus::Partial
        }
    }
}

// ─── 入口函数 ───

/// 一键卸载 Claude Code
///
/// 执行 5 步清理流程，每步进度通过 stream_command 实时推送给前端：
/// 1. 创建备份目录
/// 2. 备份并删除 ~/.claude/
/// 3. 清理系统凭证（钥匙串 / 凭据管理器）
/// 4. 清理 shell 环境变量
/// 5. 卸载 Claude Code CLI
///
/// # 参数
/// - `app` / `state` / `channel_id`: 流式日志通道，由 commands 层注入
/// - `dry_run`: 为 true 时不执行破坏性操作，仅返回将要执行的操作说明
pub async fn uninstall_claude_code(
    app: &AppHandle,
    state: &SessionState,
    channel_id: &str,
    dry_run: bool,
) -> Result<UninstallReport, String> {
    let app_config_dir = crate::config::get_app_config_dir();

    let timestamp = Local::now().format("%Y%m%d_%H%M%S").to_string();
    let backup_dir = app_config_dir
        .join("backups")
        .join(format!("uninstall-{}", timestamp));
    let backup_dir_str = backup_dir.to_string_lossy().to_string();

    emit_progress(
        app,
        channel_id,
        format!("===== 开始卸载 Claude Code (dry_run={}) =====", dry_run),
    );

    let mut steps: Vec<UninstallStep> = Vec::new();

    let mut cancelled_after: Option<usize> = None;
    macro_rules! check_cancel {
        ($idx:expr) => {
            if !dry_run && state.cancel_token.load(std::sync::atomic::Ordering::SeqCst) {
                emit_error_line(app, channel_id, "用户已取消，停止后续步骤");
                cancelled_after = Some($idx);
            }
        };
    }

    // Step 1
    let step1 = step_create_backup(app, channel_id, &backup_dir, dry_run).await;
    let step1_failed = step1.status == UninstallStepStatus::Failed;
    steps.push(step1);
    check_cancel!(steps.len());

    // Step 2
    if cancelled_after.is_none() {
        let claude_dir = crate::config::get_claude_config_dir();
        let step2 =
            step_backup_delete_claude(app, state, channel_id, &claude_dir, &backup_dir, dry_run)
                .await;
        steps.push(step2);
        check_cancel!(steps.len());
    }

    // Step 3
    if cancelled_after.is_none() {
        let step3 = step_clean_credentials(app, state, channel_id, dry_run).await;
        steps.push(step3);
        check_cancel!(steps.len());
    }

    // Step 4
    if cancelled_after.is_none() {
        let home = crate::config::get_home_dir();
        let step4 = step_clean_shell_env(app, channel_id, &home, &backup_dir, dry_run).await;
        steps.push(step4);
        check_cancel!(steps.len());
    }

    // Step 5
    if cancelled_after.is_none() {
        let step5 = step_uninstall_cli(app, state, channel_id, dry_run).await;
        steps.push(step5);
    }

    // 把因取消而未执行的步骤补成 Skipped("已取消")，便于前端展示
    let total_steps = 5;
    while steps.len() < total_steps {
        steps.push(UninstallStep {
            name: format!("步骤 {}", steps.len() + 1),
            status: UninstallStepStatus::Skipped,
            message: "用户已取消，未执行".to_string(),
        });
    }

    let overall = if cancelled_after.is_some() {
        UninstallOverallStatus::Failed
    } else {
        UninstallReport::determine_overall(&steps, step1_failed)
    };

    emit_progress(
        app,
        channel_id,
        format!("===== 卸载完成: {:?} =====", overall),
    );

    Ok(UninstallReport {
        backup_path: backup_dir_str,
        steps,
        overall,
    })
}

// ─── Step 1: 创建备份 ───

async fn step_create_backup(
    app: &AppHandle,
    cid: &str,
    backup_dir: &Path,
    dry_run: bool,
) -> UninstallStep {
    emit_progress(app, cid, format!("Step 1/5: 创建备份 -> {:?}", backup_dir));

    if dry_run {
        emit_progress(
            app,
            cid,
            format!("  [dry-run] 将创建备份目录: {:?}", backup_dir),
        );
        return UninstallStep {
            name: "创建备份".to_string(),
            status: UninstallStepStatus::Skipped,
            message: format!("将创建备份目录: {}", backup_dir.display()),
        };
    }

    match fs::create_dir_all(backup_dir) {
        Ok(()) => {
            emit_progress(app, cid, "  ✓ 备份目录创建成功");
            UninstallStep {
                name: "创建备份".to_string(),
                status: UninstallStepStatus::Success,
                message: format!("备份目录已创建: {}", backup_dir.display()),
            }
        }
        Err(e) => {
            emit_error_line(app, cid, format!("  ✗ 备份目录创建失败: {}", e));
            UninstallStep {
                name: "创建备份".to_string(),
                status: UninstallStepStatus::Failed,
                message: format!("备份目录创建失败: {}", e),
            }
        }
    }
}

// ─── Step 2: 备份并删除 ~/.claude/ ───

async fn step_backup_delete_claude(
    app: &AppHandle,
    state: &SessionState,
    cid: &str,
    claude_dir: &Path,
    backup_dir: &Path,
    dry_run: bool,
) -> UninstallStep {
    emit_progress(app, cid, "Step 2/5: 备份并删除 ~/.claude/");

    if !claude_dir.exists() {
        emit_progress(app, cid, "  ~/.claude 目录不存在，跳过");
        return UninstallStep {
            name: "备份并删除 ~/.claude/".to_string(),
            status: UninstallStepStatus::Skipped,
            message: "~/.claude 目录不存在，无需清理".to_string(),
        };
    }

    if dry_run {
        emit_progress(
            app,
            cid,
            format!(
                "  [dry-run] 将备份并删除 ~/.claude/ (路径: {:?})",
                claude_dir
            ),
        );
        return UninstallStep {
            name: "备份并删除 ~/.claude/".to_string(),
            status: UninstallStepStatus::Skipped,
            message: format!(
                "将备份 {} 到 {:?} 并删除原始目录",
                claude_dir.display(),
                backup_dir
            ),
        };
    }

    let dest = backup_dir.join(".claude");
    let source_str = claude_dir.to_string_lossy().to_string();
    let dest_str = dest.to_string_lossy().to_string();

    // cp -R 流式备份
    let cp_outcome =
        match stream_command(app, state, cid, "cp", &["-R", &source_str, &dest_str]).await {
            Ok(o) => o,
            Err(e) => {
                emit_error_line(app, cid, format!("  ✗ 执行 cp 命令失败: {}", e));
                return UninstallStep {
                    name: "备份并删除 ~/.claude/".to_string(),
                    status: UninstallStepStatus::Failed,
                    message: format!("执行 cp 命令失败: {}", e),
                };
            }
        };

    if cp_outcome.cancelled {
        return UninstallStep {
            name: "备份并删除 ~/.claude/".to_string(),
            status: UninstallStepStatus::Failed,
            message: "已取消".to_string(),
        };
    }
    if !cp_outcome.success {
        emit_error_line(app, cid, "  ✗ 备份 ~/.claude 失败");
        return UninstallStep {
            name: "备份并删除 ~/.claude/".to_string(),
            status: UninstallStepStatus::Failed,
            message: format!("备份 ~/.claude 失败 (exit_code={:?})", cp_outcome.exit_code),
        };
    }

    emit_progress(app, cid, format!("  ✓ ~/.claude 已备份到 {:?}", dest));

    match fs::remove_dir_all(claude_dir) {
        Ok(()) => {
            emit_progress(app, cid, "  ✓ ~/.claude 已删除");
            UninstallStep {
                name: "备份并删除 ~/.claude/".to_string(),
                status: UninstallStepStatus::Success,
                message: format!("~/.claude 已备份到 {} 并删除", dest.display()),
            }
        }
        Err(e) => {
            emit_error_line(app, cid, format!("  ✗ 删除 ~/.claude 失败: {}", e));
            UninstallStep {
                name: "备份并删除 ~/.claude/".to_string(),
                status: UninstallStepStatus::Failed,
                message: format!("~/.claude 已备份但删除失败: {}", e),
            }
        }
    }
}

// ─── Step 3: 清理系统凭证 ───

async fn step_clean_credentials(
    app: &AppHandle,
    state: &SessionState,
    cid: &str,
    dry_run: bool,
) -> UninstallStep {
    emit_progress(app, cid, "Step 3/5: 清理系统凭证");
    step_clean_credentials_impl(app, state, cid, dry_run).await
}

#[cfg(target_os = "macos")]
async fn step_clean_credentials_impl(
    app: &AppHandle,
    state: &SessionState,
    cid: &str,
    dry_run: bool,
) -> UninstallStep {
    if dry_run {
        emit_progress(
            app,
            cid,
            "  [dry-run] 将检查并清理 macOS 钥匙串中的 Claude Code 凭证",
        );
        return UninstallStep {
            name: "清理系统凭证".to_string(),
            status: UninstallStepStatus::Skipped,
            message: "将检查并清理 macOS 钥匙串中的 Claude Code 凭证".to_string(),
        };
    }

    // find 命令静默执行（避免在日志里泄露凭据元信息），不走 stream_command
    emit_progress(
        app,
        cid,
        "  检测钥匙串中是否存在 Claude Code-credentials...",
    );
    let find_output = Command::new("security")
        .args(["find-generic-password", "-s", "Claude Code-credentials"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .output();

    let exists = matches!(&find_output, Ok(o) if o.status.success());

    if !exists {
        emit_progress(app, cid, "  macOS 钥匙串中未找到 Claude Code 凭证");
        return UninstallStep {
            name: "清理系统凭证".to_string(),
            status: UninstallStepStatus::Skipped,
            message: "macOS 钥匙串中未找到 Claude Code 凭证".to_string(),
        };
    }

    // delete 走流式日志（虽然通常没什么输出）
    let outcome = match stream_command(
        app,
        state,
        cid,
        "security",
        &["delete-generic-password", "-s", "Claude Code-credentials"],
    )
    .await
    {
        Ok(o) => o,
        Err(e) => {
            return UninstallStep {
                name: "清理系统凭证".to_string(),
                status: UninstallStepStatus::Failed,
                message: format!("删除钥匙串凭证失败: {}", e),
            };
        }
    };

    if outcome.cancelled {
        return UninstallStep {
            name: "清理系统凭证".to_string(),
            status: UninstallStepStatus::Failed,
            message: "已取消".to_string(),
        };
    }

    if outcome.success {
        emit_progress(app, cid, "  ✓ macOS 钥匙串凭证已删除");
        UninstallStep {
            name: "清理系统凭证".to_string(),
            status: UninstallStepStatus::Success,
            message: "macOS 钥匙串中的 Claude Code 凭证已删除".to_string(),
        }
    } else {
        emit_error_line(app, cid, "  ✗ 删除钥匙串凭证失败");
        UninstallStep {
            name: "清理系统凭证".to_string(),
            status: UninstallStepStatus::Failed,
            message: format!("删除钥匙串凭证失败 (exit_code={:?})", outcome.exit_code),
        }
    }
}

#[cfg(target_os = "windows")]
async fn step_clean_credentials_impl(
    app: &AppHandle,
    state: &SessionState,
    cid: &str,
    dry_run: bool,
) -> UninstallStep {
    if dry_run {
        emit_progress(
            app,
            cid,
            "  [dry-run] 将检查并清理 Windows 凭据管理器中的 Claude Code 凭证",
        );
        return UninstallStep {
            name: "清理系统凭证".to_string(),
            status: UninstallStepStatus::Skipped,
            message: "将检查并清理 Windows 凭据管理器中的 Claude Code 凭证".to_string(),
        };
    }

    emit_progress(app, cid, "  执行 cmdkey /list 列出凭据条目...");
    let list_output = match Command::new("cmdkey").arg("/list").output() {
        Ok(o) => o,
        Err(e) => {
            emit_error_line(app, cid, format!("  ✗ cmdkey /list 执行失败: {}", e));
            return UninstallStep {
                name: "清理系统凭证".to_string(),
                status: UninstallStepStatus::Failed,
                message: format!("cmdkey /list 执行失败: {}", e),
            };
        }
    };

    let stdout = String::from_utf8_lossy(&list_output.stdout);
    let mut targets: Vec<String> = Vec::new();

    for line in stdout.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("Target:") {
            let target = rest.trim();
            if target.contains("Claude") {
                targets.push(target.to_string());
            }
        }
    }

    if targets.is_empty() {
        emit_progress(
            app,
            cid,
            "  Windows 凭据管理器中未找到 Claude Code 相关条目",
        );
        return UninstallStep {
            name: "清理系统凭证".to_string(),
            status: UninstallStepStatus::Skipped,
            message: "Windows 凭据管理器中未找到 Claude Code 相关条目".to_string(),
        };
    }

    let mut deleted_count = 0usize;
    let mut errors: Vec<String> = Vec::new();

    for target in &targets {
        let outcome = match stream_command(
            app,
            state,
            cid,
            "cmdkey",
            &[&format!("/delete:{}", target)],
        )
        .await
        {
            Ok(o) => o,
            Err(e) => {
                errors.push(format!("删除 {} 失败: {}", target, e));
                continue;
            }
        };
        if outcome.cancelled {
            return UninstallStep {
                name: "清理系统凭证".to_string(),
                status: UninstallStepStatus::Failed,
                message: "已取消".to_string(),
            };
        }
        if outcome.success {
            deleted_count += 1;
        } else {
            errors.push(format!(
                "删除 {} 失败 (exit_code={:?})",
                target, outcome.exit_code
            ));
        }
    }

    if errors.is_empty() {
        emit_progress(
            app,
            cid,
            format!("  ✓ 已删除 {} 个 Windows 凭据条目", deleted_count),
        );
        UninstallStep {
            name: "清理系统凭证".to_string(),
            status: UninstallStepStatus::Success,
            message: format!("已删除 {} 个 Windows 凭据条目", deleted_count),
        }
    } else {
        let error_msg = errors.join("; ");
        emit_error_line(app, cid, format!("  ✗ 部分凭据删除失败: {}", error_msg));
        UninstallStep {
            name: "清理系统凭证".to_string(),
            status: UninstallStepStatus::Failed,
            message: format!("部分凭据删除失败: {}", error_msg),
        }
    }
}

#[cfg(target_os = "linux")]
async fn step_clean_credentials_impl(
    app: &AppHandle,
    _state: &SessionState,
    cid: &str,
    _dry_run: bool,
) -> UninstallStep {
    emit_progress(app, cid, "  Linux 凭证清理未实现，跳过");
    UninstallStep {
        name: "清理系统凭证".to_string(),
        status: UninstallStepStatus::Skipped,
        message: "Linux 凭证清理未实现".to_string(),
    }
}

// ─── Step 4: 清理 shell 环境变量 ───

async fn step_clean_shell_env(
    app: &AppHandle,
    cid: &str,
    home: &Path,
    backup_dir: &Path,
    dry_run: bool,
) -> UninstallStep {
    emit_progress(app, cid, "Step 4/5: 清理 shell 环境变量");

    let files_to_check = [".zshrc", ".bashrc", ".bash_profile"];
    let mut existing_files: Vec<PathBuf> = Vec::new();

    for fname in &files_to_check {
        let path = home.join(fname);
        if path.exists() && path.is_file() {
            existing_files.push(path);
        }
    }

    if existing_files.is_empty() {
        emit_progress(app, cid, "  未找到 shell 配置文件，跳过");
        return UninstallStep {
            name: "清理 shell 环境变量".to_string(),
            status: UninstallStepStatus::Skipped,
            message: "未找到 shell 配置文件，无需清理".to_string(),
        };
    }

    if dry_run {
        let file_list: Vec<String> = existing_files
            .iter()
            .map(|p| p.display().to_string())
            .collect();
        emit_progress(
            app,
            cid,
            format!(
                "  [dry-run] 将处理 {} 个文件: {:?}",
                existing_files.len(),
                file_list
            ),
        );
        return UninstallStep {
            name: "清理 shell 环境变量".to_string(),
            status: UninstallStepStatus::Skipped,
            message: format!(
                "将清理以下文件中的 ANTHROPIC_/CLAUDE_ 环境变量: {}",
                file_list.join(", ")
            ),
        };
    }

    let mut total_cleaned = 0usize;
    let mut file_errors: Vec<String> = Vec::new();

    for file_path in &existing_files {
        let file_name = file_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "unknown".to_string());
        let bak_name = format!("{}.bak", file_name);
        let bak_path = backup_dir.join(&bak_name);

        match fs::copy(file_path, &bak_path) {
            Ok(_) => {
                emit_progress(
                    app,
                    cid,
                    format!("  ✓ 已备份 {} -> {:?}", file_path.display(), bak_path),
                );
            }
            Err(e) => {
                let err_msg = format!("备份 {} 失败: {}", file_path.display(), e);
                emit_error_line(app, cid, format!("  ✗ {}", err_msg));
                file_errors.push(err_msg);
                continue;
            }
        }

        let content = match fs::read_to_string(file_path) {
            Ok(s) => s,
            Err(e) => {
                let err_msg = format!("读取 {} 失败: {}", file_path.display(), e);
                emit_error_line(app, cid, format!("  ✗ {}", err_msg));
                file_errors.push(err_msg);
                continue;
            }
        };

        let lines: Vec<&str> = content.lines().collect();
        let filtered_lines: Vec<&str> = lines
            .iter()
            .filter(|line| !is_env_export_line(line))
            .copied()
            .collect();

        let removed_count = lines.len() - filtered_lines.len();

        if removed_count == 0 {
            emit_progress(
                app,
                cid,
                format!("  {} 中没有需要清理的 export 行", file_path.display()),
            );
            continue;
        }

        let new_content = filtered_lines.join("\n");
        let new_content = if content.ends_with('\n') {
            format!("{}\n", new_content)
        } else {
            new_content
        };

        match fs::write(file_path, &new_content) {
            Ok(()) => {
                emit_progress(
                    app,
                    cid,
                    format!(
                        "  ✓ {}: 已清理 {} 条环境变量",
                        file_path.display(),
                        removed_count
                    ),
                );
                total_cleaned += removed_count;
            }
            Err(e) => {
                let err_msg = format!("写入 {} 失败: {}", file_path.display(), e);
                emit_error_line(app, cid, format!("  ✗ {}", err_msg));
                file_errors.push(err_msg);
            }
        }
    }

    if !file_errors.is_empty() {
        let errors = file_errors.join("; ");
        emit_error_line(app, cid, format!("  ✗ 部分文件清理失败: {}", errors));
        return UninstallStep {
            name: "清理 shell 环境变量".to_string(),
            status: UninstallStepStatus::Failed,
            message: format!("部分文件清理失败: {}", errors),
        };
    }

    if total_cleaned == 0 {
        emit_progress(
            app,
            cid,
            "  shell 配置文件中未找到需要清理的 ANTHROPIC_/CLAUDE_ 环境变量",
        );
        return UninstallStep {
            name: "清理 shell 环境变量".to_string(),
            status: UninstallStepStatus::Skipped,
            message: "shell 配置文件中未找到需要清理的 ANTHROPIC_/CLAUDE_ 环境变量".to_string(),
        };
    }

    emit_progress(app, cid, format!("  ✓ 共清理 {} 条环境变量", total_cleaned));
    UninstallStep {
        name: "清理 shell 环境变量".to_string(),
        status: UninstallStepStatus::Success,
        message: format!("共清理 {} 条环境变量", total_cleaned),
    }
}

/// 判断一行是否为需要过滤的 export 环境变量
///
/// 匹配规则（严格）：
/// - 行首（忽略前导空白）以 `export ` 开头
/// - 变量名以 `ANTHROPIC_` 或 `CLAUDE_` 开头
/// - 包含 `=` 号（即有赋值）
fn is_env_export_line(line: &str) -> bool {
    let trimmed = line.trim_start();
    if !trimmed.starts_with("export ") {
        return false;
    }
    let after_export = &trimmed[7..]; // 跳过 "export "
    if !after_export.starts_with("ANTHROPIC_") && !after_export.starts_with("CLAUDE_") {
        return false;
    }
    after_export.contains('=')
}

// ─── Step 5: 卸载 Claude Code CLI ───

async fn step_uninstall_cli(
    app: &AppHandle,
    state: &SessionState,
    cid: &str,
    dry_run: bool,
) -> UninstallStep {
    emit_progress(app, cid, "Step 5/5: 卸载 Claude Code CLI");

    let claude_exists = check_claude_exists();

    if !claude_exists {
        emit_progress(app, cid, "  Claude Code 尚未安装，跳过");
        return UninstallStep {
            name: "卸载 Claude Code CLI".to_string(),
            status: UninstallStepStatus::Skipped,
            message: "Claude Code 尚未安装".to_string(),
        };
    }

    if dry_run {
        emit_progress(
            app,
            cid,
            "  [dry-run] 将执行 npm uninstall -g @anthropic-ai/claude-code",
        );
        return UninstallStep {
            name: "卸载 Claude Code CLI".to_string(),
            status: UninstallStepStatus::Skipped,
            message: "将执行 npm uninstall -g @anthropic-ai/claude-code".to_string(),
        };
    }

    let outcome = match stream_command(
        app,
        state,
        cid,
        "npm",
        &["uninstall", "-g", "@anthropic-ai/claude-code"],
    )
    .await
    {
        Ok(o) => o,
        Err(e) => {
            emit_error_line(app, cid, format!("  ✗ 执行 npm uninstall 失败: {}", e));
            return UninstallStep {
                name: "卸载 Claude Code CLI".to_string(),
                status: UninstallStepStatus::Failed,
                message: format!("执行 npm uninstall 失败: {}", e),
            };
        }
    };

    if outcome.cancelled {
        return UninstallStep {
            name: "卸载 Claude Code CLI".to_string(),
            status: UninstallStepStatus::Failed,
            message: "已取消".to_string(),
        };
    }

    if outcome.success {
        emit_progress(app, cid, "  ✓ Claude Code CLI 已卸载");
        UninstallStep {
            name: "卸载 Claude Code CLI".to_string(),
            status: UninstallStepStatus::Success,
            message: "Claude Code CLI 已成功卸载".to_string(),
        }
    } else {
        emit_error_line(
            app,
            cid,
            format!("  ✗ npm uninstall 失败 (exit_code={:?})", outcome.exit_code),
        );
        UninstallStep {
            name: "卸载 Claude Code CLI".to_string(),
            status: UninstallStepStatus::Failed,
            message: format!("卸载失败 (exit_code={:?})", outcome.exit_code),
        }
    }
}

/// 检查系统中是否存在 claude 命令
#[cfg(not(target_os = "windows"))]
fn check_claude_exists() -> bool {
    Command::new("which")
        .arg("claude")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[cfg(target_os = "windows")]
fn check_claude_exists() -> bool {
    Command::new("where")
        .arg("claude")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_env_export_line_positive() {
        assert!(is_env_export_line("export ANTHROPIC_API_KEY=sk-xxx"));
        assert!(is_env_export_line(
            "export ANTHROPIC_BASE_URL=https://example.com"
        ));
        assert!(is_env_export_line("export CLAUDE_CODE_API_KEY=abc123"));
        assert!(is_env_export_line("  export ANTHROPIC_API_KEY=value"));
    }

    #[test]
    fn test_is_env_export_line_negative() {
        // 没有 export 前缀
        assert!(!is_env_export_line("ANTHROPIC_API_KEY=sk-xxx"));
        // 注释
        assert!(!is_env_export_line("# export ANTHROPIC_API_KEY=sk-xxx"));
        // 不含 =
        assert!(!is_env_export_line("export ANTHROPIC_API_KEY"));
        // 变量名前缀不匹配（没有下划线后缀的部分匹配不算）
        assert!(!is_env_export_line("export ANTHROPIC=value"));
        assert!(!is_env_export_line("export CLAUDE=value"));
        // 其他变量
        assert!(!is_env_export_line("export PATH=/usr/bin"));
        assert!(!is_env_export_line("export NODE_ENV=production"));
    }

    #[test]
    fn test_is_env_export_line_multiple_assignments() {
        // 在同一行有后续赋值仍应匹配
        assert!(is_env_export_line(
            "export ANTHROPIC_BASE_URL=http://a.com ANOTHER_VAR=b"
        ));
    }

    #[test]
    fn test_determine_overall_all_success() {
        let steps = vec![
            UninstallStep {
                name: "s1".into(),
                status: UninstallStepStatus::Success,
                message: "ok".into(),
            },
            UninstallStep {
                name: "s2".into(),
                status: UninstallStepStatus::Success,
                message: "ok".into(),
            },
        ];
        assert_eq!(
            UninstallReport::determine_overall(&steps, false),
            UninstallOverallStatus::Success
        );
    }

    #[test]
    fn test_determine_overall_partial_when_some_failed() {
        let steps = vec![
            UninstallStep {
                name: "s1".into(),
                status: UninstallStepStatus::Success,
                message: "ok".into(),
            },
            UninstallStep {
                name: "s2".into(),
                status: UninstallStepStatus::Failed,
                message: "err".into(),
            },
        ];
        assert_eq!(
            UninstallReport::determine_overall(&steps, false),
            UninstallOverallStatus::Partial
        );
    }

    #[test]
    fn test_determine_overall_failed_when_backup_failed() {
        let steps = vec![
            UninstallStep {
                name: "s1".into(),
                status: UninstallStepStatus::Failed,
                message: "backup failed".into(),
            },
            UninstallStep {
                name: "s2".into(),
                status: UninstallStepStatus::Success,
                message: "ok".into(),
            },
        ];
        assert_eq!(
            UninstallReport::determine_overall(&steps, true),
            UninstallOverallStatus::Failed
        );
    }

    #[test]
    fn test_is_env_export_line_preserves_commented_lines() {
        // 注释掉的 export 行不应该被过滤
        assert!(!is_env_export_line("# export ANTHROPIC_API_KEY=abc"));
        assert!(!is_env_export_line("// export ANTHROPIC_API_KEY=abc"));
    }
}
