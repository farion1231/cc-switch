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
fn parse_unknown_app_returns_localized_error_message() {
    let err = AppType::from_str("unknown").unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("可选值") || msg.contains("Allowed"));
    assert!(msg.contains("unknown"));
}

#[test]
fn qoder_editions_have_independent_ids_and_cn_command_alias() {
    assert_eq!(AppType::from_str("qoder").unwrap(), AppType::Qoder);
    assert_eq!(AppType::from_str("qoder-cn").unwrap(), AppType::QoderCn);
    assert_eq!(AppType::from_str("qodercn").unwrap(), AppType::QoderCn);
    assert_eq!(AppType::QoderCn.as_str(), "qoder-cn");
    assert_eq!(
        serde_json::to_string(&AppType::QoderCn).unwrap(),
        "\"qoder-cn\""
    );
}
