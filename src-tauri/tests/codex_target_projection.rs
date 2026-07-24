use cc_switch_lib::project_codex_provider_config;

#[test]
fn provider_projection_changes_only_provider_owned_codex_fields() {
    let live = r#"# Target-owned values must survive Provider changes.
approval_policy = "on-request"
sandbox_mode = "workspace-write"
sqlite_home = "/home/mikasa/.local/share/codex"
model_provider = "old-route"
model = "old-model"
model_reasoning_effort = "medium"
web_search = "disabled"
future_target_setting = "keep-me"
model_instructions_file = "/home/mikasa/.codex/model-instructions.md"

[projects."/home/mikasa/work"]
trust_level = "trusted"

[mcp_servers.local]
command = "linux-only-command"

[model_providers.old-route]
name = "Old route"
base_url = "https://old.example/v1"
experimental_bearer_token = "old-secret"

[model_providers.handwritten]
name = "Keep this unrelated route"
base_url = "http://localhost:11434/v1"

[model_providers.legacy-new-alias]
name = "Legacy alias for the next managed route"
base_url = "https://new.example/v1"
wire_api = "responses"
legacy_timeout = 45
"#;
    let desired = r#"model_provider = "new-route"
model = "new-model"
model_reasoning_effort = "Medium"
model_context_window = 262144
disable_response_storage = true
web_search = "live"

# Common config may be present in the effective Provider, but Target-owned
# fields must not leak across environments.
approval_policy = "never"
sqlite_home = "C:/Users/someone/.codex"
model_instructions_file = "C:/Users/someone/.codex/model-instructions.md"

[model_providers.new-route]
name = "New route"
base_url = "https://new.example/v1"
wire_api = "responses"
experimental_bearer_token = "new-secret"

[mcp_servers.from_windows]
command = "windows-only.exe"
"#;

    let projected = project_codex_provider_config(live, desired).expect("project config");
    let parsed = projected.parse::<toml::Table>().expect("valid TOML");

    assert_eq!(parsed["model_provider"].as_str(), Some("new-route"));
    assert_eq!(parsed["model"].as_str(), Some("new-model"));
    assert_eq!(
        parsed["model_reasoning_effort"].as_str(),
        Some("medium"),
        "Codex effort enums must be projected in their canonical lowercase form"
    );
    assert_eq!(parsed["model_context_window"].as_integer(), Some(262144));
    assert!(
        parsed.get("disable_response_storage").is_none(),
        "response-storage policy belongs to the Target"
    );
    assert_eq!(parsed["web_search"].as_str(), Some("live"));
    assert_eq!(parsed["approval_policy"].as_str(), Some("on-request"));
    assert_eq!(parsed["sandbox_mode"].as_str(), Some("workspace-write"));
    assert_eq!(
        parsed["sqlite_home"].as_str(),
        Some("/home/mikasa/.local/share/codex")
    );
    assert_eq!(parsed["future_target_setting"].as_str(), Some("keep-me"));
    assert_eq!(
        parsed["model_instructions_file"].as_str(),
        Some("/home/mikasa/.codex/model-instructions.md")
    );
    assert_eq!(
        parsed["projects"]["/home/mikasa/work"]["trust_level"].as_str(),
        Some("trusted")
    );
    assert_eq!(
        parsed["mcp_servers"]["local"]["command"].as_str(),
        Some("linux-only-command")
    );
    assert!(parsed["mcp_servers"].get("from_windows").is_none());
    assert_eq!(
        parsed["model_providers"]["new-route"]["base_url"].as_str(),
        Some("https://new.example/v1")
    );
    assert_eq!(
        parsed["model_providers"]["handwritten"]["base_url"].as_str(),
        Some("http://localhost:11434/v1")
    );
    assert!(
        parsed["model_providers"].get("old-route").is_none(),
        "the previously active managed route must be removed"
    );
    assert!(
        parsed["model_providers"].get("legacy-new-alias").is_none(),
        "an inactive alias of the selected route must be collapsed"
    );
    assert_eq!(
        parsed["model_providers"]["new-route"]["legacy_timeout"].as_integer(),
        Some(45),
        "unknown fields from a collapsed alias must be preserved"
    );
}

#[test]
fn provider_projection_removes_stale_managed_fields_absent_from_next_provider() {
    let live = r#"model_provider = "old"
model = "old-model"
model_reasoning_effort = "high"
model_catalog_json = "old-catalog.json"
web_search = "live"
approval_policy = "on-request"

[model_providers.old]
name = "Old"
"#;

    let projected = project_codex_provider_config(live, "").expect("project config");
    let parsed = projected.parse::<toml::Table>().expect("valid TOML");

    for key in [
        "model_provider",
        "model",
        "model_reasoning_effort",
        "model_catalog_json",
        "web_search",
    ] {
        assert!(parsed.get(key).is_none(), "stale {key} must be removed");
    }
    assert_eq!(parsed["approval_policy"].as_str(), Some("on-request"));
    assert!(
        parsed
            .get("model_providers")
            .and_then(toml::Value::as_table)
            .is_none_or(|providers| providers.get("old").is_none()),
        "the stale active managed route must be removed"
    );
}

#[test]
fn provider_projection_replaces_generated_route_with_stable_custom_route() {
    let live = r#"model_provider = "cc_switch_pinai_0860d0fe"

[model_providers.cc_switch_pinai_0860d0fe]
name = "PinAI"
base_url = "https://api.pinaic.com"
wire_api = "responses"
legacy_timeout = 45
"#;
    let desired = r#"model_provider = "custom"

[model_providers.custom]
name = "Another relay"
base_url = "https://relay.example"
wire_api = "responses"
"#;

    let projected = project_codex_provider_config(live, desired).expect("project config");
    let parsed = projected.parse::<toml::Table>().expect("valid TOML");

    assert_eq!(parsed["model_provider"].as_str(), Some("custom"));
    assert!(parsed["model_providers"]
        .get("cc_switch_pinai_0860d0fe")
        .is_none());
    assert_eq!(
        parsed["model_providers"]["custom"]["base_url"].as_str(),
        Some("https://relay.example")
    );
}
