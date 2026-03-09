use serde_json::json;

use cc_switch_lib::{bridges::config as config_bridge, AppType};

use super::support::{ensure_test_home, reset_test_fs, test_mutex};

#[test]
fn config_parity_snippet_round_trip_matches_legacy() {
    let _guard = test_mutex()
        .lock()
        .unwrap_or_else(|err| err.into_inner());

    let snippet = r#"{"env":{"ANTHROPIC_BASE_URL":"https://example.com"}}"#.to_string();

    reset_test_fs();
    let _home = ensure_test_home();
    config_bridge::legacy_set_common_config_snippet("claude", Some(snippet.clone()))
        .expect("legacy set snippet");
    let legacy = config_bridge::legacy_get_common_config_snippet("claude")
        .expect("legacy get snippet");

    reset_test_fs();
    let _home = ensure_test_home();
    config_bridge::set_common_config_snippet("claude", Some(snippet.clone()))
        .expect("core set snippet");
    let core = config_bridge::get_common_config_snippet("claude").expect("core get snippet");

    assert_eq!(core, legacy);
}

#[test]
fn config_parity_extract_matches_legacy() {
    let _guard = test_mutex()
        .lock()
        .unwrap_or_else(|err| err.into_inner());

    reset_test_fs();
    let _home = ensure_test_home();
    let input = Some(json!({
        "env": {
            "ANTHROPIC_BASE_URL": "https://example.com",
            "ANTHROPIC_AUTH_TOKEN": "secret"
        }
    }));
    let legacy = config_bridge::legacy_extract_common_config_snippet(AppType::Claude, input.clone())
        .expect("legacy extract");

    reset_test_fs();
    let _home = ensure_test_home();
    let core = config_bridge::extract_common_config_snippet(AppType::Claude, input)
        .expect("core extract");

    let legacy_json: serde_json::Value = serde_json::from_str(&legacy).expect("legacy json");
    let core_json: serde_json::Value = serde_json::from_str(&core).expect("core json");
    assert_eq!(core_json, legacy_json);
}
