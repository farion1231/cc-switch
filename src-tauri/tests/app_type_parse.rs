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
}

#[test]
fn workbuddy_round_trips_through_its_serialized_names() {
    assert!(matches!(
        AppType::from_str("workbuddy"),
        Ok(AppType::WorkBuddy)
    ));
    // The dash and underscore spellings are the ones other commands and deep
    // links already use for multi-word apps.
    assert!(matches!(
        AppType::from_str("work-buddy"),
        Ok(AppType::WorkBuddy)
    ));
    assert!(matches!(
        AppType::from_str("Work_Buddy"),
        Ok(AppType::WorkBuddy)
    ));
    assert_eq!(AppType::WorkBuddy.as_str(), "workbuddy");

    let serialized = serde_json::to_string(&AppType::WorkBuddy).expect("serialize WorkBuddy");
    assert_eq!(serialized, "\"workbuddy\"");
    let parsed: AppType = serde_json::from_str("\"workbuddy\"").expect("deserialize WorkBuddy");
    assert_eq!(parsed, AppType::WorkBuddy);
    assert!(AppType::all().any(|app| app == AppType::WorkBuddy));
}

#[test]
fn workbuddy_is_additive_and_has_no_mcp_or_skill_surface() {
    assert!(AppType::WorkBuddy.is_additive_mode());
    assert!(!cc_switch_lib::McpApps::default().is_enabled_for(&AppType::WorkBuddy));
    assert!(!cc_switch_lib::SkillApps::default().is_enabled_for(&AppType::WorkBuddy));
}

#[test]
fn parse_unknown_app_returns_localized_error_message() {
    let err = AppType::from_str("unknown").unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("可选值") || msg.contains("Allowed"));
    assert!(msg.contains("unknown"));
}
