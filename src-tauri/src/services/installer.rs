/// 工具安装服务
///
/// 提供自动化安装 Claude Code 及其依赖（Node.js）的能力。
/// 所有安装操作都通过 shell 命令执行，支持 macOS、Linux 平台。
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::process::Command;
use tauri::AppHandle;

use crate::services::stream_command::{
    capture_command, emit_error_line, emit_progress, stream_command, SessionState,
};

// 以下 sync 命令运行抽象仅供 tests 模块的 install_windows_package_with_runner 单测使用。
// 生产 Windows 路径已切换到 install_windows_package_streaming，但保留这些类型可以
// 让现有的 FakeCommandRunner 单测继续验证 primary/fallback 选择逻辑。
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandSpec {
    pub program: String,
    pub args: Vec<String>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandResult {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

#[allow(dead_code)]
trait CommandRunner {
    fn run(&mut self, spec: &CommandSpec) -> Result<CommandResult, String>;
}

#[allow(dead_code)]
struct SystemCommandRunner;

impl CommandRunner for SystemCommandRunner {
    fn run(&mut self, spec: &CommandSpec) -> Result<CommandResult, String> {
        let output = Command::new(&spec.program)
            .args(&spec.args)
            .output()
            .map_err(|e| {
                format!(
                    "执行命令失败 ({} {}): {}",
                    spec.program,
                    spec.args.join(" "),
                    e
                )
            })?;

        Ok(CommandResult {
            success: output.status.success(),
            stdout: String::from_utf8_lossy(&output.stdout).trim().to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        })
    }
}

/// 安装结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstallResult {
    /// 是否安装成功
    pub success: bool,
    /// 安装结果消息
    pub message: String,
    /// 安装后的版本号（如果成功）
    pub installed_version: Option<String>,
    /// 本次动作：install / upgrade / none
    pub action: Option<String>,
    /// 是否已经安装，无需重复执行
    pub already_installed: Option<bool>,
    /// 是否通过安装后验证
    pub verified: Option<bool>,
    /// 机器可读的错误码
    pub error_code: Option<String>,
}

/// 检查 Node.js 是否已安装
///
/// # Returns
/// - `Ok(true)` - Node.js 已安装
/// - `Ok(false)` - Node.js 未安装
/// - `Err(String)` - 检测失败
pub fn check_nodejs_installed() -> Result<bool, String> {
    #[cfg(target_os = "windows")]
    {
        let output = Command::new("cmd")
            .args(["/C", "node --version"])
            .output()
            .map_err(|e| format!("执行 node 命令失败: {}", e))?;

        Ok(output.status.success())
    }

    #[cfg(not(target_os = "windows"))]
    {
        let output = Command::new("/bin/bash")
            .arg("-c")
            .arg("node --version")
            .output()
            .map_err(|e| format!("执行 node 命令失败: {}", e))?;

        Ok(output.status.success())
    }
}

/// 检查 Node.js 版本是否满足要求（>= 18.0.0）
///
/// # Returns
/// - `Ok(true)` - 版本满足要求
/// - `Ok(false)` - 版本不满足要求或未安装
/// - `Err(String)` - 检测失败
pub fn check_nodejs_version_sufficient() -> Result<bool, String> {
    #[cfg(target_os = "windows")]
    {
        let output = Command::new("cmd")
            .args(["/C", "node --version"])
            .output()
            .map_err(|e| format!("执行 node 命令失败: {}", e))?;

        if !output.status.success() {
            return Ok(false);
        }

        let version_str = String::from_utf8_lossy(&output.stdout);
        parse_and_check_node_version(&version_str)
    }

    #[cfg(not(target_os = "windows"))]
    {
        let output = Command::new("/bin/bash")
            .arg("-c")
            .arg("node --version")
            .output()
            .map_err(|e| format!("执行 node 命令失败: {}", e))?;

        if !output.status.success() {
            return Ok(false);
        }

        let version_str = String::from_utf8_lossy(&output.stdout);
        parse_and_check_node_version(&version_str)
    }
}

/// 解析并检查 Node.js 版本是否 >= 18.0.0
fn parse_and_check_node_version(version_str: &str) -> Result<bool, String> {
    // 版本格式: v18.20.0 或 18.20.0
    let version = version_str.trim().trim_start_matches('v');
    let parts: Vec<&str> = version.split('.').collect();

    if parts.is_empty() {
        return Err(format!("无法解析 Node.js 版本: {}", version_str));
    }

    let major = parts[0]
        .parse::<u32>()
        .map_err(|_| format!("无法解析主版本号: {}", parts[0]))?;

    Ok(major >= 18)
}

/// 检查 Homebrew 是否已安装（仅 macOS）
#[cfg(target_os = "macos")]
fn check_homebrew_installed() -> Result<bool, String> {
    let output = Command::new("/bin/bash")
        .arg("-c")
        .arg("which brew")
        .output()
        .map_err(|e| format!("检测 Homebrew 失败: {}", e))?;

    Ok(output.status.success())
}

/// 通过 Homebrew 流式安装 Node.js（仅 macOS）
#[cfg(target_os = "macos")]
pub async fn install_nodejs(
    app: &AppHandle,
    state: &SessionState,
    channel_id: &str,
) -> Result<InstallResult, String> {
    emit_progress(app, channel_id, "[1/3] 检测 Homebrew 是否可用...");
    if !check_homebrew_installed()? {
        emit_error_line(app, channel_id, "Homebrew 未安装");
        return Ok(InstallResult {
            success: false,
            message: "Homebrew 未安装，请先安装 Homebrew: https://brew.sh".to_string(),
            installed_version: None,
            action: None,
            already_installed: None,
            verified: Some(false),
            error_code: Some("missing_homebrew".to_string()),
        });
    }

    emit_progress(
        app,
        channel_id,
        "[2/3] 执行 brew install node（可能需要数分钟）...",
    );
    let outcome = stream_command(
        app,
        state,
        channel_id,
        "/bin/bash",
        &["-c", "brew install node"],
    )
    .await?;

    if outcome.cancelled {
        return Ok(cancelled_result("Node.js 安装已取消"));
    }
    if !outcome.success {
        emit_error_line(app, channel_id, "brew install node 失败");
        return Ok(InstallResult {
            success: false,
            message: format!(
                "安装 Node.js 失败 (exit_code={:?})。请查看上方日志。",
                outcome.exit_code
            ),
            installed_version: None,
            action: None,
            already_installed: None,
            verified: Some(false),
            error_code: Some("node_install_failed".to_string()),
        });
    }

    emit_progress(app, channel_id, "[3/3] 验证 node --version...");
    let (ok, stdout, _) = capture_command(state, "/bin/bash", &["-c", "node --version"]).await?;
    let version = if ok {
        Some(stdout.trim().to_string())
    } else {
        None
    };
    emit_progress(
        app,
        channel_id,
        format!(
            "✓ Node.js 安装成功: {}",
            version.as_deref().unwrap_or("未知")
        ),
    );

    Ok(InstallResult {
        success: true,
        message: "Node.js 安装成功".to_string(),
        installed_version: version,
        action: Some("install".to_string()),
        already_installed: Some(false),
        verified: Some(true),
        error_code: None,
    })
}

/// Linux 平台安装 Node.js（提示用户手动安装）
#[cfg(target_os = "linux")]
pub async fn install_nodejs(
    app: &AppHandle,
    _state: &SessionState,
    channel_id: &str,
) -> Result<InstallResult, String> {
    let msg = "Linux 平台请使用包管理器手动安装 Node.js:\n\
              - Ubuntu/Debian: sudo apt install nodejs npm\n\
              - Fedora: sudo dnf install nodejs\n\
              - Arch: sudo pacman -S nodejs npm";
    emit_error_line(app, channel_id, msg);
    Ok(InstallResult {
        success: false,
        message: msg.to_string(),
        installed_version: None,
        action: None,
        already_installed: None,
        verified: Some(false),
        error_code: Some("manual_node_install_required".to_string()),
    })
}

/// Windows 平台流式安装 Node.js（优先使用 winget，其次使用 Chocolatey）
#[cfg(target_os = "windows")]
pub async fn install_nodejs(
    app: &AppHandle,
    state: &SessionState,
    channel_id: &str,
) -> Result<InstallResult, String> {
    install_windows_package_streaming(
        app,
        state,
        channel_id,
        "Node.js",
        &[&[
            "winget",
            "install",
            "--id",
            "OpenJS.NodeJS.LTS",
            "-e",
            "--accept-source-agreements",
            "--accept-package-agreements",
            "--disable-interactivity",
        ]],
        &[&["choco", "install", "nodejs-lts", "-y"]],
        &["cmd", "/C", "node --version"],
        "manual_node_install_required",
        "Windows 平台自动安装 Node.js 失败。请先确认已安装 winget 或 Chocolatey，并以管理员身份重试；或前往 https://nodejs.org 手动安装。",
    )
    .await
}

/// 流式安装 Claude Code（macOS/Linux）
///
/// 执行官方安装脚本: `curl -fsSL https://claude.ai/install.sh | bash`
#[cfg(not(target_os = "windows"))]
pub async fn install_claude_code(
    app: &AppHandle,
    state: &SessionState,
    channel_id: &str,
) -> Result<InstallResult, String> {
    emit_progress(app, channel_id, "[1/2] 下载并执行 Claude Code 安装脚本...");
    let outcome = stream_command(
        app,
        state,
        channel_id,
        "/bin/bash",
        &["-c", "curl -fsSL https://claude.ai/install.sh | bash"],
    )
    .await?;

    if outcome.cancelled {
        return Ok(cancelled_result("Claude Code 安装已取消"));
    }
    if !outcome.success {
        emit_error_line(app, channel_id, "安装脚本执行失败");
        return Ok(InstallResult {
            success: false,
            message: format!(
                "安装 Claude Code 失败 (exit_code={:?})。请查看上方日志。",
                outcome.exit_code
            ),
            installed_version: None,
            action: Some("install".to_string()),
            already_installed: Some(false),
            verified: Some(false),
            error_code: Some("claude_install_failed".to_string()),
        });
    }

    emit_progress(app, channel_id, "[2/2] 验证 claude --version...");
    let (ok, stdout, _) = capture_command(state, "/bin/bash", &["-c", "claude --version"]).await?;
    let version = if ok {
        Some(extract_version(&stdout))
    } else {
        None
    };
    emit_progress(
        app,
        channel_id,
        format!(
            "✓ Claude Code 安装成功: {}",
            version.as_deref().unwrap_or("未知")
        ),
    );

    Ok(InstallResult {
        success: true,
        message: "Claude Code 安装成功".to_string(),
        installed_version: version,
        action: Some("install".to_string()),
        already_installed: Some(false),
        verified: Some(true),
        error_code: None,
    })
}

/// Windows 平台流式安装 Claude Code（通过 npm）
#[cfg(target_os = "windows")]
pub async fn install_claude_code(
    app: &AppHandle,
    state: &SessionState,
    channel_id: &str,
) -> Result<InstallResult, String> {
    install_windows_package_streaming(
        app,
        state,
        channel_id,
        "Claude Code",
        &[
            &["cmd", "/C", "npm install -g @anthropic-ai/claude-code"],
            &[
                "cmd",
                "/C",
                "npm install -g @anthropic-ai/claude-code@latest",
            ],
        ],
        &[],
        &["cmd", "/C", "claude --version"],
        "manual_claude_install_required",
        "Windows 平台自动安装 Claude Code 失败。请先确认 Node.js/npm 可用，并以管理员身份重试；或前往 https://claude.ai/download 手动安装。",
    )
    .await
}

/// 取消时统一构造的 InstallResult
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

/// 从版本输出中提取纯版本号
///
/// 例如: "claude 1.0.20" -> "1.0.20"
fn extract_version(raw: &str) -> String {
    let re = Regex::new(r"\d+\.\d+\.\d+(-[\w.]+)?").expect("Invalid version regex");
    re.find(raw)
        .map(|m| m.as_str().to_string())
        .unwrap_or_else(|| raw.trim().to_string())
}

/// Windows 平台流式安装：依次尝试 primary → fallback 命令，每条命令都通过
/// stream_command 把 stdout/stderr 实时推送给前端。第一条成功后即停止。
#[cfg(target_os = "windows")]
async fn install_windows_package_streaming(
    app: &AppHandle,
    state: &SessionState,
    channel_id: &str,
    display_name: &str,
    primary_commands: &[&[&str]],
    fallback_commands: &[&[&str]],
    verify_command: &[&str],
    failure_error_code: &str,
    manual_message: &str,
) -> Result<InstallResult, String> {
    let mut executed: Option<String> = None;

    for cmd in primary_commands.iter().chain(fallback_commands.iter()) {
        let Some((program, args)) = cmd.split_first() else {
            continue;
        };
        emit_progress(
            app,
            channel_id,
            format!("尝试安装 {} via {}...", display_name, program),
        );
        let outcome = stream_command(app, state, channel_id, program, args).await?;
        if outcome.cancelled {
            return Ok(cancelled_result(&format!("{} 安装已取消", display_name)));
        }
        if outcome.success {
            executed = Some(format!("{} {}", program, args.join(" ")));
            break;
        }
    }

    let Some(executed_command) = executed else {
        emit_error_line(app, channel_id, manual_message);
        return Ok(InstallResult {
            success: false,
            message: manual_message.to_string(),
            installed_version: None,
            action: None,
            already_installed: Some(false),
            verified: Some(false),
            error_code: Some(failure_error_code.to_string()),
        });
    };

    emit_progress(app, channel_id, format!("验证 {} 安装...", display_name));
    let (program, args) = verify_command
        .split_first()
        .ok_or_else(|| "verify_command 不能为空".to_string())?;
    let (ok, stdout, stderr) = capture_command(state, program, args).await?;
    let raw = if stdout.trim().is_empty() {
        stderr
    } else {
        stdout
    };
    let installed_version = if ok && !raw.trim().is_empty() {
        Some(extract_version(&raw))
    } else {
        None
    };

    if installed_version.is_none() {
        emit_error_line(
            app,
            channel_id,
            format!("{} 安装命令已执行，但未检测到可用版本", display_name),
        );
        return Ok(InstallResult {
            success: false,
            message: format!(
                "{} 安装命令已执行，但暂未检测到可用版本。请重新打开终端后重试。",
                display_name
            ),
            installed_version: None,
            action: Some("install".to_string()),
            already_installed: Some(false),
            verified: Some(false),
            error_code: Some("install_verification_failed".to_string()),
        });
    }

    emit_progress(
        app,
        channel_id,
        format!(
            "✓ {} 安装成功: {}",
            display_name,
            installed_version.as_deref().unwrap_or("未知")
        ),
    );

    Ok(InstallResult {
        success: true,
        message: format!("{} 安装成功 ({})", display_name, executed_command),
        installed_version,
        action: Some("install".to_string()),
        already_installed: Some(false),
        verified: Some(true),
        error_code: None,
    })
}

#[cfg(target_os = "windows")]
fn run_windows_command(args: &[&str]) -> Result<std::process::Output, String> {
    let (program, rest) = args
        .split_first()
        .ok_or_else(|| "缺少可执行命令".to_string())?;

    Command::new(program)
        .args(rest)
        .output()
        .map_err(|e| format!("执行命令失败 ({}): {}", args.join(" "), e))
}

#[cfg(target_os = "windows")]
fn verify_windows_command(version_check: &str) -> Option<String> {
    let output = run_windows_command(&["cmd", "/C", version_check]).ok()?;
    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let raw = if stdout.is_empty() { stderr } else { stdout };

    if raw.is_empty() {
        None
    } else {
        Some(extract_version(&raw))
    }
}

#[cfg(target_os = "windows")]
fn try_windows_install_commands(commands: &[&[&str]]) -> Result<Option<String>, String> {
    for command in commands {
        let output = run_windows_command(command)?;
        if output.status.success() {
            return Ok(Some(command.join(" ")));
        }
    }

    Ok(None)
}

#[cfg(target_os = "windows")]
fn install_windows_package(
    display_name: &str,
    primary_commands: &[&[&str]],
    fallback_commands: &[&[&str]],
    version_check: &str,
    failure_error_code: &str,
    manual_message: &str,
) -> Result<InstallResult, String> {
    let primary = try_windows_install_commands(primary_commands)?;
    let fallback = if primary.is_none() {
        try_windows_install_commands(fallback_commands)?
    } else {
        None
    };

    let Some(executed_command) = primary.or(fallback) else {
        return Ok(InstallResult {
            success: false,
            message: manual_message.to_string(),
            installed_version: None,
            action: None,
            already_installed: Some(false),
            verified: Some(false),
            error_code: Some(failure_error_code.to_string()),
        });
    };

    let installed_version = verify_windows_command(version_check);
    if installed_version.is_none() {
        return Ok(InstallResult {
            success: false,
            message: format!(
                "{} 安装命令已执行，但暂未检测到可用版本。请重新打开终端后重试。",
                display_name
            ),
            installed_version: None,
            action: Some("install".to_string()),
            already_installed: Some(false),
            verified: Some(false),
            error_code: Some("install_verification_failed".to_string()),
        });
    }

    Ok(InstallResult {
        success: true,
        message: format!("{} 安装成功 ({})", display_name, executed_command),
        installed_version,
        action: Some("install".to_string()),
        already_installed: Some(false),
        verified: Some(true),
        error_code: None,
    })
}

#[cfg(any(test, target_os = "windows"))]
fn build_windows_command_spec(args: &[&str]) -> CommandSpec {
    let (program, rest) = args
        .split_first()
        .expect("build_windows_command_spec requires at least a program name");
    CommandSpec {
        program: program.to_string(),
        args: rest.iter().map(|s| s.to_string()).collect(),
    }
}

#[cfg(any(test, target_os = "windows"))]
fn build_windows_verify_spec(version_check: &str) -> CommandSpec {
    CommandSpec {
        program: "cmd".to_string(),
        args: vec!["/C".to_string(), version_check.to_string()],
    }
}

#[cfg(any(test, target_os = "windows"))]
fn install_windows_package_with_runner(
    runner: &mut dyn CommandRunner,
    display_name: &str,
    primary_commands: &[CommandSpec],
    fallback_commands: &[CommandSpec],
    verify_spec: CommandSpec,
    failure_error_code: &str,
    manual_message: &str,
) -> InstallResult {
    let mut executed_command: Option<String> = None;

    for cmd in primary_commands {
        match runner.run(cmd) {
            Ok(result) if result.success => {
                executed_command = Some(format!("{} {}", cmd.program, cmd.args.join(" ")));
                break;
            }
            _ => continue,
        }
    }

    if executed_command.is_none() {
        for cmd in fallback_commands {
            match runner.run(cmd) {
                Ok(result) if result.success => {
                    executed_command = Some(format!("{} {}", cmd.program, cmd.args.join(" ")));
                    break;
                }
                _ => continue,
            }
        }
    }

    let Some(executed) = executed_command else {
        return InstallResult {
            success: false,
            message: manual_message.to_string(),
            installed_version: None,
            action: None,
            already_installed: Some(false),
            verified: Some(false),
            error_code: Some(failure_error_code.to_string()),
        };
    };

    let installed_version = match runner.run(&verify_spec) {
        Ok(result) if result.success => {
            let raw = if result.stdout.is_empty() {
                result.stderr
            } else {
                result.stdout
            };
            if raw.is_empty() {
                None
            } else {
                Some(extract_version(&raw))
            }
        }
        _ => None,
    };

    if installed_version.is_none() {
        return InstallResult {
            success: false,
            message: format!(
                "{} 安装命令已执行，但暂未检测到可用版本。请重新打开终端后重试。",
                display_name
            ),
            installed_version: None,
            action: Some("install".to_string()),
            already_installed: Some(false),
            verified: Some(false),
            error_code: Some("install_verification_failed".to_string()),
        };
    }

    InstallResult {
        success: true,
        message: format!("{} 安装成功 ({})", display_name, executed),
        installed_version,
        action: Some("install".to_string()),
        already_installed: Some(false),
        verified: Some(true),
        error_code: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct CommandRunExpectation {
        spec: CommandSpec,
        result: CommandResult,
    }

    impl CommandRunExpectation {
        fn success(spec: CommandSpec, stdout: &str, stderr: &str) -> Self {
            CommandRunExpectation {
                spec,
                result: CommandResult {
                    success: true,
                    stdout: stdout.to_string(),
                    stderr: stderr.to_string(),
                },
            }
        }

        fn failure(spec: CommandSpec, stdout: &str, stderr: &str) -> Self {
            CommandRunExpectation {
                spec,
                result: CommandResult {
                    success: false,
                    stdout: stdout.to_string(),
                    stderr: stderr.to_string(),
                },
            }
        }
    }

    struct FakeCommandRunner {
        expectations: Vec<CommandRunExpectation>,
        pub calls: Vec<CommandSpec>,
        next_index: usize,
    }

    impl FakeCommandRunner {
        fn new(expectations: Vec<CommandRunExpectation>) -> Self {
            FakeCommandRunner {
                expectations,
                calls: Vec::new(),
                next_index: 0,
            }
        }
    }

    impl CommandRunner for FakeCommandRunner {
        fn run(&mut self, spec: &CommandSpec) -> Result<CommandResult, String> {
            self.calls.push(spec.clone());
            if self.next_index >= self.expectations.len() {
                return Err(format!(
                    "unexpected command call: {} {}",
                    spec.program,
                    spec.args.join(" ")
                ));
            }
            let expected = &self.expectations[self.next_index];
            self.next_index += 1;
            assert_eq!(
                spec,
                &expected.spec,
                "unexpected command spec at index {}",
                self.next_index - 1
            );
            Ok(expected.result.clone())
        }
    }

    #[test]
    fn test_parse_and_check_node_version() {
        assert!(parse_and_check_node_version("v18.20.0").unwrap());
        assert!(parse_and_check_node_version("18.20.0").unwrap());
        assert!(parse_and_check_node_version("v20.0.0").unwrap());
        assert!(!parse_and_check_node_version("v16.0.0").unwrap());
        assert!(!parse_and_check_node_version("v14.21.3").unwrap());
    }

    #[test]
    fn test_extract_version() {
        assert_eq!(extract_version("claude 1.0.20"), "1.0.20");
        assert_eq!(extract_version("v2.3.4-beta.1"), "2.3.4-beta.1");
        assert_eq!(extract_version("18.20.0"), "18.20.0");
    }

    #[test]
    fn build_windows_command_spec_preserves_program_and_args() {
        let spec = build_windows_command_spec(&["winget", "install", "OpenJS.NodeJS.LTS"]);

        assert_eq!(spec.program, "winget");
        assert_eq!(
            spec.args,
            vec!["install".to_string(), "OpenJS.NodeJS.LTS".to_string()]
        );
    }

    #[test]
    fn build_windows_verify_spec_wraps_command_in_cmd_c() {
        let spec = build_windows_verify_spec("claude --version");

        assert_eq!(spec.program, "cmd");
        assert_eq!(
            spec.args,
            vec!["/C".to_string(), "claude --version".to_string()]
        );
    }

    #[test]
    fn install_windows_package_returns_verification_failed_code_when_verify_fails() {
        let mut runner = FakeCommandRunner::new(vec![
            CommandRunExpectation::success(
                build_windows_command_spec(&["winget", "install", "OpenJS.NodeJS.LTS"]),
                "installed",
                "",
            ),
            CommandRunExpectation::failure(
                build_windows_verify_spec("node --version"),
                "",
                "node not found",
            ),
        ]);

        let result = install_windows_package_with_runner(
            &mut runner,
            "Node.js",
            &[build_windows_command_spec(&[
                "winget",
                "install",
                "OpenJS.NodeJS.LTS",
            ])],
            &[],
            build_windows_verify_spec("node --version"),
            "manual_node_install_required",
            "manual fallback",
        );

        assert!(!result.success);
        assert_eq!(
            result.error_code,
            Some("install_verification_failed".to_string())
        );
        assert_eq!(result.verified, Some(false));
    }

    #[test]
    fn install_windows_package_falls_back_to_second_backend_after_first_failure() {
        let mut runner = FakeCommandRunner::new(vec![
            CommandRunExpectation::failure(
                build_windows_command_spec(&["winget", "install", "OpenJS.NodeJS.LTS"]),
                "",
                "winget failed",
            ),
            CommandRunExpectation::success(
                build_windows_command_spec(&["choco", "install", "nodejs-lts", "-y"]),
                "installed",
                "",
            ),
            CommandRunExpectation::success(
                build_windows_verify_spec("node --version"),
                "v20.1.0",
                "",
            ),
        ]);

        let result = install_windows_package_with_runner(
            &mut runner,
            "Node.js",
            &[build_windows_command_spec(&[
                "winget",
                "install",
                "OpenJS.NodeJS.LTS",
            ])],
            &[build_windows_command_spec(&[
                "choco",
                "install",
                "nodejs-lts",
                "-y",
            ])],
            build_windows_verify_spec("node --version"),
            "manual_node_install_required",
            "manual fallback",
        );

        assert!(result.success);
        assert_eq!(result.installed_version, Some("20.1.0".to_string()));
        assert_eq!(runner.calls.len(), 3);
        assert_eq!(runner.calls[0].program, "winget");
        assert_eq!(runner.calls[1].program, "choco");
        assert_eq!(runner.calls[2].program, "cmd");
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn install_windows_package_returns_manual_message_when_no_command_succeeds() {
        let result = install_windows_package(
            "Claude Code",
            &[],
            &[],
            "claude --version",
            "manual_claude_install_required",
            "manual fallback",
        )
        .expect("install should return result");

        assert!(!result.success);
        assert_eq!(result.message, "manual fallback");
        assert_eq!(
            result.error_code,
            Some("manual_claude_install_required".to_string())
        );
        assert_eq!(result.verified, Some(false));
    }
}
