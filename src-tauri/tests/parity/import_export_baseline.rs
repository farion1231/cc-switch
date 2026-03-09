use serde_json::json;

use cc_switch_lib::{
    bridges::import_export as import_export_bridge, AppType, MultiAppConfig, Provider,
};

use super::support::{
    create_empty_legacy_state, create_legacy_state_with_config, ensure_test_home, reset_test_fs,
    test_mutex,
};

fn export_file() -> std::path::PathBuf {
    std::env::temp_dir().join("cc-switch-tauri-parity-export.sql")
}

fn import_config() -> MultiAppConfig {
    let mut config = MultiAppConfig::default();
    let manager = config
        .get_manager_mut(&AppType::Claude)
        .expect("claude manager");
    manager.current = "provider-a".to_string();
    manager.providers.insert(
        "provider-a".to_string(),
        Provider::with_id(
            "provider-a".to_string(),
            "Provider A".to_string(),
            json!({
                "env": {
                    "ANTHROPIC_BASE_URL": "https://example.com",
                    "ANTHROPIC_AUTH_TOKEN": "secret"
                }
            }),
            None,
        ),
    );
    config
}

#[test]
fn import_export_baseline_legacy_round_trip_is_stable() {
    let _guard = test_mutex()
        .lock()
        .unwrap_or_else(|err| err.into_inner());

    let export = export_file();
    let _ = std::fs::remove_file(&export);

    reset_test_fs();
    let _home = ensure_test_home();
    let legacy_state = create_legacy_state_with_config(&import_config());
    import_export_bridge::legacy_export_config_to_file(&legacy_state, export.to_string_lossy().as_ref())
        .expect("legacy export");

    reset_test_fs();
    let _home = ensure_test_home();
    let import_state = create_empty_legacy_state();
    import_export_bridge::legacy_import_config_from_file(&import_state, export.to_string_lossy().as_ref())
        .expect("legacy import");

    let providers = import_state.db.get_all_providers("claude").expect("legacy providers");
    assert!(providers.get("provider-a").is_some());
}
