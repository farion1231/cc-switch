use std::cmp::Ordering;
use std::collections::HashSet;
use std::io::{ErrorKind, Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::config::{
    atomic_write, delete_file, get_home_dir, read_json_file, sanitize_provider_name,
    write_json_file, write_text_file,
};
use crate::error::AppError;
use serde_json::{json, Value};
use std::fs;
use std::process::Command;
use toml_edit::DocumentMut;
use tungstenite::Message;

pub const CC_SWITCH_CODEX_MODEL_PROVIDER_ID: &str = "custom";
pub const CC_SWITCH_CODEX_MODEL_CATALOG_FILENAME: &str = "cc-switch-model-catalog.json";
const CODEX_MODEL_CATALOG_TEMPLATE_SLUG: &str = "gpt-5.5";
// Codex Desktop currently filters catalog entries through this Statsig dynamic
// config before rendering the model picker.
const CODEX_DESKTOP_STATSIG_MODELS_CONFIG_ID: &str = "107580212";
const CODEX_DESKTOP_STATSIG_CACHE_KEY_MARKER: &str = "statsig.cached.evaluations";
const CODEX_DESKTOP_STATSIG_LAST_MODIFIED_KEY_MARKER: &str =
    "statsig.last_modified_time.evaluations";
const CODEX_DESKTOP_MODEL_WHITELIST_RETRY_INTERVAL: Duration = Duration::from_secs(5);
// Codex Desktop may refresh Statsig with a newer default-only Network cache.
// Keep patched caches preferred long enough for the active CC Switch provider.
const CODEX_DESKTOP_STATSIG_PIN_HORIZON: Duration = Duration::from_secs(30 * 24 * 60 * 60);
const CODEX_DESKTOP_STATSIG_PIN_REFRESH_AFTER: Duration = Duration::from_secs(7 * 24 * 60 * 60);
const CODEX_DESKTOP_REMOTE_DEBUGGING_PORT: u16 = 8315;
const CODEX_DESKTOP_DEVTOOLS_TIMEOUT: Duration = Duration::from_millis(900);

#[derive(Debug, Default)]
struct CodexDesktopModelWhitelistSyncState {
    model_ids: Vec<String>,
    generation: u64,
}

type CodexDesktopModelWhitelistSyncHandle =
    Arc<(Mutex<CodexDesktopModelWhitelistSyncState>, Condvar)>;

static CODEX_DESKTOP_MODEL_WHITELIST_SYNC: OnceLock<CodexDesktopModelWhitelistSyncHandle> =
    OnceLock::new();

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CodexDesktopStatsigWrapperEncoding {
    Utf8,
    Utf16Le,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CodexDesktopRuntimeWhitelistPatchResult {
    changed: bool,
    active_model_count: usize,
    patched_value_count: usize,
    storage_cache_count: usize,
}

/// Reserved built-in provider IDs from OpenAI Codex's config/model-provider
/// catalog. Keep in sync with Codex `RESERVED_MODEL_PROVIDER_IDS` and legacy
/// removed provider aliases.
const CODEX_RESERVED_MODEL_PROVIDER_IDS: &[&str] = &[
    "amazon-bedrock",
    "openai",
    "ollama",
    "lmstudio",
    "oss",
    "ollama-chat",
];

/// 获取 Codex 配置目录路径
pub fn get_codex_config_dir() -> PathBuf {
    if let Some(custom) = crate::settings::get_codex_override_dir() {
        return custom;
    }

    get_home_dir().join(".codex")
}

/// 获取 Codex auth.json 路径
pub fn get_codex_auth_path() -> PathBuf {
    get_codex_config_dir().join("auth.json")
}

/// 获取 Codex config.toml 路径
pub fn get_codex_config_path() -> PathBuf {
    get_codex_config_dir().join("config.toml")
}

pub fn get_codex_model_catalog_path() -> PathBuf {
    get_codex_config_dir().join(CC_SWITCH_CODEX_MODEL_CATALOG_FILENAME)
}

/// 获取 Codex 供应商配置文件路径
#[allow(dead_code)]
pub fn get_codex_provider_paths(
    provider_id: &str,
    provider_name: Option<&str>,
) -> (PathBuf, PathBuf) {
    let base_name = provider_name
        .map(sanitize_provider_name)
        .unwrap_or_else(|| sanitize_provider_name(provider_id));

    let auth_path = get_codex_config_dir().join(format!("auth-{base_name}.json"));
    let config_path = get_codex_config_dir().join(format!("config-{base_name}.toml"));

    (auth_path, config_path)
}

/// 删除 Codex 供应商配置文件
#[allow(dead_code)]
pub fn delete_codex_provider_config(
    provider_id: &str,
    provider_name: &str,
) -> Result<(), AppError> {
    let (auth_path, config_path) = get_codex_provider_paths(provider_id, Some(provider_name));

    delete_file(&auth_path).ok();
    delete_file(&config_path).ok();

    Ok(())
}

/// 原子写 Codex 的 `auth.json` 与 `config.toml`，在第二步失败时回滚第一步
pub fn write_codex_live_atomic(
    auth: &Value,
    config_text_opt: Option<&str>,
) -> Result<(), AppError> {
    let auth_path = get_codex_auth_path();
    let config_path = get_codex_config_path();

    if let Some(parent) = auth_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
    }

    // 读取旧内容用于回滚
    let old_auth = if auth_path.exists() {
        Some(fs::read(&auth_path).map_err(|e| AppError::io(&auth_path, e))?)
    } else {
        None
    };
    let _old_config = if config_path.exists() {
        Some(fs::read(&config_path).map_err(|e| AppError::io(&config_path, e))?)
    } else {
        None
    };

    // 准备写入内容
    let cfg_text = match config_text_opt {
        Some(s) => s.to_string(),
        None => String::new(),
    };
    if !cfg_text.trim().is_empty() {
        toml::from_str::<toml::Table>(&cfg_text).map_err(|e| AppError::toml(&config_path, e))?;
    }

    // 第一步：写 auth.json
    write_json_file(&auth_path, auth)?;

    // 第二步：写 config.toml（失败则回滚 auth.json）
    if let Err(e) = write_text_file(&config_path, &cfg_text) {
        // 回滚 auth.json
        if let Some(bytes) = old_auth {
            let _ = atomic_write(&auth_path, &bytes);
        } else {
            let _ = delete_file(&auth_path);
        }
        return Err(e);
    }

    Ok(())
}

/// 读取 `~/.codex/config.toml`，若不存在返回空字符串
pub fn read_codex_config_text() -> Result<String, AppError> {
    let path = get_codex_config_path();
    if path.exists() {
        std::fs::read_to_string(&path).map_err(|e| AppError::io(&path, e))
    } else {
        Ok(String::new())
    }
}

/// 对非空的 TOML 文本进行语法校验
pub fn validate_config_toml(text: &str) -> Result<(), AppError> {
    if text.trim().is_empty() {
        return Ok(());
    }
    toml::from_str::<toml::Table>(text)
        .map(|_| ())
        .map_err(|e| AppError::toml(Path::new("config.toml"), e))
}

/// 读取并校验 `~/.codex/config.toml`，返回文本（可能为空）
pub fn read_and_validate_codex_config_text() -> Result<String, AppError> {
    let s = read_codex_config_text()?;
    validate_config_toml(&s)?;
    Ok(s)
}

fn active_codex_model_provider_id(doc: &DocumentMut) -> Option<String> {
    doc.get("model_provider")
        .and_then(|item| item.as_str())
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(str::to_string)
}

pub(crate) fn is_custom_codex_model_provider_id(id: &str) -> bool {
    let id = id.trim();
    !id.is_empty()
        && !CODEX_RESERVED_MODEL_PROVIDER_IDS
            .iter()
            .any(|reserved| reserved.eq_ignore_ascii_case(id))
}

/// Write only Codex `config.toml` for provider switching.
///
/// Codex login state lives in `auth.json`; provider routing, endpoint, model,
/// and provider-scoped bearer tokens live in `config.toml`. Provider switches
/// should not overwrite the user's ChatGPT login cache.
pub fn write_codex_live_config_atomic(config_text_opt: Option<&str>) -> Result<(), AppError> {
    let config_path = get_codex_config_path();
    let cfg_text = match config_text_opt {
        Some(config_text) => config_text.to_string(),
        None => String::new(),
    };

    if !cfg_text.trim().is_empty() {
        toml::from_str::<toml::Table>(&cfg_text).map_err(|e| AppError::toml(&config_path, e))?;
    }

    write_text_file(&config_path, &cfg_text)
}

pub fn extract_codex_auth_api_key(auth: &Value) -> Option<String> {
    auth.get("OPENAI_API_KEY")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|key| !key.is_empty())
        .map(str::to_string)
}

pub fn extract_codex_api_key(auth: Option<&Value>, config_text: Option<&str>) -> Option<String> {
    auth.and_then(extract_codex_auth_api_key)
        .or_else(|| config_text.and_then(extract_codex_experimental_bearer_token))
}

/// Extract the upstream base URL from a Codex `config.toml` string.
///
/// Prefers the active `[model_providers.<model_provider>].base_url`, falling
/// back to a top-level `base_url`. Deliberately never reads a non-active
/// `[model_providers.*]` section — the frontend `extractCodexBaseUrl`
/// (`getRecoverableBaseUrlAssignments`) excludes those too, and a leftover
/// section unrelated to the active provider must not leak into `{{baseUrl}}`.
pub fn extract_codex_base_url(config_text: &str) -> Option<String> {
    let doc = config_text.parse::<toml::Value>().ok()?;

    if let Some(active_provider) = doc.get("model_provider").and_then(|v| v.as_str()) {
        if let Some(base_url) = doc
            .get("model_providers")
            .and_then(|providers| providers.get(active_provider))
            .and_then(|provider| provider.get("base_url"))
            .and_then(|v| v.as_str())
        {
            return Some(base_url.to_string());
        }
    }

    doc.get("base_url")
        .and_then(|v| v.as_str())
        .map(ToString::to_string)
}

pub fn codex_auth_has_login_material(auth: &Value) -> bool {
    let Some(obj) = auth.as_object() else {
        return false;
    };

    obj.iter().any(|(key, value)| {
        if key == "auth_mode" {
            return false;
        }

        if key == "OPENAI_API_KEY" {
            return value
                .as_str()
                .map(str::trim)
                .is_some_and(|token| !token.is_empty());
        }

        match value {
            Value::Null => false,
            Value::String(text) => !text.trim().is_empty(),
            Value::Array(items) => !items.is_empty(),
            Value::Object(map) => !map.is_empty(),
            _ => true,
        }
    })
}

pub fn codex_auth_has_oauth_login_material(auth: &Value) -> bool {
    let Some(obj) = auth.as_object() else {
        return false;
    };

    obj.iter().any(|(key, value)| {
        if key == "auth_mode" || key == "OPENAI_API_KEY" {
            return false;
        }

        match value {
            Value::Null => false,
            Value::String(text) => !text.trim().is_empty(),
            Value::Array(items) => !items.is_empty(),
            Value::Object(map) => !map.is_empty(),
            _ => true,
        }
    })
}

pub fn should_restore_codex_provider_token_for_backfill(
    category: Option<&str>,
    template_settings: &Value,
) -> bool {
    if category == Some("official") {
        return false;
    }

    let Some(auth) = template_settings.get("auth") else {
        return true;
    };

    let has_provider_api_key = extract_codex_auth_api_key(auth).is_some();
    let has_oauth_login = codex_auth_has_oauth_login_material(auth);
    !has_oauth_login || has_provider_api_key
}

fn parse_codex_positive_u64(value: Option<&Value>) -> Option<u64> {
    match value {
        Some(Value::Number(n)) => n.as_u64().filter(|v| *v > 0),
        Some(Value::String(s)) => s.trim().parse::<u64>().ok().filter(|v| *v > 0),
        _ => None,
    }
}

fn extract_codex_top_level_u64(config_text: &str, field: &str) -> Option<u64> {
    let doc = config_text.parse::<toml::Value>().ok()?;
    doc.get(field)
        .and_then(|value| value.as_integer())
        .and_then(|value| u64::try_from(value).ok())
        .filter(|value| *value > 0)
}

fn codex_catalog_model_entry(
    template: &Value,
    provider_id: &str,
    model: &str,
    display_name: &str,
    context_window: u64,
    priority: usize,
) -> Value {
    let mut entry = template.clone();
    let Some(entry_obj) = entry.as_object_mut() else {
        return json!({});
    };

    entry_obj.insert("slug".to_string(), json!(model));
    entry_obj.insert("model".to_string(), json!(model));
    entry_obj.insert("provider".to_string(), json!(provider_id));
    entry_obj.insert("backend_provider".to_string(), json!(provider_id));
    entry_obj.insert("display_name".to_string(), json!(display_name));
    entry_obj.insert("description".to_string(), json!(display_name));
    entry_obj.insert("context_window".to_string(), json!(context_window));
    entry_obj.insert("max_context_window".to_string(), json!(context_window));
    entry_obj.insert("minimal_client_version".to_string(), json!("0.0.1"));
    entry_obj.insert(
        "available_in_plans".to_string(),
        json!(["free", "plus", "pro", "team", "business", "enterprise"]),
    );
    entry_obj.insert("visibility".to_string(), json!("list"));
    entry_obj.insert("supported_in_api".to_string(), json!(true));
    entry_obj.insert("priority".to_string(), json!(1000 + priority));
    entry_obj.insert("additional_speed_tiers".to_string(), json!([]));
    entry_obj.insert("service_tiers".to_string(), json!([]));
    entry_obj.insert("availability_nux".to_string(), Value::Null);
    entry_obj.insert("upgrade".to_string(), Value::Null);

    entry
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CodexCatalogModelSpec {
    provider_id: String,
    model: String,
    display_name: String,
    context_window: u64,
}

fn codex_catalog_provider_id(config_text: &str) -> String {
    config_text
        .parse::<DocumentMut>()
        .ok()
        .and_then(|doc| active_codex_model_provider_id(&doc))
        .filter(|id| is_custom_codex_model_provider_id(id))
        .unwrap_or_else(|| CC_SWITCH_CODEX_MODEL_PROVIDER_ID.to_string())
}

fn codex_catalog_model_specs(settings: &Value, config_text: &str) -> Vec<CodexCatalogModelSpec> {
    let Some(models) = settings
        .get("modelCatalog")
        .and_then(|catalog| catalog.get("models"))
        .and_then(|models| models.as_array())
    else {
        return Vec::new();
    };

    let default_context_window =
        extract_codex_top_level_u64(config_text, "model_context_window").unwrap_or(128_000);
    let provider_id = codex_catalog_provider_id(config_text);
    let mut seen = HashSet::new();
    let mut specs = Vec::new();

    for model_config in models {
        let Some(model) = model_config
            .get("model")
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|model| !model.is_empty())
        else {
            continue;
        };

        if !seen.insert(model.to_string()) {
            continue;
        }

        let display_name = model_config
            .get("displayName")
            .or_else(|| model_config.get("display_name"))
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .unwrap_or(model);
        let context_window = parse_codex_positive_u64(
            model_config
                .get("contextWindow")
                .or_else(|| model_config.get("context_window")),
        )
        .unwrap_or(default_context_window);

        specs.push(CodexCatalogModelSpec {
            provider_id: provider_id.clone(),
            model: model.to_string(),
            display_name: display_name.to_string(),
            context_window,
        });
    }

    specs
}

fn codex_model_ids_from_settings(settings: &Value) -> Vec<String> {
    let Some(models) = settings
        .get("modelCatalog")
        .and_then(|catalog| catalog.get("models"))
        .and_then(|models| models.as_array())
    else {
        return Vec::new();
    };

    let mut seen = HashSet::new();
    let mut model_ids = Vec::new();
    for model_config in models {
        let Some(model) = model_config
            .get("model")
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|model| !model.is_empty())
        else {
            continue;
        };

        if seen.insert(model.to_string()) {
            model_ids.push(model.to_string());
        }
    }

    model_ids
}

fn decode_codex_desktop_statsig_wrapper(
    bytes: &[u8],
) -> Option<(Option<u8>, CodexDesktopStatsigWrapperEncoding, Value)> {
    // Chromium localStorage LevelDB values can carry a one-byte type prefix and
    // may store strings as either UTF-8 or UTF-16LE. Preserve the original form.
    let (prefix, json_bytes) = if matches!(bytes.first(), Some(0) | Some(1)) {
        (bytes.first().copied(), &bytes[1..])
    } else {
        (None, bytes)
    };

    if let Ok(text) = std::str::from_utf8(json_bytes) {
        if let Ok(wrapper) = serde_json::from_str(text) {
            return Some((prefix, CodexDesktopStatsigWrapperEncoding::Utf8, wrapper));
        }
    }

    if json_bytes.len() % 2 == 0 {
        let units = json_bytes
            .chunks_exact(2)
            .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
            .collect::<Vec<_>>();
        if let Ok(text) = String::from_utf16(&units) {
            if let Ok(wrapper) = serde_json::from_str(&text) {
                return Some((prefix, CodexDesktopStatsigWrapperEncoding::Utf16Le, wrapper));
            }
        }
    }

    None
}

fn encode_codex_desktop_statsig_wrapper(
    prefix: Option<u8>,
    encoding: CodexDesktopStatsigWrapperEncoding,
    wrapper: &Value,
) -> Option<Vec<u8>> {
    let text = serde_json::to_string(wrapper).ok()?;
    let payload_capacity = match encoding {
        CodexDesktopStatsigWrapperEncoding::Utf8 => text.len(),
        CodexDesktopStatsigWrapperEncoding::Utf16Le => text.len() * 2,
    };
    let mut encoded = Vec::with_capacity(payload_capacity + usize::from(prefix.is_some()));
    if let Some(prefix) = prefix {
        encoded.push(prefix);
    }
    match encoding {
        CodexDesktopStatsigWrapperEncoding::Utf8 => encoded.extend_from_slice(text.as_bytes()),
        CodexDesktopStatsigWrapperEncoding::Utf16Le => {
            for unit in text.encode_utf16() {
                encoded.extend_from_slice(&unit.to_le_bytes());
            }
        }
    }
    Some(encoded)
}

fn codex_desktop_statsig_available_model_ids(wrapper: &Value) -> Option<HashSet<String>> {
    let data_text = wrapper.get("data").and_then(|value| value.as_str())?;
    let data = serde_json::from_str::<Value>(data_text).ok()?;
    data.get("dynamic_configs")
        .and_then(|value| value.get(CODEX_DESKTOP_STATSIG_MODELS_CONFIG_ID))
        .and_then(|value| value.get("value"))
        .and_then(|value| value.get("available_models"))
        .and_then(|value| value.as_array())
        .map(|models| {
            models
                .iter()
                .filter_map(|value| value.as_str().map(str::to_string))
                .collect::<HashSet<_>>()
        })
}

fn codex_desktop_statsig_has_all_models(wrapper: &Value, model_ids: &[String]) -> bool {
    let Some(available_models) = codex_desktop_statsig_available_model_ids(wrapper) else {
        return false;
    };
    model_ids
        .iter()
        .all(|model_id| available_models.contains(model_id))
}

fn merge_codex_desktop_statsig_available_models(wrapper: &mut Value, model_ids: &[String]) -> bool {
    if model_ids.is_empty() {
        return false;
    }

    let Some(data_text) = wrapper.get("data").and_then(|value| value.as_str()) else {
        return false;
    };
    let Ok(mut data) = serde_json::from_str::<Value>(data_text) else {
        return false;
    };

    let Some(data_obj) = data.as_object_mut() else {
        return false;
    };
    let dynamic_configs = data_obj
        .entry("dynamic_configs")
        .or_insert_with(|| json!({}));
    let Some(dynamic_configs_obj) = dynamic_configs.as_object_mut() else {
        return false;
    };
    let config = dynamic_configs_obj
        .entry(CODEX_DESKTOP_STATSIG_MODELS_CONFIG_ID)
        .or_insert_with(|| json!({ "value": {} }));
    let Some(config_obj) = config.as_object_mut() else {
        return false;
    };
    let value = config_obj.entry("value").or_insert_with(|| json!({}));
    let Some(value_obj) = value.as_object_mut() else {
        return false;
    };
    let available_models = value_obj
        .entry("available_models")
        .or_insert_with(|| json!([]));
    let Some(available_models) = available_models.as_array_mut() else {
        return false;
    };

    let mut seen = available_models
        .iter()
        .filter_map(|value| value.as_str().map(str::to_string))
        .collect::<HashSet<_>>();
    let mut changed = false;

    for model_id in model_ids {
        if seen.insert(model_id.clone()) {
            available_models.push(json!(model_id));
            changed = true;
        }
    }

    if !changed {
        return false;
    }

    let Ok(updated_data_text) = serde_json::to_string(&data) else {
        return false;
    };
    if let Some(wrapper_obj) = wrapper.as_object_mut() {
        wrapper_obj.insert("data".to_string(), json!(updated_data_text));
        true
    } else {
        false
    }
}

fn codex_desktop_statsig_cache_key_from_leveldb_key(key_text: &str) -> Option<String> {
    let marker_start = key_text.find(CODEX_DESKTOP_STATSIG_CACHE_KEY_MARKER)?;
    Some(
        key_text[marker_start..]
            .trim_matches(char::from(0))
            .to_string(),
    )
}

fn codex_desktop_statsig_timestamp_millis(value: &Value) -> Option<i64> {
    if let Some(value) = value.as_i64() {
        return Some(value);
    }
    if let Some(value) = value.as_u64() {
        return i64::try_from(value).ok();
    }
    value.as_f64().and_then(|value| {
        if value.is_finite() && value >= 0.0 && value <= i64::MAX as f64 {
            Some(value as i64)
        } else {
            None
        }
    })
}

fn codex_desktop_active_statsig_cache_keys(last_modified: &Value) -> Vec<String> {
    let Some(entries) = last_modified.as_object() else {
        return Vec::new();
    };

    let mut cache_keys = entries
        .iter()
        .filter(|(key, _)| key.contains(CODEX_DESKTOP_STATSIG_CACHE_KEY_MARKER))
        .filter_map(|(key, value)| {
            codex_desktop_statsig_timestamp_millis(value).map(|timestamp| (key.clone(), timestamp))
        })
        .collect::<Vec<_>>();

    cache_keys.sort_by(|(left_key, left_time), (right_key, right_time)| {
        right_time
            .cmp(left_time)
            .then_with(|| left_key.cmp(right_key))
    });

    cache_keys.into_iter().map(|(key, _)| key).collect()
}

fn codex_desktop_statsig_active_rank(
    key_text: &str,
    active_cache_keys: &[String],
) -> Option<usize> {
    active_cache_keys
        .iter()
        .position(|cache_key| key_text.contains(cache_key))
}

fn codex_desktop_now_millis() -> i64 {
    let Ok(elapsed) = SystemTime::now().duration_since(UNIX_EPOCH) else {
        return 0;
    };
    elapsed.as_millis().min(i64::MAX as u128) as i64
}

fn codex_desktop_saturating_add_duration_millis(base: i64, duration: Duration) -> i64 {
    let millis = duration.as_millis().min(i64::MAX as u128) as i64;
    base.saturating_add(millis)
}

fn pin_codex_desktop_statsig_last_modified_cache_keys(
    last_modified: &mut Value,
    cache_keys: &HashSet<String>,
    now_millis: i64,
) -> bool {
    if cache_keys.is_empty() {
        return false;
    }

    let Some(entries) = last_modified.as_object_mut() else {
        return false;
    };

    let refresh_after = codex_desktop_saturating_add_duration_millis(
        now_millis,
        CODEX_DESKTOP_STATSIG_PIN_REFRESH_AFTER,
    );
    let pinned_until =
        codex_desktop_saturating_add_duration_millis(now_millis, CODEX_DESKTOP_STATSIG_PIN_HORIZON);
    let mut changed = false;

    for cache_key in cache_keys {
        if let Some(value) = entries.get_mut(cache_key) {
            let current_timestamp =
                codex_desktop_statsig_timestamp_millis(value).unwrap_or_default();
            if current_timestamp < refresh_after {
                *value = json!(pinned_until);
                changed = true;
            }
        } else {
            entries.insert(cache_key.clone(), json!(pinned_until));
            changed = true;
        }
    }

    changed
}

fn codex_desktop_local_storage_leveldb_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();

    #[cfg(target_os = "macos")]
    {
        let codex_support_dir = get_home_dir()
            .join("Library")
            .join("Application Support")
            .join("Codex");
        candidates.push(
            codex_support_dir
                .join("Default")
                .join("Local Storage")
                .join("leveldb"),
        );
        candidates.push(codex_support_dir.join("Local Storage").join("leveldb"));
        candidates.push(
            codex_support_dir
                .join("Default")
                .join("Partitions")
                .join("codex-browser-app")
                .join("Local Storage")
                .join("leveldb"),
        );
        candidates.push(
            codex_support_dir
                .join("Partitions")
                .join("codex-browser-app")
                .join("Local Storage")
                .join("leveldb"),
        );
        candidates.push(
            codex_support_dir
                .join("codex-browser-app")
                .join("Local Storage")
                .join("leveldb"),
        );
    }

    #[cfg(target_os = "windows")]
    {
        if let Some(appdata) = std::env::var_os("APPDATA") {
            let codex_support_dir = PathBuf::from(appdata).join("Codex");
            candidates.push(
                codex_support_dir
                    .join("Default")
                    .join("Local Storage")
                    .join("leveldb"),
            );
            candidates.push(codex_support_dir.join("Local Storage").join("leveldb"));
        }
    }

    #[cfg(target_os = "linux")]
    {
        if let Some(config_home) = std::env::var_os("XDG_CONFIG_HOME") {
            let codex_support_dir = PathBuf::from(config_home).join("Codex");
            candidates.push(
                codex_support_dir
                    .join("Default")
                    .join("Local Storage")
                    .join("leveldb"),
            );
            candidates.push(codex_support_dir.join("Local Storage").join("leveldb"));
        }
        let codex_support_dir = get_home_dir().join(".config").join("Codex");
        candidates.push(
            codex_support_dir
                .join("Default")
                .join("Local Storage")
                .join("leveldb"),
        );
        candidates.push(codex_support_dir.join("Local Storage").join("leveldb"));
    }

    candidates
}

fn sync_codex_desktop_available_models_cache_path(
    leveldb_path: &Path,
    model_ids: &[String],
) -> Result<usize, String> {
    let options = rusty_leveldb::Options {
        create_if_missing: false,
        ..Default::default()
    };
    let mut db = rusty_leveldb::DB::open(leveldb_path, options).map_err(|err| match err.code {
        rusty_leveldb::StatusCode::LockError => {
            format!("Codex Desktop localStorage LevelDB is locked: {leveldb_path:?}")
        }
        _ => format!("Failed to open Codex Desktop localStorage LevelDB {leveldb_path:?}: {err}"),
    })?;

    let mut cache_entries = Vec::new();
    let mut last_modified_entries = Vec::new();
    {
        use rusty_leveldb::LdbIterator;

        let mut iter = db.new_iter().map_err(|err| {
            format!("Failed to iterate Codex Desktop localStorage LevelDB: {err}")
        })?;
        while iter.advance() {
            let Some((key, value)) = iter.current() else {
                continue;
            };

            let key_text = String::from_utf8_lossy(&key).to_string();
            if key_text.contains(CODEX_DESKTOP_STATSIG_LAST_MODIFIED_KEY_MARKER) {
                if let Some((prefix, encoding, last_modified)) =
                    decode_codex_desktop_statsig_wrapper(&value)
                {
                    last_modified_entries.push((key.to_vec(), prefix, encoding, last_modified));
                }
            }

            if key_text.contains(CODEX_DESKTOP_STATSIG_CACHE_KEY_MARKER) {
                cache_entries.push((key.to_vec(), key_text, value.to_vec()));
            }
        }
    }

    let mut active_cache_keys = Vec::new();
    let mut seen_active_cache_keys = HashSet::new();
    for (_, _, _, last_modified) in &last_modified_entries {
        for cache_key in codex_desktop_active_statsig_cache_keys(last_modified) {
            if seen_active_cache_keys.insert(cache_key.clone()) {
                active_cache_keys.push(cache_key);
            }
        }
    }

    cache_entries.sort_by(|(_, left_key_text, _), (_, right_key_text, _)| {
        let left_rank = codex_desktop_statsig_active_rank(left_key_text, &active_cache_keys);
        let right_rank = codex_desktop_statsig_active_rank(right_key_text, &active_cache_keys);
        match (left_rank, right_rank) {
            (Some(left), Some(right)) => left.cmp(&right),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => left_key_text.cmp(right_key_text),
        }
    });

    let mut updates = Vec::new();
    let mut pinned_cache_keys = HashSet::new();
    let mut active_update_count = 0usize;

    for (key, key_text, value) in cache_entries {
        let Some(cache_key) = codex_desktop_statsig_cache_key_from_leveldb_key(&key_text) else {
            continue;
        };
        let is_active = codex_desktop_statsig_active_rank(&key_text, &active_cache_keys).is_some();

        let Some((prefix, encoding, mut wrapper)) = decode_codex_desktop_statsig_wrapper(&value)
        else {
            continue;
        };

        let had_all_models = codex_desktop_statsig_has_all_models(&wrapper, model_ids);
        let changed = !had_all_models
            && merge_codex_desktop_statsig_available_models(&mut wrapper, model_ids);
        if codex_desktop_statsig_has_all_models(&wrapper, model_ids) {
            pinned_cache_keys.insert(cache_key);
        }

        if !changed {
            continue;
        }

        let Some(updated_value) = encode_codex_desktop_statsig_wrapper(prefix, encoding, &wrapper)
        else {
            continue;
        };
        if is_active {
            active_update_count += 1;
        }
        updates.push((key, updated_value));
    }

    let now_millis = codex_desktop_now_millis();
    for (key, prefix, encoding, mut last_modified) in last_modified_entries {
        if !pin_codex_desktop_statsig_last_modified_cache_keys(
            &mut last_modified,
            &pinned_cache_keys,
            now_millis,
        ) {
            continue;
        }
        let Some(updated_value) =
            encode_codex_desktop_statsig_wrapper(prefix, encoding, &last_modified)
        else {
            continue;
        };
        updates.push((key, updated_value));
    }

    let updated_count = updates.len();
    for (key, value) in updates {
        db.put(&key, &value)
            .map_err(|err| format!("Failed to update Codex Desktop localStorage LevelDB: {err}"))?;
    }
    db.close()
        .map_err(|err| format!("Failed to close Codex Desktop localStorage LevelDB: {err}"))?;

    if updated_count > 0 {
        log::info!(
            "Synced Codex Desktop Statsig LevelDB path {leveldb_path:?}: updated {updated_count} entries, active cache updates: {active_update_count}, pinned cache keys: {}",
            pinned_cache_keys.len()
        );
    } else if !active_cache_keys.is_empty() {
        log::debug!(
            "Codex Desktop Statsig LevelDB path {leveldb_path:?} already has {} pinned cache keys; active cache candidates: {}",
            pinned_cache_keys.len(),
            active_cache_keys.len()
        );
    }

    Ok(updated_count)
}

fn sync_codex_desktop_available_models_cache(model_ids: &[String]) -> Result<usize, String> {
    if model_ids.is_empty() {
        return Ok(0);
    }

    let mut seen_paths = HashSet::new();
    let leveldb_paths = codex_desktop_local_storage_leveldb_candidates()
        .into_iter()
        .filter(|path| path.exists())
        .filter(|path| seen_paths.insert(path.clone()))
        .collect::<Vec<_>>();
    if leveldb_paths.is_empty() {
        return Ok(0);
    }

    let mut updated_count = 0;
    let mut errors = Vec::new();

    for leveldb_path in leveldb_paths {
        match sync_codex_desktop_available_models_cache_path(&leveldb_path, model_ids) {
            Ok(count) => updated_count += count,
            Err(err) => errors.push(err),
        }
    }

    if updated_count == 0 && !errors.is_empty() {
        Err(errors.join("; "))
    } else {
        if !errors.is_empty() {
            log::warn!(
                "Some Codex Desktop model whitelist cache paths could not be synced: {}",
                errors.join("; ")
            );
        }
        Ok(updated_count)
    }
}

fn log_codex_desktop_available_models_cache_sync_result(
    model_ids: &[String],
    result: Result<usize, String>,
    is_retry: bool,
) {
    match result {
        Ok(updated_count) if updated_count > 0 => {
            log::info!(
                "Synced {} Codex model ids into {} Codex Desktop Statsig cache entries{}",
                model_ids.len(),
                updated_count,
                if is_retry { " during retry" } else { "" }
            );
        }
        Ok(_) => {
            log::debug!(
                "No Codex Desktop Statsig cache entry needed model whitelist updates for {} model ids{}",
                model_ids.len(),
                if is_retry { " during retry" } else { "" }
            );
        }
        Err(err) if is_retry => {
            log::debug!(
                "Codex Desktop model whitelist cache retry is pending for {} model ids: {err}",
                model_ids.len()
            );
        }
        Err(err) => {
            log::warn!(
                "Failed to sync Codex Desktop model whitelist cache; CC Switch will keep retrying while this Codex provider is active: {err}"
            );
        }
    }
}

fn codex_desktop_runtime_patch_expression(model_ids: &[String]) -> Result<String, String> {
    let model_ids_json = serde_json::to_string(model_ids)
        .map_err(|err| format!("Failed to serialize Codex model ids for runtime patch: {err}"))?;
    Ok(format!(
        r#"
(() => {{
  const MODEL_IDS = {model_ids_json};
  const CONFIG_ID = "{CODEX_DESKTOP_STATSIG_MODELS_CONFIG_ID}";
  const CACHE_PREFIX = "{CODEX_DESKTOP_STATSIG_CACHE_KEY_MARKER}";
  const LAST_MODIFIED_KEY = "{CODEX_DESKTOP_STATSIG_LAST_MODIFIED_KEY_MARKER}";
  const PIN_UNTIL = Date.now() + 30 * 24 * 60 * 60 * 1000;
  let changed = false;
  let patchedValueCount = 0;
  let storageCacheCount = 0;

  const hasAllModels = (value) => {{
    const list = value && Array.isArray(value.available_models) ? value.available_models : [];
    return MODEL_IDS.every((modelId) => list.includes(modelId));
  }};

  const patchValue = (value) => {{
    if (!value || typeof value !== "object") return false;
    if (!Array.isArray(value.available_models)) value.available_models = [];
    const seen = new Set(value.available_models);
    let valueChanged = false;
    for (const modelId of MODEL_IDS) {{
      if (!seen.has(modelId)) {{
        value.available_models.push(modelId);
        seen.add(modelId);
        valueChanged = true;
      }}
    }}
    if (valueChanged) {{
      changed = true;
      patchedValueCount += 1;
    }}
    return valueChanged;
  }};

  const patchConfig = (config) => {{
    if (!config || typeof config !== "object") return false;
    let configChanged = false;
    configChanged = patchValue(config.value) || configChanged;
    configChanged = patchValue(config.__evaluation && config.__evaluation.value) || configChanged;
    return configChanged;
  }};

  const patchStatsigTree = (root, depth = 0, seen = new WeakSet()) => {{
    if (!root || typeof root !== "object" || depth > 5 || seen.has(root)) return false;
    seen.add(root);
    let treeChanged = false;
    if (root.dynamic_configs && root.dynamic_configs[CONFIG_ID]) {{
      treeChanged = patchConfig(root.dynamic_configs[CONFIG_ID]) || treeChanged;
    }}
    if (root[CONFIG_ID]) {{
      treeChanged = patchConfig(root[CONFIG_ID]) || treeChanged;
    }}
    for (const key of Object.keys(root)) {{
      const value = root[key];
      if (value && typeof value === "object") {{
        treeChanged = patchStatsigTree(value, depth + 1, seen) || treeChanged;
      }}
    }}
    return treeChanged;
  }};

  const readStoredJson = (key) => {{
    const raw = localStorage.getItem(key);
    if (!raw) return null;
    const wrapper = JSON.parse(raw);
    if (wrapper && typeof wrapper.data === "string") {{
      return {{ wrapper, data: JSON.parse(wrapper.data), wrapped: true }};
    }}
    return {{ wrapper: null, data: wrapper, wrapped: false }};
  }};

  const writeStoredJson = (key, stored) => {{
    if (stored.wrapped) {{
      stored.wrapper.data = JSON.stringify(stored.data);
      localStorage.setItem(key, JSON.stringify(stored.wrapper));
    }} else {{
      localStorage.setItem(key, JSON.stringify(stored.data));
    }}
  }};

  const patchedStorageKeys = [];
  for (const key of Object.keys(localStorage)) {{
    if (!key.startsWith(CACHE_PREFIX)) continue;
    try {{
      const stored = readStoredJson(key);
      const config = stored && stored.data && stored.data.dynamic_configs && stored.data.dynamic_configs[CONFIG_ID];
      const beforeHasAll = config && config.value && hasAllModels(config.value);
      const storageChanged = patchConfig(config);
      const afterHasAll = config && config.value && hasAllModels(config.value);
      if (storageChanged) {{
        writeStoredJson(key, stored);
        storageCacheCount += 1;
      }}
      if (beforeHasAll || afterHasAll) {{
        patchedStorageKeys.push(key);
      }}
    }} catch (_) {{}}
  }}

  try {{
    const stored = readStoredJson(LAST_MODIFIED_KEY);
    if (stored && stored.data && typeof stored.data === "object") {{
      let lastModifiedChanged = false;
      for (const key of patchedStorageKeys) {{
        const current = Number(stored.data[key] || 0);
        if (!Number.isFinite(current) || current < PIN_UNTIL) {{
          stored.data[key] = PIN_UNTIL;
          lastModifiedChanged = true;
        }}
      }}
      if (lastModifiedChanged) {{
        writeStoredJson(LAST_MODIFIED_KEY, stored);
      }}
    }}
  }} catch (_) {{}}

  const client = window.__STATSIG__ && window.__STATSIG__.firstInstance;
  const storePatched = patchStatsigTree(client && client._store);
  patchConfig(client && client.getDynamicConfig && client.getDynamicConfig(CONFIG_ID));
  patchConfig(client && client._memoCache && client._memoCache["c|" + CONFIG_ID]);

  if (client && changed) {{
    try {{
      if (storePatched && typeof client._setStatus === "function") {{
        const values = client._store && client._store._values && client._store._values._values;
        client._setStatus(client.loadingStatus || "Ready", values || null);
      }} else if (typeof client.$emt === "function") {{
        client.$emt({{ name: "values_updated", status: client.loadingStatus || "Ready", values: null }});
      }}
    }} catch (_) {{}}
  }}

  const activeModels = client && client.getDynamicConfig
    ? (client.getDynamicConfig(CONFIG_ID).value || {{}}).available_models
    : null;
  return {{
    changed,
    activeModelCount: Array.isArray(activeModels) ? activeModels.length : 0,
    patchedValueCount,
    storageCacheCount
  }};
}})()
"#
    ))
}

fn codex_desktop_devtools_page_websocket_url() -> Result<Option<String>, String> {
    let address = SocketAddr::from(([127, 0, 0, 1], CODEX_DESKTOP_REMOTE_DEBUGGING_PORT));
    let mut stream = match TcpStream::connect_timeout(&address, CODEX_DESKTOP_DEVTOOLS_TIMEOUT) {
        Ok(stream) => stream,
        Err(err)
            if matches!(
                err.kind(),
                ErrorKind::ConnectionRefused | ErrorKind::NotFound | ErrorKind::TimedOut
            ) =>
        {
            return Ok(None);
        }
        Err(err) => {
            return Err(format!(
                "Failed to connect to Codex Desktop DevTools target list: {err}"
            ));
        }
    };
    stream
        .set_read_timeout(Some(CODEX_DESKTOP_DEVTOOLS_TIMEOUT))
        .map_err(|err| format!("Failed to set Codex Desktop DevTools read timeout: {err}"))?;
    stream
        .set_write_timeout(Some(CODEX_DESKTOP_DEVTOOLS_TIMEOUT))
        .map_err(|err| format!("Failed to set Codex Desktop DevTools write timeout: {err}"))?;

    let request = format!(
        "GET /json/list HTTP/1.1\r\nHost: 127.0.0.1:{CODEX_DESKTOP_REMOTE_DEBUGGING_PORT}\r\nAccept: application/json\r\nConnection: close\r\n\r\n"
    );
    match stream.write_all(request.as_bytes()) {
        Ok(()) => {}
        Err(err)
            if matches!(
                err.kind(),
                ErrorKind::BrokenPipe | ErrorKind::ConnectionReset | ErrorKind::TimedOut
            ) =>
        {
            return Ok(None);
        }
        Err(err) => {
            return Err(format!(
                "Failed to query Codex Desktop DevTools target list: {err}"
            ));
        }
    }

    let mut response_bytes = Vec::new();
    let mut buffer = [0; 4096];
    loop {
        match stream.read(&mut buffer) {
            Ok(0) => break,
            Ok(bytes_read) => response_bytes.extend_from_slice(&buffer[..bytes_read]),
            Err(err)
                if matches!(
                    err.kind(),
                    ErrorKind::WouldBlock | ErrorKind::TimedOut | ErrorKind::ConnectionReset
                ) =>
            {
                if response_bytes.is_empty() {
                    return Ok(None);
                }
                break;
            }
            Err(err) => {
                return Err(format!(
                    "Failed to read Codex Desktop DevTools target list: {err}"
                ));
            }
        }
    }

    let response = String::from_utf8(response_bytes)
        .map_err(|err| format!("Codex Desktop DevTools target list was not UTF-8: {err}"))?;
    let (headers, body) = response
        .split_once("\r\n\r\n")
        .ok_or_else(|| "Codex Desktop DevTools target list response had no body".to_string())?;
    let status_ok = headers
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .is_some_and(|status| status == "200");
    if !status_ok {
        return Ok(None);
    }

    let targets = serde_json::from_str::<Value>(body)
        .map_err(|err| format!("Failed to parse Codex Desktop DevTools target list: {err}"))?;
    let Some(targets) = targets.as_array() else {
        return Ok(None);
    };

    let page = targets
        .iter()
        .find(|target| {
            target.get("type").and_then(Value::as_str) == Some("page")
                && target
                    .get("url")
                    .and_then(Value::as_str)
                    .is_some_and(|url| url == "app://-/index.html")
        })
        .or_else(|| {
            targets.iter().find(|target| {
                target.get("type").and_then(Value::as_str) == Some("page")
                    && target
                        .get("url")
                        .and_then(Value::as_str)
                        .is_some_and(|url| url.starts_with("app://-"))
            })
        });

    Ok(page
        .and_then(|target| target.get("webSocketDebuggerUrl"))
        .and_then(Value::as_str)
        .map(str::to_string))
}

fn connect_codex_desktop_devtools_websocket(
    websocket_url: &str,
) -> Result<tungstenite::WebSocket<TcpStream>, String> {
    let parsed_url = url::Url::parse(websocket_url)
        .map_err(|err| format!("Invalid Codex Desktop DevTools websocket URL: {err}"))?;
    if parsed_url.scheme() != "ws" {
        return Err(format!(
            "Unsupported Codex Desktop DevTools websocket scheme `{}`",
            parsed_url.scheme()
        ));
    }
    let host = parsed_url
        .host_str()
        .ok_or_else(|| "Codex Desktop DevTools websocket URL has no host".to_string())?;
    let port = parsed_url.port_or_known_default().ok_or_else(|| {
        "Codex Desktop DevTools websocket URL has no explicit or known port".to_string()
    })?;

    let mut last_error = None;
    for address in (host, port)
        .to_socket_addrs()
        .map_err(|err| format!("Failed to resolve Codex Desktop DevTools address: {err}"))?
    {
        match TcpStream::connect_timeout(&address, CODEX_DESKTOP_DEVTOOLS_TIMEOUT) {
            Ok(stream) => {
                stream
                    .set_read_timeout(Some(CODEX_DESKTOP_DEVTOOLS_TIMEOUT))
                    .map_err(|err| {
                        format!("Failed to set Codex Desktop DevTools read timeout: {err}")
                    })?;
                stream
                    .set_write_timeout(Some(CODEX_DESKTOP_DEVTOOLS_TIMEOUT))
                    .map_err(|err| {
                        format!("Failed to set Codex Desktop DevTools write timeout: {err}")
                    })?;

                let (socket, _) = tungstenite::client(websocket_url, stream).map_err(|err| {
                    format!("Failed to connect Codex Desktop DevTools websocket: {err}")
                })?;
                return Ok(socket);
            }
            Err(err) => {
                last_error = Some(err);
            }
        }
    }

    Err(match last_error {
        Some(err) => format!("Failed to connect Codex Desktop DevTools websocket: {err}"),
        None => "Codex Desktop DevTools websocket host resolved no addresses".to_string(),
    })
}

fn read_codex_desktop_cdp_response<Stream>(
    socket: &mut tungstenite::WebSocket<Stream>,
    request_id: i64,
) -> Result<Value, String>
where
    Stream: Read + Write,
{
    loop {
        let message = socket
            .read()
            .map_err(|err| format!("Failed to read Codex Desktop CDP response: {err}"))?;
        let text = match message {
            Message::Text(text) => text,
            Message::Binary(bytes) => String::from_utf8(bytes)
                .map_err(|err| format!("Codex Desktop CDP response was not UTF-8: {err}"))?,
            Message::Close(_) => {
                return Err("Codex Desktop CDP socket closed before response".to_string());
            }
            Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => continue,
        };
        let value: Value = serde_json::from_str(&text)
            .map_err(|err| format!("Failed to parse Codex Desktop CDP response: {err}"))?;
        if value.get("id").and_then(Value::as_i64) == Some(request_id) {
            return Ok(value);
        }
    }
}

fn send_codex_desktop_cdp_request<Stream>(
    socket: &mut tungstenite::WebSocket<Stream>,
    request_id: i64,
    method: &str,
    params: Value,
) -> Result<Value, String>
where
    Stream: Read + Write,
{
    let request = json!({
        "id": request_id,
        "method": method,
        "params": params,
    });
    socket
        .send(Message::Text(request.to_string()))
        .map_err(|err| format!("Failed to send Codex Desktop CDP request: {err}"))?;
    read_codex_desktop_cdp_response(socket, request_id)
}

fn sync_codex_desktop_available_models_runtime(
    model_ids: &[String],
) -> Result<Option<CodexDesktopRuntimeWhitelistPatchResult>, String> {
    if model_ids.is_empty() {
        return Ok(None);
    }

    let Some(websocket_url) = codex_desktop_devtools_page_websocket_url()? else {
        return Ok(None);
    };
    let mut socket = connect_codex_desktop_devtools_websocket(websocket_url.as_str())?;

    send_codex_desktop_cdp_request(&mut socket, 1, "Runtime.enable", json!({}))?;
    let expression = codex_desktop_runtime_patch_expression(model_ids)?;
    let response = send_codex_desktop_cdp_request(
        &mut socket,
        2,
        "Runtime.evaluate",
        json!({
            "expression": expression,
            "returnByValue": true,
            "awaitPromise": true,
        }),
    )?;
    let _ = socket.close(None);

    if let Some(exception) = response
        .get("result")
        .and_then(|result| result.get("exceptionDetails"))
    {
        return Err(format!(
            "Codex Desktop runtime patch threw an exception: {exception}"
        ));
    }
    let Some(value) = response
        .get("result")
        .and_then(|result| result.get("result"))
        .and_then(|result| result.get("value"))
    else {
        return Err("Codex Desktop runtime patch returned no value".to_string());
    };

    Ok(Some(CodexDesktopRuntimeWhitelistPatchResult {
        changed: value
            .get("changed")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        active_model_count: value
            .get("activeModelCount")
            .and_then(Value::as_u64)
            .unwrap_or(0) as usize,
        patched_value_count: value
            .get("patchedValueCount")
            .and_then(Value::as_u64)
            .unwrap_or(0) as usize,
        storage_cache_count: value
            .get("storageCacheCount")
            .and_then(Value::as_u64)
            .unwrap_or(0) as usize,
    }))
}

fn log_codex_desktop_available_models_runtime_sync_result(
    model_ids: &[String],
    result: Result<Option<CodexDesktopRuntimeWhitelistPatchResult>, String>,
    is_retry: bool,
) {
    match result {
        Ok(Some(result)) if result.changed => {
            log::info!(
                "Synced {} Codex model ids into Codex Desktop renderer Statsig state{}; active models: {}, patched values: {}, storage caches: {}",
                model_ids.len(),
                if is_retry { " during retry" } else { "" },
                result.active_model_count,
                result.patched_value_count,
                result.storage_cache_count
            );
        }
        Ok(Some(result)) => {
            log::debug!(
                "Codex Desktop renderer Statsig state already includes {} model ids{}; active models: {}",
                model_ids.len(),
                if is_retry { " during retry" } else { "" },
                result.active_model_count
            );
        }
        Ok(None) => {
            log::debug!(
                "Codex Desktop DevTools target is not available for renderer model whitelist sync{}",
                if is_retry { " during retry" } else { "" }
            );
        }
        Err(err) if is_retry => {
            log::debug!(
                "Codex Desktop renderer model whitelist retry is pending for {} model ids: {err}",
                model_ids.len()
            );
        }
        Err(err) => {
            log::warn!(
                "Failed to sync Codex Desktop renderer model whitelist; CC Switch will keep retrying while this Codex provider is active: {err}"
            );
        }
    }
}

fn sync_codex_desktop_available_models_everywhere(model_ids: &[String], is_retry: bool) {
    log_codex_desktop_available_models_cache_sync_result(
        model_ids,
        sync_codex_desktop_available_models_cache(model_ids),
        is_retry,
    );
    log_codex_desktop_available_models_runtime_sync_result(
        model_ids,
        sync_codex_desktop_available_models_runtime(model_ids),
        is_retry,
    );
}

fn codex_desktop_model_whitelist_sync_handle() -> CodexDesktopModelWhitelistSyncHandle {
    CODEX_DESKTOP_MODEL_WHITELIST_SYNC
        .get_or_init(|| {
            let handle = Arc::new((
                Mutex::new(CodexDesktopModelWhitelistSyncState::default()),
                Condvar::new(),
            ));
            let worker_handle = Arc::clone(&handle);
            if let Err(err) = std::thread::Builder::new()
                .name("codex-desktop-model-whitelist-sync".to_string())
                .spawn(move || codex_desktop_model_whitelist_sync_worker(worker_handle))
            {
                log::warn!("Failed to start Codex Desktop model whitelist sync worker: {err}");
            }
            handle
        })
        .clone()
}

fn codex_desktop_model_whitelist_sync_worker(handle: CodexDesktopModelWhitelistSyncHandle) {
    let (lock, cvar) = &*handle;
    let mut seen_generation = 0;

    loop {
        let model_ids = {
            let mut state = match lock.lock() {
                Ok(state) => state,
                Err(err) => {
                    log::warn!("Codex Desktop model whitelist sync state was poisoned; continuing");
                    err.into_inner()
                }
            };
            while state.generation == seen_generation {
                state = match cvar.wait(state) {
                    Ok(state) => state,
                    Err(err) => {
                        log::warn!(
                            "Codex Desktop model whitelist sync state was poisoned; continuing"
                        );
                        err.into_inner()
                    }
                };
            }
            seen_generation = state.generation;
            state.model_ids.clone()
        };

        if model_ids.is_empty() {
            continue;
        }

        // Codex Desktop can lock or refresh its Statsig localStorage after a
        // provider switch, so keep reconciling the active provider's model IDs.
        loop {
            let mut state = match lock.lock() {
                Ok(state) => state,
                Err(err) => {
                    log::warn!("Codex Desktop model whitelist sync state was poisoned; continuing");
                    err.into_inner()
                }
            };
            let wait_result = cvar.wait_timeout_while(
                state,
                CODEX_DESKTOP_MODEL_WHITELIST_RETRY_INTERVAL,
                |state| state.generation == seen_generation,
            );
            state = match wait_result {
                Ok((state, _)) => state,
                Err(err) => {
                    log::warn!("Codex Desktop model whitelist sync state was poisoned; continuing");
                    err.into_inner().0
                }
            };

            if state.generation != seen_generation {
                break;
            }
            drop(state);

            sync_codex_desktop_available_models_everywhere(&model_ids, true);
        }
    }
}

fn monitor_codex_desktop_available_models_cache(model_ids: Vec<String>) {
    let handle = codex_desktop_model_whitelist_sync_handle();
    let (lock, cvar) = &*handle;
    let mut state = match lock.lock() {
        Ok(state) => state,
        Err(err) => {
            log::warn!("Codex Desktop model whitelist sync state was poisoned; continuing");
            err.into_inner()
        }
    };
    state.model_ids = model_ids;
    state.generation = state.generation.wrapping_add(1);
    cvar.notify_one();
}

fn sync_codex_desktop_available_models_cache_from_settings(settings: &Value) {
    let model_ids = codex_model_ids_from_settings(settings);
    if model_ids.is_empty() {
        monitor_codex_desktop_available_models_cache(Vec::new());
        return;
    }

    sync_codex_desktop_available_models_everywhere(&model_ids, false);
    monitor_codex_desktop_available_models_cache(model_ids);
}

fn find_codex_model_template(catalog: &Value) -> Option<Value> {
    catalog
        .get("models")
        .and_then(|models| models.as_array())
        .and_then(|models| {
            models.iter().find(|model| {
                model.get("slug").and_then(|slug| slug.as_str())
                    == Some(CODEX_MODEL_CATALOG_TEMPLATE_SLUG)
            })
        })
        .cloned()
}

fn load_codex_model_template_from_cache() -> Result<Option<Value>, AppError> {
    let path = get_codex_config_dir().join("models_cache.json");
    if !path.exists() {
        return Ok(None);
    }

    let text = fs::read_to_string(&path).map_err(|e| AppError::io(&path, e))?;
    let catalog: Value = serde_json::from_str(&text).map_err(|e| AppError::json(&path, e))?;
    Ok(find_codex_model_template(&catalog))
}

/// Fixed candidates for locating the `codex` CLI when it is not on the process
/// PATH (common in GUI apps launched outside a terminal).
const CODEX_CLI_FIXED_CANDIDATES: &[&str] = &[
    "codex",                                // PATH (all platforms)
    "/opt/homebrew/bin/codex",              // macOS Apple Silicon Homebrew
    "/usr/local/bin/codex",                 // macOS Intel Homebrew / Linux
    "/home/linuxbrew/.linuxbrew/bin/codex", // Linux Homebrew
];

fn push_codex_cli_candidate(
    candidates: &mut Vec<PathBuf>,
    seen: &mut HashSet<String>,
    candidate: PathBuf,
) {
    let key = candidate.to_string_lossy().into_owned();
    if seen.insert(key) {
        candidates.push(candidate);
    }
}

fn push_existing_codex_cli_candidate(
    candidates: &mut Vec<PathBuf>,
    seen: &mut HashSet<String>,
    candidate: PathBuf,
) {
    if candidate.exists() {
        push_codex_cli_candidate(candidates, seen, candidate);
    }
}

fn push_codex_cli_candidates_from_version_dirs(
    candidates: &mut Vec<PathBuf>,
    seen: &mut HashSet<String>,
    versions_dir: PathBuf,
    suffix: &[&str],
) {
    let Ok(entries) = fs::read_dir(versions_dir) else {
        return;
    };

    let mut discovered = entries
        .filter_map(Result::ok)
        .map(|entry| {
            let mut candidate = entry.path();
            for component in suffix {
                candidate.push(component);
            }
            candidate
        })
        .filter(|candidate| candidate.exists())
        .collect::<Vec<_>>();

    // Prefer newer-looking version directories before older global installs.
    discovered.sort_by(|a, b| b.cmp(a));
    for candidate in discovered {
        push_codex_cli_candidate(candidates, seen, candidate);
    }
}

fn push_home_codex_cli_candidates(
    candidates: &mut Vec<PathBuf>,
    seen: &mut HashSet<String>,
    home: &Path,
) {
    for relative in [
        ".nvm/current/bin/codex",
        ".volta/bin/codex",
        ".asdf/shims/codex",
        ".local/share/mise/shims/codex",
        ".config/mise/shims/codex",
        ".local/bin/codex",
        ".npm-global/bin/codex",
        ".npm-packages/bin/codex",
        ".local/share/pnpm/codex",
        "Library/pnpm/codex",
    ] {
        push_existing_codex_cli_candidate(candidates, seen, home.join(relative));
    }

    push_codex_cli_candidates_from_version_dirs(
        candidates,
        seen,
        home.join(".nvm/versions/node"),
        &["bin", "codex"],
    );
    push_codex_cli_candidates_from_version_dirs(
        candidates,
        seen,
        home.join(".local/share/fnm/node-versions"),
        &["installation", "bin", "codex"],
    );
    push_codex_cli_candidates_from_version_dirs(
        candidates,
        seen,
        home.join("Library/Application Support/fnm/node-versions"),
        &["installation", "bin", "codex"],
    );
}

fn push_env_codex_cli_candidates(candidates: &mut Vec<PathBuf>, seen: &mut HashSet<String>) {
    for (env_key, suffix) in [
        ("NPM_CONFIG_PREFIX", &["bin", "codex"][..]),
        ("VOLTA_HOME", &["bin", "codex"][..]),
        ("ASDF_DATA_DIR", &["shims", "codex"][..]),
        ("MISE_DATA_DIR", &["shims", "codex"][..]),
        ("PNPM_HOME", &["codex"][..]),
    ] {
        let Some(prefix) = std::env::var_os(env_key) else {
            continue;
        };
        let mut candidate = PathBuf::from(prefix);
        for component in suffix {
            candidate.push(component);
        }
        push_existing_codex_cli_candidate(candidates, seen, candidate);
    }

    if let Some(nvm_dir) = std::env::var_os("NVM_DIR") {
        push_codex_cli_candidates_from_version_dirs(
            candidates,
            seen,
            PathBuf::from(nvm_dir).join("versions/node"),
            &["bin", "codex"],
        );
    }

    if let Some(fnm_dir) = std::env::var_os("FNM_DIR") {
        push_codex_cli_candidates_from_version_dirs(
            candidates,
            seen,
            PathBuf::from(fnm_dir).join("node-versions"),
            &["installation", "bin", "codex"],
        );
    }

    #[cfg(windows)]
    {
        if let Some(appdata) = std::env::var_os("APPDATA") {
            let npm_dir = PathBuf::from(appdata).join("npm");
            for name in ["codex.cmd", "codex.exe", "codex"] {
                push_existing_codex_cli_candidate(candidates, seen, npm_dir.join(name));
            }
        }
    }
}

fn codex_cli_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    let mut seen = HashSet::new();

    for candidate in CODEX_CLI_FIXED_CANDIDATES {
        push_codex_cli_candidate(&mut candidates, &mut seen, PathBuf::from(candidate));
    }

    push_env_codex_cli_candidates(&mut candidates, &mut seen);
    push_home_codex_cli_candidates(&mut candidates, &mut seen, &get_home_dir());

    candidates
}

fn load_codex_model_template_from_bundled() -> Result<Option<Value>, AppError> {
    for candidate in codex_cli_candidates() {
        let candidate_label = candidate.to_string_lossy();
        let output = match Command::new(&candidate)
            .args(["debug", "models", "--bundled"])
            .output()
        {
            Ok(output) => output,
            Err(err) => {
                log::debug!("failed to run `{candidate_label} debug models --bundled`: {err}");
                continue;
            }
        };

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            log::debug!("`{candidate_label} debug models --bundled` failed: {stderr}");
            continue;
        }

        let catalog: Value = match serde_json::from_slice(&output.stdout) {
            Ok(catalog) => catalog,
            Err(e) => {
                log::debug!(
                    "Failed to parse `{candidate_label} debug models --bundled` output: {e}"
                );
                continue;
            }
        };
        if let Some(template) = find_codex_model_template(&catalog) {
            return Ok(Some(template));
        }
    }

    Ok(None)
}

fn load_codex_model_template_static() -> Option<Value> {
    let text = include_str!("resources/gpt5_5_template.json");
    match serde_json::from_str(text) {
        Ok(template) => Some(template),
        Err(e) => {
            log::warn!("Failed to parse bundled gpt-5.5 template: {e}");
            None
        }
    }
}

fn load_codex_model_catalog_template() -> Result<Value, AppError> {
    // ① models_cache.json (created by Codex when it connects to OpenAI)
    if let Some(template) = load_codex_model_template_from_cache()? {
        return Ok(template);
    }
    // ② codex CLI (PATH + platform-specific common paths)
    if let Some(template) = load_codex_model_template_from_bundled()? {
        return Ok(template);
    }
    // ③ Static fallback bundled at compile time
    if let Some(template) = load_codex_model_template_static() {
        return Ok(template);
    }

    Err(AppError::Message(format!(
        "Codex model catalog template `{CODEX_MODEL_CATALOG_TEMPLATE_SLUG}` not found. Please start Codex once so models_cache.json is available, or ensure the `codex` CLI is on PATH."
    )))
}

fn codex_model_catalog_from_specs(specs: &[CodexCatalogModelSpec], template: &Value) -> Value {
    let entries: Vec<Value> = specs
        .iter()
        .enumerate()
        .map(|(index, spec)| {
            codex_catalog_model_entry(
                template,
                &spec.provider_id,
                &spec.model,
                &spec.display_name,
                spec.context_window,
                index,
            )
        })
        .collect();

    json!({ "models": entries })
}

fn codex_model_catalog_from_settings(
    settings: &Value,
    config_text: &str,
) -> Result<Option<Value>, AppError> {
    let specs = codex_catalog_model_specs(settings, config_text);
    if specs.is_empty() {
        return Ok(None);
    }

    let template = load_codex_model_catalog_template()?;
    Ok(Some(codex_model_catalog_from_specs(&specs, &template)))
}

fn set_codex_model_catalog_json_field(
    config_text: &str,
    catalog_path: Option<&Path>,
) -> Result<String, AppError> {
    let mut doc = config_text
        .parse::<DocumentMut>()
        .map_err(|e| AppError::Message(format!("Invalid Codex config.toml: {e}")))?;

    match catalog_path {
        Some(_) => {
            doc["model_catalog_json"] = toml_edit::value(CC_SWITCH_CODEX_MODEL_CATALOG_FILENAME);
        }
        None => {
            let should_remove = doc
                .get("model_catalog_json")
                .and_then(|item| item.as_str())
                .map(|path| {
                    Path::new(path).file_name().and_then(|name| name.to_str())
                        == Some(CC_SWITCH_CODEX_MODEL_CATALOG_FILENAME)
                })
                .unwrap_or(false);
            if should_remove {
                doc.as_table_mut().remove("model_catalog_json");
            }
        }
    }

    Ok(doc.to_string())
}

fn set_codex_openai_base_url_field(
    config_text: &str,
    base_url: Option<&str>,
) -> Result<String, AppError> {
    let mut doc = config_text
        .parse::<DocumentMut>()
        .map_err(|e| AppError::Message(format!("Invalid Codex config.toml: {e}")))?;

    match base_url.map(str::trim).filter(|value| !value.is_empty()) {
        Some(base_url) => {
            doc["openai_base_url"] = toml_edit::value(base_url);
        }
        None => {
            doc.as_table_mut().remove("openai_base_url");
        }
    }

    Ok(doc.to_string())
}

/// Generate Codex `model_catalog_json` from provider settings and inject/remove
/// the top-level TOML field that points Codex to the generated file.
pub fn prepare_codex_config_text_with_model_catalog(
    settings: &Value,
    config_text: &str,
) -> Result<String, AppError> {
    let catalog_path = get_codex_model_catalog_path();

    if let Some(catalog) = codex_model_catalog_from_settings(settings, config_text)? {
        let config_text = set_codex_model_catalog_json_field(config_text, Some(&catalog_path))?;
        let config_text = set_codex_openai_base_url_field(
            &config_text,
            extract_codex_base_url(&config_text).as_deref(),
        )?;
        write_json_file(&catalog_path, &catalog)?;
        Ok(config_text)
    } else {
        set_codex_model_catalog_json_field(config_text, None)
    }
}

/// Reverse of `prepare_codex_config_text_with_model_catalog`: read the
/// cc-switch–maintained catalog file referenced by `~/.codex/config.toml` and
/// convert it back into the simplified shape the frontend table uses:
/// `{ "models": [{ "model", "displayName"?, "contextWindow"? }, ...] }`.
///
/// We only reverse-parse catalogs whose `model_catalog_json` path is the
/// cc-switch–generated file (identified by filename
/// `cc-switch-model-catalog.json`). A user-managed external catalog file is
/// left alone — surfacing its richer structure as the simplified table would
/// be a downgrade we can't safely round-trip.
///
/// `displayName` and `contextWindow` are omitted from the returned entry when
/// the on-disk value matches the fallback that
/// `codex_model_catalog_from_settings` injects for unset inputs (slug for
/// display_name, `model_context_window` or 128_000 for context_window). This
/// preserves the "user left it blank" intent across round-trip; an unavoidable
/// edge case is that a user-typed value that happens to equal the fallback
/// will also collapse to blank, but the next save writes the same fallback so
/// behavior stays consistent.
///
/// All failure modes (missing file, parse error, no `model_catalog_json`,
/// entries without `slug`) collapse to `Ok(None)` so callers can treat this
/// as best-effort enrichment without making `read_live_settings` brittle.
pub fn read_codex_model_catalog_simplified_from_live() -> Result<Option<Value>, AppError> {
    let config_text = read_codex_config_text()?;
    let generated_path = get_codex_model_catalog_path();
    let Some(catalog_path) = resolve_cc_switch_catalog_path(&config_text, &generated_path) else {
        return Ok(None);
    };
    if !catalog_path.exists() {
        return Ok(None);
    }
    let Ok(catalog_text) = fs::read_to_string(&catalog_path) else {
        return Ok(None);
    };
    Ok(build_simplified_catalog_from_texts(
        &config_text,
        &catalog_text,
    ))
}

/// Given `config.toml` text, resolve the on-disk path of the cc-switch–owned
/// catalog file (returns `None` if `model_catalog_json` is absent or points at
/// a file we don't own). Relative paths fall back to `generated_path`.
pub(crate) fn resolve_cc_switch_catalog_path(
    config_text: &str,
    generated_path: &Path,
) -> Option<PathBuf> {
    if config_text.trim().is_empty() {
        return None;
    }
    let doc = config_text.parse::<DocumentMut>().ok()?;
    let catalog_path_str = doc
        .get("model_catalog_json")
        .and_then(|item| item.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())?;

    let referenced_path = Path::new(catalog_path_str);
    let is_cc_switch_owned = referenced_path.file_name().and_then(|name| name.to_str())
        == Some(CC_SWITCH_CODEX_MODEL_CATALOG_FILENAME);
    if !is_cc_switch_owned {
        return None;
    }

    if referenced_path.is_absolute() {
        Some(referenced_path.to_path_buf())
    } else {
        Some(generated_path.to_path_buf())
    }
}

/// Pure reverse-parsing core: convert Codex catalog JSON text back into the
/// frontend's simplified `{ models: [{ model, displayName?, contextWindow? }] }`
/// shape. Returns `None` when the catalog is unparseable, has no `models`
/// array, or yields zero valid entries.
fn build_simplified_catalog_from_texts(config_text: &str, catalog_text: &str) -> Option<Value> {
    let catalog: Value = serde_json::from_str(catalog_text).ok()?;
    let models = catalog.get("models").and_then(|m| m.as_array())?;

    let default_context_window =
        extract_codex_top_level_u64(config_text, "model_context_window").unwrap_or(128_000);

    let mut entries = Vec::with_capacity(models.len());
    for entry in models {
        let Some(model) = entry
            .get("slug")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
        else {
            continue;
        };

        let mut obj = serde_json::Map::new();
        obj.insert("model".to_string(), json!(model));

        if let Some(display_name) = entry
            .get("display_name")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty() && *s != model)
        {
            obj.insert("displayName".to_string(), json!(display_name));
        }

        if let Some(context_window) = entry
            .get("context_window")
            .and_then(|v| v.as_u64())
            .filter(|v| *v > 0 && *v != default_context_window)
        {
            obj.insert("contextWindow".to_string(), json!(context_window));
        }

        entries.push(Value::Object(obj));
    }

    if entries.is_empty() {
        return None;
    }

    Some(json!({ "models": entries }))
}

/// Decide the `config.toml` text to write during a takeover-off restore,
/// projecting the model catalog **only when `settings` carries an inline
/// `modelCatalog`**.
///
/// Restore feeds back a stored backup, and Codex backups come in two shapes that
/// need opposite handling:
///
/// - **Snapshot backup** (`read_codex_live_settings`): `{ auth, config }` with no
///   inline `modelCatalog`. Its `config.toml` text already carries whatever
///   `model_catalog_json` pointer existed at backup time, and the generated
///   catalog file on disk is untouched. Here we must keep the config **raw** —
///   running catalog projection would see "no specs" and strip the live pointer.
/// - **Provider-rebuilt backup** (`update_live_backup_from_provider`): the DB
///   provider's settings, i.e. `{ auth, config (no pointer), modelCatalog
///   (inline DB SSOT) }`. Here the pointer/catalog file must be (re)generated
///   from the inline `modelCatalog`, or the mapping is lost on restore.
///
/// Gating on the presence of the inline `modelCatalog` key routes each shape
/// correctly; an empty inline catalog still projects (and so correctly drops a
/// now-stale pointer), while an absent key leaves the text untouched. This is
/// **orthogonal to auth** — a provider-rebuilt backup can pair an inline
/// `modelCatalog` with empty `auth.json` (the API key living in the config's
/// `experimental_bearer_token`), so the caller must decide config projection
/// independently of whether it writes or deletes `auth.json`.
pub fn prepare_codex_live_config_text_with_optional_catalog(
    settings: &Value,
    config_text: &str,
) -> Result<String, AppError> {
    if settings.get("modelCatalog").is_some() {
        prepare_codex_config_text_with_model_catalog(settings, config_text)
    } else {
        Ok(config_text.to_string())
    }
}

pub fn write_codex_provider_live_with_catalog(
    settings: &Value,
    category: Option<&str>,
    auth: &Value,
    config_text: Option<&str>,
) -> Result<(), AppError> {
    let prepared_config = config_text
        .map(|text| prepare_codex_config_text_with_model_catalog(settings, text))
        .transpose()?;

    write_codex_live_for_provider(category, auth, prepared_config.as_deref())?;
    sync_codex_desktop_available_models_cache_from_settings(settings);

    Ok(())
}

/// Extract a provider-scoped `experimental_bearer_token` from Codex `config.toml`.
///
/// Mobile compat: third-party providers may store the API key inside
/// `[model_providers.<id>].experimental_bearer_token` while keeping the
/// user's ChatGPT login cache intact in `auth.json`. Falls back to the
/// top-level `experimental_bearer_token` when no active model provider is set.
pub fn extract_codex_experimental_bearer_token(config_text: &str) -> Option<String> {
    if !config_text.contains("experimental_bearer_token") {
        return None;
    }
    let doc = config_text.parse::<DocumentMut>().ok()?;
    let provider_id = active_codex_model_provider_id(&doc);

    let top_level_token = || {
        doc.get("experimental_bearer_token")
            .and_then(|item| item.as_str())
    };
    let token = match provider_id.as_deref() {
        Some(id) if is_custom_codex_model_provider_id(id) => doc
            .get("model_providers")
            .and_then(|item| item.as_table())
            .and_then(|table| table.get(id))
            .and_then(|item| item.as_table())
            .and_then(|table| table.get("experimental_bearer_token"))
            .and_then(|item| item.as_str())
            .or_else(top_level_token),
        Some(_) => top_level_token(),
        None => top_level_token(),
    };

    token
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(str::to_string)
}

fn set_codex_experimental_bearer_token(config_text: &str, token: &str) -> Result<String, AppError> {
    if config_text.trim().is_empty() {
        return Err(AppError::localized(
            "provider.codex.config.missing",
            "Codex 第三方供应商缺少 config.toml 配置，无法写入 bearer token",
            "Codex third-party provider is missing config.toml, cannot write bearer token",
        ));
    }

    let mut doc = config_text
        .parse::<DocumentMut>()
        .map_err(|e| AppError::Message(format!("Invalid Codex config.toml: {e}")))?;

    let Some(provider_id) = active_codex_model_provider_id(&doc) else {
        doc["experimental_bearer_token"] = toml_edit::value(token);
        return Ok(doc.to_string());
    };

    if !is_custom_codex_model_provider_id(&provider_id) {
        // Reserved Codex provider IDs are owned by the CLI. Keep third-party
        // bearer tokens at the top level so we do not shadow built-in tables.
        doc["experimental_bearer_token"] = toml_edit::value(token);
        return Ok(doc.to_string());
    }

    if let Some(model_providers) = doc
        .get_mut("model_providers")
        .and_then(|item| item.as_table_mut())
    {
        if let Some(provider_table) = model_providers
            .get_mut(provider_id.as_str())
            .and_then(|item| item.as_table_mut())
        {
            provider_table["experimental_bearer_token"] = toml_edit::value(token);
            return Ok(doc.to_string());
        }
    }

    doc["experimental_bearer_token"] = toml_edit::value(token);
    Ok(doc.to_string())
}

pub fn remove_codex_experimental_bearer_token_if(
    config_text: &str,
    predicate: impl Fn(&str) -> bool,
) -> Result<String, AppError> {
    if config_text.trim().is_empty() || !config_text.contains("experimental_bearer_token") {
        return Ok(config_text.to_string());
    }

    let mut doc = config_text
        .parse::<DocumentMut>()
        .map_err(|e| AppError::Message(format!("Invalid Codex config.toml: {e}")))?;

    if let Some(provider_id) = active_codex_model_provider_id(&doc) {
        if let Some(provider_table) = doc
            .get_mut("model_providers")
            .and_then(|item| item.as_table_mut())
            .and_then(|table| table.get_mut(provider_id.as_str()))
            .and_then(|item| item.as_table_mut())
        {
            let should_remove = provider_table
                .get("experimental_bearer_token")
                .and_then(|item| item.as_str())
                .map(str::trim)
                .is_some_and(&predicate);
            if should_remove {
                provider_table.remove("experimental_bearer_token");
            }
        }
    }

    let should_remove_top_level = doc
        .get("experimental_bearer_token")
        .and_then(|item| item.as_str())
        .map(str::trim)
        .is_some_and(&predicate);
    if should_remove_top_level {
        doc.as_table_mut().remove("experimental_bearer_token");
    }
    Ok(doc.to_string())
}

fn remove_codex_experimental_bearer_token(config_text: &str) -> Result<String, AppError> {
    remove_codex_experimental_bearer_token_if(config_text, |_| true)
}

/// Read the current Codex live settings as a `{ auth, config }` object.
///
/// Missing `auth.json` collapses to `{}` so a config-only third-party install
/// is still importable; both files empty is treated as "no live install".
pub fn read_codex_live_settings() -> Result<Value, AppError> {
    let auth_path = get_codex_auth_path();
    let auth_present = auth_path.exists();
    let auth: Value = if auth_present {
        read_json_file(&auth_path)?
    } else {
        json!({})
    };
    let cfg_text = read_and_validate_codex_config_text()?;
    if !auth_present && cfg_text.trim().is_empty() {
        return Err(AppError::localized(
            "codex.live.missing",
            "Codex 配置文件不存在",
            "Codex configuration is missing",
        ));
    }
    Ok(json!({ "auth": auth, "config": cfg_text }))
}

/// `[model_providers.custom]` entry that makes an official (ChatGPT OAuth)
/// provider behave like Codex's built-in `openai` entry while running under
/// the shared custom id: `requires_openai_auth` routes auth to the ChatGPT
/// login in `auth.json` (base_url then defaults to the official Codex
/// backend), `name = "OpenAI"` keeps Codex's `is_openai()` feature gates
/// (web search, remote compaction), and `supports_websockets` restores the
/// built-in default that custom entries otherwise lose.
fn codex_unified_official_provider_table() -> toml_edit::Table {
    let mut table = toml_edit::Table::new();
    table["name"] = toml_edit::value("OpenAI");
    table["requires_openai_auth"] = toml_edit::value(true);
    table["supports_websockets"] = toml_edit::value(true);
    table["wire_api"] = toml_edit::value("responses");
    table
}

fn table_matches_codex_unified_official_provider(table: &toml_edit::Table) -> bool {
    table.len() == 4
        && table.get("name").and_then(|item| item.as_str()) == Some("OpenAI")
        && table
            .get("requires_openai_auth")
            .and_then(|item| item.as_bool())
            == Some(true)
        && table
            .get("supports_websockets")
            .and_then(|item| item.as_bool())
            == Some(true)
        && table.get("wire_api").and_then(|item| item.as_str()) == Some("responses")
}

/// 统一 Codex 会话历史：把官方供应商的 live 配置改写为以共享的
/// `custom` model_provider 标识运行（认证仍走 `auth.json` 的 ChatGPT 登录），
/// 使开关开启后创建的官方会话与第三方会话共用同一个 resume 历史桶。
///
/// 两种情况拒绝注入、原样返回：
/// - 配置已有显式 `model_provider`：用户手工指定的路由不被覆盖；
/// - 配置已有形态不同的 `[model_providers.custom]` 表：设置 `model_provider`
///   会激活这张我们不认识的表（可能带第三方 base_url/token，会把 ChatGPT
///   OAuth 流量路由到错误后端），宁可让开关对该配置不生效。
pub fn inject_codex_unified_session_bucket(config_text: &str) -> Result<String, AppError> {
    let mut doc = config_text
        .parse::<DocumentMut>()
        .map_err(|e| AppError::Message(format!("Invalid Codex config.toml: {e}")))?;

    if doc.get("model_provider").is_some() {
        return Ok(config_text.to_string());
    }

    let existing_custom_conflicts = doc
        .get("model_providers")
        .and_then(|item| item.as_table())
        .and_then(|providers| providers.get(CC_SWITCH_CODEX_MODEL_PROVIDER_ID))
        .and_then(|item| item.as_table())
        .is_some_and(|table| !table_matches_codex_unified_official_provider(table));
    if existing_custom_conflicts {
        log::warn!(
            "官方 Codex 配置已存在自定义 [model_providers.custom]，跳过统一会话路由注入以避免激活未知路由"
        );
        return Ok(config_text.to_string());
    }

    doc["model_provider"] = toml_edit::value(CC_SWITCH_CODEX_MODEL_PROVIDER_ID);

    if doc.get("model_providers").is_none() {
        let mut parent = toml_edit::Table::new();
        parent.set_implicit(true);
        doc["model_providers"] = toml_edit::Item::Table(parent);
    }
    if let Some(providers) = doc["model_providers"].as_table_mut() {
        if !providers.contains_key(CC_SWITCH_CODEX_MODEL_PROVIDER_ID) {
            providers.insert(
                CC_SWITCH_CODEX_MODEL_PROVIDER_ID,
                toml_edit::Item::Table(codex_unified_official_provider_table()),
            );
        }
    }
    Ok(doc.to_string())
}

/// `inject_codex_unified_session_bucket` 的反向操作：从配置文本里剥掉注入的
/// 统一会话路由，保证切换回填不会把它带进数据库的存储配置（关闭开关后
/// 切换即可完全还原）。仅当形态与注入产物完全一致时才剥离；第三方模板和
/// 用户自定义的 `custom` 条目（带 base_url 等差异字段）原样保留。
pub fn strip_codex_unified_session_bucket(config_text: &str) -> Result<String, AppError> {
    if !config_text.contains("model_provider") {
        return Ok(config_text.to_string());
    }
    let mut doc = config_text
        .parse::<DocumentMut>()
        .map_err(|e| AppError::Message(format!("Invalid Codex config.toml: {e}")))?;

    if doc.get("model_provider").and_then(|item| item.as_str())
        != Some(CC_SWITCH_CODEX_MODEL_PROVIDER_ID)
    {
        return Ok(config_text.to_string());
    }
    let matches_injected = doc
        .get("model_providers")
        .and_then(|item| item.as_table())
        .and_then(|providers| providers.get(CC_SWITCH_CODEX_MODEL_PROVIDER_ID))
        .and_then(|item| item.as_table())
        .is_some_and(table_matches_codex_unified_official_provider);
    if !matches_injected {
        return Ok(config_text.to_string());
    }

    doc.as_table_mut().remove("model_provider");
    let providers_empty = doc["model_providers"]
        .as_table_mut()
        .map(|providers| {
            providers.remove(CC_SWITCH_CODEX_MODEL_PROVIDER_ID);
            providers.is_empty()
        })
        .unwrap_or(false);
    if providers_empty {
        doc.as_table_mut().remove("model_providers");
    }
    Ok(doc.to_string())
}

/// 统一会话开关开启时，把官方供应商 `{ auth, config }` 设置对象中的
/// config 文本注入共享 custom 路由；开关关闭或非官方供应商时不做改动。
///
/// 普通 live 写入（`write_codex_live_for_provider`）与代理接管备份
/// （`update_live_backup_from_provider`）两条落盘路径共用：接管期间
/// live 归代理所有，注入必须进备份，接管释放恢复的 live 才带统一路由。
pub fn apply_codex_unified_session_bucket_to_settings(
    category: Option<&str>,
    settings: &mut Value,
) -> Result<(), AppError> {
    if category != Some("official") || !crate::settings::unify_codex_session_history() {
        return Ok(());
    }
    let config_text = settings
        .get("config")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string();
    let injected = inject_codex_unified_session_bucket(&config_text)?;
    if injected != config_text {
        if let Some(obj) = settings.as_object_mut() {
            obj.insert("config".to_string(), Value::String(injected));
        }
    }
    Ok(())
}

/// Backfill helper: strip the unified-session injection from a live
/// `{ auth, config }` settings object before it is stored back to the DB.
pub fn strip_codex_unified_session_bucket_from_settings(
    settings: &mut Value,
) -> Result<(), AppError> {
    let Some(config_text) = settings
        .get("config")
        .and_then(|value| value.as_str())
        .map(str::to_string)
    else {
        return Ok(());
    };
    let stripped = strip_codex_unified_session_bucket(&config_text)?;
    if stripped != config_text {
        if let Some(obj) = settings.as_object_mut() {
            obj.insert("config".to_string(), Value::String(stripped));
        }
    }
    Ok(())
}

/// Route a Codex live write between full auth+config or config-only.
///
/// Official providers with usable login material own `auth.json`. Third-party
/// providers only touch `config.toml` when the compatibility setting is enabled
/// so the user's ChatGPT login cache survives provider switches.
///
/// 统一会话开关开启时，官方配置在落盘前注入共享的 `custom` 路由
/// （见 `inject_codex_unified_session_bucket`）。
pub fn write_codex_live_for_provider(
    category: Option<&str>,
    auth: &Value,
    config_text: Option<&str>,
) -> Result<(), AppError> {
    let unified_official_config =
        if category == Some("official") && crate::settings::unify_codex_session_history() {
            Some(inject_codex_unified_session_bucket(
                config_text.unwrap_or(""),
            )?)
        } else {
            None
        };
    let config_text = unified_official_config.as_deref().or(config_text);

    let should_write_auth = (category == Some("official") && codex_auth_has_login_material(auth))
        || (category != Some("official")
            && !crate::settings::preserve_codex_official_auth_on_switch());

    if should_write_auth {
        write_codex_live_atomic(auth, config_text)
    } else {
        let live_config = prepare_codex_provider_live_config(auth, config_text.unwrap_or(""))?;
        write_codex_live_config_atomic(Some(&live_config))
    }
}

/// Build the live Codex config for provider switching.
///
/// The stored provider keeps its API key in `auth.OPENAI_API_KEY`. Live Codex
/// requests can use a provider-scoped `experimental_bearer_token`, so switching
/// providers only needs to update `config.toml`; `auth.json` stays as the user's
/// long-lived ChatGPT login cache.
pub fn prepare_codex_provider_live_config(
    auth: &Value,
    config_text: &str,
) -> Result<String, AppError> {
    let token = extract_codex_auth_api_key(auth)
        .or_else(|| extract_codex_experimental_bearer_token(config_text));

    Ok(match token {
        Some(token) => set_codex_experimental_bearer_token(config_text, &token)?,
        None => config_text.to_string(),
    })
}

/// During DB backfill, lift a live `experimental_bearer_token` back into
/// `auth.OPENAI_API_KEY` so the stored provider keeps its canonical shape
/// and generated live tokens don't leak into stored provider TOML.
///
/// Only intervenes when the live config actually carries a bearer token —
/// otherwise the function is a no-op so the caller's normal backfill path
/// (which keeps live `auth` as the authoritative source) is unaffected.
pub fn restore_codex_provider_token_for_backfill(
    settings: &mut Value,
    template_settings: &Value,
) -> Result<(), AppError> {
    let Some(config_text) = settings
        .get("config")
        .and_then(|value| value.as_str())
        .map(str::to_string)
    else {
        return Ok(());
    };

    let Some(token) = extract_codex_experimental_bearer_token(&config_text) else {
        return Ok(());
    };

    let cleaned_config = remove_codex_experimental_bearer_token(&config_text)?;

    if let Some(obj) = settings.as_object_mut() {
        obj.insert("config".to_string(), Value::String(cleaned_config));

        let mut auth = template_settings
            .get("auth")
            .filter(|value| value.is_object())
            .cloned()
            .unwrap_or_else(|| Value::Object(serde_json::Map::new()));
        if let Some(auth_obj) = auth.as_object_mut() {
            auth_obj.insert("OPENAI_API_KEY".to_string(), Value::String(token));
        }
        obj.insert("auth".to_string(), auth);
    }

    Ok(())
}

pub fn restore_codex_settings_for_backfill(
    settings: &mut Value,
    template_settings: &Value,
    restore_provider_token: bool,
) -> Result<(), AppError> {
    if restore_provider_token {
        restore_codex_provider_token_for_backfill(settings, template_settings)?;
    }
    Ok(())
}

/// Update a field in Codex config.toml using toml_edit (syntax-preserving).
///
/// Supported fields:
/// - `"base_url"`: writes to `[model_providers.<current>].base_url` if `model_provider` exists,
///   otherwise falls back to top-level `base_url`.
/// - `"wire_api"`: writes to `[model_providers.<current>].wire_api` if `model_provider` exists,
///   otherwise falls back to top-level `wire_api`.
/// - `"model"` / `"model_catalog_json"`: writes to top-level field.
///
/// Empty value removes the field.
pub fn update_codex_toml_field(toml_str: &str, field: &str, value: &str) -> Result<String, String> {
    let mut doc = toml_str
        .parse::<DocumentMut>()
        .map_err(|e| format!("TOML parse error: {e}"))?;

    let trimmed = value.trim();

    match field {
        "base_url" | "wire_api" => {
            let model_provider = doc
                .get("model_provider")
                .and_then(|item| item.as_str())
                .map(str::to_string);

            if let Some(provider_key) = model_provider {
                // Ensure [model_providers] table exists
                if doc.get("model_providers").is_none() {
                    doc["model_providers"] = toml_edit::table();
                }

                if let Some(model_providers) = doc["model_providers"].as_table_mut() {
                    // Ensure [model_providers.<provider_key>] table exists
                    if !model_providers.contains_key(&provider_key) {
                        model_providers[&provider_key] = toml_edit::table();
                    }

                    if let Some(provider_table) = model_providers[&provider_key].as_table_mut() {
                        if trimmed.is_empty() {
                            provider_table.remove(field);
                        } else {
                            provider_table[field] = toml_edit::value(trimmed);
                        }
                        return Ok(doc.to_string());
                    }
                }
            }

            // Fallback: no model_provider or structure mismatch → top-level field
            if trimmed.is_empty() {
                doc.as_table_mut().remove(field);
            } else {
                doc[field] = toml_edit::value(trimmed);
            }
        }
        "model" | "model_catalog_json" => {
            if trimmed.is_empty() {
                doc.as_table_mut().remove(field);
            } else {
                doc[field] = toml_edit::value(trimmed);
            }
        }
        _ => return Err(format!("unsupported field: {field}")),
    }

    Ok(doc.to_string())
}

/// Remove `base_url` from the active model_provider section only if it matches `predicate`.
/// Also removes top-level `base_url` if it matches.
/// Used by proxy cleanup to strip local proxy URLs without touching user-configured URLs.
pub fn remove_codex_toml_base_url_if(toml_str: &str, predicate: impl Fn(&str) -> bool) -> String {
    let mut doc = match toml_str.parse::<DocumentMut>() {
        Ok(doc) => doc,
        Err(_) => return toml_str.to_string(),
    };

    let model_provider = doc
        .get("model_provider")
        .and_then(|item| item.as_str())
        .map(str::to_string);

    if let Some(provider_key) = model_provider {
        if let Some(model_providers) = doc
            .get_mut("model_providers")
            .and_then(|v| v.as_table_mut())
        {
            if let Some(provider_table) = model_providers
                .get_mut(provider_key.as_str())
                .and_then(|v| v.as_table_mut())
            {
                let should_remove = provider_table
                    .get("base_url")
                    .and_then(|item| item.as_str())
                    .map(&predicate)
                    .unwrap_or(false);
                if should_remove {
                    provider_table.remove("base_url");
                }
            }
        }
    }

    // Fallback: also clean up top-level base_url if it matches
    let should_remove_root = doc
        .get("base_url")
        .and_then(|item| item.as_str())
        .map(&predicate)
        .unwrap_or(false);
    if should_remove_root {
        doc.as_table_mut().remove("base_url");
    }

    doc.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn unified_session_bucket_injects_for_empty_official_config() {
        let injected = inject_codex_unified_session_bucket("").expect("inject");
        let doc: toml::Table = toml::from_str(&injected).expect("parse injected config");

        assert_eq!(
            doc.get("model_provider").and_then(|v| v.as_str()),
            Some(CC_SWITCH_CODEX_MODEL_PROVIDER_ID)
        );
        let custom = doc["model_providers"][CC_SWITCH_CODEX_MODEL_PROVIDER_ID]
            .as_table()
            .expect("custom provider table");
        assert_eq!(custom.get("name").and_then(|v| v.as_str()), Some("OpenAI"));
        assert_eq!(
            custom.get("requires_openai_auth").and_then(|v| v.as_bool()),
            Some(true)
        );
        assert_eq!(
            custom.get("supports_websockets").and_then(|v| v.as_bool()),
            Some(true)
        );
        assert_eq!(
            custom.get("wire_api").and_then(|v| v.as_str()),
            Some("responses")
        );
    }

    #[test]
    fn unified_session_bucket_preserves_other_keys_and_explicit_routing() {
        let with_catalog = "model_catalog_json = \"cc-switch-model-catalog.json\"\n";
        let injected = inject_codex_unified_session_bucket(with_catalog).expect("inject");
        assert!(injected.contains("model_catalog_json"));
        assert!(injected.contains("model_provider = \"custom\""));

        // 用户显式指定过 model_provider 的官方配置不被覆盖
        let explicit = "model_provider = \"openai_https\"\n";
        let unchanged = inject_codex_unified_session_bucket(explicit).expect("inject");
        assert_eq!(unchanged, explicit);
    }

    #[test]
    fn unified_session_bucket_skips_conflicting_custom_table() {
        // 残留的非注入形态 custom 表：设置 model_provider 会把官方流量
        // 路由到表里的第三方端点，必须整体拒绝注入。
        let stale = r#"[model_providers.custom]
name = "Relay"
base_url = "https://relay.example/v1"
"#;
        let unchanged = inject_codex_unified_session_bucket(stale).expect("inject");
        assert_eq!(unchanged, stale);

        // 已是注入形态的 custom 表（如重复注入）则照常补上 model_provider
        let injected_once = inject_codex_unified_session_bucket("").expect("inject");
        let reinjected = inject_codex_unified_session_bucket(&injected_once).expect("re-inject");
        assert_eq!(reinjected, injected_once);
    }

    #[test]
    fn unified_session_bucket_strip_round_trips_injection() {
        let injected = inject_codex_unified_session_bucket("").expect("inject");
        let stripped = strip_codex_unified_session_bucket(&injected).expect("strip");
        assert_eq!(stripped.trim(), "");

        let with_catalog = "model_catalog_json = \"cc-switch-model-catalog.json\"\n";
        let injected = inject_codex_unified_session_bucket(with_catalog).expect("inject");
        let stripped = strip_codex_unified_session_bucket(&injected).expect("strip");
        assert_eq!(stripped, with_catalog);
    }

    #[test]
    fn unified_session_bucket_strip_keeps_third_party_custom_entry() {
        // 第三方模板同样用 custom 路由，但条目带 base_url 等差异字段，
        // 形态不等于注入产物，必须原样保留。
        let third_party = r#"model_provider = "custom"

[model_providers.custom]
name = "Relay"
base_url = "https://relay.example/v1"
wire_api = "responses"
requires_openai_auth = true
"#;
        let untouched = strip_codex_unified_session_bucket(third_party).expect("strip");
        assert_eq!(untouched, third_party);
    }

    #[test]
    fn unified_session_bucket_strip_from_settings_only_touches_config() {
        let injected = inject_codex_unified_session_bucket("").expect("inject");
        let mut settings = json!({
            "auth": { "tokens": { "access_token": "secret" } },
            "config": injected,
        });
        strip_codex_unified_session_bucket_from_settings(&mut settings).expect("strip settings");
        assert_eq!(
            settings
                .get("config")
                .and_then(|v| v.as_str())
                .map(str::trim),
            Some("")
        );
        assert!(settings.pointer("/auth/tokens/access_token").is_some());
    }

    #[test]
    fn extract_base_url_prefers_active_provider_section() {
        let input = r#"model_provider = "azure"

[model_providers.azure]
base_url = "https://azure.example.com/v1"

[model_providers.other]
base_url = "https://other.example.com/v1"
"#;

        assert_eq!(
            extract_codex_base_url(input).as_deref(),
            Some("https://azure.example.com/v1")
        );
    }

    #[test]
    fn extract_base_url_falls_back_to_top_level_only() {
        let top_level = r#"base_url = "https://top-level.example.com/v1""#;
        assert_eq!(
            extract_codex_base_url(top_level).as_deref(),
            Some("https://top-level.example.com/v1")
        );
    }

    // Mirrors the frontend extractCodexBaseUrl: a non-active provider section
    // is never a credential source, whether the active provider points
    // elsewhere (e.g. the built-in "openai") or none is selected at all.
    #[test]
    fn extract_base_url_ignores_non_active_provider_sections() {
        let mismatched = r#"model_provider = "openai"

[model_providers.custom]
base_url = "https://leftover.example.com/v1"
"#;
        assert_eq!(extract_codex_base_url(mismatched), None);

        let no_active = r#"[model_providers.any]
base_url = "https://single.example.com/v1"
"#;
        assert_eq!(extract_codex_base_url(no_active), None);
    }

    #[test]
    fn prepare_provider_live_config_rejects_key_without_config() {
        let err = prepare_codex_provider_live_config(&json!({"OPENAI_API_KEY": "sk-test"}), "")
            .expect_err("empty config with API key should not truncate live config");

        assert!(
            err.to_string().contains("config.toml"),
            "error should explain missing config.toml, got: {err}"
        );
    }

    #[test]
    fn prepare_provider_live_config_uses_top_level_token_for_reserved_provider() {
        let input = r#"model_provider = "openai"
model = "gpt-5"
"#;

        let output =
            prepare_codex_provider_live_config(&json!({"OPENAI_API_KEY": "sk-test"}), input)
                .expect("prepare live config");
        let parsed: toml::Value = toml::from_str(&output).expect("parse output");

        assert_eq!(
            parsed
                .get("experimental_bearer_token")
                .and_then(|v| v.as_str()),
            Some("sk-test")
        );
        assert!(
            parsed.get("model_providers").is_none(),
            "reserved provider tables should not be synthesized"
        );
    }

    #[test]
    fn extract_bearer_uses_top_level_token_for_reserved_provider() {
        let input = r#"model_provider = "openai"
experimental_bearer_token = "top-level-key"

[model_providers.openai]
experimental_bearer_token = "stale-table-key"
"#;

        assert_eq!(
            extract_codex_experimental_bearer_token(input).as_deref(),
            Some("top-level-key")
        );
    }

    #[test]
    fn should_not_restore_provider_token_for_oauth_only_template() {
        let oauth_template = json!({
            "auth": {
                "auth_mode": "chatgpt",
                "tokens": {
                    "access_token": "oauth-access"
                }
            }
        });
        let api_key_template = json!({
            "auth": {
                "OPENAI_API_KEY": "sk-test"
            }
        });

        assert!(
            !should_restore_codex_provider_token_for_backfill(Some("custom"), &oauth_template),
            "OAuth-only templates should not backfill bearer tokens into OPENAI_API_KEY"
        );
        assert!(
            should_restore_codex_provider_token_for_backfill(Some("custom"), &api_key_template),
            "custom API-key providers should still restore provider bearer tokens"
        );
        assert!(
            !should_restore_codex_provider_token_for_backfill(Some("official"), &api_key_template),
            "official providers should never restore third-party bearer tokens"
        );
    }

    #[test]
    fn prepare_provider_live_config_does_not_create_incomplete_provider_table() {
        let input = r#"model_provider = "vendor_x"
model = "gpt-5"
"#;

        let output =
            prepare_codex_provider_live_config(&json!({"OPENAI_API_KEY": "sk-test"}), input)
                .expect("prepare live config");
        let parsed: toml::Value = toml::from_str(&output).expect("parse output");

        assert_eq!(
            parsed
                .get("experimental_bearer_token")
                .and_then(|v| v.as_str()),
            Some("sk-test")
        );
        assert!(
            parsed.get("model_providers").is_none(),
            "missing provider tables should not be synthesized without endpoint fields"
        );
    }

    #[test]
    fn prepare_provider_live_config_preserves_custom_provider_id() {
        let input = r#"model_provider = "vendor_alpha"
model = "gpt-5.4"
profile = "work"

[model_providers.vendor_alpha]
name = "Vendor Alpha"
base_url = "https://alpha.example/v1"
wire_api = "responses"

[profiles.work]
model_provider = "vendor_alpha"
model = "gpt-5.4"
"#;

        let result =
            prepare_codex_provider_live_config(&json!({"OPENAI_API_KEY": "sk-test"}), input)
                .expect("prepare live config");
        let parsed: toml::Value = toml::from_str(&result).unwrap();

        assert_eq!(
            parsed.get("model_provider").and_then(|v| v.as_str()),
            Some("vendor_alpha")
        );
        assert!(
            parsed
                .get("model_providers")
                .and_then(|v| v.get("custom"))
                .is_none(),
            "provider writes should not force custom provider ids"
        );
        assert_eq!(
            parsed
                .get("model_providers")
                .and_then(|v| v.get("vendor_alpha"))
                .and_then(|v| v.get("experimental_bearer_token"))
                .and_then(|v| v.as_str()),
            Some("sk-test")
        );
        assert_eq!(
            parsed
                .get("profiles")
                .and_then(|v| v.get("work"))
                .and_then(|v| v.get("model_provider"))
                .and_then(|v| v.as_str()),
            Some("vendor_alpha"),
            "profile provider references should be preserved"
        );
    }

    #[test]
    fn backfill_preserves_live_model_provider_id() {
        let mut live_settings = json!({
            "auth": {},
            "config": r#"model_provider = "vendor_beta"

[model_providers.vendor_beta]
name = "Vendor Beta"
base_url = "https://beta.example/v1"
wire_api = "responses"
"#,
        });
        let template_settings = json!({
            "auth": {},
            "config": r#"model_provider = "custom"

[model_providers.custom]
name = "Custom"
base_url = "https://custom.example/v1"
wire_api = "responses"
"#,
        });

        restore_codex_settings_for_backfill(&mut live_settings, &template_settings, false).unwrap();
        let config = live_settings.get("config").and_then(Value::as_str).unwrap();
        let parsed: toml::Value = toml::from_str(config).unwrap();

        assert_eq!(
            parsed.get("model_provider").and_then(|v| v.as_str()),
            Some("vendor_beta")
        );
        assert!(
            parsed
                .get("model_providers")
                .and_then(|v| v.get("vendor_beta"))
                .is_some(),
            "backfill should not rewrite user-selected provider tables"
        );
    }

    #[test]
    fn base_url_writes_into_correct_model_provider_section() {
        let input = r#"model_provider = "any"
model = "gpt-5.1-codex"

[model_providers.any]
name = "any"
wire_api = "responses"
"#;

        let result = update_codex_toml_field(input, "base_url", "https://example.com/v1").unwrap();
        let parsed: toml::Value = toml::from_str(&result).unwrap();

        let base_url = parsed
            .get("model_providers")
            .and_then(|v| v.get("any"))
            .and_then(|v| v.get("base_url"))
            .and_then(|v| v.as_str())
            .expect("base_url should be in model_providers.any");
        assert_eq!(base_url, "https://example.com/v1");

        // Should NOT have top-level base_url
        assert!(parsed.get("base_url").is_none());

        // wire_api preserved
        let wire_api = parsed
            .get("model_providers")
            .and_then(|v| v.get("any"))
            .and_then(|v| v.get("wire_api"))
            .and_then(|v| v.as_str());
        assert_eq!(wire_api, Some("responses"));
    }

    #[test]
    fn wire_api_writes_into_correct_model_provider_section() {
        let input = r#"model_provider = "chat_only"
model = "gpt-5.1-codex"

[model_providers.chat_only]
name = "Chat Only"
base_url = "https://example.com/v1"
wire_api = "chat"
"#;

        let result = update_codex_toml_field(input, "wire_api", "responses").unwrap();
        let parsed: toml::Value = toml::from_str(&result).unwrap();

        let provider = parsed
            .get("model_providers")
            .and_then(|v| v.get("chat_only"))
            .expect("model_providers.chat_only should exist");

        assert_eq!(
            provider.get("wire_api").and_then(|v| v.as_str()),
            Some("responses")
        );
        assert_eq!(
            provider.get("base_url").and_then(|v| v.as_str()),
            Some("https://example.com/v1")
        );
        assert!(parsed.get("wire_api").is_none());
    }

    #[test]
    fn base_url_creates_section_when_missing() {
        let input = r#"model_provider = "custom"
model = "gpt-4"
"#;

        let result = update_codex_toml_field(input, "base_url", "https://custom.api/v1").unwrap();
        let parsed: toml::Value = toml::from_str(&result).unwrap();

        let base_url = parsed
            .get("model_providers")
            .and_then(|v| v.get("custom"))
            .and_then(|v| v.get("base_url"))
            .and_then(|v| v.as_str())
            .expect("should create section and set base_url");
        assert_eq!(base_url, "https://custom.api/v1");
    }

    #[test]
    fn base_url_falls_back_to_top_level_without_model_provider() {
        let input = r#"model = "gpt-4"
"#;

        let result = update_codex_toml_field(input, "base_url", "https://fallback.api/v1").unwrap();
        let parsed: toml::Value = toml::from_str(&result).unwrap();

        let base_url = parsed
            .get("base_url")
            .and_then(|v| v.as_str())
            .expect("should set top-level base_url");
        assert_eq!(base_url, "https://fallback.api/v1");
    }

    #[test]
    fn clearing_base_url_removes_only_from_correct_section() {
        let input = r#"model_provider = "any"

[model_providers.any]
name = "any"
base_url = "https://old.api/v1"
wire_api = "responses"

[mcp_servers.context7]
command = "npx"
"#;

        let result = update_codex_toml_field(input, "base_url", "").unwrap();
        let parsed: toml::Value = toml::from_str(&result).unwrap();

        // base_url removed from model_providers.any
        let any_section = parsed
            .get("model_providers")
            .and_then(|v| v.get("any"))
            .expect("model_providers.any should exist");
        assert!(any_section.get("base_url").is_none());

        // wire_api preserved
        assert_eq!(
            any_section.get("wire_api").and_then(|v| v.as_str()),
            Some("responses")
        );

        // mcp_servers untouched
        assert!(parsed.get("mcp_servers").is_some());
    }

    #[test]
    fn model_field_operates_on_top_level() {
        let input = r#"model_provider = "any"
model = "gpt-4"

[model_providers.any]
name = "any"
"#;

        let result = update_codex_toml_field(input, "model", "gpt-5").unwrap();
        let parsed: toml::Value = toml::from_str(&result).unwrap();
        assert_eq!(parsed.get("model").and_then(|v| v.as_str()), Some("gpt-5"));

        // Clear model
        let result2 = update_codex_toml_field(&result, "model", "").unwrap();
        let parsed2: toml::Value = toml::from_str(&result2).unwrap();
        assert!(parsed2.get("model").is_none());
    }

    #[test]
    fn preserves_comments_and_whitespace() {
        let input = r#"# My Codex config
model_provider = "any"
model = "gpt-4"

# Provider section
[model_providers.any]
name = "any"
base_url = "https://old.api/v1"
"#;

        let result = update_codex_toml_field(input, "base_url", "https://new.api/v1").unwrap();

        // Comments should be preserved
        assert!(result.contains("# My Codex config"));
        assert!(result.contains("# Provider section"));
    }

    #[test]
    fn does_not_misplace_when_profiles_section_follows() {
        let input = r#"model_provider = "any"

[model_providers.any]
name = "any"
base_url = "https://old.api/v1"

[profiles.default]
model = "gpt-4"
"#;

        let result = update_codex_toml_field(input, "base_url", "https://new.api/v1").unwrap();
        let parsed: toml::Value = toml::from_str(&result).unwrap();

        // base_url in correct section
        let base_url = parsed
            .get("model_providers")
            .and_then(|v| v.get("any"))
            .and_then(|v| v.get("base_url"))
            .and_then(|v| v.as_str());
        assert_eq!(base_url, Some("https://new.api/v1"));

        // profiles section untouched
        let profile_model = parsed
            .get("profiles")
            .and_then(|v| v.get("default"))
            .and_then(|v| v.get("model"))
            .and_then(|v| v.as_str());
        assert_eq!(profile_model, Some("gpt-4"));
    }

    #[test]
    fn remove_base_url_if_predicate() {
        let input = r#"model_provider = "any"

[model_providers.any]
name = "any"
base_url = "http://127.0.0.1:5000/v1"
wire_api = "responses"
"#;

        let result =
            remove_codex_toml_base_url_if(input, |url| url.starts_with("http://127.0.0.1"));
        let parsed: toml::Value = toml::from_str(&result).unwrap();

        let any_section = parsed
            .get("model_providers")
            .and_then(|v| v.get("any"))
            .unwrap();
        assert!(any_section.get("base_url").is_none());
        assert_eq!(
            any_section.get("wire_api").and_then(|v| v.as_str()),
            Some("responses")
        );
    }

    #[test]
    fn remove_base_url_if_keeps_non_matching() {
        let input = r#"model_provider = "any"

[model_providers.any]
base_url = "https://production.api/v1"
"#;

        let result =
            remove_codex_toml_base_url_if(input, |url| url.starts_with("http://127.0.0.1"));
        let parsed: toml::Value = toml::from_str(&result).unwrap();

        let base_url = parsed
            .get("model_providers")
            .and_then(|v| v.get("any"))
            .and_then(|v| v.get("base_url"))
            .and_then(|v| v.as_str());
        assert_eq!(base_url, Some("https://production.api/v1"));
    }

    #[test]
    fn codex_model_catalog_uses_provider_models_and_context() {
        let template = json!({
            "slug": "gpt-5.5",
            "display_name": "GPT-5.5",
            "description": "Frontier model",
            "base_instructions": "gpt-5.5 base instructions",
            "model_messages": {
                "instructions_template": "gpt-5.5 instructions template",
                "instructions_variables": {
                    "personality_default": "",
                    "personality_friendly": "",
                    "personality_pragmatic": ""
                }
            },
            "additional_speed_tiers": ["fast"],
            "service_tiers": [
                {
                    "id": "priority",
                    "name": "Fast",
                    "description": "1.5x speed, increased usage"
                }
            ],
            "availability_nux": {
                "message": "GPT-5.5 is now available."
            },
            "upgrade": {
                "target": "gpt-5.5"
            },
            "context_window": 272000,
            "max_context_window": 272000
        });
        let settings = json!({
            "modelCatalog": {
                "models": [
                    {
                        "model": "deepseek-v4-flash",
                        "displayName": "DeepSeek V4 Flash",
                        "contextWindow": "64000"
                    },
                    {
                        "model": "kimi-k2",
                        "display_name": "Kimi K2"
                    }
                ]
            }
        });
        let specs = codex_catalog_model_specs(
            &settings,
            r#"model_provider = "custom"
model_context_window = 128000
"#,
        );
        let catalog = codex_model_catalog_from_specs(&specs, &template);
        let models = catalog
            .get("models")
            .and_then(|value| value.as_array())
            .expect("models should be an array");

        assert_eq!(models.len(), 2);
        assert_eq!(
            models[0].get("slug").and_then(|value| value.as_str()),
            Some("deepseek-v4-flash")
        );
        assert_eq!(
            models[0].get("model").and_then(|value| value.as_str()),
            Some("deepseek-v4-flash")
        );
        assert_eq!(
            models[0].get("provider").and_then(|value| value.as_str()),
            Some("custom")
        );
        assert_eq!(
            models[0]
                .get("backend_provider")
                .and_then(|value| value.as_str()),
            Some("custom")
        );
        assert_eq!(
            models[0]
                .get("minimal_client_version")
                .and_then(|value| value.as_str()),
            Some("0.0.1")
        );
        assert_eq!(
            models[0]
                .get("available_in_plans")
                .and_then(|value| value.as_array())
                .map(Vec::len),
            Some(6)
        );
        assert_eq!(
            models[0]
                .get("context_window")
                .and_then(|value| value.as_u64()),
            Some(64_000)
        );
        assert_eq!(
            models[1]
                .get("context_window")
                .and_then(|value| value.as_u64()),
            Some(128_000)
        );
        assert!(
            models[0].get("model_messages").is_some(),
            "Codex requires model_messages in custom catalogs"
        );
        assert_eq!(
            models[0]
                .get("base_instructions")
                .and_then(|value| value.as_str()),
            Some("gpt-5.5 base instructions")
        );
        assert_eq!(
            models[0].get("model_messages"),
            template.get("model_messages"),
            "custom catalog entries should keep the gpt-5.5 agent template"
        );
        assert_eq!(
            models[0].get("additional_speed_tiers"),
            Some(&json!([])),
            "generated third-party entries should not inherit OpenAI speed tiers"
        );
        assert!(
            models[0]
                .get("availability_nux")
                .is_some_and(|value| value.is_null()),
            "generated third-party entries should not inherit GPT-5.5 launch messaging"
        );
    }

    #[test]
    fn codex_desktop_statsig_merge_appends_models_without_duplicates() {
        let data = json!({
            "dynamic_configs": {
                CODEX_DESKTOP_STATSIG_MODELS_CONFIG_ID: {
                    "value": {
                        "available_models": [
                            "gpt-5.1-codex-max",
                            "gpt-5.1-codex",
                            "deepseek-v4-flash"
                        ]
                    }
                }
            }
        });
        let mut wrapper = json!({
            "source": "NetworkNotModified",
            "data": data.to_string()
        });
        let models = vec![
            "deepseek-v4-flash".to_string(),
            "kimi-k2".to_string(),
            "glm-4.6".to_string(),
        ];

        assert!(merge_codex_desktop_statsig_available_models(
            &mut wrapper,
            &models
        ));

        let data_text = wrapper
            .get("data")
            .and_then(|value| value.as_str())
            .unwrap();
        let updated_data: Value = serde_json::from_str(data_text).unwrap();
        let available_models = updated_data
            .get("dynamic_configs")
            .and_then(|value| value.get(CODEX_DESKTOP_STATSIG_MODELS_CONFIG_ID))
            .and_then(|value| value.get("value"))
            .and_then(|value| value.get("available_models"))
            .and_then(|value| value.as_array())
            .unwrap();

        assert_eq!(
            available_models,
            &vec![
                json!("gpt-5.1-codex-max"),
                json!("gpt-5.1-codex"),
                json!("deepseek-v4-flash"),
                json!("kimi-k2"),
                json!("glm-4.6"),
            ]
        );
    }

    #[test]
    fn codex_model_ids_from_settings_trims_and_deduplicates() {
        let settings = json!({
            "modelCatalog": {
                "models": [
                    { "model": " deepseek-v4-flash " },
                    { "model": "deepseek-v4-flash" },
                    { "model": "" },
                    { "model": "kimi-k2" },
                    { "displayName": "Missing model id" }
                ]
            }
        });

        assert_eq!(
            codex_model_ids_from_settings(&settings),
            vec!["deepseek-v4-flash".to_string(), "kimi-k2".to_string()]
        );
    }

    #[test]
    fn codex_desktop_statsig_merge_creates_missing_config_path() {
        let mut wrapper = json!({
            "source": "NetworkNotModified",
            "data": "{}"
        });
        let models = vec!["deepseek-v4-flash".to_string()];

        assert!(merge_codex_desktop_statsig_available_models(
            &mut wrapper,
            &models
        ));

        let data_text = wrapper
            .get("data")
            .and_then(|value| value.as_str())
            .unwrap();
        let updated_data: Value = serde_json::from_str(data_text).unwrap();
        assert_eq!(
            updated_data
                .get("dynamic_configs")
                .and_then(|value| value.get(CODEX_DESKTOP_STATSIG_MODELS_CONFIG_ID))
                .and_then(|value| value.get("value"))
                .and_then(|value| value.get("available_models")),
            Some(&json!(["deepseek-v4-flash"]))
        );
    }

    #[test]
    fn codex_desktop_statsig_wrapper_round_trips_chromium_prefix() {
        let wrapper = json!({
            "source": "NetworkNotModified",
            "data": "{}"
        });
        let mut encoded = vec![1];
        encoded.extend_from_slice(serde_json::to_string(&wrapper).unwrap().as_bytes());

        let (prefix, encoding, decoded) = decode_codex_desktop_statsig_wrapper(&encoded).unwrap();
        assert_eq!(prefix, Some(1));
        assert_eq!(encoding, CodexDesktopStatsigWrapperEncoding::Utf8);
        assert_eq!(decoded, wrapper);
        assert_eq!(
            encode_codex_desktop_statsig_wrapper(prefix, encoding, &decoded).unwrap(),
            encoded
        );
    }

    #[test]
    fn codex_desktop_statsig_wrapper_round_trips_utf16le_values() {
        let wrapper = json!({
            "source": "NetworkNotModified",
            "data": "{}"
        });
        let text = serde_json::to_string(&wrapper).unwrap();
        let mut encoded = vec![1];
        for unit in text.encode_utf16() {
            encoded.extend_from_slice(&unit.to_le_bytes());
        }

        let (prefix, encoding, decoded) = decode_codex_desktop_statsig_wrapper(&encoded).unwrap();
        assert_eq!(prefix, Some(1));
        assert_eq!(encoding, CodexDesktopStatsigWrapperEncoding::Utf16Le);
        assert_eq!(decoded, wrapper);
        assert_eq!(
            encode_codex_desktop_statsig_wrapper(prefix, encoding, &decoded).unwrap(),
            encoded
        );
    }

    #[test]
    fn codex_desktop_statsig_merge_ignores_unparseable_data() {
        let mut wrapper = json!({
            "source": "NetworkNotModified",
            "data": "{not json"
        });
        let models = vec!["deepseek-v4-flash".to_string()];

        assert!(!merge_codex_desktop_statsig_available_models(
            &mut wrapper,
            &models
        ));
    }

    #[test]
    fn codex_desktop_runtime_patch_expression_uses_json_model_ids() {
        let expression = codex_desktop_runtime_patch_expression(&[
            "deepseek-v4-flash".to_string(),
            "quote\"model".to_string(),
        ])
        .unwrap();

        assert!(expression.contains(r#"const MODEL_IDS = ["deepseek-v4-flash","quote\"model"];"#));
        assert!(expression.contains(CODEX_DESKTOP_STATSIG_MODELS_CONFIG_ID));
        assert!(expression.contains(CODEX_DESKTOP_STATSIG_CACHE_KEY_MARKER));
    }

    #[test]
    fn codex_desktop_statsig_last_modified_keys_sort_newest_first() {
        let last_modified = json!({
            "statsig.cached.evaluations.old": 1000,
            "statsig.cached.evaluations.new": 3000,
            "statsig.cached.evaluations.middle": 2000,
            "unrelated": 9999
        });

        assert_eq!(
            codex_desktop_active_statsig_cache_keys(&last_modified),
            vec![
                "statsig.cached.evaluations.new".to_string(),
                "statsig.cached.evaluations.middle".to_string(),
                "statsig.cached.evaluations.old".to_string(),
            ]
        );
    }

    #[test]
    fn codex_desktop_statsig_last_modified_pin_prefers_custom_cache_keys() {
        let mut last_modified = json!({
            "statsig.cached.evaluations.custom": 1000,
            "statsig.cached.evaluations.default": 2000,
        });
        let cache_keys = HashSet::from(["statsig.cached.evaluations.custom".to_string()]);

        assert!(pin_codex_desktop_statsig_last_modified_cache_keys(
            &mut last_modified,
            &cache_keys,
            10_000
        ));

        let custom_timestamp = last_modified
            .get("statsig.cached.evaluations.custom")
            .and_then(codex_desktop_statsig_timestamp_millis)
            .unwrap();
        let default_timestamp = last_modified
            .get("statsig.cached.evaluations.default")
            .and_then(codex_desktop_statsig_timestamp_millis)
            .unwrap();

        assert!(custom_timestamp > default_timestamp);
    }

    fn test_codex_desktop_statsig_wrapper(models: &[&str]) -> Value {
        let data = json!({
            "dynamic_configs": {
                CODEX_DESKTOP_STATSIG_MODELS_CONFIG_ID: {
                    "value": {
                        "available_models": models
                    }
                }
            }
        });
        json!({
            "source": "Network",
            "data": data.to_string()
        })
    }

    #[test]
    fn codex_desktop_statsig_leveldb_sync_updates_active_and_pins_custom_cache() {
        let temp_dir = tempfile::tempdir().expect("create temp leveldb");
        let options = rusty_leveldb::Options {
            create_if_missing: true,
            ..Default::default()
        };
        let mut db = rusty_leveldb::DB::open(temp_dir.path(), options).expect("open temp leveldb");

        let active_key = b"_https://codex\x00statsig.cached.evaluations.active".to_vec();
        let custom_key = b"_https://codex\x00statsig.cached.evaluations.custom".to_vec();
        let last_modified_key =
            b"_https://codex\x00statsig.last_modified_time.evaluations".to_vec();

        let active_value = encode_codex_desktop_statsig_wrapper(
            Some(1),
            CodexDesktopStatsigWrapperEncoding::Utf8,
            &test_codex_desktop_statsig_wrapper(&["gpt-5.1-codex"]),
        )
        .unwrap();
        let custom_value = encode_codex_desktop_statsig_wrapper(
            Some(1),
            CodexDesktopStatsigWrapperEncoding::Utf8,
            &test_codex_desktop_statsig_wrapper(&["gpt-5.1-codex", "deepseek-v4-flash"]),
        )
        .unwrap();
        let last_modified_value = encode_codex_desktop_statsig_wrapper(
            Some(1),
            CodexDesktopStatsigWrapperEncoding::Utf8,
            &json!({
                "statsig.cached.evaluations.active": 2000,
                "statsig.cached.evaluations.custom": 1000,
            }),
        )
        .unwrap();

        db.put(&active_key, &active_value)
            .expect("write active cache");
        db.put(&custom_key, &custom_value)
            .expect("write custom cache");
        db.put(&last_modified_key, &last_modified_value)
            .expect("write last modified");
        db.close().expect("close seeded leveldb");

        let updated_count = sync_codex_desktop_available_models_cache_path(
            temp_dir.path(),
            &["deepseek-v4-flash".to_string()],
        )
        .expect("sync temp leveldb");
        assert_eq!(
            updated_count, 2,
            "sync should update the active cache and pin last_modified"
        );

        let options = rusty_leveldb::Options {
            create_if_missing: false,
            ..Default::default()
        };
        let mut db =
            rusty_leveldb::DB::open(temp_dir.path(), options).expect("reopen temp leveldb");

        let active_value = db.get(&active_key).expect("read active cache");
        let (_, _, active_wrapper) =
            decode_codex_desktop_statsig_wrapper(&active_value).expect("decode active cache");
        assert!(codex_desktop_statsig_has_all_models(
            &active_wrapper,
            &["deepseek-v4-flash".to_string()]
        ));

        let last_modified_value = db
            .get(&last_modified_key)
            .expect("read last_modified cache");
        let (_, _, last_modified) = decode_codex_desktop_statsig_wrapper(&last_modified_value)
            .expect("decode last_modified cache");
        let active_keys = codex_desktop_active_statsig_cache_keys(&last_modified);
        assert!(
            matches!(
                active_keys.first().map(String::as_str),
                Some("statsig.cached.evaluations.active" | "statsig.cached.evaluations.custom")
            ),
            "a cache containing custom models should remain preferred after Desktop refreshes default cache"
        );

        db.close().expect("close temp leveldb");
    }

    #[test]
    fn model_catalog_json_field_writes_relative_filename() {
        let input = r#"model_provider = "any"

[model_providers.any]
name = "any"
"#;
        let catalog_path = Path::new("/tmp/cc-switch-model-catalog.json");

        let result = set_codex_model_catalog_json_field(input, Some(catalog_path)).unwrap();
        let parsed: toml::Value = toml::from_str(&result).unwrap();
        assert_eq!(
            parsed
                .get("model_catalog_json")
                .and_then(|value| value.as_str()),
            Some(CC_SWITCH_CODEX_MODEL_CATALOG_FILENAME)
        );
        assert!(
            parsed
                .get("model_providers")
                .and_then(|value| value.get("any"))
                .and_then(|value| value.get("model_catalog_json"))
                .is_none(),
            "model_catalog_json should stay top-level"
        );
    }

    #[test]
    fn openai_base_url_field_writes_active_provider_base_url_at_top_level() {
        let input = r#"model_provider = "custom"

[model_providers.custom]
name = "Relay"
base_url = "http://127.0.0.1:15721/v1"
wire_api = "responses"
"#;

        let base_url = extract_codex_base_url(input);
        let result = set_codex_openai_base_url_field(input, base_url.as_deref()).unwrap();
        let parsed: toml::Value = toml::from_str(&result).unwrap();

        assert_eq!(
            parsed
                .get("openai_base_url")
                .and_then(|value| value.as_str()),
            Some("http://127.0.0.1:15721/v1")
        );
        assert!(
            parsed
                .get("model_providers")
                .and_then(|value| value.get("custom"))
                .and_then(|value| value.get("openai_base_url"))
                .is_none(),
            "openai_base_url must stay top-level for Codex Desktop"
        );
    }

    #[test]
    fn openai_base_url_field_removes_only_when_requested() {
        let input = r#"openai_base_url = "http://127.0.0.1:15721/v1"
"#;

        let result = set_codex_openai_base_url_field(input, None).unwrap();
        let parsed: toml::Value = toml::from_str(&result).unwrap();

        assert!(parsed.get("openai_base_url").is_none());
    }

    #[test]
    fn resolve_catalog_path_returns_none_when_config_missing_field() {
        let generated = PathBuf::from("/tmp/.codex/cc-switch-model-catalog.json");
        assert!(resolve_cc_switch_catalog_path("", &generated).is_none());
        assert!(
            resolve_cc_switch_catalog_path("model = \"gpt-5\"", &generated).is_none(),
            "no model_catalog_json field should yield None"
        );
    }

    #[test]
    fn resolve_catalog_path_accepts_cc_switch_owned_file() {
        let generated = PathBuf::from("/tmp/.codex/cc-switch-model-catalog.json");
        let config = r#"model_catalog_json = "/tmp/.codex/cc-switch-model-catalog.json"
"#;
        let resolved = resolve_cc_switch_catalog_path(config, &generated).expect("path resolves");
        assert_eq!(resolved, generated);
    }

    #[test]
    fn resolve_catalog_path_rejects_user_owned_external_file() {
        let generated = PathBuf::from("/tmp/.codex/cc-switch-model-catalog.json");
        let config = r#"model_catalog_json = "/Users/me/.codex/my-handwritten-catalog.json"
"#;
        assert!(
            resolve_cc_switch_catalog_path(config, &generated).is_none(),
            "external catalog files should be left alone"
        );
    }

    #[test]
    fn build_simplified_catalog_round_trips_user_input() {
        let config = "";
        let catalog = r#"{
            "models": [
                { "slug": "deepseek-v4-pro", "display_name": "deepseek-v4-pro", "context_window": 1000000 },
                { "slug": "deepseek-v4-flash", "display_name": "DeepSeek Flash", "context_window": 1000000 }
            ]
        }"#;
        let result = build_simplified_catalog_from_texts(config, catalog).expect("entries found");
        let models = result
            .get("models")
            .and_then(|m| m.as_array())
            .expect("models array");
        assert_eq!(models.len(), 2);

        // First entry: display_name == slug → displayName squashed; explicit
        // context_window != default 128_000 → preserved.
        assert_eq!(
            models[0].get("model").and_then(|v| v.as_str()),
            Some("deepseek-v4-pro")
        );
        assert!(models[0].get("displayName").is_none());
        assert_eq!(
            models[0].get("contextWindow").and_then(|v| v.as_u64()),
            Some(1_000_000)
        );

        // Second entry: display_name distinct from slug → preserved.
        assert_eq!(
            models[1].get("displayName").and_then(|v| v.as_str()),
            Some("DeepSeek Flash")
        );
    }

    #[test]
    fn build_simplified_catalog_squashes_default_context_window() {
        // Default fallback is 128_000 when config.toml has no model_context_window.
        let catalog = r#"{
            "models": [{ "slug": "kimi", "display_name": "kimi", "context_window": 128000 }]
        }"#;
        let result = build_simplified_catalog_from_texts("", catalog).expect("entry");
        let entry = &result.get("models").unwrap().as_array().unwrap()[0];
        assert!(
            entry.get("contextWindow").is_none(),
            "default 128_000 should be squashed so the form shows blank, matching the user's blank input"
        );
    }

    #[test]
    fn build_simplified_catalog_respects_explicit_model_context_window() {
        // When config.toml sets model_context_window, that becomes the default fallback.
        let config = r#"model_context_window = 200000
"#;
        let catalog = r#"{
            "models": [
                { "slug": "a", "display_name": "a", "context_window": 200000 },
                { "slug": "b", "display_name": "b", "context_window": 500000 }
            ]
        }"#;
        let result = build_simplified_catalog_from_texts(config, catalog).expect("entries");
        let models = result.get("models").unwrap().as_array().unwrap();
        // Matches default → squashed.
        assert!(models[0].get("contextWindow").is_none());
        // Different from default → preserved.
        assert_eq!(
            models[1].get("contextWindow").and_then(|v| v.as_u64()),
            Some(500_000)
        );
    }

    #[test]
    fn build_simplified_catalog_returns_none_when_unparseable() {
        assert!(build_simplified_catalog_from_texts("", "not json").is_none());
        assert!(build_simplified_catalog_from_texts("", "{}").is_none());
        assert!(
            build_simplified_catalog_from_texts("", r#"{"models": []}"#).is_none(),
            "empty models array should yield None so the field is not inserted at all"
        );
        assert!(
            build_simplified_catalog_from_texts(
                "",
                r#"{"models": [{"display_name": "no slug"}]}"#,
            )
            .is_none(),
            "entries lacking slug are skipped; a fully-skipped catalog yields None"
        );
    }

    #[test]
    fn codex_cli_candidates_are_non_empty() {
        let candidates = codex_cli_candidates();
        assert!(
            candidates
                .iter()
                .any(|candidate| candidate == Path::new("codex")),
            "codex CLI candidates must include the PATH entry"
        );
    }

    #[test]
    fn codex_cli_candidates_include_user_node_manager_bins() {
        let temp_home = tempfile::tempdir().expect("create temp home");
        let home = temp_home.path();
        let expected = [
            home.join(".nvm/versions/node/v22.14.0/bin/codex"),
            home.join(".volta/bin/codex"),
            home.join(".asdf/shims/codex"),
            home.join(".local/share/mise/shims/codex"),
            home.join(".local/share/fnm/node-versions/v22.14.0/installation/bin/codex"),
        ];

        for candidate in &expected {
            std::fs::create_dir_all(candidate.parent().expect("candidate parent"))
                .expect("create candidate parent");
            std::fs::write(candidate, "").expect("create candidate");
        }

        let mut candidates = Vec::new();
        let mut seen = HashSet::new();
        push_home_codex_cli_candidates(&mut candidates, &mut seen, home);

        for candidate in expected {
            assert!(
                candidates.contains(&candidate),
                "user-level Codex CLI candidate should be discovered: {}",
                candidate.display()
            );
        }
    }

    #[test]
    fn codex_cli_candidates_deduplicate_entries() {
        let temp_home = tempfile::tempdir().expect("create temp home");
        let home = temp_home.path();
        let candidate = home.join(".volta/bin/codex");
        std::fs::create_dir_all(candidate.parent().expect("candidate parent"))
            .expect("create candidate parent");
        std::fs::write(&candidate, "").expect("create candidate");

        let mut candidates = Vec::new();
        let mut seen = HashSet::new();
        push_existing_codex_cli_candidate(&mut candidates, &mut seen, candidate.clone());
        push_home_codex_cli_candidates(&mut candidates, &mut seen, home);

        assert_eq!(
            candidates.iter().filter(|path| **path == candidate).count(),
            1,
            "duplicate candidates should be removed"
        );
    }

    #[test]
    fn static_template_is_valid_json_with_slug() {
        let template =
            load_codex_model_template_static().expect("static template must parse as valid JSON");
        assert_eq!(
            template.get("slug").and_then(|v| v.as_str()),
            Some("gpt-5.5"),
            "static template slug must be gpt-5.5"
        );
    }

    #[test]
    fn static_template_has_required_keys() {
        let template =
            load_codex_model_template_static().expect("static template must parse as valid JSON");
        for key in &[
            "model_messages",
            "base_instructions",
            "context_window",
            "display_name",
        ] {
            assert!(
                template.get(key).is_some(),
                "static template must contain key '{key}'"
            );
        }
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn set_catalog_json_field_writes_filename_ignoring_unc_path() {
        let input = r#"model_provider = "custom"
model = "glm-5"
"#;
        // Simulate a WSL UNC path as cc-switch would see it on Windows;
        // the function now writes just the relative filename.
        let unc_path =
            Path::new(r"\\wsl.localhost\Ubuntu\home\user\.codex\cc-switch-model-catalog.json");

        let result = set_codex_model_catalog_json_field(input, Some(unc_path)).unwrap();
        let parsed: toml::Value = toml::from_str(&result).unwrap();

        let written_path = parsed
            .get("model_catalog_json")
            .and_then(|v| v.as_str())
            .expect("model_catalog_json should be set");
        assert_eq!(
            written_path, CC_SWITCH_CODEX_MODEL_CATALOG_FILENAME,
            "should write only the relative filename, not the UNC path"
        );
    }

    #[test]
    fn set_catalog_json_field_writes_filename_for_any_path() {
        let input = r#"model_provider = "custom"
model = "glm-5"
"#;
        let regular_path = Path::new("/home/user/.codex/cc-switch-model-catalog.json");

        let result = set_codex_model_catalog_json_field(input, Some(regular_path)).unwrap();
        let parsed: toml::Value = toml::from_str(&result).unwrap();

        assert_eq!(
            parsed.get("model_catalog_json").and_then(|v| v.as_str()),
            Some(CC_SWITCH_CODEX_MODEL_CATALOG_FILENAME),
            "should write only the relative filename, not the full path"
        );
    }

    #[test]
    fn set_catalog_json_none_removes_cc_switch_owned_by_filename() {
        // After the WSL fix, TOML may contain a Linux-style path.
        // The None arm must still remove it (file_name match catches any format).
        let input = r#"model_catalog_json = "/home/user/.codex/cc-switch-model-catalog.json"
"#;
        let result = set_codex_model_catalog_json_field(input, None).unwrap();
        let parsed: toml::Value = toml::from_str(&result).unwrap();
        assert!(
            parsed.get("model_catalog_json").is_none(),
            "None arm should remove cc-switch-owned field regardless of path format"
        );
    }

    #[test]
    fn set_catalog_json_none_preserves_user_owned_catalog() {
        let input = r#"model_catalog_json = "/Users/me/.codex/my-custom-catalog.json"
"#;
        let result = set_codex_model_catalog_json_field(input, None).unwrap();
        let parsed: toml::Value = toml::from_str(&result).unwrap();
        assert_eq!(
            parsed.get("model_catalog_json").and_then(|v| v.as_str()),
            Some("/Users/me/.codex/my-custom-catalog.json"),
            "None arm should NOT remove user-owned catalog"
        );
    }

    #[test]
    fn resolve_catalog_finds_relative_filename() {
        let config_text = r#"model_provider = "custom"
model_catalog_json = "cc-switch-model-catalog.json"
"#;
        let generated_path = PathBuf::from("/home/user/.codex/cc-switch-model-catalog.json");
        let result = resolve_cc_switch_catalog_path(config_text, &generated_path);
        assert_eq!(
            result,
            Some(generated_path),
            "relative filename should resolve to generated_path for file I/O"
        );
    }

    #[test]
    fn resolve_catalog_ignores_user_owned_relative() {
        let config_text = r#"model_catalog_json = "my-custom-catalog.json"
"#;
        let generated_path = PathBuf::from("/home/user/.codex/cc-switch-model-catalog.json");
        let result = resolve_cc_switch_catalog_path(config_text, &generated_path);
        assert_eq!(
            result, None,
            "user-owned catalog should not be claimed by cc-switch"
        );
    }

    #[test]
    fn set_catalog_json_none_removes_relative_path() {
        let input = r#"model_catalog_json = "cc-switch-model-catalog.json"
"#;
        let result = set_codex_model_catalog_json_field(input, None).unwrap();
        let parsed: toml::Value = toml::from_str(&result).unwrap();
        assert!(
            parsed.get("model_catalog_json").is_none(),
            "None arm should remove relative cc-switch-owned field"
        );
    }
}
