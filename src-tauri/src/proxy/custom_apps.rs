//! Local OpenAI Chat Completions routes for user-defined app profiles.

use crate::{database::Database, error::AppError, provider::Provider, store::AppState};
use axum::{
    body::Body,
    extract::{Path, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use url::Url;
use uuid::Uuid;

use super::{http_client, privacy_filter, server::ProxyState, upstream_guard};

const PROFILE_KEY: &str = "custom_app_profiles";

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CustomApp {
    id: String,
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    token: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct StoredApp {
    id: String,
    name: String,
    token_hash: [u8; 32],
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CustomProvider {
    name: String,
    base_url: String,
    model: String,
    #[serde(default, skip_serializing)]
    api_key: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProviderList {
    providers: Vec<ProviderCard>,
    selected: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProviderCard {
    id: String,
    name: String,
    model: String,
}

fn profiles(db: &Database) -> Result<Vec<StoredApp>, AppError> {
    db.get_setting(PROFILE_KEY)?
        .map(|data| serde_json::from_str(&data))
        .transpose()
        .map(|data| data.unwrap_or_default())
        .map_err(|_| AppError::Config("Could not read custom app profiles.".into()))
}

fn app_namespace(id: &str) -> String {
    format!("custom:{id}")
}

fn create_app(db: &Database, name: String) -> Result<CustomApp, AppError> {
    let name = name.trim().to_string();
    if name.is_empty() || name.chars().count() > 80 {
        return Err(AppError::Config("Enter a name of 1–80 characters.".into()));
    }
    let id = Uuid::new_v4().simple().to_string();
    let token = format!("ccs-{}", Uuid::new_v4().simple());
    // Keep profile creation atomic across concurrent invocations.
    db.update_json_setting::<Vec<StoredApp>>(PROFILE_KEY, |profiles| {
        profiles.push(StoredApp {
            id: id.clone(),
            name: name.clone(),
            token_hash: Sha256::digest(token.as_bytes()).into(),
        });
        Ok(())
    })?;
    Ok(CustomApp {
        id,
        name,
        token: Some(token),
    })
}

fn provider_endpoint(base_url: &str) -> Result<Url, &'static str> {
    let mut url = Url::parse(base_url.trim()).map_err(|_| "Enter a valid provider base URL.")?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("Use an HTTP or HTTPS base URL without credentials, a query, or a fragment.");
    }
    url.path_segments_mut()
        .map_err(|_| "Enter a valid provider base URL.")?
        .pop_if_empty()
        .push("chat")
        .push("completions");
    Ok(url)
}

fn load_provider(provider: Provider) -> Result<CustomProvider, AppError> {
    serde_json::from_value(provider.settings_config)
        .map_err(|_| AppError::Config("Invalid custom provider configuration.".into()))
}

fn require_app(db: &Database, id: &str) -> Result<String, AppError> {
    profiles(db)?
        .iter()
        .any(|profile| profile.id == id)
        .then(|| app_namespace(id))
        .ok_or_else(|| AppError::Config("Custom app profile was not found.".into()))
}

fn add_provider(db: &Database, app_id: &str, provider: CustomProvider) -> Result<String, AppError> {
    let namespace = require_app(db, app_id)?;
    provider_endpoint(&provider.base_url).map_err(|error| AppError::Config(error.into()))?;
    let name = provider.name.trim();
    let model = provider.model.trim();
    if name.is_empty() || model.is_empty() {
        return Err(AppError::Config("Enter a provider name and model.".into()));
    }
    let id = Uuid::new_v4().simple().to_string();
    let row = Provider::with_id(
        id.clone(),
        name.to_string(),
        json!({
            "name": name,
            "baseUrl": provider.base_url.trim(),
            "model": model,
            "apiKey": provider.api_key.trim(),
        }),
        None,
    );
    db.save_provider(&namespace, &row)?;
    Ok(id)
}

fn select_provider(db: &Database, app_id: &str, provider_id: &str) -> Result<(), AppError> {
    let namespace = require_app(db, app_id)?;
    if db.get_provider_by_id(provider_id, &namespace)?.is_none() {
        return Err(AppError::Config(
            "Provider was not found in this profile.".into(),
        ));
    }
    db.set_current_provider(&namespace, provider_id)
}

#[tauri::command]
pub(crate) fn list_custom_apps(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<CustomApp>, String> {
    let apps = profiles(&state.db).map_err(|error| error.to_string())?;
    Ok(apps
        .into_iter()
        .map(|app| CustomApp {
            id: app.id,
            name: app.name,
            token: None,
        })
        .collect())
}

#[tauri::command]
pub(crate) fn create_custom_app(
    state: tauri::State<'_, AppState>,
    name: String,
) -> Result<CustomApp, String> {
    create_app(&state.db, name).map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) fn rotate_custom_app_token(
    state: tauri::State<'_, AppState>,
    app_id: String,
) -> Result<String, String> {
    let token = format!("ccs-{}", Uuid::new_v4().simple());
    state
        .db
        .update_json_setting::<Vec<StoredApp>>(PROFILE_KEY, |profiles| {
            let profile = profiles
                .iter_mut()
                .find(|profile| profile.id == app_id)
                .ok_or_else(|| AppError::Config("Custom app profile was not found.".into()))?;
            profile.token_hash = Sha256::digest(token.as_bytes()).into();
            Ok(())
        })
        .map_err(|error| error.to_string())?;
    Ok(token)
}

#[tauri::command]
pub(crate) fn list_custom_providers(
    state: tauri::State<'_, AppState>,
    app_id: String,
) -> Result<ProviderList, String> {
    let read = || -> Result<ProviderList, AppError> {
        let namespace = require_app(&state.db, &app_id)?;
        let selected = state.db.get_current_provider(&namespace)?;
        let providers = state
            .db
            .get_all_providers(&namespace)?
            .into_iter()
            .map(|(id, row)| {
                let provider = load_provider(row)?;
                Ok(ProviderCard {
                    id,
                    name: provider.name,
                    model: provider.model,
                })
            })
            .collect::<Result<_, AppError>>()?;
        Ok(ProviderList {
            providers,
            selected,
        })
    };
    read().map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) fn add_custom_provider(
    state: tauri::State<'_, AppState>,
    app_id: String,
    provider: CustomProvider,
) -> Result<(), String> {
    add_provider(&state.db, &app_id, provider)
        .map(|_| ())
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) fn select_custom_provider(
    state: tauri::State<'_, AppState>,
    app_id: String,
    provider_id: String,
) -> Result<(), String> {
    select_provider(&state.db, &app_id, &provider_id).map_err(|error| error.to_string())
}

fn route_error(status: StatusCode, message: &'static str) -> Response {
    (
        status,
        Json(json!({"error": {"type": "custom_app_error", "message": message}})),
    )
        .into_response()
}

pub(super) async fn chat(
    State(state): State<ProxyState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(mut body): Json<Value>,
) -> Response {
    let route = forward(&state, &id, &headers, &mut body).await;
    match route {
        Ok(upstream) => {
            if upstream.status().is_redirection() {
                return route_error(StatusCode::BAD_GATEWAY, "The provider returned a redirect.");
            }
            let mut builder = Response::builder().status(upstream.status());
            for name in [
                header::CONTENT_TYPE,
                header::CONTENT_ENCODING,
                header::RETRY_AFTER,
            ] {
                if let Some(value) = upstream.headers().get(&name) {
                    builder = builder.header(name, value);
                }
            }
            builder
                .body(Body::from_stream(upstream.bytes_stream()))
                .unwrap_or_else(|_| {
                    route_error(StatusCode::BAD_GATEWAY, "Invalid upstream response.")
                })
        }
        Err((status, message)) => route_error(status, message),
    }
}

type RouteError = (StatusCode, &'static str);
const STORAGE_ERROR: RouteError = (
    StatusCode::INTERNAL_SERVER_ERROR,
    "Could not read custom app configuration.",
);

async fn forward(
    state: &ProxyState,
    id: &str,
    headers: &HeaderMap,
    body: &mut Value,
) -> Result<reqwest::Response, RouteError> {
    let status = state.status.read().await;
    let is_loopback = status
        .address
        .trim_matches(['[', ']'])
        .parse::<std::net::IpAddr>()
        .is_ok_and(|address| address.is_loopback());
    if !is_loopback {
        return Err((
            StatusCode::FORBIDDEN,
            "Custom app routes require a loopback listener.",
        ));
    }
    let (listen_address, listen_port) = (status.address.clone(), status.port);
    drop(status);
    let app = profiles(&state.db)
        .map_err(|_| STORAGE_ERROR)?
        .into_iter()
        .find(|app| app.id == id)
        .ok_or((StatusCode::NOT_FOUND, "Unknown custom app profile."))?;
    let token = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .ok_or((StatusCode::UNAUTHORIZED, "A custom app token is required."))?;
    let token_hash: [u8; 32] = Sha256::digest(token.as_bytes()).into();
    if !bool::from(app.token_hash.ct_eq(&token_hash)) {
        return Err((StatusCode::UNAUTHORIZED, "Invalid custom app token."));
    }
    let namespace = app_namespace(id);
    let missing_provider = (
        StatusCode::SERVICE_UNAVAILABLE,
        "Select a provider for this custom app.",
    );
    let provider_id = state
        .db
        .get_current_provider(&namespace)
        .map_err(|_| STORAGE_ERROR)?
        .ok_or(missing_provider)?;
    let row = state
        .db
        .get_provider_by_id(&provider_id, &namespace)
        .map_err(|_| STORAGE_ERROR)?
        .ok_or(missing_provider)?;
    let provider = load_provider(row).map_err(|_| STORAGE_ERROR)?;
    let endpoint = provider_endpoint(&provider.base_url)
        .map_err(|message| (StatusCode::BAD_GATEWAY, message))?;
    upstream_guard::reject_self_forwarding(endpoint.as_str(), &listen_address, listen_port)
        .await
        .map_err(|_| {
            (
                StatusCode::BAD_GATEWAY,
                "The provider URL points back to CC Switch.",
            )
        })?;
    let object = body
        .as_object_mut()
        .ok_or((StatusCode::BAD_REQUEST, "Expected a JSON object."))?;
    object.insert("model".into(), json!(provider.model));

    let privacy = state
        .db
        .get_privacy_filter_config()
        .map_err(|_| STORAGE_ERROR)?;
    if privacy.enabled {
        privacy_filter::redact_request_body(
            body,
            privacy_filter::Placeholders::for_language(&crate::tray::tray_language()),
            privacy.filter_tool_results,
        );
    }
    let client = http_client::no_redirect_for_url(endpoint.as_str()).map_err(|_| {
        (
            StatusCode::BAD_GATEWAY,
            "Could not configure the upstream connection.",
        )
    })?;
    let mut request = client.post(endpoint).json(body);
    if !provider.api_key.is_empty() {
        request = request.bearer_auth(provider.api_key);
    }
    request
        .send()
        .await
        .map_err(|_| (StatusCode::BAD_GATEWAY, "Could not reach the provider."))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proxy::{server::ProxyServer, ProxyConfig};
    use axum::{routing::post, Router};
    use std::sync::Arc;

    #[tokio::test]
    async fn custom_profiles_switch_independently_over_http() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base_url = format!("http://{}/v1", listener.local_addr().unwrap());
        let finish_stream = Arc::new(tokio::sync::Notify::new());
        let upstream_finish = finish_stream.clone();
        let upstream = tokio::spawn(async move {
            let routes = Router::new().route(
                "/v1/chat/completions",
                post(move |headers: HeaderMap, Json(body): Json<Value>| {
                    let upstream_finish = upstream_finish.clone();
                    async move {
                    let model = body["model"].as_str().unwrap();
                    assert_eq!(
                        headers.get(header::AUTHORIZATION).unwrap(),
                        &format!("Bearer key-{model}")
                    );
                    if body["stream"] == true {
                        let chunk = json!({"model": model});
                        let stream = async_stream::stream! {
                            yield Ok::<_, std::convert::Infallible>(format!("data: {chunk}\n\n"));
                            upstream_finish.notified().await;
                            yield Ok(format!("data: {chunk}\n\ndata: [DONE]\n\n"));
                        };
                        return (
                            [(header::CONTENT_TYPE, "text/event-stream")],
                            Body::from_stream(stream),
                        )
                            .into_response();
                    }
                    Json(json!({"model": model})).into_response()
                    }
                }),
            );
            axum::serve(listener, routes).await.unwrap();
        });
        let db = Arc::new(Database::memory().unwrap());
        let first = create_app(&db, "Meeting summaries".into()).unwrap();
        let second = create_app(&db, "Local notes".into()).unwrap();
        let add = |app: &CustomApp, model: &str| {
            add_provider(
                &db,
                &app.id,
                CustomProvider {
                    name: model.into(),
                    base_url: base_url.clone(),
                    model: model.into(),
                    api_key: format!("key-{model}"),
                },
            )
            .unwrap()
        };
        let a = add(&first, "a");
        let b = add(&first, "b");
        let c = add(&second, "c");
        assert!(select_provider(&db, &first.id, &c).is_err());
        select_provider(&db, &first.id, &a).unwrap();
        let config = ProxyConfig {
            listen_port: 0,
            ..Default::default()
        };
        let server = ProxyServer::new(config.clone(), db.clone(), None);
        let port = server.start().await.unwrap().port;
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .unwrap();
        let send = |app: &CustomApp, token: &str, stream: bool| {
            client
                .post(format!(
                    "http://127.0.0.1:{port}/apps/{}/v1/chat/completions",
                    app.id
                ))
                .bearer_auth(token)
                .json(&json!({"model": "fixed-client-alias", "stream": stream}))
                .send()
        };
        let first_token = first.token.as_deref().unwrap();
        let second_token = second.token.as_deref().unwrap();
        assert_eq!(
            send(&first, second_token, false).await.unwrap().status(),
            401
        );
        assert_eq!(
            send(&second, second_token, false).await.unwrap().status(),
            503
        );
        select_provider(&db, &second.id, &c).unwrap();

        let mut streaming = send(&first, first_token, true).await.unwrap();
        assert_eq!(
            streaming.headers()[header::CONTENT_TYPE],
            "text/event-stream"
        );
        let chunk = streaming.chunk().await.unwrap().unwrap();
        assert!(std::str::from_utf8(&chunk)
            .unwrap()
            .contains("\"model\":\"a\""));
        select_provider(&db, &first.id, &b).unwrap();
        let switched: Value = send(&first, first_token, false)
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let independent: Value = send(&second, second_token, false)
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(switched["model"], "b");
        assert_eq!(independent["model"], "c");
        finish_stream.notify_one();
        let rest = streaming.text().await.unwrap();
        assert!(rest.contains("\"model\":\"a\"") && rest.contains("[DONE]"));

        server.stop().await.unwrap();
        let restarted = ProxyServer::new(config, db.clone(), None);
        let port = restarted.start().await.unwrap().port;
        let response: Value = client
            .post(format!(
                "http://127.0.0.1:{port}/apps/{}/v1/chat/completions",
                first.id
            ))
            .bearer_auth(first_token)
            .json(&json!({"model":"fixed-client-alias"}))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(response["model"], "b");
        restarted.stop().await.unwrap();
        upstream.abort();
    }
}
