// unused imports removed
use std::path::PathBuf;

use crate::config::{
    atomic_write, delete_file, get_home_dir, sanitize_provider_name, write_json_file,
    write_text_file,
};
use crate::error::AppError;
use serde_json::Value;
use std::fs;
use std::path::Path;
use toml_edit::DocumentMut;

pub const CC_SWITCH_CODEX_MODEL_PROVIDER_ID: &str = "ccswitch";

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

fn is_custom_codex_model_provider_id(id: &str) -> bool {
    let id = id.trim();
    !id.is_empty()
        && !CODEX_RESERVED_MODEL_PROVIDER_IDS
            .iter()
            .any(|reserved| reserved.eq_ignore_ascii_case(id))
}

fn stable_codex_model_provider_id_from_config(config_text: &str) -> Option<String> {
    let doc = config_text.parse::<DocumentMut>().ok()?;
    let provider_id = active_codex_model_provider_id(&doc)?;

    if is_custom_codex_model_provider_id(&provider_id) {
        Some(provider_id)
    } else {
        None
    }
}

fn codex_model_provider_id_with_table_from_config(
    config_text: &str,
) -> Result<Option<String>, AppError> {
    if config_text.trim().is_empty() {
        return Ok(None);
    }

    let doc = config_text
        .parse::<DocumentMut>()
        .map_err(|e| AppError::Message(format!("Invalid Codex config.toml: {e}")))?;
    let Some(provider_id) = active_codex_model_provider_id(&doc) else {
        return Ok(None);
    };

    let has_provider_table = doc
        .get("model_providers")
        .and_then(|item| item.as_table())
        .and_then(|table| table.get(provider_id.as_str()))
        .is_some();

    Ok(has_provider_table.then_some(provider_id))
}

fn normalize_codex_live_config_model_provider_with_anchors<'a>(
    config_text: &str,
    anchor_config_texts: impl IntoIterator<Item = &'a str>,
) -> Result<String, AppError> {
    if config_text.trim().is_empty() {
        return Ok(config_text.to_string());
    }

    let mut doc = config_text
        .parse::<DocumentMut>()
        .map_err(|e| AppError::Message(format!("Invalid Codex config.toml: {e}")))?;

    let Some(source_provider_id) = active_codex_model_provider_id(&doc) else {
        return Ok(config_text.to_string());
    };

    let has_source_provider_table = doc
        .get("model_providers")
        .and_then(|item| item.as_table())
        .and_then(|table| table.get(source_provider_id.as_str()))
        .is_some();
    if !has_source_provider_table {
        return Ok(config_text.to_string());
    }

    let stable_provider_id = anchor_config_texts
        .into_iter()
        .find_map(stable_codex_model_provider_id_from_config)
        .or_else(|| {
            is_custom_codex_model_provider_id(&source_provider_id)
                .then(|| source_provider_id.clone())
        })
        .unwrap_or_else(|| CC_SWITCH_CODEX_MODEL_PROVIDER_ID.to_string());

    if stable_provider_id == source_provider_id {
        return Ok(config_text.to_string());
    }

    if let Some(model_providers) = doc
        .get_mut("model_providers")
        .and_then(|item| item.as_table_mut())
    {
        let Some(provider_table) = model_providers.remove(source_provider_id.as_str()) else {
            return Ok(config_text.to_string());
        };
        model_providers[stable_provider_id.as_str()] = provider_table;
    }

    rewrite_codex_profile_model_provider_refs(&mut doc, &source_provider_id, &stable_provider_id);
    doc["model_provider"] = toml_edit::value(stable_provider_id.as_str());

    Ok(doc.to_string())
}

fn rewrite_codex_profile_model_provider_refs(
    doc: &mut DocumentMut,
    source_provider_id: &str,
    stable_provider_id: &str,
) {
    let Some(profiles) = doc
        .get_mut("profiles")
        .and_then(|item| item.as_table_like_mut())
    else {
        return;
    };

    let profile_keys: Vec<String> = profiles.iter().map(|(key, _)| key.to_string()).collect();
    for profile_key in profile_keys {
        let Some(profile_table) = profiles
            .get_mut(&profile_key)
            .and_then(|item| item.as_table_like_mut())
        else {
            continue;
        };

        let references_source = profile_table
            .get("model_provider")
            .and_then(|item| item.as_str())
            == Some(source_provider_id);
        if references_source {
            profile_table.insert("model_provider", toml_edit::value(stable_provider_id));
        }
    }
}

/// Keep Codex's active `model_provider` stable across CC Switch provider changes.
///
/// Codex stores and filters resume history by `model_provider`, so switching between
/// provider-specific ids like `rightcode` and `aihubmix` makes history appear to move.
/// We preserve an existing custom provider id when possible and only rewrite the
/// live config text that Codex sees at provider-driven write boundaries.
pub fn normalize_codex_settings_config_model_provider(
    settings: &mut Value,
    anchor_config_text: Option<&str>,
) -> Result<(), AppError> {
    let Some(config_text) = settings
        .get("config")
        .and_then(|value| value.as_str())
        .map(str::to_string)
    else {
        return Ok(());
    };

    let current_config_text = read_codex_config_text().ok();
    let anchors = anchor_config_text
        .into_iter()
        .chain(current_config_text.as_deref());
    let normalized =
        normalize_codex_live_config_model_provider_with_anchors(&config_text, anchors)?;

    if let Some(obj) = settings.as_object_mut() {
        obj.insert("config".to_string(), Value::String(normalized));
    }

    Ok(())
}

fn restore_codex_backfill_model_provider_id(
    config_text: &str,
    template_config_text: &str,
) -> Result<String, AppError> {
    let Some(template_provider_id) =
        codex_model_provider_id_with_table_from_config(template_config_text)?
    else {
        return Ok(config_text.to_string());
    };

    if config_text.trim().is_empty() {
        return Ok(config_text.to_string());
    }

    let mut doc = config_text
        .parse::<DocumentMut>()
        .map_err(|e| AppError::Message(format!("Invalid Codex config.toml: {e}")))?;
    let Some(live_provider_id) = active_codex_model_provider_id(&doc) else {
        return Ok(config_text.to_string());
    };

    if live_provider_id == template_provider_id {
        return Ok(config_text.to_string());
    }

    if let Some(model_providers) = doc
        .get_mut("model_providers")
        .and_then(|item| item.as_table_mut())
    {
        let Some(provider_table) = model_providers.remove(live_provider_id.as_str()) else {
            return Ok(config_text.to_string());
        };
        model_providers[template_provider_id.as_str()] = provider_table;
    } else {
        return Ok(config_text.to_string());
    }

    rewrite_codex_profile_model_provider_refs(&mut doc, &live_provider_id, &template_provider_id);
    doc["model_provider"] = toml_edit::value(template_provider_id.as_str());

    Ok(doc.to_string())
}

/// Convert a Codex live config that was normalized for history stability back
/// to the provider-specific id used by the stored provider template.
pub fn restore_codex_settings_config_model_provider_for_backfill(
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
    let Some(template_config_text) = template_settings
        .get("config")
        .and_then(|value| value.as_str())
    else {
        return Ok(());
    };

    let restored = restore_codex_backfill_model_provider_id(&config_text, template_config_text)?;
    if let Some(obj) = settings.as_object_mut() {
        obj.insert("config".to_string(), Value::String(restored));
    }

    Ok(())
}

/// Atomically write Codex live config after normalizing provider-specific ids.
///
/// Use this for provider-driven live writes. Keep `write_codex_live_atomic` available
/// for exact restore/backup paths that must preserve the config text byte-for-byte.
pub fn write_codex_live_atomic_with_stable_provider(
    auth: &Value,
    config_text_opt: Option<&str>,
) -> Result<(), AppError> {
    match config_text_opt {
        Some(config_text) => {
            let mut settings = serde_json::Map::new();
            settings.insert("config".to_string(), Value::String(config_text.to_string()));
            let mut settings = Value::Object(settings);
            normalize_codex_settings_config_model_provider(&mut settings, None)?;
            let config_text = settings
                .get("config")
                .and_then(|value| value.as_str())
                .unwrap_or(config_text);
            write_codex_live_atomic(auth, Some(config_text))
        }
        None => write_codex_live_atomic(auth, None),
    }
}

/// Update a field in Codex config.toml using toml_edit (syntax-preserving).
///
/// Supported fields:
/// - `"base_url"`: writes to `[model_providers.<current>].base_url` if `model_provider` exists,
///   otherwise falls back to top-level `base_url`.
/// - `"model"`: writes to top-level `model` field.
///
/// Empty value removes the field.
pub fn update_codex_toml_field(toml_str: &str, field: &str, value: &str) -> Result<String, String> {
    let mut doc = toml_str
        .parse::<DocumentMut>()
        .map_err(|e| format!("TOML parse error: {e}"))?;

    let trimmed = value.trim();

    match field {
        "base_url" => {
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
                            provider_table.remove("base_url");
                        } else {
                            provider_table["base_url"] = toml_edit::value(trimmed);
                        }
                        return Ok(doc.to_string());
                    }
                }
            }

            // Fallback: no model_provider or structure mismatch → top-level base_url
            if trimmed.is_empty() {
                doc.as_table_mut().remove("base_url");
            } else {
                doc["base_url"] = toml_edit::value(trimmed);
            }
        }
        "model" => {
            if trimmed.is_empty() {
                doc.as_table_mut().remove("model");
            } else {
                doc["model"] = toml_edit::value(trimmed);
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

    #[test]
    fn normalize_live_config_preserves_current_custom_model_provider_id() {
        let current = r#"model_provider = "rightcode"

[model_providers.rightcode]
name = "RightCode"
base_url = "https://rightcode.example/v1"
wire_api = "responses"
"#;
        let target = r#"model_provider = "aihubmix"
model = "gpt-5.4"

[model_providers.aihubmix]
name = "AiHubMix"
base_url = "https://aihubmix.example/v1"
wire_api = "responses"
requires_openai_auth = true

[mcp_servers.context7]
command = "npx"
"#;

        let result =
            normalize_codex_live_config_model_provider_with_anchors(target, Some(current)).unwrap();
        let parsed: toml::Value = toml::from_str(&result).unwrap();

        assert_eq!(
            parsed.get("model_provider").and_then(|v| v.as_str()),
            Some("rightcode")
        );

        let model_providers = parsed
            .get("model_providers")
            .and_then(|v| v.as_table())
            .expect("model_providers should exist");
        assert!(
            model_providers.get("aihubmix").is_none(),
            "source provider id should not remain in live config"
        );

        let stable_provider = model_providers
            .get("rightcode")
            .expect("stable provider table should exist");
        assert_eq!(
            stable_provider.get("base_url").and_then(|v| v.as_str()),
            Some("https://aihubmix.example/v1")
        );
        assert!(
            parsed.get("mcp_servers").is_some(),
            "unrelated config should be preserved"
        );
    }

    #[test]
    fn normalize_live_config_uses_target_custom_provider_when_current_is_reserved() {
        let current = r#"model_provider = "openai""#;
        let target = r#"model_provider = "aihubmix"

[model_providers.aihubmix]
name = "AiHubMix"
base_url = "https://aihubmix.example/v1"
wire_api = "responses"
"#;

        let result =
            normalize_codex_live_config_model_provider_with_anchors(target, Some(current)).unwrap();
        let parsed: toml::Value = toml::from_str(&result).unwrap();

        assert_eq!(
            parsed.get("model_provider").and_then(|v| v.as_str()),
            Some("aihubmix")
        );
        assert!(
            parsed
                .get("model_providers")
                .and_then(|v| v.get("aihubmix"))
                .is_some(),
            "target provider id should be kept when there is no reusable live custom id"
        );
    }

    #[test]
    fn normalize_live_config_leaves_official_empty_config_unchanged() {
        let current = r#"model_provider = "rightcode"

[model_providers.rightcode]
base_url = "https://rightcode.example/v1"
"#;

        let result =
            normalize_codex_live_config_model_provider_with_anchors("", Some(current)).unwrap();

        assert_eq!(result, "");
    }

    #[test]
    fn normalize_live_config_rewrites_matching_profile_model_provider_refs() {
        let current = r#"model_provider = "session_anchor"

[model_providers.session_anchor]
name = "Session Anchor"
base_url = "https://anchor.example/v1"
wire_api = "responses"
"#;
        let target = r#"model_provider = "vendor_alpha"
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
            normalize_codex_live_config_model_provider_with_anchors(target, Some(current)).unwrap();
        let parsed: toml::Value = toml::from_str(&result).unwrap();

        assert_eq!(
            parsed.get("model_provider").and_then(|v| v.as_str()),
            Some("session_anchor")
        );
        assert_eq!(
            parsed
                .get("profiles")
                .and_then(|v| v.get("work"))
                .and_then(|v| v.get("model_provider"))
                .and_then(|v| v.as_str()),
            Some("session_anchor"),
            "profile override matching the rewritten provider should stay valid"
        );
    }

    #[test]
    fn normalize_live_config_keeps_unrelated_profile_model_provider_refs() {
        let current = r#"model_provider = "session_anchor"

[model_providers.session_anchor]
name = "Session Anchor"
base_url = "https://anchor.example/v1"
wire_api = "responses"
"#;
        let target = r#"model_provider = "vendor_alpha"
model = "gpt-5.4"

[model_providers.vendor_alpha]
name = "Vendor Alpha"
base_url = "https://alpha.example/v1"
wire_api = "responses"

[model_providers.local_profile]
name = "Local Profile"
base_url = "http://localhost:11434/v1"
wire_api = "responses"

[profiles.local]
model_provider = "local_profile"
model = "local-model"
"#;

        let result =
            normalize_codex_live_config_model_provider_with_anchors(target, Some(current)).unwrap();
        let parsed: toml::Value = toml::from_str(&result).unwrap();

        assert_eq!(
            parsed
                .get("profiles")
                .and_then(|v| v.get("local"))
                .and_then(|v| v.get("model_provider"))
                .and_then(|v| v.as_str()),
            Some("local_profile"),
            "unrelated profile provider references should be preserved"
        );
        assert!(
            parsed
                .get("model_providers")
                .and_then(|v| v.get("local_profile"))
                .is_some(),
            "unrelated provider tables should also remain available"
        );
    }

    #[test]
    fn normalize_live_config_keeps_stable_provider_across_repeated_switches() {
        let anchor = r#"model_provider = "session_anchor"

[model_providers.session_anchor]
name = "Session Anchor"
base_url = "https://anchor.example/v1"
wire_api = "responses"
"#;
        let first_target = r#"model_provider = "vendor_alpha"

[model_providers.vendor_alpha]
name = "Vendor Alpha"
base_url = "https://alpha.example/v1"
wire_api = "responses"
"#;
        let second_target = r#"model_provider = "vendor_beta"

[model_providers.vendor_beta]
name = "Vendor Beta"
base_url = "https://beta.example/v1"
wire_api = "responses"
"#;

        let first =
            normalize_codex_live_config_model_provider_with_anchors(first_target, Some(anchor))
                .unwrap();
        let second = normalize_codex_live_config_model_provider_with_anchors(
            second_target,
            Some(first.as_str()),
        )
        .unwrap();
        let parsed: toml::Value = toml::from_str(&second).unwrap();

        assert_eq!(
            parsed.get("model_provider").and_then(|v| v.as_str()),
            Some("session_anchor"),
            "stable provider id should not drift across repeated switches"
        );
        assert_eq!(
            parsed
                .get("model_providers")
                .and_then(|v| v.get("session_anchor"))
                .and_then(|v| v.get("base_url"))
                .and_then(|v| v.as_str()),
            Some("https://beta.example/v1")
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
}
