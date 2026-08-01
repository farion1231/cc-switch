//! Transparent Codex Live Voice routing.
//!
//! Live uses two related transports: a multipart HTTP request creates the
//! WebRTC call, then a websocket sideband attaches to the returned call id.
//! Both legs must use the same provider and must not affect text circuit-breaker
//! accounting.

use super::live_attestation;
use super::providers::{get_adapter, ProviderAdapter};
use super::server::ProxyState;
use super::ProxyError;
use crate::app_config::AppType;
use crate::commands::CodexOAuthState;
use crate::database::CODEX_OFFICIAL_PROVIDER_ID;
use crate::provider::Provider;
use crate::settings::CodexLiveVoiceRoute;
use axum::body::Body;
use axum::extract::ws::{
    CloseFrame as ClientCloseFrame, Message as ClientMessage, WebSocket, WebSocketUpgrade,
};
use axum::extract::{Path, State};
use axum::http::{HeaderMap, HeaderName, HeaderValue, Response, StatusCode, Uri};
use axum::response::IntoResponse;
use base64::Engine;
use futures::{SinkExt, StreamExt};
use http_body_util::{BodyExt, Limited};
use std::collections::{HashMap, HashSet};
use std::net::IpAddr;
use std::time::{Duration, Instant};
use tauri::Manager;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message as UpstreamMessage;

const LIVE_CALL_BINDING_TTL: Duration = Duration::from_secs(10 * 60);
const OFFICIAL_LIVE_CALL_ENDPOINT: &str = "realtime/calls";
const OFFICIAL_LIVE_SIDEBAND_ENDPOINT: &str = "realtime/calls/{call_id}";
const DEFAULT_THIRD_PARTY_LIVE_CALL_ENDPOINT: &str = "live";
const DEFAULT_THIRD_PARTY_LIVE_SIDEBAND_ENDPOINT: &str = "live/{call_id}";
const LIVE_MAX_REQUEST_BYTES: usize = 2 * 1024 * 1024;
const LIVE_MAX_RESPONSE_BYTES: usize = 2 * 1024 * 1024;
const LIVE_MAX_BINDINGS: usize = 128;
const LIVE_MAX_MESSAGE_BYTES: usize = 8 * 1024 * 1024;
const LIVE_MAX_FRAME_BYTES: usize = 2 * 1024 * 1024;
const LIVE_MAX_PROTOCOLS: usize = 16;
const LIVE_MAX_PROTOCOL_BYTES: usize = 128;
const LIVE_CREATE_TIMEOUT: Duration = Duration::from_secs(120);
const LIVE_CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
const LIVE_WRITE_TIMEOUT: Duration = Duration::from_secs(20);
const LIVE_IDLE_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const LIVE_CLOSE_TIMEOUT: Duration = Duration::from_secs(2);
const LIVE_INTERNAL_ERROR_CODE: u16 = 1011;
const LIVE_IDLE_CLOSE_CODE: u16 = 1001;
pub(crate) const LIVE_MAX_ACTIVE_SIDEBANDS: usize = 16;
const LIVE_DISABLED_BODY: &str = r#"{"error":{"code":"codex_live_voice_disabled","message":"Codex Live voice is disabled in CC Switch settings","type":"feature_disabled"}}"#;

#[derive(Clone)]
pub(crate) struct LiveCallBinding {
    route: LiveProviderRoute,
    attestation: Option<http::HeaderValue>,
    created_at: Instant,
    generation: uuid::Uuid,
    phase: LiveCallBindingPhase,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum LiveCallBindingPhase {
    Available,
    Connecting(uuid::Uuid),
}

#[derive(Clone, Copy)]
struct LiveCallReservation {
    generation: uuid::Uuid,
    attempt: uuid::Uuid,
}

#[derive(Clone)]
struct LiveProviderRoute {
    provider: Provider,
    create_endpoint: String,
    sideband_endpoint: String,
    official: bool,
}

struct LiveCreateFailure {
    error: ProxyError,
    may_have_created_call: bool,
}

impl LiveCreateFailure {
    fn preflight(error: ProxyError) -> Self {
        Self {
            error,
            may_have_created_call: false,
        }
    }

    fn transport(error: reqwest::Error, provider_name: &str, initial_url: &str) -> Self {
        Self {
            may_have_created_call: !connect_failure_before_initial_send(
                error.is_connect(),
                error.url(),
                initial_url,
            ),
            error: ProxyError::ForwardFailed(format!(
                "Live call creation failed via {provider_name}: {error}"
            )),
        }
    }
}

pub async fn handle_live_call(
    State(state): State<ProxyState>,
    request: axum::extract::Request,
) -> Result<Response<Body>, ProxyError> {
    if !crate::settings::codex_live_voice_enabled() {
        log::info!("[Codex Live] Rejected call creation because Live voice is disabled");
        return Ok(live_disabled_response());
    }
    if !live_listener_is_loopback(&state).await {
        log::warn!("[Codex Live] Rejected call creation on a non-loopback listener");
        return Ok(live_non_loopback_response());
    }
    if let Some(proxy_url) = super::http_client::get_current_proxy_url() {
        log::warn!(
            "[Codex Live] Rejected call creation because websocket proxying is unavailable for the configured global proxy ({})",
            super::http_client::mask_url(&proxy_url)
        );
        return Ok(live_outbound_proxy_unsupported_response());
    }

    let (parts, body) = request.into_parts();
    if let Err(reason) = validate_live_client_headers(&parts.headers, true) {
        log::warn!("[Codex Live] Rejected call creation: {reason}");
        return Ok(live_client_rejected_response());
    }
    let adapter = get_adapter(&AppType::Codex);
    let body = Limited::new(body, LIVE_MAX_REQUEST_BYTES)
        .collect()
        .await
        .map_err(|error| {
            ProxyError::ConfigError(format!(
                "Live request exceeds the {LIVE_MAX_REQUEST_BYTES}-byte limit or could not be read: {error}"
            ))
        })?
        .to_bytes();
    let original_headers = parts.headers;

    let selected = match crate::settings::codex_live_voice_route() {
        CodexLiveVoiceRoute::Official => {
            let route = select_official_live_route(&state).await?;
            create_live_call(
                &state,
                adapter.as_ref(),
                route,
                &parts.uri,
                &original_headers,
                &body,
            )
            .await
            .map_err(|failure| failure.error)?
        }
        CodexLiveVoiceRoute::CurrentProvider => {
            let route = select_current_live_route(&state).await?;
            create_live_call(
                &state,
                adapter.as_ref(),
                route,
                &parts.uri,
                &original_headers,
                &body,
            )
            .await
            .map_err(|failure| failure.error)?
        }
        CodexLiveVoiceRoute::OfficialThenCurrent => {
            let official = match select_official_live_route(&state).await {
                Ok(route) => {
                    create_live_call(
                        &state,
                        adapter.as_ref(),
                        route,
                        &parts.uri,
                        &original_headers,
                        &body,
                    )
                    .await
                }
                Err(error) => Err(LiveCreateFailure::preflight(error)),
            };

            match official {
                Ok(result) if !should_fallback_from_official(result.1.status()) => result,
                Ok(official_result) => {
                    log::warn!(
                        "[Codex Live] Official call returned {}; trying the explicitly Live-capable current provider",
                        official_result.1.status()
                    );
                    match select_current_live_route(&state).await {
                        Ok(route) if route.provider.id != official_result.0.provider.id => {
                            match create_live_call(
                                &state,
                                adapter.as_ref(),
                                route,
                                &parts.uri,
                                &original_headers,
                                &body,
                            )
                            .await
                            {
                                Ok(current_result) => current_result,
                                Err(failure) if failure.may_have_created_call => {
                                    return Err(ProxyError::ForwardFailed(format!(
                                        "current-provider Live fallback may have created a call; refusing another retry: {}",
                                        failure.error
                                    )));
                                }
                                Err(failure) => {
                                    log::warn!(
                                        "[Codex Live] Current-provider fallback failed ({}); returning the official response",
                                        failure.error
                                    );
                                    official_result
                                }
                            }
                        }
                        Ok(_) => official_result,
                        Err(error) => {
                            log::warn!(
                                "[Codex Live] No eligible current-provider fallback ({error}); returning the official response"
                            );
                            official_result
                        }
                    }
                }
                Err(failure) if failure.may_have_created_call => return Err(failure.error),
                Err(failure) => {
                    let official_error = failure.error;
                    log::warn!(
                        "[Codex Live] Official call unavailable ({official_error}); trying the explicitly Live-capable current provider"
                    );
                    let route = select_current_live_route(&state).await.map_err(|current_error| {
                        ProxyError::ConfigError(format!(
                            "official Live route failed ({official_error}); current-provider route is unavailable ({current_error})"
                        ))
                    })?;
                    create_live_call(
                        &state,
                        adapter.as_ref(),
                        route,
                        &parts.uri,
                        &original_headers,
                        &body,
                    )
                    .await
                    .map_err(|failure| failure.error)?
                }
            }
        }
    };
    let (route, upstream, attestation) = selected;

    let status = upstream.status();
    let mut response_headers = upstream.headers().clone();
    if status.is_success() {
        let live_location = response_headers
            .get(http::header::LOCATION)
            .and_then(|value| value.to_str().ok())
            .and_then(parse_live_location);
        let Some((call_id, location_query)) = live_location else {
            log::warn!(
                "[Codex Live] Upstream {} returned success without a usable Location call id",
                route.provider.name
            );
            return Ok(live_invalid_upstream_response());
        };

        let mut calls = state.live_calls.write().await;
        calls.retain(|_, binding| binding.created_at.elapsed() < LIVE_CALL_BINDING_TTL);
        if calls.len() >= LIVE_MAX_BINDINGS {
            if let Some(oldest) = calls
                .iter()
                .min_by_key(|(_, binding)| binding.created_at)
                .map(|(call_id, _)| call_id.clone())
            {
                calls.remove(&oldest);
            }
        }
        calls.insert(
            call_id.clone(),
            LiveCallBinding {
                route: route.clone(),
                attestation,
                created_at: Instant::now(),
                generation: uuid::Uuid::new_v4(),
                phase: LiveCallBindingPhase::Available,
            },
        );
        drop(calls);

        let local_location =
            local_sideband_location(parts.uri.path(), &call_id, location_query.as_deref())?;
        response_headers.insert(http::header::LOCATION, local_location);
        log::info!(
            "[Codex Live] Bound call {call_id} to provider {}",
            route.provider.name
        );
    }
    let response_body = read_limited_live_response(upstream).await?;

    let mut response = Response::builder().status(status);
    for (name, value) in &response_headers {
        if !is_hop_by_hop(name) && name != http::header::CONTENT_LENGTH {
            response = response.header(name, value);
        }
    }
    response
        .body(Body::from(response_body))
        .map_err(|error| ProxyError::Internal(format!("failed to build Live response: {error}")))
}

async fn create_live_call(
    state: &ProxyState,
    adapter: &dyn ProviderAdapter,
    route: LiveProviderRoute,
    client_uri: &Uri,
    original_headers: &HeaderMap,
    original_body: &bytes::Bytes,
) -> Result<(LiveProviderRoute, reqwest::Response, Option<HeaderValue>), LiveCreateFailure> {
    let mut headers = original_headers.clone();
    let attestation = if route.official {
        let supplied_bytes = header_len(&headers, live_attestation::HEADER_NAME);
        let supplied_was_usable = headers
            .get(live_attestation::HEADER_NAME)
            .is_some_and(live_attestation::is_usable_value);
        let value = live_attestation::ensure(&mut headers)
            .await
            .map_err(|error| {
                LiveCreateFailure::preflight(ProxyError::ConfigError(format!(
                    "failed to prepare Codex Live attestation: {error}"
                )))
            })?;
        if !supplied_was_usable {
            log::info!(
                "[Codex Live] Replaced an unusable client attestation with a verified local proof (client_bytes={supplied_bytes}, generated_bytes={})",
                value.as_bytes().len()
            );
        }
        Some(value)
    } else {
        headers.remove(live_attestation::HEADER_NAME);
        None
    };

    let upstream_url =
        live_upstream_url(adapter, &route.provider, &route.create_endpoint, client_uri)
            .map_err(LiveCreateFailure::preflight)?;
    let headers = upstream_headers(headers, adapter, &route.provider, route.official, state)
        .await
        .map_err(LiveCreateFailure::preflight)?;
    let (headers, body, converted) =
        prepare_live_call_request(route.official, headers, original_body.clone())
            .await
            .map_err(LiveCreateFailure::preflight)?;
    log::info!(
        "[Codex Live] Creating call via {} (body_bytes={}, backend_json={}, attestation={}, content_type_bytes={})",
        route.provider.name,
        body.len(),
        converted,
        headers.contains_key(live_attestation::HEADER_NAME),
        header_len(&headers, http::header::CONTENT_TYPE.as_str())
    );

    let upstream = super::http_client::get()
        .post(upstream_url.as_str())
        .headers(headers)
        .body(body)
        .timeout(LIVE_CREATE_TIMEOUT)
        .send()
        .await
        .map_err(|error| {
            LiveCreateFailure::transport(error, &route.provider.name, &upstream_url)
        })?;
    Ok((route, upstream, attestation))
}

fn connect_failure_before_initial_send(
    is_connect_error: bool,
    error_url: Option<&url::Url>,
    initial_url: &str,
) -> bool {
    // A connect error after an automatic redirect is not known to be side
    // effect free: the initial POST already reached an upstream server.
    is_connect_error && error_url.is_some_and(|url| url.as_str() == initial_url)
}

async fn read_limited_live_response(
    response: reqwest::Response,
) -> Result<bytes::Bytes, ProxyError> {
    let mut stream = response.bytes_stream();
    let mut body = bytes::BytesMut::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|error| {
            ProxyError::ForwardFailed(format!("failed to read Live response: {error}"))
        })?;
        if body.len().saturating_add(chunk.len()) > LIVE_MAX_RESPONSE_BYTES {
            return Err(ProxyError::ForwardFailed(format!(
                "Live response exceeded the {LIVE_MAX_RESPONSE_BYTES}-byte limit"
            )));
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body.freeze())
}

fn should_fallback_from_official(status: http::StatusCode) -> bool {
    matches!(
        status,
        http::StatusCode::UNAUTHORIZED
            | http::StatusCode::FORBIDDEN
            | http::StatusCode::TOO_MANY_REQUESTS
    )
}

fn header_len(headers: &HeaderMap, name: &str) -> usize {
    headers.get_all(name).iter().map(|value| value.len()).sum()
}

async fn normalize_live_call_request(
    mut headers: HeaderMap,
    body: bytes::Bytes,
) -> Result<(HeaderMap, bytes::Bytes, bool), ProxyError> {
    let Some(content_type) = headers
        .get(http::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
    else {
        return Ok((headers, body, false));
    };
    if !content_type
        .to_ascii_lowercase()
        .starts_with("multipart/form-data")
    {
        return Ok((headers, body, false));
    }

    let boundary = multer::parse_boundary(content_type).map_err(|error| {
        ProxyError::ConfigError(format!("invalid Live multipart boundary: {error}"))
    })?;
    let stream =
        futures::stream::once(async move { Ok::<bytes::Bytes, std::convert::Infallible>(body) });
    let mut multipart = multer::Multipart::new(stream, boundary);
    let mut sdp = None;
    let mut session = None;
    while let Some(field) = multipart.next_field().await.map_err(|error| {
        ProxyError::ConfigError(format!("failed to parse Live multipart body: {error}"))
    })? {
        let name = field.name().map(str::to_string);
        let data = field.bytes().await.map_err(|error| {
            ProxyError::ConfigError(format!("failed to read Live multipart field: {error}"))
        })?;
        match name.as_deref() {
            Some("sdp") => {
                sdp = Some(String::from_utf8(data.to_vec()).map_err(|error| {
                    ProxyError::ConfigError(format!("Live SDP is not UTF-8: {error}"))
                })?);
            }
            Some("session") => {
                session = Some(serde_json::from_slice::<serde_json::Value>(&data).map_err(
                    |error| {
                        ProxyError::ConfigError(format!(
                            "Live session field is not valid JSON: {error}"
                        ))
                    },
                )?);
            }
            _ => {}
        }
    }

    let sdp = sdp.ok_or_else(|| {
        ProxyError::ConfigError("Live multipart body is missing the sdp field".to_string())
    })?;
    let session = session.ok_or_else(|| {
        ProxyError::ConfigError("Live multipart body is missing the session field".to_string())
    })?;
    let body = serde_json::to_vec(&serde_json::json!({
        "sdp": sdp,
        "session": session,
    }))
    .map_err(|error| {
        ProxyError::Internal(format!("failed to encode ChatGPT Live request: {error}"))
    })?;
    headers.insert(
        http::header::CONTENT_TYPE,
        http::HeaderValue::from_static("application/json"),
    );
    headers.remove(http::header::CONTENT_LENGTH);
    Ok((headers, bytes::Bytes::from(body), true))
}

async fn prepare_live_call_request(
    official: bool,
    headers: HeaderMap,
    body: bytes::Bytes,
) -> Result<(HeaderMap, bytes::Bytes, bool), ProxyError> {
    if official {
        normalize_live_call_request(headers, body).await
    } else {
        Ok((headers, body, false))
    }
}

pub async fn handle_live_sideband(
    State(state): State<ProxyState>,
    Path(call_id): Path<String>,
    uri: Uri,
    headers: HeaderMap,
    upgrade: WebSocketUpgrade,
) -> Result<Response<Body>, ProxyError> {
    if !crate::settings::codex_live_voice_enabled() {
        log::info!(
            "[Codex Live] Rejected sideband for call {call_id} because Live voice is disabled"
        );
        return Ok(live_disabled_response());
    }
    if !live_listener_is_loopback(&state).await {
        log::warn!("[Codex Live] Rejected sideband for call {call_id} on a non-loopback listener");
        return Ok(live_non_loopback_response());
    }
    if let Err(reason) = validate_live_client_headers(&headers, false) {
        log::warn!("[Codex Live] Rejected sideband for call {call_id}: {reason}");
        return Ok(live_client_rejected_response());
    }
    if !is_valid_call_id(&call_id) {
        return Err(ProxyError::ConfigError(
            "invalid Live sideband call id".to_string(),
        ));
    }

    let requested_protocols = parse_websocket_protocols(&headers)?;
    let slot_permit = match state.live_sideband_slots.clone().try_acquire_owned() {
        Ok(permit) => permit,
        Err(_) => {
            log::warn!(
                "[Codex Live] Rejected sideband for call {call_id}: active connection limit reached"
            );
            return Ok(live_capacity_response());
        }
    };
    let (binding, reservation) = {
        let mut calls = state.live_calls.write().await;
        reserve_live_call_binding(&mut calls, &call_id)?
    };
    let setup_result = async {
        let route = binding.route.clone();
        let provider = route.provider.clone();
        let adapter = get_adapter(&AppType::Codex);
        let endpoint = render_sideband_endpoint(&route.sideband_endpoint, &call_id)?;
        let upstream_http_url = live_upstream_url(adapter.as_ref(), &provider, &endpoint, &uri)?;
        let upstream_ws_url = websocket_url(&upstream_http_url)?;
        let mut outbound_headers =
            upstream_headers(headers, adapter.as_ref(), &provider, route.official, &state).await?;
        if let Some(attestation) = binding.attestation.clone() {
            outbound_headers.insert(live_attestation::HEADER_NAME, attestation);
        } else {
            outbound_headers.remove(live_attestation::HEADER_NAME);
        }
        let mut upstream_request =
            upstream_ws_url
                .as_str()
                .into_client_request()
                .map_err(|error| {
                    ProxyError::ConfigError(format!("invalid Live websocket URL: {error}"))
                })?;
        for (name, value) in &outbound_headers {
            if !is_websocket_handshake_header(name) {
                upstream_request
                    .headers_mut()
                    .insert(name.clone(), value.clone());
            }
        }
        install_websocket_protocol_header(&requested_protocols, upstream_request.headers_mut())?;

        let websocket_config = tokio_tungstenite::tungstenite::protocol::WebSocketConfig {
            max_write_buffer_size: LIVE_MAX_MESSAGE_BYTES * 2,
            max_message_size: Some(LIVE_MAX_MESSAGE_BYTES),
            max_frame_size: Some(LIVE_MAX_FRAME_BYTES),
            ..Default::default()
        };
        let (upstream, upstream_response) = tokio::time::timeout(
            LIVE_CONNECT_TIMEOUT,
            tokio_tungstenite::connect_async_with_config(
                upstream_request,
                Some(websocket_config),
                false,
            ),
        )
        .await
        .map_err(|_| ProxyError::ForwardFailed("Live sideband connect timed out".to_string()))?
        .map_err(|error| {
            ProxyError::ForwardFailed(format!("Live sideband connect failed: {error}"))
        })?;
        let selected_protocol =
            selected_websocket_protocol(upstream_response.headers(), &requested_protocols)?;
        Ok::<_, ProxyError>((provider, upstream, selected_protocol))
    }
    .await;
    let (provider, upstream, selected_protocol) = match setup_result {
        Ok(result) => result,
        Err(error) => {
            let restored = {
                let mut calls = state.live_calls.write().await;
                restore_live_call_binding(&mut calls, &call_id, reservation)
            };
            log::warn!(
                "[Codex Live] Sideband setup failed for call {call_id}; binding_restored={restored}: {error}"
            );
            return Err(error);
        }
    };
    let consumed = {
        let mut calls = state.live_calls.write().await;
        consume_live_call_binding(&mut calls, &call_id, reservation)
    };
    if !consumed {
        return Err(ProxyError::Internal(format!(
            "Live sideband binding changed during setup for call {call_id}"
        )));
    }
    let requested_protocol_count = requested_protocols.len();
    log::info!(
        "[Codex Live] Sideband connected for call {call_id} via {} (requested_protocols={}, negotiated_protocol={})",
        provider.name,
        requested_protocol_count,
        selected_protocol.is_some()
    );

    let upgrade = match selected_protocol {
        Some(protocol) => upgrade.protocols([protocol]),
        None => upgrade,
    }
    .max_message_size(LIVE_MAX_MESSAGE_BYTES)
    .max_frame_size(LIVE_MAX_FRAME_BYTES)
    .max_write_buffer_size(LIVE_MAX_MESSAGE_BYTES * 2);
    Ok(upgrade
        .on_upgrade(move |client| async move {
            let _slot_permit = slot_permit;
            relay_live_sideband(client, upstream, call_id).await;
        })
        .into_response())
}

async fn select_official_live_route(state: &ProxyState) -> Result<LiveProviderRoute, ProxyError> {
    let provider = state
        .db
        .get_provider_by_id(CODEX_OFFICIAL_PROVIDER_ID, AppType::Codex.as_str())
        .map_err(|error| ProxyError::DatabaseError(error.to_string()))?
        .ok_or_else(|| {
            ProxyError::ConfigError("OpenAI Official provider is unavailable for Live".to_string())
        })?;
    if !super::providers::is_codex_official_provider(&provider) {
        return Err(ProxyError::ConfigError(
            "the built-in OpenAI Official provider has invalid identity metadata".to_string(),
        ));
    }
    Ok(official_live_route(provider))
}

async fn select_current_live_route(state: &ProxyState) -> Result<LiveProviderRoute, ProxyError> {
    let provider_id = crate::settings::get_effective_current_provider(&state.db, &AppType::Codex)
        .map_err(|error| ProxyError::DatabaseError(error.to_string()))?
        .ok_or_else(|| {
            ProxyError::ConfigError("no current Codex provider is configured".to_string())
        })?;
    let provider = state
        .db
        .get_provider_by_id(&provider_id, AppType::Codex.as_str())
        .map_err(|error| ProxyError::DatabaseError(error.to_string()))?
        .ok_or_else(|| {
            ProxyError::ConfigError(format!(
                "the current Codex provider {provider_id} no longer exists"
            ))
        })?;
    live_route_for_provider(provider)
}

fn official_live_route(provider: Provider) -> LiveProviderRoute {
    LiveProviderRoute {
        provider,
        create_endpoint: OFFICIAL_LIVE_CALL_ENDPOINT.to_string(),
        sideband_endpoint: OFFICIAL_LIVE_SIDEBAND_ENDPOINT.to_string(),
        official: true,
    }
}

fn live_route_for_provider(provider: Provider) -> Result<LiveProviderRoute, ProxyError> {
    if super::providers::is_codex_official_provider(&provider) {
        return Ok(official_live_route(provider));
    }
    let meta = provider.meta.as_ref();
    if meta.is_some_and(|meta| meta.is_full_url == Some(true)) {
        return Err(ProxyError::ConfigError(format!(
            "the current provider {} uses a full request URL, which cannot be combined with relative Codex Live endpoints",
            provider.name
        )));
    }
    let config = meta
        .and_then(|meta| meta.enabled_codex_live())
        .ok_or_else(|| {
            ProxyError::ConfigError(format!(
                "the current provider {} has not declared Codex Live support",
                provider.name
            ))
        })?;
    let create_endpoint = normalize_live_endpoint(
        config.create_endpoint.as_deref(),
        DEFAULT_THIRD_PARTY_LIVE_CALL_ENDPOINT,
        false,
    )?;
    let sideband_endpoint = normalize_live_endpoint(
        config.sideband_endpoint.as_deref(),
        DEFAULT_THIRD_PARTY_LIVE_SIDEBAND_ENDPOINT,
        true,
    )?;
    Ok(LiveProviderRoute {
        provider,
        create_endpoint,
        sideband_endpoint,
        official: false,
    })
}

fn normalize_live_endpoint(
    configured: Option<&str>,
    default: &str,
    requires_call_id: bool,
) -> Result<String, ProxyError> {
    let endpoint = configured
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(default)
        .trim_matches('/');
    let unsafe_value = endpoint.contains("://")
        || endpoint.contains('?')
        || endpoint.contains('#')
        || endpoint.contains("..")
        || endpoint.contains('\\')
        || endpoint.chars().any(char::is_whitespace);
    if endpoint.is_empty() || unsafe_value {
        return Err(ProxyError::ConfigError(
            "Codex Live endpoints must be relative paths without query strings or traversal"
                .to_string(),
        ));
    }
    if requires_call_id && endpoint.matches("{call_id}").count() != 1 {
        return Err(ProxyError::ConfigError(
            "Codex Live sideband endpoint must contain {call_id} exactly once".to_string(),
        ));
    }
    Ok(endpoint.to_string())
}

fn render_sideband_endpoint(template: &str, call_id: &str) -> Result<String, ProxyError> {
    if !is_valid_call_id(call_id) {
        return Err(ProxyError::ConfigError(
            "invalid Live sideband call id".to_string(),
        ));
    }
    Ok(template.replace("{call_id}", call_id))
}

fn live_disabled_response() -> Response<Body> {
    let mut response = Response::new(Body::from(LIVE_DISABLED_BODY));
    *response.status_mut() = StatusCode::SERVICE_UNAVAILABLE;
    response.headers_mut().insert(
        http::header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    response
}

fn live_non_loopback_response() -> Response<Body> {
    const BODY: &str = r#"{"error":{"code":"codex_live_loopback_required","message":"Codex Live voice is available only when the CC Switch proxy listens on loopback","type":"security_policy"}}"#;
    let mut response = Response::new(Body::from(BODY));
    *response.status_mut() = StatusCode::FORBIDDEN;
    response.headers_mut().insert(
        http::header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    response
}

fn live_client_rejected_response() -> Response<Body> {
    const BODY: &str = r#"{"error":{"code":"codex_live_client_rejected","message":"Codex Live accepts only local Codex Desktop traffic","type":"security_policy"}}"#;
    let mut response = Response::new(Body::from(BODY));
    *response.status_mut() = StatusCode::FORBIDDEN;
    response.headers_mut().insert(
        http::header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    response
}

fn live_capacity_response() -> Response<Body> {
    const BODY: &str = r#"{"error":{"code":"codex_live_capacity_reached","message":"Too many active Codex Live sideband connections","type":"capacity_limit"}}"#;
    let mut response = Response::new(Body::from(BODY));
    *response.status_mut() = StatusCode::SERVICE_UNAVAILABLE;
    response.headers_mut().insert(
        http::header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    response
}

fn live_outbound_proxy_unsupported_response() -> Response<Body> {
    const BODY: &str = r#"{"error":{"code":"codex_live_outbound_proxy_unsupported","message":"Codex Live cannot start while a global outbound proxy is configured because websocket proxying is not yet supported","type":"unsupported_configuration"}}"#;
    let mut response = Response::new(Body::from(BODY));
    *response.status_mut() = StatusCode::SERVICE_UNAVAILABLE;
    response.headers_mut().insert(
        http::header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    response
}

fn reserve_live_call_binding(
    calls: &mut HashMap<String, LiveCallBinding>,
    call_id: &str,
) -> Result<(LiveCallBinding, LiveCallReservation), ProxyError> {
    calls.retain(|_, binding| binding.created_at.elapsed() < LIVE_CALL_BINDING_TTL);
    let binding = calls.get_mut(call_id).ok_or_else(|| {
        ProxyError::ConfigError(format!(
            "Live sideband provider binding is missing or expired for call {call_id}"
        ))
    })?;
    if !matches!(binding.phase, LiveCallBindingPhase::Available) {
        return Err(ProxyError::ConfigError(format!(
            "Live sideband provider binding is already connecting for call {call_id}"
        )));
    }

    let reservation = LiveCallReservation {
        generation: binding.generation,
        attempt: uuid::Uuid::new_v4(),
    };
    binding.phase = LiveCallBindingPhase::Connecting(reservation.attempt);
    Ok((binding.clone(), reservation))
}

fn restore_live_call_binding(
    calls: &mut HashMap<String, LiveCallBinding>,
    call_id: &str,
    reservation: LiveCallReservation,
) -> bool {
    let Some(binding) = calls.get_mut(call_id) else {
        return false;
    };
    if binding.generation != reservation.generation
        || binding.phase != LiveCallBindingPhase::Connecting(reservation.attempt)
    {
        return false;
    }
    binding.phase = LiveCallBindingPhase::Available;
    true
}

fn consume_live_call_binding(
    calls: &mut HashMap<String, LiveCallBinding>,
    call_id: &str,
    reservation: LiveCallReservation,
) -> bool {
    let matches = calls.get(call_id).is_some_and(|binding| {
        binding.generation == reservation.generation
            && binding.phase == LiveCallBindingPhase::Connecting(reservation.attempt)
    });
    if matches {
        calls.remove(call_id);
    }
    matches
}

fn live_invalid_upstream_response() -> Response<Body> {
    const BODY: &str = r#"{"error":{"code":"codex_live_invalid_upstream_response","message":"Live upstream returned success without a usable Location call id","type":"upstream_protocol_error"}}"#;
    let mut response = Response::new(Body::from(BODY));
    *response.status_mut() = StatusCode::BAD_GATEWAY;
    response.headers_mut().insert(
        http::header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    response
}

fn local_sideband_location(
    client_path: &str,
    call_id: &str,
    upstream_query: Option<&str>,
) -> Result<HeaderValue, ProxyError> {
    if !is_valid_call_id(call_id) {
        return Err(ProxyError::ConfigError(
            "invalid Live sideband call id".to_string(),
        ));
    }
    let mut path = format!("{}/{call_id}", client_path.trim_end_matches('/'));
    if let Some(query) = upstream_query {
        path.push('?');
        path.push_str(query);
    }
    HeaderValue::from_str(&path).map_err(|error| {
        ProxyError::Internal(format!(
            "failed to build local Live Location header: {error}"
        ))
    })
}

async fn live_listener_is_loopback(state: &ProxyState) -> bool {
    is_loopback_address(&state.config.read().await.listen_address)
}

fn is_loopback_address(address: &str) -> bool {
    let address = address.trim().trim_matches(['[', ']']);
    address.eq_ignore_ascii_case("localhost")
        || address
            .parse::<IpAddr>()
            .map(|address| address.is_loopback())
            .unwrap_or(false)
}

fn validate_live_client_headers(
    headers: &HeaderMap,
    require_attestation: bool,
) -> Result<(), &'static str> {
    if headers.contains_key(http::header::ORIGIN)
        || headers
            .keys()
            .any(|name| name.as_str().starts_with("sec-fetch-"))
    {
        return Err("browser-originated requests are not allowed");
    }
    if !live_host_is_loopback(headers) {
        return Err("Host must identify a loopback address");
    }
    if require_attestation {
        let mut values = headers.get_all(live_attestation::HEADER_NAME).iter();
        let Some(value) = values.next() else {
            return Err("the Codex attestation header is missing");
        };
        if values.next().is_some()
            || value.as_bytes().is_empty()
            || value.as_bytes().len() > live_attestation::MAX_BYTES
        {
            return Err("the Codex attestation header is malformed");
        }
    }
    Ok(())
}

fn live_host_is_loopback(headers: &HeaderMap) -> bool {
    let mut hosts = headers.get_all(http::header::HOST).iter();
    let Some(host) = hosts.next() else {
        return false;
    };
    if hosts.next().is_some() {
        return false;
    }
    host.to_str()
        .ok()
        .and_then(|value| value.parse::<http::uri::Authority>().ok())
        .is_some_and(|authority| is_loopback_address(authority.host()))
}

fn live_upstream_url(
    adapter: &dyn ProviderAdapter,
    provider: &Provider,
    endpoint: &str,
    client_uri: &Uri,
) -> Result<String, ProxyError> {
    let base_url = adapter.extract_base_url(provider)?;
    let endpoint = match client_uri.query() {
        Some(query) => format!("{endpoint}?{query}"),
        None => endpoint.to_string(),
    };
    Ok(adapter.build_url(&base_url, &endpoint))
}

async fn upstream_headers(
    headers: HeaderMap,
    adapter: &dyn ProviderAdapter,
    provider: &Provider,
    official: bool,
    state: &ProxyState,
) -> Result<HeaderMap, ProxyError> {
    let mut headers = filter_live_headers(&headers, official);
    headers.remove(http::header::HOST);
    headers.remove(http::header::CONTENT_LENGTH);
    headers.remove(http::header::AUTHORIZATION);
    headers.remove("proxy-authorization");
    headers.remove("chatgpt-account-id");
    if official {
        let bound_account_id = provider
            .meta
            .as_ref()
            .and_then(|meta| meta.managed_account_id_for("codex_oauth"));
        match read_current_codex_credentials() {
            Ok((token, account_id))
                if account_binding_matches(bound_account_id.as_deref(), account_id.as_deref())
                    && access_token_is_fresh(&token) =>
            {
                apply_codex_oauth_headers(&mut headers, &token, account_id.as_deref())?;
            }
            current_credentials => {
                let current_error = match &current_credentials {
                    Ok((_, account_id))
                        if !account_binding_matches(
                            bound_account_id.as_deref(),
                            account_id.as_deref(),
                        ) =>
                    {
                        "current Codex login does not match the bound CC Switch account".to_string()
                    }
                    Ok(_) => "current Codex OAuth access token is expired".to_string(),
                    Err(error) => error.clone(),
                };
                let app_handle = state.app_handle.as_ref().ok_or_else(|| {
                    ProxyError::AuthError(format!(
                        "Codex Live OAuth is unavailable without AppHandle ({current_error})"
                    ))
                })?;
                let oauth_state = app_handle.state::<CodexOAuthState>();
                let oauth = oauth_state.0.read().await;
                let token = match bound_account_id.as_deref() {
                    Some(account_id) => oauth.get_valid_token_for_account(account_id).await,
                    None => oauth.get_valid_token().await,
                }
                .map_err(|error| {
                    ProxyError::AuthError(format!(
                        "Codex Live OAuth authentication failed: {error} (current Codex login unavailable: {current_error})"
                    ))
                })?;
                let account_id = match bound_account_id {
                    Some(account_id) => Some(account_id),
                    None => oauth.default_account_id().await,
                };
                apply_codex_oauth_headers(&mut headers, &token, account_id.as_deref())?;
            }
        }
    } else if let Some(auth) = adapter.extract_auth(provider) {
        for (name, value) in adapter.get_auth_headers(&auth)? {
            headers.insert(name, value);
        }
    }
    Ok(headers)
}

fn filter_live_headers(source: &HeaderMap, official: bool) -> HeaderMap {
    let mut filtered = HeaderMap::new();
    for (name, value) in source {
        let allowed = !is_hop_by_hop(name)
            && !is_websocket_handshake_header(name)
            && if official {
                is_official_live_header(name)
            } else {
                !is_sensitive_credential_header(name)
            };
        if allowed {
            filtered.append(name.clone(), value.clone());
        }
    }
    filtered
}

fn is_official_live_header(name: &HeaderName) -> bool {
    matches!(
        name.as_str(),
        "accept"
            | "accept-language"
            | "content-type"
            | "openai-beta"
            | "originator"
            | "session_id"
            | "user-agent"
            | "x-client-request-id"
            | "x-codex-window-id"
            | "x-oai-attestation"
    )
}

fn is_sensitive_credential_header(name: &HeaderName) -> bool {
    let name = name.as_str();
    matches!(
        name,
        "authorization"
            | "proxy-authorization"
            | "cookie"
            | "cookie2"
            | "set-cookie"
            | "api-key"
            | "x-api-key"
            | "x-goog-api-key"
            | "x-auth-token"
            | "x-access-token"
            | "access-token"
            | "authentication"
            | "chatgpt-account-id"
    ) || name.ends_with("-api-key")
        || name.ends_with("-secret")
}

fn read_current_codex_credentials() -> Result<(String, Option<String>), String> {
    let path = crate::codex_config::get_codex_auth_path();
    let content = std::fs::read_to_string(&path)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    parse_current_codex_credentials(&content)
}

fn parse_current_codex_credentials(content: &str) -> Result<(String, Option<String>), String> {
    let auth: serde_json::Value = serde_json::from_str(content)
        .map_err(|error| format!("failed to parse Codex auth JSON: {error}"))?;
    if auth.get("auth_mode").and_then(|value| value.as_str()) != Some("chatgpt") {
        return Err("Codex is not logged in with ChatGPT OAuth".to_string());
    }
    let tokens = auth
        .get("tokens")
        .and_then(|value| value.as_object())
        .ok_or_else(|| "Codex OAuth tokens are missing".to_string())?;
    let access_token = tokens
        .get("access_token")
        .and_then(|value| value.as_str())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "Codex OAuth access token is missing".to_string())?
        .to_string();
    let account_id = tokens
        .get("account_id")
        .and_then(|value| value.as_str())
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    Ok((access_token, account_id))
}

fn account_binding_matches(bound_account_id: Option<&str>, account_id: Option<&str>) -> bool {
    match bound_account_id {
        Some(bound) => account_id == Some(bound),
        None => true,
    }
}

fn access_token_is_fresh(token: &str) -> bool {
    const REFRESH_BUFFER_SECONDS: i64 = 5 * 60;
    let Some(payload) = token.split('.').nth(1) else {
        return false;
    };
    let Ok(payload) = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(payload) else {
        return false;
    };
    let Ok(claims) = serde_json::from_slice::<serde_json::Value>(&payload) else {
        return false;
    };
    claims
        .get("exp")
        .and_then(serde_json::Value::as_i64)
        .is_some_and(|exp| exp > chrono::Utc::now().timestamp() + REFRESH_BUFFER_SECONDS)
}

fn apply_codex_oauth_headers(
    headers: &mut HeaderMap,
    token: &str,
    account_id: Option<&str>,
) -> Result<(), ProxyError> {
    let bearer = http::HeaderValue::from_str(&format!("Bearer {token}"))
        .map_err(|error| ProxyError::AuthError(format!("invalid Codex OAuth token: {error}")))?;
    headers.insert(http::header::AUTHORIZATION, bearer);
    if let Some(account_id) = account_id {
        let value = http::HeaderValue::from_str(account_id).map_err(|error| {
            ProxyError::AuthError(format!("invalid ChatGPT account id: {error}"))
        })?;
        headers.insert("chatgpt-account-id", value);
    }
    Ok(())
}

fn is_hop_by_hop(name: &HeaderName) -> bool {
    matches!(
        name.as_str(),
        "connection"
            | "keep-alive"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "upgrade"
    )
}

fn is_websocket_handshake_header(name: &HeaderName) -> bool {
    name.as_str().starts_with("sec-websocket-") || name == http::header::UPGRADE
}

fn install_websocket_protocol_header(
    protocols: &[String],
    target: &mut HeaderMap,
) -> Result<(), ProxyError> {
    target.remove(http::header::SEC_WEBSOCKET_PROTOCOL);
    if !protocols.is_empty() {
        // tungstenite 0.24 does not trim entries when validating the server's
        // selected protocol, so serialize without optional whitespace.
        let value = HeaderValue::from_str(&protocols.join(",")).map_err(|error| {
            ProxyError::ConfigError(format!(
                "invalid Live websocket protocol list after normalization: {error}"
            ))
        })?;
        target.insert(http::header::SEC_WEBSOCKET_PROTOCOL, value);
    }
    Ok(())
}

fn parse_websocket_protocols(headers: &HeaderMap) -> Result<Vec<String>, ProxyError> {
    let mut protocols = Vec::new();
    let mut seen = HashSet::new();
    for value in headers.get_all(http::header::SEC_WEBSOCKET_PROTOCOL).iter() {
        let value = value.to_str().map_err(|error| {
            ProxyError::ConfigError(format!(
                "Live websocket protocol header is not valid text: {error}"
            ))
        })?;
        for protocol in value.split(',').map(str::trim) {
            if protocol.is_empty()
                || protocol.len() > LIVE_MAX_PROTOCOL_BYTES
                || !protocol.bytes().all(is_http_token_byte)
            {
                return Err(ProxyError::ConfigError(
                    "Live websocket protocol contains an invalid token".to_string(),
                ));
            }
            if seen.insert(protocol.to_string()) {
                protocols.push(protocol.to_string());
                if protocols.len() > LIVE_MAX_PROTOCOLS {
                    return Err(ProxyError::ConfigError(format!(
                        "Live websocket requested more than {LIVE_MAX_PROTOCOLS} protocols"
                    )));
                }
            }
        }
    }
    Ok(protocols)
}

fn selected_websocket_protocol(
    headers: &HeaderMap,
    requested: &[String],
) -> Result<Option<String>, ProxyError> {
    let Some(value) = headers.get(http::header::SEC_WEBSOCKET_PROTOCOL) else {
        return Ok(None);
    };
    let protocol = value.to_str().map(str::trim).map_err(|error| {
        ProxyError::ForwardFailed(format!(
            "Live sideband returned an invalid websocket protocol: {error}"
        ))
    })?;
    if protocol.is_empty()
        || protocol.contains(',')
        || protocol.len() > LIVE_MAX_PROTOCOL_BYTES
        || !protocol.bytes().all(is_http_token_byte)
        || !requested.iter().any(|offered| offered == protocol)
    {
        return Err(ProxyError::ForwardFailed(
            "Live sideband selected a websocket protocol that was not offered".to_string(),
        ));
    }
    Ok(Some(protocol.to_string()))
}

fn is_http_token_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(
            byte,
            b'!' | b'#'
                | b'$'
                | b'%'
                | b'&'
                | b'\''
                | b'*'
                | b'+'
                | b'-'
                | b'.'
                | b'^'
                | b'_'
                | b'`'
                | b'|'
                | b'~'
        )
}

fn websocket_url(http_url: &str) -> Result<url::Url, ProxyError> {
    let mut url = url::Url::parse(http_url)
        .map_err(|error| ProxyError::ConfigError(format!("invalid Live URL: {error}")))?;
    let ws_scheme = match url.scheme() {
        "http" => "ws",
        "https" => "wss",
        "ws" | "wss" => return Ok(url),
        scheme => {
            return Err(ProxyError::ConfigError(format!(
                "unsupported Live URL scheme: {scheme}"
            )))
        }
    };
    url.set_scheme(ws_scheme)
        .map_err(|_| ProxyError::ConfigError("failed to set Live websocket scheme".to_string()))?;
    Ok(url)
}

fn extract_live_call_id(location: &str) -> Option<&str> {
    let final_segment = location
        .split('?')
        .next()
        .unwrap_or(location)
        .trim_end_matches('/')
        .rsplit('/')
        .next()?;
    is_valid_call_id(final_segment).then_some(final_segment)
}

fn parse_live_location(location: &str) -> Option<(String, Option<String>)> {
    let location = location.parse::<Uri>().ok()?;
    let call_id = extract_live_call_id(location.path())?.to_string();
    let query = location.query().map(str::to_string);
    Some((call_id, query))
}

fn is_valid_call_id(value: &str) -> bool {
    (value.starts_with("rtc_") || value.len() == 36)
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

async fn relay_live_sideband(
    client: WebSocket,
    upstream: tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    call_id: String,
) {
    let (mut client_tx, mut client_rx) = client.split();
    let (mut upstream_tx, mut upstream_rx) = upstream.split();
    let outcome = loop {
        let idle_timeout = tokio::time::sleep(LIVE_IDLE_TIMEOUT);
        tokio::pin!(idle_timeout);
        tokio::select! {
            _ = &mut idle_timeout => {
                log::warn!("[Codex Live] Sideband idle timeout for {call_id}");
                send_upstream_close(&mut upstream_tx, LIVE_IDLE_CLOSE_CODE, "sideband idle timeout").await;
                send_client_close(&mut client_tx, LIVE_IDLE_CLOSE_CODE, "sideband idle timeout").await;
                break "idle_timeout";
            }
            message = client_rx.next() => {
                let Some(message) = message else {
                    log::warn!("[Codex Live] client sideband transport ended without a Close frame for {call_id}");
                    send_upstream_internal_error(&mut upstream_tx, "client sideband transport closed").await;
                    break "client_transport_ended";
                };
                let message = match message {
                    Ok(message) => message,
                    Err(error) => {
                        log::warn!("[Codex Live] client sideband transport error for {call_id}: {error}");
                        send_upstream_internal_error(&mut upstream_tx, "client sideband transport error").await;
                        break "client_read_error";
                    }
                };
                let metadata = client_frame_metadata(&message);
                log_sideband_frame(&call_id, "client_to_upstream", metadata);
                let is_close = metadata.kind == "close";
                let message = client_to_upstream(message);
                if is_close {
                    if send_upstream_with_timeout(&mut upstream_tx, message, LIVE_CLOSE_TIMEOUT).await.is_err() {
                        log::warn!("[Codex Live] timed out or failed forwarding the client Close frame for {call_id}");
                    }
                    break "client_close";
                }
                if send_upstream_with_timeout(&mut upstream_tx, message, LIVE_WRITE_TIMEOUT).await.is_err() {
                    log::warn!("[Codex Live] upstream sideband write timed out or failed for {call_id}");
                    send_client_internal_error(&mut client_tx, "upstream sideband write failed").await;
                    break "upstream_write_error";
                }
            }
            message = upstream_rx.next() => {
                let Some(message) = message else {
                    log::warn!("[Codex Live] upstream sideband transport ended without a Close frame for {call_id}");
                    send_client_internal_error(&mut client_tx, "upstream sideband transport closed").await;
                    break "upstream_transport_ended";
                };
                let message = match message {
                    Ok(message) => message,
                    Err(error) => {
                        log::warn!("[Codex Live] upstream sideband transport error for {call_id}: {error}");
                        send_client_internal_error(&mut client_tx, "upstream sideband transport error").await;
                        break "upstream_read_error";
                    }
                };
                let metadata = upstream_frame_metadata(&message);
                log_sideband_frame(&call_id, "upstream_to_client", metadata);
                let is_close = metadata.kind == "close";
                let Some(message) = upstream_to_client(message) else { continue };
                if is_close {
                    if send_client_with_timeout(&mut client_tx, message, LIVE_CLOSE_TIMEOUT).await.is_err() {
                        log::warn!("[Codex Live] timed out or failed forwarding the upstream Close frame for {call_id}");
                    }
                    break "upstream_close";
                }
                if send_client_with_timeout(&mut client_tx, message, LIVE_WRITE_TIMEOUT).await.is_err() {
                    log::warn!("[Codex Live] client sideband write timed out or failed for {call_id}");
                    send_upstream_internal_error(&mut upstream_tx, "client sideband write failed").await;
                    break "client_write_error";
                }
            }
        }
    };
    match tokio::time::timeout(LIVE_CLOSE_TIMEOUT, upstream_tx.close()).await {
        Ok(Ok(())) => {}
        Ok(Err(error)) => {
            log::warn!("[Codex Live] upstream sideband close failed for {call_id}: {error}");
        }
        Err(_) => {
            log::warn!("[Codex Live] upstream sideband close timed out for {call_id}");
        }
    }
    match tokio::time::timeout(LIVE_CLOSE_TIMEOUT, client_tx.close()).await {
        Ok(Ok(())) => {}
        Ok(Err(error)) => {
            log::warn!("[Codex Live] client sideband close failed for {call_id}: {error}");
        }
        Err(_) => {
            log::warn!("[Codex Live] client sideband close timed out for {call_id}");
        }
    }
    log::info!("[Codex Live] Sideband closed for call {call_id} (outcome={outcome})");
}

fn client_to_upstream(message: ClientMessage) -> UpstreamMessage {
    match message {
        ClientMessage::Text(value) => UpstreamMessage::Text(value),
        ClientMessage::Binary(value) => UpstreamMessage::Binary(value),
        ClientMessage::Ping(value) => UpstreamMessage::Ping(value),
        ClientMessage::Pong(value) => UpstreamMessage::Pong(value),
        ClientMessage::Close(frame) => UpstreamMessage::Close(frame.map(|frame| {
            tokio_tungstenite::tungstenite::protocol::CloseFrame {
                code: frame.code.into(),
                reason: frame.reason,
            }
        })),
    }
}

fn upstream_to_client(message: UpstreamMessage) -> Option<ClientMessage> {
    match message {
        UpstreamMessage::Text(value) => Some(ClientMessage::Text(value.to_string())),
        UpstreamMessage::Binary(value) => Some(ClientMessage::Binary(value.to_vec())),
        UpstreamMessage::Ping(value) => Some(ClientMessage::Ping(value.to_vec())),
        UpstreamMessage::Pong(value) => Some(ClientMessage::Pong(value.to_vec())),
        UpstreamMessage::Close(frame) => {
            Some(ClientMessage::Close(frame.map(|frame| ClientCloseFrame {
                code: frame.code.into(),
                reason: frame.reason,
            })))
        }
        UpstreamMessage::Frame(_) => None,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SidebandFrameMetadata {
    kind: &'static str,
    bytes: usize,
    close_code: Option<u16>,
}

fn client_frame_metadata(message: &ClientMessage) -> SidebandFrameMetadata {
    match message {
        ClientMessage::Text(value) => frame_metadata("text", value.len(), None),
        ClientMessage::Binary(value) => frame_metadata("binary", value.len(), None),
        ClientMessage::Ping(value) => frame_metadata("ping", value.len(), None),
        ClientMessage::Pong(value) => frame_metadata("pong", value.len(), None),
        ClientMessage::Close(frame) => frame_metadata(
            "close",
            frame.as_ref().map_or(0, |frame| 2 + frame.reason.len()),
            frame.as_ref().map(|frame| frame.code),
        ),
    }
}

fn upstream_frame_metadata(message: &UpstreamMessage) -> SidebandFrameMetadata {
    match message {
        UpstreamMessage::Text(value) => frame_metadata("text", value.len(), None),
        UpstreamMessage::Binary(value) => frame_metadata("binary", value.len(), None),
        UpstreamMessage::Ping(value) => frame_metadata("ping", value.len(), None),
        UpstreamMessage::Pong(value) => frame_metadata("pong", value.len(), None),
        UpstreamMessage::Close(frame) => frame_metadata(
            "close",
            frame.as_ref().map_or(0, |frame| 2 + frame.reason.len()),
            frame.as_ref().map(|frame| frame.code.into()),
        ),
        UpstreamMessage::Frame(_) => frame_metadata("frame", 0, None),
    }
}

fn frame_metadata(
    kind: &'static str,
    bytes: usize,
    close_code: Option<u16>,
) -> SidebandFrameMetadata {
    SidebandFrameMetadata {
        kind,
        bytes,
        close_code,
    }
}

fn log_sideband_frame(call_id: &str, direction: &str, metadata: SidebandFrameMetadata) {
    let close_code = metadata
        .close_code
        .map(|code| code.to_string())
        .unwrap_or_else(|| "none".to_string());
    if metadata.kind == "close" {
        log::info!(
            "[Codex Live] Sideband frame for {call_id} (direction={direction}, type={}, bytes={}, close_code={close_code})",
            metadata.kind,
            metadata.bytes
        );
    } else {
        log::debug!(
            "[Codex Live] Sideband frame for {call_id} (direction={direction}, type={}, bytes={}, close_code={close_code})",
            metadata.kind,
            metadata.bytes
        );
    }
}

async fn send_upstream_with_timeout<S>(
    sink: &mut S,
    message: UpstreamMessage,
    timeout: Duration,
) -> Result<(), ()>
where
    S: futures::Sink<UpstreamMessage> + Unpin,
{
    match tokio::time::timeout(timeout, sink.send(message)).await {
        Ok(Ok(())) => Ok(()),
        Ok(Err(_)) | Err(_) => Err(()),
    }
}

async fn send_client_with_timeout<S>(
    sink: &mut S,
    message: ClientMessage,
    timeout: Duration,
) -> Result<(), ()>
where
    S: futures::Sink<ClientMessage> + Unpin,
{
    match tokio::time::timeout(timeout, sink.send(message)).await {
        Ok(Ok(())) => Ok(()),
        Ok(Err(_)) | Err(_) => Err(()),
    }
}

async fn send_upstream_close<S>(sink: &mut S, code: u16, reason: &'static str)
where
    S: futures::Sink<UpstreamMessage> + Unpin,
{
    let message =
        UpstreamMessage::Close(Some(tokio_tungstenite::tungstenite::protocol::CloseFrame {
            code: code.into(),
            reason: reason.into(),
        }));
    let _ = send_upstream_with_timeout(sink, message, LIVE_CLOSE_TIMEOUT).await;
}

async fn send_client_close<S>(sink: &mut S, code: u16, reason: &'static str)
where
    S: futures::Sink<ClientMessage> + Unpin,
{
    let message = ClientMessage::Close(Some(ClientCloseFrame {
        code,
        reason: reason.into(),
    }));
    let _ = send_client_with_timeout(sink, message, LIVE_CLOSE_TIMEOUT).await;
}

async fn send_upstream_internal_error<S>(sink: &mut S, reason: &'static str)
where
    S: futures::Sink<UpstreamMessage> + Unpin,
{
    send_upstream_close(sink, LIVE_INTERNAL_ERROR_CODE, reason).await;
}

async fn send_client_internal_error<S>(sink: &mut S, reason: &'static str)
where
    S: futures::Sink<ClientMessage> + Unpin,
{
    send_client_close(sink, LIVE_INTERNAL_ERROR_CODE, reason).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_supported_live_call_ids() {
        assert_eq!(
            extract_live_call_id("/v1/live/rtc_test?intent=quicksilver"),
            Some("rtc_test")
        );
        assert_eq!(
            extract_live_call_id("/v1/live/123e4567-e89b-12d3-a456-426614174000"),
            Some("123e4567-e89b-12d3-a456-426614174000")
        );
        assert_eq!(extract_live_call_id("/v1/live/rtc_test/status"), None);
        assert_eq!(extract_live_call_id("/v1/live/rtc_test/"), Some("rtc_test"));
        assert_eq!(extract_live_call_id("/v1/live"), None);
    }

    #[test]
    fn parses_live_location_query_parameters() {
        assert_eq!(
            parse_live_location("/v1/live/rtc_test?intent=quicksilver&token=a%2Fb"),
            Some((
                "rtc_test".to_string(),
                Some("intent=quicksilver&token=a%2Fb".to_string())
            ))
        );
        assert_eq!(
            parse_live_location("https://example.com/v1/live/rtc_test?intent=quicksilver"),
            Some((
                "rtc_test".to_string(),
                Some("intent=quicksilver".to_string())
            ))
        );
    }

    #[test]
    fn codex_live_voice_disabled_response_is_machine_readable_service_unavailable() {
        let response = live_disabled_response();

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(
            response.headers().get(http::header::CONTENT_TYPE).unwrap(),
            "application/json"
        );
    }

    #[test]
    fn explicit_outbound_proxy_is_rejected_before_live_call_creation() {
        let response = live_outbound_proxy_unsupported_response();

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(
            response.headers().get(http::header::CONTENT_TYPE).unwrap(),
            "application/json"
        );
    }

    #[test]
    fn failed_sideband_setup_restores_only_its_own_binding_generation() {
        let call_id = "rtc_retry";
        let mut calls = HashMap::new();
        calls.insert(call_id.to_string(), test_live_binding());

        let (_, first) = reserve_live_call_binding(&mut calls, call_id).unwrap();
        assert!(reserve_live_call_binding(&mut calls, call_id).is_err());
        assert!(restore_live_call_binding(&mut calls, call_id, first));

        let (_, second) = reserve_live_call_binding(&mut calls, call_id).unwrap();
        assert_ne!(first.attempt, second.attempt);
        assert!(!restore_live_call_binding(&mut calls, call_id, first));
        assert!(consume_live_call_binding(&mut calls, call_id, second));
        assert!(!calls.contains_key(call_id));
    }

    #[test]
    fn converts_live_http_url_to_websocket() {
        assert_eq!(
            websocket_url("http://127.0.0.1:8080/v1/live/rtc_test")
                .unwrap()
                .as_str(),
            "ws://127.0.0.1:8080/v1/live/rtc_test"
        );
    }

    #[test]
    fn normalizes_protocols_and_accepts_the_second_offered_protocol() {
        let mut client_headers = HeaderMap::new();
        client_headers.insert(
            http::header::SEC_WEBSOCKET_PROTOCOL,
            http::HeaderValue::from_static("realtime,   openai-live"),
        );
        client_headers.insert(
            http::header::SEC_WEBSOCKET_KEY,
            http::HeaderValue::from_static("client-generated-key"),
        );
        let protocols = parse_websocket_protocols(&client_headers).unwrap();
        let mut upstream_headers = HeaderMap::new();

        install_websocket_protocol_header(&protocols, &mut upstream_headers).unwrap();

        assert_eq!(protocols, vec!["realtime", "openai-live"]);
        assert_eq!(
            upstream_headers
                .get(http::header::SEC_WEBSOCKET_PROTOCOL)
                .unwrap(),
            "realtime,openai-live"
        );
        assert!(upstream_headers
            .get(http::header::SEC_WEBSOCKET_KEY)
            .is_none());
        upstream_headers.insert(
            http::header::SEC_WEBSOCKET_PROTOCOL,
            HeaderValue::from_static("openai-live"),
        );
        assert_eq!(
            selected_websocket_protocol(&upstream_headers, &protocols).unwrap(),
            Some("openai-live".to_string())
        );
    }

    #[tokio::test]
    async fn upstream_handshake_can_select_the_second_offered_protocol() {
        use tokio_tungstenite::tungstenite::handshake::server::{ErrorResponse, Request, Response};

        #[allow(clippy::result_large_err)]
        fn select_second_protocol(
            request: &Request,
            mut response: Response,
        ) -> Result<Response, ErrorResponse> {
            assert_eq!(
                request
                    .headers()
                    .get(http::header::SEC_WEBSOCKET_PROTOCOL)
                    .expect("protocol request header"),
                "realtime,openai-live"
            );
            response.headers_mut().insert(
                http::header::SEC_WEBSOCKET_PROTOCOL,
                HeaderValue::from_static("openai-live"),
            );
            Ok(response)
        }

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test websocket listener");
        let address = listener.local_addr().expect("read test listener address");
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("accept test websocket");
            let mut websocket = tokio_tungstenite::accept_hdr_async(stream, select_second_protocol)
                .await
                .expect("complete server websocket handshake");
            websocket.close(None).await.expect("close server websocket");
        });

        let mut client_headers = HeaderMap::new();
        client_headers.insert(
            http::header::SEC_WEBSOCKET_PROTOCOL,
            HeaderValue::from_static("realtime,   openai-live"),
        );
        let protocols = parse_websocket_protocols(&client_headers).unwrap();
        let mut request = format!("ws://{address}/live")
            .into_client_request()
            .expect("build websocket request");
        install_websocket_protocol_header(&protocols, request.headers_mut()).unwrap();

        let (_websocket, response) = tokio_tungstenite::connect_async(request)
            .await
            .expect("connect with second selected protocol");
        assert_eq!(
            selected_websocket_protocol(response.headers(), &protocols).unwrap(),
            Some("openai-live".to_string())
        );
        server.await.expect("join websocket test server");
    }

    #[test]
    fn rejects_unoffered_or_malformed_websocket_protocols() {
        let mut response_headers = HeaderMap::new();
        response_headers.insert(
            http::header::SEC_WEBSOCKET_PROTOCOL,
            HeaderValue::from_static("not-offered"),
        );
        assert!(selected_websocket_protocol(&response_headers, &["realtime".to_string()]).is_err());

        let mut request_headers = HeaderMap::new();
        request_headers.insert(
            http::header::SEC_WEBSOCKET_PROTOCOL,
            HeaderValue::from_static("realtime, bad protocol"),
        );
        assert!(parse_websocket_protocols(&request_headers).is_err());
    }

    #[test]
    fn preserves_close_code_and_reason_in_both_directions() {
        let upstream = client_to_upstream(ClientMessage::Close(Some(ClientCloseFrame {
            code: 1011,
            reason: "upstream unavailable".into(),
        })));
        let UpstreamMessage::Close(Some(upstream_frame)) = upstream else {
            panic!("expected upstream Close frame");
        };
        assert_eq!(u16::from(upstream_frame.code), 1011);
        assert_eq!(upstream_frame.reason, "upstream unavailable");

        let client = upstream_to_client(UpstreamMessage::Close(Some(
            tokio_tungstenite::tungstenite::protocol::CloseFrame {
                code: 1013_u16.into(),
                reason: "try again".into(),
            },
        )))
        .expect("convert upstream Close frame");
        let ClientMessage::Close(Some(client_frame)) = client else {
            panic!("expected client Close frame");
        };
        assert_eq!(client_frame.code, 1013);
        assert_eq!(client_frame.reason, "try again");
    }

    #[tokio::test]
    async fn transport_errors_emit_a_sanitized_1011_close() {
        let (mut sender, mut receiver) = futures::channel::mpsc::channel(1);

        send_upstream_internal_error(&mut sender, "upstream sideband transport error").await;

        let message = receiver.next().await.expect("receive Close frame");
        let UpstreamMessage::Close(Some(frame)) = message else {
            panic!("expected upstream Close frame");
        };
        assert_eq!(u16::from(frame.code), LIVE_INTERNAL_ERROR_CODE);
        assert_eq!(frame.reason, "upstream sideband transport error");
    }

    #[test]
    fn codex_oauth_headers_replace_client_credentials() {
        let mut headers = HeaderMap::new();
        headers.insert(
            http::header::AUTHORIZATION,
            http::HeaderValue::from_static("Bearer stale"),
        );
        headers.insert(
            "chatgpt-account-id",
            http::HeaderValue::from_static("stale-account"),
        );

        apply_codex_oauth_headers(&mut headers, "fresh-token", Some("official-account"))
            .expect("apply OAuth headers");

        assert_eq!(
            headers.get(http::header::AUTHORIZATION).unwrap(),
            "Bearer fresh-token"
        );
        assert_eq!(
            headers.get("chatgpt-account-id").unwrap(),
            "official-account"
        );
    }

    #[test]
    fn official_live_header_allowlist_drops_client_credentials() {
        let mut source = HeaderMap::new();
        source.insert(
            http::header::CONTENT_TYPE,
            HeaderValue::from_static("multipart/form-data; boundary=test"),
        );
        source.insert(
            live_attestation::HEADER_NAME,
            HeaderValue::from_static("ccswitch-key"),
        );
        source.insert("x-api-key", HeaderValue::from_static("third-party-key"));
        source.insert(
            http::header::COOKIE,
            HeaderValue::from_static("session=third-party"),
        );
        source.insert(
            http::header::AUTHORIZATION,
            HeaderValue::from_static("Bearer third-party"),
        );
        source.insert("x-provider-secret", HeaderValue::from_static("secret"));

        let filtered = filter_live_headers(&source, true);

        assert!(filtered.contains_key(http::header::CONTENT_TYPE));
        assert!(filtered.contains_key(live_attestation::HEADER_NAME));
        assert!(!filtered.contains_key("x-api-key"));
        assert!(!filtered.contains_key(http::header::COOKIE));
        assert!(!filtered.contains_key(http::header::AUTHORIZATION));
        assert!(!filtered.contains_key("x-provider-secret"));
    }

    #[test]
    fn live_client_requires_loopback_host_and_codex_attestation() {
        let mut headers = HeaderMap::new();
        headers.insert(
            http::header::HOST,
            HeaderValue::from_static("127.0.0.1:15721"),
        );
        headers.insert(
            live_attestation::HEADER_NAME,
            HeaderValue::from_static("ccswitch-key"),
        );
        assert!(validate_live_client_headers(&headers, true).is_ok());

        headers.remove(live_attestation::HEADER_NAME);
        assert!(validate_live_client_headers(&headers, true).is_err());
        headers.insert(
            live_attestation::HEADER_NAME,
            HeaderValue::from_static("ccswitch-key"),
        );
        headers.insert(
            http::header::ORIGIN,
            HeaderValue::from_static("https://attacker.example"),
        );
        assert!(validate_live_client_headers(&headers, true).is_err());
        headers.remove(http::header::ORIGIN);
        headers.insert(
            http::header::HOST,
            HeaderValue::from_static("attacker.example"),
        );
        assert!(validate_live_client_headers(&headers, true).is_err());
    }

    #[test]
    fn live_sideband_rejects_browser_fetch_metadata() {
        let mut headers = HeaderMap::new();
        headers.insert(http::header::HOST, HeaderValue::from_static("[::1]:15721"));
        assert!(validate_live_client_headers(&headers, false).is_ok());
        headers.insert("sec-fetch-mode", HeaderValue::from_static("websocket"));
        assert!(validate_live_client_headers(&headers, false).is_err());
    }

    #[test]
    fn parses_current_codex_chatgpt_credentials() {
        let (token, account_id) = parse_current_codex_credentials(
            r#"{"auth_mode":"chatgpt","tokens":{"access_token":"fresh-token","account_id":"official-account","refresh_token":"unused"}}"#,
        )
        .expect("parse current Codex credentials");

        assert_eq!(token, "fresh-token");
        assert_eq!(account_id.as_deref(), Some("official-account"));
        assert!(account_binding_matches(
            Some("official-account"),
            account_id.as_deref()
        ));
        assert!(!account_binding_matches(
            Some("different-account"),
            account_id.as_deref()
        ));
    }

    #[test]
    fn rejects_non_chatgpt_codex_credentials() {
        let error = parse_current_codex_credentials(
            r#"{"auth_mode":"apikey","tokens":{"access_token":"not-oauth"}}"#,
        )
        .expect_err("reject non-ChatGPT auth mode");

        assert_eq!(error, "Codex is not logged in with ChatGPT OAuth");
    }

    #[test]
    fn rejects_expired_codex_access_tokens() {
        fn token_with_exp(exp: i64) -> String {
            let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
                .encode(serde_json::json!({"exp": exp}).to_string());
            format!("header.{payload}.signature")
        }

        assert!(!access_token_is_fresh(&token_with_exp(
            chrono::Utc::now().timestamp() - 60
        )));
        assert!(access_token_is_fresh(&token_with_exp(
            chrono::Utc::now().timestamp() + 3600
        )));
        assert!(!access_token_is_fresh("not-a-jwt"));
    }

    #[test]
    fn uses_provider_specific_live_call_paths() {
        let mut official = test_provider(CODEX_OFFICIAL_PROVIDER_ID, None);
        official.category = Some("official".to_string());
        let mut third_party = test_provider("third-party", None);
        third_party.meta = Some(crate::provider::ProviderMeta {
            codex_live: Some(crate::provider::CodexLiveConfig {
                enabled: true,
                create_endpoint: Some("voice/live".to_string()),
                sideband_endpoint: Some("voice/live/{call_id}".to_string()),
            }),
            ..Default::default()
        });

        let official = live_route_for_provider(official).expect("official route");
        let third_party = live_route_for_provider(third_party).expect("third-party route");
        assert_eq!(official.create_endpoint, "realtime/calls");
        assert_eq!(official.sideband_endpoint, "realtime/calls/{call_id}");
        assert!(official.official);
        assert_eq!(third_party.create_endpoint, "voice/live");
        assert_eq!(third_party.sideband_endpoint, "voice/live/{call_id}");
        assert!(!third_party.official);
        assert_eq!(
            render_sideband_endpoint(&third_party.sideband_endpoint, "rtc_test").unwrap(),
            "voice/live/rtc_test"
        );
    }

    #[test]
    fn third_party_live_is_opt_in_and_endpoints_are_relative() {
        let provider = test_provider("third-party", None);
        assert!(live_route_for_provider(provider).is_err());

        let mut full_url_provider = test_provider("full-url", None);
        full_url_provider.meta = Some(crate::provider::ProviderMeta {
            is_full_url: Some(true),
            codex_live: Some(crate::provider::CodexLiveConfig {
                enabled: true,
                create_endpoint: None,
                sideband_endpoint: None,
            }),
            ..Default::default()
        });
        assert!(matches!(
            live_route_for_provider(full_url_provider),
            Err(ProxyError::ConfigError(message)) if message.contains("full request URL")
        ));

        assert!(normalize_live_endpoint(Some("https://evil.test/live"), "live", false).is_err());
        assert!(normalize_live_endpoint(Some("live/../admin"), "live", false).is_err());
        assert!(normalize_live_endpoint(Some("live/no-template"), "live/{call_id}", true).is_err());
    }

    #[test]
    fn live_listener_accepts_only_loopback_addresses() {
        assert!(is_loopback_address("127.0.0.1"));
        assert!(is_loopback_address("::1"));
        assert!(is_loopback_address("[::1]"));
        assert!(is_loopback_address("localhost"));
        assert!(!is_loopback_address("0.0.0.0"));
        assert!(!is_loopback_address("192.168.1.10"));
    }

    #[test]
    fn successful_live_location_is_rewritten_to_the_local_route() {
        assert_eq!(
            local_sideband_location("/v1/live", "rtc_test", None)
                .unwrap()
                .to_str()
                .unwrap(),
            "/v1/live/rtc_test"
        );
        assert_eq!(
            local_sideband_location(
                "/v1/live",
                "rtc_test",
                Some("intent=quicksilver&token=a%2Fb")
            )
            .unwrap()
            .to_str()
            .unwrap(),
            "/v1/live/rtc_test?intent=quicksilver&token=a%2Fb"
        );
        assert!(local_sideband_location("/v1/live", "../escape", None).is_err());
        assert_eq!(
            live_invalid_upstream_response().status(),
            StatusCode::BAD_GATEWAY
        );
    }

    #[test]
    fn falls_back_only_for_official_availability_failures() {
        assert!(should_fallback_from_official(
            http::StatusCode::UNAUTHORIZED
        ));
        assert!(should_fallback_from_official(http::StatusCode::FORBIDDEN));
        assert!(should_fallback_from_official(
            http::StatusCode::TOO_MANY_REQUESTS
        ));
        assert!(!should_fallback_from_official(
            http::StatusCode::BAD_GATEWAY
        ));
        assert!(!should_fallback_from_official(
            http::StatusCode::SERVICE_UNAVAILABLE
        ));
        assert!(!should_fallback_from_official(
            http::StatusCode::BAD_REQUEST
        ));
        assert!(!should_fallback_from_official(http::StatusCode::OK));
    }

    #[test]
    fn only_an_initial_url_connect_failure_is_known_unsent() {
        let initial = url::Url::parse("https://example.com/v1/live").unwrap();
        let redirected = url::Url::parse("https://redirect.example/v1/live").unwrap();

        assert!(connect_failure_before_initial_send(
            true,
            Some(&initial),
            initial.as_str()
        ));
        assert!(!connect_failure_before_initial_send(
            true,
            Some(&redirected),
            initial.as_str()
        ));
        assert!(!connect_failure_before_initial_send(
            true,
            None,
            initial.as_str()
        ));
        assert!(!connect_failure_before_initial_send(
            false,
            Some(&initial),
            initial.as_str()
        ));
    }

    #[tokio::test]
    async fn converts_api_multipart_to_chatgpt_backend_json() {
        let boundary = "codex-realtime-call-boundary";
        let multipart = format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"sdp\"\r\nContent-Type: application/sdp\r\n\r\nv=offer\r\n\r\n--{boundary}\r\nContent-Disposition: form-data; name=\"session\"\r\nContent-Type: application/json\r\n\r\n{{\"model\":\"gpt-realtime\"}}\r\n--{boundary}--\r\n"
        );
        let mut headers = HeaderMap::new();
        headers.insert(
            http::header::CONTENT_TYPE,
            http::HeaderValue::from_str(&format!("multipart/form-data; boundary={boundary}"))
                .unwrap(),
        );

        let (headers, body, converted) =
            prepare_live_call_request(true, headers, bytes::Bytes::from(multipart))
                .await
                .expect("convert multipart");

        assert!(converted);
        assert_eq!(
            headers.get(http::header::CONTENT_TYPE).unwrap(),
            "application/json"
        );
        let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body["sdp"], "v=offer\r\n");
        assert_eq!(body["session"]["model"], "gpt-realtime");
    }

    #[tokio::test]
    async fn preserves_api_multipart_for_third_party_live() {
        let boundary = "codex-third-party-live-boundary";
        let multipart = format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"sdp\"\r\nContent-Type: application/sdp\r\n\r\nv=offer\r\n\r\n--{boundary}\r\nContent-Disposition: form-data; name=\"session\"\r\nContent-Type: application/json\r\n\r\n{{\"model\":\"gpt-realtime\"}}\r\n--{boundary}--\r\n"
        );
        let mut headers = HeaderMap::new();
        let content_type = format!("multipart/form-data; boundary={boundary}");
        headers.insert(
            http::header::CONTENT_TYPE,
            http::HeaderValue::from_str(&content_type).unwrap(),
        );
        let original_body = bytes::Bytes::from(multipart);
        let (headers, body, converted) =
            prepare_live_call_request(false, headers, original_body.clone())
                .await
                .expect("preserve multipart");

        assert!(!converted);
        assert_eq!(
            headers.get(http::header::CONTENT_TYPE).unwrap(),
            &content_type
        );
        assert_eq!(body, original_body);
    }

    fn test_provider(name: &str, provider_type: Option<&str>) -> Provider {
        let mut provider = Provider::with_id(
            name.to_string(),
            name.to_string(),
            serde_json::json!({"base_url": "https://example.com/v1"}),
            None,
        );
        if let Some(provider_type) = provider_type {
            provider.meta = Some(crate::provider::ProviderMeta {
                provider_type: Some(provider_type.to_string()),
                ..Default::default()
            });
        }
        provider
    }

    fn test_live_binding() -> LiveCallBinding {
        LiveCallBinding {
            route: live_route_for_provider({
                let mut provider = test_provider(CODEX_OFFICIAL_PROVIDER_ID, None);
                provider.category = Some("official".to_string());
                provider
            })
            .expect("official route"),
            attestation: None,
            created_at: Instant::now(),
            generation: uuid::Uuid::new_v4(),
            phase: LiveCallBindingPhase::Available,
        }
    }
}
