use std::io::{self, BufRead, BufReader, Read};
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::Value;

const READER: &str = include_str!("hermes_wsl_reader.py");
const MAX_FRAME: u64 = 1024 * 1024;

/// Decode bounded JSON-string frames as a byte stream. Deserializing directly
/// avoids retaining another complete encoded copy of a large raw transcript.
/// Projection and duplicate selection remain shared with the native provider.
struct FramedReader<R> {
    source: BufReader<R>,
    frame: io::Cursor<Vec<u8>>,
}

impl<R: Read> FramedReader<R> {
    fn new(source: R) -> Self {
        Self {
            source: BufReader::new(source),
            frame: io::Cursor::new(Vec::new()),
        }
    }
}

impl<R: Read> Read for FramedReader<R> {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        if output.is_empty() {
            return Ok(0);
        }
        loop {
            let count = self.frame.read(output)?;
            if count != 0 {
                return Ok(count);
            }
            let mut line = Vec::new();
            let count = self
                .source
                .by_ref()
                .take(MAX_FRAME + 1)
                .read_until(b'\n', &mut line)?;
            if count == 0 {
                return Ok(0);
            }
            if count as u64 > MAX_FRAME || line.last() != Some(&b'\n') {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Hermes WSL frame exceeds its limit or is incomplete",
                ));
            }
            let decoded: String = serde_json::from_slice(&line)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
            self.frame = io::Cursor::new(decoded.into_bytes());
        }
    }
}

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
        serde_json::from_reader::<_, Value>(BufReader::new(FramedReader::new(stdout)))
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
        .map_err(|e| format!("Invalid Hermes WSL response: {e}"));
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
    stdout
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn framed_json_preserves_escape_boundaries_and_rejects_invalid_frames() {
        let frames = ["[\"", "\\", "n中文\"]"];
        let stream = frames
            .iter()
            .map(|s| format!("{}\n", serde_json::to_string(s).unwrap()))
            .collect::<String>();
        let value: Value = serde_json::from_reader(FramedReader::new(stream.as_bytes())).unwrap();
        assert_eq!(value, serde_json::json!(["\n中文"]));
        for invalid in ["\"unfinished\"", "123\n"] {
            assert!(
                serde_json::from_reader::<_, Value>(FramedReader::new(invalid.as_bytes())).is_err()
            );
        }
        let oversized = format!("\"{}\"\n", "x".repeat(MAX_FRAME as usize));
        assert!(
            serde_json::from_reader::<_, Value>(FramedReader::new(oversized.as_bytes())).is_err()
        );
    }

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
