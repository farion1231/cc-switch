use super::*;
use crate::provider::Provider;
use serde_json::{json, Value};
use serial_test::serial;

fn opencode_provider(id: &str) -> Provider {
    Provider {
        id: id.to_string(),
        name: format!("Provider {id}"),
        settings_config: json!({
            "npm": "@ai-sdk/openai-compatible",
            "name": format!("Provider {id}"),
            "options": {
                "baseURL": "https://api.example.com/v1",
                "apiKey": "FAKE_KEY"
            },
            "models": {
                "gpt-4o": {
                    "name": "GPT-4o"
                }
            }
        }),
        website_url: None,
        category: Some("custom".to_string()),
        created_at: Some(1),
        sort_index: Some(0),
        notes: None,
        meta: None,
        icon: None,
        icon_color: None,
        in_failover_queue: false,
    }
}

#[test]
#[serial]
fn sync_current_provider_for_app_skips_db_only_opencode_provider() {
    with_test_home(|state, _| {
        let provider = opencode_provider("db-only-opencode");
        ProviderService::add(state, AppType::OpenCode, provider.clone(), false)
            .expect("seed db-only opencode provider");

        ProviderService::sync_current_provider_for_app(state, AppType::OpenCode)
            .expect("sync additive opencode providers");

        let live_providers =
            crate::opencode_config::get_providers().expect("read opencode providers after sync");
        assert!(
            !live_providers.contains_key(&provider.id),
            "db-only opencode provider should not be written to live during sync"
        );
    });
}

#[test]
#[serial]
fn sync_current_provider_for_app_preserves_legacy_live_opencode_provider() {
    with_test_home(|state, _| {
        let provider = opencode_provider("legacy-opencode");
        crate::opencode_config::set_provider(&provider.id, provider.settings_config.clone())
            .expect("seed opencode live provider");
        state
            .db
            .save_provider(AppType::OpenCode.as_str(), &provider)
            .expect("seed legacy opencode provider in db");

        let mut updated = provider.clone();
        updated.settings_config["options"]["apiKey"] =
            Value::String("updated-FAKE_KEY".to_string());
        state
            .db
            .save_provider(AppType::OpenCode.as_str(), &updated)
            .expect("update legacy opencode provider in db");

        ProviderService::sync_current_provider_for_app(state, AppType::OpenCode)
            .expect("sync legacy opencode provider");

        // After split write-back, options.apiKey should NOT appear in opencode.json
        let live_providers =
            crate::opencode_config::get_providers().expect("read opencode providers");
        assert_eq!(
            live_providers
                .get(&provider.id)
                .and_then(|config| config.get("options"))
                .and_then(|options| options.get("apiKey")),
            None,
            "options.apiKey should not be written to opencode.json after split write-back"
        );

        // Credential should be in auth.json instead as object
        let auth_entry =
            crate::opencode_auth::get_opencode_auth_entry(&provider.id).expect("read auth.json");
        assert_eq!(
            auth_entry,
            Some(json!({"type": "api", "key": "updated-FAKE_KEY"})),
            "credential should be written to auth.json[provider_id] as object"
        );
    });
}

#[test]
#[serial]
fn sync_current_provider_for_app_restores_legacy_opencode_provider_after_live_reset() {
    with_test_home(|state, _| {
        let provider = opencode_provider("legacy-opencode-reset");
        state
            .db
            .save_provider(AppType::OpenCode.as_str(), &provider)
            .expect("seed legacy opencode provider in db");

        ProviderService::sync_current_provider_for_app(state, AppType::OpenCode)
            .expect("sync legacy opencode provider after reset");

        let live_providers =
            crate::opencode_config::get_providers().expect("read opencode providers");
        assert!(
            live_providers.contains_key(&provider.id),
            "legacy opencode provider should be restored when live config is reset"
        );
    });
}

#[test]
#[serial]
fn import_opencode_providers_from_live_marks_provider_as_live_managed() {
    with_test_home(|state, _| {
        let provider = opencode_provider("imported-opencode");
        crate::opencode_config::set_provider(&provider.id, provider.settings_config.clone())
            .expect("seed opencode live provider");

        let imported = import_opencode_providers_from_live(state)
            .expect("import opencode providers from live");
        assert_eq!(imported, 1);

        let saved = state
            .db
            .get_provider_by_id(&provider.id, AppType::OpenCode.as_str())
            .expect("query imported opencode provider")
            .expect("imported opencode provider should exist");
        assert_eq!(
            saved
                .meta
                .as_ref()
                .and_then(|meta| meta.live_config_managed),
            Some(true),
            "providers imported from live should be treated as live-managed"
        );
    });
}

#[test]
#[serial]
fn import_opencode_provider_attaches_matching_auth_entry() {
    with_test_home(|state, _| {
        let provider = opencode_provider("auth-provider");
        crate::opencode_config::set_provider(&provider.id, provider.settings_config.clone())
            .expect("seed opencode live provider");
        crate::opencode_auth::set_opencode_auth_entry(
            "auth-provider",
            json!({"type": "api", "key": "FAKE_KEY"}),
        )
        .expect("seed auth.json entry");

        let imported = import_opencode_providers_from_live(state)
            .expect("import opencode providers from live");
        assert_eq!(imported, 1);

        let saved = state
            .db
            .get_provider_by_id("auth-provider", AppType::OpenCode.as_str())
            .expect("query imported provider")
            .expect("imported provider should exist");

        let auth = saved
            .settings_config
            .get("auth")
            .expect("auth should be attached");
        assert_eq!(
            auth.get("source").and_then(|v| v.as_str()),
            Some("opencode_auth_json")
        );
        assert_eq!(auth.get("type").and_then(|v| v.as_str()), Some("api"));
        assert_eq!(auth.get("key").and_then(|v| v.as_str()), Some("FAKE_KEY"));
    });
}

#[test]
#[serial]
fn import_opencode_provider_without_matching_auth_still_works() {
    with_test_home(|state, _| {
        let provider = opencode_provider("no-auth-provider");
        crate::opencode_config::set_provider(&provider.id, provider.settings_config.clone())
            .expect("seed opencode live provider");

        let imported = import_opencode_providers_from_live(state)
            .expect("import opencode providers from live");
        assert_eq!(imported, 1);

        let saved = state
            .db
            .get_provider_by_id("no-auth-provider", AppType::OpenCode.as_str())
            .expect("query imported provider")
            .expect("imported provider should exist");

        assert!(
            saved.settings_config.get("auth").is_none(),
            "provider without auth entry should not have auth attached"
        );
    });
}

#[test]
#[serial]
fn import_opencode_provider_without_auth_file_still_works() {
    with_test_home(|state, _| {
        let provider = opencode_provider("no-auth-file-provider");
        crate::opencode_config::set_provider(&provider.id, provider.settings_config.clone())
            .expect("seed opencode live provider");

        let imported = import_opencode_providers_from_live(state)
            .expect("import opencode providers from live");
        assert_eq!(imported, 1);
    });
}

#[test]
#[serial]
fn import_opencode_multiple_providers_each_bind_own_auth() {
    with_test_home(|state, _| {
        let p1 = opencode_provider("multi-a");
        let p2 = opencode_provider("multi-b");
        crate::opencode_config::set_provider(&p1.id, p1.settings_config.clone())
            .expect("seed provider a");
        crate::opencode_config::set_provider(&p2.id, p2.settings_config.clone())
            .expect("seed provider b");
        crate::opencode_auth::set_opencode_auth_entry(
            "multi-a",
            json!({"type": "api", "key": "FAKE_KEY_A"}),
        )
        .expect("seed auth a");
        crate::opencode_auth::set_opencode_auth_entry(
            "multi-b",
            json!({"type": "api", "key": "FAKE_KEY_B"}),
        )
        .expect("seed auth b");

        let imported =
            import_opencode_providers_from_live(state).expect("import opencode providers");
        assert_eq!(imported, 2);

        let saved_a = state
            .db
            .get_provider_by_id("multi-a", AppType::OpenCode.as_str())
            .expect("query a")
            .expect("a exists");
        let saved_b = state
            .db
            .get_provider_by_id("multi-b", AppType::OpenCode.as_str())
            .expect("query b")
            .expect("b exists");

        assert_eq!(
            saved_a
                .settings_config
                .pointer("/auth/key")
                .and_then(|v| v.as_str()),
            Some("FAKE_KEY_A")
        );
        assert_eq!(
            saved_b
                .settings_config
                .pointer("/auth/key")
                .and_then(|v| v.as_str()),
            Some("FAKE_KEY_B")
        );
    });
}

#[test]
#[serial]
fn import_opencode_unrelated_auth_entries_do_not_affect_provider() {
    with_test_home(|state, _| {
        let provider = opencode_provider("target-provider");
        crate::opencode_config::set_provider(&provider.id, provider.settings_config.clone())
            .expect("seed provider");
        crate::opencode_auth::set_opencode_auth_entry(
            "unrelated-provider",
            json!({"type": "api", "key": "FAKE_KEY"}),
        )
        .expect("seed unrelated auth");

        let imported =
            import_opencode_providers_from_live(state).expect("import opencode providers");
        assert_eq!(imported, 1);

        let saved = state
            .db
            .get_provider_by_id("target-provider", AppType::OpenCode.as_str())
            .expect("query provider")
            .expect("provider exists");

        assert!(
            saved.settings_config.get("auth").is_none(),
            "unrelated auth entries should not attach to this provider"
        );
    });
}

#[test]
#[serial]
fn import_opencode_invalid_auth_json_returns_error() {
    with_test_home(|state, home| {
        let provider = opencode_provider("auth-err-provider");
        crate::opencode_config::set_provider(&provider.id, provider.settings_config.clone())
            .expect("seed provider");

        let opencode_data_dir = home.join(".local").join("share").join("opencode");
        fs::create_dir_all(&opencode_data_dir).expect("create opencode data dir");
        fs::write(opencode_data_dir.join("auth.json"), "{invalid json}")
            .expect("write malformed auth.json");

        let result = import_opencode_providers_from_live(state);
        assert!(
            result.is_err(),
            "invalid auth.json should produce import error"
        );
    });
}

#[test]
#[serial]
fn import_opencode_prefers_auth_json_over_options_api_key() {
    with_test_home(|state, _| {
        let mut settings = serde_json::Map::new();
        settings.insert("npm".to_string(), json!("@ai-sdk/openai-compatible"));
        settings.insert("name".to_string(), json!("Has Both"));
        settings.insert(
            "options".to_string(),
            json!({
                "baseURL": "https://api.example.com/v1",
                "apiKey": "OPTIONS_KEY"
            }),
        );
        settings.insert("models".to_string(), json!({"gpt-4o": {"name": "GPT-4o"}}));

        crate::opencode_config::set_provider("both-keys-provider", Value::Object(settings))
            .expect("seed provider with options.apiKey");
        crate::opencode_auth::set_opencode_auth_entry(
            "both-keys-provider",
            json!({"type": "api", "key": "AUTH_JSON_KEY"}),
        )
        .expect("seed auth.json entry");

        let imported =
            import_opencode_providers_from_live(state).expect("import opencode providers");
        assert_eq!(imported, 1);

        let saved = state
            .db
            .get_provider_by_id("both-keys-provider", AppType::OpenCode.as_str())
            .expect("query provider")
            .expect("provider exists");

        let auth_key = saved
            .settings_config
            .pointer("/auth/key")
            .and_then(|v| v.as_str());
        assert_eq!(
            auth_key,
            Some("AUTH_JSON_KEY"),
            "auth.json key should be preferred"
        );

        let options_key = saved
            .settings_config
            .pointer("/options/apiKey")
            .and_then(|v| v.as_str());
        assert_eq!(
            options_key,
            Some("OPTIONS_KEY"),
            "options.apiKey should not be silently deleted"
        );
    });
}

#[test]
#[serial]
fn opencode_write_back_splits_auth_field_to_auth_json() {
    with_test_home(|state, _| {
        let mut provider = opencode_provider("split-auth-provider");
        provider.settings_config["auth"] = json!({
            "source": "opencode_auth_json",
            "type": "api",
            "key": "FAKE_AUTH_KEY"
        });
        state
            .db
            .save_provider(AppType::OpenCode.as_str(), &provider)
            .expect("seed provider in db");

        ProviderService::sync_current_provider_for_app(state, AppType::OpenCode)
            .expect("sync provider");

        // Credential should be in auth.json (without source marker)
        let auth_entry = crate::opencode_auth::get_opencode_auth_entry("split-auth-provider")
            .expect("read auth.json");
        assert_eq!(
            auth_entry,
            Some(json!({"type": "api", "key": "FAKE_AUTH_KEY"})),
            "credential should be written to auth.json without source marker"
        );

        // opencode.json should NOT have auth or options.apiKey
        let live_providers =
            crate::opencode_config::get_providers().expect("read opencode providers");
        let live = live_providers
            .get("split-auth-provider")
            .expect("provider exists");
        assert!(
            live.get("auth").is_none(),
            "auth should not be in opencode.json"
        );
        assert_eq!(
            live.pointer("/options/apiKey"),
            None,
            "options.apiKey should not be in opencode.json"
        );
    });
}

#[test]
#[serial]
fn opencode_write_back_splits_legacy_api_key_to_auth_json() {
    with_test_home(|state, _| {
        let provider = opencode_provider("legacy-key-provider");
        state
            .db
            .save_provider(AppType::OpenCode.as_str(), &provider)
            .expect("seed provider in db");

        ProviderService::sync_current_provider_for_app(state, AppType::OpenCode)
            .expect("sync provider");

        // Legacy options.apiKey should be written to auth.json as an object-shaped API auth entry
        let auth_entry = crate::opencode_auth::get_opencode_auth_entry("legacy-key-provider")
            .expect("read auth.json");
        assert_eq!(
            auth_entry,
            Some(json!({"type": "api", "key": "FAKE_KEY"})),
            "legacy apiKey should be written to auth.json as object with type field"
        );

        // opencode.json should NOT have options.apiKey
        let live_providers =
            crate::opencode_config::get_providers().expect("read opencode providers");
        let live = live_providers
            .get("legacy-key-provider")
            .expect("provider exists");
        assert_eq!(
            live.pointer("/options/apiKey"),
            None,
            "options.apiKey should not appear in opencode.json"
        );

        // Non-secret options should still be present
        assert_eq!(
            live.pointer("/options/baseURL"),
            Some(&json!("https://api.example.com/v1")),
            "non-secret options should be preserved"
        );
    });
}

#[test]
#[serial]
fn opencode_write_back_creates_auth_json_if_missing() {
    with_test_home(|state, _| {
        let provider = opencode_provider("new-auth-provider");
        // No auth.json exists yet
        state
            .db
            .save_provider(AppType::OpenCode.as_str(), &provider)
            .expect("seed provider in db");

        ProviderService::sync_current_provider_for_app(state, AppType::OpenCode)
            .expect("sync provider");

        let auth_entry = crate::opencode_auth::get_opencode_auth_entry("new-auth-provider")
            .expect("read auth.json");
        assert!(
            auth_entry.is_some(),
            "auth.json should be created when there is credential to write"
        );
    });
}

#[test]
#[serial]
fn opencode_write_back_falls_back_to_options_api_key_when_auth_json_invalid() {
    with_test_home(|state, _| {
        let provider = opencode_provider("invalid-auth-provider");
        state
            .db
            .save_provider(AppType::OpenCode.as_str(), &provider)
            .expect("seed provider in db");

        // Write invalid JSON to auth.json
        let auth_path = crate::opencode_auth::get_opencode_auth_path();
        std::fs::create_dir_all(auth_path.parent().unwrap()).expect("create parent dir");
        std::fs::write(&auth_path, "not valid json{{{").expect("write invalid auth.json");

        // write_live_snapshot should succeed but fall back to legacy
        let result = write_live_snapshot(&AppType::OpenCode, &provider);
        assert!(
            result.is_ok(),
            "write_live_snapshot should succeed with fallback"
        );

        // Invalid auth.json should not be overwritten
        let content = std::fs::read_to_string(&auth_path).expect("read auth.json");
        assert_eq!(
            content, "not valid json{{{",
            "invalid auth.json should not be overwritten"
        );

        // opencode.json should have options.apiKey as legacy fallback
        let live_providers =
            crate::opencode_config::get_providers().expect("read opencode providers");
        let live = live_providers
            .get("invalid-auth-provider")
            .expect("provider exists");
        assert_eq!(
            live.pointer("/options/apiKey"),
            Some(&json!("FAKE_KEY")),
            "options.apiKey should be in opencode.json as legacy fallback when auth.json is invalid"
        );

        // Non-secret options should still be present
        assert_eq!(
            live.pointer("/options/baseURL"),
            Some(&json!("https://api.example.com/v1")),
            "non-secret options should be preserved"
        );
    });
}

#[test]
#[serial]
fn opencode_write_back_preserves_unrelated_auth_entries() {
    with_test_home(|state, _| {
        // Pre-seed an unrelated auth entry
        crate::opencode_auth::set_opencode_auth_entry(
            "other-provider",
            json!({"type": "api", "key": "OTHER_FAKE_KEY"}),
        )
        .expect("seed unrelated auth entry");

        let provider = opencode_provider("split-provider");
        state
            .db
            .save_provider(AppType::OpenCode.as_str(), &provider)
            .expect("seed provider in db");

        ProviderService::sync_current_provider_for_app(state, AppType::OpenCode)
            .expect("sync provider");

        // Unrelated entry should still be there
        let other_entry = crate::opencode_auth::get_opencode_auth_entry("other-provider")
            .expect("read auth.json");
        assert_eq!(
            other_entry,
            Some(json!({"type": "api", "key": "OTHER_FAKE_KEY"})),
            "unrelated auth entries should be preserved"
        );

        // New entry should be there too
        let new_entry = crate::opencode_auth::get_opencode_auth_entry("split-provider")
            .expect("read auth.json");
        assert!(new_entry.is_some(), "new credential should be written");
    });
}

#[test]
#[serial]
fn opencode_write_back_no_credential_does_not_create_auth_json() {
    with_test_home(|state, _| {
        let mut provider = opencode_provider("no-cred-provider");
        // Remove apiKey from options
        if let Some(options) = provider
            .settings_config
            .get_mut("options")
            .and_then(|v| v.as_object_mut())
        {
            options.remove("apiKey");
        }
        state
            .db
            .save_provider(AppType::OpenCode.as_str(), &provider)
            .expect("seed provider in db");

        ProviderService::sync_current_provider_for_app(state, AppType::OpenCode)
            .expect("sync provider");

        let auth_path = crate::opencode_auth::get_opencode_auth_path();
        assert!(
            !auth_path.exists(),
            "auth.json should not be created when there is no credential"
        );
    });
}

#[test]
#[serial]
fn opencode_write_back_empty_api_key_does_not_create_auth_json() {
    with_test_home(|state, _| {
        let mut provider = opencode_provider("empty-key-provider");
        provider.settings_config["options"]["apiKey"] = Value::String("".to_string());
        state
            .db
            .save_provider(AppType::OpenCode.as_str(), &provider)
            .expect("seed provider in db");

        ProviderService::sync_current_provider_for_app(state, AppType::OpenCode)
            .expect("sync provider");

        let auth_path = crate::opencode_auth::get_opencode_auth_path();
        assert!(
            !auth_path.exists(),
            "auth.json should not be created for empty apiKey"
        );
    });
}

#[test]
#[serial]
fn opencode_write_back_whitespace_api_key_does_not_create_auth_json() {
    with_test_home(|state, _| {
        let mut provider = opencode_provider("ws-key-provider");
        provider.settings_config["options"]["apiKey"] = Value::String("   ".to_string());
        state
            .db
            .save_provider(AppType::OpenCode.as_str(), &provider)
            .expect("seed provider in db");

        ProviderService::sync_current_provider_for_app(state, AppType::OpenCode)
            .expect("sync provider");

        let auth_path = crate::opencode_auth::get_opencode_auth_path();
        assert!(
            !auth_path.exists(),
            "auth.json should not be created for whitespace-only apiKey"
        );
    });
}

#[test]
#[serial]
fn opencode_write_back_fallback_restores_api_key_from_internal_auth() {
    with_test_home(|state, _| {
        // Provider has internal auth object but no inline options.apiKey
        let mut provider = opencode_provider("auth-only-provider");
        provider.settings_config["auth"] = json!({
            "source": "opencode_auth_json",
            "type": "api",
            "key": "AUTH_ONLY_FAKE_KEY"
        });
        // Remove inline apiKey to simulate auth-only source
        if let Some(options) = provider
            .settings_config
            .get_mut("options")
            .and_then(|v| v.as_object_mut())
        {
            options.remove("apiKey");
        }
        state
            .db
            .save_provider(AppType::OpenCode.as_str(), &provider)
            .expect("seed provider in db");

        // Write invalid JSON to auth.json
        let auth_path = crate::opencode_auth::get_opencode_auth_path();
        std::fs::create_dir_all(auth_path.parent().unwrap()).expect("create parent dir");
        std::fs::write(&auth_path, "not valid json{{{").expect("write invalid auth.json");

        // write_live_snapshot should succeed with fallback
        let result = write_live_snapshot(&AppType::OpenCode, &provider);
        assert!(
            result.is_ok(),
            "write_live_snapshot should succeed with fallback"
        );

        // auth.json should not be overwritten
        let content = std::fs::read_to_string(&auth_path).expect("read auth.json");
        assert_eq!(
            content, "not valid json{{{",
            "invalid auth.json should not be overwritten"
        );

        // opencode.json should have options.apiKey restored from internal auth
        let live_providers =
            crate::opencode_config::get_providers().expect("read opencode providers");
        let live = live_providers
            .get("auth-only-provider")
            .expect("provider exists");
        assert_eq!(
            live.pointer("/options/apiKey"),
            Some(&json!("AUTH_ONLY_FAKE_KEY")),
            "options.apiKey should be restored from internal auth object when auth.json is invalid"
        );
    });
}

#[test]
#[serial]
fn opencode_write_back_fallback_overrides_empty_inline_with_internal_auth_key() {
    with_test_home(|state, _| {
        // Provider has internal auth object AND empty inline placeholder
        let mut provider = opencode_provider("empty-inline-provider");
        provider.settings_config["auth"] = json!({
            "source": "opencode_auth_json",
            "type": "api",
            "key": "REAL_FAKE_KEY"
        });
        provider.settings_config["options"]["apiKey"] = Value::String("   ".to_string());
        state
            .db
            .save_provider(AppType::OpenCode.as_str(), &provider)
            .expect("seed provider in db");

        // Write invalid JSON to auth.json
        let auth_path = crate::opencode_auth::get_opencode_auth_path();
        std::fs::create_dir_all(auth_path.parent().unwrap()).expect("create parent dir");
        std::fs::write(&auth_path, "not valid json{{{").expect("write invalid auth.json");

        let result = write_live_snapshot(&AppType::OpenCode, &provider);
        assert!(
            result.is_ok(),
            "write_live_snapshot should succeed with fallback"
        );

        // opencode.json should have the real key overriding the empty placeholder
        let live_providers =
            crate::opencode_config::get_providers().expect("read opencode providers");
        let live = live_providers
            .get("empty-inline-provider")
            .expect("provider exists");
        assert_eq!(
            live.pointer("/options/apiKey"),
            Some(&json!("REAL_FAKE_KEY")),
            "internal auth key should override empty inline placeholder when auth store is invalid"
        );
    });
}

#[test]
#[serial]
fn opencode_round_trip_clean_split() {
    with_test_home(|state, _| {
        // Seed live: provider definition in opencode.json, credential in auth.json
        let mut settings = serde_json::Map::new();
        settings.insert("npm".to_string(), json!("@ai-sdk/openai-compatible"));
        settings.insert("name".to_string(), json!("RoundTrip Provider"));
        settings.insert(
            "options".to_string(),
            json!({"baseURL": "https://api.example.com/v1"}),
        );
        settings.insert("models".to_string(), json!({"gpt-4o": {"name": "GPT-4o"}}));
        crate::opencode_config::set_provider("round-trip-provider", Value::Object(settings))
            .expect("seed provider definition");
        crate::opencode_auth::set_opencode_auth_entry(
            "round-trip-provider",
            json!({"type": "api", "key": "ROUND_TRIP_FAKE_KEY"}),
        )
        .expect("seed auth entry");

        // Import
        let imported = import_opencode_providers_from_live(state).expect("import");
        assert_eq!(imported, 1);
        let saved = state
            .db
            .get_provider_by_id("round-trip-provider", AppType::OpenCode.as_str())
            .expect("query")
            .expect("exists");
        assert_eq!(
            saved
                .settings_config
                .pointer("/auth/key")
                .and_then(|v| v.as_str()),
            Some("ROUND_TRIP_FAKE_KEY"),
            "import should bind credential from auth.json"
        );

        // Write-back
        let result = write_live_snapshot(&AppType::OpenCode, &saved);
        assert!(result.is_ok(), "write-back should succeed");

        // Verify clean split: credential in auth.json, no apiKey in opencode.json
        let auth_entry = crate::opencode_auth::get_opencode_auth_entry("round-trip-provider")
            .expect("read auth.json");
        assert_eq!(
            auth_entry,
            Some(json!({"type": "api", "key": "ROUND_TRIP_FAKE_KEY"})),
            "credential should remain in auth.json"
        );
        let live_providers =
            crate::opencode_config::get_providers().expect("read opencode providers");
        let live = live_providers
            .get("round-trip-provider")
            .expect("provider exists");
        assert_eq!(
            live.pointer("/options/apiKey"),
            None,
            "opencode.json should not contain options.apiKey in normal path"
        );
        assert_eq!(
            live.pointer("/options/baseURL"),
            Some(&json!("https://api.example.com/v1")),
            "non-secret options should be preserved"
        );
    });
}

#[test]
#[serial]
fn opencode_write_back_still_writes_provider_definition_when_auth_json_invalid() {
    with_test_home(|state, _| {
        let provider = opencode_provider("def-only-provider");
        state
            .db
            .save_provider(AppType::OpenCode.as_str(), &provider)
            .expect("seed provider in db");

        let auth_path = crate::opencode_auth::get_opencode_auth_path();
        std::fs::create_dir_all(auth_path.parent().unwrap()).expect("create parent dir");
        std::fs::write(&auth_path, "not valid json{{{").expect("write invalid auth.json");

        let result = write_live_snapshot(&AppType::OpenCode, &provider);
        assert!(result.is_ok(), "write_live_snapshot should succeed");

        // Provider definition must be in opencode.json even though auth.json is invalid
        let live_providers =
            crate::opencode_config::get_providers().expect("read opencode providers");
        let live = live_providers
            .get("def-only-provider")
            .expect("provider definition should exist");
        assert_eq!(
            live.pointer("/options/baseURL"),
            Some(&json!("https://api.example.com/v1")),
            "non-secret provider definition should be written to opencode.json"
        );
        assert_eq!(
            live.pointer("/options/apiKey"),
            Some(&json!("FAKE_KEY")),
            "credential falls back to inline options.apiKey when auth.json is invalid"
        );

        // auth.json should remain untouched
        let content = std::fs::read_to_string(&auth_path).expect("read auth.json");
        assert_eq!(
            content, "not valid json{{{",
            "invalid auth.json should not be overwritten"
        );
    });
}

#[test]
#[serial]
fn opencode_oauth_fields_preserved_in_auth_json() {
    with_test_home(|state, _| {
        // Seed an OAuth-style auth entry with extra fields
        crate::opencode_auth::set_opencode_auth_entry(
            "oauth-provider",
            json!({
                "type": "oauth",
                "refresh": "FAKE_REFRESH",
                "access": "FAKE_ACCESS",
                "expires": 1234567890,
                "accountId": "acct_FAKE"
            }),
        )
        .expect("seed oauth auth entry");

        let mut settings = serde_json::Map::new();
        settings.insert("npm".to_string(), json!("@ai-sdk/openai-compatible"));
        settings.insert("name".to_string(), json!("OAuth Provider"));
        settings.insert(
            "options".to_string(),
            json!({"baseURL": "https://oauth.example.com"}),
        );
        settings.insert("models".to_string(), json!({"gpt-4o": {"name": "GPT-4o"}}));
        crate::opencode_config::set_provider("oauth-provider", Value::Object(settings))
            .expect("seed provider definition");

        // Import — should bind the full OAuth object
        let imported = import_opencode_providers_from_live(state).expect("import");
        assert_eq!(imported, 1);
        let saved = state
            .db
            .get_provider_by_id("oauth-provider", AppType::OpenCode.as_str())
            .expect("query")
            .expect("exists");
        assert_eq!(
            saved
                .settings_config
                .pointer("/auth/type")
                .and_then(|v| v.as_str()),
            Some("oauth"),
            "imported auth should have type=oauth"
        );
        assert_eq!(
            saved
                .settings_config
                .pointer("/auth/accountId")
                .and_then(|v| v.as_str()),
            Some("acct_FAKE"),
            "OAuth extension fields should be preserved on import"
        );

        // Write-back — should preserve all fields in auth.json
        let result = write_live_snapshot(&AppType::OpenCode, &saved);
        assert!(result.is_ok(), "write-back should succeed");

        let auth_entry = crate::opencode_auth::get_opencode_auth_entry("oauth-provider")
            .expect("read auth.json")
            .expect("entry exists");
        assert_eq!(
            auth_entry.get("type").and_then(|v| v.as_str()),
            Some("oauth")
        );
        assert_eq!(
            auth_entry.get("refresh").and_then(|v| v.as_str()),
            Some("FAKE_REFRESH")
        );
        assert_eq!(
            auth_entry.get("access").and_then(|v| v.as_str()),
            Some("FAKE_ACCESS")
        );
        assert_eq!(
            auth_entry.get("expires").and_then(|v| v.as_u64()),
            Some(1234567890)
        );
        assert_eq!(
            auth_entry.get("accountId").and_then(|v| v.as_str()),
            Some("acct_FAKE")
        );
    });
}
