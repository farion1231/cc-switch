mod support;

use cc_switch_lib::{
    update_settings, AppSettings, AppState, AppType, Database, McpServer, McpService,
    MultiAppConfig,
};
use serde_json::{json, Value};
use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

fn apps() -> [AppType; 12] {
    [
        AppType::Claude,
        AppType::Codex,
        AppType::Gemini,
        AppType::GrokBuild,
        AppType::OpenCode,
        AppType::Hermes,
        AppType::Pi,
        AppType::Mcode,
        AppType::Kimi,
        AppType::Dsh,
        AppType::Agy,
        AppType::Omp,
    ]
}

struct Fixture {
    home: &'static Path,
    env: Vec<(&'static str, Option<OsString>)>,
}

impl Fixture {
    fn new() -> Self {
        support::reset_test_fs();
        let home = support::ensure_test_home();
        for directory in [".hermes", ".pi", ".omp", ".minimax", ".kimi-code", ".dsh"] {
            let path = home.join(directory);
            if path.exists() {
                fs::remove_dir_all(path).unwrap();
            }
        }
        let env = ["KIMI_CODE_HOME", "MINIMAX_DATA_DIR", "DSH_HOME"]
            .into_iter()
            .map(|key| (key, std::env::var_os(key)))
            .collect();
        std::env::set_var("KIMI_CODE_HOME", home.join(".kimi-code"));
        std::env::set_var("MINIMAX_DATA_DIR", home.join(".minimax"));
        std::env::set_var("DSH_HOME", home.join(".dsh"));
        update_settings(AppSettings {
            pi_config_dir: Some(home.join(".pi/agent").to_string_lossy().into_owned()),
            omp_config_dir: Some(home.join(".omp/agent").to_string_lossy().into_owned()),
            hermes_config_dir: Some(home.join(".hermes").to_string_lossy().into_owned()),
            opencode_config_dir: Some(home.join(".config/opencode").to_string_lossy().into_owned()),
            ..Default::default()
        })
        .unwrap();
        let fixture = Self { home, env };
        fs::create_dir_all(home.join(".claude")).unwrap();
        for app in apps() {
            fs::create_dir_all(fixture.path(&app).parent().unwrap()).unwrap();
        }
        fixture
    }

    fn path(&self, app: &AppType) -> PathBuf {
        self.home.join(match app {
            AppType::Claude => ".claude.json",
            AppType::Codex => ".codex/config.toml",
            AppType::Gemini => ".gemini/settings.json",
            AppType::GrokBuild => ".grok/config.toml",
            AppType::OpenCode => ".config/opencode/opencode.json",
            AppType::Hermes => ".hermes/config.yaml",
            AppType::Pi => ".pi/agent/mcp.json",
            AppType::Mcode => ".minimax/mcp.json",
            AppType::Kimi => ".kimi-code/mcp.json",
            AppType::Dsh => ".dsh/profiles/web/cordis.patch.yml",
            AppType::Agy => ".gemini/config/mcp_config.json",
            AppType::Omp => ".omp/agent/mcp.json",
            _ => unreachable!(),
        })
    }

    fn read(&self, app: &AppType) -> Value {
        let text = fs::read_to_string(self.path(app)).unwrap();
        let document: Value = match app {
            AppType::Codex | AppType::GrokBuild => {
                serde_json::to_value(toml::from_str::<toml::Value>(&text).unwrap()).unwrap()
            }
            AppType::Hermes | AppType::Dsh => serde_yaml::from_str(&text).unwrap(),
            _ => serde_json::from_str(&text).unwrap(),
        };
        if *app == AppType::Dsh {
            return document
                .as_array()
                .unwrap()
                .iter()
                .flat_map(|entry| {
                    std::iter::once(entry).chain(entry["insert"].as_array().into_iter().flatten())
                })
                .find(|entry| entry["config"]["serverName"] == "shared")
                .map(|entry| entry["config"].clone())
                .unwrap_or(Value::Null);
        }
        let map = match app {
            AppType::Codex | AppType::GrokBuild | AppType::Hermes => "mcp_servers",
            AppType::OpenCode => "mcp",
            _ => "mcpServers",
        };
        document[map]["shared"].clone()
    }

    fn seed(&self, app: &AppType) {
        let mut spec = spec_for(app);
        let document = match app {
            AppType::OpenCode => {
                json!({"mcp":{"shared":{"type":"local","command":[spec["command"],"--context",app.as_str(),""],"environment":spec["env"],"enabled":true}}})
            }
            AppType::Codex | AppType::GrokBuild | AppType::Hermes => {
                spec.as_object_mut().unwrap().remove("type");
                json!({"mcp_servers":{"shared":spec}})
            }
            AppType::Dsh => {
                spec.as_object_mut().unwrap().remove("type");
                spec["serverName"] = json!("shared");
                spec["transport"] = json!("stdio");
                json!([{"id":"native-shared","name":"@deepseek-ai/dsh-mcp-client","config":spec}])
            }
            AppType::Kimi => {
                spec.as_object_mut().unwrap().remove("type");
                spec["transport"] = json!("stdio");
                json!({"mcpServers":{"shared":spec}})
            }
            _ => json!({"mcpServers":{"shared":spec}}),
        };
        let text = match app {
            AppType::Codex | AppType::GrokBuild => toml::to_string(&document).unwrap(),
            AppType::Hermes | AppType::Dsh => serde_yaml::to_string(&document).unwrap(),
            _ => document.to_string(),
        };
        fs::write(self.path(app), text).unwrap();
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        for (key, previous) in &self.env {
            match previous {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
    }
}

fn spec_for(app: &AppType) -> Value {
    json!({"type":"stdio","command":format!("fixture-{}",app.as_str()),"args":["--context",app.as_str(),""],"env":{"TEAM_CONTEXT":app.as_str()}})
}

fn all_app_server() -> McpServer {
    let mut value = json!({"id":"shared","name":"Team MCP","server":{"command":"shared-default","args":["default"]},"apps":{},"appOverrides":{}});
    for app in apps() {
        value["apps"][app.as_str()] = json!(true);
        value["appOverrides"][app.as_str()] = spec_for(&app);
    }
    serde_json::from_value(value).unwrap()
}

fn assert_native(fixture: &Fixture, app: &AppType, expected: &Value) {
    let native = fixture.read(app);
    if *app == AppType::OpenCode {
        assert_eq!(
            native["command"][0],
            expected["command"],
            "{} command",
            app.as_str()
        );
        assert_eq!(
            json!(&native["command"].as_array().unwrap()[1..]),
            expected["args"],
            "{} args",
            app.as_str()
        );
        assert_eq!(
            native["environment"],
            expected["env"],
            "{} env",
            app.as_str()
        );
    } else {
        assert_eq!(
            native["command"],
            expected["command"],
            "{} command",
            app.as_str()
        );
        assert_eq!(native["args"], expected["args"], "{} args", app.as_str());
        assert_eq!(native["env"], expected["env"], "{} env", app.as_str());
    }
    assert!(native.get("appOverrides").is_none());
}

#[test]
fn every_managed_app_uses_its_own_spec_through_save_toggle_restore_and_delete() {
    let _guard = support::test_mutex()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let fixture = Fixture::new();
    let state = AppState::new(Arc::new(Database::memory().unwrap()));
    McpService::upsert_server(&state, all_app_server()).unwrap();
    let mut evidence = serde_json::Map::new();
    for app in apps() {
        assert_native(&fixture, &app, &spec_for(&app));
        evidence.insert(app.as_str().into(), fixture.read(&app));
        McpService::toggle_app(&state, "shared", app.clone(), false).unwrap();
        let disabled = fixture.read(&app);
        assert!(
            disabled.is_null() || disabled["enabled"] == false || disabled["disabled"] == true,
            "{} not disabled",
            app.as_str()
        );
        McpService::toggle_app(&state, "shared", app.clone(), true).unwrap();
        assert_native(&fixture, &app, &spec_for(&app));
    }
    let restored = Database::memory().unwrap();
    restored
        .import_sql_string(&state.db.export_sql_string_for_sync().unwrap())
        .unwrap();
    let restored = AppState::new(Arc::new(restored));
    McpService::sync_all_enabled(&restored).unwrap();
    for app in apps() {
        assert_native(&fixture, &app, &spec_for(&app));
    }
    assert!(McpService::delete_server(&restored, "shared").unwrap());
    for app in apps() {
        assert!(
            fixture.read(&app).is_null(),
            "{} still present",
            app.as_str()
        );
    }
    if let Ok(dir) = std::env::var("CC_SWITCH_7997_EVIDENCE_DIR") {
        fs::write(
            Path::new(&dir).join("agent-matrix.json"),
            serde_json::to_vec_pretty(&evidence).unwrap(),
        )
        .unwrap();
        fs::write(
            Path::new(&dir).join("agent-record.json"),
            serde_json::to_vec_pretty(&state.db.get_all_mcp_servers().unwrap()["shared"]).unwrap(),
        )
        .unwrap();
    }
}

#[test]
fn importing_distinct_native_specs_keeps_one_id_without_writing_source_files() {
    let _guard = support::test_mutex()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let fixture = Fixture::new();
    let state = AppState::new(Arc::new(Database::memory().unwrap()));
    let mut before = Vec::new();
    for app in apps() {
        fixture.seed(&app);
        before.push((app.clone(), fs::read(fixture.path(&app)).unwrap()));
    }
    assert_eq!(McpService::import_from_all_apps(&state).unwrap(), 1);
    assert_eq!(McpService::import_from_all_apps(&state).unwrap(), 0);
    assert_eq!(McpService::get_all_servers(&state).unwrap().len(), 1);
    for (app, bytes) in before {
        assert_eq!(
            fs::read(fixture.path(&app)).unwrap(),
            bytes,
            "{} import writes",
            app.as_str()
        );
    }
    McpService::sync_all_enabled(&state).unwrap();
    for app in apps() {
        assert_native(&fixture, &app, &spec_for(&app));
    }
}

#[test]
fn json_migration_preserves_every_apps_flag_and_independent_spec() {
    let _guard = support::test_mutex()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let _fixture = Fixture::new();
    let original = all_app_server();
    let mut config = MultiAppConfig::default();
    config.mcp.servers = Some(std::collections::HashMap::from([(
        "shared".into(),
        original.clone(),
    )]));
    let db = Database::memory().unwrap();
    db.migrate_from_json(&config).unwrap();
    let saved = db.get_all_mcp_servers().unwrap()["shared"].clone();
    assert_eq!(saved.app_overrides, original.app_overrides);
    assert_eq!(saved.apps, original.apps);
}

#[test]
fn replacing_stdio_with_http_clears_every_old_connection_field() {
    let _guard = support::test_mutex()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let fixture = Fixture::new();
    let state = AppState::new(Arc::new(Database::memory().unwrap()));
    let mut server = serde_json::to_value(all_app_server()).unwrap();
    for app in apps() {
        server["appOverrides"][app.as_str()]["cwd"] = json!("/isolated/old-workspace");
    }
    McpService::upsert_server(&state, serde_json::from_value(server.clone()).unwrap()).unwrap();
    let mut stale = Vec::new();
    for app in apps() {
        server["appOverrides"][app.as_str()] = json!({"type":"http","url":format!("https://{}.example.invalid/mcp",app.as_str()),"headers":{"X-Team":app.as_str()}});
        McpService::upsert_server(&state, serde_json::from_value(server.clone()).unwrap()).unwrap();
        let native = fixture.read(&app);
        let url_field = if app == AppType::Gemini {
            "httpUrl"
        } else {
            "url"
        };
        assert_eq!(
            native[url_field],
            server["appOverrides"][app.as_str()]["url"]
        );
        let headers = if app == AppType::Codex {
            "http_headers"
        } else {
            "headers"
        };
        assert_eq!(native[headers]["X-Team"], app.as_str());
        if ["command", "args", "env", "environment", "cwd"]
            .iter()
            .any(|field| native.get(field).is_some())
        {
            stale.push(app.as_str().to_string());
        }
    }
    assert!(
        stale.is_empty(),
        "old connection fields survived for {stale:?}"
    );
}

#[test]
fn pi_disabled_config_edits_keep_the_owned_tombstone_in_sync() {
    let _guard = support::test_mutex()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    for disable_first in [true, false] {
        let fixture = Fixture::new();
        let state = AppState::new(Arc::new(Database::memory().unwrap()));
        McpService::upsert_server(&state, all_app_server()).unwrap();
        if disable_first {
            McpService::toggle_app(&state, "shared", AppType::Pi, false).unwrap();
        }
        let mut edited = McpService::get_all_servers(&state).unwrap()["shared"].clone();
        edited.apps.pi = false;
        edited.app_overrides.pi.as_mut().unwrap()["command"] = json!("pi-edited-while-off");
        McpService::upsert_server(&state, edited).unwrap();
        let native = fixture.read(&AppType::Pi);
        assert_eq!(native["enabled"], false);
        assert_eq!(native["command"], "pi-edited-while-off");
        McpService::delete_server(&state, "shared").unwrap();
        assert!(fixture.read(&AppType::Pi).is_null());
    }
}

#[test]
fn native_roundtrips_do_not_detach_shared_connections() {
    let _guard = support::test_mutex()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let mut detached = Vec::new();
    for app in apps() {
        for transport in ["stdio", "http"] {
            let fixture = Fixture::new();
            let state = AppState::new(Arc::new(Database::memory().unwrap()));
            let spec = if transport == "stdio" {
                json!({"command":"shared-command","cwd":"/isolated/workspace"})
            } else {
                json!({"type":"http","url":"https://example.invalid/shared"})
            };
            let mut raw = json!({"id":"shared","name":"shared","server":spec,"apps":{}});
            raw["apps"][app.as_str()] = json!(true);
            McpService::upsert_server(&state, serde_json::from_value(raw).unwrap()).unwrap();
            McpService::import_from_all_apps(&state).unwrap();
            let mut saved = McpService::get_all_servers(&state).unwrap()["shared"].clone();
            if saved.app_overrides.get(&app).is_some() {
                detached.push(format!("{}:{transport}", app.as_str()));
                continue;
            }
            if transport == "stdio" {
                saved.server["command"] = json!("shared-updated");
            } else {
                saved.server["url"] = json!("https://example.invalid/updated");
            }
            McpService::upsert_server(&state, saved).unwrap();
            let native = fixture.read(&app);
            if transport == "http" {
                assert_eq!(
                    native[if app == AppType::Gemini {
                        "httpUrl"
                    } else {
                        "url"
                    }],
                    "https://example.invalid/updated"
                );
            } else if app == AppType::OpenCode {
                assert_eq!(native["command"][0], "shared-updated");
            } else {
                assert_eq!(native["command"], "shared-updated");
            }
        }
    }
    assert!(
        detached.is_empty(),
        "projection-only differences detached {detached:?}"
    );
}

#[test]
fn omp_import_ownership_uses_the_same_empty_collection_semantics() {
    let _guard = support::test_mutex()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let fixture = Fixture::new();
    let state = AppState::new(Arc::new(Database::memory().unwrap()));
    let server: McpServer = serde_json::from_value(
        json!({"id":"shared","name":"shared","server":{"command":"runner"},"apps":{}}),
    )
    .unwrap();
    state.db.save_mcp_server(&server).unwrap();
    fs::write(
        fixture.path(&AppType::Omp),
        json!({"mcpServers":{"shared":{"command":"runner","args":[],"env":{},"enabled":true}}})
            .to_string(),
    )
    .unwrap();
    McpService::import_from_all_apps(&state).unwrap();
    McpService::sync_enabled_for_app(&state, &AppType::Omp).unwrap();
    McpService::toggle_app(&state, "shared", AppType::Omp, false).unwrap();
    McpService::delete_server(&state, "shared").unwrap();
    assert!(fixture.read(&AppType::Omp).is_null());
}

#[test]
fn gemini_independent_native_timeout_is_preserved() {
    let _guard = support::test_mutex()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let fixture = Fixture::new();
    let state = AppState::new(Arc::new(Database::memory().unwrap()));
    let mut server = all_app_server();
    server.app_overrides.gemini.as_mut().unwrap()["timeout"] = json!(45000);
    McpService::upsert_server(&state, server).unwrap();
    assert_eq!(fixture.read(&AppType::Gemini)["timeout"], 45000);
}

#[test]
fn importing_changed_native_options_updates_the_restorable_app_spec() {
    let _guard = support::test_mutex()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let mut stale = Vec::new();
    for app in [
        AppType::Mcode,
        AppType::Kimi,
        AppType::Dsh,
        AppType::Agy,
        AppType::Pi,
        AppType::Omp,
    ] {
        let fixture = Fixture::new();
        let state = AppState::new(Arc::new(Database::memory().unwrap()));
        let mut raw = serde_json::to_value(all_app_server()).unwrap();
        raw["appOverrides"][app.as_str()]["timeout"] = json!(5000);
        McpService::upsert_server(&state, serde_json::from_value(raw).unwrap()).unwrap();
        let path = fixture.path(&app);
        if app == AppType::Dsh {
            let mut native: Value =
                serde_yaml::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
            for patch in native.as_array_mut().unwrap() {
                if let Some(inserted) = patch.get_mut("insert").and_then(Value::as_array_mut) {
                    for entry in inserted {
                        if entry["config"]["serverName"] == "shared" {
                            entry["config"]["timeout"] = json!(9000);
                        }
                    }
                } else if patch["config"]["serverName"] == "shared" {
                    patch["config"]["timeout"] = json!(9000);
                }
            }
            fs::write(&path, serde_yaml::to_string(&native).unwrap()).unwrap();
        } else {
            let mut native: Value =
                serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
            native["mcpServers"]["shared"]["timeout"] = json!(9000);
            fs::write(&path, native.to_string()).unwrap();
        }
        McpService::import_from_all_apps(&state).unwrap();
        let saved = McpService::get_all_servers(&state).unwrap()["shared"].clone();
        if saved.server_for_app(&app)["timeout"] != 9000 {
            stale.push(app.as_str().to_string());
        }
        let restored = Database::memory().unwrap();
        restored
            .import_sql_string(&state.db.export_sql_string().unwrap())
            .unwrap();
        fs::remove_file(&path).unwrap();
        McpService::sync_enabled_for_app(&AppState::new(Arc::new(restored)), &app).unwrap();
        if fixture.read(&app)["timeout"] != 9000 {
            stale.push(format!("{} restore", app.as_str()));
        }
    }
    assert!(
        stale.is_empty(),
        "native options were not adopted: {stale:?}"
    );
}

#[test]
fn claude_native_projection_metadata_does_not_detach_the_default() {
    let _guard = support::test_mutex()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let fixture = Fixture::new();
    let state = AppState::new(Arc::new(Database::memory().unwrap()));
    let server: McpServer = serde_json::from_value(json!({"id":"shared","name":"shared","server":{"command":"runner","description":"other-client metadata"},"apps":{"claude":true}})).unwrap();
    McpService::upsert_server(&state, server).unwrap();
    McpService::import_from_claude(&state).unwrap();
    let mut saved = McpService::get_all_servers(&state).unwrap()["shared"].clone();
    assert!(saved.app_overrides.claude.is_none());
    saved.server["command"] = json!("updated");
    McpService::upsert_server(&state, saved).unwrap();
    assert_eq!(fixture.read(&AppType::Claude)["command"], "updated");
}

#[test]
fn pi_owned_disabled_updates_reject_external_changes_and_roll_back_late_failures() {
    let _guard = support::test_mutex()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let fixture = Fixture::new();
    let state = AppState::new(Arc::new(Database::memory().unwrap()));
    McpService::upsert_server(&state, all_app_server()).unwrap();
    let mut pi: Value =
        serde_json::from_str(&fs::read_to_string(fixture.path(&AppType::Pi)).unwrap()).unwrap();
    pi["mcpServers"]["shared"]["command"] = json!("external-pi");
    fs::write(fixture.path(&AppType::Pi), pi.to_string()).unwrap();
    let before = fs::read(fixture.path(&AppType::Pi)).unwrap();
    let mut edited = McpService::get_all_servers(&state).unwrap()["shared"].clone();
    edited.apps.pi = false;
    edited.app_overrides.pi.as_mut().unwrap()["command"] = json!("pi-new");
    assert!(McpService::upsert_server(&state, edited).is_err());
    assert_eq!(fs::read(fixture.path(&AppType::Pi)).unwrap(), before);
    assert!(
        McpService::get_all_servers(&state).unwrap()["shared"]
            .apps
            .pi
    );
    // Restore the fixture's Pi entry, then make a later OMP write fail.
    pi["mcpServers"]["shared"]["command"] = spec_for(&AppType::Pi)["command"].clone();
    fs::write(fixture.path(&AppType::Pi), pi.to_string()).unwrap();
    McpService::toggle_app(&state, "shared", AppType::Pi, false).unwrap();
    let before = fs::read(fixture.path(&AppType::Pi)).unwrap();
    let mut omp: Value =
        serde_json::from_str(&fs::read_to_string(fixture.path(&AppType::Omp)).unwrap()).unwrap();
    omp["mcpServers"]["shared"]["command"] = json!("external-omp");
    fs::write(fixture.path(&AppType::Omp), omp.to_string()).unwrap();
    let mut edited = McpService::get_all_servers(&state).unwrap()["shared"].clone();
    edited.app_overrides.pi.as_mut().unwrap()["command"] = json!("pi-new");
    assert!(McpService::upsert_server(&state, edited).is_err());
    assert_eq!(fs::read(fixture.path(&AppType::Pi)).unwrap(), before);
    assert_eq!(
        McpService::get_all_servers(&state).unwrap()["shared"].server_for_app(&AppType::Pi)
            ["command"],
        spec_for(&AppType::Pi)["command"]
    );
}
