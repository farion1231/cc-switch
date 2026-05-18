use crate::opencode_subscription::models::{
    OpenCodeSubscriptionConnectionResult, OpenCodeSubscriptionError,
    OpenCodeSubscriptionProviderRecord, OpenCodeSubscriptionStreamResult,
    SaveOpenCodeSubscriptionProviderRequest,
};
use crate::opencode_subscription::provider::{
    build_provider, is_openai_chat_completions_endpoint, validate_save_request,
};
use crate::store::AppState;
use futures::StreamExt;
use serde::Deserialize;
use serde_json::{json, Value};
use std::time::Instant;
use tauri::State;

type CommandResult<T> = Result<T, OpenCodeSubscriptionError>;

#[tauri::command]
pub async fn opencode_subscription_save_provider(
    state: State<'_, AppState>,
    req: SaveOpenCodeSubscriptionProviderRequest,
) -> CommandResult<OpenCodeSubscriptionProviderRecord> {
    validate_save_request(&req)?;
    let provider = build_provider(&req);
    let provider_id = provider.id.clone();
    state
        .db
        .save_provider("claude", &provider)
        .map_err(db_error)?;
    state
        .db
        .save_opencode_subscription_provider(
            &provider_id,
            req.subscription_kind,
            &req.base_url,
            &format!("provider://claude/{provider_id}/ANTHROPIC_AUTH_TOKEN"),
            req.default_model.as_deref(),
        )
        .map_err(db_error)
}

#[tauri::command]
pub async fn opencode_subscription_test_connection(
    state: State<'_, AppState>,
    provider_id: String,
) -> CommandResult<OpenCodeSubscriptionConnectionResult> {
    let credentials = provider_credentials(&state, &provider_id)?;
    let started = Instant::now();
    let client = reqwest::Client::new();
    let response = client
        .get(endpoint_url(&credentials.base_url, "models")?)
        .bearer_auth(&credentials.api_key)
        .send()
        .await
        .map_err(connection_error)?;
    let status = response.status();
    let body: Value = response.json().await.map_err(connection_error)?;
    if !status.is_success() {
        return Err(OpenCodeSubscriptionError::new(
            "PROVIDER_CONNECTION_FAILED",
            "OpenCode endpoint rejected the connection test.",
            "Verify the API key, endpoint, and account status.",
        )
        .with_details(redact_value(&body).to_string()));
    }
    let models = extract_models(&body);
    Ok(OpenCodeSubscriptionConnectionResult {
        success: true,
        provider_id,
        status: Some(status.as_u16()),
        latency_ms: started.elapsed().as_millis(),
        message: "Connection test succeeded.".to_string(),
        models,
    })
}

#[tauri::command]
pub async fn opencode_subscription_list_models(
    state: State<'_, AppState>,
    provider_id: String,
) -> CommandResult<Vec<String>> {
    Ok(opencode_subscription_test_connection(state, provider_id)
        .await?
        .models)
}

#[tauri::command]
pub async fn opencode_subscription_test_stream(
    state: State<'_, AppState>,
    provider_id: String,
) -> CommandResult<OpenCodeSubscriptionStreamResult> {
    let credentials = provider_credentials(&state, &provider_id)?;
    let started = Instant::now();
    let client = reqwest::Client::new();
    let model = credentials
        .default_model
        .as_deref()
        .filter(|model| !model.trim().is_empty())
        .unwrap_or("auto");
    let response = client
        .post(endpoint_url(&credentials.base_url, "chat/completions")?)
        .bearer_auth(&credentials.api_key)
        .json(&json!({
            "model": model,
            "stream": true,
            "messages": [
                { "role": "user", "content": "Reply with ok." }
            ],
            "max_tokens": 8
        }))
        .send()
        .await
        .map_err(connection_error)?;
    let status = response.status();
    if !status.is_success() {
        let details = response
            .text()
            .await
            .unwrap_or_else(|_| "failed to read error body".to_string());
        return Err(OpenCodeSubscriptionError::new(
            "PROVIDER_CONNECTION_FAILED",
            "OpenCode endpoint rejected the stream test.",
            "Verify the endpoint supports OpenAI-compatible streaming chat completions.",
        )
        .with_details(redact_text(&details)));
    }

    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let bytes = chunk.map_err(connection_error)?;
        if bytes.is_empty() {
            continue;
        }
        let text = String::from_utf8_lossy(&bytes);
        let first_line = text
            .lines()
            .find(|line| !line.trim().is_empty())
            .map(str::trim)
            .map(str::to_string);
        if let Some(first_event) = first_line {
            if first_event.starts_with("data:") {
                return Ok(OpenCodeSubscriptionStreamResult {
                    success: true,
                    provider_id,
                    status: Some(status.as_u16()),
                    latency_ms: started.elapsed().as_millis(),
                    first_event: Some(redact_text(&first_event)),
                    message: "Streaming test returned an SSE data event.".to_string(),
                });
            }
            return Err(OpenCodeSubscriptionError::new(
                "STREAM_FORMAT_UNSUPPORTED",
                "OpenCode endpoint returned a non-SSE streaming chunk.",
                "Use an OpenAI-compatible streaming endpoint or disable stream tests for this provider.",
            )
            .with_details(redact_text(&first_event)));
        }
    }

    Err(OpenCodeSubscriptionError::new(
        "STREAM_FORMAT_UNSUPPORTED",
        "OpenCode endpoint returned no streaming chunks.",
        "Verify the endpoint supports streaming chat completions.",
    ))
}

#[derive(Debug)]
struct Credentials {
    api_key: String,
    base_url: String,
    default_model: Option<String>,
}

fn provider_credentials(
    state: &State<'_, AppState>,
    provider_id: &str,
) -> CommandResult<Credentials> {
    let provider = state
        .db
        .get_provider_by_id(provider_id, "claude")
        .map_err(db_error)?
        .ok_or_else(|| {
            OpenCodeSubscriptionError::new(
                "PROVIDER_CONNECTION_FAILED",
                "OpenCode subscription provider was not found.",
                "Save the OpenCode Go/Zen provider before testing it.",
            )
        })?;

    let provider_type = provider
        .meta
        .as_ref()
        .and_then(|meta| meta.provider_type.as_deref())
        .unwrap_or_default();
    if !matches!(
        provider_type,
        "opencode_go_subscription" | "opencode_zen_subscription"
    ) {
        return Err(OpenCodeSubscriptionError::new(
            "PROVIDER_CONNECTION_FAILED",
            "Selected provider is not an OpenCode Go/Zen subscription provider.",
            "Choose a provider saved by OpenCode Subscriptions.",
        ));
    }

    let env = provider
        .settings_config
        .get("env")
        .and_then(|value| value.as_object())
        .ok_or_else(|| {
            OpenCodeSubscriptionError::new(
                "PROVIDER_CONNECTION_FAILED",
                "Provider config is missing env settings.",
                "Save the OpenCode provider again.",
            )
        })?;

    let api_key = env
        .get("ANTHROPIC_AUTH_TOKEN")
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            OpenCodeSubscriptionError::new(
                "PROVIDER_CONNECTION_FAILED",
                "Provider config is missing API key.",
                "Save the OpenCode provider with a legal API key.",
            )
        })?
        .to_string();
    let base_url = env
        .get("ANTHROPIC_BASE_URL")
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            OpenCodeSubscriptionError::new(
                "PROVIDER_CONNECTION_FAILED",
                "Provider config is missing endpoint.",
                "Save the OpenCode provider with a valid endpoint.",
            )
        })?
        .to_string();
    let default_model = env
        .get("ANTHROPIC_MODEL")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);

    Ok(Credentials {
        api_key,
        base_url,
        default_model,
    })
}

fn endpoint_url(base_url: &str, path: &str) -> CommandResult<String> {
    let mut base = base_url.trim_end_matches('/').to_string();
    if is_openai_chat_completions_endpoint(&base) {
        if path.trim_matches('/') == "chat/completions" {
            return Ok(base);
        }

        const CHAT_COMPLETIONS_SUFFIX: &str = "/chat/completions";
        let prefix_len = base.len().saturating_sub(CHAT_COMPLETIONS_SUFFIX.len());
        base.truncate(prefix_len);
        return Ok(format!(
            "{}/{}",
            base.trim_end_matches('/'),
            path.trim_start_matches('/')
        ));
    }

    if !base.ends_with("/v1") {
        base.push_str("/v1");
    }
    Ok(format!("{base}/{path}"))
}

fn extract_models(body: &Value) -> Vec<String> {
    #[derive(Debug, Deserialize)]
    struct ModelItem {
        id: Option<String>,
        name: Option<String>,
    }

    body.get("data")
        .or_else(|| body.get("models"))
        .and_then(|value| value.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| serde_json::from_value::<ModelItem>(item.clone()).ok())
                .filter_map(|item| item.id.or(item.name))
                .collect()
        })
        .unwrap_or_default()
}

fn db_error(error: crate::AppError) -> OpenCodeSubscriptionError {
    OpenCodeSubscriptionError::new(
        "PROVIDER_CONNECTION_FAILED",
        "OpenCode subscription provider storage failed.",
        "Retry the operation. Existing Provider switching remains isolated from this error.",
    )
    .with_details(error.to_string())
}

fn connection_error(error: reqwest::Error) -> OpenCodeSubscriptionError {
    OpenCodeSubscriptionError::new(
        "PROVIDER_CONNECTION_FAILED",
        "OpenCode endpoint request failed.",
        "Verify the endpoint, API key, and network connection.",
    )
    .with_details(error.to_string())
}

fn redact_value(value: &Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(key, value)| {
                    if is_secret_key(key) {
                        (key.clone(), Value::String("[redacted]".to_string()))
                    } else {
                        (key.clone(), redact_value(value))
                    }
                })
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(redact_value).collect()),
        _ => value.clone(),
    }
}

fn redact_text(value: &str) -> String {
    let mut redacted = value.to_string();
    for key in ["api_key", "token", "authorization", "cookie", "session"] {
        redacted = redacted.replace(key, "[redacted-key]");
    }
    redacted
}

fn is_secret_key(key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
    lower.contains("key")
        || lower.contains("token")
        || lower.contains("authorization")
        || lower.contains("cookie")
        || lower.contains("session")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_openai_model_list() {
        let body = json!({ "data": [{ "id": "gpt-a" }, { "id": "gpt-b" }] });
        assert_eq!(extract_models(&body), vec!["gpt-a", "gpt-b"]);
    }

    #[test]
    fn redacts_secret_keys_in_diagnostics() {
        let body = json!({ "error": { "token": "secret", "message": "bad" } });
        assert_eq!(redact_value(&body)["error"]["token"], "[redacted]");
    }

    #[test]
    fn endpoint_url_keeps_full_chat_completions_endpoint() {
        let base = "https://opencode.ai/zen/go/v1/chat/completions";
        assert_eq!(
            endpoint_url(base, "chat/completions").expect("chat endpoint"),
            base
        );
        assert_eq!(
            endpoint_url(base, "models").expect("models endpoint"),
            "https://opencode.ai/zen/go/v1/models"
        );
    }
}
