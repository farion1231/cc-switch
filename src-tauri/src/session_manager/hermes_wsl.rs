use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::Value;

const READER: &str = include_str!("hermes_wsl_reader.py");
const MAX_RESPONSE: u64 = 32 * 1024 * 1024;

/// Only the two WSL UNC hosts are routed to a Linux reader. Keep the public
/// source reference as UNC so normal provider/path validation still applies.
fn target(path: &Path) -> Option<(String, String)> {
    let text = path.to_str()?;
    let rest = text
        .strip_prefix(r"\\?\UNC\")
        .or_else(|| text.strip_prefix(r"\\"))?;
    let mut parts = rest.split('\\');
    let host = parts.next()?;
    if !host.eq_ignore_ascii_case("wsl.localhost") && !host.eq_ignore_ascii_case("wsl$") {
        return None;
    }
    let distro = parts.next()?;
    if distro.is_empty() || distro.starts_with('-') {
        return None;
    }
    let rest: Vec<_> = parts.collect();
    if rest.is_empty() || rest.iter().any(|p| p.is_empty() || *p == "." || *p == "..") {
        return None;
    }
    Some((distro.to_string(), format!("/{}", rest.join("/"))))
}

pub(super) fn is_wsl(path: &Path) -> bool {
    target(path).is_some()
}

pub(super) fn read(path: &Path, request: Value) -> Result<Value, String> {
    let (distro, native) = target(path).ok_or("Invalid Hermes WSL database path")?;
    let mut command = Command::new("wsl.exe");
    command.args([
        "--distribution",
        &distro,
        "--exec",
        "timeout",
        "8s",
        "python3",
        "-I",
        "-c",
        READER,
        &native,
        &request.to_string(),
    ]);
    // Do not show a console window on each session refresh.
    use std::os::windows::process::CommandExt;
    command.creation_flags(0x08000000);
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to start Hermes WSL reader: {e}"))?;
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let out = thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout
            .take(MAX_RESPONSE + 1)
            .read_to_end(&mut bytes)
            .map(|_| bytes)
    });
    let err = thread::spawn(move || {
        let mut bytes = Vec::new();
        stderr.take(4096).read_to_end(&mut bytes).map(|_| bytes)
    });
    let deadline = Instant::now() + Duration::from_secs(10);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(20)),
            result => {
                let _ = child.kill();
                let _ = child.wait();
                break Err(match result {
                    Err(e) => format!("Failed to wait for Hermes WSL reader: {e}"),
                    _ => "Hermes WSL reader timed out".to_string(),
                });
            }
        }
    };
    let stdout = out
        .join()
        .map_err(|_| "Hermes WSL reader thread failed")?
        .map_err(|e| e.to_string())?;
    let stderr = err
        .join()
        .map_err(|_| "Hermes WSL reader thread failed")?
        .map_err(|e| e.to_string())?;
    if !status?.success() {
        return Err(format!(
            "Hermes WSL read failed (python3 is required): {}",
            String::from_utf8_lossy(&stderr).trim()
        ));
    }
    if stdout.len() as u64 > MAX_RESPONSE {
        return Err("Hermes WSL response exceeds 32 MiB".to_string());
    }
    serde_json::from_slice(&stdout).map_err(|e| format!("Invalid Hermes WSL response: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routes_only_wsl_hosts_and_preserves_linux_path() {
        for path in [
            r"\\wsl.localhost\Ubuntu-22.04\home\user\.hermes\state.db",
            r"\\wsl$\Ubuntu-22.04\home\user\.hermes\state.db",
            r"\\?\UNC\wsl.localhost\Ubuntu-22.04\home\user\.hermes\state.db",
        ] {
            assert_eq!(
                target(Path::new(path)),
                Some(("Ubuntu-22.04".into(), "/home/user/.hermes/state.db".into()))
            );
        }
        for path in [
            r"C:\Users\user\.hermes\state.db",
            r"\\server\share\state.db",
            r"\\wsl$\-option\state.db",
            r"\\wsl$\Ubuntu\..\state.db",
        ] {
            assert!(target(Path::new(path)).is_none());
        }
    }
}
