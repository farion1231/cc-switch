use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use indexmap::IndexMap;
use json_five::rt::parser::{
    from_str as rt_from_str, JSONKeyValuePair as RtJSONKeyValuePair,
    JSONObjectContext as RtJSONObjectContext, JSONText as RtJSONText, JSONValue as RtJSONValue,
    KeyValuePairContext as RtKeyValuePairContext,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::config::{atomic_write, get_openclaw_config_dir};
use crate::error::AppError;

const OPENCLAW_DEFAULT_SOURCE: &str =
    "{\n  models: {\n    mode: 'merge',\n    providers: {},\n  },\n}\n";

pub fn get_openclaw_dir() -> PathBuf {
    get_openclaw_config_dir()
}

pub fn get_openclaw_config_path() -> PathBuf {
    get_openclaw_dir().join("openclaw.json")
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

    fn save(self) -> Result<(), AppError> {
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
            return Ok(());
        }

        atomic_write(&self.path, next_source.as_bytes())
    }
}

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
    #[serde(flatten, default, skip_serializing_if = "HashMap::is_empty")]
    pub extra: HashMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenClawModelEntry {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alias: Option<String>,
    #[serde(flatten, default, skip_serializing_if = "HashMap::is_empty")]
    pub extra: HashMap<String, Value>,
}

pub fn read_openclaw_config() -> Result<Value, AppError> {
    let path = get_openclaw_config_path();
    if !path.exists() {
        return Ok(default_openclaw_config_value());
    }

    let content = fs::read_to_string(&path).map_err(|e| AppError::io(&path, e))?;
    json5::from_str(&content)
        .map_err(|e| AppError::Config(format!("Failed to parse OpenClaw config as JSON5: {e}")))
}

pub fn write_openclaw_config(config: &Value) -> Result<(), AppError> {
    let root = config
        .as_object()
        .ok_or_else(|| AppError::Config("OpenClaw config root must be an object".to_string()))?;

    let current_config = read_openclaw_config()?;
    let mut document = OpenClawConfigDocument::load()?;
    for (section, value) in root {
        if current_config.get(section) == Some(value) {
            continue;
        }
        document.set_root_section(section, value)?;
    }
    document.save()
}

pub fn get_providers() -> Result<Map<String, Value>, AppError> {
    let config = read_openclaw_config()?;
    Ok(config
        .get("models")
        .and_then(|value| value.get("providers"))
        .and_then(|value| value.as_object())
        .cloned()
        .unwrap_or_default())
}

pub fn set_provider(id: &str, provider_config: Value) -> Result<(), AppError> {
    let mut full_config = read_openclaw_config()?;

    if full_config.get("models").is_none() {
        full_config["models"] = json!({
            "mode": "merge",
            "providers": {}
        });
    }

    if full_config["models"].get("providers").is_none() {
        full_config["models"]["providers"] = json!({});
    }

    if let Some(providers) = full_config["models"]
        .get_mut("providers")
        .and_then(|value| value.as_object_mut())
    {
        providers.insert(id.to_string(), provider_config);
    }

    write_openclaw_config(&full_config)
}

pub fn remove_provider(id: &str) -> Result<(), AppError> {
    let mut config = read_openclaw_config()?;
    if let Some(providers) = config
        .get_mut("models")
        .and_then(|value| value.get_mut("providers"))
        .and_then(|value| value.as_object_mut())
    {
        providers.remove(id);
    }
    write_openclaw_config(&config)
}

pub fn get_typed_providers() -> Result<IndexMap<String, OpenClawProviderConfig>, AppError> {
    let providers = get_providers()?;
    let mut result = IndexMap::new();

    for (id, value) in providers {
        match serde_json::from_value::<OpenClawProviderConfig>(value.clone()) {
            Ok(config) => {
                result.insert(id, config);
            }
            Err(err) => {
                log::warn!("Failed to parse OpenClaw provider '{id}': {err}");
            }
        }
    }

    Ok(result)
}

pub fn set_typed_provider(id: &str, config: &OpenClawProviderConfig) -> Result<(), AppError> {
    let value = serde_json::to_value(config).map_err(|e| AppError::JsonSerialize { source: e })?;
    set_provider(id, value)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenClawModelCost {
    pub input: f64,
    pub output: f64,
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenClawDefaultModel {
    pub primary: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fallbacks: Vec<String>,
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenClawModelCatalogEntry {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alias: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost: Option<OpenClawModelCost>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u32>,
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenClawAgentsDefaults {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<OpenClawDefaultModel>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub models: Option<HashMap<String, OpenClawModelCatalogEntry>>,
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenClawEnvConfig {
    #[serde(flatten)]
    pub vars: HashMap<String, Value>,
}

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

pub fn get_default_model() -> Result<Option<OpenClawDefaultModel>, AppError> {
    let config = read_openclaw_config()?;
    let Some(model_value) = config
        .get("agents")
        .and_then(|agents| agents.get("defaults"))
        .and_then(|defaults| defaults.get("model"))
    else {
        return Ok(None);
    };

    let model = serde_json::from_value(model_value.clone())
        .map_err(|e| AppError::Config(format!("Failed to parse agents.defaults.model: {e}")))?;
    Ok(Some(model))
}

pub fn set_default_model(model: &OpenClawDefaultModel) -> Result<(), AppError> {
    let mut config = read_openclaw_config()?;
    ensure_agents_defaults_path(&mut config);
    let model_value =
        serde_json::to_value(model).map_err(|e| AppError::JsonSerialize { source: e })?;
    config["agents"]["defaults"]["model"] = model_value;
    write_openclaw_config(&config)
}

pub fn get_model_catalog() -> Result<Option<HashMap<String, OpenClawModelCatalogEntry>>, AppError> {
    let config = read_openclaw_config()?;
    let Some(models_value) = config
        .get("agents")
        .and_then(|agents| agents.get("defaults"))
        .and_then(|defaults| defaults.get("models"))
    else {
        return Ok(None);
    };

    let catalog = serde_json::from_value(models_value.clone())
        .map_err(|e| AppError::Config(format!("Failed to parse agents.defaults.models: {e}")))?;
    Ok(Some(catalog))
}

pub fn set_model_catalog(
    catalog: &HashMap<String, OpenClawModelCatalogEntry>,
) -> Result<(), AppError> {
    let mut config = read_openclaw_config()?;
    ensure_agents_defaults_path(&mut config);
    let catalog_value =
        serde_json::to_value(catalog).map_err(|e| AppError::JsonSerialize { source: e })?;
    config["agents"]["defaults"]["models"] = catalog_value;
    write_openclaw_config(&config)
}

pub fn get_agents_defaults() -> Result<Option<OpenClawAgentsDefaults>, AppError> {
    let config = read_openclaw_config()?;
    let Some(defaults_value) = config
        .get("agents")
        .and_then(|agents| agents.get("defaults"))
    else {
        return Ok(None);
    };

    let defaults = serde_json::from_value(defaults_value.clone())
        .map_err(|e| AppError::Config(format!("Failed to parse agents.defaults: {e}")))?;
    Ok(Some(defaults))
}

pub fn set_agents_defaults(defaults: &OpenClawAgentsDefaults) -> Result<(), AppError> {
    let mut config = read_openclaw_config()?;
    if config.get("agents").is_none() {
        config["agents"] = json!({});
    }

    let value =
        serde_json::to_value(defaults).map_err(|e| AppError::JsonSerialize { source: e })?;
    config["agents"]["defaults"] = value;
    write_openclaw_config(&config)
}

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

fn ordered_env_value(env: &OpenClawEnvConfig) -> Value {
    let mut entries: Vec<_> = env.vars.iter().collect();
    entries.sort_by(|(left, _), (right, _)| left.cmp(right));

    let mut map = Map::new();
    for (key, value) in entries {
        map.insert(key.clone(), value.clone());
    }

    Value::Object(map)
}

pub fn set_env_config(env: &OpenClawEnvConfig) -> Result<(), AppError> {
    let mut config = read_openclaw_config()?;
    config["env"] = ordered_env_value(env);
    write_openclaw_config(&config)
}

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

pub fn set_tools_config(tools: &OpenClawToolsConfig) -> Result<(), AppError> {
    let mut config = read_openclaw_config()?;
    let value = serde_json::to_value(tools).map_err(|e| AppError::JsonSerialize { source: e })?;
    config["tools"] = value;
    write_openclaw_config(&config)
}

fn ensure_agents_defaults_path(config: &mut Value) {
    if config.get("agents").is_none() {
        config["agents"] = json!({});
    }
    if config["agents"].get("defaults").is_none() {
        config["agents"]["defaults"] = json!({});
    }
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
    // Match the legacy Tauri write path exactly so parity is driven by one serializer.
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

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use tempfile::tempdir;

    #[test]
    #[serial]
    fn openclaw_provider_round_trip() -> Result<(), AppError> {
        let temp = tempdir().expect("tempdir");
        std::env::set_var("CC_SWITCH_TEST_HOME", temp.path());

        set_typed_provider(
            "demo",
            &OpenClawProviderConfig {
                base_url: Some("https://example.com/v1".to_string()),
                api_key: Some("key".to_string()),
                api: Some("openai-completions".to_string()),
                models: vec![OpenClawModelEntry {
                    id: "gpt-4.1".to_string(),
                    name: Some("GPT-4.1".to_string()),
                    alias: None,
                    extra: HashMap::new(),
                }],
                extra: HashMap::new(),
            },
        )?;

        let providers = get_typed_providers()?;
        assert_eq!(
            providers
                .get("demo")
                .and_then(|item| item.base_url.as_deref()),
            Some("https://example.com/v1")
        );

        Ok(())
    }

    #[test]
    #[serial]
    fn openclaw_agents_defaults_round_trip() -> Result<(), AppError> {
        let temp = tempdir().expect("tempdir");
        std::env::set_var("CC_SWITCH_TEST_HOME", temp.path());

        let default_model = OpenClawDefaultModel {
            primary: "demo/gpt-5".to_string(),
            fallbacks: vec!["demo/gpt-4.1".to_string()],
            extra: HashMap::new(),
        };
        set_default_model(&default_model)?;

        let mut catalog = HashMap::new();
        catalog.insert(
            "demo/gpt-5".to_string(),
            OpenClawModelCatalogEntry {
                alias: Some("GPT-5".to_string()),
                cost: Some(OpenClawModelCost {
                    input: 1.0,
                    output: 2.0,
                    extra: HashMap::new(),
                }),
                context_window: Some(200_000),
                extra: HashMap::new(),
            },
        );
        set_model_catalog(&catalog)?;

        let defaults = get_agents_defaults()?.expect("agents defaults should exist");
        assert_eq!(
            defaults.model.as_ref().map(|model| model.primary.as_str()),
            Some("demo/gpt-5")
        );
        assert_eq!(
            defaults
                .models
                .as_ref()
                .and_then(|models| models.get("demo/gpt-5"))
                .and_then(|entry| entry.alias.as_deref()),
            Some("GPT-5")
        );

        Ok(())
    }

    #[test]
    #[serial]
    fn openclaw_env_and_tools_round_trip() -> Result<(), AppError> {
        let temp = tempdir().expect("tempdir");
        std::env::set_var("CC_SWITCH_TEST_HOME", temp.path());

        let mut env_vars = HashMap::new();
        env_vars.insert(
            "OPENAI_API_KEY".to_string(),
            Value::String("sk-openclaw".to_string()),
        );
        set_env_config(&OpenClawEnvConfig { vars: env_vars })?;

        set_tools_config(&OpenClawToolsConfig {
            profile: Some("strict".to_string()),
            allow: vec!["read:*".to_string()],
            deny: vec!["write:*".to_string()],
            extra: HashMap::new(),
        })?;

        let env = get_env_config()?;
        let tools = get_tools_config()?;
        assert_eq!(
            env.vars.get("OPENAI_API_KEY").and_then(Value::as_str),
            Some("sk-openclaw")
        );
        assert_eq!(tools.profile.as_deref(), Some("strict"));
        assert_eq!(tools.allow, vec!["read:*".to_string()]);
        assert_eq!(tools.deny, vec!["write:*".to_string()]);

        Ok(())
    }

    #[test]
    #[serial]
    fn openclaw_env_write_preserves_legacy_json5_shape() -> Result<(), AppError> {
        let temp = tempdir().expect("tempdir");
        std::env::set_var("CC_SWITCH_TEST_HOME", temp.path());

        let mut env_vars = HashMap::new();
        env_vars.insert(
            "ANTHROPIC_API_KEY".to_string(),
            Value::String("legacy-anthropic".to_string()),
        );
        env_vars.insert(
            "OPENAI_API_KEY".to_string(),
            Value::String("legacy-openai".to_string()),
        );

        set_env_config(&OpenClawEnvConfig { vars: env_vars })?;

        let path = get_openclaw_config_path();
        let written = fs::read_to_string(&path).map_err(|e| AppError::io(&path, e))?;

        assert!(written.contains("models: {"));
        assert!(written.contains("providers: {},"));
        assert!(written.contains("env: {"));
        assert!(written.contains("\"ANTHROPIC_API_KEY\": \"legacy-anthropic\""));
        assert!(written.contains("\"OPENAI_API_KEY\": \"legacy-openai\""));

        let parsed: Value =
            json5::from_str(&written).map_err(|e| AppError::Config(e.to_string()))?;
        assert_eq!(
            parsed
                .get("env")
                .and_then(|env| env.get("ANTHROPIC_API_KEY")),
            Some(&Value::String("legacy-anthropic".to_string()))
        );
        assert_eq!(
            parsed.get("env").and_then(|env| env.get("OPENAI_API_KEY")),
            Some(&Value::String("legacy-openai".to_string()))
        );

        Ok(())
    }
}
