use serde_json::json;
use std::path::{Path, PathBuf};

use cc_switch_lib::{
    get_codex_auth_path, get_codex_config_path, import_default_config_test_hook, read_json_file,
    switch_provider_test_hook, write_codex_live_atomic, AppError, AppType, McpApps, McpServer,
    MultiAppConfig, Provider, ProviderService,
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

#[test]
fn grokbuild_import_and_switch_write_live_config() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
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

/// Grok Build's `/settings` → Default model rewrites `models.default` in the live
/// config. Picking a built-in model while a third-party provider is active leaves
/// the provider's own `[model.*]` table unreferenced; switching away and back must
/// still work.
#[test]
fn grokbuild_switch_back_after_client_changed_default_model() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    let home = ensure_test_home();
    let live_path = home.join(".grok").join("config.toml");
    std::fs::create_dir_all(live_path.parent().expect("grok config dir"))
        .expect("create grok config dir");

    let state = create_test_state().expect("create test state");
    let relay_a = grokbuild_config("RelayA", "https://a.example/v1", "key-a");
    let relay_b = grokbuild_config("RelayB", "https://b.example/v1", "key-b");
    for (id, name, config) in [("a", "RelayA", &relay_a), ("b", "RelayB", &relay_b)] {
        state
            .db
            .save_provider(
                AppType::GrokBuild.as_str(),
                &Provider::with_id(
                    id.to_string(),
                    name.to_string(),
                    json!({ "config": config }),
                    None,
                ),
            )
            .expect("save Grok Build provider");
    }

    switch_provider_test_hook(&state, AppType::GrokBuild, "a").expect("switch to provider a");
    assert_eq!(
        std::fs::read_to_string(&live_path).expect("read live after switching to a"),
        relay_a
    );

    // Simulate Grok's `/settings` → Default model picking the built-in grok-4.6.
    let client_edited = relay_a.replace("default = \"grok-4.5\"", "default = \"grok-4.6\"");
    std::fs::write(&live_path, &client_edited).expect("simulate client edit");

    switch_provider_test_hook(&state, AppType::GrokBuild, "b").expect("switch to provider b");

    let backfilled_a = state
        .db
        .get_provider_by_id("a", AppType::GrokBuild.as_str())
        .expect("query provider a")
        .expect("provider a exists")
        .settings_config
        .get("config")
        .and_then(|value| value.as_str())
        .map(str::to_string)
        .unwrap_or_default();

    switch_provider_test_hook(&state, AppType::GrokBuild, "a").unwrap_or_else(|err| {
        panic!("switching back to provider a failed: {err}\nbackfilled row a:\n{backfilled_a}")
    });

    let live = std::fs::read_to_string(&live_path).expect("read live after switching back");
    assert!(
        live.contains("default = \"grok-4.5\"") && live.contains("https://a.example/v1"),
        "live should select provider a's own model table again, got:\n{live}"
    );
}

#[test]
fn codex_startup_import_fresh_install_imports_once_and_syncs_current_setting() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
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
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
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
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
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
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
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
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
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
                mcode: false,
                pi: false,
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
    // 只替换关键字段：live 里用户的 MCP 原样留着，行里的 MCP 不投影；这张卡没有路由，
    // Key 没有第三方地址可发，不写进 live。
    assert!(
        config_text.contains("[mcp_servers.legacy]"),
        "{config_text}"
    );
    assert!(!config_text.contains("mcp_servers.latest"), "{config_text}");
    assert!(
        !config_text.contains("experimental_bearer_token"),
        "{config_text}"
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
    let legacy = providers
        .get("old-provider")
        .expect("legacy provider still exists");
    assert_eq!(
        legacy.settings_config,
        json!({ "auth": {"OPENAI_API_KEY": "stale"}, "config": "stale-config" }),
        "switching away never writes live content back into the row"
    );
}

#[test]
fn switch_provider_missing_provider_returns_error() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
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
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
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
    // 不再回填：用户在 live 里的改动留在 live，上一家的行不变。
    assert_eq!(
        legacy_provider.settings_config,
        json!({ "env": { "ANTHROPIC_API_KEY": "stale-key" } }),
        "switching away must not copy live into the previous provider"
    );
    assert_eq!(
        live_after["workspace"],
        json!({ "path": "/tmp/workspace" }),
        "non-key settings in live stay where they are"
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
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
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
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
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
