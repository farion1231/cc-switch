use cc_switch_lib::live::project::codex::{CodexProjection, RowInput, ROUTE_ID};
use cc_switch_lib::{CodexModelConfig, ProviderMeta, UniversalProvider};

/// 辅助函数：构建一个启用了 Codex 的 UniversalProvider
fn make_codex_universal(
    base_url: &str,
    api_key: &str,
    meta: Option<ProviderMeta>,
) -> UniversalProvider {
    let mut p = UniversalProvider::new(
        "test-id".to_string(),
        "Test".to_string(),
        "custom".to_string(),
        base_url.to_string(),
        api_key.to_string(),
    );
    p.apps.codex = true;
    p.models.codex = Some(CodexModelConfig {
        model: Some("gpt-4o".to_string()),
        reasoning_effort: Some("high".to_string()),
    });
    p.meta = meta;
    p
}

/// 从生成的 Provider 中提取 config.toml 文本
fn extract_config_toml(provider: &cc_switch_lib::Provider) -> &str {
    provider
        .settings_config
        .get("config")
        .and_then(|v| v.as_str())
        .expect("config should be a toml string")
}

/// 直接验证 `catalog_input_text` 使用 `provider_meta` 中的 `codex_model_provider_id`。
#[test]
fn catalog_input_text_uses_custom_model_provider_id() {
    let settings = serde_json::json!({
        "auth": { "OPENAI_API_KEY": "sk-test" },
        "config": "model = \"gpt-4o\"\nmodel_provider = \"custom\"\n\n[model_providers.custom]\nbase_url = \"https://api.example.com\"\nwire_api = \"responses\"\n"
    });
    let projection = CodexProjection::of(&RowInput {
        settings: &settings,
        official: false,
        proxy_injected_oauth: false,
    })
    .expect("projection should succeed");

    // 无 meta 时默认 "custom"
    let text = projection.catalog_input_text(None);
    assert!(
        text.contains(&format!("model_provider = \"{ROUTE_ID}\"")),
        "expected default model_provider = \"custom\", got:\n{text}"
    );
    assert!(
        text.contains(&format!("[model_providers.{ROUTE_ID}]")),
        "expected [model_providers.custom] section, got:\n{text}"
    );

    // 设置了 codex_model_provider_id 时使用该值
    let meta = ProviderMeta {
        codex_model_provider_id: Some("my_gateway".to_string()),
        ..Default::default()
    };
    let text = projection.catalog_input_text(Some(&meta));
    assert!(
        text.contains("model_provider = \"my_gateway\""),
        "expected model_provider = \"my_gateway\", got:\n{text}"
    );
    assert!(
        text.contains("[model_providers.my_gateway]"),
        "expected [model_providers.my_gateway] section, got:\n{text}"
    );
    assert!(
        !text.contains("model_provider = \"custom\""),
        "should not contain default \"custom\" when custom id is set, got:\n{text}"
    );
    assert!(
        !text.contains("[model_providers.custom]"),
        "should not contain [model_providers.custom] when custom id is set, got:\n{text}"
    );

    // 空字符串 codex_model_provider_id 回退到默认
    let meta_empty = ProviderMeta {
        codex_model_provider_id: Some(String::new()),
        ..Default::default()
    };
    let text = projection.catalog_input_text(Some(&meta_empty));
    assert!(
        text.contains(&format!("model_provider = \"{ROUTE_ID}\"")),
        "empty codex_model_provider_id should fall back to default, got:\n{text}"
    );
}

#[test]
fn respects_custom_model_provider_id() {
    // 验证当 ProviderMeta 中设置了 codex_model_provider_id 时
    // 生成的 config.toml 使用该值而非 "custom"
    let meta = ProviderMeta {
        codex_model_provider_id: Some("my_gateway".to_string()),
        ..Default::default()
    };
    let universal = make_codex_universal("https://api.example.com", "sk-test", Some(meta));

    let provider = universal
        .to_codex_provider()
        .expect("should build codex provider");
    let toml = extract_config_toml(&provider);

    // model_provider 应该是用户自定义值
    assert!(
        toml.contains("model_provider = \"my_gateway\""),
        "expected model_provider = \"my_gateway\", got:\n{toml}"
    );
    // 节名也应一致
    assert!(
        toml.contains("[model_providers.my_gateway]"),
        "expected [model_providers.my_gateway] section, got:\n{toml}"
    );
    // 不应再出现 "custom"
    assert!(
        !toml.contains("model_provider = \"custom\""),
        "should not contain default \"custom\" model_provider, got:\n{toml}"
    );
    assert!(
        !toml.contains("[model_providers.custom]"),
        "should not contain [model_providers.custom] section, got:\n{toml}"
    );
}

#[test]
fn defaults_to_custom_when_unset() {
    // 验证未设置时默认仍为 "custom"
    let universal = make_codex_universal("https://api.example.com", "sk-test", None);

    let provider = universal
        .to_codex_provider()
        .expect("should build codex provider");
    let toml = extract_config_toml(&provider);

    assert!(
        toml.contains("model_provider = \"custom\""),
        "expected default model_provider = \"custom\", got:\n{toml}"
    );
    assert!(
        toml.contains("[model_providers.custom]"),
        "expected [model_providers.custom] section, got:\n{toml}"
    );
}

#[test]
fn defaults_to_custom_when_meta_exists_but_field_unset() {
    // 设置了 meta 但 codex_model_provider_id 为 None 时，仍应默认 "custom"
    let meta = ProviderMeta {
        codex_model_provider_id: None,
        ..Default::default()
    };
    let universal = make_codex_universal("https://api.example.com", "sk-test", Some(meta));

    let provider = universal
        .to_codex_provider()
        .expect("should build codex provider");
    let toml = extract_config_toml(&provider);

    assert!(
        toml.contains("model_provider = \"custom\""),
        "expected default model_provider = \"custom\", got:\n{toml}"
    );
    assert!(
        toml.contains("[model_providers.custom]"),
        "expected [model_providers.custom] section, got:\n{toml}"
    );
}
