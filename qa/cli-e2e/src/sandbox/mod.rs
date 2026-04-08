use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::Mutex;
use walkdir::WalkDir;

use crate::runner::HarnessEnv;

#[derive(Debug, Clone)]
pub struct CommandOutput {
    pub status: i32,
    pub stdout: String,
    pub stderr: String,
}

impl CommandOutput {
    pub fn success(&self) -> bool {
        self.status == 0
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SessionResponse {
    ok: bool,
    stdout: String,
    stderr: String,
    error: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum SessionRequest<'a> {
    Run { args: &'a [String] },
    Shutdown,
}

pub struct Sandbox {
    env: HarnessEnv,
    scenario_name: String,
    root_dir: PathBuf,
    home_dir: PathBuf,
    notes: Vec<String>,
}

pub struct CliSession {
    child: Child,
    stdin: ChildStdin,
    stdout: tokio::io::BufReader<ChildStdout>,
    stderr_buffer: Arc<Mutex<String>>,
    command_log_path: PathBuf,
}

impl Sandbox {
    pub fn new(env: &HarnessEnv, scenario_name: &str) -> Result<Self> {
        let timestamp = Utc::now().format("%Y%m%dT%H%M%S%.3fZ").to_string();
        let root_dir = env.artifacts_dir.join(scenario_name).join(timestamp);
        let home_dir = root_dir.join("home");

        fs::create_dir_all(&home_dir)
            .with_context(|| format!("failed to create {}", home_dir.display()))?;
        fs::File::create(root_dir.join("command.log"))
            .with_context(|| format!("failed to create command log in {}", root_dir.display()))?;

        Ok(Self {
            env: env.clone(),
            scenario_name: scenario_name.to_string(),
            root_dir,
            home_dir,
            notes: Vec::new(),
        })
    }

    pub fn fixture_path(&self, relative: &str) -> PathBuf {
        self.env.fixtures_dir.join(relative)
    }

    pub fn home_path(&self, relative: &str) -> PathBuf {
        self.home_dir.join(relative)
    }

    pub fn work_path(&self, relative: &str) -> PathBuf {
        self.root_dir.join(relative)
    }

    pub fn add_note(&mut self, note: impl Into<String>) {
        self.notes.push(note.into());
    }

    pub fn stage_home_fixture(&self, relative_fixture_dir: &str) -> Result<()> {
        copy_dir_contents(&self.fixture_path(relative_fixture_dir), &self.home_dir)
    }

    pub fn stage_fixture_to_path(&self, relative_fixture: &str, destination: &Path) -> Result<()> {
        let source = self.fixture_path(relative_fixture);
        if source.is_dir() {
            copy_dir_contents(&source, destination)
        } else {
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("failed to create {}", parent.display()))?;
            }
            fs::copy(&source, destination).with_context(|| {
                format!(
                    "failed to copy {} to {}",
                    source.display(),
                    destination.display()
                )
            })?;
            Ok(())
        }
    }

    pub fn write_home_text(&self, relative: &str, content: &str) -> Result<()> {
        self.write_text(&self.home_path(relative), content)
    }

    pub fn write_text(&self, path: &Path, content: &str) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        fs::write(path, content).with_context(|| format!("failed to write {}", path.display()))
    }

    pub async fn run(&mut self, args: &[String]) -> Result<CommandOutput> {
        self.run_with_env(args, &[]).await
    }

    pub async fn run_with_env(
        &mut self,
        args: &[String],
        extra_envs: &[(&str, &str)],
    ) -> Result<CommandOutput> {
        self.log_command(args, None)?;

        let output = Command::new(&self.env.bin_path)
            .args(args)
            .current_dir(&self.env.repo_root)
            .env("HOME", &self.home_dir)
            .env("CC_SWITCH_TEST_HOME", &self.home_dir)
            .env("NO_COLOR", "1")
            .envs(extra_envs.iter().copied())
            .output()
            .await
            .with_context(|| format!("failed to run {}", self.env.bin_path.display()))?;

        let result = CommandOutput {
            status: output.status.code().unwrap_or(1),
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        };
        self.log_output(&result)?;
        Ok(result)
    }

    pub async fn run_ok(&mut self, args: &[String]) -> Result<CommandOutput> {
        self.run_ok_with_env(args, &[]).await
    }

    pub async fn run_ok_with_env(
        &mut self,
        args: &[String],
        extra_envs: &[(&str, &str)],
    ) -> Result<CommandOutput> {
        let output = self.run_with_env(args, extra_envs).await?;
        if output.success() {
            Ok(output)
        } else {
            Err(anyhow!(
                "command failed (exit={}): {}\nstderr:\n{}",
                output.status,
                format_args(args),
                output.stderr
            ))
        }
    }

    pub async fn start_session(&mut self) -> Result<CliSession> {
        let mut child = Command::new(&self.env.bin_path)
            .arg("__e2e-session")
            .current_dir(&self.env.repo_root)
            .env("HOME", &self.home_dir)
            .env("CC_SWITCH_TEST_HOME", &self.home_dir)
            .env("NO_COLOR", "1")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .with_context(|| format!("failed to start {}", self.env.bin_path.display()))?;

        let stdin = child
            .stdin
            .take()
            .context("failed to capture __e2e-session stdin")?;
        let stdout = child
            .stdout
            .take()
            .context("failed to capture __e2e-session stdout")?;
        let stderr = child
            .stderr
            .take()
            .context("failed to capture __e2e-session stderr")?;

        let stderr_buffer = Arc::new(Mutex::new(String::new()));
        let stderr_target = stderr_buffer.clone();
        tokio::spawn(async move {
            let mut reader = tokio::io::BufReader::new(stderr).lines();
            while let Ok(Some(line)) = reader.next_line().await {
                let mut guard = stderr_target.lock().await;
                guard.push_str(&line);
                guard.push('\n');
            }
        });

        self.add_note("started hidden __e2e-session");

        Ok(CliSession {
            child,
            stdin,
            stdout: tokio::io::BufReader::new(stdout),
            stderr_buffer,
            command_log_path: self.root_dir.join("command.log"),
        })
    }

    pub fn finalize(&self, success: bool, mock_requests: Option<String>) -> Result<()> {
        let stdout_path = self.root_dir.join("stdout.txt");
        let stderr_path = self.root_dir.join("stderr.txt");
        if !stdout_path.exists() {
            fs::write(&stdout_path, "")
                .with_context(|| format!("failed to create {}", stdout_path.display()))?;
        }
        if !stderr_path.exists() {
            fs::write(&stderr_path, "")
                .with_context(|| format!("failed to create {}", stderr_path.display()))?;
        }

        let tree = build_tree_snapshot(&self.home_dir)?;
        fs::write(self.root_dir.join("sandbox-tree.txt"), tree)
            .with_context(|| format!("failed to write sandbox tree for {}", self.scenario_name))?;

        snapshot_live_configs(&self.home_dir, &self.root_dir.join("live-config"))?;

        fs::write(
            self.root_dir.join("mock-requests.json"),
            mock_requests.unwrap_or_else(|| "[]".to_string()),
        )
        .with_context(|| format!("failed to write mock requests for {}", self.scenario_name))?;

        fs::write(self.root_dir.join("notes.md"), self.notes.join("\n"))
            .with_context(|| format!("failed to write notes for {}", self.scenario_name))?;

        if success && !self.env.keep_artifacts {
            fs::remove_dir_all(&self.root_dir)
                .with_context(|| format!("failed to cleanup {}", self.root_dir.display()))?;
        }

        Ok(())
    }

    fn log_command(&self, args: &[String], prefix: Option<&str>) -> Result<()> {
        let mut file = OpenOptions::new()
            .append(true)
            .open(self.root_dir.join("command.log"))
            .with_context(|| format!("failed to open command log for {}", self.scenario_name))?;
        if let Some(prefix) = prefix {
            writeln!(file, "[{}] {}", prefix, format_args(args))?;
        } else {
            writeln!(file, "$ {}", format_args(args))?;
        }
        Ok(())
    }

    fn log_output(&self, output: &CommandOutput) -> Result<()> {
        append_text(&self.root_dir.join("stdout.txt"), &output.stdout)?;
        append_text(&self.root_dir.join("stderr.txt"), &output.stderr)?;

        let mut file = OpenOptions::new()
            .append(true)
            .open(self.root_dir.join("command.log"))
            .with_context(|| format!("failed to open command log for {}", self.scenario_name))?;
        writeln!(file, "exit: {}", output.status)?;
        if !output.stdout.trim().is_empty() {
            writeln!(file, "stdout:\n{}", output.stdout)?;
        }
        if !output.stderr.trim().is_empty() {
            writeln!(file, "stderr:\n{}", output.stderr)?;
        }
        writeln!(file)?;
        Ok(())
    }
}

impl CliSession {
    pub async fn run(&mut self, args: &[String]) -> Result<CommandOutput> {
        let request = SessionRequest::Run { args };
        let payload = serde_json::to_string(&request)?;
        self.stdin.write_all(payload.as_bytes()).await?;
        self.stdin.write_all(b"\n").await?;
        self.stdin.flush().await?;

        append_text(
            &self.command_log_path,
            &format!("[session] {}\n", format_args(args)),
        )?;

        let mut line = String::new();
        let bytes = self.stdout.read_line(&mut line).await?;
        if bytes == 0 {
            let stderr = self.stderr_snapshot().await;
            return Err(anyhow!(
                "__e2e-session closed unexpectedly while running {}\nstderr:\n{}",
                format_args(args),
                stderr
            ));
        }

        let response: SessionResponse = serde_json::from_str(line.trim())
            .with_context(|| format!("invalid __e2e-session response: {line}"))?;

        let output = CommandOutput {
            status: if response.ok { 0 } else { 1 },
            stdout: response.stdout,
            stderr: if let Some(error) = response.error {
                if response.stderr.trim().is_empty() {
                    error
                } else {
                    format!("{}\n{}", response.stderr, error)
                }
            } else {
                response.stderr
            },
        };

        append_text(
            &self.command_log_path,
            &format!(
                "exit: {}\nstdout:\n{}\nstderr:\n{}\n\n",
                output.status, output.stdout, output.stderr
            ),
        )?;
        Ok(output)
    }

    pub async fn run_ok(&mut self, args: &[String]) -> Result<CommandOutput> {
        let output = self.run(args).await?;
        if output.success() {
            Ok(output)
        } else {
            Err(anyhow!(
                "session command failed: {}\nstderr:\n{}",
                format_args(args),
                output.stderr
            ))
        }
    }

    pub async fn close(mut self) -> Result<()> {
        let payload = serde_json::to_string(&SessionRequest::Shutdown)?;
        self.stdin.write_all(payload.as_bytes()).await?;
        self.stdin.write_all(b"\n").await?;
        self.stdin.flush().await?;
        let _ = self.child.wait().await;
        Ok(())
    }

    pub async fn stderr_snapshot(&self) -> String {
        self.stderr_buffer.lock().await.clone()
    }
}

fn append_text(path: &Path, text: &str) -> Result<()> {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("failed to open {}", path.display()))?;
    file.write_all(text.as_bytes())
        .with_context(|| format!("failed to append {}", path.display()))
}

fn format_args(args: &[String]) -> String {
    args.iter()
        .map(|arg| {
            if arg.contains(' ') {
                format!("{arg:?}")
            } else {
                arg.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn copy_dir_contents(source: &Path, destination: &Path) -> Result<()> {
    for entry in WalkDir::new(source) {
        let entry = entry?;
        let relative = entry.path().strip_prefix(source)?;
        let target = destination.join(relative);
        if entry.file_type().is_dir() {
            fs::create_dir_all(&target)
                .with_context(|| format!("failed to create {}", target.display()))?;
        } else if entry.file_type().is_symlink() {
            let resolved = fs::canonicalize(entry.path())
                .with_context(|| format!("failed to resolve symlink {}", entry.path().display()))?;
            let metadata = fs::metadata(&resolved)
                .with_context(|| format!("failed to inspect resolved {}", resolved.display()))?;
            if metadata.is_dir() {
                copy_dir_contents(&resolved, &target)?;
            } else {
                if let Some(parent) = target.parent() {
                    fs::create_dir_all(parent)
                        .with_context(|| format!("failed to create {}", parent.display()))?;
                }
                fs::copy(&resolved, &target).with_context(|| {
                    format!(
                        "failed to copy resolved {} to {}",
                        resolved.display(),
                        target.display()
                    )
                })?;
            }
        } else {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("failed to create {}", parent.display()))?;
            }
            fs::copy(entry.path(), &target).with_context(|| {
                format!(
                    "failed to copy {} to {}",
                    entry.path().display(),
                    target.display()
                )
            })?;
        }
    }
    Ok(())
}

fn build_tree_snapshot(root: &Path) -> Result<String> {
    let mut lines = Vec::new();
    for entry in WalkDir::new(root) {
        let entry = entry?;
        let relative = entry.path().strip_prefix(root)?;
        if relative.as_os_str().is_empty() {
            lines.push(".".to_string());
        } else {
            lines.push(relative.display().to_string());
        }
    }
    Ok(lines.join("\n"))
}

fn snapshot_live_configs(home_dir: &Path, snapshot_dir: &Path) -> Result<()> {
    let mappings = [
        (home_dir.join(".claude"), snapshot_dir.join(".claude")),
        (
            home_dir.join(".claude.json"),
            snapshot_dir.join(".claude.json"),
        ),
        (home_dir.join(".codex"), snapshot_dir.join(".codex")),
        (home_dir.join(".gemini"), snapshot_dir.join(".gemini")),
        (
            home_dir.join(".config").join("opencode"),
            snapshot_dir.join(".config").join("opencode"),
        ),
        (home_dir.join(".openclaw"), snapshot_dir.join(".openclaw")),
        (home_dir.join(".cc-switch"), snapshot_dir.join(".cc-switch")),
    ];

    for (source, destination) in mappings {
        if !source.exists() {
            continue;
        }
        if source.is_dir() {
            copy_dir_contents(&source, &destination)?;
        } else {
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("failed to create {}", parent.display()))?;
            }
            fs::copy(&source, &destination).with_context(|| {
                format!(
                    "failed to copy {} to {}",
                    source.display(),
                    destination.display()
                )
            })?;
        }
    }

    Ok(())
}
