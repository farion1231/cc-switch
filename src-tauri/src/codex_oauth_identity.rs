//! ChatGPT OAuth identity from Codex JWT claims.
//!
//! Official Codex treats `chatgpt_account_id` as the workspace id
//! (`ChatGPT-Account-Id`). Team members share that value, so account-manager
//! storage must use a user-scoped id instead.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde_json::Value;

/// User-scoped storage key plus the workspace id Codex writes to auth.json.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexOAuthIdentity {
    pub storage_id: String,
    pub workspace_id: Option<String>,
    pub email: Option<String>,
}

fn jwt_payload(token: &str) -> Option<Value> {
    let mut parts = token.split('.');
    let (_header, payload, _sig) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some() {
        return None;
    }
    serde_json::from_slice(&URL_SAFE_NO_PAD.decode(payload).ok()?).ok()
}

fn claim_str(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).and_then(|raw| {
        let trimmed = raw.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_string())
    })
}

fn auth_claim(payload: &Value) -> Option<&Value> {
    payload.get("https://api.openai.com/auth")
}

fn profile_claim(payload: &Value) -> Option<&Value> {
    payload.get("https://api.openai.com/profile")
}

fn org_workspace_id(payload: &Value) -> Option<String> {
    payload
        .get("organizations")
        .and_then(Value::as_array)
        .and_then(|orgs| orgs.first())
        .and_then(|org| claim_str(org, "id"))
}

fn identity_from_payload(payload: &Value) -> CodexOAuthIdentity {
    let auth = auth_claim(payload);
    let profile = profile_claim(payload);
    let workspace_id = [
        claim_str(payload, "chatgpt_account_id"),
        auth.and_then(|auth| claim_str(auth, "chatgpt_account_id")),
        org_workspace_id(payload),
    ]
    .into_iter()
    .flatten()
    .next();
    let storage_id = [
        claim_str(payload, "chatgpt_user_id"),
        auth.and_then(|auth| claim_str(auth, "chatgpt_user_id")),
        claim_str(payload, "email"),
        auth.and_then(|auth| claim_str(auth, "email")),
        profile.and_then(|profile| claim_str(profile, "email")),
        claim_str(payload, "sub"),
    ]
    .into_iter()
    .flatten()
    .next()
    .unwrap_or_default();
    let email = [
        claim_str(payload, "email"),
        auth.and_then(|auth| claim_str(auth, "email")),
        profile.and_then(|profile| claim_str(profile, "email")),
    ]
    .into_iter()
    .flatten()
    .next();
    CodexOAuthIdentity {
        storage_id,
        workspace_id,
        email,
    }
}

pub fn extract_identity_from_jwt(token: &str) -> Option<CodexOAuthIdentity> {
    let identity = identity_from_payload(&jwt_payload(token)?);
    if identity.storage_id.is_empty() && identity.workspace_id.is_none() && identity.email.is_none()
    {
        return None;
    }
    Some(identity)
}

/// Prefer `id_token`, then `access_token`. Never key accounts by workspace id.
pub fn extract_identity_from_oauth_tokens(
    id_token: Option<&str>,
    access_token: &str,
) -> Option<CodexOAuthIdentity> {
    [id_token, Some(access_token)]
        .into_iter()
        .flatten()
        .find_map(|token| {
            extract_identity_from_jwt(token).filter(|identity| !identity.storage_id.is_empty())
        })
}

/// Workspace id for `tokens.account_id` / `ChatGPT-Account-Id`.
pub fn workspace_id_from_tokens(
    id_token: Option<&str>,
    access_token: Option<&str>,
    fallback: &str,
) -> String {
    [id_token, access_token]
        .into_iter()
        .flatten()
        .find_map(|token| extract_identity_from_jwt(token)?.workspace_id)
        .filter(|id| !id.is_empty())
        .unwrap_or_else(|| fallback.trim().to_string())
}

/// Match live auth.json to a managed user. Team members share `tokens.account_id`.
pub fn live_auth_matches_managed_account(auth: &Value, storage_id: &str) -> bool {
    let storage_id = storage_id.trim();
    if storage_id.is_empty() || auth.get("auth_mode").and_then(Value::as_str) != Some("chatgpt") {
        return false;
    }
    let Some(tokens) = auth.get("tokens").and_then(Value::as_object) else {
        return false;
    };
    for token in ["id_token", "access_token"]
        .into_iter()
        .filter_map(|key| tokens.get(key).and_then(Value::as_str))
    {
        if extract_identity_from_jwt(token).is_some_and(|id| id.storage_id == storage_id) {
            return true;
        }
    }
    // Legacy accounts were keyed by the shared workspace id. Keep that match so
    // existing Team logins can still adopt/clear live auth.json.
    tokens
        .get("account_id")
        .and_then(Value::as_str)
        .is_some_and(|id| id.trim() == storage_id)
}

#[cfg(test)]
pub(crate) fn encode_unsigned_jwt(payload: &str) -> String {
    let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"none"}"#);
    format!("{header}.{}.", URL_SAFE_NO_PAD.encode(payload.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn prefers_user_id_and_keeps_workspace_for_headers() {
        let jwt = encode_unsigned_jwt(
            r#"{"https://api.openai.com/auth":{"chatgpt_account_id":"org-team","chatgpt_user_id":"user-a"},"email":"a@example.com"}"#,
        );
        let identity = extract_identity_from_oauth_tokens(None, &jwt).unwrap();
        assert_eq!(identity.storage_id, "user-a");
        assert_eq!(identity.workspace_id.as_deref(), Some("org-team"));
        assert_eq!(identity.email.as_deref(), Some("a@example.com"));
        assert_eq!(
            workspace_id_from_tokens(Some(&jwt), None, "user-a"),
            "org-team"
        );
    }

    #[test]
    fn live_auth_does_not_match_teammate_via_shared_workspace() {
        let jwt_a =
            encode_unsigned_jwt(r#"{"chatgpt_account_id":"org-team","chatgpt_user_id":"user-a"}"#);
        let auth = json!({
            "auth_mode": "chatgpt",
            "tokens": {
                "account_id": "org-team",
                "id_token": jwt_a,
                "access_token": "access"
            }
        });
        assert!(live_auth_matches_managed_account(&auth, "user-a"));
        assert!(!live_auth_matches_managed_account(&auth, "user-b"));
        // Legacy Team accounts were stored under the shared workspace id.
        assert!(live_auth_matches_managed_account(&auth, "org-team"));
    }
}
