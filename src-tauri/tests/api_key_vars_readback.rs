//! Regression coverage for client configuration readback after an API-key
//! variable rotation and process restart.

use cc_switch_lib::{
    api_key_vars::{
        self, resolve,
        runner::{ApiKeyRefreshConfig, RefreshOutput, ScriptRun},
    },
    AppType, Provider, ProviderService,
};
use serde_json::json;

#[path = "support.rs"]
mod support;
use support::{create_test_state, ensure_test_home, reset_test_fs, test_mutex};

const VAR: &str = "IT_READBACK_KEY";
const FIRST: &str = "readback-key-first";
const SECOND: &str = "readback-key-second";

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
            api_key: value.into(),
            expires_at: None,
            refresh_after: None,
            message: None,
        },
        resolved_file: "index.mjs".into(),
    })
}

fn provider(app: AppType, id: &str) -> Provider {
    let settings = match app {
        AppType::Pi => json!({
            "name": id,
            "baseUrl": "https://api.example.test/v1",
            "apiKey": format!("{{{{{VAR}}}}}"),
            "api": "openai-completions",
            "models": [{"id": "model-a", "apiKey": format!("{{{{{VAR}}}}}")}],
        }),
        AppType::Mcode => json!({
            "name": id,
            "kind": "custom",
            "enabled": true,
            "api": "openai-completions",
            "options": {
                "baseURL": "https://api.example.test/v1",
                "apiKey": format!("{{{{{VAR}}}}}"),
            },
            "models": {"model-a": {"name": "Model A", "api_key": format!("{{{{{VAR}}}}}")}},
        }),
        _ => unreachable!("readback fixture is additive only"),
    };
    let mut provider = Provider::with_id(id.into(), id.into(), settings, None);
    provider.icon = Some("custom-icon".into());
    provider.notes = Some("keep metadata fields".into());
    provider
}

fn row(state: &cc_switch_lib::AppState, app: AppType, id: &str) -> Provider {
    state
        .db
        .get_provider_by_id(id, app.as_str())
        .expect("read provider")
        .expect("provider exists")
}

#[test]
fn derived_readback_keeps_templates_after_rotation_and_restart_but_user_edits_detach() {
    let _guard = test_mutex()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    ensure_test_home();
    reset_test_fs();
    let state = create_test_state().expect("state");
    state
        .db
        .upsert_api_key_var(None, VAR, &refresh_config())
        .expect("create variable");
    api_key_vars::commit_result(&state, VAR, success(FIRST)).expect("seed first value");
    api_key_vars::commit_result(&state, VAR, success(SECOND)).expect("rotate value");

    for (app, id) in [
        (AppType::Pi, "pi-readback"),
        (AppType::Mcode, "mcode-readback"),
    ] {
        ProviderService::add(&state, app.clone(), provider(app, id), false)
            .expect("store template provider");
    }

    // Simulate a fresh process: only the current value is loaded from SQLite,
    // so FIRST is no longer available as a cache-history match.
    resolve::cache_load([(VAR.to_owned(), true, Some(SECOND.to_owned()))]);

    let mut pi = row(&state, AppType::Pi, "pi-readback");
    pi.settings_config["apiKey"] = json!(FIRST);
    pi.settings_config["models"][0]["apiKey"] = json!(FIRST);
    state
        .db
        .save_provider(AppType::Pi.as_str(), &pi)
        .expect("Pi client readback preserves stored template");
    let pi = row(&state, AppType::Pi, "pi-readback");
    assert_eq!(
        pi.settings_config["apiKey"],
        json!(format!("{{{{{VAR}}}}}"))
    );
    assert_eq!(
        pi.settings_config["models"][0]["apiKey"],
        json!(format!("{{{{{VAR}}}}}"))
    );
    assert_eq!(pi.icon.as_deref(), Some("custom-icon"));
    assert_eq!(pi.notes.as_deref(), Some("keep metadata fields"));

    let mut mcode = row(&state, AppType::Mcode, "mcode-readback");
    mcode.settings_config["options"]["apiKey"] = json!(FIRST);
    mcode.settings_config["models"]["model-a"]["api_key"] = json!(FIRST);
    state
        .db
        .update_provider_settings_config(
            AppType::Mcode.as_str(),
            "mcode-readback",
            &mcode.settings_config,
        )
        .expect("Mcode client readback preserves stored template");
    let mcode = row(&state, AppType::Mcode, "mcode-readback");
    assert_eq!(
        mcode.settings_config["options"]["apiKey"],
        json!(format!("{{{{{VAR}}}}}"))
    );
    assert_eq!(
        mcode.settings_config["models"]["model-a"]["api_key"],
        json!(format!("{{{{{VAR}}}}}"))
    );

    let mut explicit_edit = pi;
    explicit_edit.settings_config["apiKey"] = json!(FIRST);
    explicit_edit.settings_config["models"][0]["apiKey"] = json!(FIRST);
    ProviderService::update(&state, AppType::Pi, None, explicit_edit)
        .expect("explicit user literal must detach template");
    let detached = row(&state, AppType::Pi, "pi-readback");
    assert_eq!(detached.settings_config["apiKey"], json!(FIRST));
    assert_eq!(
        detached.settings_config["models"][0]["apiKey"],
        json!(FIRST)
    );
}
