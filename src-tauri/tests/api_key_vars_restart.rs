//! Process-boundary regression coverage for API-key variable startup recovery.
//!
//! The parent test launches this same integration-test executable twice.  Each child receives a
//! unique, explicitly isolated home directory, so this never reads or writes a developer's
//! Codex, Claude, Pi, MCode, or Hermes configuration.

use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use cc_switch_lib::{
    api_key_vars::{
        self,
        resolve::{self, RefStatus},
        runner::{ApiKeyRefreshConfig, ApiKeyRefreshState, RefreshOutput, ScriptRun},
    },
    mode, AppState, AppType, Database, Provider, ProviderService,
};
use rusqlite::Connection;
use serde_json::json;
use tempfile::TempDir;

const PHASE_ENV: &str = "CC_SWITCH_API_KEY_RESTART_PHASE";
const HOME_ENV: &str = "CC_SWITCH_API_KEY_RESTART_HOME";
const VAR: &str = "RESTART_SHARED_KEY";
const FIRST: &str = "fixture-restart-value-a";
const SECOND: &str = "fixture-restart-value-b";
const DISABLED: &str = "RESTART_DISABLED_KEY";
const EXPIRED: &str = "RESTART_EXPIRED_KEY";
const MULTI_VALID: &str = "RESTART_MULTI_VALID_KEY";
const MULTI_EXPIRED: &str = "RESTART_MULTI_EXPIRED_KEY";
const MISSING: &str = "RESTART_MISSING_KEY";

fn ref_value() -> String {
    format!("{{{{{VAR}}}}}")
}

fn refresh_config(script_dir: &Path) -> ApiKeyRefreshConfig {
    ApiKeyRefreshConfig {
        enabled: true,
        script_dir: script_dir.display().to_string(),
        timeout_secs: Some(5),
        lead_time_secs: None,
    }
}

fn script_result(value: &str) -> ScriptRun {
    ScriptRun {
        output: RefreshOutput {
            api_key: value.to_string(),
            expires_at: Some(chrono::Utc::now().timestamp_millis() + 3_600_000),
            refresh_after: Some(chrono::Utc::now().timestamp_millis() + 1_800_000),
            message: None,
        },
        resolved_file: "index.mjs".to_string(),
    }
}

fn claude_provider() -> Provider {
    Provider::with_id(
        "restart-claude-current".into(),
        "restart Claude current".into(),
        json!({"env": {
            "ANTHROPIC_BASE_URL": "https://restart.example.invalid/claude",
            "ANTHROPIC_AUTH_TOKEN": ref_value(),
        }}),
        None,
    )
}

fn codex_provider() -> Provider {
    Provider::with_id(
        "restart-codex-current".into(),
        "restart Codex current".into(),
        json!({
            "auth": {"OPENAI_API_KEY": ref_value()},
            "config": "model_provider = \"restart\"\n\n[model_providers.restart]\nname = \"Restart\"\nbase_url = \"https://restart.example.invalid/codex\"\nwire_api = \"responses\"\n",
        }),
        None,
    )
}

fn pi_provider(id: &str) -> Provider {
    Provider::with_id(
        id.into(),
        id.into(),
        json!({
            "name": id,
            "baseUrl": "https://restart.example.invalid/pi",
            "apiKey": ref_value(),
            "api": "openai-completions",
            "models": [{"id": "restart-model", "apiKey": ref_value()}],
        }),
        None,
    )
}

fn pi_provider_with_keys(id: &str, api_key: String, model_key: String) -> Provider {
    Provider::with_id(
        id.into(),
        id.into(),
        json!({
            "name": id,
            "baseUrl": "https://restart.example.invalid/pi",
            "apiKey": api_key,
            "api": "openai-completions",
            "models": [{"id": "restart-model", "apiKey": model_key}],
        }),
        None,
    )
}

fn opencode_provider() -> Provider {
    Provider::with_id(
        "restart-opencode-live".into(),
        "restart OpenCode live".into(),
        json!({
            "npm": "@ai-sdk/openai-compatible",
            "name": "restart-opencode-live",
            "options": {
                "baseURL": "https://restart.example.invalid/opencode",
                "apiKey": ref_value(),
            },
            "models": {"restart-model": {"name": "Restart model", "apiKey": ref_value()}},
        }),
        None,
    )
}

fn child_command(home: &Path, phase: &str) -> Command {
    let mut command = Command::new(std::env::current_exe().expect("current test executable"));
    command
        .arg("--exact")
        .arg("restart_child_probe")
        .arg("--ignored")
        .arg("--nocapture")
        .arg("--test-threads=1")
        .env(PHASE_ENV, phase)
        .env(HOME_ENV, home)
        // Do not alter HOME/CODEX_HOME: all app paths are explicitly isolated instead.
        .env("CC_SWITCH_TEST_HOME", home)
        .env("LOCALAPPDATA", home.join("AppData").join("Local"))
        .env("APPDATA", home.join("AppData").join("Roaming"))
        .env("PI_CODING_AGENT_DIR", home.join("pi-agent"))
        .env("MINIMAX_DATA_DIR", home.join("mcode"))
        .env("MAVIS_DATA_DIR", home.join("mcode"))
        .env("HERMES_HOME", home.join("hermes"))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}

fn run_child(home: &Path, phase: &str) {
    let mut child = child_command(home, phase)
        .spawn()
        .unwrap_or_else(|error| panic!("spawn restart child phase {phase}: {error}"));
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        match child.try_wait().expect("poll restart child") {
            Some(status) => {
                let output = child
                    .wait_with_output()
                    .expect("collect restart child output");
                assert!(
                    status.success(),
                    "restart child phase {phase} failed with {status}; stdout:\n{}\nstderr:\n{}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr),
                );
                return;
            }
            None if Instant::now() >= deadline => {
                let _ = child.kill();
                let output = child
                    .wait_with_output()
                    .expect("collect timed-out restart child output");
                panic!(
                    "restart child phase {phase} exceeded 60 seconds; stdout:\n{}\nstderr:\n{}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr),
                );
            }
            None => thread::sleep(Duration::from_millis(25)),
        }
    }
}

#[test]
fn api_key_variables_reconcile_live_values_after_real_process_restart() {
    let temp = TempDir::new().expect("create unique restart test directory");
    run_child(temp.path(), "seed");
    run_child(temp.path(), "restart");
}

#[test]
fn cold_restart_leaves_unusable_and_removed_additive_nodes_unchanged() {
    let temp = TempDir::new().expect("create unique negative restart test directory");
    run_child(temp.path(), "seed-negative");
    run_child(temp.path(), "restart-negative");
}

#[test]
fn cold_restart_continues_after_one_app_has_an_unreadable_provider_row() {
    let temp = TempDir::new().expect("create malformed-row restart test directory");
    run_child(temp.path(), "seed");
    Connection::open(temp.path().join(".cc-switch").join("cc-switch.db"))
        .expect("open isolated database")
        .execute(
            "UPDATE providers SET settings_config = X'00' WHERE id = 'restart-claude-current' AND app_type = 'claude'",
            [],
        )
        .expect("make only the Claude provider row unreadable");
    run_child(temp.path(), "restart-malformed-claude");
}

#[test]
fn cold_restart_preserves_proxy_contract_then_projects_cached_value_on_exit() {
    let temp = TempDir::new().expect("create proxy restart test directory");
    run_child(temp.path(), "seed-proxy");
    run_child(temp.path(), "restart-proxy");
}

#[test]
fn cold_restart_retries_one_transient_additive_projection_failure() {
    let temp = TempDir::new().expect("create retry restart test directory");
    run_child(temp.path(), "seed");
    run_child(temp.path(), "restart-retry-pi");
}

#[test]
#[ignore = "invoked only by api_key_variables_reconcile_live_values_after_real_process_restart"]
fn restart_child_probe() {
    let phase = std::env::var(PHASE_ENV).expect("restart child phase");
    let home = PathBuf::from(std::env::var_os(HOME_ENV).expect("restart child home"));
    assert!(home.is_dir(), "isolated restart home must exist");
    match phase.as_str() {
        "seed" => seed(&home),
        "restart" => restart(&home),
        "seed-negative" => seed_negative(&home),
        "restart-negative" => restart_negative(&home),
        "restart-malformed-claude" => restart_malformed_claude(&home),
        "seed-proxy" => seed_proxy(&home),
        "restart-proxy" => restart_proxy(&home),
        "restart-retry-pi" => restart_retry_pi(&home),
        other => panic!("unknown restart child phase {other}"),
    }
}

fn seed(home: &Path) {
    let scripts = home.join("scripts");
    fs::create_dir_all(&scripts).expect("create script directory");
    let marker = home.join("script-ran.marker");
    fs::write(
        scripts.join("index.mjs"),
        format!(
            "import fs from 'node:fs'; fs.writeFileSync({:?}, 'ran'); console.log(JSON.stringify({{apiKey:'unexpected'}}));\n",
            marker
        ),
    )
    .expect("write script that must not run during recovery");

    let state = AppState::new(std::sync::Arc::new(
        Database::init().expect("seed database"),
    ));
    state
        .db
        .upsert_api_key_var(None, VAR, &refresh_config(&scripts))
        .expect("seed variable");
    api_key_vars::commit_result(&state, VAR, Ok(script_result(FIRST)))
        .expect("seed initial live value");

    ProviderService::add(&state, AppType::Claude, claude_provider(), true)
        .expect("seed current direct Claude provider");
    ProviderService::add(&state, AppType::Codex, codex_provider(), true)
        .expect("seed current direct Codex provider");
    // Persist direct mode/current pointers so controller::startup takes the same production mode
    // recovery path after the process boundary.
    ProviderService::switch(&state, AppType::Claude, "restart-claude-current")
        .expect("select current direct Claude provider");
    ProviderService::switch(&state, AppType::Codex, "restart-codex-current")
        .expect("select current direct Codex provider");
    ProviderService::add(&state, AppType::Pi, pi_provider("restart-pi-live"), true)
        .expect("seed live Pi additive node");
    ProviderService::add(&state, AppType::OpenCode, opencode_provider(), true)
        .expect("seed live OpenCode additive node");
    ProviderService::add(&state, AppType::Pi, pi_provider("restart-pi-draft"), false)
        .expect("seed database-only Pi draft");

    assert_live_contains(home, FIRST);
    assert_db_templates(&state);

    // Model a refresh whose transaction successfully persisted the new current value but whose
    // live projections all failed before they could be written.  Do not call commit_result here.
    let now = chrono::Utc::now().timestamp_millis();
    state
        .db
        .update_api_key_var_result(
            VAR,
            Some(SECOND),
            &ApiKeyRefreshState {
                expires_at: Some(now + 3_600_000),
                refresh_after: Some(now + 1_800_000),
                last_success_at: Some(now),
                resolved_script: Some("index.mjs".into()),
                ..Default::default()
            },
        )
        .expect("persist refreshed value without live propagation");
    assert_live_contains(home, FIRST);
    assert!(
        !marker.exists(),
        "seed must not run the configured Node script"
    );
}

fn restart(home: &Path) {
    // A new executable must begin without the previous process's cache.
    assert_eq!(resolve::lookup(VAR), (RefStatus::Unknown, None));
    let state = AppState::new(std::sync::Arc::new(
        Database::init().expect("restart database"),
    ));
    assert_eq!(
        state
            .db
            .load_api_key_var_cache()
            .expect("load persisted key cache"),
        1
    );
    assert_eq!(
        resolve::lookup(VAR),
        (RefStatus::Ok, Some(SECOND.to_string())),
        "the persisted, unexpired value must restore before startup projection"
    );

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("startup runtime");
    runtime.block_on(mode::controller::startup(&state));
    let errors = runtime
        .block_on(api_key_vars::reconcile_cached_values_on_startup(&state))
        .expect("reconcile cached values");
    assert!(
        errors.is_empty(),
        "unexpected reconciliation errors: {errors:#?}"
    );
    assert_live_contains(home, SECOND);
    assert_db_templates(&state);
    assert!(
        !home.join("script-ran.marker").exists(),
        "cold-start reconciliation must not execute the configured Node script"
    );
}

fn seed_negative(home: &Path) {
    let scripts = home.join("scripts");
    fs::create_dir_all(&scripts).expect("create negative script directory");
    let marker = home.join("negative-script-ran.marker");
    fs::write(
        scripts.join("index.mjs"),
        format!(
            "import fs from 'node:fs'; fs.writeFileSync({:?}, 'ran');\n",
            marker
        ),
    )
    .expect("write negative script marker");

    let state = AppState::new(std::sync::Arc::new(
        Database::init().expect("negative seed database"),
    ));
    let config = refresh_config(&scripts);
    state
        .db
        .upsert_api_key_var(None, VAR, &config)
        .expect("seed shared variable");
    api_key_vars::commit_result(&state, VAR, Ok(script_result(FIRST)))
        .expect("seed shared live value");
    ProviderService::add(&state, AppType::Claude, claude_provider(), true)
        .expect("seed negative current Claude provider");
    ProviderService::switch(&state, AppType::Claude, "restart-claude-current")
        .expect("select negative current Claude provider");

    for name in [DISABLED, EXPIRED, MULTI_VALID, MULTI_EXPIRED] {
        state
            .db
            .upsert_api_key_var(None, name, &config)
            .unwrap_or_else(|error| panic!("seed {name}: {error}"));
        api_key_vars::commit_result(&state, name, Ok(script_result(&format!("{name}-value"))))
            .unwrap_or_else(|error| panic!("seed {name} value: {error}"));
    }

    ProviderService::add(
        &state,
        AppType::Pi,
        pi_provider_with_keys("restart-pi-disabled", ref_for(DISABLED), ref_for(DISABLED)),
        true,
    )
    .expect("seed disabled Pi node while variable is usable");
    ProviderService::add(
        &state,
        AppType::Pi,
        pi_provider_with_keys("restart-pi-expired", ref_for(EXPIRED), ref_for(EXPIRED)),
        true,
    )
    .expect("seed expired Pi node while variable is usable");
    ProviderService::add(
        &state,
        AppType::Pi,
        pi_provider_with_keys(
            "restart-pi-multi-expired",
            ref_for(MULTI_VALID),
            ref_for(MULTI_EXPIRED),
        ),
        true,
    )
    .expect("seed multi-reference Pi node while both variables are usable");

    // A missing reference can exist in a persisted draft/template after an external edit.  Its
    // native node is deliberately seeded as a literal then the database row is changed directly,
    // keeping this test focused on startup reconciliation rather than save-time validation.
    ProviderService::add(
        &state,
        AppType::Pi,
        pi_provider_with_keys(
            "restart-pi-missing",
            "missing-live-value".into(),
            "missing-live-value".into(),
        ),
        true,
    )
    .expect("seed missing-reference Pi live node");
    let mut missing = state
        .db
        .get_provider_by_id("restart-pi-missing", AppType::Pi.as_str())
        .expect("read missing provider")
        .expect("missing provider exists");
    missing.settings_config["apiKey"] = json!(ref_for(MISSING));
    missing.settings_config["models"][0]["apiKey"] = json!(ref_for(MISSING));
    state
        .db
        .save_provider(AppType::Pi.as_str(), &missing)
        .expect("persist missing reference template");

    ProviderService::add(&state, AppType::Pi, pi_provider("restart-pi-removed"), true)
        .expect("seed Pi node that will be externally removed");
    ProviderService::remove_from_live_config(&state, AppType::Pi, "restart-pi-removed")
        .expect("simulate external Pi node removal");
    ProviderService::add(&state, AppType::Pi, pi_provider("restart-pi-draft"), false)
        .expect("seed negative database-only Pi draft");

    // Make the already-projected native nodes unusable without changing their bytes.
    state
        .db
        .upsert_api_key_var(
            Some(DISABLED),
            DISABLED,
            &ApiKeyRefreshConfig {
                enabled: false,
                ..config.clone()
            },
        )
        .expect("disable variable after projection");
    let now = chrono::Utc::now().timestamp_millis();
    for name in [EXPIRED, MULTI_EXPIRED] {
        state
            .db
            .update_api_key_var_result(
                name,
                None,
                &ApiKeyRefreshState {
                    expires_at: Some(now - 1),
                    ..Default::default()
                },
            )
            .unwrap_or_else(|error| panic!("expire {name}: {error}"));
    }
    state
        .db
        .update_api_key_var_result(
            VAR,
            Some(SECOND),
            &ApiKeyRefreshState {
                expires_at: Some(now + 3_600_000),
                refresh_after: Some(now + 1_800_000),
                last_success_at: Some(now),
                resolved_script: Some("index.mjs".into()),
                ..Default::default()
            },
        )
        .expect("persist shared value without propagation");

    let pi_path = home.join("pi-agent").join("models.json");
    let before = fs::read(&pi_path).expect("read Pi bytes before restart");
    fs::write(home.join("negative-pi-before-restart.bin"), before)
        .expect("save expected unchanged Pi bytes");
    assert!(!marker.exists(), "negative seed must not run Node script");
}

fn restart_negative(home: &Path) {
    assert_eq!(resolve::lookup(VAR), (RefStatus::Unknown, None));
    let state = AppState::new(std::sync::Arc::new(
        Database::init().expect("negative restart database"),
    ));
    assert_eq!(
        state
            .db
            .load_api_key_var_cache()
            .expect("load negative persisted key cache"),
        5
    );
    assert_eq!(
        resolve::lookup(VAR),
        (RefStatus::Ok, Some(SECOND.to_string()))
    );

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("negative startup runtime");
    runtime.block_on(mode::controller::startup(&state));
    let errors = runtime
        .block_on(api_key_vars::reconcile_cached_values_on_startup(&state))
        .expect("negative reconciliation");
    assert!(
        errors.len() == 4
            && [
                "restart-pi-disabled",
                "restart-pi-expired",
                "restart-pi-multi-expired",
                "restart-pi-missing",
            ]
            .iter()
            .all(|name| errors.iter().any(|error| error.contains(name))),
        "only the four intentionally unusable live nodes may be skipped: {errors:#?}"
    );

    let claude = fs::read_to_string(home.join(".claude").join("settings.json"))
        .expect("read reconciled Claude settings");
    assert!(
        claude.contains(SECOND),
        "usable current provider was not updated"
    );
    let pi_path = home.join("pi-agent").join("models.json");
    assert_eq!(
        fs::read(&pi_path).expect("read Pi after restart"),
        fs::read(home.join("negative-pi-before-restart.bin")).expect("read expected Pi bytes"),
        "disabled, missing, expired, multi-expired, and externally removed nodes must not change",
    );
    let pi = fs::read_to_string(&pi_path).expect("read unchanged Pi config");
    assert!(
        !pi.contains("restart-pi-removed") && !pi.contains("restart-pi-draft"),
        "startup must not recreate externally removed or database-only Pi nodes"
    );
    assert_negative_templates(&state);
    assert!(
        !home.join("negative-script-ran.marker").exists(),
        "negative reconciliation must not execute the configured Node script"
    );
}

fn restart_malformed_claude(home: &Path) {
    assert_eq!(resolve::lookup(VAR), (RefStatus::Unknown, None));
    let state = AppState::new(std::sync::Arc::new(
        Database::init().expect("malformed-row restart database"),
    ));
    state
        .db
        .load_api_key_var_cache()
        .expect("load malformed-row key cache");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("malformed-row runtime");
    runtime.block_on(mode::controller::startup(&state));
    let errors = runtime
        .block_on(api_key_vars::reconcile_cached_values_on_startup(&state))
        .expect("malformed-row reconciliation");
    assert!(
        errors.len() == 1 && errors[0].starts_with("claude:"),
        "only the deliberately unreadable Claude app should fail: {errors:#?}"
    );
    assert_non_claude_live_contains(home, SECOND);
    assert_non_claude_templates(&state);
}

fn seed_proxy(home: &Path) {
    seed(home);
    let state = AppState::new(std::sync::Arc::new(
        Database::init().expect("proxy seed database"),
    ));
    state
        .db
        .load_api_key_var_cache()
        .expect("load proxy seed key cache");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("proxy seed runtime");
    runtime.block_on(async {
        let mut config = state
            .db
            .get_proxy_config()
            .await
            .expect("read proxy config");
        config.listen_port = 0;
        state
            .db
            .update_proxy_config(config)
            .await
            .expect("set isolated random proxy port");
        mode::controller::enter(&state, &AppType::Claude, false)
            .await
            .expect("enter Claude proxy mode");
        state
            .proxy_service
            .stop()
            .await
            .expect("stop only the seed proxy server");
    });
}

fn restart_proxy(home: &Path) {
    let state = AppState::new(std::sync::Arc::new(
        Database::init().expect("proxy restart database"),
    ));
    state
        .db
        .load_api_key_var_cache()
        .expect("load proxy restart key cache");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("proxy restart runtime");
    runtime.block_on(mode::controller::startup(&state));
    let live_path = home.join(".claude").join("settings.json");
    let before = fs::read(&live_path).expect("read reattached proxy contract");
    let before_text = String::from_utf8_lossy(&before);
    assert!(
        before_text.contains("PROXY_MANAGED"),
        "startup must reattach the saved Claude proxy contract"
    );
    let errors = runtime
        .block_on(api_key_vars::reconcile_cached_values_on_startup(&state))
        .expect("proxy reconciliation");
    assert!(
        errors.is_empty(),
        "unexpected proxy reconciliation errors: {errors:#?}"
    );
    assert_eq!(
        fs::read(&live_path).expect("read proxy contract after reconciliation"),
        before,
        "reconciliation must not replace a reattached proxy contract"
    );
    runtime.block_on(async {
        mode::controller::exit(&state, &AppType::Claude)
            .await
            .expect("exit Claude proxy mode");
        assert!(
            !state.proxy_service.is_running().await,
            "leaving the last proxy app must stop its isolated server"
        );
    });
    let direct = fs::read_to_string(&live_path).expect("read direct Claude projection");
    assert!(
        direct.contains(SECOND),
        "proxy exit must project cached value B"
    );
}

fn restart_retry_pi(home: &Path) {
    let state = AppState::new(std::sync::Arc::new(
        Database::init().expect("retry restart database"),
    ));
    state
        .db
        .load_api_key_var_cache()
        .expect("load retry key cache");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("retry runtime");
    runtime.block_on(mode::controller::startup(&state));
    let pi_path = home.join("pi-agent").join("models.json");
    let original = fs::read(&pi_path).expect("read Pi before transient failure");
    fs::remove_file(&pi_path).expect("remove Pi file for transient failure");
    fs::create_dir(&pi_path).expect("replace Pi file with same-name directory");
    let repair_path = pi_path.clone();
    let repair = runtime.spawn(async move {
        fs::remove_dir(&repair_path).expect("remove transient Pi directory");
        fs::write(&repair_path, original).expect("restore Pi file before retry");
    });
    let errors = runtime.block_on(async {
        let errors = api_key_vars::reconcile_cached_values_on_startup(&state)
            .await
            .expect("retry reconciliation");
        repair.await.expect("repair task joined");
        errors
    });
    assert!(
        errors.is_empty(),
        "retry should recover the only transient error: {errors:#?}"
    );
    assert_live_contains(home, SECOND);
}

fn assert_non_claude_live_contains(home: &Path, expected: &str) {
    let codex = fs::read_to_string(cc_switch_lib::get_codex_config_path())
        .expect("read isolated Codex config");
    assert!(
        codex.contains(expected),
        "Codex live value mismatch after Claude failure"
    );
    let pi = fs::read_to_string(home.join("pi-agent").join("models.json"))
        .expect("read isolated Pi models");
    assert!(
        pi.contains(expected),
        "Pi live value mismatch after Claude failure"
    );
    let opencode = ProviderService::read_live_settings(AppType::OpenCode)
        .expect("read isolated OpenCode config")
        .to_string();
    assert!(
        opencode.contains(expected),
        "OpenCode live value mismatch after Claude failure"
    );
}

fn assert_non_claude_templates(state: &AppState) {
    let template = ref_value();
    for (app, id, pointer) in [
        (
            AppType::Codex,
            "restart-codex-current",
            "/auth/OPENAI_API_KEY",
        ),
        (AppType::Pi, "restart-pi-live", "/apiKey"),
        (
            AppType::OpenCode,
            "restart-opencode-live",
            "/options/apiKey",
        ),
        (AppType::Pi, "restart-pi-draft", "/apiKey"),
    ] {
        let provider = state
            .db
            .get_provider_by_id(id, app.as_str())
            .expect("read provider after Claude failure")
            .expect("provider exists after Claude failure");
        assert_eq!(
            provider
                .settings_config
                .pointer(pointer)
                .and_then(serde_json::Value::as_str),
            Some(template.as_str()),
            "{}/{} lost its persisted template after another app failed",
            app.as_str(),
            id
        );
    }
}

fn ref_for(name: &str) -> String {
    format!("{{{{{name}}}}}")
}

fn assert_negative_templates(state: &AppState) {
    for (id, variable) in [
        ("restart-pi-disabled", DISABLED),
        ("restart-pi-expired", EXPIRED),
        ("restart-pi-missing", MISSING),
        ("restart-pi-draft", VAR),
    ] {
        let provider = state
            .db
            .get_provider_by_id(id, AppType::Pi.as_str())
            .expect("read negative provider")
            .expect("negative provider exists");
        let template = ref_for(variable);
        assert_eq!(
            provider.settings_config["apiKey"].as_str(),
            Some(template.as_str()),
            "{id} lost its stored template"
        );
    }
    let multi = state
        .db
        .get_provider_by_id("restart-pi-multi-expired", AppType::Pi.as_str())
        .expect("read multi-reference provider")
        .expect("multi-reference provider exists");
    let valid_template = ref_for(MULTI_VALID);
    let expired_template = ref_for(MULTI_EXPIRED);
    assert_eq!(
        multi.settings_config["apiKey"].as_str(),
        Some(valid_template.as_str())
    );
    assert_eq!(
        multi.settings_config["models"][0]["apiKey"].as_str(),
        Some(expired_template.as_str())
    );
}

fn assert_live_contains(home: &Path, expected: &str) {
    let claude = fs::read_to_string(home.join(".claude").join("settings.json"))
        .expect("read isolated Claude settings");
    assert!(
        claude.contains(expected),
        "Claude current live value mismatch"
    );

    let codex = fs::read_to_string(cc_switch_lib::get_codex_config_path())
        .expect("read isolated Codex config");
    assert!(
        codex.contains(expected),
        "Codex current live value mismatch"
    );

    let pi = fs::read_to_string(home.join("pi-agent").join("models.json"))
        .expect("read isolated Pi models");
    assert!(pi.contains(expected), "Pi live node value mismatch");
    assert!(
        !pi.contains("restart-pi-draft"),
        "database-only Pi draft was unexpectedly created in live config"
    );

    let opencode = ProviderService::read_live_settings(AppType::OpenCode)
        .expect("read isolated OpenCode config")
        .to_string();
    assert!(
        opencode.contains(expected),
        "OpenCode live node value mismatch"
    );
}

fn assert_db_templates(state: &AppState) {
    let template = ref_value();
    for (app, id, pointer) in [
        (
            AppType::Claude,
            "restart-claude-current",
            "/env/ANTHROPIC_AUTH_TOKEN",
        ),
        (
            AppType::Codex,
            "restart-codex-current",
            "/auth/OPENAI_API_KEY",
        ),
        (AppType::Pi, "restart-pi-live", "/apiKey"),
        (
            AppType::OpenCode,
            "restart-opencode-live",
            "/options/apiKey",
        ),
        (AppType::Pi, "restart-pi-draft", "/apiKey"),
    ] {
        let provider = state
            .db
            .get_provider_by_id(id, app.as_str())
            .expect("read stored provider")
            .expect("stored provider exists");
        assert_eq!(
            provider
                .settings_config
                .pointer(pointer)
                .and_then(serde_json::Value::as_str),
            Some(template.as_str()),
            "{}/{} lost its persisted reference template",
            app.as_str(),
            id
        );
    }
}
