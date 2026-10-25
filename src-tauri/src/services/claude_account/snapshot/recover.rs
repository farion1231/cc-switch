//! 恢复：补完崩溃时没做完的切换。

use serde_json::Value;

use super::creds::{account_fingerprint, credential_org};
use super::files::{read_profile, refresh_locks};
use super::store::Slot;
use super::switch::finish;
use super::{default_owner, load_state, update_state, Ctx, Step, SWITCH};
use crate::error::AppError;

/// 补完上一次没做完的切换（启动时、每次切换前）。补完的切换停在 [`Step::Done`]，等调用方确认。
pub fn recover(ctx: &Ctx<'_>) -> Result<(), AppError> {
    let _serial = SWITCH
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    recover_locked(ctx)
}

pub(super) fn recover_locked(ctx: &Ctx<'_>) -> Result<(), AppError> {
    let Some(journal) = load_state(ctx.data_dir)
        .journal
        .filter(|journal| journal.step != Step::Done)
    else {
        return Ok(());
    };
    let _refresh = refresh_locks(&ctx.default)?;
    let now = ctx.store.read(&ctx.default)?;
    let now_account = now.as_ref().map(account_fingerprint);
    // 默认槽位里还是 T：要么就是写进去的那份，要么已提交、之后被 Claude Code 刷新过。刷新
    // 只换令牌：组织不变，`.claude.json` 的资料也还是 T 的（已换过）或切换前那个号的（还没
    // 换）。用户之后在 Claude Code 里登了别的号——哪怕同一个 Team 组织——资料就是那个号的，
    // 什么都不动。
    let refreshed = journal.step == Step::Committed
        && journal.target_org.is_some()
        && now.as_ref().and_then(credential_org) == journal.target_org
        && {
            let owner = default_owner(&ctx.default);
            owner.as_deref() == Some(journal.target.as_str()) || owner == journal.before_owner
        };
    if now_account.as_deref() == Some(journal.target_account.as_str()) || refreshed {
        log::info!("[ClaudeAccount] 补完上次没做完的切换");
        let profile = read_profile(&Slot::Account(journal.target_dir.clone()).global_config())
            .unwrap_or(Value::Null);
        return finish(ctx, &journal, &profile);
    }
    if now_account != journal.before_account {
        // 既不是写之前的、也不是 T：Claude Code 在这之后登录过。账号槽位都还在，不补。
        log::warn!("[ClaudeAccount] 上次切换没做完，默认登录之后又变了，按现状处理");
    }
    // 提交点没写进去：收回那一步写进 O 账号槽位的是 O 当时的登录，和默认槽位里的是同一份，
    // 下次切走时会再覆盖一次，不影响使用。
    update_state(ctx.data_dir, |state| state.journal = None)?;
    Ok(())
}
