use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ProxyError {
    #[error("请求体超过大小上限: {0} 字节")]
    RequestBodyTooLarge(usize),

    #[error("上游响应体超过大小上限: {0} 字节")]
    ResponseBodyTooLarge(usize),

    #[error("服务器已在运行")]
    AlreadyRunning,

    #[error("服务器未运行")]
    NotRunning,

    #[error("地址绑定失败: {0}")]
    BindFailed(String),

    #[error("停止超时")]
    StopTimeout,

    #[error("停止失败: {0}")]
    StopFailed(String),

    #[error("请求转发失败: {0}")]
    ForwardFailed(String),

    #[error("无可用的Provider")]
    NoAvailableProvider,

    #[error("所有供应商已熔断，无可用渠道")]
    AllProvidersCircuitOpen,

    #[error("未配置供应商")]
    NoProvidersConfigured,

    #[error("上游错误 (状态码 {status}): {body:?}")]
    UpstreamError { status: u16, body: Option<String> },

    #[error("超过最大重试次数")]
    MaxRetriesExceeded,

    #[error("数据库错误: {0}")]
    DatabaseError(String),

    #[error("配置错误: {0}")]
    ConfigError(String),

    #[error("格式转换错误: {0}")]
    TransformError(String),

    #[error("无效的请求: {0}")]
    InvalidRequest(String),

    #[error("超时: {0}")]
    Timeout(String),

    /// 认证错误
    #[error("认证失败: {0}")]
    AuthError(String),

    /// Local pre-exhaustion policy, never an upstream usage-limit response.
    #[error("Codex 官方额度保护 (quota protection): {0}")]
    CodexQuotaProtection(String),

    /// 「暂停使用官方登录」开着：Codex 不带官方登录，官方请求在本地拒绝。
    #[error("已暂停使用 Codex 官方登录，官方请求不会发出。请在模型菜单换成第三方模型；官方额度恢复后，在 CC Switch 的 Codex 页面点「恢复官方登录」，然后重启 Codex。 (The Codex official login is paused, so official requests are not sent. Pick a third-party model in the model menu; once the official quota is back, click \"Resume official login\" on the Codex page in CC Switch, then restart Codex.)")]
    CodexOfficialLoginPaused,

    #[error("内部错误: {0}")]
    Internal(String),
}

impl IntoResponse for ProxyError {
    fn into_response(self) -> Response {
        let (status, body) = match &self {
            ProxyError::UpstreamError {
                status: upstream_status,
                body: upstream_body,
            } => {
                let http_status =
                    StatusCode::from_u16(*upstream_status).unwrap_or(StatusCode::BAD_GATEWAY);

                // 尝试解析上游响应体为 JSON，如果失败则包装为字符串
                let error_body = if let Some(body_str) = upstream_body {
                    if let Ok(json_body) = serde_json::from_str::<serde_json::Value>(body_str) {
                        // 上游返回的是 JSON，直接透传
                        json_body
                    } else {
                        // 上游返回的不是 JSON，包装为错误消息
                        json!({
                            "error": {
                                "message": body_str,
                                "type": "upstream_error",
                            }
                        })
                    }
                } else {
                    json!({
                        "error": {
                            "message": format!("Upstream error (status {})", upstream_status),
                            "type": "upstream_error",
                        }
                    })
                };

                (http_status, error_body)
            }
            _ => {
                let (http_status, message) = match &self {
                    ProxyError::AlreadyRunning => (StatusCode::CONFLICT, self.to_string()),
                    ProxyError::NotRunning => (StatusCode::SERVICE_UNAVAILABLE, self.to_string()),
                    ProxyError::BindFailed(_) => {
                        (StatusCode::INTERNAL_SERVER_ERROR, self.to_string())
                    }
                    ProxyError::StopTimeout => {
                        (StatusCode::INTERNAL_SERVER_ERROR, self.to_string())
                    }
                    ProxyError::StopFailed(_) => {
                        (StatusCode::INTERNAL_SERVER_ERROR, self.to_string())
                    }
                    ProxyError::ForwardFailed(_) => (StatusCode::BAD_GATEWAY, self.to_string()),
                    ProxyError::NoAvailableProvider => {
                        (StatusCode::SERVICE_UNAVAILABLE, self.to_string())
                    }
                    ProxyError::AllProvidersCircuitOpen => {
                        (StatusCode::SERVICE_UNAVAILABLE, self.to_string())
                    }
                    ProxyError::NoProvidersConfigured => {
                        (StatusCode::SERVICE_UNAVAILABLE, self.to_string())
                    }
                    ProxyError::MaxRetriesExceeded => {
                        (StatusCode::SERVICE_UNAVAILABLE, self.to_string())
                    }
                    ProxyError::DatabaseError(_) => {
                        (StatusCode::INTERNAL_SERVER_ERROR, self.to_string())
                    }
                    ProxyError::ConfigError(_) => (StatusCode::BAD_REQUEST, self.to_string()),
                    ProxyError::TransformError(_) => {
                        (StatusCode::UNPROCESSABLE_ENTITY, self.to_string())
                    }
                    ProxyError::InvalidRequest(_) => (StatusCode::BAD_REQUEST, self.to_string()),
                    ProxyError::Timeout(_) => (StatusCode::GATEWAY_TIMEOUT, self.to_string()),
                    ProxyError::AuthError(_) => (StatusCode::UNAUTHORIZED, self.to_string()),
                    ProxyError::CodexQuotaProtection(_) | ProxyError::CodexOfficialLoginPaused => {
                        (StatusCode::FORBIDDEN, self.to_string())
                    }
                    ProxyError::Internal(_) => {
                        (StatusCode::INTERNAL_SERVER_ERROR, self.to_string())
                    }
                    ProxyError::RequestBodyTooLarge(_) => {
                        (StatusCode::PAYLOAD_TOO_LARGE, self.to_string())
                    }
                    ProxyError::ResponseBodyTooLarge(_) => {
                        (StatusCode::BAD_GATEWAY, self.to_string())
                    }
                    ProxyError::UpstreamError { .. } => unreachable!(),
                };

                let error_body = json!({
                    "error": {
                        "message": message,
                        "type": "proxy_error",
                    }
                });

                (http_status, error_body)
            }
        };

        (status, Json(body)).into_response()
    }
}

/// 错误分类
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCategory {
    /// 可重试错误（网络问题、5xx）
    Retryable, // 网络超时、5xx 错误
    /// 不可重试错误（4xx、认证失败）
    NonRetryable, // 认证失败、参数错误、4xx 错误
}

/// 把错误连同 `source()` 原因链拼成一行。
///
/// reqwest / hyper 的外层文本只有「error sending request」「error decoding response body」
/// 这类描述，真正能区分 DNS、TCP、TLS、连接被重置的信息都在原因链里。
pub fn describe_error_chain(error: &(dyn std::error::Error + 'static)) -> String {
    let mut text = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        let cause_text = cause.to_string();
        // 有的错误会把原因也写进自己的文本，重复的就不再追加
        if !cause_text.is_empty() && !text.ends_with(&cause_text) {
            text.push_str(": ");
            text.push_str(&cause_text);
        }
        source = cause.source();
    }
    text
}

/// reqwest 错误的文本带完整 URL（查询参数里可能有 Key），先去掉 URL 再展开原因链。
pub fn describe_reqwest_error(error: reqwest::Error) -> String {
    describe_error_chain(&error.without_url())
}

/// 连不上的原因，给界面挑一句用户看得懂的提示用。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NetworkErrorKind {
    /// 系统拒绝发起连接：多半是防火墙或安全软件拦了本程序（Windows 10013、Unix EACCES/EPERM）
    Blocked,
    /// 地址到得了，但端口上没有服务
    Refused,
    Timeout,
    Dns,
    Tls,
    Other,
}

/// 沿 `source()` 链判断连不上的原因。
///
/// 按 `io::ErrorKind` 判断而不是匹配错误文本：系统给的错误文本是本地化的（中文 Windows 上
/// 10013 是「以一种访问权限不允许的方式…」）。DNS 和 TLS 没有对应的 ErrorKind，只能认
/// hyper / rustls 自己的英文文本。
pub fn classify_error_chain(error: &(dyn std::error::Error + 'static)) -> NetworkErrorKind {
    let (mut dns, mut blocked, mut refused, mut timeout, mut tls) =
        (false, false, false, false, false);
    let mut current = Some(error);
    while let Some(cause) = current {
        if let Some(io) = cause.downcast_ref::<std::io::Error>() {
            match io.kind() {
                std::io::ErrorKind::PermissionDenied => blocked = true,
                std::io::ErrorKind::ConnectionRefused => refused = true,
                std::io::ErrorKind::TimedOut => timeout = true,
                _ => {}
            }
        }
        let text = cause.to_string().to_ascii_lowercase();
        if text.starts_with("dns error") || text.contains("failed to lookup address") {
            dns = true;
        }
        if text.contains("certificate") || text.contains("tls") || text.contains("handshake") {
            tls = true;
        }
        current = cause.source();
    }
    if dns {
        NetworkErrorKind::Dns
    } else if blocked {
        NetworkErrorKind::Blocked
    } else if refused {
        NetworkErrorKind::Refused
    } else if timeout {
        NetworkErrorKind::Timeout
    } else if tls {
        NetworkErrorKind::Tls
    } else {
        NetworkErrorKind::Other
    }
}

pub fn classify_reqwest_error(error: &reqwest::Error) -> NetworkErrorKind {
    if error.is_timeout() {
        return NetworkErrorKind::Timeout;
    }
    classify_error_chain(error)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct Outer(std::io::Error);

    impl std::fmt::Display for Outer {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("error sending request")
        }
    }

    impl std::error::Error for Outer {
        fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
            Some(&self.0)
        }
    }

    #[test]
    fn describe_error_chain_appends_every_cause() {
        let error = Outer(std::io::Error::new(
            std::io::ErrorKind::ConnectionRefused,
            "tcp connect error",
        ));
        assert_eq!(
            describe_error_chain(&error),
            "error sending request: tcp connect error"
        );
    }

    #[tokio::test]
    async fn describe_reqwest_error_keeps_cause_and_drops_url() {
        // 先占一个端口再释放，保证连接被拒绝
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);

        let error = reqwest::Client::new()
            .get(format!(
                "http://127.0.0.1:{port}/v1/responses?key=secret-token"
            ))
            .send()
            .await
            .expect_err("connection should be refused");
        assert!(error.is_connect());

        assert_eq!(classify_reqwest_error(&error), NetworkErrorKind::Refused);
        let text = describe_reqwest_error(error);
        assert!(text.starts_with("error sending request: "), "{text}");
        assert!(!text.contains("secret-token"), "{text}");
    }

    /// 外层文本可指定的错误，模拟 hyper_util 的 ConnectError（「dns error」「tcp connect error」）
    #[derive(Debug)]
    struct Labeled(&'static str, Option<std::io::Error>);

    impl std::fmt::Display for Labeled {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str(self.0)
        }
    }

    impl std::error::Error for Labeled {
        fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
            self.1
                .as_ref()
                .map(|e| e as &(dyn std::error::Error + 'static))
        }
    }

    fn io(kind: std::io::ErrorKind, text: &str) -> Option<std::io::Error> {
        Some(std::io::Error::new(kind, text.to_string()))
    }

    #[test]
    fn classify_error_chain_reads_io_kind_not_localized_text() {
        use std::io::ErrorKind;
        let cases = [
            (
                Labeled(
                    "tcp connect error",
                    io(
                        ErrorKind::PermissionDenied,
                        "以一种访问权限不允许的方式做了一个访问套接字的尝试。 (os error 10013)",
                    ),
                ),
                NetworkErrorKind::Blocked,
            ),
            (
                Labeled("tcp connect error", io(ErrorKind::ConnectionRefused, "x")),
                NetworkErrorKind::Refused,
            ),
            (
                Labeled("tcp connect error", io(ErrorKind::TimedOut, "x")),
                NetworkErrorKind::Timeout,
            ),
            (
                Labeled("dns error", io(ErrorKind::Other, "x")),
                NetworkErrorKind::Dns,
            ),
            (
                Labeled(
                    "error sending request",
                    io(
                        ErrorKind::InvalidData,
                        "invalid peer certificate: UnknownIssuer",
                    ),
                ),
                NetworkErrorKind::Tls,
            ),
            (
                Labeled("connection reset", io(ErrorKind::ConnectionReset, "x")),
                NetworkErrorKind::Other,
            ),
        ];
        for (error, expected) in cases {
            assert_eq!(classify_error_chain(&error), expected, "{error:?}");
        }
    }
}
