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

const MAX_FILE_BYTES: u64 = 1024 * 1024;
const MISSING_REVISION: &str = "missing";
static FILE_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));
#[cfg(test)]
static TEST_AGENT_DIR: LazyLock<Mutex<Option<PathBuf>>> = LazyLock::new(|| Mutex::new(None));

// ============================================================================
// Agent discovery providers (omp config.yml `disabledProviders`)
// ============================================================================

/// omp provider ids that auto-discover MCP servers and skills from *other*
/// agents. Disabling all of these in omp's `config.yml` `disabledProviders`
/// (array, exact-id match via `filterProviders`) stops omp from loading
/// MCP/skills inherited from those agents, aligning cc-switch's per-app
/// enable flags with omp's actual loaded set.
pub(crate) const AGENT_DISCOVERY_PROVIDER_IDS: [&str; 12] = [
    "claude",
    "claude-plugins",
    "agents",
    "codex",
    "gemini",
    "opencode",
    "cursor",
    "vscode",
    "cline",
    "windsurf",
    "github",
    "agent-plugins",
];

/// Display name for each discovery provider id, shown in the confirm dialog.
/// Order matches `AGENT_DISCOVERY_PROVIDER_IDS`.
pub(crate) fn agent_discovery_provider_display_name(id: &str) -> &'static str {
    match id {
        "claude" => "Claude Code",
        "claude-plugins" => "Claude Code 插件市场",
        "agents" => "Agent 目录 (.agent/.agents)",
        "codex" => "OpenAI Codex",
        "gemini" => "Gemini CLI",
        "opencode" => "OpenCode",
        "cursor" => "Cursor",
        "vscode" => "VS Code",
        "cline" => "Cline",
        "windsurf" => "Windsurf",
        "github" => "GitHub Copilot",
        "agent-plugins" => "Agent Plugins",
        _ => "",
    }
}

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

    // Named profile: OMP_PROFILE, then PI_PROFILE.
    for var in ["OMP_PROFILE", "PI_PROFILE"] {
        if let Some(name) = named_profile(std::env::var_os(var)) {
            return Ok(get_home_dir()
                .join(".omp")
                .join("profiles")
                .join(name)
                .join("agent"));
        }
    }

    // PI_CODING_AGENT_DIR is used directly as the agent dir (default profile).
    if let Some(dir) = env_path("PI_CODING_AGENT_DIR") {
        return Ok(dir);
    }

    // PI_CONFIG_DIR replaces the `.omp` base: ~/<PI_CONFIG_DIR>/agent.
    if let Some(dir) = env_path("PI_CONFIG_DIR") {
        return Ok(dir.join("agent"));
    }

    Ok(get_home_dir().join(".omp").join("agent"))
}

fn named_profile(raw: Option<std::ffi::OsString>) -> Option<String> {
    let value = raw?.to_string_lossy().trim().to_string();
    if value.is_empty() || value.eq_ignore_ascii_case("default") {
        None
    } else {
        Some(value)
    }
}

fn env_path(name: &str) -> Option<PathBuf> {
    let raw = std::env::var_os(name)?;
    let value = raw.to_string_lossy();
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(crate::settings::resolve_override_path(trimmed))
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
    if provider_key.trim().is_empty() {
        return Err(AppError::InvalidInput(
            "Oh My Pi provider key cannot be empty".to_string(),
        ));
    }
    config.as_object().ok_or_else(|| {
        AppError::InvalidInput("Oh My Pi provider configuration must be an object".to_string())
    })?;
    Ok(())
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

fn read_ohmypi_settings_with_revision() -> Result<(Value, String), AppError> {
    let _guard = lock_files()?;
    let path = get_ohmypi_settings_path()?;
    read_document_with_revision(&path, "Oh My Pi settings", true)
}

fn write_ohmypi_settings(document: &Value, expected_revision: &str) -> Result<(), AppError> {
    let _guard = lock_files()?;
    let path = get_ohmypi_settings_path()?;
    write_document(
        &path,
        document,
        expected_revision,
        "Oh My Pi settings",
        true,
    )
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

/// Write `modelRoles.default` to `<selector>`. No-op when `selector` is `None`.
pub(crate) fn write_ohmypi_default_model(selector: Option<&str>) -> Result<(), AppError> {
    let Some(selector) = selector else {
        return Ok(());
    };
    let (mut document, expected_revision) = read_ohmypi_settings_with_revision()?;
    set_nested(
        &mut document,
        &["modelRoles", "default"],
        Value::String(selector.to_string()),
    );
    write_ohmypi_settings(&document, &expected_revision)
}

/// Read omp `config.yml` `disabledProviders` as a list of provider id strings.
///
/// Missing key → empty vector. Non-array or non-string elements → `AppError::Config`.
/// The returned ids preserve file order and duplicates (callers that need a
/// set deduplicate as required).
pub(crate) fn read_ohmypi_disabled_providers() -> Result<Vec<String>, AppError> {
    let document = read_ohmypi_settings()?;
    read_disabled_providers_from(&document)
}

/// Write a union of `ids` into `disabledProviders`, preserving existing
/// entries (deduplicated, stable order: existing entries first in their
/// original order, then new ids in `AGENT_DISCOVERY_PROVIDER_IDS` order) and
/// leaving every other config key untouched. Reuses the file lock +
/// revision guard + atomic write used by the rest of config.yml I/O.
///
/// Idempotent: writing the same ids twice is a no-op on the file. Returns the
/// updated `disabledProviders` list.
pub(crate) fn set_ohmypi_disabled_providers_union(ids: &[&str]) -> Result<Vec<String>, AppError> {
    let (mut document, expected_revision) = read_ohmypi_settings_with_revision()?;
    let mut existing = read_disabled_providers_from(&document)?;
    let have: std::collections::HashSet<&str> = existing.iter().map(String::as_str).collect();
    let to_add: Vec<String> = ids
        .iter()
        .copied()
        .filter(|id| !have.contains(*id))
        .map(String::from)
        .collect();
    existing.extend(to_add);
    let new_value: Value = existing.iter().cloned().map(Value::String).collect();
    set_nested(&mut document, &["disabledProviders"], new_value);
    write_ohmypi_settings(&document, &expected_revision)?;
    Ok(existing)
}

fn read_disabled_providers_from(document: &Value) -> Result<Vec<String>, AppError> {
    let Some(array) = document.get("disabledProviders") else {
        return Ok(Vec::new());
    };
    let Some(items) = array.as_array() else {
        return Err(AppError::Config(
            "Oh My Pi settings 'disabledProviders' must be an array".to_string(),
        ));
    };
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        let Some(s) = item.as_str() else {
            return Err(AppError::Config(
                "Oh My Pi settings 'disabledProviders' entries must be strings".to_string(),
            ));
        };
        out.push(s.to_string());
    }
    Ok(out)
}

/// Split a `<provider>/<model>` selector on the first `/` and return the provider id.
pub(crate) fn provider_from_selector(selector: &str) -> &str {
    selector
        .split_once('/')
        .map(|(provider, _)| provider)
        .unwrap_or(selector)
}

fn set_nested(value: &mut Value, path: &[&str], new_value: Value) {
    match path.split_first() {
        None => *value = new_value,
        Some((segment, rest)) => {
            let object = value
                .as_object_mut()
                .expect("settings path must be an object");
            if rest.is_empty() {
                object.insert(segment.to_string(), new_value);
            } else {
                let child = object
                    .entry(segment.to_string())
                    .or_insert_with(|| Value::Object(Map::new()));
                set_nested(child, rest, new_value);
            }
        }
    }
}

// ============================================================================
// MCP server I/O (mcp.json)
// ============================================================================

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
    fn model_roles_default_write_and_read() {
        let _agent = TestAgentDir::new();
        write_agent_file("config.yml", "modelRoles:\n  smol: openai/gpt-4o-mini\n");

        write_ohmypi_default_model(Some("example/example-model")).expect("write default");
        let source = read_agent_file("config.yml");
        assert!(source.contains("default: example/example-model"));
        // unrelated role key preserved
        assert!(source.contains("smol: openai/gpt-4o-mini"));

        assert_eq!(
            read_ohmypi_default_model()
                .expect("read default")
                .as_deref(),
            Some("example/example-model")
        );
        assert_eq!(provider_from_selector("example/example-model"), "example");
    }

    #[test]
    #[serial]
    fn write_default_model_with_none_is_noop() {
        let _agent = TestAgentDir::new();
        write_ohmypi_default_model(None).expect("noop write with None");
        assert!(!get_ohmypi_settings_path().expect("settings path").exists());
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

    #[test]
    #[serial]
    fn read_disabled_providers_missing_key_is_empty() {
        let _agent = TestAgentDir::new();
        write_agent_file("config.yml", "modelRoles:\n  default: openai/gpt-4o\n");
        let ids = read_ohmypi_disabled_providers().expect("read disabled providers");
        assert!(ids.is_empty());
    }

    #[test]
    #[serial]
    fn read_disabled_providers_reads_array() {
        let _agent = TestAgentDir::new();
        write_agent_file(
            "config.yml",
            "modelRoles:\n  default: openai/gpt-4o\ndisabledProviders:\n  - claude\n  - codex\n",
        );
        let ids = read_ohmypi_disabled_providers().expect("read disabled providers");
        assert_eq!(ids, vec!["claude".to_string(), "codex".to_string()]);
    }

    #[test]
    #[serial]
    fn read_disabled_providers_rejects_non_array() {
        let _agent = TestAgentDir::new();
        write_agent_file("config.yml", "disabledProviders: not-an-array\n");
        let err = read_ohmypi_disabled_providers().expect_err("non-array should fail");
        assert!(matches!(err, AppError::Config(_)));
    }

    #[test]
    #[serial]
    fn set_disabled_providers_union_is_idempotent_and_preserves_existing() {
        let _agent = TestAgentDir::new();
        write_agent_file(
            "config.yml",
            "modelRoles:\n  default: openai/gpt-4o\ndisabledProviders:\n  - claude\n  - custom-src\n",
        );

        let result = set_ohmypi_disabled_providers_union(&AGENT_DISCOVERY_PROVIDER_IDS)
            .expect("union write");
        // existing entries preserved first, then missing ids in const order
        assert_eq!(result.len(), AGENT_DISCOVERY_PROVIDER_IDS.len() + 1);
        assert_eq!(result[0], "claude");
        assert_eq!(result[1], "custom-src");
        // remaining are the 11 missing discovery ids in const order
        assert_eq!(result[2], "claude-plugins");

        // idempotent: second write yields the same set, no duplication
        let result2 = set_ohmypi_disabled_providers_union(&AGENT_DISCOVERY_PROVIDER_IDS)
            .expect("idempotent union write");
        assert_eq!(result, result2);

        // other keys preserved
        let source = read_agent_file("config.yml");
        assert!(source.contains("default: openai/gpt-4o"));
    }

    #[test]
    #[serial]
    fn set_disabled_providers_union_preserves_other_keys() {
        let _agent = TestAgentDir::new();
        write_agent_file(
            "config.yml",
            "modelRoles:\n  default: openai/gpt-4o\nskills:\n  enableClaudeUser: true\n",
        );
        set_ohmypi_disabled_providers_union(&AGENT_DISCOVERY_PROVIDER_IDS).expect("union write");
        let source = read_agent_file("config.yml");
        assert!(source.contains("default: openai/gpt-4o"));
        assert!(source.contains("enableClaudeUser: true"));
        assert!(source.contains("disabledProviders"));
    }

    #[test]
    #[serial]
    fn set_disabled_providers_union_conflict_on_concurrent_write() {
        let _agent = TestAgentDir::new();
        write_agent_file("config.yml", "modelRoles:\n  default: openai/gpt-4o\n");
        // Read with a revision, then mutate the file on disk to change its
        // revision before attempting the union write through the public API
        // (which re-reads internally). Simulate by writing a stale revision
        // via the private writer path through the public function — instead,
        // verify the guard fires by racing: write, union-write (ok), then
        // tamper and union-write again reading a captured stale revision.
        set_ohmypi_disabled_providers_union(&["claude"]).expect("first write");
        // Tamper on disk so the next internal read sees a different revision.
        write_agent_file(
            "config.yml",
            "modelRoles:\n  default: openai/gpt-4o\ndisabledProviders:\n  - claude\n",
        );
        // Public API re-reads fresh revision; to force a conflict we must use
        // the internal writer with a deliberately stale revision.
        let stale = "0000000000000000000000000000000000000000000000000000000000000000";
        let err = write_ohmypi_settings(&json!({"disabledProviders": ["claude"]}), stale);
        assert!(matches!(err, Err(AppError::Conflict(_))));
    }

    #[test]
    fn agent_discovery_provider_ids_are_unique_and_complete() {
        let ids: std::collections::HashSet<&str> =
            AGENT_DISCOVERY_PROVIDER_IDS.iter().copied().collect();
        assert_eq!(
            ids.len(),
            AGENT_DISCOVERY_PROVIDER_IDS.len(),
            "ids must be unique"
        );
        assert_eq!(AGENT_DISCOVERY_PROVIDER_IDS.len(), 12);
        for id in AGENT_DISCOVERY_PROVIDER_IDS {
            assert!(!agent_discovery_provider_display_name(id).is_empty());
        }
    }
}
