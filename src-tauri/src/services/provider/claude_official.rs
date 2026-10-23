//! Claude 官方卡绑定的账号 → `settings.json` 的 `env.CLAUDE_CONFIG_DIR`。
//!
//! 官方卡可以在授权中心里绑一个 Claude 官方账号（`authBinding.authProvider = "claude_official"`，
//! `accountId` 是账号 key）。切到这张卡时把这个账号在本机的目录写进 `env.CLAUDE_CONFIG_DIR`：
//! Claude Code 先读默认的 `settings.json`，看到它就改用账号目录，之后新开的 `claude`、
//! IDE 里的新对话都是这个账号。没绑账号的官方卡就是「默认登录」（`~/.claude`），和以前一样。
//!
//! 这个键的值是本机路径，不进供应商行（行随云同步），切换时由这里按本机账号表注入。
//! 它不是关键字段：用户自己写的 `CLAUDE_CONFIG_DIR` 不能被切换删掉。CC Switch 只删形如
//! `<数据目录>/claude-accounts/<id>/config` 的值，也就是自己写的。
//!
//! macOS 上暂不开放：Claude Code（≤ 2.1.309 已复核）在读 settings 之前就按启动时的环境
//! 预取了钥匙串，缓存不区分条目、30 秒内不失效，新会话开头会用默认登录的令牌发请求。
//! macOS 的官方卡改为「从卡片打开终端」，变量在进程启动前设好，没有这个问题。

use serde_json::Value;

use crate::app_config::AppType;
use crate::error::AppError;
use crate::live::patch::json::JsonPatch;
use crate::live::patch::KeyPath;
use crate::provider::Provider;
use crate::services::claude_account::{self, layout, ClaudeAccountManager};
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

/// 这个平台能不能靠 `settings.json` 全局切换官方账号。
///
/// - macOS：不能（见模块说明），改用从卡片打开终端；
/// - 设了 Claude 配置目录覆盖：不能。覆盖目录要和 shell 里的 `CLAUDE_CONFIG_DIR` 一致
///   Claude Code 才读得到，CC Switch 证明不了这一点。
pub fn global_switch_available() -> bool {
    !cfg!(target_os = "macos") && crate::settings::get_claude_override_dir().is_none()
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

/// 切到 `target` 时 `CLAUDE_CONFIG_DIR` 该是什么：绑了本机账号、平台支持、live 里没有用户
/// 自己的值时，是账号目录；否则不写。
pub fn target_config_dir(target: &Provider) -> Option<String> {
    if !global_switch_available() {
        return None;
    }
    let account = bound_account(target)?;
    let dir = claude_account::config_dir_on_disk(&crate::config::get_app_config_dir(), &account)?;
    if user_owns_live_config_dir() {
        log::warn!("[ClaudeAccount] settings.json 里有用户自己的 CLAUDE_CONFIG_DIR，不写账号目录");
        return None;
    }
    Some(dir.to_string_lossy().into_owned())
}

/// 默认 `settings.json` 里已经有一个不归 CC Switch 的 `CLAUDE_CONFIG_DIR`。
fn user_owns_live_config_dir() -> bool {
    let Ok(bytes) = std::fs::read(crate::config::get_claude_settings_path()) else {
        return false;
    };
    let Ok(doc) = serde_json::from_slice::<Value>(&bytes) else {
        return false;
    };
    doc.get("env")
        .and_then(|env| env.get(CONFIG_DIR_ENV))
        .is_some_and(|value| !owned_by_cc_switch(value))
}

/// 删掉 CC Switch 以前写的账号目录（进入代理模式：Claude 订阅永不走代理）。
pub fn strip_from_patch(patch: &mut JsonPatch) {
    patch
        .remove_when
        .push((config_dir_key(), owned_by_cc_switch));
}

/// 给切换补丁加上账号目录：先删掉 CC Switch 以前写的，`target` 绑了账号就写它的目录。
pub fn add_to_patch(patch: &mut JsonPatch, target: &Provider) {
    strip_from_patch(patch);
    if let Some(dir) = target_config_dir(target) {
        // 切过去之前把共享层同步到最新（默认的设置、MCP 可能在这之后改过）
        claude_account::sync_shared(std::path::Path::new(&dir));
        patch.set.push((config_dir_key(), Value::String(dir)));
    }
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
    super::claude_direct::reapply(state.db.as_ref(), Some(provider), provider)
        .map(|_| ())
        .map_err(|error| error.to_string())
}

/// live 的 `settings.json` 现在指向某个 CC Switch 账号目录（全局用的是那个账号）。
pub fn live_points_to_account() -> bool {
    let Ok(bytes) = std::fs::read(crate::config::get_claude_settings_path()) else {
        return false;
    };
    serde_json::from_slice::<Value>(&bytes)
        .ok()
        .and_then(|doc| doc.get("env")?.get(CONFIG_DIR_ENV).cloned())
        .is_some_and(|value| owned_by_cc_switch(&value))
}

/// 从卡片打开终端时给 `claude` 进程的账号目录：绑了本机账号就是它的目录（所有平台）。
/// 绑的账号不在这台电脑上时报错，免得悄悄用错号。
pub fn terminal_config_dir(provider: &Provider) -> Result<Option<std::path::PathBuf>, AppError> {
    let Some(account) = bound_account(provider) else {
        // 默认登录：全局指针指着别的账号时，新开的 claude 会跟着 settings.json 跳过去，
        // 又没有办法在启动环境里表达「用默认目录」（显式写 ~/.claude 会换钥匙串条目和
        // .claude.json 的位置），只能请用户先切回来。
        if is_official(provider) && live_points_to_account() {
            return Err(AppError::localized(
                "claude.account.default_shadowed",
                "全局正在使用另一个 Claude 官方账号。要用默认登录，请先切换到这张卡再打开终端",
                "Another Claude account is in use globally. To use the default sign-in, switch to this card first, then open the terminal.",
            ));
        }
        return Ok(None);
    };
    match claude_account::config_dir_on_disk(&crate::config::get_app_config_dir(), &account) {
        Some(dir) => Ok(Some(dir)),
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
        add_to_patch(&mut patch, &official(None));
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
        // 绑了但本机没有这个账号：不写目录，退回默认登录
        assert_eq!(target_config_dir(&official(Some("missing:acc"))), None);
        assert!(terminal_config_dir(&official(Some("missing:acc"))).is_err());
        assert!(terminal_config_dir(&official(None)).unwrap().is_none());
    }
}
