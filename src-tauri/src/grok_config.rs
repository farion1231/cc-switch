//! Grok Build 配置文件管理。
//!
//! Grok Build 将供应商连接信息保存在 `config.toml` 的 `[endpoints]`、
//! `[models]`、`[subagents]` 与 `[model.*]` 中。CC Switch 只重写这些
//! 供应商字段，UI、MCP、遥测等其它配置始终原样保留。

use crate::config::{get_home_dir, write_text_file};
use crate::error::AppError;
use crate::provider::Provider;
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use toml_edit::{value, DocumentMut, Item, Table};

pub const CC_SWITCH_GROK_MODEL_ID: &str = "ccswitch";
pub const GROK_PROXY_TOKEN_PLACEHOLDER: &str = "PROXY_MANAGED";

/// 获取 Grok Build 配置目录。
///
/// 优先级与 Grok Build 自身约定保持一致：`GROK_CONFIG`、`GROK_HOME`、
/// CC Switch 设置覆盖、最后回退到 `~/.grok`。
pub fn get_grok_config_dir() -> PathBuf {
    if let Some(path) = std::env::var_os("GROK_CONFIG").filter(|value| !value.is_empty()) {
        let path = PathBuf::from(path);
        return path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
    }
    if let Some(path) = std::env::var_os("GROK_HOME").filter(|value| !value.is_empty()) {
        return PathBuf::from(path);
    }
    if let Some(custom) = crate::settings::get_grok_override_dir() {
        return custom;
    }
    get_home_dir().join(".grok")
}

pub fn get_grok_config_path() -> PathBuf {
    if let Some(path) = std::env::var_os("GROK_CONFIG").filter(|value| !value.is_empty()) {
        return PathBuf::from(path);
    }
    get_grok_config_dir().join("config.toml")
}

pub fn get_grok_backup_dir() -> PathBuf {
    get_home_dir().join(".grok_switch").join("backups")
}

fn backup_current_config(path: &Path, next_text: &str) -> Result<(), AppError> {
    let Ok(current) = fs::read(path) else {
        return Ok(());
    };
    if current == next_text.as_bytes() {
        return Ok(());
    }

    let backup_dir = get_grok_backup_dir();
    fs::create_dir_all(&backup_dir).map_err(|e| AppError::io(&backup_dir, e))?;
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S-%3f");
    let backup_path = backup_dir.join(format!("config-{stamp}.toml"));
    fs::write(&backup_path, current).map_err(|e| AppError::io(&backup_path, e))?;

    let mut backups = fs::read_dir(&backup_dir)
        .map_err(|e| AppError::io(&backup_dir, e))?
        .filter_map(Result::ok)
        .filter(|entry| {
            entry.file_name().to_string_lossy().starts_with("config-")
                && entry.path().extension().and_then(|ext| ext.to_str()) == Some("toml")
        })
        .collect::<Vec<_>>();
    backups.sort_by_key(|entry| entry.file_name());
    let remove_count = backups.len().saturating_sub(10);
    for entry in backups.into_iter().take(remove_count) {
        fs::remove_file(entry.path()).map_err(|e| AppError::io(entry.path(), e))?;
    }
    Ok(())
}

pub fn read_grok_config_text() -> Result<String, AppError> {
    let path = get_grok_config_path();
    if !path.exists() {
        return Err(AppError::localized(
            "grok.live.missing",
            "Grok Build 配置文件不存在",
            "Grok Build configuration file is missing",
        ));
    }
    std::fs::read_to_string(&path).map_err(|e| AppError::io(&path, e))
}

pub fn write_grok_config_text(text: &str) -> Result<(), AppError> {
    let path = get_grok_config_path();
    validate_config_toml(text)?;
    backup_current_config(&path, text)?;
    write_text_file(&path, text)
}

pub fn validate_config_toml(text: &str) -> Result<(), AppError> {
    text.parse::<DocumentMut>().map(|_| ()).map_err(|e| {
        AppError::localized(
            "provider.grok.config.invalid_toml",
            format!("Grok config.toml 格式错误: {e}"),
            format!("Invalid Grok config.toml: {e}"),
        )
    })
}

fn auth_api_key(settings: &Value) -> Option<String> {
    let auth = settings.get("auth")?.as_object()?;
    ["OPENAI_API_KEY", "XAI_API_KEY", "GROK_API_KEY"]
        .into_iter()
        .find_map(|key| auth.get(key).and_then(Value::as_str))
        .map(str::trim)
        .filter(|key| !key.is_empty())
        .map(ToString::to_string)
}

fn api_backend_from_format(format: Option<&str>) -> Option<&'static str> {
    let normalized = format?.trim().to_ascii_lowercase();
    if matches!(
        normalized.as_str(),
        "chat" | "chat_completions" | "chat-completions" | "openai_chat" | "openai-chat"
    ) {
        Some("chat_completions")
    } else if matches!(
        normalized.as_str(),
        "responses" | "openai_responses" | "openai-responses"
    ) {
        Some("responses")
    } else {
        None
    }
}

pub fn api_format_from_backend(backend: Option<&str>) -> &'static str {
    match backend
        .map(str::trim)
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("chat") | Some("chat_completions") | Some("chat-completions") => "openai_chat",
        _ => "openai_responses",
    }
}

fn selected_model_table(doc: &DocumentMut) -> Option<Table> {
    let active = doc
        .get("models")
        .and_then(|item| item.get("default"))
        .and_then(Item::as_value)
        .and_then(|value| value.as_str());
    if let Some(table) = active.and_then(|name| {
        doc.get("model")
            .and_then(|item| item.get(name))
            .and_then(Item::as_table)
    }) {
        return Some(table.clone());
    }
    if let Some(table) = doc
        .get("model")
        .and_then(|item| item.get(CC_SWITCH_GROK_MODEL_ID))
        .and_then(Item::as_table)
    {
        return Some(table.clone());
    }
    doc.get("model")
        .and_then(Item::as_table)
        .and_then(|models| models.iter().find_map(|(_, item)| item.as_table()))
        .cloned()
}

fn provider_config_doc(provider: &Provider) -> Result<DocumentMut, AppError> {
    let config_text = provider
        .settings_config
        .get("config")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            AppError::localized(
                "provider.grok.config.missing",
                "Grok 供应商缺少 config 配置",
                "Grok provider is missing config",
            )
        })?;
    config_text.parse::<DocumentMut>().map_err(|e| {
        AppError::localized(
            "provider.grok.config.invalid_toml",
            format!("Grok config.toml 格式错误: {e}"),
            format!("Invalid Grok config.toml: {e}"),
        )
    })
}

fn provider_model_table(provider: &Provider) -> Result<Table, AppError> {
    let doc = provider_config_doc(provider)?;
    selected_model_table(&doc).ok_or_else(|| {
        AppError::localized(
            "provider.grok.model.missing",
            "Grok 供应商配置缺少 [model.ccswitch] 模型定义",
            "Grok provider config is missing the [model.ccswitch] model definition",
        )
    })
}

fn set_owned_key(live: &mut DocumentMut, provider: &DocumentMut, section: &str, key: &str) {
    if let Some(item) = provider.get(section).and_then(|item| item.get(key)) {
        live[section][key] = item.clone();
    } else if let Some(table) = live.get_mut(section).and_then(Item::as_table_like_mut) {
        table.remove(key);
    }
}

fn merge_profile_doc(live_doc: &mut DocumentMut, profile_doc: &DocumentMut) {
    for (section, keys) in [
        ("endpoints", &["models_base_url"][..]),
        ("models", &["default", "web_search"][..]),
        ("subagents", &["default_model"][..]),
    ] {
        for key in keys {
            set_owned_key(live_doc, profile_doc, section, key);
        }
    }
    live_doc.as_table_mut().remove("model");
    if let Some(models) = profile_doc.get("model") {
        live_doc["model"] = models.clone();
    }
}

/// 将 Grok Profile 管理的段落加入现有全局配置，同时保留其它全局设置。
pub fn merge_grok_profile_config_text(
    existing_text: &str,
    profile_text: &str,
) -> Result<String, AppError> {
    let mut live_doc = if existing_text.trim().is_empty() {
        DocumentMut::new()
    } else {
        existing_text.parse::<DocumentMut>().map_err(|e| {
            AppError::localized(
                "grok.live.invalid_toml",
                format!("现有 Grok config.toml 格式错误: {e}"),
                format!("Existing Grok config.toml is invalid: {e}"),
            )
        })?
    };
    let profile_doc = profile_text.parse::<DocumentMut>().map_err(|e| {
        AppError::localized(
            "provider.grok.config.invalid_toml",
            format!("Grok Profile TOML 格式错误: {e}"),
            format!("Invalid Grok Profile TOML: {e}"),
        )
    })?;
    selected_model_table(&profile_doc).ok_or_else(|| {
        AppError::localized(
            "provider.grok.model.missing",
            "Grok Profile 缺少 [model.*] 模型定义",
            "Grok Profile is missing a [model.*] model definition",
        )
    })?;

    merge_profile_doc(&mut live_doc, &profile_doc);
    Ok(live_doc.to_string())
}

pub fn merge_grok_profile_into_live(profile_text: &str) -> Result<(), AppError> {
    let existing = fs::read_to_string(get_grok_config_path()).unwrap_or_default();
    let next = merge_grok_profile_config_text(&existing, profile_text)?;
    write_grok_config_text(&next)
}

/// 删除所有供应商拥有的字段，让 Grok 回退到 `grok login` 管理的官方账号。
pub fn use_official_auth_config_text(text: &str) -> Result<String, AppError> {
    let mut doc = if text.trim().is_empty() {
        DocumentMut::new()
    } else {
        text.parse::<DocumentMut>().map_err(|e| {
            AppError::localized(
                "grok.live.invalid_toml",
                format!("现有 Grok config.toml 格式错误: {e}"),
                format!("Existing Grok config.toml is invalid: {e}"),
            )
        })?
    };
    for (section, keys) in [
        ("endpoints", &["models_base_url"][..]),
        ("models", &["default", "web_search"][..]),
        ("subagents", &["default_model"][..]),
    ] {
        if let Some(table) = doc.get_mut(section).and_then(Item::as_table_like_mut) {
            for key in keys {
                table.remove(key);
            }
        }
    }
    doc.as_table_mut().remove("model");
    Ok(doc.to_string())
}

pub fn apply_privacy_protection_config_text(text: &str) -> Result<String, AppError> {
    let mut doc = if text.trim().is_empty() {
        DocumentMut::new()
    } else {
        text.parse::<DocumentMut>().map_err(|e| {
            AppError::localized(
                "grok.live.invalid_toml",
                format!("Grok config.toml 格式错误: {e}"),
                format!("Invalid Grok config.toml: {e}"),
            )
        })?
    };
    doc["features"]["telemetry"] = value(false);
    doc["telemetry"]["trace_upload"] = value(false);
    doc["telemetry"]["mixpanel_enabled"] = value(false);
    doc["harness"]["disable_codebase_upload"] = value(true);
    Ok(doc.to_string())
}

pub fn apply_privacy_protection_live() -> Result<String, AppError> {
    let existing = fs::read_to_string(get_grok_config_path()).unwrap_or_default();
    let next = apply_privacy_protection_config_text(&existing)?;
    write_grok_config_text(&next)?;
    Ok(next)
}

/// 将供应商 Profile patch 到一份 Grok 配置文本中。
///
/// 修改 `[endpoints].models_base_url`、`[models].default/web_search`、
/// `[subagents].default_model` 与全部 `[model.*]`；其它段落原样保留。
/// 调用者可覆盖 `base_url`、`api_key` 与 `api_backend`，用于本地代理接管。
pub fn patch_config_text_for_provider(
    existing_text: &str,
    provider: &Provider,
    base_url_override: Option<&str>,
    api_key_override: Option<&str>,
    api_backend_override: Option<&str>,
) -> Result<String, AppError> {
    let mut live_doc = if existing_text.trim().is_empty() {
        DocumentMut::new()
    } else {
        existing_text.parse::<DocumentMut>().map_err(|e| {
            AppError::localized(
                "grok.live.invalid_toml",
                format!("现有 Grok config.toml 格式错误: {e}"),
                format!("Existing Grok config.toml is invalid: {e}"),
            )
        })?
    };
    if provider.category.as_deref() == Some("official") {
        return use_official_auth_config_text(existing_text);
    }

    let mut provider_doc = provider_config_doc(provider)?;
    provider_model_table(provider)?;

    let api_key = api_key_override
        .map(ToString::to_string)
        .or_else(|| auth_api_key(&provider.settings_config));
    let fallback_backend = api_backend_override.or_else(|| {
        api_backend_from_format(
            provider
                .meta
                .as_ref()
                .and_then(|meta| meta.api_format.as_deref()),
        )
    });
    if let Some(models) = provider_doc.get_mut("model").and_then(Item::as_table_mut) {
        for (_, item) in models.iter_mut() {
            let Some(model) = item.as_table_mut() else {
                continue;
            };
            if let Some(base_url) = base_url_override {
                model["base_url"] = value(base_url);
            }
            if let Some(api_key) = api_key.as_deref() {
                if api_key_override.is_some()
                    || (model.get("api_key").is_none() && model.get("env_key").is_none())
                {
                    model["api_key"] = value(api_key);
                }
            }
            if let Some(backend) = fallback_backend {
                if api_backend_override.is_some() || model.get("api_backend").is_none() {
                    model["api_backend"] = value(backend);
                }
            }
        }
    }

    if let Some(base_url) = base_url_override {
        provider_doc["endpoints"]["models_base_url"] = value(base_url);
    }
    merge_profile_doc(&mut live_doc, &provider_doc);
    Ok(live_doc.to_string())
}

pub fn write_grok_provider_live(provider: &Provider) -> Result<(), AppError> {
    let existing = std::fs::read_to_string(get_grok_config_path()).unwrap_or_default();
    let patched = patch_config_text_for_provider(&existing, provider, None, None, None)?;
    write_grok_config_text(&patched)
}

pub fn write_grok_takeover_live(provider: &Provider, proxy_base_url: &str) -> Result<(), AppError> {
    let existing = std::fs::read_to_string(get_grok_config_path()).unwrap_or_default();
    let patched = patch_config_text_for_provider(
        &existing,
        provider,
        Some(proxy_base_url),
        Some(GROK_PROXY_TOKEN_PLACEHOLDER),
        None,
    )?;
    write_grok_config_text(&patched)
}

/// 将 Grok live 配置转换为供应商存储格式 `{ auth, config }`。
pub fn settings_from_config_text(text: &str) -> Result<Value, AppError> {
    let mut doc = text.parse::<DocumentMut>().map_err(|e| {
        AppError::localized(
            "grok.live.invalid_toml",
            format!("Grok config.toml 格式错误: {e}"),
            format!("Invalid Grok config.toml: {e}"),
        )
    })?;
    selected_model_table(&doc).ok_or_else(|| {
        AppError::localized(
            "grok.live.model_missing",
            "Grok config.toml 中未找到当前模型配置",
            "The active model is missing from Grok config.toml",
        )
    })?;

    let default_model = doc
        .get("models")
        .and_then(|item| item.get("default"))
        .and_then(Item::as_value)
        .and_then(|value| value.as_str())
        .map(ToString::to_string);
    let mut api_key = String::new();
    if let Some(models) = doc.get_mut("model").and_then(Item::as_table_mut) {
        if let Some(name) = default_model.as_deref() {
            if let Some(model) = models.get(name).and_then(Item::as_table) {
                api_key = model
                    .get("api_key")
                    .and_then(Item::as_value)
                    .and_then(|value| value.as_str())
                    .map(ToString::to_string)
                    .or_else(|| {
                        model
                            .get("env_key")
                            .and_then(Item::as_value)
                            .and_then(|value| value.as_str())
                            .and_then(|name| std::env::var(name).ok())
                    })
                    .unwrap_or_default();
            }
        }
        if api_key.is_empty() {
            api_key = models
                .iter()
                .filter_map(|(_, item)| item.as_table())
                .find_map(|model| {
                    model
                        .get("api_key")
                        .and_then(Item::as_value)
                        .and_then(|value| value.as_str())
                        .map(ToString::to_string)
                })
                .unwrap_or_default();
        }
        for (_, item) in models.iter_mut() {
            let Some(model) = item.as_table_mut() else {
                continue;
            };
            let same_as_profile_key = model
                .get("api_key")
                .and_then(Item::as_value)
                .and_then(|value| value.as_str())
                == Some(api_key.as_str());
            if same_as_profile_key {
                model.remove("api_key");
            }
        }
    }

    let mut provider_doc = DocumentMut::new();
    for (section, keys) in [
        ("endpoints", &["models_base_url"][..]),
        ("models", &["default", "web_search"][..]),
        ("subagents", &["default_model"][..]),
    ] {
        for key in keys {
            if let Some(item) = doc.get(section).and_then(|item| item.get(key)) {
                provider_doc[section][key] = item.clone();
            }
        }
    }
    if let Some(models) = doc.get("model") {
        provider_doc["model"] = models.clone();
    }

    Ok(json!({
        "auth": { "OPENAI_API_KEY": api_key },
        "config": provider_doc.to_string(),
    }))
}

pub fn read_grok_live_settings() -> Result<Value, AppError> {
    settings_from_config_text(&read_grok_config_text()?)
}

pub fn infer_api_format_from_settings(settings: &Value) -> &'static str {
    let backend = settings
        .get("config")
        .and_then(Value::as_str)
        .and_then(|text| text.parse::<DocumentMut>().ok())
        .and_then(|doc| selected_model_table(&doc))
        .and_then(|table| {
            table
                .get("api_backend")
                .and_then(Item::as_value)
                .and_then(|value| value.as_str())
                .map(ToString::to_string)
        });
    api_format_from_backend(backend.as_deref())
}

pub fn config_text_has_proxy_placeholder(text: &str) -> bool {
    text.parse::<DocumentMut>()
        .ok()
        .and_then(|doc| {
            doc.get("model").and_then(Item::as_table).map(|models| {
                models.iter().any(|(_, item)| {
                    item.as_table()
                        .and_then(|table| table.get("api_key"))
                        .and_then(Item::as_value)
                        .and_then(|value| value.as_str())
                        == Some(GROK_PROXY_TOKEN_PLACEHOLDER)
                })
            })
        })
        .unwrap_or(false)
}

pub fn active_base_url(text: &str) -> Option<String> {
    let doc = text.parse::<DocumentMut>().ok()?;
    selected_model_table(&doc)
        .and_then(|table| {
            table
                .get("base_url")
                .and_then(Item::as_value)
                .and_then(|value| value.as_str())
                .map(ToString::to_string)
        })
        .or_else(|| {
            doc.get("endpoints")
                .and_then(|item| item.get("models_base_url"))
                .and_then(Item::as_value)
                .and_then(|value| value.as_str())
                .map(ToString::to_string)
        })
}

pub fn cleanup_takeover_config_text(text: &str) -> Result<String, AppError> {
    let mut doc = text.parse::<DocumentMut>().map_err(|e| {
        AppError::localized(
            "grok.live.invalid_toml",
            format!("Grok config.toml 格式错误: {e}"),
            format!("Invalid Grok config.toml: {e}"),
        )
    })?;
    let managed_names = doc
        .get("model")
        .and_then(Item::as_table)
        .map(|models| {
            models
                .iter()
                .filter_map(|(name, item)| {
                    let managed = item
                        .as_table()
                        .and_then(|table| table.get("api_key"))
                        .and_then(Item::as_value)
                        .and_then(|value| value.as_str())
                        == Some(GROK_PROXY_TOKEN_PLACEHOLDER);
                    managed.then(|| name.to_string())
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if !managed_names.is_empty() {
        if let Some(models) = doc.get_mut("model").and_then(Item::as_table_like_mut) {
            for name in &managed_names {
                models.remove(name);
            }
        }
        for (section, key) in [
            ("models", "default"),
            ("models", "web_search"),
            ("subagents", "default_model"),
        ] {
            let selected_is_managed = doc
                .get(section)
                .and_then(|item| item.get(key))
                .and_then(Item::as_value)
                .and_then(|value| value.as_str())
                .is_some_and(|name| managed_names.iter().any(|managed| managed == name));
            if selected_is_managed {
                if let Some(table) = doc.get_mut(section).and_then(Item::as_table_like_mut) {
                    table.remove(key);
                }
            }
        }
        if let Some(endpoints) = doc.get_mut("endpoints").and_then(Item::as_table_like_mut) {
            endpoints.remove("models_base_url");
        }
    }
    Ok(doc.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{Provider, ProviderMeta};

    fn provider() -> Provider {
        let mut provider = Provider::with_id(
            "xai".to_string(),
            "xAI".to_string(),
            json!({
                "auth": { "OPENAI_API_KEY": "xai-key" },
                "config": r#"[endpoints]
models_base_url = "https://api.x.ai/v1"

[models]
default = "fast"
web_search = "search"

[subagents]
default_model = "fast"

[model.fast]
model = "grok-4.5"
base_url = "https://api.x.ai/v1"
name = "Grok 4.5"
api_backend = "responses"
context_window = 500000
supports_backend_search = true

[model.search]
model = "grok-4.5"
base_url = "https://api.x.ai/v1"
api_backend = "responses"
supports_backend_search = true
"#,
            }),
            None,
        );
        provider.meta = Some(ProviderMeta {
            api_format: Some("openai_responses".to_string()),
            ..Default::default()
        });
        provider
    }

    #[test]
    fn patch_replaces_provider_sections_and_preserves_unrelated_config() {
        let existing = r#"[hints]
project_picker_disabled = true

[models]
default = "cli"
default_reasoning_effort = "high"

[model.cli]
model = "old"
base_url = "https://old.example/v1"
api_key = "old-key"
api_backend = "responses"

[mcp_servers.keep]
url = "https://example.test/mcp"
"#;
        let patched = patch_config_text_for_provider(existing, &provider(), None, None, None)
            .expect("patch Grok config");
        let doc = patched
            .parse::<DocumentMut>()
            .expect("parse patched config");

        assert_eq!(doc["models"]["default"].as_str(), Some("fast"));
        assert_eq!(doc["models"]["web_search"].as_str(), Some("search"));
        assert_eq!(doc["subagents"]["default_model"].as_str(), Some("fast"));
        assert_eq!(
            doc["models"]["default_reasoning_effort"].as_str(),
            Some("high")
        );
        assert!(doc["model"].get("cli").is_none());
        assert_eq!(doc["model"]["fast"]["api_key"].as_str(), Some("xai-key"));
        assert_eq!(
            doc["endpoints"]["models_base_url"].as_str(),
            Some("https://api.x.ai/v1")
        );
        assert_eq!(
            doc["mcp_servers"]["keep"]["url"].as_str(),
            Some("https://example.test/mcp")
        );
    }

    #[test]
    fn merge_profile_into_global_config_preserves_unmanaged_sections() {
        let existing = r#"[features]
telemetry = false

[models]
default = "old"
default_reasoning_effort = "high"

[model.old]
model = "old-model"
"#;
        let profile = provider().settings_config["config"]
            .as_str()
            .expect("profile config");
        let merged = merge_grok_profile_config_text(existing, profile).expect("merge profile");
        let doc = merged.parse::<DocumentMut>().expect("parse merged config");

        assert_eq!(doc["features"]["telemetry"].as_bool(), Some(false));
        assert_eq!(
            doc["models"]["default_reasoning_effort"].as_str(),
            Some("high")
        );
        assert_eq!(doc["models"]["default"].as_str(), Some("fast"));
        assert!(doc["model"].get("old").is_none());
        assert!(doc["model"]["fast"].is_table());
    }

    #[test]
    fn live_import_preserves_multi_model_profile_and_moves_shared_key_to_auth() {
        let settings = settings_from_config_text(
            r#"[endpoints]
models_base_url = "https://api.example/v1"

[models]
default = "cli"
web_search = "search"

[subagents]
default_model = "cli"

[model.cli]
model = "grok-4.5"
base_url = "https://api.example/v1"
api_key = "secret"
api_backend = "chat_completions"

[model.search]
model = "grok-4.5-search"
base_url = "https://api.example/v1"
api_key = "secret"
api_backend = "responses"
supports_backend_search = true
"#,
        )
        .expect("import Grok config");

        assert_eq!(settings["auth"]["OPENAI_API_KEY"], "secret");
        let config = settings["config"].as_str().expect("config string");
        let config_doc = config
            .parse::<DocumentMut>()
            .expect("parse provider config");
        assert_eq!(config_doc["models"]["default"].as_str(), Some("cli"));
        assert_eq!(config_doc["models"]["web_search"].as_str(), Some("search"));
        assert!(config_doc["model"]["cli"].is_table());
        assert!(config_doc["model"]["search"].is_table());
        assert!(!config.contains("secret"));
        assert_eq!(infer_api_format_from_settings(&settings), "openai_chat");
    }

    #[test]
    fn takeover_routes_every_profile_model_without_changing_backend() {
        let patched = patch_config_text_for_provider(
            "[features]\ntelemetry = false\n",
            &provider(),
            Some("http://127.0.0.1:15721/grok/v1"),
            Some(GROK_PROXY_TOKEN_PLACEHOLDER),
            None,
        )
        .expect("patch takeover config");
        let doc = patched.parse::<DocumentMut>().expect("parse takeover");
        for name in ["fast", "search"] {
            assert_eq!(
                doc["model"][name]["base_url"].as_str(),
                Some("http://127.0.0.1:15721/grok/v1")
            );
            assert_eq!(
                doc["model"][name]["api_key"].as_str(),
                Some(GROK_PROXY_TOKEN_PLACEHOLDER)
            );
            assert_eq!(
                doc["model"][name]["api_backend"].as_str(),
                Some("responses")
            );
        }
        assert_eq!(doc["features"]["telemetry"].as_bool(), Some(false));
    }

    #[test]
    fn official_auth_removes_only_provider_owned_fields() {
        let cleaned = use_official_auth_config_text(
            r#"[features]
telemetry = false

[models]
default = "custom"
web_search = "custom"
default_reasoning_effort = "high"

[model.custom]
model = "grok-4.5"
api_key = "secret"
"#,
        )
        .expect("clean provider overrides");
        let doc = cleaned
            .parse::<DocumentMut>()
            .expect("parse official config");
        assert!(doc.get("model").is_none());
        assert!(doc["models"].get("default").is_none());
        assert_eq!(
            doc["models"]["default_reasoning_effort"].as_str(),
            Some("high")
        );
        assert_eq!(doc["features"]["telemetry"].as_bool(), Some(false));
    }

    #[test]
    fn privacy_protection_preserves_provider_profile() {
        let protected = apply_privacy_protection_config_text(
            &provider().settings_config["config"].as_str().unwrap(),
        )
        .expect("apply privacy protection");
        let doc = protected
            .parse::<DocumentMut>()
            .expect("parse protected config");
        assert_eq!(doc["features"]["telemetry"].as_bool(), Some(false));
        assert_eq!(doc["telemetry"]["trace_upload"].as_bool(), Some(false));
        assert_eq!(doc["telemetry"]["mixpanel_enabled"].as_bool(), Some(false));
        assert_eq!(
            doc["harness"]["disable_codebase_upload"].as_bool(),
            Some(true)
        );
        assert!(doc["model"]["fast"].is_table());
    }
}
