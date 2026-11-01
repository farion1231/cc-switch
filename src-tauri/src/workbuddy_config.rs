//! WorkBuddy（腾讯）的自定义模型配置：`~/.workbuddy/models.json`。
//!
//! WorkBuddy 桌面端内置 CodeBuddy Code 引擎，启动引擎时把 `CODEBUDDY_CONFIG_DIR` 指到
//! 自己的数据目录（默认 `~/.workbuddy`，可用 `WORKBUDDY_CONFIG_DIR` 覆盖），所以它和
//! CodeBuddy 的 `~/.codebuddy` 互不共享。文件格式：
//!
//! ```json
//! { "models": [{ "id": "...", "name": "...", "vendor": "...", "url": "...", "apiKey": "..." }],
//!   "availableModels": ["..."] }
//! ```
//!
//! 每个模型条目自带 `url` / `apiKey`，没有「供应商」这一层。CC Switch 的一个供应商展开成
//! 多条模型，条目上打 [`OWNER_FIELD`] 标记归属；没有标记的条目属于用户（或 WorkBuddy 自己
//! 的设置界面），这里只读不改。WorkBuddy 会热加载这个文件。
use crate::config::{atomic_write_private, get_home_dir};
use crate::error::AppError;
use indexmap::IndexMap;
use serde_json::{Map, Value};
use std::{
    fs,
    path::{Path, PathBuf},
};

/// 条目归属标记：值是 CC Switch 的供应商 id。WorkBuddy 不校验未知字段，会原样保留。
pub(crate) const OWNER_FIELD: &str = "ccSwitchProviderId";

/// 供应商层面的字段，写入时复制到每个模型条目（条目自己的同名字段优先）。
const SHARED_FIELDS: [&str; 4] = ["vendor", "url", "apiKey", "api"];

/// WorkBuddy 官方登录卡（`category: "official"`）：只绑定授权中心里的一个 WorkBuddy 账号，
/// 卡片「添加」时还原那个号的登录文件（同一时间只有一张在「已添加」）。它不是自定义模型，从不写进 models.json（添加、编辑、
/// 同步、移除、删除都跳过文件）。
pub(crate) fn is_official_provider(provider: &crate::provider::Provider) -> bool {
    provider.category.as_deref() == Some("official")
}

pub(crate) fn data_dir() -> PathBuf {
    explicit_data_dir(std::env::var("WORKBUDDY_CONFIG_DIR").ok().as_deref())
        .unwrap_or_else(|| get_home_dir().join(".workbuddy"))
}

fn explicit_data_dir(value: Option<&str>) -> Option<PathBuf> {
    value
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
}

pub(crate) fn config_path() -> PathBuf {
    data_dir().join("models.json")
}

/// 读出整个文件；不存在或为空（WorkBuddy 首启会建一个 0 字节文件）都当作空对象。
/// 顶层是数组的旧写法也接受（WorkBuddy 同样接受），写回时统一成对象。
fn read(path: &Path) -> Result<Map<String, Value>, AppError> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(AppError::io(path, e)),
    };
    let text = text.trim_start_matches('\u{feff}').trim();
    if text.is_empty() {
        return Ok(Map::new());
    }
    match serde_json::from_str::<Value>(text).map_err(|e| AppError::json(path, e))? {
        Value::Object(map) => Ok(map),
        Value::Array(models) => {
            let mut map = Map::new();
            map.insert("models".into(), Value::Array(models));
            Ok(map)
        }
        Value::Null => Ok(Map::new()),
        _ => Err(AppError::Config(
            "WorkBuddy models.json must be a JSON object".into(),
        )),
    }
}

fn models_of(document: &Map<String, Value>) -> Result<&[Value], AppError> {
    match document.get("models") {
        None | Some(Value::Null) => Ok(&[]),
        Some(Value::Array(models)) => Ok(models),
        Some(_) => Err(AppError::Config(
            "WorkBuddy models.json \"models\" must be an array".into(),
        )),
    }
}

fn owner_of(model: &Value) -> Option<&str> {
    model.get(OWNER_FIELD).and_then(Value::as_str)
}

fn model_id(model: &Value) -> Option<&str> {
    model
        .get("id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|id| !id.is_empty())
}

/// 按归属标记把条目还原成 CC Switch 的供应商配置：共享字段取第一条的值，
/// 每条模型只留自己的字段。
pub(crate) fn get_providers() -> Result<IndexMap<String, Value>, AppError> {
    providers_at(&config_path())
}

fn providers_at(path: &Path) -> Result<IndexMap<String, Value>, AppError> {
    let document = read(path)?;
    let mut grouped: IndexMap<String, Vec<Map<String, Value>>> = IndexMap::new();
    for model in models_of(&document)? {
        let (Some(owner), Some(object)) = (owner_of(model), model.as_object()) else {
            continue;
        };
        grouped
            .entry(owner.to_string())
            .or_default()
            .push(object.clone());
    }
    Ok(grouped
        .into_iter()
        .map(|(id, entries)| {
            let mut config = Map::new();
            for field in SHARED_FIELDS {
                if let Some(value) = entries.iter().find_map(|entry| entry.get(field)) {
                    config.insert(field.into(), value.clone());
                }
            }
            let models = entries
                .into_iter()
                .map(|mut entry| {
                    entry.remove(OWNER_FIELD);
                    for field in SHARED_FIELDS {
                        if entry.get(field) == config.get(field) {
                            entry.remove(field);
                        }
                    }
                    Value::Object(entry)
                })
                .collect();
            config.insert("models".into(), Value::Array(models));
            (id, Value::Object(config))
        })
        .collect())
}

pub(crate) fn validate_provider(config: &Value) -> Result<(), AppError> {
    let Some(models) = config.get("models").and_then(Value::as_array) else {
        return Err(AppError::InvalidInput(
            "Add at least one WorkBuddy model".into(),
        ));
    };
    if models.is_empty() {
        return Err(AppError::InvalidInput(
            "Add at least one WorkBuddy model".into(),
        ));
    }
    let mut seen = std::collections::HashSet::new();
    for model in models {
        let Some(id) = model_id(model) else {
            return Err(AppError::InvalidInput(
                "Every WorkBuddy model needs a model ID".into(),
            ));
        };
        if !seen.insert(id) {
            return Err(AppError::InvalidInput(format!(
                "Duplicate WorkBuddy model ID '{id}'"
            )));
        }
        let url = model
            .get("url")
            .or_else(|| config.get("url"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim();
        // `${VAR}` 形式由 WorkBuddy 启动时展开，这里放行。
        if !url.contains("${")
            && !url::Url::parse(url).is_ok_and(|url| {
                matches!(url.scheme(), "http" | "https") && url.host_str().is_some()
            })
        {
            return Err(AppError::InvalidInput(
                "Enter a valid WorkBuddy endpoint URL".into(),
            ));
        }
    }
    Ok(())
}

/// 供应商配置展开成写进 models.json 的条目。
fn expand_entries(id: &str, config: &Value) -> Vec<Value> {
    let models = config
        .get("models")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    models
        .into_iter()
        .filter_map(|model| {
            let Value::Object(model) = model else {
                return None;
            };
            let mut entry = Map::new();
            // id 放最前面，方便用户手看文件。
            if let Some(model_id) = model.get("id") {
                entry.insert("id".into(), model_id.clone());
            }
            for (key, value) in &model {
                if key != "id" && key != OWNER_FIELD {
                    entry.insert(key.clone(), value.clone());
                }
            }
            for field in SHARED_FIELDS {
                if entry.contains_key(field) {
                    continue;
                }
                if let Some(value) = config.get(field).filter(|value| !is_blank(value)) {
                    entry.insert(field.into(), value.clone());
                }
            }
            entry.insert(OWNER_FIELD.into(), Value::String(id.to_string()));
            Some(Value::Object(entry))
        })
        .collect()
}

fn is_blank(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::String(text) => text.trim().is_empty(),
        _ => false,
    }
}

/// 替换（`Some`）或移除（`None`）一个供应商的全部条目，其余条目和顶层字段原样保留。
fn update(path: &Path, id: &str, config: Option<&Value>) -> Result<(), AppError> {
    let mut document = read(path)?;
    let existing = models_of(&document)?.to_vec();
    let removed_ids: Vec<String> = existing
        .iter()
        .filter(|model| owner_of(model) == Some(id))
        .filter_map(|model| model_id(model).map(str::to_string))
        .collect();
    let mut kept: Vec<Value> = existing
        .into_iter()
        .filter(|model| owner_of(model) != Some(id))
        .collect();

    let new_entries = config.map(|config| expand_entries(id, config));
    let new_ids: Vec<String> = new_entries
        .iter()
        .flatten()
        .filter_map(|model| model_id(model).map(str::to_string))
        .collect();

    if config.is_none() && removed_ids.is_empty() {
        return Ok(());
    }

    // 同一个模型 id 在 WorkBuddy 里只能有一条；别人的条目（用户自己的或别的供应商的）不覆盖。
    if let Some(conflict) = kept
        .iter()
        .filter_map(model_id)
        .find(|existing| new_ids.iter().any(|id| id == existing))
    {
        return Err(AppError::InvalidInput(format!(
            "WorkBuddy models.json already has a model with ID '{conflict}'"
        )));
    }

    if let Some(entries) = new_entries {
        kept.extend(entries);
    }
    document.insert("models".into(), Value::Array(kept));

    // availableModels 为空或不存在时 WorkBuddy 显示全部模型，这时不去创建它（创建了反而会
    // 把内置模型藏起来）；已经有白名单时，把新模型加进去、把撤下的模型移出去。
    if let Some(Value::Array(available)) = document.get_mut("availableModels") {
        if !available.is_empty() {
            available.retain(|value| {
                value.as_str().is_none_or(|model| {
                    !removed_ids.iter().any(|id| id == model)
                        || new_ids.iter().any(|id| id == model)
                })
            });
            for model in &new_ids {
                if !available.iter().any(|value| value.as_str() == Some(model)) {
                    available.push(Value::String(model.clone()));
                }
            }
        }
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
    }
    let json = serde_json::to_string_pretty(&Value::Object(document))
        .map_err(|e| AppError::JsonSerialize { source: e })?;
    atomic_write_private(path, format!("{json}\n").as_bytes())
}

pub(crate) fn set_provider(id: &str, config: &Value) -> Result<(), AppError> {
    validate_provider(config)?;
    update(&config_path(), id, Some(config))
}

pub(crate) fn remove_provider(id: &str) -> Result<(), AppError> {
    let path = config_path();
    if !path.exists() {
        return Ok(());
    }
    update(&path, id, None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn provider() -> Value {
        json!({
            "vendor": "DeepSeek",
            "url": "https://api.deepseek.com/v1/chat/completions",
            "apiKey": "sk-test",
            "models": [
                { "id": "deepseek-chat", "name": "DeepSeek Chat", "supportsToolCall": true },
                { "id": "deepseek-reasoner", "supportsReasoning": true }
            ]
        })
    }

    #[test]
    fn data_dir_override_ignores_blank_values() {
        assert_eq!(explicit_data_dir(Some(" /wb ")), Some("/wb".into()));
        assert_eq!(explicit_data_dir(Some(" \t")), None);
        assert_eq!(explicit_data_dir(None), None);
    }

    #[test]
    fn writes_entries_and_preserves_user_models() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("models.json");
        fs::write(
            &path,
            r#"{"models":[{"id":"mine","url":"https://x.test/v1/chat/completions"}],"extra":1}"#,
        )
        .unwrap();

        update(&path, "ds", Some(&provider())).unwrap();
        let doc: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        let models = doc["models"].as_array().unwrap();
        assert_eq!(models.len(), 3);
        assert_eq!(models[0]["id"], "mine");
        assert_eq!(models[1]["id"], "deepseek-chat");
        assert_eq!(models[1]["apiKey"], "sk-test");
        assert_eq!(models[1]["vendor"], "DeepSeek");
        assert_eq!(models[1][OWNER_FIELD], "ds");
        assert_eq!(models[2]["supportsReasoning"], true);
        assert_eq!(doc["extra"], 1);
        // 没有白名单时不创建白名单
        assert!(doc.get("availableModels").is_none());

        update(&path, "ds", None).unwrap();
        let doc: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            doc["models"],
            json!([{"id":"mine","url":"https://x.test/v1/chat/completions"}])
        );
    }

    #[test]
    fn existing_allow_list_is_kept_in_sync() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("models.json");
        fs::write(
            &path,
            r#"{"models":[],"availableModels":["default-model"]}"#,
        )
        .unwrap();

        update(&path, "ds", Some(&provider())).unwrap();
        let doc: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            doc["availableModels"],
            json!(["default-model", "deepseek-chat", "deepseek-reasoner"])
        );

        let mut edited = provider();
        edited["models"] = json!([{ "id": "deepseek-chat" }]);
        update(&path, "ds", Some(&edited)).unwrap();
        let doc: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            doc["availableModels"],
            json!(["default-model", "deepseek-chat"])
        );
    }

    #[test]
    fn refuses_to_overwrite_models_it_does_not_own() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("models.json");
        fs::write(&path, r#"{"models":[{"id":"deepseek-chat"}]}"#).unwrap();
        let before = fs::read_to_string(&path).unwrap();
        assert!(update(&path, "ds", Some(&provider())).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), before);
    }

    #[test]
    fn empty_or_missing_file_is_treated_as_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("models.json");
        fs::write(&path, "").unwrap();
        update(&path, "ds", Some(&provider())).unwrap();
        let doc: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(doc["models"].as_array().unwrap().len(), 2);

        let missing = dir.path().join("nested").join("models.json");
        update(&missing, "ds", Some(&provider())).unwrap();
        assert!(missing.exists());
    }

    #[test]
    fn validation_requires_models_and_endpoint() {
        assert!(validate_provider(&provider()).is_ok());
        assert!(validate_provider(&json!({ "url": "https://a.test", "models": [] })).is_err());
        assert!(validate_provider(&json!({ "models": [{ "id": "m" }] })).is_err());
        assert!(validate_provider(&json!({
            "url": "https://a.test/v1/chat/completions",
            "models": [{ "id": "m" }, { "id": "m" }]
        }))
        .is_err());
        assert!(validate_provider(&json!({
            "url": "${WB_URL}",
            "models": [{ "id": "m" }]
        }))
        .is_ok());
    }

    #[test]
    fn read_back_groups_by_owner() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("models.json");
        fs::write(
            &path,
            r#"{"models":[{"id":"mine","url":"https://x.test"}]}"#,
        )
        .unwrap();
        update(&path, "ds", Some(&provider())).unwrap();
        let providers = providers_at(&path).unwrap();
        assert_eq!(providers.len(), 1);
        assert_eq!(providers["ds"], provider());
    }
}
