//! 凭据（[`Secret`]）上的小工具：账号键的拆合、指纹、认人、过期时间。

use serde_json::{Map, Value};

use super::store::{CredentialStore, Secret, Slot};
use super::{identity, State, ACCOUNT_KEYS};
use crate::error::AppError;
use crate::services::claude_account::AccountPlan;

/// `base` 的非账号键 + `account` 的账号键。
pub(super) fn merge(base: Option<&Secret>, account: &Secret) -> Secret {
    let mut map = base
        .and_then(without_account_keys)
        .map(|secret| secret.0)
        .unwrap_or_default();
    for key in ACCOUNT_KEYS {
        if let Some(value) = account.0.get(*key) {
            map.insert((*key).to_string(), value.clone());
        }
    }
    Secret(map)
}

/// 去掉账号键后剩下的（MCP 授权等，属于这台电脑）；什么都不剩时为 `None`。
fn without_account_keys(secret: &Secret) -> Option<Secret> {
    let rest: Map<String, Value> = secret
        .0
        .iter()
        .filter(|(key, _)| !ACCOUNT_KEYS.contains(&key.as_str()))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    (!rest.is_empty()).then_some(Secret(rest))
}

/// 把槽位里 `secret` 的账号键去掉：剩下的写回，什么都不剩就删掉。
pub(super) fn strip_account_keys(
    store: &dyn CredentialStore,
    slot: &Slot,
    secret: &Secret,
) -> Result<(), AppError> {
    match without_account_keys(secret) {
        Some(rest) => store.write(slot, &rest),
        None => store.delete(slot),
    }
}

/// 写入后读回比对，不一致就返回 `failed()` 的错误。
pub(super) fn write_verified(
    store: &dyn CredentialStore,
    slot: &Slot,
    secret: &Secret,
    failed: impl FnOnce() -> AppError,
) -> Result<(), AppError> {
    store.write(slot, secret)?;
    if store.read(slot)?.as_ref().map(Secret::fingerprint) != Some(secret.fingerprint()) {
        return Err(failed());
    }
    Ok(())
}

/// 只算账号键的指纹（共享键可能被 Claude Code 正常改写，不影响「是谁」）。
pub(super) fn account_fingerprint(secret: &Secret) -> String {
    let subset: Map<String, Value> = ACCOUNT_KEYS
        .iter()
        .filter_map(|key| Some(((*key).to_string(), secret.0.get(*key)?.clone())))
        .collect();
    Secret(subset).fingerprint()
}

pub(super) fn credential_org(secret: &Secret) -> Option<String> {
    secret
        .0
        .get("organizationUuid")
        .and_then(Value::as_str)
        .map(str::to_string)
}

pub(super) fn has_login(secret: &Secret) -> bool {
    secret
        .0
        .get("claudeAiOauth")
        .and_then(|oauth| oauth.get("refreshToken"))
        .and_then(Value::as_str)
        .is_some_and(|token| !token.is_empty())
}

/// 登录（refresh token）什么时候过期，毫秒。没有这个字段时为 `None`。
pub(super) fn login_expiry(secret: &Secret) -> Option<i64> {
    let value = secret
        .0
        .get("claudeAiOauth")?
        .get("refreshTokenExpiresAt")?;
    let at = value
        .as_i64()
        .or_else(|| value.as_f64().map(|at| at as i64))?;
    // 秒级的时间戳换成毫秒
    Some(if at < 100_000_000_000 { at * 1000 } else { at })
}

/// 凭据里 Claude Code 记的套餐（非机密字段）。
pub(super) fn credential_plan(secret: &Secret) -> AccountPlan {
    let oauth = secret.0.get("claudeAiOauth");
    let field = |name: &str| {
        oauth
            .and_then(|oauth| oauth.get(name))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    };
    AccountPlan {
        subscription_type: field("subscriptionType"),
        rate_limit_tier: field("rateLimitTier"),
    }
}

/// 把 `account` 的登录过期时间按 `secret` 记进状态（没有这个字段就删掉旧记录）。
pub(super) fn note_expiry(state: &mut State, account: &str, secret: &Secret) {
    match login_expiry(secret) {
        Some(at) => {
            state.login_expiry.insert(account.to_string(), at);
        }
        None => {
            state.login_expiry.remove(account);
        }
    }
}

/// 槽位里能用的登录：有 refreshToken、没过期。
pub(super) fn read_login(
    store: &dyn CredentialStore,
    slot: &Slot,
) -> Result<Option<Secret>, AppError> {
    let now = chrono::Utc::now().timestamp_millis();
    Ok(store
        .read(slot)?
        .filter(|secret| has_login(secret) && login_expiry(secret).is_none_or(|at| at > now)))
}

/// 一份登录是谁：凭据加同一目录的 `.claude.json`。没有登录时为 `None`；有登录但认不出（没有
/// `oauthAccount`、凭据和资料的组织对不上）时报错，免得把它收错地方。
pub(super) fn owner_of(
    secret: Option<&Secret>,
    profile: Option<&Value>,
) -> Result<Option<identity::Identity>, AppError> {
    let Some(secret) = secret.filter(|secret| has_login(secret)) else {
        return Ok(None);
    };
    let found = profile.and_then(identity::identity_from_profile);
    let profile_org = profile
        .and_then(|doc| doc.get("oauthAccount"))
        .and_then(|account| account.get("organizationUuid"))
        .and_then(Value::as_str);
    let credential_org = secret.0.get("organizationUuid").and_then(Value::as_str);
    let orgs_agree = match (credential_org, profile_org) {
        (Some(a), Some(b)) => a == b,
        _ => true,
    };
    match found {
        Some(found) if orgs_agree => Ok(Some(found)),
        _ => Err(AppError::localized(
            "claude.snapshot.owner_unknown",
            "认不出 Claude Code 现在登录的是哪个账号（登录资料不完整或不一致），没有切换。运行一次 claude 让它补全资料后再试",
            "Can't tell which account Claude Code is signed in to right now (its profile is missing or inconsistent), so nothing was switched. Run claude once so it can fill in the profile, then try again.",
        )),
    }
}

/// 读完之后 Claude Code 又改了登录（刷新、登录、MCP 授权）：放弃这次写入。
pub(super) fn changed() -> AppError {
    AppError::localized(
        "claude.snapshot.changed",
        "Claude Code 刚刚改动了登录信息，切换已取消，请再试一次",
        "Claude Code just changed its sign-in data, so the switch was canceled. Try again.",
    )
}
