//! Pi Coding Agent / Oh-my-pi 配置文件读写模块
//!
//! 处理供应商配置的读写（累加式，所有供应商共存于同一文件）。
//!
//! ## 配置根目录解析顺序
//!
//! 1. CCS 设置 `piConfigDir`（例如 `"~/.omp"` 兼容 Oh-my-pi）
//! 2. `PI_HOME` 环境变量
//! 3. `PI_CODING_AGENT_DIR` / `PI_CONFIG_DIR`（Oh-my-pi）
//! 4. 平台默认 `~/.pi`
//!
//! ## 配置文件
//!
//! - 模型/供应商：优先已存在的 `models.yml` → `models.yaml` → `models.json`
//!   （Oh-my-pi 默认 `~/.omp/agent/models.yml`；原版 Pi 默认 `models.json`）
//! - 激活供应商：
//!   - Oh-my-pi：`config.yml` 的 `modelRoles.default`（`provider/model`）
//!   - 原版 Pi：`settings.json` 的 `defaultProvider`
//!
//! ## 配置结构示例（JSON / YAML 同构）
//!
//! ```json
//! {
//!   "providers": {
//!     "anthropic": {
//!       "baseUrl": "https://api.anthropic.com",
//!       "api": "anthropic-messages",
//!       "apiKey": "$ANTHROPIC_API_KEY",
//!       "models": [
//!         { "id": "claude-opus-4-8", "name": "Claude Opus 4.8" }
//!       ]
//!     }
//!   }
//! }
//! ```

use crate::config::{atomic_write, get_app_config_dir};
use crate::error::AppError;
use crate::settings::get_pi_override_dir;
use chrono::Local;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

// ============================================================================
// serde_json compatibility helpers
// ============================================================================

/// Get or insert a key in a JSON value's object map, initializing with an empty object.
fn json_value_entry<'a>(value: &'a mut Value, key: &str) -> &'a mut Value {
    if !value.is_object() {
        *value = Value::Object(Map::new());
    }
    value.as_object_mut()
        .expect("just ensured object")
        .entry(key.to_string())
        .or_insert_with(|| Value::Object(Map::new()))
}

fn expand_user_path(raw: &str) -> PathBuf {
    let trimmed = raw.trim();
    if trimmed == "~" {
        if let Some(home) = dirs::home_dir() {
            return home;
        }
    } else if let Some(stripped) = trimmed.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(stripped);
        }
    } else if let Some(stripped) = trimmed.strip_prefix("~\\") {
        if let Some(home) = dirs::home_dir() {
            return home.join(stripped);
        }
    }
    PathBuf::from(trimmed)
}

fn env_dir(name: &str) -> Option<PathBuf> {
    let raw = std::env::var_os(name)?;
    let value = raw.to_string_lossy();
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(expand_user_path(trimmed))
}

fn is_yaml_path(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            let lower = ext.to_ascii_lowercase();
            lower == "yml" || lower == "yaml"
        })
}

fn path_backup_ext(path: &Path) -> &'static str {
    if is_yaml_path(path) {
        "yml"
    } else {
        "json"
    }
}

fn parse_config_text(path: &Path, content: &str) -> Result<Value, AppError> {
    if content.trim().is_empty() {
        return Ok(default_pi_config_value());
    }

    if is_yaml_path(path) {
        let yaml: serde_yaml::Value = serde_yaml::from_str(content)
            .map_err(|e| AppError::Config(format!("Failed to parse Pi config as YAML: {e}")))?;
        serde_json::to_value(yaml)
            .map_err(|e| AppError::Config(format!("Failed to convert Pi YAML config to JSON: {e}")))
    } else {
        serde_json::from_str(content)
            .map_err(|e| AppError::Config(format!("Failed to parse Pi config as JSON: {e}")))
    }
}

fn serialize_config_text(path: &Path, value: &Value) -> Result<String, AppError> {
    if is_yaml_path(path) {
        let yaml = serde_json::from_value::<serde_yaml::Value>(value.clone()).map_err(|e| {
            AppError::Config(format!("Failed to convert Pi config to YAML value: {e}"))
        })?;
        let serialized = serde_yaml::to_string(&yaml)
            .map_err(|e| AppError::Config(format!("Failed to serialize Pi config as YAML: {e}")))?;
        Ok(serialized)
    } else {
        let serialized = serde_json::to_string_pretty(value)
            .map_err(|e| AppError::JsonSerialize { source: e })?;
        Ok(format!("{serialized}\n"))
    }
}

fn parse_settings_text(path: &Path, content: &str) -> Result<Value, AppError> {
    if content.trim().is_empty() {
        return Ok(default_pi_settings_value());
    }

    if is_yaml_path(path) {
        let yaml: serde_yaml::Value = serde_yaml::from_str(content)
            .map_err(|e| AppError::Config(format!("Failed to parse Pi settings as YAML: {e}")))?;
        serde_json::to_value(yaml).map_err(|e| {
            AppError::Config(format!("Failed to convert Pi YAML settings to JSON: {e}"))
        })
    } else {
        serde_json::from_str(content)
            .map_err(|e| AppError::Config(format!("Failed to parse Pi settings as JSON: {e}")))
    }
}

fn serialize_settings_text(path: &Path, value: &Value) -> Result<String, AppError> {
    if is_yaml_path(path) {
        let yaml = serde_json::from_value::<serde_yaml::Value>(value.clone()).map_err(|e| {
            AppError::Config(format!("Failed to convert Pi settings to YAML value: {e}"))
        })?;
        let serialized = serde_yaml::to_string(&yaml).map_err(|e| {
            AppError::Config(format!("Failed to serialize Pi settings as YAML: {e}"))
        })?;
        Ok(serialized)
    } else {
        let serialized = serde_json::to_string_pretty(value)
            .map_err(|e| AppError::JsonSerialize { source: e })?;
        Ok(format!("{serialized}\n"))
    }
}

// ============================================================================
// Path Functions
// ============================================================================

/// 获取 Pi / Oh-my-pi 配置根目录
///
/// 解析顺序:
///   1. CCS 设置 `piConfigDir`（显式覆盖，支持 `~/.omp`）
///   2. `PI_HOME` 环境变量
///   3. `PI_CODING_AGENT_DIR` / `PI_CONFIG_DIR`（Oh-my-pi）
///   4. 平台默认 `~/.pi`
pub fn get_pi_dir() -> PathBuf {
    if let Some(override_dir) = get_pi_override_dir() {
        return override_dir;
    }

    if let Some(dir) = env_dir("PI_HOME") {
        return dir;
    }

    if let Some(dir) = env_dir("PI_CODING_AGENT_DIR") {
        return dir;
    }

    if let Some(dir) = env_dir("PI_CONFIG_DIR") {
        return dir;
    }

    crate::config::get_home_dir().join(".pi")
}

fn get_pi_agent_dir() -> PathBuf {
    get_pi_dir().join("agent")
}

/// Whether this Pi root should use Oh-my-pi file names (`models.yml` / `config.yml`).
///
/// True when the directory is named `.omp`/`omp`, or YAML layout files already exist.
fn prefers_omp_layout(pi_dir: &Path) -> bool {
    let dir_name = pi_dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");
    if dir_name.eq_ignore_ascii_case(".omp") || dir_name.eq_ignore_ascii_case("omp") {
        return true;
    }

    let agent = pi_dir.join("agent");
    [
        "config.yml",
        "config.yaml",
        "models.yml",
        "models.yaml",
    ]
    .iter()
    .any(|name| agent.join(name).exists())
}

/// 解析实际使用的 models 配置文件路径。
///
/// 优先已存在文件：`models.yml` → `models.yaml` → `models.json`。
/// 若都不存在：Oh-my-pi 布局（`~/.omp` 或已有 YAML）默认 `models.yml`，否则 `models.json`。
pub fn get_pi_config_path() -> PathBuf {
    let agent = get_pi_agent_dir();
    for name in ["models.yml", "models.yaml", "models.json"] {
        let path = agent.join(name);
        if path.exists() {
            return path;
        }
    }

    if prefers_omp_layout(&get_pi_dir()) {
        agent.join("models.yml")
    } else {
        agent.join("models.json")
    }
}

/// 解析 settings / config 文件路径。
///
/// 优先已存在：`config.yml` → `config.yaml` → `settings.json`。
/// 若都不存在：Oh-my-pi 布局默认 `config.yml`，否则 `settings.json`。
pub fn get_pi_settings_path() -> PathBuf {
    let agent = get_pi_agent_dir();
    for name in ["config.yml", "config.yaml", "settings.json"] {
        let path = agent.join(name);
        if path.exists() {
            return path;
        }
    }

    if prefers_omp_layout(&get_pi_dir()) {
        agent.join("config.yml")
    } else {
        agent.join("settings.json")
    }
}

fn uses_omp_settings_layout(path: &Path) -> bool {
    is_yaml_path(path)
        && path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| {
                let lower = n.to_ascii_lowercase();
                lower == "config.yml" || lower == "config.yaml"
            })
}

/// Extract provider id from an Oh-my-pi role value like `anthropic/claude-sonnet-4-5`
/// or `anthropic/claude-opus-4-6:high`. `@smol`-style aliases are ignored.
fn parse_omp_role_provider(role: &str) -> Option<String> {
    let trimmed = role.trim();
    if trimmed.is_empty() || trimmed.starts_with('@') {
        return None;
    }
    let provider = trimmed.split_once('/').map(|(p, _)| p).unwrap_or(trimmed);
    let provider = provider.trim();
    if provider.is_empty() {
        None
    } else {
        Some(provider.to_string())
    }
}

/// Build `provider/model[:thinking]` for Oh-my-pi `modelRoles.default`.
fn build_omp_default_role(provider: &str, existing_role: Option<&str>) -> Result<String, AppError> {
    // Same provider: keep model (+ thinking) from the existing role.
    if let Some(existing) = existing_role {
        if let Some((existing_provider, rest)) = existing.split_once('/') {
            if existing_provider.trim() == provider {
                let rest = rest.trim();
                if !rest.is_empty() {
                    return Ok(format!("{provider}/{rest}"));
                }
            }
        }
    }

    // Otherwise pin the provider's first configured model id.
    if let Some(config) = get_provider(provider)? {
        if let Some(model) = config
            .models
            .iter()
            .map(|m| m.id.trim())
            .find(|id| !id.is_empty())
        {
            return Ok(format!("{provider}/{model}"));
        }
    }

    // Last resort: provider id only (omp may not accept this, but keeps a signal).
    Ok(provider.to_string())
}

fn pi_write_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

// ============================================================================
// Type Definitions
// ============================================================================

/// Pi 写入结果
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PiWriteOutcome {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backup_path: Option<String>,
}

/// Pi 供应商配置（对应 providers 中的条目）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PiProviderConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub models: Vec<PiModelEntry>,
    /// Preserve unknown fields for forward compatibility (e.g. `headers`,
    /// `authHeader`, `oauth`, `modelOverrides`, `compat`).
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

impl Default for PiProviderConfig {
    fn default() -> Self {
        Self {
            base_url: None,
            api_key: None,
            api: None,
            models: Vec::new(),
            extra: HashMap::new(),
        }
    }
}

/// Pi model 配置条目
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PiModelEntry {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub reasoning: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub input: Vec<String>,
    /// Preserve unknown fields for forward compatibility.
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

fn is_false(b: &bool) -> bool {
    !*b
}

// ============================================================================
// Backup & Cleanup
// ============================================================================

fn create_pi_backup(source: &str, kind: &str, ext: &str) -> Result<PathBuf, AppError> {
    use crate::settings::effective_backup_retain_count;

    let backup_dir = get_app_config_dir().join("backups").join("pi");
    fs::create_dir_all(&backup_dir).map_err(|e| AppError::io(&backup_dir, e))?;

    let safe_ext = if ext.trim().is_empty() { "json" } else { ext };
    let base_id = format!("pi_{kind}_{}", Local::now().format("%Y%m%d_%H%M%S"));
    let mut filename = format!("{base_id}.{safe_ext}");
    let mut backup_path = backup_dir.join(&filename);
    let mut counter = 1;

    while backup_path.exists() {
        filename = format!("{base_id}_{counter}.{safe_ext}");
        backup_path = backup_dir.join(&filename);
        counter += 1;
    }

    atomic_write(&backup_path, source.as_bytes())?;
    cleanup_pi_backups(&backup_dir, kind, effective_backup_retain_count())?;
    Ok(backup_path)
}

fn cleanup_pi_backups(dir: &Path, kind: &str, retain: usize) -> Result<(), AppError> {
    let prefix = format!("pi_{kind}_");
    let mut entries = fs::read_dir(dir)
        .map_err(|e| AppError::io(dir, e))?
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            let path = entry.path();
            let ext_ok = path
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| {
                    let lower = ext.to_ascii_lowercase();
                    lower == "json" || lower == "yml" || lower == "yaml"
                });
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("");
            ext_ok && name.starts_with(&prefix)
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
                "Failed to remove old Pi config backup {}: {err}",
                entry.path().display()
            );
        }
    }

    Ok(())
}

// ============================================================================
// Core Read/Write
// ============================================================================

/// 读取整个 models 配置为 `serde_json::Value`（YAML/JSON 统一成 JSON 树）。
///
/// 如果文件不存在，返回包含空 `providers` 映射的默认结构。
pub fn read_pi_config() -> Result<Value, AppError> {
    let path = get_pi_config_path();
    if !path.exists() {
        return Ok(default_pi_config_value());
    }

    let content = fs::read_to_string(&path).map_err(|e| AppError::io(&path, e))?;
    let mut value = parse_config_text(&path, &content)?;
    normalize_providers_field(&mut value);
    Ok(value)
}

fn default_pi_config_value() -> Value {
    serde_json::json!({ "providers": {} })
}

fn default_pi_settings_value() -> Value {
    serde_json::json!({})
}

/// 读取 settings/config 为 `serde_json::Value`。
pub fn read_pi_settings() -> Result<Value, AppError> {
    let path = get_pi_settings_path();
    if !path.exists() {
        return Ok(default_pi_settings_value());
    }

    let content = fs::read_to_string(&path).map_err(|e| AppError::io(&path, e))?;
    parse_settings_text(&path, &content)
}

/// 内部写入:把完整的 settings/config 落盘(调用方需持锁)。
fn write_pi_settings_locked(value: &Value) -> Result<PiWriteOutcome, AppError> {
    let path = get_pi_settings_path();
    let raw = if path.exists() {
        fs::read_to_string(&path).map_err(|e| AppError::io(&path, e))?
    } else {
        String::new()
    };

    let serialized = serialize_settings_text(&path, value)?;
    if serialized == raw {
        return Ok(PiWriteOutcome::default());
    }

    let backup_path = if !raw.is_empty() {
        Some(create_pi_backup(&raw, "settings", path_backup_ext(&path))?)
    } else {
        None
    };

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
    }

    atomic_write(&path, serialized.as_bytes())?;

    log::debug!("Pi settings written to {:?}", path);
    Ok(PiWriteOutcome {
        backup_path: backup_path.map(|p| p.display().to_string()),
    })
}

/// 内部写入:把完整的 models 配置落盘(调用方需持锁)。
fn write_pi_config_locked(value: &Value) -> Result<PiWriteOutcome, AppError> {
    let path = get_pi_config_path();
    let raw = if path.exists() {
        fs::read_to_string(&path).map_err(|e| AppError::io(&path, e))?
    } else {
        String::new()
    };

    // 重新归一化一遍,避免写入意料外的形状(防御性)。
    let mut to_write = value.clone();
    normalize_providers_field(&mut to_write);

    let serialized = serialize_config_text(&path, &to_write)?;
    if serialized == raw {
        return Ok(PiWriteOutcome::default());
    }

    let backup_path = if !raw.is_empty() {
        Some(create_pi_backup(&raw, "models", path_backup_ext(&path))?)
    } else {
        None
    };

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
    }

    atomic_write(&path, serialized.as_bytes())?;

    log::debug!("Pi config written to {:?}", path);
    Ok(PiWriteOutcome {
        backup_path: backup_path.map(|p| p.display().to_string()),
    })
}

/// Ensure the top-level `providers` field is a JSON object. Insert default
/// if missing; convert arrays (legacy) into a dict by taking any non-empty
/// `name` / `id` field as the key. Non-object types (e.g. strings) are
/// replaced with an empty dict.
fn normalize_providers_field(value: &mut Value) {
    let obj = match value.as_object_mut() {
        Some(obj) => obj,
        None => {
            *value = default_pi_config_value();
            return;
        }
    };

    // Normalize "providers": ensure it's a dict (not array, not missing).
    match obj.get_mut("providers") {
        None => {
            obj.insert("providers".to_string(), Value::Object(Map::new()));
        }
        Some(Value::Object(_)) => {
            // Already an object — nothing to do.
        }
        Some(Value::Array(arr)) => {
            let mut map = Map::new();
            for entry in arr.drain(..) {
                if let Some(key) = entry
                    .as_object()
                    .and_then(|o| o.get("name").or_else(|| o.get("id")))
                    .and_then(|v| v.as_str())
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                {
                    map.insert(key.to_string(), entry);
                }
            }
            *obj.get_mut("providers").unwrap() = Value::Object(map);
        }
        Some(_) => {
            *obj.get_mut("providers").unwrap() = Value::Object(Map::new());
        }
    }
}

// ============================================================================
// Provider Functions
// ============================================================================

/// 获取所有供应商配置,返回 name -> provider config 的 map。
///
/// 使用 `IndexMap` 保证声明顺序(与磁盘文件一致)。
pub fn get_providers() -> Result<IndexMap<String, PiProviderConfig>, AppError> {
    let config = read_pi_config()?;
    let mut out: IndexMap<String, PiProviderConfig> = IndexMap::new();

    if let Some(map) = config.get("providers").and_then(|v| v.as_object()) {
        for (name, raw) in map {
            // 容错:任何非对象条目都跳过,而不是让整次读取失败。
            match serde_json::from_value::<PiProviderConfig>(raw.clone()) {
                Ok(provider) => {
                    out.insert(name.clone(), provider);
                }
                Err(e) => {
                    log::warn!("Failed to parse Pi provider '{name}': {e}");
                }
            }
        }
    }

    Ok(out)
}

/// 获取单个供应商配置。
pub fn get_provider(name: &str) -> Result<Option<PiProviderConfig>, AppError> {
    Ok(get_providers()?.shift_remove(name))
}

/// Upsert 一个供应商配置。
pub fn set_provider(
    name: &str,
    provider_config: PiProviderConfig,
) -> Result<PiWriteOutcome, AppError> {
    let _guard = pi_write_lock().lock()?;
    let mut config = read_pi_config()?;
    let providers_value = json_value_entry(&mut config, "providers");

    if !providers_value.is_object() {
        *providers_value = Value::Object(Map::new());
    }

    let providers_map = providers_value
        .as_object_mut()
        .expect("just ensured object");

    let serialized = serde_json::to_value(&provider_config)
        .map_err(|e| AppError::JsonSerialize { source: e })?;

    // Forward-compat merge: keep unknown disk fields (headers/oauth/…), but
    // replace known schema fields from the typed config. Optional fields that
    // are None must be removed so callers can clear apiKey/baseUrl/api.
    const KNOWN_FIELDS: &[&str] = &["baseUrl", "apiKey", "api", "models"];

    if let Some(existing) = providers_map.get(name) {
        if let (Some(existing_obj), Some(new_obj)) =
            (existing.as_object(), serialized.as_object())
        {
            let mut merged = existing_obj.clone();
            for key in KNOWN_FIELDS {
                merged.remove(*key);
            }
            for (k, v) in new_obj {
                merged.insert(k.clone(), v.clone());
            }
            providers_map.insert(name.to_string(), Value::Object(merged));
        } else {
            providers_map.insert(name.to_string(), serialized);
        }
    } else {
        providers_map.insert(name.to_string(), serialized);
    }

    write_pi_config_locked(&config)
}

/// Upsert 一个供应商配置（保留原始 JSON，用于结构不完全匹配时的兜底写入）。
pub fn set_provider_raw(name: &str, config: Value) -> Result<PiWriteOutcome, AppError> {
    let _guard = pi_write_lock().lock()?;
    let mut root = read_pi_config()?;
    let providers_value = json_value_entry(&mut root, "providers");

    if !providers_value.is_object() {
        *providers_value = Value::Object(Map::new());
    }

    let providers_map = providers_value
        .as_object_mut()
        .expect("just ensured object");

    if let Some(existing) = providers_map.get(name) {
        if let (Some(existing_obj), Some(new_obj)) = (existing.as_object(), config.as_object()) {
            let mut merged = existing_obj.clone();
            for (k, v) in new_obj {
                merged.insert(k.clone(), v.clone());
            }
            providers_map.insert(name.to_string(), Value::Object(merged));
        } else {
            providers_map.insert(name.to_string(), config);
        }
    } else {
        providers_map.insert(name.to_string(), config);
    }

    write_pi_config_locked(&root)
}

/// 删除一个供应商配置。
pub fn remove_provider(name: &str) -> Result<PiWriteOutcome, AppError> {
    let _guard = pi_write_lock().lock()?;
    let mut config = read_pi_config()?;
    let removed = if let Some(providers) = config
        .get_mut("providers")
        .and_then(|v| v.as_object_mut())
    {
        providers.remove(name).is_some()
    } else {
        false
    };

    if !removed {
        return Ok(PiWriteOutcome::default());
    }

    write_pi_config_locked(&config)
}

/// 持久化当前激活供应商。
///
/// - 原版 Pi：写入 `settings.json` 的 `defaultProvider`
/// - Oh-my-pi：写入 `config.yml` 的 `modelRoles.default`（`provider/model`）
pub fn set_active_provider(name: &str) -> Result<PiWriteOutcome, AppError> {
    let _guard = pi_write_lock().lock()?;
    let settings_path = get_pi_settings_path();
    let mut settings = read_pi_settings()?;
    let use_omp = uses_omp_settings_layout(&settings_path) || prefers_omp_layout(&get_pi_dir());

    if use_omp {
        let existing_role = settings
            .pointer("/modelRoles/default")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        let role_value = build_omp_default_role(name, existing_role.as_deref())?;

        if settings.get("modelRoles").and_then(|v| v.as_object()).is_none() {
            settings["modelRoles"] = serde_json::json!({});
        }
        settings["modelRoles"]["default"] = Value::String(role_value);
        if let Some(obj) = settings.as_object_mut() {
            obj.remove("defaultProvider");
        }
    } else {
        settings["defaultProvider"] = Value::String(name.to_string());
    }

    write_pi_settings_locked(&settings)
}

/// 读取当前激活的供应商。
///
/// - Oh-my-pi：从 `modelRoles.default` 解析 `provider/...`
/// - 原版 Pi：读取 `defaultProvider`
pub fn get_active_provider() -> Result<Option<String>, AppError> {
    let settings_path = get_pi_settings_path();
    let settings = read_pi_settings()?;
    let use_omp = uses_omp_settings_layout(&settings_path) || prefers_omp_layout(&get_pi_dir());

    if use_omp {
        if let Some(provider) = settings
            .pointer("/modelRoles/default")
            .and_then(|v| v.as_str())
            .and_then(parse_omp_role_provider)
        {
            return Ok(Some(provider));
        }
    }

    Ok(settings
        .get("defaultProvider")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty()))
}

// ============================================================================
// 健康检查
// ============================================================================

/// Pi 配置文件健康警告
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PiHealthWarning {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

/// 扫描 `models.json` 中的已知配置隐患,例如缺失的 `baseUrl` / `api` 字段等。
///
/// Note: JSON object keys are unique after serde_json parse, so duplicate
/// provider names cannot be detected here.
pub fn scan_pi_config_health() -> Result<Vec<PiHealthWarning>, AppError> {
    let config = read_pi_config()?;
    let mut warnings = Vec::new();

    let Some(providers) = config.get("providers").and_then(|v| v.as_object()) else {
        return Ok(warnings);
    };

    for (name, value) in providers {
        if let Some(obj) = value.as_object() {
            if !obj.contains_key("baseUrl") {
                warnings.push(PiHealthWarning {
                    code: "missing_base_url".to_string(),
                    message: format!("Provider '{name}' is missing 'baseUrl'"),
                    provider: Some(name.clone()),
                    path: Some(get_pi_config_path().display().to_string()),
                });
            }
            if !obj.contains_key("api") {
                warnings.push(PiHealthWarning {
                    code: "missing_api".to_string(),
                    message: format!(
                        "Provider '{name}' is missing 'api' (openai-completions / openai-responses / anthropic-messages / google-generative-ai)"
                    ),
                    provider: Some(name.clone()),
                    path: Some(get_pi_config_path().display().to_string()),
                });
            }
            let models_empty = obj
                .get("models")
                .and_then(|v| v.as_array())
                .map(|arr| arr.is_empty())
                .unwrap_or(true);
            if models_empty {
                warnings.push(PiHealthWarning {
                    code: "missing_models".to_string(),
                    message: format!("Provider '{name}' has no models configured"),
                    provider: Some(name.clone()),
                    path: Some(get_pi_config_path().display().to_string()),
                });
            }
        }
    }

    Ok(warnings)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    fn test_guard() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|err| err.into_inner())
    }

    fn with_test_home<T>(test_fn: impl FnOnce() -> T) -> T {
        let _guard = test_guard();
        let tmp = tempfile::tempdir().unwrap();
        let old_test_home = std::env::var_os("CC_SWITCH_TEST_HOME");
        std::env::set_var("CC_SWITCH_TEST_HOME", tmp.path());
        let old_pi_home = std::env::var_os("PI_HOME");
        let old_pi_coding = std::env::var_os("PI_CODING_AGENT_DIR");
        let old_pi_config = std::env::var_os("PI_CONFIG_DIR");
        std::env::remove_var("PI_HOME");
        std::env::remove_var("PI_CODING_AGENT_DIR");
        std::env::remove_var("PI_CONFIG_DIR");
        let result = test_fn();
        match old_pi_home {
            Some(value) => std::env::set_var("PI_HOME", value),
            None => std::env::remove_var("PI_HOME"),
        }
        match old_pi_coding {
            Some(value) => std::env::set_var("PI_CODING_AGENT_DIR", value),
            None => std::env::remove_var("PI_CODING_AGENT_DIR"),
        }
        match old_pi_config {
            Some(value) => std::env::set_var("PI_CONFIG_DIR", value),
            None => std::env::remove_var("PI_CONFIG_DIR"),
        }
        match old_test_home {
            Some(value) => std::env::set_var("CC_SWITCH_TEST_HOME", value),
            None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
        }
        result
    }

    fn with_omp_home<T>(test_fn: impl FnOnce() -> T) -> T {
        with_test_home(|| {
            let omp = crate::config::get_home_dir().join(".omp");
            fs::create_dir_all(omp.join("agent")).unwrap();
            std::env::set_var("PI_HOME", &omp);
            test_fn()
        })
    }

    #[test]
    fn normalize_inserts_default_providers() {
        let mut v = serde_json::json!({});
        normalize_providers_field(&mut v);
        assert!(v.get("providers").unwrap().is_object());
        assert_eq!(
            v.get("providers").unwrap().as_object().unwrap().len(),
            0
        );
    }

    #[test]
    fn normalize_converts_array_to_dict() {
        let mut v = serde_json::json!({
            "providers": [
                { "name": "openai", "baseUrl": "https://api.openai.com/v1" },
                { "id": "anthropic", "baseUrl": "https://api.anthropic.com" }
            ]
        });
        normalize_providers_field(&mut v);
        let providers = v.get("providers").unwrap().as_object().unwrap();
        assert!(providers.contains_key("openai"));
        assert!(providers.contains_key("anthropic"));
    }

    #[test]
    fn normalize_replaces_invalid_type() {
        let mut v = serde_json::json!({ "providers": "not-an-object" });
        normalize_providers_field(&mut v);
        assert!(v.get("providers").unwrap().is_object());
    }

    #[test]
    fn round_trip_preserves_unknown_fields() {
        with_test_home(|| {
            let provider = PiProviderConfig {
                base_url: Some("https://example.com/v1".to_string()),
                api: Some("openai-completions".to_string()),
                api_key: Some("sk-test".to_string()),
                models: vec![PiModelEntry {
                    id: "model-1".to_string(),
                    name: Some("Model 1".to_string()),
                    reasoning: true,
                    context_window: Some(128000),
                    max_tokens: None,
                    input: vec!["text".to_string()],
                    extra: HashMap::new(),
                }],
                extra: HashMap::new(),
            };
            set_provider("test-provider", provider).unwrap();

            // Now write an unknown field directly to the file and re-read.
            let path = get_pi_config_path();
            let raw = fs::read_to_string(&path).unwrap();
            let mut value: Value = serde_json::from_str(&raw).unwrap();
            value["providers"]["test-provider"]["headers"] =
                serde_json::json!({ "x-custom": "value" });
            fs::write(&path, serde_json::to_string_pretty(&value).unwrap()).unwrap();

            // set_provider again should not clobber the unknown `headers`,
            // and None api_key must clear the previous key.
            let provider2 = PiProviderConfig {
                base_url: Some("https://example.com/v2".to_string()),
                api: Some("openai-completions".to_string()),
                api_key: None,
                models: vec![],
                extra: HashMap::new(),
            };
            set_provider("test-provider", provider2).unwrap();

            let raw = fs::read_to_string(&path).unwrap();
            let value: Value = serde_json::from_str(&raw).unwrap();
            let entry = &value["providers"]["test-provider"];
            assert_eq!(entry["baseUrl"], "https://example.com/v2");
            assert_eq!(entry["headers"]["x-custom"], "value");
            assert!(
                entry.get("apiKey").is_none(),
                "cleared apiKey must be removed from disk"
            );
        });
    }

    #[test]
    fn settings_and_models_backups_use_distinct_prefixes() {
        with_test_home(|| {
            set_active_provider("anthropic").unwrap();
            let provider = PiProviderConfig {
                base_url: Some("https://example.com".to_string()),
                api: Some("openai-completions".to_string()),
                api_key: Some("sk-x".to_string()),
                models: vec![PiModelEntry {
                    id: "m1".to_string(),
                    name: None,
                    reasoning: false,
                    context_window: None,
                    max_tokens: None,
                    input: vec![],
                    extra: HashMap::new(),
                }],
                extra: HashMap::new(),
            };
            set_provider("p1", provider).unwrap();
            // Second write triggers backups.
            set_active_provider("openai").unwrap();
            set_provider(
                "p1",
                PiProviderConfig {
                    base_url: Some("https://example.com/v2".to_string()),
                    api: Some("openai-completions".to_string()),
                    api_key: None,
                    models: vec![],
                    extra: HashMap::new(),
                },
            )
            .unwrap();

            let backup_dir = get_app_config_dir().join("backups").join("pi");
            let names: Vec<String> = fs::read_dir(&backup_dir)
                .unwrap()
                .filter_map(|e| e.ok())
                .filter_map(|e| e.file_name().into_string().ok())
                .collect();
            assert!(
                names.iter().any(|n| n.starts_with("pi_settings_")),
                "expected settings backup, got {names:?}"
            );
            assert!(
                names.iter().any(|n| n.starts_with("pi_models_")),
                "expected models backup, got {names:?}"
            );
        });
    }

    #[test]
    fn scan_detects_missing_fields() {
        with_test_home(|| {
            let raw = r#"{
                "providers": {
                    "good": {
                        "baseUrl": "https://api.example.com",
                        "api": "openai-completions",
                        "models": [{ "id": "m1" }]
                    },
                    "bad": { "models": [] }
                }
            }"#;
            let path = get_pi_config_path();
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, raw).unwrap();

            let warnings = scan_pi_config_health().unwrap();
            assert!(warnings.iter().any(|w| {
                w.code == "missing_base_url" && w.provider.as_deref() == Some("bad")
            }));
            assert!(warnings
                .iter()
                .any(|w| w.code == "missing_api" && w.provider.as_deref() == Some("bad")));
            assert!(warnings
                .iter()
                .any(|w| w.code == "missing_models" && w.provider.as_deref() == Some("bad")));
            assert!(!warnings.iter().any(|w| w.provider.as_deref() == Some("good")));
        });
    }

    #[test]
    fn remove_provider_drops_entry() {
        with_test_home(|| {
            let provider = PiProviderConfig {
                base_url: Some("https://example.com".to_string()),
                api: Some("openai-completions".to_string()),
                api_key: Some("sk-x".to_string()),
                models: vec![],
                extra: HashMap::new(),
            };
            set_provider("to-remove", provider).unwrap();
            assert!(get_provider("to-remove").unwrap().is_some());

            remove_provider("to-remove").unwrap();
            assert!(get_provider("to-remove").unwrap().is_none());
        });
    }

    #[test]
    fn active_provider_round_trips_via_settings_json() {
        with_test_home(|| {
            set_active_provider("anthropic").unwrap();
            assert_eq!(get_active_provider().unwrap().as_deref(), Some("anthropic"));

            let settings_path = get_pi_settings_path();
            assert!(settings_path.exists(), "settings.json should be created");
            let raw = fs::read_to_string(&settings_path).unwrap();
            let value: Value = serde_json::from_str(&raw).unwrap();
            assert_eq!(value["defaultProvider"], "anthropic");

            let models_path = get_pi_config_path();
            if models_path.exists() {
                let models_raw = fs::read_to_string(&models_path).unwrap();
                let models_value: Value = serde_json::from_str(&models_raw).unwrap();
                assert!(
                    models_value.get("defaultProvider").is_none(),
                    "defaultProvider must not be written to models.json"
                );
            }
        });
    }

    #[test]
    fn omp_layout_defaults_to_yml_paths() {
        with_omp_home(|| {
            assert!(get_pi_dir().ends_with(".omp"));
            assert!(get_pi_config_path().ends_with("models.yml"));
            assert!(get_pi_settings_path().ends_with("config.yml"));
        });
    }

    #[test]
    fn omp_active_provider_uses_model_roles_default() {
        with_omp_home(|| {
            let provider = PiProviderConfig {
                base_url: Some("https://api.example.com".to_string()),
                api: Some("openai-completions".to_string()),
                api_key: Some("sk-test".to_string()),
                models: vec![PiModelEntry {
                    id: "fast-chat".to_string(),
                    name: Some("Fast".to_string()),
                    reasoning: false,
                    context_window: None,
                    max_tokens: None,
                    input: vec![],
                    extra: HashMap::new(),
                }],
                extra: HashMap::new(),
            };
            set_provider("my-gateway", provider).unwrap();
            set_active_provider("my-gateway").unwrap();

            assert_eq!(
                get_active_provider().unwrap().as_deref(),
                Some("my-gateway")
            );

            let settings_path = get_pi_settings_path();
            assert!(settings_path.ends_with("config.yml"));
            let raw = fs::read_to_string(&settings_path).unwrap();
            let value: Value = parse_settings_text(&settings_path, &raw).unwrap();
            assert_eq!(value["modelRoles"]["default"], "my-gateway/fast-chat");
            assert!(value.get("defaultProvider").is_none());

            // Preserve model + thinking when re-activating same provider.
            value_set_model_role(&settings_path, "my-gateway/fast-chat:high");
            set_active_provider("my-gateway").unwrap();
            let raw = fs::read_to_string(&settings_path).unwrap();
            let value: Value = parse_settings_text(&settings_path, &raw).unwrap();
            assert_eq!(value["modelRoles"]["default"], "my-gateway/fast-chat:high");
        });
    }

    fn value_set_model_role(path: &Path, role: &str) {
        let mut settings = read_pi_settings().unwrap();
        settings["modelRoles"]["default"] = Value::String(role.to_string());
        let serialized = serialize_settings_text(path, &settings).unwrap();
        fs::write(path, serialized).unwrap();
    }

    #[test]
    fn omp_round_trip_models_yml() {
        with_omp_home(|| {
            let provider = PiProviderConfig {
                base_url: Some("https://gateway.example.com/v1".to_string()),
                api: Some("openai-completions".to_string()),
                api_key: Some("MY_KEY".to_string()),
                models: vec![PiModelEntry {
                    id: "claude-sonnet".to_string(),
                    name: Some("Claude Sonnet".to_string()),
                    reasoning: false,
                    context_window: Some(200000),
                    max_tokens: Some(8192),
                    input: vec!["text".to_string()],
                    extra: HashMap::new(),
                }],
                extra: HashMap::new(),
            };
            set_provider("my-gateway", provider).unwrap();

            let path = get_pi_config_path();
            assert!(path.ends_with("models.yml"));
            let raw = fs::read_to_string(&path).unwrap();
            assert!(raw.contains("my-gateway"));
            assert!(raw.contains("baseUrl") || raw.contains("base_url") || raw.contains("claude-sonnet"));

            let loaded = get_provider("my-gateway").unwrap().expect("provider");
            assert_eq!(loaded.base_url.as_deref(), Some("https://gateway.example.com/v1"));
            assert_eq!(loaded.models[0].id, "claude-sonnet");
        });
    }
}
