use std::ffi::OsString;
use std::io::Write;
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::{Command, Output, Stdio};

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::app_config::AppType;
use crate::codex_state_db::codex_state_db_paths;
use crate::error::AppError;
use crate::settings::{ManagedTarget, TargetKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TargetArtifactState {
    Missing,
    Valid,
    Invalid,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetInspection {
    pub target_id: String,
    pub reachable: bool,
    pub config: TargetArtifactState,
    pub auth: TargetArtifactState,
    pub active_session_count: usize,
    pub archived_session_count: usize,
    pub state_db_present: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WslTargetDiscovery {
    pub distro: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub home_dir: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config_path: Option<String>,
    pub reachable: bool,
    pub codex_config_present: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WslConfigSnapshot {
    original_config: Option<Vec<u8>>,
    original_catalog: Option<Vec<u8>>,
}

// Keep the script static and pass every dynamic value as a positional argument.
// `/bin/sh -c` is deliberately non-login, so it neither reads the target user's
// shell profile nor interpolates Provider content. One process performs the
// permission lookup, temporary write, digest verification, chmod, and rename.
const WSL_ATOMIC_WRITE_SCRIPT: &str = r#"set -eu
path=$1
temporary=$2
expected_sha256=$3
if [ -f "$path" ]; then
  mode=$(stat -c %a -- "$path")
else
  mode=600
fi
case "$mode" in
  ''|*[!0-7]*) exit 65 ;;
esac
if [ "${#mode}" -lt 3 ] || [ "${#mode}" -gt 4 ]; then
  exit 65
fi
trap 'rm -f -- "$temporary"' EXIT HUP INT TERM
cat > "$temporary"
actual_sha256=$(sha256sum -- "$temporary")
actual_sha256=${actual_sha256%% *}
if [ "$actual_sha256" != "$expected_sha256" ]; then
  exit 66
fi
chmod -- "$mode" "$temporary"
mv -f -- "$temporary" "$path"
trap - EXIT HUP INT TERM
"#;

/// Read-only inspection for a local Windows Managed Target.
pub struct WindowsTargetInspector;

impl WindowsTargetInspector {
    pub fn inspect(target: &ManagedTarget) -> Result<TargetInspection, AppError> {
        if target.app != AppType::Codex {
            return Err(AppError::Message(
                "Windows Target inspection currently supports Codex only".to_string(),
            ));
        }
        if !matches!(target.kind, TargetKind::LocalWindows) {
            return Err(AppError::Message(
                "Windows Target inspector cannot inspect a non-Windows Target".to_string(),
            ));
        }

        let config_dir = Path::new(&target.config_location.path);
        if !config_dir.is_dir() {
            return Ok(TargetInspection {
                target_id: target.id.clone(),
                reachable: false,
                config: TargetArtifactState::Missing,
                auth: TargetArtifactState::Missing,
                active_session_count: 0,
                archived_session_count: 0,
                state_db_present: false,
            });
        }

        let config_path = config_dir.join("config.toml");
        let (config, config_text) = inspect_toml(&config_path);
        let auth = inspect_json(&config_dir.join("auth.json"));
        let state_db_present = codex_state_db_paths(config_dir, &config_text)
            .iter()
            .any(|path| path.is_file());

        Ok(TargetInspection {
            target_id: target.id.clone(),
            reachable: true,
            config,
            auth,
            active_session_count: count_jsonl_files(&config_dir.join("sessions")),
            archived_session_count: count_jsonl_files(&config_dir.join("archived_sessions")),
            state_db_present,
        })
    }
}

/// Read-only adapter for a Codex Target inside WSL. All user-controlled values
/// are passed to `wsl.exe` as individual argv entries and no shell is started.
#[derive(Debug, Clone)]
pub struct WslTargetAdapter {
    executable: OsString,
}

impl Default for WslTargetAdapter {
    fn default() -> Self {
        Self {
            executable: OsString::from("wsl.exe"),
        }
    }
}

impl WslTargetAdapter {
    pub fn with_executable(executable: impl Into<OsString>) -> Self {
        Self {
            executable: executable.into(),
        }
    }

    pub fn discover(&self) -> Result<Vec<WslTargetDiscovery>, AppError> {
        let output = new_wsl_command(&self.executable)
            .args(["--list", "--quiet"])
            .output()
            .map_err(|source| AppError::IoContext {
                context: "Failed to list WSL distributions".to_string(),
                source,
            })?;
        if !output.status.success() {
            return Err(AppError::Message(format!(
                "wsl.exe failed to list distributions (status: {})",
                output.status
            )));
        }

        let mut discovered = Vec::new();
        for distro in decode_wsl_text(&output.stdout)
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
        {
            let user_output = self.run_in_distro(distro, None, &["id", "-un"])?;
            if !user_output.status.success() {
                discovered.push(WslTargetDiscovery {
                    distro: distro.to_string(),
                    user: None,
                    home_dir: None,
                    config_path: None,
                    reachable: false,
                    codex_config_present: false,
                });
                continue;
            }
            let user = String::from_utf8_lossy(&user_output.stdout)
                .trim()
                .to_string();
            if user.is_empty() {
                discovered.push(WslTargetDiscovery {
                    distro: distro.to_string(),
                    user: None,
                    home_dir: None,
                    config_path: None,
                    reachable: false,
                    codex_config_present: false,
                });
                continue;
            }

            let home_output = self.run(distro, &user, &["printenv", "HOME"])?;
            let home = home_output
                .status
                .success()
                .then(|| {
                    String::from_utf8_lossy(&home_output.stdout)
                        .trim()
                        .to_string()
                })
                .filter(|home| home.starts_with('/'));
            let Some(home) = home else {
                discovered.push(WslTargetDiscovery {
                    distro: distro.to_string(),
                    user: Some(user),
                    home_dir: None,
                    config_path: None,
                    reachable: false,
                    codex_config_present: false,
                });
                continue;
            };
            let config_path = linux_join(&home, ".codex");
            let codex_config_present = self
                .run(distro, &user, &["test", "-d", &config_path])?
                .status
                .success();
            discovered.push(WslTargetDiscovery {
                distro: distro.to_string(),
                user: Some(user),
                home_dir: Some(home),
                config_path: Some(config_path),
                reachable: true,
                codex_config_present,
            });
        }
        Ok(discovered)
    }

    /// List Codex sessions inside a WSL Target by reading JSONL files via argv-safe commands.
    pub fn scan_codex_sessions(
        &self,
        target: &ManagedTarget,
    ) -> Result<Vec<crate::session_manager::SessionMeta>, AppError> {
        let TargetKind::Wsl { distro, user } = &target.kind else {
            return Err(AppError::Message(
                "WSL session scan requires a WSL Target".to_string(),
            ));
        };
        validate_wsl_target(distro, user, &target.config_location.path)?;
        let config_dir = target.config_location.path.trim_end_matches('/');
        let mut paths = Vec::new();
        for subdir in ["sessions", "archived_sessions"] {
            let root = linux_join(config_dir, subdir);
            paths.extend(self.list_jsonl_files(distro, user, &root)?);
        }

        let mut sessions = Vec::new();
        for linux_path in paths {
            let Some(bytes) = self.read_file_bytes(distro, user, &linux_path)? else {
                continue;
            };
            let Ok(text) = String::from_utf8(bytes) else {
                continue;
            };
            let source =
                crate::session_manager::encode_wsl_session_source(distro, user, &linux_path);
            if let Some(meta) = crate::session_manager::providers::codex::parse_session_from_text(
                &source,
                &text,
                &std::collections::HashMap::new(),
            ) {
                sessions.push(meta);
            }
        }
        Ok(sessions)
    }

    pub fn read_file_bytes(
        &self,
        distro: &str,
        user: &str,
        path: &str,
    ) -> Result<Option<Vec<u8>>, AppError> {
        self.read_optional_file(distro, user, path)
    }

    pub fn delete_codex_session_file(
        &self,
        distro: &str,
        user: &str,
        linux_path: &str,
        session_id: &str,
    ) -> Result<bool, AppError> {
        if !linux_path.starts_with('/') || linux_path.contains('\0') {
            return Err(AppError::InvalidInput(
                "Invalid WSL session path".to_string(),
            ));
        }
        let Some(bytes) = self.read_file_bytes(distro, user, linux_path)? else {
            return Err(AppError::Message(format!(
                "WSL session file not found: {linux_path}"
            )));
        };
        let text = String::from_utf8(bytes)
            .map_err(|_| AppError::Message("WSL session file is not valid UTF-8".to_string()))?;
        let meta = crate::session_manager::providers::codex::parse_session_from_text(
            linux_path,
            &text,
            &std::collections::HashMap::new(),
        )
        .ok_or_else(|| {
            AppError::Message("Failed to parse WSL Codex session metadata".to_string())
        })?;
        if meta.session_id != session_id {
            return Err(AppError::Message(format!(
                "Codex session ID mismatch: expected {session_id}, found {}",
                meta.session_id
            )));
        }
        self.remove_file(distro, user, linux_path)?;
        Ok(true)
    }

    fn list_jsonl_files(
        &self,
        distro: &str,
        user: &str,
        root: &str,
    ) -> Result<Vec<String>, AppError> {
        if !self
            .run(distro, user, &["test", "-d", root])?
            .status
            .success()
        {
            return Ok(Vec::new());
        }
        let output = self.run(
            distro,
            user,
            &["find", root, "-type", "f", "-name", "*.jsonl", "-print0"],
        )?;
        if !output.status.success() {
            return Ok(Vec::new());
        }
        Ok(output
            .stdout
            .split(|byte| *byte == 0)
            .filter(|entry| !entry.is_empty())
            .filter_map(|entry| String::from_utf8(entry.to_vec()).ok())
            .collect())
    }

    pub fn inspect(&self, target: &ManagedTarget) -> Result<TargetInspection, AppError> {
        if target.app != AppType::Codex {
            return Err(AppError::Message(
                "WSL Target inspection currently supports Codex only".to_string(),
            ));
        }
        let TargetKind::Wsl { distro, user } = &target.kind else {
            return Err(AppError::Message(
                "WSL Target adapter cannot inspect a non-WSL Target".to_string(),
            ));
        };
        validate_wsl_target(distro, user, &target.config_location.path)?;

        let config_dir = target.config_location.path.trim_end_matches('/');
        if !self
            .run(distro, user, &["test", "-d", config_dir])?
            .status
            .success()
        {
            return Ok(TargetInspection {
                target_id: target.id.clone(),
                reachable: false,
                config: TargetArtifactState::Missing,
                auth: TargetArtifactState::Missing,
                active_session_count: 0,
                archived_session_count: 0,
                state_db_present: false,
            });
        }

        let config_path = linux_join(config_dir, "config.toml");
        let auth_path = linux_join(config_dir, "auth.json");
        let (config, config_text) = self.inspect_toml(distro, user, &config_path)?;
        let auth = self.inspect_json(distro, user, &auth_path)?;
        let state_db_present = self.state_db_present(
            distro,
            user,
            config_dir,
            config_text.as_deref().unwrap_or_default(),
        )?;

        Ok(TargetInspection {
            target_id: target.id.clone(),
            reachable: true,
            config,
            auth,
            active_session_count: self.count_jsonl_files(
                distro,
                user,
                &linux_join(config_dir, "sessions"),
            )?,
            archived_session_count: self.count_jsonl_files(
                distro,
                user,
                &linux_join(config_dir, "archived_sessions"),
            )?,
            state_db_present,
        })
    }

    /// Apply only Provider-owned Codex fields inside the distro. The returned
    /// snapshot can restore the exact previous bytes if the registry commit
    /// later fails.
    pub fn apply_provider_config(
        &self,
        target: &ManagedTarget,
        desired_provider_config: &str,
        desired_model_catalog: Option<&[u8]>,
    ) -> Result<WslConfigSnapshot, AppError> {
        let (distro, user, config_path) = wsl_config_coordinates(target)?;
        let catalog_path = linux_join(
            &target.config_location.path,
            crate::codex_config::CC_SWITCH_CODEX_MODEL_CATALOG_FILENAME,
        );
        let snapshot = WslConfigSnapshot {
            original_config: self.read_optional_file(distro, user, &config_path)?,
            original_catalog: self.read_optional_file(distro, user, &catalog_path)?,
        };
        let live_text = match snapshot.original_config.as_deref() {
            Some(bytes) => String::from_utf8(bytes.to_vec()).map_err(|_| {
                AppError::Message("WSL Codex config.toml is not valid UTF-8".to_string())
            })?,
            None => String::new(),
        };
        let projected = crate::codex_config::project_codex_provider_config(
            &live_text,
            desired_provider_config,
        )?;
        projected.parse::<toml::Table>().map_err(|error| {
            AppError::Config(format!(
                "Projected WSL Codex config.toml is invalid: {error}"
            ))
        })?;
        if let Some(catalog) = desired_model_catalog {
            serde_json::from_slice::<serde_json::Value>(catalog).map_err(|error| {
                AppError::Config(format!("Projected WSL model catalog is invalid: {error}"))
            })?;
        }
        let apply_result = match desired_model_catalog {
            Some(catalog) => self
                .write_file_atomic(distro, user, &catalog_path, catalog)
                .and_then(|_| {
                    self.write_file_atomic(distro, user, &config_path, projected.as_bytes())
                }),
            None => self
                .write_file_atomic(distro, user, &config_path, projected.as_bytes())
                .and_then(|_| self.remove_file(distro, user, &catalog_path)),
        };
        if let Err(error) = apply_result {
            self.restore_provider_config(target, &snapshot)
                .map_err(|rollback_error| {
                    AppError::Message(format!(
                        "WSL Provider projection failed: {error}; rollback failed: {rollback_error}"
                    ))
                })?;
            return Err(error);
        }
        Ok(snapshot)
    }

    pub fn restore_provider_config(
        &self,
        target: &ManagedTarget,
        snapshot: &WslConfigSnapshot,
    ) -> Result<(), AppError> {
        let (distro, user, config_path) = wsl_config_coordinates(target)?;
        let catalog_path = linux_join(
            &target.config_location.path,
            crate::codex_config::CC_SWITCH_CODEX_MODEL_CATALOG_FILENAME,
        );
        self.restore_file(
            distro,
            user,
            &catalog_path,
            snapshot.original_catalog.as_deref(),
        )?;
        self.restore_file(
            distro,
            user,
            &config_path,
            snapshot.original_config.as_deref(),
        )
    }

    fn restore_file(
        &self,
        distro: &str,
        user: &str,
        path: &str,
        contents: Option<&[u8]>,
    ) -> Result<(), AppError> {
        match contents {
            Some(bytes) => self.write_file_atomic(distro, user, path, bytes),
            None => self.remove_file(distro, user, path),
        }
    }

    fn remove_file(&self, distro: &str, user: &str, path: &str) -> Result<(), AppError> {
        let output = self.run(distro, user, &["rm", "-f", "--", path])?;
        ensure_wsl_success(output, "remove WSL managed file").map(|_| ())
    }

    fn read_optional_file(
        &self,
        distro: &str,
        user: &str,
        path: &str,
    ) -> Result<Option<Vec<u8>>, AppError> {
        let output = self.run(distro, user, &["cat", "--", path])?;
        if output.status.success() {
            return Ok(Some(output.stdout));
        }
        if !self
            .run(distro, user, &["test", "-e", path])?
            .status
            .success()
        {
            return Ok(None);
        }
        ensure_wsl_success(output, "read WSL managed file").map(Some)
    }

    fn write_file_atomic(
        &self,
        distro: &str,
        user: &str,
        path: &str,
        contents: &[u8],
    ) -> Result<(), AppError> {
        let temporary = format!("{path}.cc-switch-{}.tmp", uuid::Uuid::new_v4());
        let expected_sha256 = format!("{:x}", Sha256::digest(contents));
        let write = self.run_with_input(
            distro,
            user,
            &[
                "/bin/sh",
                "-c",
                WSL_ATOMIC_WRITE_SCRIPT,
                "cc-switch-wsl-write",
                path,
                &temporary,
                &expected_sha256,
            ],
            contents,
        )?;
        ensure_wsl_success(write, "atomically write and verify WSL managed file").map(|_| ())
    }

    fn run_with_input(
        &self,
        distro: &str,
        user: &str,
        args: &[&str],
        input: &[u8],
    ) -> Result<Output, AppError> {
        let mut command = new_wsl_command(&self.executable);
        command
            .arg("-d")
            .arg(distro)
            .arg("-u")
            .arg(user)
            .arg("--exec")
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        let mut child = command.spawn().map_err(|source| AppError::IoContext {
            context: format!("Failed to run command in WSL distro '{distro}'"),
            source,
        })?;
        child
            .stdin
            .take()
            .ok_or_else(|| AppError::Message("Failed to open WSL command stdin".to_string()))?
            .write_all(input)
            .map_err(|source| AppError::IoContext {
                context: "Failed to stream WSL config.toml".to_string(),
                source,
            })?;
        child
            .wait_with_output()
            .map_err(|source| AppError::IoContext {
                context: format!("Failed to wait for command in WSL distro '{distro}'"),
                source,
            })
    }

    fn run(&self, distro: &str, user: &str, args: &[&str]) -> Result<Output, AppError> {
        self.run_in_distro(distro, Some(user), args)
    }

    fn run_in_distro(
        &self,
        distro: &str,
        user: Option<&str>,
        args: &[&str],
    ) -> Result<Output, AppError> {
        let mut command = new_wsl_command(&self.executable);
        command.arg("-d").arg(distro);
        if let Some(user) = user {
            command.arg("-u").arg(user);
        }
        command.arg("--").args(args);
        command.output().map_err(|source| AppError::IoContext {
            context: format!("Failed to run command in WSL distro '{distro}'"),
            source,
        })
    }

    fn inspect_toml(
        &self,
        distro: &str,
        user: &str,
        path: &str,
    ) -> Result<(TargetArtifactState, Option<String>), AppError> {
        if !self.file_exists(distro, user, path)? {
            return Ok((TargetArtifactState::Missing, None));
        }
        let output = self.run(distro, user, &["cat", "--", path])?;
        if !output.status.success() {
            return Ok((TargetArtifactState::Invalid, None));
        }
        let Ok(text) = String::from_utf8(output.stdout) else {
            return Ok((TargetArtifactState::Invalid, None));
        };
        let state = if text.parse::<toml::Table>().is_ok() {
            TargetArtifactState::Valid
        } else {
            TargetArtifactState::Invalid
        };
        Ok((state, Some(text)))
    }

    fn inspect_json(
        &self,
        distro: &str,
        user: &str,
        path: &str,
    ) -> Result<TargetArtifactState, AppError> {
        if !self.file_exists(distro, user, path)? {
            return Ok(TargetArtifactState::Missing);
        }
        let output = self.run(distro, user, &["cat", "--", path])?;
        if !output.status.success() {
            return Ok(TargetArtifactState::Invalid);
        }
        Ok(
            if serde_json::from_slice::<serde_json::Value>(&output.stdout).is_ok() {
                TargetArtifactState::Valid
            } else {
                TargetArtifactState::Invalid
            },
        )
    }

    fn file_exists(&self, distro: &str, user: &str, path: &str) -> Result<bool, AppError> {
        Ok(self
            .run(distro, user, &["test", "-f", path])?
            .status
            .success())
    }

    fn count_jsonl_files(&self, distro: &str, user: &str, root: &str) -> Result<usize, AppError> {
        if !self
            .run(distro, user, &["test", "-d", root])?
            .status
            .success()
        {
            return Ok(0);
        }
        let output = self.run(
            distro,
            user,
            &["find", root, "-type", "f", "-name", "*.jsonl", "-print0"],
        )?;
        if !output.status.success() {
            return Ok(0);
        }
        Ok(output
            .stdout
            .split(|byte| *byte == 0)
            .filter(|entry| !entry.is_empty())
            .count())
    }

    fn state_db_present(
        &self,
        distro: &str,
        user: &str,
        config_dir: &str,
        config_text: &str,
    ) -> Result<bool, AppError> {
        let mut homes = vec![config_dir.to_string()];
        let configured_home = config_text
            .parse::<toml::Table>()
            .ok()
            .and_then(|table| table.get("sqlite_home")?.as_str().map(str::to_string));
        let override_home = match configured_home {
            Some(home) => Some(home),
            None => self.read_env(distro, user, "CODEX_SQLITE_HOME")?,
        };
        if let Some(raw_home) = override_home {
            if let Some(home) = self.resolve_linux_user_path(distro, user, &raw_home)? {
                if !homes.contains(&home) {
                    homes.push(home);
                }
            }
        }

        for home in homes {
            if self.file_exists(distro, user, &linux_join(&home, "state_5.sqlite"))? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn read_env(&self, distro: &str, user: &str, name: &str) -> Result<Option<String>, AppError> {
        let output = self.run(distro, user, &["printenv", name])?;
        if !output.status.success() {
            return Ok(None);
        }
        let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Ok((!value.is_empty()).then_some(value))
    }

    fn resolve_linux_user_path(
        &self,
        distro: &str,
        user: &str,
        raw: &str,
    ) -> Result<Option<String>, AppError> {
        let raw = raw.trim();
        if raw.is_empty() {
            return Ok(None);
        }
        if raw.starts_with('/') {
            return Ok(Some(raw.trim_end_matches('/').to_string()));
        }
        let Some(home) = self.read_env(distro, user, "HOME")? else {
            return Ok(None);
        };
        if raw == "~" {
            return Ok(Some(home));
        }
        if let Some(relative) = raw.strip_prefix("~/") {
            return Ok(Some(linux_join(&home, relative)));
        }
        Ok(Some(linux_join(&home, raw)))
    }
}

fn wsl_config_coordinates(target: &ManagedTarget) -> Result<(&str, &str, String), AppError> {
    if target.app != AppType::Codex {
        return Err(AppError::Message(
            "WSL config writes currently support Codex only".to_string(),
        ));
    }
    let TargetKind::Wsl { distro, user } = &target.kind else {
        return Err(AppError::Message(
            "WSL adapter cannot write a non-WSL Target".to_string(),
        ));
    };
    validate_wsl_target(distro, user, &target.config_location.path)?;
    Ok((
        distro,
        user,
        linux_join(&target.config_location.path, "config.toml"),
    ))
}

fn ensure_wsl_success(output: Output, action: &str) -> Result<Vec<u8>, AppError> {
    if output.status.success() {
        return Ok(output.stdout);
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    Err(AppError::Message(format!(
        "Failed to {action} (status: {}): {}",
        output.status,
        stderr.trim()
    )))
}

fn decode_wsl_text(bytes: &[u8]) -> String {
    if bytes.contains(&0) {
        let words = bytes
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect::<Vec<_>>();
        return String::from_utf16_lossy(&words)
            .trim_start_matches('\u{feff}')
            .to_string();
    }
    String::from_utf8_lossy(bytes)
        .trim_start_matches('\u{feff}')
        .to_string()
}

fn validate_wsl_target(distro: &str, user: &str, config_dir: &str) -> Result<(), AppError> {
    if distro.trim().is_empty() || user.trim().is_empty() {
        return Err(AppError::InvalidInput(
            "WSL distro and user must not be empty".to_string(),
        ));
    }
    if !config_dir.starts_with('/') || config_dir.contains('\0') {
        return Err(AppError::InvalidInput(
            "WSL Target config path must be an absolute Linux path".to_string(),
        ));
    }
    Ok(())
}

fn linux_join(root: &str, child: &str) -> String {
    format!(
        "{}/{}",
        root.trim_end_matches('/'),
        child.trim_start_matches('/')
    )
}

fn inspect_toml(path: &Path) -> (TargetArtifactState, String) {
    if !path.is_file() {
        return (TargetArtifactState::Missing, String::new());
    }
    match std::fs::read_to_string(path) {
        Ok(text) if text.parse::<toml::Table>().is_ok() => (TargetArtifactState::Valid, text),
        Ok(text) => (TargetArtifactState::Invalid, text),
        Err(_) => (TargetArtifactState::Invalid, String::new()),
    }
}

fn inspect_json(path: &Path) -> TargetArtifactState {
    if !path.is_file() {
        return TargetArtifactState::Missing;
    }
    match std::fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
    {
        Some(_) => TargetArtifactState::Valid,
        None => TargetArtifactState::Invalid,
    }
}

fn count_jsonl_files(root: &Path) -> usize {
    let Ok(entries) = std::fs::read_dir(root) else {
        return 0;
    };
    entries
        .flatten()
        .map(|entry| entry.path())
        .map(|path| {
            if path.is_dir() {
                count_jsonl_files(&path)
            } else if path.extension().and_then(|extension| extension.to_str()) == Some("jsonl") {
                1
            } else {
                0
            }
        })
        .sum()
}

fn new_wsl_command(executable: &OsString) -> Command {
    let command = Command::new(executable);
    #[cfg(windows)]
    let command = {
        let mut command = command;
        command.creation_flags(wsl_process_creation_flags());
        command
    };
    command
}

#[cfg(any(windows, test))]
fn wsl_process_creation_flags() -> u32 {
    0x0800_0000
}

#[cfg(test)]
mod tests {
    #[test]
    fn wsl_processes_are_configured_without_a_console_window() {
        assert_eq!(super::wsl_process_creation_flags(), 0x0800_0000);
    }
}
