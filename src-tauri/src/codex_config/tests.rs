use super::*;
use crate::test_support::TestHome;
use serde_json::json;
use serial_test::serial;

mod reasoning_overrides;

#[test]
fn official_proxy_route_is_recognized_by_its_address() {
    let proxy = |url: &str| url == "http://127.0.0.1:15721/v1";
    for (config, expected) in [
        ("openai_base_url = \"http://127.0.0.1:15721/v1/\"\n", true),
        ("model_provider = \"openai\"\nopenai_base_url = \"http://127.0.0.1:15721/v1\"\n", true),
        ("model_provider = \"custom\"\n[model_providers.custom]\nname = \"OpenAI\"\nbase_url = \"http://127.0.0.1:15721/v1\"\nrequires_openai_auth = true\n", true),
        // 别的地址（比如其他工具改道到自己的本地服务）不算。
        ("openai_base_url = \"http://127.0.0.1:10531/v1\"\n", false),
        // 选了别的 provider，改道不生效。
        ("model_provider = \"relay\"\nopenai_base_url = \"http://127.0.0.1:15721/v1\"\n", false),
        // 官方直连的统一会话镜像表没有地址。
        ("model_provider = \"custom\"\n[model_providers.custom]\nname = \"OpenAI\"\nrequires_openai_auth = true\n", false),
        ("model = \"gpt-5.5\"\n", false),
    ] {
        assert_eq!(
            codex_config_routes_official_to_proxy(config, proxy),
            expected,
            "{config}"
        );
    }
}

#[test]
fn codex_id_token_user_identity_requires_a_nonempty_subject() {
    let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"none"}"#);
    let subject_payload = URL_SAFE_NO_PAD.encode(json!({ "sub": "stable-user" }).to_string());
    assert_eq!(
        extract_codex_id_token_user_identity(&test_codex_id_token("stable-user")),
        Some("sub:stable-user".to_string())
    );
    assert_eq!(extract_codex_id_token_user_identity("not-a-jwt"), None);
    assert_eq!(
        extract_codex_id_token_user_identity(&format!("{header}.{subject_payload}")),
        None
    );
    assert_eq!(
        extract_codex_id_token_user_identity(&format!("{header}.{subject_payload}..extra")),
        None
    );
    assert_eq!(
        extract_codex_id_token_user_identity(&format!("invalid.{subject_payload}.signature")),
        None
    );
    assert_eq!(
        extract_codex_id_token_user_identity(&test_codex_id_token("   ")),
        None
    );

    let payload = URL_SAFE_NO_PAD.encode(json!({ "email": "user@example.test" }).to_string());
    assert_eq!(
        extract_codex_id_token_user_identity(&format!("{header}.{payload}.")),
        None
    );
}

#[derive(Debug, PartialEq)]
struct CodexLiveTestState {
    auth_bytes: Vec<u8>,
    auth_value: Value,
    config_bytes: Vec<u8>,
    config_value: toml::Value,
    catalog_bytes: Vec<u8>,
    catalog_value: Value,
    marker_bytes: Vec<u8>,
    marker_value: Value,
}

fn capture_codex_live_test_state() -> CodexLiveTestState {
    let auth_bytes = fs::read(get_codex_auth_path()).expect("read live auth bytes");
    let config_bytes = fs::read(get_codex_config_path()).expect("read live config bytes");
    let catalog_bytes = fs::read(get_codex_model_catalog_path()).expect("read live catalog bytes");
    let marker_bytes = fs::read(get_codex_managed_oauth_live_auth_marker_path())
        .expect("read managed auth marker bytes");

    CodexLiveTestState {
        auth_value: serde_json::from_slice(&auth_bytes).expect("parse live auth"),
        config_value: toml::from_str(
            std::str::from_utf8(&config_bytes).expect("live config must be UTF-8"),
        )
        .expect("parse live config"),
        catalog_value: serde_json::from_slice(&catalog_bytes).expect("parse live catalog"),
        marker_value: serde_json::from_slice(&marker_bytes).expect("parse managed auth marker"),
        auth_bytes,
        config_bytes,
        catalog_bytes,
        marker_bytes,
    }
}

fn seed_rotated_managed_codex_live_state() -> CodexLiveTestState {
    let id_token = test_codex_id_token("user-a");
    let auth = codex_managed_oauth_auth_value(
        "account-a",
        "access-r1",
        Some(&id_token),
        "refresh-r1",
        "2026-08-06T00:00:01Z",
    );
    crate::config::write_json_file(&get_codex_auth_path(), &auth).expect("seed live auth R1");
    crate::config::write_text_file(
        &get_codex_config_path(),
        "# cas-guard-sentinel\nmodel = \"gpt-5.5\"\nmodel_catalog_json = \"cc-switch-model-catalog.json\"\n",
    )
    .expect("seed live config");
    crate::config::write_json_file(
        &get_codex_model_catalog_path(),
        &json!({ "models": [{ "slug": "cas-guard-sentinel" }] }),
    )
    .expect("seed live catalog");
    record_codex_managed_oauth_live_auth(&auth, "account-a").expect("seed managed auth marker");

    capture_codex_live_test_state()
}

#[test]
#[serial]
fn ensure_live_auth_guard_rejects_rotated_refresh_without_mutating_live_bundle() {
    let _home = TestHome::new();
    let before = seed_rotated_managed_codex_live_state();

    let result = ensure_codex_live_auth_unchanged_for_managed_account("account-a", "refresh-r0");

    assert!(result.is_err(), "R1 live auth must reject an expected R0");
    assert_eq!(capture_codex_live_test_state(), before);
}

#[test]
#[serial]
fn clear_live_auth_guard_rejects_rotated_refresh_without_mutating_live_bundle() {
    let _home = TestHome::new();
    let before = seed_rotated_managed_codex_live_state();

    let result =
        clear_codex_live_auth_for_managed_account_if_unchanged("account-a", Some("refresh-r0"));

    assert!(result.is_err(), "R1 live auth must reject an expected R0");
    assert_eq!(capture_codex_live_test_state(), before);
}

#[test]
fn catalog_tool_profile_from_api_format() {
    assert_eq!(
        CodexCatalogToolProfile::from_api_format(Some("anthropic")),
        CodexCatalogToolProfile::Anthropic
    );
    assert_eq!(
        CodexCatalogToolProfile::from_api_format(Some("openai_responses")),
        CodexCatalogToolProfile::NativeResponses
    );
    assert_eq!(
        CodexCatalogToolProfile::from_api_format(Some("openai_chat")),
        CodexCatalogToolProfile::ProxyChat
    );
    assert_eq!(
        CodexCatalogToolProfile::from_api_format(None),
        CodexCatalogToolProfile::ProxyChat
    );
}

#[test]
fn unified_session_bucket_strip_keeps_third_party_custom_entry() {
    // 第三方模板同样用 custom 路由，但条目带 base_url 等差异字段，
    // 形态不等于注入产物，必须原样保留。
    let third_party = r#"model_provider = "custom"

[model_providers.custom]
name = "Relay"
base_url = "https://relay.example/v1"
wire_api = "responses"
requires_openai_auth = true
"#;
    let untouched = strip_codex_unified_session_bucket(third_party).expect("strip");
    assert_eq!(untouched, third_party);
}

#[test]
fn extract_base_url_prefers_active_provider_section() {
    let input = r#"model_provider = "azure"

[model_providers.azure]
base_url = "https://azure.example.com/v1"

[model_providers.other]
base_url = "https://other.example.com/v1"
"#;

    assert_eq!(
        extract_codex_base_url(input).as_deref(),
        Some("https://azure.example.com/v1")
    );
}

#[test]
fn extract_base_url_falls_back_to_top_level_only() {
    let top_level = r#"base_url = "https://top-level.example.com/v1""#;
    assert_eq!(
        extract_codex_base_url(top_level).as_deref(),
        Some("https://top-level.example.com/v1")
    );
}

// Mirrors the frontend extractCodexBaseUrl: a non-active provider section
// is never a credential source, whether the active provider points
// elsewhere (e.g. the built-in "openai") or none is selected at all.
#[test]
fn extract_base_url_ignores_non_active_provider_sections() {
    let mismatched = r#"model_provider = "openai"

[model_providers.custom]
base_url = "https://leftover.example.com/v1"
"#;
    assert_eq!(extract_codex_base_url(mismatched), None);

    let no_active = r#"[model_providers.any]
base_url = "https://single.example.com/v1"
"#;
    assert_eq!(extract_codex_base_url(no_active), None);
}

#[test]
#[serial]
fn managed_chatgpt_login_matches_local_marker_and_workspace() {
    let _home = TestHome::new();
    let shared_chatgpt_user_token = |subject: &str| {
        let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"none"}"#);
        let payload = URL_SAFE_NO_PAD.encode(
            json!({
                "sub": subject,
                "https://api.openai.com/auth": {
                    "chatgpt_user_id": "shared-team-user-id"
                }
            })
            .to_string(),
        );
        format!("{header}.{payload}.")
    };
    // 原生 auth 保留 workspace ID；marker 用本地 ID 区分同 workspace 登录。
    let full_bundle = json!({
        "auth_mode": "chatgpt",
        "OPENAI_API_KEY": null,
        "tokens": {
            "id_token": shared_chatgpt_user_token("user-a"),
            "access_token": "access",
            "refresh_token": "refresh-secret",
            "account_id": "workspace-shared"
        },
        "last_refresh": "2026-01-02T03:04:05.000000000Z"
    });
    record_codex_managed_oauth_live_auth(&full_bundle, "local-account-a")
        .expect("record managed auth marker");
    crate::config::write_json_file(&get_codex_auth_path(), &full_bundle)
        .expect("write managed live auth");
    assert!(
        codex_live_auth_matches_managed_request("local-account-a", "access").unwrap(),
        "the selected account's exact live bearer must match"
    );
    assert!(
        !codex_live_auth_matches_managed_request("local-account-a", "other-access").unwrap(),
        "another user's bearer in the same workspace must not match"
    );
    let managed_id_token = full_bundle
        .pointer("/tokens/id_token")
        .and_then(Value::as_str)
        .expect("managed id token");
    assert!(
        codex_live_auth_is_managed_chatgpt_login(&full_bundle, "local-account-a"),
        "a full refreshable bundle for the managed account must be recognized"
    );
    assert!(
        !codex_live_auth_is_managed_chatgpt_login(&full_bundle, "local-account-b"),
        "another local login in the same workspace must not match"
    );
    let mut other_user = full_bundle.clone();
    other_user["tokens"]["id_token"] = json!(shared_chatgpt_user_token("user-b"));
    assert!(
        !codex_live_auth_is_managed_chatgpt_login(&other_user, "local-account-a"),
        "a native login for another user in the same workspace must not match"
    );
    crate::config::write_json_file(&get_codex_auth_path(), &other_user)
        .expect("write other user's native login");
    assert!(
        read_codex_live_auth_refresh_for_account("local-account-a").is_none(),
        "another user's refresh token must not be adopted"
    );
    clear_codex_live_auth_for_managed_account("local-account-a")
        .expect("clear stale local ownership marker");
    assert!(
        get_codex_auth_path().exists(),
        "removing local account A must not delete native account B"
    );

    crate::config::write_json_file(
        &get_codex_managed_oauth_live_auth_marker_path(),
        &json!({
            "version": 2,
            "account_id": "workspace-shared"
        }),
    )
    .expect("write legacy managed auth marker");
    assert!(
        read_codex_live_auth_refresh_for_managed_account(
            "workspace-shared",
            Some(managed_id_token),
        )
        .is_err(),
        "a legacy marker must not migrate across users in one workspace"
    );
    assert!(
        !codex_live_auth_is_managed_chatgpt_login(&other_user, "workspace-shared"),
        "a legacy marker without user identity must not establish ownership"
    );
    assert!(
        read_codex_live_auth_refresh_for_account("workspace-shared").is_none(),
        "a legacy marker must not authorize refresh-token adoption"
    );
    clear_codex_live_auth_for_managed_account("workspace-shared")
        .expect("clear ambiguous legacy marker");
    assert!(
        get_codex_auth_path().exists(),
        "clearing an ambiguous legacy marker must preserve native auth"
    );

    // 非 chatgpt 模式（API key）不应命中。
    let api_key_auth = json!({ "OPENAI_API_KEY": "sk-live" });
    assert!(!codex_live_auth_is_managed_chatgpt_login(
        &api_key_auth,
        "local-account-a"
    ));
}

/// 原生 ChatGPT 登录没有 marker 是常态：不认所有权，也不算读取失败；marker 坏了同样
/// 不认。
#[test]
#[serial]
fn missing_or_malformed_marker_never_establishes_ownership() {
    let _home = TestHome::new();
    let id_token = test_codex_id_token("user-a");
    let auth = codex_managed_oauth_auth_value(
        "workspace-a",
        "access",
        Some(&id_token),
        "refresh",
        "2026-01-01T00:00:00Z",
    );
    crate::config::write_json_file(&get_codex_auth_path(), &auth).expect("write live auth");
    let marker = get_codex_managed_oauth_live_auth_marker_path();

    assert!(!marker.exists());
    assert!(!codex_auth_matches_recorded_managed_oauth(&auth, "local-account-a").unwrap());
    assert!(!codex_live_auth_matches_managed_request("local-account-a", "access").unwrap());

    crate::config::write_text_file(&marker, "{not json").expect("write malformed marker");
    assert!(!codex_auth_matches_recorded_managed_oauth(&auth, "local-account-a").unwrap());
}

#[test]
#[serial]
fn legacy_managed_marker_removal_requires_manager_identity() {
    let _home = TestHome::new();
    let id_token = test_codex_id_token("legacy-user");
    let auth = codex_managed_oauth_auth_value(
        "legacy-workspace",
        "access",
        Some(&id_token),
        "refresh",
        "2026-01-01T00:00:00Z",
    );
    crate::config::write_json_file(&get_codex_auth_path(), &auth).expect("write legacy live auth");
    crate::config::write_json_file(
        &get_codex_managed_oauth_live_auth_marker_path(),
        &json!({
            "version": 2,
            "account_id": "legacy-workspace"
        }),
    )
    .expect("write legacy marker");

    let other_user = test_codex_id_token("other-user");
    assert!(prepare_codex_live_auth_for_managed_account_removal(
        "legacy-workspace",
        Some(&other_user),
    )
    .is_err());
    assert!(get_codex_auth_path().exists());
    assert!(get_codex_managed_oauth_live_auth_marker_path().exists());

    prepare_codex_live_auth_for_managed_account_removal("legacy-workspace", Some(&id_token))
        .expect("prove and migrate legacy ownership");
    clear_codex_live_auth_for_managed_account("legacy-workspace")
        .expect("remove proven managed live auth");
    assert!(!get_codex_auth_path().exists());
    assert!(!get_codex_managed_oauth_live_auth_marker_path().exists());
}

#[test]
fn openai_account_material_mirrors_codex_account_probe() {
    assert!(codex_auth_has_openai_account_material(&json!({
        "OPENAI_API_KEY": "sk-test"
    })));
    assert!(codex_auth_has_openai_account_material(&json!({
        "auth_mode": "chatgpt",
        "tokens": { "access_token": "acc" }
    })));
    assert!(codex_auth_has_openai_account_material(&json!({
        "personal_access_token": "pat"
    })));
    // Bedrock credentials make account_state() fail on a
    // requires_openai_auth provider, so they must not count as a login.
    assert!(!codex_auth_has_openai_account_material(&json!({
        "bedrock_api_key": "bedrock"
    })));
    assert!(!codex_auth_has_openai_account_material(&json!({
        "last_refresh": "2026-09-01T00:00:00Z",
        "tokens": { "account_id": "acct" }
    })));
    assert!(!codex_auth_has_openai_account_material(&json!({
        "OPENAI_API_KEY": "   "
    })));

    // Precedence mirrors AuthDotJson::resolved_mode: an implicit Bedrock
    // credential outranks a leftover OPENAI_API_KEY, so the pair is still
    // Bedrock and must not be promoted to an OpenAI login.
    assert!(!codex_auth_has_openai_account_material(&json!({
        "OPENAI_API_KEY": "sk-stale",
        "bedrock_api_key": "bedrock"
    })));
    assert!(!codex_auth_has_openai_account_material(&json!({
        "OPENAI_API_KEY": "sk-stale",
        "bedrock_access_keys": { "access_key_id": "a", "secret_access_key": "s" }
    })));
    // An explicit auth_mode wins outright, in both directions.
    assert!(!codex_auth_has_openai_account_material(&json!({
        "auth_mode": "bedrockApiKey",
        "OPENAI_API_KEY": "sk-stale",
        "bedrock_api_key": "bedrock"
    })));
    assert!(codex_auth_has_openai_account_material(&json!({
        "auth_mode": "apikey",
        "OPENAI_API_KEY": "sk-live",
        "bedrock_api_key": "bedrock"
    })));
    // personal_access_token outranks the API key even when blank: Codex
    // then attempts PAT auth with nothing and ends up signed out.
    assert!(!codex_auth_has_openai_account_material(&json!({
        "personal_access_token": "",
        "OPENAI_API_KEY": "sk-live"
    })));
    // agent_identity only counts under an explicit mode; implicitly the
    // payload resolves to ChatGPT, which has no tokens here.
    assert!(!codex_auth_has_openai_account_material(&json!({
        "agent_identity": "jwt"
    })));
    assert!(codex_auth_has_openai_account_material(&json!({
        "auth_mode": "agentIdentity",
        "agent_identity": "jwt"
    })));
    // `auth_mode: null` is absent to serde; a string it rejects fails the
    // whole load (exact-match, so casing matters); headers auth cannot be
    // loaded from storage at all.
    assert!(codex_auth_has_openai_account_material(&json!({
        "auth_mode": null,
        "OPENAI_API_KEY": "sk-live"
    })));
    assert!(!codex_auth_has_openai_account_material(&json!({
        "auth_mode": "ApiKey",
        "OPENAI_API_KEY": "sk-live"
    })));
    assert!(!codex_auth_has_openai_account_material(&json!({
        "auth_mode": "headers",
        "OPENAI_API_KEY": "sk-live"
    })));
}

#[test]
fn auth_store_mode_reads_top_level_cli_auth_credentials_store() {
    assert_eq!(
        codex_config_auth_store_mode("model = \"gpt-5\"\n"),
        CodexAuthStoreMode::File
    );
    assert_eq!(
        codex_config_auth_store_mode("cli_auth_credentials_store = \"file\"\n"),
        CodexAuthStoreMode::File
    );
    assert_eq!(
        codex_config_auth_store_mode("cli_auth_credentials_store = \"keyring\"\n"),
        CodexAuthStoreMode::Keyring
    );
    assert_eq!(
        codex_config_auth_store_mode("cli_auth_credentials_store = \"auto\"\n"),
        CodexAuthStoreMode::Auto
    );
    assert_eq!(
        codex_config_auth_store_mode("cli_auth_credentials_store = \"ephemeral\"\n"),
        CodexAuthStoreMode::Ephemeral
    );
    // Codex's serde is lowercase-only; anything else fails its load.
    assert_eq!(
        codex_config_auth_store_mode("cli_auth_credentials_store = \"Keyring\"\n"),
        CodexAuthStoreMode::Unknown
    );
    // Only the top-level key counts.
    assert_eq!(
        codex_config_auth_store_mode(
            "[model_providers.x]\ncli_auth_credentials_store = \"keyring\"\n"
        ),
        CodexAuthStoreMode::File
    );
}

#[test]
fn extract_bearer_uses_top_level_token_for_reserved_provider() {
    let input = r#"model_provider = "openai"
experimental_bearer_token = "top-level-key"

[model_providers.openai]
experimental_bearer_token = "stale-table-key"
"#;

    assert_eq!(
        extract_codex_experimental_bearer_token(input).as_deref(),
        Some("top-level-key")
    );
}

#[test]
fn credential_login_material_only_counts_real_credentials() {
    assert!(codex_auth_has_credential_login_material(&json!({
        "tokens": { "access_token": "t" }
    })));
    assert!(codex_auth_has_credential_login_material(&json!({
        "tokens": { "refresh_token": "r" }
    })));
    assert!(codex_auth_has_credential_login_material(&json!({
        "personal_access_token": "pat"
    })));

    // API key and pure metadata are not credentials in this predicate's
    // sense — they must not shield a stale key from cleanup.
    assert!(!codex_auth_has_credential_login_material(&json!({
        "OPENAI_API_KEY": "sk-x"
    })));
    assert!(!codex_auth_has_credential_login_material(&json!({
        "OPENAI_API_KEY": "sk-x",
        "last_refresh": "2026-01-01T00:00:00Z",
        "tokens": { "account_id": "acct-meta-only" }
    })));
    assert!(!codex_auth_has_credential_login_material(&json!({})));
}

#[test]
fn stale_third_party_residue_detection() {
    // Shapes a preserve-off third-party switch leaves behind: cleared.
    assert!(codex_live_auth_is_stale_third_party_residue(&json!({
        "OPENAI_API_KEY": "sk-third-party"
    })));
    assert!(codex_live_auth_is_stale_third_party_residue(&json!({
        "auth_mode": "apikey",
        "OPENAI_API_KEY": "sk-third-party"
    })));
    assert!(codex_live_auth_is_stale_third_party_residue(&json!({
        "OPENAI_API_KEY": "sk-third-party",
        "last_refresh": "2026-01-01T00:00:00Z",
        "tokens": { "account_id": "acct-meta-only" }
    })));

    // Anything carrying a real credential must survive untouched.
    assert!(!codex_live_auth_is_stale_third_party_residue(&json!({
        "OPENAI_API_KEY": "sk-x",
        "tokens": { "access_token": "t" }
    })));
    assert!(!codex_live_auth_is_stale_third_party_residue(&json!({
        "auth_mode": "chatgpt",
        "OPENAI_API_KEY": null,
        "tokens": { "access_token": "official-oauth-token" }
    })));

    // Nothing to clear.
    assert!(!codex_live_auth_is_stale_third_party_residue(&json!({})));
    assert!(!codex_live_auth_is_stale_third_party_residue(&json!({
        "OPENAI_API_KEY": ""
    })));
}

#[test]
fn dynamic_template_backfills_parser_required_fields_from_static() {
    // Simulate an official row from a Codex build whose ModelInfo lacks parser-side required fields such
    // as `supports_reasoning_summaries` (codex >= 0.144.5 rejects the
    // whole catalog file without it).
    let mut template = json!({
        "slug": "gpt-5.5",
        "context_window": 272_000,
        "supports_parallel_tool_calls": false
    });
    fill_template_fields_from_static(&mut template);

    assert_eq!(
        template
            .get("supports_reasoning_summaries")
            .and_then(Value::as_bool),
        Some(true)
    );
    // Keys already present in the dynamic template are never overwritten.
    assert_eq!(
        template
            .get("supports_parallel_tool_calls")
            .and_then(Value::as_bool),
        Some(false)
    );
    assert_eq!(
        template.get("context_window").and_then(Value::as_u64),
        Some(272_000)
    );
    // Optional capability fields must NOT be backfilled: for the catalog
    // parser "missing" means the parser default, not the static
    // template's value.
    assert!(template.get("supports_search_tool").is_none());
    assert!(template.get("supports_image_detail_original").is_none());
    assert!(template.get("web_search_tool_type").is_none());

    // A cache template missing supports_parallel_tool_calls gets the
    // static gpt-5.5 default backfilled (codex 0.148.0 rejects the
    // catalog without it, #6661).
    let mut stale = json!({ "slug": "gpt-5.5" });
    fill_template_fields_from_static(&mut stale);
    assert_eq!(
        stale
            .get("supports_parallel_tool_calls")
            .and_then(Value::as_bool),
        Some(true)
    );
}

#[test]
fn proxy_chat_catalog_entries_carry_reasoning_summaries_flag() {
    // End to end: a stale dynamic template, once backfilled, must yield
    // catalog entries codex 0.144.5+ can parse.
    let mut template = json!({ "slug": "gpt-5.5" });
    fill_template_fields_from_static(&mut template);
    let specs = vec![CodexCatalogModelSpec {
        model: "k3".to_string(),
        display_name: Some("Kimi K3".to_string()),
        context_window: Some(262_144),
        supports_parallel_tool_calls: None,
        input_modalities: None,
        base_instructions: None,
        reasoning_levels: None,
        default_reasoning_level: None,
    }];
    let catalog = codex_model_catalog_from_specs(
        &specs,
        &template,
        CodexCatalogToolProfile::ProxyChat,
        128_000,
    );
    assert_eq!(
        catalog["models"][0]
            .get("supports_reasoning_summaries")
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        catalog["models"][0]
            .get("supports_parallel_tool_calls")
            .and_then(Value::as_bool),
        Some(true)
    );
}

#[test]
fn provider_catalog_models_do_not_inherit_hidden_template_visibility() {
    let template = json!({ "slug": "gpt-5.5", "visibility": "hide" });
    let settings = json!({
        "modelCatalog": { "models": [
            { "model": "glm-5.3-flash" },
            { "model": "glm-5.3" }
        ] }
    });
    let specs = codex_catalog_model_specs(&settings);
    let catalog = codex_model_catalog_from_specs(
        &specs,
        &template,
        CodexCatalogToolProfile::ProxyChat,
        128_000,
    );
    let models = catalog["models"].as_array().unwrap();
    assert_eq!(models.len(), 2);
    for model in models {
        assert_eq!(model["visibility"], "list");
    }
    assert_eq!(template["visibility"], "hide");
}

#[test]
fn codex_model_catalog_uses_provider_models_and_context() {
    let template = json!({
        "slug": "gpt-5.5",
        "display_name": "GPT-5.5",
        "description": "Frontier model",
        "base_instructions": "gpt-5.5 base instructions",
        "model_messages": {
            "instructions_template": "gpt-5.5 instructions template",
            "instructions_variables": {
                "personality_default": "",
                "personality_friendly": "",
                "personality_pragmatic": ""
            }
        },
        "additional_speed_tiers": ["fast"],
        "service_tiers": [
            {
                "id": "priority",
                "name": "Fast",
                "description": "1.5x speed, increased usage"
            }
        ],
        "availability_nux": {
            "message": "GPT-5.5 is now available."
        },
        "upgrade": {
            "target": "gpt-5.5"
        },
        "context_window": 272000,
        "max_context_window": 272000
    });
    let settings = json!({
        "modelCatalog": {
            "models": [
                {
                    "model": "deepseek-v4-flash",
                    "displayName": "DeepSeek V4 Flash",
                    "contextWindow": "64000"
                },
                {
                    "model": "kimi-k2",
                    "display_name": "Kimi K2"
                }
            ]
        }
    });
    let specs = codex_catalog_model_specs(&settings);
    let catalog = codex_model_catalog_from_specs(
        &specs,
        &template,
        CodexCatalogToolProfile::ProxyChat,
        128_000,
    );
    let models = catalog
        .get("models")
        .and_then(|value| value.as_array())
        .expect("models should be an array");

    assert_eq!(models.len(), 2);
    assert_eq!(
        models[0].get("slug").and_then(|value| value.as_str()),
        Some("deepseek-v4-flash")
    );
    assert_eq!(
        models[0]
            .get("context_window")
            .and_then(|value| value.as_u64()),
        Some(64_000)
    );
    assert_eq!(
        models[1]
            .get("context_window")
            .and_then(|value| value.as_u64()),
        Some(128_000)
    );
    assert!(
        models[0].get("model_messages").is_some(),
        "Codex requires model_messages in custom catalogs"
    );
    assert_eq!(
        models[0]
            .get("base_instructions")
            .and_then(|value| value.as_str()),
        Some("gpt-5.5 base instructions")
    );
    assert_eq!(
        models[0].get("model_messages"),
        template.get("model_messages"),
        "custom catalog entries should keep the gpt-5.5 agent template"
    );
    assert_eq!(
        models[0].get("additional_speed_tiers"),
        Some(&json!([])),
        "generated third-party entries should not inherit OpenAI speed tiers"
    );
    assert!(
        models[0]
            .get("availability_nux")
            .is_some_and(|value| value.is_null()),
        "generated third-party entries should not inherit GPT-5.5 launch messaging"
    );
}

#[test]
fn native_responses_catalog_honors_per_model_reasoning_levels() {
    // The native template only declares none/high. A per-model
    // reasoningLevels override must replace supported_reasoning_levels and
    // pick a sensible default_reasoning_level.
    let settings = json!({
        "modelCatalog": {
            "models": [
                {
                    "model": "deepseek-v4-flash",
                    "reasoningLevels": ["none", "low", "medium", "high", "xhigh", "max"],
                    "defaultReasoningLevel": "xhigh"
                },
                {
                    "model": "no-default-model",
                    "reasoningLevels": ["low", "medium", "high"]
                },
                {
                    "model": "template-default-model",
                    "reasoningLevels": ["none", "high", "xhigh"]
                },
                {
                    "model": "dirty-levels",
                    "reasoningLevels": ["none", "bogus", "high", ""]
                },
                {
                    "model": "unordered-model",
                    "reasoningLevels": ["xhigh", "low", "bogus", "low"],
                    "defaultReasoningLevel": "bogus"
                },
                {
                    "model": "single-none-model",
                    "reasoningLevels": ["none"]
                },
                {
                    "model": "absent-levels-model"
                }
            ]
        }
    });

    let catalog =
        codex_model_catalog_from_settings(&settings, "", CodexCatalogToolProfile::NativeResponses)
            .expect("catalog generation should not error")
            .expect("non-empty modelCatalog must yield a catalog");

    let models = catalog["models"].as_array().expect("models array");
    let efforts = |index: usize| -> Vec<String> {
        models[index]["supported_reasoning_levels"]
            .as_array()
            .expect("supported_reasoning_levels array")
            .iter()
            .filter_map(|level| level.get("effort").and_then(|v| v.as_str()))
            .map(str::to_string)
            .collect()
    };

    // Explicit default wins.
    assert_eq!(
        efforts(0),
        vec!["none", "low", "medium", "high", "xhigh", "max"]
    );
    assert_eq!(
        models[0]
            .get("default_reasoning_level")
            .and_then(|v| v.as_str()),
        Some("xhigh")
    );

    // No explicit default: falls back to the last (highest) declared level.
    assert_eq!(efforts(1), vec!["low", "medium", "high"]);
    assert_eq!(
        models[1]
            .get("default_reasoning_level")
            .and_then(|v| v.as_str()),
        Some("high")
    );

    // Template default ("high") is kept when it is still in the list.
    assert_eq!(efforts(2), vec!["none", "high", "xhigh"]);
    assert_eq!(
        models[2]
            .get("default_reasoning_level")
            .and_then(|v| v.as_str()),
        Some("high")
    );

    // Unknown / empty efforts are dropped; the default still resolves to
    // a supported level (the template default, "high").
    assert_eq!(efforts(3), vec!["none", "high"]);
    assert_eq!(
        models[3]
            .get("default_reasoning_level")
            .and_then(|v| v.as_str()),
        Some("high")
    );

    // Declaration order is normalized to canonical order, duplicates and
    // an unknown explicit default are dropped, and the fallback picks the
    // highest supported level in canonical order (not the last declared
    // one, and never an unknown effort).
    assert_eq!(efforts(4), vec!["low", "xhigh"]);
    assert_eq!(
        models[4]
            .get("default_reasoning_level")
            .and_then(|v| v.as_str()),
        Some("xhigh")
    );

    // A single declared level collapses the picker: Codex applies the only
    // level directly instead of offering a choice. The template default
    // ("high") is no longer in the list, so the default falls back to the
    // highest supported level, i.e. "none". This is how a model that
    // ignores reasoning effort must be declared — leaving
    // `reasoningLevels` out inherits the template's none/high pair and
    // re-exposes the very picker we want to avoid.
    assert_eq!(efforts(5), vec!["none"]);
    assert_eq!(
        models[5]
            .get("default_reasoning_level")
            .and_then(|v| v.as_str()),
        Some("none")
    );

    // Guards the inheritance the comment above depends on: with no
    // declaration at all the entry keeps the native template's none/high
    // pair and its "high" default.
    assert_eq!(efforts(6), vec!["none", "high"]);
    assert_eq!(
        models[6]
            .get("default_reasoning_level")
            .and_then(|v| v.as_str()),
        Some("high")
    );
}

#[test]
fn vendor_catalog_honors_per_model_reasoning_levels() {
    // The DeepSeek official catalog declares low/high/max; a per-model
    // override must win over the official entry.
    let settings = json!({
        "modelCatalog": {
            "models": [
                {
                    "model": "deepseek-v4-flash",
                    "reasoningLevels": ["none", "low", "medium", "high", "xhigh", "max"],
                    "defaultReasoningLevel": "xhigh"
                }
            ]
        }
    });

    let catalog = codex_model_catalog_from_settings(
        &settings,
        DEEPSEEK_NATIVE_CONFIG,
        CodexCatalogToolProfile::NativeResponses,
    )
    .expect("vendor catalog generation should not error")
    .expect("non-empty modelCatalog must yield a catalog");

    let entry = &catalog["models"][0];
    let efforts: Vec<&str> = entry["supported_reasoning_levels"]
        .as_array()
        .expect("supported_reasoning_levels array")
        .iter()
        .filter_map(|level| level.get("effort").and_then(|v| v.as_str()))
        .collect();
    assert_eq!(
        efforts,
        vec!["none", "low", "medium", "high", "xhigh", "max"]
    );
    assert_eq!(
        entry
            .get("default_reasoning_level")
            .and_then(|v| v.as_str()),
        Some("xhigh")
    );
}

#[test]
fn vendor_catalog_unknown_model_does_not_inherit_flagship_modalities() {
    // A vision variant not in the official DeepSeek catalog must not
    // inherit the flagship entry's text-only modalities; the registry /
    // fail-open logic should resolve it as image-capable instead.
    let settings = json!({
        "modelCatalog": {
            "models": [
                {
                    "model": "deepseek-v4-flash-vision-exp",
                    "displayName": "DeepSeek V4 Flash Vision Exp"
                }
            ]
        }
    });

    let catalog = codex_model_catalog_from_settings(
        &settings,
        DEEPSEEK_NATIVE_CONFIG,
        CodexCatalogToolProfile::NativeResponses,
    )
    .expect("vendor catalog generation should not error")
    .expect("non-empty modelCatalog must yield a catalog");

    let modalities: Vec<&str> = catalog["models"][0]["input_modalities"]
        .as_array()
        .expect("input_modalities array")
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert_eq!(
        modalities,
        vec!["text", "image"],
        "unknown vision model must not inherit the flagship's text-only modalities"
    );
}

#[test]
fn vendor_catalog_unknown_model_explicit_modalities_override() {
    // An explicit user inputModalities declaration must win over the
    // registry/fail-open resolution even for unmatched models.
    let settings = json!({
        "modelCatalog": {
            "models": [
                {
                    "model": "deepseek-v4-flash-vision-exp",
                    "inputModalities": ["text"]
                }
            ]
        }
    });

    let catalog = codex_model_catalog_from_settings(
        &settings,
        DEEPSEEK_NATIVE_CONFIG,
        CodexCatalogToolProfile::NativeResponses,
    )
    .expect("vendor catalog generation should not error")
    .expect("non-empty modelCatalog must yield a catalog");

    let modalities: Vec<&str> = catalog["models"][0]["input_modalities"]
        .as_array()
        .expect("input_modalities array")
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert_eq!(modalities, vec!["text"]);
}

#[test]
fn vendor_catalog_matched_model_keeps_vendor_modalities() {
    // A model that IS in the official catalog must keep the vendor's
    // declared modalities verbatim (deepseek-v4-pro is text-only there).
    let settings = json!({
        "modelCatalog": {
            "models": [
                {
                    "model": "deepseek-v4-pro",
                    "displayName": "DeepSeek V4 Pro"
                }
            ]
        }
    });

    let catalog = codex_model_catalog_from_settings(
        &settings,
        DEEPSEEK_NATIVE_CONFIG,
        CodexCatalogToolProfile::NativeResponses,
    )
    .expect("vendor catalog generation should not error")
    .expect("non-empty modelCatalog must yield a catalog");

    let modalities: Vec<&str> = catalog["models"][0]["input_modalities"]
        .as_array()
        .expect("input_modalities array")
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert_eq!(modalities, vec!["text"]);
}

#[test]
fn native_responses_profile_suppresses_apply_patch_and_keeps_shell() {
    // Native (direct) /responses providers must NOT emit a freeform
    // apply_patch (type=="custom") tool — gateways like MiMo reject it.
    // The native profile uses the bundled clean template and relies on
    // shell_type="shell_command" for edits, plus per-row overrides.
    let settings = json!({
        "modelCatalog": {
            "models": [
                {
                    "model": "MiniMax-M3",
                    "displayName": "MiniMax-M3",
                    "contextWindow": 1_000_000,
                    "supportsParallelToolCalls": true,
                    "inputModalities": ["text", "image"],
                    "baseInstructions": "You are Codex, a coding agent based on MiniMax-M3."
                }
            ]
        }
    });

    let catalog =
        codex_model_catalog_from_settings(&settings, "", CodexCatalogToolProfile::NativeResponses)
            .expect("native catalog generation should not error")
            .expect("non-empty modelCatalog must yield a catalog");

    let entry = &catalog["models"][0];
    assert_eq!(
        entry.get("slug").and_then(|v| v.as_str()),
        Some("MiniMax-M3")
    );
    assert_eq!(
        entry.get("shell_type").and_then(|v| v.as_str()),
        Some("shell_command"),
        "native entries edit via shell, not the custom apply_patch tool"
    );
    assert!(
        entry.get("apply_patch_tool_type").is_none(),
        "native entries must NOT declare a freeform apply_patch tool"
    );
    // `base_instructions` is REQUIRED by Codex's catalog parser, so it must
    // be present — and the per-row official override must win over the
    // template default.
    assert_eq!(
        entry.get("base_instructions").and_then(|v| v.as_str()),
        Some("You are Codex, a coding agent based on MiniMax-M3."),
        "per-row baseInstructions override must apply (and field must exist)"
    );
    assert!(
        entry.get("model_messages").is_none(),
        "native entries must not carry the gpt-5.5 model_messages persona text"
    );
    assert_eq!(
        entry.get("supports_parallel_tool_calls"),
        Some(&json!(true)),
        "per-row supportsParallelToolCalls override must apply"
    );
    assert_eq!(
        entry.get("input_modalities"),
        Some(&json!(["text", "image"])),
        "per-row inputModalities override must apply"
    );
    assert_eq!(
        entry.get("context_window").and_then(|v| v.as_u64()),
        Some(1_000_000)
    );
}

#[test]
fn catalog_infers_image_input_independently_of_tool_profile() {
    // Start from a deliberately text-only template to prove that every
    // profile overwrites template defaults with shared capability logic.
    let template = json!({
        "input_modalities": ["text"],
        "apply_patch_tool_type": "freeform"
    });
    let specs = vec![
        CodexCatalogModelSpec {
            model: "gpt-5.4".to_string(),
            display_name: Some("GPT 5.4".to_string()),
            context_window: Some(128_000),
            supports_parallel_tool_calls: None,
            input_modalities: None,
            base_instructions: None,
            reasoning_levels: None,
            default_reasoning_level: None,
        },
        CodexCatalogModelSpec {
            model: "qwen/qwen3-coder-plus".to_string(),
            display_name: Some("Qwen3 Coder Plus".to_string()),
            context_window: Some(128_000),
            supports_parallel_tool_calls: None,
            input_modalities: None,
            base_instructions: None,
            reasoning_levels: None,
            default_reasoning_level: None,
        },
        CodexCatalogModelSpec {
            model: "deepseek/deepseek-v4-flash-0731".to_string(),
            display_name: Some("DeepSeek V4 Flash 0731".to_string()),
            context_window: Some(128_000),
            supports_parallel_tool_calls: None,
            input_modalities: None,
            base_instructions: None,
            reasoning_levels: None,
            default_reasoning_level: None,
        },
        CodexCatalogModelSpec {
            model: "glm-5.2v".to_string(),
            display_name: Some("GLM 5.2V".to_string()),
            context_window: Some(128_000),
            supports_parallel_tool_calls: None,
            input_modalities: None,
            base_instructions: None,
            reasoning_levels: None,
            default_reasoning_level: None,
        },
        CodexCatalogModelSpec {
            model: "deepseek-v4-flash".to_string(),
            display_name: Some("Explicit Visual Override".to_string()),
            context_window: Some(128_000),
            supports_parallel_tool_calls: None,
            input_modalities: Some(vec!["text".to_string(), "image".to_string()]),
            base_instructions: None,
            reasoning_levels: None,
            default_reasoning_level: None,
        },
        CodexCatalogModelSpec {
            model: "custom-text-alias".to_string(),
            display_name: Some("Explicit Text Override".to_string()),
            context_window: Some(128_000),
            supports_parallel_tool_calls: None,
            input_modalities: Some(vec!["text".to_string()]),
            base_instructions: None,
            reasoning_levels: None,
            default_reasoning_level: None,
        },
    ];

    for profile in [
        CodexCatalogToolProfile::ProxyChat,
        CodexCatalogToolProfile::Copilot,
        CodexCatalogToolProfile::NativeResponses,
        CodexCatalogToolProfile::Anthropic,
    ] {
        let catalog = codex_model_catalog_from_specs(&specs, &template, profile, 128_000);
        let models = catalog["models"].as_array().expect("models array");
        let modalities = |slug: &str| {
            models
                .iter()
                .find(|entry| entry["slug"] == slug)
                .and_then(|entry| entry.get("input_modalities"))
                .cloned()
                .unwrap_or(Value::Null)
        };

        assert_eq!(modalities("gpt-5.4"), json!(["text", "image"]));
        assert_eq!(modalities("qwen/qwen3-coder-plus"), json!(["text"]));
        assert_eq!(
            modalities("deepseek/deepseek-v4-flash-0731"),
            json!(["text"]),
            "OpenRouter's dated DeepSeek Flash alias is text-only"
        );
        assert_eq!(modalities("glm-5.2v"), json!(["text", "image"]));
        assert_eq!(
            modalities("deepseek-v4-flash"),
            json!(["text", "image"]),
            "explicit provider metadata must override the text-only registry"
        );
        assert_eq!(modalities("custom-text-alias"), json!(["text"]));
    }
}

#[test]
fn copilot_catalog_uses_declared_vision_capabilities_and_infers_unknowns() {
    let settings = json!({
        "modelCatalog": {
            "models": [
                { "model": "gpt-5.6-sol", "inputModalities": ["text", "image"] },
                { "model": "gpt-5.6-terra", "inputModalities": ["text"] },
                { "model": "copilot-unknown" },
                { "model": "qwen3-coder-plus", "inputModalities": ["text", "image"] }
            ]
        }
    });
    let template = json!({
        "input_modalities": ["text"],
        "supports_image_detail_original": true
    });
    let specs = codex_catalog_model_specs(&settings);
    let catalog = codex_model_catalog_from_specs(
        &specs,
        &template,
        CodexCatalogToolProfile::Copilot,
        128_000,
    );
    let models = catalog["models"].as_array().unwrap();
    assert_eq!(models.len(), 4);
    for (model, expected) in [
        ("gpt-5.6-sol", json!(["text", "image"])),
        ("gpt-5.6-terra", json!(["text"])),
        ("copilot-unknown", json!(["text", "image"])),
        ("qwen3-coder-plus", json!(["text", "image"])),
    ] {
        let entry = models.iter().find(|entry| entry["slug"] == model).unwrap();
        assert_eq!(entry["input_modalities"], expected, "{model}");
        assert_eq!(entry["supports_image_detail_original"], json!(false));
    }
}

#[test]
fn native_responses_catalog_always_carries_base_instructions() {
    // Regression guard for the "missing field `base_instructions`" parse
    // error: Codex refuses to load a model catalog whose entries lack
    // base_instructions. Synthesized presets carry no per-row override, so
    // the entry MUST inherit the template's neutral default rather than
    // dropping the field entirely.
    let settings = json!({
        "modelCatalog": { "models": [{ "model": "qwen3-coder-plus" }] }
    });

    let catalog =
        codex_model_catalog_from_settings(&settings, "", CodexCatalogToolProfile::NativeResponses)
            .expect("native catalog generation should not error")
            .expect("non-empty modelCatalog must yield a catalog");

    let base = catalog["models"][0]
        .get("base_instructions")
        .and_then(|v| v.as_str());
    assert!(
        base.is_some_and(|s| !s.trim().is_empty()),
        "every native entry must carry a non-empty base_instructions (Codex requires it)"
    );
}

const DEEPSEEK_NATIVE_CONFIG: &str = r#"model = "deepseek-v4-flash"
model_provider = "custom"

[model_providers.custom]
name = "deepseek"
base_url = "https://api.deepseek.com"
wire_api = "responses"
"#;

#[test]
fn deepseek_host_native_catalog_mirrors_official_entries() {
    // DeepSeek publishes an official Codex models.json (freeform
    // apply_patch + GPT-5 harness + low/high/max reasoning levels). For a
    // deepseek.com native provider the generated catalog must mirror it
    // verbatim instead of the stripped neutral template — the harness
    // tells the model to use apply_patch, so stripping the tool while
    // keeping the harness would be self-inconsistent.
    let settings = json!({
        "modelCatalog": {
            "models": [
                { "model": "deepseek-flash", "displayName": "DeepSeek Flash" },
                { "model": "deepseek-v4-pro", "contextWindow": 500_000 }
            ]
        }
    });

    let catalog = codex_model_catalog_from_settings(
        &settings,
        DEEPSEEK_NATIVE_CONFIG,
        CodexCatalogToolProfile::NativeResponses,
    )
    .expect("vendor catalog generation should not error")
    .expect("non-empty modelCatalog must yield a catalog");

    let flash = &catalog["models"][0];
    assert_eq!(
        flash.get("slug").and_then(|v| v.as_str()),
        Some("deepseek-flash")
    );
    assert_eq!(
        flash.get("apply_patch_tool_type").and_then(|v| v.as_str()),
        Some("freeform"),
        "official DeepSeek entries keep the freeform apply_patch grant"
    );
    assert!(
        flash
            .get("base_instructions")
            .and_then(|v| v.as_str())
            .is_some_and(|s| s.starts_with("You are Codex, an agent based on GPT-5")),
        "official GPT-5 harness must survive verbatim"
    );
    let efforts: Vec<&str> = flash["supported_reasoning_levels"]
        .as_array()
        .expect("official reasoning levels array")
        .iter()
        .filter_map(|level| level.get("effort").and_then(|v| v.as_str()))
        .collect();
    assert_eq!(efforts, vec!["low", "high", "max"]);
    // DeepSeek has no tool_search support; `true` makes Codex defer MCP
    // tools behind tool search, so none can ever be called (#6647).
    assert_eq!(flash.get("supports_search_tool"), Some(&json!(false)));
    assert_eq!(
        flash.get("web_search_tool_type").and_then(|v| v.as_str()),
        Some("text")
    );
    assert_eq!(
        flash.get("supports_reasoning_summaries"),
        Some(&json!(true))
    );
    // deepseek-flash accepts image input per the vendor's own catalog and
    // vision guide (api-docs.deepseek.com/guides/vision); the legacy
    // deepseek-v4-flash alias routes to it and must not be gated (#7283).
    assert_eq!(
        flash.get("input_modalities"),
        Some(&json!(["text", "image"]))
    );
    assert!(
        flash.get("model_messages").is_some(),
        "official entries are mirrored verbatim, incl. model_messages"
    );
    // No explicit contextWindow on the row: the official 1m window must
    // survive instead of being clobbered by the 128k default.
    assert_eq!(
        flash.get("context_window").and_then(|v| v.as_u64()),
        Some(1_048_576)
    );
    // Explicit user display name still wins over the official one.
    assert_eq!(
        flash.get("display_name").and_then(|v| v.as_str()),
        Some("DeepSeek Flash")
    );

    let pro = &catalog["models"][1];
    assert_eq!(
        pro.get("slug").and_then(|v| v.as_str()),
        Some("deepseek-v4-pro")
    );
    // Explicit user context window override wins…
    assert_eq!(
        pro.get("context_window").and_then(|v| v.as_u64()),
        Some(500_000)
    );
    assert_eq!(
        pro.get("max_context_window").and_then(|v| v.as_u64()),
        Some(500_000)
    );
    // …while the untouched official display name is kept.
    assert_eq!(
        pro.get("display_name").and_then(|v| v.as_str()),
        Some("DeepSeek-V4-Pro")
    );
}

#[test]
fn deepseek_official_catalog_unknown_model_clones_flagship() {
    // A user-added model id the official file doesn't know keeps the
    // gateway's capability profile (clone of the flagship entry) without
    // impersonating it: own slug/name, demoted priority, and the official
    // context window rather than the 128k synthetic default.
    let settings = json!({
        "modelCatalog": { "models": [{ "model": "deepseek-v4-lite" }] }
    });

    let catalog = codex_model_catalog_from_settings(
        &settings,
        DEEPSEEK_NATIVE_CONFIG,
        CodexCatalogToolProfile::NativeResponses,
    )
    .expect("vendor catalog generation should not error")
    .expect("non-empty modelCatalog must yield a catalog");

    let entry = &catalog["models"][0];
    assert_eq!(
        entry.get("slug").and_then(|v| v.as_str()),
        Some("deepseek-v4-lite")
    );
    assert_eq!(
        entry.get("display_name").and_then(|v| v.as_str()),
        Some("deepseek-v4-lite")
    );
    assert!(
        entry
            .get("priority")
            .and_then(|v| v.as_u64())
            .is_some_and(|p| p >= 1000),
        "clones must sort after official entries"
    );
    assert_eq!(
        entry.get("apply_patch_tool_type").and_then(|v| v.as_str()),
        Some("freeform")
    );
    assert_eq!(
        entry.get("context_window").and_then(|v| v.as_u64()),
        Some(1_048_576),
        "absent contextWindow keeps the flagship's official window"
    );
    assert!(entry
        .get("base_instructions")
        .and_then(|v| v.as_str())
        .is_some_and(|s| !s.trim().is_empty()));
}

#[test]
fn deepseek_official_catalog_legacy_flash_alias_stays_image_capable() {
    // The vendor's catalog now ships `deepseek-flash` only; the legacy
    // `deepseek-v4-flash` id the preset defaulted to is still accepted by
    // the API and routes to the same vision-capable Flash model, so it must
    // clone the flagship and resolve image-capable instead of being gated
    // text-only (#7283).
    let settings = json!({
        "modelCatalog": { "models": [{ "model": "deepseek-v4-flash" }] }
    });

    let catalog = codex_model_catalog_from_settings(
        &settings,
        DEEPSEEK_NATIVE_CONFIG,
        CodexCatalogToolProfile::NativeResponses,
    )
    .expect("vendor catalog generation should not error")
    .expect("non-empty modelCatalog must yield a catalog");

    let entry = &catalog["models"][0];
    assert_eq!(
        entry.get("slug").and_then(|v| v.as_str()),
        Some("deepseek-v4-flash")
    );
    assert_eq!(
        entry.get("input_modalities"),
        Some(&json!(["text", "image"])),
        "the legacy alias routes to the vision-capable Flash model and must fail open"
    );
}

#[test]
fn official_vendor_catalog_gated_by_native_profile_and_host() {
    // The official mirror is a capability GRANT, so the gate must be
    // narrow: native `/responses` profile AND the vendor's own host. Chat
    // runs through the proxy converter (gpt-5.5 contract), the Anthropic
    // transform drops custom tools, and aggregators hosting the same
    // model may reject freeform tools — all of them keep their templates.
    assert!(codex_official_vendor_catalog_models(
        DEEPSEEK_NATIVE_CONFIG,
        CodexCatalogToolProfile::NativeResponses
    )
    .is_some_and(|models| !models.is_empty()));

    for profile in [
        CodexCatalogToolProfile::ProxyChat,
        CodexCatalogToolProfile::Copilot,
        CodexCatalogToolProfile::Anthropic,
    ] {
        assert!(
            codex_official_vendor_catalog_models(DEEPSEEK_NATIVE_CONFIG, profile).is_none(),
            "only the NativeResponses profile may mirror the official catalog"
        );
    }

    let minimax_config = r#"model = "MiniMax-M3"
model_provider = "custom"

[model_providers.custom]
name = "minimax"
base_url = "https://api.minimaxi.com/v1"
wire_api = "responses"
"#;
    assert!(
        codex_official_vendor_catalog_models(
            minimax_config,
            CodexCatalogToolProfile::NativeResponses
        )
        .is_none(),
        "non-DeepSeek native hosts keep the neutral template"
    );
    assert!(
        codex_official_vendor_catalog_models("", CodexCatalogToolProfile::NativeResponses)
            .is_none()
    );
}

#[test]
fn proxy_catalog_profiles_keep_apply_patch_and_disable_original_image_detail() {
    let template = load_codex_native_responses_template();
    let specs = vec![CodexCatalogModelSpec {
        model: "x".to_string(),
        display_name: Some("x".to_string()),
        context_window: Some(128_000),
        supports_parallel_tool_calls: None,
        input_modalities: None,
        base_instructions: None,
        reasoning_levels: None,
        default_reasoning_level: None,
    }];
    let mut proxy_template = template.clone();
    proxy_template["apply_patch_tool_type"] = json!("freeform");
    proxy_template["supports_image_detail_original"] = json!(true);
    for profile in [
        CodexCatalogToolProfile::ProxyChat,
        CodexCatalogToolProfile::Copilot,
    ] {
        let catalog = codex_model_catalog_from_specs(&specs, &proxy_template, profile, 128_000);
        assert_eq!(
            catalog["models"][0]["apply_patch_tool_type"],
            json!("freeform")
        );
        assert_eq!(
            catalog["models"][0]["supports_image_detail_original"],
            json!(false)
        );
    }
}

#[test]
fn catalog_web_search_policy_with_and_without_explicit_models() {
    let config = "model = \"MiniMax-M3\"\n";
    for (settings, has_explicit_catalog) in [
        (json!({}), false),
        (
            json!({"modelCatalog": {"models": [{"model": "MiniMax-M3"}]}}),
            true,
        ),
    ] {
        for (profile, disabled) in [
            (CodexCatalogToolProfile::Copilot, true),
            (CodexCatalogToolProfile::Anthropic, true),
            (CodexCatalogToolProfile::ProxyChat, false),
            (
                CodexCatalogToolProfile::NativeResponses,
                has_explicit_catalog,
            ),
        ] {
            assert_eq!(
                codex_disables_web_search(&settings, config, profile),
                disabled,
                "{profile:?}, explicit catalog={has_explicit_catalog}"
            );
            let catalog = plan_codex_model_catalog(&settings, config, profile)
                .unwrap()
                .catalog;
            if has_explicit_catalog || matches!(profile, CodexCatalogToolProfile::Copilot) {
                let catalog = catalog.expect("catalog for the configured model");
                let models = catalog["models"].as_array().unwrap();
                assert_eq!(models.len(), 1, "{profile:?}");
                assert_eq!(models[0]["slug"], "MiniMax-M3", "{profile:?}");
            } else {
                assert!(catalog.is_none(), "{profile:?}");
            }
        }
    }
}

#[test]
fn copilot_catalog_preserves_context_and_explicit_parallel_capabilities() {
    let settings = json!({
        "modelCatalog": {
            "models": [
                { "model": "claude-sonnet-5", "contextWindow": 400000 },
                { "model": "parallel-model", "supportsParallelToolCalls": true },
                { "model": "serial-model", "supports_parallel_tool_calls": false }
            ]
        }
    });
    let template = load_codex_classic_tool_template();
    assert_eq!(template["supports_parallel_tool_calls"], json!(true));
    let catalog = codex_model_catalog_from_specs(
        &codex_catalog_model_specs(&settings),
        &template,
        CodexCatalogToolProfile::Copilot,
        128_000,
    );
    let models = catalog["models"].as_array().unwrap();
    assert_eq!(models.len(), 3);
    assert_eq!(models[0]["context_window"], json!(400000));
    assert_eq!(models[0]["max_context_window"], json!(400000));
    for (entry, parallel) in models.iter().zip([false, true, false]) {
        assert_eq!(entry["supports_parallel_tool_calls"], json!(parallel));
        assert!(entry["base_instructions"].is_string());
        for key in CODEX_CATALOG_PARSER_REQUIRED_FIELDS {
            assert!(entry.get(*key).is_some(), "{key} is required by Codex");
        }
    }
}

#[test]
fn web_search_blacklist_disables_only_known_reject_gateways() {
    let cfg = |model: &str, base_url: &str| {
        format!(
            "model_provider = \"custom\"\nmodel = \"{model}\"\n\n[model_providers.custom]\nname = \"x\"\nbase_url = \"{base_url}\"\nwire_api = \"responses\"\n"
        )
    };

    // Blacklisted by host (first-party reject gateways) → disable.
    for (model, host) in [
        ("mimo-v2.5-pro", "https://api.xiaomimimo.com/v1"),
        ("mimo-v2.5", "https://token-plan-cn.xiaomimimo.com/v1"),
        ("LongCat-2.0", "https://api.longcat.chat/openai/v1"),
        ("MiniMax-M3", "https://api.minimax.io/v1"),
        ("MiniMax-M3", "https://api.minimaxi.com/v1"),
        // Use an alias to exercise host detection independently of the
        // MiniMax model-prefix fallback.
        ("custom-model", "https://api.minimax.cn/v1"),
        ("step-4-flash", "https://api.stepfun.com/v1"),
        ("step-4-flash", "https://api.stepfun.ai/v1"),
        ("deepseek-v4-pro", "https://qianfan.baidubce.com/v2"),
        (
            "astron-code-latest",
            "https://maas-coding-api.cn-huabei-1.xf-yun.com/v1",
        ),
        ("glm-5.3", "https://open.bigmodel.cn/api/v1"),
        ("glm-5.3", "https://api.z.ai/api/v1"),
    ] {
        assert!(
            codex_native_gateway_rejects_web_search(&cfg(model, host)),
            "{host} should be blacklisted"
        );
    }

    // Blacklisted by MODEL brand even on an aggregator host (SiliconFlow
    // fronting a reject vendor's model) → disable.
    for (model, host) in [
        ("MiniMax-M3", "https://api.siliconflow.cn/v1"),
        ("MiniMaxAI/MiniMax-M3", "https://api.siliconflow.cn/v1"),
        ("mimo-v2.5-pro", "https://some-aggregator.example/v1"),
        ("zai-org/glm-5.3", "https://some-aggregator.example/v1"),
        (
            "qwen/qwen3-coder-plus",
            "https://some-aggregator.example/v1",
        ),
    ] {
        assert!(
            codex_native_gateway_rejects_web_search(&cfg(model, host)),
            "{model} @ {host} should be blacklisted by model brand"
        );
    }

    // Qwen3-Coder is blacklisted by model, not by DashScope host. This keeps
    // general Qwen models that support built-in web_search on the same host
    // enabled while protecting the native qwen3-coder-plus preset.
    assert!(codex_native_gateway_rejects_web_search(&cfg(
        "qwen3-coder-plus",
        "https://dashscope.aliyuncs.com/compatible-mode/v1",
    )));
    assert!(!codex_native_gateway_rejects_web_search(&cfg(
        "qwen3.7-plus",
        "https://dashscope.aliyuncs.com/compatible-mode/v1",
    )));

    // NOT blacklisted → keep Codex default (relays/GPT, DouBao, general Qwen,
    // and any unknown provider incl. an aggregator serving a non-reject model).
    for (model, host) in [
        ("gpt-5.5", "https://www.packyapi.com/v1"),
        ("gpt-5-codex", "https://aihubmix.com/v1"),
        (
            "doubao-seed-2-1-pro-260628",
            "https://ark.cn-beijing.volces.com/api/v3",
        ),
        ("Pro/moonshotai/Kimi-K2.6", "https://api.siliconflow.cn/v1"),
        // Host-label matching: `z.ai` / `bigmodel.cn` must not swallow
        // unrelated domains that merely contain them as a substring.
        ("gpt-5.5", "https://api.xyz.ai/v1"),
        ("gpt-5.5", "https://viz.ai/v1"),
        ("gpt-5.5", "https://notbigmodel.cn/v1"),
        ("gpt-5.5", "https://z.ai.example.com/v1"),
        ("gpt-5.5", "https://api.stepfun.com.example.com/v1"),
    ] {
        assert!(
            !codex_native_gateway_rejects_web_search(&cfg(model, host)),
            "{model} @ {host} should NOT be blacklisted"
        );
    }
}

#[test]
fn url_host_matcher_uses_label_boundaries() {
    let hosts = &["z.ai", "bigmodel.cn"];
    for url in [
        "https://api.z.ai/api/v1",
        "https://open.bigmodel.cn/api/v1",
        "https://Open.BigModel.cn/api/coding/paas/v4",
        "https://user:pw@api.z.ai:8443/api/v1?x=1#f",
        "z.ai",
        "api.z.ai.",
    ] {
        assert!(codex_url_host_matches_any(url, hosts), "{url}");
    }
    for url in [
        "https://api.xyz.ai/v1",
        "https://viz.ai/v1",
        "https://z.ai.example.com/v1",
        "https://notbigmodel.cn/v1",
        "https://example.com/z.ai/v1",
        "https://example.com/?next=https://api.z.ai",
        "",
    ] {
        assert!(!codex_url_host_matches_any(url, hosts), "{url}");
    }
    assert_eq!(codex_url_host("https://[::1]:8080/v1"), "::1");
    assert_eq!(codex_url_host("HTTP://Example.COM:80"), "example.com");
}

#[test]
fn resolve_catalog_path_returns_none_when_config_missing_field() {
    let base = PathBuf::from("/tmp/.codex");
    assert!(resolve_cc_switch_catalog_path("", &base).is_none());
    assert!(
        resolve_cc_switch_catalog_path("model = \"gpt-5\"", &base).is_none(),
        "no model_catalog_json field should yield None"
    );
}

#[test]
fn resolve_catalog_path_accepts_cc_switch_owned_file() {
    let base = PathBuf::from("/tmp/.codex");
    let config = r#"model_catalog_json = "/tmp/.codex/cc-switch-model-catalog.json"
"#;
    let resolved = resolve_cc_switch_catalog_path(config, &base).expect("path resolves");
    assert_eq!(resolved, base.join(CC_SWITCH_CODEX_MODEL_CATALOG_FILENAME));
}

#[test]
fn resolve_catalog_path_rejects_user_owned_external_file() {
    let base = PathBuf::from("/tmp/.codex");
    let config = r#"model_catalog_json = "/Users/me/.codex/my-handwritten-catalog.json"
"#;
    assert!(
        resolve_cc_switch_catalog_path(config, &base).is_none(),
        "external catalog files should be left alone"
    );
}

#[test]
fn build_simplified_catalog_round_trips_user_input() {
    let config = "";
    let catalog = r#"{
            "models": [
                { "slug": "deepseek-v4-pro", "display_name": "deepseek-v4-pro", "context_window": 1000000 },
                { "slug": "deepseek-v4-flash", "display_name": "DeepSeek Flash", "context_window": 1000000 }
            ]
        }"#;
    let result = build_simplified_catalog_from_texts(config, catalog).expect("entries found");
    let models = result
        .get("models")
        .and_then(|m| m.as_array())
        .expect("models array");
    assert_eq!(models.len(), 2);

    // First entry: display_name == slug → displayName squashed; explicit
    // context_window != default 128_000 → preserved.
    assert_eq!(
        models[0].get("model").and_then(|v| v.as_str()),
        Some("deepseek-v4-pro")
    );
    assert!(models[0].get("displayName").is_none());
    assert_eq!(
        models[0].get("contextWindow").and_then(|v| v.as_u64()),
        Some(1_000_000)
    );

    // Second entry: display_name distinct from slug → preserved.
    assert_eq!(
        models[1].get("displayName").and_then(|v| v.as_str()),
        Some("DeepSeek Flash")
    );
}

fn auto_review_overrides(catalog: &Value) -> Vec<Option<&str>> {
    catalog["models"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| {
            row.get("auto_review_model_override")
                .and_then(Value::as_str)
        })
        .collect()
}

/// 卡上选的自动审批模型写进这张卡目录的每一行（主模型可以是任何一行）；不在模型列表里
/// 就不写，用 Codex 的默认。
#[test]
fn card_auto_review_model_is_written_to_every_catalog_row() {
    let config = "model = \"deepseek-v4-pro\"\n";
    let settings = |auto_review: &str| {
        json!({
            "modelCatalog": {
                "models": [
                    { "model": "deepseek-v4-pro" },
                    { "model": "deepseek-v4-flash" }
                ],
                "autoReviewModel": auto_review
            }
        })
    };
    let profile = CodexCatalogToolProfile::NativeResponses;

    let catalog =
        codex_model_catalog_from_settings(&settings(" deepseek-v4-flash "), config, profile)
            .unwrap()
            .expect("catalog");
    assert_eq!(
        auto_review_overrides(&catalog),
        vec![Some("deepseek-v4-flash"); 2]
    );

    for not_written in ["", "codex-auto-review"] {
        let catalog = codex_model_catalog_from_settings(&settings(not_written), config, profile)
            .unwrap()
            .expect("catalog");
        assert_eq!(
            auto_review_overrides(&catalog),
            vec![None; 2],
            "{not_written:?}"
        );
    }
}

/// 读回 live 目录时还原卡上选的审批模型；聚合设置写的 `ccs-*`、各行不一致的都不还原。
#[test]
fn simplified_catalog_restores_the_card_auto_review_model() {
    let catalog = |overrides: [&str; 2]| {
        json!({ "models": [
            { "slug": "deepseek-v4-pro", "auto_review_model_override": overrides[0] },
            { "slug": "deepseek-v4-flash", "auto_review_model_override": overrides[1] }
        ]})
        .to_string()
    };
    let restored = |overrides| {
        build_simplified_catalog_from_texts("", &catalog(overrides))
            .expect("entries")
            .get("autoReviewModel")
            .and_then(Value::as_str)
            .map(str::to_string)
    };
    assert_eq!(
        restored(["deepseek-v4-flash", "deepseek-v4-flash"]).as_deref(),
        Some("deepseek-v4-flash")
    );
    assert_eq!(restored(["ccs-kimi/k3", "ccs-kimi/k3"]), None);
    assert_eq!(restored(["deepseek-v4-flash", "deepseek-v4-pro"]), None);

    let plain = r#"{ "models": [{ "slug": "deepseek-v4-pro" }] }"#;
    assert!(build_simplified_catalog_from_texts("", plain)
        .expect("entries")
        .get("autoReviewModel")
        .is_none());
}

/// 在官方列表换成 `rows` 的情况下跑 `f`（本线程）。
fn with_official_models<T>(rows: Vec<Value>, f: impl FnOnce() -> T) -> T {
    CODEX_OPENAI_OFFICIAL_MODELS_OVERRIDE.with(|slot| *slot.borrow_mut() = Some(rows));
    let out = f();
    CODEX_OPENAI_OFFICIAL_MODELS_OVERRIDE.with(|slot| *slot.borrow_mut() = None);
    out
}

/// 官方 GPT 条目的形状：提示词在 `model_messages`，带 freeform 工具、Lite 协议、速度档。
fn official_gpt_row(slug: &str, prompt: &str) -> Value {
    native_row(
        slug,
        json!({
            "display_name": slug.to_uppercase(),
            "model_messages": { "instructions_template": prompt },
            "apply_patch_tool_type": "freeform",
            "web_search_tool_type": "text_and_image",
            "tool_mode": "code_mode_only",
            "use_responses_lite": true,
            "visibility": "hide",
            "service_tiers": [{ "id": "priority", "name": "Fast" }],
            "additional_speed_tiers": ["fast"],
            "upgrade": { "model": "gpt-next" },
            "context_window": 272_000,
            "max_context_window": 872_000,
            "auto_compact_token_limit": 244_800,
            "effective_context_window_percent": 95,
            "supports_image_detail_original": true,
            "supported_reasoning_levels": [
                { "effort": "low", "description": "l" },
                { "effort": "xhigh", "description": "x" }
            ],
            "default_reasoning_level": "low",
        }),
    )
}

fn official_gpt_rows() -> Vec<Value> {
    normalize_codex_native_rows(vec![
        official_gpt_row("gpt-6", "GPT-6 base prompt"),
        official_gpt_row("gpt-6-sol", "GPT-6 Sol prompt"),
        official_gpt_row("gpt-5.5", "GPT-5.5 prompt"),
    ])
    .unwrap()
}

fn catalog_for(models: Value, config: &str, profile: CodexCatalogToolProfile) -> Vec<Value> {
    let settings = json!({ "modelCatalog": { "models": models } });
    codex_model_catalog_from_settings(&settings, config, profile)
        .unwrap()
        .unwrap()["models"]
        .as_array()
        .unwrap()
        .clone()
}

#[test]
fn card_can_keep_official_speed_tiers_on_native_gpt_rows() {
    let catalog = |keep: bool, profile| {
        let settings = json!({ "modelCatalog": {
            "models": [{ "model": "gpt-6-sol" }, { "model": "glm-5" }],
            "officialSpeedTiers": keep
        }});
        with_official_models(official_gpt_rows(), || {
            codex_model_catalog_from_settings(&settings, "", profile)
                .unwrap()
                .unwrap()
        })
    };
    let tiers = |catalog: &Value, row: usize| {
        (
            catalog["models"][row]["additional_speed_tiers"].clone(),
            catalog["models"][row]["service_tiers"].clone(),
        )
    };
    let official = (
        json!(["fast"]),
        json!([{ "id": "priority", "name": "Fast" }]),
    );
    let cleared = (json!([]), json!([]));

    let kept = catalog(true, CodexCatalogToolProfile::NativeResponses);
    assert_eq!(tiers(&kept, 0), official);
    // 模板生成的非 GPT 行没有官方档可还。
    assert_eq!(tiers(&kept, 1), cleared);
    // 默认关；Chat 转换会把 service_tier 转给不认它的上游，开了也不还。
    assert_eq!(
        tiers(&catalog(false, CodexCatalogToolProfile::NativeResponses), 0),
        cleared
    );
    assert_eq!(
        tiers(&catalog(true, CodexCatalogToolProfile::ProxyChat), 0),
        cleared
    );

    // 反向解析把开关还原到卡上，镜像行照旧只还原模型名。
    let restored = |catalog: &Value| {
        with_official_models(official_gpt_rows(), || {
            build_simplified_catalog_from_texts("", &catalog.to_string()).expect("entries")
        })
    };
    let simplified = restored(&kept);
    assert_eq!(simplified["officialSpeedTiers"], true);
    assert_eq!(simplified["models"][0], json!({ "model": "gpt-6-sol" }));
    assert!(
        restored(&catalog(false, CodexCatalogToolProfile::NativeResponses))
            .get("officialSpeedTiers")
            .is_none()
    );
}

#[test]
fn official_models_match_like_codex() {
    let rows = official_gpt_rows();
    let found = |model: &str| {
        find_codex_official_model(model, &rows)
            .and_then(|row| row["slug"].as_str())
            .map(str::to_string)
    };
    // 最长前缀。
    assert_eq!(found("gpt-6-sol").as_deref(), Some("gpt-6-sol"));
    assert_eq!(found("gpt-6-sol-high").as_deref(), Some("gpt-6-sol"));
    assert_eq!(found("gpt-6-luna").as_deref(), Some("gpt-6"));
    // 去掉一层简单的命名空间。
    assert_eq!(found("openai/gpt-5.5").as_deref(), Some("gpt-5.5"));
    assert_eq!(found("my_relay-1/gpt-6-sol").as_deref(), Some("gpt-6-sol"));
    assert_eq!(found("a/b/gpt-5.5"), None);
    assert_eq!(found("bad ns/gpt-5.5"), None);
    assert_eq!(found("/gpt-5.5"), None);
    // 区分大小写，和 Codex 一样。
    assert_eq!(found("GPT-5.5"), None);
    assert_eq!(found("gpt-5"), None);
    assert_eq!(found("glm-5"), None);
}

#[test]
fn native_gpt_rows_mirror_the_official_entry() {
    let models = with_official_models(official_gpt_rows(), || {
        catalog_for(
            json!([
                // 未声明覆盖时，提示词、工具、思考档位和窗口保持官方值。
                { "model": "gpt-6-sol" },
                { "model": "glm-5" }
            ]),
            "",
            CodexCatalogToolProfile::NativeResponses,
        )
    });

    let gpt = &models[0];
    assert_eq!(gpt["slug"], "gpt-6-sol");
    assert_eq!(gpt["display_name"], "GPT-6-SOL");
    assert_eq!(
        gpt["model_messages"]["instructions_template"],
        "GPT-6 Sol prompt"
    );
    assert_eq!(gpt["apply_patch_tool_type"], "freeform");
    assert_eq!(gpt["web_search_tool_type"], "text_and_image");
    assert_eq!(gpt["tool_mode"], "code_mode_only");
    assert_eq!(gpt["context_window"], 272_000);
    assert_eq!(gpt["max_context_window"], 872_000);
    assert_eq!(gpt["auto_compact_token_limit"], 244_800);
    assert_eq!(gpt["default_reasoning_level"], "low");
    assert_eq!(gpt["supported_reasoning_levels"][1]["effort"], "xhigh");
    assert_eq!(gpt["supports_image_detail_original"], true);
    // 官方账号、官方后端的字段改掉。
    assert_eq!(gpt["use_responses_lite"], false);
    assert_eq!(gpt["visibility"], "list");
    assert_eq!(gpt["service_tiers"], json!([]));
    assert_eq!(gpt["additional_speed_tiers"], json!([]));
    assert_eq!(gpt["upgrade"], Value::Null);
    assert_eq!(gpt["priority"], 1000);

    // 没命中的行照旧：通用模板、去掉 custom 工具。
    let glm = &models[1];
    assert_eq!(glm["slug"], "glm-5");
    assert_eq!(glm["priority"], 1001);
    assert!(glm.get("model_messages").is_none());
    assert!(glm.get("apply_patch_tool_type").is_none());
    assert_eq!(glm["shell_type"], "shell_command");
}

#[test]
fn official_gpt_rows_honor_explicit_context_windows() {
    with_official_models(official_gpt_rows(), || {
        for profile in [
            CodexCatalogToolProfile::NativeResponses,
            CodexCatalogToolProfile::ProxyChat,
        ] {
            let models = catalog_for(
                json!([
                    { "model": "gpt-6-sol", "contextWindow": 1_050_000 },
                    { "model": "openai/gpt-5.5", "contextWindow": "512000" },
                    { "model": "gpt-6-sol-high", "contextWindow": 128_000 },
                    { "model": "gpt-6", "contextWindow": "" }
                ]),
                "",
                profile,
            );
            for (model, window) in models.iter().zip([1_050_000, 512_000, 128_000]) {
                assert_eq!(model["context_window"], window);
                assert_eq!(model["max_context_window"], window);
                assert_eq!(model["auto_compact_token_limit"], window * 9 / 10);
                assert_eq!(model["effective_context_window_percent"], 95);
                assert_eq!(model["apply_patch_tool_type"], "freeform");
            }
            assert_eq!(
                models[0]["model_messages"]["instructions_template"],
                "GPT-6 Sol prompt"
            );
            assert_eq!(models[3]["context_window"], 272_000);
            assert_eq!(models[3]["max_context_window"], 872_000);
            assert_eq!(models[3]["auto_compact_token_limit"], 244_800);
        }
    });
}

#[test]
fn official_gpt_rows_honor_global_context_window_overrides() {
    with_official_models(official_gpt_rows(), || {
        let models = catalog_for(
            json!([
                { "model": "gpt-6-sol" },
                { "model": "gpt-5.5", "contextWindow": 512_000 }
            ]),
            "model_context_window = 1000000\nmodel_auto_compact_token_limit = 900000\n",
            CodexCatalogToolProfile::NativeResponses,
        );
        for model in models {
            // Codex clamps config overrides to max_context_window, so both must match.
            assert_eq!(model["context_window"], 1_000_000);
            assert_eq!(model["max_context_window"], 1_000_000);
            assert_eq!(model["auto_compact_token_limit"], 900_000);
        }
    });
}

#[test]
fn gemini_context_windows_and_compaction_limits_agree_in_every_catalog_mode() {
    for profile in [
        CodexCatalogToolProfile::NativeResponses,
        CodexCatalogToolProfile::ProxyChat,
    ] {
        for window in [272_000_u64, 1_000_000] {
            let settings = json!({ "modelCatalog": { "models": [
                { "model": "gemini-3.8-flash-high", "contextWindow": window },
                { "model": "gemini-3.8-pro" }
            ] } });
            let single = codex_model_catalog_from_settings(&settings, "", profile)
                .unwrap()
                .unwrap();
            assert_eq!(single["models"][0]["context_window"], window);
            assert_eq!(single["models"][0]["max_context_window"], window);
            assert_eq!(
                single["models"][0]["auto_compact_token_limit"],
                window * 9 / 10
            );
            assert_eq!(single["models"][1]["context_window"], 128_000);

            let stacked = plan_codex_stack_catalog(
                CodexStackRoute::ThirdParty(CodexCatalogRow {
                    settings: &json!({}),
                    config_text: "",
                    profile,
                }),
                &[CodexStackCatalogMember {
                    key: "gemini",
                    provider_name: "Gemini",
                    row: CodexCatalogRow {
                        settings: &settings,
                        config_text: "",
                        profile,
                    },
                }],
                false,
            )
            .unwrap();
            let model = &stacked["models"][0];
            assert_eq!(model["slug"], "ccs-gemini/gemini-3.8-flash-high");
            assert_eq!(model["context_window"], window);
            assert_eq!(model["auto_compact_token_limit"], window * 9 / 10);

            let global = codex_model_catalog_from_settings(
                &settings,
                "model_context_window = 512000\nmodel_auto_compact_token_limit = 400000\n",
                profile,
            )
            .unwrap()
            .unwrap();
            for model in global["models"].as_array().unwrap() {
                assert_eq!(model["context_window"], 512_000);
                assert_eq!(model["max_context_window"], 512_000);
                assert_eq!(model["auto_compact_token_limit"], 400_000);
            }
        }
    }
}

#[test]
fn official_gpt_context_windows_survive_catalog_round_trip() {
    with_official_models(official_gpt_rows(), || {
        for window in [128_000, 512_000, 1_050_000] {
            let input = json!([{ "model": "gpt-6-sol", "contextWindow": window }]);
            let catalog = json!({ "models": catalog_for(
                input.clone(), "", CodexCatalogToolProfile::NativeResponses,
            ) });
            let simplified = build_simplified_catalog_from_texts("", &catalog.to_string()).unwrap();
            // Only the user's window is restored, without sticky official capabilities.
            assert_eq!(simplified["models"], input);
            let regenerated = codex_model_catalog_from_settings(
                &json!({ "modelCatalog": simplified }),
                "",
                CodexCatalogToolProfile::NativeResponses,
            )
            .unwrap()
            .unwrap();
            assert_eq!(regenerated, catalog);
        }
    });
}

#[test]
fn official_gpt_global_context_windows_do_not_become_per_model_overrides() {
    with_official_models(official_gpt_rows(), || {
        let config = "model_context_window = 1000000\n";
        let catalog = json!({ "models": catalog_for(
            json!([{ "model": "gpt-6-sol" }]),
            config,
            CodexCatalogToolProfile::NativeResponses,
        ) });
        let simplified = build_simplified_catalog_from_texts(config, &catalog.to_string()).unwrap();
        assert_eq!(simplified["models"], json!([{ "model": "gpt-6-sol" }]));
        let regenerated = codex_model_catalog_from_settings(
            &json!({ "modelCatalog": simplified }),
            config,
            CodexCatalogToolProfile::NativeResponses,
        )
        .unwrap()
        .unwrap();
        assert_eq!(regenerated, catalog);
    });
}

#[test]
fn stacked_official_gpt_rows_keep_each_providers_context_window() {
    with_official_models(official_gpt_rows(), || {
        let route = json!({ "modelCatalog": { "models": [
            { "model": "gpt-6-sol", "contextWindow": 1_050_000 }
        ] } });
        let member = json!({ "modelCatalog": { "models": [
            { "model": "gpt-6-sol", "contextWindow": 512_000 }
        ] } });
        let catalog = plan_codex_stack_catalog(
            CodexStackRoute::ThirdParty(CodexCatalogRow {
                settings: &route,
                config_text: "",
                profile: CodexCatalogToolProfile::NativeResponses,
            }),
            &[CodexStackCatalogMember {
                key: "relay",
                provider_name: "Relay",
                row: CodexCatalogRow {
                    settings: &member,
                    config_text: "",
                    profile: CodexCatalogToolProfile::ProxyChat,
                },
            }],
            false,
        )
        .unwrap();
        let models = catalog["models"].as_array().unwrap();
        for (model, window) in models.iter().zip([1_050_000, 512_000]) {
            assert_eq!(model["context_window"], window);
            assert_eq!(model["max_context_window"], window);
            assert_eq!(model["auto_compact_token_limit"], window * 9 / 10);
            assert_eq!(
                model["model_messages"]["instructions_template"],
                "GPT-6 Sol prompt"
            );
        }
    });
}

#[test]
fn aliases_keep_their_own_name() {
    let models = with_official_models(official_gpt_rows(), || {
        catalog_for(
            json!([{ "model": "openai/gpt-5.5" }, { "model": "gpt-6-sol-high" }]),
            "",
            CodexCatalogToolProfile::NativeResponses,
        )
    });
    assert_eq!(models[0]["slug"], "openai/gpt-5.5");
    assert_eq!(models[0]["display_name"], "openai/gpt-5.5");
    assert_eq!(
        models[0]["model_messages"]["instructions_template"],
        "GPT-5.5 prompt"
    );
    assert_eq!(models[1]["slug"], "gpt-6-sol-high");
    assert_eq!(models[1]["display_name"], "gpt-6-sol-high");
    assert_eq!(
        models[1]["model_messages"]["instructions_template"],
        "GPT-6 Sol prompt"
    );
}

#[test]
fn proxy_chat_gpt_rows_mirror_but_never_send_original_image_detail() {
    let models = with_official_models(official_gpt_rows(), || {
        catalog_for(
            json!([{ "model": "gpt-6-sol" }]),
            "",
            CodexCatalogToolProfile::ProxyChat,
        )
    });
    assert_eq!(
        models[0]["model_messages"]["instructions_template"],
        "GPT-6 Sol prompt"
    );
    assert_eq!(models[0]["apply_patch_tool_type"], "freeform");
    assert_eq!(models[0]["use_responses_lite"], false);
    assert_eq!(models[0]["supports_image_detail_original"], false);
}

#[test]
fn anthropic_and_deepseek_rows_do_not_mirror_openai() {
    let (anthropic, deepseek) = with_official_models(official_gpt_rows(), || {
        (
            catalog_for(
                json!([{ "model": "gpt-6-sol" }]),
                "",
                CodexCatalogToolProfile::Anthropic,
            ),
            catalog_for(
                json!([{ "model": "gpt-6-sol" }]),
                DEEPSEEK_NATIVE_CONFIG,
                CodexCatalogToolProfile::NativeResponses,
            ),
        )
    });
    // Responses→Anthropic 的转换会丢掉 custom 工具：照旧用通用模板。
    assert!(anthropic[0].get("model_messages").is_none());
    assert!(anthropic[0].get("apply_patch_tool_type").is_none());
    // DeepSeek 官方网关按它自己的目录。
    assert_ne!(
        deepseek[0]["model_messages"]["instructions_template"],
        "GPT-6 Sol prompt"
    );
}

#[test]
fn stacked_gpt_rows_mirror_under_their_prefixed_id() {
    let models = with_official_models(official_gpt_rows(), || {
        let route = json!({ "modelCatalog": { "models": [{ "model": "gpt-6-sol" }] } });
        let member = json!({ "modelCatalog": { "models": [{ "model": "gpt-6-sol" }] } });
        plan_codex_stack_catalog(
            CodexStackRoute::ThirdParty(CodexCatalogRow {
                settings: &route,
                config_text: "",
                profile: CodexCatalogToolProfile::NativeResponses,
            }),
            &[CodexStackCatalogMember {
                key: "relay",
                provider_name: "Relay",
                row: CodexCatalogRow {
                    settings: &member,
                    config_text: "",
                    profile: CodexCatalogToolProfile::ProxyChat,
                },
            }],
            false,
        )
        .unwrap()["models"]
            .as_array()
            .unwrap()
            .clone()
    });
    // 两家都有 GPT-6 Sol：各一条，内容都是官方的。
    assert_eq!(models[0]["slug"], "gpt-6-sol");
    assert_eq!(models[1]["slug"], "ccs-relay/gpt-6-sol");
    for model in &models {
        assert_eq!(
            model["model_messages"]["instructions_template"],
            "GPT-6 Sol prompt"
        );
        assert_eq!(model["use_responses_lite"], false);
    }
    // 各家按自己的链路：走 Chat 的那家不发 original 精度的图片。
    assert_eq!(models[0]["supports_image_detail_original"], true);
    assert_eq!(models[1]["supports_image_detail_original"], false);
}

/// #9665：没配目录的行在直连下由 Codex 用官方条目（GPT-6 走 Lite），聚合下要一致；
/// 显式配了目录的行、走 Chat 转换的行仍不用 Lite。
#[test]
fn stacked_rows_without_catalog_follow_codex_responses_lite_default() {
    let models = with_official_models(official_gpt_rows(), || {
        let bare = json!({});
        let explicit = json!({ "modelCatalog": { "models": [{ "model": "gpt-6-sol" }] } });
        let member = |key, settings, profile| CodexStackCatalogMember {
            key,
            provider_name: key,
            row: CodexCatalogRow {
                settings,
                config_text: "model = \"gpt-6-sol\"\n",
                profile,
            },
        };
        plan_codex_stack_catalog(
            CodexStackRoute::ThirdParty(CodexCatalogRow {
                settings: &bare,
                config_text: "model = \"gpt-6-sol\"\n",
                profile: CodexCatalogToolProfile::NativeResponses,
            }),
            &[
                member("bare", &bare, CodexCatalogToolProfile::NativeResponses),
                member(
                    "listed",
                    &explicit,
                    CodexCatalogToolProfile::NativeResponses,
                ),
                member("chat", &bare, CodexCatalogToolProfile::ProxyChat),
            ],
            false,
        )
        .unwrap()["models"]
            .as_array()
            .unwrap()
            .clone()
    });
    let lite: Vec<_> = models
        .iter()
        .map(|model| {
            (
                model["slug"].as_str().unwrap().to_string(),
                model["use_responses_lite"].clone(),
            )
        })
        .collect();
    assert_eq!(
        lite,
        vec![
            ("gpt-6-sol".to_string(), json!(true)),
            ("ccs-bare/gpt-6-sol".to_string(), json!(true)),
            ("ccs-listed/gpt-6-sol".to_string(), json!(false)),
            ("ccs-chat/gpt-6-sol".to_string(), json!(false)),
        ]
    );
}

/// 「打开终端」给没配目录的卡盖掉上一家留下的指针时生成的单模型目录，Lite 同样跟 Codex
/// 的默认走（#9665）；走转换的卡、对不上官方条目的模型仍不用 Lite。
#[test]
fn single_model_catalog_follows_codex_responses_lite_default() {
    let lite = |model: &str, profile| {
        with_official_models(official_gpt_rows(), || {
            codex_single_model_catalog(model, "", profile).unwrap()["models"][0]
                ["use_responses_lite"]
                .clone()
        })
    };
    assert_eq!(
        lite("gpt-6-sol", CodexCatalogToolProfile::NativeResponses),
        json!(true)
    );
    assert_eq!(
        lite("gpt-6-sol", CodexCatalogToolProfile::ProxyChat),
        json!(false)
    );
    assert_ne!(
        lite("kimi-k2", CodexCatalogToolProfile::NativeResponses),
        json!(true)
    );
}

#[test]
fn mirrored_rows_round_trip_as_bare_models() {
    let (catalog, simplified) = with_official_models(official_gpt_rows(), || {
        let catalog = json!({
            "models": catalog_for(
                json!([
                    { "model": "gpt-6-sol" },
                    { "model": "glm-5", "displayName": "GLM 5", "contextWindow": 200_000 }
                ]),
                "",
                CodexCatalogToolProfile::NativeResponses,
            )
        });
        let simplified = build_simplified_catalog_from_texts("", &catalog.to_string());
        (catalog, simplified)
    });
    // 官方的显示名、窗口、档位不能被当成用户填的存回去。
    assert_eq!(catalog["models"][0]["display_name"], "GPT-6-SOL");
    let rows = simplified.unwrap()["models"].as_array().unwrap().clone();
    assert_eq!(rows[0], json!({ "model": "gpt-6-sol" }));
    assert_eq!(rows[1]["model"], "glm-5");
    assert_eq!(rows[1]["displayName"], "GLM 5");
    assert_eq!(rows[1]["contextWindow"], 200_000);
}

#[test]
fn mirrored_rows_use_and_round_trip_the_configured_display_name() {
    let (catalog, simplified) = with_official_models(official_gpt_rows(), || {
        let catalog = json!({
            "models": catalog_for(
                json!([
                    // 精确命中和前缀命中的别名都用用户填的显示名（#9380）。
                    { "model": "gpt-6-sol", "displayName": "My Sol" },
                    { "model": "gpt-6-sol-special", "displayName": "gpt-6-sol" }
                ]),
                "",
                CodexCatalogToolProfile::NativeResponses,
            )
        });
        let simplified = build_simplified_catalog_from_texts("", &catalog.to_string());
        (catalog, simplified)
    });
    let models = catalog["models"].as_array().unwrap();
    assert_eq!(models[0]["display_name"], "My Sol");
    assert_eq!(models[1]["display_name"], "gpt-6-sol");
    // 显示名不影响照搬官方的提示词。
    assert_eq!(
        models[1]["model_messages"]["instructions_template"],
        "GPT-6 Sol prompt"
    );
    let rows = simplified.unwrap()["models"].as_array().unwrap().clone();
    assert_eq!(
        rows[0],
        json!({ "model": "gpt-6-sol", "displayName": "My Sol" })
    );
    assert_eq!(
        rows[1],
        json!({ "model": "gpt-6-sol-special", "displayName": "gpt-6-sol" })
    );
}

#[test]
fn renamed_mirror_is_told_apart_from_an_old_clone_by_description() {
    let rows = official_gpt_rows();
    let official = &rows[2];
    let mut mirror = codex_official_mirror_baseline(official, "gpt-5.5");
    mirror["description"] = json!("Official description");
    let mut official = official.clone();
    official["description"] = json!("Official description");
    mirror["display_name"] = json!("My GPT");
    assert!(is_codex_official_mirror(&mirror, "gpt-5.5", &official));
    // 旧版克隆模板时 description 跟着显示名走。
    let mut clone = mirror.clone();
    clone["description"] = json!("My GPT");
    assert!(!is_codex_official_mirror(&clone, "gpt-5.5", &official));
}

#[test]
fn old_proxy_chat_clones_keep_their_user_values_on_round_trip() {
    // 旧版 ProxyChat 克隆 gpt-5.5 模板（带 model_messages），再叠用户填的值。
    let rows = official_gpt_rows();
    let settings = json!({ "modelCatalog": { "models": [
        { "model": "gpt-5.5", "displayName": "My GPT", "contextWindow": 200_000 },
        { "model": "gpt-6-sol" },
        // 窗口恰好等于官方默认、模态和并行也和官方一致：只有显示名能认出是旧克隆。
        { "model": "gpt-6", "displayName": "Old Six", "contextWindow": 272_000 }
    ] } });
    let old = codex_model_catalog_from_specs(
        &codex_catalog_model_specs(&settings),
        &rows[2],
        CodexCatalogToolProfile::ProxyChat,
        128_000,
    );
    let simplified = with_official_models(rows, || {
        build_simplified_catalog_from_texts("", &old.to_string())
    })
    .unwrap();
    let models = simplified["models"].as_array().unwrap();
    assert_eq!(models[0]["displayName"], "My GPT");
    assert_eq!(models[0]["contextWindow"], 200_000);
    // 克隆 gpt-5.5 的 gpt-6-sol 行：显示名、窗口都不是官方的，照旧还原，不当成镜像。
    assert_eq!(models[1]["model"], "gpt-6-sol");
    assert_ne!(models[1], json!({ "model": "gpt-6-sol" }));
    assert_eq!(models[2]["displayName"], "Old Six");
    assert_eq!(models[2]["contextWindow"], 272_000);
}

fn native_row(slug: &str, extra: Value) -> Value {
    let mut row = json!({
        "slug": slug,
        "priority": 1,
        "comp_hash": "3000",
        "supports_reasoning_summaries": true,
        "supports_parallel_tool_calls": false,
    });
    for (key, value) in extra.as_object().unwrap() {
        if value.is_null() {
            row.as_object_mut().unwrap().remove(key);
        } else {
            row[key] = value.clone();
        }
    }
    row
}

#[test]
fn native_rows_get_the_old_instructions_field_and_missing_required_fields() {
    let rows = normalize_codex_native_rows(vec![
        // 0.158 缓存的形状：只有新的指令字段。
        native_row(
            "gpt-6-sol",
            json!({ "model_messages": { "instructions_template": "You are Codex." } }),
        ),
        // 空串也照抄。
        native_row(
            "gpt-6-luna",
            json!({ "model_messages": { "instructions_template": "" } }),
        ),
        // 缺必填字段就补，已有值不覆盖。
        native_row(
            "gpt-5.5",
            json!({
                "supports_parallel_tool_calls": null,
                "base_instructions": "old",
            }),
        ),
        native_row(
            "both",
            json!({
                "base_instructions": "old",
                "model_messages": { "instructions_template": "new" },
            }),
        ),
    ])
    .expect("valid rows");
    assert_eq!(rows[0]["base_instructions"], "You are Codex.");
    assert_eq!(rows[0]["supports_parallel_tool_calls"], false);
    assert_eq!(rows[0]["comp_hash"], "3000", "native fields stay");
    assert_eq!(rows[1]["base_instructions"], "");
    assert_eq!(rows[2]["base_instructions"], "old");
    assert!(rows[2]["supports_parallel_tool_calls"].is_boolean());
    assert_eq!(rows[3]["base_instructions"], "old");
    assert_eq!(rows[3]["model_messages"]["instructions_template"], "new");
}

#[test]
fn one_bad_native_row_rejects_the_whole_source() {
    let good = native_row("gpt-5.5", json!({ "base_instructions": "x" }));
    for bad in [
        native_row("no-instructions", json!({})),
        native_row("", json!({ "base_instructions": "x" })),
    ] {
        assert!(
            normalize_codex_native_rows(vec![good.clone(), bad.clone()]).is_none(),
            "{bad}"
        );
    }
    assert!(normalize_codex_native_rows(Vec::new()).is_none());
}

#[test]
fn official_rows_stay_native_and_stacked_rows_follow() {
    let native = normalize_codex_native_rows(vec![
        native_row(
            "gpt-6-sol",
            json!({ "priority": 4, "base_instructions": "x", "use_responses_lite": true }),
        ),
        native_row(
            "gpt-5.5",
            json!({ "priority": 12, "base_instructions": "x", "visibility": "hide" }),
        ),
        native_row(
            "gpt-6-astra",
            json!({ "priority": 1, "base_instructions": "x", "use_responses_lite": true }),
        ),
    ])
    .unwrap();
    let stacked_settings = json!({});
    let catalog = plan_codex_stack_catalog(
        CodexStackRoute::Official {
            native,
            config_text: "",
        },
        &[CodexStackCatalogMember {
            key: "ds",
            provider_name: "DS",
            row: CodexCatalogRow {
                settings: &stacked_settings,
                config_text: "model = \"deepseek-v4-pro\"\n",
                profile: CodexCatalogToolProfile::NativeResponses,
            },
        }],
        false,
    )
    .unwrap();
    let models = catalog["models"].as_array().unwrap();
    let slugs: Vec<&str> = models.iter().map(|m| m["slug"].as_str().unwrap()).collect();
    // 官方的顺序（按 priority）不变，Stack 的在后面。
    assert_eq!(
        slugs,
        vec![
            "gpt-6-astra",
            "gpt-6-sol",
            "gpt-5.5",
            "ccs-ds/deepseek-v4-pro"
        ]
    );
    assert_eq!(
        models[1]["use_responses_lite"], true,
        "official Lite rows keep Lite"
    );
    assert_eq!(models[1]["comp_hash"], "3000");
    assert_eq!(models[2]["visibility"], "hide", "native visibility stays");
    assert!(models[0].get("auto_compact_token_limit").is_none());
    assert_eq!(models[3]["comp_hash"], "cc-switch");
}

/// 打开「经典子 agent」后每一行都写 v1（官方行、Stack 行都写）；关着时官方行保持原值，
/// Stack 行沿用模板的值。
#[test]
fn multi_agent_v2_override_is_read_in_both_shapes() {
    assert!(codex_config_forces_multi_agent_v2(
        "[features]\nmulti_agent_v2 = true\n"
    ));
    assert!(codex_config_forces_multi_agent_v2(
        "[features.multi_agent_v2]\nenabled = true\nmax_threads = 4\n"
    ));
    assert!(codex_config_forces_multi_agent_v2(
        "features = { multi_agent_v2 = { enabled = true } }\n"
    ));
    for text in [
        "",
        "[features]\nmulti_agent_v2 = false\n",
        "[features.multi_agent_v2]\nmax_threads = 4\n",
        "[features]\nmulti_agent = true\n",
        "not toml = [",
    ] {
        assert!(!codex_config_forces_multi_agent_v2(text), "{text}");
    }
}

#[test]
fn classic_subagents_stamp_every_stacked_catalog_row() {
    let plan = |classic: bool| {
        let native = normalize_codex_native_rows(vec![
            native_row(
                "gpt-6-sol",
                json!({ "base_instructions": "x", "multi_agent_version": "v2" }),
            ),
            native_row("gpt-5.5", json!({ "base_instructions": "x" })),
        ])
        .unwrap();
        let stacked_settings = json!({});
        plan_codex_stack_catalog(
            CodexStackRoute::Official {
                native,
                config_text: "",
            },
            &[CodexStackCatalogMember {
                key: "ds",
                provider_name: "DS",
                row: CodexCatalogRow {
                    settings: &stacked_settings,
                    config_text: "model = \"deepseek-v4-pro\"\n",
                    profile: CodexCatalogToolProfile::NativeResponses,
                },
            }],
            classic,
        )
        .unwrap()
    };

    let classic = plan(true);
    let models = classic["models"].as_array().unwrap();
    assert_eq!(models.len(), 3);
    for model in models {
        assert_eq!(model["multi_agent_version"], "v1", "{}", model["slug"]);
    }

    let native = plan(false);
    let models = native["models"].as_array().unwrap();
    assert_eq!(models[0]["slug"], "gpt-6-sol");
    assert_eq!(models[0]["multi_agent_version"], "v2");
    assert!(models[1].get("multi_agent_version").is_none());
}

#[test]
fn build_simplified_catalog_leaves_stack_models_out_of_the_route_row() {
    let catalog = r#"{
            "models": [
                { "slug": "deepseek/deepseek-v4" },
                { "slug": "ccs-kimi/kimi-k3" }
            ]
        }"#;
    let result = build_simplified_catalog_from_texts("", catalog).expect("entries");
    let models: Vec<&str> = result["models"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["model"].as_str().unwrap())
        .collect();
    // 路由那家自己的 `vendor/model` 名字不受影响。
    assert_eq!(models, vec!["deepseek/deepseek-v4"]);
}

/// #8485：第三方路由那家的行和 Stack 行同一个 `comp_hash`，两者之间切换时 Codex 不会先
/// 用上一家压缩（上一家额度用完时压缩失败，会话每轮都卡在上一家）。名单为空时的目录
/// 仍是模板的值。
#[test]
fn third_party_route_rows_share_the_stack_comp_hash() {
    let route_settings = json!({ "modelCatalog": { "models": [{ "model": "deepseek-v4-pro" }] } });
    let route_text = "model_provider = \"deepseek\"\nmodel = \"deepseek-v4-pro\"\n\
                          [model_providers.deepseek]\nbase_url = \"https://api.deepseek.com/v1\"\n";
    let profile = CodexCatalogToolProfile::NativeResponses;
    let plain = codex_model_catalog_from_settings(&route_settings, route_text, profile)
        .unwrap()
        .unwrap();
    assert_eq!(plain["models"][0]["comp_hash"], "3000");

    let stacked_settings = json!({});
    let stacked = plan_codex_stack_catalog(
        CodexStackRoute::ThirdParty(CodexCatalogRow {
            settings: &route_settings,
            config_text: route_text,
            profile,
        }),
        &[CodexStackCatalogMember {
            key: "ds",
            provider_name: "DS",
            row: CodexCatalogRow {
                settings: &stacked_settings,
                config_text: route_text,
                profile,
            },
        }],
        false,
    )
    .unwrap();
    let models = stacked["models"].as_array().unwrap();
    assert_eq!(models[0]["slug"], "deepseek-v4-pro");
    assert_eq!(models[1]["slug"], "ccs-ds/deepseek-v4-pro");
    for model in models {
        assert_eq!(model["comp_hash"], "cc-switch", "{}", model["slug"]);
    }
}

/// #8676：共用 `comp_hash` 的行切换时不压缩，所以按工具协议分组。镜像官方条目的第三方
/// GPT 行（路由那家和 Stack 成员）保留 code mode，自成一组；direct 行仍是原来的值。
/// 除 `comp_hash` 外，GPT 行和直连目录里的同一行一致。
#[test]
fn stack_rows_share_history_only_within_one_tool_protocol() {
    let gpt_settings = json!({ "modelCatalog": { "models": [{ "model": "gpt-6-sol" }] } });
    let gpt_text = "model = \"gpt-6-sol\"\n";
    let ds_settings = json!({ "modelCatalog": { "models": [{ "model": "deepseek-v4-pro" }] } });
    let ds_text = "model = \"deepseek-v4-pro\"\n";
    let profile = CodexCatalogToolProfile::NativeResponses;
    let row = |settings, config_text| CodexCatalogRow {
        settings,
        config_text,
        profile,
    };
    let (direct, stacked) = with_official_models(official_gpt_rows(), || {
        let direct = codex_model_catalog_from_settings(&gpt_settings, gpt_text, profile)
            .unwrap()
            .unwrap();
        let stacked = plan_codex_stack_catalog(
            CodexStackRoute::ThirdParty(row(&gpt_settings, gpt_text)),
            &[
                CodexStackCatalogMember {
                    key: "relay",
                    provider_name: "Relay",
                    row: row(&gpt_settings, gpt_text),
                },
                CodexStackCatalogMember {
                    key: "ds",
                    provider_name: "DS",
                    row: row(&ds_settings, ds_text),
                },
            ],
            false,
        )
        .unwrap();
        (direct, stacked)
    });

    let models = stacked["models"].as_array().unwrap();
    let slugs: Vec<&str> = models.iter().map(|m| m["slug"].as_str().unwrap()).collect();
    assert_eq!(
        slugs,
        vec!["gpt-6-sol", "ccs-relay/gpt-6-sol", "ccs-ds/deepseek-v4-pro"]
    );
    let mirrored = &direct["models"][0];
    assert_eq!(mirrored["tool_mode"], "code_mode_only");
    for gpt in &models[..2] {
        assert_eq!(
            gpt["comp_hash"], "cc-switch-code_mode_only",
            "{}",
            gpt["slug"]
        );
        for key in ["tool_mode", "supports_search_tool", "model_messages"] {
            assert_eq!(gpt[key], mirrored[key], "{} {key}", gpt["slug"]);
        }
    }
    assert!(models[2].get("tool_mode").is_none());
    assert_eq!(models[2]["comp_hash"], "cc-switch");
}

#[test]
fn stacked_rows_keep_their_own_tool_profile_and_never_use_responses_lite() {
    let route_settings = json!({ "modelCatalog": { "models": [{ "model": "route-model" }] } });
    let route_text = "model = \"route-model\"\n";
    let stacked_settings = json!({});
    let stacked_text = "model = \"claude-opus-5\"\nmodel_context_window = 400000\n";
    let catalog = plan_codex_stack_catalog(
        CodexStackRoute::ThirdParty(CodexCatalogRow {
            settings: &route_settings,
            config_text: route_text,
            profile: CodexCatalogToolProfile::NativeResponses,
        }),
        &[CodexStackCatalogMember {
            key: "anth",
            provider_name: "Anth",
            row: CodexCatalogRow {
                settings: &stacked_settings,
                config_text: stacked_text,
                profile: CodexCatalogToolProfile::Anthropic,
            },
        }],
        false,
    )
    .expect("catalog");
    let models = catalog["models"].as_array().unwrap();
    assert_eq!(models.len(), 2);
    let stacked = &models[1];
    assert_eq!(stacked["slug"], "ccs-anth/claude-opus-5");
    assert_eq!(stacked["display_name"], "claude-opus-5（Anth）");
    assert_eq!(stacked["description"], "claude-opus-5 · 400K");
    assert_eq!(stacked["shell_type"], "shell_command");
    assert!(stacked.get("apply_patch_tool_type").is_none());
    assert_eq!(stacked["context_window"], 400000);
    assert_eq!(stacked["auto_compact_token_limit"], 360000);
    assert_ne!(stacked["use_responses_lite"], json!(true));
    let priorities: Vec<u64> = models
        .iter()
        .map(|entry| entry["priority"].as_u64().unwrap())
        .collect();
    assert_eq!(priorities, vec![1, 2]);
}

#[test]
fn build_simplified_catalog_squashes_default_context_window() {
    // Default fallback is 128_000 when config.toml has no model_context_window.
    let catalog = r#"{
            "models": [{ "slug": "kimi", "display_name": "kimi", "context_window": 128000 }]
        }"#;
    let result = build_simplified_catalog_from_texts("", catalog).expect("entry");
    let entry = &result.get("models").unwrap().as_array().unwrap()[0];
    assert!(
        entry.get("contextWindow").is_none(),
        "default 128_000 should be squashed so the form shows blank, matching the user's blank input"
    );
}

#[test]
fn build_simplified_catalog_respects_explicit_model_context_window() {
    // When config.toml sets model_context_window, that becomes the default fallback.
    let config = r#"model_context_window = 200000
"#;
    let catalog = r#"{
            "models": [
                { "slug": "a", "display_name": "a", "context_window": 200000 },
                { "slug": "b", "display_name": "b", "context_window": 500000 }
            ]
        }"#;
    let result = build_simplified_catalog_from_texts(config, catalog).expect("entries");
    let models = result.get("models").unwrap().as_array().unwrap();
    // Matches default → squashed.
    assert!(models[0].get("contextWindow").is_none());
    // Different from default → preserved.
    assert_eq!(
        models[1].get("contextWindow").and_then(|v| v.as_u64()),
        Some(500_000)
    );
}

#[test]
fn build_simplified_catalog_squashes_inferred_modalities_and_keeps_overrides() {
    let catalog = r#"{
            "models": [
                { "slug": "gpt-5.4", "input_modalities": ["text", "image"] },
                { "slug": "qwen3-coder-plus", "input_modalities": ["text"] },
                { "slug": "gpt-text-override", "input_modalities": ["text"] },
                { "slug": "glm-5.2", "input_modalities": ["text", "image"] }
            ]
        }"#;

    let result = build_simplified_catalog_from_texts("", catalog).expect("entries");
    let models = result.get("models").unwrap().as_array().unwrap();

    assert!(
        models[0].get("inputModalities").is_none(),
        "GPT text+image is inferred and must not become a sticky hidden override"
    );
    assert!(
        models[1].get("inputModalities").is_none(),
        "confirmed text-only capability is inferred and must remain registry-driven"
    );
    assert_eq!(
        models[2].get("inputModalities"),
        Some(&json!(["text"])),
        "an unknown model explicitly forced to text-only must round-trip"
    );
    assert_eq!(
        models[3].get("inputModalities"),
        Some(&json!(["text", "image"])),
        "an explicit image override for a registered text-only model must round-trip"
    );
}

#[test]
fn build_simplified_catalog_returns_none_when_unparseable() {
    assert!(build_simplified_catalog_from_texts("", "not json").is_none());
    assert!(build_simplified_catalog_from_texts("", "{}").is_none());
    assert!(
        build_simplified_catalog_from_texts("", r#"{"models": []}"#).is_none(),
        "empty models array should yield None so the field is not inserted at all"
    );
    assert!(
        build_simplified_catalog_from_texts("", r#"{"models": [{"display_name": "no slug"}]}"#,)
            .is_none(),
        "entries lacking slug are skipped; a fully-skipped catalog yields None"
    );
}

#[test]
fn codex_cli_candidates_are_non_empty() {
    let candidates = codex_cli_candidates();
    assert!(
        candidates
            .iter()
            .any(|candidate| candidate == Path::new("codex")),
        "codex CLI candidates must include the PATH entry"
    );
}

#[test]
fn bundled_models_tell_a_missing_cli_apart_from_a_failing_one() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let missing = dir.path().join("missing-codex");
    let pick = |catalog: &Value| catalog.get("models").cloned();
    assert_eq!(
        first_bundled_catalog(vec![missing.clone()], None, pick).err(),
        Some(CodexBundledModelsError::NoCli)
    );

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let script = |name: &str, body: &str| {
            let path = dir.path().join(name);
            fs::write(&path, format!("#!/bin/sh\n{body}\n")).expect("write script");
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("chmod script");
            path
        };
        let failing = script("failing-codex", "echo boom >&2; exit 1");
        let working = script("working-codex", r#"echo '{"models":[{"slug":"x"}]}'"#);

        // 找到了命令行但跑失败：不能报成找不到。
        assert!(matches!(
            first_bundled_catalog(vec![missing.clone(), failing.clone()], None, pick),
            Err(CodexBundledModelsError::Failed(detail)) if detail.contains("boom")
        ));
        // 前面的失败不影响后面能用的候选。
        assert_eq!(
            first_bundled_catalog(vec![missing, failing, working], None, pick).ok(),
            Some(json!([{ "slug": "x" }]))
        );
    }
}

/// npm 装的 `codex` 是 `#!/usr/bin/env node` 脚本：GUI 的窄 PATH 里没有 node 会 exit 127，
/// 要靠登录 shell 的 PATH 或候选自己的目录找到它（#9764）。
#[cfg(unix)]
#[test]
fn bundled_models_find_the_interpreter_through_login_path_or_candidate_dir() {
    use std::os::unix::fs::PermissionsExt;
    let write_exec = |path: &Path, body: &str| {
        fs::create_dir_all(path.parent().expect("parent")).expect("create dir");
        fs::write(path, body).expect("write script");
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).expect("chmod");
    };
    let interpreter = "cc-switch-test-fake-node-9764";
    let shim = format!("#!/usr/bin/env {interpreter}\n");
    let runtime = "#!/bin/sh\necho '{\"models\":[{\"slug\":\"x\"}]}'\n";
    let pick = |catalog: &Value| catalog.get("models").cloned();

    let dir = tempfile::tempdir().expect("create temp dir");
    let node_bin = dir.path().join("node/bin");
    write_exec(&node_bin.join(interpreter), runtime);
    let npm_codex = dir.path().join("npm/bin/codex");
    write_exec(&npm_codex, &shim);

    // 继承的 PATH 里没有解释器：跑不起来。
    assert!(matches!(
        first_bundled_catalog(vec![npm_codex.clone()], None, pick),
        Err(CodexBundledModelsError::Failed(_))
    ));
    // 登录 shell 的 PATH 里有：能读出目录。
    let login_path = node_bin.to_string_lossy().into_owned();
    assert_eq!(
        first_bundled_catalog(vec![npm_codex], Some(&login_path), pick).ok(),
        Some(json!([{ "slug": "x" }]))
    );

    // nvm 这类安装，解释器就在候选旁边：不靠登录 shell 也能找到。
    let nvm_bin = dir.path().join("nvm/bin");
    write_exec(&nvm_bin.join(interpreter), runtime);
    let nvm_codex = nvm_bin.join("codex");
    write_exec(&nvm_codex, &shim);
    assert_eq!(
        first_bundled_catalog(vec![nvm_codex], None, pick).ok(),
        Some(json!([{ "slug": "x" }]))
    );
}

#[test]
fn codex_bundled_models_command_uses_expected_program_and_args() {
    let command = codex_bundled_models_command(Path::new("codex"), None);
    assert_eq!(command.get_program(), "codex");
    assert_eq!(
        command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>(),
        ["debug", "models", "--bundled"]
    );
}

#[test]
fn codex_cli_candidates_include_user_node_manager_bins() {
    let temp_home = tempfile::tempdir().expect("create temp home");
    let home = temp_home.path();
    let expected = [
        home.join(".nvm/versions/node/v22.14.0/bin/codex"),
        home.join(".volta/bin/codex"),
        home.join(".asdf/shims/codex"),
        home.join(".local/share/mise/shims/codex"),
        home.join(".local/share/fnm/node-versions/v22.14.0/installation/bin/codex"),
    ];

    for candidate in &expected {
        std::fs::create_dir_all(candidate.parent().expect("candidate parent"))
            .expect("create candidate parent");
        std::fs::write(candidate, "").expect("create candidate");
    }

    let mut candidates = Vec::new();
    let mut seen = HashSet::new();
    push_home_codex_cli_candidates(&mut candidates, &mut seen, home);

    for candidate in expected {
        assert!(
            candidates.contains(&candidate),
            "user-level Codex CLI candidate should be discovered: {}",
            candidate.display()
        );
    }
}

#[test]
fn codex_cli_candidates_deduplicate_entries() {
    let temp_home = tempfile::tempdir().expect("create temp home");
    let home = temp_home.path();
    let candidate = home.join(".volta/bin/codex");
    std::fs::create_dir_all(candidate.parent().expect("candidate parent"))
        .expect("create candidate parent");
    std::fs::write(&candidate, "").expect("create candidate");

    let mut candidates = Vec::new();
    let mut seen = HashSet::new();
    push_existing_codex_cli_candidate(&mut candidates, &mut seen, candidate.clone());
    push_home_codex_cli_candidates(&mut candidates, &mut seen, home);

    assert_eq!(
        candidates.iter().filter(|path| **path == candidate).count(),
        1,
        "duplicate candidates should be removed"
    );
}

fn create_cli_candidate(path: &Path) {
    std::fs::create_dir_all(path.parent().expect("candidate parent"))
        .expect("create candidate parent");
    std::fs::write(path, "").expect("create candidate");
}

fn desktop_cli(applications: &Path, bundle: &str, layout: &str) -> PathBuf {
    applications.join(bundle).join(layout)
}

#[test]
fn codex_cli_candidates_include_desktop_app_bundles_and_layouts() {
    let temp_home = tempfile::tempdir().expect("create temp home");
    let applications = temp_home.path().join("Applications");
    let current_layout = CODEX_DESKTOP_CLI_IN_BUNDLE[0];
    let legacy_layout = CODEX_DESKTOP_CLI_IN_BUNDLE[1];
    let chatgpt_current = desktop_cli(&applications, "ChatGPT.app", current_layout);
    let chatgpt_legacy = desktop_cli(&applications, "ChatGPT.app", legacy_layout);
    // 改名前的 Codex.app 只有旧结构（openai/codex#48052）。
    let codex_legacy = desktop_cli(&applications, "Codex.app", legacy_layout);
    for path in [&chatgpt_current, &chatgpt_legacy, &codex_legacy] {
        create_cli_candidate(path);
    }

    let mut candidates = Vec::new();
    let mut seen = HashSet::new();
    push_desktop_codex_cli_candidates(&mut candidates, &mut seen, temp_home.path());

    let position = |path: &Path| {
        candidates
            .iter()
            .position(|candidate| candidate == path)
            .unwrap_or_else(|| panic!("missing desktop candidate {}", path.display()))
    };
    assert!(position(&chatgpt_current) < position(&chatgpt_legacy));
    // 两份都在时 ChatGPT.app 是新的那份，先用它。
    assert!(position(&chatgpt_legacy) < position(&codex_legacy));
}

fn set_mtime(path: &Path, time: std::time::SystemTime) {
    std::fs::File::options()
        .write(true)
        .open(path)
        .expect("open")
        .set_times(std::fs::FileTimes::new().set_modified(time))
        .expect("set mtime");
}

#[test]
fn codex_cli_candidates_try_the_newest_desktop_version_first() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let root = temp.path();
    let now = std::time::SystemTime::now();
    // 按目录名倒序排的话 0.99.0 会排在前面。
    let older = root.join("0.99.0").join("codex.exe");
    let newer = root.join("0.162.0").join("codex.exe");
    create_cli_candidate(&older);
    create_cli_candidate(&newer);
    set_mtime(&older, now - std::time::Duration::from_secs(3600));
    set_mtime(&newer, now);
    std::fs::create_dir_all(root.join("logs")).expect("create dir without codex.exe");

    let mut candidates = Vec::new();
    let mut seen = HashSet::new();
    push_codex_cli_candidates_newest_first(&mut candidates, &mut seen, root, "codex.exe");

    assert_eq!(candidates, vec![newer, older]);
}

#[test]
fn desktop_cli_versions_come_from_the_bundled_package_and_version_dirs() {
    let temp_home = tempfile::tempdir().expect("create temp home");
    let applications = temp_home.path().join("Applications");
    let package = applications
        .join("ChatGPT.app")
        .join(CODEX_DESKTOP_CLI_PACKAGE_IN_BUNDLE);
    std::fs::create_dir_all(package.parent().unwrap()).unwrap();
    std::fs::write(&package, r#"{"version":"0.169.0-alpha.11.1","target":"x"}"#).unwrap();
    // 旧结构没有包描述：不算，也不报错。
    create_cli_candidate(&desktop_cli(
        &applications,
        "Codex.app",
        CODEX_DESKTOP_CLI_IN_BUNDLE[1],
    ));

    // 本机 /Applications 里真装着的桌面版也会读到，只看临时 home 里的这一份。
    let versions = codex_desktop_cli_versions_in(temp_home.path());
    assert!(versions.contains(&"0.169.0-alpha.11.1".to_string()));

    let bin = temp_home.path().join("bin");
    create_cli_candidate(&bin.join("0.169.0").join("codex.exe"));
    std::fs::create_dir_all(bin.join("logs")).unwrap();
    assert_eq!(
        codex_cli_version_dir_names(&bin, "codex.exe"),
        vec!["0.169.0".to_string()]
    );
}

#[test]
fn desktop_cli_is_the_first_existing_bundled_candidate() {
    let temp_home = tempfile::tempdir().expect("create temp home");
    let applications = temp_home.path().join("Applications");
    let legacy = desktop_cli(&applications, "Codex.app", CODEX_DESKTOP_CLI_IN_BUNDLE[1]);
    create_cli_candidate(&legacy);

    let mut candidates = Vec::new();
    push_desktop_codex_cli_candidates(&mut candidates, &mut HashSet::new(), temp_home.path());
    // 本机 /Applications 里真装着的桌面版会排在临时 home 前面，只比对取的是不是第一个。
    assert!(candidates.contains(&legacy));
    assert_eq!(
        codex_desktop_cli_in(temp_home.path()),
        candidates.first().cloned()
    );
}

#[test]
#[serial]
fn codex_cli_candidates_try_the_desktop_app_cli_after_standalone_installs() {
    let test_home = TestHome::new();
    let home = test_home.path();
    let standalone = home.join(".volta/bin/codex");
    let desktop = desktop_cli(
        &home.join("Applications"),
        "ChatGPT.app",
        CODEX_DESKTOP_CLI_IN_BUNDLE[0],
    );
    create_cli_candidate(&standalone);
    create_cli_candidate(&desktop);

    let candidates = codex_cli_candidates();
    let position = |path: &Path| {
        candidates
            .iter()
            .position(|candidate| candidate == path)
            .unwrap_or_else(|| panic!("missing candidate {}", path.display()))
    };

    // 装了独立 CLI 的人照旧先用它，桌面版的只做兜底。
    assert!(position(&standalone) < position(&desktop));
}

#[test]
fn classic_tool_template_keeps_the_classic_tool_contract() {
    let template = load_codex_classic_tool_template();
    assert_eq!(template["slug"], "gpt-5.5");
    // 转换层照着这套工具写：freeform apply_patch、unified_exec，带 Skills / 插件使用说明。
    assert_eq!(template["apply_patch_tool_type"], "freeform");
    assert_eq!(template["shell_type"], "unified_exec");
    assert_eq!(template["include_skills_usage_instructions"], true);
    assert_eq!(template["include_plugin_usage_instructions"], true);
    // 新官方条目才有的 code mode / v2 子代理 / 实验工具，不能被带给第三方模型。
    assert!(template.get("tool_mode").is_none());
    assert!(template.get("multi_agent_version").is_none());
    assert_eq!(template["experimental_supported_tools"], json!([]));
    assert_eq!(template["visibility"], "list");
}

#[test]
#[serial]
fn proxy_chat_catalog_ignores_codex_models_cache() {
    let home = TestHome::new();
    let codex_dir = home.path().join(".codex");
    std::fs::create_dir_all(&codex_dir).expect("create codex dir");
    std::fs::write(
        codex_dir.join("models_cache.json"),
        json!({ "models": [{
            "slug": "gpt-5.5",
            "visibility": "hide",
            "shell_type": "shell_command",
            "tool_mode": "code_mode_only",
            "base_instructions": "cached"
        }] })
        .to_string(),
    )
    .expect("write models cache");

    let settings = json!({ "modelCatalog": { "models": [{ "model": "glm-5.3" }] } });
    let config_text = "model_provider = \"custom\"\nmodel = \"glm-5.3\"\n\n[model_providers.custom]\nname = \"x\"\nbase_url = \"https://open.bigmodel.cn/api/coding/paas/v4\"\nwire_api = \"chat\"\n";
    let catalog = codex_model_catalog_from_settings(
        &settings,
        config_text,
        CodexCatalogToolProfile::ProxyChat,
    )
    .expect("build catalog")
    .expect("catalog");
    let entry = &catalog["models"][0];
    assert_eq!(entry["slug"], "glm-5.3");
    assert_eq!(entry["shell_type"], "unified_exec");
    assert!(entry.get("tool_mode").is_none());
    assert_ne!(entry["base_instructions"], "cached");
}

#[test]
fn static_template_has_required_keys() {
    let template = load_codex_classic_tool_template();
    for key in &[
        "model_messages",
        "base_instructions",
        "context_window",
        "display_name",
    ] {
        assert!(
            template.get(key).is_some(),
            "static template must contain key '{key}'"
        );
    }
}

#[test]
fn resolve_catalog_finds_relative_filename() {
    let config_text = r#"model_provider = "custom"
model_catalog_json = "cc-switch-model-catalog.json"
"#;
    let base_dir = PathBuf::from("/home/user/.codex");
    let result = resolve_cc_switch_catalog_path(config_text, &base_dir);
    assert_eq!(
        result,
        Some(base_dir.join(CC_SWITCH_CODEX_MODEL_CATALOG_FILENAME)),
        "relative filename should resolve under base_dir for file I/O"
    );
}

#[test]
fn resolve_catalog_rejects_absolute_path_outside_config_dir() {
    let config_text = r#"model_catalog_json = "/tmp/secret/cc-switch-model-catalog.json"
"#;
    let base_dir = PathBuf::from("/home/user/.codex");
    let result = resolve_cc_switch_catalog_path(config_text, &base_dir);
    assert_eq!(
        result, None,
        "absolute path outside ~/.codex must not be accepted"
    );
}

#[test]
fn resolve_catalog_accepts_absolute_path_inside_config_dir() {
    let config_text = r#"model_catalog_json = "/home/user/.codex/cc-switch-model-catalog.json"
"#;
    let base_dir = PathBuf::from("/home/user/.codex");
    let result = resolve_cc_switch_catalog_path(config_text, &base_dir);
    assert_eq!(
        result,
        Some(base_dir.join(CC_SWITCH_CODEX_MODEL_CATALOG_FILENAME)),
        "absolute path inside ~/.codex should be accepted"
    );
}

#[test]
fn resolve_catalog_rejects_traversal_to_parent_directory() {
    let config_text = r#"model_catalog_json = "../cc-switch-model-catalog.json"
"#;
    let base_dir = PathBuf::from("/home/user/.codex");
    let result = resolve_cc_switch_catalog_path(config_text, &base_dir);
    assert_eq!(
        result, None,
        "relative traversal outside ~/.codex must not be accepted"
    );
}

#[test]
fn resolve_catalog_rejects_symlink_escaping_config_dir() {
    // 词法包含可被符号链接绕过：~/.codex/link -> 外部目录，
    // "link/cc-switch-model-catalog.json" 词法上在 base 内，真实读取却落到
    // base 外。canonicalize 之后的二次校验必须拒绝。
    let temp = tempfile::tempdir().expect("tempdir");
    let base_dir = temp.path().join("codex");
    let outside_dir = temp.path().join("outside");
    fs::create_dir_all(&base_dir).expect("create base");
    fs::create_dir_all(&outside_dir).expect("create outside");
    let escaped_file = outside_dir.join(CC_SWITCH_CODEX_MODEL_CATALOG_FILENAME);
    fs::write(&escaped_file, r#"{"models":[]}"#).expect("write escaped catalog");

    #[cfg(unix)]
    let linked = std::os::unix::fs::symlink(&outside_dir, base_dir.join("link"));
    #[cfg(windows)]
    let linked = std::os::windows::fs::symlink_dir(&outside_dir, base_dir.join("link"));
    if let Err(err) = linked {
        // Windows 在 \\wsl.localhost 上建不了符号链接（Incorrect function），夹具无从构造
        assert!(crate::config::is_wsl_path(temp.path()), "symlink: {err}");
        eprintln!("cannot create symlinks on WSL share ({err}); skipping");
        return;
    }

    let config_text = r#"model_catalog_json = "link/cc-switch-model-catalog.json"
"#;
    let result = resolve_cc_switch_catalog_path(config_text, &base_dir);
    let via_link = base_dir
        .join("link")
        .join(CC_SWITCH_CODEX_MODEL_CATALOG_FILENAME);
    if via_link.exists() {
        assert_eq!(
            result, None,
            "symlink escaping the config dir must be rejected after canonicalization"
        );
    } else {
        // 链接穿不过去（Windows 访问 \\wsl.localhost 时不跟随远程符号链接）：
        // 解析会原样返回词法路径，但经它读不到任何东西，同样不会越界。
        assert!(
            result.as_deref().is_none_or(|path| fs::read(path).is_err()),
            "an untraversable symlink must not lead to the outside file"
        );
    }
}

#[test]
fn resolve_catalog_accepts_real_file_inside_config_dir() {
    // 存在于 base 内的真实文件：canonical 校验通过后仍应接受
    let temp = tempfile::tempdir().expect("tempdir");
    let base_dir = temp.path().join("codex");
    fs::create_dir_all(&base_dir).expect("create base");
    let catalog_file = base_dir.join(CC_SWITCH_CODEX_MODEL_CATALOG_FILENAME);
    fs::write(&catalog_file, r#"{"models":[]}"#).expect("write catalog");

    let config_text = r#"model_catalog_json = "cc-switch-model-catalog.json"
"#;
    let result = resolve_cc_switch_catalog_path(config_text, &base_dir);
    let resolved = result.expect("real file inside config dir should be accepted");
    assert_eq!(
        resolved.file_name().and_then(|n| n.to_str()),
        Some(CC_SWITCH_CODEX_MODEL_CATALOG_FILENAME)
    );
}

#[test]
fn read_limited_string_rejects_oversized_file() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("huge.json");
    let file = std::fs::File::create(&path).expect("create");
    file.set_len(MAX_CODEX_CATALOG_BYTES + 1).expect("set_len");

    let result = read_limited_string(&path, MAX_CODEX_CATALOG_BYTES);
    assert!(
        result.is_err(),
        "file larger than MAX_CODEX_CATALOG_BYTES must be rejected"
    );
}
