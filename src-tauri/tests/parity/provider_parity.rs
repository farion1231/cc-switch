use super::support::{
    claude_switch_config, codex_switch_config, ensure_test_home, reset_test_fs,
    run_core_switch_case, run_legacy_switch_case, seed_claude_live, seed_codex_live, test_mutex,
};
use cc_switch_lib::AppType;

#[test]
fn provider_parity_codex_switch_matches_legacy() {
    let _guard = test_mutex().lock().unwrap_or_else(|err| err.into_inner());

    reset_test_fs();
    let _home = ensure_test_home();
    seed_codex_live();
    let legacy = run_legacy_switch_case(&codex_switch_config(), AppType::Codex, "new-provider")
        .expect("legacy codex switch should succeed");

    reset_test_fs();
    let _home = ensure_test_home();
    seed_codex_live();
    let core = run_core_switch_case(&codex_switch_config(), AppType::Codex, "new-provider")
        .expect("core codex switch should succeed");

    assert_eq!(core, legacy);
}

#[test]
fn provider_parity_claude_switch_matches_legacy() {
    let _guard = test_mutex().lock().unwrap_or_else(|err| err.into_inner());

    reset_test_fs();
    let _home = ensure_test_home();
    seed_claude_live();
    let legacy = run_legacy_switch_case(&claude_switch_config(), AppType::Claude, "new-provider")
        .expect("legacy claude switch should succeed");

    reset_test_fs();
    let _home = ensure_test_home();
    seed_claude_live();
    let core = run_core_switch_case(&claude_switch_config(), AppType::Claude, "new-provider")
        .expect("core claude switch should succeed");

    assert_eq!(core, legacy);
}
