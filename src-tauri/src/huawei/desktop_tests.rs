// Included by the Desktop test module to reuse its generic provider fixtures.

#[test]
fn huawei_proxy_auth_does_not_require_static_key_but_requires_endpoint() {
    for provider_type in [
        "huawei_codeagent_2",
        "huawei_codeagent_3",
        "huawei_w3",
        "huawei_idea",
    ] {
        let mut provider = oauth_proxy_provider(provider_type, provider_type, "anthropic");
        validate_proxy_provider(&provider).expect("Huawei uses injected account credentials");
        provider.settings_config["env"]["ANTHROPIC_DEFAULT_SONNET_MODEL"] = json!("legacy-model");
        let routes = configured_proxy_model_routes(&provider).expect("explicit Desktop routes");
        assert_eq!(routes.len(), 1);
        assert_ne!(routes[0].upstream_model, "legacy-model");
        provider.settings_config["env"]["ANTHROPIC_BASE_URL"] = json!("");
        assert!(
            validate_proxy_provider(&provider).is_err(),
            "endpoint is still required"
        );
    }
    let ordinary = oauth_proxy_provider("ordinary", "anthropic", "anthropic");
    assert!(
        validate_proxy_provider(&ordinary).is_err(),
        "ordinary providers still need a key"
    );
}

#[test]
fn validate_direct_provider_rejects_huawei_codeagent_2() {
    let mut provider = direct_provider("huawei-2");
    provider.meta = Some(ProviderMeta {
        provider_type: Some("huawei_codeagent_2".to_string()),
        ..Default::default()
    });
    let result = validate_direct_provider(&provider);
    assert!(
        result.is_err(),
        "Direct mode should reject huawei_codeagent_2"
    );
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("\u{672c}\u{5730}\u{8def}\u{7531}")
            || msg.to_lowercase().contains("local routing"),
        "error should mention local routing mode, got: {msg}"
    );
}

#[test]
fn validate_direct_provider_rejects_huawei_codeagent_3() {
    let mut provider = direct_provider("huawei-3");
    provider.meta = Some(ProviderMeta {
        provider_type: Some("huawei_codeagent_3".to_string()),
        ..Default::default()
    });
    let result = validate_direct_provider(&provider);
    assert!(
        result.is_err(),
        "Direct mode should reject huawei_codeagent_3"
    );
}

#[test]
fn provider_mode_forces_proxy_for_huawei() {
    // 华为 CodeAgent 上游需要多头认证，Direct 会 401；provider_mode 必须在
    // 读取 claudeDesktopMode 之前强制返回 Proxy，即使 meta 未显式声明模式。
    let mut provider = direct_provider("huawei-2");
    provider.meta = Some(ProviderMeta {
        provider_type: Some("huawei_codeagent_2".to_string()),
        // 故意不设 claude_desktop_mode，验证 provider_mode 主动强制 Proxy
        ..Default::default()
    });
    assert_eq!(provider_mode(&provider), ClaudeDesktopMode::Proxy);
}

#[test]
fn proxy_model_routes_generates_defaults_for_huawei() {
    // 华为 preset 不填 claudeDesktopModelRoutes；proxy_model_routes 应从
    // env.ANTHROPIC_DEFAULT_*_MODEL 用 DEFAULT_PROXY_ROUTES 模板生成路由，
    // 并剥掉 [1M] 后缀（Claude Desktop schema 不接受，supports_1m 承载能力）。
    let mut provider = direct_provider("huawei-2");
    provider.settings_config = json!({
        "env": {
            "ANTHROPIC_BASE_URL": "https://snapengine.cida.cce.prod-szv-g.dragon.tools.huawei.com/api/v2",
            "ANTHROPIC_AUTH_TOKEN": "test-token",
            "ANTHROPIC_DEFAULT_SONNET_MODEL": "claude-sonnet-4.6[1M]",
            "ANTHROPIC_DEFAULT_OPUS_MODEL": "claude-opus-4[1M]",
            "ANTHROPIC_DEFAULT_HAIKU_MODEL": "claude-haiku-4.5",
        }
    });
    provider.meta = Some(ProviderMeta {
        provider_type: Some("huawei_codeagent_2".to_string()),
        claude_desktop_mode: Some(ClaudeDesktopMode::Proxy),
        ..Default::default()
    });

    let routes = proxy_model_routes(&provider).expect("routes for huawei");
    assert!(!routes.is_empty(), "should generate routes from env");
    assert!(
        routes.iter().all(|r| r.route_id.starts_with("claude-")),
        "route_ids should be claude-safe: {routes:?}"
    );
    assert!(
        routes.iter().all(|r| !r.upstream_model.is_empty()),
        "upstream_model should be non-empty: {routes:?}"
    );
    // [1M] / [1m] 后缀必须剥掉（supports_1m 承载能力）
    assert!(
        routes
            .iter()
            .all(|r| !r.upstream_model.to_ascii_lowercase().contains("[1m]")),
        "upstream_model should have [1M] suffix stripped: {routes:?}"
    );
    let sonnet = routes
        .iter()
        .find(|r| r.route_id == "claude-sonnet-5")
        .expect("sonnet route from DEFAULT_PROXY_ROUTES");
    assert_eq!(sonnet.upstream_model, "claude-sonnet-4.6");
    assert!(sonnet.supports_1m);
}

#[test]
fn validate_proxy_provider_accepts_huawei() {
    // 锁定行为：validate_proxy_provider 必须放行 huawei_codeagent_2/3
    // (api_format=anthropic 已在允许列表，路由从 env 默认生成)。
    // 若将来有改动意外回退华为放行，此测试应捕获。
    let mut provider = direct_provider("huawei-2");
    provider.settings_config = json!({
        "env": {
            "ANTHROPIC_BASE_URL": "https://snapengine.cida.cce.prod-szv-g.dragon.tools.huawei.com/api/v2",
            "ANTHROPIC_AUTH_TOKEN": "test-token",
            "ANTHROPIC_DEFAULT_SONNET_MODEL": "claude-sonnet-4.6[1M]",
        },
        "api_format": "anthropic",
    });
    provider.meta = Some(ProviderMeta {
        provider_type: Some("huawei_codeagent_2".to_string()),
        claude_desktop_mode: Some(ClaudeDesktopMode::Proxy),
        ..Default::default()
    });
    let result = validate_proxy_provider(&provider);
    assert!(
        result.is_ok(),
        "Proxy mode should accept huawei_codeagent_2 (api_format=anthropic): {:?}",
        result.err()
    );

    // huawei_codeagent_3 同理放行
    let mut provider3 = direct_provider("huawei-3");
    provider3.settings_config = json!({
        "env": {
            "ANTHROPIC_BASE_URL": "https://snapengine.cida.cce.prod-szv-g.dragon.tools.huawei.com/api/v2",
            "ANTHROPIC_AUTH_TOKEN": "test-token",
            "ANTHROPIC_DEFAULT_SONNET_MODEL": "claude-sonnet-4.6[1M]",
        },
        "api_format": "anthropic",
    });
    provider3.meta = Some(ProviderMeta {
        provider_type: Some("huawei_codeagent_3".to_string()),
        claude_desktop_mode: Some(ClaudeDesktopMode::Proxy),
        ..Default::default()
    });
    let result3 = validate_proxy_provider(&provider3);
    assert!(
        result3.is_ok(),
        "Proxy mode should accept huawei_codeagent_3 (api_format=anthropic): {:?}",
        result3.err()
    );
}
