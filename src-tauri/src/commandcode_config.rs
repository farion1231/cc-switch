//! Command Code CLI (`cmd`) keeps every BYOK provider in `providers.json` and
//! stores provider keys in `auth.json`. Both files are shared with the CLI and
//! this module mirrors the CLI's own write rules:
//!
//! - `providers.json` only ever holds `$ENV_VAR` / `{env:VAR}` / `!command`
//!   references; raw secrets are refused by the CLI, so they go to `auth.json`
//!   under the provider id as `{ "type": "api", "key": "..." }` (0600).
//! - `auth.json` also stores the Command Code account fields
//!   (`apiKey`, `userId`, `userName`, `keyName`, `authenticatedAt`) at the top
//!   level; those are never touched here.
//! - The provider map lives under `provider` (legacy alias: `providers`) and
//!   entries keep the file's indentation.
use crate::config::{atomic_write, atomic_write_private, get_home_dir};
use crate::error::AppError;
use indexmap::IndexMap;
use serde::Serialize;
use serde_json::{json, Map, Value};
use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

/// Provider ids the CLI reserves for its built-in subscription lanes.
const RESERVED_PROVIDER_IDS: [&str; 3] = ["anthropic", "github-copilot", "codex"];
/// Top-level `auth.json` keys owned by the Command Code account login.
const RESERVED_AUTH_FIELDS: [&str; 5] =
    ["apiKey", "userId", "userName", "keyName", "authenticatedAt"];
const API_FORMATS: [&str; 3] = [
    "openai-completions",
    "openai-responses",
    "anthropic-messages",
];
const DEFAULT_API_FORMAT: &str = "openai-completions";

pub(crate) fn data_dir() -> PathBuf {
    get_home_dir().join(".commandcode")
}

pub(crate) fn providers_path() -> PathBuf {
    data_dir().join("providers.json")
}

/// The CLI keeps one auth file per environment, selected by `COMMANDCODE_API_ENV`.
fn auth_file_name(api_env: Option<&str>) -> &'static str {
    match api_env.map(str::trim) {
        Some("local") => "auth.local.json",
        Some("staging") => "auth.staging.json",
        _ => "auth.json",
    }
}

pub(crate) fn auth_path() -> PathBuf {
    let api_env = std::env::var("COMMANDCODE_API_ENV").ok();
    data_dir().join(auth_file_name(api_env.as_deref()))
}

/// The provider list file, used by the config-directory commands.
pub(crate) fn config_path() -> PathBuf {
    providers_path()
}

fn read_json_object(path: &Path, what: &str) -> Result<Map<String, Value>, AppError> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Map::new()),
        Err(error) => return Err(AppError::io(path, error)),
    };
    if text.trim().is_empty() {
        return Ok(Map::new());
    }
    let value: Value = serde_json::from_str(&text)
        .map_err(|_| AppError::Config(format!("Invalid {what} JSON: {}", path.display())))?;
    match value {
        Value::Object(map) => Ok(map),
        _ => Err(AppError::Config(format!(
            "{what} must be a JSON object: {}",
            path.display()
        ))),
    }
}

/// The CLI reads `provider` first and falls back to the legacy `providers` key.
fn providers_of(root: &Map<String, Value>) -> Option<&Map<String, Value>> {
    root.get("provider")
        .and_then(Value::as_object)
        .or_else(|| root.get("providers").and_then(Value::as_object))
}

fn providers_of_mut(root: &mut Map<String, Value>) -> Option<&mut Map<String, Value>> {
    if root.get("provider").is_some_and(Value::is_object) {
        return root.get_mut("provider").and_then(Value::as_object_mut);
    }
    if root.get("providers").is_some_and(Value::is_object) {
        return root.get_mut("providers").and_then(Value::as_object_mut);
    }
    None
}

/// Mirror of the CLI's `providerMapKey`: keep writing into the legacy key when
/// it is the one already in use.
fn write_map_key(root: &Map<String, Value>) -> &'static str {
    if root.get("provider").is_some_and(Value::is_object) {
        "provider"
    } else if root.get("providers").is_some_and(Value::is_object) {
        "providers"
    } else {
        "provider"
    }
}

fn is_valid_provider_id(id: &str) -> bool {
    let mut chars = id.chars();
    match chars.next() {
        Some(c) if c.is_ascii_lowercase() || c.is_ascii_digit() => {}
        _ => return false,
    }
    id.len() <= 64
        && chars
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '-' | '_'))
}

fn is_reserved_provider_id(id: &str) -> bool {
    RESERVED_PROVIDER_IDS.contains(&id) || RESERVED_AUTH_FIELDS.contains(&id)
}

fn is_disabled_entry(entry: &Map<String, Value>) -> bool {
    entry.get("disabled") == Some(&Value::Bool(true))
        || entry.get("enabled") == Some(&Value::Bool(false))
}

fn stored_key<'a>(auth: &'a Map<String, Value>, id: &str) -> Option<&'a str> {
    let entry = auth.get(id)?.as_object()?;
    if entry.get("type").and_then(Value::as_str) != Some("api") {
        return None;
    }
    entry.get("key").and_then(Value::as_str)
}

fn get_providers_from(
    providers_path: &Path,
    auth_path: &Path,
) -> Result<IndexMap<String, Value>, AppError> {
    let root = read_json_object(providers_path, "Command Code providers")?;
    let auth = read_json_object(auth_path, "Command Code auth")?;
    let mut providers = IndexMap::new();
    let Some(map) = providers_of(&root) else {
        return Ok(providers);
    };
    for (id, entry) in map {
        if !is_valid_provider_id(id) || is_reserved_provider_id(id) {
            continue;
        }
        let Some(entry) = entry.as_object() else {
            continue;
        };
        if is_disabled_entry(entry) {
            continue;
        }
        let mut config = entry.clone();
        // A key stored by the CLI wins over the reference in providers.json.
        if let Some(key) = stored_key(&auth, id) {
            config.insert("apiKey".into(), json!(key));
        }
        providers.insert(id.clone(), Value::Object(config));
    }
    Ok(providers)
}

pub(crate) fn get_providers() -> Result<IndexMap<String, Value>, AppError> {
    get_providers_from(&providers_path(), &auth_path())
}

/// Any provider map entry under this key, including tombstoned and reserved ones.
pub(crate) fn provider_key_exists(id: &str) -> Result<bool, AppError> {
    let root = read_json_object(&providers_path(), "Command Code providers")?;
    Ok(providers_of(&root).is_some_and(|map| map.keys().any(|key| key.eq_ignore_ascii_case(id))))
}

/// The CLI's own reference syntax: `$ENV_VAR`, `{env:VAR}` or `!command`.
fn is_env_var_name(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn is_api_key_reference(value: &str) -> bool {
    if value.starts_with('!') {
        return true;
    }
    if let Some(var) = value.strip_prefix('$') {
        return is_env_var_name(var);
    }
    value.match_indices("{env:").any(|(index, _)| {
        let rest = &value[index + "{env:".len()..];
        rest.find('}')
            .is_some_and(|end| is_env_var_name(&rest[..end]))
    })
}

pub(crate) fn validate_provider(id: &str, config: &Value) -> Result<(), AppError> {
    if !is_valid_provider_id(id) {
        return Err(AppError::InvalidInput(
            "Command Code provider ids use lowercase letters, digits, '.', '-' or '_'".into(),
        ));
    }
    if is_reserved_provider_id(id) {
        return Err(AppError::InvalidInput(format!(
            "'{id}' is reserved by Command Code; choose another provider id"
        )));
    }
    let Some(config) = config.as_object() else {
        return Err(AppError::InvalidInput(
            "Command Code configuration must be a JSON object".into(),
        ));
    };
    if let Some(api) = config.get("api") {
        if !api.as_str().is_some_and(|api| API_FORMATS.contains(&api)) {
            return Err(AppError::InvalidInput(
                "Select a supported Command Code API format".into(),
            ));
        }
    }
    let base_url = config
        .get("baseURL")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if !url::Url::parse(base_url)
        .is_ok_and(|url| matches!(url.scheme(), "http" | "https") && url.host_str().is_some())
    {
        return Err(AppError::InvalidInput(
            "Enter a valid Command Code endpoint URL".into(),
        ));
    }
    if config
        .get("models")
        .and_then(Value::as_object)
        .is_none_or(|models| models.is_empty() || models.keys().any(|id| id.trim().is_empty()))
    {
        return Err(AppError::InvalidInput(
            "Add at least one Command Code model".into(),
        ));
    }
    match config.get("apiKey") {
        None | Some(Value::Bool(false)) | Some(Value::String(_)) => {}
        Some(_) => {
            return Err(AppError::InvalidInput(
                "Command Code API key must be a string or false".into(),
            ))
        }
    }
    if config
        .get("headers")
        .and_then(Value::as_object)
        .is_some_and(|headers| headers.values().any(|value| !value.is_string()))
    {
        return Err(AppError::InvalidInput(
            "Command Code header values must be strings".into(),
        ));
    }
    Ok(())
}

enum KeyWrite {
    /// Raw secret: stored in auth.json under the provider id.
    Stored(String),
    /// `$ENV_VAR` / `{env:VAR}` / `!command`: stays in providers.json.
    Reference,
    /// `apiKey: false` — the endpoint needs no key.
    Keyless,
}

/// Split a CC Switch provider config into the CLI's live entry plus the key
/// action. `providers.json` never receives raw secrets.
fn live_entry_from_config(config: &Value) -> (Map<String, Value>, KeyWrite) {
    let mut entry = Map::new();
    if let Some(name) = config
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())
    {
        entry.insert("name".into(), json!(name));
    }
    if let Some(api) = config
        .get("api")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|api| !api.is_empty() && *api != DEFAULT_API_FORMAT)
    {
        entry.insert("api".into(), json!(api));
    }
    entry.insert(
        "baseURL".into(),
        config.get("baseURL").cloned().unwrap_or(Value::Null),
    );
    let key_write = match config.get("apiKey") {
        Some(Value::String(key)) if !key.trim().is_empty() => {
            if is_api_key_reference(key) {
                entry.insert("apiKey".into(), json!(key));
                KeyWrite::Reference
            } else {
                KeyWrite::Stored(key.clone())
            }
        }
        _ => {
            entry.insert("apiKey".into(), Value::Bool(false));
            KeyWrite::Keyless
        }
    };
    if let Some(headers) = config
        .get("headers")
        .and_then(Value::as_object)
        .filter(|headers| !headers.is_empty())
    {
        entry.insert("headers".into(), Value::Object(headers.clone()));
    }
    entry.insert(
        "models".into(),
        config.get("models").cloned().unwrap_or_else(|| json!({})),
    );
    (entry, key_write)
}

// The CLI serializes providers.json with the indentation it finds in the file
// (`sniffIndent`), defaulting to a tab for new files.
fn sniff_indent(text: &str) -> Option<String> {
    for line in text.lines() {
        let trimmed = line.trim_start_matches([' ', '\t']);
        if trimmed.len() < line.len() && trimmed.starts_with('"') {
            return Some(line[..line.len() - trimmed.len()].to_string());
        }
    }
    None
}

fn serialize_with_indent<T: Serialize>(value: &T, indent: &str) -> Result<Vec<u8>, AppError> {
    use serde_json::ser::{PrettyFormatter, Serializer};
    let mut buffer = Vec::new();
    let mut serializer =
        Serializer::with_formatter(&mut buffer, PrettyFormatter::with_indent(indent.as_bytes()));
    value.serialize(&mut serializer).map_err(|error| {
        AppError::Message(format!(
            "Cannot serialize Command Code configuration: {error}"
        ))
    })?;
    Ok(buffer)
}

fn serialize_providers(
    root: &Map<String, Value>,
    previous: Option<&str>,
) -> Result<Vec<u8>, AppError> {
    let indent = previous
        .and_then(sniff_indent)
        .unwrap_or_else(|| "\t".into());
    let mut bytes = serialize_with_indent(root, &indent)?;
    bytes.push(b'\n');
    Ok(bytes)
}

// The CLI's proper-lockfile uses the same atomic directory lock.
struct ConfigLock(PathBuf);
impl Drop for ConfigLock {
    fn drop(&mut self) {
        let _ = fs::remove_dir(&self.0);
    }
}

fn acquire_lock(path: &Path) -> Result<ConfigLock, AppError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| AppError::io(parent, error))?;
    }
    let path = if path.exists() {
        fs::canonicalize(path).map_err(|error| AppError::Message(error.to_string()))?
    } else {
        path.to_path_buf()
    };
    let lock_path = PathBuf::from(format!("{}.lock", path.display()));
    if let Err(error) = fs::create_dir(&lock_path) {
        if error.kind() != std::io::ErrorKind::AlreadyExists {
            return Err(AppError::io(&lock_path, error));
        }
        let stale = fs::metadata(&lock_path)
            .and_then(|metadata| metadata.modified())
            .ok()
            .and_then(|mtime| mtime.elapsed().ok())
            .is_some_and(|age| age > Duration::from_secs(10));
        if !stale {
            return Err(AppError::Conflict(
                "Command Code configuration is busy; retry after the CLI finishes saving".into(),
            ));
        }
        fs::remove_dir(&lock_path).map_err(|error| AppError::io(&lock_path, error))?;
        fs::create_dir(&lock_path).map_err(|error| AppError::io(&lock_path, error))?;
    }
    Ok(ConfigLock(lock_path))
}

fn restore(path: &Path, previous: Option<&[u8]>) -> Result<(), AppError> {
    match previous {
        Some(bytes) => atomic_write(path, bytes),
        None => match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(AppError::io(path, error)),
        },
    }
}

/// `create_only` rejects a key that already exists, checked under the file lock.
fn write_provider_entry(
    providers_path: &Path,
    auth_path: &Path,
    id: &str,
    config: &Value,
    create_only: bool,
) -> Result<(), AppError> {
    let _lock = acquire_lock(providers_path)?;
    let previous_providers = fs::read(providers_path).ok();
    let previous_auth = fs::read(auth_path).ok();
    let mut root = read_json_object(providers_path, "Command Code providers")?;
    let mut auth = read_json_object(auth_path, "Command Code auth")?;
    if create_only
        && providers_of(&root).is_some_and(|map| map.keys().any(|key| key.eq_ignore_ascii_case(id)))
    {
        return Err(AppError::InvalidInput(format!(
            "Command Code provider key '{id}' already exists"
        )));
    }
    let map_key = write_map_key(&root);
    let map_value = root.entry(map_key).or_insert_with(|| json!({}));
    if !map_value.is_object() {
        *map_value = json!({});
    }
    let map = map_value.as_object_mut().expect("Validated provider map");
    let (entry, key_write) = live_entry_from_config(config);
    map.insert(id.to_string(), Value::Object(entry));
    match &key_write {
        KeyWrite::Stored(key) => {
            auth.insert(id.to_string(), json!({ "type": "api", "key": key }));
        }
        // A reference or keyless entry must not be shadowed by an older stored key.
        KeyWrite::Reference | KeyWrite::Keyless => {
            if let Some(actual) = auth
                .keys()
                .find(|key| key.eq_ignore_ascii_case(id))
                .cloned()
            {
                auth.remove(&actual);
            }
        }
    }

    let previous_text = previous_providers
        .as_deref()
        .and_then(|bytes| std::str::from_utf8(bytes).ok());
    let providers_bytes = serialize_providers(&root, previous_text)?;
    atomic_write(providers_path, &providers_bytes)?;

    let auth_result = if auth.is_empty() {
        match fs::remove_file(auth_path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(AppError::io(auth_path, error)),
        }
    } else {
        let auth_bytes = serialize_with_indent(&Value::Object(auth), "  ")?;
        if previous_auth.as_deref() == Some(auth_bytes.as_slice()) {
            Ok(())
        } else {
            atomic_write_private(auth_path, &auth_bytes)
        }
    };
    if let Err(error) = auth_result {
        if let Err(rollback_error) = restore(providers_path, previous_providers.as_deref()) {
            return Err(AppError::Message(format!(
                "Command Code key update failed ({error}); restoring {} also failed: {rollback_error}",
                providers_path.display()
            )));
        }
        return Err(error);
    }
    Ok(())
}

pub(crate) fn set_provider(id: &str, config: Value) -> Result<(), AppError> {
    validate_provider(id, &config)?;
    write_provider_entry(&providers_path(), &auth_path(), id, &config, false)
}

/// Like `set_provider`, but never replaces an existing entry.
pub(crate) fn add_provider(id: &str, config: Value) -> Result<(), AppError> {
    validate_provider(id, &config)?;
    write_provider_entry(&providers_path(), &auth_path(), id, &config, true)
}

pub(crate) fn remove_provider(id: &str) -> Result<(), AppError> {
    remove_provider_at(&providers_path(), &auth_path(), id)
}

fn remove_provider_at(providers_path: &Path, auth_path: &Path, id: &str) -> Result<(), AppError> {
    if !providers_path.exists() && !auth_path.exists() {
        return Ok(());
    }
    let _lock = acquire_lock(providers_path)?;
    let previous_providers = fs::read(providers_path).ok();
    let previous_auth = fs::read(auth_path).ok();
    let mut root = read_json_object(providers_path, "Command Code providers")?;
    let mut changed_providers = false;
    if let Some(map) = providers_of_mut(&mut root) {
        if let Some(actual) = map.keys().find(|key| key.eq_ignore_ascii_case(id)).cloned() {
            map.remove(&actual);
            changed_providers = true;
        }
    }
    let mut auth = read_json_object(auth_path, "Command Code auth")?;
    let mut changed_auth = false;
    if let Some(actual) = auth
        .keys()
        .find(|key| key.eq_ignore_ascii_case(id))
        .cloned()
    {
        auth.remove(&actual);
        changed_auth = true;
    }
    if !changed_providers && !changed_auth {
        return Ok(());
    }
    let previous_text = previous_providers
        .as_deref()
        .and_then(|bytes| std::str::from_utf8(bytes).ok());
    let providers_bytes = serialize_providers(&root, previous_text)?;
    atomic_write(providers_path, &providers_bytes)?;
    let auth_result = if !changed_auth {
        Ok(())
    } else if auth.is_empty() {
        match fs::remove_file(auth_path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(AppError::io(auth_path, error)),
        }
    } else {
        let auth_bytes = serialize_with_indent(&Value::Object(auth), "  ")?;
        if previous_auth.as_deref() == Some(auth_bytes.as_slice()) {
            Ok(())
        } else {
            atomic_write_private(auth_path, &auth_bytes)
        }
    };
    if let Err(error) = auth_result {
        if let Err(rollback_error) = restore(providers_path, previous_providers.as_deref()) {
            return Err(AppError::Message(format!(
                "Command Code provider removal failed ({error}); restoring {} also failed: {rollback_error}",
                providers_path.display()
            )));
        }
        return Err(error);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(dir: &Path) -> (PathBuf, PathBuf) {
        (dir.join("providers.json"), dir.join("auth.json"))
    }

    fn valid_config(key: Value) -> Value {
        json!({
            "name": "Example",
            "baseURL": "https://api.example.com/v1",
            "apiKey": key,
            "models": {"example-model": {"name": "Example"}}
        })
    }

    #[test]
    fn raw_keys_go_to_auth_json_and_references_stay_in_providers_json() {
        let dir = tempfile::tempdir().unwrap();
        let (providers, auth) = paths(dir.path());
        fs::write(
            &auth,
            json!({"apiKey":"account-key","userId":"u1","other":{"type":"api","key":"keep"}})
                .to_string(),
        )
        .unwrap();
        write_provider_entry(
            &providers,
            &auth,
            "raw",
            &valid_config(json!("sk-live")),
            true,
        )
        .unwrap();
        let root: Value = serde_json::from_str(&fs::read_to_string(&providers).unwrap()).unwrap();
        assert!(root["provider"]["raw"].get("apiKey").is_none());
        assert_eq!(
            root["provider"]["raw"]["baseURL"],
            "https://api.example.com/v1"
        );
        let stored: Value = serde_json::from_str(&fs::read_to_string(&auth).unwrap()).unwrap();
        assert_eq!(stored["raw"], json!({"type":"api","key":"sk-live"}));
        // Account fields and unrelated entries survive.
        assert_eq!(stored["apiKey"], "account-key");
        assert_eq!(stored["userId"], "u1");
        assert_eq!(stored["other"]["key"], "keep");

        write_provider_entry(
            &providers,
            &auth,
            "raw",
            &valid_config(json!("$EXAMPLE_KEY")),
            false,
        )
        .unwrap();
        let root: Value = serde_json::from_str(&fs::read_to_string(&providers).unwrap()).unwrap();
        assert_eq!(root["provider"]["raw"]["apiKey"], "$EXAMPLE_KEY");
        let stored: Value = serde_json::from_str(&fs::read_to_string(&auth).unwrap()).unwrap();
        assert!(stored.get("raw").is_none());
        assert_eq!(stored["apiKey"], "account-key");

        write_provider_entry(&providers, &auth, "raw", &valid_config(json!(false)), false).unwrap();
        let root: Value = serde_json::from_str(&fs::read_to_string(&providers).unwrap()).unwrap();
        assert_eq!(root["provider"]["raw"]["apiKey"], false);
    }

    #[test]
    fn create_only_rejects_existing_keys_case_insensitively() {
        let dir = tempfile::tempdir().unwrap();
        let (providers, auth) = paths(dir.path());
        write_provider_entry(&providers, &auth, "taken", &valid_config(json!("k")), true).unwrap();
        let before = fs::read_to_string(&providers).unwrap();
        assert!(
            write_provider_entry(&providers, &auth, "TAKEN", &valid_config(json!("k")), true)
                .is_err()
        );
        assert_eq!(fs::read_to_string(&providers).unwrap(), before);
    }

    #[test]
    fn default_api_format_is_omitted_and_custom_formats_are_kept() {
        let dir = tempfile::tempdir().unwrap();
        let (providers, auth) = paths(dir.path());
        let mut config = valid_config(json!("k"));
        config["api"] = json!("openai-completions");
        write_provider_entry(&providers, &auth, "default", &config, true).unwrap();
        let root: Value = serde_json::from_str(&fs::read_to_string(&providers).unwrap()).unwrap();
        assert!(root["provider"]["default"].get("api").is_none());
        config["api"] = json!("anthropic-messages");
        write_provider_entry(&providers, &auth, "anthropic", &config, true).unwrap();
        let root: Value = serde_json::from_str(&fs::read_to_string(&providers).unwrap()).unwrap();
        assert_eq!(root["provider"]["anthropic"]["api"], "anthropic-messages");
    }

    #[test]
    fn writes_preserve_unknown_fields_indentation_and_siblings() {
        let dir = tempfile::tempdir().unwrap();
        let (providers, auth) = paths(dir.path());
        let text = "{\n  \"provider\": {\n    \"existing\": {\n      \"baseURL\": \"https://keep.example.com\",\n      \"models\": { \"m\": {} }\n    }\n  },\n  \"unknown\": [1, 2]\n}\n";
        fs::write(&providers, text).unwrap();
        write_provider_entry(&providers, &auth, "added", &valid_config(json!("k")), true).unwrap();
        let after = fs::read_to_string(&providers).unwrap();
        assert!(after.contains("\n  \"provider\": {"));
        let root: Value = serde_json::from_str(&after).unwrap();
        assert_eq!(root["unknown"], json!([1, 2]));
        assert_eq!(
            root["provider"]["existing"]["baseURL"],
            "https://keep.example.com"
        );
        assert_eq!(root["provider"]["added"]["name"], "Example");
    }

    #[test]
    fn get_providers_prefers_stored_keys_and_skips_tombstones_and_reserved_ids() {
        let dir = tempfile::tempdir().unwrap();
        let (providers, auth) = paths(dir.path());
        fs::write(
            &providers,
            json!({
                "provider": {
                    "openrouter": {"baseURL":"https://openrouter.ai/api/v1","apiKey":"$OR_KEY","models":{"m":{}}},
                    "local": {"baseURL":"http://localhost:11434/v1","apiKey":false,"models":{"m":{}}},
                    "gone": {"baseURL":"https://gone.example.com","disabled":true,"models":{"m":{}}},
                    "codex": {"baseURL":"https://reserved.example.com","models":{"m":{}}},
                    "Bad": {"baseURL":"https://bad.example.com","models":{"m":{}}}
                }
            })
            .to_string(),
        )
        .unwrap();
        fs::write(
            &auth,
            json!({"openrouter":{"type":"api","key":"sk-stored"},"apiKey":"account"}).to_string(),
        )
        .unwrap();
        let providers = get_providers_from(&providers, &auth).unwrap();
        assert_eq!(providers.len(), 2);
        assert_eq!(providers["openrouter"]["apiKey"], "sk-stored");
        assert_eq!(providers["local"]["apiKey"], false);
        assert!(!providers.contains_key("gone"));
        assert!(!providers.contains_key("codex"));
        assert!(!providers.contains_key("Bad"));
    }

    #[test]
    fn remove_provider_clears_both_files_and_keeps_account_fields() {
        let dir = tempfile::tempdir().unwrap();
        let (providers, auth) = paths(dir.path());
        fs::write(
            &auth,
            json!({"apiKey":"account","gone":{"type":"api","key":"sk"},"keep":{"type":"api","key":"sk2"}})
                .to_string(),
        )
        .unwrap();
        write_provider_entry(&providers, &auth, "gone", &valid_config(json!("sk")), true).unwrap();
        remove_provider_at(&providers, &auth, "GONE").unwrap();
        let root: Value = serde_json::from_str(&fs::read_to_string(&providers).unwrap()).unwrap();
        assert!(root["provider"].get("gone").is_none());
        let stored: Value = serde_json::from_str(&fs::read_to_string(&auth).unwrap()).unwrap();
        assert!(stored.get("gone").is_none());
        assert_eq!(stored["keep"]["key"], "sk2");
        assert_eq!(stored["apiKey"], "account");
    }

    #[test]
    fn validation_rejects_invalid_ids_urls_models_and_key_types() {
        let config = valid_config(json!("k"));
        for id in [
            "",
            "Upper",
            "white space",
            "codex",
            "apiKey",
            &"a".repeat(65),
        ] {
            assert!(validate_provider(id, &config).is_err(), "{id}");
        }
        validate_provider("valid.id-1_x", &config).unwrap();
        let mut broken = valid_config(json!("k"));
        broken["baseURL"] = json!("not-a-url");
        assert!(validate_provider("ok", &broken).is_err());
        broken = valid_config(json!("k"));
        broken["models"] = json!({});
        assert!(validate_provider("ok", &broken).is_err());
        broken = valid_config(json!(42));
        assert!(validate_provider("ok", &broken).is_err());
        broken = valid_config(json!("k"));
        broken["api"] = json!("grpc");
        assert!(validate_provider("ok", &broken).is_err());
    }

    #[test]
    fn invalid_provider_json_is_never_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let (providers, auth) = paths(dir.path());
        fs::write(&providers, "{not json").unwrap();
        assert!(
            write_provider_entry(&providers, &auth, "x", &valid_config(json!("k")), true).is_err()
        );
        assert_eq!(fs::read_to_string(&providers).unwrap(), "{not json");
        assert!(!auth.exists());
    }

    #[test]
    fn auth_file_name_follows_the_cli_environment_variable() {
        assert_eq!(auth_file_name(None), "auth.json");
        assert_eq!(auth_file_name(Some("")), "auth.json");
        assert_eq!(auth_file_name(Some("prod")), "auth.json");
        assert_eq!(auth_file_name(Some(" local ")), "auth.local.json");
        assert_eq!(auth_file_name(Some("staging")), "auth.staging.json");
    }

    #[test]
    fn key_reference_syntax_matches_the_cli() {
        for reference in [
            "$VAR",
            "$_VAR1",
            "Bearer {env:TOKEN}",
            "!op read key",
            "{env:A}",
        ] {
            assert!(is_api_key_reference(reference), "{reference}");
        }
        for raw in [
            "sk-live",
            "$",
            "${VAR}",
            "{env:}",
            "{env:1BAD}",
            "Bearer sk-live",
        ] {
            assert!(!is_api_key_reference(raw), "{raw}");
        }
    }
}
