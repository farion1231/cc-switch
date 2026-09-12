//! Native Sub2API account usage query.

use crate::provider::{UsageData, UsageResult};
use serde::Deserialize;
use std::time::Duration;

#[derive(Deserialize)]
struct ApiEnvelope<T> {
    code: i64,
    message: Option<String>,
    data: Option<T>,
}

#[derive(Deserialize)]
struct LoginData {
    access_token: String,
    refresh_token: Option<String>,
    user: LoginUser,
}

#[derive(Deserialize)]
struct TwoFactorLogin {
    requires_2fa: bool,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum LoginResponseData {
    Authenticated(LoginData),
    TwoFactor(TwoFactorLogin),
}

#[derive(Deserialize)]
struct LoginUser {
    balance: f64,
    status: String,
}

#[derive(Deserialize)]
struct UsageStats {
    total_actual_cost: f64,
}

fn failure(message: impl Into<String>) -> UsageResult {
    UsageResult {
        success: false,
        data: None,
        error: Some(message.into()),
    }
}

async fn parse_response<T: for<'de> Deserialize<'de>>(
    response: reqwest::Response,
) -> Result<Result<T, UsageResult>, String> {
    let status = response.status();
    let raw = response
        .bytes()
        .await
        .map_err(|error| format!("Failed to read Sub2API response: {error}"))?;
    if !status.is_success() {
        return Ok(Err(failure(format!("Sub2API API error (HTTP {status})"))));
    }

    let envelope: ApiEnvelope<T> = match serde_json::from_slice(&raw) {
        Ok(value) => value,
        Err(error) => {
            return Ok(Err(failure(format!(
                "Failed to parse Sub2API response: {error}"
            ))))
        }
    };
    if envelope.code != 0 {
        return Ok(Err(failure(
            envelope
                .message
                .unwrap_or_else(|| "Sub2API request failed".to_string()),
        )));
    }

    Ok(envelope
        .data
        .ok_or_else(|| failure("Sub2API response is missing data")))
}

pub async fn get_usage(
    base_url: &str,
    email: &str,
    password: &str,
    timeout_secs: u64,
) -> Result<UsageResult, String> {
    let base_url = base_url.trim().trim_end_matches('/');
    if base_url.is_empty() {
        return Ok(failure("Sub2API base URL is required"));
    }
    if email.trim().is_empty() {
        return Ok(failure("Sub2API account email is required"));
    }
    if password.is_empty() {
        return Ok(failure("Sub2API account password is required"));
    }

    let client = crate::proxy::http_client::get();
    let timeout = Duration::from_secs(timeout_secs.clamp(2, 30));

    let login_response = client
        .post(format!("{base_url}/api/v1/auth/login"))
        .json(&serde_json::json!({"email": email, "password": password}))
        .timeout(timeout)
        .send()
        .await
        .map_err(|error| format!("Failed to connect to Sub2API: {error}"))?;
    let login = match parse_response::<LoginResponseData>(login_response).await? {
        Ok(LoginResponseData::Authenticated(data)) => data,
        Ok(LoginResponseData::TwoFactor(data)) if data.requires_2fa => {
            return Ok(failure(
                "Sub2API accounts requiring interactive 2FA are not supported",
            ))
        }
        Ok(LoginResponseData::TwoFactor(_)) => {
            return Ok(failure("Sub2API login response is missing access tokens"))
        }
        Err(result) => return Ok(result),
    };

    let usage_result = match client
        .get(format!("{base_url}/api/v1/usage/dashboard/stats"))
        .bearer_auth(&login.access_token)
        .timeout(timeout)
        .send()
        .await
    {
        Ok(response) => parse_response::<UsageStats>(response).await,
        Err(error) => Err(format!("Failed to query Sub2API usage: {error}")),
    };

    if let Some(refresh_token) = &login.refresh_token {
        let _ = client
            .post(format!("{base_url}/api/v1/auth/logout"))
            .bearer_auth(&login.access_token)
            .json(&serde_json::json!({"refresh_token": refresh_token}))
            .timeout(timeout)
            .send()
            .await;
    }

    let usage = match usage_result? {
        Ok(data) => data,
        Err(result) => return Ok(result),
    };

    let remaining = login.user.balance;
    let used = usage.total_actual_cost;
    Ok(UsageResult {
        success: true,
        data: Some(vec![UsageData {
            plan_name: None,
            remaining: Some(remaining),
            total: None,
            used: Some(used),
            unit: Some("USD".to_string()),
            is_valid: Some(login.user.status == "active"),
            invalid_message: None,
            extra: None,
        }]),
        error: None,
    })
}
