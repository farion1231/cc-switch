use std::time::Duration;

use anyhow::Context;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::settings::BackendSettings;

const MAX_PROMPT_LENGTH: usize = 420;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StepwiseRequest {
    #[serde(default)]
    pub last_user_message: String,
    #[serde(default)]
    pub last_assistant_message: String,
    #[serde(default)]
    pub thread_title: String,
    #[serde(default)]
    pub page_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StepwiseItem {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub label: String,
    pub prompt: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StepwisePublicSettings {
    pub enabled: bool,
    pub direct_send: bool,
    pub base_url_configured: bool,
    pub api_key_configured: bool,
    pub api_key_env: String,
    pub api_key_env_configured: bool,
    pub model: String,
    pub max_items: u8,
    pub max_input_chars: u32,
    pub max_output_tokens: u32,
    pub timeout_ms: u64,
}

pub fn public_settings(settings: &BackendSettings) -> StepwisePublicSettings {
    StepwisePublicSettings {
        enabled: settings.codex_app_stepwise_enabled,
        direct_send: settings.codex_app_stepwise_direct_send,
        base_url_configured: !settings.codex_app_stepwise_base_url.trim().is_empty(),
        api_key_configured: !stepwise_api_key(settings).is_empty(),
        api_key_env: settings.codex_app_stepwise_api_key_env.clone(),
        api_key_env_configured: std::env::var(settings.codex_app_stepwise_api_key_env.trim())
            .map(|value| !value.trim().is_empty())
            .unwrap_or(false),
        model: settings.codex_app_stepwise_model.clone(),
        max_items: settings.codex_app_stepwise_max_items,
        max_input_chars: settings.codex_app_stepwise_max_input_chars,
        max_output_tokens: settings.codex_app_stepwise_max_output_tokens,
        timeout_ms: settings.codex_app_stepwise_timeout_ms,
    }
}

pub fn settings_with_payload(mut settings: BackendSettings, payload: &Value) -> BackendSettings {
    let Some(raw_settings) = payload.get("settings").and_then(Value::as_object) else {
        return settings;
    };
    if let Some(value) = raw_settings
        .get("codexAppStepwiseEnabled")
        .and_then(Value::as_bool)
    {
        settings.codex_app_stepwise_enabled = value;
    }
    if let Some(value) = raw_settings
        .get("codexAppStepwiseDirectSend")
        .and_then(Value::as_bool)
    {
        settings.codex_app_stepwise_direct_send = value;
    }
    if let Some(value) = raw_settings
        .get("codexAppStepwiseBaseUrl")
        .and_then(Value::as_str)
    {
        settings.codex_app_stepwise_base_url = value.trim().trim_end_matches('/').to_string();
    }
    if let Some(value) = raw_settings
        .get("codexAppStepwiseApiKey")
        .and_then(Value::as_str)
    {
        settings.codex_app_stepwise_api_key = value.trim().to_string();
    }
    if let Some(value) = raw_settings
        .get("codexAppStepwiseApiKeyEnv")
        .and_then(Value::as_str)
    {
        settings.codex_app_stepwise_api_key_env = if value.trim().is_empty() {
            crate::settings::default_stepwise_api_key_env()
        } else {
            value.trim().to_string()
        };
    }
    if let Some(value) = raw_settings
        .get("codexAppStepwiseModel")
        .and_then(Value::as_str)
    {
        settings.codex_app_stepwise_model = value.trim().to_string();
    }
    if let Some(value) = raw_settings
        .get("codexAppStepwiseMaxItems")
        .and_then(Value::as_u64)
        .and_then(|value| u8::try_from(value).ok())
    {
        settings.codex_app_stepwise_max_items = crate::settings::clamp_stepwise_max_items(value);
    }
    if let Some(value) = raw_settings
        .get("codexAppStepwiseMaxInputChars")
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
    {
        settings.codex_app_stepwise_max_input_chars =
            crate::settings::clamp_stepwise_max_input_chars(value);
    }
    if let Some(value) = raw_settings
        .get("codexAppStepwiseMaxOutputTokens")
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
    {
        settings.codex_app_stepwise_max_output_tokens =
            crate::settings::clamp_stepwise_max_output_tokens(value);
    }
    if let Some(value) = raw_settings
        .get("codexAppStepwiseTimeoutMs")
        .and_then(Value::as_u64)
    {
        settings.codex_app_stepwise_timeout_ms = crate::settings::clamp_stepwise_timeout_ms(value);
    }
    settings
}

pub async fn generate(
    request: StepwiseRequest,
    settings: &BackendSettings,
) -> anyhow::Result<Value> {
    if !settings.codex_app_stepwise_enabled {
        return Ok(json!({ "status": "ok", "disabled": true, "items": [] }));
    }

    let base_url = settings
        .codex_app_stepwise_base_url
        .trim()
        .trim_end_matches('/');
    let api_key = stepwise_api_key(settings);
    let model = settings.codex_app_stepwise_model.trim();
    let max_items = settings.codex_app_stepwise_max_items;

    if max_items == 0 {
        return Ok(json!({ "status": "ok", "items": [] }));
    }
    if base_url.is_empty() || model.is_empty() {
        return Ok(json!({
            "status": "failed",
            "items": [],
            "error": "Stepwise Base URL or Model is not configured"
        }));
    }
    if api_key.is_empty() {
        return Ok(json!({
            "status": "failed",
            "items": [],
            "error": "Stepwise API Key is not configured"
        }));
    }

    let client = crate::http_client::proxied_client("")?;
    let timeout = Duration::from_millis(settings.codex_app_stepwise_timeout_ms);
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {api_key}"))
            .context("failed to build Stepwise authorization header")?,
    );

    let response = client
        .post(format!("{base_url}/chat/completions"))
        .headers(headers)
        .timeout(timeout)
        .json(&json!({
            "model": model,
            "messages": build_messages(&request, settings),
            "temperature": 0.2,
            "max_tokens": settings.codex_app_stepwise_max_output_tokens,
            "response_format": { "type": "json_object" },
        }))
        .send()
        .await
        .context("failed to request Stepwise API")?;

    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    if !status.is_success() {
        return Ok(json!({
            "status": "failed",
            "items": [],
            "error": format!("Stepwise upstream {}: {}", status.as_u16(), text.chars().take(240).collect::<String>())
        }));
    }

    let data: Value =
        serde_json::from_str(&text).context("failed to parse Stepwise API response")?;
    Ok(json!({
        "status": "ok",
        "items": extract_stepwise_items(&data, max_items)
    }))
}

pub async fn test_connection(settings: &BackendSettings) -> anyhow::Result<Value> {
    generate(
        StepwiseRequest {
            last_user_message: "测试 Stepwise 配置。".to_string(),
            last_assistant_message: "Stepwise 应返回 0 到 6 条可直接发送的后续建议。".to_string(),
            thread_title: "Codex++ Stepwise test".to_string(),
            page_url: String::new(),
        },
        settings,
    )
    .await
}

pub fn build_messages(request: &StepwiseRequest, settings: &BackendSettings) -> Vec<Value> {
    let limit = settings.codex_app_stepwise_max_input_chars as usize;
    let last_user_message = short_text(&request.last_user_message, limit.saturating_mul(35) / 100);
    let last_assistant_message = short_text(
        &request.last_assistant_message,
        limit.saturating_mul(60) / 100,
    );
    let language_input = if last_user_message.trim().is_empty() {
        last_assistant_message.clone()
    } else {
        last_user_message.clone()
    };
    let system_content = [
        "You generate concise Codex Stepwise actions.",
        "Return strict JSON only, no markdown.",
        "Schema: {\"items\":[{\"prompt\":\"...\",\"label\":\"optional short label\"}]}",
        &format!(
            "Generate 1 to {} items when the assistant result is non-empty.",
            settings.codex_app_stepwise_max_items
        ),
        "Every prompt must be directly sendable by the user.",
        "Use the latest user intent and assistant result. Avoid generic filler.",
        "Language policy: write Stepwise prompts in the dominant natural language of languageInput.",
        "Ignore technical terms, file names, commands, APIs, and product names when detecting language; keep them in their original language when natural.",
        "Return {\"items\":[]} only when both the user intent and assistant result are empty or unusable.",
    ]
    .join("\n");
    vec![
        json!({
            "role": "system",
            "content": system_content
        }),
        json!({
            "role": "user",
            "content": json!({
                "lastUserMessage": last_user_message,
                "lastAssistantMessage": last_assistant_message,
                "languageInput": language_input,
                "threadTitle": short_text(&request.thread_title, 240),
                "pageUrl": short_text(&request.page_url, 240),
                "maxItems": settings.codex_app_stepwise_max_items,
            }).to_string()
        }),
    ]
}

pub fn clamp_items(value: Value, max_items: u8) -> Vec<StepwiseItem> {
    let mut seen = std::collections::BTreeSet::new();
    let mut items = Vec::new();
    let max_items = usize::from(max_items);
    let Some(raw_items) = value.as_array() else {
        return items;
    };

    for raw in raw_items {
        let prompt = first_string_field(raw, &["prompt", "text", "action", "content", "message"])
            .or_else(|| raw.as_str())
            .unwrap_or("");
        let prompt = normalize_spaces(prompt);
        if prompt.is_empty() || seen.contains(&prompt) {
            continue;
        }
        seen.insert(prompt.clone());
        let label = first_string_field(raw, &["label", "title", "name"])
            .map(normalize_spaces)
            .unwrap_or_default();
        items.push(StepwiseItem {
            label: short_text(&label, 36),
            prompt: short_text(&prompt, MAX_PROMPT_LENGTH),
        });
        if items.len() >= max_items {
            break;
        }
    }

    items
}

pub fn extract_stepwise_items(data: &Value, max_items: u8) -> Vec<StepwiseItem> {
    for candidate in stepwise_payload_candidates(data) {
        if let Some(items) = stepwise_items_value(&candidate) {
            let items = clamp_items(items, max_items);
            if !items.is_empty() {
                return items;
            }
        }
    }
    Vec::new()
}

fn stepwise_payload_candidates(data: &Value) -> Vec<Value> {
    let mut candidates = Vec::new();
    candidates.push(data.clone());

    if let Some(content) = data
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .and_then(|choice| choice.get("message"))
        .and_then(|message| message.get("content"))
    {
        candidates.push(content.clone());
        if let Some(parsed) = parse_json_value(content) {
            candidates.push(parsed);
        }
    }

    for key in ["output", "response", "data", "result"] {
        if let Some(value) = data.get(key) {
            candidates.push(value.clone());
            if let Some(parsed) = parse_json_value(value) {
                candidates.push(parsed);
            }
        }
    }

    candidates
}

fn stepwise_items_value(value: &Value) -> Option<Value> {
    if value.as_array().is_some() {
        return Some(value.clone());
    }
    for key in [
        "items",
        "suggestions",
        "next_steps",
        "nextSteps",
        "actions",
        "prompts",
    ] {
        if let Some(items) = value.get(key).filter(|items| items.as_array().is_some()) {
            return Some(items.clone());
        }
    }
    None
}

fn parse_json_value(value: &Value) -> Option<Value> {
    let text = value.as_str()?.trim();
    if text.is_empty() {
        return None;
    }
    serde_json::from_str(text).ok()
}

fn first_string_field<'a>(value: &'a Value, keys: &[&str]) -> Option<&'a str> {
    for key in keys {
        if let Some(text) = value.get(key).and_then(Value::as_str) {
            return Some(text);
        }
    }
    None
}

fn stepwise_api_key(settings: &BackendSettings) -> String {
    let direct = settings.codex_app_stepwise_api_key.trim();
    if !direct.is_empty() {
        return direct.to_string();
    }
    std::env::var(settings.codex_app_stepwise_api_key_env.trim())
        .unwrap_or_default()
        .trim()
        .to_string()
}

fn short_text(value: &str, limit: usize) -> String {
    let text = normalize_text(value);
    if text.chars().count() <= limit {
        return text;
    }
    text.chars()
        .rev()
        .take(limit)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect()
}

fn normalize_text(value: &str) -> String {
    value
        .replace('\u{a0}', " ")
        .lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n")
        .split("\n\n\n")
        .collect::<Vec<_>>()
        .join("\n\n")
        .trim()
        .to_string()
}

fn normalize_spaces(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamp_items_dedupes_and_limits() {
        let items = clamp_items(
            json!([
                {"label": "继续", "prompt": "继续排查"},
                {"label": "重复", "prompt": "继续排查"},
                {"prompt": "补测试"},
                "更新文档"
            ]),
            2,
        );

        assert_eq!(items.len(), 2);
        assert_eq!(items[0].label, "继续");
        assert_eq!(items[0].prompt, "继续排查");
        assert_eq!(items[1].prompt, "补测试");
    }

    #[test]
    fn extracts_items_from_common_stepwise_response_shapes() {
        let response = json!({
            "choices": [{
                "message": {
                    "content": "{\"suggestions\":[{\"title\":\"继续排查\",\"text\":\"请继续检查 Stepwise 返回内容\"},{\"action\":\"补一个解析测试\"}]}"
                }
            }]
        });

        let items = extract_stepwise_items(&response, 6);

        assert_eq!(items.len(), 2);
        assert_eq!(items[0].label, "继续排查");
        assert_eq!(items[0].prompt, "请继续检查 Stepwise 返回内容");
        assert_eq!(items[1].prompt, "补一个解析测试");
    }

    #[test]
    fn prompt_contains_language_policy() {
        let settings = BackendSettings {
            codex_app_stepwise_max_items: 4,
            ..BackendSettings::default()
        };
        let messages = build_messages(
            &StepwiseRequest {
                last_user_message: "请补一个 directSend selftest，覆盖 ProseMirror。".to_string(),
                last_assistant_message: "已完成实现。".to_string(),
                thread_title: String::new(),
                page_url: String::new(),
            },
            &settings,
        );
        let system = messages[0].get("content").and_then(Value::as_str).unwrap();
        let user = messages[1].get("content").and_then(Value::as_str).unwrap();

        assert!(system.contains("dominant natural language"));
        assert!(system.contains("Generate 1 to 4 items when the assistant result is non-empty."));
        assert!(user.contains("directSend"));
        assert!(user.contains("languageInput"));
    }

    #[test]
    fn settings_with_payload_clamps_values() {
        let settings = settings_with_payload(
            BackendSettings::default(),
            &json!({
                "settings": {
                    "codexAppStepwiseEnabled": true,
                    "codexAppStepwiseDirectSend": true,
                    "codexAppStepwiseBaseUrl": "https://api.example.test/v1/",
                    "codexAppStepwiseApiKey": " sk-test ",
                    "codexAppStepwiseApiKeyEnv": "",
                    "codexAppStepwiseModel": " stepwise-mini ",
                    "codexAppStepwiseMaxItems": 9,
                    "codexAppStepwiseMaxInputChars": 999999,
                    "codexAppStepwiseMaxOutputTokens": 10,
                    "codexAppStepwiseTimeoutMs": 999999
                }
            }),
        );

        assert!(settings.codex_app_stepwise_enabled);
        assert!(settings.codex_app_stepwise_direct_send);
        assert_eq!(
            settings.codex_app_stepwise_base_url,
            "https://api.example.test/v1"
        );
        assert_eq!(settings.codex_app_stepwise_api_key, "sk-test");
        assert_eq!(
            settings.codex_app_stepwise_api_key_env,
            crate::settings::default_stepwise_api_key_env()
        );
        assert_eq!(settings.codex_app_stepwise_model, "stepwise-mini");
        assert_eq!(settings.codex_app_stepwise_max_items, 6);
        assert_eq!(settings.codex_app_stepwise_max_input_chars, 24000);
        assert_eq!(settings.codex_app_stepwise_max_output_tokens, 100);
        assert_eq!(settings.codex_app_stepwise_timeout_ms, 60000);
    }
}
