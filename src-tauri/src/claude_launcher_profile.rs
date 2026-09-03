use crate::app_config::AppType;
use crate::config::{atomic_write_private, get_app_config_dir};
use crate::error::AppError;
use crate::provider::Provider;
use crate::services::provider::{
    build_effective_settings_with_common_config, sanitize_claude_settings_for_live,
};
use crate::services::McpService;
use crate::store::AppState;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

pub(crate) const PROFILE_MARKER_FILENAME: &str = "cc-switch-profile.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ClaudeLauncherProfileMetadata {
    pub provider_id: String,
    pub provider_name: String,
    pub retired: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ClaudeLauncherProfile {
    pub config_dir: PathBuf,
    pub metadata: ClaudeLauncherProfileMetadata,
}

pub(crate) fn profiles_base_dir() -> PathBuf {
    get_app_config_dir().join("claude-profiles")
}

pub(crate) fn profile_dir(provider_id: &str) -> PathBuf {
    let digest = Sha256::digest(provider_id.as_bytes());
    profiles_base_dir().join(format!("{digest:x}"))
}
fn marker_path(config_dir: &Path) -> PathBuf {
    config_dir.join(PROFILE_MARKER_FILENAME)
}

fn write_private_json<T: Serialize>(path: &Path, value: &T) -> Result<(), AppError> {
    let bytes =
        serde_json::to_vec_pretty(value).map_err(|source| AppError::JsonSerialize { source })?;
    atomic_write_private(path, &bytes)
}

fn read_metadata(path: &Path) -> Result<ClaudeLauncherProfileMetadata, AppError> {
    let bytes = fs::read(path).map_err(|source| AppError::io(path, source))?;
    serde_json::from_slice(&bytes).map_err(|source| AppError::json(path, source))
}

fn metadata_matches_dir(metadata: &ClaudeLauncherProfileMetadata, config_dir: &Path) -> bool {
    !metadata.provider_id.trim().is_empty()
        && !metadata.provider_name.trim().is_empty()
        && profile_dir(&metadata.provider_id) == config_dir
}

fn scrub_profile_secrets(config_dir: &Path) -> Result<(), AppError> {
    let mut failures = Vec::new();
    for secret_path in [
        config_dir.join("settings.json"),
        config_dir.join(".claude.json"),
        config_dir.join(".credentials.json"),
    ] {
        match fs::remove_file(&secret_path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => failures.push(format!("{}: {error}", secret_path.display())),
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(AppError::Message(format!(
            "Claude 托管配置凭证清理失败: {}",
            failures.join("; ")
        )))
    }
}

pub(crate) fn sync_profile(state: &AppState, provider: &Provider) -> Result<PathBuf, AppError> {
    let config_dir = profile_dir(&provider.id);
    fs::create_dir_all(&config_dir).map_err(|source| AppError::io(&config_dir, source))?;
    let marker = marker_path(&config_dir);
    let mut metadata = ClaudeLauncherProfileMetadata {
        provider_id: provider.id.clone(),
        provider_name: provider.name.clone(),
        retired: true,
    };

    let result = (|| -> Result<(), AppError> {
        // Retire before any database-backed preparation. A refresh failure must
        // never leave an old active marker authorizing stale credentials.
        write_private_json(&marker, &metadata)?;
        let effective = build_effective_settings_with_common_config(
            state.db.as_ref(),
            &AppType::Claude,
            provider,
        )?;
        let settings = sanitize_claude_settings_for_live(&effective);
        let servers: HashMap<String, Value> = McpService::get_all_servers(state)?
            .into_iter()
            .filter(|(_, server)| server.apps.is_enabled_for(&AppType::Claude))
            .map(|(id, server)| (id, server.server))
            .collect();

        write_private_json(&config_dir.join("settings.json"), &settings)?;
        let mcp_path = config_dir.join(".claude.json");
        crate::claude_mcp::set_mcp_servers_map_at(&mcp_path, &servers)?;
        crate::claude_mcp::set_has_completed_onboarding_at(&mcp_path)?;
        metadata.retired = false;
        write_private_json(&marker, &metadata)
    })();

    if let Err(error) = result {
        return match scrub_profile_secrets(&config_dir) {
            Ok(()) => Err(error),
            Err(cleanup_error) => Err(AppError::Config(format!(
                "Claude 托管配置创建失败: {error}; {cleanup_error}"
            ))),
        };
    }
    Ok(config_dir)
}

pub(crate) fn refresh_profile_if_present(
    state: &AppState,
    provider_id: &str,
) -> Result<(), AppError> {
    let config_dir = profile_dir(provider_id);
    let marker = marker_path(&config_dir);
    if !marker.is_file() {
        return Ok(());
    }
    let metadata = read_metadata(&marker)?;
    if metadata.provider_id != provider_id || !metadata_matches_dir(&metadata, &config_dir) {
        return Err(AppError::Config(format!(
            "Claude 托管配置标记与目录不匹配: {}",
            marker.display()
        )));
    }
    if metadata.retired {
        return Ok(());
    }
    let provider = state
        .db
        .get_provider_by_id(provider_id, AppType::Claude.as_str())?
        .ok_or_else(|| {
            AppError::InvalidInput(format!(
                "Claude 供应商不存在，无法刷新隔离配置: {provider_id}"
            ))
        })?;
    sync_profile(state, &provider).map(|_| ())
}

/// Refresh every active managed Claude profile after shared Claude inputs change.
///
/// MCP edits do not pass through the Launch path, so profile projection must be
/// refreshed here as well. Retired profiles stay retired and secret-free.
pub(crate) fn refresh_active_profiles(state: &AppState) -> Result<(), AppError> {
    let mut failures = Vec::new();

    for profile in list_profiles()? {
        if profile.metadata.retired {
            continue;
        }
        let provider_id = profile.metadata.provider_id;
        let result = state
            .db
            .get_provider_by_id(&provider_id, AppType::Claude.as_str())?
            .ok_or_else(|| {
                AppError::InvalidInput(format!(
                    "Claude 供应商不存在，无法刷新隔离配置: {provider_id}"
                ))
            })
            .and_then(|provider| sync_profile(state, &provider).map(|_| ()));
        if let Err(error) = result {
            failures.push(format!("{provider_id}: {error}"));
        }
    }

    if failures.is_empty() {
        Ok(())
    } else {
        Err(AppError::Message(format!(
            "Claude 托管配置刷新失败: {}",
            failures.join("; ")
        )))
    }
}

pub(crate) fn list_profiles() -> Result<Vec<ClaudeLauncherProfile>, AppError> {
    let base_dir = profiles_base_dir();
    if !base_dir.exists() {
        return Ok(Vec::new());
    }

    let entries = fs::read_dir(&base_dir).map_err(|source| AppError::io(&base_dir, source))?;
    let mut profiles = Vec::new();
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                log::warn!("读取 Claude 托管配置目录条目失败（已跳过）: {error}");
                continue;
            }
        };
        let config_dir = entry.path();
        if !entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false) {
            continue;
        }
        let marker = marker_path(&config_dir);
        if !marker.is_file() {
            continue;
        }
        let metadata = match read_metadata(&marker) {
            Ok(metadata) if metadata_matches_dir(&metadata, &config_dir) => metadata,
            Ok(_) => {
                log::warn!(
                    "忽略目录或字段不匹配的 Claude 托管配置标记: {}",
                    marker.display()
                );
                continue;
            }
            Err(error) => {
                log::warn!(
                    "忽略无效的 Claude 托管配置标记 {}: {error}",
                    marker.display()
                );
                continue;
            }
        };
        profiles.push(ClaudeLauncherProfile {
            config_dir,
            metadata,
        });
    }
    profiles.sort_by(|left, right| left.config_dir.cmp(&right.config_dir));
    Ok(profiles)
}

/// Reconcile managed launch profiles after a database image is replaced.
///
/// Active profiles remain derived from the restored provider table: existing
/// providers are refreshed, while removed providers are retired and scrubbed.
/// Retired profiles stay retired until an explicit Launch calls `sync_profile`.
pub(crate) fn reconcile_profiles(state: &AppState) -> Result<(), AppError> {
    let providers = state.db.get_all_providers(AppType::Claude.as_str())?;
    let mut failures = Vec::new();

    for profile in list_profiles()? {
        if profile.metadata.retired {
            continue;
        }
        let provider_id = &profile.metadata.provider_id;
        let result = match providers.get(provider_id) {
            Some(provider) => sync_profile(state, provider).map(|_| ()),
            None => retire_profile(provider_id),
        };
        if let Err(error) = result {
            failures.push(format!("{provider_id}: {error}"));
        }
    }

    if failures.is_empty() {
        Ok(())
    } else {
        Err(AppError::Message(format!(
            "Claude 托管配置同步失败: {}",
            failures.join("; ")
        )))
    }
}

pub(crate) fn retire_profile(provider_id: &str) -> Result<(), AppError> {
    let config_dir = profile_dir(provider_id);
    let marker = marker_path(&config_dir);
    if !marker.is_file() {
        return Ok(());
    }

    let mut metadata = read_metadata(&marker)?;
    if metadata.provider_id != provider_id || !metadata_matches_dir(&metadata, &config_dir) {
        return Err(AppError::Config(format!(
            "Claude 托管配置标记与目录不匹配: {}",
            marker.display()
        )));
    }
    metadata.retired = true;
    write_private_json(&marker, &metadata)?;

    scrub_profile_secrets(&config_dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_config::{AppType, McpApps, McpServer};
    use crate::database::Database;
    use crate::provider::{Provider, ProviderMeta};
    use crate::store::AppState;
    use serde_json::{json, Value};
    use serial_test::serial;
    use std::ffi::OsString;
    use std::fs;
    use std::sync::Arc;

    struct TestHome {
        _temp: tempfile::TempDir,
        previous_home: Option<OsString>,
        previous_settings: crate::settings::AppSettings,
    }

    impl TestHome {
        fn new() -> Self {
            let previous_home = std::env::var_os("CC_SWITCH_TEST_HOME");
            let previous_settings = crate::settings::get_settings();
            let temp = tempfile::tempdir().expect("create test home");
            std::env::set_var("CC_SWITCH_TEST_HOME", temp.path());
            crate::settings::update_settings(crate::settings::AppSettings::default())
                .expect("reset isolated settings");
            Self {
                _temp: temp,
                previous_home,
                previous_settings,
            }
        }
    }

    impl Drop for TestHome {
        fn drop(&mut self) {
            crate::settings::update_settings(self.previous_settings.clone())
                .expect("restore settings");
            match &self.previous_home {
                Some(value) => std::env::set_var("CC_SWITCH_TEST_HOME", value),
                None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
            }
        }
    }

    fn provider(id: &str, name: &str, token: &str) -> Provider {
        let mut provider = Provider::with_id(
            id.to_string(),
            name.to_string(),
            json!({
                "env": {
                    "ANTHROPIC_AUTH_TOKEN": token,
                    "ANTHROPIC_BASE_URL": format!("https://{id}.example")
                },
                "api_format": "anthropic"
            }),
            None,
        );
        provider.meta = Some(ProviderMeta {
            common_config_enabled: Some(true),
            ..ProviderMeta::default()
        });
        provider
    }

    fn state_with_mcp() -> AppState {
        let db = Arc::new(Database::memory().expect("create memory database"));
        db.set_config_snippet(
            AppType::Claude.as_str(),
            Some(json!({"env": {"COMMON_FLAG": "shared"}}).to_string()),
        )
        .expect("save Claude common config");
        db.save_mcp_server(&McpServer {
            id: "claude-mcp".to_string(),
            name: "Claude MCP".to_string(),
            server: json!({
                "command": "custom-mcp",
                "env": {"MCP_TOKEN": "mcp-secret"}
            }),
            apps: McpApps {
                claude: true,
                ..McpApps::default()
            },
            description: None,
            homepage: None,
            docs: None,
            tags: Vec::new(),
        })
        .expect("save enabled Claude MCP");
        db.save_mcp_server(&McpServer {
            id: "codex-only".to_string(),
            name: "Codex MCP".to_string(),
            server: json!({"command": "must-not-project"}),
            apps: McpApps {
                codex: true,
                ..McpApps::default()
            },
            description: None,
            homepage: None,
            docs: None,
            tags: Vec::new(),
        })
        .expect("save disabled Claude MCP");
        AppState::new(db)
    }

    fn read_json(path: &std::path::Path) -> Value {
        serde_json::from_slice(&fs::read(path).expect("read JSON")).expect("parse JSON")
    }

    #[test]
    fn profile_dir_uses_full_lowercase_sha256_child() {
        let path = profile_dir("../provider/A");
        assert_eq!(
            path.parent(),
            Some(profiles_base_dir().as_path()),
            "provider IDs must never become path components"
        );
        assert_eq!(
            path.file_name().and_then(|name| name.to_str()),
            Some("fcf3863a31586806dcc985260a50438f25c6682437ebf8308b2fb4aa45579664")
        );
    }
    #[test]
    #[serial]
    fn sync_profile_projects_effective_settings_and_enabled_mcp_without_touching_global_files() {
        let _home = TestHome::new();
        let state = state_with_mcp();
        let provider = provider("provider-a", "Provider A", "secret-a");
        let global_settings = crate::config::get_claude_settings_path();
        let global_mcp = crate::config::get_claude_mcp_path();
        fs::create_dir_all(global_settings.parent().unwrap()).expect("create global settings dir");
        fs::write(&global_settings, b"global-settings-sentinel").expect("seed global settings");
        fs::write(&global_mcp, b"global-mcp-sentinel").expect("seed global MCP");

        let dir = sync_profile(&state, &provider).expect("sync profile");

        let settings = read_json(&dir.join("settings.json"));
        assert_eq!(settings["env"]["ANTHROPIC_AUTH_TOKEN"], "secret-a");
        assert_eq!(settings["env"]["COMMON_FLAG"], "shared");
        assert!(settings.get("api_format").is_none());
        let mcp = read_json(&dir.join(".claude.json"));
        assert_eq!(mcp["hasCompletedOnboarding"], true);
        assert_eq!(mcp["mcpServers"]["claude-mcp"]["command"], "custom-mcp");
        assert!(mcp["mcpServers"].get("codex-only").is_none());
        let marker = read_json(&dir.join(PROFILE_MARKER_FILENAME));
        assert_eq!(marker["providerId"], "provider-a");
        assert_eq!(marker["providerName"], "Provider A");
        assert_eq!(marker["retired"], false);
        assert_eq!(
            fs::read(&global_settings).unwrap(),
            b"global-settings-sentinel"
        );
        assert_eq!(fs::read(&global_mcp).unwrap(), b"global-mcp-sentinel");

        #[cfg(unix)]
        for path in [dir.join("settings.json"), dir.join(".claude.json")] {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }

    #[test]
    #[serial]
    fn failed_profile_creation_scrubs_partially_written_secrets() {
        let _home = TestHome::new();
        let state = state_with_mcp();
        let provider = provider("provider-a", "Provider A", "secret-a");
        let dir = profile_dir(&provider.id);
        fs::create_dir_all(dir.join(".claude.json"))
            .expect("block MCP projection after settings are written");

        assert!(sync_profile(&state, &provider).is_err());

        assert!(!dir.join("settings.json").exists());
        assert!(!dir.join(".claude.json").is_file());
        assert_eq!(
            read_json(&dir.join(PROFILE_MARKER_FILENAME))["retired"],
            true
        );
    }

    #[test]
    #[serial]
    fn profile_preparation_failure_retires_and_scrubs_active_profile() {
        let _home = TestHome::new();
        let state = state_with_mcp();
        let provider = provider("provider-a", "Provider A", "secret-a");
        let dir = sync_profile(&state, &provider).expect("create active profile");

        let db = state.db.clone();
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            let _guard = db.conn.lock().expect("lock database before poisoning");
            panic!("poison database mutex");
        }));
        assert!(state.db.conn.is_poisoned());

        assert!(sync_profile(&state, &provider).is_err());

        assert!(!dir.join("settings.json").exists());
        assert!(!dir.join(".claude.json").exists());
        assert_eq!(
            read_json(&dir.join(PROFILE_MARKER_FILENAME))["retired"],
            true
        );
    }

    #[test]
    #[serial]
    fn profiles_are_independent_and_refresh_is_scoped_to_one_provider() {
        let _home = TestHome::new();
        let state = state_with_mcp();
        let mut provider_a = provider("provider-a", "Provider A", "secret-a");
        let provider_b = provider("provider-b", "Provider B", "secret-b");
        let dir_a = sync_profile(&state, &provider_a).expect("sync A");
        let dir_b = sync_profile(&state, &provider_b).expect("sync B");
        let b_before = [
            fs::read(dir_b.join("settings.json")).unwrap(),
            fs::read(dir_b.join(".claude.json")).unwrap(),
            fs::read(dir_b.join(PROFILE_MARKER_FILENAME)).unwrap(),
        ];

        provider_a.settings_config["env"]["ANTHROPIC_AUTH_TOKEN"] =
            Value::String("secret-a-updated".to_string());
        sync_profile(&state, &provider_a).expect("refresh A");

        assert_eq!(
            read_json(&dir_a.join("settings.json"))["env"]["ANTHROPIC_AUTH_TOKEN"],
            "secret-a-updated"
        );
        assert_eq!(
            b_before,
            [
                fs::read(dir_b.join("settings.json")).unwrap(),
                fs::read(dir_b.join(".claude.json")).unwrap(),
                fs::read(dir_b.join(PROFILE_MARKER_FILENAME)).unwrap(),
            ]
        );
        let profiles = list_profiles().expect("list profiles");
        assert_eq!(profiles.len(), 2);
        assert!(profiles.iter().all(|profile| !profile.metadata.retired));
    }

    #[test]
    #[serial]
    fn refresh_existing_profile_uses_saved_provider_and_never_creates_new_profile() {
        let _home = TestHome::new();
        let state = state_with_mcp();
        let provider_a = provider("provider-a", "Provider A", "secret-a");
        state
            .db
            .save_provider(AppType::Claude.as_str(), &provider_a)
            .expect("save provider A");
        let dir_a = sync_profile(&state, &provider_a).expect("create profile A");

        let mut updated_a = provider_a.clone();
        updated_a.settings_config["env"]["ANTHROPIC_AUTH_TOKEN"] =
            Value::String("secret-a-updated".to_string());
        state
            .db
            .save_provider(AppType::Claude.as_str(), &updated_a)
            .expect("save updated provider A");
        refresh_profile_if_present(&state, "provider-a").expect("refresh existing A");
        assert_eq!(
            read_json(&dir_a.join("settings.json"))["env"]["ANTHROPIC_AUTH_TOKEN"],
            "secret-a-updated"
        );

        let provider_b = provider("provider-b", "Provider B", "secret-b");
        state
            .db
            .save_provider(AppType::Claude.as_str(), &provider_b)
            .expect("save provider B");
        refresh_profile_if_present(&state, "provider-b").expect("skip missing B profile");
        assert!(!profile_dir("provider-b").exists());
    }

    #[test]
    #[serial]
    fn refresh_does_not_reactivate_a_retired_profile() {
        let _home = TestHome::new();
        let state = state_with_mcp();
        let provider_a = provider("provider-a", "Provider A", "secret-a");
        state
            .db
            .save_provider(AppType::Claude.as_str(), &provider_a)
            .expect("save provider A");
        let dir = sync_profile(&state, &provider_a).expect("create profile A");
        retire_profile("provider-a").expect("retire profile A");

        let mut updated_a = provider_a;
        updated_a.settings_config["env"]["ANTHROPIC_AUTH_TOKEN"] =
            Value::String("secret-a-updated".to_string());
        state
            .db
            .save_provider(AppType::Claude.as_str(), &updated_a)
            .expect("save updated provider A");
        refresh_profile_if_present(&state, "provider-a").expect("skip retired profile");

        assert!(!dir.join("settings.json").exists());
        assert!(!dir.join(".claude.json").exists());
        assert!(list_profiles().expect("list profiles")[0].metadata.retired);
    }

    #[test]
    #[serial]
    fn claude_mcp_mutations_refresh_active_profiles() {
        let _home = TestHome::new();
        let state = state_with_mcp();
        let provider = provider("provider-a", "Provider A", "secret-a");
        state
            .db
            .save_provider(AppType::Claude.as_str(), &provider)
            .expect("save provider A");
        let dir = sync_profile(&state, &provider).expect("create profile A");

        crate::services::McpService::upsert_server(
            &state,
            McpServer {
                id: "late-mcp".to_string(),
                name: "Late MCP".to_string(),
                server: json!({"command": "late-mcp"}),
                apps: McpApps {
                    claude: true,
                    ..McpApps::default()
                },
                description: None,
                homepage: None,
                docs: None,
                tags: Vec::new(),
            },
        )
        .expect("add Claude MCP");
        let mcp = read_json(&dir.join(".claude.json"));
        assert_eq!(mcp["mcpServers"]["late-mcp"]["command"], "late-mcp");

        crate::services::McpService::toggle_app(&state, "late-mcp", AppType::Claude, false)
            .expect("disable Claude MCP");
        let mcp = read_json(&dir.join(".claude.json"));
        assert!(mcp["mcpServers"].get("late-mcp").is_none());

        crate::services::McpService::delete_server(&state, "claude-mcp")
            .expect("delete Claude MCP");
        let mcp = read_json(&dir.join(".claude.json"));
        assert!(mcp["mcpServers"].get("claude-mcp").is_none());
    }

    #[test]
    #[serial]
    fn claude_mcp_mutation_does_not_reactivate_retired_profile() {
        let _home = TestHome::new();
        let state = state_with_mcp();
        let provider = provider("provider-a", "Provider A", "secret-a");
        state
            .db
            .save_provider(AppType::Claude.as_str(), &provider)
            .expect("save provider A");
        let dir = sync_profile(&state, &provider).expect("create profile A");
        retire_profile("provider-a").expect("retire profile A");

        crate::services::McpService::upsert_server(
            &state,
            McpServer {
                id: "late-mcp".to_string(),
                name: "Late MCP".to_string(),
                server: json!({"command": "late-mcp"}),
                apps: McpApps {
                    claude: true,
                    ..McpApps::default()
                },
                description: None,
                homepage: None,
                docs: None,
                tags: Vec::new(),
            },
        )
        .expect("add Claude MCP");

        assert!(!dir.join("settings.json").exists());
        assert!(!dir.join(".claude.json").exists());
        assert!(list_profiles().expect("list profiles")[0].metadata.retired);
    }

    #[test]
    #[serial]
    fn retire_profile_scrubs_secrets_and_preserves_projects_history() {
        let _home = TestHome::new();

        let state = state_with_mcp();
        let provider = provider("provider-a", "Provider A", "secret-a");
        let dir = sync_profile(&state, &provider).expect("sync profile");
        let history = dir.join("projects").join("repo").join("session.jsonl");
        fs::create_dir_all(history.parent().unwrap()).expect("create history dir");
        fs::write(&history, b"session-history").expect("write history");
        fs::write(dir.join(".credentials.json"), b"{\"claudeAiOauth\":{}}")
            .expect("write OAuth credentials");

        retire_profile("provider-a").expect("retire profile");

        assert!(!dir.join("settings.json").exists());
        assert!(!dir.join(".claude.json").exists());
        assert!(!dir.join(".credentials.json").exists());
        assert_eq!(fs::read(history).unwrap(), b"session-history");
        let profiles = list_profiles().expect("list retired profiles");
        assert_eq!(profiles.len(), 1);
        assert!(profiles[0].metadata.retired);
        assert_eq!(profiles[0].metadata.provider_name, "Provider A");
        sync_profile(&state, &provider).expect("reactivate profile");
        assert!(dir.join("settings.json").is_file());
        assert!(dir.join(".claude.json").is_file());
        assert_eq!(
            fs::read(dir.join("projects/repo/session.jsonl")).unwrap(),
            b"session-history"
        );
        assert!(
            !list_profiles().expect("list reactivated profile")[0]
                .metadata
                .retired
        );
    }
}
