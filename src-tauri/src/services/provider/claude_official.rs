//! Claude 官方卡绑定的账号。
//!
//! 官方卡可以在授权中心里绑一个 Claude 官方账号（`authBinding.authProvider = "claude_official"`，
//! `accountId` 是账号 key）。没绑账号的官方卡用 Claude Code 现在的登录（`~/.claude`）。
//!
//! - 从卡片「打开终端」：绑了账号就把它的目录放进那个终端的启动环境（所有平台）。
//! - 切到绑号卡时把账号的登录移进默认槽位（见 `claude_account::snapshot`），之后任何地方
//!   新启动的 `claude` 都是这个号。设了 Claude 配置目录覆盖时不切，只能从卡片打开终端。
//!
//! 以前的开发版把 `CLAUDE_CONFIG_DIR` 写进过 `~/.claude/settings.json`（Claude Code 只在启动
//! 环境里完整认它，写在 settings 里登录状态仍从默认目录读）。切换时照样删掉 CC Switch 写的值
//! （形如 `<数据目录>/claude-accounts/<id>/config`），用户自己写的不碰。

use serde_json::Value;

use crate::app_config::AppType;
use crate::database::Database;
use crate::error::AppError;
use crate::live::patch::json::JsonPatch;
use crate::live::patch::KeyPath;
use crate::provider::Provider;
use crate::services::claude_account::{self, layout, snapshot, ClaudeAccountManager};
use crate::store::AppState;

/// 授权中心里 Claude 官方账号的认证供应商标识。
pub const AUTH_PROVIDER: &str = "claude_official";
pub const CONFIG_DIR_ENV: &str = "CLAUDE_CONFIG_DIR";

/// Claude 官方卡（预设标了 `category: "official"`）。
pub fn is_official(provider: &Provider) -> bool {
    provider.category.as_deref() == Some("official")
}

/// 官方卡绑定的账号 key；没绑（默认登录）或不是官方卡时为 `None`。
pub fn bound_account(provider: &Provider) -> Option<String> {
    if !is_official(provider) {
        return None;
    }
    provider
        .meta
        .as_ref()?
        .managed_account_id_for(AUTH_PROVIDER)
        .filter(|id| !id.is_empty())
}

fn config_dir_key() -> KeyPath {
    KeyPath::new(&["env"]).child(CONFIG_DIR_ENV)
}

/// live 里这个值是不是 CC Switch 写的账号目录。
fn owned_by_cc_switch(value: &Value) -> bool {
    value.as_str().is_some_and(|text| {
        layout::is_account_config_dir(&crate::config::get_app_config_dir(), text)
    })
}

/// 删掉以前开发版写进 `settings.json` 的账号目录。
pub fn strip_from_patch(patch: &mut JsonPatch) {
    patch
        .remove_when
        .push((config_dir_key(), owned_by_cc_switch));
}

/// 写一张卡到 live（`write`）前后的登录切换：`target` 是要切到的卡（`None` 是不切登录），
/// 先把它绑的账号移进默认槽位，`write` 失败就换回原来的号，最后确认切换日志。
pub fn with_login_switch<R>(
    target: Option<&Provider>,
    write: impl FnOnce() -> Result<R, AppError>,
) -> Result<R, AppError> {
    let previous = match target {
        Some(target) => switch_login(target)?,
        None => None,
    };
    let result = write();
    settle(previous, result.is_ok());
    result
}

/// 换掉登录之前默认槽位里是什么：切换的后续步骤失败时用来换回去。
enum PreviousLogin {
    /// 原来的号和它被收回到的目录
    Account {
        account: String,
        dir: std::path::PathBuf,
    },
    /// 原来没有登录：把刚移进去的 `target` 移回它的目录
    Empty {
        target: String,
        dir: std::path::PathBuf,
    },
}

/// 切到（或重新应用）`target` 之前：`target` 绑了本机账号时，把这个账号的登录移进
/// 默认槽位。账号不在这台电脑上（别的设备同步来的卡）时不动登录，只记日志。
/// 真的换了号时返回原来的号。
fn switch_login(target: &Provider) -> Result<Option<PreviousLogin>, AppError> {
    if !snapshot::active() {
        return Ok(None);
    }
    let Some(account) = bound_account(target) else {
        return Ok(None);
    };
    let data_dir = crate::config::get_app_config_dir();
    let Some(dir) = claude_account::config_dir_on_disk(&data_dir, &account) else {
        log::warn!("[ClaudeAccount] 卡片绑定的账号不在这台电脑上，不切换登录");
        return Ok(None);
    };
    // 切过去之前把共享层同步到最新（默认的设置、MCP 可能在这之后改过）
    claude_account::sync_shared(&dir);
    let outcome = with_snapshot_ctx(&data_dir, |ctx| snapshot::check_out(ctx, &account, &dir))?;
    Ok(match outcome {
        snapshot::Outcome::Switched {
            previous: Some((account, dir)),
            ..
        } => Some(PreviousLogin::Account { account, dir }),
        snapshot::Outcome::Switched { previous: None, .. } => Some(PreviousLogin::Empty {
            target: account,
            dir,
        }),
        snapshot::Outcome::AlreadyLive => None,
    })
}

/// 换完登录之后写卡片的结果出来了：失败就按配置的走向决定换不换回。写成了、或换回成功（或
/// 不用换回）就确认切换日志；换回失败时留着，启动时按卡片指针再核对一次。
fn settle(previous: Option<PreviousLogin>, written: bool) {
    let settled = written || previous.is_none_or(restore_login);
    if settled {
        snapshot::confirm(&crate::config::get_app_config_dir());
    }
}

/// 登录换过去了、但 settings.json 或卡片指针没写成：把登录换回原来的号，免得界面说没切、
/// 实际已经换了号。换不回去只记日志（原来的号在它的账号目录里，下次切过去就回来了）。
///
/// 配置操作已经开始发布时不换回：它会在下次操作或启动时前滚到这张卡，登录要和它同向。
/// 返回登录是否已经和卡片一致（换回了，或不用换回）。
fn restore_login(previous: PreviousLogin) -> bool {
    let store = crate::live::engine::DeviceStore::for_device();
    if matches!(
        crate::mode::operation::pending_published(&store, AppType::Claude.as_str()),
        Ok(Some(true))
    ) {
        log::info!("[ClaudeAccount] 配置写入会前滚补完，登录保持切换后的账号");
        return true;
    }
    let data_dir = crate::config::get_app_config_dir();
    let result = with_snapshot_ctx(&data_dir, |ctx| match &previous {
        PreviousLogin::Account { account, dir } => {
            snapshot::check_out(ctx, account, dir).map(|_| ())
        }
        PreviousLogin::Empty { target, dir } => snapshot::vacate(ctx, target, dir),
    });
    match result {
        Ok(()) => true,
        Err(error) => {
            log::warn!("[ClaudeAccount] 切换失败后换回原来的登录也失败，启动时再试: {error}");
            false
        }
    }
}

/// 启动时补完上次没做完的登录切换（崩溃、断电）。要在补完配置操作之后调用（指针已落定）。
///
/// 登录换过去以后、卡片指针更新之前崩溃的话，指针还停在原来的卡上：这时把登录换回切换前
/// 的号，和指针一致。
pub fn recover_login_switch(db: &Database) {
    let data_dir = crate::config::get_app_config_dir();
    let Some(journal) = snapshot::load_state(&data_dir).journal else {
        return;
    };
    if journal.step != snapshot::Step::Done {
        if let Err(error) = with_snapshot_ctx(&data_dir, snapshot::recover) {
            log::warn!("[ClaudeAccount] 补完上次的登录切换失败: {error}");
            return;
        }
    }
    let Some(journal) = snapshot::load_state(&data_dir)
        .journal
        .filter(|journal| journal.step == snapshot::Step::Done)
    else {
        return;
    };
    // 指针已经是这个号的卡，或者用户之后在 Claude Code 里换了别的号：都不用动。换回失败时
    // 日志留着，下次启动再试
    let pointer_matches = pointer_account(db).as_deref() == Some(journal.target.as_str());
    let mut settled = true;
    if !pointer_matches && snapshot::is_live(&journal.target) {
        log::info!("[ClaudeAccount] 上次切换没来得及更新卡片，把登录换回原来的号");
        let previous = match journal.before_owner {
            Some(account) => claude_account::config_dir_on_disk(&data_dir, &account)
                .map(|dir| PreviousLogin::Account { account, dir }),
            None => Some(PreviousLogin::Empty {
                target: journal.target,
                dir: journal.target_dir,
            }),
        };
        if let Some(previous) = previous {
            settled = restore_login(previous);
        }
    }
    if settled {
        snapshot::confirm(&data_dir);
    }
}

/// 账号 `account` 刚在授权中心重新登录（新登录在 `dir`）：它正在默认槽位里时，
/// 用新登录换掉默认槽位里的旧登录（旧的可能已过期或被吊销，否则重新登录了 `claude` 还是用
/// 不了）。登录成功本身不受影响，这里失败只记日志。
pub fn replace_live_login(account: &str, dir: &std::path::Path) {
    if !snapshot::active() {
        return;
    }
    let data_dir = crate::config::get_app_config_dir();
    match with_snapshot_ctx(&data_dir, |ctx| snapshot::replace_live(ctx, account, dir)) {
        Ok(true) => {
            // 同一个号换了一份登录，卡片不用动
            snapshot::confirm(&data_dir);
            log::info!("[ClaudeAccount] 重新登录的账号正在使用，已换上新登录");
        }
        Ok(false) => {}
        Err(error) => log::warn!("[ClaudeAccount] 重新登录后换上新登录失败: {error}"),
    }
}

/// 直连指针指着的那张卡绑的账号。
fn pointer_account(db: &Database) -> Option<String> {
    let app = AppType::Claude;
    let current =
        crate::mode::current::provider_for(db, &app, crate::mode::current::Purpose::Direct)
            .ok()??;
    let providers = db.get_all_providers(app.as_str()).ok()?;
    bound_account(providers.get(&current)?)
}

/// 账号在默认槽位、要用默认目录开终端时：Claude Code 现在用的必须是官方卡，否则
/// `settings.json` 里那一家的地址、Key（或本地路由）会盖过这个账号的登录。
pub fn ensure_default_terminal_is_official(db: &Database) -> Result<(), AppError> {
    let app = AppType::Claude;
    let in_use = if crate::mode::current::is_proxy(&app) {
        None
    } else {
        crate::mode::current::provider_for(db, &app, crate::mode::current::Purpose::Direct)?
            .and_then(|id| db.get_all_providers(app.as_str()).ok()?.get(&id).cloned())
    };
    if in_use.as_ref().is_some_and(is_official) {
        return Ok(());
    }
    Err(AppError::localized(
        "claude.account.default_taken",
        "这个账号现在在 Claude Code 的默认登录里，但 Claude Code 当前用的是其他供应商或本地路由，打开的终端不会用这个账号。请先切到这张卡（或任一官方卡）再打开终端",
        "This account is Claude Code's current sign-in, but Claude Code is set to another provider or the local router, so a new terminal wouldn't use this account. Switch to this card (or any official card) first, then open the terminal.",
    ))
}

fn with_snapshot_ctx<R>(
    data_dir: &std::path::Path,
    f: impl FnOnce(&snapshot::Ctx<'_>) -> Result<R, AppError>,
) -> Result<R, AppError> {
    let store = snapshot::store::platform_store();
    let accounts = claude_account::DiskAccounts {
        data_dir: data_dir.to_path_buf(),
    };
    f(&snapshot::Ctx {
        data_dir,
        default: snapshot::store::Slot::default_slot(),
        store: store.as_ref(),
        accounts: &accounts,
        adopter: &accounts,
    })
}

/// 账号增删、重新登录之后：当前直连的是官方卡时按新的账号表重写一次 live（账号目录变了、
/// 或绑定的账号没了要退回默认登录）。代理模式下 live 是代理契约，不碰。
pub async fn refresh_live_after_account_change(
    state: &AppState,
    _accounts: &ClaudeAccountManager,
) -> Result<(), String> {
    let app = AppType::Claude;
    let _guard = crate::mode::controller::lock_settled(state, &app)
        .await
        .map_err(|error| error.to_string())?;
    if crate::mode::current::is_proxy(&app) {
        return Ok(());
    }
    let current =
        crate::mode::current::provider_for(&state.db, &app, crate::mode::current::Purpose::Direct)
            .map_err(|error| error.to_string())?;
    let Some(current) = current else {
        return Ok(());
    };
    let providers = state
        .db
        .get_all_providers(app.as_str())
        .map_err(|error| error.to_string())?;
    let Some(provider) = providers.get(&current) else {
        return Ok(());
    };
    if !is_official(provider) {
        return Ok(());
    }
    // 可能要等 Claude Code 的锁、访问钥匙串：别占着异步线程
    let db = state.db.clone();
    let provider = provider.clone();
    tokio::task::spawn_blocking(move || {
        super::claude_direct::reapply(db.as_ref(), Some(&provider), &provider).map(|_| ())
    })
    .await
    .map_err(|error| error.to_string())?
    .map_err(|error| error.to_string())
}

/// 从卡片打开终端时，绑号卡的 `claude` 用哪里的登录。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccountTerminal {
    /// 账号自己的目录（`CLAUDE_CONFIG_DIR=<目录>`）
    Dir(std::path::PathBuf),
    /// 账号正在默认槽位里（切卡时移进去的，它自己的目录这时是空的）：用默认目录
    Default,
}

/// 绑了本机账号的官方卡打开终端时用哪里的登录（所有平台）；没绑为 `None`。
/// 绑的账号不在这台电脑上时报错，免得悄悄用错号。
pub fn terminal_account(provider: &Provider) -> Result<Option<AccountTerminal>, AppError> {
    let Some(account) = bound_account(provider) else {
        return Ok(None);
    };
    let dir = claude_account::config_dir_on_disk(&crate::config::get_app_config_dir(), &account);
    // 账号目录里有自己的登录就用它；没有、而这个号在默认槽位里（切卡时移进去的）才用默认目录。
    // 只看身份不够：默认目录和账号目录可能各登着同一个号（用户自己在 Claude Code 里登的）
    if let Some(dir) = dir.as_ref().filter(|dir| snapshot::dir_has_login(dir)) {
        return Ok(Some(AccountTerminal::Dir(dir.clone())));
    }
    if snapshot::is_live(&account) {
        return Ok(Some(AccountTerminal::Default));
    }
    match dir {
        Some(dir) => Ok(Some(AccountTerminal::Dir(dir))),
        None => Err(AppError::localized(
            "claude.account.not_on_device",
            format!(
                "「{}」绑定的 Claude 账号不在这台电脑上。请在授权中心添加它，或编辑卡片换一个账号",
                provider.name
            ),
            format!(
                "The Claude account bound to \"{}\" isn't on this computer. Add it in the Auth Center, or edit the card to choose another account.",
                provider.name
            ),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{AuthBinding, AuthBindingSource, ProviderMeta};
    use serde_json::json;

    fn official(binding: Option<&str>) -> Provider {
        let mut provider = Provider::with_id(
            "official".into(),
            "Claude Official".into(),
            json!({"env": {}}),
            None,
        );
        provider.category = Some("official".into());
        if let Some(account) = binding {
            provider.meta = Some(ProviderMeta {
                auth_binding: Some(AuthBinding {
                    source: AuthBindingSource::ManagedAccount,
                    auth_provider: Some(AUTH_PROVIDER.into()),
                    account_id: Some(account.into()),
                }),
                ..Default::default()
            });
        }
        provider
    }

    #[test]
    fn only_official_cards_have_bound_accounts() {
        assert_eq!(bound_account(&official(None)), None);
        assert_eq!(
            bound_account(&official(Some("acc:org"))).as_deref(),
            Some("acc:org")
        );
        let mut third_party = official(Some("acc:org"));
        third_party.category = Some("third_party".into());
        assert_eq!(bound_account(&third_party), None);
    }

    #[test]
    fn patch_removes_only_cc_switch_owned_dirs() {
        let data_dir = crate::config::get_app_config_dir();
        let ours = layout::config_dir(&data_dir, "0123456789abcdef0123456789abcdef");
        let ours = ours.to_string_lossy().into_owned();
        let mut patch = JsonPatch::default();
        strip_from_patch(&mut patch);
        // 只删以前写的指针，不再写新的
        assert!(patch.set.is_empty());
        let path = std::path::Path::new("settings.json");

        let mut doc = json!({"env": {"CLAUDE_CONFIG_DIR": ours, "KEEP": "1"}});
        patch.apply_to(path, &mut doc).unwrap();
        assert_eq!(doc, json!({"env": {"KEEP": "1"}}));

        let mut doc = json!({"env": {"CLAUDE_CONFIG_DIR": "/home/me/work-claude"}});
        patch.apply_to(path, &mut doc).unwrap();
        assert_eq!(
            doc,
            json!({"env": {"CLAUDE_CONFIG_DIR": "/home/me/work-claude"}})
        );
    }

    #[test]
    fn unbound_card_on_unknown_account_writes_nothing() {
        // 绑了但本机没有这个账号：打开终端报错，默认登录的卡不设目录
        assert!(terminal_account(&official(Some("missing:acc"))).is_err());
        assert!(terminal_account(&official(None)).unwrap().is_none());
    }
}
