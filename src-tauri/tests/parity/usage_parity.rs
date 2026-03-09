use cc_switch_lib::bridges::usage as usage_bridge;

use super::support::{
    create_empty_core_state, create_empty_legacy_state, ensure_test_home, reset_test_fs,
    seed_usage_log, test_mutex,
};

#[test]
fn usage_parity_summary_matches_legacy_for_seconds_window() {
    let _guard = test_mutex()
        .lock()
        .unwrap_or_else(|err| err.into_inner());

    reset_test_fs();
    let _home = ensure_test_home();
    let legacy_state = create_empty_legacy_state();
    seed_usage_log("req-usage-legacy", 1_710_100_000);
    let legacy = usage_bridge::legacy_get_usage_summary(
        &legacy_state,
        Some(1_710_099_000),
        Some(1_710_101_000),
    )
    .expect("legacy usage summary");
    drop(legacy_state);

    reset_test_fs();
    let _home = ensure_test_home();
    let _core_state = create_empty_core_state();
    seed_usage_log("req-usage-core", 1_710_100_000);
    let core = usage_bridge::get_usage_summary(Some(1_710_099_000), Some(1_710_101_000))
        .expect("core usage summary");

    assert_eq!(core.total_requests, legacy.total_requests);
    assert_eq!(core.total_cost, legacy.total_cost);
    assert_eq!(core.total_input_tokens, legacy.total_input_tokens);
    assert_eq!(core.total_output_tokens, legacy.total_output_tokens);
    assert_eq!(
        core.total_cache_creation_tokens,
        legacy.total_cache_creation_tokens
    );
    assert_eq!(core.total_cache_read_tokens, legacy.total_cache_read_tokens);
    assert_eq!(core.success_rate, legacy.success_rate);
}
