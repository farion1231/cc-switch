use cc_switch_lib::bridges::usage as usage_bridge;

use super::support::{
    create_empty_legacy_state, ensure_test_home, reset_test_fs, seed_usage_log, test_mutex,
};

#[test]
fn usage_baseline_legacy_summary_handles_seconds_window() {
    let _guard = test_mutex().lock().unwrap_or_else(|err| err.into_inner());
    reset_test_fs();
    let _home = ensure_test_home();
    let legacy_state = create_empty_legacy_state();
    seed_usage_log("req-usage-baseline", 1_710_000_000);

    let summary = usage_bridge::legacy_get_usage_summary(
        &legacy_state,
        Some(1_709_999_000),
        Some(1_710_001_000),
    )
    .expect("legacy usage summary");

    assert_eq!(summary.total_requests, 1);
    assert_eq!(summary.total_input_tokens, 120);
    assert_eq!(summary.total_output_tokens, 80);
    assert_eq!(summary.total_cache_read_tokens, 10);
    assert_eq!(summary.total_cache_creation_tokens, 5);
    assert_eq!(summary.success_rate, 100.0);
}
