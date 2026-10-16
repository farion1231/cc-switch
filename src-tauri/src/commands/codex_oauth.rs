//! Codex OAuth Tauri Commands
//!
//! 提供 OpenAI ChatGPT Plus/Pro OAuth 认证相关的 Tauri 命令。
//!
//! 大部分认证命令通过通用 `auth_*` 命令（参见 `commands::auth`）暴露给前端，
//! 此处定义 State wrapper 以及 Codex OAuth 专属的订阅额度和模型列表查询命令。

use crate::database::CodexQuotaActivationRecord;
use crate::proxy::providers::codex_oauth_auth::CodexOAuthManager;
use crate::services::codex_quota_activation::{try_lock_account, ActivationDecision};
use crate::services::codex_quota_activation_http::{
    send_activation_once, ActivationHttpStatus, ACTIVATION_ENDPOINT,
};
use crate::services::model_fetch::FetchedModel;
use crate::services::subscription::{query_codex_quota, CredentialStatus, SubscriptionQuota};
use crate::services::usage_cache::CodexOauthCachePublication;
use serde::Serialize;
use std::sync::Arc;
use tauri::State;
use uuid::Uuid;

/// Codex OAuth 认证状态
///
/// `CodexOAuthManager` 内部已使用细粒度锁且所有方法均为 `&self`，因此这里
/// 直接持有 `Arc`，不再包一层 `RwLock`——避免任一命令持有粗粒度锁跨网络刷新
/// 时阻塞其他命令（切换 / 认证中心操作 / token 读取）。
pub struct CodexOAuthState(pub Arc<CodexOAuthManager>);

/// Read-only quota responses carry an explicit lifecycle outcome when the
/// backend cache rejected publication because the account generation changed
/// while the request was in flight.  The quota payload is kept flattened for
/// compatibility with existing frontend consumers.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexOauthQuotaResult {
    #[serde(flatten)]
    quota: SubscriptionQuota,
    #[serde(skip_serializing_if = "Option::is_none")]
    lifecycle: Option<&'static str>,
}

fn codex_oauth_quota_result(quota: SubscriptionQuota, published: bool) -> CodexOauthQuotaResult {
    CodexOauthQuotaResult {
        quota,
        lifecycle: (!published).then_some("superseded"),
    }
}

/// 查询 Codex OAuth (ChatGPT Plus/Pro) 订阅额度
///
/// - `account_id` 未指定时回退到 `CodexOAuthManager` 的默认账号
/// - 没有任何账号时返回 `not_found`，前端 `SubscriptionQuotaView` 会静默不渲染
/// - 复用 `services::subscription::query_codex_quota`，因此 wham/usage 端点协议
///   与 Codex CLI 路径完全一致
#[tauri::command(rename_all = "camelCase")]
pub async fn get_codex_oauth_quota(
    app: tauri::AppHandle,
    app_state: State<'_, crate::store::AppState>,
    account_id: Option<String>,
    state: State<'_, CodexOAuthState>,
) -> Result<CodexOauthQuotaResult, String> {
    let manager = &state.0;

    // 解析最终使用的账号 ID：显式 > 默认账号 > 无账号 (not_found)
    let resolved = match account_id {
        Some(id) => Some(id.trim().to_string()),
        None => manager.default_account_id().await,
    };
    let Some(id) = resolved else {
        return Ok(codex_oauth_quota_result(
            SubscriptionQuota::not_found("codex_oauth"),
            true,
        ));
    };

    let cache_generation = app_state.usage_cache.codex_oauth_generation(&id);
    let result = query_codex_oauth_quota_for(manager, &id).await;
    // Cache by the resolved account, even if the default/binding changes while
    // the request is in flight. Transport errors retain the last good snapshot;
    // authentication/HTTP failures replace it so the tray hides invalid quotas.
    match result {
        Ok(quota) => {
            let published = app_state.usage_cache.put_codex_oauth_if_generation(
                id,
                cache_generation,
                quota.clone(),
            );
            if published {
                crate::tray::schedule_tray_refresh(&app);
            }
            Ok(codex_oauth_quota_result(quota, published))
        }
        Err(error) => Err(error),
    }
}

async fn query_codex_oauth_quota_for(
    manager: &CodexOAuthManager,
    id: &str,
) -> Result<SubscriptionQuota, String> {
    // 获取（必要时自动刷新）access_token
    let token = match manager.get_valid_token_for_account(id).await {
        Ok(t) => t,
        Err(e) => {
            return Ok(SubscriptionQuota::error(
                "codex_oauth",
                CredentialStatus::Expired,
                format!("Codex OAuth token unavailable: {e}"),
            ));
        }
    };
    let chatgpt_account_id = manager
        .chatgpt_account_id_for_account(id)
        .await
        .map_err(|e| e.to_string())?;

    // 瞬时传输失败以 Err 传播（前端 reject → retry + 保留上次成功值）。
    query_codex_quota(
        &token,
        Some(&chatgpt_account_id),
        "codex_oauth",
        "Codex OAuth access token expired or rejected. Please re-login via cc-switch.",
    )
    .await
}

/// 获取 Codex OAuth (ChatGPT Plus/Pro) 可用模型列表
///
/// ChatGPT Codex 反代使用 `chatgpt.com/backend-api/codex/*`，不是 OpenAI 兼容
/// `/v1/models`。这里复用托管 OAuth 账号的 access_token，直接读取 Codex 后端
/// 暴露的模型列表端点。
#[tauri::command(rename_all = "camelCase")]
pub async fn get_codex_oauth_models(
    account_id: Option<String>,
    state: State<'_, CodexOAuthState>,
) -> Result<Vec<FetchedModel>, String> {
    let manager = &state.0;
    let resolved = match account_id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty())
    {
        Some(id) => Some(id.to_string()),
        None => manager.default_account_id().await,
    };
    let Some(id) = resolved else {
        return Err("No ChatGPT account available".to_string());
    };

    let token = manager
        .get_valid_token_for_account(&id)
        .await
        .map_err(|e| format!("Codex OAuth token unavailable: {e}"))?;
    let chatgpt_account_id = manager
        .chatgpt_account_id_for_account(&id)
        .await
        .map_err(|e| e.to_string())?;

    crate::services::codex_oauth_models::fetch_models_with_token(&token, &chatgpt_account_id).await
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexQuotaActivationPollResult {
    pub account_id: String,
    pub polled: bool,
    pub attempted: bool,
    pub target: Option<String>,
    pub status: String,
    pub reason: Option<String>,
    pub quota: Option<SubscriptionQuota>,
}

#[tauri::command(rename_all = "camelCase")]
pub async fn get_codex_quota_activation_state(
    account_id: String,
    app_state: State<'_, crate::store::AppState>,
    oauth_state: State<'_, CodexOAuthState>,
) -> Result<Option<CodexQuotaActivationRecord>, String> {
    let mut record = app_state
        .db
        .get_codex_quota_activation_state(account_id.trim())
        .map_err(|error| error.to_string())?;
    if let Some(record) = &mut record {
        record.effective_interval_minutes =
            derive_activation_interval(&app_state.db, &oauth_state.0, &record.account_id).await?;
    }
    Ok(record)
}

#[tauri::command(rename_all = "camelCase")]
pub async fn list_codex_quota_activation_states(
    app_state: State<'_, crate::store::AppState>,
    oauth_state: State<'_, CodexOAuthState>,
) -> Result<Vec<CodexQuotaActivationRecord>, String> {
    let states = app_state
        .db
        .list_codex_quota_activation_states()
        .map_err(|error| error.to_string())?;
    let mut refreshed = Vec::with_capacity(states.len());
    for mut state in states {
        let interval =
            derive_activation_interval(&app_state.db, &oauth_state.0, &state.account_id).await?;
        state.effective_interval_minutes = interval;
        refreshed.push(state);
    }
    Ok(refreshed)
}

#[tauri::command(rename_all = "camelCase")]
pub async fn set_codex_quota_activation_enabled(
    account_id: String,
    enabled: bool,
    effective_interval_minutes: Option<u64>,
    app_state: State<'_, crate::store::AppState>,
    oauth_state: State<'_, CodexOAuthState>,
) -> Result<CodexQuotaActivationRecord, String> {
    let account_id = account_id.trim();
    if account_id.is_empty() {
        return Err("A specific ChatGPT account is required".to_string());
    }
    let _frontend_interval_is_untrusted = effective_interval_minutes;
    let effective_interval_minutes =
        derive_activation_interval(&app_state.db, &oauth_state.0, account_id).await?;
    if enabled && !effective_interval_minutes.is_some_and(|interval| interval > 0) {
        return Err(
            "No enabled official Codex quota interval is available for this account".to_string(),
        );
    }
    let record = app_state
        .db
        .set_codex_quota_activation_enabled(
            account_id,
            enabled,
            effective_interval_minutes,
            chrono::Utc::now().timestamp_millis(),
        )
        .map_err(|error| error.to_string())?;
    // Enabling, disabling, and re-enabling all create a new authorization
    // generation.  Drop any quota request that was started by the prior one.
    app_state.usage_cache.invalidate_codex_oauth(account_id);
    Ok(record)
}

/// Explicit automatic polling entry. The frontend timer only requests this
/// command; the backend rechecks the account, enabled state and interval.
/// Manual quota commands never call it.
#[tauri::command(rename_all = "camelCase")]
pub async fn poll_codex_oauth_quota_for_activation(
    app: tauri::AppHandle,
    account_id: String,
    effective_interval_minutes: Option<u64>,
    app_state: State<'_, crate::store::AppState>,
    state: State<'_, CodexOAuthState>,
) -> Result<CodexQuotaActivationPollResult, String> {
    let account_id = account_id.trim().to_string();
    if account_id.is_empty() {
        return Err("A specific ChatGPT account is required".to_string());
    }
    let Some(_account_guard) = try_lock_account(&account_id) else {
        return Ok(CodexQuotaActivationPollResult {
            account_id,
            polled: false,
            attempted: false,
            target: None,
            status: "busy".to_string(),
            reason: Some("another account poll is still running".to_string()),
            quota: None,
        });
    };
    let now = chrono::Utc::now().timestamp_millis();
    let _frontend_interval_is_untrusted = effective_interval_minutes;
    let effective_interval_minutes =
        derive_activation_interval(&app_state.db, &state.0, &account_id).await?;
    if !app_state
        .db
        .try_begin_codex_quota_activation_poll(&account_id, effective_interval_minutes, now)
        .map_err(|error| error.to_string())?
    {
        return Ok(CodexQuotaActivationPollResult {
            account_id,
            polled: false,
            attempted: false,
            target: None,
            status: "not_due".to_string(),
            reason: None,
            quota: None,
        });
    }
    let Some((enabled, authorization_revision)) = app_state
        .db
        .codex_quota_activation_revision(&account_id)
        .map_err(|error| error.to_string())?
    else {
        return Ok(CodexQuotaActivationPollResult {
            account_id,
            polled: false,
            attempted: false,
            target: None,
            status: "disabled".to_string(),
            reason: Some("activation_not_enabled".to_string()),
            quota: None,
        });
    };
    if !enabled {
        return Ok(CodexQuotaActivationPollResult {
            account_id,
            polled: false,
            attempted: false,
            target: None,
            status: "disabled".to_string(),
            reason: Some("activation_not_enabled".to_string()),
            quota: None,
        });
    }

    let token = match state.0.get_valid_token_for_account(&account_id).await {
        Ok(token) => token,
        Err(_) => {
            return Ok(failed_poll(
                account_id,
                "token_unavailable",
                "指定账号 token 不可用".to_string(),
            ));
        }
    };
    if !activation_revision_is_current(&app_state.db, &account_id, authorization_revision)? {
        return Ok(disabled_poll(account_id, "authorization_changed"));
    }
    let workspace = match state.0.chatgpt_account_id_for_account(&account_id).await {
        Ok(workspace) => workspace,
        Err(_) => {
            return Ok(failed_poll(
                account_id,
                "workspace_unavailable",
                "指定账号 workspace 不可用".to_string(),
            ))
        }
    };
    if !activation_revision_is_current(&app_state.db, &account_id, authorization_revision)? {
        return Ok(disabled_poll(account_id, "authorization_changed"));
    }
    let cache_generation = app_state.usage_cache.codex_oauth_generation(&account_id);
    let (quota, snapshot) = crate::services::subscription::query_codex_quota_with_snapshot(
        &token,
        Some(&workspace),
        "codex_oauth",
        "Codex OAuth access token expired or rejected. Please re-login via cc-switch.",
    )
    .await?;
    if !activation_revision_is_current(&app_state.db, &account_id, authorization_revision)? {
        return Ok(disabled_poll(account_id, "authorization_changed"));
    }
    let publication = publish_activation_quota_with(
        &app_state.usage_cache,
        account_id.clone(),
        cache_generation,
        quota.clone(),
        || async {},
    )
    .await;
    if publication != CodexOauthCachePublication::Published {
        return Ok(cache_rejected_poll(account_id, publication));
    }
    crate::tray::schedule_tray_refresh(&app);
    let Some(snapshot) = snapshot else {
        return Ok(CodexQuotaActivationPollResult {
            account_id,
            polled: true,
            attempted: false,
            target: None,
            status: "quota_unavailable".to_string(),
            reason: Some("quota_snapshot_unavailable".to_string()),
            quota: Some(quota),
        });
    };
    if let Some(reason) =
        crate::services::codex_quota_activation::snapshot_freshness_reason(&snapshot, now_millis())
    {
        return Ok(CodexQuotaActivationPollResult {
            account_id,
            polled: true,
            attempted: false,
            target: None,
            status: "stale_snapshot".to_string(),
            reason: Some(reason.to_string()),
            quota: Some(quota),
        });
    }
    let Some(mut record) = app_state
        .db
        .get_codex_quota_activation_state(&account_id)
        .map_err(|error| error.to_string())?
    else {
        return Ok(CodexQuotaActivationPollResult {
            account_id,
            polled: true,
            attempted: false,
            target: None,
            status: "disabled".to_string(),
            reason: Some("activation_not_enabled".to_string()),
            quota: Some(quota),
        });
    };
    if !record.enabled || record.authorization_revision != authorization_revision {
        return Ok(CodexQuotaActivationPollResult {
            account_id,
            polled: true,
            attempted: false,
            target: None,
            status: "disabled".to_string(),
            reason: Some("activation_not_enabled".to_string()),
            quota: Some(quota),
        });
    }
    let prior_state = record.state.clone();
    let decision = record.state.observe(&snapshot, now_millis());
    if record.state != prior_state {
        if !app_state
            .db
            .save_codex_quota_activation_state_if_revision(
                &account_id,
                authorization_revision,
                &record.state,
                now_millis(),
            )
            .map_err(|error| error.to_string())?
        {
            return Ok(disabled_poll(account_id, "authorization_changed"));
        }
    }
    let decision_for_result = decision.clone();
    let ActivationDecision::Candidate(target) = decision else {
        return Ok(CodexQuotaActivationPollResult {
            account_id,
            polled: true,
            attempted: false,
            target: None,
            status: match decision_for_result {
                ActivationDecision::Blocked(_) => "blocked".to_string(),
                ActivationDecision::Unsupported(_) => "unsupported".to_string(),
                ActivationDecision::Observe => "observed".to_string(),
                ActivationDecision::Candidate(_) => unreachable!(),
            },
            reason: Some(match decision_for_result {
                ActivationDecision::Blocked(reason) | ActivationDecision::Unsupported(reason) => {
                    reason.to_string()
                }
                ActivationDecision::Observe => "target_not_zero".to_string(),
                ActivationDecision::Candidate(_) => unreachable!(),
            }),
            quota: Some(quota),
        });
    };
    let model_capability =
        crate::services::codex_oauth_models::activation_model_capability_with_token(
            &token, &workspace,
        )
        .await;
    if !activation_revision_is_current(&app_state.db, &account_id, authorization_revision)? {
        return Ok(disabled_poll(account_id, "authorization_changed"));
    }
    let model_reason: Option<(&str, String)> = match model_capability {
        Ok(crate::services::codex_oauth_models::ActivationModelCapability::Supported) => None,
        Ok(crate::services::codex_oauth_models::ActivationModelCapability::ModelUnavailable) => {
            Some(("model_unavailable", "gpt-6-luna_not_in_catalog".to_string()))
        }
        Ok(crate::services::codex_oauth_models::ActivationModelCapability::LowUnavailable) => {
            Some((
                "model_unavailable",
                "gpt-6-luna_low_unavailable".to_string(),
            ))
        }
        Ok(crate::services::codex_oauth_models::ActivationModelCapability::CatalogInvalid) => {
            Some((
                "protocol_unknown",
                "model_catalog_capability_invalid".to_string(),
            ))
        }
        Err(reason) => Some(("model_preflight_failed", reason)),
    };
    if let Some((status, reason)) = model_reason {
        return Ok(CodexQuotaActivationPollResult {
            account_id,
            polled: true,
            attempted: false,
            target: Some(target.as_str().to_string()),
            status: status.to_string(),
            reason: Some(reason),
            quota: Some(quota),
        });
    }
    let now_after_model = now_millis();
    if crate::services::codex_quota_activation::snapshot_freshness_reason(
        &snapshot,
        now_after_model,
    )
    .is_some()
    {
        return Ok(CodexQuotaActivationPollResult {
            account_id,
            polled: true,
            attempted: false,
            target: Some(target.as_str().to_string()),
            status: "stale_snapshot".to_string(),
            reason: Some("quota_snapshot_aged_during_model_preflight".to_string()),
            quota: Some(quota),
        });
    }
    if !activation_revision_is_current(&app_state.db, &account_id, authorization_revision)? {
        return Ok(disabled_poll(account_id, "authorization_changed"));
    }
    let current_interval = derive_activation_interval(&app_state.db, &state.0, &account_id).await?;
    if current_interval != effective_interval_minutes
        || !current_interval.is_some_and(|interval| interval > 0)
    {
        app_state
            .db
            .refresh_codex_quota_activation_interval(&account_id, current_interval, now_millis())
            .map_err(|error| error.to_string())?;
        return Ok(CodexQuotaActivationPollResult {
            account_id,
            polled: true,
            attempted: false,
            target: Some(target.as_str().to_string()),
            status: "interval_changed".to_string(),
            reason: Some("effective_interval_changed_during_tick".to_string()),
            quota: Some(quota),
        });
    }
    let attempt_id = Uuid::new_v4().to_string();
    let claim_time = now_millis();
    if app_state
        .db
        .claim_codex_quota_activation_if_revision(
            &account_id,
            authorization_revision,
            &attempt_id,
            claim_time,
        )
        .map_err(|error| error.to_string())?
        .is_none()
    {
        return Ok(CodexQuotaActivationPollResult {
            account_id,
            polled: true,
            attempted: false,
            target: Some(target.as_str().to_string()),
            status: "claim_lost".to_string(),
            reason: Some("another automatic tick claimed the event".to_string()),
            quota: Some(quota),
        });
    }
    let client = match crate::proxy::http_client::get_for_single_post() {
        Ok(client) => client,
        Err(error) => {
            let _ = app_state
                .db
                .finish_codex_quota_activation_attempt_if_revision(
                    &account_id,
                    authorization_revision,
                    &attempt_id,
                    "unknown",
                    Some(&error),
                    now_millis(),
                );
            return Ok(failed_poll(account_id, "transport_unavailable", error));
        }
    };
    // This conditional write is the dispatch linearization point.  It is
    // serialized with the disable/restore UPDATE, so a close that commits
    // first prevents the sender call; a close after it succeeds may leave one
    // in-flight request but cannot make its late finish current.
    let dispatch = dispatch_activation_with(
        &app_state.db,
        &account_id,
        authorization_revision,
        &attempt_id,
        || async {},
        || async { send_activation_once(&client, ACTIVATION_ENDPOINT, &token, &workspace).await },
    )
    .await?;
    let Some(result) = dispatch else {
        let precondition_finished = app_state
            .db
            .finish_codex_quota_activation_attempt_if_revision(
                &account_id,
                authorization_revision,
                &attempt_id,
                "unknown",
                Some("dispatch_precondition_changed"),
                now_millis(),
            )
            .map_err(|error| error.to_string())?;
        return Ok(disabled_poll(
            account_id,
            if precondition_finished {
                "activation_configuration_changed"
            } else {
                "authorization_changed"
            },
        ));
    };
    let status = match result.status {
        ActivationHttpStatus::Completed => "completed",
        ActivationHttpStatus::Failed => "failed",
        ActivationHttpStatus::Unknown => "unknown",
    };
    let finished = app_state
        .db
        .finish_codex_quota_activation_attempt_if_revision(
            &account_id,
            authorization_revision,
            &attempt_id,
            status,
            result.error.as_deref(),
            chrono::Utc::now().timestamp_millis(),
        )
        .map_err(|error| error.to_string())?;
    // The POST may already have been handed to the transport when a close or
    // restore invalidates the revision.  Keep that fact visible to callers,
    // but never project the late terminal result as the current generation's
    // success.
    let status = if finished { status } else { "superseded" };
    Ok(CodexQuotaActivationPollResult {
        account_id,
        polled: true,
        attempted: true,
        target: Some(target.as_str().to_string()),
        status: status.to_string(),
        reason: result.error,
        quota: Some(quota),
    })
}

/// Shared production dispatch boundary. `before_submit` represents the final
/// preparation/barrier point and always runs outside the DB transaction. The
/// conditional status transition and account close/restore serialize through
/// SQLite; the sender is entered only after the transition succeeds.
async fn dispatch_activation_with<T, Prepare, PrepareFuture, Sender, SenderFuture>(
    db: &crate::database::Database,
    account_id: &str,
    expected_revision: i64,
    attempt_id: &str,
    before_submit: Prepare,
    sender: Sender,
) -> Result<Option<T>, String>
where
    Prepare: FnOnce() -> PrepareFuture,
    PrepareFuture: std::future::Future<Output = ()>,
    Sender: FnOnce() -> SenderFuture,
    SenderFuture: std::future::Future<Output = T>,
{
    before_submit().await;
    if !db
        .begin_codex_quota_activation_dispatch_if_revision(
            account_id,
            expected_revision,
            attempt_id,
            now_millis(),
        )
        .map_err(|error| error.to_string())?
    {
        return Ok(None);
    }
    Ok(Some(sender().await))
}

/// The automatic tick's cache publication boundary. Keeping the optional
/// barrier here lets isolated tests pause after the revision check and before
/// the real generation-aware writer, matching the production call graph.
async fn publish_activation_quota_with<Before, BeforeFuture>(
    usage_cache: &crate::services::usage_cache::UsageCache,
    account_id: String,
    expected_generation: u64,
    quota: SubscriptionQuota,
    before_write: Before,
) -> CodexOauthCachePublication
where
    Before: FnOnce() -> BeforeFuture,
    BeforeFuture: std::future::Future<Output = ()>,
{
    before_write().await;
    usage_cache.put_codex_oauth_if_generation_result(account_id, expected_generation, quota)
}

fn failed_poll(account_id: String, status: &str, reason: String) -> CodexQuotaActivationPollResult {
    CodexQuotaActivationPollResult {
        account_id,
        polled: true,
        attempted: false,
        target: None,
        status: status.to_string(),
        reason: Some(reason.chars().take(256).collect()),
        quota: None,
    }
}

fn disabled_poll(account_id: String, reason: &str) -> CodexQuotaActivationPollResult {
    CodexQuotaActivationPollResult {
        account_id,
        polled: true,
        attempted: false,
        target: None,
        status: "disabled".to_string(),
        reason: Some(reason.to_string()),
        quota: None,
    }
}

fn cache_rejected_poll(
    account_id: String,
    publication: CodexOauthCachePublication,
) -> CodexQuotaActivationPollResult {
    let (status, reason) = match publication {
        CodexOauthCachePublication::Superseded => (
            "superseded",
            "automatic quota result superseded by account invalidation",
        ),
        CodexOauthCachePublication::Stale => (
            "stale_result",
            "automatic quota result was older than the current cache",
        ),
        CodexOauthCachePublication::Unavailable => {
            ("stale_result", "automatic quota cache was unavailable")
        }
        CodexOauthCachePublication::Published => unreachable!(),
    };
    CodexQuotaActivationPollResult {
        account_id,
        polled: true,
        attempted: false,
        target: None,
        status: status.to_string(),
        reason: Some(reason.to_string()),
        quota: None,
    }
}

fn activation_revision_is_current(
    db: &crate::database::Database,
    account_id: &str,
    expected_revision: i64,
) -> Result<bool, String> {
    Ok(db
        .codex_quota_activation_revision(account_id)
        .map_err(|error| error.to_string())?
        .is_some_and(|(enabled, revision)| enabled && revision == expected_revision))
}

fn now_millis() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

async fn derive_activation_interval(
    db: &crate::database::Database,
    manager: &CodexOAuthManager,
    account_id: &str,
) -> Result<Option<u64>, String> {
    let _ = manager;
    db.derive_codex_quota_activation_interval(account_id, None)
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;
    use crate::error::AppError;
    use crate::provider::{AuthBinding, AuthBindingSource, Provider, ProviderMeta, UsageScript};
    use crate::services::codex_quota_activation::{ActivationState, WindowKind};
    use crate::services::subscription::{QuotaTier, SubscriptionQuota};
    use crate::services::usage_cache::UsageCache;
    use axum::{extract::State, http::StatusCode, response::IntoResponse, routing::post, Router};
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    use tokio::sync::Notify;

    struct FakeActivationServer {
        calls: AtomicUsize,
        seen: Notify,
        release: Notify,
    }

    async fn activation_handler(
        State(server): State<Arc<FakeActivationServer>>,
    ) -> impl IntoResponse {
        server.calls.fetch_add(1, Ordering::SeqCst);
        server.seen.notify_one();
        server.release.notified().await;
        (
            StatusCode::OK,
            [("content-type", "text/event-stream")],
            "event: response.completed\ndata: {\"response\":{\"model\":\"gpt-6-luna\",\"status\":\"completed\"}}\n\n",
        )
    }

    fn account_provider(account_id: Option<&str>, enabled: bool, interval: u64) -> Provider {
        let mut meta = ProviderMeta::default();
        meta.provider_type = Some("codex_oauth".to_string());
        meta.auth_binding = Some(AuthBinding {
            source: AuthBindingSource::ManagedAccount,
            auth_provider: Some("codex_oauth".to_string()),
            account_id: account_id.map(str::to_string),
        });
        meta.usage_script = Some(UsageScript {
            enabled,
            language: "javascript".to_string(),
            code: "return {};".to_string(),
            timeout: None,
            api_key: None,
            base_url: None,
            access_token: None,
            user_id: None,
            template_type: Some("official_subscription".to_string()),
            auto_query_interval: Some(interval),
            coding_plan_provider: None,
            access_key_id: None,
            secret_access_key: None,
            team_organization_id: None,
            team_project_id: None,
        });
        Provider {
            id: "codex-provider".to_string(),
            name: "Codex OAuth".to_string(),
            settings_config: serde_json::json!({}),
            website_url: None,
            category: Some("official".to_string()),
            created_at: None,
            sort_index: None,
            notes: None,
            meta: Some(meta),
            icon: None,
            icon_color: None,
            in_failover_queue: false,
        }
    }

    async fn prepared_claim() -> (Arc<Database>, i64, String) {
        let db = Arc::new(Database::memory().unwrap());
        db.save_provider("codex", &account_provider(Some("account"), true, 5))
            .unwrap();
        db.set_codex_quota_activation_enabled("account", true, Some(5), 1)
            .unwrap();
        db.save_codex_quota_activation_state(
            "account",
            &ActivationState::Eligible {
                target: WindowKind::FiveHour,
                event_key: "five_hour:1800000000".to_string(),
            },
            2,
        )
        .unwrap();
        let revision = db
            .codex_quota_activation_revision("account")
            .unwrap()
            .unwrap()
            .1;
        let attempt = "attempt".to_string();
        assert!(db
            .claim_codex_quota_activation_if_revision("account", revision, &attempt, 3)
            .unwrap()
            .is_some());
        (db, revision, attempt)
    }

    fn test_quota(queried_at: i64, utilization: f64) -> SubscriptionQuota {
        SubscriptionQuota {
            tool: "codex_oauth".to_string(),
            credential_status: CredentialStatus::Valid,
            credential_message: None,
            success: true,
            tiers: vec![QuotaTier {
                name: "five_hour".to_string(),
                utilization,
                resets_at: None,
                used_value_usd: None,
                max_value_usd: None,
            }],
            extra_usage: None,
            reset_credits: None,
            credits_balance: None,
            error: None,
            queried_at: Some(queried_at),
        }
    }

    #[test]
    fn s04_backend_cache_rejection_is_serialized_as_superseded() {
        let cache = UsageCache::new();
        let quota = test_quota(100, 20.0);
        let generation = cache.codex_oauth_generation("account");
        cache.invalidate_codex_oauth("account");
        assert!(!cache.put_codex_oauth_if_generation(
            "account".to_string(),
            generation,
            quota.clone(),
        ));

        let result = serde_json::to_value(codex_oauth_quota_result(quota, false)).unwrap();
        assert_eq!(result["lifecycle"], "superseded");
        assert_eq!(result["tool"], "codex_oauth");
        assert_eq!(result["success"], true);
    }

    #[tokio::test]
    async fn t02_automatic_publication_barrier_returns_superseded_without_quota() {
        let (db, revision, _) = prepared_claim().await;
        assert!(activation_revision_is_current(&db, "account", revision).unwrap());

        let cache = Arc::new(UsageCache::new());
        let generation = cache.codex_oauth_generation("account");
        let ready = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        let task_cache = cache.clone();
        let task_ready = ready.clone();
        let task_release = release.clone();
        let task = tokio::spawn(async move {
            publish_activation_quota_with(
                &task_cache,
                "account".to_string(),
                generation,
                test_quota(100, 90.0),
                move || async move {
                    task_ready.notify_one();
                    task_release.notified().await;
                },
            )
            .await
        });

        ready.notified().await;
        // The old frontend revision is intentionally irrelevant here: the
        // backend invalidation must win before the real cache writer runs.
        db.set_codex_quota_activation_enabled("account", false, None, 4)
            .unwrap();
        cache.invalidate_codex_oauth("account");
        release.notify_one();

        let publication = task.await.unwrap();
        assert_eq!(publication, CodexOauthCachePublication::Superseded);
        let result = cache_rejected_poll("account".to_string(), publication);
        assert_eq!(result.status, "superseded");
        assert!(result.quota.is_none());
        assert!(cache.with_codex_oauth("account", |_| true).is_none());
    }

    #[tokio::test]
    async fn s01_provider_configuration_is_checked_before_loopback_sender() {
        let server = Arc::new(FakeActivationServer {
            calls: AtomicUsize::new(0),
            seen: Notify::new(),
            release: Notify::new(),
        });
        let app = Router::new()
            .route("/activation", post(activation_handler))
            .with_state(server.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let (db, revision, attempt) = prepared_claim().await;
        let ready = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        let before_ready = ready.clone();
        let before_release = release.clone();
        let client =
            crate::proxy::http_client::build_client_with_redirect_policy(None, true).unwrap();
        let dispatch_db = db.clone();
        let result = tokio::spawn(async move {
            dispatch_activation_with(
                &dispatch_db,
                "account",
                revision,
                &attempt,
                move || async move {
                    before_ready.notify_one();
                    before_release.notified().await;
                },
                move || async move {
                    send_activation_once(
                        &client,
                        &format!("http://{address}/activation"),
                        "fake-token",
                        "fake-workspace",
                    )
                    .await
                },
            )
            .await
            .unwrap()
        });

        ready.notified().await;
        // This is a real provider save, so the final DB critical section must
        // observe the changed effective query switch/interval before submit.
        db.save_provider("codex", &account_provider(Some("account"), true, 0))
            .unwrap();
        release.notify_one();

        assert!(result.await.unwrap().is_none());
        assert_eq!(server.calls.load(Ordering::SeqCst), 0);
        task.abort();
    }

    #[tokio::test]
    async fn s01_disabled_or_unbound_provider_cannot_submit_reserved_attempt() {
        for (case, account_id, enabled, interval) in [
            ("query_switch_off", Some("account"), false, 5),
            ("zero_interval", Some("account"), true, 0),
            ("binding_changed", Some("other-account"), true, 5),
        ] {
            let (db, revision, attempt) = prepared_claim().await;
            db.save_provider("codex", &account_provider(account_id, enabled, interval))
                .unwrap();
            assert!(
                !db.begin_codex_quota_activation_dispatch_if_revision(
                    "account", revision, &attempt, 4,
                )
                .unwrap(),
                "{case} must fail closed before submitted"
            );
        }
    }

    #[tokio::test]
    async fn u01_expired_window_uses_real_dispatch_once_and_never_reclaims() -> Result<(), AppError>
    {
        const BOUNDARY: i64 = 1_800_000_000;
        let server = Arc::new(FakeActivationServer {
            calls: AtomicUsize::new(0),
            seen: Notify::new(),
            release: Notify::new(),
        });
        let app = Router::new()
            .route("/activation", post(activation_handler))
            .with_state(server.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let db = Arc::new(Database::memory()?);
        db.save_provider("codex", &account_provider(Some("account"), true, 5))?;
        let interval = db.derive_codex_quota_activation_interval("account", None)?;
        db.set_codex_quota_activation_enabled("account", true, interval, 1)?;
        let record = db.get_codex_quota_activation_state("account")?.unwrap();
        let observation = |server_seconds: i64, used: f64| {
            let value = serde_json::json!({
                "rate_limit": {
                    "allowed": true,
                    "limit_reached": false,
                    "primary_window": {
                        "used_percent": used,
                        "limit_window_seconds": 18000,
                        "reset_at": BOUNDARY,
                        "reset_after_seconds": (BOUNDARY - server_seconds).max(0)
                    },
                    "secondary_window": {
                        "used_percent": 50,
                        "limit_window_seconds": 604800,
                        "reset_at": server_seconds + 604800,
                        "reset_after_seconds": 604800
                    }
                }
            });
            let mut snapshot = crate::services::codex_quota_activation::snapshot_from_value(
                &value,
                server_seconds * 1000,
                server_seconds * 1000,
            )
            .unwrap();
            snapshot.server_date_at_ms = Some(server_seconds * 1000);
            snapshot.cache_age_seconds = Some(0);
            snapshot
        };

        let initial = observation(BOUNDARY - 60, 0.0);
        let mut state = record.state;
        assert!(matches!(
            state.observe(&initial, initial.observed_at_ms),
            ActivationDecision::Candidate(WindowKind::FiveHour)
        ));
        assert!(db.save_codex_quota_activation_state_if_revision(
            "account",
            record.authorization_revision,
            &state,
            initial.observed_at_ms,
        )?);
        let attempt = db
            .claim_codex_quota_activation_if_revision(
                "account",
                record.authorization_revision,
                "attempt-a",
                BOUNDARY - 50_000,
            )?
            .expect("initial candidate must claim");

        let client =
            crate::proxy::http_client::build_client_with_redirect_policy(None, true).unwrap();
        let dispatch_db = db.clone();
        let dispatch = tokio::spawn(async move {
            dispatch_activation_with(
                &dispatch_db,
                "account",
                record.authorization_revision,
                &attempt.attempt_id,
                || async {},
                move || async move {
                    send_activation_once(
                        &client,
                        &format!("http://{address}/activation"),
                        "fake-token",
                        "fake-workspace",
                    )
                    .await
                },
            )
            .await
            .unwrap()
        });
        server.seen.notified().await;
        server.release.notify_one();
        assert!(dispatch.await.unwrap().is_some());
        assert!(db.finish_codex_quota_activation_attempt_if_revision(
            "account",
            record.authorization_revision,
            "attempt-a",
            "completed",
            None,
            BOUNDARY,
        )?);

        let record = db.get_codex_quota_activation_state("account")?.unwrap();
        let expired_nonzero = observation(BOUNDARY + 120, 20.0);
        let mut state = record.state;
        assert_eq!(
            state.observe(&expired_nonzero, expired_nonzero.observed_at_ms),
            ActivationDecision::Observe
        );
        assert!(matches!(state, ActivationState::Spent { .. }));
        let repeated_zero = observation(BOUNDARY + 180, 0.0);
        assert_eq!(
            state.observe(&repeated_zero, repeated_zero.observed_at_ms),
            ActivationDecision::Observe
        );
        assert!(db
            .claim_codex_quota_activation_if_revision(
                "account",
                record.authorization_revision,
                "attempt-b",
                repeated_zero.observed_at_ms,
            )?
            .is_none());
        assert_eq!(server.calls.load(Ordering::SeqCst), 1);
        task.abort();
        Ok(())
    }

    #[tokio::test]
    async fn g01_persisted_positive_control_claims_and_submits_once() -> Result<(), AppError> {
        const BOUNDARY: i64 = 1_800_000_000;
        let server = Arc::new(FakeActivationServer {
            calls: AtomicUsize::new(0),
            seen: Notify::new(),
            release: Notify::new(),
        });
        let app = Router::new()
            .route("/activation", post(activation_handler))
            .with_state(server.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind loopback fake server");
        let address = listener.local_addr().expect("read fake server address");
        let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let db = Arc::new(Database::memory()?);
        db.save_provider("codex", &account_provider(Some("account"), true, 5))?;
        let interval = db.derive_codex_quota_activation_interval("account", None)?;
        assert_eq!(interval, Some(5));
        let enabled = db.set_codex_quota_activation_enabled("account", true, interval, 1)?;
        assert!(enabled.enabled);
        let revision = enabled.authorization_revision;

        let observation = |server_seconds: i64, used: f64| {
            let raw = serde_json::json!({
                "rate_limit": {
                    "allowed": true,
                    "limit_reached": false,
                    "primary_window": {
                        "used_percent": used,
                        "limit_window_seconds": 18000,
                        "reset_at": BOUNDARY,
                        "reset_after_seconds": BOUNDARY.saturating_sub(server_seconds).max(0)
                    },
                    "secondary_window": {
                        "used_percent": 50,
                        "limit_window_seconds": 604800,
                        "reset_at": server_seconds + 604800,
                        "reset_after_seconds": 604800
                    }
                }
            });
            let mut snapshot = crate::services::codex_quota_activation::snapshot_from_value(
                &raw,
                server_seconds * 1000,
                server_seconds * 1000,
            )
            .unwrap();
            snapshot.server_date_at_ms = Some(server_seconds * 1000);
            snapshot.cache_age_seconds = Some(0);
            snapshot
        };

        let mut state = enabled.state;
        let before_nonzero = state.clone();
        let used_before_boundary = observation(BOUNDARY - 60, 20.0);
        assert_eq!(
            state.observe(&used_before_boundary, used_before_boundary.observed_at_ms),
            ActivationDecision::Observe
        );
        assert_ne!(state, before_nonzero);
        assert!(db.save_codex_quota_activation_state_if_revision(
            "account",
            revision,
            &state,
            used_before_boundary.observed_at_ms,
        )?);
        let tracking = db
            .get_codex_quota_activation_state("account")?
            .expect("tracking state must be persisted");
        assert!(matches!(
            tracking.state,
            ActivationState::TrackingUsage { .. }
        ));
        assert!(tracking.enabled);
        assert_eq!(tracking.authorization_revision, revision);

        let mut state = tracking.state;
        let recovered = observation(BOUNDARY + 1, 0.0);
        assert_eq!(
            state.observe(&recovered, recovered.observed_at_ms),
            ActivationDecision::Candidate(WindowKind::FiveHour)
        );
        assert!(db.save_codex_quota_activation_state_if_revision(
            "account",
            revision,
            &state,
            recovered.observed_at_ms,
        )?);
        let eligible = db
            .get_codex_quota_activation_state("account")?
            .expect("eligible state must be persisted");
        assert!(matches!(eligible.state, ActivationState::Eligible { .. }));
        let attempt = db
            .claim_codex_quota_activation_if_revision(
                "account",
                revision,
                "g01-attempt",
                recovered.observed_at_ms,
            )?
            .expect("valid persisted candidate must claim");
        assert!(matches!(
            db.get_codex_quota_activation_state("account")?
                .expect("claimed row must remain readable")
                .state,
            ActivationState::Spent { .. }
        ));

        let client = crate::proxy::http_client::build_client_with_redirect_policy(None, true)
            .map_err(|error| AppError::Config(error.to_string()))?;
        let dispatch_db = db.clone();
        let dispatch = tokio::spawn(async move {
            dispatch_activation_with(
                &dispatch_db,
                "account",
                revision,
                &attempt.attempt_id,
                || async {},
                || async move {
                    send_activation_once(
                        &client,
                        &format!("http://{address}/activation"),
                        "fake-token",
                        "fake-workspace",
                    )
                    .await
                },
            )
            .await
            .unwrap()
        });
        server.seen.notified().await;
        let submitted = db
            .get_codex_quota_activation_state("account")?
            .expect("submitted row must remain readable");
        assert_eq!(submitted.last_attempt_status.as_deref(), Some("submitted"));
        server.release.notify_one();
        let result = dispatch.await.unwrap();
        assert!(result.is_some(), "submitted dispatch must enter the sender");
        assert_eq!(server.calls.load(Ordering::SeqCst), 1);
        assert!(db.finish_codex_quota_activation_attempt_if_revision(
            "account",
            revision,
            "g01-attempt",
            "completed",
            None,
            recovered.observed_at_ms + 1_000,
        )?);

        let mut state = db
            .get_codex_quota_activation_state("account")?
            .expect("finished row must remain readable")
            .state;
        let repeated_zero = observation(BOUNDARY + 2, 0.0);
        assert_eq!(
            state.observe(&repeated_zero, repeated_zero.observed_at_ms),
            ActivationDecision::Observe
        );
        assert!(db
            .claim_codex_quota_activation_if_revision(
                "account",
                revision,
                "g01-repeat",
                repeated_zero.observed_at_ms,
            )?
            .is_none());
        assert_eq!(server.calls.load(Ordering::SeqCst), 1);
        task.abort();
        Ok(())
    }

    #[tokio::test]
    async fn n02_disable_before_dispatch_commit_calls_real_sender_zero_times() {
        let server = Arc::new(FakeActivationServer {
            calls: AtomicUsize::new(0),
            seen: Notify::new(),
            release: Notify::new(),
        });
        let app = Router::new()
            .route("/activation", post(activation_handler))
            .with_state(server.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let (db, revision, attempt) = prepared_claim().await;
        let ready = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        let before_ready = ready.clone();
        let before_release = release.clone();
        let client =
            crate::proxy::http_client::build_client_with_redirect_policy(None, true).unwrap();
        let dispatch_db = db.clone();
        let result = tokio::spawn(async move {
            dispatch_activation_with(
                &dispatch_db,
                "account",
                revision,
                &attempt,
                move || async move {
                    before_ready.notify_one();
                    before_release.notified().await;
                },
                move || async move {
                    send_activation_once(
                        &client,
                        &format!("http://{address}/activation"),
                        "fake-token",
                        "fake-workspace",
                    )
                    .await
                },
            )
            .await
            .unwrap()
        });
        ready.notified().await;
        db.set_codex_quota_activation_enabled("account", false, None, 4)
            .unwrap();
        release.notify_one();
        assert!(result.await.unwrap().is_none());
        assert_eq!(server.calls.load(Ordering::SeqCst), 0);
        task.abort();
    }

    #[tokio::test]
    async fn n02_dispatch_commit_allows_one_inflight_request_but_finish_is_stale() {
        let server = Arc::new(FakeActivationServer {
            calls: AtomicUsize::new(0),
            seen: Notify::new(),
            release: Notify::new(),
        });
        let app = Router::new()
            .route("/activation", post(activation_handler))
            .with_state(server.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let (db, revision, attempt) = prepared_claim().await;
        let client =
            crate::proxy::http_client::build_client_with_redirect_policy(None, true).unwrap();
        let dispatch_db = db.clone();
        let result = tokio::spawn(async move {
            dispatch_activation_with(
                &dispatch_db,
                "account",
                revision,
                &attempt,
                || async {},
                move || async move {
                    send_activation_once(
                        &client,
                        &format!("http://{address}/activation"),
                        "fake-token",
                        "fake-workspace",
                    )
                    .await
                },
            )
            .await
            .unwrap()
        });
        server.seen.notified().await;
        db.set_codex_quota_activation_enabled("account", false, None, 4)
            .unwrap();
        server.release.notify_one();
        let sent = result
            .await
            .unwrap()
            .expect("dispatch committed before close");
        assert_eq!(sent.status, ActivationHttpStatus::Completed);
        assert_eq!(server.calls.load(Ordering::SeqCst), 1);
        assert!(!db
            .finish_codex_quota_activation_attempt_if_revision(
                "account",
                revision,
                "attempt",
                "completed",
                None,
                5,
            )
            .unwrap());
        task.abort();
    }

    #[tokio::test]
    async fn r01_full_restore_missing_row_invalidates_barriered_tick_and_sender(
    ) -> Result<(), AppError> {
        let live = Arc::new(Database::memory().unwrap());
        live.set_codex_quota_activation_enabled("account-a", true, Some(1), 1)
            .unwrap();
        live.save_codex_quota_activation_state(
            "account-a",
            &ActivationState::Eligible {
                target: WindowKind::FiveHour,
                event_key: "five_hour:18300".to_string(),
            },
            2,
        )
        .unwrap();
        let old_revision = live
            .codex_quota_activation_revision("account-a")
            .unwrap()
            .unwrap()
            .1;
        let old_attempt = "r01-old-attempt".to_string();
        assert!(live
            .claim_codex_quota_activation_if_revision("account-a", old_revision, &old_attempt, 3,)
            .unwrap()
            .is_some());

        let backup = Database::memory().unwrap();
        {
            let conn = crate::database::lock_conn!(backup.conn);
            conn.execute(
                "INSERT INTO providers (id, app_type, name, settings_config, meta)
                 VALUES ('account-provider', 'codex', 'Account Provider', '{}', '{}')",
                [],
            )
            .unwrap();
        }
        let backup_sql = backup.export_sql_string().unwrap();
        let ready = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        let sender_calls = Arc::new(AtomicUsize::new(0));
        let before_ready = ready.clone();
        let before_release = release.clone();
        let sender_calls_for_task = sender_calls.clone();
        let client =
            crate::proxy::http_client::build_client_with_redirect_policy(None, true).unwrap();
        let dispatch_db = live.clone();
        let old_attempt_for_task = old_attempt.clone();
        let tick = tokio::spawn(async move {
            dispatch_activation_with(
                &dispatch_db,
                "account-a",
                old_revision,
                &old_attempt_for_task,
                move || async move {
                    before_ready.notify_one();
                    before_release.notified().await;
                },
                move || async move {
                    sender_calls_for_task.fetch_add(1, Ordering::SeqCst);
                    send_activation_once(
                        &client,
                        "http://127.0.0.1:1/should-not-be-called",
                        "fake-token",
                        "fake-workspace",
                    )
                    .await
                },
            )
            .await
            .unwrap()
        });
        ready.notified().await;
        live.import_sql_string(&backup_sql).unwrap();
        let reenabled = live
            .set_codex_quota_activation_enabled("account-a", true, Some(1), 4)
            .unwrap();
        assert!(reenabled.authorization_revision > old_revision);
        release.notify_one();
        assert!(tick.await.unwrap().is_none());
        assert_eq!(sender_calls.load(Ordering::SeqCst), 0);
        Ok(())
    }
}
