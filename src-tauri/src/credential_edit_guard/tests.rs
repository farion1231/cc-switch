use super::*;
use crate::{provider::ProviderMeta, Database};

fn provider(app: &AppType) -> Provider {
    let settings = match app {
        AppType::Claude | AppType::ClaudeDesktop => json!({"env": {
            "ANTHROPIC_AUTH_TOKEN": "test-key-a", "ANTHROPIC_BASE_URL": "https://example.com", "ANTHROPIC_MODEL": "model-a"
        }}),
        AppType::Codex => {
            json!({"auth": {"OPENAI_API_KEY": "test-key-a"}, "config": "model_provider = \"custom\"\nmodel = \"model-a\"\n[model_providers.custom]\nbase_url = \"https://example.com\"\nwire_api = \"responses\"\n"})
        }
        AppType::GrokBuild => {
            json!({"config": "[models]\ndefault = \"custom\"\n[model.custom]\nmodel = \"model-a\"\nname = \"Custom\"\nbase_url = \"https://example.com\"\napi_key = \"test-key-a\"\napi_backend = \"openai\"\ncontext_window = 128000\n"})
        }
        AppType::Gemini => {
            json!({"env": {"GEMINI_API_KEY": "test-key-a", "GOOGLE_GEMINI_BASE_URL": "https://example.com", "GEMINI_MODEL": "model-a"}})
        }
        AppType::OpenCode | AppType::Mcode => {
            json!({"npm":"@ai-sdk/anthropic", "options":{"apiKey":"test-key-a", "baseURL":"https://example.com"},"models":{"model-a":{"name":"Model A"}}})
        }
        AppType::OpenClaw | AppType::Pi => {
            json!({"name":"Custom", "apiKey":"test-key-a", "baseUrl":"https://example.com", "api":"openai-completions", "models":[{"id":"model-a","name":"Model A", "contextWindow":128000, "maxTokens":2048, "input":["text"], "reasoning":false}]})
        }
        AppType::Hermes => {
            json!({"api_key":"test-key-a", "base_url":"https://example.com", "model":"model-a", "api_mode":"chat_completions"})
        }
        AppType::Kimi | AppType::Dsh | AppType::Agy | AppType::Omp => {
            json!({"apiKey":"test-key-a", "baseUrl":"https://example.com", "model":"model-a"})
        }
    };
    let mut provider = Provider::with_id("guarded".into(), "Custom".into(), settings, None);
    provider.meta = Some(ProviderMeta {
        credential_edit_protected: Some(true),
        ..Default::default()
    });
    provider
}

fn replace(settings: &Value, old: &str, new: &str) -> Value {
    serde_json::from_str(&serde_json::to_string(settings).unwrap().replace(old, new)).unwrap()
}

#[test]
fn credential_guard_all_app_configs_allow_model_edits_but_reject_key_and_destination_edits() {
    for app in AppType::all() {
        let db = Database::memory().unwrap();
        let original = provider(&app);
        if matches!(
            app,
            AppType::Kimi | AppType::Dsh | AppType::Agy | AppType::Omp
        ) {
            assert!(db.save_provider(app.as_str(), &original).is_err());
            assert!(db
                .get_provider_by_id(&original.id, app.as_str())
                .unwrap()
                .is_none());
            continue;
        }
        db.save_provider(app.as_str(), &original).unwrap();
        let mut edited = original.clone();
        edited.name = "Renamed".into();
        edited.settings_config = replace(&original.settings_config, "model-a", "model-b");
        db.save_provider(app.as_str(), &edited).unwrap();
        for replacement in ["test-key-b", "", "****"] {
            let mut wrong = edited.clone();
            wrong.settings_config = replace(&edited.settings_config, "test-key-a", replacement);
            assert!(
                db.save_provider(app.as_str(), &wrong).is_err(),
                "{}",
                app.as_str()
            );
            assert!(db
                .update_provider_settings_config(app.as_str(), &edited.id, &wrong.settings_config)
                .is_err());
        }
        let mut wrong = edited.clone();
        wrong.settings_config = replace(
            &edited.settings_config,
            "https://example.com",
            "https://different.example.com",
        );
        assert!(db.save_provider(app.as_str(), &wrong).is_err());
        let stored = db
            .get_provider_by_id(&original.id, app.as_str())
            .unwrap()
            .unwrap();
        assert_eq!(stored.name, edited.name);
        assert_eq!(stored.settings_config, edited.settings_config);
    }
}

#[test]
fn credential_guard_ordinary_save_cannot_drop_protection_or_rename_binding() {
    let db = Database::memory().unwrap();
    let original = provider(&AppType::Claude);
    db.save_provider("claude", &original).unwrap();
    for meta in [
        None,
        Some(ProviderMeta::default()),
        Some(ProviderMeta {
            credential_edit_protected: Some(false),
            ..Default::default()
        }),
    ] {
        let mut next = original.clone();
        next.meta = meta;
        assert!(db.save_provider("claude", &next).is_err());
    }
    let mut renamed = original.clone();
    renamed.id = "different".into();
    assert!(db
        .replace_provider_id("claude", &original.id, &renamed)
        .is_err());
    assert!(db
        .get_provider_by_id("different", "claude")
        .unwrap()
        .is_none());
    assert_eq!(
        db.get_provider_by_id(&original.id, "claude")
            .unwrap()
            .unwrap()
            .settings_config,
        original.settings_config
    );
}

#[test]
fn credential_guard_explicit_removal_is_scoped_and_changes_no_credential() {
    let db = Database::memory().unwrap();
    for app in [AppType::Claude, AppType::Codex] {
        db.save_provider(app.as_str(), &provider(&app)).unwrap();
    }
    db.remove_provider_credential_protection("claude", "guarded")
        .unwrap();
    let mut unlocked = db.get_provider_by_id("guarded", "claude").unwrap().unwrap();
    assert!(!is_protected(&unlocked));
    assert_eq!(
        unlocked.settings_config,
        provider(&AppType::Claude).settings_config
    );
    assert!(is_protected(
        &db.get_provider_by_id("guarded", "codex").unwrap().unwrap()
    ));
    unlocked.settings_config = replace(&unlocked.settings_config, "test-key-a", "test-key-b");
    db.save_provider("claude", &unlocked).unwrap();
    // A stale editor cannot restore the old protected row after the user unlocked it.
    assert!(db
        .save_provider("claude", &provider(&AppType::Claude))
        .is_err());
    assert_eq!(
        db.get_provider_by_id("guarded", "claude")
            .unwrap()
            .unwrap()
            .settings_config,
        unlocked.settings_config
    );
}

#[test]
fn credential_guard_unmanaged_rows_keep_existing_edit_behavior() {
    for app in AppType::all() {
        let db = Database::memory().unwrap();
        let mut row = provider(&app);
        row.meta = None;
        db.save_provider(app.as_str(), &row).unwrap();
        row.settings_config = replace(&row.settings_config, "test-key-a", "test-key-b");
        db.save_provider(app.as_str(), &row).unwrap();
        db.update_provider_settings_config(app.as_str(), &row.id, &row.settings_config)
            .unwrap();
    }
}

#[test]
fn credential_guard_checks_alternate_auth_fields_headers_and_sources() {
    let original = provider(&AppType::Claude);
    for (field, value) in [
        ("ANTHROPIC_API_KEY", json!("another-key")),
        ("ANTHROPIC_CUSTOM_HEADERS", json!("Authorization: other")),
        ("CLAUDE_CODE_USE_VERTEX", json!("1")),
    ] {
        let mut changed = original.clone();
        changed.settings_config["env"][field] = value;
        assert!(validate_update(&AppType::Claude, &original, &changed).is_err());
    }
    let mut changed = original.clone();
    changed.settings_config["apiKeyHelper"] = json!("some-command");
    assert!(validate_update(&AppType::Claude, &original, &changed).is_err());
    // Hooks and MCP are not credential fields owned by this provider.
    changed = original.clone();
    changed.settings_config["hooks"] = json!({"anything":{"apiKey":"unrelated"}});
    changed.settings_config["mcpServers"] = json!({"helper":{"env":{"API_KEY":"unrelated"}}});
    validate_update(&AppType::Claude, &original, &changed).unwrap();
}

#[test]
fn credential_guard_rejects_official_managed_or_external_credentials_without_resolving_them() {
    let mut row = provider(&AppType::Claude);
    row.category = Some("official".into());
    assert!(validate_supported(&AppType::Claude, &row).is_err());
    row.category = None;
    for value in [
        "",
        "${TOKEN}",
        "{file:/private/key}",
        "{env:TOKEN}",
        "!run-command",
    ] {
        row.settings_config["env"]["ANTHROPIC_AUTH_TOKEN"] = json!(value);
        assert!(validate_supported(&AppType::Claude, &row).is_err());
    }
    let mut grok = provider(&AppType::GrokBuild);
    grok.settings_config["config"] = json!(grok.settings_config["config"]
        .as_str()
        .unwrap()
        .replace("api_key = \"test-key-a\"", "env_key = \"UNREAD_ENV_KEY\""));
    assert!(validate_supported(&AppType::GrokBuild, &grok).is_err());
}

#[test]
fn credential_guard_v2_opencode_fields_and_toml_formatting_are_handled() {
    let mut original = provider(&AppType::OpenCode);
    original.settings_config = json!({"settings":{"apiKey":"test-key-a", "baseURL":"https://example.com"},"models":{"model-a":{"name":"Model A"}}});
    validate_supported(&AppType::OpenCode, &original).unwrap();
    let mut updated = original.clone();
    updated.settings_config["settings"]["apiKey"] = json!("different-key");
    assert!(validate_update(&AppType::OpenCode, &original, &updated).is_err());
    let original = provider(&AppType::Codex);
    let mut updated = original.clone();
    updated.settings_config["config"] = json!(format!(
        "# independent comment\n{}",
        original.settings_config["config"]
            .as_str()
            .unwrap()
            .replace("model-a", "model-b")
    ));
    validate_update(&AppType::Codex, &original, &updated).unwrap();
    updated.settings_config["config"] = json!("invalid [ TOML");
    assert!(validate_update(&AppType::Codex, &original, &updated).is_err());
}

#[test]
fn credential_guard_proxy_auth_overrides_cannot_change_but_other_headers_can() {
    let original = provider(&AppType::Claude);
    let mut edited = original.clone();
    edited.meta.as_mut().unwrap().local_proxy_request_overrides =
        Some(crate::provider::LocalProxyRequestOverrides {
            headers: [("X-Trace".into(), "new-value".into())].into(),
            body: None,
        });
    validate_update(&AppType::Claude, &original, &edited).unwrap();
    edited
        .meta
        .as_mut()
        .unwrap()
        .local_proxy_request_overrides
        .as_mut()
        .unwrap()
        .headers
        .insert("Authorization".into(), "Bearer wrong-key".into());
    assert!(validate_update(&AppType::Claude, &original, &edited).is_err());
}

#[test]
fn credential_guard_rejects_helpers_even_when_an_inline_key_is_also_present() {
    let mut row = provider(&AppType::Claude);
    row.settings_config["apiKeyHelper"] = json!("do-not-run");
    assert!(validate_supported(&AppType::Claude, &row).is_err());
    let mut row = provider(&AppType::GrokBuild);
    row.settings_config["config"] = json!(format!(
        "{}\nenv_key = \"DO_NOT_READ\"",
        row.settings_config["config"].as_str().unwrap()
    ));
    assert!(validate_supported(&AppType::GrokBuild, &row).is_err());
    let mut row = provider(&AppType::OpenCode);
    row.settings_config["options"]["headers"] = json!({"Authorization":"Bearer ${DO_NOT_READ}"});
    assert!(validate_supported(&AppType::OpenCode, &row).is_err());
}
