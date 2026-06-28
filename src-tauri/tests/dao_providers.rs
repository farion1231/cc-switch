use cc_switch_lib::{Database, Provider};
use serde_json::json;

fn make_provider(id: &str, name: &str) -> Provider {
    Provider::with_id(
        id.to_string(),
        name.to_string(),
        json!({ "env": { "ANTHROPIC_API_KEY": format!("sk-{id}") } }),
        None,
    )
}

fn make_provider_with_url(id: &str, name: &str, url: &str) -> Provider {
    Provider::with_id(
        id.to_string(),
        name.to_string(),
        json!({ "env": { "ANTHROPIC_API_KEY": format!("sk-{id}") } }),
        Some(url.to_string()),
    )
}

// ==================== get_all_providers ====================

#[test]
fn test_get_all_providers_empty_initially() {
    let db = Database::memory().expect("create memory db");
    let providers = db.get_all_providers("claude").expect("get all providers");
    assert!(providers.is_empty(), "should start empty for claude");
}

#[test]
fn test_get_all_providers_only_returns_matching_app_type() {
    let db = Database::memory().expect("create memory db");

    let p_claude = make_provider("c1", "Claude Provider");
    let p_codex = make_provider("cx1", "Codex Provider");

    db.save_provider("claude", &p_claude)
        .expect("save claude provider");
    db.save_provider("codex", &p_codex)
        .expect("save codex provider");

    let claude_providers = db
        .get_all_providers("claude")
        .expect("get claude providers");
    let codex_providers = db.get_all_providers("codex").expect("get codex providers");

    assert_eq!(claude_providers.len(), 1);
    assert!(claude_providers.contains_key("c1"));
    assert_eq!(codex_providers.len(), 1);
    assert!(codex_providers.contains_key("cx1"));
}

// ==================== save_provider ====================

#[test]
fn test_save_provider_insert_round_trip() {
    let db = Database::memory().expect("create memory db");

    let provider = make_provider_with_url("p1", "My Provider", "https://api.example.com");
    db.save_provider("claude", &provider)
        .expect("save provider");

    let providers = db.get_all_providers("claude").expect("get all providers");
    assert_eq!(providers.len(), 1);

    let saved = providers.get("p1").expect("provider p1 should exist");
    assert_eq!(saved.id, "p1");
    assert_eq!(saved.name, "My Provider");
    assert_eq!(
        saved.website_url.as_deref(),
        Some("https://api.example.com")
    );
}

#[test]
fn test_save_provider_update_changes_name() {
    let db = Database::memory().expect("create memory db");

    let mut provider = make_provider("p1", "Original Name");
    db.save_provider("claude", &provider).expect("initial save");

    provider.name = "Updated Name".to_string();
    db.save_provider("claude", &provider).expect("update save");

    let providers = db.get_all_providers("claude").expect("get providers");
    let saved = providers.get("p1").expect("p1 should exist");
    assert_eq!(saved.name, "Updated Name");
}

#[test]
fn test_save_multiple_providers() {
    let db = Database::memory().expect("create memory db");

    for i in 1..=5 {
        let p = make_provider(&format!("p{i}"), &format!("Provider {i}"));
        db.save_provider("claude", &p).expect("save provider");
    }

    let providers = db.get_all_providers("claude").expect("get providers");
    assert_eq!(providers.len(), 5);
}

// ==================== get_provider_by_id ====================

#[test]
fn test_get_provider_by_id_returns_correct_provider() {
    let db = Database::memory().expect("create memory db");

    let p = make_provider("target", "Target Provider");
    db.save_provider("claude", &p).expect("save provider");

    let found = db
        .get_provider_by_id("target", "claude")
        .expect("get by id");
    assert!(found.is_some(), "should find the provider");
    assert_eq!(found.unwrap().name, "Target Provider");
}

#[test]
fn test_get_provider_by_id_returns_none_for_missing() {
    let db = Database::memory().expect("create memory db");

    let result = db
        .get_provider_by_id("nonexistent", "claude")
        .expect("query should not error");
    assert!(result.is_none(), "should return None for missing provider");
}

#[test]
fn test_get_provider_by_id_wrong_app_type_returns_none() {
    let db = Database::memory().expect("create memory db");

    let p = make_provider("p1", "Claude Provider");
    db.save_provider("claude", &p).expect("save provider");

    let result = db
        .get_provider_by_id("p1", "codex")
        .expect("query should not error");
    assert!(
        result.is_none(),
        "should not find claude provider under codex app_type"
    );
}

// ==================== delete_provider ====================

#[test]
fn test_delete_provider_removes_from_list() {
    let db = Database::memory().expect("create memory db");

    let p1 = make_provider("keep", "Keep Me");
    let p2 = make_provider("delete", "Delete Me");
    db.save_provider("claude", &p1).expect("save p1");
    db.save_provider("claude", &p2).expect("save p2");

    db.delete_provider("claude", "delete")
        .expect("delete provider");

    let providers = db.get_all_providers("claude").expect("get providers");
    assert_eq!(providers.len(), 1);
    assert!(providers.contains_key("keep"), "keep should still exist");
    assert!(!providers.contains_key("delete"), "delete should be gone");
}

#[test]
fn test_delete_nonexistent_provider_does_not_error() {
    let db = Database::memory().expect("create memory db");
    let result = db.delete_provider("claude", "ghost");
    assert!(
        result.is_ok(),
        "deleting non-existent provider should not error"
    );
}

// ==================== set_current_provider / get_current_provider ====================

#[test]
fn test_set_and_get_current_provider() {
    let db = Database::memory().expect("create memory db");

    let p1 = make_provider("p1", "Provider 1");
    let p2 = make_provider("p2", "Provider 2");
    db.save_provider("claude", &p1).expect("save p1");
    db.save_provider("claude", &p2).expect("save p2");

    db.set_current_provider("claude", "p1")
        .expect("set current");
    let current = db.get_current_provider("claude").expect("get current");
    assert_eq!(current.as_deref(), Some("p1"));
}

#[test]
fn test_set_current_provider_clears_previous_current() {
    let db = Database::memory().expect("create memory db");

    let p1 = make_provider("p1", "Provider 1");
    let p2 = make_provider("p2", "Provider 2");
    db.save_provider("claude", &p1).expect("save p1");
    db.save_provider("claude", &p2).expect("save p2");

    db.set_current_provider("claude", "p1")
        .expect("set p1 current");
    db.set_current_provider("claude", "p2")
        .expect("set p2 current");

    let current = db.get_current_provider("claude").expect("get current");
    assert_eq!(current.as_deref(), Some("p2"), "p2 should now be current");


    let providers = db.get_all_providers("claude").expect("get providers");
    let current_count = providers
        .values()
        .filter(|p| p.id == current.as_deref().unwrap_or(""))
        .count();
    assert_eq!(current_count, 1, "exactly one provider should be current");
}

#[test]
fn test_get_current_provider_returns_none_when_none_set() {
    let db = Database::memory().expect("create memory db");
    let current = db.get_current_provider("claude").expect("get current");
    assert!(
        current.is_none(),
        "no current provider should be set initially"
    );
}

// ==================== update_provider_settings_config ====================

#[test]
fn test_update_provider_settings_config() {
    let db = Database::memory().expect("create memory db");

    let p = make_provider("p1", "Provider 1");
    db.save_provider("claude", &p).expect("save provider");

    let new_config = json!({ "env": { "ANTHROPIC_API_KEY": "sk-updated-key" } });
    db.update_provider_settings_config("claude", "p1", &new_config)
        .expect("update settings config");

    let found = db
        .get_provider_by_id("p1", "claude")
        .expect("get by id")
        .expect("should exist");
    assert_eq!(
        found.settings_config, new_config,
        "settings_config should be updated"
    );
}

// ==================== custom endpoints ====================

#[test]
fn test_add_and_remove_custom_endpoint() {
    let db = Database::memory().expect("create memory db");

    let p = make_provider("p1", "Provider 1");
    db.save_provider("claude", &p).expect("save provider");

    db.add_custom_endpoint("claude", "p1", "https://custom.endpoint.com")
        .expect("add endpoint");


    let providers = db.get_all_providers("claude").expect("get providers");
    let saved = providers.get("p1").expect("p1 should exist");
    let meta = saved.meta.as_ref().expect("meta should be present");
    assert!(
        meta.custom_endpoints
            .contains_key("https://custom.endpoint.com"),
        "custom endpoint should be stored"
    );

    db.remove_custom_endpoint("claude", "p1", "https://custom.endpoint.com")
        .expect("remove endpoint");

    let providers_after = db
        .get_all_providers("claude")
        .expect("get providers after remove");
    let saved_after = providers_after.get("p1").expect("p1 should exist");
    let meta_after = saved_after.meta.as_ref().expect("meta should be present");
    assert!(
        !meta_after
            .custom_endpoints
            .contains_key("https://custom.endpoint.com"),
        "custom endpoint should be removed"
    );
}

// ==================== App type isolation ====================

#[test]
fn test_current_provider_is_isolated_per_app_type() {
    let db = Database::memory().expect("create memory db");

    let p_claude = make_provider("claude-p", "Claude Provider");
    let p_codex = make_provider("codex-p", "Codex Provider");
    db.save_provider("claude", &p_claude)
        .expect("save claude provider");
    db.save_provider("codex", &p_codex)
        .expect("save codex provider");

    db.set_current_provider("claude", "claude-p")
        .expect("set claude current");
    db.set_current_provider("codex", "codex-p")
        .expect("set codex current");

    let claude_current = db
        .get_current_provider("claude")
        .expect("get claude current");
    let codex_current = db.get_current_provider("codex").expect("get codex current");

    assert_eq!(claude_current.as_deref(), Some("claude-p"));
    assert_eq!(codex_current.as_deref(), Some("codex-p"));
}

// ==================== Provider with notes/icon ====================

#[test]
fn test_save_provider_with_optional_fields() {
    let db = Database::memory().expect("create memory db");

    let mut provider = make_provider("p1", "Full Provider");
    provider.notes = Some("My note".to_string());
    provider.icon = Some("anthropic".to_string());
    provider.icon_color = Some("#00A67E".to_string());
    provider.sort_index = Some(3);
    db.save_provider("claude", &provider)
        .expect("save provider");

    let found = db
        .get_provider_by_id("p1", "claude")
        .expect("get by id")
        .expect("should exist");
    assert_eq!(found.notes.as_deref(), Some("My note"));
    assert_eq!(found.icon.as_deref(), Some("anthropic"));
    assert_eq!(found.icon_color.as_deref(), Some("#00A67E"));
    assert_eq!(found.sort_index, Some(3));
}
