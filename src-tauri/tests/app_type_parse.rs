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
    assert!(matches!(AppType::from_str("gemini"), Ok(AppType::Gemini)));
    assert!(matches!(
        AppType::from_str("antigravity"),
        Ok(AppType::Gemini)
    ));
    assert!(matches!(
        AppType::from_str(" AntiGravity \n"),
        Ok(AppType::Gemini)
    ));
}

#[test]
fn deserialize_antigravity_alias_to_gemini() {
    let parsed: AppType = serde_json::from_str("\"antigravity\"").expect("deserialize antigravity");
    assert_eq!(parsed, AppType::Gemini);

    let parsed_gemini: AppType = serde_json::from_str("\"gemini\"").expect("deserialize gemini");
    assert_eq!(parsed_gemini, AppType::Gemini);
}

#[test]
fn parse_unknown_app_returns_localized_error_message() {
    let err = AppType::from_str("unknown").unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("可选值") || msg.contains("Allowed"));
    assert!(msg.contains("unknown"));
}
