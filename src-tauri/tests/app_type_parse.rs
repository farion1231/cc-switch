use std::str::FromStr;

use cc_switch_lib::AppType;

#[test]
fn parse_known_apps_case_insensitive_and_trim() {
    assert!(matches!(AppType::from_str("claude"), Ok(AppType::Claude)));
    assert!(matches!(AppType::from_str("codex"), Ok(AppType::Codex)));
    assert!(matches!(
        AppType::from_str("grokbuild"),
        Ok(AppType::GrokBuild)
    ));
    assert!(matches!(
        AppType::from_str("Grok-Build"),
        Ok(AppType::GrokBuild)
    ));
    assert!(matches!(
        AppType::from_str(" ClAuDe \n"),
        Ok(AppType::Claude)
    ));
    assert!(matches!(AppType::from_str("\tcoDeX\t"), Ok(AppType::Codex)));
    for alias in ["deepseek-harness", "deepseek_harness", "DSH"] {
        assert!(matches!(
            AppType::from_str(alias),
            Ok(AppType::DeepSeekHarness)
        ));
    }
    assert_eq!(AppType::DeepSeekHarness.as_str(), "deepseek-harness");
    assert!(!AppType::DeepSeekHarness.is_additive_mode());
    assert!(AppType::all().any(|app| matches!(app, AppType::DeepSeekHarness)));
    assert_eq!(
        serde_json::to_string(&AppType::DeepSeekHarness).unwrap(),
        "\"deepseek-harness\""
    );
}

#[test]
fn parse_unknown_app_returns_localized_error_message() {
    let err = AppType::from_str("unknown").unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("可选值") || msg.contains("Allowed"));
    assert!(msg.contains("unknown"));
}
