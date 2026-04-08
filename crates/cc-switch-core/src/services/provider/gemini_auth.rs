//! Gemini provider auth mode helpers.

use crate::error::AppError;
use crate::provider::Provider;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GeminiAuthType {
    Packycode,
    GoogleOfficial,
    Generic,
}

const PACKYCODE_PARTNER_KEY: &str = "packycode";
const GOOGLE_OFFICIAL_PARTNER_KEY: &str = "google-official";
const PACKYCODE_KEYWORDS: [&str; 3] = ["packycode", "packyapi", "packy"];

pub(crate) fn detect_gemini_auth_type(provider: &Provider) -> GeminiAuthType {
    if let Some(key) = provider
        .meta
        .as_ref()
        .and_then(|meta| meta.partner_promotion_key.as_deref())
    {
        if key.eq_ignore_ascii_case(GOOGLE_OFFICIAL_PARTNER_KEY) {
            return GeminiAuthType::GoogleOfficial;
        }
        if key.eq_ignore_ascii_case(PACKYCODE_PARTNER_KEY) {
            return GeminiAuthType::Packycode;
        }
    }

    let name = provider.name.to_ascii_lowercase();
    if name == "google" || name.starts_with("google ") {
        return GeminiAuthType::GoogleOfficial;
    }

    if contains_packycode_keyword(&provider.name) {
        return GeminiAuthType::Packycode;
    }

    if let Some(site) = provider.website_url.as_deref() {
        if contains_packycode_keyword(site) {
            return GeminiAuthType::Packycode;
        }
    }

    if let Some(base_url) = provider
        .settings_config
        .pointer("/env/GOOGLE_GEMINI_BASE_URL")
        .and_then(|value| value.as_str())
    {
        if contains_packycode_keyword(base_url) {
            return GeminiAuthType::Packycode;
        }
    }

    GeminiAuthType::Generic
}

pub(crate) fn ensure_google_oauth_security_flag(provider: &Provider) -> Result<(), AppError> {
    if detect_gemini_auth_type(provider) != GeminiAuthType::GoogleOfficial {
        return Ok(());
    }

    crate::gemini_config::write_google_oauth_settings()
}

fn contains_packycode_keyword(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    PACKYCODE_KEYWORDS
        .iter()
        .any(|keyword| lower.contains(keyword))
}
