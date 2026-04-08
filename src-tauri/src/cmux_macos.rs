//! Launch commands in [cmux](https://www.cmux.dev/) from GUI apps.
//! On macOS: Tauri/Finder-launched processes often have a minimal `PATH` (no Homebrew), and cmux’s
//! socket may reject non-cmux-spawned clients — see error hints from [`run_in_cmux`].

#[cfg(target_os = "macos")]
use std::path::{Path, PathBuf};
#[cfg(target_os = "macos")]
use std::process::{Command, Stdio};
#[cfg(target_os = "macos")]
use std::thread;
#[cfg(target_os = "macos")]
use std::time::Duration;

/// GUI bundle executable (`Contents/MacOS/cmux`). Starting this with `CMUX_SOCKET_MODE=allowAll`
/// is how cmux accepts control from external apps (cc-switch); `open -a` alone does not set that.
#[cfg(target_os = "macos")]
pub fn find_cmux_bundle_main_executable() -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(h) = dirs::home_dir() {
        candidates.push(h.join("Applications/cmux.app/Contents/MacOS/cmux"));
    }
    candidates.push(PathBuf::from("/Applications/cmux.app/Contents/MacOS/cmux"));
    for p in candidates {
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

/// Spawn cmux with socket policy that allows non-cmux-spawned processes to use the CLI (required for Tauri).
#[cfg(target_os = "macos")]
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

/// Quit running cmux, then start it with `CMUX_SOCKET_MODE=allowAll` so cc-switch can call `cmux new-workspace` / `send`.
#[cfg(target_os = "macos")]
pub fn restart_cmux_with_allow_all() -> Result<(), String> {
    let _ = Command::new("osascript")
        .args([
            "-e",
            r#"tell application "cmux" to if running then quit"#,
        ])
        .status();

    thread::sleep(Duration::from_millis(1600));

    let exe = find_cmux_bundle_main_executable().ok_or_else(|| {
        "找不到 cmux.app（例如 /Applications/cmux.app）。请先安装 cmux。".to_string()
    })?;

    Command::new(&exe)
        .env("CMUX_SOCKET_MODE", "allowAll")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("以兼容模式启动 cmux 失败: {e}"))?;

    thread::sleep(Duration::from_millis(1200));
    Ok(())
}

/// Bring cmux to the foreground (or start it). macOS only.
#[cfg(target_os = "macos")]
pub fn activate_cmux_app() -> Result<(), String> {
    // Cold start: main binary + allowAll so socket accepts our CLI. If cmux is already running with
    // stricter policy, user must use restart_cmux_with_allow_all() once.
    let _ = spawn_cmux_main_with_allow_all();
    thread::sleep(Duration::from_millis(400));

    let status = Command::new("open")
        .args(["-a", "cmux"])
        .status()
        .map_err(|e| format!("failed to run `open -a cmux`: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err("`open -a cmux` failed — is cmux installed in /Applications?".into())
    }
}

/// Resolve the `cmux` CLI: `CMUX_CLI` env, well-known paths, then login-shell `command -v`.
#[cfg(target_os = "macos")]
pub fn resolve_cmux_cli() -> Result<PathBuf, String> {
    if let Ok(custom) = std::env::var("CMUX_CLI") {
        let trimmed = custom.trim();
        if !trimmed.is_empty() {
            let p = PathBuf::from(trimmed);
            if p.is_file() {
                return Ok(p);
            }
            return Err(format!("CMUX_CLI is set but not a file: {trimmed}"));
        }
    }

    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(h) = dirs::home_dir() {
        candidates.push(h.join("Applications/cmux.app/Contents/Resources/bin/cmux"));
        candidates.push(h.join("Applications/cmux.app/Contents/MacOS/cmux"));
        candidates.push(h.join(".local/bin/cmux"));
    }
    candidates.extend([
        PathBuf::from("/Applications/cmux.app/Contents/Resources/bin/cmux"),
        PathBuf::from("/Applications/cmux.app/Contents/MacOS/cmux"),
        PathBuf::from("/opt/homebrew/bin/cmux"),
        PathBuf::from("/usr/local/bin/cmux"),
    ]);

    for p in candidates {
        if p.is_file() {
            return Ok(p);
        }
    }

    if let Some(p) = resolve_via_zsh_login_shell() {
        return Ok(p);
    }

    Err(
        "cmux CLI not found. Install cmux, or set CMUX_CLI to the binary (e.g. /opt/homebrew/bin/cmux)."
            .into(),
    )
}

#[cfg(target_os = "macos")]
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

#[cfg(target_os = "macos")]
fn format_cmux_failure(output: &std::process::Output, step: &str) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut msg = format!("cmux {step} failed (status {:?})", output.status.code());
    if !stderr.trim().is_empty() {
        msg.push_str(": ");
        msg.push_str(stderr.trim());
    } else if !stdout.trim().is_empty() {
        msg.push_str(": ");
        msg.push_str(stdout.trim());
    }
    msg.push_str(
        " | Fix: in CC Switch → Settings → Preferred Terminal, use “Restart cmux for external control”, or in cmux Settings enable socket access for all local processes, or quit cmux and run: CMUX_SOCKET_MODE=allowAll open -a cmux (see https://www.cmux.dev/docs/api ).",
    );
    msg
}

#[cfg(target_os = "macos")]
fn run_cmux_checked(exe: &Path, args: &[&str], step: &str) -> Result<(), String> {
    let output = Command::new(exe)
        .env("CMUX_SOCKET_MODE", "allowAll")
        .args(args)
        .output()
        .map_err(|e| format!("failed to spawn cmux ({step}): {e}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format_cmux_failure(&output, step))
    }
}

/// Open cmux, create a new workspace, and `send` the given text (include `\n` if you need Enter).
#[cfg(target_os = "macos")]
pub fn run_in_cmux(send_text: &str) -> Result<(), String> {
    activate_cmux_app()?;
    thread::sleep(Duration::from_millis(900));

    let exe = resolve_cmux_cli()?;
    run_cmux_checked(&exe, &["new-workspace"], "new-workspace")?;
    thread::sleep(Duration::from_millis(350));
    run_cmux_checked(&exe, &["send", send_text], "send")?;
    Ok(())
}

#[cfg(not(target_os = "macos"))]
pub fn run_in_cmux(_send_text: &str) -> Result<(), String> {
    Err("cmux is only supported on macOS".into())
}
