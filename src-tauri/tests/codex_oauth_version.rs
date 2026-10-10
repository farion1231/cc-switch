//! Tests for configurable Codex OAuth client version.
//!
//! Verifies that `resolve_oauth_client_version` correctly returns the custom
//! version when set, and falls back to the default constant when unset.

use cc_switch_lib::{resolve_oauth_client_version, ProviderMeta};

#[test]
fn custom_oauth_version_overrides_default() {
    let meta = ProviderMeta {
        codex_oauth_client_version: Some("0.160.0".to_string()),
        ..Default::default()
    };
    let version = resolve_oauth_client_version(&meta);
    assert_eq!(version, "0.160.0");
}

#[test]
fn default_oauth_version_when_unset() {
    let meta = ProviderMeta::default();
    let version = resolve_oauth_client_version(&meta);
    assert_eq!(version, "0.159.0");
}

#[test]
fn empty_oauth_version_falls_back_to_default() {
    // 空字符串应被视为未设置，回退到默认值
    let meta = ProviderMeta {
        codex_oauth_client_version: Some(String::new()),
        ..Default::default()
    };
    let version = resolve_oauth_client_version(&meta);
    assert_eq!(version, "0.159.0");
}

#[test]
fn version_field_roundtrip_serialization() {
    let meta = ProviderMeta {
        codex_oauth_client_version: Some("0.161.0".to_string()),
        ..Default::default()
    };
    let json = serde_json::to_value(&meta).expect("serialize ProviderMeta");
    let codex_oauth_client_version = json
        .get("codexOauthClientVersion")
        .and_then(|v| v.as_str());
    assert_eq!(codex_oauth_client_version, Some("0.161.0"));

    let parsed: ProviderMeta =
        serde_json::from_value(json).expect("deserialize ProviderMeta");
    assert_eq!(
        parsed.codex_oauth_client_version.as_deref(),
        Some("0.161.0")
    );
}

#[test]
fn version_field_deserialize_from_camel_case_json() {
    let json = serde_json::json!({
        "codexOauthClientVersion": "0.162.0"
    });
    let meta: ProviderMeta =
        serde_json::from_value(json).expect("deserialize ProviderMeta from JSON");
    assert_eq!(
        meta.codex_oauth_client_version.as_deref(),
        Some("0.162.0")
    );
}

#[test]
fn version_field_missing_in_json_defaults_to_none() {
    let json = serde_json::json!({});
    let meta: ProviderMeta =
        serde_json::from_value(json).expect("deserialize ProviderMeta from empty JSON");
    assert!(meta.codex_oauth_client_version.is_none());
}
