use cc_switch_lib::bridges::deeplink as deeplink_bridge;
use serde_json::{json, Value};

use super::support::{
    create_empty_core_state, create_empty_legacy_state, ensure_test_home, reset_test_fs, test_mutex,
};

const PROVIDER_URL: &str = "ccswitch://v1/import?resource=provider&app=claude&name=DemoProvider&homepage=https%3A%2F%2Fexample.com&endpoint=https%3A%2F%2Fapi.example.com%2Fv1&apiKey=secret";

fn normalize_provider_snapshot(snapshot: Value) -> Value {
    let providers = snapshot
        .as_object()
        .expect("provider snapshot should be an object");
    assert_eq!(providers.len(), 1, "expected a single imported provider");

    let provider = providers.values().next().expect("provider should exist");
    json!({
        "name": provider["name"],
        "websiteUrl": provider["websiteUrl"],
        "settingsConfig": provider["settingsConfig"],
        "meta": provider["meta"],
        "inFailoverQueue": provider["inFailoverQueue"],
    })
}

#[test]
fn deeplink_parity_parse_and_merge_match_legacy() {
    let _guard = test_mutex()
        .lock()
        .unwrap_or_else(|err| err.into_inner());

    let legacy = deeplink_bridge::legacy_merge_deeplink_config(
        deeplink_bridge::legacy_parse_deeplink(PROVIDER_URL).expect("legacy parse"),
    )
    .expect("legacy merge");
    let core = deeplink_bridge::merge_deeplink_config(
        deeplink_bridge::parse_deeplink(PROVIDER_URL).expect("core parse"),
    )
    .expect("core merge");

    assert_eq!(
        serde_json::to_value(core).expect("core json"),
        serde_json::to_value(legacy).expect("legacy json")
    );
}

#[test]
fn deeplink_parity_provider_import_matches_legacy() {
    let _guard = test_mutex()
        .lock()
        .unwrap_or_else(|err| err.into_inner());

    reset_test_fs();
    let _home = ensure_test_home();
    let legacy_state = create_empty_legacy_state();
    let legacy_request = deeplink_bridge::legacy_merge_deeplink_config(
        deeplink_bridge::legacy_parse_deeplink(PROVIDER_URL).expect("legacy parse"),
    )
    .expect("legacy merge");
    deeplink_bridge::legacy_import_provider(&legacy_state, legacy_request)
        .expect("legacy import provider");
    let legacy_snapshot = serde_json::to_value(
        legacy_state
            .db
            .get_all_providers("claude")
            .expect("legacy providers"),
    )
    .expect("legacy snapshot");

    reset_test_fs();
    let _home = ensure_test_home();
    let _core_state = create_empty_core_state();
    let core_request =
        deeplink_bridge::merge_deeplink_config(deeplink_bridge::parse_deeplink(PROVIDER_URL).expect("core parse"))
            .expect("core merge");
    deeplink_bridge::import_provider(core_request).expect("core import provider");
    let core_state = create_empty_core_state();
    let core_snapshot = serde_json::to_value(
        core_state
            .db
            .get_all_providers("claude")
            .expect("core providers"),
    )
    .expect("core snapshot");

    assert_eq!(
        normalize_provider_snapshot(core_snapshot),
        normalize_provider_snapshot(legacy_snapshot)
    );
}
