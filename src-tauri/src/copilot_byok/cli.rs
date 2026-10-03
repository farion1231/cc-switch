use super::model::{CopilotByokGroup, CopilotByokModel};
use super::store;
use super::store::CopilotByokStore;
use super::store::{CopilotCliConfig, CopilotCliManagedEnvironment};
use crate::error::AppError;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, VecDeque};
#[cfg(any(unix, test))]
use std::fs;
#[cfg(any(unix, test))]
use std::path::Path;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const MANAGED_VARIABLES: &[&str] = &[
    "COPILOT_PROVIDER_BASE_URL",
    "COPILOT_PROVIDER_TYPE",
    "COPILOT_PROVIDER_API_KEY",
    "COPILOT_PROVIDER_BEARER_TOKEN",
    "COPILOT_PROVIDER_WIRE_API",
    "COPILOT_PROVIDER_TRANSPORT",
    "COPILOT_PROVIDER_AZURE_API_VERSION",
    "COPILOT_PROVIDER_HEADERS",
    "COPILOT_MODEL",
    "COPILOT_PROVIDER_MODEL_ID",
    "COPILOT_PROVIDER_WIRE_MODEL",
    "COPILOT_PROVIDER_MAX_PROMPT_TOKENS",
    "COPILOT_PROVIDER_MAX_OUTPUT_TOKENS",
];

const CLI_PROVIDER_TYPE_KEY: &str = "ccSwitchCopilotCliProviderType";
const CLI_BEARER_TOKEN_KEY: &str = "ccSwitchCopilotCliBearerToken";
const CLI_TRANSPORT_KEY: &str = "ccSwitchCopilotCliTransport";
const CLI_AZURE_API_VERSION_KEY: &str = "ccSwitchCopilotCliAzureApiVersion";
const CLI_MODEL_KEY: &str = "ccSwitchCopilotCliModel";
const CLI_MODEL_ID_KEY: &str = "ccSwitchCopilotCliModelId";
const CLI_WIRE_MODEL_KEY: &str = "ccSwitchCopilotCliWireModel";

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CopilotCliState {
    pub supported: bool,
    pub enabled: bool,
    pub selected_group_id: Option<String>,
    pub selected_model_id: Option<String>,
    pub selected_provider_name: Option<String>,
    pub selected_model_name: Option<String>,
    pub environment_matches: bool,
    /// Variable names only. Secret values are never returned to the frontend.
    pub environment_conflicts: Vec<String>,
    /// Selecting the official provider would remove overrides that CC Switch
    /// does not currently own, so the frontend must obtain explicit consent.
    pub official_activation_requires_confirmation: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CopilotCliEnvironmentValue {
    pub is_set: bool,
    /// Credential-bearing values never leave the backend, even in a preview.
    pub value: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CopilotCliEnvironmentChange {
    pub name: String,
    pub source: String,
    pub sensitive: bool,
    pub conflict: bool,
    pub previous: CopilotCliEnvironmentValue,
    pub current: CopilotCliEnvironmentValue,
    pub desired: CopilotCliEnvironmentValue,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CopilotCliEnvironmentPreview {
    pub group_id: String,
    pub changes: Vec<CopilotCliEnvironmentChange>,
    pub integration_conflicts: Vec<String>,
    pub confirmation_token: String,
}

type EnvironmentValues = BTreeMap<String, Option<String>>;
type ConfirmationContext = BTreeMap<PathBuf, Option<String>>;

// Keep unredacted snapshots only in process memory, behind random, single-use
// tokens. A confirmation is valid only for the exact environment and provider
// configuration the user inspected, and expires after five minutes.
struct PendingConfirmation {
    token: String,
    created: Instant,
    group_id: String,
    current: EnvironmentValues,
    desired: EnvironmentValues,
    context: ConfirmationContext,
}

static PENDING_CONFIRMATIONS: Mutex<VecDeque<PendingConfirmation>> = Mutex::new(VecDeque::new());
const CONFIRMATION_TTL: Duration = Duration::from_secs(300);

trait UserEnvironment {
    fn read(&self, name: &str) -> Result<Option<String>, AppError>;
    fn write(&self, name: &str, value: Option<&str>) -> Result<(), AppError>;
    fn broadcast_change(
        &self,
        _before: &BTreeMap<String, Option<String>>,
        _after: &BTreeMap<String, Option<String>>,
    ) -> Result<(), AppError>;
    fn external_conflicts(
        &self,
        _expected: &BTreeMap<String, Option<String>>,
    ) -> Result<Vec<String>, AppError> {
        Ok(Vec::new())
    }

    fn source(&self) -> String {
        "HKEY_CURRENT_USER\\Environment".to_string()
    }

    fn confirmation_context(&self) -> Result<ConfirmationContext, AppError> {
        Ok(BTreeMap::new())
    }

    fn restore_confirmation_context(&self, _context: &ConfirmationContext) -> Result<(), AppError> {
        Ok(())
    }

    fn broadcast_confirmed_change(
        &self,
        before: &EnvironmentValues,
        after: &EnvironmentValues,
    ) -> Result<(), AppError> {
        self.broadcast_change(before, after)
    }
}

#[cfg(windows)]
struct WindowsUserEnvironment;

#[cfg(windows)]
impl WindowsUserEnvironment {
    fn environment_key(&self) -> Result<winreg::RegKey, AppError> {
        use winreg::enums::HKEY_CURRENT_USER;
        winreg::RegKey::predef(HKEY_CURRENT_USER)
            .create_subkey("Environment")
            .map(|(key, _)| key)
            .map_err(|error| AppError::IoContext {
                context: "Failed to open HKEY_CURRENT_USER\\Environment".to_string(),
                source: error,
            })
    }
}

#[cfg(windows)]
impl UserEnvironment for WindowsUserEnvironment {
    fn read(&self, name: &str) -> Result<Option<String>, AppError> {
        use winreg::enums::HKEY_CURRENT_USER;
        let key = match winreg::RegKey::predef(HKEY_CURRENT_USER).open_subkey("Environment") {
            Ok(key) => key,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(AppError::IoContext {
                    context: "Failed to read HKEY_CURRENT_USER\\Environment".to_string(),
                    source: error,
                })
            }
        };
        match key.get_value::<String, _>(name) {
            Ok(value) => Ok(Some(value)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(AppError::IoContext {
                context: format!("Failed to read user environment variable {name}"),
                source: error,
            }),
        }
    }

    fn write(&self, name: &str, value: Option<&str>) -> Result<(), AppError> {
        let key = self.environment_key()?;
        match value {
            Some(value) => key.set_value(name, &value),
            None => match key.delete_value(name) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(error) => Err(error),
            },
        }
        .map_err(|error| AppError::IoContext {
            context: format!("Failed to update user environment variable {name}"),
            source: error,
        })
    }

    fn broadcast_change(
        &self,
        _before: &BTreeMap<String, Option<String>>,
        _after: &BTreeMap<String, Option<String>>,
    ) -> Result<(), AppError> {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            SendMessageTimeoutW, HWND_BROADCAST, SMTO_ABORTIFHUNG, WM_SETTINGCHANGE,
        };

        let environment: Vec<u16> = std::ffi::OsStr::new("Environment")
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let mut result = 0_usize;
        let delivered = unsafe {
            SendMessageTimeoutW(
                HWND_BROADCAST,
                WM_SETTINGCHANGE,
                0,
                environment.as_ptr() as isize,
                SMTO_ABORTIFHUNG,
                5_000,
                &mut result,
            )
        };
        if delivered == 0 {
            return Err(AppError::Config(
                "Updated the registry but failed to broadcast the environment change".to_string(),
            ));
        }
        Ok(())
    }
}

#[cfg(any(unix, test))]
const POSIX_BLOCK_START: &str = "# >>> CC Switch Copilot CLI >>>";
#[cfg(any(unix, test))]
const POSIX_BLOCK_END: &str = "# <<< CC Switch Copilot CLI <<<";
#[cfg(any(unix, test))]
const FISH_HOOK_HEADER: &str = "# Managed by CC Switch for GitHub Copilot CLI";
#[cfg(any(unix, test))]
const MAX_SHELL_PROFILE_BYTES: u64 = 2 * 1024 * 1024;

#[cfg(any(unix, test))]
struct UnixUserEnvironment {
    home: PathBuf,
}

#[cfg(any(unix, test))]
impl UnixUserEnvironment {
    fn new() -> Result<Self, AppError> {
        let home = dirs::home_dir().ok_or_else(|| {
            AppError::Config("Cannot determine home directory for Copilot CLI".to_string())
        })?;
        Ok(Self { home })
    }

    fn env_path(&self) -> PathBuf {
        self.home.join(".cc-switch").join("copilot-cli-env.sh")
    }

    fn fish_env_path(&self) -> PathBuf {
        self.home.join(".cc-switch").join("copilot-cli-env.fish")
    }

    fn fish_hook_path(&self) -> PathBuf {
        self.home
            .join(".config")
            .join("fish")
            .join("conf.d")
            .join("cc-switch-copilot.fish")
    }

    fn profile_paths(&self) -> Vec<PathBuf> {
        // Create the common interactive-shell files so a fresh macOS/Linux
        // account works immediately in a newly opened terminal. Add existing
        // login-shell files too, covering custom Bash/Zsh startup layouts.
        let mut paths = [".profile", ".bashrc", ".zshrc"]
            .into_iter()
            .map(|name| self.home.join(name))
            .collect::<Vec<_>>();
        for name in [".bash_profile", ".bash_login", ".zprofile"] {
            let path = self.home.join(name);
            if path.exists() || profile_contains_marker(&path) {
                paths.push(path);
            }
        }
        paths
    }

    fn read_values(&self) -> Result<BTreeMap<String, Option<String>>, AppError> {
        read_posix_env_file(&self.env_path())
    }

    fn write_values(&self, values: &BTreeMap<String, Option<String>>) -> Result<(), AppError> {
        let has_values = values.values().any(Option::is_some);
        if !has_values {
            remove_regular_file_if_exists(&self.env_path(), "Copilot CLI environment file")?;
            remove_regular_file_if_exists(
                &self.fish_env_path(),
                "Copilot CLI fish environment file",
            )?;
            return Ok(());
        }

        crate::config::atomic_write_private(
            &self.env_path(),
            render_posix_env(values)?.as_bytes(),
        )?;
        if let Err(error) = crate::config::atomic_write_private(
            &self.fish_env_path(),
            render_fish_env(values)?.as_bytes(),
        ) {
            return Err(AppError::Config(format!(
                "Failed to write Copilot CLI fish environment: {error}"
            )));
        }
        Ok(())
    }

    fn validate_managed_artifacts(
        &self,
        expected: &BTreeMap<String, Option<String>>,
    ) -> Result<Vec<String>, AppError> {
        let mut conflicts = Vec::new();
        let enabled = expected.values().any(Option::is_some);
        if enabled {
            if read_optional_regular_file(&self.env_path(), "Copilot CLI environment")?.as_deref()
                != Some(render_posix_env(expected)?.as_str())
            {
                conflicts.push(self.env_path().to_string_lossy().to_string());
            }
            if read_optional_regular_file(&self.fish_env_path(), "Copilot CLI fish environment")?
                .as_deref()
                != Some(render_fish_env(expected)?.as_str())
            {
                conflicts.push(self.fish_env_path().to_string_lossy().to_string());
            }
            for path in self.profile_paths() {
                let content =
                    read_optional_regular_file(&path, "shell profile")?.unwrap_or_default();
                if !matches!(managed_block_state(&content), Ok(ManagedBlockState::Exact)) {
                    conflicts.push(path.to_string_lossy().to_string());
                }
            }
            let fish_hook = read_optional_regular_file(&self.fish_hook_path(), "fish hook")?;
            if fish_hook.as_deref() != Some(fish_hook_contents().as_str()) {
                conflicts.push(self.fish_hook_path().to_string_lossy().to_string());
            }
        } else {
            if read_optional_regular_file(&self.env_path(), "Copilot CLI environment")?.is_some() {
                conflicts.push(self.env_path().to_string_lossy().to_string());
            }
            if read_optional_regular_file(&self.fish_env_path(), "Copilot CLI fish environment")?
                .is_some()
            {
                conflicts.push(self.fish_env_path().to_string_lossy().to_string());
            }
            for path in self.profile_paths() {
                let Some(content) = read_optional_regular_file(&path, "shell profile")? else {
                    continue;
                };
                if !matches!(
                    managed_block_state(&content),
                    Ok(ManagedBlockState::Missing)
                ) {
                    conflicts.push(path.to_string_lossy().to_string());
                }
            }
            if read_optional_regular_file(&self.fish_hook_path(), "fish hook")?.is_some() {
                conflicts.push(self.fish_hook_path().to_string_lossy().to_string());
            }
        }
        Ok(conflicts)
    }

    fn update_shell_hooks(&self, enabled: bool) -> Result<(), AppError> {
        self.update_shell_hooks_with_confirmation(enabled, false)
    }

    fn update_shell_hooks_with_confirmation(
        &self,
        enabled: bool,
        confirmed: bool,
    ) -> Result<(), AppError> {
        use crate::file_transaction::{commit_file_updates, FileUpdate};

        let mut updates = Vec::new();
        for path in self.profile_paths() {
            let content = read_optional_regular_file(&path, "shell profile")?.unwrap_or_default();
            let updated = if confirmed {
                update_confirmed_managed_block(&content, enabled)?
            } else {
                update_managed_block(&content, enabled)?
            };
            if updated != content {
                updates.push(FileUpdate::write(path, updated.into_bytes()));
            }
        }
        let fish_hook_path = self.fish_hook_path();
        if enabled {
            updates.push(FileUpdate::write(
                fish_hook_path,
                fish_hook_contents().into_bytes(),
            ));
        } else {
            updates.push(FileUpdate::delete(fish_hook_path));
        }
        commit_file_updates(
            updates,
            Some(MAX_SHELL_PROFILE_BYTES),
            "Copilot CLI shell hook",
        )
    }
}

#[cfg(any(unix, test))]
impl UserEnvironment for UnixUserEnvironment {
    fn read(&self, name: &str) -> Result<Option<String>, AppError> {
        Ok(self.read_values()?.remove(name).flatten())
    }

    fn write(&self, name: &str, value: Option<&str>) -> Result<(), AppError> {
        let mut values = self.read_values()?;
        values.insert(name.to_string(), value.map(str::to_string));
        self.write_values(&values)
    }

    fn broadcast_change(
        &self,
        _before: &BTreeMap<String, Option<String>>,
        after: &BTreeMap<String, Option<String>>,
    ) -> Result<(), AppError> {
        self.update_shell_hooks(after.values().any(Option::is_some))
    }

    fn external_conflicts(
        &self,
        expected: &BTreeMap<String, Option<String>>,
    ) -> Result<Vec<String>, AppError> {
        self.validate_managed_artifacts(expected)
    }

    fn source(&self) -> String {
        self.env_path().to_string_lossy().to_string()
    }

    fn confirmation_context(&self) -> Result<ConfirmationContext, AppError> {
        let mut paths = self.profile_paths();
        paths.extend([self.env_path(), self.fish_env_path(), self.fish_hook_path()]);
        paths
            .into_iter()
            .map(|path| {
                let content = read_optional_regular_file(&path, "Copilot CLI shell integration")?;
                Ok((path, content))
            })
            .collect()
    }

    fn restore_confirmation_context(&self, context: &ConfirmationContext) -> Result<(), AppError> {
        let mut errors = Vec::new();
        for (path, content) in context {
            let result = match content {
                Some(content) if *path == self.env_path() || *path == self.fish_env_path() => {
                    crate::config::atomic_write_private(path, content.as_bytes())
                }
                Some(content) => crate::config::atomic_write(path, content.as_bytes()),
                None => remove_regular_file_if_exists(path, "Copilot CLI shell integration"),
            };
            if let Err(error) = result {
                errors.push(error.to_string());
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(AppError::Config(errors.join("; ")))
        }
    }

    fn broadcast_confirmed_change(
        &self,
        _before: &EnvironmentValues,
        after: &EnvironmentValues,
    ) -> Result<(), AppError> {
        self.update_shell_hooks_with_confirmation(after.values().any(Option::is_some), true)
    }
}

#[cfg(any(unix, test))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ManagedBlockState {
    Missing,
    Exact,
}

#[cfg(any(unix, test))]
fn posix_hook_block() -> String {
    format!(
        "{POSIX_BLOCK_START}\n[ -f \"$HOME/.cc-switch/copilot-cli-env.sh\" ] && . \"$HOME/.cc-switch/copilot-cli-env.sh\"\n{POSIX_BLOCK_END}"
    )
}

#[cfg(any(unix, test))]
fn fish_hook_contents() -> String {
    format!(
        "{FISH_HOOK_HEADER}\nif test -f \"$HOME/.cc-switch/copilot-cli-env.fish\"\n    source \"$HOME/.cc-switch/copilot-cli-env.fish\"\nend\n"
    )
}

#[cfg(any(unix, test))]
fn profile_contains_marker(path: &Path) -> bool {
    fs::read_to_string(path)
        .ok()
        .is_some_and(|content| content.contains(POSIX_BLOCK_START))
}

#[cfg(any(unix, test))]
fn managed_block_state(content: &str) -> Result<ManagedBlockState, AppError> {
    let expected = posix_hook_block();
    let starts = content.match_indices(POSIX_BLOCK_START).collect::<Vec<_>>();
    let ends = content.match_indices(POSIX_BLOCK_END).collect::<Vec<_>>();
    if starts.is_empty() && ends.is_empty() {
        return Ok(ManagedBlockState::Missing);
    }
    if starts.len() != 1 || ends.len() != 1 || starts[0].0 >= ends[0].0 {
        return Err(AppError::Conflict(
            "Copilot CLI shell profile contains a malformed CC Switch block".to_string(),
        ));
    }
    let end = ends[0].0 + POSIX_BLOCK_END.len();
    if content[starts[0].0..end] != expected {
        return Err(AppError::Conflict(
            "Copilot CLI shell profile block was edited outside CC Switch".to_string(),
        ));
    }
    Ok(ManagedBlockState::Exact)
}

#[cfg(any(unix, test))]
fn update_managed_block(content: &str, enabled: bool) -> Result<String, AppError> {
    let state = managed_block_state(content)?;
    let expected = posix_hook_block();
    let mut clean = content.to_string();
    if state == ManagedBlockState::Exact {
        let start = clean.find(POSIX_BLOCK_START).unwrap_or_default();
        let end = clean.find(POSIX_BLOCK_END).unwrap_or_default() + POSIX_BLOCK_END.len();
        clean.replace_range(start..end, "");
        clean = clean.trim_end_matches(['\r', '\n']).to_string();
    }
    if enabled {
        if !clean.is_empty() {
            clean.push_str("\n\n");
        }
        clean.push_str(&expected);
        clean.push('\n');
    } else if !clean.is_empty() {
        clean.push('\n');
    }
    Ok(clean)
}

#[cfg(any(unix, test))]
fn update_confirmed_managed_block(content: &str, enabled: bool) -> Result<String, AppError> {
    let starts = content.match_indices(POSIX_BLOCK_START).collect::<Vec<_>>();
    let ends = content.match_indices(POSIX_BLOCK_END).collect::<Vec<_>>();
    if starts.is_empty() && ends.is_empty() {
        return update_managed_block(content, enabled);
    }
    // Explicit consent permits replacing the identifiable CC Switch block,
    // never guessing a boundary or removing unrelated shell configuration.
    if starts.len() != 1 || ends.len() != 1 || starts[0].0 >= ends[0].0 {
        return Err(AppError::Conflict(
            "Cannot safely replace a malformed Copilot CLI shell profile block".to_string(),
        ));
    }
    let mut updated = content.to_string();
    let end = ends[0].0 + POSIX_BLOCK_END.len();
    updated.replace_range(
        starts[0].0..end,
        &if enabled {
            posix_hook_block()
        } else {
            String::new()
        },
    );
    Ok(updated)
}

#[cfg(any(unix, test))]
fn render_posix_env(values: &BTreeMap<String, Option<String>>) -> Result<String, AppError> {
    let mut output = String::from("# Managed by CC Switch for GitHub Copilot CLI\n");
    for (name, value) in values {
        if let Some(value) = value {
            if value.as_bytes().contains(&0) {
                return Err(AppError::InvalidInput(format!(
                    "Copilot CLI environment value contains NUL: {name}"
                )));
            }
            let encoded = value
                .as_bytes()
                .iter()
                .map(|byte| format!("\\{byte:03o}"))
                .collect::<String>();
            output.push_str(&format!("export {name}=$(printf '%b' '{encoded}')\n"));
        }
    }
    Ok(output)
}

#[cfg(any(unix, test))]
fn render_fish_env(values: &BTreeMap<String, Option<String>>) -> Result<String, AppError> {
    let mut output = String::from("# Managed by CC Switch for GitHub Copilot CLI\n");
    for (name, value) in values {
        if let Some(value) = value {
            if value.as_bytes().contains(&0) {
                return Err(AppError::InvalidInput(format!(
                    "Copilot CLI environment value contains NUL: {name}"
                )));
            }
            let escaped = value.replace('\\', "\\\\").replace('\'', "\\'");
            output.push_str(&format!("set -gx {name} '{escaped}'\n"));
        }
    }
    Ok(output)
}

#[cfg(any(unix, test))]
fn read_posix_env_file(path: &Path) -> Result<BTreeMap<String, Option<String>>, AppError> {
    let mut values: BTreeMap<String, Option<String>> = MANAGED_VARIABLES
        .iter()
        .map(|name| ((*name).to_string(), None))
        .collect();
    let Some(content) = read_optional_regular_file(path, "Copilot CLI environment")? else {
        return Ok(values);
    };
    for line in content.lines() {
        let Some(rest) = line.strip_prefix("export ") else {
            continue;
        };
        let Some((name, encoded)) = rest.split_once("=$(printf '%b' '") else {
            continue;
        };
        let Some(encoded) = encoded.strip_suffix("')") else {
            continue;
        };
        if !MANAGED_VARIABLES.contains(&name) {
            continue;
        }
        let bytes = encoded.as_bytes();
        if bytes.len() % 4 != 0 {
            return Err(AppError::Config(format!(
                "Invalid managed Copilot CLI environment encoding for {name}"
            )));
        }
        let mut decoded = Vec::with_capacity(bytes.len() / 4);
        for chunk in bytes.chunks_exact(4) {
            if chunk[0] != b'\\' {
                return Err(AppError::Config(format!(
                    "Invalid managed Copilot CLI environment encoding for {name}"
                )));
            }
            let digits = std::str::from_utf8(&chunk[1..]).map_err(|error| {
                AppError::Config(format!("Invalid Copilot CLI environment encoding: {error}"))
            })?;
            decoded.push(u8::from_str_radix(digits, 8).map_err(|error| {
                AppError::Config(format!("Invalid Copilot CLI environment encoding: {error}"))
            })?);
        }
        let decoded = String::from_utf8(decoded).map_err(|error| {
            AppError::Config(format!("Invalid UTF-8 in Copilot CLI environment: {error}"))
        })?;
        values.insert(name.to_string(), Some(decoded));
    }
    Ok(values)
}

#[cfg(any(unix, test))]
fn read_optional_regular_file(path: &Path, label: &str) -> Result<Option<String>, AppError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(AppError::io(path, error)),
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(AppError::InvalidInput(format!(
            "{label} is not a regular file: {}",
            path.display()
        )));
    }
    if metadata.len() > MAX_SHELL_PROFILE_BYTES {
        return Err(AppError::InvalidInput(format!(
            "{label} exceeds {} MiB: {}",
            MAX_SHELL_PROFILE_BYTES / 1024 / 1024,
            path.display()
        )));
    }
    fs::read_to_string(path)
        .map(Some)
        .map_err(|error| AppError::io(path, error))
}

#[cfg(any(unix, test))]
fn remove_regular_file_if_exists(path: &Path, label: &str) -> Result<(), AppError> {
    if read_optional_regular_file(path, label)?.is_some() {
        fs::remove_file(path).map_err(|error| AppError::io(path, error))?;
    }
    Ok(())
}

fn selected<'a>(
    groups: &'a [CopilotByokGroup],
    group_id: &str,
    model_id: &str,
) -> Result<(&'a CopilotByokGroup, &'a CopilotByokModel), AppError> {
    let group = groups
        .iter()
        .find(|group| group.id == group_id && group.enabled)
        .ok_or_else(|| {
            AppError::InvalidInput(format!(
                "Unknown or disabled Copilot CLI provider: {group_id}"
            ))
        })?;
    let model = group
        .models
        .iter()
        .find(|model| model.id == model_id && model.enabled)
        .ok_or_else(|| {
            AppError::InvalidInput(format!(
                "Unknown or disabled Copilot CLI model {model_id} in provider {group_id}"
            ))
        })?;
    Ok((group, model))
}

fn is_vscode_secret_reference(value: &str) -> bool {
    value
        .strip_prefix("${input:")
        .and_then(|value| value.strip_suffix('}'))
        .is_some_and(|key| !key.trim().is_empty())
}

fn extra_string<'a>(extra: &'a BTreeMap<String, serde_json::Value>, key: &str) -> Option<&'a str> {
    extra
        .get(key)
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn cli_provider_type(group: &CopilotByokGroup) -> Result<String, AppError> {
    let provider_type = extra_string(&group.extra, CLI_PROVIDER_TYPE_KEY)
        .unwrap_or(if group.api_type == "messages" {
            "anthropic"
        } else {
            "openai"
        })
        .to_ascii_lowercase();
    if !matches!(provider_type.as_str(), "openai" | "azure" | "anthropic") {
        return Err(AppError::InvalidInput(format!(
            "Unsupported Copilot CLI provider type: {provider_type}"
        )));
    }
    if group.api_type == "messages" && provider_type != "anthropic" {
        return Err(AppError::InvalidInput(
            "Copilot CLI Messages API providers must use the anthropic provider type".to_string(),
        ));
    }
    Ok(provider_type)
}

fn provider_base_url(raw: &str, api_type: &str, provider_type: &str) -> Result<String, AppError> {
    let mut parsed = url::Url::parse(raw).map_err(|error| {
        AppError::InvalidInput(format!("Invalid Copilot CLI provider URL: {error}"))
    })?;
    if parsed.query().is_some() || parsed.fragment().is_some() {
        return Err(AppError::InvalidInput(
            "Copilot CLI provider base URL must not contain a query or fragment".to_string(),
        ));
    }

    if provider_type == "azure" {
        // Copilot CLI's Azure provider constructs deployment routes itself and
        // expects only the resource host.
        parsed.set_path("/");
    } else {
        let path = parsed.path().trim_end_matches('/').to_string();
        let suffixes: &[&str] = match api_type {
            "chat-completions" => &["/chat/completions"],
            "responses" => &["/responses"],
            "messages" => &["/v1/messages", "/messages"],
            _ => &[],
        };
        if let Some(suffix) = suffixes.iter().find(|suffix| path.ends_with(**suffix)) {
            let base_path = &path[..path.len() - suffix.len()];
            parsed.set_path(if base_path.is_empty() { "/" } else { base_path });
        }
    }

    let mut rendered = parsed.to_string();
    if parsed.path() == "/" {
        rendered = rendered.trim_end_matches('/').to_string();
    }
    Ok(rendered)
}

fn format_headers(group: &CopilotByokGroup) -> Result<Option<String>, AppError> {
    if group.request_headers.is_empty() {
        return Ok(None);
    }
    let mut lines = Vec::with_capacity(group.request_headers.len());
    for (raw_name, raw_value) in &group.request_headers {
        let name = raw_name.trim();
        if name.is_empty() || name.contains([':', '\r', '\n']) || raw_value.contains(['\r', '\n']) {
            return Err(AppError::InvalidInput(format!(
                "Copilot CLI provider header is not representable: {raw_name}"
            )));
        }
        lines.push(format!(
            "{name}: {}",
            raw_value.replace("${apiKey}", &group.api_key)
        ));
    }
    Ok(Some(lines.join("\n")))
}

fn desired_environment(
    group: &CopilotByokGroup,
    model: &CopilotByokModel,
) -> Result<BTreeMap<String, Option<String>>, AppError> {
    let bearer_token = extra_string(&group.extra, CLI_BEARER_TOKEN_KEY);
    if is_vscode_secret_reference(&group.api_key)
        || bearer_token.is_some_and(is_vscode_secret_reference)
    {
        return Err(AppError::InvalidInput(
            "Copilot CLI cannot resolve a VS Code SecretStorage ${input:...} reference; enter a literal API key or use an unauthenticated local provider"
                .to_string(),
        ));
    }
    let mut desired: BTreeMap<String, Option<String>> = MANAGED_VARIABLES
        .iter()
        .map(|name| ((*name).to_string(), None))
        .collect();
    let provider_type = cli_provider_type(group)?;
    desired.insert(
        "COPILOT_PROVIDER_BASE_URL".to_string(),
        Some(provider_base_url(
            &group.url,
            &group.api_type,
            &provider_type,
        )?),
    );
    desired.insert(
        "COPILOT_PROVIDER_TYPE".to_string(),
        Some(provider_type.clone()),
    );
    if !group.api_key.is_empty() {
        desired.insert(
            "COPILOT_PROVIDER_API_KEY".to_string(),
            Some(group.api_key.clone()),
        );
    }
    desired.insert(
        "COPILOT_PROVIDER_BEARER_TOKEN".to_string(),
        bearer_token.map(str::to_string),
    );
    desired.insert(
        "COPILOT_PROVIDER_WIRE_API".to_string(),
        match (provider_type.as_str(), group.api_type.as_str()) {
            ("anthropic", _) => None,
            (_, "responses") => Some("responses".to_string()),
            (_, "chat-completions") => Some("completions".to_string()),
            _ => None,
        },
    );
    let transport = extra_string(&group.extra, CLI_TRANSPORT_KEY)
        .unwrap_or("http")
        .to_ascii_lowercase();
    if !matches!(transport.as_str(), "http" | "websockets") {
        return Err(AppError::InvalidInput(format!(
            "Unsupported Copilot CLI provider transport: {transport}"
        )));
    }
    if transport == "websockets" && group.api_type != "responses" {
        return Err(AppError::InvalidInput(
            "Copilot CLI WebSocket transport requires the Responses API".to_string(),
        ));
    }
    desired.insert(
        "COPILOT_PROVIDER_TRANSPORT".to_string(),
        (transport != "http").then_some(transport),
    );
    desired.insert(
        "COPILOT_PROVIDER_AZURE_API_VERSION".to_string(),
        (provider_type == "azure")
            .then(|| extra_string(&group.extra, CLI_AZURE_API_VERSION_KEY))
            .flatten()
            .map(str::to_string),
    );
    desired.insert(
        "COPILOT_PROVIDER_HEADERS".to_string(),
        format_headers(group)?,
    );
    let model_id = extra_string(&model.extra, CLI_MODEL_ID_KEY).unwrap_or(&model.model_id);
    let copilot_model = extra_string(&model.extra, CLI_MODEL_KEY).unwrap_or(model_id);
    let wire_model = extra_string(&model.extra, CLI_WIRE_MODEL_KEY).unwrap_or(&model.model_id);
    desired.insert("COPILOT_MODEL".to_string(), Some(copilot_model.to_string()));
    desired.insert(
        "COPILOT_PROVIDER_MODEL_ID".to_string(),
        Some(model_id.to_string()),
    );
    desired.insert(
        "COPILOT_PROVIDER_WIRE_MODEL".to_string(),
        Some(wire_model.to_string()),
    );
    desired.insert(
        "COPILOT_PROVIDER_MAX_PROMPT_TOKENS".to_string(),
        model.max_input_tokens.map(|value| value.to_string()),
    );
    desired.insert(
        "COPILOT_PROVIDER_MAX_OUTPUT_TOKENS".to_string(),
        model.max_output_tokens.map(|value| value.to_string()),
    );
    Ok(desired)
}

fn snapshot(
    environment: &dyn UserEnvironment,
) -> Result<BTreeMap<String, Option<String>>, AppError> {
    MANAGED_VARIABLES
        .iter()
        .map(|name| Ok(((*name).to_string(), environment.read(name)?)))
        .collect()
}

pub(super) fn official_environment() -> BTreeMap<String, Option<String>> {
    MANAGED_VARIABLES
        .iter()
        .map(|name| ((*name).to_string(), None))
        .collect()
}

pub(super) fn current_environment() -> Result<BTreeMap<String, Option<String>>, AppError> {
    #[cfg(windows)]
    {
        snapshot(&WindowsUserEnvironment)
    }
    #[cfg(unix)]
    {
        snapshot(&UnixUserEnvironment::new()?)
    }
}

fn environment_value<'a>(
    values: &'a BTreeMap<String, Option<String>>,
    name: &str,
) -> Option<&'a str> {
    values
        .get(name)
        .and_then(Option::as_deref)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn environment_token_limit(
    values: &BTreeMap<String, Option<String>>,
    name: &str,
) -> Result<Option<u64>, AppError> {
    environment_value(values, name)
        .map(|value| {
            value.parse::<u64>().map_err(|error| {
                AppError::InvalidInput(format!(
                    "Cannot import existing Copilot CLI environment variable {name}: {error}"
                ))
            })
        })
        .transpose()
}

fn environment_headers(
    values: &BTreeMap<String, Option<String>>,
) -> Result<BTreeMap<String, String>, AppError> {
    let Some(raw) = environment_value(values, "COPILOT_PROVIDER_HEADERS") else {
        return Ok(BTreeMap::new());
    };
    raw.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let (name, value) = line.split_once(':').ok_or_else(|| {
                AppError::InvalidInput(
                    "Cannot import COPILOT_PROVIDER_HEADERS: each line must contain a colon"
                        .to_string(),
                )
            })?;
            Ok((name.trim().to_string(), value.trim().to_string()))
        })
        .collect()
}

/// Convert an environment that predates CC Switch management into the same
/// one-provider/one-model shape used by the first-class Copilot CLI catalog.
/// The stable id makes an interrupted migration safe to retry without creating
/// duplicate secret-bearing provider rows.
pub(super) fn imported_group_from_environment(
    values: &BTreeMap<String, Option<String>>,
    name: &str,
) -> Result<Option<CopilotByokGroup>, AppError> {
    if !values.values().any(Option::is_some) {
        return Ok(None);
    }
    let url = environment_value(values, "COPILOT_PROVIDER_BASE_URL").ok_or_else(|| {
        AppError::InvalidInput(
            "Cannot import the existing Copilot CLI environment without COPILOT_PROVIDER_BASE_URL"
                .to_string(),
        )
    })?;
    let copilot_model = environment_value(values, "COPILOT_MODEL")
        .or_else(|| environment_value(values, "COPILOT_PROVIDER_MODEL_ID"))
        .or_else(|| environment_value(values, "COPILOT_PROVIDER_WIRE_MODEL"))
        .ok_or_else(|| {
            AppError::InvalidInput(
                "Cannot import the existing Copilot CLI environment without a model id".to_string(),
            )
        })?;
    let provider_type = environment_value(values, "COPILOT_PROVIDER_TYPE")
        .unwrap_or("openai")
        .to_ascii_lowercase();
    let api_type = if provider_type == "anthropic" {
        "messages"
    } else {
        match environment_value(values, "COPILOT_PROVIDER_WIRE_API")
            .map(str::to_ascii_lowercase)
            .as_deref()
        {
            Some("responses") => "responses",
            Some("completions" | "chat-completions") | None => "chat-completions",
            Some(value) => {
                return Err(AppError::InvalidInput(format!(
                    "Cannot import unsupported COPILOT_PROVIDER_WIRE_API value: {value}"
                )))
            }
        }
    };
    let provider_model_id =
        environment_value(values, "COPILOT_PROVIDER_MODEL_ID").unwrap_or(copilot_model);
    let wire_model =
        environment_value(values, "COPILOT_PROVIDER_WIRE_MODEL").unwrap_or(provider_model_id);

    let mut digest = Sha256::new();
    for variable in MANAGED_VARIABLES {
        digest.update(variable.as_bytes());
        digest.update([0]);
        if let Some(value) = values.get(*variable).and_then(Option::as_deref) {
            digest.update(value.as_bytes());
        }
        digest.update([0]);
    }
    let encoded = format!("{:x}", digest.finalize());
    let group_id = format!("copilot-cli-imported-{}", &encoded[..24]);
    let model_id = format!("{group_id}:model");

    let mut group_extra = BTreeMap::from([(
        CLI_PROVIDER_TYPE_KEY.to_string(),
        serde_json::Value::String(provider_type.clone()),
    )]);
    for (environment_name, extra_name) in [
        ("COPILOT_PROVIDER_BEARER_TOKEN", CLI_BEARER_TOKEN_KEY),
        ("COPILOT_PROVIDER_TRANSPORT", CLI_TRANSPORT_KEY),
        (
            "COPILOT_PROVIDER_AZURE_API_VERSION",
            CLI_AZURE_API_VERSION_KEY,
        ),
    ] {
        if let Some(value) = environment_value(values, environment_name) {
            group_extra.insert(
                extra_name.to_string(),
                serde_json::Value::String(value.to_string()),
            );
        }
    }
    let model_extra = BTreeMap::from([
        (
            CLI_MODEL_KEY.to_string(),
            serde_json::Value::String(copilot_model.to_string()),
        ),
        (
            CLI_MODEL_ID_KEY.to_string(),
            serde_json::Value::String(provider_model_id.to_string()),
        ),
        (
            CLI_WIRE_MODEL_KEY.to_string(),
            serde_json::Value::String(wire_model.to_string()),
        ),
    ]);
    let mut group = CopilotByokGroup {
        id: group_id,
        name: name.to_string(),
        url: url.to_string(),
        api_key: environment_value(values, "COPILOT_PROVIDER_API_KEY")
            .unwrap_or_default()
            .to_string(),
        api_type: api_type.to_string(),
        website_url: None,
        notes: Some("Imported from the existing Copilot CLI environment".to_string()),
        icon: None,
        icon_color: None,
        category: None,
        usage_script: None,
        enabled: true,
        request_headers: environment_headers(values)?,
        models: vec![CopilotByokModel {
            id: model_id,
            model_id: provider_model_id.to_string(),
            name: copilot_model.to_string(),
            enabled: true,
            tool_calling: None,
            vision: None,
            thinking: None,
            streaming: None,
            context_window: None,
            max_input_tokens: environment_token_limit(
                values,
                "COPILOT_PROVIDER_MAX_PROMPT_TOKENS",
            )?,
            max_output_tokens: environment_token_limit(
                values,
                "COPILOT_PROVIDER_MAX_OUTPUT_TOKENS",
            )?,
            edit_tools: Vec::new(),
            zero_data_retention_enabled: false,
            supports_reasoning_effort: Vec::new(),
            reasoning_effort_format: None,
            model_options: serde_json::json!({}),
            extra: model_extra,
        }],
        extra: group_extra,
    };
    group.normalize();
    group.validate()?;
    desired_environment(&group, &group.models[0])?;
    Ok(Some(group))
}

pub(super) fn launch_environment(
    group: Option<&CopilotByokGroup>,
) -> Result<BTreeMap<String, Option<String>>, AppError> {
    let Some(group) = group else {
        return Ok(official_environment());
    };
    if !group.enabled {
        return Err(AppError::InvalidInput(format!(
            "Disabled Copilot CLI provider cannot be launched: {}",
            group.id
        )));
    }
    let model = group.models.first().ok_or_else(|| {
        AppError::Config(format!(
            "Copilot CLI provider {} has no default model",
            group.id
        ))
    })?;
    if group.models.len() != 1 || !model.enabled {
        return Err(AppError::Config(format!(
            "Copilot CLI provider {} must have exactly one enabled default model",
            group.id
        )));
    }
    desired_environment(group, model)
}

fn conflicts(
    current: &BTreeMap<String, Option<String>>,
    expected: &BTreeMap<String, Option<String>>,
) -> Vec<String> {
    MANAGED_VARIABLES
        .iter()
        .filter(|name| current.get(**name) != expected.get(**name))
        .map(|name| (*name).to_string())
        .collect()
}

fn preview_value(value: Option<&String>, sensitive: bool) -> CopilotCliEnvironmentValue {
    CopilotCliEnvironmentValue {
        is_set: value.is_some(),
        value: if sensitive { None } else { value.cloned() },
    }
}

fn preview_with_backend(
    store: &CopilotByokStore,
    group: Option<&CopilotByokGroup>,
    environment: &dyn UserEnvironment,
) -> Result<CopilotCliEnvironmentPreview, AppError> {
    let desired = launch_environment(group)?;
    let current = snapshot(environment)?;
    let official = official_environment();
    let expected = if store.cli.enabled {
        &store.cli.managed_environment.last_written
    } else {
        &official
    };
    let source = environment.source();
    let changes = MANAGED_VARIABLES
        .iter()
        .filter_map(|name| {
            let previous = expected.get(*name).and_then(Option::as_ref);
            let actual = current.get(*name).and_then(Option::as_ref);
            let target = desired.get(*name).and_then(Option::as_ref);
            if actual == previous && actual == target {
                return None;
            }
            // URLs can also embed credentials or access tokens in their query.
            let sensitive = matches!(
                *name,
                "COPILOT_PROVIDER_API_KEY"
                    | "COPILOT_PROVIDER_BEARER_TOKEN"
                    | "COPILOT_PROVIDER_HEADERS"
                    | "COPILOT_PROVIDER_BASE_URL"
            );
            Some(CopilotCliEnvironmentChange {
                name: (*name).to_string(),
                source: source.clone(),
                sensitive,
                conflict: actual != previous,
                previous: preview_value(previous, sensitive),
                current: preview_value(actual, sensitive),
                desired: preview_value(target, sensitive),
            })
        })
        .collect();
    let integration_conflicts = environment.external_conflicts(expected)?;
    let context = environment.confirmation_context()?;
    let group_id = group.map_or(super::COPILOT_CLI_OFFICIAL_PROVIDER_ID, |group| &group.id);
    let token = uuid::Uuid::new_v4().to_string();
    let mut pending = PENDING_CONFIRMATIONS
        .lock()
        .map_err(|_| AppError::Config("Copilot CLI confirmation lock is poisoned".to_string()))?;
    pending.retain(|entry| entry.created.elapsed() < CONFIRMATION_TTL);
    while pending.len() >= 8 {
        pending.pop_front();
    }
    pending.push_back(PendingConfirmation {
        token: token.clone(),
        created: Instant::now(),
        group_id: group_id.to_string(),
        current,
        desired,
        context,
    });
    Ok(CopilotCliEnvironmentPreview {
        group_id: group_id.to_string(),
        changes,
        integration_conflicts,
        confirmation_token: token,
    })
}

fn consume_confirmation(
    token: &str,
    group_id: &str,
    current: &EnvironmentValues,
    desired: &EnvironmentValues,
    environment: &dyn UserEnvironment,
) -> Result<ConfirmationContext, AppError> {
    let stale = || {
        AppError::Conflict(
            "Copilot CLI confirmation is stale; review the environment differences again"
                .to_string(),
        )
    };
    let context = environment.confirmation_context()?;
    let mut pending = PENDING_CONFIRMATIONS
        .lock()
        .map_err(|_| AppError::Config("Copilot CLI confirmation lock is poisoned".to_string()))?;
    pending.retain(|entry| entry.created.elapsed() < CONFIRMATION_TTL);
    let index = pending
        .iter()
        .position(|entry| entry.token == token)
        .ok_or_else(stale)?;
    let entry = pending.remove(index).ok_or_else(stale)?;
    if entry.group_id != group_id
        || entry.current != *current
        || entry.desired != *desired
        || entry.context != context
    {
        return Err(stale());
    }
    Ok(context)
}

fn restore_before_apply(
    environment: &dyn UserEnvironment,
    before: &EnvironmentValues,
    confirmation: Option<&ConfirmationContext>,
) -> Result<(), AppError> {
    match confirmation.filter(|context| !context.is_empty()) {
        Some(context) => environment.restore_confirmation_context(context),
        None => restore_snapshot(environment, before),
    }
}

fn write_after_confirmation(
    environment: &dyn UserEnvironment,
    current: &EnvironmentValues,
    desired: &EnvironmentValues,
    confirmation: Option<&ConfirmationContext>,
) -> Result<EnvironmentValues, AppError> {
    let before = snapshot(environment)?;
    let context_changed = match confirmation {
        Some(context) => environment.confirmation_context()? != *context,
        None => false,
    };
    if before != *current || context_changed {
        // Nothing was written: in particular, do not "roll back" a newer
        // external edit to the older snapshot the user inspected.
        return Err(AppError::Conflict(
            "Copilot CLI confirmation is stale; review the environment differences again"
                .to_string(),
        ));
    }
    for name in MANAGED_VARIABLES {
        let value = desired.get(*name).and_then(Option::as_deref);
        if let Err(error) = environment.write(name, value) {
            return match restore_before_apply(environment, &before, confirmation) {
                Ok(()) => Err(error),
                Err(rollback) => Err(AppError::Config(format!(
                    "{error}; failed to roll back Copilot CLI environment: {rollback}"
                ))),
            };
        }
    }
    Ok(before)
}

pub(super) fn preview(
    store: &CopilotByokStore,
    group: Option<&CopilotByokGroup>,
) -> Result<CopilotCliEnvironmentPreview, AppError> {
    #[cfg(windows)]
    {
        preview_with_backend(store, group, &WindowsUserEnvironment)
    }
    #[cfg(unix)]
    {
        preview_with_backend(store, group, &UnixUserEnvironment::new()?)
    }
}

fn restore_snapshot(
    environment: &dyn UserEnvironment,
    values: &BTreeMap<String, Option<String>>,
) -> Result<(), AppError> {
    let failures: Vec<String> = MANAGED_VARIABLES
        .iter()
        .filter_map(|name| {
            environment
                .write(name, values.get(*name).and_then(Option::as_deref))
                .err()
                .map(|_| (*name).to_string())
        })
        .collect();
    if !failures.is_empty() {
        return Err(AppError::Config(format!(
            "Failed to restore environment variables: {}",
            failures.join(", ")
        )));
    }
    Ok(())
}

fn state_with_backend(
    store: &CopilotByokStore,
    groups: &[CopilotByokGroup],
    environment: &dyn UserEnvironment,
) -> Result<CopilotCliState, AppError> {
    let selected = match (
        store.cli.selected_group_id.as_deref(),
        store.cli.selected_model_id.as_deref(),
    ) {
        (Some(group_id), Some(model_id)) => selected(groups, group_id, model_id).ok(),
        _ => None,
    };
    let current = snapshot(environment)?;
    let official = official_environment();
    let expected = if store.cli.enabled {
        &store.cli.managed_environment.last_written
    } else {
        &official
    };
    let mut environment_conflicts = conflicts(&current, expected);
    environment_conflicts.extend(environment.external_conflicts(expected)?);
    environment_conflicts.sort();
    environment_conflicts.dedup();
    let desired_matches_last_written = selected
        .and_then(|(group, model)| desired_environment(group, model).ok())
        .is_some_and(|desired| desired == store.cli.managed_environment.last_written);
    let official_matches =
        !store.cli.enabled && current == official && environment_conflicts.is_empty();
    Ok(CopilotCliState {
        supported: true,
        enabled: store.cli.enabled,
        selected_group_id: store.cli.selected_group_id.clone(),
        selected_model_id: store.cli.selected_model_id.clone(),
        selected_provider_name: selected.map(|(group, _)| group.name.clone()),
        selected_model_name: selected.map(|(_, model)| model.name.clone()),
        environment_matches: if store.cli.enabled {
            selected.is_some() && desired_matches_last_written && environment_conflicts.is_empty()
        } else {
            official_matches
        },
        environment_conflicts,
        official_activation_requires_confirmation: !store.cli.enabled && !official_matches,
    })
}

fn ensure_no_external_edits(
    store: &CopilotByokStore,
    current: &BTreeMap<String, Option<String>>,
    environment: &dyn UserEnvironment,
) -> Result<(), AppError> {
    if !store.cli.enabled && !store.cli.official_override_active {
        return Ok(());
    }
    let official = official_environment();
    let expected = if store.cli.enabled {
        &store.cli.managed_environment.last_written
    } else {
        &official
    };
    let mut changed = conflicts(current, expected);
    changed.extend(environment.external_conflicts(expected)?);
    changed.sort();
    changed.dedup();
    if changed.is_empty() {
        return Ok(());
    }
    Err(AppError::Conflict(format!(
        "Copilot CLI environment differs from the last applied configuration: {}",
        changed.join(", ")
    )))
}

#[cfg(test)]
fn apply_with_backend<F>(
    store: &mut CopilotByokStore,
    groups: &[CopilotByokGroup],
    group_id: &str,
    model_id: &str,
    environment: &dyn UserEnvironment,
    persist: F,
) -> Result<CopilotCliState, AppError>
where
    F: Fn(&CopilotByokStore) -> Result<(), AppError>,
{
    apply_confirmed_with_backend(
        store,
        groups,
        group_id,
        model_id,
        environment,
        None,
        persist,
    )
}

fn apply_confirmed_with_backend<F>(
    store: &mut CopilotByokStore,
    groups: &[CopilotByokGroup],
    group_id: &str,
    model_id: &str,
    environment: &dyn UserEnvironment,
    confirmation_token: Option<&str>,
    persist: F,
) -> Result<CopilotCliState, AppError>
where
    F: Fn(&CopilotByokStore) -> Result<(), AppError>,
{
    let (group, model) = selected(groups, group_id, model_id)?;
    let desired = desired_environment(group, model)?;
    let current = snapshot(environment)?;
    let confirmation = if let Some(token) = confirmation_token {
        Some(consume_confirmation(
            token,
            group_id,
            &current,
            &desired,
            environment,
        )?)
    } else {
        ensure_no_external_edits(store, &current, environment)?;
        None
    };
    let original = if store.cli.enabled || store.cli.official_override_active {
        store.cli.managed_environment.original.clone()
    } else {
        current.clone()
    };

    let before = write_after_confirmation(environment, &current, &desired, confirmation.as_ref())?;
    let previous = store.cli.clone();
    store.cli = CopilotCliConfig {
        enabled: true,
        official_override_active: false,
        selected_group_id: Some(group_id.to_string()),
        selected_model_id: Some(model_id.to_string()),
        managed_environment: CopilotCliManagedEnvironment {
            original,
            last_written: desired.clone(),
        },
    };
    if let Err(error) = persist(store) {
        store.cli = previous;
        restore_before_apply(environment, &before, confirmation.as_ref()).map_err(
            |rollback_error| {
                AppError::Config(format!(
                    "{error}; failed to roll back Copilot CLI environment: {rollback_error}"
                ))
            },
        )?;
        return Err(error);
    }
    let broadcast = if confirmation.is_some() {
        environment.broadcast_confirmed_change(&before, &desired)
    } else {
        environment.broadcast_change(&before, &desired)
    };
    if let Err(error) = broadcast {
        store.cli = previous;
        let persist_error = persist(store).err();
        let restore_error = restore_before_apply(environment, &before, confirmation.as_ref()).err();
        let rebroadcast_error =
            if restore_error.is_none() && confirmation.as_ref().is_none_or(BTreeMap::is_empty) {
                environment.broadcast_change(&desired, &before).err()
            } else {
                None
            };
        let mut details = vec![error.to_string()];
        if let Some(error) = persist_error {
            details.push(format!("failed to roll back selection: {error}"));
        }
        if let Some(error) = restore_error {
            details.push(format!("failed to roll back environment: {error}"));
        }
        if let Some(error) = rebroadcast_error {
            details.push(format!(
                "failed to broadcast the restored environment: {error}"
            ));
        }
        return Err(AppError::Config(format!(
            "Failed to activate Copilot CLI shell integration: {}",
            details.join("; ")
        )));
    }
    state_with_backend(store, groups, environment)
}

fn disable_with_backend<F>(
    store: &mut CopilotByokStore,
    groups: &[CopilotByokGroup],
    environment: &dyn UserEnvironment,
    persist: F,
) -> Result<CopilotCliState, AppError>
where
    F: Fn(&CopilotByokStore) -> Result<(), AppError>,
{
    // Backward-compatible command behavior: disabling now means selecting the
    // movable Official provider. Previous environments are catalog entries,
    // not an out-of-band restore state.
    use_official_with_backend(store, groups, environment, true, persist)
}

fn use_official_with_backend<F>(
    store: &mut CopilotByokStore,
    groups: &[CopilotByokGroup],
    environment: &dyn UserEnvironment,
    confirm_unmanaged_clear: bool,
    persist: F,
) -> Result<CopilotCliState, AppError>
where
    F: Fn(&CopilotByokStore) -> Result<(), AppError>,
{
    use_official_confirmed_with_backend(
        store,
        groups,
        environment,
        confirm_unmanaged_clear,
        None,
        persist,
    )
}

fn use_official_confirmed_with_backend<F>(
    store: &mut CopilotByokStore,
    groups: &[CopilotByokGroup],
    environment: &dyn UserEnvironment,
    confirm_unmanaged_clear: bool,
    confirmation_token: Option<&str>,
    persist: F,
) -> Result<CopilotCliState, AppError>
where
    F: Fn(&CopilotByokStore) -> Result<(), AppError>,
{
    let current = snapshot(environment)?;
    let desired = official_environment();
    let was_managed = store.cli.enabled || store.cli.official_override_active;
    let confirmation = if let Some(token) = confirmation_token {
        Some(consume_confirmation(
            token,
            super::COPILOT_CLI_OFFICIAL_PROVIDER_ID,
            &current,
            &desired,
            environment,
        )?)
    } else {
        None
    };
    if was_managed && confirmation.is_none() {
        ensure_no_external_edits(store, &current, environment)?;
    } else if !was_managed && confirmation.is_none() {
        let mut unmanaged = conflicts(&current, &desired);
        unmanaged.extend(environment.external_conflicts(&desired)?);
        unmanaged.sort();
        unmanaged.dedup();
        if !unmanaged.is_empty() && !confirm_unmanaged_clear {
            return Err(AppError::Conflict(format!(
                "Activating GitHub Copilot Official requires confirmation before clearing unmanaged overrides: {}",
                unmanaged.join(", ")
            )));
        }
    }
    let before = write_after_confirmation(environment, &current, &desired, confirmation.as_ref())?;
    let previous = store.cli.clone();
    store.cli = CopilotCliConfig::default();
    if let Err(error) = persist(store) {
        store.cli = previous;
        restore_before_apply(environment, &before, confirmation.as_ref()).map_err(
            |rollback_error| {
                AppError::Config(format!(
                "{error}; failed to roll back GitHub Copilot official environment: {rollback_error}"
            ))
            },
        )?;
        return Err(error);
    }
    let broadcast = if confirmation.is_some() {
        environment.broadcast_confirmed_change(&before, &desired)
    } else {
        environment.broadcast_change(&before, &desired)
    };
    if let Err(error) = broadcast {
        store.cli = previous;
        let persist_error = persist(store).err();
        let restore_error = restore_before_apply(environment, &before, confirmation.as_ref()).err();
        let rebroadcast_error =
            if restore_error.is_none() && confirmation.as_ref().is_none_or(BTreeMap::is_empty) {
                environment.broadcast_change(&desired, &before).err()
            } else {
                None
            };
        let mut details = vec![error.to_string()];
        if let Some(error) = persist_error {
            details.push(format!("failed to restore selection: {error}"));
        }
        if let Some(error) = restore_error {
            details.push(format!("failed to restore environment: {error}"));
        }
        if let Some(error) = rebroadcast_error {
            details.push(format!(
                "failed to broadcast the restored environment: {error}"
            ));
        }
        return Err(AppError::Config(format!(
            "Failed to activate GitHub Copilot official provider: {}",
            details.join("; ")
        )));
    }
    state_with_backend(store, groups, environment)
}

pub(super) fn get_state(
    store: &CopilotByokStore,
    groups: &[CopilotByokGroup],
) -> Result<CopilotCliState, AppError> {
    #[cfg(windows)]
    {
        state_with_backend(store, groups, &WindowsUserEnvironment)
    }
    #[cfg(unix)]
    {
        state_with_backend(store, groups, &UnixUserEnvironment::new()?)
    }
}

pub(super) fn apply(
    store: &mut CopilotByokStore,
    groups: &[CopilotByokGroup],
    group_id: &str,
    model_id: &str,
    confirmation_token: Option<&str>,
) -> Result<CopilotCliState, AppError> {
    #[cfg(windows)]
    {
        apply_confirmed_with_backend(
            store,
            groups,
            group_id,
            model_id,
            &WindowsUserEnvironment,
            confirmation_token,
            store::save_device_store,
        )
    }
    #[cfg(unix)]
    {
        apply_confirmed_with_backend(
            store,
            groups,
            group_id,
            model_id,
            &UnixUserEnvironment::new()?,
            confirmation_token,
            store::save_device_store,
        )
    }
}

pub(super) fn disable(
    store: &mut CopilotByokStore,
    groups: &[CopilotByokGroup],
) -> Result<CopilotCliState, AppError> {
    #[cfg(windows)]
    {
        disable_with_backend(
            store,
            groups,
            &WindowsUserEnvironment,
            store::save_device_store,
        )
    }
    #[cfg(unix)]
    {
        disable_with_backend(
            store,
            groups,
            &UnixUserEnvironment::new()?,
            store::save_device_store,
        )
    }
}

pub(super) fn use_official(
    store: &mut CopilotByokStore,
    groups: &[CopilotByokGroup],
    confirm_unmanaged_clear: bool,
    confirmation_token: Option<&str>,
) -> Result<CopilotCliState, AppError> {
    #[cfg(windows)]
    {
        use_official_confirmed_with_backend(
            store,
            groups,
            &WindowsUserEnvironment,
            confirm_unmanaged_clear,
            confirmation_token,
            store::save_device_store,
        )
    }
    #[cfg(unix)]
    {
        use_official_confirmed_with_backend(
            store,
            groups,
            &UnixUserEnvironment::new()?,
            confirm_unmanaged_clear,
            confirmation_token,
            store::save_device_store,
        )
    }
}

pub(super) fn validate_selection(
    store: &CopilotByokStore,
    groups: &[CopilotByokGroup],
) -> Result<(), AppError> {
    if !store.cli.enabled {
        return Ok(());
    }
    let group_id = store.cli.selected_group_id.as_deref().ok_or_else(|| {
        AppError::Config("Copilot CLI is enabled without a selected provider".to_string())
    })?;
    let model_id = store.cli.selected_model_id.as_deref().ok_or_else(|| {
        AppError::Config("Copilot CLI is enabled without a selected model".to_string())
    })?;
    selected(groups, group_id, model_id).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::cell::{Cell, RefCell};

    #[derive(Default)]
    struct MemoryEnvironment {
        values: RefCell<BTreeMap<String, String>>,
        broadcasts: Cell<usize>,
        fail_broadcast: Cell<bool>,
        fail_write_once: RefCell<Option<String>>,
        change_during_confirmation: Cell<bool>,
    }

    impl UserEnvironment for MemoryEnvironment {
        fn read(&self, name: &str) -> Result<Option<String>, AppError> {
            Ok(self.values.borrow().get(name).cloned())
        }

        fn write(&self, name: &str, value: Option<&str>) -> Result<(), AppError> {
            if self.fail_write_once.borrow().as_deref() == Some(name) {
                self.fail_write_once.borrow_mut().take();
                return Err(AppError::Config(
                    "simulated environment write failure".to_string(),
                ));
            }
            match value {
                Some(value) => {
                    self.values
                        .borrow_mut()
                        .insert(name.to_string(), value.to_string());
                }
                None => {
                    self.values.borrow_mut().remove(name);
                }
            }
            Ok(())
        }

        fn broadcast_change(
            &self,
            _before: &BTreeMap<String, Option<String>>,
            _after: &BTreeMap<String, Option<String>>,
        ) -> Result<(), AppError> {
            self.broadcasts.set(self.broadcasts.get() + 1);
            if self.fail_broadcast.get() {
                return Err(AppError::Config(
                    "simulated environment broadcast failure".to_string(),
                ));
            }
            Ok(())
        }

        fn confirmation_context(&self) -> Result<ConfirmationContext, AppError> {
            if self.change_during_confirmation.replace(false) {
                self.values.borrow_mut().insert(
                    "COPILOT_PROVIDER_MAX_OUTPUT_TOKENS".to_string(),
                    "32000".to_string(),
                );
            }
            Ok(BTreeMap::new())
        }
    }

    fn group() -> CopilotByokGroup {
        CopilotByokGroup {
            id: "provider".to_string(),
            name: "Provider".to_string(),
            url: "https://api.example.com/v1/responses".to_string(),
            api_key: "secret".to_string(),
            api_type: "responses".to_string(),
            website_url: None,
            notes: None,
            icon: None,
            icon_color: None,
            category: None,
            usage_script: None,
            enabled: true,
            request_headers: BTreeMap::from([(
                "X-Token".to_string(),
                "Token ${apiKey}".to_string(),
            )]),
            models: vec![CopilotByokModel {
                id: "model-record".to_string(),
                model_id: "wire-model".to_string(),
                name: "Model".to_string(),
                enabled: true,
                tool_calling: Some(true),
                vision: None,
                thinking: None,
                streaming: Some(true),
                context_window: Some(128_000),
                max_input_tokens: Some(120_000),
                max_output_tokens: Some(8_000),
                edit_tools: Vec::new(),
                zero_data_retention_enabled: false,
                supports_reasoning_effort: Vec::new(),
                reasoning_effort_format: None,
                model_options: json!({}),
                extra: BTreeMap::new(),
            }],
            extra: BTreeMap::new(),
        }
    }

    #[test]
    fn maps_responses_provider_to_cli_environment() {
        let desired = desired_environment(&group(), &group().models[0]).expect("environment");
        assert_eq!(
            desired["COPILOT_PROVIDER_BASE_URL"].as_deref(),
            Some("https://api.example.com/v1")
        );
        assert_eq!(
            desired["COPILOT_PROVIDER_WIRE_API"].as_deref(),
            Some("responses")
        );
        assert_eq!(desired["COPILOT_MODEL"].as_deref(), Some("wire-model"));
        assert_eq!(
            desired["COPILOT_PROVIDER_HEADERS"].as_deref(),
            Some("X-Token: Token secret")
        );
        assert_eq!(
            desired["COPILOT_PROVIDER_MODEL_ID"].as_deref(),
            Some("wire-model")
        );
        assert_eq!(
            desired["COPILOT_PROVIDER_WIRE_MODEL"].as_deref(),
            Some("wire-model")
        );
    }

    #[test]
    fn imports_an_existing_environment_as_a_stable_round_trip_provider() {
        let provider = group();
        let mut environment =
            desired_environment(&provider, &provider.models[0]).expect("source environment");
        environment.insert(
            "COPILOT_MODEL".to_string(),
            Some("user-facing-model".to_string()),
        );

        let imported =
            imported_group_from_environment(&environment, "Imported Copilot CLI Environment")
                .expect("import environment")
                .expect("environment should produce a provider");
        let imported_again =
            imported_group_from_environment(&environment, "Imported Copilot CLI Environment (2)")
                .expect("repeat import")
                .expect("environment should produce a provider");

        assert_eq!(imported.id, imported_again.id);
        assert_eq!(imported.models[0].name, "user-facing-model");
        assert_eq!(
            desired_environment(&imported, &imported.models[0]).expect("round-trip environment"),
            environment
        );
    }

    #[test]
    fn applies_a_single_model_chat_completions_provider() {
        let mut provider = group();
        provider.name = "Minimax".to_string();
        provider.url = "https://api.minimaxi.com/v1".to_string();
        provider.api_type = "chat-completions".to_string();
        provider.request_headers.clear();
        provider.models[0].model_id = "MiniMax-M3".to_string();
        provider.models[0].name = "MiniMax-M3".to_string();
        let groups = vec![provider];
        let environment = MemoryEnvironment::default();
        let mut store = CopilotByokStore::default();

        let state = apply_with_backend(
            &mut store,
            &groups,
            "provider",
            "model-record",
            &environment,
            |_| Ok(()),
        )
        .expect("single-model Chat Completions provider should activate");

        assert!(state.enabled);
        let values = environment.values.borrow();
        assert_eq!(
            values.get("COPILOT_PROVIDER_BASE_URL").map(String::as_str),
            Some("https://api.minimaxi.com/v1")
        );
        assert_eq!(
            values.get("COPILOT_PROVIDER_WIRE_API").map(String::as_str),
            Some("completions")
        );
        assert_eq!(
            values.get("COPILOT_MODEL").map(String::as_str),
            Some("MiniMax-M3")
        );
    }

    #[test]
    fn maps_official_azure_bearer_transport_and_model_overrides() {
        let mut provider = group();
        provider.url =
            "https://resource.openai.azure.com/openai/deployments/deploy/responses".to_string();
        provider
            .extra
            .insert(CLI_PROVIDER_TYPE_KEY.to_string(), json!("azure"));
        provider
            .extra
            .insert(CLI_BEARER_TOKEN_KEY.to_string(), json!("bearer-secret"));
        provider
            .extra
            .insert(CLI_TRANSPORT_KEY.to_string(), json!("websockets"));
        provider
            .extra
            .insert(CLI_AZURE_API_VERSION_KEY.to_string(), json!("2024-10-21"));
        provider.models[0]
            .extra
            .insert(CLI_MODEL_ID_KEY.to_string(), json!("gpt-5.4"));
        provider.models[0]
            .extra
            .insert(CLI_WIRE_MODEL_KEY.to_string(), json!("deploy"));

        let desired = desired_environment(&provider, &provider.models[0]).expect("environment");
        assert_eq!(
            desired["COPILOT_PROVIDER_BASE_URL"].as_deref(),
            Some("https://resource.openai.azure.com")
        );
        assert_eq!(desired["COPILOT_PROVIDER_TYPE"].as_deref(), Some("azure"));
        assert_eq!(
            desired["COPILOT_PROVIDER_BEARER_TOKEN"].as_deref(),
            Some("bearer-secret")
        );
        assert_eq!(
            desired["COPILOT_PROVIDER_TRANSPORT"].as_deref(),
            Some("websockets")
        );
        assert_eq!(desired["COPILOT_MODEL"].as_deref(), Some("gpt-5.4"));
        assert_eq!(
            desired["COPILOT_PROVIDER_WIRE_MODEL"].as_deref(),
            Some("deploy")
        );
    }

    #[test]
    fn unix_managed_environment_round_trips_arbitrary_utf8() {
        let temp = tempfile::tempdir().expect("temp directory");
        let path = temp.path().join("copilot-cli-env.sh");
        let mut values: BTreeMap<String, Option<String>> = MANAGED_VARIABLES
            .iter()
            .map(|name| ((*name).to_string(), None))
            .collect();
        let value = "line one\nline 'two' \\ $HOME 世界".to_string();
        values.insert("COPILOT_PROVIDER_API_KEY".to_string(), Some(value.clone()));
        fs::write(&path, render_posix_env(&values).expect("render env")).expect("write env");

        let parsed = read_posix_env_file(&path).expect("read env");
        assert_eq!(
            parsed["COPILOT_PROVIDER_API_KEY"].as_deref(),
            Some(value.as_str())
        );
    }

    #[test]
    fn unix_shell_hook_preserves_unmanaged_profile_content() {
        let original = "export PATH=\"$HOME/bin:$PATH\"\n";
        let enabled = update_managed_block(original, true).expect("enable hook");
        assert_eq!(
            managed_block_state(&enabled).expect("managed state"),
            ManagedBlockState::Exact
        );
        let disabled = update_managed_block(&enabled, false).expect("disable hook");
        assert_eq!(disabled, original);
    }

    #[test]
    fn unix_backend_manages_posix_and_fish_files_transactionally() {
        let _detected_home = UnixUserEnvironment::new().expect("detect user home");
        let temp = tempfile::tempdir().expect("temp directory");
        let backend = UnixUserEnvironment {
            home: temp.path().to_path_buf(),
        };
        let mut values: BTreeMap<String, Option<String>> = MANAGED_VARIABLES
            .iter()
            .map(|name| ((*name).to_string(), None))
            .collect();
        values.insert(
            "COPILOT_PROVIDER_BASE_URL".to_string(),
            Some("https://api.example.com/v1".to_string()),
        );
        values.insert(
            "COPILOT_PROVIDER_API_KEY".to_string(),
            Some("line one\nline 'two' 世界".to_string()),
        );

        backend.write_values(&values).expect("write environment");
        backend.update_shell_hooks(true).expect("enable hooks");

        assert_eq!(backend.read_values().expect("read environment"), values);
        assert!(backend
            .validate_managed_artifacts(&values)
            .expect("validate artifacts")
            .is_empty());
        assert!(fs::read_to_string(temp.path().join(".profile"))
            .expect("profile")
            .contains(POSIX_BLOCK_START));
        assert_eq!(
            fs::read_to_string(backend.fish_hook_path()).expect("fish hook"),
            fish_hook_contents()
        );

        backend.update_shell_hooks(false).expect("disable hooks");
        let cleared: BTreeMap<String, Option<String>> = MANAGED_VARIABLES
            .iter()
            .map(|name| ((*name).to_string(), None))
            .collect();
        backend.write_values(&cleared).expect("remove environment");
        assert!(!backend.env_path().exists());
        assert!(!backend.fish_env_path().exists());
        assert!(!backend.fish_hook_path().exists());
        assert!(!fs::read_to_string(temp.path().join(".profile"))
            .expect("profile")
            .contains(POSIX_BLOCK_START));
    }

    #[test]
    fn disabled_cli_providers_and_models_cannot_be_selected() {
        let mut disabled_provider = group();
        disabled_provider.enabled = false;
        assert!(selected(
            std::slice::from_ref(&disabled_provider),
            "provider",
            "model-record"
        )
        .is_err());

        let mut disabled_model = group();
        disabled_model.models[0].enabled = false;
        assert!(selected(
            std::slice::from_ref(&disabled_model),
            "provider",
            "model-record"
        )
        .is_err());
    }

    #[test]
    fn apply_then_disable_selects_official_instead_of_restoring_hidden_values() {
        let environment = MemoryEnvironment::default();
        environment
            .values
            .borrow_mut()
            .insert("COPILOT_MODEL".to_string(), "original-model".to_string());
        let groups = vec![group()];
        let mut store = CopilotByokStore {
            groups: groups.clone(),
            ..CopilotByokStore::default()
        };

        let applied = apply_with_backend(
            &mut store,
            &groups,
            "provider",
            "model-record",
            &environment,
            |_| Ok(()),
        )
        .expect("apply CLI environment");
        assert!(applied.enabled);
        assert!(applied.environment_matches);
        assert_eq!(
            environment.values.borrow().get("COPILOT_MODEL"),
            Some(&"wire-model".to_string())
        );

        let disabled = disable_with_backend(&mut store, &groups, &environment, |_| Ok(()))
            .expect("activate Official environment");
        assert!(!disabled.enabled);
        assert!(!environment.values.borrow().contains_key("COPILOT_MODEL"));
        assert!(!environment
            .values
            .borrow()
            .contains_key("COPILOT_PROVIDER_BASE_URL"));
        assert_eq!(environment.broadcasts.get(), 2);
    }

    #[test]
    fn official_provider_clears_custom_values_instead_of_restoring_them() {
        let environment = MemoryEnvironment::default();
        environment.values.borrow_mut().extend([
            (
                "COPILOT_PROVIDER_BASE_URL".to_string(),
                "https://old-provider.example.com/v1".to_string(),
            ),
            ("COPILOT_MODEL".to_string(), "old-custom-model".to_string()),
        ]);
        let groups = vec![group()];
        let mut store = CopilotByokStore {
            groups: groups.clone(),
            ..CopilotByokStore::default()
        };

        let unmanaged = state_with_backend(&store, &groups, &environment)
            .expect("inspect pre-existing custom environment");
        assert!(!unmanaged.enabled);
        assert!(!unmanaged.environment_matches);

        apply_with_backend(
            &mut store,
            &groups,
            "provider",
            "model-record",
            &environment,
            |_| Ok(()),
        )
        .expect("apply CLI environment");

        let official =
            use_official_with_backend(&mut store, &groups, &environment, false, |_| Ok(()))
                .expect("activate GitHub Copilot official provider");

        assert!(!official.enabled);
        assert!(official.selected_group_id.is_none());
        assert!(official.environment_matches);
        assert!(MANAGED_VARIABLES
            .iter()
            .all(|name| !environment.values.borrow().contains_key(*name)));
        assert_eq!(environment.broadcasts.get(), 2);
    }

    #[test]
    fn unmanaged_official_activation_requires_confirmation_without_a_restore_state() {
        let environment = MemoryEnvironment::default();
        environment.values.borrow_mut().insert(
            "COPILOT_PROVIDER_BASE_URL".to_string(),
            "https://unmanaged.example.com/v1".to_string(),
        );
        let groups = vec![group()];
        let mut store = CopilotByokStore::default();

        let error = use_official_with_backend(&mut store, &groups, &environment, false, |_| Ok(()))
            .expect_err("unmanaged overrides require explicit confirmation");
        assert!(matches!(error, AppError::Conflict(_)));
        assert_eq!(
            environment
                .values
                .borrow()
                .get("COPILOT_PROVIDER_BASE_URL")
                .map(String::as_str),
            Some("https://unmanaged.example.com/v1")
        );

        let official =
            use_official_with_backend(&mut store, &groups, &environment, true, |_| Ok(()))
                .expect("confirmed official activation");
        assert!(official.environment_matches);
        assert!(!environment
            .values
            .borrow()
            .contains_key("COPILOT_PROVIDER_BASE_URL"));

        disable_with_backend(&mut store, &groups, &environment, |_| Ok(()))
            .expect("Official selection remains idempotent");
        assert!(!environment
            .values
            .borrow()
            .contains_key("COPILOT_PROVIDER_BASE_URL"));
    }

    #[test]
    fn official_persistence_failure_rolls_back_unmanaged_environment() {
        let environment = MemoryEnvironment::default();
        environment
            .values
            .borrow_mut()
            .insert("COPILOT_MODEL".to_string(), "unmanaged-model".to_string());
        let groups = vec![group()];
        let mut store = CopilotByokStore::default();

        let error = use_official_with_backend(&mut store, &groups, &environment, true, |_| {
            Err(AppError::Config(
                "simulated persistence failure".to_string(),
            ))
        })
        .expect_err("persistence failure must roll back Official activation");

        assert!(matches!(error, AppError::Config(_)));
        assert_eq!(
            environment
                .values
                .borrow()
                .get("COPILOT_MODEL")
                .map(String::as_str),
            Some("unmanaged-model")
        );
        assert_eq!(store.cli, CopilotCliConfig::default());
    }

    #[test]
    fn official_broadcast_failure_rolls_back_environment_and_selection() {
        let environment = MemoryEnvironment::default();
        environment
            .values
            .borrow_mut()
            .insert("COPILOT_MODEL".to_string(), "unmanaged-model".to_string());
        environment.fail_broadcast.set(true);
        let groups = vec![group()];
        let mut store = CopilotByokStore::default();

        let error = use_official_with_backend(&mut store, &groups, &environment, true, |_| Ok(()))
            .expect_err("broadcast failure must roll back Official activation");

        assert!(error.to_string().contains("broadcast failure"));
        assert_eq!(
            environment
                .values
                .borrow()
                .get("COPILOT_MODEL")
                .map(String::as_str),
            Some("unmanaged-model")
        );
        assert_eq!(store.cli, CopilotCliConfig::default());
        assert_eq!(environment.broadcasts.get(), 2);
        assert!(error
            .to_string()
            .contains("failed to broadcast the restored environment"));
    }

    #[test]
    fn disabled_unix_state_reports_stale_managed_artifacts() {
        let home = tempfile::tempdir().expect("temporary home");
        let environment = UnixUserEnvironment {
            home: home.path().to_path_buf(),
        };
        fs::create_dir_all(
            environment
                .env_path()
                .parent()
                .expect("environment path parent"),
        )
        .expect("create managed environment directory");
        fs::write(
            environment.env_path(),
            "# Managed by CC Switch for GitHub Copilot CLI\nexport COPILOT_MODEL='stale'\n",
        )
        .expect("write stale environment file");
        fs::write(
            home.path().join(".profile"),
            format!(
                "before\n{}\nstale\n{}\nafter\n",
                POSIX_BLOCK_START, POSIX_BLOCK_END
            ),
        )
        .expect("write stale profile hook");

        let conflicts = environment
            .validate_managed_artifacts(&official_environment())
            .expect("inspect stale artifacts");
        assert!(conflicts
            .iter()
            .any(|path| path.ends_with("copilot-cli-env.sh")));
        assert!(conflicts.iter().any(|path| path.ends_with(".profile")));
    }

    #[test]
    fn external_edit_blocks_restore_without_overwriting_it() {
        let environment = MemoryEnvironment::default();
        let groups = vec![group()];
        let mut store = CopilotByokStore {
            groups: groups.clone(),
            ..CopilotByokStore::default()
        };
        apply_with_backend(
            &mut store,
            &groups,
            "provider",
            "model-record",
            &environment,
            |_| Ok(()),
        )
        .expect("apply CLI environment");
        environment.values.borrow_mut().insert(
            "COPILOT_PROVIDER_API_KEY".to_string(),
            "external-secret".to_string(),
        );

        let error = disable_with_backend(&mut store, &groups, &environment, |_| Ok(()))
            .expect_err("external edit must block restore");
        assert!(matches!(error, AppError::Conflict(_)));
        assert_eq!(
            environment
                .values
                .borrow()
                .get("COPILOT_PROVIDER_API_KEY")
                .map(String::as_str),
            Some("external-secret")
        );
        assert!(store.cli.enabled);
    }

    fn managed_test_environment() -> (CopilotByokStore, Vec<CopilotByokGroup>, MemoryEnvironment) {
        let environment = MemoryEnvironment::default();
        let groups = vec![group()];
        let mut store = CopilotByokStore::default();
        apply_with_backend(
            &mut store,
            &groups,
            "provider",
            "model-record",
            &environment,
            |_| Ok(()),
        )
        .expect("initial apply");
        (store, groups, environment)
    }

    #[test]
    #[serial_test::serial(copilot_cli_confirmation)]
    fn conflict_preview_is_read_only_and_hides_credential_values() {
        let (store, mut groups, environment) = managed_test_environment();
        environment
            .values
            .borrow_mut()
            .remove("COPILOT_PROVIDER_MAX_OUTPUT_TOKENS");
        environment.values.borrow_mut().insert(
            "COPILOT_PROVIDER_API_KEY".to_string(),
            "external-secret".to_string(),
        );
        groups[0].models[0].max_output_tokens = Some(16_000);
        let before = environment.values.borrow().clone();
        let preview =
            preview_with_backend(&store, Some(&groups[0]), &environment).expect("preview");
        let tokens = preview
            .changes
            .iter()
            .find(|change| change.name == "COPILOT_PROVIDER_MAX_OUTPUT_TOKENS")
            .expect("token limit difference");
        assert_eq!(tokens.previous.value.as_deref(), Some("8000"));
        assert!(!tokens.current.is_set);
        assert_eq!(tokens.desired.value.as_deref(), Some("16000"));
        let key = preview
            .changes
            .iter()
            .find(|change| change.name == "COPILOT_PROVIDER_API_KEY")
            .expect("key difference");
        assert!(key.sensitive && key.current.is_set);
        assert!(
            key.previous.value.is_none()
                && key.current.value.is_none()
                && key.desired.value.is_none()
        );
        let encoded = serde_json::to_string(&preview).expect("safe IPC serialization");
        assert!(!encoded.contains("external-secret"));
        assert!(!encoded.contains("\"secret\""));
        assert_eq!(*environment.values.borrow(), before);
        assert_eq!(environment.broadcasts.get(), 1);
    }

    #[test]
    #[serial_test::serial(copilot_cli_confirmation)]
    fn confirmed_reapply_uses_latest_provider_configuration_and_a_single_use_token() {
        let (mut store, mut groups, environment) = managed_test_environment();
        environment
            .values
            .borrow_mut()
            .remove("COPILOT_PROVIDER_MAX_OUTPUT_TOKENS");
        groups[0].models[0].max_output_tokens = Some(16_000);
        let before = environment.values.borrow().clone();
        assert!(apply_with_backend(
            &mut store,
            &groups,
            "provider",
            "model-record",
            &environment,
            |_| Ok(())
        )
        .is_err());
        assert_eq!(*environment.values.borrow(), before);
        let preview =
            preview_with_backend(&store, Some(&groups[0]), &environment).expect("preview");
        let state = apply_confirmed_with_backend(
            &mut store,
            &groups,
            "provider",
            "model-record",
            &environment,
            Some(&preview.confirmation_token),
            |_| Ok(()),
        )
        .expect("confirmed apply");
        assert!(state.environment_matches && state.environment_conflicts.is_empty());
        assert_eq!(
            environment
                .values
                .borrow()
                .get("COPILOT_PROVIDER_MAX_OUTPUT_TOKENS")
                .map(String::as_str),
            Some("16000")
        );
        assert!(apply_confirmed_with_backend(
            &mut store,
            &groups,
            "provider",
            "model-record",
            &environment,
            Some(&preview.confirmation_token),
            |_| Ok(())
        )
        .is_err());
    }

    #[test]
    #[serial_test::serial(copilot_cli_confirmation)]
    fn stale_environment_or_provider_confirmation_never_writes() {
        for changed_provider in [false, true] {
            let (mut store, mut groups, environment) = managed_test_environment();
            environment
                .values
                .borrow_mut()
                .remove("COPILOT_PROVIDER_MAX_OUTPUT_TOKENS");
            let preview =
                preview_with_backend(&store, Some(&groups[0]), &environment).expect("preview");
            if changed_provider {
                groups[0].api_key = "rotated-secret".to_string();
            } else {
                environment.values.borrow_mut().insert(
                    "COPILOT_PROVIDER_MAX_OUTPUT_TOKENS".to_string(),
                    "32000".to_string(),
                );
            }
            let before = environment.values.borrow().clone();
            let old_selection = store.cli.clone();
            let persisted = Cell::new(false);
            let error = apply_confirmed_with_backend(
                &mut store,
                &groups,
                "provider",
                "model-record",
                &environment,
                Some(&preview.confirmation_token),
                |_| {
                    persisted.set(true);
                    Ok(())
                },
            )
            .expect_err("stale preview");
            assert!(matches!(error, AppError::Conflict(_)));
            assert!(!persisted.get());
            assert_eq!(*environment.values.borrow(), before);
            assert_eq!(store.cli, old_selection);
        }
    }

    #[test]
    #[serial_test::serial(copilot_cli_confirmation)]
    fn prewrite_recheck_does_not_roll_back_a_new_external_edit() {
        let (mut store, groups, environment) = managed_test_environment();
        let preview =
            preview_with_backend(&store, Some(&groups[0]), &environment).expect("preview");
        let old_selection = store.cli.clone();
        environment.change_during_confirmation.set(true);
        assert!(apply_confirmed_with_backend(
            &mut store,
            &groups,
            "provider",
            "model-record",
            &environment,
            Some(&preview.confirmation_token),
            |_| panic!("must not persist a stale preview")
        )
        .is_err());
        assert_eq!(
            environment
                .values
                .borrow()
                .get("COPILOT_PROVIDER_MAX_OUTPUT_TOKENS")
                .map(String::as_str),
            Some("32000")
        );
        assert_eq!(environment.broadcasts.get(), 1);
        assert_eq!(store.cli, old_selection);
    }

    #[test]
    #[serial_test::serial(copilot_cli_confirmation)]
    fn confirmation_cannot_be_reused_for_another_provider() {
        let (mut store, mut groups, environment) = managed_test_environment();
        let preview =
            preview_with_backend(&store, Some(&groups[0]), &environment).expect("preview");
        let mut other = groups[0].clone();
        other.id = "other-provider".to_string();
        groups.push(other);
        assert!(apply_confirmed_with_backend(
            &mut store,
            &groups,
            "other-provider",
            "model-record",
            &environment,
            Some(&preview.confirmation_token),
            |_| Ok(())
        )
        .is_err());
        assert_eq!(store.cli.selected_group_id.as_deref(), Some("provider"));
    }

    #[test]
    #[serial_test::serial(copilot_cli_confirmation)]
    fn confirmed_failure_restores_external_values_and_selection() {
        for failed_broadcast in [false, true] {
            let (mut store, groups, environment) = managed_test_environment();
            environment.values.borrow_mut().insert(
                "COPILOT_PROVIDER_API_KEY".to_string(),
                "external-secret".to_string(),
            );
            let before = environment.values.borrow().clone();
            let old_selection = store.cli.clone();
            let preview =
                preview_with_backend(&store, Some(&groups[0]), &environment).expect("preview");
            environment.fail_broadcast.set(failed_broadcast);
            let error = apply_confirmed_with_backend(
                &mut store,
                &groups,
                "provider",
                "model-record",
                &environment,
                Some(&preview.confirmation_token),
                |_| {
                    if failed_broadcast {
                        Ok(())
                    } else {
                        Err(AppError::Config(
                            "simulated persistence failure".to_string(),
                        ))
                    }
                },
            )
            .expect_err("failed override");
            assert!(matches!(error, AppError::Config(_)));
            assert_eq!(*environment.values.borrow(), before);
            assert_eq!(store.cli, old_selection);
        }
    }

    #[test]
    #[serial_test::serial(copilot_cli_confirmation)]
    fn failed_confirmed_environment_write_preserves_all_values() {
        let (mut store, groups, environment) = managed_test_environment();
        environment.values.borrow_mut().insert(
            "COPILOT_PROVIDER_API_KEY".to_string(),
            "external-secret".to_string(),
        );
        environment
            .values
            .borrow_mut()
            .insert("UNRELATED_SETTING".to_string(), "keep-me".to_string());
        let preview =
            preview_with_backend(&store, Some(&groups[0]), &environment).expect("preview");
        let before = environment.values.borrow().clone();
        let old_selection = store.cli.clone();
        *environment.fail_write_once.borrow_mut() = Some("COPILOT_PROVIDER_MODEL_ID".to_string());
        assert!(apply_confirmed_with_backend(
            &mut store,
            &groups,
            "provider",
            "model-record",
            &environment,
            Some(&preview.confirmation_token),
            |_| panic!("must not persist after a write failure")
        )
        .is_err());
        assert_eq!(*environment.values.borrow(), before);
        assert_eq!(store.cli, old_selection);
    }

    #[test]
    #[serial_test::serial(copilot_cli_confirmation)]
    fn confirmed_official_selection_resolves_a_managed_conflict() {
        let (mut store, groups, environment) = managed_test_environment();
        environment
            .values
            .borrow_mut()
            .remove("COPILOT_PROVIDER_MAX_OUTPUT_TOKENS");
        assert!(
            use_official_with_backend(&mut store, &groups, &environment, false, |_| Ok(()))
                .is_err()
        );
        let preview = preview_with_backend(&store, None, &environment).expect("official preview");
        let state = use_official_confirmed_with_backend(
            &mut store,
            &groups,
            &environment,
            false,
            Some(&preview.confirmation_token),
            |_| Ok(()),
        )
        .expect("confirmed Official selection");
        assert!(!state.enabled && state.environment_matches);
        assert!(environment.values.borrow().is_empty());
    }

    #[test]
    #[serial_test::serial(copilot_cli_confirmation)]
    fn expired_confirmation_never_writes() {
        let (mut store, groups, environment) = managed_test_environment();
        let preview =
            preview_with_backend(&store, Some(&groups[0]), &environment).expect("preview");
        PENDING_CONFIRMATIONS
            .lock()
            .expect("pending confirmations")
            .iter_mut()
            .find(|entry| entry.token == preview.confirmation_token)
            .expect("stored preview")
            .created = Instant::now() - CONFIRMATION_TTL;
        assert!(apply_confirmed_with_backend(
            &mut store,
            &groups,
            "provider",
            "model-record",
            &environment,
            Some(&preview.confirmation_token),
            |_| Ok(())
        )
        .is_err());
        assert_eq!(environment.broadcasts.get(), 1);
    }

    #[test]
    #[serial_test::serial(copilot_cli_confirmation)]
    fn confirmed_shell_repair_preserves_unrelated_content_and_detects_new_edits() {
        let temp = tempfile::tempdir().expect("temporary home");
        let environment = UnixUserEnvironment {
            home: temp.path().to_path_buf(),
        };
        let groups = vec![group()];
        let mut store = CopilotByokStore::default();
        apply_with_backend(
            &mut store,
            &groups,
            "provider",
            "model-record",
            &environment,
            |_| Ok(()),
        )
        .expect("initial shell setup");
        let path = temp.path().join(".profile");
        let edited = format!("export KEEP_ME=1\n{POSIX_BLOCK_START}\n# external edit\n{POSIX_BLOCK_END}\nexport ALSO_KEEP_ME=2\n");
        fs::write(&path, &edited).expect("simulate edited managed block");
        let preview =
            preview_with_backend(&store, Some(&groups[0]), &environment).expect("shell preview");
        assert!(preview
            .integration_conflicts
            .contains(&path.to_string_lossy().to_string()));
        fs::write(&path, format!("{edited}# newer unrelated edit\n"))
            .expect("new edit during confirmation");
        assert!(apply_confirmed_with_backend(
            &mut store,
            &groups,
            "provider",
            "model-record",
            &environment,
            Some(&preview.confirmation_token),
            |_| Ok(())
        )
        .is_err());
        let preview = preview_with_backend(&store, Some(&groups[0]), &environment)
            .expect("refreshed preview");
        let state = apply_confirmed_with_backend(
            &mut store,
            &groups,
            "provider",
            "model-record",
            &environment,
            Some(&preview.confirmation_token),
            |_| Ok(()),
        )
        .expect("repair confirmed block");
        let repaired = fs::read_to_string(path).expect("repaired profile");
        assert!(
            repaired.contains("export KEEP_ME=1")
                && repaired.contains("export ALSO_KEEP_ME=2")
                && repaired.contains("# newer unrelated edit")
        );
        assert!(repaired.contains(&posix_hook_block()));
        assert!(state.environment_matches);
    }

    #[test]
    #[serial_test::serial(copilot_cli_confirmation)]
    fn failed_confirmed_shell_apply_restores_exact_artifact_contents() {
        let temp = tempfile::tempdir().expect("temporary home");
        let environment = UnixUserEnvironment {
            home: temp.path().to_path_buf(),
        };
        let groups = vec![group()];
        let mut store = CopilotByokStore::default();
        apply_with_backend(
            &mut store,
            &groups,
            "provider",
            "model-record",
            &environment,
            |_| Ok(()),
        )
        .expect("initial shell setup");
        fs::write(
            temp.path().join(".profile"),
            format!("# keep\n{POSIX_BLOCK_START}\n# malformed block without an end\n"),
        )
        .expect("malformed profile");
        fs::write(environment.fish_env_path(), "# external fish contents\n")
            .expect("edited fish environment");
        let before = environment
            .confirmation_context()
            .expect("original artifact bytes");
        let old_selection = store.cli.clone();
        let preview = preview_with_backend(&store, Some(&groups[0]), &environment)
            .expect("preview malformed hooks");
        assert!(apply_confirmed_with_backend(
            &mut store,
            &groups,
            "provider",
            "model-record",
            &environment,
            Some(&preview.confirmation_token),
            |_| Ok(())
        )
        .is_err());
        assert_eq!(
            environment
                .confirmation_context()
                .expect("restored artifacts"),
            before
        );
        assert_eq!(store.cli, old_selection);
    }

    #[test]
    fn provider_edit_requires_reapply_even_when_environment_is_untouched() {
        let environment = MemoryEnvironment::default();
        let groups = vec![group()];
        let mut store = CopilotByokStore {
            groups: groups.clone(),
            ..CopilotByokStore::default()
        };
        apply_with_backend(
            &mut store,
            &groups,
            "provider",
            "model-record",
            &environment,
            |_| Ok(()),
        )
        .expect("apply CLI environment");

        let mut edited_groups = groups;
        edited_groups[0].url = "https://new.example.com/v1/responses".to_string();
        let state = state_with_backend(&store, &edited_groups, &environment)
            .expect("read CLI environment state");

        assert!(!state.environment_matches);
        assert!(state.environment_conflicts.is_empty());
    }

    #[test]
    fn persistence_failure_rolls_back_environment_and_selection() {
        let environment = MemoryEnvironment::default();
        environment
            .values
            .borrow_mut()
            .insert("COPILOT_MODEL".to_string(), "original-model".to_string());
        let groups = vec![group()];
        let mut store = CopilotByokStore {
            groups: groups.clone(),
            ..CopilotByokStore::default()
        };

        let error = apply_with_backend(
            &mut store,
            &groups,
            "provider",
            "model-record",
            &environment,
            |_| {
                Err(AppError::Config(
                    "simulated persistence failure".to_string(),
                ))
            },
        )
        .expect_err("persistence failure must abort the switch");

        assert!(matches!(error, AppError::Config(_)));
        assert!(!store.cli.enabled);
        assert_eq!(
            environment.values.borrow().get("COPILOT_MODEL"),
            Some(&"original-model".to_string())
        );
        assert!(!environment
            .values
            .borrow()
            .contains_key("COPILOT_PROVIDER_BASE_URL"));
        assert_eq!(environment.broadcasts.get(), 0);
    }

    #[test]
    fn vscode_secret_reference_is_rejected_for_cli() {
        let mut provider = group();
        provider.api_key = "${input:provider-key}".to_string();

        let error = desired_environment(&provider, &provider.models[0])
            .expect_err("VS Code SecretStorage references are not available to the CLI");

        assert!(matches!(error, AppError::InvalidInput(_)));
    }
}
