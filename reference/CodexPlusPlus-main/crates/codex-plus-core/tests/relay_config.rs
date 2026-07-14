use codex_plus_core::codex_sqlite::codex_session_db_path_from_home;
use codex_plus_core::relay_config::{
    apply_pure_api_config_to_home, apply_relay_config_file_to_home, apply_relay_config_to_home,
    apply_relay_files_to_home, apply_relay_files_to_home_with_common,
    apply_relay_profile_files_to_home_with_context, apply_relay_profile_to_home_with_switch_rules,
    apply_relay_profile_to_home_with_switch_rules_and_computer_use_guard,
    backfill_relay_profile_from_home, backfill_relay_profile_from_home_with_common,
    chatgpt_auth_status_from_home, clear_relay_config_to_home,
    clear_relay_config_to_home_with_auth, delete_context_entry_from_common_config,
    extract_common_config_from_config, filter_common_config_for_selection,
    list_context_entries_from_common_config, normalize_relay_profile_for_storage,
    relay_config_status_from_home, sanitize_common_config_contents,
    set_codex_goals_feature_in_home, strip_common_config_from_config,
    sync_live_config_context_entries, upsert_context_entry_in_common_config,
};
use codex_plus_core::settings::{RelayContextSelection, RelayMode, RelayProfile, RelayProtocol};

fn write_remote_plugin_marketplace_snapshot(home: &std::path::Path) {
    let root = home.join(".tmp").join("plugins-remote");
    std::fs::create_dir_all(root.join(".agents").join("plugins")).unwrap();
    std::fs::create_dir_all(
        root.join("plugins")
            .join("product-design")
            .join(".codex-plugin"),
    )
    .unwrap();
    std::fs::write(
        root.join(".agents")
            .join("plugins")
            .join("marketplace.json"),
        r#"{"name":"openai-curated-remote","plugins":[{"name":"product-design","path":"./plugins/product-design"}]}"#,
    )
    .unwrap();
    std::fs::write(
        root.join("plugins")
            .join("product-design")
            .join(".codex-plugin")
            .join("plugin.json"),
        r#"{"name":"product-design"}"#,
    )
    .unwrap();
}

#[test]
fn codex_session_db_path_prefers_new_sqlite_directory_threads_db() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    let sqlite_dir = home.join("sqlite");
    std::fs::create_dir(&sqlite_dir).unwrap();
    std::fs::write(home.join("state_5.sqlite"), b"legacy").unwrap();

    let ignored = rusqlite::Connection::open(sqlite_dir.join("other.db")).unwrap();
    ignored
        .execute("CREATE TABLE metadata (id TEXT PRIMARY KEY)", [])
        .unwrap();
    drop(ignored);

    let selected_path = sqlite_dir.join("codex-dev.db");
    let selected = rusqlite::Connection::open(&selected_path).unwrap();
    selected
        .execute("CREATE TABLE threads (id TEXT PRIMARY KEY, cwd TEXT)", [])
        .unwrap();
    drop(selected);

    assert_eq!(codex_session_db_path_from_home(home), selected_path);
}

#[test]
fn apply_relay_config_preserves_cached_remote_plugin_marketplace() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    write_remote_plugin_marketplace_snapshot(home);

    apply_relay_files_to_home(
        home,
        r#"model = "gpt-5"
model_provider = "chatgpt"
"#,
        r#"{"auth_mode":"chatgpt"}"#,
    )
    .unwrap();

    let config = std::fs::read_to_string(home.join("config.toml")).unwrap();
    assert!(config.contains("[marketplaces.openai-curated-remote]"));
    assert!(config.contains(r#"source_type = "local""#));
    assert!(config.contains(".tmp\\plugins-remote") || config.contains(".tmp/plugins-remote"));
}

#[test]
fn codex_session_db_path_accepts_new_automation_runs_schema() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    let sqlite_dir = home.join("sqlite");
    std::fs::create_dir(&sqlite_dir).unwrap();

    let selected_path = sqlite_dir.join("codex-dev.db");
    let selected = rusqlite::Connection::open(&selected_path).unwrap();
    selected
        .execute(
            "CREATE TABLE automation_runs (thread_id TEXT PRIMARY KEY)",
            [],
        )
        .unwrap();
    drop(selected);

    assert_eq!(codex_session_db_path_from_home(home), selected_path);
}

#[test]
fn codex_session_db_path_prefers_threads_db_over_codex_dev_inbox_db() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    let sqlite_dir = home.join("sqlite");
    std::fs::create_dir(&sqlite_dir).unwrap();

    let inbox_path = sqlite_dir.join("codex-dev.db");
    let inbox = rusqlite::Connection::open(&inbox_path).unwrap();
    inbox
        .execute(
            "CREATE TABLE automation_runs (thread_id TEXT PRIMARY KEY)",
            [],
        )
        .unwrap();
    inbox
        .execute("CREATE TABLE inbox_items (id TEXT PRIMARY KEY)", [])
        .unwrap();
    drop(inbox);

    let threads_path = sqlite_dir.join("state_5.sqlite");
    let threads = rusqlite::Connection::open(&threads_path).unwrap();
    threads
        .execute(
            "CREATE TABLE threads (id TEXT PRIMARY KEY, rollout_path TEXT, cwd TEXT, title TEXT)",
            [],
        )
        .unwrap();
    drop(threads);

    assert_eq!(codex_session_db_path_from_home(home), threads_path);
}

#[test]
fn codex_session_db_path_falls_back_to_legacy_state_db() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();

    assert_eq!(
        codex_session_db_path_from_home(home),
        home.join("state_5.sqlite")
    );
}

#[test]
fn detects_chatgpt_login_from_auth_json_and_config_provider() {
    let temp = tempfile::tempdir().unwrap();
    let id_token = format!(
        "header.{}.signature",
        base64_url_no_pad(r#"{"email":"user@example.test","name":"Codex User"}"#)
    );
    std::fs::write(
        temp.path().join("auth.json"),
        format!(
            r#"{{"auth_mode":"chatgpt","tokens":{{"id_token":"{id_token}","access_token":"access-token","refresh_token":"refresh-token"}}}}"#
        ),
    )
    .unwrap();
    std::fs::write(
        temp.path().join("config.toml"),
        r#"model_provider = "chatgpt"
"#,
    )
    .unwrap();

    let status = chatgpt_auth_status_from_home(temp.path());

    assert!(status.authenticated);
    assert!(status.source.contains("auth.json"));
    assert_eq!(status.account_label.as_deref(), Some("user@example.test"));
}

#[test]
fn detects_chatgpt_login_when_config_exists_without_model_provider() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("auth.json"),
        r#"{"auth_mode":"chatgpt","tokens":{"access_token":"access-token"}}"#,
    )
    .unwrap();
    std::fs::write(temp.path().join("config.toml"), r#"model = "gpt-5""#).unwrap();

    let status = chatgpt_auth_status_from_home(temp.path());

    assert!(status.authenticated);
    assert!(status.source.contains("auth.json"));
}

#[test]
fn rejects_auth_json_tokens_without_chatgpt_auth_mode() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("auth.json"),
        r#"{"auth_mode":"apikey","tokens":{"access_token":"access-token"}}"#,
    )
    .unwrap();
    std::fs::write(
        temp.path().join("config.toml"),
        r#"model_provider = "chatgpt""#,
    )
    .unwrap();

    let status = chatgpt_auth_status_from_home(temp.path());

    assert!(!status.authenticated);
}

#[test]
fn detects_chatgpt_login_from_auth_json_without_config_toml() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("auth.json"),
        r#"{"auth_mode":"chatgpt","tokens":{"access_token":"access-token"}}"#,
    )
    .unwrap();

    let status = chatgpt_auth_status_from_home(temp.path());

    assert!(status.authenticated);
    assert!(status.source.contains("auth.json"));
}

#[test]
fn reports_relay_configured_when_required_keys_exist() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("config.toml"),
        r#"model = "gpt-5"
model_provider = "custom"
OPENAI_API_KEY = "sk-should-be-removed"
[model_providers.custom]
name = "custom"
wire_api = "responses"
requires_openai_auth = true
base_url = "http://192.168.188.245:3001/v1"
experimental_bearer_token = "sk-test-redacted"
"#,
    )
    .unwrap();

    let status = relay_config_status_from_home(temp.path());

    assert!(status.configured);
    assert!(status.requires_openai_auth);
    assert!(status.has_bearer_token);
}

#[test]
fn reports_pure_api_configured_from_auth_api_key_without_bearer_token() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("config.toml"),
        r#"model = "deepseek-v4-flash"
model_provider = "custom"

[model_providers.custom]
name = "custom"
wire_api = "responses"
requires_openai_auth = true
base_url = "http://127.0.0.1:57321/v1"
"#,
    )
    .unwrap();
    std::fs::write(
        temp.path().join("auth.json"),
        r#"{"OPENAI_API_KEY":"sk-test-redacted"}"#,
    )
    .unwrap();

    let status = relay_config_status_from_home(temp.path());

    assert!(status.configured);
    assert!(status.requires_openai_auth);
    assert!(!status.has_bearer_token);
}

#[test]
fn apply_relay_config_writes_isolated_provider_without_live_config_carryover() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("config.toml"),
        r#"model = "gpt-5"
model_catalog_json = 'C:\Users\Administrator\.codex\model-catalogs\relay-mpgm24lf.json'
model_provider = "custom1"
[model_providers.custom1]
name = "custom1"
wire_api = "responses"
requires_openai_auth = true
base_url = "http://192.168.188.245:3001/v1"
[profiles.default]
model = "gpt-5-mini"
"#,
    )
    .unwrap();

    let result = apply_relay_config_to_home(
        temp.path(),
        "https://relay.example.test/v1",
        "sk-test-redacted",
    )
    .unwrap();
    let updated = std::fs::read_to_string(temp.path().join("config.toml")).unwrap();

    assert!(result.configured);
    assert!(!updated.contains(r#"model = "gpt-5""#));
    assert!(!updated.contains("model_catalog_json"));
    assert!(!updated.contains(r#"model_provider = "custom1""#));
    assert!(!updated.contains("[model_providers.custom1]"));
    assert!(!updated.contains("[profiles.default]"));
    assert!(updated.contains(r#"model_provider = "custom""#));
    assert!(updated.contains("[model_providers.custom]"));
    assert!(updated.contains(r#"name = "custom""#));
    assert!(updated.contains(r#"wire_api = "responses""#));
    assert!(updated.contains("requires_openai_auth = true"));
    assert!(updated.contains(r#"base_url = "https://relay.example.test/v1""#));
    assert!(updated.contains(r#"experimental_bearer_token = "sk-test-redacted""#));
}

#[test]
fn apply_chat_protocol_relay_points_codex_to_local_responses_proxy() {
    let temp = tempfile::tempdir().unwrap();

    let result = codex_plus_core::relay_config::apply_relay_config_to_home_with_protocol(
        temp.path(),
        "https://chat-only.example.test/v1",
        "sk-test-redacted",
        RelayProtocol::ChatCompletions,
        57321,
    )
    .unwrap();
    let updated = std::fs::read_to_string(temp.path().join("config.toml")).unwrap();

    assert!(result.configured);
    assert!(updated.contains(r#"wire_api = "responses""#));
    assert!(updated.contains(r#"base_url = "http://127.0.0.1:57321/v1""#));
    assert!(updated.contains(r#"experimental_bearer_token = "sk-test-redacted""#));
    assert!(!updated.contains("codex_plus_chat_base_url"));
}

#[test]
fn apply_aggregate_relay_points_codex_to_local_responses_proxy_without_snapshot() {
    let temp = tempfile::tempdir().unwrap();
    let profile = RelayProfile {
        id: "agg".to_string(),
        name: "聚合供应商 1".to_string(),
        relay_mode: RelayMode::Aggregate,
        config_contents: String::new(),
        auth_contents: String::new(),
        ..RelayProfile::default()
    };

    let result = apply_relay_profile_to_home_with_switch_rules(temp.path(), &profile, "").unwrap();
    let updated = std::fs::read_to_string(temp.path().join("config.toml")).unwrap();

    assert!(result.configured);
    assert!(updated.contains(r#"wire_api = "responses""#));
    assert!(updated.contains(r#"base_url = "http://127.0.0.1:57321/v1""#));
    assert!(updated.contains(r#"experimental_bearer_token = "codex-plus-aggregate""#));
}

#[test]
fn chat_protocol_profile_keeps_upstream_base_url_separate_from_codex_proxy() {
    let temp = tempfile::tempdir().unwrap();
    let mut profile = RelayProfile {
        id: "relay-chat".to_string(),
        model: "deepseek-chat".to_string(),
        upstream_base_url: "https://api.deepseek.com".to_string(),
        api_key: "sk-test-redacted".to_string(),
        protocol: RelayProtocol::ChatCompletions,
        relay_mode: RelayMode::PureApi,
        config_contents: r#"model = "deepseek-chat"
model_provider = "custom"

[model_providers.custom]
name = "custom"
wire_api = "responses"
requires_openai_auth = true
base_url = "http://127.0.0.1:57321/v1"
"#
        .to_string(),
        auth_contents: r#"{"OPENAI_API_KEY":"sk-test-redacted"}"#.to_string(),
        ..RelayProfile::default()
    };

    normalize_relay_profile_for_storage(&mut profile).unwrap();

    assert_eq!(profile.upstream_base_url, "https://api.deepseek.com");
    assert_eq!(profile.base_url, "https://api.deepseek.com");
    assert!(!profile.config_contents.contains("codex_plus_chat_base_url"));
    assert!(
        profile
            .config_contents
            .contains(r#"base_url = "http://127.0.0.1:57321/v1""#)
    );

    apply_relay_profile_to_home_with_switch_rules(temp.path(), &profile, "").unwrap();
    let live = std::fs::read_to_string(temp.path().join("config.toml")).unwrap();
    assert!(!live.contains("codex_plus_chat_base_url"));
    assert!(live.contains(r#"base_url = "http://127.0.0.1:57321/v1""#));
}

#[test]
fn official_mix_api_profile_does_not_generate_auth_api_key() {
    let mut profile = RelayProfile {
        relay_mode: RelayMode::Official,
        official_mix_api_key: true,
        base_url: "https://relay.example/v1".to_string(),
        api_key: "sk-mix".to_string(),
        ..RelayProfile::default()
    };

    normalize_relay_profile_for_storage(&mut profile).unwrap();

    assert!(profile.auth_contents.trim().is_empty());
    assert!(
        profile
            .config_contents
            .contains(r#"wire_api = "responses""#)
    );
    assert!(
        profile
            .config_contents
            .contains("requires_openai_auth = true")
    );
    assert!(
        profile
            .config_contents
            .contains(r#"experimental_bearer_token = "sk-mix""#)
    );
}

#[test]
fn official_mix_api_profile_does_not_take_api_key_from_auth() {
    let mut profile = RelayProfile {
        relay_mode: RelayMode::Official,
        official_mix_api_key: true,
        auth_contents: r#"{"OPENAI_API_KEY":"sk-pure-api"}"#.to_string(),
        config_contents: r#"model_provider = "custom"

[model_providers.custom]
base_url = "https://relay.example/v1"
experimental_bearer_token = "sk-mix"
"#
        .to_string(),
        ..RelayProfile::default()
    };

    normalize_relay_profile_for_storage(&mut profile).unwrap();

    assert_eq!(profile.api_key, "sk-mix");
    assert!(
        profile
            .config_contents
            .contains(r#"experimental_bearer_token = "sk-mix""#)
    );
    assert!(!profile.config_contents.contains("sk-pure-api"));
}

#[test]
fn official_mix_api_profile_removes_auth_api_key_on_storage() {
    let mut profile = RelayProfile {
        relay_mode: RelayMode::Official,
        official_mix_api_key: true,
        api_key: "sk-official-mix".to_string(),
        base_url: "https://relay.example/v1".to_string(),
        auth_contents: r#"{"OPENAI_API_KEY":"sk-pure-api","auth_mode":"chatgpt","tokens":{"access_token":"official"}}"#.to_string(),
        ..RelayProfile::default()
    };

    normalize_relay_profile_for_storage(&mut profile).unwrap();

    let auth: serde_json::Value = serde_json::from_str(&profile.auth_contents).unwrap();
    assert!(auth.get("OPENAI_API_KEY").is_none());
    assert_eq!(auth["auth_mode"], "chatgpt");
    assert_eq!(auth["tokens"]["access_token"], "official");
}

#[test]
fn apply_pure_api_config_switches_auth_json_and_writes_provider_token() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("auth.json"),
        r#"{"auth_mode":"chatgpt","tokens":{"access_token":"old"}}"#,
    )
    .unwrap();
    std::fs::write(temp.path().join("config.toml"), r#"model = "gpt-5""#).unwrap();

    let result = apply_pure_api_config_to_home(
        temp.path(),
        "http://192.168.188.245:3001/v1",
        "sk-test-redacted",
    )
    .unwrap();

    let auth: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(temp.path().join("auth.json")).unwrap())
            .unwrap();
    let config = std::fs::read_to_string(temp.path().join("config.toml")).unwrap();
    assert!(result.configured);
    assert!(!config.contains(r#"model = "gpt-5""#));
    assert_eq!(
        auth,
        serde_json::json!({"OPENAI_API_KEY":"sk-test-redacted"})
    );
    assert!(config.contains(r#"model_provider = "custom""#));
    assert!(config.contains("[model_providers.custom]"));
    assert!(config.contains(r#"name = "custom""#));
    assert!(config.contains(r#"wire_api = "responses""#));
    assert!(config.contains("requires_openai_auth = true"));
    assert!(config.contains(r#"base_url = "http://192.168.188.245:3001/v1""#));
    assert!(config.contains(r#"experimental_bearer_token = "sk-test-redacted""#));
}

#[test]
fn apply_relay_files_switches_complete_config_and_auth_json() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("config.toml"), r#"model = "old""#).unwrap();
    std::fs::write(temp.path().join("auth.json"), r#"{"old":true}"#).unwrap();

    let result = apply_relay_files_to_home(
        temp.path(),
        r#"model_provider = "custom"
[model_providers.custom]
name = "custom"
wire_api = "responses"
requires_openai_auth = true
base_url = "https://relay-a.example/v1"
experimental_bearer_token = "sk-a"
"#,
        r#"{"OPENAI_API_KEY":"sk-a"}"#,
    )
    .unwrap();

    let config = std::fs::read_to_string(temp.path().join("config.toml")).unwrap();
    let auth = std::fs::read_to_string(temp.path().join("auth.json")).unwrap();

    assert!(result.configured);
    let backup_path = result.backup_path.as_ref().expect("backup path");
    assert!(backup_path.contains("codex-plus-live-"));
    assert_eq!(
        std::fs::read_to_string(std::path::Path::new(backup_path).join("config.toml")).unwrap(),
        r#"model = "old""#
    );
    assert_eq!(
        std::fs::read_to_string(std::path::Path::new(backup_path).join("auth.json")).unwrap(),
        r#"{"old":true}"#
    );
    assert!(config.contains(r#"base_url = "https://relay-a.example/v1""#));
    assert_eq!(auth, r#"{"OPENAI_API_KEY":"sk-a"}"#);
}

#[test]
fn apply_relay_files_allows_empty_isolated_auth_json() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("auth.json"), r#"{"OPENAI_API_KEY":"old"}"#).unwrap();

    let result = apply_relay_files_to_home(
        temp.path(),
        r#"model_provider = "chatgpt"
"#,
        "",
    )
    .unwrap();

    assert!(!result.configured);
    assert_eq!(
        std::fs::read_to_string(temp.path().join("auth.json")).unwrap(),
        ""
    );
}

#[test]
fn lists_codex_context_entries_from_common_config() {
    let entries = list_context_entries_from_common_config(
        r#"[mcp_servers.context7]
command = "npx"
args = ["-y", "@upstash/context7-mcp"]

[skills.writer]
enabled = true

[plugins.local]
path = "plugin.js"
"#,
    )
    .unwrap();

    assert_eq!(entries.mcp_servers[0].id, "context7");
    assert_eq!(entries.mcp_servers[0].summary, r#"command = "npx""#);
    assert_eq!(entries.skills[0].id, "writer");
    assert_eq!(entries.plugins[0].id, "local");
}

#[test]
fn lists_codex_context_entries_with_parent_mcp_table() {
    let entries = list_context_entries_from_common_config(
        r#"[mcp_servers]

[mcp_servers.ida-pro-mcp]
type = "stdio"
command = 'C:\Users\Administrator\AppData\Local\Programs\Python\Python313\python.exe'
args = ['C:\Users\Administrator\AppData\Local\Programs\Python\Python313\Lib\site-packages\ida_pro_mcp\server.py']
disabled = false
timeout = 1800
"#,
    )
    .unwrap();

    assert_eq!(entries.mcp_servers.len(), 1);
    assert_eq!(entries.mcp_servers[0].id, "ida-pro-mcp");
    assert!(entries.mcp_servers[0].enabled);
    assert!(
        entries.mcp_servers[0]
            .toml_body
            .contains("disabled = false")
    );
}

#[test]
fn lists_codex_context_entries_with_enabled_state() {
    let entries = list_context_entries_from_common_config(
        r#"[mcp_servers.enabled_mcp]
disabled = false

[mcp_servers.disabled_mcp]
disabled = true

[plugins.enabled_plugin]
enabled = true

[plugins.disabled_plugin]
enabled = false
"#,
    )
    .unwrap();

    assert!(entries.mcp_servers[0].enabled);
    assert!(!entries.mcp_servers[1].enabled);
    assert!(entries.plugins[0].enabled);
    assert!(!entries.plugins[1].enabled);
}

#[test]
fn sync_live_config_context_entries_toggles_live_context_by_enabled_state() {
    let live = r#"model = "gpt-5"

[mcp_servers]

[mcp_servers.ida-pro-mcp]
command = "python"
enabled = true

[plugins."browser@openai-bundled"]
enabled = true
"#;
    let disabled = r#"[mcp_servers.ida-pro-mcp]
command = "python"
enabled = false

[plugins."browser@openai-bundled"]
enabled = true
"#;

    let updated = sync_live_config_context_entries(live, disabled).unwrap();

    assert!(updated.contains(r#"model = "gpt-5""#));
    assert!(!updated.contains("[mcp_servers.ida-pro-mcp]"));
    assert!(updated.contains("[plugins.\"browser@openai-bundled\"]"));

    let enabled = r#"[mcp_servers.ida-pro-mcp]
command = "python"
enabled = true
"#;

    let updated = sync_live_config_context_entries(&updated, enabled).unwrap();

    assert!(updated.contains("[mcp_servers.ida-pro-mcp]"));
    assert!(updated.contains(r#"command = "python""#));
    assert!(updated.contains("[plugins.\"browser@openai-bundled\"]"));
}

#[test]
fn upserts_and_deletes_context_entry_in_common_config() {
    let common = upsert_context_entry_in_common_config(
        "",
        "mcp",
        "context7",
        r#"command = "npx"
args = ["-y", "@upstash/context7-mcp"]
"#,
    )
    .unwrap();

    assert!(common.contains("[mcp_servers.context7]"));
    assert!(common.contains(r#"command = "npx""#));

    let updated =
        upsert_context_entry_in_common_config(&common, "mcp", "context7", r#"command = "bunx""#)
            .unwrap();

    assert!(updated.contains(r#"command = "bunx""#));
    assert!(!updated.contains(r#"command = "npx""#));

    let deleted = delete_context_entry_from_common_config(&updated, "mcp", "context7").unwrap();
    assert!(!deleted.contains("[mcp_servers.context7]"));
}

#[test]
fn upserts_context_entry_tolerates_duplicate_existing_context_tables() {
    let common = r#"[plugins."browser@openai-bundled"]
enabled = true

[plugins."browser@openai-bundled"]
enabled = true
"#;

    let updated = upsert_context_entry_in_common_config(
        common,
        "plugin",
        "browser@openai-bundled",
        "enabled = false",
    )
    .unwrap();

    assert_eq!(
        updated
            .matches("[plugins.\"browser@openai-bundled\"]")
            .count(),
        1
    );
    assert!(updated.contains("enabled = false"));
}

#[test]
fn global_common_config_filters_context_by_supplier_selection() {
    let filtered = filter_common_config_for_selection(
        r#"disable_response_storage = true

[features]
goals = true

[mcp_servers.context7]
command = "npx"

[mcp_servers.memory]
command = "memory"

[skills.writer]
enabled = true

[plugins.local]
path = "plugin.js"
"#,
        &RelayContextSelection {
            mcp_servers: vec!["memory".to_string()],
            skills: vec![],
            plugins: vec!["local".to_string()],
        },
    )
    .unwrap();

    assert!(filtered.contains("disable_response_storage = true"));
    assert!(filtered.contains("[features]"));
    assert!(filtered.contains("goals = true"));
    assert!(!filtered.contains("[mcp_servers.context7]"));
    assert!(filtered.contains("[mcp_servers.memory]"));
    assert!(!filtered.contains("[skills.writer]"));
    assert!(filtered.contains("[plugins.local]"));
}

#[test]
fn extracts_codex_common_config_without_provider_fields() {
    let extracted = extract_common_config_from_config(
        r#"model = "gpt-5"
model_provider = "custom"
base_url = "https://root-provider.example/v1"
model_catalog_json = "C:\\Users\\Administrator\\.codex\\model-catalogs\\relay-a.json"
OPENAI_API_KEY = "sk-root"

[model_providers.custom]
name = "custom"
base_url = "https://relay.example/v1"

[mcp_servers.context7]
command = "npx"
args = ["-y", "@upstash/context7-mcp"]

[skills.writer]
enabled = true

[plugins.local]
path = "C:\\Tools\\plugin"
"#,
    )
    .unwrap();

    assert!(extracted.contains("[mcp_servers.context7]"));
    assert!(extracted.contains("[skills.writer]"));
    assert!(extracted.contains("[plugins.local]"));
    assert!(!extracted.contains("model_provider"));
    assert!(!extracted.contains("model ="));
    assert!(!extracted.contains("model_catalog_json"));
    assert!(!extracted.contains("base_url = \"https://root-provider.example/v1\""));
    assert!(extracted.contains("OPENAI_API_KEY = \"sk-root\""));
    assert!(!extracted.contains("[model_providers"));
}

#[test]
fn sanitizes_model_catalog_json_from_common_config() {
    let sanitized = sanitize_common_config_contents(
        r#"model_catalog_json = "C:\\Users\\Administrator\\.codex\\model-catalogs\\relay-a.json"
model_reasoning_effort = "high"

[features]
goals = true
"#,
    );

    assert!(!sanitized.contains("model_catalog_json"));
    assert!(sanitized.contains("model_reasoning_effort = \"high\""));
    assert!(sanitized.contains("[features]"));
    assert!(sanitized.contains("goals = true"));
}

#[test]
fn sanitizes_model_catalog_json_from_invalid_common_config() {
    let sanitized = sanitize_common_config_contents(
        r#"model_catalog_json = "C:\\Users\\Administrator\\.codex\\model-catalogs\\relay-a.json"
model_catalog_json = 'C:\Users\Administrator\.codex\model-catalogs\relay-b.json'
model_reasoning_effort = "high"
"#,
    );

    assert!(!sanitized.contains("model_catalog_json"));
    assert!(sanitized.contains("model_reasoning_effort = \"high\""));
}

#[test]
fn strips_common_config_from_provider_config_only_when_values_match() {
    let common = r#"[mcp_servers.context7]
command = "npx"
args = ["-y", "@upstash/context7-mcp"]

[skills.writer]
enabled = true
"#;
    let stripped = strip_common_config_from_config(
        r#"model = "gpt-5"
model_provider = "custom"

[model_providers.custom]
base_url = "https://relay.example/v1"

[mcp_servers.context7]
command = "npx"
args = ["-y", "@upstash/context7-mcp"]

[skills.writer]
enabled = false
"#,
        common,
    )
    .unwrap();

    assert!(stripped.contains(r#"model = "gpt-5""#));
    assert!(stripped.contains("[model_providers.custom]"));
    assert!(!stripped.contains("[mcp_servers.context7]"));
    assert!(stripped.contains("[skills.writer]"));
    assert!(stripped.contains("enabled = false"));
}

#[test]
fn apply_relay_files_with_common_preserves_mcp_skills_and_plugins() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("config.toml"),
        r#"model = "old"
[mcp_servers.old]
command = "old"
"#,
    )
    .unwrap();

    apply_relay_files_to_home_with_common(
        temp.path(),
        r#"model = "gpt-5"
model_provider = "custom"
model_catalog_json = 'C:\Users\Administrator\.codex\model-catalogs\relay-mpgm24lf.json'
[model_providers.custom]
name = "custom"
wire_api = "responses"
requires_openai_auth = true
base_url = "https://relay.example/v1"
experimental_bearer_token = "sk-new"
"#,
        r#"{"OPENAI_API_KEY":"sk-new"}"#,
        r#"[mcp_servers.context7]
command = "npx"

[skills.writer]
enabled = true

[plugins.local]
path = "plugin.js"
"#,
    )
    .unwrap();

    let config = std::fs::read_to_string(temp.path().join("config.toml")).unwrap();
    assert!(config.contains(r#"model = "gpt-5""#));
    assert!(config.contains(r#"base_url = "https://relay.example/v1""#));
    assert!(config.contains("[mcp_servers.context7]"));
    assert!(config.contains("[skills.writer]"));
    assert!(config.contains("[plugins.local]"));
}

#[test]
fn apply_relay_files_with_context_selection_writes_only_selected_global_context() {
    let temp = tempfile::tempdir().unwrap();
    let selection = RelayContextSelection {
        mcp_servers: vec!["memory".to_string()],
        skills: vec![],
        plugins: vec!["local".to_string()],
    };

    codex_plus_core::relay_config::apply_relay_files_to_home_with_context(
        temp.path(),
        r#"model_provider = "custom"
[model_providers.custom]
name = "custom"
wire_api = "responses"
requires_openai_auth = true
base_url = "https://relay.example/v1"
experimental_bearer_token = "sk-new"
"#,
        r#"{"OPENAI_API_KEY":"sk-new"}"#,
        r#"[mcp_servers.context7]
command = "npx"

[mcp_servers.memory]
command = "memory"

[skills.writer]
enabled = true

[plugins.local]
path = "plugin.js"
"#,
        &selection,
        "200000",
        "160000",
    )
    .unwrap();

    let config = std::fs::read_to_string(temp.path().join("config.toml")).unwrap();
    assert!(config.contains("[mcp_servers.memory]"));
    assert!(!config.contains("[mcp_servers.context7]"));
    assert!(!config.contains("[skills.writer]"));
    assert!(config.contains("[plugins.local]"));
    assert!(config.contains("model_context_window = 200000"));
    assert!(config.contains("model_auto_compact_token_limit = 160000"));
}

#[test]
fn apply_relay_files_with_context_skips_disabled_global_context() {
    let temp = tempfile::tempdir().unwrap();
    let selection = RelayContextSelection {
        mcp_servers: vec!["enabled_one".to_string()],
        skills: vec!["disabled_skill".to_string()],
        plugins: vec!["disabled_one".to_string(), "enabled_two".to_string()],
    };

    codex_plus_core::relay_config::apply_relay_files_to_home_with_context(
        temp.path(),
        r#"model_provider = "custom"
[model_providers.custom]
name = "custom"
wire_api = "responses"
requires_openai_auth = true
base_url = "https://relay.example/v1"
experimental_bearer_token = "sk-new"
"#,
        r#"{"OPENAI_API_KEY":"sk-new"}"#,
        r#"[mcp_servers.enabled_one]
command = "npx"

[plugins.disabled_one]
enabled = false

[skills.disabled_skill]
enabled = false

[plugins.enabled_two]
enabled = true
"#,
        &selection,
        "",
        "",
    )
    .unwrap();

    let config = std::fs::read_to_string(temp.path().join("config.toml")).unwrap();
    assert!(config.contains("[mcp_servers.enabled_one]"));
    assert!(config.contains("[plugins.enabled_two]"));
    assert!(!config.contains("[plugins.disabled_one]"));
    assert!(!config.contains("[skills.disabled_skill]"));
}

#[test]
fn apply_relay_profile_does_not_write_model_catalog_json_for_selected_models() {
    let temp = tempfile::tempdir().unwrap();
    let profile = RelayProfile {
        id: "relay-a".to_string(),
        name: "Relay A".to_string(),
        model: "qwen3-coder".to_string(),
        relay_mode: RelayMode::PureApi,
        config_contents: r#"model = "qwen3-coder"
model_provider = "custom"

[model_providers.custom]
name = "custom"
wire_api = "responses"
requires_openai_auth = true
base_url = "https://relay.example/v1"
experimental_bearer_token = "sk-new"
"#
        .to_string(),
        auth_contents: r#"{"OPENAI_API_KEY":"sk-new"}"#.to_string(),
        model_insert_mode: Default::default(),
        model_list: "deepseek-coder\nqwen3-coder".to_string(),
        context_window: "200000".to_string(),
        auto_compact_limit: "160000".to_string(),
        ..RelayProfile::default()
    };

    apply_relay_profile_files_to_home_with_context(temp.path(), &profile, "").unwrap();

    let config = std::fs::read_to_string(temp.path().join("config.toml")).unwrap();
    assert!(!config.contains("model_catalog_json"));
    assert!(config.contains("model_context_window = 200000"));
    assert!(config.contains("model_auto_compact_token_limit = 160000"));
    assert!(!temp.path().join("model-catalogs").exists());
}

#[test]
fn apply_relay_profile_preserves_user_model_catalog_json() {
    let temp = tempfile::tempdir().unwrap();
    let profile = RelayProfile {
        id: "relay-a".to_string(),
        relay_mode: RelayMode::PureApi,
        config_contents: r#"model = "qwen3-coder"
model_catalog_json = "C:\\old\\catalog.json"
model_provider = "custom"

[model_providers.custom]
name = "custom"
wire_api = "responses"
requires_openai_auth = true
base_url = "https://relay.example/v1"
experimental_bearer_token = "sk-new"
"#
        .to_string(),
        auth_contents: r#"{"OPENAI_API_KEY":"sk-new"}"#.to_string(),
        model_insert_mode: Default::default(),
        model_list: "deepseek-coder".to_string(),
        ..RelayProfile::default()
    };

    apply_relay_profile_files_to_home_with_context(temp.path(), &profile, "").unwrap();

    let config = std::fs::read_to_string(temp.path().join("config.toml")).unwrap();
    assert!(config.contains(r#"model_catalog_json = "C:\\old\\catalog.json""#));
    assert!(
        !temp
            .path()
            .join("model-catalogs")
            .join("relay-a.json")
            .exists()
    );
}

#[test]
fn apply_relay_profile_skips_common_config_when_disabled() {
    let temp = tempfile::tempdir().unwrap();
    let profile = RelayProfile {
        id: "relay-a".to_string(),
        relay_mode: RelayMode::PureApi,
        use_common_config: false,
        config_contents: r#"model = "qwen3-coder"
model_provider = "custom"

[model_providers.custom]
name = "custom"
wire_api = "responses"
requires_openai_auth = true
base_url = "https://relay.example/v1"
experimental_bearer_token = "sk-new"
"#
        .to_string(),
        auth_contents: r#"{"OPENAI_API_KEY":"sk-new"}"#.to_string(),
        context_selection: RelayContextSelection {
            mcp_servers: vec!["context7".to_string()],
            skills: vec![],
            plugins: vec![],
        },
        ..RelayProfile::default()
    };

    apply_relay_profile_files_to_home_with_context(
        temp.path(),
        &profile,
        r#"disable_response_storage = true

[mcp_servers.context7]
command = "npx"
"#,
    )
    .unwrap();

    let config = std::fs::read_to_string(temp.path().join("config.toml")).unwrap();
    assert!(!config.contains("disable_response_storage = true"));
    assert!(!config.contains("[mcp_servers.context7]"));
}

#[test]
fn set_codex_goals_feature_writes_and_removes_feature_flag() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("config.toml"),
        r#"model = "gpt-5.4-mini"

[features]
other = true
"#,
    )
    .unwrap();

    set_codex_goals_feature_in_home(temp.path(), true).unwrap();
    let config = std::fs::read_to_string(temp.path().join("config.toml")).unwrap();
    assert!(config.contains("[features]"));
    assert!(config.contains("goals = true"));
    assert!(config.contains("other = true"));

    set_codex_goals_feature_in_home(temp.path(), false).unwrap();
    let config = std::fs::read_to_string(temp.path().join("config.toml")).unwrap();
    assert!(config.contains("[features]"));
    assert!(config.contains("other = true"));
    assert!(!config.contains("goals = true"));
}

#[test]
fn set_codex_goals_feature_tolerates_invalid_existing_toml() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("config.toml"),
        r#"model = "gpt-5"

[marketplaces.openai-bundled]
last_updated = "2026-05-25T11:52:46Z"

[marketplaces.openai-bundled]
last_updated = "2026-05-25T11:52:46Z"
"#,
    )
    .unwrap();

    set_codex_goals_feature_in_home(temp.path(), true).unwrap();

    let config = std::fs::read_to_string(temp.path().join("config.toml")).unwrap();
    assert!(config.contains("[features]"));
    assert!(config.contains("goals = true"));
}

#[test]
fn apply_relay_files_with_context_rejects_invalid_context_token_values() {
    let temp = tempfile::tempdir().unwrap();
    let selection = RelayContextSelection::default();

    let error = codex_plus_core::relay_config::apply_relay_files_to_home_with_context(
        temp.path(),
        r#"model_provider = "custom""#,
        r#"{"OPENAI_API_KEY":"sk-new"}"#,
        "",
        &selection,
        "abc",
        "",
    )
    .unwrap_err();

    assert!(error.to_string().contains("上下文大小"));
}

#[test]
fn apply_relay_files_uses_custom_provider_id_and_updates_profile_refs() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("config.toml"),
        r#"model_provider = "stable-live"
[model_providers.stable-live]
name = "stable-live"
base_url = "https://old.example/v1"
"#,
    )
    .unwrap();

    apply_relay_files_to_home(
        temp.path(),
        r#"model_provider = "custom"
[model_providers.custom]
name = "custom"
wire_api = "responses"
requires_openai_auth = true
base_url = "https://new.example/v1"
experimental_bearer_token = "sk-new"

[profiles.default]
model_provider = "custom"
"#,
        r#"{"OPENAI_API_KEY":"sk-new"}"#,
    )
    .unwrap();

    let config = std::fs::read_to_string(temp.path().join("config.toml")).unwrap();
    assert!(config.contains(r#"model_provider = "custom""#));
    assert!(config.contains("[model_providers.custom]"));
    assert!(config.contains(r#"base_url = "https://new.example/v1""#));
    assert!(config.contains("[profiles.default]"));
    assert!(config.contains(r#"model_provider = "custom""#));
    assert!(!config.contains("[model_providers.stable-live]"));
}

#[test]
fn backfill_relay_profile_restores_template_provider_id_from_stable_live_config() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("config.toml"),
        r#"model_provider = "custom"

[model_providers.custom]
name = "custom"
wire_api = "responses"
requires_openai_auth = true
base_url = "https://new.example/v1"
experimental_bearer_token = "sk-new"

[profiles.default]
model_provider = "custom"
"#,
    )
    .unwrap();
    let mut profile = RelayProfile {
        config_contents: r#"model_provider = "vendor_alpha"

[model_providers.vendor_alpha]
name = "vendor_alpha"
wire_api = "responses"
requires_openai_auth = true
base_url = "https://old.example/v1"

[profiles.default]
model_provider = "vendor_alpha"
"#
        .to_string(),
        auth_contents: r#"{"OPENAI_API_KEY":"old"}"#.to_string(),
        ..RelayProfile::default()
    };
    let mut common = String::new();

    backfill_relay_profile_from_home_with_common(temp.path(), &mut profile, &mut common).unwrap();

    assert!(
        profile
            .config_contents
            .contains(r#"model_provider = "vendor_alpha""#)
    );
    assert!(
        profile
            .config_contents
            .contains("[model_providers.vendor_alpha]")
    );
    assert!(!profile.config_contents.contains("[model_providers.custom]"));
    assert!(
        profile
            .config_contents
            .contains(r#"model_provider = "vendor_alpha""#)
    );
    let auth: serde_json::Value = serde_json::from_str(&profile.auth_contents).unwrap();
    assert_eq!(auth["OPENAI_API_KEY"], "sk-new");
}

#[test]
fn apply_relay_files_rejects_invalid_toml_before_auth_write() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("config.toml"), "model = \"old\"\n").unwrap();
    std::fs::write(temp.path().join("auth.json"), r#"{"old":true}"#).unwrap();

    let error =
        apply_relay_files_to_home(temp.path(), "model = [", r#"{"OPENAI_API_KEY":"sk-new"}"#)
            .unwrap_err();

    assert!(error.to_string().contains("TOML"));
    assert_eq!(
        std::fs::read_to_string(temp.path().join("config.toml")).unwrap(),
        "model = \"old\"\n"
    );
    assert_eq!(
        std::fs::read_to_string(temp.path().join("auth.json")).unwrap(),
        r#"{"old":true}"#
    );
}

#[test]
fn apply_relay_files_rejects_invalid_auth_json_before_config_write() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("config.toml"), "model = \"old\"\n").unwrap();
    std::fs::write(temp.path().join("auth.json"), r#"{"old":true}"#).unwrap();

    let error = apply_relay_files_to_home(
        temp.path(),
        r#"model_provider = "custom"
[model_providers.custom]
name = "custom"
wire_api = "responses"
requires_openai_auth = true
base_url = "https://relay.example/v1"
experimental_bearer_token = "sk-new"
"#,
        "{",
    )
    .unwrap_err();

    assert!(error.to_string().contains("JSON"));
    assert_eq!(
        std::fs::read_to_string(temp.path().join("config.toml")).unwrap(),
        "model = \"old\"\n"
    );
    assert_eq!(
        std::fs::read_to_string(temp.path().join("auth.json")).unwrap(),
        r#"{"old":true}"#
    );
}

#[test]
fn apply_relay_config_file_switches_config_without_touching_auth_json() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    std::fs::write(
        home.join("config.toml"),
        "model_provider = \"CodexPlusPlus\"\nbase_url = \"old\"\n",
    )
    .unwrap();
    std::fs::write(home.join("auth.json"), "{\"auth_mode\":\"chatgpt\"}\n").unwrap();

    let result = apply_relay_config_file_to_home(
        home,
        "model_provider = \"custom\"\n\n[model_providers.custom]\nname = \"custom\"\nwire_api = \"responses\"\nrequires_openai_auth = true\nbase_url = \"http://127.0.0.1:57321/v1\"\nexperimental_bearer_token = \"sk-new\"\n",
    )
    .unwrap();

    assert!(result.configured);
    assert!(
        std::fs::read_to_string(home.join("config.toml"))
            .unwrap()
            .contains("http://127.0.0.1:57321/v1")
    );
    assert_eq!(
        std::fs::read_to_string(home.join("auth.json")).unwrap(),
        "{\"auth_mode\":\"chatgpt\"}\n"
    );
}

#[test]
fn apply_relay_config_file_accepts_utf8_bom_config() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    std::fs::write(home.join("config.toml"), "\u{feff}model = \"old\"\n").unwrap();
    std::fs::write(home.join("auth.json"), "{\"auth_mode\":\"chatgpt\"}\n").unwrap();

    let result = apply_relay_config_file_to_home(
        home,
        "\u{feff}model_provider = \"custom\"\n\n[model_providers.custom]\nname = \"custom\"\nwire_api = \"responses\"\nrequires_openai_auth = true\nbase_url = \"http://127.0.0.1:57321/v1\"\nexperimental_bearer_token = \"sk-new\"\n",
    )
    .unwrap();

    let config = std::fs::read_to_string(home.join("config.toml")).unwrap();
    assert!(result.configured);
    assert!(config.contains(r#"model_provider = "custom""#));
    assert!(config.contains("http://127.0.0.1:57321/v1"));
}

#[test]
fn apply_relay_config_does_not_carry_profiles_from_live_config() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("config.toml"),
        r#"model = "gpt-5"
[profiles.default]
model = "gpt-5-mini"
"#,
    )
    .unwrap();

    apply_relay_config_to_home(
        temp.path(),
        "https://relay.example.test/v1",
        "sk-test-redacted",
    )
    .unwrap();
    let updated = std::fs::read_to_string(temp.path().join("config.toml")).unwrap();
    let provider_index = updated.find(r#"model_provider = "custom""#).unwrap();
    let codexpp_index = updated.find("[model_providers.custom]").unwrap();

    assert!(provider_index < codexpp_index);
    assert!(!updated.contains("[profiles.default]"));
    assert!(!updated.contains(r#"model = "gpt-5""#));
}

#[test]
fn apply_relay_config_removes_legacy_codexpp_provider_table() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("config.toml"),
        r#"model_provider = "CodexPP"
[model_providers.CodexPP]
name = "CodexPP"
base_url = "https://old.example.test/v1"
"#,
    )
    .unwrap();

    apply_relay_config_to_home(
        temp.path(),
        "https://relay.example.test/v1",
        "sk-test-redacted",
    )
    .unwrap();
    let updated = std::fs::read_to_string(temp.path().join("config.toml")).unwrap();

    assert!(updated.contains(r#"model_provider = "custom""#));
    assert!(updated.contains("[model_providers.custom]"));
    assert!(!updated.contains("[model_providers.CodexPP]"));
}

#[test]
fn clear_relay_config_removes_model_provider_and_preserves_other_config() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("config.toml"),
        r#"model = "gpt-5"
model_provider = "custom"
[model_providers.custom]
name = "custom"
wire_api = "responses"
requires_openai_auth = true
base_url = "https://relay.example.test/v1"
experimental_bearer_token = "sk-test-redacted"

[model_providers.CodexPP]
name = "CodexPP"
base_url = "https://old.example.test/v1"

[model_providers.custom1]
name = "custom1"
wire_api = "responses"
base_url = "https://keep.example.test/v1"

[profiles.default]
model = "gpt-5-mini"
"#,
    )
    .unwrap();

    let result = clear_relay_config_to_home(temp.path()).unwrap();
    let updated = std::fs::read_to_string(temp.path().join("config.toml")).unwrap();

    assert!(!result.configured);
    assert!(
        result
            .backup_path
            .as_ref()
            .is_some_and(|path| path.contains("codex-plus-live-"))
    );
    assert!(updated.contains(r#"model = "gpt-5""#));
    assert!(!updated.contains("model_provider ="));
    assert!(!updated.contains("model_catalog_json"));
    assert!(!updated.contains("OPENAI_API_KEY"));
    assert!(!updated.contains("[model_providers.custom]"));
    assert!(!updated.contains("[model_providers.CodexPP]"));
    assert!(!updated.contains("[model_providers]\n"));
    assert!(!updated.contains("experimental_bearer_token"));
    assert!(updated.contains("[model_providers.custom1]"));
    assert!(updated.contains(r#"base_url = "https://keep.example.test/v1""#));
    assert!(updated.contains("[profiles.default]"));
}

#[test]
fn clear_relay_config_removes_pure_api_auth_json_key_and_preserves_other_auth_fields() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("auth.json"),
        r#"{"OPENAI_API_KEY":"sk-test-redacted","auth_mode":"chatgpt","tokens":{"access_token":"keep"}}"#,
    )
    .unwrap();
    std::fs::write(
        temp.path().join("config.toml"),
        r#"model = "gpt-5"
model_provider = "custom"
[model_providers.custom]
name = "custom"
wire_api = "responses"
requires_openai_auth = true
base_url = "https://relay.example.test/v1"
experimental_bearer_token = "sk-test-redacted"
"#,
    )
    .unwrap();

    clear_relay_config_to_home(temp.path()).unwrap();

    let auth: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(temp.path().join("auth.json")).unwrap())
            .unwrap();
    let auth_object = auth.as_object().unwrap();
    assert!(!auth_object.contains_key("OPENAI_API_KEY"));
    assert_eq!(auth["auth_mode"], "chatgpt");
    assert_eq!(auth["tokens"]["access_token"], "keep");
}

#[test]
fn clear_relay_config_removes_openai_api_key_when_auth_json_only_contains_pure_api_key() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("auth.json"),
        r#"{"OPENAI_API_KEY":"sk-test-redacted"}"#,
    )
    .unwrap();

    clear_relay_config_to_home(temp.path()).unwrap();

    let auth: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(temp.path().join("auth.json")).unwrap())
            .unwrap();
    let auth_object = auth.as_object().unwrap();
    assert!(!auth_object.contains_key("OPENAI_API_KEY"));
    assert!(auth_object.is_empty());
}

#[test]
fn clear_relay_config_with_auth_restores_official_profile_auth_json() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("auth.json"),
        r#"{"OPENAI_API_KEY":"sk-relay"}"#,
    )
    .unwrap();
    std::fs::write(
        temp.path().join("config.toml"),
        r#"model_provider = "custom"
[model_providers.custom]
base_url = "https://relay.example.test/v1"
experimental_bearer_token = "sk-relay"
"#,
    )
    .unwrap();

    clear_relay_config_to_home_with_auth(
        temp.path(),
        Some(r#"{"auth_mode":"chatgpt","tokens":{"access_token":"official-edited"}}"#),
    )
    .unwrap();

    let auth: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(temp.path().join("auth.json")).unwrap())
            .unwrap();
    assert_eq!(auth["auth_mode"], "chatgpt");
    assert_eq!(auth["tokens"]["access_token"], "official-edited");
    assert!(auth.get("OPENAI_API_KEY").is_none());
}

#[test]
fn backfill_relay_profile_reads_live_files_and_model() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("config.toml"),
        "model = \"gpt-5\"\nmodel_provider = \"live\"\n",
    )
    .unwrap();
    std::fs::write(
        temp.path().join("auth.json"),
        r#"{"OPENAI_API_KEY":"sk-live"}"#,
    )
    .unwrap();
    let mut profile = RelayProfile::default();

    backfill_relay_profile_from_home(temp.path(), &mut profile).unwrap();

    assert_eq!(profile.model, "gpt-5");
    assert!(
        profile
            .config_contents
            .contains(r#"model_provider = "live""#)
    );
    assert_eq!(profile.auth_contents, r#"{"OPENAI_API_KEY":"sk-live"}"#);
}

#[test]
fn backfill_relay_profile_reads_live_context_limits() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("config.toml"),
        r#"model = "mimo-v2.5-pro"
model_provider = "custom"
model_context_window = 1000000
model_auto_compact_token_limit = 900000

[model_providers.custom]
base_url = "http://127.0.0.1:57321/v1"
"#,
    )
    .unwrap();
    let mut profile = RelayProfile::default();

    backfill_relay_profile_from_home(temp.path(), &mut profile).unwrap();

    assert_eq!(profile.context_window, "1000000");
    assert_eq!(profile.auto_compact_limit, "900000");
    assert!(
        profile
            .config_contents
            .contains("model_context_window = 1000000")
    );
    assert!(
        profile
            .config_contents
            .contains("model_auto_compact_token_limit = 900000")
    );
}

#[test]
fn backfill_relay_profile_with_common_strips_common_config_for_switching() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("config.toml"),
        r#"model = "gpt-5"
model_provider = "live"
[model_providers.live]
base_url = "https://relay.example/v1"

[mcp_servers.context7]
command = "npx"
"#,
    )
    .unwrap();
    std::fs::write(
        temp.path().join("auth.json"),
        r#"{"OPENAI_API_KEY":"sk-live"}"#,
    )
    .unwrap();
    let mut profile = RelayProfile::default();
    let mut common = r#"[mcp_servers.context7]
command = "npx"
"#
    .to_string();

    backfill_relay_profile_from_home_with_common(temp.path(), &mut profile, &mut common).unwrap();

    assert_eq!(profile.model, "gpt-5");
    assert!(!profile.config_contents.contains("[mcp_servers.context7]"));
    assert!(
        profile
            .config_contents
            .contains(r#"model_provider = "live""#)
    );
    assert_eq!(profile.auth_contents, r#"{"OPENAI_API_KEY":"sk-live"}"#);
}

#[test]
fn backfill_relay_profile_with_common_reads_live_context_limits() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("config.toml"),
        r#"model = "mimo-v2.5-pro"
model_provider = "custom"
model_context_window = 1000000
model_auto_compact_token_limit = 900000

[model_providers.custom]
base_url = "http://127.0.0.1:57321/v1"

[mcp_servers.context7]
command = "npx"
"#,
    )
    .unwrap();
    let mut profile = RelayProfile::default();
    let mut common = r#"[mcp_servers.context7]
command = "npx"
"#
    .to_string();

    backfill_relay_profile_from_home_with_common(temp.path(), &mut profile, &mut common).unwrap();
    apply_relay_profile_files_to_home_with_context(temp.path(), &profile, &common).unwrap();

    let config = std::fs::read_to_string(temp.path().join("config.toml")).unwrap();
    assert_eq!(profile.context_window, "1000000");
    assert_eq!(profile.auto_compact_limit, "900000");
    assert!(config.contains("model_context_window = 1000000"));
    assert!(config.contains("model_auto_compact_token_limit = 900000"));
}

#[test]
fn backfill_relay_profile_with_common_tolerates_duplicate_live_toml() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("config.toml"),
        r#"model = "gpt-5.5"
model_reasoning_effort = "high"
model_provider = "aaa"
model_reasoning_effort = "high"

[model_providers.aaa]
base_url = "https://relay.example/v1"
experimental_bearer_token = "sk-live-token"

[marketplaces.openai-bundled]
last_updated = "new"

[marketplaces.openai-bundled]
last_updated = "old"

[plugins."superpowers@openai-curated"]
enabled = true
"#,
    )
    .unwrap();
    let mut profile = RelayProfile::default();
    let mut common = r#"[plugins."superpowers@openai-curated"]
enabled = true
"#
    .to_string();

    backfill_relay_profile_from_home_with_common(temp.path(), &mut profile, &mut common).unwrap();

    assert_eq!(profile.model, "gpt-5.5");
    assert!(
        profile
            .config_contents
            .contains(r#"model_reasoning_effort = "high""#)
    );
    assert_eq!(
        profile
            .config_contents
            .matches("model_reasoning_effort")
            .count(),
        1
    );
    assert_eq!(
        profile
            .config_contents
            .matches("[marketplaces.openai-bundled]")
            .count(),
        1
    );
    assert!(
        !profile
            .config_contents
            .contains("[plugins.\"superpowers@openai-curated\"]")
    );
    let auth: serde_json::Value = serde_json::from_str(&profile.auth_contents).unwrap();
    assert_eq!(auth["OPENAI_API_KEY"], "sk-live-token");
}

#[test]
fn backfill_relay_profile_with_common_lifts_bearer_token_to_auth() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("config.toml"),
        r#"model = "gpt-5"
model_provider = "live"
[model_providers.live]
base_url = "https://relay.example/v1"
experimental_bearer_token = "sk-live-token"
"#,
    )
    .unwrap();
    let mut profile = RelayProfile::default();
    let mut common = String::new();

    backfill_relay_profile_from_home_with_common(temp.path(), &mut profile, &mut common).unwrap();

    assert!(
        !profile
            .config_contents
            .contains("experimental_bearer_token")
    );
    let auth: serde_json::Value = serde_json::from_str(&profile.auth_contents).unwrap();
    assert_eq!(auth["OPENAI_API_KEY"], "sk-live-token");
}

#[test]
fn backfill_relay_profile_prefers_live_auth_over_provider_token() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("config.toml"),
        r#"model_provider = "custom"

[model_providers.custom]
name = "custom"
wire_api = "responses"
requires_openai_auth = true
base_url = "https://relay.example/v1"
experimental_bearer_token = "sk-old"
"#,
    )
    .unwrap();
    std::fs::write(
        temp.path().join("auth.json"),
        r#"{"OPENAI_API_KEY":"sk-edited"}"#,
    )
    .unwrap();
    let mut profile = RelayProfile {
        relay_mode: RelayMode::PureApi,
        auth_contents: r#"{"OPENAI_API_KEY":"sk-old"}"#.to_string(),
        ..RelayProfile::default()
    };
    let mut common = String::new();

    backfill_relay_profile_from_home_with_common(temp.path(), &mut profile, &mut common).unwrap();

    let auth: serde_json::Value = serde_json::from_str(&profile.auth_contents).unwrap();
    assert_eq!(auth["OPENAI_API_KEY"], "sk-edited");
    assert!(
        !profile
            .config_contents
            .contains("experimental_bearer_token")
    );
}

#[test]
fn apply_relay_profile_preserves_provider_specific_id_in_live_config() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("auth.json"),
        r#"{"auth_mode":"chatgpt","tokens":{"access_token":"official"}}"#,
    )
    .unwrap();
    let mut provider_b = RelayProfile {
        id: "provider-b".to_string(),
        relay_mode: RelayMode::PureApi,
        config_contents: r#"model_provider = "aihubmix"
model = "gpt-5.4"
profile = "work"

[model_providers.aihubmix]
name = "AiHubMix"
base_url = "https://aihubmix.example/v1"
wire_api = "responses"
requires_openai_auth = true

[profiles.work]
model_provider = "aihubmix"
model = "gpt-5.4"
"#
        .to_string(),
        auth_contents: r#"{"OPENAI_API_KEY":"aihubmix-key"}"#.to_string(),
        ..RelayProfile::default()
    };

    apply_relay_profile_to_home_with_switch_rules(temp.path(), &provider_b, "").unwrap();
    let live_config = std::fs::read_to_string(temp.path().join("config.toml")).unwrap();
    assert!(live_config.contains(r#"model_provider = "aihubmix""#));
    assert!(live_config.contains("[model_providers.aihubmix]"));
    assert!(!live_config.contains("[model_providers.custom]"));

    let mut common = String::new();
    backfill_relay_profile_from_home_with_common(temp.path(), &mut provider_b, &mut common)
        .unwrap();

    assert!(
        provider_b
            .config_contents
            .contains(r#"model_provider = "aihubmix""#)
    );
    assert!(
        provider_b
            .config_contents
            .contains("[model_providers.aihubmix]")
    );
    assert!(provider_b.config_contents.contains(r#"name = "AiHubMix""#));
    assert!(
        provider_b
            .config_contents
            .contains(r#"model_provider = "aihubmix""#)
    );
    assert!(
        !provider_b
            .config_contents
            .contains("[model_providers.custom]")
    );
    let auth: serde_json::Value = serde_json::from_str(&provider_b.auth_contents).unwrap();
    assert_eq!(auth["OPENAI_API_KEY"], "aihubmix-key");
    assert!(auth.get("tokens").is_none());
}

#[test]
fn backfill_current_profile_preserves_external_live_provider_id_edit_before_switch() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("config.toml"),
        r#"model_provider = "manual_edit"
model = "gpt-5.4"

[model_providers.manual_edit]
name = "Manual Edit"
base_url = "https://manual.example/v1"
wire_api = "responses"
requires_openai_auth = true
"#,
    )
    .unwrap();
    std::fs::write(
        temp.path().join("auth.json"),
        r#"{"OPENAI_API_KEY":"sk-live"}"#,
    )
    .unwrap();

    let mut current = RelayProfile {
        id: "provider-a".to_string(),
        relay_mode: RelayMode::PureApi,
        config_contents: r#"model_provider = "old_snapshot"
model = "gpt-5.4"

[model_providers.old_snapshot]
name = "Old Snapshot"
base_url = "https://old.example/v1"
wire_api = "responses"
requires_openai_auth = true
"#
        .to_string(),
        auth_contents: r#"{"OPENAI_API_KEY":"sk-old"}"#.to_string(),
        ..RelayProfile::default()
    };
    let mut common = String::new();

    backfill_relay_profile_from_home_with_common(temp.path(), &mut current, &mut common).unwrap();

    assert!(
        current
            .config_contents
            .contains(r#"model_provider = "manual_edit""#)
    );
    assert!(
        current
            .config_contents
            .contains("[model_providers.manual_edit]")
    );
    assert!(current.config_contents.contains(r#"name = "Manual Edit""#));
    assert!(!current.config_contents.contains("old_snapshot"));
    let auth: serde_json::Value = serde_json::from_str(&current.auth_contents).unwrap();
    assert_eq!(auth["OPENAI_API_KEY"], "sk-live");
}

#[test]
fn backfill_official_profile_promotes_external_pure_api_live_edit_before_switch() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("config.toml"),
        r#"model = "deepseek-chat"
model_provider = "manual_api"

[model_providers.manual_api]
name = "Manual API"
base_url = "https://manual.example/v1"
wire_api = "responses"
requires_openai_auth = true
"#,
    )
    .unwrap();
    std::fs::write(
        temp.path().join("auth.json"),
        r#"{"OPENAI_API_KEY":"sk-manual"}"#,
    )
    .unwrap();
    let mut current = RelayProfile {
        id: "official".to_string(),
        relay_mode: RelayMode::Official,
        official_mix_api_key: false,
        config_contents: String::new(),
        auth_contents: r#"{"auth_mode":"chatgpt","tokens":{"access_token":"official"}}"#
            .to_string(),
        ..RelayProfile::default()
    };
    let mut common = String::new();

    backfill_relay_profile_from_home_with_common(temp.path(), &mut current, &mut common).unwrap();
    normalize_relay_profile_for_storage(&mut current).unwrap();

    assert_eq!(current.relay_mode, RelayMode::Official);
    assert!(!current.official_mix_api_key);
    assert!(current.config_contents.is_empty());
    assert!(current.api_key.is_empty());
    assert!(!current.auth_contents.contains("OPENAI_API_KEY"));
}

#[test]
fn backfill_official_profile_promotes_external_official_mix_live_edit_before_switch() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("config.toml"),
        r#"model = "deepseek-chat"
model_provider = "manual_mix"

[model_providers.manual_mix]
name = "Manual Mix"
base_url = "https://manual.example/v1"
wire_api = "responses"
requires_openai_auth = true
experimental_bearer_token = "sk-mix"
"#,
    )
    .unwrap();
    std::fs::write(
        temp.path().join("auth.json"),
        r#"{"auth_mode":"chatgpt","tokens":{"access_token":"official"}}"#,
    )
    .unwrap();
    let mut current = RelayProfile {
        id: "official".to_string(),
        relay_mode: RelayMode::Official,
        official_mix_api_key: false,
        config_contents: String::new(),
        auth_contents: r#"{"auth_mode":"chatgpt","tokens":{"access_token":"old"}}"#.to_string(),
        ..RelayProfile::default()
    };
    let mut common = String::new();

    backfill_relay_profile_from_home_with_common(temp.path(), &mut current, &mut common).unwrap();
    normalize_relay_profile_for_storage(&mut current).unwrap();

    assert_eq!(current.relay_mode, RelayMode::Official);
    assert!(!current.official_mix_api_key);
    assert!(current.config_contents.is_empty());
    assert!(current.api_key.is_empty());
    assert!(!current.auth_contents.contains("OPENAI_API_KEY"));
}

#[test]
fn backfill_official_profile_does_not_promote_codex_plus_switch_live_config() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("config.toml"),
        r#"model = "deepseek-chat"
model_provider = "custom"

[model_providers.custom]
name = "custom"
base_url = "https://third-party.example/v1"
wire_api = "responses"
requires_openai_auth = true
"#,
    )
    .unwrap();
    std::fs::write(
        temp.path().join("auth.json"),
        r#"{"OPENAI_API_KEY":"sk-third-party"}"#,
    )
    .unwrap();
    let mut current = RelayProfile {
        id: "official".to_string(),
        relay_mode: RelayMode::Official,
        official_mix_api_key: false,
        config_contents: String::new(),
        auth_contents: r#"{"auth_mode":"chatgpt","tokens":{"access_token":"official"}}"#
            .to_string(),
        ..RelayProfile::default()
    };
    let mut common = String::new();

    backfill_relay_profile_from_home_with_common(temp.path(), &mut current, &mut common).unwrap();
    normalize_relay_profile_for_storage(&mut current).unwrap();

    assert_eq!(current.relay_mode, RelayMode::Official);
    assert!(!current.official_mix_api_key);
    assert!(current.config_contents.is_empty());
    assert!(current.api_key.is_empty());
    assert!(!current.auth_contents.contains("OPENAI_API_KEY"));
}

#[test]
fn backfill_official_profile_does_not_promote_custom_numbered_live_config() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("config.toml"),
        r#"model = "gpt-5.5"
model_provider = "custom1"

[model_providers.custom1]
name = "custom1"
base_url = "https://third-party.example/v1"
wire_api = "responses"
requires_openai_auth = true
"#,
    )
    .unwrap();
    std::fs::write(
        temp.path().join("auth.json"),
        r#"{"OPENAI_API_KEY":"sk-third-party"}"#,
    )
    .unwrap();
    let mut current = RelayProfile {
        id: "official".to_string(),
        relay_mode: RelayMode::Official,
        official_mix_api_key: false,
        config_contents: String::new(),
        auth_contents: r#"{"auth_mode":"chatgpt","tokens":{"access_token":"official"}}"#
            .to_string(),
        ..RelayProfile::default()
    };
    let mut common = String::new();

    backfill_relay_profile_from_home_with_common(temp.path(), &mut current, &mut common).unwrap();
    normalize_relay_profile_for_storage(&mut current).unwrap();

    assert_eq!(current.relay_mode, RelayMode::Official);
    assert!(!current.official_mix_api_key);
    assert!(current.config_contents.is_empty());
    assert!(current.api_key.is_empty());
    assert!(!current.auth_contents.contains("OPENAI_API_KEY"));
}

#[test]
fn backfill_official_mix_profile_keeps_key_after_switch_roundtrip_storage() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("config.toml"),
        r#"model = "gpt-5"
model_provider = "custom"

[model_providers.custom]
name = "custom"
base_url = "https://relay.example/v1"
wire_api = "responses"
requires_openai_auth = true
experimental_bearer_token = "sk-saved-mix"
"#,
    )
    .unwrap();
    std::fs::write(
        temp.path().join("auth.json"),
        r#"{"auth_mode":"chatgpt","tokens":{"access_token":"official"}}"#,
    )
    .unwrap();
    let mut profile = RelayProfile {
        id: "official-mix".to_string(),
        relay_mode: RelayMode::Official,
        official_mix_api_key: true,
        config_contents: r#"model = "gpt-5"
model_provider = "custom"

[model_providers.custom]
name = "custom"
base_url = "https://relay.example/v1"
wire_api = "responses"
requires_openai_auth = true
experimental_bearer_token = "sk-saved-mix"
"#
        .to_string(),
        auth_contents: r#"{"auth_mode":"chatgpt","tokens":{"access_token":"official"}}"#
            .to_string(),
        ..RelayProfile::default()
    };
    let mut common = String::new();

    backfill_relay_profile_from_home_with_common(temp.path(), &mut profile, &mut common).unwrap();
    normalize_relay_profile_for_storage(&mut profile).unwrap();

    assert_eq!(profile.relay_mode, RelayMode::Official);
    assert!(profile.official_mix_api_key);
    assert_eq!(profile.api_key, "sk-saved-mix");
    assert!(
        profile
            .config_contents
            .contains(r#"experimental_bearer_token = "sk-saved-mix""#)
    );
    let auth: serde_json::Value = serde_json::from_str(&profile.auth_contents).unwrap();
    assert!(auth.get("OPENAI_API_KEY").is_none());
    assert_eq!(auth["tokens"]["access_token"], "official");
}

#[test]
fn backfill_official_mix_profile_keeps_mix_mode_when_live_auth_has_api_key() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("config.toml"),
        r#"model = "gpt-5"
model_provider = "custom"

[model_providers.custom]
name = "custom"
base_url = "https://relay.example/v1"
wire_api = "responses"
requires_openai_auth = true
experimental_bearer_token = "333333333333333333333"
"#,
    )
    .unwrap();
    std::fs::write(
        temp.path().join("auth.json"),
        r#"{"OPENAI_API_KEY":"333333333333333333333","auth_mode":"chatgpt","tokens":{"access_token":"official"}}"#,
    )
    .unwrap();
    let mut profile = RelayProfile {
        id: "official-mix".to_string(),
        relay_mode: RelayMode::Official,
        official_mix_api_key: true,
        config_contents: r#"model = "gpt-5"
model_provider = "custom"

[model_providers.custom]
name = "custom"
base_url = "https://relay.example/v1"
wire_api = "responses"
requires_openai_auth = true
experimental_bearer_token = "22222222222222222222222222222222222"
"#
        .to_string(),
        auth_contents: r#"{"auth_mode":"chatgpt","tokens":{"access_token":"official"}}"#
            .to_string(),
        ..RelayProfile::default()
    };
    let mut common = String::new();

    backfill_relay_profile_from_home_with_common(temp.path(), &mut profile, &mut common).unwrap();
    normalize_relay_profile_for_storage(&mut profile).unwrap();

    assert_eq!(profile.relay_mode, RelayMode::Official);
    assert!(profile.official_mix_api_key);
    assert_eq!(profile.api_key, "333333333333333333333");
    assert!(
        profile
            .config_contents
            .contains(r#"experimental_bearer_token = "333333333333333333333""#)
    );
    assert!(!profile.auth_contents.contains("OPENAI_API_KEY"));
}

#[test]
fn apply_relay_profile_to_home_with_switch_rules_switches_auth_and_writes_provider_token() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("auth.json"),
        r#"{"auth_mode":"chatgpt","tokens":{"access_token":"official"}}"#,
    )
    .unwrap();
    let profile = RelayProfile {
        id: "relay-a".to_string(),
        relay_mode: RelayMode::PureApi,
        config_contents: r#"model = "qwen3-coder"
model_provider = "custom"

[model_providers.custom]
name = "custom"
wire_api = "responses"
requires_openai_auth = true
base_url = "https://relay.example/v1"
"#
        .to_string(),
        auth_contents: r#"{"OPENAI_API_KEY":"sk-new"}"#.to_string(),
        ..RelayProfile::default()
    };

    apply_relay_profile_to_home_with_switch_rules(temp.path(), &profile, "").unwrap();

    let auth = std::fs::read_to_string(temp.path().join("auth.json")).unwrap();
    let auth: serde_json::Value = serde_json::from_str(&auth).unwrap();
    assert_eq!(auth["OPENAI_API_KEY"], "sk-new");
    assert!(auth.get("auth_mode").is_none());
    assert!(auth.get("tokens").is_none());
    let config = std::fs::read_to_string(temp.path().join("config.toml")).unwrap();
    assert!(!config.contains("experimental_bearer_token"));
    assert!(config.contains(r#"model_provider = "custom""#));
    assert!(config.contains("[model_providers.custom]"));
}

#[test]
fn apply_relay_profile_to_home_with_switch_rules_repairs_incomplete_provider_config() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("config.toml"),
        r#"model = "live-model"
model_provider = "live_provider"

[model_providers.live_provider]
base_url = "https://live.example/v1"
experimental_bearer_token = "sk-live"
"#,
    )
    .unwrap();
    let profile = RelayProfile {
        id: "relay-a".to_string(),
        model: "qwen3-coder".to_string(),
        base_url: "https://relay.example/v1".to_string(),
        api_key: "sk-new".to_string(),
        relay_mode: RelayMode::PureApi,
        config_contents: r#"[model_providers.custom]
experimental_bearer_token = "sk-new"
"#
        .to_string(),
        auth_contents: r#"{"OPENAI_API_KEY":"sk-new"}"#.to_string(),
        ..RelayProfile::default()
    };

    apply_relay_profile_to_home_with_switch_rules(temp.path(), &profile, "").unwrap();

    let config = std::fs::read_to_string(temp.path().join("config.toml")).unwrap();
    assert!(config.contains(r#"model = "qwen3-coder""#));
    assert!(config.contains(r#"model_provider = "custom""#));
    assert!(config.contains("[model_providers.custom]"));
    assert!(config.contains(r#"name = "custom""#));
    assert!(config.contains(r#"wire_api = "responses""#));
    assert!(config.contains("requires_openai_auth = true"));
    assert!(config.contains(r#"base_url = "https://relay.example/v1""#));
    assert!(!config.contains("experimental_bearer_token"));
    assert!(!config.contains("live_provider"));
    assert!(!config.contains("https://live.example/v1"));
}

#[test]
fn apply_relay_profile_to_home_with_switch_rules_uses_config_contents_as_source() {
    let temp = tempfile::tempdir().unwrap();
    let profile = RelayProfile {
        id: "relay-a".to_string(),
        relay_mode: RelayMode::PureApi,
        config_contents: r#"model_provider = "max_ai"
model = "gpt-5.4"
disable_response_storage = true

[model_providers.max_ai]
name = "max_ai"
base_url = "https://max2.jojocode.com/v1"
wire_api = "responses"
requires_openai_auth = true
"#
        .to_string(),
        auth_contents: r#"{"OPENAI_API_KEY":"sk-new"}"#.to_string(),
        ..RelayProfile::default()
    };

    apply_relay_profile_to_home_with_switch_rules(temp.path(), &profile, "").unwrap();

    let config = std::fs::read_to_string(temp.path().join("config.toml")).unwrap();
    assert!(config.contains(r#"model = "gpt-5.4""#));
    assert!(config.contains("disable_response_storage = true"));
    assert!(config.contains(r#"model_provider = "max_ai""#));
    assert!(config.contains("[model_providers.max_ai]"));
    assert!(config.contains(r#"name = "max_ai""#));
    assert!(config.contains(r#"base_url = "https://max2.jojocode.com/v1""#));
    assert!(!config.contains("experimental_bearer_token"));
    assert!(!config.contains("[model_providers.custom]"));
}

#[cfg(windows)]
#[test]
fn apply_relay_profile_to_home_with_switch_rules_does_not_preserve_computer_use_guard_config_by_default()
 {
    let temp = tempfile::tempdir().unwrap();
    let helper = temp
        .path()
        .join("plugins")
        .join("cache")
        .join("openai-bundled")
        .join("computer-use")
        .join("26.608.12217")
        .join("node_modules")
        .join("@oai")
        .join("sky")
        .join("bin")
        .join("windows")
        .join("codex-computer-use.exe");
    std::fs::create_dir_all(helper.parent().unwrap()).unwrap();
    std::fs::write(&helper, "").unwrap();
    let profile = RelayProfile {
        id: "relay-a".to_string(),
        relay_mode: RelayMode::PureApi,
        config_contents: r#"model_provider = "max_ai"
model = "gpt-5.4"

[features]
js_repl = false

[model_providers.max_ai]
name = "max_ai"
base_url = "https://max2.jojocode.com/v1"
wire_api = "responses"
requires_openai_auth = true
"#
        .to_string(),
        auth_contents: r#"{"OPENAI_API_KEY":"sk-new"}"#.to_string(),
        ..RelayProfile::default()
    };

    apply_relay_profile_to_home_with_switch_rules(temp.path(), &profile, "").unwrap();

    let config = std::fs::read_to_string(temp.path().join("config.toml")).unwrap();
    assert!(config.contains("js_repl = false"));
    assert!(!config.contains("[plugins.\"browser@openai-bundled\"]"));
    assert!(!config.contains("[plugins.\"chrome@openai-bundled\"]"));
    assert!(!config.contains("[plugins.\"computer-use@openai-bundled\"]"));
    assert!(!config.contains(r#"notify = ["#));
    assert!(!config.contains("codex-computer-use.exe"));
}

#[cfg(windows)]
#[test]
fn apply_relay_profile_to_home_with_switch_rules_preserves_computer_use_guard_config_when_enabled()
{
    let temp = tempfile::tempdir().unwrap();
    let helper = temp
        .path()
        .join("plugins")
        .join("cache")
        .join("openai-bundled")
        .join("computer-use")
        .join("26.608.12217")
        .join("node_modules")
        .join("@oai")
        .join("sky")
        .join("bin")
        .join("windows")
        .join("codex-computer-use.exe");
    std::fs::create_dir_all(helper.parent().unwrap()).unwrap();
    std::fs::write(&helper, "").unwrap();
    let profile = RelayProfile {
        id: "relay-a".to_string(),
        relay_mode: RelayMode::PureApi,
        config_contents: r#"model_provider = "max_ai"
model = "gpt-5.4"

[features]
js_repl = false

[model_providers.max_ai]
name = "max_ai"
base_url = "https://max2.jojocode.com/v1"
wire_api = "responses"
requires_openai_auth = true
"#
        .to_string(),
        auth_contents: r#"{"OPENAI_API_KEY":"sk-new"}"#.to_string(),
        ..RelayProfile::default()
    };

    apply_relay_profile_to_home_with_switch_rules_and_computer_use_guard(
        temp.path(),
        &profile,
        "",
        true,
    )
    .unwrap();

    let config = std::fs::read_to_string(temp.path().join("config.toml")).unwrap();
    assert!(config.contains("js_repl = true"));
    assert!(config.contains("[plugins.\"browser@openai-bundled\"]"));
    assert!(config.contains("[plugins.\"chrome@openai-bundled\"]"));
    assert!(config.contains("[plugins.\"computer-use@openai-bundled\"]"));
    assert!(config.contains(r#"notify = ["#));
    assert!(config.contains("codex-computer-use.exe"));
}

#[test]
fn apply_relay_profile_to_home_with_switch_rules_preserves_unmanaged_live_context_entries() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("config.toml"),
        r#"model = "old"

[mcp_servers.manual]
command = "manual-command"

[plugins.manual]
enabled = true

[marketplaces.role-specific-plugins]
source_type = "local"
source = 'C:\Users\me\.codex\.tmp\marketplaces\role-specific-plugins'
"#,
    )
    .unwrap();
    let profile = RelayProfile {
        id: "relay-a".to_string(),
        relay_mode: RelayMode::PureApi,
        config_contents: r#"model = "gpt-5.5"
model_provider = "custom"

[model_providers.custom]
name = "custom"
wire_api = "responses"
requires_openai_auth = true
base_url = "https://relay.example/v1"
"#
        .to_string(),
        auth_contents: r#"{"OPENAI_API_KEY":"sk-new"}"#.to_string(),
        ..RelayProfile::default()
    };
    let common = r#"[mcp_servers.managed]
command = "managed-command"
"#;

    apply_relay_profile_to_home_with_switch_rules(temp.path(), &profile, common).unwrap();

    let config = std::fs::read_to_string(temp.path().join("config.toml")).unwrap();
    assert!(config.contains("[mcp_servers.manual]"));
    assert!(config.contains(r#"command = "manual-command""#));
    assert!(config.contains("[plugins.manual]"));
    assert!(config.contains("[mcp_servers.managed]"));
    assert!(config.contains(r#"command = "managed-command""#));
    assert!(config.contains("[marketplaces.role-specific-plugins]"));
    assert!(config.contains(r#"source_type = "local""#));
    assert!(config.contains("role-specific-plugins"));
}

#[test]
fn apply_relay_profile_to_home_with_switch_rules_does_not_preserve_unselected_managed_context_entries()
 {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("config.toml"),
        r#"model = "old"

[mcp_servers.manual]
command = "manual-command"

[mcp_servers.managed]
command = "old-managed"
"#,
    )
    .unwrap();
    let profile = RelayProfile {
        id: "relay-a".to_string(),
        relay_mode: RelayMode::PureApi,
        context_selection_initialized: true,
        context_selection: RelayContextSelection::default(),
        config_contents: r#"model = "gpt-5.5"
model_provider = "custom"

[model_providers.custom]
name = "custom"
wire_api = "responses"
requires_openai_auth = true
base_url = "https://relay.example/v1"
"#
        .to_string(),
        auth_contents: r#"{"OPENAI_API_KEY":"sk-new"}"#.to_string(),
        ..RelayProfile::default()
    };
    let common = r#"[mcp_servers.managed]
command = "managed-command"
"#;

    apply_relay_profile_to_home_with_switch_rules(temp.path(), &profile, common).unwrap();

    let config = std::fs::read_to_string(temp.path().join("config.toml")).unwrap();
    assert!(config.contains("[mcp_servers.manual]"));
    assert!(!config.contains("[mcp_servers.managed]"));
}

#[test]
fn filter_common_config_for_selection_writes_only_selected_context_entries() {
    let common = r#"model_reasoning_effort = "high"

[mcp_servers.keep]
command = "keep"

[mcp_servers.skip]
command = "skip"

[skills.writer]
enabled = true

[plugins.browser]
enabled = true
"#;
    let selection = RelayContextSelection {
        mcp_servers: vec!["keep".to_string()],
        skills: Vec::new(),
        plugins: vec!["browser".to_string()],
    };

    let filtered = filter_common_config_for_selection(common, &selection).unwrap();

    assert!(filtered.contains("model_reasoning_effort"));
    assert!(filtered.contains("[mcp_servers.keep]"));
    assert!(!filtered.contains("[mcp_servers.skip]"));
    assert!(!filtered.contains("[skills.writer]"));
    assert!(filtered.contains("[plugins.browser]"));
}

#[test]
fn sync_live_config_context_entries_preserves_unmanaged_live_entries() {
    let live = r#"model = "gpt-5"

[mcp_servers.manual]
command = "manual"

[mcp_servers.managed]
command = "old"
"#;
    let context = r#"[mcp_servers.managed]
command = "new"

[mcp_servers.disabled]
enabled = false
command = "disabled"
"#;

    let updated = sync_live_config_context_entries(live, context).unwrap();

    assert!(updated.contains("[mcp_servers.manual]"));
    assert!(updated.contains(r#"command = "manual""#));
    assert!(updated.contains("[mcp_servers.managed]"));
    assert!(updated.contains(r#"command = "new""#));
    assert!(!updated.contains("[mcp_servers.disabled]"));
}

#[test]
fn sync_live_config_context_entries_removes_disabled_managed_entries_from_live() {
    let live = r#"model = "gpt-5"

[mcp_servers.manual]
command = "manual"

[mcp_servers.managed]
command = "old"
"#;
    let context = r#"[mcp_servers.managed]
enabled = false
command = "old"
"#;

    let updated = sync_live_config_context_entries(live, context).unwrap();

    assert!(updated.contains("[mcp_servers.manual]"));
    assert!(!updated.contains("[mcp_servers.managed]"));
}

#[test]
fn apply_relay_profile_to_home_with_switch_rules_writes_provider_even_when_auth_has_no_api_key() {
    let temp = tempfile::tempdir().unwrap();
    let profile = RelayProfile {
        id: "relay-empty-auth".to_string(),
        relay_mode: RelayMode::PureApi,
        config_contents: r#"model = "gpt-5.5"
model_provider = "custom"

[model_providers]

[model_providers.custom]
name = "custom"
wire_api = "responses"
requires_openai_auth = true
base_url = "http://192.168.188.245:3001/v1"
"#
        .to_string(),
        auth_contents: "{}".to_string(),
        ..RelayProfile::default()
    };

    apply_relay_profile_to_home_with_switch_rules(temp.path(), &profile, "").unwrap();

    let config = std::fs::read_to_string(temp.path().join("config.toml")).unwrap();
    assert!(config.contains(r#"model = "gpt-5.5""#));
    assert!(config.contains(r#"model_provider = "custom""#));
    assert!(config.contains("[model_providers.custom]"));
    assert!(config.contains(r#"base_url = "http://192.168.188.245:3001/v1""#));
}

#[test]
fn apply_relay_profile_to_home_with_switch_rules_switches_auth_even_when_provider_token_exists() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("auth.json"),
        r#"{"auth_mode":"chatgpt","tokens":{"access_token":"official"}}"#,
    )
    .unwrap();
    let profile = RelayProfile {
        id: "relay-provider-token".to_string(),
        relay_mode: RelayMode::PureApi,
        config_contents: r#"model = "gpt-5.5"
model_provider = "custom"

[model_providers.custom]
name = "custom"
wire_api = "responses"
requires_openai_auth = true
base_url = "http://192.168.188.245:3001/v1"
experimental_bearer_token = "sk-provider-token"
"#
        .to_string(),
        auth_contents: "{}".to_string(),
        ..RelayProfile::default()
    };

    apply_relay_profile_to_home_with_switch_rules(temp.path(), &profile, "").unwrap();

    let auth = std::fs::read_to_string(temp.path().join("auth.json")).unwrap();
    let auth: serde_json::Value = serde_json::from_str(&auth).unwrap();
    assert!(auth.as_object().is_some_and(|object| object.is_empty()));

    let config = std::fs::read_to_string(temp.path().join("config.toml")).unwrap();
    assert!(!config.contains("experimental_bearer_token"));
}

#[test]
fn apply_official_mix_profile_clears_live_auth_api_key_and_keeps_login() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("auth.json"),
        r#"{"OPENAI_API_KEY":"sk-pure-api","auth_mode":"chatgpt","tokens":{"access_token":"official"}}"#,
    )
    .unwrap();
    let profile = RelayProfile {
        id: "official-mix".to_string(),
        relay_mode: RelayMode::Official,
        official_mix_api_key: true,
        config_contents: r#"model = "gpt-5"
model_provider = "custom"

[model_providers.custom]
name = "custom"
wire_api = "responses"
requires_openai_auth = true
base_url = "https://relay.example/v1"
experimental_bearer_token = "sk-official-mix"
"#
        .to_string(),
        auth_contents: r#"{"auth_mode":"chatgpt","tokens":{"access_token":"official"}}"#
            .to_string(),
        ..RelayProfile::default()
    };

    apply_relay_profile_to_home_with_switch_rules(temp.path(), &profile, "").unwrap();

    let auth = std::fs::read_to_string(temp.path().join("auth.json")).unwrap();
    let auth: serde_json::Value = serde_json::from_str(&auth).unwrap();
    assert!(auth.get("OPENAI_API_KEY").is_none());
    assert_eq!(auth["auth_mode"], "chatgpt");
    assert_eq!(auth["tokens"]["access_token"], "official");

    let config = std::fs::read_to_string(temp.path().join("config.toml")).unwrap();
    assert!(config.contains(r#"experimental_bearer_token = "sk-official-mix""#));
    assert!(config.contains("requires_openai_auth = true"));
}

#[test]
fn apply_official_mix_profile_keeps_config_token_when_api_key_field_is_empty() {
    let temp = tempfile::tempdir().unwrap();
    let profile = RelayProfile {
        id: "official-mix".to_string(),
        relay_mode: RelayMode::Official,
        official_mix_api_key: true,
        config_contents: r#"model = "gpt-5"
model_provider = "custom"

[model_providers.custom]
name = "custom"
wire_api = "responses"
requires_openai_auth = true
base_url = "https://relay.example/v1"
experimental_bearer_token = "sk-from-config"
"#
        .to_string(),
        auth_contents: String::new(),
        api_key: String::new(),
        ..RelayProfile::default()
    };

    apply_relay_profile_to_home_with_switch_rules(temp.path(), &profile, "").unwrap();

    let config = std::fs::read_to_string(temp.path().join("config.toml")).unwrap();
    assert!(config.contains(r#"experimental_bearer_token = "sk-from-config""#));
    let auth = std::fs::read_to_string(temp.path().join("auth.json")).unwrap();
    assert!(auth.trim().is_empty());
}

#[test]
fn strip_common_config_with_duplicate_context_tables_preserves_provider_config() {
    let config = r#"model = "gpt-5.5"
model_provider = "custom"

[model_providers.custom]
name = "custom"
wire_api = "responses"
requires_openai_auth = true
base_url = "http://192.168.188.245:3001/v1"
"#;
    let common = r#"model_reasoning_effort = "high"

[mcp_servers]

[plugins."documents@openai-primary-runtime"]
enabled = true

[mcp_servers]

[mcp_servers.ida-pro-mcp]
command = "python"
"#;

    let stripped = strip_common_config_from_config(config, common).unwrap();

    assert!(stripped.contains(r#"model = "gpt-5.5""#));
    assert!(stripped.contains(r#"model_provider = "custom""#));
    assert!(stripped.contains("[model_providers.custom]"));
    assert!(stripped.contains(r#"base_url = "http://192.168.188.245:3001/v1""#));
}

#[test]
fn apply_relay_profile_to_home_with_switch_rules_survives_official_roundtrip() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("config.toml"),
        r#"model = "gpt-5"
model_provider = "custom"

[model_providers.custom]
name = "custom"
base_url = "https://old.example/v1"
wire_api = "responses"
requires_openai_auth = true
experimental_bearer_token = "sk-old"
"#,
    )
    .unwrap();
    std::fs::write(
        temp.path().join("auth.json"),
        r#"{"OPENAI_API_KEY":"sk-old","tokens":{"access_token":"official"}}"#,
    )
    .unwrap();

    clear_relay_config_to_home(temp.path()).unwrap();
    let mut official = RelayProfile {
        relay_mode: RelayMode::Official,
        use_common_config: true,
        ..RelayProfile::default()
    };
    let mut common = String::new();
    backfill_relay_profile_from_home_with_common(temp.path(), &mut official, &mut common).unwrap();

    let mut relay = RelayProfile {
        id: "relay-a".to_string(),
        model: "gpt-5.4".to_string(),
        base_url: "https://max2.jojocode.com/v1".to_string(),
        api_key: "sk-new".to_string(),
        relay_mode: RelayMode::PureApi,
        config_contents: r#"[model_providers.custom]
experimental_bearer_token = "sk-new"
"#
        .to_string(),
        auth_contents: r#"{"OPENAI_API_KEY":"sk-new"}"#.to_string(),
        ..RelayProfile::default()
    };
    normalize_relay_profile_for_storage(&mut relay).unwrap();
    apply_relay_profile_to_home_with_switch_rules(temp.path(), &relay, "").unwrap();

    let config = std::fs::read_to_string(temp.path().join("config.toml")).unwrap();
    assert!(config.contains(r#"model = "gpt-5.4""#));
    assert!(config.contains(r#"model_provider = "custom""#));
    assert!(config.contains("[model_providers.custom]"));
    assert!(config.contains(r#"name = "custom""#));
    assert!(config.contains(r#"base_url = "https://max2.jojocode.com/v1""#));
    assert!(config.contains(r#"wire_api = "responses""#));
    assert!(config.contains("requires_openai_auth = true"));
    assert!(!config.contains("experimental_bearer_token"));
}

#[test]
fn backfill_relay_profile_from_official_config_without_model_providers_does_not_panic() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("config.toml"),
        r#"model = "gpt-5"

[features]
goals = true
"#,
    )
    .unwrap();
    std::fs::write(
        temp.path().join("auth.json"),
        r#"{"auth_mode":"chatgpt","tokens":{"access_token":"official"}}"#,
    )
    .unwrap();
    let mut profile = RelayProfile::default();
    let mut common = String::new();

    backfill_relay_profile_from_home_with_common(temp.path(), &mut profile, &mut common).unwrap();

    assert!(profile.config_contents.contains(r#"model = "gpt-5""#));
    assert!(!profile.auth_contents.is_empty());
}

fn base64_url_no_pad(value: &str) -> String {
    use base64::Engine;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(value.as_bytes())
}

#[test]
fn apply_relay_profile_generates_model_catalog_for_suffixed_models() {
    let temp = tempfile::tempdir().unwrap();
    let profile = RelayProfile {
        id: "relay-a".to_string(),
        name: "Relay A".to_string(),
        model: "deepseek-v4-pro".to_string(),
        relay_mode: RelayMode::PureApi,
        config_contents: r#"model = "deepseek-v4-pro"
model_provider = "custom"

[model_providers.custom]
name = "custom"
wire_api = "responses"
requires_openai_auth = true
base_url = "https://relay.example/v1"
experimental_bearer_token = "sk-new"
"#
        .to_string(),
        auth_contents: r#"{"OPENAI_API_KEY":"sk-new"}"#.to_string(),
        model_insert_mode: Default::default(),
        model_list: "deepseek-v4-pro[1M]\nclaude-sonnet-4[200K]".to_string(),
        context_window: "272000".to_string(),
        auto_compact_limit: String::new(),
        ..RelayProfile::default()
    };

    apply_relay_profile_files_to_home_with_context(temp.path(), &profile, "").unwrap();

    let config = std::fs::read_to_string(temp.path().join("config.toml")).unwrap();
    assert!(config.contains(r#"model_catalog_json = "model-catalogs/relay-a.json""#));
    let catalog_path = temp.path().join("model-catalogs").join("relay-a.json");
    assert!(catalog_path.exists());
    let catalog = std::fs::read_to_string(&catalog_path).unwrap();
    assert!(catalog.contains(r#""slug": "deepseek-v4-pro""#));
    assert!(catalog.contains(r#""context_window": 1000000"#));
    assert!(catalog.contains(r#""slug": "claude-sonnet-4""#));
    assert!(catalog.contains(r#""context_window": 200000"#));
    // 后缀不得进入 catalog 或 config
    assert!(!catalog.contains("[1M]"));
    assert!(!config.contains("[1M]"));
}

#[test]
fn apply_relay_profile_no_catalog_when_model_list_has_no_suffix() {
    let temp = tempfile::tempdir().unwrap();
    let profile = RelayProfile {
        id: "relay-a".to_string(),
        name: "Relay A".to_string(),
        model: "qwen3-coder".to_string(),
        relay_mode: RelayMode::PureApi,
        config_contents: r#"model = "qwen3-coder"
model_provider = "custom"

[model_providers.custom]
name = "custom"
wire_api = "responses"
requires_openai_auth = true
base_url = "https://relay.example/v1"
experimental_bearer_token = "sk-new"
"#
        .to_string(),
        auth_contents: r#"{"OPENAI_API_KEY":"sk-new"}"#.to_string(),
        model_insert_mode: Default::default(),
        model_list: "deepseek-coder\nqwen3-coder".to_string(),
        context_window: "200000".to_string(),
        auto_compact_limit: "160000".to_string(),
        ..RelayProfile::default()
    };

    apply_relay_profile_files_to_home_with_context(temp.path(), &profile, "").unwrap();

    let config = std::fs::read_to_string(temp.path().join("config.toml")).unwrap();
    assert!(!config.contains("model_catalog_json"));
    assert!(config.contains("model_context_window = 200000"));
    assert!(!temp.path().join("model-catalogs").exists());
}

#[test]
fn apply_relay_profile_does_not_overwrite_user_model_catalog_json() {
    let temp = tempfile::tempdir().unwrap();
    let profile = RelayProfile {
        id: "relay-a".to_string(),
        name: "Relay A".to_string(),
        model: "deepseek-v4-pro".to_string(),
        relay_mode: RelayMode::PureApi,
        config_contents: r#"model = "deepseek-v4-pro"
model_catalog_json = "/old/catalog.json"
model_provider = "custom"

[model_providers.custom]
name = "custom"
wire_api = "responses"
requires_openai_auth = true
base_url = "https://relay.example/v1"
experimental_bearer_token = "sk-new"
"#
        .to_string(),
        auth_contents: r#"{"OPENAI_API_KEY":"sk-new"}"#.to_string(),
        model_insert_mode: Default::default(),
        // 即使有后缀，用户已手写指针也应保留不覆盖
        model_list: "deepseek-v4-pro[1M]".to_string(),
        ..RelayProfile::default()
    };

    apply_relay_profile_files_to_home_with_context(temp.path(), &profile, "").unwrap();

    let config = std::fs::read_to_string(temp.path().join("config.toml")).unwrap();
    assert!(config.contains(r#"model_catalog_json = "/old/catalog.json""#));
    assert!(!config.contains("model-catalogs/relay-a.json"));
    assert!(!temp.path().join("model-catalogs").exists());
}

#[test]
fn apply_relay_profile_strips_model_suffix_and_generates_catalog() {
    let temp = tempfile::tempdir().unwrap();
    let profile = RelayProfile {
        id: "relay-ark".to_string(),
        name: "火山引擎 Ark".to_string(),
        model: "deepseek-v4-flash[1M]".to_string(),
        relay_mode: RelayMode::PureApi,
        config_contents: r#"model = "deepseek-v4-flash[1M]"
model_provider = "custom"

[model_providers.custom]
name = "custom"
wire_api = "responses"
requires_openai_auth = true
base_url = "https://ark.cn-beijing.volces.com/api/coding/v3"
experimental_bearer_token = "sk-ark"
"#
        .to_string(),
        auth_contents: r#"{"OPENAI_API_KEY":"sk-ark"}"#.to_string(),
        model_insert_mode: Default::default(),
        model_list: "glm-5.2[1M]\ndeepseek-v4-flash[1M]\nkimi-k2.6[262K]".to_string(),
        context_window: String::new(),
        auto_compact_limit: String::new(),
        ..RelayProfile::default()
    };

    apply_relay_profile_to_home_with_switch_rules_and_computer_use_guard(
        temp.path(),
        &profile,
        "",
        false,
    )
    .unwrap();

    let config = std::fs::read_to_string(temp.path().join("config.toml")).unwrap();
    // 后缀不得进入 config.toml 的 model 字段，否则 codex 无法匹配 catalog
    assert!(!config.contains("[1M]"));
    assert!(config.contains(r#"model = "deepseek-v4-flash""#));
    assert!(config.contains(r#"model_catalog_json = "model-catalogs/relay-ark.json""#));

    let catalog_path = temp.path().join("model-catalogs").join("relay-ark.json");
    assert!(catalog_path.exists());
    let catalog = std::fs::read_to_string(&catalog_path).unwrap();
    assert!(catalog.contains(r#""slug": "deepseek-v4-flash""#));
    assert!(catalog.contains(r#""context_window": 1000000"#));
    assert!(catalog.contains(r#""slug": "glm-5.2""#));
    assert!(catalog.contains(r#""context_window": 262000"#));
    assert!(!catalog.contains("[1M]"));
}

#[test]
fn apply_relay_profile_strips_suffix_from_config_contents_model() {
    let temp = tempfile::tempdir().unwrap();
    let profile = RelayProfile {
        id: "relay-ark".to_string(),
        name: "火山引擎 Ark".to_string(),
        // 用户可能在「配置模型」留空，只在 config_contents / 模型列表写后缀
        model: String::new(),
        relay_mode: RelayMode::PureApi,
        config_contents: r#"model = "glm-5.2[1M]"
model_provider = "custom"

[model_providers.custom]
name = "custom"
wire_api = "responses"
requires_openai_auth = true
base_url = "https://ark.cn-beijing.volces.com/api/coding/v3"
experimental_bearer_token = "sk-ark"
"#
        .to_string(),
        auth_contents: r#"{"OPENAI_API_KEY":"sk-ark"}"#.to_string(),
        model_insert_mode: Default::default(),
        model_list: "glm-5.2[1M]\ndeepseek-v4-flash[1M]".to_string(),
        context_window: String::new(),
        auto_compact_limit: String::new(),
        ..RelayProfile::default()
    };

    apply_relay_profile_to_home_with_switch_rules_and_computer_use_guard(
        temp.path(),
        &profile,
        "",
        false,
    )
    .unwrap();

    let config = std::fs::read_to_string(temp.path().join("config.toml")).unwrap();
    assert!(!config.contains("[1M]"));
    assert!(config.contains(r#"model = "glm-5.2""#));
    assert!(config.contains(r#"model_catalog_json = "model-catalogs/relay-ark.json""#));

    let catalog =
        std::fs::read_to_string(temp.path().join("model-catalogs").join("relay-ark.json")).unwrap();
    assert!(catalog.contains(r#""slug": "glm-5.2""#));
    assert!(catalog.contains(r#""context_window": 1000000"#));
    assert!(catalog.contains(r#""slug": "deepseek-v4-flash""#));
    assert!(!catalog.contains("[1M]"));
}

#[test]
fn apply_relay_profile_regenerates_existing_self_generated_catalog() {
    let temp = tempfile::tempdir().unwrap();
    let profile = RelayProfile {
        id: "relay-a".to_string(),
        name: "Relay A".to_string(),
        model: "deepseek-v4-pro".to_string(),
        relay_mode: RelayMode::PureApi,
        config_contents: r#"model = "deepseek-v4-pro"
model_provider = "custom"
model_catalog_json = "model-catalogs/relay-a.json"

[model_providers.custom]
name = "custom"
wire_api = "responses"
requires_openai_auth = true
base_url = "https://relay.example/v1"
experimental_bearer_token = "sk-new"
"#
        .to_string(),
        auth_contents: r#"{"OPENAI_API_KEY":"sk-new"}"#.to_string(),
        model_insert_mode: Default::default(),
        // 旧 catalog 是 200K，现在改成 1M，应被重新生成
        model_list: "deepseek-v4-pro[1M]".to_string(),
        ..RelayProfile::default()
    };

    // 先写入一个旧的、窗口错误的 catalog
    std::fs::create_dir_all(temp.path().join("model-catalogs")).unwrap();
    std::fs::write(
        temp.path().join("model-catalogs").join("relay-a.json"),
        r#"{"models":[{"slug":"deepseek-v4-pro","context_window":200000}]}"#,
    )
    .unwrap();

    apply_relay_profile_to_home_with_switch_rules_and_computer_use_guard(
        temp.path(),
        &profile,
        "",
        false,
    )
    .unwrap();

    let catalog =
        std::fs::read_to_string(temp.path().join("model-catalogs").join("relay-a.json")).unwrap();
    assert!(catalog.contains(r#""context_window": 1000000"#));
    assert!(!catalog.contains(r#""context_window": 200000"#));
}

#[test]
fn apply_relay_profile_uses_first_model_list_entry_when_model_empty() {
    let temp = tempfile::tempdir().unwrap();
    let profile = RelayProfile {
        id: "relay-a".to_string(),
        name: "Relay A".to_string(),
        model: String::new(),
        relay_mode: RelayMode::PureApi,
        config_contents: r#"model_provider = "custom"

[model_providers.custom]
name = "custom"
wire_api = "responses"
requires_openai_auth = true
base_url = "https://relay.example/v1"
experimental_bearer_token = "sk-new"
"#
        .to_string(),
        auth_contents: r#"{"OPENAI_API_KEY":"sk-new"}"#.to_string(),
        model_insert_mode: Default::default(),
        model_list: "deepseek/deepseek-v4-flash[1M]\nqwen/qwen3-coder".to_string(),
        ..RelayProfile::default()
    };

    apply_relay_profile_to_home_with_switch_rules_and_computer_use_guard(
        temp.path(),
        &profile,
        "",
        false,
    )
    .unwrap();

    let config = std::fs::read_to_string(temp.path().join("config.toml")).unwrap();
    // model 为空时，应取 model_list 第一条的 slug（剥离后缀）写入 config.toml
    assert!(config.contains(r#"model = "deepseek/deepseek-v4-flash""#));
    assert!(!config.contains("[1M]"));
    assert!(config.contains(r#"model_catalog_json = "model-catalogs/relay-a.json""#));
}

#[test]
fn relay_profile_default_has_empty_model_windows() {
    let profile = RelayProfile::default();
    assert_eq!(profile.model_windows, "");
}

fn sanitize(value: &str) -> String {
    value
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

#[test]
fn apply_model_catalog_uses_model_windows_map() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join(".codex");
    std::fs::create_dir_all(&home).unwrap();
    let profile = RelayProfile {
        id: "relay-windows".to_string(),
        name: "Relay Windows".to_string(),
        relay_mode: RelayMode::PureApi,
        config_contents: r#"model_provider = "custom"

[model_providers.custom]
name = "custom"
wire_api = "responses"
requires_openai_auth = true
base_url = "https://relay.example/v1"
experimental_bearer_token = "sk-new"
"#
        .to_string(),
        auth_contents: r#"{"OPENAI_API_KEY":"sk-new"}"#.to_string(),
        model_list: "deepseek-v4-flash\ndeepseek-v4-pro".to_string(),
        model_windows: r#"{"deepseek-v4-flash":"1M"}"#.to_string(),
        context_window: "200000".to_string(),
        ..RelayProfile::default()
    };

    apply_relay_profile_to_home_with_switch_rules_and_computer_use_guard(
        &home, &profile, "", false,
    )
    .unwrap();

    let catalog_path = home
        .join("model-catalogs")
        .join(format!("{}.json", sanitize(&profile.id)));
    let catalog: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(catalog_path).unwrap()).unwrap();
    let models = catalog["models"].as_array().unwrap();
    let flash = models
        .iter()
        .find(|m| m["slug"].as_str().unwrap() == "deepseek-v4-flash")
        .unwrap();
    let pro = models
        .iter()
        .find(|m| m["slug"].as_str().unwrap() == "deepseek-v4-pro")
        .unwrap();
    assert_eq!(flash["context_window"].as_u64().unwrap(), 1_000_000);
    assert_eq!(pro["context_window"].as_u64().unwrap(), 200_000);
}

#[test]
fn normalize_migrates_model_list_suffixes_to_model_windows() {
    let mut profile = RelayProfile {
        id: "relay-migrate".to_string(),
        name: "Migrate".to_string(),
        relay_mode: RelayMode::PureApi,
        config_contents: r#"model_provider = "custom"

[model_providers.custom]
name = "custom"
wire_api = "responses"
requires_openai_auth = true
base_url = "https://relay.example/v1"
experimental_bearer_token = "sk-new"
"#
        .to_string(),
        auth_contents: r#"{"OPENAI_API_KEY":"sk-new"}"#.to_string(),
        model_list: "deepseek-v4-flash[1M]\ndeepseek-v4-pro".to_string(),
        model_windows: String::new(),
        ..RelayProfile::default()
    };

    normalize_relay_profile_for_storage(&mut profile).unwrap();

    assert_eq!(profile.model_list, "deepseek-v4-flash\ndeepseek-v4-pro");
    let windows: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(&profile.model_windows).unwrap();
    assert_eq!(
        windows.get("deepseek-v4-flash").unwrap().as_str().unwrap(),
        "1000000"
    );
    assert!(!windows.contains_key("deepseek-v4-pro"));
}
