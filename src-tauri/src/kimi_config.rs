//! Kimi Code CLI 配置文件读写模块
//!
//! 处理 `~/.kimi-code/config.toml` 配置文件的读写操作（TOML 格式）。
//! Kimi 使用累加式供应商管理，所有供应商配置共存于同一配置文件中，
//! 切换供应商时更新顶层 `default_model`。
//!
//! ## 配置结构示例
//!
//! ```toml
//! default_model = "kimi-k2.7-code"
//!
//! [providers.kimi]
//! type = "openai"
//! base_url = "https://api.moonshot.cn/v1"
//! api_key = "sk-xxx"
//!
//! [models."kimi-k2.7-code"]
//! provider = "kimi"
//! model = "kimi-k2.7-code"
//! max_context_size = 262144
//! display_name = "Kimi K2.7 Code"
//! capabilities = ["thinking", "tool_use"]
//! ```
//!
//! 参考文档：<https://www.kimi.com/code/docs/en/kimi-code-cli/configuration/config-files.html>

use crate::config::atomic_write;
use crate::error::AppError;
use crate::settings::get_kimi_override_dir;
use serde_json::{Map, Value};
use std::fs;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use toml_edit::{Array, DocumentMut, Item, Table, Value as TomlValue};

// ============================================================================
// Path Functions
// ============================================================================

/// 获取 Kimi 配置目录
///
/// 解析顺序对齐 Kimi 自身的 `KIMI_CODE_HOME`：
///   1. CCS 设置 `kimi_config_dir`（显式覆盖）
///   2. `KIMI_CODE_HOME` 环境变量（trim 后非空；按原样，不展开 `~`）
///   3. 平台默认 `~/.kimi-code`
pub fn get_kimi_dir() -> PathBuf {
    if let Some(override_dir) = get_kimi_override_dir() {
        return override_dir;
    }

    if let Some(raw) = std::env::var_os("KIMI_CODE_HOME") {
        let value = raw.to_string_lossy();
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }

    crate::config::get_home_dir().join(".kimi-code")
}

/// 获取 Kimi 配置文件路径（`~/.kimi-code/config.toml`）
pub fn get_kimi_config_path() -> PathBuf {
    get_kimi_dir().join("config.toml")
}

/// 获取 Kimi 用户级 MCP 文件路径（`~/.kimi-code/mcp.json`）
pub fn get_kimi_mcp_path() -> PathBuf {
    get_kimi_dir().join("mcp.json")
}

fn kimi_write_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

// ============================================================================
// Core TOML Read / Write
// ============================================================================

/// 读取 Kimi 配置文件为 `toml_edit::DocumentMut`
///
/// 如果文件不存在或为空，返回空文档。使用 `toml_edit` 做字段级编辑，
/// 保留文件中所有非托管段（thinking / loop_control / hooks / services 等）。
pub fn read_kimi_config() -> Result<DocumentMut, AppError> {
    let path = get_kimi_config_path();
    if !path.exists() {
        return Ok(DocumentMut::new());
    }

    let content = fs::read_to_string(&path).map_err(|e| AppError::io(&path, e))?;
    if content.trim().is_empty() {
        return Ok(DocumentMut::new());
    }

    content
        .parse::<DocumentMut>()
        .map_err(|e| AppError::Config(format!("Failed to parse Kimi config as TOML: {e}")))
}

/// 原子写入 Kimi 配置文件
pub fn write_kimi_config(doc: &DocumentMut) -> Result<(), AppError> {
    let path = get_kimi_config_path();
    let content = doc.to_string();
    atomic_write(&path, content.as_bytes())
}

// ============================================================================
// Provider / Model Functions
// ============================================================================

/// 读取所有 provider，返回以 provider 名为键的 settings_config 映射。
///
/// 每个 provider 的 settings_config 形态（与前端 `kimiProviderPresets.ts` 一致）：
/// ```json
/// {
///   "name": "kimi",
///   "type": "openai",
///   "base_url": "https://api.moonshot.cn/v1",
///   "api_key": "sk-xxx",
///   "models": [{ "id": "...", "name": "...", "max_context_size": 262144, ... }],
///   "default_model": "kimi-k2.7-code"
/// }
/// ```
pub fn get_providers() -> Result<Map<String, Value>, AppError> {
    let doc = read_kimi_config()?;
    let mut map = Map::new();

    let Some(providers) = doc.get("providers").and_then(Item::as_table) else {
        return Ok(map);
    };

    for (name, item) in providers.iter() {
        let Some(table) = item.as_table_like() else {
            continue;
        };

        let mut settings = Map::new();
        settings.insert("name".to_string(), Value::String(name.to_string()));
        for field in ["type", "base_url", "api_key"] {
            if let Some(value) = table.get(field).and_then(item_to_json) {
                settings.insert(field.to_string(), value);
            }
        }

        let models = collect_models_for_provider(&doc, name);
        if !models.is_empty() {
            settings.insert("models".to_string(), Value::Array(models.clone()));
            // 顶层 default_model 若属于该 provider，则一并回填
            let default_model = doc
                .get("default_model")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            if let Some(dm) = default_model {
                if models
                    .iter()
                    .any(|m| m.get("id").and_then(|i| i.as_str()) == Some(dm.as_str()))
                {
                    settings.insert("default_model".to_string(), Value::String(dm));
                }
            }
        }

        map.insert(name.to_string(), Value::Object(settings));
    }

    Ok(map)
}

/// 获取单个 provider 的 settings_config。
pub fn get_provider(name: &str) -> Result<Option<Value>, AppError> {
    Ok(get_providers()?.get(name).cloned())
}

/// 读取顶层 `default_model`（模型别名）。
pub fn get_default_model() -> Result<Option<String>, AppError> {
    let doc = read_kimi_config()?;
    Ok(doc
        .get("default_model")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string()))
}

/// Upsert 一个 Kimi provider。
///
/// - 写入 `[providers.<name>]`（type / base_url / api_key，保留 env 等既有字段）
/// - 重建该 provider 名下的 `[models."<alias>"]` 条目（先删除旧的再写入新的）
/// - 更新顶层 `default_model`：优先取 settings.default_model，缺失时取首个模型 id
///
/// 整个读-改-写在写锁内进行，防止 TOCTOU 竞态。
pub fn set_provider(name: &str, settings: Value) -> Result<(), AppError> {
    let _guard = kimi_write_lock().lock()?;

    let name = name.trim();
    if name.is_empty() {
        return Err(AppError::Config(
            "Kimi provider name cannot be empty".to_string(),
        ));
    }

    let provider_type = settings
        .get("type")
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .unwrap_or("openai")
        .to_string();
    let base_url = settings
        .get("base_url")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let api_key = settings
        .get("api_key")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let models = settings
        .get("models")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let default_model = settings
        .get("default_model")
        .and_then(Value::as_str)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let mut doc = read_kimi_config()?;

    // 写入 [providers.<name>]
    {
        let root = doc.as_table_mut();
        let providers = root.entry("providers").or_insert(Item::Table(Table::new()));
        let providers_t = providers.as_table_like_mut().ok_or_else(|| {
            AppError::Config("Kimi config 'providers' is not a table".to_string())
        })?;
        let entry = providers_t.entry(name).or_insert(Item::Table(Table::new()));
        let entry_t = entry
            .as_table_like_mut()
            .ok_or_else(|| AppError::Config(format!("Kimi provider '{name}' is not a table")))?;
        entry_t.insert("type", Item::Value(provider_type.into()));
        entry_t.insert("base_url", Item::Value(base_url.into()));
        entry_t.insert("api_key", Item::Value(api_key.into()));
    }

    // 重建 [models."<alias>"]：先收集该 provider 名下的旧别名，再写入新的
    {
        let mut to_remove = Vec::new();
        if let Some(models_t) = doc.get("models").and_then(Item::as_table) {
            for (alias, item) in models_t.iter() {
                let provider_of = item
                    .as_table_like()
                    .and_then(|t| t.get("provider"))
                    .and_then(|v| v.as_str());
                if provider_of == Some(name) {
                    to_remove.push(alias.to_string());
                }
            }
        }

        let root = doc.as_table_mut();
        let models_t = root
            .entry("models")
            .or_insert(Item::Table(Table::new()))
            .as_table_like_mut()
            .ok_or_else(|| AppError::Config("Kimi config 'models' is not a table".to_string()))?;

        for alias in &to_remove {
            models_t.remove(alias);
        }

        for model in &models {
            let Some(id) = model.get("id").and_then(Value::as_str) else {
                continue;
            };
            let id = id.trim();
            if id.is_empty() {
                continue;
            }
            let wire_model = model
                .get("model")
                .and_then(Value::as_str)
                .filter(|s| !s.trim().is_empty())
                .unwrap_or(id)
                .to_string();

            let entry = models_t.entry(id).or_insert(Item::Table(Table::new()));
            let entry_t = entry
                .as_table_like_mut()
                .ok_or_else(|| AppError::Config(format!("Kimi model '{id}' is not a table")))?;
            entry_t.insert("provider", Item::Value(name.to_string().into()));
            entry_t.insert("model", Item::Value(wire_model.clone().into()));

            if let Some(ctx) = model.get("max_context_size").and_then(Value::as_u64) {
                entry_t.insert("max_context_size", Item::Value((ctx as i64).into()));
            }
            if let Some(display) = model.get("name").and_then(Value::as_str) {
                let display = display.trim();
                if !display.is_empty() && display != wire_model {
                    entry_t.insert("display_name", Item::Value(display.to_string().into()));
                }
            }
            if let Some(caps) = model.get("capabilities").and_then(Value::as_array) {
                let mut arr = Array::new();
                for cap in caps {
                    if let Some(s) = cap.as_str() {
                        arr.push(TomlValue::from(s.to_string()));
                    }
                }
                if !arr.is_empty() {
                    entry_t.insert("capabilities", Item::Value(TomlValue::Array(arr)));
                }
            }
        }
    }

    // 更新顶层 default_model
    let effective_default = default_model.or_else(|| {
        models
            .first()
            .and_then(|m| m.get("id"))
            .and_then(Value::as_str)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    });
    if let Some(dm) = effective_default {
        doc.as_table_mut()
            .insert("default_model", Item::Value(dm.into()));
    }

    write_kimi_config(&doc)
}

/// 删除一个 Kimi provider。
///
/// 同时删除 `[providers.<name>]` 与其名下的 `[models."<alias>"]` 条目。
/// 若被删 provider 持有当前 `default_model`，回退到剩余第一个模型 id；
/// 无剩余模型时移除 `default_model`。
pub fn remove_provider(name: &str) -> Result<(), AppError> {
    let _guard = kimi_write_lock().lock()?;

    let mut doc = read_kimi_config()?;

    // 删除 [providers.<name>]
    let removed = {
        let root = doc.as_table_mut();
        match root.get_mut("providers").and_then(Item::as_table_like_mut) {
            Some(providers) => providers.remove(name).is_some(),
            None => false,
        }
    };

    // 删除该 provider 名下的 models
    let mut removed_models = Vec::new();
    if let Some(models_t) = doc.get("models").and_then(Item::as_table) {
        for (alias, item) in models_t.iter() {
            let provider_of = item
                .as_table_like()
                .and_then(|t| t.get("provider"))
                .and_then(|v| v.as_str());
            if provider_of == Some(name) {
                removed_models.push(alias.to_string());
            }
        }
    }
    if !removed_models.is_empty() {
        let root = doc.as_table_mut();
        if let Some(models_t) = root.get_mut("models").and_then(Item::as_table_like_mut) {
            for alias in &removed_models {
                models_t.remove(alias);
            }
        }
    }

    // default_model 回退
    let default_removed = doc
        .get("default_model")
        .and_then(|v| v.as_str())
        .map(|dm| removed_models.iter().any(|a| a == dm))
        .unwrap_or(false);
    if default_removed {
        let fallback = first_remaining_model_id(&doc);
        let root = doc.as_table_mut();
        match fallback {
            Some(fb) => {
                root.insert("default_model", Item::Value(fb.into()));
            }
            None => {
                root.remove("default_model");
            }
        }
    }

    if removed {
        write_kimi_config(&doc)?;
    }
    Ok(())
}

/// 切换 provider 时更新顶层 `default_model`。
///
/// `default_model` 仅在设置中存在有效值（或 models 非空时取首个模型 id）
/// 才被覆盖；否则保留现有值，避免切到空配置时破坏运行状态。
pub fn apply_switch_defaults(provider_id: &str, settings_config: &Value) -> Result<(), AppError> {
    let default_model = settings_config
        .get("default_model")
        .and_then(Value::as_str)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| {
            settings_config
                .get("models")
                .and_then(Value::as_array)
                .and_then(|arr| arr.first())
                .and_then(|m| m.get("id"))
                .and_then(Value::as_str)
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        });

    if let Some(dm) = default_model {
        let mut doc = read_kimi_config()?;
        doc.as_table_mut()
            .insert("default_model", Item::Value(dm.into()));
        write_kimi_config(&doc)?;
    }
    let _ = provider_id; // 目前仅用于语义一致性；default_model 已足够
    Ok(())
}

// ============================================================================
// MCP File Access（供 mcp/kimi.rs 使用）
// ============================================================================

/// 读取 `mcp.json` 中的 mcpServers 映射（统一 MCP 结构）。
pub fn get_mcp_servers_json() -> Result<Map<String, Value>, AppError> {
    let path = get_kimi_mcp_path();
    if !path.exists() {
        return Ok(Map::new());
    }
    let content = fs::read_to_string(&path).map_err(|e| AppError::io(&path, e))?;
    if content.trim().is_empty() {
        return Ok(Map::new());
    }
    let root: Value = serde_json::from_str(&content).map_err(|e| AppError::json(&path, e))?;
    Ok(root
        .get("mcpServers")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default())
}

/// 原子地读-改-写 `mcp.json` 的 mcpServers 映射（写锁内）。
///
/// 只覆盖 `mcpServers` 字段，保留文件中其他顶层字段。
pub fn update_mcp_servers_json<F>(updater: F) -> Result<(), AppError>
where
    F: FnOnce(&mut Map<String, Value>) -> Result<(), AppError>,
{
    let _guard = kimi_write_lock().lock()?;

    let path = get_kimi_mcp_path();
    let mut root: Value = if path.exists() {
        let content = fs::read_to_string(&path).map_err(|e| AppError::io(&path, e))?;
        if content.trim().is_empty() {
            Value::Object(Map::new())
        } else {
            serde_json::from_str(&content).map_err(|e| AppError::json(&path, e))?
        }
    } else {
        Value::Object(Map::new())
    };

    let mut servers = root
        .get("mcpServers")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    updater(&mut servers)?;

    if let Some(obj) = root.as_object_mut() {
        obj.insert("mcpServers".to_string(), Value::Object(servers));
    }

    let json =
        serde_json::to_string_pretty(&root).map_err(|e| AppError::JsonSerialize { source: e })?;
    atomic_write(&path, json.as_bytes())
}

// ============================================================================
// Validation & Credential Extraction
// ============================================================================

/// 校验 Kimi provider 的 settings_config 结构。
pub fn validate_kimi_settings(settings: &Value) -> Result<(), AppError> {
    let obj = settings
        .as_object()
        .ok_or_else(|| AppError::Config("Kimi provider settings must be an object".to_string()))?;

    if let Some(name) = obj.get("name").and_then(Value::as_str) {
        if name.trim().is_empty() {
            return Err(AppError::Config(
                "Kimi provider name cannot be empty".to_string(),
            ));
        }
    }

    // type 可选，缺省按 openai 处理；非空时必须是已知协议之一
    if let Some(ty) = obj.get("type").and_then(Value::as_str) {
        const KNOWN_TYPES: &[&str] = &[
            "kimi",
            "anthropic",
            "openai",
            "openai_responses",
            "google-genai",
            "vertexai",
        ];
        if !KNOWN_TYPES.contains(&ty) {
            return Err(AppError::Config(format!(
                "Unsupported Kimi provider type '{ty}' (expected one of {})",
                KNOWN_TYPES.join(", ")
            )));
        }
    }

    if let Some(models) = obj.get("models") {
        if !models.is_array() {
            return Err(AppError::Config(
                "Kimi provider models must be an array".to_string(),
            ));
        }
    }

    Ok(())
}

/// 提取 (base_url, api_key)（settings_config 顶层扁平键）。
pub fn extract_kimi_credentials(settings: &Value) -> (String, String) {
    let base_url = settings
        .get("base_url")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let api_key = settings
        .get("api_key")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    (base_url, api_key)
}

// ============================================================================
// Internal Helpers
// ============================================================================

/// 收集指定 provider 名下的模型列表（settings_config 的 models 数组形态）。
fn collect_models_for_provider(doc: &DocumentMut, provider: &str) -> Vec<Value> {
    let mut models = Vec::new();
    let Some(models_table) = doc.get("models").and_then(Item::as_table) else {
        return models;
    };

    for (alias, item) in models_table.iter() {
        let Some(table) = item.as_table_like() else {
            continue;
        };
        let provider_of = table.get("provider").and_then(|v| v.as_str()).unwrap_or("");
        if provider_of != provider {
            continue;
        }

        let mut model = Map::new();
        model.insert("id".to_string(), Value::String(alias.to_string()));
        if let Some(wire) = table.get("model").and_then(|v| v.as_str()) {
            model.insert("model".to_string(), Value::String(wire.to_string()));
        }
        if let Some(ctx) = table.get("max_context_size").and_then(|v| v.as_integer()) {
            model.insert("max_context_size".to_string(), Value::Number(ctx.into()));
        }
        if let Some(display) = table.get("display_name").and_then(|v| v.as_str()) {
            model.insert("name".to_string(), Value::String(display.to_string()));
        }
        if let Some(caps) = table.get("capabilities").and_then(|v| v.as_array()) {
            let arr: Vec<Value> = caps
                .iter()
                .filter_map(|c| c.as_str().map(|s| Value::String(s.to_string())))
                .collect();
            if !arr.is_empty() {
                model.insert("capabilities".to_string(), Value::Array(arr));
            }
        }
        models.push(Value::Object(model));
    }

    models
}

/// 返回当前 config.toml 中剩余的、带有 models 条目的第一个模型别名。
fn first_remaining_model_id(doc: &DocumentMut) -> Option<String> {
    doc.get("models")
        .and_then(Item::as_table)
        .map(|t| t.iter().next().map(|(alias, _)| alias.to_string()))
        .flatten()
}

/// 将 toml_edit 的 Item 转为 serde_json::Value（仅支持标量）。
fn item_to_json(item: &Item) -> Option<Value> {
    if let Some(s) = item.as_str() {
        Some(Value::String(s.to_string()))
    } else if let Some(i) = item.as_integer() {
        Some(Value::Number(i.into()))
    } else if let Some(f) = item.as_float() {
        serde_json::Number::from_f64(f).map(Value::Number)
    } else if let Some(b) = item.as_bool() {
        Some(Value::Bool(b))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use std::env;
    use tempfile::TempDir;

    struct TempKimiHome {
        _dir: TempDir,
        original_home: Option<String>,
        original_kimi_home: Option<String>,
    }

    impl TempKimiHome {
        fn new() -> Self {
            let dir = TempDir::new().expect("temp dir");
            let original_home = env::var("HOME").ok();
            let original_kimi_home = env::var("KIMI_CODE_HOME").ok();
            // 指向临时目录，避免读到真实 ~/.cc-switch/settings.json 的覆盖配置
            env::set_var("HOME", dir.path());
            env::set_var("KIMI_CODE_HOME", dir.path().join("kimi-home"));
            Self {
                _dir: dir,
                original_home,
                original_kimi_home,
            }
        }
    }

    impl Drop for TempKimiHome {
        fn drop(&mut self) {
            match &self.original_home {
                Some(v) => env::set_var("HOME", v),
                None => env::remove_var("HOME"),
            }
            match &self.original_kimi_home {
                Some(v) => env::set_var("KIMI_CODE_HOME", v),
                None => env::remove_var("KIMI_CODE_HOME"),
            }
        }
    }

    fn sample_settings() -> Value {
        serde_json::json!({
            "name": "kimi",
            "type": "openai",
            "base_url": "https://api.moonshot.cn/v1",
            "api_key": "sk-test-123",
            "models": [
                {
                    "id": "kimi-k2.7-code",
                    "name": "Kimi K2.7 Code",
                    "max_context_size": 262144,
                    "capabilities": ["thinking", "tool_use"]
                },
                {
                    "id": "kimi-k3",
                    "name": "Kimi K3",
                    "max_context_size": 1048576,
                    "capabilities": ["thinking", "always_thinking", "tool_use"]
                }
            ],
            "default_model": "kimi-k2.7-code"
        })
    }

    #[test]
    #[serial]
    fn set_and_get_provider_roundtrip() {
        let _home = TempKimiHome::new();

        set_provider("kimi", sample_settings()).expect("set provider");

        let providers = get_providers().expect("get providers");
        let provider = providers.get("kimi").expect("provider exists");
        assert_eq!(provider["type"], "openai");
        assert_eq!(provider["base_url"], "https://api.moonshot.cn/v1");
        assert_eq!(provider["api_key"], "sk-test-123");
        assert_eq!(provider["default_model"], "kimi-k2.7-code");

        let models = provider["models"].as_array().expect("models array");
        assert_eq!(models.len(), 2);
        assert_eq!(models[0]["id"], "kimi-k2.7-code");
        assert_eq!(models[0]["max_context_size"], 262144);
        assert_eq!(models[1]["id"], "kimi-k3");
        assert_eq!(models[1]["max_context_size"], 1048576);

        assert_eq!(
            get_default_model().expect("default model"),
            Some("kimi-k2.7-code".to_string())
        );
    }

    #[test]
    #[serial]
    fn preserves_unrelated_sections() {
        let _home = TempKimiHome::new();

        // 预置一段包含无关段的 config.toml
        let dir = get_kimi_dir();
        std::fs::create_dir_all(&dir).expect("create dir");
        std::fs::write(
            get_kimi_config_path(),
            r#"default_model = "pre-existing"

[thinking]
enabled = true
effort = "high"
keep = "all"

[[hooks]]
event = "PreToolUse"
command = "echo hello"
"#,
        )
        .expect("write config");

        set_provider("kimi", sample_settings()).expect("set provider");

        let content = std::fs::read_to_string(get_kimi_config_path()).expect("read back");
        assert!(
            content.contains("[thinking]"),
            "thinking section must be preserved"
        );
        assert!(content.contains("effort = \"high\""));
        assert!(
            content.contains("[[hooks]]"),
            "hooks section must be preserved"
        );
        assert!(content.contains("command = \"echo hello\""));
        assert!(content.contains("kimi-k2.7-code"), "new model written");

        // default_model 应被新 provider 覆盖
        assert!(content.contains("default_model = \"kimi-k2.7-code\""));
    }

    #[test]
    #[serial]
    fn remove_provider_rolls_back_default_model() {
        let _home = TempKimiHome::new();

        set_provider("kimi", sample_settings()).expect("set provider");

        // 再写入一个 provider 作为回退目标
        let second = serde_json::json!({
            "name": "other",
            "type": "openai",
            "base_url": "https://example.com/v1",
            "api_key": "",
            "models": [{"id": "other-model", "name": "Other", "max_context_size": 65536}],
            "default_model": "other-model"
        });
        set_provider("other", second).expect("set second provider");
        // 切回 kimi 为当前
        apply_switch_defaults("kimi", &sample_settings()).expect("switch");

        assert_eq!(
            get_default_model().expect("default"),
            Some("kimi-k2.7-code".to_string())
        );

        // 删除 kimi → default_model 应回退到 other-model
        remove_provider("kimi").expect("remove kimi");
        assert_eq!(
            get_default_model().expect("default"),
            Some("other-model".to_string())
        );

        let providers = get_providers().expect("providers");
        assert!(!providers.contains_key("kimi"));
        assert!(providers.contains_key("other"));
    }

    #[test]
    #[serial]
    fn remove_last_provider_clears_default_model() {
        let _home = TempKimiHome::new();

        set_provider("kimi", sample_settings()).expect("set provider");
        remove_provider("kimi").expect("remove");

        assert_eq!(get_default_model().expect("default"), None);
        assert!(get_providers().expect("providers").is_empty());
    }

    #[test]
    #[serial]
    fn mcp_json_read_write_preserves_other_fields() {
        let _home = TempKimiHome::new();

        let dir = get_kimi_dir();
        std::fs::create_dir_all(&dir).expect("create dir");
        std::fs::write(
            get_kimi_mcp_path(),
            r#"{"mcpServers": {"filesystem": {"command": "npx"}}, "custom": 1}"#,
        )
        .expect("write mcp");

        update_mcp_servers_json(|servers| {
            servers.insert(
                "fetch".to_string(),
                serde_json::json!({"command": "uvx", "args": ["mcp-server-fetch"]}),
            );
            Ok(())
        })
        .expect("update mcp");

        let content = std::fs::read_to_string(get_kimi_mcp_path()).expect("read back mcp json");
        let root: Value = serde_json::from_str(&content).expect("parse mcp json");
        assert_eq!(root["custom"], 1, "unrelated top-level fields preserved");
        assert!(root["mcpServers"]["filesystem"].is_object());
        assert!(root["mcpServers"]["fetch"].is_object());
    }

    #[test]
    #[serial]
    fn validate_kimi_settings_rejects_unknown_type() {
        let bad = serde_json::json!({ "name": "x", "type": "nonsense" });
        assert!(validate_kimi_settings(&bad).is_err());

        let good = serde_json::json!({ "name": "x", "type": "openai", "models": [] });
        assert!(validate_kimi_settings(&good).is_ok());
    }

    #[test]
    #[serial]
    fn kimi_home_env_controls_config_path() {
        let _home = TempKimiHome::new();
        let expected = env::var("KIMI_CODE_HOME").expect("env set");
        assert_eq!(
            get_kimi_dir().to_string_lossy(),
            PathBuf::from(expected).to_string_lossy()
        );
        assert_eq!(
            get_kimi_config_path().file_name().and_then(|s| s.to_str()),
            Some("config.toml")
        );
    }
}
