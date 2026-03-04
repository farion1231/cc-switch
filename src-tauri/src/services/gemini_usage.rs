use crate::codex_account::RefreshResult;
use crate::database::Database;
use crate::error::AppError;
use crate::gemini_account::{
    GeminiAccount, GeminiLoginSession, GeminiLoginStatus, GeminiPoolStatus, GeminiProviderBinding,
    GeminiUsageState, GeminiUsageView,
};
use chrono::{DateTime, Utc};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

const CLI_LOGIN_TTL_SECONDS: i64 = 30 * 60;
const STATUS_PENDING: &str = "pending";
const STATUS_AUTHORIZED: &str = "authorized";
const STATUS_FAILED: &str = "failed";
const STATUS_EXPIRED: &str = "expired";
const STATUS_CANCELLED: &str = "cancelled";

static CLI_LOGIN_SESSIONS: LazyLock<Mutex<HashMap<String, GeminiLoginRuntime>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

#[derive(Debug, Clone)]
struct GeminiLoginRuntime {
    provider_id: String,
    updated_at_ms: i64,
    expires_at_ms: i64,
    status: String,
    message: Option<String>,
    isolated_home: PathBuf,
}

pub struct GeminiUsageService;

impl GeminiUsageService {
    fn now_ms() -> i64 {
        Utc::now().timestamp_millis()
    }

    fn now_secs() -> i64 {
        Utc::now().timestamp()
    }

    fn session_gemini_dir(isolated_home: &Path) -> PathBuf {
        isolated_home.join(".gemini")
    }

    fn session_cred_paths(isolated_home: &Path) -> (PathBuf, PathBuf) {
        let gemini_dir = Self::session_gemini_dir(isolated_home);
        (
            gemini_dir.join("oauth_creds.json"),
            gemini_dir.join("google_accounts.json"),
        )
    }

    fn expected_files_dir(isolated_home: &Path) -> String {
        Self::session_gemini_dir(isolated_home)
            .display()
            .to_string()
    }

    fn update_login_runtime(runtime: &mut GeminiLoginRuntime) {
        let now_ms = Self::now_ms();
        let files_dir = Self::expected_files_dir(&runtime.isolated_home);

        if runtime.status == STATUS_CANCELLED || runtime.status == STATUS_EXPIRED {
            runtime.updated_at_ms = now_ms;
            return;
        }

        if now_ms > runtime.expires_at_ms {
            runtime.status = STATUS_EXPIRED.to_string();
            runtime.message = Some("登录会话已过期，请重新发起会话。".to_string());
            runtime.updated_at_ms = now_ms;
            return;
        }

        let (oauth_path, accounts_path) = Self::session_cred_paths(&runtime.isolated_home);
        if !oauth_path.exists() || !accounts_path.exists() {
            runtime.status = STATUS_PENDING.to_string();
            runtime.message = Some(format!(
                "等待凭据文件就绪：请将 oauth_creds.json 与 google_accounts.json 放入 {files_dir}"
            ));
            runtime.updated_at_ms = now_ms;
            return;
        }

        match (
            Self::read_json(&oauth_path),
            Self::read_json(&accounts_path),
        ) {
            (Ok(_), Ok(_)) => {
                runtime.status = STATUS_AUTHORIZED.to_string();
                runtime.message = Some("凭据文件已就绪，可执行 finalize。".to_string());
            }
            (Err(err), _) | (_, Err(err)) => {
                runtime.status = STATUS_FAILED.to_string();
                runtime.message = Some(format!("凭据文件解析失败: {err}"));
            }
        }
        runtime.updated_at_ms = now_ms;
    }

    fn runtime_to_status(session_id: &str, runtime: &GeminiLoginRuntime) -> GeminiLoginStatus {
        let remaining_seconds = ((runtime.expires_at_ms - Self::now_ms()) / 1000).max(0);
        GeminiLoginStatus {
            session_id: session_id.to_string(),
            provider_id: runtime.provider_id.clone(),
            status: runtime.status.clone(),
            updated_at_ms: runtime.updated_at_ms,
            expires_at_ms: runtime.expires_at_ms,
            remaining_seconds,
            expected_files_dir: Some(Self::expected_files_dir(&runtime.isolated_home)),
            auth_url: Some("https://codeassist.google.com/".to_string()),
            message: runtime.message.clone(),
        }
    }

    pub fn start_cli_login(provider_id: String) -> Result<GeminiLoginSession, AppError> {
        if provider_id.trim().is_empty() {
            return Err(AppError::Config("provider_id 不能为空".to_string()));
        }

        let session_id = uuid::Uuid::new_v4().to_string();
        let now_ms = Self::now_ms();

        let runtime_root = dirs::home_dir()
            .ok_or_else(|| AppError::Config("无法定位 Home 目录".to_string()))?
            .join(".cc-switch")
            .join("runtime")
            .join("gemini-login")
            .join(&session_id);
        let isolated_home = runtime_root.join("home");
        let gemini_home = Self::session_gemini_dir(&isolated_home);
        std::fs::create_dir_all(&gemini_home).map_err(|e| AppError::io(gemini_home.clone(), e))?;

        let runtime = GeminiLoginRuntime {
            provider_id: provider_id.clone(),
            updated_at_ms: now_ms,
            expires_at_ms: now_ms + CLI_LOGIN_TTL_SECONDS * 1000,
            status: STATUS_PENDING.to_string(),
            message: Some(format!(
                "会话已创建，请将 oauth_creds.json 与 google_accounts.json 放入 {}",
                gemini_home.display()
            )),
            isolated_home: isolated_home.clone(),
        };

        let mut sessions = CLI_LOGIN_SESSIONS
            .lock()
            .map_err(|_| AppError::Config("gemini login session lock 失败".to_string()))?;
        sessions.insert(session_id.clone(), runtime);

        Ok(GeminiLoginSession {
            session_id,
            provider_id,
            started_at_ms: now_ms,
            expires_at_ms: now_ms + CLI_LOGIN_TTL_SECONDS * 1000,
            expected_files_dir: Self::expected_files_dir(&isolated_home),
            auth_url: Some("https://codeassist.google.com/".to_string()),
            instructions: "请在浏览器完成 Gemini 登录后，将 oauth_creds.json 与 google_accounts.json 复制到会话隔离目录下的 home/.gemini/ 中，再调用 finalize。".to_string(),
        })
    }

    pub fn get_cli_login_status(session_id: &str) -> Result<GeminiLoginStatus, AppError> {
        let mut sessions = CLI_LOGIN_SESSIONS
            .lock()
            .map_err(|_| AppError::Config("gemini login session lock 失败".to_string()))?;
        let runtime = sessions
            .get_mut(session_id)
            .ok_or_else(|| AppError::Config("登录会话不存在或已过期".to_string()))?;
        Self::update_login_runtime(runtime);
        Ok(Self::runtime_to_status(session_id, runtime))
    }

    pub fn cancel_cli_login(session_id: &str) -> Result<bool, AppError> {
        let mut sessions = CLI_LOGIN_SESSIONS
            .lock()
            .map_err(|_| AppError::Config("gemini login session lock 失败".to_string()))?;
        let runtime = sessions
            .get_mut(session_id)
            .ok_or_else(|| AppError::Config("登录会话不存在或已过期".to_string()))?;
        runtime.status = STATUS_CANCELLED.to_string();
        runtime.updated_at_ms = Self::now_ms();
        runtime.message = Some("登录会话已取消".to_string());
        Ok(true)
    }

    fn read_json(path: &Path) -> Result<Value, AppError> {
        let text = std::fs::read_to_string(path).map_err(|e| AppError::io(path, e))?;
        serde_json::from_str(&text).map_err(|e| AppError::json(path, e))
    }

    fn find_value_by_keys<'a>(value: &'a Value, keys: &[&str]) -> Option<&'a Value> {
        match value {
            Value::Object(map) => {
                for (k, v) in map {
                    if keys.iter().any(|target| k.eq_ignore_ascii_case(target)) {
                        return Some(v);
                    }
                }
                for child in map.values() {
                    if let Some(found) = Self::find_value_by_keys(child, keys) {
                        return Some(found);
                    }
                }
                None
            }
            Value::Array(arr) => arr
                .iter()
                .find_map(|item| Self::find_value_by_keys(item, keys)),
            _ => None,
        }
    }

    fn find_string(value: &Value, keys: &[&str]) -> Option<String> {
        Self::find_value_by_keys(value, keys)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    }

    fn parse_datetime_to_ms(text: &str) -> Option<i64> {
        DateTime::parse_from_rfc3339(text)
            .ok()
            .map(|dt| dt.timestamp_millis())
    }

    fn parse_i64(value: &Value) -> Option<i64> {
        match value {
            Value::Number(n) => n.as_i64(),
            Value::String(s) => {
                if let Ok(n) = s.parse::<i64>() {
                    Some(n)
                } else {
                    Self::parse_datetime_to_ms(s)
                }
            }
            _ => None,
        }
    }

    fn find_i64(value: &Value, keys: &[&str]) -> Option<i64> {
        Self::find_value_by_keys(value, keys).and_then(Self::parse_i64)
    }

    fn build_account_from_files(
        provider_id: &str,
        oauth_path: &Path,
        oauth_value: &Value,
        accounts_value: &Value,
    ) -> Result<GeminiAccount, AppError> {
        let email = Self::find_string(accounts_value, &["email", "account_email"])
            .or_else(|| Self::find_string(oauth_value, &["email", "account_email"]));
        let display_name =
            Self::find_string(accounts_value, &["display_name", "name", "full_name"]);
        let mut google_account_id = Self::find_string(
            accounts_value,
            &[
                "google_account_id",
                "account_id",
                "gaia_id",
                "id",
                "sub",
                "user_id",
            ],
        )
        .or_else(|| {
            Self::find_string(
                oauth_value,
                &[
                    "google_account_id",
                    "account_id",
                    "gaia_id",
                    "id",
                    "sub",
                    "user_id",
                ],
            )
        })
        .unwrap_or_default();

        let oauth_text =
            std::fs::read_to_string(oauth_path).map_err(|e| AppError::io(oauth_path, e))?;
        if google_account_id.is_empty() {
            let mut hasher = Sha256::new();
            hasher.update(oauth_text.as_bytes());
            if let Some(e) = &email {
                hasher.update(e.as_bytes());
            }
            let hex = format!("{:x}", hasher.finalize());
            google_account_id = format!("derived-{}", &hex[..16]);
        }

        let now_ms = Self::now_ms();
        Ok(GeminiAccount {
            id: format!("gemini-{}", google_account_id),
            email,
            display_name: display_name.or_else(|| Some(provider_id.to_string())),
            google_account_id,
            access_token: Self::find_string(oauth_value, &["access_token", "accessToken", "token"]),
            refresh_token: Self::find_string(oauth_value, &["refresh_token", "refreshToken"]),
            token_type: Self::find_string(oauth_value, &["token_type", "tokenType"]),
            expiry_date: Self::find_i64(
                oauth_value,
                &["expiry_date", "expires_at", "expiresAt", "expiry"],
            ),
            source: "gemini_file_import".to_string(),
            is_active: true,
            created_at: now_ms,
            updated_at: now_ms,
        })
    }

    fn build_provider_api_key_payload(
        account: &GeminiAccount,
        oauth_value: &Value,
        accounts_value: &Value,
    ) -> Value {
        let mut map = Map::<String, Value>::new();

        if let Some(v) = account
            .access_token
            .clone()
            .or_else(|| Self::find_string(oauth_value, &["access_token", "accessToken", "token"]))
        {
            map.insert("access_token".to_string(), Value::String(v));
        }
        if let Some(v) = account
            .refresh_token
            .clone()
            .or_else(|| Self::find_string(oauth_value, &["refresh_token", "refreshToken"]))
        {
            map.insert("refresh_token".to_string(), Value::String(v));
        }
        if let Some(v) = account
            .token_type
            .clone()
            .or_else(|| Self::find_string(oauth_value, &["token_type", "tokenType"]))
        {
            map.insert("token_type".to_string(), Value::String(v));
        }
        if let Some(v) = account.expiry_date.or_else(|| {
            Self::find_i64(
                oauth_value,
                &["expiry_date", "expires_at", "expiresAt", "expiry"],
            )
        }) {
            map.insert("expiry_date".to_string(), Value::Number(v.into()));
        }
        if let Some(v) = Self::find_string(oauth_value, &["client_id", "clientId"]) {
            map.insert("client_id".to_string(), Value::String(v));
        }
        if let Some(v) = Self::find_string(oauth_value, &["client_secret", "clientSecret"]) {
            map.insert("client_secret".to_string(), Value::String(v));
        }

        map.insert(
            "google_account_id".to_string(),
            Value::String(account.google_account_id.clone()),
        );
        if let Some(v) = account
            .email
            .clone()
            .or_else(|| Self::find_string(accounts_value, &["email", "account_email"]))
        {
            map.insert("email".to_string(), Value::String(v));
        }

        Value::Object(map)
    }

    fn update_provider_api_key(
        db: &Database,
        provider_id: &str,
        api_key_payload: &Value,
    ) -> Result<(), AppError> {
        let mut provider = db
            .get_provider_by_id(provider_id, "gemini")?
            .ok_or_else(|| AppError::Config(format!("Gemini provider 不存在: {provider_id}")))?;

        let api_key_string = serde_json::to_string(api_key_payload)
            .map_err(|e| AppError::Config(format!("序列化 GEMINI_API_KEY 失败: {e}")))?;

        if !provider.settings_config.is_object() {
            provider.settings_config = Value::Object(Map::new());
        }
        let root = provider
            .settings_config
            .as_object_mut()
            .ok_or_else(|| AppError::Config("provider settings_config 不是对象".to_string()))?;

        let env_entry = root
            .entry("env".to_string())
            .or_insert_with(|| Value::Object(Map::new()));
        if !env_entry.is_object() {
            *env_entry = Value::Object(Map::new());
        }
        if let Some(env_obj) = env_entry.as_object_mut() {
            env_obj.insert("GEMINI_API_KEY".to_string(), Value::String(api_key_string));
        }

        db.update_provider_settings_config("gemini", provider_id, &provider.settings_config)
    }

    pub fn finalize_cli_login(db: &Database, session_id: &str) -> Result<GeminiAccount, AppError> {
        let (provider_id, isolated_home) = {
            let mut sessions = CLI_LOGIN_SESSIONS
                .lock()
                .map_err(|_| AppError::Config("gemini login session lock 失败".to_string()))?;
            let runtime = sessions
                .get_mut(session_id)
                .ok_or_else(|| AppError::Config("登录会话不存在或已过期".to_string()))?;
            Self::update_login_runtime(runtime);
            if runtime.status == STATUS_CANCELLED || runtime.status == STATUS_EXPIRED {
                return Err(AppError::Config(format!(
                    "登录会话不可 finalize，当前状态: {}",
                    runtime.status
                )));
            }
            if runtime.status != STATUS_AUTHORIZED {
                return Err(AppError::Config(format!(
                    "凭据文件尚未就绪，当前状态: {}",
                    runtime.status
                )));
            }
            (runtime.provider_id.clone(), runtime.isolated_home.clone())
        };

        let (oauth_path, accounts_path) = Self::session_cred_paths(&isolated_home);
        if !oauth_path.exists() {
            return Err(AppError::Config(format!(
                "缺少 oauth_creds.json: {}",
                oauth_path.display()
            )));
        }
        if !accounts_path.exists() {
            return Err(AppError::Config(format!(
                "缺少 google_accounts.json: {}",
                accounts_path.display()
            )));
        }

        let oauth_value = Self::read_json(&oauth_path)?;
        let accounts_value = Self::read_json(&accounts_path)?;
        let account = Self::build_account_from_files(
            &provider_id,
            &oauth_path,
            &oauth_value,
            &accounts_value,
        )?;
        let api_key_payload =
            Self::build_provider_api_key_payload(&account, &oauth_value, &accounts_value);

        db.upsert_gemini_account(&account)?;
        db.upsert_gemini_provider_binding(&GeminiProviderBinding {
            provider_id: provider_id.clone(),
            account_id: account.id.clone(),
            auto_bound: false,
            updated_at: Self::now_ms(),
        })?;
        Self::update_provider_api_key(db, &provider_id, &api_key_payload)?;

        let usage = db
            .get_gemini_usage_state(&account.id)?
            .unwrap_or(GeminiUsageState {
                account_id: account.id.clone(),
                cooldown_until: None,
                last_error: None,
                last_refresh_at: None,
            });
        db.upsert_gemini_usage_state(&GeminiUsageState {
            last_refresh_at: Some(Self::now_ms()),
            ..usage
        })?;

        if let Ok(mut sessions) = CLI_LOGIN_SESSIONS.lock() {
            if let Some(runtime) = sessions.get_mut(session_id) {
                runtime.status = STATUS_AUTHORIZED.to_string();
                runtime.updated_at_ms = Self::now_ms();
                runtime.message = Some("账号已落库并绑定到 provider。".to_string());
            }
        }

        Ok(account)
    }

    pub fn list_accounts(db: &Database) -> Result<Vec<GeminiAccount>, AppError> {
        db.list_gemini_accounts(true)
    }

    fn usage_cooldown_seconds(usage: &GeminiUsageState) -> Option<i64> {
        let remain = usage.cooldown_until.unwrap_or(0) - Self::now_secs();
        (remain > 0).then_some(remain)
    }

    pub fn get_usage_view_by_provider(
        db: &Database,
        provider_id: &str,
    ) -> Result<GeminiUsageView, AppError> {
        let binding = db.get_gemini_provider_binding(provider_id)?;
        let account = db.get_gemini_account_by_provider(provider_id)?;
        let usage = if let Some(acc) = &account {
            db.get_gemini_usage_state(&acc.id)?
        } else {
            None
        };

        let cooldown_seconds = usage
            .as_ref()
            .and_then(Self::usage_cooldown_seconds)
            .filter(|v| *v > 0);
        let has_error = usage
            .as_ref()
            .and_then(|u| u.last_error.as_ref())
            .map(|e| !e.trim().is_empty())
            .unwrap_or(false);
        let available = account.is_some() && cooldown_seconds.unwrap_or(0) <= 0 && !has_error;

        Ok(GeminiUsageView {
            provider_id: provider_id.to_string(),
            account,
            binding,
            usage,
            available,
            cooldown_seconds,
        })
    }

    fn refresh_provider_usage_minimal(db: &Database, provider_id: &str) -> Result<bool, AppError> {
        let Some(account) = db.get_gemini_account_by_provider(provider_id)? else {
            return Ok(false);
        };

        let now_ms = Self::now_ms();
        let now_secs = Self::now_secs();
        let mut usage = db
            .get_gemini_usage_state(&account.id)?
            .unwrap_or(GeminiUsageState {
                account_id: account.id.clone(),
                cooldown_until: None,
                last_error: None,
                last_refresh_at: None,
            });

        if usage.cooldown_until.unwrap_or(0) <= now_secs {
            usage.cooldown_until = None;
        }
        if usage
            .last_error
            .as_ref()
            .map(|v| v.trim().is_empty())
            .unwrap_or(false)
        {
            usage.last_error = None;
        }
        usage.last_refresh_at = Some(now_ms);
        db.upsert_gemini_usage_state(&usage)?;
        Ok(true)
    }

    pub fn refresh_usage_now(
        db: &Database,
        provider_id: Option<String>,
    ) -> Result<RefreshResult, AppError> {
        if let Some(provider_id) = provider_id {
            if db.get_gemini_account_by_provider(&provider_id)?.is_none() {
                return Err(AppError::Config(format!(
                    "Gemini provider 未绑定账号，无法刷新用量: {}",
                    provider_id
                )));
            }
            let refreshed = Self::refresh_provider_usage_minimal(db, &provider_id)?;
            return Ok(RefreshResult {
                refreshed_accounts: usize::from(refreshed),
                success_accounts: usize::from(refreshed),
                failed_accounts: 0,
            });
        }

        let providers = db.get_all_providers("gemini")?;
        let mut refreshed_accounts = 0usize;
        let mut success_accounts = 0usize;
        let mut failed_accounts = 0usize;

        for provider_id in providers.keys() {
            match Self::refresh_provider_usage_minimal(db, provider_id) {
                Ok(true) => {
                    refreshed_accounts += 1;
                    success_accounts += 1;
                }
                Ok(false) => {}
                Err(err) => {
                    refreshed_accounts += 1;
                    failed_accounts += 1;
                    log::warn!(
                        "gemini refresh_usage_now failed for provider {}: {}",
                        provider_id,
                        err
                    );
                }
            }
        }

        Ok(RefreshResult {
            refreshed_accounts,
            success_accounts,
            failed_accounts,
        })
    }

    pub fn pool_status(db: &Database) -> Result<GeminiPoolStatus, AppError> {
        let total_accounts = db.list_gemini_accounts(false)?.len();
        let active_accounts = db.list_gemini_accounts(true)?.len();
        let providers = db.get_all_providers("gemini")?;
        let now_secs = Self::now_secs();

        let mut bound_providers = 0usize;
        let mut providers_with_available_account = 0usize;
        let mut providers_in_cooldown = 0usize;
        let mut providers_with_error = 0usize;

        for provider_id in providers.keys() {
            let binding = db.get_gemini_provider_binding(provider_id)?;
            if binding.is_some() {
                bound_providers += 1;
            }

            let Some(account) = db.get_gemini_account_by_provider(provider_id)? else {
                continue;
            };
            let usage = db.get_gemini_usage_state(&account.id)?;

            let mut in_cooldown = false;
            let mut has_error = false;
            if let Some(usage) = usage {
                in_cooldown = usage.cooldown_until.unwrap_or(0) > now_secs;
                has_error = usage
                    .last_error
                    .as_ref()
                    .map(|e| !e.trim().is_empty())
                    .unwrap_or(false);
            }

            if in_cooldown {
                providers_in_cooldown += 1;
            }
            if has_error {
                providers_with_error += 1;
            }
            if !in_cooldown && !has_error {
                providers_with_available_account += 1;
            }
        }

        Ok(GeminiPoolStatus {
            total_accounts,
            active_accounts,
            bound_providers,
            providers_with_available_account,
            providers_in_cooldown,
            providers_with_error,
        })
    }
}
