//! Hermes Agent 配置文件读写模块
//!
//! 处理 `~/.hermes/config.yaml` 配置文件的读写操作（YAML 格式）。
//! Hermes 使用累加式供应商管理，所有供应商配置共存于同一配置文件中。
//!
//! ## 配置结构示例
//!
//! ```yaml
//! model:
//!   default: "anthropic/claude-opus-4-7"
//!   provider: "openrouter"
//!   base_url: "https://openrouter.ai/api/v1"
//!
//! agent:
//!   max_turns: 50
//!   reasoning_effort: "high"
//!
//! custom_providers:
//!   - name: openrouter
//!     base_url: https://openrouter.ai/api/v1
//!     api_key: sk-or-...
//!     model: anthropic/claude-opus-4-7
//!     models:
//!       anthropic/claude-opus-4-7:
//!         context_length: 200000
//!
//! mcp_servers:
//!   filesystem:
//!     command: npx
//!     args: ["-y", "@modelcontextprotocol/server-filesystem"]
//! ```

use crate::config::{atomic_write, get_app_config_dir};
use crate::error::AppError;
use crate::settings::{effective_backup_retain_count, get_hermes_override_dir};
use chrono::Local;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

// ============================================================================
// Path Functions
// ============================================================================

/// 获取 Hermes 配置目录
///
/// 默认路径: `~/.hermes/`
/// 可通过 settings.hermes_config_dir 覆盖
pub fn get_hermes_dir() -> PathBuf {
    if let Some(override_dir) = get_hermes_override_dir() {
        return override_dir;
    }

    crate::config::get_home_dir().join(".hermes")
}

/// 获取 Hermes 配置文件路径
///
/// 返回 `~/.hermes/config.yaml`
pub fn get_hermes_config_path() -> PathBuf {
    get_hermes_dir().join("config.yaml")
}

fn hermes_write_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

// ============================================================================
// Type Definitions
// ============================================================================

/// Hermes 写入结果
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct HermesWriteOutcome {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backup_path: Option<String>,
}

/// Hermes model section config
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HermesModelConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_length: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u64>,
    /// Preserve unknown fields for forward compatibility
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

// ============================================================================
// Core YAML Read Functions
// ============================================================================

/// 读取 Hermes 配置文件为 serde_yaml::Value
///
/// 如果文件不存在，返回空 Mapping
pub fn read_hermes_config() -> Result<serde_yaml::Value, AppError> {
    let path = get_hermes_config_path();
    if !path.exists() {
        return Ok(serde_yaml::Value::Mapping(serde_yaml::Mapping::new()));
    }

    let content = fs::read_to_string(&path).map_err(|e| AppError::io(&path, e))?;
    if content.trim().is_empty() {
        return Ok(serde_yaml::Value::Mapping(serde_yaml::Mapping::new()));
    }

    serde_yaml::from_str(&content)
        .map_err(|e| AppError::Config(format!("Failed to parse Hermes config as YAML: {e}")))
}

// ============================================================================
// YAML Section-Level Replacement
// ============================================================================

/// Check if a line is a YAML top-level key (mapping key at column 0).
///
/// A top-level key line must:
/// - Start at column 0 (no leading whitespace)
/// - Not be empty or whitespace-only
/// - Not be a comment (starting with `#`)
/// - Not be a sequence item (starting with `-`)
/// - Contain `:` followed by space, tab, newline, or end-of-line
fn is_top_level_key_line(line: &str) -> bool {
    if line.is_empty() {
        return false;
    }
    let first_char = line.as_bytes()[0];
    if first_char == b' ' || first_char == b'\t' || first_char == b'#' || first_char == b'-' {
        return false;
    }
    if let Some(colon_pos) = line.find(':') {
        let after_colon = &line[colon_pos + 1..];
        after_colon.is_empty() || after_colon.starts_with(' ') || after_colon.starts_with('\t')
    } else {
        false
    }
}

/// Find the byte range of a top-level YAML section.
///
/// A YAML top-level key is a line that starts at column 0 (no leading
/// whitespace), is not a comment, and contains `:` after the key name.
///
/// Returns `(start_byte_inclusive, end_byte_exclusive)` or `None` if not found.
fn find_yaml_section_range(raw: &str, section_key: &str) -> Option<(usize, usize)> {
    let target = format!("{}:", section_key);
    let mut section_start = None;
    let mut offset = 0;

    for line in raw.split('\n') {
        if section_start.is_none() && is_top_level_key_line(line) && line.starts_with(&target) {
            // Verify exact match: after "key:" must be whitespace or EOL
            let after_target = &line[target.len()..];
            if after_target.is_empty()
                || after_target.starts_with(' ')
                || after_target.starts_with('\t')
                || after_target.starts_with('\r')
            {
                section_start = Some(offset);
            }
        } else if section_start.is_some() && is_top_level_key_line(line) {
            // Found the next top-level key — this is the end of our section
            return Some((section_start.unwrap(), offset));
        }
        offset += line.len() + 1; // +1 for the \n
    }

    // Section extends to end of file
    section_start.map(|start| (start, raw.len()))
}

/// Serialize a section key + value into a YAML fragment like:
///
/// ```yaml
/// model:
///   default: "anthropic/claude-opus-4-7"
///   provider: "openrouter"
/// ```
fn serialize_yaml_section(key: &str, value: &serde_yaml::Value) -> Result<String, AppError> {
    let mut section = serde_yaml::Mapping::new();
    section.insert(serde_yaml::Value::String(key.to_string()), value.clone());
    let yaml_str = serde_yaml::to_string(&serde_yaml::Value::Mapping(section))
        .map_err(|e| AppError::Config(format!("Failed to serialize YAML section '{key}': {e}")))?;
    Ok(yaml_str)
}

/// Replace a YAML section in raw text, or append it if not found.
fn replace_yaml_section(
    raw: &str,
    section_key: &str,
    value: &serde_yaml::Value,
) -> Result<String, AppError> {
    let serialized = serialize_yaml_section(section_key, value)?;

    if let Some((start, end)) = find_yaml_section_range(raw, section_key) {
        let mut result = String::with_capacity(raw.len());
        result.push_str(&raw[..start]);
        result.push_str(&serialized);
        // Ensure proper separation between sections
        let remainder = &raw[end..];
        if !serialized.ends_with('\n') && !remainder.is_empty() && !remainder.starts_with('\n') {
            result.push('\n');
        }
        result.push_str(remainder);
        Ok(result)
    } else {
        // Section not found — append at end
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
}

// ============================================================================
// Backup & Cleanup
// ============================================================================

fn create_hermes_backup(source: &str) -> Result<PathBuf, AppError> {
    let backup_dir = get_app_config_dir().join("backups").join("hermes");
    fs::create_dir_all(&backup_dir).map_err(|e| AppError::io(&backup_dir, e))?;

    let base_id = format!("hermes_{}", Local::now().format("%Y%m%d_%H%M%S"));
    let mut filename = format!("{base_id}.yaml");
    let mut backup_path = backup_dir.join(&filename);
    let mut counter = 1;

    while backup_path.exists() {
        filename = format!("{base_id}_{counter}.yaml");
        backup_path = backup_dir.join(&filename);
        counter += 1;
    }

    atomic_write(&backup_path, source.as_bytes())?;
    cleanup_hermes_backups(&backup_dir)?;
    Ok(backup_path)
}

fn cleanup_hermes_backups(dir: &Path) -> Result<(), AppError> {
    let retain = effective_backup_retain_count();
    let mut entries = fs::read_dir(dir)
        .map_err(|e| AppError::io(dir, e))?
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            entry
                .path()
                .extension()
                .map(|ext| ext == "yaml" || ext == "yml")
                .unwrap_or(false)
        })
        .collect::<Vec<_>>();

    if entries.len() <= retain {
        return Ok(());
    }

    entries.sort_by_key(|entry| entry.metadata().and_then(|m| m.modified()).ok());
    let remove_count = entries.len().saturating_sub(retain);
    for entry in entries.into_iter().take(remove_count) {
        if let Err(err) = fs::remove_file(entry.path()) {
            log::warn!(
                "Failed to remove old Hermes config backup {}: {err}",
                entry.path().display()
            );
        }
    }

    Ok(())
}

// ============================================================================
// High-level Write Helper
// ============================================================================

/// Write a single top-level YAML section to config.yaml using section-level replacement.
///
/// This preserves comments and unrelated sections while only modifying the
/// target section.
fn write_yaml_section_to_config(
    section_key: &str,
    value: &serde_yaml::Value,
) -> Result<HermesWriteOutcome, AppError> {
    let _guard = hermes_write_lock().lock()?;
    write_yaml_section_to_config_locked(section_key, value)
}

/// Inner write helper — caller must already hold the write lock.
fn write_yaml_section_to_config_locked(
    section_key: &str,
    value: &serde_yaml::Value,
) -> Result<HermesWriteOutcome, AppError> {
    let config_path = get_hermes_config_path();
    let raw = if config_path.exists() {
        fs::read_to_string(&config_path).map_err(|e| AppError::io(&config_path, e))?
    } else {
        String::new()
    };

    let new_raw = replace_yaml_section(&raw, section_key, value)?;

    if new_raw == raw {
        return Ok(HermesWriteOutcome::default());
    }

    let backup_path = if !raw.is_empty() {
        Some(create_hermes_backup(&raw)?)
    } else {
        None
    };

    if let Some(parent) = config_path.parent() {
        fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
    }

    atomic_write(&config_path, new_raw.as_bytes())?;

    log::debug!(
        "Hermes config section '{}' written to {:?}",
        section_key,
        config_path
    );
    Ok(HermesWriteOutcome {
        backup_path: backup_path.map(|p| p.display().to_string()),
    })
}

// ============================================================================
// Provider Functions
// ============================================================================

/// Convert a provider's `models` field from a UI-friendly array to the YAML
/// dict shape that Hermes expects.
///
/// Input (from CC Switch UI / database):
/// ```json
/// "models": [{ "id": "foo", "context_length": 200000 }, { "id": "bar" }]
/// ```
///
/// Output (what we write to YAML):
/// ```json
/// "models": { "foo": { "context_length": 200000 }, "bar": {} }
/// ```
///
/// Entries with a missing or empty `id` are dropped. The top-level `id` key
/// is stripped from each value since it now lives on the parent as the map
/// key. Insertion order is preserved (serde_json uses IndexMap under the
/// `preserve_order` feature).
fn models_array_to_dict(array: Vec<serde_json::Value>) -> serde_json::Value {
    let mut map = serde_json::Map::new();
    for item in array {
        let serde_json::Value::Object(mut obj) = item else {
            continue;
        };
        let Some(id) = obj
            .remove("id")
            .and_then(|v| v.as_str().map(|s| s.trim().to_string()))
            .filter(|s| !s.is_empty())
        else {
            continue;
        };
        map.insert(id, serde_json::Value::Object(obj));
    }
    serde_json::Value::Object(map)
}

/// Inverse of [`models_array_to_dict`]. Converts the YAML dict shape back to
/// the UI-friendly ordered array, re-injecting `id` as an object field.
fn models_dict_to_array(dict: serde_json::Map<String, serde_json::Value>) -> serde_json::Value {
    let mut out = Vec::with_capacity(dict.len());
    for (id, value) in dict {
        let mut obj = match value {
            serde_json::Value::Object(obj) => obj,
            serde_json::Value::Null => serde_json::Map::new(),
            other => {
                log::warn!("Unexpected Hermes model entry for '{id}': {other:?}, skipping");
                continue;
            }
        };
        obj.insert("id".to_string(), serde_json::Value::String(id));
        out.push(serde_json::Value::Object(obj));
    }
    serde_json::Value::Array(out)
}

/// Rewrite historical camelCase keys to Hermes' snake_case schema.
///
/// Older DeepLink import paths emitted `baseUrl` / `apiKey` / `apiMode` /
/// `maxTokens` / `contextLength`, which do not belong to Hermes'
/// `_VALID_CUSTOM_PROVIDER_FIELDS` set. Writing those raw to YAML silently
/// poisons `custom_providers:` entries. This sanitiser runs defensively on
/// every `set_provider` call so stored data heals on the next activation;
/// unknown keys pass through untouched to keep forward-compat with new
/// Hermes fields (e.g. `request_timeout_seconds`).
fn sanitize_hermes_provider_keys(config: &mut serde_json::Value) {
    const KEY_ALIASES: &[(&str, &str)] = &[
        ("baseUrl", "base_url"),
        ("apiKey", "api_key"),
        ("apiMode", "api_mode"),
        ("maxTokens", "max_tokens"),
        ("contextLength", "context_length"),
    ];
    // Legacy DeepLink emitted `api: "openai-completions"` which is neither a
    // Hermes field nor mappable to `api_mode`. `_cc_source` / `provider_key`
    // are UI-only markers injected on read — they must never reach YAML.
    const LEGACY_FIELDS_TO_DROP: &[&str] = &["api", PROVIDER_SOURCE_FIELD, "provider_key"];

    let Some(obj) = config.as_object_mut() else {
        return;
    };

    for (from, to) in KEY_ALIASES {
        if let Some(val) = obj.remove(*from) {
            // snake_case wins when both are present; stale camelCase is dropped.
            obj.entry((*to).to_string()).or_insert(val);
        }
    }

    for field in LEGACY_FIELDS_TO_DROP {
        obj.remove(*field);
    }
}

/// If `config.models` is a JSON array, convert it in-place to the dict shape.
/// No-op when `models` is absent or already a dict.
fn normalize_provider_models_for_write(config: &mut serde_json::Value) {
    let Some(obj) = config.as_object_mut() else {
        return;
    };
    let Some(models_val) = obj.get_mut("models") else {
        return;
    };
    if models_val.is_array() {
        let taken = std::mem::take(models_val);
        if let serde_json::Value::Array(arr) = taken {
            *models_val = models_array_to_dict(arr);
        }
    }
}

/// If `config.models` is a JSON dict, convert it in-place to the ordered array
/// shape. No-op when `models` is absent or already an array.
fn denormalize_provider_models_for_read(config: &mut serde_json::Value) {
    let Some(obj) = config.as_object_mut() else {
        return;
    };
    let Some(models_val) = obj.get_mut("models") else {
        return;
    };
    if models_val.is_object() {
        let taken = std::mem::take(models_val);
        if let serde_json::Value::Object(map) = taken {
            *models_val = models_dict_to_array(map);
        }
    }
}

/// Marker field injected on provider payloads sourced from Hermes v12+
/// `providers:` dict. CC Switch treats those as read-only — writes have to
/// go through Hermes' own Web UI to keep its overlay semantics intact.
pub const PROVIDER_SOURCE_FIELD: &str = "_cc_source";
pub const PROVIDER_SOURCE_CUSTOM_LIST: &str = "custom_providers";
pub const PROVIDER_SOURCE_DICT: &str = "providers_dict";

/// Normalize a single entry from the v12+ `providers:` dict into the same
/// JSON shape that `custom_providers:` list entries take, mirroring upstream
/// `_normalize_custom_provider_entry` (hermes_cli/config.py).
///
/// Returns `None` when the entry is not a mapping or lacks any usable name.
fn normalize_providers_dict_entry(
    key: &str,
    entry: &serde_yaml::Value,
) -> Result<Option<serde_json::Value>, AppError> {
    if !entry.is_mapping() {
        return Ok(None);
    }
    let mut json_val = yaml_to_json(entry)?;
    let Some(obj) = json_val.as_object_mut() else {
        return Ok(None);
    };
    // Upstream prefers an explicit `name` when present, falling back to the
    // dict key. Always round-trip it to a trimmed non-empty string.
    let resolved_name = obj
        .get("name")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| key.trim().to_string());
    if resolved_name.is_empty() {
        return Ok(None);
    }
    obj.insert("name".to_string(), serde_json::json!(resolved_name));
    obj.insert("provider_key".to_string(), serde_json::json!(key));
    obj.insert(
        PROVIDER_SOURCE_FIELD.to_string(),
        serde_json::json!(PROVIDER_SOURCE_DICT),
    );
    Ok(Some(json_val))
}

/// Collect provider entries living under the v12+ `providers:` dict.
fn read_providers_dict_entries(config: &serde_yaml::Value) -> Vec<(String, serde_json::Value)> {
    let Some(mapping) = config.get("providers").and_then(|v| v.as_mapping()) else {
        return Vec::new();
    };
    let mut out = Vec::with_capacity(mapping.len());
    for (k, v) in mapping {
        let Some(key_str) = k.as_str().map(str::trim).filter(|s| !s.is_empty()) else {
            continue;
        };
        match normalize_providers_dict_entry(key_str, v) {
            Ok(Some(entry)) => {
                let name = entry
                    .get("name")
                    .and_then(|n| n.as_str())
                    .unwrap_or(key_str)
                    .to_string();
                out.push((name, entry));
            }
            Ok(None) => {
                log::debug!("Skipping Hermes providers['{key_str}']: not a mapping");
            }
            Err(e) => {
                log::warn!("Failed to normalize Hermes providers['{key_str}']: {e}");
            }
        }
    }
    out
}

/// Get all providers as a JSON map keyed by provider name.
///
/// Unions two on-disk sources, matching upstream `get_compatible_custom_providers`:
/// - `custom_providers:` list entries (writable by CC Switch)
/// - `providers:` dict entries (v12+ schema, surfaced read-only with
///   `_cc_source = "providers_dict"` so the UI can disable edit/delete)
///
/// When a name appears in both, the list entry wins (upstream dedup order),
/// keeping CC Switch free to edit it. Models are denormalized from the YAML
/// dict shape to the UI-friendly ordered array.
pub fn get_providers() -> Result<serde_json::Map<String, serde_json::Value>, AppError> {
    let config = read_hermes_config()?;
    let mut map = serde_json::Map::new();

    if let Some(seq) = config.get("custom_providers").and_then(|v| v.as_sequence()) {
        for item in seq {
            if let Some(name) = item.get("name").and_then(|n| n.as_str()) {
                match yaml_to_json(item) {
                    Ok(mut json_val) => {
                        // Heal legacy camelCase records (from older DeepLink
                        // imports) before the UI sees them, so editing doesn't
                        // reveal stale `baseUrl` / `apiKey` fields.
                        sanitize_hermes_provider_keys(&mut json_val);
                        denormalize_provider_models_for_read(&mut json_val);
                        if let Some(obj) = json_val.as_object_mut() {
                            obj.insert(
                                PROVIDER_SOURCE_FIELD.to_string(),
                                serde_json::json!(PROVIDER_SOURCE_CUSTOM_LIST),
                            );
                        }
                        map.insert(name.to_string(), json_val);
                    }
                    Err(e) => {
                        log::warn!("Failed to convert Hermes provider '{name}' to JSON: {e}");
                    }
                }
            }
        }
    }

    for (name, mut entry) in read_providers_dict_entries(&config) {
        if map.contains_key(&name) {
            continue; // list wins over dict on duplicate names
        }
        denormalize_provider_models_for_read(&mut entry);
        map.insert(name, entry);
    }

    Ok(map)
}

/// Reject writes that would target a dict-only overlay entry.
///
/// `verb` is inlined into the user-facing error so both "edit" and "remove"
/// callers can share one implementation.
fn ensure_provider_writable(
    config: &serde_yaml::Value,
    name: &str,
    verb: &str,
) -> Result<(), AppError> {
    if is_dict_only_provider(config, name) {
        return Err(AppError::Config(format!(
            "Provider '{name}' is managed by Hermes' 'providers:' dict — {verb} via Hermes Web UI"
        )));
    }
    Ok(())
}

/// True when `name` appears in `providers:` dict but not in `custom_providers:`
/// list — i.e. it is a read-only overlay CC Switch must not touch.
fn is_dict_only_provider(config: &serde_yaml::Value, name: &str) -> bool {
    let list_has = config
        .get("custom_providers")
        .and_then(|v| v.as_sequence())
        .map(|seq| {
            seq.iter()
                .any(|item| item.get("name").and_then(|n| n.as_str()) == Some(name))
        })
        .unwrap_or(false);
    if list_has {
        return false;
    }
    config
        .get("providers")
        .and_then(|v| v.as_mapping())
        .map(|m| {
            m.iter().any(|(k, v)| {
                let key_matches = k.as_str() == Some(name);
                let name_matches = v
                    .get("name")
                    .and_then(|n| n.as_str())
                    .map(|s| s == name)
                    .unwrap_or(false);
                (key_matches || name_matches) && v.is_mapping()
            })
        })
        .unwrap_or(false)
}

/// Get a single custom provider by name.
pub fn get_provider(name: &str) -> Result<Option<serde_json::Value>, AppError> {
    Ok(get_providers()?.get(name).cloned())
}

/// Set (upsert) a custom provider by name.
///
/// Upserts into the `custom_providers:` YAML sequence (matched by `name`).
/// The entry includes:
///   - `name:` field matching the provider id
///   - singular `model:` field set to the first model id from the `models:`
///     dict — the Hermes runtime and `/model` picker both read this field
///     (runtime_provider.py reads it via `_normalize_custom_provider_entry`;
///     main.py:1436/1450 uses it for picker hints)
///   - plural `models:` dict carrying per-model `context_length` etc.
///
/// The entire read-modify-write is done under the write lock to prevent
/// TOCTOU races.
pub fn set_provider(
    name: &str,
    provider_config: serde_json::Value,
) -> Result<HermesWriteOutcome, AppError> {
    let _guard = hermes_write_lock().lock()?;

    let config = read_hermes_config()?;
    ensure_provider_writable(&config, name, "edit")?;
    let mut providers: Vec<serde_yaml::Value> = config
        .get("custom_providers")
        .and_then(|v| v.as_sequence())
        .cloned()
        .unwrap_or_default();

    // Rewrite any historical camelCase keys (e.g. from older DeepLink imports)
    // before touching models / YAML — avoids writing non-Hermes fields back.
    let mut normalized = provider_config;
    sanitize_hermes_provider_keys(&mut normalized);

    // Normalize `models` from UI array to Hermes YAML dict before serializing.
    normalize_provider_models_for_write(&mut normalized);

    // Extract the first model id (now a key in the normalized dict) so we can
    // propagate it to the singular `model:` field Hermes reads.
    let first_model_id = normalized
        .get("models")
        .and_then(|v| v.as_object())
        .and_then(|obj| obj.keys().next())
        .cloned();

    let mut yaml_val: serde_yaml::Value = json_to_yaml(&normalized)?;
    if let serde_yaml::Value::Mapping(ref mut m) = yaml_val {
        m.insert(
            serde_yaml::Value::String("name".to_string()),
            serde_yaml::Value::String(name.to_string()),
        );
        if let Some(model_id) = first_model_id {
            m.insert(
                serde_yaml::Value::String("model".to_string()),
                serde_yaml::Value::String(model_id),
            );
        } else {
            m.remove(serde_yaml::Value::String("model".to_string()));
        }
    }

    if let Some(existing) = providers
        .iter_mut()
        .find(|p| p.get("name").and_then(|n| n.as_str()) == Some(name))
    {
        // Forward-compat: carry over any on-disk fields the UI payload didn't
        // include. Hermes keeps evolving (e.g. `request_timeout_seconds`,
        // `key_env`), and users may set those via Hermes Web UI — without
        // this merge, a CC Switch edit to an unrelated field would silently
        // strip them on write-back.
        if let (Some(existing_map), serde_yaml::Value::Mapping(new_map)) =
            (existing.as_mapping(), &mut yaml_val)
        {
            for (k, v) in existing_map {
                new_map.entry(k.clone()).or_insert_with(|| v.clone());
            }
        }
        *existing = yaml_val;
    } else {
        providers.push(yaml_val);
    }

    let providers_value = serde_yaml::Value::Sequence(providers);
    write_yaml_section_to_config_locked("custom_providers", &providers_value)
}

/// Remove a custom provider by name.
///
/// Filters out the matching entry from the `custom_providers:` sequence.
/// No-op if the section is missing or no entry matches. The entire
/// read-modify-write is done under the write lock to prevent TOCTOU races.
pub fn remove_provider(name: &str) -> Result<HermesWriteOutcome, AppError> {
    let _guard = hermes_write_lock().lock()?;
    let config = read_hermes_config()?;

    ensure_provider_writable(&config, name, "remove")?;

    let mut providers: Vec<serde_yaml::Value> = config
        .get("custom_providers")
        .and_then(|v| v.as_sequence())
        .cloned()
        .unwrap_or_default();

    let original_len = providers.len();
    providers.retain(|p| p.get("name").and_then(|n| n.as_str()) != Some(name));
    if providers.len() == original_len {
        return Ok(HermesWriteOutcome::default());
    }

    let providers_value = serde_yaml::Value::Sequence(providers);
    write_yaml_section_to_config_locked("custom_providers", &providers_value)
}

// ============================================================================
// Model Config Functions
// ============================================================================

/// Get the `model` section as a typed config.
pub fn get_model_config() -> Result<Option<HermesModelConfig>, AppError> {
    let config = read_hermes_config()?;
    let Some(model_value) = config.get("model") else {
        return Ok(None);
    };
    let json_val = yaml_to_json(model_value)?;
    let model = serde_json::from_value(json_val)
        .map_err(|e| AppError::Config(format!("Failed to parse Hermes model config: {e}")))?;
    Ok(Some(model))
}

/// Set the `model` section.
pub fn set_model_config(model: &HermesModelConfig) -> Result<HermesWriteOutcome, AppError> {
    let json_val =
        serde_json::to_value(model).map_err(|e| AppError::JsonSerialize { source: e })?;
    let yaml_val = json_to_yaml(&json_val)?;
    write_yaml_section_to_config("model", &yaml_val)
}

/// Apply the top-level `model:` defaults when switching to a Hermes provider.
///
/// `model.provider` is **always** updated to the new provider id — without
/// this, switching to a provider whose settings lack a `models` list would
/// leave the runtime routing requests to the previously active provider.
///
/// `model.default` is only overwritten when the new provider declares at
/// least one model; otherwise the previous default is preserved so users
/// still have a runnable configuration (Hermes will surface a clear error
/// if the default no longer belongs to the active provider).
///
/// Existing fields in `model:` (`context_length` / `max_tokens` / `base_url`
/// / `extra`) are preserved via struct-update.
pub fn apply_switch_defaults(
    provider_id: &str,
    settings_config: &serde_json::Value,
) -> Result<HermesWriteOutcome, AppError> {
    let first_model_id = settings_config
        .get("models")
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|m| m.get("id"))
        .and_then(|id| id.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let current = get_model_config()?.unwrap_or_default();
    let merged = HermesModelConfig {
        default: first_model_id.or(current.default.clone()),
        provider: Some(provider_id.to_string()),
        ..current
    };
    set_model_config(&merged)
}

// ============================================================================
// MCP Section Access (for mcp/hermes.rs to use in Phase 4)
// ============================================================================

/// Get the `mcp_servers` section as a YAML Mapping.
pub fn get_mcp_servers_yaml() -> Result<serde_yaml::Mapping, AppError> {
    let config = read_hermes_config()?;
    Ok(config
        .get("mcp_servers")
        .and_then(|v| v.as_mapping())
        .cloned()
        .unwrap_or_default())
}

/// Atomically read-modify-write the `mcp_servers` section under the write lock.
///
/// Prevents TOCTOU races when multiple sync operations run concurrently.
pub fn update_mcp_servers_yaml<F>(updater: F) -> Result<(), AppError>
where
    F: FnOnce(&mut serde_yaml::Mapping) -> Result<(), AppError>,
{
    let _guard = hermes_write_lock().lock()?;
    let config = read_hermes_config()?;
    let mut servers = config
        .get("mcp_servers")
        .and_then(|v| v.as_mapping())
        .cloned()
        .unwrap_or_default();
    updater(&mut servers)?;
    let value = serde_yaml::Value::Mapping(servers);
    write_yaml_section_to_config_locked("mcp_servers", &value)?;
    Ok(())
}

// ============================================================================
// YAML ↔ JSON Conversion Helpers
// ============================================================================

/// Convert a `serde_yaml::Value` to a `serde_json::Value`.
pub(crate) fn yaml_to_json(yaml: &serde_yaml::Value) -> Result<serde_json::Value, AppError> {
    // Serialize YAML value to string, then parse as JSON value.
    // This handles all type mappings correctly.
    let yaml_str = serde_yaml::to_string(yaml)
        .map_err(|e| AppError::Config(format!("Failed to serialize YAML value: {e}")))?;
    serde_yaml::from_str::<serde_json::Value>(&yaml_str)
        .map_err(|e| AppError::Config(format!("Failed to convert YAML to JSON: {e}")))
}

/// Convert a `serde_json::Value` to a `serde_yaml::Value`.
pub(crate) fn json_to_yaml(json: &serde_json::Value) -> Result<serde_yaml::Value, AppError> {
    let json_str = serde_json::to_string(json)
        .map_err(|e| AppError::Config(format!("Failed to serialize JSON value: {e}")))?;
    serde_yaml::from_str(&json_str)
        .map_err(|e| AppError::Config(format!("Failed to convert JSON to YAML: {e}")))
}

// ============================================================================
// Memory Files (~/.hermes/memories/{MEMORY,USER}.md)
// ============================================================================
//
// Hermes Agent persists two memory blobs on disk:
//   - `MEMORY.md` — agent's personal notes, snapshotted into the system prompt
//   - `USER.md`   — user profile, same treatment
// Entries are separated by a `§` on its own line. Hermes' own Web UI only
// exposes on/off toggles and character budgets — it has no content editor.
// CC Switch fills that gap by reading/writing the whole file as a markdown
// blob. Character budgets (`memory_char_limit`, `user_char_limit`) and enable
// flags (`memory_enabled`, `user_profile_enabled`) live at the top level of
// `config.yaml`; Hermes truncates over-budget content at load time.

/// Which of Hermes' two memory files to operate on. Tauri deserializes this
/// directly from the `"memory"` / `"user"` strings the frontend sends, so an
/// unknown value is rejected at the IPC boundary instead of deep in the stack.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MemoryKind {
    Memory,
    User,
}

impl MemoryKind {
    fn filename(self) -> &'static str {
        match self {
            Self::Memory => "MEMORY.md",
            Self::User => "USER.md",
        }
    }
}

fn memories_dir() -> PathBuf {
    get_hermes_dir().join("memories")
}

/// Read a Hermes memory file as a markdown blob. Returns an empty string
/// when the file doesn't exist yet (first-run case).
pub fn read_memory(kind: MemoryKind) -> Result<String, AppError> {
    let path = memories_dir().join(kind.filename());
    match fs::read_to_string(&path) {
        Ok(content) => Ok(content),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(e) => Err(AppError::io(&path, e)),
    }
}

/// Atomically replace a Hermes memory file. `atomic_write` creates parent
/// directories as needed, so `~/.hermes/memories/` is materialized on first
/// write without a separate `create_dir_all` call.
pub fn write_memory(kind: MemoryKind, content: &str) -> Result<(), AppError> {
    let path = memories_dir().join(kind.filename());
    atomic_write(&path, content.as_bytes())
}

/// Character budget + enable flags for the two memory blobs, as configured
/// in Hermes' `config.yaml`. Defaults mirror `~/.hermes`'s own defaults so
/// callers get a usable budget bar even before the user edits config.yaml.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HermesMemoryLimits {
    pub memory: usize,
    pub user: usize,
    pub memory_enabled: bool,
    pub user_enabled: bool,
}

impl Default for HermesMemoryLimits {
    fn default() -> Self {
        Self {
            memory: 2200,
            user: 1375,
            memory_enabled: true,
            user_enabled: true,
        }
    }
}

/// Toggle the on/off flag for one of Hermes' two memory blobs, preserving all
/// other fields in the `memory:` section (character budgets, external provider
/// settings, etc.). Hermes stores the user-profile toggle under
/// `user_profile_enabled` (not `user_enabled`), so the mapping to on-disk keys
/// lives here rather than leaking to callers.
pub fn set_memory_enabled(kind: MemoryKind, enabled: bool) -> Result<HermesWriteOutcome, AppError> {
    let _guard = hermes_write_lock().lock()?;
    let config = read_hermes_config()?;

    let mut memory = match config.get("memory") {
        Some(serde_yaml::Value::Mapping(m)) => m.clone(),
        _ => serde_yaml::Mapping::new(),
    };

    let key = match kind {
        MemoryKind::Memory => "memory_enabled",
        MemoryKind::User => "user_profile_enabled",
    };
    memory.insert(
        serde_yaml::Value::String(key.to_string()),
        serde_yaml::Value::Bool(enabled),
    );

    write_yaml_section_to_config_locked("memory", &serde_yaml::Value::Mapping(memory))
}

/// Read memory budgets + toggles from `config.yaml`. Missing/unparsable
/// fields fall back to `HermesMemoryLimits::default()` rather than erroring,
/// so an empty or partially-populated config still yields a usable UI.
pub fn read_memory_limits() -> Result<HermesMemoryLimits, AppError> {
    let mut out = HermesMemoryLimits::default();
    let config = read_hermes_config()?;
    let Some(memory) = config.get("memory") else {
        return Ok(out);
    };

    if let Some(v) = memory.get("memory_char_limit").and_then(|v| v.as_u64()) {
        out.memory = v as usize;
    }
    if let Some(v) = memory.get("user_char_limit").and_then(|v| v.as_u64()) {
        out.user = v as usize;
    }
    if let Some(v) = memory.get("memory_enabled").and_then(|v| v.as_bool()) {
        out.memory_enabled = v;
    }
    if let Some(v) = memory.get("user_profile_enabled").and_then(|v| v.as_bool()) {
        out.user_enabled = v;
    }

    Ok(out)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use std::sync::{Mutex, OnceLock};

    fn test_guard() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|err| err.into_inner())
    }

    /// Run a test with an isolated temp home directory.
    ///
    /// Saves and restores `CC_SWITCH_TEST_HOME` to avoid interfering with
    /// parallel tests in other modules.
    fn with_test_home<T>(test_fn: impl FnOnce() -> T) -> T {
        let _guard = test_guard();
        let tmp = tempfile::tempdir().unwrap();
        let old_test_home = std::env::var_os("CC_SWITCH_TEST_HOME");
        std::env::set_var("CC_SWITCH_TEST_HOME", tmp.path());
        let result = test_fn();
        match old_test_home {
            Some(value) => std::env::set_var("CC_SWITCH_TEST_HOME", value),
            None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
        }
        result
    }

    // ---- sanitize_hermes_provider_keys tests ----

    #[test]
    fn sanitize_rewrites_camel_case_aliases() {
        let mut v = serde_json::json!({
            "name": "test",
            "baseUrl": "https://api.example.com",
            "apiKey": "sk-123",
            "apiMode": "chat_completions",
            "maxTokens": 8192,
            "contextLength": 200000,
        });
        sanitize_hermes_provider_keys(&mut v);
        let obj = v.as_object().unwrap();
        assert_eq!(obj.get("base_url").unwrap(), "https://api.example.com");
        assert_eq!(obj.get("api_key").unwrap(), "sk-123");
        assert_eq!(obj.get("api_mode").unwrap(), "chat_completions");
        assert_eq!(obj.get("max_tokens").unwrap(), 8192);
        assert_eq!(obj.get("context_length").unwrap(), 200000);
        assert!(obj.get("baseUrl").is_none());
        assert!(obj.get("apiKey").is_none());
    }

    #[test]
    fn sanitize_drops_stale_duplicate_when_snake_case_exists() {
        let mut v = serde_json::json!({
            "baseUrl": "https://old.example.com",
            "base_url": "https://new.example.com",
        });
        sanitize_hermes_provider_keys(&mut v);
        let obj = v.as_object().unwrap();
        // snake_case wins; stale camelCase is dropped
        assert_eq!(obj.get("base_url").unwrap(), "https://new.example.com");
        assert!(obj.get("baseUrl").is_none());
    }

    #[test]
    fn sanitize_drops_legacy_api_field() {
        let mut v = serde_json::json!({
            "base_url": "https://api.example.com",
            "api": "openai-completions",
        });
        sanitize_hermes_provider_keys(&mut v);
        let obj = v.as_object().unwrap();
        assert!(obj.get("api").is_none(), "legacy 'api' key must be removed");
        assert!(obj.get("base_url").is_some());
    }

    #[test]
    fn sanitize_preserves_unknown_fields() {
        let mut v = serde_json::json!({
            "base_url": "https://api.example.com",
            "request_timeout_seconds": 300,
            "rate_limit_delay": 1.5,
        });
        sanitize_hermes_provider_keys(&mut v);
        let obj = v.as_object().unwrap();
        // Forward-compat: Hermes' own new fields pass through untouched
        assert_eq!(obj.get("request_timeout_seconds").unwrap(), 300);
        assert_eq!(obj.get("rate_limit_delay").unwrap(), 1.5);
    }

    #[test]
    fn sanitize_noop_on_non_object() {
        let mut v = serde_json::json!(["not", "an", "object"]);
        sanitize_hermes_provider_keys(&mut v);
        assert!(v.is_array());
    }

    // ---- find_yaml_section_range tests ----

    #[test]
    fn find_section_in_multi_section_yaml() {
        let yaml = "\
model:
  default: gpt-4
  provider: openai
agent:
  max_turns: 10
custom_providers:
  - name: foo
";
        let (start, end) = find_yaml_section_range(yaml, "agent").unwrap();
        let section = &yaml[start..end];
        assert!(section.starts_with("agent:"));
        assert!(section.contains("max_turns"));
        assert!(!section.contains("custom_providers"));
    }

    #[test]
    fn find_section_at_end_of_file() {
        let yaml = "\
model:
  default: gpt-4
agent:
  max_turns: 10
";
        let (start, end) = find_yaml_section_range(yaml, "agent").unwrap();
        let section = &yaml[start..end];
        assert!(section.starts_with("agent:"));
        assert!(section.contains("max_turns"));
        assert_eq!(end, yaml.len());
    }

    #[test]
    fn find_section_not_found() {
        let yaml = "\
model:
  default: gpt-4
";
        assert!(find_yaml_section_range(yaml, "agent").is_none());
    }

    #[test]
    fn find_section_with_comments_between() {
        let yaml = "\
model:
  default: gpt-4

# This is a comment
  # indented comment

agent:
  max_turns: 10
";
        // model section should span from start to "agent:"
        let (start, end) = find_yaml_section_range(yaml, "model").unwrap();
        let section = &yaml[start..end];
        assert!(section.starts_with("model:"));
        // Comments and blank lines between sections are included in the prior section
        assert!(section.contains("# This is a comment"));
    }

    #[test]
    fn find_section_with_empty_lines() {
        let yaml = "\
model:
  default: gpt-4

agent:
  max_turns: 10
";
        let (start, end) = find_yaml_section_range(yaml, "model").unwrap();
        let section = &yaml[start..end];
        assert!(section.starts_with("model:"));
        // Empty lines don't terminate a section
        assert!(section.contains('\n'));
    }

    #[test]
    fn find_section_does_not_match_substring_key() {
        let yaml = "\
model_extra:
  foo: bar
model:
  default: gpt-4
";
        let (start, _end) = find_yaml_section_range(yaml, "model").unwrap();
        let section = &yaml[start..];
        // Should match "model:", not "model_extra:"
        assert!(section.starts_with("model:"));
        assert!(!section.starts_with("model_extra:"));
    }

    // ---- replace_yaml_section tests ----

    #[test]
    fn replace_existing_section() {
        let yaml = "\
model:
  default: gpt-4
  provider: openai
agent:
  max_turns: 10
";
        let new_model = serde_yaml::Value::Mapping({
            let mut m = serde_yaml::Mapping::new();
            m.insert(
                serde_yaml::Value::String("default".to_string()),
                serde_yaml::Value::String("claude-opus-4-7".to_string()),
            );
            m.insert(
                serde_yaml::Value::String("provider".to_string()),
                serde_yaml::Value::String("anthropic".to_string()),
            );
            m
        });

        let result = replace_yaml_section(yaml, "model", &new_model).unwrap();
        // The result should still contain the agent section
        assert!(result.contains("agent:"));
        assert!(result.contains("max_turns"));
        // And the model section should be updated
        assert!(result.contains("claude-opus-4-7"));
        assert!(result.contains("anthropic"));
        assert!(!result.contains("gpt-4"));
        assert!(!result.contains("openai"));
    }

    #[test]
    fn append_new_section() {
        let yaml = "\
model:
  default: gpt-4
";
        let new_agent = serde_yaml::Value::Mapping({
            let mut m = serde_yaml::Mapping::new();
            m.insert(
                serde_yaml::Value::String("max_turns".to_string()),
                serde_yaml::Value::Number(serde_yaml::Number::from(50)),
            );
            m
        });

        let result = replace_yaml_section(yaml, "agent", &new_agent).unwrap();
        assert!(result.contains("model:"));
        assert!(result.contains("gpt-4"));
        assert!(result.contains("agent:"));
        assert!(result.contains("max_turns: 50"));
    }

    #[test]
    fn replace_section_in_empty_file() {
        let yaml = "";
        let new_model = serde_yaml::Value::Mapping({
            let mut m = serde_yaml::Mapping::new();
            m.insert(
                serde_yaml::Value::String("default".to_string()),
                serde_yaml::Value::String("gpt-4".to_string()),
            );
            m
        });

        let result = replace_yaml_section(yaml, "model", &new_model).unwrap();
        assert!(result.contains("model:"));
        assert!(result.contains("gpt-4"));
        assert!(result.ends_with('\n'));
    }

    // ---- Provider CRUD via mock config ----

    #[test]
    #[serial]
    fn provider_crud_roundtrip() {
        with_test_home(|| {
            // Initially no providers
            let providers = get_providers().unwrap();
            assert!(providers.is_empty());

            // Add a provider
            let config = serde_json::json!({
                "base_url": "https://openrouter.ai/api/v1",
                "api_key": "sk-or-test"
            });
            set_provider("openrouter", config).unwrap();

            let providers = get_providers().unwrap();
            assert_eq!(providers.len(), 1);
            assert!(providers.contains_key("openrouter"));

            let provider = get_provider("openrouter").unwrap().unwrap();
            assert_eq!(provider["base_url"], "https://openrouter.ai/api/v1");
            assert_eq!(provider["name"], "openrouter");

            // Update the provider
            let config2 = serde_json::json!({
                "base_url": "https://openrouter.ai/api/v2",
                "api_key": "sk-or-updated"
            });
            set_provider("openrouter", config2).unwrap();

            let provider = get_provider("openrouter").unwrap().unwrap();
            assert_eq!(provider["base_url"], "https://openrouter.ai/api/v2");

            // Remove the provider
            remove_provider("openrouter").unwrap();
            let providers = get_providers().unwrap();
            assert!(providers.is_empty());
        });
    }

    #[test]
    #[serial]
    fn set_provider_preserves_unknown_fields_on_update() {
        // Hermes keeps adding provider-level fields (e.g.
        // `request_timeout_seconds`, `key_env`). Users may set those via
        // Hermes Web UI; a later CC Switch edit must not strip them — set_provider
        // carries over any existing on-disk fields that the UI payload didn't
        // submit.
        with_test_home(|| {
            let yaml = "\
custom_providers:
  - name: acme
    base_url: https://old.example.com
    api_key: sk-old
    request_timeout_seconds: 300
    key_env: ACME_API_KEY
";
            let config_path = get_hermes_config_path();
            fs::create_dir_all(config_path.parent().unwrap()).unwrap();
            fs::write(&config_path, yaml).unwrap();

            let update = serde_json::json!({
                "base_url": "https://new.example.com",
                "api_key": "sk-new"
            });
            set_provider("acme", update).unwrap();

            let provider = get_provider("acme").unwrap().unwrap();
            assert_eq!(provider["base_url"], "https://new.example.com");
            assert_eq!(provider["api_key"], "sk-new");
            assert_eq!(provider["request_timeout_seconds"], 300);
            assert_eq!(provider["key_env"], "ACME_API_KEY");
        });
    }

    #[test]
    #[serial]
    fn get_providers_surfaces_providers_dict_as_read_only() {
        with_test_home(|| {
            let yaml = "\
_config_version: 19
custom_providers:
  - name: mine
    base_url: https://mine.example.com
    api_key: sk-mine
providers:
  anthropic:
    base_url: https://api.anthropic.com
    api_key: sk-ant
    model: claude-opus-4.6
  ollama-local:
    base_url: http://localhost:11434/v1
    request_timeout_seconds: 300
";
            let config_path = get_hermes_config_path();
            fs::create_dir_all(config_path.parent().unwrap()).unwrap();
            fs::write(&config_path, yaml).unwrap();

            let providers = get_providers().unwrap();
            assert_eq!(providers.len(), 3);

            let mine = providers.get("mine").unwrap();
            assert_eq!(mine[PROVIDER_SOURCE_FIELD], PROVIDER_SOURCE_CUSTOM_LIST);

            let anthropic = providers.get("anthropic").unwrap();
            assert_eq!(anthropic[PROVIDER_SOURCE_FIELD], PROVIDER_SOURCE_DICT);
            assert_eq!(anthropic["provider_key"], "anthropic");
            assert_eq!(anthropic["base_url"], "https://api.anthropic.com");

            let ollama = providers.get("ollama-local").unwrap();
            assert_eq!(ollama[PROVIDER_SOURCE_FIELD], PROVIDER_SOURCE_DICT);
            // Forward-compat fields from the dict pass through untouched
            assert_eq!(ollama["request_timeout_seconds"], 300);
        });
    }

    #[test]
    #[serial]
    fn get_providers_list_wins_on_name_collision() {
        with_test_home(|| {
            let yaml = "\
_config_version: 19
custom_providers:
  - name: shared
    base_url: https://writable.example.com
providers:
  shared:
    base_url: https://overlay.example.com
";
            let config_path = get_hermes_config_path();
            fs::create_dir_all(config_path.parent().unwrap()).unwrap();
            fs::write(&config_path, yaml).unwrap();

            let providers = get_providers().unwrap();
            assert_eq!(providers.len(), 1);
            let shared = providers.get("shared").unwrap();
            assert_eq!(shared["base_url"], "https://writable.example.com");
            assert_eq!(shared[PROVIDER_SOURCE_FIELD], PROVIDER_SOURCE_CUSTOM_LIST);
        });
    }

    #[test]
    #[serial]
    fn set_provider_rejects_dict_only_entries() {
        with_test_home(|| {
            let yaml = "\
_config_version: 19
providers:
  anthropic:
    base_url: https://api.anthropic.com
    model: claude-opus-4.6
";
            let config_path = get_hermes_config_path();
            fs::create_dir_all(config_path.parent().unwrap()).unwrap();
            fs::write(&config_path, yaml).unwrap();

            let update = serde_json::json!({ "base_url": "https://hacked.example.com" });
            let err = set_provider("anthropic", update).unwrap_err();
            assert!(
                format!("{err}").contains("providers:"),
                "error message should point user at providers dict: {err}"
            );
        });
    }

    #[test]
    #[serial]
    fn remove_provider_rejects_dict_only_entries() {
        with_test_home(|| {
            let yaml = "\
_config_version: 19
providers:
  anthropic:
    base_url: https://api.anthropic.com
";
            let config_path = get_hermes_config_path();
            fs::create_dir_all(config_path.parent().unwrap()).unwrap();
            fs::write(&config_path, yaml).unwrap();

            assert!(remove_provider("anthropic").is_err());
        });
    }

    #[test]
    fn sanitize_strips_ui_only_markers() {
        let mut v = serde_json::json!({
            "base_url": "https://api.example.com",
            "_cc_source": "providers_dict",
            "provider_key": "anthropic",
        });
        sanitize_hermes_provider_keys(&mut v);
        let obj = v.as_object().unwrap();
        assert!(obj.get("_cc_source").is_none());
        assert!(obj.get("provider_key").is_none());
        assert!(obj.get("base_url").is_some());
    }

    #[test]
    #[serial]
    fn get_providers_heals_legacy_camel_case_on_read() {
        // A DB may still hold records from older DeepLink imports that wrote
        // camelCase fields into `settings_config`. The read path must surface
        // them in Hermes' native snake_case so UI editors aren't lying to users.
        with_test_home(|| {
            let yaml = "\
custom_providers:
  - name: legacy
    baseUrl: https://legacy.example.com
    apiKey: sk-legacy
    apiMode: chat_completions
    api: openai-completions
";
            let config_path = get_hermes_config_path();
            fs::create_dir_all(config_path.parent().unwrap()).unwrap();
            fs::write(&config_path, yaml).unwrap();

            let provider = get_provider("legacy").unwrap().unwrap();
            assert_eq!(provider["base_url"], "https://legacy.example.com");
            assert_eq!(provider["api_key"], "sk-legacy");
            assert_eq!(provider["api_mode"], "chat_completions");
            assert!(provider.get("baseUrl").is_none());
            assert!(provider.get("apiKey").is_none());
            assert!(provider.get("api").is_none());
        });
    }

    // ---- Model config tests ----

    #[test]
    #[serial]
    fn model_config_roundtrip() {
        with_test_home(|| {
            // Initially none
            assert!(get_model_config().unwrap().is_none());

            let model = HermesModelConfig {
                default: Some("anthropic/claude-opus-4-7".to_string()),
                provider: Some("openrouter".to_string()),
                base_url: Some("https://openrouter.ai/api/v1".to_string()),
                context_length: Some(200000),
                max_tokens: None,
                extra: HashMap::new(),
            };
            set_model_config(&model).unwrap();

            let read_model = get_model_config().unwrap().unwrap();
            assert_eq!(
                read_model.default.as_deref(),
                Some("anthropic/claude-opus-4-7")
            );
            assert_eq!(read_model.provider.as_deref(), Some("openrouter"));
            assert_eq!(read_model.context_length, Some(200000));
        });
    }

    // ---- yaml_to_json / json_to_yaml ----

    #[test]
    fn yaml_json_conversion_roundtrip() {
        let json = serde_json::json!({
            "name": "test",
            "count": 42,
            "nested": {
                "flag": true
            }
        });
        let yaml = json_to_yaml(&json).unwrap();
        let back = yaml_to_json(&yaml).unwrap();
        assert_eq!(json, back);
    }

    // ---- models array ↔ dict transforms ----

    #[test]
    fn models_array_to_dict_strips_id_and_preserves_order() {
        let arr = vec![
            serde_json::json!({ "id": "foo", "context_length": 100 }),
            serde_json::json!({ "id": "bar", "max_tokens": 2000 }),
            serde_json::json!({ "id": "baz" }),
        ];
        let dict = models_array_to_dict(arr);
        let obj = dict.as_object().unwrap();
        let keys: Vec<&String> = obj.keys().collect();
        assert_eq!(keys, vec!["foo", "bar", "baz"]);
        assert_eq!(obj["foo"]["context_length"], 100);
        assert_eq!(obj["bar"]["max_tokens"], 2000);
        assert!(obj["baz"].as_object().unwrap().is_empty());
        // id must not leak into values
        assert!(obj["foo"].get("id").is_none());
    }

    #[test]
    fn models_array_to_dict_drops_empty_and_missing_ids() {
        let arr = vec![
            serde_json::json!({ "id": "", "context_length": 1 }),
            serde_json::json!({ "id": "   ", "context_length": 2 }),
            serde_json::json!({ "context_length": 3 }),
            serde_json::json!({ "id": "kept" }),
        ];
        let dict = models_array_to_dict(arr);
        let obj = dict.as_object().unwrap();
        assert_eq!(obj.len(), 1);
        assert!(obj.contains_key("kept"));
    }

    #[test]
    fn models_dict_to_array_reinjects_id_and_preserves_order() {
        let mut map = serde_json::Map::new();
        map.insert(
            "alpha".to_string(),
            serde_json::json!({ "context_length": 10 }),
        );
        map.insert("beta".to_string(), serde_json::json!({ "max_tokens": 20 }));
        map.insert("gamma".to_string(), serde_json::Value::Null);
        let arr = models_dict_to_array(map);
        let list = arr.as_array().unwrap();
        assert_eq!(list.len(), 3);
        assert_eq!(list[0]["id"], "alpha");
        assert_eq!(list[0]["context_length"], 10);
        assert_eq!(list[1]["id"], "beta");
        assert_eq!(list[2]["id"], "gamma");
    }

    #[test]
    #[serial]
    fn provider_with_models_array_writes_dict_to_yaml() {
        with_test_home(|| {
            let config = serde_json::json!({
                "base_url": "https://api.example.com/v1",
                "api_key": "sk-test",
                "api_mode": "chat_completions",
                "models": [
                    { "id": "model-a", "context_length": 200000, "max_tokens": 32000 },
                    { "id": "model-b", "context_length": 100000 },
                ]
            });
            set_provider("demo", config).unwrap();

            // Read raw YAML to verify the on-disk shape is a sequence under `custom_providers:`.
            let raw = fs::read_to_string(get_hermes_config_path()).unwrap();
            let yaml: serde_yaml::Value = serde_yaml::from_str(&raw).unwrap();
            let providers = yaml
                .get("custom_providers")
                .and_then(|v| v.as_sequence())
                .unwrap();
            let provider = &providers[0];
            assert_eq!(
                provider.get("name").and_then(|v| v.as_str()),
                Some("demo"),
                "entry should carry a name field"
            );
            assert_eq!(
                provider.get("model").and_then(|v| v.as_str()),
                Some("model-a"),
                "entry should carry a singular `model:` field set to the first model id \
                 so Hermes runtime/picker reads it"
            );
            let models = provider.get("models").and_then(|v| v.as_mapping()).unwrap();
            assert_eq!(models.len(), 2);
            assert!(models.contains_key(serde_yaml::Value::String("model-a".into())));
            assert!(models.contains_key(serde_yaml::Value::String("model-b".into())));
            let model_a = models
                .get(serde_yaml::Value::String("model-a".into()))
                .unwrap();
            assert_eq!(
                model_a
                    .get("context_length")
                    .and_then(|v| v.as_u64())
                    .unwrap(),
                200000
            );
            // id should not leak into each model value
            assert!(model_a.get("id").is_none());
        });
    }

    #[test]
    #[serial]
    fn provider_models_roundtrip_array_dict_array_preserves_order() {
        with_test_home(|| {
            let input = serde_json::json!({
                "base_url": "https://api.example.com/v1",
                "api_key": "sk-test",
                "models": [
                    { "id": "first", "context_length": 1 },
                    { "id": "second", "context_length": 2 },
                    { "id": "third", "context_length": 3 },
                ]
            });
            set_provider("order", input).unwrap();

            let providers = get_providers().unwrap();
            let provider = providers.get("order").unwrap();
            let models = provider.get("models").and_then(|v| v.as_array()).unwrap();
            let ids: Vec<&str> = models
                .iter()
                .map(|m| m.get("id").and_then(|v| v.as_str()).unwrap())
                .collect();
            assert_eq!(ids, vec!["first", "second", "third"]);
            assert_eq!(models[0].get("context_length").unwrap(), 1);
        });
    }

    #[test]
    #[serial]
    fn provider_without_models_is_unaffected() {
        with_test_home(|| {
            let input = serde_json::json!({
                "base_url": "https://api.example.com/v1",
                "api_key": "sk-test"
            });
            set_provider("simple", input).unwrap();
            let providers = get_providers().unwrap();
            let provider = providers.get("simple").unwrap();
            assert!(provider.get("models").is_none());
            assert!(
                provider.get("model").is_none(),
                "singular `model:` should not appear when no models are declared"
            );
        });
    }

    // ---- apply_switch_defaults ----

    #[test]
    #[serial]
    fn apply_switch_defaults_sets_default_and_provider() {
        with_test_home(|| {
            let settings = serde_json::json!({
                "base_url": "https://api.example.com/v1",
                "models": [
                    { "id": "primary-model", "context_length": 200000 },
                    { "id": "fallback", "context_length": 100000 },
                ]
            });
            apply_switch_defaults("demo", &settings).unwrap();

            let model = get_model_config().unwrap().unwrap();
            assert_eq!(model.default.as_deref(), Some("primary-model"));
            assert_eq!(model.provider.as_deref(), Some("demo"));
        });
    }

    #[test]
    #[serial]
    fn apply_switch_defaults_preserves_user_context_length() {
        with_test_home(|| {
            // User previously set a custom context_length via the Model panel.
            let initial = HermesModelConfig {
                default: Some("old-model".to_string()),
                provider: Some("old-provider".to_string()),
                base_url: Some("https://user-override.example.com".to_string()),
                context_length: Some(131072),
                max_tokens: Some(16384),
                extra: HashMap::new(),
            };
            set_model_config(&initial).unwrap();

            let settings = serde_json::json!({
                "models": [{ "id": "new-model" }]
            });
            apply_switch_defaults("new-provider", &settings).unwrap();

            let model = get_model_config().unwrap().unwrap();
            assert_eq!(model.default.as_deref(), Some("new-model"));
            assert_eq!(model.provider.as_deref(), Some("new-provider"));
            // User-customized fields must survive the switch.
            assert_eq!(
                model.base_url.as_deref(),
                Some("https://user-override.example.com")
            );
            assert_eq!(model.context_length, Some(131072));
            assert_eq!(model.max_tokens, Some(16384));
        });
    }

    #[test]
    #[serial]
    fn apply_switch_defaults_updates_provider_even_without_models() {
        with_test_home(|| {
            // Seed an existing `model:` section — the user was already running
            // some provider before this switch.
            let initial = HermesModelConfig {
                default: Some("legacy-default".to_string()),
                provider: Some("legacy-provider".to_string()),
                ..Default::default()
            };
            set_model_config(&initial).unwrap();

            // New provider has no `models` list — previously this would no-op
            // and leave `model.provider` pointing at the legacy provider,
            // causing "switch succeeds but has no effect" bug.
            let settings = serde_json::json!({
                "base_url": "https://api.example.com/v1"
            });
            apply_switch_defaults("bare", &settings).unwrap();

            let model = get_model_config().unwrap().unwrap();
            assert_eq!(model.provider.as_deref(), Some("bare"));
            assert_eq!(model.default.as_deref(), Some("legacy-default"));
        });
    }

    #[test]
    #[serial]
    fn apply_switch_defaults_keeps_old_default_when_first_model_id_is_blank() {
        with_test_home(|| {
            let initial = HermesModelConfig {
                default: Some("prev-default".to_string()),
                provider: Some("prev-provider".to_string()),
                ..Default::default()
            };
            set_model_config(&initial).unwrap();

            let settings = serde_json::json!({
                "models": [{ "id": "   " }, { "id": "real" }]
            });
            apply_switch_defaults("edge", &settings).unwrap();

            let model = get_model_config().unwrap().unwrap();
            // Provider always updates.
            assert_eq!(model.provider.as_deref(), Some("edge"));
            // First entry's id is whitespace-only → blank → fall back to old default
            // (we intentionally don't scan past the first entry for a default).
            assert_eq!(model.default.as_deref(), Some("prev-default"));
        });
    }

    // ---- memory file tests ----

    #[test]
    #[serial]
    fn read_memory_returns_empty_when_file_missing() {
        with_test_home(|| {
            let memory = read_memory(MemoryKind::Memory).unwrap();
            let user = read_memory(MemoryKind::User).unwrap();
            assert!(memory.is_empty());
            assert!(user.is_empty());
        });
    }

    #[test]
    #[serial]
    fn write_then_read_memory_round_trip() {
        with_test_home(|| {
            let blob = "> note\n§\nfirst entry\n§\nsecond entry\n";
            write_memory(MemoryKind::Memory, blob).unwrap();
            assert_eq!(read_memory(MemoryKind::Memory).unwrap(), blob);

            // Writing USER.md doesn't clobber MEMORY.md.
            write_memory(MemoryKind::User, "user profile").unwrap();
            assert_eq!(read_memory(MemoryKind::Memory).unwrap(), blob);
            assert_eq!(read_memory(MemoryKind::User).unwrap(), "user profile");
        });
    }

    #[test]
    #[serial]
    fn memory_limits_fall_back_to_defaults_when_config_missing() {
        with_test_home(|| {
            let limits = read_memory_limits().unwrap();
            let defaults = HermesMemoryLimits::default();
            assert_eq!(limits.memory, defaults.memory);
            assert_eq!(limits.user, defaults.user);
            assert_eq!(limits.memory_enabled, defaults.memory_enabled);
            assert_eq!(limits.user_enabled, defaults.user_enabled);
        });
    }

    #[test]
    #[serial]
    fn set_memory_enabled_preserves_other_fields() {
        // Flipping one toggle must preserve character budgets and external
        // provider settings the user configured via Hermes Web UI — otherwise
        // a CC Switch toggle would silently wipe those fields.
        with_test_home(|| {
            let yaml = "\
memory:
  memory_char_limit: 4096
  user_char_limit: 2048
  memory_enabled: true
  user_profile_enabled: true
  provider: mem0
";
            let config_path = get_hermes_config_path();
            fs::create_dir_all(config_path.parent().unwrap()).unwrap();
            fs::write(&config_path, yaml).unwrap();

            set_memory_enabled(MemoryKind::Memory, false).unwrap();

            let limits = read_memory_limits().unwrap();
            assert!(!limits.memory_enabled, "toggle applied");
            assert!(limits.user_enabled, "unrelated toggle untouched");
            assert_eq!(limits.memory, 4096, "budgets preserved");
            assert_eq!(limits.user, 2048);

            // Verify the external provider field survived the section replacement.
            let config = read_hermes_config().unwrap();
            let provider = config
                .get("memory")
                .and_then(|v| v.get("provider"))
                .and_then(|v| v.as_str());
            assert_eq!(provider, Some("mem0"));
        });
    }

    #[test]
    #[serial]
    fn memory_limits_read_from_config_yaml() {
        with_test_home(|| {
            let yaml = "\
memory:
  memory_char_limit: 4096
  user_char_limit: 2048
  memory_enabled: false
  user_profile_enabled: true
";
            let config_path = get_hermes_config_path();
            fs::create_dir_all(config_path.parent().unwrap()).unwrap();
            fs::write(&config_path, yaml).unwrap();

            let limits = read_memory_limits().unwrap();
            assert_eq!(limits.memory, 4096);
            assert_eq!(limits.user, 2048);
            assert!(!limits.memory_enabled);
            assert!(limits.user_enabled);
        });
    }

    #[test]
    #[serial]
    fn memory_limits_ignore_top_level_keys() {
        // Regression guard: Hermes nests memory settings under `memory:`, so
        // identically-named keys at the top level must be ignored rather than
        // silently consumed.
        with_test_home(|| {
            let yaml = "\
memory_char_limit: 9999
user_char_limit: 9999
memory_enabled: false
user_profile_enabled: false
";
            let config_path = get_hermes_config_path();
            fs::create_dir_all(config_path.parent().unwrap()).unwrap();
            fs::write(&config_path, yaml).unwrap();

            let limits = read_memory_limits().unwrap();
            let defaults = HermesMemoryLimits::default();
            assert_eq!(limits.memory, defaults.memory);
            assert_eq!(limits.user, defaults.user);
            assert_eq!(limits.memory_enabled, defaults.memory_enabled);
            assert_eq!(limits.user_enabled, defaults.user_enabled);
        });
    }

    #[test]
    fn memory_kind_deserializes_from_lowercase_strings() {
        let memory: MemoryKind = serde_json::from_str("\"memory\"").unwrap();
        let user: MemoryKind = serde_json::from_str("\"user\"").unwrap();
        assert_eq!(memory, MemoryKind::Memory);
        assert_eq!(user, MemoryKind::User);
        assert!(serde_json::from_str::<MemoryKind>("\"bogus\"").is_err());
    }
}
