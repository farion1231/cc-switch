use cc_switch_lib::{
    get_default_cost_multiplier_test_hook, get_pricing_model_source_test_hook,
    set_default_cost_multiplier_test_hook, set_pricing_model_source_test_hook, AppError,
};
use serde_json::json;

#[path = "support.rs"]
mod support;
use support::{create_test_state, ensure_test_home, reset_test_fs, test_mutex};

// 测试使用 Mutex 进行串行化，跨 await 持锁是预期行为
#[allow(clippy::await_holding_lock)]
#[tokio::test]
async fn default_cost_multiplier_commands_round_trip() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let _home = ensure_test_home();

    let state = create_test_state().expect("create test state");

    let default = get_default_cost_multiplier_test_hook(&state, "claude")
        .await
        .expect("read default multiplier");
    assert_eq!(default, "1");

    set_default_cost_multiplier_test_hook(&state, "claude", "1.5")
        .await
        .expect("set multiplier");
    let updated = get_default_cost_multiplier_test_hook(&state, "claude")
        .await
        .expect("read updated multiplier");
    assert_eq!(updated, "1.5");

    let err = set_default_cost_multiplier_test_hook(&state, "claude", "not-a-number")
        .await
        .expect_err("invalid multiplier should error");
    // 错误已改为 Localized 类型（支持 i18n）
    match err {
        AppError::Localized { key, .. } => {
            assert_eq!(key, "error.invalidMultiplier");
        }
        other => panic!("expected localized error, got {other:?}"),
    }
}

// 测试使用 Mutex 进行串行化，跨 await 持锁是预期行为
#[allow(clippy::await_holding_lock)]
#[tokio::test]
async fn pricing_model_source_commands_round_trip() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let _home = ensure_test_home();

    let state = create_test_state().expect("create test state");

    let default = get_pricing_model_source_test_hook(&state, "claude")
        .await
        .expect("read default pricing model source");
    assert_eq!(default, "response");

    set_pricing_model_source_test_hook(&state, "claude", "request")
        .await
        .expect("set pricing model source");
    let updated = get_pricing_model_source_test_hook(&state, "claude")
        .await
        .expect("read updated pricing model source");
    assert_eq!(updated, "request");

    let err = set_pricing_model_source_test_hook(&state, "claude", "invalid")
        .await
        .expect_err("invalid pricing model source should error");
    // 错误已改为 Localized 类型（支持 i18n）
    match err {
        AppError::Localized { key, .. } => {
            assert_eq!(key, "error.invalidPricingMode");
        }
        other => panic!("expected localized error, got {other:?}"),
    }
}

#[test]
fn read_qwen_live_accepts_settings_json_without_env_file() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();

    let qwen_dir = home.join(".qwen");
    std::fs::create_dir_all(&qwen_dir).expect("create qwen dir");
    std::fs::write(
        qwen_dir.join("settings.json"),
        serde_json::to_string_pretty(&json!({
            "env": {
                "OPENAI_API_KEY": "settings-only-key",
                "OPENAI_BASE_URL": "https://dashscope-intl.aliyuncs.com/compatible-mode/v1"
            },
            "security": {
                "auth": {
                    "selectedType": "openai"
                }
            },
            "model": {
                "name": "qwen3-coder-plus"
            }
        }))
        .expect("serialize qwen settings"),
    )
    .expect("seed qwen settings");

    let state = create_test_state().expect("create test state");
    let live = state
        .proxy_service
        .read_qwen_live_test_hook()
        .expect("settings.json-only qwen config should be readable");

    assert_eq!(
        live["env"]["OPENAI_API_KEY"],
        json!("settings-only-key"),
        "Qwen live read should fall back to settings.json when .env is absent"
    );
    assert_eq!(
        live["config"]["model"]["name"],
        json!("qwen3-coder-plus"),
        "Qwen live read should preserve provider config fields from settings.json"
    );
}
