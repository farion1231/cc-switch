//! DeepSeek Harness live configuration support.
//!
//! DSH keeps provider profiles in `settings.yaml` and API keys in a separate
//! credentials document. This module edits those files in place; none of the
//! data is copied into cc-switch's provider database or generic sync paths.

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_yaml::{Mapping as SerdeMapping, Value as SerdeValue};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use yaml_edit::{AsYaml, Document, Mapping, YamlFile, YamlKind};

const SETTINGS_FILE: &str = "settings.yaml";
const CREDENTIALS_FILE: &str = ".credentials.yaml";
const NATIVE_NAMESPACE: &str = "llm-deepseek";
const CUSTOM_NAMESPACE: &str = "llm-pi-ai";
const DEFAULT_NAMESPACE: &str = "agent-default-model";
const NATIVE_ROUTE: &str = "deepseek-official";
const DEFAULT_BASE_URL: &str = "https://api.deepseek.com";
const DEFAULT_API_KEY_ENV: &str = "DEEPSEEK_API_KEY";
const LOCK_TIMEOUT: Duration = Duration::from_secs(2);
const LOCK_INITIAL_DELAY: Duration = Duration::from_millis(20);
const LOCK_MAX_DELAY: Duration = Duration::from_millis(200);
/// Locks older than this are abandoned: no DSH write takes anywhere near this
/// long, so a lock this old belongs to a process that died mid-write. This is
/// the fallback on platforms without a PID liveness probe and the backstop
/// against PID reuse after a crash.
const LOCK_STALE_AGE: Duration = Duration::from_secs(30);

pub const PROTOCOLS: [&str; 3] = [
    "openai-completions",
    "openai-responses",
    "anthropic-messages",
];

#[derive(Debug, Clone)]
pub struct DshPaths {
    pub home: PathBuf,
    pub settings: PathBuf,
    pub credentials: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DshModel {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u64>,
    /// Model fields owned by a newer DSH version remain on the editor wire.
    ///
    /// The React form copies model objects when it edits a row, so these values
    /// travel back with the next mutation and are written by the lossless YAML
    /// editor instead of being silently discarded by this older client.
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DshCredentialInfo {
    pub r#ref: String,
    pub configured: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    pub writable: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DshUnsupportedWarning {
    pub code: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DshProvider {
    pub route: String,
    pub kind: String,
    pub display_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "baseURL")]
    pub base_url: Option<String>,
    pub models: Vec<DshModel>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key_env: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub credential: Option<DshCredentialInfo>,
    pub customized: bool,
    pub revision: String,
    pub model_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DshDefaultModel {
    pub provider: String,
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DshSnapshot {
    pub home: String,
    pub settings_path: String,
    pub credentials_path: String,
    pub settings_revision: String,
    pub credentials_revision: String,
    pub read_only: bool,
    pub unsupported: Vec<DshUnsupportedWarning>,
    pub providers: Vec<DshProvider>,
    pub default_model: Option<DshDefaultModel>,
    pub protocols: Vec<String>,
    pub refreshed_at: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DshNativeInput {
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub models: Option<Vec<DshModel>>,
    #[serde(default)]
    pub api_key_env: Option<String>,
    #[serde(default)]
    pub expected_revision: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DshCustomInput {
    pub route: String,
    #[serde(default)]
    pub display_name: Option<String>,
    pub api: String,
    pub base_url: String,
    pub models: Vec<DshModel>,
    #[serde(default)]
    pub api_key_env: Option<String>,
    #[serde(default)]
    pub expected_revision: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DshModelDiscoveryResult {
    pub models: Vec<DshModel>,
}

fn error(code: &str, message: impl Into<String>) -> String {
    serde_json::json!({ "code": code, "message": message.into() }).to_string()
}

fn io_error(code: &str, context: &str, source: std::io::Error) -> String {
    error(code, format!("{context}: {source}"))
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

fn scalar_string(value: Option<&SerdeValue>) -> Option<String> {
    value
        .and_then(SerdeValue::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn map_get<'a>(mapping: &'a SerdeMapping, key: &str) -> Option<&'a SerdeValue> {
    mapping.get(SerdeValue::String(key.to_string()))
}

fn models_from_value(value: Option<&SerdeValue>) -> Result<Vec<DshModel>, String> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    serde_yaml::from_value::<Vec<DshModel>>(value.clone()).map_err(|_| {
        error(
            "invalid-settings",
            "DSH models must be a list of model objects",
        )
    })
}

fn default_models() -> Vec<DshModel> {
    [
        ("deepseek-v4-flash", "DeepSeek-V4-Flash"),
        ("deepseek-v4-pro", "DeepSeek-V4-Pro"),
    ]
    .into_iter()
    .map(|(id, name)| DshModel {
        id: id.to_string(),
        name: Some(name.to_string()),
        description: None,
        context_window: Some(1_000_000),
        max_tokens: Some(256_000),
        extra: BTreeMap::new(),
    })
    .collect()
}

fn validate_models(models: &[DshModel]) -> Result<(), String> {
    if models.is_empty() {
        return Err(error(
            "invalid-models",
            "At least one DSH model is required",
        ));
    }
    let mut ids = std::collections::HashSet::new();
    for model in models {
        let id = model.id.trim();
        if id.is_empty() {
            return Err(error("invalid-models", "DSH model id cannot be empty"));
        }
        if !ids.insert(id) {
            return Err(error("invalid-models", "DSH model ids must be unique"));
        }
        if model.context_window == Some(0) || model.max_tokens == Some(0) {
            return Err(error(
                "invalid-models",
                "DSH model capacities must be positive integers",
            ));
        }
    }
    Ok(())
}

fn validate_default_model_exists(
    root: &Mapping,
    provider: &str,
    model: &str,
) -> Result<(), String> {
    if provider == NATIVE_ROUTE {
        return Ok(());
    }
    let profile = root
        .get_mapping(CUSTOM_NAMESPACE)
        .and_then(|namespace| namespace.get_mapping("providers"))
        .and_then(|providers| providers.get_mapping(provider))
        .ok_or_else(|| {
            error(
                "provider-not-found",
                "The selected DSH provider does not exist",
            )
        })?;
    let models = profile.get_sequence("models").ok_or_else(|| {
        error(
            "model-not-found",
            "The selected DSH provider has no model catalog",
        )
    })?;
    if models.values().any(|value| {
        value
            .as_mapping()
            .and_then(|mapping| mapping.get("id"))
            .and_then(|value| value.as_scalar().map(|scalar| scalar.as_string()))
            .is_some_and(|id| id == model)
    }) {
        Ok(())
    } else {
        Err(error(
            "model-not-found",
            "The selected DSH model does not exist",
        ))
    }
}

fn validate_default_selection_settings(
    settings: &SerdeMapping,
    provider: &str,
    model: &str,
) -> Result<(), String> {
    if provider == NATIVE_ROUTE {
        return Ok(());
    }
    let profile = map_get(settings, CUSTOM_NAMESPACE)
        .and_then(SerdeValue::as_mapping)
        .and_then(|namespace| map_get(namespace, "providers"))
        .and_then(SerdeValue::as_mapping)
        .and_then(|providers| map_get(providers, provider))
        .and_then(SerdeValue::as_mapping)
        .ok_or_else(|| {
            error(
                "provider-not-found",
                "The selected DSH provider does not exist",
            )
        })?;
    let models = map_get(profile, "models")
        .and_then(SerdeValue::as_sequence)
        .ok_or_else(|| {
            error(
                "model-not-found",
                "The selected DSH provider has no model catalog",
            )
        })?;
    if models.iter().any(|value| {
        value
            .as_mapping()
            .and_then(|mapping| map_get(mapping, "id"))
            .and_then(SerdeValue::as_str)
            .is_some_and(|id| id == model)
    }) {
        Ok(())
    } else {
        Err(error(
            "model-not-found",
            "The selected DSH model does not exist",
        ))
    }
}

/// Reject a catalog replacement that would remove the model selected for new
/// agents. Model ids are provider-owned strings; this check intentionally does
/// not validate any other provider's catalog.
fn validate_default_model_in_models(
    default_provider: Option<&str>,
    default_model: Option<&str>,
    provider: &str,
    models: &[DshModel],
) -> Result<(), String> {
    let (Some(default_provider), Some(default_model)) = (default_provider, default_model) else {
        return Ok(());
    };
    if default_provider != provider {
        return Ok(());
    }
    if models
        .iter()
        .any(|candidate| candidate.id.trim() == default_model)
    {
        return Ok(());
    }
    Err(error(
        "model-in-use",
        format!(
            "The selected default model {provider}/{default_model} is not in the replacement catalog"
        ),
    ))
}

fn default_selection_from_yaml(root: &Mapping) -> (Option<String>, Option<String>) {
    let Some(default) = root.get_mapping(DEFAULT_NAMESPACE) else {
        return (None, None);
    };
    (
        default
            .get("provider")
            .and_then(|value| value.as_scalar().map(|scalar| scalar.as_string())),
        default
            .get("model")
            .and_then(|value| value.as_scalar().map(|scalar| scalar.as_string())),
    )
}

fn validate_route(route: &str) -> Result<(), String> {
    static ROUTE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let pattern = ROUTE.get_or_init(|| {
        Regex::new(r"^[a-z][a-z0-9]*(?:-[a-z0-9]+)*$").expect("route regex is valid")
    });
    if route == NATIVE_ROUTE || !pattern.is_match(route) {
        return Err(error(
            "invalid-route",
            "DSH provider id must be lowercase kebab-case and cannot use deepseek-official",
        ));
    }
    Ok(())
}

/// Validate a route read from an existing document without rejecting a route
/// created by an older DSH build. New routes still use the stricter kebab-case
/// rule above, while existing keys are preserved and remain editable/readable.
fn validate_existing_route(route: &str) -> Result<(), String> {
    if route.trim().is_empty() || route == NATIVE_ROUTE {
        return Err(error(
            "invalid-route",
            "DSH provider id must be non-empty and cannot use deepseek-official",
        ));
    }
    Ok(())
}

fn validate_credential_ref(reference: &str) -> Result<(), String> {
    static CREDENTIAL: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let pattern = CREDENTIAL.get_or_init(|| {
        Regex::new(r"^[A-Za-z_][A-Za-z0-9_]*$").expect("credential regex is valid")
    });
    if pattern.is_match(reference) {
        Ok(())
    } else {
        Err(error(
            "invalid-credential-ref",
            "Credential reference must be a POSIX environment identifier",
        ))
    }
}

fn normalize_optional(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

pub fn get_home() -> PathBuf {
    if let Some(override_dir) = crate::settings::get_dsh_override_dir() {
        return override_dir;
    }
    get_default_home()
}

/// Resolve the DSH home without consulting cc-switch's directory override.
///
/// The settings page uses this when resetting an override, so an environment
/// supplied `DSH_HOME` is restored instead of being replaced by `~/.dsh`.
pub fn get_default_home() -> PathBuf {
    if let Some(raw) = std::env::var_os("DSH_HOME") {
        let trimmed = raw.to_string_lossy().trim().to_string();
        if !trimmed.is_empty() {
            return expand_home(&trimmed);
        }
    }
    crate::config::get_home_dir().join(".dsh")
}

fn expand_home(raw: &str) -> PathBuf {
    if raw == "~" {
        return crate::config::get_home_dir();
    }
    if let Some(rest) = raw.strip_prefix("~/").or_else(|| raw.strip_prefix("~\\")) {
        return crate::config::get_home_dir().join(rest);
    }
    PathBuf::from(raw)
}

pub fn paths() -> DshPaths {
    let home = get_home();
    DshPaths {
        settings: home.join(SETTINGS_FILE),
        credentials: home.join(CREDENTIALS_FILE),
        home,
    }
}

fn read_bytes(path: &Path) -> Result<Vec<u8>, String> {
    match fs::read(path) {
        Ok(bytes) => Ok(bytes),
        Err(source) if source.kind() == ErrorKind::NotFound => Ok(Vec::new()),
        Err(source) => Err(io_error("read-failed", "Failed to read DSH file", source)),
    }
}

fn parse_settings(bytes: &[u8]) -> Result<SerdeMapping, String> {
    if bytes.is_empty() || bytes.iter().all(u8::is_ascii_whitespace) {
        return Ok(SerdeMapping::new());
    }
    let value: SerdeValue = serde_yaml::from_slice(bytes).map_err(|_| {
        error(
            "invalid-settings",
            "DSH settings.yaml contains invalid YAML",
        )
    })?;
    value.as_mapping().cloned().ok_or_else(|| {
        error(
            "invalid-settings-root",
            "DSH settings.yaml root must be a mapping",
        )
    })
}

fn parse_credentials(bytes: &[u8]) -> Result<SerdeMapping, String> {
    if bytes.is_empty() || bytes.iter().all(u8::is_ascii_whitespace) {
        return Ok(SerdeMapping::new());
    }
    let value: SerdeValue = serde_yaml::from_slice(bytes).map_err(|_| {
        error(
            "invalid-credentials",
            "DSH credentials file contains invalid YAML",
        )
    })?;
    let map = value.as_mapping().cloned().ok_or_else(|| {
        error(
            "invalid-credentials-root",
            "DSH credentials root must be a mapping",
        )
    })?;
    for (key, value) in &map {
        let Some(reference) = key.as_str() else {
            return Err(error(
                "invalid-credentials",
                "DSH credential references must be strings",
            ));
        };
        validate_credential_ref(reference)?;
        if value.as_str().is_none_or(|secret| secret.is_empty()) {
            return Err(error(
                "invalid-credentials",
                "DSH credential values must be non-empty strings",
            ));
        }
    }
    Ok(map)
}

fn validate_credentials_permissions(path: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        match fs::metadata(path) {
            Ok(metadata) if metadata.permissions().mode() & 0o077 != 0 => {
                return Err(error(
                    "insecure-credentials-permissions",
                    "DSH credentials must not be readable or writable by group or other users",
                ));
            }
            Ok(_) => {}
            Err(source) if source.kind() == ErrorKind::NotFound => {}
            Err(source) => {
                return Err(io_error(
                    "credentials-metadata-failed",
                    "Failed to inspect DSH credentials permissions",
                    source,
                ));
            }
        }
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

fn parse_default(settings: &SerdeMapping) -> Result<Option<DshDefaultModel>, String> {
    let Some(value) = map_get(settings, DEFAULT_NAMESPACE) else {
        return Ok(None);
    };
    let mapping = value.as_mapping().ok_or_else(|| {
        error(
            "invalid-default-model",
            "agent-default-model must be a mapping",
        )
    })?;
    let provider = scalar_string(map_get(mapping, "provider"))
        .ok_or_else(|| error("invalid-default-model", "Default provider is required"))?;
    let model = scalar_string(map_get(mapping, "model"))
        .ok_or_else(|| error("invalid-default-model", "Default model is required"))?;
    Ok(Some(DshDefaultModel {
        provider,
        model,
        reasoning_effort: scalar_string(map_get(mapping, "reasoningEffort")),
    }))
}

fn credential_info(reference: &str, credentials: &SerdeMapping) -> DshCredentialInfo {
    let file_configured = credentials
        .get(SerdeValue::String(reference.to_string()))
        .and_then(SerdeValue::as_str)
        .is_some_and(|value| !value.is_empty());
    let environment_configured =
        std::env::var_os(reference).is_some_and(|value| !value.to_string_lossy().trim().is_empty());
    DshCredentialInfo {
        r#ref: reference.to_string(),
        configured: environment_configured || file_configured,
        source: if environment_configured {
            Some("process".to_string())
        } else if file_configured {
            Some("file".to_string())
        } else {
            None
        },
        writable: !environment_configured,
    }
}

fn parse_native(
    settings: &SerdeMapping,
    credentials: &SerdeMapping,
    revision: &str,
) -> Result<DshProvider, String> {
    let section = map_get(settings, NATIVE_NAMESPACE);
    let mapping = section.and_then(SerdeValue::as_mapping);
    if section.is_some() && mapping.is_none() {
        return Err(error(
            "invalid-native-provider",
            "llm-deepseek must be a mapping",
        ));
    }
    let api_key_env = mapping
        .and_then(|map| scalar_string(map_get(map, "apiKeyEnv")))
        .unwrap_or_else(|| DEFAULT_API_KEY_ENV.to_string());
    validate_credential_ref(&api_key_env)?;
    let models = match mapping.and_then(|map| map_get(map, "models")) {
        Some(value) => models_from_value(Some(value))?,
        None => default_models(),
    };
    Ok(DshProvider {
        route: NATIVE_ROUTE.to_string(),
        kind: "native".to_string(),
        display_name: "DeepSeek Official".to_string(),
        api: None,
        base_url: mapping
            .and_then(|map| scalar_string(map_get(map, "baseURL")))
            .or_else(|| Some(DEFAULT_BASE_URL.to_string())),
        model_count: models.len(),
        models,
        credential: Some(credential_info(&api_key_env, credentials)),
        api_key_env: Some(api_key_env),
        customized: section.is_some(),
        revision: revision.to_string(),
    })
}

fn parse_custom(
    settings: &SerdeMapping,
    credentials: &SerdeMapping,
    revision: &str,
) -> Result<Vec<DshProvider>, String> {
    let Some(section) = map_get(settings, CUSTOM_NAMESPACE) else {
        return Ok(Vec::new());
    };
    let section = section
        .as_mapping()
        .ok_or_else(|| error("invalid-custom-provider", "llm-pi-ai must be a mapping"))?;
    let Some(providers) = map_get(section, "providers") else {
        return Ok(Vec::new());
    };
    let providers = providers.as_mapping().ok_or_else(|| {
        error(
            "invalid-custom-provider",
            "llm-pi-ai.providers must be a mapping",
        )
    })?;
    let mut result = Vec::new();
    for (route, profile) in providers {
        let route = route
            .as_str()
            .ok_or_else(|| error("invalid-route", "DSH provider ids must be strings"))?;
        validate_existing_route(route)?;
        let profile = profile.as_mapping().ok_or_else(|| {
            error(
                "invalid-custom-provider",
                format!("DSH provider {route} must be a mapping"),
            )
        })?;
        let api = scalar_string(map_get(profile, "api"));
        let base_url = scalar_string(map_get(profile, "baseURL"));
        let models = models_from_value(map_get(profile, "models"))?;
        let api_key_env = scalar_string(map_get(profile, "apiKeyEnv"));
        if let Some(reference) = &api_key_env {
            validate_credential_ref(reference)?;
        }
        result.push(DshProvider {
            route: route.to_string(),
            kind: "custom".to_string(),
            display_name: scalar_string(map_get(profile, "displayName"))
                .unwrap_or_else(|| route.to_string()),
            api,
            base_url,
            model_count: models.len(),
            models,
            credential: api_key_env
                .as_deref()
                .map(|reference| credential_info(reference, credentials)),
            api_key_env,
            customized: true,
            revision: revision.to_string(),
        });
    }
    result.sort_by(|left, right| left.route.cmp(&right.route));
    Ok(result)
}

pub fn snapshot() -> Result<DshSnapshot, String> {
    let paths = paths();
    let settings_bytes = read_bytes(&paths.settings)?;
    validate_credentials_permissions(&paths.credentials)?;
    let credentials_bytes = read_bytes(&paths.credentials)?;
    let settings = parse_settings(&settings_bytes)?;
    let credentials = parse_credentials(&credentials_bytes)?;
    let settings_revision = sha256_hex(&settings_bytes);
    let mut providers = vec![parse_native(&settings, &credentials, &settings_revision)?];
    providers.extend(parse_custom(&settings, &credentials, &settings_revision)?);
    let read_only = fs::metadata(&paths.settings)
        .map(|metadata| metadata.permissions().readonly())
        .unwrap_or(false);
    Ok(DshSnapshot {
        home: paths.home.to_string_lossy().to_string(),
        settings_path: paths.settings.to_string_lossy().to_string(),
        credentials_path: paths.credentials.to_string_lossy().to_string(),
        settings_revision,
        credentials_revision: sha256_hex(&credentials_bytes),
        read_only,
        unsupported: Vec::new(),
        providers,
        default_model: parse_default(&settings)?,
        protocols: PROTOCOLS.into_iter().map(str::to_string).collect(),
        refreshed_at: now_millis(),
    })
}

struct FileLock {
    path: PathBuf,
}

impl FileLock {
    fn acquire(target: &Path) -> Result<Self, String> {
        let lock_name = format!(
            "{}.lock",
            target
                .file_name()
                .ok_or_else(|| error("invalid-path", "DSH file has no name"))?
                .to_string_lossy()
        );
        let lock_path = target.with_file_name(lock_name);
        let deadline = Instant::now() + LOCK_TIMEOUT;
        let mut delay = LOCK_INITIAL_DELAY;
        loop {
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            match options.open(&lock_path) {
                Ok(mut file) => {
                    let write_result = file
                        .write_all(format!("{}\n", std::process::id()).as_bytes())
                        .and_then(|_| file.flush());
                    if let Err(source) = write_result {
                        drop(file);
                        let _ = fs::remove_file(&lock_path);
                        return Err(io_error(
                            "lock-write-failed",
                            "Failed to initialize DSH lock",
                            source,
                        ));
                    }
                    return Ok(Self { path: lock_path });
                }
                Err(source) if source.kind() == ErrorKind::AlreadyExists => {
                    // A previous process may have been terminated after
                    // creating the lock but before dropping it. Remove such an
                    // abandoned lock instead of waiting out the timeout.
                    if lock_is_stale(&lock_path) {
                        let _ = fs::remove_file(&lock_path);
                        continue;
                    }
                    if Instant::now() >= deadline {
                        return Err(error(
                            "lock-timeout",
                            "Timed out waiting for the DSH configuration writer lock",
                        ));
                    }
                    thread::sleep(delay);
                    delay = (delay * 2).min(LOCK_MAX_DELAY);
                }
                Err(source) => {
                    return Err(io_error(
                        "lock-failed",
                        "Failed to create DSH writer lock",
                        source,
                    ));
                }
            }
        }
    }
}

impl Drop for FileLock {
    fn drop(&mut self) {
        if let Err(source) = fs::remove_file(&self.path) {
            if source.kind() != ErrorKind::NotFound {
                log::warn!("Failed to remove DSH writer lock: {source}");
            }
        }
    }
}

/// True when the lock holder is gone and the `.lock` file can be reclaimed.
///
/// A lock is stale when its recorded PID is no longer alive (Unix) or when it
/// is older than [`LOCK_STALE_AGE`], which also covers platforms without a
/// liveness probe, unreadable PID files, and PID reuse after a crash.
fn lock_is_stale(lock_path: &Path) -> bool {
    #[cfg(unix)]
    if lock_holder_dead(lock_path) {
        return true;
    }
    lock_older_than(lock_path, LOCK_STALE_AGE)
}

/// Read the PID a lock file was created with and report whether that process
/// still exists.
#[cfg(unix)]
fn lock_holder_dead(lock_path: &Path) -> bool {
    let Some(pid) = lock_holder_pid(lock_path) else {
        return false;
    };
    // Signal 0 only probes for existence; no signal is delivered.
    let result = unsafe { libc::kill(pid as libc::pid_t, 0) };
    if result == 0 {
        return false;
    }
    // EPERM means a process with this PID exists but belongs to another user.
    std::io::Error::last_os_error().kind() != ErrorKind::PermissionDenied
}

#[cfg(unix)]
fn lock_holder_pid(lock_path: &Path) -> Option<u32> {
    fs::read(lock_path)
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .and_then(|text| text.trim().parse().ok())
}

fn lock_older_than(lock_path: &Path, age: Duration) -> bool {
    fs::metadata(lock_path)
        .ok()
        .and_then(|metadata| metadata.modified().ok())
        .and_then(|modified| modified.elapsed().ok())
        .is_some_and(|elapsed| elapsed >= age)
}

pub(crate) fn ensure_secure_home(home: &Path) -> Result<(), String> {
    // Must be captured before create_dir_all; only newly created homes get
    // their permissions tightened on Unix. The variable is cfg'd so Windows
    // builds do not warn about an unused binding.
    #[cfg(unix)]
    let existed = home.exists();
    fs::create_dir_all(home)
        .map_err(|source| io_error("create-home-failed", "Failed to create DSH home", source))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if !existed {
            fs::set_permissions(home, fs::Permissions::from_mode(0o700)).map_err(|source| {
                io_error(
                    "permissions-failed",
                    "Failed to secure DSH home permissions",
                    source,
                )
            })?;
        }
    }
    Ok(())
}

fn atomic_write_secure(path: &Path, data: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| error("invalid-path", "DSH path has no parent"))?;
    ensure_secure_home(parent)?;
    crate::config::atomic_write_with_mode(path, data, 0o600)
        .map_err(|source| error("write-failed", source.to_string()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(|source| {
            io_error(
                "permissions-failed",
                "Failed to secure DSH file permissions",
                source,
            )
        })?;
    }
    Ok(())
}

fn line_ending(bytes: &[u8]) -> &'static str {
    if bytes.windows(2).any(|pair| pair == b"\r\n") {
        "\r\n"
    } else {
        "\n"
    }
}

fn append_top_level_mapping(
    bytes: &[u8],
    key: &str,
    value: &SerdeMapping,
) -> Result<Vec<u8>, String> {
    let _ = parse_settings(bytes)?;
    let mut section = SerdeMapping::new();
    section.insert(key.into(), SerdeValue::Mapping(value.clone()));
    let encoded = serde_yaml::to_string(&section)
        .map_err(|_| error("serialize-failed", "Failed to encode DSH settings"))?;
    let newline = line_ending(bytes);
    let encoded = if newline == "\r\n" {
        encoded.replace('\n', "\r\n")
    } else {
        encoded
    };
    let mut output = bytes.to_vec();
    if !output.is_empty() && !output.ends_with(newline.as_bytes()) {
        output.extend_from_slice(newline.as_bytes());
    }
    output.extend_from_slice(encoded.as_bytes());
    let _ = parse_settings(&output)?;
    Ok(output)
}

fn parse_lossless(bytes: &[u8]) -> Result<YamlFile, String> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| error("invalid-encoding", "DSH YAML must be UTF-8"))?;
    let file = if text.trim().is_empty() {
        YamlFile::new()
    } else {
        YamlFile::from_str(text)
            .map_err(|_| error("invalid-yaml", "DSH YAML contains syntax errors"))?
    };
    if file.documents().count() > 1 {
        return Err(error(
            "unsupported-yaml",
            "Multi-document YAML cannot be edited safely",
        ));
    }
    let document = file.ensure_document();
    if document.as_mapping().is_none() {
        return Err(error(
            "invalid-yaml-root",
            "DSH YAML root must be a mapping",
        ));
    }
    Ok(file)
}

fn custom_profile_mapping(input: &DshCustomInput) -> Result<SerdeMapping, String> {
    let mut profile = SerdeMapping::new();
    if let Some(display_name) = &input.display_name {
        profile.insert("displayName".into(), display_name.clone().into());
    }
    profile.insert("api".into(), input.api.clone().into());
    profile.insert("baseURL".into(), input.base_url.clone().into());
    if let Some(reference) = &input.api_key_env {
        profile.insert("apiKeyEnv".into(), reference.clone().into());
    }
    profile.insert(
        "models".into(),
        serde_yaml::to_value(&input.models)
            .map_err(|_| error("serialize-failed", "Failed to encode DSH models"))?,
    );
    Ok(profile)
}

fn model_mapping(model: &DshModel) -> Result<Mapping, String> {
    let mut builder = yaml_edit::MappingBuilder::new().pair("id", model.id.as_str());
    if let Some(name) = model.name.as_deref() {
        builder = builder.pair("name", name);
    }
    if let Some(description) = model.description.as_deref() {
        builder = builder.pair("description", description);
    }
    if let Some(context_window) = model.context_window {
        builder = builder.pair("contextWindow", context_window);
    }
    if let Some(max_tokens) = model.max_tokens {
        builder = builder.pair("maxTokens", max_tokens);
    }
    let document = builder.build_document();
    let mapping = document
        .as_mapping()
        .ok_or_else(|| error("serialize-failed", "DSH model did not encode as a mapping"))?;
    for (key, value) in &model.extra {
        if matches!(
            key.as_str(),
            "id" | "name" | "description" | "contextWindow" | "maxTokens"
        ) {
            continue;
        }
        mapping.set(key, json_value_node(value)?);
    }
    Ok(mapping.clone())
}

fn model_node(model: &DshModel) -> Result<yaml_edit::YamlNode, String> {
    let mapping = model_mapping(model)?;
    let mapping_text = mapping.to_string();
    let mut lines = mapping_text.lines();
    let first = lines
        .next()
        .ok_or_else(|| error("serialize-failed", "DSH model did not encode as a mapping"))?;
    let mut item = format!("- {first}");
    for line in lines {
        item.push('\n');
        item.push_str("  ");
        item.push_str(line);
    }
    let document = Document::from_str(&item)
        .map_err(|_| error("serialize-failed", "Failed to build DSH model YAML"))?;
    document
        .as_sequence()
        .and_then(|sequence| sequence.first())
        .ok_or_else(|| {
            error(
                "serialize-failed",
                "DSH model did not encode as a list item",
            )
        })
}

fn json_value_node(value: &serde_json::Value) -> Result<yaml_edit::YamlNode, String> {
    let mut wrapper = SerdeMapping::new();
    wrapper.insert(
        "__dsh_value".into(),
        serde_yaml::to_value(value)
            .map_err(|_| error("serialize-failed", "Failed to encode DSH model field"))?,
    );
    let yaml = serde_yaml::to_string(&SerdeValue::Mapping(wrapper))
        .map_err(|_| error("serialize-failed", "Failed to encode DSH model field"))?;
    let document = Document::from_str(&yaml)
        .map_err(|_| error("serialize-failed", "Failed to build DSH model field YAML"))?;
    document.get("__dsh_value").ok_or_else(|| {
        error(
            "serialize-failed",
            "DSH model field did not encode as a YAML value",
        )
    })
}

fn update_model_mapping(mapping: &Mapping, model: &DshModel) -> Result<(), String> {
    mapping.set("id", model.id.as_str());
    set_or_remove(mapping, "name", model.name.as_deref());
    set_or_remove(mapping, "description", model.description.as_deref());
    set_or_remove_u64(mapping, "contextWindow", model.context_window);
    set_or_remove_u64(mapping, "maxTokens", model.max_tokens);
    for (key, value) in &model.extra {
        if matches!(
            key.as_str(),
            "id" | "name" | "description" | "contextWindow" | "maxTokens"
        ) {
            continue;
        }
        mapping.set(key, json_value_node(value)?);
    }
    Ok(())
}

/// A sequence copied into a newly inserted mapping entry needs its first dash
/// and every nested mapping line indented explicitly. `yaml-edit` supplies the
/// parent indentation when it replaces an existing value, but
/// `MappingEntry::new` invokes `AsYaml` with zero indentation for a new key.
struct IndentedModels<'a> {
    models: &'a [Mapping],
    indent: usize,
}

impl AsYaml for IndentedModels<'_> {
    fn as_node(&self) -> Option<&rowan::api::SyntaxNode<yaml_edit::Lang>> {
        None
    }

    fn kind(&self) -> YamlKind {
        YamlKind::Sequence
    }

    fn build_content(
        &self,
        builder: &mut rowan::GreenNodeBuilder,
        _indent: usize,
        flow_context: bool,
    ) -> bool {
        use yaml_edit::SyntaxKind;

        builder.start_node(SyntaxKind::SEQUENCE.into());
        for (index, model) in self.models.iter().enumerate() {
            if index > 0 {
                builder.token(SyntaxKind::NEWLINE.into(), "\n");
            }
            builder.token(SyntaxKind::WHITESPACE.into(), &" ".repeat(self.indent));
            builder.token(SyntaxKind::DASH.into(), "-");
            builder.token(SyntaxKind::WHITESPACE.into(), " ");
            let ends_with_newline =
                model.build_content(builder, self.indent.saturating_add(2), flow_context);
            if !ends_with_newline && index + 1 == self.models.len() {
                builder.token(SyntaxKind::NEWLINE.into(), "\n");
            }
        }
        builder.finish_node();
        true
    }

    fn is_inline(&self) -> bool {
        false
    }
}

fn set_models(mapping: &Mapping, models: &[DshModel]) -> Result<(), String> {
    let Some(sequence) = mapping.get_sequence("models") else {
        let model_nodes = models
            .iter()
            .map(model_mapping)
            .collect::<Result<Vec<_>, _>>()?;
        mapping.set(
            "models",
            IndentedModels {
                models: &model_nodes,
                indent: mapping.detect_indentation_level().saturating_add(2),
            },
        );
        return Ok(());
    };
    if sequence.is_flow_style() {
        return Err(error(
            "unsafe-yaml-edit",
            "A flow-style DSH model list cannot be edited safely",
        ));
    }

    let original_len = sequence.len();
    for (index, model) in models.iter().enumerate().take(original_len) {
        let node = sequence
            .get(index)
            .ok_or_else(|| error("unsafe-yaml-edit", "Could not read DSH model entry"))?;
        let existing = node.as_mapping().ok_or_else(|| {
            error(
                "unsafe-yaml-edit",
                "DSH model entries must be block mappings",
            )
        })?;
        update_model_mapping(existing, model)?;
    }
    while sequence.len() > models.len() {
        sequence.pop();
    }
    for model in models.iter().skip(original_len) {
        sequence.push(model_node(model)?);
    }
    Ok(())
}

fn set_or_remove(mapping: &Mapping, key: &str, value: Option<&str>) {
    if let Some(value) = value {
        mapping.set(key, value);
    } else {
        mapping.remove(key);
    }
}

fn set_or_remove_u64(mapping: &Mapping, key: &str, value: Option<u64>) {
    if let Some(value) = value {
        mapping.set(key, value);
    } else {
        mapping.remove(key);
    }
}

fn validate_default_provider(root: &Mapping, provider: &str) -> Result<(), String> {
    if provider == NATIVE_ROUTE {
        return Ok(());
    }
    validate_existing_route(provider)?;
    let exists = root
        .get_mapping(CUSTOM_NAMESPACE)
        .and_then(|namespace| namespace.get_mapping("providers"))
        .is_some_and(|providers| providers.contains_key(provider));
    if exists {
        Ok(())
    } else {
        Err(error(
            "provider-not-found",
            "The selected DSH provider does not exist",
        ))
    }
}

fn mutate_settings<F>(expected_revision: Option<&str>, mutate: F) -> Result<DshSnapshot, String>
where
    F: FnOnce(&Mapping) -> Result<(), String>,
{
    let paths = paths();
    ensure_secure_home(&paths.home)?;
    let _lock = FileLock::acquire(&paths.settings)?;
    let bytes = read_bytes(&paths.settings)?;
    let revision = sha256_hex(&bytes);
    if expected_revision.is_some_and(|expected| expected != revision) {
        return Err(error(
            "settings-conflict",
            "DSH settings changed while the editor was open",
        ));
    }
    let file = parse_lossless(&bytes)?;
    let document = file.ensure_document();
    let root = document
        .as_mapping()
        .ok_or_else(|| error("invalid-yaml-root", "DSH YAML root must be a mapping"))?;
    mutate(&root)?;
    let output = file.to_string();
    let _ = parse_settings(output.as_bytes())?;
    atomic_write_secure(&paths.settings, output.as_bytes())?;
    drop(_lock);
    snapshot()
}

fn append_settings_section(
    expected_revision: Option<&str>,
    namespace: &str,
    section: &SerdeMapping,
) -> Result<DshSnapshot, String> {
    let paths = paths();
    ensure_secure_home(&paths.home)?;
    let _lock = FileLock::acquire(&paths.settings)?;
    let bytes = read_bytes(&paths.settings)?;
    let revision = sha256_hex(&bytes);
    if expected_revision.is_some_and(|expected| expected != revision) {
        return Err(error(
            "settings-conflict",
            "DSH settings changed while the editor was open",
        ));
    }
    let parsed = parse_settings(&bytes)?;
    if map_get(&parsed, namespace).is_some() {
        return Err(error(
            "settings-conflict",
            "DSH settings changed while the editor was open",
        ));
    }
    let output = append_top_level_mapping(&bytes, namespace, section)?;
    atomic_write_secure(&paths.settings, &output)?;
    drop(_lock);
    snapshot()
}

fn custom_profile_insert_text(
    input: &DshCustomInput,
    newline: &str,
    provider_indent: usize,
) -> Result<Vec<u8>, String> {
    let profile = custom_profile_mapping(input)?;
    let value = serde_yaml::to_value(profile)
        .map_err(|_| error("serialize-failed", "Failed to encode DSH provider"))?;
    let encoded = serde_yaml::to_string(&value)
        .map_err(|_| error("serialize-failed", "Failed to encode DSH provider"))?;
    let provider_prefix = " ".repeat(provider_indent);
    let mut text = format!("{provider_prefix}{}:{}", input.route, newline);
    let mut in_models = false;
    for line in encoded.trim_end_matches(['\r', '\n']).lines() {
        let leading_spaces = line.bytes().take_while(|byte| *byte == b' ').count();
        let content = &line[leading_spaces..];
        let base_indent = if in_models {
            provider_indent.saturating_add(4)
        } else {
            provider_indent.saturating_add(2)
        };
        text.push_str(&" ".repeat(base_indent.saturating_add(leading_spaces)));
        text.push_str(content);
        text.push_str(newline);
        if leading_spaces == 0 && content == "models:" {
            in_models = true;
        }
    }
    Ok(text.into_bytes())
}

fn append_custom_provider(
    expected_revision: Option<&str>,
    input: &DshCustomInput,
) -> Result<DshSnapshot, String> {
    let paths = paths();
    ensure_secure_home(&paths.home)?;
    let _lock = FileLock::acquire(&paths.settings)?;
    let bytes = read_bytes(&paths.settings)?;
    let revision = sha256_hex(&bytes);
    if expected_revision.is_some_and(|expected| expected != revision) {
        return Err(error(
            "settings-conflict",
            "DSH settings changed while the editor was open",
        ));
    }
    let file = parse_lossless(&bytes)?;
    let document = file.ensure_document();
    let root = document
        .as_mapping()
        .ok_or_else(|| error("invalid-yaml-root", "DSH YAML root must be a mapping"))?;
    let namespace = root
        .get_mapping(CUSTOM_NAMESPACE)
        .ok_or_else(|| error("route-not-found", "DSH custom provider does not exist"))?;
    let Some(providers) = namespace.get_mapping("providers") else {
        if namespace.is_flow_style() {
            return Err(error(
                "unsafe-yaml-edit",
                "llm-pi-ai is flow-style and cannot be edited safely",
            ));
        }
        let namespace_indent = namespace.detect_indentation_level();
        let route_indent = namespace_indent.checked_add(4).ok_or_else(|| {
            error(
                "unsafe-yaml-edit",
                "Could not determine DSH provider indentation",
            )
        })?;
        let range = namespace.byte_range();
        let offset = usize::try_from(range.end).map_err(|_| {
            error(
                "unsafe-yaml-edit",
                "DSH provider namespace has an invalid source range",
            )
        })?;
        if offset > bytes.len() {
            return Err(error(
                "unsafe-yaml-edit",
                "DSH provider namespace is outside the settings file",
            ));
        }
        let newline = line_ending(&bytes);
        let mut insertion = if offset > 0 && !bytes[..offset].ends_with(newline.as_bytes()) {
            newline.as_bytes().to_vec()
        } else {
            Vec::new()
        };
        insertion.extend_from_slice(
            format!("{}providers:{}", " ".repeat(namespace_indent + 2), newline).as_bytes(),
        );
        insertion.extend_from_slice(&custom_profile_insert_text(input, newline, route_indent)?);
        let mut output = bytes;
        output.splice(offset..offset, insertion);
        let _ = parse_settings(&output)?;
        atomic_write_secure(&paths.settings, &output)?;
        drop(_lock);
        return snapshot();
    };
    if providers.contains_key(input.route.as_str()) {
        return Err(error("route-exists", "DSH provider id already exists"));
    }
    if providers.is_flow_style() {
        return Err(error(
            "unsafe-yaml-edit",
            "Adding a route to a flow-style providers mapping cannot preserve this YAML safely",
        ));
    }
    let provider_indent = {
        let detected = providers.detect_indentation_level();
        if detected == 0 {
            namespace
                .detect_indentation_level()
                .checked_add(2)
                .ok_or_else(|| {
                    error(
                        "unsafe-yaml-edit",
                        "Could not determine DSH provider indentation",
                    )
                })?
        } else {
            detected
        }
    };
    let range = providers.byte_range();
    let offset = usize::try_from(range.end).map_err(|_| {
        error(
            "unsafe-yaml-edit",
            "DSH provider mapping has an invalid source range",
        )
    })?;
    if offset > bytes.len() {
        return Err(error(
            "unsafe-yaml-edit",
            "DSH provider mapping is outside the settings file",
        ));
    }
    let newline = line_ending(&bytes);
    let mut insertion = if offset > 0 && !bytes[..offset].ends_with(newline.as_bytes()) {
        newline.as_bytes().to_vec()
    } else {
        Vec::new()
    };
    insertion.extend_from_slice(&custom_profile_insert_text(
        input,
        newline,
        provider_indent,
    )?);
    let mut output = bytes;
    output.splice(offset..offset, insertion);
    let _ = parse_settings(&output)?;
    atomic_write_secure(&paths.settings, &output)?;
    drop(_lock);
    snapshot()
}

pub fn upsert_native(input: DshNativeInput) -> Result<DshSnapshot, String> {
    // `base_url` keeps the absent-vs-cleared distinction after trimming:
    // `None` means the field was not sent (leave the stored value untouched),
    // `Some("")` means the user cleared it (remove the stored override), and a
    // non-empty value is written as the new override.
    let base_url = input.base_url.map(|value| value.trim().to_string());
    let api_key_env = normalize_optional(input.api_key_env);
    if let Some(reference) = &api_key_env {
        validate_credential_ref(reference)?;
    }
    if let Some(models) = &input.models {
        validate_models(models)?;
    }
    let settings = parse_settings(&read_bytes(&paths().settings)?)?;
    if map_get(&settings, NATIVE_NAMESPACE).is_none() {
        let mut section = SerdeMapping::new();
        if let Some(base_url) = base_url.as_deref().filter(|value| !value.is_empty()) {
            section.insert("baseURL".into(), base_url.into());
        }
        if let Some(reference) = &api_key_env {
            section.insert("apiKeyEnv".into(), reference.clone().into());
        }
        if let Some(models) = &input.models {
            section.insert(
                "models".into(),
                serde_yaml::to_value(models)
                    .map_err(|_| error("serialize-failed", "Failed to encode DSH models"))?,
            );
        }
        return append_settings_section(
            input.expected_revision.as_deref(),
            NATIVE_NAMESPACE,
            &section,
        );
    }
    mutate_settings(input.expected_revision.as_deref(), |root| {
        let section = root
            .get_mapping(NATIVE_NAMESPACE)
            .ok_or_else(|| error("invalid-settings", "llm-deepseek must be a mapping"))?;
        match base_url.as_deref() {
            Some("") => {
                section.remove("baseURL");
                // Removing the last entry would leave `llm-deepseek:` with a
                // null value that the lossless serializer emits and this
                // module rejects. A section with nothing left is equivalent
                // to no section at all, so drop it entirely.
                if section.is_empty() {
                    root.remove(NATIVE_NAMESPACE);
                }
            }
            Some(base_url) => {
                section.set("baseURL", base_url);
            }
            None => {}
        }
        if let Some(reference) = api_key_env.as_deref() {
            section.set("apiKeyEnv", reference);
        }
        if let Some(models) = &input.models {
            let (default_provider, default_model) = default_selection_from_yaml(root);
            // The native catalog is advisory, but an explicitly listed default
            // must remain available when the user replaces that catalog.
            let default_was_listed = default_model.as_deref().is_some_and(|default_model| {
                section.get_sequence("models").is_some_and(|models| {
                    models.values().any(|value| {
                        value
                            .as_mapping()
                            .and_then(|mapping| mapping.get("id"))
                            .and_then(|value| value.as_scalar().map(|scalar| scalar.as_string()))
                            .as_deref()
                            == Some(default_model)
                    })
                })
            });
            if default_provider.as_deref() == Some(NATIVE_ROUTE) && default_was_listed {
                validate_default_model_in_models(
                    default_provider.as_deref(),
                    default_model.as_deref(),
                    NATIVE_ROUTE,
                    models,
                )?;
            }
            set_models(&section, models)?;
        }
        Ok(())
    })
}

pub fn reset_native(expected_revision: Option<String>) -> Result<DshSnapshot, String> {
    mutate_settings(expected_revision.as_deref(), |root| {
        let (default_provider, default_model) = default_selection_from_yaml(root);
        if default_provider.as_deref() == Some(NATIVE_ROUTE) {
            let defaults = default_models();
            // Resetting removes only the user catalog. A default that was
            // listed by the override but not by the shipped native catalog
            // would otherwise become a dangling picker selection.
            if let Some(section) = root.get_mapping(NATIVE_NAMESPACE) {
                let was_listed = section.get_sequence("models").is_some_and(|models| {
                    models.values().any(|value| {
                        value
                            .as_mapping()
                            .and_then(|mapping| mapping.get("id"))
                            .and_then(|value| value.as_scalar().map(|scalar| scalar.as_string()))
                            .as_deref()
                            == default_model.as_deref()
                    })
                });
                if was_listed {
                    validate_default_model_in_models(
                        default_provider.as_deref(),
                        default_model.as_deref(),
                        NATIVE_ROUTE,
                        &defaults,
                    )?;
                }
            }
        }
        root.remove(NATIVE_NAMESPACE);
        Ok(())
    })
}

fn validate_custom(input: &mut DshCustomInput, existing_route: bool) -> Result<(), String> {
    input.route = input.route.trim().to_string();
    if existing_route {
        validate_existing_route(&input.route)?;
    } else {
        validate_route(&input.route)?;
    }
    input.display_name = normalize_optional(input.display_name.take());
    input.api = input.api.trim().to_string();
    if !PROTOCOLS.contains(&input.api.as_str()) {
        return Err(error(
            "invalid-protocol",
            "Unsupported DSH provider protocol",
        ));
    }
    input.base_url = input.base_url.trim().to_string();
    if input.base_url.is_empty() {
        return Err(error(
            "invalid-base-url",
            "DSH provider base URL is required",
        ));
    }
    validate_models(&input.models)?;
    input.api_key_env = normalize_optional(input.api_key_env.take());
    if let Some(reference) = &input.api_key_env {
        validate_credential_ref(reference)?;
    }
    Ok(())
}

fn update_profile(profile: &Mapping, input: &DshCustomInput) -> Result<(), String> {
    set_or_remove(profile, "displayName", input.display_name.as_deref());
    profile.set("api", input.api.as_str());
    profile.set("baseURL", input.base_url.as_str());
    set_or_remove(profile, "apiKeyEnv", input.api_key_env.as_deref());
    set_models(profile, &input.models)
}

pub fn create_custom(mut input: DshCustomInput) -> Result<DshSnapshot, String> {
    validate_custom(&mut input, false)?;
    let current = parse_settings(&read_bytes(&paths().settings)?)?;
    if map_get(&current, CUSTOM_NAMESPACE).is_none() {
        let profile = custom_profile_mapping(&input)?;
        let mut providers = SerdeMapping::new();
        providers.insert(input.route.clone().into(), SerdeValue::Mapping(profile));
        let mut section = SerdeMapping::new();
        section.insert("providers".into(), SerdeValue::Mapping(providers));
        return append_settings_section(
            input.expected_revision.as_deref(),
            CUSTOM_NAMESPACE,
            &section,
        );
    }
    append_custom_provider(input.expected_revision.as_deref(), &input)
}

pub fn update_custom(mut input: DshCustomInput) -> Result<DshSnapshot, String> {
    validate_custom(&mut input, true)?;
    mutate_settings(input.expected_revision.as_deref(), |root| {
        let namespace = root
            .get_mapping(CUSTOM_NAMESPACE)
            .ok_or_else(|| error("route-not-found", "DSH custom provider does not exist"))?;
        let providers = namespace
            .get_mapping("providers")
            .ok_or_else(|| error("route-not-found", "DSH custom provider does not exist"))?;
        let profile = providers
            .get_mapping(input.route.as_str())
            .ok_or_else(|| error("route-not-found", "DSH custom provider does not exist"))?;
        let (default_provider, default_model) = default_selection_from_yaml(root);
        validate_default_model_in_models(
            default_provider.as_deref(),
            default_model.as_deref(),
            input.route.as_str(),
            &input.models,
        )?;
        update_profile(&profile, &input)
    })
}

pub fn remove_custom(
    route: String,
    expected_revision: Option<String>,
) -> Result<DshSnapshot, String> {
    let route = route.trim().to_string();
    validate_existing_route(&route)?;
    mutate_settings(expected_revision.as_deref(), |root| {
        if let Some(default) = root.get_mapping(DEFAULT_NAMESPACE) {
            if default
                .get("provider")
                .and_then(|value| value.as_scalar().map(|scalar| scalar.as_string()))
                .as_deref()
                == Some(route.as_str())
            {
                return Err(error(
                    "provider-in-use",
                    "Select another default model before deleting this provider",
                ));
            }
        }
        let namespace = root
            .get_mapping(CUSTOM_NAMESPACE)
            .ok_or_else(|| error("route-not-found", "DSH custom provider does not exist"))?;
        let providers = namespace
            .get_mapping("providers")
            .ok_or_else(|| error("route-not-found", "DSH custom provider does not exist"))?;
        if providers.remove(route.as_str()).is_none() {
            return Err(error(
                "route-not-found",
                "DSH custom provider does not exist",
            ));
        }
        Ok(())
    })
}

pub fn set_default_model(
    selection: DshDefaultModel,
    expected_revision: Option<String>,
) -> Result<DshSnapshot, String> {
    let provider = selection.provider.trim().to_string();
    let model = selection.model.trim().to_string();
    let reasoning = normalize_optional(selection.reasoning_effort);
    if provider.is_empty() || model.is_empty() {
        return Err(error(
            "invalid-default-model",
            "Default provider and model are required",
        ));
    }
    let settings = parse_settings(&read_bytes(&paths().settings)?)?;
    if map_get(&settings, DEFAULT_NAMESPACE).is_none() {
        validate_default_selection_settings(&settings, &provider, &model)?;
        let mut section = SerdeMapping::new();
        section.insert("provider".into(), provider.clone().into());
        section.insert("model".into(), model.clone().into());
        if let Some(reasoning) = &reasoning {
            section.insert("reasoningEffort".into(), reasoning.clone().into());
        }
        return append_settings_section(expected_revision.as_deref(), DEFAULT_NAMESPACE, &section);
    }
    mutate_settings(expected_revision.as_deref(), |root| {
        validate_default_provider(root, &provider)?;
        validate_default_model_exists(root, &provider, &model)?;
        let mapping = root.get_mapping(DEFAULT_NAMESPACE).ok_or_else(|| {
            error(
                "invalid-default-model",
                "agent-default-model must be a mapping",
            )
        })?;
        mapping.set("provider", provider.as_str());
        mapping.set("model", model.as_str());
        set_or_remove(&mapping, "reasoningEffort", reasoning.as_deref());
        Ok(())
    })
}

fn mutate_credentials<F>(expected_revision: Option<&str>, mutate: F) -> Result<(), String>
where
    F: FnOnce(&Mapping) -> Result<(), String>,
{
    let paths = paths();
    ensure_secure_home(&paths.home)?;
    let _lock = FileLock::acquire(&paths.credentials)?;
    validate_credentials_permissions(&paths.credentials)?;
    let bytes = read_bytes(&paths.credentials)?;
    let revision = sha256_hex(&bytes);
    if expected_revision.is_some_and(|expected| expected != revision) {
        return Err(error(
            "credentials-conflict",
            "DSH credentials changed while the editor was open",
        ));
    }
    let file = parse_lossless(&bytes)?;
    let document = file.ensure_document();
    let root = document.as_mapping().ok_or_else(|| {
        error(
            "invalid-yaml-root",
            "DSH credentials root must be a mapping",
        )
    })?;
    mutate(&root)?;
    let output = file.to_string();
    let _ = parse_credentials(output.as_bytes())?;
    atomic_write_secure(&paths.credentials, output.as_bytes())
}

pub fn set_credential(
    reference: String,
    value: String,
    expected_revision: Option<String>,
) -> Result<(), String> {
    let reference = reference.trim().to_string();
    validate_credential_ref(&reference)?;
    if value.is_empty() {
        return Err(error("invalid-credential", "API key cannot be empty"));
    }
    if std::env::var_os(&reference).is_some_and(|value| !value.to_string_lossy().trim().is_empty())
    {
        return Err(error(
            "credential-env-shadowed",
            "This credential is supplied by the cc-switch process environment and is read-only",
        ));
    }
    mutate_credentials(expected_revision.as_deref(), |root| {
        root.set(reference.as_str(), value.as_str());
        Ok(())
    })
}

pub fn unset_credential(
    reference: String,
    expected_revision: Option<String>,
) -> Result<(), String> {
    let reference = reference.trim().to_string();
    validate_credential_ref(&reference)?;
    if std::env::var_os(&reference).is_some_and(|value| !value.to_string_lossy().trim().is_empty())
    {
        return Err(error(
            "credential-env-shadowed",
            "Environment credentials cannot be removed from this application",
        ));
    }
    mutate_credentials(expected_revision.as_deref(), |root| {
        root.remove(reference.as_str());
        Ok(())
    })
}

pub async fn discover_models(
    base_url: String,
    api: String,
    api_key: Option<String>,
    credential_ref: Option<String>,
) -> Result<DshModelDiscoveryResult, String> {
    if !matches!(api.as_str(), "openai-completions" | "openai-responses") {
        return Err(error(
            "discovery-unsupported",
            "Automatic model discovery is available only for OpenAI-compatible endpoints",
        ));
    }
    // DSH's discovery endpoint may be public or may authenticate through a
    // process/.env credential. The shared cc-switch helper requires a
    // non-empty bearer token, so use a small local request path when the
    // caller intentionally leaves the write-only key blank.
    if api_key.as_deref().is_none_or(str::is_empty) {
        if let Some(reference) = credential_ref.as_deref().filter(|value| !value.is_empty()) {
            validate_credential_ref(reference)?;
            if let Some(value) = std::env::var_os(reference)
                .map(|value| value.to_string_lossy().trim().to_string())
                .filter(|value| !value.is_empty())
            {
                return discover_models_with_key(base_url.trim(), value).await;
            }
            let credentials = read_bytes(&paths().credentials)?;
            validate_credentials_permissions(&paths().credentials)?;
            let credentials = parse_credentials(&credentials)?;
            if let Some(value) = map_get(&credentials, reference)
                .and_then(SerdeValue::as_str)
                .filter(|value| !value.is_empty())
            {
                return discover_models_with_key(base_url.trim(), value.to_string()).await;
            }
        }
        return discover_models_without_explicit_key(base_url.trim()).await;
    }
    discover_models_with_key(base_url.trim(), api_key.unwrap_or_default()).await
}

async fn discover_models_with_key(
    base_url: &str,
    api_key: String,
) -> Result<DshModelDiscoveryResult, String> {
    let models = crate::services::model_fetch::fetch_models(
        base_url,
        api_key.trim(),
        false,
        None,
        None,
        None,
        None,
    )
    .await?
    .into_iter()
    .map(|model| DshModel {
        id: model.id,
        name: None,
        description: model.owned_by,
        context_window: None,
        max_tokens: None,
        extra: BTreeMap::new(),
    })
    .collect();
    Ok(DshModelDiscoveryResult { models })
}

async fn discover_models_without_explicit_key(
    base_url: &str,
) -> Result<DshModelDiscoveryResult, String> {
    let url = format!("{}/models", base_url.trim_end_matches('/'));
    let response = crate::proxy::http_client::get()
        .get(&url)
        .header(reqwest::header::ACCEPT, "application/json")
        .timeout(Duration::from_secs(15))
        .send()
        .await
        .map_err(|source| {
            error(
                "discovery-failed",
                format!("Model discovery request failed: {source}"),
            )
        })?;
    if !response.status().is_success() {
        return Err(error(
            "discovery-failed",
            format!(
                "Model discovery endpoint returned HTTP {}",
                response.status()
            ),
        ));
    }
    let response: serde_json::Value = response.json().await.map_err(|_| {
        error(
            "discovery-failed",
            "Model discovery response was not valid JSON",
        )
    })?;
    let models = response
        .get("data")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| {
            error(
                "discovery-failed",
                "Model discovery response has no data list",
            )
        })?
        .iter()
        .filter_map(|entry| {
            let id = entry.get("id")?.as_str()?.trim();
            (!id.is_empty()).then(|| DshModel {
                id: id.to_string(),
                name: entry
                    .get("name")
                    .or_else(|| entry.get("display_name"))
                    .and_then(serde_json::Value::as_str)
                    .map(ToOwned::to_owned),
                description: entry
                    .get("owned_by")
                    .and_then(serde_json::Value::as_str)
                    .map(ToOwned::to_owned),
                context_window: entry
                    .get("context_window")
                    .or_else(|| entry.get("context_length"))
                    .and_then(serde_json::Value::as_u64),
                max_tokens: entry
                    .get("max_tokens")
                    .or_else(|| entry.get("max_output_tokens"))
                    .and_then(serde_json::Value::as_u64),
                extra: BTreeMap::new(),
            })
        })
        .collect();
    Ok(DshModelDiscoveryResult { models })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    fn with_home<T>(test: impl FnOnce(&Path) -> T) -> T {
        let dir = tempfile::tempdir().unwrap();
        let previous_home = std::env::var_os("CC_SWITCH_TEST_HOME");
        let previous_dsh = std::env::var_os("DSH_HOME");
        std::env::set_var("CC_SWITCH_TEST_HOME", dir.path());
        std::env::remove_var("DSH_HOME");
        let result = test(dir.path());
        match previous_home {
            Some(value) => std::env::set_var("CC_SWITCH_TEST_HOME", value),
            None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
        }
        match previous_dsh {
            Some(value) => std::env::set_var("DSH_HOME", value),
            None => std::env::remove_var("DSH_HOME"),
        }
        result
    }

    #[test]
    #[serial]
    fn resolves_nonempty_dsh_home_and_ignores_blank_value() {
        with_home(|home| {
            std::env::set_var("DSH_HOME", "   ");
            assert_eq!(get_home(), home.join(".dsh"));
            std::env::set_var("DSH_HOME", "~/custom-dsh");
            assert_eq!(get_home(), home.join("custom-dsh"));
        });
    }

    #[test]
    #[serial]
    fn edits_native_section_without_reformatting_siblings() {
        with_home(|_| {
            let paths = paths();
            fs::create_dir_all(&paths.home).unwrap();
            fs::write(
                &paths.settings,
                "# keep me\nunrelated:\n  enabled: true # sibling\nllm-deepseek:\n  baseURL: old # target\n",
            )
            .unwrap();
            let revision = sha256_hex(&fs::read(&paths.settings).unwrap());
            upsert_native(DshNativeInput {
                base_url: Some("https://api.deepseek.com".to_string()),
                models: None,
                api_key_env: None,
                expected_revision: Some(revision),
            })
            .unwrap();
            let output = fs::read_to_string(&paths.settings).unwrap();
            assert!(output.contains("# keep me"));
            assert!(output.contains("enabled: true # sibling"));
            assert!(output.contains("baseURL: https://api.deepseek.com # target"));
        });
    }

    #[test]
    #[serial]
    fn allows_native_catalog_edit_when_default_model_was_never_listed() {
        with_home(|_| {
            let paths = paths();
            fs::create_dir_all(&paths.home).unwrap();
            let existing = "llm-deepseek:\n  models:\n    - id: cataloged\nagent-default-model:\n  provider: deepseek-official\n  model: pass-through\n";
            fs::write(&paths.settings, existing).unwrap();

            upsert_native(DshNativeInput {
                base_url: None,
                models: Some(vec![DshModel {
                    id: "replacement".to_string(),
                    name: None,
                    description: None,
                    context_window: None,
                    max_tokens: None,
                    extra: BTreeMap::new(),
                }]),
                api_key_env: None,
                expected_revision: Some(sha256_hex(existing.as_bytes())),
            })
            .unwrap();

            let output = fs::read_to_string(&paths.settings).unwrap();
            assert!(output.contains("- id: replacement"));
            assert!(output.contains("model: pass-through"));
        });
    }

    #[test]
    #[serial]
    fn rejects_native_catalog_edit_that_removes_a_listed_default_model() {
        with_home(|_| {
            let paths = paths();
            fs::create_dir_all(&paths.home).unwrap();
            let existing = "llm-deepseek:\n  models:\n    - id: selected\nagent-default-model:\n  provider: deepseek-official\n  model: selected\n";
            fs::write(&paths.settings, existing).unwrap();

            let result = upsert_native(DshNativeInput {
                base_url: None,
                models: Some(vec![DshModel {
                    id: "replacement".to_string(),
                    name: None,
                    description: None,
                    context_window: None,
                    max_tokens: None,
                    extra: BTreeMap::new(),
                }]),
                api_key_env: None,
                expected_revision: Some(sha256_hex(existing.as_bytes())),
            });

            assert!(result.unwrap_err().contains("model-in-use"));
            assert_eq!(fs::read(&paths.settings).unwrap(), existing.as_bytes());
        });
    }

    #[test]
    #[serial]
    fn rejects_stale_revision_without_overwriting_settings() {
        with_home(|_| {
            let paths = paths();
            fs::create_dir_all(&paths.home).unwrap();
            fs::write(&paths.settings, "other: value\n").unwrap();
            let before = fs::read(&paths.settings).unwrap();
            let result = reset_native(Some("stale".to_string()));
            assert!(result.unwrap_err().contains("settings-conflict"));
            assert_eq!(fs::read(&paths.settings).unwrap(), before);
        });
    }

    #[test]
    #[serial]
    fn credential_snapshot_never_returns_the_secret() {
        with_home(|_| {
            let paths = paths();
            fs::create_dir_all(&paths.home).unwrap();
            fs::write(&paths.credentials, "DEEPSEEK_API_KEY: very-secret-value\n").unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&paths.credentials, fs::Permissions::from_mode(0o600)).unwrap();
            }
            let json = serde_json::to_string(&snapshot().unwrap()).unwrap();
            assert!(!json.contains("very-secret-value"));
            assert!(json.contains("DEEPSEEK_API_KEY"));
        });
    }

    #[test]
    #[serial]
    fn reports_process_credentials_as_configured_but_not_writable() {
        with_home(|_| {
            let key = "DSH_TEST_PROCESS_KEY";
            std::env::set_var(key, "process-secret");
            let paths = paths();
            fs::create_dir_all(&paths.home).unwrap();
            fs::write(
                &paths.settings,
                format!("llm-deepseek:\n  apiKeyEnv: {key}\n"),
            )
            .unwrap();
            let provider = snapshot()
                .unwrap()
                .providers
                .into_iter()
                .find(|provider| provider.kind == "native")
                .unwrap();
            assert_eq!(
                provider.credential.as_ref().unwrap().source.as_deref(),
                Some("process")
            );
            assert!(!provider.credential.as_ref().unwrap().writable);
            std::env::remove_var(key);
        });
    }

    #[test]
    #[serial]
    fn refuses_to_delete_the_default_custom_provider() {
        with_home(|_| {
            let paths = paths();
            fs::create_dir_all(&paths.home).unwrap();
            fs::write(
                &paths.settings,
                "llm-pi-ai:\n  providers:\n    gateway:\n      api: openai-completions\n      baseURL: https://example.test/v1\n      models:\n        - id: model\nagent-default-model:\n  provider: gateway\n  model: model\n",
            )
            .unwrap();
            let result = remove_custom("gateway".to_string(), None);
            assert!(result.unwrap_err().contains("provider-in-use"));
        });
    }

    #[test]
    #[serial]
    fn creates_secure_files_and_preserves_unknown_default_fields() {
        with_home(|_| {
            let paths = paths();
            fs::create_dir_all(&paths.home).unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&paths.home, fs::Permissions::from_mode(0o700)).unwrap();
            }
            fs::write(
                &paths.settings,
                "agent-default-model:\n  provider: deepseek-official\n  model: old\n  futureOption: keep-me # future\n",
            )
            .unwrap();
            let revision = sha256_hex(&fs::read(&paths.settings).unwrap());
            set_default_model(
                DshDefaultModel {
                    provider: NATIVE_ROUTE.to_string(),
                    model: "deepseek-v4-pro".to_string(),
                    reasoning_effort: None,
                },
                Some(revision),
            )
            .unwrap();
            let output = fs::read_to_string(&paths.settings).unwrap();
            assert!(output.contains("futureOption: keep-me # future"));
            assert!(output.contains("model: deepseek-v4-pro"));

            let credentials_revision = sha256_hex(&[]);
            set_credential(
                "NEW_KEY".to_string(),
                "secret".to_string(),
                Some(credentials_revision),
            )
            .unwrap();
            assert_eq!(
                fs::read_to_string(&paths.credentials).unwrap(),
                "NEW_KEY: secret\n"
            );

            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                assert_eq!(
                    fs::metadata(&paths.home).unwrap().permissions().mode() & 0o777,
                    0o700
                );
                assert_eq!(
                    fs::metadata(&paths.settings).unwrap().permissions().mode() & 0o777,
                    0o600
                );
                assert_eq!(
                    fs::metadata(&paths.credentials)
                        .unwrap()
                        .permissions()
                        .mode()
                        & 0o777,
                    0o600
                );
            }
        });
    }

    #[test]
    #[serial]
    fn creates_custom_provider_from_an_empty_settings_file() {
        with_home(|_| {
            let result = create_custom(DshCustomInput {
                route: "gateway".to_string(),
                display_name: Some("Gateway".to_string()),
                api: "openai-completions".to_string(),
                base_url: "https://example.test/v1".to_string(),
                models: vec![DshModel {
                    id: "model".to_string(),
                    name: None,
                    description: None,
                    context_window: None,
                    max_tokens: None,
                    extra: BTreeMap::new(),
                }],
                api_key_env: Some("GATEWAY_API_KEY".to_string()),
                expected_revision: Some(sha256_hex(&[])),
            });
            let snapshot = result.unwrap();
            let output = fs::read_to_string(paths().settings).unwrap();
            assert!(
                snapshot
                    .providers
                    .iter()
                    .any(|provider| provider.route == "gateway"),
                "written settings: {output:?}"
            );
        });
    }

    #[test]
    #[serial]
    fn appends_first_custom_namespace_without_reformatting_existing_settings() {
        with_home(|_| {
            let paths = paths();
            fs::create_dir_all(&paths.home).unwrap();
            let existing = "# header\r\nunrelated:\r\n  enabled: true # keep\r\n";
            fs::write(&paths.settings, existing).unwrap();
            let revision = sha256_hex(existing.as_bytes());
            create_custom(DshCustomInput {
                route: "gateway".to_string(),
                display_name: None,
                api: "openai-completions".to_string(),
                base_url: "https://example.test/v1".to_string(),
                models: vec![DshModel {
                    id: "model".to_string(),
                    name: None,
                    description: None,
                    context_window: None,
                    max_tokens: None,
                    extra: BTreeMap::new(),
                }],
                api_key_env: None,
                expected_revision: Some(revision),
            })
            .unwrap();
            let output = fs::read_to_string(&paths.settings).unwrap();
            assert!(output.starts_with(existing));
            assert!(output.contains("llm-pi-ai:\r\n  providers:\r\n    gateway:"));
            assert!(!output.replace("\r\n", "").contains('\n'));
        });
    }

    #[test]
    #[serial]
    fn appends_custom_route_without_reformatting_existing_providers() {
        with_home(|_| {
            let paths = paths();
            fs::create_dir_all(&paths.home).unwrap();
            let existing = "# header\r\nunrelated: keep # sibling\r\nllm-pi-ai:\r\n  futureOption: keep-me # namespace\r\n  providers:\r\n    existing:\r\n      api: openai-completions # protocol\r\n      baseURL: https://example.test/v1\r\n      futureField: keep-me # profile\r\n      models:\r\n        - id: model\r\n    # keep provider comment\r\nagent-default-model:\r\n  provider: existing\r\n  model: model\r\n";
            fs::write(&paths.settings, existing).unwrap();
            let before = fs::read(&paths.settings).unwrap();
            let snapshot = create_custom(DshCustomInput {
                route: "gateway".to_string(),
                display_name: Some("Gateway".to_string()),
                api: "openai-completions".to_string(),
                base_url: "https://example.test/v1".to_string(),
                models: vec![DshModel {
                    id: "gateway-model".to_string(),
                    name: Some("Gateway Model".to_string()),
                    description: Some("A model with all optional fields".to_string()),
                    context_window: Some(128_000),
                    max_tokens: Some(8_192),
                    extra: BTreeMap::new(),
                }],
                api_key_env: Some("GATEWAY_API_KEY".to_string()),
                expected_revision: Some(sha256_hex(&before)),
            })
            .unwrap();
            let output = fs::read_to_string(&paths.settings).unwrap();
            assert!(output.contains("# header\r\n"));
            assert!(output.contains("unrelated: keep # sibling\r\n"));
            assert!(output.contains("futureOption: keep-me # namespace\r\n"));
            assert!(output.contains("api: openai-completions # protocol\r\n"));
            assert!(output.contains("futureField: keep-me # profile\r\n"));
            assert!(output.contains("# keep provider comment\r\n"));
            assert!(output.contains("agent-default-model:\r\n  provider: existing\r\n"));
            assert!(output.contains("    gateway:\r\n"));
            assert!(output.contains("      displayName: Gateway\r\n"));
            assert!(output.contains("      apiKeyEnv: GATEWAY_API_KEY\r\n"));
            assert!(output.contains("        - id: gateway-model\r\n"));
            assert!(output.contains("          name: Gateway Model\r\n"));
            assert!(output.contains("          description: A model with all optional fields\r\n"));
            assert!(output.contains("          contextWindow: 128000\r\n"));
            assert!(output.contains("          maxTokens: 8192\r\n"));
            assert!(!output.replace("\r\n", "").contains('\n'));
            assert!(snapshot
                .providers
                .iter()
                .any(|provider| provider.route == "gateway"));
        });
    }

    #[test]
    #[serial]
    fn rejects_duplicate_custom_route_without_changing_existing_yaml() {
        with_home(|_| {
            let paths = paths();
            fs::create_dir_all(&paths.home).unwrap();
            fs::write(
                &paths.settings,
                "llm-pi-ai:\n  providers:\n    gateway:\n      api: openai-completions\n      baseURL: https://example.test/v1\n      models:\n        - id: model\n",
            )
            .unwrap();
            let before = fs::read(&paths.settings).unwrap();
            let result = create_custom(DshCustomInput {
                route: "gateway".to_string(),
                display_name: None,
                api: "openai-completions".to_string(),
                base_url: "https://other.test/v1".to_string(),
                models: vec![DshModel {
                    id: "other".to_string(),
                    name: None,
                    description: None,
                    context_window: None,
                    max_tokens: None,
                    extra: BTreeMap::new(),
                }],
                api_key_env: None,
                expected_revision: Some(sha256_hex(&before)),
            });
            assert!(result.unwrap_err().contains("route-exists"));
            assert_eq!(fs::read(&paths.settings).unwrap(), before);
        });
    }

    #[test]
    #[serial]
    fn rejects_flow_style_providers_without_changing_existing_yaml() {
        with_home(|_| {
            let paths = paths();
            fs::create_dir_all(&paths.home).unwrap();
            fs::write(
                &paths.settings,
                "llm-pi-ai:\n  providers: {existing: {api: openai-completions, baseURL: https://example.test/v1, models: [{id: model}]}}\n",
            )
            .unwrap();
            let before = fs::read(&paths.settings).unwrap();
            let result = create_custom(DshCustomInput {
                route: "gateway".to_string(),
                display_name: None,
                api: "openai-completions".to_string(),
                base_url: "https://other.test/v1".to_string(),
                models: vec![DshModel {
                    id: "other".to_string(),
                    name: None,
                    description: None,
                    context_window: None,
                    max_tokens: None,
                    extra: BTreeMap::new(),
                }],
                api_key_env: None,
                expected_revision: Some(sha256_hex(&before)),
            });
            assert!(result.unwrap_err().contains("unsafe-yaml-edit"));
            assert_eq!(fs::read(&paths.settings).unwrap(), before);
        });
    }

    #[test]
    #[serial]
    fn appends_provider_when_custom_namespace_has_no_provider_mapping() {
        with_home(|_| {
            let paths = paths();
            fs::create_dir_all(&paths.home).unwrap();
            let existing = "llm-pi-ai:\n  futureOption: keep-me # namespace\n";
            fs::write(&paths.settings, existing).unwrap();
            create_custom(DshCustomInput {
                route: "gateway".to_string(),
                display_name: None,
                api: "openai-completions".to_string(),
                base_url: "https://example.test/v1".to_string(),
                models: vec![DshModel {
                    id: "model".to_string(),
                    name: None,
                    description: None,
                    context_window: None,
                    max_tokens: None,
                    extra: BTreeMap::new(),
                }],
                api_key_env: None,
                expected_revision: Some(sha256_hex(existing.as_bytes())),
            })
            .unwrap();
            let output = fs::read_to_string(&paths.settings).unwrap();
            assert!(output.contains("futureOption: keep-me # namespace"));
            assert!(output.contains("  providers:\n    gateway:"));
            assert!(snapshot()
                .unwrap()
                .providers
                .iter()
                .any(|provider| provider.route == "gateway"));
        });
    }

    #[test]
    #[serial]
    fn updates_existing_model_catalog_without_dropping_unknown_fields_or_comments() {
        with_home(|_| {
            let paths = paths();
            fs::create_dir_all(&paths.home).unwrap();
            let existing = "llm-pi-ai:\n  providers:\n    gateway:\n      api: openai-completions\n      baseURL: https://example.test/v1\n      models:\n        # keep this model note\n        - id: old # keep this inline note\n          name: Old\n          futureField: keep-me\nagent-default-model:\n  provider: gateway\n  model: old\n";
            fs::write(&paths.settings, existing).unwrap();
            assert!(update_custom(DshCustomInput {
                route: "gateway".to_string(),
                display_name: None,
                api: "openai-completions".to_string(),
                base_url: "https://example.test/v1".to_string(),
                models: vec![DshModel {
                    id: "new".to_string(),
                    name: Some("New".to_string()),
                    description: None,
                    context_window: None,
                    max_tokens: None,
                    extra: BTreeMap::from([(
                        String::from("futureField"),
                        serde_json::json!("keep-me")
                    )]),
                }],
                api_key_env: None,
                expected_revision: Some(sha256_hex(existing.as_bytes())),
            })
            .is_err());

            // The selected default cannot be removed by a catalog edit. Keep
            // the original bytes as the failed mutation's safety guarantee.
            assert_eq!(fs::read(&paths.settings).unwrap(), existing.as_bytes());

            let revised = existing.replace("model: old", "model: new");
            fs::write(&paths.settings, &revised).unwrap();
            update_custom(DshCustomInput {
                route: "gateway".to_string(),
                display_name: None,
                api: "openai-completions".to_string(),
                base_url: "https://example.test/v1".to_string(),
                models: vec![DshModel {
                    id: "new".to_string(),
                    name: Some("New".to_string()),
                    description: None,
                    context_window: None,
                    max_tokens: None,
                    extra: BTreeMap::from([(
                        String::from("futureField"),
                        serde_json::json!("keep-me"),
                    )]),
                }],
                api_key_env: None,
                expected_revision: Some(sha256_hex(revised.as_bytes())),
            })
            .unwrap();
            let output = fs::read_to_string(&paths.settings).unwrap();
            assert!(output.contains("# keep this model note"));
            assert!(output.contains("# keep this inline note"));
            assert!(output.contains("futureField: keep-me"));
            assert!(output.contains("id: new"));
        });
    }

    #[test]
    #[serial]
    fn adds_model_catalog_to_existing_provider_without_reformatting_siblings() {
        with_home(|_| {
            let paths = paths();
            fs::create_dir_all(&paths.home).unwrap();
            let existing = "llm-pi-ai:\n  providers:\n    gateway:\n      api: openai-completions\n      baseURL: https://example.test/v1\n      futureField: keep-me # profile\n";
            fs::write(&paths.settings, existing).unwrap();
            update_custom(DshCustomInput {
                route: "gateway".to_string(),
                display_name: None,
                api: "openai-completions".to_string(),
                base_url: "https://example.test/v1".to_string(),
                models: vec![DshModel {
                    id: "model".to_string(),
                    name: Some("Model".to_string()),
                    description: None,
                    context_window: None,
                    max_tokens: None,
                    extra: BTreeMap::new(),
                }],
                api_key_env: None,
                expected_revision: Some(sha256_hex(existing.as_bytes())),
            })
            .unwrap();
            let output = fs::read_to_string(&paths.settings).unwrap();
            assert!(output.contains("futureField: keep-me # profile"));
            assert!(output.contains("      models:\n        - id: model\n          name: Model"));
            assert!(snapshot()
                .unwrap()
                .providers
                .iter()
                .any(|provider| provider.route == "gateway" && provider.model_count == 1));
        });
    }

    #[test]
    #[serial]
    fn edits_existing_nonstandard_route_keys_without_renaming_them() {
        with_home(|_| {
            let paths = paths();
            fs::create_dir_all(&paths.home).unwrap();
            let existing = "llm-pi-ai:\n  providers:\n    Legacy_GATEWAY:\n      api: openai-completions\n      baseURL: https://example.test/v1\n      models:\n        - id: model\n";
            fs::write(&paths.settings, existing).unwrap();
            update_custom(DshCustomInput {
                route: "Legacy_GATEWAY".to_string(),
                display_name: Some("Updated".to_string()),
                api: "openai-completions".to_string(),
                base_url: "https://new.example.test/v1".to_string(),
                models: vec![DshModel {
                    id: "model".to_string(),
                    name: None,
                    description: None,
                    context_window: None,
                    max_tokens: None,
                    extra: BTreeMap::new(),
                }],
                api_key_env: None,
                expected_revision: Some(sha256_hex(existing.as_bytes())),
            })
            .unwrap();
            let output = fs::read_to_string(&paths.settings).unwrap();
            assert!(output.contains("Legacy_GATEWAY:"));
            assert!(output.contains("baseURL: https://new.example.test/v1"));
            assert!(!output.contains("legacy-gateway:"));
        });
    }

    #[test]
    #[serial]
    fn credential_revision_conflict_does_not_overwrite_the_file() {
        with_home(|_| {
            let paths = paths();
            fs::create_dir_all(&paths.home).unwrap();
            fs::write(&paths.credentials, "EXISTING_KEY: original\n").unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&paths.credentials, fs::Permissions::from_mode(0o600)).unwrap();
            }
            let before = fs::read(&paths.credentials).unwrap();
            let result = set_credential(
                "EXISTING_KEY".to_string(),
                "replacement".to_string(),
                Some("stale".to_string()),
            );
            assert!(result.unwrap_err().contains("credentials-conflict"));
            assert_eq!(fs::read(&paths.credentials).unwrap(), before);
        });
    }

    #[test]
    #[serial]
    #[cfg(unix)]
    fn rejects_insecure_existing_credentials_without_rewriting_them() {
        with_home(|_| {
            use std::os::unix::fs::PermissionsExt;

            let paths = paths();
            fs::create_dir_all(&paths.home).unwrap();
            fs::write(&paths.credentials, "EXISTING_KEY: secret\n").unwrap();
            fs::set_permissions(&paths.credentials, fs::Permissions::from_mode(0o644)).unwrap();
            let before = fs::read(&paths.credentials).unwrap();

            assert!(snapshot()
                .unwrap_err()
                .contains("insecure-credentials-permissions"));
            assert!(
                set_credential("NEW_KEY".to_string(), "new-secret".to_string(), None)
                    .unwrap_err()
                    .contains("insecure-credentials-permissions")
            );
            assert_eq!(fs::read(&paths.credentials).unwrap(), before);
        });
    }

    #[test]
    #[serial]
    fn clears_native_base_url_override_when_the_field_is_emptied() {
        with_home(|_| {
            let paths = paths();
            fs::create_dir_all(&paths.home).unwrap();

            // A section that only held a base URL becomes default after the
            // clear, so the whole section is removed rather than emitting an
            // unparseable empty mapping.
            let existing = "llm-deepseek:\n  baseURL: https://example.test\n";
            fs::write(&paths.settings, existing).unwrap();
            // Whitespace-only input is the cleared signal, not "leave alone".
            upsert_native(DshNativeInput {
                base_url: Some("   ".to_string()),
                models: None,
                api_key_env: None,
                expected_revision: Some(sha256_hex(existing.as_bytes())),
            })
            .unwrap();
            let output = fs::read_to_string(&paths.settings).unwrap();
            assert!(!output.contains("baseURL"));
            assert!(!output.contains("llm-deepseek"));

            // A section with other fields keeps them and only the base URL
            // override is removed.
            let existing =
                "llm-deepseek:\n  baseURL: https://example.test\n  apiKeyEnv: CUSTOM_KEY\n";
            fs::write(&paths.settings, existing).unwrap();
            upsert_native(DshNativeInput {
                base_url: Some(String::new()),
                models: None,
                api_key_env: None,
                expected_revision: Some(sha256_hex(existing.as_bytes())),
            })
            .unwrap();
            let output = fs::read_to_string(&paths.settings).unwrap();
            assert!(!output.contains("baseURL"));
            assert!(output.contains("apiKeyEnv: CUSTOM_KEY"));
            assert!(output.contains("llm-deepseek:"));
        });
    }

    #[test]
    #[serial]
    #[cfg(unix)]
    fn reclaims_abandoned_lock_from_a_dead_process() {
        with_home(|_| {
            let paths = paths();
            fs::create_dir_all(&paths.home).unwrap();
            fs::write(
                &paths.settings,
                "llm-deepseek:\n  baseURL: https://example.test\n",
            )
            .unwrap();

            // A child that is spawned and reaped has a verifiably free PID.
            let mut child = std::process::Command::new("true").spawn().unwrap();
            let dead_pid = child.id();
            child.wait().unwrap();
            fs::write(
                &paths.settings.with_file_name("settings.yaml.lock"),
                format!("{dead_pid}\n"),
            )
            .unwrap();

            // The dead holder's lock is reclaimed instead of waiting out the
            // timeout, so the mutation succeeds immediately.
            upsert_native(DshNativeInput {
                base_url: Some("https://api.deepseek.com".to_string()),
                models: None,
                api_key_env: None,
                expected_revision: Some(sha256_hex(&fs::read(&paths.settings).unwrap())),
            })
            .unwrap();

            let output = fs::read_to_string(&paths.settings).unwrap();
            assert!(output.contains("baseURL: https://api.deepseek.com"));
            assert!(!paths.settings.with_file_name("settings.yaml.lock").exists());
        });
    }

    #[test]
    #[serial]
    fn reclaims_lock_older_than_the_stale_age() {
        with_home(|_| {
            use std::fs::FileTimes;

            let paths = paths();
            fs::create_dir_all(&paths.home).unwrap();
            fs::write(
                &paths.settings,
                "llm-deepseek:\n  baseURL: https://example.test\n",
            )
            .unwrap();

            // The live test process owns this PID, so only the age fallback
            // can reclaim the lock.
            let lock_path = paths.settings.with_file_name("settings.yaml.lock");
            fs::write(&lock_path, format!("{}\n", std::process::id())).unwrap();
            let past = SystemTime::now() - LOCK_STALE_AGE - Duration::from_secs(60);
            fs::File::options()
                .write(true)
                .open(&lock_path)
                .unwrap()
                .set_times(FileTimes::new().set_modified(past))
                .unwrap();

            upsert_native(DshNativeInput {
                base_url: Some("https://api.deepseek.com".to_string()),
                models: None,
                api_key_env: None,
                expected_revision: Some(sha256_hex(&fs::read(&paths.settings).unwrap())),
            })
            .unwrap();

            let output = fs::read_to_string(&paths.settings).unwrap();
            assert!(output.contains("baseURL: https://api.deepseek.com"));
            assert!(!lock_path.exists());
        });
    }
}
