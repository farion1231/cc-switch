//! 切换 Antigravity CLI：写 `settings.json`，并把 Key / 地址注入启动包装。

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
    if !is_official(provider) {
        crate::gemini_config::validate_gemini_settings_strict(&provider.settings_config)?;
    }
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
        if value
            .as_str()
            .is_some_and(|text| text.chars().any(char::is_control))
        {
            return Err(AppError::Config(
                "Antigravity credentials, endpoint and model must be single-line values".into(),
            ));
        }
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
    agy_config::apply_managed(db, &settings, op::APPLY, PendingTarget::default())
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
    fn agy_invalid_credentials_fail_before_writing() {
        let mut row = provider("Relay");
        row.settings_config["env"]["GEMINI_API_KEY"] = json!("bad\nkey");
        assert!(live_settings(&row).is_err());
    }
}
