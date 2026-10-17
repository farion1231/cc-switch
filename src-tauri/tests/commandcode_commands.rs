use cc_switch_lib::{AppType, Provider, ProviderService};
use serde_json::{json, Value};
use std::fs;

#[path = "support.rs"]
mod support;
use support::{create_test_state, ensure_test_home, reset_test_fs, test_mutex};

fn provider(id: &str, config: Value) -> Provider {
    let name = config
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or(id)
        .to_string();
    Provider::with_id(id.into(), name, config, None)
}

fn native_config(key: Value) -> Value {
    json!({
        "name": "Test",
        "baseURL": "https://api.example.com/v1",
        "apiKey": key,
        "models": {"model": {"name": "Model"}}
    })
}

#[test]
fn adding_a_commandcode_provider_rejects_a_key_already_in_use() {
    let _guard = test_mutex().lock().unwrap();
    reset_test_fs();
    let state = create_test_state().unwrap();
    let path = ensure_test_home().join(".commandcode/providers.json");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let native = "{\n  \"provider\": {\n    \"handwritten\": {\n      \"baseURL\": \"https://hand.example.com\",\n      \"models\": { \"m\": {} }\n    }\n  }\n}\n";
    fs::write(&path, native).unwrap();

    // Live entries, hand-written included, are never overwritten.
    assert!(ProviderService::add(
        &state,
        AppType::CommandCode,
        provider("handwritten", native_config(json!("k"))),
        true
    )
    .is_err());
    // Reserved subscription lanes are rejected by validation.
    assert!(ProviderService::add(
        &state,
        AppType::CommandCode,
        provider("codex", native_config(json!("k"))),
        true
    )
    .is_err());
    assert_eq!(fs::read_to_string(&path).unwrap(), native);
    assert!(state
        .db
        .get_all_providers("commandcode")
        .unwrap()
        .is_empty());

    // Neither is a row that only lives in the database.
    ProviderService::add(
        &state,
        AppType::CommandCode,
        provider("fresh", native_config(json!("k"))),
        false,
    )
    .unwrap();
    let mut renamed = provider("fresh", native_config(json!("k")));
    renamed.name = "Other".into();
    assert!(ProviderService::add(&state, AppType::CommandCode, renamed, true).is_err());
    assert_eq!(
        state
            .db
            .get_provider_by_id("fresh", "commandcode")
            .unwrap()
            .unwrap()
            .name,
        "Test"
    );
    // The rejected add left the live file untouched.
    let stored: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert!(stored["provider"].get("fresh").is_none());
}

#[test]
fn concurrent_commandcode_adds_with_one_key_leave_exactly_one_winner() {
    let _guard = test_mutex().lock().unwrap();
    reset_test_fs();
    let state = create_test_state().unwrap();
    let successes = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..16)
            .map(|index| {
                let state = &state;
                scope.spawn(move || {
                    let provider = Provider::with_id(
                        "same".into(),
                        format!("Racer {index}"),
                        json!({
                            "name": format!("Racer {index}"),
                            "baseURL": "https://api.example.com/v1",
                            "apiKey": "test-key",
                            "models": {"model": {"name": "Model"}}
                        }),
                        None,
                    );
                    ProviderService::add(state, AppType::CommandCode, provider, false)
                        .is_ok()
                        .then_some(index)
                })
            })
            .collect();
        handles
            .into_iter()
            .filter_map(|handle| handle.join().unwrap())
            .collect::<Vec<_>>()
    });
    assert_eq!(successes.len(), 1);
    assert_eq!(
        state
            .db
            .get_provider_by_id("same", "commandcode")
            .unwrap()
            .unwrap()
            .name,
        format!("Racer {}", successes[0])
    );
}

#[test]
fn raw_keys_go_to_auth_json_and_round_trip_through_list() {
    let _guard = test_mutex().lock().unwrap();
    reset_test_fs();
    let state = create_test_state().unwrap();
    let dir = ensure_test_home().join(".commandcode");
    ProviderService::add(
        &state,
        AppType::CommandCode,
        provider("openrouter", native_config(json!("sk-live"))),
        true,
    )
    .unwrap();

    // The secret never lands in providers.json; it goes to auth.json instead.
    let providers: Value =
        serde_json::from_str(&fs::read_to_string(dir.join("providers.json")).unwrap()).unwrap();
    assert!(providers["provider"]["openrouter"].get("apiKey").is_none());
    assert_eq!(
        providers["provider"]["openrouter"]["models"]["model"]["name"],
        "Model"
    );
    let auth: Value =
        serde_json::from_str(&fs::read_to_string(dir.join("auth.json")).unwrap()).unwrap();
    assert_eq!(auth["openrouter"], json!({"type": "api", "key": "sk-live"}));

    // list() resolves the stored key back into the provider config.
    let listed = ProviderService::list(&state, AppType::CommandCode).unwrap();
    assert_eq!(listed["openrouter"].settings_config["apiKey"], "sk-live");
    assert_eq!(
        listed["openrouter"]
            .meta
            .as_ref()
            .and_then(|meta| meta.live_config_managed),
        Some(true)
    );

    // Deleting the provider clears both files.
    ProviderService::delete(&state, AppType::CommandCode, "openrouter").unwrap();
    let providers: Value =
        serde_json::from_str(&fs::read_to_string(dir.join("providers.json")).unwrap()).unwrap();
    assert!(providers["provider"].get("openrouter").is_none());
    let auth_text = fs::read_to_string(dir.join("auth.json")).unwrap_or_default();
    let auth: Value = serde_json::from_str(&auth_text).unwrap_or_else(|_| json!({}));
    assert!(auth.get("openrouter").is_none());
}
