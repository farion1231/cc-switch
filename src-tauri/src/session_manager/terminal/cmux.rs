//! 从 CC Switch 等外部 GUI 应用通过 [cmux](https://cmux.com/) CLI 启动终端会话。
//!
//! 与 Ghostty/Warp 等不同，cmux 没有 URL scheme，必须调用 `Resources/bin/cmux` 并通过 Unix socket
//! 控制 workspace。Tauri/Finder 进程的 `PATH` 通常很短，且 socket 默认可能拒绝外部客户端，
//! 因此需要显式解析 CLI 路径、设置 `CMUX_SOCKET_MODE`，并在 cmux 未运行时冷启动 GUI 主程序。

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::Duration;

/// cmux 侧边栏 workspace 标题：`{provider_name} · {app_short}`
pub fn format_cmux_workspace_title(provider_name: &str, app: &str) -> String {
    let app_short = match app {
        "claude" => "Claude".to_string(),
        "codex" => "Codex".to_string(),
        "gemini" => "Gemini".to_string(),
        other => {
            let mut chars = other.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
            }
        }
    };
    let mut name: String = provider_name
        .replace(['\n', '\r'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    const MAX_CHARS: usize = 64;
    if name.chars().count() > MAX_CHARS {
        name = name.chars().take(MAX_CHARS - 1).collect::<String>() + "…";
    }
    format!("{name} · {app_short}")
}

pub struct CmuxWorkspaceLaunch {
    pub title: String,
    pub cwd: Option<PathBuf>,
    /// 要在 workspace 里执行的 shell 命令行（如 `bash '/tmp/script.sh'`），send 时会自动补换行。
    pub command: String,
}

/// 判断路径是否指向 cmux GUI 主二进制（`Contents/MacOS/cmux`），该路径不能当 CLI 用。
pub fn is_macos_gui_cmux_binary(path: &Path) -> bool {
    path.to_string_lossy().contains("Contents/MacOS/cmux")
}

/// 定位 cmux.app 的 GUI 主程序，用于冷启动时附带 `CMUX_SOCKET_MODE=allowAll`。
pub fn find_cmux_bundle_main_executable() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(home) = dirs::home_dir() {
        candidates.push(home.join("Applications/cmux.app/Contents/MacOS/cmux"));
    }
    candidates.push(PathBuf::from("/Applications/cmux.app/Contents/MacOS/cmux"));
    candidates.into_iter().find(|p| p.is_file())
}

fn spawn_cmux_main_with_allow_all() -> bool {
    let Some(exe) = find_cmux_bundle_main_executable() else {
        return false;
    };
    Command::new(&exe)
        .env("CMUX_SOCKET_MODE", "allowAll")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .is_ok()
}

fn cmux_ping(exe: &Path) -> bool {
    run_cmux_checked(exe, &["ping"], "ping", "automation").is_ok()
        || run_cmux_checked(exe, &["ping"], "ping", "allowAll").is_ok()
}

fn wait_for_cmux_ping(exe: &Path, max_wait_ms: u64) -> bool {
    let step = Duration::from_millis(150);
    let mut waited = 0u64;
    while waited < max_wait_ms {
        if cmux_ping(exe) {
            return true;
        }
        thread::sleep(step);
        waited += step.as_millis() as u64;
    }
    false
}

/// 确保 cmux 已运行且 socket 可用。
fn ensure_cmux_app_ready(exe: &Path) -> Result<(), String> {
    if cmux_ping(exe) {
        // 已在运行：激活窗口即可，不要再次 spawn（避免多实例或重复恢复 workspace）
        let status = Command::new("open")
            .args(["-a", "cmux"])
            .status()
            .map_err(|e| format!("Failed to run open -a cmux: {e}"))?;
        if status.success() {
            thread::sleep(Duration::from_millis(200));
            return Ok(());
        }
        return Err(
            "open -a cmux failed. Make sure cmux is installed in /Applications.".into(),
        );
    }

    // cmux 未运行：必须用 GUI 主二进制 + allowAll 启动，单纯 `open -a` 不会写入 socket 策略
    if !spawn_cmux_main_with_allow_all() {
        return Err("Failed to launch cmux.app. Make sure cmux is installed.".into());
    }
    if !wait_for_cmux_ping(exe, 5000) {
        return Err("Timed out waiting for cmux socket.".into());
    }
    thread::sleep(Duration::from_millis(400));
    Ok(())
}

/// 解析 cmux CLI 路径（`Contents/Resources/bin/cmux`），禁止使用 GUI 主二进制。
pub fn resolve_cmux_cli() -> Result<PathBuf, String> {
    if let Ok(custom) = std::env::var("CMUX_CLI") {
        let trimmed = custom.trim();
        if !trimmed.is_empty() {
            let p = PathBuf::from(trimmed);
            if p.is_file() {
                if is_macos_gui_cmux_binary(&p) {
                    return Err(format!(
                        "CMUX_CLI points to the GUI binary; use Resources/bin/cmux instead: {trimmed}"
                    ));
                }
                return Ok(p);
            }
            return Err(format!("CMUX_CLI is set but is not a valid file: {trimmed}"));
        }
    }

    // Tauri 进程 PATH 短，按常见安装位置依次探测；Ghostty/Warp 用 `open -a` 不需要这一步
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(home) = dirs::home_dir() {
        candidates.push(home.join("Applications/cmux.app/Contents/Resources/bin/cmux"));
        candidates.push(home.join(".local/bin/cmux"));
    }
    candidates.extend([
        PathBuf::from("/Applications/cmux.app/Contents/Resources/bin/cmux"),
        PathBuf::from("/opt/homebrew/bin/cmux"),
        PathBuf::from("/usr/local/bin/cmux"),
    ]);

    for p in candidates {
        if p.is_file() && !is_macos_gui_cmux_binary(&p) {
            return Ok(p);
        }
    }

    // 兜底：模拟用户在 login shell 里 `command -v cmux`（覆盖自定义安装路径）
    if let Some(p) = resolve_via_zsh_login_shell() {
        if !is_macos_gui_cmux_binary(&p) {
            return Ok(p);
        }
    }

    Err("cmux CLI not found. Install cmux or set CMUX_CLI to \
         /Applications/cmux.app/Contents/Resources/bin/cmux"
        .into())
}

fn resolve_via_zsh_login_shell() -> Option<PathBuf> {
    let output = Command::new("/bin/zsh")
        .args(["-l", "-c", "command -v cmux"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if s.is_empty() {
        return None;
    }
    let p = PathBuf::from(s);
    if p.is_file() {
        Some(p)
    } else {
        None
    }
}

fn format_cmux_failure_hint() -> &'static str {
    " | Fix: allow external socket access in cmux settings, or quit and reopen cmux; see https://cmux.com/docs/api"
}

fn format_cmux_failure(output: &Output, step: &str) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut msg = format!("cmux {step} failed (exit {:?})", output.status.code());
    if !stderr.trim().is_empty() {
        msg.push_str(": ");
        msg.push_str(stderr.trim());
    } else if !stdout.trim().is_empty() {
        msg.push_str(": ");
        msg.push_str(stdout.trim());
    }
    msg.push_str(format_cmux_failure_hint());
    msg
}

fn run_cmux_checked(
    exe: &Path,
    args: &[&str],
    step: &str,
    socket_mode: &str,
) -> Result<(), String> {
    let output = Command::new(exe)
        .env("CMUX_SOCKET_MODE", socket_mode)
        .args(args)
        .output()
        .map_err(|e| format!("Failed to run cmux ({step}): {e}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format_cmux_failure(&output, step))
    }
}

fn run_cmux_with_modes(exe: &Path, args: &[&str], step: &str) -> Result<(), String> {
    // 优先 automation；外部 GUI 进程常被拒时再试 allowAll
    if run_cmux_checked(exe, args, step, "automation").is_ok() {
        return Ok(());
    }
    run_cmux_checked(exe, args, step, "allowAll")
}

fn parse_workspace_id(stdout: &str) -> Option<String> {
    // cmux 不同版本/输出格式：`{"workspace_id":"..."}` 或 `OK workspace:4`
    let trimmed = stdout.trim();
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(trimmed) {
        if let Some(id) = v.get("workspace_id").and_then(|x| x.as_str()) {
            return Some(id.to_string());
        }
    }
    for line in trimmed.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("OK workspace:") {
            let id = rest.trim();
            if !id.is_empty() {
                return Some(format!("workspace:{id}"));
            }
        }
    }
    for token in trimmed.split_whitespace() {
        if token.starts_with("workspace:") {
            return Some(token.to_string());
        }
    }
    None
}

fn stderr_suggests_unknown_flag(stderr: &str, flag: &str) -> bool {
    let lower = stderr.to_ascii_lowercase();
    lower.contains("unknown") && lower.contains(flag)
}

#[derive(Clone, Copy, Default)]
struct NewWorkspaceOpts {
    json: bool,
    with_name: bool,
    with_command: bool,
    with_cwd: bool,
    with_focus: bool,
}

fn build_new_workspace_argv(launch: &CmuxWorkspaceLaunch, opts: NewWorkspaceOpts) -> Vec<String> {
    let mut args = vec!["new-workspace".into()];
    if opts.json {
        args.push("--json".into());
    }
    if opts.with_cwd {
        if let Some(cwd) = &launch.cwd {
            args.push("--cwd".into());
            args.push(cwd.to_string_lossy().into_owned());
        }
    }
    if opts.with_name {
        args.push("--name".into());
        args.push(launch.title.clone());
    }
    if opts.with_command {
        args.push("--command".into());
        args.push(launch.command.clone());
    }
    if opts.with_focus {
        args.push("--focus".into());
        args.push("true".into());
    }
    args
}

/// 先 automation，失败再 allowAll；失败时返回 **最后一次** 尝试的 Output。
fn run_cmux_output_with_modes(exe: &Path, args: &[&str]) -> Result<Output, String> {
    let run = |mode: &str| {
        Command::new(exe)
            .env("CMUX_SOCKET_MODE", mode)
            .args(args)
            .output()
    };
    let output = run("automation").map_err(|e| format!("Failed to run cmux: {e}"))?;
    if output.status.success() {
        return Ok(output);
    }
    let allow_output = run("allowAll").map_err(|e| format!("Failed to run cmux: {e}"))?;
    Ok(allow_output)
}

/// 清除 workspace 上待恢复的 agent session，避免 autoResume 抢先于 CC Switch 注入的命令。
fn clear_surface_resume(exe: &Path, workspace: &str) {
    let _ = run_cmux_with_modes(
        exe,
        &["surface", "resume", "clear", "--workspace", workspace],
        "surface resume clear",
    );
}

fn send_command_to_workspace(
    exe: &Path,
    workspace: &str,
    launch: &CmuxWorkspaceLaunch,
) -> Result<(), String> {
    clear_surface_resume(exe, workspace);
    // send 需要 trailing newline 才会在 shell 里执行，而不是只粘贴到提示符
    let send_body = format!("{}\n", launch.command);
    run_cmux_with_modes(
        exe,
        &["send", "--workspace", workspace, &send_body],
        "send",
    )
}

fn new_workspace_with_title_and_command(
    exe: &Path,
    launch: &CmuxWorkspaceLaunch,
) -> Result<(), String> {
    new_workspace_create_then_send(exe, launch, true)
}

/// 先创建 workspace（不用 `--command`，避免与 autoResume 竞态），再 clear resume + send。
fn new_workspace_create_then_send(
    exe: &Path,
    launch: &CmuxWorkspaceLaunch,
    with_focus: bool,
) -> Result<(), String> {
    let args = build_new_workspace_argv(
        launch,
        NewWorkspaceOpts {
            json: true,
            with_name: true,
            with_cwd: true,
            with_focus,
            ..Default::default()
        },
    );
    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let output = run_cmux_output_with_modes(exe, &arg_refs)?;

    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let ws = parse_workspace_id(&stdout)
            .ok_or_else(|| {
                format!("cmux new-workspace --json did not return a workspace id: {stdout}")
            })?;
        return send_command_to_workspace(exe, &ws, launch);
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    if with_focus && stderr_suggests_unknown_flag(&stderr, "focus") {
        return new_workspace_create_then_send(exe, launch, false);
    }
    if stderr_suggests_unknown_flag(&stderr, "name") {
        return new_workspace_rename_then_command(exe, launch, with_focus);
    }
    if stderr_suggests_unknown_flag(&stderr, "cwd") {
        return new_workspace_send_fallback(exe, launch, with_focus);
    }

    Err(format_cmux_failure(&output, "new-workspace"))
}

fn new_workspace_rename_then_command(
    exe: &Path,
    launch: &CmuxWorkspaceLaunch,
    with_focus: bool,
) -> Result<(), String> {
    // 旧版 cmux 不支持 `new-workspace --name`：先创建再 rename
    let args = build_new_workspace_argv(
        launch,
        NewWorkspaceOpts {
            json: true,
            with_cwd: true,
            with_focus,
            ..Default::default()
        },
    );
    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let output = run_cmux_output_with_modes(exe, &arg_refs)?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if with_focus && stderr_suggests_unknown_flag(&stderr, "focus") {
            return new_workspace_rename_then_command(exe, launch, false);
        }
        return Err(format_cmux_failure(&output, "new-workspace"));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let ws = parse_workspace_id(&stdout)
        .ok_or_else(|| format!("cmux new-workspace --json did not return a workspace id: {stdout}"))?;

    run_cmux_with_modes(
        exe,
        &["rename-workspace", "--workspace", &ws, &launch.title],
        "rename-workspace",
    )?;

    send_command_to_workspace(exe, &ws, launch)
}

fn new_workspace_send_fallback(
    exe: &Path,
    launch: &CmuxWorkspaceLaunch,
    with_focus: bool,
) -> Result<(), String> {
    // 更旧版本不支持 `--cwd`：在 send 正文里手动 `cd`
    let args = build_new_workspace_argv(
        launch,
        NewWorkspaceOpts {
            json: true,
            with_focus,
            ..Default::default()
        },
    );
    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let output = run_cmux_output_with_modes(exe, &arg_refs)?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if with_focus && stderr_suggests_unknown_flag(&stderr, "focus") {
            return new_workspace_send_fallback(exe, launch, false);
        }
        return Err(format_cmux_failure(&output, "new-workspace"));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let ws = parse_workspace_id(&stdout)
        .ok_or_else(|| format!("cmux new-workspace --json did not return a workspace id: {stdout}"))?;

    let mut send_text = String::new();
    if let Some(cwd) = &launch.cwd {
        send_text.push_str(&format!(
            "cd {} && ",
            shell_single_quote(&cwd.to_string_lossy())
        ));
    }
    send_text.push_str(&launch.command);
    if !send_text.ends_with('\n') {
        send_text.push('\n');
    }

    clear_surface_resume(exe, &ws);
    run_cmux_with_modes(exe, &["send", "--workspace", &ws, &send_text], "send")?;

    run_cmux_with_modes(
        exe,
        &["rename-workspace", "--workspace", &ws, &launch.title],
        "rename-workspace",
    )
}

fn shell_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

/// 创建带标题的新 workspace 并在其中执行命令（Provider 打开终端 / Session 恢复共用入口）。
pub fn run_cmux_workspace(launch: &CmuxWorkspaceLaunch) -> Result<(), String> {
    let exe = resolve_cmux_cli()?;
    ensure_cmux_app_ready(&exe)?;
    new_workspace_with_title_and_command(&exe, launch)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_claude() {
        assert_eq!(
            format_cmux_workspace_title("PackyCode", "claude"),
            "PackyCode · Claude"
        );
    }

    #[test]
    fn title_strips_newlines() {
        assert_eq!(
            format_cmux_workspace_title("a\nb", "claude"),
            "a b · Claude"
        );
    }

    #[test]
    fn title_truncates_long_name() {
        let long = "x".repeat(80);
        let title = format_cmux_workspace_title(&long, "claude");
        assert!(title.chars().count() <= 64 + " · Claude".chars().count());
        assert!(title.ends_with("· Claude"));
    }

    #[test]
    fn rejects_macos_gui_path() {
        let p = PathBuf::from("/Applications/cmux.app/Contents/MacOS/cmux");
        assert!(is_macos_gui_cmux_binary(&p));
    }

    #[test]
    fn accepts_resources_bin_path() {
        let p = PathBuf::from("/Applications/cmux.app/Contents/Resources/bin/cmux");
        assert!(!is_macos_gui_cmux_binary(&p));
    }

    #[test]
    fn parse_workspace_id_from_ok_line() {
        assert_eq!(
            parse_workspace_id("OK workspace:4\n"),
            Some("workspace:4".to_string())
        );
    }

    #[test]
    fn parse_workspace_id_from_json() {
        assert_eq!(
            parse_workspace_id(r#"{"workspace_id":"workspace:2"}"#),
            Some("workspace:2".to_string())
        );
    }

    #[test]
    fn stderr_suggests_unknown_flag_matches() {
        assert!(stderr_suggests_unknown_flag(
            "Error: unknown flag: --focus",
            "focus"
        ));
        assert!(!stderr_suggests_unknown_flag(
            "socket connection refused",
            "focus"
        ));
    }
}
