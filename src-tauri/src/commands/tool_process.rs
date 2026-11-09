use super::*;

#[derive(Clone, Copy)]
pub(crate) struct CommandDeadline {
    expires_at: std::time::Instant,
    limit: std::time::Duration,
}

impl CommandDeadline {
    pub(crate) fn from_timeout(timeout: Option<std::time::Duration>) -> Option<Self> {
        timeout.map(|limit| Self {
            expires_at: std::time::Instant::now() + limit,
            limit,
        })
    }

    fn remaining(self) -> Result<std::time::Duration, String> {
        self.expires_at
            .checked_duration_since(std::time::Instant::now())
            .filter(|remaining| !remaining.is_zero())
            .ok_or_else(|| self.timeout_error())
    }

    fn timeout_error(self) -> String {
        format!("Command timed out after {}s", self.limit.as_secs())
    }
}

#[cfg(target_os = "windows")]
pub(crate) fn terminate_child_tree(child: &mut std::process::Child) -> bool {
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};

    let status = Command::new("taskkill")
        .args(["/PID", &child.id().to_string(), "/T", "/F"])
        .creation_flags(CREATE_NO_WINDOW)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    matches!(status, Ok(status) if status.success()) || child.kill().is_ok()
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn terminate_child_tree(child: &mut std::process::Child) -> bool {
    let process_group = -(child.id() as libc::pid_t);
    // SAFETY: runtime commands are placed in a dedicated process group before spawn.
    (unsafe { libc::kill(process_group, libc::SIGKILL) == 0 }) || child.kill().is_ok()
}

#[cfg(not(target_os = "windows"))]
pub(super) fn isolate_child_process_group(cmd: &mut std::process::Command) {
    use std::os::unix::process::CommandExt;

    // setsid 而非 process_group(0)：新会话自带新进程组（组长=自身，
    // terminate_child_tree 的 kill(-pid) 整组击杀语义不变），并额外**脱离控制终端**。
    // 只隔离进程组时，探测用的交互式 shell（zsh -lic）若还持有控制终端（如 dev 模式
    // 从终端启动），其作业控制会因处于背景进程组被 SIGTTIN/SIGTTOU 停住，`wait()`
    // 永远等不到退出；脱离终端后 shell 拿不到 /dev/tty，作业控制自动关闭。
    // SAFETY: setsid 是 async-signal-safe；fork 出的子进程继承父进程组、必不是组长，
    // 调用不会因 EPERM 失败。
    unsafe {
        cmd.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
}

pub(super) fn wait_child_output(
    mut child: std::process::Child,
    deadline: Option<CommandDeadline>,
) -> Result<std::process::Output, String> {
    use std::io::Read;

    let stdout_pipe = child.stdout.take();
    let stderr_pipe = child.stderr.take();

    let stdout_handle = stdout_pipe.map(|mut pipe| {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = pipe.read_to_end(&mut buf);
            buf
        })
    });
    let stderr_handle = stderr_pipe.map(|mut pipe| {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = pipe.read_to_end(&mut buf);
            buf
        })
    });

    let status = match deadline {
        None => child
            .wait()
            .map_err(|e| format!("Failed to wait for command: {e}"))?,
        Some(deadline) => {
            loop {
                match child.try_wait() {
                    Ok(Some(status)) => break status,
                    Ok(None) => {
                        let remaining = match deadline.remaining() {
                            Ok(remaining) => remaining,
                            Err(error) => {
                                if terminate_child_tree(&mut child) {
                                    let _ = child.wait();
                                }
                                // Do not join pipe readers on timeout. If tree termination fails,
                                // a descendant may still own the write handle and never produce EOF.
                                drop(stdout_handle);
                                drop(stderr_handle);
                                return Err(error);
                            }
                        };
                        std::thread::sleep(std::cmp::min(
                            std::time::Duration::from_millis(50),
                            remaining,
                        ));
                    }
                    Err(e) => {
                        if terminate_child_tree(&mut child) {
                            let _ = child.wait();
                        }
                        return Err(format!("Failed to wait for command: {e}"));
                    }
                }
            }
        }
    };

    if let Some(deadline) = deadline {
        while stdout_handle
            .as_ref()
            .is_some_and(|handle| !handle.is_finished())
            || stderr_handle
                .as_ref()
                .is_some_and(|handle| !handle.is_finished())
        {
            let remaining = match deadline.remaining() {
                Ok(remaining) => remaining,
                Err(error) => {
                    let _ = terminate_child_tree(&mut child);
                    drop(stdout_handle);
                    drop(stderr_handle);
                    return Err(error);
                }
            };
            std::thread::sleep(std::cmp::min(
                std::time::Duration::from_millis(50),
                remaining,
            ));
        }
    }

    let stdout = stdout_handle
        .map(|handle| handle.join().unwrap_or_default())
        .unwrap_or_default();
    let stderr = stderr_handle
        .map(|handle| handle.join().unwrap_or_default())
        .unwrap_or_default();

    Ok(std::process::Output {
        status,
        stdout,
        stderr,
    })
}

fn apply_extra_env(cmd: &mut std::process::Command, extra_env: &[(&str, String)]) {
    for (key, value) in extra_env {
        cmd.env(key, value);
    }
}

/// 参数会经过 `cmd /C call`（双引号包裹）或 WSL 里的 `sh -c '…'`（单引号包裹后再分词），
/// 只放行在两边都不会被解释的字符。`@ / : +` 给插件 id（`name@marketplace`）和
/// 市场来源（`owner/repo`、`https://…`）用；空格、引号、`%`、`&`、`\`、`~` 一律不放。
pub(crate) fn is_safe_tool_command_arg(arg: &str) -> bool {
    !arg.is_empty()
        && arg.chars().all(|c| {
            c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '@' | '/' | ':' | '+')
        })
}

pub(crate) fn run_detected_tool_command_with_timeout(
    tool: &str,
    args: &[&str],
    timeout: Option<std::time::Duration>,
    extra_env: &[(&str, String)],
    working_dir: &Path,
) -> Result<std::process::Output, String> {
    if known_tool(tool).is_none() {
        return Err(format!("Unsupported tool: {tool}"));
    }
    if args.iter().any(|arg| !is_safe_tool_command_arg(arg)) {
        return Err("Invalid tool command arguments".to_string());
    }

    let deadline = CommandDeadline::from_timeout(timeout);

    #[cfg(target_os = "windows")]
    if let Some(distro) = wsl_distro_for_tool(tool) {
        return run_wsl_tool_command(tool, args, &distro, deadline, extra_env, working_dir);
    }

    // Runtime execution only needs the default entry point. Full installation
    // enumeration runs `--version` for every candidate and belongs to diagnostics.
    let tool_path = locate_default_tool(tool, deadline)?;
    let dir = tool_path
        .parent()
        .ok_or_else(|| format!("Invalid {tool} executable path"))?;
    #[cfg(target_os = "windows")]
    let current_path = effective_path_string();
    // GUI 启动时继承的是 launchd 的窄 PATH（/usr/bin:/bin:/usr/sbin:/sbin）。npm 装的 CLI
    // 是 `#!/usr/bin/env node` 脚本，node 常不在工具自己的目录里（如 codex 在
    // /opt/homebrew/bin、node 在 /usr/local/bin），只补工具目录会 exit 127（#8653）。
    // 与安装/升级同样补上登录 shell 的 PATH。
    #[cfg(not(target_os = "windows"))]
    let current_path = {
        let inherited = std::env::var_os("PATH")
            .map(|value| value.to_string_lossy().into_owned())
            .unwrap_or_default();
        match login_shell_path(deadline) {
            Some(login_path) => merge_path_segments(&login_path, &inherited),
            None => inherited,
        }
    };

    #[cfg(target_os = "windows")]
    {
        run_windows_tool_command_capture(
            &tool_path,
            args,
            &format!("{};{current_path}", dir.display()),
            deadline,
            extra_env,
            working_dir,
        )
    }

    #[cfg(not(target_os = "windows"))]
    {
        use std::process::{Command, Stdio};

        let mut cmd = Command::new(&tool_path);
        // stdin 置 null：继承来的终端（`pnpm dev`）会让要确认的 CLI 一直等到超时。
        cmd.args(args)
            .env("PATH", format!("{}:{current_path}", dir.display()))
            .current_dir(working_dir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        apply_extra_env(&mut cmd, extra_env);
        isolate_child_process_group(&mut cmd);
        let child = cmd
            .spawn()
            .map_err(|e| format!("Failed to run {tool}: {e}"))?;
        wait_child_output(child, deadline)
    }
}

/// 构造运行本机工具默认入口的命令（定位规则和 PATH 补全同 [`run_detected_tool_command_with_timeout`]），
/// 不设 stdio、不等待：给要自己管子进程的调用方用（比如长时间运行、要往 stdin 写东西的登录）。
/// `timeout` 只约束定位工具这一步。工具配置在 WSL 里时报错。
pub(crate) fn detected_tool_command(
    tool: &str,
    args: &[&str],
    timeout: std::time::Duration,
) -> Result<std::process::Command, String> {
    if known_tool(tool).is_none() {
        return Err(format!("Unsupported tool: {tool}"));
    }
    let deadline = CommandDeadline::from_timeout(Some(timeout));

    #[cfg(target_os = "windows")]
    if wsl_distro_for_tool(tool).is_some() {
        return Err(format!(
            "{tool} runs in WSL; this action needs it on Windows"
        ));
    }

    let tool_path = locate_default_tool(tool, deadline)?;
    let dir = tool_path
        .parent()
        .ok_or_else(|| format!("Invalid {tool} executable path"))?;

    #[cfg(target_os = "windows")]
    {
        let new_path = format!("{};{}", dir.display(), effective_path_string());
        Ok(build_windows_tool_command(&tool_path, args, &new_path))
    }

    #[cfg(not(target_os = "windows"))]
    {
        let inherited = std::env::var_os("PATH")
            .map(|value| value.to_string_lossy().into_owned())
            .unwrap_or_default();
        let current_path = match login_shell_path(deadline) {
            Some(login_path) => merge_path_segments(&login_path, &inherited),
            None => inherited,
        };
        let mut cmd = std::process::Command::new(&tool_path);
        cmd.args(args)
            .env("PATH", format!("{}:{current_path}", dir.display()));
        isolate_child_process_group(&mut cmd);
        Ok(cmd)
    }
}

#[cfg(target_os = "windows")]
fn run_windows_tool_command_capture(
    tool_path: &Path,
    args: &[&str],
    new_path: &str,
    deadline: Option<CommandDeadline>,
    extra_env: &[(&str, String)],
    working_dir: &Path,
) -> Result<std::process::Output, String> {
    use std::process::Stdio;

    let mut cmd = build_windows_tool_command(tool_path, args, new_path);
    apply_extra_env(&mut cmd, extra_env);
    cmd.current_dir(working_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let child = cmd
        .spawn()
        .map_err(|e| format!("Failed to run tool: {e}"))?;
    wait_child_output(child, deadline)
}

/// Convert `\\wsl$\Distro\home\user\...` / `\\wsl.localhost\...` to a Linux path.
#[cfg(target_os = "windows")]
fn wsl_unc_path_to_linux(path: &Path) -> Option<String> {
    use std::path::{Component, Prefix};

    let mut components = path.components();
    let Component::Prefix(prefix) = components.next()? else {
        return None;
    };
    match prefix.kind() {
        Prefix::UNC(server, _share) | Prefix::VerbatimUNC(server, _share) => {
            let server_name = server.to_string_lossy();
            if !(server_name.eq_ignore_ascii_case("wsl$")
                || server_name.eq_ignore_ascii_case("wsl.localhost"))
            {
                return None;
            }
        }
        _ => return None,
    }

    let mut linux = String::new();
    for component in components {
        match component {
            Component::RootDir => {}
            Component::Normal(part) => {
                linux.push('/');
                linux.push_str(&part.to_string_lossy());
            }
            Component::CurDir | Component::ParentDir | Component::Prefix(_) => return None,
        }
    }
    if linux.is_empty() {
        None
    } else {
        Some(linux)
    }
}

#[cfg(target_os = "windows")]
fn build_wsl_env_argv(extra_env: &[(&str, String)]) -> Result<Vec<String>, String> {
    let mut env_argv = Vec::new();
    for (key, value) in extra_env {
        if key.is_empty()
            || key.contains('=')
            || key.chars().any(|c| c.is_whitespace() || c.is_control())
        {
            return Err(format!("invalid env for {key}"));
        }

        // 配置目录类的变量是 Windows 侧的 `\\wsl$\…` 路径，进发行版前换成 Linux 路径；
        // 换不了（不是 WSL 路径）就不传，让工具用它自己的默认目录。
        let linux_value = if matches!(
            *key,
            "OPENCODE_CONFIG_DIR"
                | "CODEX_HOME"
                | "CLAUDE_CONFIG_DIR"
                | "GROK_HOME"
                | "OPENCLAW_STATE_DIR"
                | "OPENCLAW_CONFIG_PATH"
                | "PI_CODING_AGENT_DIR"
        ) {
            let Some(value) = wsl_unc_path_to_linux(Path::new(value)) else {
                continue;
            };
            value
        } else {
            value.clone()
        };
        if linux_value.chars().any(char::is_control) {
            return Err(format!("invalid env for {key}"));
        }
        env_argv.push(format!("{key}={linux_value}"));
    }
    Ok(env_argv)
}

#[cfg(target_os = "windows")]
fn build_wsl_tool_command(
    tool: &str,
    args: &[&str],
    deadline: Option<CommandDeadline>,
) -> Result<String, String> {
    let invocation = std::iter::once(tool)
        .chain(args.iter().copied())
        .collect::<Vec<_>>()
        .join(" ");
    let command = format!(
        "for flag in -lic -lc -c; do if \"${{SHELL:-sh}}\" \"$flag\" 'command -v {tool}' >/dev/null 2>&1; then exec \"${{SHELL:-sh}}\" \"$flag\" '{invocation}'; fi; done; exit 127"
    );

    let Some(deadline) = deadline else {
        return Ok(command);
    };
    let remaining = deadline.remaining()?;
    let timeout_arg = format!("{:.3}s", remaining.as_secs_f64());
    Ok(format!(
        "command -v timeout >/dev/null 2>&1 || {{ echo 'timeout is required for bounded CLI execution' >&2; exit 127; }}; exec timeout --signal=TERM --kill-after=1s {timeout_arg} sh -c {}",
        shell_single_quote(&command)
    ))
}

#[cfg(target_os = "windows")]
fn run_wsl_tool_command(
    tool: &str,
    args: &[&str],
    distro: &str,
    deadline: Option<CommandDeadline>,
    extra_env: &[(&str, String)],
    working_dir: &Path,
) -> Result<std::process::Output, String> {
    use std::process::{Command, Stdio};

    if !is_valid_wsl_distro_name(distro) {
        return Err(format!("[WSL:{distro}] invalid distro name"));
    }

    let command = build_wsl_tool_command(tool, args, deadline)?;
    let linux_working_dir = wsl_unc_path_to_linux(working_dir)
        .ok_or_else(|| format!("[WSL:{distro}] invalid working directory"))?;
    let env_argv = build_wsl_env_argv(extra_env).map_err(|e| format!("[WSL:{distro}] {e}"))?;

    let mut cmd = Command::new("wsl.exe");
    cmd.arg("-d")
        .arg(distro)
        .arg("--cd")
        .arg(linux_working_dir)
        .arg("--");
    if !env_argv.is_empty() {
        cmd.arg("env");
        for item in &env_argv {
            cmd.arg(item);
        }
    }
    cmd.args(["sh", "-c", &command])
        .creation_flags(CREATE_NO_WINDOW)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let child = cmd
        .spawn()
        .map_err(|e| format!("[WSL:{distro}] failed to run {tool}: {e}"))?;
    let output = wait_child_output(child, deadline).map_err(|e| {
        if e.starts_with("Command timed out") {
            format!("[WSL:{distro}] {e}")
        } else {
            e
        }
    })?;
    if output.status.code() == Some(124) {
        return Err(format!(
            "[WSL:{distro}] {}",
            deadline
                .map(CommandDeadline::timeout_error)
                .unwrap_or_else(|| "Command timed out".to_string())
        ));
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 超时击杀路径：挂死的子进程到点被整组击杀、wait 返回超时错误而非永等。
    /// 同时锚定 setsid 改造后的语义——child 是新会话/新进程组组长，
    /// terminate_child_tree 的 kill(-pid) 仍能命中（回归红线：改回 process_group
    /// 或去掉隔离都会让本测试的击杀路径失效）。
    #[cfg(not(target_os = "windows"))]
    #[test]
    fn isolated_hung_child_is_killed_on_deadline() {
        use std::process::{Command, Stdio};

        let mut cmd = Command::new("/bin/sh");
        cmd.args(["-c", "sleep 30"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        isolate_child_process_group(&mut cmd);
        let child = cmd.spawn().expect("spawn sleep");
        let started = std::time::Instant::now();
        let result = wait_child_output(
            child,
            CommandDeadline::from_timeout(Some(std::time::Duration::from_millis(200))),
        );
        assert!(result.is_err(), "expected timeout error, got {result:?}");
        assert!(
            started.elapsed() < std::time::Duration::from_secs(5),
            "kill should return promptly instead of waiting out the sleep"
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn wsl_env_allows_spaces_in_unc_config_path() {
        let extra_env = [
            (
                "OPENCODE_CONFIG_DIR",
                r"\\wsl$\Ubuntu\home\Jane Doe\.config\opencode".to_string(),
            ),
            ("OPENCODE_DISABLE_PROJECT_CONFIG", "true".to_string()),
        ];

        assert_eq!(
            build_wsl_env_argv(&extra_env).unwrap(),
            vec![
                "OPENCODE_CONFIG_DIR=/home/Jane Doe/.config/opencode".to_string(),
                "OPENCODE_DISABLE_PROJECT_CONFIG=true".to_string(),
            ]
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn wsl_env_skips_host_config_path() {
        let extra_env = [
            (
                "OPENCODE_CONFIG_DIR",
                r"C:\Users\Jane Doe\.config\opencode".to_string(),
            ),
            ("OPENCODE_DISABLE_PROJECT_CONFIG", "true".to_string()),
        ];

        assert_eq!(
            build_wsl_env_argv(&extra_env).unwrap(),
            vec!["OPENCODE_DISABLE_PROJECT_CONFIG=true".to_string()]
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_tool_capture_accepts_canonicalized_batch_paths() {
        let root = tempfile::tempdir().unwrap();
        let bin = root.path().join("模型 tools & helpers");
        std::fs::create_dir(&bin).unwrap();
        std::fs::write(
            bin.join("cc_switch_capture_helper.cmd"),
            "@echo helper-found\r\n@exit /b 23\r\n",
        )
        .unwrap();
        let path = format!("{};{}", bin.display(), effective_path_string());
        for extension in ["cmd", "bat"] {
            let entry = bin.join(format!("opencode.{extension}"));
            std::fs::write(
                &entry,
                "@echo off\r\necho arg=%~1\r\necho env=%CC_SWITCH_CAPTURE_TEST%\r\necho captured-stderr 1>&2\r\ncmd /D /C cc_switch_capture_helper.cmd\r\nexit /b %errorlevel%\r\n",
            )
            .unwrap();
            let canonical = std::fs::canonicalize(entry).unwrap();
            assert!(canonical.to_string_lossy().starts_with(r"\\?\"));
            let output = run_windows_tool_command_capture(
                &canonical,
                &["models"],
                &path,
                CommandDeadline::from_timeout(Some(std::time::Duration::from_secs(10))),
                &[("CC_SWITCH_CAPTURE_TEST", "preserved".into())],
                root.path(),
            )
            .unwrap();
            let stdout = decode_command_output(&output.stdout);
            assert_eq!(output.status.code(), Some(23), "{output:?}");
            assert!(stdout.contains("arg=models"), "{stdout}");
            assert!(stdout.contains("env=preserved"), "{stdout}");
            assert!(stdout.contains("helper-found"), "{stdout}");
            assert!(decode_command_output(&output.stderr).contains("captured-stderr"));
        }
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn wsl_env_translates_codex_and_claude_config_dirs() {
        let extra_env = [
            ("CODEX_HOME", r"\\wsl$\Ubuntu\home\jane\.codex".to_string()),
            (
                "CLAUDE_CONFIG_DIR",
                r"\\wsl.localhost\Ubuntu\home\jane\claude-alt".to_string(),
            ),
        ];
        assert_eq!(
            build_wsl_env_argv(&extra_env).unwrap(),
            vec![
                "CODEX_HOME=/home/jane/.codex".to_string(),
                "CLAUDE_CONFIG_DIR=/home/jane/claude-alt".to_string(),
            ]
        );

        let host_only = [("CODEX_HOME", r"C:\Users\jane\.codex".to_string())];
        assert!(build_wsl_env_argv(&host_only).unwrap().is_empty());

        let plugin_dirs = [
            ("GROK_HOME", r"\\wsl$\Ubuntu\home\jane\.grok".to_string()),
            (
                "OPENCLAW_STATE_DIR",
                r"\\wsl$\Ubuntu\home\jane\.openclaw".to_string(),
            ),
            (
                "OPENCLAW_CONFIG_PATH",
                r"\\wsl$\Ubuntu\home\jane\.openclaw\openclaw.json".to_string(),
            ),
        ];
        assert_eq!(
            build_wsl_env_argv(&plugin_dirs).unwrap(),
            vec![
                "GROK_HOME=/home/jane/.grok".to_string(),
                "OPENCLAW_STATE_DIR=/home/jane/.openclaw".to_string(),
                "OPENCLAW_CONFIG_PATH=/home/jane/.openclaw/openclaw.json".to_string(),
            ]
        );
    }

    #[test]
    fn tool_command_args_allow_plugin_ids_and_reject_shell_syntax() {
        for ok in [
            "plugin",
            "--json",
            "frontend-design@claude-plugins-official",
            "anthropics/claude-plugins-official",
            "https://github.com/owner/repo.git",
            "git@github.com:owner/repo.git",
            "c++",
        ] {
            assert!(is_safe_tool_command_arg(ok), "{ok}");
        }
        for bad in [
            "", "a b", "a&b", "a|b", "%PATH%", "$(id)", "a'b", "a\"b", r"C:\path", "~/x", "a;b",
            "a#b",
        ] {
            assert!(!is_safe_tool_command_arg(bad), "{bad}");
        }
    }
}
