use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

#[derive(Clone, Default)]
struct MockState {
    fail_usage: bool,
    requires_2fa: bool,
    login_body: Arc<Mutex<Option<Value>>>,
    usage_authorization: Arc<Mutex<Option<String>>>,
    logout_authorization: Arc<Mutex<Option<String>>>,
    logout_body: Arc<Mutex<Option<Value>>>,
}

async fn login(State(state): State<MockState>, Json(body): Json<Value>) -> Json<Value> {
    *state.login_body.lock().unwrap() = Some(body);
    if state.requires_2fa {
        return Json(json!({
            "code": 0,
            "message": "success",
            "data": {
                "requires_2fa": true,
                "temp_token": "temporary-login-token",
                "user_email_masked": "p***@example.com"
            }
        }));
    }
    Json(json!({
        "code": 0,
        "message": "success",
        "data": {
            "access_token": "access-123",
            "refresh_token": "refresh-456",
            "expires_in": 86400,
            "token_type": "Bearer",
            "user": {
                "balance": 9.75,
                "status": "active"
            }
        }
    }))
}

async fn usage(State(state): State<MockState>, headers: HeaderMap) -> impl IntoResponse {
    *state.usage_authorization.lock().unwrap() = headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    if state.fail_usage {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"code": 500, "message": "usage unavailable"})),
        );
    }
    (
        StatusCode::OK,
        Json(json!({
            "code": 0,
            "message": "success",
            "data": {
                "total_actual_cost": 0.25
            }
        })),
    )
}

async fn logout(
    State(state): State<MockState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Json<Value> {
    *state.logout_authorization.lock().unwrap() = headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    *state.logout_body.lock().unwrap() = Some(body);
    Json(json!({"code": 0, "message": "success", "data": {}}))
}

async fn spawn_server(
    fail_usage: bool,
    requires_2fa: bool,
) -> (String, MockState, tokio::task::JoinHandle<()>) {
    let state = MockState {
        fail_usage,
        requires_2fa,
        ..MockState::default()
    };
    let app = Router::new()
        .route("/api/v1/auth/login", post(login))
        .route("/api/v1/usage/dashboard/stats", get(usage))
        .route("/api/v1/auth/logout", post(logout))
        .with_state(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock Sub2API server");
    let address = listener.local_addr().expect("mock server address");
    let handle = tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("serve mock Sub2API requests");
    });
    (format!("http://{address}"), state, handle)
}

#[tokio::test]
async fn queries_exact_usage_and_revokes_the_login_session() {
    let (base_url, state, server) = spawn_server(false, false).await;

    let result = super::sub2api::get_usage(&base_url, "person@example.com", "account-password", 10)
        .await
        .expect("Sub2API query should complete");

    assert!(result.success);
    let data = result.data.expect("usage data");
    assert_eq!(data.len(), 1);
    assert_eq!(data[0].plan_name, None);
    assert_eq!(data[0].remaining, Some(9.75));
    assert_eq!(data[0].used, Some(0.25));
    assert_eq!(data[0].total, None);
    assert_eq!(data[0].unit.as_deref(), Some("USD"));
    assert_eq!(data[0].is_valid, Some(true));

    assert_eq!(
        state.login_body.lock().unwrap().as_ref(),
        Some(&json!({
            "email": "person@example.com",
            "password": "account-password"
        }))
    );
    assert_eq!(
        state.usage_authorization.lock().unwrap().as_deref(),
        Some("Bearer access-123")
    );
    assert_eq!(
        state.logout_authorization.lock().unwrap().as_deref(),
        Some("Bearer access-123")
    );
    assert_eq!(
        state.logout_body.lock().unwrap().as_ref(),
        Some(&json!({"refresh_token": "refresh-456"}))
    );

    server.abort();
}

#[tokio::test]
async fn revokes_the_login_session_when_usage_query_fails() {
    let (base_url, state, server) = spawn_server(true, false).await;

    let result = super::sub2api::get_usage(&base_url, "person@example.com", "account-password", 10)
        .await
        .expect("deterministic API failure should remain a usage result");

    assert!(!result.success);
    assert_eq!(
        state.logout_authorization.lock().unwrap().as_deref(),
        Some("Bearer access-123")
    );
    assert_eq!(
        state.logout_body.lock().unwrap().as_ref(),
        Some(&json!({"refresh_token": "refresh-456"}))
    );

    server.abort();
}

#[tokio::test]
async fn reports_that_interactive_two_factor_login_is_unsupported() {
    let (base_url, _state, server) = spawn_server(false, true).await;

    let result = super::sub2api::get_usage(&base_url, "person@example.com", "account-password", 10)
        .await
        .expect("2FA response should be a deterministic usage result");

    assert!(!result.success);
    assert!(
        result.error.as_deref().unwrap_or_default().contains("2FA"),
        "error should explain the unsupported login flow: {:?}",
        result.error
    );

    server.abort();
}

#[tokio::test]
async fn rejects_missing_credentials_before_sending_a_request() {
    let cases = [
        ("", "person@example.com", "account-password", "base URL"),
        (
            "https://sub2api.example.com",
            "",
            "account-password",
            "email",
        ),
        (
            "https://sub2api.example.com",
            "person@example.com",
            "",
            "password",
        ),
    ];

    for (base_url, email, password, expected) in cases {
        let result = super::sub2api::get_usage(base_url, email, password, 10)
            .await
            .expect("missing configuration should be a deterministic usage result");
        assert!(!result.success);
        let error = result.error.unwrap_or_default();
        assert!(error.contains(expected), "error={error:?}");
        assert!(!error.contains("account-password"));
    }
}
