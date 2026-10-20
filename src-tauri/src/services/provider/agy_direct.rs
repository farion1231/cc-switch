//! 切换 Antigravity CLI：只改 `settings.json`。第三方 Key 不进任何文件，只在 CC Switch
//! 打开的终端里经环境变量传给 agy（见 `agy_config` 模块说明）。

use serde_json::{json, Value};

use crate::agy_config;
use crate::database::Database;
use crate::error::AppError;
use crate::mode::operation::OperationReport;
use crate::mode::state::{op, PendingTarget};
use crate::provider::Provider;

pub(crate) fn is_official(provider: &Provider) -> bool {
    // Provider names are user editable and must never silently select OAuth.
    provider.category.as_deref() == Some("official")
        || provider
            .meta
            .as_ref()
            .and_then(|meta| meta.partner_promotion_key.as_deref())
            == Some("google-official")
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

pub(crate) fn switch_to(db: &Database, target: &Provider) -> Result<OperationReport, AppError> {
    let settings = live_settings(target)?;
    agy_config::apply_managed(
        db,
        &settings,
        op::SWITCH,
        PendingTarget::pointer(Some(target.id.clone())),
    )
}

pub(crate) fn reapply(db: &Database, target: &Provider) -> Result<OperationReport, AppError> {
    let settings = live_settings(target)?;
    // 全量同步（导入、云同步）会对每个当前供应商调用这里。没有真实内容的供应商
    // 不去改 agy 首次启动写出的 settings.json。
    if !agy_config::has_managed_content(&settings) {
        return Ok(OperationReport::default());
    }
    agy_config::apply_managed(db, &settings, op::APPLY, PendingTarget::default())
}

/// 在终端里启动这张卡要用的参数；Key 卡会先建好它的私有目录。
pub(crate) fn terminal_launch(provider: &Provider) -> Result<agy_config::AgyLaunch, AppError> {
    let settings = live_settings(provider)?;
    agy_config::prepare_launch(&provider.id, &settings)
}

/// 启动时调用：撤掉 #8268 测试版装的包装脚本、`env` 文件和 shell PATH 片段。清掉过
/// `env` 的说明当时写过 `modelProvider`，按新规则把当前供应商重写一次，让直接敲 `agy`
/// 回到官方登录。没有旧现场时什么都不做。
pub(crate) fn migrate_legacy_launcher(db: &Database) -> Result<(), AppError> {
    use crate::app_config::AppType;
    use crate::mode::current::{provider_for, Purpose};
    if !agy_config::remove_legacy_launcher()? {
        return Ok(());
    }
    let current = match provider_for(db, &AppType::Agy, Purpose::Direct)? {
        Some(id) => db.get_provider_by_id(&id, "agy")?,
        None => None,
    };
    match &current {
        Some(provider) => {
            agy_config::apply_managed(
                db,
                &live_settings(provider)?,
                op::APPLY,
                PendingTarget::default(),
            )?;
        }
        None => {
            agy_config::release_model_provider(db)?;
        }
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
}
