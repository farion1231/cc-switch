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
use std::collections::{HashMap, HashSet};
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

/// Hermes 健康检查警告
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HermesHealthWarning {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

/// Hermes 写入结果
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct HermesWriteOutcome {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backup_path: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<HermesHealthWarning>,
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

/// Hermes agent section config (agent + approvals)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HermesAgentConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_turns: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_use_enforcement: Option<serde_json::Value>,
    /// Preserve unknown fields for forward compatibility
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

/// Hermes env config (from .env file, not config.yaml)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HermesEnvConfig {
    #[serde(flatten)]
    pub vars: HashMap<String, serde_json::Value>,
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

    let warnings = scan_hermes_health_internal(&new_raw);

    log::debug!(
        "Hermes config section '{section_key}' written to {:?}",
        config_path
    );
    Ok(HermesWriteOutcome {
        backup_path: backup_path.map(|p| p.display().to_string()),
        warnings,
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

/// Get all custom providers as a JSON map keyed by provider name.
///
/// Reads the `custom_providers:` YAML sequence where each item has a `name`
/// field, and converts it to a map for CC Switch consumption. Each entry's
/// `models` field is converted from the YAML dict shape back to the
/// UI-friendly ordered array shape.
///
/// We intentionally use the legacy `custom_providers:` list (not the v12+
/// `providers:` dict) because Hermes's runtime_provider.py
/// `_get_named_custom_provider` has a bug in its `providers:` branch: the
/// returned entry dict drops `api_mode` / `transport` / `models` /
/// singular `model:`. That leaves `_resolve_named_custom_runtime` to fall
/// back to `chat_completions`, breaking every anthropic_messages provider.
/// The legacy `custom_providers:` branch goes through
/// `_normalize_custom_provider_entry` and preserves all fields correctly.
pub fn get_providers() -> Result<serde_json::Map<String, serde_json::Value>, AppError> {
    let config = read_hermes_config()?;
    let providers = config
        .get("custom_providers")
        .and_then(|v| v.as_sequence())
        .map(|seq| {
            let mut map = serde_json::Map::new();
            for item in seq {
                if let Some(name) = item.get("name").and_then(|n| n.as_str()) {
                    match yaml_to_json(item) {
                        Ok(mut json_val) => {
                            denormalize_provider_models_for_read(&mut json_val);
                            map.insert(name.to_string(), json_val);
                        }
                        Err(e) => {
                            log::warn!("Failed to convert Hermes provider '{name}' to JSON: {e}");
                        }
                    }
                }
            }
            map
        })
        .unwrap_or_default();
    Ok(providers)
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
    let mut providers: Vec<serde_yaml::Value> = config
        .get("custom_providers")
        .and_then(|v| v.as_sequence())
        .cloned()
        .unwrap_or_default();

    // Normalize `models` from UI array to Hermes YAML dict before serializing.
    let mut normalized = provider_config;
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
// Agent Config Functions
// ============================================================================

/// Get the `agent` section as a typed config.
pub fn get_agent_config() -> Result<Option<HermesAgentConfig>, AppError> {
    let config = read_hermes_config()?;
    let Some(agent_value) = config.get("agent") else {
        return Ok(None);
    };
    let json_val = yaml_to_json(agent_value)?;
    let agent = serde_json::from_value(json_val)
        .map_err(|e| AppError::Config(format!("Failed to parse Hermes agent config: {e}")))?;
    Ok(Some(agent))
}

/// Set the `agent` section.
pub fn set_agent_config(agent: &HermesAgentConfig) -> Result<HermesWriteOutcome, AppError> {
    let json_val =
        serde_json::to_value(agent).map_err(|e| AppError::JsonSerialize { source: e })?;
    let yaml_val = json_to_yaml(&json_val)?;
    write_yaml_section_to_config("agent", &yaml_val)
}

// ============================================================================
// .env Functions
// ============================================================================

/// Read the Hermes `.env` file (`~/.hermes/.env`).
///
/// Parses dotenv format (KEY=VALUE, `#` comments, blank lines).
pub fn read_env() -> Result<HermesEnvConfig, AppError> {
    let path = get_hermes_dir().join(".env");
    if !path.exists() {
        return Ok(HermesEnvConfig {
            vars: HashMap::new(),
        });
    }
    let content = fs::read_to_string(&path).map_err(|e| AppError::io(&path, e))?;
    let mut vars = HashMap::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = trimmed.split_once('=') {
            let key = key.trim().to_string();
            let value = value
                .trim()
                .trim_matches('"')
                .trim_matches('\'')
                .to_string();
            vars.insert(key, serde_json::Value::String(value));
        }
    }
    Ok(HermesEnvConfig { vars })
}

/// Write the Hermes `.env` file (`~/.hermes/.env`).
///
/// Preserves comment lines and ordering. Keys not present in the new env are
/// removed; new keys are appended.
pub fn write_env(env: &HermesEnvConfig) -> Result<HermesWriteOutcome, AppError> {
    let path = get_hermes_dir().join(".env");
    let _guard = hermes_write_lock().lock()?;

    // Read existing file to preserve comments and ordering
    let existing_content = if path.exists() {
        fs::read_to_string(&path).map_err(|e| AppError::io(&path, e))?
    } else {
        String::new()
    };

    // Build new content: preserve comment lines, update/add key-value pairs
    let mut remaining_keys: HashSet<String> = env.vars.keys().cloned().collect();
    let mut lines: Vec<String> = Vec::new();

    for line in existing_content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            lines.push(line.to_string());
            continue;
        }
        if let Some((key, _)) = trimmed.split_once('=') {
            let key = key.trim();
            if let Some(new_value) = env.vars.get(key) {
                // Update existing key
                let value_str = json_value_as_env_str(new_value);
                lines.push(format!("{key}={value_str}"));
                remaining_keys.remove(key);
            }
            // If key is not in new env, it's deleted (don't add it)
        } else {
            lines.push(line.to_string());
        }
    }

    // Add new keys that weren't in the original file (sorted for determinism)
    let mut new_keys: Vec<String> = remaining_keys.into_iter().collect();
    new_keys.sort();
    for key in new_keys {
        if let Some(value) = env.vars.get(&key) {
            let value_str = json_value_as_env_str(value);
            lines.push(format!("{key}={value_str}"));
        }
    }

    let mut new_content = lines.join("\n");
    if !new_content.is_empty() && !new_content.ends_with('\n') {
        new_content.push('\n');
    }

    if new_content == existing_content {
        return Ok(HermesWriteOutcome::default());
    }

    // Backup if file existed with content
    let backup_path = if !existing_content.is_empty() {
        Some(create_hermes_backup(&existing_content)?)
    } else {
        None
    };

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
    }
    atomic_write(&path, new_content.as_bytes())?;

    Ok(HermesWriteOutcome {
        backup_path: backup_path.map(|p| p.display().to_string()),
        warnings: Vec::new(),
    })
}

/// Convert a serde_json::Value to a string suitable for .env files.
fn json_value_as_env_str(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

// ============================================================================
// Health Check
// ============================================================================

/// Scan Hermes config for known configuration hazards.
///
/// Parse failures are reported as warnings (not errors) so the UI can
/// display them without blocking.
pub fn scan_hermes_config_health() -> Result<Vec<HermesHealthWarning>, AppError> {
    let path = get_hermes_config_path();
    if !path.exists() {
        return Ok(Vec::new());
    }
    let content = fs::read_to_string(&path).map_err(|e| AppError::io(&path, e))?;
    Ok(scan_hermes_health_internal(&content))
}

fn scan_hermes_health_internal(content: &str) -> Vec<HermesHealthWarning> {
    let mut warnings = Vec::new();

    if content.trim().is_empty() {
        return warnings;
    }

    match serde_yaml::from_str::<serde_yaml::Value>(content) {
        Ok(config) => {
            // Check for model section issues
            if let Some(model) = config.get("model") {
                if model.get("default").is_none() && model.get("provider").is_none() {
                    warnings.push(HermesHealthWarning {
                        code: "model_no_default".to_string(),
                        message: "No default model or provider configured in 'model' section"
                            .to_string(),
                        path: Some("model".to_string()),
                    });
                }
            }

            // Check custom_providers is a sequence
            if let Some(providers) = config.get("custom_providers") {
                if !providers.is_sequence() {
                    warnings.push(HermesHealthWarning {
                        code: "custom_providers_not_list".to_string(),
                        message: "custom_providers should be a YAML list (sequence), not a mapping"
                            .to_string(),
                        path: Some("custom_providers".to_string()),
                    });
                }
            }
        }
        Err(err) => {
            warnings.push(HermesHealthWarning {
                code: "config_parse_failed".to_string(),
                message: format!("Hermes config could not be parsed as YAML: {err}"),
                path: Some(get_hermes_config_path().display().to_string()),
            });
        }
    }

    warnings
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

    // ---- .env read/write tests ----

    #[test]
    #[serial]
    fn env_read_write_roundtrip() {
        with_test_home(|| {
            // Write initial env
            let env = HermesEnvConfig {
                vars: {
                    let mut m = HashMap::new();
                    m.insert(
                        "API_KEY".to_string(),
                        serde_json::Value::String("sk-test-123".to_string()),
                    );
                    m.insert(
                        "DEBUG".to_string(),
                        serde_json::Value::String("true".to_string()),
                    );
                    m
                },
            };
            write_env(&env).unwrap();

            // Read back
            let env_read = read_env().unwrap();
            assert_eq!(
                env_read.vars.get("API_KEY").unwrap().as_str().unwrap(),
                "sk-test-123"
            );
            assert_eq!(
                env_read.vars.get("DEBUG").unwrap().as_str().unwrap(),
                "true"
            );

            // Update: remove DEBUG, add NEW_VAR
            let env2 = HermesEnvConfig {
                vars: {
                    let mut m = HashMap::new();
                    m.insert(
                        "API_KEY".to_string(),
                        serde_json::Value::String("sk-test-456".to_string()),
                    );
                    m.insert(
                        "NEW_VAR".to_string(),
                        serde_json::Value::String("hello".to_string()),
                    );
                    m
                },
            };
            write_env(&env2).unwrap();

            let env_read2 = read_env().unwrap();
            assert_eq!(
                env_read2.vars.get("API_KEY").unwrap().as_str().unwrap(),
                "sk-test-456"
            );
            assert!(env_read2.vars.get("DEBUG").is_none());
            assert_eq!(
                env_read2.vars.get("NEW_VAR").unwrap().as_str().unwrap(),
                "hello"
            );
        });
    }

    #[test]
    #[serial]
    fn env_preserves_comments() {
        with_test_home(|| {
            let hermes_dir = get_hermes_dir();
            fs::create_dir_all(&hermes_dir).unwrap();
            let env_path = hermes_dir.join(".env");
            fs::write(
                &env_path,
                "# Hermes environment config\nAPI_KEY=old-key\n# Keep this comment\nDEBUG=true\n",
            )
            .unwrap();

            let env = HermesEnvConfig {
                vars: {
                    let mut m = HashMap::new();
                    m.insert(
                        "API_KEY".to_string(),
                        serde_json::Value::String("new-key".to_string()),
                    );
                    m.insert(
                        "DEBUG".to_string(),
                        serde_json::Value::String("false".to_string()),
                    );
                    m
                },
            };
            write_env(&env).unwrap();

            let content = fs::read_to_string(&env_path).unwrap();
            assert!(content.contains("# Hermes environment config"));
            assert!(content.contains("# Keep this comment"));
            assert!(content.contains("API_KEY=new-key"));
            assert!(content.contains("DEBUG=false"));
        });
    }

    // ---- Model/Agent config tests ----

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

    #[test]
    #[serial]
    fn agent_config_roundtrip() {
        with_test_home(|| {
            assert!(get_agent_config().unwrap().is_none());

            let agent = HermesAgentConfig {
                max_turns: Some(50),
                reasoning_effort: Some("high".to_string()),
                tool_use_enforcement: None,
                extra: HashMap::new(),
            };
            set_agent_config(&agent).unwrap();

            let read_agent = get_agent_config().unwrap().unwrap();
            assert_eq!(read_agent.max_turns, Some(50));
            assert_eq!(read_agent.reasoning_effort.as_deref(), Some("high"));
        });
    }

    // ---- Health check tests ----

    #[test]
    fn health_check_on_invalid_yaml() {
        let warnings = scan_hermes_health_internal("not: valid: yaml: [");
        assert!(!warnings.is_empty());
        assert_eq!(warnings[0].code, "config_parse_failed");
    }

    #[test]
    fn health_check_model_no_default() {
        let yaml = "model:\n  context_length: 200000\n";
        let warnings = scan_hermes_health_internal(yaml);
        assert!(warnings.iter().any(|w| w.code == "model_no_default"));
    }

    #[test]
    fn health_check_custom_providers_not_list() {
        let yaml = "custom_providers:\n  foo:\n    base_url: http://localhost\n";
        let warnings = scan_hermes_health_internal(yaml);
        assert!(warnings
            .iter()
            .any(|w| w.code == "custom_providers_not_list"));
    }

    #[test]
    fn health_check_valid_config() {
        let yaml = "\
model:
  default: gpt-4
  provider: openrouter
custom_providers:
  - name: openrouter
    base_url: https://openrouter.ai/api/v1
";
        let warnings = scan_hermes_health_internal(yaml);
        assert!(warnings.is_empty());
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
}
