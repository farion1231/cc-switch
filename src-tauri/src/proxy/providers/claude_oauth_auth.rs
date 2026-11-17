//! Claude Code 订阅登录（官方 Claude 账号）作为路由凭据。
//!
//! 走路由的 Claude 卡片自己不存 key：凭据是这台电脑上的 Claude Code 登录——
//! macOS 钥匙串（`Claude Code-credentials`）、`~/.claude/.credentials.json`，
//! 或卡片绑定的托管账号自己的槽位。forwarder 在每次请求前读一次，作为 Bearer
//! token 注入。
//!
//! CC Switch 不刷新 token：刷新由 Claude Code 自己负责，它写回同一个存储。
//! token 过期时会在上游得到 401，交给既有的供应商故障转移处理。

use serde_json::Value;

use crate::error::AppError;
use crate::provider::Provider;
use crate::services::claude_account::snapshot::{
    self,
    store::{platform_store, Slot},
};
use crate::services::provider::claude_official;

/// 卡片用哪个槽位：绑了托管账号就是那个账号的槽位，否则是本机默认 `~/.claude` 登录。
fn slot_for(provider: &Provider) -> Slot {
    let data_dir = crate::config::get_app_config_dir();
    claude_official::bound_account(provider)
        .and_then(|account| snapshot::account_slot(&data_dir, &account))
        .unwrap_or_else(Slot::default_slot)
}

/// 卡片当前可用的 Claude Code OAuth access token。
///
/// 错误里只带原因（不含 token 本身）：token 只会交给注入它的调用方。
pub fn access_token_for(provider: &Provider) -> Result<String, AppError> {
    let secret = platform_store()
        .read(&slot_for(provider))?
        .ok_or_else(no_login)?;
    let oauth = secret
        .0
        .get("claudeAiOauth")
        .and_then(Value::as_object)
        .ok_or_else(no_login)?;
    let token = oauth
        .get("accessToken")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .ok_or_else(no_login)?;
    warn_if_expired(oauth.get("expiresAt"));
    Ok(token.to_string())
}

/// 凭据里记的过期时间已经过了：不阻断（记录的过期时间可能滞后于 Claude Code 的刷新），
/// 只提醒失败时会走故障转移。
fn warn_if_expired(expires_at: Option<&Value>) {
    let Some(at) = expires_at.and_then(|value| {
        value
            .as_i64()
            .or_else(|| value.as_f64().map(|at| at as i64))
    }) else {
        return;
    };
    let at = if at < 100_000_000_000 { at * 1000 } else { at };
    if at < chrono::Utc::now().timestamp_millis() {
        log::warn!(
            "[ClaudeOAuth] Claude Code 登录凭据记录的过期时间已过（{at}），仍按当前 token 请求；\
             若上游返回 401 会走故障转移。在 Claude Code 里重新登录即可刷新。"
        );
    }
}

fn no_login() -> AppError {
    AppError::localized(
        "proxy.claude_oauth.no_login",
        "路由到 Claude 订阅需要先登录 Claude Code（找不到本机登录凭据）",
        "Routing to the Claude subscription needs a Claude Code sign-in (no local credential found).",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn unbound_card_uses_the_default_slot() {
        let provider = Provider::with_id("default".into(), "default".into(), json!({ "env": {} }), None);
        assert_eq!(slot_for(&provider), Slot::default_slot());
    }
}
