//! Antigravity CLI 的卡片只用来打开终端，没有「启用」：官方卡按会话传 `--model`，第三方
//! Key 不进任何文件，只在 CC Switch 打开的终端里经环境变量传给 agy（见 `agy_config` 模块说明）。

use serde_json::{json, Value};

use crate::agy_config;
use crate::app_config::AppType;
use crate::database::Database;
use crate::error::AppError;
use crate::provider::Provider;
use crate::services::agy_account::{keychain, AgyAccountManager};
use crate::services::sync_protocol::sync_mutex;
use crate::store::AppState;

use super::{lock_native_provider_operations, ProviderService};

pub(crate) fn is_official(provider: &Provider) -> bool {
    // Provider names are user editable and must never silently select OAuth.
    provider.has_official_category()
        || provider
            .meta
            .as_ref()
            .and_then(|meta| meta.partner_promotion_key.as_deref())
            == Some("google-official")
}

fn bound_account_id(provider: &Provider) -> Option<String> {
    if !is_official(provider) {
        return None;
    }
    provider
        .meta
        .as_ref()?
        .managed_account_id_for("agy_oauth")
        .filter(|id| !id.trim().is_empty())
}

/// 保存官方卡的新账号绑定时，同步 agy 的全局登录。只改模型等字段时保留当前登录。
pub(crate) fn save_from_editor(
    state: &AppState,
    accounts: &AgyAccountManager,
    mut provider: Provider,
) -> Result<bool, AppError> {
    let app = AppType::Agy;
    let binding = bound_account_id(&provider);
    // Take the sync lock first, as restores may project provider configuration.
    // Snapshots must wait until the login switch has succeeded or the row is restored.
    let _sync_guard = (binding.is_some()
        && should_switch_bound_account(
            keychain::supported(),
            crate::commands::tool_runs_in_wsl("agy"),
        ))
    .then(|| sync_mutex().blocking_lock());
    let _guard = lock_native_provider_operations(state, &app);
    ProviderService::prepare_new_provider(&app, &mut provider)?;
    let previous = state.db.get_provider_by_id(&provider.id, &app)?;
    let binding_changed = binding != previous.as_ref().and_then(bound_account_id);

    state.db.save_provider(&app, &provider)?;
    if !binding_changed {
        return Ok(true);
    }

    let written = tauri::async_runtime::block_on(switch_to_bound_account(accounts, &provider));
    ProviderService::keep_row_if_written(state, &app, &provider.id, previous.as_ref(), written)
}

fn should_switch_bound_account(keychain_supported: bool, runs_in_wsl: bool) -> bool {
    keychain_supported && !runs_in_wsl
}

/// 用授权中心的同一套切换逻辑，写入并核对 agy 的实际登录。
pub(crate) async fn switch_to_bound_account(
    accounts: &AgyAccountManager,
    provider: &Provider,
) -> Result<(), AppError> {
    let Some(account_id) = bound_account_id(provider) else {
        return Ok(());
    };
    let runs_in_wsl = crate::commands::tool_runs_in_wsl("agy");
    if !should_switch_bound_account(keychain::supported(), runs_in_wsl) {
        if runs_in_wsl {
            log::info!("[AgyAccount] Antigravity 运行在 WSL 里，忽略卡片绑定的本机账号");
        }
        return Ok(());
    }
    if !accounts
        .list()
        .await
        .iter()
        .any(|account| account.id == account_id)
    {
        return Err(AppError::localized(
            "agy.account.bound_missing",
            format!(
                "「{}」绑定的 Antigravity 账号不在这台电脑的授权中心里：请先在授权中心添加它，或编辑这张卡改选账号",
                provider.name
            ),
            format!(
                "The Antigravity account bound to \"{}\" is not in this computer's Auth Center. Add it there, or edit the card to choose another account.",
                provider.name
            ),
        ));
    }
    accounts.switch_to(&account_id).await
}

fn live_settings(provider: &Provider) -> Result<Value, AppError> {
    crate::gemini_config::validate_gemini_settings(&provider.settings_config)?;
    let mut settings = provider.settings_config.clone();
    if is_official(provider) {
        if let Some(env) = settings.get_mut("env").and_then(Value::as_object_mut) {
            env.remove("GEMINI_API_KEY");
            env.remove("GOOGLE_GEMINI_BASE_URL");
        } else {
            settings
                .as_object_mut()
                .map(|obj| obj.insert("env".to_string(), json!({})));
        }
    }
    for value in [
        settings.pointer("/env/GEMINI_API_KEY"),
        settings.pointer("/env/GOOGLE_GEMINI_BASE_URL"),
        settings.pointer("/config/model/name"),
        settings.pointer("/env/GEMINI_MODEL"),
    ]
    .into_iter()
    .flatten()
    {
        if !value.is_null() && !value.is_string() {
            return Err(AppError::Config(
                "Antigravity credentials, endpoint and model must be strings".into(),
            ));
        }
        if value
            .as_str()
            .is_some_and(|text| text.chars().any(char::is_control))
        {
            return Err(AppError::Config(
                "Antigravity credentials, endpoint and model must be single-line values".into(),
            ));
        }
    }
    // A model-only environment still uses native login. Gemini CLI's generic
    // validator treats any nonempty env as API authentication, which rejects
    // GEMINI_MODEL written by the shared editor when no key is needed.
    let has_key = settings["env"]["GEMINI_API_KEY"]
        .as_str()
        .is_some_and(|value| !value.trim().is_empty());
    let has_endpoint = settings["env"]["GOOGLE_GEMINI_BASE_URL"]
        .as_str()
        .is_some_and(|value| !value.trim().is_empty());
    if has_endpoint && !has_key {
        return Err(AppError::localized(
            "gemini.validation.missing_api_key",
            "Gemini 配置缺少必需字段: GEMINI_API_KEY",
            "Gemini config missing required field: GEMINI_API_KEY",
        ));
    }
    Ok(settings)
}

/// 在终端里启动这张卡要用的参数；Key 卡会先建好它的私有目录。
pub(crate) fn terminal_launch(provider: &Provider) -> Result<agy_config::AgyLaunch, AppError> {
    let settings = live_settings(provider)?;
    agy_config::prepare_launch(&provider.id, &settings)
}

/// 会话页的恢复命令如果指向某张 Key 卡的私有目录，返回那张卡和会话 ID，好按「打开终端」
/// 的方式补上 Key 再恢复。认不出（官方登录会话、卡已删除）时返回 `None`，照原命令执行。
pub(crate) fn key_resume_target(
    db: &Database,
    command: &str,
) -> Result<Option<(Provider, String)>, AppError> {
    for provider in db
        .get_provider_rows(&AppType::Agy)?
        .into_readable("识别会话恢复命令")
        .into_values()
    {
        let dir = agy_config::key_profile_dir(&provider.id);
        let prefix = agy_config::resume_command(Some(&dir), "");
        let Some(id) = command.strip_prefix(&prefix) else {
            continue;
        };
        if !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
            return Ok(Some((provider, id.to_string())));
        }
    }
    Ok(None)
}

/// 启动时调用：撤掉 #8268 测试版装的包装脚本、`env` 文件和 shell PATH 片段，以及它写进
/// 真实 `settings.json` 的 `modelProvider`，让直接敲 `agy` 回到官方登录。模型不动。没有旧
/// 现场时什么都不做。
pub(crate) fn migrate_legacy_launcher(db: &Database) -> Result<(), AppError> {
    if agy_config::remove_legacy_launcher()? {
        agy_config::release_model_provider(db)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn provider(name: &str) -> Provider {
        Provider::with_id(
            "test".into(),
            name.into(),
            json!({"env":{"GEMINI_API_KEY":"fixture","GOOGLE_GEMINI_BASE_URL":"https://example.test"},"config":{"model":{"name":"test-model"}}}),
            None,
        )
    }
    #[test]
    fn agy_provider_names_do_not_override_credentials() {
        let row = provider("Google API key");
        assert!(!is_official(&row));
        assert_eq!(
            live_settings(&row).unwrap()["env"]["GEMINI_API_KEY"],
            "fixture"
        );
        let mut official = row;
        official.category = Some("official".into());
        let config = live_settings(&official).unwrap();
        assert!(config["env"].get("GEMINI_API_KEY").is_none());
        assert!(config["env"].get("GOOGLE_GEMINI_BASE_URL").is_none());
        assert_eq!(config["config"]["model"]["name"], "test-model");
    }
    #[test]
    fn agy_model_only_login_does_not_require_api_key() {
        let mut row = provider("Imported login");
        row.settings_config["env"] = json!({"GEMINI_MODEL":"gemini-3.8-flash-low"});
        assert!(live_settings(&row).is_ok());
    }

    #[test]
    fn agy_endpoint_requires_a_nonempty_key() {
        for key in [Value::Null, json!(""), json!("   ")] {
            let mut row = provider("Relay");
            row.settings_config["env"]["GEMINI_API_KEY"] = key;
            assert!(live_settings(&row).is_err());
        }
    }

    #[test]
    fn agy_key_can_use_the_default_google_endpoint() {
        let mut row = provider("Google API key");
        row.settings_config["env"] = json!({"GEMINI_API_KEY":"fixture"});
        assert!(live_settings(&row).is_ok());
    }

    #[test]
    fn agy_rejects_non_string_credentials_before_writing() {
        let mut row = provider("Relay");
        row.settings_config["env"]["GEMINI_API_KEY"] = json!(123);
        assert!(live_settings(&row).is_err());
    }

    #[test]
    fn agy_invalid_credentials_fail_before_writing() {
        let mut row = provider("Relay");
        row.settings_config["env"]["GEMINI_API_KEY"] = json!("bad\nkey");
        assert!(live_settings(&row).is_err());
    }

    #[test]
    fn agy_bound_account_switch_skips_when_running_in_wsl() {
        assert!(should_switch_bound_account(true, false));
        assert!(!should_switch_bound_account(true, true));
        assert!(!should_switch_bound_account(false, false));
        assert!(!should_switch_bound_account(false, true));
    }
}
