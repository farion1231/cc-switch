//! Provider service module
//!
//! Handles provider CRUD operations, switching, and configuration management.

mod endpoints;
mod gemini_auth;
mod live;
mod pi;
mod usage;

use indexmap::IndexMap;
use regex::Regex;
use serde::Deserialize;
use serde_json::Value;

use crate::app_config::AppType;
use crate::database::{validate_cost_multiplier, validate_pricing_source};
use crate::error::AppError;
use crate::provider::{Provider, UsageResult};
use crate::services::mcp::McpService;
use crate::settings::CustomEndpoint;
use crate::store::AppState;

// Re-export sub-module functions for external access
pub use live::{
    import_default_config, import_hermes_providers_from_live, import_openclaw_providers_from_live,
    import_opencode_providers_from_live, read_live_settings,
    should_import_default_config_on_startup, sync_current_to_live,
    update_toml_common_config_snippet,
};

pub fn import_pi_providers_from_live(state: &AppState) -> Result<usize, AppError> {
    pi::import_from_live(state)
}

// Internal re-exports (pub(crate))
pub(crate) use live::sanitize_claude_settings_for_live;
pub(crate) use live::{
    build_effective_provider_for_live_with_codex_oauth_manager,
    build_effective_settings_with_common_config, normalize_provider_common_config_for_storage,
    provider_exists_in_live_config, strip_common_config_from_live_settings,
    sync_current_provider_for_app_to_live, write_live_with_common_config_for_codex_oauth_manager,
    write_live_with_common_config_for_state, LiveSyncOutcome,
};

// Internal re-exports
use live::{
    remove_hermes_provider_from_live, remove_openclaw_provider_from_live,
    remove_opencode_provider_from_live, write_gemini_live,
};
use usage::validate_usage_script;

/// Codex official providers are safe to select during takeover: Codex keeps
/// ownership of the active ChatGPT login and the proxy only forwards the
/// authenticated request. Other apps' official providers retain the block.
pub fn official_provider_supports_proxy_takeover(app_type: &AppType, provider: &Provider) -> bool {
    matches!(app_type, AppType::Codex)
        && crate::proxy::providers::is_codex_official_provider(provider)
}

/// 统一会话开关变更后，立即按新开关状态重写当前官方 Codex 供应商的
/// live 配置，使开关即时生效（无需等下一次切换）。
/// 当前供应商非官方（或不存在）时为 no-op：注入只作用于官方配置，
/// 第三方 live 配置不受开关影响。
pub fn reapply_current_codex_official_live(state: &AppState) -> Result<bool, AppError> {
    let current_id = ProviderService::current(state, AppType::Codex)?;
    if current_id.is_empty() {
        return Ok(false);
    }
    let providers = state.db.get_all_providers(AppType::Codex.as_str())?;
    let Some(provider) = providers.get(&current_id) else {
        return Ok(false);
    };
    if provider.category.as_deref() != Some("official")
        && !crate::proxy::providers::is_codex_official_provider(provider)
    {
        return Ok(false);
    }

    // 代理接管期间 live 归代理所有（开启代理时官方供应商只警告不拦截，
    // 二者可以共存）。与切换/保存路径一致：以 backup/占位符为所有权信号，
    // 只更新备份，注入后的配置由接管释放时的恢复路径落盘。
    let outcome =
        live::sync_live_for_provider_respecting_takeover(state, &AppType::Codex, provider)?;
    if outcome == LiveSyncOutcome::BackupOnly {
        return Ok(true);
    }
    // 重写 live 会整体替换 config.toml（有意设计），[mcp_servers] 随之丢失，
    // 写完必须立刻从 DB 重新投影启用的 MCP。只投影 Codex 而非
    // sync_all_enabled：后者按 AppType::all() 顺序逐应用短路，排在 Codex
    // 前面的无关应用 live 损坏（如 ~/.claude.json 坏 JSON）会阻断 Codex
    // 的重投影，让刚被清掉的 [mcp_servers] 无人补回。
    // 投影失败降级为警告：走到这里 live 已按新开关状态落盘，开关事实上
    // 已生效；若把错误上抛，save_settings 会回滚开关设置，制造"设置=旧值、
    // live=新桶"的会话分裂——正是该回滚要防止的状态。MCP 投影可自愈
    // （下次切换 / 任一 MCP 启停操作都会重新投影）。
    if let Err(err) = McpService::sync_enabled_for_app(state, &AppType::Codex) {
        log::warn!("统一会话开关重写 live 后重投影 Codex MCP 失败（将在下次同步时自愈）: {err}");
    }
    Ok(true)
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
    #[cfg(any(target_os = "macos", windows))]
    use crate::claude_desktop_config::PROFILE_ID;
    use crate::config::{get_claude_settings_path, read_json_file, write_json_file};
    use crate::database::Database;
    use crate::provider::{
        AuthBinding, AuthBindingSource, ClaudeModelConfig, ProviderMeta, UniversalProvider,
        UsageScript,
    };
    #[cfg(any(target_os = "macos", windows))]
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
    }

    impl TempHome {
        fn new() -> Self {
            let dir = TempDir::new().expect("failed to create temp home");
            let original_home = env::var("HOME").ok();
            #[cfg(windows)]
            let original_local_app_data = env::var("LOCALAPPDATA").ok();
            let original_userprofile = env::var("USERPROFILE").ok();
            let original_test_home = env::var("CC_SWITCH_TEST_HOME").ok();

            env::set_var("HOME", dir.path());
            #[cfg(windows)]
            env::set_var("LOCALAPPDATA", dir.path().join("AppData").join("Local"));
            env::set_var("USERPROFILE", dir.path());
            env::set_var("CC_SWITCH_TEST_HOME", dir.path());

            Self {
                dir,
                original_home,
                #[cfg(windows)]
                original_local_app_data,
                original_userprofile,
                original_test_home,
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

            unbound.settings_config["config"] = Value::String(
                crate::codex_config::inject_codex_unified_session_bucket("")
                    .expect("inject live-only unified session route"),
            );
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
        // (live.rs::apply_common_config_to_settings)，因此任何凭据都不得进入片段。
        // 之前这里只硬编码跳过 GEMINI_API_KEY/GOOGLE_GEMINI_BASE_URL，而
        // GOOGLE_API_KEY 是 provider.rs 认可的一等 Gemini 凭据 → 会泄露到别的供应商。
        let settings = json!({
            "env": {
                "GEMINI_API_KEY": "g-gem",
                "GOOGLE_API_KEY": "g-legacy-real-key",
                "GOOGLE_GEMINI_BASE_URL": "https://gemini.example",
                "GOOGLE_APPLICATION_CREDENTIALS": "/path/creds.json",
                "SOME_PROXY_AUTH_TOKEN": "tok-proxy",
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
        ] {
            assert!(
                value.get(leaked).is_none(),
                "credential {leaked} must not leak into the shared Gemini snippet"
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
        ] {
            assert!(
                !ProviderService::is_sensitive_config_key(key),
                "{key} is ordinary shareable config and must not be stripped"
            );
        }
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
        write_live_with_common_config_for_state(&state, &AppType::Claude, &original)
            .expect("seed live file");

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
        write_live_with_common_config_for_state(&state, &AppType::Claude, &original)
            .expect("seed live file");
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
            Some("https://api.new.example")
        );
        let backup = db
            .get_live_backup("claude")
            .await
            .expect("read backup")
            .expect("backup remains");
        assert!(backup.original_config.contains("https://api.new.example"));
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
        write_live_with_common_config_for_state(&state, &AppType::Claude, &original)
            .expect("seed live file");
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
        crate::gemini_config::write_gemini_env_atomic(&HashMap::from([
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
    fn validate_provider_settings_rejects_negative_cost_multiplier() {
        let mut provider = Provider::with_id(
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
        provider.meta = Some(ProviderMeta {
            cost_multiplier: Some("-1".to_string()),
            ..ProviderMeta::default()
        });

        let err = ProviderService::validate_provider_settings(&AppType::Claude, &provider)
            .expect_err("negative multiplier should be rejected");
        assert!(matches!(
            err,
            AppError::Localized {
                key: "error.invalidMultiplier",
                ..
            }
        ));
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
experimental_bearer_token = "sk-live-secret"
model_catalog_json = "cc-switch-model-catalog.json"
web_search = "disabled"

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
        // 真正可共享的键保留
        assert!(
            extracted.contains("disable_response_storage = true"),
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
    async fn update_current_claude_provider_syncs_live_when_proxy_takeover_detected_without_backup()
    {
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
                },
                "permissions": { "allow": ["Bash"] }
            }),
            None,
        );
        db.save_provider("claude", &original)
            .expect("save provider");
        db.set_current_provider("claude", "p1")
            .expect("set current provider");
        crate::settings::set_current_provider(&AppType::Claude, Some("p1"))
            .expect("set local current provider");

        db.update_proxy_config(ProxyConfig {
            live_takeover_active: true,
            listen_port: 0,
            ..Default::default()
        })
        .await
        .expect("update proxy config");
        {
            let mut config = db
                .get_proxy_config_for_app("claude")
                .await
                .expect("get app proxy config");
            config.enabled = true;
            db.update_proxy_config_for_app(config)
                .await
                .expect("update app proxy config");
        }

        write_json_file(
            &get_claude_settings_path(),
            &json!({
                "env": {
                    "ANTHROPIC_BASE_URL": "http://127.0.0.1:15721",
                    "ANTHROPIC_API_KEY": "PROXY_MANAGED",
                    "ANTHROPIC_MODEL": "stale-model"
                },
                "permissions": { "allow": ["Bash"] }
            }),
        )
        .expect("seed taken-over live file");

        let proxy_info = state
            .proxy_service
            .start()
            .await
            .expect("start proxy service");

        let updated = Provider::with_id(
            "p1".into(),
            "Claude A".into(),
            json!({
                "env": {
                    "ANTHROPIC_API_KEY": "token-updated",
                    "ANTHROPIC_BASE_URL": "https://api.updated.example",
                    "ANTHROPIC_MODEL": "model-updated"
                },
                "permissions": { "allow": ["Read"] }
            }),
            None,
        );

        ProviderService::update(&state, AppType::Claude, None, updated.clone())
            .expect("update current provider");

        let backup = db
            .get_live_backup("claude")
            .await
            .expect("get live backup")
            .expect("backup exists");
        let stored_provider = db
            .get_provider_by_id("p1", "claude")
            .expect("get stored provider")
            .expect("stored provider exists");
        let expected_backup =
            serde_json::to_string(&stored_provider.settings_config).expect("serialize");
        assert_eq!(backup.original_config, expected_backup);

        let live: Value = read_json_file(&get_claude_settings_path()).expect("read live");
        assert_eq!(
            live.get("permissions"),
            updated.settings_config.get("permissions"),
            "provider edits should propagate into Claude live config during takeover"
        );
        assert_eq!(
            live.get("env")
                .and_then(|env| env.get("ANTHROPIC_API_KEY"))
                .and_then(|v| v.as_str()),
            Some("PROXY_MANAGED"),
            "takeover placeholder should stay intact"
        );
        assert_eq!(
            live.get("env")
                .and_then(|env| env.get("ANTHROPIC_BASE_URL"))
                .and_then(|v| v.as_str()),
            Some(format!("http://127.0.0.1:{}", proxy_info.port).as_str()),
            "proxy base URL should stay intact"
        );
        assert!(
            live.get("env")
                .and_then(|env| env.get("ANTHROPIC_MODEL"))
                .is_none(),
            "model override should be removed in takeover live config"
        );
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
            live_takeover_active: true,
            listen_port: 0,
            ..Default::default()
        })
        .await
        .expect("update proxy config");
        {
            let mut config = db
                .get_proxy_config_for_app("codex")
                .await
                .expect("get app proxy config");
            config.enabled = true;
            db.update_proxy_config_for_app(config)
                .await
                .expect("enable Codex proxy config");
        }
        db.save_live_backup(
            "codex",
            &serde_json::to_string(&original.settings_config).expect("serialize backup"),
        )
        .await
        .expect("seed live backup");

        state
            .proxy_service
            .start()
            .await
            .expect("start proxy service");
        state
            .proxy_service
            .sync_codex_live_from_provider_while_proxy_active(&original)
            .await
            .expect("seed taken-over Codex live config");
        assert!(
            state
                .proxy_service
                .detect_takeover_in_live_config_for_app(&AppType::Codex),
            "seeded Codex live config should be recognized as takeover-owned"
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

    #[cfg(any(target_os = "macos", windows))]
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
        // ephemeral 端口：避免与本机正在运行的 App（默认 15721）冲突
        db.update_proxy_config(ProxyConfig {
            listen_port: 0,
            ..Default::default()
        })
        .await
        .expect("update proxy config");
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

        let proxy_info = state
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
            json!(format!(
                "http://127.0.0.1:{}/claude-desktop",
                proxy_info.port
            )),
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
    fn add_first_managed_codex_current_failure_rolls_back_provider_and_live_state() {
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
                .expect_err("DB current failure should abort managed add");
            assert!(
                error
                    .to_string()
                    .contains("forced first managed Codex current failure"),
                "add should surface the DB current failure, got: {error}"
            );
            assert!(
                state
                    .db
                    .get_provider_by_id(&provider.id, AppType::Codex.as_str())
                    .expect("query rolled back provider")
                    .is_none(),
                "failed current commit must remove the newly inserted provider row"
            );
            assert_eq!(
                state
                    .db
                    .get_current_provider(AppType::Codex.as_str())
                    .expect("read current after rollback"),
                None
            );
            assert_eq!(
                crate::codex_config::CodexLiveStateSnapshot::capture()
                    .expect("capture Codex live after rollback"),
                live_before,
                "failed current commit must exactly restore Codex live files"
            );
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
    fn managed_codex_switch_db_current_failure_restores_live_bundle_and_current() {
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

            let live_after = crate::codex_config::CodexLiveStateSnapshot::capture()
                .expect("capture auth/config/catalog/marker after rollback");
            assert_eq!(
                live_after, live_before,
                "failed switch must exactly restore auth, config, catalog, and managed marker"
            );
            assert_eq!(
                crate::settings::get_current_provider(&AppType::Codex).as_deref(),
                Some(provider_a.id.as_str()),
                "failed switch must restore the device-local current provider"
            );
            assert_eq!(
                state
                    .db
                    .get_current_provider(AppType::Codex.as_str())
                    .expect("read DB current after rollback")
                    .as_deref(),
                Some(provider_a.id.as_str()),
                "failed switch must keep the DB current provider unchanged"
            );
        });
    }

    #[test]
    #[serial]
    fn managed_codex_takeover_update_db_failure_restores_backup_live_and_binding() {
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

            let mut provider = Provider::with_id(
                "managed-official-a".to_string(),
                "OpenAI Official A".to_string(),
                json!({
                    "auth": {},
                    "config": "model = \"gpt-5.4\"\n"
                }),
                None,
            );
            provider.category = Some("official".to_string());
            provider.meta = Some(ProviderMeta {
                auth_binding: Some(AuthBinding {
                    source: AuthBindingSource::ManagedAccount,
                    auth_provider: Some("codex_oauth".to_string()),
                    account_id: Some("acct-managed-a".to_string()),
                }),
                ..Default::default()
            });
            state
                .db
                .save_provider(AppType::Codex.as_str(), &provider)
                .expect("save official provider A");
            state
                .db
                .set_current_provider(AppType::Codex.as_str(), &provider.id)
                .expect("set DB current");
            crate::settings::set_current_provider(&AppType::Codex, Some(&provider.id))
                .expect("set local current");

            tauri::async_runtime::block_on(async {
                state
                    .db
                    .update_proxy_config(ProxyConfig {
                        listen_port: 15_721,
                        ..Default::default()
                    })
                    .await
                    .expect("set proxy port");
                state
                    .db
                    .save_live_backup(
                        AppType::Codex.as_str(),
                        &serde_json::to_string(&json!({
                            "config": "model = \"gpt-5.4\"\n"
                        }))
                        .expect("serialize baseline backup"),
                    )
                    .await
                    .expect("save baseline backup");
                state
                    .proxy_service
                    .sync_codex_live_from_provider_while_proxy_active(&provider)
                    .await
                    .expect("seed managed takeover live");
            });

            let backup_before =
                tauri::async_runtime::block_on(state.db.get_live_backup(AppType::Codex.as_str()))
                    .expect("read baseline backup")
                    .expect("baseline backup exists");
            let live_before = crate::codex_config::CodexLiveStateSnapshot::capture()
                .expect("capture managed takeover live");

            {
                let conn = state.db.conn.lock().expect("lock database");
                conn.execute_batch(
                    "CREATE TRIGGER reject_managed_takeover_provider_update
                     BEFORE UPDATE ON providers
                     WHEN NEW.app_type = 'codex'
                       AND NEW.id = 'managed-official-a'
                       AND NEW.name = 'OpenAI Official B'
                     BEGIN
                       SELECT RAISE(ABORT, 'forced managed takeover provider failure');
                     END;",
                )
                .expect("install provider failure trigger");
            }

            let mut updated = provider.clone();
            updated.name = "OpenAI Official B".to_string();
            updated
                .meta
                .as_mut()
                .and_then(|meta| meta.auth_binding.as_mut())
                .expect("managed binding")
                .account_id = Some("acct-managed-b".to_string());

            let error = ProviderService::update(state, AppType::Codex, None, updated)
                .expect_err("DB failure should abort takeover update");
            assert!(
                error
                    .to_string()
                    .contains("forced managed takeover provider failure"),
                "update should surface DB failure: {error}"
            );

            let saved = state
                .db
                .get_provider_by_id(&provider.id, AppType::Codex.as_str())
                .expect("read provider after rollback")
                .expect("provider still exists");
            assert_eq!(saved.name, "OpenAI Official A");
            assert_eq!(
                saved
                    .meta
                    .as_ref()
                    .and_then(|meta| meta.managed_account_id_for("codex_oauth")),
                Some("acct-managed-a".to_string())
            );

            let backup_after =
                tauri::async_runtime::block_on(state.db.get_live_backup(AppType::Codex.as_str()))
                    .expect("read backup after rollback")
                    .expect("backup still exists");
            assert_eq!(backup_after.original_config, backup_before.original_config);
            assert_eq!(
                crate::codex_config::CodexLiveStateSnapshot::capture()
                    .expect("capture live after rollback"),
                live_before,
                "failed takeover update must restore auth/config/catalog/marker exactly"
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
                write_live_with_common_config_for_state(state, &AppType::Codex, &third_party)
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

    fn cursor_third_party_provider(id: &str) -> Provider {
        Provider::with_id(
            id.to_string(),
            "Cursor ThirdParty".to_string(),
            json!({
                "baseUrl": "https://api.kimi.com/v1",
                "apiKey": "sk-kimi",
                "modelCatalog": {
                    "models": [
                        { "model": "custom-k3", "displayName": "k3" }
                    ]
                }
            }),
            None,
        )
    }

    #[tokio::test]
    #[serial]
    async fn switch_cursor_third_party_force_enables_takeover_and_public_route() {
        let _home = TempHome::new();
        crate::settings::reload_settings().expect("reload settings");
        let db = Arc::new(Database::memory().expect("in-memory database"));
        let state = Arc::new(AppState::new(db.clone()));
        // 端口置 0（临时端口），避免与本地/其他测试的 15721 冲突
        db.update_proxy_config(ProxyConfig {
            listen_port: 0,
            ..Default::default()
        })
        .await
        .expect("set ephemeral proxy port");

        let provider = cursor_third_party_provider("cursor-tp");
        db.save_provider(AppType::Cursor.as_str(), &provider)
            .expect("save cursor provider");

        // Cursor 的 Base URL 走云端账户同步、本地写不进去，所以切换供应商时
        // 必须强制开启「路由接管 + 公网路由」，否则该供应商在 Cursor 里不可用
        // （与命令层一致走 spawn_blocking：switch 内部用 block_on 驱动异步操作）
        let switch_state = state.clone();
        tokio::task::spawn_blocking(move || {
            ProviderService::switch(&switch_state, AppType::Cursor, "cursor-tp")
        })
        .await
        .expect("join switch task")
        .expect("cursor provider switch should succeed");

        let settings = crate::settings::get_settings();
        assert!(
            settings.public_route_enabled,
            "切换 Cursor 第三方供应商必须强制开启公网路由"
        );
        assert!(
            settings
                .public_route_api_key
                .as_deref()
                .is_some_and(|k| !k.is_empty()),
            "强制开启公网路由时必须保证鉴权密钥存在"
        );

        let taken_over = db
            .get_live_backup(AppType::Cursor.as_str())
            .await
            .ok()
            .flatten()
            .is_some()
            || state
                .proxy_service
                .detect_takeover_in_live_config_for_app(&AppType::Cursor);
        assert!(taken_over, "切换 Cursor 第三方供应商必须强制开启路由接管");

        state.proxy_service.stop().await.expect("stop proxy server");
    }

    #[test]
    #[serial]
    fn switch_cursor_official_provider_does_not_force_public_route() {
        with_test_home(|state, _| {
            crate::settings::reload_settings().expect("reload settings");
            let mut provider = cursor_third_party_provider("cursor-official");
            provider.category = Some("official".to_string());
            state
                .db
                .save_provider(AppType::Cursor.as_str(), &provider)
                .expect("save cursor official provider");

            ProviderService::switch(state, AppType::Cursor, "cursor-official")
                .expect("cursor official switch should succeed");

            assert!(
                !crate::settings::get_settings().public_route_enabled,
                "官方供应商无需公网路由，不应强制开启"
            );
        });
    }
}

impl ProviderService {
    fn managed_codex_oauth_account_id(provider: &Provider) -> Option<String> {
        provider
            .meta
            .as_ref()
            .and_then(|meta| meta.managed_account_id_for("codex_oauth"))
            .map(|id| id.trim().to_string())
            .filter(|id| !id.is_empty())
    }

    /// 提交 current（settings/DB）前的预检：若目标是托管 Codex official provider，
    /// 先解析一次有效 live 配置（会联网换取并缓存 token）。同时返回这份已解析配置，
    /// 让后续落盘直接复用同一 token bundle，避免一次操作重复解析/刷新。
    fn preflight_managed_codex_live(
        state: &AppState,
        app_type: &AppType,
        provider: &Provider,
    ) -> Result<Option<Provider>, AppError> {
        if matches!(app_type, AppType::Codex)
            && Self::managed_codex_oauth_account_id(provider).is_some()
        {
            return build_effective_provider_for_live_with_codex_oauth_manager(
                state.db.as_ref(),
                app_type,
                provider,
                &state.codex_oauth_manager,
            )
            .map(Some);
        }
        Ok(None)
    }

    fn write_preflighted_or_current_live(
        state: &AppState,
        app_type: &AppType,
        provider: &Provider,
        preflighted_provider: Option<&Provider>,
    ) -> Result<(), AppError> {
        if let Some(effective_provider) = preflighted_provider {
            live::write_live_snapshot(app_type, effective_provider)
        } else {
            write_live_with_common_config_for_state(state, app_type, provider)
        }
    }

    fn managed_codex_transaction_error(
        operation: &str,
        error: AppError,
        snapshot: &crate::codex_config::CodexLiveStateSnapshot,
        restore_local_current: Option<(&AppType, Option<&str>)>,
    ) -> AppError {
        let mut rollback_failures = Vec::new();
        if let Some((app_type, previous_local_current)) = restore_local_current {
            if let Err(rollback_error) =
                crate::settings::set_current_provider(app_type, previous_local_current)
            {
                rollback_failures.push(format!("恢复本地 current 失败: {rollback_error}"));
            }
        }
        if let Err(rollback_error) = snapshot.restore_preserving_newer_same_account_auth() {
            rollback_failures.push(rollback_error.to_string());
        }

        if rollback_failures.is_empty() {
            error
        } else {
            AppError::Message(format!(
                "{operation}失败: {error}; 回滚同时失败: {}",
                rollback_failures.join("; ")
            ))
        }
    }

    fn managed_codex_add_transaction_error(
        state: &AppState,
        operation: &str,
        error: AppError,
        provider: &Provider,
        previous_provider: Option<&Provider>,
        provider_saved: bool,
        snapshot: &crate::codex_config::CodexLiveStateSnapshot,
    ) -> AppError {
        let mut rollback_failures = Vec::new();

        if provider_saved {
            let provider_rollback = match previous_provider {
                Some(previous) => state.db.save_provider(AppType::Codex.as_str(), previous),
                None => state
                    .db
                    .delete_provider(AppType::Codex.as_str(), &provider.id),
            };
            if let Err(rollback_error) = provider_rollback {
                rollback_failures.push(format!("恢复 Provider 数据失败: {rollback_error}"));
            }
        }

        if let Err(rollback_error) = snapshot.restore_preserving_newer_same_account_auth() {
            rollback_failures.push(rollback_error.to_string());
        }

        if rollback_failures.is_empty() {
            error
        } else {
            AppError::Message(format!(
                "{operation}失败: {error}; 回滚同时失败: {}",
                rollback_failures.join("; ")
            ))
        }
    }

    fn managed_codex_takeover_transaction_error(
        state: &AppState,
        operation: &str,
        error: AppError,
        snapshot: &crate::codex_config::CodexLiveStateSnapshot,
        previous_backup: Option<&crate::proxy::types::LiveBackup>,
        restore_local_current: Option<(&AppType, Option<&str>)>,
    ) -> AppError {
        let mut rollback_failures = Vec::new();
        if let Some((app_type, previous_local_current)) = restore_local_current {
            if let Err(rollback_error) =
                crate::settings::set_current_provider(app_type, previous_local_current)
            {
                rollback_failures.push(format!("恢复本地 current 失败: {rollback_error}"));
            }
        }
        let backup_restore = match previous_backup {
            Some(backup) => futures::executor::block_on(
                state
                    .db
                    .save_live_backup(AppType::Codex.as_str(), &backup.original_config),
            ),
            None => {
                futures::executor::block_on(state.db.delete_live_backup(AppType::Codex.as_str()))
            }
        };
        if let Err(rollback_error) = backup_restore {
            rollback_failures.push(format!("恢复 Codex Live 备份失败: {rollback_error}"));
        }
        if let Err(rollback_error) = snapshot.restore_preserving_newer_same_account_auth() {
            rollback_failures.push(rollback_error.to_string());
        }

        if rollback_failures.is_empty() {
            error
        } else {
            AppError::Message(format!(
                "{operation}失败: {error}; 回滚同时失败: {}",
                rollback_failures.join("; ")
            ))
        }
    }

    fn outgoing_managed_codex_oauth_account_id(
        app_type: &AppType,
        existing_provider: Option<&Provider>,
        provider: &Provider,
    ) -> Option<String> {
        if !matches!(app_type, AppType::Codex) {
            return None;
        }

        let old_account_id = existing_provider.and_then(Self::managed_codex_oauth_account_id)?;
        if Self::managed_codex_oauth_account_id(provider).as_deref()
            == Some(old_account_id.as_str())
        {
            return None;
        }

        Some(old_account_id)
    }

    fn prepare_outgoing_managed_codex_live_auth(
        state: &AppState,
        account_id: Option<&str>,
    ) -> Result<Option<String>, AppError> {
        let Some(account_id) = account_id else {
            return Ok(None);
        };
        live::prepare_codex_managed_oauth_live_auth_switch_away(
            state.codex_oauth_manager.clone(),
            account_id.to_string(),
        )
    }

    fn ensure_outgoing_managed_codex_live_auth_unchanged(
        account_id: Option<&str>,
        expected_refresh_token: Option<&str>,
    ) -> Result<(), AppError> {
        if let (Some(account_id), Some(expected_refresh_token)) =
            (account_id, expected_refresh_token)
        {
            crate::codex_config::ensure_codex_live_auth_unchanged_for_managed_account(
                account_id,
                expected_refresh_token,
            )?;
        }
        Ok(())
    }

    fn clear_outgoing_managed_codex_live_auth(
        account_id: Option<&str>,
        expected_refresh_token: Option<&str>,
    ) -> Result<(), AppError> {
        let Some(account_id) = account_id else {
            return Ok(());
        };
        if let Some(expected_refresh_token) = expected_refresh_token {
            crate::codex_config::clear_codex_live_auth_for_managed_account_if_unchanged(
                account_id,
                Some(expected_refresh_token),
            )
        } else {
            crate::codex_config::clear_codex_live_auth_for_managed_account(account_id)
        }
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
        crate::settings::get_effective_current_provider(&state.db, &app_type)
            .map(|opt| opt.unwrap_or_default())
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
        normalize_provider_common_config_for_storage(state.db.as_ref(), &app_type, &mut provider)?;
        Self::normalize_usage_script_credential_overrides(&app_type, &mut provider);
        if app_type.is_additive_mode() {
            Self::set_provider_live_config_managed(&mut provider, add_to_live);
        }

        let is_managed_codex_add = matches!(app_type, AppType::Codex)
            && Self::managed_codex_oauth_account_id(&provider).is_some();
        let _managed_codex_add_guard = if is_managed_codex_add {
            Some(futures::executor::block_on(
                state.proxy_service.lock_switch_for_app(app_type.as_str()),
            ))
        } else {
            None
        };

        if is_managed_codex_add {
            let effective_current =
                crate::settings::get_effective_current_provider(&state.db, &app_type)?;

            // Adding a non-current managed provider only mutates its DB row. Keep
            // the same switch lock until that row is committed so a waiting switch
            // cannot observe a partially saved binding.
            if effective_current.is_some() {
                state.db.save_provider(app_type.as_str(), &provider)?;
                return Ok(true);
            }

            // For the first managed Codex provider, resolve the complete live
            // bundle before mutating DB state. Then commit Live -> provider row ->
            // current under one switch lock. A failure restores both files and the
            // provider row, avoiding a visible but unusable orphan provider.
            let previous_provider = state
                .db
                .get_provider_by_id(&provider.id, app_type.as_str())?;
            let preflighted_provider =
                Self::preflight_managed_codex_live(state, &app_type, &provider)?;
            let snapshot = crate::codex_config::CodexLiveStateSnapshot::capture()?;
            let mut provider_saved = false;
            let commit_result = (|| {
                Self::write_preflighted_or_current_live(
                    state,
                    &app_type,
                    &provider,
                    preflighted_provider.as_ref(),
                )?;
                state.db.save_provider(app_type.as_str(), &provider)?;
                provider_saved = true;
                state
                    .db
                    .set_current_provider(app_type.as_str(), &provider.id)?;
                Ok::<(), AppError>(())
            })();

            if let Err(error) = commit_result {
                return Err(Self::managed_codex_add_transaction_error(
                    state,
                    "新增首个托管 Codex provider",
                    error,
                    &provider,
                    previous_provider.as_ref(),
                    provider_saved,
                    &snapshot,
                ));
            }

            return Ok(true);
        }

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
            write_live_with_common_config_for_state(state, &app_type, &provider)?;
            return Ok(true);
        }

        // For other apps: Check if sync is needed (if this is current provider, or no current provider)
        let current = state.db.get_current_provider(app_type.as_str())?;
        if current.is_none() {
            // No current provider, set as current and sync. Managed Codex adds
            // use the transactional path above because token resolution can fail.
            state
                .db
                .set_current_provider(app_type.as_str(), &provider.id)?;
            write_live_with_common_config_for_state(state, &app_type, &provider)?;
        }

        Ok(true)
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
        // Serialize the read/decide/commit window for every Codex update. We do
        // not yet know whether the stored row is managed (the request may be an
        // unbind), so the existing row and effective current must both be read
        // only after this lock is held. Non-managed Codex updates release it
        // before entering the legacy path, whose proxy helpers take the lock
        // themselves.
        let codex_update_switch_guard = if matches!(app_type, AppType::Codex) {
            Some(futures::executor::block_on(
                state.proxy_service.lock_switch_for_app(app_type.as_str()),
            ))
        } else {
            None
        };
        let existing_provider = state
            .db
            .get_provider_by_id(&original_id, app_type.as_str())?;
        // Normalize Claude model keys
        Self::normalize_provider_if_claude(&app_type, &mut provider);
        Self::validate_provider_settings(&app_type, &provider)?;
        normalize_provider_common_config_for_storage(state.db.as_ref(), &app_type, &mut provider)?;
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
            state.db.save_provider(app_type.as_str(), &provider)?;
            state.db.delete_provider(app_type.as_str(), &original_id)?;

            if crate::settings::get_current_provider(&app_type).as_deref() == Some(&original_id) {
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

            // Save to database after live-config presence is resolved so parse errors
            // do not report failure after already mutating DB state.
            state.db.save_provider(app_type.as_str(), &provider)?;

            if !live_config_managed {
                return Ok(true);
            }
            write_live_with_common_config_for_state(state, &app_type, &provider)?;
            return Ok(true);
        }

        // For other apps: Check if this is current provider (use effective current, not just DB)
        let effective_current =
            crate::settings::get_effective_current_provider(&state.db, &app_type)?;
        let is_current = effective_current.as_deref() == Some(provider.id.as_str());

        let existing_managed_codex_account_id = existing_provider
            .as_ref()
            .and_then(Self::managed_codex_oauth_account_id);
        let target_managed_codex_account_id = Self::managed_codex_oauth_account_id(&provider);
        let outgoing_managed_codex_account_id = Self::outgoing_managed_codex_oauth_account_id(
            &app_type,
            existing_provider.as_ref(),
            &provider,
        );
        let managed_codex_update = matches!(app_type, AppType::Codex)
            && (existing_managed_codex_account_id.is_some()
                || target_managed_codex_account_id.is_some());

        if managed_codex_update {
            // A non-current managed row still commits under the same lock: once
            // the row is saved, a waiting switch observes the new binding. If we
            // released first, a switch could activate the old binding and leave
            // DB current/live inconsistent with the subsequent save.
            if !is_current {
                state.db.save_provider(app_type.as_str(), &provider)?;
                return Ok(true);
            }

            let outgoing_live_refresh_token = Self::prepare_outgoing_managed_codex_live_auth(
                state,
                outgoing_managed_codex_account_id.as_deref(),
            )?;

            // The lock acquired before reading existing/current spans the
            // complete direct/takeover transaction. Backup update and takeover
            // Live sync therefore cannot expose a gap to concurrent hot-switch.
            let previous_backup =
                futures::executor::block_on(state.db.get_live_backup(app_type.as_str()))?;
            let has_live_backup = previous_backup.is_some();
            let live_taken_over = state
                .proxy_service
                .detect_takeover_in_live_config_for_app(&app_type);
            let preflighted_provider =
                Self::preflight_managed_codex_live(state, &app_type, &provider)?;
            // Capture after preflight: a legitimate refresh may have advanced
            // auth.json, and rollback must never restore the older generation.
            let snapshot = crate::codex_config::CodexLiveStateSnapshot::capture()?;

            if !has_live_backup && !live_taken_over {
                let commit_result = (|| {
                    Self::ensure_outgoing_managed_codex_live_auth_unchanged(
                        outgoing_managed_codex_account_id.as_deref(),
                        outgoing_live_refresh_token.as_deref(),
                    )?;
                    Self::write_preflighted_or_current_live(
                        state,
                        &app_type,
                        &provider,
                        preflighted_provider.as_ref(),
                    )?;
                    Self::clear_outgoing_managed_codex_live_auth(
                        outgoing_managed_codex_account_id.as_deref(),
                        outgoing_live_refresh_token.as_deref(),
                    )?;
                    state.db.save_provider(app_type.as_str(), &provider)?;
                    Ok::<(), AppError>(())
                })();
                if let Err(error) = commit_result {
                    return Err(Self::managed_codex_transaction_error(
                        "更新托管 Codex provider",
                        error,
                        &snapshot,
                        None,
                    ));
                }

                if let Err(err) = McpService::sync_enabled_for_app(state, &app_type) {
                    log::warn!(
                        "保存供应商后重投影 {app_type:?} MCP 失败（将在下次同步时自愈）: {err}"
                    );
                }
                return Ok(true);
            }

            let commit_result = (|| {
                Self::ensure_outgoing_managed_codex_live_auth_unchanged(
                    outgoing_managed_codex_account_id.as_deref(),
                    outgoing_live_refresh_token.as_deref(),
                )?;
                futures::executor::block_on(
                    state.proxy_service.update_live_backup_from_provider_inner(
                        app_type.as_str(),
                        &provider,
                        outgoing_managed_codex_account_id.as_deref(),
                    ),
                )
                .map_err(|error| AppError::Message(format!("更新 Live 备份失败: {error}")))?;

                if live_taken_over {
                    futures::executor::block_on(
                        state
                            .proxy_service
                            .sync_codex_live_from_provider_while_proxy_active_guarded(
                                &provider,
                                outgoing_managed_codex_account_id.as_deref(),
                                outgoing_live_refresh_token.as_deref(),
                            ),
                    )
                    .map_err(|error| {
                        AppError::Message(format!("同步 Codex Live 配置失败: {error}"))
                    })?;
                } else {
                    // A backup without a takeover marker is a recoverable
                    // half-takeover state. Keep the actual Live bundle aligned
                    // with the edited current provider as well as the backup.
                    Self::ensure_outgoing_managed_codex_live_auth_unchanged(
                        outgoing_managed_codex_account_id.as_deref(),
                        outgoing_live_refresh_token.as_deref(),
                    )?;
                    Self::write_preflighted_or_current_live(
                        state,
                        &app_type,
                        &provider,
                        preflighted_provider.as_ref(),
                    )?;
                }

                Self::clear_outgoing_managed_codex_live_auth(
                    outgoing_managed_codex_account_id.as_deref(),
                    outgoing_live_refresh_token.as_deref(),
                )?;

                // DB is the final commit. Every fallible side effect above can be
                // restored exactly while the previous provider row is untouched.
                state.db.save_provider(app_type.as_str(), &provider)?;
                Ok::<(), AppError>(())
            })();
            if let Err(error) = commit_result {
                return Err(Self::managed_codex_takeover_transaction_error(
                    state,
                    "更新接管中的 Codex provider",
                    error,
                    &snapshot,
                    previous_backup.as_ref(),
                    None,
                ));
            }

            return Ok(true);
        }

        drop(codex_update_switch_guard);

        // Save to database
        state.db.save_provider(app_type.as_str(), &provider)?;

        if is_current {
            let outcome =
                live::sync_live_for_provider_respecting_takeover(state, &app_type, &provider)?;
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
                    _ => {}
                }
            }
            state.db.delete_provider(app_type.as_str(), id)?;
            return Ok(());
        }

        // For other apps: Check both local settings and database
        let local_current = crate::settings::get_current_provider(&app_type);
        let db_current = state.db.get_current_provider(app_type.as_str())?;

        if local_current.as_deref() == Some(id) || db_current.as_deref() == Some(id) {
            return Err(AppError::Message(
                "无法删除当前正在使用的供应商".to_string(),
            ));
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

    /// Switch to a provider
    ///
    /// Switch flow:
    /// 1. Validate target provider exists
    /// 2. Check if proxy takeover mode is active AND proxy server is running
    /// 3. If takeover mode active: hot-switch proxy target and refresh proxy-safe Live labels
    /// 4. If normal mode:
    ///    a. **Backfill mechanism**: Backfill current live config to current provider
    ///    b. Update local settings current_provider_xxx (device-level)
    ///    c. Update database is_current (as default for new devices)
    ///    d. Write target provider config to live files
    ///    e. Sync MCP configuration
    pub fn switch(state: &AppState, app_type: AppType, id: &str) -> Result<SwitchResult, AppError> {
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

        // Provider switches and takeover toggles both mutate live config and the
        // restore backup. Serialize them per app, then decide from the locked
        // current state so a just-started takeover cannot be overwritten by a
        // normal live write.
        let _switch_guard =
            if app_type.supports_local_proxy() || matches!(app_type, AppType::Cursor) {
                Some(futures::executor::block_on(
                    state.proxy_service.lock_switch_for_app(app_type.as_str()),
                ))
            } else {
                None
            };

        // Backup or live placeholders mean the live file is owned by proxy
        // takeover, even if the proxy server is temporarily stopped or is in the
        // activation window before enabled=true is committed.
        let is_app_taken_over =
            futures::executor::block_on(state.db.get_live_backup(app_type.as_str()))
                .ok()
                .flatten()
                .is_some();
        let live_taken_over = state
            .proxy_service
            .detect_takeover_in_live_config_for_app(&app_type);

        let should_hot_switch = is_app_taken_over || live_taken_over;

        // Block switching to unsupported official providers when proxy takeover
        // is active. Codex official account cards use native auth passthrough.
        if should_hot_switch
            && _provider.category.as_deref() == Some("official")
            && !official_provider_supports_proxy_takeover(&app_type, _provider)
        {
            return Err(AppError::localized(
                "switch.official_blocked_by_proxy",
                "代理接管模式下不能切换到官方供应商，使用代理访问官方 API 可能导致账号被封禁。请先关闭代理接管，或选择第三方供应商。",
                "Cannot switch to official provider while proxy takeover is active. Using proxy with official APIs may cause account bans.",
            ));
        }

        let mut result = if should_hot_switch {
            // Proxy takeover mode: hot-switch without restoring upstream Live config.
            // The proxy layer may still refresh proxy-safe Live fields so client labels
            // follow the selected provider while endpoints remain local.
            log::info!(
                "代理接管模式：热切换 {} 的目标供应商为 {}",
                app_type.as_str(),
                id
            );

            futures::executor::block_on(
                state
                    .proxy_service
                    .hot_switch_provider_inner(app_type.as_str(), id),
            )
            .map_err(|e| AppError::Message(format!("热切换失败: {e}")))?;

            // The proxy server will route requests to the new provider via is_current.
            // MCP sync is intentionally skipped while Live config is owned by takeover.
            SwitchResult::default()
        } else {
            // Normal mode: full switch with Live config write
            Self::switch_normal(state, app_type.clone(), id, &providers)?
        };

        // Cursor 第三方供应商：强制开启路由接管 + 公网路由（见函数注释）。
        // set_takeover_for_app 会竞争同一把 per-app 切换锁，必须先释放守卫。
        drop(_switch_guard);
        Self::force_public_route_for_cursor(state, &app_type, _provider, &mut result.warnings);
        Ok(result)
    }

    /// Cursor 的 Base URL / 模型设置走云端账户同步，本地无法程序化写入
    /// （2026-08-05 实测确认），所以第三方供应商只有经「路由接管 + 公网路由
    /// 隧道」才能在 Cursor 里实际可用。切换 Cursor 第三方供应商时强制开启两者。
    ///
    /// 失败不阻断切换本身：记录 warning 引导用户去「设置 → 路由」手动处理。
    /// 官方供应商（category = official）不强制——其不经过本地代理。
    fn force_public_route_for_cursor(
        state: &AppState,
        app_type: &AppType,
        provider: &Provider,
        warnings: &mut Vec<String>,
    ) {
        if !matches!(app_type, AppType::Cursor) || provider.category.as_deref() == Some("official")
        {
            return;
        }
        use futures::executor::block_on;

        // 1) 路由接管（内部会自动启动本地代理；已接管时幂等）
        if let Err(e) = block_on(state.proxy_service.set_takeover_for_app("cursor", true)) {
            log::warn!("[Cursor] 切换供应商后强制开启路由接管失败: {e}");
            warnings.push(format!(
                "已切换供应商，但开启 Cursor 路由接管失败：{e}。请到「设置 → 路由」手动开启接管与公网路由，否则 Cursor 无法使用该供应商。"
            ));
            return;
        }

        // 2) 公网路由开关 + 保证隧道鉴权密钥存在
        let mut settings = crate::settings::get_settings();
        settings.public_route_enabled = true;
        if settings
            .public_route_api_key
            .as_deref()
            .map(str::is_empty)
            .unwrap_or(true)
        {
            settings.public_route_api_key = Some(crate::settings::generate_public_route_api_key());
        }
        if let Err(e) = crate::settings::update_settings(settings) {
            log::warn!("[Cursor] 保存公网路由设置失败: {e}");
            warnings.push(format!(
                "已开启 Cursor 路由接管，但保存公网路由设置失败：{e}。请到「设置 → 路由 → 公网路由」手动开启。"
            ));
        } else {
            // 3) 启动公网隧道。单测环境跳过真实 cloudflared 进程（避免下载
            //    二进制 / 残留子进程）；隧道失败同样不阻断切换。
            #[cfg(not(test))]
            if let Err(e) = block_on(state.proxy_service.start_public_route_tunnel()) {
                log::warn!("[Cursor] 公网隧道启动失败: {e}");
                warnings.push(format!(
                    "已开启 Cursor 路由接管与公网路由，但公网隧道启动失败：{e}。可到「设置 → 路由 → 公网路由」查看原因并重试。"
                ));
            }
        }
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

        // Backfill: Backfill current live config to current provider
        // Use effective current provider (validated existence) to ensure backfill targets valid provider
        let current_id = crate::settings::get_effective_current_provider(&state.db, &app_type)?;
        let current_managed_codex_account_id = current_id
            .as_deref()
            .and_then(|current_id| providers.get(current_id))
            .and_then(Self::managed_codex_oauth_account_id);
        let mut backfill_completed = false;
        if let Some(current_id) = current_id {
            if current_id != id {
                // Additive mode apps - all providers coexist in the same file,
                // no backfill needed (backfill is for exclusive mode apps like Claude/Codex/Gemini)
                if !app_type.is_additive_mode() {
                    // Only backfill when switching to a different provider
                    if let Ok(live_config) = read_live_settings(app_type.clone()) {
                        if let Some(mut current_provider) = providers.get(&current_id).cloned() {
                            // 切走前先把 live 里的可共享改动（含用户直接在应用内
                            // 装插件/加 hook/改偏好）同步进通用配置片段，再做剥离回填。
                            // 详见 sync_common_config_snippet_from_live 的文档。
                            Self::sync_common_config_snippet_from_live(
                                state,
                                &app_type,
                                &current_provider,
                                &live_config,
                                &mut result,
                            );

                            current_provider.settings_config =
                                strip_common_config_from_live_settings(
                                    state.db.as_ref(),
                                    &app_type,
                                    &current_provider,
                                    live_config,
                                );
                            if let Err(e) =
                                state.db.save_provider(app_type.as_str(), &current_provider)
                            {
                                log::warn!("Backfill failed: {e}");
                                result
                                    .warnings
                                    .push(format!("backfill_failed:{current_id}"));
                            } else {
                                backfill_completed = true;
                            }
                        }
                    }
                }
            }
        }

        let target_managed_codex_account_id = Self::managed_codex_oauth_account_id(provider);
        let outgoing_managed_codex_account_id = current_managed_codex_account_id
            .as_ref()
            .filter(|account_id| target_managed_codex_account_id.as_ref() != Some(*account_id))
            .cloned();
        let outgoing_live_refresh_token = Self::prepare_outgoing_managed_codex_live_auth(
            state,
            outgoing_managed_codex_account_id.as_deref(),
        )?;

        // 提交 current 前预检托管 Codex token（见 preflight_managed_codex_live）。
        let preflighted_provider = Self::preflight_managed_codex_live(state, &app_type, provider)?;
        let use_managed_codex_transaction = matches!(app_type, AppType::Codex)
            && (current_managed_codex_account_id.is_some()
                || target_managed_codex_account_id.is_some());

        if use_managed_codex_transaction {
            // auth/config/catalog/marker form one logical live commit. Write them
            // before current, then restore the exact four-file snapshot on any
            // failure so native logins and CLI-rotated tokens are not reconstructed
            // from a stale provider row.
            let snapshot = crate::codex_config::CodexLiveStateSnapshot::capture()?;
            let live_result = (|| {
                Self::ensure_outgoing_managed_codex_live_auth_unchanged(
                    outgoing_managed_codex_account_id.as_deref(),
                    outgoing_live_refresh_token.as_deref(),
                )?;
                Self::write_preflighted_or_current_live(
                    state,
                    &app_type,
                    provider,
                    preflighted_provider.as_ref(),
                )?;
                Self::clear_outgoing_managed_codex_live_auth(
                    outgoing_managed_codex_account_id.as_deref(),
                    outgoing_live_refresh_token.as_deref(),
                )?;
                Ok::<(), AppError>(())
            })();
            if let Err(error) = live_result {
                return Err(Self::managed_codex_transaction_error(
                    "写入 Codex Live",
                    error,
                    &snapshot,
                    None,
                ));
            }

            let previous_local_current = crate::settings::get_current_provider(&app_type);
            if let Err(error) = crate::settings::set_current_provider(&app_type, Some(id)) {
                return Err(Self::managed_codex_transaction_error(
                    "更新本地 current",
                    error,
                    &snapshot,
                    Some((&app_type, previous_local_current.as_deref())),
                ));
            }
            if let Err(error) = state.db.set_current_provider(app_type.as_str(), id) {
                return Err(Self::managed_codex_transaction_error(
                    "更新数据库 current",
                    error,
                    &snapshot,
                    Some((&app_type, previous_local_current.as_deref())),
                ));
            }
        } else {
            // Codex: validate the live projection before committing current —
            // the write-layer safety gates can refuse the switch, and a
            // refusal after current moved would let the next switch backfill
            // the old live config into the new provider's DB row. (The
            // managed branch above has its own snapshot rollback instead.)
            if matches!(app_type, AppType::Codex) && preflighted_provider.is_none() {
                live::preflight_codex_live_write_for_state(state, provider)?;
            }

            // Additive mode apps skip setting is_current (no such concept).
            if !app_type.is_additive_mode() {
                crate::settings::set_current_provider(&app_type, Some(id))?;
                state.db.set_current_provider(app_type.as_str(), id)?;
            }

            // Sync to live (write_gemini_live handles security flag internally for Gemini).
            Self::write_preflighted_or_current_live(
                state,
                &app_type,
                provider,
                preflighted_provider.as_ref(),
            )?;
        }

        // A material-less official Codex provider gets a config-only live
        // write, which can leave the previous third-party key in
        // ~/.codex/auth.json and strand the user on a 401 with no login
        // screen. Only clean up after a successful backfill — the DB copy
        // made above is what keeps that key recoverable. Failures degrade to
        // a log entry: config.toml and is_current are already committed, so
        // failing the switch here would report a switch that in fact happened.
        if matches!(app_type, AppType::Codex)
            && backfill_completed
            && (provider.category.as_deref() == Some("official")
                || crate::proxy::providers::is_codex_official_provider(provider))
            && target_managed_codex_account_id.is_none()
        {
            let db_auth = provider.settings_config.get("auth");
            match crate::codex_config::clear_stale_codex_live_auth_after_official_switch(
                db_auth.unwrap_or(&serde_json::Value::Null),
            ) {
                Ok(true) => log::info!(
                    "Removed stale third-party auth.json after switching to official Codex provider '{}'",
                    provider.id
                ),
                Ok(false) => {}
                Err(e) => log::warn!("Failed to clean stale Codex auth.json: {e}"),
            }
        }
        // Third-party dual of the block above: with preservation off, the
        // config-only write is expected to delete auth.json. A deletion
        // failure (read-only dir, ACL, file lock) must not fail the switch —
        // config and current are already committed — but the user has to see
        // that the official login is still on disk, so surface it as a
        // switch warning instead of only a log line.
        if matches!(app_type, AppType::Codex)
            && provider.category.as_deref() != Some("official")
            && !crate::proxy::providers::is_codex_official_provider(provider)
            && !crate::settings::preserve_codex_official_auth_on_switch()
            && crate::codex_config::get_codex_auth_path().exists()
        {
            log::warn!("Codex auth.json still present after a preservation-off third-party switch");
            result
                .warnings
                .push("codex_auth_cleanup_failed".to_string());
        }
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

        // 切换重写了目标应用的 live，只重投影该应用的 MCP（Codex 的
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

    /// Sync current provider to live configuration (re-export)
    pub fn sync_current_to_live(state: &AppState) -> Result<(), AppError> {
        sync_current_to_live(state)
    }

    pub fn sync_current_provider_for_app(
        state: &AppState,
        app_type: AppType,
    ) -> Result<(), AppError> {
        if app_type.is_additive_mode() {
            return sync_current_provider_for_app_to_live(state, &app_type);
        }

        let current_id =
            match crate::settings::get_effective_current_provider(&state.db, &app_type)? {
                Some(id) => id,
                None => return Ok(()),
            };

        let providers = state.db.get_all_providers(app_type.as_str())?;
        let Some(provider) = providers.get(&current_id) else {
            return Ok(());
        };

        let outcome = live::sync_live_for_provider_respecting_takeover(state, &app_type, provider)?;
        if outcome == LiveSyncOutcome::BackupOnly {
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

    /// 切走某供应商前，把它 live 配置里的可共享部分重新提取并**整体替换**到
    /// 通用配置片段，使在 live 应用里直接做的改动不会因切换而丢失。
    ///
    /// 采用"整体重提取 + 替换"而非"只合并新增"，是为了同时覆盖三种情况：
    /// - **新增**：用户直接在应用里装了插件、加了 hook、改了 env/主题/权限等共享
    ///   偏好，被捕获进通用配置，切到别的供应商也带得过去；
    /// - **删除**：被删掉的键不在新提取结果里，于是从片段里消失、下次切换不会被
    ///   重新注入——否则会出现"插件怎么删也删不掉"的反直觉 bug；
    /// - **密钥安全**：提取器已剥掉 auth / model / endpoint，密钥永不进共享片段。
    ///
    /// 之所以"整体替换"是安全的：每次写 live 都会把当前片段合并进去，所以切走时
    /// 读到的 live 一定是"片段 + 本地改动"的超集，重提取只会丢掉用户真正删掉的键，
    /// 不会误删其它供应商共享的内容。
    ///
    /// **作用域**：Claude + Codex。Codex 提取器（`extract_codex_common_config`）
    /// 已剥离全部供应商专属与 cc-switch 注入内容：`model` / `model_provider` /
    /// 顶层 `base_url` / 整张 `model_providers` 表（含端点与统一会话桶）、
    /// `mcp_servers`（SSOT 在 DB 表）、顶层 `experimental_bearer_token`
    /// fallback、`model_catalog_json`、`web_search = "disabled"` 哨兵——密钥与
    /// 注入产物不会进共享片段。Gemini 暂未纳入，如需支持应单独验证后再加。
    ///
    /// 仅对**显式勾选"写入通用配置"**（`meta.common_config_enabled == Some(true)`）的
    /// 供应商生效；用户**显式清空**过片段（`_cleared`）时跳过，避免把用户主动清掉的
    /// 配置又塞回来。所有失败均为非致命，只记 warning，绝不阻断切换。
    fn sync_common_config_snippet_from_live(
        state: &AppState,
        app_type: &AppType,
        provider: &Provider,
        live_config: &Value,
        result: &mut SwitchResult,
    ) {
        // 作用域限定 Claude + Codex（见函数文档）。
        if !matches!(app_type, AppType::Claude | AppType::Codex) {
            return;
        }

        let opted_in = provider
            .meta
            .as_ref()
            .and_then(|meta| meta.common_config_enabled)
            == Some(true);
        if !opted_in {
            return;
        }

        match state.db.is_config_snippet_cleared(app_type.as_str()) {
            Ok(true) => return, // 用户显式清空过通用配置，尊重其选择，不再自动塞回
            Ok(false) => {}
            Err(err) => {
                log::warn!(
                    "Failed to read common config cleared flag for {}: {err}",
                    app_type.as_str()
                );
                return;
            }
        }

        let new_snippet = match Self::extract_common_config_snippet_from_settings(
            app_type.clone(),
            live_config,
        ) {
            Ok(snippet) => snippet,
            Err(err) => {
                log::warn!(
                    "Failed to extract common config from live for {} provider '{}': {err}",
                    app_type.as_str(),
                    provider.id
                );
                return;
            }
        };

        // 未变化则跳过，避免无谓写库（不切 live 配置时这是常态路径）。
        let current = state
            .db
            .get_config_snippet(app_type.as_str())
            .ok()
            .flatten();
        if current.as_deref() == Some(new_snippet.as_str()) {
            return;
        }

        if let Err(err) = state
            .db
            .set_config_snippet(app_type.as_str(), Some(new_snippet))
        {
            log::warn!(
                "Failed to persist synced common config for {} provider '{}': {err}",
                app_type.as_str(),
                provider.id
            );
            result
                .warnings
                .push(format!("common_config_sync_failed:{}", provider.id));
        }
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
            AppType::Cursor => Ok(String::new()), // Cursor doesn't use common config snippets
            AppType::Pi => Ok(String::new()),
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
            AppType::Cursor => Ok(String::new()), // Cursor doesn't use common config snippets
            AppType::Pi => Ok(String::new()),
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
    /// 单数 `*_TOKEN`、AWS Bedrock / Vertex 凭据、以及通用 secret / password /
    /// 私钥命名。
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
        ];
        const SENSITIVE_EXACT: &[&str] = &[
            "APIKEY",
            "API_KEY",
            "TOKEN",
            "SECRET",
            "PASSWORD",
            "CREDENTIALS",
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
        ];

        SENSITIVE_EXACT.contains(&upper.as_str())
            || SENSITIVE_SUFFIXES.iter().any(|s| upper.ends_with(s))
            || SENSITIVE_CONTAINS.iter().any(|c| upper.contains(c))
    }

    /// Extract common config for Claude (JSON format)
    fn extract_claude_common_config(settings: &Value) -> Result<String, AppError> {
        let mut config = settings.clone();

        // 供应商专属的**非机密**字段（模型 + 端点），不应共享。凭据/机密不在此列举，
        // 改由 `is_sensitive_config_key`（模式匹配）统一剥离，新供应商的 `*_API_KEY`
        // 等无需再手工补名单即可被覆盖。
        const ENV_PROVIDER_SPECIFIC_EXCLUDES: &[&str] = &[
            "ANTHROPIC_MODEL",
            "ANTHROPIC_REASONING_MODEL", // legacy: 已废弃，但旧配置可能残留
            "ANTHROPIC_DEFAULT_HAIKU_MODEL",
            "ANTHROPIC_DEFAULT_HAIKU_MODEL_NAME",
            "ANTHROPIC_DEFAULT_OPUS_MODEL",
            "ANTHROPIC_DEFAULT_OPUS_MODEL_NAME",
            "ANTHROPIC_DEFAULT_SONNET_MODEL",
            "ANTHROPIC_DEFAULT_SONNET_MODEL_NAME",
            // Fable 是 v3.16.3 新增的第四档模型映射，与 haiku/sonnet/opus 同属供应商专属，
            // 不得进入通用配置片段，否则会污染其它供应商（issue #4272）。
            "ANTHROPIC_DEFAULT_FABLE_MODEL",
            "ANTHROPIC_DEFAULT_FABLE_MODEL_NAME",
            "CLAUDE_CODE_SUBAGENT_MODEL",
            // Context limits follow the actual upstream model. Sharing these
            // across providers can cap GPT/Kimi to the wrong window and make
            // Claude Code compact too early or miss the upstream limit.
            "CLAUDE_CODE_MAX_CONTEXT_TOKENS",
            "CLAUDE_CODE_AUTO_COMPACT_WINDOW",
            "ANTHROPIC_BASE_URL",
        ];

        const TOP_LEVEL_EXCLUDES: &[&str] = &[
            "apiBaseUrl",
            // Legacy model fields
            "primaryModel",
            "smallFastModel",
        ];

        // Remove env fields: provider-specific (models/endpoint) + 任何凭据键。
        if let Some(env) = config.get_mut("env").and_then(|v| v.as_object_mut()) {
            let sensitive: Vec<String> = env
                .keys()
                .filter(|k| Self::is_sensitive_config_key(k))
                .cloned()
                .collect();
            for key in ENV_PROVIDER_SPECIFIC_EXCLUDES {
                env.remove(*key);
            }
            for key in &sensitive {
                env.remove(key);
            }
            // If env is empty after removal, remove the env object itself
            if env.is_empty() {
                config.as_object_mut().map(|obj| obj.remove("env"));
            }
        }

        // Remove top-level fields: legacy model fields + 任何凭据键
        // （例如非标准的顶层 apiKey / api_key / *_TOKEN）。
        if let Some(obj) = config.as_object_mut() {
            let sensitive: Vec<String> = obj
                .keys()
                .filter(|k| Self::is_sensitive_config_key(k))
                .cloned()
                .collect();
            for key in TOP_LEVEL_EXCLUDES {
                obj.remove(*key);
            }
            for key in &sensitive {
                obj.remove(key);
            }
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

        // Remove provider-specific fields.
        let root = doc.as_table_mut();
        root.remove("model");
        root.remove("model_provider");
        // Legacy/alt formats might use a top-level base_url.
        root.remove("base_url");
        // wire_api 与 base_url 同属供应商路由语义：无 model_provider 时
        // update_codex_toml_field / 前端 setCodexWireApi 都会把它落在顶层，
        // 进了片段会改写其它供应商的协议选择（chat vs responses）。
        root.remove("wire_api");

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
                // 端点按名剥离（它不是凭据，模式匹配够不着）；凭据全部交给
                // `is_sensitive_config_key` 统一模式匹配（与 Claude 提取器一致）。
                // 只列固定名单会漏掉下一个 `*_API_KEY` —— 例如 `GOOGLE_API_KEY`
                // （provider.rs 认可的一等 Gemini 凭据），而共享片段会被 deep-merge
                // 回其它 Gemini 供应商，漏剥即等于把 A 账号的密钥写进 B 供应商并
                // 发往 B 的 base_url。`GEMINI_API_KEY` 不必单列：`_KEY` 后缀已覆盖。
                if key == "GOOGLE_GEMINI_BASE_URL" || Self::is_sensitive_config_key(key) {
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
    /// 等一等凭据会进入共享片段，再被 `apply_common_config_to_settings` 深合并进
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

        // 4) 代理接管中的 live 快照里也可能有一份副本。这一步的失败**必须传播**：
        //
        //    关代理时 `restore_live_config_for_app_with_fallback_inner`（proxy.rs:869）
        //    会把这份快照原样写回 `~/.gemini/.env`。若它仍带毒而我们照样清了片段、置了
        //    完成标记，那么代理一停凭据就当场复活，而一次性标记又保证不会再清第二次；
        //    此后片段里已没有这个键，下一次切换的 backfill 就把它永久写进受害供应商的
        //    配置——还是本函数开头那个顺序陷阱，只是换了扇门进来。
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
            if let Some(options) = obj.get_mut("options").and_then(|v| v.as_object_mut()) {
                options.remove("apiKey");
                options.remove("baseURL");
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

    pub(crate) fn write_gemini_live(provider: &Provider) -> Result<(), AppError> {
        write_gemini_live(provider)
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
                    // 回填快照只要求 TOML 语法合法。
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
            AppType::Cursor => {
                // Cursor: accept any JSON object (env vars)
                if !provider.settings_config.is_object() {
                    return Err(AppError::localized(
                        "provider.cursor.settings.not_object",
                        "Cursor 配置必须是 JSON 对象",
                        "Cursor configuration must be a JSON object",
                    ));
                }
            }
            AppType::Pi => {
                crate::pi_config::validate_provider_node(&provider.id, &provider.settings_config)?;
            }
        }

        // Validate and clean UsageScript configuration (common for all app types)
        if let Some(meta) = &provider.meta {
            if let Some(multiplier) = meta.cost_multiplier.as_deref() {
                validate_cost_multiplier(multiplier)?;
            }
            if let Some(source) = meta.pricing_model_source.as_deref() {
                validate_pricing_source(source)?;
            }
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
            AppType::OpenClaw | AppType::Hermes | AppType::Pi => {
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
            AppType::Cursor => {
                // Cursor uses a flat canonical settingsConfig: { baseUrl, apiKey, model }
                let settings = &provider.settings_config;
                let api_key = settings
                    .get("apiKey")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| {
                        AppError::localized(
                            "provider.cursor.api_key.missing",
                            "缺少 API Key",
                            "API key is missing",
                        )
                    })?
                    .to_string();

                let base_url = settings
                    .get("baseUrl")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| {
                        AppError::localized(
                            "provider.cursor.base_url.missing",
                            "缺少 Base URL 配置",
                            "Missing Base URL configuration",
                        )
                    })?
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
        let is_current = match crate::settings::get_effective_current_provider(&state.db, &app_type)
        {
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
