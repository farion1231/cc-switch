//! Thin adapter for the Pi family's native files.
//!
//! Pi and RPI ship the same layout (`models.json`, `settings.json`) in separate
//! directories. Pi owns account login and the active provider/model in
//! `settings.json`; CC Switch only manages explicit provider entries in
//! `models.json`. Every path here is resolved from the `AppType`, so a write for
//! one app can never land in the other app's tree.

use crate::app_config::AppType;
use crate::config::{atomic_write_private, get_home_dir};
use crate::error::AppError;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex, MutexGuard};

const MAX_PI_FILE_BYTES: u64 = 1024 * 1024;
const MISSING_MODELS_REVISION: &str = "missing";
static MODELS_FILE_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));
#[cfg(test)]
static TEST_AGENT_DIR: LazyLock<Mutex<Option<PathBuf>>> = LazyLock::new(|| Mutex::new(None));
#[cfg(test)]
static TEST_RPI_AGENT_DIR: LazyLock<Mutex<Option<PathBuf>>> = LazyLock::new(|| Mutex::new(None));

/// Product name used in user-facing messages and log lines.
pub(crate) fn display_name(app: &AppType) -> &'static str {
    match app {
        AppType::Rpi => "RPI",
        _ => "Pi",
    }
}

/// Directory holding the app's native Pi-family files.
pub(crate) fn agent_dir(app: &AppType) -> Result<PathBuf, AppError> {
    match app {
        AppType::Pi => get_pi_agent_dir(),
        AppType::Rpi => get_rpi_agent_dir(),
        other => Err(AppError::InvalidInput(format!(
            "{} is not a Pi-family application",
            other.as_str()
        ))),
    }
}

pub(crate) fn models_path(app: &AppType) -> Result<PathBuf, AppError> {
    Ok(agent_dir(app)?.join("models.json"))
}

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
    #[cfg(test)]
    if let Some(path) = TEST_AGENT_DIR
        .lock()
        .expect("lock Pi test directory")
        .clone()
    {
        return resolve_pi_agent_dir(Some(path), None, get_home_dir().join(".pi").join("agent"));
    }

    resolve_pi_agent_dir(
        crate::settings::get_pi_override_dir(),
        std::env::var_os("PI_CODING_AGENT_DIR"),
        get_home_dir().join(".pi").join("agent"),
    )
}

fn get_rpi_agent_dir() -> Result<PathBuf, AppError> {
    #[cfg(test)]
    if let Some(path) = TEST_RPI_AGENT_DIR
        .lock()
        .expect("lock RPI test directory")
        .clone()
    {
        return resolve_rpi_agent_dir(Some(path), None, get_home_dir().join(".rpi").join("agent"));
    }

    resolve_rpi_agent_dir(
        crate::settings::get_rpi_override_dir(),
        std::env::var_os("RPI_CODING_AGENT_DIR"),
        get_home_dir().join(".rpi").join("agent"),
    )
}

fn resolve_pi_agent_dir(
    settings_override: Option<PathBuf>,
    env_override: Option<std::ffi::OsString>,
    default_path: PathBuf,
) -> Result<PathBuf, AppError> {
    let (path, source) = match settings_override {
        Some(path) => (path, "Pi settings override"),
        None => match env_override {
            Some(value) if !value.is_empty() => (
                crate::settings::resolve_override_path(value.to_string_lossy().as_ref()),
                "PI_CODING_AGENT_DIR",
            ),
            _ => (default_path, "Pi default"),
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

fn resolve_rpi_agent_dir(
    settings_override: Option<PathBuf>,
    env_override: Option<std::ffi::OsString>,
    default_path: PathBuf,
) -> Result<PathBuf, AppError> {
    let (path, source) = match settings_override {
        Some(path) => (path, "RPI settings override"),
        None => match env_override {
            Some(value) if !value.is_empty() => (
                crate::settings::resolve_override_path(value.to_string_lossy().as_ref()),
                "RPI_CODING_AGENT_DIR",
            ),
            _ => (default_path, "RPI default"),
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

#[cfg(test)]
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

pub(crate) fn read_native_providers(app: &AppType) -> Result<IndexMap<String, Value>, AppError> {
    let _guard = lock_models_file()?;
    read_native_providers_locked(app, &models_path(app)?)
}

pub(crate) fn read_native_provider(
    app: &AppType,
    provider_key: &str,
) -> Result<Option<Value>, AppError> {
    let _guard = lock_models_file()?;
    let path = models_path(app)?;
    let document = read_models_document(app, &path)?;
    Ok(providers(app, &document, &path)?.get(provider_key).cloned())
}

pub(crate) fn provider_exists(app: &AppType, provider_key: &str) -> Result<bool, AppError> {
    let _guard = lock_models_file()?;
    let path = models_path(app)?;
    let document = read_models_document(app, &path)?;
    Ok(providers(app, &document, &path)?.contains_key(provider_key))
}

pub(crate) fn insert_provider(
    app: &AppType,
    provider_key: &str,
    config: &Value,
) -> Result<bool, AppError> {
    validate_provider_node(app, provider_key, config)?;
    let _guard = lock_models_file()?;
    let path = models_path(app)?;
    let (mut document, expected_revision) = read_models_document_with_revision(app, &path)?;
    let providers = providers_mut(app, &mut document, &path)?;

    match providers.get(provider_key) {
        Some(current) if current == config => return Ok(false),
        Some(_) => {
            return Err(AppError::InvalidInput(format!(
                "{} provider key '{provider_key}' already exists in models.json",
                display_name(app)
            )))
        }
        None => {}
    }

    providers.insert(provider_key.to_string(), config.clone());
    write_models_document(app, &path, &document, &expected_revision)?;
    Ok(true)
}

pub(crate) fn replace_provider(
    app: &AppType,
    provider_key: &str,
    expected: &Value,
    replacement: &Value,
) -> Result<(), AppError> {
    validate_provider_node(app, provider_key, replacement)?;
    let _guard = lock_models_file()?;
    let path = models_path(app)?;
    let (mut document, expected_revision) = read_models_document_with_revision(app, &path)?;
    let providers = providers_mut(app, &mut document, &path)?;
    let current = providers.get(provider_key).ok_or_else(|| {
        AppError::Conflict(format!(
            "{} provider '{provider_key}' is no longer present in models.json",
            display_name(app)
        ))
    })?;
    if current != expected {
        return Err(AppError::Conflict(format!(
            "{} provider '{provider_key}' changed outside CC Switch",
            display_name(app)
        )));
    }
    if current == replacement {
        return Ok(());
    }
    providers.insert(provider_key.to_string(), replacement.clone());
    write_models_document(app, &path, &document, &expected_revision)
}

pub(crate) fn replace_provider_if_present(
    app: &AppType,
    provider_key: &str,
    replacement: &Value,
) -> Result<Option<Value>, AppError> {
    validate_provider_node(app, provider_key, replacement)?;
    let _guard = lock_models_file()?;
    let path = models_path(app)?;
    let (mut document, expected_revision) = read_models_document_with_revision(app, &path)?;
    let providers = providers_mut(app, &mut document, &path)?;
    let Some(current) = providers.get(provider_key).cloned() else {
        return Ok(None);
    };
    if current == *replacement {
        return Ok(Some(current));
    }
    providers.insert(provider_key.to_string(), replacement.clone());
    write_models_document(app, &path, &document, &expected_revision)?;
    Ok(Some(current))
}

pub(crate) fn remove_provider(
    app: &AppType,
    provider_key: &str,
) -> Result<Option<Value>, AppError> {
    remove_provider_inner(app, provider_key, None)
}

pub(crate) fn remove_provider_if_matches(
    app: &AppType,
    provider_key: &str,
    expected: &Value,
) -> Result<bool, AppError> {
    remove_provider_inner(app, provider_key, Some(expected)).map(|removed| removed.is_some())
}

fn remove_provider_inner(
    app: &AppType,
    provider_key: &str,
    expected: Option<&Value>,
) -> Result<Option<Value>, AppError> {
    let _guard = lock_models_file()?;
    let path = models_path(app)?;
    let (mut document, expected_revision) = read_models_document_with_revision(app, &path)?;
    let providers = providers_mut(app, &mut document, &path)?;
    let Some(current) = providers.get(provider_key).cloned() else {
        return Ok(None);
    };
    if expected.is_some_and(|expected| current != *expected) {
        return Err(AppError::Conflict(format!(
            "{} provider '{provider_key}' changed outside CC Switch",
            display_name(app)
        )));
    }
    providers.remove(provider_key);
    write_models_document(app, &path, &document, &expected_revision)?;
    Ok(Some(current))
}

pub(crate) fn restore_provider_if_missing(
    app: &AppType,
    provider_key: &str,
    config: &Value,
) -> Result<(), AppError> {
    let _guard = lock_models_file()?;
    let path = models_path(app)?;
    let (mut document, expected_revision) = read_models_document_with_revision(app, &path)?;
    let providers = providers_mut(app, &mut document, &path)?;
    match providers.get(provider_key) {
        Some(current) if current == config => Ok(()),
        Some(_) => Err(AppError::Conflict(format!(
            "cannot restore {} provider '{provider_key}' because another value now owns the key",
            display_name(app)
        ))),
        None => {
            providers.insert(provider_key.to_string(), config.clone());
            write_models_document(app, &path, &document, &expected_revision)
        }
    }
}

// --- Pi-specific wrappers (Pi state, sessions and deeplinks keep using these) ---

pub(crate) fn read_pi_native_providers() -> Result<IndexMap<String, Value>, AppError> {
    read_native_providers(&AppType::Pi)
}

#[cfg(test)]
pub(crate) fn read_pi_native_provider(provider_key: &str) -> Result<Option<Value>, AppError> {
    read_native_provider(&AppType::Pi, provider_key)
}

pub(crate) fn pi_provider_exists(provider_key: &str) -> Result<bool, AppError> {
    provider_exists(&AppType::Pi, provider_key)
}

/// Validate the shape CC Switch can persist as one
/// `models.json.providers.<provider_key>` node.
///
/// Provider ownership is intentionally source-based: every explicit object in
/// `models.json.providers` is manageable, including keys also built into Pi.
/// Pi's `/login` credentials live in `auth.json` and are never read here.
pub(crate) fn validate_provider_node(
    app: &AppType,
    provider_key: &str,
    config: &Value,
) -> Result<(), AppError> {
    if provider_key.trim().is_empty() {
        return Err(AppError::InvalidInput(format!(
            "{} provider key cannot be empty",
            display_name(app)
        )));
    }
    config.as_object().ok_or_else(|| {
        AppError::InvalidInput(format!(
            "{} provider configuration must be an object",
            display_name(app)
        ))
    })?;
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

fn lock_models_file() -> Result<MutexGuard<'static, ()>, AppError> {
    MODELS_FILE_LOCK
        .lock()
        .map_err(|error| AppError::Config(format!("Pi models file lock is poisoned: {error}")))
}

fn read_native_providers_locked(
    app: &AppType,
    path: &Path,
) -> Result<IndexMap<String, Value>, AppError> {
    let document = read_models_document(app, path)?;
    let providers = providers(app, &document, path)?;
    Ok(providers
        .iter()
        .map(|(provider_key, config)| (provider_key.clone(), config.clone()))
        .collect())
}

fn read_models_document(app: &AppType, path: &Path) -> Result<Value, AppError> {
    read_models_document_with_revision(app, path).map(|(document, _)| document)
}

fn read_models_document_with_revision(
    app: &AppType,
    path: &Path,
) -> Result<(Value, String), AppError> {
    if !path.exists() {
        return Ok((
            Value::Object(Map::new()),
            MISSING_MODELS_REVISION.to_string(),
        ));
    }
    let label = format!("{} models", display_name(app));
    let bytes = read_file_limited(path, &label)?;
    let revision = revision(&bytes);
    let document = parse_json5_value(path, &label, bytes)?;
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

fn providers<'a>(
    app: &AppType,
    document: &'a Value,
    path: &Path,
) -> Result<&'a Map<String, Value>, AppError> {
    let root = document.as_object().ok_or_else(|| {
        AppError::Config(format!(
            "{} models root must be an object: {}",
            display_name(app),
            path.display()
        ))
    })?;
    match root.get("providers") {
        None => Ok(empty_json_object()),
        Some(Value::Object(providers)) => Ok(providers),
        Some(_) => Err(AppError::Config(format!(
            "{} models 'providers' must be an object: {}",
            display_name(app),
            path.display()
        ))),
    }
}

fn providers_mut<'a>(
    app: &AppType,
    document: &'a mut Value,
    path: &Path,
) -> Result<&'a mut Map<String, Value>, AppError> {
    let root = document.as_object_mut().ok_or_else(|| {
        AppError::Config(format!(
            "{} models root must be an object: {}",
            display_name(app),
            path.display()
        ))
    })?;
    let value = root
        .entry("providers".to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    value.as_object_mut().ok_or_else(|| {
        AppError::Config(format!(
            "{} models 'providers' must be an object: {}",
            display_name(app),
            path.display()
        ))
    })
}

fn empty_json_object() -> &'static Map<String, Value> {
    static EMPTY: LazyLock<Map<String, Value>> = LazyLock::new(Map::new);
    &EMPTY
}

fn write_models_document(
    app: &AppType,
    path: &Path,
    document: &Value,
    expected_revision: &str,
) -> Result<(), AppError> {
    let mut bytes =
        serde_json::to_vec_pretty(document).map_err(|source| AppError::JsonSerialize { source })?;
    bytes.push(b'\n');
    ensure_private_models_parent(app, path)?;
    ensure_models_revision(app, path, expected_revision)?;
    atomic_write_private(path, &bytes)
}

fn ensure_models_revision(
    app: &AppType,
    path: &Path,
    expected_revision: &str,
) -> Result<(), AppError> {
    let label = format!("{} models", display_name(app));
    let actual_revision = match fs::File::open(path) {
        Ok(_) => revision(&read_file_limited(path, &label)?),
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

fn ensure_private_models_parent(app: &AppType, path: &Path) -> Result<(), AppError> {
    let parent = path.parent().ok_or_else(|| {
        AppError::Config(format!(
            "{} models path has no parent directory: {}",
            display_name(app),
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

fn nonempty_string(value: Option<&Value>) -> Option<&str> {
    value
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
pub(crate) mod test_support {
    use std::path::{Path, PathBuf};

    pub(crate) struct TestAgentDir {
        _dir: Option<tempfile::TempDir>,
        previous: Option<PathBuf>,
    }

    impl TestAgentDir {
        pub(crate) fn new() -> Self {
            let dir = tempfile::tempdir().expect("create Pi test directory");
            let agent_dir = dir.path().join("agent");
            Self::set(agent_dir, Some(dir))
        }

        pub(crate) fn at(agent_dir: &Path) -> Self {
            Self::set(agent_dir.to_path_buf(), None)
        }

        fn set(agent_dir: PathBuf, dir: Option<tempfile::TempDir>) -> Self {
            let previous = super::TEST_AGENT_DIR
                .lock()
                .expect("lock Pi test directory")
                .replace(agent_dir);
            Self {
                _dir: dir,
                previous,
            }
        }
    }

    impl Drop for TestAgentDir {
        fn drop(&mut self) {
            *super::TEST_AGENT_DIR
                .lock()
                .expect("lock Pi test directory") = self.previous.take();
        }
    }

    /// Same isolation as [`TestAgentDir`], pointing at the RPI tree.
    pub(crate) struct TestRpiAgentDir {
        _dir: Option<tempfile::TempDir>,
        previous: Option<PathBuf>,
    }

    impl TestRpiAgentDir {
        pub(crate) fn new() -> Self {
            let dir = tempfile::tempdir().expect("create RPI test directory");
            let agent_dir = dir.path().join("agent");
            let previous = super::TEST_RPI_AGENT_DIR
                .lock()
                .expect("lock RPI test directory")
                .replace(agent_dir);
            Self {
                _dir: Some(dir),
                previous,
            }
        }
    }

    impl Drop for TestRpiAgentDir {
        fn drop(&mut self) {
            *super::TEST_RPI_AGENT_DIR
                .lock()
                .expect("lock RPI test directory") = self.previous.take();
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
    fn provider_node_accepts_unknown_native_fields() {
        let mut value = provider();
        value["sdkOption"] = json!({"timeout": 30});
        value["models"][0]["compat"] = json!({"supportsDeveloperRole": true});
        validate_provider_node(&AppType::Pi, "cc-switch-example", &value).expect("valid provider");
    }

    #[test]
    fn provider_node_ownership_depends_on_models_json_membership() {
        let mut oauth = provider();
        oauth["oauth"] = json!("anthropic");
        validate_provider_node(&AppType::Pi, "cc-switch-example", &oauth)
            .expect("an explicit models.json node stays manageable");
        validate_provider_node(&AppType::Pi, "anthropic", &json!({}))
            .expect("a built-in provider key may be explicitly configured");
        assert!(validate_provider_node(&AppType::Pi, "", &json!({})).is_err());
        assert!(validate_provider_node(&AppType::Pi, "anthropic", &json!("invalid")).is_err());
    }

    #[test]
    fn relative_agent_directory_is_rejected() {
        let error = resolve_pi_agent_dir(
            None,
            Some("relative/pi-agent".into()),
            PathBuf::from("default"),
        )
        .expect_err("relative Pi directory must be rejected");
        assert!(error.to_string().contains("absolute directory"));
    }

    #[test]
    fn settings_directory_precedes_the_environment() {
        let temp = tempfile::tempdir().expect("tempdir");
        let settings_dir = temp.path().join("settings-agent");
        let env_dir = temp.path().join("env-agent");

        assert_eq!(
            resolve_pi_agent_dir(
                Some(settings_dir.clone()),
                Some(env_dir.into_os_string()),
                temp.path().join("default-agent"),
            )
            .expect("resolve Pi directory"),
            settings_dir
        );
    }

    #[test]
    #[serial]
    fn duplicate_provider_key_is_validation_not_a_write_conflict() {
        let _agent = test_support::TestAgentDir::new();
        insert_provider(&AppType::Pi, "duplicate", &provider()).expect("insert provider");
        let mut replacement = provider();
        replacement["name"] = json!("Other");

        let error = insert_provider(&AppType::Pi, "duplicate", &replacement)
            .expect_err("duplicate provider key must be rejected");
        assert!(matches!(error, AppError::InvalidInput(_)));
    }

    #[cfg(unix)]
    #[test]
    #[serial]
    fn newly_created_models_file_and_agent_directory_are_private() {
        use std::os::unix::fs::PermissionsExt;

        let _agent = test_support::TestAgentDir::new();
        insert_provider(&AppType::Pi, "cc-switch-private", &provider())
            .expect("write private models file");

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
        ensure_private_models_parent(&AppType::Pi, &path).expect("create agent directory");
        fs::write(&path, r#"{"providers":{"external":{"models":[]}}}"#)
            .expect("write initial models");
        let (_, stale_revision) =
            read_models_document_with_revision(&AppType::Pi, &path).expect("read models revision");

        let external = r#"{"providers":{"external":{"models":[]},"pi-added":{"models":[]}}}"#;
        fs::write(&path, external).expect("edit models externally");

        let replacement = json!({"providers": {"cc-switch": provider()}});
        let error = write_models_document(&AppType::Pi, &path, &replacement, &stale_revision)
            .expect_err("stale write must fail");
        assert!(matches!(error, AppError::Conflict(_)));
        assert_eq!(
            fs::read_to_string(path).expect("read external models"),
            external
        );
    }
}
