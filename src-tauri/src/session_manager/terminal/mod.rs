use std::process::Command;

pub fn launch_terminal(
    target: &str,
    command: &str,
    cwd: Option<&str>,
    custom_config: Option<&str>,
) -> Result<(), String> {
    if command.trim().is_empty() {
        return Err("Resume command is empty".to_string());
    }

    if !cfg!(target_os = "macos") {
        return Err("Terminal resume is only supported on macOS".to_string());
    }

    match target {
        "terminal" => launch_macos_terminal(command, cwd),
        "iTerm" | "iterm" => launch_iterm(command, cwd),
        "ghostty" => launch_ghostty(command, cwd),
        "kitty" => launch_kitty(command, cwd),
        #[cfg(target_os = "macos")]
        "otty" => launch_otty(command, cwd),
        "wezterm" => launch_wezterm(command, cwd),
        "kaku" => launch_kaku(command, cwd),
        "alacritty" => launch_alacritty(command, cwd),
        #[cfg(unix)]
        "warp" => launch_warp(command, cwd),
        "custom" => launch_custom(command, cwd, custom_config),
        _ => Err(format!("Unsupported terminal target: {target}")),
    }
}

fn launch_macos_terminal(command: &str, cwd: Option<&str>) -> Result<(), String> {
    let full_command = build_shell_command(command, cwd);
    let escaped = escape_osascript(&full_command);
    let script = format!(
        r#"tell application "Terminal"
    activate
    do script "{escaped}"
end tell"#
    );

    let status = Command::new("osascript")
        .arg("-e")
        .arg(script)
        .status()
        .map_err(|e| format!("Failed to launch Terminal: {e}"))?;

    if status.success() {
        Ok(())
    } else {
        Err("Terminal command execution failed".to_string())
    }
}

#[cfg(target_os = "macos")]
fn build_otty_tab_args(command: &str, cwd: Option<&str>) -> Vec<String> {
    let mut args = vec![
        "tab".to_string(),
        "new".to_string(),
        "--window".to_string(),
        "0".to_string(),
    ];
    if let Some(cwd) = cwd.filter(|value| !value.trim().is_empty()) {
        args.push("--cwd".to_string());
        args.push(cwd.to_string());
    }
    args.push("--command".to_string());
    args.push(command.to_string());
    args
}

#[cfg(target_os = "macos")]
fn build_otty_window_args(command: &str, cwd: Option<&str>) -> Vec<String> {
    let mut args = vec![
        "open".to_string(),
        "--command".to_string(),
        command.to_string(),
    ];
    if let Some(cwd) = cwd.filter(|value| !value.trim().is_empty()) {
        args.push(cwd.to_string());
    }
    args
}

#[cfg(target_os = "macos")]
fn launch_otty(command: &str, cwd: Option<&str>) -> Result<(), String> {
    let otty_cli = find_otty_cli().ok_or_else(|| {
        "Otty CLI not found. Install Otty to /Applications or ~/Applications.".to_string()
    })?;

    let tab_result = Command::new(&otty_cli)
        .args(build_otty_tab_args(command, cwd))
        .output()
        .map_err(|e| format!("Failed to launch Otty CLI: {e}"))?;
    if tab_result.status.success() {
        return Ok(());
    }

    let window_result = Command::new(&otty_cli)
        .args(build_otty_window_args(command, cwd))
        .output()
        .map_err(|e| format!("Failed to launch Otty CLI: {e}"))?;
    if window_result.status.success() {
        return Ok(());
    }

    Err(format!(
        "Failed to launch Otty: {}",
        String::from_utf8_lossy(&window_result.stderr).trim()
    ))
}

#[cfg(target_os = "macos")]
fn find_otty_cli() -> Option<std::path::PathBuf> {
    otty_cli_candidates()
        .into_iter()
        .find(|path| path.is_file() && is_executable_file(path))
}

#[cfg(target_os = "macos")]
fn otty_cli_candidates() -> Vec<std::path::PathBuf> {
    let mut candidates = vec![std::path::PathBuf::from(
        "/Applications/Otty.app/Contents/MacOS/otty-cli",
    )];

    if let Some(home) = std::env::var_os("HOME") {
        candidates.push(
            std::path::PathBuf::from(home).join("Applications/Otty.app/Contents/MacOS/otty-cli"),
        );
    }

    candidates.push(std::path::PathBuf::from("/usr/local/bin/otty"));
    candidates.push(std::path::PathBuf::from("/opt/homebrew/bin/otty"));

    if let Some(path) = std::env::var_os("PATH") {
        for directory in std::env::split_paths(&path) {
            candidates.push(directory.join("otty"));
            candidates.push(directory.join("otty-cli"));
        }
    }

    candidates
}

#[cfg(target_os = "macos")]
fn is_executable_file(path: &std::path::Path) -> bool {
    use std::os::unix::fs::PermissionsExt;

    path.metadata()
        .map(|metadata| metadata.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

fn launch_iterm(command: &str, cwd: Option<&str>) -> Result<(), String> {
    let full_command = build_shell_command(command, cwd);
    let escaped = escape_osascript(&full_command);
    // iTerm2 AppleScript to create a new window and execute command
    let script = format!(
        r#"tell application "iTerm"
    activate
    create window with default profile
    tell current session of current window
        write text "{escaped}"
    end tell
end tell"#
    );

    let status = Command::new("osascript")
        .arg("-e")
        .arg(script)
        .status()
        .map_err(|e| format!("Failed to launch iTerm: {e}"))?;

    if status.success() {
        Ok(())
    } else {
        Err("iTerm command execution failed".to_string())
    }
}

fn launch_ghostty(command: &str, cwd: Option<&str>) -> Result<(), String> {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string());

    let mut args = vec![
        "-na".to_string(),
        "Ghostty".to_string(),
        "--args".to_string(),
        "--quit-after-last-window-closed=true".to_string(),
    ];

    if let Some(dir) = cwd {
        if !dir.trim().is_empty() {
            args.push(format!("--working-directory={dir}"));
        }
    }

    args.push("-e".to_string());
    args.push(shell);
    args.push("-l".to_string());
    args.push("-c".to_string());
    args.push(command.to_string());

    let status = Command::new("open")
        .args(&args)
        .status()
        .map_err(|e| format!("Failed to launch Ghostty: {e}"))?;

    if status.success() {
        Ok(())
    } else {
        Err("Failed to launch Ghostty. Make sure it is installed.".to_string())
    }
}

fn launch_kitty(command: &str, cwd: Option<&str>) -> Result<(), String> {
    let full_command = build_shell_command(command, cwd);

    // 获取用户默认 shell
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string());

    let status = Command::new("open")
        .arg("-na")
        .arg("kitty")
        .arg("--args")
        .arg("-e")
        .arg(&shell)
        .arg("-l")
        .arg("-c")
        .arg(&full_command)
        .status()
        .map_err(|e| format!("Failed to launch Kitty: {e}"))?;

    if status.success() {
        Ok(())
    } else {
        Err("Failed to launch Kitty. Make sure it is installed.".to_string())
    }
}

fn launch_wezterm(command: &str, cwd: Option<&str>) -> Result<(), String> {
    // wezterm start --cwd ... -- command
    // To invoke via `open`, we use `open -na "WezTerm" --args start ...`
    let args = build_wezterm_compatible_args("WezTerm", command, cwd);

    let status = Command::new("open")
        .args(args.iter().map(String::as_str))
        .status()
        .map_err(|e| format!("Failed to launch WezTerm: {e}"))?;

    if status.success() {
        Ok(())
    } else {
        Err("Failed to launch WezTerm.".to_string())
    }
}

fn launch_kaku(command: &str, cwd: Option<&str>) -> Result<(), String> {
    // Kaku is a WezTerm-derived terminal and keeps a compatible `start` entrypoint.
    let args = build_wezterm_compatible_args("Kaku", command, cwd);

    let status = Command::new("open")
        .args(args.iter().map(String::as_str))
        .status()
        .map_err(|e| format!("Failed to launch Kaku: {e}"))?;

    if status.success() {
        Ok(())
    } else {
        Err("Failed to launch Kaku.".to_string())
    }
}

fn build_wezterm_compatible_args(app_name: &str, command: &str, cwd: Option<&str>) -> Vec<String> {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string());
    build_wezterm_compatible_args_with_shell(app_name, command, cwd, &shell)
}

fn build_wezterm_compatible_args_with_shell(
    app_name: &str,
    command: &str,
    cwd: Option<&str>,
    shell: &str,
) -> Vec<String> {
    let full_command = build_shell_command(command, None);
    let mut args = vec![
        "-na".to_string(),
        app_name.to_string(),
        "--args".to_string(),
        "start".to_string(),
    ];

    if let Some(dir) = cwd {
        args.push("--cwd".to_string());
        args.push(dir.to_string());
    }

    // Invoke shell to run the command string (to handle pipes, etc)
    args.push("--".to_string());
    args.push(shell.to_string());
    args.push("-c".to_string());
    args.push(full_command);
    args
}

#[cfg(unix)]
fn launch_warp(command: &str, cwd: Option<&str>) -> Result<(), String> {
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;

    let cwd = cwd.ok_or("Failed to resume session without cwd")?;

    let mut script_file = tempfile::Builder::new()
        .disable_cleanup(true)
        .permissions(std::fs::Permissions::from_mode(0o755))
        .tempfile_in(cwd)
        .map_err(|e| format!("Failed to create temporary script file for launching Warp: {e}"))?;

    writeln!(
        &mut script_file,
        r#"#!/usr/bin/env sh

        rm -- "$0"

        exec {command}
        "#,
    )
    .map_err(|e| format!("Failed to write to temporary script file for Warp: {e}"))?;

    let mut warp_url = url::Url::parse("warp://action/new_tab").unwrap();
    warp_url
        .query_pairs_mut()
        .append_pair("path", &script_file.path().to_string_lossy());
    let warp_url = warp_url.to_string();

    let status = Command::new("open")
        .args(["-a", "Warp", &warp_url])
        .status()
        .map_err(|e| format!("Failed to launch Warp: {e}"))?;

    if status.success() {
        Ok(())
    } else {
        Err("Failed to launch Warp.".to_string())
    }
}

fn launch_alacritty(command: &str, cwd: Option<&str>) -> Result<(), String> {
    // Alacritty: open -na Alacritty --args --working-directory ... -e shell -c command
    let full_command = build_shell_command(command, None);
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string());

    let mut args = vec!["-na", "Alacritty", "--args"];

    if let Some(dir) = cwd {
        args.push("--working-directory");
        args.push(dir);
    }

    args.push("-e");
    args.push(&shell);
    args.push("-c");
    args.push(&full_command);

    let status = Command::new("open")
        .args(&args)
        .status()
        .map_err(|e| format!("Failed to launch Alacritty: {e}"))?;

    if status.success() {
        Ok(())
    } else {
        Err("Failed to launch Alacritty.".to_string())
    }
}

fn launch_custom(
    command: &str,
    cwd: Option<&str>,
    custom_config: Option<&str>,
) -> Result<(), String> {
    let template = custom_config.ok_or("No custom terminal config provided")?;

    if template.trim().is_empty() {
        return Err("Custom terminal command template is empty".to_string());
    }

    let cmd_str = command;
    // `{cwd}` 是磁盘上扫来的路径，先做转义；`{command}` 保持原样——模板作者写下
    // 这个占位符的本意就是让它当命令展开。
    //
    // ⚠️ 这里的转义**不是完备防护**，只在占位符处于未加引号的 shell 词位置时成立。
    // 模板若写成 `echo "{cwd}"`，插入的单引号会落进双引号里变成普通字符，`cwd`
    // 里的 `$(...)` 照样求值。安全性取决于模板怎么写，而模板不由这里控制。
    //
    // 目前本分支无 UI 入口（终端选项列表没有 `custom`，前端也从不传
    // `customConfig`），所以不可达。**接线前必须换掉这个方案**——正确做法是让
    // 模板声明参数位、由此处按 argv 传递，而不是让用户拼 shell 字符串。
    let dir_str = shell_escape(cwd.unwrap_or("."));

    let final_cmd_line = template
        .replace("{command}", cmd_str)
        .replace("{cwd}", &dir_str);

    // Execute via sh -c
    let status = Command::new("sh")
        .arg("-c")
        .arg(&final_cmd_line)
        .status()
        .map_err(|e| format!("Failed to execute custom terminal launcher: {e}"))?;

    if status.success() {
        Ok(())
    } else {
        Err("Custom terminal execution returned error code".to_string())
    }
}

fn build_shell_command(command: &str, cwd: Option<&str>) -> String {
    match cwd {
        Some(dir) if !dir.trim().is_empty() => {
            format!("cd {} && {}", shell_escape(dir), command)
        }
        _ => command.to_string(),
    }
}

/// POSIX 单引号转义。
///
/// **必须是单引号**：双引号内 `$(...)`、反引号、`$VAR` 照常展开，而这里包的是
/// `projectDir`——会话历史里记录的真实项目路径，macOS 允许目录名含 `$` `(` `)`，
/// 所以一个名为 `$(...)` 的目录就足以让命令替换在用户终端里执行。
///
/// 单引号内不做任何展开，唯一的特例是 `'` 自身无法被表示：用「闭合-转义-重开」
/// 的 `'\''` 序列绕过。
pub(crate) fn shell_escape(value: &str) -> String {
    format!("'{}'", value.replace('\'', r"'\''"))
}

fn escape_osascript(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_shell_command_keeps_command_without_cwd_prefix_when_not_provided() {
        assert_eq!(
            build_shell_command("claude --resume abc-123", None),
            "claude --resume abc-123"
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn otty_launches_a_new_tab_without_injecting_into_the_current_session() {
        assert_eq!(
            build_otty_tab_args("claude --resume abc-123", Some("/tmp/project-$(id -un)")),
            vec![
                "tab",
                "new",
                "--window",
                "0",
                "--cwd",
                "/tmp/project-$(id -un)",
                "--command",
                "claude --resume abc-123",
            ]
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn otty_falls_back_to_a_new_window_with_the_same_command_and_cwd() {
        assert_eq!(
            build_otty_window_args("claude --resume abc-123", Some("/tmp/project dir")),
            vec![
                "open",
                "--command",
                "claude --resume abc-123",
                "/tmp/project dir",
            ]
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn otty_cli_candidates_include_bundle_and_installed_cli_locations() {
        let candidates = otty_cli_candidates();

        assert!(candidates.contains(&std::path::PathBuf::from(
            "/Applications/Otty.app/Contents/MacOS/otty-cli"
        )));
        assert!(candidates.contains(&std::path::PathBuf::from("/usr/local/bin/otty")));
        assert!(candidates.contains(&std::path::PathBuf::from("/opt/homebrew/bin/otty")));
    }

    #[test]
    fn wezterm_compatible_terminals_use_start_and_cwd_arguments() {
        let args = build_wezterm_compatible_args_with_shell(
            "Kaku",
            "claude --resume abc-123",
            Some("/tmp/project dir"),
            "/bin/zsh",
        );

        assert_eq!(
            args,
            vec![
                "-na".to_string(),
                "Kaku".to_string(),
                "--args".to_string(),
                "start".to_string(),
                "--cwd".to_string(),
                "/tmp/project dir".to_string(),
                "--".to_string(),
                "/bin/zsh".to_string(),
                "-c".to_string(),
                "claude --resume abc-123".to_string(),
            ]
        );
    }

    #[test]
    fn ghostty_uses_working_directory_arg_for_cwd() {
        // cwd should be passed as --working-directory, not embedded in the shell command string
        // This avoids shell expansion of special characters in directory paths
        let cwd = "/tmp/project dir";
        let command = "claude --resume abc-123";

        // Verify build_shell_command does NOT include cwd when used in ghostty context
        // (ghostty passes cwd via --working-directory flag instead)
        assert_eq!(
            build_shell_command(command, None),
            "claude --resume abc-123"
        );

        // Verify shell_escape works correctly for paths with spaces
        assert_eq!(shell_escape(cwd), "'/tmp/project dir'");
    }

    #[test]
    fn shell_escape_neutralizes_command_substitution_in_directory_names() {
        // 这些字符在 macOS 目录名里全部合法，而 `cwd` 就是会话历史里的
        // `projectDir`——一个名为 `$(...)` 的目录必须原样落到 `cd` 后面，
        // 不能被 shell 求值。旧的双引号实现对这三种全部失守。
        assert_eq!(shell_escape("/tmp/$(id -un)"), "'/tmp/$(id -un)'");
        assert_eq!(shell_escape("/tmp/`id -un`"), "'/tmp/`id -un`'");
        assert_eq!(shell_escape("/tmp/$HOME"), "'/tmp/$HOME'");
    }

    #[test]
    fn shell_escape_handles_embedded_single_quote() {
        // 单引号是单引号包裹法唯一表示不了的字符，靠「闭合-转义-重开」绕过。
        assert_eq!(shell_escape("/tmp/it's"), r"'/tmp/it'\''s'");
    }

    #[test]
    fn shell_escape_survives_the_osascript_layer() {
        // Terminal / iTerm 的链路是两层：shell_escape 的结果先被塞进 AppleScript
        // 字符串字面量，由 escape_osascript 再转义一次，AppleScript 求值后才交给
        // shell。反斜杠会在中间那层被加倍，必须确认最终落到 shell 的字节没变形。
        let escaped = shell_escape("/tmp/it's");
        assert_eq!(escaped, r"'/tmp/it'\''s'");

        let for_applescript = escape_osascript(&escaped);
        assert_eq!(for_applescript, r"'/tmp/it'\\''s'");

        // AppleScript 把 `\\` 求值回单个 `\`，于是 shell 拿到的正是 escaped 本身。
        assert_eq!(for_applescript.replace(r"\\", r"\"), escaped);
    }

    #[test]
    fn build_shell_command_quotes_the_cwd_it_prefixes() {
        // Terminal / iTerm / kitty 三条路径都经这里；ghostty / wezterm / alacritty
        // 走 `cwd = None` 并把目录当独立 argv 传，不受影响。
        assert_eq!(
            build_shell_command("claude --resume x", Some("/tmp/$(id -un)")),
            "cd '/tmp/$(id -un)' && claude --resume x"
        );
    }
}
