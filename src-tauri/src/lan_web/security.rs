//! Security middleware for the LAN Mobile Web UI server:
//! - Strict `Origin` + `Sec-Fetch-Site` CSRF guard on `/api/*` (no wildcard CORS,
//!   rejects `Origin: null` and cross-origin requests).
//! - Security response headers (`CSP`, `X-Content-Type-Options`, `Referrer-Policy`,
//!   `X-Frame-Options`) on every response.
//! - `HttpOnly; SameSite=Strict` session cookie parsing & verification.

use super::config::{verify_session_cookie, SESSION_COOKIE_NAME};
use axum::{
    extract::Request,
    http::{header, HeaderMap, HeaderValue, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

const CSP_VALUE: &str = "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self' data:; connect-src 'self'; object-src 'none'; frame-ancestors 'none'; base-uri 'self'";

/// Attach strict security headers to every LAN web response.
/// API responses additionally receive `Cache-Control: no-store`.
pub async fn security_headers(request: Request, next: Next) -> Response {
    let is_api = request.uri().path().starts_with("/api/");
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    headers.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(CSP_VALUE),
    );
    if is_api {
        headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    }
    response
}

/// Reject cross-origin requests to `/api/*` before any auth or business logic runs.
///
/// Follows `@farion1231`'s Origin-first veto principle from #8454:
/// 1. If `Sec-Fetch-Site` is present and not `"same-origin"` or `"none"` -> 403.
/// 2. If `Origin` is present:
///    - `"null"` or malformed -> 403.
///    - Must be `http://` or `https://` and match the request's `Host` authority
///      (allowing loopback aliases `localhost` / `127.0.0.1` / `[::1]` on the same port).
pub async fn origin_guard(request: Request, next: Next) -> Response {
    if request.uri().path().starts_with("/api/") {
        if let Err(reason) = check_origin_and_fetch_site(request.headers()) {
            log::warn!(
                "[LanWeb] rejected {} {}: {reason}",
                request.method(),
                request.uri().path()
            );
            return (
                StatusCode::FORBIDDEN,
                Json(json!({ "error": format!("Forbidden: {reason}") })),
            )
                .into_response();
        }
    }
    next.run(request).await
}

pub fn check_origin_and_fetch_site(headers: &HeaderMap) -> Result<(), &'static str> {
    if let Some(raw_site) = headers.get("sec-fetch-site") {
        let site = raw_site
            .to_str()
            .map(str::trim)
            .unwrap_or("")
            .to_ascii_lowercase();
        if !site.is_empty() && site != "same-origin" && site != "none" {
            return Err("cross-origin request blocked (Sec-Fetch-Site)");
        }
    }

    if let Some(raw_origin) = headers.get(header::ORIGIN) {
        let origin_str = raw_origin
            .to_str()
            .map(str::trim)
            .map_err(|_| "invalid Origin header")?;
        if origin_str.is_empty() || origin_str.eq_ignore_ascii_case("null") {
            return Err("null Origin is not allowed");
        }
        let origin_url = url::Url::parse(origin_str).map_err(|_| "malformed Origin header")?;
        if origin_url.scheme() != "http" && origin_url.scheme() != "https" {
            return Err("unsupported Origin scheme");
        }
        let origin_host = origin_url.host_str().ok_or("Origin missing host")?;
        let origin_port = origin_url.port_or_known_default();

        let host_header = headers
            .get(header::HOST)
            .and_then(|v| v.to_str().ok())
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .ok_or("missing Host header")?;

        let (req_host, req_port) = parse_host_authority(host_header, origin_url.scheme())
            .ok_or("malformed Host header")?;

        let same_port = origin_port == req_port;
        let same_host = hosts_equivalent(origin_host, &req_host);

        if !(same_port && same_host) {
            return Err("Origin does not match Host");
        }
    }

    Ok(())
}

fn parse_host_authority(host_header: &str, scheme: &str) -> Option<(String, Option<u16>)> {
    if host_header.contains('/')
        || host_header.contains('\\')
        || host_header.contains('@')
        || host_header.contains('?')
        || host_header.contains('#')
    {
        return None;
    }
    let synthetic = format!("{scheme}://{host_header}/");
    let parsed = url::Url::parse(&synthetic).ok()?;
    if !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.path() != "/"
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return None;
    }
    let host = parsed.host_str()?.to_string();
    let port = parsed.port_or_known_default();
    Some((host, port))
}

fn hosts_equivalent(a: &str, b: &str) -> bool {
    let na = normalize_host(a);
    let nb = normalize_host(b);
    if na == nb {
        return true;
    }
    is_loopback_host(na) && is_loopback_host(nb)
}

fn normalize_host(host: &str) -> &str {
    host.trim()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .trim_end_matches('.')
}

fn is_loopback_host(host: &str) -> bool {
    host.eq_ignore_ascii_case("localhost") || host == "127.0.0.1" || host == "::1"
}

/// Extract and validate the `ccs_lan_session` cookie from request headers.
/// Checks every cookie with that name in case a browser sends duplicates across paths.
pub fn has_valid_session_cookie(headers: &HeaderMap, active_token: &str) -> bool {
    if active_token.is_empty() {
        return false;
    }
    for value in headers.get_all(header::COOKIE) {
        let Ok(raw) = value.to_str() else {
            continue;
        };
        for pair in raw.split(';') {
            let pair = pair.trim();
            if let Some((name, val)) = pair.split_once('=') {
                if name.trim() == SESSION_COOKIE_NAME
                    && verify_session_cookie(val.trim(), active_token)
                {
                    return true;
                }
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lan_web::config::{derive_session_cookie, generate_token};

    fn make_headers(pairs: &[(&'static str, &str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (k, v) in pairs {
            map.append(
                header::HeaderName::from_static(k),
                HeaderValue::from_str(v).unwrap(),
            );
        }
        map
    }

    #[test]
    fn origin_guard_allows_same_origin_and_loopback_alias() {
        let h1 = make_headers(&[
            ("host", "192.168.1.10:15722"),
            ("origin", "http://192.168.1.10:15722"),
            ("sec-fetch-site", "same-origin"),
        ]);
        assert!(check_origin_and_fetch_site(&h1).is_ok());

        let h2 = make_headers(&[
            ("host", "127.0.0.1:15722"),
            ("origin", "http://localhost:15722"),
            ("sec-fetch-site", "same-origin"),
        ]);
        assert!(check_origin_and_fetch_site(&h2).is_ok());

        // Non-browser curl/API calls without Origin or Sec-Fetch-Site pass Origin guard
        // (and are then gated by session cookie authentication).
        let h3 = make_headers(&[("host", "127.0.0.1:15722")]);
        assert!(check_origin_and_fetch_site(&h3).is_ok());
    }

    #[test]
    fn origin_guard_blocks_cross_origin_null_and_port_mismatch() {
        let evil = make_headers(&[
            ("host", "192.168.1.10:15722"),
            ("origin", "https://evil.example"),
        ]);
        assert!(check_origin_and_fetch_site(&evil).is_err());

        let null_origin = make_headers(&[("host", "192.168.1.10:15722"), ("origin", "null")]);
        assert!(check_origin_and_fetch_site(&null_origin).is_err());

        let other_port = make_headers(&[
            ("host", "192.168.1.10:15722"),
            ("origin", "http://192.168.1.10:9999"),
        ]);
        assert!(check_origin_and_fetch_site(&other_port).is_err());

        let cross_site_fetch = make_headers(&[
            ("host", "192.168.1.10:15722"),
            ("sec-fetch-site", "cross-site"),
        ]);
        assert!(check_origin_and_fetch_site(&cross_site_fetch).is_err());

        let same_site_other_port = make_headers(&[
            ("host", "192.168.1.10:15722"),
            ("sec-fetch-site", "same-site"),
        ]);
        assert!(check_origin_and_fetch_site(&same_site_other_port).is_err());

        // Malformed Host authority with userinfo / path / query is rejected
        for bad_host in [
            "evil@192.168.1.10:15722",
            "192.168.1.10:15722/extra",
            "192.168.1.10:15722?a=1",
            "192.168.1.10:15722#frag",
        ] {
            let h = make_headers(&[("host", bad_host), ("origin", "http://192.168.1.10:15722")]);
            assert!(
                check_origin_and_fetch_site(&h).is_err(),
                "expected bad host {bad_host} to fail"
            );
        }
    }

    #[test]
    fn session_cookie_verification_handles_multiple_cookies() {
        let token = generate_token();
        let valid = derive_session_cookie(&token);
        let h = make_headers(&[(
            "cookie",
            &format!("other=1; {SESSION_COOKIE_NAME}=stale; {SESSION_COOKIE_NAME}={valid}"),
        )]);
        assert!(has_valid_session_cookie(&h, &token));

        let bad = make_headers(&[("cookie", &format!("{SESSION_COOKIE_NAME}=wrong"))]);
        assert!(!has_valid_session_cookie(&bad, &token));
    }
}
