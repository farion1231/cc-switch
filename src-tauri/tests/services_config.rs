use std::path::PathBuf;

use cc_switch_lib::{AppType, ConfigService, MultiAppConfig, Provider, ProviderService};
use serde_json::json;

#[path = "support.rs"]
mod support;
use support::{create_test_state_with_config, ensure_test_home, reset_test_fs, test_mutex};

fn seed_backups(dir: &std::path::Path, count: usize) -> Vec<PathBuf> {
    std::fs::create_dir_all(dir).expect("create backup dir");
    let mut paths = Vec::new();
    for i in 0..count {
        let p = dir.join(format!("backup_{i:04}.json"));
        std::fs::write(&p, "{}").expect("write seed backup");
        paths.push(p);
    }
    paths
}

fn seed_config_file(dir: &std::path::Path, content: &str) -> PathBuf {
    std::fs::create_dir_all(dir).expect("create config dir");
    let path = dir.join("config.json");
    std::fs::write(&path, content).expect("write config file");
    path
}

#[test]
fn create_backup_returns_empty_when_file_missing() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();

    let missing = home.join(".cc-switch").join("no-such-file.json");
    let result = ConfigService::create_backup(&missing).expect("should not error");
    assert_eq!(result, "", "missing file -> empty backup ID");
}

#[test]
fn create_backup_creates_backup_file_and_returns_id() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();

    let cc_dir = home.join(".cc-switch");
    let config_path = seed_config_file(&cc_dir, r#"{"version":1}"#);

    let backup_id =
        ConfigService::create_backup(&config_path).expect("create_backup should succeed");
    assert!(!backup_id.is_empty(), "backup ID must not be empty");
    assert!(
        backup_id.starts_with("backup_"),
        "ID should have 'backup_' prefix"
    );

    let backup_file = cc_dir.join("backups").join(format!("{backup_id}.json"));
    assert!(
        backup_file.exists(),
        "backup file should be created at {}",
        backup_file.display()
    );
}

#[test]
fn create_backup_content_matches_source() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();

    let cc_dir = home.join(".cc-switch");
    let original = r#"{"providers":{"claude":{"current":"my-provider"}}}"#;
    let config_path = seed_config_file(&cc_dir, original);

    let backup_id = ConfigService::create_backup(&config_path).expect("create_backup");
    let backup_file = cc_dir.join("backups").join(format!("{backup_id}.json"));
    let backed_up = std::fs::read_to_string(&backup_file).expect("read backup file");

    assert_eq!(backed_up, original, "backup content must equal original");
}

#[test]
fn create_backup_handles_path_with_parent() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();

    let deep_dir = home.join(".cc-switch").join("deep").join("nested");
    let config_path = seed_config_file(&deep_dir, "{}");

    let backup_id = ConfigService::create_backup(&config_path).expect("deep path backup");
    assert!(!backup_id.is_empty());
}

#[test]
fn create_backup_rotates_oldest_backups_when_over_limit() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();

    let cc_dir = home.join(".cc-switch");
    let config_path = seed_config_file(&cc_dir, "{}");
    let backup_dir = cc_dir.join("backups");

    seed_backups(&backup_dir, 9);

    let id_10 = ConfigService::create_backup(&config_path).expect("10th backup");
    assert!(!id_10.is_empty());

    let count_after_10: usize = std::fs::read_dir(&backup_dir)
        .expect("read backup dir")
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map(|x| x == "json").unwrap_or(false))
        .count();
    assert!(
        count_after_10 <= 10,
        "should not exceed 10 backups after 10th creation, got {count_after_10}"
    );

    ConfigService::create_backup(&config_path).expect("11th backup");

    let count_after_11: usize = std::fs::read_dir(&backup_dir)
        .expect("read backup dir")
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map(|x| x == "json").unwrap_or(false))
        .count();
    assert!(
        count_after_11 <= 10,
        "rotation should keep at most 10 backups, got {count_after_11}"
    );
}

#[test]
fn sync_current_providers_to_live_empty_config_no_error() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    ensure_test_home();

    let mut config = MultiAppConfig::default();
    let result = ConfigService::sync_current_providers_to_live(&mut config);
    assert!(
        result.is_ok(),
        "empty config sync should not error: {:?}",
        result
    );
}

#[test]
fn sync_current_providers_to_live_skips_empty_current() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    ensure_test_home();

    let mut config = MultiAppConfig::default();
    {
        let manager = config
            .get_manager_mut(&AppType::Claude)
            .expect("claude manager");
        manager.current = String::new();
    }

    let result = ConfigService::sync_current_providers_to_live(&mut config);
    assert!(result.is_ok(), "should skip and return Ok: {result:?}");
}

#[test]
fn sync_current_providers_to_live_skips_missing_provider() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    ensure_test_home();

    let mut config = MultiAppConfig::default();
    {
        let manager = config
            .get_manager_mut(&AppType::Claude)
            .expect("claude manager");
        manager.current = "ghost-provider".to_string();
    }

    let result = ConfigService::sync_current_providers_to_live(&mut config);
    assert!(
        result.is_ok(),
        "missing current provider should be a warn, not error: {result:?}"
    );
}

#[test]
fn sync_current_providers_to_live_codex_missing_auth_returns_error() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    ensure_test_home();

    let mut config = MultiAppConfig::default();
    {
        let manager = config
            .get_manager_mut(&AppType::Codex)
            .expect("codex manager");
        manager.current = "broken".to_string();
        manager.providers.insert(
            "broken".to_string(),
            Provider::with_id(
                "broken".to_string(),
                "Broken".to_string(),
                json!({ "config": "" }),
                None,
            ),
        );
    }

    let result = ConfigService::sync_current_providers_to_live(&mut config);
    assert!(result.is_err(), "missing auth should cause error");
}

#[test]
fn create_backup_ids_are_unique_across_calls() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();

    let cc_dir = home.join(".cc-switch");
    let config_path = seed_config_file(&cc_dir, "{}");

    let id1 = ConfigService::create_backup(&config_path).expect("first backup");
    std::thread::sleep(std::time::Duration::from_millis(1100));
    let id2 = ConfigService::create_backup(&config_path).expect("second backup");

    assert!(id1.starts_with("backup_"), "id1 = {id1}");
    assert!(id2.starts_with("backup_"), "id2 = {id2}");
}

#[test]
fn extract_claude_common_config_removes_credentials() {
    let settings = json!({
        "env": {
            "ANTHROPIC_API_KEY": "sk-secret",
            "ANTHROPIC_AUTH_TOKEN": "tok-secret",
            "ANTHROPIC_BASE_URL": "https://custom.api",
            "ANTHROPIC_MODEL": "claude-opus-4",
            "CUSTOM_SETTING": "keep_this"
        }
    });

    let snippet =
        ProviderService::extract_common_config_snippet_from_settings(AppType::Claude, &settings)
            .expect("extract should succeed");

    assert!(!snippet.contains("sk-secret"), "API key must be stripped");
    assert!(
        !snippet.contains("tok-secret"),
        "auth token must be stripped"
    );
    assert!(
        !snippet.contains("ANTHROPIC_BASE_URL"),
        "base URL must be stripped"
    );
    assert!(
        snippet.contains("CUSTOM_SETTING"),
        "non-credential env vars must remain"
    );
}

#[test]
fn extract_codex_common_config_removes_provider_fields() {
    let toml_config = r#"model = "gpt-4"
model_provider = "openai"
base_url = "https://api.openai.com"

[model_providers.openai]
base_url = "https://api.openai.com/v1"

[mcp_servers.my_tool]
command = "mytool"
"#;

    let settings = json!({ "auth": {"OPENAI_API_KEY": "secret"}, "config": toml_config });

    let snippet =
        ProviderService::extract_common_config_snippet_from_settings(AppType::Codex, &settings)
            .expect("extract should succeed");

    assert!(
        !snippet.contains("gpt-4"),
        "model field should be removed from common config"
    );
    assert!(
        !snippet.contains("model_provider"),
        "model_provider should be removed"
    );
    assert!(
        !snippet.contains("[model_providers"),
        "model_providers table should be removed"
    );
    assert!(
        snippet.contains("my_tool"),
        "mcp_servers should be kept in common config"
    );
}

#[test]
fn extract_gemini_common_config_removes_credentials() {
    let settings = json!({
        "env": {
            "GEMINI_API_KEY": "api-secret",
            "GOOGLE_GEMINI_BASE_URL": "https://custom.gemini",
            "GEMINI_MODEL": "gemini-pro"
        }
    });

    let snippet =
        ProviderService::extract_common_config_snippet_from_settings(AppType::Gemini, &settings)
            .expect("extract should succeed");

    assert!(!snippet.contains("api-secret"), "API key must be stripped");
    assert!(
        !snippet.contains("GOOGLE_GEMINI_BASE_URL"),
        "base URL must be stripped"
    );
    assert!(
        snippet.contains("GEMINI_MODEL"),
        "non-credential env vars should be kept"
    );
}

#[test]
fn extract_claude_common_config_empty_settings_returns_empty_object() {
    let settings = json!({});

    let snippet =
        ProviderService::extract_common_config_snippet_from_settings(AppType::Claude, &settings)
            .expect("extract from empty should succeed");

    assert_eq!(snippet.trim(), "{}", "empty settings -> {{}}");
}

#[test]
fn extract_gemini_common_config_only_credentials_returns_empty_object() {
    let settings = json!({
        "env": {
            "GEMINI_API_KEY": "secret",
            "GOOGLE_GEMINI_BASE_URL": "https://base"
        }
    });

    let snippet =
        ProviderService::extract_common_config_snippet_from_settings(AppType::Gemini, &settings)
            .expect("extract");

    assert_eq!(snippet.trim(), "{}", "only credentials -> {{}}");
}

#[test]
fn validate_claude_provider_rejects_non_object() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    ensure_test_home();

    let state = create_test_state_with_config(&MultiAppConfig::default()).expect("create state");

    let bad_provider = Provider::with_id(
        "bad2".to_string(),
        "Bad2".to_string(),
        json!("still not an object"),
        None,
    );
    let result = ProviderService::add(&state, AppType::Claude, bad_provider, false);
    assert!(
        result.is_err(),
        "non-object Claude config should be rejected"
    );
}

#[test]
fn validate_codex_provider_rejects_missing_auth() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    ensure_test_home();

    let state = create_test_state_with_config(&MultiAppConfig::default()).expect("create state");

    let no_auth_provider = Provider::with_id(
        "codex-no-auth".to_string(),
        "NoAuth".to_string(),
        json!({ "config": "base_url = \"https://example.com\"" }),
        None,
    );
    let result = ProviderService::add(&state, AppType::Codex, no_auth_provider, false);
    assert!(
        result.is_err(),
        "Codex provider without auth should be rejected"
    );
    let err_str = result.unwrap_err().to_string();
    assert!(
        err_str.contains("auth"),
        "error should mention auth, got: {err_str}"
    );
}
