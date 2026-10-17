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
) -> Result<(), AppError> {
    let mut bytes =
        serde_json::to_vec_pretty(document).map_err(|source| AppError::JsonSerialize { source })?;
    bytes.push(b'\n');
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

/// Root keys owned by the curated Oh My Pi form controls. They are serialized
/// from the settings object; everything else may come from the raw passthrough
/// text (with comments) the user typed in the YAML editor.
const CONTROLLED_PROVIDER_KEYS: [&str; 7] = [
    "name", "baseUrl", "api", "apiKey", "headers", "compat", "models",
];

/// Byte range of a top-level (`column 0`) YAML section, hermes-style: the
/// section spans from its key line to the next top-level key line or EOF.
fn top_level_section_range(raw: &str, section_key: &str) -> Option<(usize, usize)> {
    let target = format!("{section_key}:");
    let mut start = None;
    let mut offset = 0;
    for line in raw.split('\n') {
        let trimmed = line.trim_end_matches('\r');
        let is_key_line = !line.is_empty()
            && !line.starts_with([' ', '\t', '#', '-'])
            && line.find(':').is_some_and(|colon| {
                line[colon + 1..].is_empty() || line[colon + 1..].starts_with([' ', '\t', '\r'])
            });
        if start.is_none() {
            if is_key_line && trimmed.starts_with(&target) {
                let after = &trimmed[target.len()..];
                if after.is_empty() || after.starts_with([' ', '\t']) {
                    start = Some(offset);
                }
            }
        } else if is_key_line {
            if let Some(start) = start {
                return Some((start, offset));
            }
        }
        offset += line.len() + 1;
    }
    start.map(|start| (start, raw.len()))
}

fn line_indent(line: &str) -> usize {
    line.len() - line.trim_start_matches([' ', '\t']).len()
}

/// Byte range of one `providers.<key>` entry inside the `providers:` section.
///
/// Entries are the first indented mapping level below `providers:`; an entry
/// ends at the next line indented no deeper (a sibling entry or the end of the
/// section). Blank lines and comments belong to whatever follows them.
fn provider_entry_range(raw: &str, provider_key: &str) -> Option<(usize, usize)> {
    let (section_start, section_end) = top_level_section_range(raw, "providers")?;
    let target = format!("{provider_key}:");
    let mut entry_indent: Option<usize> = None;
    let mut entry_start: Option<usize> = None;
    let mut offset = section_start;
    for line in raw[section_start..section_end].split('\n') {
        let trimmed_end = line.trim_end_matches('\r');
        let trimmed = trimmed_end.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            offset += line.len() + 1;
            continue;
        }
        let indent = line_indent(trimmed_end);
        // The `providers:` key line itself (indent 0) must not set the entry
        // indent; entries are the first indented level below it.
        if entry_indent.is_none() && indent == 0 {
            offset += line.len() + 1;
            continue;
        }
        match entry_indent {
            None => entry_indent = Some(indent),
            Some(entry_indent) => {
                if let Some(entry_start) = entry_start {
                    if indent <= entry_indent {
                        return Some((entry_start, offset));
                    }
                }
            }
        }
        if entry_start.is_none()
            && indent == entry_indent.expect("set above")
            && trimmed.starts_with(&target)
        {
            let after = &trimmed[target.len()..];
            if after.is_empty() || after.starts_with([' ', '\t']) {
                entry_start = Some(offset);
            }
        }
        offset += line.len() + 1;
    }
    entry_start.map(|start| (start, section_end.min(offset)))
}

/// The raw text of one provider entry (as written in models.yml), or `None`
/// when the entry is absent. Indentation is stripped so the result can be
/// shown in the "other fields" YAML editor.
pub(crate) fn read_ohmypi_provider_entry_raw(
    provider_key: &str,
) -> Result<Option<String>, AppError> {
    let _guard = lock_files()?;
    let path = get_ohmypi_models_path()?;
    if !path.exists() {
        return Ok(None);
    }
    let raw = String::from_utf8(read_file_limited(&path, "Oh My Pi models")?).map_err(|error| {
        AppError::Config(format!(
            "Oh My Pi models file must be UTF-8 ({}): {error}",
            path.display()
        ))
    })?;
    Ok(entry_raw_from_source(&raw, provider_key))
}

/// The raw text of every requested provider entry, from a single file read.
/// Missing keys are simply absent from the result.
pub(crate) fn read_ohmypi_provider_entries_raw(
    provider_keys: &[String],
) -> Result<IndexMap<String, String>, AppError> {
    let _guard = lock_files()?;
    let path = get_ohmypi_models_path()?;
    let mut entries = IndexMap::new();
    if !path.exists() {
        return Ok(entries);
    }
    let raw = String::from_utf8(read_file_limited(&path, "Oh My Pi models")?).map_err(|error| {
        AppError::Config(format!(
            "Oh My Pi models file must be UTF-8 ({}): {error}",
            path.display()
        ))
    })?;
    for key in provider_keys {
        if let Some(text) = entry_raw_from_source(&raw, key) {
            entries.insert(key.clone(), text);
        }
    }
    Ok(entries)
}

fn entry_raw_from_source(raw: &str, provider_key: &str) -> Option<String> {
    let (start, end) = provider_entry_range(raw, provider_key)?;
    let entry = &raw[start..end];
    // Skip the `<key>:` line and dedent its indent level plus one more: the
    // editor text is the entry's fields at column 0 ("other fields" YAML),
    // not the keyed entry. The key line's comment never reaches the editor;
    // the writer preserves it in place instead.
    let key_indent = entry.lines().next().map(line_indent).unwrap_or(0);
    let strip = key_indent + 2;
    let dedented = entry
        .lines()
        .skip(1)
        .map(|line| {
            if line.len() >= strip && line[..strip].trim().is_empty() {
                &line[strip..]
            } else {
                line.trim_start_matches('\r')
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    Some(dedented.trim_end_matches(['\n', '\r', ' ']).to_string())
}

/// Top-level (column 0) lines of a controlled key, plus the nested block that
/// follows them, are dropped from the passthrough text: those keys belong to
/// the curated form fields and are serialized from the settings object.
/// Returns the kept text plus each controlled key's trailing comment (the
/// text after `#` on its key line, e.g. `baseUrl: https://x #local`), which
/// the composer re-attaches to the serialized line.
fn drop_controlled_keys(passthrough: &str) -> (String, Vec<(String, String)>) {
    let mut kept: Vec<&str> = Vec::new();
    let mut comments: Vec<(String, String)> = Vec::new();
    let mut skipping_indent: Option<usize> = None;
    for line in passthrough.lines() {
        let trimmed_end = line.trim_end_matches('\r');
        let trimmed = trimmed_end.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            // Comments and blank lines always survive, even inside a skipped
            // controlled-key block — they may document the field after them.
            kept.push(line);
            continue;
        }
        let indent = line_indent(trimmed_end);
        if let Some(limit) = skipping_indent {
            // Nested content (including sequence items at the same indent, as
            // YAML allows for block sequences under a mapping key) belongs to
            // the skipped controlled key.
            if indent > limit || (indent == limit && trimmed.starts_with('-')) {
                continue;
            }
            skipping_indent = None;
        }
        if indent == 0 {
            let key = trimmed.split(':').next().unwrap_or_default().trim();
            if CONTROLLED_PROVIDER_KEYS.contains(&key) {
                skipping_indent = Some(0);
                if let Some((_, comment)) = trimmed_end.split_once('#') {
                    comments.push((key.to_string(), comment.trim().to_string()));
                }
                continue;
            }
        }
        kept.push(line);
    }
    (kept.join("\n"), comments)
}

/// Compose the `models.yml` fragment for one provider entry: curated fields
/// serialized from `config`, then the raw passthrough text (comments kept)
/// indented under the entry. Keys parsed from `passthrough_raw` are excluded
/// from the serialized part so they are written exactly as typed.
fn compose_provider_entry(
    provider_key: &str,
    config: &Value,
    passthrough_raw: Option<&str>,
) -> Result<String, AppError> {
    let Some(object) = config.as_object() else {
        return Err(AppError::Config(
            "Oh My Pi provider configuration must be an object".to_string(),
        ));
    };

    let (passthrough_kept, key_comments) = passthrough_raw
        .map(drop_controlled_keys)
        .unwrap_or_default();
    let mut passthrough_text = passthrough_kept;
    if passthrough_text.trim().is_empty() {
        passthrough_text.clear();
    }

    // A stale editor snapshot may repeat the `<key>:` line (comment included);
    // peel it off so it doesn't leak into the passthrough YAML.
    let first_line = passthrough_text
        .lines()
        .next()
        .unwrap_or_default()
        .to_string();
    if first_line
        .trim_start()
        .starts_with(&format!("{provider_key}:"))
    {
        passthrough_text = passthrough_text
            .lines()
            .skip(1)
            .collect::<Vec<_>>()
            .join("\n");
        if passthrough_text.trim().is_empty() {
            passthrough_text.clear();
        }
    }

    // Which root keys does the passthrough text own? Parsed (not extracted from
    // lines) so quoted/exotic keys are handled correctly.
    let passthrough_keys: std::collections::HashSet<String> = if passthrough_text.is_empty() {
        Default::default()
    } else {
        serde_yaml::from_str::<serde_yaml::Value>(&passthrough_text)
            .ok()
            .and_then(|doc| match doc {
                serde_yaml::Value::Mapping(map) => Some(
                    map.into_iter()
                        .filter_map(|(key, _)| key.as_str().map(str::to_string))
                        .collect(),
                ),
                _ => None,
            })
            .unwrap_or_default()
    };

    let mut curated = serde_yaml::Mapping::new();
    for (key, value) in object {
        if passthrough_keys.contains(key) {
            continue;
        }
        let yaml_value =
            serde_json::from_value::<serde_yaml::Value>(value.clone()).map_err(|error| {
                AppError::Config(format!(
                    "Oh My Pi provider '{provider_key}' could not be converted to YAML: {error}"
                ))
            })?;
        curated.insert(serde_yaml::Value::String(key.clone()), yaml_value);
    }

    let body = if curated.is_empty() {
        format!("{provider_key}:\n")
    } else {
        let mut entry = serde_yaml::Mapping::new();
        entry.insert(
            serde_yaml::Value::String(provider_key.to_string()),
            serde_yaml::Value::Mapping(curated),
        );
        serde_yaml::to_string(&serde_yaml::Value::Mapping(entry)).map_err(|error| {
            AppError::Config(format!(
                "Failed to serialize Oh My Pi provider '{provider_key}': {error}"
            ))
        })?
    };
    let mut body = body.trim_end_matches('\n').to_string();
    // Re-attach the trailing comments users typed on controlled-key lines
    // (e.g. `baseUrl: https://x #local`): the line itself is re-serialized
    // from the form value, only the comment survives from the editor text.
    for (key, comment) in &key_comments {
        if comment.is_empty() {
            continue;
        }
        // serde_yaml 0.9 serializes the entry's fields at indent 2.
        let marker = format!("\n  {key}:");
        if let Some(line_start) = body.find(&marker) {
            // Search past the marker's own leading newline.
            let line_end = body[line_start + 1..]
                .find('\n')
                .map(|end| line_start + 1 + end)
                .unwrap_or(body.len());
            let line = body[line_start..line_end].to_string();
            if !line.contains('#') {
                let annotated = format!("{line}  #{comment}");
                body.replace_range(line_start..line_end, &annotated);
            }
        }
    }
    if !passthrough_text.is_empty() {
        for line in passthrough_text.trim_end().lines() {
            if line.trim().is_empty() {
                continue;
            }
            body.push('\n');
            body.push_str("  ");
            body.push_str(line.trim_end_matches('\r'));
        }
    }
    // The fragment lives inside `providers:` — indent every line by one level.
    let fragment = body
        .lines()
        .map(|line| format!("  {line}"))
        .collect::<Vec<_>>()
        .join("\n");
    Ok(format!("{fragment}\n"))
}

/// Replace (or append) one provider entry in the raw models.yml text, leaving
/// every other byte — comments included — untouched.
fn replace_provider_entry_in_raw(raw: &str, provider_key: &str, entry_fragment: &str) -> String {
    if let Some((start, end)) = provider_entry_range(raw, provider_key) {
        // Preserve the key line's comment (e.g. `example: #local`): it lives
        // outside the editor text, so only the file can carry it. Re-attach
        // it to the regenerated key line unless one was typed there already.
        let old_key_line = raw[start..end].lines().next().unwrap_or_default();
        let new_key_line = entry_fragment.lines().next().unwrap_or_default();
        let owned_fragment;
        let old_comment = old_key_line.split_once('#').map(|(_, c)| c);
        let new_comment = new_key_line.split_once('#').map(|(_, c)| c);
        let entry_fragment = if let (Some(comment), None) = (old_comment, new_comment) {
            let mut fragment = new_key_line.to_string();
            fragment.push_str(" #");
            fragment.push_str(comment.trim_end_matches('\r'));
            fragment.push('\n');
            fragment.push_str(
                &entry_fragment
                    .lines()
                    .skip(1)
                    .collect::<Vec<_>>()
                    .join("\n"),
            );
            if entry_fragment.ends_with('\n') {
                fragment.push('\n');
            }
            owned_fragment = fragment;
            owned_fragment.as_str()
        } else {
            entry_fragment
        };
        let mut result = String::with_capacity(raw.len());
        result.push_str(&raw[..start]);
        result.push_str(entry_fragment);
        let mut remainder = &raw[end..];
        while remainder.starts_with('\n') || remainder.starts_with('\r') {
            remainder = &remainder[1..];
        }
        result.push_str(remainder);
        return result;
    }

    let mut result = raw.to_string();
    if let Some((_, section_end)) = top_level_section_range(raw, "providers") {
        // Insert before the next top-level section (or EOF).
        let insert_at = result.len().min(section_end);
        let mut prefix = result[..insert_at].to_string();
        while prefix.ends_with('\n') || prefix.ends_with('\r') || prefix.ends_with(' ') {
            prefix.pop();
        }
        prefix.push('\n');
        let suffix = result[insert_at..].to_string();
        result = prefix;
        result.push_str(entry_fragment);
        // The fragment already ends with a newline; only add separation when
        // another top-level section follows.
        if !suffix.is_empty() && !suffix.starts_with('\n') {
            result.push('\n');
        }
        result.push_str(&suffix);
    } else {
        if !result.is_empty() && !result.ends_with('\n') {
            result.push('\n');
        }
        result.push_str("providers:\n");
        result.push_str(entry_fragment);
    }
    result
}

/// Remove one provider entry from the raw models.yml text, leaving every
/// other byte — comments included — untouched.
fn remove_provider_entry_from_raw(raw: &str, provider_key: &str) -> String {
    let Some((start, end)) = provider_entry_range(raw, provider_key) else {
        return raw.to_string();
    };
    let mut result = String::with_capacity(raw.len());
    result.push_str(&raw[..start]);
    let mut remainder = &raw[end..];
    while remainder.starts_with('\n') || remainder.starts_with('\r') {
        remainder = &remainder[1..];
    }
    result.push_str(remainder);
    result
}

/// Parse the edited raw text back and verify the composed entry still means
/// exactly `config`; guards against indentation or duplicate-key mistakes.
fn verify_entry(raw: &str, provider_key: &str, config: &Value) -> Result<(), AppError> {
    let document: serde_yaml::Value = serde_yaml::from_str(raw).map_err(|error| {
        AppError::Config(format!(
            "Oh My Pi models text is not valid YAML after editing '{provider_key}': {error}"
        ))
    })?;
    let parsed = document
        .as_mapping()
        .and_then(|root| root.get(serde_yaml::Value::String("providers".to_string())))
        .and_then(|providers| providers.as_mapping())
        .and_then(|providers| providers.get(serde_yaml::Value::String(provider_key.to_string())));
    let parsed_json = parsed
        .map(serde_json::to_value)
        .transpose()
        .map_err(|error| {
            AppError::Config(format!(
                "Oh My Pi provider '{provider_key}' could not be converted from YAML: {error}"
            ))
        })?
        .unwrap_or(Value::Null);
    if parsed_json != *config {
        return Err(AppError::Config(format!(
            "Oh My Pi provider '{provider_key}' round-trip mismatch after text edit: parsed {parsed_json} != config {config} in\n{raw}"
        )));
    }
    Ok(())
}

/// Write one provider entry to models.yml at the text level: only the
/// `providers.<key>` block is rewritten (curated fields serialized from
/// `config`, other fields from `passthrough_raw` verbatim with comments);
/// the rest of the file — comments included — is left untouched.
///
/// Returns `Ok(false)` when nothing changed on disk.
pub(crate) fn write_ohmypi_provider_entry(
    provider_key: &str,
    config: &Value,
    passthrough_raw: Option<&str>,
) -> Result<bool, AppError> {
    validate_provider_node(provider_key, config)?;
    let _guard = lock_files()?;
    let path = get_ohmypi_models_path()?;
    let raw = if path.exists() {
        String::from_utf8(read_file_limited(&path, "Oh My Pi models")?).map_err(|error| {
            AppError::Config(format!(
                "Oh My Pi models file must be UTF-8 ({}): {error}",
                path.display()
            ))
        })?
    } else {
        String::new()
    };
    let entry_fragment = compose_provider_entry(provider_key, config, passthrough_raw)?;
    let new_raw = replace_provider_entry_in_raw(&raw, provider_key, &entry_fragment);
    if new_raw == raw {
        return Ok(false);
    }
    verify_entry(&new_raw, provider_key, config)?;
    ensure_parent(&path)?;
    atomic_write_private(&path, new_raw.as_bytes())?;
    Ok(true)
}

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
    let (document, _) = read_document_with_revision(&path, "Oh My Pi models", true)?;
    match providers(&document, &path)?.get(provider_key) {
        Some(current) if current == config => return Ok(false),
        Some(_) => {
            return Err(AppError::InvalidInput(format!(
                "Oh My Pi provider key '{provider_key}' already exists in models.yml"
            )))
        }
        None => {}
    }

    // Text-level writer: only the `providers.<key>` block is composed; the
    // rest of the file — comments included — is preserved byte for byte.
    drop(_guard);
    write_ohmypi_provider_entry(provider_key, config, None)?;
    Ok(true)
}

pub(crate) fn replace_ohmypi_provider(
    provider_key: &str,
    expected: &Value,
    replacement: &Value,
) -> Result<(), AppError> {
    replace_ohmypi_provider_if_present(provider_key, expected, replacement, None)?.ok_or_else(
        || {
            AppError::Conflict(format!(
                "Oh My Pi provider '{provider_key}' is no longer present in models.yml"
            ))
        },
    )?;
    Ok(())
}

/// Replace a provider entry at the text level so comments survive. Returns the
/// previous entry value.
pub(crate) fn replace_ohmypi_provider_if_present(
    provider_key: &str,
    expected: &Value,
    replacement: &Value,
    passthrough_raw: Option<&str>,
) -> Result<Option<Value>, AppError> {
    validate_provider_node(provider_key, replacement)?;
    let _guard = lock_files()?;
    let path = get_ohmypi_models_path()?;
    let (document, _) = read_document_with_revision(&path, "Oh My Pi models", true)?;
    let Some(current) = providers(&document, &path)?.get(provider_key).cloned() else {
        return Ok(None);
    };
    if current != *expected {
        return Err(AppError::Conflict(format!(
            "Oh My Pi provider '{provider_key}' changed outside CC Switch"
        )));
    }
    if current == *replacement && passthrough_raw.is_none() {
        return Ok(Some(current));
    }
    // Text-level writer takes its own file lock; release ours first. It runs
    // even when the parsed config is unchanged: comment-only edits never
    // change `replacement` yet must reach the file (the writer's own
    // `new_raw == raw` check is the real no-op guard).
    drop(_guard);
    write_ohmypi_provider_entry(provider_key, replacement, passthrough_raw)?;
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
    let (document, expected_revision) =
        read_document_with_revision(&path, "Oh My Pi models", true)?;
    let Some(current) = providers(&document, &path)?.get(provider_key).cloned() else {
        return Ok(None);
    };
    if expected.is_some_and(|expected| current != *expected) {
        return Err(AppError::Conflict(format!(
            "Oh My Pi provider '{provider_key}' changed outside CC Switch"
        )));
    }
    // Text-level splice: only the `providers.<key>` block is removed; the
    // rest of the file — comments included — is preserved byte for byte.
    if path.exists() {
        let raw =
            String::from_utf8(read_file_limited(&path, "Oh My Pi models")?).map_err(|error| {
                AppError::Config(format!(
                    "Oh My Pi models file must be UTF-8 ({}): {error}",
                    path.display()
                ))
            })?;
        let new_raw = remove_provider_entry_from_raw(&raw, provider_key);
        if new_raw != raw {
            // Fail if the file changed between the read above and this write.
            ensure_revision(&path, &expected_revision, "Oh My Pi models")?;
            ensure_parent(&path)?;
            atomic_write_private(&path, new_raw.as_bytes())?;
        }
    }
    Ok(Some(current))
}

pub(crate) fn restore_ohmypi_provider_if_missing(
    provider_key: &str,
    config: &Value,
) -> Result<(), AppError> {
    validate_provider_node(provider_key, config)?;
    let _guard = lock_files()?;
    let path = get_ohmypi_models_path()?;
    let (document, _) = read_document_with_revision(&path, "Oh My Pi models", true)?;
    match providers(&document, &path)?.get(provider_key) {
        Some(current) if current == config => Ok(()),
        Some(_) => Err(AppError::Conflict(format!(
            "cannot restore Oh My Pi provider '{provider_key}' because another value now owns the key"
        ))),
        None => {
            // Text-level writer: only the `providers.<key>` block is composed;
            // the rest of the file — comments included — is preserved.
            drop(_guard);
            write_ohmypi_provider_entry(provider_key, config, None)?;
            Ok(())
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
        // `providers:` with all entries removed parses as null; treat it as
        // empty rather than failing every later read.
        None | Some(Value::Null) => Ok(empty_json_object()),
        Some(Value::Object(providers)) => Ok(providers),
        Some(_) => Err(AppError::Config(format!(
            "Oh My Pi models 'providers' must be an object: {}",
            path.display()
        ))),
    }
}

fn empty_json_object() -> &'static Map<String, Value> {
    static EMPTY: LazyLock<Map<String, Value>> = LazyLock::new(Map::new);
    &EMPTY
}

// ============================================================================
// Settings (config.yml)
// ============================================================================

/// Read the full `config.yml` as a JSON value.
fn read_ohmypi_settings() -> Result<Value, AppError> {
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
        write_document(&path, &document, &expected_revision, "Oh My Pi MCP")?;
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
    write_document(&path, &document, &expected_revision, "Oh My Pi MCP")
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
    write_document(&path, &document, &expected_revision, "Oh My Pi MCP")
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
    fn drop_controlled_keys_keeps_comments_and_unmanaged_fields() {
        let passthrough = "# edited comment\nbaseUrl: https://api.duolalab.test/v1 #local\napi: openai-completions\napiKey: sk-live\n# extra unmanaged field\nauth: none\nmodels:\n- id: glm-4.7\n  name: GLM 4.7\n";
        let (dropped, comments) = drop_controlled_keys(passthrough);
        assert!(dropped.contains("# edited comment"), "{dropped}");
        assert!(dropped.contains("# extra unmanaged field"), "{dropped}");
        assert!(dropped.contains("auth: none"), "{dropped}");
        assert!(!dropped.contains("baseUrl:"), "{dropped}");
        assert!(!dropped.contains("apiKey:"), "{dropped}");
        assert!(!dropped.contains("models:"), "{dropped}");
        assert!(!dropped.contains("glm-4.7"), "{dropped}");
        // Trailing comments on controlled-key lines are captured for the
        // composer to re-attach.
        assert_eq!(comments, vec![("baseUrl".to_string(), "local".to_string())]);
    }

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
    fn controlled_key_trailing_comment_survives_save() {
        let _agent = TestAgentDir::new();
        write_agent_file(
            "models.yml",
            "providers:\n  llama.cpp:\n    api: openai-responses\n    name: llama.cpp\n    baseUrl: http://127.0.0.1:8080\n    auth: none #ab\n    discovery: #ab\n      type: llama.cpp\n",
        );
        let config = json!({
            "api": "openai-responses",
            "name": "llama.cpp",
            "baseUrl": "http://127.0.0.1:8080",
            "auth": "none",
            "discovery": {"type": "llama.cpp"}
        });
        // Editor text mirrors the file plus a new trailing comment on `api`.
        let passthrough = "api: openai-responses #asd\nname: llama.cpp\nbaseUrl: http://127.0.0.1:8080\nauth: none #ab\ndiscovery: #ab\n  type: llama.cpp";
        assert!(
            write_ohmypi_provider_entry("llama.cpp", &config, Some(passthrough)).expect("write"),
            "must not be a no-op"
        );
        let source = read_agent_file("models.yml");
        assert!(source.contains("api: openai-responses  #asd"), "{source}");
        assert!(source.contains("auth: none #ab"), "{source}");
        assert_eq!(
            read_ohmypi_native_provider("llama.cpp").expect("read"),
            Some(config)
        );
    }

    #[test]
    #[serial]
    fn key_line_comment_round_trips_through_raw_editor() {
        let _agent = TestAgentDir::new();
        write_agent_file(
            "models.yml",
            "providers:\n  example: #local\n    baseUrl: https://a.example\n",
        );

        // The key line never reaches the editor text.
        let raw = read_ohmypi_provider_entry_raw("example")
            .expect("read raw")
            .expect("entry present");
        assert!(!raw.contains("example:"), "{raw}");
        assert_eq!(raw, "baseUrl: https://a.example", "{raw}");

        // Saving edited fields keeps the key line comment from the file.
        let config = json!({"baseUrl": "https://b.example"});
        let edited = format!("{raw}\n");
        assert!(
            write_ohmypi_provider_entry("example", &config, Some(&edited))
                .expect("text-level write")
        );

        let source = read_agent_file("models.yml");
        assert!(source.contains("example: #local"), "{source}");
        assert!(source.contains("https://b.example"), "{source}");
        assert_eq!(
            read_ohmypi_native_provider("example").expect("read"),
            Some(config.clone())
        );

        // Removing the comment in the file and saving again drops it.
        write_agent_file(
            "models.yml",
            "providers:\n  example:\n    baseUrl: https://b.example\n",
        );
        let changed_config = json!({"baseUrl": "https://c.example"});
        assert!(
            write_ohmypi_provider_entry("example", &changed_config, Some(&edited))
                .expect("write again")
        );
        assert!(
            !read_agent_file("models.yml").contains("example: #"),
            "{}",
            read_agent_file("models.yml")
        );
    }

    #[test]
    #[serial]
    fn comment_only_edit_reaches_file_despite_unchanged_config() {
        let _agent = TestAgentDir::new();
        let config = json!({"baseUrl": "https://a.example", "auth": "none"});
        insert_ohmypi_provider("example", &config).expect("insert");

        // User only adds a comment in the editor: parsed config is unchanged,
        // but the new comment must reach the file.
        let passthrough = "# my note\nauth: none\n";
        let previous =
            replace_ohmypi_provider_if_present("example", &config, &config, Some(passthrough))
                .expect("replace")
                .expect("present");
        assert_eq!(previous, config);

        let source = read_agent_file("models.yml");
        assert!(source.contains("# my note"), "{source}");
        assert_eq!(
            read_ohmypi_native_provider("example").expect("read"),
            Some(config)
        );
    }

    #[test]
    #[serial]
    fn text_level_edit_preserves_comments_and_sibling_entries() {
        let _agent = TestAgentDir::new();
        write_agent_file(
            "models.yml",
            "# top comment\nproviders:\n  # sibling comment\n  existing:\n    baseUrl: https://a.example\n    compat:\n      supportsStore: true\n",
        );

        let mut config = provider();
        config["auth"] = json!("none");
        let passthrough = "# unmanaged key\nauth: none\n";
        assert!(
            write_ohmypi_provider_entry("example", &config, Some(passthrough))
                .expect("text-level write")
        );

        let source = read_agent_file("models.yml");
        assert!(source.contains("# top comment"), "{source}");
        assert!(source.contains("# sibling comment"), "{source}");
        assert!(source.contains("# unmanaged key"), "{source}");
        assert!(source.contains("supportsStore: true"), "{source}");
        assert!(source.contains("auth: none"), "{source}");

        // Parsed content must round-trip the full config object.
        let providers = read_ohmypi_native_providers().expect("read providers");
        assert_eq!(providers.get("example"), Some(&config));
        assert!(providers.contains_key("existing"));

        // Raw entry (for the editor) keeps the comment.
        let raw = read_ohmypi_provider_entry_raw("example")
            .expect("read raw entry")
            .expect("entry present");
        assert!(raw.contains("# unmanaged key"), "{raw}");

        // Re-writing with the same passthrough is a no-op.
        assert!(
            !write_ohmypi_provider_entry("example", &config, Some(passthrough))
                .expect("idempotent write")
        );
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
