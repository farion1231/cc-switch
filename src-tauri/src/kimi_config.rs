//! Kimi Code configuration read/write helpers.
//!
//! Kimi Code stores user configuration under `KIMI_CODE_HOME` or `~/.kimi-code`.
//! Providers live in `config.toml` under `[providers.<id>]`, model aliases live
//! under `[models.<alias>]`, and MCP servers live in `mcp.json` under
//! `mcpServers`.

use crate::config::{get_home_dir, write_json_file, write_text_file};
use crate::error::AppError;
use crate::settings::get_kimi_override_dir;
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};
use toml_edit::{value, DocumentMut, Item, Table};

pub fn get_kimi_dir() -> PathBuf {
    if let Some(override_dir) = get_kimi_override_dir() {
        return override_dir;
    }

    if let Ok(home) = std::env::var("KIMI_CODE_HOME") {
        let trimmed = home.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }

    get_home_dir().join(".kimi-code")
}

pub fn get_kimi_config_path() -> PathBuf {
    get_kimi_dir().join("config.toml")
}

pub fn get_kimi_mcp_path() -> PathBuf {
    get_kimi_dir().join("mcp.json")
}

pub fn get_kimi_extra_skill_dirs() -> Result<Vec<PathBuf>, AppError> {
    let config_text = read_kimi_config_text()?;
    if config_text.trim().is_empty() {
        return Ok(Vec::new());
    }

    let doc = config_text
        .parse::<toml::Value>()
        .map_err(|e| AppError::Config(format!("TOML 解析错误: config.toml: {e}")))?;
    let Some(extra_dirs) = doc
        .get("extra_skill_dirs")
        .and_then(|value| value.as_array())
    else {
        return Ok(Vec::new());
    };

    Ok(extra_dirs
        .iter()
        .filter_map(|value| value.as_str())
        .filter_map(expand_kimi_skill_dir)
        .collect())
}

fn expand_kimi_skill_dir(raw: &str) -> Option<PathBuf> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    if trimmed == "~" {
        return Some(get_home_dir());
    }

    if let Some(rest) = trimmed
        .strip_prefix("~/")
        .or_else(|| trimmed.strip_prefix("~\\"))
    {
        return Some(get_home_dir().join(rest));
    }

    let path = Path::new(trimmed);
    if path.is_absolute() {
        Some(path.to_path_buf())
    } else {
        Some(get_home_dir().join(path))
    }
}

pub fn read_kimi_config_text() -> Result<String, AppError> {
    let path = get_kimi_config_path();
    if path.exists() {
        std::fs::read_to_string(&path).map_err(|e| AppError::io(&path, e))
    } else {
        Ok(String::new())
    }
}

pub fn validate_config_toml(text: &str) -> Result<(), AppError> {
    if text.trim().is_empty() {
        return Ok(());
    }

    text.parse::<DocumentMut>()
        .map(|_| ())
        .map_err(|e| AppError::Config(format!("TOML 解析错误: config.toml: {e}")))
}

pub fn read_live_settings() -> Result<Value, AppError> {
    Ok(json!({ "config": read_kimi_config_text()? }))
}

fn parse_config_text(text: &str) -> Result<DocumentMut, AppError> {
    if text.trim().is_empty() {
        return Ok(DocumentMut::new());
    }

    text.parse::<DocumentMut>()
        .map_err(|e| AppError::Config(format!("TOML 解析错误: config.toml: {e}")))
}

fn config_doc_from_settings(settings: &Value) -> Result<DocumentMut, AppError> {
    let obj = settings.as_object().ok_or_else(|| {
        AppError::localized(
            "provider.kimi.settings.not_object",
            "Kimi 配置必须是 JSON 对象",
            "Kimi configuration must be a JSON object",
        )
    })?;

    let config_text = obj.get("config").and_then(Value::as_str).unwrap_or("");
    validate_config_toml(config_text)?;
    parse_config_text(config_text)
}

fn provider_item_from_fragment(
    provider_id: &str,
    fragment_doc: &DocumentMut,
) -> Result<Item, AppError> {
    if let Some(item) = fragment_doc
        .get("providers")
        .and_then(|providers| providers.get(provider_id))
    {
        return Ok(item.clone());
    }

    if let Some(providers) = fragment_doc.get("providers").and_then(Item::as_table_like) {
        let mut iter = providers.iter();
        if let (Some((_, item)), None) = (iter.next(), iter.next()) {
            return Ok(item.clone());
        }
    }

    let mut table = Table::new();
    for (key, item) in fragment_doc.as_table().iter() {
        if matches!(
            key,
            "providers" | "models" | "default_model" | "model" | "max_context_size"
        ) {
            continue;
        }
        table.insert(key, item.clone());
    }

    Ok(Item::Table(table))
}

fn ensure_providers_table(doc: &mut DocumentMut) -> Result<(), AppError> {
    if doc.get("providers").is_none() {
        doc["providers"] = Item::Table(Table::new());
    }

    if doc["providers"].as_table_like().is_none() {
        return Err(AppError::Config(
            "Kimi config.toml 中 providers 必须是 TOML 表".to_string(),
        ));
    }

    Ok(())
}

fn ensure_models_table(doc: &mut DocumentMut) -> Result<(), AppError> {
    if doc.get("models").is_none() {
        doc["models"] = Item::Table(Table::new());
    }

    if doc["models"].as_table_like().is_none() {
        return Err(AppError::Config(
            "Kimi config.toml 中 models 必须是 TOML 表".to_string(),
        ));
    }

    Ok(())
}

fn item_str(item: Option<&Item>) -> Option<String> {
    item.and_then(Item::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn model_provider_id(model_item: &Item) -> Option<String> {
    item_str(model_item.get("provider"))
}

fn remove_models_for_provider(doc: &mut DocumentMut, provider_id: &str) {
    let Some(models) = doc.get_mut("models").and_then(Item::as_table_like_mut) else {
        return;
    };

    let to_remove: Vec<String> = models
        .iter()
        .filter_map(|(alias, model)| {
            (model_provider_id(model).as_deref() == Some(provider_id)).then(|| alias.to_string())
        })
        .collect();

    for alias in to_remove {
        models.remove(&alias);
    }
}

fn insert_models_from_fragment(
    doc: &mut DocumentMut,
    provider_id: &str,
    fragment_doc: &DocumentMut,
) -> Result<Option<String>, AppError> {
    ensure_models_table(doc)?;

    let mut inserted_aliases = Vec::new();
    if let Some(fragment_models) = fragment_doc.get("models").and_then(Item::as_table_like) {
        let models = doc["models"]
            .as_table_like_mut()
            .ok_or_else(|| AppError::Config("Kimi models must be a table".to_string()))?;

        for (alias, item) in fragment_models.iter() {
            let mut model_item = item.clone();
            model_item["provider"] = value(provider_id);
            models.insert(alias, model_item);
            inserted_aliases.push(alias.to_string());
        }
    }

    if inserted_aliases.is_empty() {
        let model_id =
            item_str(fragment_doc.get("model")).unwrap_or_else(|| provider_id.to_string());
        let max_context_size = fragment_doc
            .get("max_context_size")
            .and_then(Item::as_integer)
            .unwrap_or(262_144);

        let mut table = Table::new();
        table.insert("provider", value(provider_id));
        table.insert("model", value(model_id));
        table.insert("max_context_size", value(max_context_size));

        let models = doc["models"]
            .as_table_like_mut()
            .ok_or_else(|| AppError::Config("Kimi models must be a table".to_string()))?;
        models.insert(provider_id, Item::Table(table));
        inserted_aliases.push(provider_id.to_string());
    }

    Ok(item_str(fragment_doc.get("default_model")).or_else(|| inserted_aliases.first().cloned()))
}

pub fn set_provider(id: &str, settings: Value) -> Result<(), AppError> {
    let provider_id = id.trim();
    if provider_id.is_empty() {
        return Err(AppError::Config("Kimi provider id 不能为空".to_string()));
    }

    let mut doc = parse_config_text(&read_kimi_config_text()?)?;
    let fragment_doc = config_doc_from_settings(&settings)?;
    let provider_item = provider_item_from_fragment(provider_id, &fragment_doc)?;
    ensure_providers_table(&mut doc)?;
    remove_models_for_provider(&mut doc, provider_id);

    let providers = doc["providers"]
        .as_table_like_mut()
        .ok_or_else(|| AppError::Config("Kimi providers must be a table".to_string()))?;
    providers.insert(provider_id, provider_item);

    if let Some(default_model) = insert_models_from_fragment(&mut doc, provider_id, &fragment_doc)?
    {
        doc["default_model"] = value(default_model);
    }

    write_text_file(&get_kimi_config_path(), &doc.to_string())
}

pub fn remove_provider(id: &str) -> Result<(), AppError> {
    let mut doc = parse_config_text(&read_kimi_config_text()?)?;

    if let Some(providers) = doc
        .get_mut("providers")
        .and_then(|item| item.as_table_like_mut())
    {
        providers.remove(id);
    }

    let removed_default = doc
        .get("default_model")
        .and_then(Item::as_str)
        .is_some_and(|alias| {
            doc.get("models")
                .and_then(|models| models.get(alias))
                .and_then(|model| model.get("provider"))
                .and_then(Item::as_str)
                == Some(id)
        });
    remove_models_for_provider(&mut doc, id);
    if removed_default {
        doc.as_table_mut().remove("default_model");
    }

    write_text_file(&get_kimi_config_path(), &doc.to_string())
}

pub fn get_providers() -> Result<Map<String, Value>, AppError> {
    let doc = parse_config_text(&read_kimi_config_text()?)?;
    let mut out = Map::new();

    let Some(providers) = doc.get("providers").and_then(Item::as_table_like) else {
        return Ok(out);
    };

    let models = doc.get("models").and_then(Item::as_table_like);
    let default_model = doc.get("default_model").and_then(Item::as_str);

    for (id, item) in providers.iter() {
        let mut fragment = DocumentMut::new();
        fragment["providers"] = Item::Table(Table::new());
        if let Some(table) = fragment["providers"].as_table_like_mut() {
            table.insert(id, item.clone());
        }
        if let Some(models) = models {
            for (alias, model_item) in models.iter() {
                if model_provider_id(model_item).as_deref() == Some(id) {
                    if fragment.get("models").is_none() {
                        fragment["models"] = Item::Table(Table::new());
                    }
                    if let Some(table) = fragment["models"].as_table_like_mut() {
                        table.insert(alias, model_item.clone());
                    }
                    if default_model == Some(alias) {
                        fragment["default_model"] = value(alias);
                    }
                }
            }
        }
        out.insert(id.to_string(), json!({ "config": fragment.to_string() }));
    }

    Ok(out)
}

pub fn extract_credentials_from_config(
    config_text: &str,
    provider_id: Option<&str>,
) -> (String, String) {
    let Ok(doc) = config_text.parse::<toml::Value>() else {
        return (String::new(), String::new());
    };

    let active_id = provider_id
        .map(str::to_string)
        .or_else(|| {
            let model_alias = env_string("KIMI_MODEL_NAME")?;
            doc.get("models")
                .and_then(|models| models.get(model_alias))
                .and_then(|model| model.get("provider"))
                .and_then(|provider| provider.as_str())
                .map(str::to_string)
        })
        .or_else(|| {
            let default_model = doc.get("default_model").and_then(|value| value.as_str())?;
            doc.get("models")
                .and_then(|models| models.get(default_model))
                .and_then(|model| model.get("provider"))
                .and_then(|provider| provider.as_str())
                .map(str::to_string)
        })
        .or_else(|| {
            doc.get("current_provider")
                .and_then(|value| value.as_str())
                .map(str::to_string)
        });

    let provider = active_id.as_deref().and_then(|id| {
        doc.get("providers")
            .and_then(|providers| providers.get(id))
            .and_then(|value| value.as_table())
    });

    let provider_type = provider
        .and_then(|table| table.get("type").and_then(|value| value.as_str()))
        .unwrap_or("kimi");
    let (api_key_env, base_url_env) = credential_env_keys(provider_type);
    let model_base_url = env_string("KIMI_MODEL_BASE_URL");
    let model_api_key = env_string("KIMI_MODEL_API_KEY");

    let base_url = model_base_url
        .as_deref()
        .or_else(|| provider.and_then(|table| non_empty_table_str(table, "base_url")))
        .or_else(|| provider.and_then(|table| provider_env_value(table, base_url_env)))
        .or_else(|| doc.get("base_url").and_then(|value| value.as_str()))
        .unwrap_or("")
        .trim_end_matches('/')
        .to_string();

    let api_key = model_api_key
        .as_deref()
        .or_else(|| provider.and_then(|table| non_empty_table_str(table, "api_key")))
        .or_else(|| provider.and_then(|table| provider_env_value(table, api_key_env)))
        .or_else(|| doc.get("api_key").and_then(|value| value.as_str()))
        .unwrap_or("")
        .to_string();

    (base_url, api_key)
}

fn env_string(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn non_empty_table_str<'a>(
    table: &'a toml::map::Map<String, toml::Value>,
    key: &str,
) -> Option<&'a str> {
    table
        .get(key)
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn provider_env_value<'a>(
    table: &'a toml::map::Map<String, toml::Value>,
    key: Option<&str>,
) -> Option<&'a str> {
    let key = key?;
    table
        .get("env")
        .and_then(|value| value.as_table())
        .and_then(|env| env.get(key))
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn credential_env_keys(provider_type: &str) -> (Option<&'static str>, Option<&'static str>) {
    match provider_type {
        "kimi" => (Some("KIMI_API_KEY"), Some("KIMI_BASE_URL")),
        "anthropic" => (Some("ANTHROPIC_API_KEY"), Some("ANTHROPIC_BASE_URL")),
        "openai" | "openai_responses" => (Some("OPENAI_API_KEY"), Some("OPENAI_BASE_URL")),
        "google-genai" => (Some("GOOGLE_API_KEY"), None),
        "vertexai" => (Some("VERTEXAI_API_KEY"), None),
        _ => (None, None),
    }
}

pub fn read_mcp_config() -> Result<Value, AppError> {
    let path = get_kimi_mcp_path();
    if !path.exists() {
        return Ok(json!({ "mcpServers": {} }));
    }

    let content = std::fs::read_to_string(&path).map_err(|e| AppError::io(&path, e))?;
    if content.trim().is_empty() {
        return Ok(json!({ "mcpServers": {} }));
    }

    serde_json::from_str(&content).map_err(|e| AppError::json(&path, e))
}

pub fn write_mcp_config(config: &Value) -> Result<(), AppError> {
    write_json_file(&get_kimi_mcp_path(), config)
}

pub fn get_mcp_servers() -> Result<Map<String, Value>, AppError> {
    let config = read_mcp_config()?;
    Ok(config
        .get("mcpServers")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default())
}

pub fn set_mcp_server(id: &str, spec: Value) -> Result<(), AppError> {
    let mut config = read_mcp_config()?;

    if config.get("mcpServers").is_none() {
        config["mcpServers"] = json!({});
    }

    let Some(servers) = config.get_mut("mcpServers").and_then(Value::as_object_mut) else {
        return Err(AppError::McpValidation(
            "Kimi mcp.json 的 mcpServers 必须是 JSON 对象".to_string(),
        ));
    };

    servers.insert(id.to_string(), spec);
    write_mcp_config(&config)
}

pub fn remove_mcp_server(id: &str) -> Result<(), AppError> {
    let mut config = read_mcp_config()?;

    if let Some(servers) = config.get_mut("mcpServers").and_then(Value::as_object_mut) {
        servers.remove(id);
    }

    write_mcp_config(&config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    fn test_mutex() -> &'static Mutex<()> {
        static MUTEX: OnceLock<Mutex<()>> = OnceLock::new();
        MUTEX.get_or_init(|| Mutex::new(()))
    }

    fn clear_kimi_model_env() -> (
        Option<std::ffi::OsString>,
        Option<std::ffi::OsString>,
        Option<std::ffi::OsString>,
    ) {
        let previous_model_name = std::env::var_os("KIMI_MODEL_NAME");
        let previous_model_api_key = std::env::var_os("KIMI_MODEL_API_KEY");
        let previous_model_base_url = std::env::var_os("KIMI_MODEL_BASE_URL");
        std::env::remove_var("KIMI_MODEL_NAME");
        std::env::remove_var("KIMI_MODEL_API_KEY");
        std::env::remove_var("KIMI_MODEL_BASE_URL");
        (
            previous_model_name,
            previous_model_api_key,
            previous_model_base_url,
        )
    }

    fn restore_kimi_model_env(
        previous_model_name: Option<std::ffi::OsString>,
        previous_model_api_key: Option<std::ffi::OsString>,
        previous_model_base_url: Option<std::ffi::OsString>,
    ) {
        match previous_model_name {
            Some(value) => std::env::set_var("KIMI_MODEL_NAME", value),
            None => std::env::remove_var("KIMI_MODEL_NAME"),
        }
        match previous_model_api_key {
            Some(value) => std::env::set_var("KIMI_MODEL_API_KEY", value),
            None => std::env::remove_var("KIMI_MODEL_API_KEY"),
        }
        match previous_model_base_url {
            Some(value) => std::env::set_var("KIMI_MODEL_BASE_URL", value),
            None => std::env::remove_var("KIMI_MODEL_BASE_URL"),
        }
    }

    #[test]
    fn extract_credentials_resolves_provider_from_default_model_and_env() {
        let _guard = test_mutex().lock().expect("acquire test mutex");
        let previous_env = clear_kimi_model_env();
        let config = r#"
default_model = "kimi-main"

[providers.kimi]
type = "kimi"

[providers.kimi.env]
KIMI_API_KEY = "sk-test"
KIMI_BASE_URL = "https://api.example.com/v1/"

[models.kimi-main]
provider = "kimi"
model = "kimi-k2"
max_context_size = 262144
"#;

        let (base_url, api_key) = extract_credentials_from_config(config, None);
        restore_kimi_model_env(previous_env.0, previous_env.1, previous_env.2);

        assert_eq!(base_url, "https://api.example.com/v1");
        assert_eq!(api_key, "sk-test");
    }

    #[test]
    fn root_provider_fragment_creates_default_model_alias() {
        let _guard = test_mutex().lock().expect("acquire test mutex");
        let fragment = parse_config_text(
            r#"
type = "kimi"
api_key = "sk-test"
base_url = "https://api.example.com/v1"
model = "kimi-k2"
max_context_size = 262144
"#,
        )
        .expect("fragment should parse");
        let provider_item = provider_item_from_fragment("acme", &fragment).expect("provider item");
        let mut doc = DocumentMut::new();
        ensure_providers_table(&mut doc).expect("providers table");
        doc["providers"]
            .as_table_like_mut()
            .expect("providers table mutable")
            .insert("acme", provider_item);

        let default_model =
            insert_models_from_fragment(&mut doc, "acme", &fragment).expect("model alias");
        if let Some(alias) = default_model {
            doc["default_model"] = value(alias);
        }

        assert_eq!(doc["default_model"].as_str(), Some("acme"));
        assert_eq!(doc["models"]["acme"]["provider"].as_str(), Some("acme"));
        assert_eq!(doc["models"]["acme"]["model"].as_str(), Some("kimi-k2"));
        assert!(doc["providers"]["acme"].get("model").is_none());
    }

    #[test]
    fn official_managed_kimi_code_config_uses_quoted_keys() {
        let _guard = test_mutex().lock().expect("acquire test mutex");
        let previous_env = clear_kimi_model_env();
        let config = r#"
default_model = "kimi-code/kimi-for-coding"

[providers."managed:kimi-code"]
type = "kimi"
api_key = "sk-test"
base_url = "https://api.kimi.com/coding/v1"

[models."kimi-code/kimi-for-coding"]
provider = "managed:kimi-code"
model = "kimi-for-coding"
max_context_size = 262144
"#;

        let doc = parse_config_text(config).expect("official config should parse");
        assert_eq!(
            doc["providers"]["managed:kimi-code"]["base_url"].as_str(),
            Some("https://api.kimi.com/coding/v1")
        );
        assert_eq!(
            doc["models"]["kimi-code/kimi-for-coding"]["provider"].as_str(),
            Some("managed:kimi-code")
        );

        let (base_url, api_key) = extract_credentials_from_config(config, None);
        restore_kimi_model_env(previous_env.0, previous_env.1, previous_env.2);
        assert_eq!(base_url, "https://api.kimi.com/coding/v1");
        assert_eq!(api_key, "sk-test");
    }

    #[test]
    fn set_provider_writes_official_managed_kimi_code_config() {
        let _guard = test_mutex().lock().expect("acquire test mutex");
        let previous_env = clear_kimi_model_env();
        let temp = tempfile::tempdir().expect("create temp dir");
        let previous_home = std::env::var_os("KIMI_CODE_HOME");
        std::env::set_var("KIMI_CODE_HOME", temp.path());

        let settings = json!({
            "config": r#"
default_model = "kimi-code/kimi-for-coding"

[providers."managed:kimi-code"]
type = "kimi"
api_key = "sk-test"
base_url = "https://api.kimi.com/coding/v1"

[models."kimi-code/kimi-for-coding"]
provider = "managed:kimi-code"
model = "kimi-for-coding"
max_context_size = 262144
"#
        });

        set_provider("managed:kimi-code", settings).expect("set official provider");
        let written = read_kimi_config_text().expect("read written config");

        match previous_home {
            Some(value) => std::env::set_var("KIMI_CODE_HOME", value),
            None => std::env::remove_var("KIMI_CODE_HOME"),
        }

        assert!(written.contains(r#"default_model = "kimi-code/kimi-for-coding""#));
        assert!(written.contains(r#"[providers."managed:kimi-code"]"#));
        assert!(written.contains(r#"[models."kimi-code/kimi-for-coding"]"#));

        let (base_url, api_key) = extract_credentials_from_config(&written, None);
        restore_kimi_model_env(previous_env.0, previous_env.1, previous_env.2);
        assert_eq!(base_url, "https://api.kimi.com/coding/v1");
        assert_eq!(api_key, "sk-test");
    }

    #[test]
    fn set_provider_preserves_official_schema_fields() {
        let _guard = test_mutex().lock().expect("acquire test mutex");
        let previous_env = clear_kimi_model_env();
        let temp = tempfile::tempdir().expect("create temp dir");
        let previous_home = std::env::var_os("KIMI_CODE_HOME");
        std::env::set_var("KIMI_CODE_HOME", temp.path());

        let settings = json!({
            "config": r#"
default_model = "alias"

[providers.vendor]
type = "openai_responses"
base_url = "https://api.example.com/v1"
custom_provider_field = "kept"

[providers.vendor.env]
OPENAI_API_KEY = "sk-env"

[providers.vendor.headers]
X_Custom_Header = "custom"

[models.alias]
provider = "vendor"
model = "gpt-5.1"
max_context_size = 128000
custom_model_field = "kept"
"#
        });

        set_provider("vendor", settings).expect("set provider");
        let written = read_kimi_config_text().expect("read written config");

        match previous_home {
            Some(value) => std::env::set_var("KIMI_CODE_HOME", value),
            None => std::env::remove_var("KIMI_CODE_HOME"),
        }

        let doc = parse_config_text(&written).expect("written config should parse");
        assert_eq!(doc["default_model"].as_str(), Some("alias"));
        assert_eq!(
            doc["providers"]["vendor"]["custom_provider_field"].as_str(),
            Some("kept")
        );
        assert_eq!(
            doc["providers"]["vendor"]["headers"]["X_Custom_Header"].as_str(),
            Some("custom")
        );
        assert_eq!(
            doc["models"]["alias"]["custom_model_field"].as_str(),
            Some("kept")
        );

        let (base_url, api_key) = extract_credentials_from_config(&written, None);
        restore_kimi_model_env(previous_env.0, previous_env.1, previous_env.2);
        assert_eq!(base_url, "https://api.example.com/v1");
        assert_eq!(api_key, "sk-env");
    }

    #[test]
    fn extract_credentials_supports_official_provider_env_keys() {
        let _guard = test_mutex().lock().expect("acquire test mutex");
        let previous_env = clear_kimi_model_env();
        let cases = [
            ("kimi", "KIMI_API_KEY", Some("KIMI_BASE_URL")),
            ("anthropic", "ANTHROPIC_API_KEY", Some("ANTHROPIC_BASE_URL")),
            ("openai", "OPENAI_API_KEY", Some("OPENAI_BASE_URL")),
            (
                "openai_responses",
                "OPENAI_API_KEY",
                Some("OPENAI_BASE_URL"),
            ),
            ("google-genai", "GOOGLE_API_KEY", None),
            ("vertexai", "VERTEXAI_API_KEY", None),
        ];

        for (provider_type, api_key_env, base_url_env) in cases {
            let mut config = format!(
                r#"
default_model = "main"

[providers.main]
type = "{provider_type}"

[providers.main.env]
{api_key_env} = "sk-{provider_type}"
"#
            );
            if let Some(base_url_env) = base_url_env {
                config.push_str(&format!(
                    "{base_url_env} = \"https://{provider_type}.example.com/v1\"\n"
                ));
            }
            config.push_str(
                r#"
[models.main]
provider = "main"
model = "model"
"#,
            );

            let (base_url, api_key) = extract_credentials_from_config(&config, None);
            assert_eq!(api_key, format!("sk-{provider_type}"));
            if base_url_env.is_some() {
                assert_eq!(base_url, format!("https://{provider_type}.example.com/v1"));
            } else {
                assert_eq!(base_url, "");
            }
        }

        restore_kimi_model_env(previous_env.0, previous_env.1, previous_env.2);
    }

    #[test]
    fn extra_skill_dirs_expand_from_official_config() {
        let _guard = test_mutex().lock().expect("acquire test mutex");
        let temp = tempfile::tempdir().expect("create temp dir");
        let previous_home = std::env::var_os("KIMI_CODE_HOME");
        let previous_test_home = std::env::var_os("CC_SWITCH_TEST_HOME");
        std::env::set_var("KIMI_CODE_HOME", temp.path().join(".kimi-code"));
        std::env::set_var("CC_SWITCH_TEST_HOME", temp.path());

        write_text_file(
            &get_kimi_config_path(),
            r#"extra_skill_dirs = ["~/team-skills", ".agents/team-skills"]"#,
        )
        .expect("write config");

        let dirs = get_kimi_extra_skill_dirs().expect("read extra dirs");

        match previous_home {
            Some(value) => std::env::set_var("KIMI_CODE_HOME", value),
            None => std::env::remove_var("KIMI_CODE_HOME"),
        }
        match previous_test_home {
            Some(value) => std::env::set_var("CC_SWITCH_TEST_HOME", value),
            None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
        }

        assert_eq!(
            dirs,
            vec![
                temp.path().join("team-skills"),
                temp.path().join(".agents").join("team-skills"),
            ]
        );
    }

    #[test]
    fn extract_credentials_respects_official_model_env_overrides() {
        let _guard = test_mutex().lock().expect("acquire test mutex");
        let previous_model_name = std::env::var_os("KIMI_MODEL_NAME");
        let previous_model_api_key = std::env::var_os("KIMI_MODEL_API_KEY");
        let previous_model_base_url = std::env::var_os("KIMI_MODEL_BASE_URL");
        std::env::set_var("KIMI_MODEL_NAME", "override-model");
        std::env::set_var("KIMI_MODEL_API_KEY", "env-key");
        std::env::set_var("KIMI_MODEL_BASE_URL", "https://env.example.com/");
        let config = r#"
default_model = "default"

[providers.default]
type = "kimi"
api_key = "default-key"
base_url = "https://default.example.com"

[providers.override]
type = "kimi"
api_key = "override-key"
base_url = "https://override.example.com"

[models.default]
provider = "default"
model = "kimi-for-coding"

[models.override-model]
provider = "override"
model = "kimi-for-coding"
"#;

        let (base_url, api_key) = extract_credentials_from_config(config, None);

        match previous_model_name {
            Some(value) => std::env::set_var("KIMI_MODEL_NAME", value),
            None => std::env::remove_var("KIMI_MODEL_NAME"),
        }
        match previous_model_api_key {
            Some(value) => std::env::set_var("KIMI_MODEL_API_KEY", value),
            None => std::env::remove_var("KIMI_MODEL_API_KEY"),
        }
        match previous_model_base_url {
            Some(value) => std::env::set_var("KIMI_MODEL_BASE_URL", value),
            None => std::env::remove_var("KIMI_MODEL_BASE_URL"),
        }

        assert_eq!(base_url, "https://env.example.com");
        assert_eq!(api_key, "env-key");
    }
}
