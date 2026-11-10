//! Local auth.json import. Claims are consistency checked, not signature verified.
//! No network requests or changes to Codex's active login occur during import.
use super::*;
use serde_json::Value;

const MAX_AUTH_BYTES: usize = 1024 * 1024;

fn invalid(message: &str) -> CodexOAuthError {
    CodexOAuthError::ParseError(message.to_string())
}

fn required(value: &Value, key: &str) -> Result<String, CodexOAuthError> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .map(str::to_owned)
        .ok_or_else(|| invalid(&format!("auth.json 缺少有效的 {key}")))
}

fn claims(token: &str) -> Result<Value, CodexOAuthError> {
    let parts: Vec<_> = token.split('.').collect();
    if parts.len() != 3 || parts.iter().any(|p| p.is_empty()) {
        return Err(invalid("auth.json 中的 JWT 格式无效"));
    }
    let bytes = URL_SAFE_NO_PAD
        .decode(parts[1])
        .map_err(|_| invalid("JWT 内容无法解析"))?;
    let value: Value = serde_json::from_slice(&bytes).map_err(|_| invalid("JWT 内容无法解析"))?;
    if !value.is_object() {
        return Err(invalid("JWT 内容必须是对象"));
    }
    Ok(value)
}

fn workspace(value: &Value) -> Option<&str> {
    value
        .get("chatgpt_account_id")
        .and_then(Value::as_str)
        .or_else(|| {
            value
                .pointer("/https:~1~1api.openai.com~1auth/chatgpt_account_id")
                .and_then(Value::as_str)
        })
        .filter(|s| !s.trim().is_empty())
}

struct ImportedLogin {
    workspace: String,
    email: Option<String>,
    refresh: String,
    id_token: String,
    access: CachedAccessToken,
}

fn parse(content: &str) -> Result<ImportedLogin, CodexOAuthError> {
    if content.len() > MAX_AUTH_BYTES {
        return Err(invalid("auth.json 不能超过 1 MB"));
    }
    // Never include serde's input diagnostics: they can contain credential text.
    let auth: Value = serde_json::from_str(content.trim_start_matches('\u{feff}'))
        .map_err(|_| invalid("文件不是有效的 JSON"))?;
    if !auth.is_object() {
        return Err(invalid("auth.json 必须是 JSON 对象"));
    }
    if auth.get("auth_mode").is_some_and(|v| v != "chatgpt")
        || auth
            .get("OPENAI_API_KEY")
            .is_some_and(|v| !v.is_null() && v != "")
    {
        return Err(invalid(
            "请选择 ChatGPT 官方账号的 auth.json，不能导入 API Key 登录",
        ));
    }
    let tokens = auth
        .get("tokens")
        .filter(|v| v.is_object())
        .ok_or_else(|| invalid("auth.json 缺少 tokens 对象"))?;
    let id_token = required(tokens, "id_token")?;
    let access_token = required(tokens, "access_token")?;
    let refresh = required(tokens, "refresh_token")?;
    let id = claims(&id_token)?;
    let access = claims(&access_token)?;
    let subject = crate::codex_config::extract_codex_id_token_subject(&id_token)
        .ok_or_else(|| invalid("id_token 缺少有效的用户身份或 JWT 头"))?;
    if access
        .get("sub")
        .and_then(Value::as_str)
        .is_some_and(|s| s != subject)
    {
        return Err(invalid("id_token 与 access_token 的用户不一致"));
    }
    let account = workspace(&id)
        .or_else(|| workspace(&access))
        .ok_or_else(|| invalid("无法从令牌中读取 ChatGPT workspace"))?;
    for candidate in [
        workspace(&id),
        workspace(&access),
        tokens.get("account_id").and_then(Value::as_str),
    ] {
        if candidate.is_some_and(|s| s != account) {
            return Err(invalid("auth.json 中的账号与令牌 workspace 不一致"));
        }
    }
    let expires_at_ms = access
        .get("exp")
        .and_then(Value::as_i64)
        .and_then(|s| s.checked_mul(1000))
        .filter(|v| *v > 0)
        .ok_or_else(|| invalid("access_token 缺少有效的过期时间"))?;
    let now = chrono::Utc::now().timestamp_millis();
    // Preserve the original token age; importing must not make stale tokens look fresh.
    // Zero represents an unknown age when neither source timestamp is present.
    let obtained_at_ms = match auth.get("last_refresh").filter(|v| !v.is_null()) {
        Some(v) => v
            .as_str()
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|t| t.timestamp_millis())
            .ok_or_else(|| invalid("last_refresh 时间格式无效"))?,
        None => access
            .get("iat")
            .and_then(Value::as_i64)
            .and_then(|s| s.checked_mul(1000))
            .unwrap_or(0),
    };
    if obtained_at_ms < 0 || obtained_at_ms > now + 300_000 {
        return Err(invalid("令牌刷新时间不合理，请检查文件或系统时间"));
    }
    Ok(ImportedLogin {
        workspace: account.to_string(),
        email: id
            .get("email")
            .or_else(|| access.get("email"))
            .and_then(Value::as_str)
            .map(str::to_owned),
        refresh,
        id_token,
        access: CachedAccessToken {
            token: access_token,
            expires_at_ms,
            obtained_at_ms,
        },
    })
}

impl CodexOAuthManager {
    pub async fn import_auth_json(&self, content: &str) -> Result<GitHubAccount, CodexOAuthError> {
        let login = parse(content)?;
        self.add_account_internal(
            login.workspace,
            login.refresh,
            login.email,
            Some(login.id_token),
            Some(login.access),
            AccountLoginContext::default(),
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn jwt(value: Value) -> String {
        format!(
            "{}.{}.signature",
            URL_SAFE_NO_PAD.encode(br#"{"alg":"RS256"}"#),
            URL_SAFE_NO_PAD.encode(value.to_string())
        )
    }
    fn fixture(subject: &str) -> Value {
        let now = chrono::Utc::now().timestamp();
        json!({"auth_mode":"chatgpt", "OPENAI_API_KEY":null,
            "tokens": {"id_token":jwt(json!({"sub":subject,"email":"test@example.com","https://api.openai.com/auth":{"chatgpt_account_id":"team"}})),
                "access_token":jwt(json!({"sub":subject,"exp":now+3600,"iat":now-60,"https://api.openai.com/auth":{"chatgpt_account_id":"team"}})),
                "refresh_token":"test-refresh", "account_id":"team"}})
    }
    #[test]
    fn rejects_invalid_or_mixed_credentials_without_echoing_secrets() {
        for text in [
            "{secret-refresh",
            "[]",
            "{\"OPENAI_API_KEY\":\"secret-key\"}",
        ] {
            let error = parse(text).err().unwrap().to_string();
            assert!(!error.contains("secret"));
        }
        let mut value = fixture("alice");
        value["tokens"]["account_id"] = json!("other-team");
        assert!(parse(&value.to_string()).is_err());
        value = fixture("alice");
        value["tokens"]["access_token"] = fixture("bob")["tokens"]["access_token"].clone();
        assert!(parse(&value.to_string()).is_err());
        value = fixture("alice");
        value["tokens"]["refresh_token"] = json!("");
        assert!(parse(&value.to_string()).is_err());
        assert!(parse(&"x".repeat(MAX_AUTH_BYTES + 1)).is_err());
    }
    #[tokio::test]
    async fn imports_persists_and_rejects_duplicate_without_overwriting() {
        let temp = tempfile::tempdir().unwrap();
        let manager = CodexOAuthManager::new(temp.path().to_path_buf());
        let content = fixture("alice").to_string();
        let account = manager.import_auth_json(&content).await.unwrap();
        assert!(!account.reauth_required);
        assert!(matches!(
            manager.import_auth_json(&content).await,
            Err(CodexOAuthError::DuplicateAccount)
        ));
        let other = manager
            .import_auth_json(&fixture("bob").to_string())
            .await
            .unwrap();
        assert_ne!(other.id, account.id);
        assert_eq!(
            manager.get_status().await.default_account_id,
            Some(account.id.clone())
        );
        let reloaded = CodexOAuthManager::new(temp.path().to_path_buf());
        assert_eq!(reloaded.list_accounts().await.len(), 2);
        assert_eq!(
            reloaded
                .test_refresh_token_for_account(&account.id)
                .await
                .as_deref(),
            Some("test-refresh")
        );
        assert_eq!(
            manager.accounts.read().await[&account.id].token_updated_at_ms,
            parse(&content).unwrap().access.obtained_at_ms
        );
        // Exercise the same bundle reader used by official-card switching.
        for (id, subject) in [
            (&account.id, "alice"),
            (&other.id, "bob"),
            (&account.id, "alice"),
        ] {
            let bundle = manager
                .get_valid_token_bundle_for_account(id)
                .await
                .unwrap();
            assert_eq!(
                crate::codex_config::extract_codex_id_token_subject(
                    bundle.id_token.as_deref().unwrap()
                )
                .as_deref(),
                Some(subject)
            );
            assert_eq!(bundle.refresh_token, "test-refresh");
        }
    }
    #[tokio::test]
    async fn failed_write_does_not_publish_account() {
        let temp = tempfile::tempdir().unwrap();
        let manager = CodexOAuthManager::new(temp.path().to_path_buf());
        // A directory at the destination forces the atomic rename to fail.
        std::fs::create_dir(&manager.storage_path).unwrap();
        assert!(manager
            .import_auth_json(&fixture("alice").to_string())
            .await
            .is_err());
        assert!(manager.list_accounts().await.is_empty());
        assert!(manager.get_status().await.default_account_id.is_none());
    }
    #[tokio::test]
    async fn simultaneous_imports_cannot_duplicate_account() {
        let temp = tempfile::tempdir().unwrap();
        let manager = CodexOAuthManager::new(temp.path().to_path_buf());
        let content = fixture("alice").to_string();
        let (a, b) = tokio::join!(
            manager.import_auth_json(&content),
            manager.import_auth_json(&content)
        );
        assert_ne!(a.is_ok(), b.is_ok());
        assert_eq!(manager.list_accounts().await.len(), 1);
    }
    #[test]
    fn accepts_expired_access_for_refresh_and_utf8_bom() {
        let mut value = fixture("alice");
        value["tokens"]["access_token"] = json!(jwt(json!({"sub":"alice","exp":1})));
        assert!(parse(&format!("\u{feff}{value}"))
            .unwrap()
            .access
            .is_expiring_soon());
    }
}
