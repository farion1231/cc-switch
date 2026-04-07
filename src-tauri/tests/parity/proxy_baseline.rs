#![allow(clippy::await_holding_lock)]

use serde_json::json;

use super::support::{
    claude_switch_config, create_legacy_state_with_config, ensure_test_home, reset_test_fs,
    seed_claude_live, test_mutex,
};

async fn enable_auto_failover_legacy(state: &cc_switch_lib::AppState) -> Result<(), String> {
    let mut queue = state
        .db
        .get_failover_queue("claude")
        .map_err(|e| e.to_string())?;

    if queue.is_empty() {
        let current_id = state
            .db
            .get_current_provider("claude")
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "missing current provider".to_string())?;

        state
            .db
            .add_to_failover_queue("claude", &current_id)
            .map_err(|e| e.to_string())?;
        queue = state
            .db
            .get_failover_queue("claude")
            .map_err(|e| e.to_string())?;
    }

    let p1_provider_id = queue
        .first()
        .map(|item| item.provider_id.clone())
        .ok_or_else(|| "empty failover queue".to_string())?;

    let mut config = state
        .db
        .get_proxy_config_for_app("claude")
        .await
        .map_err(|e| e.to_string())?;
    config.auto_failover_enabled = true;
    state
        .db
        .update_proxy_config_for_app(config)
        .await
        .map_err(|e| e.to_string())?;
    state
        .proxy_service
        .switch_proxy_target("claude", &p1_provider_id)
        .await
}

#[tokio::test(flavor = "current_thread")]
async fn proxy_baseline_legacy_takeover_start_stop_is_stable() {
    let _guard = test_mutex().lock().unwrap_or_else(|err| err.into_inner());

    reset_test_fs();
    let _home = ensure_test_home();
    seed_claude_live();
    let state = create_legacy_state_with_config(&claude_switch_config());

    state
        .proxy_service
        .set_takeover_for_app("claude", true)
        .await
        .expect("enable takeover");

    let running_status = state
        .proxy_service
        .get_status()
        .await
        .expect("proxy status");
    let takeover_status = state
        .proxy_service
        .get_takeover_status()
        .await
        .expect("takeover status");
    let live_during: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(cc_switch_lib::get_claude_settings_path())
            .expect("read live config during takeover"),
    )
    .expect("parse live during takeover");

    state
        .proxy_service
        .stop_with_restore()
        .await
        .expect("stop proxy");

    let stopped_status = state
        .proxy_service
        .get_status()
        .await
        .expect("proxy stopped status");
    let takeover_after = state
        .proxy_service
        .get_takeover_status()
        .await
        .expect("takeover status after stop");
    let live_after: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(cc_switch_lib::get_claude_settings_path())
            .expect("read live config after stop"),
    )
    .expect("parse live after stop");

    assert_eq!(
        json!({
            "running": running_status.running,
            "address": running_status.address,
            "takeover": takeover_status.claude,
            "liveToken": live_during["env"]["ANTHROPIC_API_KEY"],
            "liveBaseUrl": live_during["env"]["ANTHROPIC_BASE_URL"],
            "stopped": stopped_status.running,
            "restored": takeover_after.claude,
            "restoredToken": live_after["env"]["ANTHROPIC_API_KEY"],
        }),
        json!({
            "running": true,
            "address": "127.0.0.1",
            "takeover": true,
            "liveToken": "PROXY_MANAGED",
            "liveBaseUrl": "http://127.0.0.1:15721",
            "stopped": false,
            "restored": false,
            "restoredToken": "legacy-key",
        })
    );
}

#[tokio::test(flavor = "current_thread")]
async fn proxy_baseline_legacy_auto_failover_enable_is_stable() {
    let _guard = test_mutex().lock().unwrap_or_else(|err| err.into_inner());

    reset_test_fs();
    let _home = ensure_test_home();
    let state = create_legacy_state_with_config(&claude_switch_config());

    enable_auto_failover_legacy(&state)
        .await
        .expect("enable auto failover");

    let config = state
        .db
        .get_proxy_config_for_app("claude")
        .await
        .expect("proxy config");
    let queue = state
        .db
        .get_failover_queue("claude")
        .expect("failover queue");
    let current = state
        .db
        .get_current_provider("claude")
        .expect("current provider");

    assert_eq!(
        json!({
            "enabled": config.auto_failover_enabled,
            "queue": queue,
            "current": current,
        }),
        json!({
            "enabled": true,
            "queue": [{
                "providerId": "old-provider",
                "providerName": "Legacy Claude",
                "sortIndex": null
            }],
            "current": "old-provider",
        })
    );
}
