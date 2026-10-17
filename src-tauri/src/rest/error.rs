//! REST 控制面错误 → axum 响应。

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

#[derive(Debug)]
pub enum RestError {
    /// 业务错误（来自 `AppError` / service 层）
    App(String),
    /// 请求错误（参数解析、无效 app 等）
    Bad(String),
    /// 未授权
    Unauthorized,
    /// 未找到
    NotFound(String),
}

impl IntoResponse for RestError {
    fn into_response(self) -> Response {
        let (code, msg) = match self {
            RestError::App(s) => (StatusCode::INTERNAL_SERVER_ERROR, s),
            RestError::Bad(s) => (StatusCode::BAD_REQUEST, s),
            RestError::Unauthorized => (StatusCode::UNAUTHORIZED, "unauthorized".to_string()),
            RestError::NotFound(s) => (StatusCode::NOT_FOUND, s),
        };
        (code, msg).into_response()
    }
}

impl From<crate::error::AppError> for RestError {
    fn from(e: crate::error::AppError) -> Self {
        RestError::App(e.to_string())
    }
}

impl From<String> for RestError {
    fn from(s: String) -> Self {
        RestError::App(s)
    }
}

impl From<RestError> for Response {
    fn from(e: RestError) -> Response {
        e.into_response()
    }
}
