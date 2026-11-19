//! Official MiniMax Code (MCode) OAuth device login & multi-account switcher.
//!
//! Credentials in MCode CLI live under `$MINIMAX_DATA_DIR/auth/prod/{cn|en}/mcode-public/`:
//! - `auth-state.json` (`schemaVersion: 2`, status, region, generation, expiresAtMs)
//! - `auth.json` (`schemaVersion: 1`, `records["com.minimax.mcode.oauth.prod.{region}\0{key}"]`)
//! - `$MINIMAX_DATA_DIR/preferences/mcode-region.json` (`regions.prod = "cn" | "en"`)
//!
//! CC Switch persists saved accounts in `{app_config_dir}/mcode-accounts.json` and mirrors
//! the active selection into MCode's native directory so switching accounts inside CC Switch
//! takes effect immediately in `mcode`.

use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::Local;
use rand_core_or_uuid::{UuidHelper};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use tokio::sync::Mutex;

use crate::{
    config::{atomic_write_private, get_app_config_dir},
    mcode_config,
    services::{
        coding_plan::parse_minimax_tiers,
        subscription::{CredentialStatus, QuotaTier, SubscriptionQuota, TIER_FIVE_HOUR},
    },
};

const CLIENT_ID: &str = "mcode-public";
const SCOPE: &str = "agent.default";
const AUDIENCE: &str = "agent-backend";
const BUILD_ENV: &str = "prod";
const TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum McodeRegion {
    #[default]
    Cn,
    En,
}

impl McodeRegion {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cn => "cn",
            Self::En => "en",
        }
    }

    fn account_base(self) -> &'static str {
        match self {
            Self::Cn => "https://account.minimax.cn",
            Self::En => "https://account.minimax.io",
        }
    }

    fn agent_base(self) -> &'static str {
        match self {
            Self::Cn => "https://agent.minimaxi.com",
            Self::En => "https://agent.minimax.io",
        }
    }

    fn openplatform_base(self) -> &'static str {
        match self {
            Self::Cn => "https://www.minimaxi.com",
            Self::En => "https://platform.minimax.io",
        }
    }

    fn sys_lang(self) -> &'static str {
        match self {
            Self::Cn => "zh",
            Self::En => "en",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McodeOAuthAccount {
    pub id: String,
    pub name: String,
    pub email: Option<String>,
    pub avatar_url: Option<String>,
    pub region: McodeRegion,
    pub authenticated: bool,
    pub expires_at: Option<i64>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McodeOAuthStatus {
    pub accounts: Vec<McodeOAuthAccount>,
    pub current_account_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McodeLoginStart {
    pub flow_id: String,
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in: u64,
    pub interval: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McodeLoginPoll {
    pub authenticated: bool,
    pub interval: u64,
    pub ended: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct AccountStore {
    #[serde(default)]
    accounts: Vec<StoredMcodeAccount>,
    #[serde(default)]
    current_account_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredMcodeAccount {
    id: String,
    name: String,
    email: Option<String>,
    avatar_url: Option<String>,
    region: McodeRegion,
    access_token: String,
    refresh_token: String,
    expires_at_ms: i64,
    generation: u64,
    #[serde(default)]
    login_epoch: Option<String>,
    #[serde(default)]
    subject: Option<String>,
    #[serde(default)]
    real_user_id: Option<String>,
    #[serde(default)]
    needs_reauth: bool,
    #[serde(default)]
    error: Option<String>,
}

struct PendingFlow {
    region: McodeRegion,
    account_id: Option<String>,
    device_code: String,
    user_code: String,
    polling_with_user_code: bool,
    code_verifier: String,
    expires_at: Instant,
    interval: u64,
    last_poll: Option<Instant>,
}

mod rand_core_or_uuid {
    use uuid::Uuid;

    pub(super) struct UuidHelper;
    impl UuidHelper {
        pub(super) fn random_bytes_32() -> [u8; 32] {
            let mut out = [0u8; 32];
            out[..16].copy_from_slice(Uuid::new_v4().as_bytes());
            out[16..].copy_from_slice(Uuid::new_v4().as_bytes());
            out
        }
    }
}

pub struct McodeOAuthManager {
    store_lock: Arc<Mutex<()>>,
    flows: Mutex<HashMap<String, PendingFlow>>,
}

impl Default for McodeOAuthManager {
    fn default() -> Self {
        Self {
            store_lock: Arc::new(Mutex::new(())),
            flows: Mutex::new(HashMap::new()),
        }
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

fn store_path() -> PathBuf {
    get_app_config_dir().join("mcode-accounts.json")
}

fn auth_home() -> PathBuf {
    mcode_config::data_dir().join("auth")
}

fn namespace_home(region: McodeRegion) -> PathBuf {
    auth_home()
        .join(BUILD_ENV)
        .join(region.as_str())
        .join(CLIENT_ID)
}

fn region_pref_path() -> PathBuf {
    mcode_config::data_dir()
        .join("preferences")
        .join("mcode-region.json")
}

fn native_record_key(auth_home: &Path, region: McodeRegion) -> String {
    let service = format!("com.minimax.mcode.oauth.{BUILD_ENV}.{}", region.as_str());
    let mut hasher = Sha256::new();
    hasher.update(format!("{}\0{CLIENT_ID}", auth_home.to_string_lossy()).as_bytes());
    let account = URL_SAFE_NO_PAD.encode(hasher.finalize());
    format!("{service}\0{account}")
}

fn load_store() -> Result<AccountStore, String> {
    let path = store_path();
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Ok(AccountStore::default());
        }
        Err(err) => return Err(format!("Failed to read MCode accounts store: {err}")),
    };
    serde_json::from_str(&text)
        .map_err(|err| format!("Failed to parse MCode accounts store: {err}"))
}

fn save_store(store: &AccountStore) -> Result<(), String> {
    let path = store_path();
    let bytes = serde_json::to_vec_pretty(store)
        .map_err(|err| format!("Failed to serialize MCode accounts store: {err}"))?;
    atomic_write_private(&path, &bytes)
        .map_err(|err| format!("Failed to write MCode accounts store: {err}"))
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct NativeCredentialRecord {
    access_token: String,
    refresh_token: String,
    #[serde(default)]
    expires_at_ms: i64,
    #[serde(default)]
    generation: u64,
    #[serde(default)]
    login_epoch: Option<String>,
    #[serde(default)]
    subject: Option<String>,
    #[serde(default)]
    account_id: Option<String>,
}

fn read_preferred_region() -> Option<McodeRegion> {
    let text = fs::read_to_string(region_pref_path()).ok()?;
    let val: Value = serde_json::from_str(&text).ok()?;
    match val.pointer("/regions/prod").and_then(Value::as_str) {
        Some("cn") => Some(McodeRegion::Cn),
        Some("en") => Some(McodeRegion::En),
        _ => None,
    }
}

fn read_native_credential(region: McodeRegion) -> Option<NativeCredentialRecord> {
    let ns = namespace_home(region);
    let state_text = fs::read_to_string(ns.join("auth-state.json")).ok()?;
    let state: Value = serde_json::from_str(&state_text).ok()?;
    let status = state.get("status").and_then(Value::as_str)?;
    if !matches!(status, "authenticated" | "expired" | "refreshing") {
        return None;
    }

    let auth_text = fs::read_to_string(ns.join("auth.json")).ok()?;
    let auth_val: Value = serde_json::from_str(&auth_text).ok()?;
    let records = auth_val.get("records")?.as_object()?;

    let exact_key = native_record_key(&auth_home(), region);
    if let Some(entry) = records.get(&exact_key) {
        if let Ok(rec) = serde_json::from_value::<NativeCredentialRecord>(entry.clone()) {
            if !rec.access_token.trim().is_empty() && !rec.refresh_token.trim().is_empty() {
                return Some(rec);
            }
        }
    }

    let prefix = format!("com.minimax.mcode.oauth.{BUILD_ENV}.{}\0", region.as_str());
    for (k, v) in records {
        if k.starts_with(&prefix) {
            if let Ok(rec) = serde_json::from_value::<NativeCredentialRecord>(v.clone()) {
                if !rec.access_token.trim().is_empty() && !rec.refresh_token.trim().is_empty() {
                    return Some(rec);
                }
            }
        }
    }
    None
}

fn write_native_account(account: &StoredMcodeAccount) -> Result<(), String> {
    let ahome = auth_home();
    let ns = namespace_home(account.region);
    fs::create_dir_all(&ns).map_err(|e| format!("Failed to create MCode auth directory: {e}"))?;

    // Ensure the inactive region is anonymous so MCode never raises AUTH_DOMAIN_CONFLICT.
    let other_region = match account.region {
        McodeRegion::Cn => McodeRegion::En,
        McodeRegion::En => McodeRegion::Cn,
    };
    let other_state_path = namespace_home(other_region).join("auth-state.json");
    if other_state_path.exists() {
        let gen = fs::read_to_string(&other_state_path)
            .ok()
            .and_then(|s| serde_json::from_str::<Value>(&s).ok())
            .and_then(|v| v.get("generation").and_then(Value::as_u64))
            .unwrap_or(0)
            + 1;
        let anon = json!({
            "schemaVersion": 2,
            "status": "anonymous",
            "storeKind": "file",
            "clientId": CLIENT_ID,
            "scopes": [SCOPE],
            "audience": AUDIENCE,
            "buildEnv": BUILD_ENV,
            "region": other_region.as_str(),
            "generation": gen,
        });
        let _ = atomic_write_private(
            &other_state_path,
            format!("{}\n", serde_json::to_string_pretty(&anon).unwrap()).as_bytes(),
        );
    }

    let key = native_record_key(&ahome, account.region);
    let mut records = Map::new();
    let mut rec = json!({
        "schemaVersion": 1,
        "accessToken": account.access_token,
        "refreshToken": account.refresh_token,
        "tokenType": "Bearer",
        "clientId": CLIENT_ID,
        "scopes": [SCOPE],
        "audience": AUDIENCE,
        "expiresAtMs": account.expires_at_ms,
        "generation": account.generation.max(1),
        "loginEpoch": account.login_epoch.clone().unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
    });
    if let Some(sub) = &account.subject {
        rec["subject"] = json!(sub);
    }
    if let Some(rid) = &account.real_user_id {
        rec["accountId"] = json!(rid);
    }
    records.insert(key, rec);

    let auth_json = json!({
        "schemaVersion": 1,
        "records": records,
    });
    atomic_write_private(
        &ns.join("auth.json"),
        format!("{}\n", serde_json::to_string_pretty(&auth_json).unwrap()).as_bytes(),
    )
    .map_err(|e| format!("Failed to write MCode auth.json: {e}"))?;

    let state_json = json!({
        "schemaVersion": 2,
        "status": "authenticated",
        "storeKind": "file",
        "clientId": CLIENT_ID,
        "scopes": [SCOPE],
        "audience": AUDIENCE,
        "buildEnv": BUILD_ENV,
        "region": account.region.as_str(),
        "generation": account.generation.max(1),
        "expiresAtMs": account.expires_at_ms,
    });
    atomic_write_private(
        &ns.join("auth-state.json"),
        format!("{}\n", serde_json::to_string_pretty(&state_json).unwrap()).as_bytes(),
    )
    .map_err(|e| format!("Failed to write MCode auth-state.json: {e}"))?;

    let pref_path = region_pref_path();
    let mut existing_regions = fs::read_to_string(&pref_path)
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .and_then(|v| v.get("regions").and_then(Value::as_object).cloned())
        .unwrap_or_default();
    existing_regions.insert("prod".into(), json!(account.region.as_str()));
    let pref_json = json!({
        "version": 1,
        "regions": existing_regions,
        "updatedAtMs": now_ms(),
    });
    atomic_write_private(
        &pref_path,
        format!("{}\n", serde_json::to_string_pretty(&pref_json).unwrap()).as_bytes(),
    )
    .map_err(|e| format!("Failed to write MCode region preference: {e}"))?;

    Ok(())
}

fn clear_native_region(region: McodeRegion) -> Result<(), String> {
    let ns = namespace_home(region);
    let auth_path = ns.join("auth.json");
    if auth_path.exists() {
        let auth_json = json!({ "schemaVersion": 1, "records": {} });
        let _ = atomic_write_private(
            &auth_path,
            format!("{}\n", serde_json::to_string_pretty(&auth_json).unwrap()).as_bytes(),
        );
    }
    let state_path = ns.join("auth-state.json");
    if state_path.exists() {
        let gen = fs::read_to_string(&state_path)
            .ok()
            .and_then(|s| serde_json::from_str::<Value>(&s).ok())
            .and_then(|v| v.get("generation").and_then(Value::as_u64))
            .unwrap_or(0)
            + 1;
        let anon = json!({
            "schemaVersion": 2,
            "status": "anonymous",
            "storeKind": "file",
            "clientId": CLIENT_ID,
            "scopes": [SCOPE],
            "audience": AUDIENCE,
            "buildEnv": BUILD_ENV,
            "region": region.as_str(),
            "generation": gen,
        });
        atomic_write_private(
            &state_path,
            format!("{}\n", serde_json::to_string_pretty(&anon).unwrap()).as_bytes(),
        )
        .map_err(|e| format!("Failed to clear MCode auth-state.json: {e}"))?;
    }
    Ok(())
}

fn decode_jwt_claims(token: &str) -> Option<Value> {
    let part = token.split('.').nth(1)?;
    let bytes = URL_SAFE_NO_PAD.decode(part).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn find_active_native() -> Option<(McodeRegion, NativeCredentialRecord)> {
    let order = match read_preferred_region() {
        Some(McodeRegion::En) => [McodeRegion::En, McodeRegion::Cn],
        _ => [McodeRegion::Cn, McodeRegion::En],
    };
    for region in order {
        if let Some(rec) = read_native_credential(region) {
            return Some((region, rec));
        }
    }
    None
}

fn sync_native_with_store(store: &mut AccountStore) -> bool {
    let Some((region, native)) = find_active_native() else {
        if store.current_account_id.is_some() {
            store.current_account_id = None;
            return true;
        }
        return false;
    };

    let claims = decode_jwt_claims(&native.access_token);
    let subject = native.subject.clone().or_else(|| {
        claims
            .as_ref()
            .and_then(|c| c.get("sub").and_then(Value::as_str).map(String::from))
    });
    let account_id_claim = native.account_id.clone().or_else(|| {
        claims.as_ref().and_then(|c| {
            c.get("account_id")
                .and_then(Value::as_str)
                .map(String::from)
        })
    });

    if let Some(existing) = store.accounts.iter_mut().find(|acct| {
        acct.region == region
            && ((subject.is_some() && acct.subject == subject)
                || (account_id_claim.is_some() && acct.real_user_id == account_id_claim)
                || acct.refresh_token == native.refresh_token)
    }) {
        let mut changed = false;
        if native.generation > existing.generation || native.expires_at_ms > existing.expires_at_ms
        {
            existing.access_token = native.access_token;
            existing.refresh_token = native.refresh_token;
            existing.expires_at_ms = native.expires_at_ms;
            existing.generation = native.generation.max(existing.generation);
            existing.login_epoch = native.login_epoch.or(existing.login_epoch.clone());
            existing.needs_reauth = false;
            existing.error = None;
            changed = true;
        }
        if store.current_account_id.as_deref() != Some(existing.id.as_str()) {
            store.current_account_id = Some(existing.id.clone());
            changed = true;
        }
        return changed;
    }

    let id = format!(
        "mcode-{}",
        subject
            .as_deref()
            .or(account_id_claim.as_deref())
            .map(|s| s
                .chars()
                .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
                .collect::<String>())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string())
    );
    let fallback_name = account_id_claim
        .clone()
        .or_else(|| subject.clone())
        .unwrap_or_else(|| format!("MiniMax ({})", region.as_str().to_uppercase()));
    let acct = StoredMcodeAccount {
        id: id.clone(),
        name: fallback_name,
        email: None,
        avatar_url: None,
        region,
        access_token: native.access_token,
        refresh_token: native.refresh_token,
        expires_at_ms: native.expires_at_ms,
        generation: native.generation.max(1),
        login_epoch: native.login_epoch,
        subject,
        real_user_id: account_id_claim,
        needs_reauth: false,
        error: None,
    };
    store.accounts.push(acct);
    store.current_account_id = Some(id);
    true
}

#[derive(Default)]
struct UserIdentity {
    real_user_id: Option<String>,
    name: Option<String>,
    email: Option<String>,
}

fn build_agent_url(region: McodeRegion, path: &str, now_millis: i64, user_id: Option<&str>) -> String {
    let base = region.agent_base();
    let mut url = url::Url::parse(&format!("{base}{path}")).expect("valid MCode agent URL");
    let offset_secs = (-Local::now().offset().local_minus_utc()).to_string();
    let lang = region.sys_lang();
    let now_str = now_millis.to_string();
    let uid = user_id.map(str::trim).filter(|s| !s.is_empty()).unwrap_or("0");
    url.query_pairs_mut()
        .append_pair("device_platform", "mcode")
        .append_pair("biz_id", "3")
        .append_pair("app_id", "3001")
        .append_pair("version_code", "22201")
        .append_pair("unix", &now_str)
        .append_pair("timezone_offset", &offset_secs)
        .append_pair("sys_language", lang)
        .append_pair("lang", lang)
        .append_pair("device_id", "0")
        .append_pair("os_name", std::env::consts::OS)
        .append_pair("browser_name", "mcode")
        .append_pair("user_id", uid)
        .append_pair("client", "mcode");
    url.to_string()
}

fn agent_signed_headers(
    req: reqwest::RequestBuilder,
    full_url: &str,
    access_token: &str,
    now_millis: i64,
) -> reqwest::RequestBuilder {
    let parsed = url::Url::parse(full_url).expect("valid parsed agent URL");
    let path_and_query = match parsed.query() {
        Some(q) => format!("{}?{q}", parsed.path()),
        None => parsed.path().to_string(),
    };
    let encoded_path = url::form_urlencoded::byte_serialize(path_and_query.as_bytes())
        .collect::<String>()
        .replace('+', "%20");
    let ts_sec = now_millis / 1000;
    let yy = md5_hex(
        format!(
            "{encoded_path}_{{}}{}ooui",
            md5_hex(now_millis.to_string().as_bytes())
        )
        .as_bytes(),
    );
    let x_sig = md5_hex(format!("{ts_sec}I*7Cf%WZ#S&%1RlZJ&C2").as_bytes());
    req.header("Accept", "application/json")
        .header("Content-Type", "application/json")
        .header("User-Agent", "MiniMaxCode")
        .header("Authorization", format!("Bearer {access_token}"))
        .header("yy", yy)
        .header("x-timestamp", ts_sec.to_string())
        .header("x-signature", x_sig)
}

async fn fetch_user_identity(region: McodeRegion, access_token: &str) -> Result<UserIdentity, String> {
    let now = now_ms();
    let url = build_agent_url(region, "/v1/api/user/info", now, None);
    let client = crate::proxy::http_client::get();
    let req = agent_signed_headers(client.get(&url), &url, access_token, now).timeout(TIMEOUT);
    let resp = req
        .send()
        .await
        .map_err(|e| format!("MCode account identity request failed: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("MCode account identity HTTP {}", resp.status().as_u16()));
    }
    let body: Value = resp
        .json()
        .await
        .map_err(|e| format!("Invalid MCode identity response: {e}"))?;
    let user_info = body
        .pointer("/data/userInfo")
        .or_else(|| body.pointer("/data/user_info"))
        .or_else(|| body.get("userInfo"))
        .or_else(|| body.get("user_info"))
        .unwrap_or(&body);
    let pick = |keys: &[&str]| -> Option<String> {
        for k in keys {
            if let Some(s) = user_info.get(*k).and_then(Value::as_str) {
                let t = s.trim();
                if !t.is_empty() {
                    return Some(t.to_string());
                }
            }
        }
        None
    };
    let real_user_id = pick(&["realUserID", "real_user_id"]);
    let email = pick(&["email", "userMail", "user_email"]);
    let name = pick(&["name", "userName", "user_name", "subUserName", "sub_user_name"])
        .or_else(|| email.clone());
    Ok(UserIdentity {
        real_user_id,
        name,
        email,
    })
}

async fn post_matrix_json(
    region: McodeRegion,
    path: &str,
    payload: &Value,
    access_token: &str,
    user_id: Option<&str>,
) -> Option<Value> {
    let now = now_ms();
    let url = build_agent_url(region, path, now, user_id);
    let client = crate::proxy::http_client::get();
    let resp = agent_signed_headers(client.post(&url), &url, access_token, now)
        .json(payload)
        .timeout(TIMEOUT)
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    resp.json::<Value>().await.ok()
}

fn extract_membership_fields(body: &Value) -> (Option<bool>, Option<String>, Option<String>) {
    let data = body.get("data").unwrap_or(body);
    let has_token_plan = body
        .get("has_token_plan")
        .or_else(|| data.get("has_token_plan"))
        .and_then(Value::as_bool);
    let op_group_id = body
        .get("op_group_id")
        .or_else(|| data.get("op_group_id"))
        .and_then(Value::as_str)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let plan_tier = body
        .get("token_plan_tier")
        .or_else(|| data.get("token_plan_tier"))
        .and_then(Value::as_str)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    (has_token_plan, op_group_id, plan_tier)
}

async fn fetch_membership_context(
    region: McodeRegion,
    access_token: &str,
    real_user_id: Option<&str>,
) -> (Option<bool>, Option<String>, Option<String>) {
    let mut uid = real_user_id.map(str::to_string);
    if uid.is_none() {
        if let Ok(id) = fetch_user_identity(region, access_token).await {
            uid = id.real_user_id;
        }
    }
    let uid_ref = uid.as_deref();
    let mut op_group_id = None;
    let mut plan_tier = None;
    let mut has_plan = None;

    if let Some(extra) = post_matrix_json(
        region,
        "/matrix/api/v1/user/get_user_extra_info",
        &json!({}),
        access_token,
        uid_ref,
    )
    .await
    {
        let workspaces = extra
            .get("workspaces")
            .or_else(|| extra.pointer("/data/workspaces"))
            .and_then(Value::as_array);
        if let Some(ws_list) = workspaces {
            for ws in ws_list {
                if ws.get("workspace_type").and_then(Value::as_i64) == Some(0) {
                    let (hp, gid, tier) = extract_membership_fields(ws);
                    has_plan = hp;
                    op_group_id = gid;
                    plan_tier = tier;
                    if let Some(ws_id) = ws.get("workspace_id").cloned() {
                        if let Some(member) = post_matrix_json(
                            region,
                            "/matrix/api/v1/commerce/get_membership_info",
                            &json!({ "workspace_id": ws_id }),
                            access_token,
                            uid_ref,
                        )
                        .await
                        {
                            let (mhp, mgid, mtier) = extract_membership_fields(&member);
                            if mhp.is_some() {
                                has_plan = mhp;
                            }
                            if mgid.is_some() {
                                op_group_id = mgid;
                            }
                            if mtier.is_some() {
                                plan_tier = mtier;
                            }
                        }
                    }
                    break;
                }
            }
        }
    }

    if op_group_id.is_none() && has_plan.is_none() {
        if let Some(member) = post_matrix_json(
            region,
            "/matrix/api/v1/commerce/get_membership_info",
            &json!({}),
            access_token,
            uid_ref,
        )
        .await
        {
            return extract_membership_fields(&member);
        }
    }

    (has_plan, op_group_id, plan_tier)
}

struct TokenOutcome {
    access_token: String,
    refresh_token: String,
    expires_in_sec: i64,
    subject: Option<String>,
    account_id: Option<String>,
}

fn parse_token_body(body: &Value, fallback_refresh: Option<&str>) -> Result<TokenOutcome, String> {
    let access_token = body
        .get("access_token")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "MCode token response missing access_token".to_string())?
        .to_string();
    let refresh_token = body
        .get("refresh_token")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| fallback_refresh.map(str::to_string))
        .ok_or_else(|| "MCode token response missing refresh_token".to_string())?;
    let expires_in_sec = body
        .get("expires_in")
        .and_then(Value::as_i64)
        .filter(|&v| v > 0)
        .unwrap_or(3600);
    let claims = decode_jwt_claims(&access_token);
    let subject = claims
        .as_ref()
        .and_then(|c| c.get("sub").and_then(Value::as_str).map(String::from));
    let account_id = claims.as_ref().and_then(|c| {
        c.get("account_id")
            .and_then(Value::as_str)
            .map(String::from)
    });
    Ok(TokenOutcome {
        access_token,
        refresh_token,
        expires_in_sec,
        subject,
        account_id,
    })
}

async fn refresh_token_http(region: McodeRegion, refresh_token: &str) -> Result<TokenOutcome, String> {
    let url = format!("{}/oauth2/token", region.account_base());
    let client = crate::proxy::http_client::get();
    let resp = client
        .post(&url)
        .header("Accept", "application/json")
        .form(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
            ("client_id", CLIENT_ID),
            ("scope", SCOPE),
            ("audience", AUDIENCE),
        ])
        .timeout(TIMEOUT)
        .send()
        .await
        .map_err(|e| format!("MCode token refresh request failed: {e}"))?;
    let status = resp.status();
    let body: Value = resp
        .json()
        .await
        .map_err(|e| format!("Invalid MCode token refresh response: {e}"))?;
    if !status.is_success() || body.get("error").is_some() {
        let err = body
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("oauth_refresh_failed");
        return Err(format!("MCode token refresh rejected ({err})"));
    }
    parse_token_body(&body, Some(refresh_token))
}

impl McodeOAuthManager {
    pub async fn status(&self) -> Result<McodeOAuthStatus, String> {
        let mut store = {
            let _guard = self.store_lock.lock().await;
            let mut s = load_store()?;
            if sync_native_with_store(&mut s) {
                save_store(&s)?;
            }
            s
        };

        // Enrich accounts that haven't resolved display name/email yet (e.g. freshly absorbed from CLI).
        let mut enriched_updates: Vec<(String, UserIdentity)> = Vec::new();
        for acct in &store.accounts {
            if !acct.needs_reauth
                && acct.email.is_none()
                && acct.real_user_id.is_none()
                && acct.expires_at_ms > now_ms()
            {
                if let Ok(identity) = fetch_user_identity(acct.region, &acct.access_token).await {
                    if identity.name.is_some()
                        || identity.email.is_some()
                        || identity.real_user_id.is_some()
                    {
                        enriched_updates.push((acct.id.clone(), identity));
                    }
                }
            }
        }
        if !enriched_updates.is_empty() {
            let _guard = self.store_lock.lock().await;
            let mut latest = load_store()?;
            for (id, identity) in enriched_updates {
                if let Some(acct) = latest.accounts.iter_mut().find(|a| a.id == id) {
                    if let Some(name) = identity.name {
                        acct.name = name;
                    }
                    if identity.email.is_some() {
                        acct.email = identity.email;
                    }
                    if identity.real_user_id.is_some() {
                        acct.real_user_id = identity.real_user_id;
                    }
                }
            }
            save_store(&latest)?;
            store = latest;
        }

        let accounts = store
            .accounts
            .into_iter()
            .map(|a| McodeOAuthAccount {
                id: a.id,
                name: a.name,
                email: a.email,
                avatar_url: a.avatar_url,
                region: a.region,
                authenticated: !a.needs_reauth,
                expires_at: Some(a.expires_at_ms),
                error: a.error,
            })
            .collect();

        Ok(McodeOAuthStatus {
            accounts,
            current_account_id: store.current_account_id,
        })
    }

    pub async fn start(
        &self,
        region: McodeRegion,
        account_id: Option<String>,
    ) -> Result<McodeLoginStart, String> {
        let verifier = URL_SAFE_NO_PAD.encode(UuidHelper::random_bytes_32());
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        let url = format!("{}/oauth2/device/code", region.account_base());

        let client = crate::proxy::http_client::get();
        let resp = client
            .post(&url)
            .header("Accept", "application/json")
            .form(&[
                ("client_id", CLIENT_ID),
                ("scope", SCOPE),
                ("audience", AUDIENCE),
                ("code_challenge", challenge.as_str()),
                ("code_challenge_method", "S256"),
            ])
            .timeout(TIMEOUT)
            .send()
            .await
            .map_err(|e| format!("Failed to start MCode OAuth login: {e}"))?;

        let status = resp.status();
        let body: Value = resp
            .json()
            .await
            .map_err(|e| format!("Invalid MCode device authorization response: {e}"))?;
        if !status.is_success() {
            let msg = body
                .get("error_description")
                .or_else(|| body.get("error"))
                .and_then(Value::as_str)
                .unwrap_or("device authorization rejected");
            return Err(format!("MCode device login failed ({}): {msg}", status.as_u16()));
        }

        let user_code = body
            .get("user_code")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| "MCode device authorization missing user_code".to_string())?
            .to_string();
        let raw_device_code = body
            .get("device_code")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        let expired_in_field = body.get("expired_in").and_then(Value::as_i64);
        let polling_with_user_code = raw_device_code.is_none() && expired_in_field.is_some();
        let device_code = raw_device_code.unwrap_or_else(|| user_code.clone());
        let verification_uri = body
            .get("verification_uri_complete")
            .or_else(|| body.get("verification_uri"))
            .or_else(|| body.get("verification_url"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| "MCode device authorization missing verification_uri".to_string())?
            .to_string();

        let expires_in = body
            .get("expires_in")
            .and_then(Value::as_u64)
            .filter(|&v| v > 0)
            .or_else(|| {
                expired_in_field.and_then(|v| {
                    let v = v as i64;
                    if v <= 0 {
                        None
                    } else if v < 1_000_000_000_000 {
                        Some(v as u64)
                    } else {
                        let diff = (v - now_ms()) / 1000;
                        (diff > 0).then_some(diff as u64)
                    }
                })
            })
            .unwrap_or(600);
        let raw_interval = body.get("interval").and_then(Value::as_u64);
        let interval = if polling_with_user_code {
            raw_interval.map(|ms| (ms / 1000).max(1)).unwrap_or(5)
        } else {
            raw_interval.unwrap_or(5).max(1)
        };

        let flow_id = uuid::Uuid::new_v4().to_string();
        self.flows.lock().await.insert(
            flow_id.clone(),
            PendingFlow {
                region,
                account_id,
                device_code,
                user_code: user_code.clone(),
                polling_with_user_code,
                code_verifier: verifier,
                expires_at: Instant::now() + Duration::from_secs(expires_in),
                interval,
                last_poll: None,
            },
        );

        Ok(McodeLoginStart {
            flow_id,
            user_code,
            verification_uri,
            expires_in,
            interval,
        })
    }

    pub async fn poll(&self, flow_id: &str) -> Result<McodeLoginPoll, String> {
        let (region, account_id, poll_param_key, poll_param_val, verifier, interval) = {
            let mut flows = self.flows.lock().await;
            let Some(flow) = flows.get_mut(flow_id) else {
                return Ok(McodeLoginPoll {
                    authenticated: false,
                    interval: 5,
                    ended: Some("cancelled".into()),
                });
            };
            if Instant::now() >= flow.expires_at {
                flows.remove(flow_id);
                return Ok(McodeLoginPoll {
                    authenticated: false,
                    interval: 5,
                    ended: Some("expired".into()),
                });
            }
            if let Some(last) = flow.last_poll {
                let min_wait = Duration::from_secs(flow.interval.max(1));
                if last.elapsed() < min_wait {
                    return Ok(McodeLoginPoll {
                        authenticated: false,
                        interval: flow.interval,
                        ended: None,
                    });
                }
            }
            flow.last_poll = Some(Instant::now());
            let (k, v) = if flow.polling_with_user_code {
                ("user_code", flow.user_code.clone())
            } else {
                ("device_code", flow.device_code.clone())
            };
            (
                flow.region,
                flow.account_id.clone(),
                k,
                v,
                flow.code_verifier.clone(),
                flow.interval,
            )
        };

        let url = format!("{}/oauth2/token", region.account_base());
        let client = crate::proxy::http_client::get();
        let resp = client
            .post(&url)
            .header("Accept", "application/json")
            .form(&[
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                (poll_param_key, poll_param_val.as_str()),
                ("client_id", CLIENT_ID),
                ("code_verifier", verifier.as_str()),
            ])
            .timeout(TIMEOUT)
            .send()
            .await
            .map_err(|e| format!("Failed to poll MCode OAuth status: {e}"))?;

        let http_ok = resp.status().is_success();
        let body: Value = resp
            .json()
            .await
            .map_err(|e| format!("Invalid MCode token poll response: {e}"))?;

        let status_str = body.get("status").and_then(Value::as_str);
        let error_str = body.get("error").and_then(Value::as_str);

        if (http_ok && status_str == Some("pending")) || error_str == Some("authorization_pending") {
            return Ok(McodeLoginPoll {
                authenticated: false,
                interval,
                ended: None,
            });
        }
        if (http_ok && status_str == Some("slow_down")) || error_str == Some("slow_down") {
            let mut flows = self.flows.lock().await;
            let next_interval = if let Some(flow) = flows.get_mut(flow_id) {
                flow.interval = (flow.interval + 5).min(30);
                flow.interval
            } else {
                interval + 5
            };
            return Ok(McodeLoginPoll {
                authenticated: false,
                interval: next_interval,
                ended: None,
            });
        }
        if (http_ok && matches!(status_str, Some("denied" | "access_denied")))
            || error_str == Some("access_denied")
        {
            self.flows.lock().await.remove(flow_id);
            return Ok(McodeLoginPoll {
                authenticated: false,
                interval,
                ended: Some("denied".into()),
            });
        }
        if (http_ok && matches!(status_str, Some("expired" | "expired_token")))
            || error_str == Some("expired_token")
        {
            self.flows.lock().await.remove(flow_id);
            return Ok(McodeLoginPoll {
                authenticated: false,
                interval,
                ended: Some("expired".into()),
            });
        }
        if !http_ok || error_str.is_some() {
            self.flows.lock().await.remove(flow_id);
            let msg = body
                .get("error_description")
                .or_else(|| body.get("error"))
                .and_then(Value::as_str)
                .unwrap_or("MCode OAuth failed");
            return Err(msg.to_string());
        }

        let outcome = parse_token_body(&body, None)?;
        let identity = fetch_user_identity(region, &outcome.access_token)
            .await
            .unwrap_or_default();

        let _guard = self.store_lock.lock().await;
        // Abort cleanly if flow was cancelled while network finished.
        if self.flows.lock().await.remove(flow_id).is_none() {
            return Ok(McodeLoginPoll {
                authenticated: false,
                interval,
                ended: Some("cancelled".into()),
            });
        }

        let mut store = load_store()?;
        let expires_at_ms = now_ms() + outcome.expires_in_sec * 1000;
        let resolved_name = identity
            .name
            .or_else(|| identity.email.clone())
            .or_else(|| identity.real_user_id.clone())
            .or_else(|| outcome.account_id.clone())
            .or_else(|| outcome.subject.clone())
            .unwrap_or_else(|| format!("MiniMax ({})", region.as_str().to_uppercase()));

        let target_idx = if let Some(ref target_id) = account_id {
            store.accounts.iter().position(|a| a.id == *target_id)
        } else {
            store.accounts.iter().position(|a| {
                a.region == region
                    && ((identity.real_user_id.is_some()
                        && a.real_user_id == identity.real_user_id)
                        || (outcome.subject.is_some() && a.subject == outcome.subject))
            })
        };

        let saved_account = if let Some(idx) = target_idx {
            let acct = &mut store.accounts[idx];
            acct.name = resolved_name;
            acct.email = identity.email.or(acct.email.clone());
            acct.region = region;
            acct.access_token = outcome.access_token;
            acct.refresh_token = outcome.refresh_token;
            acct.expires_at_ms = expires_at_ms;
            acct.generation = acct.generation.max(1) + 1;
            acct.login_epoch = Some(uuid::Uuid::new_v4().to_string());
            acct.subject = outcome.subject.or(acct.subject.clone());
            acct.real_user_id = identity.real_user_id.or(acct.real_user_id.clone());
            acct.needs_reauth = false;
            acct.error = None;
            acct.clone()
        } else {
            let new_id = account_id.unwrap_or_else(|| format!("mcode-{}", uuid::Uuid::new_v4()));
            let acct = StoredMcodeAccount {
                id: new_id,
                name: resolved_name,
                email: identity.email,
                avatar_url: None,
                region,
                access_token: outcome.access_token,
                refresh_token: outcome.refresh_token,
                expires_at_ms,
                generation: 1,
                login_epoch: Some(uuid::Uuid::new_v4().to_string()),
                subject: outcome.subject,
                real_user_id: identity.real_user_id.or(outcome.account_id),
                needs_reauth: false,
                error: None,
            };
            store.accounts.push(acct.clone());
            acct
        };

        write_native_account(&saved_account)?;
        store.current_account_id = Some(saved_account.id);
        save_store(&store)?;

        Ok(McodeLoginPoll {
            authenticated: true,
            interval,
            ended: None,
        })
    }

    pub async fn cancel(&self, flow_id: &str) -> bool {
        self.flows.lock().await.remove(flow_id).is_some()
    }

    pub async fn use_account(&self, account_id: &str) -> Result<(), String> {
        let _guard = self.store_lock.lock().await;
        let mut store = load_store()?;
        let acct = store
            .accounts
            .iter()
            .find(|a| a.id == account_id)
            .cloned()
            .ok_or_else(|| "MCode account not found".to_string())?;
        if acct.needs_reauth {
            return Err("MCode account requires sign-in again".to_string());
        }
        write_native_account(&acct)?;
        store.current_account_id = Some(account_id.to_string());
        save_store(&store)?;
        Ok(())
    }

    pub async fn remove(&self, account_id: &str) -> Result<(), String> {
        let _guard = self.store_lock.lock().await;
        let mut store = load_store()?;
        let idx = store
            .accounts
            .iter()
            .position(|a| a.id == account_id)
            .ok_or_else(|| "MCode account not found".to_string())?;
        let removed = store.accounts.remove(idx);
        let was_current = store.current_account_id.as_deref() == Some(account_id);
        if was_current {
            clear_native_region(removed.region)?;
            store.current_account_id = None;
        }
        save_store(&store)?;
        Ok(())
    }

    pub async fn quota(&self, account_id: &str) -> Result<SubscriptionQuota, String> {
        let mut acct = {
            let _guard = self.store_lock.lock().await;
            let store = load_store()?;
            store
                .accounts
                .into_iter()
                .find(|a| a.id == account_id)
                .ok_or_else(|| "MCode account not found".to_string())?
        };

        if acct.needs_reauth {
            return Ok(SubscriptionQuota::error(
                "mcode_oauth",
                CredentialStatus::Expired,
                "Sign-in expired".into(),
            ));
        }

        // Refresh proactively if within 60s of expiry.
        if now_ms() >= acct.expires_at_ms - 60_000 {
            match refresh_token_http(acct.region, &acct.refresh_token).await {
                Ok(outcome) => {
                    let _guard = self.store_lock.lock().await;
                    let mut store = load_store()?;
                    if let Some(target) = store.accounts.iter_mut().find(|a| a.id == account_id) {
                        target.access_token = outcome.access_token;
                        target.refresh_token = outcome.refresh_token;
                        target.expires_at_ms = now_ms() + outcome.expires_in_sec * 1000;
                        target.generation = target.generation.max(1) + 1;
                        target.needs_reauth = false;
                        target.error = None;
                        acct = target.clone();
                        if store.current_account_id.as_deref() == Some(account_id) {
                            let _ = write_native_account(&acct);
                        }
                        let _ = save_store(&store);
                    }
                }
                Err(err) => {
                    return Ok(SubscriptionQuota::error(
                        "mcode_oauth",
                        CredentialStatus::Expired,
                        err,
                    ));
                }
            }
        }

        let (_has_plan, op_group_id, plan_tier) =
            fetch_membership_context(acct.region, &acct.access_token, acct.real_user_id.as_deref())
                .await;

        let url = format!(
            "{}/v1/api/openplatform/coding_plan/remains?business=Agent&device_platform=mcode",
            acct.region.openplatform_base()
        );
        let client = crate::proxy::http_client::get();
        let mut req = client
            .get(&url)
            .header("Accept", "application/json")
            .header("Authorization", format!("Bearer {}", acct.access_token))
            .timeout(TIMEOUT);
        if let Some(ref gid) = op_group_id {
            req = req.header("X-Group-Id", gid);
        }
        let resp = req
            .send()
            .await
            .map_err(|e| format!("MCode quota request failed: {e}"))?;
        let status = resp.status();
        if status.as_u16() == 401 || status.as_u16() == 403 {
            return Ok(SubscriptionQuota::error(
                "mcode_oauth",
                CredentialStatus::Expired,
                format!("HTTP {}", status.as_u16()),
            ));
        }
        if !status.is_success() {
            return Ok(SubscriptionQuota::error(
                "mcode_oauth",
                CredentialStatus::Valid,
                format!("HTTP {}", status.as_u16()),
            ));
        }
        let body: Value = resp
            .json()
            .await
            .map_err(|e| format!("Invalid MCode quota response: {e}"))?;
        if let Some(code) = body.pointer("/base_resp/status_code").and_then(Value::as_i64) {
            if code != 0 {
                let msg = body
                    .pointer("/base_resp/status_msg")
                    .and_then(Value::as_str)
                    .unwrap_or("API error");
                return Ok(SubscriptionQuota::error(
                    "mcode_oauth",
                    CredentialStatus::Valid,
                    format!("API error ({code}): {msg}"),
                ));
            }
        }

        let mut tiers = parse_minimax_tiers(&body);
        if tiers.is_empty() {
            if let Some(first) = body
                .get("model_remains")
                .and_then(Value::as_array)
                .and_then(|arr| arr.first())
            {
                if let Some(pct) = first
                    .get("current_interval_remaining_percent")
                    .and_then(Value::as_f64)
                {
                    let resets_at = first
                        .get("end_time")
                        .and_then(Value::as_i64)
                        .and_then(chrono::DateTime::from_timestamp_millis)
                        .map(|dt| dt.to_rfc3339());
                    tiers.push(QuotaTier {
                        name: TIER_FIVE_HOUR.to_string(),
                        utilization: 100.0 - pct,
                        resets_at,
                        used_value_usd: None,
                        max_value_usd: None,
                    });
                }
            }
        }

        Ok(SubscriptionQuota {
            tool: "mcode_oauth".to_string(),
            credential_status: CredentialStatus::Valid,
            credential_message: None,
            success: true,
            tiers,
            extra_usage: None,
            reset_credits: None,
            credits_balance: None,
            plan_name: plan_tier,
            error: None,
            queried_at: Some(now_ms()),
        })
    }
}

/// Pure-Rust RFC 1321 MD5 digest in lowercase hex for MCode's `yy`/`x-signature` request headers.
fn md5_hex(input: &[u8]) -> String {
    const S: [u32; 64] = [
        7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5,
        9, 14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10,
        15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
    ];
    const K: [u32; 64] = [
        0xd76aa478, 0xe8c7b756, 0x242070db, 0xc1bdceee, 0xf57c0faf, 0x4787c62a, 0xa8304613,
        0xfd469501, 0x698098d8, 0x8b44f7af, 0xffff5bb1, 0x895cd7be, 0x6b901122, 0xfd987193,
        0xa679438e, 0x49b40821, 0xf61e2562, 0xc040b340, 0x265e5a51, 0xe9b6c7aa, 0xd62f105d,
        0x02441453, 0xd8a1e681, 0xe7d3fbc8, 0x21e1cde6, 0xc33707d6, 0xf4d50d87, 0x455a14ed,
        0xa9e3e905, 0xfcefa3f8, 0x676f02d9, 0x8d2a4c8a, 0xfffa3942, 0x8771f681, 0x6d9d6122,
        0xfde5380c, 0xa4beea44, 0x4bdecfa9, 0xf6bb4b60, 0xbebfbc70, 0x289b7ec6, 0xeaa127fa,
        0xd4ef3085, 0x04881d05, 0xd9d4d039, 0xe6db99e5, 0x1fa27cf8, 0xc4ac5665, 0xf4292244,
        0x432aff97, 0xab9423a7, 0xfc93a039, 0x655b59c3, 0x8f0ccc92, 0xffeff47d, 0x85845dd1,
        0x6fa87e4f, 0xfe2ce6e0, 0xa3014314, 0x4e0811a1, 0xf7537e82, 0xbd3af235, 0x2ad7d2bb,
        0xeb86d391,
    ];

    let mut msg = input.to_vec();
    let bit_len = (input.len() as u64).wrapping_mul(8);
    msg.push(0x80);
    while (msg.len() % 64) != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_le_bytes());

    let (mut a0, mut b0, mut c0, mut d0): (u32, u32, u32, u32) =
        (0x67452301, 0xefcdab89, 0x98badcfe, 0x10325476);

    for chunk in msg.chunks_exact(64) {
        let mut m = [0u32; 16];
        for (i, word) in m.iter_mut().enumerate() {
            *word = u32::from_le_bytes([
                chunk[i * 4],
                chunk[i * 4 + 1],
                chunk[i * 4 + 2],
                chunk[i * 4 + 3],
            ]);
        }
        let (mut a, mut b, mut c, mut d) = (a0, b0, c0, d0);
        for i in 0..64 {
            let (f, g) = match i {
                0..=15 => ((b & c) | ((!b) & d), i),
                16..=31 => ((d & b) | ((!d) & c), (5 * i + 1) % 16),
                32..=47 => (b ^ c ^ d, (3 * i + 5) % 16),
                _ => (c ^ (b | (!d)), (7 * i) % 16),
            };
            let temp = d;
            d = c;
            c = b;
            b = b.wrapping_add(
                a.wrapping_add(f)
                    .wrapping_add(K[i])
                    .wrapping_add(m[g])
                    .rotate_left(S[i]),
            );
            a = temp;
        }
        a0 = a0.wrapping_add(a);
        b0 = b0.wrapping_add(b);
        c0 = c0.wrapping_add(c);
        d0 = d0.wrapping_add(d);
    }

    let mut out = String::with_capacity(32);
    for w in [a0, b0, c0, d0] {
        for byte in w.to_le_bytes() {
            out.push_str(&format!("{byte:02x}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    #[test]
    fn md5_matches_rfc1321_vector() {
        assert_eq!(md5_hex(b"abc"), "900150983cd24fb0d6963f7d28e17f72");
    }

    #[test]
    #[serial]
    fn absorbs_native_login_and_switches_between_saved_accounts() {
        let dir = tempfile::tempdir().unwrap();
        let home_prev = std::env::var_os("MINIMAX_DATA_DIR");
        let cfg_prev = std::env::var_os("CC_SWITCH_TEST_HOME");
        std::env::set_var("MINIMAX_DATA_DIR", dir.path().join(".minimax"));
        std::env::set_var("CC_SWITCH_TEST_HOME", dir.path());

        let acct_a = StoredMcodeAccount {
            id: "mcode-a".into(),
            name: "Alice".into(),
            email: Some("alice@example.com".into()),
            avatar_url: None,
            region: McodeRegion::Cn,
            access_token: "tok-a".into(),
            refresh_token: "ref-a".into(),
            expires_at_ms: now_ms() + 3_600_000,
            generation: 1,
            login_epoch: Some("ep-a".into()),
            subject: Some("sub-a".into()),
            real_user_id: Some("uid-a".into()),
            needs_reauth: false,
            error: None,
        };
        let acct_b = StoredMcodeAccount {
            id: "mcode-b".into(),
            name: "Bob".into(),
            email: Some("bob@example.com".into()),
            avatar_url: None,
            region: McodeRegion::En,
            access_token: "tok-b".into(),
            refresh_token: "ref-b".into(),
            expires_at_ms: now_ms() + 3_600_000,
            generation: 1,
            login_epoch: Some("ep-b".into()),
            subject: Some("sub-b".into()),
            real_user_id: Some("uid-b".into()),
            needs_reauth: false,
            error: None,
        };

        write_native_account(&acct_a).unwrap();
        let mut store = AccountStore {
            accounts: vec![acct_b.clone()],
            current_account_id: None,
        };
        assert!(sync_native_with_store(&mut store));
        assert_eq!(store.accounts.len(), 2);
        let absorbed_id = store.current_account_id.clone().unwrap();
        assert_ne!(absorbed_id, "mcode-b");

        // Switch to Bob (En region): Cn becomes anonymous, En becomes active.
        write_native_account(&acct_b).unwrap();
        store.current_account_id = Some("mcode-b".into());
        save_store(&store).unwrap();

        assert!(read_native_credential(McodeRegion::Cn).is_none());
        let active_en = read_native_credential(McodeRegion::En).unwrap();
        assert_eq!(active_en.access_token, "tok-b");
        assert_eq!(read_preferred_region(), Some(McodeRegion::En));

        if let Some(prev) = home_prev {
            std::env::set_var("MINIMAX_DATA_DIR", prev);
        } else {
            std::env::remove_var("MINIMAX_DATA_DIR");
        }
        if let Some(prev) = cfg_prev {
            std::env::set_var("CC_SWITCH_TEST_HOME", prev);
        } else {
            std::env::remove_var("CC_SWITCH_TEST_HOME");
        }
    }
}
