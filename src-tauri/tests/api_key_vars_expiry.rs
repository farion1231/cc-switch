use std::sync::Arc;

use cc_switch_lib::api_key_vars::{
    self,
    runner::{ApiKeyRefreshConfig, RefreshOutput, ScriptRun},
};
use cc_switch_lib::{AppState, AppType, Database, MultiAppConfig, Provider, ProviderService};
use serde_json::{json, Value};

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

fn editor_provider(id: &str, settings: Value) -> Provider {
    Provider::with_id(id.to_string(), id.to_string(), settings, None)
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

#[test]
fn editor_save_of_expired_key_keeps_existing_rows_and_live_files() {
    let _serial = support::test_mutex()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    support::reset_test_fs();
    let home = support::ensure_test_home();
    let state = AppState::new(Arc::new(Database::init().expect("database")));

    state
        .db
        .upsert_api_key_var(None, "EDITOR_EXPIRED_KEY", &config())
        .expect("create key variable");
    api_key_vars::commit_result(
        &state,
        "EDITOR_EXPIRED_KEY",
        Ok(refresh(
            "expired-editor-key",
            chrono::Utc::now().timestamp_millis() - 1,
        )),
    )
    .expect("persist expired value");

    let cases = [
        (
            AppType::Claude,
            "editor-claude",
            json!({"env": {
                "ANTHROPIC_AUTH_TOKEN": "original-claude-key",
                "ANTHROPIC_BASE_URL": "https://claude.example"
            }}),
        ),
        (
            AppType::Codex,
            "editor-codex",
            json!({
                "auth": {"OPENAI_API_KEY": "original-codex-key"},
                "config": "model_provider = \"relay\"\n[model_providers.relay]\nname = \"Relay\"\nbase_url = \"https://codex.example/v1\"\nwire_api = \"responses\"\n"
            }),
        ),
        (
            AppType::Gemini,
            "editor-gemini",
            json!({"env": {
                "GEMINI_API_KEY": "original-gemini-key",
                "GOOGLE_GEMINI_BASE_URL": "https://gemini.example"
            }}),
        ),
        (
            AppType::GrokBuild,
            "editor-grok",
            json!({"config": "[models]\ndefault = \"grok-test\"\n\n[model.\"grok-test\"]\nmodel = \"grok-test\"\nname = \"Test\"\nbase_url = \"https://grok.example/v1\"\napi_key = \"original-grok-key\"\napi_backend = \"responses\"\ncontext_window = 500000\n"}),
        ),
    ];

    for (app_type, id, raw) in cases {
        let live_path = match &app_type {
            AppType::Claude => cc_switch_lib::get_claude_settings_path(),
            // Third-party Codex credentials are projected into config.toml.
            AppType::Codex => cc_switch_lib::get_codex_config_path(),
            AppType::Gemini => home.join(".gemini/.env"),
            AppType::GrokBuild => cc_switch_lib::get_grok_config_path(),
            _ => unreachable!("only dedicated editor applications are covered"),
        };
        assert!(live_path.starts_with(home), "{}", live_path.display());

        let initial_base = ProviderService::editor_view(&state, app_type.clone(), &raw, None)
            .expect("initial editor view")
            .settings;
        ProviderService::add_from_editor(
            &state,
            app_type.clone(),
            editor_provider(id, raw),
            true,
            Some(
                serde_json::from_value(json!({"base": initial_base, "onConflict": "refuse"}))
                    .expect("editor save"),
            ),
        )
        .expect("add initial live provider");

        let before_row = state
            .db
            .get_provider_by_id(id, app_type.as_str())
            .expect("read initial provider")
            .expect("initial provider exists");
        let before_live = std::fs::read(&live_path).expect("read initial live file");
        let base = ProviderService::editor_view(
            &state,
            app_type.clone(),
            &before_row.settings_config,
            None,
        )
        .expect("persisted editor view")
        .settings;
        let mut changed = before_row.clone();
        changed.settings_config = base.clone();
        match &app_type {
            AppType::Claude => {
                changed.settings_config["env"]["ANTHROPIC_AUTH_TOKEN"] =
                    json!("{{EDITOR_EXPIRED_KEY}}");
            }
            AppType::Codex => {
                changed.settings_config["auth"]["OPENAI_API_KEY"] = json!("{{EDITOR_EXPIRED_KEY}}");
            }
            AppType::Gemini => {
                changed.settings_config["env"]["GEMINI_API_KEY"] = json!("{{EDITOR_EXPIRED_KEY}}");
            }
            AppType::GrokBuild => {
                let config = changed.settings_config["config"]
                    .as_str()
                    .expect("Grok config")
                    .replace("original-grok-key", "{{EDITOR_EXPIRED_KEY}}");
                changed.settings_config["config"] = json!(config);
            }
            _ => unreachable!(),
        }

        let error = ProviderService::update_from_editor(
            &state,
            app_type.clone(),
            None,
            changed,
            Some(
                serde_json::from_value(json!({"base": base, "onConflict": "refuse"}))
                    .expect("editor save"),
            ),
        )
        .expect_err("expired variable must not be projected through an editor save");
        assert!(
            error.to_string().contains("已过期"),
            "{app_type:?}: {error}"
        );
        assert_eq!(
            std::fs::read(&live_path).expect("live file unchanged"),
            before_live,
            "{app_type:?} live file changed"
        );
        let saved = state
            .db
            .get_provider_by_id(id, app_type.as_str())
            .expect("read saved provider")
            .expect("saved provider exists");
        assert_eq!(
            serde_json::to_value(saved).expect("serialize saved provider"),
            serde_json::to_value(before_row).expect("serialize baseline provider"),
            "{app_type:?} row changed"
        );
    }
}

#[test]
fn first_editor_add_of_expired_key_creates_no_provider_or_live_file() {
    let _serial = support::test_mutex()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    support::reset_test_fs();
    let home = support::ensure_test_home();
    let state = AppState::new(Arc::new(Database::init().expect("database")));

    state
        .db
        .upsert_api_key_var(None, "EDITOR_ADD_EXPIRED_KEY", &config())
        .expect("create key variable");
    api_key_vars::commit_result(
        &state,
        "EDITOR_ADD_EXPIRED_KEY",
        Ok(refresh(
            "expired-editor-add-key",
            chrono::Utc::now().timestamp_millis() - 1,
        )),
    )
    .expect("persist expired value");

    let cases = [
        (
            AppType::Claude,
            "add-claude",
            json!({"env": {
                "ANTHROPIC_AUTH_TOKEN": "{{EDITOR_ADD_EXPIRED_KEY}}",
                "ANTHROPIC_BASE_URL": "https://claude.example"
            }}),
        ),
        (
            AppType::Codex,
            "add-codex",
            json!({
                "auth": {"OPENAI_API_KEY": "{{EDITOR_ADD_EXPIRED_KEY}}"},
                "config": "model_provider = \"relay\"\n[model_providers.relay]\nname = \"Relay\"\nbase_url = \"https://codex.example/v1\"\nwire_api = \"responses\"\n"
            }),
        ),
        (
            AppType::Gemini,
            "add-gemini",
            json!({"env": {
                "GEMINI_API_KEY": "{{EDITOR_ADD_EXPIRED_KEY}}",
                "GOOGLE_GEMINI_BASE_URL": "https://gemini.example"
            }}),
        ),
        (
            AppType::GrokBuild,
            "add-grok",
            json!({"config": "[models]\ndefault = \"grok-test\"\n\n[model.\"grok-test\"]\nmodel = \"grok-test\"\nname = \"Test\"\nbase_url = \"https://grok.example/v1\"\napi_key = \"{{EDITOR_ADD_EXPIRED_KEY}}\"\napi_backend = \"responses\"\ncontext_window = 500000\n"}),
        ),
    ];

    for (app_type, id, raw) in cases {
        let live_path = match &app_type {
            AppType::Claude => cc_switch_lib::get_claude_settings_path(),
            AppType::Codex => cc_switch_lib::get_codex_config_path(),
            AppType::Gemini => home.join(".gemini/.env"),
            AppType::GrokBuild => cc_switch_lib::get_grok_config_path(),
            _ => unreachable!(),
        };
        let base = ProviderService::editor_view(&state, app_type.clone(), &raw, None)
            .expect("editor view")
            .settings;
        let error = ProviderService::add_from_editor(
            &state,
            app_type.clone(),
            editor_provider(id, raw),
            true,
            Some(
                serde_json::from_value(json!({"base": base, "onConflict": "refuse"}))
                    .expect("editor save"),
            ),
        )
        .expect_err("first editor add must reject an expired live key");
        assert!(
            error.to_string().contains("已过期"),
            "{app_type:?}: {error}"
        );
        assert!(
            state
                .db
                .get_provider_by_id(id, app_type.as_str())
                .expect("read provider")
                .is_none(),
            "{app_type:?} persisted an invalid provider"
        );
        assert!(
            !live_path.exists(),
            "{app_type:?} created {}",
            live_path.display()
        );
    }
}

#[test]
fn editor_drafts_may_retain_expired_references_when_not_projected() {
    let _serial = support::test_mutex()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    support::reset_test_fs();
    let state = AppState::new(Arc::new(Database::init().expect("database")));

    state
        .db
        .upsert_api_key_var(None, "EDITOR_DRAFT_EXPIRED_KEY", &config())
        .expect("create key variable");
    api_key_vars::commit_result(
        &state,
        "EDITOR_DRAFT_EXPIRED_KEY",
        Ok(refresh(
            "expired-editor-draft-key",
            chrono::Utc::now().timestamp_millis() - 1,
        )),
    )
    .expect("persist expired value");

    let current = json!({"env": {
        "ANTHROPIC_AUTH_TOKEN": "current-key",
        "ANTHROPIC_BASE_URL": "https://claude.example"
    }});
    let current_base = ProviderService::editor_view(&state, AppType::Claude, &current, None)
        .expect("current editor view")
        .settings;
    ProviderService::add_from_editor(
        &state,
        AppType::Claude,
        editor_provider("current", current),
        true,
        Some(
            serde_json::from_value(json!({"base": current_base, "onConflict": "refuse"}))
                .expect("editor save"),
        ),
    )
    .expect("add current provider");

    let draft = json!({"env": {
        "ANTHROPIC_AUTH_TOKEN": "{{EDITOR_DRAFT_EXPIRED_KEY}}",
        "ANTHROPIC_BASE_URL": "https://draft.example"
    }});
    let draft_base = ProviderService::editor_view(&state, AppType::Claude, &draft, None)
        .expect("draft editor view")
        .settings;
    ProviderService::add_from_editor(
        &state,
        AppType::Claude,
        editor_provider("draft", draft),
        true,
        Some(
            serde_json::from_value(json!({"base": draft_base, "onConflict": "refuse"}))
                .expect("editor save"),
        ),
    )
    .expect("inactive draft must remain storable");
    assert_eq!(
        state
            .db
            .get_provider_by_id("draft", AppType::Claude.as_str())
            .expect("read draft")
            .expect("draft exists")
            .settings_config["env"]["ANTHROPIC_AUTH_TOKEN"],
        "{{EDITOR_DRAFT_EXPIRED_KEY}}"
    );
}

#[test]
fn proxy_route_editor_save_rejects_expired_key_before_row_write() {
    let _serial = support::test_mutex()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    support::reset_test_fs();
    let state = AppState::new(Arc::new(Database::init().expect("database")));

    state
        .db
        .upsert_api_key_var(None, "EDITOR_PROXY_EXPIRED_KEY", &config())
        .expect("create key variable");
    api_key_vars::commit_result(
        &state,
        "EDITOR_PROXY_EXPIRED_KEY",
        Ok(refresh(
            "expired-editor-proxy-key",
            chrono::Utc::now().timestamp_millis() - 1,
        )),
    )
    .expect("persist expired value");

    ProviderService::add(
        &state,
        AppType::Claude,
        provider("direct", "direct-key"),
        true,
    )
    .expect("add direct provider");
    ProviderService::add(
        &state,
        AppType::Claude,
        provider("route", "route-key"),
        false,
    )
    .expect("add route provider");

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("proxy runtime");
    runtime.block_on(async {
        let mut proxy = state
            .db
            .get_proxy_config()
            .await
            .expect("read proxy config");
        proxy.listen_port = 0;
        state
            .db
            .update_proxy_config(proxy)
            .await
            .expect("use isolated random proxy port");
        cc_switch_lib::mode::controller::enter_with_route(
            &state,
            &AppType::Claude,
            false,
            Some("route"),
        )
        .await
        .expect("enter proxy route mode");
    });

    let before = state
        .db
        .get_provider_by_id("route", AppType::Claude.as_str())
        .expect("read route")
        .expect("route exists");
    let base = ProviderService::editor_view(&state, AppType::Claude, &before.settings_config, None)
        .expect("route editor view")
        .settings;
    let mut changed = before.clone();
    changed.settings_config = base.clone();
    changed.settings_config["env"]["ANTHROPIC_AUTH_TOKEN"] = json!("{{EDITOR_PROXY_EXPIRED_KEY}}");
    let result = ProviderService::update_from_editor(
        &state,
        AppType::Claude,
        None,
        changed,
        Some(
            serde_json::from_value(json!({"base": base, "onConflict": "refuse"}))
                .expect("editor save"),
        ),
    );
    runtime.block_on(async {
        state
            .proxy_service
            .stop()
            .await
            .expect("stop isolated proxy service");
    });

    let error = result.expect_err("proxy route must not project an expired key");
    assert!(error.to_string().contains("已过期"), "{error}");
    assert_eq!(
        serde_json::to_value(
            state
                .db
                .get_provider_by_id("route", AppType::Claude.as_str())
                .expect("read saved route")
                .expect("saved route exists"),
        )
        .expect("serialize route"),
        serde_json::to_value(before).expect("serialize original route")
    );
}

#[test]
fn proxy_stack_editor_save_rejects_expired_key_before_row_write() {
    let _serial = support::test_mutex()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    support::reset_test_fs();
    let home = support::ensure_test_home();
    let state = AppState::new(Arc::new(Database::init().expect("database")));

    state
        .db
        .upsert_api_key_var(None, "EDITOR_STACK_EXPIRED_KEY", &config())
        .expect("create key variable");
    api_key_vars::commit_result(
        &state,
        "EDITOR_STACK_EXPIRED_KEY",
        Ok(refresh(
            "expired-editor-stack-key",
            chrono::Utc::now().timestamp_millis() - 1,
        )),
    )
    .expect("persist expired value");

    ProviderService::add(
        &state,
        AppType::Claude,
        provider("direct", "direct-key"),
        true,
    )
    .expect("add direct provider");
    ProviderService::add(
        &state,
        AppType::Claude,
        provider("route", "route-key"),
        false,
    )
    .expect("add route provider");
    ProviderService::add(
        &state,
        AppType::Claude,
        provider("stack-member", "stack-member-key"),
        false,
    )
    .expect("add Stack provider");

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("proxy runtime");
    runtime.block_on(async {
        let mut proxy = state
            .db
            .get_proxy_config()
            .await
            .expect("read proxy config");
        proxy.listen_port = 0;
        state
            .db
            .update_proxy_config(proxy)
            .await
            .expect("use isolated random proxy port");
        cc_switch_lib::mode::controller::enter_with_route(
            &state,
            &AppType::Claude,
            true,
            Some("route"),
        )
        .await
        .expect("enter proxy Stack mode");
        cc_switch_lib::mode::controller::set_stack_member(
            &state,
            &AppType::Claude,
            "stack-member",
            true,
        )
        .await
        .expect("enable Stack member");
    });

    let live_path = home.join(".claude").join("settings.json");
    let before_live = std::fs::read(&live_path).expect("read Stack contract");
    let before = state
        .db
        .get_provider_by_id("stack-member", AppType::Claude.as_str())
        .expect("read Stack member")
        .expect("Stack member exists");
    let base = ProviderService::editor_view(&state, AppType::Claude, &before.settings_config, None)
        .expect("Stack member editor view")
        .settings;
    let mut changed = before.clone();
    changed.settings_config = base.clone();
    changed.settings_config["env"]["ANTHROPIC_AUTH_TOKEN"] = json!("{{EDITOR_STACK_EXPIRED_KEY}}");
    let result = ProviderService::update_from_editor(
        &state,
        AppType::Claude,
        None,
        changed,
        Some(
            serde_json::from_value(json!({"base": base, "onConflict": "refuse"}))
                .expect("editor save"),
        ),
    );
    runtime.block_on(async {
        state
            .proxy_service
            .stop()
            .await
            .expect("stop isolated proxy service");
    });

    let error = result.expect_err("Stack member must not project an expired key");
    assert!(error.to_string().contains("已过期"), "{error}");
    assert_eq!(
        std::fs::read(&live_path).expect("Stack contract unchanged"),
        before_live
    );
    assert_eq!(
        serde_json::to_value(
            state
                .db
                .get_provider_by_id("stack-member", AppType::Claude.as_str())
                .expect("read saved Stack member")
                .expect("saved Stack member exists"),
        )
        .expect("serialize Stack member"),
        serde_json::to_value(before).expect("serialize original Stack member")
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
