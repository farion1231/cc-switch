//! Provider service module
//!
//! Handles provider CRUD operations, switching, and configuration management.

mod endpoints;
mod gemini_auth;
mod live;
mod usage;

use indexmap::IndexMap;
use regex::Regex;
use serde::Deserialize;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

use crate::app_config::AppType;
use crate::codex_config::CC_SWITCH_CODEX_MODEL_PROVIDER_ID;
use crate::config::{get_claude_settings_path, write_json_file};
use crate::database::{validate_cost_multiplier, validate_pricing_source};
use crate::error::AppError;
use crate::provider::{ClaudeActivationMode, Provider, UsageResult};
use crate::services::mcp::McpService;
use crate::settings::CustomEndpoint;
use crate::store::AppState;

// Re-export sub-module functions for external access
pub use live::{
    import_default_config, import_hermes_providers_from_live, import_openclaw_providers_from_live,
    import_opencode_providers_from_live, read_live_settings,
    should_import_default_config_on_startup, sync_current_to_live,
};

// Internal re-exports (pub(crate))
pub(crate) use live::sanitize_claude_settings_for_live;
pub(crate) use live::{
    build_effective_settings_with_common_config, normalize_provider_common_config_for_storage,
    provider_exists_in_live_config, strip_common_config_from_live_settings,
    sync_current_provider_for_app_to_live, write_claude_profile_with_common_config,
    write_live_with_common_config,
};

// Internal re-exports
use live::{
    remove_hermes_provider_from_live, remove_openclaw_provider_from_live,
    remove_opencode_provider_from_live, write_gemini_live,
};
use usage::validate_usage_script;

/// Provider business logic service
pub struct ProviderService;

#[derive(Debug, Clone)]
struct ClaudeSwitchPlan {
    activation_mode: ClaudeActivationMode,
    override_dir: Option<String>,
}

#[derive(Debug, Clone)]
struct ClaudeRollbackState {
    previous_provider_override_dir: Option<String>,
    previous_local_current: Option<String>,
    previous_db_current: Option<String>,
    previous_live_settings: Option<Value>,
    target_live_path: Option<PathBuf>,
    target_live_settings: Option<Value>,
    previous_config_env: Option<String>,
}

/// Result of a provider switch operation, including any non-fatal warnings
#[derive(Debug, serde::Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SwitchResult {
    pub warnings: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{get_claude_settings_path, read_json_file, write_json_file};
    use crate::database::Database;
    use crate::provider::{ClaudeActivationMode, ProviderMeta};
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
        original_userprofile: Option<String>,
        original_test_home: Option<String>,
    }

    struct UserEnvVarGuard {
        name: &'static str,
        original: Option<String>,
    }

    impl UserEnvVarGuard {
        fn new(name: &'static str) -> Self {
            Self {
                name,
                original: crate::services::env_manager::get_user_env_var(name)
                    .expect("capture user env var"),
            }
        }
    }

    impl Drop for UserEnvVarGuard {
        fn drop(&mut self) {
            let _ =
                crate::services::env_manager::set_user_env_var(self.name, self.original.as_deref());
        }
    }

    impl TempHome {
        fn new() -> Self {
            let dir = TempDir::new().expect("failed to create temp home");
            let original_home = env::var("HOME").ok();
            let original_userprofile = env::var("USERPROFILE").ok();
            let original_test_home = env::var("CC_SWITCH_TEST_HOME").ok();

            env::set_var("HOME", dir.path());
            env::set_var("USERPROFILE", dir.path());
            env::set_var("CC_SWITCH_TEST_HOME", dir.path());

            Self {
                dir,
                original_home,
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
        let old_app_config_override = crate::app_store::get_app_config_dir_override();
        std::env::set_var("CC_SWITCH_TEST_HOME", temp.path());
        std::env::set_var("HOME", temp.path());
        crate::app_store::set_app_config_dir_override_for_tests(None);
        crate::settings::update_settings(crate::settings::AppSettings::default())
            .expect("reset settings");

        let db = Arc::new(Database::memory().expect("in-memory database"));
        let state = AppState::new(db);
        let result = test(&state, temp.path());

        crate::app_store::set_app_config_dir_override_for_tests(old_app_config_override);
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

    fn claude_provider(
        id: &str,
        name: &str,
        token: &str,
        base_url: &str,
        meta: Option<ProviderMeta>,
    ) -> Provider {
        Provider {
            id: id.to_string(),
            name: name.to_string(),
            settings_config: json!({
                "env": {
                    "ANTHROPIC_AUTH_TOKEN": token,
                    "ANTHROPIC_BASE_URL": base_url
                }
            }),
            website_url: None,
            category: Some("custom".to_string()),
            created_at: Some(1),
            sort_index: Some(0),
            notes: None,
            meta,
            icon: None,
            icon_color: None,
            in_failover_queue: false,
        }
    }

    fn codex_provider(id: &str, name: &str, config: &str) -> Provider {
        Provider {
            id: id.to_string(),
            name: name.to_string(),
            settings_config: json!({
                "auth": {},
                "config": config,
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

    fn seed_codex_thread_rows(home: &Path, rows: &[(&str, &str)]) {
        let codex_dir = home.join(".codex");
        let rollout_dir = codex_dir
            .join("sessions")
            .join("2026")
            .join("04")
            .join("24");
        fs::create_dir_all(&codex_dir).expect("create codex dir");
        fs::create_dir_all(&rollout_dir).expect("create rollout dir");
        let conn = rusqlite::Connection::open(codex_dir.join("state_5.sqlite"))
            .expect("open codex state db");
        conn.execute(
            "CREATE TABLE threads (id TEXT PRIMARY KEY, model_provider TEXT NOT NULL, rollout_path TEXT NOT NULL)",
            [],
        )
        .expect("create threads table");
        for (id, provider) in rows {
            let path = rollout_dir.join(format!("rollout-{id}.jsonl"));
            fs::write(
                &path,
                format!(
                    "{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"{id}\",\"model_provider\":\"{provider}\"}}}}\n{{\"type\":\"event_msg\",\"payload\":{{}}}}\n"
                ),
            )
            .expect("write rollout metadata");
            conn.execute(
                "INSERT INTO threads (id, model_provider, rollout_path) VALUES (?1, ?2, ?3)",
                rusqlite::params![id, provider, path.to_string_lossy()],
            )
            .expect("seed thread");
        }
    }

    fn seed_codex_threads(home: &Path, provider: &str) {
        seed_codex_thread_rows(home, &[("thread-1", provider), ("thread-2", provider)]);
    }

    fn seed_codex_thread_rows_nullable(home: &Path, rows: &[(&str, Option<&str>)]) {
        let codex_dir = home.join(".codex");
        let rollout_dir = codex_dir
            .join("sessions")
            .join("2026")
            .join("04")
            .join("24");
        fs::create_dir_all(&codex_dir).expect("create codex dir");
        fs::create_dir_all(&rollout_dir).expect("create rollout dir");
        let conn = rusqlite::Connection::open(codex_dir.join("state_5.sqlite"))
            .expect("open codex state db");
        conn.execute(
            "CREATE TABLE threads (id TEXT PRIMARY KEY, model_provider TEXT NULL, rollout_path TEXT NOT NULL)",
            [],
        )
        .expect("create threads table");
        for (id, provider) in rows {
            let path = rollout_dir.join(format!("rollout-{id}.jsonl"));
            let provider_json = provider
                .map(|provider| format!(r#","model_provider":"{provider}""#))
                .unwrap_or_default();
            fs::write(
                &path,
                format!(
                    "{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"{id}\"{provider_json}}}}}\n{{\"type\":\"event_msg\",\"payload\":{{}}}}\n"
                ),
            )
            .expect("write rollout metadata");
            conn.execute(
                "INSERT INTO threads (id, model_provider, rollout_path) VALUES (?1, ?2, ?3)",
                rusqlite::params![id, provider, path.to_string_lossy()],
            )
            .expect("seed thread");
        }
    }

    fn codex_rollout_providers(home: &Path) -> Vec<(String, Option<String>)> {
        let conn = rusqlite::Connection::open(home.join(".codex").join("state_5.sqlite"))
            .expect("open codex state db");
        let mut stmt = conn
            .prepare("SELECT id, rollout_path FROM threads ORDER BY id")
            .expect("prepare rollout query");
        stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .expect("query rollouts")
        .map(|row| {
            let (id, path) = row.expect("rollout row");
            let first_line = fs::read_to_string(path)
                .expect("read rollout")
                .lines()
                .next()
                .expect("rollout first line")
                .to_string();
            let value: serde_json::Value =
                serde_json::from_str(&first_line).expect("parse rollout first line");
            let provider = value
                .get("payload")
                .and_then(|payload| payload.get("model_provider"))
                .and_then(|provider| provider.as_str())
                .map(str::to_string);
            (id, provider)
        })
        .collect()
    }

    fn codex_thread_providers(home: &Path) -> Vec<(String, i64)> {
        let conn = rusqlite::Connection::open(home.join(".codex").join("state_5.sqlite"))
            .expect("open codex state db");
        let mut stmt = conn
            .prepare(
                "SELECT model_provider, COUNT(*) FROM threads GROUP BY model_provider ORDER BY model_provider",
            )
            .expect("prepare provider query");
        stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .expect("query providers")
            .map(|row| row.expect("provider row"))
            .collect()
    }

    fn rewrite_codex_thread_provider(home: &Path, id: &str, provider: &str) {
        let conn = rusqlite::Connection::open(home.join(".codex").join("state_5.sqlite"))
            .expect("open codex state db");
        let rollout_path: String = conn
            .query_row(
                "SELECT rollout_path FROM threads WHERE id = ?1",
                [id],
                |row| row.get(0),
            )
            .expect("query rollout path");
        conn.execute(
            "UPDATE threads SET model_provider = ?1 WHERE id = ?2",
            rusqlite::params![provider, id],
        )
        .expect("rewrite thread provider");

        let path = PathBuf::from(rollout_path);
        let text = fs::read_to_string(&path).expect("read rollout");
        let Some(first_newline) = text.find('\n') else {
            panic!("rollout should contain a first line");
        };
        let first_line = &text[..first_newline];
        let rest = &text[first_newline..];
        let mut value: serde_json::Value =
            serde_json::from_str(first_line).expect("parse rollout metadata");
        value["payload"]["model_provider"] = serde_json::Value::String(provider.to_string());
        let first_line = serde_json::to_string(&value).expect("serialize rollout metadata");
        fs::write(path, format!("{first_line}{rest}")).expect("rewrite rollout");
    }

    #[test]
    #[serial]
    fn switch_codex_to_official_syncs_desktop_threads_to_openai_provider() {
        with_test_home(|state, home| {
            let api_provider = codex_provider(
                "codex-api",
                "Codex API",
                "model_provider = \"OpenAI\"\nmodel = \"gpt-5.4\"\n",
            );
            let official_provider = codex_provider("codex-official", "OpenAI Official", "");

            state
                .db
                .save_provider(AppType::Codex.as_str(), &api_provider)
                .expect("save api provider");
            state
                .db
                .save_provider(AppType::Codex.as_str(), &official_provider)
                .expect("save official provider");
            state
                .db
                .set_current_provider(AppType::Codex.as_str(), "codex-api")
                .expect("set current provider");
            crate::settings::set_current_provider(&AppType::Codex, Some("codex-api"))
                .expect("set local current provider");
            seed_codex_threads(home, "OpenAI");

            ProviderService::switch(state, AppType::Codex, "codex-official")
                .expect("switch to official codex provider");

            assert_eq!(
                codex_thread_providers(home),
                vec![("openai".to_string(), 2)],
                "OAuth login should make Codex Desktop history visible under the openai provider key"
            );
            assert_eq!(
                codex_rollout_providers(home),
                vec![
                    ("thread-1".to_string(), Some("openai".to_string())),
                    ("thread-2".to_string(), Some("openai".to_string())),
                ],
                "OAuth login should update Codex rollout session metadata"
            );
        });
    }

    #[test]
    #[serial]
    fn switch_codex_to_api_syncs_desktop_threads_to_stable_provider_key() {
        with_test_home(|state, home| {
            let official_provider = codex_provider("codex-official", "OpenAI Official", "");
            let api_provider = codex_provider(
                "codex-api",
                "Codex API",
                "model_provider = \"OpenAI\"\nmodel = \"gpt-5.4\"\n[model_providers.OpenAI]\nname = \"OpenAI\"\nbase_url = \"http://127.0.0.1:12345/v1\"\n",
            );

            state
                .db
                .save_provider(AppType::Codex.as_str(), &official_provider)
                .expect("save official provider");
            state
                .db
                .save_provider(AppType::Codex.as_str(), &api_provider)
                .expect("save api provider");
            state
                .db
                .set_current_provider(AppType::Codex.as_str(), "codex-official")
                .expect("set current provider");
            crate::settings::set_current_provider(&AppType::Codex, Some("codex-official"))
                .expect("set local current provider");
            seed_codex_threads(home, "OpenAI");

            ProviderService::switch(state, AppType::Codex, "codex-api")
                .expect("switch to api codex provider");

            assert_eq!(
                codex_thread_providers(home),
                vec![("ccswitch".to_string(), 2)],
                "API login should make Codex Desktop history visible under the stable cc-switch provider key"
            );
            assert_eq!(
                codex_rollout_providers(home),
                vec![
                    ("thread-1".to_string(), Some("ccswitch".to_string())),
                    ("thread-2".to_string(), Some("ccswitch".to_string())),
                ],
                "API login should update Codex rollout session metadata to the stable cc-switch provider key"
            );
        });
    }

    #[test]
    #[serial]
    fn switch_codex_only_relabels_threads_from_previous_provider() {
        with_test_home(|state, home| {
            let official_provider = codex_provider("codex-official", "OpenAI Official", "");
            let api_provider = codex_provider(
                "codex-api",
                "Codex API",
                "model_provider = \"OpenAI\"\nmodel = \"gpt-5.4\"\n",
            );
            let other_provider = codex_provider(
                "codex-other",
                "Codex Other",
                "model_provider = \"azure\"\nmodel = \"gpt-5.4\"\n",
            );

            state
                .db
                .save_provider(AppType::Codex.as_str(), &official_provider)
                .expect("save official provider");
            state
                .db
                .save_provider(AppType::Codex.as_str(), &api_provider)
                .expect("save api provider");
            state
                .db
                .save_provider(AppType::Codex.as_str(), &other_provider)
                .expect("save other provider");
            state
                .db
                .set_current_provider(AppType::Codex.as_str(), "codex-official")
                .expect("set current provider");
            crate::settings::set_current_provider(&AppType::Codex, Some("codex-official"))
                .expect("set local current provider");
            seed_codex_thread_rows(
                home,
                &[
                    ("official-thread", "openai"),
                    ("api-thread", "OpenAI"),
                    ("other-thread", "azure"),
                ],
            );

            ProviderService::switch(state, AppType::Codex, "codex-api")
                .expect("switch to api codex provider");

            assert_eq!(
                codex_thread_providers(home),
                vec![("OpenAI".to_string(), 2), ("azure".to_string(), 1)],
                "switching from official to API should not relabel unrelated provider history"
            );
            assert_eq!(
                codex_rollout_providers(home),
                vec![
                    ("api-thread".to_string(), Some("OpenAI".to_string())),
                    ("official-thread".to_string(), Some("OpenAI".to_string())),
                    ("other-thread".to_string(), Some("azure".to_string())),
                ],
                "rollout metadata should only move previous-provider sessions"
            );
        });
    }

    #[test]
    #[serial]
    fn switch_codex_settles_previous_provider_rewrite_race() {
        with_test_home(|state, home| {
            let official_provider = codex_provider("codex-official", "OpenAI Official", "");
            let api_provider = codex_provider(
                "codex-api",
                "Codex API",
                "model_provider = \"OpenAI\"\nmodel = \"gpt-5.4\"\n",
            );

            state
                .db
                .save_provider(AppType::Codex.as_str(), &official_provider)
                .expect("save official provider");
            state
                .db
                .save_provider(AppType::Codex.as_str(), &api_provider)
                .expect("save api provider");
            state
                .db
                .set_current_provider(AppType::Codex.as_str(), "codex-official")
                .expect("set current provider");
            crate::settings::set_current_provider(&AppType::Codex, Some("codex-official"))
                .expect("set local current provider");
            seed_codex_thread_rows(
                home,
                &[("official-thread", "openai"), ("racing-thread", "openai")],
            );

            let home_for_race = home.to_path_buf();
            let race = std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(50));
                rewrite_codex_thread_provider(&home_for_race, "racing-thread", "openai");
            });

            ProviderService::switch(state, AppType::Codex, "codex-api")
                .expect("switch to api codex provider");
            race.join().expect("race writer should finish");

            assert_eq!(
                codex_thread_providers(home),
                vec![("OpenAI".to_string(), 2)],
                "switch should settle stale writes from the previous Codex provider"
            );
            assert_eq!(
                codex_rollout_providers(home),
                vec![
                    ("official-thread".to_string(), Some("OpenAI".to_string())),
                    ("racing-thread".to_string(), Some("OpenAI".to_string())),
                ],
                "rollout metadata should also settle stale previous-provider writes"
            );
        });
    }

    #[test]
    #[serial]
    fn switch_codex_updates_null_rollout_metadata_when_scoped_to_previous_provider() {
        with_test_home(|state, home| {
            let official_provider = codex_provider("codex-official", "OpenAI Official", "");
            let api_provider = codex_provider(
                "codex-api",
                "Codex API",
                "model_provider = \"OpenAI\"\nmodel = \"gpt-5.4\"\n",
            );
            let other_provider = codex_provider(
                "codex-other",
                "Codex Other",
                "model_provider = \"azure\"\nmodel = \"gpt-5.4\"\n",
            );

            state
                .db
                .save_provider(AppType::Codex.as_str(), &official_provider)
                .expect("save official provider");
            state
                .db
                .save_provider(AppType::Codex.as_str(), &api_provider)
                .expect("save api provider");
            state
                .db
                .save_provider(AppType::Codex.as_str(), &other_provider)
                .expect("save other provider");
            state
                .db
                .set_current_provider(AppType::Codex.as_str(), "codex-official")
                .expect("set current provider");
            crate::settings::set_current_provider(&AppType::Codex, Some("codex-official"))
                .expect("set local current provider");
            seed_codex_thread_rows_nullable(
                home,
                &[
                    ("old-null-thread", None),
                    ("official-thread", Some("openai")),
                    ("other-thread", Some("azure")),
                ],
            );

            ProviderService::switch(state, AppType::Codex, "codex-api")
                .expect("switch to api codex provider");

            assert_eq!(
                codex_thread_providers(home),
                vec![("OpenAI".to_string(), 2), ("azure".to_string(), 1)],
                "null provider rows selected by the scoped SQL should move to the target provider"
            );
            assert_eq!(
                codex_rollout_providers(home),
                vec![
                    ("official-thread".to_string(), Some("OpenAI".to_string())),
                    ("old-null-thread".to_string(), Some("OpenAI".to_string())),
                    ("other-thread".to_string(), Some("azure".to_string())),
                ],
                "rollout metadata with a missing provider should stay consistent with relabeled null rows"
            );
        });
    }

    #[tokio::test]
    #[serial]
    async fn hot_switch_codex_only_relabels_threads_from_previous_provider() {
        let _home = TempHome::new();
        crate::settings::reload_settings().expect("reload settings");

        let db = Arc::new(Database::memory().expect("init db"));
        let state = AppState::new(db.clone());
        let home = PathBuf::from(std::env::var("CC_SWITCH_TEST_HOME").expect("test home"));

        let official_provider = codex_provider("codex-official", "OpenAI Official", "");
        let api_provider = codex_provider(
            "codex-api",
            "Codex API",
            "model_provider = \"OpenAI\"\nmodel = \"gpt-5.4\"\n",
        );
        let other_provider = codex_provider(
            "codex-other",
            "Codex Other",
            "model_provider = \"azure\"\nmodel = \"gpt-5.4\"\n",
        );

        db.save_provider(AppType::Codex.as_str(), &official_provider)
            .expect("save official provider");
        db.save_provider(AppType::Codex.as_str(), &api_provider)
            .expect("save api provider");
        db.save_provider(AppType::Codex.as_str(), &other_provider)
            .expect("save other provider");
        db.set_current_provider(AppType::Codex.as_str(), "codex-official")
            .expect("set current provider");
        crate::settings::set_current_provider(&AppType::Codex, Some("codex-official"))
            .expect("set local current provider");
        seed_codex_thread_rows(
            &home,
            &[
                ("official-thread", "openai"),
                ("api-thread", "OpenAI"),
                ("other-thread", "azure"),
            ],
        );
        db.save_live_backup("codex", "{}")
            .await
            .expect("seed codex takeover backup");
        state
            .proxy_service
            .start()
            .await
            .expect("start proxy service");

        ProviderService::switch(&state, AppType::Codex, "codex-api")
            .expect("hot switch to api codex provider");

        assert_eq!(
            codex_thread_providers(&home),
            vec![("OpenAI".to_string(), 2), ("azure".to_string(), 1)],
            "hot-switching from official to API should not relabel unrelated provider history"
        );
        assert_eq!(
            codex_rollout_providers(&home),
            vec![
                ("api-thread".to_string(), Some("OpenAI".to_string())),
                ("official-thread".to_string(), Some("OpenAI".to_string())),
                ("other-thread".to_string(), Some("azure".to_string())),
            ],
            "hot-switch rollout metadata should only move previous-provider sessions"
        );
    }

    #[test]
    #[serial]
    fn sync_current_claude_profile_env_preserves_external_override_without_active_provider() {
        with_test_home(|state, home| {
            let external_dir = home.join(".external-claude");
            fs::create_dir_all(&external_dir).expect("create external claude dir");
            crate::settings::set_claude_provider_override_dir(Some(
                &external_dir.to_string_lossy(),
            ))
            .expect("set external override dir");

            ProviderService::sync_current_claude_profile_env(state)
                .expect("sync without active provider should succeed");

            assert_eq!(
                crate::settings::get_claude_override_dir().as_deref(),
                Some(external_dir.as_path()),
                "passive sync should not clear an externally configured Claude override when CC Switch has no active Claude provider"
            );
        });
    }

    fn omo_config_path(home: &Path, category: &str) -> PathBuf {
        home.join(".config").join("opencode").join(match category {
            "omo" => crate::services::omo::STANDARD.preferred_filename,
            "omo-slim" => crate::services::omo::SLIM.preferred_filename,
            other => panic!("unexpected OMO category in test: {other}"),
        })
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
    fn switch_claude_profile_only_reuses_profile_dir_without_overwriting_live_settings() {
        with_test_home(|state, home| {
            let default_dir = home.join(".claude");
            fs::create_dir_all(&default_dir).expect("create default claude dir");
            write_json_file(
                &default_dir.join("settings.json"),
                &json!({
                    "env": {
                        "ANTHROPIC_AUTH_TOKEN": "default-live-token",
                        "ANTHROPIC_BASE_URL": "https://default-live.example"
                    }
                }),
            )
            .expect("seed default live settings");

            let official_dir = home.join(".claude-profiles").join("official");
            fs::create_dir_all(&official_dir).expect("create official profile dir");
            write_json_file(
                &official_dir.join("settings.json"),
                &json!({
                    "env": {
                        "ANTHROPIC_AUTH_TOKEN": "official-live-token",
                        "ANTHROPIC_BASE_URL": "https://official-live.example"
                    }
                }),
            )
            .expect("seed official profile settings");

            let default_provider = claude_provider(
                "default",
                "Default",
                "default-provider-token",
                "https://default-provider.example",
                None,
            );
            let official_provider = claude_provider(
                "claude-official",
                "Claude Official",
                "provider-token-should-not-write",
                "https://provider-should-not-write.example",
                Some(ProviderMeta {
                    claude_profile_dir: Some(official_dir.to_string_lossy().to_string()),
                    claude_activation_mode: Some(ClaudeActivationMode::ProfileOnly),
                    ..Default::default()
                }),
            );

            state
                .db
                .save_provider(AppType::Claude.as_str(), &default_provider)
                .expect("save default provider");
            state
                .db
                .save_provider(AppType::Claude.as_str(), &official_provider)
                .expect("save official provider");
            state
                .db
                .set_current_provider(AppType::Claude.as_str(), "default")
                .expect("set current provider");
            crate::settings::set_current_provider(&AppType::Claude, Some("default"))
                .expect("set local current provider");

            ProviderService::switch(state, AppType::Claude, "claude-official")
                .expect("switch to official profile");

            assert_eq!(
                crate::settings::get_current_provider(&AppType::Claude).as_deref(),
                Some("claude-official"),
                "local current provider should switch"
            );
            assert_eq!(
                crate::settings::get_effective_current_provider(&state.db, &AppType::Claude)
                    .expect("effective current provider")
                    .as_deref(),
                Some("claude-official"),
                "effective current provider should switch"
            );
            assert_eq!(
                crate::settings::get_claude_override_dir().as_deref(),
                Some(official_dir.as_path()),
                "claude override dir should point to the profile"
            );

            let live: Value = read_json_file(&get_claude_settings_path()).expect("read live");
            assert_eq!(
                live["env"]["ANTHROPIC_AUTH_TOKEN"],
                Value::String("official-live-token".to_string()),
                "profile-only mode should preserve the existing auth profile contents"
            );
            assert_eq!(
                live["env"]["ANTHROPIC_BASE_URL"],
                Value::String("https://official-live.example".to_string()),
                "profile-only mode should keep the existing base URL"
            );
        });
    }

    #[test]
    #[serial]
    fn sync_current_claude_profile_only_preserves_profile_live_settings() {
        with_test_home(|state, home| {
            let official_dir = home.join(".claude-profiles").join("official");
            fs::create_dir_all(&official_dir).expect("create official profile dir");
            write_json_file(
                &official_dir.join("settings.json"),
                &json!({
                    "env": {
                        "ANTHROPIC_AUTH_TOKEN": "official-live-token",
                        "ANTHROPIC_BASE_URL": "https://official-live.example"
                    }
                }),
            )
            .expect("seed official profile settings");

            let official_provider = claude_provider(
                "claude-official",
                "Claude Official",
                "provider-token-should-not-write",
                "https://provider-should-not-write.example",
                Some(ProviderMeta {
                    claude_profile_dir: Some(official_dir.to_string_lossy().to_string()),
                    claude_activation_mode: Some(ClaudeActivationMode::ProfileOnly),
                    ..Default::default()
                }),
            );

            state
                .db
                .save_provider(AppType::Claude.as_str(), &official_provider)
                .expect("save official provider");
            state
                .db
                .set_current_provider(AppType::Claude.as_str(), "claude-official")
                .expect("set current provider");
            crate::settings::set_current_provider(&AppType::Claude, Some("claude-official"))
                .expect("set local current provider");
            crate::settings::set_claude_provider_override_dir(Some(
                &official_dir.to_string_lossy(),
            ))
            .expect("set active profile override");

            ProviderService::sync_current_provider_for_app(state, AppType::Claude)
                .expect("sync current profile-only provider");

            let live: Value =
                read_json_file(&official_dir.join("settings.json")).expect("read official profile");
            assert_eq!(
                live["env"]["ANTHROPIC_AUTH_TOKEN"],
                Value::String("official-live-token".to_string()),
                "sync-current should not overwrite profile-only auth profile contents"
            );
            assert_eq!(
                live["env"]["ANTHROPIC_BASE_URL"],
                Value::String("https://official-live.example".to_string()),
                "sync-current should keep the profile-only base URL"
            );
        });
    }

    #[test]
    #[serial]
    fn switch_away_from_claude_profile_only_does_not_backfill_external_profile_settings() {
        with_test_home(|state, home| {
            let default_dir = home.join(".claude");
            fs::create_dir_all(&default_dir).expect("create default claude dir");
            write_json_file(
                &default_dir.join("settings.json"),
                &json!({
                    "env": {
                        "ANTHROPIC_AUTH_TOKEN": "default-live-token",
                        "ANTHROPIC_BASE_URL": "https://default-live.example"
                    }
                }),
            )
            .expect("seed default live settings");

            let profile_dir = home.join(".claude-profiles").join("external");
            fs::create_dir_all(&profile_dir).expect("create profile dir");
            write_json_file(
                &profile_dir.join("settings.json"),
                &json!({
                    "env": {
                        "ANTHROPIC_AUTH_TOKEN": "external-profile-token",
                        "ANTHROPIC_BASE_URL": "https://external-profile.example"
                    }
                }),
            )
            .expect("seed external profile settings");

            let profile_only_provider = claude_provider(
                "claude-profile-only",
                "Claude Profile Only",
                "stored-profile-provider-token",
                "https://stored-profile-provider.example",
                Some(ProviderMeta {
                    claude_profile_dir: Some(profile_dir.to_string_lossy().to_string()),
                    claude_activation_mode: Some(ClaudeActivationMode::ProfileOnly),
                    ..Default::default()
                }),
            );
            let legacy_provider = claude_provider(
                "claude-legacy",
                "Claude Legacy",
                "legacy-provider-token",
                "https://legacy-provider.example",
                None,
            );

            state
                .db
                .save_provider(AppType::Claude.as_str(), &profile_only_provider)
                .expect("save profile-only provider");
            state
                .db
                .save_provider(AppType::Claude.as_str(), &legacy_provider)
                .expect("save legacy provider");
            state
                .db
                .set_current_provider(AppType::Claude.as_str(), "claude-profile-only")
                .expect("set current provider");
            crate::settings::set_current_provider(&AppType::Claude, Some("claude-profile-only"))
                .expect("set local current provider");
            crate::settings::set_claude_provider_override_dir(Some(&profile_dir.to_string_lossy()))
                .expect("set profile override");

            ProviderService::switch(state, AppType::Claude, "claude-legacy")
                .expect("switch away from profile-only provider");

            let stored = state
                .db
                .get_provider_by_id("claude-profile-only", AppType::Claude.as_str())
                .expect("load stored profile-only provider")
                .expect("stored profile-only provider");
            assert_eq!(
                stored.settings_config["env"]["ANTHROPIC_AUTH_TOKEN"],
                Value::String("stored-profile-provider-token".to_string()),
                "profile-only switch-away should not import external profile auth into provider storage"
            );
            assert_eq!(
                stored.settings_config["env"]["ANTHROPIC_BASE_URL"],
                Value::String("https://stored-profile-provider.example".to_string()),
                "profile-only switch-away should keep stored provider config unchanged"
            );
        });
    }

    #[test]
    #[serial]
    fn switch_claude_profile_and_config_writes_to_target_profile_without_touching_default_live() {
        with_test_home(|state, home| {
            let default_dir = home.join(".claude");
            fs::create_dir_all(&default_dir).expect("create default claude dir");
            write_json_file(
                &default_dir.join("settings.json"),
                &json!({
                    "env": {
                        "ANTHROPIC_AUTH_TOKEN": "default-live-token",
                        "ANTHROPIC_BASE_URL": "https://default-live.example"
                    }
                }),
            )
            .expect("seed default live settings");

            let api_dir = home.join(".claude-profiles").join("api");

            let default_provider = claude_provider(
                "default",
                "Default",
                "default-provider-token",
                "https://default-provider.example",
                None,
            );
            let api_provider = claude_provider(
                "claude-api",
                "Claude API",
                "api-provider-token",
                "https://api-provider.example",
                Some(ProviderMeta {
                    claude_profile_dir: Some(api_dir.to_string_lossy().to_string()),
                    claude_activation_mode: Some(ClaudeActivationMode::ProfileAndConfig),
                    ..Default::default()
                }),
            );

            state
                .db
                .save_provider(AppType::Claude.as_str(), &default_provider)
                .expect("save default provider");
            state
                .db
                .save_provider(AppType::Claude.as_str(), &api_provider)
                .expect("save api provider");
            state
                .db
                .set_current_provider(AppType::Claude.as_str(), "default")
                .expect("set current provider");
            crate::settings::set_current_provider(&AppType::Claude, Some("default"))
                .expect("set local current provider");

            ProviderService::switch(state, AppType::Claude, "claude-api")
                .expect("switch to api profile");

            assert_eq!(
                crate::settings::get_claude_override_dir().as_deref(),
                Some(api_dir.as_path()),
                "claude override dir should point to the api profile"
            );

            let api_live: Value =
                read_json_file(&api_dir.join("settings.json")).expect("read api profile");
            assert_eq!(
                api_live["env"]["ANTHROPIC_AUTH_TOKEN"],
                Value::String("api-provider-token".to_string()),
                "profile-and-config mode should write provider settings into the target profile"
            );

            let default_live: Value =
                read_json_file(&default_dir.join("settings.json")).expect("read default profile");
            assert_eq!(
                default_live["env"]["ANTHROPIC_AUTH_TOKEN"],
                Value::String("default-live-token".to_string()),
                "switching api profiles must not mutate the default live settings"
            );
        });
    }

    #[test]
    #[serial]
    fn sync_current_claude_profile_and_config_applies_profile_before_live_write() {
        with_test_home(|state, home| {
            let default_dir = home.join(".claude");
            fs::create_dir_all(&default_dir).expect("create default claude dir");
            write_json_file(
                &default_dir.join("settings.json"),
                &json!({
                    "env": {
                        "ANTHROPIC_AUTH_TOKEN": "default-live-token",
                        "ANTHROPIC_BASE_URL": "https://default-live.example"
                    }
                }),
            )
            .expect("seed default live settings");

            let stale_profile_dir = home.join(".claude-profiles").join("stale");
            fs::create_dir_all(&stale_profile_dir).expect("create stale profile dir");
            crate::settings::set_claude_provider_override_dir(Some(
                &stale_profile_dir.to_string_lossy(),
            ))
            .expect("seed stale override");

            let api_dir = home.join(".claude-profiles").join("api");
            let api_provider = claude_provider(
                "claude-api",
                "Claude API",
                "api-provider-token",
                "https://api-provider.example",
                Some(ProviderMeta {
                    claude_profile_dir: Some(api_dir.to_string_lossy().to_string()),
                    claude_activation_mode: Some(ClaudeActivationMode::ProfileAndConfig),
                    ..Default::default()
                }),
            );

            state
                .db
                .save_provider(AppType::Claude.as_str(), &api_provider)
                .expect("save api provider");
            state
                .db
                .set_current_provider(AppType::Claude.as_str(), "claude-api")
                .expect("set current provider");
            crate::settings::set_current_provider(&AppType::Claude, Some("claude-api"))
                .expect("set local current provider");

            ProviderService::sync_current_provider_for_app(state, AppType::Claude)
                .expect("sync current claude provider");

            assert_eq!(
                crate::settings::get_claude_override_dir().as_deref(),
                Some(api_dir.as_path()),
                "sync-current should select the profile-and-config provider profile before writing live settings"
            );

            let api_live: Value =
                read_json_file(&api_dir.join("settings.json")).expect("read api profile");
            assert_eq!(
                api_live["env"]["ANTHROPIC_AUTH_TOKEN"],
                Value::String("api-provider-token".to_string()),
                "sync-current should write provider settings into the selected target profile"
            );

            assert!(
                !stale_profile_dir.join("settings.json").exists(),
                "sync-current must not write provider settings into the previously selected profile"
            );

            let default_live: Value =
                read_json_file(&default_dir.join("settings.json")).expect("read default profile");
            assert_eq!(
                default_live["env"]["ANTHROPIC_AUTH_TOKEN"],
                Value::String("default-live-token".to_string()),
                "sync-current profile-and-config should not mutate the default live settings"
            );
        });
    }

    #[test]
    #[serial]
    fn add_first_claude_profile_only_sets_profile_without_overwriting_default_live() {
        with_test_home(|state, home| {
            let default_dir = home.join(".claude");
            fs::create_dir_all(&default_dir).expect("create default claude dir");
            write_json_file(
                &default_dir.join("settings.json"),
                &json!({
                    "env": {
                        "ANTHROPIC_AUTH_TOKEN": "default-live-token",
                        "ANTHROPIC_BASE_URL": "https://default-live.example"
                    }
                }),
            )
            .expect("seed default live settings");

            let official_dir = home.join(".claude-profiles").join("official");
            fs::create_dir_all(&official_dir).expect("create official profile dir");
            write_json_file(
                &official_dir.join("settings.json"),
                &json!({
                    "env": {
                        "ANTHROPIC_AUTH_TOKEN": "official-live-token",
                        "ANTHROPIC_BASE_URL": "https://official-live.example"
                    }
                }),
            )
            .expect("seed official profile settings");

            let official_provider = claude_provider(
                "claude-official",
                "Claude Official",
                "provider-token-should-not-write",
                "https://provider-should-not-write.example",
                Some(ProviderMeta {
                    claude_profile_dir: Some(official_dir.to_string_lossy().to_string()),
                    claude_activation_mode: Some(ClaudeActivationMode::ProfileOnly),
                    ..Default::default()
                }),
            );

            ProviderService::add(state, AppType::Claude, official_provider, true)
                .expect("add first profile-only provider");

            assert_eq!(
                crate::settings::get_effective_current_provider(&state.db, &AppType::Claude)
                    .expect("effective current provider")
                    .as_deref(),
                Some("claude-official"),
                "first added provider should become current"
            );
            assert_eq!(
                crate::settings::get_claude_override_dir().as_deref(),
                Some(official_dir.as_path()),
                "profile-only add should set the selected profile dir"
            );

            let default_live: Value =
                read_json_file(&default_dir.join("settings.json")).expect("read default profile");
            assert_eq!(
                default_live["env"]["ANTHROPIC_AUTH_TOKEN"],
                Value::String("default-live-token".to_string()),
                "profile-only add should not overwrite the default live settings"
            );
        });
    }

    #[test]
    #[serial]
    fn add_first_claude_profile_only_rejects_missing_dir_before_saving() {
        with_test_home(|state, home| {
            let missing_dir = home.join(".claude-profiles").join("missing-official");
            let broken_provider = claude_provider(
                "claude-official",
                "Claude Official",
                "provider-token-should-not-save",
                "https://provider-should-not-save.example",
                Some(ProviderMeta {
                    claude_profile_dir: Some(missing_dir.to_string_lossy().to_string()),
                    claude_activation_mode: Some(ClaudeActivationMode::ProfileOnly),
                    ..Default::default()
                }),
            );

            let err = ProviderService::add(state, AppType::Claude, broken_provider, true)
                .expect_err("missing profile-only dir should reject first add");
            assert!(
                err.to_string().contains("profile"),
                "expected profile validation error, got {err:?}"
            );
            assert!(
                state
                    .db
                    .get_provider_by_id("claude-official", AppType::Claude.as_str())
                    .expect("query provider")
                    .is_none(),
                "failed first add must not persist invalid provider"
            );
            assert!(
                crate::settings::get_effective_current_provider(&state.db, &AppType::Claude)
                    .expect("effective current provider")
                    .is_none(),
                "failed first add must not set current provider"
            );
        });
    }

    #[test]
    #[serial]
    fn add_first_claude_profile_and_config_writes_to_target_profile() {
        with_test_home(|state, home| {
            let default_dir = home.join(".claude");
            fs::create_dir_all(&default_dir).expect("create default claude dir");
            write_json_file(
                &default_dir.join("settings.json"),
                &json!({
                    "env": {
                        "ANTHROPIC_AUTH_TOKEN": "default-live-token",
                        "ANTHROPIC_BASE_URL": "https://default-live.example"
                    }
                }),
            )
            .expect("seed default live settings");

            let api_dir = home.join(".claude-profiles").join("api");
            let api_provider = claude_provider(
                "claude-api",
                "Claude API",
                "api-provider-token",
                "https://api-provider.example",
                Some(ProviderMeta {
                    claude_profile_dir: Some(api_dir.to_string_lossy().to_string()),
                    claude_activation_mode: Some(ClaudeActivationMode::ProfileAndConfig),
                    ..Default::default()
                }),
            );

            ProviderService::add(state, AppType::Claude, api_provider, true)
                .expect("add first profile-and-config provider");

            assert_eq!(
                crate::settings::get_effective_current_provider(&state.db, &AppType::Claude)
                    .expect("effective current provider")
                    .as_deref(),
                Some("claude-api"),
                "first added provider should become current"
            );
            assert_eq!(
                crate::settings::get_claude_override_dir().as_deref(),
                Some(api_dir.as_path()),
                "profile-and-config add should set the selected profile dir"
            );

            let api_live: Value =
                read_json_file(&api_dir.join("settings.json")).expect("read api profile");
            assert_eq!(
                api_live["env"]["ANTHROPIC_AUTH_TOKEN"],
                Value::String("api-provider-token".to_string()),
                "profile-and-config add should write provider settings into the target profile"
            );

            let default_live: Value =
                read_json_file(&default_dir.join("settings.json")).expect("read default profile");
            assert_eq!(
                default_live["env"]["ANTHROPIC_AUTH_TOKEN"],
                Value::String("default-live-token".to_string()),
                "profile-and-config add should not mutate the default live settings"
            );
        });
    }

    #[test]
    #[serial]
    fn update_current_claude_profile_only_applies_profile_without_overwriting_live() {
        with_test_home(|state, home| {
            let default_dir = home.join(".claude");
            fs::create_dir_all(&default_dir).expect("create default claude dir");
            write_json_file(
                &default_dir.join("settings.json"),
                &json!({
                    "env": {
                        "ANTHROPIC_AUTH_TOKEN": "default-live-token",
                        "ANTHROPIC_BASE_URL": "https://default-live.example"
                    }
                }),
            )
            .expect("seed default live settings");

            let official_dir = home.join(".claude-profiles").join("official");
            fs::create_dir_all(&official_dir).expect("create official profile dir");
            write_json_file(
                &official_dir.join("settings.json"),
                &json!({
                    "env": {
                        "ANTHROPIC_AUTH_TOKEN": "official-live-token",
                        "ANTHROPIC_BASE_URL": "https://official-live.example"
                    }
                }),
            )
            .expect("seed official profile settings");

            let legacy_provider = claude_provider(
                "claude-current",
                "Claude Current",
                "legacy-token",
                "https://legacy.example",
                None,
            );
            state
                .db
                .save_provider(AppType::Claude.as_str(), &legacy_provider)
                .expect("save legacy provider");
            state
                .db
                .set_current_provider(AppType::Claude.as_str(), "claude-current")
                .expect("set current provider");
            crate::settings::set_current_provider(&AppType::Claude, Some("claude-current"))
                .expect("set local current provider");

            let updated_provider = claude_provider(
                "claude-current",
                "Claude Current",
                "provider-token-should-not-write",
                "https://provider-should-not-write.example",
                Some(ProviderMeta {
                    claude_profile_dir: Some(official_dir.to_string_lossy().to_string()),
                    claude_activation_mode: Some(ClaudeActivationMode::ProfileOnly),
                    ..Default::default()
                }),
            );

            ProviderService::update(state, AppType::Claude, None, updated_provider)
                .expect("update current provider to profile-only");

            assert_eq!(
                crate::settings::get_claude_override_dir().as_deref(),
                Some(official_dir.as_path()),
                "updating the current Claude provider should apply the selected profile dir"
            );
            let default_live: Value =
                read_json_file(&default_dir.join("settings.json")).expect("read default profile");
            assert_eq!(
                default_live["env"]["ANTHROPIC_AUTH_TOKEN"],
                Value::String("default-live-token".to_string()),
                "profile-only current-provider update should not overwrite default live settings"
            );
        });
    }

    #[test]
    #[serial]
    fn update_current_claude_profile_only_rejects_missing_dir_before_saving() {
        with_test_home(|state, home| {
            let legacy_provider = claude_provider(
                "claude-current",
                "Claude Current",
                "legacy-token",
                "https://legacy.example",
                None,
            );
            state
                .db
                .save_provider(AppType::Claude.as_str(), &legacy_provider)
                .expect("save legacy provider");
            state
                .db
                .set_current_provider(AppType::Claude.as_str(), "claude-current")
                .expect("set current provider");
            crate::settings::set_current_provider(&AppType::Claude, Some("claude-current"))
                .expect("set local current provider");

            let updated_provider = claude_provider(
                "claude-current",
                "Claude Current",
                "provider-token-should-not-save",
                "https://provider-should-not-save.example",
                Some(ProviderMeta {
                    claude_profile_dir: Some(
                        home.join(".claude-profiles")
                            .join("missing-official")
                            .to_string_lossy()
                            .to_string(),
                    ),
                    claude_activation_mode: Some(ClaudeActivationMode::ProfileOnly),
                    ..Default::default()
                }),
            );

            let err = ProviderService::update(state, AppType::Claude, None, updated_provider)
                .expect_err("missing profile-only dir should reject update");
            assert!(
                err.to_string().contains("profile"),
                "expected profile validation error, got {err:?}"
            );

            let stored = state
                .db
                .get_provider_by_id("claude-current", AppType::Claude.as_str())
                .expect("query stored provider")
                .expect("stored provider exists");
            assert!(
                stored
                    .meta
                    .as_ref()
                    .and_then(|meta| meta.claude_activation_mode.as_ref())
                    .is_none(),
                "failed current-provider update must not persist invalid profile activation"
            );
            assert_eq!(
                stored.settings_config["env"]["ANTHROPIC_AUTH_TOKEN"],
                Value::String("legacy-token".to_string()),
                "failed current-provider update must leave previous settings in DB"
            );
        });
    }

    #[test]
    #[serial]
    fn terminal_launch_profile_and_config_syncs_selected_provider_without_switching_current() {
        with_test_home(|state, home| {
            let api_dir = home.join(".claude-profiles").join("api");
            fs::create_dir_all(&api_dir).expect("create api profile dir");
            write_json_file(
                &api_dir.join("settings.json"),
                &json!({
                    "env": {
                        "ANTHROPIC_AUTH_TOKEN": "stale-token",
                        "ANTHROPIC_BASE_URL": "https://stale.example"
                    }
                }),
            )
            .expect("seed stale api profile settings");

            let default_provider = claude_provider(
                "default",
                "Default",
                "default-provider-token",
                "https://default-provider.example",
                None,
            );
            let api_provider = claude_provider(
                "claude-api",
                "Claude API",
                "api-provider-token",
                "https://api-provider.example",
                Some(ProviderMeta {
                    claude_profile_dir: Some(api_dir.to_string_lossy().to_string()),
                    claude_activation_mode: Some(ClaudeActivationMode::ProfileAndConfig),
                    ..Default::default()
                }),
            );

            state
                .db
                .save_provider(AppType::Claude.as_str(), &default_provider)
                .expect("save default provider");
            state
                .db
                .save_provider(AppType::Claude.as_str(), &api_provider)
                .expect("save api provider");
            state
                .db
                .set_current_provider(AppType::Claude.as_str(), "default")
                .expect("set current provider");
            crate::settings::set_current_provider(&AppType::Claude, Some("default"))
                .expect("set local current provider");

            ProviderService::prepare_claude_profile_terminal_launch(state, &api_provider)
                .expect("prepare profile launch");

            let api_live: Value =
                read_json_file(&api_dir.join("settings.json")).expect("read api profile");
            assert_eq!(
                api_live["env"]["ANTHROPIC_AUTH_TOKEN"],
                Value::String("api-provider-token".to_string()),
                "terminal launch should write the selected provider settings into the target profile"
            );
            assert_eq!(
                crate::settings::get_effective_current_provider(&state.db, &AppType::Claude)
                    .expect("effective current provider")
                    .as_deref(),
                Some("default"),
                "preparing a terminal launch must not switch the current provider"
            );
            assert!(
                crate::settings::get_claude_override_dir().is_none(),
                "preparing a terminal launch must not persist a global Claude override"
            );
        });
    }

    #[test]
    #[serial]
    fn switch_claude_profile_mode_rolls_back_current_provider_and_override_on_failure() {
        with_test_home(|state, home| {
            let default_dir = home.join(".claude");
            fs::create_dir_all(&default_dir).expect("create default claude dir");
            write_json_file(
                &default_dir.join("settings.json"),
                &json!({
                    "env": {
                        "ANTHROPIC_AUTH_TOKEN": "default-live-token",
                        "ANTHROPIC_BASE_URL": "https://default-live.example"
                    }
                }),
            )
            .expect("seed default live settings");

            let default_provider = claude_provider(
                "default",
                "Default",
                "default-provider-token",
                "https://default-provider.example",
                None,
            );
            let broken_provider = claude_provider(
                "claude-official",
                "Claude Official",
                "provider-token-should-not-write",
                "https://provider-should-not-write.example",
                Some(ProviderMeta {
                    claude_profile_dir: Some(
                        home.join(".claude-profiles")
                            .join("missing-official")
                            .to_string_lossy()
                            .to_string(),
                    ),
                    claude_activation_mode: Some(ClaudeActivationMode::ProfileOnly),
                    ..Default::default()
                }),
            );

            state
                .db
                .save_provider(AppType::Claude.as_str(), &default_provider)
                .expect("save default provider");
            state
                .db
                .save_provider(AppType::Claude.as_str(), &broken_provider)
                .expect("save broken provider");
            state
                .db
                .set_current_provider(AppType::Claude.as_str(), "default")
                .expect("set current provider");
            crate::settings::set_current_provider(&AppType::Claude, Some("default"))
                .expect("set local current provider");

            let err = ProviderService::switch(state, AppType::Claude, "claude-official")
                .expect_err("switch should fail when profile-only dir is missing");

            assert!(
                err.to_string().contains("profile"),
                "expected missing profile error, got {err:?}"
            );
            assert_eq!(
                crate::settings::get_current_provider(&AppType::Claude).as_deref(),
                Some("default"),
                "failed switch should restore local current provider"
            );
            assert_eq!(
                crate::settings::get_effective_current_provider(&state.db, &AppType::Claude)
                    .expect("effective current provider")
                    .as_deref(),
                Some("default"),
                "failed switch should restore effective current provider"
            );
            assert!(
                crate::settings::get_claude_override_dir().is_none(),
                "failed switch should restore the previous claude override dir"
            );

            let live: Value = read_json_file(&get_claude_settings_path()).expect("read live");
            assert_eq!(
                live["env"]["ANTHROPIC_AUTH_TOKEN"],
                Value::String("default-live-token".to_string()),
                "failed switch should leave default live settings untouched"
            );
        });
    }

    #[test]
    #[serial]
    fn rollback_claude_profile_and_config_restores_target_profile_settings() {
        with_test_home(|state, home| {
            let default_dir = home.join(".claude");
            fs::create_dir_all(&default_dir).expect("create default claude dir");
            write_json_file(
                &default_dir.join("settings.json"),
                &json!({
                    "env": {
                        "ANTHROPIC_AUTH_TOKEN": "default-live-token",
                        "ANTHROPIC_BASE_URL": "https://default-live.example"
                    }
                }),
            )
            .expect("seed default live settings");

            let api_dir = home.join(".claude-profiles").join("api");
            fs::create_dir_all(&api_dir).expect("create api profile dir");
            write_json_file(
                &api_dir.join("settings.json"),
                &json!({
                    "env": {
                        "ANTHROPIC_AUTH_TOKEN": "api-original-token",
                        "ANTHROPIC_BASE_URL": "https://api-original.example"
                    }
                }),
            )
            .expect("seed api profile settings");

            let api_provider = claude_provider(
                "claude-api",
                "Claude API",
                "api-provider-token",
                "https://api-provider.example",
                Some(ProviderMeta {
                    claude_profile_dir: Some(api_dir.to_string_lossy().to_string()),
                    claude_activation_mode: Some(ClaudeActivationMode::ProfileAndConfig),
                    ..Default::default()
                }),
            );
            let plan = ProviderService::claude_switch_plan(&api_provider);
            let rollback = ProviderService::capture_claude_rollback_state(state, Some(&plan))
                .expect("capture rollback");

            ProviderService::apply_claude_switch_plan(&plan).expect("apply profile plan");
            write_json_file(
                &api_dir.join("settings.json"),
                &json!({
                    "env": {
                        "ANTHROPIC_AUTH_TOKEN": "failed-switch-token",
                        "ANTHROPIC_BASE_URL": "https://failed-switch.example"
                    }
                }),
            )
            .expect("simulate failed switch writing target profile");

            ProviderService::rollback_claude_switch(state, &rollback).expect("rollback switch");

            let api_live: Value =
                read_json_file(&api_dir.join("settings.json")).expect("read api profile");
            assert_eq!(
                api_live["env"]["ANTHROPIC_AUTH_TOKEN"],
                Value::String("api-original-token".to_string()),
                "rollback should restore the target profile file changed by profile-and-config"
            );
            let default_live: Value =
                read_json_file(&default_dir.join("settings.json")).expect("read default profile");
            assert_eq!(
                default_live["env"]["ANTHROPIC_AUTH_TOKEN"],
                Value::String("default-live-token".to_string()),
                "rollback should preserve the previous default profile snapshot"
            );
        });
    }

    #[test]
    #[serial]
    fn rollback_claude_profile_and_config_removes_new_target_profile_settings() {
        with_test_home(|state, home| {
            let default_dir = home.join(".claude");
            fs::create_dir_all(&default_dir).expect("create default claude dir");
            write_json_file(
                &default_dir.join("settings.json"),
                &json!({
                    "env": {
                        "ANTHROPIC_AUTH_TOKEN": "default-live-token",
                        "ANTHROPIC_BASE_URL": "https://default-live.example"
                    }
                }),
            )
            .expect("seed default live settings");

            let api_dir = home.join(".claude-profiles").join("api");
            fs::create_dir_all(&api_dir).expect("create api profile dir");
            let api_settings_path = api_dir.join("settings.json");

            let api_provider = claude_provider(
                "claude-api",
                "Claude API",
                "api-provider-token",
                "https://api-provider.example",
                Some(ProviderMeta {
                    claude_profile_dir: Some(api_dir.to_string_lossy().to_string()),
                    claude_activation_mode: Some(ClaudeActivationMode::ProfileAndConfig),
                    ..Default::default()
                }),
            );
            let plan = ProviderService::claude_switch_plan(&api_provider);
            let rollback = ProviderService::capture_claude_rollback_state(state, Some(&plan))
                .expect("capture rollback");

            ProviderService::apply_claude_switch_plan(&plan).expect("apply profile plan");
            write_json_file(
                &api_settings_path,
                &json!({
                    "env": {
                        "ANTHROPIC_AUTH_TOKEN": "failed-switch-token",
                        "ANTHROPIC_BASE_URL": "https://failed-switch.example"
                    }
                }),
            )
            .expect("simulate failed switch creating target profile settings");

            ProviderService::rollback_claude_switch(state, &rollback).expect("rollback switch");

            assert!(
                !api_settings_path.exists(),
                "rollback should remove a target profile settings file created by a failed profile-and-config switch"
            );
            let default_live: Value =
                read_json_file(&default_dir.join("settings.json")).expect("read default profile");
            assert_eq!(
                default_live["env"]["ANTHROPIC_AUTH_TOKEN"],
                Value::String("default-live-token".to_string()),
                "rollback should preserve the previous default profile snapshot"
            );
        });
    }

    #[test]
    #[serial]
    fn rollback_claude_legacy_removes_new_default_settings() {
        with_test_home(|state, home| {
            let profile_dir = home.join(".claude-profiles").join("official");
            fs::create_dir_all(&profile_dir).expect("create profile dir");
            write_json_file(
                &profile_dir.join("settings.json"),
                &json!({
                    "env": {
                        "ANTHROPIC_AUTH_TOKEN": "profile-token",
                        "ANTHROPIC_BASE_URL": "https://profile.example"
                    }
                }),
            )
            .expect("seed profile settings");
            crate::settings::set_claude_provider_override_dir(Some(&profile_dir.to_string_lossy()))
                .expect("set previous profile override");

            let default_settings_path = home.join(".claude").join("settings.json");
            let legacy_provider = claude_provider(
                "claude-legacy",
                "Claude Legacy",
                "legacy-token",
                "https://legacy.example",
                None,
            );
            let plan = ProviderService::claude_switch_plan(&legacy_provider);
            let rollback = ProviderService::capture_claude_rollback_state(state, Some(&plan))
                .expect("capture rollback");

            ProviderService::apply_claude_switch_plan(&plan).expect("apply legacy plan");
            write_json_file(
                &default_settings_path,
                &json!({
                    "env": {
                        "ANTHROPIC_AUTH_TOKEN": "failed-legacy-token",
                        "ANTHROPIC_BASE_URL": "https://failed-legacy.example"
                    }
                }),
            )
            .expect("simulate failed legacy switch creating default settings");

            ProviderService::rollback_claude_switch(state, &rollback).expect("rollback switch");

            assert!(
                !default_settings_path.exists(),
                "rollback should remove default Claude settings created by a failed legacy switch"
            );
            let profile_live: Value =
                read_json_file(&profile_dir.join("settings.json")).expect("read profile settings");
            assert_eq!(
                profile_live["env"]["ANTHROPIC_AUTH_TOKEN"],
                Value::String("profile-token".to_string()),
                "rollback should restore the previously active profile settings"
            );
            assert_eq!(
                crate::settings::get_claude_override_dir().as_deref(),
                Some(profile_dir.as_path()),
                "rollback should restore previous Claude profile override"
            );
        });
    }

    #[test]
    #[serial]
    fn rollback_claude_legacy_removes_new_configured_default_settings() {
        with_test_home(|state, home| {
            let previous_profile_dir = home.join(".claude-profiles").join("official");
            fs::create_dir_all(&previous_profile_dir).expect("create previous profile dir");
            write_json_file(
                &previous_profile_dir.join("settings.json"),
                &json!({
                    "env": {
                        "ANTHROPIC_AUTH_TOKEN": "profile-token",
                        "ANTHROPIC_BASE_URL": "https://profile.example"
                    }
                }),
            )
            .expect("seed previous profile settings");
            crate::settings::set_claude_provider_override_dir(Some(
                &previous_profile_dir.to_string_lossy(),
            ))
            .expect("set previous profile override");

            let configured_legacy_dir = home.join(".configured-claude");
            fs::create_dir_all(&configured_legacy_dir).expect("create configured legacy dir");
            let mut settings = crate::settings::get_settings();
            settings.claude_config_dir = Some(configured_legacy_dir.to_string_lossy().to_string());
            crate::settings::update_settings(settings).expect("set configured legacy dir");

            let configured_settings_path = configured_legacy_dir.join("settings.json");
            let legacy_provider = claude_provider(
                "claude-legacy",
                "Claude Legacy",
                "legacy-token",
                "https://legacy.example",
                None,
            );
            let plan = ProviderService::claude_switch_plan(&legacy_provider);
            let rollback = ProviderService::capture_claude_rollback_state(state, Some(&plan))
                .expect("capture rollback");

            ProviderService::apply_claude_switch_plan(&plan).expect("apply legacy plan");
            write_json_file(
                &configured_settings_path,
                &json!({
                    "env": {
                        "ANTHROPIC_AUTH_TOKEN": "failed-legacy-token",
                        "ANTHROPIC_BASE_URL": "https://failed-legacy.example"
                    }
                }),
            )
            .expect("simulate failed legacy switch creating configured settings");

            ProviderService::rollback_claude_switch(state, &rollback).expect("rollback switch");

            assert!(
                !configured_settings_path.exists(),
                "rollback should remove settings created in the configured legacy Claude dir"
            );
            assert_eq!(
                crate::settings::get_claude_override_dir().as_deref(),
                Some(previous_profile_dir.as_path()),
                "rollback should restore the previously active profile override"
            );
        });
    }

    #[test]
    #[serial]
    fn apply_claude_legacy_clears_stale_provider_profile_override() {
        with_test_home(|_state, home| {
            let stale_profile_dir = home.join(".claude-profiles").join("stale");
            fs::create_dir_all(&stale_profile_dir).expect("create stale profile dir");
            crate::settings::set_claude_provider_override_dir(Some(
                &stale_profile_dir.to_string_lossy(),
            ))
            .expect("seed stale profile override");

            let legacy_provider = claude_provider(
                "claude-legacy",
                "Claude Legacy",
                "legacy-token",
                "https://legacy.example",
                Some(ProviderMeta {
                    claude_profile_dir: Some(stale_profile_dir.to_string_lossy().to_string()),
                    claude_activation_mode: Some(ClaudeActivationMode::Legacy),
                    ..Default::default()
                }),
            );

            let plan = ProviderService::claude_switch_plan(&legacy_provider);
            ProviderService::apply_claude_switch_plan(&plan).expect("apply legacy plan");

            assert!(
                crate::settings::get_claude_override_dir().is_none(),
                "legacy activation should clear any stale provider profile override"
            );
        });
    }

    #[test]
    #[serial]
    fn apply_claude_legacy_preserves_configured_dir_env() {
        let _env_guard = UserEnvVarGuard::new("CLAUDE_CONFIG_DIR");
        with_test_home(|_state, home| {
            let stale_profile_dir = home.join(".claude-profiles").join("stale");
            fs::create_dir_all(&stale_profile_dir).expect("create stale profile dir");
            crate::settings::set_claude_provider_override_dir(Some(
                &stale_profile_dir.to_string_lossy(),
            ))
            .expect("seed stale profile override");

            let configured_legacy_dir = home.join(".configured-claude");
            fs::create_dir_all(&configured_legacy_dir).expect("create configured legacy dir");
            let configured_legacy_dir = configured_legacy_dir.to_string_lossy().to_string();
            let mut settings = crate::settings::get_settings();
            settings.claude_config_dir = Some(configured_legacy_dir.clone());
            crate::settings::update_settings(settings).expect("set configured legacy dir");

            let legacy_provider = claude_provider(
                "claude-legacy",
                "Claude Legacy",
                "legacy-token",
                "https://legacy.example",
                None,
            );

            let plan = ProviderService::claude_switch_plan(&legacy_provider);
            ProviderService::apply_claude_switch_plan(&plan).expect("apply legacy plan");

            assert_eq!(
                crate::settings::get_claude_override_dir().as_deref(),
                Some(Path::new(configured_legacy_dir.as_str())),
                "legacy activation should clear the stale provider profile override and reveal the configured legacy dir"
            );
            assert_eq!(
                crate::services::env_manager::get_user_env_var("CLAUDE_CONFIG_DIR")
                    .expect("read claude config env")
                    .as_deref(),
                Some(configured_legacy_dir.as_str()),
                "legacy activation should point Claude CLI at the configured legacy dir"
            );
        });
    }

    #[test]
    #[serial]
    fn startup_sync_rejects_missing_profile_only_directory() {
        with_test_home(|state, home| {
            let broken_provider = claude_provider(
                "claude-official",
                "Claude Official",
                "provider-token",
                "https://provider.example",
                Some(ProviderMeta {
                    claude_profile_dir: Some(
                        home.join(".claude-profiles")
                            .join("missing-official")
                            .to_string_lossy()
                            .to_string(),
                    ),
                    claude_activation_mode: Some(ClaudeActivationMode::ProfileOnly),
                    ..Default::default()
                }),
            );

            state
                .db
                .save_provider(AppType::Claude.as_str(), &broken_provider)
                .expect("save broken provider");
            state
                .db
                .set_current_provider(AppType::Claude.as_str(), "claude-official")
                .expect("set current provider");
            crate::settings::set_current_provider(&AppType::Claude, Some("claude-official"))
                .expect("set local current provider");

            let err = ProviderService::sync_current_claude_profile_env(state)
                .expect_err("startup sync should fail when profile-only dir is missing");

            assert!(
                err.to_string().contains("profile"),
                "expected missing profile error, got {err:?}"
            );
            assert!(
                crate::settings::get_claude_override_dir().is_none(),
                "failed startup sync should not mutate the Claude override dir"
            );
        });
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
    fn extract_codex_common_config_preserves_mcp_servers_base_url() {
        let config_toml = r#"model_provider = "azure"
model = "gpt-4"
disable_response_storage = true

[model_providers.azure]
name = "Azure OpenAI"
base_url = "https://azure.example/v1"
wire_api = "responses"

[mcp_servers.my_server]
base_url = "http://localhost:8080"
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
        assert!(
            extracted.contains("http://localhost:8080"),
            "should keep mcp_servers.* base_url"
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

        state
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
            Some("http://127.0.0.1:15721"),
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
    async fn update_current_claude_profile_only_skips_proxy_live_write_during_takeover() {
        let _home = TempHome::new();
        crate::settings::reload_settings().expect("reload settings");

        let profile_dir = crate::config::get_home_dir()
            .join(".claude-profiles")
            .join("official");
        fs::create_dir_all(&profile_dir).expect("create profile dir");
        write_json_file(
            &profile_dir.join("settings.json"),
            &json!({
                "env": {
                    "ANTHROPIC_AUTH_TOKEN": "official-live-token",
                    "ANTHROPIC_BASE_URL": "https://official-live.example"
                },
                "permissions": { "allow": ["Bash"] }
            }),
        )
        .expect("seed profile settings");

        let db = Arc::new(Database::memory().expect("init db"));
        let state = AppState::new(db.clone());
        let original = claude_provider("p1", "Claude A", "token-a", "https://api.a.example", None);
        db.save_provider("claude", &original)
            .expect("save provider");
        db.set_current_provider("claude", "p1")
            .expect("set current provider");
        crate::settings::set_current_provider(&AppType::Claude, Some("p1"))
            .expect("set local current provider");

        db.update_proxy_config(ProxyConfig {
            live_takeover_active: true,
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
                    "ANTHROPIC_API_KEY": "PROXY_MANAGED"
                }
            }),
        )
        .expect("seed taken-over live file");

        state
            .proxy_service
            .start()
            .await
            .expect("start proxy service");

        let updated = claude_provider(
            "p1",
            "Claude A",
            "provider-token-should-not-write",
            "https://provider-should-not-write.example",
            Some(ProviderMeta {
                claude_profile_dir: Some(profile_dir.to_string_lossy().to_string()),
                claude_activation_mode: Some(ClaudeActivationMode::ProfileOnly),
                ..Default::default()
            }),
        );

        ProviderService::update(&state, AppType::Claude, None, updated)
            .expect("update current provider to profile-only during takeover");

        let live: Value =
            read_json_file(&profile_dir.join("settings.json")).expect("read profile settings");
        assert_eq!(
            live["env"]["ANTHROPIC_AUTH_TOKEN"],
            Value::String("official-live-token".to_string()),
            "profile-only proxy sync should not overwrite the external profile auth"
        );
        assert_eq!(
            live["env"]["ANTHROPIC_BASE_URL"],
            Value::String("https://official-live.example".to_string()),
            "profile-only proxy sync should not overwrite the external profile base URL"
        );
    }

    #[tokio::test]
    #[serial]
    async fn switch_claude_profile_only_applies_profile_during_proxy_hot_switch() {
        let _env_guard = UserEnvVarGuard::new("CLAUDE_CONFIG_DIR");
        let _home = TempHome::new();
        crate::settings::reload_settings().expect("reload settings");

        let profile_dir = crate::config::get_home_dir()
            .join(".claude-profiles")
            .join("official");
        fs::create_dir_all(&profile_dir).expect("create profile dir");
        write_json_file(
            &profile_dir.join("settings.json"),
            &json!({
                "env": {
                    "ANTHROPIC_AUTH_TOKEN": "official-live-token",
                    "ANTHROPIC_BASE_URL": "https://official-live.example"
                }
            }),
        )
        .expect("seed profile settings");

        let db = Arc::new(Database::memory().expect("init db"));
        let state = AppState::new(db.clone());
        let legacy = claude_provider(
            "legacy",
            "Claude Legacy",
            "legacy-token",
            "https://legacy.example",
            None,
        );
        let profile_only = claude_provider(
            "profile-only",
            "Claude Profile",
            "provider-token-should-not-write",
            "https://provider-should-not-write.example",
            Some(ProviderMeta {
                claude_profile_dir: Some(profile_dir.to_string_lossy().to_string()),
                claude_activation_mode: Some(ClaudeActivationMode::ProfileOnly),
                ..Default::default()
            }),
        );
        db.save_provider("claude", &legacy)
            .expect("save legacy provider");
        db.save_provider("claude", &profile_only)
            .expect("save profile-only provider");
        db.set_current_provider("claude", "legacy")
            .expect("set current provider");
        crate::settings::set_current_provider(&AppType::Claude, Some("legacy"))
            .expect("set local current provider");

        db.save_live_backup(
            "claude",
            &serde_json::to_string(&legacy.settings_config).expect("serialize legacy provider"),
        )
        .await
        .expect("seed live backup");
        db.update_proxy_config(ProxyConfig {
            listen_port: 15731,
            ..Default::default()
        })
        .await
        .expect("set proxy test port");
        let default_live_path = get_claude_settings_path();
        write_json_file(
            &default_live_path,
            &json!({
                "env": {
                    "ANTHROPIC_BASE_URL": "http://127.0.0.1:15721",
                    "ANTHROPIC_API_KEY": "PROXY_MANAGED"
                }
            }),
        )
        .expect("seed taken-over live file");

        state
            .proxy_service
            .start()
            .await
            .expect("start proxy service");

        ProviderService::switch(&state, AppType::Claude, "profile-only")
            .expect("hot-switch to profile-only provider");

        assert_eq!(
            crate::settings::get_claude_override_dir().as_deref(),
            Some(profile_dir.as_path()),
            "profile-only hot-switch should apply the selected Claude profile"
        );
        assert_eq!(
            crate::services::env_manager::get_user_env_var("CLAUDE_CONFIG_DIR")
                .expect("read claude config dir")
                .as_deref(),
            Some(profile_dir.to_string_lossy().as_ref()),
            "profile-only hot-switch should update the persisted Claude env"
        );

        let profile_live: Value =
            read_json_file(&profile_dir.join("settings.json")).expect("read profile settings");
        assert_eq!(
            profile_live["env"]["ANTHROPIC_AUTH_TOKEN"],
            Value::String("official-live-token".to_string()),
            "profile-only hot-switch should not overwrite the selected profile file"
        );

        let default_live: Value =
            read_json_file(&default_live_path).expect("read default live settings");
        assert_eq!(
            default_live["env"]["ANTHROPIC_API_KEY"],
            Value::String("PROXY_MANAGED".to_string()),
            "profile-only hot-switch should not write provider credentials into proxy live config"
        );
    }

    #[tokio::test]
    #[serial]
    async fn switch_claude_profile_and_config_applies_profile_before_proxy_hot_switch() {
        let _env_guard = UserEnvVarGuard::new("CLAUDE_CONFIG_DIR");
        let _home = TempHome::new();
        crate::settings::reload_settings().expect("reload settings");

        let profile_dir = crate::config::get_home_dir()
            .join(".claude-profiles")
            .join("api");
        fs::create_dir_all(&profile_dir).expect("create profile dir");
        let default_live_path = get_claude_settings_path();
        write_json_file(
            &default_live_path,
            &json!({
                "env": {
                    "ANTHROPIC_BASE_URL": "http://127.0.0.1:15721",
                    "ANTHROPIC_API_KEY": "PROXY_MANAGED"
                }
            }),
        )
        .expect("seed default live file");

        let db = Arc::new(Database::memory().expect("init db"));
        let state = AppState::new(db.clone());
        let legacy = claude_provider(
            "legacy",
            "Claude Legacy",
            "legacy-token",
            "https://legacy.example",
            None,
        );
        let profile_and_config = claude_provider(
            "profile-and-config",
            "Claude API",
            "api-provider-token",
            "https://api-provider.example",
            Some(ProviderMeta {
                claude_profile_dir: Some(profile_dir.to_string_lossy().to_string()),
                claude_activation_mode: Some(ClaudeActivationMode::ProfileAndConfig),
                ..Default::default()
            }),
        );
        db.save_provider("claude", &legacy)
            .expect("save legacy provider");
        db.save_provider("claude", &profile_and_config)
            .expect("save profile-and-config provider");
        db.set_current_provider("claude", "legacy")
            .expect("set current provider");
        crate::settings::set_current_provider(&AppType::Claude, Some("legacy"))
            .expect("set local current provider");
        db.save_live_backup(
            "claude",
            &serde_json::to_string(&legacy.settings_config).expect("serialize legacy provider"),
        )
        .await
        .expect("seed live backup");
        db.update_proxy_config(ProxyConfig {
            listen_port: 15732,
            ..Default::default()
        })
        .await
        .expect("set proxy test port");

        state
            .proxy_service
            .start()
            .await
            .expect("start proxy service");

        ProviderService::switch(&state, AppType::Claude, "profile-and-config")
            .expect("hot-switch to profile-and-config provider");

        assert_eq!(
            crate::settings::get_claude_override_dir().as_deref(),
            Some(profile_dir.as_path()),
            "profile-and-config hot-switch should apply the selected Claude profile"
        );
        assert_eq!(
            crate::services::env_manager::get_user_env_var("CLAUDE_CONFIG_DIR")
                .expect("read claude config dir")
                .as_deref(),
            Some(profile_dir.to_string_lossy().as_ref()),
            "profile-and-config hot-switch should update the persisted Claude env"
        );

        let profile_live: Value =
            read_json_file(&profile_dir.join("settings.json")).expect("read profile settings");
        assert_eq!(
            profile_live["env"]["ANTHROPIC_AUTH_TOKEN"],
            Value::String("PROXY_MANAGED".to_string()),
            "profile-and-config hot-switch should write proxy-managed auth into the selected profile"
        );
        assert_eq!(
            profile_live["env"]["ANTHROPIC_BASE_URL"],
            Value::String("http://127.0.0.1:15732".to_string()),
            "profile-and-config hot-switch should keep the proxy endpoint in the selected profile"
        );

        let default_live: Value =
            read_json_file(&default_live_path).expect("read default live settings");
        assert_eq!(
            default_live["env"]["ANTHROPIC_API_KEY"],
            Value::String("PROXY_MANAGED".to_string()),
            "profile-and-config hot-switch should not write provider credentials into the previous default profile"
        );
    }

    #[tokio::test]
    #[serial]
    async fn switch_claude_profile_and_config_rolls_back_profile_when_proxy_hot_switch_fails() {
        let _env_guard = UserEnvVarGuard::new("CLAUDE_CONFIG_DIR");
        let _home = TempHome::new();
        crate::settings::reload_settings().expect("reload settings");

        let previous_profile_dir = crate::config::get_home_dir()
            .join(".claude-profiles")
            .join("previous");
        fs::create_dir_all(&previous_profile_dir).expect("create previous profile dir");
        crate::settings::set_claude_provider_override_dir(Some(
            &previous_profile_dir.to_string_lossy(),
        ))
        .expect("seed previous profile override");
        crate::services::env_manager::set_user_env_var(
            "CLAUDE_CONFIG_DIR",
            Some(&previous_profile_dir.to_string_lossy()),
        )
        .expect("seed previous claude env");

        let target_profile_dir = crate::config::get_home_dir()
            .join(".claude-profiles")
            .join("api");
        fs::create_dir_all(target_profile_dir.join("settings.json"))
            .expect("create blocking target settings directory");

        let default_live_path = crate::config::get_home_dir()
            .join(".claude")
            .join("settings.json");
        write_json_file(
            &default_live_path,
            &json!({
                "env": {
                    "ANTHROPIC_BASE_URL": "http://127.0.0.1:15721",
                    "ANTHROPIC_API_KEY": "PROXY_MANAGED"
                }
            }),
        )
        .expect("seed taken-over default live file");

        let db = Arc::new(Database::memory().expect("init db"));
        let state = AppState::new(db.clone());
        let legacy = claude_provider(
            "legacy",
            "Claude Legacy",
            "legacy-token",
            "https://legacy.example",
            None,
        );
        let profile_and_config = claude_provider(
            "profile-and-config",
            "Claude API",
            "api-provider-token",
            "https://api-provider.example",
            Some(ProviderMeta {
                claude_profile_dir: Some(target_profile_dir.to_string_lossy().to_string()),
                claude_activation_mode: Some(ClaudeActivationMode::ProfileAndConfig),
                ..Default::default()
            }),
        );
        db.save_provider("claude", &legacy)
            .expect("save legacy provider");
        db.save_provider("claude", &profile_and_config)
            .expect("save profile-and-config provider");
        db.set_current_provider("claude", "legacy")
            .expect("set db current provider");
        crate::settings::set_current_provider(&AppType::Claude, Some("legacy"))
            .expect("set local current provider");
        db.save_live_backup(
            "claude",
            &serde_json::to_string(&legacy.settings_config).expect("serialize legacy provider"),
        )
        .await
        .expect("seed live backup");
        db.update_proxy_config(ProxyConfig {
            listen_port: 15733,
            ..Default::default()
        })
        .await
        .expect("set proxy test port");

        state
            .proxy_service
            .start()
            .await
            .expect("start proxy service");

        let err = ProviderService::switch(&state, AppType::Claude, "profile-and-config")
            .expect_err("hot-switch should fail when target profile settings path is a directory");
        assert!(
            err.to_string().contains("热切换失败"),
            "expected hot-switch error, got {err:?}"
        );
        assert_eq!(
            crate::settings::get_claude_override_dir().as_deref(),
            Some(previous_profile_dir.as_path()),
            "failed hot-switch should restore the previous Claude profile override"
        );
        assert_eq!(
            crate::services::env_manager::get_user_env_var("CLAUDE_CONFIG_DIR")
                .expect("read claude config dir")
                .as_deref(),
            Some(previous_profile_dir.to_string_lossy().as_ref()),
            "failed hot-switch should restore the persisted Claude env"
        );
        assert_eq!(
            crate::settings::get_effective_current_provider(&state.db, &AppType::Claude)
                .expect("effective current provider")
                .as_deref(),
            Some("legacy"),
            "failed hot-switch should restore the logical current provider"
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
}

impl ProviderService {
    fn claude_switch_plan(provider: &Provider) -> ClaudeSwitchPlan {
        let activation_mode = provider
            .meta
            .as_ref()
            .and_then(|meta| meta.claude_activation_mode.clone())
            .unwrap_or(ClaudeActivationMode::Legacy);
        let override_dir = if matches!(activation_mode, ClaudeActivationMode::Legacy) {
            None
        } else {
            provider
                .meta
                .as_ref()
                .and_then(|meta| meta.claude_profile_dir.as_ref())
                .map(|value| value.trim())
                .filter(|value| !value.is_empty())
                .map(|value| value.to_string())
        };

        ClaudeSwitchPlan {
            activation_mode,
            override_dir,
        }
    }

    fn validate_claude_switch_plan(provider: &Provider) -> Result<(), AppError> {
        let plan = Self::claude_switch_plan(provider);
        if matches!(plan.activation_mode, ClaudeActivationMode::Legacy) {
            return Ok(());
        }

        let Some(raw_dir) = plan.override_dir.as_deref() else {
            return Err(AppError::Message(
                "Claude profile switching requires a profile directory".to_string(),
            ));
        };

        if !Path::new(raw_dir).is_absolute() {
            return Err(AppError::Message(
                "Claude profile directory must be an absolute path".to_string(),
            ));
        }

        Ok(())
    }

    fn claude_settings_path_for_dir(dir: &Path) -> PathBuf {
        let settings = dir.join("settings.json");
        if settings.exists() {
            return settings;
        }

        let legacy = dir.join("claude.json");
        if legacy.exists() {
            return legacy;
        }

        settings
    }

    fn claude_legacy_settings_path(settings: &crate::settings::AppSettings) -> PathBuf {
        let legacy_dir = settings
            .claude_config_dir
            .as_deref()
            .map(Self::resolve_claude_override_path)
            .unwrap_or_else(|| crate::config::get_home_dir().join(".claude"));
        Self::claude_settings_path_for_dir(&legacy_dir)
    }

    fn resolve_claude_override_path(raw: &str) -> PathBuf {
        if raw == "~" {
            return crate::config::get_home_dir();
        }
        if let Some(stripped) = raw.strip_prefix("~/").or_else(|| raw.strip_prefix("~\\")) {
            return crate::config::get_home_dir().join(stripped);
        }
        PathBuf::from(raw)
    }

    fn claude_live_settings_path_for_plan(plan: &ClaudeSwitchPlan) -> Option<PathBuf> {
        match plan.activation_mode {
            ClaudeActivationMode::Legacy => Some(Self::claude_legacy_settings_path(
                &crate::settings::get_settings(),
            )),
            ClaudeActivationMode::ProfileOnly => None,
            ClaudeActivationMode::ProfileAndConfig => plan
                .override_dir
                .as_deref()
                .map(Path::new)
                .map(Self::claude_settings_path_for_dir),
        }
    }

    fn capture_claude_rollback_state(
        state: &AppState,
        plan: Option<&ClaudeSwitchPlan>,
    ) -> Result<ClaudeRollbackState, AppError> {
        let settings = crate::settings::get_settings();
        let previous_live_settings = read_live_settings(AppType::Claude).ok();
        let target_live_path = plan.and_then(Self::claude_live_settings_path_for_plan);
        let target_live_settings = target_live_path
            .as_ref()
            .and_then(|path| crate::config::read_json_file(path).ok());

        Ok(ClaudeRollbackState {
            previous_provider_override_dir: settings.claude_provider_config_dir,
            previous_local_current: crate::settings::get_current_provider(&AppType::Claude),
            previous_db_current: state.db.get_current_provider(AppType::Claude.as_str())?,
            previous_live_settings,
            target_live_path,
            target_live_settings,
            previous_config_env: crate::services::env_manager::get_user_env_var(
                "CLAUDE_CONFIG_DIR",
            )
            .ok()
            .flatten(),
        })
    }

    fn apply_claude_switch_plan(plan: &ClaudeSwitchPlan) -> Result<(), AppError> {
        crate::settings::set_claude_provider_override_dir(plan.override_dir.as_deref())?;
        let settings = crate::settings::get_settings();
        let legacy_config_dir = settings.claude_config_dir.as_deref();
        crate::services::env_manager::set_user_env_var(
            "CLAUDE_CONFIG_DIR",
            match plan.activation_mode {
                ClaudeActivationMode::Legacy => legacy_config_dir,
                ClaudeActivationMode::ProfileOnly | ClaudeActivationMode::ProfileAndConfig => {
                    plan.override_dir.as_deref()
                }
            },
        )
        .map_err(AppError::Message)
    }

    fn validate_claude_runtime_switch_plan(plan: &ClaudeSwitchPlan) -> Result<(), AppError> {
        if matches!(plan.activation_mode, ClaudeActivationMode::ProfileOnly) {
            let profile_dir = plan.override_dir.as_deref().ok_or_else(|| {
                AppError::Message(
                    "Claude profile switching requires a profile directory".to_string(),
                )
            })?;
            if !PathBuf::from(profile_dir).exists() {
                return Err(AppError::Message(format!(
                    "Claude profile directory does not exist: {profile_dir}"
                )));
            }
        }

        Ok(())
    }

    pub(crate) fn prepare_claude_profile_terminal_launch(
        state: &AppState,
        provider: &Provider,
    ) -> Result<(), AppError> {
        Self::validate_claude_switch_plan(provider)?;

        let plan = Self::claude_switch_plan(provider);
        Self::validate_claude_runtime_switch_plan(&plan)?;

        if !matches!(plan.activation_mode, ClaudeActivationMode::ProfileAndConfig) {
            return Ok(());
        }

        let profile_dir = plan.override_dir.as_deref().ok_or_else(|| {
            AppError::Message("Claude profile switching requires a profile directory".to_string())
        })?;

        write_claude_profile_with_common_config(state.db.as_ref(), provider, Path::new(profile_dir))
    }

    fn rollback_claude_switch(
        state: &AppState,
        rollback: &ClaudeRollbackState,
    ) -> Result<(), AppError> {
        crate::settings::set_claude_provider_override_dir(
            rollback.previous_provider_override_dir.as_deref(),
        )?;
        crate::services::env_manager::set_user_env_var(
            "CLAUDE_CONFIG_DIR",
            rollback.previous_config_env.as_deref(),
        )
        .map_err(AppError::Message)?;
        crate::settings::set_current_provider(
            &AppType::Claude,
            rollback.previous_local_current.as_deref(),
        )?;

        match rollback.previous_db_current.as_deref() {
            Some(id) => state
                .db
                .set_current_provider(AppType::Claude.as_str(), id)?,
            None => state.db.clear_current_provider(AppType::Claude.as_str())?,
        }

        if let Some(previous_live_settings) = rollback.previous_live_settings.as_ref() {
            write_json_file(&get_claude_settings_path(), previous_live_settings)?;
        }
        match (
            rollback.target_live_path.as_ref(),
            rollback.target_live_settings.as_ref(),
        ) {
            (Some(target_live_path), Some(target_live_settings)) => {
                write_json_file(target_live_path, target_live_settings)?;
            }
            (Some(target_live_path), None) if target_live_path.exists() => {
                fs::remove_file(target_live_path)
                    .map_err(|err| AppError::io(target_live_path, err))?;
            }
            _ => {}
        }

        Ok(())
    }

    fn codex_desktop_provider_key(provider: &Provider) -> Result<String, AppError> {
        let config = provider
            .settings_config
            .get("config")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();

        if config.is_empty() {
            return Ok("openai".to_string());
        }

        let provider_key = crate::codex_config::extract_codex_model_provider(config)
            .unwrap_or_else(|| "openai".to_string());
        if provider_key.eq_ignore_ascii_case("openai") && config.contains("[model_providers.") {
            return Ok(CC_SWITCH_CODEX_MODEL_PROVIDER_ID.to_string());
        }

        Ok(provider_key)
    }

    fn codex_provider_where_clause(column: &str) -> String {
        format!("({column} = ?1 OR (?1 = 'openai' AND lower({column}) = 'openai'))")
    }

    fn codex_threads_have_model_provider(conn: &rusqlite::Connection) -> Result<bool, AppError> {
        let table_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'threads'",
            [],
            |row| row.get(0),
        )?;
        if table_count == 0 {
            return Ok(false);
        }

        let mut stmt = conn.prepare("PRAGMA table_info(threads)")?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            let column_name: String = row.get(1)?;
            if column_name == "model_provider" {
                return Ok(true);
            }
        }

        Ok(false)
    }

    fn codex_threads_have_rollout_path(conn: &rusqlite::Connection) -> Result<bool, AppError> {
        let mut stmt = conn.prepare("PRAGMA table_info(threads)")?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            let column_name: String = row.get(1)?;
            if column_name == "rollout_path" {
                return Ok(true);
            }
        }

        Ok(false)
    }

    fn sync_codex_rollout_session_meta(
        rollout_path: &Path,
        source_provider: Option<&str>,
        target_provider: &str,
    ) -> Result<bool, AppError> {
        let text = match std::fs::read_to_string(rollout_path) {
            Ok(text) => text,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(err) => return Err(AppError::io(rollout_path, err)),
        };

        let Some(first_newline) = text.find('\n') else {
            return Ok(false);
        };
        let first_line = &text[..first_newline];
        let rest = &text[first_newline..];
        let mut value: serde_json::Value = match serde_json::from_str(first_line) {
            Ok(value) => value,
            Err(_) => return Ok(false),
        };

        if value.get("type").and_then(Value::as_str) != Some("session_meta") {
            return Ok(false);
        }

        let Some(payload) = value.get_mut("payload").and_then(Value::as_object_mut) else {
            return Ok(false);
        };
        let current_provider = payload.get("model_provider").and_then(Value::as_str);
        if current_provider == Some(target_provider)
            || source_provider.is_some_and(|source| {
                current_provider.is_some_and(|current| {
                    current != source
                        && !(source == "openai" && current.eq_ignore_ascii_case("openai"))
                })
            })
        {
            return Ok(false);
        }

        payload.insert(
            "model_provider".to_string(),
            Value::String(target_provider.to_string()),
        );

        let updated_first_line = serde_json::to_string(&value).map_err(|err| {
            AppError::Message(format!(
                "Failed to serialize Codex rollout metadata {}: {err}",
                rollout_path.display()
            ))
        })?;
        crate::config::write_text_file(rollout_path, &format!("{updated_first_line}{rest}"))?;

        Ok(true)
    }

    fn sync_codex_desktop_threads(
        provider: &Provider,
        source_provider: Option<&str>,
    ) -> Result<usize, AppError> {
        let target_provider = Self::codex_desktop_provider_key(provider)?;
        let db_path = crate::codex_config::get_codex_config_dir().join("state_5.sqlite");
        if !db_path.exists() {
            return Ok(0);
        }

        let conn = rusqlite::Connection::open_with_flags(
            &db_path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE,
        )
        .map_err(|err| {
            AppError::Message(format!(
                "Failed to open Codex desktop state database {}: {err}",
                db_path.display()
            ))
        })?;

        if !Self::codex_threads_have_model_provider(&conn)? {
            return Ok(0);
        }

        let rollout_paths = if Self::codex_threads_have_rollout_path(&conn)? {
            if let Some(source_provider) = source_provider {
                let sql = format!(
                    "SELECT rollout_path FROM threads WHERE model_provider IS NULL OR {}",
                    Self::codex_provider_where_clause("model_provider")
                );
                let mut stmt = conn.prepare(&sql)?;
                let rows = stmt.query_map(rusqlite::params![source_provider], |row| row.get(0))?;
                rows.collect::<Result<Vec<String>, _>>()?
            } else {
                let mut stmt = conn.prepare(
                    "SELECT rollout_path FROM threads WHERE model_provider IS NULL OR model_provider <> ?1",
                )?;
                let rows = stmt.query_map(rusqlite::params![target_provider.as_str()], |row| {
                    row.get(0)
                })?;
                rows.collect::<Result<Vec<String>, _>>()?
            }
        } else {
            Vec::new()
        };

        let updated = if let Some(source_provider) = source_provider {
            let sql = format!(
                "UPDATE threads SET model_provider = ?2 WHERE model_provider IS NULL OR {}",
                Self::codex_provider_where_clause("model_provider")
            );
            conn.execute(
                &sql,
                rusqlite::params![source_provider, target_provider.as_str()],
            )?
        } else {
            conn.execute(
                "UPDATE threads SET model_provider = ?1 WHERE model_provider IS NULL OR model_provider <> ?1",
                rusqlite::params![target_provider.as_str()],
            )?
        };

        let mut rollout_updated = 0usize;
        for rollout_path in rollout_paths {
            match Self::sync_codex_rollout_session_meta(
                Path::new(&rollout_path),
                source_provider,
                &target_provider,
            ) {
                Ok(true) => rollout_updated += 1,
                Ok(false) => {}
                Err(err) => log::warn!(
                    "Codex rollout session metadata sync failed for {}: {err}",
                    rollout_path
                ),
            }
        }
        if rollout_updated > 0 {
            log::info!("Codex rollout session metadata sync updated {rollout_updated} files");
        }

        Ok(updated)
    }

    fn sync_codex_desktop_threads_after_switch(
        provider: &Provider,
        source_provider: Option<&str>,
        result: &mut SwitchResult,
    ) {
        match Self::sync_codex_desktop_threads(provider, source_provider) {
            Ok(mut updated) => {
                if source_provider.is_some() {
                    std::thread::sleep(std::time::Duration::from_millis(100));
                    match Self::sync_codex_desktop_threads(provider, source_provider) {
                        Ok(settled) => updated += settled,
                        Err(err) => log::warn!(
                            "Codex desktop thread model_provider settle sync failed: {err}"
                        ),
                    }
                }
                log::info!("Codex desktop thread model_provider sync updated {updated} rows");
            }
            Err(err) => {
                log::warn!("Codex desktop thread model_provider sync failed: {err}");
                result
                    .warnings
                    .push("codex_thread_provider_sync_failed".to_string());
            }
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

    /// List all providers for an app type
    pub fn list(
        state: &AppState,
        app_type: AppType,
    ) -> Result<IndexMap<String, Provider>, AppError> {
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
        let mut provider = provider;
        // Normalize Claude model keys
        Self::normalize_provider_if_claude(&app_type, &mut provider);
        Self::validate_provider_settings(&app_type, &provider)?;
        normalize_provider_common_config_for_storage(state.db.as_ref(), &app_type, &mut provider)?;
        if app_type.is_additive_mode() {
            Self::set_provider_live_config_managed(&mut provider, add_to_live);
        }

        let current = if app_type.is_additive_mode() {
            None
        } else {
            state.db.get_current_provider(app_type.as_str())?
        };
        if current.is_none() && matches!(app_type, AppType::Claude) {
            let plan = Self::claude_switch_plan(&provider);
            Self::validate_claude_runtime_switch_plan(&plan)?;
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
            write_live_with_common_config(state.db.as_ref(), &app_type, &provider)?;
            return Ok(true);
        }

        // For other apps: Check if sync is needed (if this is current provider, or no current provider)
        if current.is_none() {
            if matches!(app_type, AppType::Claude) {
                let plan = Self::claude_switch_plan(&provider);
                let rollback = Self::capture_claude_rollback_state(state, Some(&plan))?;

                let activate_result = (|| -> Result<(), AppError> {
                    Self::validate_claude_runtime_switch_plan(&plan)?;
                    Self::apply_claude_switch_plan(&plan)?;
                    crate::settings::set_current_provider(&app_type, Some(provider.id.as_str()))?;
                    state
                        .db
                        .set_current_provider(app_type.as_str(), &provider.id)?;

                    if !matches!(plan.activation_mode, ClaudeActivationMode::ProfileOnly) {
                        write_live_with_common_config(state.db.as_ref(), &app_type, &provider)?;
                    }

                    Ok(())
                })();

                if let Err(err) = activate_result {
                    if let Err(rollback_err) = Self::rollback_claude_switch(state, &rollback) {
                        return Err(AppError::Message(format!(
                            "{err}; additionally failed to roll back Claude switch state: {rollback_err}"
                        )));
                    }
                    return Err(err);
                }

                return Ok(true);
            }
            // No current provider, set as current and sync
            state
                .db
                .set_current_provider(app_type.as_str(), &provider.id)?;
            write_live_with_common_config(state.db.as_ref(), &app_type, &provider)?;
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
        let mut provider = provider;
        let original_id = original_id.unwrap_or(provider.id.as_str()).to_string();
        let provider_id_changed = original_id != provider.id;
        let existing_provider = state
            .db
            .get_provider_by_id(&original_id, app_type.as_str())?;
        // Normalize Claude model keys
        Self::normalize_provider_if_claude(&app_type, &mut provider);
        Self::validate_provider_settings(&app_type, &provider)?;
        normalize_provider_common_config_for_storage(state.db.as_ref(), &app_type, &mut provider)?;

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
            write_live_with_common_config(state.db.as_ref(), &app_type, &provider)?;
            return Ok(true);
        }

        // For other apps: Check if this is current provider (use effective current, not just DB)
        let effective_current =
            crate::settings::get_effective_current_provider(&state.db, &app_type)?;
        let is_current = effective_current.as_deref() == Some(provider.id.as_str());
        if is_current && matches!(app_type, AppType::Claude) {
            let plan = Self::claude_switch_plan(&provider);
            Self::validate_claude_runtime_switch_plan(&plan)?;
        }

        // Save to database only after current-provider runtime validation succeeds.
        state.db.save_provider(app_type.as_str(), &provider)?;

        if is_current {
            let claude_switch_plan =
                matches!(app_type, AppType::Claude).then(|| Self::claude_switch_plan(&provider));
            let claude_rollback = if matches!(app_type, AppType::Claude) {
                Some(Self::capture_claude_rollback_state(
                    state,
                    claude_switch_plan.as_ref(),
                )?)
            } else {
                None
            };

            // 如果 Claude 代理接管处于激活状态，并且代理服务正在运行：
            // - 不直接走普通 Live 写入逻辑
            // - 改为更新 Live 备份，并在 Claude 下同步代理安全的 Live 配置
            let has_live_backup =
                futures::executor::block_on(state.db.get_live_backup(app_type.as_str()))
                    .ok()
                    .flatten()
                    .is_some();
            let is_proxy_running = futures::executor::block_on(state.proxy_service.is_running());
            let live_taken_over = state
                .proxy_service
                .detect_takeover_in_live_config_for_app(&app_type);
            let should_sync_via_proxy = is_proxy_running && (has_live_backup || live_taken_over);

            let update_result = (|| -> Result<(), AppError> {
                if let Some(plan) = claude_switch_plan.as_ref() {
                    Self::validate_claude_runtime_switch_plan(plan)?;
                    Self::apply_claude_switch_plan(plan)?;
                }

                if should_sync_via_proxy {
                    futures::executor::block_on(
                        state
                            .proxy_service
                            .update_live_backup_from_provider(app_type.as_str(), &provider),
                    )
                    .map_err(|e| AppError::Message(format!("更新 Live 备份失败: {e}")))?;

                    let is_claude_profile_only = matches!(
                        claude_switch_plan
                            .as_ref()
                            .map(|plan| &plan.activation_mode),
                        Some(ClaudeActivationMode::ProfileOnly)
                    );
                    if matches!(app_type, AppType::Claude) && !is_claude_profile_only {
                        futures::executor::block_on(
                            state
                                .proxy_service
                                .sync_claude_live_from_provider_while_proxy_active(&provider),
                        )
                        .map_err(|e| {
                            AppError::Message(format!("同步 Claude Live 配置失败: {e}"))
                        })?;
                    }
                } else if !matches!(
                    claude_switch_plan
                        .as_ref()
                        .map(|plan| &plan.activation_mode),
                    Some(ClaudeActivationMode::ProfileOnly)
                ) {
                    write_live_with_common_config(state.db.as_ref(), &app_type, &provider)?;
                    // Sync MCP
                    McpService::sync_all_enabled(state)?;
                }

                Ok(())
            })();

            if let Err(err) = update_result {
                if let Some(rollback) = claude_rollback.as_ref() {
                    if let Err(rollback_err) = Self::rollback_claude_switch(state, rollback) {
                        return Err(AppError::Message(format!(
                            "{err}; additionally failed to roll back Claude switch state: {rollback_err}"
                        )));
                    }
                }
                return Err(err);
            }
        }

        Ok(true)
    }

    /// Delete a provider
    ///
    /// 同时检查本地 settings 和数据库的当前供应商，防止删除任一端正在使用的供应商。
    /// 对于累加模式应用（OpenCode, OpenClaw），可以随时删除任意供应商，同时从 live 配置中移除。
    pub fn delete(state: &AppState, app_type: AppType, id: &str) -> Result<(), AppError> {
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
    /// 3. If takeover mode active: hot-switch proxy target only (no Live config write)
    /// 4. If normal mode:
    ///    a. **Backfill mechanism**: Backfill current live config to current provider
    ///    b. Update local settings current_provider_xxx (device-level)
    ///    c. Update database is_current (as default for new devices)
    ///    d. Write target provider config to live files
    ///    e. Sync MCP configuration
    pub fn switch(state: &AppState, app_type: AppType, id: &str) -> Result<SwitchResult, AppError> {
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

        // Check if proxy takeover mode is active AND proxy server is actually running
        // Both conditions must be true to use hot-switch mode
        // Use blocking wait since this is a sync function
        let is_app_taken_over =
            futures::executor::block_on(state.db.get_live_backup(app_type.as_str()))
                .ok()
                .flatten()
                .is_some();
        let is_proxy_running = futures::executor::block_on(state.proxy_service.is_running());
        let live_taken_over = state
            .proxy_service
            .detect_takeover_in_live_config_for_app(&app_type);

        // Hot-switch only when BOTH: this app is taken over AND proxy server is actually running
        let should_hot_switch = (is_app_taken_over || live_taken_over) && is_proxy_running;

        // Block switching to official providers when proxy takeover is active.
        // Using a proxy with official APIs (Anthropic/OpenAI/Google) may cause account bans.
        if should_hot_switch && _provider.category.as_deref() == Some("official") {
            return Err(AppError::localized(
                "switch.official_blocked_by_proxy",
                "代理接管模式下不能切换到官方供应商，使用代理访问官方 API 可能导致账号被封禁。请先关闭代理接管，或选择第三方供应商。",
                "Cannot switch to official provider while proxy takeover is active. Using proxy with official APIs may cause account bans.",
            ));
        }

        if should_hot_switch {
            let claude_switch_plan = if matches!(app_type, AppType::Claude) {
                Some(Self::claude_switch_plan(_provider))
            } else {
                None
            };
            let claude_rollback = if matches!(app_type, AppType::Claude) {
                Some(Self::capture_claude_rollback_state(
                    state,
                    claude_switch_plan.as_ref(),
                )?)
            } else {
                None
            };
            let codex_source_provider = if matches!(app_type, AppType::Codex) {
                crate::settings::get_effective_current_provider(&state.db, &app_type)?
                    .as_deref()
                    .and_then(|id| providers.get(id))
                    .and_then(|provider| Self::codex_desktop_provider_key(provider).ok())
            } else {
                None
            };

            // Proxy takeover mode: hot-switch only, don't write Live config
            log::info!(
                "代理接管模式：热切换 {} 的目标供应商为 {}",
                app_type.as_str(),
                id
            );

            let hot_switch_result = (|| -> Result<(), AppError> {
                if let Some(plan) = claude_switch_plan.as_ref() {
                    Self::validate_claude_runtime_switch_plan(plan)?;
                    Self::apply_claude_switch_plan(plan)?;
                }

                if !matches!(
                    claude_switch_plan
                        .as_ref()
                        .map(|plan| &plan.activation_mode),
                    Some(ClaudeActivationMode::ProfileOnly)
                ) {
                    futures::executor::block_on(
                        state
                            .proxy_service
                            .hot_switch_provider(app_type.as_str(), id),
                    )
                    .map_err(|e| AppError::Message(format!("热切换失败: {e}")))?;
                } else {
                    state.db.set_current_provider(app_type.as_str(), id)?;
                    crate::settings::set_current_provider(&app_type, Some(id))?;
                }

                Ok(())
            })();

            if let Err(err) = hot_switch_result {
                if let Some(rollback) = claude_rollback.as_ref() {
                    if let Err(rollback_err) = Self::rollback_claude_switch(state, rollback) {
                        return Err(AppError::Message(format!(
                            "{err}; additionally failed to roll back Claude switch state: {rollback_err}"
                        )));
                    }
                }
                return Err(err);
            }

            // Note: No Live config write, no MCP sync
            // The proxy server will route requests to the new provider via is_current
            let mut result = SwitchResult::default();
            if matches!(app_type, AppType::Codex) {
                Self::sync_codex_desktop_threads_after_switch(
                    _provider,
                    codex_source_provider.as_deref(),
                    &mut result,
                );
            }
            return Ok(result);
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
        let claude_switch_plan =
            matches!(app_type, AppType::Claude).then(|| Self::claude_switch_plan(provider));
        let claude_rollback = if matches!(app_type, AppType::Claude) {
            Some(Self::capture_claude_rollback_state(
                state,
                claude_switch_plan.as_ref(),
            )?)
        } else {
            None
        };

        // Backfill: Backfill current live config to current provider
        // Use effective current provider (validated existence) to ensure backfill targets valid provider
        let current_id = crate::settings::get_effective_current_provider(&state.db, &app_type)?;

        if let Some(current_id) = current_id.as_deref() {
            if current_id != id {
                // Additive mode apps - all providers coexist in the same file,
                // no backfill needed (backfill is for exclusive mode apps like Claude/Codex/Gemini)
                if !app_type.is_additive_mode() {
                    // Only backfill when switching to a different provider
                    if let Some(mut current_provider) = providers.get(current_id).cloned() {
                        let skip_backfill = matches!(
                            (
                                &app_type,
                                current_provider
                                    .meta
                                    .as_ref()
                                    .and_then(|meta| meta.claude_activation_mode.as_ref())
                            ),
                            (AppType::Claude, Some(ClaudeActivationMode::ProfileOnly))
                        );
                        if !skip_backfill {
                            if let Ok(live_config) = read_live_settings(app_type.clone()) {
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
                                }
                            }
                        }
                    }
                }
            }
        }

        let switch_result = (|| -> Result<(), AppError> {
            if let Some(plan) = claude_switch_plan.as_ref() {
                Self::validate_claude_runtime_switch_plan(plan)?;
                Self::apply_claude_switch_plan(plan)?;
            }

            // Additive mode apps skip setting is_current (no such concept)
            if !app_type.is_additive_mode() {
                // Update local settings (device-level, takes priority)
                crate::settings::set_current_provider(&app_type, Some(id))?;

                // Update database is_current (as default for new devices)
                state.db.set_current_provider(app_type.as_str(), id)?;
            }

            let should_write_live = !matches!(
                claude_switch_plan
                    .as_ref()
                    .map(|plan| &plan.activation_mode),
                Some(ClaudeActivationMode::ProfileOnly)
            );

            if should_write_live {
                // Sync to live (write_gemini_live handles security flag internally for Gemini)
                write_live_with_common_config(state.db.as_ref(), &app_type, provider)?;
            }

            // Hermes is additive, so "switching" doesn't overwrite a live config file
            // — we instead update the top-level `model:` section to point at this
            // provider's first declared model. Without this, clicking "switch" would
            // only shuffle entries in custom_providers[] while Hermes keeps using
            // whatever `model.provider` was set before.
            if matches!(app_type, AppType::Hermes) {
                if let Err(e) = crate::hermes_config::apply_switch_defaults(
                    &provider.id,
                    &provider.settings_config,
                ) {
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
            if app_type.is_additive_mode()
                && Self::provider_live_config_managed(provider) != Some(true)
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

            // Sync MCP
            McpService::sync_all_enabled(state)?;

            Ok(())
        })();

        if let Err(err) = switch_result {
            if let Some(rollback) = claude_rollback.as_ref() {
                if let Err(rollback_err) = Self::rollback_claude_switch(state, rollback) {
                    return Err(AppError::Message(format!(
                        "{err}; additionally failed to roll back Claude switch state: {rollback_err}"
                    )));
                }
            }
            return Err(err);
        }

        if matches!(app_type, AppType::Codex) {
            let source_provider = current_id
                .as_deref()
                .and_then(|id| providers.get(id))
                .and_then(|provider| Self::codex_desktop_provider_key(provider).ok());
            Self::sync_codex_desktop_threads_after_switch(
                provider,
                source_provider.as_deref(),
                &mut result,
            );
        }

        Ok(result)
    }

    /// Sync current provider to live configuration (re-export)
    pub fn sync_current_to_live(state: &AppState) -> Result<(), AppError> {
        Self::sync_current_claude_profile_env(state)?;
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

        let takeover_enabled =
            futures::executor::block_on(state.db.get_proxy_config_for_app(app_type.as_str()))
                .map(|config| config.enabled)
                .unwrap_or(false);

        let has_live_backup =
            futures::executor::block_on(state.db.get_live_backup(app_type.as_str()))
                .ok()
                .flatten()
                .is_some();

        let live_taken_over = state
            .proxy_service
            .detect_takeover_in_live_config_for_app(&app_type);

        if matches!(app_type, AppType::Claude) {
            let plan = Self::claude_switch_plan(provider);
            Self::validate_claude_runtime_switch_plan(&plan)?;
            Self::apply_claude_switch_plan(&plan)?;
        }

        if takeover_enabled && (has_live_backup || live_taken_over) {
            futures::executor::block_on(
                state
                    .proxy_service
                    .update_live_backup_from_provider(app_type.as_str(), provider),
            )
            .map_err(|e| AppError::Message(format!("更新 Live 备份失败: {e}")))?;
            return Ok(());
        }

        sync_current_provider_for_app_to_live(state, &app_type)
    }

    pub fn sync_current_claude_profile_env(state: &AppState) -> Result<(), AppError> {
        let current_id =
            match crate::settings::get_effective_current_provider(&state.db, &AppType::Claude)? {
                Some(id) => id,
                None => return Ok(()),
            };

        let providers = state.db.get_all_providers(AppType::Claude.as_str())?;
        let Some(provider) = providers.get(&current_id) else {
            return Ok(());
        };

        let plan = Self::claude_switch_plan(provider);
        Self::validate_claude_runtime_switch_plan(&plan)?;
        Self::apply_claude_switch_plan(&plan)
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
            AppType::OpenCode => Self::extract_opencode_common_config(&provider.settings_config),
            AppType::OpenClaw => Self::extract_openclaw_common_config(&provider.settings_config),
            AppType::Hermes => Ok(String::new()), // Hermes doesn't use common config snippets
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
            AppType::OpenCode => Self::extract_opencode_common_config(settings_config),
            AppType::OpenClaw => Self::extract_openclaw_common_config(settings_config),
            AppType::Hermes => Ok(String::new()), // Hermes doesn't use common config snippets
        }
    }

    /// Extract common config for Claude (JSON format)
    fn extract_claude_common_config(settings: &Value) -> Result<String, AppError> {
        let mut config = settings.clone();

        // Fields to exclude from common config
        const ENV_EXCLUDES: &[&str] = &[
            // Auth
            "ANTHROPIC_API_KEY",
            "ANTHROPIC_AUTH_TOKEN",
            // Models and Claude Code model-menu display names
            "ANTHROPIC_MODEL",
            "ANTHROPIC_REASONING_MODEL", // legacy: 已废弃，但旧配置可能残留
            "ANTHROPIC_DEFAULT_HAIKU_MODEL",
            "ANTHROPIC_DEFAULT_HAIKU_MODEL_NAME",
            "ANTHROPIC_DEFAULT_OPUS_MODEL",
            "ANTHROPIC_DEFAULT_OPUS_MODEL_NAME",
            "ANTHROPIC_DEFAULT_SONNET_MODEL",
            "ANTHROPIC_DEFAULT_SONNET_MODEL_NAME",
            // Endpoint
            "ANTHROPIC_BASE_URL",
        ];

        const TOP_LEVEL_EXCLUDES: &[&str] = &[
            "apiBaseUrl",
            // Legacy model fields
            "primaryModel",
            "smallFastModel",
        ];

        // Remove env fields
        if let Some(env) = config.get_mut("env").and_then(|v| v.as_object_mut()) {
            for key in ENV_EXCLUDES {
                env.remove(*key);
            }
            // If env is empty after removal, remove the env object itself
            if env.is_empty() {
                config.as_object_mut().map(|obj| obj.remove("env"));
            }
        }

        // Remove top-level fields
        if let Some(obj) = config.as_object_mut() {
            for key in TOP_LEVEL_EXCLUDES {
                obj.remove(*key);
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

        // Remove entire model_providers table (provider-specific configuration)
        root.remove("model_providers");

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
                if key == "GOOGLE_GEMINI_BASE_URL" || key == "GEMINI_API_KEY" {
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
                Self::validate_claude_switch_plan(provider)?;
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
            AppType::ClaudeDesktop => {
                let credentials =
                    crate::claude_desktop_config::direct_gateway_credentials(provider)?;
                Ok((credentials.api_key, credentials.base_url))
            }
            AppType::Codex => {
                let auth = provider
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

                let api_key = auth
                    .get("OPENAI_API_KEY")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        AppError::localized(
                            "provider.codex.api_key.missing",
                            "缺少 API Key",
                            "API key is missing",
                        )
                    })?
                    .to_string();

                let config_toml = provider
                    .settings_config
                    .get("config")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

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
            AppType::OpenClaw | AppType::Hermes => {
                // OpenClaw/Hermes use apiKey and baseUrl directly on the object
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

        // 同步到 Claude
        if let Some(mut claude_provider) = provider.to_claude_provider() {
            // 合并已有配置
            if let Some(existing) = state.db.get_provider_by_id(&claude_provider.id, "claude")? {
                let mut merged = existing.settings_config.clone();
                Self::merge_json(&mut merged, &claude_provider.settings_config);
                claude_provider.settings_config = merged;
            }
            state.db.save_provider("claude", &claude_provider)?;
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
        } else {
            let gemini_id = format!("universal-gemini-{id}");
            let _ = state.db.delete_provider("gemini", &gemini_id);
        }

        Ok(true)
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
