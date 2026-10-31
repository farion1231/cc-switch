//! CodeBuddy Code (Tencent) user-level custom models: `~/.codebuddy/models.json`.
//!
//! Each CC Switch provider owns exactly one entry of the native `models` array; the
//! provider key is the model `id`. Every other entry, `availableModels` and any unknown
//! top-level key are preserved, and the existing layout survives through `JsoncDocument`.
//! The file must stay strict JSON: CodeBuddy ignores it entirely when it has comments.
//! CodeBuddy watches this file and reloads it on its own, so no restart is needed.
//!
//! Docs: <https://www.codebuddy.ai/docs/cli/models>, <https://www.codebuddy.ai/docs/cli/env-vars>
use crate::config::{atomic_write_private, get_home_dir};
use crate::error::AppError;
use crate::jsonc_document::JsoncDocument;
use indexmap::IndexMap;
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
};

/// Serializes CC Switch's own read-modify-write cycles on `models.json`.
static MODELS_LOCK: Mutex<()> = Mutex::new(());

/// Prefix CodeBuddy Code shows for models.json entries (e.g. in `--help` and `/model`).
const CUSTOM_MODEL_PREFIX: &str = "custom-local:";

/// `CODEBUDDY_CONFIG_DIR` relocates CodeBuddy Code's configuration and data files.
pub(crate) fn data_dir() -> PathBuf {
    explicit_data_dir(std::env::var("CODEBUDDY_CONFIG_DIR").ok().as_deref())
        .unwrap_or_else(|| get_home_dir().join(".codebuddy"))
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

pub(crate) fn settings_path() -> PathBuf {
    data_dir().join("settings.json")
}

fn read_text(path: &Path) -> Result<String, AppError> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(text),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(error) => Err(AppError::io(path, error)),
    }
}

/// An empty or whitespace-only file is treated as `{}` (CodeBuddy may create it empty).
fn read_document(path: &Path) -> Result<JsoncDocument, AppError> {
    let text = read_text(path)?;
    let source = text.trim_start_matches('\u{feff}');
    if source.trim().is_empty() {
        return JsoncDocument::parse("{}");
    }
    // CodeBuddy Code parses models.json as strict JSON and ignores the whole file
    // otherwise (verified with 2.164: one `//` comment hides every custom model).
    serde_json::from_str::<Value>(source).map_err(|_| {
        AppError::Config(
            "CodeBuddy models.json is not valid JSON (comments and trailing commas are not supported)"
                .into(),
        )
    })?;
    // Strict JSON is a JSONC subset; editing through JsoncDocument keeps the user's layout.
    JsoncDocument::parse(source)
        .map_err(|_| AppError::Config("Invalid CodeBuddy models.json".into()))
}

fn models_of(value: &Value) -> Result<Vec<Value>, AppError> {
    match value.get("models") {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Array(models)) => Ok(models.clone()),
        Some(_) => Err(AppError::Config(
            "CodeBuddy models.json \"models\" must be an array".into(),
        )),
    }
}

fn entry_id(entry: &Value) -> Option<&str> {
    entry
        .get("id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|id| !id.is_empty())
}

fn providers_in(value: &Value) -> Result<IndexMap<String, Value>, AppError> {
    let mut providers = IndexMap::new();
    for entry in models_of(value)? {
        if !entry.is_object() {
            continue;
        }
        if let Some(id) = entry_id(&entry) {
            providers.entry(id.to_string()).or_insert(entry.clone());
        }
    }
    Ok(providers)
}

/// User-level custom models, keyed by model id.
pub(crate) fn get_providers() -> Result<IndexMap<String, Value>, AppError> {
    let document = read_document(&config_path())?;
    providers_in(document.value())
}

pub(crate) fn provider_key_exists(id: &str) -> Result<bool, AppError> {
    Ok(get_providers()?.contains_key(id))
}

fn is_env_reference(value: &str) -> bool {
    value.contains("${")
}

/// Auth binding provider for CodeBuddy official sign-in cards (see `services::codebuddy_account`).
pub(crate) const OFFICIAL_AUTH_PROVIDER: &str = "codebuddy_oauth";

/// A CodeBuddy "official sign-in" card binds one managed CodeBuddy account; using it switches
/// CodeBuddy's global sign-in. It is never a models.json entry, so live writes skip it.
pub(crate) fn is_official_provider(provider: &crate::provider::Provider) -> bool {
    provider.category.as_deref() == Some("official")
}

/// The managed account an official card is bound to, if any.
pub(crate) fn official_account_id(provider: &crate::provider::Provider) -> Option<String> {
    provider
        .meta
        .as_ref()
        .and_then(|meta| meta.managed_account_id_for(OFFICIAL_AUTH_PROVIDER))
        .map(|id| id.trim().to_string())
        .filter(|id| !id.is_empty())
}

pub(crate) fn validate_provider(id: &str, config: &Value) -> Result<(), AppError> {
    if id.trim().is_empty() || id.trim() != id || id.chars().any(char::is_control) {
        return Err(AppError::InvalidInput(
            "Enter a CodeBuddy model ID without leading or trailing spaces".into(),
        ));
    }
    if !config.is_object() {
        return Err(AppError::InvalidInput(
            "CodeBuddy model configuration must be a JSON object".into(),
        ));
    }
    let url = config
        .get("url")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default();
    let valid_url = is_env_reference(url)
        || url::Url::parse(url)
            .is_ok_and(|url| matches!(url.scheme(), "http" | "https") && url.host_str().is_some());
    if !valid_url {
        return Err(AppError::InvalidInput(
            "Enter a valid CodeBuddy endpoint URL".into(),
        ));
    }
    for field in ["name", "vendor", "apiKey", "api"] {
        if config.get(field).is_some_and(|value| !value.is_string()) {
            return Err(AppError::InvalidInput(format!(
                "CodeBuddy model field \"{field}\" must be a string"
            )));
        }
    }
    for field in ["maxInputTokens", "maxOutputTokens"] {
        if config
            .get(field)
            .is_some_and(|value| value.as_u64().is_none_or(|n| n == 0))
        {
            return Err(AppError::InvalidInput(format!(
                "CodeBuddy model field \"{field}\" must be a positive integer"
            )));
        }
    }
    for field in ["supportsToolCall", "supportsImages", "supportsReasoning"] {
        if config.get(field).is_some_and(|value| !value.is_boolean()) {
            return Err(AppError::InvalidInput(format!(
                "CodeBuddy model field \"{field}\" must be true or false"
            )));
        }
    }
    Ok(())
}

/// The default model in user settings (`settings.json` → `model`), if readable.
fn selected_model(settings: &Path) -> Option<String> {
    let text = read_text(settings).ok()?;
    let document = JsoncDocument::parse(text.trim_start_matches('\u{feff}')).ok()?;
    document
        .value()
        .get("model")
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn write_entry(
    path: &Path,
    settings: &Path,
    id: &str,
    entry: Option<Value>,
    create_only: bool,
) -> Result<(), AppError> {
    let _guard = MODELS_LOCK
        .lock()
        .map_err(|_| AppError::Message("CodeBuddy models.json lock poisoned".into()))?;
    let mut document = read_document(path)?;
    let mut desired = document.value().clone();
    let mut models = models_of(&desired)?;
    let position = models
        .iter()
        .position(|model| model.is_object() && entry_id(model) == Some(id));

    if create_only && position.is_some() {
        return Err(AppError::InvalidInput(format!(
            "CodeBuddy model '{id}' already exists"
        )));
    }

    match entry {
        Some(mut entry) => {
            entry
                .as_object_mut()
                .expect("validated object")
                .insert("id".into(), json!(id));
            match position {
                Some(index) => models[index] = entry,
                None => models.push(entry),
            }
            // A non-empty availableModels hides every model not listed; keep a newly
            // managed model visible in CodeBuddy's /model picker.
            if position.is_none() {
                if let Some(Value::Array(available)) = desired.get_mut("availableModels") {
                    if !available.is_empty() && !available.iter().any(|v| v.as_str() == Some(id)) {
                        available.push(json!(id));
                    }
                }
            }
        }
        None => {
            let Some(index) = position else {
                return Ok(());
            };
            // CodeBuddy lists custom models as `custom-local:<id>` and accepts either form.
            let selected = selected_model(settings);
            let selected = selected
                .as_deref()
                .map(|model| model.strip_prefix(CUSTOM_MODEL_PREFIX).unwrap_or(model));
            if selected == Some(id) {
                return Err(AppError::InvalidInput(
                    "Select another default model in CodeBuddy before removing this model".into(),
                ));
            }
            models.remove(index);
        }
    }

    desired
        .as_object_mut()
        .expect("JsoncDocument root is an object")
        .insert("models".into(), Value::Array(models));
    if !document.apply(&desired)? && path.exists() {
        return Ok(());
    }
    let source = document.validated_source()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| AppError::io(parent, error))?;
    }
    atomic_write_private(path, source.as_bytes())
}

pub(crate) fn set_provider(id: &str, config: Value) -> Result<(), AppError> {
    validate_provider(id, &config)?;
    write_entry(&config_path(), &settings_path(), id, Some(config), false)
}

/// Like `set_provider`, but never replaces an existing model entry.
pub(crate) fn add_provider(id: &str, config: Value) -> Result<(), AppError> {
    validate_provider(id, &config)?;
    write_entry(&config_path(), &settings_path(), id, Some(config), true)
}

pub(crate) fn remove_provider(id: &str) -> Result<(), AppError> {
    if !config_path().exists() {
        return Ok(());
    }
    write_entry(&config_path(), &settings_path(), id, None, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model(url: &str) -> Value {
        json!({
            "name": "DeepSeek V3",
            "vendor": "DeepSeek",
            "url": url,
            "apiKey": "test-key",
            "maxInputTokens": 128000,
            "maxOutputTokens": 8192,
            "supportsToolCall": true
        })
    }

    #[test]
    fn official_cards_resolve_their_bound_account() {
        use crate::provider::Provider;
        let mut card = Provider::with_id("official".into(), "Official".into(), json!({}), None);
        assert!(!is_official_provider(&card));
        card.category = Some("official".into());
        assert!(is_official_provider(&card));
        assert_eq!(official_account_id(&card), None);

        let bind = |provider: &str, account: &str| {
            serde_json::from_value(json!({"authBinding": {
                "source": "managed_account",
                "authProvider": provider,
                "accountId": account
            }}))
            .unwrap()
        };
        card.meta = Some(bind("codebuddy_oauth", " u1@codebuddy.ai "));
        assert_eq!(
            official_account_id(&card).as_deref(),
            Some("u1@codebuddy.ai")
        );
        card.meta = Some(bind("codebuddy_oauth", "  "));
        assert_eq!(official_account_id(&card), None);
        card.meta = Some(bind("codex_oauth", "u1@codebuddy.ai"));
        assert_eq!(official_account_id(&card), None);
    }

    #[test]
    fn config_dir_override_ignores_blank_values() {
        assert_eq!(explicit_data_dir(Some(" /custom ")), Some("/custom".into()));
        assert_eq!(explicit_data_dir(Some("  ")), None);
        assert_eq!(explicit_data_dir(None), None);
    }

    #[test]
    fn validation_requires_an_endpoint_and_typed_fields() {
        validate_provider(
            "deepseek-chat",
            &model("https://api.deepseek.com/v1/chat/completions"),
        )
        .unwrap();
        validate_provider("env-url", &model("${MY_URL}")).unwrap();
        assert!(validate_provider("", &model("https://example.com")).is_err());
        assert!(validate_provider(" padded", &model("https://example.com")).is_err());
        assert!(validate_provider("bad-url", &model("not a url")).is_err());
        let mut bad = model("https://example.com");
        bad["maxOutputTokens"] = json!("lots");
        assert!(validate_provider("bad-tokens", &bad).is_err());
        let mut bad = model("https://example.com");
        bad["supportsImages"] = json!("yes");
        assert!(validate_provider("bad-flag", &bad).is_err());
    }

    #[test]
    fn additive_changes_preserve_other_models_and_keys() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("models.json");
        let settings = dir.path().join("settings.json");
        let original = "{\n  \"models\": [\n    {\"id\": \"keep\", \"name\": \"Keep\", \"url\": \"https://keep.example/v1/chat/completions\"}\n  ],\n  \"extra\": [1, 2]\n}\n";
        fs::write(&path, original).unwrap();

        write_entry(
            &path,
            &settings,
            "cc-model",
            Some(model("https://api.example.com/v1/chat/completions")),
            true,
        )
        .unwrap();
        let text = fs::read_to_string(&path).unwrap();
        // Strict JSON in, strict JSON out (CodeBuddy rejects anything else).
        serde_json::from_str::<Value>(&text).unwrap();
        assert!(text.contains("\"extra\": [1, 2]"));
        let value = read_document(&path).unwrap().value().clone();
        let providers = providers_in(&value).unwrap();
        assert_eq!(providers.keys().collect::<Vec<_>>(), ["keep", "cc-model"]);
        assert_eq!(providers["cc-model"]["id"], "cc-model");
        assert_eq!(value["extra"], json!([1, 2]));

        assert!(write_entry(
            &path,
            &settings,
            "cc-model",
            Some(model("https://x.example")),
            true
        )
        .is_err());

        write_entry(&path, &settings, "cc-model", None, false).unwrap();
        let value = read_document(&path).unwrap().value().clone();
        assert_eq!(
            providers_in(&value).unwrap().keys().collect::<Vec<_>>(),
            ["keep"]
        );
        assert_eq!(value["extra"], json!([1, 2]));
    }

    #[test]
    fn empty_file_is_an_empty_document_and_written_privately() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("models.json");
        let settings = dir.path().join("settings.json");
        fs::write(&path, "").unwrap();
        write_entry(
            &path,
            &settings,
            "m",
            Some(model("https://api.example.com/v1/chat/completions")),
            false,
        )
        .unwrap();
        let value: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(value["models"][0]["id"], "m");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
    }

    #[test]
    fn new_models_stay_visible_when_available_models_filters_the_picker() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("models.json");
        let settings = dir.path().join("settings.json");
        fs::write(&path, r#"{"availableModels":["gpt-5"]}"#).unwrap();
        write_entry(
            &path,
            &settings,
            "m",
            Some(model("https://e.example")),
            false,
        )
        .unwrap();
        let value = read_document(&path).unwrap().value().clone();
        assert_eq!(value["availableModels"], json!(["gpt-5", "m"]));

        fs::write(&path, r#"{"availableModels":[]}"#).unwrap();
        write_entry(
            &path,
            &settings,
            "m",
            Some(model("https://e.example")),
            false,
        )
        .unwrap();
        let value = read_document(&path).unwrap().value().clone();
        assert_eq!(value["availableModels"], json!([]));
    }

    #[test]
    fn refuses_to_remove_the_default_model() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("models.json");
        let settings = dir.path().join("settings.json");
        write_entry(
            &path,
            &settings,
            "chosen",
            Some(model("https://e.example")),
            false,
        )
        .unwrap();
        fs::write(&settings, r#"{"model":"chosen"}"#).unwrap();
        let before = fs::read_to_string(&path).unwrap();
        assert!(write_entry(&path, &settings, "chosen", None, false).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), before);
        fs::write(&settings, r#"{"model":"custom-local:chosen"}"#).unwrap();
        assert!(write_entry(&path, &settings, "chosen", None, false).is_err());
        fs::write(&settings, r#"{"model":"other"}"#).unwrap();
        write_entry(&path, &settings, "chosen", None, false).unwrap();
    }

    #[test]
    fn rejects_files_codebuddy_cannot_parse() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("models.json");
        let settings = dir.path().join("settings.json");
        let text = "{\n  // comment\n  \"models\": []\n}\n";
        fs::write(&path, text).unwrap();
        assert!(write_entry(
            &path,
            &settings,
            "m",
            Some(model("https://e.example")),
            false
        )
        .is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), text);
    }

    #[test]
    fn rejects_non_array_models() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("models.json");
        let settings = dir.path().join("settings.json");
        fs::write(&path, r#"{"models":{"a":1}}"#).unwrap();
        assert!(write_entry(
            &path,
            &settings,
            "m",
            Some(model("https://e.example")),
            false
        )
        .is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), r#"{"models":{"a":1}}"#);
    }
}
