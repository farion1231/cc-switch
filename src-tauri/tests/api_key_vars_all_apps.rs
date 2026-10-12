//! End-to-end coverage for API-key variables in every additive provider format.
//!
//! These tests only use an isolated home plus explicit client-directory overrides.  They never
//! start a client binary and never inspect the developer's real configuration.

use std::{ffi::OsString, fs, path::Path};

use cc_switch_lib::{
    api_key_vars::{
        self,
        runner::{self, ApiKeyRefreshConfig, RefreshOutput, ScriptContext, ScriptRun},
        ApiKeyVarInput,
    },
    AppType, Provider, ProviderService,
};
use serde_json::json;

#[path = "support.rs"]
mod support;
use support::{create_test_state, ensure_test_home, reset_test_fs, test_mutex};

const VAR: &str = "IT_ALL_APPS_KEY";
const FIRST: &str = "all-apps-key-one";
const SECOND: &str = "all-apps-key-two";
const THIRD: &str = "all-apps-key-three";

/// Restores process overrides even when an assertion panics.  Every client-specific path below
/// must remain rooted in `ensure_test_home()`.
struct EnvRestore(Vec<(&'static str, Option<OsString>)>);

impl EnvRestore {
    fn set(name: &'static str, value: &Path) -> Self {
        let old = std::env::var_os(name);
        std::env::set_var(name, value);
        Self(vec![(name, old)])
    }

    fn also_set(&mut self, name: &'static str, value: &Path) {
        self.0.push((name, std::env::var_os(name)));
        std::env::set_var(name, value);
    }
}

impl Drop for EnvRestore {
    fn drop(&mut self) {
        for (name, old) in self.0.drain(..).rev() {
            match old {
                Some(value) => std::env::set_var(name, value),
                None => std::env::remove_var(name),
            }
        }
    }
}

fn config(enabled: bool, script_dir: &Path) -> ApiKeyRefreshConfig {
    ApiKeyRefreshConfig {
        enabled,
        script_dir: script_dir.display().to_string(),
        timeout_secs: Some(5),
        lead_time_secs: None,
    }
}

fn key_provider(app: AppType, id: &str) -> Provider {
    let settings_config = match app {
        AppType::OpenCode => json!({
            "npm": "@ai-sdk/openai-compatible", "name": id,
            "options": {"baseURL": "https://api.example.test/v1", "apiKey": format!("{{{{{VAR}}}}}")},
            "models": {"model-a": {"name": "Model A", "apiKey": format!("{{{{{VAR}}}}}")}},
            "headers": {"Authorization": "{{NOT_A_KEY}}"},
        }),
        AppType::OpenClaw => json!({
            "baseUrl": "https://api.example.test/v1", "apiKey": format!("{{{{{VAR}}}}}"),
            "api": "openai-completions", "models": [{"id": "model-a", "apiKey": format!("{{{{{VAR}}}}}")}],
            "headers": {"Authorization": "{{NOT_A_KEY}}"},
        }),
        AppType::Hermes => json!({
            "api": "openai-chat", "base_url": "https://api.example.test/v1",
            "api_key": format!("{{{{{VAR}}}}}"),
            "models": {"model-a": {"name": "Model A", "api_key": format!("{{{{{VAR}}}}}")}},
        }),
        AppType::Pi => json!({
            "name": id, "baseUrl": "https://api.example.test/v1", "apiKey": format!("{{{{{VAR}}}}}"),
            "api": "openai-completions", "models": [{"id": "model-a", "apiKey": format!("{{{{{VAR}}}}}")}],
        }),
        AppType::Mcode => json!({
            "name": id, "kind": "custom", "enabled": true, "api": "openai-completions",
            "options": {"baseURL": "https://api.example.test/v1", "apiKey": format!("{{{{{VAR}}}}}")},
            "models": {"model-a": {"name": "Model A", "api_key": format!("{{{{{VAR}}}}}")}},
        }),
        _ => unreachable!("this fixture only covers additive app formats"),
    };
    let mut provider = Provider::with_id(id.to_string(), id.to_string(), settings_config, None);
    provider.category = Some("custom".into());
    provider
}

fn run_multifile_fixture(dir: &Path) -> runner::ScriptRun {
    fs::create_dir_all(dir.join("lib")).expect("create multi-file script fixture");
    let module = dir.join("lib").join("key.mjs");
    if !module.exists() {
        fs::write(&module, "export const apiKey = process.env.TEST_KEY;\n")
            .expect("write imported module");
        fs::write(
            dir.join("index.mjs"),
            "import { apiKey } from './lib/key.mjs';\nconsole.log(JSON.stringify({ apiKey }));\n",
        )
        .expect("write index.mjs");
    }
    std::env::set_var("TEST_KEY", FIRST);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    let result = runtime
        .block_on(async {
            let node = runner::resolve_node(None)
                .await
                .expect("Node is required for this E2E fixture");
            runner::run_script(
                &config(true, dir),
                &ScriptContext {
                    var_name: VAR.into(),
                    reason: "manual",
                    current_expires_at: None,
                },
                Some(node.to_str().expect("Unicode Node path")),
                &[],
            )
            .await
        })
        .expect("run multi-file Node fixture");
    std::env::remove_var("TEST_KEY");
    result
}

fn assert_inside(path: &Path, home: &Path) {
    assert!(
        path.starts_with(home),
        "{} escaped isolated test home {}",
        path.display(),
        home.display()
    );
}

fn cached_run(value: &str) -> ScriptRun {
    ScriptRun {
        output: RefreshOutput {
            api_key: value.to_string(),
            expires_at: None,
            refresh_after: None,
            message: None,
        },
        resolved_file: "index.mjs".into(),
    }
}

#[test]
fn refresh_projects_shared_key_to_all_additive_apps_without_enabling_drafts() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    let home = ensure_test_home().to_path_buf();
    let pi_dir = home.join("isolated-pi-agent");
    let mcode_dir = home.join("isolated-mcode");
    let hermes_dir = home.join("isolated-hermes");
    let mut env = EnvRestore::set("PI_CODING_AGENT_DIR", &pi_dir);
    env.also_set("MINIMAX_DATA_DIR", &mcode_dir);
    env.also_set("MAVIS_DATA_DIR", &mcode_dir);
    env.also_set("HERMES_HOME", &hermes_dir);
    for path in [&pi_dir, &mcode_dir, &hermes_dir] {
        assert_inside(path, &home);
    }

    let script_dir = home.join("scripts").join("multi file");
    let state = create_test_state().expect("state");
    state
        .db
        .upsert_api_key_var(None, VAR, &config(true, &script_dir))
        .expect("save variable");
    let first = run_multifile_fixture(&script_dir);
    assert_eq!(
        first.output.api_key, FIRST,
        "the fixture must import lib/key.mjs"
    );
    api_key_vars::commit_result(&state, VAR, Ok(first)).expect("commit first script result");

    let apps = [
        AppType::OpenCode,
        AppType::OpenClaw,
        AppType::Hermes,
        AppType::Pi,
        AppType::Mcode,
    ];
    for app in &apps {
        ProviderService::add(
            &state,
            app.clone(),
            key_provider(
                app.clone(),
                &format!("{}/live-a", app.as_str()).replace('/', "-"),
            ),
            true,
        )
        .unwrap_or_else(|error| panic!("add first live {} provider: {error}", app.as_str()));
        ProviderService::add(
            &state,
            app.clone(),
            key_provider(
                app.clone(),
                &format!("{}/live-b", app.as_str()).replace('/', "-"),
            ),
            true,
        )
        .unwrap_or_else(|error| panic!("add second live {} provider: {error}", app.as_str()));
        ProviderService::add(
            &state,
            app.clone(),
            key_provider(
                app.clone(),
                &format!("{}/draft", app.as_str()).replace('/', "-"),
            ),
            false,
        )
        .unwrap_or_else(|error| panic!("save inactive {} draft: {error}", app.as_str()));
    }

    // Changing the script output simulates a real refresh.  `commit_result` must update only
    // entries which actually exist in each native config, and must leave literal templates in DB.
    fs::write(
        script_dir.join("lib").join("key.mjs"),
        format!("export const apiKey = '{SECOND}';\n"),
    )
    .expect("advance fixture value");
    let next = run_multifile_fixture(&script_dir);
    let outcome = api_key_vars::commit_result(&state, VAR, Ok(next)).expect("commit refreshed key");
    assert_eq!(
        outcome.updated_providers.len(),
        10,
        "{:#?}",
        outcome.live_errors
    );
    assert!(outcome.live_errors.is_empty(), "{:#?}", outcome.live_errors);

    // A malformed native file must only fail that projection.  The successful apps still move
    // forward, and repairing the file lets the next refresh recover the missing projection.
    let openclaw_path = home.join(".openclaw").join("openclaw.json");
    let valid_openclaw = fs::read_to_string(&openclaw_path).expect("read isolated OpenClaw file");
    fs::write(&openclaw_path, "{ malformed json5").expect("make OpenClaw projection fail");
    fs::write(
        script_dir.join("lib").join("key.mjs"),
        format!("export const apiKey = '{THIRD}';\n"),
    )
    .expect("advance fixture value again");
    let partial = api_key_vars::commit_result(&state, VAR, Ok(run_multifile_fixture(&script_dir)))
        .expect("commit despite one projection failure");
    assert_eq!(
        partial.updated_providers.len(),
        8,
        "{:#?}",
        partial.live_errors
    );
    assert!(
        partial
            .live_errors
            .iter()
            .any(|error| error.contains("openclaw/")),
        "{:#?}",
        partial.live_errors
    );
    assert!(
        ProviderService::read_live_settings(AppType::OpenCode)
            .expect("OpenCode remains writable")
            .to_string()
            .contains(THIRD),
        "an OpenClaw error must not roll back other applications"
    );
    fs::write(&openclaw_path, valid_openclaw).expect("repair isolated OpenClaw file");
    let recovered =
        api_key_vars::commit_result(&state, VAR, Ok(run_multifile_fixture(&script_dir)))
            .expect("retry repaired projection");
    assert_eq!(
        recovered.updated_providers.len(),
        10,
        "{:#?}",
        recovered.live_errors
    );
    assert!(
        recovered.live_errors.is_empty(),
        "{:#?}",
        recovered.live_errors
    );

    for app in apps {
        let rendered = if app == AppType::Pi {
            fs::read_to_string(pi_dir.join("models.json")).expect("read isolated Pi models.json")
        } else {
            ProviderService::read_live_settings(app.clone())
                .unwrap_or_else(|error| {
                    panic!("read isolated {} live configuration: {error}", app.as_str())
                })
                .to_string()
        };
        assert!(
            rendered.contains(THIRD),
            "{} did not receive refreshed value: {rendered}",
            app.as_str()
        );
        assert!(
            !rendered.contains(VAR),
            "{} leaked reference into live: {rendered}",
            app.as_str()
        );
        let draft = state
            .db
            .get_provider_by_id(&format!("{}-draft", app.as_str()), app.as_str())
            .expect("read draft")
            .expect("draft row");
        assert!(
            draft
                .settings_config
                .to_string()
                .contains(&format!("{{{{{VAR}}}}}")),
            "{} DB template was replaced",
            app.as_str()
        );
        assert!(
            !rendered.contains(&format!("{}-draft", app.as_str())),
            "{} draft was unexpectedly enabled",
            app.as_str()
        );
        if matches!(app, AppType::OpenCode | AppType::OpenClaw) {
            assert!(
                rendered.contains("NOT_A_KEY"),
                "{} must preserve unrelated request headers",
                app.as_str()
            );
        }
    }

    let refs = api_key_vars::all_references(&state.db).expect("collect all references");
    assert_eq!(refs.get(VAR).expect("shared ref").len(), 15);
    let view = api_key_vars::list_views(&state.db)
        .expect("masked views")
        .into_iter()
        .find(|view| view.name == VAR)
        .expect("view");
    assert!(
        !serde_json::to_string(&view).unwrap().contains(THIRD),
        "views must mask the current secret"
    );

    // Removing a native entry leaves its saved template in the database, but a refresh must
    // report it as inactive instead of recreating it.
    ProviderService::remove_from_live_config(&state, AppType::OpenClaw, "openclaw-live-a")
        .expect("remove one OpenClaw native provider");
    let omitted = api_key_vars::commit_result(&state, VAR, Ok(run_multifile_fixture(&script_dir)))
        .expect("refresh after native removal");
    assert_eq!(
        omitted.updated_providers.len(),
        9,
        "{:#?}",
        omitted.live_errors
    );
    assert!(omitted.live_errors.is_empty(), "{:#?}", omitted.live_errors);
    let after_remove = fs::read_to_string(&openclaw_path).expect("read OpenClaw after refresh");
    assert!(
        !after_remove.contains("openclaw-live-a"),
        "refresh must not resurrect a removed native provider: {after_remove}"
    );
}

#[test]
fn unresolved_drafts_are_allowed_but_live_enable_is_rejected_without_writing() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    let home = ensure_test_home().to_path_buf();
    let pi_dir = home.join("unresolved-pi");
    let mut env = EnvRestore::set("PI_CODING_AGENT_DIR", &pi_dir);
    env.also_set("MINIMAX_DATA_DIR", &home.join("unresolved-mcode"));
    env.also_set("MAVIS_DATA_DIR", &home.join("unresolved-mcode"));
    env.also_set("HERMES_HOME", &home.join("unresolved-hermes"));
    let state = create_test_state().expect("state");

    let missing = "IT_UNDEFINED_PI_DRAFT_KEY";
    api_key_vars::resolve::cache_remove(missing);
    let mut draft = key_provider(AppType::Pi, "pi-unresolved-draft");
    draft.settings_config =
        api_key_vars::resolve::rewrite_refs(&AppType::Pi, &draft.settings_config, VAR, missing)
            .expect("give this test an independently undefined variable");
    let mut live = draft.clone();
    live.id = "pi-unresolved-live".into();
    ProviderService::add(&state, AppType::Pi, draft, false)
        .expect("unresolved inactive draft is storable");
    let before = pi_dir.join("models.json");
    assert!(
        !before.exists(),
        "inactive draft must not create Pi live config"
    );

    assert!(
        ProviderService::add(&state, AppType::Pi, live, true).is_err(),
        "unknown variable must block enabling"
    );
    assert!(
        !before.exists(),
        "failed enable must not write a literal reference into live config"
    );
}

#[cfg(windows)]
#[test]
fn claude_desktop_direct_profile_resolves_variable_and_keeps_database_template() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    let home = ensure_test_home().to_path_buf();
    let local_app_data = home.join("Desktop AppData").join("Local");
    assert_inside(&local_app_data, &home);
    let _env = EnvRestore::set("LOCALAPPDATA", &local_app_data);
    let state = create_test_state().expect("state");
    state
        .db
        .upsert_api_key_var(None, VAR, &config(true, &home.join("desktop-script")))
        .expect("save variable");
    api_key_vars::commit_result(&state, VAR, Ok(cached_run(FIRST))).expect("seed key value");

    let provider = Provider::with_id(
        "desktop-var".into(),
        "Desktop variable provider".into(),
        json!({"env": {
            "ANTHROPIC_BASE_URL": "https://desktop.example.test",
            "ANTHROPIC_AUTH_TOKEN": format!("{{{{{VAR}}}}}"),
        }}),
        None,
    );
    ProviderService::add(&state, AppType::ClaudeDesktop, provider, true)
        .expect("write isolated Claude Desktop direct profile");

    let profile = local_app_data
        .join("Claude-3p")
        .join("configLibrary")
        .join("00000000-0000-4000-8000-000000157210.json");
    assert_inside(&profile, &home);
    let first_profile = fs::read_to_string(&profile).expect("read isolated 3P profile");
    assert!(first_profile.contains(FIRST), "{first_profile}");
    assert!(!first_profile.contains(VAR), "{first_profile}");

    let refreshed = api_key_vars::commit_result(&state, VAR, Ok(cached_run(SECOND)))
        .expect("refresh Desktop provider");
    assert_eq!(
        refreshed.updated_providers.len(),
        1,
        "{:#?}",
        refreshed.live_errors
    );
    assert!(
        fs::read_to_string(&profile)
            .expect("read refreshed profile")
            .contains(SECOND),
        "Desktop profile was not re-projected"
    );
    let saved = state
        .db
        .get_provider_by_id("desktop-var", AppType::ClaudeDesktop.as_str())
        .expect("read Desktop provider")
        .expect("saved Desktop provider");
    assert_eq!(
        saved.settings_config["env"]["ANTHROPIC_AUTH_TOKEN"],
        format!("{{{{{VAR}}}}}")
    );
}

#[test]
fn codex_toml_bearer_references_use_root_fallback_and_table_source_independently() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    let home = ensure_test_home().to_path_buf();
    let state = create_test_state().expect("state");
    state
        .db
        .upsert_api_key_var(None, VAR, &config(true, &home.join("codex-script")))
        .expect("save variable");
    api_key_vars::commit_result(&state, VAR, Ok(cached_run(FIRST))).expect("seed key value");

    let root_config = format!(
        "experimental_bearer_token = \"{{{{{VAR}}}}}\"\nmodel_provider = \"relay\"\n\n[model_providers.relay]\nname = \"Relay\"\nbase_url = \"https://codex.example.test/v1\"\nwire_api = \"responses\"\n"
    );
    let root_provider = Provider::with_id(
        "codex-root-bearer".into(),
        "Codex root bearer fallback".into(),
        json!({"auth": {}, "config": root_config}),
        None,
    );
    ProviderService::add(&state, AppType::Codex, root_provider, true)
        .expect("project root bearer fallback");
    let live_path = cc_switch_lib::get_codex_config_path();
    assert_inside(&live_path, &home);
    let live = fs::read_to_string(&live_path).expect("read isolated Codex config");
    assert_eq!(live.matches(FIRST).count(), 1, "{live}");
    assert!(!live.contains(VAR), "{live}");
    assert!(
        !live.contains("experimental_bearer_token = \"{{"),
        "a source root template must never reach native TOML: {live}"
    );

    let refreshed = api_key_vars::commit_result(&state, VAR, Ok(cached_run(SECOND)))
        .expect("refresh root fallback bearer");
    assert_eq!(
        refreshed.updated_providers.len(),
        1,
        "{:#?}",
        refreshed.live_errors
    );
    let live = fs::read_to_string(&live_path).expect("read refreshed Codex config");
    assert_eq!(live.matches(SECOND).count(), 1, "{live}");
    let saved_root = state
        .db
        .get_provider_by_id("codex-root-bearer", AppType::Codex.as_str())
        .expect("read Codex row")
        .expect("saved Codex row");
    assert_eq!(
        saved_root.settings_config["auth"],
        json!({}),
        "the test intentionally has no auth API key"
    );
    assert_eq!(
        saved_root.settings_config["config"].as_str().expect("TOML"),
        root_config
    );

    let table_config = format!(
        "model_provider = \"relay\"\n\n[model_providers.relay]\nname = \"Relay\"\nbase_url = \"https://codex.example.test/v1\"\nwire_api = \"responses\"\nexperimental_bearer_token = \"{{{{{VAR}}}}}\"\n"
    );
    let table_provider = Provider::with_id(
        "codex-table-bearer".into(),
        "Codex table bearer".into(),
        json!({"auth": {}, "config": table_config}),
        None,
    );
    ProviderService::add(&state, AppType::Codex, table_provider, true)
        .expect("save table bearer provider");
    ProviderService::switch(&state, AppType::Codex, "codex-table-bearer")
        .expect("project table bearer source");
    let live = fs::read_to_string(&live_path).expect("read table bearer live TOML");
    assert_eq!(live.matches(SECOND).count(), 1, "{live}");
    assert!(!live.contains(VAR), "{live}");
    api_key_vars::commit_result(&state, VAR, Ok(cached_run(THIRD)))
        .expect("refresh table bearer source");
    let live = fs::read_to_string(&live_path).expect("read refreshed table bearer live TOML");
    assert_eq!(live.matches(THIRD).count(), 1, "{live}");
    let saved_table = state
        .db
        .get_provider_by_id("codex-table-bearer", AppType::Codex.as_str())
        .expect("read table Codex row")
        .expect("saved table Codex row");
    assert_eq!(
        saved_table.settings_config["config"]
            .as_str()
            .expect("TOML"),
        table_config
    );
}

#[test]
fn unsynced_universal_reference_blocks_deletion_and_rename_keeps_derived_templates() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    let home = ensure_test_home().to_path_buf();
    let state = create_test_state().expect("state");
    state
        .db
        .upsert_api_key_var(None, VAR, &config(true, &home.join("universal-script")))
        .expect("save variable");
    api_key_vars::commit_result(&state, VAR, Ok(cached_run(FIRST))).expect("seed key value");

    let universal = serde_json::from_value(json!({
        "id": "universal-unsynced", "name": "Unsynced universal", "providerType": "custom",
        "baseUrl": "https://universal.example.test", "apiKey": format!("{{{{{VAR}}}}}"),
        "apps": {"claude": true}, "models": {}
    }))
    .expect("infer UniversalProvider from public JSON shape");
    ProviderService::upsert_universal(&state, universal).expect("save unsynced universal provider");
    let refs = api_key_vars::all_references(&state.db).expect("references include universal");
    let universal_ref = refs.get(VAR).expect("universal reference");
    assert_eq!(universal_ref.len(), 1);
    assert_eq!(universal_ref[0].app, "universal");
    assert!(
        api_key_vars::delete(&state, VAR).is_err(),
        "unsynced universal reference blocks deletion"
    );

    ProviderService::sync_universal_to_apps(&state, "universal-unsynced")
        .expect("derive child providers without requiring a live current provider");
    let child = state
        .db
        .get_provider_by_id(
            "universal-claude-universal-unsynced",
            AppType::Claude.as_str(),
        )
        .expect("read derived child")
        .expect("derived Claude child");
    assert_eq!(
        child.settings_config["env"]["ANTHROPIC_AUTH_TOKEN"],
        format!("{{{{{VAR}}}}}")
    );

    let renamed = "IT_ALL_APPS_KEY_RENAMED";
    api_key_vars::upsert(
        &state,
        &ApiKeyVarInput {
            name: renamed.into(),
            script_dir: String::new(),
            enabled: false,
            timeout_secs: None,
            lead_time_secs: None,
        },
        Some(VAR),
    )
    .expect("rename variable and all dependent templates atomically");
    let universal = ProviderService::get_universal(&state, "universal-unsynced")
        .expect("read universal")
        .expect("universal remains");
    assert_eq!(universal.api_key, format!("{{{{{renamed}}}}}"));
    let child = state
        .db
        .get_provider_by_id(
            "universal-claude-universal-unsynced",
            AppType::Claude.as_str(),
        )
        .expect("read renamed child")
        .expect("child remains");
    assert_eq!(
        child.settings_config["env"]["ANTHROPIC_AUTH_TOKEN"],
        format!("{{{{{renamed}}}}}")
    );
}
