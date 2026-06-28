use cc_switch_lib::{AppType, Provider, ProviderService};
use serde_json::json;

#[path = "support.rs"]
mod support;
use support::{create_test_state, ensure_test_home, reset_test_fs, test_mutex};

fn seed_provider(state: &cc_switch_lib::AppState, id: &str, app_type: AppType) {
    let provider = Provider::with_id(
        id.to_string(),
        id.to_string(),
        json!({"env": {}}),
        None,
    );
    state
        .db
        .save_provider(app_type.as_str(), &provider)
        .expect("seed provider");
}

#[test]
fn get_custom_endpoints_returns_empty_for_unknown_provider() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    ensure_test_home();

    let state = create_test_state().expect("create state");
    let result = ProviderService::get_custom_endpoints(&state, AppType::Claude, "nonexistent-id")
        .expect("should succeed");
    assert!(
        result.is_empty(),
        "nonexistent provider should return empty list"
    );
}

#[test]
fn add_custom_endpoint_with_empty_url_returns_error() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    ensure_test_home();

    let state = create_test_state().expect("create state");
    let result =
        ProviderService::add_custom_endpoint(&state, AppType::Claude, "provider-1", "".to_string());
    assert!(result.is_err(), "empty URL should be rejected");
}

#[test]
fn add_custom_endpoint_with_whitespace_only_url_returns_error() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    ensure_test_home();

    let state = create_test_state().expect("create state");
    let result = ProviderService::add_custom_endpoint(
        &state,
        AppType::Claude,
        "provider-1",
        "   ".to_string(),
    );
    assert!(result.is_err(), "whitespace-only URL should be rejected");
}

#[test]
fn add_custom_endpoint_with_trailing_slash_normalizes_url() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    ensure_test_home();

    let state = create_test_state().expect("create state");
    seed_provider(&state, "prov-normalize", AppType::Claude);
    ProviderService::add_custom_endpoint(
        &state,
        AppType::Claude,
        "prov-normalize",
        "https://api.example.com/v1/".to_string(),
    )
    .expect("add with trailing slash should succeed");

    let endpoints =
        ProviderService::get_custom_endpoints(&state, AppType::Claude, "prov-normalize")
            .expect("get_custom_endpoints should succeed");

    assert!(!endpoints.is_empty(), "endpoint should have been added");
    assert!(
        !endpoints[0].url.ends_with('/'),
        "stored URL should not have trailing slash, got: {}",
        endpoints[0].url
    );
    assert_eq!(
        endpoints[0].url, "https://api.example.com/v1",
        "URL should be normalized without trailing slash"
    );
}

#[test]
fn add_custom_endpoint_succeeds_for_new_provider_id() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    ensure_test_home();

    let state = create_test_state().expect("create state");
    seed_provider(&state, "brand-new-provider", AppType::Claude);
    let result = ProviderService::add_custom_endpoint(
        &state,
        AppType::Claude,
        "brand-new-provider",
        "https://new.provider.api/v1".to_string(),
    );
    assert!(
        result.is_ok(),
        "adding endpoint to new provider should succeed: {result:?}"
    );
}

#[test]
fn get_custom_endpoints_returns_endpoint_after_add() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    ensure_test_home();

    let state = create_test_state().expect("create state");
    seed_provider(&state, "prov-get-after-add", AppType::Claude);
    ProviderService::add_custom_endpoint(
        &state,
        AppType::Claude,
        "prov-get-after-add",
        "https://endpoint.test/api".to_string(),
    )
    .expect("add should succeed");

    let endpoints =
        ProviderService::get_custom_endpoints(&state, AppType::Claude, "prov-get-after-add")
            .expect("get should succeed");

    assert_eq!(endpoints.len(), 1, "should have exactly one endpoint");
    assert_eq!(endpoints[0].url, "https://endpoint.test/api");
}

#[test]
fn remove_custom_endpoint_removes_added_endpoint() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    ensure_test_home();

    let state = create_test_state().expect("create state");
    let url = "https://removable.endpoint/api";
    seed_provider(&state, "prov-remove", AppType::Claude);
    ProviderService::add_custom_endpoint(&state, AppType::Claude, "prov-remove", url.to_string())
        .expect("add should succeed");

    let before = ProviderService::get_custom_endpoints(&state, AppType::Claude, "prov-remove")
        .expect("get after add");
    assert_eq!(before.len(), 1, "should have one endpoint before removal");

    ProviderService::remove_custom_endpoint(
        &state,
        AppType::Claude,
        "prov-remove",
        url.to_string(),
    )
    .expect("remove should succeed");

    let after = ProviderService::get_custom_endpoints(&state, AppType::Claude, "prov-remove")
        .expect("get after remove");
    assert!(after.is_empty(), "should have no endpoints after removal");
}

#[test]
fn remove_custom_endpoint_with_trailing_slash_normalizes_before_remove() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    ensure_test_home();

    let state = create_test_state().expect("create state");
    let url = "https://trailing.slash/api";
    seed_provider(&state, "prov-trail", AppType::Claude);
    ProviderService::add_custom_endpoint(&state, AppType::Claude, "prov-trail", url.to_string())
        .expect("add should succeed");

    ProviderService::remove_custom_endpoint(
        &state,
        AppType::Claude,
        "prov-trail",
        format!("{url}/"),
    )
    .expect("remove with trailing slash should succeed");

    let after = ProviderService::get_custom_endpoints(&state, AppType::Claude, "prov-trail")
        .expect("get after remove");
    assert!(
        after.is_empty(),
        "endpoint should be removed even with trailing slash in remove call"
    );
}

#[test]
fn remove_custom_endpoint_nonexistent_url_is_noop() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    ensure_test_home();

    let state = create_test_state().expect("create state");
    let result = ProviderService::remove_custom_endpoint(
        &state,
        AppType::Claude,
        "any-provider",
        "https://does.not.exist/api".to_string(),
    );
    assert!(
        result.is_ok(),
        "removing nonexistent endpoint should be a no-op: {result:?}"
    );
}

#[test]
fn update_endpoint_last_used_sets_timestamp() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    ensure_test_home();

    let state = create_test_state().expect("create state");
    let url = "https://update.last.used/api";
    seed_provider(&state, "prov-update-ts", AppType::Claude);
    ProviderService::add_custom_endpoint(
        &state,
        AppType::Claude,
        "prov-update-ts",
        url.to_string(),
    )
    .expect("add should succeed");

    let before = ProviderService::get_custom_endpoints(&state, AppType::Claude, "prov-update-ts")
        .expect("get before update_last_used");
    assert!(
        before[0].last_used.is_none(),
        "last_used should be None before first use"
    );

    ProviderService::update_endpoint_last_used(
        &state,
        AppType::Claude,
        "prov-update-ts",
        url.to_string(),
    )
    .expect("update_endpoint_last_used should succeed");

    let after = ProviderService::get_custom_endpoints(&state, AppType::Claude, "prov-update-ts")
        .expect("get after update_last_used");
    assert!(
        after[0].last_used.is_some(),
        "last_used should be set after update_endpoint_last_used"
    );
    assert!(
        after[0].last_used.unwrap() > 0,
        "last_used timestamp should be positive"
    );
}

#[test]
fn multiple_endpoints_can_be_added_to_same_provider() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    ensure_test_home();

    let state = create_test_state().expect("create state");
    let provider_id = "prov-multi";
    seed_provider(&state, provider_id, AppType::Claude);

    for i in 0..3 {
        ProviderService::add_custom_endpoint(
            &state,
            AppType::Claude,
            provider_id,
            format!("https://endpoint-{i}.test/api"),
        )
        .expect("add should succeed");
    }

    let endpoints = ProviderService::get_custom_endpoints(&state, AppType::Claude, provider_id)
        .expect("get should succeed");
    assert_eq!(endpoints.len(), 3, "should have all 3 endpoints");
}

#[test]
fn get_custom_endpoints_results_have_required_fields() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    ensure_test_home();

    let state = create_test_state().expect("create state");
    seed_provider(&state, "prov-fields", AppType::Claude);
    ProviderService::add_custom_endpoint(
        &state,
        AppType::Claude,
        "prov-fields",
        "https://fields.test/api".to_string(),
    )
    .expect("add should succeed");

    let endpoints = ProviderService::get_custom_endpoints(&state, AppType::Claude, "prov-fields")
        .expect("get should succeed");

    assert_eq!(endpoints.len(), 1);
    let ep = &endpoints[0];
    assert_eq!(ep.url, "https://fields.test/api", "url field should match");
    assert!(ep.added_at > 0, "added_at should be a positive timestamp");
}

#[test]
fn codex_provider_also_supports_custom_endpoints() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    ensure_test_home();

    let state = create_test_state().expect("create state");
    seed_provider(&state, "codex-prov", AppType::Codex);
    ProviderService::add_custom_endpoint(
        &state,
        AppType::Codex,
        "codex-prov",
        "https://codex.endpoint/v1".to_string(),
    )
    .expect("add to Codex provider should succeed");

    let endpoints = ProviderService::get_custom_endpoints(&state, AppType::Codex, "codex-prov")
        .expect("get should succeed");
    assert_eq!(
        endpoints.len(),
        1,
        "Codex provider should have one endpoint"
    );
    assert_eq!(endpoints[0].url, "https://codex.endpoint/v1");
}
