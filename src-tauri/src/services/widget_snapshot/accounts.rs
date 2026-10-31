//! 小组件「授权中心」段：全部托管账号，内容和授权中心逐个渠道对齐。
//!
//! 写的是结构化数据（档位名、用量、重置时间、套餐、域名、日期、重置次数、余额、失败原因……），
//! 文案和排版在小组件那边照授权中心做（`macos-widget/Sources/AccountsWidget.swift`），
//! 这样每个渠道各自的信息都能显示：
//! - Claude：套餐（Pro / Max 5x / 20x / Team…）、添加日期、登录快过期 / 可能已过期
//! - Copilot：域名、Enterprise Server、登录日期，高级请求
//! - ChatGPT：登录日期、缺 id_token 时要重新登录，各档 + 存下的重置次数 + Credits 余额
//! - xAI / Antigravity：登录 / 添加日期，凭据失效时要重新登录
//! - Kimi：区域、是不是 Kimi 自己的登录，没订阅写「未订阅」
//!
//! 账号列表是本地数据（每轮都读）；额度要请求各家接口，按账号缓存、分级刷新——有供应商在用的号
//! 每轮（10 分钟）查，其余 30 分钟查一次；要重新登录的号不查。最多 3 个请求并发。

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use once_cell::sync::Lazy;
use serde::Serialize;
use tauri::{AppHandle, Manager};

use crate::app_config::AppType;
use crate::provider::AuthBindingSource;
use crate::services::subscription::{CredentialStatus, QuotaTier, SubscriptionQuota};
use crate::store::AppState;

const IN_USE_MAX_AGE: Duration = Duration::from_secs(10 * 60);
const IDLE_MAX_AGE: Duration = Duration::from_secs(30 * 60);
/// 定时器和上次查询不会刚好对齐：差这么一点也算到期，免得多等一轮。
const AGE_SLACK: Duration = Duration::from_secs(60);
const MAX_CONCURRENT_QUERIES: usize = 3;
/// 单个账号查询的硬上限：只防真卡死（占着并发名额不放）。设得很长，正常查询（包括刷新登录令牌）
/// 不会被中途丢弃；调用方等多久另算（`mod.rs` 的 `REFRESH_STEP_TIMEOUT`）。
const QUERY_HARD_LIMIT: Duration = Duration::from_secs(5 * 60);
/// 失败原因只留这么长：后端错误里可能带响应体或带参数的地址，不整段写进共享目录。
const MAX_MESSAGE_CHARS: usize = 120;

/// 授权中心的分组：(authProvider, 小组件里的 service 键, 显示名)。顺序同授权中心。
const SERVICES: &[(&str, &str, &str)] = &[
    ("claude_official", "claude", "Claude"),
    ("github_copilot", "copilot", "Copilot"),
    ("codex_oauth", "chatgpt", "ChatGPT"),
    ("xai_oauth", "xai", "xAI"),
    ("kimi_oauth", "kimi", "Kimi"),
    ("agy_oauth", "agy", "Antigravity"),
];

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WidgetAccount {
    pub service: String,
    pub service_name: String,
    pub id: String,
    pub login: String,
    /// 有在用的供应商绑着它（agy / Kimi：它们现在登着的号）。
    pub in_use: bool,
    /// 授权中心里的「默认」标签（agy：「agy 在用」，Kimi：「Kimi 在用」）。
    pub is_default: bool,
    pub needs_reauth: bool,
    /// 第二行的原始信息，小组件按渠道拼成「Max 20x · 9月12日添加」这样的一行。
    pub details: AccountDetails,
    /// 绑着这个账号的供应商数（「N 个供应商在用」）。
    pub users: usize,
    pub quota: AccountQuota,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountDetails {
    /// 登录 / 添加时间（Unix 秒）
    pub signed_in_at: Option<i64>,
    /// DeepSeek：账号有名字时第二行写手机号 / 邮箱（已脱敏）
    pub contact: Option<String>,
    /// Copilot：github.com 或 GHES 域名
    pub domain: Option<String>,
    /// Claude：`pro` / `max` / `team` / `enterprise`
    pub plan: Option<String>,
    /// Claude：区分 Max 5x / 20x、Team 高级席位
    pub rate_limit_tier: Option<String>,
    /// Claude：登录大概什么时候过期（Unix 毫秒）
    pub login_expires_at: Option<i64>,
    /// Kimi：`mainland-cn` / `global`
    pub region: Option<String>,
    /// Kimi：是不是 Kimi Code 自己的那个登录
    pub primary: bool,
    /// 需要重新登录的原因：`codexIdToken` / `xaiExpired` / `agyExpired` / `claudeExpired` /
    /// `claudeMaybeExpired` / `kimiSignedOut` / `dshExpired`
    pub reauth_reason: Option<&'static str>,
    /// Kimi 没登录时 Kimi 给的错误（没有时写「未登录」）
    pub error: Option<String>,
}

/// 授权中心账号行右边的额度列。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum AccountQuota {
    /// 还没查过 / 凭据不在这台机器上（和授权中心一样不显示）
    None,
    #[serde(rename_all = "camelCase")]
    Rows {
        tiers: Vec<TierRow>,
        /// ChatGPT 订阅存下的限额重置
        reset_credits: Option<ResetCreditsRow>,
        /// ChatGPT 订阅的 Codex Credits 余额（Credits 数）
        credits_balance: Option<f64>,
        /// 查询时间（Unix 毫秒）
        queried_at: Option<i64>,
    },
    /// 查询失败：`loginExpired` / `tokenRefreshPending` / `other`（带后端给的原因）
    #[serde(rename_all = "camelCase")]
    Failed {
        kind: &'static str,
        message: Option<String>,
    },
    /// 查成功但没有额度档：目前只有 Kimi 没订阅时（`noSubscription`）
    Note { note: &'static str },
    /// DeepSeek 账号的余额和赠金（已带币种符号，如 "¥12.34"），授权中心分两行写
    Balance {
        wallets: Vec<String>,
        bonus: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TierRow {
    /// 档位名（five_hour / seven_day_opus / agy_gemini_weekly / premium …），小组件按它取文案
    pub name: String,
    /// 已用百分比 0–100
    pub utilization: f64,
    pub resets_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResetCreditsRow {
    pub count: usize,
    /// 最早一次到期（ISO 8601）；全都不过期时为空
    pub first_expiry: Option<String>,
    /// 每一次的到期时间（先到期的在前，不过期的为空）：小组件按当前时间再滤一遍，
    /// 下次查询前就有到期的也不会多写
    pub expires_at: Vec<Option<String>>,
}

/// 列出来的账号（还没配额度）。
#[derive(Debug, Clone)]
struct ListedAccount {
    auth_provider: &'static str,
    service: &'static str,
    service_name: &'static str,
    id: String,
    login: String,
    is_default: bool,
    in_use: bool,
    needs_reauth: bool,
    users: usize,
    details: AccountDetails,
}

impl ListedAccount {
    fn key(&self) -> String {
        format!("{}:{}", self.auth_provider, self.id)
    }
}

#[derive(Debug, Clone)]
struct CachedQuota {
    at: Instant,
    quota: AccountQuota,
}

static LISTED: Lazy<Mutex<Option<Vec<ListedAccount>>>> = Lazy::new(|| Mutex::new(None));
static QUOTAS: Lazy<Mutex<HashMap<String, CachedQuota>>> = Lazy::new(|| Mutex::new(HashMap::new()));
/// DeepSeek Harness 的账号（授权中心「DeepSeek Harness」）：只有一个号、状态和余额一次查回来，
/// 不走上面的账号列表；30 分钟查一次。
static DSH: Lazy<Mutex<Option<(Instant, crate::services::dsh_account::DshAccountStatus)>>> =
    Lazy::new(|| Mutex::new(None));
static DSH_IN_FLIGHT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// 余额照授权中心 `DshAccount.tsx` 的 formatWallet：两位小数，人民币 ¥、美元 $，别的币种写在后面。
fn format_wallet(wallet: &crate::services::dsh_account::DshWallet) -> String {
    let text = wallet
        .balance
        .parse::<f64>()
        .ok()
        .filter(|v| v.is_finite())
        .map(|v| format!("{v:.2}"))
        .unwrap_or_else(|| wallet.balance.clone());
    match wallet.currency.as_str() {
        "CNY" => format!("¥{text}"),
        "USD" => format!("${text}"),
        other => format!("{text} {other}"),
    }
}

/// DeepSeek 账号 → 授权中心那一行（没登录时没有这一行）
fn dsh_account(status: &crate::services::dsh_account::DshAccountStatus) -> Option<WidgetAccount> {
    if !status.signed_in {
        return None;
    }
    let profile = status.profile.as_ref();
    let name = profile.and_then(|p| p.name.clone());
    let contact = profile.and_then(|p| p.contact.clone());
    let login = name.clone().or_else(|| contact.clone()).unwrap_or_default();
    let quota = if status.expired {
        AccountQuota::None
    } else if let Some(error) = status.error.clone() {
        AccountQuota::Failed {
            kind: "other",
            message: Some(truncate_message(error)),
        }
    } else {
        AccountQuota::Balance {
            wallets: status.wallets.iter().map(format_wallet).collect(),
            bonus: status.bonus_wallets.iter().map(format_wallet).collect(),
        }
    };
    Some(WidgetAccount {
        service: "dsh".to_string(),
        service_name: "DeepSeek".to_string(),
        id: "default".to_string(),
        login,
        in_use: false,
        is_default: false,
        needs_reauth: status.expired,
        details: AccountDetails {
            // 有名字时第二行写联系方式；只有联系方式时它已经当名字用了
            contact: name.and(contact),
            reauth_reason: status.expired.then_some("dshExpired"),
            ..Default::default()
        },
        users: 0,
        quota,
    })
}

/// 到期就后台查一次 DeepSeek 账号（任务自己跑完，不被调用方超时打断）
fn spawn_dsh_query_if_due() -> Option<tauri::async_runtime::JoinHandle<()>> {
    let due = lock(&DSH)
        .as_ref()
        .is_none_or(|(at, _)| at.elapsed() + AGE_SLACK >= IDLE_MAX_AGE);
    if !due || DSH_IN_FLIGHT.swap(true, std::sync::atomic::Ordering::AcqRel) {
        return None;
    }
    Some(tauri::async_runtime::spawn(async move {
        let _permit = QUERY_SLOTS.clone().acquire_owned().await;
        let started = Instant::now();
        match tokio::time::timeout(QUERY_HARD_LIMIT, crate::services::dsh_account::status()).await {
            Ok(Ok(status)) => {
                log::info!(
                    "[Widget] DeepSeek 账号查询完成（{} ms）",
                    started.elapsed().as_millis()
                );
                *lock(&DSH) = Some((Instant::now(), status));
            }
            Ok(Err(e)) => log::info!("[Widget] DeepSeek 账号查询失败: {e}"),
            Err(_) => log::info!("[Widget] DeepSeek 账号查询超时"),
        }
        DSH_IN_FLIGHT.store(false, std::sync::atomic::Ordering::Release);
    }))
}

/// 正在查的账号（同一个号同时只查一份）
static IN_FLIGHT: Lazy<Mutex<HashSet<String>>> = Lazy::new(|| Mutex::new(HashSet::new()));
static QUERY_SLOTS: Lazy<std::sync::Arc<tokio::sync::Semaphore>> =
    Lazy::new(|| std::sync::Arc::new(tokio::sync::Semaphore::new(MAX_CONCURRENT_QUERIES)));

fn truncate_message(message: String) -> String {
    if message.chars().count() <= MAX_MESSAGE_CHARS {
        return message;
    }
    let mut short: String = message.chars().take(MAX_MESSAGE_CHARS).collect();
    short.push('…');
    short
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// 一条供应商绑定：(authProvider, 账号 id；None = 跟随默认账号)。
type Binding = (String, Option<String>);

fn provider_bindings(provider: &crate::provider::Provider) -> Vec<Binding> {
    let Some(meta) = provider.meta.as_ref() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    if let Some(binding) = meta.auth_binding.as_ref() {
        if binding.source == AuthBindingSource::ManagedAccount {
            if let Some(auth_provider) = binding.auth_provider.clone() {
                let account = binding
                    .account_id
                    .as_deref()
                    .map(str::trim)
                    .filter(|id| !id.is_empty())
                    .map(str::to_string);
                out.push((auth_provider, account));
            }
        }
    }
    if let Some(account) = meta.github_account_id.clone() {
        out.push(("github_copilot".to_string(), Some(account)));
    }
    out
}

/// 各应用在用的那家的绑定（「在用」），以及全部供应商的绑定（「N 个供应商在用」）。
fn collect_bindings(state: &AppState) -> (HashSet<Binding>, Vec<Binding>) {
    let mut in_use = HashSet::new();
    let mut all = Vec::new();
    for app in AppType::all() {
        let current = crate::mode::current::provider_for(
            &state.db,
            &app,
            crate::mode::current::Purpose::InUse,
        )
        .ok()
        .flatten();
        let Ok(providers) = state.db.get_all_providers(app.as_str()) else {
            continue;
        };
        for (id, provider) in &providers {
            let bindings = provider_bindings(provider);
            if current.as_deref() == Some(id.as_str()) {
                in_use.extend(bindings.iter().cloned());
            }
            all.extend(bindings);
        }
    }
    (in_use, all)
}

fn binds(binding: &Binding, auth_provider: &str, id: &str, is_default: bool) -> bool {
    binding.0 == auth_provider
        && match &binding.1 {
            Some(account) => account == id,
            None => is_default,
        }
}

fn is_in_use(bindings: &HashSet<Binding>, auth_provider: &str, id: &str, is_default: bool) -> bool {
    bindings
        .iter()
        .any(|b| binds(b, auth_provider, id, is_default))
}

fn count_users(all: &[Binding], auth_provider: &str, id: &str, is_default: bool) -> usize {
    all.iter()
        .filter(|b| binds(b, auth_provider, id, is_default))
        .count()
}

async fn list_accounts(app: &AppHandle) -> Vec<ListedAccount> {
    let Some(state) = app.try_state::<AppState>() else {
        return Vec::new();
    };
    let (in_use_bindings, all_bindings) = collect_bindings(state.inner());
    let now_ms = chrono::Utc::now().timestamp_millis();
    let mut out = Vec::new();
    for &(auth_provider, service, service_name) in SERVICES {
        let mut push = |id: String,
                        login: String,
                        is_default: bool,
                        in_use: bool,
                        needs_reauth: bool,
                        users: usize,
                        details: AccountDetails| {
            out.push(ListedAccount {
                auth_provider,
                service,
                service_name,
                id,
                login,
                is_default,
                in_use,
                needs_reauth,
                users,
                details,
            });
        };

        if auth_provider == "kimi_oauth" {
            let Some(kimi) = app.try_state::<crate::services::kimi_oauth::KimiOAuthManager>()
            else {
                continue;
            };
            match kimi.status().await {
                Ok(status) => {
                    for account in status.accounts {
                        let current =
                            status.current_account_id.as_deref() == Some(account.id.as_str());
                        let region = serde_json::to_value(account.region)
                            .ok()
                            .and_then(|v| v.as_str().map(str::to_string));
                        let details = AccountDetails {
                            region,
                            primary: account.primary,
                            reauth_reason: (!account.authenticated).then_some("kimiSignedOut"),
                            error: account.error.clone().map(truncate_message),
                            ..Default::default()
                        };
                        push(
                            account.id,
                            account.nickname.unwrap_or(account.name),
                            current,
                            current,
                            !account.authenticated,
                            0,
                            details,
                        );
                    }
                }
                Err(e) => log::debug!("[Widget] 读取 Kimi 账号失败: {e}"),
            }
            continue;
        }
        if auth_provider == "agy_oauth" && !cfg!(target_os = "macos") {
            continue;
        }
        let accounts = crate::commands::auth_list_accounts(
            auth_provider.to_string(),
            app.state(),
            app.state(),
            app.state(),
            app.state(),
            app.state(),
        )
        .await;
        let accounts = match accounts {
            Ok(accounts) => accounts,
            Err(e) => {
                log::debug!("[Widget] 读取 {service_name} 账号失败: {e}");
                continue;
            }
        };
        for account in accounts {
            let signed_in_at = (account.authenticated_at > 0).then_some(account.authenticated_at);
            let mut details = AccountDetails {
                signed_in_at,
                ..Default::default()
            };
            let needs_reauth = match auth_provider {
                "codex_oauth" => {
                    details.reauth_reason = account.reauth_required.then_some("codexIdToken");
                    account.reauth_required
                }
                "xai_oauth" => {
                    details.reauth_reason = account.requires_reauth.then_some("xaiExpired");
                    account.requires_reauth
                }
                "agy_oauth" => {
                    details.reauth_reason = account.requires_reauth.then_some("agyExpired");
                    account.requires_reauth
                }
                "claude_official" => {
                    details.plan = account.plan.clone();
                    details.rate_limit_tier = account.rate_limit_tier.clone();
                    details.login_expires_at = account.login_expires_at;
                    // 记下的过期时间可能偏早，所以授权中心只写「可能已过期」
                    let expired = account.login_expires_at.is_some_and(|at| at <= now_ms);
                    details.reauth_reason = if account.requires_reauth {
                        Some("claudeExpired")
                    } else if expired {
                        Some("claudeMaybeExpired")
                    } else {
                        None
                    };
                    account.requires_reauth || expired
                }
                _ => {
                    details.domain = Some(account.github_domain.clone());
                    account.reauth_required
                }
            };
            // agy 的「默认」就是 agy 现在登着的号
            let in_use = (auth_provider == "agy_oauth" && account.is_default)
                || is_in_use(
                    &in_use_bindings,
                    auth_provider,
                    &account.id,
                    account.is_default,
                );
            // Claude 只算明确绑它的卡（没有「默认账号」）
            let users = count_users(
                &all_bindings,
                auth_provider,
                &account.id,
                account.is_default,
            );
            push(
                account.id,
                account.login,
                account.is_default,
                in_use,
                needs_reauth,
                users,
                details,
            );
        }
    }
    out
}

/// Copilot 的用量接口不是 `SubscriptionQuota`：按前端 `useCopilotQuota` 折成一档 premium。
fn copilot_quota(
    usage: crate::proxy::providers::copilot_auth::CopilotUsageResponse,
) -> SubscriptionQuota {
    let premium = usage.quota_snapshots.premium_interactions;
    let tiers = if premium.unlimited || premium.entitlement <= 0 {
        Vec::new()
    } else {
        let used = (premium.entitlement - premium.remaining) as f64;
        vec![QuotaTier {
            name: "premium".to_string(),
            utilization: used / premium.entitlement as f64 * 100.0,
            resets_at: Some(usage.quota_reset_date),
            used_value_usd: None,
            max_value_usd: None,
        }]
    };
    SubscriptionQuota {
        tool: "github_copilot".to_string(),
        credential_status: CredentialStatus::Valid,
        credential_message: None,
        success: true,
        tiers,
        extra_usage: None,
        reset_credits: None,
        credits_balance: None,
        error: None,
        queried_at: Some(chrono::Utc::now().timestamp_millis()),
    }
}

async fn query(app: &AppHandle, account: &ListedAccount) -> Result<SubscriptionQuota, String> {
    let id = account.id.clone();
    match account.auth_provider {
        // 和卡片同一条路：结果也会按账号写进托盘缓存
        "claude_official" => {
            crate::commands::get_claude_account_quota(app.clone(), app.state(), id)
                .await
                .map(|q| q.quota)
        }
        "github_copilot" => crate::commands::copilot_get_usage_for_account(id, app.state())
            .await
            .map(copilot_quota),
        "codex_oauth" => {
            crate::commands::get_codex_oauth_quota(app.clone(), app.state(), Some(id), app.state())
                .await
        }
        "xai_oauth" => crate::commands::get_xai_oauth_quota(Some(id), app.state()).await,
        "kimi_oauth" => crate::commands::kimi_oauth_quota(id, app.state()).await,
        "agy_oauth" => crate::commands::agy_account_quota(id, app.state()).await,
        other => Err(format!("unknown auth provider {other}")),
    }
}

/// 查询结果 → 额度列，照授权中心 `subscriptionQuotaState` / `quotaRows`：
/// 没凭据不显示；失败写原因；成功列各档，再加存下的重置次数和 Credits 余额；
/// Kimi 查成功却一档都没有写「未订阅」。
fn account_quota(
    auth_provider: &str,
    result: Result<SubscriptionQuota, String>,
    now: chrono::DateTime<chrono::Utc>,
) -> AccountQuota {
    let quota = match result {
        Ok(quota) => quota,
        Err(message) => {
            return AccountQuota::Failed {
                kind: "other",
                message: Some(truncate_message(message)),
            }
        }
    };
    if matches!(
        quota.credential_status,
        CredentialStatus::NotFound | CredentialStatus::ParseError
    ) {
        return AccountQuota::None;
    }
    if !quota.success {
        let kind = match quota.credential_status {
            CredentialStatus::Expired => "loginExpired",
            CredentialStatus::RefreshPending => "tokenRefreshPending",
            _ => "other",
        };
        return AccountQuota::Failed {
            kind,
            message: (kind == "other")
                .then(|| quota.error.clone())
                .flatten()
                .map(truncate_message),
        };
    }
    let tiers: Vec<TierRow> = quota
        .tiers
        .iter()
        .filter(|tier| tier.utilization.is_finite())
        .map(|tier| TierRow {
            name: tier.name.clone(),
            utilization: tier.utilization.clamp(0.0, 100.0),
            resets_at: tier.resets_at.clone(),
        })
        .collect();
    if tiers.is_empty() {
        return if auth_provider == "kimi_oauth" {
            AccountQuota::Note {
                note: "noSubscription",
            }
        } else {
            AccountQuota::None
        };
    }
    // 过期的重置次数不算（后端已按到期先后排好，不过期的在最后）
    let reset_credits = quota.reset_credits.as_ref().and_then(|credits| {
        let left: Vec<&Option<String>> = credits
            .expires_at
            .iter()
            .filter(|at| {
                at.as_deref()
                    .and_then(|at| chrono::DateTime::parse_from_rfc3339(at).ok())
                    .is_none_or(|at| at > now)
            })
            .collect();
        (!left.is_empty()).then(|| ResetCreditsRow {
            count: left.len(),
            first_expiry: left[0].clone(),
            expires_at: left.iter().map(|at| (*at).clone()).collect(),
        })
    });
    AccountQuota::Rows {
        tiers,
        reset_credits,
        credits_balance: quota
            .credits_balance
            .filter(|balance| balance.is_finite() && *balance > 0.0),
        queried_at: quota.queried_at,
    }
}

fn is_due(cached: Option<&CachedQuota>, in_use: bool, now: Instant) -> bool {
    let max_age = if in_use { IN_USE_MAX_AGE } else { IDLE_MAX_AGE };
    cached.is_none_or(|c| now.duration_since(c.at) + AGE_SLACK >= max_age)
}

/// 重列账号（本地数据，很快）。之后用 `query_due` 查额度。启动后先跑这一步再写快照，小组件从第一份快照起就有账号，
/// 不会因为账号段还空着而误以为主程序太旧。
pub async fn list(app: &AppHandle) {
    let listed = list_accounts(app).await;
    // 删掉的账号别留在缓存里
    {
        let keys: HashSet<String> = listed.iter().map(ListedAccount::key).collect();
        lock(&QUOTAS).retain(|key, _| keys.contains(key));
    }
    *lock(&LISTED) = Some(listed);
}

/// 给到期的账号查额度（按上次列出的账号）。
///
/// 每个账号的查询在自己的任务里跑完，查完马上写进缓存：调用方超时不再等时，已查完的结果不丢，
/// 正在跑的也不会被中途丢弃——查询里可能在刷新会轮换的登录令牌（ChatGPT、Claude 等），
/// 中途丢弃可能刷新了却没保存，账号就掉登录了。同一个账号同时只查一份。
pub async fn query_due(app: &AppHandle) {
    let Some(listed) = lock(&LISTED).clone() else {
        return;
    };
    let now = Instant::now();
    let due: Vec<ListedAccount> = {
        let quotas = lock(&QUOTAS);
        let mut in_flight = lock(&IN_FLIGHT);
        listed
            .into_iter()
            .filter(|a| !a.needs_reauth && is_due(quotas.get(&a.key()), a.in_use, now))
            .filter(|a| in_flight.insert(a.key()))
            .collect()
    };

    let handles: Vec<_> = due
        .into_iter()
        .map(|account| {
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                let _permit = QUERY_SLOTS.clone().acquire_owned().await;
                let started = Instant::now();
                let result =
                    match tokio::time::timeout(QUERY_HARD_LIMIT, query(&app, &account)).await {
                        Ok(result) => result,
                        Err(_) => Err(format!("timed out after {}s", QUERY_HARD_LIMIT.as_secs())),
                    };
                log::info!(
                    "[Widget] {} 账号额度查询{}（{} ms）",
                    account.service_name,
                    if result.is_ok() { "完成" } else { "失败" },
                    started.elapsed().as_millis()
                );
                let quota = account_quota(account.auth_provider, result, chrono::Utc::now());
                let key = account.key();
                lock(&QUOTAS).insert(
                    key.clone(),
                    CachedQuota {
                        at: Instant::now(),
                        quota,
                    },
                );
                lock(&IN_FLIGHT).remove(&key);
            })
        })
        .collect();
    let mut handles = handles;
    handles.extend(spawn_dsh_query_if_due());
    futures::future::join_all(handles).await;
}

/// 按上次列出的账号和缓存的额度拼出快照段；还没列过时为 None。
pub fn snapshot() -> Option<Vec<WidgetAccount>> {
    let listed = lock(&LISTED).clone()?;
    let quotas = lock(&QUOTAS);
    Some(
        listed
            .into_iter()
            .map(|account| {
                // 要重新登录的号不显示额度（授权中心同样不传额度列）
                let quota = if account.needs_reauth {
                    AccountQuota::None
                } else {
                    quotas
                        .get(&account.key())
                        .map(|c| c.quota.clone())
                        .unwrap_or(AccountQuota::None)
                };
                WidgetAccount {
                    service: account.service.to_string(),
                    service_name: account.service_name.to_string(),
                    id: account.id,
                    login: account.login,
                    in_use: account.in_use,
                    is_default: account.is_default,
                    needs_reauth: account.needs_reauth,
                    details: account.details,
                    users: account.users,
                    quota,
                }
            })
            .chain(
                lock(&DSH)
                    .as_ref()
                    .and_then(|(_, status)| dsh_account(status)),
            )
            .collect(),
    )
}

/// Antigravity 官方登录的额度档位：卡片绑了账号就用它，没绑用 agy 现在登着的号；还没查到时为 None。
pub fn agy_tiers(bound: Option<&str>) -> Option<Vec<TierRow>> {
    let listed = lock(&LISTED).clone()?;
    let account = listed.iter().find(|a| {
        a.auth_provider == "agy_oauth"
            && match bound {
                Some(id) => a.id == id,
                None => a.is_default,
            }
    })?;
    match &lock(&QUOTAS).get(&account.key())?.quota {
        AccountQuota::Rows { tiers, .. } => Some(tiers.clone()),
        _ => None,
    }
}

/// 关掉小组件时清空，下次打开重新查。
pub fn clear() {
    *lock(&LISTED) = None;
    lock(&QUOTAS).clear();
    *lock(&DSH) = None;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::subscription::ResetCredits;

    fn quota(tiers: Vec<QuotaTier>) -> SubscriptionQuota {
        SubscriptionQuota {
            tool: String::new(),
            credential_status: CredentialStatus::Valid,
            credential_message: None,
            success: true,
            tiers,
            extra_usage: None,
            reset_credits: None,
            credits_balance: None,
            error: None,
            queried_at: Some(1),
        }
    }

    fn tier(name: &str, used: f64) -> QuotaTier {
        QuotaTier {
            name: name.to_string(),
            utilization: used,
            resets_at: None,
            used_value_usd: None,
            max_value_usd: None,
        }
    }

    #[test]
    fn in_use_accounts_refresh_more_often() {
        // 只往后加：Windows 的 Instant 从开机算起，CI 机器刚开机时往前减 30 分钟会溢出 panic
        let start = Instant::now();
        let now = start + Duration::from_secs(60 * 60);
        let cached = |age: u64| CachedQuota {
            at: start + Duration::from_secs(60 * 60 - age),
            quota: AccountQuota::None,
        };
        assert!(is_due(None, false, now));
        assert!(is_due(Some(&cached(10 * 60)), true, now));
        assert!(!is_due(Some(&cached(10 * 60)), false, now));
        assert!(is_due(Some(&cached(30 * 60)), false, now));
        assert!(!is_due(Some(&cached(60)), true, now));
    }

    #[test]
    fn binding_without_account_follows_default() {
        let mut bindings = HashSet::new();
        bindings.insert(("codex_oauth".to_string(), None));
        bindings.insert(("claude_official".to_string(), Some("acc-2".to_string())));
        assert!(is_in_use(&bindings, "codex_oauth", "a", true));
        assert!(!is_in_use(&bindings, "codex_oauth", "b", false));
        assert!(is_in_use(&bindings, "claude_official", "acc-2", false));
        assert!(!is_in_use(&bindings, "claude_official", "acc-1", false));
        assert!(!is_in_use(&bindings, "xai_oauth", "a", true));

        let all: Vec<Binding> = vec![
            ("codex_oauth".to_string(), Some("a".to_string())),
            ("codex_oauth".to_string(), None),
            ("codex_oauth".to_string(), Some("b".to_string())),
        ];
        assert_eq!(count_users(&all, "codex_oauth", "a", true), 2);
        assert_eq!(count_users(&all, "codex_oauth", "b", false), 1);
    }

    #[test]
    fn every_tier_is_kept_like_the_auth_center() {
        use crate::services::subscription_agy as agy;
        let q = quota(vec![
            tier("five_hour", 10.0),
            tier("seven_day", 40.0),
            tier("seven_day_opus", 95.0),
            tier(agy::TIER_AGY_GEMINI_WEEKLY, 0.0),
        ]);
        let AccountQuota::Rows { tiers, .. } =
            account_quota("claude_official", Ok(q), chrono::Utc::now())
        else {
            panic!("expected rows");
        };
        // 不像托盘那样把每周的几档合成一行
        let names: Vec<_> = tiers.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "five_hour",
                "seven_day",
                "seven_day_opus",
                agy::TIER_AGY_GEMINI_WEEKLY
            ]
        );
    }

    #[test]
    fn chatgpt_extras_and_kimi_note() {
        let now = chrono::Utc::now();
        let past = (now - chrono::Duration::days(1)).to_rfc3339();
        let future = (now + chrono::Duration::days(3)).to_rfc3339();
        let mut q = quota(vec![tier("five_hour", 20.0)]);
        q.reset_credits = Some(ResetCredits {
            expires_at: vec![Some(past), Some(future.clone()), None],
        });
        q.credits_balance = Some(1250.0);
        let AccountQuota::Rows {
            reset_credits,
            credits_balance,
            ..
        } = account_quota("codex_oauth", Ok(q), now)
        else {
            panic!("expected rows");
        };
        // 过期的那次不算
        assert_eq!(
            reset_credits,
            Some(ResetCreditsRow {
                count: 2,
                first_expiry: Some(future.clone()),
                expires_at: vec![Some(future), None],
            })
        );
        assert_eq!(credits_balance, Some(1250.0));

        assert_eq!(
            account_quota("kimi_oauth", Ok(quota(vec![])), now),
            AccountQuota::Note {
                note: "noSubscription"
            }
        );
        assert_eq!(
            account_quota("xai_oauth", Ok(quota(vec![])), now),
            AccountQuota::None
        );
    }

    #[test]
    fn failures_and_missing_credentials() {
        let now = chrono::Utc::now();
        let mut expired = quota(vec![]);
        expired.success = false;
        expired.credential_status = CredentialStatus::Expired;
        assert_eq!(
            account_quota("claude_official", Ok(expired), now),
            AccountQuota::Failed {
                kind: "loginExpired",
                message: None
            }
        );
        let mut missing = quota(vec![]);
        missing.credential_status = CredentialStatus::NotFound;
        assert_eq!(
            account_quota("kimi_oauth", Ok(missing), now),
            AccountQuota::None
        );
        assert_eq!(
            account_quota("xai_oauth", Err("boom".into()), now),
            AccountQuota::Failed {
                kind: "other",
                message: Some("boom".into())
            }
        );
    }

    #[test]
    fn quota_serializes_with_state_tag() {
        let json = serde_json::to_value(AccountQuota::Note {
            note: "noSubscription",
        })
        .unwrap();
        assert_eq!(json["state"], "note");
        let json = serde_json::to_value(AccountQuota::Rows {
            tiers: vec![],
            reset_credits: None,
            credits_balance: None,
            queried_at: None,
        })
        .unwrap();
        assert_eq!(json["state"], "rows");
        assert!(json.get("resetCredits").is_some());
    }

    #[test]
    fn long_failure_messages_are_truncated() {
        let short = "HTTP 429".to_string();
        assert_eq!(truncate_message(short.clone()), short);
        let long = "x".repeat(500);
        let cut = truncate_message(long);
        assert_eq!(cut.chars().count(), MAX_MESSAGE_CHARS + 1);
        assert!(cut.ends_with('…'));
    }

    #[test]
    fn dsh_account_mirrors_the_auth_center_row() {
        use crate::services::dsh_account::{DshAccountProfile, DshAccountStatus, DshWallet};
        let wallet = |currency: &str, balance: &str| DshWallet {
            currency: currency.to_string(),
            balance: balance.to_string(),
        };
        assert!(dsh_account(&DshAccountStatus::default()).is_none());

        let status = DshAccountStatus {
            signed_in: true,
            profile: Some(DshAccountProfile {
                id: None,
                name: Some("Owl".into()),
                contact: Some("138****0000".into()),
                avatar_url: None,
            }),
            wallets: vec![wallet("CNY", "12.3"), wallet("USD", "abc")],
            bonus_wallets: vec![wallet("EUR", "1")],
            ..Default::default()
        };
        let account = dsh_account(&status).unwrap();
        assert_eq!(account.login, "Owl");
        assert_eq!(account.details.contact.as_deref(), Some("138****0000"));
        assert_eq!(
            account.quota,
            AccountQuota::Balance {
                wallets: vec!["¥12.30".into(), "$abc".into()],
                bonus: vec!["1.00 EUR".into()],
            }
        );

        let expired = DshAccountStatus {
            expired: true,
            ..status.clone()
        };
        let account = dsh_account(&expired).unwrap();
        assert!(account.needs_reauth);
        assert_eq!(account.details.reauth_reason, Some("dshExpired"));
        assert_eq!(account.quota, AccountQuota::None);

        let failed = DshAccountStatus {
            error: Some("boom".into()),
            ..status
        };
        assert!(matches!(
            dsh_account(&failed).unwrap().quota,
            AccountQuota::Failed { .. }
        ));
    }
}
