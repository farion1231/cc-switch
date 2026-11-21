use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::State;

use crate::app_config::AppType;
use crate::database::Database;
use crate::error::AppError;
use crate::mode::current::{provider_for, Purpose};
use crate::provider::{Provider, ProviderMeta};
use crate::store::AppState;

const APP: AppType = AppType::QoderCnDesktop;
const GATEWAY_SETTING: &str = "qoder_cn_desktop_gateway";
const MODEL_ALIAS: &str = "cc-switch-qoder";

#[derive(Default, Deserialize, Serialize)]
struct Gateway {
    enabled: bool,
    token: String,
}

fn read_gateway(db: &Database) -> Result<Gateway, AppError> {
    db.get_setting(GATEWAY_SETTING)?
        .map(|value| {
            serde_json::from_str(&value).map_err(|error| AppError::json(GATEWAY_SETTING, error))
        })
        .transpose()
        .map(Option::unwrap_or_default)
}

fn write_gateway(db: &Database, gateway: &Gateway) -> Result<(), AppError> {
    let serialized =
        serde_json::to_string(gateway).map_err(|source| AppError::JsonSerialize { source })?;
    db.set_setting(GATEWAY_SETTING, &serialized)
}

pub(crate) fn disable_qoder_cn_desktop_gateway(db: &Database) -> Result<(), AppError> {
    let mut gateway = read_gateway(db)?;
    if gateway.enabled {
        gateway.enabled = false;
        write_gateway(db, &gateway)?;
    }
    Ok(())
}

pub(crate) fn qoder_cn_desktop_gateway_token(db: &Database) -> Result<Option<String>, AppError> {
    let gateway = read_gateway(db)?;
    Ok((gateway.enabled && !gateway.token.is_empty()).then_some(gateway.token))
}

pub(crate) async fn restore_qoder_cn_desktop_gateway(state: &AppState) -> Result<(), String> {
    if qoder_cn_desktop_gateway_token(&state.db)
        .map_err(|error| error.to_string())?
        .is_some()
    {
        state.proxy_service.start().await?;
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum Format {
    OpenaiResponses,
    OpenaiChat,
    Anthropic,
}

impl Format {
    fn from_str(value: &str) -> Option<Self> {
        match value {
            "openai_responses" => Some(Self::OpenaiResponses),
            "openai_chat" => Some(Self::OpenaiChat),
            "anthropic" => Some(Self::Anthropic),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::OpenaiResponses => "openai_responses",
            Self::OpenaiChat => "openai_chat",
            Self::Anthropic => "anthropic",
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProviderSummary {
    id: String,
    name: String,
    base_url: String,
    model: String,
    format: Format,
    anthropic_api_key: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderInput {
    id: Option<String>,
    name: String,
    base_url: String,
    model: String,
    format: Format,
    anthropic_api_key: bool,
    api_key: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QoderCnDesktopStatus {
    enabled: bool,
    running: bool,
    base_url: String,
    model: &'static str,
    gateway_token: String,
    current_provider_id: Option<String>,
    providers: Vec<ProviderSummary>,
    importable_providers: Vec<ProviderSummary>,
}

fn configured_string<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value
        .get(key)?
        .as_str()
        .map(str::trim)
        .filter(|v| !v.is_empty())
}

fn summarize(provider: &Provider) -> Option<ProviderSummary> {
    Some(ProviderSummary {
        id: provider.id.clone(),
        name: provider.name.clone(),
        base_url: configured_string(&provider.settings_config, "base_url")?.into(),
        model: configured_string(&provider.settings_config, "model")?.into(),
        format: Format::from_str(provider.meta.as_ref()?.api_format.as_deref()?)?,
        anthropic_api_key: provider.meta.as_ref()?.api_key_field.as_deref()
            == Some("ANTHROPIC_API_KEY"),
    })
}

fn importable(provider: &Provider) -> Option<(ProviderSummary, &str)> {
    if provider.has_official_category()
        || provider.is_codex_oauth()
        || provider.is_github_copilot()
        || provider.is_xai_oauth()
    {
        return None;
    }
    let env = provider.settings_config.get("env")?;
    let format = Format::from_str(crate::proxy::providers::get_claude_api_format(provider))?;
    let (api_key, anthropic_api_key) = configured_string(env, "ANTHROPIC_API_KEY")
        .map(|key| (key, true))
        .or_else(|| configured_string(env, "ANTHROPIC_AUTH_TOKEN").map(|key| (key, false)))?;
    if api_key == crate::live::project::claude::PROXY_TOKEN_PLACEHOLDER {
        return None;
    }
    let base_url = configured_string(env, "ANTHROPIC_BASE_URL")?;
    let model = [
        "ANTHROPIC_MODEL",
        "ANTHROPIC_DEFAULT_SONNET_MODEL",
        "ANTHROPIC_DEFAULT_OPUS_MODEL",
    ]
    .iter()
    .find_map(|key| configured_string(env, key))?;
    Some((
        ProviderSummary {
            id: provider.id.clone(),
            name: provider.name.clone(),
            base_url: base_url.into(),
            model: model.into(),
            format,
            anthropic_api_key,
        },
        api_key,
    ))
}

#[tauri::command]
pub async fn qoder_cn_desktop_status(
    state: State<'_, AppState>,
) -> Result<QoderCnDesktopStatus, AppError> {
    let gateway = read_gateway(&state.db)?;
    let proxy = state
        .proxy_service
        .get_status()
        .await
        .map_err(AppError::Message)?;
    let (origin, _) = state
        .proxy_service
        .build_proxy_urls()
        .await
        .map_err(AppError::Message)?;
    let sources = state.db.get_provider_rows(&AppType::Claude)?;
    Ok(QoderCnDesktopStatus {
        enabled: gateway.enabled,
        running: proxy.running && gateway.enabled,
        base_url: format!("{origin}/qoder-cn/v1"),
        model: MODEL_ALIAS,
        gateway_token: gateway.token,
        current_provider_id: provider_for(&state.db, &APP, Purpose::InUse)?,
        providers: state
            .db
            .get_provider_rows(&APP)?
            .readable
            .values()
            .filter_map(summarize)
            .collect(),
        importable_providers: sources
            .readable
            .values()
            .filter_map(|p| importable(p).map(|(s, _)| s))
            .collect(),
    })
}

#[tauri::command]
pub async fn qoder_cn_desktop_set_gateway(
    state: State<'_, AppState>,
    enabled: bool,
) -> Result<(), AppError> {
    let _lock = state.proxy_service.lock_switch_for_app(APP.as_str()).await;
    let mut gateway = read_gateway(&state.db)?;
    if enabled {
        if provider_for(&state.db, &APP, Purpose::InUse)?.is_none() {
            return Err(message("selectProvider"));
        }
        state
            .proxy_service
            .start()
            .await
            .map_err(AppError::Message)?;
        if gateway.token.is_empty() {
            gateway.token = format!("ccs-qoder-{}", uuid::Uuid::new_v4().simple());
        }
    }
    gateway.enabled = enabled;
    write_gateway(&state.db, &gateway)
}

fn message(key: &str) -> AppError {
    AppError::Message(format!("qoderCnDesktop.errors.{key}"))
}

fn save_provider(
    db: &Database,
    input: ProviderInput,
    existing: Option<Provider>,
) -> Result<(), AppError> {
    let name = input.name.trim();
    let model = input.model.trim();
    let base_url = input.base_url.trim().trim_end_matches('/');
    let url = url::Url::parse(base_url).map_err(|_| message("invalidUrl"))?;
    if !(url.scheme() == "https"
        || (url.scheme() == "http"
            && matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "[::1]"))))
        || url.host().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(message("unsupportedUrl"));
    }
    let api_key = match input.api_key.trim() {
        "" => existing.as_ref().and_then(|provider| {
            let old =
                url::Url::parse(configured_string(&provider.settings_config, "base_url")?).ok()?;
            if old.origin() != url.origin() {
                return None;
            }
            configured_string(&provider.settings_config, "apiKey")
        }),
        key => Some(key),
    };
    let api_key = api_key.ok_or_else(|| message("keyRequired"))?;
    if name.is_empty() || model.is_empty() {
        return Err(message("nameAndModelRequired"));
    }
    if axum::http::HeaderValue::from_str(api_key).is_err() {
        return Err(message("invalidKey"));
    }
    let settings = json!({ "base_url": base_url, "model": model, "apiKey": api_key });
    let mut provider = existing.unwrap_or_else(|| {
        Provider::with_id(
            input.id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
            name.into(),
            Value::Null,
            None,
        )
    });
    provider.name = name.into();
    provider.settings_config = settings;
    let meta = provider.meta.get_or_insert_with(ProviderMeta::default);
    meta.api_format = Some(input.format.as_str().into());
    meta.api_key_field = (input.format == Format::Anthropic && input.anthropic_api_key)
        .then(|| "ANTHROPIC_API_KEY".into());
    db.save_provider(&APP, &provider)
}

#[tauri::command]
pub async fn qoder_cn_desktop_save_provider(
    state: State<'_, AppState>,
    input: ProviderInput,
) -> Result<(), AppError> {
    let _lock = state.proxy_service.lock_switch_for_app(APP.as_str()).await;
    let existing = match input.id.as_ref() {
        Some(id) => Some(
            state
                .db
                .get_provider_by_id(id, &APP)?
                .ok_or_else(|| message("providerMissing"))?,
        ),
        None => None,
    };
    save_provider(&state.db, input, existing)
}

#[tauri::command]
pub async fn qoder_cn_desktop_import_provider(
    state: State<'_, AppState>,
    source_id: String,
) -> Result<(), AppError> {
    let _lock = state.proxy_service.lock_switch_for_app(APP.as_str()).await;
    let source = state
        .db
        .get_provider_by_id(&source_id, &AppType::Claude)?
        .ok_or_else(|| message("sourceMissing"))?;
    let (summary, api_key) = importable(&source).ok_or_else(|| message("sourceUnsupported"))?;
    let id = format!("claude:{source_id}");
    if state.db.provider_exists(&id, &APP)? {
        return Ok(());
    }
    save_provider(
        &state.db,
        ProviderInput {
            id: Some(id),
            name: summary.name,
            base_url: summary.base_url,
            model: summary.model,
            format: summary.format,
            anthropic_api_key: summary.anthropic_api_key,
            api_key: api_key.into(),
        },
        None,
    )
}

#[tauri::command]
pub async fn qoder_cn_desktop_select_provider(
    state: State<'_, AppState>,
    id: String,
) -> Result<(), AppError> {
    let _lock = state.proxy_service.lock_switch_for_app(APP.as_str()).await;
    let provider = state
        .db
        .get_provider_by_id(&id, &APP)?
        .ok_or_else(|| message("providerMissing"))?;
    if summarize(&provider).is_none() {
        return Err(message("providerIncomplete"));
    }
    state.db.set_current_provider(&APP, &id)
}

#[tauri::command]
pub async fn qoder_cn_desktop_delete_provider(
    state: State<'_, AppState>,
    id: String,
) -> Result<(), AppError> {
    let _lock = state.proxy_service.lock_switch_for_app(APP.as_str()).await;
    if read_gateway(&state.db)?.enabled
        && provider_for(&state.db, &APP, Purpose::InUse)?.as_deref() == Some(&id)
    {
        return Err(message("activeProvider"));
    }
    state.db.delete_provider(&APP, &id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proxy::{server::ProxyServer, types::ProxyConfig};
    use axum::{http::HeaderMap, routing::post, Json, Router};
    use reqwest::StatusCode;
    use std::{sync::Arc, time::Duration};

    #[tokio::test]
    async fn qoder_desktop_gateway_switches_protocol_model_and_credentials() {
        let upstream = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = upstream.local_addr().unwrap().port();
        let fixture = Router::new().route(
            "/a/v1/responses",
            post(|headers: HeaderMap, Json(body): Json<Value>| async move {
                assert_eq!(headers["authorization"], "Bearer upstream-a");
                assert_eq!(body["model"], "model-a");
                assert_eq!(body["input"][0]["content"], "Remember MAPLE.");
                Json(json!({"id":"resp_a","object":"response","model":"model-a","status":"completed",
                    "output":[{"type":"message","role":"assistant","content":[{"type":"output_text","text":"Remembered."}]}]}))
            }),
        ).route(
            "/b/v1/chat/completions",
            post(|headers: HeaderMap, Json(body): Json<Value>| async move {
                assert_eq!(headers["authorization"], "Bearer upstream-b");
                assert_eq!(body["model"], "model-b");
                assert_eq!(body["messages"][0]["content"], "Remember MAPLE.");
                assert_eq!(body["messages"][2]["content"], "Which word did I tell you?");
                Json(json!({"id":"chat_b","object":"chat.completion","model":"model-b",
                    "choices":[{"index":0,"finish_reason":"stop","message":{"role":"assistant","content":"MAPLE"}}]}))
            }),
        );
        let server = tokio::spawn(async move { axum::serve(upstream, fixture).await.unwrap() });

        let db = Arc::new(Database::memory().unwrap());
        write_gateway(
            &db,
            &Gateway {
                enabled: true,
                token: "local-gateway".into(),
            },
        )
        .unwrap();
        for (id, format) in [("a", Format::OpenaiResponses), ("b", Format::OpenaiChat)] {
            save_provider(
                &db,
                ProviderInput {
                    id: Some(id.into()),
                    name: id.into(),
                    base_url: format!("http://127.0.0.1:{port}/{id}/v1"),
                    model: format!("model-{id}"),
                    format,
                    anthropic_api_key: false,
                    api_key: format!("upstream-{id}"),
                },
                None,
            )
            .unwrap();
        }
        db.set_current_provider(&APP, "a").unwrap();
        let proxy = ProxyServer::new(
            ProxyConfig {
                listen_port: 0,
                ..ProxyConfig::default()
            },
            db.clone(),
            None,
        );
        let info = proxy.start().await.unwrap();
        let url = format!("http://127.0.0.1:{}/qoder-cn/v1/responses", info.port);
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap();
        let first_input = json!({"role":"user","content":"Remember MAPLE."});
        let first_body = json!({"model": MODEL_ALIAS, "input": [first_input.clone()]});

        let unauthenticated = client.post(&url).json(&first_body).send().await.unwrap();
        assert_eq!(unauthenticated.status(), StatusCode::UNAUTHORIZED);
        let first: Value = client
            .post(&url)
            .bearer_auth("local-gateway")
            .json(&first_body)
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(first["output"][0]["content"][0]["text"], "Remembered.");

        db.set_current_provider(&APP, "b").unwrap();
        let second_body = json!({"model":MODEL_ALIAS,"input":[first_input,
            {"role":"assistant", "content":"Remembered."},
            {"role":"user", "content":"Which word did I tell you?"} ]});
        let second: Value = client
            .post(&url)
            .bearer_auth("local-gateway")
            .json(&second_body)
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(second["output"][0]["content"][0]["text"], "MAPLE");

        disable_qoder_cn_desktop_gateway(&db).unwrap();
        let disabled = client
            .post(&url)
            .bearer_auth("local-gateway")
            .json(&second_body)
            .send()
            .await
            .unwrap();
        assert_eq!(disabled.status(), StatusCode::UNAUTHORIZED);

        proxy.stop().await.unwrap();
        server.abort();
    }
}
