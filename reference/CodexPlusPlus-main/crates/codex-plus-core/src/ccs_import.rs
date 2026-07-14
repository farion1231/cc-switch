use std::path::{Path, PathBuf};

use anyhow::Context;
use rusqlite::Connection;
use serde_json::Value;

use crate::settings::{RelayMode, RelayProfile, RelayProtocol};

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CcsProviderImport {
    pub source_id: String,
    pub name: String,
    pub base_url: String,
    pub api_key: String,
    pub protocol: RelayProtocol,
    pub config_contents: String,
    pub auth_contents: String,
}

pub fn default_ccs_db_path() -> PathBuf {
    home_dir()
        .join(format!(".{}-{}", "cc", "switch"))
        .join(format!("{}-{}.db", "cc", "switch"))
}

pub fn list_codex_providers_from_default_db() -> anyhow::Result<Vec<CcsProviderImport>> {
    list_codex_providers_from_db(&default_ccs_db_path())
}

pub fn list_codex_providers_from_db(path: &Path) -> anyhow::Result<Vec<CcsProviderImport>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let conn = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .with_context(|| format!("failed to open provider database {}", path.display()))?;
    let mut stmt = conn.prepare(
        "SELECT id, name, settings_config
         FROM providers
         WHERE app_type = 'codex'
         ORDER BY COALESCE(sort_index, 999999), created_at ASC, id ASC",
    )?;
    let rows = stmt.query_map([], |row| {
        let source_id: String = row.get(0)?;
        let name: String = row.get(1)?;
        let settings_config: String = row.get(2)?;
        Ok((source_id, name, settings_config))
    })?;

    let mut providers = Vec::new();
    for row in rows {
        let (source_id, name, settings_config) = row?;
        let Ok(config) = serde_json::from_str::<Value>(&settings_config) else {
            continue;
        };
        if let Some(provider) = import_from_ccs_value(&source_id, &name, &config) {
            providers.push(provider);
        }
    }
    Ok(providers)
}

pub fn imported_provider_identity(provider: &RelayProfile) -> String {
    ccs_import_key(
        strip_ccs_name_suffix(&provider.name),
        &provider.upstream_base_url,
    )
}

pub fn provider_identity_from_ccs(provider: &CcsProviderImport) -> String {
    ccs_import_key(&provider.name, &provider.base_url)
}

pub fn relay_profile_from_ccs(
    provider: &CcsProviderImport,
    existing_ids: &[String],
) -> RelayProfile {
    RelayProfile {
        id: unique_profile_id(
            &format!("ccs-{}", sanitize_id(&provider.source_id)),
            existing_ids,
        ),
        name: format!("{}（ccswitch）", strip_ccs_name_suffix(&provider.name)),
        model: String::new(),
        base_url: provider.base_url.clone(),
        upstream_base_url: provider.base_url.clone(),
        api_key: provider.api_key.clone(),
        protocol: provider.protocol,
        relay_mode: RelayMode::PureApi,
        official_mix_api_key: false,
        test_model: String::new(),
        config_contents: provider.config_contents.clone(),
        auth_contents: provider.auth_contents.clone(),
        use_common_config: true,
        context_selection: crate::settings::RelayContextSelection::default(),
        context_selection_initialized: false,
        context_window: String::new(),
        auto_compact_limit: String::new(),
        model_insert_mode: Default::default(),
        model_list: String::new(),
        model_windows: String::new(),
        user_agent: String::new(),
    }
}

fn import_from_ccs_value(source_id: &str, name: &str, config: &Value) -> Option<CcsProviderImport> {
    let base_url = extract_base_url(config)?;
    let api_key = extract_api_key(config).unwrap_or_default();
    let protocol = extract_protocol(config);
    let config_contents = extract_config_contents(config)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| build_config_toml(&base_url, &api_key, protocol));
    let auth_contents = extract_auth_contents(config)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| build_auth_json(&api_key));
    Some(CcsProviderImport {
        source_id: source_id.to_string(),
        name: name.to_string(),
        base_url,
        api_key,
        protocol,
        config_contents,
        auth_contents,
    })
}

fn extract_base_url(config: &Value) -> Option<String> {
    string_at(config, &["base_url", "baseURL"])
        .or_else(|| {
            config
                .get("config")
                .and_then(|value| string_at(value, &["base_url", "baseURL"]))
        })
        .or_else(|| {
            config
                .get("config")
                .and_then(Value::as_str)
                .and_then(extract_toml_base_url)
        })
        .map(trim_trailing_slash)
        .filter(|value| !value.is_empty())
}

fn extract_api_key(config: &Value) -> Option<String> {
    if let Some(key) = config
        .pointer("/env/OPENAI_API_KEY")
        .and_then(Value::as_str)
    {
        return Some(key.to_string());
    }
    if let Some(key) = config
        .pointer("/auth/OPENAI_API_KEY")
        .and_then(Value::as_str)
    {
        return Some(key.to_string());
    }
    string_at(config, &["apiKey", "api_key"]).or_else(|| {
        config
            .get("config")
            .and_then(|value| string_at(value, &["apiKey", "api_key"]))
    })
}

fn extract_protocol(config: &Value) -> RelayProtocol {
    if let Some(api_format) = string_at(config, &["api_format", "apiFormat"]) {
        if is_chat_protocol(&api_format) {
            return RelayProtocol::ChatCompletions;
        }
    }
    if let Some(wire_api) = config
        .get("config")
        .and_then(Value::as_str)
        .and_then(extract_toml_wire_api)
    {
        if is_chat_protocol(&wire_api) {
            return RelayProtocol::ChatCompletions;
        }
    }
    if extract_base_url(config)
        .map(|value| value.to_ascii_lowercase().ends_with("/chat/completions"))
        .unwrap_or(false)
    {
        return RelayProtocol::ChatCompletions;
    }
    RelayProtocol::Responses
}

fn extract_config_contents(config: &Value) -> Option<String> {
    config
        .get("config")
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn extract_auth_contents(config: &Value) -> Option<String> {
    config.get("auth").and_then(|auth| {
        if auth.is_object() {
            serde_json::to_string_pretty(auth)
                .ok()
                .map(|value| format!("{value}\n"))
        } else {
            auth.as_str().map(str::to_string)
        }
    })
}

fn string_at(value: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(Value::as_str))
        .map(str::to_string)
}

fn trim_trailing_slash(value: String) -> String {
    value.trim().trim_end_matches('/').to_string()
}

fn is_chat_protocol(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "chat" | "chat_completions" | "chat-completions" | "openai_chat" | "openai-chat"
    )
}

fn extract_toml_base_url(text: &str) -> Option<String> {
    extract_toml_string_value(text, "base_url")
}

fn extract_toml_wire_api(text: &str) -> Option<String> {
    extract_toml_string_value(text, "wire_api")
}

fn extract_toml_string_value(text: &str, key: &str) -> Option<String> {
    for line in text.lines() {
        let trimmed = line.trim();
        let Some(rest) = trimmed.strip_prefix(key) else {
            continue;
        };
        let rest = rest.trim_start();
        let Some(rest) = rest.strip_prefix('=') else {
            continue;
        };
        let rest = rest.trim_start();
        let quote = rest.chars().next()?;
        if quote != '"' && quote != '\'' {
            continue;
        }
        let rest = &rest[quote.len_utf8()..];
        let end = rest.find(quote)?;
        return Some(rest[..end].to_string());
    }
    None
}

fn build_config_toml(base_url: &str, api_key: &str, protocol: RelayProtocol) -> String {
    let wire_api = match protocol {
        RelayProtocol::Responses => "responses",
        RelayProtocol::ChatCompletions => "chat",
    };
    [
        "model_provider = \"CodexPlusPlus\"".to_string(),
        String::new(),
        "[model_providers.CodexPlusPlus]".to_string(),
        "name = \"CodexPlusPlus\"".to_string(),
        format!("wire_api = \"{wire_api}\""),
        "requires_openai_auth = true".to_string(),
        format!("base_url = \"{}\"", toml_string(base_url)),
        format!("experimental_bearer_token = \"{}\"", toml_string(api_key)),
        String::new(),
    ]
    .join("\n")
}

fn build_auth_json(api_key: &str) -> String {
    format!(
        "{}\n",
        serde_json::to_string_pretty(&serde_json::json!({ "OPENAI_API_KEY": api_key }))
            .unwrap_or_else(|_| "{\"OPENAI_API_KEY\":\"\"}".to_string())
    )
}

fn toml_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn ccs_import_key(name: &str, base_url: &str) -> String {
    format!(
        "{}\n{}",
        strip_ccs_name_suffix(name).trim().to_ascii_lowercase(),
        base_url.trim().trim_end_matches('/').to_ascii_lowercase()
    )
}

fn strip_ccs_name_suffix(name: &str) -> &str {
    name.trim()
        .strip_suffix("（ccswitch）")
        .or_else(|| name.trim().strip_suffix("(ccswitch)"))
        .unwrap_or_else(|| name.trim())
}

fn sanitize_id(value: &str) -> String {
    let mut result = String::new();
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            result.push(ch.to_ascii_lowercase());
        } else if !result.ends_with('-') {
            result.push('-');
        }
    }
    let result = result.trim_matches('-').to_string();
    if result.is_empty() {
        "provider".to_string()
    } else {
        result
    }
}

fn unique_profile_id(base: &str, existing_ids: &[String]) -> String {
    if !existing_ids.iter().any(|id| id == base) {
        return base.to_string();
    }
    let mut index = 2;
    loop {
        let candidate = format!("{base}-{index}");
        if !existing_ids.iter().any(|id| id == &candidate) {
            return candidate;
        }
        index += 1;
    }
}

fn home_dir() -> PathBuf {
    directories::BaseDirs::new()
        .map(|dirs| dirs.home_dir().to_path_buf())
        .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("."))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;
    use serde_json::json;

    fn create_ccs_db(path: &Path) {
        let conn = Connection::open(path).unwrap();
        conn.execute(
            "CREATE TABLE providers (
                id TEXT NOT NULL,
                app_type TEXT NOT NULL,
                name TEXT NOT NULL,
                settings_config TEXT NOT NULL,
                created_at INTEGER,
                sort_index INTEGER,
                PRIMARY KEY (id, app_type)
            )",
            [],
        )
        .unwrap();
    }

    fn insert_provider(path: &Path, id: &str, name: &str, config: Value, sort_index: i64) {
        let conn = Connection::open(path).unwrap();
        conn.execute(
            "INSERT INTO providers (id, app_type, name, settings_config, created_at, sort_index)
             VALUES (?1, 'codex', ?2, ?3, ?4, ?5)",
            params![id, name, config.to_string(), 1000 + sort_index, sort_index],
        )
        .unwrap();
    }

    #[test]
    fn imports_direct_base_url_and_api_key_provider() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join(format!("{}-{}.db", "cc", "switch"));
        create_ccs_db(&db);
        insert_provider(
            &db,
            "one",
            "One",
            json!({
                "base_url": "https://example.com/v1/",
                "apiKey": "key-1",
                "api_format": "chat_completions"
            }),
            1,
        );

        let providers = list_codex_providers_from_db(&db).unwrap();
        assert_eq!(providers.len(), 1);
        assert_eq!(providers[0].base_url, "https://example.com/v1");
        assert_eq!(providers[0].api_key, "key-1");
        assert_eq!(providers[0].protocol, RelayProtocol::ChatCompletions);
    }

    #[test]
    fn imports_toml_config_provider() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join(format!("{}-{}.db", "cc", "switch"));
        create_ccs_db(&db);
        insert_provider(
            &db,
            "two",
            "Two",
            json!({
                "config": "model_provider = 'custom'\n[model_providers.custom]\nwire_api = 'responses'\nbase_url = 'https://relay.example/v1'\n",
                "auth": { "OPENAI_API_KEY": "key-2" }
            }),
            1,
        );

        let providers = list_codex_providers_from_db(&db).unwrap();
        assert_eq!(providers.len(), 1);
        assert_eq!(providers[0].base_url, "https://relay.example/v1");
        assert_eq!(providers[0].api_key, "key-2");
        assert_eq!(providers[0].protocol, RelayProtocol::Responses);
    }
}
