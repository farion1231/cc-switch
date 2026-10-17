//! Explicit desktop restart. No provider switch restarts clients implicitly.

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct DesktopRestartOutcome {
    pub action: String,
    pub desktop_pid: u32,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct DesktopStatus {
    pub supported: bool,
    pub installed: bool,
    pub running: bool,
}

pub fn validate_switch_target(
    expected: &str,
    selected: Option<&str>,
    proxy_mode: bool,
) -> Result<(), String> {
    if expected.trim().is_empty() || proxy_mode || selected != Some(expected) {
        return Err("The selected Codex provider or mode changed before restart. The desktop was not restarted; retry from the current provider.".into());
    }
    Ok(())
}

#[cfg(target_os = "windows")]
pub fn status() -> Result<DesktopStatus, String> {
    let output = run_powershell_script(include_bytes!("codex_desktop_windows.ps1"), "Status")?;
    serde_json::from_slice(&output.stdout).map_err(|e| format!("Invalid Codex desktop status: {e}"))
}

#[cfg(not(target_os = "windows"))]
pub fn status() -> Result<DesktopStatus, String> {
    Ok(DesktopStatus {
        supported: false,
        installed: false,
        running: false,
    })
}

#[cfg(target_os = "windows")]
pub fn restart() -> Result<DesktopRestartOutcome, String> {
    // Serialize requests so two button clicks cannot close the newly opened app.
    static RESTART_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = RESTART_LOCK
        .try_lock()
        .map_err(|_| "Codex desktop is already restarting".to_string())?;
    let output = run_powershell_script(include_bytes!("codex_desktop_windows.ps1"), "Restart")?;
    serde_json::from_slice(&output.stdout)
        .map_err(|e| format!("Invalid Codex desktop restart receipt: {e}"))
}

#[cfg(target_os = "windows")]
fn run_powershell_script(source: &[u8], mode: &str) -> Result<std::process::Output, String> {
    use std::{
        io::{Read, Write},
        os::windows::process::CommandExt,
        process::{Command, Stdio},
        time::{Duration, Instant},
    };
    let mut script = tempfile::Builder::new()
        .suffix(".ps1")
        .tempfile()
        .map_err(|e| e.to_string())?;
    // Windows PowerShell 5.1 needs a BOM to decode UTF-8 scripts consistently.
    script
        .write_all(b"\xef\xbb\xbf")
        .map_err(|e| e.to_string())?;
    script.write_all(source).map_err(|e| e.to_string())?;
    script.flush().map_err(|e| e.to_string())?;
    // PowerShell cannot read a Windows delete-on-close file while its handle is
    // open. Keep the path's cleanup guard, but release the handle before launch.
    let script = script.into_temp_path();
    let mut child = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
        ])
        .arg(&script)
        .args(["-Mode", mode])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .creation_flags(0x08000000)
        .spawn()
        .map_err(|e| format!("Cannot start Codex desktop restart: {e}"))?;
    // Drain both pipes while the helper runs. Waiting first can deadlock when a
    // diagnostic fills either pipe and prevents the child from exiting.
    let mut stdout = child.stdout.take().ok_or("Missing desktop helper stdout")?;
    let mut stderr = child.stderr.take().ok_or("Missing desktop helper stderr")?;
    let stdout_reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout.read_to_end(&mut bytes).map(|_| bytes)
    });
    let stderr_reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stderr.read_to_end(&mut bytes).map(|_| bytes)
    });
    let deadline = Instant::now() + Duration::from_secs(60);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) => (),
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                break Err(error.to_string());
            }
        }
        if Instant::now() >= deadline {
            let _ = child.kill(); // Only the helper started above, never a Codex client.
            let _ = child.wait();
            break Err("Codex desktop restart timed out. Check whether the desktop app has exited before retrying.".into());
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    let stdout = stdout_reader
        .join()
        .map_err(|_| "Desktop helper stdout reader failed")?
        .map_err(|e| e.to_string())?;
    let stderr = stderr_reader
        .join()
        .map_err(|_| "Desktop helper stderr reader failed")?
        .map_err(|e| e.to_string())?;
    let output = std::process::Output {
        status: status?,
        stdout,
        stderr,
    };
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(output)
}

#[cfg(not(target_os = "windows"))]
pub fn restart() -> Result<DesktopRestartOutcome, String> {
    Err("Desktop restart currently supports the Windows OpenAI.Codex package only".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn switch_restart_rejects_a_stale_provider_or_mode() {
        assert!(validate_switch_target("A", Some("A"), false).is_ok());
        assert!(validate_switch_target("A", Some("B"), false).is_err());
        assert!(validate_switch_target("A", None, false).is_err());
        assert!(validate_switch_target("A", Some("A"), true).is_err());
        assert!(validate_switch_target("", Some(""), false).is_err());
    }
    #[cfg(not(target_os = "windows"))]
    #[test]
    fn unsupported_platform_does_not_offer_desktop_controls() {
        let state = status().unwrap();
        assert!(!state.supported);
        assert!(!state.installed);
        assert!(!state.running);
    }
    #[test]
    fn desktop_restart_receipt_requires_a_pid_and_action() {
        let receipt: DesktopRestartOutcome =
            serde_json::from_str(r#"{"action":"restarted","desktop_pid":42}"#).unwrap();
        assert_eq!(receipt.desktop_pid, 42);
        assert_eq!(receipt.action, "restarted");
        assert!(
            serde_json::from_str::<DesktopRestartOutcome>(r#"{"action":"restarted"}"#).is_err()
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn materialized_script_runs_in_powershell_and_cleans_up() {
        // Exercise the same temp-file handoff as production, with no real app
        // close or activation. Directly running a checked-in script misses the
        // Windows delete-on-close sharing regression.
        let source = r#"param([string]$Mode)
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)
if ($Mode -ne 'Plan') { throw 'Unexpected execution mode' }
@{ action = 'restarted'; desktop_pid = 42; script_path = $PSCommandPath; label = '桌面' } | ConvertTo-Json -Compress
"#;
        let output = run_powershell_script(source.as_bytes(), "Plan").unwrap();
        let receipt: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(receipt["action"], "restarted");
        assert_eq!(receipt["desktop_pid"], 42);
        assert_eq!(receipt["label"], "桌面");
        let path = receipt["script_path"].as_str().unwrap();
        assert!(!std::path::Path::new(path).exists());
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn materialized_script_drains_both_pipes_before_waiting() {
        let source = r#"param([string]$Mode)
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)
[Console]::Out.Write(('o' * 262144))
[Console]::Error.Write(('e' * 262144))
"#;
        let output = run_powershell_script(source.as_bytes(), "Plan").unwrap();
        assert_eq!(output.stdout.len(), 262144);
        assert_eq!(output.stderr.len(), 262144);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn materialized_desktop_failure_reports_only_the_message() {
        // Run the production error handler without querying or changing a real
        // installation. A package lookup failure should not expose a PS stack.
        let (_, body) = include_str!("codex_desktop_windows.ps1")
            .split_once("$ErrorActionPreference")
            .unwrap();
        let source = format!(
            "param([string]$Mode)\nfunction Get-AppxPackage {{ param($Name) }}\n$ErrorActionPreference{body}"
        );
        let error = run_powershell_script(source.as_bytes(), "Plan").unwrap_err();
        assert_eq!(
            error,
            "Codex desktop (OpenAI.Codex) is not installed for this Windows user."
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn desktop_restart_lifecycle_fixtures() {
        use std::os::windows::process::CommandExt;
        let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../scripts/tests/codex-desktop-restart.Tests.ps1");
        let output = std::process::Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
            ])
            .arg(script)
            .creation_flags(0x08000000)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
