use serde_json::json;

use cc_switch_lib::{
    bridges::{omo as omo_bridge, openclaw as openclaw_bridge},
    Provider,
};

use super::support::{
    create_empty_core_state, create_empty_legacy_state, reset_test_fs, test_mutex,
};

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

fn seed_core_omo_state() {
    let state = create_empty_core_state();
    let mut provider = cc_switch_core::Provider::with_id(
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
        .expect("save core omo provider");
    state
        .db
        .set_omo_provider_current("opencode", "omo-provider", "omo")
        .expect("set core current omo provider");

    let path = cc_switch_core::opencode_config::get_opencode_dir().join("oh-my-opencode.jsonc");
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create core opencode dir");
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
    .expect("write core omo config");
}

#[test]
fn openclaw_parity_env_round_trip_matches_legacy() {
    let _guard = test_mutex().lock().unwrap_or_else(|err| err.into_inner());

    let payload = cc_switch_core::openclaw_config::OpenClawEnvConfig {
        vars: std::collections::HashMap::from([
            (
                "ANTHROPIC_API_KEY".to_string(),
                serde_json::Value::String("legacy-anthropic".to_string()),
            ),
            (
                "OPENAI_API_KEY".to_string(),
                serde_json::Value::String("legacy-openai".to_string()),
            ),
        ]),
    };

    reset_test_fs();
    openclaw_bridge::legacy_set_env_from_core(payload.clone()).expect("legacy set env");
    let legacy_env = openclaw_bridge::legacy_get_env_as_core().expect("legacy get env");
    let legacy_file = openclaw_bridge::read_config_file().expect("legacy openclaw file");

    reset_test_fs();
    openclaw_bridge::set_env_from_core(payload.clone()).expect("core set env");
    let core_env = openclaw_bridge::get_env_as_core().expect("core get env");
    let core_file = openclaw_bridge::read_config_file().expect("core openclaw file");

    assert_eq!(
        serde_json::to_value(core_env).expect("core env json"),
        serde_json::to_value(legacy_env).expect("legacy env json")
    );
    assert_eq!(core_file, legacy_file);
}

#[test]
fn omo_parity_disable_matches_legacy() {
    let _guard = test_mutex().lock().unwrap_or_else(|err| err.into_inner());

    reset_test_fs();
    let legacy_state = seed_legacy_omo_state();
    omo_bridge::legacy_disable_standard_current(&legacy_state).expect("legacy disable current omo");
    let legacy_current = omo_bridge::legacy_get_standard_provider_id(&legacy_state)
        .expect("legacy current omo provider");
    let legacy_file_exists = omo_bridge::standard_config_exists();

    reset_test_fs();
    seed_core_omo_state();
    omo_bridge::disable_standard_current().expect("core disable current omo");
    let core_current = omo_bridge::get_standard_provider_id().expect("core current omo provider");
    let core_file_exists = omo_bridge::standard_config_exists();

    assert_eq!(core_current, legacy_current);
    assert_eq!(core_file_exists, legacy_file_exists);
}
