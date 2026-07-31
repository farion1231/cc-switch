//! Antigravity authentication type detection
//!
//! Detects whether an Antigravity provider uses Google OAuth or generic API Key.

use crate::error::AppError;
use crate::provider::Provider;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AntigravityAuthType {
    GoogleOfficial,
    Generic,
}

const GOOGLE_OFFICIAL_PARTNER_KEY: &str = "google-official";

pub(crate) fn detect_antigravity_auth_type(provider: &Provider) -> AntigravityAuthType {
    if let Some(key) = provider
        .meta
        .as_ref()
        .and_then(|meta| meta.partner_promotion_key.as_deref())
    {
        if key.eq_ignore_ascii_case(GOOGLE_OFFICIAL_PARTNER_KEY) {
            return AntigravityAuthType::GoogleOfficial;
        }
    }

    let name_lower = provider.name.to_ascii_lowercase();
    if name_lower == "google" || name_lower.starts_with("google ") || name_lower.contains("antigravity") {
        return AntigravityAuthType::GoogleOfficial;
    }

    AntigravityAuthType::Generic
}

pub(crate) fn is_google_official_antigravity(provider: &Provider) -> bool {
    detect_antigravity_auth_type(provider) == AntigravityAuthType::GoogleOfficial
}

pub(crate) fn ensure_google_oauth_security_flag(provider: &Provider) -> Result<(), AppError> {
    if !is_google_official_antigravity(provider) {
        return Ok(());
    }

    // Write to Antigravity directory config.json (~/.gemini/config/config.json)
    use crate::antigravity_config::write_google_oauth_settings;
    write_google_oauth_settings()?;

    Ok(())
}
