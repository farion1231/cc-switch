//! Regression coverage for the distinction between a native-config readback
//! and an explicit user/synchronization write.
//!
//! These tests always use `support`'s isolated HOME. They intentionally use
//! synthetic credentials and never start a desktop client.

use serde_json::json;

use cc_switch_lib::{
    api_key_vars::{
        self,
        runner::{ApiKeyRefreshConfig, RefreshOutput, ScriptRun},
    },
    AppType, EditorSave, Provider, ProviderMeta, ProviderService,
};

#[path = "support.rs"]
mod support;
use support::{create_test_state, ensure_test_home, reset_test_fs, test_mutex};

const OLD: &str = "PR_REVIEW_OLD";
const DISABLED: &str = "PR_REVIEW_DISABLED";
const LITERAL: &str = "pr-review-explicit-literal";

fn config(enabled: bool) -> ApiKeyRefreshConfig {
    ApiKeyRefreshConfig {
        enabled,
        script_dir: String::new(),
        timeout_secs: None,
        lead_time_secs: None,
    }
}

fn seed_value(state: &cc_switch_lib::AppState, name: &str, value: &str) {
    state
        .db
        .upsert_api_key_var(None, name, &config(true))
        .expect("create enabled variable");
    api_key_vars::commit_result(
        state,
        name,
        Ok(ScriptRun {
            output: RefreshOutput {
                api_key: value.into(),
                expires_at: None,
                refresh_after: None,
                message: None,
            },
            resolved_file: "index.mjs".into(),
        }),
    )
    .expect("seed cached variable value");
}

fn editor_save(base: &serde_json::Value) -> EditorSave {
    serde_json::from_value(json!({"base": base, "onConflict": "refuse"})).expect("editor save")
}

fn claude(id: &str, key: &str) -> Provider {
    let mut provider = Provider::with_id(
        id.into(),
        "Original Claude".into(),
        json!({"env": {
            "ANTHROPIC_BASE_URL": "https://claude.example.test",
            "ANTHROPIC_AUTH_TOKEN": key,
            "ANTHROPIC_MODEL": "claude-test"
        }}),
        None,
    );
    provider.notes = Some("must survive a failed editor save".into());
    provider.meta = Some(ProviderMeta {
        common_config_enabled: Some(false),
        ..Default::default()
    });
    provider
}

fn child_key(state: &cc_switch_lib::AppState, app: AppType, id: &str) -> String {
    let row = state
        .db
        .get_provider_by_id(id, app.as_str())
        .expect("read generated child")
        .expect("generated child exists");
    match app {
        AppType::Claude => row.settings_config["env"]["ANTHROPIC_AUTH_TOKEN"]
            .as_str()
            .expect("Claude key")
            .to_string(),
        AppType::Codex => row.settings_config["auth"]["OPENAI_API_KEY"]
            .as_str()
            .expect("Codex key")
            .to_string(),
        AppType::Gemini => row.settings_config["env"]["GEMINI_API_KEY"]
            .as_str()
            .expect("Gemini key")
            .to_string(),
        _ => unreachable!("only universal child applications are asserted"),
    }
}

#[test]
fn editor_rejects_disabled_reference_without_mutating_existing_row() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    ensure_test_home();
    reset_test_fs();
    let state = create_test_state().expect("state");

    state
        .db
        .upsert_api_key_var(None, DISABLED, &config(false))
        .expect("create disabled variable");
    let initial = claude("rollback-disabled", "literal-before-editor-failure");
    let initial_base =
        ProviderService::editor_view(&state, AppType::Claude, &initial.settings_config, None)
            .expect("initial editor view")
            .settings;
    ProviderService::add_from_editor(
        &state,
        AppType::Claude,
        initial.clone(),
        true,
        Some(editor_save(&initial_base)),
    )
    .expect("add current Claude provider");

    let before = state
        .db
        .get_provider_by_id(&initial.id, AppType::Claude.as_str())
        .expect("read original")
        .expect("original exists");
    let base = ProviderService::editor_view(&state, AppType::Claude, &before.settings_config, None)
        .expect("editor view")
        .settings;
    let mut edited = before.clone();
    edited.settings_config = base.clone();
    edited.name = "Changed Claude".into();
    edited.notes = Some("changed note must roll back".into());
    edited
        .meta
        .as_mut()
        .expect("seeded metadata")
        .common_config_enabled = Some(true);
    edited.settings_config["env"]["ANTHROPIC_AUTH_TOKEN"] = json!(format!("{{{{{DISABLED}}}}}"));

    assert!(
        ProviderService::update_from_editor(
            &state,
            AppType::Claude,
            None,
            edited,
            Some(editor_save(&base)),
        )
        .is_err(),
        "disabled live reference must be rejected"
    );

    let after = state
        .db
        .get_provider_by_id(&before.id, AppType::Claude.as_str())
        .expect("read restored row")
        .expect("restored row exists");
    assert_eq!(after.settings_config, before.settings_config);
    assert_eq!(after.name, before.name);
    assert_eq!(after.notes, before.notes);
    assert_eq!(
        serde_json::to_value(&after.meta).expect("serialize restored metadata"),
        serde_json::to_value(&before.meta).expect("serialize original metadata")
    );
    let live = std::fs::read_to_string(cc_switch_lib::get_claude_settings_path())
        .expect("read unchanged live config");
    assert!(live.contains("literal-before-editor-failure"), "{live}");
    assert!(!live.contains(DISABLED), "{live}");
}

#[test]
fn editor_file_failure_restores_full_row_after_valid_variable_write() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    ensure_test_home();
    reset_test_fs();
    let state = create_test_state().expect("state");
    seed_value(&state, OLD, LITERAL);

    let initial = claude("rollback-file", "literal-before-file-failure");
    let initial_base =
        ProviderService::editor_view(&state, AppType::Claude, &initial.settings_config, None)
            .expect("initial editor view")
            .settings;
    ProviderService::add_from_editor(
        &state,
        AppType::Claude,
        initial.clone(),
        true,
        Some(editor_save(&initial_base)),
    )
    .expect("add current Claude provider");
    let before = state
        .db
        .get_provider_by_id(&initial.id, AppType::Claude.as_str())
        .expect("read original")
        .expect("original exists");
    let base = ProviderService::editor_view(&state, AppType::Claude, &before.settings_config, None)
        .expect("editor view")
        .settings;
    let mut edited = before.clone();
    edited.settings_config = base.clone();
    edited.name = "Changed Claude".into();
    edited.notes = Some("changed note must roll back".into());
    edited
        .meta
        .as_mut()
        .expect("seeded metadata")
        .common_config_enabled = Some(true);
    edited.settings_config["env"]["ANTHROPIC_AUTH_TOKEN"] = json!(format!("{{{{{OLD}}}}}"));

    // A malformed external edit makes the real live writer fail after the row
    // was staged. Unlike a disabled reference, this exercises the rollback
    // path rather than a preflight validation branch.
    let live_path = cc_switch_lib::get_claude_settings_path();
    std::fs::write(&live_path, b"{not valid JSON").expect("damage isolated live config");
    assert!(
        ProviderService::update_from_editor(
            &state,
            AppType::Claude,
            None,
            edited,
            Some(editor_save(&base)),
        )
        .is_err(),
        "malformed live config must fail the real writer"
    );

    let after = state
        .db
        .get_provider_by_id(&before.id, AppType::Claude.as_str())
        .expect("read restored row")
        .expect("restored row exists");
    assert_eq!(after.settings_config, before.settings_config);
    assert_eq!(after.name, before.name);
    assert_eq!(after.notes, before.notes);
    assert_eq!(
        serde_json::to_value(&after.meta).expect("serialize restored metadata"),
        serde_json::to_value(&before.meta).expect("serialize original metadata")
    );
    assert_eq!(
        std::fs::read_to_string(&live_path).expect("read unchanged failed live config"),
        "{not valid JSON"
    );
}

#[test]
fn universal_explicit_literal_detaches_old_template_in_all_children() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    ensure_test_home();
    reset_test_fs();
    let state = create_test_state().expect("state");
    seed_value(&state, OLD, LITERAL);

    ProviderService::upsert_universal(
        &state,
        serde_json::from_value(json!({
            "id": "explicit-literal",
            "name": "Shared",
            "providerType": "custom",
            "baseUrl": "https://api.example.test",
            "apiKey": format!("{{{{{OLD}}}}}"),
            "apps": {"claude": true, "codex": true, "gemini": true},
            "models": {}
        }))
        .expect("construct universal provider from its public JSON shape"),
    )
    .expect("save template parent");
    let mut universal = ProviderService::get_universal(&state, "explicit-literal")
        .expect("read template parent")
        .expect("template parent exists");
    ProviderService::sync_universal_to_apps(&state, &universal.id).expect("create children");

    for (app, id) in [
        (AppType::Claude, "universal-claude-explicit-literal"),
        (AppType::Codex, "universal-codex-explicit-literal"),
        (AppType::Gemini, "universal-gemini-explicit-literal"),
    ] {
        ProviderService::switch(&state, app, id).expect("make generated child current");
    }

    // A synchronization is an explicit parent-authoritative write. The
    // literal intentionally equals OLD's cached value, which distinguishes it
    // from a native readback and catches accidental template preservation.
    universal.api_key = LITERAL.into();
    ProviderService::upsert_universal(&state, universal.clone()).expect("save literal parent");
    ProviderService::sync_universal_to_apps(&state, &universal.id).expect("sync literal children");

    for (app, id) in [
        (AppType::Claude, "universal-claude-explicit-literal"),
        (AppType::Codex, "universal-codex-explicit-literal"),
        (AppType::Gemini, "universal-gemini-explicit-literal"),
    ] {
        assert_eq!(child_key(&state, app, id), LITERAL, "{id}");
    }
    for path in [
        cc_switch_lib::get_claude_settings_path(),
        // Third-party Codex credentials live in its active model-provider
        // table. With default login preservation disabled, auth.json is
        // intentionally removed while config.toml carries the API key.
        cc_switch_lib::get_codex_config_path(),
        ensure_test_home().join(".gemini/.env"),
    ] {
        let live = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
        assert!(live.contains(LITERAL), "{}: {live}", path.display());
        assert!(!live.contains(OLD), "{}: {live}", path.display());
    }
}
