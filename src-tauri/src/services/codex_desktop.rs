//! Explicit desktop restart. No provider switch restarts clients implicitly.

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct DesktopRestartOutcome {
    pub action: String,
    pub desktop_pid: u32,
}

#[cfg(target_os = "windows")]
pub fn restart() -> Result<DesktopRestartOutcome, String> {
    use std::{
        io::Write,
        os::windows::process::CommandExt,
        process::{Command, Stdio},
        time::{Duration, Instant},
    };
    // Serialize requests so two button clicks cannot close the newly opened app.
    static RESTART_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = RESTART_LOCK
        .try_lock()
        .map_err(|_| "Codex desktop is already restarting".to_string())?;
    let mut script = tempfile::Builder::new()
        .suffix(".ps1")
        .tempfile()
        .map_err(|e| e.to_string())?;
    // Windows PowerShell 5.1 needs a BOM to decode UTF-8 scripts consistently.
    script
        .write_all(b"\xef\xbb\xbf")
        .map_err(|e| e.to_string())?;
    script
        .write_all(include_bytes!("codex_desktop_windows.ps1"))
        .map_err(|e| e.to_string())?;
    script.flush().map_err(|e| e.to_string())?;
    let mut child = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
        ])
        .arg(script.path())
        .args(["-Mode", "Restart"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .creation_flags(0x08000000)
        .spawn()
        .map_err(|e| format!("Cannot start Codex desktop restart: {e}"))?;
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        if child.try_wait().map_err(|e| e.to_string())?.is_some() {
            break;
        }
        if Instant::now() >= deadline {
            let _ = child.kill(); // Only the helper started above, never a Codex client.
            let _ = child.wait();
            return Err("Codex desktop restart timed out. Check whether the desktop app has exited before retrying.".into());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let output = child.wait_with_output().map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    serde_json::from_slice(&output.stdout)
        .map_err(|e| format!("Invalid Codex desktop restart receipt: {e}"))
}

#[cfg(not(target_os = "windows"))]
pub fn restart() -> Result<DesktopRestartOutcome, String> {
    Err("Desktop restart currently supports the Windows OpenAI.Codex package only".into())
}

#[cfg(test)]
mod tests {
    use super::*;
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
