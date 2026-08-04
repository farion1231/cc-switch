//! 聚合路由（UniversalProvider cc_switch 类型）功能测试
//!
//! 验证：
//! - cc_switch 类型的 UP 派生 provider 携带 providerType 标记
//! - 非 cc_switch 类型不意外带标记

use cc_switch_lib::{
    CodexModelConfig, UniversalProvider,
};

/// cc_switch 类型 UP 的 to_*_provider() 应设置 meta.provider_type = "cc_switch"
#[test]
fn cc_switch_derived_provider_has_provider_type_marker() {
    let mut up = UniversalProvider::new(
        "u1".to_string(),
        "CC Switch".to_string(),
        "cc_switch".to_string(),
        "http://127.0.0.1:15721".to_string(),
        "".to_string(),
    );
    up.apps.claude = true;
    up.apps.codex = true;
    up.apps.gemini = true;
    up.models.codex = Some(CodexModelConfig {
        model: Some("gpt-4o-mini".to_string()),
        reasoning_effort: Some("high".to_string()),
    });

    let claude = up.to_claude_provider().expect("claude provider");
    assert_eq!(
        claude.meta.as_ref().and_then(|m| m.provider_type.as_deref()),
        Some("cc_switch"),
        "claude provider should carry cc_switch marker"
    );

    let codex = up.to_codex_provider().expect("codex provider");
    assert_eq!(
        codex.meta.as_ref().and_then(|m| m.provider_type.as_deref()),
        Some("cc_switch"),
        "codex provider should carry cc_switch marker"
    );

    let gemini = up.to_gemini_provider().expect("gemini provider");
    assert_eq!(
        gemini.meta.as_ref().and_then(|m| m.provider_type.as_deref()),
        Some("cc_switch"),
        "gemini provider should carry cc_switch marker"
    );
}

/// 非 cc_switch 类型 UP 的 to_*_provider() 不应设置 provider_type 标记
#[test]
fn non_cc_switch_derived_provider_has_no_provider_type() {
    let mut up = UniversalProvider::new(
        "u1".to_string(),
        "NewAPI".to_string(),
        "newapi".to_string(),
        "https://api.example.com".to_string(),
        "sk-key".to_string(),
    );
    up.apps.claude = true;

    let claude = up.to_claude_provider().expect("claude provider");
    assert_eq!(
        claude.meta.as_ref().and_then(|m| m.provider_type.as_deref()),
        None,
        "non-cc_switch provider should not carry cc_switch marker"
    );
}