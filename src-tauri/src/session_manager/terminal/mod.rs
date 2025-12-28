use std::process::Command;

pub fn launch_terminal(target: &str, command: &str, cwd: Option<&str>) -> Result<(), String> {
    if command.trim().is_empty() {
        return Err("Resume command is empty".to_string());
    }

    if !cfg!(target_os = "macos") {
        return Err("Terminal resume is only supported on macOS".to_string());
    }

    match target {
        "terminal" => launch_macos_terminal(command, cwd),
        "kitty" => launch_kitty(command, cwd),
        _ => Err(format!("Unsupported terminal target: {target}")),
    }
}

fn launch_macos_terminal(command: &str, cwd: Option<&str>) -> Result<(), String> {
    let full_command = build_shell_command(command, cwd);
    let escaped = escape_osascript(&full_command);
    let script = format!("tell application \"Terminal\" to do script \"{escaped}\"");

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

fn launch_kitty(command: &str, cwd: Option<&str>) -> Result<(), String> {
    let mut cmd = Command::new("kitty");
    cmd.arg("@").arg("launch").arg("--type=window");
    if let Some(dir) = cwd {
        if !dir.trim().is_empty() {
            cmd.arg("--cwd").arg(dir);
        }
    }

    cmd.arg("bash").arg("-lc").arg(command);

    let status = cmd
        .status()
        .map_err(|e| format!("Failed to launch kitty: {e}"))?;

    if status.success() {
        Ok(())
    } else {
        Err("Kitty launch failed. Ensure kitty is running with remote control enabled.".to_string())
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

fn shell_escape(value: &str) -> String {
    let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

fn escape_osascript(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}
