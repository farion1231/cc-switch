use crate::app_config::AppType;
use crate::error::AppError;
use crate::provider::Provider;
use sha2::{Digest, Sha256};
use url::Url;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CredentialFields {
    pub api_key: Option<String>,
    pub base_url: Option<String>,
}

/// Extract Provider Stored Credentials using the app-specific storage contract.
pub fn extract_provider_credentials(provider: &Provider, app_type: &AppType) -> CredentialFields {
    // `resolve_usage_credentials` returns (base_url, api_key). Bind by name here so
    // the public CredentialFields order cannot silently swap secret and URL.
    let (base_url, api_key) = match app_type {
        AppType::Claude
        | AppType::ClaudeDesktop
        | AppType::Codex
        | AppType::Gemini
        | AppType::OpenCode
        | AppType::OpenClaw
        | AppType::Hermes => provider.resolve_usage_credentials(app_type),
    };

    CredentialFields {
        api_key: non_empty(api_key),
        base_url: non_empty(base_url),
    }
}

fn non_empty(value: String) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

/// Canonicalize a provider base URL before comparing Stored and Live Credentials.
pub fn normalize_base_url(input: &str) -> Result<String, AppError> {
    let trimmed = input.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return Err(AppError::InvalidInput(
            "base URL cannot be empty".to_string(),
        ));
    }

    let mut url = Url::parse(trimmed)
        .map_err(|e| AppError::InvalidInput(format!("invalid base URL: {e}")))?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err(AppError::InvalidInput(
            "base URL must be an absolute HTTP(S) URL".to_string(),
        ));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(AppError::InvalidInput(
            "base URL must not contain user information".to_string(),
        ));
    }

    // url::Url canonicalizes the scheme/host and removes default ports. Fragments
    // never affect the API endpoint and are excluded from credential comparison.
    url.set_fragment(None);
    let path = url.path().trim_end_matches('/').to_string();
    url.set_path(if path.is_empty() { "/" } else { &path });

    let normalized = url.to_string();
    Ok(normalized.trim_end_matches('/').to_string())
}

pub fn mask_credential(value: &str) -> String {
    let char_count = value.chars().count();
    if char_count <= 8 {
        return "*".repeat(char_count.max(1));
    }

    let prefix: String = value.chars().take(4).collect();
    let suffix: String = value.chars().skip(char_count - 4).collect();
    format!("{prefix}{}{}", "*".repeat((char_count - 8).max(3)), suffix)
}

/// Domain-separated SHA-256. The field name prevents equal API-key and URL text
/// from sharing a fingerprint; no raw credential is returned or logged.
pub fn credential_fingerprint(field: &str, value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(field.as_bytes());
    hasher.update([0]);
    hasher.update(value.as_bytes());
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalized_base_url_equates_default_ports_case_and_trailing_slash() {
        assert_eq!(
            normalize_base_url(" HTTPS://Example.COM:443/v1/ ").unwrap(),
            "https://example.com/v1"
        );
        assert_eq!(
            normalize_base_url("http://EXAMPLE.com:80/").unwrap(),
            "http://example.com"
        );
    }

    #[test]
    fn rejects_non_http_and_userinfo_urls() {
        assert!(normalize_base_url("file:///tmp/provider").is_err());
        assert!(normalize_base_url("https://token@example.com/v1").is_err());
    }

    #[test]
    fn fingerprint_is_field_separated_and_never_contains_raw_value() {
        let raw = "sk-secret-value";
        let api = credential_fingerprint("api_key", raw);
        let url = credential_fingerprint("base_url", raw);
        assert_eq!(api.len(), 64);
        assert_ne!(api, url);
        assert!(!api.contains(raw));
    }

    #[test]
    fn masking_handles_short_and_unicode_credentials_without_panicking() {
        assert_eq!(mask_credential("short"), "*****");
        assert_eq!(mask_credential("abcd1234wxyz"), "abcd****wxyz");
        assert_eq!(mask_credential("密钥-abcd-结尾"), "密钥-a***d-结尾");
    }
}
