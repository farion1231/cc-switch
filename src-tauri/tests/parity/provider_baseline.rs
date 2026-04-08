use super::support::{
    claude_switch_config, codex_switch_config, ensure_test_home, reset_test_fs,
    run_legacy_switch_case, seed_claude_live, seed_codex_live, test_mutex,
};
use cc_switch_lib::AppType;

#[test]
fn provider_baseline_legacy_codex_switch_snapshot_is_stable() {
    let _guard = test_mutex().lock().unwrap_or_else(|err| err.into_inner());
    reset_test_fs();
    let _home = ensure_test_home();
    seed_codex_live();

    let snapshot = run_legacy_switch_case(&codex_switch_config(), AppType::Codex, "new-provider")
        .expect("legacy codex switch should succeed");

    assert_eq!(snapshot.current.as_deref(), Some("new-provider"));
    assert!(snapshot
        .files
        .get("codex/auth.json")
        .is_some_and(|text| text.contains("fresh-key")));
    assert!(snapshot
        .files
        .get("codex/config.toml")
        .is_some_and(|text| text.contains("mcp_servers.echo-server")));
}

#[test]
fn provider_baseline_legacy_claude_switch_snapshot_is_stable() {
    let _guard = test_mutex().lock().unwrap_or_else(|err| err.into_inner());
    reset_test_fs();
    let _home = ensure_test_home();
    seed_claude_live();

    let snapshot = run_legacy_switch_case(&claude_switch_config(), AppType::Claude, "new-provider")
        .expect("legacy claude switch should succeed");

    assert_eq!(snapshot.current.as_deref(), Some("new-provider"));
    assert!(snapshot
        .files
        .get("claude/settings.json")
        .is_some_and(|text| text.contains("fresh-key")));
}
