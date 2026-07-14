use anyhow::Context;
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use toml_edit::{DocumentMut, Item, Table, TableLike};

use crate::settings::{RelayContextSelection, RelayProfile, RelayProtocol};

const RELAY_PROVIDER: &str = "custom";
const LEGACY_RELAY_PROVIDERS: &[&str] = &["CodexPlusPlus", "CodexPP"];
const CHAT_UPSTREAM_BASE_URL_KEY: &str = "codex_plus_chat_base_url";
const RESERVED_MODEL_PROVIDER_IDS: &[&str] = &[
    "amazon-bedrock",
    "openai",
    "ollama",
    "lmstudio",
    "oss",
    "ollama-chat",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatGptAuthStatus {
    pub authenticated: bool,
    pub source: String,
    pub account_label: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RelayConfigStatus {
    pub configured: bool,
    pub requires_openai_auth: bool,
    pub has_bearer_token: bool,
    pub config_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RelayStatus {
    pub authenticated: bool,
    pub auth_source: String,
    pub account_label: Option<String>,
    pub config_path: String,
    pub configured: bool,
    pub requires_openai_auth: bool,
    pub has_bearer_token: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RelayApplyResult {
    pub config_path: String,
    pub backup_path: Option<String>,
    pub configured: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelayProfileTestResult {
    pub http_status: u16,
    pub endpoint: String,
    pub response_preview: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexContextEntry {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub summary: String,
    pub toml_body: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexContextEntries {
    pub mcp_servers: Vec<CodexContextEntry>,
    pub skills: Vec<CodexContextEntry>,
    pub plugins: Vec<CodexContextEntry>,
}

pub fn default_codex_home_dir() -> PathBuf {
    crate::codex_home::default_codex_home_dir()
}

pub fn default_relay_status() -> RelayStatus {
    relay_status_from_home(&default_codex_home_dir())
}

pub fn set_codex_goals_feature_in_home(home: &Path, enabled: bool) -> anyhow::Result<()> {
    std::fs::create_dir_all(home)?;
    let config_path = home.join("config.toml");
    let existing = std::fs::read_to_string(&config_path).unwrap_or_default();
    let updated = match parse_toml_document(&existing) {
        Ok(mut doc) => {
            if enabled {
                let features = table_mut_or_insert(&mut doc, "features")?;
                features["goals"] = toml_edit::value(true);
            } else if let Some(features) = table_mut_if_exists(&mut doc, "features") {
                features.remove("goals");
                if features.is_empty() {
                    doc.as_table_mut().remove("features");
                }
            }
            ensure_trailing_newline(doc.to_string())
        }
        Err(_) => set_codex_goals_feature_text_fallback(&existing, enabled),
    };
    crate::settings::atomic_write(&config_path, updated.as_bytes())
}

fn set_codex_goals_feature_text_fallback(existing: &str, enabled: bool) -> String {
    let mut kept = Vec::new();
    let mut skipping_features = false;

    for line in existing.lines() {
        let trimmed = line.trim();
        if trimmed == "[features]" {
            skipping_features = true;
            continue;
        }
        if skipping_features && trimmed.starts_with('[') && trimmed.ends_with(']') {
            skipping_features = false;
        }
        if !skipping_features {
            kept.push(line);
        }
    }

    let mut updated = kept.join("\n").trim_end().to_string();
    if enabled {
        if !updated.is_empty() {
            updated.push_str("\n\n");
        }
        updated.push_str("[features]\ngoals = true");
    }
    ensure_trailing_newline(updated)
}

fn table_mut_or_insert<'a>(doc: &'a mut DocumentMut, key: &str) -> anyhow::Result<&'a mut Table> {
    if !doc.as_table().contains_key(key) {
        doc[key] = toml_edit::table();
    }
    if doc.get(key).and_then(Item::as_table).is_none() {
        doc[key] = toml_edit::table();
    }
    doc.get_mut(key)
        .and_then(Item::as_table_mut)
        .ok_or_else(|| anyhow::anyhow!("{key} 必须是 TOML table"))
}

fn table_mut_if_exists<'a>(doc: &'a mut DocumentMut, key: &str) -> Option<&'a mut Table> {
    doc.get_mut(key).and_then(Item::as_table_mut)
}

pub fn relay_status_from_home(home: &Path) -> RelayStatus {
    let auth = chatgpt_auth_status_from_home(home);
    let config = relay_config_status_from_home(home);
    RelayStatus {
        authenticated: auth.authenticated,
        auth_source: auth.source,
        account_label: auth.account_label,
        config_path: config.config_path,
        configured: config.configured,
        requires_openai_auth: config.requires_openai_auth,
        has_bearer_token: config.has_bearer_token,
    }
}

pub fn chatgpt_auth_status_from_home(home: &Path) -> ChatGptAuthStatus {
    let auth_path = home.join("auth.json");
    if let Some(account_label) = auth_json_chatgpt_account_label(&auth_path) {
        return ChatGptAuthStatus {
            authenticated: true,
            source: auth_path.to_string_lossy().to_string(),
            account_label,
            message: "已通过 auth.json 和 config.toml 检测到 ChatGPT 登录。".to_string(),
        };
    }

    ChatGptAuthStatus {
        authenticated: false,
        source: String::new(),
        account_label: None,
        message: "未检测到 ChatGPT 登录账号。".to_string(),
    }
}

pub fn relay_config_status_from_home(home: &Path) -> RelayConfigStatus {
    let config_path = home.join("config.toml");
    let contents = std::fs::read_to_string(&config_path).unwrap_or_default();
    let auth_contents = std::fs::read_to_string(home.join("auth.json")).unwrap_or_default();
    let root_provider = root_key_string(&contents, "model_provider");
    let provider = root_provider
        .as_ref()
        .and_then(|provider| table_values(&contents, &format!("model_providers.{provider}")));
    let requires_openai_auth = provider
        .as_ref()
        .and_then(|values| values.get("requires_openai_auth"))
        .map(|value| value.trim() == "true")
        .unwrap_or(false);
    let has_bearer_token = provider
        .as_ref()
        .and_then(|values| values.get("experimental_bearer_token"))
        .map(|value| unquote_toml_string(value).trim().to_string())
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false);
    let has_base_url = provider
        .as_ref()
        .and_then(|values| values.get("base_url"))
        .map(|value| !unquote_toml_string(value).trim().is_empty())
        .unwrap_or(false);
    RelayConfigStatus {
        configured: root_provider.is_some()
            && requires_openai_auth
            && (has_bearer_token || codex_auth_api_key(&auth_contents).is_some())
            && has_base_url,
        requires_openai_auth,
        has_bearer_token,
        config_path: config_path.to_string_lossy().to_string(),
    }
}

pub fn apply_relay_config_to_home(
    home: &Path,
    base_url: &str,
    bearer_token: &str,
) -> anyhow::Result<RelayApplyResult> {
    apply_relay_config_to_home_with_protocol(
        home,
        base_url,
        bearer_token,
        RelayProtocol::Responses,
        crate::protocol_proxy::DEFAULT_PROTOCOL_PROXY_PORT,
    )
}

pub fn apply_relay_config_to_home_with_protocol(
    home: &Path,
    base_url: &str,
    bearer_token: &str,
    protocol: RelayProtocol,
    proxy_port: u16,
) -> anyhow::Result<RelayApplyResult> {
    let base_url = base_url.trim();
    if base_url.is_empty() {
        anyhow::bail!("中转 Base URL 不能为空");
    }
    let bearer_token = bearer_token.trim();
    if bearer_token.is_empty() {
        anyhow::bail!("中转 Key 不能为空");
    }
    let codex_base_url = codex_base_url_for_protocol(base_url, protocol, proxy_port);
    let updated = upsert_model_provider_config("", &codex_base_url, bearer_token)?;
    let auth_contents = serde_json::to_string_pretty(&json!({
        "OPENAI_API_KEY": bearer_token
    }))?;
    let backup_path =
        write_codex_live_atomic(home, Some(&updated), Some(auth_contents.as_bytes()), false)?;
    let status = relay_config_status_from_home(home);
    Ok(RelayApplyResult {
        config_path: status.config_path,
        backup_path,
        configured: status.configured,
    })
}

pub fn apply_pure_api_config_to_home(
    home: &Path,
    base_url: &str,
    bearer_token: &str,
) -> anyhow::Result<RelayApplyResult> {
    apply_pure_api_config_to_home_with_protocol(
        home,
        base_url,
        bearer_token,
        RelayProtocol::Responses,
        crate::protocol_proxy::DEFAULT_PROTOCOL_PROXY_PORT,
    )
}

pub fn apply_relay_files_to_home(
    home: &Path,
    config_contents: &str,
    auth_contents: &str,
) -> anyhow::Result<RelayApplyResult> {
    apply_relay_files_to_home_with_computer_use_guard(home, config_contents, auth_contents, false)
}

pub fn apply_relay_files_to_home_with_computer_use_guard(
    home: &Path,
    config_contents: &str,
    auth_contents: &str,
    preserve_computer_use_guard: bool,
) -> anyhow::Result<RelayApplyResult> {
    if config_contents.trim().is_empty() {
        anyhow::bail!("config.toml 内容不能为空");
    }
    std::fs::create_dir_all(home)?;

    let backup_path = write_codex_live_atomic(
        home,
        Some(config_contents),
        Some(auth_contents.as_bytes()),
        preserve_computer_use_guard,
    )?;

    let status = relay_config_status_from_home(home);
    Ok(RelayApplyResult {
        config_path: status.config_path,
        backup_path,
        configured: status.configured,
    })
}

pub fn apply_relay_files_to_home_with_common(
    home: &Path,
    config_contents: &str,
    auth_contents: &str,
    common_config_contents: &str,
) -> anyhow::Result<RelayApplyResult> {
    let config_contents = merge_common_config_into_config(config_contents, common_config_contents)?;
    apply_relay_files_to_home(home, &config_contents, auth_contents)
}

pub fn apply_relay_files_to_home_with_context(
    home: &Path,
    config_contents: &str,
    auth_contents: &str,
    common_config_contents: &str,
    selection: &RelayContextSelection,
    context_window: &str,
    auto_compact_limit: &str,
) -> anyhow::Result<RelayApplyResult> {
    let selected_common = filter_common_config_for_selection(common_config_contents, selection)?;
    let config_with_common = merge_common_config_into_config(config_contents, &selected_common)?;
    let config_with_common =
        preserve_unmanaged_live_context_entries(home, &config_with_common, common_config_contents)?;
    let config_with_limits =
        apply_context_limits_to_config(&config_with_common, context_window, auto_compact_limit)?;
    apply_relay_files_to_home(home, &config_with_limits, auth_contents)
}

pub fn apply_relay_profile_files_to_home_with_context(
    home: &Path,
    profile: &RelayProfile,
    common_config_contents: &str,
) -> anyhow::Result<RelayApplyResult> {
    let selected_common = if profile.use_common_config {
        filter_common_config_for_profile(common_config_contents, profile)?
    } else {
        String::new()
    };
    let profile_config = complete_relay_profile_config(profile)?;
    let config_with_common = merge_common_config_into_config(&profile_config, &selected_common)?;
    let config_with_common =
        preserve_unmanaged_live_context_entries(home, &config_with_common, common_config_contents)?;
    let config_with_limits = apply_context_limits_to_config(
        &config_with_common,
        &profile.context_window,
        &profile.auto_compact_limit,
    )?;
    let config_with_catalog = apply_model_catalog_to_config(home, profile, &config_with_limits)?;
    apply_relay_files_to_home(home, &config_with_catalog, &profile.auth_contents)
}

pub fn apply_relay_profile_to_home_with_switch_rules(
    home: &Path,
    profile: &RelayProfile,
    common_config_contents: &str,
) -> anyhow::Result<RelayApplyResult> {
    apply_relay_profile_to_home_with_switch_rules_and_computer_use_guard(
        home,
        profile,
        common_config_contents,
        false,
    )
}

pub fn apply_relay_profile_to_home_with_switch_rules_and_computer_use_guard(
    home: &Path,
    profile: &RelayProfile,
    common_config_contents: &str,
    preserve_computer_use_guard: bool,
) -> anyhow::Result<RelayApplyResult> {
    let selected_common = if profile.use_common_config {
        filter_common_config_for_profile(common_config_contents, profile)?
    } else {
        String::new()
    };
    let profile_config = complete_relay_profile_config(profile)?;
    let config_with_common = merge_common_config_into_config(&profile_config, &selected_common)?;
    let config_with_common =
        preserve_unmanaged_live_context_entries(home, &config_with_common, common_config_contents)?;
    let config_with_limits = apply_context_limits_to_config(
        &config_with_common,
        &profile.context_window,
        &profile.auto_compact_limit,
    )?;
    let config_with_catalog = apply_model_catalog_to_config(home, profile, &config_with_limits)?;

    if profile.relay_mode == crate::settings::RelayMode::PureApi {
        apply_relay_files_to_home_with_computer_use_guard(
            home,
            &config_with_catalog,
            &profile.auth_contents,
            preserve_computer_use_guard,
        )
    } else {
        let auth_contents = official_profile_auth_for_switch(home, &profile.auth_contents)?;
        apply_relay_files_to_home_with_computer_use_guard(
            home,
            &config_with_catalog,
            &auth_contents,
            preserve_computer_use_guard,
        )
    }
}

pub fn apply_relay_profile_config_to_home_with_context(
    home: &Path,
    profile: &RelayProfile,
    common_config_contents: &str,
) -> anyhow::Result<RelayApplyResult> {
    let selected_common = if profile.use_common_config {
        filter_common_config_for_selection(common_config_contents, &profile.context_selection)?
    } else {
        String::new()
    };
    let profile_config = complete_relay_profile_config(profile)?;
    let config_with_common = merge_common_config_into_config(&profile_config, &selected_common)?;
    let config_with_limits = apply_context_limits_to_config(
        &config_with_common,
        &profile.context_window,
        &profile.auto_compact_limit,
    )?;
    let config_with_catalog = apply_model_catalog_to_config(home, profile, &config_with_limits)?;
    apply_relay_config_file_to_home(home, &config_with_catalog)
}

pub fn apply_relay_config_file_to_home(
    home: &Path,
    config_contents: &str,
) -> anyhow::Result<RelayApplyResult> {
    let config_contents = config_contents
        .strip_prefix('\u{feff}')
        .unwrap_or(config_contents);
    if config_contents.trim().is_empty() {
        anyhow::bail!("config.toml 内容不能为空");
    }
    std::fs::create_dir_all(home)?;

    let backup_path = write_codex_live_atomic(home, Some(config_contents), None, false)?;

    let status = relay_config_status_from_home(home);
    Ok(RelayApplyResult {
        config_path: status.config_path,
        backup_path,
        configured: status.configured,
    })
}

pub fn apply_pure_api_config_to_home_with_protocol(
    home: &Path,
    base_url: &str,
    bearer_token: &str,
    protocol: RelayProtocol,
    proxy_port: u16,
) -> anyhow::Result<RelayApplyResult> {
    let base_url = base_url.trim();
    if base_url.is_empty() {
        anyhow::bail!("中转 Base URL 不能为空");
    }
    let bearer_token = bearer_token.trim();
    if bearer_token.is_empty() {
        anyhow::bail!("中转 Key 不能为空");
    }
    let codex_base_url = codex_base_url_for_protocol(base_url, protocol, proxy_port);
    let updated = upsert_model_provider_config("", &codex_base_url, bearer_token)?;
    let auth_contents = serde_json::to_string_pretty(&json!({
        "OPENAI_API_KEY": bearer_token
    }))?;
    let backup_path =
        write_codex_live_atomic(home, Some(&updated), Some(auth_contents.as_bytes()), false)?;
    let status = relay_config_status_from_home(home);
    Ok(RelayApplyResult {
        config_path: status.config_path,
        backup_path,
        configured: status.configured,
    })
}

pub async fn test_relay_profile(
    profile: &RelayProfile,
    model: &str,
) -> anyhow::Result<RelayProfileTestResult> {
    let base_url = relay_profile_base_url(profile);
    let base_url = base_url.trim().trim_end_matches('/');
    if base_url.is_empty() {
        anyhow::bail!("Base URL 不能为空");
    }
    let api_key = relay_profile_api_key(profile);
    let api_key = api_key.trim();
    if api_key.is_empty() {
        anyhow::bail!("API Key 不能为空");
    }

    let client = crate::http_client::proxied_client("CodexPlusPlus/RelayTest")?;
    let endpoint = match profile.protocol {
        RelayProtocol::Responses => format!("{base_url}/responses"),
        RelayProtocol::ChatCompletions => format!("{base_url}/chat/completions"),
    };
    let test_model = model.trim();
    if test_model.is_empty() {
        anyhow::bail!("测试模型不能为空");
    }

    let payload = relay_profile_test_payload(profile.protocol, test_model);
    let response = client
        .post(&endpoint)
        .bearer_auth(api_key)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .json(&payload)
        .send()
        .await?;
    let http_status = response.status().as_u16();

    // 如果 404 且 base_url 末尾没有 /v1，尝试自动补 /v1 后再发一次。
    // 许多上游（中转站、自建代理）暴露的路径以 /v1/ 开头，
    // 用户容易遗漏这个前缀，导致 /responses 或 /chat/completions 404。
    if http_status == 404 && !base_url.ends_with("/v1") {
        let v1_url = format!("{base_url}/v1");
        let v1_endpoint = match profile.protocol {
            RelayProtocol::Responses => format!("{v1_url}/responses"),
            RelayProtocol::ChatCompletions => format!("{v1_url}/chat/completions"),
        };
        let v1_response = client
            .post(&v1_endpoint)
            .bearer_auth(api_key)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .json(&payload)
            .send()
            .await?;
        let v1_status = v1_response.status().as_u16();
        if v1_status < 400 {
            let response_text = v1_response.text().await.unwrap_or_default();
            return Ok(RelayProfileTestResult {
                http_status: v1_status,
                endpoint: v1_endpoint,
                response_preview: format!(
                    "（Base URL 建议加上 /v1 前缀）{}",
                    response_text.chars().take(280).collect::<String>()
                ),
            });
        }
    }

    let response_text = response.text().await.unwrap_or_default();
    Ok(RelayProfileTestResult {
        http_status,
        endpoint,
        response_preview: response_text.chars().take(320).collect(),
    })
}

fn relay_profile_test_payload(protocol: RelayProtocol, model: &str) -> Value {
    match protocol {
        RelayProtocol::Responses => serde_json::json!({
            "model": model,
            "input": "hi",
            "max_output_tokens": 16
        }),
        RelayProtocol::ChatCompletions => serde_json::json!({
            "model": model,
            "messages": [
                { "role": "user", "content": "hi" }
            ],
            "max_tokens": 16
        }),
    }
}

fn codex_base_url_for_protocol(base_url: &str, protocol: RelayProtocol, proxy_port: u16) -> String {
    match protocol {
        RelayProtocol::Responses => base_url.to_string(),
        RelayProtocol::ChatCompletions => {
            crate::protocol_proxy::local_responses_proxy_base_url(proxy_port)
        }
    }
}

pub fn clear_relay_config_to_home(home: &Path) -> anyhow::Result<RelayApplyResult> {
    clear_relay_config_to_home_with_auth(home, None)
}

pub fn clear_relay_config_to_home_with_auth(
    home: &Path,
    auth_contents: Option<&str>,
) -> anyhow::Result<RelayApplyResult> {
    clear_relay_config_to_home_with_auth_and_computer_use_guard(home, auth_contents, false)
}

pub fn clear_relay_config_to_home_with_auth_and_computer_use_guard(
    home: &Path,
    auth_contents: Option<&str>,
    preserve_computer_use_guard: bool,
) -> anyhow::Result<RelayApplyResult> {
    std::fs::create_dir_all(home)?;
    let auth_bytes = match auth_contents {
        Some(contents) if !contents.trim().is_empty() => Some(contents.as_bytes().to_vec()),
        _ => pure_api_auth_json_removed(home)?,
    };
    let config_path = home.join("config.toml");
    let existing = std::fs::read_to_string(&config_path).unwrap_or_default();
    let mut without_tables = remove_table(&existing, &format!("model_providers.{RELAY_PROVIDER}"));
    for legacy_provider in LEGACY_RELAY_PROVIDERS {
        without_tables = remove_table(
            &without_tables,
            &format!("model_providers.{legacy_provider}"),
        );
    }
    let mut updated = without_tables;
    for key in [
        "OPENAI_API_KEY",
        "model_provider",
        "model_catalog_json",
        "base_url",
    ] {
        updated = remove_root_key(&updated, key);
    }
    let backup_path = write_codex_live_atomic(
        home,
        Some(&updated),
        auth_bytes.as_deref(),
        preserve_computer_use_guard,
    )?;
    let status = relay_config_status_from_home(home);
    Ok(RelayApplyResult {
        config_path: status.config_path,
        backup_path,
        configured: status.configured,
    })
}

fn pure_api_auth_json_removed(home: &Path) -> anyhow::Result<Option<Vec<u8>>> {
    let auth_path = home.join("auth.json");
    if !auth_path.exists() {
        return Ok(None);
    }

    let existing = std::fs::read_to_string(&auth_path)?;
    let Ok(mut value) = serde_json::from_str::<Value>(&existing) else {
        return Ok(None);
    };
    let Some(object) = value.as_object_mut() else {
        return Ok(None);
    };
    if object.remove("OPENAI_API_KEY").is_none() {
        return Ok(None);
    }

    Ok(Some(serde_json::to_vec_pretty(&value)?))
}

pub fn backfill_relay_profile_from_home(
    home: &Path,
    profile: &mut RelayProfile,
) -> anyhow::Result<()> {
    profile.config_contents = read_optional_text(&home.join("config.toml"))?;
    profile.auth_contents = read_optional_text(&home.join("auth.json"))?;
    let live_config = profile.config_contents.clone();
    sync_context_limits_from_config(profile, &live_config);
    if profile.model.trim().is_empty() {
        if let Some(model) = root_key_string(&profile.config_contents, "model") {
            profile.model = model;
        }
    }
    Ok(())
}

pub fn backfill_relay_profile_from_home_with_common(
    home: &Path,
    profile: &mut RelayProfile,
    common_config_contents: &mut String,
) -> anyhow::Result<()> {
    let live_config = read_optional_text(&home.join("config.toml"))?;
    let template_config = profile.config_contents.clone();
    let template_auth = profile.auth_contents.clone();
    profile.config_contents = if profile.use_common_config {
        strip_common_config_from_config(&live_config, common_config_contents)?
    } else {
        ensure_trailing_newline(live_config.clone())
    };
    profile.config_contents =
        restore_profile_provider_id_for_backfill(&profile.config_contents, &template_config)?;
    profile.auth_contents = read_optional_text(&home.join("auth.json"))?;
    restore_profile_auth_from_live_config(profile, &template_auth)?;
    sync_profile_mode_from_backfilled_live(profile);
    sync_context_limits_from_config(profile, &live_config);
    if profile.model.trim().is_empty() {
        if let Some(model) = root_key_string(&live_config, "model") {
            profile.model = model;
        }
    }
    Ok(())
}

pub fn extract_common_config_from_config(config_text: &str) -> anyhow::Result<String> {
    let mut doc = parse_toml_document(config_text)?;
    for key in [
        "model",
        "model_provider",
        "base_url",
        "model_catalog_json",
        CHAT_UPSTREAM_BASE_URL_KEY,
    ] {
        doc.as_table_mut().remove(key);
    }
    doc.as_table_mut().remove("model_providers");
    Ok(normalize_optional_toml(doc))
}

pub fn sanitize_common_config_contents(common_config: &str) -> String {
    match parse_toml_document(common_config) {
        Ok(mut doc) => {
            remove_provider_specific_common_keys(doc.as_table_mut());
            normalize_optional_toml(doc)
        }
        Err(_) => sanitize_common_config_text_fallback(common_config),
    }
}

pub fn strip_common_config_from_config(
    config_text: &str,
    common_config_contents: &str,
) -> anyhow::Result<String> {
    let trimmed = common_config_contents.trim();
    if trimmed.is_empty() {
        return Ok(normalize_duplicate_toml_text(config_text));
    }

    match (
        parse_toml_document(config_text),
        parse_toml_document(trimmed),
    ) {
        (Ok(mut target_doc), Ok(source_doc)) => {
            remove_toml_table_like(target_doc.as_table_mut(), source_doc.as_table());
            Ok(normalize_optional_toml(target_doc))
        }
        _ => Ok(strip_common_config_text_fallback(config_text, trimmed)),
    }
}

pub fn merge_common_config_into_config(
    config_text: &str,
    common_config_contents: &str,
) -> anyhow::Result<String> {
    let sanitized_common = sanitize_common_config_contents(common_config_contents);
    let trimmed = sanitized_common.trim();
    if trimmed.is_empty() {
        return Ok(ensure_trailing_newline(config_text.to_string()));
    }

    let mut target_doc = parse_toml_document(config_text)?;
    let source_doc = parse_toml_document(trimmed)?;
    merge_toml_table_like(target_doc.as_table_mut(), source_doc.as_table());
    Ok(normalize_optional_toml(target_doc))
}

pub fn list_context_entries_from_common_config(
    common_config: &str,
) -> anyhow::Result<CodexContextEntries> {
    let normalized = normalize_duplicate_toml_text(common_config);
    let doc = parse_toml_document(&normalized)?;
    Ok(CodexContextEntries {
        mcp_servers: list_context_entries_for_table(&doc, "mcp_servers"),
        skills: list_context_entries_for_table(&doc, "skills"),
        plugins: list_context_entries_for_table(&doc, "plugins"),
    })
}

pub fn upsert_context_entry_in_common_config(
    common_config: &str,
    kind: &str,
    id: &str,
    toml_body: &str,
) -> anyhow::Result<String> {
    let id = id.trim();
    if id.is_empty() {
        anyhow::bail!("上下文 id 不能为空");
    }
    let table_name = context_table_name(kind)?;
    let body_doc = parse_toml_document(toml_body)?;
    let normalized = normalize_duplicate_toml_text(common_config);
    let mut doc = parse_toml_document(&normalized)?;
    if !doc.as_table().contains_key(table_name) {
        doc[table_name] = toml_edit::table();
    }
    if doc[table_name].as_table().is_none() {
        anyhow::bail!("{table_name} 必须是 TOML 表");
    }
    doc[table_name][id] = Item::Table(body_doc.as_table().clone());
    Ok(normalize_optional_toml(doc))
}

pub fn delete_context_entry_from_common_config(
    common_config: &str,
    kind: &str,
    id: &str,
) -> anyhow::Result<String> {
    let table_name = context_table_name(kind)?;
    let normalized = normalize_duplicate_toml_text(common_config);
    let mut doc = parse_toml_document(&normalized)?;
    if let Some(table) = doc[table_name].as_table_mut() {
        table.remove(id.trim());
        if table.is_empty() {
            doc.as_table_mut().remove(table_name);
        }
    }
    Ok(normalize_optional_toml(doc))
}

pub fn filter_common_config_for_selection(
    common_config: &str,
    selection: &RelayContextSelection,
) -> anyhow::Result<String> {
    let sanitized_common = sanitize_common_config_contents(common_config);
    let mut filtered = parse_toml_document(&sanitized_common)?;
    filter_context_tables_for_selection(filtered.as_table_mut(), selection);
    remove_disabled_context_tables(filtered.as_table_mut());
    Ok(normalize_optional_toml(filtered))
}

fn filter_common_config_for_profile(
    common_config: &str,
    profile: &RelayProfile,
) -> anyhow::Result<String> {
    if profile.context_selection_initialized {
        filter_common_config_for_selection(common_config, &profile.context_selection)
    } else {
        let sanitized_common = sanitize_common_config_contents(common_config);
        let mut filtered = parse_toml_document(&sanitized_common)?;
        remove_disabled_context_tables(filtered.as_table_mut());
        Ok(normalize_optional_toml(filtered))
    }
}

pub fn sync_live_config_context_entries(
    live_config: &str,
    context_config: &str,
) -> anyhow::Result<String> {
    let normalized_live = normalize_duplicate_toml_text(live_config);
    let normalized_context = normalize_duplicate_toml_text(context_config);
    let mut live_doc = parse_toml_document(&normalized_live)?;
    if normalized_context.trim().is_empty() {
        return Ok(normalize_optional_toml(live_doc));
    }
    let managed_doc = parse_toml_document(&normalized_context)?;
    remove_managed_context_entries(live_doc.as_table_mut(), managed_doc.as_table());
    let mut context_doc = managed_doc;
    remove_disabled_context_tables(context_doc.as_table_mut());
    merge_managed_context_tables(live_doc.as_table_mut(), context_doc.as_table());
    Ok(normalize_optional_toml(live_doc))
}

fn preserve_unmanaged_live_context_entries(
    home: &Path,
    config_text: &str,
    managed_context_config: &str,
) -> anyhow::Result<String> {
    let live_config = read_optional_text(&home.join("config.toml"))?;
    if live_config.trim().is_empty() {
        return Ok(ensure_trailing_newline(config_text.to_string()));
    }
    let mut target_doc = parse_toml_document(config_text)?;
    let live_doc = parse_toml_document(&live_config)?;
    let managed_doc =
        parse_toml_document(&sanitize_common_config_contents(managed_context_config))?;
    preserve_unmanaged_context_tables(
        target_doc.as_table_mut(),
        live_doc.as_table(),
        managed_doc.as_table(),
    );
    Ok(normalize_optional_toml(target_doc))
}

fn filter_context_tables_for_selection(
    table: &mut toml_edit::Table,
    selection: &RelayContextSelection,
) {
    filter_context_table_for_ids(table, "mcp_servers", &selection.mcp_servers);
    filter_context_table_for_ids(table, "skills", &selection.skills);
    filter_context_table_for_ids(table, "plugins", &selection.plugins);
}

fn filter_context_table_for_ids(
    table: &mut toml_edit::Table,
    table_name: &str,
    selected_ids: &[String],
) {
    let Some(item) = table.get_mut(table_name) else {
        return;
    };
    let Some(context_table) = item.as_table_mut() else {
        return;
    };
    let selected = selected_ids
        .iter()
        .map(|id| id.trim())
        .filter(|id| !id.is_empty())
        .collect::<HashSet<_>>();
    let remove_ids = context_table
        .iter()
        .filter_map(|(id, _)| (!selected.contains(id)).then_some(id.to_string()))
        .collect::<Vec<_>>();
    for id in remove_ids {
        context_table.remove(&id);
    }
}

fn merge_managed_context_tables(target: &mut toml_edit::Table, managed: &toml_edit::Table) {
    for table_name in ["mcp_servers", "skills", "plugins"] {
        merge_managed_context_table(target, managed, table_name);
    }
}

fn merge_managed_context_table(
    target: &mut toml_edit::Table,
    managed: &toml_edit::Table,
    table_name: &str,
) {
    let Some(managed_item) = managed.get(table_name) else {
        return;
    };
    let Some(managed_table) = managed_item.as_table_like() else {
        return;
    };
    if target.get(table_name).is_none() {
        target[table_name] = toml_edit::table();
    }
    let Some(target_table) = target.get_mut(table_name).and_then(Item::as_table_like_mut) else {
        target[table_name] = managed_item.clone();
        return;
    };
    for (id, item) in managed_table.iter() {
        target_table.insert(id, item.clone());
    }
}

fn remove_managed_context_entries(target: &mut toml_edit::Table, managed: &toml_edit::Table) {
    for table_name in ["mcp_servers", "skills", "plugins"] {
        remove_managed_context_entry_table(target, managed, table_name);
    }
}

fn remove_managed_context_entry_table(
    target: &mut toml_edit::Table,
    managed: &toml_edit::Table,
    table_name: &str,
) {
    let Some(managed_item) = managed.get(table_name) else {
        return;
    };
    let Some(managed_table) = managed_item.as_table_like() else {
        return;
    };
    let Some(target_table) = target.get_mut(table_name).and_then(Item::as_table_like_mut) else {
        return;
    };
    for (id, _) in managed_table.iter() {
        target_table.remove(id);
    }
}

fn preserve_unmanaged_context_tables(
    target: &mut toml_edit::Table,
    live: &toml_edit::Table,
    managed: &toml_edit::Table,
) {
    for table_name in ["mcp_servers", "skills", "plugins"] {
        preserve_unmanaged_context_table(target, live, managed, table_name);
    }
}

fn preserve_unmanaged_context_table(
    target: &mut toml_edit::Table,
    live: &toml_edit::Table,
    managed: &toml_edit::Table,
    table_name: &str,
) {
    let Some(live_item) = live.get(table_name) else {
        return;
    };
    let Some(live_table) = live_item.as_table_like() else {
        return;
    };
    if target.get(table_name).is_none() {
        target[table_name] = toml_edit::table();
    }
    let Some(target_table) = target.get_mut(table_name).and_then(Item::as_table_like_mut) else {
        return;
    };
    let managed_ids = managed
        .get(table_name)
        .and_then(Item::as_table_like)
        .map(|table| {
            table
                .iter()
                .map(|(id, _)| id.to_string())
                .collect::<HashSet<_>>()
        })
        .unwrap_or_default();
    for (id, item) in live_table.iter() {
        if !managed_ids.contains(id) && target_table.get(id).is_none() {
            target_table.insert(id, item.clone());
        }
    }
}

fn remove_disabled_context_tables(table: &mut toml_edit::Table) {
    for table_name in ["mcp_servers", "skills", "plugins"] {
        let Some(item) = table.get_mut(table_name) else {
            continue;
        };
        let Some(context_table) = item.as_table_mut() else {
            continue;
        };
        let disabled_ids: Vec<String> = context_table
            .iter()
            .filter_map(|(id, item)| {
                let enabled = item.as_table().map(context_entry_enabled).unwrap_or(true);
                (!enabled).then_some(id.to_string())
            })
            .collect();
        for id in disabled_ids {
            context_table.remove(&id);
        }
    }
}

fn write_codex_live_atomic(
    home: &Path,
    config_text: Option<&str>,
    auth_bytes: Option<&[u8]>,
    preserve_computer_use_guard: bool,
) -> anyhow::Result<Option<String>> {
    std::fs::create_dir_all(home)?;
    let config_path = home.join("config.toml");
    let auth_path = home.join("auth.json");
    #[cfg(windows)]
    let guarded_config_text = match config_text {
        Some(config_text) if preserve_computer_use_guard => {
            let notify_exe = crate::computer_use_guard::find_computer_use_notify_exe(home);
            let marketplace_path =
                crate::computer_use_guard::ensure_openai_bundled_marketplace(home)?;
            let guarded = if let Some(marketplace_path) = marketplace_path.as_deref() {
                crate::computer_use_guard::guard_config_text_with_marketplace(
                    config_text,
                    notify_exe.as_deref(),
                    Some(marketplace_path),
                )?
            } else {
                crate::computer_use_guard::guard_config_text(config_text, notify_exe.as_deref())?
            };
            Some(guarded)
        }
        Some(config_text) => Some(normalize_config_text_for_write(config_text)),
        None => None,
    };
    #[cfg(windows)]
    let config_text = guarded_config_text.as_deref();

    let config_text = match config_text {
        Some(config_text) => Some(preserve_live_marketplace_configs(home, config_text)?),
        None => None,
    };
    let config_text = config_text.as_deref();

    let config_text = match config_text {
        Some(config_text) => Some(
            crate::plugin_marketplace::preserve_openai_curated_remote_marketplace_config(
                home,
                config_text,
            )?,
        ),
        None => None,
    };
    let config_text = config_text.as_deref();

    if let Some(config_text) = config_text {
        validate_toml_config(config_text, &config_path)?;
    }
    if let Some(auth_bytes) = auth_bytes {
        validate_auth_json(auth_bytes, &auth_path)?;
    }

    let old_config = read_optional_bytes(&config_path)?;
    let old_auth = read_optional_bytes(&auth_path)?;
    let backup_path = create_live_backup(home, old_config.as_deref(), old_auth.as_deref())?;
    let mut auth_written = false;

    if let Some(auth_bytes) = auth_bytes {
        if let Err(error) = crate::settings::atomic_write(&auth_path, auth_bytes) {
            return Err(error.context("写入 auth.json 失败"));
        }
        auth_written = true;
    }

    if let Some(config_text) = config_text {
        if let Err(error) = crate::settings::atomic_write(&config_path, config_text.as_bytes()) {
            if auth_written {
                let _ = restore_optional_file(&auth_path, old_auth.as_deref());
            }
            let _ = restore_optional_file(&config_path, old_config.as_deref());
            return Err(error.context("写入 config.toml 失败"));
        }
    }

    Ok(backup_path)
}

fn preserve_live_marketplace_configs(home: &Path, config_text: &str) -> anyhow::Result<String> {
    let live_config = read_optional_text(&home.join("config.toml"))?;
    if live_config.trim().is_empty() {
        return Ok(config_text.to_string());
    }

    let mut target = parse_toml_document(config_text)?;
    let live = parse_toml_document(&live_config)?;
    let Some(live_marketplaces) = live.get("marketplaces").and_then(Item::as_table_like) else {
        return Ok(ensure_trailing_newline(target.to_string()));
    };
    if live_marketplaces.is_empty() {
        return Ok(ensure_trailing_newline(target.to_string()));
    }

    if target.get("marketplaces").is_none() {
        target["marketplaces"] = toml_edit::table();
    }
    if target
        .get("marketplaces")
        .and_then(Item::as_table_like)
        .is_none()
    {
        target["marketplaces"] = toml_edit::table();
    }
    let Some(target_marketplaces) = target
        .get_mut("marketplaces")
        .and_then(Item::as_table_like_mut)
    else {
        return Ok(ensure_trailing_newline(target.to_string()));
    };

    for (name, marketplace) in live_marketplaces.iter() {
        if target_marketplaces.get(name).is_none() {
            target_marketplaces.insert(name, marketplace.clone());
        }
    }

    Ok(ensure_trailing_newline(target.to_string()))
}

fn active_provider_id(doc: &DocumentMut) -> Option<String> {
    doc.get("model_provider")
        .and_then(Item::as_str)
        .map(str::trim)
        .filter(|provider| !provider.is_empty())
        .map(ToString::to_string)
}

fn active_or_default_provider_id(doc: &DocumentMut) -> String {
    active_provider_id(doc)
        .filter(|provider| {
            is_custom_provider_id(provider) && !LEGACY_RELAY_PROVIDERS.contains(&provider.as_str())
        })
        .unwrap_or_else(|| RELAY_PROVIDER.to_string())
}

fn is_custom_provider_id(provider: &str) -> bool {
    !provider.is_empty() && !RESERVED_MODEL_PROVIDER_IDS.contains(&provider)
}

fn provider_table_exists(doc: &DocumentMut, provider_id: &str) -> bool {
    doc.get("model_providers")
        .and_then(Item::as_table)
        .and_then(|table| table.get(provider_id))
        .is_some()
}

fn parse_toml_document(contents: &str) -> anyhow::Result<DocumentMut> {
    let contents = contents.trim_start_matches('\u{feff}');
    if contents.trim().is_empty() {
        Ok(DocumentMut::new())
    } else {
        contents
            .parse::<DocumentMut>()
            .map_err(|error| anyhow::anyhow!("config.toml TOML 解析失败：{error}"))
    }
}

fn remove_provider_specific_common_keys(table: &mut dyn TableLike) {
    for key in [
        "model",
        "model_provider",
        "base_url",
        "model_catalog_json",
        CHAT_UPSTREAM_BASE_URL_KEY,
    ] {
        table.remove(key);
    }
    table.remove("model_providers");
}

fn sanitize_common_config_text_fallback(common_config: &str) -> String {
    let mut kept = Vec::new();
    let mut in_root = true;
    let mut skipping_model_providers = false;

    for line in common_config.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            in_root = false;
            skipping_model_providers =
                trimmed == "[model_providers]" || trimmed.starts_with("[model_providers.");
            if skipping_model_providers {
                continue;
            }
        } else if skipping_model_providers {
            continue;
        }

        if in_root {
            if let Some((key, _)) = trimmed.split_once('=') {
                let key = key.trim();
                if matches!(
                    key,
                    "model"
                        | "model_provider"
                        | "base_url"
                        | "model_catalog_json"
                        | CHAT_UPSTREAM_BASE_URL_KEY
                ) {
                    continue;
                }
            }
        }

        kept.push(line);
    }

    normalize_text_toml(kept.join("\n"))
}

fn normalize_text_toml(contents: String) -> String {
    let trimmed = contents.trim();
    if trimmed.is_empty() {
        String::new()
    } else {
        ensure_trailing_newline(trimmed.to_string())
    }
}

pub fn normalize_config_text(contents: &str) -> String {
    normalize_duplicate_toml_text(contents)
}

fn normalize_duplicate_toml_text(contents: &str) -> String {
    let mut seen_root_keys = HashSet::new();
    let mut seen_headers = HashSet::new();
    let mut kept = Vec::new();
    let mut skipping_duplicate_table = false;
    let mut in_root = true;

    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            in_root = false;
            skipping_duplicate_table = !seen_headers.insert(trimmed.to_string());
            if skipping_duplicate_table {
                continue;
            }
            kept.push(line);
            continue;
        }

        if skipping_duplicate_table {
            continue;
        }

        if in_root && !trimmed.is_empty() && !trimmed.starts_with('#') {
            if let Some((key, _)) = trimmed.split_once('=') {
                let key = key.trim();
                if !key.is_empty() && !key.contains('.') && !seen_root_keys.insert(key.to_string())
                {
                    continue;
                }
            }
        }

        kept.push(line);
    }

    normalize_text_toml(kept.join("\n"))
}

fn strip_common_config_text_fallback(config_text: &str, common_config: &str) -> String {
    let normalized = normalize_duplicate_toml_text(config_text);
    let anchors = common_config_anchors(common_config);
    if anchors.root_keys.is_empty() && anchors.table_headers.is_empty() {
        return normalized;
    }

    let mut kept = Vec::new();
    let mut skipping_table = false;

    for line in normalized.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            skipping_table = anchors.table_headers.contains(trimmed);
            if skipping_table {
                continue;
            }
            kept.push(line);
            continue;
        }

        if skipping_table {
            continue;
        }

        if !trimmed.is_empty() && !trimmed.starts_with('#') {
            if let Some((key, _)) = trimmed.split_once('=') {
                if anchors.root_keys.contains(key.trim()) {
                    continue;
                }
            }
        }

        kept.push(line);
    }

    normalize_text_toml(kept.join("\n"))
}

struct CommonConfigAnchors {
    root_keys: HashSet<String>,
    table_headers: HashSet<String>,
}

fn common_config_anchors(common_config: &str) -> CommonConfigAnchors {
    let mut root_keys = HashSet::new();
    let mut table_headers = HashSet::new();
    let mut in_root = true;

    for line in common_config.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            in_root = false;
            table_headers.insert(trimmed.to_string());
            continue;
        }

        if in_root && !trimmed.is_empty() && !trimmed.starts_with('#') {
            if let Some((key, _)) = trimmed.split_once('=') {
                let key = key.trim();
                if !key.is_empty() {
                    root_keys.insert(key.to_string());
                }
            }
        }
    }

    CommonConfigAnchors {
        root_keys,
        table_headers,
    }
}

fn validate_toml_config(config_text: &str, path: &Path) -> anyhow::Result<()> {
    let config_text = config_text.trim_start_matches('\u{feff}');
    if config_text.trim().is_empty() {
        return Ok(());
    }
    config_text
        .parse::<toml::Table>()
        .with_context(|| format!("{} 不是有效 TOML", path.display()))?;
    Ok(())
}

fn normalize_config_text_for_write(config_text: &str) -> String {
    config_text.trim_start_matches('\u{feff}').to_string()
}

fn validate_auth_json(auth_bytes: &[u8], path: &Path) -> anyhow::Result<()> {
    if auth_bytes.iter().all(|byte| byte.is_ascii_whitespace()) {
        return Ok(());
    }
    serde_json::from_slice::<Value>(auth_bytes)
        .with_context(|| format!("{} 不是有效 JSON", path.display()))?;
    Ok(())
}

fn parse_optional_positive_u64(value: &str, label: &str) -> anyhow::Result<Option<u64>> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    let parsed = trimmed
        .parse::<u64>()
        .with_context(|| format!("{label}必须是正整数"))?;
    if parsed == 0 {
        anyhow::bail!("{label}必须大于 0");
    }
    Ok(Some(parsed))
}

fn apply_context_limits_to_config(
    config_text: &str,
    context_window: &str,
    auto_compact_limit: &str,
) -> anyhow::Result<String> {
    let mut doc = parse_toml_document(config_text)?;
    if let Some(value) = parse_optional_positive_u64(context_window, "上下文大小")? {
        doc["model_context_window"] = toml_edit::value(value as i64);
    }
    if let Some(value) = parse_optional_positive_u64(auto_compact_limit, "压缩上下文大小")? {
        doc["model_auto_compact_token_limit"] = toml_edit::value(value as i64);
    }
    Ok(normalize_optional_toml(doc))
}

fn apply_model_catalog_to_config(
    home: &Path,
    profile: &RelayProfile,
    config_text: &str,
) -> anyhow::Result<String> {
    let catalog_relative = format!(
        "model-catalogs/{}.json",
        sanitize_catalog_filename(&profile.id)
    );
    // 用户已手写 model_catalog_json 指针时保留，不覆盖（保 preserves_user_model_catalog_json 测试）
    // 仅当现有指针指向本 profile 自己生成的 catalog 时才重新生成。
    if let Some(existing) = root_key_string(config_text, "model_catalog_json") {
        if existing != catalog_relative {
            return Ok(config_text.to_string());
        }
    }
    let (model_list, model_windows): (String, std::collections::HashMap<String, String>) =
        if profile.model_windows.trim().is_empty() && profile.model_list.contains('[') {
            crate::model_suffix::migrate_model_list_with_suffixes(&profile.model_list)
        } else {
            (
                profile.model_list.clone(),
                serde_json::from_str(&profile.model_windows).unwrap_or_default(),
            )
        };
    let entries =
        crate::model_suffix::collect_catalog_entries(&model_list, &model_windows, &profile.model);
    // 无后缀条目则 no-op，保持现有 per-profile 单值行为（保 does_not_write 测试）
    if !entries.iter().any(|entry| entry.suffix_window.is_some()) {
        return Ok(config_text.to_string());
    }
    let fallback = parse_optional_positive_u64(&profile.context_window, "上下文大小")?;
    let catalog_path = home.join(&catalog_relative);
    if let Some(parent) = catalog_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let catalog_json = crate::model_suffix::build_model_catalog_json(&entries, fallback);
    std::fs::write(&catalog_path, catalog_json)?;
    let mut doc = parse_toml_document(config_text)?;
    doc["model_catalog_json"] = toml_edit::value(catalog_relative);
    Ok(normalize_optional_toml(doc))
}

fn sanitize_catalog_filename(id: &str) -> String {
    id.chars()
        .map(|char| {
            if char.is_ascii_alphanumeric() || char == '-' || char == '_' {
                char
            } else {
                '-'
            }
        })
        .collect()
}

fn sync_context_limits_from_config(profile: &mut RelayProfile, config_text: &str) {
    if let Some(value) = root_positive_int_string(config_text, "model_context_window") {
        profile.context_window = value;
    }
    if let Some(value) = root_positive_int_string(config_text, "model_auto_compact_token_limit") {
        profile.auto_compact_limit = value;
    }
}

fn root_positive_int_string(config_text: &str, key: &str) -> Option<String> {
    if let Ok(doc) = parse_toml_document(config_text) {
        if let Some(value) = doc
            .get(key)
            .and_then(Item::as_value)
            .and_then(toml_edit::Value::as_integer)
            .filter(|value| *value > 0)
        {
            return Some(value.to_string());
        }
    }

    root_key_value(config_text, key)
        .and_then(|value| value.split('#').next())
        .map(str::trim)
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0)
        .map(|value| value.to_string())
}

fn toml_value_is_subset(target: &toml_edit::Value, source: &toml_edit::Value) -> bool {
    match (target, source) {
        (toml_edit::Value::String(target), toml_edit::Value::String(source)) => {
            target.value() == source.value()
        }
        (toml_edit::Value::Integer(target), toml_edit::Value::Integer(source)) => {
            target.value() == source.value()
        }
        (toml_edit::Value::Float(target), toml_edit::Value::Float(source)) => {
            target.value() == source.value()
        }
        (toml_edit::Value::Boolean(target), toml_edit::Value::Boolean(source)) => {
            target.value() == source.value()
        }
        (toml_edit::Value::Datetime(target), toml_edit::Value::Datetime(source)) => {
            target.value() == source.value()
        }
        (toml_edit::Value::Array(target), toml_edit::Value::Array(source)) => {
            toml_array_contains_subset(target, source)
        }
        (toml_edit::Value::InlineTable(target), toml_edit::Value::InlineTable(source)) => {
            source.iter().all(|(key, source_item)| {
                target
                    .get(key)
                    .is_some_and(|target_item| toml_value_is_subset(target_item, source_item))
            })
        }
        _ => false,
    }
}

fn toml_array_contains_subset(target: &toml_edit::Array, source: &toml_edit::Array) -> bool {
    let mut matched = vec![false; target.len()];
    let target_items: Vec<&toml_edit::Value> = target.iter().collect();

    source.iter().all(|source_item| {
        if let Some((index, _)) = target_items
            .iter()
            .enumerate()
            .find(|(index, target_item)| {
                !matched[*index] && toml_value_is_subset(target_item, source_item)
            })
        {
            matched[index] = true;
            true
        } else {
            false
        }
    })
}

fn toml_remove_array_items(target: &mut toml_edit::Array, source: &toml_edit::Array) {
    for source_item in source.iter() {
        let index = {
            let target_items: Vec<&toml_edit::Value> = target.iter().collect();
            target_items
                .iter()
                .enumerate()
                .find(|(_, target_item)| toml_value_is_subset(target_item, source_item))
                .map(|(index, _)| index)
        };

        if let Some(index) = index {
            target.remove(index);
        }
    }
}

fn merge_toml_item(target: &mut Item, source: &Item) {
    if let Some(source_table) = source.as_table_like() {
        if let Some(target_table) = target.as_table_like_mut() {
            merge_toml_table_like(target_table, source_table);
            return;
        }
    }

    *target = source.clone();
}

fn merge_toml_table_like(target: &mut dyn TableLike, source: &dyn TableLike) {
    for (key, source_item) in source.iter() {
        match target.get_mut(key) {
            Some(target_item) => merge_toml_item(target_item, source_item),
            None => {
                target.insert(key, source_item.clone());
            }
        }
    }
}

fn remove_toml_item(target: &mut Item, source: &Item) {
    if let Some(source_table) = source.as_table_like() {
        if let Some(target_table) = target.as_table_like_mut() {
            remove_toml_table_like(target_table, source_table);
            if target_table.is_empty() {
                *target = Item::None;
            }
            return;
        }
    }

    if let Some(source_value) = source.as_value() {
        let mut remove_item = false;

        if let Some(target_value) = target.as_value_mut() {
            match (target_value, source_value) {
                (toml_edit::Value::Array(target_arr), toml_edit::Value::Array(source_arr)) => {
                    toml_remove_array_items(target_arr, source_arr);
                    remove_item = target_arr.is_empty();
                }
                (target_value, source_value)
                    if toml_value_is_subset(target_value, source_value) =>
                {
                    remove_item = true;
                }
                _ => {}
            }
        }

        if remove_item {
            *target = Item::None;
        }
    }
}

fn remove_toml_table_like(target: &mut dyn TableLike, source: &dyn TableLike) {
    let keys: Vec<String> = source.iter().map(|(key, _)| key.to_string()).collect();

    for key in keys {
        let mut remove_key = false;
        if let (Some(target_item), Some(source_item)) = (target.get_mut(&key), source.get(&key)) {
            remove_toml_item(target_item, source_item);
            remove_key = target_item.is_none()
                || target_item
                    .as_table_like()
                    .is_some_and(|table_like| table_like.is_empty());
        }

        if remove_key {
            target.remove(&key);
        }
    }
}

fn normalize_optional_toml(doc: DocumentMut) -> String {
    let contents = doc.to_string();
    if contents.trim().is_empty() {
        String::new()
    } else {
        ensure_trailing_newline(contents)
    }
}

fn list_context_entries_for_table(doc: &DocumentMut, table_name: &str) -> Vec<CodexContextEntry> {
    let Some(table) = doc.get(table_name).and_then(Item::as_table) else {
        return Vec::new();
    };
    table
        .iter()
        .filter_map(|(id, item)| {
            let table = item.as_table()?;
            let body = table_body_to_string(table);
            Some(CodexContextEntry {
                id: id.to_string(),
                kind: context_kind_name(table_name).to_string(),
                title: id.to_string(),
                summary: context_entry_summary(&body),
                toml_body: body,
                enabled: context_entry_enabled(table),
            })
        })
        .collect()
}

fn table_body_to_string(table: &Table) -> String {
    let mut doc = DocumentMut::new();
    merge_toml_table_like(doc.as_table_mut(), table);
    normalize_optional_toml(doc)
}

fn context_table_name(kind: &str) -> anyhow::Result<&'static str> {
    match kind {
        "mcp" | "mcpServer" | "mcpServers" => Ok("mcp_servers"),
        "skill" | "skills" => Ok("skills"),
        "plugin" | "plugins" => Ok("plugins"),
        other => anyhow::bail!("未知上下文类型：{other}"),
    }
}

fn context_kind_name(table: &str) -> &'static str {
    match table {
        "mcp_servers" => "mcp",
        "skills" => "skill",
        "plugins" => "plugin",
        _ => "unknown",
    }
}

fn context_entry_summary(body: &str) -> String {
    body.lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with('#'))
        .unwrap_or("")
        .chars()
        .take(96)
        .collect()
}

fn context_entry_enabled(table: &Table) -> bool {
    if table
        .get("enabled")
        .and_then(|value| value.as_bool())
        .is_some_and(|enabled| !enabled)
    {
        return false;
    }
    if table
        .get("disabled")
        .and_then(|value| value.as_bool())
        .is_some_and(|disabled| disabled)
    {
        return false;
    }
    true
}

fn set_provider_id(doc: &mut DocumentMut, provider_id: &str) {
    doc["model_provider"] = toml_edit::value(provider_id);
}

fn restore_profile_provider_id_for_backfill(
    live_config: &str,
    template_config: &str,
) -> anyhow::Result<String> {
    let Some(template_provider_id) = provider_id_with_table_from_config(template_config)? else {
        return Ok(ensure_trailing_newline(live_config.to_string()));
    };
    if live_config.trim().is_empty() {
        return Ok(ensure_trailing_newline(live_config.to_string()));
    }

    let mut doc = parse_toml_document(live_config)?;
    let Some(live_provider_id) = active_provider_id(&doc) else {
        return Ok(ensure_trailing_newline(doc.to_string()));
    };
    if live_provider_id == template_provider_id {
        return Ok(ensure_trailing_newline(doc.to_string()));
    }
    if live_provider_id != RELAY_PROVIDER || template_provider_id == RELAY_PROVIDER {
        return Ok(ensure_trailing_newline(doc.to_string()));
    }
    if !provider_table_exists(&doc, &live_provider_id) {
        return Ok(ensure_trailing_newline(doc.to_string()));
    }

    rename_provider_table(&mut doc, &live_provider_id, &template_provider_id);
    rewrite_profile_provider_refs(&mut doc, &live_provider_id, &template_provider_id);
    set_provider_id(&mut doc, &template_provider_id);
    Ok(ensure_trailing_newline(doc.to_string()))
}

fn provider_id_with_table_from_config(config_text: &str) -> anyhow::Result<Option<String>> {
    if config_text.trim().is_empty() {
        return Ok(None);
    }
    let doc = parse_toml_document(config_text)?;
    let Some(provider_id) = active_provider_id(&doc) else {
        return Ok(None);
    };
    Ok(provider_table_exists(&doc, &provider_id).then_some(provider_id))
}

fn restore_profile_auth_from_live_config(
    profile: &mut RelayProfile,
    template_auth: &str,
) -> anyhow::Result<()> {
    let Some(token) = experimental_bearer_token_from_config(&profile.config_contents)? else {
        return Ok(());
    };
    profile.api_key = token.clone();

    if profile.relay_mode == crate::settings::RelayMode::Official && profile.official_mix_api_key {
        profile.auth_contents = remove_openai_api_key_from_auth_contents(&profile.auth_contents)?;
        return Ok(());
    }

    if !profile.auth_contents.trim().is_empty() {
        if codex_auth_api_key(&profile.auth_contents).is_none() {
            return Ok(());
        }
        profile.config_contents =
            remove_experimental_bearer_token_from_config(&profile.config_contents)?;
        return Ok(());
    }

    profile.config_contents =
        remove_experimental_bearer_token_from_config(&profile.config_contents)?;

    let mut auth = if template_auth.trim().is_empty() {
        json!({})
    } else {
        serde_json::from_str::<Value>(template_auth).with_context(|| "auth.json JSON 解析失败")?
    };
    if !auth.is_object() {
        auth = json!({});
    }
    if let Some(auth_object) = auth.as_object_mut() {
        auth_object.insert("OPENAI_API_KEY".to_string(), Value::String(token));
    } else {
        anyhow::bail!("auth.json 必须是 JSON 对象");
    }
    profile.auth_contents = serde_json::to_string_pretty(&auth)?;
    Ok(())
}

fn sync_profile_mode_from_backfilled_live(profile: &mut RelayProfile) {
    if profile.relay_mode == crate::settings::RelayMode::Official && !profile.official_mix_api_key {
        return;
    }

    if codex_auth_api_key(&profile.auth_contents)
        .as_deref()
        .is_some_and(|value| !value.trim().is_empty())
    {
        profile.relay_mode = crate::settings::RelayMode::PureApi;
        profile.official_mix_api_key = false;
        return;
    }

    let has_provider_endpoint = provider_string_from_config(&profile.config_contents, "base_url")
        .as_deref()
        .is_some_and(|value| !value.trim().is_empty());
    if has_provider_endpoint || !profile.api_key.trim().is_empty() {
        profile.relay_mode = crate::settings::RelayMode::Official;
        profile.official_mix_api_key = true;
    }
}

fn official_profile_auth_for_switch(home: &Path, auth_contents: &str) -> anyhow::Result<String> {
    let source = if auth_contents.trim().is_empty() {
        read_optional_text(&home.join("auth.json"))?
    } else {
        auth_contents.to_string()
    };
    remove_openai_api_key_from_auth_contents(&source)
}

fn codex_auth_api_key(auth_contents: &str) -> Option<String> {
    let auth: Value = serde_json::from_str(auth_contents).ok()?;
    auth.get("OPENAI_API_KEY")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(ToString::to_string)
}

/// 解析 profile 實際使用的模型：優先取 config.toml 裡的 `model =`，
/// 否則退回 profile.model 欄位。供應商測試用它做回退，避免串到別家供應商的模型名。
pub fn relay_profile_model(profile: &RelayProfile) -> String {
    root_key_string(&profile.config_contents, "model")
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| profile.model.trim().to_string())
}

pub fn relay_profile_base_url(profile: &RelayProfile) -> String {
    if profile.relay_mode == crate::settings::RelayMode::Aggregate {
        return crate::protocol_proxy::local_responses_proxy_base_url(
            crate::protocol_proxy::DEFAULT_PROTOCOL_PROXY_PORT,
        );
    }
    if profile.protocol == RelayProtocol::ChatCompletions {
        if !profile.upstream_base_url.trim().is_empty() {
            return profile.upstream_base_url.trim().to_string();
        }
        if let Some(value) = root_key_string(&profile.config_contents, CHAT_UPSTREAM_BASE_URL_KEY)
            .filter(|value| !value.trim().is_empty())
        {
            return value;
        }
        if !profile.base_url.trim().is_empty() {
            return profile.base_url.trim().to_string();
        }
    }
    let provider_base_url = provider_string_from_config(&profile.config_contents, "base_url")
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_default();
    if profile.protocol == RelayProtocol::ChatCompletions
        && provider_base_url
            == crate::protocol_proxy::local_responses_proxy_base_url(
                crate::protocol_proxy::DEFAULT_PROTOCOL_PROXY_PORT,
            )
    {
        String::new()
    } else if !provider_base_url.is_empty() {
        provider_base_url
    } else {
        profile.base_url.trim().to_string()
    }
}

pub fn relay_profile_api_key(profile: &RelayProfile) -> String {
    if profile.relay_mode == crate::settings::RelayMode::Aggregate {
        return "codex-plus-aggregate".to_string();
    }
    if profile.relay_mode == crate::settings::RelayMode::Official {
        return experimental_bearer_token_from_config(&profile.config_contents)
            .ok()
            .flatten()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| profile.api_key.trim().to_string());
    }
    codex_auth_api_key(&profile.auth_contents)
        .or_else(|| {
            experimental_bearer_token_from_config(&profile.config_contents)
                .ok()
                .flatten()
        })
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| profile.api_key.trim().to_string())
}

fn complete_relay_profile_config(profile: &RelayProfile) -> anyhow::Result<String> {
    let mut doc = parse_toml_document(&profile.config_contents)?;
    let provider_id = active_or_default_provider_id(&doc);
    set_provider_id(&mut doc, &provider_id);

    let mut model = relay_profile_model(profile);
    // 若用户未填写默认模型，但 model_list 有内容，则取第一条作为默认 model，
    // 避免 codex 启动时回退到历史会话中带后缀的模型名。
    if model.trim().is_empty() && !profile.model_list.trim().is_empty() {
        if let Some(first) = profile
            .model_list
            .split(['\r', '\n', ','])
            .map(str::trim)
            .find(|value| !value.is_empty())
        {
            model = crate::model_suffix::parse_model_suffix(first).0;
        }
    }
    // 若用户把后缀语法（如 deepseek-v4-flash[1M]）写在 model 字段，
    // 写入 config.toml 前需剥离后缀；codex 本身不理解后缀，只会按原串匹配 catalog slug。
    let (model, _) = crate::model_suffix::parse_model_suffix(&model);
    if !model.trim().is_empty() {
        doc["model"] = toml_edit::value(model.trim());
    }

    let base_url = relay_profile_base_url(profile);
    let api_key = relay_profile_api_key(profile);
    doc.as_table_mut().remove(CHAT_UPSTREAM_BASE_URL_KEY);
    retain_only_provider_table(&mut doc, &provider_id);
    for legacy_provider in LEGACY_RELAY_PROVIDERS {
        if provider_id != *legacy_provider {
            remove_provider_table(&mut doc, legacy_provider);
        }
    }
    let provider = ensure_provider_table(&mut doc, &provider_id)?;
    if provider
        .get("name")
        .and_then(Item::as_str)
        .map(str::trim)
        .is_none_or(str::is_empty)
    {
        provider["name"] = toml_edit::value(provider_id.as_str());
    }
    if provider
        .get("wire_api")
        .and_then(Item::as_str)
        .map(str::trim)
        .is_none_or(str::is_empty)
    {
        provider["wire_api"] = toml_edit::value("responses");
    }
    if provider
        .get("requires_openai_auth")
        .and_then(Item::as_bool)
        .is_none()
    {
        provider["requires_openai_auth"] = toml_edit::value(true);
    }
    let provider_base_url = codex_base_url_for_protocol(
        base_url.trim(),
        profile.protocol,
        crate::protocol_proxy::DEFAULT_PROTOCOL_PROXY_PORT,
    );
    if !provider_base_url.trim().is_empty() {
        provider["base_url"] = toml_edit::value(provider_base_url.trim());
    }
    if profile.relay_mode == crate::settings::RelayMode::PureApi {
        provider.remove("experimental_bearer_token");
    } else if !api_key.trim().is_empty() {
        provider["experimental_bearer_token"] = toml_edit::value(api_key.trim());
    }

    Ok(move_model_providers_before_profiles(
        &ensure_trailing_newline(doc.to_string()),
    ))
}

pub fn normalize_relay_profile_for_storage(profile: &mut RelayProfile) -> anyhow::Result<()> {
    if profile.model_windows.trim().is_empty() && profile.model_list.contains('[') {
        let (clean_list, windows) =
            crate::model_suffix::migrate_model_list_with_suffixes(&profile.model_list);
        profile.model_list = clean_list;
        profile.model_windows = serde_json::to_string(&windows).unwrap_or_default();
    }
    if profile.relay_mode == crate::settings::RelayMode::Official && !profile.official_mix_api_key {
        let has_api_config = !profile.base_url.trim().is_empty()
            || !profile.api_key.trim().is_empty()
            || codex_auth_api_key(&profile.auth_contents).is_some()
            || config_has_model_provider(profile.config_contents.as_str());
        if has_api_config {
            profile.config_contents.clear();
        }
        if !profile.model_list.trim().is_empty() {
            profile.model_list = merge_model_into_model_list(&profile.model, &profile.model_list);
        }
        profile.model.clear();
        profile.base_url.clear();
        profile.upstream_base_url.clear();
        profile.api_key.clear();
        if auth_contents_looks_like_chatgpt_auth(&profile.auth_contents) {
            profile.auth_contents =
                remove_openai_api_key_from_auth_contents(&profile.auth_contents)?;
        } else {
            profile.auth_contents.clear();
        }
        return Ok(());
    }
    let source_base_url = relay_profile_base_url(profile);
    let source_api_key = relay_profile_api_key(profile);
    if !profile.config_contents.trim().is_empty()
        || profile.relay_mode == crate::settings::RelayMode::PureApi
        || profile.official_mix_api_key
    {
        profile.config_contents = complete_relay_profile_config(profile)?;
    }
    if profile.relay_mode == crate::settings::RelayMode::PureApi
        && profile.auth_contents.trim().is_empty()
        && !source_api_key.trim().is_empty()
    {
        profile.auth_contents = serde_json::to_string_pretty(&json!({
            "OPENAI_API_KEY": source_api_key.trim()
        }))?;
    }
    if profile.relay_mode == crate::settings::RelayMode::Official {
        profile.auth_contents = remove_openai_api_key_from_auth_contents(&profile.auth_contents)?;
    }
    profile.model = relay_profile_model(profile);
    profile.model_list = merge_model_into_model_list(&profile.model, &profile.model_list);
    profile.upstream_base_url = source_base_url.clone();
    profile.base_url = source_base_url;
    profile.api_key = relay_profile_api_key(profile);
    Ok(())
}

fn remove_openai_api_key_from_auth_contents(auth_contents: &str) -> anyhow::Result<String> {
    if auth_contents.trim().is_empty() {
        return Ok(String::new());
    }
    let mut value =
        serde_json::from_str::<Value>(auth_contents).with_context(|| "auth.json JSON 解析失败")?;
    let Some(object) = value.as_object_mut() else {
        anyhow::bail!("auth.json 必须是 JSON 对象");
    };
    object.remove("OPENAI_API_KEY");
    if object.is_empty() {
        return Ok(String::new());
    }
    Ok(format!("{}\n", serde_json::to_string_pretty(&value)?))
}

fn merge_model_into_model_list(model: &str, model_list: &str) -> String {
    let model = model.trim();
    let mut models = Vec::new();
    if !model.is_empty() {
        models.push(model.to_string());
    }
    for item in model_list.split(['\r', '\n', ',']).map(str::trim) {
        if !item.is_empty() && !models.iter().any(|existing| existing == item) {
            models.push(item.to_string());
        }
    }
    models.join("\n")
}

fn config_has_model_provider(config_contents: &str) -> bool {
    parse_toml_document(config_contents)
        .ok()
        .and_then(|doc| {
            doc.get("model_provider")
                .and_then(Item::as_str)
                .map(str::to_string)
        })
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false)
}

fn auth_contents_looks_like_chatgpt_auth(contents: &str) -> bool {
    let Ok(value) = serde_json::from_str::<Value>(contents) else {
        return false;
    };
    let is_chatgpt = value
        .get("auth_mode")
        .and_then(Value::as_str)
        .map(|mode| mode.eq_ignore_ascii_case("chatgpt"))
        .unwrap_or(false);
    is_chatgpt
        && value
            .get("tokens")
            .map(tokens_have_login_secret)
            .unwrap_or(false)
}

fn provider_string_from_config(config_contents: &str, key: &str) -> Option<String> {
    let doc = parse_toml_document(config_contents).ok()?;
    let active = active_provider_id(&doc);
    if let Some(provider_id) = active.as_deref() {
        if let Some(value) = doc
            .get("model_providers")
            .and_then(Item::as_table)
            .and_then(|providers| providers.get(provider_id))
            .and_then(Item::as_table)
            .and_then(|provider| provider.get(key))
            .and_then(Item::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            return Some(value.to_string());
        }
    }

    for provider in provider_tables(&doc) {
        if let Some(value) = provider
            .get(key)
            .and_then(Item::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            return Some(value.to_string());
        }
    }
    None
}

fn experimental_bearer_token_from_config(config_contents: &str) -> anyhow::Result<Option<String>> {
    let doc = parse_toml_document(config_contents)?;
    if let Some(provider_id) = active_provider_id(&doc) {
        if let Some(token) = doc
            .get("model_providers")
            .and_then(Item::as_table)
            .and_then(|providers| providers.get(&provider_id))
            .and_then(Item::as_table)
            .and_then(|provider| provider.get("experimental_bearer_token"))
            .and_then(Item::as_str)
            .map(str::trim)
            .filter(|token| !token.is_empty())
        {
            return Ok(Some(token.to_string()));
        }
    }
    Ok(None)
}

fn remove_experimental_bearer_token_from_config(config_contents: &str) -> anyhow::Result<String> {
    let mut doc = parse_toml_document(config_contents)?;
    if let Some(providers) = doc.get_mut("model_providers").and_then(Item::as_table_mut) {
        for (_, item) in providers.iter_mut() {
            if let Some(provider) = item.as_table_like_mut() {
                provider.remove("experimental_bearer_token");
            }
        }
    }
    Ok(ensure_trailing_newline(doc.to_string()))
}

fn provider_tables(doc: &DocumentMut) -> Vec<&dyn TableLike> {
    let mut tables: Vec<&dyn TableLike> = Vec::new();
    if let Some(providers) = doc.get("model_providers").and_then(Item::as_table) {
        for (_, item) in providers.iter() {
            if let Some(provider) = item.as_table_like() {
                tables.push(provider);
            }
        }
    }
    tables
}

fn ensure_provider_table<'a>(
    doc: &'a mut DocumentMut,
    provider_id: &str,
) -> anyhow::Result<&'a mut Table> {
    let providers = table_mut_or_insert(doc, "model_providers")?;
    if !providers.contains_key(provider_id)
        || providers
            .get(provider_id)
            .and_then(Item::as_table)
            .is_none()
    {
        providers.insert(provider_id, toml_edit::table());
    }
    providers
        .get_mut(provider_id)
        .and_then(Item::as_table_mut)
        .ok_or_else(|| anyhow::anyhow!("model_providers.{provider_id} 必须是 TOML table"))
}

fn remove_provider_table(doc: &mut DocumentMut, provider_id: &str) {
    if let Some(providers) = doc.get_mut("model_providers").and_then(Item::as_table_mut) {
        providers.remove(provider_id);
        if providers.is_empty() {
            doc.as_table_mut().remove("model_providers");
        }
    }
}

fn retain_only_provider_table(doc: &mut DocumentMut, provider_id: &str) {
    if let Some(providers) = doc.get_mut("model_providers").and_then(Item::as_table_mut) {
        let provider = providers
            .remove(provider_id)
            .unwrap_or_else(toml_edit::table);
        providers.clear();
        providers.insert(provider_id, provider);
    }
}

fn rename_provider_table(doc: &mut DocumentMut, from: &str, to: &str) {
    if from == to {
        return;
    }
    if let Some(providers) = doc.get_mut("model_providers").and_then(Item::as_table_mut) {
        let moved = providers.remove(from).unwrap_or_else(toml_edit::table);
        providers.insert(to, moved);
    }
}

fn rewrite_profile_provider_refs(doc: &mut DocumentMut, from: &str, to: &str) {
    let Some(profiles) = doc.get_mut("profiles").and_then(Item::as_table_mut) else {
        return;
    };
    for (_, item) in profiles.iter_mut() {
        let Some(profile) = item.as_table_mut() else {
            continue;
        };
        if profile
            .get("model_provider")
            .and_then(Item::as_str)
            .is_some_and(|provider| provider == from)
        {
            profile.insert("model_provider", toml_edit::value(to));
        }
    }
}

fn read_optional_text(path: &Path) -> anyhow::Result<String> {
    match std::fs::read_to_string(path) {
        Ok(contents) => Ok(contents),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(error) => Err(error.into()),
    }
}

fn read_optional_bytes(path: &Path) -> anyhow::Result<Option<Vec<u8>>> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn restore_optional_file(path: &Path, contents: Option<&[u8]>) -> anyhow::Result<()> {
    match contents {
        Some(contents) => crate::settings::atomic_write(path, contents),
        None => match std::fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        },
    }
}

fn create_live_backup(
    home: &Path,
    config: Option<&[u8]>,
    auth: Option<&[u8]>,
) -> anyhow::Result<Option<String>> {
    if config.is_none() && auth.is_none() {
        return Ok(None);
    }

    let backup_dir = home
        .join("backups")
        .join(format!("codex-plus-live-{}", timestamp_millis()));
    std::fs::create_dir_all(&backup_dir)?;
    if let Some(config) = config {
        std::fs::write(backup_dir.join("config.toml"), config)?;
    }
    if let Some(auth) = auth {
        std::fs::write(backup_dir.join("auth.json"), auth)?;
    }
    Ok(Some(backup_dir.to_string_lossy().to_string()))
}

fn timestamp_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn ensure_trailing_newline(mut contents: String) -> String {
    if !contents.ends_with('\n') {
        contents.push('\n');
    }
    contents
}

fn move_model_providers_before_profiles(contents: &str) -> String {
    let lines = contents.lines().collect::<Vec<_>>();
    let Some(provider_start) = lines
        .iter()
        .position(|line| line.trim_start().starts_with("[model_providers."))
    else {
        return ensure_trailing_newline(contents.to_string());
    };
    let provider_end = lines[provider_start + 1..]
        .iter()
        .position(|line| line.trim_start().starts_with('['))
        .map(|offset| provider_start + 1 + offset)
        .unwrap_or(lines.len());
    let Some(profile_start) = lines
        .iter()
        .position(|line| line.trim_start().starts_with("[profiles."))
    else {
        return ensure_trailing_newline(contents.to_string());
    };
    if provider_start < profile_start {
        return ensure_trailing_newline(contents.to_string());
    }

    let mut output = Vec::with_capacity(lines.len());
    output.extend_from_slice(&lines[..profile_start]);
    output.extend_from_slice(&lines[provider_start..provider_end]);
    if output.last().is_some_and(|line| !line.trim().is_empty()) {
        output.push("");
    }
    output.extend_from_slice(&lines[profile_start..provider_start]);
    output.extend_from_slice(&lines[provider_end..]);
    ensure_trailing_newline(output.join("\n"))
}

fn auth_json_chatgpt_account_label(path: &Path) -> Option<Option<String>> {
    let Ok(contents) = std::fs::read_to_string(path) else {
        return None;
    };
    let Ok(value) = serde_json::from_str::<Value>(&contents) else {
        return None;
    };
    let is_chatgpt = value
        .get("auth_mode")
        .and_then(Value::as_str)
        .map(|mode| mode.eq_ignore_ascii_case("chatgpt"))
        .unwrap_or(false);
    let tokens = value.get("tokens")?;
    if !is_chatgpt || !tokens_have_login_secret(tokens) {
        return None;
    }
    Some(account_label_from_tokens(tokens))
}

fn tokens_have_login_secret(tokens: &Value) -> bool {
    ["access_token", "id_token", "refresh_token"]
        .iter()
        .any(|key| {
            tokens
                .get(*key)
                .and_then(Value::as_str)
                .map(|token| !token.trim().is_empty())
                .unwrap_or(false)
        })
}

fn account_label_from_tokens(tokens: &Value) -> Option<String> {
    ["id_token", "access_token"].iter().find_map(|key| {
        tokens
            .get(*key)
            .and_then(Value::as_str)
            .and_then(account_label_from_jwt)
    })
}

fn account_label_from_jwt(token: &str) -> Option<String> {
    let payload = token.split('.').nth(1)?;
    use base64::Engine;
    let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload.as_bytes())
        .ok()
        .or_else(|| {
            base64::engine::general_purpose::URL_SAFE
                .decode(payload.as_bytes())
                .ok()
        })?;
    let value: Value = serde_json::from_slice(&decoded).ok()?;
    value
        .get("email")
        .and_then(Value::as_str)
        .or_else(|| {
            value
                .get("https://api.openai.com/profile")
                .and_then(|profile| profile.get("email"))
                .and_then(Value::as_str)
        })
        .or_else(|| value.get("name").and_then(Value::as_str))
        .map(str::trim)
        .filter(|label| !label.is_empty())
        .map(ToString::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backfill_relay_profile_from_home_with_common_restores_template_provider_id() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(
            temp.path().join("config.toml"),
            "model_provider = \"custom\"\nmodel = \"gpt-image-2\"\n\n[model_providers.custom]\nname = \"custom\"\nwire_api = \"responses\"\nrequires_openai_auth = true\nbase_url = \"https://ahg.codes\"\n",
        )
        .unwrap();
        std::fs::write(temp.path().join("auth.json"), "{}\n").unwrap();

        let mut profile = RelayProfile {
            relay_mode: crate::settings::RelayMode::PureApi,
            protocol: crate::settings::RelayProtocol::Responses,
            config_contents: "model_provider = \"ai\"\nmodel = \"gpt-image-2\"\n\n[model_providers.ai]\nname = \"ai\"\nwire_api = \"responses\"\nrequires_openai_auth = true\nbase_url = \"https://ahg.codes\"\n"
                .to_string(),
            auth_contents: "{}\n".to_string(),
            ..RelayProfile::default()
        };
        let mut common = String::new();

        backfill_relay_profile_from_home_with_common(temp.path(), &mut profile, &mut common)
            .unwrap();

        assert!(profile.config_contents.contains("model_provider = \"ai\""));
        assert!(profile.config_contents.contains("[model_providers.ai]"));
        assert!(!profile.config_contents.contains("[model_providers.custom]"));
    }

    #[test]
    fn relay_profile_model_prefers_config_then_field_then_empty() {
        // 1. 供應商測試的回退第一級：config.toml 的 model = 優先
        let from_config = RelayProfile {
            config_contents: "model = \"deepseek-v4-flash\"\nmodel_provider = \"custom\"\n"
                .to_string(),
            model: "should-not-be-used".to_string(),
            ..RelayProfile::default()
        };
        assert_eq!(relay_profile_model(&from_config), "deepseek-v4-flash");

        // 2. config 沒寫 model 時退回 profile.model 欄位
        let from_field = RelayProfile {
            config_contents: "model_provider = \"custom\"\n".to_string(),
            model: "deepseek-v4-pro".to_string(),
            ..RelayProfile::default()
        };
        assert_eq!(relay_profile_model(&from_field), "deepseek-v4-pro");

        // 3. 兩者皆空 → 空字串；呼叫端據此才回退到全域 relayTestModel
        let empty = RelayProfile {
            config_contents: String::new(),
            model: String::new(),
            ..RelayProfile::default()
        };
        assert!(relay_profile_model(&empty).trim().is_empty());
    }
}

pub fn root_key_string(contents: &str, key: &str) -> Option<String> {
    root_key_value(contents, key).map(unquote_toml_string)
}

fn root_key_value<'a>(contents: &'a str, key: &str) -> Option<&'a str> {
    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            return None;
        }
        if trimmed.starts_with('#') || trimmed.is_empty() {
            continue;
        }
        let Some((name, value)) = trimmed.split_once('=') else {
            continue;
        };
        if name.trim() == key {
            return Some(value);
        }
    }
    None
}

fn upsert_model_provider_config(
    contents: &str,
    base_url: &str,
    bearer_token: &str,
) -> anyhow::Result<String> {
    let mut doc = parse_toml_document(contents)?;
    let provider_id = active_or_default_provider_id(&doc);
    set_provider_id(&mut doc, &provider_id);
    for legacy_provider in LEGACY_RELAY_PROVIDERS {
        remove_provider_table(&mut doc, legacy_provider);
    }
    if provider_id != RELAY_PROVIDER {
        remove_provider_table(&mut doc, RELAY_PROVIDER);
    }

    let provider = ensure_provider_table(&mut doc, &provider_id)?;
    provider["name"] = toml_edit::value(provider_id.as_str());
    provider["wire_api"] = toml_edit::value("responses");
    provider["requires_openai_auth"] = toml_edit::value(true);
    provider["base_url"] = toml_edit::value(base_url);
    provider["experimental_bearer_token"] = toml_edit::value(bearer_token);

    Ok(move_model_providers_before_profiles(
        &ensure_trailing_newline(doc.to_string()),
    ))
}

fn remove_table(contents: &str, table: &str) -> String {
    let header = format!("[{table}]");
    let mut lines = Vec::new();
    let mut skipping = false;
    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            if trimmed == header {
                skipping = true;
                continue;
            }
            skipping = false;
        }
        if !skipping {
            lines.push(line.to_string());
        }
    }
    lines.join("\n")
}

fn remove_root_key(contents: &str, key: &str) -> String {
    let mut lines = Vec::new();
    let mut in_root = true;
    for line in contents.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with('[') {
            in_root = false;
        }
        if in_root && root_line_key(line) == Some(key) {
            continue;
        }
        lines.push(line.to_string());
    }
    lines.join("\n")
}

fn table_values(contents: &str, table: &str) -> Option<std::collections::HashMap<String, String>> {
    let header = format!("[{table}]");
    let mut in_table = false;
    let mut values = std::collections::HashMap::new();
    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            if in_table {
                break;
            }
            in_table = trimmed == header;
            continue;
        }
        if !in_table || trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = trimmed.split_once('=') {
            values.insert(key.trim().to_string(), value.trim().to_string());
        }
    }
    in_table.then_some(values)
}

fn unquote_toml_string(value: &str) -> String {
    let value = value.trim();
    value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .unwrap_or(value)
        .to_string()
}

fn root_line_key(line: &str) -> Option<&str> {
    let trimmed = line.trim();
    if trimmed.starts_with('#') || trimmed.starts_with('[') {
        return None;
    }
    trimmed.split_once('=').map(|(key, _)| key.trim())
}
