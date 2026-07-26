use serde_json::json;

use cc_switch_lib::{
    get_claude_settings_path, read_json_file, AppType, MultiAppConfig, Provider, ProviderMeta,
    ProviderService,
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
    ProviderService::add(&state, AppType::Claude, provider.clone(), true)
        .expect("add provider");

    // The shared statusLine must survive the write, sourced from the refreshed snippet.
    let live_after: serde_json::Value =
        read_json_file(&get_claude_settings_path()).expect("read live after add");
    assert_eq!(
        live_after["statusLine"]["command"],
        "node /custom/statusline.mjs",
        "add() must preserve the live-side shared statusLine via pre-write snippet sync"
    );
}
