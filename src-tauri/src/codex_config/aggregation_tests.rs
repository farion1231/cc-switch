use super::*;
use serde_json::json;
use serial_test::serial;

const DEEPSEEK_NATIVE_CONFIG: &str = r#"model = "deepseek-v4-flash"
model_provider = "custom"

[model_providers.custom]
name = "deepseek"
base_url = "https://api.deepseek.com"
wire_api = "responses"
"#;

fn temp_codex_path() -> (tempfile::TempDir, std::path::PathBuf) {
    let temp_home = tempfile::tempdir().expect("create temp home");
    let codex_dir = temp_home.path().join(".codex");
    (temp_home, codex_dir)
}

fn temp_codex_dir() -> (tempfile::TempDir, std::path::PathBuf) {
    let (temp_home, codex_dir) = temp_codex_path();
    std::fs::create_dir_all(&codex_dir).expect("create codex dir");
    (temp_home, codex_dir)
}

fn write_json(path: impl AsRef<std::path::Path>, value: &Value) {
    let bytes = serde_json::to_vec(value).expect("serialize JSON fixture");
    std::fs::write(path, bytes).expect("write JSON fixture");
}

fn read_json(path: impl AsRef<std::path::Path>) -> Value {
    let bytes = std::fs::read(path).expect("read JSON fixture");
    serde_json::from_slice(&bytes).expect("parse JSON fixture")
}

fn test_provider(
    id: &str,
    name: &str,
    settings_config: Value,
    category: &str,
    api_format: Option<&str>,
) -> Provider {
    let mut provider = Provider::with_id(id.to_string(), name.to_string(), settings_config, None);
    provider.category = Some(category.to_string());
    provider.meta = api_format.map(|format| crate::provider::ProviderMeta {
        api_format: Some(format.to_string()),
        ..Default::default()
    });
    provider
}

fn official_provider(settings_config: Value) -> Provider {
    test_provider(
        crate::database::CODEX_OFFICIAL_PROVIDER_ID,
        "OpenAI Official",
        settings_config,
        "official",
        None,
    )
}

fn responses_provider(id: &str, name: &str, settings_config: Value) -> Provider {
    test_provider(id, name, settings_config, "codex", Some("openai_responses"))
}

#[test]
fn official_proxy_route_aggregate_mode_injects_bearer_placeholder() {
    // 关闭官方登录（聚合模式）：Codex 必须拿到一个占位凭据才不弹登录页。
    let output = apply_codex_official_proxy_route_with_auth("", "http://127.0.0.1:15721/v1", false)
        .expect("apply aggregate route");
    let doc: toml::Value = toml::from_str(&output).expect("parse output");

    assert_eq!(
        doc.get("model_provider").and_then(toml::Value::as_str),
        Some(CC_SWITCH_CODEX_OFFICIAL_PROXY_PROVIDER_ID)
    );
    let provider = &doc["model_providers"][CC_SWITCH_CODEX_OFFICIAL_PROXY_PROVIDER_ID];
    assert_eq!(
        provider
            .get("requires_openai_auth")
            .and_then(toml::Value::as_bool),
        Some(false)
    );
    assert_eq!(
        provider
            .get("experimental_bearer_token")
            .and_then(toml::Value::as_str),
        Some("PROXY_MANAGED")
    );

    // 移除接管路由时占位 token 一并被清掉，不残留到恢复后的配置。
    let cleaned = remove_codex_official_proxy_route(&output).expect("clean aggregate route");
    let cleaned_doc: toml::Value = toml::from_str(&cleaned).expect("parse cleaned");
    assert!(cleaned_doc.get("model_provider").is_none());
    assert!(!cleaned.contains("experimental_bearer_token"));
}

#[test]
fn codex_auth_credentials_parse_token_and_optional_account_id() {
    let credentials = codex_auth_credentials_from_value(&json!({
        "tokens": {
            "access_token": "  official-token  ",
            "account_id": "  workspace-123  "
        }
    }))
    .expect("parse Codex auth credentials");

    assert_eq!(credentials.access_token, "official-token");
    assert_eq!(credentials.account_id.as_deref(), Some("workspace-123"));

    let personal = codex_auth_credentials_from_value(&json!({
        "tokens": {
            "access_token": "personal-token",
            "account_id": "   "
        }
    }))
    .expect("parse personal-account credentials");
    assert_eq!(personal.account_id, None);
}

#[test]
#[serial]
fn clear_stale_auth_skipped_when_official_login_disabled() {
    let dir = tempfile::TempDir::new().expect("create temp home");
    let original_home = std::env::var("HOME").ok();
    let original_userprofile = std::env::var("USERPROFILE").ok();
    std::env::set_var("HOME", dir.path());
    std::env::set_var("USERPROFILE", dir.path());
    crate::settings::reload_settings().expect("reload settings");

    let codex_dir = get_codex_config_dir();
    std::fs::create_dir_all(&codex_dir).expect("create codex dir");
    let auth_path = get_codex_auth_path();
    write_json_file(&auth_path, &json!({ "OPENAI_API_KEY": "sk-third-party" }))
        .expect("seed stale third-party auth");

    // 聚合模式（关闭官方登录）：不得删除 auth.json，否则 Codex 弹登录。
    let removed = clear_stale_codex_live_auth_after_official_switch(
        &json!({ "enableOfficialLogin": false }),
        &json!({}),
    )
    .expect("cleanup must not fail");
    assert!(!removed, "aggregate mode must not delete auth.json");
    assert!(
        auth_path.exists(),
        "stale auth must be preserved in aggregate mode"
    );

    // 启用官方登录时仍按原逻辑清理残留的第三方 key。
    let removed = clear_stale_codex_live_auth_after_official_switch(&json!({}), &json!({}))
        .expect("cleanup must not fail");
    assert!(
        removed,
        "official login mode still clears stale third-party auth"
    );
    assert!(
        !auth_path.exists(),
        "stale auth must be deleted for official login mode"
    );

    match original_home {
        Some(value) => std::env::set_var("HOME", value),
        None => std::env::remove_var("HOME"),
    }
    match original_userprofile {
        Some(value) => std::env::set_var("USERPROFILE", value),
        None => std::env::remove_var("USERPROFILE"),
    }
    crate::settings::update_settings(crate::settings::AppSettings::default())
        .expect("reset settings");
}

#[test]
fn aggregate_catalog_uses_each_bound_providers_tool_profile() {
    let db = crate::database::Database::memory().expect("create in-memory database");
    let mut chat = Provider::with_id(
        "chat-provider".to_string(),
        "Chat Provider".to_string(),
        json!({ "apiFormat": "openai_chat" }),
        None,
    );
    chat.meta = Some(crate::provider::ProviderMeta {
        api_format: Some("openai_chat".to_string()),
        ..Default::default()
    });
    let mut native = Provider::with_id(
        "native-provider".to_string(),
        "Native Provider".to_string(),
        json!({ "apiFormat": "openai_responses" }),
        None,
    );
    native.meta = Some(crate::provider::ProviderMeta {
        api_format: Some("openai_responses".to_string()),
        ..Default::default()
    });
    db.save_provider("codex", &chat)
        .expect("save chat provider");
    db.save_provider("codex", &native)
        .expect("save native provider");

    let settings = json!({
        "codexCustomModels": [
            {
                "model": "gpt-5.4-mini",
                "providerId": "chat-provider",
                "upstreamModel": "chat-model"
            },
            {
                "model": "gpt-5.2",
                "providerId": "native-provider",
                "upstreamModel": "native-model"
            }
        ]
    });

    let resolve_provider =
        |provider_id: &str| resolve_codex_custom_catalog_provider_from_db(&db, provider_id);
    let entries = codex_custom_catalog_entries(
        &settings,
        "",
        CodexCatalogToolProfile::NativeResponses,
        Some(&resolve_provider),
    )
    .expect("build aggregate catalog entries");
    assert_eq!(
        entries[0]
            .get("apply_patch_tool_type")
            .and_then(|value| value.as_str()),
        Some("freeform"),
        "a Chat route must keep the freeform apply_patch surface"
    );
    assert!(
        entries[1].get("apply_patch_tool_type").is_none(),
        "a native Responses route must suppress freeform apply_patch"
    );
}

#[test]
fn aggregate_catalog_preserves_bound_providers_official_vendor_capabilities() {
    let mut provider = Provider::with_id(
        "deepseek-provider".to_string(),
        "DeepSeek Provider".to_string(),
        json!({
            "apiFormat": "openai_responses",
            "model": "deepseek-v4-pro",
            "config": DEEPSEEK_NATIVE_CONFIG
        }),
        None,
    );
    provider.meta = Some(crate::provider::ProviderMeta {
        api_format: Some("openai_responses".to_string()),
        ..Default::default()
    });
    let settings = json!({
        "codexCustomModels": [{
            "model": "gpt-5.2",
            "providerId": "deepseek-provider",
            "upstreamModel": "deepseek-v4-pro"
        }]
    });
    let resolve_provider =
        |provider_id: &str| (provider_id == provider.id).then(|| provider.clone());

    let entries = codex_custom_catalog_entries(
        &settings,
        "",
        CodexCatalogToolProfile::NativeResponses,
        Some(&resolve_provider),
    )
    .expect("build aggregate catalog entries");

    let entry = &entries[0];
    assert_eq!(entry.get("slug"), Some(&json!("gpt-5.2")));
    assert_eq!(entry.get("display_name"), Some(&json!("gpt-5.2")));
    assert_eq!(entry.get("apply_patch_tool_type"), Some(&json!("freeform")));
    assert!(entry
        .get("base_instructions")
        .and_then(Value::as_str)
        .is_some_and(|text| text.starts_with("You are Codex, an agent based on GPT-5")));
    assert_eq!(entry.get("context_window"), Some(&json!(1_048_576)));
}

#[test]
fn aggregate_vendor_catalog_preserves_case_sensitive_public_slot_identity() {
    let mut provider = Provider::with_id(
        "deepseek-provider".to_string(),
        "DeepSeek Provider".to_string(),
        json!({
            "apiFormat": "openai_responses",
            "model": "deepseek-v4-pro",
            "config": DEEPSEEK_NATIVE_CONFIG
        }),
        None,
    );
    provider.meta = Some(crate::provider::ProviderMeta {
        api_format: Some("openai_responses".to_string()),
        ..Default::default()
    });
    let settings = json!({
        "codexCustomModels": [{
            "model": "DEEPSEEK-V4-PRO",
            "providerId": "deepseek-provider"
        }]
    });
    let resolve_provider =
        |provider_id: &str| (provider_id == provider.id).then(|| provider.clone());

    let entries = codex_custom_catalog_entries(
        &settings,
        "",
        CodexCatalogToolProfile::NativeResponses,
        Some(&resolve_provider),
    )
    .expect("build aggregate catalog entries");

    assert_eq!(entries[0].get("slug"), Some(&json!("DEEPSEEK-V4-PRO")));
    assert_eq!(
        entries[0].get("display_name"),
        Some(&json!("DEEPSEEK-V4-PRO"))
    );
}

#[test]
fn vendor_catalog_alias_selects_template_by_upstream_model() {
    let vendor_models = vec![
        json!({
            "slug": "vendor-flash",
            "display_name": "Vendor Flash",
            "description": "Vendor Flash",
            "vendor_capability": "flash"
        }),
        json!({
            "slug": "vendor-pro",
            "display_name": "Vendor Pro",
            "description": "Vendor Pro",
            "vendor_capability": "pro"
        }),
    ];
    let spec = CodexCatalogModelSpec {
        model: "gpt-5.2".to_string(),
        display_name: None,
        context_window: None,
        supports_parallel_tool_calls: None,
        input_modalities: None,
        base_instructions: None,
        reasoning_levels: None,
        default_reasoning_level: None,
    };

    let entry = codex_vendor_catalog_model_entry(&vendor_models, &spec, 0, Some("vendor-pro"));

    assert_eq!(entry.get("slug"), Some(&json!("gpt-5.2")));
    assert_eq!(entry.get("vendor_capability"), Some(&json!("pro")));
}

#[test]
fn aggregate_catalog_uses_bound_providers_default_context_window() {
    let mut provider = Provider::with_id(
        "native-provider".to_string(),
        "Native Provider".to_string(),
        json!({
            "apiFormat": "openai_responses",
            "model": "native-model",
            "config": "model = \"native-model\"\nmodel_context_window = 262144\n"
        }),
        None,
    );
    provider.meta = Some(crate::provider::ProviderMeta {
        api_format: Some("openai_responses".to_string()),
        ..Default::default()
    });
    let settings = json!({
        "codexCustomModels": [{
            "model": "gpt-5.2",
            "providerId": "native-provider",
            "upstreamModel": "native-model",
            "reasoningLevels": ["low", "medium", "high"],
            "defaultReasoningLevel": "medium"
        }]
    });
    let resolve_provider =
        |provider_id: &str| (provider_id == provider.id).then(|| provider.clone());

    let entries = codex_custom_catalog_entries(
        &settings,
        "model_context_window = 999999\n",
        CodexCatalogToolProfile::NativeResponses,
        Some(&resolve_provider),
    )
    .expect("build aggregate catalog entries");

    assert_eq!(entries[0].get("context_window"), Some(&json!(262_144)));
    assert_eq!(entries[0].get("max_context_window"), Some(&json!(262_144)));
    assert_eq!(
        entries[0]["supported_reasoning_levels"]
            .as_array()
            .expect("reasoning levels")
            .iter()
            .filter_map(|level| level["effort"].as_str())
            .collect::<Vec<_>>(),
        vec!["low", "medium", "high"]
    );
    assert_eq!(entries[0]["default_reasoning_level"], json!("medium"));
}

#[test]
fn aggregate_catalog_omits_mapping_when_bound_provider_is_missing() {
    let settings = json!({
        "codexCustomModels": [{
            "model": "gpt-5.2",
            "providerId": "deleted-provider",
            "upstreamModel": "deepseek-v4-flash"
        }]
    });
    let resolve_provider = |_: &str| -> Option<Provider> { None };

    let entries = codex_custom_catalog_entries(
        &settings,
        "",
        CodexCatalogToolProfile::NativeResponses,
        Some(&resolve_provider),
    )
    .expect("build aggregate catalog entries");

    assert!(
        entries.is_empty(),
        "a stale mapping must not advertise an unroutable slot"
    );
}

#[test]
fn aggregate_catalog_omits_official_self_binding_without_official_login() {
    let official = Provider::with_id(
        crate::database::CODEX_OFFICIAL_PROVIDER_ID.to_string(),
        "Codex Official".to_string(),
        json!({}),
        None,
    );
    let settings = json!({
        "enableOfficialLogin": false,
        "codexCustomModels": [{
            "model": "gpt-5.2",
            "providerId": "codex-official"
        }]
    });
    let resolve_provider =
        |provider_id: &str| (provider_id == official.id).then(|| official.clone());

    let entries = codex_custom_catalog_entries(
        &settings,
        "",
        CodexCatalogToolProfile::NativeResponses,
        Some(&resolve_provider),
    )
    .expect("build aggregate catalog entries");

    assert!(
        entries.is_empty(),
        "aggregate mode must not advertise an official self-binding that routing rejects"
    );
}

#[test]
fn aggregate_catalog_infers_modalities_from_the_actual_upstream_model() {
    let db = crate::database::Database::memory().expect("create in-memory database");
    let provider = Provider::with_id(
        "deepseek-provider".to_string(),
        "DeepSeek Provider".to_string(),
        json!({
            "apiFormat": "openai_responses",
            "model": "deepseek-v4-flash",
            "config": "model = \"deepseek-v4-flash\""
        }),
        None,
    );
    db.save_provider("codex", &provider)
        .expect("save bound provider");
    let settings = json!({
        "codexCustomModels": [{
            "model": "gpt-5.2",
            "providerId": "deepseek-provider",
            "upstreamModel": "deepseek-v4-flash"
        }]
    });
    let resolve_provider =
        |provider_id: &str| resolve_codex_custom_catalog_provider_from_db(&db, provider_id);

    let entries = codex_custom_catalog_entries(
        &settings,
        "",
        CodexCatalogToolProfile::NativeResponses,
        Some(&resolve_provider),
    )
    .expect("build aggregate catalog entries");

    assert_eq!(entries[0].get("slug"), Some(&json!("gpt-5.2")));
    assert_eq!(
        entries[0].get("input_modalities"),
        Some(&json!(["text"])),
        "capabilities must follow the routed upstream model, not the public slot"
    );
}

#[test]
fn aggregate_catalog_infers_modalities_from_the_bound_providers_default_model() {
    let db = crate::database::Database::memory().expect("create in-memory database");
    let provider = Provider::with_id(
        "deepseek-provider".to_string(),
        "DeepSeek Provider".to_string(),
        json!({
            "apiFormat": "openai_responses",
            "model": "deepseek-v4-flash",
            "config": "model = \"deepseek-v4-flash\""
        }),
        None,
    );
    db.save_provider("codex", &provider)
        .expect("save bound provider");
    let settings = json!({
        "codexCustomModels": [{
            "model": "gpt-5.2",
            "providerId": "deepseek-provider"
        }]
    });
    let resolve_provider =
        |provider_id: &str| resolve_codex_custom_catalog_provider_from_db(&db, provider_id);

    let entries = codex_custom_catalog_entries(
        &settings,
        "",
        CodexCatalogToolProfile::NativeResponses,
        Some(&resolve_provider),
    )
    .expect("build aggregate catalog entries");

    assert_eq!(
        entries[0].get("input_modalities"),
        Some(&json!(["text"])),
        "an omitted upstream override must inherit the bound provider's routed model"
    );
}

#[test]
fn aggregate_catalog_prefers_bound_provider_declared_modalities() {
    let db = crate::database::Database::memory().expect("create in-memory database");
    let provider = Provider::with_id(
        "declared-provider".to_string(),
        "Declared Provider".to_string(),
        json!({
            "apiFormat": "openai_responses",
            "model": "custom-text-upstream",
            "config": "model = \"custom-text-upstream\"",
            "modelCatalog": {
                "models": [{
                    "model": "custom-text-upstream",
                    "inputModalities": ["text"]
                }]
            }
        }),
        None,
    );
    db.save_provider("codex", &provider)
        .expect("save bound provider");
    let settings = json!({
        "codexCustomModels": [{
            "model": "gpt-5.2",
            "providerId": "declared-provider",
            "upstreamModel": "custom-text-upstream"
        }]
    });
    let resolve_provider =
        |provider_id: &str| resolve_codex_custom_catalog_provider_from_db(&db, provider_id);

    let entries = codex_custom_catalog_entries(
        &settings,
        "",
        CodexCatalogToolProfile::NativeResponses,
        Some(&resolve_provider),
    )
    .expect("build aggregate catalog entries");

    assert_eq!(
        entries[0].get("input_modalities"),
        Some(&json!(["text"])),
        "the bound provider's explicit capability declaration must win"
    );
}

#[cfg(target_os = "macos")]
#[test]
fn codex_cli_candidates_include_chatgpt_desktop_binary() {
    assert!(
        codex_cli_candidates().iter().any(|candidate| {
            candidate == Path::new("/Applications/ChatGPT.app/Contents/Resources/codex")
        }),
        "fresh desktop-only installs need the bundled Codex version for cache validation"
    );
}

#[test]
fn write_codex_models_cache_for_aggregate_refreshes_cache() {
    let (_temp_home, codex_dir) = temp_codex_dir();

    // Pre-existing cache with a stale timestamp and an official model.
    let stale_cache = json!({
        "fetched_at": "2026-08-01T00:00:00.000000000Z",
        "etag": "W/\"old\"",
        "client_version": "0.146.0",
        "models": [{
            "slug": "gpt-5.5",
            "display_name": "GPT-5.5",
            "context_window": 400000
        }]
    });
    write_json(codex_dir.join("models_cache.json"), &stale_cache);

    // Aggregate provider: official login disabled + custom model row.
    let settings = json!({
        "enableOfficialLogin": false,
        "codexCustomModels": [{
            "model": "deepseek-v4-flash",
            "providerId": "deepseek",
            "displayName": "DeepSeek V4 Flash",
            "contextWindow": 131072
        }]
    });

    write_codex_models_cache_for_aggregate_at(codex_dir.clone(), &settings, "", None)
        .expect("aggregate cache write must succeed");

    let written = read_json(codex_dir.join("models_cache.json"));

    let fetched_at = written
        .get("fetched_at")
        .and_then(|v| v.as_str())
        .expect("fetched_at must exist");
    let parsed_fetched_at =
        chrono::DateTime::parse_from_rfc3339(fetched_at).expect("fetched_at must be valid RFC3339");
    let age = chrono::Utc::now().signed_duration_since(parsed_fetched_at);
    assert!(
        age.num_seconds().abs() < 60,
        "fetched_at must be fresh (age {age:?})"
    );

    let models = written
        .get("models")
        .and_then(|v| v.as_array())
        .expect("models array");
    let slugs: Vec<&str> = models
        .iter()
        .filter_map(|m| m.get("slug").and_then(|s| s.as_str()))
        .collect();
    assert!(
        !slugs.contains(&"gpt-5.5"),
        "stale official entries must be cleared in aggregate mode: {slugs:?}"
    );
    assert!(
        slugs.contains(&"deepseek-v4-flash"),
        "custom model must be present: {slugs:?}"
    );
    assert_eq!(slugs.len(), 1, "only the mapped custom model should remain");
    assert!(
        written
            .get("etag")
            .and_then(|value| value.as_str())
            .is_some_and(|etag| etag.starts_with("W/\"cc-switch-")),
        "aggregate rewrites must be marked as cc-switch-owned"
    );
}

#[test]
fn models_cache_uses_detected_codex_version_when_cache_is_missing() {
    let (_temp_home, codex_dir) = temp_codex_dir();

    write_models_cache_json_with_client_version(
        &codex_dir,
        vec![json!({ "slug": "gpt-5.2" })],
        Some("codex-cli 0.147.3-alpha.2"),
    )
    .expect("write cache with detected Codex version");

    let cache = read_json(codex_dir.join("models_cache.json"));
    assert_eq!(
        cache.get("client_version").and_then(Value::as_str),
        Some("0.147.3")
    );
}

#[test]
fn models_cache_prefers_detected_codex_version_after_upgrade() {
    let (_temp_home, codex_dir) = temp_codex_dir();
    write_json(
        codex_dir.join("models_cache.json"),
        &json!({
            "client_version": "0.146.0",
            "models": []
        }),
    );

    write_models_cache_json_with_client_version(
        &codex_dir,
        vec![json!({ "slug": "gpt-5.2" })],
        Some("codex-cli 0.147.0"),
    )
    .expect("rewrite cache for upgraded Codex");

    let cache = read_json(codex_dir.join("models_cache.json"));
    assert_eq!(
        cache.get("client_version").and_then(Value::as_str),
        Some("0.147.0")
    );
}

#[test]
fn models_cache_refuses_to_invent_client_version() {
    let (_temp_home, codex_dir) = temp_codex_dir();

    let result = write_models_cache_json_with_client_version(
        &codex_dir,
        vec![json!({ "slug": "gpt-5.2" })],
        None,
    );

    assert!(result.is_err(), "missing client version must fail closed");
    assert!(
        !codex_dir.join("models_cache.json").exists(),
        "an unverifiable cache must not be published"
    );
}

#[test]
fn codex_client_version_parser_rejects_malformed_output() {
    assert_eq!(
        parse_codex_cli_client_version("codex-cli 0.147.3-alpha.2"),
        Some("0.147.3".to_string())
    );
    assert_eq!(parse_codex_cli_client_version("codex-cli dev"), None);
    assert_eq!(parse_codex_cli_client_version("0.147"), None);
    assert_eq!(parse_codex_cli_client_version(""), None);
}

#[test]
fn write_codex_models_cache_for_aggregate_builds_from_custom_mappings_only() {
    let (_temp_home, codex_dir) = temp_codex_dir();

    // 旧缓存残留多个官方/旧供应商条目：聚合模式下不可路由，必须全部清掉，
    // 只保留当前 codexCustomModels 映射的条目。
    let stale_cache = json!({
        "fetched_at": "2026-08-01T00:00:00.000000000Z",
        "etag": "W/\"old\"",
        "client_version": "0.146.0",
        "models": [
            {"slug": "gpt-5.5", "display_name": "GPT-5.5"},
            {"slug": "gpt-5.4", "display_name": "GPT-5.4"},
            {"slug": "deepseek-v4-pro", "display_name": "DeepSeek V4 Pro"}
        ]
    });
    write_json(codex_dir.join("models_cache.json"), &stale_cache);

    let settings = json!({
        "enableOfficialLogin": false,
        "codexCustomModels": [
            {
                "model": "gpt-5.2",
                "providerId": "deepseek",
                "upstreamModel": "deepseek-v4-flash",
                "displayName": "DeepSeek V4 Flash",
                "contextWindow": 131072
            },
            {
                "model": "gpt-5.5",
                "providerId": "glm",
                "upstreamModel": "glm-5",
                "displayName": "GLM-5",
                "contextWindow": 131072
            }
        ]
    });

    write_codex_models_cache_for_aggregate_at(codex_dir.clone(), &settings, "", None)
        .expect("aggregate cache write must succeed");

    let written = read_json(codex_dir.join("models_cache.json"));
    let models = written
        .get("models")
        .and_then(|v| v.as_array())
        .expect("models array");
    let slugs: Vec<&str> = models
        .iter()
        .filter_map(|m| m.get("slug").and_then(|s| s.as_str()))
        .collect();
    assert_eq!(
        slugs,
        vec!["gpt-5.2", "gpt-5.5"],
        "only mapped carrier slots remain, stale official entries cleared"
    );
}

#[test]
fn aggregate_rewrite_updates_existing_official_baseline_before_overwrite() {
    let (_temp_home, codex_dir) = temp_codex_dir();

    let old_baseline = json!({
        "fetched_at": "2026-08-01T00:00:00Z",
        "etag": "W/\"official-old\"",
        "client_version": "0.146.0",
        "cc_switch_captured_at": "2026-08-01T00:00:00Z",
        "models": [{"slug": "gpt-5.4", "display_name": "GPT-5.4"}]
    });
    write_json(
        codex_dir.join("cc-switch-official-models-cache.json"),
        &old_baseline,
    );

    let new_official = json!({
        "fetched_at": "2026-08-04T00:00:00Z",
        "etag": "W/\"official-new\"",
        "client_version": "0.146.0",
        "models": [{"slug": "gpt-5.5", "display_name": "GPT-5.5"}]
    });
    write_json(codex_dir.join("models_cache.json"), &new_official);

    let settings = json!({
        "enableOfficialLogin": false,
        "codexCustomModels": [{
            "model": "gpt-5.2",
            "providerId": "deepseek",
            "upstreamModel": "deepseek-v4-flash"
        }]
    });
    write_codex_models_cache_for_aggregate_at(codex_dir.clone(), &settings, "", None)
        .expect("write aggregate cache");

    let baseline = read_json(codex_dir.join("cc-switch-official-models-cache.json"));
    assert_eq!(
        baseline.pointer("/models/0/slug").and_then(Value::as_str),
        Some("gpt-5.5"),
        "a clean official cache observed before aggregate rewrite must replace the old baseline"
    );
}

#[test]
fn write_codex_models_cache_for_provider_rebuilds_for_regular_provider() {
    let (_temp_home, codex_dir) = temp_codex_dir();

    // 残留缓存：既有官方模型，又有被覆盖成 DeepSeek 的条目。
    let stale_cache = json!({
        "fetched_at": "2026-08-01T00:00:00.000000000Z",
        "etag": "W/\"old\"",
        "client_version": "0.146.0",
        "models": [
            {"slug": "gpt-5.5", "display_name": "GPT-5.5"},
            {"slug": "gpt-5.2", "display_name": "DeepSeek V4 Flash"},
            {"slug": "deepseek-v4-pro", "display_name": "DeepSeek V4 Pro"}
        ]
    });
    write_json(codex_dir.join("models_cache.json"), &stale_cache);

    let provider = responses_provider(
        "deepseek",
        "DeepSeek",
        json!({
            "modelCatalog": {
                "models": [
                    {"model": "deepseek-v4-flash", "displayName": "DeepSeek V4 Flash", "contextWindow": 1048576},
                    {"model": "deepseek-v4-pro", "displayName": "DeepSeek V4 Pro", "contextWindow": 1048576}
                ]
            },
            "config": "model_provider = \"custom\"\nmodel = \"deepseek-v4-flash\"\n[model_providers.custom]\nbase_url = \"https://api.deepseek.com\"\nwire_api = \"responses\"\n"
        }),
    );

    write_codex_models_cache_for_provider_at(codex_dir.clone(), &provider, "", None)
        .expect("rebuild must succeed");

    let written = read_json(codex_dir.join("models_cache.json"));
    let models = written
        .get("models")
        .and_then(|v| v.as_array())
        .expect("models array");
    let slugs: Vec<&str> = models
        .iter()
        .filter_map(|m| m.get("slug").and_then(|s| s.as_str()))
        .collect();
    assert_eq!(
        slugs,
        vec!["deepseek-v4-flash", "deepseek-v4-pro"],
        "regular provider cache must contain only its own catalog"
    );
    assert!(
        !slugs.contains(&"gpt-5.5"),
        "stale official entries must be cleared"
    );
    assert!(
        written
            .get("etag")
            .and_then(|value| value.as_str())
            .is_some_and(|etag| etag.starts_with("W/\"cc-switch-")),
        "regular-provider rewrites must be marked as cc-switch-owned"
    );
}

#[test]
fn write_codex_models_cache_for_official_login_merges_custom_entries() {
    let (_temp_home, codex_dir) = temp_codex_dir();

    // 缓存已有官方 gpt-5.5（官方登录拉取后 Codex 自行写入）。
    let stale_cache = json!({
        "fetched_at": "2026-08-01T00:00:00.000000000Z",
        "etag": "W/\"old\"",
        "client_version": "0.146.0",
        "models": [{"slug": "gpt-5.5", "display_name": "GPT-5.5"}]
    });
    write_json(codex_dir.join("models_cache.json"), &stale_cache);
    write_json(
        codex_dir.join("cc-switch-official-models-cache.json"),
        &stale_cache,
    );

    // 官方登录 + 自定义模型：桌面端读 models_cache.json，需把自定义条目
    // 合并进去，否则官方登录下看不到聚合模型。
    let provider = official_provider(json!({
        "enableOfficialLogin": true,
        "codexCustomModels": [{
            "model": "gpt-5.2",
            "providerId": "deepseek",
            "upstreamModel": "deepseek-v4-flash",
            "displayName": "DeepSeek V4 Flash"
        }]
    }));

    write_codex_models_cache_for_provider_at(codex_dir.clone(), &provider, "", None)
        .expect("official-login aggregation write must succeed");

    let written = read_json(codex_dir.join("models_cache.json"));
    let models = written
        .get("models")
        .and_then(|v| v.as_array())
        .expect("models array");
    let slugs: Vec<&str> = models
        .iter()
        .filter_map(|m| m.get("slug").and_then(|s| s.as_str()))
        .collect();
    assert!(
        slugs.contains(&"gpt-5.5"),
        "official model must be preserved under official login: {slugs:?}"
    );
    assert!(
        slugs.contains(&"gpt-5.2"),
        "custom model must be merged into the desktop cache: {slugs:?}"
    );
    assert!(
        written
            .get("etag")
            .and_then(|value| value.as_str())
            .is_some_and(|etag| etag.starts_with("W/\"cc-switch-")),
        "official-login aggregation must mark the rendered cache as cc-switch-owned"
    );

    let baseline = read_json(codex_dir.join("cc-switch-official-models-cache.json"));
    let baseline_slugs: Vec<&str> = baseline
        .get("models")
        .and_then(|value| value.as_array())
        .into_iter()
        .flatten()
        .filter_map(|model| model.get("slug").and_then(|slug| slug.as_str()))
        .collect();
    assert_eq!(
        baseline_slugs,
        vec!["gpt-5.5"],
        "the sidecar must retain the clean official catalog without custom entries"
    );

    write_codex_models_cache_for_provider_at(codex_dir.clone(), &provider, "", None)
        .expect("repeated official-login aggregation must reuse the saved baseline");
    let repeated = read_json(codex_dir.join("models_cache.json"));
    let repeated_slugs: Vec<&str> = repeated
        .get("models")
        .and_then(|value| value.as_array())
        .into_iter()
        .flatten()
        .filter_map(|model| model.get("slug").and_then(|slug| slug.as_str()))
        .collect();
    assert!(repeated_slugs.contains(&"gpt-5.5"));
    assert!(repeated_slugs.contains(&"gpt-5.2"));
}

#[test]
fn official_login_cache_omits_official_slot_with_missing_custom_provider() {
    let (_temp_home, codex_dir) = temp_codex_dir();
    let official_cache = json!({
        "fetched_at": "2026-08-11T00:00:00Z",
        "etag": "W/\"official\"",
        "client_version": "0.147.0",
        "models": [
            {"slug": "gpt-5.5", "display_name": "GPT-5.5"},
            {"slug": "gpt-5.2", "display_name": "GPT-5.2"}
        ]
    });
    for filename in [
        "models_cache.json",
        CC_SWITCH_CODEX_OFFICIAL_MODELS_CACHE_FILENAME,
    ] {
        write_json(codex_dir.join(filename), &official_cache);
    }
    let settings = json!({
        "enableOfficialLogin": true,
        "codexCustomModels": [{
            "model": "gpt-5.2",
            "providerId": "deleted-provider",
            "upstreamModel": "deepseek-v4-flash"
        }]
    });
    let resolve_provider = |_: &str| -> Option<Provider> { None };

    write_codex_models_cache_for_official_login_at(
        codex_dir.clone(),
        &settings,
        "",
        Some(&resolve_provider),
    )
    .expect("render official-login cache");

    let written = read_json(codex_dir.join("models_cache.json"));
    let slugs: Vec<&str> = written["models"]
        .as_array()
        .expect("models array")
        .iter()
        .filter_map(|model| model.get("slug").and_then(Value::as_str))
        .collect();

    assert_eq!(
        slugs,
        vec!["gpt-5.5"],
        "a missing custom binding must also suppress its colliding official slot"
    );
    assert!(
        written
            .get("etag")
            .and_then(Value::as_str)
            .is_some_and(|etag| etag.starts_with("W/\"cc-switch-")),
        "suppressing a colliding official row must mark the cache as rewritten"
    );
}

#[test]
fn official_login_cache_preserves_clean_baseline_for_missing_noncolliding_provider() {
    let (_temp_home, codex_dir) = temp_codex_dir();
    let official_cache = json!({
        "fetched_at": "2026-08-11T00:00:00Z",
        "etag": "W/\"official\"",
        "client_version": "0.147.0",
        "models": [{"slug": "gpt-5.5", "display_name": "GPT-5.5"}]
    });
    for filename in [
        "models_cache.json",
        CC_SWITCH_CODEX_OFFICIAL_MODELS_CACHE_FILENAME,
    ] {
        write_json(codex_dir.join(filename), &official_cache);
    }
    let settings = json!({
        "enableOfficialLogin": true,
        "codexCustomModels": [{
            "model": "custom-only-slot",
            "providerId": "deleted-provider"
        }]
    });
    let resolve_provider = |_: &str| -> Option<Provider> { None };

    write_codex_models_cache_for_official_login_at(
        codex_dir.clone(),
        &settings,
        "",
        Some(&resolve_provider),
    )
    .expect("process official-login cache");

    let written = read_json(codex_dir.join("models_cache.json"));
    assert_eq!(
        written, official_cache,
        "a skipped noncolliding mapping must not rewrite a clean official cache"
    );
}

#[test]
fn write_codex_models_cache_for_official_login_preserves_trusted_official_cache() {
    let (_temp_home, codex_dir) = temp_codex_dir();

    let stale_cache = json!({
        "fetched_at": "2026-08-01T00:00:00.000000000Z",
        "etag": "W/\"old\"",
        "client_version": "0.146.0",
        "models": [{"slug": "gpt-5.5", "display_name": "GPT-5.5"}]
    });
    write_json(codex_dir.join("models_cache.json"), &stale_cache);
    write_json(
        codex_dir.join("cc-switch-official-models-cache.json"),
        &stale_cache,
    );

    // 已建立可信 sidecar 时，官方登录且无自定义模型应保持纯官方缓存。
    let provider = official_provider(json!({ "enableOfficialLogin": true }));

    write_codex_models_cache_for_provider_at(codex_dir.clone(), &provider, "", None)
        .expect("trusted official cache handling must not error");

    let written = read_json(codex_dir.join("models_cache.json"));
    assert_eq!(
        written
            .get("models")
            .and_then(|v| v.as_array())
            .map(|m| m.len()),
        Some(1),
        "trusted official cache must be preserved without custom models"
    );
}

#[test]
fn official_login_without_custom_models_restores_saved_baseline() {
    let (_temp_home, codex_dir) = temp_codex_dir();

    let rendered_cache = json!({
        "fetched_at": "2026-08-04T00:00:00Z",
        "etag": "W/\"cc-switch-1754265600\"",
        "client_version": "0.146.0",
        "models": [{"slug": "deepseek-v4", "display_name": "DeepSeek V4"}]
    });
    let official_baseline = json!({
        "fetched_at": "2026-08-03T00:00:00Z",
        "etag": "W/\"official-clean\"",
        "client_version": "0.146.0",
        "models": [{"slug": "gpt-5.5", "display_name": "GPT-5.5"}]
    });
    write_json(codex_dir.join("models_cache.json"), &rendered_cache);
    write_json(
        codex_dir.join("cc-switch-official-models-cache.json"),
        &official_baseline,
    );

    let provider = official_provider(json!({ "enableOfficialLogin": true }));

    write_codex_models_cache_for_provider_at(codex_dir.clone(), &provider, "", None)
        .expect("restore official baseline");

    let restored = read_json(codex_dir.join("models_cache.json"));
    assert_eq!(
        restored.pointer("/models/0/slug").and_then(Value::as_str),
        Some("gpt-5.5"),
        "switching back to plain official login must replace the fresh rendered cache"
    );
    assert_eq!(
        restored.get("etag").and_then(Value::as_str),
        Some("W/\"official-clean\"")
    );
}

#[test]
fn write_codex_models_cache_for_provider_aggregate_delegates_to_merge() {
    let (_temp_home, codex_dir) = temp_codex_dir();

    let stale_cache = json!({
        "fetched_at": "2026-08-01T00:00:00.000000000Z",
        "etag": "W/\"old\"",
        "client_version": "0.146.0",
        "models": [{"slug": "gpt-5.5", "display_name": "GPT-5.5"}]
    });
    write_json(codex_dir.join("models_cache.json"), &stale_cache);

    // 聚合模式（官方 + 关登录）：只保留映射到供应商的自定义模型，
    // 旧缓存里的官方/残留条目全部清掉。
    let provider = official_provider(json!({
        "enableOfficialLogin": false,
        "codexCustomModels": [{
            "model": "deepseek-v4-flash",
            "providerId": "deepseek",
            "displayName": "DeepSeek V4 Flash",
            "contextWindow": 131072
        }]
    }));

    write_codex_models_cache_for_provider_at(codex_dir.clone(), &provider, "", None)
        .expect("aggregate write must succeed");

    let written = read_json(codex_dir.join("models_cache.json"));
    let slugs: Vec<&str> = written
        .get("models")
        .and_then(|v| v.as_array())
        .into_iter()
        .flatten()
        .filter_map(|m| m.get("slug").and_then(|s| s.as_str()))
        .collect();
    assert!(
        !slugs.contains(&"gpt-5.5"),
        "stale official entries must be cleared in aggregate mode: {slugs:?}"
    );
    assert!(
        slugs.contains(&"deepseek-v4-flash"),
        "custom model must be present in aggregate mode: {slugs:?}"
    );
}

#[test]
fn write_codex_models_cache_for_official_login_clears_without_official_baseline() {
    let (_temp_home, codex_dir) = temp_codex_dir();

    // 空缓存（models 为空数组）：没有可靠官方基线，删除以触发官方拉取。
    let empty_cache = json!({
        "fetched_at": "2026-08-01T00:00:00.000000000Z",
        "etag": "W/\"old\"",
        "client_version": "0.146.0",
        "models": []
    });
    write_json(codex_dir.join("models_cache.json"), &empty_cache);

    let settings = json!({
        "enableOfficialLogin": true,
        "codexCustomModels": [{
            "model": "gpt-5.2",
            "providerId": "deepseek",
            "upstreamModel": "deepseek-v4-flash",
            "displayName": "DeepSeek V4 Flash"
        }]
    });

    write_codex_models_cache_for_official_login_at(codex_dir.clone(), &settings, "", None)
        .expect("clear must not error");

    assert!(
        !codex_dir.join("models_cache.json").exists(),
        "an unusable cache must be removed so Codex can fetch an official baseline"
    );
}

#[test]
fn write_codex_models_cache_for_official_login_removes_aggregate_leftover_cache() {
    let (_temp_home, codex_dir) = temp_codex_dir();

    // 缓存是聚合模式（关闭官方登录）写的：etag 是 cc-switch 生成的，
    // 里面的条目不是官方基线，删除缓存让 Codex 立即拉官方模型。
    let aggregate_cache = json!({
        "fetched_at": "2026-08-01T00:00:00.000000000Z",
        "etag": "W/\"cc-switch-1754000000\"",
        "client_version": "0.146.0",
        "models": [{"slug": "gpt-5.2", "display_name": "DeepSeek V4 Flash"}]
    });
    write_json(codex_dir.join("models_cache.json"), &aggregate_cache);

    let settings = json!({
        "enableOfficialLogin": true,
        "codexCustomModels": [{
            "model": "gpt-5.5",
            "providerId": "glm",
            "upstreamModel": "glm-5",
            "displayName": "GLM-5"
        }]
    });

    write_codex_models_cache_for_official_login_at(codex_dir.clone(), &settings, "", None)
        .expect("clear must not error");

    assert!(
        !codex_dir.join("models_cache.json").exists(),
        "a fresh aggregate cache must be removed so official login refetches immediately"
    );
}

#[test]
fn official_login_quarantines_unmarked_legacy_cache_until_official_refetch() {
    let (_temp_home, codex_dir) = temp_codex_dir();

    let legacy_rendered = json!({
        "fetched_at": "2026-08-03T17:00:00Z",
        "etag": "W/\"official-etag-preserved-by-legacy-cc-switch\"",
        "client_version": "0.146.0",
        "models": [
            {"slug": "gpt-5.5", "display_name": "GPT-5.5"},
            {"slug": "gpt-5.2", "display_name": "DeepSeek V4 Flash"}
        ]
    });
    write_json(codex_dir.join("models_cache.json"), &legacy_rendered);

    let settings = json!({
        "enableOfficialLogin": true,
        "codexCustomModels": [{
            "model": "gpt-5.2",
            "providerId": "deepseek",
            "upstreamModel": "deepseek-v4-flash",
            "displayName": "DeepSeek V4 Flash"
        }]
    });
    write_codex_models_cache_for_official_login_at(codex_dir.clone(), &settings, "", None)
        .expect("quarantine legacy cache");
    assert!(
        !codex_dir.join("models_cache.json").exists(),
        "an unmarked pre-sidecar cache must be removed so Codex fetches a clean official catalog"
    );

    let refetched_official = json!({
        "fetched_at": "2026-08-04T01:00:00Z",
        "etag": "W/\"official-refetched\"",
        "client_version": "0.146.0",
        "models": [
            {"slug": "gpt-5.5", "display_name": "GPT-5.5"},
            {"slug": "gpt-5.2", "display_name": "GPT-5.2"}
        ]
    });
    write_json(codex_dir.join("models_cache.json"), &refetched_official);
    write_codex_models_cache_for_official_login_at(codex_dir.clone(), &settings, "", None)
        .expect("merge after official refetch");

    let baseline = read_json(codex_dir.join("cc-switch-official-models-cache.json"));
    assert_eq!(
        baseline
            .pointer("/models/1/display_name")
            .and_then(Value::as_str),
        Some("GPT-5.2"),
        "the clean sidecar must come from the refetched official catalog"
    );
    let rendered = read_json(codex_dir.join("models_cache.json"));
    assert_eq!(
        rendered
            .pointer("/models/1/display_name")
            .and_then(Value::as_str),
        Some("DeepSeek V4 Flash"),
        "the rendered cache must still apply the current custom mapping"
    );
}

#[test]
fn official_login_does_not_capture_proxy_merged_catalog_as_baseline() {
    let (_temp_home, codex_dir) = temp_codex_dir();

    let clean_baseline = json!({
        "fetched_at": "2026-08-08T00:00:00Z",
        "etag": "W/\"official-old\"",
        "client_version": "0.147.0",
        "cc_switch_captured_at": chrono::Utc::now().to_rfc3339(),
        "models": [{"slug": "gpt-5.5", "display_name": "GPT-5.5"}]
    });
    let proxy_merged_catalog = json!({
        "fetched_at": "2026-08-09T00:00:00Z",
        "etag": "W/\"official-new\"",
        "client_version": "0.147.0",
        "cc_switch_merged": true,
        "models": [
            {"slug": "gpt-5.5", "display_name": "GPT-5.5"},
            {"slug": "gpt-5.2", "display_name": "DeepSeek V4 Flash"}
        ]
    });
    write_json(
        codex_dir.join(CC_SWITCH_CODEX_OFFICIAL_MODELS_CACHE_FILENAME),
        &clean_baseline,
    );
    write_json(codex_dir.join("models_cache.json"), &proxy_merged_catalog);

    let settings = json!({
        "enableOfficialLogin": true,
        "codexCustomModels": [{
            "model": "gpt-5.2",
            "providerId": "deepseek",
            "upstreamModel": "deepseek-v4-flash",
            "displayName": "DeepSeek V4 Flash"
        }]
    });
    write_codex_models_cache_for_official_login_at(codex_dir.clone(), &settings, "", None)
        .expect("render official-login cache");

    let baseline = read_json(codex_dir.join(CC_SWITCH_CODEX_OFFICIAL_MODELS_CACHE_FILENAME));
    let baseline_slugs: Vec<&str> = baseline
        .get("models")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|model| model.get("slug").and_then(Value::as_str))
        .collect();
    assert_eq!(
        baseline_slugs,
        vec!["gpt-5.5"],
        "a proxy-merged catalog must never become the official baseline"
    );
}

#[test]
fn forwarded_official_catalog_replaces_awaiting_baseline_before_merge() {
    let (_temp_home, codex_dir) = temp_codex_dir();
    write_json(
        codex_dir.join(CC_SWITCH_CODEX_OFFICIAL_MODELS_CACHE_FILENAME),
        &json!({
            "cc_switch_state": "awaiting_official_refresh"
        }),
    );

    let clean_catalog = json!({
        "models": [{"slug": "gpt-5.5", "display_name": "GPT-5.5"}]
    });
    capture_forwarded_codex_official_models_baseline_at(
        &codex_dir,
        &clean_catalog,
        "0.147.0",
        Some("W/\"official-catalog\""),
    )
    .expect("capture clean forwarded catalog");

    let baseline = read_json(codex_dir.join(CC_SWITCH_CODEX_OFFICIAL_MODELS_CACHE_FILENAME));
    assert_eq!(baseline["models"], clean_catalog["models"]);
    assert_eq!(baseline["client_version"], "0.147.0");
    assert_eq!(baseline["etag"], "W/\"official-catalog\"");
    assert!(baseline.get("fetched_at").and_then(Value::as_str).is_some());
    assert!(baseline
        .get(CODEX_OFFICIAL_BASELINE_CAPTURED_AT_KEY)
        .and_then(Value::as_str)
        .is_some());
    assert!(baseline.get(CODEX_OFFICIAL_MODELS_MERGED_KEY).is_none());
    assert!(baseline.get(CODEX_OFFICIAL_BASELINE_STATE_KEY).is_none());
}

#[test]
fn forwarded_merged_catalog_cannot_replace_the_official_baseline() {
    let (_temp_home, codex_dir) = temp_codex_dir();
    let awaiting = json!({
        "cc_switch_state": "awaiting_official_refresh"
    });
    write_json(
        codex_dir.join(CC_SWITCH_CODEX_OFFICIAL_MODELS_CACHE_FILENAME),
        &awaiting,
    );

    let captured = capture_forwarded_codex_official_models_baseline_at(
        &codex_dir,
        &json!({
            "cc_switch_merged": true,
            "models": [{"slug": "custom-slot"}]
        }),
        "0.147.0",
        Some("W/\"cc-switch-merged\""),
    )
    .expect("reject merged catalog without an IO error");

    assert!(!captured);
    let saved = read_json(codex_dir.join(CC_SWITCH_CODEX_OFFICIAL_MODELS_CACHE_FILENAME));
    assert_eq!(saved, awaiting);
}

#[test]
fn forwarded_catalog_with_cc_switch_body_etag_cannot_be_laundered() {
    let (_temp_home, codex_dir) = temp_codex_path();

    let captured = capture_forwarded_codex_official_models_baseline_at(
        &codex_dir,
        &json!({
            "etag": "W/\"cc-switch-merged-1\"",
            "models": [{"slug": "custom-slot"}]
        }),
        "0.147.0",
        Some("W/\"official-http-etag\""),
    )
    .expect("reject cc-switch body ETag without an IO error");

    assert!(!captured);
    assert!(!codex_dir
        .join(CC_SWITCH_CODEX_OFFICIAL_MODELS_CACHE_FILENAME)
        .exists());
}

#[test]
fn official_login_expires_saved_baseline_instead_of_refreshing_it_forever() {
    let (_temp_home, codex_dir) = temp_codex_dir();

    let rendered_cache = json!({
        "fetched_at": chrono::Utc::now().to_rfc3339(),
        "etag": "W/\"cc-switch-recent\"",
        "client_version": "0.146.0",
        "models": [{"slug": "gpt-5.5"}, {"slug": "gpt-5.2"}]
    });
    let expired_baseline = json!({
        "fetched_at": "2026-08-01T00:00:00Z",
        "etag": "W/\"official-old\"",
        "client_version": "0.146.0",
        "cc_switch_captured_at": "2026-08-01T00:00:00Z",
        "models": [{"slug": "gpt-5.5"}]
    });
    write_json(codex_dir.join("models_cache.json"), &rendered_cache);
    write_json(
        codex_dir.join("cc-switch-official-models-cache.json"),
        &expired_baseline,
    );

    let settings = json!({
        "enableOfficialLogin": true,
        "codexCustomModels": [{
            "model": "gpt-5.2",
            "providerId": "deepseek",
            "upstreamModel": "deepseek-v4-flash"
        }]
    });
    write_codex_models_cache_for_official_login_at(codex_dir.clone(), &settings, "", None)
        .expect("expire official baseline");

    assert!(
        !codex_dir.join("models_cache.json").exists(),
        "an expired clean baseline must remove the fresh rendered cache so Codex refetches"
    );
    let sidecar = read_json(codex_dir.join("cc-switch-official-models-cache.json"));
    assert_eq!(
        sidecar.get("cc_switch_state").and_then(Value::as_str),
        Some("awaiting_official_refresh"),
        "the expired baseline must not be reused by the next 240-second refresh"
    );
}

#[test]
fn repeated_same_official_snapshot_does_not_extend_baseline_ttl() {
    let (_temp_home, codex_dir) = temp_codex_dir();

    let official_live = json!({
        "fetched_at": "2026-08-01T00:00:00Z",
        "etag": "W/\"official-unchanged\"",
        "client_version": "0.146.0",
        "models": [{"slug": "gpt-5.5"}]
    });
    let mut expired_sidecar = official_live.clone();
    expired_sidecar
        .as_object_mut()
        .expect("sidecar object")
        .insert(
            "cc_switch_captured_at".to_string(),
            Value::String("2026-08-01T00:00:00Z".to_string()),
        );
    write_json(codex_dir.join("models_cache.json"), &official_live);
    write_json(
        codex_dir.join("cc-switch-official-models-cache.json"),
        &expired_sidecar,
    );

    restore_or_clear_codex_official_models_cache(&codex_dir)
        .expect("expire unchanged official snapshot");

    assert!(
        !codex_dir.join("models_cache.json").exists(),
        "re-observing the same official fingerprint must not reset its capture time"
    );
}

#[test]
fn invalid_official_baseline_capture_time_forces_refetch() {
    for (case, captured_at) in [
        ("malformed", "not-a-timestamp"),
        ("far-future", "2099-01-01T00:00:00Z"),
    ] {
        let (_temp_home, codex_dir) = temp_codex_dir();

        let rendered_cache = json!({
            "fetched_at": chrono::Utc::now().to_rfc3339(),
            "etag": "W/\"cc-switch-recent\"",
            "models": [{"slug": "gpt-5.5"}]
        });
        let sidecar = json!({
            "fetched_at": "2026-08-04T00:00:00Z",
            "etag": "W/\"official-stale\"",
            "cc_switch_captured_at": captured_at,
            "models": [{"slug": "gpt-5.5"}]
        });
        write_json(codex_dir.join("models_cache.json"), &rendered_cache);
        write_json(
            codex_dir.join("cc-switch-official-models-cache.json"),
            &sidecar,
        );

        restore_or_clear_codex_official_models_cache(&codex_dir)
            .expect("handle invalid capture time");

        assert!(
            !codex_dir.join("models_cache.json").exists(),
            "{case} capture time must quarantine the sidecar and force an official refetch"
        );
    }
}

#[test]
fn official_baseline_capture_state_has_exact_ttl_boundaries() {
    let now = chrono::DateTime::parse_from_rfc3339("2026-08-04T12:00:00Z")
        .expect("parse fixed now")
        .with_timezone(&chrono::Utc);
    let state_for = |captured_at: Value| {
        codex_official_baseline_capture_state(&json!({ "cc_switch_captured_at": captured_at }), now)
    };

    assert_eq!(
        codex_official_baseline_capture_state(&json!({}), now),
        CodexOfficialBaselineCaptureState::Missing
    );
    assert_eq!(
        state_for(json!("2026-08-04T11:55:01Z")),
        CodexOfficialBaselineCaptureState::Fresh,
        "299 seconds must remain fresh"
    );
    assert_eq!(
        state_for(json!("2026-08-04T11:55:00Z")),
        CodexOfficialBaselineCaptureState::Expired,
        "300 seconds is the exact expiry boundary"
    );
    assert_eq!(
        state_for(json!("2026-08-04T11:54:59Z")),
        CodexOfficialBaselineCaptureState::Expired,
        "301 seconds must be expired"
    );
    assert_eq!(
        state_for(json!("not-a-timestamp")),
        CodexOfficialBaselineCaptureState::Invalid
    );
    assert_eq!(
        state_for(json!(42)),
        CodexOfficialBaselineCaptureState::Invalid
    );
    assert_eq!(
        state_for(json!("2026-08-04T12:01:01Z")),
        CodexOfficialBaselineCaptureState::Invalid,
        "capture times beyond the clock-skew allowance must be quarantined"
    );
}

#[test]
fn write_codex_models_cache_derives_single_entry_from_top_level_model() {
    let (_temp_home, codex_dir) = temp_codex_dir();
    write_json(
        codex_dir.join("models_cache.json"),
        &json!({
            "client_version": "0.146.0",
            "models": []
        }),
    );

    // 供应商只有顶层 model、没有 modelCatalog：从配置的 model 派生单条缓存，
    // 桌面端至少能发现默认模型，而不是写入空列表。
    let config_text = "model_provider = \"custom\"\nmodel = \"deepseek-v4-flash\"\n[model_providers.custom]\nbase_url = \"https://api.deepseek.com\"\nwire_api = \"responses\"\n";
    let provider = responses_provider("deepseek", "DeepSeek", json!({}));

    write_codex_models_cache_for_provider_at(codex_dir.clone(), &provider, config_text, None)
        .expect("write must succeed");

    let written = read_json(codex_dir.join("models_cache.json"));
    let models = written
        .get("models")
        .and_then(|v| v.as_array())
        .expect("models array");
    assert_eq!(models.len(), 1, "single entry derived from top-level model");
    assert_eq!(
        models[0].get("slug").and_then(|v| v.as_str()),
        Some("deepseek-v4-flash"),
        "derived entry carries the configured model slug"
    );
}

#[test]
fn custom_model_entries_parse_and_dedupe() {
    let settings = json!({
        "codexCustomModels": [
            {
                "model": "my-deepseek",
                "providerId": "prov-1",
                "upstreamModel": "deepseek-chat",
                "displayName": "DeepSeek via cc-switch",
                "contextWindow": 128000,
                "inputModalities": ["text"]
            },
            {
                "model": "my-deepseek",
                "providerId": "prov-2"
            },
            {
                "model": "  ",
                "providerId": "prov-3"
            },
            {
                "model": "snake-case",
                "provider_id": "prov-4",
                "upstream_model": "deepseek-reasoner"
            },
            {
                "model": "my-deepseek[1M]",
                "providerId": "prov-5"
            }
        ]
    });
    let entries = codex_custom_model_entries(&settings);
    assert_eq!(
        entries.len(),
        2,
        "duplicate (including [1M]-suffixed) and empty model ids are skipped"
    );
    assert_eq!(entries[0].model, "my-deepseek");
    assert_eq!(entries[0].provider_id, "prov-1");
    assert_eq!(entries[0].upstream_model.as_deref(), Some("deepseek-chat"));
    assert_eq!(entries[0].context_window, Some(128_000));
    assert_eq!(
        entries[0].input_modalities.as_deref(),
        Some(&["text".to_string()][..])
    );
    assert_eq!(entries[1].model, "snake-case");
    assert_eq!(entries[1].provider_id, "prov-4");
    assert_eq!(
        entries[1].upstream_model.as_deref(),
        Some("deepseek-reasoner")
    );
}
