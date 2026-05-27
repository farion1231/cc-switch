use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::config::{get_app_config_dir, get_claude_config_dir, read_json_file, write_json_file};
use crate::error::AppError;
use crate::provider::Provider;
use crate::services::subscription::{SubscriptionQuota, TIER_FIVE_HOUR, TIER_SEVEN_DAY};

pub const AUTH_PROVIDER: &str = "claude_official";
const AUTO_SWITCH_USAGE_THRESHOLD: f64 = 95.0;

#[cfg(target_os = "macos")]
const MACOS_KEYCHAIN_SERVICE: &str = "Claude Code-credentials";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeOfficialAccount {
    pub id: String,
    pub label: String,
    pub email: Option<String>,
    pub quota: Option<SubscriptionQuota>,
    #[serde(rename = "createdAt")]
    pub created_at: i64,
    #[serde(rename = "updatedAt")]
    pub updated_at: i64,
    #[serde(rename = "storageKind")]
    pub storage_kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredClaudeOfficialAccount {
    #[serde(flatten)]
    account: ClaudeOfficialAccount,
    credentials: Value,
}

pub fn activate_provider_account(provider: &Provider) -> Result<Vec<String>, AppError> {
    if let Some(account_id) = provider
        .meta
        .as_ref()
        .and_then(|meta| meta.managed_account_id_for(AUTH_PROVIDER))
    {
        return activate_best_available_account(&account_id);
    }

    if provider
        .meta
        .as_ref()
        .and_then(|meta| meta.provider_type.as_deref())
        != Some(AUTH_PROVIDER)
    {
        return Ok(Vec::new());
    }

    let accounts = list_accounts()?;
    match accounts.len() {
        0 => Err(AppError::Message(
            "Claude Official 供应商尚未绑定账号。请先完成 /login，然后保存并激活当前登录。"
                .to_string(),
        )),
        1 => activate_best_available_account(&accounts[0].id),
        _ => {
            if let Some(account) = first_available_account(None)? {
                activate_account(&account.id)
            } else {
                Err(AppError::Message(
                    "所有 Claude Official 账号的 5h/7d 用量都已接近上限，请稍后再试。".to_string(),
                ))
            }
        }
    }
}

pub fn list_accounts() -> Result<Vec<ClaudeOfficialAccount>, AppError> {
    let dir = accounts_dir();
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut accounts = Vec::new();
    for entry in fs::read_dir(&dir).map_err(|e| AppError::io(&dir, e))? {
        let entry = match entry {
            Ok(entry) => entry,
            Err(err) => {
                log::warn!("Failed to read Claude official account entry: {err}");
                continue;
            }
        };
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        match read_json_file::<StoredClaudeOfficialAccount>(&path) {
            Ok(stored) => {
                let mut account = stored.account;
                if account.email.is_none() {
                    account.email = extract_account_email(&stored.credentials);
                }
                accounts.push(account);
            }
            Err(err) => log::warn!(
                "Failed to parse Claude official account {}: {err}",
                path.display()
            ),
        }
    }

    accounts.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    Ok(accounts)
}

pub async fn capture_current_account(
    label: Option<String>,
) -> Result<ClaudeOfficialAccount, AppError> {
    let (credentials, storage_kind) = read_current_credentials()?;
    let access_token = extract_access_token(&credentials).ok_or_else(|| {
        AppError::Message(
            "已读取 Claude Code 登录凭据，但未找到 accessToken。请在终端完成 /login 后重试。"
                .to_string(),
        )
    })?;
    let profile = query_oauth_profile(&access_token).await?;
    let email = extract_account_email(&profile)
        .or_else(|| extract_account_email(&credentials))
        .ok_or_else(|| {
        AppError::Message(
            "已读取 Claude Code 登录凭据，但官方 profile 未返回登录邮箱。请在终端完成 /login 后重试。"
                .to_string(),
        )
    })?;
    let quota = query_claude_quota_for_token(&access_token).await?;
    let now = chrono::Utc::now().timestamp_millis();
    let id = uuid::Uuid::new_v4().to_string();
    let label = label
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| format!("Claude Official {}", chrono::Utc::now().format("%Y-%m-%d")));

    let account = ClaudeOfficialAccount {
        id,
        label,
        email: Some(email),
        quota: Some(quota),
        created_at: now,
        updated_at: now,
        storage_kind,
    };
    let stored = StoredClaudeOfficialAccount {
        account: account.clone(),
        credentials,
    };
    write_json_file(&account_path(&account.id), &stored)?;
    restrict_owner_read_write(&account_path(&account.id));
    let warnings = activate_account(&account.id)?;
    if !warnings.is_empty() {
        log::warn!(
            "Claude official account activation completed with warnings after capture: {}",
            warnings.join(",")
        );
    }
    Ok(account)
}

pub async fn refresh_account_quota(account_id: &str) -> Result<ClaudeOfficialAccount, AppError> {
    let mut stored = read_stored_account(account_id)?;
    let access_token = extract_access_token(&stored.credentials).ok_or_else(|| {
        AppError::Message(
            "该 Claude Official 账号快照缺少 accessToken，请重新登录保存。".to_string(),
        )
    })?;
    let profile = query_oauth_profile(&access_token).await?;
    if stored.account.email.is_none() {
        stored.account.email =
            extract_account_email(&profile).or_else(|| extract_account_email(&stored.credentials));
    }
    stored.account.quota = Some(query_claude_quota_for_token(&access_token).await?);
    stored.account.updated_at = chrono::Utc::now().timestamp_millis();
    write_json_file(&account_path(&stored.account.id), &stored)?;
    restrict_owner_read_write(&account_path(&stored.account.id));
    Ok(stored.account)
}

pub fn start_login_terminal() -> Result<(), AppError> {
    #[cfg(target_os = "macos")]
    {
        let script = r#"tell application "Terminal"
    activate
    set loginTab to do script "claude"
    delay 1.5
    do script "/login" in loginTab
end tell"#;

        let status = Command::new("osascript")
            .arg("-e")
            .arg(script)
            .status()
            .map_err(|e| AppError::Message(format!("启动 Terminal 登录 Claude Code 失败: {e}")))?;

        if status.success() {
            Ok(())
        } else {
            Err(AppError::Message(
                "启动 Terminal 登录 Claude Code 失败".to_string(),
            ))
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        Err(AppError::Message(
            "Claude 官方登录自动化目前仅支持 macOS".to_string(),
        ))
    }
}

async fn query_claude_quota_for_token(access_token: &str) -> Result<SubscriptionQuota, AppError> {
    let quota = crate::services::subscription::query_claude_quota(access_token).await;
    if !quota.success {
        return Err(AppError::Message(
            quota
                .error
                .clone()
                .or(quota.credential_message.clone())
                .unwrap_or_else(|| "Claude 官方用量查询失败，请完成 /login 后重试。".to_string()),
        ));
    }

    let has_expected_window = quota.tiers.iter().any(|tier| {
        tier.name == TIER_FIVE_HOUR
            || tier.name == TIER_SEVEN_DAY
            || tier.name == crate::services::subscription::TIER_SEVEN_DAY_OPUS
            || tier.name == crate::services::subscription::TIER_SEVEN_DAY_SONNET
    });
    if !has_expected_window {
        return Err(AppError::Message(
            "Claude 官方用量查询成功，但未返回 5小时或7天窗口信息。".to_string(),
        ));
    }

    Ok(quota)
}

fn activate_best_available_account(preferred_account_id: &str) -> Result<Vec<String>, AppError> {
    let preferred = match refresh_account_quota_blocking(preferred_account_id) {
        Ok(account) => Some(account),
        Err(err) => {
            log::warn!(
                "Preferred Claude official account {preferred_account_id} unavailable, trying fallback accounts: {err}"
            );
            None
        }
    };

    if let Some(preferred) = preferred.as_ref() {
        if !is_quota_near_limit(preferred.quota.as_ref()) {
            return activate_account(&preferred.id);
        }
    }

    if let Some(account) = first_available_account(Some(preferred_account_id))? {
        let mut warnings = activate_account(&account.id)?;
        warnings.push(format!(
            "claude_official_auto_switched:{}",
            account.email.as_deref().unwrap_or(account.id.as_str())
        ));
        return Ok(warnings);
    }

    Err(AppError::Message(
        if preferred.is_some() {
            "当前 Claude Official 账号 5h/7d 用量已接近 95%，且没有找到可自动切换的可用账号。"
        } else {
            "绑定的 Claude Official 账号快照不可用，且没有找到可自动切换的可用账号。"
        }
        .to_string(),
    ))
}

fn first_available_account(
    exclude_account_id: Option<&str>,
) -> Result<Option<ClaudeOfficialAccount>, AppError> {
    for account in list_accounts()? {
        if exclude_account_id == Some(account.id.as_str()) {
            continue;
        }

        match refresh_account_quota_blocking(&account.id) {
            Ok(refreshed) if !is_quota_near_limit(refreshed.quota.as_ref()) => {
                return Ok(Some(refreshed));
            }
            Ok(_) => {}
            Err(err) => {
                log::warn!(
                    "Failed to refresh Claude official account {} during auto-switch: {err}",
                    account.id
                );
            }
        }
    }

    Ok(None)
}

fn refresh_account_quota_blocking(account_id: &str) -> Result<ClaudeOfficialAccount, AppError> {
    match tokio::runtime::Handle::try_current() {
        Ok(handle) => {
            let account_id = account_id.to_string();
            std::thread::spawn(move || handle.block_on(refresh_account_quota(&account_id)))
                .join()
                .map_err(|_| {
                    AppError::Message("刷新 Claude 官方账号用量时线程异常退出".to_string())
                })?
        }
        Err(_) => {
            let runtime = tokio::runtime::Runtime::new()
                .map_err(|e| AppError::Message(format!("无法创建用量查询运行时: {e}")))?;
            runtime.block_on(refresh_account_quota(account_id))
        }
    }
}

fn is_quota_near_limit(quota: Option<&SubscriptionQuota>) -> bool {
    quota
        .filter(|quota| quota.success)
        .map(|quota| {
            quota.tiers.iter().any(|tier| {
                (tier.name == TIER_FIVE_HOUR || tier.name.starts_with(TIER_SEVEN_DAY))
                    && tier.utilization >= AUTO_SWITCH_USAGE_THRESHOLD
            })
        })
        .unwrap_or(true)
}

async fn query_oauth_profile(access_token: &str) -> Result<Value, AppError> {
    let resp = crate::proxy::http_client::get()
        .get("https://api.anthropic.com/api/oauth/profile")
        .header("Authorization", format!("Bearer {access_token}"))
        .header("anthropic-beta", "oauth-2025-04-20")
        .header("User-Agent", "claude-code/2.0.0")
        .header("Accept", "application/json")
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
        .map_err(|e| AppError::Message(format!("查询 Claude 官方账号信息失败: {e}")))?;

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(AppError::Message(format!(
            "查询 Claude 官方账号信息失败 (HTTP {status}): {body}"
        )));
    }

    resp.json::<Value>()
        .await
        .map_err(|e| AppError::Message(format!("解析 Claude 官方账号信息失败: {e}")))
}

pub fn remove_account(account_id: &str) -> Result<(), AppError> {
    let path = account_path(account_id);
    if path.exists() {
        fs::remove_file(&path).map_err(|e| AppError::io(&path, e))?;
    }
    Ok(())
}

pub fn activate_account(account_id: &str) -> Result<Vec<String>, AppError> {
    let account_id = account_id.trim();
    if account_id.is_empty() {
        return Err(AppError::Message(
            "Claude Official 账号 ID 为空。请先保存并选择一个官方账号快照。".to_string(),
        ));
    }

    let stored = read_stored_account(account_id)?;
    let credentials_path = credentials_file_path();
    write_json_file(&credentials_path, &stored.credentials)?;
    restrict_owner_read_write(&credentials_path);

    let mut warnings = Vec::new();
    #[cfg(target_os = "macos")]
    if let Err(err) = write_macos_keychain_credentials(&stored.credentials) {
        log::warn!("Failed to update Claude Code keychain credentials: {err}");
        warnings.push("macos_keychain_update_failed".to_string());
    }

    Ok(warnings)
}

fn read_stored_account(account_id: &str) -> Result<StoredClaudeOfficialAccount, AppError> {
    let account_id = account_id.trim();
    if account_id.is_empty() {
        return Err(AppError::Message(
            "Claude Official 账号 ID 为空。请先保存并选择一个官方账号快照。".to_string(),
        ));
    }
    read_json_file::<StoredClaudeOfficialAccount>(&account_path(account_id))
}

fn accounts_dir() -> PathBuf {
    get_app_config_dir().join("claude-official-accounts")
}

fn account_path(account_id: &str) -> PathBuf {
    accounts_dir().join(format!("{account_id}.json"))
}

fn credentials_file_path() -> PathBuf {
    get_claude_config_dir().join(".credentials.json")
}

fn read_current_credentials() -> Result<(Value, String), AppError> {
    #[cfg(target_os = "macos")]
    {
        match read_macos_keychain_credentials() {
            Ok(value) => return Ok((value, "macos_keychain".to_string())),
            Err(err) => {
                log::warn!(
                    "Claude Code keychain credentials unavailable, trying file fallback: {err}"
                );
            }
        }
    }

    let path = credentials_file_path();
    let credentials = read_json_file::<Value>(&path).map_err(|err| {
        AppError::Message(format!(
            "未找到 Claude Code 官方登录凭据。请先在 Claude Code 中运行 /login，或确认 {} 存在。原始错误: {err}",
            path.display()
        ))
    })?;
    Ok((credentials, "credentials_file".to_string()))
}

fn extract_account_email(credentials: &Value) -> Option<String> {
    match credentials {
        Value::Object(map) => {
            for (key, value) in map {
                if key.to_ascii_lowercase().contains("email") {
                    if let Some(email) = value.as_str().and_then(normalize_email) {
                        return Some(email);
                    }
                }
            }

            for value in map.values() {
                if let Some(email) = extract_account_email(value) {
                    return Some(email);
                }
            }
        }
        Value::Array(values) => {
            for value in values {
                if let Some(email) = extract_account_email(value) {
                    return Some(email);
                }
            }
        }
        Value::String(value) => {
            if let Some(email) = extract_email_from_jwt(value) {
                return Some(email);
            }
        }
        _ => {}
    }

    None
}

fn extract_access_token(credentials: &Value) -> Option<String> {
    credentials
        .get("claudeAiOauth")
        .or_else(|| credentials.get("claude.ai_oauth"))
        .and_then(|entry| entry.get("accessToken"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(ToString::to_string)
}

fn extract_email_from_jwt(token: &str) -> Option<String> {
    let mut parts = token.split('.');
    parts.next()?;
    let payload = parts.next()?;
    parts.next()?;

    let decoded = URL_SAFE_NO_PAD.decode(payload.as_bytes()).ok()?;
    let value = serde_json::from_slice::<Value>(&decoded).ok()?;
    extract_account_email(&value)
}

fn normalize_email(value: &str) -> Option<String> {
    let email = value.trim().to_ascii_lowercase();
    if is_plausible_email(&email) {
        Some(email)
    } else {
        None
    }
}

fn is_plausible_email(value: &str) -> bool {
    let Some((local, domain)) = value.split_once('@') else {
        return false;
    };

    !local.is_empty()
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && !value.chars().any(char::is_whitespace)
        && value.len() <= 320
}

#[cfg(target_os = "macos")]
fn read_macos_keychain_credentials() -> Result<Value, AppError> {
    let mut command = Command::new("security");
    command.args(["find-generic-password", "-s", MACOS_KEYCHAIN_SERVICE, "-w"]);

    let output = command.output().map_err(|e| {
        AppError::Message(format!(
            "读取 macOS Keychain 中的 Claude Code 凭据失败: {e}"
        ))
    })?;
    if !output.status.success() {
        return Err(AppError::Message(
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        ));
    }

    let text = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str(text.trim())
        .map_err(|e| AppError::Message(format!("Claude Code Keychain 凭据不是有效 JSON: {e}")))
}

#[cfg(target_os = "macos")]
fn write_macos_keychain_credentials(credentials: &Value) -> Result<(), AppError> {
    let account = std::env::var("USER").unwrap_or_else(|_| "Claude Code".to_string());
    let secret = serde_json::to_string(credentials)
        .map_err(|e| AppError::Message(format!("序列化 Claude Code 凭据失败: {e}")))?;

    let output = Command::new("security")
        .args([
            "add-generic-password",
            "-U",
            "-a",
            &account,
            "-s",
            MACOS_KEYCHAIN_SERVICE,
            "-w",
            &secret,
        ])
        .output()
        .map_err(|e| AppError::Message(format!("写入 macOS Keychain 失败: {e}")))?;

    if !output.status.success() {
        return Err(AppError::Message(
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        ));
    }

    Ok(())
}

fn restrict_owner_read_write(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Err(err) = fs::set_permissions(path, fs::Permissions::from_mode(0o600)) {
            log::warn!(
                "Failed to set 0600 permissions on {}: {err}",
                path.display()
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::ProviderMeta;
    use serde_json::json;

    #[test]
    fn provider_without_claude_official_binding_is_noop() {
        let provider = Provider {
            id: "official".to_string(),
            name: "Claude Official".to_string(),
            settings_config: json!({ "env": {} }),
            website_url: None,
            category: Some("official".to_string()),
            created_at: None,
            sort_index: None,
            notes: None,
            meta: Some(ProviderMeta::default()),
            icon: None,
            icon_color: None,
            in_failover_queue: false,
        };

        let warnings = activate_provider_account(&provider).expect("noop should succeed");
        assert!(warnings.is_empty());
    }

    #[test]
    fn extract_account_email_reads_nested_email_field() {
        let credentials = json!({
            "claudeAiOauth": {
                "account": {
                    "email": "USER@Example.COM "
                }
            }
        });

        assert_eq!(
            extract_account_email(&credentials).as_deref(),
            Some("user@example.com")
        );
    }

    #[test]
    fn extract_access_token_reads_claude_oauth_entry() {
        let credentials = json!({
            "claudeAiOauth": {
                "accessToken": " token-123 "
            }
        });

        assert_eq!(
            extract_access_token(&credentials).as_deref(),
            Some("token-123")
        );
    }
}
