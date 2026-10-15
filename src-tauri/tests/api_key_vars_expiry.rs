use std::sync::Arc;

use cc_switch_lib::api_key_vars::{
    self,
    runner::{ApiKeyRefreshConfig, RefreshOutput, ScriptRun},
};
use cc_switch_lib::{AppState, AppType, Database, MultiAppConfig, Provider, ProviderService};
use serde_json::json;

#[path = "support.rs"]
mod support;

fn config() -> ApiKeyRefreshConfig {
    ApiKeyRefreshConfig {
        enabled: true,
        script_dir: String::new(),
        timeout_secs: None,
        lead_time_secs: None,
    }
}

fn refresh(value: &str, expires_at: i64) -> ScriptRun {
    ScriptRun {
        output: RefreshOutput {
            api_key: value.to_string(),
            expires_at: Some(expires_at),
            refresh_after: None,
            message: None,
        },
        resolved_file: "index.mjs".to_string(),
    }
}

fn provider(id: &str, key: &str) -> Provider {
    Provider::with_id(
        id.to_string(),
        id.to_string(),
        json!({
            "env": {
                "ANTHROPIC_AUTH_TOKEN": key,
                "ANTHROPIC_BASE_URL": "https://relay.example"
            }
        }),
        None,
    )
}

#[test]
fn expired_key_blocks_switch_without_touching_current_or_live_config() {
    let _serial = support::test_mutex()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    support::reset_test_fs();
    let home = support::ensure_test_home();

    let mut initial = MultiAppConfig::default();
    let providers = &mut initial
        .get_manager_mut(&AppType::Claude)
        .expect("Claude manager")
        .providers;
    providers.insert("current".into(), provider("current", "valid-current-key"));
    providers.insert("expired".into(), provider("expired", "{{EXPIRY_KEY}}"));
    let state = AppState::new(Arc::new(Database::init().expect("database")));
    state
        .db
        .migrate_from_json(&initial)
        .expect("seed providers");

    ProviderService::switch(&state, AppType::Claude, "current").expect("switch baseline");
    let live_path = home.join(".claude").join("settings.json");
    assert_eq!(
        state
            .db
            .get_current_provider("claude")
            .expect("current row"),
        Some("current".to_string())
    );

    state
        .db
        .upsert_api_key_var(None, "EXPIRY_KEY", &config())
        .expect("create key variable");
    // A failed pre-switch refresh may retain this value. While it is still
    // within its reported lifetime, activation remains valid.
    api_key_vars::commit_result(
        &state,
        "EXPIRY_KEY",
        Ok(refresh(
            "still-valid-key",
            chrono::Utc::now().timestamp_millis() + 60_000,
        )),
    )
    .expect("persist still-valid fallback");
    ProviderService::switch(&state, AppType::Claude, "expired")
        .expect("an unexpired retained key remains switchable");
    ProviderService::switch(&state, AppType::Claude, "current").expect("restore baseline");
    let before_live = std::fs::read(&live_path).expect("restored baseline live config");
    assert_eq!(
        state
            .db
            .get_current_provider("claude")
            .expect("current row"),
        Some("current".to_string())
    );

    api_key_vars::commit_result(
        &state,
        "EXPIRY_KEY",
        Ok(refresh(
            "expired-key",
            chrono::Utc::now().timestamp_millis() - 1,
        )),
    )
    .expect("persist expired value");

    let error = ProviderService::switch(&state, AppType::Claude, "expired")
        .expect_err("an expired variable must not activate a provider");
    assert!(error.to_string().contains("已过期"), "{error}");
    assert_eq!(
        std::fs::read(&live_path).expect("unchanged live config"),
        before_live
    );
    assert_eq!(
        state
            .db
            .get_current_provider("claude")
            .expect("current unchanged"),
        Some("current".to_string())
    );

    // A successful refresh replaces the expired value and makes the same
    // provider switchable again. The provider row keeps the template.
    api_key_vars::commit_result(
        &state,
        "EXPIRY_KEY",
        Ok(refresh(
            "fresh-key",
            chrono::Utc::now().timestamp_millis() + 60_000,
        )),
    )
    .expect("persist refreshed value");
    ProviderService::switch(&state, AppType::Claude, "expired")
        .expect("fresh replacement allows switch");
    assert_eq!(
        state
            .db
            .get_current_provider("claude")
            .expect("current refreshed"),
        Some("expired".to_string())
    );
    let row = state
        .db
        .get_provider_by_id("expired", "claude")
        .expect("read provider")
        .expect("provider exists");
    assert_eq!(
        row.settings_config["env"]["ANTHROPIC_AUTH_TOKEN"],
        "{{EXPIRY_KEY}}"
    );
    assert!(std::fs::read_to_string(&live_path)
        .expect("updated live config")
        .contains("fresh-key"));
}

#[test]
fn editing_current_provider_to_an_expired_variable_preserves_row_and_live_config() {
    let _serial = support::test_mutex()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    support::reset_test_fs();
    let home = support::ensure_test_home();

    let mut initial = MultiAppConfig::default();
    initial
        .get_manager_mut(&AppType::Claude)
        .expect("Claude manager")
        .providers
        .insert("current".into(), provider("current", "original-key"));
    let state = AppState::new(Arc::new(Database::init().expect("database")));
    state.db.migrate_from_json(&initial).expect("seed provider");
    ProviderService::switch(&state, AppType::Claude, "current").expect("switch baseline");
    let live_path = home.join(".claude").join("settings.json");
    let before_live = std::fs::read(&live_path).expect("baseline live config");

    state
        .db
        .upsert_api_key_var(None, "EDIT_EXPIRY_KEY", &config())
        .expect("create key variable");
    api_key_vars::commit_result(
        &state,
        "EDIT_EXPIRY_KEY",
        Ok(refresh(
            "expired-edit-key",
            chrono::Utc::now().timestamp_millis() - 1,
        )),
    )
    .expect("persist expired value");

    let mut edited = state
        .db
        .get_provider_by_id("current", "claude")
        .expect("read current provider")
        .expect("current provider exists");
    edited.settings_config["env"]["ANTHROPIC_AUTH_TOKEN"] = json!("{{EDIT_EXPIRY_KEY}}");
    let error = ProviderService::update(&state, AppType::Claude, None, edited)
        .expect_err("editing the active provider to an expired key must fail");
    assert!(error.to_string().contains("已过期"), "{error}");
    assert_eq!(
        std::fs::read(&live_path).expect("live config unchanged"),
        before_live
    );
    let saved = state
        .db
        .get_provider_by_id("current", "claude")
        .expect("read saved provider")
        .expect("saved provider exists");
    assert_eq!(
        saved.settings_config["env"]["ANTHROPIC_AUTH_TOKEN"],
        "original-key"
    );
}

#[cfg(windows)]
struct EnvRestore {
    name: &'static str,
    previous: Option<std::ffi::OsString>,
}

#[cfg(windows)]
impl EnvRestore {
    fn set(name: &'static str, value: &std::path::Path) -> Self {
        let previous = std::env::var_os(name);
        std::env::set_var(name, value);
        Self { name, previous }
    }
}

#[cfg(windows)]
impl Drop for EnvRestore {
    fn drop(&mut self) {
        if let Some(value) = self.previous.take() {
            std::env::set_var(self.name, value);
        } else {
            std::env::remove_var(self.name);
        }
    }
}

#[cfg(windows)]
#[test]
fn expired_desktop_switch_keeps_current_pointer_and_profile_bytes() {
    let _serial = support::test_mutex()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    support::reset_test_fs();
    let home = support::ensure_test_home().to_path_buf();
    let local_app_data = home.join("desktop-appdata").join("Local");
    let _local_app_data = EnvRestore::set("LOCALAPPDATA", &local_app_data);
    let state = AppState::new(Arc::new(Database::init().expect("database")));

    ProviderService::add(
        &state,
        AppType::ClaudeDesktop,
        provider("desktop-current", "desktop-current-key"),
        true,
    )
    .expect("add current desktop provider");
    ProviderService::switch(&state, AppType::ClaudeDesktop, "desktop-current")
        .expect("select current desktop provider");

    state
        .db
        .upsert_api_key_var(None, "DESKTOP_EXPIRY_KEY", &config())
        .expect("create desktop key variable");
    api_key_vars::commit_result(
        &state,
        "DESKTOP_EXPIRY_KEY",
        Ok(refresh(
            "expired-desktop-key",
            chrono::Utc::now().timestamp_millis() - 1,
        )),
    )
    .expect("persist expired desktop key");
    ProviderService::add(
        &state,
        AppType::ClaudeDesktop,
        provider("desktop-expired", "{{DESKTOP_EXPIRY_KEY}}"),
        false,
    )
    .expect("store inactive expired desktop provider");

    let profile = local_app_data
        .join("Claude-3p")
        .join("configLibrary")
        .join("00000000-0000-4000-8000-000000157210.json");
    let before_profile = std::fs::read(&profile).expect("desktop profile");
    let error = ProviderService::switch(&state, AppType::ClaudeDesktop, "desktop-expired")
        .expect_err("expired desktop key must not switch");
    assert!(error.to_string().contains("已过期"), "{error}");
    assert_eq!(
        std::fs::read(&profile).expect("desktop profile unchanged"),
        before_profile
    );
    assert_eq!(
        state
            .db
            .get_current_provider(AppType::ClaudeDesktop.as_str())
            .expect("desktop current"),
        Some("desktop-current".to_string())
    );
}
