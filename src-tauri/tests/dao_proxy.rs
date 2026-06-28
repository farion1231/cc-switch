use cc_switch_lib::proxy::types::{AppProxyConfig, GlobalProxyConfig};
use cc_switch_lib::{AppType, Database, Provider};
use serde_json::json;

fn seed_provider(db: &Database, id: &str, app_type: AppType) {
    let provider = Provider::with_id(
        id.to_string(),
        id.to_string(),
        json!({"env": {}}),
        None,
    );
    db.save_provider(app_type.as_str(), &provider)
        .expect("seed provider");
}

// === Global Proxy Config ===

#[tokio::test]
async fn get_global_proxy_config_returns_default() {
    let db = Database::memory().expect("create memory db");
    let config = db.get_global_proxy_config().await.expect("get global config");
    assert!(!config.proxy_enabled);
    assert_eq!(config.listen_address, "127.0.0.1");
    assert_eq!(config.listen_port, 15721);
    assert!(config.enable_logging);
}

#[tokio::test]
async fn update_and_retrieve_global_proxy_config() {
    let db = Database::memory().expect("create memory db");
    let _ = db.get_global_proxy_config().await.expect("seed rows");

    let updated = GlobalProxyConfig {
        proxy_enabled: true,
        listen_address: "0.0.0.0".to_string(),
        listen_port: 8080,
        enable_logging: false,
    };
    db.update_global_proxy_config(updated).await.expect("update config");

    let retrieved = db.get_global_proxy_config().await.expect("get config");
    assert!(retrieved.proxy_enabled);
    assert_eq!(retrieved.listen_address, "0.0.0.0");
    assert_eq!(retrieved.listen_port, 8080);
    assert!(!retrieved.enable_logging);
}

// === App Proxy Config ===

#[tokio::test]
async fn get_proxy_config_for_app_returns_default() {
    let db = Database::memory().expect("create memory db");
    let config = db
        .get_proxy_config_for_app("claude")
        .await
        .expect("get app config");
    assert_eq!(config.app_type, "claude");
    assert!(!config.enabled);
    assert!(!config.auto_failover_enabled);
}

#[tokio::test]
async fn update_and_retrieve_proxy_config_for_app() {
    let db = Database::memory().expect("create memory db");
    let _ = db
        .get_proxy_config_for_app("claude")
        .await
        .expect("seed rows");

    let updated = AppProxyConfig {
        app_type: "claude".to_string(),
        enabled: true,
        auto_failover_enabled: true,
        max_retries: 10,
        streaming_first_byte_timeout: 120,
        streaming_idle_timeout: 240,
        non_streaming_timeout: 900,
        circuit_failure_threshold: 6,
        circuit_success_threshold: 3,
        circuit_timeout_seconds: 120,
        circuit_error_rate_threshold: 0.8,
        circuit_min_requests: 20,
    };
    db.update_proxy_config_for_app(updated)
        .await
        .expect("update app config");

    let retrieved = db
        .get_proxy_config_for_app("claude")
        .await
        .expect("get app config");
    assert!(retrieved.enabled);
    assert!(retrieved.auto_failover_enabled);
    assert_eq!(retrieved.max_retries, 10);
    assert_eq!(retrieved.circuit_failure_threshold, 6);
}

// === Provider Health ===

#[tokio::test]
async fn get_provider_health_returns_healthy_default() {
    let db = Database::memory().expect("create memory db");
    let health = db
        .get_provider_health("unknown-provider", "claude")
        .await
        .expect("get health");
    assert!(health.is_healthy);
    assert_eq!(health.consecutive_failures, 0);
}

#[tokio::test]
async fn update_provider_health_success_resets_failures() {
    let db = Database::memory().expect("create memory db");
    seed_provider(&db, "p1", AppType::Claude);
    db.update_provider_health("p1", "claude", false, Some("error".to_string()))
        .await
        .expect("record failure");
    db.update_provider_health("p1", "claude", true, None)
        .await
        .expect("record success");

    let health = db
        .get_provider_health("p1", "claude")
        .await
        .expect("get health");
    assert!(health.is_healthy);
    assert_eq!(health.consecutive_failures, 0);
}

#[tokio::test]
async fn update_provider_health_failure_increments_count() {
    let db = Database::memory().expect("create memory db");
    seed_provider(&db, "p1", AppType::Claude);
    db.update_provider_health("p1", "claude", false, Some("timeout".to_string()))
        .await
        .expect("fail 1");
    db.update_provider_health("p1", "claude", false, Some("timeout".to_string()))
        .await
        .expect("fail 2");

    let health = db
        .get_provider_health("p1", "claude")
        .await
        .expect("get health");
    assert_eq!(health.consecutive_failures, 2);
}

#[tokio::test]
async fn update_provider_health_with_threshold_marks_unhealthy() {
    let db = Database::memory().expect("create memory db");
    seed_provider(&db, "p1", AppType::Claude);
    for _ in 0..2 {
        db.update_provider_health_with_threshold(
            "p1",
            "claude",
            false,
            Some("error".to_string()),
            2,
        )
        .await
        .expect("fail");
    }

    let health = db
        .get_provider_health("p1", "claude")
        .await
        .expect("get health");
    assert!(!health.is_healthy);
    assert_eq!(health.consecutive_failures, 2);
}

#[tokio::test]
async fn reset_provider_health_returns_to_default() {
    let db = Database::memory().expect("create memory db");
    seed_provider(&db, "p1", AppType::Claude);
    db.update_provider_health("p1", "claude", false, Some("error".to_string()))
        .await
        .expect("fail");
    db.reset_provider_health("p1", "claude")
        .await
        .expect("reset");

    let health = db
        .get_provider_health("p1", "claude")
        .await
        .expect("get health");
    assert!(health.is_healthy);
    assert_eq!(health.consecutive_failures, 0);
}

#[tokio::test]
async fn clear_all_provider_health_removes_all_records() {
    let db = Database::memory().expect("create memory db");
    seed_provider(&db, "p1", AppType::Claude);
    seed_provider(&db, "p2", AppType::Codex);
    db.update_provider_health("p1", "claude", false, None)
        .await
        .expect("fail p1");
    db.update_provider_health("p2", "codex", false, None)
        .await
        .expect("fail p2");
    db.clear_all_provider_health().await.expect("clear all");

    let h1 = db
        .get_provider_health("p1", "claude")
        .await
        .expect("get p1");
    let h2 = db
        .get_provider_health("p2", "codex")
        .await
        .expect("get p2");
    assert!(h1.is_healthy);
    assert_eq!(h1.consecutive_failures, 0);
    assert!(h2.is_healthy);
    assert_eq!(h2.consecutive_failures, 0);
}

// === Live Backup ===

#[tokio::test]
async fn has_any_live_backup_false_initially() {
    let db = Database::memory().expect("create memory db");
    assert!(!db.has_any_live_backup().await.expect("check backup"));
}

#[tokio::test]
async fn save_and_retrieve_live_backup() {
    let db = Database::memory().expect("create memory db");
    let config_json = r#"{"key":"value"}"#;
    db.save_live_backup("claude", config_json)
        .await
        .expect("save backup");

    let backup = db
        .get_live_backup("claude")
        .await
        .expect("get backup")
        .expect("backup exists");
    assert_eq!(backup.app_type, "claude");
    assert_eq!(backup.original_config, config_json);
}

#[tokio::test]
async fn get_live_backup_returns_none_if_missing() {
    let db = Database::memory().expect("create memory db");
    let backup = db.get_live_backup("claude").await.expect("get backup");
    assert!(backup.is_none());
}

#[tokio::test]
async fn delete_live_backup_removes_it() {
    let db = Database::memory().expect("create memory db");
    db.save_live_backup("claude", r#"{}"#)
        .await
        .expect("save backup");
    db.delete_live_backup("claude")
        .await
        .expect("delete backup");

    let backup = db.get_live_backup("claude").await.expect("get backup");
    assert!(backup.is_none());
}

#[tokio::test]
async fn has_any_live_backup_true_after_save() {
    let db = Database::memory().expect("create memory db");
    db.save_live_backup("claude", r#"{}"#)
        .await
        .expect("save backup");
    assert!(db.has_any_live_backup().await.expect("check backup"));
}

// === Live Takeover ===

#[tokio::test]
async fn is_live_takeover_active_false_initially() {
    let db = Database::memory().expect("create memory db");
    assert!(!db.is_live_takeover_active().await.expect("check takeover"));
}

// === Sync Flags ===

#[test]
fn get_and_set_proxy_flags_sync_round_trip() {
    let db = Database::memory().expect("create memory db");
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        db.get_proxy_config_for_app("claude")
            .await
            .expect("seed rows");
    });

    db.set_proxy_flags_sync("claude", true, true)
        .expect("set flags");
    let (enabled, failover) = db.get_proxy_flags_sync("claude");
    assert!(enabled);
    assert!(failover);
}
