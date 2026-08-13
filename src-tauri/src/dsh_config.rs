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
}

#[derive(Debug)]
struct PreparedConfig {
    profile: JsonValue,
    default_model: String,
    default_reasoning_effort: Option<String>,
    credential_action: CredentialAction,
    api_key_env: String,
}

#[derive(Debug)]
enum CredentialAction {
    Keep,
    Set(String),
    Unset,
}

#[derive(Debug)]
struct CredentialUpdate {
    path: PathBuf,
    previous: Option<Vec<u8>>,
    next: Vec<u8>,
}

/// Cross-process writer lock compatible with DeepSeek Harness' `<file>.lock`
/// protocol. Harness never removes a contender's lock, so neither do we.
struct DshFileLock {
    path: PathBuf,
}

impl Drop for DshFileLock {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_file(&self.path) {
            if error.kind() != ErrorKind::NotFound {
                log::warn!(
                    "Failed to release DeepSeek Harness writer lock {}: {error}",
                    self.path.display()
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
                return Ok(DshFileLock { path: lock_path });
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

/// Read the native DeepSeek profile currently represented by Harness user
/// settings. Environment-only credentials are intentionally never returned.
pub fn read_live_settings() -> Result<Option<JsonValue>, AppError> {
    read_live_config_at(Some(&get_dsh_dir()))
}

/// Write a native DeepSeek profile and make it the default Harness selection.
pub fn write_live_settings(settings_config: &JsonValue) -> Result<DshWriteOutcome, AppError> {
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
    if !settings_path.exists() {
        return Ok(None);
    }

    let raw = fs::read_to_string(&settings_path).map_err(|e| AppError::io(&settings_path, e))?;
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
    if !path.exists() {
        return Ok(None);
    }
    let raw = fs::read_to_string(&path).map_err(|e| AppError::io(&path, e))?;
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
    let _guard = dsh_write_lock().lock()?;

    let path = home.join(SETTINGS_FILENAME);
    let credentials_path = home.join(CREDENTIALS_FILENAME);
    ensure_private_home(home)?;
    // A settings-only switch must not contend with Harness' credential store.
    // Set/Unset operations still acquire both native locks in stable path
    // order so separate CC Switch processes cannot deadlock each other.
    let (_credentials_lock, _settings_lock): (Option<DshFileLock>, DshFileLock) =
        match &prepared.credential_action {
            CredentialAction::Keep => (None, acquire_file_lock(&path)?),
            CredentialAction::Set(_) | CredentialAction::Unset if credentials_path < path => (
                Some(acquire_file_lock(&credentials_path)?),
                acquire_file_lock(&path)?,
            ),
            CredentialAction::Set(_) | CredentialAction::Unset => {
                let settings_lock = acquire_file_lock(&path)?;
                let credentials_lock = acquire_file_lock(&credentials_path)?;
                (Some(credentials_lock), settings_lock)
            }
        };

    // The settings read is protected by its native writer lock. Credential
    // reads below occur only for Set/Unset, while the credential lock is held.
    let raw = if path.exists() {
        fs::read_to_string(&path).map_err(|e| AppError::io(&path, e))?
    } else {
        String::new()
    };
    let root = parse_settings_document(&raw, &path)?;

    let llm_yaml = serde_yaml::to_value(&prepared.profile)
        .map_err(|e| AppError::Config(format!("Failed to serialize DeepSeek profile: {e}")))?;
    let mut selection = YamlMapping::new();
    selection.insert(
        YamlValue::String("provider".to_string()),
        YamlValue::String(DSH_PROVIDER_ID.to_string()),
    );
    selection.insert(
        YamlValue::String("model".to_string()),
        YamlValue::String(prepared.default_model),
    );
    if let Some(effort) = prepared.default_reasoning_effort {
        selection.insert(
            YamlValue::String("reasoningEffort".to_string()),
            YamlValue::String(effort),
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

    // Validate and render both documents before changing either one. The
    // credential is committed first because it is reversible; a settings
    // failure restores the exact previous credential bytes.
    let credential_update = match &prepared.credential_action {
        CredentialAction::Keep => None,
        CredentialAction::Set(api_key) => {
            prepare_managed_credential(home, &prepared.api_key_env, Some(api_key))?
        }
        CredentialAction::Unset => prepare_managed_credential(home, &prepared.api_key_env, None)?,
    };

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
    Ok(DshWriteOutcome {
        backup_path: backup_path.map(|p| p.display().to_string()),
    })
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
        if !matches!(effort, "off" | "high" | "max") {
            return Err(AppError::InvalidInput(
                "DeepSeek Harness default reasoning effort must be off, high, or max".to_string(),
            ));
        }
    }

    let credential_action = match profile.remove("apiKey") {
        None | Some(JsonValue::Null) => CredentialAction::Keep,
        Some(JsonValue::String(value)) if value.is_empty() => CredentialAction::Unset,
        Some(JsonValue::String(value)) => {
            if value.chars().any(char::is_control) {
                return Err(AppError::InvalidInput(
                    "DeepSeek Harness API key must not contain control characters".to_string(),
                ));
            }
            CredentialAction::Set(value)
        }
        Some(_) => {
            return Err(AppError::InvalidInput(
                "DeepSeek Harness apiKey must be a string".to_string(),
            ))
        }
    };

    let api_key_env = match profile.get("apiKeyEnv") {
        None => DEFAULT_API_KEY_ENV.to_string(),
        Some(JsonValue::String(value)) => value.trim().to_string(),
        Some(_) => {
            return Err(AppError::InvalidInput(
                "DeepSeek Harness apiKeyEnv must be a string".to_string(),
            ))
        }
    };
    validate_credential_ref(&api_key_env)?;
    if !matches!(credential_action, CredentialAction::Keep)
        && std::env::var_os(&api_key_env).is_some_and(|value| !value.is_empty())
    {
        return Err(AppError::InvalidInput(format!(
            "DeepSeek Harness credential '{api_key_env}' is supplied read-only by the launching environment; unset it before switching"
        )));
    }
    if profile.contains_key("apiKeyEnv") {
        profile.insert(
            "apiKeyEnv".to_string(),
            JsonValue::String(api_key_env.clone()),
        );
    }

    let profile_reasoning_effort = match profile.get("reasoningEffort") {
        None | Some(JsonValue::Null) => None,
        Some(JsonValue::String(value)) if matches!(value.as_str(), "off" | "high" | "max") => {
            Some(value.as_str())
        }
        Some(JsonValue::String(_)) => {
            return Err(AppError::InvalidInput(
                "DeepSeek Harness reasoningEffort must be off, high, or max".to_string(),
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
    if !path.exists() {
        return Ok(None);
    }
    assert_owner_only(&path)?;
    let raw = fs::read_to_string(&path).map_err(|e| AppError::io(&path, e))?;
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
    let previous = if path.exists() {
        assert_owner_only(&path)?;
        Some(fs::read(&path).map_err(|e| AppError::io(&path, e))?)
    } else {
        None
    };
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
        None => {
            if update.path.exists() {
                fs::remove_file(&update.path).map_err(|e| AppError::io(&update.path, e))?;
            }
            Ok(())
        }
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
fn assert_owner_only(path: &Path) -> Result<(), AppError> {
    use std::os::unix::fs::PermissionsExt;
    let mode = fs::metadata(path)
        .map_err(|e| AppError::io(path, e))?
        .permissions()
        .mode();
    if mode & 0o077 != 0 {
        return Err(AppError::Config(format!(
            "DeepSeek Harness credentials file {} must have mode 0600",
            path.display()
        )));
    }
    Ok(())
}

#[cfg(not(unix))]
fn assert_owner_only(_path: &Path) -> Result<(), AppError> {
    Ok(())
}

fn ensure_private_home(home: &Path) -> Result<(), AppError> {
    fs::create_dir_all(home).map_err(|e| AppError::io(home, e))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(home, fs::Permissions::from_mode(0o700))
            .map_err(|e| AppError::io(home, e))?;
    }
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
/// contents in a group/world-readable temporary file. On Windows the shared
/// writer already uses `ReplaceFileW` (with its WSL-UNC fallback), so retain
/// that replacement path; POSIX needs an explicit mode on the initial
/// `create_new` because chmod-after-rename leaves a crash-persistent exposure
/// window for credentials.
fn write_owner_only_atomic(path: &Path, contents: &[u8]) -> Result<(), AppError> {
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
    while path.exists() {
        path = root.join(format!("{base}_{counter}.yaml"));
        counter += 1;
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
}
