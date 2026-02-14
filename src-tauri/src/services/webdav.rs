//! WebDAV HTTP transport layer.
//!
//! Low-level HTTP primitives for WebDAV operations (PUT, GET, HEAD, MKCOL, PROPFIND).
//! The sync protocol logic lives in [`super::webdav_sync`].

use reqwest::{Method, RequestBuilder, StatusCode, Url};
use std::time::Duration;

use crate::error::AppError;
use crate::proxy::http_client;

const DEFAULT_TIMEOUT_SECS: u64 = 30;
/// Timeout for large file transfers (PUT/GET of db.sql, skills.zip).
const TRANSFER_TIMEOUT_SECS: u64 = 300;

/// Auth pair: `(username, Some(password))`.
pub type WebDavAuth = Option<(String, Option<String>)>;

// ─── WebDAV extension methods ────────────────────────────────

fn method_propfind() -> Method {
    Method::from_bytes(b"PROPFIND").expect("PROPFIND is a valid HTTP method")
}

fn method_mkcol() -> Method {
    Method::from_bytes(b"MKCOL").expect("MKCOL is a valid HTTP method")
}

// ─── URL utilities ───────────────────────────────────────────

/// Parse and validate a WebDAV base URL (must be http or https).
pub fn parse_base_url(raw: &str) -> Result<Url, AppError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(AppError::localized(
            "webdav.base_url.required",
            "WebDAV 地址不能为空",
            "WebDAV URL is required.",
        ));
    }
    let url = Url::parse(trimmed).map_err(|e| {
        AppError::localized(
            "webdav.base_url.invalid",
            format!("WebDAV 地址无效: {e}"),
            format!("Invalid WebDAV URL: {e}"),
        )
    })?;
    match url.scheme() {
        "http" | "https" => Ok(url),
        _ => Err(AppError::localized(
            "webdav.base_url.scheme_invalid",
            "WebDAV 仅支持 http/https 地址",
            "WebDAV URL must use http or https.",
        )),
    }
}

/// Build a full URL from a base URL string and path segments.
///
/// Each segment is individually percent-encoded by the `url` crate.
pub fn build_remote_url(base_url: &str, segments: &[String]) -> Result<String, AppError> {
    let mut url = parse_base_url(base_url)?;
    {
        let mut path = url.path_segments_mut().map_err(|_| {
            AppError::localized(
                "webdav.base_url.unusable",
                "WebDAV 地址格式不支持追加路径",
                "WebDAV URL format does not support appending path segments.",
            )
        })?;
        path.pop_if_empty();
        for seg in segments {
            path.push(seg);
        }
    }
    Ok(url.to_string())
}

/// Split a slash-delimited path into non-empty segments.
pub fn path_segments(raw: &str) -> impl Iterator<Item = &str> {
    raw.trim_matches('/').split('/').filter(|s| !s.is_empty())
}

// ─── Auth ────────────────────────────────────────────────────

/// Build auth from username/password. Returns `None` if username is blank.
pub fn auth_from_credentials(username: &str, password: &str) -> WebDavAuth {
    let user = username.trim();
    if user.is_empty() {
        return None;
    }
    Some((user.to_string(), Some(password.to_string())))
}

/// Apply Basic-Auth to a request builder if auth is present.
fn apply_auth(builder: RequestBuilder, auth: &WebDavAuth) -> RequestBuilder {
    match auth {
        Some((user, pass)) => builder.basic_auth(user, pass.as_deref()),
        None => builder,
    }
}

fn webdav_transport_error(
    key: &'static str,
    op_zh: &str,
    op_en: &str,
    target_url: &str,
    err: &reqwest::Error,
) -> AppError {
    let (zh_reason, en_reason) = if err.is_timeout() {
        ("请求超时", "request timed out")
    } else if err.is_connect() {
        ("连接失败", "connection failed")
    } else if err.is_request() {
        ("请求构造失败", "request build failed")
    } else {
        ("网络请求失败", "network request failed")
    };

    let safe_url = redact_url(target_url);
    AppError::localized(
        key,
        format!("WebDAV {op_zh}失败（{zh_reason}）: {safe_url}"),
        format!("WebDAV {op_en} failed ({en_reason}): {safe_url}"),
    )
}

// ─── HTTP operations ─────────────────────────────────────────

/// Test WebDAV connectivity via PROPFIND Depth=0 on the base URL.
pub async fn test_connection(base_url: &str, auth: &WebDavAuth) -> Result<(), AppError> {
    let url = parse_base_url(base_url)?;
    let client = http_client::get();

    let resp = apply_auth(
        client
            .request(method_propfind(), url)
            .header("Depth", "0")
            .timeout(Duration::from_secs(DEFAULT_TIMEOUT_SECS)),
        auth,
    )
    .send()
    .await
    .map_err(|e| {
        webdav_transport_error(
            "webdav.connection_failed",
            "连接",
            "connection",
            base_url,
            &e,
        )
    })?;

    if resp.status().is_success() || resp.status() == StatusCode::MULTI_STATUS {
        return Ok(());
    }
    Err(webdav_status_error("PROPFIND", resp.status(), base_url))
}

/// Ensure a chain of remote directories exists.
///
/// Uses optimistic MKCOL: try creating first, fall back to PROPFIND verification
/// on ambiguous responses. This halves the round-trips vs PROPFIND-first approach.
pub async fn ensure_remote_directories(
    base_url: &str,
    segments: &[String],
    auth: &WebDavAuth,
) -> Result<(), AppError> {
    if segments.is_empty() {
        return Ok(());
    }
    let client = http_client::get();

    for depth in 1..=segments.len() {
        let prefix = &segments[..depth];
        let url = build_remote_url(base_url, prefix)?;
        let dir_url = if url.ends_with('/') {
            url
        } else {
            format!("{url}/")
        };

        let resp = apply_auth(
            client
                .request(method_mkcol(), &dir_url)
                .timeout(Duration::from_secs(DEFAULT_TIMEOUT_SECS)),
            auth,
        )
        .send()
        .await
        .map_err(|e| {
            webdav_transport_error(
                "webdav.mkcol_failed",
                "MKCOL 请求",
                "MKCOL request",
                &dir_url,
                &e,
            )
        })?;

        let status = resp.status();
        match status {
            s if s == StatusCode::CREATED || s.is_success() => {
                log::info!("[WebDAV] MKCOL ok: {}", redact_url(&dir_url));
            }
            // 405 commonly means "already exists" on many WebDAV servers
            StatusCode::METHOD_NOT_ALLOWED => {}
            // Ambiguous — verify directory actually exists via PROPFIND
            s if s == StatusCode::CONFLICT || s.is_redirection() => {
                if !propfind_exists(&client, &dir_url, auth).await? {
                    return Err(webdav_status_error("MKCOL", status, &dir_url));
                }
            }
            _ => {
                return Err(webdav_status_error("MKCOL", status, &dir_url));
            }
        }
    }
    Ok(())
}

/// PUT bytes to a remote WebDAV URL.
pub async fn put_bytes(
    url: &str,
    auth: &WebDavAuth,
    bytes: Vec<u8>,
    content_type: &str,
) -> Result<(), AppError> {
    let client = http_client::get();
    let resp = apply_auth(
        client
            .put(url)
            .header("Content-Type", content_type)
            .body(bytes)
            .timeout(Duration::from_secs(TRANSFER_TIMEOUT_SECS)),
        auth,
    )
    .send()
    .await
    .map_err(|e| {
        webdav_transport_error(
            "webdav.put_failed",
            "PUT 请求",
            "PUT request",
            url,
            &e,
        )
    })?;

    if resp.status().is_success() {
        return Ok(());
    }
    Err(webdav_status_error("PUT", resp.status(), url))
}

/// GET bytes from a remote WebDAV URL. Returns `None` on 404.
///
/// On success returns `(body_bytes, optional_etag)`.
pub async fn get_bytes(
    url: &str,
    auth: &WebDavAuth,
) -> Result<Option<(Vec<u8>, Option<String>)>, AppError> {
    let client = http_client::get();
    let resp = apply_auth(
        client
            .get(url)
            .timeout(Duration::from_secs(TRANSFER_TIMEOUT_SECS)),
        auth,
    )
    .send()
    .await
    .map_err(|e| {
        webdav_transport_error(
            "webdav.get_failed",
            "GET 请求",
            "GET request",
            url,
            &e,
        )
    })?;

    if resp.status() == StatusCode::NOT_FOUND {
        return Ok(None);
    }
    if !resp.status().is_success() {
        return Err(webdav_status_error("GET", resp.status(), url));
    }
    let etag = resp
        .headers()
        .get("etag")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| {
            AppError::localized(
                "webdav.response_read_failed",
                format!("读取 WebDAV 响应失败: {e}"),
                format!("Failed to read WebDAV response: {e}"),
            )
        })?;
    Ok(Some((bytes.to_vec(), etag)))
}

/// HEAD request to retrieve the ETag. Returns `None` on 404.
pub async fn head_etag(url: &str, auth: &WebDavAuth) -> Result<Option<String>, AppError> {
    let client = http_client::get();
    let resp = apply_auth(
        client
            .head(url)
            .timeout(Duration::from_secs(DEFAULT_TIMEOUT_SECS)),
        auth,
    )
    .send()
    .await
    .map_err(|e| {
        webdav_transport_error(
            "webdav.head_failed",
            "HEAD 请求",
            "HEAD request",
            url,
            &e,
        )
    })?;

    if resp.status() == StatusCode::NOT_FOUND {
        return Ok(None);
    }
    if !resp.status().is_success() {
        return Err(webdav_status_error("HEAD", resp.status(), url));
    }
    Ok(resp
        .headers()
        .get("etag")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string()))
}

// ─── Internal helpers ────────────────────────────────────────

/// PROPFIND Depth=0 to check if a remote resource exists.
async fn propfind_exists(
    client: &reqwest::Client,
    url: &str,
    auth: &WebDavAuth,
) -> Result<bool, AppError> {
    let resp = apply_auth(
        client
            .request(method_propfind(), url)
            .header("Depth", "0")
            .timeout(Duration::from_secs(DEFAULT_TIMEOUT_SECS)),
        auth,
    )
    .send()
    .await;
    match resp {
        Ok(r) => Ok(r.status().is_success() || r.status() == StatusCode::MULTI_STATUS),
        Err(e) => {
            log::warn!(
                "[WebDAV] PROPFIND check failed for {}: {e}",
                redact_url(url)
            );
            Ok(false)
        }
    }
}

// ─── Service detection & error helpers ───────────────────────

/// Check if a URL points to Jianguoyun (坚果云).
pub fn is_jianguoyun(url: &str) -> bool {
    Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(|h| h.to_lowercase()))
        .map(|host| host.contains("jianguoyun.com") || host.contains("nutstore"))
        .unwrap_or(false)
}

/// Build an `AppError` with service-specific hints for WebDAV failures.
pub fn webdav_status_error(op: &str, status: StatusCode, url: &str) -> AppError {
    let safe_url = redact_url(url);
    let mut zh = format!("WebDAV {op} 失败: {status} ({safe_url})");
    let mut en = format!("WebDAV {op} failed: {status} ({safe_url})");
    let jgy = is_jianguoyun(url);

    if matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN) {
        if jgy {
            zh.push_str(
                "。坚果云请使用「第三方应用密码」，并确认地址指向 /dav/ 下的目录。",
            );
            en.push_str(
                ". For Jianguoyun, use an app-specific password and ensure the URL points under /dav/.",
            );
        } else {
            zh.push_str("。请检查 WebDAV 用户名、密码及目录读写权限。");
            en.push_str(". Please check WebDAV username/password and directory permissions.");
        }
    } else if jgy && (status == StatusCode::NOT_FOUND || status.is_redirection()) {
        zh.push_str("。坚果云常见原因：地址不在 /dav/ 可写目录下。");
        en.push_str(". Common Jianguoyun cause: URL is outside a writable /dav/ directory.");
    } else if op == "MKCOL" && status == StatusCode::CONFLICT {
        if jgy {
            zh.push_str(
                "。坚果云不允许自动创建顶层文件夹，请先在网页端手动创建后重试。",
            );
            en.push_str(
                ". Jianguoyun does not allow creating top-level folders automatically; create it manually first.",
            );
        } else {
            zh.push_str("。请确认上级目录存在。");
            en.push_str(". Please ensure the parent directory exists.");
        }
    }

    AppError::localized("webdav.http.status", zh, en)
}

fn redact_url(raw: &str) -> String {
    match Url::parse(raw) {
        Ok(mut parsed) => {
            let _ = parsed.set_username("");
            let _ = parsed.set_password(None);

            let mut out = format!("{}://", parsed.scheme());
            if let Some(host) = parsed.host_str() {
                out.push_str(host);
            }
            if let Some(port) = parsed.port() {
                out.push(':');
                out.push_str(&port.to_string());
            }
            out.push_str(parsed.path());

            let mut keys: Vec<String> = parsed.query_pairs().map(|(k, _)| k.into_owned()).collect();
            keys.sort();
            keys.dedup();
            if !keys.is_empty() {
                out.push_str("?[keys:");
                out.push_str(&keys.join(","));
                out.push(']');
            }
            out
        }
        Err(_) => raw.split('?').next().unwrap_or(raw).to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_remote_url_encodes_path_segments() {
        let url = build_remote_url(
            "https://dav.example.com/remote.php/dav/files/demo/",
            &[
                "cc switch-sync".to_string(),
                "v2".to_string(),
                "default profile".to_string(),
                "manifest.json".to_string(),
            ],
        )
        .unwrap();
        assert_eq!(
            url,
            "https://dav.example.com/remote.php/dav/files/demo/cc%20switch-sync/v2/default%20profile/manifest.json"
        );
        assert!(!url.contains("//cc"), "should not have double-slash");
    }

    #[test]
    fn is_jianguoyun_detects_correctly() {
        assert!(is_jianguoyun("https://dav.jianguoyun.com/dav"));
        assert!(is_jianguoyun("https://dav.jianguoyun.com/dav/folder"));
        assert!(!is_jianguoyun("https://nextcloud.example.com/dav"));
    }

    #[test]
    fn path_segments_splits_correctly() {
        let segs: Vec<_> = path_segments("/a/b/c/").collect();
        assert_eq!(segs, vec!["a", "b", "c"]);

        let segs: Vec<_> = path_segments("single").collect();
        assert_eq!(segs, vec!["single"]);

        let segs: Vec<_> = path_segments("").collect();
        assert!(segs.is_empty());
    }

    #[test]
    fn auth_from_credentials_trims_and_rejects_blank() {
        assert!(auth_from_credentials("  ", "pass").is_none());
        let auth = auth_from_credentials(" user ", "pass");
        assert_eq!(auth, Some(("user".to_string(), Some("pass".to_string()))));
    }

    #[test]
    fn redact_url_hides_credentials_and_query_values() {
        let redacted = redact_url("https://alice:secret@example.com:8443/dav?token=abc&foo=1");
        assert_eq!(
            redacted,
            "https://example.com:8443/dav?[keys:foo,token]"
        );
        assert!(!redacted.contains("secret"));
    }
}
