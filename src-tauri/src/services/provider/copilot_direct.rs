//! GitHub Copilot CLI 的卡片：官方卡只用来打开终端（按会话传 `--model`，绑了授权中心
//! 账号时经环境变量传令牌），绝不写文件；自带模型卡是累加式，「添加 / 移除」改写 Copilot
//! 原生的 `providers.json`（见 `copilot_config` 模块说明）。

use crate::copilot_config::{self, CopilotLaunch};
use crate::error::AppError;
use crate::provider::Provider;

/// 授权中心里 GitHub 账号的托管标识，和 Copilot 代理供应商共用。
pub(crate) const ACCOUNT_AUTH_PROVIDER: &str = "github_copilot";

/// 官方登录卡：分类标为官方，或者没有自带模型的 `provider` 节点。供应商名称可编辑，不参与判断。
pub(crate) fn is_official(provider: &Provider) -> bool {
    provider.category.as_deref() == Some("official")
        || !copilot_config::is_byok_settings(&provider.settings_config)
}

/// 在终端里启动这张卡要用的参数。自带模型卡必须已经添加进 `providers.json`：Copilot 只从
/// 那里认 `<卡片 id>/<模型>`。
pub(crate) fn terminal_launch(provider: &Provider) -> Result<CopilotLaunch, AppError> {
    if is_official(provider) {
        // 官方卡只认模型和推理强度，自带模型字段一概不用。
        let mut settings = serde_json::json!({});
        if let Some(env) = provider.settings_config.get("env") {
            let model = env.get(copilot_config::MODEL_ENV).cloned();
            settings["env"] = serde_json::json!({});
            if let Some(model) = model {
                settings["env"][copilot_config::MODEL_ENV] = model;
            }
        }
        if let Some(effort) = provider
            .settings_config
            .get(copilot_config::REASONING_EFFORT_KEY)
        {
            settings[copilot_config::REASONING_EFFORT_KEY] = effort.clone();
        }
        return copilot_config::prepare_launch(&provider.id, &settings);
    }
    if !copilot_config::provider_key_exists(&provider.id)? {
        return Err(AppError::localized(
            "copilot.launch.not_added",
            format!(
                "「{}」还没有添加到 GitHub Copilot CLI：先点「添加」再打开终端",
                provider.name
            ),
            format!(
                "\"{}\" has not been added to GitHub Copilot CLI yet. Add it first, then open a terminal",
                provider.name
            ),
        ));
    }
    copilot_config::prepare_launch(&provider.id, &provider.settings_config)
}

/// 官方卡绑定的授权中心 GitHub 账号。
pub(crate) fn bound_account_id(provider: &Provider) -> Option<String> {
    if !is_official(provider) {
        return None;
    }
    provider
        .meta
        .as_ref()
        .and_then(|meta| meta.managed_account_id_for(ACCOUNT_AUTH_PROVIDER))
        .filter(|id| !id.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_config::AppType;
    use crate::copilot_config::test_support::CopilotHomeGuard;
    use crate::database::Database;
    use crate::services::provider::ProviderService;
    use crate::store::AppState;
    use serde_json::{json, Value};
    use serial_test::serial;
    use std::sync::Arc;

    fn provider(settings: Value) -> Provider {
        Provider::with_id("test".into(), "GitHub API key".into(), settings, None)
    }

    fn byok_settings() -> Value {
        json!({
            "provider": {"type":"openai","wireApi":"responses","baseUrl":"https://relay.test/v1","apiKey":"fixture"},
            "models": [{"id":"gpt-5.4"}],
            "reasoningEffort": "high"
        })
    }

    #[test]
    fn provider_names_do_not_decide_the_login_mode() {
        let row = provider(byok_settings());
        assert!(!is_official(&row));

        let mut official = row;
        official.category = Some("official".into());
        assert!(is_official(&official));
        let launch = terminal_launch(&official).unwrap();
        assert!(!launch.byok);
        assert_eq!(launch.model, None);
        assert_eq!(launch.reasoning_effort.as_deref(), Some("high"));
        assert!(launch.env.iter().all(|(_, value)| value.is_none()));
    }

    #[test]
    fn model_only_cards_use_the_github_login() {
        let row = provider(json!({"env":{"COPILOT_MODEL":"claude-sonnet-4.6"}}));
        assert!(is_official(&row));
        assert_eq!(bound_account_id(&row), None);
        let launch = terminal_launch(&row).unwrap();
        assert_eq!(launch.model.as_deref(), Some("claude-sonnet-4.6"));
    }

    /// 临时 home + 临时 `COPILOT_HOME` 下跑 Copilot 的供应商服务。
    fn with_copilot_home(test: impl FnOnce(&AppState, &std::path::Path)) {
        let dir = tempfile::tempdir().unwrap();
        let old_test_home = std::env::var_os("CC_SWITCH_TEST_HOME");
        std::env::set_var("CC_SWITCH_TEST_HOME", dir.path());
        let copilot_home = dir.path().join(".copilot");
        let guard = CopilotHomeGuard::new(&copilot_home);
        let state = AppState::new(Arc::new(Database::memory().unwrap()));
        test(&state, &copilot_home);
        drop(guard);
        match old_test_home {
            Some(value) => std::env::set_var("CC_SWITCH_TEST_HOME", value),
            None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
        }
    }

    #[test]
    #[serial]
    fn byok_cards_are_added_launched_and_removed_through_providers_json() {
        with_copilot_home(|state, home| {
            let mut card = Provider::with_id("relay".into(), "Relay".into(), byok_settings(), None);
            ProviderService::add(state, AppType::Copilot, card.clone(), false).unwrap();
            let listed = ProviderService::list(state, AppType::Copilot).unwrap();
            assert_eq!(
                listed["relay"].meta.as_ref().unwrap().live_config_managed,
                Some(false)
            );
            // 没添加时不能打开终端
            let error = terminal_launch(&listed["relay"]).unwrap_err();
            assert!(matches!(
                error,
                AppError::Localized {
                    key: "copilot.launch.not_added",
                    ..
                }
            ));

            ProviderService::add_to_live_config(state, AppType::Copilot, "relay").unwrap();
            let file = std::fs::read_to_string(home.join("providers.json")).unwrap();
            assert!(file.contains("\"name\": \"relay\""), "{file}");
            assert!(!file.contains("reasoningEffort"), "{file}");
            let launch = terminal_launch(&card).unwrap();
            assert_eq!(launch.model.as_deref(), Some("relay/gpt-5.4"));
            assert_eq!(launch.reasoning_effort.as_deref(), Some("high"));
            // switch 对自带模型卡等同于添加，不报错
            ProviderService::switch(state, AppType::Copilot, "relay").unwrap();

            // Copilot 里选了它当默认模型：移除和删除都拒绝
            std::fs::write(home.join("settings.json"), r#"{"model":"relay/gpt-5.4"}"#).unwrap();
            assert_eq!(
                ProviderService::current(state, AppType::Copilot).unwrap(),
                "relay"
            );
            assert!(
                ProviderService::remove_from_live_config(state, AppType::Copilot, "relay").is_err()
            );
            assert!(ProviderService::delete(state, AppType::Copilot, "relay").is_err());

            // 编辑写回文件，但保留卡片自己的推理强度
            std::fs::write(home.join("settings.json"), r#"{"model":"gpt-5-mini"}"#).unwrap();
            card.settings_config["models"] = json!([{"id":"gpt-5.4"},{"id":"gpt-5.5"}]);
            card.settings_config["reasoningEffort"] = json!("low");
            ProviderService::update(state, AppType::Copilot, None, card.clone()).unwrap();
            let native = copilot_config::get_providers().unwrap();
            assert_eq!(native["relay"]["models"].as_array().unwrap().len(), 2);
            let listed = ProviderService::list(state, AppType::Copilot).unwrap();
            assert_eq!(listed["relay"].settings_config["reasoningEffort"], "low");
            assert_eq!(
                listed["relay"].meta.as_ref().unwrap().live_config_managed,
                Some(true)
            );

            ProviderService::remove_from_live_config(state, AppType::Copilot, "relay").unwrap();
            assert!(!copilot_config::provider_key_exists("relay").unwrap());
            let listed = ProviderService::list(state, AppType::Copilot).unwrap();
            assert_eq!(
                listed["relay"].meta.as_ref().unwrap().live_config_managed,
                Some(false)
            );
            ProviderService::delete(state, AppType::Copilot, "relay").unwrap();
            assert!(ProviderService::list(state, AppType::Copilot)
                .unwrap()
                .get("relay")
                .is_none());
        });
    }

    #[test]
    #[serial]
    fn broken_native_file_keeps_saved_cards_and_recovers_after_repair() {
        with_copilot_home(|state, home| {
            for (id, settings, live) in [
                ("login", json!({"env":{}}), false),
                ("draft", byok_settings(), false),
                ("relay", byok_settings(), true),
            ] {
                ProviderService::add(
                    state,
                    AppType::Copilot,
                    Provider::with_id(id.into(), id.into(), settings, None),
                    live,
                )
                .unwrap();
            }
            let path = home.join("providers.json");
            let original = std::fs::read(&path).unwrap();
            std::fs::write(&path, b"{").unwrap();
            let cards = ProviderService::list(state, AppType::Copilot).unwrap();
            assert!(is_official(&cards["login"]));
            for (id, live) in [("draft", false), ("relay", true)] {
                assert_eq!(
                    cards[id].meta.as_ref().unwrap().live_config_managed,
                    Some(live)
                );
            }
            assert!(ProviderService::add_to_live_config(state, AppType::Copilot, "draft").is_err());
            assert_eq!(std::fs::read(&path).unwrap(), b"{");
            std::fs::write(&path, original).unwrap();
            ProviderService::add_to_live_config(state, AppType::Copilot, "draft").unwrap();
            assert!(copilot_config::provider_key_exists("draft").unwrap());
        });
    }

    #[test]
    #[serial]
    fn list_merges_native_entries_and_converts_legacy_rows() {
        with_copilot_home(|state, home| {
            std::fs::create_dir_all(home).unwrap();
            std::fs::write(
                home.join("providers.json"),
                r#"{"providers":[{"name":"foundry","baseUrl":"http://localhost:5272/v1"},{"name":"mine","baseUrl":"https://new.test/v1"}],"models":[{"id":"phi","provider":"foundry"},{"id":"m","provider":"mine"}]}"#,
            )
            .unwrap();
            let mut mine = Provider::with_id(
                "mine".into(),
                "Mine".into(),
                json!({"provider":{"baseUrl":"https://old.test/v1"},"models":[{"id":"old"}],"reasoningEffort":"max"}),
                None,
            );
            state.db.save_provider("copilot", &mine).unwrap();
            let legacy = Provider::with_id(
                "legacy".into(),
                "Legacy".into(),
                json!({"env":{"COPILOT_PROVIDER_BASE_URL":"https://legacy.test/v1","COPILOT_PROVIDER_API_KEY":"k","COPILOT_MODEL":"lm"}}),
                None,
            );
            state.db.save_provider("copilot", &legacy).unwrap();
            let mut official = Provider::with_id(
                "foundry".into(),
                "GitHub Copilot".into(),
                json!({"env":{"COPILOT_MODEL":"auto"}}),
                None,
            );
            official.category = Some("official".into());
            state.db.save_provider("copilot", &official).unwrap();

            let listed = ProviderService::list(state, AppType::Copilot).unwrap();
            // 官方卡不参与合并，也永远不是「已添加」
            assert_eq!(
                listed["foundry"].settings_config,
                json!({"env":{"COPILOT_MODEL":"auto"}})
            );
            assert_eq!(
                listed["foundry"].meta.as_ref().unwrap().live_config_managed,
                Some(false)
            );
            mine = listed["mine"].clone();
            assert_eq!(
                mine.settings_config["provider"]["baseUrl"],
                "https://new.test/v1"
            );
            assert_eq!(mine.settings_config["models"], json!([{"id":"m"}]));
            assert_eq!(mine.settings_config["reasoningEffort"], "max");
            assert_eq!(mine.name, "Mine");
            let legacy = &listed["legacy"];
            assert_eq!(
                legacy.settings_config,
                json!({"provider":{"baseUrl":"https://legacy.test/v1","apiKey":"k"},"models":[{"id":"lm"}]})
            );
            assert_eq!(
                legacy.meta.as_ref().and_then(|m| m.live_config_managed),
                Some(false)
            );
            // 转换结果存回库里
            let saved = state
                .db
                .get_provider_by_id("legacy", "copilot")
                .unwrap()
                .unwrap();
            assert_eq!(saved.settings_config, legacy.settings_config);

            // 官方卡拒绝添加 / 切换
            assert!(
                ProviderService::add_to_live_config(state, AppType::Copilot, "foundry").is_err()
            );
            assert!(ProviderService::switch(state, AppType::Copilot, "foundry").is_err());
        });
    }

    #[test]
    #[serial]
    fn native_only_entries_show_up_as_cards() {
        with_copilot_home(|state, home| {
            std::fs::create_dir_all(home).unwrap();
            std::fs::write(
                home.join("providers.json"),
                r#"{"providers":[{"name":"local","baseUrl":"http://localhost:1/v1"}],"models":[{"id":"x","provider":"local"}]}"#,
            )
            .unwrap();
            let listed = ProviderService::list(state, AppType::Copilot).unwrap();
            let local = &listed["local"];
            assert_eq!(local.name, "local");
            assert_eq!(
                local.settings_config,
                json!({"provider":{"baseUrl":"http://localhost:1/v1"},"models":[{"id":"x"}]})
            );
            assert_eq!(local.meta.as_ref().unwrap().live_config_managed, Some(true));
        });
    }

    #[test]
    #[serial]
    fn startup_still_imports_the_official_login_card() {
        with_copilot_home(|state, home| {
            use crate::services::provider::{
                import_default_config, should_import_default_config_on_startup,
            };
            std::fs::create_dir_all(home).unwrap();
            // 选的是 providers.json 里的自带模型：不是官方模型，不导入。
            std::fs::write(
                home.join("providers.json"),
                r#"{"providers":[{"name":"relay","baseUrl":"https://relay.test/v1"}],"models":[{"id":"m","provider":"relay"}]}"#,
            )
            .unwrap();
            std::fs::write(home.join("settings.json"), r#"{"model":"relay/m"}"#).unwrap();
            assert!(should_import_default_config_on_startup(state, &AppType::Copilot).unwrap());
            assert!(!import_default_config(state, AppType::Copilot).unwrap());

            std::fs::write(home.join("settings.json"), r#"{"model":"gpt-5-mini"}"#).unwrap();
            assert!(import_default_config(state, AppType::Copilot).unwrap());
            let row = state
                .db
                .get_provider_by_id("default", "copilot")
                .unwrap()
                .unwrap();
            assert!(is_official(&row));
            assert_eq!(
                row.settings_config,
                json!({"env":{"COPILOT_MODEL":"gpt-5-mini"}})
            );
            // 累加式没有直连指针，「默认」看 Copilot 自己的设置。
            assert_eq!(
                crate::settings::get_current_provider(&AppType::Copilot),
                None
            );
            assert!(!should_import_default_config_on_startup(state, &AppType::Copilot).unwrap());
        });
    }
}
