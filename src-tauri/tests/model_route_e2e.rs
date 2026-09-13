//! Model routing end-to-end test
//!
//! Verifies that a UniversalProvider in the database is correctly picked up
//! by ModelRouter when a matching model name comes through.
//!
//! Run: cd src-tauri && cargo test model_route_e2e -- --test-threads=1

use cc_switch_lib::ModelRouter;
use cc_switch_lib::{
    ClaudeModelConfig, CodexModelConfig, GeminiModelConfig, UniversalProvider,
    UniversalProviderModels,
};

#[path = "support.rs"]
mod support;
use support::{create_test_state, ensure_test_home, reset_test_fs, test_mutex};

/// Helper: build a UniversalProvider with given model configs
fn make_up(id: &str, name: &str, models: UniversalProviderModels) -> UniversalProvider {
    UniversalProvider {
        id: id.to_string(),
        name: name.to_string(),
        provider_type: "custom".to_string(),
        apps: Default::default(),
        base_url: "https://api.test.com".to_string(),
        api_key: "sk-test-key".to_string(),
        models,
        enabled: true,
        routes: vec![],
        website_url: None,
        notes: None,
        icon: None,
        icon_color: None,
        meta: None,
        created_at: None,
        sort_index: None,
    }
}

/// Test 1: Seed a UniversalProvider via DB, then route a matching model to it
#[test]
fn db_seeded_up_routes_correctly() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    ensure_test_home();

    let state = create_test_state().expect("create test state");
    let db = &state.db;

    // Seed a UniversalProvider with sonnet model
    let up = make_up(
        "my-custom-up",
        "My Custom Provider",
        UniversalProviderModels {
            claude: Some(ClaudeModelConfig {
                model: None,
                haiku_model: None,
                sonnet_model: Some("claude-sonnet-4-6".to_string()),
                opus_model: None,
            }),
            codex: None,
            gemini: None,
        },
    );
    db.save_universal_provider(&up).expect("save UP");

    // Verify it's in the DB
    let providers = db.get_all_universal_providers().expect("get UPs");
    assert_eq!(providers.len(), 1);
    assert!(providers.contains_key("my-custom-up"));

    // Route a matching model
    let result = ModelRouter::match_model("claude-sonnet-4-6", &providers, "claude");
    assert!(result.is_some(), "should match sonnet model");
    assert_eq!(result.unwrap().id, "my-custom-up");

    // Route a non-matching model
    let result = ModelRouter::match_model("claude-opus-4-8", &providers, "claude");
    assert!(result.is_none(), "should NOT match opus model");
}

/// Test 2: Multiple UniversalProviders in DB, exact match wins
#[test]
fn db_multiple_ups_exact_wins() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    ensure_test_home();

    let state = create_test_state().expect("create test state");
    let db = &state.db;

    // UP with wildcard
    let wild = make_up(
        "wildcard-up",
        "Wildcard UP",
        UniversalProviderModels {
            codex: Some(CodexModelConfig {
                model: Some("gpt-4*".to_string()),
                reasoning_effort: None,
            }),
            ..Default::default()
        },
    );
    // UP with exact match
    let exact = make_up(
        "exact-up",
        "Exact UP",
        UniversalProviderModels {
            codex: Some(CodexModelConfig {
                model: Some("gpt-4.5-preview".to_string()),
                reasoning_effort: None,
            }),
            ..Default::default()
        },
    );

    db.save_universal_provider(&wild).expect("save wildcard UP");
    db.save_universal_provider(&exact).expect("save exact UP");

    let providers = db.get_all_universal_providers().expect("get UPs");
    assert_eq!(providers.len(), 2);

    // Exact match should win over wildcard
    let result = ModelRouter::match_model("gpt-4.5-preview", &providers, "codex");
    assert!(result.is_some());
    assert_eq!(
        result.unwrap().id,
        "exact-up",
        "exact match should win over wildcard"
    );
}

/// Test 3: Empty DB returns no match
#[test]
fn db_empty_no_match() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    ensure_test_home();

    let state = create_test_state().expect("create test state");
    let db = &state.db;

    let providers = db.get_all_universal_providers().expect("get UPs");
    assert!(providers.is_empty());

    let result = ModelRouter::match_model("any-model", &providers, "claude");
    assert!(result.is_none());
}

/// Test 4: Gemini wildcard routing via DB
#[test]
fn db_gemini_wildcard_routing() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    ensure_test_home();

    let state = create_test_state().expect("create test state");
    let db = &state.db;

    let up = make_up(
        "gemini-up",
        "Gemini Provider",
        UniversalProviderModels {
            gemini: Some(GeminiModelConfig {
                model: Some("gemini-2*".to_string()),
            }),
            ..Default::default()
        },
    );
    db.save_universal_provider(&up).expect("save UP");

    let providers = db.get_all_universal_providers().expect("get UPs");

    for model in &[
        "gemini-2.0-flash",
        "gemini-2.5-pro",
        "gemini-2.0-flash-lite",
    ] {
        let result = ModelRouter::match_model(model, &providers, "gemini");
        assert!(result.is_some(), "should match {}", model);
        assert_eq!(result.unwrap().id, "gemini-up");
    }
}
