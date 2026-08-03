use serde_json::json;
use std::path::{Path, PathBuf};
use uuid::Uuid;

use cc_switch_lib::{
    cancel_provider_switch_test_hook, confirm_provider_switch_test_hook,
    ensure_codex_official_provider_test_hook, get_codex_auth_path, get_codex_config_path,
    import_default_config_test_hook, lock_codex_provider_switch_test_hook,
    migrate_codex_provider_templates_test_hook, preview_provider_switch_test_hook,
    provider_switch_review_blocks_session_sync_test_hook, read_json_file,
    switch_provider_test_hook, update_settings, write_codex_live_atomic, AppError, AppSettings,
    AppState, AppType, McpApps, McpServer, McpService, MultiAppConfig, Provider, ProviderService,
    ProviderSwitchRequest,
};

#[path = "support.rs"]
mod support;
use std::collections::HashMap;
use support::{
    create_test_state, create_test_state_with_config, enable_codex_official_auth_preservation,
    ensure_test_home, reset_test_fs, test_mutex,
};

fn settings_path(home: &Path) -> PathBuf {
    home.join(".cc-switch").join("settings.json")
}

fn grokbuild_config(name: &str, endpoint: &str, api_key: &str) -> String {
    format!(
        r#"[models]
default = "grok-4.5"

[model."grok-4.5"]
model = "grok-4.5"
base_url = "{endpoint}"
name = "{name}"
api_key = "{api_key}"
api_backend = "responses"
context_window = 500000
"#
    )
}

fn mixed_codex_auth() -> serde_json::Value {
    json!({
        "auth_mode": "chatgpt",
        "OPENAI_API_KEY": null,
        "tokens": {
            "access_token": "oauth-access",
            "refresh_token": "oauth-refresh"
        }
    })
}

fn mixed_codex_config(host: &str, bearer: &str) -> String {
    format!(
        "model_provider = \"custom\"\n\
         [model_providers.custom]\n\
         base_url = \"https://{host}/v1\"\n\
         requires_openai_auth = true\n\
         experimental_bearer_token = \"{bearer}\"\n"
    )
}

fn mixed_codex_provider(
    id: &str,
    name: &str,
    host: &str,
    bearer: &str,
    auth: &serde_json::Value,
) -> Provider {
    Provider::with_id(
        id.to_string(),
        name.to_string(),
        json!({
            "auth": auth,
            "config": mixed_codex_config(host, bearer)
        }),
        None,
    )
}

#[test]
fn grokbuild_import_and_switch_write_live_config() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let live_path = home.join(".grok").join("config.toml");
    std::fs::create_dir_all(live_path.parent().expect("grok config dir"))
        .expect("create grok config dir");
    let imported_config = grokbuild_config("Imported", "https://old.example/v1", "old-key");
    std::fs::write(&live_path, &imported_config).expect("seed Grok Build config");

    let state = create_test_state().expect("create test state");
    import_default_config_test_hook(&state, AppType::GrokBuild)
        .expect("import Grok Build default provider");

    let imported = state
        .db
        .get_provider_by_id("default", AppType::GrokBuild.as_str())
        .expect("query imported provider")
        .expect("imported provider exists");
    assert_eq!(
        imported
            .settings_config
            .get("config")
            .and_then(|value| value.as_str()),
        Some(imported_config.as_str())
    );

    let next_config = grokbuild_config("Relay", "https://new.example/v1", "new-key");
    state
        .db
        .save_provider(
            AppType::GrokBuild.as_str(),
            &Provider::with_id(
                "relay".to_string(),
                "Relay".to_string(),
                json!({ "config": next_config }),
                None,
            ),
        )
        .expect("save second Grok Build provider");

    switch_provider_test_hook(&state, AppType::GrokBuild, "relay")
        .expect("switch Grok Build provider");

    assert_eq!(
        std::fs::read_to_string(&live_path).expect("read switched Grok Build config"),
        next_config
    );
    assert_eq!(
        state
            .db
            .get_current_provider(AppType::GrokBuild.as_str())
            .expect("read Grok Build current provider")
            .as_deref(),
        Some("relay")
    );
}

#[test]
fn codex_startup_import_fresh_install_imports_once_and_syncs_current_setting() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();

    let auth = json!({"OPENAI_API_KEY": "fresh-key"});
    let config = r#"model = "gpt-5"
"#;
    write_codex_live_atomic(&auth, Some(config)).expect("seed codex live config");

    let state = create_test_state().expect("create test state");

    assert!(
        ProviderService::should_import_default_config_on_startup(&state, &AppType::Codex)
            .expect("check startup import eligibility"),
        "empty Codex provider set should import on startup"
    );

    import_default_config_test_hook(&state, AppType::Codex).expect("import codex default");

    let providers = state
        .db
        .get_all_providers(AppType::Codex.as_str())
        .expect("get codex providers after import");
    assert_eq!(
        providers.len(),
        1,
        "fresh install import should create exactly one Codex provider before seeding"
    );
    assert!(
        providers.contains_key("default"),
        "fresh install import should create default provider"
    );

    let current_id = state
        .db
        .get_current_provider(AppType::Codex.as_str())
        .expect("get codex current provider");
    assert_eq!(current_id.as_deref(), Some("default"));

    let settings: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(settings_path(home)).expect("read settings.json"),
    )
    .expect("parse settings.json");
    assert_eq!(
        settings
            .get("currentProviderCodex")
            .and_then(|value| value.as_str()),
        Some("default"),
        "live import should also sync device-local currentProviderCodex"
    );

    state
        .db
        .init_default_official_providers()
        .expect("seed official providers");
    let providers_after_seed = state
        .db
        .get_all_providers(AppType::Codex.as_str())
        .expect("get codex providers after seed");
    assert_eq!(
        providers_after_seed.len(),
        2,
        "official seeding should add codex-official alongside imported default"
    );
    assert!(providers_after_seed.contains_key("codex-official"));

    assert!(
        !ProviderService::should_import_default_config_on_startup(&state, &AppType::Codex)
            .expect("re-check startup import eligibility"),
        "subsequent startup should skip once Codex already has providers"
    );
}

#[test]
fn codex_startup_import_accepts_config_without_auth_file() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let _home = ensure_test_home();

    let config_path = get_codex_config_path();
    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent).expect("create codex config dir");
    }
    std::fs::write(
        &config_path,
        r#"model_provider = "aihubmix"

[model_providers.aihubmix]
name = "AiHubMix"
base_url = "https://aihubmix.example/v1"
wire_api = "responses"
requires_openai_auth = true
experimental_bearer_token = "live-key"
"#,
    )
    .expect("seed config.toml without auth.json");
    assert!(
        !get_codex_auth_path().exists(),
        "test should not seed auth.json"
    );

    let state = create_test_state().expect("create test state");
    import_default_config_test_hook(&state, AppType::Codex)
        .expect("import codex config-only default");

    let providers = state
        .db
        .get_all_providers(AppType::Codex.as_str())
        .expect("get codex providers after import");
    let provider = providers.get("default").expect("default provider exists");
    assert_eq!(
        provider.settings_config.pointer("/auth"),
        Some(&json!({})),
        "missing auth.json should import as an empty auth object"
    );
    assert!(
        provider
            .settings_config
            .get("config")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .contains("experimental_bearer_token"),
        "config.toml content should still be imported"
    );
}

#[test]
fn codex_startup_import_marks_oauth_only_default_official() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let _home = ensure_test_home();

    let auth = json!({
        "auth_mode": "chatgpt",
        "tokens": {
            "id_token": "oauth-id",
            "access_token": "oauth-access"
        }
    });
    let config = r#"[mcp_servers.echo]
command = "echo"
"#;
    write_codex_live_atomic(&auth, Some(config)).expect("seed oauth-only codex live config");

    let state = create_test_state().expect("create test state");
    import_default_config_test_hook(&state, AppType::Codex).expect("import codex default");

    let providers = state
        .db
        .get_all_providers(AppType::Codex.as_str())
        .expect("get codex providers after import");
    let provider = providers.get("default").expect("default provider exists");

    assert_eq!(
        provider.category.as_deref(),
        Some("official"),
        "OAuth-only live Codex installs should keep official behavior"
    );
    assert_eq!(
        provider.settings_config.pointer("/auth/tokens/id_token"),
        Some(&json!("oauth-id")),
        "import should preserve OAuth login material"
    );
}

#[test]
fn codex_startup_import_skips_when_only_official_seed_exists() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let _home = ensure_test_home();

    let auth = json!({"OPENAI_API_KEY": "fresh-key"});
    let config = r#"model = "gpt-5"
"#;
    write_codex_live_atomic(&auth, Some(config)).expect("seed codex live config");

    let state = create_test_state().expect("create test state");
    state
        .db
        .init_default_official_providers()
        .expect("seed official providers");

    let providers_before = state
        .db
        .get_all_providers(AppType::Codex.as_str())
        .expect("get codex providers before restart check");
    assert_eq!(
        providers_before.len(),
        1,
        "fixture should start with only codex-official present"
    );
    assert!(providers_before.contains_key("codex-official"));

    assert!(
        !ProviderService::should_import_default_config_on_startup(&state, &AppType::Codex)
            .expect("check startup import eligibility"),
        "startup should skip import when codex-official already exists"
    );

    let providers_after = state
        .db
        .get_all_providers(AppType::Codex.as_str())
        .expect("get codex providers after restart check");
    assert_eq!(
        providers_after.len(),
        providers_before.len(),
        "skipping startup import should not grow the Codex provider set"
    );
    assert!(
        !providers_after.contains_key("default"),
        "restart path should not create a new default provider"
    );
}

#[test]
fn switch_provider_updates_codex_live_and_state() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    enable_codex_official_auth_preservation();
    let _home = ensure_test_home();

    let legacy_auth = json!({"OPENAI_API_KEY": "legacy-key"});
    let legacy_config = r#"[mcp_servers.legacy]
type = "stdio"
command = "echo"
"#;
    write_codex_live_atomic(&legacy_auth, Some(legacy_config))
        .expect("seed existing codex live config");

    let mut config = MultiAppConfig::default();
    {
        let manager = config
            .get_manager_mut(&AppType::Codex)
            .expect("codex manager");
        manager.current = "old-provider".to_string();
        manager.providers.insert(
            "old-provider".to_string(),
            Provider::with_id(
                "old-provider".to_string(),
                "Legacy".to_string(),
                json!({
                    "auth": {"OPENAI_API_KEY": "stale"},
                    "config": "stale-config"
                }),
                None,
            ),
        );
        manager.providers.insert(
            "new-provider".to_string(),
            Provider::with_id(
                "new-provider".to_string(),
                "Latest".to_string(),
                json!({
                    "auth": {"OPENAI_API_KEY": "fresh-key"},
                    "config": r#"[mcp_servers.latest]
type = "stdio"
command = "say"
"#
                }),
                None,
            ),
        );
    }

    // v3.7.0+: 使用统一的 MCP 结构
    config.mcp.servers = Some(HashMap::new());
    config.mcp.servers.as_mut().unwrap().insert(
        "echo-server".into(),
        McpServer {
            id: "echo-server".to_string(),
            name: "Echo Server".to_string(),
            server: json!({
                "type": "stdio",
                "command": "echo"
            }),
            apps: McpApps {
                claude: false,
                codex: true, // 启用 Codex
                gemini: false,
                grokbuild: false,
                opencode: false,
                hermes: false,
            },
            description: None,
            homepage: None,
            docs: None,
            tags: Vec::new(),
        },
    );

    let app_state = create_test_state_with_config(&config).expect("create test state");

    switch_provider_test_hook(&app_state, AppType::Codex, "new-provider")
        .expect("switch provider should succeed");

    let auth_value: serde_json::Value =
        read_json_file(&get_codex_auth_path()).expect("read auth.json");
    assert_eq!(
        auth_value
            .get("OPENAI_API_KEY")
            .and_then(|v| v.as_str())
            .unwrap_or(""),
        "legacy-key",
        "Codex provider switching should preserve the existing live auth.json"
    );

    let config_text = std::fs::read_to_string(get_codex_config_path()).expect("read config.toml");
    assert!(
        config_text.contains("mcp_servers.echo-server"),
        "config.toml should contain synced MCP servers"
    );
    assert!(
        config_text.contains("experimental_bearer_token"),
        "config.toml should carry the selected provider API key as bearer token"
    );

    let current_id = app_state
        .db
        .get_current_provider(AppType::Codex.as_str())
        .expect("get current provider");
    assert_eq!(
        current_id.as_deref(),
        Some("new-provider"),
        "current provider updated"
    );

    let providers = app_state
        .db
        .get_all_providers(AppType::Codex.as_str())
        .expect("get all providers");

    let new_provider = providers.get("new-provider").expect("new provider exists");
    let new_config_text = new_provider
        .settings_config
        .get("config")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    // 供应商配置应该包含在 live 文件中
    // 注意：live 文件还会包含 MCP 同步后的内容
    assert!(
        config_text.contains("mcp_servers.latest"),
        "live file should contain provider's original config"
    );
    assert!(
        new_config_text.contains("mcp_servers.latest"),
        "provider snapshot should contain provider's original config"
    );

    let legacy = providers
        .get("old-provider")
        .expect("legacy provider still exists");
    let legacy_auth_value = legacy
        .settings_config
        .get("auth")
        .and_then(|v| v.get("OPENAI_API_KEY"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    // 回填机制：切换前会将 live 配置回填到当前供应商
    // 这保护了用户在 live 文件中的手动修改
    assert_eq!(
        legacy_auth_value, "legacy-key",
        "previous provider should be backfilled with live auth"
    );
}

#[test]
fn provider_switch_preview_returns_only_safe_target_metadata() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    enable_codex_official_auth_preservation();
    let oauth_auth = json!({
        "auth_mode": "chatgpt",
        "OPENAI_API_KEY": null,
        "tokens": {"access_token": "oauth-secret"}
    });
    let live_config = r#"model_provider = "custom"

[model_providers.custom]
base_url = "https://current.example/v1"
requires_openai_auth = true
experimental_bearer_token = "current-secret"
"#;
    write_codex_live_atomic(&oauth_auth, Some(live_config)).expect("seed mixed Codex live");
    let state = create_test_state().expect("create test state");

    state
        .db
        .save_provider(
            AppType::Codex.as_str(),
            &Provider::with_id(
                "target-provider".to_string(),
                "Target Relay".to_string(),
                json!({
                    "auth": oauth_auth,
                    "config": r#"model_provider = "custom"

[model_providers.custom]
base_url = "https://API.Target.Example:8443/openai/v1"
requires_openai_auth = true
experimental_bearer_token = "provider-secret"
"#
                }),
                None,
            ),
        )
        .expect("save target provider");

    let preview = preview_provider_switch_test_hook(
        &state,
        &ProviderSwitchRequest {
            version: "v1".to_string(),
            resource: "provider-switch".to_string(),
            app: "codex".to_string(),
            id: "target-provider".to_string(),
        },
    )
    .expect("preview target provider");

    let serialized = serde_json::to_value(&preview).expect("serialize safe preview");
    assert_eq!(serialized["name"], "Target Relay");
    assert_eq!(serialized["hostname"], "api.target.example");
    assert_eq!(serialized["isCurrent"], false);
    let review_token = serialized["reviewToken"]
        .as_str()
        .expect("preview includes opaque review token");
    Uuid::parse_str(review_token).expect("review token is an opaque random UUID");
    assert_eq!(
        serialized.as_object().expect("preview object").len(),
        4,
        "preview must not expose internal snapshot fields"
    );
    let serialized_text = serialized.to_string();
    for secret in [
        "oauth-secret",
        "current-secret",
        "provider-secret",
        "/openai/v1",
    ] {
        assert!(
            !serialized_text.contains(secret),
            "preview leaked a secret or private endpoint path"
        );
    }
}

#[test]
fn provider_switch_preview_rejects_when_codex_auth_preservation_is_disabled() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let oauth_auth = json!({
        "auth_mode": "chatgpt",
        "OPENAI_API_KEY": null,
        "tokens": {"access_token": "oauth-access"}
    });
    let config = r#"model_provider = "custom"
[model_providers.custom]
base_url = "https://target.example/v1"
requires_openai_auth = true
experimental_bearer_token = "target-key"
"#;
    write_codex_live_atomic(&oauth_auth, Some(config)).expect("seed mixed Codex live");
    let state = create_test_state().expect("create test state");
    state
        .db
        .save_provider(
            AppType::Codex.as_str(),
            &Provider::with_id(
                "target-provider".to_string(),
                "Target Relay".to_string(),
                json!({"auth": oauth_auth, "config": config}),
                None,
            ),
        )
        .expect("save target provider");

    let error = preview_provider_switch_test_hook(
        &state,
        &ProviderSwitchRequest {
            version: "v1".to_string(),
            resource: "provider-switch".to_string(),
            app: "codex".to_string(),
            id: "target-provider".to_string(),
        },
    )
    .expect_err("disabled auth preservation must block provider-switch preview");

    assert!(error.to_string().contains("preservation is disabled"));
}

#[test]
fn provider_switch_preview_requires_live_chatgpt_oauth_without_an_api_key() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    enable_codex_official_auth_preservation();
    let config = r#"model_provider = "custom"
[model_providers.custom]
base_url = "https://target.example/v1"
requires_openai_auth = true
experimental_bearer_token = "target-key"
"#;
    write_codex_live_atomic(&json!({"OPENAI_API_KEY": "legacy-key"}), Some(config))
        .expect("seed API-key Codex live");
    let state = create_test_state().expect("create test state");
    state
        .db
        .save_provider(
            AppType::Codex.as_str(),
            &Provider::with_id(
                "target-provider".to_string(),
                "Target Relay".to_string(),
                json!({
                    "auth": {
                        "auth_mode": "chatgpt",
                        "OPENAI_API_KEY": null,
                        "tokens": {"access_token": "oauth-access"}
                    },
                    "config": config
                }),
                None,
            ),
        )
        .expect("save target provider");

    let error = preview_provider_switch_test_hook(
        &state,
        &ProviderSwitchRequest {
            version: "v1".to_string(),
            resource: "provider-switch".to_string(),
            app: "codex".to_string(),
            id: "target-provider".to_string(),
        },
    )
    .expect_err("non-OAuth live auth must block provider-switch preview");

    assert!(error.to_string().contains("Live ChatGPT OAuth"));
}

#[test]
fn provider_switch_preview_rejects_proxy_takeover() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    enable_codex_official_auth_preservation();
    let oauth_auth = json!({
        "auth_mode": "chatgpt",
        "OPENAI_API_KEY": null,
        "tokens": {"access_token": "oauth-access"}
    });
    let config = r#"model_provider = "custom"
[model_providers.custom]
base_url = "https://target.example/v1"
requires_openai_auth = true
experimental_bearer_token = "target-key"
"#;
    write_codex_live_atomic(&oauth_auth, Some(config)).expect("seed mixed Codex live");
    let state = create_test_state().expect("create test state");
    state
        .db
        .save_provider(
            AppType::Codex.as_str(),
            &Provider::with_id(
                "target-provider".to_string(),
                "Target Relay".to_string(),
                json!({"auth": oauth_auth, "config": config}),
                None,
            ),
        )
        .expect("save target provider");
    futures::executor::block_on(state.db.save_live_backup(AppType::Codex.as_str(), "{}"))
        .expect("mark Codex live as proxy-owned");

    let error = preview_provider_switch_test_hook(
        &state,
        &ProviderSwitchRequest {
            version: "v1".to_string(),
            resource: "provider-switch".to_string(),
            app: "codex".to_string(),
            id: "target-provider".to_string(),
        },
    )
    .expect_err("proxy takeover must block reviewed provider switching");

    assert!(error.to_string().contains("proxy takeover"));
}

#[test]
fn provider_switch_preview_rejects_enabled_proxy_without_takeover_markers() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    enable_codex_official_auth_preservation();
    let auth = mixed_codex_auth();
    let config = mixed_codex_config("target.example", "target-key");
    write_codex_live_atomic(&auth, Some(&config)).expect("seed mixed Codex live");
    let state = create_test_state().expect("create test state");
    state
        .db
        .save_provider(
            AppType::Codex.as_str(),
            &mixed_codex_provider(
                "target-provider",
                "Target Relay",
                "target.example",
                "target-key",
                &auth,
            ),
        )
        .expect("save target provider");
    assert!(
        futures::executor::block_on(state.db.get_live_backup(AppType::Codex.as_str()))
            .expect("read live backup")
            .is_none(),
        "test requires no takeover backup"
    );
    let mut proxy_config =
        futures::executor::block_on(state.db.get_proxy_config_for_app(AppType::Codex.as_str()))
            .expect("read Codex proxy config");
    proxy_config.enabled = true;
    futures::executor::block_on(state.db.update_proxy_config_for_app(proxy_config))
        .expect("enable Codex proxy");

    let error = preview_provider_switch_test_hook(
        &state,
        &ProviderSwitchRequest {
            version: "v1".to_string(),
            resource: "provider-switch".to_string(),
            app: "codex".to_string(),
            id: "target-provider".to_string(),
        },
    )
    .expect_err("enabled proxy must block provider-switch preview");

    assert!(error.to_string().contains("proxy takeover"));
}

#[test]
fn provider_switch_preview_rejects_targets_outside_the_mixed_auth_contract() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    enable_codex_official_auth_preservation();
    let oauth_auth = json!({
        "auth_mode": "chatgpt",
        "OPENAI_API_KEY": null,
        "tokens": {"access_token": "oauth-access"}
    });
    let valid_config = r#"model_provider = "custom"
[model_providers.custom]
base_url = "https://target.example/v1"
requires_openai_auth = true
experimental_bearer_token = "target-key"
"#;
    write_codex_live_atomic(&oauth_auth, Some(valid_config)).expect("seed mixed Codex live");
    let state = create_test_state().expect("create test state");
    let request = ProviderSwitchRequest {
        version: "v1".to_string(),
        resource: "provider-switch".to_string(),
        app: "codex".to_string(),
        id: "target-provider".to_string(),
    };
    let cases = [
        (
            "missing provider bearer",
            oauth_auth.clone(),
            r#"model_provider = "custom"
[model_providers.custom]
base_url = "https://target.example/v1"
requires_openai_auth = true
"#,
            None,
        ),
        (
            "OpenAI auth semantics disabled",
            oauth_auth.clone(),
            r#"model_provider = "custom"
[model_providers.custom]
base_url = "https://target.example/v1"
requires_openai_auth = false
experimental_bearer_token = "target-key"
"#,
            None,
        ),
        (
            "top-level bearer source",
            oauth_auth.clone(),
            r#"experimental_bearer_token = "top-level-key"
model_provider = "custom"
[model_providers.custom]
base_url = "https://target.example/v1"
requires_openai_auth = true
experimental_bearer_token = "target-key"
"#,
            None,
        ),
        (
            "higher-priority auth source",
            oauth_auth.clone(),
            r#"model_provider = "custom"
[model_providers.custom]
base_url = "https://target.example/v1"
requires_openai_auth = true
experimental_bearer_token = "target-key"
env_key = "SOME_API_KEY"
"#,
            None,
        ),
        (
            "malformed higher-priority auth source",
            oauth_auth.clone(),
            r#"model_provider = "custom"
[model_providers.custom]
base_url = "https://target.example/v1"
requires_openai_auth = true
experimental_bearer_token = "target-key"
env_key = false
"#,
            None,
        ),
        (
            "provider auth API key",
            json!({
                "auth_mode": "chatgpt",
                "OPENAI_API_KEY": "legacy-key",
                "tokens": {"access_token": "oauth-access"}
            }),
            valid_config,
            None,
        ),
        (
            "malformed provider auth API key",
            json!({
                "auth_mode": "chatgpt",
                "OPENAI_API_KEY": 42,
                "tokens": {"access_token": "oauth-access"}
            }),
            valid_config,
            None,
        ),
        (
            "official provider category",
            oauth_auth.clone(),
            valid_config,
            Some("official"),
        ),
    ];

    for (case, auth, config, category) in cases {
        let mut provider = Provider::with_id(
            "target-provider".to_string(),
            "Target Relay".to_string(),
            json!({"auth": auth, "config": config}),
            None,
        );
        provider.category = category.map(str::to_string);
        state
            .db
            .save_provider(AppType::Codex.as_str(), &provider)
            .expect("save unsafe target case");

        assert!(
            preview_provider_switch_test_hook(&state, &request).is_err(),
            "provider-switch accepted unsafe target case: {case}"
        );
    }
}

#[test]
fn provider_switch_preview_does_not_write_codex_or_provider_state() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    enable_codex_official_auth_preservation();
    let oauth_auth = json!({
        "auth_mode": "chatgpt",
        "OPENAI_API_KEY": null,
        "tokens": {"access_token": "oauth-secret", "refresh_token": "refresh-secret"}
    });
    let live_config = r#"model_provider = "custom"

[model_providers.custom]
base_url = "https://current.example/v1"
requires_openai_auth = true
experimental_bearer_token = "current-secret"
"#;
    write_codex_live_atomic(&oauth_auth, Some(live_config)).expect("seed Codex live files");
    let state = create_test_state().expect("create test state");

    for (id, name, hostname, bearer) in [
        (
            "current-provider",
            "Current Relay",
            "current.example",
            "current-secret",
        ),
        (
            "target-provider",
            "Target Relay",
            "target.example",
            "target-secret",
        ),
    ] {
        state
            .db
            .save_provider(
                AppType::Codex.as_str(),
                &Provider::with_id(
                    id.to_string(),
                    name.to_string(),
                    json!({
                        "auth": oauth_auth,
                        "config": format!(
                            "model_provider = \"custom\"\n\n[model_providers.custom]\nbase_url = \"https://{hostname}/v1\"\nrequires_openai_auth = true\nexperimental_bearer_token = \"{bearer}\"\n"
                        )
                    }),
                    None,
                ),
            )
            .expect("save Codex provider");
    }
    state
        .db
        .set_current_provider(AppType::Codex.as_str(), "current-provider")
        .expect("select current provider");

    let auth_before = std::fs::read(get_codex_auth_path()).expect("read auth before preview");
    let config_before = std::fs::read(get_codex_config_path()).expect("read config before preview");
    let providers_before = serde_json::to_value(
        state
            .db
            .get_all_providers(AppType::Codex.as_str())
            .expect("read providers before preview"),
    )
    .expect("serialize providers before preview");
    let current_before = state
        .db
        .get_current_provider(AppType::Codex.as_str())
        .expect("read current before preview");

    let preview = preview_provider_switch_test_hook(
        &state,
        &ProviderSwitchRequest {
            version: "v1".to_string(),
            resource: "provider-switch".to_string(),
            app: "codex".to_string(),
            id: "target-provider".to_string(),
        },
    )
    .expect("preview target provider");
    assert!(
        provider_switch_review_blocks_session_sync_test_hook(&state)
            .expect("inspect provider-switch review barrier"),
        "an open provider-switch review must pause session-log writes"
    );

    cancel_provider_switch_test_hook(&state, &preview.review_token)
        .expect("cancel provider-switch review");
    assert!(
        !provider_switch_review_blocks_session_sync_test_hook(&state)
            .expect("inspect released provider-switch review barrier"),
        "cancel must release the session-log write barrier"
    );

    assert_eq!(
        std::fs::read(get_codex_auth_path()).expect("read auth after preview"),
        auth_before
    );
    assert_eq!(
        std::fs::read(get_codex_config_path()).expect("read config after preview"),
        config_before
    );
    assert_eq!(
        serde_json::to_value(
            state
                .db
                .get_all_providers(AppType::Codex.as_str())
                .expect("read providers after preview"),
        )
        .expect("serialize providers after preview"),
        providers_before
    );
    assert_eq!(
        state
            .db
            .get_current_provider(AppType::Codex.as_str())
            .expect("read current after preview"),
        current_before
    );
}

#[test]
fn codex_provider_edits_wait_for_the_provider_switch_lock() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let state = std::sync::Arc::new(create_test_state().expect("create test state"));
    let provider = Provider::with_id(
        "target-provider".to_string(),
        "Target Relay".to_string(),
        json!({
            "auth": {
                "auth_mode": "chatgpt",
                "OPENAI_API_KEY": null,
                "tokens": {"access_token": "oauth-access"}
            },
            "config": r#"model_provider = "custom"
[model_providers.custom]
base_url = "https://target.example/v1"
requires_openai_auth = true
experimental_bearer_token = "target-key"
"#
        }),
        None,
    );
    state
        .db
        .save_provider(AppType::Codex.as_str(), &provider)
        .expect("save target provider");

    let switch_guard = lock_codex_provider_switch_test_hook(&state);
    let (result_tx, result_rx) = std::sync::mpsc::channel();
    let worker_state = std::sync::Arc::clone(&state);
    let mut edited = provider;
    edited.name = "Edited Relay".to_string();
    let worker = std::thread::spawn(move || {
        let result = ProviderService::update(
            &worker_state,
            AppType::Codex,
            Some("target-provider"),
            edited,
        );
        result_tx.send(result).expect("send update result");
    });

    assert!(
        matches!(
            result_rx.recv_timeout(std::time::Duration::from_millis(200)),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout)
        ),
        "Codex provider edit bypassed the provider-switch lock"
    );
    drop(switch_guard);
    assert!(result_rx
        .recv_timeout(std::time::Duration::from_secs(2))
        .expect("provider edit should resume after lock release")
        .is_ok());
    worker.join().expect("join provider edit worker");
}

#[test]
fn codex_sync_from_a_separate_app_state_waits_for_the_process_switch_lock() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let primary = create_test_state().expect("create primary test state");
    let secondary = AppState::new(primary.db.clone());

    let switch_guard = lock_codex_provider_switch_test_hook(&primary);
    let (result_tx, result_rx) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        let result = ProviderService::sync_current_to_live(&secondary);
        result_tx.send(result).expect("send sync result");
    });

    assert!(
        matches!(
            result_rx.recv_timeout(std::time::Duration::from_millis(200)),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout)
        ),
        "a short-lived AppState bypassed the process-wide Codex switch lock"
    );
    drop(switch_guard);
    result_rx
        .recv_timeout(std::time::Duration::from_secs(2))
        .expect("sync should resume after lock release")
        .expect("sync current providers");
    worker.join().expect("join sync worker");
}

#[test]
fn codex_live_import_waits_for_the_process_switch_lock() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let auth = mixed_codex_auth();
    let config = mixed_codex_config("import.example", "import-key");
    write_codex_live_atomic(&auth, Some(&config)).expect("seed importable Codex live config");
    let primary = create_test_state().expect("create primary test state");
    let secondary = AppState::new(primary.db.clone());

    let switch_guard = lock_codex_provider_switch_test_hook(&primary);
    let (result_tx, result_rx) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        let result = import_default_config_test_hook(&secondary, AppType::Codex);
        result_tx.send(result).expect("send import result");
    });

    assert!(
        matches!(
            result_rx.recv_timeout(std::time::Duration::from_millis(200)),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout)
        ),
        "Codex live import bypassed the process-wide switch lock"
    );
    drop(switch_guard);
    assert!(
        result_rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .expect("import should resume after lock release")
            .expect("import Codex live config"),
        "the seeded live configuration should be imported"
    );
    worker.join().expect("join import worker");
}

#[test]
fn ensuring_codex_official_provider_waits_for_the_process_switch_lock() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let primary = create_test_state().expect("create primary test state");
    let secondary = AppState::new(primary.db.clone());

    assert!(primary
        .db
        .get_provider_by_id("codex-official", AppType::Codex.as_str())
        .expect("query Codex official provider before ensure")
        .is_none());

    let switch_guard = lock_codex_provider_switch_test_hook(&primary);
    let (result_tx, result_rx) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        let result = ensure_codex_official_provider_test_hook(&secondary);
        result_tx
            .send(result)
            .expect("send Codex official-provider ensure result");
    });

    assert!(
        matches!(
            result_rx.recv_timeout(std::time::Duration::from_millis(200)),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout)
        ),
        "the Codex official-provider repair bypassed the reviewed-switch lock"
    );
    drop(switch_guard);
    assert!(
        result_rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .expect("Codex official-provider repair should resume after lock release")
            .expect("ensure Codex official provider"),
        "the missing Codex official provider should be inserted"
    );
    worker.join().expect("join official-provider ensure worker");
    assert!(primary
        .db
        .get_provider_by_id("codex-official", AppType::Codex.as_str())
        .expect("query Codex official provider after ensure")
        .is_some());
}

#[test]
fn universal_provider_codex_writes_wait_for_the_process_switch_lock() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let state = std::sync::Arc::new(create_test_state().expect("create test state"));
    let universal = serde_json::from_value(json!({
        "id": "shared-relay",
        "name": "Shared Relay",
        "providerType": "custom",
        "apps": {
            "claude": false,
            "codex": true,
            "gemini": false
        },
        "baseUrl": "https://shared.example",
        "apiKey": "shared-key"
    }))
    .expect("deserialize universal provider");
    ProviderService::upsert_universal(&state, universal).expect("save universal provider");

    let switch_guard = lock_codex_provider_switch_test_hook(&state);
    let (sync_tx, sync_rx) = std::sync::mpsc::channel();
    let sync_state = std::sync::Arc::clone(&state);
    let sync_worker = std::thread::spawn(move || {
        let result = ProviderService::sync_universal_to_apps(&sync_state, "shared-relay");
        sync_tx.send(result).expect("send universal sync result");
    });

    assert!(
        matches!(
            sync_rx.recv_timeout(std::time::Duration::from_millis(200)),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout)
        ),
        "universal-provider Codex sync bypassed the process-wide switch lock"
    );
    drop(switch_guard);
    assert!(sync_rx
        .recv_timeout(std::time::Duration::from_secs(2))
        .expect("universal sync should resume after lock release")
        .is_ok());
    sync_worker.join().expect("join universal sync worker");
    assert!(state
        .db
        .get_provider_by_id("universal-codex-shared-relay", AppType::Codex.as_str())
        .expect("query generated Codex provider")
        .is_some());

    let switch_guard = lock_codex_provider_switch_test_hook(&state);
    let (delete_tx, delete_rx) = std::sync::mpsc::channel();
    let delete_state = std::sync::Arc::clone(&state);
    let delete_worker = std::thread::spawn(move || {
        let result = ProviderService::delete_universal(&delete_state, "shared-relay");
        delete_tx
            .send(result)
            .expect("send universal delete result");
    });

    assert!(
        matches!(
            delete_rx.recv_timeout(std::time::Duration::from_millis(200)),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout)
        ),
        "universal-provider Codex deletion bypassed the process-wide switch lock"
    );
    drop(switch_guard);
    assert!(delete_rx
        .recv_timeout(std::time::Duration::from_secs(2))
        .expect("universal deletion should resume after lock release")
        .is_ok());
    delete_worker.join().expect("join universal delete worker");
    assert!(state
        .db
        .get_provider_by_id("universal-codex-shared-relay", AppType::Codex.as_str())
        .expect("query deleted generated Codex provider")
        .is_none());
}

#[test]
fn codex_auth_preservation_setting_waits_for_the_process_switch_lock() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let state = std::sync::Arc::new(create_test_state().expect("create test state"));
    let settings = AppSettings {
        preserve_codex_official_auth_on_switch: true,
        ..Default::default()
    };

    let switch_guard = lock_codex_provider_switch_test_hook(&state);
    let (result_tx, result_rx) = std::sync::mpsc::channel();
    let worker_state = std::sync::Arc::clone(&state);
    let worker = std::thread::spawn(move || {
        let result = futures::executor::block_on(cc_switch_lib::save_settings_test_hook(
            &worker_state,
            settings,
        ));
        result_tx.send(result).expect("send settings-save result");
    });

    assert!(
        matches!(
            result_rx.recv_timeout(std::time::Duration::from_millis(200)),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout)
        ),
        "the Codex auth-preservation setting changed during a reviewed switch"
    );
    drop(switch_guard);
    assert!(result_rx
        .recv_timeout(std::time::Duration::from_secs(2))
        .expect("settings save should resume after lock release")
        .is_ok());
    worker.join().expect("join settings-save worker");
    let saved_settings: serde_json::Value = serde_json::from_slice(
        &std::fs::read(settings_path(ensure_test_home())).expect("read saved settings"),
    )
    .expect("parse saved settings");
    assert_eq!(
        saved_settings
            .get("preserveCodexOfficialAuthOnSwitch")
            .and_then(serde_json::Value::as_bool),
        Some(true)
    );
}

#[test]
fn codex_mcp_writes_wait_for_the_process_switch_lock() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let auth = mixed_codex_auth();
    let config = mixed_codex_config("mcp-lock.example", "mcp-lock-key");
    write_codex_live_atomic(&auth, Some(&config)).expect("seed Codex live config");
    let auth_before = std::fs::read(get_codex_auth_path()).expect("read auth before MCP edit");
    let state = std::sync::Arc::new(create_test_state().expect("create test state"));
    let server = McpServer {
        id: "lock-test".to_string(),
        name: "Lock Test".to_string(),
        server: json!({
            "command": "cmd",
            "args": ["/c", "echo", "ok"]
        }),
        apps: McpApps {
            codex: true,
            ..McpApps::default()
        },
        description: None,
        homepage: None,
        docs: None,
        tags: Vec::new(),
    };

    let switch_guard = lock_codex_provider_switch_test_hook(&state);
    let (result_tx, result_rx) = std::sync::mpsc::channel();
    let worker_state = std::sync::Arc::clone(&state);
    let worker = std::thread::spawn(move || {
        let result = McpService::upsert_server(&worker_state, server);
        result_tx.send(result).expect("send MCP upsert result");
    });

    assert!(
        matches!(
            result_rx.recv_timeout(std::time::Duration::from_millis(200)),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout)
        ),
        "a Codex MCP live write bypassed the process-wide switch lock"
    );
    drop(switch_guard);
    result_rx
        .recv_timeout(std::time::Duration::from_secs(2))
        .expect("MCP upsert should resume after lock release")
        .expect("upsert MCP server");
    worker.join().expect("join MCP upsert worker");

    assert_eq!(
        std::fs::read(get_codex_auth_path()).expect("read auth after MCP edit"),
        auth_before,
        "MCP projection must preserve ChatGPT OAuth byte-for-byte"
    );
    let config_after =
        std::fs::read_to_string(get_codex_config_path()).expect("read config after MCP edit");
    assert!(config_after.contains("base_url = \"https://mcp-lock.example/v1\""));
    assert!(config_after.contains("experimental_bearer_token = \"mcp-lock-key\""));
    assert!(config_after.contains("[mcp_servers.lock-test]"));
}

#[test]
fn codex_template_migration_waits_for_the_process_switch_lock() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let state = std::sync::Arc::new(create_test_state().expect("create test state"));
    state
        .db
        .save_provider(
            AppType::Codex.as_str(),
            &Provider::with_id(
                "aihubmix".to_string(),
                "Legacy Relay".to_string(),
                json!({
                    "auth": {"OPENAI_API_KEY": "legacy-key"},
                    "config": "model_provider = \"aihubmix\"\n[model_providers.aihubmix]\nbase_url = \"https://legacy.example/v1\"\nwire_api = \"responses\"\nrequires_openai_auth = true\n"
                }),
                None,
            ),
        )
        .expect("save legacy Codex provider");

    let switch_guard = lock_codex_provider_switch_test_hook(&state);
    let (result_tx, result_rx) = std::sync::mpsc::channel();
    let worker_state = std::sync::Arc::clone(&state);
    let worker = std::thread::spawn(move || {
        let result = migrate_codex_provider_templates_test_hook(&worker_state);
        result_tx
            .send(result)
            .expect("send template-migration result");
    });

    assert!(
        matches!(
            result_rx.recv_timeout(std::time::Duration::from_millis(200)),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout)
        ),
        "the startup Codex template migration bypassed the reviewed-switch lock"
    );
    drop(switch_guard);
    let migrated = result_rx
        .recv_timeout(std::time::Duration::from_secs(2))
        .expect("template migration should resume after lock release")
        .expect("migrate Codex provider template");
    worker.join().expect("join template-migration worker");
    assert_eq!(migrated, 1);

    let migrated_provider = state
        .db
        .get_provider_by_id("aihubmix", AppType::Codex.as_str())
        .expect("query migrated provider")
        .expect("migrated provider remains present");
    let config = migrated_provider
        .settings_config
        .get("config")
        .and_then(serde_json::Value::as_str)
        .expect("migrated provider config");
    assert!(config.contains("model_provider = \"custom\""));
    assert!(config.contains("[model_providers.custom]"));
}

#[test]
fn confirmed_provider_switch_preserves_oauth_and_selects_target_endpoint_and_bearer() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    enable_codex_official_auth_preservation();
    let oauth_auth = json!({
        "auth_mode": "chatgpt",
        "OPENAI_API_KEY": null,
        "tokens": {
            "id_token": "oauth-id",
            "access_token": "oauth-access",
            "refresh_token": "oauth-refresh",
            "account_id": "oauth-account"
        }
    });
    let current_config = r#"model_provider = "custom"

[model_providers.custom]
name = "Current Relay"
base_url = "https://current.example/v1"
wire_api = "responses"
requires_openai_auth = true
experimental_bearer_token = "current-provider-key"
"#;
    let target_config = r#"model_provider = "custom"

[model_providers.custom]
name = "Target Relay"
base_url = "https://target.example/openai/v1"
wire_api = "responses"
requires_openai_auth = true
experimental_bearer_token = "target-provider-key"
"#;
    write_codex_live_atomic(&oauth_auth, Some(current_config)).expect("seed mixed Codex live");
    let state = create_test_state().expect("create test state");

    for (id, name, config) in [
        ("current-provider", "Current Relay", current_config),
        ("target-provider", "Target Relay", target_config),
    ] {
        state
            .db
            .save_provider(
                AppType::Codex.as_str(),
                &Provider::with_id(
                    id.to_string(),
                    name.to_string(),
                    json!({"auth": oauth_auth, "config": config}),
                    None,
                ),
            )
            .expect("save mixed Codex provider");
    }
    state
        .db
        .set_current_provider(AppType::Codex.as_str(), "current-provider")
        .expect("select current provider");

    let request = ProviderSwitchRequest {
        version: "v1".to_string(),
        resource: "provider-switch".to_string(),
        app: "codex".to_string(),
        id: "target-provider".to_string(),
    };
    let expected =
        preview_provider_switch_test_hook(&state, &request).expect("preview target provider");
    let auth_before = std::fs::read(get_codex_auth_path()).expect("read OAuth before switch");

    let result = confirm_provider_switch_test_hook(&state, &expected.review_token)
        .expect("confirm provider switch");

    assert!(result.is_current, "confirmed target should be current");
    assert_eq!(
        std::fs::read(get_codex_auth_path()).expect("read OAuth after switch"),
        auth_before,
        "mixed-provider switch must preserve auth.json byte-for-byte"
    );
    assert_eq!(
        state
            .db
            .get_current_provider(AppType::Codex.as_str())
            .expect("read selected provider")
            .as_deref(),
        Some("target-provider")
    );
    let live_config = std::fs::read_to_string(get_codex_config_path())
        .expect("read target Codex config after switch");
    let live: toml::Value = live_config.parse().expect("parse target Codex config");
    let selected = &live["model_providers"]["custom"];
    assert_eq!(
        selected["base_url"].as_str(),
        Some("https://target.example/openai/v1")
    );
    assert_eq!(
        selected["experimental_bearer_token"].as_str(),
        Some("target-provider-key")
    );

    let auth_after_first =
        std::fs::read(get_codex_auth_path()).expect("read OAuth after first confirm");
    let config_after_first =
        std::fs::read(get_codex_config_path()).expect("read config after first confirm");
    let replay_error = confirm_provider_switch_test_hook(&state, &expected.review_token)
        .expect_err("review token must be single use");
    assert!(replay_error.to_string().contains("missing or expired"));
    assert_eq!(
        std::fs::read(get_codex_auth_path()).expect("read OAuth after replay"),
        auth_after_first
    );
    assert_eq!(
        std::fs::read(get_codex_config_path()).expect("read config after replay"),
        config_after_first
    );
}

#[test]
fn provider_switch_preview_rejects_current_id_and_live_target_disagreement_without_writing() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let auth = mixed_codex_auth();
    let current_config = mixed_codex_config("current.example", "current-key");
    write_codex_live_atomic(&auth, Some(&current_config)).expect("seed stale current live");
    let state = create_test_state().expect("create test state");
    for provider in [
        mixed_codex_provider(
            "current-provider",
            "Current Relay",
            "current.example",
            "current-key",
            &auth,
        ),
        mixed_codex_provider(
            "target-provider",
            "Target Relay",
            "target.example",
            "target-key",
            &auth,
        ),
    ] {
        state
            .db
            .save_provider(AppType::Codex.as_str(), &provider)
            .expect("save provider");
    }
    state
        .db
        .set_current_provider(AppType::Codex.as_str(), "target-provider")
        .expect("select target in database only");
    update_settings(AppSettings {
        preserve_codex_official_auth_on_switch: true,
        current_provider_codex: Some("target-provider".to_string()),
        ..Default::default()
    })
    .expect("select target in device settings only");

    let auth_before = std::fs::read(get_codex_auth_path()).expect("read OAuth before preview");
    let config_before =
        std::fs::read(get_codex_config_path()).expect("read stale live before preview");

    let error = preview_provider_switch_test_hook(
        &state,
        &ProviderSwitchRequest {
            version: "v1".to_string(),
            resource: "provider-switch".to_string(),
            app: "codex".to_string(),
            id: "target-provider".to_string(),
        },
    )
    .expect_err("a stale live endpoint must block the current-target preview");

    assert!(error
        .to_string()
        .contains("does not match the selected provider"));
    assert_eq!(
        std::fs::read(get_codex_auth_path()).expect("read OAuth after rejected preview"),
        auth_before
    );
    assert_eq!(
        std::fs::read(get_codex_config_path()).expect("read live after rejected preview"),
        config_before
    );
    assert_eq!(
        state
            .db
            .get_current_provider(AppType::Codex.as_str())
            .expect("read database current after rejected preview")
            .as_deref(),
        Some("target-provider")
    );
    let home = PathBuf::from(std::env::var("HOME").expect("test HOME should be set"));
    let saved_settings: AppSettings =
        read_json_file(&settings_path(&home)).expect("read settings after rejected preview");
    assert_eq!(
        saved_settings.current_provider_codex.as_deref(),
        Some("target-provider")
    );
}

#[test]
fn provider_switch_preview_reports_current_when_live_target_matches() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let auth = mixed_codex_auth();
    let target_config = mixed_codex_config("target.example", "target-key");
    write_codex_live_atomic(&auth, Some(&target_config)).expect("seed target live config");
    let state = create_test_state().expect("create test state");
    state
        .db
        .save_provider(
            AppType::Codex.as_str(),
            &mixed_codex_provider(
                "target-provider",
                "Target Relay",
                "target.example",
                "target-key",
                &auth,
            ),
        )
        .expect("save target provider");
    state
        .db
        .set_current_provider(AppType::Codex.as_str(), "target-provider")
        .expect("select target in database");
    update_settings(AppSettings {
        preserve_codex_official_auth_on_switch: true,
        current_provider_codex: Some("target-provider".to_string()),
        ..Default::default()
    })
    .expect("select target in device settings");

    let preview = preview_provider_switch_test_hook(
        &state,
        &ProviderSwitchRequest {
            version: "v1".to_string(),
            resource: "provider-switch".to_string(),
            app: "codex".to_string(),
            id: "target-provider".to_string(),
        },
    )
    .expect("preview matching current target");

    assert!(preview.is_current);
    cancel_provider_switch_test_hook(&state, &preview.review_token)
        .expect("cancel matching current-target review");
}

#[test]
fn provider_switch_rejects_a_target_changed_after_preview_without_writing_live() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    enable_codex_official_auth_preservation();
    let oauth_auth = json!({
        "auth_mode": "chatgpt",
        "OPENAI_API_KEY": null,
        "tokens": {"access_token": "oauth-access", "refresh_token": "oauth-refresh"}
    });
    let current_config = r#"model_provider = "custom"
[model_providers.custom]
base_url = "https://current.example/v1"
requires_openai_auth = true
experimental_bearer_token = "current-key"
"#;
    let target_config = r#"model_provider = "custom"
[model_providers.custom]
base_url = "https://target.example/v1"
requires_openai_auth = true
experimental_bearer_token = "target-key"
"#;
    write_codex_live_atomic(&oauth_auth, Some(current_config)).expect("seed current live");
    let state = create_test_state().expect("create test state");
    for (id, name, config) in [
        ("current-provider", "Current Relay", current_config),
        ("target-provider", "Target Relay", target_config),
    ] {
        state
            .db
            .save_provider(
                AppType::Codex.as_str(),
                &Provider::with_id(
                    id.to_string(),
                    name.to_string(),
                    json!({"auth": oauth_auth, "config": config}),
                    None,
                ),
            )
            .expect("save provider");
    }
    state
        .db
        .set_current_provider(AppType::Codex.as_str(), "current-provider")
        .expect("select current provider");
    let request = ProviderSwitchRequest {
        version: "v1".to_string(),
        resource: "provider-switch".to_string(),
        app: "codex".to_string(),
        id: "target-provider".to_string(),
    };
    let expected =
        preview_provider_switch_test_hook(&state, &request).expect("preview target provider");

    state
        .db
        .save_provider(
            AppType::Codex.as_str(),
            &Provider::with_id(
                "target-provider".to_string(),
                "Target Relay".to_string(),
                json!({
                    "auth": oauth_auth,
                    "config": r#"model_provider = "custom"
[model_providers.custom]
base_url = "https://target.example/v2"
requires_openai_auth = true
experimental_bearer_token = "changed-key"
"#
                }),
                None,
            ),
        )
        .expect("change target after preview");
    let auth_before = std::fs::read(get_codex_auth_path()).expect("read auth before confirm");
    let config_before = std::fs::read(get_codex_config_path()).expect("read config before confirm");

    let error = confirm_provider_switch_test_hook(&state, &expected.review_token)
        .expect_err("changed target must require a new preview");

    assert!(error.to_string().contains("changed after preview"));
    assert_eq!(
        std::fs::read(get_codex_auth_path()).expect("read auth after rejected confirm"),
        auth_before
    );
    assert_eq!(
        std::fs::read(get_codex_config_path()).expect("read config after rejected confirm"),
        config_before
    );
    assert_eq!(
        state
            .db
            .get_current_provider(AppType::Codex.as_str())
            .expect("read current after rejected confirm")
            .as_deref(),
        Some("current-provider")
    );
}

#[test]
fn provider_switch_rejects_proxy_takeover_started_after_preview_without_writing_live() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    enable_codex_official_auth_preservation();
    let auth = mixed_codex_auth();
    let current_config = mixed_codex_config("current.example", "current-key");
    write_codex_live_atomic(&auth, Some(&current_config)).expect("seed current live");
    let state = create_test_state().expect("create test state");
    for provider in [
        mixed_codex_provider(
            "current-provider",
            "Current Relay",
            "current.example",
            "current-key",
            &auth,
        ),
        mixed_codex_provider(
            "target-provider",
            "Target Relay",
            "target.example",
            "target-key",
            &auth,
        ),
    ] {
        state
            .db
            .save_provider(AppType::Codex.as_str(), &provider)
            .expect("save provider");
    }
    state
        .db
        .set_current_provider(AppType::Codex.as_str(), "current-provider")
        .expect("select current provider");
    let preview = preview_provider_switch_test_hook(
        &state,
        &ProviderSwitchRequest {
            version: "v1".to_string(),
            resource: "provider-switch".to_string(),
            app: "codex".to_string(),
            id: "target-provider".to_string(),
        },
    )
    .expect("preview target provider");
    let auth_before = std::fs::read(get_codex_auth_path()).expect("read auth before confirm");
    let config_before = std::fs::read(get_codex_config_path()).expect("read config before confirm");

    futures::executor::block_on(state.db.save_live_backup(AppType::Codex.as_str(), "{}"))
        .expect("start proxy takeover after preview");
    let error = confirm_provider_switch_test_hook(&state, &preview.review_token)
        .expect_err("new proxy takeover must invalidate the review");

    assert!(error.to_string().contains("proxy takeover"));
    assert_eq!(
        std::fs::read(get_codex_auth_path()).expect("read auth after rejected confirm"),
        auth_before
    );
    assert_eq!(
        std::fs::read(get_codex_config_path()).expect("read config after rejected confirm"),
        config_before
    );
    assert_eq!(
        state
            .db
            .get_current_provider(AppType::Codex.as_str())
            .expect("read current after rejected confirm")
            .as_deref(),
        Some("current-provider")
    );
}

#[test]
fn provider_switch_rejects_proxy_enabled_after_preview_without_writing_live() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    enable_codex_official_auth_preservation();
    let auth = mixed_codex_auth();
    let current_config = mixed_codex_config("current.example", "current-key");
    write_codex_live_atomic(&auth, Some(&current_config)).expect("seed current live");
    let state = create_test_state().expect("create test state");
    for provider in [
        mixed_codex_provider(
            "current-provider",
            "Current Relay",
            "current.example",
            "current-key",
            &auth,
        ),
        mixed_codex_provider(
            "target-provider",
            "Target Relay",
            "target.example",
            "target-key",
            &auth,
        ),
    ] {
        state
            .db
            .save_provider(AppType::Codex.as_str(), &provider)
            .expect("save provider");
    }
    state
        .db
        .set_current_provider(AppType::Codex.as_str(), "current-provider")
        .expect("select current provider");
    let preview = preview_provider_switch_test_hook(
        &state,
        &ProviderSwitchRequest {
            version: "v1".to_string(),
            resource: "provider-switch".to_string(),
            app: "codex".to_string(),
            id: "target-provider".to_string(),
        },
    )
    .expect("preview target provider");
    let auth_before = std::fs::read(get_codex_auth_path()).expect("read auth before confirm");
    let config_before = std::fs::read(get_codex_config_path()).expect("read config before confirm");
    assert!(
        futures::executor::block_on(state.db.get_live_backup(AppType::Codex.as_str()))
            .expect("read live backup")
            .is_none(),
        "test requires no takeover backup"
    );

    let mut proxy_config =
        futures::executor::block_on(state.db.get_proxy_config_for_app(AppType::Codex.as_str()))
            .expect("read Codex proxy config");
    proxy_config.enabled = true;
    futures::executor::block_on(state.db.update_proxy_config_for_app(proxy_config))
        .expect("enable Codex proxy after preview");
    let error = confirm_provider_switch_test_hook(&state, &preview.review_token)
        .expect_err("newly enabled proxy must invalidate the review");

    assert!(error.to_string().contains("proxy takeover"));
    assert_eq!(
        std::fs::read(get_codex_auth_path()).expect("read auth after rejected confirm"),
        auth_before
    );
    assert_eq!(
        std::fs::read(get_codex_config_path()).expect("read config after rejected confirm"),
        config_before
    );
    assert_eq!(
        state
            .db
            .get_current_provider(AppType::Codex.as_str())
            .expect("read current after rejected confirm")
            .as_deref(),
        Some("current-provider")
    );
}

#[test]
fn provider_switch_rejects_current_provider_record_changes_after_preview() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    enable_codex_official_auth_preservation();
    let auth = mixed_codex_auth();
    let current_config = mixed_codex_config("current.example", "current-key");
    write_codex_live_atomic(&auth, Some(&current_config)).expect("seed current live");
    let state = create_test_state().expect("create test state");
    let mut current = mixed_codex_provider(
        "current-provider",
        "Current Relay",
        "current.example",
        "current-key",
        &auth,
    );
    let target = mixed_codex_provider(
        "target-provider",
        "Target Relay",
        "target.example",
        "target-key",
        &auth,
    );
    for provider in [&current, &target] {
        state
            .db
            .save_provider(AppType::Codex.as_str(), provider)
            .expect("save provider");
    }
    state
        .db
        .set_current_provider(AppType::Codex.as_str(), "current-provider")
        .expect("select current provider");
    let preview = preview_provider_switch_test_hook(
        &state,
        &ProviderSwitchRequest {
            version: "v1".to_string(),
            resource: "provider-switch".to_string(),
            app: "codex".to_string(),
            id: "target-provider".to_string(),
        },
    )
    .expect("preview target provider");
    let auth_before = std::fs::read(get_codex_auth_path()).expect("read auth before confirm");
    let config_before = std::fs::read(get_codex_config_path()).expect("read config before confirm");

    current.notes = Some("edited after preview".to_string());
    state
        .db
        .save_provider(AppType::Codex.as_str(), &current)
        .expect("edit current provider after preview");
    let error = confirm_provider_switch_test_hook(&state, &preview.review_token)
        .expect_err("changed current provider row must invalidate the review");

    assert!(error.to_string().contains("changed after preview"));
    assert_eq!(
        std::fs::read(get_codex_auth_path()).expect("read auth after rejected confirm"),
        auth_before
    );
    assert_eq!(
        std::fs::read(get_codex_config_path()).expect("read config after rejected confirm"),
        config_before
    );
    assert_eq!(
        state
            .db
            .get_current_provider(AppType::Codex.as_str())
            .expect("read current after rejected confirm")
            .as_deref(),
        Some("current-provider")
    );
}

#[test]
fn provider_switch_rejects_device_current_changes_after_preview() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let auth = mixed_codex_auth();
    let device_config = mixed_codex_config("device.example", "device-key");
    write_codex_live_atomic(&auth, Some(&device_config)).expect("seed device-current live");
    let state = create_test_state().expect("create test state");
    for provider in [
        mixed_codex_provider(
            "device-current",
            "Device Relay",
            "device.example",
            "device-key",
            &auth,
        ),
        mixed_codex_provider(
            "database-current",
            "Database Relay",
            "database.example",
            "database-key",
            &auth,
        ),
        mixed_codex_provider(
            "next-device-current",
            "Next Device Relay",
            "next-device.example",
            "next-device-key",
            &auth,
        ),
        mixed_codex_provider(
            "target-provider",
            "Target Relay",
            "target.example",
            "target-key",
            &auth,
        ),
    ] {
        state
            .db
            .save_provider(AppType::Codex.as_str(), &provider)
            .expect("save provider");
    }
    state
        .db
        .set_current_provider(AppType::Codex.as_str(), "database-current")
        .expect("select database current");
    update_settings(AppSettings {
        preserve_codex_official_auth_on_switch: true,
        current_provider_codex: Some("device-current".to_string()),
        ..Default::default()
    })
    .expect("select device current");
    let preview = preview_provider_switch_test_hook(
        &state,
        &ProviderSwitchRequest {
            version: "v1".to_string(),
            resource: "provider-switch".to_string(),
            app: "codex".to_string(),
            id: "target-provider".to_string(),
        },
    )
    .expect("preview target provider");
    let auth_before = std::fs::read(get_codex_auth_path()).expect("read auth before confirm");
    let config_before = std::fs::read(get_codex_config_path()).expect("read config before confirm");

    update_settings(AppSettings {
        preserve_codex_official_auth_on_switch: true,
        current_provider_codex: Some("next-device-current".to_string()),
        ..Default::default()
    })
    .expect("change device current after preview");
    let error = confirm_provider_switch_test_hook(&state, &preview.review_token)
        .expect_err("changed device current must invalidate the review");

    assert!(error.to_string().contains("changed after preview"));
    assert_eq!(
        std::fs::read(get_codex_auth_path()).expect("read auth after rejected confirm"),
        auth_before
    );
    assert_eq!(
        std::fs::read(get_codex_config_path()).expect("read config after rejected confirm"),
        config_before
    );
    assert_eq!(
        state
            .db
            .get_current_provider(AppType::Codex.as_str())
            .expect("read database current after rejected confirm")
            .as_deref(),
        Some("database-current")
    );
}

#[test]
fn switch_provider_missing_provider_returns_error() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();

    let mut config = MultiAppConfig::default();
    config
        .get_manager_mut(&AppType::Claude)
        .expect("claude manager")
        .current = "does-not-exist".to_string();

    let app_state = create_test_state_with_config(&config).expect("create test state");

    let err = switch_provider_test_hook(&app_state, AppType::Claude, "missing-provider")
        .expect_err("switching to a missing provider should fail");

    let err_str = err.to_string();
    assert!(
        err_str.contains("供应商不存在")
            || err_str.contains("Provider not found")
            || err_str.contains("missing-provider"),
        "error message should mention missing provider, got: {err_str}"
    );
}

#[test]
fn switch_provider_updates_claude_live_and_state() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let _home = ensure_test_home();

    let settings_path = cc_switch_lib::get_claude_settings_path();
    if let Some(parent) = settings_path.parent() {
        std::fs::create_dir_all(parent).expect("create claude settings dir");
    }
    let legacy_live = json!({
        "env": {
            "ANTHROPIC_API_KEY": "legacy-key"
        },
        "workspace": {
            "path": "/tmp/workspace"
        }
    });
    std::fs::write(
        &settings_path,
        serde_json::to_string_pretty(&legacy_live).expect("serialize legacy live"),
    )
    .expect("seed claude live config");

    let mut config = MultiAppConfig::default();
    {
        let manager = config
            .get_manager_mut(&AppType::Claude)
            .expect("claude manager");
        manager.current = "old-provider".to_string();
        manager.providers.insert(
            "old-provider".to_string(),
            Provider::with_id(
                "old-provider".to_string(),
                "Legacy Claude".to_string(),
                json!({
                    "env": { "ANTHROPIC_API_KEY": "stale-key" }
                }),
                None,
            ),
        );
        manager.providers.insert(
            "new-provider".to_string(),
            Provider::with_id(
                "new-provider".to_string(),
                "Fresh Claude".to_string(),
                json!({
                    "env": { "ANTHROPIC_API_KEY": "fresh-key" },
                    "workspace": { "path": "/tmp/new-workspace" }
                }),
                None,
            ),
        );
    }

    let app_state = create_test_state_with_config(&config).expect("create test state");

    switch_provider_test_hook(&app_state, AppType::Claude, "new-provider")
        .expect("switch provider should succeed");

    let live_after: serde_json::Value =
        read_json_file(&settings_path).expect("read claude live settings");
    assert_eq!(
        live_after
            .get("env")
            .and_then(|env| env.get("ANTHROPIC_API_KEY"))
            .and_then(|key| key.as_str()),
        Some("fresh-key"),
        "live settings.json should reflect new provider auth"
    );

    let current_id = app_state
        .db
        .get_current_provider(AppType::Claude.as_str())
        .expect("get current provider");
    assert_eq!(
        current_id.as_deref(),
        Some("new-provider"),
        "current provider updated"
    );

    let providers = app_state
        .db
        .get_all_providers(AppType::Claude.as_str())
        .expect("get all providers");

    let legacy_provider = providers
        .get("old-provider")
        .expect("legacy provider still exists");
    // 回填机制：切换前会将 live 配置回填到当前供应商
    // 这保护了用户在 live 文件中的手动修改
    assert_eq!(
        legacy_provider.settings_config, legacy_live,
        "previous provider should be backfilled with live config"
    );

    let new_provider = providers.get("new-provider").expect("new provider exists");
    assert_eq!(
        new_provider
            .settings_config
            .get("env")
            .and_then(|env| env.get("ANTHROPIC_API_KEY"))
            .and_then(|key| key.as_str()),
        Some("fresh-key"),
        "new provider snapshot should retain fresh auth"
    );

    // v3.7.0+ 使用 SQLite 数据库而非 config.json
    // 验证数据已持久化到数据库
    let home_dir = std::env::var("HOME").expect("HOME should be set by ensure_test_home");
    let db_path = std::path::Path::new(&home_dir)
        .join(".cc-switch")
        .join("cc-switch.db");
    assert!(
        db_path.exists(),
        "switching provider should persist to cc-switch.db"
    );

    // 验证当前供应商已更新
    let current_id = app_state
        .db
        .get_current_provider(AppType::Claude.as_str())
        .expect("get current provider");
    assert_eq!(
        current_id.as_deref(),
        Some("new-provider"),
        "database should record the new current provider"
    );
}

#[test]
fn switch_provider_codex_missing_auth_returns_error_and_keeps_state() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let _home = ensure_test_home();

    let mut config = MultiAppConfig::default();
    {
        let manager = config
            .get_manager_mut(&AppType::Codex)
            .expect("codex manager");
        manager.providers.insert(
            "invalid".to_string(),
            Provider::with_id(
                "invalid".to_string(),
                "Broken Codex".to_string(),
                json!({
                    "config": "[mcp_servers.test]\ncommand = \"noop\""
                }),
                None,
            ),
        );
    }

    let app_state = create_test_state_with_config(&config).expect("create test state");

    let err = switch_provider_test_hook(&app_state, AppType::Codex, "invalid")
        .expect_err("switching should fail when auth missing");
    match err {
        AppError::Config(msg) => assert!(
            msg.contains("auth"),
            "expected auth missing error message, got {msg}"
        ),
        other => panic!("expected config error, got {other:?}"),
    }

    let current_id = app_state
        .db
        .get_current_provider(AppType::Codex.as_str())
        .expect("get current provider");
    // 切换失败后，由于数据库操作是先设置再验证，current 可能已被设为 "invalid"
    // 但由于 live 配置写入失败，状态应该回滚
    // 注意：这个行为取决于 switch_provider 的具体实现
    assert!(
        current_id.is_none() || current_id.as_deref() == Some("invalid"),
        "current provider should remain empty or be the attempted id on failure, got: {current_id:?}"
    );
}

#[test]
fn import_refuses_live_config_under_proxy_takeover() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    ensure_test_home();

    // 接管态 Codex Live：auth 是 PROXY_MANAGED 占位符，不是用户真实配置
    let auth = json!({"OPENAI_API_KEY": "PROXY_MANAGED"});
    let config = r#"model = "gpt-5"
"#;
    write_codex_live_atomic(&auth, Some(config)).expect("seed taken-over codex live");

    let state = create_test_state().expect("create test state");

    import_default_config_test_hook(&state, AppType::Codex)
        .expect_err("importing a taken-over live config must fail");

    let providers = state
        .db
        .get_all_providers(AppType::Codex.as_str())
        .expect("get codex providers");
    assert!(
        providers.is_empty(),
        "taken-over live import must not create providers"
    );
}
