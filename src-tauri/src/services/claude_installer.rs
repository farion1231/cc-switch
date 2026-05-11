//! Claude Code 在 macOS 上的安装、检测、更新核心逻辑。
//!
//! 设计原则：把"装"这件事完全交给 Anthropic 官方 install.sh + binary 自带
//! 的 `update` 子命令；本模块只负责：
//!
//! 1. 知道 Claude 装在哪、是怎么装的（[`detect_claude`]）
//! 2. 把 install.sh 跑起来 + 验证装上了（[`run_install_sh`]）
//! 3. 把已装 binary 的 `update` 子命令跑起来 + 比对版本（[`run_claude_update`]）
//!
//! 不做版本号客户端比对、不做 channel 判断 —— 这些都交给 Anthropic 自己的
//! native auto-updater 决定。
//!
//! 仅 macOS。其他平台返回 None / 不可用，由调用端做平台分流。

use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;

use serde::{Deserialize, Serialize};
use tauri::AppHandle;

use crate::services::installer::InstallResult;
use crate::services::stream_command::{
    capture_command, emit_error_line, emit_progress, stream_command, SessionState,
};

// ─── 类型 ──────────────────────────────────────────────────────────────────

/// Claude binary 的安装来源。
///
/// 决定 UI 是否显示"迁移到 native"按钮、决定 `update` 命令该怎么跑（native
/// 走 binary 自带 updater；brew 装的不能直接 `claude update`，得 brew upgrade）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type")]
pub enum InstallMethod {
    /// 通过 install.sh 安装的 native binary —— 后台自动更新可用。
    Native,
    /// 通过 Homebrew 安装。`cask` 是具体的 cask 名（claude-code 或
    /// claude-code@latest），决定卸载/重装命令该用哪个名字。
    Brew { cask: String },
    /// 其他来源：自编译、npm 全局包、未识别 PATH 位置等。当前不主动支持，
    /// 但允许检测到，由 UI 决定怎么提示用户。
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectedClaude {
    pub binary_path: PathBuf,
    pub install_method: InstallMethod,
}

// ─── 探测 ──────────────────────────────────────────────────────────────────

/// 探测 claude 二进制的位置和安装来源。返回 None 表示没装（或所有兜底路径都没命中）。
///
/// 探测顺序（先命中先返回）：
/// 1. PATH 里的 `claude`（依赖 `env_path::fix_path_from_login_shell` 已经把
///    用户 shell 的 PATH 灌进当前进程）
/// 2. `~/.local/bin/claude` —— install.sh 当前默认路径
/// 3. `~/.claude/local/bin/claude` —— install.sh 老路径兜底
/// 4. `/opt/homebrew/bin/claude` —— Apple Silicon brew 默认 prefix
/// 5. `/usr/local/bin/claude` —— Intel brew 默认 prefix
#[cfg(target_os = "macos")]
pub fn detect_claude() -> Option<DetectedClaude> {
    let path = locate_binary()?;
    let method = classify_install_method(&path);
    Some(DetectedClaude {
        binary_path: path,
        install_method: method,
    })
}

#[cfg(not(target_os = "macos"))]
pub fn detect_claude() -> Option<DetectedClaude> {
    None
}

#[cfg(target_os = "macos")]
fn locate_binary() -> Option<PathBuf> {
    if let Some(path) = locate_via_command_v() {
        return Some(path);
    }
    locate_via_known_paths()
}

#[cfg(target_os = "macos")]
fn locate_via_command_v() -> Option<PathBuf> {
    // 用 `command -v` 而非 `which`：`command` 是 POSIX 内建，不依赖外部
    // /usr/bin/which，更可靠。失败/没找到都退回到 known paths 兜底。
    let out = StdCommand::new("/bin/sh")
        .args(["-c", "command -v claude"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let raw = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if raw.is_empty() {
        return None;
    }
    let path = PathBuf::from(raw);
    if path.exists() {
        Some(path)
    } else {
        None
    }
}

#[cfg(target_os = "macos")]
fn locate_via_known_paths() -> Option<PathBuf> {
    let home = std::env::var("HOME").ok()?;
    let candidates = [
        format!("{home}/.local/bin/claude"),
        format!("{home}/.claude/local/bin/claude"),
        "/opt/homebrew/bin/claude".to_string(),
        "/usr/local/bin/claude".to_string(),
    ];
    candidates
        .into_iter()
        .map(PathBuf::from)
        .find(|p| p.exists())
}

#[cfg(target_os = "macos")]
fn classify_install_method(binary_path: &Path) -> InstallMethod {
    let path_str = binary_path.to_string_lossy().to_string();
    let home = std::env::var("HOME").unwrap_or_default();

    // Native：install.sh 的两个已知路径之一
    let native_paths = [
        format!("{home}/.local/bin/claude"),
        format!("{home}/.claude/local/bin/claude"),
    ];
    if native_paths.iter().any(|p| p == &path_str) {
        return InstallMethod::Native;
    }

    // Brew：路径在 brew 前缀下，并且 brew list 能确认确实是 claude-code cask
    let in_brew_prefix = path_str.starts_with("/opt/homebrew/")
        || path_str.starts_with("/usr/local/bin/")
        || path_str.starts_with("/usr/local/Cellar/");
    if in_brew_prefix {
        if let Some(cask) = detect_brew_cask() {
            return InstallMethod::Brew { cask };
        }
    }

    InstallMethod::Other
}

/// 探测当前用户装了哪个 claude-code cask。返回 None 表示 brew 没装这两个
/// 之一（路径在 brew 前缀但来自其他渠道，比如手动放进去的二进制）。
///
/// 优先返回 `claude-code@latest`（用户主动选了 latest channel），其次 `claude-code`。
#[cfg(target_os = "macos")]
fn detect_brew_cask() -> Option<String> {
    let out = StdCommand::new("/bin/sh")
        .args([
            "-c",
            // brew list --cask 输出每行一个 cask 名，没有版本号（跟 --versions 不同）。
            // 这里直接 list + grep 精确匹配两个名字之一。
            r#"brew list --cask 2>/dev/null | grep -E '^claude-code(@latest)?$'"#,
        ])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    let casks: Vec<&str> = stdout
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();

    // 同时装了两个的极端情况：优先 @latest（更新更频繁，用户更可能"想要最新"）
    if casks.contains(&"claude-code@latest") {
        Some("claude-code@latest".to_string())
    } else if casks.contains(&"claude-code") {
        Some("claude-code".to_string())
    } else {
        None
    }
}

// ─── 安装 ──────────────────────────────────────────────────────────────────

/// 跑官方 install.sh 安装/升级 Claude Code，并验证结果。
///
/// 调用方：
/// - `install_claude_code` 命令（用户首次安装）
/// - `migrate_brew_to_native` 流程（brew → native 迁移的安装阶段）
///
/// 验证策略：跑完 install.sh 后立刻 `detect_claude()`，因为 install.sh 把
/// binary 装到 `~/.local/bin/claude`，但 `~/.local/bin` 不一定在当前 PATH
/// 里 —— `detect_claude` 的 known-path 兜底能直接看到这份 binary。
#[cfg(target_os = "macos")]
pub async fn run_install_sh(
    app: &AppHandle,
    state: &SessionState,
    cid: &str,
) -> Result<InstallResult, String> {
    emit_progress(
        app,
        cid,
        "正在执行官方安装脚本：curl -fsSL https://claude.ai/install.sh | bash",
    );

    let outcome = stream_command(
        app,
        state,
        cid,
        "/bin/bash",
        &["-c", "curl -fsSL https://claude.ai/install.sh | bash"],
    )
    .await?;

    if outcome.cancelled {
        return Ok(cancelled_result("Claude Code 安装已取消"));
    }
    if !outcome.success {
        emit_error_line(app, cid, "安装脚本执行失败");
        return Ok(failed_result(
            "claude_install_failed",
            format!(
                "安装脚本执行失败 (exit_code={:?})。请查看上方日志，常见原因：网络不通、curl 不可用、磁盘空间不足。",
                outcome.exit_code
            ),
        ));
    }

    let Some(detected) = detect_claude() else {
        emit_error_line(app, cid, "安装命令已退出 0，但未在常见路径检测到 claude binary");
        return Ok(failed_result(
            "install_verification_failed",
            "安装脚本已执行，但未能在 ~/.local/bin/ 或 PATH 中检测到 claude。请重启终端后再试，或在「关于」页重新诊断。"
                .to_string(),
        ));
    };

    let version = capture_version(state, &detected.binary_path).await;
    let success_msg = format!(
        "✓ Claude Code 已就绪~ 可以尽情的 AI Coding 了~~ ({})",
        version.as_deref().unwrap_or("未知版本")
    );
    emit_progress(app, cid, success_msg.clone());

    Ok(InstallResult {
        success: true,
        message: success_msg,
        installed_version: version,
        action: Some("install".to_string()),
        already_installed: Some(false),
        verified: Some(true),
        error_code: None,
    })
}

/// 调用 binary 自带的 `update` 子命令让它自己判断升不升、升到哪。
/// 升完拿一次 `--version` 比对，告诉用户究竟动没动版本。
///
/// 适用：`detected.install_method == Native`。Brew 装的别走这里
/// （应该走 `brew upgrade --cask <name>`，由 UI 分流）。
#[cfg(target_os = "macos")]
pub async fn run_claude_update(
    app: &AppHandle,
    state: &SessionState,
    cid: &str,
    detected: &DetectedClaude,
) -> Result<InstallResult, String> {
    let prev_version = capture_version(state, &detected.binary_path).await;
    emit_progress(
        app,
        cid,
        format!(
            "当前版本：{}",
            prev_version.as_deref().unwrap_or("未知")
        ),
    );

    // 路径用 shell 单引号包裹防 $ / 空格 等字符；单引号自身做经典转义。
    let binary_str = detected.binary_path.to_string_lossy();
    let escaped = binary_str.replace('\'', r#"'"'"'"#);
    let cmd = format!("'{escaped}' update");

    emit_progress(app, cid, format!("正在执行：{cmd}"));

    let outcome = stream_command(app, state, cid, "/bin/bash", &["-c", &cmd]).await?;

    if outcome.cancelled {
        return Ok(cancelled_result("更新已取消"));
    }
    if !outcome.success {
        emit_error_line(app, cid, "claude update 退出码非 0");
        return Ok(failed_result(
            "claude_update_failed",
            format!(
                "更新失败 (exit_code={:?})。请查看上方日志。",
                outcome.exit_code
            ),
        ));
    }

    let new_version = capture_version(state, &detected.binary_path).await;
    let upgraded = matches!(
        (&prev_version, &new_version),
        (Some(a), Some(b)) if a != b
    );

    let msg = if upgraded {
        format!(
            "✓ 已升级 {} → {}",
            prev_version.as_deref().unwrap_or("?"),
            new_version.as_deref().unwrap_or("?")
        )
    } else {
        format!(
            "✓ 已是最新版本：{}",
            new_version.as_deref().unwrap_or("?")
        )
    };
    emit_progress(app, cid, msg.clone());

    Ok(InstallResult {
        success: true,
        message: msg,
        installed_version: new_version,
        action: Some(if upgraded { "upgrade" } else { "none" }.to_string()),
        already_installed: Some(!upgraded),
        verified: Some(true),
        error_code: None,
    })
}

// ─── 内部 helpers ──────────────────────────────────────────────────────────

#[cfg(target_os = "macos")]
async fn capture_version(state: &SessionState, binary: &Path) -> Option<String> {
    let escaped = binary.to_string_lossy().replace('\'', r#"'"'"'"#);
    let cmd = format!("'{escaped}' --version");
    let (ok, stdout, _) = capture_command(state, "/bin/bash", &["-c", &cmd])
        .await
        .ok()?;
    if !ok {
        return None;
    }
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(extract_version(trimmed))
    }
}

#[cfg(target_os = "macos")]
fn extract_version(raw: &str) -> String {
    if let Ok(re) = regex::Regex::new(r"\d+\.\d+\.\d+(-[\w.]+)?") {
        if let Some(m) = re.find(raw) {
            return m.as_str().to_string();
        }
    }
    raw.trim().to_string()
}

#[cfg(target_os = "macos")]
fn cancelled_result(message: &str) -> InstallResult {
    InstallResult {
        success: false,
        message: message.to_string(),
        installed_version: None,
        action: None,
        already_installed: None,
        verified: Some(false),
        error_code: Some("cancelled".to_string()),
    }
}

#[cfg(target_os = "macos")]
fn failed_result(error_code: &str, message: String) -> InstallResult {
    InstallResult {
        success: false,
        message,
        installed_version: None,
        action: Some("install".to_string()),
        already_installed: Some(false),
        verified: Some(false),
        error_code: Some(error_code.to_string()),
    }
}

// ─── 单元测试 ──────────────────────────────────────────────────────────────

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    #[test]
    fn classify_native_paths() {
        let home = std::env::var("HOME").unwrap();
        let p = PathBuf::from(format!("{home}/.local/bin/claude"));
        // 这里只测分类逻辑，不依赖 brew 状态：纯字符串匹配命中 Native。
        assert_eq!(classify_install_method(&p), InstallMethod::Native);

        let p = PathBuf::from(format!("{home}/.claude/local/bin/claude"));
        assert_eq!(classify_install_method(&p), InstallMethod::Native);
    }

    #[test]
    fn classify_unknown_path_returns_other() {
        // 一条 brew 前缀外、native 前缀外的 PATH，应当落到 Other。
        let p = PathBuf::from("/opt/some-other-pm/bin/claude");
        assert_eq!(classify_install_method(&p), InstallMethod::Other);
    }

    #[test]
    fn extract_version_finds_semver() {
        assert_eq!(extract_version("claude 2.1.138"), "2.1.138");
        assert_eq!(extract_version("v3.14.2-beta.1"), "3.14.2-beta.1");
        // 没有 semver 时退回原文本（用于诊断异常输出）
        assert_eq!(extract_version("error: not installed"), "error: not installed");
    }
}
