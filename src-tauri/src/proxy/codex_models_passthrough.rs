//! 官方路由下 Codex 的模型列表请求（`GET /models`）原样转给官方上游。
//!
//! 官方路由写成内置 openai + `openai_base_url` 指向本地路由（见 `codex_direct`），Codex
//! 拉模型列表也打到本地路由。官方的列表按账号返回：账号专属模型、套餐可用性、Daybreak
//! 权限（`available_access_programs`）都只在这里有。本地路由不转发时 Codex 只能退回它
//! 自带的列表，这些就都丢了（例如桌面版开了 Daybreak 后官方模型全部变灰）。
//!
//! 只转官方路由：第三方路由也可能写成内置 openai、请求带着 ChatGPT 登录，转出去会把
//! 官方的列表配给第三方。请求头和凭据都是 Codex 自己的，和直连官方时发出的一样；回包
//! 原样交回 Codex（304、401 也照交，由 Codex 自己处理）。连不上上游、被重定向到别处、
//! 回包过大时返回 `None`，调用方照旧回空列表，Codex 退回自带列表，不比不转差。
//!
//! 唯一的例外是压缩：Codex 拉列表不带 `Accept-Encoding`，官方就回 600 多 KB 明文，
//! 多走本地路由这一跳后常常超过 Codex 约 5 秒的等待上限（实测冷启动一半退回自带列表）。
//! 这时由本地路由向上游要 gzip（约 130 KB），收到后解开再交给 Codex，Codex 拿到的仍是
//! 它要的明文。Codex 自己带了 `Accept-Encoding` 时照旧原样转发、原样交回。

use std::time::Duration;

use axum::body::Body;
use axum::http::{HeaderMap, HeaderName, Uri};
use axum::response::Response;

use super::hyper_client::ProxyResponse;
use super::response_processor::strip_hop_by_hop_response_headers;

/// Codex 自己等 5 秒左右就放弃，这里不必更长。
const UPSTREAM_TIMEOUT: Duration = Duration::from_secs(10);
/// 官方列表几百 KB，留足余量。
const MAX_BODY_BYTES: usize = 8 * 1024 * 1024;
/// 占位凭据：路由暂停使用官方登录、或进程还没重载配置时 Codex 带的是它。
const PROXY_AUTH_PLACEHOLDER: &str = "PROXY_MANAGED";

/// 不转给上游的请求头：逐跳头、`Host`、`Content-Length`。
const SKIPPED_REQUEST_HEADERS: &[&str] = &[
    "host",
    "content-length",
    "connection",
    "keep-alive",
    "proxy-authorization",
    "proxy-connection",
    "te",
    "trailer",
    "transfer-encoding",
    "upgrade",
];

/// 当前路由是官方卡、且请求带着 Codex 自己的登录时，把请求转给官方列表地址。
pub(crate) async fn forward_if_official_route(
    db: &crate::database::Database,
    uri: &Uri,
    headers: &HeaderMap,
) -> Option<Response> {
    let route = crate::mode::current::provider_in_use(db, &crate::app_config::AppType::Codex)
        .ok()
        .flatten()?;
    if !super::providers::is_codex_official_provider(&route) {
        return None;
    }
    if crate::settings::codex_proxy_without_official_login() || !has_codex_login(headers) {
        return None;
    }
    forward(
        crate::services::codex_oauth_models::CODEX_OAUTH_MODELS_URL,
        uri,
        headers,
    )
    .await
}

/// 带着 Bearer 登录，而且不是占位凭据。
fn has_codex_login(headers: &HeaderMap) -> bool {
    headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| {
            let mut parts = value.split_whitespace();
            let scheme = parts.next()?;
            let token = parts.next()?;
            (scheme.eq_ignore_ascii_case("bearer") && parts.next().is_none()).then_some(token)
        })
        .is_some_and(|token| !token.contains(PROXY_AUTH_PLACEHOLDER))
}

/// `Connection` 里点名的头也是逐跳的。
fn connection_listed(headers: &HeaderMap) -> Vec<HeaderName> {
    headers
        .get_all(axum::http::header::CONNECTION)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .filter_map(|name| HeaderName::from_bytes(name.trim().as_bytes()).ok())
        .collect()
}

async fn forward(upstream: &str, uri: &Uri, headers: &HeaderMap) -> Option<Response> {
    let url = match uri.query() {
        Some(query) => format!("{upstream}?{query}"),
        None => upstream.to_string(),
    };
    let expected_host = reqwest::Url::parse(upstream).ok()?.host_str()?.to_string();
    let listed = connection_listed(headers);
    let mut request = super::http_client::get_for_url(&url)
        .get(&url)
        .timeout(UPSTREAM_TIMEOUT);
    for (name, value) in headers {
        if SKIPPED_REQUEST_HEADERS.contains(&name.as_str()) || listed.contains(name) {
            continue;
        }
        request = request.header(name, value);
    }
    // Codex 没要压缩时由这边向上游要，收到后解开再交回（见模块说明）。
    let decode_for_codex = !headers.contains_key(axum::http::header::ACCEPT_ENCODING);
    if decode_for_codex {
        request = request.header(axum::http::header::ACCEPT_ENCODING, "gzip");
    }
    let upstream_response = match request.send().await {
        Ok(response) => response,
        Err(error) => {
            log::warn!(
                "[Codex] 官方模型列表转发失败，交给 Codex 用自带列表: {}",
                super::error::describe_reqwest_error(error)
            );
            return None;
        }
    };
    // 被重定向到别处（登录页之类）的回包不交给 Codex。
    if upstream_response.url().host_str() != Some(expected_host.as_str()) {
        log::warn!(
            "[Codex] 官方模型列表被重定向到 {}，交给 Codex 用自带列表",
            upstream_response.url()
        );
        return None;
    }
    let status = upstream_response.status();
    let mut response_headers = upstream_response.headers().clone();
    // 边读边数，超限立即断开，不先把整包收进内存。
    let body = match ProxyResponse::Reqwest(upstream_response)
        .bytes_with_limit(MAX_BODY_BYTES)
        .await
    {
        Ok(body) => body,
        Err(error) => {
            log::warn!("[Codex] 读取官方模型列表失败，交给 Codex 用自带列表: {error}");
            return None;
        }
    };
    // Codex 自己要了压缩时，字节原样交回（照旧带着 Content-Encoding）；长度一律重新算。
    let body = if decode_for_codex {
        decode_body(&mut response_headers, body)?
    } else {
        body
    };
    strip_hop_by_hop_response_headers(&mut response_headers);
    response_headers.remove(axum::http::header::CONTENT_LENGTH);
    let mut response = Response::new(Body::from(body));
    *response.status_mut() = status;
    *response.headers_mut() = response_headers;
    Some(response)
}

/// 把这边替 Codex 要来的压缩回包解成明文，并去掉 `Content-Encoding`。解出来超过上限、
/// 或是解不了的编码时返回 `None`（交给 Codex 用自带列表）。空包（304 之类）不用解。
fn decode_body(headers: &mut HeaderMap, body: bytes::Bytes) -> Option<bytes::Bytes> {
    let Some(encoding) = super::content_encoding::get_content_encoding(headers) else {
        return Some(body);
    };
    headers.remove(axum::http::header::CONTENT_ENCODING);
    if body.is_empty() {
        return Some(body);
    }
    match super::content_encoding::decompress_body_with_limit(&encoding, &body, MAX_BODY_BYTES) {
        Ok(Some(decoded)) => Some(decoded.into()),
        Ok(None) => {
            log::warn!("[Codex] 官方模型列表用了解不了的压缩 {encoding}，交给 Codex 用自带列表");
            None
        }
        Err(error) => {
            log::warn!("[Codex] 解压官方模型列表失败，交给 Codex 用自带列表: {error}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::StatusCode;
    use http_body_util::BodyExt;
    use serde_json::{json, Value};

    /// 回显收到的路径、查询和请求头；`If-None-Match` 对上时回 304。
    async fn echo_upstream() -> String {
        async fn models(uri: Uri, headers: HeaderMap) -> Response {
            if headers.get("if-none-match").and_then(|v| v.to_str().ok()) == Some("\"v1\"") {
                let mut response = Response::new(Body::empty());
                *response.status_mut() = StatusCode::NOT_MODIFIED;
                response
                    .headers_mut()
                    .insert("etag", "\"v1\"".parse().unwrap());
                return response;
            }
            let seen: serde_json::Map<String, Value> = headers
                .iter()
                .map(|(name, value)| {
                    (
                        name.as_str().to_string(),
                        json!(value.to_str().unwrap_or_default()),
                    )
                })
                .collect();
            let body = json!({
                "models": [{ "slug": "gpt-x", "available_access_programs": { "cyber": ["standard", "daybreak_blue"] } }],
                "path": uri.path(),
                "query": uri.query(),
                "headers": seen,
            });
            // 和官方一样：要了 gzip 才压缩。
            let wants_gzip = headers
                .get("accept-encoding")
                .and_then(|v| v.to_str().ok())
                .is_some_and(|v| v.contains("gzip"));
            let mut response = if wants_gzip {
                let mut response = Response::new(Body::from(gzip(body.to_string().as_bytes())));
                response
                    .headers_mut()
                    .insert("content-encoding", "gzip".parse().unwrap());
                response
            } else {
                Response::new(Body::from(body.to_string()))
            };
            response
                .headers_mut()
                .insert("etag", "\"v1\"".parse().unwrap());
            response
                .headers_mut()
                .insert("content-type", "application/json".parse().unwrap());
            response
        }
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        async fn oversized() -> Vec<u8> {
            vec![b' '; MAX_BODY_BYTES + 1]
        }
        /// 压缩后很小、解开超过上限。
        async fn bomb() -> Response {
            let mut response = Response::new(Body::from(gzip(&vec![b' '; MAX_BODY_BYTES + 1])));
            response
                .headers_mut()
                .insert("content-encoding", "gzip".parse().unwrap());
            response
        }
        let app = axum::Router::new()
            .route("/backend-api/codex/models", axum::routing::get(models))
            .route("/oversized/models", axum::routing::get(oversized))
            .route("/bomb/models", axum::routing::get(bomb));
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        format!("http://127.0.0.1:{port}/backend-api/codex/models")
    }

    fn gzip(bytes: &[u8]) -> Vec<u8> {
        use std::io::Write;
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        encoder.write_all(bytes).unwrap();
        encoder.finish().unwrap()
    }

    fn codex_headers() -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert("authorization", "Bearer chatgpt-token".parse().unwrap());
        headers.insert("chatgpt-account-id", "acct-1".parse().unwrap());
        headers.insert("originator", "codex_desktop".parse().unwrap());
        headers.insert("host", "127.0.0.1:15721".parse().unwrap());
        headers.insert("connection", "keep-alive, x-hop".parse().unwrap());
        headers.insert("x-hop", "1".parse().unwrap());
        headers
    }

    async fn body_json(response: Response) -> Value {
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[tokio::test]
    async fn the_official_list_reaches_codex_with_its_own_login_and_query() {
        let upstream = echo_upstream().await;
        let uri: Uri = "/v1/models?client_version=0.167.0".parse().unwrap();
        let response = forward(&upstream, &uri, &codex_headers())
            .await
            .expect("forwarded");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers().get("etag").unwrap(), "\"v1\"");
        let body = body_json(response).await;
        assert_eq!(
            body["models"][0]["available_access_programs"]["cyber"],
            json!(["standard", "daybreak_blue"])
        );
        assert_eq!(body["query"], "client_version=0.167.0");
        let seen = &body["headers"];
        assert_eq!(seen["authorization"], "Bearer chatgpt-token");
        assert_eq!(seen["chatgpt-account-id"], "acct-1");
        assert_eq!(seen["originator"], "codex_desktop");
        // 逐跳头和本地地址不往上游带。
        assert_ne!(seen["host"], "127.0.0.1:15721");
        assert!(seen.get("x-hop").is_none());
    }

    #[tokio::test]
    async fn codex_gets_plain_json_while_the_upstream_hop_is_gzipped() {
        // Codex 拉列表不带 Accept-Encoding。
        let upstream = echo_upstream().await;
        let uri: Uri = "/v1/models".parse().unwrap();
        let response = forward(&upstream, &uri, &codex_headers())
            .await
            .expect("forwarded");
        assert!(response.headers().get("content-encoding").is_none());
        let body = body_json(response).await;
        assert_eq!(body["headers"]["accept-encoding"], "gzip");
        assert_eq!(body["models"][0]["slug"], "gpt-x");
    }

    #[tokio::test]
    async fn codex_own_accept_encoding_is_passed_through_untouched() {
        let upstream = echo_upstream().await;
        let mut headers = codex_headers();
        headers.insert("accept-encoding", "gzip, br".parse().unwrap());
        let uri: Uri = "/v1/models".parse().unwrap();
        let response = forward(&upstream, &uri, &headers).await.expect("forwarded");
        assert_eq!(response.headers().get("content-encoding").unwrap(), "gzip");
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let plain = crate::proxy::content_encoding::decompress_body("gzip", &bytes)
            .unwrap()
            .unwrap();
        let body: Value = serde_json::from_slice(&plain).unwrap();
        assert_eq!(body["headers"]["accept-encoding"], "gzip, br");
    }

    #[tokio::test]
    async fn a_reply_that_inflates_past_the_limit_falls_back_to_the_empty_list() {
        let upstream = echo_upstream()
            .await
            .replace("/backend-api/codex/models", "/bomb/models");
        let uri: Uri = "/v1/models".parse().unwrap();
        assert!(forward(&upstream, &uri, &codex_headers()).await.is_none());
    }

    #[tokio::test]
    async fn not_modified_is_handed_back_unchanged() {
        let upstream = echo_upstream().await;
        let mut headers = codex_headers();
        headers.insert("if-none-match", "\"v1\"".parse().unwrap());
        let uri: Uri = "/v1/models?client_version=0.167.0".parse().unwrap();
        let response = forward(&upstream, &uri, &headers).await.expect("forwarded");
        assert_eq!(response.status(), StatusCode::NOT_MODIFIED);
        assert_eq!(response.headers().get("etag").unwrap(), "\"v1\"");
    }

    #[tokio::test]
    async fn an_unreachable_upstream_falls_back_to_the_empty_list() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let upstream = format!("http://127.0.0.1:{port}/backend-api/codex/models");
        let uri: Uri = "/v1/models".parse().unwrap();
        assert!(forward(&upstream, &uri, &codex_headers()).await.is_none());
    }

    #[tokio::test]
    async fn an_oversized_reply_falls_back_to_the_empty_list() {
        let upstream = echo_upstream()
            .await
            .replace("/backend-api/codex/models", "/oversized/models");
        let uri: Uri = "/v1/models".parse().unwrap();
        assert!(forward(&upstream, &uri, &codex_headers()).await.is_none());
    }

    #[test]
    fn only_a_real_bearer_login_is_forwarded() {
        let with = |value: &str| {
            let mut headers = HeaderMap::new();
            headers.insert("authorization", value.parse().unwrap());
            has_codex_login(&headers)
        };
        assert!(with("Bearer chatgpt-token"));
        assert!(with("bearer chatgpt-token"));
        assert!(!with("Bearer PROXY_MANAGED"));
        assert!(!with("Basic abc"));
        assert!(!with("Bearer"));
        assert!(!has_codex_login(&HeaderMap::new()));
    }

    #[tokio::test]
    async fn a_route_that_is_not_official_is_not_forwarded() {
        // 内存库里没有正在用的供应商：和第三方路由一样不转。
        let db = crate::database::Database::memory().expect("memory db");
        let uri: Uri = "/v1/models".parse().unwrap();
        assert!(forward_if_official_route(&db, &uri, &codex_headers())
            .await
            .is_none());
    }
}
