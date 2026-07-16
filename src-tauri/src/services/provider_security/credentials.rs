use crate::app_config::AppType;
use crate::error::AppError;
use crate::provider::Provider;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
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

pub fn base_urls_equivalent(
    stored: Option<&str>,
    candidate: Option<&str>,
) -> Result<bool, AppError> {
    match (stored, candidate) {
        (Some(stored), Some(candidate)) => {
            Ok(normalize_base_url(stored)? == normalize_base_url(candidate)?)
        }
        (None, None) => Ok(true),
        _ => Ok(false),
    }
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

/// Copy only explicitly confirmed credential fields from a Live settings snapshot.
/// Non-credential provider fields always remain sourced from the Project DB row.
pub fn apply_selected_credentials(
    target: &mut Provider,
    live_settings: &Value,
    app_type: &AppType,
    confirmed_fields: &BTreeSet<String>,
) -> Result<(), AppError> {
    if confirmed_fields
        .iter()
        .any(|field| !matches!(field.as_str(), "apiKey" | "baseUrl"))
    {
        return Err(AppError::InvalidInput(
            "unsupported credential field".to_string(),
        ));
    }

    let mut live_provider = target.clone();
    live_provider.settings_config = live_settings.clone();
    let live = extract_provider_credentials(&live_provider, app_type);

    let api_key = confirmed_fields
        .contains("apiKey")
        .then_some(live.api_key)
        .flatten();
    let base_url = confirmed_fields
        .contains("baseUrl")
        .then_some(live.base_url)
        .flatten();
    if (confirmed_fields.contains("apiKey") && api_key.is_none())
        || (confirmed_fields.contains("baseUrl") && base_url.is_none())
    {
        return Err(AppError::Message(
            "ERROR:provider_credentials_missing".to_string(),
        ));
    }

    let settings = object_mut(&mut target.settings_config);
    match app_type {
        AppType::Claude | AppType::ClaudeDesktop => {
            let env = nested_object_mut(settings, "env");
            set_selected(env, "ANTHROPIC_AUTH_TOKEN", api_key);
            set_selected(env, "ANTHROPIC_BASE_URL", base_url);
        }
        AppType::Gemini => {
            let env = nested_object_mut(settings, "env");
            set_selected(env, "GEMINI_API_KEY", api_key);
            set_selected(env, "GOOGLE_GEMINI_BASE_URL", base_url);
        }
        AppType::OpenCode => {
            let options = nested_object_mut(settings, "options");
            set_selected(options, "apiKey", api_key);
            set_selected(options, "baseURL", base_url);
        }
        AppType::OpenClaw => {
            set_selected(settings, "apiKey", api_key);
            set_selected(settings, "baseUrl", base_url);
        }
        AppType::Hermes => {
            set_selected(settings, "api_key", api_key);
            set_selected(settings, "base_url", base_url);
        }
        AppType::Codex => {
            if let Some(api_key) = api_key {
                nested_object_mut(settings, "auth")
                    .insert("OPENAI_API_KEY".to_string(), Value::String(api_key));
            }
            if let Some(base_url) = base_url {
                let config = settings
                    .get("config")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let updated =
                    crate::codex_config::update_codex_toml_field(config, "base_url", &base_url)
                        .map_err(AppError::InvalidInput)?;
                settings.insert("config".to_string(), Value::String(updated));
            }
        }
    }
    Ok(())
}

fn object_mut(value: &mut Value) -> &mut Map<String, Value> {
    if !value.is_object() {
        *value = Value::Object(Map::new());
    }
    value.as_object_mut().expect("object initialized")
}

fn nested_object_mut<'a>(
    parent: &'a mut Map<String, Value>,
    key: &str,
) -> &'a mut Map<String, Value> {
    object_mut(
        parent
            .entry(key.to_string())
            .or_insert_with(|| Value::Object(Map::new())),
    )
}

fn set_selected(target: &mut Map<String, Value>, key: &str, value: Option<String>) {
    if let Some(value) = value {
        target.insert(key.to_string(), Value::String(value));
    }
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
    fn equivalent_base_urls_compare_using_canonical_values() {
        assert!(base_urls_equivalent(
            Some(" HTTPS://Example.COM:443/v1/ "),
            Some("https://example.com/v1")
        )
        .unwrap());
        assert!(!base_urls_equivalent(Some("https://a.example"), None).unwrap());
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
