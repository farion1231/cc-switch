//! Deep link utility functions
//!
//! Common helpers for URL validation, Base64 decoding, etc.

use crate::error::AppError;
use base64::prelude::*;
use std::net::{IpAddr, Ipv6Addr};
use url::Url;

/// Maximum allowed length for a deep link parameter value
const MAX_PARAM_LENGTH: usize = 8192;

/// Maximum allowed size for base64-decoded content
#[allow(dead_code)]
const MAX_DECODED_SIZE: usize = 512 * 1024; // 512 KB

/// Validate that a parameter value is within acceptable length limits
pub fn validate_param_length(value: &str, field_name: &str) -> Result<(), AppError> {
    if value.len() > MAX_PARAM_LENGTH {
        return Err(AppError::InvalidInput(format!(
            "'{field_name}' exceeds maximum length of {MAX_PARAM_LENGTH} characters"
        )));
    }
    Ok(())
}

/// Validate that decoded bytes are within size limits
#[allow(dead_code)]
pub fn validate_decoded_size(bytes: &[u8], field_name: &str) -> Result<(), AppError> {
    if bytes.len() > MAX_DECODED_SIZE {
        return Err(AppError::InvalidInput(format!(
            "'{field_name}' decoded content exceeds maximum size of {MAX_DECODED_SIZE} bytes"
        )));
    }
    Ok(())
}

/// Validate that a string is a valid HTTP(S) URL and not pointing to internal/private hosts
pub fn validate_url(url_str: &str, field_name: &str) -> Result<(), AppError> {
    validate_param_length(url_str, field_name)?;

    let url = Url::parse(url_str)
        .map_err(|e| AppError::InvalidInput(format!("Invalid URL for '{field_name}': {e}")))?;

    let scheme = url.scheme();
    if scheme != "http" && scheme != "https" {
        return Err(AppError::InvalidInput(format!(
            "Invalid URL scheme for '{field_name}': must be http or https, got '{scheme}'"
        )));
    }

    // SSRF protection: block requests to internal/private hosts
    if let Some(host_str) = url.host_str() {
        validate_host_not_internal(host_str, field_name)?;
    }

    Ok(())
}

/// Validate that a hostname does not resolve to localhost or private/internal IP ranges.
///
/// Prevents Server-Side Request Forgery (SSRF) attacks via deep link URLs.
fn validate_host_not_internal(host: &str, field_name: &str) -> Result<(), AppError> {
    // Block localhost variants
    let host_lower = host.to_lowercase();
    if host_lower == "localhost"
        || host_lower == "127.0.0.1"
        || host_lower == "::1"
        || host_lower == "0.0.0.0"
        || host_lower.ends_with(".local")
        || host_lower.ends_with(".internal")
    {
        return Err(AppError::InvalidInput(format!(
            "'{field_name}' URL must not point to localhost or internal network: '{host}'"
        )));
    }

    // Try parsing as IP address
    if let Ok(ip) = host.parse::<IpAddr>() {
        if is_private_ip(&ip) {
            return Err(AppError::InvalidInput(format!(
                "'{field_name}' URL must not point to private network address: '{host}'"
            )));
        }
    }

    Ok(())
}

/// Check if an IP address is in a private/non-routable range
fn is_private_ip(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_private()
                || v4.is_loopback()
                || v4.is_link_local()
                || v4.octets()[0] == 0 // 0.0.0.0/8 (current network)
                || v4.octets()[0] == 100 && v4.octets()[1] >= 64 && v4.octets()[1] <= 127 // 100.64.0.0/10 (CGN)
                || v4.octets()[0] == 169 && v4.octets()[1] == 254 // 169.254.0.0/16 (link-local)
                || (v4.octets()[0] == 198 && (v4.octets()[1] == 18 || v4.octets()[1] == 19)) // 198.18.0.0/15 (benchmark)
        }
        IpAddr::V6(v6) => {
            v6.is_loopback()
                || v6.is_unique_local()
                || v6.is_unicast_link_local()
                || *v6 == Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 0) // ::
                || *v6 == Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 1) // ::1
        }
    }
}

/// Validate API key format: must be non-empty and within reasonable length
pub fn validate_api_key(key: &str, field_name: &str) -> Result<(), AppError> {
    let trimmed = key.trim();
    if trimmed.is_empty() {
        return Err(AppError::InvalidInput(format!(
            "'{field_name}' cannot be empty"
        )));
    }
    if trimmed.len() > 1024 {
        return Err(AppError::InvalidInput(format!(
            "'{field_name}' exceeds maximum length of 1024 characters"
        )));
    }
    Ok(())
}

/// Validate provider name: must be non-empty and within reasonable length
pub fn validate_name(name: &str) -> Result<(), AppError> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(AppError::InvalidInput("Name cannot be empty".to_string()));
    }
    if trimmed.len() > 256 {
        return Err(AppError::InvalidInput(
            "Name exceeds maximum length of 256 characters".to_string(),
        ));
    }
    Ok(())
}

/// Decode a Base64 parameter from deep link URL
///
/// This function handles common issues with Base64 in URLs:
/// - `+` being decoded as space
/// - Missing padding `=`
/// - Both standard and URL-safe Base64 variants
pub fn decode_base64_param(field: &str, raw: &str) -> Result<Vec<u8>, AppError> {
    let mut candidates: Vec<String> = Vec::new();
    // Keep spaces (to restore `+`), but remove newlines
    let trimmed = raw.trim_matches(|c| c == '\r' || c == '\n');

    // First try restoring spaces to "+"
    if trimmed.contains(' ') {
        let replaced = trimmed.replace(' ', "+");
        if !replaced.is_empty() && !candidates.contains(&replaced) {
            candidates.push(replaced);
        }
    }

    // Original value
    if !trimmed.is_empty() && !candidates.contains(&trimmed.to_string()) {
        candidates.push(trimmed.to_string());
    }

    // Add padding variants
    let existing = candidates.clone();
    for candidate in existing {
        let mut padded = candidate.clone();
        let remainder = padded.len() % 4;
        if remainder != 0 {
            padded.extend(std::iter::repeat_n('=', 4 - remainder));
        }
        if !candidates.contains(&padded) {
            candidates.push(padded);
        }
    }

    let mut last_error: Option<String> = None;
    for candidate in candidates {
        for engine in [
            &BASE64_STANDARD,
            &BASE64_STANDARD_NO_PAD,
            &BASE64_URL_SAFE,
            &BASE64_URL_SAFE_NO_PAD,
        ] {
            match engine.decode(&candidate) {
                Ok(bytes) => return Ok(bytes),
                Err(err) => last_error = Some(err.to_string()),
            }
        }
    }

    Err(AppError::InvalidInput(format!(
        "{field} 参数 Base64 解码失败：{}。请确认链接参数已用 Base64 编码并经过 URL 转义（尤其是将 '+' 编码为 %2B，或使用 URL-safe Base64）。",
        last_error.unwrap_or_else(|| "未知错误".to_string())
    )))
}

/// Infer homepage URL from API endpoint
///
/// Examples:
/// - https://api.anthropic.com/v1 → https://anthropic.com
/// - https://api.openai.com/v1 → https://openai.com
/// - https://api-test.company.com/v1 → https://company.com
pub fn infer_homepage_from_endpoint(endpoint: &str) -> Option<String> {
    let url = Url::parse(endpoint).ok()?;
    let host = url.host_str()?;

    // Remove common API prefixes
    let clean_host = host
        .strip_prefix("api.")
        .or_else(|| host.strip_prefix("api-"))
        .unwrap_or(host);

    Some(format!("https://{clean_host}"))
}
