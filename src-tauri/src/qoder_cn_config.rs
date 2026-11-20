//! Qoder CN 自定义供应商：`~/.qoder-cn/settings.json` 的 `providers`。
//!
//! 我们拥有的条目键是 `qoder-custom-{供应商 id}`，里面写 [`OWNER_FIELD`]。
//! OAuth 令牌不进这个文件，只写本地网关地址和本地密钥。用户自己的供应商和
//! `enabledPlugins` 等其它字段原样保留。
use crate::config::{atomic_write_private, get_home_dir};
use crate::error::AppError;
use indexmap::IndexMap;
use serde_json::{Map, Value};
use std::{
    fs,
    path::{Path, PathBuf},
};

pub(crate) const OWNER_FIELD: &str = "ccSwitchProviderId";

const EFFORTS: [&str; 5] = ["low", "medium", "high", "xhigh", "max"];

pub(crate) fn data_dir() -> PathBuf {
    explicit_data_dir(std::env::var("QODER_CN_CONFIG_DIR").ok().as_deref())
        .unwrap_or_else(|| get_home_dir().join(".qoder-cn"))
}

fn explicit_data_dir(value: Option<&str>) -> Option<PathBuf> {
    value
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
}

pub(crate) fn settings_path() -> PathBuf {
    data_dir().join("settings.json")
}

pub(crate) fn provider_key(id: &str) -> String {
    format!("qoder-custom-{id}")
}

fn valid_id(id: &str) -> bool {
    let mut chars = id.chars();
    match chars.next() {
        Some(first) if first.is_ascii_alphanumeric() => {
            chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '.' || ch == '_' || ch == '-')
        }
        _ => false,
    }
}

/// 只物化本地网关。OAuth access/refresh 仍留在账号管理器里。
pub(crate) fn oauth_live_config(
    provider: &crate::provider::Provider,
    origin: &str,
) -> Result<Value, AppError> {
    let config = &provider.settings_config;
    let key = config
        .get("apiKey")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if key.len() < 32 {
        return Err(AppError::InvalidInput(
            "Qoder CN OAuth requires a private gateway key".into(),
        ));
    }
    if !valid_id(&provider.id) {
        return Err(AppError::InvalidInput(
            "Qoder CN provider id must match [A-Za-z0-9][A-Za-z0-9._-]*".into(),
        ));
    }
    let models = qoder_models(config)?;
    let default_model = models
        .first()
        .and_then(|model| model.get("model"))
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::InvalidInput("Qoder CN OAuth requires a Grok model".into()))?
        .to_string();
    let url = format!(
        "{}/qoder-cn/{}/v1",
        origin.trim_end_matches('/'),
        provider.id
    );
    Ok(serde_json::json!({
        OWNER_FIELD: provider.id,
        "displayName": provider.name,
        "baseUrl": url,
        "apiKey": key,
        "type": "openai-compatible",
        "protocol": "openai",
        "authType": "bearer",
        "model": default_model,
        "models": models,
    }))
}

fn qoder_models(config: &Value) -> Result<Vec<Value>, AppError> {
    let Some(models) = config.get("models").and_then(Value::as_array) else {
        return Err(AppError::InvalidInput(
            "Qoder CN OAuth requires a models array".into(),
        ));
    };
    let mut written = Vec::new();
    for model in models {
        let Some(id) = model.get("id").and_then(Value::as_str).map(str::trim) else {
            continue;
        };
        if id.is_empty() || !id.starts_with("grok-") {
            continue;
        }
        let efforts = model
            .pointer("/reasoning/supportedEfforts")
            .and_then(Value::as_array)
            .map(|values| {
                values
                    .iter()
                    .filter_map(Value::as_str)
                    .filter(|effort| EFFORTS.contains(effort))
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            })
            .filter(|efforts| !efforts.is_empty())
            .unwrap_or_else(|| {
                ["low", "medium", "high", "xhigh"]
                    .into_iter()
                    .map(str::to_string)
                    .collect()
            });
        written.push(serde_json::json!({
            "model": id,
            "displayName": model.get("name").and_then(Value::as_str).filter(|name| !name.trim().is_empty()).unwrap_or(id),
            "contextWindow": model.get("maxInputTokens").and_then(Value::as_u64).unwrap_or(500_000),
            "maxOutputTokens": model.get("maxOutputTokens").and_then(Value::as_u64).unwrap_or(8192),
            "capabilities": {
                "tools": model.get("supportsToolCall").and_then(Value::as_bool).unwrap_or(true),
                "vision": model.get("supportsImages").and_then(Value::as_bool).unwrap_or(false),
                "thinking": {
                    "modes": ["enabled"],
                    "supportsEffort": true,
                    "supportedEffortLevels": efforts,
                }
            }
        }));
    }
    if written.is_empty() {
        return Err(AppError::InvalidInput(
            "Qoder CN OAuth requires a grok-* model".into(),
        ));
    }
    Ok(written)
}

pub(crate) fn get_providers() -> Result<IndexMap<String, Value>, AppError> {
    let path = settings_path();
    if !path.exists() {
        return Ok(IndexMap::new());
    }
    owned_providers(&read(&path)?)
}

fn owned_providers(document: &Map<String, Value>) -> Result<IndexMap<String, Value>, AppError> {
    let mut providers = IndexMap::new();
    let Some(entries) = document.get("providers").and_then(Value::as_object) else {
        return Ok(providers);
    };
    for (key, value) in entries {
        let Some(owner) = value.get(OWNER_FIELD).and_then(Value::as_str) else {
            continue;
        };
        if provider_key(owner) != *key {
            continue;
        }
        providers.insert(owner.to_string(), value.clone());
    }
    Ok(providers)
}

pub(crate) fn set_provider(id: &str, config: &Value) -> Result<(), AppError> {
    if !valid_id(id) {
        return Err(AppError::InvalidInput(
            "Qoder CN provider id must match [A-Za-z0-9][A-Za-z0-9._-]*".into(),
        ));
    }
    if config.get(OWNER_FIELD).and_then(Value::as_str) != Some(id) {
        return Err(AppError::InvalidInput(
            "Qoder CN provider is missing its owner mark".into(),
        ));
    }
    update(&settings_path(), id, Some(config))
}

pub(crate) fn remove_provider(id: &str) -> Result<(), AppError> {
    let path = settings_path();
    if !path.exists() {
        return Ok(());
    }
    update(&path, id, None)
}

fn read(path: &Path) -> Result<Map<String, Value>, AppError> {
    let text = fs::read_to_string(path).map_err(|e| AppError::io(path, e))?;
    let value: Value = serde_json::from_str(&text).map_err(|e| AppError::Json {
        path: path.display().to_string(),
        source: e,
    })?;
    match value {
        Value::Object(map) => Ok(map),
        _ => Err(AppError::InvalidInput(
            "Qoder CN settings.json must be a JSON object".into(),
        )),
    }
}

fn update(path: &Path, id: &str, config: Option<&Value>) -> Result<(), AppError> {
    let mut document = if path.exists() {
        read(path)?
    } else {
        Map::new()
    };
    let providers = document
        .entry("providers")
        .or_insert_with(|| Value::Object(Map::new()));
    let Some(providers) = providers.as_object_mut() else {
        return Err(AppError::InvalidInput(
            "Qoder CN settings.json providers must be an object".into(),
        ));
    };
    let key = provider_key(id);
    match config {
        Some(config) => {
            providers.insert(key, config.clone());
        }
        None => {
            providers.remove(&key);
        }
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
    }
    let json = serde_json::to_string_pretty(&Value::Object(document))
        .map_err(|e| AppError::JsonSerialize { source: e })?;
    atomic_write_private(path, format!("{json}\n").as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::Provider;
    use serde_json::json;

    fn provider() -> Provider {
        let mut provider = Provider::with_id(
            "grok-oauth".into(),
            "xAI (Grok) OAuth".into(),
            json!({
                "apiKey": "0123456789abcdef0123456789abcdef",
                "models": [{
                    "id": "grok-4.7-build-fast",
                    "supportsImages": true,
                    "reasoning": { "supportedEfforts": ["low", "high", "xhigh"] }
                }]
            }),
            None,
        );
        provider.meta = None;
        provider
    }

    #[test]
    fn writes_gateway_and_keeps_existing_providers() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        fs::write(
            &path,
            r#"{"enabledPlugins":{"a":true},"providers":{"qoder-custom-mine":{"baseUrl":"https://example.test/v1","apiKey":"secret"}}}"#,
        )
        .unwrap();

        let live = oauth_live_config(&provider(), "http://127.0.0.1:15721").unwrap();
        update(&path, "grok-oauth", Some(&live)).unwrap();
        let doc: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(doc["enabledPlugins"]["a"], true);
        assert_eq!(doc["providers"]["qoder-custom-mine"]["apiKey"], "secret");
        let ours = &doc["providers"]["qoder-custom-grok-oauth"];
        assert_eq!(ours[OWNER_FIELD], "grok-oauth");
        assert_eq!(
            ours["baseUrl"],
            "http://127.0.0.1:15721/qoder-cn/grok-oauth/v1"
        );
        assert_eq!(ours["apiKey"], "0123456789abcdef0123456789abcdef");
        assert_eq!(ours["protocol"], "openai");
        assert_eq!(ours["models"][0]["model"], "grok-4.7-build-fast");
        assert_eq!(ours["models"][0]["capabilities"]["vision"], true);
        assert!(
            !ours.to_string().contains("refresh")
                && ours["apiKey"].as_str().unwrap().starts_with("0123")
        );

        let owned = owned_providers(doc.as_object().unwrap()).unwrap();
        assert!(owned.contains_key("grok-oauth"));
        assert_eq!(owned.len(), 1);

        update(&path, "grok-oauth", None).unwrap();
        let doc: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert!(doc["providers"].get("qoder-custom-grok-oauth").is_none());
        assert!(doc["providers"].get("qoder-custom-mine").is_some());
    }

    #[test]
    fn refuses_a_short_gateway_key() {
        let mut provider = provider();
        provider.settings_config["apiKey"] = json!("short");
        assert!(oauth_live_config(&provider, "http://127.0.0.1:1").is_err());
    }
}
