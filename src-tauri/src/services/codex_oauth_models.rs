//! Codex OAuth model list service.
//!
//! ChatGPT Codex exposes models through `chatgpt.com/backend-api/codex/models`,
//! which is not an OpenAI-compatible `/v1/models` endpoint.

use crate::proxy::providers::codex_oauth_auth::{
    CODEX_OAUTH_CLIENT_VERSION, CODEX_OAUTH_ORIGINATOR,
};
use crate::services::model_fetch::FetchedModel;
use futures::StreamExt;
use serde_json::Value;
use std::time::Duration;

pub(crate) const CODEX_OAUTH_MODELS_URL: &str = "https://chatgpt.com/backend-api/codex/models";
const CODEX_OAUTH_FETCH_TIMEOUT_SECS: u64 = 15;

pub async fn fetch_models_with_token(
    token: &str,
    account_id: &str,
) -> Result<Vec<FetchedModel>, String> {
    let client = crate::proxy::http_client::get();
    let response = build_models_request(&client, token, account_id)
        .send()
        .await
        .map_err(|e| format!("Request failed: {e}"))?;

    let status = response.status();
    if !status.is_success() {
        let body = read_bounded_model_response(response)
            .await
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
            .unwrap_or_default();
        return Err(format!("HTTP {status}: {body}"));
    }

    let raw = read_bounded_model_response(response)
        .await
        .map_err(|error| format!("Failed to read response: {error}"))?;
    let value: Value =
        serde_json::from_slice(&raw).map_err(|e| format!("Failed to parse response: {e}"))?;

    Ok(parse_models(value))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ActivationModelCapability {
    Supported,
    ModelUnavailable,
    LowUnavailable,
    CatalogInvalid,
}

pub(crate) async fn activation_model_capability_with_token(
    token: &str,
    account_id: &str,
) -> Result<ActivationModelCapability, String> {
    let client = crate::proxy::http_client::get();
    let response = build_models_request(&client, token, account_id)
        .timeout(Duration::from_secs(CODEX_OAUTH_FETCH_TIMEOUT_SECS))
        .send()
        .await
        .map_err(|_| "model_catalog_request_failed".to_string())?;
    if !response.status().is_success() {
        return Err(format!("model_catalog_http_{}", response.status().as_u16()));
    }
    let bytes = read_bounded_model_response(response).await?;
    let value: Value =
        serde_json::from_slice(&bytes).map_err(|_| "model_catalog_invalid_json".to_string())?;
    Ok(parse_activation_model_capability(&value))
}

fn parse_activation_model_supports_low(value: &Value) -> bool {
    matches!(
        parse_activation_model_capability(value),
        ActivationModelCapability::Supported
    )
}

pub(crate) fn parse_activation_model_capability(value: &Value) -> ActivationModelCapability {
    let entries = value
        .get("data")
        .and_then(Value::as_array)
        .or_else(|| value.get("models").and_then(Value::as_array))
        .or_else(|| value.get("items").and_then(Value::as_array))
        .or_else(|| value.as_array());
    let matching = if let Some(entries) = entries {
        entries
            .iter()
            .filter(|entry| {
                entry
                    .get("slug")
                    .or_else(|| entry.get("id"))
                    .or_else(|| entry.get("model"))
                    .or_else(|| entry.get("name"))
                    .and_then(Value::as_str)
                    .is_some_and(|id| id == "gpt-6-luna")
            })
            .collect::<Vec<_>>()
    } else if let Some(models) = value.get("models").and_then(Value::as_object) {
        models.get("gpt-6-luna").into_iter().collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    if matching.is_empty() {
        return ActivationModelCapability::ModelUnavailable;
    }
    if matching.len() > 1 {
        return ActivationModelCapability::CatalogInvalid;
    }
    let levels = [
        "reasoning_levels",
        "reasoningLevels",
        "supported_reasoning_levels",
        "supportedReasoningLevels",
    ]
    .iter()
    .find_map(|key| matching[0].get(*key).and_then(Value::as_array));
    let Some(levels) = levels else {
        return ActivationModelCapability::LowUnavailable;
    };
    let mut saw_low = false;
    for level in levels {
        let effort = match level {
            Value::String(level) => Some(level.as_str()),
            Value::Object(object) => object.get("effort").and_then(Value::as_str),
            _ => None,
        };
        let Some(effort) = effort else {
            return ActivationModelCapability::CatalogInvalid;
        };
        saw_low |= effort.eq_ignore_ascii_case("low");
    }
    if saw_low {
        ActivationModelCapability::Supported
    } else {
        ActivationModelCapability::LowUnavailable
    }
}

async fn read_bounded_model_response(response: reqwest::Response) -> Result<Vec<u8>, String> {
    const MAX_MODEL_RESPONSE_BYTES: usize = 2 * 1024 * 1024;
    let mut output = Vec::new();
    let mut stream = Box::pin(response.bytes_stream());
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| "model_catalog_read_failed".to_string())?;
        if output.len().saturating_add(chunk.len()) > MAX_MODEL_RESPONSE_BYTES {
            return Err("model_catalog_response_too_large".to_string());
        }
        output.extend_from_slice(&chunk);
    }
    Ok(output)
}

fn build_models_request(
    client: &reqwest::Client,
    token: &str,
    account_id: &str,
) -> reqwest::RequestBuilder {
    client
        .get(CODEX_OAUTH_MODELS_URL)
        .query(&[("client_version", CODEX_OAUTH_CLIENT_VERSION)])
        .header("Authorization", format!("Bearer {token}"))
        .header("originator", CODEX_OAUTH_ORIGINATOR)
        .header("version", CODEX_OAUTH_CLIENT_VERSION)
        .header("chatgpt-account-id", account_id)
        .timeout(Duration::from_secs(CODEX_OAUTH_FETCH_TIMEOUT_SECS))
}

fn parse_models(value: Value) -> Vec<FetchedModel> {
    let entries = value
        .get("data")
        .and_then(Value::as_array)
        .or_else(|| value.get("models").and_then(Value::as_array))
        .or_else(|| value.get("items").and_then(Value::as_array))
        .or_else(|| value.as_array());

    let mut models = Vec::new();

    if let Some(entries) = entries {
        for entry in entries {
            push_model_entry(&mut models, entry, None);
        }
    }

    if let Some(model_map) = value.get("models").and_then(Value::as_object) {
        for (key, entry) in model_map {
            push_model_entry(&mut models, entry, Some(key));
        }
    }

    models.sort_by(|a, b| a.id.cmp(&b.id));
    models.dedup_by(|a, b| a.id == b.id);
    models
}

fn push_model_entry(models: &mut Vec<FetchedModel>, entry: &Value, fallback_id: Option<&str>) {
    if let Some(id) = entry.as_str().map(str::trim).filter(|id| !id.is_empty()) {
        models.push(FetchedModel {
            id: id.to_string(),
            owned_by: Some("Codex".to_string()),
        });
        return;
    }

    let Some(obj) = entry.as_object() else {
        if let Some(id) = fallback_id.map(str::trim).filter(|id| !id.is_empty()) {
            models.push(FetchedModel {
                id: id.to_string(),
                owned_by: Some("Codex".to_string()),
            });
        }
        return;
    };

    let Some(id) = string_field(obj, &["slug", "id", "model", "name"]).or_else(|| {
        fallback_id
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .map(str::to_string)
    }) else {
        return;
    };
    let owned_by = string_field(
        obj,
        &[
            "owned_by", "ownedBy", "provider", "vendor", "category", "owner",
        ],
    )
    .or_else(|| Some("Codex".to_string()));

    models.push(FetchedModel { id, owned_by });
}

fn string_field(obj: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<String> {
    keys.iter()
        .filter_map(|key| obj.get(*key))
        .filter_map(Value::as_str)
        .map(str::trim)
        .find(|value| !value.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, response::IntoResponse, routing::get, Router};
    use serde_json::json;

    #[test]
    fn codex_oauth_model_discovery_uses_gpt6_compatible_identity() {
        let request = build_models_request(&reqwest::Client::new(), "test-token", "test-account")
            .build()
            .unwrap();
        assert_eq!(request.headers()["authorization"], "Bearer test-token");
        assert_eq!(request.headers()["chatgpt-account-id"], "test-account");
        assert_eq!(request.headers()["originator"], "codex_cli_rs");
        let version = request
            .url()
            .query_pairs()
            .find(|(key, _)| key == "client_version")
            .unwrap()
            .1
            .into_owned();
        let parts: Vec<u32> = version
            .split('.')
            .map(|part| part.parse().unwrap())
            .collect();
        // GPT-6.1 Sol is absent from the 0.155.0 catalog for this account; the
        // catalog gate sits above the model's advertised minimal_client_version.
        assert!(parts.as_slice() >= [0, 159, 0].as_slice());
        assert_eq!(request.headers()["version"], version);
        let models = parse_models(json!({"models": [
            {"slug": "gpt-6.1-sol", "minimal_client_version": "0.153.0"},
            {"slug": "gpt-6-sol", "minimal_client_version": "0.155.0"}
        ]}));
        assert_eq!(models[0].id, "gpt-6-sol");
        assert_eq!(models[1].id, "gpt-6.1-sol");
    }

    #[test]
    fn parse_codex_oauth_models_accepts_openai_style_data() {
        let models = parse_models(json!({
            "data": [
                { "id": "gpt-5.4", "owned_by": "openai" },
                { "id": "gpt-5.4-mini", "ownedBy": "openai" }
            ]
        }));

        assert_eq!(models.len(), 2);
        assert_eq!(models[0].id, "gpt-5.4");
        assert_eq!(models[0].owned_by.as_deref(), Some("openai"));
        assert_eq!(models[1].id, "gpt-5.4-mini");
        assert_eq!(models[1].owned_by.as_deref(), Some("openai"));
    }

    #[test]
    fn parse_codex_oauth_models_accepts_model_list_shape() {
        let models = parse_models(json!({
            "models": [
                { "slug": "gpt-5.3-codex", "display_name": "GPT-5.3 Codex" },
                "gpt-5.5"
            ]
        }));

        assert_eq!(
            models.into_iter().map(|model| model.id).collect::<Vec<_>>(),
            vec!["gpt-5.3-codex".to_string(), "gpt-5.5".to_string()]
        );
    }

    #[test]
    fn parse_codex_oauth_models_deduplicates_ids() {
        let models = parse_models(json!({
            "data": [
                { "id": "gpt-5.4" },
                { "model": "gpt-5.4" }
            ]
        }));

        assert_eq!(models.len(), 1);
        assert_eq!(models[0].id, "gpt-5.4");
    }

    #[test]
    fn parse_codex_oauth_models_accepts_model_map_shape() {
        let models = parse_models(json!({
            "models": {
                "gpt-5.4": { "display_name": "GPT-5.4" },
                "gpt-5.5": { "slug": "gpt-5.5" }
            }
        }));

        assert_eq!(
            models.into_iter().map(|model| model.id).collect::<Vec<_>>(),
            vec!["gpt-5.4".to_string(), "gpt-5.5".to_string()]
        );
    }

    #[test]
    fn activation_model_preflight_requires_account_model_and_low_capability() {
        assert!(parse_activation_model_supports_low(&json!({"data":[
            {"slug":"gpt-6-luna","reasoning_levels":["low","high"]}
        ]})));
        assert!(!parse_activation_model_supports_low(&json!({"data":[
            {"slug":"gpt-6-luna","reasoning_levels":["medium","high"]}
        ]})));
        assert!(!parse_activation_model_supports_low(&json!({"data":[
            {"slug":"gpt-6-luna"}
        ]})));
        assert!(!parse_activation_model_supports_low(&json!({"data":[
            "gpt-6-luna"
        ]})));
        assert!(!parse_activation_model_supports_low(&json!({"data":[
            {"slug":"gpt-6-luna","reasoning_levels":["low"]},
            {"slug":"gpt-6-luna","reasoning_levels":["high"]}
        ]})));
        assert!(parse_activation_model_supports_low(&json!({"models": {
            "gpt-6-luna": {"reasoningLevels":["low"]}
        }})));
    }

    #[test]
    fn activation_model_preflight_accepts_low_in_object_reasoning_levels() {
        assert!(parse_activation_model_supports_low(&json!({
            "data": [{
                "slug": "gpt-6-luna",
                "supported_reasoning_levels": [
                    {"effort": "low", "description": "Low"},
                    {"effort": "high", "description": "High"}
                ]
            }]
        })));
    }

    #[test]
    fn activation_model_preflight_rejects_missing_low_in_object_reasoning_levels() {
        assert!(!parse_activation_model_supports_low(&json!({
            "models": [{
                "id": "gpt-6-luna",
                "reasoningLevels": [
                    {"effort": "medium"},
                    {"effort": "high"}
                ]
            }]
        })));
        assert!(!parse_activation_model_supports_low(&json!({
            "models": [{"id": "gpt-6-luna"}]
        })));
        assert_eq!(
            parse_activation_model_capability(&json!({
                "models": [{
                    "id": "gpt-6-luna",
                    "supported_reasoning_levels": [{"description": "missing effort"}]
                }]
            })),
            ActivationModelCapability::CatalogInvalid
        );
    }

    #[test]
    fn activation_model_preflight_rejects_conflicting_object_model_entries() {
        assert!(!parse_activation_model_supports_low(&json!({
            "items": [
                {
                    "name": "gpt-6-luna",
                    "supportedReasoningLevels": [{"effort": "low"}]
                },
                {
                    "name": "gpt-6-luna",
                    "supportedReasoningLevels": [{"effort": "high"}]
                }
            ]
        })));
    }

    #[tokio::test]
    async fn activation_catalog_size_limit_stops_while_streaming() {
        async fn oversized() -> axum::response::Response {
            (
                [(axum::http::header::CONTENT_TYPE, "application/json")],
                Body::from(vec![b'x'; 2 * 1024 * 1024 + 1]),
            )
                .into_response()
        }
        let app = Router::new().route("/models", get(oversized));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let response = reqwest::Client::new()
            .get(format!("http://{address}/models"))
            .send()
            .await
            .unwrap();
        let result = read_bounded_model_response(response).await;
        assert_eq!(result, Err("model_catalog_response_too_large".to_string()));
        server.abort();
    }
}
