use crate::codex_account::{
    CodexAccount, CodexProviderBinding, CodexUsageState, CodexUsageView, ImportResult,
    LoginSession, RefreshResult,
};
use crate::database::Database;
use crate::error::AppError;
use chrono::Utc;
use serde_json::Value;
use std::path::PathBuf;
use std::sync::{Arc, LazyLock};
use tokio::sync::RwLock;

const SWITCHER_IMPORT_DONE_KEY: &str = "codex_switcher_import_done";
const USAGE_POLLER_ENABLED_KEY: &str = "codex_usage_poller_enabled";
const USAGE_ENDPOINT: &str = "https://chatgpt.com/backend-api/wham/usage";
const POLL_INTERVAL_SECS: u64 = 60;

static LOGIN_SESSIONS: LazyLock<RwLock<std::collections::HashMap<String, String>>> =
    LazyLock::new(|| RwLock::new(std::collections::HashMap::new()));

pub struct CodexUsageService;

impl CodexUsageService {
    fn decode_jwt_payload(token: &str) -> Option<Value> {
        let mut parts = token.split('.');
        let _header = parts.next()?;
        let payload = parts.next()?;
        let normalized = payload.replace('-', "+").replace('_', "/");
        let pad_len = (4 - normalized.len() % 4) % 4;
        let padded = format!("{}{}", normalized, "=".repeat(pad_len));
        let bytes = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, padded)
            .ok()?;
        serde_json::from_slice::<Value>(&bytes).ok()
    }

    fn now_ms() -> i64 {
        Utc::now().timestamp_millis()
    }

    fn is_usage_poller_enabled(db: &Database) -> bool {
        db
            .get_setting(USAGE_POLLER_ENABLED_KEY)
            .ok()
            .flatten()
            .map(|v| v == "true" || v == "1")
            .unwrap_or(true)
    }

    fn is_import_done(db: &Database) -> bool {
        db
            .get_setting(SWITCHER_IMPORT_DONE_KEY)
            .ok()
            .flatten()
            .map(|v| v == "true" || v == "1")
            .unwrap_or(false)
    }

    fn mark_import_done(db: &Database) -> Result<(), AppError> {
        db.set_setting(SWITCHER_IMPORT_DONE_KEY, "true")
    }

    fn switcher_accounts_path() -> Option<PathBuf> {
        dirs::home_dir().map(|h| h.join(".codex-switcher").join("accounts.json"))
    }

    fn parse_switcher_accounts(data: &Value) -> Vec<CodexAccount> {
        let accounts = data
            .get("accounts")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let now = Self::now_ms();
        accounts
            .into_iter()
            .filter_map(|item| {
                let id = item.get("id")?.as_str()?.to_string();
                let auth_data = item.get("auth_data").unwrap_or(&Value::Null);
                let access_token = auth_data
                    .get("access_token")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                let account_id = auth_data
                    .get("account_id")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                if access_token.is_empty() || account_id.is_empty() {
                    return None;
                }
                Some(CodexAccount {
                    id,
                    email: item.get("email").and_then(Value::as_str).map(str::to_string),
                    display_name: item.get("name").and_then(Value::as_str).map(str::to_string),
                    account_id,
                    plan_type: item
                        .get("plan_type")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    auth_mode: "chatgpt".to_string(),
                    access_token,
                    refresh_token: auth_data
                        .get("refresh_token")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    id_token: auth_data
                        .get("id_token")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    last_refresh_at: None,
                    last_used_at: item.get("last_used_at").and_then(Value::as_i64),
                    source: "codex_switcher_import".to_string(),
                    is_active: true,
                    created_at: item.get("created_at").and_then(Value::as_i64).unwrap_or(now),
                    updated_at: now,
                })
            })
            .collect()
    }

    fn parse_legacy_provider_accounts(db: &Database) -> Vec<CodexAccount> {
        let now = Self::now_ms();
        let providers = db.get_all_providers("codex").unwrap_or_default();
        providers
            .into_values()
            .filter_map(|provider| {
                let auth = provider.settings_config.get("auth")?;
                let account_id = auth
                    .get("account_id")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                let access_token = auth
                    .get("tokens")
                    .and_then(|v| v.get("access_token"))
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                if account_id.is_empty() || access_token.is_empty() {
                    return None;
                }
                Some(CodexAccount {
                    id: provider.id,
                    email: auth
                        .get("tokens")
                        .and_then(|t| t.get("email"))
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    display_name: Some(provider.name),
                    account_id,
                    plan_type: None,
                    auth_mode: auth
                        .get("auth_mode")
                        .and_then(Value::as_str)
                        .unwrap_or("chatgpt")
                        .to_string(),
                    access_token,
                    refresh_token: auth
                        .get("tokens")
                        .and_then(|v| v.get("refresh_token"))
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    id_token: auth
                        .get("tokens")
                        .and_then(|v| v.get("id_token"))
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    last_refresh_at: Some(now),
                    last_used_at: Some(now),
                    source: "legacy_provider_migration".to_string(),
                    is_active: true,
                    created_at: now,
                    updated_at: now,
                })
            })
            .collect()
    }

    fn best_match_account_for_provider(
        provider_name: &str,
        accounts: &[CodexAccount],
    ) -> Option<CodexAccount> {
        let normalized = provider_name.to_lowercase();
        accounts
            .iter()
            .find(|a| {
                a.display_name
                    .as_ref()
                    .map(|n| normalized.contains(&n.to_lowercase()))
                    .unwrap_or(false)
                    || a.email
                        .as_ref()
                        .map(|e| normalized.contains(&e.to_lowercase()))
                        .unwrap_or(false)
            })
            .cloned()
            .or_else(|| accounts.first().cloned())
    }

    pub fn import_from_switcher_once(db: &Database) -> Result<ImportResult, AppError> {
        if Self::is_import_done(db) {
            return Ok(ImportResult::default());
        }

        let Some(path) = Self::switcher_accounts_path() else {
            Self::mark_import_done(db)?;
            return Ok(ImportResult::default());
        };
        if !path.exists() {
            Self::mark_import_done(db)?;
            return Ok(ImportResult::default());
        }

        let raw = std::fs::read_to_string(&path).map_err(|e| AppError::io(path.clone(), e))?;
        let json: Value = serde_json::from_str(&raw).map_err(|e| {
            AppError::Config(format!(
                "解析 Codex Switcher 账号文件失败 ({}): {e}",
                path.display()
            ))
        })?;

        let mut accounts = Self::parse_switcher_accounts(&json);
        if accounts.is_empty() {
            accounts = Self::parse_legacy_provider_accounts(db);
        }
        if accounts.is_empty() {
            Self::mark_import_done(db)?;
            return Ok(ImportResult::default());
        }

        let mut imported = 0usize;
        let mut skipped = 0usize;
        for account in &accounts {
            let existed = db.get_codex_account_by_id(&account.id)?.is_some();
            db.upsert_codex_account(account)?;
            if existed {
                skipped += 1;
            } else {
                imported += 1;
            }
        }

        let providers = db.get_all_providers("codex")?;
        let mut bindings_updated = 0usize;
        for (_, provider) in providers {
            if let Some(acc) = Self::best_match_account_for_provider(&provider.name, &accounts) {
                let binding = CodexProviderBinding {
                    provider_id: provider.id,
                    account_id: acc.id,
                    auto_bound: true,
                    updated_at: Self::now_ms(),
                };
                db.upsert_codex_provider_binding(&binding)?;
                bindings_updated += 1;
            }
        }

        Self::mark_import_done(db)?;
        Ok(ImportResult {
            imported,
            skipped,
            bindings_updated,
        })
    }

    fn bool_opt(v: Option<&Value>) -> Option<bool> {
        v.and_then(|x| match x {
            Value::Bool(b) => Some(*b),
            Value::Number(n) => Some(n.as_i64().unwrap_or(0) != 0),
            _ => None,
        })
    }

    fn f64_opt(v: Option<&Value>) -> Option<f64> {
        v.and_then(|x| match x {
            Value::Number(n) => n.as_f64(),
            Value::String(s) => s.parse::<f64>().ok(),
            _ => None,
        })
    }

    fn i64_opt(v: Option<&Value>) -> Option<i64> {
        v.and_then(|x| match x {
            Value::Number(n) => n.as_i64(),
            Value::String(s) => s.parse::<i64>().ok(),
            _ => None,
        })
    }

    fn parse_usage(account_id: String, json: &Value) -> CodexUsageState {
        let rate_limit = json.get("rate_limit").unwrap_or(json);
        let primary = rate_limit
            .get("primary_window")
            .or_else(|| json.get("primary_window"))
            .or_else(|| json.get("primary"));
        let secondary = rate_limit
            .get("secondary_window")
            .or_else(|| json.get("secondary_window"))
            .or_else(|| json.get("secondary"));
        let credits = json.get("credits");
        let now_ms = Self::now_ms();
        let primary_used_percent = Self::f64_opt(primary.and_then(|v| v.get("used_percent")))
            .or_else(|| {
                Self::f64_opt(primary.and_then(|v| v.get("remaining_percent")))
                    .map(|remaining| (100.0 - remaining).clamp(0.0, 100.0))
            })
            .or_else(|| Self::f64_opt(json.get("primary_used_percent")));
        let secondary_used_percent = Self::f64_opt(secondary.and_then(|v| v.get("used_percent")))
            .or_else(|| {
                Self::f64_opt(secondary.and_then(|v| v.get("remaining_percent")))
                    .map(|remaining| (100.0 - remaining).clamp(0.0, 100.0))
            })
            .or_else(|| Self::f64_opt(json.get("secondary_used_percent")));

        CodexUsageState {
            account_id,
            allowed: Self::bool_opt(rate_limit.get("allowed"))
                .or_else(|| Self::bool_opt(json.get("allowed"))),
            limit_reached: Self::bool_opt(rate_limit.get("limit_reached"))
                .or_else(|| Self::bool_opt(json.get("limit_reached"))),
            primary_used_percent,
            primary_limit_window_seconds: Self::i64_opt(
                primary
                    .and_then(|v| v.get("limit_window_seconds"))
                    .or_else(|| json.get("primary_limit_window_seconds")),
            ),
            primary_reset_at: Self::i64_opt(
                primary
                    .and_then(|v| v.get("reset_at").or_else(|| v.get("resets_at")))
                    .or_else(|| json.get("primary_reset_at")),
            ),
            primary_reset_after_seconds: Self::i64_opt(
                primary
                    .and_then(|v| {
                        v.get("reset_after_seconds")
                            .or_else(|| v.get("resets_in_seconds"))
                    })
                    .or_else(|| json.get("primary_reset_after_seconds")),
            ),
            secondary_used_percent,
            secondary_limit_window_seconds: Self::i64_opt(
                secondary
                    .and_then(|v| v.get("limit_window_seconds"))
                    .or_else(|| json.get("secondary_limit_window_seconds")),
            ),
            secondary_reset_at: Self::i64_opt(
                secondary
                    .and_then(|v| v.get("reset_at").or_else(|| v.get("resets_at")))
                    .or_else(|| json.get("secondary_reset_at")),
            ),
            secondary_reset_after_seconds: Self::i64_opt(
                secondary
                    .and_then(|v| {
                        v.get("reset_after_seconds")
                            .or_else(|| v.get("resets_in_seconds"))
                    })
                    .or_else(|| json.get("secondary_reset_after_seconds")),
            ),
            credits_has_credits: Self::bool_opt(
                credits
                    .and_then(|v| v.get("has_credits"))
                    .or_else(|| json.get("credits_has_credits")),
            ),
            credits_balance: Self::f64_opt(
                credits
                    .and_then(|v| v.get("balance"))
                    .or_else(|| json.get("credits_balance")),
            ),
            credits_unlimited: Self::bool_opt(
                credits
                    .and_then(|v| v.get("unlimited"))
                    .or_else(|| json.get("credits_unlimited")),
            ),
            last_refresh_at: Some(now_ms),
            last_error: None,
        }
    }

    fn usage_cooldown_seconds(usage: &CodexUsageState) -> Option<i64> {
        if usage.allowed == Some(true) && usage.limit_reached == Some(false) {
            return Some(0);
        }
        let by_secs = usage
            .primary_reset_after_seconds
            .unwrap_or(0)
            .max(usage.secondary_reset_after_seconds.unwrap_or(0));
        if by_secs > 0 {
            return Some(by_secs);
        }
        let now = Utc::now().timestamp();
        let p = usage.primary_reset_at.unwrap_or(0) - now;
        let s = usage.secondary_reset_at.unwrap_or(0) - now;
        let fallback = p.max(s);
        (fallback > 0).then_some(fallback)
    }

    pub async fn refresh_usage_for_account(
        db: &Database,
        account: &CodexAccount,
    ) -> Result<(), AppError> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(20))
            .build()
            .map_err(|e| AppError::Message(e.to_string()))?;
        let mut req = client.get(USAGE_ENDPOINT).header(
            "authorization",
            format!("Bearer {}", account.access_token),
        );
        req = req
            .header("chatgpt-account-id", &account.account_id)
            .header("user-agent", "codex-cli/1.0.0");

        let now_ms = Self::now_ms();
        match req.send().await {
            Ok(resp) => {
                let status = resp.status();
                let body = resp
                    .text()
                    .await
                    .map_err(|e| AppError::Message(e.to_string()))?;
                if !status.is_success() {
                    let usage = CodexUsageState {
                        account_id: account.id.clone(),
                        last_refresh_at: Some(now_ms),
                        last_error: Some(format!("HTTP {}: {}", status.as_u16(), body)),
                        ..Default::default()
                    };
                    db.upsert_codex_usage_state(&usage)?;
                    return Err(AppError::Config(format!(
                        "usage refresh failed for account {}: HTTP {}",
                        account.id,
                        status.as_u16()
                    )));
                }
                let json: Value = serde_json::from_str(&body).map_err(|e| {
                    AppError::Config(format!("解析 usage 响应失败 account={} : {e}", account.id))
                })?;
                let usage = Self::parse_usage(account.id.clone(), &json);
                db.upsert_codex_usage_state(&usage)?;
            }
            Err(e) => {
                let usage = CodexUsageState {
                    account_id: account.id.clone(),
                    last_refresh_at: Some(now_ms),
                    last_error: Some(e.to_string()),
                    ..Default::default()
                };
                db.upsert_codex_usage_state(&usage)?;
                return Err(AppError::Config(format!(
                    "usage refresh request failed for account {}: {}",
                    account.id, e
                )));
            }
        }
        Ok(())
    }

    pub async fn refresh_usage_now(
        db: &Database,
        provider_id: Option<String>,
    ) -> Result<RefreshResult, AppError> {
        let mut accounts: Vec<CodexAccount> = Vec::new();
        if let Some(pid) = provider_id {
            if let Some(acc) = db.get_codex_account_by_provider(&pid)? {
                accounts.push(acc);
            } else {
                return Err(AppError::Config(format!(
                    "Codex provider 未绑定账号，无法刷新用量: {}",
                    pid
                )));
            }
        } else {
            accounts = db.list_codex_accounts(true)?;
        }
        if accounts.is_empty() {
            return Err(AppError::Config("没有可刷新的 Codex 账号".to_string()));
        }

        let mut success_accounts = 0usize;
        let mut failed_accounts = 0usize;
        for account in &accounts {
            match Self::refresh_usage_for_account(db, account).await {
                Ok(_) => success_accounts += 1,
                Err(_) => failed_accounts += 1,
            }
        }

        Ok(RefreshResult {
            refreshed_accounts: accounts.len(),
            success_accounts,
            failed_accounts,
        })
    }

    pub fn get_usage_view_by_provider(
        db: &Database,
        provider_id: &str,
    ) -> Result<CodexUsageView, AppError> {
        let binding = db.get_codex_provider_binding(provider_id)?;
        let account = db.get_codex_account_by_provider(provider_id)?;
        let usage = if let Some(acc) = &account {
            db.get_codex_usage_state(&acc.id)?
        } else {
            None
        };
        let cooldown_seconds = usage
            .as_ref()
            .and_then(Self::usage_cooldown_seconds)
            .filter(|v| *v > 0);
        let available = usage
            .as_ref()
            .map(|u| {
                u.allowed.unwrap_or(true)
                    && !u.limit_reached.unwrap_or(false)
                    && cooldown_seconds.unwrap_or(0) <= 0
            })
            .unwrap_or(true);
        Ok(CodexUsageView {
            provider_id: provider_id.to_string(),
            account,
            binding,
            usage,
            available,
            cooldown_seconds,
        })
    }

    pub async fn start_usage_poller(db: Arc<Database>) {
        if !Self::is_usage_poller_enabled(&db) {
            log::info!("codex usage poller disabled by settings");
            return;
        }
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(POLL_INTERVAL_SECS));
        let _ = Self::refresh_usage_now(&db, None).await;
        loop {
            interval.tick().await;
            if let Err(e) = Self::refresh_usage_now(&db, None).await {
                log::warn!("codex usage poller refresh failed: {e}");
            }
        }
    }

    pub async fn maybe_import_and_start(db: Arc<Database>) {
        if let Err(e) = Self::import_from_switcher_once(&db) {
            log::warn!("codex switcher auto import failed: {e}");
        }
        tokio::spawn(Self::start_usage_poller(db));
    }

    pub async fn start_login(provider_id: String) -> Result<LoginSession, AppError> {
        let session_id = uuid::Uuid::new_v4().to_string();
        LOGIN_SESSIONS
            .write()
            .await
            .insert(session_id.clone(), provider_id.clone());
        Ok(LoginSession {
            session_id,
            provider_id,
            auth_url: "https://chatgpt.com/codex".to_string(),
        })
    }

    pub async fn complete_login(
        db: &Database,
        session_id: String,
        callback_payload: String,
    ) -> Result<CodexAccount, AppError> {
        let provider_id = LOGIN_SESSIONS
            .write()
            .await
            .remove(&session_id)
            .ok_or_else(|| AppError::Config("登录会话不存在或已过期".to_string()))?;

        let payload: Value = serde_json::from_str(&callback_payload)
            .map_err(|e| AppError::Config(format!("登录回调格式错误: {e}")))?;
        let account_id = payload
            .get("account_id")
            .or_else(|| payload.get("accountId"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let access_token = payload
            .get("access_token")
            .or_else(|| payload.get("accessToken"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        if account_id.is_empty() || access_token.is_empty() {
            return Err(AppError::Config(
                "回调缺少 account_id/access_token".to_string(),
            ));
        }
        let now = Self::now_ms();
        let account = CodexAccount {
            id: payload
                .get("id")
                .and_then(Value::as_str)
                .map(str::to_string)
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
            email: payload.get("email").and_then(Value::as_str).map(str::to_string),
            display_name: payload
                .get("display_name")
                .or_else(|| payload.get("displayName"))
                .and_then(Value::as_str)
                .map(str::to_string),
            account_id,
            plan_type: payload
                .get("plan_type")
                .or_else(|| payload.get("planType"))
                .and_then(Value::as_str)
                .map(str::to_string),
            auth_mode: "chatgpt".to_string(),
            access_token,
            refresh_token: payload
                .get("refresh_token")
                .or_else(|| payload.get("refreshToken"))
                .and_then(Value::as_str)
                .map(str::to_string),
            id_token: payload
                .get("id_token")
                .or_else(|| payload.get("idToken"))
                .and_then(Value::as_str)
                .map(str::to_string),
            last_refresh_at: Some(now),
            last_used_at: Some(now),
            source: "cc_login".to_string(),
            is_active: true,
            created_at: now,
            updated_at: now,
        };
        db.upsert_codex_account(&account)?;
        let binding = CodexProviderBinding {
            provider_id,
            account_id: account.id.clone(),
            auto_bound: false,
            updated_at: now,
        };
        db.upsert_codex_provider_binding(&binding)?;
        Ok(account)
    }

    pub fn bind_from_provider_auth(
        db: &Database,
        provider_id: &str,
    ) -> Result<CodexAccount, AppError> {
        let provider = db
            .get_provider_by_id(provider_id, "codex")?
            .ok_or_else(|| AppError::Config(format!("Codex provider not found: {provider_id}")))?;
        let auth = provider
            .settings_config
            .get("auth")
            .ok_or_else(|| AppError::Config("Provider 缺少 auth 配置".to_string()))?;
        let tokens = auth
            .get("tokens")
            .and_then(Value::as_object)
            .ok_or_else(|| AppError::Config("Provider 缺少 auth.tokens".to_string()))?;
        let access_token = tokens
            .get("access_token")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        if access_token.is_empty() {
            return Err(AppError::Config("Provider tokens.access_token 为空".to_string()));
        }

        let token_payload = Self::decode_jwt_payload(&access_token).unwrap_or(Value::Null);
        let account_id = tokens
            .get("account_id")
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| {
                token_payload
                    .get("https://api.openai.com/auth")
                    .and_then(|v| v.get("chatgpt_account_id"))
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .unwrap_or_default();
        if account_id.is_empty() {
            return Err(AppError::Config(
                "Provider 登录态缺少 account_id，无法绑定".to_string(),
            ));
        }

        let now = Self::now_ms();
        let account = CodexAccount {
            id: provider_id.to_string(),
            email: token_payload
                .get("https://api.openai.com/profile")
                .and_then(|v| v.get("email"))
                .and_then(Value::as_str)
                .map(str::to_string),
            display_name: Some(provider.name.clone()),
            account_id,
            plan_type: token_payload
                .get("https://api.openai.com/auth")
                .and_then(|v| v.get("chatgpt_plan_type"))
                .and_then(Value::as_str)
                .map(str::to_string),
            auth_mode: "chatgpt".to_string(),
            access_token,
            refresh_token: tokens
                .get("refresh_token")
                .and_then(Value::as_str)
                .map(str::to_string),
            id_token: tokens
                .get("id_token")
                .and_then(Value::as_str)
                .map(str::to_string),
            last_refresh_at: Some(now),
            last_used_at: Some(now),
            source: "legacy_provider_migration".to_string(),
            is_active: true,
            created_at: now,
            updated_at: now,
        };
        db.upsert_codex_account(&account)?;
        db.upsert_codex_provider_binding(&CodexProviderBinding {
            provider_id: provider_id.to_string(),
            account_id: account.id.clone(),
            auto_bound: false,
            updated_at: now,
        })?;
        Ok(account)
    }
}
