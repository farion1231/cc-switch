//! DeepSeek Harness' standard `web` profile. Other profiles remain native-owned.
use crate::config::{atomic_write_private, get_home_dir};
use crate::error::AppError;
use indexmap::IndexMap;
use serde_json::{json, Value};
use serde_yaml::Value as Yaml;
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

pub(crate) fn data_dir() -> PathBuf {
    std::env::var_os("DSH_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| get_home_dir().join(".dsh"))
}

pub(crate) fn config_path() -> PathBuf {
    data_dir().join("profiles/web/cordis.patch.yml")
}

fn read_text(path: &Path) -> Result<String, AppError> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(text),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(error) => Err(AppError::io(path, error)),
    }
}

pub(crate) fn entries(text: &str) -> Result<Vec<Yaml>, AppError> {
    let value: Yaml = serde_yaml::from_str(text)
        .map_err(|_| AppError::Config("Invalid DeepSeek Harness profile YAML".into()))?;
    match value {
        Yaml::Null => Ok(vec![]),
        Yaml::Sequence(entries) => Ok(entries),
        _ => Err(AppError::Config(
            "DeepSeek Harness profile must be a YAML sequence".into(),
        )),
    }
}

/// Replace one plugin override, retaining every other entry byte for byte (including !!js).
/// The replaced entry itself is reserialized and must not use tags, anchors or aliases.
fn replace_entry(text: &str, id: &str, entry: Option<Yaml>) -> Result<String, AppError> {
    let parsed = entries(text)?;
    let matching: Vec<_> = parsed
        .iter()
        .enumerate()
        .filter(|(_, entry)| entry["id"].as_str() == Some(id))
        .map(|(index, _)| index)
        .collect();
    if matching.len() > 1 {
        return Err(AppError::Config(format!(
            "Duplicate DeepSeek Harness plugin id '{id}'"
        )));
    }
    replace_patch(text, &parsed, matching.first().copied(), entry)
}

/// Edit a top-level patch, including an insert list without its own plugin id.
fn replace_patch(
    text: &str,
    parsed: &[Yaml],
    index: Option<usize>,
    entry: Option<Yaml>,
) -> Result<String, AppError> {
    // DSH requires a sequence even when the last patch is removed.
    let empty = if parsed.is_empty() || (parsed.len() == 1 && index.is_some()) {
        "[]\n"
    } else {
        ""
    };
    let reserialized = entry.is_some();
    let replacement = entry
        .map(|entry| serde_yaml::to_string(&vec![entry]))
        .transpose()
        .map_err(|error| AppError::Config(error.to_string()))?
        .unwrap_or_else(|| empty.to_owned());
    if parsed.is_empty() {
        // Native profiles start with comments followed by `[]`. Appending a
        // block entry to that flow sequence would produce invalid YAML.
        let mut next: String = text
            .split_inclusive('\n')
            .filter(|line| line.trim().is_empty() || line.trim_start().starts_with('#'))
            .collect();
        if !next.is_empty() && !next.ends_with('\n') {
            next.push('\n');
        }
        next.push_str(&replacement);
        return Ok(next);
    }
    let mut starts = vec![];
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        if line.starts_with("- ") || line.trim_end() == "-" {
            starts.push(offset);
        }
        offset += line.len();
    }
    if parsed.len() != starts.len() {
        return Err(AppError::Config(
            "Use block-style YAML for the DeepSeek Harness profile".into(),
        ));
    }
    if let Some(index) = index {
        let end = starts.get(index + 1).copied().unwrap_or(text.len());
        // serde_yaml drops `!!js` tags and expands anchors, so rewriting an entry
        // that uses them would silently change what DSH evaluates.
        if reserialized
            && text[starts[index]..end]
                .lines()
                .any(crate::omp_config::has_yaml_reference_tokens)
        {
            return Err(AppError::Config(
                "This DeepSeek Harness profile entry uses YAML tags (such as !!js), anchors or aliases; edit it in cordis.patch.yml directly".into(),
            ));
        }
        Ok(format!(
            "{}{}{}",
            &text[..starts[index]],
            replacement,
            &text[end..]
        ))
    } else if replacement.is_empty() {
        Ok(text.into())
    } else {
        Ok(format!(
            "{}{}{}",
            text,
            if text.is_empty() || text.ends_with('\n') {
                ""
            } else {
                "\n"
            },
            replacement
        ))
    }
}

/// Same `<file>.lock` exclusive-create protocol and PID record as dsh-atomic-write.
struct FileLock(PathBuf);
impl FileLock {
    fn acquire(path: &Path) -> Result<Self, AppError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| AppError::io(parent, error))?;
        }
        let lock = PathBuf::from(format!("{}.lock", path.display()));
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&lock)
            .map_err(|error| {
                if error.kind() == std::io::ErrorKind::AlreadyExists {
                    AppError::Conflict(
                        "DeepSeek Harness is saving its configuration; retry shortly".into(),
                    )
                } else {
                    AppError::io(&lock, error)
                }
            })?;
        let guard = Self(lock);
        writeln!(file, "{}", std::process::id()).map_err(|error| AppError::io(&guard.0, error))?;
        Ok(guard)
    }
}
impl Drop for FileLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn restore(path: &Path, previous: Option<Vec<u8>>) -> Result<(), AppError> {
    match previous {
        Some(bytes) => atomic_write_private(path, &bytes),
        None => match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(AppError::io(path, error)),
        },
    }
}

/// Hold the native writer lock through the database commit, rolling back on failure.
fn edit_path_and_commit<T>(
    path: &Path,
    edit: impl FnOnce(&str) -> Result<String, AppError>,
    commit: impl FnOnce() -> Result<T, AppError>,
) -> Result<T, AppError> {
    let _guard = FileLock::acquire(path)?;
    let previous = if path.exists() {
        Some(fs::read(path).map_err(|error| AppError::io(path, error))?)
    } else {
        None
    };
    let next = edit(&read_text(path)?)?;
    atomic_write_private(path, next.as_bytes())?;
    match commit() {
        Ok(result) => Ok(result),
        Err(error) => {
            restore(path, previous).map_err(|rollback| {
                AppError::Message(format!(
                    "{error}; restoring DeepSeek Harness configuration failed: {rollback}"
                ))
            })?;
            Err(error)
        }
    }
}

fn provider_entry(text: &str) -> Result<Yaml, AppError> {
    let found: Vec<_> = entries(text)?
        .into_iter()
        .filter(|entry| entry["id"].as_str() == Some("llm-pi-ai"))
        .collect();
    if found.len() > 1 {
        return Err(AppError::Config("Duplicate llm-pi-ai overrides".into()));
    }
    let mut entry = found.into_iter().next().unwrap_or(
        serde_yaml::to_value(json!({"id":"llm-pi-ai","config":{"providers":{}}})).unwrap(),
    );
    if entry.get("config").is_none() {
        entry["config"] = serde_yaml::to_value(json!({})).unwrap();
    }
    if !entry["config"].is_mapping() {
        return Err(AppError::Config(
            "Invalid DeepSeek Harness llm-pi-ai configuration".into(),
        ));
    }
    if entry["config"].get("providers").is_none() {
        entry["config"]["providers"] = serde_yaml::to_value(json!({})).unwrap();
    }
    if !entry["config"]["providers"].is_mapping() {
        return Err(AppError::Config(
            "Invalid DeepSeek Harness providers mapping".into(),
        ));
    }
    Ok(entry)
}

fn credential_document(path: &Path) -> Result<Yaml, AppError> {
    let text = read_text(path)?;
    if text.trim().is_empty() {
        return Ok(serde_yaml::to_value(json!({"version":1,"refs":{},"records":{}})).unwrap());
    }
    let document: Yaml = serde_yaml::from_str(&text)
        .map_err(|_| AppError::Config("Invalid DeepSeek Harness credentials YAML".into()))?;
    if document["version"].as_u64() != Some(1)
        || (!document["refs"].is_null() && !document["refs"].is_mapping())
    {
        return Err(AppError::Config("Unsupported DeepSeek Harness credential document; migrate it in DeepSeek Harness first".into()));
    }
    Ok(document)
}

pub(crate) fn get_providers() -> Result<IndexMap<String, Value>, AppError> {
    get_providers_from(&config_path(), &data_dir().join(".credentials.yaml"))
}

fn get_providers_from(
    path: &Path,
    credentials_path: &Path,
) -> Result<IndexMap<String, Value>, AppError> {
    let entry = provider_entry(&read_text(path)?)?;
    let credentials = credential_document(credentials_path)?;
    let mut result = IndexMap::new();
    for (key, native) in entry["config"]["providers"].as_mapping().unwrap() {
        let id = key
            .as_str()
            .ok_or_else(|| AppError::Config("Invalid DeepSeek Harness provider id".into()))?;
        let mut config: Value = serde_json::to_value(native).map_err(|_| {
            AppError::Config(format!(
                "Provider '{id}' contains dynamic YAML; edit it in DeepSeek Harness"
            ))
        })?;
        if !config.is_object() {
            return Err(AppError::Config(format!(
                "Invalid DeepSeek Harness provider '{id}'"
            )));
        }
        let reference = config
            .get("apiKeyEnv")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let key = credentials["refs"][reference]
            .as_str()
            .map(str::to_string)
            .or_else(|| std::env::var(reference).ok())
            .unwrap_or_default();
        let mut models = serde_json::Map::new();
        for model in config
            .get("models")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if let Some(id) = model.get("id").and_then(Value::as_str) {
                let mut model = model.clone();
                let mut limits = serde_json::Map::new();
                for (native, editor) in [("contextWindow", "context"), ("maxTokens", "output")] {
                    if let Some(value) = model.get(native) {
                        limits.insert(editor.into(), value.clone());
                    }
                }
                model["limit"] = Value::Object(limits);
                if let Some(efforts) = model.get("reasoningEfforts") {
                    model["reasoning"] = json!(efforts.is_object());
                }
                if let Some(input) = model.get("input").cloned() {
                    model["modalities"] = json!({"input":input,"output":["text"]});
                }
                models.insert(id.into(), model);
            }
        }
        config["name"] = config.get("displayName").cloned().unwrap_or(json!(id));
        config["options"] = json!({"baseURL": config.get("baseURL"),"apiKey":key,"headers":config.get("headers").cloned().unwrap_or(json!({}))});
        config["models"] = Value::Object(models);
        result.insert(id.into(), config);
    }
    Ok(result)
}

pub(crate) fn validate_provider(id: &str, config: &Value) -> Result<(), AppError> {
    if id.is_empty()
        || id == "deepseek-official"
        || id.starts_with('-')
        || id.ends_with('-')
        || id.contains("--")
        || id
            .chars()
            .any(|c| !c.is_ascii_lowercase() && !c.is_ascii_digit() && c != '-')
    {
        return Err(AppError::InvalidInput("DeepSeek Harness provider ids must be lowercase hyphenated identifiers; deepseek-official is reserved".into()));
    }
    if !matches!(
        config["api"].as_str(),
        Some("openai-completions" | "openai-responses" | "anthropic-messages")
    ) {
        return Err(AppError::InvalidInput(
            "Select a supported DeepSeek Harness API protocol".into(),
        ));
    }
    if !config
        .pointer("/options/baseURL")
        .and_then(Value::as_str)
        .and_then(|url| url::Url::parse(url).ok())
        .is_some_and(|url| matches!(url.scheme(), "http" | "https") && url.host_str().is_some())
    {
        return Err(AppError::InvalidInput(
            "Enter a valid DeepSeek Harness endpoint URL".into(),
        ));
    }
    if config
        .pointer("/options/apiKey")
        .and_then(Value::as_str)
        .is_none_or(|key| key.trim().is_empty())
        && config
            .get("apiKeyEnv")
            .and_then(Value::as_str)
            .is_none_or(|reference| reference.is_empty())
    {
        return Err(AppError::InvalidInput(
            "Enter an API key or retain a native credential reference".into(),
        ));
    }
    let Some(models) = config["models"]
        .as_object()
        .filter(|models| !models.is_empty())
    else {
        return Err(AppError::InvalidInput(
            "Add at least one DeepSeek Harness model".into(),
        ));
    };
    for (id, model) in models {
        if id.trim().is_empty() || !model.is_object() {
            return Err(AppError::InvalidInput(
                "Invalid DeepSeek Harness model".into(),
            ));
        }
        for field in ["context", "output"] {
            if model
                .pointer(&format!("/limit/{field}"))
                .is_some_and(|value| !value.is_null() && value.as_u64().is_none_or(|n| n == 0))
            {
                return Err(AppError::InvalidInput(
                    "Model capacities must be positive integers".into(),
                ));
            }
        }
    }
    Ok(())
}

fn update_provider(
    text: &str,
    id: &str,
    config: Option<Value>,
    create: bool,
) -> Result<String, AppError> {
    let mut entry = provider_entry(text)?;
    let providers = entry["config"]["providers"].as_mapping_mut().unwrap();
    if config.is_none() && !providers.contains_key(Yaml::from(id)) {
        return Ok(text.to_owned());
    }
    if create && providers.contains_key(Yaml::from(id)) {
        return Err(AppError::Conflict(format!(
            "DeepSeek Harness provider '{id}' already exists"
        )));
    }
    if let Some(config) = config {
        providers.insert(
            id.into(),
            serde_yaml::to_value(config).map_err(|error| AppError::Config(error.to_string()))?,
        );
    } else {
        providers.remove(Yaml::from(id));
    }
    replace_entry(text, "llm-pi-ai", Some(entry))
}

pub(crate) fn save_and_commit<T>(
    id: &str,
    config: Option<Value>,
    create: bool,
    commit: impl FnOnce() -> Result<T, AppError>,
) -> Result<T, AppError> {
    save_paths_and_commit(
        &config_path(),
        &data_dir().join(".credentials.yaml"),
        id,
        config,
        create,
        commit,
    )
}

fn save_paths_and_commit<T>(
    path: &Path,
    credentials_path: &Path,
    id: &str,
    config: Option<Value>,
    create: bool,
    commit: impl FnOnce() -> Result<T, AppError>,
) -> Result<T, AppError> {
    if config.is_none() && !path.exists() {
        return commit();
    }
    let _credentials_lock = FileLock::acquire(credentials_path)?;
    let previous_credentials = if credentials_path.exists() {
        Some(fs::read(credentials_path).map_err(|error| AppError::io(credentials_path, error))?)
    } else {
        None
    };
    let mut changed_credentials = false;
    let native = if let Some(config) = config {
        validate_provider(id, &config)?;
        let mut native = config.clone();
        let object = native.as_object_mut().unwrap();
        object.remove("name");
        object.remove("options");
        object.remove("kind");
        object.remove("enabled");
        object.remove("headers");
        object.insert(
            "displayName".into(),
            config.get("name").cloned().unwrap_or(json!(id)),
        );
        object.insert("baseURL".into(), config["options"]["baseURL"].clone());
        if let Some(headers) = config.pointer("/options/headers") {
            object.insert("headers".into(), headers.clone());
        }
        let mut models = vec![];
        for (model_id, model) in config["models"].as_object().unwrap() {
            let mut model = model.clone();
            model["id"] = json!(model_id);
            for (from, to) in [("context", "contextWindow"), ("output", "maxTokens")] {
                if let Some(value) = model
                    .pointer(&format!("/limit/{from}"))
                    .filter(|value| !value.is_null())
                    .cloned()
                {
                    model[to] = value;
                }
            }
            model.as_object_mut().unwrap().remove("limit");
            if let Some(input) = model.pointer("/modalities/input").cloned() {
                model["input"] = input;
            }
            let object = model.as_object_mut().unwrap();
            object.remove("modalities");
            if object.get("reasoning") == Some(&Value::Bool(false)) {
                object.insert("reasoningEfforts".into(), Value::Bool(false));
            } else if object.get("reasoning") == Some(&Value::Bool(true))
                && !object.contains_key("reasoningEfforts")
            {
                object.insert(
                    "reasoningEfforts".into(),
                    json!({"off":null,"low":"low","medium":"medium","high":"high"}),
                );
            }
            object.remove("reasoning");
            models.push(model);
        }
        object.insert("models".into(), json!(models));
        if let Some(key) = config
            .pointer("/options/apiKey")
            .and_then(Value::as_str)
            .filter(|key| !key.trim().is_empty())
        {
            let reference = format!(
                "CC_SWITCH_DSH_{}",
                id.replace('-', "_").to_ascii_uppercase()
            );
            let mut credentials = credential_document(credentials_path)?;
            let unchanged_key =
                config
                    .get("apiKeyEnv")
                    .and_then(Value::as_str)
                    .is_some_and(|reference| {
                        credentials["refs"][reference].as_str() == Some(key)
                            || std::env::var(reference).ok().as_deref() == Some(key)
                    });
            if !unchanged_key {
                if credentials["refs"].is_null() {
                    credentials["refs"] = Yaml::Mapping(Default::default());
                }
                credentials["refs"][&reference] = Yaml::from(key);
                let yaml = serde_yaml::to_string(&credentials)
                    .map_err(|error| AppError::Config(error.to_string()))?;
                atomic_write_private(credentials_path, yaml.as_bytes())?;
                changed_credentials = true;
                object.insert("apiKeyEnv".into(), json!(reference));
            }
        }
        Some(native)
    } else {
        None
    };
    let result = edit_path_and_commit(
        path,
        |text| update_provider(text, id, native, create),
        commit,
    );
    if result.is_err() && changed_credentials {
        restore(credentials_path, previous_credentials)?;
    }
    result
}

pub(crate) fn set_provider(id: &str, config: Value) -> Result<(), AppError> {
    save_and_commit(id, Some(config), false, || Ok(()))
}
pub(crate) fn remove_provider(id: &str) -> Result<(), AppError> {
    save_and_commit(id, None, false, || Ok(()))
}
pub(crate) fn provider_key_exists(id: &str) -> Result<bool, AppError> {
    Ok(get_providers()?.contains_key(id))
}

pub(crate) fn mcp_entries() -> Result<Vec<Yaml>, AppError> {
    mcp_entries_from(&read_text(&config_path())?)
}

fn mcp_entries_from(text: &str) -> Result<Vec<Yaml>, AppError> {
    Ok(plugin_entries(&entries(text)?)
        .filter(|(_, _, entry)| entry["name"].as_str() == Some("@deepseek-ai/dsh-mcp-client"))
        .map(|(_, _, entry)| entry.clone())
        .collect())
}

fn plugin_entries(patches: &[Yaml]) -> impl Iterator<Item = (usize, Option<usize>, &Yaml)> {
    patches.iter().enumerate().flat_map(|(index, patch)| {
        std::iter::once((index, None, patch)).chain(
            patch["insert"]
                .as_sequence()
                .into_iter()
                .flatten()
                .enumerate()
                .map(move |(insert_index, entry)| (index, Some(insert_index), entry)),
        )
    })
}

pub(crate) fn sync_mcp_and_commit<T>(
    id: &str,
    spec: Option<&Value>,
    commit: impl FnOnce() -> Result<T, AppError>,
) -> Result<T, AppError> {
    sync_mcp_file_and_commit(&config_path(), id, spec, commit)
}

fn sync_mcp_file_and_commit<T>(
    path: &Path,
    id: &str,
    spec: Option<&Value>,
    commit: impl FnOnce() -> Result<T, AppError>,
) -> Result<T, AppError> {
    if spec.is_none() && !path.exists() {
        return commit();
    }
    if let Some(spec) = spec {
        crate::mcp::validation::validate_server_spec(spec)?;
        if spec["type"] == "sse" {
            return Err(AppError::InvalidInput("DeepSeek Harness supports stdio and Streamable HTTP MCP; use an HTTP endpoint instead of SSE".into()));
        }
        if id.is_empty()
            || id.len() > 32
            || id
                .chars()
                .any(|c| !c.is_ascii_alphanumeric() && c != '_' && c != '-')
        {
            return Err(AppError::InvalidInput(
                "DeepSeek Harness MCP names must contain 1–32 letters, digits, '-' or '_'".into(),
            ));
        }
    }
    edit_path_and_commit(
        path,
        |text| {
            let all_entries = entries(text)?;
            let matches: Vec<_> = plugin_entries(&all_entries)
                .filter(|(_, _, entry)| {
                    entry["name"].as_str() == Some("@deepseek-ai/dsh-mcp-client")
                        && entry["config"]["serverName"].as_str() == Some(id)
                })
                .collect();
            if matches.len() > 1 {
                return Err(AppError::Config(format!(
                    "Duplicate DeepSeek Harness MCP server '{id}'"
                )));
            }
            let location = matches.into_iter().next();
            let existing = location.map(|(_, _, entry)| entry.clone());
            if existing.is_none() && spec.is_none() {
                return Ok(text.to_owned());
            }
            let plugin_id = existing
                .as_ref()
                .and_then(|entry| entry["id"].as_str())
                .map(str::to_owned)
                .unwrap_or_else(|| format!("cc-switch-mcp-{id}"));
            if existing.is_some()
                && existing
                    .as_ref()
                    .and_then(|entry| entry["id"].as_str())
                    .is_none()
            {
                return Err(AppError::Config(
                    "Assign an id to this native MCP entry before editing it in CC Switch".into(),
                ));
            }
            let id_matches = plugin_entries(&all_entries)
                .filter(|(_, _, entry)| entry["id"].as_str() == Some(&plugin_id))
                .count();
            if id_matches > usize::from(existing.is_some()) {
                return Err(AppError::Conflict(format!(
                    "DeepSeek Harness plugin id '{plugin_id}' is already in use"
                )));
            }
            let next = if let Some(spec) = spec {
                let mut entry = existing.unwrap_or(
                    serde_yaml::to_value(
                        json!({"id":plugin_id,"name":"@deepseek-ai/dsh-mcp-client","config":{}}),
                    )
                    .unwrap(),
                );
                entry
                    .as_mapping_mut()
                    .unwrap()
                    .remove(Yaml::from("disabled"));
                let config = entry["config"].as_mapping_mut().ok_or_else(|| {
                    AppError::Config("Invalid DeepSeek Harness MCP configuration".into())
                })?;
                for field in [
                    "command",
                    "args",
                    "env",
                    "cwd",
                    "url",
                    "headers",
                    "transport",
                ] {
                    config.remove(Yaml::from(field));
                }
                for (key, value) in spec.as_object().unwrap() {
                    if key != "type" {
                        config.insert(key.as_str().into(), serde_yaml::to_value(value).unwrap());
                    }
                }
                config.insert("serverName".into(), id.into());
                config.insert(
                    "transport".into(),
                    if spec.get("url").is_some() {
                        "streamable-http"
                    } else {
                        "stdio"
                    }
                    .into(),
                );
                Some(entry)
            } else {
                None
            };
            match location {
                Some((index, Some(insert_index), _)) => {
                    let mut patch = all_entries[index].clone();
                    let inserted = patch["insert"].as_sequence_mut().unwrap();
                    if let Some(next) = next {
                        inserted[insert_index] = next;
                    } else {
                        inserted.remove(insert_index);
                    }
                    let next = if inserted.is_empty() {
                        None
                    } else {
                        Some(patch)
                    };
                    replace_patch(text, &all_entries, Some(index), next)
                }
                Some((index, None, _)) => replace_patch(text, &all_entries, Some(index), next),
                None => replace_patch(
                    text,
                    &all_entries,
                    None,
                    next.map(|entry| {
                        let mut patch = serde_yaml::Mapping::new();
                        patch.insert("insert".into(), Yaml::Sequence(vec![entry]));
                        Yaml::Mapping(patch)
                    }),
                ),
            }
        },
        commit,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    // @deepseek-ai/dsh-app-boot@0.2.1-rc.2 PROFILE_PATCH_TEMPLATE.
    const FACTORY_PROFILE: &str = "# Your patch layer for this dsh profile, applied after every bundle layer:\n# a top-level YAML array of loader patch entries (id-targeted config\n# overrides, disables, and insert lists; `!!js` expressions allowed).\n[]\n";

    fn editor_config() -> Value {
        json!({"name":"Gateway","api":"openai-responses","options":{"baseURL":"https://gateway.example/v1","apiKey":"test-value","headers":{"X-Client":"cc-switch"}},"models":{"test-model":{"name":"Model","limit":{"context":100000,"output":1000},"modalities":{"input":["text","image"],"output":["text"]},"reasoningEfforts":{"off":null,"high":"high"},"compat":{"supportsStore":false}}}})
    }

    #[test]
    fn provider_initializes_factory_profile_as_valid_block_yaml() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("cordis.patch.yml");
        let credentials = temp.path().join(".credentials.yaml");
        fs::write(&path, FACTORY_PROFILE).unwrap();
        save_paths_and_commit(
            &path,
            &credentials,
            "gateway",
            Some(editor_config()),
            true,
            || Ok(()),
        )
        .unwrap();
        let text = read_text(&path).unwrap();
        assert!(text.starts_with(FACTORY_PROFILE.trim_end_matches("[]\n")));
        assert_eq!(entries(&text).unwrap().len(), 1);
        assert_eq!(get_providers_from(&path, &credentials).unwrap().len(), 1);
        assert!(!text.lines().any(|line| line.trim() == "[]"));
    }

    #[test]
    fn mcp_initializes_factory_profile_as_valid_block_yaml() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("cordis.patch.yml");
        fs::write(&path, FACTORY_PROFILE).unwrap();
        sync_mcp_file_and_commit(
            &path,
            "test",
            Some(&json!({"type":"http","url":"https://example.com/mcp"})),
            || Ok(()),
        )
        .unwrap();
        let text = read_text(&path).unwrap();
        assert!(text.starts_with(FACTORY_PROFILE.trim_end_matches("[]\n")));
        let entries = entries(&text).unwrap();
        assert_eq!(entries.len(), 1);
        // DSH only creates plugins from insert lists; id-only patches cannot
        // create a plugin absent from the bundle's existing entry list.
        let plugins = entries[0]["insert"].as_sequence().unwrap();
        assert_eq!(plugins.len(), 1);
        assert_eq!(plugins[0]["id"], "cc-switch-mcp-test");
        assert_eq!(plugins[0]["name"], "@deepseek-ai/dsh-mcp-client");
        assert_eq!(plugins[0]["config"]["serverName"], "test");
        assert_eq!(plugins[0]["config"]["transport"], "streamable-http");
        assert_eq!(mcp_entries_from(&text).unwrap(), *plugins);
        assert!(!text.lines().any(|line| line.trim() == "[]"));
    }

    #[test]
    fn removing_last_mcp_restores_an_empty_yaml_sequence() {
        for original in [FACTORY_PROFILE, ""] {
            let temp = tempfile::tempdir().unwrap();
            let path = temp.path().join("cordis.patch.yml");
            fs::write(&path, original).unwrap();
            sync_mcp_file_and_commit(&path, "test", Some(&json!({"command":"node"})), || Ok(()))
                .unwrap();
            sync_mcp_file_and_commit(&path, "test", None, || Ok(())).unwrap();
            let text = read_text(&path).unwrap();
            let document: Yaml = serde_yaml::from_str(&text).unwrap();
            assert_eq!(document, Yaml::Sequence(vec![]));
            assert_eq!(
                text,
                if original.is_empty() {
                    "[]\n"
                } else {
                    original
                }
            );
        }
        assert_eq!(replace_entry("- id: last\n", "last", None).unwrap(), "[]\n");
    }

    #[test]
    fn provider_credentials_allow_missing_or_null_refs_and_preserve_login_records() {
        for refs in ["", "refs: null\n"] {
            let temp = tempfile::tempdir().unwrap();
            let path = temp.path().join("cordis.patch.yml");
            let credentials = temp.path().join(".credentials.yaml");
            let original =
                format!("version: 1\n{refs}records: {{owner: {{account: {{value: keep}}}}}}\n");
            fs::write(&credentials, &original).unwrap();
            let original_document: Yaml = serde_yaml::from_str(&original).unwrap();
            assert!(get_providers_from(&path, &credentials).unwrap().is_empty());
            // Existing providers also remain readable without a stored ref.
            fs::write(&path, "- id: llm-pi-ai\n  config:\n    providers:\n      native: {apiKeyEnv: CC_SWITCH_DSH_TEST_MISSING_REF, models: []}\n").unwrap();
            assert_eq!(get_providers_from(&path, &credentials).unwrap().len(), 1);
            assert_eq!(read_text(&credentials).unwrap(), original);
            save_paths_and_commit(
                &path,
                &credentials,
                "gateway",
                Some(editor_config()),
                true,
                || Ok(()),
            )
            .unwrap();
            let saved = credential_document(&credentials).unwrap();
            assert_eq!(saved["records"], original_document["records"]);
            assert_eq!(saved["refs"]["CC_SWITCH_DSH_GATEWAY"], "test-value");
            assert_eq!(
                get_providers_from(&path, &credentials).unwrap()["gateway"]["options"]["apiKey"],
                "test-value"
            );
        }
    }

    #[test]
    fn provider_credentials_still_reject_invalid_refs_sections() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join(".credentials.yaml");
        for refs in ["[]", "invalid", "123"] {
            fs::write(&path, format!("version: 1\nrefs: {refs}\n")).unwrap();
            assert!(credential_document(&path).is_err());
        }
    }

    #[test]
    fn provider_round_trip_uses_private_credentials_and_preserves_extensions() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("cordis.patch.yml");
        let credentials = temp.path().join(".credentials.yaml");
        let original = "# native plugin\n- id: other\n  name: my-plugin\n  config: {value: keep}\n";
        fs::write(&path, original).unwrap();
        fs::write(
            &credentials,
            "version: 1\nrefs: {OTHER_KEY: other}\nrecords: {owner: {account: {value: keep}}}\n",
        )
        .unwrap();
        save_paths_and_commit(
            &path,
            &credentials,
            "gateway",
            Some(editor_config()),
            true,
            || Ok(()),
        )
        .unwrap();
        let text = read_text(&path).unwrap();
        assert!(text.starts_with(original));
        assert!(!text.contains("test-value"));
        let entry = provider_entry(&text).unwrap();
        let model = &entry["config"]["providers"]["gateway"]["models"][0];
        assert_eq!(model["contextWindow"], 100000);
        assert_eq!(model["input"][1], "image");
        assert_eq!(model["compat"]["supportsStore"], false);
        let imported = get_providers_from(&path, &credentials).unwrap();
        assert_eq!(imported["gateway"]["options"]["apiKey"], "test-value");
        let previous_credentials = fs::read(&credentials).unwrap();
        let mut edited = imported["gateway"].clone();
        edited["name"] = json!("Renamed");
        save_paths_and_commit(&path, &credentials, "gateway", Some(edited), false, || {
            Ok(())
        })
        .unwrap();
        assert_eq!(fs::read(&credentials).unwrap(), previous_credentials);
        assert_eq!(
            credential_document(&credentials).unwrap()["refs"]["OTHER_KEY"],
            "other"
        );
        save_paths_and_commit(&path, &credentials, "gateway", None, false, || Ok(())).unwrap();
        assert!(get_providers_from(&path, &credentials).unwrap().is_empty());
        assert_eq!(fs::read(&credentials).unwrap(), previous_credentials);
    }

    #[test]
    fn provider_and_credentials_roll_back_together_on_failure() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("cordis.patch.yml");
        let credentials = temp.path().join(".credentials.yaml");
        fs::write(&path, "- id: other\n").unwrap();
        let fail = || Err::<(), _>(AppError::Database("injected failure".into()));
        assert!(save_paths_and_commit(
            &path,
            &credentials,
            "gateway",
            Some(editor_config()),
            true,
            fail
        )
        .is_err());
        assert_eq!(read_text(&path).unwrap(), "- id: other\n");
        assert!(!credentials.exists());
        let _guard = FileLock::acquire(&path).unwrap();
        assert!(save_paths_and_commit(
            &path,
            &credentials,
            "gateway",
            Some(editor_config()),
            true,
            || Ok(())
        )
        .is_err());
        assert!(!credentials.exists());
    }

    #[test]
    fn mcp_transport_changes_preserve_native_options_and_other_plugins() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("cordis.patch.yml");
        fs::write(&path,"# keep\n- id: other\n  config: {dynamic: !!js process.env.VALUE}\n- id: github\n  name: '@deepseek-ai/dsh-mcp-client'\n  disabled: true\n  config:\n    serverName: github\n    transport: stdio\n    command: node\n    toolCallTimeoutMs: 12345\n").unwrap();
        sync_mcp_file_and_commit(
            &path,
            "github",
            Some(&json!({"type":"http","url":"https://example.com/mcp"})),
            || Ok(()),
        )
        .unwrap();
        let text = read_text(&path).unwrap();
        assert!(text.contains("dynamic: !!js process.env.VALUE"));
        let entry = entries(&text).unwrap().remove(1);
        assert_eq!(entry["config"]["toolCallTimeoutMs"], 12345);
        assert_eq!(entry["config"]["transport"], "streamable-http");
        assert!(entry["config"].get("command").is_none());
        assert!(entry.get("disabled").is_none());
        assert!(sync_mcp_file_and_commit(
            &path,
            "github",
            Some(&json!({"type":"sse","url":"https://example.com/sse"})),
            || Ok(())
        )
        .is_err());
        assert_eq!(read_text(&path).unwrap(), text);
    }

    #[test]
    fn mcp_edits_and_removes_inserted_plugins_without_removing_siblings() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("cordis.patch.yml");
        let untouched = "# keep\n- id: other\n  config: {dynamic: !!js process.env.VALUE}\n";
        let group = "- insert:\n  - id: github\n    name: '@deepseek-ai/dsh-mcp-client'\n    disabled: true\n    config: {serverName: github, transport: stdio, command: node, toolCallTimeoutMs: 12345}\n  - id: sibling\n    name: custom-plugin\n    config: {dynamic: other}\n";
        fs::write(&path, format!("{untouched}{group}")).unwrap();
        let sibling = entries(group).unwrap()[0]["insert"][1].clone();
        assert_eq!(mcp_entries_from(group).unwrap().len(), 1);
        sync_mcp_file_and_commit(
            &path,
            "github",
            Some(&json!({"type":"http","url":"https://example.com/mcp"})),
            || Ok(()),
        )
        .unwrap();
        let text = read_text(&path).unwrap();
        assert!(text.starts_with(untouched));
        let entry = mcp_entries_from(&text).unwrap().remove(0);
        assert_eq!(entry["config"]["toolCallTimeoutMs"], 12345);
        assert_eq!(entry["config"]["transport"], "streamable-http");
        assert!(entry["config"].get("command").is_none());
        assert!(entry.get("disabled").is_none());
        sync_mcp_file_and_commit(&path, "github", None, || Ok(())).unwrap();
        let text = read_text(&path).unwrap();
        assert!(text.starts_with(untouched));
        assert!(mcp_entries_from(&text).unwrap().is_empty());
        assert_eq!(
            entries(&text).unwrap()[1]["insert"],
            Yaml::Sequence(vec![sibling])
        );
    }

    #[test]
    fn mcp_edits_refuse_to_drop_tags_from_rewritten_entries() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("cordis.patch.yml");
        let spec = json!({"type":"stdio","command":"node"});
        // Editing or removing one plugin rewrites its whole insert group.
        let group = "- insert:\n  - id: github\n    name: '@deepseek-ai/dsh-mcp-client'\n    config: {serverName: github, transport: stdio, command: node}\n  - id: sibling\n    name: custom-plugin\n    config: {dynamic: !!js process.env.OTHER}\n";
        fs::write(&path, group).unwrap();
        for next in [Some(&spec), None] {
            assert!(sync_mcp_file_and_commit(&path, "github", next, || Ok(())).is_err());
            assert_eq!(read_text(&path).unwrap(), group);
        }
        // A tagged top-level entry can't be edited, but removing it rewrites nothing else.
        let own = "- id: cc-switch-mcp-github\n  name: '@deepseek-ai/dsh-mcp-client'\n  config: {serverName: github, transport: stdio, command: node, env: {TOKEN: !!js process.env.TOKEN}}\n";
        fs::write(&path, own).unwrap();
        assert!(sync_mcp_file_and_commit(&path, "github", Some(&spec), || Ok(())).is_err());
        assert_eq!(read_text(&path).unwrap(), own);
        sync_mcp_file_and_commit(&path, "github", None, || Ok(())).unwrap();
        assert_eq!(read_text(&path).unwrap(), "[]\n");
    }

    #[test]
    fn mcp_rejects_plugin_id_collisions_and_rolls_back_failed_commit() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("cordis.patch.yml");
        let original = "- id: cc-switch-mcp-test\n  name: unrelated-plugin\n";
        fs::write(&path, original).unwrap();
        assert!(
            sync_mcp_file_and_commit(&path, "test", Some(&json!({"command":"node"})), || Ok(()))
                .is_err()
        );
        assert_eq!(read_text(&path).unwrap(), original);
        assert!(sync_mcp_file_and_commit(
            &path,
            "other",
            Some(&json!({"command":"node"})),
            || Err::<(), _>(AppError::Database("injected".into()))
        )
        .is_err());
        assert_eq!(read_text(&path).unwrap(), original);
    }

    #[test]
    fn additive_profile_edit_preserves_other_plugins_and_providers() {
        let original = "# user comment\n- id: other\n  name: custom-plugin\n  config:\n    value: !!js process.env.USER_VALUE\n- id: llm-pi-ai\n  config:\n    retryPolicy: {mode: normal}\n    providers:\n      existing: {apiKeyEnv: EXISTING_KEY}\n";
        let next = update_provider(
            original,
            "new-provider",
            Some(json!({"api":"openai-responses","models":[{"id":"model"}]})),
            true,
        )
        .unwrap();
        assert!(next.starts_with(original.split("- id: llm-pi-ai").next().unwrap()));
        let entry = provider_entry(&next).unwrap();
        assert_eq!(entry["config"]["retryPolicy"]["mode"], "normal");
        assert_eq!(
            entry["config"]["providers"]["existing"]["apiKeyEnv"],
            "EXISTING_KEY"
        );
        assert!(update_provider(&next, "new-provider", Some(json!({})), true).is_err());
        let removed = update_provider(&next, "new-provider", None, false).unwrap();
        assert!(provider_entry(&removed).unwrap()["config"]["providers"]
            .get("new-provider")
            .is_none());
    }

    #[test]
    fn provider_edits_refuse_to_reserialize_tags_and_anchors() {
        for native in [
            "      native: {apiKey: !!js process.env.NATIVE_KEY}\n",
            "      native: &shared {apiKeyEnv: NATIVE_KEY}\n      copy: *shared\n",
        ] {
            let original = format!("- id: llm-pi-ai\n  config:\n    providers:\n{native}");
            let error = update_provider(
                &original,
                "new-provider",
                Some(json!({"api":"openai-responses","models":[{"id":"model"}]})),
                true,
            )
            .unwrap_err();
            assert!(error.to_string().contains("YAML tags"));
        }
        // Tag-like text inside quotes and comments is plain data.
        let quoted = "- id: llm-pi-ai\n  config:\n    providers:\n      native: {name: '!!js not a tag', apiKeyEnv: KEY} # !!js note\n";
        let next =
            update_provider(quoted, "new-provider", Some(json!({"models":[]})), true).unwrap();
        assert_eq!(
            provider_entry(&next).unwrap()["config"]["providers"]["native"]["name"],
            "!!js not a tag"
        );
    }

    #[test]
    fn malformed_yaml_and_duplicate_overrides_are_rejected() {
        for text in [
            "{broken",
            "providers: {}",
            "- id: llm-pi-ai\n- id: llm-pi-ai\n",
        ] {
            assert!(update_provider(text, "test", Some(json!({})), false).is_err());
        }
    }

    #[test]
    fn profile_write_holds_native_lock_and_rolls_back_database_failure() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("cordis.patch.yml");
        fs::write(&path, "# keep\n[]\n").unwrap();
        let result: Result<(), AppError> = edit_path_and_commit(
            &path,
            |_| Ok("- id: test\n".into()),
            || {
                assert!(path.with_extension("yml.lock").exists());
                Err(AppError::Database("injected failure".into()))
            },
        );
        assert!(result.is_err());
        assert_eq!(read_text(&path).unwrap(), "# keep\n[]\n");
        let guard = FileLock::acquire(&path).unwrap();
        assert!(edit_path_and_commit(&path, |_| Ok(String::new()), || Ok(())).is_err());
        drop(guard);
        assert!(!path.with_extension("yml.lock").exists());
    }
}
