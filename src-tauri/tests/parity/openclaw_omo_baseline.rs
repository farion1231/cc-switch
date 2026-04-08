use serde_json::json;

use cc_switch_lib::{
    bridges::{omo as omo_bridge, openclaw as openclaw_bridge},
    Provider,
};

use super::support::{create_empty_legacy_state, reset_test_fs, test_mutex};

fn seed_legacy_omo_state() -> cc_switch_lib::AppState {
    let state = create_empty_legacy_state();
    let mut provider = Provider::with_id(
        "omo-provider".to_string(),
        "OMO Provider".to_string(),
        json!({
            "agents": {
                "default": {
                    "model": "gpt-5"
                }
            }
        }),
        None,
    );
    provider.category = Some("omo".to_string());
    state
        .db
        .save_provider("opencode", &provider)
        .expect("save legacy omo provider");
    state
        .db
        .set_omo_provider_current("opencode", "omo-provider", "omo")
        .expect("set legacy current omo provider");

    let path = cc_switch_core::opencode_config::get_opencode_dir().join("oh-my-opencode.jsonc");
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create opencode dir");
    }
    std::fs::write(
        &path,
        serde_json::to_string_pretty(&json!({
            "agents": {
                "default": {
                    "model": "gpt-5"
                }
            }
        }))
        .expect("serialize omo config"),
    )
    .expect("write omo config");

    state
}

#[test]
fn openclaw_baseline_legacy_env_round_trip_is_stable() {
    let _guard = test_mutex().lock().unwrap_or_else(|err| err.into_inner());
    reset_test_fs();

    openclaw_bridge::legacy_set_env_from_core(cc_switch_core::openclaw_config::OpenClawEnvConfig {
        vars: std::collections::HashMap::from([(
            "ANTHROPIC_API_KEY".to_string(),
            serde_json::Value::String("test-key".to_string()),
        )]),
    })
    .expect("set openclaw env");

    let env = openclaw_bridge::legacy_get_env_as_core().expect("get openclaw env");
    assert_eq!(
        env.vars.get("ANTHROPIC_API_KEY"),
        Some(&serde_json::Value::String("test-key".to_string()))
    );
}

#[test]
fn omo_baseline_legacy_disable_clears_current_and_file() {
    let _guard = test_mutex().lock().unwrap_or_else(|err| err.into_inner());
    reset_test_fs();

    let state = seed_legacy_omo_state();
    omo_bridge::legacy_disable_standard_current(&state).expect("legacy disable current omo");

    let current =
        omo_bridge::legacy_get_standard_provider_id(&state).expect("legacy current omo provider");
    let file_exists = omo_bridge::standard_config_exists();

    assert!(current.is_none());
    assert!(!file_exists);
}
