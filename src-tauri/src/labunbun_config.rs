//! LaBunbun Code 的原生配置：`~/.labunbun/settings.json`。
//!
//! 供应商写在 `providers.openaiCompatible[]`：每一项有 `id`、`baseUrl`、`apiKeyEnv`、
//! 可选的 `api` 和 `models[]`。Key 的值放在 `env.<apiKeyEnv>`，shell 里的同名变量优先。
//! 当前模型写在顶层 `model`，格式是 `provider/model`。
//!
//! CC Switch 按切换式应用管理它：库里保存这一家供应商的 `baseUrl`、`apiKey`、`api` 和
//! `models`；切换时只写当前这一家，并把 `model` 指向它的第一个模型。其它顶层键、其它
//! 原生供应商原样保留。

use crate::config::{atomic_write_private, get_home_dir};
use crate::error::AppError;
use crate::provider::Provider;
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

static WRITE_LOCK: Mutex<()> = Mutex::new(());

/// `env` 里保存 Key 的默认变量名。预设可以自带别的名字。
pub(crate) const DEFAULT_API_KEY_ENV: &str = "LABUNBUN_API_KEY";
/// 原生文件支持的三种接口。
pub(crate) const API_IDS: &[&str] = &[
    "openai-completions",
    "openai-responses",
    "anthropic-messages",
];

/// `~/.labunbun`。
pub fn get_labunbun_dir() -> PathBuf {
    get_home_dir().join(".labunbun")
}

/// `~/.labunbun/settings.json`。
pub fn get_labunbun_settings_path() -> PathBuf {
    get_labunbun_dir().join("settings.json")
}

/// 读整个设置文件；文件不存在时是空对象。
pub fn read_settings() -> Result<Value, AppError> {
    read_settings_at(&get_labunbun_settings_path())
}

fn read_settings_at(path: &Path) -> Result<Value, AppError> {
    if !path.exists() {
        return Ok(json!({}));
    }
    let text = std::fs::read_to_string(path).map_err(|e| AppError::io(path, e))?;
    if text.trim().is_empty() {
        return Ok(json!({}));
    }
    serde_json::from_str(&text).map_err(|e| {
        AppError::localized(
            "provider.labunbun.settings.invalid",
            format!("LaBunbun 设置文件格式错误: {e}"),
            format!("Invalid LaBunbun settings file: {e}"),
        )
    })
}

fn write_settings(document: &Value) -> Result<(), AppError> {
    let path = get_labunbun_settings_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
    }
    let text = serde_json::to_string_pretty(document)
        .map_err(|e| AppError::Config(format!("无法序列化 LaBunbun 设置: {e}")))?;
    // 文件里的 `env` 保存 API Key，按私有文件写。
    atomic_write_private(&path, text.as_bytes())
}

/// 原生文件里的全部供应商，键是原生 `id`。
pub fn get_providers() -> Result<Map<String, Value>, AppError> {
    let document = read_settings()?;
    Ok(providers_in(&document)
        .iter()
        .filter_map(|entry| {
            let id = entry.get("id")?.as_str()?.trim();
            (!id.is_empty()).then(|| (id.to_string(), entry.clone()))
        })
        .collect())
}

pub fn provider_key_exists(id: &str) -> Result<bool, AppError> {
    Ok(get_providers()?.contains_key(id))
}

fn providers_in(document: &Value) -> &[Value] {
    document
        .pointer("/providers/openaiCompatible")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

/// 当前模型对应的原生供应商；`model` 不是 `provider/model` 时是第一条。
fn current_entry(document: &Value) -> Option<(String, Value)> {
    let entries = providers_in(document);
    let current = document
        .get("model")
        .and_then(Value::as_str)
        .and_then(|model| model.split_once('/').map(|(provider, _)| provider));
    let entry = current
        .and_then(|id| {
            entries
                .iter()
                .find(|entry| entry.get("id").and_then(Value::as_str) == Some(id))
        })
        .or_else(|| entries.first())?;
    let id = entry.get("id")?.as_str()?.trim().to_string();
    (!id.is_empty()).then(|| (id, entry.clone()))
}

/// 从 live 里取一家原生供应商，转成供应商行的 `settings_config`。
pub(crate) fn provider_config_from_live(document: &Value, entry_id: &str) -> Option<Value> {
    let entry = providers_in(document)
        .iter()
        .find(|entry| entry.get("id").and_then(Value::as_str) == Some(entry_id))?;
    let env_name = entry.get("apiKeyEnv").and_then(Value::as_str);
    let api_key = env_name
        .and_then(|name| document.pointer(&format!("/env/{name}")))
        .and_then(Value::as_str)
        .unwrap_or_default();
    let mut config = entry.clone();
    if let Some(object) = config.as_object_mut() {
        object.remove("id");
    }
    config["apiKey"] = Value::String(api_key.to_string());
    Some(config)
}

/// 首次导入：`model` 指向的那一家（或第一条），转成 `settings_config` 和原生 id。
pub(crate) fn import_candidate(document: &Value) -> Option<(String, Value)> {
    let (id, _) = current_entry(document)?;
    let config = provider_config_from_live(document, &id)?;
    Some((id, config))
}

/// 校验并生成原生条目。返回条目和要写进 `env` 的 Key 变量与值。
fn native_entry(provider_id: &str, config: &Value) -> Result<(Value, String, String), AppError> {
    let object = config.as_object().ok_or_else(|| {
        AppError::localized(
            "provider.labunbun.settings.not_object",
            "LaBunbun 供应商配置必须是 JSON 对象",
            "LaBunbun provider configuration must be a JSON object",
        )
    })?;

    let base_url = object
        .get("baseUrl")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|url| url.starts_with("http://") || url.starts_with("https://"))
        .ok_or_else(|| {
            AppError::localized(
                "provider.labunbun.base_url.invalid",
                "LaBunbun 供应商缺少有效的 baseUrl（http:// 或 https://）",
                "LaBunbun provider needs a valid baseUrl (http:// or https://)",
            )
        })?;

    let api_key_env = object
        .get("apiKeyEnv")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .unwrap_or(DEFAULT_API_KEY_ENV)
        .to_string();

    let api = match object.get("api").and_then(Value::as_str) {
        Some(api) if API_IDS.contains(&api) => Some(api.to_string()),
        Some(api) => {
            return Err(AppError::localized(
                "provider.labunbun.api.invalid",
                format!(
                    "不支持的 LaBunbun api 值 '{api}'，可选值: {}",
                    API_IDS.join(", ")
                ),
                format!(
                    "Unsupported LaBunbun api '{api}', allowed: {}",
                    API_IDS.join(", ")
                ),
            ));
        }
        None => None,
    };

    let models = object
        .get("models")
        .and_then(Value::as_array)
        .filter(|models| !models.is_empty())
        .ok_or_else(|| {
            AppError::localized(
                "provider.labunbun.models.missing",
                "LaBunbun 供应商至少需要一个模型",
                "LaBunbun provider needs at least one model",
            )
        })?;
    for model in models {
        model
            .get("id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .ok_or_else(|| {
                AppError::localized(
                    "provider.labunbun.model.id.missing",
                    "LaBunbun 模型缺少 id",
                    "LaBunbun model is missing an id",
                )
            })?;
        for field in ["contextWindow", "maxOutputTokens"] {
            let valid = model
                .get(field)
                .and_then(Value::as_u64)
                .is_some_and(|value| value > 0);
            if !valid {
                return Err(AppError::localized(
                    "provider.labunbun.model.field.invalid",
                    format!("LaBunbun 模型的 {field} 必须是正整数"),
                    format!("LaBunbun model {field} must be a positive integer"),
                ));
            }
        }
    }

    let api_key = object
        .get("apiKey")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default()
        .to_string();

    let mut entry = json!({
        "id": provider_id,
        "baseUrl": base_url,
        "apiKeyEnv": api_key_env,
        "models": models,
    });
    if let Some(api) = api {
        entry["api"] = Value::String(api);
    }
    Ok((entry, api_key_env, api_key))
}

/// 校验供应商行的 `settings_config`（不写文件）。
pub(crate) fn validate_provider_config(provider_id: &str, config: &Value) -> Result<(), AppError> {
    native_entry(provider_id, config).map(|_| ())
}

/// 第一个模型的 id；`model` 指向它。
fn first_model_id(config: &Value) -> Option<&str> {
    config
        .get("models")?
        .as_array()?
        .first()?
        .get("id")?
        .as_str()
        .map(str::trim)
        .filter(|id| !id.is_empty())
}

/// 在 `document` 里放入 `provider` 的条目、Key 和当前模型。保留同一条目里的未知字段。
fn apply_provider(document: &mut Value, provider_id: &str, config: &Value) -> Result<(), AppError> {
    let (mut entry, env_name, api_key) = native_entry(provider_id, config)?;

    if let Some(existing) = providers_in(document)
        .iter()
        .find(|entry| entry.get("id").and_then(Value::as_str) == Some(provider_id))
        .and_then(Value::as_object)
        .cloned()
    {
        // 原生条目里 CC Switch 不管理的字段原样保留。
        for (key, value) in existing {
            if entry.get(&key).is_none() && key != "id" {
                entry[&key] = value;
            }
        }
    }

    let root = document.as_object_mut().ok_or_else(|| {
        AppError::localized(
            "provider.labunbun.settings.not_object",
            "LaBunbun 设置文件必须是 JSON 对象",
            "LaBunbun settings file must be a JSON object",
        )
    })?;
    let providers = root
        .entry("providers")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or_else(|| {
            AppError::localized(
                "provider.labunbun.providers.not_object",
                "LaBunbun 设置里的 providers 必须是对象",
                "LaBunbun settings providers must be an object",
            )
        })?;
    let entries = providers
        .entry("openaiCompatible")
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .ok_or_else(|| {
            AppError::localized(
                "provider.labunbun.providers.not_array",
                "LaBunbun 设置里的 providers.openaiCompatible 必须是数组",
                "LaBunbun settings providers.openaiCompatible must be an array",
            )
        })?;
    match entries
        .iter()
        .position(|entry| entry.get("id").and_then(Value::as_str) == Some(provider_id))
    {
        Some(index) => entries[index] = entry,
        None => entries.push(entry),
    }

    if !api_key.is_empty() {
        root.entry("env")
            .or_insert_with(|| json!({}))
            .as_object_mut()
            .ok_or_else(|| {
                AppError::localized(
                    "provider.labunbun.env.not_object",
                    "LaBunbun 设置里的 env 必须是对象",
                    "LaBunbun settings env must be an object",
                )
            })?
            .insert(env_name, Value::String(api_key));
    }

    if let Some(model) = first_model_id(config) {
        root.insert(
            "model".to_string(),
            Value::String(format!("{provider_id}/{model}")),
        );
    }
    Ok(())
}

/// 切换：写入 `provider`，删掉其它由 CC Switch 管理的原生条目，再把 `model` 指向它。
/// `managed_ids` 是库里这一应用的全部供应商；原生文件里不属于它们的条目不动。
pub(crate) fn switch_to(provider: &Provider, managed_ids: &[String]) -> Result<(), AppError> {
    let _guard = crate::sync_utils::lock_file_operation(&WRITE_LOCK);
    let mut document = read_settings()?;
    let target = provider.id.as_str();
    let mut stale_env = Vec::new();
    if let Some(entries) = document
        .pointer_mut("/providers/openaiCompatible")
        .and_then(Value::as_array_mut)
    {
        entries.retain(|entry| {
            let id = entry.get("id").and_then(Value::as_str).unwrap_or_default();
            let remove = id != target && managed_ids.iter().any(|managed| managed == id);
            if remove {
                if let Some(name) = entry.get("apiKeyEnv").and_then(Value::as_str) {
                    stale_env.push(name.to_string());
                }
            }
            !remove
        });
    }
    // 先清掉被移除条目的 Key，再写目标供应商：两家共用同一个 env 名字时，
    // 目标写入的 Key 不能被这一步删掉。
    if let Some(env) = document.get_mut("env").and_then(Value::as_object_mut) {
        for name in stale_env {
            env.remove(&name);
        }
    }
    apply_provider(&mut document, target, &provider.settings_config)?;
    write_settings(&document)
}

/// 把当前供应商重新投影到 live，不删其它条目、不改指针。
pub(crate) fn reapply(provider: &Provider) -> Result<(), AppError> {
    let _guard = crate::sync_utils::lock_file_operation(&WRITE_LOCK);
    let mut document = read_settings()?;
    apply_provider(&mut document, &provider.id, &provider.settings_config)?;
    write_settings(&document)
}

/// 从 live 删掉一家的条目；`model` 指向它时一并清掉。文件里没有就什么都不写。
pub(crate) fn remove_provider(provider_id: &str) -> Result<(), AppError> {
    let _guard = crate::sync_utils::lock_file_operation(&WRITE_LOCK);
    let mut document = read_settings()?;
    let mut changed = false;
    let mut removed_env = Vec::new();
    if let Some(entries) = document
        .pointer_mut("/providers/openaiCompatible")
        .and_then(Value::as_array_mut)
    {
        let before = entries.len();
        entries.retain(|entry| {
            let keep = entry.get("id").and_then(Value::as_str) != Some(provider_id);
            if !keep {
                if let Some(name) = entry.get("apiKeyEnv").and_then(Value::as_str) {
                    removed_env.push(name.to_string());
                }
            }
            keep
        });
        changed = entries.len() != before;
    }
    if changed {
        if let Some(root) = document.as_object_mut() {
            let points_at_removed = root
                .get("model")
                .and_then(Value::as_str)
                .and_then(|model| model.split_once('/'))
                .is_some_and(|(provider, _)| provider == provider_id);
            if points_at_removed {
                root.remove("model");
            }
            if let Some(env) = root.get_mut("env").and_then(Value::as_object_mut) {
                for name in removed_env {
                    env.remove(&name);
                }
            }
        }
        write_settings(&document)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    struct HomeGuard {
        previous: Option<std::ffi::OsString>,
        _temp: tempfile::TempDir,
    }

    impl HomeGuard {
        fn new() -> Self {
            let temp = tempfile::tempdir().expect("create isolated test home");
            let previous = std::env::var_os("CC_SWITCH_TEST_HOME");
            std::env::set_var("CC_SWITCH_TEST_HOME", temp.path());
            Self {
                previous,
                _temp: temp,
            }
        }
    }

    impl Drop for HomeGuard {
        fn drop(&mut self) {
            match &self.previous {
                Some(value) => std::env::set_var("CC_SWITCH_TEST_HOME", value),
                None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
            }
        }
    }

    fn provider(id: &str, base_url: &str, model: &str) -> Provider {
        Provider::with_id(
            id.into(),
            id.into(),
            json!({
                "baseUrl": base_url,
                "apiKey": "sk-test",
                "api": "openai-completions",
                "models": [{
                    "id": model,
                    "contextWindow": 128000,
                    "maxOutputTokens": 8192
                }]
            }),
            None,
        )
    }

    #[test]
    #[serial]
    fn switch_writes_one_entry_env_and_model_and_keeps_unknown_keys() {
        let _home = HomeGuard::new();
        std::fs::create_dir_all(get_labunbun_dir()).unwrap();
        std::fs::write(
            get_labunbun_settings_path(),
            r#"{"permissionMode":"acceptEdits","providers":{"openaiCompatible":[{"id":"manual","baseUrl":"https://manual.example/v1","apiKeyEnv":"MANUAL_KEY","models":[{"id":"m","contextWindow":1,"maxOutputTokens":1}]}]}}"#,
        )
        .unwrap();

        let a = provider("a", "https://a.example/v1", "model-a");
        let b = provider("b", "https://b.example/v1", "model-b");
        switch_to(&a, &["a".into(), "b".into()]).unwrap();
        switch_to(&b, &["a".into(), "b".into()]).unwrap();

        let document = read_settings().unwrap();
        assert_eq!(document.get("permissionMode").unwrap(), "acceptEdits");
        assert_eq!(document.get("model").unwrap(), "b/model-b");
        let entries = providers_in(&document);
        assert_eq!(entries.len(), 2, "manual entry stays, a is replaced");
        assert!(entries.iter().any(|entry| entry["id"] == "manual"));
        assert!(entries.iter().any(|entry| entry["id"] == "b"));
        assert_eq!(
            document.pointer("/env/LABUNBUN_API_KEY").unwrap(),
            "sk-test"
        );
        assert_eq!(document.pointer("/env/MANUAL_KEY"), None);
    }

    #[test]
    #[serial]
    fn reapply_updates_the_current_entry_and_model() {
        let _home = HomeGuard::new();
        let mut a = provider("a", "https://a.example/v1", "model-a");
        reapply(&a).unwrap();
        a.settings_config["baseUrl"] = json!("https://a2.example/v1");
        a.settings_config["models"][0]["id"] = json!("model-a2");
        reapply(&a).unwrap();

        let document = read_settings().unwrap();
        assert_eq!(document.get("model").unwrap(), "a/model-a2");
        assert_eq!(
            providers_in(&document)[0]["baseUrl"],
            "https://a2.example/v1"
        );
    }

    #[test]
    #[serial]
    fn removing_the_current_provider_clears_the_model_pointer_and_env() {
        let _home = HomeGuard::new();
        let a = provider("a", "https://a.example/v1", "model-a");
        reapply(&a).unwrap();
        remove_provider("a").unwrap();

        let document = read_settings().unwrap();
        assert!(providers_in(&document).is_empty());
        assert_eq!(document.get("model"), None);
        assert_eq!(document.pointer("/env/LABUNBUN_API_KEY"), None);
    }

    #[test]
    #[serial]
    fn validation_rejects_missing_fields() {
        let _home = HomeGuard::new();
        let mut missing_models = provider("a", "https://a.example/v1", "m");
        missing_models.settings_config["models"] = json!([]);
        assert!(native_entry("a", &missing_models.settings_config).is_err());

        let mut bad_url = provider("a", "ftp://a.example/v1", "m");
        bad_url.settings_config["baseUrl"] = json!("ftp://a.example/v1");
        assert!(native_entry("a", &bad_url.settings_config).is_err());

        let mut bad_window = provider("a", "https://a.example/v1", "m");
        bad_window.settings_config["models"][0]["contextWindow"] = json!(0);
        assert!(native_entry("a", &bad_window.settings_config).is_err());
    }

    #[test]
    #[serial]
    fn import_reads_the_referenced_entry_and_its_key() {
        let _home = HomeGuard::new();
        std::fs::create_dir_all(get_labunbun_dir()).unwrap();
        std::fs::write(
            get_labunbun_settings_path(),
            r#"{"model":"x/model-x","env":{"CUSTOM_KEY":"sk-import"},"providers":{"openaiCompatible":[{"id":"x","baseUrl":"https://x.example/v1","apiKeyEnv":"CUSTOM_KEY","models":[{"id":"model-x","contextWindow":1,"maxOutputTokens":1}]}]}}"#,
        )
        .unwrap();

        let (id, config) = import_candidate(&read_settings().unwrap()).unwrap();
        assert_eq!(id, "x");
        assert_eq!(config.get("apiKey").unwrap(), "sk-import");
        assert_eq!(config.get("baseUrl").unwrap(), "https://x.example/v1");
    }
}
