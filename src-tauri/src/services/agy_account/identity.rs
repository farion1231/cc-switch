//! 认出 agy 钥匙串里那份登录是哪个 Google 账号。只在本地解码，不发任何请求：
//! CC Switch 不用 agy 的 OAuth 客户端登录、续期，也不拿 agy 的令牌去问 Google。

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::Deserialize;

/// id_token 里认账号要用的字段。令牌是 agy 自己从 Google 拿到的，只解码不验签。
#[derive(Debug, Default, Clone, Deserialize)]
pub struct IdentityClaims {
    pub sub: Option<String>,
    pub email: Option<String>,
    pub name: Option<String>,
    pub picture: Option<String>,
}

pub fn identity(id_token: &str) -> Option<IdentityClaims> {
    let payload = id_token.split('.').nth(1)?;
    let bytes = URL_SAFE_NO_PAD.decode(payload.trim_end_matches('=')).ok()?;
    let claims: IdentityClaims = serde_json::from_slice(&bytes).ok()?;
    claims.sub.as_ref().filter(|sub| !sub.is_empty())?;
    Some(claims)
}

/// agy 1.5 起钥匙串里只有邮箱、没有 id_token（拿不到 Google 的 `sub`）。还没收录过的号
/// 用邮箱当账号 id；同一个邮箱以后总是对回这一条。
pub fn email_account_id(email: &str) -> String {
    format!("email:{}", email.trim().to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    fn fake_id_token(claims: Value) -> String {
        format!(
            "e30.{}.sig",
            URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).unwrap())
        )
    }

    #[test]
    fn identity_requires_a_subject() {
        let token = fake_id_token(json!({"sub": "123", "email": "a@example.com"}));
        let claims = identity(&token).unwrap();
        assert_eq!(claims.sub.as_deref(), Some("123"));
        assert_eq!(claims.email.as_deref(), Some("a@example.com"));
        assert!(identity(&fake_id_token(json!({"email": "a@example.com"}))).is_none());
        assert!(identity("garbage").is_none());
    }

    #[test]
    fn email_ids_ignore_case_and_whitespace() {
        assert_eq!(
            email_account_id(" Alice@Example.com "),
            "email:alice@example.com"
        );
    }
}
