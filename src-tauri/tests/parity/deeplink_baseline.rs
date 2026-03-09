use cc_switch_lib::bridges::deeplink as deeplink_bridge;

use super::support::{create_empty_legacy_state, ensure_test_home, reset_test_fs, test_mutex};

const PROVIDER_URL: &str = "ccswitch://v1/import?resource=provider&app=claude&name=DemoProvider&homepage=https%3A%2F%2Fexample.com&endpoint=https%3A%2F%2Fapi.example.com%2Fv1&apiKey=secret";

#[test]
fn deeplink_baseline_legacy_parse_merge_and_import_are_stable() {
    let _guard = test_mutex()
        .lock()
        .unwrap_or_else(|err| err.into_inner());

    reset_test_fs();
    let _home = ensure_test_home();
    let parsed = deeplink_bridge::legacy_parse_deeplink(PROVIDER_URL).expect("legacy parse");
    assert_eq!(parsed.resource, "provider");

    let merged = deeplink_bridge::legacy_merge_deeplink_config(parsed).expect("legacy merge");
    assert_eq!(merged.app.as_deref(), Some("claude"));

    let state = create_empty_legacy_state();
    let provider_id =
        deeplink_bridge::legacy_import_provider(&state, merged).expect("legacy import provider");
    let providers = state.db.get_all_providers("claude").expect("export providers");
    assert!(providers.get(&provider_id).is_some());
}
