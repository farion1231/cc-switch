//! WebDAV HTTP transport layer.
//!
//! Low-level HTTP primitives for WebDAV operations (PUT, GET, HEAD, MKCOL, PROPFIND).
//! The sync protocol logic lives in [`super::webdav_sync`].

use reqwest::{Method, RequestBuilder, StatusCode, Url};
use std::time::Duration;

use crate::error::AppError;
use crate::proxy::http_client;
use futures::StreamExt;

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
            // Ambiguous — verify directory actually exists via PROPFIND
            s if s == StatusCode::METHOD_NOT_ALLOWED
                || s == StatusCode::CONFLICT
                || s.is_redirection() =>
            {
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
    .map_err(|e| webdav_transport_error("webdav.put_failed", "PUT 请求", "PUT request", url, &e))?;

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
    max_bytes: usize,
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
    .map_err(|e| webdav_transport_error("webdav.get_failed", "GET 请求", "GET request", url, &e))?;

    if resp.status() == StatusCode::NOT_FOUND {
        return Ok(None);
    }
    if !resp.status().is_success() {
        return Err(webdav_status_error("GET", resp.status(), url));
    }
    ensure_content_length_within_limit(resp.headers(), max_bytes, url)?;

    let etag = resp
        .headers()
        .get("etag")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let mut bytes = Vec::new();
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| {
            AppError::localized(
                "webdav.response_read_failed",
                format!("读取 WebDAV 响应失败: {e}"),
                format!("Failed to read WebDAV response: {e}"),
            )
        })?;
        if bytes.len().saturating_add(chunk.len()) > max_bytes {
            return Err(response_too_large_error(url, max_bytes));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(Some((bytes, etag)))
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
        webdav_transport_error("webdav.head_failed", "HEAD 请求", "HEAD request", url, &e)
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

/// DELETE a remote resource. A missing resource (404) counts as success so
/// garbage collection stays idempotent.
pub async fn delete_resource(url: &str, auth: &WebDavAuth) -> Result<(), AppError> {
    let client = http_client::get();
    let resp = apply_auth(
        client
            .delete(url)
            .timeout(Duration::from_secs(DEFAULT_TIMEOUT_SECS)),
        auth,
    )
    .send()
    .await
    .map_err(|e| {
        webdav_transport_error(
            "webdav.delete_failed",
            "DELETE 请求",
            "DELETE request",
            url,
            &e,
        )
    })?;

    if resp.status().is_success() || resp.status() == StatusCode::NOT_FOUND {
        return Ok(());
    }
    Err(webdav_status_error("DELETE", resp.status(), url))
}

/// One child entry returned by [`list_collection`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DavEntry {
    /// Percent-decoded last path segment.
    pub name: String,
    pub is_collection: bool,
}

/// Upper bound for a PROPFIND Depth=1 response body.
const MAX_PROPFIND_BYTES: usize = 8 * 1024 * 1024;

/// List the direct children of a collection via PROPFIND Depth=1.
///
/// Returns `None` when the collection does not exist (404), so callers can
/// treat "nothing uploaded yet" separately from transport failures.
pub async fn list_collection(
    url: &str,
    auth: &WebDavAuth,
) -> Result<Option<Vec<DavEntry>>, AppError> {
    let dir_url = if url.ends_with('/') {
        url.to_string()
    } else {
        format!("{url}/")
    };
    let client = http_client::get();
    let body = r#"<?xml version="1.0" encoding="utf-8"?><D:propfind xmlns:D="DAV:"><D:prop><D:resourcetype/></D:prop></D:propfind>"#;
    let resp = apply_auth(
        client
            .request(method_propfind(), &dir_url)
            .header("Depth", "1")
            .header("Content-Type", "application/xml; charset=utf-8")
            .body(body)
            .timeout(Duration::from_secs(DEFAULT_TIMEOUT_SECS)),
        auth,
    )
    .send()
    .await
    .map_err(|e| {
        webdav_transport_error(
            "webdav.propfind_failed",
            "PROPFIND 请求",
            "PROPFIND request",
            &dir_url,
            &e,
        )
    })?;

    if resp.status() == StatusCode::NOT_FOUND {
        return Ok(None);
    }
    if !(resp.status().is_success() || resp.status() == StatusCode::MULTI_STATUS) {
        return Err(webdav_status_error("PROPFIND", resp.status(), &dir_url));
    }
    ensure_content_length_within_limit(resp.headers(), MAX_PROPFIND_BYTES, &dir_url)?;
    let mut bytes = Vec::new();
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| {
            AppError::localized(
                "webdav.response_read_failed",
                format!("读取 WebDAV 响应失败: {e}"),
                format!("Failed to read WebDAV response: {e}"),
            )
        })?;
        if bytes.len().saturating_add(chunk.len()) > MAX_PROPFIND_BYTES {
            return Err(response_too_large_error(&dir_url, MAX_PROPFIND_BYTES));
        }
        bytes.extend_from_slice(&chunk);
    }
    let text = String::from_utf8_lossy(&bytes);
    let request_path = Url::parse(&dir_url)
        .map(|u| u.path().to_string())
        .unwrap_or_default();
    Ok(Some(parse_propfind_children(&text, &request_path)))
}

/// Normalize an href (absolute URL or path) to a decoded path without a trailing slash.
fn normalize_href_path(href: &str) -> String {
    let path = if href.starts_with("http://") || href.starts_with("https://") {
        Url::parse(href)
            .map(|u| u.path().to_string())
            .unwrap_or_else(|_| href.to_string())
    } else {
        href.to_string()
    };
    let decoded = percent_encoding::percent_decode_str(&path)
        .decode_utf8_lossy()
        .to_string();
    decoded.trim_end_matches('/').to_string()
}

/// Parse a multistatus body into the children of `request_path`.
///
/// The entry describing the collection itself is skipped. Some reverse proxies
/// rewrite the path prefix, so "self" is detected by suffix match as well.
pub(crate) fn parse_propfind_children(xml: &str, request_path: &str) -> Vec<DavEntry> {
    use super::xml_scan::{scan, XmlEvent};

    let self_path = normalize_href_path(request_path);
    let mut entries = Vec::new();
    let mut in_response = false;
    let mut in_href = false;
    let mut in_resourcetype = false;
    let mut href: Option<String> = None;
    let mut is_collection = false;

    for event in scan(xml) {
        match event {
            XmlEvent::Start(name) => match name.as_str() {
                "response" => {
                    in_response = true;
                    href = None;
                    is_collection = false;
                }
                "href" if in_response && href.is_none() => in_href = true,
                "resourcetype" if in_response => in_resourcetype = true,
                "collection" if in_resourcetype => is_collection = true,
                _ => {}
            },
            XmlEvent::Text(text) if in_href => {
                href = Some(text);
            }
            XmlEvent::End(name) => match name.as_str() {
                "href" => in_href = false,
                "resourcetype" => in_resourcetype = false,
                "response" => {
                    in_response = false;
                    let Some(raw) = href.take() else { continue };
                    let path = normalize_href_path(&raw);
                    let is_self = path == self_path
                        || (!path.is_empty() && self_path.ends_with(&path))
                        || (!self_path.is_empty() && path.ends_with(&self_path));
                    if is_self {
                        continue;
                    }
                    let Some(name) = path.rsplit('/').next().filter(|n| !n.is_empty()) else {
                        continue;
                    };
                    entries.push(DavEntry {
                        name: name.to_string(),
                        is_collection,
                    });
                }
                _ => {}
            },
            _ => {}
        }
    }
    entries
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
            zh.push_str("。坚果云请使用「第三方应用密码」，并确认地址指向 /dav/ 下的目录。");
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
            zh.push_str("。坚果云不允许自动创建顶层文件夹，请先在网页端手动创建后重试。");
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
    crate::redact_url_for_log(raw)
}

fn response_too_large_error(url: &str, max_bytes: usize) -> AppError {
    let max_mb = max_bytes / 1024 / 1024;
    AppError::localized(
        "webdav.response_too_large",
        format!(
            "WebDAV 响应体超过上限（{} MB）: {}",
            max_mb,
            redact_url(url)
        ),
        format!(
            "WebDAV response body exceeds limit ({} MB): {}",
            max_mb,
            redact_url(url)
        ),
    )
}

fn ensure_content_length_within_limit(
    headers: &reqwest::header::HeaderMap,
    max_bytes: usize,
    url: &str,
) -> Result<(), AppError> {
    let Some(content_length) = headers.get(reqwest::header::CONTENT_LENGTH) else {
        return Ok(());
    };
    let Ok(raw) = content_length.to_str() else {
        return Ok(());
    };
    let Ok(value) = raw.parse::<u64>() else {
        return Ok(());
    };
    if value > max_bytes as u64 {
        return Err(response_too_large_error(url, max_bytes));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::header::{HeaderMap, HeaderValue, CONTENT_LENGTH};

    #[test]
    fn build_remote_url_encodes_path_segments() {
        let url = build_remote_url(
            "https://dav.example.com/remote.php/dav/files/demo/",
            &[
                "cc switch-sync".to_string(),
                "v2".to_string(),
                "db-v6".to_string(),
                "default profile".to_string(),
                "manifest.json".to_string(),
            ],
        )
        .unwrap();
        assert_eq!(
            url,
            "https://dav.example.com/remote.php/dav/files/demo/cc%20switch-sync/v2/db-v6/default%20profile/manifest.json"
        );
        assert!(!url.contains("//cc"), "should not have double-slash");
    }

    #[test]
    fn parse_propfind_children_skips_self_and_decodes_names() {
        let xml = r#"<?xml version="1.0" encoding="utf-8"?>
<d:multistatus xmlns:d="DAV:">
  <d:response>
    <d:href>/dav/root/usage-v1/default/devices/</d:href>
    <d:propstat><d:prop><d:resourcetype><d:collection/></d:resourcetype></d:prop></d:propstat>
  </d:response>
  <d:response>
    <d:href>/dav/root/usage-v1/default/devices/0f8fad5b-d9cb-469f-a165-70867728950e/</d:href>
    <d:propstat><d:prop><d:resourcetype><d:collection/></d:resourcetype></d:prop></d:propstat>
  </d:response>
  <d:response>
    <d:href>https://dav.example.com/dav/root/usage-v1/default/devices/with%20space.txt</d:href>
    <d:propstat><d:prop><d:resourcetype/></d:prop></d:propstat>
  </d:response>
</d:multistatus>"#;
        let entries = parse_propfind_children(xml, "/dav/root/usage-v1/default/devices/");
        assert_eq!(
            entries,
            vec![
                DavEntry {
                    name: "0f8fad5b-d9cb-469f-a165-70867728950e".into(),
                    is_collection: true,
                },
                DavEntry {
                    name: "with space.txt".into(),
                    is_collection: false,
                },
            ]
        );
    }

    #[test]
    fn parse_propfind_children_handles_prefix_rewriting_proxies() {
        // Reverse proxy strips "/public" from hrefs; the self entry must still be skipped.
        let xml = r#"<multistatus xmlns="DAV:">
  <response><href>/dav/devices/</href><propstat><prop><resourcetype><collection/></resourcetype></prop></propstat></response>
  <response><href>/dav/devices/abc/</href><propstat><prop><resourcetype><collection/></resourcetype></prop></propstat></response>
</multistatus>"#;
        let entries = parse_propfind_children(xml, "/public/dav/devices/");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "abc");
        assert!(entries[0].is_collection);
    }

    #[test]
    fn parse_propfind_children_tolerates_garbage() {
        assert!(parse_propfind_children("not xml at all", "/x").is_empty());
        assert!(parse_propfind_children("<response><href>", "/x").is_empty());
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
        // userinfo 与整个 query 一并剥离，host+path 保留用于诊断。
        let redacted = redact_url("https://alice:secret@example.com:8443/dav?token=abc&foo=1");
        assert_eq!(redacted, "https://example.com:8443/dav");
        assert!(!redacted.contains("secret"));
        assert!(!redacted.contains("abc"));
    }

    #[test]
    fn ensure_content_length_within_limit_accepts_missing_or_small_values() {
        let empty = HeaderMap::new();
        assert!(
            ensure_content_length_within_limit(&empty, 1024, "https://dav.example.com").is_ok()
        );

        let mut small = HeaderMap::new();
        small.insert(CONTENT_LENGTH, HeaderValue::from_static("1024"));
        assert!(
            ensure_content_length_within_limit(&small, 1024, "https://dav.example.com").is_ok()
        );
    }

    #[test]
    fn ensure_content_length_within_limit_rejects_oversized_values() {
        let mut large = HeaderMap::new();
        large.insert(CONTENT_LENGTH, HeaderValue::from_static("2048"));

        let err = ensure_content_length_within_limit(&large, 1024, "https://dav.example.com")
            .expect_err("oversized response should be rejected");
        assert!(
            err.to_string().contains("too large") || err.to_string().contains("超过"),
            "unexpected error: {err}"
        );
    }
}
