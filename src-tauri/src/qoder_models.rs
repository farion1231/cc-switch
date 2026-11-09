//! Qoder's native custom-provider schema (`settings.json.providers`).
//! The account's runtime access remains Qoder's responsibility; this module
//! edits local configuration without changing authentication or runtime access.
use crate::{
    app_config::AppType,
    database::Database,
    error::AppError,
    jsonc_document::JsoncDocument,
    provider::{Provider, ProviderMeta},
    qoder_config,
};
use serde::Serialize;
use serde_json::{json, Map, Value};
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::Path,
};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QoderModels {
    pub providers: Map<String, Value>,
    pub current_model: String,
    pub metadata: HashMap<String, ProviderMeta>,
}

fn read(path: &Path) -> Result<JsoncDocument, AppError> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => "{}".into(),
        Err(error) => return Err(AppError::io(path, error)),
    };
    let document = JsoncDocument::parse(text.trim_start_matches('\u{feff}'))?;
    for field in ["providers", "model"] {
        if document.root_key_count(field)? > 1 {
            return Err(AppError::Config(format!(
                "Qoder settings contain duplicate '{field}' keys"
            )));
        }
        if document
            .value()
            .get(field)
            .is_some_and(|value| !value.is_object())
        {
            return Err(AppError::Config(format!(
                "Qoder '{field}' must be an object"
            )));
        }
    }
    if let Some(providers) = document.value().get("providers").and_then(Value::as_object) {
        for provider in providers.values() {
            if !provider.is_object()
                || provider
                    .get("models")
                    .is_some_and(|models| !models.is_array())
            {
                return Err(AppError::Config(
                    "Qoder providers must be objects with an optional model array".into(),
                ));
            }
            if provider
                .get("models")
                .and_then(Value::as_array)
                .is_some_and(|models| {
                    models
                        .iter()
                        .any(|model| !model.get("model").is_some_and(Value::is_string))
                })
            {
                return Err(AppError::Config(
                    "Each Qoder model must declare a string model ID".into(),
                ));
            }
        }
    }
    Ok(document)
}

fn snapshot(document: &Value) -> QoderModels {
    QoderModels {
        metadata: HashMap::new(),
        providers: document
            .get("providers")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default(),
        current_model: document
            .pointer("/model/name")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .unwrap_or("auto")
            .into(),
    }
}

pub fn get(app: &AppType) -> Result<QoderModels, AppError> {
    let _guard = qoder_config::settings_write_lock(app)
        .lock()
        .map_err(|error| AppError::Message(error.to_string()))?;
    Ok(snapshot(
        read(&qoder_config::data_dir(app).join("settings.json"))?.value(),
    ))
}

pub(crate) fn with_metadata(
    db: &Database,
    app: &AppType,
    mut models: QoderModels,
) -> Result<QoderModels, AppError> {
    for (id, stored) in db.get_all_providers(app.as_str())? {
        if models.providers.contains_key(&id) {
            if let Some(meta) = stored.meta {
                models.metadata.insert(id, meta);
            }
        }
    }
    Ok(models)
}

/// Credentials always come from the native file. CC Switch stores only its
/// usage-query metadata in the database, without duplicating Qoder settings.
pub(crate) fn usage_provider(db: &Database, app: &AppType, id: &str) -> Result<Provider, AppError> {
    let mut models = get(app)?;
    let config = models
        .providers
        .remove(id)
        .ok_or_else(|| AppError::InvalidInput(format!("Qoder provider not found: {id}")))?;
    let name = config
        .get("displayName")
        .and_then(Value::as_str)
        .unwrap_or(id)
        .to_string();
    let mut provider = Provider::with_id(id.into(), name, config, None);
    provider.meta = db
        .get_provider_by_id(id, app.as_str())?
        .and_then(|stored| stored.meta);
    Ok(provider)
}

pub fn save_usage_script(
    db: &Database,
    app: &AppType,
    id: &str,
    script: crate::provider::UsageScript,
) -> Result<QoderModels, AppError> {
    crate::services::provider::validate_usage_script(&script)?;
    let mut provider = usage_provider(db, app, id)?;
    provider
        .meta
        .get_or_insert_with(Default::default)
        .usage_script = Some(script);
    provider.settings_config = json!({});
    db.save_provider(app.as_str(), &provider)?;
    with_metadata(db, app, get(app)?)
}

fn update(
    app: &AppType,
    change: impl FnOnce(&mut Value) -> Result<(), AppError>,
) -> Result<QoderModels, AppError> {
    let _guard = qoder_config::settings_write_lock(app)
        .lock()
        .map_err(|error| AppError::Message(error.to_string()))?;
    update_file(&qoder_config::data_dir(app).join("settings.json"), change)
}

fn update_file(
    path: &Path,
    change: impl FnOnce(&mut Value) -> Result<(), AppError>,
) -> Result<QoderModels, AppError> {
    let mut document = read(path)?;
    let mut next = document.value().clone();
    change(&mut next)?;
    qoder_config::write_settings(path, &mut document, &mut next)?;
    Ok(snapshot(&next))
}

pub fn save_provider(
    app: &AppType,
    id: &str,
    provider: Value,
    expected: Option<Value>,
    make_default: bool,
) -> Result<QoderModels, AppError> {
    validate_provider(id, &provider)?;
    update(app, |document| {
        save_provider_in(document, id, provider, expected.as_ref(), make_default)
    })
}

fn save_provider_in(
    document: &mut Value,
    id: &str,
    provider: Value,
    expected: Option<&Value>,
    make_default: bool,
) -> Result<(), AppError> {
    check_expected(document, id, expected)?;
    if document.get("providers").is_none() {
        document["providers"] = json!({});
    }
    let current = snapshot(document).current_model;
    let default = default_model(&provider)
        .ok_or_else(|| AppError::InvalidInput("Add a Qoder model first".into()))?;
    let chosen = format!("{id}/{default}");
    let removed_active_model = current
        .strip_prefix(&format!("{id}/"))
        .is_some_and(|model| !has_model(&provider, model));
    document["providers"][id] = provider;
    if make_default || removed_active_model {
        set_model_in(document, &chosen)?;
    }
    Ok(())
}

pub fn delete_provider(app: &AppType, id: &str, expected: Value) -> Result<QoderModels, AppError> {
    update(app, |document| {
        check_expected(document, id, Some(&expected))?;
        if snapshot(document)
            .current_model
            .starts_with(&format!("{id}/"))
        {
            set_model_in(document, "auto")?;
        }
        if let Some(providers) = document.get_mut("providers").and_then(Value::as_object_mut) {
            providers.remove(id);
        }
        Ok(())
    })
}

pub fn set_model(app: &AppType, model: &str) -> Result<QoderModels, AppError> {
    update(app, |document| set_model_in(document, model))
}

fn set_model_in(document: &mut Value, model: &str) -> Result<(), AppError> {
    let current = snapshot(document).current_model;
    if model != "auto" && model != current {
        let (id, model_id) = model
            .split_once('/')
            .ok_or_else(|| AppError::InvalidInput("Select an available Qoder model".into()))?;
        let provider = document
            .get("providers")
            .and_then(|providers| providers.get(id))
            .ok_or_else(|| {
                AppError::InvalidInput(
                    "Qoder provider is no longer configured; refresh the model list".into(),
                )
            })?;
        validate_provider(id, provider)?;
        if !has_model(provider, model_id) {
            return Err(AppError::InvalidInput(
                "Qoder model is no longer configured; refresh the model list".into(),
            ));
        }
    }
    if document.get("model").is_none() {
        document["model"] = json!({});
    }
    document["model"]["name"] = json!(model);
    Ok(())
}

fn check_expected(document: &Value, id: &str, expected: Option<&Value>) -> Result<(), AppError> {
    let current = document
        .get("providers")
        .and_then(|providers| providers.get(id));
    if current != expected {
        return Err(AppError::InvalidInput(
            "Qoder provider changed outside this editor. Refresh and reopen it before saving"
                .into(),
        ));
    }
    Ok(())
}

fn default_model(provider: &Value) -> Option<&str> {
    provider
        .get("model")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            provider
                .get("models")?
                .as_array()?
                .first()?
                .get("model")?
                .as_str()
        })
}

fn has_model(provider: &Value, id: &str) -> bool {
    provider.get("model").and_then(Value::as_str) == Some(id)
        || provider
            .get("models")
            .and_then(Value::as_array)
            .is_some_and(|models| {
                models
                    .iter()
                    .any(|model| model.get("model").and_then(Value::as_str) == Some(id))
            })
}

pub fn validate_provider(id: &str, provider: &Value) -> Result<(), AppError> {
    let invalid = |message: &str| AppError::InvalidInput(message.into());
    if id.is_empty()
        || !id.starts_with(|character: char| character.is_ascii_alphanumeric())
        || !id.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-')
        })
        || matches!(id, "qoder" | "constructor" | "prototype")
    {
        return Err(invalid("Provider ID must start with a letter or digit and contain only letters, digits, '.', '_' or '-'; reserved IDs are not allowed"));
    }
    let object = provider
        .as_object()
        .ok_or_else(|| invalid("Qoder provider must be an object"))?;
    for (field, options) in [
        (
            "type",
            &["openai-compatible", "alibaba", "aliyun-bailian"][..],
        ),
        ("protocol", &["openai", "openai-responses", "anthropic"][..]),
        ("authType", &["bearer", "api-key"][..]),
    ] {
        if object
            .get(field)
            .is_some_and(|value| !value.as_str().is_some_and(|value| options.contains(&value)))
        {
            return Err(invalid(&format!("Select a supported Qoder {field}")));
        }
    }
    let url = object
        .get("baseUrl")
        .and_then(Value::as_str)
        .and_then(|value| url::Url::parse(value).ok());
    if !url.is_some_and(|url| {
        matches!(url.scheme(), "http" | "https")
            && url.host_str().is_some()
            && url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none()
            && url.fragment().is_none()
    }) {
        return Err(invalid(
            "Enter an HTTP(S) base URL without credentials, query parameters or a fragment",
        ));
    }
    let key = object
        .get("apiKey")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            invalid(
                "Enter an API key, an environment reference, or an empty key for a local service",
            )
        })?;
    if key.chars().any(char::is_control)
        || (matches!(
            object.get("type").and_then(Value::as_str),
            Some("alibaba" | "aliyun-bailian")
        ) && key.trim().is_empty())
    {
        return Err(invalid("Enter a valid API key"));
    }
    for field in [
        "image",
        "contextWindow",
        "maxOutputTokens",
        "temperature",
        "top_p",
        "topP",
        "top_k",
        "topK",
        "thinking",
        "reasoning",
        "generation",
        "preserve_thinking",
        "preserveThinking",
    ] {
        if object.contains_key(field) {
            return Err(invalid(&format!("Qoder does not support provider-level '{field}'; configure model capabilities and token limits on each model")));
        }
    }
    let mut ids = HashSet::new();
    if let Some(models) = object.get("models") {
        for model in models
            .as_array()
            .ok_or_else(|| invalid("Qoder models must be an array"))?
        {
            let model_object = model
                .as_object()
                .ok_or_else(|| invalid("Each Qoder model must be an object"))?;
            let id = model
                .get("model")
                .and_then(Value::as_str)
                .filter(|value| {
                    !value.trim().is_empty()
                        && value.trim() == *value
                        && !value.chars().any(char::is_control)
                })
                .ok_or_else(|| invalid("Enter a model ID"))?;
            if !ids.insert(id) {
                return Err(invalid("Model IDs must be unique within a provider"));
            }
            for field in ["contextWindow", "maxOutputTokens"] {
                if model_object.get(field).is_some_and(|value| {
                    !value
                        .as_u64()
                        .is_some_and(|value| value > 0 && value <= 9_007_199_254_740_991)
                }) {
                    return Err(invalid("Model token limits must be positive integers"));
                }
            }
            for field in [
                "maxTokens",
                "temperature",
                "top_p",
                "topP",
                "top_k",
                "topK",
                "thinking",
                "reasoning",
                "generation",
                "preserve_thinking",
                "preserveThinking",
            ] {
                if model_object.contains_key(field) {
                    return Err(invalid(&format!("Qoder does not support model-level '{field}'; use maxOutputTokens and capabilities instead")));
                }
            }
            if let Some(capabilities) = model.get("capabilities") {
                let capabilities = capabilities
                    .as_object()
                    .ok_or_else(|| invalid("Model capabilities must be an object"))?;
                for field in ["tools", "vision", "cacheControl"] {
                    if capabilities
                        .get(field)
                        .is_some_and(|value| !value.is_boolean())
                    {
                        return Err(invalid("Model capability switches must be boolean"));
                    }
                }
                if let Some(thinking) = capabilities.get("thinking") {
                    let thinking = thinking
                        .as_object()
                        .ok_or_else(|| invalid("Thinking capabilities must be an object"))?;
                    if !thinking
                        .get("modes")
                        .and_then(Value::as_array)
                        .is_some_and(|modes| {
                            modes
                                .iter()
                                .all(|mode| matches!(mode.as_str(), Some("enabled" | "adaptive")))
                        })
                    {
                        return Err(invalid("Choose valid thinking modes: enabled or adaptive"));
                    }
                }
            }
        }
    }
    if default_model(provider)
        .is_none_or(|model| model.trim().is_empty() || model.chars().any(char::is_control))
    {
        return Err(invalid(
            "Add at least one model or specify a default model ID",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provider() -> Value {
        json!({"displayName":"Example","protocol":"openai-responses","baseUrl":"https://example.test/v1","apiKey":"${EXAMPLE_API_KEY}","model":"vendor/model-one","models":[{"model":"vendor/model-one","contextWindow":128000,"maxOutputTokens":8192,"capabilities":{"tools":true,"vision":false,"thinking":{"modes":[]}},"customNativeOption":"keep"}],"routing":{"summary":"vendor/model-one"}})
    }

    #[test]
    fn qoder_models_write_native_providers_and_select_models_preserving_jsonc() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("settings.json");
        fs::write(&path, "{\n  // native settings\n  \"model\": {\"name\": \"auto\", \"reasoningEffort\": \"high\"},\n  \"mcpServers\": {\"local\": {\"command\": \"node\"}},\n  \"skills\": {\"custom\": true},\n  \"modelConfigs\": {\"customModels\": [{\"provider\":\"legacy\"}]},\n  \"providers\": {\"other\": {\"model\":\"native\",\"apiKey\":\"untouched\"}}\n}\n").unwrap();
        let before = read(&path).unwrap().value().clone();
        validate_provider("example", &provider()).unwrap();
        let state = update_file(&path, |document| {
            save_provider_in(document, "example", provider(), None, true)
        })
        .unwrap();
        assert_eq!(state.current_model, "example/vendor/model-one");
        let source = fs::read_to_string(&path).unwrap();
        assert!(source.contains("// native settings"));
        let saved = read(&path).unwrap();
        for field in ["mcpServers", "skills", "modelConfigs"] {
            assert_eq!(saved.value()[field], before[field]);
        }
        assert_eq!(
            saved.value()["providers"]["other"],
            before["providers"]["other"]
        );
        assert_eq!(saved.value()["model"]["reasoningEffort"], "high");
        assert_eq!(saved.value()["privacy"]["usageStatisticsEnabled"], false);
        assert_eq!(saved.value()["providers"]["example"], provider());
        assert!(saved.value()["providers"]["example"]["models"].is_array());
    }

    #[test]
    fn qoder_model_editor_rejects_stale_snapshots_and_keeps_invalid_files() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("settings.json");
        let original =
            json!({"providers":{"example":provider()},"model":{"name":"auto"}}).to_string();
        fs::write(&path, &original).unwrap();
        let mut changed = provider();
        changed["apiKey"] = json!("changed-in-other-app");
        assert!(update_file(&path, |document| save_provider_in(
            document,
            "example",
            provider(),
            Some(&changed),
            true
        ))
        .is_err());
        assert!(update_file(&path, |document| save_provider_in(
            document,
            "example",
            provider(),
            None,
            false
        ))
        .is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
        for malformed in [
            "broken JSON",
            "{\"providers\":[]}",
            "{\"model\":null}",
            "{\"providers\":{},\"providers\":{}}",
        ] {
            fs::write(&path, malformed).unwrap();
            assert!(update_file(&path, |_| Ok(())).is_err());
            assert_eq!(fs::read_to_string(&path).unwrap(), malformed);
        }
    }

    #[test]
    fn qoder_native_model_validation_rejects_unusable_settings() {
        for (field, value) in [
            ("protocol", json!("gemini")),
            ("authType", json!("custom")),
            ("baseUrl", json!("https://user:password@example.test/v1")),
            ("baseUrl", json!("https://example.test/v1?key=secret")),
            ("models", json!([{"model":"same"},{"model":"same"}])),
            ("models", json!([{"model":"model","contextWindow":1.5}])),
            ("models", json!([{"model":"model","maxOutputTokens":0}])),
            (
                "models",
                json!([{"model":"model","thinking":{"modes":["enabled"]}}]),
            ),
        ] {
            let mut invalid = provider();
            invalid[field] = value;
            assert!(validate_provider("example", &invalid).is_err(), "{field}");
        }
        for id in ["qoder", "constructor", "../outside", "bad/id", ""] {
            assert!(validate_provider(id, &provider()).is_err());
        }
        let mut local = provider();
        local["apiKey"] = json!("");
        validate_provider("local", &local).unwrap();
    }

    #[test]
    fn qoder_editing_active_model_keeps_default_model_valid() {
        let mut document = json!({"providers":{"example":provider()},"model":{"name":"example/vendor/model-one","reasoningEffort":"high"}});
        let mut edited = provider();
        edited["model"] = json!("model-two");
        edited["models"] = json!([{"model":"model-two"}]);
        save_provider_in(&mut document, "example", edited, Some(&provider()), false).unwrap();
        assert_eq!(document["model"]["name"], "example/model-two");
        assert_eq!(document["model"]["reasoningEffort"], "high");
        let before = document.clone();
        assert!(set_model_in(&mut document, "missing/model").is_err());
        assert_eq!(document, before);
    }
}
