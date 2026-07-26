use serde_json::json;

use cc_switch_lib::{
    get_claude_settings_path, get_codex_config_path, read_json_file, write_codex_live_atomic,
    AppType, MultiAppConfig, Provider, ProviderMeta, ProviderService,
};

#[path = "support.rs"]
mod support;
use support::{create_test_state_with_config, ensure_test_home, reset_test_fs, test_mutex};

/// Seed `~/.claude/settings.json` with a live settings blob carrying a
/// distinctive `statusLine` — a *shared* config field that the common-config
/// snippet is meant to carry — plus provider-specific env keys (model + key)
/// that the extractor strips. The provider's own `settings_config` will NOT
/// carry this statusLine, so it only survives the write if the snippet is
/// refreshed from live before the write (the pre-write sync under test).
fn seed_dirty_claude_live(statusline_command: &str) {
    let live = json!({
        "env": {
            "ANTHROPIC_MODEL": "live-model-not-shared",
            "ANTHROPIC_API_KEY": "live-key-not-shared"
        },
        "statusLine": { "command": statusline_command, "type": "command" }
    });
    let settings_path = get_claude_settings_path();
    std::fs::create_dir_all(settings_path.parent().expect("settings dir")).expect("create dir");
    std::fs::write(
        &settings_path,
        serde_json::to_string_pretty(&live).expect("serialize live"),
    )
    .expect("write live settings");
}

/// A Claude provider opted into common-config sharing, whose `settings_config`
/// holds provider-specific env (model + key) but NO `statusLine`. The
/// statusLine only reaches live through the common-config snippet.
fn claude_provider_with_common_config(provider_id: &str) -> Provider {
    let mut provider = Provider::with_id(
        provider_id.to_string(),
        provider_id.to_string(),
        json!({
            "env": {
                "ANTHROPIC_API_KEY": "provider-key",
                "ANTHROPIC_MODEL": "provider-model"
            }
        }),
        None,
    );
    provider.meta = Some(ProviderMeta {
        common_config_enabled: Some(true),
        ..ProviderMeta::default()
    });
    provider
}

#[test]
fn add_new_provider_preserves_live_statusline_via_common_config() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let _home = ensure_test_home();

    // Live has a statusLine the provider's settings_config does not carry.
    seed_dirty_claude_live("node /custom/statusline.mjs");

    let mut config = MultiAppConfig::default();
    let provider = claude_provider_with_common_config("p1");
    {
        let manager = config
            .get_manager_mut(&AppType::Claude)
            .expect("claude manager");
        manager.providers.insert("p1".to_string(), provider.clone());
    }
    let state = create_test_state_with_config(&config).expect("create test state");

    // No current provider → add() takes the `current.is_none()` branch and writes live.
    ProviderService::add(&state, AppType::Claude, provider.clone(), true).expect("add provider");

    // The shared statusLine must survive the write, sourced from the refreshed snippet.
    let live_after: serde_json::Value =
        read_json_file(&get_claude_settings_path()).expect("read live after add");
    assert_eq!(
        live_after["statusLine"]["command"], "node /custom/statusline.mjs",
        "add() must preserve the live-side shared statusLine via pre-write snippet sync"
    );
}

#[test]
fn update_current_provider_preserves_live_statusline_via_common_config() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let _home = ensure_test_home();

    seed_dirty_claude_live("node /custom/statusline.mjs");

    let mut config = MultiAppConfig::default();
    let provider = claude_provider_with_common_config("p1");
    {
        let manager = config
            .get_manager_mut(&AppType::Claude)
            .expect("claude manager");
        manager.current = "p1".to_string();
        manager.providers.insert("p1".to_string(), provider.clone());
    }
    let state = create_test_state_with_config(&config).expect("create test state");

    // update() with no takeover → takes the `else` branch and writes live.
    ProviderService::update(&state, AppType::Claude, None, provider).expect("update provider");

    let live_after: serde_json::Value =
        read_json_file(&get_claude_settings_path()).expect("read live after update");
    assert_eq!(
        live_after["statusLine"]["command"], "node /custom/statusline.mjs",
        "update() must preserve the live-side shared statusLine via pre-write snippet sync"
    );
}

#[test]
fn sync_current_provider_to_live_preserves_live_statusline_via_common_config() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let _home = ensure_test_home();

    seed_dirty_claude_live("node /custom/statusline.mjs");

    let mut config = MultiAppConfig::default();
    let provider = claude_provider_with_common_config("p1");
    {
        let manager = config
            .get_manager_mut(&AppType::Claude)
            .expect("claude manager");
        manager.current = "p1".to_string();
        manager.providers.insert("p1".to_string(), provider);
    }
    let state = create_test_state_with_config(&config).expect("create test state");

    // No takeover → sync_current_provider_for_app reaches the non-takeover write.
    ProviderService::sync_current_provider_for_app(&state, AppType::Claude)
        .expect("sync current provider");

    let live_after: serde_json::Value =
        read_json_file(&get_claude_settings_path()).expect("read live after sync");
    assert_eq!(
        live_after["statusLine"]["command"],
        "node /custom/statusline.mjs",
        "sync_current_provider_for_app must preserve the live-side shared statusLine via pre-write snippet sync"
    );
}

#[test]
fn reapply_codex_official_preserves_live_shared_config_field() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let _home = ensure_test_home();

    // Seed Codex live (auth.json + config.toml) with a shared field
    // (model_reasoning_effort) that the extractor keeps but the provider's
    // settings_config does not carry.
    let live_auth = json!({
        "auth_mode": "chatgpt",
        "OPENAI_API_KEY": null,
        "tokens": { "access_token": "official-oauth-token", "account_id": "acct" }
    });
    write_codex_live_atomic(&live_auth, Some("model_reasoning_effort = \"high\"\n"))
        .expect("seed codex live with shared field");

    let mut config = MultiAppConfig::default();
    {
        let manager = config
            .get_manager_mut(&AppType::Codex)
            .expect("codex manager");
        let mut official = Provider::with_id(
            "official-provider".to_string(),
            "Official".to_string(),
            json!({
                "auth": {
                    "auth_mode": "chatgpt",
                    "OPENAI_API_KEY": null,
                    "tokens": { "access_token": "official-oauth-token", "account_id": "acct" }
                },
                "config": ""
            }),
            None,
        );
        official.category = Some("official".to_string());
        official.meta = Some(ProviderMeta {
            common_config_enabled: Some(true),
            ..ProviderMeta::default()
        });
        manager.current = "official-provider".to_string();
        manager
            .providers
            .insert("official-provider".to_string(), official);
    }
    let state = create_test_state_with_config(&config).expect("create test state");

    cc_switch_lib::reapply_current_codex_official_live(&state).expect("reapply official live");

    let live_toml =
        std::fs::read_to_string(get_codex_config_path()).expect("read config.toml after reapply");
    assert!(
        live_toml.contains("model_reasoning_effort"),
        "reapply must preserve the live-side Codex shared field via pre-write snippet sync, got: {live_toml}"
    );
}

#[test]
fn sync_simple_is_noop_for_additive_mode_app() {
    // Additive-mode apps (OpenCode/OpenClaw) don't use common-config snippets;
    // the helper must no-op without touching the snippet DB or live file.
    // Mirrors the switch path's Claude+Codex-only scope guard.
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let _home = ensure_test_home();

    let mut config = MultiAppConfig::default();
    let provider = Provider::with_id(
        "opencode-p1".to_string(),
        "OpenCode P1".to_string(),
        json!({}),
        None,
    );
    {
        let manager = config
            .get_manager_mut(&AppType::OpenCode)
            .expect("opencode manager");
        manager.current = "opencode-p1".to_string();
        manager
            .providers
            .insert("opencode-p1".to_string(), provider.clone());
    }
    let state = create_test_state_with_config(&config).expect("create test state");

    // Pre-set a snippet so we can detect if the helper wrongly overwrites it.
    state
        .db
        .set_config_snippet(
            AppType::OpenCode.as_str(),
            Some(r#"{"marker":"preset"}"#.to_string()),
        )
        .expect("set preset snippet");

    // Drive the OpenCode sync path — additive mode, helper must no-op.
    ProviderService::sync_current_provider_for_app(&state, AppType::OpenCode)
        .expect("sync opencode current provider");

    let snippet = state
        .db
        .get_config_snippet(AppType::OpenCode.as_str())
        .expect("get snippet")
        .expect("snippet present");
    assert!(
        snippet.contains("preset"),
        "additive-mode sync must not touch the common-config snippet, got: {snippet}"
    );
}
