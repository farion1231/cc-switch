//! Deep link module tests

use super::mcp::parse_mcp_apps;
use super::parser::parse_deeplink_url;
use super::prompt::import_prompt_from_deeplink;
use super::provider::parse_and_merge_config;
use super::utils::{infer_homepage_from_endpoint, validate_url};
use super::DeepLinkImportRequest;
use crate::AppType;
use crate::{store::AppState, Database};
use base64::prelude::*;
use std::sync::Arc;

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
fn test_parse_deeplink_with_notes() {
    let url = "ccswitch://v1/import?resource=provider&app=codex&name=Codex&homepage=https%3A%2F%2Fcodex.com&endpoint=https%3A%2F%2Fapi.codex.com&apiKey=key123&notes=Test%20notes";

    let request = parse_deeplink_url(url).unwrap();

    assert_eq!(request.notes, Some("Test notes".to_string()));
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
        config: None,
        config_format: None,
        config_url: None,
        extra_env: None,
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
        config: None,
        config_format: None,
        config_url: None,
        extra_env: None,
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
        config: Some(config_b64),
        config_format: Some("json".to_string()),
        config_url: None,
        extra_env: None,
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
        config: Some(config_b64),
        config_format: Some("json".to_string()),
        config_url: None,
        extra_env: None,
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

// =============================================================================
// Prompt Tests
// =============================================================================

#[test]
fn test_import_prompt_allows_space_in_base64_content() {
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

// =============================================================================
// Extra Env Tests (v3.10+)
// =============================================================================

fn provider_request_fixture(
    app: &str,
    name: &str,
    api_key: &str,
    extra_env: Option<String>,
) -> DeepLinkImportRequest {
    DeepLinkImportRequest {
        version: "v1".to_string(),
        resource: "provider".to_string(),
        app: Some(app.to_string()),
        name: Some(name.to_string()),
        homepage: Some("https://example.com".to_string()),
        endpoint: Some("https://api.example.com".to_string()),
        api_key: Some(api_key.to_string()),
        extra_env,
        ..Default::default()
    }
}

fn claude_request_fixture(extra_env: Option<String>) -> DeepLinkImportRequest {
    provider_request_fixture("claude", "Test", "sk-test", extra_env)
}

fn gemini_request_fixture(extra_env: Option<String>) -> DeepLinkImportRequest {
    provider_request_fixture("gemini", "Test Gemini", "test-api-key", extra_env)
}

fn usage_script_fixture_b64() -> String {
    BASE64_STANDARD.encode("async function main() { return []; }".as_bytes())
}

#[test]
fn test_parse_provider_with_extra_env() {
    let extra_env_json =
        r#"{"CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS":"1","ENABLE_TOOL_SEARCH":"true"}"#;
    // Use URL-safe base64 (no + or /) to avoid URL-encoding issues in test string
    let extra_env_b64 = BASE64_URL_SAFE_NO_PAD.encode(extra_env_json.as_bytes());
    let url = format!(
        "ccswitch://v1/import?resource=provider&app=claude&name=Test&endpoint=https%3A%2F%2Fapi.example.com&apiKey=sk-test&extraEnv={}",
        extra_env_b64
    );

    let request = parse_deeplink_url(&url).unwrap();
    assert_eq!(request.extra_env, Some(extra_env_b64));
}

#[test]
fn test_build_claude_settings_with_extra_env() {
    use super::provider::build_provider_from_request;

    let extra_env_json = r#"{"CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS":"1","ENABLE_TOOL_SEARCH":"true","CLAUDE_CODE_EFFORT_LEVEL":"max"}"#;
    let extra_env_b64 = BASE64_STANDARD.encode(extra_env_json.as_bytes());

    let mut request = claude_request_fixture(Some(extra_env_b64));
    request.model = Some("k2.6".to_string());
    request.haiku_model = Some("k2.6".to_string());
    request.sonnet_model = Some("k2.6".to_string());
    request.opus_model = Some("k2.6".to_string());

    let provider = build_provider_from_request(&AppType::Claude, &request).unwrap();
    let env = provider.settings_config["env"].as_object().unwrap();

    // Standard fields
    assert_eq!(env["ANTHROPIC_AUTH_TOKEN"], "sk-test");
    assert_eq!(env["ANTHROPIC_BASE_URL"], "https://api.example.com");
    assert_eq!(env["ANTHROPIC_MODEL"], "k2.6");

    // Extra env fields merged in
    assert_eq!(
        env["CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS"], "1",
        "extra_env should merge CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS"
    );
    assert_eq!(
        env["ENABLE_TOOL_SEARCH"], "true",
        "extra_env should merge ENABLE_TOOL_SEARCH"
    );
    assert_eq!(
        env["CLAUDE_CODE_EFFORT_LEVEL"], "max",
        "extra_env should merge CLAUDE_CODE_EFFORT_LEVEL"
    );
}

#[test]
fn test_claude_usage_script_defaults_follow_extra_env_overrides() {
    use super::provider::build_provider_from_request;

    let extra_env_json = r#"{
        "ANTHROPIC_AUTH_TOKEN":"sk-usage-override",
        "ANTHROPIC_BASE_URL":"https://usage.example.com"
    }"#;
    let extra_env_b64 = BASE64_STANDARD.encode(extra_env_json.as_bytes());

    let mut request = claude_request_fixture(Some(extra_env_b64));
    request.usage_script = Some(usage_script_fixture_b64());

    let provider = build_provider_from_request(&AppType::Claude, &request).unwrap();
    let usage_script = provider
        .meta
        .as_ref()
        .and_then(|m| m.usage_script.as_ref())
        .expect("usage script metadata");

    assert_eq!(usage_script.api_key.as_deref(), Some("sk-usage-override"));
    assert_eq!(
        usage_script.base_url.as_deref(),
        Some("https://usage.example.com"),
    );
}

#[test]
fn test_claude_alternative_auth_key_replaces_default_auth_token() {
    use super::provider::build_provider_from_request;

    let extra_env_json = r#"{
        "ANTHROPIC_API_KEY":"sk-api-key-override",
        "ANTHROPIC_BASE_URL":"https://api.example.com"
    }"#;
    let extra_env_b64 = BASE64_STANDARD.encode(extra_env_json.as_bytes());

    let provider = build_provider_from_request(
        &AppType::Claude,
        &claude_request_fixture(Some(extra_env_b64)),
    )
    .unwrap();
    let env = provider.settings_config["env"].as_object().unwrap();

    assert!(env.get("ANTHROPIC_AUTH_TOKEN").is_none());
    assert_eq!(env["ANTHROPIC_API_KEY"], "sk-api-key-override");
}

#[test]
fn test_claude_openrouter_key_replaces_default_auth_token() {
    use super::provider::build_provider_from_request;

    let extra_env_json = r#"{
        "OPENROUTER_API_KEY":"sk-openrouter-override",
        "ANTHROPIC_BASE_URL":"https://openrouter.ai/api"
    }"#;
    let extra_env_b64 = BASE64_STANDARD.encode(extra_env_json.as_bytes());

    let provider = build_provider_from_request(
        &AppType::Claude,
        &claude_request_fixture(Some(extra_env_b64)),
    )
    .unwrap();
    let env = provider.settings_config["env"].as_object().unwrap();

    assert!(env.get("ANTHROPIC_AUTH_TOKEN").is_none());
    assert_eq!(env["OPENROUTER_API_KEY"], "sk-openrouter-override");
}

#[test]
fn test_invalid_claude_alternative_auth_does_not_remove_default_auth_token() {
    use super::provider::build_provider_from_request;

    let extra_env_json = r#"{
        "ANTHROPIC_API_KEY":"",
        "ANTHROPIC_BASE_URL":"https://api.example.com"
    }"#;
    let extra_env_b64 = BASE64_STANDARD.encode(extra_env_json.as_bytes());

    let provider = build_provider_from_request(
        &AppType::Claude,
        &claude_request_fixture(Some(extra_env_b64)),
    )
    .unwrap();
    let env = provider.settings_config["env"].as_object().unwrap();

    assert_eq!(env["ANTHROPIC_AUTH_TOKEN"], "sk-test");
    assert!(env.get("ANTHROPIC_API_KEY").is_none());
}

#[test]
fn test_extra_env_stringifies_scalars_and_skips_invalid_values() {
    use super::provider::build_provider_from_request;

    let extra_env_json = r#"{
        "CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS": 1,
        "ENABLE_TOOL_SEARCH": true,
        "ANTHROPIC_AUTH_TOKEN": "",
        "ANTHROPIC_BASE_URL": 1,
        "OPENROUTER_API_KEY": false,
        "CLAUDE_CODE_EFFORT_LEVEL": "max",
        "IGNORED_ARRAY": ["bad"]
    }"#;
    let extra_env_b64 = BASE64_STANDARD.encode(extra_env_json.as_bytes());

    let provider = build_provider_from_request(
        &AppType::Claude,
        &claude_request_fixture(Some(extra_env_b64)),
    )
    .unwrap();
    let env = provider.settings_config["env"].as_object().unwrap();

    assert_eq!(env["CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS"], "1");
    assert_eq!(env["ENABLE_TOOL_SEARCH"], "true");
    assert_eq!(env["CLAUDE_CODE_EFFORT_LEVEL"], "max");
    assert_eq!(
        env["ANTHROPIC_AUTH_TOKEN"], "sk-test",
        "empty-string override should preserve the required auth token"
    );
    assert_eq!(
        env["ANTHROPIC_BASE_URL"], "https://api.example.com",
        "numeric override should preserve the required base URL"
    );
    assert!(env.get("OPENROUTER_API_KEY").is_none());
    assert!(env.get("IGNORED_ARRAY").is_none());
}

#[test]
fn test_build_gemini_settings_with_extra_env() {
    use super::provider::build_provider_from_request;

    let extra_env_json = r#"{
        "GEMINI_API_KEY":"override-key",
        "GOOGLE_GEMINI_BASE_URL":"https://override.example.com",
        "GEMINI_MODEL":"gemini-2.5-pro",
        "TRACE_SAMPLE_RATE":0.5,
        "feature.flag":"1"
    }"#;
    let extra_env_b64 = BASE64_STANDARD.encode(extra_env_json.as_bytes());

    let provider = build_provider_from_request(
        &AppType::Gemini,
        &gemini_request_fixture(Some(extra_env_b64)),
    )
    .unwrap();
    let env = provider.settings_config["env"].as_object().unwrap();

    assert_eq!(env["GEMINI_API_KEY"], "override-key");
    assert_eq!(
        env["GOOGLE_GEMINI_BASE_URL"],
        "https://override.example.com"
    );
    assert_eq!(env["GEMINI_MODEL"], "gemini-2.5-pro");
    assert_eq!(env["TRACE_SAMPLE_RATE"], "0.5");
    assert!(env.get("feature.flag").is_none());
}

#[test]
fn test_gemini_usage_script_defaults_follow_extra_env_overrides() {
    use super::provider::build_provider_from_request;

    let extra_env_json = r#"{
        "GEMINI_API_KEY":"gemini-usage-override",
        "GOOGLE_GEMINI_BASE_URL":"https://gemini-usage.example.com"
    }"#;
    let extra_env_b64 = BASE64_STANDARD.encode(extra_env_json.as_bytes());

    let mut request = gemini_request_fixture(Some(extra_env_b64));
    request.usage_script = Some(usage_script_fixture_b64());

    let provider = build_provider_from_request(&AppType::Gemini, &request).unwrap();
    let usage_script = provider
        .meta
        .as_ref()
        .and_then(|m| m.usage_script.as_ref())
        .expect("usage script metadata");

    assert_eq!(
        usage_script.api_key.as_deref(),
        Some("gemini-usage-override"),
    );
    assert_eq!(
        usage_script.base_url.as_deref(),
        Some("https://gemini-usage.example.com"),
    );
}

#[test]
fn test_gemini_empty_usage_api_key_falls_back_to_final_provider_key() {
    use super::provider::build_provider_from_request;

    let extra_env_json = r#"{
        "GEMINI_API_KEY":"gemini-usage-override",
        "GOOGLE_GEMINI_BASE_URL":"https://gemini-usage.example.com"
    }"#;
    let extra_env_b64 = BASE64_STANDARD.encode(extra_env_json.as_bytes());

    let mut request = gemini_request_fixture(Some(extra_env_b64));
    request.usage_script = Some(usage_script_fixture_b64());
    request.usage_api_key = Some(String::new());

    let provider = build_provider_from_request(&AppType::Gemini, &request).unwrap();
    let usage_script = provider
        .meta
        .as_ref()
        .and_then(|m| m.usage_script.as_ref())
        .expect("usage script metadata");

    assert_eq!(
        usage_script.api_key.as_deref(),
        Some("gemini-usage-override"),
    );
}

#[test]
fn test_gemini_extra_env_preserves_protected_fields_on_invalid_scalar_override() {
    use super::provider::build_provider_from_request;

    let extra_env_json = r#"{
        "GEMINI_API_KEY": true,
        "GOOGLE_GEMINI_BASE_URL": 1,
        "TRACE_SAMPLE_RATE":0.5
    }"#;
    let extra_env_b64 = BASE64_STANDARD.encode(extra_env_json.as_bytes());

    let provider = build_provider_from_request(
        &AppType::Gemini,
        &gemini_request_fixture(Some(extra_env_b64)),
    )
    .unwrap();
    let env = provider.settings_config["env"].as_object().unwrap();

    assert_eq!(env["GEMINI_API_KEY"], "test-api-key");
    assert_eq!(env["GOOGLE_GEMINI_BASE_URL"], "https://api.example.com");
    assert_eq!(env["TRACE_SAMPLE_RATE"], "0.5");
}

#[test]
fn test_extra_env_does_not_break_without_value() {
    use super::provider::build_provider_from_request;

    let request = claude_request_fixture(None);
    let provider = build_provider_from_request(&AppType::Claude, &request).unwrap();
    let env = provider.settings_config["env"].as_object().unwrap();

    // Should still have standard fields
    assert_eq!(env["ANTHROPIC_AUTH_TOKEN"], "sk-test");
    // Should not have extra env keys
    assert!(env.get("CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS").is_none());
}

#[test]
fn test_extra_env_ignores_invalid_base64() {
    use super::provider::build_provider_from_request;

    let request = claude_request_fixture(Some("!!!invalid-base64!!!".to_string()));

    // Should not panic; invalid extra_env is silently skipped
    let provider = build_provider_from_request(&AppType::Claude, &request).unwrap();
    let env = provider.settings_config["env"].as_object().unwrap();
    assert_eq!(env["ANTHROPIC_AUTH_TOKEN"], "sk-test");
}

#[test]
fn test_extra_env_ignores_valid_base64_non_json_object() {
    use super::provider::build_provider_from_request;

    let extra_env_b64 = BASE64_STANDARD.encode(br#"["not","an","object"]"#);
    let request = claude_request_fixture(Some(extra_env_b64));

    let provider = build_provider_from_request(&AppType::Claude, &request).unwrap();
    let env = provider.settings_config["env"].as_object().unwrap();
    assert_eq!(env["ANTHROPIC_AUTH_TOKEN"], "sk-test");
    assert!(env.get("not").is_none());
}

#[test]
fn test_extra_env_rejects_unsupported_provider_app() {
    use super::provider::build_provider_from_request;

    let extra_env_json = r#"{"OPENAI_API_KEY":"override-key"}"#;
    let extra_env_b64 = BASE64_STANDARD.encode(extra_env_json.as_bytes());
    let cases = [
        (AppType::Codex, "codex", "sk-codex"),
        (AppType::OpenCode, "opencode", "sk-opencode"),
        (AppType::OpenClaw, "openclaw", "sk-openclaw"),
        (AppType::Hermes, "hermes", "sk-hermes"),
    ];

    for (app_type, app_name, api_key) in cases {
        let request = provider_request_fixture(
            app_name,
            &format!("Test {app_name}"),
            api_key,
            Some(extra_env_b64.clone()),
        );

        let err = build_provider_from_request(&app_type, &request).unwrap_err();
        assert!(err
            .to_string()
            .contains("extraEnv is currently only supported for Claude, ClaudeDesktop and Gemini"));
    }
}
