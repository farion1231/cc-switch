//! Optional ChatGPT membership metadata. Token expiry and quota reset times are
//! deliberately never used as membership dates. No credentials are persisted.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexMembership {
    pub plan_type: Option<String>,
    /// Membership validity, not necessarily the next automatic charge date.
    pub active_until: Option<String>,
}

fn text(value: Option<&Value>) -> Option<String> {
    value?
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

fn date(value: Option<&Value>) -> Option<String> {
    let value = value?;
    let parsed = if let Some(s) = value.as_str() {
        if let Ok(at) = chrono::DateTime::parse_from_rfc3339(s.trim()) {
            return Some(at.with_timezone(&chrono::Utc).to_rfc3339());
        }
        s.trim().parse::<i64>().ok()?
    } else {
        value.as_i64()?
    };
    // Explicit subscription claims can be ISO strings or Unix seconds/milliseconds.
    let at = if parsed >= 100_000_000_000 {
        chrono::DateTime::from_timestamp_millis(parsed)
    } else {
        chrono::DateTime::from_timestamp(parsed, 0)
    }?;
    (parsed > 0).then(|| at.to_rfc3339())
}

fn claims(token: &str) -> Option<Value> {
    let payload = token.split('.').nth(1)?;
    let bytes = URL_SAFE_NO_PAD.decode(payload.trim_end_matches('=')).ok()?;
    serde_json::from_slice::<Value>(&bytes)
        .ok()?
        .get("https://api.openai.com/auth")
        .cloned()
}

fn from_claims(auth: &Value) -> CodexMembership {
    CodexMembership {
        plan_type: text(auth.get("chatgpt_plan_type")),
        active_until: date(auth.get("chatgpt_subscription_active_until")),
    }
}

fn from_response(body: &Value, account_id: &str) -> Option<CodexMembership> {
    // Never attach another workspace's metadata to this account.
    if text(body.get("account_id")).is_some_and(|id| id != account_id) {
        return None;
    }
    let membership = CodexMembership {
        plan_type: text(body.get("subscription_plan")).or_else(|| text(body.get("plan_type"))),
        active_until: date(body.get("active_until")).or_else(|| date(body.get("expires_at"))),
    };
    (membership.plan_type.is_some() || membership.active_until.is_some()).then_some(membership)
}

pub(super) async fn query(token: &str, account_id: Option<&str>) -> Option<CodexMembership> {
    let auth = claims(token);
    let claimed_id = auth
        .as_ref()
        .and_then(|auth| text(auth.get("chatgpt_account_id")));
    if let (Some(requested), Some(claimed)) = (account_id, claimed_id.as_deref()) {
        if requested != claimed {
            return None;
        }
    }
    let mut membership = auth.as_ref().map(from_claims).unwrap_or(CodexMembership {
        plan_type: None,
        active_until: None,
    });
    let current = membership
        .active_until
        .as_ref()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .is_some_and(|at| at > chrono::Utc::now());
    if !current {
        if let Some(id) = account_id
            .or(claimed_id.as_deref())
            .filter(|id| !id.trim().is_empty())
        {
            if let Some(fetched) = fetch(token, id).await {
                membership.plan_type = fetched.plan_type.or(membership.plan_type);
                membership.active_until = fetched.active_until.or(membership.active_until);
            }
        }
    }
    (membership.plan_type.is_some() || membership.active_until.is_some()).then_some(membership)
}

fn from_account_check(body: &Value, account_id: &str) -> Option<CodexMembership> {
    let accounts = body.get("accounts")?.as_object()?;
    for (key, record) in accounts {
        let account = record.get("account").unwrap_or(record);
        let id = text(account.get("account_id")).or_else(|| text(account.get("id")));
        if id.as_deref().unwrap_or(key) != account_id {
            continue;
        }
        let entitlement = record.get("entitlement").unwrap_or(&Value::Null);
        let membership = CodexMembership {
            plan_type: text(entitlement.get("subscription_plan"))
                .or_else(|| text(account.get("plan_type"))),
            active_until: date(entitlement.get("expires_at")),
        };
        return (membership.plan_type.is_some() || membership.active_until.is_some())
            .then_some(membership);
    }
    None
}

async fn fetch(token: &str, account_id: &str) -> Option<CodexMembership> {
    fetch_from(
        &crate::proxy::http_client::get(),
        "https://chatgpt.com",
        token,
        account_id,
    )
    .await
}

async fn fetch_from(
    client: &reqwest::Client,
    origin: &str,
    token: &str,
    account_id: &str,
) -> Option<CodexMembership> {
    // Bound both fallback requests together; errors never propagate into quota.
    tokio::time::timeout(std::time::Duration::from_secs(8), async {
        let mut membership = request(
            client,
            origin,
            token,
            account_id,
            "/backend-api/accounts/check/v4-2023-04-27",
        )
        .await
        .and_then(|body| from_account_check(&body, account_id));
        if membership
            .as_ref()
            .and_then(|m| m.active_until.as_ref())
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .is_some_and(|at| at > chrono::Utc::now())
        {
            return membership;
        }
        if let Some(fetched) = request(
            client,
            origin,
            token,
            account_id,
            "/backend-api/subscriptions",
        )
        .await
        .and_then(|body| from_response(&body, account_id))
        {
            if let Some(existing) = membership.as_mut() {
                existing.plan_type = fetched.plan_type.or(existing.plan_type.take());
                existing.active_until = fetched.active_until;
            } else {
                membership = Some(fetched);
            }
        }
        membership
    })
    .await
    .ok()
    .flatten()
}

async fn request(
    client: &reqwest::Client,
    origin: &str,
    token: &str,
    account_id: &str,
    path: &str,
) -> Option<Value> {
    // No response bodies or credentials in logs. Account identity is explicit in
    // both requests; accounts/check must not fall back to a default workspace.
    let response = client
        .get(format!("{origin}{path}"))
        .query(&[("account_id", account_id)])
        .bearer_auth(token)
        .header("ChatGPT-Account-Id", account_id)
        .header("Accept", "application/json")
        .header("Referer", "https://chatgpt.com/")
        .header("x-openai-target-path", path)
        .header("x-openai-target-route", path)
        .header("User-Agent", "codex-cli")
        .timeout(std::time::Duration::from_secs(4))
        .send()
        .await
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    response.json::<Value>().await.ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn explicit_membership_claim_only() {
        let auth = json!({"chatgpt_plan_type":"plus", "exp": 4102444800i64,
            "reset_at":4102444800i64});
        assert!(from_claims(&auth).active_until.is_none());
        let auth = json!({"chatgpt_subscription_active_until":"2030-01-01T00:00:00Z"});
        assert_eq!(
            from_claims(&auth).active_until.as_deref(),
            Some("2030-01-01T00:00:00+00:00")
        );
    }

    #[test]
    fn membership_dates_accept_iso_seconds_and_milliseconds() {
        let expected = date(Some(&json!(1893456000)));
        assert_eq!(expected, date(Some(&json!(1893456000000i64))));
        assert_eq!(expected, date(Some(&json!("1893456000"))));
        assert_eq!(expected, date(Some(&json!("2030-01-01T00:00:00Z"))));
        for invalid in [
            json!(null),
            json!(true),
            json!(-1),
            json!("bad"),
            json!(""),
            json!({}),
        ] {
            assert!(date(Some(&invalid)).is_none());
        }
    }

    #[test]
    fn response_is_scoped_and_never_uses_token_expiry() {
        let body = json!({"account_id":"other", "active_until":1893456000});
        assert!(from_response(&body, "selected").is_none());
        assert!(
            from_response(&json!({"exp":1893456000,"reset_at":1893456000}), "selected").is_none()
        );
        let body =
            json!({"subscription_plan":"plus", "active_until":"bad", "expires_at":1893456000});
        let membership = from_response(&body, "selected").unwrap();
        assert_eq!(membership.plan_type.as_deref(), Some("plus"));
        assert!(membership.active_until.is_some());
    }

    #[test]
    fn account_check_never_selects_default_or_another_paid_workspace() {
        let body = json!({"accounts": {
            "personal": {"account":{"id":"personal"}, "entitlement": {
                "subscription_plan":"plus", "expires_at":"2030-01-01T00:00:00Z"}},
            "default": {"account":{"id":"team", "is_default":true}, "entitlement": {
                "subscription_plan":"team", "expires_at":"2031-01-01T00:00:00Z"}}
        }});
        let membership = from_account_check(&body, "personal").unwrap();
        assert_eq!(membership.plan_type.as_deref(), Some("plus"));
        assert_eq!(
            membership.active_until.as_deref(),
            Some("2030-01-01T00:00:00+00:00")
        );
        assert!(from_account_check(&body, "missing").is_none());
        assert!(from_account_check(&json!({"accounts":[]}), "personal").is_none());
    }

    async fn serve(router: axum::Router) -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        (origin, task)
    }

    #[tokio::test]
    async fn http_fallback_uses_selected_account_in_header_and_query() {
        use axum::{extract::Query, http::HeaderMap, routing::get, Json, Router};
        let router = Router::new()
            .route("/backend-api/accounts/check/v4-2023-04-27", get(|| async {
                Json(json!({"accounts":{"other":{"account":{"id":"other"},
                    "entitlement":{"expires_at":"2090-01-01T00:00:00Z"}}}}))
            }))
            .route("/backend-api/subscriptions", get(|headers: HeaderMap, Query(query): Query<std::collections::HashMap<String,String>>| async move {
                assert_eq!(headers.get("ChatGPT-Account-Id").unwrap(), "selected");
                assert_eq!(query.get("account_id").unwrap(), "selected");
                Json(json!({"subscription_plan":"plus","active_until":"2030-01-01T00:00:00Z"}))
            }));
        let (origin, task) = serve(router).await;
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let membership = fetch_from(&client, &origin, "fixture", "selected")
            .await
            .unwrap();
        task.abort();
        assert_eq!(
            membership.active_until.as_deref(),
            Some("2030-01-01T00:00:00+00:00")
        );
    }

    #[tokio::test]
    async fn http_errors_and_invalid_json_are_optional() {
        use axum::{http::StatusCode, routing::get, Router};
        let router = Router::new()
            .route(
                "/backend-api/accounts/check/v4-2023-04-27",
                get(|| async { StatusCode::FORBIDDEN }),
            )
            .route(
                "/backend-api/subscriptions",
                get(|| async { "<html>unavailable</html>" }),
            );
        let (origin, task) = serve(router).await;
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        assert!(fetch_from(&client, &origin, "fixture", "selected")
            .await
            .is_none());
        task.abort();
    }

    #[tokio::test]
    async fn mismatched_account_does_not_fetch_or_return_claims() {
        let auth = json!({"https://api.openai.com/auth": {
            "chatgpt_account_id":"other", "chatgpt_plan_type":"plus",
            "chatgpt_subscription_active_until":"2090-01-01T00:00:00Z"}});
        let token = format!(
            "header.{}.signature",
            URL_SAFE_NO_PAD.encode(auth.to_string())
        );
        assert!(query(&token, Some("selected")).await.is_none());
        assert_eq!(
            query(&token, Some("other"))
                .await
                .unwrap()
                .plan_type
                .as_deref(),
            Some("plus")
        );
        assert!(query("malformed", None).await.is_none());
    }
}
