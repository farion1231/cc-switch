use cc_switch_lib::{
    get_pricing_model_source_test_hook, set_pricing_model_source_test_hook, AppError,
};

#[path = "support.rs"]
mod support;
use support::{create_test_state, ensure_test_home, reset_test_fs, test_mutex};

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
