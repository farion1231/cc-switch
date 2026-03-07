use anyhow::anyhow;
use cc_switch_core::AppType;

const VALID_APP_TYPES: &str = "claude, codex, gemini, opencode, openclaw";
const PROXY_APP_TYPES: &str = "claude, codex, gemini";

pub(crate) fn parse_app_type(s: &str) -> anyhow::Result<AppType> {
    s.parse()
        .map_err(|_| anyhow!("Invalid app type: {}. Valid values: {}", s, VALID_APP_TYPES))
}

pub(crate) fn parse_proxy_app_type(s: &str) -> anyhow::Result<AppType> {
    let app_type = parse_app_type(s)?;
    match app_type {
        AppType::Claude | AppType::Codex | AppType::Gemini => Ok(app_type),
        _ => Err(anyhow!(
            "Proxy commands currently support only {}. Unsupported app: {}",
            PROXY_APP_TYPES,
            s
        )),
    }
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

    #[test]
    fn parse_proxy_app_type_rejects_additive_apps() {
        let err = parse_proxy_app_type("openclaw").expect_err("openclaw should be unsupported");
        assert!(err.to_string().contains(PROXY_APP_TYPES));
    }
}
