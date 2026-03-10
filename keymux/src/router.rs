//! HTTP router and request handlers

use crate::acl_vault::{AclKeyVault, ProviderKey};
use crate::ranker::Ranker;
use crate::types::{ChatCompletionRequest, ChatCompletionResponse, ModelId, RankContext, CapabilityFlags, CarrierMetrics};
use crate::openapi::PathRegistry;
use crate::feed::{FeedBroadcaster, FeedConfig, create_feed_stream, sse_content_type};
use anyhow::Result;
use axum::{
    extract::State,
    http::{StatusCode, header},
    routing::{get, post},
    Json, Router,
    response::{IntoResponse, sse::Sse},
};
use log::{info, error, debug};
use std::net::SocketAddr;
use std::sync::Arc;

/// Application state shared across handlers
pub struct AppState {
    pub key_vault: Arc<AclKeyVault>,
    pub ranker: Arc<dyn Ranker>,
    pub feed: Arc<FeedBroadcaster>,
    pub openapi_registry: Arc<PathRegistry>,
}

/// Start TCP/HTTP server
pub async fn start_tcp_server(
    addr: SocketAddr,
    key_vault: Arc<AclKeyVault>,
    ranker: Arc<dyn Ranker>,
) -> Result<()> {
    let feed = Arc::new(FeedBroadcaster::new(FeedConfig::default()));
    let openapi_registry = Arc::new(PathRegistry::new());
    
    let state = AppState {
        key_vault,
        ranker,
        feed,
        openapi_registry,
    };
    
    let app = Router::new()
        .route("/v1/chat/completions", post(handle_chat_completions))
        .route("/v1/models", get(handle_models))
        .route("/v1/embeddings", post(handle_embeddings))
        .route("/openapi.json", get(handle_openapi))
        .route("/feed", get(handle_feed))
        .route("/health", get(health_check))
        .route("/ready", get(health_check))
        .with_state(Arc::new(state));
    
    info!("  TCP/HTTP server listening on {}", addr);
    
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    
    Ok(())
}

/// Health check endpoint
async fn health_check(
    State(state): State<Arc<AppState>>,
) -> (StatusCode, Json<serde_json::Value>) {
    let total_keys = state.key_vault.list_keys().len();
    let provider_count = state.key_vault.list_providers().len();
    let ready = total_keys > 0;
    let status = if ready { "healthy" } else { "degraded" };

    let code = if ready {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };

    (
        code,
        Json(serde_json::json!({
            "status": status,
            "ready": ready,
            "providers": provider_count,
            "keys": total_keys,
            "timestamp": chrono::Utc::now().to_rfc3339(),
        })),
    )
}

/// OpenAPI spec endpoint
async fn handle_openapi(
    State(state): State<Arc<AppState>>,
) -> Json<crate::openapi::OpenApiDocument> {
    let doc = state.openapi_registry.build_document("http://localhost:8888");
    Json(doc)
}

/// Feed endpoint (SSE)
async fn handle_feed(
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    let receiver = state.feed.subscribe();
    let config = state.feed.config().clone();
    let stream = create_feed_stream(receiver, config);
    
    (
        [(header::CONTENT_TYPE, sse_content_type())],
        Sse::new(stream)
    )
}

/// Handle /v1/chat/completions
async fn handle_chat_completions(
    State(state): State<Arc<AppState>>,
    Json(request): Json<ChatCompletionRequest>,
) -> Result<(StatusCode, Json<ChatCompletionResponse>), StatusCode> {
    debug!("Received chat completion request for model: {}", request.model);
    
    // Parse model identifier (/provider/model syntax)
    let model_id = ModelId::parse(&request.model)
        .ok_or_else(|| {
            error!("Invalid model format: {}", request.model);
            StatusCode::BAD_REQUEST
        })?;
    
    debug!("  Provider: {}, Model: {}", model_id.provider, model_id.model);
    
    // Get available keys for provider
    let keys = state.key_vault.get_keys_for_provider(&model_id.provider);
    
    if keys.is_empty() {
        error!("No keys available for provider: {}", model_id.provider);
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    }
    
    // Select best key using ranker
    let best_key_index = crate::ranker::select_best_key(
        state.ranker.as_ref(),
        &keys,
        |key| {
            // Get ranker metrics for this key
            let (latency, cost) = state.key_vault.get_key(&key.id)
                .and_then(|_| None) // TODO: load from metrics store
                .unwrap_or((100.0, 0.000001));
            
            RankContext {
                provider: key.provider.clone(),
                model: model_id.model.clone(),
                key_id: key.id.clone(),
                observed_latency_ms: latency,
                cost_per_token: cost,
                capability_flags: CapabilityFlags::default(),
                quota_remaining: key.quota_limit.map(|l| 1.0 - (key.quota_used / l)).unwrap_or(1.0),
                carrier_quality: CarrierMetrics::default(),
            }
        },
    );
    
    let selected_key = &keys[best_key_index.unwrap_or(0)];
    
    debug!("  Selected key: {}", selected_key.id);
    
    // Forward request to upstream provider
    let response = forward_to_upstream(&model_id, &request, selected_key)
        .await
        .map_err(|e| {
            error!("Failed to forward request: {}", e);
            StatusCode::BAD_GATEWAY
        })?;
    
    // Update quota usage
    if let Some(usage) = &response.usage {
        let mut vault_clone = state.key_vault.clone();
        // Note: AclKeyVault uses interior mutability for quota updates
        // In production, use a proper mutex or atomic updates
        drop(vault_clone); // Placeholder - implement proper quota tracking
    }
    
    Ok((StatusCode::OK, Json(response)))
}

/// Handle /v1/models
async fn handle_models(
    State(state): State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    // List all available models from configured providers
    let providers = state.key_vault.list_providers();
    
    let models: Vec<serde_json::Value> = providers.iter()
        .flat_map(|provider| {
            // TODO: Fetch actual model list from provider API
            vec![
                serde_json::json!({
                    "id": format!("{}/{}", provider, "default-model"),
                    "object": "model",
                    "created": chrono::Utc::now().timestamp(),
                    "owned_by": provider,
                })
            ]
        })
        .collect();
    
    Json(serde_json::json!({
        "object": "list",
        "data": models,
    }))
}

/// Handle /v1/embeddings (placeholder)
async fn handle_embeddings(
    State(_state): State<Arc<AppState>>,
    Json(_request): Json<serde_json::Value>,
) -> Json<serde_json::Value> {
    // TODO: Implement embeddings support
    Json(serde_json::json!({
        "object": "list",
        "data": [],
    }))
}

/// Forward request to upstream provider via LiteBike
async fn forward_to_upstream(
    model_id: &ModelId,
    request: &ChatCompletionRequest,
    key: &ProviderKey,
) -> Result<ChatCompletionResponse, reqwest::Error> {
    // LiteBike OpenAI-compatible endpoint
    let litebike_url = std::env::var("LITEBIKE_URL")
        .unwrap_or_else(|_| "http://localhost:8889/v1".to_string());
    
    // Build upstream URL based on provider
    let upstream_url = match model_id.provider.as_str() {
        "anthropic" => format!("{}/chat/completions", litebike_url),
        "openai" => format!("{}/chat/completions", litebike_url),
        "google" => format!("{}/chat/completions", litebike_url),
        _ => format!("{}/chat/completions", litebike_url), // Default to LiteBike
    };
    
    let client = reqwest::Client::new();
    
    // Transform request to provider-specific format
    let transformed_request = transform_request_for_provider(model_id, request)?;
    
    // Send request to LiteBike
    let response = client
        .post(&upstream_url)
        .header("Authorization", format!("Bearer {}", key.key))
        .header("Content-Type", "application/json")
        .json(&transformed_request)
        .send()
        .await?
        .error_for_status()?
        .json::<ChatCompletionResponse>()
        .await?;
    
    Ok(response)
}

/// Transform request to provider-specific format
fn transform_request_for_provider(
    model_id: &ModelId,
    request: &ChatCompletionRequest,
) -> Result<serde_json::Value, anyhow::Error> {
    match model_id.provider.as_str() {
        "anthropic" => {
            // Transform OpenAI format to Anthropic format
            Ok(serde_json::json!({
                "model": model_id.model,
                "max_tokens": request.max_tokens.unwrap_or(1024),
                "messages": request.messages.iter()
                    .filter(|m| m.role != "system")
                    .map(|m| serde_json::json!({
                        "role": if m.role == "assistant" { "assistant" } else { "user" },
                        "content": m.content.clone().unwrap_or_default(),
                    }))
                    .collect::<Vec<_>>(),
                "system": request.messages.iter()
                    .find(|m| m.role == "system")
                    .and_then(|m| m.content.clone()),
            }))
        }
        "google" => {
            // Transform to Google format
            Ok(serde_json::json!({
                "contents": request.messages.iter()
                    .filter(|m| m.role != "system")
                    .map(|m| serde_json::json!({
                        "role": if m.role == "assistant" { "model" } else { "user" },
                        "parts": [{ "text": m.content.clone().unwrap_or_default() }],
                    }))
                    .collect::<Vec<_>>(),
            }))
        }
        _ => {
            // Default: OpenAI-compatible (pass through)
            Ok(serde_json::to_value(request)?)
        }
    }
}
