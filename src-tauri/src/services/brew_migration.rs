//! 把通过 Homebrew 安装的 Claude Code 迁移到官方 native install.sh 安装。
//!
//! 为什么要做：官方 `install.sh` 安装的 binary 支持后台自动更新，brew cask
//! 不会自动更新。生产用户对"自动用上最新版"的诉求很强，主动给一条迁移路径
//! 比让用户手动卸载重装更安全（带备份+回滚）。
//!
//! 流程（每步都流式 emit 日志）：
//!
//! ```text
//! [1/4] 备份 ~/.claude.json + ~/.claude/settings.json
//!       到 ~/.cc-doctor/backups/migration-{timestamp}/
//! [2/4] brew uninstall --cask {cask}
//! [3/4] curl -fsSL https://claude.ai/install.sh | bash
//! [4/4] detect_claude 验证新装的 binary 在 ~/.local/bin/claude
//! ```
//!
//! 任何步骤失败都自动 `brew install --cask {原 cask}` 回滚。备份目录无论成败
//! 都保留在磁盘上，方便用户用 `~/.cc-doctor/backups/` 找到。

use std::fs;
use std::path::{Path, PathBuf};

use chrono::Local;
use serde::{Deserialize, Serialize};
use tauri::AppHandle;

use crate::services::claude_installer::{detect_claude, run_install_sh, InstallMethod};
use crate::services::stream_command::{
    emit_error_line, emit_progress, stream_command, SessionState,
};

// ─── 数据结构 ──────────────────────────────────────────────────────────────

/// 迁移前的 dry-run 预览，给 UI 弹"确认迁移"对话框用。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationPreview {
    /// 当前安装的 brew cask 名（claude-code 或 claude-code@latest），
    /// 决定卸载/回滚命令该用哪个名字。
    pub cask_name: String,
    /// 当前 brew binary 路径，给用户看一眼"动哪个"。
    pub current_binary_path: String,
    /// 备份目标目录绝对路径（执行迁移时会创建）。
    pub backup_target_dir: String,
    /// 会被备份的文件清单（已存在的才列出）。
    pub backup_items: Vec<String>,
    /// 各步骤说明（仅文案，不含执行状态）。
    pub steps: Vec<MigrationStepInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationStepInfo {
    pub name: String,
    pub description: String,
}

/// 迁移执行结果。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationResult {
    /// 备份目录（迁移开始就建好；建失败则为 None）。
    pub backup_path: Option<String>,
    /// 各步骤执行情况。
    pub steps: Vec<MigrationStep>,
    /// 整体状态。
    pub overall: MigrationOverallStatus,
    /// 是否触发了 brew 回滚（brew uninstall 之后的步骤失败时为 true）。
    pub rolled_back: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationStep {
    pub name: String,
    pub status: MigrationStepStatus,
    pub message: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub enum MigrationStepStatus {
    Success,
    Skipped,
    Failed,
}

/// 整体状态。
///
/// - `Success`：4 步全过，新 native 已就绪。
/// - `RolledBack`：中途失败但成功回滚到原 brew 安装，用户当前可继续用 brew 版。
/// - `Failed`：失败且未触发回滚（备份/前置检查阶段失败），或回滚也失败了
///   （这种情况备份目录是用户的最后保险）。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub enum MigrationOverallStatus {
    Success,
    RolledBack,
    Failed,
}

impl MigrationStep {
    fn success(name: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            status: MigrationStepStatus::Success,
            message: message.into(),
        }
    }

    fn failed(name: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            status: MigrationStepStatus::Failed,
            message: message.into(),
        }
    }
}

// ─── 公共 API ──────────────────────────────────────────────────────────────

/// 构造迁移预览，由 `preview_brew_migration` 命令调用。
///
/// 失败原因（返回 `Err(String)`）：
/// - 未检测到已装的 Claude Code
/// - 检测到的 Claude Code 不是 brew 安装（不需要迁移）
#[cfg(target_os = "macos")]
pub fn build_migration_preview() -> Result<MigrationPreview, String> {
    let detected = detect_claude().ok_or_else(|| {
        "未检测到已安装的 Claude Code。请先安装。".to_string()
    })?;

    let cask = match detected.install_method {
        InstallMethod::Brew { cask } => cask,
        InstallMethod::Native => {
            return Err("当前已经是官方 native 安装，无需迁移。".to_string());
        }
        InstallMethod::Other => {
            return Err(
                "当前 Claude Code 不是通过 Homebrew 安装，无法用本流程迁移。\
                 请手动卸载后通过「一键安装」装官方版。"
                    .to_string(),
            );
        }
    };

    let backup_dir = make_backup_dir_path();
    let backup_items = enumerate_backup_items();

    let steps = vec![
        MigrationStepInfo {
            name: "1/4 备份用户配置".into(),
            description: format!("将复制 {} 个配置文件到备份目录", backup_items.len()),
        },
        MigrationStepInfo {
            name: format!("2/4 brew uninstall --cask {}", cask),
            description: "卸载 Homebrew 版本，保留用户配置不动".into(),
        },
        MigrationStepInfo {
            name: "3/4 执行官方 install.sh".into(),
            description: "下载 native binary 到 ~/.local/bin/claude".into(),
        },
        MigrationStepInfo {
            name: "4/4 验证 native 安装".into(),
            description: "失败则自动 brew install 回滚到原版本".into(),
        },
    ];

    Ok(MigrationPreview {
        cask_name: cask,
        current_binary_path: detected.binary_path.to_string_lossy().to_string(),
        backup_target_dir: backup_dir.to_string_lossy().to_string(),
        backup_items,
        steps,
    })
}

/// 执行迁移。流式 emit 日志，由 `migrate_brew_to_native` 命令调用。
///
/// 任何"已经动了 brew uninstall 但后续失败"的情况都自动尝试 `brew install`
/// 回滚。备份目录无论结局如何都保留在磁盘上。
#[cfg(target_os = "macos")]
pub async fn execute_migration(
    app: &AppHandle,
    state: &SessionState,
    cid: &str,
) -> Result<MigrationResult, String> {
    let mut steps = Vec::new();

    // ─── 前置检查 ───
    let preview = match build_migration_preview() {
        Ok(p) => p,
        Err(e) => {
            emit_error_line(app, cid, e.clone());
            steps.push(MigrationStep::failed("前置检查", e));
            return Ok(MigrationResult {
                backup_path: None,
                steps,
                overall: MigrationOverallStatus::Failed,
                rolled_back: false,
            });
        }
    };

    let cask = preview.cask_name.clone();
    let backup_dir = PathBuf::from(&preview.backup_target_dir);
    let backup_dir_str = preview.backup_target_dir.clone();

    // ─── [1/4] 备份 ───
    emit_progress(app, cid, format!("[1/4] 创建备份目录: {}", backup_dir.display()));
    if let Err(e) = fs::create_dir_all(&backup_dir) {
        let msg = format!("创建备份目录失败: {e}");
        emit_error_line(app, cid, msg.clone());
        steps.push(MigrationStep::failed("创建备份目录", msg));
        return Ok(MigrationResult {
            backup_path: None,
            steps,
            overall: MigrationOverallStatus::Failed,
            rolled_back: false,
        });
    }

    match backup_user_config(&backup_dir, &preview.backup_items) {
        Ok(copied) => {
            emit_progress(
                app,
                cid,
                format!("✓ 已备份 {copied} 个配置文件到 {}", backup_dir.display()),
            );
            steps.push(MigrationStep::success(
                "备份用户配置",
                format!("已备份 {copied} 个文件"),
            ));
        }
        Err(e) => {
            emit_error_line(app, cid, e.clone());
            steps.push(MigrationStep::failed("备份用户配置", e));
            return Ok(MigrationResult {
                backup_path: Some(backup_dir_str),
                steps,
                overall: MigrationOverallStatus::Failed,
                rolled_back: false,
            });
        }
    }

    // ─── [2/4] brew uninstall ───
    emit_progress(app, cid, format!("[2/4] brew uninstall --cask {cask}"));
    let cask_arg = shell_quote_simple(&cask);
    let outcome = stream_command(
        app,
        state,
        cid,
        "/bin/bash",
        &["-c", &format!("brew uninstall --cask {cask_arg}")],
    )
    .await?;
    if outcome.cancelled {
        steps.push(MigrationStep::failed("brew uninstall", "已取消"));
        return Ok(MigrationResult {
            backup_path: Some(backup_dir_str),
            steps,
            overall: MigrationOverallStatus::Failed,
            rolled_back: false,
        });
    }
    if !outcome.success {
        let msg = format!(
            "brew uninstall 失败 (exit_code={:?})。原 brew 安装未受影响。",
            outcome.exit_code
        );
        emit_error_line(app, cid, msg.clone());
        steps.push(MigrationStep::failed("brew uninstall", msg));
        return Ok(MigrationResult {
            backup_path: Some(backup_dir_str),
            steps,
            overall: MigrationOverallStatus::Failed,
            rolled_back: false,
        });
    }
    steps.push(MigrationStep::success(
        "brew uninstall",
        format!("已卸载 {cask}"),
    ));

    // ─── [3/4] install.sh ───
    emit_progress(app, cid, "[3/4] 通过 install.sh 安装官方 native 版本");
    let install = run_install_sh(app, state, cid).await?;
    if !install.success {
        let rollback_ok = rollback_brew_install(app, state, cid, &cask).await?;
        let msg = if rollback_ok {
            format!("install.sh 失败：{}。已回滚到 brew 安装。", install.message)
        } else {
            format!(
                "install.sh 失败：{}。回滚到 brew 也失败！请用备份目录手动恢复：{}",
                install.message, backup_dir_str
            )
        };
        emit_error_line(app, cid, msg.clone());
        steps.push(MigrationStep::failed("install.sh", msg));
        return Ok(MigrationResult {
            backup_path: Some(backup_dir_str),
            steps,
            overall: if rollback_ok {
                MigrationOverallStatus::RolledBack
            } else {
                MigrationOverallStatus::Failed
            },
            rolled_back: rollback_ok,
        });
    }
    steps.push(MigrationStep::success("install.sh", install.message.clone()));

    // ─── [4/4] 验证 ───
    emit_progress(app, cid, "[4/4] 验证 native binary 路径");
    let verified = detect_claude();
    let is_native = matches!(
        verified.as_ref().map(|d| &d.install_method),
        Some(InstallMethod::Native)
    );

    if !is_native {
        let rollback_ok = rollback_brew_install(app, state, cid, &cask).await?;
        let msg = if rollback_ok {
            "未在 native 路径检测到 binary，已回滚到 brew 安装。".to_string()
        } else {
            format!(
                "未在 native 路径检测到 binary，回滚也失败！请用备份目录手动恢复：{backup_dir_str}"
            )
        };
        emit_error_line(app, cid, msg.clone());
        steps.push(MigrationStep::failed("验证 native 安装", msg));
        return Ok(MigrationResult {
            backup_path: Some(backup_dir_str),
            steps,
            overall: if rollback_ok {
                MigrationOverallStatus::RolledBack
            } else {
                MigrationOverallStatus::Failed
            },
            rolled_back: rollback_ok,
        });
    }

    let final_path = verified
        .as_ref()
        .map(|d| d.binary_path.to_string_lossy().to_string())
        .unwrap_or_default();
    steps.push(MigrationStep::success(
        "验证 native 安装",
        format!("当前 binary: {final_path}"),
    ));

    emit_progress(
        app,
        cid,
        "✓ 已迁移到官方 native 安装。Claude Code 已就绪~ 可以尽情的 AI Coding 了~~",
    );

    Ok(MigrationResult {
        backup_path: Some(backup_dir_str),
        steps,
        overall: MigrationOverallStatus::Success,
        rolled_back: false,
    })
}

// ─── 内部 helpers ──────────────────────────────────────────────────────────

#[cfg(target_os = "macos")]
fn make_backup_dir_path() -> PathBuf {
    let app_config_dir = crate::config::get_app_config_dir();
    let ts = Local::now().format("%Y%m%d-%H%M%S");
    app_config_dir
        .join("backups")
        .join(format!("migration-{ts}"))
}

#[cfg(target_os = "macos")]
fn enumerate_backup_items() -> Vec<String> {
    let home = match std::env::var("HOME") {
        Ok(h) => h,
        Err(_) => return Vec::new(),
    };
    [
        format!("{home}/.claude.json"),
        format!("{home}/.claude/settings.json"),
    ]
    .into_iter()
    .filter(|p| Path::new(p).exists())
    .collect()
}

/// 备份用户关键配置。返回实际复制的文件数。
///
/// 策略：只备份"用户态配置文件"（小，关键），**不备份**整个 `~/.claude/`
/// 目录（含 sessions / projects / mcp 状态等，可能上百 MB；且 install.sh
/// 不会动这个目录）。
#[cfg(target_os = "macos")]
fn backup_user_config(backup_dir: &Path, items: &[String]) -> Result<usize, String> {
    let mut copied = 0;
    for src_str in items {
        let src = Path::new(src_str);
        if !src.exists() {
            continue;
        }
        let file_name = src
            .file_name()
            .ok_or_else(|| format!("无法获取文件名: {src_str}"))?;
        let dst = backup_dir.join(file_name);
        fs::copy(src, &dst).map_err(|e| {
            format!("备份 {} → {} 失败: {e}", src.display(), dst.display())
        })?;
        copied += 1;
    }
    Ok(copied)
}

#[cfg(target_os = "macos")]
async fn rollback_brew_install(
    app: &AppHandle,
    state: &SessionState,
    cid: &str,
    cask: &str,
) -> Result<bool, String> {
    emit_progress(app, cid, format!("[回滚] brew install --cask {cask}"));
    let outcome = stream_command(
        app,
        state,
        cid,
        "/bin/bash",
        &[
            "-c",
            &format!("brew install --cask {}", shell_quote_simple(cask)),
        ],
    )
    .await?;
    Ok(outcome.success && !outcome.cancelled)
}

/// 简单 shell 转义：cask 名按文档只允许 `[a-zA-Z0-9._@-]`，命中即原样输出；
/// 其他情况下用单引号包裹防御。Claude Code 的两个 cask 名都满足前者。
#[cfg(target_os = "macos")]
fn shell_quote_simple(s: &str) -> String {
    if !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '@' | '-'))
    {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', r#"'"'"'"#))
    }
}

// ─── 单元测试 ──────────────────────────────────────────────────────────────

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    #[test]
    fn shell_quote_passes_through_safe_chars() {
        assert_eq!(shell_quote_simple("claude-code"), "claude-code");
        assert_eq!(shell_quote_simple("claude-code@latest"), "claude-code@latest");
    }

    #[test]
    fn shell_quote_wraps_unsafe_chars() {
        assert_eq!(shell_quote_simple("foo bar"), "'foo bar'");
        assert_eq!(shell_quote_simple("a;rm -rf /"), "'a;rm -rf /'");
    }

    #[test]
    fn shell_quote_escapes_single_quotes() {
        assert_eq!(shell_quote_simple("it's"), r#"'it'"'"'s'"#);
    }

    #[test]
    fn migration_step_constructors_set_status() {
        let s = MigrationStep::success("name", "msg");
        assert_eq!(s.status, MigrationStepStatus::Success);
        assert_eq!(s.name, "name");

        let f = MigrationStep::failed("x", "y");
        assert_eq!(f.status, MigrationStepStatus::Failed);
    }
}
