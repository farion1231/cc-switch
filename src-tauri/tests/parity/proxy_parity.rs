#![allow(clippy::await_holding_lock)]

use serde_json::json;

use cc_switch_lib::bridges::proxy as proxy_bridge;

use super::support::{
    claude_switch_config, create_core_state_with_config, create_legacy_state_with_config,
    ensure_test_home, reset_test_fs, seed_claude_live, test_mutex,
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

fn read_claude_live() -> serde_json::Value {
    serde_json::from_str(
        &std::fs::read_to_string(cc_switch_lib::get_claude_settings_path())
            .expect("read claude live config"),
    )
    .expect("parse claude live config")
}

#[tokio::test(flavor = "current_thread")]
async fn proxy_parity_takeover_start_stop_matches_legacy() {
    let _guard = test_mutex().lock().unwrap_or_else(|err| err.into_inner());

    reset_test_fs();
    let _home = ensure_test_home();
    seed_claude_live();
    let legacy_state = create_legacy_state_with_config(&claude_switch_config());
    legacy_state
        .proxy_service
        .set_takeover_for_app("claude", true)
        .await
        .expect("legacy enable takeover");
    let legacy_during = json!({
        "status": legacy_state.proxy_service.get_status().await.expect("legacy status"),
        "takeover": legacy_state.proxy_service.get_takeover_status().await.expect("legacy takeover"),
        "live": read_claude_live(),
    });
    legacy_state
        .proxy_service
        .stop_with_restore()
        .await
        .expect("legacy stop");
    let legacy_after = json!({
        "status": legacy_state.proxy_service.get_status().await.expect("legacy status after stop"),
        "takeover": legacy_state.proxy_service.get_takeover_status().await.expect("legacy takeover after stop"),
        "live": read_claude_live(),
    });
    let legacy = json!({
        "during": {
            "running": legacy_during["status"]["running"],
            "address": legacy_during["status"]["address"],
            "takeover": legacy_during["takeover"],
            "live": legacy_during["live"],
        },
        "after": {
            "running": legacy_after["status"]["running"],
            "takeover": legacy_after["takeover"],
            "live": legacy_after["live"],
        }
    });

    reset_test_fs();
    let _home = ensure_test_home();
    seed_claude_live();
    let _core_state = create_core_state_with_config(&claude_switch_config());
    proxy_bridge::set_proxy_takeover_for_app("claude", true)
        .await
        .expect("core enable takeover");
    let core_during = json!({
        "status": proxy_bridge::get_proxy_status().await.expect("core status"),
        "takeover": proxy_bridge::get_proxy_takeover_status().await.expect("core takeover"),
        "live": read_claude_live(),
    });
    proxy_bridge::stop_proxy_with_restore()
        .await
        .expect("core stop");
    let core_after = json!({
        "status": proxy_bridge::get_proxy_status().await.expect("core status after stop"),
        "takeover": proxy_bridge::get_proxy_takeover_status().await.expect("core takeover after stop"),
        "live": read_claude_live(),
    });
    let core = json!({
        "during": {
            "running": core_during["status"]["running"],
            "address": core_during["status"]["address"],
            "takeover": core_during["takeover"],
            "live": core_during["live"],
        },
        "after": {
            "running": core_after["status"]["running"],
            "takeover": core_after["takeover"],
            "live": core_after["live"],
        }
    });

    assert_eq!(core, legacy);
}

#[tokio::test(flavor = "current_thread")]
async fn proxy_parity_auto_failover_enable_matches_legacy() {
    let _guard = test_mutex().lock().unwrap_or_else(|err| err.into_inner());

    reset_test_fs();
    let _home = ensure_test_home();
    let legacy_state = create_legacy_state_with_config(&claude_switch_config());
    enable_auto_failover_legacy(&legacy_state)
        .await
        .expect("legacy auto failover enable");
    let legacy = json!({
        "config": legacy_state.db.get_proxy_config_for_app("claude").await.expect("legacy config"),
        "queue": legacy_state.db.get_failover_queue("claude").expect("legacy queue"),
        "current": legacy_state.db.get_current_provider("claude").expect("legacy current"),
    });

    reset_test_fs();
    let _home = ensure_test_home();
    let core_state = create_core_state_with_config(&claude_switch_config());
    proxy_bridge::set_auto_failover_enabled("claude", true)
        .await
        .expect("core auto failover enable");
    let core = json!({
        "config": proxy_bridge::get_proxy_config_for_app("claude").await.expect("core config"),
        "queue": proxy_bridge::get_failover_queue("claude").await.expect("core queue"),
        "current": core_state.db.get_current_provider("claude").expect("core current"),
    });

    assert_eq!(core, legacy);
}
