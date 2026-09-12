//! Read-only Pi provider membership and global default reference.

use crate::error::AppError;
use crate::pi_config::{
    read_pi_auth_provider_ids, read_pi_native_defaults, read_pi_native_providers,
    PI_LOGIN_PROVIDER_TYPE,
};
use crate::store::AppState;
use serde::Serialize;

const PI_APP: &str = "pi";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PiCurrentState {
    pub enabled_provider_ids: Vec<String>,
    pub default_provider_id: Option<String>,
    pub login_provider_ids: Vec<String>,
}

pub(crate) struct PiStateService;

impl PiStateService {
    pub(crate) fn current(state: &AppState) -> Result<PiCurrentState, AppError> {
        let _guard = futures::executor::block_on(state.proxy_service.lock_switch_for_app(PI_APP));
        let native = read_pi_native_providers()?;
        let enabled_provider_ids = native.keys().cloned().collect::<Vec<_>>();
        let default_provider_id = match read_pi_native_defaults() {
            Ok(defaults) => defaults.default_provider,
            Err(error) => {
                log::warn!("Failed to read Pi global default provider for advisory UI: {error}");
                None
            }
        };
        // Pi owns login credentials in auth.json. Only providers actually
        // configured through `/login` are surfaced here, and explicit
        // models.json nodes win so the same provider never appears twice.
        let saved = state.db.get_all_providers(PI_APP)?;
        let auth_provider_ids = match read_pi_auth_provider_ids() {
            Ok(provider_ids) => provider_ids,
            Err(error) => {
                log::warn!("Failed to read Pi auth provider ids for advisory UI: {error}");
                // auth.json 暂时不可读时，沿用已同步的 /login 来源标记，避免前端
                // 把只读卡片误判为可编辑。真实文件恢复后会重新以 auth.json 为准。
                saved
                    .iter()
                    .filter_map(|(id, provider)| {
                        let meta = provider.meta.as_ref();
                        (meta.and_then(|value| value.provider_type.as_deref())
                            == Some(PI_LOGIN_PROVIDER_TYPE)
                            || meta.and_then(|value| value.pi_login_origin) == Some(true))
                        .then(|| id.clone())
                    })
                    .collect()
            }
        };
        let login_provider_ids = auth_provider_ids
            .into_iter()
            .filter(|id| {
                if native.contains_key(id) {
                    return false;
                }
                // 用户自己保存在 CC Switch、但尚未写入 models.json 的同 ID
                // Provider 仍可正常启用；只有登录投影及其显式覆盖历史恢复只读。
                saved.get(id).is_none_or(|provider| {
                    let meta = provider.meta.as_ref();
                    meta.and_then(|value| value.provider_type.as_deref())
                        == Some(PI_LOGIN_PROVIDER_TYPE)
                        || meta.and_then(|value| value.pi_login_origin) == Some(true)
                })
            })
            .collect();
        Ok(PiCurrentState {
            enabled_provider_ids,
            default_provider_id,
            login_provider_ids,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;
    use crate::pi_config::test_support::TestAgentDir;
    use crate::provider::{Provider, ProviderMeta};
    use serde_json::json;
    use serial_test::serial;
    use std::fs;
    use std::sync::Arc;

    #[test]
    #[serial]
    fn state_exposes_every_explicit_provider_node() {
        let _agent = TestAgentDir::new();
        let state = AppState::new(Arc::new(
            Database::memory().expect("create in-memory database"),
        ));
        let models_path = crate::pi_config::get_pi_models_path().expect("models path");
        fs::create_dir_all(models_path.parent().expect("models directory"))
            .expect("create models directory");
        fs::write(
            models_path,
            r#"{
                "providers": {
                    "cc-switch-managed": {
                        "name": "Managed",
                        "baseUrl": "https://api.example.com/v1",
                        "api": "openai-completions",
                        "models": [{ "id": "model-a" }]
                    },
                    "native-oauth": {
                        "oauth": "example",
                        "baseUrl": "https://api.example.com/v1",
                        "api": "openai-completions",
                        "models": [{ "id": "model-b" }]
                    },
                    "anthropic": {},
                    "unsupported": {
                        "futureField": true
                    }
                }
            }"#,
        )
        .expect("write models");
        let settings_path = crate::pi_config::get_pi_settings_path().expect("settings path");
        fs::write(
            settings_path,
            r#"{"defaultProvider":"cc-switch-managed","defaultModel":"model-a"}"#,
        )
        .expect("write settings");

        let current = PiStateService::current(&state).expect("read state");
        assert_eq!(
            current.enabled_provider_ids,
            vec![
                "cc-switch-managed".to_string(),
                "native-oauth".to_string(),
                "anthropic".to_string(),
                "unsupported".to_string(),
            ]
        );
        assert_eq!(
            current.default_provider_id.as_deref(),
            Some("cc-switch-managed")
        );
    }

    #[test]
    #[serial]
    fn login_providers_include_only_auth_entries_and_deduplicate_models_json() {
        let _agent = TestAgentDir::new();
        let state = AppState::new(Arc::new(
            Database::memory().expect("create in-memory database"),
        ));
        let agent_dir = crate::pi_config::get_pi_agent_dir().expect("agent directory");
        fs::create_dir_all(&agent_dir).expect("create agent directory");
        fs::write(
            agent_dir.join("models.json"),
            br#"{"providers":{"deepseek":{"name":"DeepSeek override"},"5090":{"name":"5090"}}}"#,
        )
        .expect("write models");
        fs::write(
            agent_dir.join("auth.json"),
            br#"{
                "deepseek": {"type": "api_key", "key": "sk-native"},
                "kimi-coding": {"type": "api_key", "key": "sk-kimi-native"},
                "future-login-provider": {"type": "api_key", "key": "sk-future"}
            }"#,
        )
        .expect("write auth");
        fs::write(
            agent_dir.join("settings.json"),
            br#"{"defaultProvider":"deepseek","defaultModel":"deepseek-v4-flash"}"#,
        )
        .expect("write settings");
        state
            .db
            .save_provider(
                PI_APP,
                &Provider::with_id(
                    "future-login-provider".to_string(),
                    "User-created provider".to_string(),
                    json!({"name": "User-created provider"}),
                    None,
                ),
            )
            .expect("save user-created DB provider");

        let current = PiStateService::current(&state).expect("read state");
        assert_eq!(
            current.enabled_provider_ids,
            vec!["deepseek".to_string(), "5090".to_string()]
        );
        assert_eq!(current.default_provider_id.as_deref(), Some("deepseek"));
        assert_eq!(current.login_provider_ids, vec!["kimi-coding".to_string()]);
    }

    #[test]
    #[serial]
    fn invalid_auth_falls_back_to_saved_login_provider_ids() {
        let _agent = TestAgentDir::new();
        let state = AppState::new(Arc::new(
            Database::memory().expect("create in-memory database"),
        ));
        let agent_dir = crate::pi_config::get_pi_agent_dir().expect("agent directory");
        fs::create_dir_all(&agent_dir).expect("create agent directory");
        fs::write(agent_dir.join("auth.json"), "{").expect("write invalid auth");

        let mut provider = Provider::with_id(
            "deepseek".to_string(),
            "DeepSeek".to_string(),
            json!({"name": "DeepSeek", "source": "pi-login"}),
            None,
        );
        provider.meta = Some(ProviderMeta {
            provider_type: Some(PI_LOGIN_PROVIDER_TYPE.to_string()),
            ..ProviderMeta::default()
        });
        state
            .db
            .save_provider(PI_APP, &provider)
            .expect("save login provider");

        let mut overridden = Provider::with_id(
            "kimi-coding".to_string(),
            "Kimi explicit override".to_string(),
            json!({"name": "Kimi explicit override"}),
            None,
        );
        overridden.meta = Some(ProviderMeta {
            pi_login_origin: Some(true),
            ..ProviderMeta::default()
        });
        state
            .db
            .save_provider(PI_APP, &overridden)
            .expect("save provider with login origin");

        let current = PiStateService::current(&state).expect("read state with fallback");
        assert_eq!(
            current.login_provider_ids,
            vec!["deepseek".to_string(), "kimi-coding".to_string()]
        );
    }

    #[test]
    #[serial]
    fn invalid_global_settings_do_not_hide_provider_membership() {
        let _agent = TestAgentDir::new();
        let state = AppState::new(Arc::new(
            Database::memory().expect("create in-memory database"),
        ));
        let models_path = crate::pi_config::get_pi_models_path().expect("models path");
        fs::create_dir_all(models_path.parent().expect("models directory"))
            .expect("create models directory");
        fs::write(
            models_path,
            r#"{
                "providers": {
                    "cc-switch-managed": {
                        "name": "Managed",
                        "baseUrl": "https://api.example.com/v1",
                        "api": "openai-completions",
                        "models": [{ "id": "model-a" }]
                    }
                }
            }"#,
        )
        .expect("write models");
        let settings_path = crate::pi_config::get_pi_settings_path().expect("settings path");
        fs::write(settings_path, "[]").expect("write invalid settings");

        let current = PiStateService::current(&state).expect("read membership");
        assert_eq!(
            current.enabled_provider_ids,
            vec!["cc-switch-managed".to_string()]
        );
        assert_eq!(current.default_provider_id, None);
    }
}
