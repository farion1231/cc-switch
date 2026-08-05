//! Thin adapter for Pi's native files.
//!
//! Pi owns account login and the active provider/model in `settings.json`.
//! CC Switch only manages explicit custom entries in `models.json`.

use crate::config::{atomic_write_private, get_home_dir};
use crate::error::AppError;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex, MutexGuard};
use url::Url;

const MAX_PI_FILE_BYTES: u64 = 1024 * 1024;
const MISSING_MODELS_REVISION: &str = "missing";
static MODELS_FILE_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

// Provider ids compiled into the pinned Pi release. They remain Pi-owned even
// when an entry with the same key appears in models.json.
const PI_BUILTIN_PROVIDER_KEYS: &[&str] = &[
    "amazon-bedrock",
    "ant-ling",
    "anthropic",
    "azure-openai-responses",
    "cerebras",
    "cloudflare-ai-gateway",
    "cloudflare-workers-ai",
    "deepseek",
    "fireworks",
    "github-copilot",
    "google",
    "google-vertex",
    "groq",
    "huggingface",
    "kimi-coding",
    "minimax",
    "minimax-cn",
    "mistral",
    "moonshotai",
    "moonshotai-cn",
    "nvidia",
    "openai",
    "openai-codex",
    "opencode",
    "opencode-go",
    "openrouter",
    "qwen-token-plan",
    "qwen-token-plan-cn",
    "radius",
    "together",
    "vercel-ai-gateway",
    "xai",
    "xiaomi",
    "xiaomi-token-plan-ams",
    "xiaomi-token-plan-cn",
    "xiaomi-token-plan-sgp",
    "zai",
    "zai-coding-cn",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PiNativeDefaults {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_dir: Option<String>,
}

pub(crate) fn get_pi_agent_dir() -> Result<PathBuf, AppError> {
    let (path, source) = match std::env::var_os("PI_CODING_AGENT_DIR") {
        Some(value) if !value.is_empty() => (
            crate::settings::resolve_override_path(value.to_string_lossy().as_ref()),
            "PI_CODING_AGENT_DIR",
        ),
        _ => match crate::settings::get_pi_override_dir() {
            Some(path) => (path, "Pi settings override"),
            None => (get_home_dir().join(".pi").join("agent"), "Pi default"),
        },
    };
    if !path.is_absolute() {
        return Err(AppError::InvalidInput(format!(
            "{source} must resolve to an absolute directory: {}",
            path.display()
        )));
    }
    Ok(path)
}

pub(crate) fn get_pi_models_path() -> Result<PathBuf, AppError> {
    Ok(get_pi_agent_dir()?.join("models.json"))
}

pub(crate) fn get_pi_settings_path() -> Result<PathBuf, AppError> {
    Ok(get_pi_agent_dir()?.join("settings.json"))
}

pub(crate) fn read_pi_native_defaults() -> Result<PiNativeDefaults, AppError> {
    let path = get_pi_settings_path()?;
    if !path.exists() {
        return Ok(PiNativeDefaults::default());
    }
    let value = read_json5_value(&path, "Pi settings")?;
    let object = value.as_object().ok_or_else(|| {
        AppError::Config(format!(
            "Pi settings root must be an object: {}",
            path.display()
        ))
    })?;
    Ok(PiNativeDefaults {
        default_provider: optional_string(object, "defaultProvider", &path)?,
        default_model: optional_string(object, "defaultModel", &path)?,
        session_dir: optional_string(object, "sessionDir", &path)?,
    })
}

pub(crate) fn read_pi_native_providers() -> Result<IndexMap<String, Value>, AppError> {
    let _guard = lock_models_file()?;
    read_pi_native_providers_locked(&get_pi_models_path()?)
}

pub(crate) fn read_pi_native_provider(provider_key: &str) -> Result<Option<Value>, AppError> {
    let _guard = lock_models_file()?;
    let path = get_pi_models_path()?;
    let document = read_models_document(&path)?;
    Ok(providers(&document, &path)?.get(provider_key).cloned())
}

pub(crate) fn pi_provider_exists(provider_key: &str) -> Result<bool, AppError> {
    let _guard = lock_models_file()?;
    let path = get_pi_models_path()?;
    let document = read_models_document(&path)?;
    Ok(providers(&document, &path)?.contains_key(provider_key))
}

pub(crate) fn insert_pi_provider(provider_key: &str, config: &Value) -> Result<bool, AppError> {
    validate_managed_provider(provider_key, config)?;
    let _guard = lock_models_file()?;
    let path = get_pi_models_path()?;
    let (mut document, expected_revision) = read_models_document_with_revision(&path)?;
    let providers = providers_mut(&mut document, &path)?;

    match providers.get(provider_key) {
        Some(current) if current == config => return Ok(false),
        Some(_) => {
            return Err(AppError::Conflict(format!(
                "Pi provider key '{provider_key}' already exists in models.json"
            )))
        }
        None => {}
    }

    providers.insert(provider_key.to_string(), config.clone());
    write_models_document(&path, &document, &expected_revision)?;
    Ok(true)
}

pub(crate) fn replace_pi_provider(
    provider_key: &str,
    expected: &Value,
    replacement: &Value,
) -> Result<(), AppError> {
    validate_managed_provider(provider_key, replacement)?;
    let _guard = lock_models_file()?;
    let path = get_pi_models_path()?;
    let (mut document, expected_revision) = read_models_document_with_revision(&path)?;
    let providers = providers_mut(&mut document, &path)?;
    let current = providers.get(provider_key).ok_or_else(|| {
        AppError::Conflict(format!(
            "Pi provider '{provider_key}' is no longer present in models.json"
        ))
    })?;
    if current != expected {
        return Err(AppError::Conflict(format!(
            "Pi provider '{provider_key}' changed outside CC Switch"
        )));
    }
    if current == replacement {
        return Ok(());
    }
    providers.insert(provider_key.to_string(), replacement.clone());
    write_models_document(&path, &document, &expected_revision)
}

pub(crate) fn remove_pi_provider(provider_key: &str) -> Result<Option<Value>, AppError> {
    remove_pi_provider_inner(provider_key, None)
}

pub(crate) fn remove_pi_provider_if_matches(
    provider_key: &str,
    expected: &Value,
) -> Result<bool, AppError> {
    remove_pi_provider_inner(provider_key, Some(expected)).map(|removed| removed.is_some())
}

fn remove_pi_provider_inner(
    provider_key: &str,
    expected: Option<&Value>,
) -> Result<Option<Value>, AppError> {
    let _guard = lock_models_file()?;
    let path = get_pi_models_path()?;
    let (mut document, expected_revision) = read_models_document_with_revision(&path)?;
    let providers = providers_mut(&mut document, &path)?;
    let Some(current) = providers.get(provider_key).cloned() else {
        return Ok(None);
    };
    if is_pi_owned_provider(provider_key, &current) {
        return Err(AppError::Conflict(format!(
            "Pi provider '{provider_key}' is managed by Pi and cannot be removed by CC Switch"
        )));
    }
    if let Err(error) = validate_managed_provider(provider_key, &current) {
        return Err(AppError::Conflict(format!(
            "Pi provider '{provider_key}' changed outside CC Switch and is no longer supported: {error}"
        )));
    }
    if expected.is_some_and(|expected| current != *expected) {
        return Err(AppError::Conflict(format!(
            "Pi provider '{provider_key}' changed outside CC Switch"
        )));
    }
    providers.remove(provider_key);
    write_models_document(&path, &document, &expected_revision)?;
    Ok(Some(current))
}

pub(crate) fn restore_pi_provider_if_missing(
    provider_key: &str,
    config: &Value,
) -> Result<(), AppError> {
    let _guard = lock_models_file()?;
    let path = get_pi_models_path()?;
    let (mut document, expected_revision) = read_models_document_with_revision(&path)?;
    let providers = providers_mut(&mut document, &path)?;
    match providers.get(provider_key) {
        Some(current) if current == config => Ok(()),
        Some(_) => Err(AppError::Conflict(format!(
            "cannot restore Pi provider '{provider_key}' because another value now owns the key"
        ))),
        None => {
            providers.insert(provider_key.to_string(), config.clone());
            write_models_document(&path, &document, &expected_revision)
        }
    }
}

pub(crate) fn is_pi_builtin_provider_key(provider_key: &str) -> bool {
    PI_BUILTIN_PROVIDER_KEYS.contains(&provider_key)
}

pub(crate) fn is_pi_owned_provider(provider_key: &str, config: &Value) -> bool {
    is_pi_builtin_provider_key(provider_key)
        || config
            .as_object()
            .is_some_and(|object| object.contains_key("oauth"))
}

pub(crate) fn validate_managed_provider(
    provider_key: &str,
    config: &Value,
) -> Result<(), AppError> {
    if provider_key.trim().is_empty() {
        return Err(AppError::InvalidInput(
            "Pi provider key cannot be empty".to_string(),
        ));
    }
    if is_pi_builtin_provider_key(provider_key) {
        return Err(AppError::InvalidInput(format!(
            "Pi built-in provider '{provider_key}' is managed by Pi"
        )));
    }

    let provider = config.as_object().ok_or_else(|| {
        AppError::InvalidInput("Pi provider configuration must be an object".to_string())
    })?;
    if provider.contains_key("oauth") {
        return Err(AppError::InvalidInput(
            "Pi OAuth providers are managed by Pi /login".to_string(),
        ));
    }
    validate_optional_string(provider, "name")?;
    validate_optional_string(provider, "apiKey")?;
    validate_headers(provider.get("headers"), "provider headers")?;

    let models = provider
        .get("models")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            AppError::InvalidInput("Pi provider must contain a non-empty models array".to_string())
        })?;
    if models.is_empty() {
        return Err(AppError::InvalidInput(
            "Pi provider must contain at least one model".to_string(),
        ));
    }

    let provider_api = nonempty_string(provider.get("api"));
    let provider_url = nonempty_string(provider.get("baseUrl"));
    if let Some(provider_url) = provider_url {
        validate_http_url(provider_url, "provider baseUrl")?;
    }

    let mut model_ids = HashSet::with_capacity(models.len());
    for (index, model) in models.iter().enumerate() {
        let model = model.as_object().ok_or_else(|| {
            AppError::InvalidInput(format!("Pi model at index {index} must be an object"))
        })?;
        let id = nonempty_string(model.get("id")).ok_or_else(|| {
            AppError::InvalidInput(format!(
                "Pi model at index {index} must have a non-empty id"
            ))
        })?;
        if !model_ids.insert(id) {
            return Err(AppError::InvalidInput(format!(
                "Pi provider contains duplicate model id '{id}'"
            )));
        }
        validate_optional_string(model, "name")?;
        validate_optional_bool(model, "reasoning")?;
        validate_positive_number(model, "contextWindow", id)?;
        validate_positive_number(model, "maxTokens", id)?;
        nonempty_string(model.get("api"))
            .or(provider_api)
            .ok_or_else(|| {
                AppError::InvalidInput(format!("Pi model '{id}' has no effective interface format"))
            })?;
        let model_url = nonempty_string(model.get("baseUrl"))
            .or(provider_url)
            .ok_or_else(|| {
                AppError::InvalidInput(format!("Pi model '{id}' has no effective request URL"))
            })?;
        validate_http_url(model_url, &format!("model '{id}' baseUrl"))?;

        if let Some(input) = model.get("input") {
            let valid = input
                .as_array()
                .is_some_and(|values| !values.is_empty() && values.iter().all(Value::is_string));
            if !valid {
                return Err(AppError::InvalidInput(format!(
                    "Pi model '{id}' input must be a non-empty string array"
                )));
            }
        }
    }
    Ok(())
}

pub(crate) fn provider_base_url(config: &Value) -> Result<String, AppError> {
    let provider = config.as_object().ok_or_else(|| {
        AppError::InvalidInput("Pi provider configuration must be an object".to_string())
    })?;
    nonempty_string(provider.get("baseUrl"))
        .or_else(|| {
            provider
                .get("models")
                .and_then(Value::as_array)
                .and_then(|models| {
                    models
                        .iter()
                        .find_map(|model| nonempty_string(model.get("baseUrl")))
                })
        })
        .map(str::to_string)
        .ok_or_else(|| AppError::InvalidInput("Pi provider has no request URL".to_string()))
}

pub(crate) fn provider_contains_model(config: &Value, model_id: &str) -> bool {
    config
        .get("models")
        .and_then(Value::as_array)
        .is_some_and(|models| {
            models
                .iter()
                .any(|model| model.get("id").and_then(Value::as_str) == Some(model_id))
        })
}

fn lock_models_file() -> Result<MutexGuard<'static, ()>, AppError> {
    MODELS_FILE_LOCK
        .lock()
        .map_err(|error| AppError::Config(format!("Pi models file lock is poisoned: {error}")))
}

fn read_pi_native_providers_locked(path: &Path) -> Result<IndexMap<String, Value>, AppError> {
    let document = read_models_document(path)?;
    let providers = providers(&document, path)?;
    Ok(providers
        .iter()
        .map(|(provider_key, config)| (provider_key.clone(), config.clone()))
        .collect())
}

fn read_models_document(path: &Path) -> Result<Value, AppError> {
    read_models_document_with_revision(path).map(|(document, _)| document)
}

fn read_models_document_with_revision(path: &Path) -> Result<(Value, String), AppError> {
    if !path.exists() {
        return Ok((
            Value::Object(Map::new()),
            MISSING_MODELS_REVISION.to_string(),
        ));
    }
    let bytes = read_file_limited(path, "Pi models")?;
    let revision = revision(&bytes);
    let document = parse_json5_value(path, "Pi models", bytes)?;
    Ok((document, revision))
}

fn read_json5_value(path: &Path, label: &str) -> Result<Value, AppError> {
    parse_json5_value(path, label, read_file_limited(path, label)?)
}

fn read_file_limited(path: &Path, label: &str) -> Result<Vec<u8>, AppError> {
    let file = fs::File::open(path).map_err(|error| AppError::io(path, error))?;
    let metadata = file.metadata().map_err(|error| AppError::io(path, error))?;
    if metadata.len() > MAX_PI_FILE_BYTES {
        return Err(AppError::InvalidInput(format!(
            "{label} file exceeds the 1 MiB limit: {}",
            path.display()
        )));
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(MAX_PI_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| AppError::io(path, error))?;
    if bytes.len() as u64 > MAX_PI_FILE_BYTES {
        return Err(AppError::InvalidInput(format!(
            "{label} file exceeds the 1 MiB limit: {}",
            path.display()
        )));
    }
    Ok(bytes)
}

fn parse_json5_value(path: &Path, label: &str, bytes: Vec<u8>) -> Result<Value, AppError> {
    let source = String::from_utf8(bytes).map_err(|error| {
        AppError::Config(format!(
            "{label} file must be UTF-8 ({}): {error}",
            path.display()
        ))
    })?;
    json5::from_str(&source).map_err(|error| {
        AppError::Config(format!(
            "{label} file is not valid JSON/JSONC ({}): {error}",
            path.display()
        ))
    })
}

fn providers<'a>(document: &'a Value, path: &Path) -> Result<&'a Map<String, Value>, AppError> {
    let root = document.as_object().ok_or_else(|| {
        AppError::Config(format!(
            "Pi models root must be an object: {}",
            path.display()
        ))
    })?;
    match root.get("providers") {
        None => Ok(empty_json_object()),
        Some(Value::Object(providers)) => Ok(providers),
        Some(_) => Err(AppError::Config(format!(
            "Pi models 'providers' must be an object: {}",
            path.display()
        ))),
    }
}

fn providers_mut<'a>(
    document: &'a mut Value,
    path: &Path,
) -> Result<&'a mut Map<String, Value>, AppError> {
    let root = document.as_object_mut().ok_or_else(|| {
        AppError::Config(format!(
            "Pi models root must be an object: {}",
            path.display()
        ))
    })?;
    let value = root
        .entry("providers".to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    value.as_object_mut().ok_or_else(|| {
        AppError::Config(format!(
            "Pi models 'providers' must be an object: {}",
            path.display()
        ))
    })
}

fn empty_json_object() -> &'static Map<String, Value> {
    static EMPTY: LazyLock<Map<String, Value>> = LazyLock::new(Map::new);
    &EMPTY
}

fn write_models_document(
    path: &Path,
    document: &Value,
    expected_revision: &str,
) -> Result<(), AppError> {
    let mut bytes =
        serde_json::to_vec_pretty(document).map_err(|source| AppError::JsonSerialize { source })?;
    bytes.push(b'\n');
    ensure_private_models_parent(path)?;
    ensure_models_revision(path, expected_revision)?;
    atomic_write_private(path, &bytes)
}

fn ensure_models_revision(path: &Path, expected_revision: &str) -> Result<(), AppError> {
    let actual_revision = match fs::File::open(path) {
        Ok(_) => revision(&read_file_limited(path, "Pi models")?),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            MISSING_MODELS_REVISION.to_string()
        }
        Err(error) => return Err(AppError::io(path, error)),
    };
    if actual_revision == expected_revision {
        Ok(())
    } else {
        Err(AppError::Conflict(format!(
            "Pi models.json changed outside CC Switch: {}",
            path.display()
        )))
    }
}

fn revision(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn ensure_private_models_parent(path: &Path) -> Result<(), AppError> {
    let parent = path.parent().ok_or_else(|| {
        AppError::Config(format!(
            "Pi models path has no parent directory: {}",
            path.display()
        ))
    })?;
    let created = !parent.exists();
    fs::create_dir_all(parent).map_err(|source| AppError::io(parent, source))?;

    #[cfg(not(unix))]
    let _ = created;

    #[cfg(unix)]
    if created {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(parent, fs::Permissions::from_mode(0o700))
            .map_err(|source| AppError::io(parent, source))?;
    }

    Ok(())
}

fn optional_string(
    object: &Map<String, Value>,
    key: &str,
    path: &Path,
) -> Result<Option<String>, AppError> {
    match object.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value.clone())),
        Some(_) => Err(AppError::Config(format!(
            "Pi settings '{key}' must be a string: {}",
            path.display()
        ))),
    }
}

fn validate_optional_string(object: &Map<String, Value>, key: &str) -> Result<(), AppError> {
    match object.get(key) {
        None => Ok(()),
        Some(Value::String(value)) if !value.is_empty() => Ok(()),
        Some(Value::String(_)) => Err(AppError::InvalidInput(format!(
            "Pi provider field '{key}' cannot be empty"
        ))),
        Some(_) => Err(AppError::InvalidInput(format!(
            "Pi provider field '{key}' must be a string"
        ))),
    }
}

fn validate_optional_bool(object: &Map<String, Value>, key: &str) -> Result<(), AppError> {
    match object.get(key) {
        None | Some(Value::Bool(_)) => Ok(()),
        Some(_) => Err(AppError::InvalidInput(format!(
            "Pi provider field '{key}' must be a boolean"
        ))),
    }
}

fn validate_headers(value: Option<&Value>, label: &str) -> Result<(), AppError> {
    match value {
        None => Ok(()),
        Some(Value::Object(headers)) if headers.values().all(Value::is_string) => Ok(()),
        Some(_) => Err(AppError::InvalidInput(format!(
            "Pi {label} must be an object with string values"
        ))),
    }
}

fn validate_positive_number(
    model: &Map<String, Value>,
    field: &str,
    model_id: &str,
) -> Result<(), AppError> {
    match model.get(field) {
        None => Ok(()),
        Some(Value::Number(value)) if value.as_f64().is_some_and(|value| value > 0.0) => Ok(()),
        Some(_) => Err(AppError::InvalidInput(format!(
            "Pi model '{model_id}' field '{field}' must be a positive number"
        ))),
    }
}

fn nonempty_string(value: Option<&Value>) -> Option<&str> {
    value
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
}

fn validate_http_url(value: &str, label: &str) -> Result<(), AppError> {
    let parsed = Url::parse(value)
        .map_err(|error| AppError::InvalidInput(format!("Pi {label} is invalid: {error}")))?;
    if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
        return Err(AppError::InvalidInput(format!(
            "Pi {label} must be an absolute HTTP(S) URL"
        )));
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod test_support {
    use std::ffi::OsString;

    pub(crate) struct TestAgentDir {
        _dir: tempfile::TempDir,
        previous: Option<OsString>,
    }

    impl TestAgentDir {
        pub(crate) fn new() -> Self {
            let dir = tempfile::tempdir().expect("create Pi test directory");
            let agent_dir = dir.path().join("agent");
            let previous = std::env::var_os("PI_CODING_AGENT_DIR");
            std::env::set_var("PI_CODING_AGENT_DIR", &agent_dir);
            Self {
                _dir: dir,
                previous,
            }
        }
    }

    impl Drop for TestAgentDir {
        fn drop(&mut self) {
            match &self.previous {
                Some(value) => std::env::set_var("PI_CODING_AGENT_DIR", value),
                None => std::env::remove_var("PI_CODING_AGENT_DIR"),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use serial_test::serial;

    fn provider() -> Value {
        json!({
            "name": "Example",
            "baseUrl": "https://api.example.com/v1",
            "api": "openai-completions",
            "apiKey": "secret",
            "models": [{"id": "example-model"}]
        })
    }

    #[test]
    fn managed_provider_keeps_unknown_native_fields() {
        let mut value = provider();
        value["sdkOption"] = json!({"timeout": 30});
        value["models"][0]["compat"] = json!({"supportsDeveloperRole": true});
        validate_managed_provider("cc-switch-example", &value).expect("valid provider");
    }

    #[test]
    fn managed_provider_rejects_oauth_and_invalid_effective_models() {
        let mut oauth = provider();
        oauth["oauth"] = json!("anthropic");
        assert!(validate_managed_provider("cc-switch-example", &oauth).is_err());
        assert!(validate_managed_provider("anthropic", &provider()).is_err());

        let mut missing_api = provider();
        missing_api.as_object_mut().expect("object").remove("api");
        assert!(validate_managed_provider("cc-switch-example", &missing_api).is_err());

        missing_api["models"][0]["api"] = json!("openai-completions");
        missing_api["models"][0]["baseUrl"] = json!("https://model.example.com/v1");
        missing_api
            .as_object_mut()
            .expect("object")
            .remove("baseUrl");
        validate_managed_provider("cc-switch-example", &missing_api)
            .expect("per-model native overrides supply the effective values");
        assert_eq!(
            provider_base_url(&missing_api).expect("resolve model-level request URL"),
            "https://model.example.com/v1"
        );
    }

    #[test]
    #[serial]
    fn relative_agent_directory_is_rejected() {
        let _agent = test_support::TestAgentDir::new();
        std::env::set_var("PI_CODING_AGENT_DIR", "relative/pi-agent");

        let error = get_pi_agent_dir().expect_err("relative Pi directory must be rejected");
        assert!(error.to_string().contains("absolute directory"));
    }

    #[cfg(unix)]
    #[test]
    #[serial]
    fn newly_created_models_file_and_agent_directory_are_private() {
        use std::os::unix::fs::PermissionsExt;

        let _agent = test_support::TestAgentDir::new();
        insert_pi_provider("cc-switch-private", &provider()).expect("write private models file");

        let path = get_pi_models_path().expect("models path");
        let file_mode = fs::metadata(&path)
            .expect("models metadata")
            .permissions()
            .mode()
            & 0o777;
        let directory_mode = fs::metadata(path.parent().expect("agent directory"))
            .expect("agent directory metadata")
            .permissions()
            .mode()
            & 0o777;

        assert_eq!(file_mode, 0o600);
        assert_eq!(directory_mode, 0o700);
    }

    #[test]
    #[serial]
    fn stale_models_revision_does_not_overwrite_an_external_edit() {
        let _agent = test_support::TestAgentDir::new();
        let path = get_pi_models_path().expect("models path");
        ensure_private_models_parent(&path).expect("create agent directory");
        fs::write(&path, r#"{"providers":{"external":{"models":[]}}}"#)
            .expect("write initial models");
        let (_, stale_revision) =
            read_models_document_with_revision(&path).expect("read models revision");

        let external = r#"{"providers":{"external":{"models":[]},"pi-added":{"models":[]}}}"#;
        fs::write(&path, external).expect("edit models externally");

        let replacement = json!({"providers": {"cc-switch": provider()}});
        let error = write_models_document(&path, &replacement, &stale_revision)
            .expect_err("stale write must fail");
        assert!(matches!(error, AppError::Conflict(_)));
        assert_eq!(
            fs::read_to_string(path).expect("read external models"),
            external
        );
    }
}
