mod support;

use cc_switch_lib::{AppState, AppType, Database, McpServer, McpService, MultiAppConfig};
use serde_json::{json, Value};
use std::{fs, path::Path, sync::Arc};

fn prepare_home() -> &'static Path {
    support::reset_test_fs();
    let home = support::ensure_test_home();
    fs::create_dir_all(home.join(".claude")).unwrap();
    fs::create_dir_all(home.join(".codex")).unwrap();
    home
}

fn codex_server(home: &Path) -> Value {
    let text = fs::read_to_string(home.join(".codex/config.toml")).unwrap();
    let config: toml::Value = toml::from_str(&text).unwrap();
    serde_json::to_value(&config["mcp_servers"]["serena"]).unwrap()
}

fn independent_server() -> McpServer {
    serde_json::from_value(json!({
        "id":"serena","name":"Serena","tags":["team"],
        "server":{"command":"shared-command","args":["--shared"],"env":{"SHARED":"fixture"}},
        "appOverrides":{
            "claude":{"command":"claude-command","args":["--claude"]},
            "codex":{"type":"http","url":"https://example.invalid/mcp","headers":{"X-Team":"synthetic"}}
        },
        "apps":{"claude":true,"codex":true}
    })).unwrap()
}

fn claude_server(home: &Path) -> Value {
    let config: Value =
        serde_json::from_str(&fs::read_to_string(home.join(".claude.json")).unwrap()).unwrap();
    config["mcpServers"]["serena"].clone()
}

#[test]
fn claude_override_is_mirrored_to_account_profiles_on_save_toggle_and_delete() {
    let _guard = support::test_mutex()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let home = prepare_home();
    let data_dir = home.join(".cc-switch");
    let local_id = "0123456789abcdef0123456789abcdef";
    let config_dir = data_dir
        .join("claude-accounts")
        .join(local_id)
        .join("config");
    fs::create_dir_all(&config_dir).unwrap();
    fs::write(
        data_dir.join("claude_accounts.json"),
        json!({"version":1,"accounts":{"fixture-account":{"localId":local_id,"addedAt":0}}})
            .to_string(),
    )
    .unwrap();
    let profile = config_dir.join(".claude.json");
    fs::write(&profile, json!({"mcpServers":{"account-only":{"command":"keep"}},"oauthAccount":{"accountUuid":"synthetic"}}).to_string()).unwrap();
    let read_profile =
        || -> Value { serde_json::from_slice(&fs::read(&profile).unwrap()).unwrap() };
    let state = AppState::new(Arc::new(Database::memory().unwrap()));
    McpService::upsert_server(&state, independent_server()).unwrap();
    assert_eq!(read_profile()["mcpServers"]["serena"], claude_server(home));
    assert_eq!(
        read_profile()["mcpServers"]["serena"]["command"],
        "claude-command"
    );
    let mut server = independent_server();
    server.app_overrides.claude.as_mut().unwrap()["command"] = json!("claude-updated");
    McpService::upsert_server(&state, server).unwrap();
    assert_eq!(
        read_profile()["mcpServers"]["serena"]["command"],
        "claude-updated"
    );
    McpService::toggle_app(&state, "serena", AppType::Claude, false).unwrap();
    assert!(read_profile()["mcpServers"].get("serena").is_none());
    McpService::toggle_app(&state, "serena", AppType::Claude, true).unwrap();
    assert_eq!(
        read_profile()["mcpServers"]["serena"]["command"],
        "claude-updated"
    );
    McpService::delete_server(&state, "serena").unwrap();
    assert!(read_profile()["mcpServers"].get("serena").is_none());
    assert_eq!(
        read_profile()["mcpServers"]["account-only"]["command"],
        "keep"
    );
    assert_eq!(read_profile()["oauthAccount"]["accountUuid"], "synthetic");
}

#[test]
fn codex_import_does_not_detach_defaults_after_filtering_foreign_fields() {
    let _guard = support::test_mutex()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let home = prepare_home();
    let state = AppState::new(Arc::new(Database::memory().unwrap()));
    let mut server = independent_server();
    server.app_overrides = Default::default();
    server.server = json!({"command":"shared-tool","timeout":900000,"autoApprove":["read"],"tool_timeout_sec":120});
    McpService::upsert_server(&state, server.clone()).unwrap();
    let native = codex_server(home);
    assert!(native.get("timeout").is_none());
    assert!(native.get("autoApprove").is_none());
    assert_eq!(native["tool_timeout_sec"], 120);
    McpService::import_from_codex(&state).unwrap();
    McpService::import_from_codex(&state).unwrap();
    let mut saved = McpService::get_all_servers(&state).unwrap()["serena"].clone();
    assert!(saved.app_overrides.codex.is_none());
    assert_eq!(saved.server, server.server);
    saved.server["command"] = json!("shared-updated");
    McpService::upsert_server(&state, saved).unwrap();
    assert_eq!(codex_server(home)["command"], "shared-updated");
    assert_eq!(codex_server(home)["tool_timeout_sec"], 120);
}

fn assert_invalid_restore_is_rejected(kind: &str) {
    let _guard = support::test_mutex()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    for invalid in [
        "{",
        r#"{"unsupported-agent":{"command":"x"}}"#,
        r#"{"codex":{}}"#,
    ] {
        let home = prepare_home();
        let state = AppState::new(Arc::new(Database::init().unwrap()));
        McpService::upsert_server(&state, independent_server()).unwrap();
        let before_db = fs::read(home.join(".cc-switch/cc-switch.db")).unwrap();
        let before_claude = fs::read(home.join(".claude.json")).unwrap();
        let before_codex = fs::read(home.join(".codex/config.toml")).unwrap();
        let sql = state.db.export_sql_string().unwrap();
        let malformed = format!("{sql}\nUPDATE mcp_servers SET app_overrides='{invalid}';\n");
        let result = match kind {
            "sql" => state.db.import_sql_string(&malformed),
            "sqlite" => {
                let dir = home.join(".cc-switch/backups");
                fs::create_dir_all(&dir).unwrap();
                let source = rusqlite::Connection::open(dir.join("invalid-mcp.db")).unwrap();
                source.execute_batch(&malformed).unwrap();
                drop(source);
                state.db.restore_from_backup("invalid-mcp.db")
            }
            _ => unreachable!(),
        };
        assert!(
            result.is_err(),
            "{kind} accepted invalid overrides: {invalid}"
        );
        assert_eq!(
            fs::read(home.join(".cc-switch/cc-switch.db")).unwrap(),
            before_db
        );
        assert_eq!(fs::read(home.join(".claude.json")).unwrap(), before_claude);
        assert_eq!(
            fs::read(home.join(".codex/config.toml")).unwrap(),
            before_codex
        );
        assert_eq!(
            McpService::get_all_servers(&state).unwrap()["serena"].name,
            "Serena"
        );
    }
}

#[test]
fn invalid_overrides_in_sql_restore_preserve_the_current_database() {
    assert_invalid_restore_is_rejected("sql");
}

#[test]
fn invalid_overrides_in_sqlite_restore_preserve_the_current_database() {
    assert_invalid_restore_is_rejected("sqlite");
}

#[test]
fn repeated_codex_import_prefers_standard_table_over_legacy_table() {
    let _guard = support::test_mutex()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let home = prepare_home();
    let state = AppState::new(Arc::new(Database::memory().unwrap()));
    fs::write(
        home.join(".codex/config.toml"),
        "[mcp.servers.serena]\ncommand='legacy'\n[mcp_servers.serena]\ncommand='current'\n",
    )
    .unwrap();
    McpService::import_from_codex(&state).unwrap();
    McpService::import_from_codex(&state).unwrap();
    McpService::sync_enabled_for_app(&state, &AppType::Codex).unwrap();
    assert_eq!(codex_server(home)["command"], "current");
}

#[test]
fn contradictory_explicit_transport_is_rejected_before_writing() {
    let _guard = support::test_mutex()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let home = prepare_home();
    let state = AppState::new(Arc::new(Database::memory().unwrap()));
    McpService::upsert_server(&state, independent_server()).unwrap();
    let before = fs::read(home.join(".claude.json")).unwrap();
    let mut server = independent_server();
    server.app_overrides.claude = Some(json!({"type":"stdio","url":"https://example.invalid/mcp"}));
    assert!(McpService::upsert_server(&state, server).is_err());
    assert_eq!(fs::read(home.join(".claude.json")).unwrap(), before);
}

#[test]
fn independent_specs_survive_reopen_toggle_reset_and_delete() {
    let _guard = support::test_mutex()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let home = prepare_home();
    fs::write(
        home.join(".claude.json"),
        r#"{"mcpServers":{"unmanaged":{"command":"keep-me"}}}"#,
    )
    .unwrap();
    fs::write(
        home.join(".codex/config.toml"),
        "model = 'keep-model'\n[mcp_servers.unmanaged]\ncommand = 'keep-me'\n",
    )
    .unwrap();
    {
        let state = AppState::new(Arc::new(Database::init().unwrap()));
        McpService::upsert_server(&state, independent_server()).unwrap();
    }
    let state = AppState::new(Arc::new(Database::init().unwrap()));
    let saved = McpService::get_all_servers(&state).unwrap()["serena"].clone();
    assert_eq!(saved.app_overrides, independent_server().app_overrides);
    assert_eq!(saved.server_for_app(&AppType::Gemini), &saved.server);
    let claude_before = fs::read(home.join(".claude.json")).unwrap();
    let codex = codex_server(home);
    assert_eq!(codex["url"], "https://example.invalid/mcp");
    assert_eq!(codex["http_headers"]["X-Team"], "synthetic");
    assert!(codex.get("command").is_none() && codex.get("env").is_none());
    assert!(claude_server(home).get("env").is_none());

    McpService::toggle_app(&state, "serena", AppType::Codex, false).unwrap();
    assert!(!fs::read_to_string(home.join(".codex/config.toml"))
        .unwrap()
        .contains("mcp_servers.serena"));
    assert_eq!(fs::read(home.join(".claude.json")).unwrap(), claude_before);
    McpService::toggle_app(&state, "serena", AppType::Codex, true).unwrap();
    assert_eq!(codex_server(home), codex);
    let mut edited = McpService::get_all_servers(&state).unwrap()["serena"].clone();
    edited.server["command"] = json!("updated-shared-command");
    McpService::upsert_server(&state, edited.clone()).unwrap();
    assert_eq!(codex_server(home), codex);
    assert_eq!(claude_server(home)["command"], "claude-command");

    edited.app_overrides.codex = None;
    McpService::upsert_server(&state, edited).unwrap();
    let codex = codex_server(home);
    assert_eq!(codex["command"], "updated-shared-command");
    assert!(codex.get("url").is_none() && codex.get("http_headers").is_none());
    assert_eq!(claude_server(home)["command"], "claude-command");
    assert!(McpService::delete_server(&state, "serena").unwrap());
    for file in [".claude.json", ".codex/config.toml"] {
        let text = fs::read_to_string(home.join(file)).unwrap();
        assert!(text.contains("unmanaged"));
        assert!(!text.contains("serena"));
        assert!(!text.contains("appOverrides"));
    }
}

#[test]
fn invalid_overrides_fail_before_any_database_or_live_write() {
    let _guard = support::test_mutex()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let home = prepare_home();
    let state = AppState::new(Arc::new(Database::memory().unwrap()));
    McpService::upsert_server(&state, independent_server()).unwrap();
    let original =
        serde_json::to_value(&McpService::get_all_servers(&state).unwrap()["serena"]).unwrap();
    let claude_before = fs::read(home.join(".claude.json")).unwrap();
    let codex_before = fs::read(home.join(".codex/config.toml")).unwrap();
    for invalid in [
        json!({}),
        json!({"command":"uvx","args":[2]}),
        json!({"type":"http","url":""}),
        json!({"command":"uvx","env":{"KEY":null}}),
    ] {
        let mut edited = independent_server();
        edited.name = "must not persist".into();
        edited.app_overrides.codex = Some(invalid);
        assert!(McpService::upsert_server(&state, edited.clone()).is_err());
        assert!(state.db.save_mcp_server(&edited).is_err());
        assert_eq!(
            serde_json::to_value(&McpService::get_all_servers(&state).unwrap()["serena"]).unwrap(),
            original
        );
        assert_eq!(fs::read(home.join(".claude.json")).unwrap(), claude_before);
        assert_eq!(
            fs::read(home.join(".codex/config.toml")).unwrap(),
            codex_before
        );
    }
    let mut unsupported = original;
    unsupported["appOverrides"]["unsupported-agent"] = json!({"command":"unsupported"});
    assert!(serde_json::from_value::<McpServer>(unsupported).is_err());
}

#[test]
fn json_migration_and_sql_restore_preserve_independent_configs() {
    let _guard = support::test_mutex()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let home = prepare_home();
    let mut config = MultiAppConfig::default();
    config.mcp.servers = None;
    config.mcp.claude.servers.insert(
        "serena".into(),
        json!({"enabled":true,"server":{"command":"claude-command"}}),
    );
    config.mcp.codex.servers.insert(
        "serena".into(),
        json!({"enabled":true,"server":{"command":"codex-command"}}),
    );
    assert!(config.migrate_mcp_to_unified().unwrap());
    let db = Database::memory().unwrap();
    db.migrate_from_json(&config).unwrap();
    for sql in [
        db.export_sql_string().unwrap(),
        db.export_sql_string_for_sync().unwrap(),
    ] {
        let restored = Database::memory().unwrap();
        restored.import_sql_string(&sql).unwrap();
        let state = AppState::new(Arc::new(restored));
        McpService::sync_all_enabled(&state).unwrap();
        assert_eq!(claude_server(home)["command"], "claude-command");
        assert_eq!(codex_server(home)["command"], "codex-command");
        assert_eq!(McpService::get_all_servers(&state).unwrap().len(), 1);
    }
}

#[test]
fn reverse_import_and_reimport_change_only_the_imported_app() {
    let _guard = support::test_mutex()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let home = prepare_home();
    fs::write(
        home.join(".codex/config.toml"),
        "[mcp_servers.serena]\ncommand = 'codex-command'\n",
    )
    .unwrap();
    fs::write(
        home.join(".claude.json"),
        json!({"mcpServers":{"serena":{"command":"claude-command"}}}).to_string(),
    )
    .unwrap();
    let state = AppState::new(Arc::new(Database::memory().unwrap()));
    assert_eq!(McpService::import_from_codex(&state).unwrap(), 1);
    assert_eq!(McpService::import_from_claude(&state).unwrap(), 0);
    McpService::sync_all_enabled(&state).unwrap();
    assert_eq!(claude_server(home)["command"], "claude-command");
    assert_eq!(codex_server(home)["command"], "codex-command");
    fs::write(
        home.join(".claude.json"),
        json!({"mcpServers":{"serena":{"command":"claude-updated"}}}).to_string(),
    )
    .unwrap();
    assert_eq!(McpService::import_from_claude(&state).unwrap(), 0);
    McpService::sync_all_enabled(&state).unwrap();
    let saved = McpService::get_all_servers(&state).unwrap();
    assert_eq!(saved.len(), 1);
    assert_eq!(saved["serena"].server["command"], "codex-command");
    assert_eq!(claude_server(home)["command"], "claude-updated");
    assert_eq!(codex_server(home)["command"], "codex-command");
}

#[test]
fn equivalent_native_shapes_keep_following_the_shared_default() {
    let _guard = support::test_mutex()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let home = prepare_home();
    let state = AppState::new(Arc::new(Database::memory().unwrap()));
    fs::write(
        home.join(".claude.json"),
        json!({"mcpServers":{"serena":{"command":"shared","args":[],"env":{}}}}).to_string(),
    )
    .unwrap();
    fs::write(
        home.join(".codex/config.toml"),
        "[mcp_servers.serena]\ncommand = 'shared'\n",
    )
    .unwrap();
    McpService::import_from_claude(&state).unwrap();
    McpService::import_from_codex(&state).unwrap();
    let mut saved = McpService::get_all_servers(&state).unwrap()["serena"].clone();
    assert!(saved.app_overrides.claude.is_none() && saved.app_overrides.codex.is_none());
    saved.server["command"] = json!("shared-updated");
    McpService::upsert_server(&state, saved).unwrap();
    assert_eq!(claude_server(home)["command"], "shared-updated");
    assert_eq!(codex_server(home)["command"], "shared-updated");
}

#[test]
fn reimport_does_not_clear_an_explicit_override_that_matches_the_default() {
    let _guard = support::test_mutex()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let home = prepare_home();
    let state = AppState::new(Arc::new(Database::memory().unwrap()));
    let mut server = independent_server();
    server.app_overrides.codex = Some(server.server.clone());
    McpService::upsert_server(&state, server).unwrap();
    McpService::import_from_codex(&state).unwrap();
    let mut saved = McpService::get_all_servers(&state).unwrap()["serena"].clone();
    assert!(saved.app_overrides.codex.is_some());
    saved.server["command"] = json!("new-shared-command");
    McpService::upsert_server(&state, saved).unwrap();
    assert_eq!(codex_server(home)["command"], "shared-command");
}

#[test]
fn same_id_imports_retain_each_apps_arguments_after_resync() {
    let _guard = support::test_mutex()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let home = prepare_home();
    let claude = json!({"command":"uvx","args":["serena","--context","claude-code"]});
    fs::write(
        home.join(".claude.json"),
        json!({"mcpServers":{"serena":claude}}).to_string(),
    )
    .unwrap();
    fs::write(
        home.join(".codex/config.toml"),
        "[mcp_servers.serena]\ncommand = 'uvx'\nargs = ['serena', '--context', 'codex']\n",
    )
    .unwrap();
    let before = codex_server(home);
    let state = AppState::new(Arc::new(Database::memory().unwrap()));
    assert_eq!(McpService::import_from_claude(&state).unwrap(), 1);
    assert_eq!(McpService::import_from_codex(&state).unwrap(), 0);
    let saved = McpService::get_all_servers(&state).unwrap();
    assert_eq!(saved.len(), 1);
    assert!(saved["serena"].apps.claude && saved["serena"].apps.codex);
    // Import must not write either live file.
    assert_eq!(codex_server(home), before);
    McpService::sync_enabled_for_app(&state, &AppType::Codex).unwrap();
    let after = codex_server(home);
    if let Ok(dir) = std::env::var("CC_SWITCH_7997_EVIDENCE_DIR") {
        fs::write(
            Path::new(&dir).join("import-resync.json"),
            serde_json::to_vec_pretty(&json!({
                "claude":claude,"codexBefore":before,"codexAfter":after,"saved":saved
            }))
            .unwrap(),
        )
        .unwrap();
    }
    assert_eq!(after["args"], before["args"]);
}

#[test]
fn explicit_app_configuration_survives_storage_and_projection() {
    let _guard = support::test_mutex()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let home = prepare_home();
    let state = AppState::new(Arc::new(Database::memory().unwrap()));
    let server: McpServer = serde_json::from_value(json!({
        "id":"serena","name":"Serena",
        "server":{"command":"uvx","args":["serena","--context","shared"]},
        "appOverrides":{
            "claude":{"command":"uvx","args":["serena","--context","claude-code"]},
            "codex":{"command":"uvx","args":["serena","--context","codex"]}
        },
        "apps":{"claude":true,"codex":true}
    }))
    .unwrap();
    McpService::upsert_server(&state, server).unwrap();
    let stored =
        serde_json::to_value(&McpService::get_all_servers(&state).unwrap()["serena"]).unwrap();
    assert_eq!(stored["appOverrides"]["codex"]["args"][2], "codex");
    assert_eq!(codex_server(home)["args"][2], "codex");
    let claude: Value =
        serde_json::from_str(&fs::read_to_string(home.join(".claude.json")).unwrap()).unwrap();
    assert_eq!(claude["mcpServers"]["serena"]["args"][2], "claude-code");
}
