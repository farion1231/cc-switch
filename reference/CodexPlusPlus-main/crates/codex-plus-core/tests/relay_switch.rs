use codex_plus_core::relay_switch::switch_relay_profile_in_home;
use codex_plus_core::settings::{
    AggregateRelayMember, AggregateRelayProfile, AggregateRelayStrategy, BackendSettings,
    LaunchMode, RelayMode, RelayProfile, SettingsStore,
};

#[test]
fn switch_rolls_back_active_settings_when_live_write_fails() {
    let temp = tempfile::tempdir().unwrap();
    let store = SettingsStore::new(temp.path().join("settings.json"));
    let original = BackendSettings {
        active_relay_id: "a".to_string(),
        relay_profiles: vec![pure_profile("a", "https://a.example/v1", "sk-a")],
        ..BackendSettings::default()
    };
    store.save(&original).unwrap();
    std::fs::create_dir(temp.path().join("codex")).unwrap();
    std::fs::write(
        temp.path().join("codex").join("auth.json"),
        r#"{"OPENAI_API_KEY":"sk-a"}"#,
    )
    .unwrap();
    std::fs::write(
        temp.path().join("codex").join("config.toml"),
        r#"model_provider = "custom"

[model_providers.custom]
name = "custom"
wire_api = "responses"
requires_openai_auth = true
base_url = "https://a.example/v1"
"#,
    )
    .unwrap();
    let next = BackendSettings {
        active_relay_id: "b".to_string(),
        relay_profiles: vec![
            pure_profile("a", "https://a.example/v1", "sk-a"),
            RelayProfile {
                id: "b".to_string(),
                name: "B".to_string(),
                relay_mode: RelayMode::PureApi,
                config_contents: "model_provider = \"custom\"\n".to_string(),
                auth_contents: "{bad json".to_string(),
                ..RelayProfile::default()
            },
        ],
        ..BackendSettings::default()
    };

    let error = switch_relay_profile_in_home(&store, &temp.path().join("codex"), next, "a")
        .expect_err("invalid auth should fail switch");

    assert!(error.to_string().contains("auth.json"));
    assert_eq!(store.load().unwrap().active_relay_id, "a");
}

#[test]
fn switch_backfills_previous_profile_from_live_before_selecting_target() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("codex");
    std::fs::create_dir(&home).unwrap();
    std::fs::write(
        home.join("config.toml"),
        r#"model = "edited-live-model"
model_provider = "manual_a"
model_context_window = 1000000
model_auto_compact_token_limit = 900000

[model_providers.manual_a]
name = "manual_a"
wire_api = "responses"
requires_openai_auth = true
base_url = "https://edited-a.example/v1"
"#,
    )
    .unwrap();
    std::fs::write(
        home.join("auth.json"),
        r#"{"OPENAI_API_KEY":"sk-edited-a"}"#,
    )
    .unwrap();
    let store = SettingsStore::new(temp.path().join("settings.json"));
    let original = BackendSettings {
        active_relay_id: "a".to_string(),
        relay_profiles: vec![
            pure_profile("a", "https://a.example/v1", "sk-a"),
            pure_profile("b", "https://b.example/v1", "sk-b"),
        ],
        ..BackendSettings::default()
    };
    store.save(&original).unwrap();
    let next = BackendSettings {
        active_relay_id: "b".to_string(),
        relay_profiles: original.relay_profiles.clone(),
        ..BackendSettings::default()
    };

    switch_relay_profile_in_home(&store, &home, next, "a").unwrap();

    let stored = store.load().unwrap();
    let previous = stored
        .relay_profiles
        .iter()
        .find(|profile| profile.id == "a")
        .unwrap();
    assert!(previous.config_contents.contains("edited-live-model"));
    assert!(previous.config_contents.contains("manual_a"));
    assert_eq!(previous.context_window, "1000000");
    assert_eq!(previous.auto_compact_limit, "900000");
    assert_eq!(stored.active_relay_id, "b");
    assert_eq!(stored.launch_mode, LaunchMode::Patch);
}

#[test]
fn switch_to_aggregate_relay_allows_empty_config_snapshot() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("codex");
    std::fs::create_dir(&home).unwrap();
    let store = SettingsStore::new(temp.path().join("settings.json"));
    let api = pure_profile("api", "https://api.example/v1", "sk-api");
    let aggregate = RelayProfile {
        id: "agg".to_string(),
        name: "聚合供应商 1".to_string(),
        relay_mode: RelayMode::Aggregate,
        config_contents: String::new(),
        auth_contents: String::new(),
        ..RelayProfile::default()
    };
    let original = BackendSettings {
        active_relay_id: "api".to_string(),
        relay_profiles: vec![api.clone(), aggregate.clone()],
        ..BackendSettings::default()
    };
    store.save(&original).unwrap();
    let next = BackendSettings {
        active_relay_id: "agg".to_string(),
        relay_profiles: vec![api, aggregate],
        aggregate_relay_profiles: vec![AggregateRelayProfile {
            id: "agg".to_string(),
            name: "聚合供应商 1".to_string(),
            strategy: AggregateRelayStrategy::Failover,
            members: vec![AggregateRelayMember {
                relay_id: "api".to_string(),
                weight: 1,
            }],
        }],
        active_aggregate_relay_id: "agg".to_string(),
        ..BackendSettings::default()
    };

    let result = switch_relay_profile_in_home(&store, &home, next, "api").unwrap();
    let live = std::fs::read_to_string(home.join("config.toml")).unwrap();

    assert!(result.configured);
    assert_eq!(store.load().unwrap().active_relay_id, "agg");
    assert!(live.contains(r#"base_url = "http://127.0.0.1:57321/v1""#));
}

#[test]
fn switch_returns_normalized_previous_official_profile_after_backfill() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("codex");
    std::fs::create_dir(&home).unwrap();
    std::fs::write(
        home.join("config.toml"),
        r#"model = "gpt-5.5"
model_reasoning_effort = "high"
model_provider = "custom"

[model_providers.custom]
name = "custom"
wire_api = "responses"
requires_openai_auth = true
base_url = "https://third-party.example/v1"

[features]
goals = true
"#,
    )
    .unwrap();
    std::fs::write(
        home.join("auth.json"),
        r#"{"OPENAI_API_KEY":"sk-third-party"}"#,
    )
    .unwrap();
    let store = SettingsStore::new(temp.path().join("settings.json"));
    let official = RelayProfile {
        id: "official".to_string(),
        name: "官方".to_string(),
        relay_mode: RelayMode::Official,
        official_mix_api_key: false,
        auth_contents: r#"{"auth_mode":"chatgpt","tokens":{"access_token":"official"}}"#
            .to_string(),
        ..RelayProfile::default()
    };
    let pure = pure_profile("api", "https://third-party.example/v1", "sk-third-party");
    let original = BackendSettings {
        active_relay_id: "official".to_string(),
        relay_profiles: vec![official.clone(), pure.clone()],
        ..BackendSettings::default()
    };
    store.save(&original).unwrap();
    let next = BackendSettings {
        active_relay_id: "api".to_string(),
        relay_profiles: vec![official, pure],
        ..BackendSettings::default()
    };

    let result = switch_relay_profile_in_home(&store, &home, next, "official").unwrap();
    let returned = result
        .settings
        .relay_profiles
        .iter()
        .find(|profile| profile.id == "official")
        .unwrap();

    assert_eq!(returned.relay_mode, RelayMode::Official);
    assert!(!returned.official_mix_api_key);
    assert!(returned.config_contents.is_empty());
    assert!(returned.api_key.is_empty());
}

fn pure_profile(id: &str, base_url: &str, key: &str) -> RelayProfile {
    RelayProfile {
        id: id.to_string(),
        name: id.to_uppercase(),
        relay_mode: RelayMode::PureApi,
        config_contents: format!(
            r#"model_provider = "custom"

[model_providers.custom]
name = "custom"
wire_api = "responses"
requires_openai_auth = true
base_url = "{base_url}"
"#
        ),
        auth_contents: format!(r#"{{"OPENAI_API_KEY":"{key}"}}"#),
        ..RelayProfile::default()
    }
}
