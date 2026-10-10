//! Provider service module
//!
//! Handles provider CRUD operations, switching, and configuration management.

pub(crate) mod claude_direct;
mod claude_editor;
pub(crate) mod codex_client_catalog;
pub(crate) mod codex_direct;
mod codex_editor;
mod codex_login;
pub(crate) mod codex_official_models;
mod editor_toml;
mod endpoints;
mod gemini_auth;
pub(crate) mod gemini_direct;
mod gemini_editor;
pub(crate) mod grok_direct;
mod grok_editor;
mod live;
#[cfg(test)]
mod opencode_tests;
mod pi;
mod usage;

use indexmap::IndexMap;
use regex::Regex;
use serde::Deserialize;
use serde_json::Value;

use crate::app_config::AppType;
use crate::error::AppError;
use crate::provider::{Provider, ProviderMeta, UsageResult};
use crate::services::mcp::McpService;
use crate::settings::CustomEndpoint;
use crate::store::AppState;

// Re-export sub-module functions for external access
pub use live::{
    import_default_config, import_hermes_providers_from_live, import_openclaw_providers_from_live,
    import_opencode_providers_from_live, read_live_settings,
    should_import_default_config_on_startup, sync_current_to_live,
};

pub fn import_pi_providers_from_live(state: &AppState) -> Result<usize, AppError> {
    pi::import_from_live(state)
}

pub use claude_editor::{EditorSave, EditorView};

// Internal re-exports (pub(crate))
pub(crate) use live::{
    provider_exists_in_live_config, sync_additive_app_to_live, write_live_for_state,
    LiveSyncOutcome,
};

// Internal re-exports
use live::{
    remove_hermes_provider_from_live, remove_openclaw_provider_from_live,
    remove_opencode_provider_from_live,
};
use usage::validate_usage_script;

/// Codex official providers are safe to select during takeover: Codex keeps
/// ownership of the active ChatGPT login and the proxy only forwards the
/// authenticated request. Other apps' official providers retain the block.
pub fn official_provider_supports_proxy_takeover(app_type: &AppType, provider: &Provider) -> bool {
    matches!(app_type, AppType::Codex)
        && crate::proxy::providers::is_codex_official_provider(provider)
}

/// 统一会话开关变更后，立即按新开关状态重写当前官方 Codex 供应商的选路（关键字段），
/// 使开关即时生效，无需等下一次切换。当前供应商非官方（或不存在）时为 no-op：开关只
/// 影响官方直连的选路。代理模式下 live 是代理契约，不受这个开关影响。
pub fn reapply_current_codex_official_live(state: &AppState) -> Result<bool, AppError> {
    let _switch_guard = crate::mode::controller::lock_settled_blocking(state, &AppType::Codex)?;
    let current_id = ProviderService::current(state, AppType::Codex)?;
    if current_id.is_empty() {
        return Ok(false);
    }
    let providers = state.db.get_all_providers(AppType::Codex.as_str())?;
    let Some(provider) = providers.get(&current_id) else {
        return Ok(false);
    };
    if !codex_direct::is_official(provider) {
        return Ok(false);
    }
    live::sync_live_for_provider_respecting_mode(state, &AppType::Codex, provider, None)?;
    Ok(true)
}

/// 新版不再读通用配置片段，但旧设备经云同步拿到新建的行时仍按这个标记合并片段；不写的
/// 话，旧版切到它会把 hooks、MCP 等共享设置整份抹掉。
fn keep_common_config_for_old_versions(provider: &mut Provider) {
    provider
        .meta
        .get_or_insert_with(Default::default)
        .common_config_enabled = Some(true);
}

/// 编辑器保存的是新增的供应商，还是已有的。
#[derive(Clone, Copy, PartialEq, Eq)]
enum EditorSaveKind {
    Add,
    Update,
}

impl EditorSaveKind {
    /// 这次保存要不要把关键字段也换进 live：直连模式下编辑的是当前供应商，或者新增的是
    /// 第一个供应商。代理模式下 live 的关键字段是代理契约，只写全局改动。
    fn writes_key_fields(
        self,
        state: &AppState,
        app_type: &AppType,
        mode: &crate::mode::state::ModeState,
        id: &str,
    ) -> Result<bool, AppError> {
        if mode.is_proxy() {
            return Ok(false);
        }
        let direct = crate::mode::current::provider_for(
            &state.db,
            app_type,
            crate::mode::current::Purpose::Direct,
        )?;
        Ok(match self {
            Self::Add => direct.is_none(),
            Self::Update => direct.as_deref() == Some(id),
        })
    }
}

/// Provider business logic service
pub struct ProviderService;

/// Result of a provider switch operation, including any non-fatal warnings
#[derive(Debug, serde::Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SwitchResult {
    pub warnings: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(any(target_os = "macos", windows, target_os = "linux"))]
    use crate::claude_desktop_config::PROFILE_ID;
    use crate::config::{get_claude_settings_path, read_json_file, write_json_file};
    use crate::database::Database;
    use crate::provider::{
        AuthBinding, AuthBindingSource, ClaudeModelConfig, ProviderMeta, UniversalProvider,
        UsageScript,
    };
    #[cfg(any(target_os = "macos", windows, target_os = "linux"))]
    use crate::provider::{ClaudeDesktopMode, ClaudeDesktopModelRoute};
    use crate::proxy::types::ProxyConfig;
    use crate::store::AppState;
    use serde_json::json;
    use serial_test::serial;
    use std::env;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Mutex, OnceLock};
    use tempfile::TempDir;

    struct TempHome {
        #[allow(dead_code)]
        dir: TempDir,
        original_home: Option<String>,
        #[cfg(windows)]
        original_local_app_data: Option<String>,
        original_userprofile: Option<String>,
        original_test_home: Option<String>,
        #[cfg(target_os = "linux")]
        original_xdg_config_home: Option<std::ffi::OsString>,
    }

    impl TempHome {
        fn new() -> Self {
            let dir = TempDir::new().expect("failed to create temp home");
            let original_home = env::var("HOME").ok();
            #[cfg(windows)]
            let original_local_app_data = env::var("LOCALAPPDATA").ok();
            let original_userprofile = env::var("USERPROFILE").ok();
            let original_test_home = env::var("CC_SWITCH_TEST_HOME").ok();
            #[cfg(target_os = "linux")]
            let original_xdg_config_home = env::var_os("XDG_CONFIG_HOME");

            env::set_var("HOME", dir.path());
            #[cfg(windows)]
            env::set_var("LOCALAPPDATA", dir.path().join("AppData").join("Local"));
            env::set_var("USERPROFILE", dir.path());
            env::set_var("CC_SWITCH_TEST_HOME", dir.path());
            // Claude Desktop Linux paths follow XDG_CONFIG_HOME; pin them under the temp home.
            #[cfg(target_os = "linux")]
            env::remove_var("XDG_CONFIG_HOME");

            Self {
                dir,
                original_home,
                #[cfg(windows)]
                original_local_app_data,
                original_userprofile,
                original_test_home,
                #[cfg(target_os = "linux")]
                original_xdg_config_home,
            }
        }
    }

    impl Drop for TempHome {
        fn drop(&mut self) {
            match &self.original_home {
                Some(value) => env::set_var("HOME", value),
                None => env::remove_var("HOME"),
            }

            #[cfg(windows)]
            {
                match &self.original_local_app_data {
                    Some(value) => env::set_var("LOCALAPPDATA", value),
                    None => env::remove_var("LOCALAPPDATA"),
                }
            }

            match &self.original_userprofile {
                Some(value) => env::set_var("USERPROFILE", value),
                None => env::remove_var("USERPROFILE"),
            }

            match &self.original_test_home {
                Some(value) => env::set_var("CC_SWITCH_TEST_HOME", value),
                None => env::remove_var("CC_SWITCH_TEST_HOME"),
            }

            #[cfg(target_os = "linux")]
            {
                match &self.original_xdg_config_home {
                    Some(value) => env::set_var("XDG_CONFIG_HOME", value),
                    None => env::remove_var("XDG_CONFIG_HOME"),
                }
            }
        }
    }

    #[cfg(windows)]
    fn claude_desktop_profile_path(home: &Path) -> PathBuf {
        home.join("AppData")
            .join("Local")
            .join("Claude-3p")
            .join("configLibrary")
            .join(format!("{PROFILE_ID}.json"))
    }

    #[cfg(target_os = "macos")]
    fn claude_desktop_profile_path(home: &Path) -> PathBuf {
        home.join("Library")
            .join("Application Support")
            .join("Claude-3p")
            .join("configLibrary")
            .join(format!("{PROFILE_ID}.json"))
    }

    #[cfg(target_os = "linux")]
    fn claude_desktop_profile_path(home: &Path) -> PathBuf {
        home.join(".config")
            .join("Claude-3p")
            .join("configLibrary")
            .join(format!("{PROFILE_ID}.json"))
    }

    fn test_guard() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|err| err.into_inner())
    }

    fn with_test_home<T>(test: impl FnOnce(&AppState, &Path) -> T) -> T {
        let _guard = test_guard();
        let temp = tempfile::tempdir().expect("tempdir");
        let old_test_home = std::env::var_os("CC_SWITCH_TEST_HOME");
        let old_home = std::env::var_os("HOME");
        std::env::set_var("CC_SWITCH_TEST_HOME", temp.path());
        std::env::set_var("HOME", temp.path());

        let db = Arc::new(Database::memory().expect("in-memory database"));
        let state = AppState::new(db);
        let result = test(&state, temp.path());

        match old_test_home {
            Some(value) => std::env::set_var("CC_SWITCH_TEST_HOME", value),
            None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
        }
        match old_home {
            Some(value) => std::env::set_var("HOME", value),
            None => std::env::remove_var("HOME"),
        }

        result
    }

    fn codex_settings(base_url: &str, api_key: &str) -> Value {
        json!({
            "auth": {
                "OPENAI_API_KEY": api_key
            },
            "config": format!(
                "model_provider = \"custom\"\n\
                 [model_providers.custom]\n\
                 name = \"custom\"\n\
                 base_url = \"{base_url}\"\n\
                 wire_api = \"chat\"\n"
            )
        })
    }

    #[test]
    #[serial]
    fn codex_editor_view_preserves_draft_and_stored_copilot_metadata() {
        with_test_home(|state, _home| {
            crate::settings::reload_settings().expect("reload settings");
            let settings = json!({
                "auth": {},
                "apiFormat": "openai_chat",
                "config": "model = \"claude-sonnet-5\"\nmodel_provider = \"copilot\"\nmodel_context_window = 400000\n\n[model_providers.copilot]\nbase_url = \"https://api.githubcopilot.com\"\nwire_api = \"responses\"\nrequires_openai_auth = true\n"
            });
            let meta = ProviderMeta {
                provider_type: Some("github_copilot".to_string()),
                api_format: Some("openai_chat".to_string()),
                auth_binding: Some(AuthBinding {
                    source: AuthBindingSource::ManagedAccount,
                    auth_provider: Some("github_copilot".to_string()),
                    account_id: Some("copilot-editor-account".to_string()),
                }),
                ..Default::default()
            };
            let mut stored = Provider::with_id(
                "copilot-editor".to_string(),
                "Copilot".to_string(),
                settings.clone(),
                None,
            );
            stored.meta = Some(meta.clone());
            state
                .db
                .save_provider(AppType::Codex.as_str(), &stored)
                .expect("store Copilot provider");

            for (provider_id, draft_meta) in [(None, Some(&meta)), (Some(stored.id.as_str()), None)]
            {
                let view = ProviderService::editor_view_with_meta(
                    state,
                    AppType::Codex,
                    &settings,
                    None,
                    provider_id,
                    draft_meta,
                )
                .expect("keyless managed Copilot preview");
                let text = view.settings["config"].as_str().expect("config text");
                let parsed: toml::Value = toml::from_str(text).expect("preview TOML");
                let route = &parsed["model_providers"]["custom"];
                assert_eq!(parsed["model_provider"].as_str(), Some("custom"));
                assert_eq!(route["wire_api"].as_str(), Some("responses"));
                assert!(route.get("requires_openai_auth").is_none());
                assert!(route.get("experimental_bearer_token").is_none());
                assert_eq!(view.settings["auth"], json!({}));
                assert!(!text.contains("copilot-editor-account"));
                assert_eq!(parsed["model_context_window"].as_integer(), Some(400000));
                assert_eq!(
                    parsed["web_search"].as_str(),
                    Some("disabled"),
                    "Copilot must not inherit the generic ProxyChat search policy"
                );
            }

            let cleared_meta = ProviderMeta::default();
            let cleared = ProviderService::editor_view_with_meta(
                state,
                AppType::Codex,
                &settings,
                None,
                Some(&stored.id),
                Some(&cleared_meta),
            )
            .expect("generic keyless preview with explicitly cleared metadata");
            let generic = ProviderService::editor_view(state, AppType::Codex, &settings, None)
                .expect("generic keyless preview without managed metadata");
            // 普通空 Key 草稿也允许预览，不能再用报错区分身份；检查 Copilot 专属策略。
            for (view, reason) in [
                (
                    cleared,
                    "explicit draft metadata must not fall back to the stored Copilot type",
                ),
                (
                    generic,
                    "the compatibility wrapper must not infer managed auth from the URL",
                ),
            ] {
                let text = view.settings["config"].as_str().expect("config text");
                let parsed: toml::Value = toml::from_str(text).expect("preview TOML");
                assert!(parsed.get("web_search").is_none(), "{reason}");
                assert_eq!(view.settings["auth"], json!({}));
                assert!(!text.contains("cc-switch-editor-pending-key"));
            }
            assert!(!crate::codex_config::get_codex_config_path().exists());
            assert!(!crate::codex_config::get_codex_model_catalog_path().exists());
        });
    }

    #[test]
    #[serial]
    fn codex_editor_metadata_does_not_change_other_app_previews() {
        with_test_home(|state, _home| {
            crate::settings::reload_settings().expect("reload settings");
            let settings = json!({"env": {
                "ANTHROPIC_BASE_URL": "https://relay.example",
                "ANTHROPIC_AUTH_TOKEN": "editor-key",
                "ANTHROPIC_MODEL": "claude-sonnet-5"
            }});
            let expected =
                ProviderService::editor_view(state, AppType::Claude, &settings, None).unwrap();
            let meta = ProviderMeta {
                provider_type: Some("github_copilot".to_string()),
                ..Default::default()
            };
            let actual = ProviderService::editor_view_with_meta(
                state,
                AppType::Claude,
                &settings,
                None,
                Some("absent-provider"),
                Some(&meta),
            )
            .unwrap();
            assert_eq!(
                serde_json::to_value(actual).unwrap(),
                serde_json::to_value(expected).unwrap()
            );
        });
    }

    fn usage_script_with_credentials(
        api_key: Option<&str>,
        base_url: Option<&str>,
        template_type: Option<&str>,
    ) -> UsageScript {
        UsageScript {
            enabled: true,
            language: "javascript".to_string(),
            code: "return { remaining: 1, unit: 'USD' };".to_string(),
            timeout: Some(10),
            api_key: api_key.map(str::to_string),
            base_url: base_url.map(str::to_string),
            access_token: None,
            user_id: None,
            template_type: template_type.map(str::to_string),
            auto_query_interval: None,
            coding_plan_provider: None,
            access_key_id: Some("ak-test".to_string()),
            secret_access_key: Some("sk-test".to_string()),
            team_organization_id: None,
            team_project_id: None,
        }
    }

    fn codex_provider_with_usage(
        id: &str,
        base_url: &str,
        api_key: &str,
        usage_api_key: Option<&str>,
        usage_base_url: Option<&str>,
        template_type: Option<&str>,
    ) -> Provider {
        let mut provider = Provider::with_id(
            id.to_string(),
            format!("Provider {id}"),
            codex_settings(base_url, api_key),
            None,
        );
        provider.meta = Some(ProviderMeta {
            usage_script: Some(usage_script_with_credentials(
                usage_api_key,
                usage_base_url,
                template_type,
            )),
            ..Default::default()
        });
        provider
    }

    fn managed_codex_provider(id: &str, account_id: &str) -> Provider {
        let mut provider = Provider::with_id(
            id.to_string(),
            format!("Managed {id}"),
            json!({
                "auth": {},
                "config": ""
            }),
            None,
        );
        provider.category = Some("official".to_string());
        provider.meta = Some(ProviderMeta {
            auth_binding: Some(AuthBinding {
                source: AuthBindingSource::ManagedAccount,
                auth_provider: Some("codex_oauth".to_string()),
                account_id: Some(account_id.to_string()),
            }),
            ..Default::default()
        });
        provider
    }

    fn openclaw_provider(id: &str) -> Provider {
        Provider {
            id: id.to_string(),
            name: format!("Provider {id}"),
            settings_config: json!({
                "baseUrl": "https://api.deepseek.com",
                "apiKey": "test-key",
                "api": "openai-completions",
                "models": [],
            }),
            website_url: None,
            category: Some("custom".to_string()),
            created_at: Some(1),
            sort_index: Some(0),
            notes: None,
            meta: None,
            icon: None,
            icon_color: None,
            in_failover_queue: false,
        }
    }

    fn hermes_provider(id: &str) -> Provider {
        Provider {
            id: id.to_string(),
            name: format!("Provider {id}"),
            settings_config: json!({
                "api": "openai-chat",
                "base_url": "https://api.example.com/v1",
                "api_key": "test-key",
                "models": {
                    "gpt-4o": {
                        "name": "GPT-4o"
                    }
                }
            }),
            website_url: None,
            category: Some("custom".to_string()),
            created_at: Some(1),
            sort_index: Some(0),
            notes: None,
            meta: None,
            icon: None,
            icon_color: None,
            in_failover_queue: false,
        }
    }

    fn opencode_provider(id: &str) -> Provider {
        Provider {
            id: id.to_string(),
            name: format!("Provider {id}"),
            settings_config: json!({
                "npm": "@ai-sdk/openai-compatible",
                "name": format!("Provider {id}"),
                "options": {
                    "baseURL": "https://api.example.com/v1",
                    "apiKey": "test-key"
                },
                "models": {
                    "gpt-4o": {
                        "name": "GPT-4o"
                    }
                }
            }),
            website_url: None,
            category: Some("custom".to_string()),
            created_at: Some(1),
            sort_index: Some(0),
            notes: None,
            meta: None,
            icon: None,
            icon_color: None,
            in_failover_queue: false,
        }
    }

    fn opencode_omo_provider(id: &str, category: &str) -> Provider {
        let mut settings = serde_json::Map::new();
        settings.insert(
            "agents".to_string(),
            json!({
                "writer": {
                    "model": "gpt-4o-mini"
                }
            }),
        );
        if category == "omo" {
            settings.insert(
                "categories".to_string(),
                json!({
                    "default": ["writer"]
                }),
            );
        }
        settings.insert(
            "otherFields".to_string(),
            json!({
                "theme": "dark"
            }),
        );

        Provider {
            id: id.to_string(),
            name: format!("Provider {id}"),
            settings_config: Value::Object(settings),
            website_url: None,
            category: Some(category.to_string()),
            created_at: Some(1),
            sort_index: Some(0),
            notes: None,
            meta: None,
            icon: None,
            icon_color: None,
            in_failover_queue: false,
        }
    }

    fn omo_config_path(home: &Path, category: &str) -> PathBuf {
        home.join(".config").join("opencode").join(match category {
            "omo" => crate::services::omo::STANDARD.preferred_filename,
            "omo-slim" => crate::services::omo::SLIM.preferred_filename,
            other => panic!("unexpected OMO category in test: {other}"),
        })
    }

    #[test]
    #[serial]
    fn add_clears_usage_credentials_that_match_provider_config() {
        with_test_home(|state, _| {
            let provider = codex_provider_with_usage(
                "codex-a",
                "https://api.a.example/v1/",
                "sk-a",
                Some(" sk-a "),
                Some(" https://api.a.example/v1/ "),
                None,
            );

            ProviderService::add(state, AppType::Codex, provider, false).expect("add provider");

            let saved = state
                .db
                .get_provider_by_id("codex-a", AppType::Codex.as_str())
                .expect("query saved provider")
                .expect("saved provider should exist");
            let script = saved
                .meta
                .as_ref()
                .and_then(|meta| meta.usage_script.as_ref())
                .expect("usage script should remain");

            assert_eq!(script.api_key, None);
            assert_eq!(script.base_url, None);
        });
    }

    #[test]
    #[serial]
    fn update_preserves_usage_credentials_that_only_match_previous_config() {
        with_test_home(|state, _| {
            let provider = codex_provider_with_usage(
                "codex-usage-old",
                "https://api.a.example/v1/",
                "sk-a",
                Some("sk-a"),
                Some("https://api.a.example/v1/"),
                None,
            );
            state
                .db
                .save_provider(AppType::Codex.as_str(), &provider)
                .expect("seed provider with explicit usage credentials");

            let mut updated = provider.clone();
            updated.settings_config = codex_settings("https://api.b.example/v1/", "sk-b");

            ProviderService::update(state, AppType::Codex, None, updated)
                .expect("update provider main credentials");

            let saved = state
                .db
                .get_provider_by_id("codex-usage-old", AppType::Codex.as_str())
                .expect("query updated provider")
                .expect("updated provider should exist");
            let script = saved
                .meta
                .as_ref()
                .and_then(|meta| meta.usage_script.as_ref())
                .expect("usage script should remain");

            assert_eq!(script.api_key.as_deref(), Some("sk-a"));
            assert_eq!(
                script.base_url.as_deref(),
                Some("https://api.a.example/v1/")
            );
            assert_eq!(
                saved.resolve_usage_credentials(&AppType::Codex),
                ("https://api.b.example/v1".to_string(), "sk-b".to_string())
            );
        });
    }

    #[test]
    #[serial]
    fn copied_provider_uses_edited_credentials_after_add_clears_mirrored_usage_credentials() {
        with_test_home(|state, _| {
            let copied_provider = codex_provider_with_usage(
                "codex-copy",
                "https://api.a.example/v1/",
                "sk-a",
                Some("sk-a"),
                Some("https://api.a.example/v1/"),
                None,
            );

            ProviderService::add(state, AppType::Codex, copied_provider, false)
                .expect("add copied provider");

            let saved_after_add = state
                .db
                .get_provider_by_id("codex-copy", AppType::Codex.as_str())
                .expect("query copied provider")
                .expect("copied provider should exist");
            let script_after_add = saved_after_add
                .meta
                .as_ref()
                .and_then(|meta| meta.usage_script.as_ref())
                .expect("usage script should remain");
            assert_eq!(script_after_add.api_key, None);
            assert_eq!(script_after_add.base_url, None);

            let mut edited_provider = saved_after_add.clone();
            edited_provider.settings_config = codex_settings("https://api.b.example/v1/", "sk-b");

            ProviderService::update(state, AppType::Codex, None, edited_provider)
                .expect("edit copied provider credentials");

            let saved_after_update = state
                .db
                .get_provider_by_id("codex-copy", AppType::Codex.as_str())
                .expect("query edited provider")
                .expect("edited provider should exist");
            let script_after_update = saved_after_update
                .meta
                .as_ref()
                .and_then(|meta| meta.usage_script.as_ref())
                .expect("usage script should remain");

            assert_eq!(script_after_update.api_key, None);
            assert_eq!(script_after_update.base_url, None);
            assert_eq!(
                saved_after_update.resolve_usage_credentials(&AppType::Codex),
                ("https://api.b.example/v1".to_string(), "sk-b".to_string())
            );
        });
    }

    #[test]
    #[serial]
    fn update_clears_usage_credentials_that_match_current_config() {
        with_test_home(|state, _| {
            let provider = codex_provider_with_usage(
                "codex-current",
                "https://api.a.example/v1",
                "sk-a",
                Some("sk-usage"),
                Some("https://usage.example/api"),
                None,
            );
            state
                .db
                .save_provider(AppType::Codex.as_str(), &provider)
                .expect("seed provider with distinct usage credentials");

            let mut updated = provider.clone();
            updated.settings_config = codex_settings("https://api.b.example/v1/", "sk-b");
            updated.meta = Some(ProviderMeta {
                usage_script: Some(usage_script_with_credentials(
                    Some(" sk-b "),
                    Some(" https://api.b.example/v1/ "),
                    None,
                )),
                ..Default::default()
            });

            ProviderService::update(state, AppType::Codex, None, updated)
                .expect("update provider with redundant usage credentials");

            let saved = state
                .db
                .get_provider_by_id("codex-current", AppType::Codex.as_str())
                .expect("query updated provider")
                .expect("updated provider should exist");
            let script = saved
                .meta
                .as_ref()
                .and_then(|meta| meta.usage_script.as_ref())
                .expect("usage script should remain");

            assert_eq!(script.api_key, None);
            assert_eq!(script.base_url, None);
        });
    }

    #[test]
    #[serial]
    fn add_preserves_distinct_usage_credentials() {
        with_test_home(|state, _| {
            let provider = codex_provider_with_usage(
                "codex-distinct",
                "https://api.main.example/v1",
                "sk-main",
                Some("sk-usage"),
                Some("https://usage.example/api"),
                None,
            );

            ProviderService::add(state, AppType::Codex, provider, false).expect("add provider");

            let saved = state
                .db
                .get_provider_by_id("codex-distinct", AppType::Codex.as_str())
                .expect("query saved provider")
                .expect("saved provider should exist");
            let script = saved
                .meta
                .as_ref()
                .and_then(|meta| meta.usage_script.as_ref())
                .expect("usage script should remain");

            assert_eq!(script.api_key.as_deref(), Some("sk-usage"));
            assert_eq!(
                script.base_url.as_deref(),
                Some("https://usage.example/api")
            );
        });
    }

    #[test]
    #[serial]
    fn add_does_not_clear_token_plan_credentials() {
        with_test_home(|state, _| {
            let provider = codex_provider_with_usage(
                "codex-token-plan",
                "https://api.plan.example/v1",
                "sk-plan",
                Some("sk-plan"),
                Some("https://api.plan.example/v1"),
                Some("token_plan"),
            );

            ProviderService::add(state, AppType::Codex, provider, false).expect("add provider");

            let saved = state
                .db
                .get_provider_by_id("codex-token-plan", AppType::Codex.as_str())
                .expect("query saved provider")
                .expect("saved provider should exist");
            let script = saved
                .meta
                .as_ref()
                .and_then(|meta| meta.usage_script.as_ref())
                .expect("usage script should remain");

            assert_eq!(script.api_key.as_deref(), Some("sk-plan"));
            assert_eq!(
                script.base_url.as_deref(),
                Some("https://api.plan.example/v1")
            );
            assert_eq!(script.access_key_id.as_deref(), Some("ak-test"));
            assert_eq!(script.secret_access_key.as_deref(), Some("sk-test"));
        });
    }

    #[test]
    fn validate_provider_settings_rejects_missing_auth() {
        let provider = Provider::with_id(
            "codex".into(),
            "Codex".into(),
            json!({ "config": "base_url = \"https://example.com\"" }),
            None,
        );
        let err = ProviderService::validate_provider_settings(&AppType::Codex, &provider)
            .expect_err("missing auth should be rejected");
        assert!(
            err.to_string().contains("auth"),
            "expected auth error, got {err:?}"
        );
    }

    #[test]
    #[serial]
    fn add_accepts_multiple_unbound_codex_official_cards() {
        with_test_home(|state, _| {
            crate::settings::reload_settings().expect("reload settings");
            state
                .db
                .init_default_official_providers()
                .expect("seed official providers");
            let fixed_id = crate::database::CODEX_OFFICIAL_PROVIDER_ID;
            state
                .db
                .set_current_provider(AppType::Codex.as_str(), fixed_id)
                .expect("set database current");
            crate::settings::set_current_provider(&AppType::Codex, Some(fixed_id))
                .expect("set local current");

            for id in ["follow-login-a", "follow-login-b"] {
                let mut provider = Provider::with_id(
                    id.to_string(),
                    id.to_string(),
                    json!({ "auth": {}, "config": "" }),
                    None,
                );
                provider.category = Some("official".to_string());
                ProviderService::add(state, AppType::Codex, provider, false)
                    .expect("add unbound Official card");
            }

            let providers = state
                .db
                .get_all_providers(AppType::Codex.as_str())
                .expect("read providers");
            assert!(providers.contains_key("follow-login-a"));
            assert!(providers.contains_key("follow-login-b"));
        });
    }

    #[test]
    #[serial]
    fn update_keeps_official_provider_id_when_binding_and_unbinding() {
        with_test_home(|state, _| {
            crate::settings::reload_settings().expect("reload settings");
            tauri::async_runtime::block_on(async {
                state
                    .codex_oauth_manager
                    .add_test_account_with_user_identity(
                        "acct-managed",
                        "managed-access-token",
                        "managed-user",
                    )
                    .await
                    .expect("seed managed account");
            });

            let provider_id = crate::database::CODEX_OFFICIAL_PROVIDER_ID;
            let mut unbound = Provider::with_id(
                provider_id.to_string(),
                "OpenAI Official".to_string(),
                json!({ "auth": {}, "config": "" }),
                None,
            );
            unbound.category = Some("official".to_string());
            state
                .db
                .save_provider(AppType::Codex.as_str(), &unbound)
                .expect("save unbound card");
            state
                .db
                .set_current_provider(AppType::Codex.as_str(), provider_id)
                .expect("set database current");
            crate::settings::set_current_provider(&AppType::Codex, Some(provider_id))
                .expect("set local current");

            let mut bound = managed_codex_provider(provider_id, "acct-managed");
            bound.name = unbound.name.clone();
            ProviderService::update(state, AppType::Codex, Some(provider_id), bound)
                .expect("bind managed account");

            let saved_bound = state
                .db
                .get_provider_by_id(provider_id, AppType::Codex.as_str())
                .expect("query bound card")
                .expect("bound card should keep its ID");
            assert_eq!(
                ProviderService::managed_codex_oauth_account_id(&saved_bound).as_deref(),
                Some("acct-managed")
            );
            assert_eq!(
                state
                    .db
                    .get_current_provider(AppType::Codex.as_str())
                    .expect("read database current")
                    .as_deref(),
                Some(provider_id)
            );
            assert_eq!(
                crate::settings::get_current_provider(&AppType::Codex).as_deref(),
                Some(provider_id)
            );

            // 旧版「统一会话历史」注入进 live、又被回填进行里的形态，保存时剥掉。
            unbound.settings_config["config"] = Value::String("model_provider = \"custom\"\n\n[model_providers.custom]\nname = \"OpenAI\"\nrequires_openai_auth = true\nsupports_websockets = true\nwire_api = \"responses\"\n".to_string());
            ProviderService::update(state, AppType::Codex, Some(provider_id), unbound)
                .expect("unbind managed account");

            let saved_unbound = state
                .db
                .get_provider_by_id(provider_id, AppType::Codex.as_str())
                .expect("query unbound card")
                .expect("unbound card should keep its ID");
            assert!(ProviderService::managed_codex_oauth_account_id(&saved_unbound).is_none());
            assert_eq!(saved_unbound.settings_config["config"], json!(""));
            assert_eq!(
                state
                    .db
                    .get_current_provider(AppType::Codex.as_str())
                    .expect("read database current")
                    .as_deref(),
                Some(provider_id)
            );
            assert_eq!(
                crate::settings::get_current_provider(&AppType::Codex).as_deref(),
                Some(provider_id)
            );
        });
    }

    #[test]
    fn extract_gemini_common_config_strips_credentials_keeps_shareable() {
        // Gemini 的共享片段会被 deep-merge 回**其它** Gemini 供应商的 env
        // (旧版切换时合并片段；新版片段冻结只给旧版读)，因此任何凭据都不得进入片段。
        // 之前这里只硬编码跳过 GEMINI_API_KEY/GOOGLE_GEMINI_BASE_URL，而
        // GOOGLE_API_KEY 是 provider.rs 认可的一等 Gemini 凭据 → 会泄露到别的供应商。
        let settings = json!({
            "env": {
                "GEMINI_API_KEY": "g-gem",
                "GOOGLE_API_KEY": "g-legacy-real-key",
                "GOOGLE_GEMINI_BASE_URL": "https://gemini.example",
                "GOOGLE_APPLICATION_CREDENTIALS": "/path/creds.json",
                "SOME_PROXY_AUTH_TOKEN": "tok-proxy",
                // 关键字段归供应商：片段已冻结，收进去会从行里剥掉、再也写不回 live
                "GEMINI_MODEL": "gemini-2.5-pro",
                "GOOGLE_GENAI_USE_VERTEXAI": "true",
                // 可共享的非机密配置必须保留
                "GEMINI_TIMEOUT_MS": "30000"
            }
        });

        let snippet =
            ProviderService::extract_gemini_common_config(&settings).expect("extract should work");
        let value: Value = serde_json::from_str(&snippet).expect("snippet is valid JSON");

        for leaked in [
            "GEMINI_API_KEY",
            "GOOGLE_API_KEY",
            "GOOGLE_APPLICATION_CREDENTIALS",
            "SOME_PROXY_AUTH_TOKEN",
            "GEMINI_MODEL",
            "GOOGLE_GENAI_USE_VERTEXAI",
        ] {
            assert!(
                value.get(leaked).is_none(),
                "{leaked} must not go into the shared Gemini snippet"
            );
        }
        assert_eq!(
            value.get("GEMINI_TIMEOUT_MS").and_then(|v| v.as_str()),
            Some("30000"),
            "shareable non-secret config must be preserved"
        );
    }

    /// 造一个「已被污染」的现场：片段里带 A 账号的凭据 + 一个合法可共享键。
    #[test]
    fn sensitive_key_matcher_covers_common_credential_namings() {
        for key in [
            // 裸 `_KEY`：最常见的写法，却曾被"只枚举 `_API_KEY` 这些子类"漏在外面
            "OPENAI_KEY",
            "GROQ_KEY",
            "XAI_KEY",
            // 不带分隔符的复合写法
            "VOLC_ACCESSKEY",
            "ALIYUN_SECRETKEY",
            "SOME_APITOKEN",
            // personal access token：既不含 TOKEN 也不含 KEY
            "GITHUB_PAT",
            "gitlab_pat",
            // 口令类缩写
            "MYSQL_PWD",
            "DB_PASS",
            "GPG_PASSPHRASE",
            "AWS_CREDS",
            // 发往上游的自定义请求头、Cookie、Authorization
            "ANTHROPIC_CUSTOM_HEADERS",
            "GEMINI_CLI_CUSTOM_HEADERS",
            "headers",
            "UPSTREAM_COOKIE",
            "PROXY_AUTHORIZATION",
        ] {
            assert!(
                ProviderService::is_sensitive_config_key(key),
                "{key} must be treated as a credential"
            );
        }

        // 后缀必须带下划线，不能把正常配置一起卷进来
        for key in [
            "PATH",
            "OLDPWD",
            "GEMINI_COMPAT",
            "SSL_BYPASS",
            "GEMINI_TIMEOUT_MS",
            "CLAUDE_CODE_MAX_OUTPUT_TOKENS",
            // 名字带 HEADER 但不是发往上游的凭据：普通开关、用户自己的遥测端点
            "CLAUDE_CODE_ATTRIBUTION_HEADER",
            "OTEL_EXPORTER_OTLP_HEADERS",
        ] {
            assert!(
                !ProviderService::is_sensitive_config_key(key),
                "{key} is ordinary shareable config and must not be stripped"
            );
        }
    }

    /// 测试夹具：按键排序写一份 `~/.gemini/.env`。
    fn write_gemini_env(map: &HashMap<String, String>) -> Result<(), AppError> {
        let mut keys: Vec<&String> = map.keys().collect();
        keys.sort();
        let text = keys
            .into_iter()
            .map(|key| format!("{key}={}", map[key]))
            .collect::<Vec<_>>()
            .join("\n");
        crate::gemini_config::write_gemini_env_text_atomic(&text)
    }

    fn seed_leaked_gemini_state(db: &Arc<Database>) {
        db.set_config_snippet(
            "gemini",
            Some(
                json!({
                    "GOOGLE_API_KEY": "key-A-leaked",
                    "SOME_PROXY_AUTH_TOKEN": "tok-A-leaked",
                    "GEMINI_TIMEOUT_MS": "30000"
                })
                .to_string(),
            ),
        )
        .expect("seed snippet");

        // 受害者 B：泄漏的密钥已经被合并进它的 env
        let victim = Provider::with_id(
            "b".into(),
            "Relay B".into(),
            json!({ "env": {
                "GOOGLE_GEMINI_BASE_URL": "https://relay-b.example",
                "GOOGLE_API_KEY": "key-A-leaked",
                "GEMINI_TIMEOUT_MS": "30000"
            }}),
            None,
        );
        db.save_provider("gemini", &victim).expect("save victim");

        // 供应商 C：自己写了同名键但值不同，不能被误删
        let unrelated = Provider::with_id(
            "c".into(),
            "Own Key C".into(),
            json!({ "env": {
                "GOOGLE_GEMINI_BASE_URL": "https://c.example",
                "GOOGLE_API_KEY": "key-C-owned"
            }}),
            None,
        );
        db.save_provider("gemini", &unrelated).expect("save c");
    }

    /// Saving the active provider while takeover has never been enabled must
    /// rewrite the real live file immediately.
    #[tokio::test]
    #[serial]
    async fn update_current_claude_provider_writes_live_when_proxy_never_enabled() {
        let _home = TempHome::new();
        crate::settings::reload_settings().expect("reload settings");

        let db = Arc::new(Database::memory().expect("init db"));
        let state = AppState::new(db.clone());
        let original = Provider::with_id(
            "p1".into(),
            "Claude A".into(),
            json!({
                "env": {
                    "ANTHROPIC_AUTH_TOKEN": "token-a",
                    "ANTHROPIC_BASE_URL": "https://api.old.example"
                }
            }),
            None,
        );
        db.save_provider("claude", &original)
            .expect("save provider");
        db.set_current_provider("claude", "p1")
            .expect("set current provider");
        crate::settings::set_current_provider(&AppType::Claude, Some("p1"))
            .expect("set local current provider");
        write_live_for_state(&state, &AppType::Claude, &original).expect("seed live file");

        let mut updated = original.clone();
        updated.settings_config["env"]["ANTHROPIC_BASE_URL"] =
            Value::String("https://api.new.example".into());
        ProviderService::update(&state, AppType::Claude, None, updated)
            .expect("update current provider");

        let live: Value = read_json_file(&get_claude_settings_path()).expect("read live");
        assert_eq!(
            live["env"]["ANTHROPIC_BASE_URL"].as_str(),
            Some("https://api.new.example")
        );
    }

    /// 编辑当前供应商、去掉它的独有字段：live 里 CC Switch 写进去的那个值随之删掉，
    /// 用户自己的键不动。
    #[tokio::test]
    #[serial]
    async fn update_current_claude_provider_drops_the_compat_switch_it_no_longer_has() {
        let _home = TempHome::new();
        crate::settings::reload_settings().expect("reload settings");

        let db = Arc::new(Database::memory().expect("init db"));
        let state = AppState::new(db.clone());
        let original = Provider::with_id(
            "ds".into(),
            "DeepSeek".into(),
            json!({ "env": {
                "ANTHROPIC_BASE_URL": "https://api.deepseek.example/anthropic",
                "CLAUDE_CODE_DISABLE_ARTIFACT": "1"
            }}),
            None,
        );
        db.save_provider("claude", &original)
            .expect("save provider");
        ProviderService::switch(&state, AppType::Claude, "ds").expect("switch");
        let mut live: Value = read_json_file(&get_claude_settings_path()).expect("read live");
        live["env"]["DEBUG"] = json!("1");
        write_json_file(&get_claude_settings_path(), &live).expect("user edit");

        let mut updated = original.clone();
        updated.settings_config["env"]
            .as_object_mut()
            .expect("env")
            .remove("CLAUDE_CODE_DISABLE_ARTIFACT");
        ProviderService::update(&state, AppType::Claude, None, updated).expect("update");

        let live: Value = read_json_file(&get_claude_settings_path()).expect("read live");
        assert_eq!(
            live,
            json!({ "env": {
                "ANTHROPIC_BASE_URL": "https://api.deepseek.example/anthropic",
                "DEBUG": "1"
            }})
        );
    }

    /// 文件已经写成目标供应商、指针还没改时崩溃：下次启动按 pending 补完指针，
    /// 文件和指针重新一致。
    #[tokio::test]
    #[serial]
    async fn claude_switch_interrupted_before_the_pointer_rolls_forward() {
        use crate::mode::operation::failpoint;

        let _home = TempHome::new();
        crate::settings::reload_settings().expect("reload settings");

        let db = Arc::new(Database::memory().expect("init db"));
        let state = AppState::new(db.clone());
        for (id, url) in [("a", "https://a.example"), ("b", "https://b.example")] {
            let provider = Provider::with_id(
                id.into(),
                id.into(),
                json!({ "env": { "ANTHROPIC_BASE_URL": url } }),
                None,
            );
            db.save_provider("claude", &provider)
                .expect("save provider");
        }
        ProviderService::switch(&state, AppType::Claude, "a").expect("switch to a");

        failpoint::crash_at(Some("published:0"));
        let result = ProviderService::switch(&state, AppType::Claude, "b");
        failpoint::crash_at(None);
        assert!(result.is_err(), "the injected crash surfaces");

        let live: Value = read_json_file(&get_claude_settings_path()).expect("read live");
        assert_eq!(
            live["env"]["ANTHROPIC_BASE_URL"],
            json!("https://b.example")
        );
        assert_eq!(
            db.get_current_provider("claude")
                .expect("current")
                .as_deref(),
            Some("a"),
            "the pointer has not moved yet"
        );

        crate::mode::operation::recover_on_startup(&db);
        assert_eq!(
            db.get_current_provider("claude")
                .expect("current")
                .as_deref(),
            Some("b")
        );
        assert_eq!(
            crate::settings::get_current_provider(&AppType::Claude).as_deref(),
            Some("b")
        );
    }

    /// A stale backup row must be refreshed but must not divert the live write.
    #[tokio::test]
    #[serial]
    async fn update_current_claude_provider_writes_live_when_backup_row_is_stale() {
        let _home = TempHome::new();
        crate::settings::reload_settings().expect("reload settings");

        let db = Arc::new(Database::memory().expect("init db"));
        let state = AppState::new(db.clone());
        let original = Provider::with_id(
            "p1".into(),
            "Claude A".into(),
            json!({
                "env": {
                    "ANTHROPIC_AUTH_TOKEN": "token-a",
                    "ANTHROPIC_BASE_URL": "https://api.old.example"
                }
            }),
            None,
        );
        db.save_provider("claude", &original)
            .expect("save provider");
        db.set_current_provider("claude", "p1")
            .expect("set current provider");
        crate::settings::set_current_provider(&AppType::Claude, Some("p1"))
            .expect("set local current provider");
        write_live_for_state(&state, &AppType::Claude, &original).expect("seed live file");
        db.save_live_backup(
            "claude",
            &serde_json::to_string(&original.settings_config).expect("serialize backup"),
        )
        .await
        .expect("seed stale backup");
        assert!(!state.proxy_service.is_running().await);

        let mut updated = original.clone();
        updated.settings_config["env"]["ANTHROPIC_BASE_URL"] =
            Value::String("https://api.new.example".into());
        ProviderService::update(&state, AppType::Claude, None, updated)
            .expect("update current provider");

        let live: Value = read_json_file(&get_claude_settings_path()).expect("read live");
        assert_eq!(
            live["env"]["ANTHROPIC_BASE_URL"].as_str(),
            Some("https://api.new.example"),
            "a leftover backup row is not proxy mode: the edit goes to live"
        );
    }

    /// An enabled flag left behind by an interrupted teardown is not enough to
    /// suppress a live write when neither placeholder nor backup evidence exists.
    #[tokio::test]
    #[serial]
    async fn update_current_claude_provider_ignores_enabled_flag_without_evidence() {
        let _home = TempHome::new();
        crate::settings::reload_settings().expect("reload settings");

        let db = Arc::new(Database::memory().expect("init db"));
        let state = AppState::new(db.clone());
        let original = Provider::with_id(
            "p1".into(),
            "Claude A".into(),
            json!({
                "env": {
                    "ANTHROPIC_AUTH_TOKEN": "token-a",
                    "ANTHROPIC_BASE_URL": "https://api.old.example"
                }
            }),
            None,
        );
        db.save_provider("claude", &original)
            .expect("save provider");
        db.set_current_provider("claude", "p1")
            .expect("set current provider");
        crate::settings::set_current_provider(&AppType::Claude, Some("p1"))
            .expect("set local current provider");
        write_live_for_state(&state, &AppType::Claude, &original).expect("seed live file");
        let mut config = db
            .get_proxy_config_for_app("claude")
            .await
            .expect("read proxy config");
        config.enabled = true;
        db.update_proxy_config_for_app(config)
            .await
            .expect("leave enabled flag set");
        assert!(!state.proxy_service.is_running().await);

        let mut updated = original.clone();
        updated.settings_config["env"]["ANTHROPIC_BASE_URL"] =
            Value::String("https://api.new.example".into());
        ProviderService::update(&state, AppType::Claude, None, updated)
            .expect("update current provider");

        let live: Value = read_json_file(&get_claude_settings_path()).expect("read live");
        assert_eq!(
            live["env"]["ANTHROPIC_BASE_URL"].as_str(),
            Some("https://api.new.example")
        );
    }

    #[tokio::test]
    #[serial]
    async fn scrub_gemini_removes_leaked_credentials_from_snippet_and_providers() {
        let _home = TempHome::new();
        crate::settings::reload_settings().expect("reload settings");
        let db = Arc::new(Database::memory().expect("init db"));
        let state = AppState::new(db.clone());
        seed_leaked_gemini_state(&db);

        ProviderService::scrub_leaked_gemini_common_config(&state)
            .await
            .expect("scrub must succeed");

        // 片段：凭据清掉，可共享配置保留
        let snippet = db
            .get_config_snippet("gemini")
            .expect("read snippet")
            .expect("snippet must still exist");
        let snippet: Value = serde_json::from_str(&snippet).expect("valid json");
        assert!(snippet.get("GOOGLE_API_KEY").is_none());
        assert!(snippet.get("SOME_PROXY_AUTH_TOKEN").is_none());
        assert_eq!(
            snippet.get("GEMINI_TIMEOUT_MS").and_then(Value::as_str),
            Some("30000"),
            "shareable config must survive the scrub"
        );

        // 受害者 B：扩散过去的那一份被清掉
        let providers = db.get_all_providers("gemini").expect("providers");
        let victim_env = &providers["b"].settings_config["env"];
        assert!(
            victim_env.get("GOOGLE_API_KEY").is_none(),
            "leaked key must be removed from the victim provider"
        );
        assert_eq!(
            victim_env.get("GEMINI_TIMEOUT_MS").and_then(Value::as_str),
            Some("30000"),
            "non-credential config must not be touched"
        );
    }

    #[tokio::test]
    #[serial]
    async fn scrub_gemini_keeps_a_providers_own_differently_valued_key() {
        let _home = TempHome::new();
        crate::settings::reload_settings().expect("reload settings");
        let db = Arc::new(Database::memory().expect("init db"));
        let state = AppState::new(db.clone());
        seed_leaked_gemini_state(&db);

        ProviderService::scrub_leaked_gemini_common_config(&state)
            .await
            .expect("scrub must succeed");

        // 这条最容易写错成「按键名一刀切」：C 自己的密钥值与片段不同，是它自己的凭据
        let providers = db.get_all_providers("gemini").expect("providers");
        assert_eq!(
            providers["c"].settings_config["env"]
                .get("GOOGLE_API_KEY")
                .and_then(Value::as_str),
            Some("key-C-owned"),
            "a provider's own key must not be deleted by name matching"
        );
    }

    #[tokio::test]
    #[serial]
    async fn scrub_gemini_audit_records_key_names_but_never_values() {
        let _home = TempHome::new();
        crate::settings::reload_settings().expect("reload settings");
        let db = Arc::new(Database::memory().expect("init db"));
        let state = AppState::new(db.clone());
        seed_leaked_gemini_state(&db);

        ProviderService::scrub_leaked_gemini_common_config(&state)
            .await
            .expect("scrub must succeed");

        let audit_text = db
            .get_setting("gemini_common_config_scrub_audit_v1")
            .expect("read audit")
            .expect("an audit record must exist so the deletion is not silent");

        // 值绝不能进这条记录：`settings` 会随 WebDAV/S3 同步上传，留值等于把一次
        // 清除换成一份跨设备扩散、没有界面入口、永不过期的明文副本。
        assert!(
            !audit_text.contains("key-A-leaked") && !audit_text.contains("tok-A-leaked"),
            "the audit record must never carry credential values: {audit_text}"
        );

        // 但必须说清楚删了什么、从哪删的，否则用户只能靠翻日志
        let audit: Value = serde_json::from_str(&audit_text).expect("audit is JSON");
        let removed: Vec<&str> = audit["removedFromSnippet"]
            .as_array()
            .expect("removedFromSnippet array")
            .iter()
            .filter_map(Value::as_str)
            .collect();
        assert!(
            removed.contains(&"GOOGLE_API_KEY") && removed.contains(&"SOME_PROXY_AUTH_TOKEN"),
            "every key removed from the snippet must be named: {audit}"
        );
        let victim = audit["providers"]
            .as_array()
            .expect("providers array")
            .iter()
            .find(|entry| entry["id"] == json!("b"))
            .expect("every provider whose config gets rewritten must be recorded");
        assert_eq!(
            victim["removedKeys"],
            json!(["GOOGLE_API_KEY"]),
            "the record must name what was taken from each provider: {audit}"
        );
    }

    #[tokio::test]
    #[serial]
    async fn scrub_gemini_never_overwrites_an_existing_audit_record() {
        let _home = TempHome::new();
        crate::settings::reload_settings().expect("reload settings");
        let db = Arc::new(Database::memory().expect("init db"));
        let state = AppState::new(db.clone());
        seed_leaked_gemini_state(&db);

        // 上一轮改到一半就中止的情形：完成标记没置位，下次启动会重跑，但那时
        // 读到的"原始状态"已经残缺。无条件覆盖会拿残缺记录盖掉第一轮那份完整的。
        db.set_setting(
            "gemini_common_config_scrub_audit_v1",
            "{\"from\":\"an earlier, complete run\"}",
        )
        .expect("seed an existing audit record");

        ProviderService::scrub_leaked_gemini_common_config(&state)
            .await
            .expect("scrub must succeed");

        assert_eq!(
            db.get_setting("gemini_common_config_scrub_audit_v1")
                .expect("read audit")
                .as_deref(),
            Some("{\"from\":\"an earlier, complete run\"}"),
            "an audit record from an earlier run must survive a retry"
        );
    }

    #[tokio::test]
    #[serial]
    async fn scrub_gemini_cleans_the_live_env_without_a_current_provider() {
        let _home = TempHome::new();
        crate::settings::reload_settings().expect("reload settings");
        let db = Arc::new(Database::memory().expect("init db"));
        let state = AppState::new(db.clone());
        seed_leaked_gemini_state(&db);

        // 没有当前供应商——这正是 sync_current_provider_for_app 直接返回 Ok 而
        // 根本不写文件的分支。此时 live 若清不掉，片段又已被清空，下次切换的
        // backfill 就会把残留永久写进受害供应商的配置。
        write_gemini_env(&HashMap::from([
            ("GOOGLE_API_KEY".to_string(), "key-A-leaked".to_string()),
            ("GEMINI_TIMEOUT_MS".to_string(), "30000".to_string()),
            // 只存在于 live 的手工修改：定向删除必须保住它，全量重投影会抹掉
            (
                "HTTPS_PROXY".to_string(),
                "http://127.0.0.1:7890".to_string(),
            ),
        ]))
        .expect("seed live env");

        ProviderService::scrub_leaked_gemini_common_config(&state)
            .await
            .expect("scrub must succeed");

        let live = crate::gemini_config::read_gemini_env().expect("read live env");
        assert!(
            !live.contains_key("GOOGLE_API_KEY"),
            "the leaked credential must be gone from ~/.gemini/.env: {live:?}"
        );
        assert_eq!(
            live.get("HTTPS_PROXY").map(String::as_str),
            Some("http://127.0.0.1:7890"),
            "a hand-added live-only var must survive targeted removal: {live:?}"
        );
    }

    #[tokio::test]
    #[serial]
    async fn scrub_gemini_live_cleanup_preserves_the_rest_of_the_env_file() {
        let _home = TempHome::new();
        crate::settings::reload_settings().expect("reload settings");
        let db = Arc::new(Database::memory().expect("init db"));
        let state = AppState::new(db.clone());
        seed_leaked_gemini_state(&db);

        // 这是一次用户没主动触发的启动期清理，不该顺手重写与泄漏无关的内容。
        // read→HashMap→write 的往返会把注释、空行、无法识别的行全丢掉并按键名重排。
        let original = "\
# my own notes
GOOGLE_API_KEY=key-C-owned

GOOGLE_API_KEY=key-A-leaked
this line is not KEY=VALUE at all
GEMINI_TIMEOUT_MS=30000
";
        crate::gemini_config::write_gemini_env_text_atomic(original).expect("seed live env");

        ProviderService::scrub_leaked_gemini_common_config(&state)
            .await
            .expect("scrub must succeed");

        let raw = std::fs::read_to_string(crate::gemini_config::get_gemini_env_path())
            .expect("read live env");
        assert!(
            !raw.contains("key-A-leaked"),
            "the leaked line must be gone: {raw:?}"
        );
        assert!(
            raw.contains("# my own notes"),
            "comments must survive a targeted removal: {raw:?}"
        );
        assert!(
            raw.contains("this line is not KEY=VALUE at all"),
            "unparseable lines must survive a targeted removal: {raw:?}"
        );
        // 被泄漏值遮住的那条重新生效——正是想要的结果，遮住它的恰恰是泄漏值
        assert_eq!(
            crate::gemini_config::read_gemini_env()
                .expect("read live env")
                .get("GOOGLE_API_KEY")
                .map(String::as_str),
            Some("key-C-owned"),
            "only the matching line may be dropped: {raw:?}"
        );
    }

    #[tokio::test]
    #[serial]
    async fn scrub_gemini_aborts_before_clearing_the_snippet_when_the_live_backup_fails() {
        let _home = TempHome::new();
        crate::settings::reload_settings().expect("reload settings");
        let db = Arc::new(Database::memory().expect("init db"));
        let state = AppState::new(db.clone());
        seed_leaked_gemini_state(&db);

        // 关代理时这份快照会被原样写回 live。若清不动它却照样清了片段、置了完成标记，
        // 代理一停凭据就复活，而一次性标记保证不会再清第二次。
        db.save_live_backup("gemini", "}not json{")
            .await
            .expect("seed backup");

        let result = ProviderService::scrub_leaked_gemini_common_config(&state).await;
        assert!(
            result.is_err(),
            "a backup that cannot be cleaned must abort the scrub"
        );

        // 片段是「该剥哪些键」的唯一知识来源，中止后必须原样留着，否则下次重试
        // 会因为 poison 为空而直接短路，反倒把标记置上
        let snippet = db
            .get_config_snippet("gemini")
            .expect("read snippet")
            .expect("snippet must still exist");
        assert!(
            snippet.contains("key-A-leaked"),
            "the snippet must be left intact so the next boot can retry: {snippet}"
        );
        assert!(
            db.get_setting("gemini_common_config_credentials_scrubbed_v1")
                .expect("read flag")
                .is_none(),
            "the one-shot flag must not be set when the scrub aborted"
        );
    }

    #[tokio::test]
    #[serial]
    async fn scrub_gemini_leaves_no_residue_for_backfill_to_persist() {
        let _home = TempHome::new();
        crate::settings::reload_settings().expect("reload settings");
        let db = Arc::new(Database::memory().expect("init db"));
        let state = AppState::new(db.clone());
        seed_leaked_gemini_state(&db);

        ProviderService::scrub_leaked_gemini_common_config(&state)
            .await
            .expect("scrub must succeed");

        // 顺序陷阱回归：如果只清了片段，切走供应商时 remove_common_config_from_settings
        // 就不再认识这个键，live 里的残留会被 backfill 永久写进供应商配置。
        // 清理必须是原子的——清完之后，任何地方都不该再有那个值。
        let snippet = db
            .get_config_snippet("gemini")
            .expect("read snippet")
            .unwrap_or_default();
        assert!(!snippet.contains("key-A-leaked"));

        for (id, provider) in db.get_all_providers("gemini").expect("providers") {
            assert!(
                !provider
                    .settings_config
                    .to_string()
                    .contains("key-A-leaked"),
                "provider '{id}' still carries the leaked value"
            );
        }
    }

    #[tokio::test]
    #[serial]
    async fn scrub_gemini_is_idempotent_and_skips_on_second_run() {
        let _home = TempHome::new();
        crate::settings::reload_settings().expect("reload settings");
        let db = Arc::new(Database::memory().expect("init db"));
        let state = AppState::new(db.clone());
        seed_leaked_gemini_state(&db);

        ProviderService::scrub_leaked_gemini_common_config(&state)
            .await
            .expect("first run");

        // 第二次必须是 no-op：用户清理后重新填的凭据不能被再抹一遍
        db.set_config_snippet(
            "gemini",
            Some(json!({"GOOGLE_API_KEY": "restored"}).to_string()),
        )
        .expect("user re-adds a value");

        ProviderService::scrub_leaked_gemini_common_config(&state)
            .await
            .expect("second run");

        let snippet = db
            .get_config_snippet("gemini")
            .expect("read snippet")
            .expect("snippet exists");
        assert!(
            snippet.contains("restored"),
            "the one-shot flag must prevent a second scrub: {snippet}"
        );
    }

    #[test]
    fn extract_claude_common_config_strips_all_credentials_keeps_shareable() {
        // env 混入多种凭据（Anthropic/OpenRouter/Google/OpenAI/Gemini + AWS/Vertex）
        // 与可共享配置；顶层混入非标准的 apiKey/api_key 凭据与正常设置。
        let settings = json!({
            "env": {
                "ANTHROPIC_API_KEY": "sk-ant",
                "ANTHROPIC_AUTH_TOKEN": "tok-ant",
                "OPENROUTER_API_KEY": "sk-or",
                "GOOGLE_API_KEY": "g-key",
                "OPENAI_API_KEY": "sk-oai",
                "GEMINI_API_KEY": "g-gem",
                "AWS_ACCESS_KEY_ID": "AKIA",
                "AWS_SECRET_ACCESS_KEY": "secret",
                "AWS_SESSION_TOKEN": "sess",
                "GOOGLE_APPLICATION_CREDENTIALS": "/path/creds.json",
                "AWS_BEARER_TOKEN_BEDROCK": "bedrock-tok",
                "ANTHROPIC_BASE_URL": "https://example.com",
                "ANTHROPIC_MODEL": "claude-x",
                "CLAUDE_CODE_SUBAGENT_MODEL": "gpt-5.4-mini",
                "CLAUDE_CODE_MAX_CONTEXT_TOKENS": "400000",
                "CLAUDE_CODE_AUTO_COMPACT_WINDOW": "400000",
                // 可共享、非机密配置（复数 _TOKENS 不应被误剥）
                "ENABLE_TOOL_SEARCH": "true",
                "CLAUDE_CODE_MAX_OUTPUT_TOKENS": "8192"
            },
            "apiKey": "sk-top",
            "api_key": "sk-top2",
            "theme": "dark",
            "includeCoAuthoredBy": false
        });

        let snippet = ProviderService::extract_claude_common_config(&settings)
            .expect("extract should succeed");
        let value: Value = serde_json::from_str(&snippet).expect("snippet is valid JSON");

        // 所有凭据都不得出现在共享片段里
        let env = value.get("env");
        for leaked in [
            "ANTHROPIC_API_KEY",
            "ANTHROPIC_AUTH_TOKEN",
            "OPENROUTER_API_KEY",
            "GOOGLE_API_KEY",
            "OPENAI_API_KEY",
            "GEMINI_API_KEY",
            "AWS_ACCESS_KEY_ID",
            "AWS_SECRET_ACCESS_KEY",
            "AWS_SESSION_TOKEN",
            "GOOGLE_APPLICATION_CREDENTIALS",
            "AWS_BEARER_TOKEN_BEDROCK",
        ] {
            assert!(
                env.and_then(|e| e.get(leaked)).is_none(),
                "credential {leaked} must not leak into common config"
            );
        }
        assert!(
            value.get("apiKey").is_none() && value.get("api_key").is_none(),
            "top-level credentials must be stripped"
        );

        // 端点/模型（provider-specific 非机密）也应剥掉
        assert!(env.and_then(|e| e.get("ANTHROPIC_BASE_URL")).is_none());
        assert!(env.and_then(|e| e.get("ANTHROPIC_MODEL")).is_none());
        assert!(env
            .and_then(|e| e.get("CLAUDE_CODE_SUBAGENT_MODEL"))
            .is_none());
        assert!(env
            .and_then(|e| e.get("CLAUDE_CODE_MAX_CONTEXT_TOKENS"))
            .is_none());
        assert!(env
            .and_then(|e| e.get("CLAUDE_CODE_AUTO_COMPACT_WINDOW"))
            .is_none());

        // 可共享的非机密配置必须保留（含复数 _TOKENS 不被误剥）
        assert_eq!(
            env.and_then(|e| e.get("ENABLE_TOOL_SEARCH"))
                .and_then(|v| v.as_str()),
            Some("true")
        );
        assert_eq!(
            env.and_then(|e| e.get("CLAUDE_CODE_MAX_OUTPUT_TOKENS"))
                .and_then(|v| v.as_str()),
            Some("8192")
        );
        assert_eq!(value.get("theme").and_then(|v| v.as_str()), Some("dark"));
        assert_eq!(value.get("includeCoAuthoredBy"), Some(&json!(false)));
    }

    /// 关键字段（协议选择器、Bedrock/Vertex 区域、`/model` 的选择等）不进共享片段；
    /// 同在 `CLAUDE_CODE_USE_` 前缀下、与供应商无关的开关照常共享。
    #[test]
    fn extract_claude_common_config_keeps_key_fields_per_provider() {
        let settings = json!({
            "env": {
                "CLAUDE_CODE_USE_BEDROCK": "1",
                "CLAUDE_CODE_USE_VERTEX": "1",
                "CLAUDE_CODE_SKIP_BEDROCK_AUTH": "1",
                "AWS_REGION": "us-west-2",
                "AWS_PROFILE": "work",
                "CLOUD_ML_REGION": "us-east5",
                "VERTEX_REGION_CLAUDE_4_0_OPUS": "europe-west1",
                "ANTHROPIC_VERTEX_PROJECT_ID": "my-project",
                "ANTHROPIC_SMALL_FAST_MODEL": "haiku",
                "CLAUDE_CODE_SUBAGENT_MODEL_FORCE": "1",
                "CLAUDE_CODE_USE_POWERSHELL_TOOL": "1",
                "CLAUDE_CODE_ATTRIBUTION_HEADER": "0",
                "DISABLE_TELEMETRY": "1"
            },
            "model": "opus",
            "fallbackModel": "sonnet",
            "apiKeyHelper": "~/bin/key.sh",
            "awsAuthRefresh": "aws sso login",
            "hooks": { "Stop": [] },
            "theme": "dark"
        });

        let snippet = ProviderService::extract_claude_common_config(&settings)
            .expect("extract should succeed");
        let value: Value = serde_json::from_str(&snippet).expect("snippet is valid JSON");

        assert_eq!(
            value,
            json!({
                "env": {
                    "CLAUDE_CODE_USE_POWERSHELL_TOOL": "1",
                    "CLAUDE_CODE_ATTRIBUTION_HEADER": "0",
                    "DISABLE_TELEMETRY": "1"
                },
                "hooks": { "Stop": [] },
                "theme": "dark"
            })
        );
        assert!(
            snippet.find("CLAUDE_CODE_USE_POWERSHELL_TOOL") < snippet.find("DISABLE_TELEMETRY"),
            "removing keys must not reorder the ones that stay: {snippet}"
        );
    }

    /// Regression for issue #4272: Fable tier env keys must not enter the shared
    /// Claude common-config snippet (same class as haiku/sonnet/opus model pins).
    #[test]
    fn extract_claude_common_config_strips_fable_model_env_keys() {
        let settings = json!({
            "env": {
                "ANTHROPIC_DEFAULT_HAIKU_MODEL": "haiku-mapped",
                "ANTHROPIC_DEFAULT_HAIKU_MODEL_NAME": "Haiku Mapped",
                "ANTHROPIC_DEFAULT_SONNET_MODEL": "sonnet-mapped[1M]",
                "ANTHROPIC_DEFAULT_SONNET_MODEL_NAME": "Sonnet Mapped",
                "ANTHROPIC_DEFAULT_OPUS_MODEL": "opus-mapped[1M]",
                "ANTHROPIC_DEFAULT_OPUS_MODEL_NAME": "Opus Mapped",
                "ANTHROPIC_DEFAULT_FABLE_MODEL": "deepseek-v4-flash[1M]",
                "ANTHROPIC_DEFAULT_FABLE_MODEL_NAME": "deepseek-v4-flash",
                "ANTHROPIC_MODEL": "default-mapped",
                "ENABLE_TOOL_SEARCH": "true"
            },
            "theme": "dark"
        });

        let snippet = ProviderService::extract_claude_common_config(&settings)
            .expect("extract should succeed");
        let value: Value = serde_json::from_str(&snippet).expect("snippet is valid JSON");
        let env = value.get("env");

        for stripped in [
            "ANTHROPIC_DEFAULT_HAIKU_MODEL",
            "ANTHROPIC_DEFAULT_HAIKU_MODEL_NAME",
            "ANTHROPIC_DEFAULT_SONNET_MODEL",
            "ANTHROPIC_DEFAULT_SONNET_MODEL_NAME",
            "ANTHROPIC_DEFAULT_OPUS_MODEL",
            "ANTHROPIC_DEFAULT_OPUS_MODEL_NAME",
            "ANTHROPIC_DEFAULT_FABLE_MODEL",
            "ANTHROPIC_DEFAULT_FABLE_MODEL_NAME",
            "ANTHROPIC_MODEL",
        ] {
            assert!(
                env.and_then(|e| e.get(stripped)).is_none(),
                "provider-specific model key {stripped} must not enter common config"
            );
        }

        assert_eq!(
            env.and_then(|e| e.get("ENABLE_TOOL_SEARCH"))
                .and_then(|v| v.as_str()),
            Some("true")
        );
        assert_eq!(value.get("theme").and_then(|v| v.as_str()), Some("dark"));
    }

    #[test]
    fn extract_credentials_returns_expected_values() {
        let provider = Provider::with_id(
            "claude".into(),
            "Claude".into(),
            json!({
                "env": {
                    "ANTHROPIC_AUTH_TOKEN": "token",
                    "ANTHROPIC_BASE_URL": "https://claude.example"
                }
            }),
            None,
        );
        let (api_key, base_url) =
            ProviderService::extract_credentials(&provider, &AppType::Claude).unwrap();
        assert_eq!(api_key, "token");
        assert_eq!(base_url, "https://claude.example");
    }

    #[test]
    fn extract_codex_common_config_strips_provider_fields_and_injected_artifacts() {
        // 顶层 experimental_bearer_token 模拟无活跃路由时的 fallback 注入；
        // web_search = "disabled" 是 cc-switch 对黑名单网关注入的哨兵；
        // 顶层 wire_api 模拟无 model_provider 时的 fallback 写法；
        // [mcp.servers] 是历史错误格式，sync_all_enabled 清不掉它。
        let config_toml = r#"model_provider = "azure"
model = "gpt-4"
wire_api = "chat"
disable_response_storage = true
model_reasoning_effort = "high"
approval_policy = "on-request"
experimental_bearer_token = "sk-live-secret"
model_catalog_json = "cc-switch-model-catalog.json"
web_search = "disabled"

[agents]
default_subagent_model = "gpt-4-mini"
max_threads = 4

[model_providers.azure]
name = "Azure OpenAI"
base_url = "https://azure.example/v1"
wire_api = "responses"

[mcp_servers.my_server]
base_url = "http://localhost:8080"

[mcp.servers.legacy_server]
command = "legacy-cmd"
"#;

        let settings = json!({ "config": config_toml });
        let extracted = ProviderService::extract_codex_common_config(&settings)
            .expect("extract_codex_common_config should succeed");

        assert!(
            !extracted
                .lines()
                .any(|line| line.trim_start().starts_with("model_provider")),
            "should remove top-level model_provider"
        );
        assert!(
            !extracted
                .lines()
                .any(|line| line.trim_start().starts_with("model =")),
            "should remove top-level model"
        );
        assert!(
            !extracted.contains("[model_providers"),
            "should remove entire model_providers table"
        );
        // MCP 归 DB mcp_servers 表所有，不得进共享片段（含历史错误格式 [mcp.servers]）
        assert!(
            !extracted.contains("mcp_servers") && !extracted.contains("http://localhost:8080"),
            "should strip mcp_servers from the shared snippet, got: {extracted}"
        );
        assert!(
            !extracted.contains("[mcp") && !extracted.contains("legacy-cmd"),
            "should strip the legacy [mcp.servers] form from the shared snippet, got: {extracted}"
        );
        // 顶层 wire_api 是供应商路由语义（model_providers 整表已剥，
        // 剩余任何 wire_api 都意味着泄漏）
        assert!(
            !extracted.contains("wire_api"),
            "should strip top-level wire_api from the shared snippet, got: {extracted}"
        );
        // 注入产物不得进共享片段（bearer token 泄漏为密钥级问题）
        assert!(
            !extracted.contains("experimental_bearer_token")
                && !extracted.contains("sk-live-secret"),
            "should strip top-level fallback bearer token, got: {extracted}"
        );
        assert!(
            !extracted.contains("model_catalog_json"),
            "should strip catalog projection pointer, got: {extracted}"
        );
        assert!(
            !extracted.contains("web_search"),
            "should strip the cc-switch web_search disabled sentinel, got: {extracted}"
        );
        // 关键字段归供应商（片段已冻结，收进去会从行里剥掉、再也写不回 live）
        for key in [
            "disable_response_storage",
            "model_reasoning_effort",
            "default_subagent_model",
        ] {
            assert!(
                !extracted.contains(key),
                "key field {key} must stay with the provider, got: {extracted}"
            );
        }
        // 真正可共享的键保留
        assert!(
            extracted.contains("approval_policy = \"on-request\"")
                && extracted.contains("max_threads = 4"),
            "shareable keys must survive extraction, got: {extracted}"
        );
    }

    #[test]
    fn extract_codex_common_config_keeps_user_set_web_search() {
        let config_toml = "web_search = \"enabled\"\ndisable_response_storage = true\n";
        let settings = json!({ "config": config_toml });
        let extracted = ProviderService::extract_codex_common_config(&settings)
            .expect("extract should succeed");
        assert!(
            extracted.contains("web_search = \"enabled\""),
            "a user-set web_search value is a shareable preference, got: {extracted}"
        );
    }

    #[tokio::test]
    #[serial]
    async fn editing_the_routed_claude_provider_rewrites_the_proxy_contract() {
        let _home = TempHome::new();
        crate::settings::reload_settings().expect("reload settings");

        let db = Arc::new(Database::memory().expect("init db"));
        let state = AppState::new(db.clone());

        let original = Provider::with_id(
            "p1".into(),
            "Claude A".into(),
            json!({
                "env": {
                    "ANTHROPIC_API_KEY": "token-a",
                    "ANTHROPIC_BASE_URL": "https://api.a.example",
                    "ANTHROPIC_MODEL": "model-a"
                }
            }),
            None,
        );
        db.save_provider("claude", &original)
            .expect("save provider");
        db.set_current_provider("claude", "p1")
            .expect("set current provider");
        crate::settings::set_current_provider(&AppType::Claude, Some("p1"))
            .expect("set local current provider");
        write_json_file(
            &get_claude_settings_path(),
            &json!({
                "env": {
                    "ANTHROPIC_BASE_URL": "https://api.a.example",
                    "ANTHROPIC_API_KEY": "token-a",
                    "ANTHROPIC_MODEL": "model-a"
                },
                "permissions": { "allow": ["Bash"] }
            }),
        )
        .expect("seed live file");

        db.update_proxy_config(ProxyConfig {
            listen_port: 0,
            ..Default::default()
        })
        .await
        .expect("update proxy config");
        crate::mode::controller::enter(&state, &AppType::Claude, false)
            .await
            .expect("enter routing mode");
        let proxy_url = state
            .proxy_service
            .build_proxy_urls()
            .await
            .expect("proxy url")
            .0;

        let updated = Provider::with_id(
            "p1".into(),
            "Claude A".into(),
            json!({
                "env": {
                    "ANTHROPIC_API_KEY": "token-updated",
                    "ANTHROPIC_BASE_URL": "https://api.updated.example",
                    "ANTHROPIC_MODEL": "model-updated"
                }
            }),
            None,
        );
        ProviderService::update(&state, AppType::Claude, None, updated)
            .expect("update routed provider");

        let live: Value = read_json_file(&get_claude_settings_path()).expect("read live");
        let env = &live["env"];
        assert_eq!(env["ANTHROPIC_API_KEY"], "PROXY_MANAGED");
        assert_eq!(env["ANTHROPIC_BASE_URL"], proxy_url.as_str());
        assert!(env.get("ANTHROPIC_MODEL").is_none(), "{live}");
        assert_eq!(
            env["ANTHROPIC_DEFAULT_SONNET_MODEL_NAME"], "model-updated",
            "the contract's display names follow the edited provider"
        );
        assert_eq!(
            live["permissions"],
            json!({ "allow": ["Bash"] }),
            "user settings in live are left alone"
        );

        state
            .proxy_service
            .stop()
            .await
            .expect("stop proxy service");
    }

    #[tokio::test]
    #[serial]
    async fn update_current_codex_provider_refreshes_and_clears_catalog_during_takeover() {
        let _home = TempHome::new();
        crate::settings::reload_settings().expect("reload settings");

        let db = Arc::new(Database::memory().expect("init db"));
        let state = AppState::new(db.clone());

        let mut original = Provider::with_id(
            "p1".into(),
            "Codex A".into(),
            json!({
                "auth": { "OPENAI_API_KEY": "token-a" },
                "config": r#"model_provider = "custom"
model = "old-model"

[model_providers.custom]
name = "Codex A"
base_url = "https://api.a.example/v1"
wire_api = "responses"
requires_openai_auth = true
"#,
                "modelCatalog": {
                    "models": [{ "model": "old-model" }]
                }
            }),
            None,
        );
        original.meta = Some(ProviderMeta {
            api_format: Some("openai_responses".into()),
            ..Default::default()
        });
        db.save_provider("codex", &original).expect("save provider");
        db.set_current_provider("codex", "p1")
            .expect("set current provider");
        crate::settings::set_current_provider(&AppType::Codex, Some("p1"))
            .expect("set local current provider");

        db.update_proxy_config(ProxyConfig {
            listen_port: 0,
            ..Default::default()
        })
        .await
        .expect("update proxy config");
        crate::mode::controller::enter(&state, &AppType::Codex, false)
            .await
            .expect("enter routing mode");
        assert!(
            state
                .proxy_service
                .live_has_proxy_placeholder(&AppType::Codex),
            "Codex live config should carry the proxy contract"
        );

        let mut updated = original.clone();
        updated.settings_config["config"] = json!(
            r#"model_provider = "custom"
model = "gpt-5.4"

[model_providers.custom]
name = "Codex A"
base_url = "https://api.updated.example/v1"
wire_api = "responses"
requires_openai_auth = true
"#
        );
        updated.settings_config["modelCatalog"] = json!({
            "models": [{ "model": "gpt-5.4", "displayName": "GPT 5.4" }]
        });

        ProviderService::update(&state, AppType::Codex, None, updated.clone())
            .expect("update current Codex provider mapping");

        let catalog_path = crate::codex_config::get_codex_model_catalog_path();
        let catalog: Value = read_json_file(&catalog_path).expect("read generated catalog");
        assert_eq!(catalog["models"][0]["slug"], "gpt-5.4");
        assert_eq!(
            catalog["models"][0]["input_modalities"],
            json!(["text", "image"]),
            "unknown/GPT models must fail open to image input"
        );
        let live_config = fs::read_to_string(crate::codex_config::get_codex_config_path())
            .expect("read Codex config.toml");
        assert!(live_config.contains("model_catalog_json"));

        updated.settings_config["modelCatalog"] = json!({ "models": [] });
        ProviderService::update(&state, AppType::Codex, None, updated)
            .expect("remove current Codex provider mapping");

        let live_config = fs::read_to_string(crate::codex_config::get_codex_config_path())
            .expect("read Codex config.toml after mapping removal");
        assert!(
            !live_config.contains("model_catalog_json"),
            "removing mappings during takeover must clear the stale catalog pointer"
        );

        state
            .proxy_service
            .stop()
            .await
            .expect("stop proxy service");
    }

    #[cfg(any(target_os = "macos", windows, target_os = "linux"))]
    #[tokio::test]
    #[serial]
    async fn update_current_claude_desktop_provider_syncs_profile_when_proxy_takeover_is_active() {
        let home = TempHome::new();
        crate::settings::reload_settings().expect("reload settings");

        let db = Arc::new(Database::memory().expect("init db"));
        let state = AppState::new(db.clone());

        let mut original = Provider::with_id(
            "p1".into(),
            "Desktop A".into(),
            json!({
                "env": {
                    "ANTHROPIC_AUTH_TOKEN": "token-a",
                    "ANTHROPIC_BASE_URL": "https://opencode.ai/zen/go"
                }
            }),
            None,
        );
        original.meta = Some(ProviderMeta {
            api_format: Some("openai_chat".into()),
            claude_desktop_mode: Some(ClaudeDesktopMode::Proxy),
            claude_desktop_model_routes: std::collections::HashMap::from([(
                "claude-sonnet-4-6".into(),
                ClaudeDesktopModelRoute {
                    model: "deepseek-v4-flash".into(),
                    label_override: Some("DeepSeek V4 Flash".into()),
                    supports_1m: None,
                },
            )]),
            ..Default::default()
        });
        db.save_provider("claude-desktop", &original)
            .expect("save provider");
        db.set_current_provider("claude-desktop", "p1")
            .expect("set current provider");
        crate::settings::set_current_provider(&AppType::ClaudeDesktop, Some("p1"))
            .expect("set local current provider");

        // Claude Desktop keeps backup state from takeover startup; this sentinel only
        // marks takeover as active so provider updates rewrite the 3P profile.
        db.save_live_backup("claude-desktop", "{}")
            .await
            .expect("seed live backup");
        {
            let mut config = db
                .get_proxy_config_for_app("claude-desktop")
                .await
                .expect("get app proxy config");
            config.enabled = true;
            db.update_proxy_config_for_app(config)
                .await
                .expect("update app proxy config");
        }

        state
            .proxy_service
            .start()
            .await
            .expect("start proxy service");

        let mut updated = Provider::with_id(
            "p1".into(),
            "Desktop A".into(),
            json!({
                "env": {
                    "ANTHROPIC_AUTH_TOKEN": "token-updated",
                    "ANTHROPIC_BASE_URL": "https://opencode.ai/zen/go"
                }
            }),
            None,
        );
        updated.meta = Some(ProviderMeta {
            api_format: Some("openai_chat".into()),
            claude_desktop_mode: Some(ClaudeDesktopMode::Proxy),
            claude_desktop_model_routes: std::collections::HashMap::from([(
                "claude-sonnet-4-6".into(),
                ClaudeDesktopModelRoute {
                    model: "deepseek-v4-flash".into(),
                    label_override: Some("DeepSeek V4 Flash Updated".into()),
                    supports_1m: Some(true),
                },
            )]),
            ..Default::default()
        });

        ProviderService::update(&state, AppType::ClaudeDesktop, None, updated.clone())
            .expect("update current provider");

        let backup = db
            .get_live_backup("claude-desktop")
            .await
            .expect("get live backup")
            .expect("backup exists");
        assert_eq!(
            backup.original_config, "{}",
            "Claude Desktop provider edits should not rewrite takeover backup"
        );

        let profile_path = claude_desktop_profile_path(home.dir.path());
        let profile: Value = read_json_file(&profile_path).expect("read desktop profile");
        assert_eq!(
            profile["inferenceGatewayBaseUrl"],
            json!("http://127.0.0.1:15721/claude-desktop"),
            "desktop profile should stay pointed at the local gateway during takeover"
        );
        assert_eq!(profile["inferenceGatewayAuthScheme"], json!("bearer"));
        assert_eq!(
            profile["inferenceModels"],
            json!([{ "name": "claude-sonnet-4-6", "labelOverride": "DeepSeek V4 Flash Updated", "supports1m": true }]),
            "provider edits should propagate into the Claude Desktop 3P profile during takeover"
        );
    }

    #[test]
    #[serial]
    fn rename_rejects_missing_original_provider() {
        with_test_home(|state, _| {
            let original = openclaw_provider("deepseek");
            ProviderService::add(state, AppType::OpenClaw, original.clone(), false)
                .expect("seed db-only provider");

            let mut renamed = original.clone();
            renamed.id = "deepseek-copy".to_string();

            let err = ProviderService::update(
                state,
                AppType::OpenClaw,
                Some("missing-provider"),
                renamed,
            )
            .expect_err("stale originalId should be rejected");

            assert!(
                err.to_string().contains("Original provider"),
                "expected missing original provider error, got {err:?}"
            );
            assert!(
                state
                    .db
                    .get_provider_by_id("deepseek-copy", AppType::OpenClaw.as_str())
                    .expect("query renamed provider")
                    .is_none(),
                "rename must not create a new row when originalId is stale"
            );
        });
    }

    #[test]
    #[serial]
    fn db_only_additive_update_survives_live_config_parse_errors() {
        with_test_home(|state, home| {
            let provider = openclaw_provider("deepseek");
            ProviderService::add(state, AppType::OpenClaw, provider.clone(), false)
                .expect("seed db-only provider");

            let stored = state
                .db
                .get_provider_by_id("deepseek", AppType::OpenClaw.as_str())
                .expect("query stored provider")
                .expect("provider should exist");
            assert_eq!(
                stored
                    .meta
                    .as_ref()
                    .and_then(|meta| meta.live_config_managed),
                Some(false),
                "db-only provider should be marked as not live-managed"
            );

            let openclaw_dir = home.join(".openclaw");
            fs::create_dir_all(&openclaw_dir).expect("create openclaw dir");
            fs::write(openclaw_dir.join("openclaw.json"), "{ invalid json5")
                .expect("write malformed config");

            let mut updated = stored.clone();
            updated.name = "DeepSeek Edited".to_string();
            updated.meta.get_or_insert_with(ProviderMeta::default);

            ProviderService::update(state, AppType::OpenClaw, None, updated)
                .expect("db-only update should ignore live parse errors");

            let saved = state
                .db
                .get_provider_by_id("deepseek", AppType::OpenClaw.as_str())
                .expect("query updated provider")
                .expect("updated provider should exist");
            assert_eq!(saved.name, "DeepSeek Edited");
        });
    }

    #[test]
    #[serial]
    fn sync_current_provider_for_app_skips_db_only_opencode_provider() {
        with_test_home(|state, _| {
            let provider = opencode_provider("db-only-opencode");
            ProviderService::add(state, AppType::OpenCode, provider.clone(), false)
                .expect("seed db-only opencode provider");

            ProviderService::sync_current_provider_for_app(state, AppType::OpenCode)
                .expect("sync additive opencode providers");

            let live_providers = crate::opencode_config::get_providers()
                .expect("read opencode providers after sync");
            assert!(
                !live_providers.contains_key(&provider.id),
                "db-only opencode provider should not be written to live during sync"
            );
        });
    }

    #[test]
    #[serial]
    fn sync_current_provider_for_app_skips_db_only_openclaw_provider() {
        with_test_home(|state, _| {
            let provider = openclaw_provider("db-only-openclaw");
            ProviderService::add(state, AppType::OpenClaw, provider.clone(), false)
                .expect("seed db-only openclaw provider");

            ProviderService::sync_current_provider_for_app(state, AppType::OpenClaw)
                .expect("sync additive openclaw providers");

            let live_providers = crate::openclaw_config::get_providers()
                .expect("read openclaw providers after sync");
            assert!(
                !live_providers.contains_key(&provider.id),
                "db-only openclaw provider should not be written to live during sync"
            );
        });
    }

    #[test]
    #[serial]
    fn sync_current_provider_for_app_preserves_legacy_live_opencode_provider() {
        with_test_home(|state, _| {
            let provider = opencode_provider("legacy-opencode");
            crate::opencode_config::set_provider(&provider.id, provider.settings_config.clone())
                .expect("seed opencode live provider");
            state
                .db
                .save_provider(AppType::OpenCode.as_str(), &provider)
                .expect("seed legacy opencode provider in db");

            let mut updated = provider.clone();
            updated.settings_config["options"]["apiKey"] = Value::String("updated-key".to_string());
            state
                .db
                .save_provider(AppType::OpenCode.as_str(), &updated)
                .expect("update legacy opencode provider in db");

            ProviderService::sync_current_provider_for_app(state, AppType::OpenCode)
                .expect("sync legacy opencode provider");

            let live_providers =
                crate::opencode_config::get_providers().expect("read opencode providers");
            assert_eq!(
                live_providers
                    .get(&provider.id)
                    .and_then(|config| config.get("options"))
                    .and_then(|options| options.get("apiKey")),
                Some(&Value::String("updated-key".to_string())),
                "legacy provider that already exists in live should still be synced"
            );
        });
    }

    #[test]
    #[serial]
    fn sync_current_provider_for_app_restores_legacy_opencode_provider_after_live_reset() {
        with_test_home(|state, _| {
            let provider = opencode_provider("legacy-opencode-reset");
            state
                .db
                .save_provider(AppType::OpenCode.as_str(), &provider)
                .expect("seed legacy opencode provider in db");

            ProviderService::sync_current_provider_for_app(state, AppType::OpenCode)
                .expect("sync legacy opencode provider after reset");

            let live_providers =
                crate::opencode_config::get_providers().expect("read opencode providers");
            assert!(
                live_providers.contains_key(&provider.id),
                "legacy opencode provider should be restored when live config is reset"
            );
        });
    }

    #[test]
    #[serial]
    fn sync_current_provider_for_app_restores_legacy_openclaw_provider_after_live_reset() {
        with_test_home(|state, _| {
            let mut provider = openclaw_provider("legacy-openclaw-reset");
            provider.settings_config["models"] = json!([
                {
                    "id": "claude-sonnet-4",
                    "name": "Claude Sonnet 4"
                }
            ]);
            state
                .db
                .save_provider(AppType::OpenClaw.as_str(), &provider)
                .expect("seed legacy openclaw provider in db");

            ProviderService::sync_current_provider_for_app(state, AppType::OpenClaw)
                .expect("sync legacy openclaw provider after reset");

            let live_providers =
                crate::openclaw_config::get_providers().expect("read openclaw providers");
            assert!(
                live_providers.contains_key(&provider.id),
                "legacy openclaw provider should be restored when live config is reset"
            );
        });
    }

    #[test]
    #[serial]
    fn add_first_managed_codex_with_reauth_required_account_is_rejected() {
        with_test_home(|state, _| {
            crate::settings::reload_settings().expect("reload settings");
            tauri::async_runtime::block_on(async {
                state
                    .codex_oauth_manager
                    .add_test_account_with_access_token("acct-legacy", "managed-token", None)
                    .await
                    .expect("seed legacy account without id_token");
            });
            let provider = managed_codex_provider("managed-legacy", "acct-legacy");

            let error = ProviderService::add(state, AppType::Codex, provider.clone(), false)
                .expect_err("reauth-required account must not be written to live auth");
            assert!(
                error.to_string().contains("id_token"),
                "backend should require re-login even if the frontend gate is bypassed: {error}"
            );
            assert!(state
                .db
                .get_provider_by_id(&provider.id, AppType::Codex.as_str())
                .expect("query provider")
                .is_none());
            assert!(!crate::codex_config::get_codex_auth_path().exists());
        });
    }

    #[test]
    #[serial]
    fn switch_from_managed_codex_official_to_unbound_clears_live_without_backfilling_token() {
        with_test_home(|state, _| {
            crate::settings::reload_settings().expect("reload settings");
            tauri::async_runtime::block_on(async {
                state
                    .codex_oauth_manager
                    .add_test_account_with_user_identity(
                        "acct-managed",
                        "managed-token",
                        "managed-user",
                    )
                    .await
                    .expect("seed managed Codex OAuth account");
            });

            let mut managed = Provider::with_id(
                "managed-official".to_string(),
                "Managed Official".to_string(),
                json!({
                    "auth": {},
                    "config": ""
                }),
                None,
            );
            managed.category = Some("official".to_string());
            managed.meta = Some(ProviderMeta {
                auth_binding: Some(AuthBinding {
                    source: AuthBindingSource::ManagedAccount,
                    auth_provider: Some("codex_oauth".to_string()),
                    account_id: Some("acct-managed".to_string()),
                }),
                ..Default::default()
            });

            let mut unbound = Provider::with_id(
                "unbound-official".to_string(),
                "Unbound Official".to_string(),
                json!({
                    "auth": {},
                    "config": ""
                }),
                None,
            );
            unbound.category = Some("official".to_string());

            state
                .db
                .save_provider(AppType::Codex.as_str(), &managed)
                .expect("save managed provider");
            state
                .db
                .save_provider(AppType::Codex.as_str(), &unbound)
                .expect("save unbound provider");

            ProviderService::switch(state, AppType::Codex, "managed-official")
                .expect("switch to managed official");
            let live_auth: Value = read_json_file(&crate::codex_config::get_codex_auth_path())
                .expect("read managed live auth");
            assert_eq!(
                live_auth
                    .pointer("/tokens/access_token")
                    .and_then(Value::as_str),
                Some("managed-token"),
                "managed switch should write the selected ChatGPT token to live auth"
            );

            // Simulate a bare Codex CLI self-refresh. The app marker still
            // describes the pre-refresh write, while both access and refresh
            // token material on disk have rotated.
            let rotated_id_token = crate::codex_config::test_codex_id_token("managed-user");
            let rotated_live_auth = crate::codex_config::codex_managed_oauth_auth_value(
                "acct-managed",
                "cli-rotated-access",
                Some(&rotated_id_token),
                "cli-rotated-refresh",
                "2099-01-02T03:04:05Z",
            );
            write_json_file(
                &crate::codex_config::get_codex_auth_path(),
                &rotated_live_auth,
            )
            .expect("simulate Codex CLI token rotation");

            ProviderService::switch(state, AppType::Codex, "unbound-official")
                .expect("switch to unbound official");

            assert!(
                !crate::codex_config::get_codex_auth_path().exists(),
                "switching to an unbound official provider should clear the recorded managed live auth"
            );
            assert_eq!(
                tauri::async_runtime::block_on(
                    state
                        .codex_oauth_manager
                        .test_refresh_token_for_account("acct-managed")
                )
                .as_deref(),
                Some("cli-rotated-refresh"),
                "switch-away must adopt the CLI-rotated refresh token before deleting live auth"
            );

            let saved_managed = state
                .db
                .get_provider_by_id("managed-official", AppType::Codex.as_str())
                .expect("query managed provider")
                .expect("managed provider should exist");
            assert_eq!(
                saved_managed.settings_config.get("auth"),
                Some(&json!({})),
                "switch-away backfill must not persist the managed access token into provider storage"
            );
        });
    }

    #[test]
    #[serial]
    fn managed_codex_switch_adopts_outgoing_cli_rotation_before_account_or_key_overwrite() {
        with_test_home(|state, _| {
            crate::settings::reload_settings().expect("reload settings");
            tauri::async_runtime::block_on(async {
                state
                    .codex_oauth_manager
                    .add_test_account_with_user_identity("acct-a", "managed-access-a", "user-a")
                    .await
                    .expect("seed account A");
                state
                    .codex_oauth_manager
                    .add_test_account_with_user_identity("acct-b", "managed-access-b", "user-b")
                    .await
                    .expect("seed account B");
            });

            let provider_a = managed_codex_provider("managed-a", "acct-a");
            let provider_b = managed_codex_provider("managed-b", "acct-b");
            let mut third_party = Provider::with_id(
                "third-party".to_string(),
                "Third Party".to_string(),
                json!({
                    "auth": { "OPENAI_API_KEY": "sk-third-party" },
                    "config": r#"model_provider = "third"
[model_providers.third]
name = "Third"
base_url = "https://third.example/v1"
wire_api = "responses"
"#
                }),
                None,
            );
            third_party.category = Some("custom".to_string());
            for provider in [&provider_a, &provider_b, &third_party] {
                state
                    .db
                    .save_provider(AppType::Codex.as_str(), provider)
                    .expect("save provider");
            }

            ProviderService::switch(state, AppType::Codex, &provider_a.id)
                .expect("activate managed A");
            let id_token_a = crate::codex_config::test_codex_id_token("user-a");
            write_json_file(
                &crate::codex_config::get_codex_auth_path(),
                &crate::codex_config::codex_managed_oauth_auth_value(
                    "acct-a",
                    "cli-access-a1",
                    Some(&id_token_a),
                    "cli-refresh-a1",
                    "2099-01-02T00:00:00Z",
                ),
            )
            .expect("rotate account A live auth");

            ProviderService::switch(state, AppType::Codex, &provider_b.id)
                .expect("switch managed A to managed B");
            assert_eq!(
                tauri::async_runtime::block_on(
                    state
                        .codex_oauth_manager
                        .test_refresh_token_for_account("acct-a")
                )
                .as_deref(),
                Some("cli-refresh-a1"),
                "A's CLI generation must be adopted before B overwrites auth.json"
            );
            let live_b: Value =
                read_json_file(&crate::codex_config::get_codex_auth_path()).expect("read B auth");
            assert_eq!(
                live_b.pointer("/tokens/account_id").and_then(Value::as_str),
                Some("acct-b")
            );

            let id_token_b = crate::codex_config::test_codex_id_token("user-b");
            write_json_file(
                &crate::codex_config::get_codex_auth_path(),
                &crate::codex_config::codex_managed_oauth_auth_value(
                    "acct-b",
                    "cli-access-b1",
                    Some(&id_token_b),
                    "cli-refresh-b1",
                    "2099-01-03T00:00:00Z",
                ),
            )
            .expect("rotate account B live auth");

            ProviderService::switch(state, AppType::Codex, &third_party.id)
                .expect("switch managed B to API-key provider");
            assert_eq!(
                tauri::async_runtime::block_on(
                    state
                        .codex_oauth_manager
                        .test_refresh_token_for_account("acct-b")
                )
                .as_deref(),
                Some("cli-refresh-b1"),
                "B's CLI generation must be adopted before the third-party switch removes auth.json"
            );
            assert!(
                !crate::codex_config::get_codex_auth_path().exists(),
                "third-party switches are config-only: auth.json is removed"
            );
            let live_config = std::fs::read_to_string(crate::codex_config::get_codex_config_path())
                .expect("read third-party config");
            assert!(
                live_config.contains("experimental_bearer_token = \"sk-third-party\""),
                "the third-party key rides in config.toml; got:\n{live_config}"
            );
        });
    }

    #[test]
    #[serial]
    fn managed_codex_direct_update_adopts_outgoing_cli_rotation_and_commits_target_binding() {
        with_test_home(|state, _| {
            crate::settings::reload_settings().expect("reload settings");
            tauri::async_runtime::block_on(async {
                state
                    .codex_oauth_manager
                    .add_test_account_with_user_identity("acct-a", "managed-access-a", "user-a")
                    .await
                    .expect("seed account A");
                state
                    .codex_oauth_manager
                    .add_test_account_with_user_identity("acct-b", "managed-access-b", "user-b")
                    .await
                    .expect("seed account B");
            });

            let provider = managed_codex_provider("managed-official-a", "acct-a");
            state
                .db
                .save_provider(AppType::Codex.as_str(), &provider)
                .expect("save managed official provider");
            ProviderService::switch(state, AppType::Codex, &provider.id)
                .expect("activate managed account A");
            assert!(
                tauri::async_runtime::block_on(state.db.get_live_backup(AppType::Codex.as_str()))
                    .expect("read initial live backup")
                    .is_none(),
                "direct update precondition requires no takeover backup"
            );

            let id_token_a = crate::codex_config::test_codex_id_token("user-a");
            write_json_file(
                &crate::codex_config::get_codex_auth_path(),
                &crate::codex_config::codex_managed_oauth_auth_value(
                    "acct-a",
                    "cli-access-a1",
                    Some(&id_token_a),
                    "cli-refresh-a1",
                    "2099-03-01T00:00:00Z",
                ),
            )
            .expect("simulate account A CLI rotation");

            let mut updated = provider.clone();
            updated.name = "OpenAI Official B".to_string();
            updated
                .meta
                .as_mut()
                .and_then(|meta| meta.auth_binding.as_mut())
                .expect("managed binding")
                .account_id = Some("acct-b".to_string());

            ProviderService::update(state, AppType::Codex, None, updated.clone())
                .expect("directly update managed binding from A to B");

            assert_eq!(
                tauri::async_runtime::block_on(
                    state
                        .codex_oauth_manager
                        .test_refresh_token_for_account("acct-a")
                )
                .as_deref(),
                Some("cli-refresh-a1"),
                "direct update must adopt A's CLI generation before overwriting live auth"
            );
            let saved = state
                .db
                .get_provider_by_id(&provider.id, AppType::Codex.as_str())
                .expect("read updated provider")
                .expect("updated provider exists");
            assert_eq!(saved.name, updated.name);
            assert_eq!(
                saved
                    .meta
                    .as_ref()
                    .and_then(|meta| meta.managed_account_id_for("codex_oauth"))
                    .as_deref(),
                Some("acct-b")
            );

            let live_b: Value = read_json_file(&crate::codex_config::get_codex_auth_path())
                .expect("read account B live auth");
            assert_eq!(
                live_b.pointer("/tokens/account_id").and_then(Value::as_str),
                Some("acct-b")
            );
            assert!(
                crate::codex_config::codex_auth_matches_recorded_managed_oauth(&live_b, "acct-b")
                    .expect("check account B marker"),
                "clearing outgoing account A must not remove account B's marker"
            );
            assert!(
                tauri::async_runtime::block_on(state.db.get_live_backup(AppType::Codex.as_str()))
                    .expect("read live backup after direct update")
                    .is_none(),
                "direct update must not create a takeover backup"
            );
        });
    }

    #[test]
    #[serial]
    fn same_account_managed_codex_update_rejects_equal_timestamp_refresh_conflict() {
        with_test_home(|state, _| {
            crate::settings::reload_settings().expect("reload settings");
            tauri::async_runtime::block_on(async {
                state
                    .codex_oauth_manager
                    .add_test_account_with_user_identity(
                        "acct-managed",
                        "managed-access",
                        "managed-user",
                    )
                    .await
                    .expect("seed managed account");
            });

            let provider = managed_codex_provider("managed-same-account", "acct-managed");
            state
                .db
                .save_provider(AppType::Codex.as_str(), &provider)
                .expect("save managed provider");
            ProviderService::switch(state, AppType::Codex, &provider.id)
                .expect("activate managed provider");

            // Different refresh material at the exact manager generation
            // timestamp is ambiguous at millisecond precision. A same-account
            // update has no outgoing-account guard, so its managed bundle
            // preflight itself must refuse to overwrite this CLI generation.
            tauri::async_runtime::block_on(
                state
                    .codex_oauth_manager
                    .test_set_token_updated_at_ms("acct-managed", 1_700_000_000_000),
            );
            let id_token = crate::codex_config::test_codex_id_token("managed-user");
            let cli_live_auth = crate::codex_config::codex_managed_oauth_auth_value(
                "acct-managed",
                "cli-access-r1",
                Some(&id_token),
                "cli-refresh-r1",
                "2023-11-14T22:13:20Z",
            );
            write_json_file(&crate::codex_config::get_codex_auth_path(), &cli_live_auth)
                .expect("seed equal-timestamp CLI generation");

            let mut updated = provider.clone();
            updated.name = "Managed updated".to_string();
            let error = ProviderService::update(state, AppType::Codex, None, updated)
                .expect_err("ambiguous same-account generation must block the live write");
            assert!(
                error
                    .to_string()
                    .contains("无法安全判断 refresh token 新旧"),
                "update should explain the safe-write rejection: {error}"
            );

            let live_after: Value = read_json_file(&crate::codex_config::get_codex_auth_path())
                .expect("read preserved CLI auth");
            assert_eq!(
                live_after, cli_live_auth,
                "same-account managed update must not overwrite ambiguous CLI token material"
            );
            assert_eq!(
                tauri::async_runtime::block_on(
                    state
                        .codex_oauth_manager
                        .test_refresh_token_for_account("acct-managed")
                )
                .as_deref(),
                Some("test-refresh-token"),
                "ambiguous CLI material must not replace the manager generation either"
            );
            assert_eq!(
                state
                    .db
                    .get_provider_by_id(&provider.id, AppType::Codex.as_str())
                    .expect("read provider after rejected update")
                    .expect("provider remains present")
                    .name,
                provider.name,
                "rejected preflight must leave the provider row unchanged"
            );
        });
    }

    #[test]
    #[serial]
    fn switch_away_rejects_legacy_refresh_conflict_on_every_retry() {
        with_test_home(|state, _| {
            crate::settings::reload_settings().expect("reload settings");
            tauri::async_runtime::block_on(async {
                state
                    .codex_oauth_manager
                    .add_test_account_with_user_identity(
                        "acct-legacy",
                        "managed-access",
                        "legacy-user",
                    )
                    .await
                    .expect("seed managed account");
            });

            let managed = managed_codex_provider("managed-legacy", "acct-legacy");
            let mut unbound = Provider::with_id(
                "unbound-official".to_string(),
                "Unbound Official".to_string(),
                json!({
                    "auth": {},
                    "config": ""
                }),
                None,
            );
            unbound.category = Some("official".to_string());
            for provider in [&managed, &unbound] {
                state
                    .db
                    .save_provider(AppType::Codex.as_str(), provider)
                    .expect("save provider");
            }
            ProviderService::switch(state, AppType::Codex, &managed.id)
                .expect("activate managed provider");

            tauri::async_runtime::block_on(
                state
                    .codex_oauth_manager
                    .test_set_token_updated_at_ms("acct-legacy", 0),
            );
            let id_token = crate::codex_config::test_codex_id_token("legacy-user");
            let cli_live_auth = crate::codex_config::codex_managed_oauth_auth_value(
                "acct-legacy",
                "cli-access-r1",
                Some(&id_token),
                "cli-refresh-r1",
                "2023-11-14T22:13:20Z",
            );
            write_json_file(&crate::codex_config::get_codex_auth_path(), &cli_live_auth)
                .expect("seed CLI generation against legacy manager state");

            for attempt in 1..=2 {
                let error = ProviderService::switch(state, AppType::Codex, &unbound.id)
                    .expect_err("legacy conflict must block every switch-away retry");
                assert!(
                    error
                        .to_string()
                        .contains("无法安全判断 refresh token 新旧"),
                    "attempt {attempt} should remain ambiguous: {error}"
                );
                let live_after: Value = read_json_file(&crate::codex_config::get_codex_auth_path())
                    .expect("read preserved CLI auth");
                assert_eq!(
                    live_after, cli_live_auth,
                    "attempt {attempt} must not overwrite or delete the CLI generation"
                );
                assert_eq!(
                    state
                        .db
                        .get_current_provider(AppType::Codex.as_str())
                        .expect("read current provider")
                        .as_deref(),
                    Some(managed.id.as_str()),
                    "attempt {attempt} must not commit the target provider"
                );
            }

            assert_eq!(
                tauri::async_runtime::block_on(
                    state
                        .codex_oauth_manager
                        .test_refresh_token_for_account("acct-legacy")
                )
                .as_deref(),
                Some("test-refresh-token"),
                "ambiguous legacy retries must keep manager material unchanged"
            );
        });
    }

    #[test]
    #[serial]
    fn codex_auth_center_remove_and_logout_clear_live_credentials_and_marker() {
        with_test_home(|state, _| {
            crate::settings::reload_settings().expect("reload settings");
            tauri::async_runtime::block_on(async {
                state
                    .codex_oauth_manager
                    .add_test_account_with_user_identity(
                        "acct-managed",
                        "managed-access",
                        "managed-user",
                    )
                    .await
                    .expect("seed managed account");
            });
            let provider = managed_codex_provider("managed-auth-center", "acct-managed");
            state
                .db
                .save_provider(AppType::Codex.as_str(), &provider)
                .expect("save managed provider");
            ProviderService::switch(state, AppType::Codex, &provider.id)
                .expect("activate managed provider");
            assert!(crate::codex_config::get_codex_auth_path().exists());
            assert!(crate::codex_config::codex_managed_oauth_live_auth_marker_exists());

            tauri::async_runtime::block_on(
                state.codex_oauth_manager.remove_account("acct-managed"),
            )
            .expect("remove managed account");
            assert!(
                !crate::codex_config::get_codex_auth_path().exists(),
                "removing the active account must delete its refreshable live auth"
            );
            assert!(
                !crate::codex_config::codex_managed_oauth_live_auth_marker_exists(),
                "removing the active account must delete its marker"
            );
            assert_eq!(
                state
                    .db
                    .get_provider_by_id(&provider.id, AppType::Codex.as_str())
                    .expect("read provider")
                    .and_then(|provider| provider.meta)
                    .and_then(|meta| meta.managed_account_id_for("codex_oauth"))
                    .as_deref(),
                Some("acct-managed"),
                "the binding is retained so re-login with the same account can recover it"
            );

            tauri::async_runtime::block_on(async {
                state
                    .codex_oauth_manager
                    .add_test_account_with_user_identity(
                        "acct-managed",
                        "managed-access-2",
                        "managed-user",
                    )
                    .await
                    .expect("re-login managed account");
            });
            ProviderService::switch(state, AppType::Codex, &provider.id)
                .expect("reactivate managed provider after re-login");
            assert!(crate::codex_config::get_codex_auth_path().exists());

            tauri::async_runtime::block_on(state.codex_oauth_manager.clear_auth())
                .expect("logout all managed accounts");
            assert!(!crate::codex_config::get_codex_auth_path().exists());
            assert!(!crate::codex_config::codex_managed_oauth_live_auth_marker_exists());
            assert!(
                tauri::async_runtime::block_on(state.codex_oauth_manager.list_accounts())
                    .is_empty()
            );
        });
    }

    #[test]
    #[serial]
    fn deleted_codex_account_can_rebind_or_switch_in_either_mode() {
        for mode in ["direct", "proxy"] {
            for rebind in [true, false] {
                with_test_home(|state, _| {
                    crate::settings::reload_settings().unwrap();
                    let runtime = tauri::async_runtime::handle();
                    let token = crate::codex_config::test_codex_id_token("same-user");
                    runtime
                        .block_on(
                            state
                                .codex_oauth_manager
                                .add_test_account_with_workspace_and_access_token(
                                    "old-local-id",
                                    "workspace",
                                    "old-access",
                                    Some(&token),
                                ),
                        )
                        .unwrap();
                    runtime.block_on(async {
                        let mut config = state.db.get_proxy_config().await.unwrap();
                        config.listen_port = 0;
                        state.db.update_proxy_config(config).await.unwrap();
                    });
                    let current = managed_codex_provider("current", "old-local-id");
                    state.db.save_provider("codex", &current).unwrap();
                    ProviderService::switch(state, AppType::Codex, &current.id).unwrap();
                    if mode == "proxy" {
                        runtime
                            .block_on(crate::mode::controller::enter(
                                state,
                                &AppType::Codex,
                                false,
                            ))
                            .unwrap();
                    }
                    runtime
                        .block_on(remove_codex_account_leaving_dangling_bindings(
                            state,
                            "old-local-id",
                        ))
                        .unwrap();
                    let restarted = (mode == "direct").then(|| AppState::new(state.db.clone()));
                    let state = restarted.as_ref().unwrap_or(state);
                    // Ordinary login creates a new local ID, even for the same user/workspace.
                    runtime
                        .block_on(
                            state
                                .codex_oauth_manager
                                .add_test_account_with_workspace_and_access_token(
                                    "new-local-id",
                                    "workspace",
                                    "new-access",
                                    Some(&token),
                                ),
                        )
                        .unwrap();
                    assert_eq!(
                        ProviderService::managed_codex_oauth_account_id(
                            &state
                                .db
                                .get_provider_by_id("current", "codex")
                                .unwrap()
                                .unwrap()
                        )
                        .as_deref(),
                        Some("old-local-id")
                    );

                    // 绑定已失效：直连下进入路由要报错让用户重新绑定，客户端文件不动。
                    if mode == "direct" {
                        let error = runtime
                            .block_on(crate::mode::controller::enter(
                                state,
                                &AppType::Codex,
                                false,
                            ))
                            .unwrap_err();
                        assert!(error.contains("选择账号"), "{error}");
                        assert!(!crate::mode::current::is_proxy(&AppType::Codex));
                        assert!(!state
                            .proxy_service
                            .live_has_proxy_placeholder(&AppType::Codex));
                    }

                    let target = managed_codex_provider(
                        if rebind { "current" } else { "target" },
                        "new-local-id",
                    );
                    if rebind {
                        ProviderService::update(state, AppType::Codex, None, target.clone())
                            .unwrap();
                    } else {
                        state.db.save_provider("codex", &target).unwrap();
                        ProviderService::switch(state, AppType::Codex, &target.id).unwrap();
                    }
                    let auth: Value = read_json_file(&crate::codex_config::get_codex_auth_path())
                        .unwrap_or_else(|error| panic!("{mode}, rebind={rebind}: {error}"));
                    assert_eq!(
                        auth["tokens"]["access_token"], "new-access",
                        "{mode}, rebind={rebind}"
                    );
                    assert_eq!(
                        crate::mode::current::provider_for(
                            &state.db,
                            &AppType::Codex,
                            crate::mode::current::Purpose::InUse,
                        )
                        .unwrap()
                        .as_deref(),
                        Some(target.id.as_str())
                    );

                    if mode == "proxy" && !rebind {
                        // 路由换了，直连指针仍是失效的那家：在路由模式下改它的绑定只存行，
                        // 退出路由时再写回。
                        ProviderService::update(
                            state,
                            AppType::Codex,
                            None,
                            managed_codex_provider("current", "new-local-id"),
                        )
                        .unwrap();
                    }
                    runtime.block_on(async {
                        if mode == "direct" {
                            crate::mode::controller::enter(state, &AppType::Codex, false)
                                .await
                                .unwrap();
                        }
                        crate::mode::controller::exit(state, &AppType::Codex)
                            .await
                            .unwrap();
                        assert!(!state.proxy_service.is_running().await);
                    });
                    let restored: Value =
                        read_json_file(&crate::codex_config::get_codex_auth_path()).unwrap();
                    assert_eq!(
                        restored["tokens"]["access_token"], "new-access",
                        "leaving routing mode must keep the replacement account ({mode}, rebind={rebind})"
                    );
                });
            }
        }
    }

    /// 删号但不解绑供应商：模拟旧版删号留下的悬空行。解绑失败、别的设备同步来的行
    /// 同样会悬空，恢复路径仍要能处理。
    async fn remove_codex_account_leaving_dangling_bindings(
        state: &AppState,
        account_id: &str,
    ) -> Result<(), String> {
        let _switch_guard = state
            .proxy_service
            .lock_switch_for_app(AppType::Codex.as_str())
            .await;
        state
            .codex_oauth_manager
            .remove_account(account_id)
            .await
            .map_err(|error| error.to_string())
    }

    fn seed_codex_recovery_accounts(state: &AppState) {
        tauri::async_runtime::block_on(async {
            for (id, user) in [("acct-old", "old-user"), ("acct-new", "new-user")] {
                state
                    .codex_oauth_manager
                    .add_test_account_with_user_identity(id, "access", user)
                    .await
                    .expect("seed account");
            }
        });
    }

    #[test]
    #[serial]
    fn codex_account_removal_and_logout_unbind_current_and_inactive_cards() {
        for logout_all in [false, true] {
            with_test_home(|state, _| {
                crate::settings::reload_settings().unwrap();
                seed_codex_recovery_accounts(state);
                let current = managed_codex_provider("current", "acct-old");
                let mut legacy = managed_codex_provider("legacy", "acct-old");
                legacy.category = None;
                legacy.notes = Some("keep notes".into());
                legacy.settings_config["auth"] =
                    json!({"tokens": {"refresh_token": "stale-snapshot"}});
                legacy.settings_config["model"] = json!("keep-model");
                let other = managed_codex_provider("other", "acct-new");
                // 从另一台设备同步来的行：绑的是那台设备的本地账号，本机删号或登出都不能动它，
                // 否则解绑会随云同步回去，弄坏那台设备上正常的绑定。
                let foreign = managed_codex_provider("foreign", "acct-other-device");
                let mut unrelated = managed_codex_provider("unrelated", "acct-old");
                unrelated
                    .meta
                    .as_mut()
                    .unwrap()
                    .auth_binding
                    .as_mut()
                    .unwrap()
                    .source = AuthBindingSource::ProviderConfig;
                for provider in [&current, &legacy, &other, &foreign, &unrelated] {
                    state.db.save_provider("codex", provider).unwrap();
                }
                state.db.save_provider("claude", &current).unwrap();
                state
                    .db
                    .conn
                    .lock()
                    .unwrap()
                    .execute(
                        "UPDATE providers SET meta = json_set(meta, '$.futureField', 'keep') WHERE id = 'legacy'",
                        [],
                    )
                    .unwrap();
                ProviderService::switch(state, AppType::Codex, &current.id).unwrap();
                if logout_all {
                    tauri::async_runtime::block_on(
                        crate::commands::logout_codex_oauth_with_switch_lock(state),
                    )
                    .unwrap();
                } else {
                    tauri::async_runtime::block_on(
                        crate::commands::remove_codex_oauth_account_with_switch_lock(
                            state, "acct-old",
                        ),
                    )
                    .unwrap();
                }
                for id in ["current", "legacy"] {
                    let saved = state.db.get_provider_by_id(id, "codex").unwrap().unwrap();
                    assert!(saved.meta.unwrap().auth_binding.is_none());
                    assert_eq!(saved.settings_config["auth"], json!({}));
                    assert_eq!(saved.category.as_deref(), Some("official"));
                }
                let saved_legacy = state
                    .db
                    .get_provider_by_id("legacy", "codex")
                    .unwrap()
                    .unwrap();
                assert_eq!(saved_legacy.notes, legacy.notes);
                assert_eq!(saved_legacy.settings_config["model"], "keep-model");
                let future: String = state
                    .db
                    .conn
                    .lock()
                    .unwrap()
                    .query_row(
                        "SELECT json_extract(meta, '$.futureField') FROM providers WHERE id = 'legacy'",
                        [],
                        |row| row.get(0),
                    )
                    .unwrap();
                assert_eq!(future, "keep");
                let bound_to = |id: &str, app: &str| {
                    state
                        .db
                        .get_provider_by_id(id, app)
                        .unwrap()
                        .unwrap()
                        .meta
                        .unwrap()
                        .managed_account_id_for("codex_oauth")
                };
                assert_eq!(bound_to("other", "codex").is_none(), logout_all);
                assert_eq!(
                    bound_to("foreign", "codex").as_deref(),
                    Some("acct-other-device")
                );
                assert!(state
                    .db
                    .get_provider_by_id("unrelated", "codex")
                    .unwrap()
                    .unwrap()
                    .meta
                    .unwrap()
                    .auth_binding
                    .is_some());
                assert!(state
                    .db
                    .get_provider_by_id("current", "claude")
                    .unwrap()
                    .unwrap()
                    .meta
                    .unwrap()
                    .auth_binding
                    .is_some());
                assert_eq!(
                    crate::settings::get_current_provider(&AppType::Codex).as_deref(),
                    Some("current")
                );
                assert!(!crate::codex_config::get_codex_auth_path().exists());
                assert!(!crate::codex_config::codex_managed_oauth_live_auth_marker_exists());
                ProviderService::switch(state, AppType::Codex, "legacy").unwrap();
                assert!(
                    !crate::codex_config::get_codex_auth_path().exists(),
                    "unbound legacy card must not resurrect stored credentials"
                );
                assert!(state
                    .db
                    .unbind_codex_managed_accounts(&["acct-old".to_string()])
                    .unwrap()
                    .is_empty());
            });
        }
    }

    #[test]
    #[serial]
    fn codex_failed_batch_unbinding_rolls_back_and_leaves_a_recovery_path() {
        for rebind in [false, true] {
            with_test_home(|state, _| {
                crate::settings::reload_settings().unwrap();
                seed_codex_recovery_accounts(state);
                let current = managed_codex_provider("a-current", "acct-old");
                let second = managed_codex_provider("z-second", "acct-old");
                let target = managed_codex_provider("target", "acct-new");
                for provider in [&current, &second, &target] {
                    state.db.save_provider("codex", provider).unwrap();
                }
                ProviderService::switch(state, AppType::Codex, &current.id).unwrap();
                state
                    .db
                    .conn
                    .lock()
                    .unwrap()
                    .execute_batch(
                        "CREATE TRIGGER reject_unbind BEFORE UPDATE OF meta ON providers
                     WHEN OLD.id = 'z-second' AND json_extract(NEW.meta, '$.authBinding') IS NULL
                     BEGIN SELECT RAISE(FAIL, 'injected unbind failure'); END;",
                    )
                    .unwrap();
                let error = tauri::async_runtime::block_on(
                    crate::commands::remove_codex_oauth_account_with_switch_lock(state, "acct-old"),
                )
                .unwrap_err();
                assert!(error.contains("账号已删除") && error.contains("解绑失败"));
                for id in [&current.id, &second.id] {
                    assert_eq!(
                        state
                            .db
                            .get_provider_by_id(id, "codex")
                            .unwrap()
                            .unwrap()
                            .meta
                            .unwrap()
                            .managed_account_id_for("codex_oauth")
                            .as_deref(),
                        Some("acct-old")
                    );
                }
                assert!(
                    !tauri::async_runtime::block_on(state.codex_oauth_manager.list_accounts())
                        .iter()
                        .any(|account| account.id == "acct-old")
                );
                if rebind {
                    let replacement = managed_codex_provider(&current.id, "acct-new");
                    ProviderService::update(state, AppType::Codex, None, replacement).unwrap();
                } else {
                    ProviderService::switch(state, AppType::Codex, &target.id).unwrap();
                }
                let auth: Value =
                    read_json_file(&crate::codex_config::get_codex_auth_path()).unwrap();
                assert_eq!(auth["tokens"]["account_id"], "acct-new");
            });
        }
    }

    #[test]
    #[serial]
    fn deleted_codex_account_recovers_after_persisted_startup() {
        for exit_state in ["detached", "crashed"] {
            let _guard = test_guard();
            let _home = TempHome::new();
            crate::settings::reload_settings().unwrap();
            let runtime = tauri::async_runtime::handle();
            let db = Arc::new(Database::memory().unwrap());
            let state = AppState::new(db.clone());
            let token = crate::codex_config::test_codex_id_token("same-user");
            runtime.block_on(async {
                state
                    .codex_oauth_manager
                    .add_test_account_with_workspace_and_access_token(
                        "old-local-id",
                        "workspace",
                        "old-access",
                        Some(&token),
                    )
                    .await
                    .unwrap();
                let mut config = state.db.get_proxy_config().await.unwrap();
                config.listen_port = 0;
                state.db.update_proxy_config(config).await.unwrap();
            });
            let current = managed_codex_provider("current", "old-local-id");
            state.db.save_provider("codex", &current).unwrap();
            ProviderService::switch(&state, AppType::Codex, "current").unwrap();
            runtime.block_on(async {
                crate::mode::controller::enter(&state, &AppType::Codex, false)
                    .await
                    .unwrap();
                remove_codex_account_leaving_dangling_bindings(&state, "old-local-id")
                    .await
                    .unwrap();
                state
                    .codex_oauth_manager
                    .add_test_account_with_workspace_and_access_token(
                        "new-local-id",
                        "workspace",
                        "new-access",
                        Some(&token),
                    )
                    .await
                    .unwrap();
            });
            // Model a later CLI login. Startup recovery must leave it intact.
            let native_auth = crate::codex_config::codex_managed_oauth_auth_value(
                "native-workspace",
                "native-access",
                Some(&token),
                "native-refresh",
                "2026-09-14T00:00:00Z",
            );
            write_json_file(&crate::codex_config::get_codex_auth_path(), &native_auth).unwrap();
            runtime.block_on(async {
                if exit_state == "detached" {
                    // 直连供应商的账号没了写不出来：只清掉占位符，登录不动。
                    crate::mode::controller::detach_all(&state).await;
                    assert!(!state
                        .proxy_service
                        .live_has_proxy_placeholder(&AppType::Codex));
                } else {
                    // 崩溃：客户端仍指着代理，只是监听没了。
                    state.proxy_service.stop().await.unwrap();
                    assert!(state
                        .proxy_service
                        .live_has_proxy_placeholder(&AppType::Codex));
                }
                assert!(crate::mode::current::is_proxy(&AppType::Codex));
            });
            drop(state);

            crate::settings::reload_settings().unwrap();
            // 内存库跨重启沿用同一实例；要验证的持久化状态（账号库、live-state、
            // settings、auth.json）都在磁盘上。
            let restarted = AppState::new(db);
            assert_eq!(
                ProviderService::managed_codex_oauth_account_id(
                    &restarted
                        .db
                        .get_provider_by_id("current", "codex")
                        .unwrap()
                        .unwrap()
                )
                .as_deref(),
                Some("old-local-id"),
            );
            runtime.block_on(async {
                let accounts = restarted.codex_oauth_manager.list_accounts().await;
                assert_eq!(accounts.len(), 1);
                assert_eq!(accounts[0].id, "new-local-id");
                let store_path = crate::config::get_app_config_dir().join("codex_oauth_auth.json");
                let persisted_accounts = fs::read(&store_path).unwrap();
                // Match setup's ordering: extract common config from the direct live
                // file before re-attaching routing mode.
                ProviderService::scrub_leaked_gemini_common_config(&restarted)
                    .await
                    .unwrap();
                crate::initialize_common_config_snippets(&restarted);
                crate::mode::controller::startup(&restarted).await;
                // 路由的账号失效，接不上就退回直连。
                assert!(
                    !crate::mode::current::is_proxy(&AppType::Codex),
                    "{exit_state}"
                );
                assert!(
                    !restarted
                        .db
                        .get_proxy_config_for_app("codex")
                        .await
                        .unwrap()
                        .enabled
                );
                assert!(!restarted.proxy_service.is_running().await);
                assert!(!restarted
                    .proxy_service
                    .live_has_proxy_placeholder(&AppType::Codex));
                assert_eq!(fs::read(&store_path).unwrap(), persisted_accounts);
                assert_eq!(
                    read_json_file::<Value>(&crate::codex_config::get_codex_auth_path()).unwrap(),
                    native_auth
                );
                // Stub the network refresh only after proving the account survived on disk.
                restarted
                    .codex_oauth_manager
                    .test_cache_access_token("new-local-id", "new-access")
                    .await;
            });
            let target = managed_codex_provider("current", "new-local-id");
            ProviderService::update(&restarted, AppType::Codex, None, target).unwrap();
            runtime.block_on(async {
                crate::mode::controller::enter(&restarted, &AppType::Codex, false)
                    .await
                    .unwrap();
                assert!(
                    restarted
                        .db
                        .get_proxy_config_for_app("codex")
                        .await
                        .unwrap()
                        .enabled
                );
                assert!(restarted
                    .proxy_service
                    .live_has_proxy_placeholder(&AppType::Codex));
                crate::mode::controller::exit(&restarted, &AppType::Codex)
                    .await
                    .unwrap();
                assert!(!restarted.proxy_service.is_running().await);
            });
            let auth: Value = read_json_file(&crate::codex_config::get_codex_auth_path()).unwrap();
            assert_eq!(auth["tokens"]["access_token"], "new-access", "{exit_state}");
        }
    }

    #[test]
    #[serial]
    fn codex_auth_center_removal_waits_for_provider_switch_lock() {
        with_test_home(|state, _| {
            crate::settings::reload_settings().expect("reload settings");
            tauri::async_runtime::block_on(async {
                state
                    .codex_oauth_manager
                    .add_test_account_with_user_identity(
                        "acct-managed",
                        "managed-access",
                        "managed-user",
                    )
                    .await
                    .expect("seed managed account");
            });

            let switch_guard = tauri::async_runtime::block_on(
                state
                    .proxy_service
                    .lock_switch_for_app(AppType::Codex.as_str()),
            );
            let (started_tx, started_rx) = std::sync::mpsc::channel();
            let (done_tx, done_rx) = std::sync::mpsc::channel();
            std::thread::scope(|scope| {
                scope.spawn(|| {
                    started_tx.send(()).expect("signal removal start");
                    let result = tauri::async_runtime::block_on(
                        crate::commands::remove_codex_oauth_account_with_switch_lock(
                            state,
                            "acct-managed",
                        ),
                    );
                    done_tx.send(result).expect("send removal result");
                });
                started_rx.recv().expect("wait for removal task");
                assert!(
                    done_rx
                        .recv_timeout(std::time::Duration::from_millis(100))
                        .is_err(),
                    "Auth Center removal must wait while a provider transaction owns the Codex lock"
                );
                drop(switch_guard);
                done_rx
                    .recv_timeout(std::time::Duration::from_secs(5))
                    .expect("removal should finish after lock release")
                    .expect("remove managed account");
            });
            assert!(
                tauri::async_runtime::block_on(state.codex_oauth_manager.list_accounts())
                    .is_empty()
            );
        });
    }

    #[test]
    #[serial]
    fn codex_auth_center_logout_waits_for_provider_switch_lock() {
        with_test_home(|state, _| {
            crate::settings::reload_settings().expect("reload settings");
            tauri::async_runtime::block_on(async {
                state
                    .codex_oauth_manager
                    .add_test_account_with_user_identity(
                        "acct-managed",
                        "managed-access",
                        "managed-user",
                    )
                    .await
                    .expect("seed managed account");
            });
            let provider = managed_codex_provider("managed-logout-lock", "acct-managed");
            state
                .db
                .save_provider(AppType::Codex.as_str(), &provider)
                .expect("save managed provider");
            ProviderService::switch(state, AppType::Codex, &provider.id)
                .expect("activate managed provider");
            assert!(crate::codex_config::get_codex_auth_path().exists());
            assert!(crate::codex_config::codex_managed_oauth_live_auth_marker_exists());

            let switch_guard = tauri::async_runtime::block_on(
                state
                    .proxy_service
                    .lock_switch_for_app(AppType::Codex.as_str()),
            );
            let (started_tx, started_rx) = std::sync::mpsc::channel();
            let (done_tx, done_rx) = std::sync::mpsc::channel();
            std::thread::scope(|scope| {
                scope.spawn(|| {
                    started_tx.send(()).expect("signal logout start");
                    let result = tauri::async_runtime::block_on(
                        crate::commands::logout_codex_oauth_with_switch_lock(state),
                    );
                    done_tx.send(result).expect("send logout result");
                });
                started_rx.recv().expect("wait for logout task");
                assert!(
                    done_rx
                        .recv_timeout(std::time::Duration::from_millis(100))
                        .is_err(),
                    "Auth Center logout must wait while a provider transaction owns the Codex lock"
                );
                drop(switch_guard);
                done_rx
                    .recv_timeout(std::time::Duration::from_secs(5))
                    .expect("logout should finish after lock release")
                    .expect("logout managed accounts");
            });

            assert!(
                tauri::async_runtime::block_on(state.codex_oauth_manager.list_accounts())
                    .is_empty()
            );
            assert!(
                !crate::codex_config::get_codex_auth_path().exists(),
                "logout must clear the active managed live auth"
            );
            assert!(
                !crate::codex_config::codex_managed_oauth_live_auth_marker_exists(),
                "logout must clear the managed live auth marker"
            );
        });
    }

    #[test]
    #[serial]
    fn switch_to_managed_codex_official_with_unresolvable_account_keeps_current_unchanged() {
        with_test_home(|state, _| {
            crate::settings::reload_settings().expect("reload settings");

            // 基线：一个普通第三方 provider，可正常切换，作为初始 current。
            // config 必须带自定义 provider 表：config-only 切换要求 key 有
            // provider 级落点。
            let mut baseline = Provider::with_id(
                "baseline".to_string(),
                "Baseline".to_string(),
                json!({
                    "auth": { "OPENAI_API_KEY": "sk-baseline" },
                    "config": "model_provider = \"baseline\"\n[model_providers.baseline]\nbase_url = \"https://baseline.example/v1\"\n"
                }),
                None,
            );
            baseline.category = Some("custom".to_string());

            // 托管 official provider，绑定一个 manager 中不存在的账号：切换预检
            // 取 token 必然失败。
            let mut managed = Provider::with_id(
                "managed-official".to_string(),
                "Managed Official".to_string(),
                json!({ "auth": {}, "config": "" }),
                None,
            );
            managed.category = Some("official".to_string());
            managed.meta = Some(ProviderMeta {
                auth_binding: Some(AuthBinding {
                    source: AuthBindingSource::ManagedAccount,
                    auth_provider: Some("codex_oauth".to_string()),
                    account_id: Some("acct-missing".to_string()),
                }),
                ..Default::default()
            });

            state
                .db
                .save_provider(AppType::Codex.as_str(), &baseline)
                .expect("save baseline");
            state
                .db
                .save_provider(AppType::Codex.as_str(), &managed)
                .expect("save managed");

            ProviderService::switch(state, AppType::Codex, "baseline").expect("switch to baseline");

            // 切到绑定了不存在账号的托管 provider：预检失败 → 返回 Err。
            let result = ProviderService::switch(state, AppType::Codex, "managed-official");
            assert!(
                result.is_err(),
                "switch must fail when the managed OAuth token cannot be resolved"
            );

            // current 必须仍是 baseline：预检在提交 current 之前失败，不留下
            // 「DB/UI 指向新 provider、但 live 仍是旧 provider」的不一致状态。
            let current =
                crate::settings::get_effective_current_provider(&state.db, &AppType::Codex)
                    .expect("read current");
            assert_eq!(
                current.as_deref(),
                Some("baseline"),
                "a failed managed switch must not move current off the previous provider"
            );
        });
    }

    #[test]
    fn native_opencode_common_config_excludes_provider_credentials() {
        let config = json!({
            "settings": {"baseURL": "https://native.example", "apiKey": "test", "timeout": 1000},
            "body": {"metadata": {"keep": true}}
        });
        let common: Value = serde_json::from_str(
            &ProviderService::extract_opencode_common_config(&config).unwrap(),
        )
        .unwrap();
        assert_eq!(
            common,
            json!({"settings": {"timeout": 1000}, "body": {"metadata": {"keep": true}}})
        );
    }

    #[test]
    #[serial]
    fn native_opencode_provider_keeps_source_format_through_database_and_sync() {
        use crate::provider::OpenCodeConfigFormat;
        for native in [
            json!({}),
            json!({"models": {"alias": {"modelID": "upstream", "limit": {"input": 10000}}}}),
            json!({
                "package": "@opencode/ai/providers/anthropic",
                "settings": {"baseURL": "https://native.example", "apiKey": "test"},
                "headers": {"X-Tenant": "example"},
                "body": {"metadata": {"keep": true}},
                "models": {"model": {"capabilities": {"tools": true, "input": ["text"], "output": ["text"]}, "variants": [
                    {"id": "low", "settings": {"reasoningEffort": "low"}},
                    {"id": "high", "settings": {"reasoningEffort": "high"}}
                ]}}
            }),
        ] {
            with_test_home(|state, _| {
                // Also exercise upgrading an existing DB entry with no source metadata.
                let existing = Provider::with_id(
                    "anthropic".into(),
                    "Existing name".into(),
                    native.clone(),
                    None,
                );
                state.db.save_provider("opencode", &existing).unwrap();
                crate::opencode_config::set_provider_with_format(
                    "anthropic",
                    native.clone(),
                    OpenCodeConfigFormat::V2,
                )
                .unwrap();
                assert_eq!(import_opencode_providers_from_live(state).unwrap(), 1);
                assert_eq!(import_opencode_providers_from_live(state).unwrap(), 0);
                let mut saved = state
                    .db
                    .get_provider_by_id("anthropic", "opencode")
                    .unwrap()
                    .unwrap();
                assert_eq!(saved.name, "Existing name");
                assert_eq!(saved.settings_config, native);
                assert_eq!(
                    saved.meta.as_ref().unwrap().opencode_config_format,
                    Some(OpenCodeConfigFormat::V2)
                );

                // Source metadata must survive removal and re-enabling even for {}.
                crate::opencode_config::remove_provider("anthropic").unwrap();
                live::write_live_snapshot(&AppType::OpenCode, &saved).unwrap();
                assert_eq!(
                    crate::opencode_config::read_opencode_config().unwrap()["providers"]
                        ["anthropic"],
                    native
                );
                saved.settings_config["name"] = json!("Edited native provider");
                let expected = saved.settings_config.clone();
                ProviderService::update(state, AppType::OpenCode, None, saved).unwrap();
                let config = crate::opencode_config::read_opencode_config().unwrap();
                assert_eq!(config["providers"]["anthropic"], expected);
                assert!(config.get("provider").is_none());

                let mut full = Provider::with_id(
                    "anthropic".into(),
                    "Full config".into(),
                    json!({"providers": {"anthropic": expected}}),
                    None,
                );
                live::write_live_snapshot(&AppType::OpenCode, &full).unwrap();
                let before =
                    std::fs::read(crate::opencode_config::get_opencode_config_path().unwrap())
                        .unwrap();
                full.settings_config = json!({"providers": {"other": {}}});
                assert!(live::write_live_snapshot(&AppType::OpenCode, &full).is_err());
                assert_eq!(
                    std::fs::read(crate::opencode_config::get_opencode_config_path().unwrap())
                        .unwrap(),
                    before
                );
            });
        }
    }

    #[test]
    #[serial]
    fn malformed_native_opencode_provider_allows_legacy_import_update_and_sync() {
        use crate::provider::OpenCodeConfigFormat;
        for native in [
            json!({"package": false}),
            json!({"models": {"m": {"variants": {}}}}),
            json!({"settings": {"timeout": "1000"}}),
            json!({"models": {"m": {"limit": {"input": "1000"}}}}),
            json!({"models": {"m": {"capabilities": {"tools": true}}}}),
        ] {
            for existing in [false, true] {
                with_test_home(|state, _| {
                    let provider = opencode_provider("shared");
                    if existing {
                        state.db.save_provider("opencode", &provider).unwrap();
                    }
                    let mut expected = json!({
                        "provider": {"shared": provider.settings_config},
                        "providers": {"shared": native},
                        "model": "shared/gpt-4o"
                    });
                    write_json_file(
                        &crate::opencode_config::get_opencode_config_path().unwrap(),
                        &expected,
                    )
                    .unwrap();
                    import_opencode_providers_from_live(state).unwrap();
                    let mut saved = state
                        .db
                        .get_provider_by_id("shared", "opencode")
                        .unwrap()
                        .unwrap();
                    assert_eq!(saved.settings_config, provider.settings_config, "{native}");
                    assert_ne!(
                        saved
                            .meta
                            .as_ref()
                            .and_then(|meta| meta.opencode_config_format),
                        Some(OpenCodeConfigFormat::V2)
                    );
                    assert_eq!(import_opencode_providers_from_live(state).unwrap(), 0);

                    saved.settings_config["options"]["apiKey"] = json!("fake-new");
                    ProviderService::update(state, AppType::OpenCode, None, saved.clone()).unwrap();
                    expected["provider"]["shared"] = saved.settings_config.clone();
                    assert_eq!(
                        crate::opencode_config::read_opencode_config().unwrap(),
                        expected
                    );
                    assert_eq!(
                        state
                            .db
                            .get_provider_by_id("shared", "opencode")
                            .unwrap()
                            .unwrap()
                            .settings_config,
                        saved.settings_config
                    );

                    saved.settings_config["options"]["apiKey"] = json!("fake-synced");
                    state.db.save_provider("opencode", &saved).unwrap();
                    ProviderService::sync_current_provider_for_app(state, AppType::OpenCode)
                        .unwrap();
                    expected["provider"]["shared"] = saved.settings_config;
                    assert_eq!(
                        crate::opencode_config::read_opencode_config().unwrap(),
                        expected
                    );
                    assert_eq!(import_opencode_providers_from_live(state).unwrap(), 0);
                });
            }
        }
    }

    #[test]
    #[serial]
    fn invalid_native_opencode_edit_is_rejected_before_saving() {
        use crate::provider::OpenCodeConfigFormat;
        with_test_home(|state, _| {
            let native = json!({"settings": {"baseURL": "https://a.example"}});
            crate::opencode_config::set_provider_with_format(
                "native",
                native.clone(),
                OpenCodeConfigFormat::V2,
            )
            .unwrap();
            import_opencode_providers_from_live(state).unwrap();
            let mut edited = state
                .db
                .get_provider_by_id("native", "opencode")
                .unwrap()
                .unwrap();
            // OpenCode requires tools, input and output together.
            edited.settings_config = json!({"settings": {"baseURL": "https://b.example"}, "models": {"m": {"capabilities": {"tools": true}}}});

            let err = ProviderService::update(state, AppType::OpenCode, None, edited.clone())
                .unwrap_err()
                .to_string();
            assert!(err.contains("models.m.capabilities"), "{err}");
            edited.id = "added".into();
            assert!(ProviderService::add(state, AppType::OpenCode, edited, true).is_err());

            let saved = state
                .db
                .get_provider_by_id("native", "opencode")
                .unwrap()
                .unwrap();
            assert_eq!(saved.settings_config, native);
            assert!(state
                .db
                .get_provider_by_id("added", "opencode")
                .unwrap()
                .is_none());
            assert_eq!(
                crate::opencode_config::read_opencode_config().unwrap()["providers"]["native"],
                native
            );
        });
    }

    #[test]
    #[serial]
    fn full_opencode_config_writes_legacy_entry_behind_invalid_native() {
        with_test_home(|_, _| {
            let legacy =
                json!({"npm": "@ai-sdk/openai", "options": {"apiKey": "v1"}, "models": {"m": {}}});
            let full = Provider::with_id(
                "shared".into(),
                "Full config".into(),
                json!({"provider": {"shared": legacy}, "providers": {"shared": {"package": false}}}),
                None,
            );
            live::write_live_snapshot(&AppType::OpenCode, &full).unwrap();
            let config = crate::opencode_config::read_opencode_config().unwrap();
            assert_eq!(config["provider"]["shared"], legacy);
            assert!(config.get("providers").is_none());
        });
    }

    #[test]
    #[serial]
    fn opencode_provider_roundtrip_preserves_fields_on_import_and_update() {
        with_test_home(|state, _| {
            let mut provider = opencode_provider("roundtrip-opencode");
            provider.settings_config["api"] = json!("https://api.example.com/v1");
            provider.settings_config["env"] = json!(["EXAMPLE_API_KEY"]);
            provider.settings_config["models"]["gpt-4o"]["limit"] = json!({
                "input": 120000,
                "context": 128000,
                "output": 8000
            });
            crate::opencode_config::set_provider(&provider.id, provider.settings_config.clone())
                .expect("seed opencode live provider");

            assert_eq!(import_opencode_providers_from_live(state).unwrap(), 1);
            let mut saved = state
                .db
                .get_provider_by_id(&provider.id, AppType::OpenCode.as_str())
                .unwrap()
                .expect("imported provider");
            assert_eq!(saved.settings_config, provider.settings_config);
            assert_eq!(import_opencode_providers_from_live(state).unwrap(), 0);

            saved.settings_config["options"]["apiKey"] = json!("updated-key");
            let expected = saved.settings_config.clone();
            ProviderService::update(state, AppType::OpenCode, None, saved)
                .expect("update imported provider");
            let live = crate::opencode_config::get_providers().unwrap();
            assert_eq!(live[&provider.id], expected);
        });
    }

    #[test]
    #[serial]
    fn opencode_provider_roundtrip_preserves_fields_on_live_write() {
        with_test_home(|_, _| {
            let mut provider = opencode_provider("write-opencode");
            provider.settings_config["api"] = json!("https://api.example.com/v1");
            provider.settings_config["env"] = json!(["EXAMPLE_API_KEY"]);
            provider.settings_config["models"]["gpt-4o"]["limit"] = json!({
                "input": 120000,
                "context": 128000,
                "output": 8000
            });
            let expected = provider.settings_config.clone();

            // Exercise the full-config fragment extraction as well as the writer.
            provider.settings_config = json!({
                "$schema": "https://opencode.ai/config.json",
                "provider": { provider.id.clone(): expected.clone() }
            });
            live::write_live_snapshot(&AppType::OpenCode, &provider)
                .expect("write opencode provider");

            let live = crate::opencode_config::get_providers().unwrap();
            assert_eq!(live[&provider.id], expected);
        });
    }

    #[test]
    #[serial]
    fn import_opencode_providers_from_live_marks_provider_as_live_managed() {
        with_test_home(|state, _| {
            let provider = opencode_provider("imported-opencode");
            crate::opencode_config::set_provider(&provider.id, provider.settings_config.clone())
                .expect("seed opencode live provider");

            let imported = import_opencode_providers_from_live(state)
                .expect("import opencode providers from live");
            assert_eq!(imported, 1);

            let saved = state
                .db
                .get_provider_by_id(&provider.id, AppType::OpenCode.as_str())
                .expect("query imported opencode provider")
                .expect("imported opencode provider should exist");
            assert_eq!(
                saved
                    .meta
                    .as_ref()
                    .and_then(|meta| meta.live_config_managed),
                Some(true),
                "providers imported from live should be treated as live-managed"
            );
        });
    }

    #[test]
    #[serial]
    fn import_opencode_providers_from_live_updates_existing_provider_from_live() {
        with_test_home(|state, _| {
            let provider = opencode_provider("existing-opencode");
            state
                .db
                .save_provider(AppType::OpenCode.as_str(), &provider)
                .expect("seed existing opencode provider");

            let mut live_settings = provider.settings_config.clone();
            live_settings.as_object_mut().unwrap().remove("name");
            live_settings["npm"] = Value::String("@ai-sdk/anthropic".to_string());
            live_settings["models"]["gpt-4o"]["name"] = Value::String("Claude Sonnet".to_string());
            crate::opencode_config::set_provider(&provider.id, live_settings)
                .expect("seed edited live opencode provider");

            let updated = import_opencode_providers_from_live(state)
                .expect("import opencode providers from live");
            assert_eq!(updated, 1);

            let saved = state
                .db
                .get_provider_by_id(&provider.id, AppType::OpenCode.as_str())
                .expect("query updated opencode provider")
                .expect("opencode provider should exist");
            assert_eq!(saved.name, provider.name);
            assert_eq!(saved.settings_config["npm"], json!("@ai-sdk/anthropic"));
            assert_eq!(
                saved.settings_config["models"]["gpt-4o"]["name"],
                json!("Claude Sonnet")
            );
        });
    }
    #[test]
    #[serial]
    fn import_openclaw_providers_from_live_marks_provider_as_live_managed() {
        with_test_home(|state, _| {
            let mut provider = openclaw_provider("imported-openclaw");
            provider.settings_config["models"] = json!([
                {
                    "id": "claude-sonnet-4",
                    "name": "Claude Sonnet 4"
                }
            ]);
            crate::openclaw_config::set_provider(&provider.id, provider.settings_config.clone())
                .expect("seed openclaw live provider");

            let imported = import_openclaw_providers_from_live(state)
                .expect("import openclaw providers from live");
            assert_eq!(imported, 1);

            let saved = state
                .db
                .get_provider_by_id(&provider.id, AppType::OpenClaw.as_str())
                .expect("query imported openclaw provider")
                .expect("imported openclaw provider should exist");
            assert_eq!(
                saved
                    .meta
                    .as_ref()
                    .and_then(|meta| meta.live_config_managed),
                Some(true),
                "providers imported from live should be treated as live-managed"
            );
        });
    }

    #[test]
    #[serial]
    fn import_openclaw_providers_from_live_updates_existing_provider_from_live() {
        with_test_home(|state, _| {
            let mut provider = openclaw_provider("existing-openclaw");
            provider.settings_config["models"] = json!([
                {
                    "id": "claude-sonnet-4",
                    "name": "Claude Sonnet 4"
                }
            ]);
            state
                .db
                .save_provider(AppType::OpenClaw.as_str(), &provider)
                .expect("seed existing openclaw provider");

            let mut live_settings = provider.settings_config.clone();
            live_settings["baseUrl"] = Value::String("https://api.example.com/v1".to_string());
            live_settings["models"][0]["name"] = Value::String("Claude Sonnet 4.1".to_string());
            crate::openclaw_config::set_provider(&provider.id, live_settings)
                .expect("seed edited live openclaw provider");

            let updated = import_openclaw_providers_from_live(state)
                .expect("import openclaw providers from live");
            assert_eq!(updated, 1);

            let saved = state
                .db
                .get_provider_by_id(&provider.id, AppType::OpenClaw.as_str())
                .expect("query updated openclaw provider")
                .expect("openclaw provider should exist");
            assert_eq!(saved.name, provider.name);
            assert_eq!(
                saved.settings_config["baseUrl"],
                json!("https://api.example.com/v1")
            );
            assert_eq!(
                saved.settings_config["models"][0]["name"],
                json!("Claude Sonnet 4.1")
            );
        });
    }

    #[test]
    #[serial]
    fn import_hermes_providers_from_live_updates_existing_provider_from_live() {
        with_test_home(|state, _| {
            let provider = hermes_provider("existing-hermes");
            state
                .db
                .save_provider(AppType::Hermes.as_str(), &provider)
                .expect("seed existing hermes provider");

            let mut live_settings = provider.settings_config.clone();
            live_settings["base_url"] = Value::String("https://api.hermes.example/v1".to_string());
            live_settings["models"]["gpt-4o"]["name"] = Value::String("GPT-4o Updated".to_string());
            crate::hermes_config::set_provider(&provider.id, live_settings)
                .expect("seed edited live hermes provider");

            let updated = import_hermes_providers_from_live(state)
                .expect("import hermes providers from live");
            assert_eq!(updated, 1);

            let saved = state
                .db
                .get_provider_by_id(&provider.id, AppType::Hermes.as_str())
                .expect("query updated hermes provider")
                .expect("hermes provider should exist");
            assert_eq!(saved.name, provider.name);
            assert_eq!(
                saved.settings_config["base_url"],
                json!("https://api.hermes.example/v1")
            );
            // models are denormalized from YAML dict to UI-friendly array by
            // get_providers(), so access by index rather than dict key
            assert_eq!(
                saved.settings_config["models"][0]["name"],
                json!("GPT-4o Updated")
            );
            assert_eq!(saved.settings_config["models"][0]["id"], json!("gpt-4o"));
        });
    }

    #[test]
    #[serial]
    fn legacy_additive_provider_still_errors_on_live_config_parse_failure() {
        with_test_home(|state, home| {
            let provider = openclaw_provider("legacy-provider");
            state
                .db
                .save_provider(AppType::OpenClaw.as_str(), &provider)
                .expect("seed legacy provider without live_config_managed marker");

            let openclaw_dir = home.join(".openclaw");
            fs::create_dir_all(&openclaw_dir).expect("create openclaw dir");
            fs::write(openclaw_dir.join("openclaw.json"), "{ invalid json5")
                .expect("write malformed config");

            let mut updated = provider.clone();
            updated.name = "Legacy Edited".to_string();

            let err = ProviderService::update(state, AppType::OpenClaw, None, updated)
                .expect_err("legacy providers should still surface live parse errors");
            assert!(
                err.to_string().contains("Failed to parse OpenClaw config"),
                "expected parse error, got {err:?}"
            );
        });
    }

    #[test]
    #[serial]
    fn update_persists_non_current_omo_variants_in_database() {
        with_test_home(|state, _| {
            for category in ["omo", "omo-slim"] {
                let provider = opencode_omo_provider(&format!("{category}-provider"), category);
                state
                    .db
                    .save_provider(AppType::OpenCode.as_str(), &provider)
                    .unwrap_or_else(|err| panic!("seed {category} provider: {err}"));

                let mut updated = provider.clone();
                updated.name = format!("Updated {category}");
                updated.settings_config["agents"]["writer"]["model"] =
                    Value::String(format!("{category}-next-model"));

                ProviderService::update(state, AppType::OpenCode, None, updated)
                    .unwrap_or_else(|err| panic!("update {category} provider: {err}"));

                let saved = state
                    .db
                    .get_provider_by_id(&provider.id, AppType::OpenCode.as_str())
                    .unwrap_or_else(|err| panic!("query updated {category} provider: {err}"))
                    .unwrap_or_else(|| panic!("{category} provider should exist"));

                assert_eq!(saved.name, format!("Updated {category}"));
                assert_eq!(
                    saved.settings_config["agents"]["writer"]["model"],
                    Value::String(format!("{category}-next-model")),
                    "{category} updates should persist in the database"
                );
            }
        });
    }

    #[test]
    #[serial]
    fn update_current_omo_variant_rewrites_config_from_saved_provider() {
        with_test_home(|state, home| {
            for category in ["omo", "omo-slim"] {
                let provider = opencode_omo_provider(&format!("{category}-current"), category);
                state
                    .db
                    .save_provider(AppType::OpenCode.as_str(), &provider)
                    .unwrap_or_else(|err| panic!("seed current {category} provider: {err}"));
                state
                    .db
                    .set_omo_provider_current(AppType::OpenCode.as_str(), &provider.id, category)
                    .unwrap_or_else(|err| panic!("set current {category} provider: {err}"));

                let mut updated = provider.clone();
                updated.name = format!("Current {category} updated");
                updated.settings_config["agents"]["writer"]["model"] =
                    Value::String(format!("{category}-saved-model"));
                updated.settings_config["otherFields"]["theme"] =
                    Value::String(format!("{category}-light"));

                ProviderService::update(state, AppType::OpenCode, None, updated)
                    .unwrap_or_else(|err| panic!("update current {category} provider: {err}"));

                let saved = state
                    .db
                    .get_provider_by_id(&provider.id, AppType::OpenCode.as_str())
                    .unwrap_or_else(|err| panic!("query current {category} provider: {err}"))
                    .unwrap_or_else(|| panic!("current {category} provider should exist"));
                assert_eq!(saved.name, format!("Current {category} updated"));

                let written = fs::read_to_string(omo_config_path(home, category))
                    .unwrap_or_else(|err| panic!("read written {category} config: {err}"));
                let written_json: Value = serde_json::from_str(&written)
                    .unwrap_or_else(|err| panic!("parse written {category} config: {err}"));

                assert_eq!(
                    written_json["agents"]["writer"]["model"],
                    Value::String(format!("{category}-saved-model")),
                    "{category} config should be written from the saved provider state"
                );
                assert_eq!(
                    written_json["theme"],
                    Value::String(format!("{category}-light")),
                    "{category} top-level config should reflect updated otherFields"
                );
            }
        });
    }

    #[test]
    #[serial]
    fn update_current_omo_variant_does_not_persist_database_when_file_write_fails() {
        with_test_home(|state, home| {
            let provider = opencode_omo_provider("omo-current", "omo");
            state
                .db
                .save_provider(AppType::OpenCode.as_str(), &provider)
                .unwrap_or_else(|err| panic!("seed current omo provider: {err}"));
            state
                .db
                .set_omo_provider_current(AppType::OpenCode.as_str(), &provider.id, "omo")
                .unwrap_or_else(|err| panic!("set current omo provider: {err}"));

            let config_dir = home.join(".config").join("opencode");
            fs::create_dir_all(config_dir.parent().expect("config dir parent"))
                .expect("create .config dir");
            fs::write(&config_dir, "not a directory").expect("block opencode config dir");

            let mut updated = provider.clone();
            updated.name = "Current omo updated".to_string();
            updated.settings_config["agents"]["writer"]["model"] =
                Value::String("omo-saved-model".to_string());

            ProviderService::update(state, AppType::OpenCode, None, updated)
                .expect_err("update should fail when current omo file write fails");

            let saved = state
                .db
                .get_provider_by_id(&provider.id, AppType::OpenCode.as_str())
                .unwrap_or_else(|err| panic!("query current omo provider: {err}"))
                .unwrap_or_else(|| panic!("current omo provider should exist"));

            assert_eq!(saved.name, provider.name);
            assert_eq!(
                saved.settings_config["agents"]["writer"]["model"],
                provider.settings_config["agents"]["writer"]["model"],
                "database should remain unchanged when file write fails"
            );
        });
    }

    #[test]
    #[serial]
    fn update_current_omo_variant_rolls_back_file_when_plugin_sync_fails() {
        with_test_home(|state, home| {
            let provider = opencode_omo_provider("omo-current", "omo");
            state
                .db
                .save_provider(AppType::OpenCode.as_str(), &provider)
                .unwrap_or_else(|err| panic!("seed current omo provider: {err}"));
            state
                .db
                .set_omo_provider_current(AppType::OpenCode.as_str(), &provider.id, "omo")
                .unwrap_or_else(|err| panic!("set current omo provider: {err}"));

            let config_path = omo_config_path(home, "omo");
            fs::create_dir_all(config_path.parent().expect("omo config parent"))
                .expect("create omo config dir");
            let previous_content = serde_json::to_string_pretty(&json!({
                "theme": "legacy-live-theme",
                "agents": {
                    "writer": {
                        "model": "legacy-live-model"
                    }
                },
                "categories": {
                    "default": ["writer"]
                }
            }))
            .expect("serialize previous config");
            fs::write(&config_path, &previous_content).expect("seed previous omo config");

            let opencode_config_path = home.join(".config").join("opencode").join("opencode.json");
            fs::write(&opencode_config_path, "{ invalid json").expect("seed malformed opencode");

            let mut updated = provider.clone();
            updated.name = "Current omo updated".to_string();
            updated.settings_config["agents"]["writer"]["model"] =
                Value::String("omo-saved-model".to_string());
            updated.settings_config["otherFields"]["theme"] =
                Value::String("omo-light".to_string());

            ProviderService::update(state, AppType::OpenCode, None, updated)
                .expect_err("update should fail when plugin sync fails");

            let saved = state
                .db
                .get_provider_by_id(&provider.id, AppType::OpenCode.as_str())
                .unwrap_or_else(|err| panic!("query current omo provider: {err}"))
                .unwrap_or_else(|| panic!("current omo provider should exist"));

            assert_eq!(saved.name, provider.name);
            assert_eq!(
                saved.settings_config["agents"]["writer"]["model"],
                provider.settings_config["agents"]["writer"]["model"],
                "database should remain unchanged when plugin sync fails"
            );

            let written =
                fs::read_to_string(&config_path).expect("read rolled back omo config content");
            assert_eq!(
                written, previous_content,
                "OMO config should roll back to its previous on-disk contents"
            );
        });
    }

    #[test]
    #[serial]
    fn sync_universal_to_apps_preserves_child_metadata() {
        with_test_home(|state, _home| {
            let mut universal = UniversalProvider::new(
                "metadata".into(),
                "Original".into(),
                "custom".into(),
                "https://old.example".into(),
                "old-key".into(),
            );
            universal.apps.claude = true;
            universal.apps.codex = true;
            universal.apps.gemini = true;
            universal.meta = Some(
                serde_json::from_value(json!({
                    "usage_script": {"enabled": false, "language": "javascript", "code": "parent"}
                }))
                .unwrap(),
            );
            state.db.save_universal_provider(&universal).unwrap();
            ProviderService::sync_universal_to_apps(state, &universal.id).unwrap();

            let mut expected = Vec::new();
            for (index, app) in ["claude", "codex", "gemini"].iter().enumerate() {
                let id = format!("universal-{app}-metadata");
                let mut child = state.db.get_provider_by_id(&id, app).unwrap().unwrap();
                assert_eq!(
                    serde_json::to_value(&child.meta).unwrap(),
                    serde_json::to_value(&universal.meta).unwrap()
                );
                child.meta = Some(
                    serde_json::from_value(json!({
                        "usage_script": {"enabled": true, "language": "javascript", "code": app,
                            "apiKey": "usage-only-key", "autoQueryInterval": 15},
                        "commonConfigEnabled": false,
                        "endpointAutoSelect": true
                    }))
                    .unwrap(),
                );
                child.created_at = Some(123 + index as i64);
                child.sort_index = Some(10 + index);
                child.settings_config["local_setting"] = json!(app);
                state.db.save_provider(app, &child).unwrap();
                state
                    .db
                    .add_custom_endpoint(app, &id, "https://extra.example")
                    .unwrap();
                expected.push(child);
            }

            universal.name = "Updated".into();
            universal.base_url = "https://new.example".into();
            universal.api_key = "new-key".into();
            universal.notes = Some("shared note".into());
            universal.models = serde_json::from_value(json!({
                "claude": {"model": "claude-new"},
                "codex": {"model": "codex-new", "reasoningEffort": "low"},
                "gemini": {"model": "gemini-new"}
            }))
            .unwrap();
            // Both absent and present parent metadata must not overwrite child settings.
            for parent_meta in [None, universal.meta.clone()] {
                universal.meta = parent_meta;
                state.db.save_universal_provider(&universal).unwrap();
                ProviderService::sync_universal_to_apps(state, &universal.id).unwrap();
                for (app, before) in ["claude", "codex", "gemini"].iter().zip(&expected) {
                    let after = state
                        .db
                        .get_provider_by_id(&before.id, app)
                        .unwrap()
                        .unwrap();
                    assert_eq!(
                        serde_json::to_value(&after.meta).unwrap(),
                        serde_json::to_value(&before.meta).unwrap(),
                        "{app}"
                    );
                    assert_eq!(after.created_at, before.created_at, "{app}");
                    assert_eq!(after.sort_index, before.sort_index, "{app}");
                    assert_eq!(after.name, "Updated");
                    assert_eq!(after.notes.as_deref(), Some("shared note"));
                    assert_eq!(after.settings_config["local_setting"], json!(app));
                    let generated = match *app {
                        "claude" => universal.to_claude_provider(),
                        "codex" => universal.to_codex_provider(),
                        _ => universal.to_gemini_provider(),
                    }
                    .unwrap();
                    let mut expected_settings = before.settings_config.clone();
                    ProviderService::merge_json(&mut expected_settings, &generated.settings_config);
                    assert_eq!(after.settings_config, expected_settings);
                    assert_eq!(
                        state.db.get_all_providers(app).unwrap()[&before.id]
                            .meta
                            .as_ref()
                            .unwrap()
                            .custom_endpoints
                            .len(),
                        1
                    );
                }
            }
        });
    }

    #[test]
    #[serial]
    fn sync_universal_to_apps_reprojects_current_child_to_live() {
        with_test_home(|state, _home| {
            let mut universal = UniversalProvider::new(
                "shared".to_string(),
                "Shared Relay".to_string(),
                "custom".to_string(),
                "https://api.new.example".to_string(),
                "new-key".to_string(),
            );
            universal.apps.claude = true;
            universal.models.claude = Some(ClaudeModelConfig {
                model: Some("claude-sonnet-4".to_string()),
                ..Default::default()
            });
            state
                .db
                .save_universal_provider(&universal)
                .expect("save universal provider");

            let child = universal
                .to_claude_provider()
                .expect("claude child provider");
            state
                .db
                .save_provider("claude", &child)
                .expect("seed child provider");
            state
                .db
                .set_current_provider("claude", &child.id)
                .expect("set current child");
            crate::settings::set_current_provider(&AppType::Claude, Some(&child.id))
                .expect("set local current child");

            let mut old_live = child.settings_config.clone();
            old_live["env"]["ANTHROPIC_BASE_URL"] =
                Value::String("https://api.old.example".to_string());
            write_json_file(&get_claude_settings_path(), &old_live).expect("seed old live");

            ProviderService::sync_universal_to_apps(state, "shared")
                .expect("sync universal provider");

            let live: Value = read_json_file(&get_claude_settings_path()).expect("read live");
            assert_eq!(
                live["env"]["ANTHROPIC_BASE_URL"].as_str(),
                Some("https://api.new.example")
            );
            assert_eq!(
                live["env"]["ANTHROPIC_AUTH_TOKEN"].as_str(),
                Some("new-key")
            );
        });
    }

    #[test]
    #[serial]
    fn add_first_managed_codex_with_missing_account_leaves_no_provider_or_live_state() {
        with_test_home(|state, _| {
            crate::settings::reload_settings().expect("reload settings");
            let provider = managed_codex_provider("managed-missing", "acct-missing");
            let live_before = crate::codex_config::CodexLiveStateSnapshot::capture()
                .expect("capture empty Codex live state");

            ProviderService::add(state, AppType::Codex, provider.clone(), false)
                .expect_err("missing managed account should fail before add commits");

            assert!(
                state
                    .db
                    .get_provider_by_id(&provider.id, AppType::Codex.as_str())
                    .expect("query failed managed add")
                    .is_none(),
                "failed preflight must not leave an orphan provider row"
            );
            assert_eq!(
                state
                    .db
                    .get_current_provider(AppType::Codex.as_str())
                    .expect("read current after failed add"),
                None
            );
            assert_eq!(
                crate::codex_config::CodexLiveStateSnapshot::capture()
                    .expect("capture Codex live after failed add"),
                live_before,
                "failed preflight must not mutate Codex live files"
            );
        });
    }

    #[test]
    #[serial]
    fn add_first_managed_codex_current_failure_rolls_forward_on_recovery() {
        with_test_home(|state, _| {
            crate::settings::reload_settings().expect("reload settings");
            tauri::async_runtime::block_on(async {
                state
                    .codex_oauth_manager
                    .add_test_account_with_user_identity(
                        "acct-managed",
                        "managed-token",
                        "managed-user",
                    )
                    .await
                    .expect("seed managed Codex OAuth account");
            });

            let provider = managed_codex_provider("managed-first", "acct-managed");
            let live_before = crate::codex_config::CodexLiveStateSnapshot::capture()
                .expect("capture empty Codex live state");
            {
                let conn = state.db.conn.lock().expect("lock database");
                conn.execute_batch(
                    "CREATE TRIGGER reject_first_managed_current_update
                     BEFORE UPDATE OF is_current ON providers
                     WHEN NEW.app_type = 'codex'
                       AND NEW.id = 'managed-first'
                       AND NEW.is_current = 1
                     BEGIN
                       SELECT RAISE(ABORT, 'forced first managed Codex current failure');
                     END;",
                )
                .expect("install first-current failure trigger");
            }

            let error = ProviderService::add(state, AppType::Codex, provider.clone(), false)
                .expect_err("DB current failure surfaces");
            assert!(
                error
                    .to_string()
                    .contains("forced first managed Codex current failure"),
                "add should surface the DB current failure, got: {error}"
            );
            // 文件已经发布：行留着，pending 等着补完指针，live 不回滚。
            assert!(state
                .db
                .get_provider_by_id(&provider.id, AppType::Codex.as_str())
                .expect("query provider")
                .is_some());
            assert!(crate::mode::operation::has_pending(AppType::Codex.as_str()));
            assert_ne!(
                crate::codex_config::CodexLiveStateSnapshot::capture().expect("capture live"),
                live_before
            );

            state
                .db
                .conn
                .lock()
                .expect("lock database")
                .execute_batch("DROP TRIGGER reject_first_managed_current_update;")
                .expect("drop trigger");
            crate::mode::operation::recover_on_startup(&state.db);
            assert!(!crate::mode::operation::has_pending(
                AppType::Codex.as_str()
            ));
            assert_eq!(
                state
                    .db
                    .get_current_provider(AppType::Codex.as_str())
                    .expect("read current after recovery")
                    .as_deref(),
                Some(provider.id.as_str())
            );
        });
    }

    #[test]
    #[serial]
    fn missing_codex_account_preserves_native_login_but_corrupt_store_blocks_recovery() {
        for takeover in [false, true] {
            for corrupt in [false, true] {
                with_test_home(|state, _| {
                    crate::settings::reload_settings().unwrap();
                    crate::settings::update_settings(crate::settings::AppSettings {
                        preserve_codex_official_auth_on_switch: true,
                        ..Default::default()
                    })
                    .unwrap();
                    let runtime = tauri::async_runtime::handle();
                    runtime
                        .block_on(
                            state
                                .codex_oauth_manager
                                .add_test_account_with_user_identity("old", "access", "user"),
                        )
                        .unwrap();
                    let current = managed_codex_provider("current", "old");
                    state.db.save_provider("codex", &current).unwrap();
                    ProviderService::switch(state, AppType::Codex, "current").unwrap();
                    let mut auth: Value =
                        read_json_file(&crate::codex_config::get_codex_auth_path()).unwrap();
                    if takeover {
                        runtime.block_on(async {
                            // 端口 0 启动后实际端口会记进库，重开的 AppState 不启动代理
                            // 也能算出契约地址。
                            let mut config = state.db.get_proxy_config().await.unwrap();
                            config.listen_port = 0;
                            state.db.update_proxy_config(config).await.unwrap();
                            crate::mode::controller::enter(state, &AppType::Codex, false)
                                .await
                                .unwrap();
                        });
                    }
                    runtime
                        .block_on(remove_codex_account_leaving_dangling_bindings(state, "old"))
                        .unwrap();
                    // A stale marker must not claim a later native login of the same user.
                    auth["tokens"]["refresh_token"] = json!("native-rotated-token");
                    write_json_file(&crate::codex_config::get_codex_auth_path(), &auth).unwrap();
                    crate::codex_config::record_codex_managed_oauth_live_auth(&auth, "old")
                        .unwrap();
                    if corrupt {
                        fs::write(
                            crate::config::get_app_config_dir().join("codex_oauth_auth.json"),
                            "{broken",
                        )
                        .unwrap();
                    }
                    let restarted = AppState::new(state.db.clone());
                    let target = Provider::with_id(
                        "target".into(),
                        "Third party".into(),
                        codex_settings("https://example.test/v1", "sk-target"),
                        None,
                    );
                    state.db.save_provider("codex", &target).unwrap();
                    let before = crate::codex_config::CodexLiveStateSnapshot::capture().unwrap();
                    let result = ProviderService::switch(&restarted, AppType::Codex, "target");
                    if corrupt {
                        assert!(result.is_err());
                        assert_eq!(
                            crate::codex_config::CodexLiveStateSnapshot::capture().unwrap(),
                            before
                        );
                        assert_eq!(
                            crate::mode::current::provider_for(
                                &state.db,
                                &AppType::Codex,
                                crate::mode::current::Purpose::InUse,
                            )
                            .unwrap()
                            .as_deref(),
                            Some("current")
                        );
                    } else {
                        result.unwrap();
                        assert!(
                            !crate::codex_config::codex_managed_oauth_live_auth_marker_exists(),
                            "missing account must relinquish ownership"
                        );
                    }
                    assert_eq!(
                        read_json_file::<Value>(&crate::codex_config::get_codex_auth_path())
                            .unwrap(),
                        auth
                    );
                });
            }
        }
    }

    #[test]
    #[serial]
    fn managed_codex_switch_db_current_failure_rolls_forward_on_recovery() {
        with_test_home(|state, _| {
            crate::settings::reload_settings().expect("reload settings");
            tauri::async_runtime::block_on(async {
                state
                    .codex_oauth_manager
                    .add_test_account_with_user_identity(
                        "acct-managed-a",
                        "managed-token-a",
                        "user-a",
                    )
                    .await
                    .expect("seed first managed Codex OAuth account");
                state
                    .codex_oauth_manager
                    .add_test_account_with_user_identity(
                        "acct-managed-b",
                        "managed-token-b",
                        "user-b",
                    )
                    .await
                    .expect("seed second managed Codex OAuth account");
            });

            let managed_provider = |id: &str, account_id: &str, model: &str| {
                let mut provider = Provider::with_id(
                    id.to_string(),
                    format!("Managed {id}"),
                    json!({
                        "auth": {},
                        "config": format!("model = \"{model}\"\n"),
                        "modelCatalog": {
                            "models": [{ "model": model }]
                        }
                    }),
                    None,
                );
                provider.category = Some("official".to_string());
                provider.meta = Some(ProviderMeta {
                    auth_binding: Some(AuthBinding {
                        source: AuthBindingSource::ManagedAccount,
                        auth_provider: Some("codex_oauth".to_string()),
                        account_id: Some(account_id.to_string()),
                    }),
                    ..Default::default()
                });
                provider
            };

            let provider_a = managed_provider("managed-a", "acct-managed-a", "gpt-5.4-managed-a");
            let provider_b = managed_provider("managed-b", "acct-managed-b", "gpt-5.4-managed-b");
            state
                .db
                .save_provider(AppType::Codex.as_str(), &provider_a)
                .expect("save first managed provider");
            state
                .db
                .save_provider(AppType::Codex.as_str(), &provider_b)
                .expect("save second managed provider");

            ProviderService::switch(state, AppType::Codex, &provider_a.id)
                .expect("activate first managed provider");
            let auth_before: Value = read_json_file(&crate::codex_config::get_codex_auth_path())
                .expect("read first managed auth");
            assert!(
                crate::codex_config::get_codex_config_path().exists(),
                "baseline must include config.toml"
            );
            assert!(
                crate::codex_config::get_codex_model_catalog_path().exists(),
                "baseline must include the generated model catalog"
            );
            assert!(
                crate::codex_config::codex_auth_matches_recorded_managed_oauth(
                    &auth_before,
                    "acct-managed-a",
                )
                .expect("check first managed auth marker"),
                "baseline must include a marker owned by the first managed account"
            );
            let live_before = crate::codex_config::CodexLiveStateSnapshot::capture()
                .expect("capture auth/config/catalog/marker before failed switch");

            {
                let conn = state.db.conn.lock().expect("lock database");
                conn.execute_batch(
                    "CREATE TRIGGER reject_managed_b_current_update
                     BEFORE UPDATE OF is_current ON providers
                     WHEN NEW.app_type = 'codex'
                       AND NEW.id = 'managed-b'
                       AND NEW.is_current = 1
                     BEGIN
                       SELECT RAISE(ABORT, 'forced managed Codex current failure');
                     END;",
                )
                .expect("install current-provider failure trigger");
            }

            let error = ProviderService::switch(state, AppType::Codex, &provider_b.id)
                .expect_err("DB current failure should abort managed switch");
            assert!(
                error
                    .to_string()
                    .contains("forced managed Codex current failure"),
                "switch should surface the DB commit failure, got: {error}"
            );

            // 文件已经发布，只是指针没落定：不回滚，pending 等着补完。
            assert_ne!(
                crate::codex_config::CodexLiveStateSnapshot::capture().expect("capture live"),
                live_before
            );
            let auth_after: Value = read_json_file(&crate::codex_config::get_codex_auth_path())
                .expect("read managed auth");
            assert!(
                crate::codex_config::codex_auth_matches_recorded_managed_oauth(
                    &auth_after,
                    "acct-managed-b",
                )
                .expect("check managed marker"),
                "auth.json and the marker belong to the target account"
            );
            assert!(crate::mode::operation::has_pending(AppType::Codex.as_str()));

            state
                .db
                .conn
                .lock()
                .expect("lock database")
                .execute_batch("DROP TRIGGER reject_managed_b_current_update;")
                .expect("drop trigger");
            crate::mode::operation::recover_on_startup(&state.db);
            assert!(!crate::mode::operation::has_pending(
                AppType::Codex.as_str()
            ));
            assert_eq!(
                crate::settings::get_current_provider(&AppType::Codex).as_deref(),
                Some(provider_b.id.as_str())
            );
            assert_eq!(
                state
                    .db
                    .get_current_provider(AppType::Codex.as_str())
                    .expect("read DB current after recovery")
                    .as_deref(),
                Some(provider_b.id.as_str())
            );
        });
    }

    #[test]
    #[serial]
    fn managed_codex_update_rechecks_current_after_waiting_for_switch_lock() {
        with_test_home(|state, _| {
            crate::settings::reload_settings().expect("reload settings");
            tauri::async_runtime::block_on(async {
                state
                    .codex_oauth_manager
                    .add_test_account_with_user_identity(
                        "acct-managed-a",
                        "managed-token-a",
                        "user-a",
                    )
                    .await
                    .expect("seed managed account A");
                state
                    .codex_oauth_manager
                    .add_test_account_with_user_identity(
                        "acct-managed-b",
                        "managed-token-b",
                        "user-b",
                    )
                    .await
                    .expect("seed managed account B");
            });

            let mut official = Provider::with_id(
                "managed-official-a".to_string(),
                "OpenAI Official".to_string(),
                json!({ "auth": {}, "config": "model = \"gpt-5.4\"\n" }),
                None,
            );
            official.category = Some("official".to_string());
            official.meta = Some(ProviderMeta {
                auth_binding: Some(AuthBinding {
                    source: AuthBindingSource::ManagedAccount,
                    auth_provider: Some("codex_oauth".to_string()),
                    account_id: Some("acct-managed-a".to_string()),
                }),
                ..Default::default()
            });
            state
                .db
                .save_provider(AppType::Codex.as_str(), &official)
                .expect("save official A");
            state
                .db
                .set_current_provider(AppType::Codex.as_str(), &official.id)
                .expect("set official current");
            crate::settings::set_current_provider(&AppType::Codex, Some(&official.id))
                .expect("set local official current");

            let mut third_party = Provider::with_id(
                "third-party-current".to_string(),
                "Third Party".to_string(),
                json!({
                    "auth": { "OPENAI_API_KEY": "sk-third" },
                    "config": r#"model_provider = "third"
[model_providers.third]
name = "Third"
base_url = "https://third.example/v1"
wire_api = "responses"
"#
                }),
                None,
            );
            third_party.category = Some("custom".to_string());
            state
                .db
                .save_provider(AppType::Codex.as_str(), &third_party)
                .expect("save third party");

            let mut updated = official.clone();
            updated
                .meta
                .as_mut()
                .and_then(|meta| meta.auth_binding.as_mut())
                .expect("managed binding")
                .account_id = Some("acct-managed-b".to_string());

            let switch_guard = tauri::async_runtime::block_on(
                state
                    .proxy_service
                    .lock_switch_for_app(AppType::Codex.as_str()),
            );
            let (started_tx, started_rx) = std::sync::mpsc::channel();
            let (update_result, live_after_switch) = std::thread::scope(|scope| {
                let updater = scope.spawn(move || {
                    started_tx.send(()).expect("signal updater start");
                    ProviderService::update(state, AppType::Codex, None, updated)
                });
                started_rx.recv().expect("wait for updater");

                // This emulates a switch that already owns the per-app lock and
                // commits a different current target before the queued update is
                // allowed to inspect current/existing state.
                state
                    .db
                    .set_current_provider(AppType::Codex.as_str(), &third_party.id)
                    .expect("switch DB current to third party");
                crate::settings::set_current_provider(
                    &AppType::Codex,
                    Some(third_party.id.as_str()),
                )
                .expect("switch local current to third party");
                write_live_for_state(state, &AppType::Codex, &third_party)
                    .expect("write third-party live");
                let live_after_switch = crate::codex_config::CodexLiveStateSnapshot::capture()
                    .expect("capture third-party live");

                drop(switch_guard);
                let result = updater.join().expect("join managed updater");
                (result, live_after_switch)
            });

            update_result.expect("save queued non-current managed row");
            assert_eq!(
                state
                    .db
                    .get_current_provider(AppType::Codex.as_str())
                    .expect("read DB current")
                    .as_deref(),
                Some(third_party.id.as_str())
            );
            assert_eq!(
                crate::codex_config::CodexLiveStateSnapshot::capture()
                    .expect("capture live after queued update"),
                live_after_switch,
                "queued provider edit must not rewrite the newly switched current live"
            );
            let saved_official = state
                .db
                .get_provider_by_id(&official.id, AppType::Codex.as_str())
                .expect("read saved official")
                .expect("official exists");
            assert_eq!(
                saved_official
                    .meta
                    .as_ref()
                    .and_then(|meta| meta.managed_account_id_for("codex_oauth")),
                Some("acct-managed-b".to_string())
            );
        });
    }
}

impl ProviderService {
    pub(crate) fn managed_codex_oauth_account_id(provider: &Provider) -> Option<String> {
        provider
            .meta
            .as_ref()
            .and_then(|meta| meta.managed_account_id_for("codex_oauth"))
            .map(|id| id.trim().to_string())
            .filter(|id| !id.is_empty())
    }

    fn normalize_provider_if_claude(app_type: &AppType, provider: &mut Provider) {
        if matches!(app_type, AppType::Claude) {
            let mut v = provider.settings_config.clone();
            if normalize_claude_models_in_value(&mut v) {
                provider.settings_config = v;
            }
        }
    }

    /// Check whether a provider exists in live config, tolerating parse errors
    /// only for providers that are explicitly marked as DB-only.
    fn check_live_config_exists(
        app_type: &AppType,
        provider_id: &str,
        live_config_managed: Option<bool>,
    ) -> Result<bool, AppError> {
        if live_config_managed == Some(false) {
            Ok(provider_exists_in_live_config(app_type, provider_id).unwrap_or(false))
        } else {
            provider_exists_in_live_config(app_type, provider_id)
        }
    }

    fn provider_live_config_managed(provider: &Provider) -> Option<bool> {
        provider
            .meta
            .as_ref()
            .and_then(|meta| meta.live_config_managed)
    }

    fn set_provider_live_config_managed(provider: &mut Provider, managed: bool) {
        provider
            .meta
            .get_or_insert_with(Default::default)
            .live_config_managed = Some(managed);
    }

    fn normalize_usage_script_credential_overrides(app_type: &AppType, provider: &mut Provider) {
        let current_credentials = provider.resolve_usage_credentials(app_type);

        let Some(usage_script) = provider
            .meta
            .as_mut()
            .and_then(|meta| meta.usage_script.as_mut())
        else {
            return;
        };

        if usage_script.template_type.as_deref() == Some("token_plan") {
            return;
        }

        if usage_script.api_key.as_deref().is_some_and(|api_key| {
            Self::should_clear_usage_api_key_override(api_key, &current_credentials)
        }) {
            usage_script.api_key = None;
        }

        if usage_script.base_url.as_deref().is_some_and(|base_url| {
            Self::should_clear_usage_base_url_override(base_url, &current_credentials)
        }) {
            usage_script.base_url = None;
        }
    }

    fn should_clear_usage_api_key_override(
        script_api_key: &str,
        current_credentials: &(String, String),
    ) -> bool {
        let candidate = script_api_key.trim();
        if candidate.is_empty() {
            return true;
        }

        let matches_provider_key = |api_key: &str| {
            let api_key = api_key.trim();
            !api_key.is_empty() && api_key == candidate
        };

        matches_provider_key(&current_credentials.1)
    }

    fn should_clear_usage_base_url_override(
        script_base_url: &str,
        current_credentials: &(String, String),
    ) -> bool {
        let candidate = Self::normalize_usage_base_url_for_compare(script_base_url);
        if candidate.is_empty() {
            return true;
        }

        let matches_provider_base_url = |base_url: &str| {
            let base_url = Self::normalize_usage_base_url_for_compare(base_url);
            !base_url.is_empty() && base_url == candidate
        };

        matches_provider_base_url(&current_credentials.0)
    }

    fn normalize_usage_base_url_for_compare(base_url: &str) -> String {
        base_url.trim().trim_end_matches('/').to_string()
    }

    /// List all providers for an app type
    pub fn list(
        state: &AppState,
        app_type: AppType,
    ) -> Result<IndexMap<String, Provider>, AppError> {
        if app_type == AppType::Pi {
            return pi::list(state);
        }
        if app_type == AppType::Mcode {
            let native = crate::mcode_config::get_providers()?;
            let mut saved = state.db.get_all_providers("mcode")?;
            for (id, config) in &native {
                if !saved.contains_key(id) {
                    let name = config.get("name").and_then(Value::as_str).unwrap_or(id);
                    saved.insert(
                        id.clone(),
                        Provider::with_id(id.clone(), name.into(), config.clone(), None),
                    );
                }
            }
            for (id, provider) in &mut saved {
                if let Some(config) = native.get(id) {
                    if let Some(name) = config.get("name").and_then(Value::as_str) {
                        provider.name = name.into();
                    }
                    provider.settings_config = config.clone();
                }
                Self::set_provider_live_config_managed(provider, native.contains_key(id));
                state.db.save_provider("mcode", provider)?;
            }
            return Ok(saved);
        }
        state.db.get_all_providers(app_type.as_str())
    }

    /// Get current provider ID
    ///
    /// 使用有效的当前供应商 ID（验证过存在性）。
    /// 优先从本地 settings 读取，验证后 fallback 到数据库的 is_current 字段。
    /// 这确保了云同步场景下多设备可以独立选择供应商，且返回的 ID 一定有效。
    ///
    /// 对于累加模式应用（OpenCode, OpenClaw），不存在"当前供应商"概念，直接返回空字符串。
    pub fn current(state: &AppState, app_type: AppType) -> Result<String, AppError> {
        // Additive mode apps have no "current" provider concept
        if app_type.is_additive_mode() {
            return Ok(String::new());
        }
        // 代理模式下界面上的「当前」是代理路由到的那家。
        crate::mode::current::provider_for(
            &state.db,
            &app_type,
            crate::mode::current::Purpose::InUse,
        )
        .map(|opt| opt.unwrap_or_default())
    }

    fn save_mcode_provider(
        state: &AppState,
        provider: &Provider,
        write_live: bool,
        create: bool,
    ) -> Result<bool, AppError> {
        let previous = state.db.get_provider_by_id(&provider.id, "mcode")?;
        state.db.save_provider("mcode", provider)?;
        if write_live {
            let config = provider.settings_config.clone();
            let written = if create {
                crate::mcode_config::add_provider(&provider.id, config)
            } else {
                crate::mcode_config::set_provider(&provider.id, config)
            };
            if let Err(error) = written {
                match previous {
                    Some(previous) => state.db.save_provider("mcode", &previous)?,
                    None => state.db.delete_provider("mcode", &provider.id)?,
                }
                return Err(error);
            }
        }
        Ok(true)
    }

    /// Add a new provider
    pub fn add(
        state: &AppState,
        app_type: AppType,
        provider: Provider,
        add_to_live: bool,
    ) -> Result<bool, AppError> {
        if app_type == AppType::Pi {
            return pi::add(state, provider, add_to_live);
        }

        let mut provider = provider;
        // Normalize Claude model keys
        Self::normalize_provider_if_claude(&app_type, &mut provider);
        Self::validate_provider_settings(&app_type, &provider)?;
        Self::normalize_usage_script_credential_overrides(&app_type, &mut provider);
        if app_type.is_additive_mode() {
            Self::set_provider_live_config_managed(&mut provider, add_to_live);
        }
        if matches!(app_type, AppType::Claude | AppType::Codex | AppType::Gemini) {
            keep_common_config_for_old_versions(&mut provider);
        }

        if matches!(app_type, AppType::Codex) {
            return Self::add_codex(state, provider);
        }

        if app_type == AppType::Mcode {
            // The key is user-chosen, and saving would overwrite an existing row or live node.
            // Adds are serialized here; the live write repeats the check under MCode's file lock.
            let _guard = futures::executor::block_on(
                state.proxy_service.lock_switch_for_app(app_type.as_str()),
            );
            if state
                .db
                .get_provider_by_id(&provider.id, "mcode")?
                .is_some()
                || crate::mcode_config::provider_key_exists(&provider.id)?
            {
                return Err(AppError::InvalidInput(format!(
                    "MCode provider key '{}' already exists",
                    provider.id
                )));
            }
            return Self::save_mcode_provider(state, &provider, add_to_live, true);
        }

        // 还没有当前供应商时这一家会被写进 live：和进入代理、切换互斥（同 `update`）。
        let _switch_guard = crate::mode::controller::lock_settled_blocking(state, &app_type)?;

        // Save to database
        state.db.save_provider(app_type.as_str(), &provider)?;

        // Additive mode apps (OpenCode, OpenClaw): optionally write to live config.
        if app_type.is_additive_mode() {
            // OMO / OMO Slim providers use exclusive mode and write to dedicated config file.
            if matches!(app_type, AppType::OpenCode)
                && matches!(provider.category.as_deref(), Some("omo") | Some("omo-slim"))
            {
                // Do not auto-enable newly added OMO / OMO Slim providers.
                // Users must explicitly switch/apply an OMO provider to activate it.
                return Ok(true);
            }
            if !add_to_live {
                return Ok(true);
            }
            write_live_for_state(state, &app_type, &provider)?;
            return Ok(true);
        }

        // For other apps: Check if sync is needed (if this is current provider, or no current provider)
        let current = crate::mode::current::provider_for(
            &state.db,
            &app_type,
            crate::mode::current::Purpose::Direct,
        )?;
        if current.is_none() {
            // 第一个供应商同样只写关键字段，不覆盖用户已有的配置文件。
            match app_type {
                AppType::Claude => {
                    claude_direct::switch_to(state.db.as_ref(), None, &provider)?;
                    return Ok(true);
                }
                AppType::Gemini => {
                    gemini_direct::switch_to(state.db.as_ref(), &provider)?;
                    return Ok(true);
                }
                AppType::GrokBuild => {
                    grok_direct::switch_to(state.db.as_ref(), None, &provider)?;
                    return Ok(true);
                }
                _ => {}
            }
            // No current provider, set as current and sync. Managed Codex adds
            // use the transactional path above because token resolution can fail.
            state
                .db
                .set_current_provider(app_type.as_str(), &provider.id)?;
            write_live_for_state(state, &app_type, &provider)?;
        }

        Ok(true)
    }

    /// 新增 Codex 供应商。还没有当前供应商时，它就是第一个：行、live 和指针一起提交，
    /// live 只写关键字段，不覆盖用户已有的 config.toml。写 live 失败时撤回刚存的行，
    /// 免得留下一个看得见却用不了的供应商。
    fn add_codex(state: &AppState, provider: Provider) -> Result<bool, AppError> {
        let app_type = AppType::Codex;
        // 和切换互斥：等着的切换不能看到只存了一半的托管账号绑定。
        let _switch_guard = crate::mode::controller::lock_settled_blocking(state, &app_type)?;
        let current = crate::mode::current::provider_for(
            &state.db,
            &app_type,
            crate::mode::current::Purpose::Direct,
        )?;
        if current.is_some() {
            state.db.save_provider(app_type.as_str(), &provider)?;
            return Ok(true);
        }

        // 行有问题（会把官方登录发给第三方等）在存行之前就拒绝。
        codex_direct::preflight(state.db.as_ref(), &provider)?;
        let previous = state
            .db
            .get_provider_by_id(&provider.id, app_type.as_str())?;
        state.db.save_provider(app_type.as_str(), &provider)?;
        let written = codex_direct::write_direct(
            state.db.as_ref(),
            &state.codex_oauth_manager,
            crate::mode::state::op::SWITCH,
            codex_direct::Owner::None,
            Some(&provider),
            crate::mode::state::PendingTarget::pointer(Some(provider.id.clone())),
        );
        if let Err(error) = written {
            // 文件已经发布、只是落定状态失败时，pending 会在下次操作或启动时补完指针，
            // 行要留着；还没发布就失败，撤回刚存的行。
            if crate::mode::operation::has_pending(AppType::Codex.as_str()) {
                return Err(error);
            }
            let rollback = match &previous {
                Some(previous) => state.db.save_provider(app_type.as_str(), previous),
                None => state.db.delete_provider(app_type.as_str(), &provider.id),
            };
            if let Err(rollback) = rollback {
                return Err(AppError::Message(format!(
                    "新增首个 Codex 供应商失败: {error}; 恢复供应商数据同时失败: {rollback}"
                )));
            }
            return Err(error);
        }
        Ok(true)
    }

    /// 供应商编辑器底部配置的显示内容（Claude Code、Codex、Gemini CLI、Grok Build）：切到
    /// 这个供应商之后配置文件会是什么样，以及行里不随切换生效的字段。`category` 用来认出
    /// 官方卡。
    pub fn editor_view(
        state: &AppState,
        app_type: AppType,
        settings_config: &Value,
        category: Option<&str>,
    ) -> Result<EditorView, AppError> {
        Self::editor_view_with_meta(state, app_type, settings_config, category, None, None)
    }

    /// Codex previews retain managed-provider identity. Explicit draft metadata,
    /// including an empty value, takes precedence over the stored card's metadata.
    pub fn editor_view_with_meta(
        state: &AppState,
        app_type: AppType,
        settings_config: &Value,
        category: Option<&str>,
        provider_id: Option<&str>,
        meta: Option<&ProviderMeta>,
    ) -> Result<EditorView, AppError> {
        match app_type {
            AppType::Claude => claude_editor::view(state, settings_config),
            AppType::Codex => {
                let stored_meta = match (meta, provider_id) {
                    (None, Some(id)) => state
                        .db
                        .get_provider_by_id(id, AppType::Codex.as_str())?
                        .and_then(|provider| provider.meta),
                    _ => None,
                };
                codex_editor::view(
                    state,
                    settings_config,
                    category,
                    meta.or(stored_meta.as_ref()),
                )
            }
            AppType::Gemini => gemini_editor::view(state, settings_config, category),
            AppType::GrokBuild => grok_editor::view(state, settings_config, category),
            other => Err(AppError::InvalidInput(format!(
                "{} 的编辑器还不支持按关键字段显示",
                other.as_str()
            ))),
        }
    }

    /// 编辑已有供应商时给 [`Self::editor_view`] 的 `category`：按库里那一行，用切换时同一个
    /// 判断认官方卡。Gemini 还按合作方标记和名字认出旧版没有 `category` 的 Google 卡，Codex
    /// 还认固定 id 和托管账号；只看 `category` 的话，这些卡预览里的登录方式和切换写的不同。
    /// 没有 `provider_id`（新增）或行不存在时原样用 `category`。
    pub fn editor_category(
        state: &AppState,
        app_type: &AppType,
        provider_id: Option<&str>,
        category: Option<String>,
    ) -> Result<Option<String>, AppError> {
        let Some(id) = provider_id else {
            return Ok(category);
        };
        let Some(row) = state.db.get_provider_by_id(id, app_type.as_str())? else {
            return Ok(category);
        };
        let official = match app_type {
            AppType::Codex => codex_direct::is_official(&row),
            AppType::Gemini => gemini_direct::is_official(&row),
            _ => false,
        };
        Ok(if official {
            Some("official".to_string())
        } else {
            category
        })
    }

    /// 从编辑器新增供应商。Claude Code、Codex、Gemini CLI、Grok Build 按关键字段拆开保存
    /// （见各自的 `*_editor`），其余应用和 `add` 一样。
    pub fn add_from_editor(
        state: &AppState,
        app_type: AppType,
        provider: Provider,
        add_to_live: bool,
        editor: Option<EditorSave>,
    ) -> Result<bool, AppError> {
        match (app_type, editor) {
            (AppType::Claude, Some(editor)) => {
                Self::add_claude_from_editor(state, provider, editor)
            }
            (AppType::Codex, Some(editor)) => {
                Self::save_codex_from_editor(state, provider, editor, EditorSaveKind::Add)
            }
            (app_type @ (AppType::Gemini | AppType::GrokBuild), Some(editor)) => {
                Self::save_gemini_or_grok_from_editor(
                    state,
                    app_type,
                    provider,
                    editor,
                    EditorSaveKind::Add,
                )
            }
            (app_type, _) => Self::add(state, app_type, provider, add_to_live),
        }
    }

    /// 从编辑器保存供应商。Claude Code、Codex、Gemini CLI、Grok Build 按关键字段拆开保存
    /// （见各自的 `*_editor`），其余应用和 `update` 一样。
    pub fn update_from_editor(
        state: &AppState,
        app_type: AppType,
        original_id: Option<&str>,
        provider: Provider,
        editor: Option<EditorSave>,
    ) -> Result<bool, AppError> {
        match (app_type, editor) {
            (AppType::Claude, Some(editor))
                if original_id.is_none_or(|original| original == provider.id) =>
            {
                Self::update_claude_from_editor(state, provider, editor)
            }
            (AppType::Codex, Some(editor))
                if original_id.is_none_or(|original| original == provider.id) =>
            {
                Self::save_codex_from_editor(state, provider, editor, EditorSaveKind::Update)
            }
            (app_type @ (AppType::Gemini | AppType::GrokBuild), Some(editor))
                if original_id.is_none_or(|original| original == provider.id) =>
            {
                Self::save_gemini_or_grok_from_editor(
                    state,
                    app_type,
                    provider,
                    editor,
                    EditorSaveKind::Update,
                )
            }
            (app_type, _) => Self::update(state, app_type, original_id, provider),
        }
    }

    /// 从编辑器新增或保存 Codex 供应商：关键字段、独有字段存回行，其余改动作为全局设置
    /// 写进 live（三方比较）。直连模式下编辑当前供应商、或新增第一个供应商时，关键字段在
    /// 同一次写入里换进 live；代理模式下编辑路由那家，全局设置写完后按新行重写代理契约。
    fn save_codex_from_editor(
        state: &AppState,
        provider: Provider,
        editor: EditorSave,
        kind: EditorSaveKind,
    ) -> Result<bool, AppError> {
        let app_type = AppType::Codex;
        let _switch_guard = crate::mode::controller::lock_settled_blocking(state, &app_type)?;
        let existing = state
            .db
            .get_provider_by_id(&provider.id, app_type.as_str())?;
        let mut provider = provider;
        let plan = codex_editor::plan_save(
            existing.as_ref().map(|row| &row.settings_config),
            &provider.settings_config,
            &editor.base,
            &match (existing.as_ref(), editor.draft.as_ref()) {
                (Some(row), _) => codex_editor::Origin::row(&row.settings_config)?,
                (None, Some(draft)) => codex_editor::Origin::row(draft)?,
                (None, None) => codex_editor::Origin::Live(codex_editor::live_exclusive(state)?),
            },
            codex_direct::is_official(&provider),
            provider.uses_proxy_injected_oauth(),
            editor.on_conflict,
        )?;
        provider.settings_config = plan.row_settings.clone();
        Self::validate_provider_settings(&app_type, &provider)?;
        Self::normalize_usage_script_credential_overrides(&app_type, &mut provider);
        if kind == EditorSaveKind::Add {
            keep_common_config_for_old_versions(&mut provider);
        }

        let mode = crate::mode::current::mode_state(&app_type);
        let key_fields = kind.writes_key_fields(state, &app_type, &mode, &provider.id)?;

        state.db.save_provider(app_type.as_str(), &provider)?;
        let written = if key_fields {
            codex_editor::write_live(
                state.db.as_ref(),
                &state.codex_oauth_manager,
                &plan.edits,
                codex_editor::KeyFields::Direct {
                    prev: existing.as_ref(),
                    target: &provider,
                    set_pointer: kind == EditorSaveKind::Add,
                },
            )
        } else {
            codex_editor::write_live(
                state.db.as_ref(),
                &state.codex_oauth_manager,
                &plan.edits,
                codex_editor::KeyFields::None,
            )
            .and_then(|()| Self::resync_proxy_for_saved_row(state, &app_type, &provider))
        };
        Self::keep_row_if_written(state, &app_type, &provider.id, existing.as_ref(), written)
    }

    /// 从编辑器新增或保存 Gemini CLI、Grok Build 供应商：关键字段存回行，其余改动作为全局
    /// 设置写进 live（三方比较）。直连模式下编辑当前供应商、或新增第一个供应商时，关键
    /// 字段在同一次写入里换进 live；代理模式下编辑路由那家，全局设置写完后按新行重写代理
    /// 契约。
    fn save_gemini_or_grok_from_editor(
        state: &AppState,
        app_type: AppType,
        provider: Provider,
        editor: EditorSave,
        kind: EditorSaveKind,
    ) -> Result<bool, AppError> {
        let _switch_guard = crate::mode::controller::lock_settled_blocking(state, &app_type)?;
        let existing = state
            .db
            .get_provider_by_id(&provider.id, app_type.as_str())?;
        let stored = existing.as_ref().map(|row| &row.settings_config);
        let mut provider = provider;
        enum Edits {
            Gemini(gemini_editor::GeminiEdits),
            Grok(editor_toml::TomlEdits),
        }
        let edits = if matches!(app_type, AppType::Gemini) {
            let plan = gemini_editor::plan_save(
                stored,
                &provider.settings_config,
                &editor.base,
                editor.on_conflict,
            )?;
            provider.settings_config = plan.row_settings;
            Edits::Gemini(plan.edits)
        } else {
            let plan = grok_editor::plan_save(
                stored,
                &provider.settings_config,
                &editor.base,
                grok_direct::is_official(&provider),
                editor.on_conflict,
            )?;
            provider.settings_config = plan.row_settings;
            Edits::Grok(plan.edits)
        };
        Self::validate_provider_settings(&app_type, &provider)?;
        Self::normalize_usage_script_credential_overrides(&app_type, &mut provider);
        if kind == EditorSaveKind::Add && matches!(app_type, AppType::Gemini) {
            keep_common_config_for_old_versions(&mut provider);
        }

        let mode = crate::mode::current::mode_state(&app_type);
        let key_fields = kind.writes_key_fields(state, &app_type, &mode, &provider.id)?;
        let set_pointer = kind == EditorSaveKind::Add;

        state.db.save_provider(app_type.as_str(), &provider)?;
        let written = match &edits {
            Edits::Gemini(edits) => gemini_editor::write_live(
                state.db.as_ref(),
                edits,
                key_fields.then_some(&provider),
                set_pointer,
            ),
            Edits::Grok(edits) => grok_editor::write_live(
                state.db.as_ref(),
                edits,
                key_fields.then_some((existing.as_ref(), &provider)),
                set_pointer,
            ),
        }
        .and_then(|()| Self::resync_proxy_for_saved_row(state, &app_type, &provider));
        Self::keep_row_if_written(state, &app_type, &provider.id, existing.as_ref(), written)
    }

    /// 编辑器保存先存行、再写 live（`written`）。写 live 失败时：文件已经发布、只是落定
    /// 状态失败的，pending 会在下次操作或启动时补完，行要留着；还没发布就失败（没有
    /// pending），撤回刚存的行。
    fn keep_row_if_written(
        state: &AppState,
        app_type: &AppType,
        provider_id: &str,
        existing: Option<&Provider>,
        written: Result<(), AppError>,
    ) -> Result<bool, AppError> {
        let Err(error) = written else {
            return Ok(true);
        };
        if crate::mode::operation::has_pending(app_type.as_str()) {
            return Err(error);
        }
        let rollback = match existing {
            Some(existing) => state.db.save_provider(app_type.as_str(), existing),
            None => state.db.delete_provider(app_type.as_str(), provider_id),
        };
        if let Err(rollback) = rollback {
            log::warn!(
                "恢复 {} 供应商 '{provider_id}' 失败: {rollback}",
                app_type.as_str()
            );
        }
        Err(error)
    }

    /// 代理模式下存好了这一行：它是代理路由或在 Stack 名单里时按新行重写代理契约，契约没变
    /// 就不碰客户端文件（见 [`crate::mode::controller::resync_saved_row_locked`]）。调用方
    /// 持有这个应用的切换锁。
    fn resync_proxy_for_saved_row(
        state: &AppState,
        app_type: &AppType,
        provider: &Provider,
    ) -> Result<(), AppError> {
        futures::executor::block_on(crate::mode::controller::resync_saved_row_locked(
            state, app_type, provider,
        ))
        .map_err(AppError::Message)
    }

    fn add_claude_from_editor(
        state: &AppState,
        provider: Provider,
        editor: EditorSave,
    ) -> Result<bool, AppError> {
        let app_type = AppType::Claude;
        let _switch_guard = crate::mode::controller::lock_settled_blocking(state, &app_type)?;
        let mut provider = provider;
        Self::normalize_provider_if_claude(&app_type, &mut provider);
        let plan = claude_editor::plan_save(None, &provider.settings_config, &editor.base)?;
        provider.settings_config = plan.row_settings.clone();
        Self::validate_provider_settings(&app_type, &provider)?;
        Self::normalize_usage_script_credential_overrides(&app_type, &mut provider);
        keep_common_config_for_old_versions(&mut provider);

        let existing = state
            .db
            .get_provider_by_id(&provider.id, app_type.as_str())?;
        let first = crate::mode::current::provider_for(
            &state.db,
            &app_type,
            crate::mode::current::Purpose::Direct,
        )?
        .is_none();
        state.db.save_provider(app_type.as_str(), &provider)?;

        let key_fields = first.then_some(claude_editor::KeyFieldWrite {
            prev: None,
            target: &provider,
            set_pointer: true,
        });
        let written =
            claude_editor::write_live(state.db.as_ref(), &plan, editor.on_conflict, key_fields);
        Self::keep_row_if_written(state, &app_type, &provider.id, existing.as_ref(), written)
    }

    /// 和其他编辑器一样先存行、再写 live：写 live 失败且没有 pending 时撤回行；已经发布的
    /// 由 pending 补完，行和 live 不会对不上。
    fn update_claude_from_editor(
        state: &AppState,
        provider: Provider,
        editor: EditorSave,
    ) -> Result<bool, AppError> {
        let app_type = AppType::Claude;
        let _switch_guard = crate::mode::controller::lock_settled_blocking(state, &app_type)?;
        let existing = state
            .db
            .get_provider_by_id(&provider.id, app_type.as_str())?;
        let mut provider = provider;
        Self::normalize_provider_if_claude(&app_type, &mut provider);
        let plan = claude_editor::plan_save(
            existing.as_ref().map(|row| &row.settings_config),
            &provider.settings_config,
            &editor.base,
        )?;
        provider.settings_config = plan.row_settings.clone();
        Self::validate_provider_settings(&app_type, &provider)?;
        Self::normalize_usage_script_credential_overrides(&app_type, &mut provider);

        let mode = crate::mode::current::mode_state(&app_type);
        // 代理模式下 live 的关键字段是代理契约，这里只写全局改动；编辑的是代理路由那家
        // 或 Stack 名单里的一家时，写完按新行重写契约（契约没变就不动）。直连指针那家在
        // 退出代理时写回。
        let key_fields = EditorSaveKind::Update
            .writes_key_fields(state, &app_type, &mode, &provider.id)?
            .then_some(claude_editor::KeyFieldWrite {
                prev: existing.as_ref(),
                target: &provider,
                set_pointer: false,
            });

        state.db.save_provider(app_type.as_str(), &provider)?;
        let written =
            claude_editor::write_live(state.db.as_ref(), &plan, editor.on_conflict, key_fields)
                .and_then(|()| Self::resync_proxy_for_saved_row(state, &app_type, &provider));
        Self::keep_row_if_written(state, &app_type, &provider.id, existing.as_ref(), written)
    }

    /// Update a provider
    pub fn update(
        state: &AppState,
        app_type: AppType,
        original_id: Option<&str>,
        provider: Provider,
    ) -> Result<bool, AppError> {
        if app_type == AppType::Pi {
            return pi::update(state, original_id, provider);
        }

        let mut provider = provider;
        let original_id = original_id.unwrap_or(provider.id.as_str()).to_string();
        let provider_id_changed = original_id != provider.id;
        // 读旧行、判断谁是当前供应商、存行、写 live 都在切换锁里：进入代理和切换都等这次
        // 保存写完，这里也不会在读完模式之后、写 live 之前被它们改掉模式（否则直连的关键
        // 字段会盖掉刚写的代理契约）。Codex 还要在锁里读旧行，看它是不是托管账号的行。
        let _switch_guard = crate::mode::controller::lock_settled_blocking(state, &app_type)?;
        let existing_provider = state
            .db
            .get_provider_by_id(&original_id, app_type.as_str())?;
        // Normalize Claude model keys
        Self::normalize_provider_if_claude(&app_type, &mut provider);
        Self::validate_provider_settings(&app_type, &provider)?;
        if matches!(app_type, AppType::Codex) && provider.category.as_deref() == Some("official") {
            crate::codex_config::strip_codex_unified_session_bucket_from_settings(
                &mut provider.settings_config,
            )?;
        }
        Self::normalize_usage_script_credential_overrides(&app_type, &mut provider);

        if provider_id_changed {
            if !app_type.is_additive_mode() {
                return Err(AppError::Message(
                    "Only additive-mode providers support changing provider key".to_string(),
                ));
            }

            let Some(existing_provider) = existing_provider else {
                return Err(AppError::Message(format!(
                    "Original provider '{}' does not exist in app '{}'",
                    original_id,
                    app_type.as_str()
                )));
            };

            // OMO / OMO Slim providers are activated via a dedicated current-state mechanism
            // (set_omo_provider_current) that is NOT captured by provider_exists_in_live_config,
            // which only checks opencode.json. A rename would orphan that current-state marker
            // and silently break subsequent OMO file syncs. Block it unconditionally.
            if matches!(app_type, AppType::OpenCode)
                && matches!(
                    existing_provider.category.as_deref(),
                    Some("omo") | Some("omo-slim")
                )
            {
                return Err(AppError::Message(
                    "Provider key cannot be changed for OMO/OMO Slim providers".to_string(),
                ));
            }

            let original_in_live = Self::check_live_config_exists(
                &app_type,
                &original_id,
                Self::provider_live_config_managed(&existing_provider),
            )?;
            if original_in_live {
                return Err(AppError::Message(
                    "Provider key cannot be changed after the provider has been added to the app config"
                        .to_string(),
                ));
            }

            let next_id_in_live = Self::check_live_config_exists(
                &app_type,
                &provider.id,
                Self::provider_live_config_managed(&existing_provider),
            )?;
            if state
                .db
                .get_provider_by_id(&provider.id, app_type.as_str())?
                .is_some()
                || next_id_in_live
            {
                return Err(AppError::Message(format!(
                    "Provider '{}' already exists in app '{}'",
                    provider.id,
                    app_type.as_str()
                )));
            }

            Self::set_provider_live_config_managed(&mut provider, false);
            let was_current = crate::mode::current::local_direct_pointer(&app_type).as_deref()
                == Some(original_id.as_str());
            state.db.save_provider(app_type.as_str(), &provider)?;
            state.db.delete_provider(app_type.as_str(), &original_id)?;

            if was_current {
                crate::settings::set_current_provider(&app_type, Some(provider.id.as_str()))?;
            }

            return Ok(true);
        }

        // Additive mode apps (OpenCode, OpenClaw): only sync to live when the provider
        // already exists in live config. Editing a DB-only provider must not auto-add it.
        if app_type.is_additive_mode() {
            let omo_variant = if matches!(app_type, AppType::OpenCode) {
                match provider.category.as_deref() {
                    Some("omo") => Some(&crate::services::omo::STANDARD),
                    Some("omo-slim") => Some(&crate::services::omo::SLIM),
                    _ => None,
                }
            } else {
                None
            };
            if let Some(variant) = omo_variant {
                let is_current = state.db.is_omo_provider_current(
                    app_type.as_str(),
                    &provider.id,
                    variant.category,
                )?;
                if is_current {
                    crate::services::OmoService::write_provider_config_to_file(&provider, variant)?;
                }
                if let Err(err) = state.db.save_provider(app_type.as_str(), &provider) {
                    if is_current {
                        if let Err(rollback_err) =
                            crate::services::OmoService::write_config_to_file(state, variant)
                        {
                            log::warn!(
                                "Failed to roll back {} config after DB save error: {}",
                                variant.label,
                                rollback_err
                            );
                        }
                    }
                    return Err(err);
                }
                return Ok(true);
            }
            let live_config_managed = Self::check_live_config_exists(
                &app_type,
                &provider.id,
                Self::provider_live_config_managed(&provider).or_else(|| {
                    existing_provider
                        .as_ref()
                        .and_then(Self::provider_live_config_managed)
                }),
            )?;
            Self::set_provider_live_config_managed(&mut provider, live_config_managed);

            if app_type == AppType::Mcode {
                return Self::save_mcode_provider(state, &provider, live_config_managed, false);
            }

            // Save to database after live-config presence is resolved so parse errors
            // do not report failure after already mutating DB state.
            state.db.save_provider(app_type.as_str(), &provider)?;

            if !live_config_managed {
                return Ok(true);
            }
            write_live_for_state(state, &app_type, &provider)?;
            return Ok(true);
        }

        // For other apps: 是否是直连指针那家。代理模式下是不是代理路由或 Stack 名单里的一家，
        // 由同步 live 的那一步判断。
        let mode = crate::mode::current::mode_state(&app_type);
        let is_direct_current = crate::mode::current::provider_for(
            &state.db,
            &app_type,
            crate::mode::current::Purpose::Direct,
        )?
        .as_deref()
            == Some(provider.id.as_str());

        if matches!(app_type, AppType::Codex) {
            return Self::update_codex(
                state,
                &provider,
                existing_provider.as_ref(),
                is_direct_current,
            );
        }

        // Save to database
        state.db.save_provider(app_type.as_str(), &provider)?;

        if is_direct_current || mode.is_proxy() {
            let outcome = live::sync_live_for_provider_respecting_mode(
                state,
                &app_type,
                &provider,
                existing_provider.as_ref(),
            )?;
            if outcome == LiveSyncOutcome::WroteLive {
                // MCP is stored in the database and projected after a successful
                // live write. Keep the failure best-effort so the provider save
                // itself is not reported as failed when MCP projection can retry.
                if let Err(err) = McpService::sync_enabled_for_app(state, &app_type) {
                    log::warn!(
                        "保存供应商后重投影 {app_type:?} MCP 失败（将在下次同步时自愈）: {err}"
                    );
                }
            }
        }

        Ok(true)
    }

    /// 保存 Codex 供应商。调用方持有这个应用的切换锁。
    ///
    /// - 直连模式下编辑直连那家：先存行，再只替换 live 里的关键字段和独有字段（换托管
    ///   账号时先采纳、再清掉旧账号的登录）；写 live 失败就把行恢复原样。
    /// - 代理模式下先存行，编辑的是路由那家或 Stack 名单里的一家时按新行重写代理契约（见
    ///   [`Self::resync_proxy_for_saved_row`]），写失败同样把行恢复原样。
    /// - 其余只存行。
    fn update_codex(
        state: &AppState,
        provider: &Provider,
        existing: Option<&Provider>,
        is_direct_current: bool,
    ) -> Result<bool, AppError> {
        let app_type = AppType::Codex;
        let mode = crate::mode::current::mode_state(&app_type);
        if !mode.is_proxy() && !is_direct_current {
            state.db.save_provider(app_type.as_str(), provider)?;
            return Ok(true);
        }
        if !mode.is_proxy() {
            codex_direct::preflight(state.db.as_ref(), provider)?;
        }

        state.db.save_provider(app_type.as_str(), provider)?;
        let written = if mode.is_proxy() {
            Self::resync_proxy_for_saved_row(state, &app_type, provider)
        } else {
            codex_direct::write_direct(
                state.db.as_ref(),
                &state.codex_oauth_manager,
                crate::mode::state::op::APPLY,
                existing.map_or(codex_direct::Owner::None, codex_direct::Owner::Provider),
                Some(provider),
                crate::mode::state::PendingTarget::default(),
            )
            .map(|_| ())
        };
        if let Err(error) = written {
            if crate::mode::operation::has_pending(AppType::Codex.as_str()) {
                return Err(error);
            }
            if let Some(existing) = existing {
                if let Err(rollback) = state.db.save_provider(app_type.as_str(), existing) {
                    return Err(AppError::Message(format!(
                        "更新 Codex 供应商失败: {error}; 恢复供应商数据同时失败: {rollback}"
                    )));
                }
            }
            return Err(error);
        }
        Ok(true)
    }

    pub(crate) fn update_pi_usage_script(
        state: &AppState,
        id: &str,
        script: crate::provider::UsageScript,
    ) -> Result<bool, AppError> {
        pi::update_usage_script(state, id, script)
    }

    /// Delete a provider
    ///
    /// 同时检查本地 settings 和数据库的当前供应商，防止删除任一端正在使用的供应商。
    /// 对于累加模式应用（OpenCode, OpenClaw），可以随时删除任意供应商，同时从 live 配置中移除。
    pub fn delete(state: &AppState, app_type: AppType, id: &str) -> Result<(), AppError> {
        if app_type == AppType::Pi {
            return pi::delete(state, id);
        }

        // Additive mode apps - no current provider concept
        if app_type.is_additive_mode() {
            // Single DB read shared across all additive-mode sub-paths below.
            let existing = state.db.get_provider_by_id(id, app_type.as_str())?;

            if matches!(app_type, AppType::OpenCode) {
                let provider_category = existing.as_ref().and_then(|p| p.category.clone());
                let omo_variant = match provider_category.as_deref() {
                    Some("omo") => Some(&crate::services::omo::STANDARD),
                    Some("omo-slim") => Some(&crate::services::omo::SLIM),
                    _ => None,
                };
                if let Some(variant) = omo_variant {
                    let was_current = state.db.is_omo_provider_current(
                        app_type.as_str(),
                        id,
                        variant.category,
                    )?;
                    state.db.delete_provider(app_type.as_str(), id)?;
                    if was_current {
                        crate::services::OmoService::delete_config_file(variant)?;
                    }
                    return Ok(());
                }
            }

            // Non-OMO path for both OpenCode and OpenClaw:
            // remove from live first (atomicity), then DB.
            //
            // Use check_live_config_exists rather than trusting the flag alone: the flag
            // can be stale (Some(false) for a provider that was written to live before the
            // live_config_managed flip was introduced). check_live_config_exists reads the
            // actual file when the flag is Some(false), so it handles historical data correctly.
            let live_managed = existing
                .as_ref()
                .and_then(Self::provider_live_config_managed);
            if Self::check_live_config_exists(&app_type, id, live_managed)? {
                match app_type {
                    AppType::OpenCode => remove_opencode_provider_from_live(id)?,
                    AppType::OpenClaw => remove_openclaw_provider_from_live(id)?,
                    AppType::Hermes => remove_hermes_provider_from_live(id)?,
                    AppType::Mcode => crate::mcode_config::remove_provider(id)?,
                    _ => {}
                }
            }
            state.db.delete_provider(app_type.as_str(), id)?;
            return Ok(());
        }

        // For other apps: 本地记录、DB、代理路由任何一处指着它都不能删
        if crate::mode::current::is_referenced(&state.db, &app_type, id)? {
            return Err(AppError::Message(
                "无法删除当前正在使用的供应商".to_string(),
            ));
        }

        // Stack 名单里的先移出（和客户端文件同一个操作提交，key 留在登记簿里），成功了再删行。
        // 删行失败时它已经不在名单里，重新加入即可。
        if crate::mode::stack::is_member(&app_type, id)? {
            futures::executor::block_on(crate::mode::controller::set_stack_member(
                state, &app_type, id, false,
            ))
            .map_err(|error| AppError::Message(error.message))?;
        }

        state.db.delete_provider(app_type.as_str(), id)
    }

    /// Remove provider from live config only (for additive mode apps like OpenCode, OpenClaw)
    ///
    /// Does NOT delete from database - provider remains in the list.
    /// This is used when user wants to "remove" a provider from active config
    /// but keep it available for future use.
    pub fn remove_from_live_config(
        state: &AppState,
        app_type: AppType,
        id: &str,
    ) -> Result<(), AppError> {
        if app_type == AppType::Pi {
            return pi::remove(state, id);
        }

        match app_type {
            AppType::OpenCode => {
                let provider_category = state
                    .db
                    .get_provider_by_id(id, app_type.as_str())?
                    .and_then(|p| p.category);

                let omo_variant = match provider_category.as_deref() {
                    Some("omo") => Some(&crate::services::omo::STANDARD),
                    Some("omo-slim") => Some(&crate::services::omo::SLIM),
                    _ => None,
                };
                if let Some(variant) = omo_variant {
                    state
                        .db
                        .clear_omo_provider_current(app_type.as_str(), id, variant.category)?;
                    let still_has_current = state
                        .db
                        .get_current_omo_provider("opencode", variant.category)?
                        .is_some();
                    if still_has_current {
                        crate::services::OmoService::write_config_to_file(state, variant)?;
                    } else {
                        crate::services::OmoService::delete_config_file(variant)?;
                    }
                } else {
                    remove_opencode_provider_from_live(id)?;
                }
            }
            AppType::OpenClaw => {
                remove_openclaw_provider_from_live(id)?;
            }
            AppType::Hermes => {
                remove_hermes_provider_from_live(id)?;
            }
            AppType::Mcode => crate::mcode_config::remove_provider(id)?,
            _ => {
                return Err(AppError::Message(format!(
                    "App {} does not support remove from live config",
                    app_type.as_str()
                )));
            }
        }

        if let Some(mut provider) = state.db.get_provider_by_id(id, app_type.as_str())? {
            Self::set_provider_live_config_managed(&mut provider, false);
            state.db.save_provider(app_type.as_str(), &provider)?;
        }

        Ok(())
    }

    /// 切换供应商。
    ///
    /// - 代理模式：只换代理路由，直连指针不变；契约没变时客户端文件不读也不写。
    /// - 直连模式：切换式应用只替换客户端文件里的关键字段（不回填），文件和指针在同一个
    ///   操作里提交；累加式应用按各自的规则写入。
    pub fn switch(state: &AppState, app_type: AppType, id: &str) -> Result<SwitchResult, AppError> {
        let app = app_type.as_str().to_string();
        let result = Self::switch_inner(state, app_type, id);
        match &result {
            Ok(result) if result.warnings.is_empty() => {
                log::info!("[SWITCH] {app} 切到 {id} 完成");
            }
            Ok(result) => {
                log::warn!(
                    "[SWITCH] {app} 切到 {id} 完成，有警告: {:?}",
                    result.warnings
                );
            }
            // 规则拒绝：什么都没改。
            Err(error @ AppError::Localized { key, .. })
                if *key == "switch.official_blocked_by_proxy" =>
            {
                log::info!("[SWITCH] {app} 拒绝切到 {id}: {error}");
            }
            Err(error) => log::error!(
                "[SWITCH] {app} 切到 {id} 失败: {}",
                crate::error_for_log(&error.to_string())
            ),
        }
        result
    }

    fn switch_inner(
        state: &AppState,
        app_type: AppType,
        id: &str,
    ) -> Result<SwitchResult, AppError> {
        if app_type == AppType::Pi {
            return pi::enable(state, id);
        }

        // Check if provider exists
        let providers = state.db.get_all_providers(app_type.as_str())?;
        let _provider = providers
            .get(id)
            .ok_or_else(|| AppError::Message(format!("供应商 {id} 不存在")))?;

        // OMO providers are switched through their own exclusive path.
        if matches!(app_type, AppType::OpenCode) && _provider.category.as_deref() == Some("omo") {
            return Self::switch_normal(state, app_type, id, &providers);
        }

        // OMO Slim providers are switched through their own exclusive path.
        if matches!(app_type, AppType::OpenCode)
            && _provider.category.as_deref() == Some("omo-slim")
        {
            return Self::switch_normal(state, app_type, id, &providers);
        }

        if matches!(app_type, AppType::ClaudeDesktop) {
            return Self::switch_normal(state, app_type, id, &providers);
        }

        // 切换和进入 / 退出代理都会改客户端文件和指针。按应用串行，拿到锁、补完上一次
        // 没做完的写入之后再读模式和指针：刚进入代理的应用不会被一次直连写入覆盖，上次
        // 失败后重试也按补完后的指针删上一家的独有字段。
        let _switch_guard = crate::mode::controller::lock_settled_blocking(state, &app_type)?;

        if crate::mode::current::is_proxy(&app_type) {
            // 代理模式：只换代理路由，直连指针不变。契约没变时客户端文件不读也不写；
            // 变了在同一个操作里先改写客户端，再发布路由。
            if _provider.category.as_deref() == Some("official")
                && !official_provider_supports_proxy_takeover(&app_type, _provider)
            {
                return Err(AppError::localized(
                    "switch.official_blocked_by_proxy",
                    "路由模式下不能切换到官方供应商，使用代理访问官方 API 可能导致账号被封禁。请先退出路由模式，或选择第三方供应商。",
                    "Cannot switch to an official provider in routing mode. Using a proxy with official APIs may cause account bans.",
                ));
            }
            log::info!(
                "[SWITCH] {} 代理模式：路由 {} → {}",
                app_type.as_str(),
                crate::mode::current::mode_state(&app_type)
                    .proxy_route
                    .as_deref()
                    .unwrap_or("（无）"),
                id
            );
            futures::executor::block_on(crate::mode::controller::switch_route_locked(
                state, &app_type, _provider,
            ))
            .map_err(|e| AppError::Message(format!("切换路由失败: {e}")))?;
            // MCP 不随路由变：客户端文件没按直连重写。
            return Ok(SwitchResult::default());
        }

        // Normal mode: full switch with Live config write
        Self::switch_normal(state, app_type, id, &providers)
    }

    /// Normal switch flow (non-proxy mode)
    fn switch_normal(
        state: &AppState,
        app_type: AppType,
        id: &str,
        providers: &indexmap::IndexMap<String, Provider>,
    ) -> Result<SwitchResult, AppError> {
        let provider = providers
            .get(id)
            .ok_or_else(|| AppError::Message(format!("供应商 {id} 不存在")))?;

        if app_type.is_additive_mode() {
            log::info!("[SWITCH] {} 写入 {id}（累加式应用）", app_type.as_str());
        } else {
            let previous = crate::mode::current::provider_for(
                &state.db,
                &app_type,
                crate::mode::current::Purpose::Direct,
            )
            .ok()
            .flatten();
            log::info!(
                "[SWITCH] {} 直连：{} → {id}，写客户端配置",
                app_type.as_str(),
                previous.as_deref().unwrap_or("（无）")
            );
        }

        if matches!(app_type, AppType::Claude) {
            return Self::switch_claude_direct(state, provider, providers);
        }
        if matches!(app_type, AppType::Codex) {
            return Self::switch_codex_direct(state, provider, providers);
        }
        if matches!(app_type, AppType::Gemini) {
            gemini_direct::switch_to(state.db.as_ref(), provider)?;
            return Ok(SwitchResult::default());
        }
        if matches!(app_type, AppType::GrokBuild) {
            return Self::switch_grok_direct(state, provider, providers);
        }

        // OMO ↔ OMO Slim are mutually exclusive; activating one removes the other's config file.
        if matches!(app_type, AppType::OpenCode) {
            let omo_pair = match provider.category.as_deref() {
                Some("omo") => Some((&crate::services::omo::STANDARD, &crate::services::omo::SLIM)),
                Some("omo-slim") => {
                    Some((&crate::services::omo::SLIM, &crate::services::omo::STANDARD))
                }
                _ => None,
            };
            if let Some((enable, disable)) = omo_pair {
                state
                    .db
                    .set_omo_provider_current(app_type.as_str(), id, enable.category)?;
                crate::services::OmoService::write_config_to_file(state, enable)?;
                let _ = crate::services::OmoService::delete_config_file(disable);
                return Ok(SwitchResult::default());
            }
        }

        let mut result = SwitchResult::default();

        // Additive mode apps skip setting is_current (no such concept).
        if !app_type.is_additive_mode() {
            crate::settings::set_current_provider(&app_type, Some(id))?;
            state.db.set_current_provider(app_type.as_str(), id)?;
        }

        // 写 live（Claude Desktop、累加式应用；切换式应用在上面各自的分支里写完了）。
        write_live_for_state(state, &app_type, provider)?;

        // Hermes is additive, so "switching" doesn't overwrite a live config file
        // — we instead update the top-level `model:` section to point at this
        // provider's first declared model. Without this, clicking "switch" would
        // only shuffle entries in custom_providers[] while Hermes keeps using
        // whatever `model.provider` was set before.
        if matches!(app_type, AppType::Hermes) {
            if let Err(e) =
                crate::hermes_config::apply_switch_defaults(&provider.id, &provider.settings_config)
            {
                log::warn!(
                    "Failed to update Hermes model defaults after switching to '{}': {e}",
                    provider.id
                );
                result
                    .warnings
                    .push(format!("hermes_model_defaults_failed:{}", provider.id));
            }
        }

        // For additive-mode providers that were DB-only (live_config_managed == Some(false)),
        // flip the flag to true now that the provider has been successfully written to the live
        // file. This ensures sync_all_providers_to_live() will include it on future syncs.
        //
        // If persisting the marker fails, roll back the just-written live config so we don't leave
        // the provider in a silent inconsistent state (present in live, but still marked DB-only).
        if app_type.is_additive_mode() && Self::provider_live_config_managed(provider) != Some(true)
        {
            let mut updated = provider.clone();
            Self::set_provider_live_config_managed(&mut updated, true);
            if let Err(e) = state.db.save_provider(app_type.as_str(), &updated) {
                let rollback_result = match app_type {
                    AppType::OpenCode => remove_opencode_provider_from_live(&provider.id),
                    AppType::OpenClaw => remove_openclaw_provider_from_live(&provider.id),
                    AppType::Hermes => remove_hermes_provider_from_live(&provider.id),
                    AppType::Mcode => crate::mcode_config::remove_provider(&provider.id),
                    _ => Ok(()),
                };

                match rollback_result {
                    Ok(()) => {
                        return Err(AppError::Message(format!(
                            "Failed to persist live_config_managed for '{}' after writing live config; live changes were rolled back: {e}",
                            provider.id
                        )));
                    }
                    Err(rollback_err) => {
                        return Err(AppError::Message(format!(
                            "Failed to persist live_config_managed for '{}' after writing live config: {e}; additionally failed to roll back live config: {rollback_err}",
                            provider.id
                        )));
                    }
                }
            }
        }

        // 切换重写了目标应用的 live，只重投影该应用的 MCP（Grok Build 的
        // [mcp_servers] 与 live 同文件，整体替换后必须补回；其余应用的
        // MCP 文件独立于 live，投影是幂等维护）。不用全量 sync_all_enabled：
        // 无关应用的 live 损坏（如 ~/.claude.json 坏 JSON）不该阻断切换。
        // 走到这里 DB is_current 与 live 都已落盘，切换事实上已成功；
        // 投影失败上抛会让前端报"切换失败"制造分裂假象，故降级为警告
        // （MCP 投影可自愈：下次切换 / 任一 MCP 启停都会重新投影）。
        if let Err(err) = McpService::sync_enabled_for_app(state, &app_type) {
            log::warn!("切换供应商后重投影 {app_type:?} MCP 失败（将在下次同步时自愈）: {err}");
        }

        Ok(result)
    }

    /// Claude Code 直连切换：只替换关键字段和独有字段，文件和指针在同一个操作里提交。
    ///
    /// 不回填、不同步通用配置片段：用户在 live 里的改动本来就留在原处。发布前的失败
    /// （解析不了、并发冲突）什么都不改；开始发布后由 pending 保证前滚补完。
    fn switch_claude_direct(
        state: &AppState,
        provider: &Provider,
        providers: &IndexMap<String, Provider>,
    ) -> Result<SwitchResult, AppError> {
        let current_id = crate::mode::current::provider_for(
            &state.db,
            &AppType::Claude,
            crate::mode::current::Purpose::Direct,
        )?;
        let prev = current_id
            .as_deref()
            .and_then(|current_id| providers.get(current_id));
        claude_direct::switch_to(state.db.as_ref(), prev, provider)?;

        // MCP 在 ~/.claude.json，和 settings.json 无关；重投影是幂等维护，失败只记警告
        // （切换已经提交，下次同步会自愈）。
        if let Err(err) = McpService::sync_enabled_for_app(state, &AppType::Claude) {
            log::warn!("切换供应商后重投影 claude MCP 失败（将在下次同步时自愈）: {err}");
        }
        Ok(SwitchResult::default())
    }

    /// Codex 直连切换：`config.toml` 只替换关键字段和独有字段；`auth.json`、模型目录、
    /// 托管账号标记和指针在同一个操作里提交。
    ///
    /// 不回填、不同步通用配置片段、不补回 MCP：用户在 live 里的改动（含 `[mcp_servers]`）
    /// 本来就留在原处。行有问题（会把官方登录发给第三方、带 Key 却没地方放）时在写任何
    /// 东西之前报错，指针也不动。
    fn switch_codex_direct(
        state: &AppState,
        provider: &Provider,
        providers: &IndexMap<String, Provider>,
    ) -> Result<SwitchResult, AppError> {
        let current_id = crate::mode::current::provider_for(
            &state.db,
            &AppType::Codex,
            crate::mode::current::Purpose::Direct,
        )?;
        let owner = current_id
            .as_deref()
            .and_then(|current_id| providers.get(current_id))
            .map_or(codex_direct::Owner::None, codex_direct::Owner::Provider);
        codex_direct::write_direct(
            state.db.as_ref(),
            &state.codex_oauth_manager,
            crate::mode::state::op::SWITCH,
            owner,
            Some(provider),
            crate::mode::state::PendingTarget::pointer(Some(provider.id.clone())),
        )?;

        let mut result = SwitchResult::default();
        // 保留登录关闭时切到第三方要删掉 auth.json。删不掉（只读目录、被占用）不让切换
        // 失败：配置和指针都已提交，但要让用户看到官方登录还在盘上。
        if !codex_direct::is_official(provider)
            && !crate::settings::preserve_codex_official_auth_on_switch()
            && crate::codex_config::get_codex_auth_path().exists()
        {
            log::warn!("Codex auth.json still present after a preservation-off third-party switch");
            result
                .warnings
                .push("codex_auth_cleanup_failed".to_string());
        }
        Ok(result)
    }

    /// Grok Build 直连切换：`models.default` 和模型表换成目标的，按写入记录删掉上一家
    /// 的表；文件、指针和写入记录在同一个操作里提交。不回填、不补回 MCP。
    fn switch_grok_direct(
        state: &AppState,
        provider: &Provider,
        providers: &IndexMap<String, Provider>,
    ) -> Result<SwitchResult, AppError> {
        let current_id = crate::mode::current::provider_for(
            &state.db,
            &AppType::GrokBuild,
            crate::mode::current::Purpose::Direct,
        )?;
        let live_owner = current_id
            .as_deref()
            .and_then(|current_id| providers.get(current_id));
        grok_direct::switch_to(state.db.as_ref(), live_owner, provider)?;
        Ok(SwitchResult::default())
    }

    /// Sync current provider to live configuration (re-export)
    pub fn sync_current_to_live(state: &AppState) -> Result<(), AppError> {
        sync_current_to_live(state)
    }

    pub fn sync_current_provider_for_app(
        state: &AppState,
        app_type: AppType,
    ) -> Result<(), AppError> {
        if app_type.is_additive_mode() {
            return sync_additive_app_to_live(state, &app_type);
        }
        // 没有正在用的那家、或者在代理模式（客户端文件没按直连重写）时不重投影 MCP。
        let outcome = live::sync_current_provider_for_app_respecting_mode(state, &app_type)?;
        if outcome != Some(LiveSyncOutcome::WroteLive) {
            return Ok(());
        }
        McpService::sync_enabled_for_app(state, &app_type)
    }

    pub fn migrate_legacy_common_config_usage(
        state: &AppState,
        app_type: AppType,
        legacy_snippet: &str,
    ) -> Result<(), AppError> {
        if app_type.is_additive_mode() || legacy_snippet.trim().is_empty() {
            return Ok(());
        }

        let providers = state.db.get_all_providers(app_type.as_str())?;

        for provider in providers.values() {
            if provider
                .meta
                .as_ref()
                .and_then(|meta| meta.common_config_enabled)
                .is_some()
            {
                continue;
            }

            if !live::provider_uses_common_config(&app_type, provider, Some(legacy_snippet)) {
                continue;
            }

            let mut updated_provider = provider.clone();
            updated_provider
                .meta
                .get_or_insert_with(Default::default)
                .common_config_enabled = Some(true);

            match live::remove_common_config_from_settings(
                &app_type,
                &updated_provider.settings_config,
                legacy_snippet,
            ) {
                Ok(settings) => updated_provider.settings_config = settings,
                Err(err) => {
                    log::warn!(
                        "Failed to normalize legacy common config for {} provider '{}': {err}",
                        app_type.as_str(),
                        updated_provider.id
                    );
                }
            }

            state
                .db
                .save_provider(app_type.as_str(), &updated_provider)?;
        }

        Ok(())
    }

    pub fn migrate_legacy_common_config_usage_if_needed(
        state: &AppState,
        app_type: AppType,
    ) -> Result<(), AppError> {
        if app_type.is_additive_mode() {
            return Ok(());
        }

        let Some(snippet) = state.db.get_config_snippet(app_type.as_str())? else {
            return Ok(());
        };

        if snippet.trim().is_empty() {
            return Ok(());
        }

        Self::migrate_legacy_common_config_usage(state, app_type, &snippet)
    }

    /// Extract common config snippet from current provider
    ///
    /// Extracts the current provider's configuration and removes provider-specific fields
    /// (API keys, model settings, endpoints) to create a reusable common config snippet.
    pub fn extract_common_config_snippet(
        state: &AppState,
        app_type: AppType,
    ) -> Result<String, AppError> {
        // Get current provider
        let current_id = Self::current(state, app_type.clone())?;
        if current_id.is_empty() {
            return Err(AppError::Message("No current provider".to_string()));
        }

        let providers = state.db.get_all_providers(app_type.as_str())?;
        let provider = providers
            .get(&current_id)
            .ok_or_else(|| AppError::Message(format!("Provider {current_id} not found")))?;

        match app_type {
            AppType::Claude => Self::extract_claude_common_config(&provider.settings_config),
            AppType::ClaudeDesktop => Ok(String::new()),
            AppType::Codex => Self::extract_codex_common_config(&provider.settings_config),
            AppType::Gemini => Self::extract_gemini_common_config(&provider.settings_config),
            AppType::GrokBuild => Ok(String::new()),
            AppType::OpenCode => Self::extract_opencode_common_config(&provider.settings_config),
            AppType::OpenClaw => Self::extract_openclaw_common_config(&provider.settings_config),
            AppType::Hermes => Ok(String::new()), // Hermes doesn't use common config snippets
            AppType::Pi | AppType::Mcode => Ok(String::new()),
        }
    }

    /// Extract common config snippet from a config value (e.g. editor content).
    pub fn extract_common_config_snippet_from_settings(
        app_type: AppType,
        settings_config: &Value,
    ) -> Result<String, AppError> {
        match app_type {
            AppType::Claude => Self::extract_claude_common_config(settings_config),
            AppType::ClaudeDesktop => Ok(String::new()),
            AppType::Codex => Self::extract_codex_common_config(settings_config),
            AppType::Gemini => Self::extract_gemini_common_config(settings_config),
            AppType::GrokBuild => Ok(String::new()),
            AppType::OpenCode => Self::extract_opencode_common_config(settings_config),
            AppType::OpenClaw => Self::extract_openclaw_common_config(settings_config),
            AppType::Hermes => Ok(String::new()), // Hermes doesn't use common config snippets
            AppType::Pi | AppType::Mcode => Ok(String::new()),
        }
    }

    /// 判断一个 env / 顶层配置键名是否为凭据/机密：凡命中一律不得写入共享的
    /// 通用配置片段。**故意从严**——多剥一个非机密键只是它不被共享（可恢复的小
    /// 不便），漏剥一个凭据则会把密钥注入到每个供应商（不可恢复的泄漏）。因此用
    /// 模式匹配覆盖整类，而非枚举具体名字（枚举永远会漏掉下一个 `*_API_KEY`）。
    ///
    /// 覆盖：Anthropic / OpenRouter / Google / OpenAI / Gemini 等 `*_API_KEY`
    /// （Claude provider 的凭据见 `Provider::resolve_usage_credentials`，确实支持
    /// `OPENROUTER_API_KEY` / `GOOGLE_API_KEY` 等回退）、各类 `*_AUTH_TOKEN` /
    /// 单数 `*_TOKEN`、AWS Bedrock / Vertex 凭据、通用 secret / password /
    /// 私钥命名，以及发往上游的自定义请求头、Cookie、Authorization。
    pub(crate) fn is_sensitive_config_key(name: &str) -> bool {
        let upper = name.to_ascii_uppercase();

        // 单数 `_TOKEN` 命中 AWS_SESSION_TOKEN 等，但**不**误伤复数 `_TOKENS`
        // （CLAUDE_CODE_MAX_OUTPUT_TOKENS / MAX_THINKING_TOKENS 是正常可共享配置）。
        const SENSITIVE_SUFFIXES: &[&str] = &[
            // 裸 `_KEY` 是最常见的凭据写法（OPENAI_KEY / GROQ_KEY / XAI_KEY…），
            // 必须单列：只枚举 `_API_KEY` / `_ACCESS_KEY` 这些子类，等于把最普通
            // 的那一种漏在外面。下面几条 `_*_KEY` 被它蕴含，保留是为了说明覆盖面。
            "_KEY",
            "_API_KEY",
            "_ACCESS_KEY",
            "_ACCESS_KEY_ID",
            "_KEY_ID",
            "_PRIVATE_KEY",
            // 不带分隔符的复合写法各走各的后缀：`_KEY` 够不着 `..._APIKEY`
            // （倒数第四个字符是 I 不是下划线）。VOLC_ACCESSKEY 是火山引擎文档
            // 里的正式变量名，本仓库就实现了火山 AK/SK 用量查询。
            "_APIKEY",
            "_ACCESSKEY",
            "_SECRETKEY",
            "_APITOKEN",
            "_AUTH_TOKEN",
            "_TOKEN",
            // GITHUB_PAT / GITLAB_PAT 等 personal access token 的惯用写法，
            // 既不含 TOKEN 也不含 KEY，前面每一条规则都够不着。
            "_PAT",
            // 口令类的常见缩写。`_PASS` 不会误伤 `*_BYPASS`（那个以 `_BYPASS`
            // 结尾），`_PWD` 也不会误伤 shell 的 PWD / OLDPWD。
            "_PWD",
            "_PASS",
            "_PASSPHRASE",
            "_CREDS",
            // 发往上游的自定义请求头（ANTHROPIC_CUSTOM_HEADERS、
            // GEMINI_CLI_CUSTOM_HEADERS）：常见写法是 `Authorization: Bearer …`
            // 或 `Cookie: …`，整串就是凭据。只认 `_CUSTOM_HEADERS`，不按 HEADER
            // 一刀切：CLAUDE_CODE_ATTRIBUTION_HEADER 是普通开关，
            // OTEL_EXPORTER_OTLP_HEADERS 发往用户自己的遥测端点，都应照常共享。
            "_CUSTOM_HEADERS",
        ];
        const SENSITIVE_EXACT: &[&str] = &[
            "APIKEY",
            "API_KEY",
            "TOKEN",
            "SECRET",
            "PASSWORD",
            "CREDENTIALS",
            "HEADERS",
        ];
        // contains：覆盖 AWS_SECRET_ACCESS_KEY / *_CLIENT_SECRET /
        // GOOGLE_APPLICATION_CREDENTIALS / AWS_BEARER_TOKEN_BEDROCK 等变体。
        const SENSITIVE_CONTAINS: &[&str] = &[
            "SECRET",
            "PASSWORD",
            "PASSWD",
            "CREDENTIAL",
            "PRIVATE_KEY",
            "BEARER_TOKEN",
            "COOKIE",
            "AUTHORIZATION",
        ];

        SENSITIVE_EXACT.contains(&upper.as_str())
            || SENSITIVE_SUFFIXES.iter().any(|s| upper.ends_with(s))
            || SENSITIVE_CONTAINS.iter().any(|c| upper.contains(c))
    }

    /// Claude `env` 里的键能否进通用配置片段。
    ///
    /// 片段会合并进每一家勾选了它的供应商，所以只能放与供应商无关的设置。关键字段
    /// （请求发到哪、凭什么鉴权、哪个模型名、哪种协议）一旦进了片段，就会跟着切换带给
    /// 下一家：从 Bedrock 切到官方后，官方的 live 里还留着 `CLAUDE_CODE_USE_BEDROCK=1`，
    /// Claude Code 继续走 Bedrock。凭据另由 `is_sensitive_config_key` 统一剥离。
    fn claude_env_key_is_shared(key: &str) -> bool {
        // Context limits follow the actual upstream model. Sharing these
        // across providers can cap GPT/Kimi to the wrong window and make
        // Claude Code compact too early or miss the upstream limit.
        const UPSTREAM_WINDOW_KEYS: &[&str] = &[
            "CLAUDE_CODE_MAX_CONTEXT_TOKENS",
            "CLAUDE_CODE_AUTO_COMPACT_WINDOW",
        ];

        !crate::live::floor::claude_floor_env(key)
            && !UPSTREAM_WINDOW_KEYS.contains(&key)
            && !Self::is_sensitive_config_key(key)
    }

    /// Claude 顶层键能否进通用配置片段，口径同 [`Self::claude_env_key_is_shared`]。
    /// `model` 是 `/model` 保存的选择，属于当时那一家。
    fn claude_top_key_is_shared(key: &str) -> bool {
        !crate::live::floor::claude_floor_top(key) && !Self::is_sensitive_config_key(key)
    }

    /// Extract common config for Claude (JSON format)
    fn extract_claude_common_config(settings: &Value) -> Result<String, AppError> {
        let mut config = settings.clone();

        if let Some(obj) = config.as_object_mut() {
            if let Some(Value::Object(env)) = obj.get_mut("env") {
                env.retain(|key, _| Self::claude_env_key_is_shared(key));
            }
            obj.retain(|key, value| match key.as_str() {
                "env" => !value.as_object().is_some_and(|env| env.is_empty()),
                _ => Self::claude_top_key_is_shared(key),
            });
        }

        // Check if result is empty
        if config.as_object().is_none_or(|obj| obj.is_empty()) {
            return Ok("{}".to_string());
        }

        serde_json::to_string_pretty(&config)
            .map_err(|e| AppError::Message(format!("Serialization failed: {e}")))
    }

    /// Extract common config for Codex (TOML format)
    fn extract_codex_common_config(settings: &Value) -> Result<String, AppError> {
        // Codex config is stored as { "auth": {...}, "config": "toml string" }
        let config_toml = settings
            .get("config")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if config_toml.is_empty() {
            return Ok(String::new());
        }

        let mut doc = config_toml
            .parse::<toml_edit::DocumentMut>()
            .map_err(|e| AppError::Message(format!("TOML parse error: {e}")))?;

        // 关键字段（选路、模型名、推理档位等，见 `live::floor`）归供应商，不进片段：
        // 片段已冻结，收进去的值会从行里被迁移剥掉、再也写不回 live。
        let root = doc.as_table_mut();
        for key in crate::live::floor::CODEX_FLOOR_TOP {
            root.remove(key);
        }
        for path in crate::live::floor::CODEX_FLOOR_NESTED {
            let [parent, key] = path else { continue };
            if let Some(table) = root
                .get_mut(parent)
                .and_then(|item| item.as_table_like_mut())
            {
                table.remove(key);
                if table.is_empty() {
                    root.remove(parent);
                }
            }
        }

        // Remove entire model_providers table (provider-specific configuration)
        root.remove("model_providers");

        // MCP 服务器归 DB mcp_servers 表所有：进了共享片段会绕过按应用的
        // 启用状态被合并进所有勾选通用配置的供应商，且在通用配置编辑框里
        // 显示为一份"重复"的 MCP 配置。
        root.remove("mcp_servers");
        // 历史错误格式 [mcp.servers] 一并剥离（与 strip_codex_mcp_servers_from_settings
        // 一致）：sync_all_enabled 只管理 [mcp_servers.*]，legacy 形态一旦进了
        // 片段就会被合并进所有供应商，且没有任何同步路径能清掉这个孤儿。
        if let Some(mcp_tbl) = root
            .get_mut("mcp")
            .and_then(|item| item.as_table_like_mut())
        {
            mcp_tbl.remove("servers");
            if mcp_tbl.is_empty() {
                root.remove("mcp");
            }
        }

        // cc-switch 写 live 时注入的产物一律不进共享片段：
        // - experimental_bearer_token 正常写在 [model_providers.<id>] 内（上面
        //   整表已剥），但无活跃路由 / 内建保留 id / 路由表缺失三种 fallback
        //   会落在顶层——不剥等于把 API 密钥写进共享片段。
        root.remove("experimental_bearer_token");
        // - model_catalog_json 指向按供应商生成的 catalog 投影文件（DB 为 SSOT）。
        root.remove("model_catalog_json");
        // - web_search 只剥 cc-switch 注入的 "disabled" 哨兵；用户手设的其它值
        //   属于可共享偏好，保留。
        if root
            .get(crate::codex_config::CODEX_WEB_SEARCH_FIELD)
            .and_then(|item| item.as_str())
            == Some(crate::codex_config::CODEX_WEB_SEARCH_DISABLED)
        {
            root.remove(crate::codex_config::CODEX_WEB_SEARCH_FIELD);
        }

        // Clean up multiple empty lines (keep at most one blank line).
        let mut cleaned = String::new();
        let mut blank_run = 0usize;
        for line in doc.to_string().lines() {
            if line.trim().is_empty() {
                blank_run += 1;
                if blank_run <= 1 {
                    cleaned.push('\n');
                }
                continue;
            }
            blank_run = 0;
            cleaned.push_str(line);
            cleaned.push('\n');
        }

        Ok(cleaned.trim().to_string())
    }

    /// Extract common config for Gemini (JSON format)
    ///
    /// Extracts `.env` values while excluding provider-specific credentials:
    /// - GOOGLE_GEMINI_BASE_URL
    /// - GEMINI_API_KEY
    fn extract_gemini_common_config(settings: &Value) -> Result<String, AppError> {
        let env = settings.get("env").and_then(|v| v.as_object());

        let mut snippet = serde_json::Map::new();
        if let Some(env) = env {
            for (key, value) in env {
                // 关键字段（地址、凭据、模型名、Vertex 选择器）归供应商，不进片段：
                // 片段已冻结，新版不再合并它，收进去的值会从行里被迁移剥掉、再也写不回
                // live（首启导入的 `GEMINI_MODEL` 就是这样丢的）。凭据另交给
                // `is_sensitive_config_key` 统一模式匹配（与 Claude 提取器一致），
                // 兜住关键字段清单以外的 `*_API_KEY`、`*_TOKEN`。
                if crate::live::floor::gemini_floor_env(key) || Self::is_sensitive_config_key(key) {
                    continue;
                }
                let Value::String(v) = value else {
                    continue;
                };
                let trimmed = v.trim();
                if !trimmed.is_empty() {
                    snippet.insert(key.to_string(), Value::String(trimmed.to_string()));
                }
            }
        }

        if snippet.is_empty() {
            return Ok("{}".to_string());
        }

        serde_json::to_string_pretty(&Value::Object(snippet))
            .map_err(|e| AppError::Message(format!("Serialization failed: {e}")))
    }

    /// 一次性清理：把历史泄漏进 Gemini 共享片段的凭据从所有存储位置抹掉。
    ///
    /// 背景：`extract_gemini_common_config` 曾只剥离两个固定键名，`GOOGLE_API_KEY`
    /// 等一等凭据会进入共享片段，再在切换时被深合并进
    /// **其它** Gemini 供应商的 env，随请求发往对方的 base_url。
    ///
    /// 光修提取器不够：Gemini 的片段一旦生成就**永不自动重提取**（启动期
    /// auto-extract 与导入后补提取都要求 `snippet.is_none()`，切换时的回写又只对
    /// Claude / Codex 生效），所以存量片段会一直带着密钥继续注入。
    ///
    /// 两个关键约束：
    ///
    /// 1. **不能只清片段**。合并与剥离是一对靠「值相等」严格抵消的操作：切走供应商时
    ///    `remove_common_config_from_settings` 依据片段内容把注入的键删掉。片段里一旦
    ///    没了这个键，backfill 就会把 live 中残留的密钥原样写进受害供应商的
    ///    `settings_config`——泄漏从瞬时污染变成永久污染。所以片段、各供应商配置、
    ///    live 文件必须一起清。
    /// 2. **按值相等定向删除，不按键名一刀切**。复用 `remove_common_config_from_settings`
    ///    可以只清掉扩散出去的那一份，保留某个供应商自己写的、值不同的同名键。
    ///
    /// 步骤顺序本身是安全属性的一部分：**清片段必须排在最后**。片段是
    /// `remove_common_config_from_settings` 唯一的"该剥哪些键"来源，一旦清空，任何
    /// 残留（live 文件里的、下一轮重试要处理的）都再也无法被识别和剥离。所以所有
    /// 可能失败的步骤都排在它前面，失败即带错返回，让下次启动能原样重来。
    ///
    /// 清理后部分供应商会显示缺少 API Key，需用户重填——这是正确行为：那把密钥本就
    /// 不属于它们。（受害者原有的同名键在合并时已被覆盖，无法恢复。）动手前会往
    /// settings 的 `gemini_common_config_scrub_audit_v1` 写一条审计记录，内容是
    /// **键名与受影响的供应商 id，不含值**：`settings` 会随 WebDAV/S3 同步上传，
    /// 而这里处理的正是必须销毁的凭据，留值等于把一次清除换成一份跨设备扩散、
    /// 没有界面入口、永不过期的明文副本。
    pub async fn scrub_leaked_gemini_common_config(state: &AppState) -> Result<(), AppError> {
        const FLAG: &str = "gemini_common_config_credentials_scrubbed_v1";
        const AUDIT_KEY: &str = "gemini_common_config_scrub_audit_v1";
        let app = AppType::Gemini;

        if state.db.get_bool_flag(FLAG).unwrap_or(false) {
            return Ok(());
        }

        let Some(snippet_text) = state.db.get_config_snippet(app.as_str())? else {
            state.db.set_setting(FLAG, "true")?;
            return Ok(());
        };

        // 片段解析不了就不动它，只标记完成——乱改用户数据比留着更糟
        let Ok(Value::Object(entries)) = serde_json::from_str::<Value>(&snippet_text) else {
            state.db.set_setting(FLAG, "true")?;
            return Ok(());
        };

        let mut poison = serde_json::Map::new();
        let mut clean = serde_json::Map::new();
        for (key, value) in entries {
            if Self::is_sensitive_config_key(&key) {
                poison.insert(key, value);
            } else {
                clean.insert(key, value);
            }
        }

        if poison.is_empty() {
            state.db.set_setting(FLAG, "true")?;
            return Ok(());
        }

        log::warn!(
            "检测到 {} 个凭据键残留在 Gemini 通用配置片段中，开始一次性清理",
            poison.len()
        );

        let poison_keys: Vec<String> = poison.keys().cloned().collect();
        let poison_value = Value::Object(poison);
        let poison_text = serde_json::to_string(&poison_value)
            .map_err(|e| AppError::Message(format!("Serialization failed: {e}")))?;

        // 1) 先算出各供应商清理后的配置，但**先不落库**
        let providers = state.db.get_all_providers(app.as_str())?;
        let mut pending: Vec<(String, Provider, Value)> = Vec::new();
        for (id, provider) in providers {
            let cleaned = match live::remove_common_config_from_settings(
                &app,
                &provider.settings_config,
                &poison_text,
            ) {
                Ok(cleaned) => cleaned,
                Err(err) => {
                    log::warn!("清理供应商 '{id}' 的泄漏凭据失败: {err}");
                    continue;
                }
            };
            if cleaned != provider.settings_config {
                pending.push((id, provider, cleaned));
            }
        }

        // 2) 落库前留一份审计记录：**只记键名与受影响的供应商，不记值**。
        //
        //    「按值相等定向删除」在一种合法场景下也会命中：用户有意在多个供应商里
        //    复用同一把 key。所以必须留下"删了什么、从哪删的"，否则用户只能靠翻
        //    日志。但不能留值——`settings` 表不在 `SYNC_SKIP_TABLES` 里，会随
        //    WebDAV/S3 同步上传，而这里处理的恰恰是必须销毁的泄漏凭据：留值等于
        //    把一次清除换成一份没有界面入口、永不过期、还会跨设备扩散的明文副本。
        //    密钥本来就该轮换，可恢复性不值这个代价。
        let removed_env_keys = |before: &Value, after: &Value| -> Vec<String> {
            let before_env = before.get("env").and_then(Value::as_object);
            let after_env = after.get("env").and_then(Value::as_object);
            match (before_env, after_env) {
                (Some(before_env), Some(after_env)) => before_env
                    .keys()
                    .filter(|key| !after_env.contains_key(*key))
                    .cloned()
                    .collect(),
                (Some(before_env), None) => before_env.keys().cloned().collect(),
                _ => Vec::new(),
            }
        };
        let audit = serde_json::json!({
            "removedFromSnippet": poison_keys,
            "providers": pending
                .iter()
                .map(|(id, provider, cleaned)| serde_json::json!({
                    "id": id,
                    "removedKeys": removed_env_keys(&provider.settings_config, cleaned),
                }))
                .collect::<Vec<_>>(),
        });
        let audit_text = serde_json::to_string(&audit)
            .map_err(|e| AppError::Message(format!("Serialization failed: {e}")))?;
        // 只在没有记录时写。provider 的写入不是一个事务（每次 save_provider 各自
        // 提交），上一轮可能改到一半就中止；此时完成标记没置位，下次启动会重跑，
        // 而重跑看到的"原始状态"已经残缺。无条件 INSERT OR REPLACE 会拿这份残缺
        // 记录盖掉第一轮那份完整的。
        if state.db.get_setting(AUDIT_KEY)?.is_none() {
            state.db.set_setting(AUDIT_KEY, &audit_text)?;
        }

        // 3) 各供应商 settings_config：按值相等定向删除扩散出去的副本
        for (id, provider, cleaned) in pending {
            let mut updated = provider;
            updated.settings_config = cleaned;
            state.db.save_provider(app.as_str(), &updated)?;
            log::info!("已从 Gemini 供应商 '{id}' 中清除泄漏的共享凭据");
        }

        // 4) 旧版接管留下的 live 快照（备份行）里也可能有一份副本。这一步的失败**必须
        //    传播**：
        //
        //    新版启动时不回放这份快照（转存到本机文件后删除），但同一次启动里它排在
        //    这里之后；在那之前降级的话，旧版停代理时会把快照原样写回 `~/.gemini/.env`。
        //    若它仍带毒而我们照样清了片段、置了完成标记，凭据就会复活，而一次性标记又
        //    保证不会再清第二次。
        //
        //    带错返回是安全的失败方式：调用方（lib.rs:1189）只记 warn 不中断启动，
        //    片段和标记都原样留着，下次启动照原样重来。
        if let Some(backup) = state.db.get_live_backup(app.as_str()).await? {
            let original: Value = serde_json::from_str(&backup.original_config)
                .map_err(|e| AppError::Message(format!("解析 Gemini 代理接管备份失败: {e}")))?;
            let cleaned = live::remove_common_config_from_settings(&app, &original, &poison_text)?;
            if cleaned != original {
                let text = serde_json::to_string(&cleaned)
                    .map_err(|e| AppError::Message(format!("Serialization failed: {e}")))?;
                state.db.save_live_backup(app.as_str(), &text).await?;
                log::info!("已从 Gemini 代理接管备份中清除泄漏的共享凭据");
            }
        }

        // 5) `~/.gemini/.env`：**定向**删除，且必须在清片段之前做，失败即中止。
        //
        //    为什么不用 `sync_current_provider_for_app` 重投影：它在没有当前供应商
        //    时直接返回 Ok 而根本不写文件，泄漏值会原样留在 live 里；等片段被清空
        //    之后，下次切换时 `remove_common_config_from_settings` 再也认不出这个
        //    键，backfill 就把它永久写进受害供应商的配置——正是本函数开头说的那个
        //    顺序陷阱，只是由"没修"变成"修了一半更糟"。定向删除还顺带保住了只存在
        //    于 live、与供应商无关的手工 env（重投影会把它们抹掉）。
        //
        //    删除走 `remove_gemini_env_entries` 的**保序**实现而不是 read→HashMap→
        //    write 往返：后者会顺手抹掉注释、空行和无法识别的行，并按键名重排整个
        //    文件。全量投影时那无所谓，但这里是一次用户没主动触发的启动期清理，不该
        //    连带改写与泄漏无关的内容。
        //
        //    失败就带着错误返回：片段此刻还留着毒键，完成标记也没置位，下次启动能
        //    照原样重来。清片段是不可逆的一步，必须排在所有会失败的步骤之后。
        let poison_env: HashMap<String, String> = poison_value
            .as_object()
            .map(|map| {
                map.iter()
                    .filter_map(|(key, value)| {
                        value.as_str().map(|text| (key.clone(), text.to_string()))
                    })
                    .collect()
            })
            .unwrap_or_default();
        if crate::gemini_config::remove_gemini_env_entries(&poison_env)? {
            log::info!("已从 ~/.gemini/.env 中清除泄漏的共享凭据");
        }

        // 6) 片段本身：保留可共享的部分。全部清空时删行而不是写 "{}"——留着空行会让
        //    should_auto_extract_config_snippet 永远为 false，用户的合法共享配置再也
        //    重建不回来。同理绝不置 cleared 标记。
        if clean.is_empty() {
            state.db.set_config_snippet(app.as_str(), None)?;
        } else {
            let cleaned_snippet = serde_json::to_string_pretty(&Value::Object(clean))
                .map_err(|e| AppError::Message(format!("Serialization failed: {e}")))?;
            state
                .db
                .set_config_snippet(app.as_str(), Some(cleaned_snippet))?;
        }

        state.db.set_setting(FLAG, "true")?;
        log::info!("Gemini 通用配置凭据清理完成");
        Ok(())
    }

    /// Extract common config for OpenCode (JSON format)
    fn extract_opencode_common_config(settings: &Value) -> Result<String, AppError> {
        // OpenCode uses a different config structure with npm, options, models
        // For common config, we exclude provider-specific fields like apiKey
        let mut config = settings.clone();

        // Remove provider-specific fields
        if let Some(obj) = config.as_object_mut() {
            for key in ["options", "settings"] {
                if let Some(options) = obj.get_mut(key).and_then(|v| v.as_object_mut()) {
                    options.remove("apiKey");
                    options.remove("baseURL");
                }
            }
            // Keep npm and models as they might be common
        }

        if config.is_null() || (config.is_object() && config.as_object().unwrap().is_empty()) {
            return Ok("{}".to_string());
        }

        serde_json::to_string_pretty(&config)
            .map_err(|e| AppError::Message(format!("Serialization failed: {e}")))
    }

    /// Extract common config for OpenClaw (JSON format)
    fn extract_openclaw_common_config(settings: &Value) -> Result<String, AppError> {
        // OpenClaw uses a different config structure with baseUrl, apiKey, api, models
        // For common config, we exclude provider-specific fields like apiKey
        let mut config = settings.clone();

        // Remove provider-specific fields
        if let Some(obj) = config.as_object_mut() {
            obj.remove("apiKey");
            obj.remove("baseUrl");
            // Keep api and models as they might be common
        }

        if config.is_null() || (config.is_object() && config.as_object().unwrap().is_empty()) {
            return Ok("{}".to_string());
        }

        serde_json::to_string_pretty(&config)
            .map_err(|e| AppError::Message(format!("Serialization failed: {e}")))
    }

    /// Import default configuration from live files (re-export)
    ///
    /// Returns `Ok(true)` if imported, `Ok(false)` if skipped.
    pub fn import_default_config(state: &AppState, app_type: AppType) -> Result<bool, AppError> {
        import_default_config(state, app_type)
    }

    pub fn should_import_default_config_on_startup(
        state: &AppState,
        app_type: &AppType,
    ) -> Result<bool, AppError> {
        should_import_default_config_on_startup(state, app_type)
    }

    /// Read current live settings (re-export)
    pub fn read_live_settings(app_type: AppType) -> Result<Value, AppError> {
        read_live_settings(app_type)
    }

    /// Get custom endpoints list (re-export)
    pub fn get_custom_endpoints(
        state: &AppState,
        app_type: AppType,
        provider_id: &str,
    ) -> Result<Vec<CustomEndpoint>, AppError> {
        endpoints::get_custom_endpoints(state, app_type, provider_id)
    }

    /// Add custom endpoint (re-export)
    pub fn add_custom_endpoint(
        state: &AppState,
        app_type: AppType,
        provider_id: &str,
        url: String,
    ) -> Result<(), AppError> {
        endpoints::add_custom_endpoint(state, app_type, provider_id, url)
    }

    /// Remove custom endpoint (re-export)
    pub fn remove_custom_endpoint(
        state: &AppState,
        app_type: AppType,
        provider_id: &str,
        url: String,
    ) -> Result<(), AppError> {
        endpoints::remove_custom_endpoint(state, app_type, provider_id, url)
    }

    /// Update endpoint last used timestamp (re-export)
    pub fn update_endpoint_last_used(
        state: &AppState,
        app_type: AppType,
        provider_id: &str,
        url: String,
    ) -> Result<(), AppError> {
        endpoints::update_endpoint_last_used(state, app_type, provider_id, url)
    }

    /// Update provider sort order
    pub fn update_sort_order(
        state: &AppState,
        app_type: AppType,
        updates: Vec<ProviderSortUpdate>,
    ) -> Result<bool, AppError> {
        let mut providers = state.db.get_all_providers(app_type.as_str())?;

        for update in updates {
            if let Some(provider) = providers.get_mut(&update.id) {
                provider.sort_index = Some(update.sort_index);
                state.db.save_provider(app_type.as_str(), provider)?;
            }
        }

        Ok(true)
    }

    /// Query provider usage (re-export)
    pub async fn query_usage(
        state: &AppState,
        app_type: AppType,
        provider_id: &str,
    ) -> Result<UsageResult, AppError> {
        usage::query_usage(state, app_type, provider_id).await
    }

    /// Test usage script (re-export)
    #[allow(clippy::too_many_arguments)]
    pub async fn test_usage_script(
        state: &AppState,
        app_type: AppType,
        provider_id: &str,
        script_code: &str,
        timeout: u64,
        api_key: Option<&str>,
        base_url: Option<&str>,
        access_token: Option<&str>,
        user_id: Option<&str>,
        template_type: Option<&str>,
    ) -> Result<UsageResult, AppError> {
        usage::test_usage_script(
            state,
            app_type,
            provider_id,
            script_code,
            timeout,
            api_key,
            base_url,
            access_token,
            user_id,
            template_type,
        )
        .await
    }

    fn validate_provider_settings(app_type: &AppType, provider: &Provider) -> Result<(), AppError> {
        match app_type {
            AppType::Claude => {
                if !provider.settings_config.is_object() {
                    return Err(AppError::localized(
                        "provider.claude.settings.not_object",
                        "Claude 配置必须是 JSON 对象",
                        "Claude configuration must be a JSON object",
                    ));
                }
            }
            AppType::ClaudeDesktop => {
                crate::claude_desktop_config::validate_provider(provider)?;
            }
            AppType::Codex => {
                let settings = provider.settings_config.as_object().ok_or_else(|| {
                    AppError::localized(
                        "provider.codex.settings.not_object",
                        "Codex 配置必须是 JSON 对象",
                        "Codex configuration must be a JSON object",
                    )
                })?;

                let auth = settings.get("auth").ok_or_else(|| {
                    AppError::localized(
                        "provider.codex.auth.missing",
                        format!("供应商 {} 缺少 auth 配置", provider.id),
                        format!("Provider {} is missing auth configuration", provider.id),
                    )
                })?;
                if !auth.is_object() {
                    return Err(AppError::localized(
                        "provider.codex.auth.not_object",
                        format!("供应商 {} 的 auth 配置必须是 JSON 对象", provider.id),
                        format!(
                            "Provider {} auth configuration must be a JSON object",
                            provider.id
                        ),
                    ));
                }

                if let Some(config_value) = settings.get("config") {
                    if !(config_value.is_string() || config_value.is_null()) {
                        return Err(AppError::localized(
                            "provider.codex.config.invalid_type",
                            "Codex config 字段必须是字符串",
                            "Codex config field must be a string",
                        ));
                    }
                    if let Some(cfg_text) = config_value.as_str() {
                        crate::codex_config::validate_config_toml(cfg_text)?;
                    }
                }
            }
            AppType::Gemini => {
                use crate::gemini_config::validate_gemini_settings;
                validate_gemini_settings(&provider.settings_config)?
            }
            AppType::GrokBuild => {
                let settings = provider.settings_config.as_object().ok_or_else(|| {
                    AppError::localized(
                        "provider.grokbuild.settings.not_object",
                        "Grok Build 配置必须是 JSON 对象",
                        "Grok Build configuration must be a JSON object",
                    )
                })?;
                let config = settings
                    .get("config")
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        AppError::localized(
                            "provider.grokbuild.config.missing",
                            "Grok Build 配置缺少 config 字段",
                            "Grok Build configuration is missing the config field",
                        )
                    })?;
                if provider.category.as_deref() == Some("official") {
                    // 官方条目走 Grok CLI 自带 OAuth：空 config 合法，
                    // 其余内容只要求 TOML 语法合法。
                    crate::grok_config::validate_config_toml_syntax(config)?;
                } else {
                    crate::grok_config::validate_config_toml(config)?;
                }
            }
            AppType::OpenCode => {
                // OpenCode uses a different config structure: { npm, options, models }
                // Basic validation - must be an object
                if !provider.settings_config.is_object() {
                    return Err(AppError::localized(
                        "provider.opencode.settings.not_object",
                        "OpenCode 配置必须是 JSON 对象",
                        "OpenCode configuration must be a JSON object",
                    ));
                }
                // The row is saved before the live write, which would refuse an invalid
                // native declaration only after the database already holds it.
                if !matches!(provider.category.as_deref(), Some("omo") | Some("omo-slim")) {
                    let (fragment, format) = crate::opencode_config::provider_fragment(
                        &provider.id,
                        &provider.settings_config,
                        provider.opencode_config_format(),
                    )?;
                    if format == crate::provider::OpenCodeConfigFormat::V2 {
                        crate::opencode_config::validate_native_provider(&provider.id, fragment)?;
                    }
                }
            }
            AppType::OpenClaw => {
                // OpenClaw uses config structure: { baseUrl, apiKey, api, models }
                // Basic validation - must be an object
                if !provider.settings_config.is_object() {
                    return Err(AppError::localized(
                        "provider.openclaw.settings.not_object",
                        "OpenClaw 配置必须是 JSON 对象",
                        "OpenClaw configuration must be a JSON object",
                    ));
                }
            }
            AppType::Hermes => {
                // Hermes: accept any JSON object for now
                if !provider.settings_config.is_object() {
                    return Err(AppError::localized(
                        "provider.hermes.settings.not_object",
                        "Hermes 配置必须是 JSON 对象",
                        "Hermes configuration must be a JSON object",
                    ));
                }
            }
            AppType::Mcode => {
                crate::mcode_config::validate_provider(&provider.id, &provider.settings_config)?
            }
            AppType::Pi => {
                crate::pi_config::validate_provider_node(&provider.id, &provider.settings_config)?;
            }
        }

        // Validate and clean UsageScript configuration (common for all app types)
        if let Some(meta) = &provider.meta {
            if let Some(usage_script) = &meta.usage_script {
                validate_usage_script(usage_script)?;
            }
        }

        Ok(())
    }

    #[allow(dead_code)]
    fn extract_credentials(
        provider: &Provider,
        app_type: &AppType,
    ) -> Result<(String, String), AppError> {
        match app_type {
            AppType::Claude => {
                let env = provider
                    .settings_config
                    .get("env")
                    .and_then(|v| v.as_object())
                    .ok_or_else(|| {
                        AppError::localized(
                            "provider.claude.env.missing",
                            "配置格式错误: 缺少 env",
                            "Invalid configuration: missing env section",
                        )
                    })?;

                let api_key = env
                    .get("ANTHROPIC_AUTH_TOKEN")
                    .or_else(|| env.get("ANTHROPIC_API_KEY"))
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        AppError::localized(
                            "provider.claude.api_key.missing",
                            "缺少 API Key",
                            "API key is missing",
                        )
                    })?
                    .to_string();

                let base_url = env
                    .get("ANTHROPIC_BASE_URL")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        AppError::localized(
                            "provider.claude.base_url.missing",
                            "缺少 ANTHROPIC_BASE_URL 配置",
                            "Missing ANTHROPIC_BASE_URL configuration",
                        )
                    })?
                    .to_string();

                Ok((api_key, base_url))
            }
            AppType::GrokBuild => {
                let config_toml = provider
                    .settings_config
                    .get("config")
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        AppError::localized(
                            "provider.grokbuild.config.missing",
                            "Grok Build 配置缺少 config 字段",
                            "Grok Build configuration is missing the config field",
                        )
                    })?;
                let (base_url, api_key) = crate::grok_config::extract_credentials(config_toml)
                    .ok_or_else(|| {
                        AppError::localized(
                            "provider.grokbuild.credentials.missing",
                            "Grok Build 配置缺少 Base URL 或 API Key",
                            "Grok Build configuration is missing the base URL or API key",
                        )
                    })?;
                Ok((api_key, base_url))
            }
            AppType::ClaudeDesktop => {
                let credentials =
                    crate::claude_desktop_config::direct_gateway_credentials(provider)?;
                Ok((credentials.api_key, credentials.base_url))
            }
            AppType::Codex => {
                let _auth = provider
                    .settings_config
                    .get("auth")
                    .and_then(|v| v.as_object())
                    .ok_or_else(|| {
                        AppError::localized(
                            "provider.codex.auth.missing",
                            "配置格式错误: 缺少 auth",
                            "Invalid configuration: missing auth section",
                        )
                    })?;

                let config_toml = provider
                    .settings_config
                    .get("config")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                let api_key = crate::codex_config::extract_codex_api_key(
                    provider.settings_config.get("auth"),
                    Some(config_toml),
                )
                .ok_or_else(|| {
                    AppError::localized(
                        "provider.codex.api_key.missing",
                        "缺少 API Key",
                        "API key is missing",
                    )
                })?;

                let base_url = if config_toml.contains("base_url") {
                    let re = Regex::new(r#"base_url\s*=\s*["']([^"']+)["']"#).map_err(|e| {
                        AppError::localized(
                            "provider.regex_init_failed",
                            format!("正则初始化失败: {e}"),
                            format!("Failed to initialize regex: {e}"),
                        )
                    })?;
                    re.captures(config_toml)
                        .and_then(|caps| caps.get(1))
                        .map(|m| m.as_str().to_string())
                        .ok_or_else(|| {
                            AppError::localized(
                                "provider.codex.base_url.invalid",
                                "config.toml 中 base_url 格式错误",
                                "base_url in config.toml has invalid format",
                            )
                        })?
                } else {
                    return Err(AppError::localized(
                        "provider.codex.base_url.missing",
                        "config.toml 中缺少 base_url 配置",
                        "base_url is missing from config.toml",
                    ));
                };

                Ok((api_key, base_url))
            }
            AppType::Gemini => {
                use crate::gemini_config::json_to_env;

                let env_map = json_to_env(&provider.settings_config)?;

                let api_key = env_map.get("GEMINI_API_KEY").cloned().ok_or_else(|| {
                    AppError::localized(
                        "gemini.missing_api_key",
                        "缺少 GEMINI_API_KEY",
                        "Missing GEMINI_API_KEY",
                    )
                })?;

                let base_url = env_map
                    .get("GOOGLE_GEMINI_BASE_URL")
                    .cloned()
                    .unwrap_or_else(|| "https://generativelanguage.googleapis.com".to_string());

                Ok((api_key, base_url))
            }
            AppType::OpenCode => {
                // OpenCode uses options.apiKey and options.baseURL
                let options = provider
                    .settings_config
                    .get("options")
                    .and_then(|v| v.as_object())
                    .ok_or_else(|| {
                        AppError::localized(
                            "provider.opencode.options.missing",
                            "配置格式错误: 缺少 options",
                            "Invalid configuration: missing options section",
                        )
                    })?;

                let api_key = options
                    .get("apiKey")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        AppError::localized(
                            "provider.opencode.api_key.missing",
                            "缺少 API Key",
                            "API key is missing",
                        )
                    })?
                    .to_string();

                let base_url = options
                    .get("baseURL")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();

                Ok((api_key, base_url))
            }
            AppType::OpenClaw | AppType::Hermes | AppType::Pi | AppType::Mcode => {
                // These native formats use apiKey and baseUrl directly on the object.
                let api_key = provider
                    .settings_config
                    .get("apiKey")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        AppError::localized(
                            "provider.openclaw.api_key.missing",
                            "缺少 API Key",
                            "API key is missing",
                        )
                    })?
                    .to_string();

                let base_url = provider
                    .settings_config
                    .get("baseUrl")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();

                Ok((api_key, base_url))
            }
        }
    }
}

/// Normalize Claude model keys in a JSON value
///
/// Reads old key (ANTHROPIC_SMALL_FAST_MODEL), writes new keys (DEFAULT_*), and deletes old key.
pub(crate) fn normalize_claude_models_in_value(settings: &mut Value) -> bool {
    let mut changed = false;
    let env = match settings.get_mut("env").and_then(|v| v.as_object_mut()) {
        Some(obj) => obj,
        None => return changed,
    };

    let model = env
        .get("ANTHROPIC_MODEL")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let small_fast = env
        .get("ANTHROPIC_SMALL_FAST_MODEL")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let current_haiku = env
        .get("ANTHROPIC_DEFAULT_HAIKU_MODEL")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let current_sonnet = env
        .get("ANTHROPIC_DEFAULT_SONNET_MODEL")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let current_opus = env
        .get("ANTHROPIC_DEFAULT_OPUS_MODEL")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let target_haiku = current_haiku
        .or_else(|| small_fast.clone())
        .or_else(|| model.clone());
    let target_sonnet = current_sonnet
        .or_else(|| model.clone())
        .or_else(|| small_fast.clone());
    let target_opus = current_opus
        .or_else(|| model.clone())
        .or_else(|| small_fast.clone());

    if env.get("ANTHROPIC_DEFAULT_HAIKU_MODEL").is_none() {
        if let Some(v) = target_haiku {
            env.insert(
                "ANTHROPIC_DEFAULT_HAIKU_MODEL".to_string(),
                Value::String(v),
            );
            changed = true;
        }
    }
    if env.get("ANTHROPIC_DEFAULT_SONNET_MODEL").is_none() {
        if let Some(v) = target_sonnet {
            env.insert(
                "ANTHROPIC_DEFAULT_SONNET_MODEL".to_string(),
                Value::String(v),
            );
            changed = true;
        }
    }
    if env.get("ANTHROPIC_DEFAULT_OPUS_MODEL").is_none() {
        if let Some(v) = target_opus {
            env.insert("ANTHROPIC_DEFAULT_OPUS_MODEL".to_string(), Value::String(v));
            changed = true;
        }
    }

    if env.remove("ANTHROPIC_SMALL_FAST_MODEL").is_some() {
        changed = true;
    }

    changed
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProviderSortUpdate {
    pub id: String,
    #[serde(rename = "sortIndex")]
    pub sort_index: usize,
}

// ============================================================================
// 统一供应商（Universal Provider）服务方法
// ============================================================================

use crate::provider::UniversalProvider;
use std::collections::HashMap;

impl ProviderService {
    /// 获取所有统一供应商
    pub fn list_universal(
        state: &AppState,
    ) -> Result<HashMap<String, UniversalProvider>, AppError> {
        state.db.get_all_universal_providers()
    }

    /// 获取单个统一供应商
    pub fn get_universal(
        state: &AppState,
        id: &str,
    ) -> Result<Option<UniversalProvider>, AppError> {
        state.db.get_universal_provider(id)
    }

    /// 添加或更新统一供应商（不自动同步，需手动调用 sync_universal_to_apps）
    pub fn upsert_universal(
        state: &AppState,
        provider: UniversalProvider,
    ) -> Result<bool, AppError> {
        // 保存统一供应商
        state.db.save_universal_provider(&provider)?;

        Ok(true)
    }

    /// 删除统一供应商
    pub fn delete_universal(state: &AppState, id: &str) -> Result<bool, AppError> {
        // 获取统一供应商（用于删除生成的子供应商）
        let provider = state.db.get_universal_provider(id)?;

        // 删除统一供应商
        state.db.delete_universal_provider(id)?;

        // 删除生成的子供应商
        if let Some(p) = provider {
            if p.apps.claude {
                let claude_id = format!("universal-claude-{id}");
                let _ = state.db.delete_provider("claude", &claude_id);
            }
            if p.apps.codex {
                let codex_id = format!("universal-codex-{id}");
                let _ = state.db.delete_provider("codex", &codex_id);
            }
            if p.apps.gemini {
                let gemini_id = format!("universal-gemini-{id}");
                let _ = state.db.delete_provider("gemini", &gemini_id);
            }
        }

        Ok(true)
    }

    /// 同步统一供应商到各应用
    pub fn sync_universal_to_apps(state: &AppState, id: &str) -> Result<bool, AppError> {
        let provider = state
            .db
            .get_universal_provider(id)?
            .ok_or_else(|| AppError::Message(format!("统一供应商 {id} 不存在")))?;

        // Keep DB and live projections in sync independently per application:
        // one broken config file must not prevent the other two apps from being
        // updated, but it must still be reported instead of returning success.
        let mut live_failures = Vec::new();

        // 同步到 Claude
        if let Some(mut claude_provider) = provider.to_claude_provider() {
            // 合并已有配置
            if let Some(existing) = state.db.get_provider_by_id(&claude_provider.id, "claude")? {
                let mut merged = existing.settings_config.clone();
                Self::merge_json(&mut merged, &claude_provider.settings_config);
                claude_provider.settings_config = merged;
                // 已有子供应商的应用专属配置与排序不属于统一供应商管理的字段。
                claude_provider.meta = existing.meta;
                claude_provider.created_at = existing.created_at;
                claude_provider.sort_index = existing.sort_index;
            }
            state.db.save_provider("claude", &claude_provider)?;
            Self::project_universal_child_to_live(
                state,
                AppType::Claude,
                &claude_provider.id,
                &mut live_failures,
            );
        } else {
            // 如果禁用了 Claude，删除对应的子供应商
            let claude_id = format!("universal-claude-{id}");
            let _ = state.db.delete_provider("claude", &claude_id);
        }

        // 同步到 Codex
        if let Some(mut codex_provider) = provider.to_codex_provider() {
            // 合并已有配置
            if let Some(existing) = state.db.get_provider_by_id(&codex_provider.id, "codex")? {
                let mut merged = existing.settings_config.clone();
                Self::merge_json(&mut merged, &codex_provider.settings_config);
                codex_provider.settings_config = merged;
                // 已有子供应商的应用专属配置与排序不属于统一供应商管理的字段。
                codex_provider.meta = existing.meta;
                codex_provider.created_at = existing.created_at;
                codex_provider.sort_index = existing.sort_index;
            }
            state.db.save_provider("codex", &codex_provider)?;
            Self::project_universal_child_to_live(
                state,
                AppType::Codex,
                &codex_provider.id,
                &mut live_failures,
            );
        } else {
            let codex_id = format!("universal-codex-{id}");
            let _ = state.db.delete_provider("codex", &codex_id);
        }

        // 同步到 Gemini
        if let Some(mut gemini_provider) = provider.to_gemini_provider() {
            // 合并已有配置
            if let Some(existing) = state.db.get_provider_by_id(&gemini_provider.id, "gemini")? {
                let mut merged = existing.settings_config.clone();
                Self::merge_json(&mut merged, &gemini_provider.settings_config);
                gemini_provider.settings_config = merged;
                // 已有子供应商的应用专属配置与排序不属于统一供应商管理的字段。
                gemini_provider.meta = existing.meta;
                gemini_provider.created_at = existing.created_at;
                gemini_provider.sort_index = existing.sort_index;
            }
            state.db.save_provider("gemini", &gemini_provider)?;
            Self::project_universal_child_to_live(
                state,
                AppType::Gemini,
                &gemini_provider.id,
                &mut live_failures,
            );
        } else {
            let gemini_id = format!("universal-gemini-{id}");
            let _ = state.db.delete_provider("gemini", &gemini_id);
        }

        if live_failures.is_empty() {
            Ok(true)
        } else {
            Err(AppError::Message(format!(
                "统一供应商已保存到数据库，但以下应用的配置文件未能写入，仍是旧内容：{}。请重试同步，或切换一次该应用的供应商。",
                live_failures.join("、")
            )))
        }
    }

    /// Re-project a generated universal child only when it is the effective
    /// current provider for that app. Failures are collected by the caller so
    /// the other applications can continue syncing.
    fn project_universal_child_to_live(
        state: &AppState,
        app_type: AppType,
        child_id: &str,
        failures: &mut Vec<String>,
    ) {
        // 正在用的那家（代理模式下是代理路由）才需要重投影。
        let is_current = match crate::mode::current::provider_for(
            &state.db,
            &app_type,
            crate::mode::current::Purpose::InUse,
        ) {
            Ok(current) => current.as_deref() == Some(child_id),
            Err(err) => {
                log::warn!(
                    "读取 {} 当前供应商失败，跳过统一供应商的 live 重投影: {err}",
                    app_type.as_str()
                );
                failures.push(app_type.as_str().to_string());
                return;
            }
        };
        if !is_current {
            return;
        }

        if let Err(err) = Self::sync_current_provider_for_app(state, app_type.clone()) {
            log::warn!(
                "统一供应商同步后重写 {} live 配置失败: {err}",
                app_type.as_str()
            );
            failures.push(app_type.as_str().to_string());
        }
    }

    /// 递归合并 JSON：base 为底，patch 覆盖同名字段
    fn merge_json(base: &mut serde_json::Value, patch: &serde_json::Value) {
        use serde_json::Value;

        match (base, patch) {
            (Value::Object(base_map), Value::Object(patch_map)) => {
                for (k, v_patch) in patch_map {
                    match base_map.get_mut(k) {
                        Some(v_base) => Self::merge_json(v_base, v_patch),
                        None => {
                            base_map.insert(k.clone(), v_patch.clone());
                        }
                    }
                }
            }
            // 其它类型：直接覆盖
            (base_val, patch_val) => {
                *base_val = patch_val.clone();
            }
        }
    }
}
