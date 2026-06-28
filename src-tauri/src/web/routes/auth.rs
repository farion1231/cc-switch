use crate::web::{
    middleware::auth::{generate_token, get_auth_token, revoke_token},
    models::ApiResponse,
};
use axum::{extract::Request, routing::post, Json, Router};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub token: String,
}

#[derive(Debug, Serialize)]
pub struct LoginResponse {
    pub token: String,
}

pub fn routes() -> Router {
    Router::new()
        .route("/login", post(login_route))
        .route("/logout", post(logout_route))
}

async fn login_route(Json(req): Json<LoginRequest>) -> Json<ApiResponse<LoginResponse>> {
    let expected = get_auth_token();

    if !constant_time_eq(req.token.as_bytes(), expected.as_bytes()) {
        return Json(ApiResponse::error("Invalid auth token".to_string()));
    }

    match generate_token("admin") {
        Ok(token) => Json(ApiResponse::success(LoginResponse { token })),
        Err(e) => Json(ApiResponse::error(format!(
            "Failed to generate token: {}",
            e
        ))),
    }
}

async fn logout_route(request: Request) -> Json<ApiResponse<()>> {
    let auth_header = request
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok());

    let token = match auth_header {
        Some(header) if header.starts_with("Bearer ") => &header[7..],
        _ => {
            return Json(ApiResponse::error(
                "Missing or invalid authorization header".to_string(),
            ));
        }
    };

    match revoke_token(token) {
        Ok(_) => Json(ApiResponse::success(())),
        Err(_) => Json(ApiResponse::error("Invalid token".to_string())),
    }
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::web::middleware::auth::{generate_token, is_jti_revoked, validate_token};
    use serial_test::serial;
    use std::env;

    #[tokio::test]
    #[serial]
    async fn login_route_returns_jwt_on_valid_token() {
        crate::web::middleware::auth::reset_auth_token_cache();
        unsafe { env::set_var("AUTH_TOKEN", "test-login-secret") };

        let response = login_route(Json(LoginRequest {
            token: "test-login-secret".to_string(),
        }))
        .await;

        let json = response.0;
        assert!(json.success);

        unsafe { env::remove_var("AUTH_TOKEN") };
    }

    #[tokio::test]
    #[serial]
    async fn login_route_rejects_invalid_token() {
        crate::web::middleware::auth::reset_auth_token_cache();
        unsafe { env::set_var("AUTH_TOKEN", "correct-secret") };

        let response = login_route(Json(LoginRequest {
            token: "wrong-secret".to_string(),
        }))
        .await;

        let json = response.0;
        assert!(!json.success);

        unsafe { env::remove_var("AUTH_TOKEN") };
    }

    #[tokio::test]
    #[serial]
    async fn logout_route_revokes_token() {
        crate::web::middleware::auth::reset_auth_token_cache();
        unsafe { env::set_var("AUTH_TOKEN", "logout-test-secret") };
        let _ = get_auth_token();

        let token = generate_token("admin").unwrap();
        let jti = validate_token(&token).unwrap().jti;

        let request = axum::http::Request::builder()
            .uri("/auth/logout")
            .method("POST")
            .header("Authorization", format!("Bearer {}", token))
            .body(axum::body::Body::empty())
            .unwrap();

        let response = logout_route(request).await;
        assert!(response.0.success);
        assert!(is_jti_revoked(&jti));

        unsafe { env::remove_var("AUTH_TOKEN") };
    }
}
