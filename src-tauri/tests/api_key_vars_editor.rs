//! Editor-save regression coverage for API-key variable templates.
//!
//! These tests use the real provider service and its live-file projections, but
//! always run under the isolated test HOME from `support`.

use serde_json::{json, Value};

use cc_switch_lib::api_key_vars::{
    self,
    runner::{ApiKeyRefreshConfig, RefreshOutput, ScriptRun},
};
use cc_switch_lib::{AppType, EditorSave, Provider, ProviderService};

#[path = "support.rs"]
mod support;
use support::{create_test_state, ensure_test_home, reset_test_fs, test_mutex};

const VAR_A: &str = "IT_EDITOR_VAR_A";
const VAR_B: &str = "IT_EDITOR_VAR_B";
const VALUE_A: &str = "test-editor-value-a";
const VALUE_B: &str = "test-editor-value-b";

fn refresh_config() -> ApiKeyRefreshConfig {
    ApiKeyRefreshConfig {
        enabled: true,
        script_dir: String::new(),
        timeout_secs: None,
        lead_time_secs: None,
    }
}

fn success(value: &str) -> Result<ScriptRun, api_key_vars::runner::RunFailure> {
    Ok(ScriptRun {
        output: RefreshOutput {
            api_key: value.to_string(),
            expires_at: None,
            refresh_after: None,
            message: None,
        },
        resolved_file: "index.mjs".into(),
    })
}

fn editor_save(base: &Value) -> EditorSave {
    serde_json::from_value(json!({ "base": base, "onConflict": "refuse" })).expect("editor save")
}

fn editor_save_with_draft(base: &Value, draft: &Value) -> EditorSave {
    serde_json::from_value(json!({
        "base": base,
        "draft": draft,
        "onConflict": "refuse"
    }))
    .expect("editor save")
}

fn provider(id: &str, settings: Value) -> Provider {
    Provider::with_id(id.into(), id.into(), settings, None)
}

fn row(state: &cc_switch_lib::AppState, app: AppType, id: &str) -> Provider {
    state
        .db
        .get_provider_by_id(id, app.as_str())
        .expect("read provider")
        .expect("provider exists")
}

fn read(path: std::path::PathBuf) -> String {
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
}

#[test]
fn editor_saves_variable_templates_and_only_projects_resolved_values_to_live_files() {
    let _guard = test_mutex()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let home = ensure_test_home();
    reset_test_fs();

    // Every path asserted below must stay inside the isolated test home. This
    // is the guard against touching a developer's real Codex configuration.
    for path in [
        cc_switch_lib::get_claude_settings_path(),
        cc_switch_lib::get_codex_config_path(),
        cc_switch_lib::get_grok_config_path(),
        home.join(".gemini/.env"),
        home.join(".gemini/settings.json"),
    ] {
        assert!(
            path.starts_with(home),
            "outside test HOME: {}",
            path.display()
        );
    }

    let state = create_test_state().expect("state");
    for name in [VAR_A, VAR_B] {
        state
            .db
            .upsert_api_key_var(None, name, &refresh_config())
            .expect("create variable");
    }
    api_key_vars::commit_result(&state, VAR_A, success(VALUE_A)).expect("seed variable");
    api_key_vars::commit_result(&state, VAR_B, success(VALUE_B)).expect("seed variable");

    let claude_plain = json!({"env": {
        "ANTHROPIC_AUTH_TOKEN": "plain-claude",
        "ANTHROPIC_BASE_URL": "https://claude.example"
    }});
    let claude_base = ProviderService::editor_view(&state, AppType::Claude, &claude_plain, None)
        .expect("Claude editor view")
        .settings;
    ProviderService::add_from_editor(
        &state,
        AppType::Claude,
        provider("claude", claude_plain.clone()),
        true,
        Some(editor_save(&claude_base)),
    )
    .expect("add Claude from editor");

    let codex_plain = json!({
        "auth": {"OPENAI_API_KEY": "plain-codex"},
        "config": "model_provider = \"relay\"\n[model_providers.relay]\nname = \"Relay\"\nbase_url = \"https://codex.example/v1\"\nwire_api = \"responses\"\n"
    });
    let codex_base = ProviderService::editor_view(&state, AppType::Codex, &codex_plain, None)
        .expect("Codex editor view")
        .settings;
    ProviderService::add_from_editor(
        &state,
        AppType::Codex,
        provider("codex", codex_plain.clone()),
        true,
        Some(editor_save(&codex_base)),
    )
    .expect("add Codex from editor");

    let gemini_plain = json!({"env": {
        "GEMINI_API_KEY": "plain-gemini",
        "GOOGLE_GEMINI_BASE_URL": "https://gemini.example"
    }});
    let gemini_base = ProviderService::editor_view(&state, AppType::Gemini, &gemini_plain, None)
        .expect("Gemini editor view")
        .settings;
    ProviderService::add_from_editor(
        &state,
        AppType::Gemini,
        provider("gemini", gemini_plain.clone()),
        true,
        Some(editor_save(&gemini_base)),
    )
    .expect("add Gemini from editor");

    let grok_plain = json!({"config": "[models]\ndefault = \"grok-test\"\n\n[model.\"grok-test\"]\nmodel = \"grok-test\"\nname = \"Test\"\nbase_url = \"https://grok.example/v1\"\napi_key = \"plain-grok\"\napi_backend = \"responses\"\ncontext_window = 500000\n"});
    let grok_base = ProviderService::editor_view(&state, AppType::GrokBuild, &grok_plain, None)
        .expect("Grok editor view")
        .settings;
    ProviderService::add_from_editor(
        &state,
        AppType::GrokBuild,
        provider("grok", grok_plain.clone()),
        true,
        Some(editor_save(&grok_base)),
    )
    .expect("add Grok from editor");

    // Open each persisted editor view, convert a plain key to a template, and
    // save via the actual update endpoint. Claude/Grok then change to a second
    // reference, covering the old-resolved-value restoration regression.
    let claude_first = row(&state, AppType::Claude, "claude");
    let claude_first_base =
        ProviderService::editor_view(&state, AppType::Claude, &claude_first.settings_config, None)
            .expect("Claude persisted editor view")
            .settings;
    let mut claude_first_edit = claude_first_base.clone();
    claude_first_edit["env"]["ANTHROPIC_AUTH_TOKEN"] = json!(format!("{{{{{VAR_A}}}}}"));
    let mut claude_updated = claude_first.clone();
    claude_updated.settings_config = claude_first_edit;
    ProviderService::update_from_editor(
        &state,
        AppType::Claude,
        None,
        claude_updated,
        Some(editor_save(&claude_first_base)),
    )
    .expect("save Claude variable ref");
    let claude_second = row(&state, AppType::Claude, "claude");
    let claude_second_base = ProviderService::editor_view(
        &state,
        AppType::Claude,
        &claude_second.settings_config,
        None,
    )
    .expect("Claude template remains visible")
    .settings;
    assert_eq!(
        claude_second_base["env"]["ANTHROPIC_AUTH_TOKEN"],
        json!(format!("{{{{{VAR_A}}}}}"))
    );
    let mut claude_second_edit = claude_second_base.clone();
    claude_second_edit["env"]["ANTHROPIC_AUTH_TOKEN"] = json!(format!("{{{{{VAR_B}}}}}"));
    let mut claude_updated = claude_second;
    claude_updated.settings_config = claude_second_edit;
    ProviderService::update_from_editor(
        &state,
        AppType::Claude,
        None,
        claude_updated,
        Some(editor_save(&claude_second_base)),
    )
    .expect("replace Claude variable ref");

    let codex_saved = row(&state, AppType::Codex, "codex");
    let codex_base =
        ProviderService::editor_view(&state, AppType::Codex, &codex_saved.settings_config, None)
            .expect("Codex persisted editor view")
            .settings;
    let mut codex_edit = codex_base.clone();
    codex_edit["auth"]["OPENAI_API_KEY"] = json!(format!("{{{{{VAR_A}}}}}"));
    let mut codex_updated = codex_saved;
    codex_updated.settings_config = codex_edit;
    ProviderService::update_from_editor(
        &state,
        AppType::Codex,
        None,
        codex_updated,
        Some(editor_save(&codex_base)),
    )
    .expect("save Codex variable ref");
    let codex_shown = ProviderService::editor_view(
        &state,
        AppType::Codex,
        &row(&state, AppType::Codex, "codex").settings_config,
        None,
    )
    .expect("Codex template remains visible")
    .settings;
    assert_eq!(
        codex_shown["auth"]["OPENAI_API_KEY"],
        json!(format!("{{{{{VAR_A}}}}}"))
    );

    let gemini_saved = row(&state, AppType::Gemini, "gemini");
    let gemini_base =
        ProviderService::editor_view(&state, AppType::Gemini, &gemini_saved.settings_config, None)
            .expect("Gemini persisted editor view")
            .settings;
    let mut gemini_edit = gemini_base.clone();
    gemini_edit["env"]["GEMINI_API_KEY"] = json!(format!("{{{{{VAR_A}}}}}"));
    let mut gemini_updated = gemini_saved;
    gemini_updated.settings_config = gemini_edit;
    ProviderService::update_from_editor(
        &state,
        AppType::Gemini,
        None,
        gemini_updated,
        Some(editor_save(&gemini_base)),
    )
    .expect("save Gemini variable ref");
    let gemini_shown = ProviderService::editor_view(
        &state,
        AppType::Gemini,
        &row(&state, AppType::Gemini, "gemini").settings_config,
        None,
    )
    .expect("Gemini template remains visible")
    .settings;
    assert_eq!(
        gemini_shown["env"]["GEMINI_API_KEY"],
        json!(format!("{{{{{VAR_A}}}}}"))
    );

    let grok_first = row(&state, AppType::GrokBuild, "grok");
    let grok_first_base = ProviderService::editor_view(
        &state,
        AppType::GrokBuild,
        &grok_first.settings_config,
        None,
    )
    .expect("Grok persisted editor view")
    .settings;
    let mut grok_first_edit = grok_first_base.clone();
    let grok_first_text = grok_first_edit["config"]
        .as_str()
        .expect("Grok TOML")
        .to_string();
    grok_first_edit["config"] =
        json!(grok_first_text.replace("plain-grok", &format!("{{{{{VAR_A}}}}}")));
    let mut grok_updated = grok_first.clone();
    grok_updated.settings_config = grok_first_edit;
    ProviderService::update_from_editor(
        &state,
        AppType::GrokBuild,
        None,
        grok_updated,
        Some(editor_save(&grok_first_base)),
    )
    .expect("save Grok variable ref");
    let grok_second = row(&state, AppType::GrokBuild, "grok");
    let grok_second_base = ProviderService::editor_view(
        &state,
        AppType::GrokBuild,
        &grok_second.settings_config,
        None,
    )
    .expect("Grok template remains visible")
    .settings;
    assert!(grok_second_base["config"]
        .as_str()
        .expect("Grok TOML")
        .contains(&format!("{{{{{VAR_A}}}}}")));
    let mut grok_second_edit = grok_second_base.clone();
    let grok_second_text = grok_second_edit["config"]
        .as_str()
        .expect("Grok TOML")
        .to_string();
    grok_second_edit["config"] =
        json!(grok_second_text.replace(&format!("{{{{{VAR_A}}}}}"), &format!("{{{{{VAR_B}}}}}")));
    let mut grok_updated = grok_second;
    grok_updated.settings_config = grok_second_edit;
    ProviderService::update_from_editor(
        &state,
        AppType::GrokBuild,
        None,
        grok_updated,
        Some(editor_save(&grok_second_base)),
    )
    .expect("replace Grok variable ref");

    for (app, id, expected) in [
        (AppType::Claude, "claude", format!("{{{{{VAR_B}}}}}")),
        (AppType::Codex, "codex", format!("{{{{{VAR_A}}}}}")),
        (AppType::Gemini, "gemini", format!("{{{{{VAR_A}}}}}")),
        (AppType::GrokBuild, "grok", format!("{{{{{VAR_B}}}}}")),
    ] {
        let stored = row(&state, app.clone(), id);
        let text = stored.settings_config.to_string();
        assert!(
            text.contains(&expected),
            "{app:?} row lost its template: {text}"
        );
        assert!(
            !text.contains(VALUE_A) && !text.contains(VALUE_B),
            "{app:?} row persisted resolved value: {text}"
        );
        ProviderService::switch(&state, app, id).expect("switch resolved provider");
    }

    assert!(read(cc_switch_lib::get_claude_settings_path()).contains(VALUE_B));
    assert!(read(cc_switch_lib::get_codex_config_path()).contains(VALUE_A));
    assert!(read(home.join(".gemini/.env")).contains(VALUE_A));
    assert!(read(cc_switch_lib::get_grok_config_path()).contains(VALUE_B));

    // A refresh reprojects the selected rows to live files without changing the
    // rows. The values used here are synthetic test-only secrets.
    api_key_vars::commit_result(&state, VAR_A, success("test-editor-value-a2"))
        .expect("refresh variable");
    assert!(read(cc_switch_lib::get_codex_config_path()).contains("test-editor-value-a2"));
    assert!(read(home.join(".gemini/.env")).contains("test-editor-value-a2"));
    let codex_row = row(&state, AppType::Codex, "codex")
        .settings_config
        .to_string();
    assert!(codex_row.contains(&format!("{{{{{VAR_A}}}}}")));
    assert!(
        !codex_row.contains("experimental_bearer_token"),
        "Codex's live-only injected token leaked into the provider row: {codex_row}"
    );
    assert!(!codex_row.contains("test-editor-value-a2"), "{codex_row}");
    let export = state.db.export_sql_string().expect("export");
    assert!(!export.contains(VALUE_A));
    assert!(!export.contains(VALUE_B));
    assert!(!export.contains("test-editor-value-a2"));
    assert!(!export.contains("experimental_bearer_token"));
}

#[test]
fn editor_adds_variable_template_providers_without_persisting_the_resolved_value() {
    let _guard = test_mutex()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let home = ensure_test_home();
    reset_test_fs();
    let state = create_test_state().expect("state");
    state
        .db
        .upsert_api_key_var(None, VAR_A, &refresh_config())
        .expect("create variable");
    api_key_vars::commit_result(&state, VAR_A, success(VALUE_A)).expect("seed variable");

    let cases = vec![
        (
            AppType::Claude,
            "new-claude",
            json!({"env": {"ANTHROPIC_AUTH_TOKEN": format!("{{{{{VAR_A}}}}}"), "ANTHROPIC_BASE_URL": "https://claude.example"}}),
            cc_switch_lib::get_claude_settings_path(),
        ),
        (
            AppType::Codex,
            "new-codex",
            json!({"auth": {"OPENAI_API_KEY": format!("{{{{{VAR_A}}}}}")}, "config": "model_provider = \"relay\"\n[model_providers.relay]\nname = \"Relay\"\nbase_url = \"https://codex.example/v1\"\nwire_api = \"responses\"\n"}),
            cc_switch_lib::get_codex_config_path(),
        ),
        (
            AppType::Gemini,
            "new-gemini",
            json!({"env": {"GEMINI_API_KEY": format!("{{{{{VAR_A}}}}}"), "GOOGLE_GEMINI_BASE_URL": "https://gemini.example"}}),
            home.join(".gemini/.env"),
        ),
        (
            AppType::GrokBuild,
            "new-grok",
            json!({"config": format!("[models]\ndefault = \"grok-test\"\n\n[model.\"grok-test\"]\nmodel = \"grok-test\"\nname = \"Test\"\nbase_url = \"https://grok.example/v1\"\napi_key = \"{{{{{VAR_A}}}}}\"\napi_backend = \"responses\"\ncontext_window = 500000\n")}),
            cc_switch_lib::get_grok_config_path(),
        ),
    ];

    for (app, id, raw, live_path) in cases {
        assert!(
            live_path.starts_with(home),
            "outside test HOME: {}",
            live_path.display()
        );
        let base = ProviderService::editor_view(&state, app.clone(), &raw, None)
            .expect("new-provider editor view")
            .settings;
        ProviderService::add_from_editor(
            &state,
            app.clone(),
            provider(id, raw.clone()),
            true,
            Some(editor_save_with_draft(&base, &raw)),
        )
        .expect("add template provider from editor");
        let stored_provider = row(&state, app.clone(), id);
        let stored = stored_provider.settings_config.to_string();
        assert!(
            stored.contains(&format!("{{{{{VAR_A}}}}}")),
            "{app:?}: {stored}"
        );
        assert!(!stored.contains(VALUE_A), "{app:?}: {stored}");
        let shown =
            ProviderService::editor_view(&state, app, &stored_provider.settings_config, None)
                .expect("stored editor view")
                .settings
                .to_string();
        assert!(shown.contains(&format!("{{{{{VAR_A}}}}}")), "{shown}");
        assert!(!shown.contains(VALUE_A), "{shown}");
        assert!(read(live_path).contains(VALUE_A));
    }

    let export = state.db.export_sql_string().expect("export");
    assert!(!export.contains(VALUE_A), "{export}");
}

#[test]
fn explicit_user_key_edits_detach_current_and_historical_variable_values() {
    let _guard = test_mutex()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    ensure_test_home();
    reset_test_fs();
    let state = create_test_state().expect("state");
    state
        .db
        .upsert_api_key_var(None, VAR_A, &refresh_config())
        .expect("create variable");
    api_key_vars::commit_result(&state, VAR_A, success(VALUE_A)).expect("seed old value");

    let template = json!({
        "npm": "@ai-sdk/openai-compatible",
        "name": "Detach test",
        "options": {
            "baseURL": "https://api.example.test/v1",
            "apiKey": format!("{{{{{VAR_A}}}}}")
        },
        "models": {"model-a": {"name": "Model A"}}
    });
    for id in ["historical", "current"] {
        ProviderService::add(
            &state,
            AppType::OpenCode,
            provider(id, template.clone()),
            false,
        )
        .expect("add DB-only provider");
    }

    // A native/import read that contains the resolved value must retain the
    // template. This uses the legacy DAO entry point on purpose.
    let mut imported = row(&state, AppType::OpenCode, "historical");
    imported.settings_config["options"]["apiKey"] = json!(VALUE_A);
    state
        .db
        .save_provider(AppType::OpenCode.as_str(), &imported)
        .expect("import readback preserves template");
    assert_eq!(
        row(&state, AppType::OpenCode, "historical").settings_config["options"]["apiKey"],
        json!(format!("{{{{{VAR_A}}}}}"))
    );

    api_key_vars::commit_result(&state, VAR_A, success(VALUE_B)).expect("refresh current value");

    let mut historical = row(&state, AppType::OpenCode, "historical");
    historical.settings_config["options"]["apiKey"] = json!(VALUE_A);
    ProviderService::update(&state, AppType::OpenCode, None, historical)
        .expect("user can detach historical value");

    let mut current = row(&state, AppType::OpenCode, "current");
    current.settings_config["options"]["apiKey"] = json!(VALUE_B);
    ProviderService::update(&state, AppType::OpenCode, None, current)
        .expect("user can detach current value");

    for (id, expected) in [("historical", VALUE_A), ("current", VALUE_B)] {
        assert_eq!(
            row(&state, AppType::OpenCode, id).settings_config["options"]["apiKey"],
            json!(expected),
            "explicit edit for {id} must not be restored as a template"
        );
    }
    api_key_vars::delete(&state, VAR_A).expect("detached references no longer block deletion");
}
