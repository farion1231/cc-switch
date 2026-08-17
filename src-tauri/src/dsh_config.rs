//! DeepSeek Harness native-provider configuration support.
//!
//! CC Switch owns only the `llm-deepseek` and `agent-default-model` sections
//! in `$DSH_HOME/settings.yaml`. API keys remain references in that document;
//! their values live in the Harness-managed `.credentials.yaml` store.

#[cfg(not(unix))]
use crate::config::atomic_write;
use crate::config::{get_app_config_dir, get_home_dir};
use crate::error::AppError;
use crate::settings::effective_backup_retain_count;
use chrono::Local;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use serde_yaml::{Mapping as YamlMapping, Value as YamlValue};
use std::collections::HashSet;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
#[cfg(unix)]
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

pub const DSH_PROVIDER_ID: &str = "deepseek-official";
const DEFAULT_API_KEY_ENV: &str = "DEEPSEEK_API_KEY";
const SETTINGS_FILENAME: &str = "settings.yaml";
const CREDENTIALS_FILENAME: &str = ".credentials.yaml";
const LLM_SECTION: &str = "llm-deepseek";
const DEFAULT_MODEL_SECTION: &str = "agent-default-model";
const DEFAULT_MODEL_ID: &str = "deepseek-v4-flash";
const LOCK_RETRY_INITIAL: Duration = Duration::from_millis(20);
const LOCK_RETRY_MAX: Duration = Duration::from_millis(200);
const LOCK_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DshWriteOutcome {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backup_path: Option<String>,
    #[serde(skip)]
    pub(crate) rollback_token: Option<DshLiveStateSnapshot>,
}

#[derive(Debug)]
struct PreparedConfig {
    profile: JsonValue,
    default_model: String,
    default_reasoning_effort: Option<String>,
    credential_action: CredentialAction,
    api_key_env: String,
}

enum CredentialAction {
    Keep,
    Set(String),
    Unset,
}

impl std::fmt::Debug for CredentialAction {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Keep => formatter.write_str("Keep"),
            Self::Set(_) => formatter.write_str("Set([REDACTED])"),
            Self::Unset => formatter.write_str("Unset"),
        }
    }
}

struct CredentialUpdate {
    path: PathBuf,
    previous: Option<Vec<u8>>,
    next: Vec<u8>,
}

#[derive(Clone, PartialEq, Eq)]
struct DshFileTransition {
    before: Option<Vec<u8>>,
    committed: Option<Vec<u8>>,
}

/// Compare-and-restore token produced by the same locked read/modify/write
/// operation that committed the live Harness files.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct DshLiveStateSnapshot {
    home: PathBuf,
    settings: Option<DshFileTransition>,
    credentials: Option<DshFileTransition>,
}

impl std::fmt::Debug for DshLiveStateSnapshot {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DshLiveStateSnapshot")
            .field("home", &self.home)
            .field("settings_changed", &self.settings.is_some())
            .field("credentials_changed", &self.credentials.is_some())
            .finish()
    }
}

/// Cross-process writer lock compatible with DeepSeek Harness' `<file>.lock`
/// protocol. Harness never removes a contender's lock, so neither do we.
struct DshFileLock {
    path: Option<PathBuf>,
}

impl DshFileLock {
    fn release(mut self) -> Result<(), AppError> {
        let path = self
            .path
            .as_ref()
            .expect("DeepSeek Harness writer lock must have a path");
        match fs::remove_file(path) {
            Ok(()) => {
                self.path.take();
                Ok(())
            }
            Err(error) if error.kind() == ErrorKind::NotFound => {
                self.path.take();
                Ok(())
            }
            Err(error) => Err(AppError::io(path, error)),
        }
    }
}

impl Drop for DshFileLock {
    fn drop(&mut self) {
        let Some(path) = self.path.as_ref() else {
            return;
        };
        if let Err(error) = fs::remove_file(path) {
            if error.kind() != ErrorKind::NotFound {
                log::warn!(
                    "Failed to release DeepSeek Harness writer lock {}: {error}",
                    path.display()
                );
            }
        }
    }
}

fn dsh_write_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn acquire_file_lock(filename: &Path) -> Result<DshFileLock, AppError> {
    let mut lock_name = filename.as_os_str().to_os_string();
    lock_name.push(".lock");
    let lock_path = PathBuf::from(lock_name);
    let deadline = Instant::now() + LOCK_TIMEOUT;
    let mut delay = LOCK_RETRY_INITIAL;
    loop {
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        match options.open(&lock_path) {
            Ok(mut file) => {
                use std::io::Write;
                if let Err(error) = writeln!(file, "{}", std::process::id()) {
                    let _ = fs::remove_file(&lock_path);
                    return Err(AppError::io(&lock_path, error));
                }
                return Ok(DshFileLock {
                    path: Some(lock_path),
                });
            }
            Err(error) if error.kind() == ErrorKind::AlreadyExists => {
                if Instant::now() >= deadline {
                    return Err(AppError::Config(format!(
                        "Timed out waiting for DeepSeek Harness writer lock {}",
                        lock_path.display()
                    )));
                }
                thread::sleep(delay);
                delay = std::cmp::min(delay.saturating_mul(2), LOCK_RETRY_MAX);
            }
            Err(error) => return Err(AppError::io(&lock_path, error)),
        }
    }
}

fn combine_errors(context: &str, first: AppError, second: AppError) -> AppError {
    AppError::Message(format!("{context}: {first}; additionally: {second}"))
}

fn release_file_locks(
    credentials_lock: Option<DshFileLock>,
    settings_lock: DshFileLock,
) -> Result<(), AppError> {
    let settings_result = settings_lock.release();
    let credentials_result = credentials_lock
        .map(DshFileLock::release)
        .transpose()
        .map(|_| ());
    match (settings_result, credentials_result) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(error), Ok(())) | (Ok(()), Err(error)) => Err(error),
        (Err(first), Err(second)) => Err(combine_errors(
            "Failed to release DeepSeek Harness writer locks",
            first,
            second,
        )),
    }
}

fn finish_locked<T>(
    operation: Result<T, AppError>,
    credentials_lock: Option<DshFileLock>,
    settings_lock: DshFileLock,
) -> Result<T, AppError> {
    let release = release_file_locks(credentials_lock, settings_lock);
    match (operation, release) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(error), Ok(())) => Err(error),
        // Once the live files are committed, returning Err would discard a
        // rollback token and falsely tell the caller that nothing changed.
        // Keep the successful outcome; the stale lock itself makes later
        // conforming writes fail visibly instead of permitting divergence.
        (Ok(value), Err(error)) => {
            log::warn!(
                "DeepSeek Harness files committed, but an explicit writer lock release failed: {error}"
            );
            Ok(value)
        }
        (Err(first), Err(second)) => Err(combine_errors(
            "DeepSeek Harness operation and lock release both failed",
            first,
            second,
        )),
    }
}

fn acquire_native_file_locks(
    settings_path: &Path,
    credentials_path: &Path,
    include_credentials: bool,
) -> Result<(Option<DshFileLock>, DshFileLock), AppError> {
    if !include_credentials {
        return Ok((None, acquire_file_lock(settings_path)?));
    }

    if credentials_path < settings_path {
        let credentials_lock = acquire_file_lock(credentials_path)?;
        match acquire_file_lock(settings_path) {
            Ok(settings_lock) => Ok((Some(credentials_lock), settings_lock)),
            Err(error) => match credentials_lock.release() {
                Ok(()) => Err(error),
                Err(release_error) => Err(combine_errors(
                    "Failed to acquire DeepSeek Harness settings lock",
                    error,
                    release_error,
                )),
            },
        }
    } else {
        let settings_lock = acquire_file_lock(settings_path)?;
        match acquire_file_lock(credentials_path) {
            Ok(credentials_lock) => Ok((Some(credentials_lock), settings_lock)),
            Err(error) => match settings_lock.release() {
                Ok(()) => Err(error),
                Err(release_error) => Err(combine_errors(
                    "Failed to acquire DeepSeek Harness credentials lock",
                    error,
                    release_error,
                )),
            },
        }
    }
}

/// Resolve the Harness home with native precedence: explicit override,
/// non-blank `DSH_HOME`, then `~/.dsh`.
pub fn resolve_dsh_home(override_dir: Option<&Path>) -> PathBuf {
    if let Some(path) = override_dir {
        return absolutize_dsh_home(path);
    }
    if let Some(raw) = std::env::var_os("DSH_HOME") {
        let value = raw.to_string_lossy();
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return absolutize_dsh_home(Path::new(trimmed));
        }
    }
    get_home_dir().join(".dsh")
}

fn absolutize_dsh_home(path: &Path) -> PathBuf {
    let raw = path.to_string_lossy();
    let expanded = if raw == "~" {
        get_home_dir()
    } else if let Some(rest) = raw.strip_prefix("~/").or_else(|| raw.strip_prefix("~\\")) {
        get_home_dir().join(rest)
    } else {
        path.to_path_buf()
    };
    if expanded.is_absolute() {
        expanded
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| get_home_dir())
            .join(expanded)
    }
}

#[cfg(windows)]
fn is_wsl_unc_path(path: &Path) -> bool {
    let normalized = path.to_string_lossy().replace('/', "\\");
    let lower = normalized.to_ascii_lowercase();
    let canonical = lower
        .strip_prefix("\\\\?\\unc\\")
        .map(|rest| format!("\\\\{rest}"))
        .unwrap_or(lower);
    canonical.starts_with("\\\\wsl$\\") || canonical.starts_with("\\\\wsl.localhost\\")
}

fn ensure_safe_live_write_path(home: &Path) -> Result<(), AppError> {
    #[cfg(windows)]
    {
        if is_wsl_unc_path(home) {
            return Err(AppError::InvalidInput(format!(
                "Refusing to write DeepSeek Harness files through Windows WSL UNC path {}; configure and run CC Switch inside WSL so settings and credentials remain owner-only",
                home.display()
            )));
        }
        let mut resolved_ancestor = Some(home);
        let resolves_into_wsl = loop {
            let Some(candidate) = resolved_ancestor else {
                break false;
            };
            match fs::canonicalize(candidate) {
                Ok(resolved) => break is_wsl_unc_path(&resolved),
                Err(error) if error.kind() == ErrorKind::NotFound => {
                    resolved_ancestor = candidate.parent();
                }
                Err(error) => return Err(AppError::io(candidate, error)),
            }
        };
        if resolves_into_wsl {
            return Err(AppError::InvalidInput(format!(
                "Refusing to write DeepSeek Harness files through Windows WSL UNC path {}; configure and run CC Switch inside WSL so settings and credentials remain owner-only",
                home.display()
            )));
        }
    }
    Ok(())
}

fn read_optional_bytes(path: &Path) -> Result<Option<Vec<u8>>, AppError> {
    match fs::read(path) {
        Ok(contents) => Ok(Some(contents)),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(AppError::io(path, error)),
    }
}

fn read_optional_string(path: &Path) -> Result<Option<String>, AppError> {
    match fs::read_to_string(path) {
        Ok(contents) => Ok(Some(contents)),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(AppError::io(path, error)),
    }
}

fn read_optional_private_bytes(path: &Path) -> Result<Option<Vec<u8>>, AppError> {
    use std::io::Read;

    let mut file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(AppError::io(path, error)),
    };
    let metadata = file.metadata().map_err(|error| AppError::io(path, error))?;
    assert_owner_only_metadata(path, &metadata)?;

    let mut contents = Vec::new();
    file.read_to_end(&mut contents)
        .map_err(|error| AppError::io(path, error))?;
    Ok(Some(contents))
}

/// Resolve the configured Harness home used by CC Switch.
pub fn get_dsh_dir() -> PathBuf {
    resolve_dsh_home(crate::settings::get_deepseek_harness_override_dir().as_deref())
}

pub fn get_dsh_settings_path() -> PathBuf {
    get_dsh_dir().join(SETTINGS_FILENAME)
}

pub fn get_dsh_credentials_path() -> PathBuf {
    get_dsh_dir().join(CREDENTIALS_FILENAME)
}

impl DshLiveStateSnapshot {
    pub(crate) fn restore(&self) -> Result<(), AppError> {
        ensure_safe_live_write_path(&self.home)?;
        if self.settings.is_none() && self.credentials.is_none() {
            return Ok(());
        }
        let _guard = dsh_write_lock().lock()?;
        ensure_private_home(&self.home)?;

        let settings_path = self.home.join(SETTINGS_FILENAME);
        let credentials_path = self.home.join(CREDENTIALS_FILENAME);
        let (credentials_lock, settings_lock) = acquire_native_file_locks(
            &settings_path,
            &credentials_path,
            self.credentials.is_some(),
        )?;
        let operation = (|| {
            // Compare every tracked file before restoring either one. This
            // prevents a failed DB transaction from clobbering Harness edits
            // that landed after CC Switch committed the live files.
            if let Some(transition) = self.credentials.as_ref() {
                verify_committed_file(&credentials_path, transition)?;
            }
            if let Some(transition) = self.settings.as_ref() {
                verify_committed_file(&settings_path, transition)?;
            }

            // Restore the old settings generation first. Reintroducing an old
            // credential while the newly selected endpoint/ref is still live
            // could expose that secret to the wrong endpoint. If settings
            // restoration fails, short-circuit and keep the committed
            // credential state rather than creating that unsafe combination.
            restore_settings_then_credentials(
                || match self.settings.as_ref() {
                    Some(transition) => restore_file_transition(&settings_path, transition, false),
                    None => Ok(()),
                },
                || match self.credentials.as_ref() {
                    Some(transition) => {
                        restore_file_transition(&credentials_path, transition, true)
                    }
                    None => Ok(()),
                },
            )
        })();
        finish_locked(operation, credentials_lock, settings_lock)
    }
}

fn restore_settings_then_credentials(
    restore_settings: impl FnOnce() -> Result<(), AppError>,
    restore_credentials: impl FnOnce() -> Result<(), AppError>,
) -> Result<(), AppError> {
    restore_settings()?;
    restore_credentials()
}

/// Read the native DeepSeek profile currently represented by Harness user
/// settings. Environment-only credentials are intentionally never returned.
pub fn read_live_settings() -> Result<Option<JsonValue>, AppError> {
    read_live_config_at(Some(&get_dsh_dir()))
}

/// Write a native DeepSeek profile and make it the default Harness selection.
pub fn write_live_settings(settings_config: &JsonValue) -> Result<DshWriteOutcome, AppError> {
    write_live_settings_transactional(settings_config)
}

pub(crate) fn write_live_settings_transactional(
    settings_config: &JsonValue,
) -> Result<DshWriteOutcome, AppError> {
    let backup_root = get_app_config_dir()
        .join("backups")
        .join("deepseek-harness");
    write_live_config_inner(&get_dsh_dir(), settings_config, Some(&backup_root))
}

/// Explicit-home variant used by tests and callers that already resolved the
/// Harness home. It does not create CC Switch backup artifacts.
pub fn read_live_config_at(override_dir: Option<&Path>) -> Result<Option<JsonValue>, AppError> {
    let home = resolve_dsh_home(override_dir);
    let settings_path = home.join(SETTINGS_FILENAME);
    let Some(raw) = read_optional_string(&settings_path)? else {
        return Ok(None);
    };

    let root = parse_settings_document(&raw, &settings_path)?;
    let llm_key = YamlValue::String(LLM_SECTION.to_string());
    let llm = root.get(&llm_key);
    let selection = read_selection(&root, &settings_path)?;

    if let Some(selection) = selection.as_ref() {
        // User settings are layered over Harness' bundled base config, so a
        // missing provider still resolves to the official DeepSeek route.
        if selection.provider.as_deref().unwrap_or(DSH_PROVIDER_ID) != DSH_PROVIDER_ID {
            return Ok(None);
        }
    }

    if llm.is_none() && selection.is_none() {
        return Ok(None);
    }

    let mut profile = match llm {
        Some(YamlValue::Mapping(_)) => yaml_to_json(llm.expect("checked Some"), LLM_SECTION)?,
        Some(_) => {
            return Err(AppError::Config(format!(
                "DeepSeek Harness section '{LLM_SECTION}' must be a mapping in {}",
                settings_path.display()
            )))
        }
        None => JsonValue::Object(serde_json::Map::new()),
    };
    let object = profile.as_object_mut().ok_or_else(|| {
        AppError::Config(format!(
            "DeepSeek Harness section '{LLM_SECTION}' must be an object"
        ))
    })?;

    if let Some(selection) = selection {
        object.insert(
            "defaultModel".to_string(),
            JsonValue::String(
                selection
                    .model
                    .unwrap_or_else(|| DEFAULT_MODEL_ID.to_string()),
            ),
        );
        if let Some(effort) = selection.reasoning_effort {
            object.insert(
                "defaultReasoningEffort".to_string(),
                JsonValue::String(effort),
            );
        }
    } else {
        // Harness supplies this selection from its bundled base config when
        // the user settings file only overrides the DeepSeek provider.
        object.insert(
            "defaultModel".to_string(),
            JsonValue::String(DEFAULT_MODEL_ID.to_string()),
        );
    }

    validate_settings_config(&JsonValue::Object(object.clone())).map_err(|error| {
        AppError::Config(format!(
            "Invalid DeepSeek Harness provider settings in {}: {error}",
            settings_path.display()
        ))
    })?;

    let api_key_env = object
        .get("apiKeyEnv")
        .and_then(JsonValue::as_str)
        .unwrap_or(DEFAULT_API_KEY_ENV);
    validate_credential_ref(api_key_env)?;
    if let Some(api_key) = read_managed_credential(&home, api_key_env)? {
        object.insert("apiKey".to_string(), JsonValue::String(api_key));
    }

    Ok(Some(profile))
}

/// Explicit-home write variant. Unlike [`write_live_settings`], it does not
/// create backups under the CC Switch data directory.
#[cfg(test)]
fn write_live_config_at(
    override_dir: Option<&Path>,
    settings_config: &JsonValue,
) -> Result<DshWriteOutcome, AppError> {
    let home = resolve_dsh_home(override_dir);
    write_live_config_inner(&home, settings_config, None)
}

#[cfg(test)]
fn get_current_model_at(override_dir: Option<&Path>) -> Result<Option<String>, AppError> {
    let home = resolve_dsh_home(override_dir);
    let path = home.join(SETTINGS_FILENAME);
    let Some(raw) = read_optional_string(&path)? else {
        return Ok(None);
    };
    let root = parse_settings_document(&raw, &path)?;
    let Some(selection) = read_selection(&root, &path)? else {
        return Ok(None);
    };
    if selection.provider.as_deref() != Some(DSH_PROVIDER_ID) {
        return Ok(None);
    }
    Ok(selection.model)
}

fn write_live_config_inner(
    home: &Path,
    settings_config: &JsonValue,
    backup_root: Option<&Path>,
) -> Result<DshWriteOutcome, AppError> {
    let prepared = prepare_config(settings_config)?;
    reject_current_process_env_shadow(&prepared)?;
    ensure_safe_live_write_path(home)?;
    let _guard = dsh_write_lock().lock()?;

    let path = home.join(SETTINGS_FILENAME);
    let credentials_path = home.join(CREDENTIALS_FILENAME);
    ensure_private_home(home)?;
    // A settings-only switch must not contend with Harness' credential store.
    // Set/Unset operations still acquire both native locks in stable path
    // order so separate CC Switch processes cannot deadlock each other.
    let include_credentials = !matches!(prepared.credential_action, CredentialAction::Keep);
    let (credentials_lock, settings_lock) =
        acquire_native_file_locks(&path, &credentials_path, include_credentials)?;

    let operation = (|| {
        // The settings read is protected by its native writer lock. Credential
        // reads below occur only for Set/Unset, while the credential lock is held.
        let raw_bytes = read_optional_bytes(&path)?;
        let raw = raw_bytes
            .as_deref()
            .map(|contents| {
                std::str::from_utf8(contents)
                    .map(str::to_owned)
                    .map_err(|_| {
                        AppError::Config(format!(
                            "DeepSeek Harness settings at {} must be UTF-8",
                            path.display()
                        ))
                    })
            })
            .transpose()?
            .unwrap_or_default();
        let root = parse_settings_document(&raw, &path)?;
        let credential_update = match &prepared.credential_action {
            CredentialAction::Keep => None,
            CredentialAction::Set(api_key) => {
                prepare_managed_credential(home, &prepared.api_key_env, Some(api_key))?
            }
            CredentialAction::Unset => {
                prepare_managed_credential(home, &prepared.api_key_env, None)?
            }
        };
        reject_unsafe_credential_generation(&prepared, &root, &path, credential_update.is_some())?;

        let llm_yaml = serde_yaml::to_value(&prepared.profile)
            .map_err(|e| AppError::Config(format!("Failed to serialize DeepSeek profile: {e}")))?;
        let mut selection = YamlMapping::new();
        selection.insert(
            YamlValue::String("provider".to_string()),
            YamlValue::String(DSH_PROVIDER_ID.to_string()),
        );
        selection.insert(
            YamlValue::String("model".to_string()),
            YamlValue::String(prepared.default_model.clone()),
        );
        if let Some(effort) = prepared.default_reasoning_effort.as_ref() {
            selection.insert(
                YamlValue::String("reasoningEffort".to_string()),
                YamlValue::String(effort.clone()),
            );
        }

        let with_llm = patch_top_level_mapping_section(&raw, &root, LLM_SECTION, &llm_yaml)?;
        let next_root = parse_settings_document(&with_llm, &path)?;
        let next = patch_top_level_mapping_section(
            &with_llm,
            &next_root,
            DEFAULT_MODEL_SECTION,
            &YamlValue::Mapping(selection),
        )?;
        parse_settings_document(&next, &path)?;

        if next == raw && credential_update.is_none() {
            return Ok(DshWriteOutcome::default());
        }

        let backup_path = match (backup_root, raw.is_empty()) {
            (Some(root), false) => Some(create_settings_backup(root, &raw)?),
            _ => None,
        };
        if let Some(update) = credential_update.as_ref() {
            apply_credential_update(update)?;
        }
        if next != raw {
            if let Err(error) = write_settings_atomic(&path, next.as_bytes()) {
                if let Some(update) = credential_update.as_ref() {
                    if let Err(rollback_error) = rollback_credential_update(update) {
                        return Err(AppError::Message(format!(
                            "Failed to write DeepSeek Harness settings ({error}); credential rollback also failed ({rollback_error})"
                        )));
                    }
                }
                return Err(error);
            }
        }
        let settings_transition = (next != raw).then(|| DshFileTransition {
            before: raw_bytes,
            committed: Some(next.as_bytes().to_vec()),
        });
        let credentials_transition = credential_update.as_ref().map(|update| DshFileTransition {
            before: update.previous.clone(),
            committed: Some(update.next.clone()),
        });
        Ok(DshWriteOutcome {
            backup_path: backup_path.map(|p| p.display().to_string()),
            rollback_token: Some(DshLiveStateSnapshot {
                home: home.to_path_buf(),
                settings: settings_transition,
                credentials: credentials_transition,
            }),
        })
    })();

    finish_locked(operation, credentials_lock, settings_lock)
}

fn reject_unsafe_credential_generation(
    prepared: &PreparedConfig,
    root: &YamlMapping,
    settings_path: &Path,
    credential_changes: bool,
) -> Result<(), AppError> {
    if !credential_changes || !matches!(prepared.credential_action, CredentialAction::Set(_)) {
        return Ok(());
    }

    let llm_key = YamlValue::String(LLM_SECTION.to_string());
    let (current_endpoint, current_reference) = match root.get(&llm_key) {
        None => (None, DEFAULT_API_KEY_ENV.to_string()),
        Some(YamlValue::Mapping(mapping)) => {
            let base_url_key = YamlValue::String("baseURL".to_string());
            let endpoint = match mapping.get(&base_url_key) {
                None | Some(YamlValue::Null) => None,
                Some(YamlValue::String(value)) => Some(value.clone()),
                Some(_) => {
                    return Err(AppError::Config(format!(
                        "DeepSeek Harness setting '{LLM_SECTION}.baseURL' must be a string in {}",
                        settings_path.display()
                    )))
                }
            };
            let reference_key = YamlValue::String("apiKeyEnv".to_string());
            let reference = match mapping.get(&reference_key) {
                None | Some(YamlValue::Null) => DEFAULT_API_KEY_ENV.to_string(),
                Some(YamlValue::String(value)) => value.clone(),
                Some(_) => {
                    return Err(AppError::Config(format!(
                        "DeepSeek Harness setting '{LLM_SECTION}.apiKeyEnv' must be a string in {}",
                        settings_path.display()
                    )))
                }
            };
            validate_credential_ref(&reference).map_err(|error| {
                AppError::Config(format!(
                    "Invalid DeepSeek Harness credential reference in {}: {error}",
                    settings_path.display()
                ))
            })?;
            (endpoint, reference)
        }
        Some(_) => {
            return Err(AppError::Config(format!(
                "DeepSeek Harness section '{LLM_SECTION}' must be a mapping in {}",
                settings_path.display()
            )))
        }
    };
    let target_endpoint = prepared
        .profile
        .get("baseURL")
        .and_then(JsonValue::as_str)
        .map(ToOwned::to_owned);

    if current_reference == prepared.api_key_env && current_endpoint != target_endpoint {
        return Err(AppError::InvalidInput(format!(
            "Refusing to change DeepSeek Harness baseURL and API key under the same credential reference '{}'; use a new apiKeyEnv so the new credential is installed before the endpoint is activated",
            prepared.api_key_env
        )));
    }
    Ok(())
}

/// Validate a persisted provider bundle without reading or writing Harness
/// files. Provider transactions use this before committing non-current rows.
pub(crate) fn validate_settings_config(settings_config: &JsonValue) -> Result<(), AppError> {
    prepare_config(settings_config).map(|_| ())
}

fn reject_current_process_env_shadow(prepared: &PreparedConfig) -> Result<(), AppError> {
    if !matches!(prepared.credential_action, CredentialAction::Keep)
        && std::env::var_os(&prepared.api_key_env).is_some_and(|value| !value.is_empty())
    {
        return Err(AppError::InvalidInput(format!(
            "DeepSeek Harness credential '{}' is supplied read-only by the launching environment; unset it before switching",
            prepared.api_key_env
        )));
    }
    Ok(())
}

fn prepare_config(settings_config: &JsonValue) -> Result<PreparedConfig, AppError> {
    let mut profile = settings_config.as_object().cloned().ok_or_else(|| {
        AppError::InvalidInput("DeepSeek Harness provider config must be an object".to_string())
    })?;

    let default_model = required_private_string(&mut profile, "defaultModel", "model")?;
    if default_model.chars().any(char::is_control) {
        return Err(AppError::InvalidInput(
            "DeepSeek Harness model must not contain control characters".to_string(),
        ));
    }

    let requested_default_reasoning_effort = optional_private_string(
        &mut profile,
        "defaultReasoningEffort",
        "default reasoning effort",
    )?;
    if let Some(effort) = requested_default_reasoning_effort.as_deref() {
        if !matches!(effort, "off" | "low" | "high" | "max") {
            return Err(AppError::InvalidInput(
                "DeepSeek Harness default reasoning effort must be off, low, high, or max"
                    .to_string(),
            ));
        }
    }

    let credential_action = match profile.remove("apiKey") {
        None | Some(JsonValue::Null) => CredentialAction::Keep,
        Some(JsonValue::String(value)) => {
            let value = value.trim_matches(is_ecmascript_whitespace);
            if value.is_empty() {
                CredentialAction::Unset
            } else if !value.bytes().all(|byte| (0x21..=0x7e).contains(&byte)) {
                return Err(AppError::InvalidInput(
                    "DeepSeek Harness API key must contain only printable ASCII characters without spaces"
                        .to_string(),
                ));
            } else {
                CredentialAction::Set(value.to_string())
            }
        }
        Some(_) => {
            return Err(AppError::InvalidInput(
                "DeepSeek Harness apiKey must be a string".to_string(),
            ))
        }
    };

    let api_key_env = match profile.get("apiKeyEnv") {
        None | Some(JsonValue::Null) => DEFAULT_API_KEY_ENV.to_string(),
        Some(JsonValue::String(value)) => value.trim_matches(is_ecmascript_whitespace).to_string(),
        Some(_) => {
            return Err(AppError::InvalidInput(
                "DeepSeek Harness apiKeyEnv must be a string".to_string(),
            ))
        }
    };
    validate_credential_ref(&api_key_env)?;
    if profile.contains_key("apiKeyEnv") {
        profile.insert(
            "apiKeyEnv".to_string(),
            JsonValue::String(api_key_env.clone()),
        );
    }

    let profile_reasoning_effort = match profile.get("reasoningEffort") {
        None | Some(JsonValue::Null) => None,
        Some(JsonValue::String(value))
            if matches!(value.as_str(), "off" | "low" | "high" | "max") =>
        {
            Some(value.as_str())
        }
        Some(JsonValue::String(_)) => {
            return Err(AppError::InvalidInput(
                "DeepSeek Harness reasoningEffort must be off, low, high, or max".to_string(),
            ))
        }
        Some(_) => {
            return Err(AppError::InvalidInput(
                "DeepSeek Harness reasoningEffort must be a string".to_string(),
            ))
        }
    };
    let thinking_disabled = match profile.get("thinking") {
        None | Some(JsonValue::Null) => false,
        Some(JsonValue::String(value)) if value == "disabled" => true,
        Some(JsonValue::String(value)) if value == "enabled" => false,
        Some(JsonValue::String(_)) => {
            return Err(AppError::InvalidInput(
                "DeepSeek Harness thinking must be enabled or disabled".to_string(),
            ))
        }
        Some(_) => {
            return Err(AppError::InvalidInput(
                "DeepSeek Harness thinking must be a string".to_string(),
            ))
        }
    };
    // The provider profile and the default-model selection are independent
    // native settings. Keep an omitted selection effort omitted instead of
    // copying `llm-deepseek.reasoningEffort` into `agent-default-model`.
    let default_reasoning_effort =
        requested_default_reasoning_effort.or_else(|| thinking_disabled.then(|| "off".to_string()));
    if thinking_disabled
        && (profile_reasoning_effort.is_some_and(|effort| effort != "off")
            || default_reasoning_effort.as_deref() != Some("off"))
    {
        return Err(AppError::InvalidInput(
            "DeepSeek Harness thinking=disabled requires reasoning effort off".to_string(),
        ));
    }

    validate_optional_string(profile.get("baseURL"), "baseURL")?;
    validate_positive_integer(profile.get("maxTokens"), "maxTokens")?;
    validate_positive_integer(profile.get("defaultContextWindow"), "defaultContextWindow")?;
    if let Some(value) = profile.get("streamIdleTimeoutMs") {
        let timeout = value.as_f64().filter(|value| value.is_finite());
        if timeout.is_none_or(|value| value <= 0.0 || value > i32::MAX as f64) {
            return Err(AppError::InvalidInput(format!(
                "DeepSeek Harness streamIdleTimeoutMs must be positive and no greater than {}",
                i32::MAX
            )));
        }
    }
    validate_retry_policy(profile.get("retryPolicy"))?;
    validate_models(profile.get("models"))?;
    Ok(PreparedConfig {
        profile: JsonValue::Object(profile),
        default_model,
        default_reasoning_effort,
        credential_action,
        api_key_env,
    })
}

fn is_ecmascript_whitespace(character: char) -> bool {
    matches!(
        character,
        '\u{0009}'
            | '\u{000A}'
            | '\u{000B}'
            | '\u{000C}'
            | '\u{000D}'
            | '\u{0020}'
            | '\u{00A0}'
            | '\u{1680}'
            | '\u{2000}'
            ..='\u{200A}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202F}'
                | '\u{205F}'
                | '\u{3000}'
                | '\u{FEFF}'
    )
}

fn required_private_string(
    object: &mut serde_json::Map<String, JsonValue>,
    key: &str,
    label: &str,
) -> Result<String, AppError> {
    match object.remove(key) {
        Some(JsonValue::String(value)) if !value.trim().is_empty() => Ok(value.trim().to_string()),
        _ => Err(AppError::InvalidInput(format!(
            "DeepSeek Harness {label} is required"
        ))),
    }
}

fn optional_private_string(
    object: &mut serde_json::Map<String, JsonValue>,
    key: &str,
    label: &str,
) -> Result<Option<String>, AppError> {
    match object.remove(key) {
        None | Some(JsonValue::Null) => Ok(None),
        Some(JsonValue::String(value)) if value.trim().is_empty() => Ok(None),
        Some(JsonValue::String(value)) => Ok(Some(value.trim().to_string())),
        Some(_) => Err(AppError::InvalidInput(format!(
            "DeepSeek Harness {label} must be a string"
        ))),
    }
}

fn validate_optional_string(value: Option<&JsonValue>, label: &str) -> Result<(), AppError> {
    match value {
        None | Some(JsonValue::Null) | Some(JsonValue::String(_)) => Ok(()),
        Some(_) => Err(AppError::InvalidInput(format!(
            "DeepSeek Harness {label} must be a string"
        ))),
    }
}

fn validate_models(models: Option<&JsonValue>) -> Result<(), AppError> {
    let Some(models) = models else {
        return Ok(());
    };
    let entries = models.as_array().ok_or_else(|| {
        AppError::InvalidInput("DeepSeek Harness models must be an array".to_string())
    })?;
    let mut seen = HashSet::new();
    for (index, entry) in entries.iter().enumerate() {
        let object = entry.as_object().ok_or_else(|| {
            AppError::InvalidInput(format!(
                "DeepSeek Harness model at index {index} must be an object"
            ))
        })?;
        let id = entry
            .as_object()
            .and_then(|item| item.get("id"))
            .and_then(JsonValue::as_str)
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .ok_or_else(|| {
                AppError::InvalidInput(format!(
                    "DeepSeek Harness model at index {index} must have a non-empty id"
                ))
            })?;
        if id.chars().any(char::is_control) {
            return Err(AppError::InvalidInput(format!(
                "DeepSeek Harness model '{id}' contains control characters"
            )));
        }
        if !seen.insert(id.to_string()) {
            return Err(AppError::InvalidInput(format!(
                "DeepSeek Harness model id '{id}' is duplicated"
            )));
        }
        for key in ["name", "description"] {
            if let Some(value) = object.get(key) {
                if !value.is_string() || key == "name" && value.as_str() == Some("") {
                    return Err(AppError::InvalidInput(format!(
                        "DeepSeek Harness model '{id}' {key} must be a{} string",
                        if key == "name" { " non-empty" } else { "" }
                    )));
                }
            }
        }
        for key in ["contextWindow", "maxTokens"] {
            validate_positive_integer(object.get(key), &format!("model '{id}' {key}"))?;
        }
    }
    Ok(())
}

fn validate_positive_integer(value: Option<&JsonValue>, label: &str) -> Result<(), AppError> {
    let Some(value) = value else {
        return Ok(());
    };
    if value
        .as_u64()
        .is_none_or(|number| number == 0 || number > MAX_SAFE_INTEGER)
    {
        return Err(AppError::InvalidInput(format!(
            "DeepSeek Harness {label} must be a positive JavaScript-safe integer"
        )));
    }
    Ok(())
}

fn validate_retry_policy(value: Option<&JsonValue>) -> Result<(), AppError> {
    let Some(value) = value else {
        return Ok(());
    };
    let object = value.as_object().ok_or_else(|| {
        AppError::InvalidInput("DeepSeek Harness retryPolicy must be an object".to_string())
    })?;
    let mode = object.get("mode").and_then(JsonValue::as_str);
    let allowed: &[&str] = match mode {
        Some("normal") => &["mode", "maxRetries", "retryableCodes", "backoff"],
        Some("always") => &["mode", "backoff"],
        _ => {
            return Err(AppError::InvalidInput(
                "DeepSeek Harness retryPolicy.mode must be normal or always".to_string(),
            ))
        }
    };
    if let Some(key) = object.keys().find(|key| !allowed.contains(&key.as_str())) {
        return Err(AppError::InvalidInput(format!(
            "DeepSeek Harness retryPolicy has unknown key '{key}'"
        )));
    }
    if mode == Some("normal") {
        if let Some(retries) = object.get("maxRetries") {
            if retries
                .as_u64()
                .is_none_or(|retries| retries > MAX_SAFE_INTEGER)
            {
                return Err(AppError::InvalidInput(
                    "DeepSeek Harness retryPolicy.maxRetries must be a non-negative JavaScript-safe integer".to_string(),
                ));
            }
        }
        if let Some(codes) = object.get("retryableCodes") {
            let codes = codes
                .as_array()
                .filter(|codes| !codes.is_empty())
                .ok_or_else(|| {
                    AppError::InvalidInput(
                        "DeepSeek Harness retryPolicy.retryableCodes must be a non-empty array"
                            .to_string(),
                    )
                })?;
            let mut seen = HashSet::new();
            if codes.iter().any(|code| {
                code.as_str()
                    .filter(|code| !code.is_empty())
                    .is_none_or(|code| !seen.insert(code))
            }) {
                return Err(AppError::InvalidInput(
                    "DeepSeek Harness retryPolicy.retryableCodes must contain unique non-empty strings"
                        .to_string(),
                ));
            }
        }
    }
    if let Some(backoff) = object.get("backoff") {
        let backoff = backoff.as_object().ok_or_else(|| {
            AppError::InvalidInput(
                "DeepSeek Harness retryPolicy.backoff must be an object".to_string(),
            )
        })?;
        let allowed = ["initialDelayMs", "maxDelayMs", "jitterRatio"];
        if let Some(key) = backoff.keys().find(|key| !allowed.contains(&key.as_str())) {
            return Err(AppError::InvalidInput(format!(
                "DeepSeek Harness retryPolicy.backoff has unknown key '{key}'"
            )));
        }
        let initial = validate_timer(backoff.get("initialDelayMs"), "initialDelayMs")?;
        let maximum = validate_timer(backoff.get("maxDelayMs"), "maxDelayMs")?;
        if initial
            .zip(maximum)
            .is_some_and(|(initial, maximum)| initial > maximum)
        {
            return Err(AppError::InvalidInput(
                "DeepSeek Harness retryPolicy initialDelayMs must not exceed maxDelayMs"
                    .to_string(),
            ));
        }
        if let Some(jitter) = backoff.get("jitterRatio") {
            if jitter
                .as_f64()
                .filter(|value| value.is_finite())
                .is_none_or(|value| !(0.0..=1.0).contains(&value))
            {
                return Err(AppError::InvalidInput(
                    "DeepSeek Harness retryPolicy jitterRatio must be between 0 and 1".to_string(),
                ));
            }
        }
    }
    Ok(())
}

fn validate_timer(value: Option<&JsonValue>, label: &str) -> Result<Option<f64>, AppError> {
    let Some(value) = value else {
        return Ok(None);
    };
    let value = value.as_f64().filter(|value| value.is_finite());
    if value.is_none_or(|value| value <= 0.0 || value > i32::MAX as f64) {
        return Err(AppError::InvalidInput(format!(
            "DeepSeek Harness retryPolicy {label} must be positive and no greater than {}",
            i32::MAX
        )));
    }
    Ok(value)
}

fn validate_credential_ref(value: &str) -> Result<(), AppError> {
    let mut chars = value.chars();
    let valid_first = chars
        .next()
        .is_some_and(|ch| ch == '_' || ch.is_ascii_alphabetic());
    if !valid_first || !chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric()) {
        return Err(AppError::InvalidInput(format!(
            "DeepSeek Harness credential reference '{value}' must be a POSIX identifier"
        )));
    }
    Ok(())
}

#[derive(Debug)]
struct DshSelection {
    provider: Option<String>,
    model: Option<String>,
    reasoning_effort: Option<String>,
}

fn read_selection(root: &YamlMapping, path: &Path) -> Result<Option<DshSelection>, AppError> {
    let selection_key = YamlValue::String(DEFAULT_MODEL_SECTION.to_string());
    let Some(value) = root.get(&selection_key) else {
        return Ok(None);
    };
    let mapping = value.as_mapping().ok_or_else(|| {
        AppError::Config(format!(
            "DeepSeek Harness section '{DEFAULT_MODEL_SECTION}' must be a mapping in {}",
            path.display()
        ))
    })?;
    Ok(Some(DshSelection {
        provider: optional_yaml_string(mapping, "provider", path)?,
        model: optional_yaml_string(mapping, "model", path)?,
        reasoning_effort: optional_yaml_string(mapping, "reasoningEffort", path)?,
    }))
}

fn optional_yaml_string(
    mapping: &YamlMapping,
    key: &str,
    path: &Path,
) -> Result<Option<String>, AppError> {
    let yaml_key = YamlValue::String(key.to_string());
    match mapping.get(&yaml_key) {
        None | Some(YamlValue::Null) => Ok(None),
        Some(YamlValue::String(value)) if !value.trim().is_empty() => {
            Ok(Some(value.trim().to_string()))
        }
        Some(YamlValue::String(_)) => Ok(None),
        Some(_) => Err(AppError::Config(format!(
            "DeepSeek Harness setting '{DEFAULT_MODEL_SECTION}.{key}' must be a string in {}",
            path.display()
        ))),
    }
}

fn parse_settings_document(raw: &str, path: &Path) -> Result<YamlMapping, AppError> {
    if raw.trim().is_empty() {
        return Ok(YamlMapping::new());
    }
    let parsed: YamlValue = serde_yaml::from_str(raw).map_err(|_| {
        AppError::Config(format!(
            "Invalid DeepSeek Harness YAML document at {}",
            path.display()
        ))
    })?;
    parsed.as_mapping().cloned().ok_or_else(|| {
        AppError::Config(format!(
            "DeepSeek Harness settings at {} must be a mapping",
            path.display()
        ))
    })
}

fn yaml_to_json(value: &YamlValue, context: &str) -> Result<JsonValue, AppError> {
    serde_json::to_value(value).map_err(|e| {
        AppError::Config(format!(
            "Failed to convert DeepSeek Harness {context} to JSON: {e}"
        ))
    })
}

fn read_managed_credential(home: &Path, reference: &str) -> Result<Option<String>, AppError> {
    let path = home.join(CREDENTIALS_FILENAME);
    let Some(raw) = read_optional_private_bytes(&path)? else {
        return Ok(None);
    };
    let raw = String::from_utf8(raw).map_err(|_| {
        AppError::Config(format!(
            "DeepSeek Harness credentials at {} must be UTF-8",
            path.display()
        ))
    })?;
    let values = parse_credentials_document(&raw, &path)?;
    Ok(values
        .get(YamlValue::String(reference.to_string()))
        .and_then(YamlValue::as_str)
        .map(ToOwned::to_owned))
}

fn prepare_managed_credential(
    home: &Path,
    reference: &str,
    value: Option<&str>,
) -> Result<Option<CredentialUpdate>, AppError> {
    validate_credential_ref(reference)?;
    let path = home.join(CREDENTIALS_FILENAME);
    let previous = read_optional_private_bytes(&path)?;
    let raw = previous
        .as_deref()
        .map(|bytes| {
            std::str::from_utf8(bytes).map(str::to_owned).map_err(|_| {
                AppError::Config(format!(
                    "DeepSeek Harness credentials at {} must be UTF-8",
                    path.display()
                ))
            })
        })
        .transpose()?
        .unwrap_or_default();
    let parsed = parse_credentials_document(&raw, &path)?;
    let existing = parsed
        .get(YamlValue::String(reference.to_string()))
        .and_then(YamlValue::as_str);
    if existing == value {
        return Ok(None);
    }
    let next = match value {
        Some(value) => replace_yaml_section(
            &raw,
            &parsed,
            reference,
            &YamlValue::String(value.to_string()),
        )?,
        None => remove_yaml_section(&raw, &parsed, reference)?,
    };
    parse_credentials_document(&next, &path)?;
    Ok(Some(CredentialUpdate {
        path,
        previous,
        next: next.into_bytes(),
    }))
}

fn apply_credential_update(update: &CredentialUpdate) -> Result<(), AppError> {
    write_private_atomic(&update.path, &update.next)
}

fn rollback_credential_update(update: &CredentialUpdate) -> Result<(), AppError> {
    match update.previous.as_deref() {
        Some(previous) => write_private_atomic(&update.path, previous),
        None => remove_file_if_present(&update.path),
    }
}

fn remove_file_if_present(path: &Path) -> Result<(), AppError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(AppError::io(path, error)),
    }
}

fn verify_committed_file(path: &Path, transition: &DshFileTransition) -> Result<(), AppError> {
    let current = read_optional_bytes(path)?;
    if current != transition.committed {
        return Err(AppError::Conflict(format!(
            "DeepSeek Harness file {} changed after CC Switch wrote it; refusing to overwrite the newer contents during rollback",
            path.display()
        )));
    }
    Ok(())
}

fn restore_file_transition(
    path: &Path,
    transition: &DshFileTransition,
    private: bool,
) -> Result<(), AppError> {
    match transition.before.as_deref() {
        Some(contents) if private => write_private_atomic(path, contents),
        Some(contents) => write_settings_atomic(path, contents),
        None => remove_file_if_present(path),
    }
}

fn parse_credentials_document(raw: &str, path: &Path) -> Result<YamlMapping, AppError> {
    if raw.trim().is_empty() {
        return Ok(YamlMapping::new());
    }
    let parsed: YamlValue = serde_yaml::from_str(raw).map_err(|_| {
        // Parser diagnostics can quote the source line, which is a secret.
        AppError::Config(format!(
            "Invalid DeepSeek Harness credentials document at {}",
            path.display()
        ))
    })?;
    let mapping = parsed.as_mapping().cloned().ok_or_else(|| {
        AppError::Config(format!(
            "DeepSeek Harness credentials at {} must be a string mapping",
            path.display()
        ))
    })?;
    for (key, value) in &mapping {
        let Some(key) = key.as_str() else {
            return Err(AppError::Config(format!(
                "DeepSeek Harness credentials at {} contain a non-string reference",
                path.display()
            )));
        };
        validate_credential_ref(key)?;
        match value {
            YamlValue::String(value) if !value.is_empty() => {}
            _ => {
                return Err(AppError::Config(format!(
                    "DeepSeek Harness credential '{key}' at {} must be a non-empty string",
                    path.display()
                )))
            }
        }
    }
    Ok(mapping)
}

#[cfg(unix)]
fn assert_owner_only_metadata(path: &Path, metadata: &fs::Metadata) -> Result<(), AppError> {
    use std::os::unix::fs::PermissionsExt;
    let mode = metadata.permissions().mode();
    if mode & 0o077 != 0 {
        return Err(AppError::Config(format!(
            "DeepSeek Harness credentials file {} must have mode 0600",
            path.display()
        )));
    }
    Ok(())
}

#[cfg(not(unix))]
fn assert_owner_only_metadata(_path: &Path, _metadata: &fs::Metadata) -> Result<(), AppError> {
    Ok(())
}

fn assert_owner_only(path: &Path) -> Result<(), AppError> {
    let metadata = fs::metadata(path).map_err(|error| AppError::io(path, error))?;
    assert_owner_only_metadata(path, &metadata)
}

fn ensure_private_home(home: &Path) -> Result<(), AppError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;

        // `mode` applies only to directories this call creates. In
        // particular, do not chmod a pre-existing directory or the target of
        // a symlink supplied as DSH_HOME.
        let mut builder = fs::DirBuilder::new();
        builder.recursive(true).mode(0o700);
        builder.create(home).map_err(|e| AppError::io(home, e))?;
    }
    #[cfg(not(unix))]
    fs::create_dir_all(home).map_err(|e| AppError::io(home, e))?;
    Ok(())
}

fn write_settings_atomic(path: &Path, contents: &[u8]) -> Result<(), AppError> {
    if let Some(home) = path.parent() {
        ensure_private_home(home)?;
    }
    write_owner_only_atomic(path, contents)
}

fn write_private_atomic(path: &Path, contents: &[u8]) -> Result<(), AppError> {
    if let Some(home) = path.parent() {
        ensure_private_home(home)?;
    }
    write_owner_only_atomic(path, contents)?;
    assert_owner_only(path)
}

/// Atomically replace a Harness-owned document without ever materializing its
/// contents in a group/world-readable temporary file. Windows WSL UNC paths
/// are rejected because Windows replacement APIs cannot guarantee POSIX mode
/// 0600 there. POSIX needs an explicit mode on the initial `create_new`
/// because chmod-after-rename leaves a crash-persistent exposure window.
fn write_owner_only_atomic(path: &Path, contents: &[u8]) -> Result<(), AppError> {
    if let Some(home) = path.parent() {
        ensure_safe_live_write_path(home)?;
    }
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;

        let parent = path.parent().ok_or_else(|| {
            AppError::Config("DeepSeek Harness document path has no parent".to_string())
        })?;
        let file_name = path.file_name().ok_or_else(|| {
            AppError::Config("DeepSeek Harness document path has no filename".to_string())
        })?;
        static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();

        let mut last_collision = None;
        let (temporary, mut file) = (|| {
            for _ in 0..16 {
                let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
                let mut temporary_name = file_name.to_os_string();
                temporary_name.push(format!(".tmp.{}.{timestamp}.{counter}", std::process::id()));
                let candidate = parent.join(temporary_name);
                match fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .open(&candidate)
                {
                    Ok(file) => return Ok((candidate, file)),
                    Err(source) if source.kind() == ErrorKind::AlreadyExists => {
                        last_collision = Some((candidate, source));
                    }
                    Err(source) => return Err(AppError::io(&candidate, source)),
                }
            }

            let (candidate, source) =
                last_collision.expect("owner-only temporary filename loop must run");
            Err(AppError::io(&candidate, source))
        })()?;

        if let Err(source) = file.write_all(contents).and_then(|_| file.flush()) {
            drop(file);
            let _ = fs::remove_file(&temporary);
            return Err(AppError::io(&temporary, source));
        }
        drop(file);
        if let Err(source) = fs::rename(&temporary, path) {
            let _ = fs::remove_file(&temporary);
            return Err(AppError::IoContext {
                context: format!(
                    "Failed to atomically replace DeepSeek Harness document: {} -> {}",
                    temporary.display(),
                    path.display()
                ),
                source,
            });
        }
        Ok(())
    }

    #[cfg(not(unix))]
    atomic_write(path, contents)
}

fn is_top_level_key_line(line: &str) -> bool {
    if line.is_empty() || matches!(line.as_bytes()[0], b' ' | b'\t' | b'#' | b'-') {
        return false;
    }
    line.find(':').is_some_and(|position| {
        let after = &line[position + 1..];
        after.is_empty() || after.starts_with([' ', '\t', '\r', '\n'])
    })
}

fn line_has_key(line: &str, key: &str) -> bool {
    if !is_top_level_key_line(line) {
        return false;
    }
    let before = line[..line.find(':').expect("checked colon")].trim_end();
    before == key || before == format!("'{key}'") || before == format!("\"{key}\"")
}

fn find_yaml_section_range(raw: &str, key: &str) -> Option<(usize, usize)> {
    let mut start = None;
    let mut offset = 0;
    for line in raw.split_inclusive('\n') {
        if start.is_none() && line_has_key(line, key) {
            start = Some(offset);
        } else if let Some(section_start) = start {
            if is_top_level_key_line(line) {
                return Some((
                    section_start,
                    trailing_comment_start(raw, section_start, offset),
                ));
            }
        }
        offset += line.len();
    }
    start.map(|section_start| {
        (
            section_start,
            trailing_comment_start(raw, section_start, raw.len()),
        )
    })
}

fn trailing_comment_start(raw: &str, start: usize, boundary: usize) -> usize {
    let mut offset = start;
    let mut last_content_end = start;
    for line in raw[start..boundary].split_inclusive('\n') {
        let trimmed = line.trim();
        if !trimmed.is_empty() && !trimmed.starts_with('#') {
            last_content_end = offset + line.len();
        }
        offset += line.len();
    }
    last_content_end
}

fn serialize_yaml_section(key: &str, value: &YamlValue) -> Result<String, AppError> {
    let mut section = YamlMapping::new();
    section.insert(YamlValue::String(key.to_string()), value.clone());
    serde_yaml::to_string(&YamlValue::Mapping(section)).map_err(|e| {
        AppError::Config(format!(
            "Failed to serialize DeepSeek Harness YAML section '{key}': {e}"
        ))
    })
}

/// Patch direct children of an existing top-level mapping without rebuilding
/// the whole section. Existing child text is retained byte-for-byte when its
/// semantic value is unchanged; changed leaves/subtrees replace only their
/// own child block, new children append, and removed children are deleted.
/// This keeps comments, quoting, key order, and formatting for every untouched
/// setting. A missing section still uses the canonical serializer.
fn patch_top_level_mapping_section(
    raw: &str,
    parsed: &YamlMapping,
    section_key: &str,
    value: &YamlValue,
) -> Result<String, AppError> {
    let target = value.as_mapping().ok_or_else(|| {
        AppError::Config(format!(
            "DeepSeek Harness YAML section '{section_key}' must be a mapping"
        ))
    })?;
    let Some((section_start, section_end)) = find_yaml_section_range(raw, section_key) else {
        return replace_yaml_section(raw, parsed, section_key, value);
    };
    let existing = parsed
        .get(YamlValue::String(section_key.to_string()))
        .and_then(YamlValue::as_mapping)
        .ok_or_else(|| {
            AppError::Config(format!(
                "DeepSeek Harness YAML section '{section_key}' must be a mapping"
            ))
        })?;

    let section = &raw[section_start..section_end];
    let children = find_mapping_child_ranges(section, section_key)?;
    let existing_keys = existing
        .keys()
        .map(|key| {
            key.as_str().ok_or_else(|| {
                AppError::Config(format!(
                    "DeepSeek Harness YAML section '{section_key}' contains a non-string key"
                ))
            })
        })
        .collect::<Result<HashSet<_>, _>>()?;
    let located_keys = children
        .iter()
        .map(|child| child.key.as_str())
        .collect::<HashSet<_>>();
    if existing_keys != located_keys {
        return Err(AppError::Config(format!(
            "DeepSeek Harness YAML section '{section_key}' uses unsupported child-key formatting"
        )));
    }
    let mut rendered = section.to_string();

    // Work backwards so byte ranges remain valid. Only direct string keys are
    // supported by Harness' settings schemas; fail closed for exotic YAML.
    for child in children.iter().rev() {
        let yaml_key = YamlValue::String(child.key.clone());
        match target.get(&yaml_key) {
            Some(next_value) if existing.get(&yaml_key) == Some(next_value) => {}
            Some(next_value) => {
                let replacement = serialize_yaml_child(&child.key, next_value, child.indent)?;
                rendered.replace_range(child.key_start..child.end, &replacement);
            }
            None => rendered.replace_range(child.start..child.end, ""),
        }
    }

    let child_indent = children.first().map_or(2, |child| child.indent);
    let mut additions = String::new();
    for (key, next_value) in target {
        let key = key.as_str().ok_or_else(|| {
            AppError::Config(format!(
                "DeepSeek Harness YAML section '{section_key}' contains a non-string key"
            ))
        })?;
        if !located_keys.contains(key) {
            additions.push_str(&serialize_yaml_child(key, next_value, child_indent)?);
        }
    }
    if !additions.is_empty() {
        if !rendered.ends_with('\n') {
            rendered.push('\n');
        }
        rendered.push_str(&additions);
    }

    let mut result = String::with_capacity(raw.len() + rendered.len());
    result.push_str(&raw[..section_start]);
    result.push_str(&rendered);
    result.push_str(&raw[section_end..]);
    Ok(result)
}

#[derive(Debug)]
struct YamlChildRange {
    key: String,
    indent: usize,
    key_start: usize,
    start: usize,
    end: usize,
}

fn find_mapping_child_ranges(
    section: &str,
    section_key: &str,
) -> Result<Vec<YamlChildRange>, AppError> {
    let first_line_end = section.find('\n').map_or(section.len(), |index| index + 1);
    let header = &section[..first_line_end];
    let header_colon = header.find(':').ok_or_else(|| {
        AppError::Config(format!(
            "DeepSeek Harness YAML section '{section_key}' has no mapping delimiter"
        ))
    })?;
    if !header[header_colon + 1..].trim().is_empty() {
        return Err(AppError::Config(format!(
            "DeepSeek Harness YAML section '{section_key}' uses unsupported inline formatting"
        )));
    }
    let body = &section[first_line_end..];
    let indent = body
        .split_inclusive('\n')
        .find_map(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                return None;
            }
            let leading = line.len() - line.trim_start_matches([' ', '\t']).len();
            (leading > 0).then_some(leading)
        })
        .unwrap_or(2);

    let mut starts = Vec::<(String, usize, usize)>::new();
    let mut pending_comment_start = None;
    let mut offset = first_line_end;
    for line in body.split_inclusive('\n') {
        let trimmed = line.trim();
        if let Some(key) = mapping_key_at_indent(line, indent) {
            let key_start = offset;
            let owned_start = pending_comment_start.unwrap_or(offset);
            starts.push((key, owned_start, key_start));
            pending_comment_start = None;
        } else if trimmed.is_empty() {
            pending_comment_start = None;
        } else if trimmed.starts_with('#') {
            pending_comment_start.get_or_insert(offset);
        } else {
            pending_comment_start = None;
        }
        offset += line.len();
    }

    let mut ranges = Vec::with_capacity(starts.len());
    for (index, (key, start, key_start)) in starts.iter().enumerate() {
        if ranges
            .iter()
            .any(|range: &YamlChildRange| range.key == *key)
        {
            return Err(AppError::Config(format!(
                "DeepSeek Harness YAML section '{section_key}' contains duplicate key '{key}'"
            )));
        }
        let boundary = starts
            .get(index + 1)
            .map_or(section.len(), |(_, start, _)| *start);
        ranges.push(YamlChildRange {
            key: key.clone(),
            indent,
            key_start: *key_start,
            start: *start,
            end: trailing_comment_start(section, *start, boundary),
        });
    }
    Ok(ranges)
}

fn mapping_key_at_indent(line: &str, indent: usize) -> Option<String> {
    let without_newline = line.trim_end_matches(['\r', '\n']);
    let leading = without_newline.len() - without_newline.trim_start_matches([' ', '\t']).len();
    if leading != indent {
        return None;
    }
    let content = &without_newline[leading..];
    if content.is_empty() || content.starts_with(['#', '-']) {
        return None;
    }
    let colon = content.find(':')?;
    let after = &content[colon + 1..];
    if !after.is_empty() && !after.starts_with([' ', '\t']) {
        return None;
    }
    let raw_key = content[..colon].trim_end();
    if raw_key.is_empty() {
        return None;
    }
    if let Some(quoted) = raw_key
        .strip_prefix('\'')
        .and_then(|key| key.strip_suffix('\''))
    {
        return Some(quoted.replace("''", "'"));
    }
    if let Some(quoted) = raw_key
        .strip_prefix('"')
        .and_then(|key| key.strip_suffix('"'))
    {
        return serde_yaml::from_str::<String>(raw_key)
            .ok()
            .or_else(|| Some(quoted.to_string()));
    }
    Some(raw_key.to_string())
}

fn serialize_yaml_child(key: &str, value: &YamlValue, indent: usize) -> Result<String, AppError> {
    let serialized = serialize_yaml_section(key, value)?;
    let padding = " ".repeat(indent);
    let mut output = String::with_capacity(serialized.len() + indent * 2);
    for line in serialized.split_inclusive('\n') {
        output.push_str(&padding);
        output.push_str(line);
    }
    if !output.ends_with('\n') {
        output.push('\n');
    }
    Ok(output)
}

fn replace_yaml_section(
    raw: &str,
    parsed: &YamlMapping,
    key: &str,
    value: &YamlValue,
) -> Result<String, AppError> {
    let serialized = serialize_yaml_section(key, value)?;
    if let Some((start, end)) = find_yaml_section_range(raw, key) {
        let mut result = String::with_capacity(raw.len() + serialized.len());
        result.push_str(&raw[..start]);
        result.push_str(&serialized);
        result.push_str(&raw[end..]);
        return Ok(result);
    }

    if parsed.contains_key(YamlValue::String(key.to_string())) {
        return Err(AppError::Config(format!(
            "DeepSeek Harness YAML key '{key}' uses unsupported formatting; edit it to a plain or quoted top-level key before switching"
        )));
    }
    let mut result = raw.to_string();
    if !result.is_empty() && !result.ends_with('\n') {
        result.push('\n');
    }
    result.push_str(&serialized);
    if !result.ends_with('\n') {
        result.push('\n');
    }
    Ok(result)
}

fn remove_yaml_section(raw: &str, parsed: &YamlMapping, key: &str) -> Result<String, AppError> {
    if let Some((start, end)) = find_yaml_section_range(raw, key) {
        let mut result = String::with_capacity(raw.len() - (end - start));
        result.push_str(&raw[..start]);
        result.push_str(&raw[end..]);
        return Ok(result);
    }
    if parsed.contains_key(YamlValue::String(key.to_string())) {
        return Err(AppError::Config(format!(
            "DeepSeek Harness YAML key '{key}' uses unsupported formatting; edit it to a plain or quoted top-level key before switching"
        )));
    }
    Ok(raw.to_string())
}

fn create_settings_backup(root: &Path, source: &str) -> Result<PathBuf, AppError> {
    // Backups contain the entire settings document, including namespaces CC
    // Switch does not own and which may carry personal values. Give them the
    // same owner-only creation contract as the native document.
    ensure_private_home(root)?;
    let base = format!("settings_{}", Local::now().format("%Y%m%d_%H%M%S"));
    let mut path = root.join(format!("{base}.yaml"));
    let mut counter = 1;
    loop {
        match fs::symlink_metadata(&path) {
            Ok(_) => {
                path = root.join(format!("{base}_{counter}.yaml"));
                counter += 1;
            }
            Err(error) if error.kind() == ErrorKind::NotFound => break,
            Err(error) => return Err(AppError::io(&path, error)),
        }
    }
    write_owner_only_atomic(&path, source.as_bytes())?;
    cleanup_settings_backups(root)?;
    Ok(path)
}

fn cleanup_settings_backups(root: &Path) -> Result<(), AppError> {
    let retain = effective_backup_retain_count();
    let mut entries = fs::read_dir(root)
        .map_err(|e| AppError::io(root, e))?
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "yaml"))
        .collect::<Vec<_>>();
    if entries.len() <= retain {
        return Ok(());
    }
    entries.sort_by_key(|entry| entry.metadata().and_then(|meta| meta.modified()).ok());
    let remove_count = entries.len() - retain;
    for entry in entries.into_iter().take(remove_count) {
        if let Err(error) = fs::remove_file(entry.path()) {
            log::warn!(
                "Failed to remove old DeepSeek Harness backup {}: {error}",
                entry.path().display()
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn write_private(path: &Path, contents: &str) {
        fs::write(path, contents).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
        }
    }

    #[cfg(unix)]
    #[test]
    fn native_documents_are_owner_only_from_initial_atomic_create() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().unwrap();
        write_live_config_at(
            Some(temp.path()),
            &json!({
                "apiKey": "secret-value",
                "defaultModel": "deepseek-v4-flash"
            }),
        )
        .unwrap();

        for filename in [SETTINGS_FILENAME, CREDENTIALS_FILENAME] {
            let path = temp.path().join(filename);
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600,
                "{filename} must be created owner-only"
            );
        }
    }

    #[test]
    fn writes_native_sections_credentials_and_preserves_unrelated_text() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(
            temp.path().join(SETTINGS_FILENAME),
            "# header\ntelemetry:\n  enabled: false\n\n# keep this comment\nother: value\n",
        )
        .unwrap();
        write_private(
            &temp.path().join(CREDENTIALS_FILENAME),
            "# managed store\nOTHER_KEY: keep-me\n",
        );

        write_live_config_at(
            Some(temp.path()),
            &json!({
                "apiKey": "secret-value",
                "defaultModel": "deepseek-v4-pro",
                "defaultReasoningEffort": "max",
                "apiKeyEnv": "TEAM_DEEPSEEK_KEY",
                "baseURL": "https://gateway.example",
                "futureField": { "enabled": true }
            }),
        )
        .unwrap();

        let settings = fs::read_to_string(temp.path().join(SETTINGS_FILENAME)).unwrap();
        assert!(settings.contains("# header"));
        assert!(settings.contains("# keep this comment"));
        assert!(settings.contains("telemetry:"));
        assert!(settings.contains("futureField:"));
        assert!(!settings.contains("apiKey:"));
        assert!(!settings.contains("defaultModel:"));
        assert!(settings.contains("provider: deepseek-official"));
        assert!(settings.contains("model: deepseek-v4-pro"));
        assert!(settings.contains("reasoningEffort: max"));

        let credentials = fs::read_to_string(temp.path().join(CREDENTIALS_FILENAME)).unwrap();
        assert!(credentials.contains("# managed store"));
        assert!(credentials.contains("OTHER_KEY: keep-me"));
        let parsed =
            parse_credentials_document(&credentials, &temp.path().join(CREDENTIALS_FILENAME))
                .unwrap();
        assert_eq!(
            parsed
                .get(YamlValue::String("TEAM_DEEPSEEK_KEY".to_string()))
                .and_then(YamlValue::as_str),
            Some("secret-value")
        );
    }

    #[test]
    fn reads_snapshot_without_exposing_environment_only_secret() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(
            temp.path().join(SETTINGS_FILENAME),
            "llm-deepseek:\n  apiKeyEnv: TEST_ENV_ONLY_KEY\n  baseURL: https://api.example\nagent-default-model:\n  provider: deepseek-official\n  model: model-a\n",
        )
        .unwrap();
        std::env::set_var("TEST_ENV_ONLY_KEY", "must-not-leak");
        let snapshot = read_live_config_at(Some(temp.path())).unwrap().unwrap();
        std::env::remove_var("TEST_ENV_ONLY_KEY");

        assert_eq!(snapshot["defaultModel"], "model-a");
        assert_eq!(snapshot["baseURL"], "https://api.example");
        assert!(snapshot.get("apiKey").is_none());
    }

    #[test]
    fn reads_provider_only_override_with_bundled_default_model() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(
            temp.path().join(SETTINGS_FILENAME),
            "llm-deepseek:\n  baseURL: https://api.example\nagent-default-model:\n  provider: deepseek-official\n",
        )
        .unwrap();

        let snapshot = read_live_config_at(Some(temp.path())).unwrap().unwrap();

        assert_eq!(snapshot["defaultModel"], DEFAULT_MODEL_ID);
        assert_eq!(snapshot["baseURL"], "https://api.example");
    }

    #[test]
    fn reads_model_only_override_with_bundled_official_provider() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(
            temp.path().join(SETTINGS_FILENAME),
            "agent-default-model:\n  model: private-model\n",
        )
        .unwrap();

        let snapshot = read_live_config_at(Some(temp.path())).unwrap().unwrap();

        assert_eq!(snapshot["defaultModel"], "private-model");
    }

    #[test]
    fn reads_provider_override_without_selection_from_bundled_base() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(
            temp.path().join(SETTINGS_FILENAME),
            "llm-deepseek:\n  baseURL: https://api.example\n",
        )
        .unwrap();

        let snapshot = read_live_config_at(Some(temp.path())).unwrap().unwrap();

        assert_eq!(snapshot["defaultModel"], DEFAULT_MODEL_ID);
        assert_eq!(snapshot["baseURL"], "https://api.example");
    }

    #[test]
    fn read_round_trips_managed_secret_and_unknown_profile_fields() {
        let temp = tempfile::tempdir().unwrap();
        let input = json!({
            "apiKey": "stored-secret",
            "defaultModel": "private-model",
            "apiKeyEnv": "TEST_MANAGED_KEY",
            "models": [{ "id": "private-model", "description": "custom" }],
            "newHarnessOption": [1, 2, 3]
        });
        write_live_config_at(Some(temp.path()), &input).unwrap();
        let actual = read_live_config_at(Some(temp.path())).unwrap().unwrap();
        assert_eq!(actual, input);
        assert_eq!(
            get_current_model_at(Some(temp.path())).unwrap().as_deref(),
            Some("private-model")
        );
    }

    #[test]
    fn replacing_owned_sections_preserves_unrelated_sections_and_comments() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(
            temp.path().join(SETTINGS_FILENAME),
            "llm-deepseek:\n  baseURL: https://old.example\n\n# belongs to unrelated section\nlogging:\n  level: debug\nagent-default-model:\n  provider: another-route\n  model: old\n# trailing note\n",
        )
        .unwrap();
        write_live_config_at(
            Some(temp.path()),
            &json!({
                "defaultModel": "new-model",
                "baseURL": "https://new.example"
            }),
        )
        .unwrap();
        let actual = fs::read_to_string(temp.path().join(SETTINGS_FILENAME)).unwrap();
        assert!(actual.contains("# belongs to unrelated section\nlogging:"));
        assert!(actual.contains("level: debug"));
        assert!(actual.contains("# trailing note"));
        assert!(!actual.contains("https://old.example"));
        assert!(!actual.contains("model: old"));
    }

    #[test]
    fn patches_owned_children_without_reformatting_unchanged_keys_or_comments() {
        let temp = tempfile::tempdir().unwrap();
        let original = concat!(
            "llm-deepseek:\n",
            "  # endpoint chosen by ops\n",
            "  baseURL: 'https://old.example' # replace this leaf\n",
            "  maxTokens: 0x3e800 # preserve numeric formatting\n",
            "  models: [ { id: deepseek-v4-pro } ] # preserve flow style\n",
            "  # preserve this retry note\n",
            "  retryPolicy: { mode: always }\n",
            "agent-default-model:\n",
            "  provider: 'deepseek-official' # keep provider formatting\n",
            "  # selected deployment\n",
            "  model: 'old-model'\n",
            "  reasoningEffort: max # unchanged leaf\n",
        );
        fs::write(temp.path().join(SETTINGS_FILENAME), original).unwrap();

        write_live_config_at(
            Some(temp.path()),
            &json!({
                "defaultModel": "new-model",
                "defaultReasoningEffort": "max",
                "baseURL": "https://new.example",
                "maxTokens": 256000,
                "models": [{ "id": "deepseek-v4-pro" }],
                "retryPolicy": { "mode": "always" }
            }),
        )
        .unwrap();

        let actual = fs::read_to_string(temp.path().join(SETTINGS_FILENAME)).unwrap();
        assert!(actual.contains("  # endpoint chosen by ops\n"));
        assert!(actual.contains("  maxTokens: 0x3e800 # preserve numeric formatting\n"));
        assert!(actual.contains("  models: [ { id: deepseek-v4-pro } ] # preserve flow style\n"));
        assert!(actual.contains("  # preserve this retry note\n"));
        assert!(actual.contains("  retryPolicy: { mode: always }\n"));
        assert!(actual.contains("  provider: 'deepseek-official' # keep provider formatting\n"));
        assert!(actual.contains("  # selected deployment\n"));
        assert!(actual.contains("  reasoningEffort: max # unchanged leaf\n"));
        assert!(!actual.contains("https://old.example"));
        assert!(!actual.contains("model: 'old-model'"));
    }

    #[test]
    fn keep_credential_action_does_not_wait_for_credentials_lock() {
        let temp = tempfile::tempdir().unwrap();
        ensure_private_home(temp.path()).unwrap();
        let credentials_lock = acquire_file_lock(&temp.path().join(CREDENTIALS_FILENAME)).unwrap();

        write_live_config_at(
            Some(temp.path()),
            &json!({ "defaultModel": "deepseek-v4-flash" }),
        )
        .expect("settings-only writes must not acquire the credentials lock");

        drop(credentials_lock);
        assert!(temp.path().join(SETTINGS_FILENAME).exists());
    }

    #[test]
    fn rejects_invalid_model_catalog_without_touching_files() {
        let temp = tempfile::tempdir().unwrap();
        let result = write_live_config_at(
            Some(temp.path()),
            &json!({
                "defaultModel": "model-a",
                "models": [{ "id": "model-a" }, { "id": "model-a" }]
            }),
        );
        assert!(result.is_err());
        assert!(!temp.path().join(SETTINGS_FILENAME).exists());
        assert!(!temp.path().join(CREDENTIALS_FILENAME).exists());
    }

    #[test]
    fn rejects_invalid_retry_policy_and_unsafe_integers_before_writing() {
        let temp = tempfile::tempdir().unwrap();
        for input in [
            json!({
                "defaultModel": "model-a",
                "maxTokens": MAX_SAFE_INTEGER + 1
            }),
            json!({
                "defaultModel": "model-a",
                "retryPolicy": { "mode": "normal", "maxRetries": -1 }
            }),
            json!({
                "defaultModel": "model-a",
                "retryPolicy": {
                    "mode": "normal",
                    "backoff": { "initialDelayMs": 20, "maxDelayMs": 10 }
                }
            }),
        ] {
            assert!(write_live_config_at(Some(temp.path()), &input).is_err());
        }
        assert!(!temp.path().join(SETTINGS_FILENAME).exists());
        assert!(!temp.path().join(CREDENTIALS_FILENAME).exists());
    }

    #[test]
    fn current_model_is_none_when_another_provider_is_selected() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(
            temp.path().join(SETTINGS_FILENAME),
            "llm-deepseek:\n  baseURL: https://api.example\nagent-default-model:\n  provider: custom-route\n  model: other-model\n",
        )
        .unwrap();
        assert_eq!(get_current_model_at(Some(temp.path())).unwrap(), None);
        assert_eq!(read_live_config_at(Some(temp.path())).unwrap(), None);
    }

    #[test]
    fn profile_reasoning_effort_does_not_materialize_default_selection_effort() {
        let temp = tempfile::tempdir().unwrap();
        write_live_config_at(
            Some(temp.path()),
            &json!({
                "defaultModel": "deepseek-v4-pro",
                "reasoningEffort": "max"
            }),
        )
        .unwrap();

        let settings = fs::read_to_string(temp.path().join(SETTINGS_FILENAME)).unwrap();
        assert_eq!(settings.matches("reasoningEffort: max").count(), 1);
        let selection = settings
            .split("agent-default-model:")
            .nth(1)
            .expect("default-model selection");
        assert!(!selection.contains("reasoningEffort:"));
    }

    #[test]
    fn disabled_thinking_rejects_non_off_effort_before_writing() {
        let temp = tempfile::tempdir().unwrap();
        let result = write_live_config_at(
            Some(temp.path()),
            &json!({
                "defaultModel": "deepseek-v4-pro",
                "thinking": "disabled",
                "reasoningEffort": "high"
            }),
        );
        assert!(result.is_err());
        assert!(!temp.path().join(SETTINGS_FILENAME).exists());
    }

    #[test]
    fn empty_api_key_unsets_only_the_managed_reference() {
        let temp = tempfile::tempdir().unwrap();
        write_private(
            &temp.path().join(CREDENTIALS_FILENAME),
            "DEEPSEEK_API_KEY: old-secret\nOTHER_KEY: keep-me\n",
        );

        write_live_config_at(
            Some(temp.path()),
            &json!({
                "apiKey": "",
                "defaultModel": "deepseek-v4-flash"
            }),
        )
        .unwrap();

        let credentials = fs::read_to_string(temp.path().join(CREDENTIALS_FILENAME)).unwrap();
        assert!(!credentials.contains("DEEPSEEK_API_KEY"));
        assert!(credentials.contains("OTHER_KEY: keep-me"));
    }

    #[test]
    fn api_key_normalization_matches_harness_credential_rules() {
        let padded = prepare_config(&json!({
            "apiKey": "  sk-valid_ascii-123  ",
            "defaultModel": "deepseek-v4-flash"
        }))
        .unwrap();
        assert!(matches!(
            padded.credential_action,
            CredentialAction::Set(ref value) if value == "sk-valid_ascii-123"
        ));
        let debug = format!("{padded:?}");
        assert!(debug.contains("Set([REDACTED])"));
        assert!(!debug.contains("sk-valid_ascii-123"));

        let bom_padded = prepare_config(&json!({
            "apiKey": "\u{feff}sk-bom-trimmed\u{feff}",
            "defaultModel": "deepseek-v4-flash"
        }))
        .unwrap();
        assert!(matches!(
            bom_padded.credential_action,
            CredentialAction::Set(ref value) if value == "sk-bom-trimmed"
        ));

        let whitespace = prepare_config(&json!({
            "apiKey": " \t\r\n ",
            "defaultModel": "deepseek-v4-flash"
        }))
        .unwrap();
        assert!(matches!(
            whitespace.credential_action,
            CredentialAction::Unset
        ));

        for invalid in ["sk embedded-space", "密钥", "sk\u{007f}", "\u{0085}"] {
            let error = prepare_config(&json!({
                "apiKey": invalid,
                "defaultModel": "deepseek-v4-flash"
            }))
            .unwrap_err();
            assert!(!error.to_string().contains(invalid));
        }

        let temp = tempfile::tempdir().unwrap();
        assert!(write_live_config_at(
            Some(temp.path()),
            &json!({
                "apiKey": "\u{0085}",
                "defaultModel": "deepseek-v4-flash"
            }),
        )
        .is_err());
        assert_eq!(
            read_optional_bytes(&temp.path().join(CREDENTIALS_FILENAME)).unwrap(),
            None
        );
        assert_eq!(
            read_optional_bytes(&temp.path().join(SETTINGS_FILENAME)).unwrap(),
            None
        );

        let invalid_ref_home = tempfile::tempdir().unwrap();
        assert!(write_live_config_at(
            Some(invalid_ref_home.path()),
            &json!({
                "apiKey": "sk-valid",
                "apiKeyEnv": "\u{0085}VICTIM_ENV",
                "defaultModel": "deepseek-v4-flash"
            }),
        )
        .is_err());
        assert_eq!(
            read_optional_bytes(&invalid_ref_home.path().join(CREDENTIALS_FILENAME)).unwrap(),
            None
        );
        assert_eq!(
            read_optional_bytes(&invalid_ref_home.path().join(SETTINGS_FILENAME)).unwrap(),
            None
        );
    }

    #[test]
    fn accepts_low_reasoning_effort_in_profile_and_default_selection() {
        let temp = tempfile::tempdir().unwrap();
        write_live_config_at(
            Some(temp.path()),
            &json!({
                "defaultModel": "deepseek-v4-pro",
                "reasoningEffort": "low",
                "defaultReasoningEffort": "low"
            }),
        )
        .unwrap();

        let settings = fs::read_to_string(temp.path().join(SETTINGS_FILENAME)).unwrap();
        assert_eq!(settings.matches("reasoningEffort: low").count(), 2);
        let live = read_live_config_at(Some(temp.path())).unwrap().unwrap();
        assert_eq!(live["reasoningEffort"], "low");
        assert_eq!(live["defaultReasoningEffort"], "low");
    }

    #[test]
    fn rejects_non_string_base_url_before_touching_files() {
        let temp = tempfile::tempdir().unwrap();
        let input = json!({
            "defaultModel": "deepseek-v4-flash",
            "baseURL": ["https://api.example"]
        });

        assert!(validate_settings_config(&input).is_err());
        assert!(write_live_config_at(Some(temp.path()), &input).is_err());
        assert_eq!(
            read_optional_bytes(&temp.path().join(SETTINGS_FILENAME)).unwrap(),
            None
        );
        assert_eq!(
            read_optional_bytes(&temp.path().join(CREDENTIALS_FILENAME)).unwrap(),
            None
        );
    }

    #[test]
    fn static_validation_ignores_runtime_env_shadow_but_live_write_rejects_it() {
        let temp = tempfile::tempdir().unwrap();
        let input = json!({
            "apiKey": "managed-secret",
            "apiKeyEnv": "TEST_STATIC_VALIDATION_ENV_SHADOW",
            "defaultModel": "deepseek-v4-flash"
        });
        std::env::set_var("TEST_STATIC_VALIDATION_ENV_SHADOW", "inherited-secret");
        let validation = validate_settings_config(&input);
        let write = write_live_config_at(Some(temp.path()), &input);
        std::env::remove_var("TEST_STATIC_VALIDATION_ENV_SHADOW");

        assert!(validation.is_ok());
        assert!(write.is_err());
        assert_eq!(
            read_optional_bytes(&temp.path().join(SETTINGS_FILENAME)).unwrap(),
            None
        );
    }

    #[test]
    fn optional_file_read_only_treats_not_found_as_missing() {
        let temp = tempfile::tempdir().unwrap();
        let missing = temp.path().join("missing");
        assert_eq!(read_optional_string(&missing).unwrap(), None);

        let directory = temp.path().join("not-a-file");
        fs::create_dir(&directory).unwrap();
        assert!(read_optional_string(&directory).is_err());
        assert!(read_optional_bytes(&directory).is_err());
    }

    #[test]
    fn rejects_endpoint_and_key_generation_change_under_same_reference() {
        let temp = tempfile::tempdir().unwrap();
        let settings_path = temp.path().join(SETTINGS_FILENAME);
        let credentials_path = temp.path().join(CREDENTIALS_FILENAME);
        let original_settings = concat!(
            "llm-deepseek:\n",
            "  apiKeyEnv: TEST_ATOMIC_SAME_REF\n",
            "  baseURL: https://old.example\n",
            "agent-default-model:\n",
            "  provider: deepseek-official\n",
            "  model: old-model\n",
        );
        let original_credentials = "TEST_ATOMIC_SAME_REF: old-secret\n";
        fs::write(&settings_path, original_settings).unwrap();
        write_private(&credentials_path, original_credentials);

        let error = write_live_config_at(
            Some(temp.path()),
            &json!({
                "apiKey": "new-secret",
                "apiKeyEnv": "TEST_ATOMIC_SAME_REF",
                "baseURL": "https://new.example",
                "defaultModel": "new-model"
            }),
        )
        .unwrap_err();

        assert!(error.to_string().contains("same credential reference"));
        assert_eq!(
            fs::read(&settings_path).unwrap(),
            original_settings.as_bytes()
        );
        assert_eq!(
            fs::read(&credentials_path).unwrap(),
            original_credentials.as_bytes()
        );
    }

    #[test]
    fn switches_endpoint_and_key_generation_through_a_new_reference() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(
            temp.path().join(SETTINGS_FILENAME),
            concat!(
                "llm-deepseek:\n",
                "  apiKeyEnv: TEST_ATOMIC_OLD_REF\n",
                "  baseURL: https://old.example\n",
                "agent-default-model:\n",
                "  provider: deepseek-official\n",
                "  model: old-model\n",
            ),
        )
        .unwrap();
        write_private(
            &temp.path().join(CREDENTIALS_FILENAME),
            "TEST_ATOMIC_OLD_REF: old-secret\n",
        );

        let outcome = write_live_config_at(
            Some(temp.path()),
            &json!({
                "apiKey": "new-secret",
                "apiKeyEnv": "TEST_ATOMIC_NEW_REF",
                "baseURL": "https://new.example",
                "defaultModel": "new-model"
            }),
        )
        .unwrap();

        let settings = fs::read_to_string(temp.path().join(SETTINGS_FILENAME)).unwrap();
        assert!(settings.contains("apiKeyEnv: TEST_ATOMIC_NEW_REF"));
        assert!(settings.contains("baseURL: https://new.example"));
        let credentials = fs::read_to_string(temp.path().join(CREDENTIALS_FILENAME)).unwrap();
        assert!(credentials.contains("TEST_ATOMIC_OLD_REF: old-secret"));
        assert!(credentials.contains("TEST_ATOMIC_NEW_REF: new-secret"));
        assert!(outcome.rollback_token.is_some());
        assert!(!format!("{outcome:?}").contains("new-secret"));
    }

    #[test]
    fn allows_endpoint_change_when_same_reference_keeps_identical_key_bytes() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(
            temp.path().join(SETTINGS_FILENAME),
            concat!(
                "llm-deepseek:\n",
                "  apiKeyEnv: TEST_ATOMIC_STABLE_REF\n",
                "  baseURL: https://old.example\n",
                "agent-default-model:\n",
                "  provider: deepseek-official\n",
                "  model: old-model\n",
            ),
        )
        .unwrap();
        write_private(
            &temp.path().join(CREDENTIALS_FILENAME),
            "TEST_ATOMIC_STABLE_REF: stable-secret\n",
        );

        let outcome = write_live_config_at(
            Some(temp.path()),
            &json!({
                "apiKey": "stable-secret",
                "apiKeyEnv": "TEST_ATOMIC_STABLE_REF",
                "baseURL": "https://new.example",
                "defaultModel": "new-model"
            }),
        )
        .unwrap();

        assert!(fs::read_to_string(temp.path().join(SETTINGS_FILENAME))
            .unwrap()
            .contains("baseURL: https://new.example"));
        assert!(outcome
            .rollback_token
            .as_ref()
            .is_some_and(|token| token.credentials.is_none()));
    }

    #[test]
    fn rollback_token_restores_exact_bytes_and_absence() {
        let temp = tempfile::tempdir().unwrap();
        let settings_path = temp.path().join(SETTINGS_FILENAME);
        let credentials_path = temp.path().join(CREDENTIALS_FILENAME);
        let original_settings = concat!(
            "# exact settings bytes\n",
            "llm-deepseek:\n",
            "  apiKeyEnv: TEST_ROLLBACK_OLD_REF\n",
            "  baseURL: https://old.example\n",
            "agent-default-model:\n",
            "  provider: deepseek-official\n",
            "  model: old-model\n",
        );
        let original_credentials = concat!(
            "# exact credential bytes\n",
            "TEST_ROLLBACK_OLD_REF: old-secret\n",
        );
        fs::write(&settings_path, original_settings).unwrap();
        write_private(&credentials_path, original_credentials);

        let outcome = write_live_config_at(
            Some(temp.path()),
            &json!({
                "apiKey": "new-secret",
                "apiKeyEnv": "TEST_ROLLBACK_NEW_REF",
                "baseURL": "https://new.example",
                "defaultModel": "new-model"
            }),
        )
        .unwrap();
        let token = outcome.rollback_token.unwrap();
        token.restore().unwrap();

        assert_eq!(
            fs::read(&settings_path).unwrap(),
            original_settings.as_bytes()
        );
        assert_eq!(
            fs::read(&credentials_path).unwrap(),
            original_credentials.as_bytes()
        );
        assert!(matches!(token.restore(), Err(AppError::Conflict(_))));

        let empty = tempfile::tempdir().unwrap();
        let create_outcome = write_live_config_at(
            Some(empty.path()),
            &json!({ "defaultModel": "deepseek-v4-flash" }),
        )
        .unwrap();
        create_outcome.rollback_token.unwrap().restore().unwrap();
        assert_eq!(
            read_optional_bytes(&empty.path().join(SETTINGS_FILENAME)).unwrap(),
            None
        );
    }

    #[test]
    fn rollback_of_unset_restores_settings_before_target_credential() {
        let temp = tempfile::tempdir().unwrap();
        let settings_path = temp.path().join(SETTINGS_FILENAME);
        let credentials_path = temp.path().join(CREDENTIALS_FILENAME);
        let original_settings = concat!(
            "llm-deepseek:\n",
            "  apiKeyEnv: TEST_UNSET_OLD_REF\n",
            "  baseURL: https://old.example\n",
            "agent-default-model:\n",
            "  provider: deepseek-official\n",
            "  model: old-model\n",
        );
        let original_credentials = concat!(
            "TEST_UNSET_OLD_REF: old-secret\n",
            "TEST_UNSET_TARGET_REF: target-secret\n",
        );
        fs::write(&settings_path, original_settings).unwrap();
        write_private(&credentials_path, original_credentials);

        let outcome = write_live_config_at(
            Some(temp.path()),
            &json!({
                "apiKey": "",
                "apiKeyEnv": "TEST_UNSET_TARGET_REF",
                "baseURL": "https://new.example",
                "defaultModel": "new-model"
            }),
        )
        .unwrap();
        assert!(!fs::read_to_string(&credentials_path)
            .unwrap()
            .contains("TEST_UNSET_TARGET_REF"));

        outcome.rollback_token.unwrap().restore().unwrap();
        assert_eq!(
            fs::read(&settings_path).unwrap(),
            original_settings.as_bytes()
        );
        assert_eq!(
            fs::read(&credentials_path).unwrap(),
            original_credentials.as_bytes()
        );
    }

    #[test]
    fn safe_restore_order_short_circuits_before_credentials_on_settings_error() {
        use std::cell::Cell;

        let credential_restore_called = Cell::new(false);
        let result = restore_settings_then_credentials(
            || Err(AppError::Message("settings restore failed".to_string())),
            || {
                credential_restore_called.set(true);
                Ok(())
            },
        );

        assert!(result.is_err());
        assert!(!credential_restore_called.get());
    }

    #[test]
    fn rollback_token_preserves_newer_harness_edits_without_partial_restore() {
        let temp = tempfile::tempdir().unwrap();
        let settings_path = temp.path().join(SETTINGS_FILENAME);
        let credentials_path = temp.path().join(CREDENTIALS_FILENAME);
        fs::write(
            &settings_path,
            concat!(
                "llm-deepseek:\n",
                "  apiKeyEnv: TEST_CAS_OLD_REF\n",
                "  baseURL: https://old.example\n",
                "agent-default-model:\n",
                "  provider: deepseek-official\n",
                "  model: old-model\n",
            ),
        )
        .unwrap();
        write_private(&credentials_path, "TEST_CAS_OLD_REF: old-secret\n");

        let outcome = write_live_config_at(
            Some(temp.path()),
            &json!({
                "apiKey": "new-secret",
                "apiKeyEnv": "TEST_CAS_NEW_REF",
                "baseURL": "https://new.example",
                "defaultModel": "new-model"
            }),
        )
        .unwrap();
        let committed_credentials = fs::read(&credentials_path).unwrap();
        let mut newer_settings = fs::read(&settings_path).unwrap();
        newer_settings.extend_from_slice(b"# Harness changed this after CC Switch\n");
        fs::write(&settings_path, &newer_settings).unwrap();

        let error = outcome.rollback_token.unwrap().restore().unwrap_err();
        assert!(matches!(error, AppError::Conflict(_)));
        assert_eq!(fs::read(&settings_path).unwrap(), newer_settings);
        assert_eq!(fs::read(&credentials_path).unwrap(), committed_credentials);
    }

    #[test]
    fn explicit_lock_release_surfaces_removal_errors() {
        let temp = tempfile::tempdir().unwrap();
        let document = temp.path().join("document.yaml");
        let lock = acquire_file_lock(&document).unwrap();
        let lock_path = lock.path.as_ref().unwrap().clone();
        fs::remove_file(&lock_path).unwrap();
        fs::create_dir(&lock_path).unwrap();

        assert!(lock.release().is_err());
        fs::remove_dir(&lock_path).unwrap();
    }

    #[test]
    fn committed_operation_keeps_rollback_outcome_when_lock_release_fails() {
        let temp = tempfile::tempdir().unwrap();
        let document = temp.path().join("document.yaml");
        let lock = acquire_file_lock(&document).unwrap();
        let lock_path = lock.path.as_ref().unwrap().clone();
        fs::remove_file(&lock_path).unwrap();
        fs::create_dir(&lock_path).unwrap();

        assert_eq!(finish_locked(Ok(42), None, lock).unwrap(), 42);
        fs::remove_dir(&lock_path).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn private_home_creation_does_not_chmod_existing_or_symlinked_directories() {
        use std::os::unix::fs::{symlink, PermissionsExt};

        let temp = tempfile::tempdir().unwrap();
        let existing = temp.path().join("existing");
        fs::create_dir(&existing).unwrap();
        fs::set_permissions(&existing, fs::Permissions::from_mode(0o750)).unwrap();
        ensure_private_home(&existing).unwrap();
        assert_eq!(
            fs::metadata(&existing).unwrap().permissions().mode() & 0o777,
            0o750
        );

        let target = temp.path().join("target");
        let linked = temp.path().join("linked");
        fs::create_dir(&target).unwrap();
        fs::set_permissions(&target, fs::Permissions::from_mode(0o750)).unwrap();
        symlink(&target, &linked).unwrap();
        ensure_private_home(&linked).unwrap();
        assert_eq!(
            fs::metadata(&target).unwrap().permissions().mode() & 0o777,
            0o750
        );

        let created = temp.path().join("created");
        ensure_private_home(&created).unwrap();
        assert_eq!(
            fs::metadata(&created).unwrap().permissions().mode() & 0o077,
            0
        );
    }

    #[cfg(unix)]
    #[test]
    fn credential_permissions_are_checked_before_secret_bytes_are_parsed() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join(CREDENTIALS_FILENAME);
        fs::write(&path, [0xff, 0xfe, 0xfd]).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();

        let read_error = read_managed_credential(temp.path(), DEFAULT_API_KEY_ENV).unwrap_err();
        let prepare_error =
            prepare_managed_credential(temp.path(), DEFAULT_API_KEY_ENV, Some("replacement"))
                .err()
                .expect("wide credential permissions must be rejected");
        assert!(read_error.to_string().contains("mode 0600"));
        assert!(prepare_error.to_string().contains("mode 0600"));

        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        assert!(read_managed_credential(temp.path(), DEFAULT_API_KEY_ENV)
            .unwrap_err()
            .to_string()
            .contains("UTF-8"));
    }

    #[cfg(windows)]
    #[test]
    fn keep_write_fails_closed_for_wsl_unc_before_filesystem_access() {
        for home in [
            Path::new(r"\\wsl$\DefinitelyMissing\home\user\.dsh"),
            Path::new(r"\\wsl.localhost\DefinitelyMissing\home\user\.dsh"),
            Path::new(r"\\?\UNC\wsl.localhost\DefinitelyMissing\home\user\.dsh"),
        ] {
            let error = write_live_config_inner(
                home,
                &json!({ "defaultModel": "deepseek-v4-flash" }),
                None,
            )
            .unwrap_err();
            assert!(error.to_string().contains("WSL UNC"));
        }
    }
}
