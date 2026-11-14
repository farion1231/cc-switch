//! 切换：把一个账号的登录移进默认槽位（[`check_out`]），或移回它自己的目录（[`vacate`]）。

use std::path::{Path, PathBuf};
use std::time::Instant;

use serde_json::Value;

use super::creds::{
    account_fingerprint, changed, credential_org, has_login, merge, note_expiry, owner_of,
    read_login, strip_account_keys, write_verified,
};
use super::files::{
    backup_dir, backup_slot, read_profile, refresh_locks, storage_lock, write_profile,
};
use super::recover::recover_locked;
use super::store::{CredentialStore, Secret, Slot};
use super::{
    default_owner, identity, update_state, Ctx, Journal, Outcome, Step, COMMIT_DEADLINE, SWITCH,
};
use crate::error::AppError;

/// 一次切换要用到的、在刷新锁里读好的东西。
struct Plan {
    /// 默认槽位现在的凭据、资料和它是谁
    live: Option<Secret>,
    live_profile: Option<Value>,
    live_owner: Option<identity::Identity>,
    target_secret: Secret,
    target_profile: Value,
    journal: Journal,
}

/// 默认槽位里已经是 T 时怎么办。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// 切换：已经是 T 就不动（账号目录里另有一份登录也留着）
    Switch,
    /// 重新登录：只在已经是 T 时做，用账号目录里的新登录换掉默认槽位里的旧登录
    Replace,
}

/// 把账号 `target`（目录 `target_dir`）移进默认槽位。
pub fn check_out(ctx: &Ctx<'_>, target: &str, target_dir: &Path) -> Result<Outcome, AppError> {
    run(ctx, target, target_dir, Mode::Switch)
}

/// 账号 `target` 刚重新登录（新登录在 `target_dir`）、默认槽位里又正是它：用新登录换掉默认槽位
/// 里的旧登录（旧的可能已经过期或被吊销）。默认槽位不是它时不动。返回换了没有。
pub fn replace_live(ctx: &Ctx<'_>, target: &str, target_dir: &Path) -> Result<bool, AppError> {
    let outcome = run(ctx, target, target_dir, Mode::Replace)?;
    Ok(matches!(outcome, Outcome::Switched { .. }))
}

fn run(ctx: &Ctx<'_>, target: &str, target_dir: &Path, mode: Mode) -> Result<Outcome, AppError> {
    let _serial = SWITCH
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    recover_locked(ctx)?;

    let started = Instant::now();
    // 刷新锁：挡住 Claude Code 在我们读写之间刷新令牌（两个槽位都拿）
    let _refresh = refresh_locks(&ctx.default)?;
    let _target_refresh = refresh_locks(&Slot::Account(target_dir.to_path_buf()))?;

    let Some(plan) = plan(ctx, target, target_dir, mode)? else {
        return Ok(Outcome::AlreadyLive);
    };
    // 1. 收回默认槽位里原来的号
    let outcome = park_previous(ctx, &plan)?;
    // 2. 提交点
    commit(ctx, &plan, started)?;
    // 3、4。默认槽位已经是 T，切换就算成了：这里失败（配置锁忙、钥匙串一时读不了）不报错，
    // 免得调用方以为没切、不更新卡片；日志留着，下次切换或启动时补完
    if let Err(error) = finish(ctx, &plan.journal, &plan.target_profile) {
        log::warn!("[ClaudeAccount] 登录已切换，收尾没做完，稍后补完: {error}");
    }
    log::info!("[ClaudeAccount] 已切换 Claude Code 的登录");
    Ok(outcome)
}

/// 读默认槽位和 T 的登录、核对 T 的身份。不用动时（切换时默认槽位里已经是 T、重新登录时
/// 默认槽位里不是 T）为 `None`。
fn plan(
    ctx: &Ctx<'_>,
    target: &str,
    target_dir: &Path,
    mode: Mode,
) -> Result<Option<Plan>, AppError> {
    let target_slot = Slot::Account(target_dir.to_path_buf());
    let live = ctx.store.read(&ctx.default)?;
    let live_profile = read_profile(&ctx.default.global_config());
    let live_owner = owner_of(live.as_ref(), live_profile.as_ref())?;

    // 切换时默认槽位里已经是 T（切卡时移进去的，或用户在 Claude Code 里登的就是它）：不动。
    // 账号目录里另有一份登录也留着：同一个号的另一次登录，各自刷新、互不影响
    let live_is_target = live_owner.as_ref().map(|owner| owner.key.as_str()) == Some(target);
    if live_is_target != (mode == Mode::Replace) {
        return Ok(None);
    }

    let target_secret = match read_login(ctx.store, &target_slot)? {
        Some(secret) => secret,
        None => {
            // 槽位里没有能用的登录：用收回时留的备份
            read_login(ctx.store, &backup_slot(target_dir))?.ok_or_else(|| {
                AppError::localized(
                    "claude.snapshot.target_signed_out",
                    "这个 Claude 账号的登录已失效或已过期，请在授权中心重新登录后再切换",
                    "This Claude account is signed out or its sign-in has expired. Sign in again in the Auth Center, then switch.",
                )
            })?
        }
    };
    let target_profile = read_profile(&target_slot.global_config()).ok_or_else(|| {
        AppError::localized(
            "claude.snapshot.target_profile_missing",
            "读不到这个 Claude 账号的资料，请在授权中心重新登录",
            "This Claude account's profile could not be read. Sign in again in the Auth Center.",
        )
    })?;
    // 账号目录里登的必须就是 T：从卡片打开的终端里 `/login` 过别的号时，资料和凭据都是那个号的
    match owner_of(Some(&target_secret), Some(&target_profile)) {
        Ok(Some(found)) if found.key == target => {}
        _ => {
            return Err(AppError::localized(
                "claude.snapshot.target_mismatch",
                "这个账号的目录里现在登录的不是它（可能在它的终端里登录了别的账号），没有切换。请在授权中心重新登录这个账号",
                "This account's folder is now signed in to a different account (maybe someone signed in to another account in its terminal), so nothing was switched. Sign in to this account again in the Auth Center.",
            ))
        }
    }

    let journal = Journal {
        target: target.to_string(),
        target_dir: target_dir.to_path_buf(),
        target_org: credential_org(&target_secret),
        target_account: account_fingerprint(&target_secret),
        before_account: live.as_ref().map(account_fingerprint),
        before_owner: live_owner.as_ref().map(|owner| owner.key.clone()),
        step: Step::Parked,
    };
    Ok(Some(Plan {
        live,
        live_profile,
        live_owner,
        target_secret,
        target_profile,
        journal,
    }))
}

/// 把默认槽位里原来的号收回它的账号槽位；还没收录的先收录。原来就是 T（重新登录）时不收回：
/// 旧登录由新登录顶替。
fn park_previous(ctx: &Ctx<'_>, plan: &Plan) -> Result<Outcome, AppError> {
    let (Some(live), Some(owner)) = (plan.live.as_ref(), plan.live_owner.as_ref()) else {
        return Ok(Outcome::Switched {
            adopted: None,
            previous: None,
        });
    };
    if owner.key == plan.journal.target {
        return Ok(Outcome::Switched {
            adopted: None,
            previous: None,
        });
    }
    let mut adopted = None;
    let park_dir = match ctx.accounts.dir_of(&owner.key) {
        Some(dir) => dir,
        None => {
            let profile = plan
                .live_profile
                .as_ref()
                .and_then(|doc| doc.get("oauthAccount"))
                .cloned()
                .unwrap_or(Value::Null);
            let dir = ctx.adopter.adopt(owner, &profile)?;
            adopted = Some(owner.key.clone());
            dir
        }
    };
    park(
        ctx.store,
        live,
        plan.live_profile.as_ref(),
        &owner.key,
        &Slot::Account(park_dir.clone()),
    )?;
    update_state(ctx.data_dir, |state| note_expiry(state, &owner.key, live))?;
    Ok(Outcome::Switched {
        adopted,
        previous: Some((owner.key.clone(), park_dir)),
    })
}

/// 提交点：在 Claude Code 的写锁里比对后把默认槽位写成「原来的共享键 + T 的账号键」。
fn commit(ctx: &Ctx<'_>, plan: &Plan, started: Instant) -> Result<(), AppError> {
    // 上一次切换做完了、还没被确认（启动时的补救还没跑到）：这次没写进去就把它放回去
    let mut prior = None;
    update_state(ctx.data_dir, |state| {
        prior = state
            .journal
            .replace(plan.journal.clone())
            .filter(|journal| journal.step == Step::Done);
    })?;
    let cancel = |error: AppError| -> AppError {
        let _ = update_state(ctx.data_dir, |state| state.journal = prior.clone());
        error
    };
    if started.elapsed() > COMMIT_DEADLINE {
        return Err(cancel(AppError::localized(
            "claude.snapshot.too_slow",
            "等待 Claude Code 的锁或钥匙串太久，切换已取消，请再试一次",
            "Waiting for Claude Code's locks or the keychain took too long, so the switch was canceled. Try again.",
        )));
    }
    let planned = merge(plan.live.as_ref(), &plan.target_secret);
    {
        let _storage = storage_lock(&ctx.default).map_err(cancel)?;
        let now = ctx.store.read(&ctx.default).map_err(cancel)?;
        if now.as_ref().map(Secret::fingerprint) != plan.live.as_ref().map(Secret::fingerprint) {
            // 读完之后 Claude Code 改过（登录、MCP 授权等）：什么都不写，下次再来
            return Err(cancel(changed()));
        }
        // 写失败（含结果不明）时日志留着，下次按默认槽位的实际内容判断
        write_verified(ctx.store, &ctx.default, &planned, || {
            AppError::localized(
                "claude.snapshot.verify_failed",
                "写入 Claude Code 的登录后读回不一致，请重试",
                "The Claude Code sign-in read back differently after writing. Try again.",
            )
        })?;
    }
    update_state(ctx.data_dir, |state| {
        if let Some(journal) = state.journal.as_mut() {
            journal.step = Step::Committed;
        }
    })?;
    Ok(())
}

/// 提交点之后的步骤（切换和恢复共用，可重复执行）：换资料，把 T 的账号键从它自己的槽位里
/// 去掉（只在那份还是被移走的同一份时；MCP 授权等其余凭据留在原处）。`target_profile` 是 T
/// 账号目录里的 `.claude.json`。
pub(super) fn finish(
    ctx: &Ctx<'_>,
    journal: &Journal,
    target_profile: &Value,
) -> Result<(), AppError> {
    let mut profile = target_profile
        .get("oauthAccount")
        .cloned()
        .unwrap_or(Value::Null);
    if let Some(account) = profile.as_object_mut() {
        // 记成刚拉过：资料超过 24 小时，还没换过来的旧会话会用它内存里的旧令牌重拉、把旧号的
        // 资料写回来
        account.insert(
            "profileFetchedAt".to_string(),
            Value::from(chrono::Utc::now().timestamp_millis()),
        );
    }
    if profile.is_object() {
        write_profile(&ctx.default.global_config(), Some(&profile))?;
    }
    // 用的是备份时，备份里那份现在就在默认槽位
    let backup = backup_slot(&journal.target_dir);
    if let Some(secret) = ctx.store.read(&backup)? {
        if account_fingerprint(&secret) == journal.target_account {
            ctx.store.delete(&backup)?;
        }
    }
    let target_slot = Slot::Account(journal.target_dir.clone());
    {
        let _storage = storage_lock(&target_slot)?;
        if let Some(secret) = ctx.store.read(&target_slot)? {
            if account_fingerprint(&secret) == journal.target_account {
                strip_account_keys(ctx.store, &target_slot, &secret)?;
            }
        }
    }
    update_state(ctx.data_dir, |state| {
        // 在默认槽位里由 Claude Code 自己续期
        state.login_expiry.remove(&journal.target);
        state.journal = Some(Journal {
            step: Step::Done,
            ..journal.clone()
        });
    })?;
    Ok(())
}

/// 把 `target` 从默认槽位移回它自己的槽位，默认槽位回到没有登录：切换前默认槽位本来就是
/// 空的、后面写卡片失败时用。默认槽位里已经不是 `target`（用户期间登了别的号）就不动。
pub fn vacate(ctx: &Ctx<'_>, target: &str, target_dir: &Path) -> Result<(), AppError> {
    let _serial = SWITCH
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    recover_locked(ctx)?;
    let default = &ctx.default;
    // 目标槽位的锁由 park 拿（锁不可重入，这里先拿会自己等自己）
    let _refresh = refresh_locks(default)?;
    let Some(live) = ctx.store.read(default)?.filter(has_login) else {
        return Ok(());
    };
    if default_owner(default).as_deref() != Some(target) {
        return Ok(());
    }
    let live_profile = read_profile(&default.global_config());
    park(
        ctx.store,
        &live,
        live_profile.as_ref(),
        target,
        &Slot::Account(target_dir.to_path_buf()),
    )?;
    {
        let _storage = storage_lock(default)?;
        let now = ctx.store.read(default)?;
        if now.as_ref().map(Secret::fingerprint) != Some(live.fingerprint()) {
            return Err(changed());
        }
        strip_account_keys(ctx.store, default, &live)?;
    }
    write_profile(&default.global_config(), None)?;
    update_state(ctx.data_dir, |state| note_expiry(state, target, &live))?;
    log::info!("[ClaudeAccount] 已把登录移回账号目录，默认登录恢复为空");
    Ok(())
}

/// 把默认槽位里 O 的账号键写回 O 的账号槽位（覆盖：默认槽位里的那份是 O 最新在用的）。
/// `live_profile` 是默认槽位的 `.claude.json`、`owner` 是核实过的 O：账号目录里的资料已经
/// 不是 O 的（从它的终端里 `/login` 过别的号）时一起换回来，否则切回 O 时对不上身份。
fn park(
    store: &dyn CredentialStore,
    live: &Secret,
    live_profile: Option<&Value>,
    owner: &str,
    slot: &Slot,
) -> Result<(), AppError> {
    let _refresh = refresh_locks(slot)?;
    let path = slot.global_config();
    let dir_owner = read_profile(&path).and_then(|doc| identity::identity_from_profile(&doc));
    if dir_owner.map(|found| found.key).as_deref() != Some(owner) {
        if let Some(account) = live_profile.and_then(|doc| doc.get("oauthAccount")) {
            write_profile(&path, Some(account))?;
        }
    }
    let _storage = storage_lock(slot)?;
    let existing = store.read(slot)?;
    if let (Some(existing), Slot::Account(dir)) = (existing.as_ref(), slot) {
        if has_login(existing) && account_fingerprint(existing) != account_fingerprint(live) {
            keep_backup(store, dir, existing)?;
        }
    }
    write_verified(store, slot, &merge(existing.as_ref(), live), || {
        AppError::localized(
            "claude.snapshot.park_failed",
            "保存原来的 Claude 登录失败，没有切换",
            "Saving the previous Claude sign-in failed, so nothing was switched.",
        )
    })
}

/// 收回时账号槽位里原有的另一份登录：账号键挪到备份槽位（覆盖上一份备份）。写不进去就不切换。
fn keep_backup(store: &dyn CredentialStore, dir: &Path, existing: &Secret) -> Result<(), AppError> {
    let backup_dir: PathBuf = backup_dir(dir);
    std::fs::create_dir_all(&backup_dir).map_err(|source| AppError::io(&backup_dir, source))?;
    write_verified(
        store,
        &Slot::Account(backup_dir),
        &merge(None, existing),
        || {
            AppError::localized(
                "claude.snapshot.backup_failed",
                "备份账号原有的 Claude 登录失败，没有切换",
                "Backing up the account's existing Claude sign-in failed, so nothing was switched.",
            )
        },
    )?;
    log::warn!("[ClaudeAccount] 账号槽位里原有另一份登录，已留作备份");
    Ok(())
}
