use axum::{
    extract::{Query, State, WebSocketUpgrade},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::{error, info};

use crate::web::middleware::auth::validate_token;

pub struct WsState {
    pub tx: broadcast::Sender<String>,
}

impl WsState {
    pub fn new(tx: broadcast::Sender<String>) -> Self {
        Self { tx }
    }
}

pub async fn ws_handler(
    ws: WebSocketUpgrade,
    Query(params): Query<HashMap<String, String>>,
    State((_, ws_state)): State<(Arc<crate::web::models::app_state::AppState>, Arc<WsState>)>,
) -> Response {
    // Validate JWT token from query param (browsers cannot set custom headers for WebSockets).
    let token = params.get("token").map(|s| s.as_str()).unwrap_or("");
    if token.is_empty() || validate_token(token).is_err() {
        return (StatusCode::UNAUTHORIZED, "Missing or invalid token").into_response();
    }

    ws.on_upgrade(move |socket| handle_socket(socket, ws_state))
}

async fn handle_socket(mut socket: axum::extract::ws::WebSocket, ws_state: Arc<WsState>) {
    info!("New WebSocket connection established");

    let mut rx = ws_state.tx.subscribe();

    loop {
        tokio::select! {
            msg = rx.recv() => {
                match msg {
                    Ok(text) => {
                        if let Err(e) = socket.send(axum::extract::ws::Message::Text(text.into())).await {
                            error!("WebSocket send error: {}", e);
                            break;
                        }
                    }
                    Err(e) => {
                        error!("Broadcast channel error: {}", e);
                        break;
                    }
                }
            }
            msg = socket.recv() => {
                match msg {
                    Some(Ok(axum::extract::ws::Message::Close(_))) => {
                        info!("WebSocket connection closed by client");
                        break;
                    }
                    Some(Ok(_)) => {}
                    Some(Err(e)) => {
                        error!("WebSocket receive error: {}", e);
                        break;
                    }
                    None => {
                        info!("WebSocket connection closed");
                        break;
                    }
                }
            }
        }
    }
}

pub fn broadcast_event(ws_state: &WsState, event: &str, data: serde_json::Value) {
    let message = serde_json::json!({
        "event": event,
        "data": data,
        "timestamp": chrono::Utc::now().timestamp_millis()
    });

    if let Err(e) = ws_state.tx.send(message.to_string()) {
        error!("Failed to broadcast event: {}", e);
    }
}
