use anyhow::anyhow;
use cc_switch_core::AppType;

const VALID_APP_TYPES: &str = "claude, codex, gemini, opencode, openclaw";

pub(crate) fn parse_app_type(s: &str) -> anyhow::Result<AppType> {
    s.parse().map_err(|_| {
        anyhow!(
            "Invalid app type: {}. Valid values: {}",
            s,
            VALID_APP_TYPES
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_app_type_accepts_openclaw() {
        assert!(matches!(
            parse_app_type("openclaw").expect("openclaw should parse"),
            AppType::OpenClaw
        ));
    }

    #[test]
    fn parse_app_type_rejects_unknown_app_with_consistent_message() {
        let err = parse_app_type("unknown").expect_err("unknown app should fail");
        assert!(err.to_string().contains(VALID_APP_TYPES));
    }
}
