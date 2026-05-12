use std::sync::Arc;

use cc_switch_lib::{delete_model_pricing, get_model_pricing, AppState, Database};
use tauri::Manager;

fn build_test_app(db: Arc<Database>) -> tauri::App<tauri::test::MockRuntime> {
    tauri::test::mock_builder()
        .manage(AppState::new(db))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("build test app")
}

#[test]
fn get_model_pricing_does_not_restore_deleted_default_pricing() {
    let db = Arc::new(Database::memory().expect("create memory db"));
    let app = build_test_app(db);
    let state = app.state::<AppState>();
    let model_id = "claude-sonnet-4-5-20250929".to_string();

    let seeded_pricing = get_model_pricing(state.clone()).expect("list seeded pricing");
    assert!(
        seeded_pricing
            .iter()
            .any(|pricing| pricing.model_id == model_id),
        "default pricing should exist before deletion"
    );

    delete_model_pricing(state.clone(), model_id.clone()).expect("delete default pricing");

    let pricing_after_delete = get_model_pricing(state).expect("list pricing after deletion");
    assert!(
        !pricing_after_delete
            .iter()
            .any(|pricing| pricing.model_id == model_id),
        "deleted default pricing should not be restored by listing pricing"
    );
}
