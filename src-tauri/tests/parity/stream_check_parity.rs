use cc_switch_lib::bridges::stream_check as stream_check_bridge;

use super::support::{create_empty_legacy_state, ensure_test_home, reset_test_fs, test_mutex};

#[test]
fn stream_check_parity_save_and_get_config_matches_legacy() {
    let _guard = test_mutex().lock().unwrap_or_else(|err| err.into_inner());

    reset_test_fs();
    let _home = ensure_test_home();
    let legacy = {
        let legacy_state = create_empty_legacy_state();
        let mut config =
            stream_check_bridge::legacy_get_config(&legacy_state).expect("legacy default config");
        config.timeout_secs = 22;
        config.max_retries = 3;
        config.degraded_threshold_ms = 4567;
        config.claude_model = "claude-parity".to_string();
        config.codex_model = "codex-parity".to_string();
        config.gemini_model = "gemini-parity".to_string();
        config.test_prompt = "hello".to_string();
        stream_check_bridge::legacy_save_config(&legacy_state, &config)
            .expect("legacy save config");
        stream_check_bridge::legacy_get_config(&legacy_state).expect("legacy get config")
    };

    reset_test_fs();
    let _home = ensure_test_home();
    let mut config = stream_check_bridge::get_config().expect("core default config");
    config.timeout_secs = 22;
    config.max_retries = 3;
    config.degraded_threshold_ms = 4567;
    config.claude_model = "claude-parity".to_string();
    config.codex_model = "codex-parity".to_string();
    config.gemini_model = "gemini-parity".to_string();
    config.test_prompt = "hello".to_string();
    stream_check_bridge::save_config(config).expect("core save config");
    let core = stream_check_bridge::get_config().expect("core get config");

    assert_eq!(core.timeout_secs, legacy.timeout_secs);
    assert_eq!(core.max_retries, legacy.max_retries);
    assert_eq!(core.degraded_threshold_ms, legacy.degraded_threshold_ms);
    assert_eq!(core.claude_model, legacy.claude_model);
    assert_eq!(core.codex_model, legacy.codex_model);
    assert_eq!(core.gemini_model, legacy.gemini_model);
    assert_eq!(core.test_prompt, legacy.test_prompt);
}
