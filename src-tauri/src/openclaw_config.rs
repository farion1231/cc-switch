//! OpenClaw 配置文件读写模块
//!
//! 处理 `~/.openclaw/openclaw.json` 配置文件的读写操作（JSON5 格式）。
//! OpenClaw 使用累加式供应商管理，所有供应商配置共存于同一配置文件中。

use crate::config::{atomic_write, get_app_config_dir};
use crate::error::AppError;
use crate::settings::{effective_backup_retain_count, get_openclaw_override_dir};
use chrono::Local;
use indexmap::IndexMap;
use json_five::rt::parser::{
    from_str as rt_from_str, JSONKeyValuePair as RtJSONKeyValuePair,
    JSONObjectContext as RtJSONObjectContext, JSONText as RtJSONText, JSONValue as RtJSONValue,
    KeyValuePairContext as RtKeyValuePairContext,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

const OPENCLAW_DEFAULT_SOURCE: &str =
    "{\n  models: {\n    mode: 'merge',\n    providers: {},\n  },\n}\n";
const OPENCLAW_TOOLS_PROFILES: &[&str] = &["minimal", "coding", "messaging", "full"];

// ============================================================================
// Path Functions
// ============================================================================

/// 获取 OpenClaw 配置目录
///
/// 默认路径: `~/.openclaw/`，非默认 profile 使用 `~/.openclaw-<profile>/`。
/// settings.openclaw_config_dir 优先，其次是 OPENCLAW_STATE_DIR。
pub fn get_openclaw_dir() -> PathBuf {
    // Tests set CC_SWITCH_TEST_HOME and must never inherit the user's
    // persistent settings override or write outside their temporary root.
    let override_dir = if env::var_os("CC_SWITCH_TEST_HOME").is_some() {
        None
    } else {
        get_openclaw_override_dir()
    };
    resolve_openclaw_dir(
        &crate::config::get_home_dir(),
        override_dir,
        std::env::var("OPENCLAW_STATE_DIR").ok().as_deref(),
        std::env::var("OPENCLAW_PROFILE").ok().as_deref(),
    )
}

/// 获取 OpenClaw 配置文件路径
///
/// 返回当前 OpenClaw 配置目录下的 `openclaw.json`
pub fn get_openclaw_config_path() -> PathBuf {
    get_openclaw_dir().join("openclaw.json")
}

fn non_empty_path(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

fn resolve_openclaw_dir(
    home: &Path,
    override_dir: Option<PathBuf>,
    state_dir: Option<&str>,
    profile: Option<&str>,
) -> PathBuf {
    if let Some(dir) = override_dir {
        return dir;
    }
    if let Some(dir) = non_empty_path(state_dir) {
        return expand_config_path(dir, home);
    }
    match non_empty_path(profile).filter(|value| !value.eq_ignore_ascii_case("default")) {
        Some(profile) => home.join(format!(".openclaw-{profile}")),
        None => home.join(".openclaw"),
    }
}

/// Resolve the shared OpenClaw workspace from the official configuration.
///
/// `agents.defaults.workspace` takes precedence over `OPENCLAW_WORKSPACE_DIR`,
/// then defaults to `<OpenClaw state directory>/workspace` (including profiles).
/// Propagate configuration errors so writes never silently target a fallback.
pub fn get_openclaw_workspace_dir() -> Result<PathBuf, AppError> {
    let config = read_effective_workspace_config()?;
    Ok(resolve_openclaw_workspace_dir(
        &config,
        &get_openclaw_dir(),
        &crate::config::get_home_dir(),
        std::env::var("OPENCLAW_WORKSPACE_DIR").ok().as_deref(),
    ))
}

fn resolve_openclaw_workspace_dir(
    config: &Value,
    state_dir: &Path,
    home: &Path,
    workspace_env: Option<&str>,
) -> PathBuf {
    let configured = config
        .pointer("/agents/defaults/workspace")
        .and_then(Value::as_str);
    if let Some(path) = non_empty_path(configured) {
        return expand_config_path(path, home);
    }
    if let Some(path) = non_empty_path(workspace_env) {
        return resolve_env_path(path);
    }
    state_dir.join("workspace")
}

fn expand_config_path(path: &str, home: &Path) -> PathBuf {
    if path == "~" {
        home.to_path_buf()
    } else if let Some(relative) = path.strip_prefix("~/").or_else(|| path.strip_prefix("~\\")) {
        home.join(relative)
    } else {
        PathBuf::from(path)
    }
}

/// Match Node's `path.resolve()` semantics used for OPENCLAW_*_DIR values.
/// A literal `~/workspace` is relative to the process cwd and is not expanded.
fn resolve_env_path(path: &str) -> PathBuf {
    let candidate = PathBuf::from(path);
    if candidate.is_absolute() {
        candidate
    } else {
        env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(candidate)
    }
}

fn read_effective_workspace_config() -> Result<Value, AppError> {
    let path = get_openclaw_config_path();
    if !path.exists() {
        return Ok(default_openclaw_config_value());
    }

    let source = fs::read_to_string(&path).map_err(|e| AppError::io(&path, e))?;
    let parsed: Value = json5::from_str(&source)
        .map_err(|e| AppError::Config(format!("Failed to parse OpenClaw config as JSON5: {e}")))?;
    let root = path.parent().unwrap_or_else(|| Path::new("."));
    let mut chain = vec![path.clone()];
    let mut included = resolve_workspace_includes(parsed, root, root, &mut chain)?;
    if let Some(workspace) = included.pointer("/agents/defaults/workspace").cloned() {
        let resolved = resolve_workspace_env(workspace)?;
        if let Some(defaults) = included
            .get_mut("agents")
            .and_then(Value::as_object_mut)
            .and_then(|agents| agents.get_mut("defaults"))
            .and_then(Value::as_object_mut)
        {
            defaults.insert("workspace".to_string(), resolved);
        }
    }
    Ok(included)
}

fn resolve_workspace_includes(
    value: Value,
    base_dir: &Path,
    root_dir: &Path,
    chain: &mut Vec<PathBuf>,
) -> Result<Value, AppError> {
    match value {
        Value::Array(values) => Ok(Value::Array(
            values
                .into_iter()
                .map(|item| resolve_workspace_includes(item, base_dir, root_dir, chain))
                .collect::<Result<Vec<_>, _>>()?,
        )),
        Value::Object(mut object) => {
            let include = object.remove("$include");
            let mut merged = Map::new();
            if let Some(include) = include {
                let paths = match include {
                    Value::String(path) => vec![path],
                    Value::Array(paths) => paths
                        .into_iter()
                        .map(|path| {
                            path.as_str().map(str::to_owned).ok_or_else(|| {
                                AppError::Config(
                                    "OpenClaw $include entries must be strings".to_string(),
                                )
                            })
                        })
                        .collect::<Result<Vec<_>, _>>()?,
                    _ => {
                        return Err(AppError::Config(
                            "OpenClaw $include must be a string or array of strings".to_string(),
                        ));
                    }
                };
                for include_path in paths {
                    let include_file = resolve_include_path(&include_path, base_dir, root_dir)?;
                    if chain.contains(&include_file) {
                        return Err(AppError::Config(format!(
                            "Circular OpenClaw $include detected: {}",
                            include_file.display()
                        )));
                    }
                    let include_source = fs::read_to_string(&include_file).map_err(|e| {
                        AppError::Config(format!(
                            "Failed to read OpenClaw include {}: {e}",
                            include_file.display()
                        ))
                    })?;
                    let parsed: Value = json5::from_str(&include_source).map_err(|e| {
                        AppError::Config(format!(
                            "Failed to parse OpenClaw include {}: {e}",
                            include_file.display()
                        ))
                    })?;
                    chain.push(include_file.clone());
                    let resolved = resolve_workspace_includes(
                        parsed,
                        include_file.parent().unwrap_or(root_dir),
                        root_dir,
                        chain,
                    )?;
                    chain.pop();
                    let resolved = resolved.as_object().ok_or_else(|| {
                        AppError::Config(format!(
                            "OpenClaw include {} must contain an object",
                            include_file.display()
                        ))
                    })?;
                    merge_workspace_objects(&mut merged, resolved.clone());
                }
            }
            let mut siblings = Map::new();
            for (key, child) in object {
                siblings.insert(
                    key,
                    resolve_workspace_includes(child, base_dir, root_dir, chain)?,
                );
            }
            merge_workspace_objects(&mut merged, siblings);
            Ok(Value::Object(merged))
        }
        other => Ok(other),
    }
}

fn resolve_include_path(path: &str, base_dir: &Path, root_dir: &Path) -> Result<PathBuf, AppError> {
    if path.is_empty() || path.contains('\0') || path.len() >= 4096 {
        return Err(AppError::Config(format!(
            "Invalid OpenClaw include path: {path}"
        )));
    }
    let candidate = if Path::new(path).is_absolute() {
        PathBuf::from(path)
    } else {
        base_dir.join(path)
    };
    let canonical = candidate
        .canonicalize()
        .map_err(|e| AppError::Config(format!("Failed to resolve OpenClaw include {path}: {e}")))?;
    let root = root_dir
        .canonicalize()
        .unwrap_or_else(|_| root_dir.to_path_buf());
    if !canonical.starts_with(&root) {
        return Err(AppError::Config(format!(
            "OpenClaw include escapes config directory: {path}"
        )));
    }
    Ok(canonical)
}

fn merge_workspace_objects(target: &mut Map<String, Value>, source: Map<String, Value>) {
    for (key, value) in source {
        match (target.get_mut(&key), value) {
            (Some(Value::Object(existing)), Value::Object(incoming)) => {
                merge_workspace_objects(existing, incoming)
            }
            (_, value) => {
                target.insert(key, value);
            }
        }
    }
}

fn resolve_workspace_env(value: Value) -> Result<Value, AppError> {
    match value {
        Value::String(value) => Ok(Value::String(resolve_workspace_env_string(&value)?)),
        Value::Array(values) => Ok(Value::Array(
            values
                .into_iter()
                .map(resolve_workspace_env)
                .collect::<Result<Vec<_>, _>>()?,
        )),
        Value::Object(object) => Ok(Value::Object(
            object
                .into_iter()
                .map(|(key, value)| Ok((key, resolve_workspace_env(value)?)))
                .collect::<Result<Map<_, _>, AppError>>()?,
        )),
        other => Ok(other),
    }
}

fn resolve_workspace_env_string(value: &str) -> Result<String, AppError> {
    let mut output = String::with_capacity(value.len());
    let mut cursor = 0;
    while cursor < value.len() {
        let rest = &value[cursor..];
        if rest.starts_with("$${") {
            let end = rest.find('}').ok_or_else(|| {
                AppError::Config("Unterminated escaped OpenClaw environment reference".to_string())
            })?;
            output.push_str(&rest[1..=end]);
            cursor += end + 1;
            continue;
        }
        if !rest.starts_with("${") {
            let ch = rest.chars().next().expect("non-empty string");
            output.push(ch);
            cursor += ch.len_utf8();
            continue;
        }
        let end = rest.find('}').ok_or_else(|| {
            AppError::Config("Unterminated OpenClaw environment reference".to_string())
        })?;
        let expression = &rest[2..end];
        let (name, fallback) = expression
            .split_once(":-")
            .map_or((expression, None), |(name, fallback)| {
                (name, Some(fallback))
            });
        let valid_name = !name.is_empty()
            && name.chars().enumerate().all(|(index, ch)| {
                ch == '_' || ch.is_ascii_uppercase() || (index > 0 && ch.is_ascii_digit())
            });
        if !valid_name || fallback.is_some_and(|fallback| fallback.contains(['$', '{'])) {
            output.push_str(&rest[..=end]);
            cursor += end + 1;
            continue;
        }
        let resolved = env::var(name)
            .ok()
            .filter(|value| !value.is_empty())
            .or_else(|| fallback.map(str::to_owned))
            .ok_or_else(|| {
                AppError::Config(format!("Missing OpenClaw environment variable: {name}"))
            })?;
        output.push_str(&resolved);
        cursor += end + 1;
    }
    Ok(output)
}

fn default_openclaw_config_value() -> Value {
    json!({
        "models": {
            "mode": "merge",
            "providers": {}
        }
    })
}

fn openclaw_write_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

// ============================================================================
// Type Definitions
// ============================================================================

/// OpenClaw 健康检查警告
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OpenClawHealthWarning {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

/// OpenClaw 写入结果
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct OpenClawWriteOutcome {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backup_path: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<OpenClawHealthWarning>,
}

/// OpenClaw 供应商配置（对应 models.providers 中的条目）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenClawProviderConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub models: Vec<OpenClawModelEntry>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub headers: HashMap<String, String>,
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

/// OpenClaw 模型条目
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenClawModelEntry {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alias: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost: Option<OpenClawModelCost>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u32>,
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

/// OpenClaw 模型成本配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenClawModelCost {
    pub input: f64,
    pub output: f64,
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

/// OpenClaw 默认模型配置（agents.defaults.model）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenClawDefaultModel {
    pub primary: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fallbacks: Vec<String>,
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

/// OpenClaw 模型目录条目（agents.defaults.models 中的值）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenClawModelCatalogEntry {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alias: Option<String>,
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

/// OpenClaw agents.defaults 配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenClawAgentsDefaults {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<OpenClawDefaultModel>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub models: Option<HashMap<String, OpenClawModelCatalogEntry>>,
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

/// OpenClaw agents 顶层配置
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct OpenClawAgents {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub defaults: Option<OpenClawAgentsDefaults>,
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

/// OpenClaw env 配置（openclaw.json 的 env 节点）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenClawEnvConfig {
    #[serde(flatten)]
    pub vars: HashMap<String, Value>,
}

/// OpenClaw tools 配置（openclaw.json 的 tools 节点）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenClawToolsConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allow: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub deny: Vec<String>,
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

// ============================================================================
// Core Read/Write Functions
// ============================================================================

/// 读取 OpenClaw 配置文件
///
/// 支持 JSON5 格式，返回完整的配置 JSON 对象
pub fn read_openclaw_config() -> Result<Value, AppError> {
    let path = get_openclaw_config_path();
    if !path.exists() {
        return Ok(default_openclaw_config_value());
    }

    let content = fs::read_to_string(&path).map_err(|e| AppError::io(&path, e))?;
    json5::from_str(&content)
        .map_err(|e| AppError::Config(format!("Failed to parse OpenClaw config as JSON5: {e}")))
}

/// 对现有 OpenClaw 配置做健康检查。
///
/// 解析失败时返回单条 parse 警告，不抛出错误。
pub fn scan_openclaw_config_health() -> Result<Vec<OpenClawHealthWarning>, AppError> {
    let path = get_openclaw_config_path();
    if !path.exists() {
        return Ok(Vec::new());
    }

    let content = fs::read_to_string(&path).map_err(|e| AppError::io(&path, e))?;
    match json5::from_str::<Value>(&content) {
        Ok(config) => Ok(scan_openclaw_health_from_value(&config)),
        Err(err) => Ok(vec![OpenClawHealthWarning {
            code: "config_parse_failed".to_string(),
            message: format!("OpenClaw config could not be parsed as JSON5: {err}"),
            path: Some(path.display().to_string()),
        }]),
    }
}

struct OpenClawConfigDocument {
    path: PathBuf,
    original_source: Option<String>,
    text: RtJSONText,
}

impl OpenClawConfigDocument {
    fn load() -> Result<Self, AppError> {
        let path = get_openclaw_config_path();
        let original_source = if path.exists() {
            Some(fs::read_to_string(&path).map_err(|e| AppError::io(&path, e))?)
        } else {
            None
        };

        let source = original_source
            .clone()
            .unwrap_or_else(|| OPENCLAW_DEFAULT_SOURCE.to_string());
        let text = rt_from_str(&source).map_err(|e| {
            AppError::Config(format!(
                "Failed to parse OpenClaw config as round-trip JSON5 document: {}",
                e.message
            ))
        })?;

        Ok(Self {
            path,
            original_source,
            text,
        })
    }

    fn set_root_section(&mut self, key: &str, value: &Value) -> Result<(), AppError> {
        let RtJSONValue::JSONObject {
            key_value_pairs,
            context,
        } = &mut self.text.value
        else {
            return Err(AppError::Config(
                "OpenClaw config root must be a JSON5 object".to_string(),
            ));
        };

        if key_value_pairs.is_empty()
            && context
                .as_ref()
                .map(|ctx| ctx.wsc.0.is_empty())
                .unwrap_or(true)
        {
            *context = Some(RtJSONObjectContext {
                wsc: ("\n  ".to_string(),),
            });
        }

        let leading_ws = context
            .as_ref()
            .map(|ctx| ctx.wsc.0.clone())
            .unwrap_or_default();
        let entry_separator_ws = derive_entry_separator(&leading_ws);
        let child_indent = extract_trailing_indent(&leading_ws);
        let new_value = value_to_rt_value(value, &child_indent)?;

        if let Some(existing) = key_value_pairs
            .iter_mut()
            .find(|pair| json5_key_name(&pair.key) == Some(key))
        {
            existing.value = new_value;
            return Ok(());
        }

        let new_pair = if let Some(last_pair) = key_value_pairs.last_mut() {
            let last_ctx = ensure_kvp_context(last_pair);
            let closing_ws = if let Some(after_comma) = last_ctx.wsc.3.clone() {
                last_ctx.wsc.3 = Some(entry_separator_ws.clone());
                after_comma
            } else {
                let closing_ws = std::mem::take(&mut last_ctx.wsc.2);
                last_ctx.wsc.3 = Some(entry_separator_ws.clone());
                closing_ws
            };

            make_root_pair(key, new_value, closing_ws)
        } else {
            make_root_pair(
                key,
                new_value,
                derive_closing_ws_from_separator(&leading_ws),
            )
        };

        key_value_pairs.push(new_pair);
        Ok(())
    }

    fn save(self) -> Result<OpenClawWriteOutcome, AppError> {
        let _guard = openclaw_write_lock().lock()?;

        let current_source = if self.path.exists() {
            Some(fs::read_to_string(&self.path).map_err(|e| AppError::io(&self.path, e))?)
        } else {
            None
        };

        if current_source != self.original_source {
            return Err(AppError::Config(
                "OpenClaw config changed on disk. Please reload and try again.".to_string(),
            ));
        }

        let next_source = self.text.to_string();
        if current_source.as_deref() == Some(next_source.as_str()) {
            let warnings = scan_openclaw_health_from_value(
                &json5::from_str::<Value>(&next_source).map_err(|e| {
                    AppError::Config(format!(
                        "Failed to parse unchanged OpenClaw config as JSON5: {e}"
                    ))
                })?,
            );

            return Ok(OpenClawWriteOutcome {
                backup_path: None,
                warnings,
            });
        }

        let backup_path = current_source
            .as_ref()
            .map(|source| create_openclaw_backup(source))
            .transpose()?
            .map(|path| path.display().to_string());

        atomic_write(&self.path, next_source.as_bytes())?;

        let warnings = scan_openclaw_health_from_value(
            &json5::from_str::<Value>(&next_source).map_err(|e| {
                AppError::Config(format!(
                    "Failed to parse newly written OpenClaw config as JSON5: {e}"
                ))
            })?,
        );

        log::debug!("OpenClaw config written to {:?}", self.path);
        Ok(OpenClawWriteOutcome {
            backup_path,
            warnings,
        })
    }
}

fn write_root_section(section: &str, value: &Value) -> Result<OpenClawWriteOutcome, AppError> {
    let mut document = OpenClawConfigDocument::load()?;
    document.set_root_section(section, value)?;
    document.save()
}

fn create_openclaw_backup(source: &str) -> Result<PathBuf, AppError> {
    let backup_dir = get_app_config_dir().join("backups").join("openclaw");
    fs::create_dir_all(&backup_dir).map_err(|e| AppError::io(&backup_dir, e))?;

    let base_id = format!("openclaw_{}", Local::now().format("%Y%m%d_%H%M%S"));
    let mut filename = format!("{base_id}.json5");
    let mut backup_path = backup_dir.join(&filename);
    let mut counter = 1;

    while backup_path.exists() {
        filename = format!("{base_id}_{counter}.json5");
        backup_path = backup_dir.join(&filename);
        counter += 1;
    }

    atomic_write(&backup_path, source.as_bytes())?;
    cleanup_openclaw_backups(&backup_dir)?;
    Ok(backup_path)
}

fn cleanup_openclaw_backups(dir: &Path) -> Result<(), AppError> {
    let retain = effective_backup_retain_count();
    let mut entries = fs::read_dir(dir)
        .map_err(|e| AppError::io(dir, e))?
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            entry
                .path()
                .extension()
                .map(|ext| ext == "json5" || ext == "json")
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
                "Failed to remove old OpenClaw config backup {}: {err}",
                entry.path().display()
            );
        }
    }

    Ok(())
}

fn ensure_object(value: &mut Value) -> &mut Map<String, Value> {
    if !value.is_object() {
        *value = Value::Object(Map::new());
    }
    value
        .as_object_mut()
        .expect("value should be object after normalization")
}

fn ensure_kvp_context(pair: &mut RtJSONKeyValuePair) -> &mut RtKeyValuePairContext {
    pair.context.get_or_insert_with(|| RtKeyValuePairContext {
        wsc: (String::new(), " ".to_string(), String::new(), None),
    })
}

fn extract_trailing_indent(separator_ws: &str) -> String {
    separator_ws
        .rsplit_once('\n')
        .map(|(_, tail)| tail.to_string())
        .unwrap_or_default()
}

fn derive_closing_ws_from_separator(separator_ws: &str) -> String {
    let Some((prefix, indent)) = separator_ws.rsplit_once('\n') else {
        return String::new();
    };

    let reduced_indent = if indent.ends_with('\t') {
        &indent[..indent.len().saturating_sub(1)]
    } else if indent.ends_with("  ") {
        &indent[..indent.len().saturating_sub(2)]
    } else if indent.ends_with(' ') {
        &indent[..indent.len().saturating_sub(1)]
    } else {
        indent
    };

    format!("{prefix}\n{reduced_indent}")
}

fn derive_entry_separator(leading_ws: &str) -> String {
    if leading_ws.is_empty() {
        return String::new();
    }

    if leading_ws.contains('\n') {
        return format!("\n{}", extract_trailing_indent(leading_ws));
    }

    String::new()
}

fn value_to_rt_value(value: &Value, parent_indent: &str) -> Result<RtJSONValue, AppError> {
    // `json-five` 0.3.1 can panic when pretty-printing nested empty maps/arrays.
    // Serialize with `serde_json` instead; the resulting JSON is valid JSON5 and
    // can still be parsed back into the round-trip AST we use for insertion.
    let source = serde_json::to_string_pretty(value)
        .map_err(|e| AppError::Config(format!("Failed to serialize JSON section: {e}")))?;

    let adjusted = reindent_json5_block(&source, parent_indent);
    let text = rt_from_str(&adjusted).map_err(|e| {
        AppError::Config(format!(
            "Failed to parse generated JSON5 section: {}",
            e.message
        ))
    })?;
    Ok(text.value)
}

fn reindent_json5_block(source: &str, parent_indent: &str) -> String {
    let normalized = normalize_json_five_output(source);
    if parent_indent.is_empty() || !normalized.contains('\n') {
        return normalized;
    }

    let mut lines = normalized.lines();
    let Some(first_line) = lines.next() else {
        return String::new();
    };

    let mut result = String::from(first_line);
    for line in lines {
        result.push('\n');
        result.push_str(parent_indent);
        result.push_str(line);
    }
    result
}

fn normalize_json_five_output(source: &str) -> String {
    source.replace("\\/", "/")
}

fn make_root_pair(key: &str, value: RtJSONValue, closing_ws: String) -> RtJSONKeyValuePair {
    RtJSONKeyValuePair {
        key: make_json5_key(key),
        value,
        context: Some(RtKeyValuePairContext {
            wsc: (String::new(), " ".to_string(), closing_ws, None),
        }),
    }
}

fn make_json5_key(key: &str) -> RtJSONValue {
    if is_identifier_key(key) {
        RtJSONValue::Identifier(key.to_string())
    } else {
        RtJSONValue::DoubleQuotedString(key.to_string())
    }
}

fn is_identifier_key(key: &str) -> bool {
    let mut chars = key.chars();
    let Some(first) = chars.next() else {
        return false;
    };

    matches!(first, 'a'..='z' | 'A'..='Z' | '_' | '$')
        && chars.all(|ch| matches!(ch, 'a'..='z' | 'A'..='Z' | '0'..='9' | '_' | '$'))
}

fn json5_key_name(key: &RtJSONValue) -> Option<&str> {
    match key {
        RtJSONValue::Identifier(name)
        | RtJSONValue::DoubleQuotedString(name)
        | RtJSONValue::SingleQuotedString(name) => Some(name),
        _ => None,
    }
}

fn warning(code: &str, message: impl Into<String>, path: Option<&str>) -> OpenClawHealthWarning {
    OpenClawHealthWarning {
        code: code.to_string(),
        message: message.into(),
        path: path.map(|value| value.to_string()),
    }
}

fn scan_openclaw_health_from_value(config: &Value) -> Vec<OpenClawHealthWarning> {
    let mut warnings = Vec::new();

    if let Some(profile) = config
        .get("tools")
        .and_then(|tools| tools.get("profile"))
        .and_then(Value::as_str)
    {
        if !OPENCLAW_TOOLS_PROFILES.contains(&profile) {
            warnings.push(warning(
                "invalid_tools_profile",
                format!("tools.profile uses unsupported value '{profile}'."),
                Some("tools.profile"),
            ));
        }
    }

    if config
        .get("agents")
        .and_then(|agents| agents.get("defaults"))
        .and_then(|defaults| defaults.get("timeout"))
        .is_some()
    {
        warnings.push(warning(
            "legacy_agents_timeout",
            "agents.defaults.timeout is deprecated; use agents.defaults.timeoutSeconds.",
            Some("agents.defaults.timeout"),
        ));
    }

    if let Some(value) = config.get("env").and_then(|env| env.get("vars")) {
        if !value.is_object() {
            warnings.push(warning(
                "stringified_env_vars",
                "env.vars should be an object. The current value looks stringified or malformed.",
                Some("env.vars"),
            ));
        }
    }

    if let Some(value) = config.get("env").and_then(|env| env.get("shellEnv")) {
        if !value.is_object() {
            warnings.push(warning(
                "stringified_env_shell_env",
                "env.shellEnv should be an object. The current value looks stringified or malformed.",
                Some("env.shellEnv"),
            ));
        }
    }

    warnings
}

fn remove_legacy_timeout(defaults_value: &mut Value) {
    if let Some(defaults_obj) = defaults_value.as_object_mut() {
        defaults_obj.remove("timeout");
    }
}

// ============================================================================
// Provider Functions (Untyped - for raw JSON operations)
// ============================================================================

/// 获取所有供应商配置（原始 JSON）
///
/// 从 `models.providers` 读取
pub fn get_providers() -> Result<Map<String, Value>, AppError> {
    let config = read_openclaw_config()?;
    Ok(config
        .get("models")
        .and_then(|m| m.get("providers"))
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default())
}

/// 获取单个供应商配置（原始 JSON）
pub fn get_provider(id: &str) -> Result<Option<Value>, AppError> {
    Ok(get_providers()?.get(id).cloned())
}

/// 设置供应商配置（原始 JSON）
///
/// 写入到 `models.providers`
pub fn set_provider(id: &str, provider_config: Value) -> Result<OpenClawWriteOutcome, AppError> {
    let mut full_config = read_openclaw_config()?;
    let root = ensure_object(&mut full_config);
    let models = root.entry("models".to_string()).or_insert_with(|| {
        json!({
            "mode": "merge",
            "providers": {}
        })
    });
    let providers = ensure_object(models)
        .entry("providers".to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    ensure_object(providers).insert(id.to_string(), provider_config);

    let models_value = root.get("models").cloned().unwrap_or_else(|| {
        json!({
            "mode": "merge",
            "providers": {}
        })
    });
    write_root_section("models", &models_value)
}

/// 删除供应商配置
pub fn remove_provider(id: &str) -> Result<OpenClawWriteOutcome, AppError> {
    let mut config = read_openclaw_config()?;
    let mut removed = false;

    if let Some(providers) = config
        .get_mut("models")
        .and_then(|models| models.get_mut("providers"))
        .and_then(Value::as_object_mut)
    {
        removed = providers.remove(id).is_some();
    }

    if !removed {
        return Ok(OpenClawWriteOutcome::default());
    }

    let models_value = config.get("models").cloned().unwrap_or_else(|| {
        json!({
            "mode": "merge",
            "providers": {}
        })
    });
    write_root_section("models", &models_value)
}

// ============================================================================
// Provider Functions (Typed)
// ============================================================================

/// 获取所有供应商配置（类型化）
pub fn get_typed_providers() -> Result<IndexMap<String, OpenClawProviderConfig>, AppError> {
    let providers = get_providers()?;
    let mut result = IndexMap::new();

    for (id, value) in providers {
        match serde_json::from_value::<OpenClawProviderConfig>(value.clone()) {
            Ok(config) => {
                result.insert(id, config);
            }
            Err(e) => {
                log::warn!("Failed to parse OpenClaw provider '{id}': {e}");
            }
        }
    }

    Ok(result)
}

/// 设置供应商配置（类型化）
pub fn set_typed_provider(
    id: &str,
    config: &OpenClawProviderConfig,
) -> Result<OpenClawWriteOutcome, AppError> {
    let value = serde_json::to_value(config).map_err(|e| AppError::JsonSerialize { source: e })?;
    set_provider(id, value)
}

// ============================================================================
// Agents Configuration Functions
// ============================================================================

/// 读取默认模型配置（agents.defaults.model）
pub fn get_default_model() -> Result<Option<OpenClawDefaultModel>, AppError> {
    let config = read_openclaw_config()?;

    let Some(model_value) = config
        .get("agents")
        .and_then(|a| a.get("defaults"))
        .and_then(|d| d.get("model"))
    else {
        return Ok(None);
    };

    let model = serde_json::from_value(model_value.clone())
        .map_err(|e| AppError::Config(format!("Failed to parse agents.defaults.model: {e}")))?;
    Ok(Some(model))
}

/// 设置默认模型配置（agents.defaults.model）
pub fn set_default_model(model: &OpenClawDefaultModel) -> Result<OpenClawWriteOutcome, AppError> {
    let mut config = read_openclaw_config()?;
    let root = ensure_object(&mut config);
    let agents = root
        .entry("agents".to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    let defaults = ensure_object(agents)
        .entry("defaults".to_string())
        .or_insert_with(|| Value::Object(Map::new()));

    let model_value =
        serde_json::to_value(model).map_err(|e| AppError::JsonSerialize { source: e })?;
    ensure_object(defaults).insert("model".to_string(), model_value);

    let agents_value = root
        .get("agents")
        .cloned()
        .unwrap_or_else(|| Value::Object(Map::new()));
    write_root_section("agents", &agents_value)
}

/// 读取模型目录/允许列表（agents.defaults.models）
pub fn get_model_catalog() -> Result<Option<HashMap<String, OpenClawModelCatalogEntry>>, AppError> {
    let config = read_openclaw_config()?;

    let Some(models_value) = config
        .get("agents")
        .and_then(|a| a.get("defaults"))
        .and_then(|d| d.get("models"))
    else {
        return Ok(None);
    };

    let catalog = serde_json::from_value(models_value.clone())
        .map_err(|e| AppError::Config(format!("Failed to parse agents.defaults.models: {e}")))?;
    Ok(Some(catalog))
}

/// 设置模型目录/允许列表（agents.defaults.models）
pub fn set_model_catalog(
    catalog: &HashMap<String, OpenClawModelCatalogEntry>,
) -> Result<OpenClawWriteOutcome, AppError> {
    let mut config = read_openclaw_config()?;
    let root = ensure_object(&mut config);
    let agents = root
        .entry("agents".to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    let defaults = ensure_object(agents)
        .entry("defaults".to_string())
        .or_insert_with(|| Value::Object(Map::new()));

    let catalog_value =
        serde_json::to_value(catalog).map_err(|e| AppError::JsonSerialize { source: e })?;
    ensure_object(defaults).insert("models".to_string(), catalog_value);

    let agents_value = root
        .get("agents")
        .cloned()
        .unwrap_or_else(|| Value::Object(Map::new()));
    write_root_section("agents", &agents_value)
}

// ============================================================================
// Full Agents Defaults Functions
// ============================================================================

/// Read the full agents.defaults config
pub fn get_agents_defaults() -> Result<Option<OpenClawAgentsDefaults>, AppError> {
    let config = read_openclaw_config()?;

    let Some(defaults_value) = config.get("agents").and_then(|a| a.get("defaults")) else {
        return Ok(None);
    };

    let defaults = serde_json::from_value(defaults_value.clone())
        .map_err(|e| AppError::Config(format!("Failed to parse agents.defaults: {e}")))?;
    Ok(Some(defaults))
}

/// Write the full agents.defaults config
pub fn set_agents_defaults(
    defaults: &OpenClawAgentsDefaults,
) -> Result<OpenClawWriteOutcome, AppError> {
    let mut config = read_openclaw_config()?;
    let root = ensure_object(&mut config);
    let agents = root
        .entry("agents".to_string())
        .or_insert_with(|| Value::Object(Map::new()));

    let mut defaults_value =
        serde_json::to_value(defaults).map_err(|e| AppError::JsonSerialize { source: e })?;
    remove_legacy_timeout(&mut defaults_value);
    ensure_object(agents).insert("defaults".to_string(), defaults_value);

    let agents_value = root
        .get("agents")
        .cloned()
        .unwrap_or_else(|| Value::Object(Map::new()));
    write_root_section("agents", &agents_value)
}

// ============================================================================
// Env Configuration
// ============================================================================

/// Read the env config section
pub fn get_env_config() -> Result<OpenClawEnvConfig, AppError> {
    let config = read_openclaw_config()?;

    let Some(env_value) = config.get("env") else {
        return Ok(OpenClawEnvConfig {
            vars: HashMap::new(),
        });
    };

    serde_json::from_value(env_value.clone())
        .map_err(|e| AppError::Config(format!("Failed to parse env config: {e}")))
}

/// Write the env config section
pub fn set_env_config(env: &OpenClawEnvConfig) -> Result<OpenClawWriteOutcome, AppError> {
    let value = serde_json::to_value(env).map_err(|e| AppError::JsonSerialize { source: e })?;
    write_root_section("env", &value)
}

// ============================================================================
// Tools Configuration
// ============================================================================

/// Read the tools config section
pub fn get_tools_config() -> Result<OpenClawToolsConfig, AppError> {
    let config = read_openclaw_config()?;

    let Some(tools_value) = config.get("tools") else {
        return Ok(OpenClawToolsConfig {
            profile: None,
            allow: Vec::new(),
            deny: Vec::new(),
            extra: HashMap::new(),
        });
    };

    serde_json::from_value(tools_value.clone())
        .map_err(|e| AppError::Config(format!("Failed to parse tools config: {e}")))
}

/// Write the tools config section
pub fn set_tools_config(tools: &OpenClawToolsConfig) -> Result<OpenClawWriteOutcome, AppError> {
    let value = serde_json::to_value(tools).map_err(|e| AppError::JsonSerialize { source: e })?;
    write_root_section("tools", &value)
}

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

    struct TestEnvironment {
        values: Vec<(&'static str, Option<std::ffi::OsString>)>,
    }

    impl TestEnvironment {
        fn isolated(home: &Path) -> Self {
            let names = [
                "CC_SWITCH_TEST_HOME",
                "HOME",
                "OPENCLAW_STATE_DIR",
                "OPENCLAW_PROFILE",
                "OPENCLAW_WORKSPACE_DIR",
                "PR_REVIEW_WORKSPACE",
            ];
            let values = names
                .into_iter()
                .map(|name| (name, std::env::var_os(name)))
                .collect();
            std::env::set_var("CC_SWITCH_TEST_HOME", home);
            std::env::set_var("HOME", home);
            for name in names.into_iter().skip(2) {
                std::env::remove_var(name);
            }
            Self { values }
        }
    }

    impl Drop for TestEnvironment {
        fn drop(&mut self) {
            for (name, value) in self.values.drain(..) {
                match value {
                    Some(value) => std::env::set_var(name, value),
                    None => std::env::remove_var(name),
                }
            }
        }
    }

    fn with_test_paths<T>(source: &str, test: impl FnOnce(&Path) -> T) -> T {
        let _guard = test_guard();
        let temp = tempfile::tempdir().unwrap();
        let openclaw_dir = temp.path().join(".openclaw");
        fs::create_dir_all(&openclaw_dir).unwrap();
        let config_path = openclaw_dir.join("openclaw.json");
        fs::write(&config_path, source).unwrap();
        let _environment = TestEnvironment::isolated(temp.path());
        test(&config_path)
    }

    #[test]
    fn workspace_dir_uses_official_precedence() {
        let home = tempfile::tempdir().unwrap();
        let state = home.path().join(".openclaw");
        let configured = home.path().join("configured");
        let environment = home.path().join("environment");
        let config = json!({"agents": {"defaults": {"workspace": configured}}});
        assert_eq!(
            resolve_openclaw_workspace_dir(&config, &state, home.path(), environment.to_str()),
            configured
        );
        assert_eq!(
            resolve_openclaw_workspace_dir(&json!({}), &state, home.path(), environment.to_str()),
            environment
        );
        assert_eq!(
            resolve_openclaw_workspace_dir(&json!({}), &state, home.path(), None),
            state.join("workspace")
        );
    }

    #[test]
    fn workspace_dir_ignores_unsupported_keys_and_empty_values() {
        let home = tempfile::tempdir().unwrap();
        let state = home.path().join(".openclaw");
        for workspace in [json!(null), json!("  "), json!(123)] {
            let config = json!({
                "agents": {"defaults": {"workspace": workspace}},
                "agent": {"workspace": "wrong-agent-dir"},
                "workspace": {"path": "wrong-legacy-dir"}
            });
            assert_eq!(
                resolve_openclaw_workspace_dir(&config, &state, home.path(), Some("  ")),
                state.join("workspace")
            );
            assert_eq!(
                resolve_openclaw_workspace_dir(&config, &state, home.path(), Some(" ~/from-env ")),
                std::env::current_dir().unwrap().join("~/from-env")
            );
        }
    }

    #[test]
    fn workspace_dir_expands_home_without_rebasing_relative_paths() {
        let home = tempfile::tempdir().unwrap();
        let state = home.path().join(".openclaw");
        for (value, expected) in [
            ("~", home.path().to_path_buf()),
            (" ~/custom ", home.path().join("custom")),
            ("relative-workspace", PathBuf::from("relative-workspace")),
        ] {
            assert_eq!(
                resolve_openclaw_workspace_dir(
                    &json!({"agents": {"defaults": {"workspace": value}}}),
                    &state,
                    home.path(),
                    None
                ),
                expected
            );
        }
    }

    #[test]
    #[serial]
    fn workspace_dir_resolves_env_substitution_and_includes() {
        with_test_paths("{ agents: { $include: 'agents.json5' } }", |config_path| {
            let include_path = config_path.parent().unwrap().join("agents.json5");
            std::fs::write(
                include_path,
                "{ defaults: { workspace: '${PR_REVIEW_WORKSPACE}/agent' } }",
            )
            .unwrap();
            std::env::set_var("PR_REVIEW_WORKSPACE", "~/included-root");
            assert_eq!(
                get_openclaw_workspace_dir().unwrap(),
                crate::config::get_home_dir().join("included-root/agent")
            );
        });
    }

    #[test]
    #[serial]
    fn workspace_dir_reports_missing_env_and_include_errors() {
        with_test_paths(
            "{ agents: { defaults: { workspace: '${MISSING_REVIEW_WORKSPACE}/agent' } } }",
            |_| assert!(get_openclaw_workspace_dir().is_err()),
        );
        with_test_paths("{ agents: { $include: 'missing-agents.json5' } }", |_| {
            assert!(get_openclaw_workspace_dir().is_err())
        });
        with_test_paths(
            "{ env: { token: '${MISSING_UNRELATED_VAR}' }, agents: { defaults: { workspace: '~/safe' } } }",
            |_| {
                assert_eq!(
                    get_openclaw_workspace_dir().unwrap(),
                    crate::config::get_home_dir().join("safe")
                );
            },
        );
    }

    #[test]
    #[serial]
    fn workspace_env_tilde_is_literal_and_resolved_from_cwd() {
        with_test_paths("{}", |_| {
            std::env::set_var("OPENCLAW_WORKSPACE_DIR", "~/literal-workspace");
            let expected = std::env::current_dir().unwrap().join("~/literal-workspace");
            assert_eq!(get_openclaw_workspace_dir().unwrap(), expected);
        });
    }

    #[test]
    fn workspace_dir_profile_changes_the_state_directory() {
        let home = tempfile::tempdir().unwrap();
        for profile in [None, Some(""), Some(" default "), Some("DEFAULT")] {
            assert_eq!(
                resolve_openclaw_dir(home.path(), None, None, profile),
                home.path().join(".openclaw")
            );
        }
        let state = resolve_openclaw_dir(home.path(), None, None, Some(" work "));
        assert_eq!(state, home.path().join(".openclaw-work"));
        assert_eq!(
            resolve_openclaw_workspace_dir(&json!({}), &state, home.path(), None),
            home.path().join(".openclaw-work").join("workspace")
        );
    }

    #[test]
    fn workspace_dir_respects_state_and_settings_overrides() {
        let home = tempfile::tempdir().unwrap();
        let custom = home.path().join("custom-state");
        assert_eq!(
            resolve_openclaw_dir(home.path(), None, Some("~/state"), Some("work")),
            home.path().join("state")
        );
        let state = resolve_openclaw_dir(
            home.path(),
            Some(custom.clone()),
            Some("~/state"),
            Some("work"),
        );
        assert_eq!(state, custom);
        assert_eq!(
            resolve_openclaw_workspace_dir(&json!({}), &state, home.path(), None),
            custom.join("workspace")
        );
    }

    #[test]
    #[serial]
    fn workspace_dir_reads_json5_and_propagates_parse_errors() {
        with_test_paths(
            "{ agents: { defaults: { workspace: '~/custom', }, }, }",
            |_| {
                assert_eq!(
                    get_openclaw_workspace_dir().unwrap(),
                    crate::config::get_home_dir().join("custom")
                );
            },
        );
        with_test_paths("{ invalid config", |_| {
            assert!(get_openclaw_workspace_dir().is_err());
        });
    }

    #[test]
    fn scan_health_detects_known_openclaw_issues() {
        let config = json!({
            "tools": { "profile": "default" },
            "agents": { "defaults": { "timeout": 30 } },
            "env": { "vars": "[object Object]", "shellEnv": "oops" }
        });

        let warnings = scan_openclaw_health_from_value(&config);
        let codes = warnings
            .into_iter()
            .map(|warning| warning.code)
            .collect::<Vec<_>>();
        assert!(codes.contains(&"invalid_tools_profile".to_string()));
        assert!(codes.contains(&"legacy_agents_timeout".to_string()));
        assert!(codes.contains(&"stringified_env_vars".to_string()));
        assert!(codes.contains(&"stringified_env_shell_env".to_string()));
    }

    #[test]
    #[serial]
    fn default_model_write_preserves_top_level_comments() {
        let source = r#"{
  // top-level comment
  models: {
    mode: 'merge',
    providers: {},
  },
}
"#;

        with_test_paths(source, |_| {
            let outcome = set_default_model(&OpenClawDefaultModel {
                primary: "provider/model".to_string(),
                fallbacks: Vec::new(),
                extra: HashMap::new(),
            })
            .unwrap();

            assert!(outcome.backup_path.is_some());

            let written = fs::read_to_string(get_openclaw_config_path()).unwrap();
            assert!(written.contains("// top-level comment"));
            assert!(written.contains("agents: {"));
            assert!(written.contains("provider/model"));
        });
    }

    #[test]
    #[serial]
    fn default_model_noop_write_skips_backup() {
        let source = r#"{
  models: {
    mode: 'merge',
    providers: {},
  },
}
"#;

        with_test_paths(source, |_| {
            let model = OpenClawDefaultModel {
                primary: "provider/model".to_string(),
                fallbacks: vec!["provider/fallback".to_string()],
                extra: HashMap::new(),
            };

            let first_outcome = set_default_model(&model).unwrap();
            assert!(first_outcome.backup_path.is_some());

            let first_written = fs::read_to_string(get_openclaw_config_path()).unwrap();
            let backup_dir = get_app_config_dir().join("backups").join("openclaw");
            let backup_count = fs::read_dir(&backup_dir).unwrap().count();
            assert_eq!(backup_count, 1);

            let second_outcome = set_default_model(&model).unwrap();
            assert!(second_outcome.backup_path.is_none());

            let second_written = fs::read_to_string(get_openclaw_config_path()).unwrap();
            assert_eq!(second_written, first_written);
            assert_eq!(fs::read_dir(&backup_dir).unwrap().count(), backup_count);
        });
    }

    #[test]
    #[serial]
    fn save_detects_external_conflict() {
        let source = r#"{
  models: {
    mode: 'merge',
    providers: {},
  },
}
"#;

        with_test_paths(source, |config_path| {
            let mut document = OpenClawConfigDocument::load().unwrap();
            document
                .set_root_section("env", &json!({ "TOKEN": "value" }))
                .unwrap();

            fs::write(config_path, "{ changedExternally: true }\n").unwrap();
            let err = document.save().unwrap_err();
            assert!(err.to_string().contains("OpenClaw config changed on disk"));
        });
    }

    #[test]
    #[serial]
    fn remove_last_provider_writes_empty_providers_without_panic() {
        let source = r#"{
  models: {
    mode: 'merge',
    providers: {
      '1-copy': {
        api: 'anthropic-messages',
      },
    },
  },
}
"#;

        with_test_paths(source, |_| {
            let outcome = remove_provider("1-copy").unwrap();
            assert!(outcome.backup_path.is_some());

            let config = read_openclaw_config().unwrap();
            let providers = config
                .get("models")
                .and_then(|models| models.get("providers"))
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();

            assert!(providers.is_empty());

            let written = fs::read_to_string(get_openclaw_config_path()).unwrap();
            assert!(written.contains("\"providers\": {}"));
        });
    }
}
