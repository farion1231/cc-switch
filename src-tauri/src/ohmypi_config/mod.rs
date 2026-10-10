//! Thin adapter for Oh My Pi's native files.
//!
//! Oh My Pi (`omp`, can1357/oh-my-pi) keeps its configuration under
//! `~/.omp/agent` (or `~/.omp/profiles/<name>/agent` for a named profile):
//! - `models.yml` / `models.yaml` — provider entries (`providers` map), YAML.
//! - `config.yml` / `config.yaml` — settings (`modelRoles.default`), YAML.
//! - `mcp.json` — MCP servers (`mcpServers` map), JSON.
//!

use crate::config::{atomic_write_private, get_home_dir};
use crate::error::AppError;
use indexmap::IndexMap;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex, MutexGuard};

mod validation;

const MAX_FILE_BYTES: u64 = 1024 * 1024;
const MISSING_REVISION: &str = "missing";
static FILE_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));
#[cfg(test)]
static TEST_AGENT_DIR: LazyLock<Mutex<Option<PathBuf>>> = LazyLock::new(|| Mutex::new(None));

// ============================================================================
// Path resolution
// ============================================================================

pub(crate) fn get_ohmypi_agent_dir() -> Result<PathBuf, AppError> {
    #[cfg(test)]
    if let Some(path) = TEST_AGENT_DIR
        .lock()
        .expect("lock Oh My Pi test directory")
        .clone()
    {
        return Ok(path);
    }

    if let Some(dir) = crate::settings::get_ohmypi_override_dir() {
        return Ok(dir);
    }

    get_ohmypi_native_agent_dir()
}

pub(crate) fn get_ohmypi_active_profile() -> Result<Option<String>, AppError> {
    let selected = std::env::var_os("OMP_PROFILE").or_else(|| std::env::var_os("PI_PROFILE"));
    normalize_profile(
        selected
            .as_deref()
            .map(|name| name.to_string_lossy())
            .as_deref(),
    )
}

fn normalize_profile(raw: Option<&str>) -> Result<Option<String>, AppError> {
    let name = raw.unwrap_or_default().trim();
    if name.is_empty() || name == "default" {
        return Ok(None);
    }
    let reserved = name
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    let reserved_device = matches!(reserved.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (reserved.len() == 4
            && (reserved.starts_with("COM") || reserved.starts_with("LPT"))
            && reserved.as_bytes()[3].is_ascii_digit());
    if name.len() > 64
        || !name.as_bytes()[0].is_ascii_lowercase() && !name.as_bytes()[0].is_ascii_digit()
        || !name
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || b"._-".contains(&c))
        || name.ends_with('.')
        || reserved_device
    {
        return Err(AppError::Config(format!(
            "Invalid Oh My Pi profile: {name}"
        )));
    }
    Ok(Some(name.to_string()))
}

fn native_agent_dir(
    home: &Path,
    config_dir: Option<&str>,
    profile: Option<&str>,
    inherited_profile: Option<&str>,
    agent_dir: Option<&Path>,
) -> Result<PathBuf, AppError> {
    // PI_CONFIG_DIR is a directory name relative to home, as in omp's path.join(home, name).
    let config_dir = config_dir.filter(|name| !name.is_empty()).unwrap_or(".omp");
    let root = home.join(config_dir.trim_start_matches(std::path::MAIN_SEPARATOR));
    if let Some(profile) = profile {
        return Ok(root.join("profiles").join(profile).join("agent"));
    }
    // A parent omp process may have exported its named profile directory.
    // Explicit default mode must ignore that derived override.
    let agent_dir = agent_dir.filter(|agent| {
        inherited_profile
            .is_none_or(|profile| *agent != root.join("profiles").join(profile).join("agent"))
    });
    if let Some(agent) = agent_dir.filter(|path| !path.as_os_str().is_empty()) {
        return if agent.is_absolute() {
            Ok(agent.to_path_buf())
        } else {
            std::env::current_dir()
                .map(|cwd| cwd.join(agent))
                .map_err(|error| {
                    AppError::Config(format!("Cannot resolve Oh My Pi agent directory: {error}"))
                })
        };
    }
    Ok(root.join("agent"))
}

pub(crate) fn get_ohmypi_native_agent_dir() -> Result<PathBuf, AppError> {
    let profile = get_ohmypi_active_profile()?;
    let config_dir = std::env::var("PI_CONFIG_DIR").ok();
    let agent_dir = std::env::var_os("PI_CODING_AGENT_DIR").map(PathBuf::from);
    let inherited_profile = normalize_profile(std::env::var("PI_PROFILE").ok().as_deref())
        .ok()
        .flatten();
    native_agent_dir(
        &get_home_dir(),
        config_dir.as_deref(),
        profile.as_deref(),
        inherited_profile.as_deref(),
        agent_dir.as_deref(),
    )
}

pub(crate) fn get_ohmypi_data_dir() -> Result<PathBuf, AppError> {
    let agent = get_ohmypi_agent_dir()?;
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        let profile = get_ohmypi_active_profile()?;
        let config_dir = std::env::var("PI_CONFIG_DIR").ok();
        let default_agent = native_agent_dir(
            &get_home_dir(),
            config_dir.as_deref(),
            profile.as_deref(),
            None,
            None,
        )?;
        let xdg = std::env::var_os("XDG_DATA_HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from);
        Ok(data_dir(
            &agent,
            &default_agent,
            profile.as_deref(),
            xdg.as_deref(),
        ))
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    Ok(agent)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn data_dir(
    agent: &Path,
    default_agent: &Path,
    profile: Option<&str>,
    xdg: Option<&Path>,
) -> PathBuf {
    if agent == default_agent {
        if let Some(xdg) = xdg {
            let mut root = xdg.join("omp");
            if let Some(profile) = profile {
                root = root.join("profiles").join(profile);
            }
            if root.exists() {
                return root;
            }
        }
    }
    agent.to_path_buf()
}

/// Existing `models.yml` → `models.yaml` wins; greenfield defaults to `models.yml`.
pub(crate) fn get_ohmypi_models_path() -> Result<PathBuf, AppError> {
    let agent = get_ohmypi_agent_dir()?;
    for name in ["models.yml", "models.yaml"] {
        let path = agent.join(name);
        if path.exists() {
            return Ok(path);
        }
    }
    Ok(agent.join("models.yml"))
}

/// Existing `config.yml` → `config.yaml` wins; greenfield defaults to `config.yml`.
pub(crate) fn get_ohmypi_settings_path() -> Result<PathBuf, AppError> {
    let agent = get_ohmypi_agent_dir()?;
    for name in ["config.yml", "config.yaml"] {
        let path = agent.join(name);
        if path.exists() {
            return Ok(path);
        }
    }
    Ok(agent.join("config.yml"))
}

/// User-level MCP config path (`~/.omp/agent/mcp.json`).
pub(crate) fn get_ohmypi_mcp_path() -> Result<PathBuf, AppError> {
    Ok(get_ohmypi_agent_dir()?.join("mcp.json"))
}

// ============================================================================
// Read/write primitives
// ============================================================================

fn lock_files() -> Result<MutexGuard<'static, ()>, AppError> {
    FILE_LOCK
        .lock()
        .map_err(|error| AppError::Config(format!("Oh My Pi file lock is poisoned: {error}")))
}

fn read_file_limited(path: &Path, label: &str) -> Result<Vec<u8>, AppError> {
    let file = fs::File::open(path).map_err(|error| AppError::io(path, error))?;
    let metadata = file.metadata().map_err(|error| AppError::io(path, error))?;
    if metadata.len() > MAX_FILE_BYTES {
        return Err(AppError::InvalidInput(format!(
            "{label} file exceeds the 1 MiB limit: {}",
            path.display()
        )));
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(MAX_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| AppError::io(path, error))?;
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return Err(AppError::InvalidInput(format!(
            "{label} file exceeds the 1 MiB limit: {}",
            path.display()
        )));
    }
    Ok(bytes)
}

fn revision(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn read_yaml_value(path: &Path, label: &str) -> Result<Value, AppError> {
    let bytes = read_file_limited(path, label)?;
    let source = String::from_utf8(bytes).map_err(|error| {
        AppError::Config(format!(
            "{label} file must be UTF-8 ({}): {error}",
            path.display()
        ))
    })?;
    if source.trim().is_empty() {
        return Ok(Value::Object(Map::new()));
    }
    let yaml: serde_yaml::Value = serde_yaml::from_str(&source).map_err(|error| {
        AppError::Config(format!(
            "{label} file is not valid YAML ({}): {error}",
            path.display()
        ))
    })?;
    serde_json::to_value(yaml).map_err(|error| {
        AppError::Config(format!(
            "{label} file could not be converted from YAML ({}): {error}",
            path.display()
        ))
    })
}

fn read_json_value(path: &Path, label: &str) -> Result<Value, AppError> {
    let bytes = read_file_limited(path, label)?;
    let source = String::from_utf8(bytes).map_err(|error| {
        AppError::Config(format!(
            "{label} file must be UTF-8 ({}): {error}",
            path.display()
        ))
    })?;
    if source.trim().is_empty() {
        return Ok(Value::Object(Map::new()));
    }
    serde_json::from_str(&source).map_err(|error| {
        AppError::Config(format!(
            "{label} file is not valid JSON ({}): {error}",
            path.display()
        ))
    })
}

fn read_document_with_revision(
    path: &Path,
    label: &str,
    yaml: bool,
) -> Result<(Value, String), AppError> {
    if !path.exists() {
        return Ok((Value::Object(Map::new()), MISSING_REVISION.to_string()));
    }
    let bytes = read_file_limited(path, label)?;
    let revision = revision(&bytes);
    let document = if yaml {
        read_yaml_value(path, label)?
    } else {
        read_json_value(path, label)?
    };
    Ok((document, revision))
}

fn write_document(
    path: &Path,
    document: &Value,
    expected_revision: &str,
    label: &str,
    yaml: bool,
) -> Result<(), AppError> {
    let bytes = if yaml {
        let yaml: serde_yaml::Value =
            serde_json::from_value(document.clone()).map_err(|error| {
                AppError::Config(format!(
                    "{label} config could not be converted to YAML: {error}"
                ))
            })?;
        serde_yaml::to_string(&yaml)
            .map_err(|error| {
                AppError::Config(format!("{label} YAML serialization failed: {error}"))
            })?
            .into_bytes()
    } else {
        let mut bytes = serde_json::to_vec_pretty(document)
            .map_err(|source| AppError::JsonSerialize { source })?;
        bytes.push(b'\n');
        bytes
    };
    ensure_parent(path)?;
    ensure_revision(path, expected_revision, label)?;
    atomic_write_private(path, &bytes)
}

fn ensure_revision(path: &Path, expected_revision: &str, label: &str) -> Result<(), AppError> {
    let actual_revision = match fs::File::open(path) {
        Ok(_) => revision(&read_file_limited(path, label)?),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => MISSING_REVISION.to_string(),
        Err(error) => return Err(AppError::io(path, error)),
    };
    if actual_revision == expected_revision {
        Ok(())
    } else {
        Err(AppError::Conflict(format!(
            "{label} changed outside CC Switch: {}",
            path.display()
        )))
    }
}

fn ensure_parent(path: &Path) -> Result<(), AppError> {
    let parent = path.parent().ok_or_else(|| {
        AppError::Config(format!(
            "Oh My Pi config path has no parent directory: {}",
            path.display()
        ))
    })?;
    if !parent.exists() {
        fs::create_dir_all(parent).map_err(|source| AppError::io(parent, source))?;
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
            "Oh My Pi settings '{key}' must be a string: {}",
            path.display()
        ))),
    }
}

fn nonempty_string(value: Option<&Value>) -> Option<&str> {
    value
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
}

// ============================================================================
// Provider CRUD (models.yml)
// ============================================================================

pub(crate) fn read_ohmypi_native_providers() -> Result<IndexMap<String, Value>, AppError> {
    let _guard = lock_files()?;
    let path = get_ohmypi_models_path()?;
    let document = read_document_with_revision(&path, "Oh My Pi models", true)?.0;
    Ok(providers(&document, &path)?
        .iter()
        .map(|(key, config)| (key.clone(), config.clone()))
        .collect())
}

pub(crate) fn read_ohmypi_native_provider(provider_key: &str) -> Result<Option<Value>, AppError> {
    let _guard = lock_files()?;
    let path = get_ohmypi_models_path()?;
    let document = read_document_with_revision(&path, "Oh My Pi models", true)?.0;
    Ok(providers(&document, &path)?.get(provider_key).cloned())
}

pub(crate) fn ohmypi_provider_exists(provider_key: &str) -> Result<bool, AppError> {
    let _guard = lock_files()?;
    let path = get_ohmypi_models_path()?;
    let document = read_document_with_revision(&path, "Oh My Pi models", true)?.0;
    Ok(providers(&document, &path)?.contains_key(provider_key))
}

pub(crate) fn insert_ohmypi_provider(provider_key: &str, config: &Value) -> Result<bool, AppError> {
    validate_provider_node(provider_key, config)?;
    let _guard = lock_files()?;
    let path = get_ohmypi_models_path()?;
    let (mut document, expected_revision) =
        read_document_with_revision(&path, "Oh My Pi models", true)?;
    let providers = providers_mut(&mut document, &path)?;

    match providers.get(provider_key) {
        Some(current) if current == config => return Ok(false),
        Some(_) => {
            return Err(AppError::InvalidInput(format!(
                "Oh My Pi provider key '{provider_key}' already exists in models.yml"
            )))
        }
        None => {}
    }

    providers.insert(provider_key.to_string(), config.clone());
    write_document(
        &path,
        &document,
        &expected_revision,
        "Oh My Pi models",
        true,
    )?;
    Ok(true)
}

pub(crate) fn replace_ohmypi_provider(
    provider_key: &str,
    expected: &Value,
    replacement: &Value,
) -> Result<(), AppError> {
    validate_provider_node(provider_key, replacement)?;
    let _guard = lock_files()?;
    let path = get_ohmypi_models_path()?;
    let (mut document, expected_revision) =
        read_document_with_revision(&path, "Oh My Pi models", true)?;
    let providers = providers_mut(&mut document, &path)?;
    let current = providers.get(provider_key).ok_or_else(|| {
        AppError::Conflict(format!(
            "Oh My Pi provider '{provider_key}' is no longer present in models.yml"
        ))
    })?;
    if current != expected {
        return Err(AppError::Conflict(format!(
            "Oh My Pi provider '{provider_key}' changed outside CC Switch"
        )));
    }
    if current == replacement {
        return Ok(());
    }
    providers.insert(provider_key.to_string(), replacement.clone());
    write_document(
        &path,
        &document,
        &expected_revision,
        "Oh My Pi models",
        true,
    )
}

pub(crate) fn replace_ohmypi_provider_if_present(
    provider_key: &str,
    replacement: &Value,
) -> Result<Option<Value>, AppError> {
    validate_provider_node(provider_key, replacement)?;
    let _guard = lock_files()?;
    let path = get_ohmypi_models_path()?;
    let (mut document, expected_revision) =
        read_document_with_revision(&path, "Oh My Pi models", true)?;
    let providers = providers_mut(&mut document, &path)?;
    let Some(current) = providers.get(provider_key).cloned() else {
        return Ok(None);
    };
    if current == *replacement {
        return Ok(Some(current));
    }
    providers.insert(provider_key.to_string(), replacement.clone());
    write_document(
        &path,
        &document,
        &expected_revision,
        "Oh My Pi models",
        true,
    )?;
    Ok(Some(current))
}

pub(crate) fn remove_ohmypi_provider(provider_key: &str) -> Result<Option<Value>, AppError> {
    remove_ohmypi_provider_inner(provider_key, None)
}

pub(crate) fn remove_ohmypi_provider_if_matches(
    provider_key: &str,
    expected: &Value,
) -> Result<bool, AppError> {
    remove_ohmypi_provider_inner(provider_key, Some(expected)).map(|removed| removed.is_some())
}

fn remove_ohmypi_provider_inner(
    provider_key: &str,
    expected: Option<&Value>,
) -> Result<Option<Value>, AppError> {
    let _guard = lock_files()?;
    let path = get_ohmypi_models_path()?;
    let (mut document, expected_revision) =
        read_document_with_revision(&path, "Oh My Pi models", true)?;
    let providers = providers_mut(&mut document, &path)?;
    let Some(current) = providers.get(provider_key).cloned() else {
        return Ok(None);
    };
    if expected.is_some_and(|expected| current != *expected) {
        return Err(AppError::Conflict(format!(
            "Oh My Pi provider '{provider_key}' changed outside CC Switch"
        )));
    }
    providers.remove(provider_key);
    write_document(
        &path,
        &document,
        &expected_revision,
        "Oh My Pi models",
        true,
    )?;
    Ok(Some(current))
}

pub(crate) fn restore_ohmypi_provider_if_missing(
    provider_key: &str,
    config: &Value,
) -> Result<(), AppError> {
    let _guard = lock_files()?;
    let path = get_ohmypi_models_path()?;
    let (mut document, expected_revision) =
        read_document_with_revision(&path, "Oh My Pi models", true)?;
    let providers = providers_mut(&mut document, &path)?;
    match providers.get(provider_key) {
        Some(current) if current == config => Ok(()),
        Some(_) => Err(AppError::Conflict(format!(
            "cannot restore Oh My Pi provider '{provider_key}' because another value now owns the key"
        ))),
        None => {
            providers.insert(provider_key.to_string(), config.clone());
            write_document(&path, &document, &expected_revision, "Oh My Pi models", true)
        }
    }
}

/// Validate the shape CC Switch can persist as one `models.yml.providers.<key>` node.
///
/// Both full providers (`models` non-empty) and override-only providers
/// (`models` absent/empty) are accepted; the node must simply be a non-empty
/// object keyed by a non-empty id.
pub(crate) fn validate_provider_node(provider_key: &str, config: &Value) -> Result<(), AppError> {
    validation::validate_provider_node(provider_key, config)
}

pub(crate) fn provider_base_url(config: &Value) -> Result<String, AppError> {
    let provider = config.as_object().ok_or_else(|| {
        AppError::InvalidInput("Oh My Pi provider configuration must be an object".to_string())
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
        .ok_or_else(|| AppError::InvalidInput("Oh My Pi provider has no request URL".to_string()))
}

fn providers<'a>(document: &'a Value, path: &Path) -> Result<&'a Map<String, Value>, AppError> {
    let root = document.as_object().ok_or_else(|| {
        AppError::Config(format!(
            "Oh My Pi models root must be an object: {}",
            path.display()
        ))
    })?;
    match root.get("providers") {
        None => Ok(empty_json_object()),
        Some(Value::Object(providers)) => Ok(providers),
        Some(_) => Err(AppError::Config(format!(
            "Oh My Pi models 'providers' must be an object: {}",
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
            "Oh My Pi models root must be an object: {}",
            path.display()
        ))
    })?;
    let value = root
        .entry("providers".to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    value.as_object_mut().ok_or_else(|| {
        AppError::Config(format!(
            "Oh My Pi models 'providers' must be an object: {}",
            path.display()
        ))
    })
}

fn empty_json_object() -> &'static Map<String, Value> {
    static EMPTY: LazyLock<Map<String, Value>> = LazyLock::new(Map::new);
    &EMPTY
}

// ============================================================================
// Settings (config.yml)
// ============================================================================

/// Read the full `config.yml` as a JSON value.
pub(crate) fn read_ohmypi_settings() -> Result<Value, AppError> {
    let _guard = lock_files()?;
    let path = get_ohmypi_settings_path()?;
    if !path.exists() {
        return Ok(Value::Object(Map::new()));
    }
    read_yaml_value(&path, "Oh My Pi settings")
}

/// Read `modelRoles.default` (the full `<provider>/<model>` selector, if set).
pub(crate) fn read_ohmypi_default_model() -> Result<Option<String>, AppError> {
    let document = read_ohmypi_settings()?;
    let path = get_ohmypi_settings_path()?;
    let Some(object) = document.as_object() else {
        return Ok(None);
    };
    let Some(model_roles) = object.get("modelRoles") else {
        return Ok(None);
    };
    let Some(roles) = model_roles.as_object() else {
        return Ok(None);
    };
    optional_string(roles, "default", &path)
}

/// Split a `<provider>/<model>` selector on the first `/` and return the provider id.
pub(crate) fn provider_from_selector(selector: &str) -> &str {
    selector
        .split_once('/')
        .map(|(provider, _)| provider)
        .unwrap_or(selector)
}

// ============================================================================
// MCP server I/O (mcp.json)
// ============================================================================

#[cfg(test)]
pub(crate) fn read_ohmypi_mcp_servers() -> Result<IndexMap<String, Value>, AppError> {
    let _guard = lock_files()?;
    let path = get_ohmypi_mcp_path()?;
    let document = read_document_with_revision(&path, "Oh My Pi MCP", false)?.0;
    let root = document.as_object().ok_or_else(|| {
        AppError::Config(format!(
            "Oh My Pi MCP root must be an object: {}",
            path.display()
        ))
    })?;
    match root.get("mcpServers") {
        None => Ok(IndexMap::new()),
        Some(Value::Object(servers)) => Ok(servers
            .iter()
            .map(|(key, config)| (key.clone(), config.clone()))
            .collect()),
        Some(_) => Err(AppError::Config(format!(
            "Oh My Pi MCP 'mcpServers' must be an object: {}",
            path.display()
        ))),
    }
}

/// Mutate the native MCP document while preserving other root keys and checking external writes.
pub(crate) fn update_ohmypi_mcp_document(
    update: impl FnOnce(&mut Value) -> Result<bool, AppError>,
) -> Result<(), AppError> {
    let _guard = lock_files()?;
    let path = get_ohmypi_mcp_path()?;
    let (mut document, expected_revision) =
        read_document_with_revision(&path, "Oh My Pi MCP", false)?;
    if !document.is_object() {
        return Err(AppError::Config(format!(
            "Oh My Pi MCP root must be an object: {}",
            path.display()
        )));
    }
    if update(&mut document)? {
        write_document(&path, &document, &expected_revision, "Oh My Pi MCP", false)?;
    }
    Ok(())
}

#[cfg(test)]
pub(crate) fn set_ohmypi_mcp_server(id: &str, config: &Value) -> Result<(), AppError> {
    let _guard = lock_files()?;
    let path = get_ohmypi_mcp_path()?;
    let (mut document, expected_revision) =
        read_document_with_revision(&path, "Oh My Pi MCP", false)?;
    let root = document.as_object_mut().ok_or_else(|| {
        AppError::Config(format!(
            "Oh My Pi MCP root must be an object: {}",
            path.display()
        ))
    })?;
    let servers = root
        .entry("mcpServers".to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    let servers = servers.as_object_mut().ok_or_else(|| {
        AppError::Config(format!(
            "Oh My Pi MCP 'mcpServers' must be an object: {}",
            path.display()
        ))
    })?;
    servers.insert(id.to_string(), config.clone());
    write_document(&path, &document, &expected_revision, "Oh My Pi MCP", false)
}

#[cfg(test)]
pub(crate) fn remove_ohmypi_mcp_server(id: &str) -> Result<(), AppError> {
    let _guard = lock_files()?;
    let path = get_ohmypi_mcp_path()?;
    let (mut document, expected_revision) =
        read_document_with_revision(&path, "Oh My Pi MCP", false)?;
    let root = document.as_object_mut().ok_or_else(|| {
        AppError::Config(format!(
            "Oh My Pi MCP root must be an object: {}",
            path.display()
        ))
    })?;
    let Some(servers) = root.get_mut("mcpServers").and_then(Value::as_object_mut) else {
        return Ok(());
    };
    servers.remove(id);
    write_document(&path, &document, &expected_revision, "Oh My Pi MCP", false)
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
            let dir = tempfile::tempdir().expect("create Oh My Pi test directory");
            let agent_dir = dir.path().join("agent");
            Self::set(agent_dir, Some(dir))
        }

        #[allow(dead_code)] // used only by tests; compiled-in outside cfg(test) via shared test_support
        pub(crate) fn at(agent_dir: &Path) -> Self {
            Self::set(agent_dir.to_path_buf(), None)
        }

        fn set(agent_dir: PathBuf, dir: Option<tempfile::TempDir>) -> Self {
            let previous = super::TEST_AGENT_DIR
                .lock()
                .expect("lock Oh My Pi test directory")
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
                .expect("lock Oh My Pi test directory") = self.previous.take();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::TestAgentDir;
    use super::*;
    use serde_json::json;
    use serial_test::serial;

    #[test]
    fn native_profile_and_config_root_resolution() {
        let home = tempfile::tempdir().expect("home");
        assert_eq!(normalize_profile(Some("").or(Some("work"))).unwrap(), None);
        assert_eq!(
            normalize_profile(Some("default").or(Some("work"))).unwrap(),
            None
        );
        assert_eq!(
            normalize_profile(None.or(Some("work"))).unwrap().as_deref(),
            Some("work")
        );
        assert_eq!(
            native_agent_dir(home.path(), Some(".custom"), None, None, None).unwrap(),
            home.path().join(".custom/agent")
        );
        assert_eq!(
            native_agent_dir(
                home.path(),
                Some(".custom"),
                Some("work"),
                None,
                Some(Path::new("ignored"))
            )
            .unwrap(),
            home.path().join(".custom/profiles/work/agent")
        );
        let inherited = home.path().join(".custom/profiles/work/agent");
        assert_eq!(
            native_agent_dir(
                home.path(),
                Some(".custom"),
                None,
                Some("work"),
                Some(&inherited)
            )
            .unwrap(),
            home.path().join(".custom/agent")
        );
        let custom = home.path().join("custom-agent");
        assert_eq!(
            native_agent_dir(
                home.path(),
                Some(".custom"),
                None,
                Some("work"),
                Some(&custom)
            )
            .unwrap(),
            custom
        );
        for invalid in ["../other", "UPPER", "con", "nul.txt", "trailing."] {
            assert!(normalize_profile(Some(invalid)).is_err(), "{invalid}");
        }
    }

    #[test]
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    fn xdg_data_root_follows_existing_profile_and_agent_override() {
        let home = tempfile::tempdir().expect("home");
        let xdg = tempfile::tempdir().expect("xdg");
        let default = home.path().join(".omp/agent");
        std::fs::create_dir_all(xdg.path().join("omp")).unwrap();
        assert_eq!(
            data_dir(&default, &default, None, Some(xdg.path())),
            xdg.path().join("omp")
        );
        let custom = home.path().join("custom-agent");
        assert_eq!(data_dir(&custom, &default, None, Some(xdg.path())), custom);
        let work = home.path().join(".omp/profiles/work/agent");
        assert_eq!(data_dir(&work, &work, Some("work"), Some(xdg.path())), work);
        std::fs::create_dir_all(xdg.path().join("omp/profiles/work")).unwrap();
        assert_eq!(
            data_dir(&work, &work, Some("work"), Some(xdg.path())),
            xdg.path().join("omp/profiles/work")
        );
    }

    fn provider() -> Value {
        json!({
            "baseUrl": "https://api.example.com/v1",
            "api": "openai-completions",
            "apiKey": "secret",
            "models": [{"id": "example-model", "name": "Example Model"}]
        })
    }

    fn override_only_provider() -> Value {
        json!({
            "baseUrl": "https://custom-proxy.example.com",
            "api": "anthropic-messages",
            "apiKey": "proxy-key"
        })
    }

    fn write_agent_file(name: &str, content: &str) {
        let agent = get_ohmypi_agent_dir().expect("agent dir");
        std::fs::create_dir_all(&agent).expect("create agent dir");
        std::fs::write(agent.join(name), content).expect("write file");
    }

    fn read_agent_file(name: &str) -> String {
        let agent = get_ohmypi_agent_dir().expect("agent dir");
        std::fs::read_to_string(agent.join(name)).expect("read file")
    }

    #[test]
    #[serial]
    fn provider_crud_round_trip() {
        let _agent = TestAgentDir::new();
        let config = provider();

        assert!(insert_ohmypi_provider("example", &config).expect("insert"));
        assert!(!insert_ohmypi_provider("example", &config).expect("identical insert no-op"));

        let providers = read_ohmypi_native_providers().expect("read providers");
        assert_eq!(providers.len(), 1);
        assert_eq!(providers.get("example"), Some(&config));

        let mut updated = config.clone();
        updated["apiKey"] = json!("new-secret");
        replace_ohmypi_provider("example", &config, &updated).expect("replace");
        assert_eq!(
            read_ohmypi_native_provider("example").expect("read provider"),
            Some(updated.clone())
        );

        let removed = remove_ohmypi_provider("example").expect("remove");
        assert_eq!(removed, Some(updated));
        assert!(!ohmypi_provider_exists("example").expect("exists"));
    }

    #[test]
    #[serial]
    fn override_only_provider_is_accepted() {
        let _agent = TestAgentDir::new();
        let config = override_only_provider();
        assert!(insert_ohmypi_provider("proxy", &config).expect("insert override-only"));
        let providers = read_ohmypi_native_providers().expect("read providers");
        assert_eq!(providers.get("proxy"), Some(&config));
    }

    #[test]
    #[serial]
    fn read_default_model_and_selector_split() {
        let _agent = TestAgentDir::new();
        write_agent_file("config.yml", "modelRoles:\n  smol: openai/gpt-4o-mini\n");

        assert_eq!(read_ohmypi_default_model().expect("read default"), None);
        assert_eq!(provider_from_selector("example/example-model"), "example");
    }

    #[test]
    #[serial]
    fn mcp_server_io() {
        let _agent = TestAgentDir::new();
        let spec = json!({
            "type": "stdio",
            "command": "npx",
            "args": ["-y", "@modelcontextprotocol/server"],
            "env": {"API_KEY": "secret"}
        });
        set_ohmypi_mcp_server("filesystem", &spec).expect("set server");
        let servers = read_ohmypi_mcp_servers().expect("read servers");
        assert_eq!(servers.get("filesystem"), Some(&spec));

        remove_ohmypi_mcp_server("filesystem").expect("remove server");
        assert!(read_ohmypi_mcp_servers().expect("read servers").is_empty());
    }

    #[test]
    #[serial]
    fn unmanaged_keys_preserved_after_provider_edit() {
        let _agent = TestAgentDir::new();
        write_agent_file(
            "models.yml",
            "providers:\n  existing:\n    baseUrl: https://a.example\n    compat:\n      supportsStore: true\n",
        );

        let config = provider();
        insert_ohmypi_provider("example", &config).expect("insert second provider");

        let source = read_agent_file("models.yml");
        assert!(source.contains("supportsStore: true"));
        assert!(source.contains("example"));
        assert!(source.contains("existing"));
    }

    #[test]
    #[serial]
    fn provider_base_url_falls_back_to_model_url() {
        let config = json!({
            "api": "openai-completions",
            "models": [{"id": "m", "baseUrl": "https://model.example"}]
        });
        assert_eq!(
            provider_base_url(&config).expect("base url"),
            "https://model.example"
        );
    }
}
