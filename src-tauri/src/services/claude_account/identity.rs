//! 认出一个账号目录登的是谁：只读非机密字段，不碰凭据。
//!
//! - `<config>/.claude.json` 的 `oauthAccount`：Claude Code 登录后写的账号资料
//!   （`accountUuid`、`organizationUuid`、`emailAddress`、`organizationName`）。
//! - `claude auth status --json` 的输出：套餐（`subscriptionType`）只在这里有，
//!   `.claude.json` 缺资料时也用它补邮箱和组织名。

use std::path::Path;

use serde::Deserialize;
use serde_json::Value;

/// 一个账号目录的身份。`key` 是跨设备认同一个账号的依据（卡片绑定存的就是它）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    pub key: String,
    pub email: Option<String>,
    pub org_name: Option<String>,
}

/// `claude auth status --json` 里用得到的部分。
#[derive(Debug, Default, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthStatus {
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub org_name: Option<String>,
    #[serde(default)]
    pub subscription_type: Option<String>,
}

/// 账号 key：`<accountUuid>:<organizationUuid>`。同一个人在不同组织（个人 / Team）是两个号。
pub fn account_key(account_uuid: &str, organization_uuid: Option<&str>) -> String {
    format!("{account_uuid}:{}", organization_uuid.unwrap_or_default())
}

/// 从账号目录的 `.claude.json` 读身份；没登录、文件不在或缺 `accountUuid` 时为 `None`。
pub fn read_identity(config_dir: &Path) -> Option<Identity> {
    let bytes = std::fs::read(config_dir.join(".claude.json")).ok()?;
    let doc: Value = serde_json::from_slice(&bytes).ok()?;
    identity_from_profile(&doc)
}

pub fn identity_from_profile(doc: &Value) -> Option<Identity> {
    let account = doc.get("oauthAccount")?;
    let text = |key: &str| {
        account
            .get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    };
    let account_uuid = text("accountUuid")?;
    let org_uuid = text("organizationUuid");
    Some(Identity {
        key: account_key(&account_uuid, org_uuid.as_deref()),
        email: text("emailAddress"),
        org_name: text("organizationName"),
    })
}

/// 解析 `claude auth status --json` 的输出；前后可能夹着别的行，取第一个 JSON 对象。
pub fn parse_auth_status(output: &str) -> Option<AuthStatus> {
    let start = output.find('{')?;
    let end = output.rfind('}')?;
    serde_json::from_str(output.get(start..=end)?).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reads_identity_from_profile() {
        let doc = json!({
            "oauthAccount": {
                "accountUuid": "acc-1",
                "organizationUuid": "org-1",
                "emailAddress": "me@example.com",
                "organizationName": "Acme"
            },
            "mcpServers": {}
        });
        assert_eq!(
            identity_from_profile(&doc),
            Some(Identity {
                key: "acc-1:org-1".to_string(),
                email: Some("me@example.com".to_string()),
                org_name: Some("Acme".to_string()),
            })
        );
    }

    #[test]
    fn missing_account_uuid_is_no_identity() {
        assert_eq!(identity_from_profile(&json!({})), None);
        assert_eq!(
            identity_from_profile(&json!({"oauthAccount": {"emailAddress": "x@y"}})),
            None
        );
    }

    #[test]
    fn personal_and_team_are_different_keys() {
        assert_ne!(account_key("acc", None), account_key("acc", Some("org")));
    }

    #[test]
    fn parses_auth_status_with_noise() {
        let status = parse_auth_status(
            "warning: something\n{\"loggedIn\":true,\"email\":\"me@example.com\",\"subscriptionType\":\"max\"}\n",
        )
        .unwrap();
        assert_eq!(status.email.as_deref(), Some("me@example.com"));
        assert_eq!(status.subscription_type.as_deref(), Some("max"));
        assert!(parse_auth_status("not json").is_none());
    }
}
