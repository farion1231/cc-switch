//! S3 HTTP transport layer.
//!
//! Low-level HTTP primitives for S3 operations (PUT, GET, HEAD).
//! Implements AWS Signature Version 4 request signing.
//! The sync protocol logic lives in the upcoming `s3_sync` module.

use reqwest::StatusCode;
use std::time::Duration;
use url::Url;

use crate::error::AppError;
use crate::proxy::http_client;
use futures::StreamExt;

const DEFAULT_TIMEOUT_SECS: u64 = 30;
/// Timeout for large file transfers (PUT / GET of db.sql, skills.zip, etc.).
const TRANSFER_TIMEOUT_SECS: u64 = 300;

// ─── Credentials ─────────────────────────────────────────────

/// S3-compatible storage credentials.
pub(crate) struct S3Credentials {
    pub access_key_id: String,
    pub secret_access_key: String,
    pub region: String,
    pub bucket: String,
    /// Custom endpoint host (e.g. `minio.example.com:9000`).
    /// Empty string means AWS official endpoint.
    pub endpoint: String,
}

// ─── URL construction ────────────────────────────────────────

/// Returns `true` for AWS official endpoints (empty or contains `amazonaws.com`).
fn is_aws_endpoint(endpoint: &str) -> bool {
    endpoint.is_empty() || endpoint.contains("amazonaws.com")
}

/// Strip an optional scheme prefix and trailing slash from an endpoint string.
fn normalize_endpoint(endpoint: &str) -> &str {
    endpoint
        .strip_prefix("https://")
        .or_else(|| endpoint.strip_prefix("http://"))
        .unwrap_or(endpoint)
        .trim_end_matches('/')
}

/// Split an endpoint into its scheme and host-with-port parts.
///
/// Preserves the original scheme when one is provided, defaulting to `"https"`
/// when the endpoint is bare (no `://` prefix).
///
/// ```text
/// "http://minio:9000"           → ("http",  "minio:9000")
/// "https://storage.example.com" → ("https", "storage.example.com")
/// "minio:9000"                  → ("https", "minio:9000")
/// "storage.example.com"         → ("https", "storage.example.com")
/// ```
fn split_scheme_host(endpoint: &str) -> (&str, &str) {
    if let Some(rest) = endpoint.strip_prefix("http://") {
        ("http", rest.trim_end_matches('/'))
    } else if let Some(rest) = endpoint.strip_prefix("https://") {
        ("https", rest.trim_end_matches('/'))
    } else {
        ("https", endpoint.trim_end_matches('/'))
    }
}

/// Build the full URL for an S3 object.
///
/// - AWS endpoints use virtual-hosted style: `https://{bucket}.s3.{region}.amazonaws.com/{key}`
/// - Custom endpoints use path style:       `https://{endpoint}/{bucket}/{key}`
fn build_object_url(creds: &S3Credentials, key: &str) -> String {
    let key = key.trim_start_matches('/');
    if is_aws_endpoint(&creds.endpoint) {
        format!(
            "https://{}.s3.{}.amazonaws.com/{}",
            creds.bucket, creds.region, key
        )
    } else {
        let (scheme, host) = split_scheme_host(&creds.endpoint);
        format!("{}://{}/{}/{}", scheme, host, creds.bucket, key)
    }
}

/// Build the bucket-level URL (for HEAD bucket / test connection).
fn build_bucket_url(creds: &S3Credentials) -> String {
    if is_aws_endpoint(&creds.endpoint) {
        format!(
            "https://{}.s3.{}.amazonaws.com/",
            creds.bucket, creds.region
        )
    } else {
        let (scheme, host) = split_scheme_host(&creds.endpoint);
        format!("{}://{}/{}/", scheme, host, creds.bucket)
    }
}

// ─── Cryptographic helpers ───────────────────────────────────

fn hmac_sha256(key: &[u8], data: &[u8]) -> Vec<u8> {
    use hmac::{Hmac, Mac};
    type HmacSha256 = Hmac<sha2::Sha256>;
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC accepts any key length");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

fn sha256_hex(data: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(data))
}

/// Percent-encode following AWS Sig V4 rules (RFC 3986 unreserved characters only).
fn uri_encode(input: &str, encode_slash: bool) -> String {
    let mut out = String::with_capacity(input.len());
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            b'/' if !encode_slash => out.push('/'),
            _ => {
                use std::fmt::Write;
                let _ = write!(out, "%{:02X}", byte);
            }
        }
    }
    out
}

// ─── AWS Signature V4 signing ────────────────────────────────

/// Sign an HTTP request using AWS Signature Version 4.
///
/// Mutates `headers` by adding `host`, `x-amz-date`, `x-amz-content-sha256`,
/// and the final `authorization` header.
fn sign_request(
    method: &str,
    url: &Url,
    headers: &mut reqwest::header::HeaderMap,
    body_hash: &str,
    creds: &S3Credentials,
    now: chrono::DateTime<chrono::Utc>,
) {
    let timestamp = now.format("%Y%m%dT%H%M%SZ").to_string();
    let datestamp = now.format("%Y%m%d").to_string();

    // ── Step 1: Add required headers ──
    let host_value = match url.port() {
        Some(port) => format!("{}:{}", url.host_str().unwrap_or_default(), port),
        None => url.host_str().unwrap_or_default().to_string(),
    };
    headers.insert("host", host_value.parse().unwrap());
    headers.insert("x-amz-date", timestamp.parse().unwrap());
    headers.insert("x-amz-content-sha256", body_hash.parse().unwrap());

    // ── Step 2: Build canonical request ──

    // Canonical URI (already percent-encoded by the url crate).
    let canonical_uri = url.path();
    let canonical_uri = if canonical_uri.is_empty() {
        "/"
    } else {
        canonical_uri
    };

    // Canonical query string — sorted, re-encoded per Sig V4 rules.
    let mut query_pairs: Vec<(String, String)> = url
        .query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    query_pairs.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
    let canonical_query = if query_pairs.is_empty() {
        String::new()
    } else {
        query_pairs
            .iter()
            .map(|(k, v)| format!("{}={}", uri_encode(k, true), uri_encode(v, true)))
            .collect::<Vec<_>>()
            .join("&")
    };

    // Canonical headers — sorted by lowercase name.
    let mut header_names: Vec<String> = headers
        .keys()
        .map(|k| k.as_str().to_lowercase())
        .collect();
    header_names.sort();
    header_names.dedup();

    let canonical_headers: String = header_names
        .iter()
        .map(|name| {
            let value = headers
                .get(name.as_str())
                .map(|v| v.to_str().unwrap_or("").trim().to_string())
                .unwrap_or_default();
            format!("{}:{}\n", name, value)
        })
        .collect();

    let signed_headers = header_names.join(";");

    let canonical_request = format!(
        "{}\n{}\n{}\n{}\n{}\n{}",
        method, canonical_uri, canonical_query, canonical_headers, signed_headers, body_hash
    );

    // ── Step 3: Build string to sign ──
    let scope = format!("{}/{}/s3/aws4_request", datestamp, creds.region);
    let string_to_sign = format!(
        "AWS4-HMAC-SHA256\n{}\n{}\n{}",
        timestamp,
        scope,
        sha256_hex(canonical_request.as_bytes())
    );

    // ── Step 4: Derive signing key ──
    //   HMAC(HMAC(HMAC(HMAC("AWS4"+secret, date), region), "s3"), "aws4_request")
    let k_date = hmac_sha256(
        format!("AWS4{}", creds.secret_access_key).as_bytes(),
        datestamp.as_bytes(),
    );
    let k_region = hmac_sha256(&k_date, creds.region.as_bytes());
    let k_service = hmac_sha256(&k_region, b"s3");
    let k_signing = hmac_sha256(&k_service, b"aws4_request");

    // ── Step 5: Compute signature ──
    let sig_bytes = hmac_sha256(&k_signing, string_to_sign.as_bytes());
    let signature: String = sig_bytes.iter().map(|b| format!("{:02x}", b)).collect();

    // ── Step 6: Add Authorization header ──
    let authorization = format!(
        "AWS4-HMAC-SHA256 Credential={}/{}, SignedHeaders={}, Signature={}",
        creds.access_key_id, scope, signed_headers, signature
    );
    headers.insert("authorization", authorization.parse().unwrap());
}

// ─── Error helpers ───────────────────────────────────────────

/// Redact a URL for safe inclusion in error messages (strips query parameters).
fn redact_url(raw: &str) -> String {
    match Url::parse(raw) {
        Ok(parsed) => {
            let mut out = format!("{}://", parsed.scheme());
            if let Some(host) = parsed.host_str() {
                out.push_str(host);
            }
            if let Some(port) = parsed.port() {
                out.push(':');
                out.push_str(&port.to_string());
            }
            out.push_str(parsed.path());
            out
        }
        Err(_) => raw.split('?').next().unwrap_or(raw).to_string(),
    }
}

fn s3_transport_error(
    key: &'static str,
    op_zh: &str,
    op_en: &str,
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

    AppError::localized(
        key,
        format!("S3 {op_zh}失败（{zh_reason}）"),
        format!("S3 {op_en} failed ({en_reason})"),
    )
}

fn s3_status_error(op: &str, status: StatusCode, url: &str) -> AppError {
    let safe_url = redact_url(url);
    let mut zh = format!("S3 {op} 失败: {status} ({safe_url})");
    let mut en = format!("S3 {op} failed: {status} ({safe_url})");

    if matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN) {
        zh.push_str("。请检查 Access Key ID 和 Secret Access Key。");
        en.push_str(". Please verify your Access Key ID and Secret Access Key.");
    } else if status == StatusCode::NOT_FOUND && op == "HEAD bucket" {
        zh.push_str("。请检查存储桶名称和区域是否正确。");
        en.push_str(". Please check the bucket name and region.");
    }

    AppError::localized("s3.http.status", zh, en)
}

fn response_too_large_error(url: &str, max_bytes: usize) -> AppError {
    let max_mb = max_bytes / 1024 / 1024;
    AppError::localized(
        "s3.response_too_large",
        format!(
            "S3 响应体超过上限（{} MB）: {}",
            max_mb,
            redact_url(url)
        ),
        format!(
            "S3 response body exceeds limit ({} MB): {}",
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
    let Some(cl) = headers.get(reqwest::header::CONTENT_LENGTH) else {
        return Ok(());
    };
    let Ok(raw) = cl.to_str() else {
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

// ─── Transport functions ─────────────────────────────────────

/// Test S3 connectivity by sending HEAD to the bucket root.
pub(crate) async fn test_connection(creds: &S3Credentials) -> Result<(), AppError> {
    let url_str = build_bucket_url(creds);
    let url = Url::parse(&url_str).map_err(|e| {
        AppError::localized(
            "s3.url.invalid",
            format!("S3 URL 无效: {e}"),
            format!("Invalid S3 URL: {e}"),
        )
    })?;

    let client = http_client::get();
    let body_hash = sha256_hex(b"");
    let mut headers = reqwest::header::HeaderMap::new();
    sign_request("HEAD", &url, &mut headers, &body_hash, creds, chrono::Utc::now());

    let resp = client
        .head(url.as_str())
        .headers(headers)
        .timeout(Duration::from_secs(DEFAULT_TIMEOUT_SECS))
        .send()
        .await
        .map_err(|e| s3_transport_error("s3.connection_failed", "连接", "connection", &e))?;

    if resp.status().is_success() {
        return Ok(());
    }
    Err(s3_status_error("HEAD bucket", resp.status(), &url_str))
}

/// Upload bytes to an S3 object.
pub(crate) async fn put_object(
    creds: &S3Credentials,
    key: &str,
    bytes: Vec<u8>,
    content_type: &str,
) -> Result<(), AppError> {
    let url_str = build_object_url(creds, key);
    let url = Url::parse(&url_str).map_err(|e| {
        AppError::localized(
            "s3.url.invalid",
            format!("S3 URL 无效: {e}"),
            format!("Invalid S3 URL: {e}"),
        )
    })?;

    let client = http_client::get();
    let body_hash = sha256_hex(&bytes);
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert("content-type", content_type.parse().unwrap());
    sign_request("PUT", &url, &mut headers, &body_hash, creds, chrono::Utc::now());

    let resp = client
        .put(url.as_str())
        .headers(headers)
        .body(bytes)
        .timeout(Duration::from_secs(TRANSFER_TIMEOUT_SECS))
        .send()
        .await
        .map_err(|e| s3_transport_error("s3.put_failed", "PUT 请求", "PUT request", &e))?;

    if resp.status().is_success() {
        return Ok(());
    }
    Err(s3_status_error("PUT", resp.status(), &url_str))
}

/// Download an S3 object. Returns `None` if the object does not exist (404).
///
/// On success returns `(body_bytes, optional_etag)`.
pub(crate) async fn get_object(
    creds: &S3Credentials,
    key: &str,
    max_bytes: usize,
) -> Result<Option<(Vec<u8>, Option<String>)>, AppError> {
    let url_str = build_object_url(creds, key);
    let url = Url::parse(&url_str).map_err(|e| {
        AppError::localized(
            "s3.url.invalid",
            format!("S3 URL 无效: {e}"),
            format!("Invalid S3 URL: {e}"),
        )
    })?;

    let client = http_client::get();
    let body_hash = sha256_hex(b"");
    let mut headers = reqwest::header::HeaderMap::new();
    sign_request("GET", &url, &mut headers, &body_hash, creds, chrono::Utc::now());

    let resp = client
        .get(url.as_str())
        .headers(headers)
        .timeout(Duration::from_secs(TRANSFER_TIMEOUT_SECS))
        .send()
        .await
        .map_err(|e| s3_transport_error("s3.get_failed", "GET 请求", "GET request", &e))?;

    if resp.status() == StatusCode::NOT_FOUND {
        return Ok(None);
    }
    if !resp.status().is_success() {
        return Err(s3_status_error("GET", resp.status(), &url_str));
    }
    ensure_content_length_within_limit(resp.headers(), max_bytes, &url_str)?;

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
                "s3.response_read_failed",
                format!("读取 S3 响应失败: {e}"),
                format!("Failed to read S3 response: {e}"),
            )
        })?;
        if bytes.len().saturating_add(chunk.len()) > max_bytes {
            return Err(response_too_large_error(&url_str, max_bytes));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(Some((bytes, etag)))
}

/// Retrieve the ETag of an S3 object via HEAD. Returns `None` on 404.
pub(crate) async fn head_object(
    creds: &S3Credentials,
    key: &str,
) -> Result<Option<String>, AppError> {
    let url_str = build_object_url(creds, key);
    let url = Url::parse(&url_str).map_err(|e| {
        AppError::localized(
            "s3.url.invalid",
            format!("S3 URL 无效: {e}"),
            format!("Invalid S3 URL: {e}"),
        )
    })?;

    let client = http_client::get();
    let body_hash = sha256_hex(b"");
    let mut headers = reqwest::header::HeaderMap::new();
    sign_request("HEAD", &url, &mut headers, &body_hash, creds, chrono::Utc::now());

    let resp = client
        .head(url.as_str())
        .headers(headers)
        .timeout(Duration::from_secs(DEFAULT_TIMEOUT_SECS))
        .send()
        .await
        .map_err(|e| s3_transport_error("s3.head_failed", "HEAD 请求", "HEAD request", &e))?;

    if resp.status() == StatusCode::NOT_FOUND {
        return Ok(None);
    }
    if !resp.status().is_success() {
        return Err(s3_status_error("HEAD", resp.status(), &url_str));
    }
    Ok(resp
        .headers()
        .get("etag")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string()))
}

// ─── Tests ───────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    // ── Crypto helpers ──

    #[test]
    fn sha256_hex_empty_body() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn sha256_hex_known_value() {
        // From the AWS S3 documentation PUT Object example body.
        assert_eq!(
            sha256_hex(b"Welcome to Amazon S3."),
            "44ce7dd67c959e0d3524ffac1771dfbba87d2b6b4b4e99e42034a8b803f8b072"
        );
    }

    #[test]
    fn hmac_sha256_rfc2104_test_vector() {
        // HMAC-SHA256("key", "The quick brown fox jumps over the lazy dog")
        let result = hmac_sha256(b"key", b"The quick brown fox jumps over the lazy dog");
        let hex: String = result.iter().map(|b| format!("{:02x}", b)).collect();
        assert_eq!(
            hex,
            "f7bc83f430538424b13298e6aa6fb143ef4d59a14946175997479dbc2d1a3cd8"
        );
    }

    // ── URL construction: virtual-hosted vs path-style ──

    #[test]
    fn build_object_url_virtual_hosted_style_aws() {
        let creds = test_creds("", "us-east-1", "mybucket");
        assert_eq!(
            build_object_url(&creds, "path/to/file.json"),
            "https://mybucket.s3.us-east-1.amazonaws.com/path/to/file.json"
        );
    }

    #[test]
    fn build_object_url_virtual_hosted_explicit_aws_endpoint() {
        let creds = test_creds(
            "s3.ap-northeast-1.amazonaws.com",
            "ap-northeast-1",
            "mybucket",
        );
        assert_eq!(
            build_object_url(&creds, "key.txt"),
            "https://mybucket.s3.ap-northeast-1.amazonaws.com/key.txt"
        );
    }

    #[test]
    fn build_object_url_path_style_custom_endpoint() {
        let creds = test_creds("minio.example.com:9000", "us-east-1", "mybucket");
        assert_eq!(
            build_object_url(&creds, "path/to/file.json"),
            "https://minio.example.com:9000/mybucket/path/to/file.json"
        );
    }

    #[test]
    fn build_object_url_strips_leading_slash_from_key() {
        let creds = test_creds("", "us-east-1", "b");
        assert_eq!(
            build_object_url(&creds, "/leading/slash.txt"),
            "https://b.s3.us-east-1.amazonaws.com/leading/slash.txt"
        );
    }

    #[test]
    fn build_bucket_url_aws() {
        let creds = test_creds("", "us-west-2", "testbucket");
        assert_eq!(
            build_bucket_url(&creds),
            "https://testbucket.s3.us-west-2.amazonaws.com/"
        );
    }

    #[test]
    fn build_bucket_url_custom_endpoint() {
        let creds = test_creds("storage.example.com", "us-east-1", "data");
        assert_eq!(
            build_bucket_url(&creds),
            "https://storage.example.com/data/"
        );
    }

    #[test]
    fn build_object_url_endpoint_with_trailing_slash() {
        let creds = test_creds("minio.local:9000/", "us-east-1", "b");
        assert_eq!(build_object_url(&creds, "k"), "https://minio.local:9000/b/k");
    }

    #[test]
    fn build_object_url_endpoint_with_scheme_prefix() {
        let creds = test_creds("https://minio.local:9000", "us-east-1", "b");
        assert_eq!(build_object_url(&creds, "k"), "https://minio.local:9000/b/k");
    }

    // ── HTTP scheme preservation (MinIO support) ──

    #[test]
    fn build_object_url_preserves_http_scheme() {
        let creds = test_creds("http://minio:9000", "us-east-1", "mybucket");
        let url = build_object_url(&creds, "path/to/file.json");
        assert!(
            url.starts_with("http://"),
            "expected http:// scheme, got: {url}"
        );
        assert_eq!(url, "http://minio:9000/mybucket/path/to/file.json");
    }

    #[test]
    fn build_object_url_preserves_https_scheme() {
        let creds = test_creds("https://storage.example.com", "us-east-1", "mybucket");
        let url = build_object_url(&creds, "path/to/file.json");
        assert!(
            url.starts_with("https://"),
            "expected https:// scheme, got: {url}"
        );
        assert_eq!(
            url,
            "https://storage.example.com/mybucket/path/to/file.json"
        );
    }

    #[test]
    fn build_object_url_bare_endpoint_defaults_to_https() {
        let creds = test_creds("minio:9000", "us-east-1", "mybucket");
        let url = build_object_url(&creds, "path/to/file.json");
        assert!(
            url.starts_with("https://"),
            "bare endpoint should default to https://, got: {url}"
        );
        assert_eq!(url, "https://minio:9000/mybucket/path/to/file.json");
    }

    #[test]
    fn build_bucket_url_preserves_http_scheme() {
        let creds = test_creds("http://minio:9000", "us-east-1", "data");
        let url = build_bucket_url(&creds);
        assert!(
            url.starts_with("http://"),
            "expected http:// scheme, got: {url}"
        );
        assert_eq!(url, "http://minio:9000/data/");
    }

    // ── Endpoint detection ──

    #[test]
    fn is_aws_endpoint_detection() {
        assert!(is_aws_endpoint(""), "empty string = AWS");
        assert!(is_aws_endpoint("s3.us-east-1.amazonaws.com"));
        assert!(is_aws_endpoint("s3.amazonaws.com"));
        assert!(!is_aws_endpoint("minio.example.com"));
        assert!(!is_aws_endpoint("storage.googleapis.com"));
        assert!(!is_aws_endpoint("r2.cloudflarestorage.com"));
    }

    // ── URI encoding ──

    #[test]
    fn uri_encode_preserves_unreserved_chars() {
        assert_eq!(
            uri_encode("hello-world_test.txt~v2", true),
            "hello-world_test.txt~v2"
        );
    }

    #[test]
    fn uri_encode_encodes_spaces_and_special_chars() {
        assert_eq!(uri_encode("hello world", true), "hello%20world");
        assert_eq!(uri_encode("a+b=c&d", true), "a%2Bb%3Dc%26d");
    }

    #[test]
    fn uri_encode_slash_handling() {
        assert_eq!(uri_encode("path/to/file", false), "path/to/file");
        assert_eq!(uri_encode("path/to/file", true), "path%2Fto%2Ffile");
    }

    // ── AWS Signature V4 signing ──

    #[test]
    fn sig_v4_signing_against_aws_test_vector() {
        // Based on the AWS documentation example: GET /?lifecycle
        // https://docs.aws.amazon.com/AmazonS3/latest/API/sig-v4-header-based-auth.html
        let creds = S3Credentials {
            access_key_id: "AKIAIOSFODNN7EXAMPLE".to_string(),
            secret_access_key: "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY".to_string(),
            region: "us-east-1".to_string(),
            bucket: "examplebucket".to_string(),
            endpoint: String::new(),
        };

        let now = chrono::Utc.with_ymd_and_hms(2013, 5, 24, 0, 0, 0).unwrap();
        let url = Url::parse("https://examplebucket.s3.amazonaws.com/?lifecycle").unwrap();
        let body_hash = sha256_hex(b"");

        let mut headers = reqwest::header::HeaderMap::new();
        sign_request("GET", &url, &mut headers, &body_hash, &creds, now);

        let auth = headers.get("authorization").unwrap().to_str().unwrap();

        // Verify credential scope
        assert!(
            auth.starts_with(
                "AWS4-HMAC-SHA256 Credential=AKIAIOSFODNN7EXAMPLE/20130524/us-east-1/s3/aws4_request"
            ),
            "unexpected credential scope in: {auth}"
        );
        // Verify signed headers
        assert!(
            auth.contains("SignedHeaders=host;x-amz-content-sha256;x-amz-date"),
            "unexpected signed headers in: {auth}"
        );
        // Verify exact signature from AWS documentation
        assert!(
            auth.contains(
                "Signature=fea454ca298b7da1c68078a5d1bdbfbbe0d65c699e0f91ac7a200a0136783543"
            ),
            "signature mismatch in: {auth}"
        );
    }

    #[test]
    fn sig_v4_includes_content_type_when_present() {
        let creds = S3Credentials {
            access_key_id: "TESTKEY".to_string(),
            secret_access_key: "TESTSECRET".to_string(),
            region: "us-east-1".to_string(),
            bucket: "b".to_string(),
            endpoint: String::new(),
        };

        let now = chrono::Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        let url = Url::parse("https://b.s3.us-east-1.amazonaws.com/key.json").unwrap();
        let body_hash = sha256_hex(b"{}");

        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert("content-type", "application/json".parse().unwrap());
        sign_request("PUT", &url, &mut headers, &body_hash, &creds, now);

        let auth = headers.get("authorization").unwrap().to_str().unwrap();
        // content-type must appear in the signed headers
        assert!(
            auth.contains("content-type"),
            "content-type should be in signed headers: {auth}"
        );
    }

    #[test]
    fn sig_v4_signing_key_derivation() {
        // Verify the signing key derivation chain independently.
        // Using the same AWS example credentials and date.
        let secret = "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY";
        let datestamp = "20130524";
        let region = "us-east-1";

        let k_date = hmac_sha256(format!("AWS4{}", secret).as_bytes(), datestamp.as_bytes());
        let k_region = hmac_sha256(&k_date, region.as_bytes());
        let k_service = hmac_sha256(&k_region, b"s3");
        let k_signing = hmac_sha256(&k_service, b"aws4_request");

        // The signing key should be a 32-byte value (256 bits).
        assert_eq!(k_signing.len(), 32);

        // Verify it is deterministic — computing again yields the same result.
        let k_date2 = hmac_sha256(format!("AWS4{}", secret).as_bytes(), datestamp.as_bytes());
        let k_region2 = hmac_sha256(&k_date2, region.as_bytes());
        let k_service2 = hmac_sha256(&k_region2, b"s3");
        let k_signing2 = hmac_sha256(&k_service2, b"aws4_request");
        assert_eq!(k_signing, k_signing2);
    }

    // ── Redact URL ──

    #[test]
    fn redact_url_strips_query_params() {
        let r = redact_url(
            "https://mybucket.s3.us-east-1.amazonaws.com/file.txt?X-Amz-Credential=AKID&X-Amz-Signature=abc",
        );
        assert!(!r.contains("AKID"));
        assert!(!r.contains("abc"));
        assert!(r.contains("mybucket.s3.us-east-1.amazonaws.com"));
        assert!(r.contains("/file.txt"));
    }

    #[test]
    fn redact_url_preserves_path() {
        let r = redact_url("https://minio.local:9000/bucket/path/to/file.json");
        assert_eq!(r, "https://minio.local:9000/bucket/path/to/file.json");
    }

    // ── Content-length check ──

    #[test]
    fn ensure_content_length_within_limit_rejects_oversized() {
        use reqwest::header::{HeaderMap, HeaderValue, CONTENT_LENGTH};
        let mut h = HeaderMap::new();
        h.insert(CONTENT_LENGTH, HeaderValue::from_static("2048"));
        assert!(ensure_content_length_within_limit(&h, 1024, "https://example.com").is_err());
    }

    #[test]
    fn ensure_content_length_within_limit_accepts_within_bounds() {
        use reqwest::header::{HeaderMap, HeaderValue, CONTENT_LENGTH};
        let mut h = HeaderMap::new();
        h.insert(CONTENT_LENGTH, HeaderValue::from_static("512"));
        assert!(ensure_content_length_within_limit(&h, 1024, "https://example.com").is_ok());

        let empty = HeaderMap::new();
        assert!(ensure_content_length_within_limit(&empty, 1024, "https://example.com").is_ok());
    }

    // ── Helper ──

    fn test_creds(endpoint: &str, region: &str, bucket: &str) -> S3Credentials {
        S3Credentials {
            access_key_id: String::new(),
            secret_access_key: String::new(),
            region: region.to_string(),
            bucket: bucket.to_string(),
            endpoint: endpoint.to_string(),
        }
    }
}

// ─── Live integration tests (run with --ignored) ─────────────

#[cfg(test)]
mod integration_tests {
    use super::*;

    fn test_creds() -> S3Credentials {
        S3Credentials {
            access_key_id: std::env::var("S3_TEST_AK").expect("S3_TEST_AK env required"),
            secret_access_key: std::env::var("S3_TEST_SK").expect("S3_TEST_SK env required"),
            region: std::env::var("S3_TEST_REGION").unwrap_or_else(|_| "us-east-1".to_string()),
            bucket: std::env::var("S3_TEST_BUCKET").expect("S3_TEST_BUCKET env required"),
            endpoint: std::env::var("S3_TEST_ENDPOINT").unwrap_or_default(),
        }
    }

    #[tokio::test]
    #[ignore]
    async fn live_s3_connection() {
        crate::proxy::http_client::init(None).ok();
        let creds = test_creds();
        let result = test_connection(&creds).await;
        assert!(result.is_ok(), "Connection failed: {:?}", result.err());
        println!("PASS: test_connection OK");
    }

    #[tokio::test]
    #[ignore]
    async fn live_s3_put_get_head_roundtrip() {
        crate::proxy::http_client::init(None).ok();
        let creds = test_creds();
        let key = "cc-switch-sync/v2/default/_integration_test.json";
        let data = br#"{"test":true,"ts":12345}"#;

        // PUT
        let r = put_object(&creds, key, data.to_vec(), "application/json").await;
        assert!(r.is_ok(), "PUT failed: {:?}", r.err());
        println!("PASS: put_object {} bytes", data.len());

        // GET
        let r = get_object(&creds, key, 1 << 20).await;
        assert!(r.is_ok(), "GET failed: {:?}", r.err());
        let (body, etag) = r.unwrap().expect("should exist");
        assert_eq!(body, data);
        println!("PASS: get_object {} bytes, etag={:?}", body.len(), etag);

        // HEAD
        let r = head_object(&creds, key).await;
        assert!(r.is_ok(), "HEAD failed: {:?}", r.err());
        assert!(r.unwrap().is_some());
        println!("PASS: head_object OK");

        // 404
        let r = get_object(&creds, "cc-switch-sync/_no_such_key", 1024).await;
        assert!(r.is_ok());
        assert!(r.unwrap().is_none());
        println!("PASS: get_object(404) returned None");

        println!("ALL LIVE S3 TESTS PASSED");
    }
}
