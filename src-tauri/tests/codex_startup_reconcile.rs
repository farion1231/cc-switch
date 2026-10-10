use cc_switch_lib::{
    get_codex_auth_path, get_codex_config_path, update_settings, AppSettings, AppState, AppType,
    Provider, ProviderService,
};
use serde_json::json;
use std::fs;

#[path = "support.rs"]
mod support;
use support::{create_test_state, ensure_test_home, reset_test_fs, test_mutex};

const CUSTOM: &str = r#"# Keep the user's formatting and unrelated settings.
model_provider = "relay"
model = "custom-model"

[model_providers.relay]
name = "relay"
base_url = "https://relay.example/v1"
wire_api = "responses"
requires_openai_auth = true

[mcp_servers.local]
command = "echo"
"#;

const LEGACY_AUTH: &str = r#"{"OPENAI_API_KEY":"local-key"}"#;

fn bearer_config() -> String {
    CUSTOM.replace(
        "requires_openai_auth = true",
        "requires_openai_auth = true\nexperimental_bearer_token = 'local-key'",
    )
}

fn top_level_reroute(explicit_selector: bool) -> String {
    let selector = if explicit_selector {
        "model_provider = 'openai'\n"
    } else {
        ""
    };
    format!(
        "{selector}openai_base_url = 'https://relay.example/v1'\n\
         model = 'custom-model'\n\n\
         [model_providers.unused]\n\
         base_url = 'https://unused.example/v1'\n\
         experimental_bearer_token = 'unused-key'\n"
    )
}

fn fixture(config: &str, auth: Option<&str>) -> AppState {
    reset_test_fs();
    fs::create_dir_all(get_codex_config_path().parent().unwrap()).unwrap();
    fs::write(get_codex_config_path(), config).unwrap();
    if let Some(auth) = auth {
        fs::write(get_codex_auth_path(), auth).unwrap();
    }
    let state = create_test_state().unwrap();
    // A manually added official card has a UUID, not the built-in seed id.
    let mut official = Provider::with_id(
        "manual-official".into(),
        "OpenAI Official".into(),
        json!({"auth": {}, "config": ""}),
        None,
    );
    official.category = Some("official".into());
    state.db.save_provider("codex", &official).unwrap();
    state
        .db
        .set_current_provider("codex", &official.id)
        .unwrap();
    update_settings(AppSettings {
        current_provider_codex: Some(official.id),
        ..Default::default()
    })
    .unwrap();
    fs::write(
        ensure_test_home().join(".cc-switch/live-state.json"),
        json!({"version": 1, "apps": {"codex": {"mode": "direct"}}}).to_string(),
    )
    .unwrap();
    state
}

fn reconcile_without_live_writes(state: &AppState) -> Result<bool, cc_switch_lib::AppError> {
    let snapshot = |path| {
        fs::read(&path)
            .ok()
            .map(|bytes| (bytes, fs::metadata(path).unwrap().modified().unwrap()))
    };
    let before_config = snapshot(get_codex_config_path());
    let before_auth = snapshot(get_codex_auth_path());
    let result = ProviderService::reconcile_codex_current_on_startup(state);
    assert_eq!(snapshot(get_codex_config_path()), before_config);
    assert_eq!(snapshot(get_codex_auth_path()), before_auth);
    result
}

#[test]
fn stale_manual_official_is_reconciled_once_and_survives_restart() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let state = fixture(CUSTOM, Some("{\n  \"OPENAI_API_KEY\": \"local-key\"\n}\n"));
    let official_before = state
        .db
        .get_provider_by_id("manual-official", "codex")
        .unwrap();
    assert!(reconcile_without_live_writes(&state).unwrap());
    let providers = ProviderService::list(&state, AppType::Codex).unwrap();
    assert_eq!(providers.len(), 2);
    assert_eq!(providers["default"].name, "relay");
    assert_eq!(providers["default"].category.as_deref(), Some("custom"));
    assert_eq!(providers["default"].settings_config["config"], CUSTOM);
    assert_eq!(
        providers["default"].settings_config["auth"]["OPENAI_API_KEY"],
        "local-key"
    );
    assert_eq!(
        serde_json::to_value(
            state
                .db
                .get_provider_by_id("manual-official", "codex")
                .unwrap()
        )
        .unwrap(),
        serde_json::to_value(official_before).unwrap()
    );
    let settings: serde_json::Value = serde_json::from_slice(
        &fs::read(ensure_test_home().join(".cc-switch/settings.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(settings["currentProviderCodex"], "default");
    drop(state);

    let restarted = create_test_state().unwrap();
    assert_eq!(
        ProviderService::current(&restarted, AppType::Codex).unwrap(),
        "default"
    );
    assert_eq!(
        restarted
            .db
            .get_current_provider("codex")
            .unwrap()
            .as_deref(),
        Some("default")
    );
    assert!(!reconcile_without_live_writes(&restarted).unwrap());
    assert_eq!(
        ProviderService::list(&restarted, AppType::Codex)
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn top_level_reroute_imports_legacy_key_despite_unused_provider_tables() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    for explicit_selector in [true, false] {
        let config = top_level_reroute(explicit_selector);
        let state = fixture(&config, Some(LEGACY_AUTH));
        assert!(reconcile_without_live_writes(&state).unwrap());
        let providers = ProviderService::list(&state, AppType::Codex).unwrap();
        assert_eq!(providers.len(), 2);
        assert_eq!(
            providers["default"].settings_config["auth"],
            json!({"OPENAI_API_KEY": "local-key"})
        );
        assert_eq!(providers["default"].settings_config["config"], config);
        assert_eq!(
            ProviderService::current(&state, AppType::Codex).unwrap(),
            "default"
        );
        assert!(!reconcile_without_live_writes(&state).unwrap());
    }
}

#[test]
fn top_level_reroute_reuses_existing_card_with_the_same_key() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    for explicit_selector in [true, false] {
        let config = top_level_reroute(explicit_selector);
        // Both the original top-level shape and a custom table represent the
        // same route once the switch engine has moved the key into the table.
        for saved_config in [config.as_str(), CUSTOM] {
            let state = fixture(&config, Some(LEGACY_AUTH));
            let mut saved = Provider::with_id(
                "saved-relay".into(),
                "My relay".into(),
                json!({"auth": {"OPENAI_API_KEY": "local-key"}, "config": saved_config}),
                None,
            );
            saved.category = Some("custom".into());
            saved.notes = Some("Keep my provider metadata".into());
            state.db.save_provider("codex", &saved).unwrap();
            let before = state
                .db
                .get_provider_by_id(&saved.id, "codex")
                .unwrap()
                .unwrap();
            assert!(reconcile_without_live_writes(&state).unwrap());
            assert_eq!(
                ProviderService::current(&state, AppType::Codex).unwrap(),
                saved.id
            );
            let providers = ProviderService::list(&state, AppType::Codex).unwrap();
            assert_eq!(providers.len(), 2);
            assert_eq!(
                serde_json::to_value(&providers[&saved.id]).unwrap(),
                serde_json::to_value(&before).unwrap()
            );
        }
    }
}

#[test]
fn top_level_reroute_keeps_bearer_after_switching_away_and_back() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    for explicit_selector in [true, false] {
        for preserve in [false, true] {
            let state = fixture(&top_level_reroute(explicit_selector), Some(LEGACY_AUTH));
            update_settings(AppSettings {
                current_provider_codex: Some("manual-official".into()),
                preserve_codex_official_auth_on_switch: preserve,
                ..Default::default()
            })
            .unwrap();
            assert!(reconcile_without_live_writes(&state).unwrap());
            ProviderService::switch(&state, AppType::Codex, "manual-official").unwrap();
            ProviderService::switch(&state, AppType::Codex, "default").unwrap();
            let config = fs::read_to_string(get_codex_config_path())
                .unwrap()
                .parse::<toml::Value>()
                .unwrap();
            assert_eq!(config["model_provider"].as_str(), Some("custom"));
            let route = &config["model_providers"]["custom"];
            assert_eq!(route["base_url"].as_str(), Some("https://relay.example/v1"));
            assert_eq!(
                route["experimental_bearer_token"].as_str(),
                Some("local-key")
            );
            if !preserve {
                assert!(!get_codex_auth_path().exists());
            }
            assert_eq!(
                ProviderService::current(&state, AppType::Codex).unwrap(),
                "default"
            );
        }
    }
}

#[test]
fn top_level_reroute_skips_import_without_an_unambiguous_legacy_key() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    for explicit_selector in [true, false] {
        for auth in [
            None,
            Some("{}"),
            Some(r#"{"tokens":{"access_token":"native-login"}}"#),
            Some(r#"{"OPENAI_API_KEY":"unrelated-key","tokens":{"access_token":"native-login"}}"#),
        ] {
            let state = fixture(&top_level_reroute(explicit_selector), auth);
            assert!(!reconcile_without_live_writes(&state).unwrap());
            assert_eq!(
                ProviderService::current(&state, AppType::Codex).unwrap(),
                "manual-official"
            );
            assert_eq!(
                ProviderService::list(&state, AppType::Codex).unwrap().len(),
                1
            );
        }
    }
}

#[test]
fn custom_route_stays_custom_with_oauth_or_no_auth_file() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    for auth in [
        None,
        Some(r#"{"tokens":{"access_token":"official-login"}}"#),
    ] {
        let state = fixture(&bearer_config(), auth);
        assert!(reconcile_without_live_writes(&state).unwrap());
        let provider = state
            .db
            .get_provider_by_id("default", "codex")
            .unwrap()
            .unwrap();
        assert_eq!(provider.category.as_deref(), Some("custom"));
        assert_eq!(provider.settings_config["auth"], json!({}));
    }
}

fn native_login() -> serde_json::Value {
    json!({
        "auth_mode": "chatgpt",
        "OPENAI_API_KEY": null,
        "tokens": {
            "id_token": "synthetic-native-id",
            "access_token": "synthetic-native-access",
            "refresh_token": "synthetic-native-refresh",
            "account_id": "synthetic-native-account"
        },
        "last_refresh": "2026-10-01T00:00:00Z"
    })
}

fn read_auth() -> serde_json::Value {
    serde_json::from_slice(&fs::read(get_codex_auth_path()).unwrap()).unwrap()
}

#[test]
fn native_login_survives_reconcile_restart_and_switching_back_to_official() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    for preserve in [false, true] {
        for auth in [
            native_login(),
            json!({"auth_mode": "apikey", "OPENAI_API_KEY": "synthetic-native-api-key"}),
        ] {
            let state = fixture(&bearer_config(), Some(&auth.to_string()));
            update_settings(AppSettings {
                current_provider_codex: Some("manual-official".into()),
                preserve_codex_official_auth_on_switch: preserve,
                ..Default::default()
            })
            .unwrap();
            assert!(reconcile_without_live_writes(&state).unwrap());
            drop(state);

            let restarted = create_test_state().unwrap();
            assert!(!reconcile_without_live_writes(&restarted).unwrap());
            ProviderService::switch(&restarted, AppType::Codex, "manual-official").unwrap();
            assert_eq!(read_auth(), auth, "switching back keeps the native login");

            // Exercise the existing login-preservation switch after discovery,
            // including a CLI token refresh before switching away again.
            let mut refreshed = auth.clone();
            if refreshed.get("tokens").is_some() {
                refreshed["tokens"]["refresh_token"] = json!("synthetic-rotated-refresh");
            }
            fs::write(get_codex_auth_path(), refreshed.to_string()).unwrap();
            ProviderService::switch(&restarted, AppType::Codex, "default").unwrap();
            if preserve {
                assert_eq!(read_auth(), refreshed);
            } else {
                assert!(!get_codex_auth_path().exists());
                let stash: serde_json::Value = serde_json::from_slice(
                    &fs::read(ensure_test_home().join(".cc-switch/codex-login-stash.json"))
                        .unwrap(),
                )
                .unwrap();
                assert!(stash["logins"]
                    .as_object()
                    .unwrap()
                    .values()
                    .any(|v| v == &refreshed));
            }
            drop(restarted);
            let restarted = create_test_state().unwrap();
            assert!(!reconcile_without_live_writes(&restarted).unwrap());
            ProviderService::switch(&restarted, AppType::Codex, "manual-official").unwrap();
            assert_eq!(read_auth(), refreshed, "the latest login is restored");
            let export = restarted.db.export_sql_string_for_sync().unwrap();
            for secret in ["synthetic-native-", "synthetic-rotated-refresh"] {
                assert!(
                    !export.contains(secret),
                    "native login must stay device-local"
                );
            }
        }
    }
}

#[test]
fn reconciliation_keeps_an_existing_login_stash_restorable() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let state = fixture(&bearer_config(), None);
    let auth = native_login();
    let path = ensure_test_home().join(".cc-switch/codex-login-stash.json");
    let stash = json!({
        "logins": {"account:synthetic-native-account": auth},
        "last": "account:synthetic-native-account"
    })
    .to_string();
    fs::write(&path, &stash).unwrap();
    assert!(reconcile_without_live_writes(&state).unwrap());
    assert_eq!(fs::read_to_string(&path).unwrap(), stash);
    drop(state);
    let restarted = create_test_state().unwrap();
    ProviderService::switch(&restarted, AppType::Codex, "manual-official").unwrap();
    assert_eq!(read_auth(), auth);
}

#[test]
fn reconciliation_leaves_managed_login_handoff_to_the_switch_engine() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let auth = native_login();
    let state = fixture(&bearer_config(), Some(&auth.to_string()));
    let mut official = state
        .db
        .get_provider_by_id("manual-official", "codex")
        .unwrap()
        .unwrap();
    official.meta = Some(
        serde_json::from_value(json!({
            "authBinding": {
                "source": "managed_account",
                "authProvider": "codex_oauth",
                "accountId": "synthetic-managed-account"
            }
        }))
        .unwrap(),
    );
    state.db.save_provider("codex", &official).unwrap();

    assert!(!reconcile_without_live_writes(&state).unwrap());
    assert_eq!(
        ProviderService::current(&state, AppType::Codex).unwrap(),
        official.id
    );
    assert_eq!(
        ProviderService::list(&state, AppType::Codex).unwrap().len(),
        1
    );
    assert_eq!(read_auth(), auth);
    assert!(!ensure_test_home()
        .join(".cc-switch/codex-login-stash.json")
        .exists());
}

#[test]
fn official_api_endpoint_does_not_reclassify_native_login_as_third_party_residue() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    for config in [
        "openai_base_url = 'https://api.openai.com/v1'".to_string(),
        CUSTOM.replace("https://relay.example/v1", "https://api.openai.com/v1"),
        CUSTOM.replace("https://relay.example/v1", "https://API.OPENAI.COM:443/v1/"),
        CUSTOM.replace(
            "https://relay.example/v1",
            "https://chatgpt.com/backend-api/codex",
        ),
        CUSTOM.replace(
            "https://relay.example/v1",
            "https://chat.openai.com/backend-api/codex",
        ),
    ] {
        let auth = json!({"auth_mode": "apikey", "OPENAI_API_KEY": "synthetic-openai-key"});
        let state = fixture(&config, Some(&auth.to_string()));
        assert!(!reconcile_without_live_writes(&state).unwrap());
        ProviderService::switch(&state, AppType::Codex, "manual-official").unwrap();
        assert_eq!(read_auth(), auth);
        assert_eq!(
            ProviderService::list(&state, AppType::Codex).unwrap().len(),
            1
        );
        assert!(!state
            .db
            .export_sql_string_for_sync()
            .unwrap()
            .contains("synthetic-openai-key"));
    }
}

#[test]
fn reuses_matching_custom_snapshot_without_overwriting_it() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let config = bearer_config();
    let state = fixture(&config, None);
    let mut saved = Provider::with_id(
        "saved-custom".into(),
        "My custom provider".into(),
        json!({"auth": {}, "config": format!("# Extra comment\n{config}")}),
        None,
    );
    saved.category = Some("custom".into());
    state.db.save_provider("codex", &saved).unwrap();
    assert!(reconcile_without_live_writes(&state).unwrap());
    assert_eq!(
        ProviderService::current(&state, AppType::Codex).unwrap(),
        saved.id
    );
    assert_eq!(
        ProviderService::list(&state, AppType::Codex).unwrap().len(),
        2
    );
    assert_eq!(
        state
            .db
            .get_provider_by_id(&saved.id, "codex")
            .unwrap()
            .unwrap()
            .settings_config,
        saved.settings_config
    );
}

#[test]
fn preserves_existing_default_and_does_not_match_a_different_account() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let state = fixture(CUSTOM, Some(r#"{"OPENAI_API_KEY":"live-key"}"#));
    let mut saved = Provider::with_id(
        "default".into(),
        "Another account".into(),
        json!({"auth": {"OPENAI_API_KEY": "another-key"}, "config": CUSTOM}),
        None,
    );
    saved.category = Some("custom".into());
    state.db.save_provider("codex", &saved).unwrap();
    assert!(reconcile_without_live_writes(&state).unwrap());
    let current = ProviderService::current(&state, AppType::Codex).unwrap();
    assert_ne!(current, "default");
    assert_ne!(current, "manual-official");
    assert_eq!(
        ProviderService::list(&state, AppType::Codex).unwrap().len(),
        3
    );
    assert_eq!(
        state
            .db
            .get_provider_by_id("default", "codex")
            .unwrap()
            .unwrap()
            .settings_config,
        saved.settings_config
    );
}

#[test]
fn official_inactive_incomplete_and_proxy_routes_are_not_imported() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let cases = [
        "",
        "model_provider = 'openai'\n",
        // Unused custom tables must not change the current official selection.
        "model_provider = 'openai'\n[model_providers.relay]\nbase_url = 'https://relay.example/v1'\n",
        "model_provider = 'custom'\n[model_providers.custom]\nname = 'OpenAI'\nrequires_openai_auth = true\nsupports_websockets = true\nwire_api = 'responses'\n",
        "model_provider = 'relay'\n",
        "model_provider = 'relay'\n[model_providers.relay]\nbase_url = 'invalid'\n",
        "model_provider = 'cc-switch-official'\n[model_providers.cc-switch-official]\nbase_url = 'http://127.0.0.1:15721/v1'\n",
        "model_provider = 'custom'\n[model_providers.custom]\nbase_url = 'http://127.0.0.1:15721/v1'\nrequires_openai_auth = true\n",
        "profile = 'official'\nmodel_provider = 'relay'\n[profiles.official]\nmodel_provider = 'openai'\n[model_providers.relay]\nbase_url = 'https://relay.example/v1'\n",
    ];
    for config in cases {
        let state = fixture(config, None);
        assert!(!reconcile_without_live_writes(&state).unwrap(), "{config}");
        assert_eq!(
            ProviderService::current(&state, AppType::Codex).unwrap(),
            "manual-official"
        );
        assert_eq!(
            ProviderService::list(&state, AppType::Codex).unwrap().len(),
            1
        );
    }
}

#[test]
fn proxy_mode_including_detached_and_legacy_modes_is_not_reconciled() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    for mode in [
        json!({"mode": "proxy", "attached": false, "proxy_route": "manual-official"}),
        json!({"mode": "direct", "attached": true}),
        json!({}),
    ] {
        let state = fixture(CUSTOM, None);
        fs::write(
            ensure_test_home().join(".cc-switch/live-state.json"),
            json!({"version": 1, "apps": {"codex": mode}}).to_string(),
        )
        .unwrap();
        state.db.set_proxy_flags_sync("codex", true, false).unwrap();
        assert!(!reconcile_without_live_writes(&state).unwrap());
        assert_eq!(
            state.db.get_current_provider("codex").unwrap().as_deref(),
            Some("manual-official")
        );
        assert_eq!(
            ProviderService::list(&state, AppType::Codex).unwrap().len(),
            1
        );
    }
}

#[test]
fn malformed_or_missing_live_config_does_not_change_selection() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let state = fixture("invalid = [", None);
    assert!(reconcile_without_live_writes(&state).is_err());
    fs::remove_file(get_codex_config_path()).unwrap();
    assert!(reconcile_without_live_writes(&state).is_err());
    assert_eq!(
        ProviderService::current(&state, AppType::Codex).unwrap(),
        "manual-official"
    );
    assert_eq!(
        ProviderService::list(&state, AppType::Codex).unwrap().len(),
        1
    );
}

#[test]
fn native_oauth_never_enters_provider_rows_or_sync_exports() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let auth = json!({
        "auth_mode": "chatgpt",
        "tokens": {
            "access_token": "synthetic-native-access",
            "refresh_token": "synthetic-native-refresh",
            "account_id": "synthetic-native-account"
        },
        "OPENAI_API_KEY": "unrelated-native-key"
    });
    let state = fixture(&bearer_config(), Some(&auth.to_string()));
    assert!(reconcile_without_live_writes(&state).unwrap());
    let row = state
        .db
        .get_provider_by_id("default", "codex")
        .unwrap()
        .unwrap();
    assert_eq!(row.settings_config["auth"], json!({}));
    let export = state.db.export_sql_string_for_sync().unwrap();
    for secret in [
        "synthetic-native-access",
        "synthetic-native-refresh",
        "synthetic-native-account",
        "unrelated-native-key",
    ] {
        assert!(!export.contains(secret));
    }

    // Native token rotation does not change the identity of the custom route.
    fs::write(
        get_codex_auth_path(),
        r#"{"tokens":{"access_token":"rotated-native-access"}}"#,
    )
    .unwrap();
    update_settings(AppSettings {
        current_provider_codex: Some("manual-official".into()),
        ..Default::default()
    })
    .unwrap();
    assert!(reconcile_without_live_writes(&state).unwrap());
    assert_eq!(
        ProviderService::list(&state, AppType::Codex).unwrap().len(),
        2
    );
    assert!(!state
        .db
        .export_sql_string_for_sync()
        .unwrap()
        .contains("rotated-native-access"));
}

#[test]
fn native_api_key_is_not_imported_when_route_has_its_own_credentials() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    for credential in [
        "experimental_bearer_token = 'route-key'",
        "env_key = 'RELAY_API_KEY'",
    ] {
        let config = CUSTOM.replace(
            "requires_openai_auth = true",
            &format!("requires_openai_auth = true\n{credential}"),
        );
        let state = fixture(
            &config,
            Some(r#"{"OPENAI_API_KEY":"unrelated-native-key"}"#),
        );
        assert!(reconcile_without_live_writes(&state).unwrap());
        let row = state
            .db
            .get_provider_by_id("default", "codex")
            .unwrap()
            .unwrap();
        assert_eq!(row.settings_config["auth"], json!({}));
        assert!(!state
            .db
            .export_sql_string_for_sync()
            .unwrap()
            .contains("unrelated-native-key"));
    }
}

#[test]
fn keyless_official_auth_fallback_is_not_adopted_as_a_custom_provider() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    for auth in [None, Some(r#"{"tokens":{"access_token":"native-login"}}"#)] {
        let state = fixture(CUSTOM, auth);
        assert!(!reconcile_without_live_writes(&state).unwrap());
        assert_eq!(
            ProviderService::current(&state, AppType::Codex).unwrap(),
            "manual-official"
        );
        assert_eq!(
            ProviderService::list(&state, AppType::Codex).unwrap().len(),
            1
        );
    }
}

#[test]
fn reuses_the_existing_provider_after_a_real_switch_normalizes_live() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let state = fixture(CUSTOM, Some(LEGACY_AUTH));
    let mut row = Provider::with_id(
        "saved-relay".into(),
        "Existing named provider".into(),
        json!({"auth":{"OPENAI_API_KEY":"local-key"},"config":CUSTOM}),
        None,
    );
    row.category = Some("custom".into());
    row.notes = Some("Keep my provider metadata".into());
    state.db.save_provider("codex", &row).unwrap();
    ProviderService::switch(&state, AppType::Codex, &row.id).unwrap();
    let live = fs::read_to_string(get_codex_config_path()).unwrap();
    assert!(live.contains("model_provider = \"custom\""));
    assert!(live.contains("experimental_bearer_token"));
    assert!(!get_codex_auth_path().exists());
    // Unrelated client edits must not defeat matching either.
    fs::write(
        get_codex_config_path(),
        format!("# Client comment\n{live}\n[mcp_servers.another]\ncommand = 'echo'\n"),
    )
    .unwrap();
    state
        .db
        .set_current_provider("codex", "manual-official")
        .unwrap();
    update_settings(AppSettings {
        current_provider_codex: Some("manual-official".into()),
        ..Default::default()
    })
    .unwrap();
    assert!(reconcile_without_live_writes(&state).unwrap());
    assert_eq!(
        ProviderService::current(&state, AppType::Codex).unwrap(),
        "saved-relay"
    );
    assert_eq!(
        ProviderService::list(&state, AppType::Codex).unwrap().len(),
        2
    );
    let saved = state
        .db
        .get_provider_by_id(&row.id, "codex")
        .unwrap()
        .unwrap();
    assert_eq!(saved.settings_config, row.settings_config);
    assert_eq!(saved.notes, row.notes);
}

#[test]
fn profile_route_overrides_are_left_external_instead_of_creating_unusable_cards() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let config = format!(
        "profile = 'work'\n{}\n[profiles.work]\nmodel_provider = 'relay'\n",
        bearer_config().replace("model_provider = \"relay\"", "model_provider = 'openai'")
    );
    let state = fixture(&config, None);
    assert!(!reconcile_without_live_writes(&state).unwrap());
    assert_eq!(
        ProviderService::list(&state, AppType::Codex).unwrap().len(),
        1
    );
    assert_eq!(
        ProviderService::current(&state, AppType::Codex).unwrap(),
        "manual-official"
    );
}

#[test]
fn profile_and_top_level_proxy_routes_are_never_imported() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let host = [127, 0, 0, 1].map(|part| part.to_string()).join(".");
    for profile in [false, true] {
        for credential in ["", "experimental_bearer_token = 'PROXY_MANAGED'\n"] {
            let selector = if profile {
                "profile = 'work'\nmodel_provider = 'openai'\n[profiles.work]\nmodel_provider = 'relay'\n"
            } else {
                "model_provider = 'relay'\n"
            };
            let config = format!("{selector}[model_providers.relay]\nname = 'Relay'\nbase_url = 'http://{host}:15721/v1'\nwire_api = 'responses'\n{credential}");
            let state = fixture(&config, None);
            assert!(!reconcile_without_live_writes(&state).unwrap());
            assert_eq!(
                ProviderService::list(&state, AppType::Codex).unwrap().len(),
                1
            );
        }
    }
}

#[test]
fn settings_write_failure_rolls_back_new_row_and_db_selection() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let state = fixture(CUSTOM, Some(LEGACY_AUTH));
    let path = ensure_test_home().join(".cc-switch/settings.json");
    fs::rename(&path, path.with_extension("saved.json")).unwrap();
    fs::create_dir(&path).unwrap();
    assert!(reconcile_without_live_writes(&state).is_err());
    assert_eq!(
        state.db.get_current_provider("codex").unwrap().as_deref(),
        Some("manual-official")
    );
    assert_eq!(
        ProviderService::current(&state, AppType::Codex).unwrap(),
        "manual-official"
    );
    assert_eq!(
        ProviderService::list(&state, AppType::Codex).unwrap().len(),
        1
    );
}

#[test]
fn database_failure_preserves_selection_before_and_after_local_publish() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    for fail_at_commit in [false, true] {
        let state = fixture(CUSTOM, Some(LEGACY_AUTH));
        let settings_path = ensure_test_home().join(".cc-switch/settings.json");
        let before = fs::read(&settings_path).unwrap();
        let db_path = ensure_test_home().join(".cc-switch/cc-switch.db");
        let conn = rusqlite::Connection::open(db_path).unwrap();
        if fail_at_commit {
            conn.execute_batch("CREATE TABLE review_parent(id TEXT PRIMARY KEY);
                CREATE TABLE review_child(parent_id TEXT REFERENCES review_parent(id) DEFERRABLE INITIALLY DEFERRED);
                CREATE TRIGGER review_fail AFTER UPDATE OF is_current ON providers WHEN NEW.is_current = 1
                BEGIN INSERT INTO review_child VALUES ('missing'); END;").unwrap();
        } else {
            conn.execute_batch("CREATE TRIGGER review_fail BEFORE UPDATE OF is_current ON providers WHEN NEW.is_current = 1
                BEGIN SELECT RAISE(ABORT, 'synthetic selection write failure'); END;").unwrap();
        }
        assert!(reconcile_without_live_writes(&state).is_err());
        assert_eq!(fs::read(&settings_path).unwrap(), before);
        assert_eq!(
            state.db.get_current_provider("codex").unwrap().as_deref(),
            Some("manual-official")
        );
        assert_eq!(
            ProviderService::current(&state, AppType::Codex).unwrap(),
            "manual-official"
        );
        assert_eq!(
            ProviderService::list(&state, AppType::Codex).unwrap().len(),
            1
        );
    }
}

#[test]
fn ambiguous_matches_do_not_pick_an_arbitrary_provider() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    for same_model in [false, true] {
        let state = fixture(CUSTOM, Some(LEGACY_AUTH));
        for id in ["first", "second"] {
            let config = if same_model {
                CUSTOM.to_string()
            } else {
                CUSTOM.replace("custom-model", id)
            };
            let mut row = Provider::with_id(
                id.into(),
                id.into(),
                json!({"auth":{"OPENAI_API_KEY":"local-key"}, "config": config}),
                None,
            );
            row.category = Some("custom".into());
            state.db.save_provider("codex", &row).unwrap();
        }
        assert!(!reconcile_without_live_writes(&state).unwrap());
        assert_eq!(
            ProviderService::current(&state, AppType::Codex).unwrap(),
            "manual-official"
        );
        assert_eq!(
            ProviderService::list(&state, AppType::Codex).unwrap().len(),
            3
        );
    }
}
