use crate::web::{
    middleware::auth::{generate_token, get_auth_token, revoke_token, validate_token},
    models::ApiResponse,
};
use axum::{extract::Request, routing::post, Json, Router};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub token: Option<String>,
    pub username: Option<String>,
    pub password: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct LoginResponse {
    pub token: String,
}

#[derive(Debug, Deserialize)]
pub struct VerifyRequest {
    pub token: String,
}

#[derive(Debug, Serialize)]
pub struct VerifyResponse {
    pub valid: bool,
}

pub fn routes() -> Router {
    Router::new()
        .route("/login", post(login_route))
        .route("/logout", post(logout_route))
        .route("/verify", post(verify_route))
        .route("/generate", post(generate_route))
}

async fn generate_route(request: Request) -> Json<ApiResponse<String>> {
    let provided = extract_bearer_token(&request);
    let expected = get_auth_token();
    let valid = provided
        .as_deref()
        .map(|p| constant_time_eq(p.as_bytes(), expected.as_bytes()))
        .unwrap_or(false);
    if !valid {
        return Json(ApiResponse::error("Invalid static token".to_string()));
    }

    match generate_token("admin") {
        Ok(token) => Json(ApiResponse::success(token)),
        Err(e) => Json(ApiResponse::error(format!(
            "Failed to generate token: {}",
            e
        ))),
    }
}

fn extract_bearer_token(request: &Request) -> Option<String> {
    request
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|header| {
            if header.starts_with("Bearer ") {
                Some(header[7..].to_string())
            } else {
                None
            }
        })
}

async fn login_route(Json(req): Json<LoginRequest>) -> Json<ApiResponse<LoginResponse>> {
    // Password-based login was removed; only token exchange is supported.
    if req.username.is_some() || req.password.is_some() {
        return Json(ApiResponse::error(
            "Password login is no longer supported. Use token login instead.".to_string(),
        ));
    }

    let provided = match req.token {
        Some(t) => t,
        None => return Json(ApiResponse::error("Missing auth token".to_string())),
    };

    let expected = get_auth_token();
    // Run both checks without short-circuiting to avoid timing side-channels
    // that would reveal whether the caller supplied a static token or a JWT.
    let static_valid = constant_time_eq(provided.as_bytes(), expected.as_bytes());
    let jwt_valid = validate_token(&provided).is_ok();
    if !static_valid && !jwt_valid {
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

async fn verify_route(Json(req): Json<VerifyRequest>) -> Json<ApiResponse<VerifyResponse>> {
    if req.token.trim().is_empty() {
        return Json(ApiResponse::error("Token is required".to_string()));
    }
    let valid = validate_token(&req.token).is_ok();
    Json(ApiResponse::success(VerifyResponse { valid }))
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
    use subtle::ConstantTimeEq;
    a.ct_eq(b).into()
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
            token: Some("test-login-secret".to_string()),
            username: None,
            password: None,
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
            token: Some("wrong-secret".to_string()),
            username: None,
            password: None,
        }))
        .await;

        let json = response.0;
        assert!(!json.success);

        unsafe { env::remove_var("AUTH_TOKEN") };
    }

    #[tokio::test]
    #[serial]
    async fn generate_route_requires_static_token() {
        crate::web::middleware::auth::reset_auth_token_cache();
        unsafe { env::set_var("AUTH_TOKEN", "generate-test-secret") };

        let request = axum::http::Request::builder()
            .uri("/auth/generate")
            .method("POST")
            .header("Authorization", "Bearer generate-test-secret")
            .body(axum::body::Body::empty())
            .unwrap();

        let response = generate_route(request).await;
        assert!(response.0.success, "generate failed: {:?}", response.0.error);
        assert!(!response.0.data.unwrap().is_empty());

        let bad_request = axum::http::Request::builder()
            .uri("/auth/generate")
            .method("POST")
            .header("Authorization", "Bearer wrong-secret")
            .body(axum::body::Body::empty())
            .unwrap();
        let bad_response = generate_route(bad_request).await;
        assert!(!bad_response.0.success);

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
