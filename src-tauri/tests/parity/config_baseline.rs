use serde_json::json;

use cc_switch_lib::{bridges::config as config_bridge, AppType};

use super::support::{ensure_test_home, reset_test_fs, test_mutex};

#[test]
fn config_baseline_legacy_snippet_round_trip_is_stable() {
    let _guard = test_mutex().lock().unwrap_or_else(|err| err.into_inner());

    reset_test_fs();
    let _home = ensure_test_home();

    let snippet = r#"{"env":{"ANTHROPIC_BASE_URL":"https://example.com"}}"#.to_string();
    config_bridge::legacy_set_common_config_snippet("claude", Some(snippet.clone()))
        .expect("legacy set snippet");
    let stored = config_bridge::legacy_get_common_config_snippet("claude")
        .expect("legacy get snippet")
        .expect("snippet should exist");
    assert_eq!(stored, snippet);

    let extracted = config_bridge::legacy_extract_common_config_snippet(
        AppType::Claude,
        Some(json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://example.com",
                "ANTHROPIC_AUTH_TOKEN": "secret"
            }
        })),
    )
    .expect("legacy extract snippet");
    let parsed: serde_json::Value = serde_json::from_str(&extracted).expect("snippet json");
    assert_eq!(parsed, json!({}));
}
