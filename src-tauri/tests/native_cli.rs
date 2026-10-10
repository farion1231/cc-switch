use std::{
    collections::BTreeMap,
    fs,
    path::Path,
    process::{Command, Output},
};

use cc_switch_lib::{AppType, Database, Provider};
use serde_json::{json, Value};

#[path = "support.rs"]
mod support;

fn run(home: &Path, app: &str, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_cc-switch-cli"))
        .env("CC_SWITCH_TEST_HOME", home)
        .env_remove("CODEX_HOME")
        .args(["--json", "--app", app])
        .args(args)
        .output()
        .expect("run CLI")
}

fn success(output: Output) -> Value {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("JSON output")
}

fn snapshot(root: &Path) -> BTreeMap<std::path::PathBuf, Vec<u8>> {
    fn walk(root: &Path, dir: &Path, files: &mut BTreeMap<std::path::PathBuf, Vec<u8>>) {
        for entry in fs::read_dir(dir).expect("snapshot directory") {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(root, &path, files);
            } else {
                files.insert(
                    path.strip_prefix(root).unwrap().into(),
                    fs::read(path).unwrap(),
                );
            }
        }
    }
    let mut files = BTreeMap::new();
    walk(root, root, &mut files);
    files
}

fn seed(app: &AppType) {
    let db = Database::init().unwrap();
    for id in ["old", "new"] {
        let settings = match app {
            AppType::Claude => json!({"env": {
                "ANTHROPIC_AUTH_TOKEN": format!("fixture-secret-{id}"),
                "ANTHROPIC_BASE_URL": format!("https://{id}.example.test/private-token"),
                "ANTHROPIC_MODEL": format!("{id}-model")
            }}),
            AppType::Codex => json!({
                "auth": {"OPENAI_API_KEY": format!("fixture-secret-{id}")},
                "config": format!("model = '{id}-model'\nmodel_provider = 'custom'\n[model_providers.custom]\nname = 'Custom'\nbase_url = 'https://{id}.example.test/v1'\nwire_api = 'responses'\nrequires_openai_auth = true\n")
            }),
            AppType::Gemini => json!({"env": {
                "GEMINI_API_KEY": format!("fixture-secret-{id}"),
                "GOOGLE_GEMINI_BASE_URL": format!("https://{id}.example.test"),
                "GEMINI_MODEL": format!("{id}-model")
            }}),
            _ => unreachable!(),
        };
        db.save_provider(
            app.as_str(),
            &Provider::with_id(id.into(), format!("Provider {id}"), settings, None),
        )
        .unwrap();
    }
    db.set_current_provider(app.as_str(), "old").unwrap();
}

#[test]
fn reads_dry_runs_and_rejected_switches_leave_all_files_unchanged() {
    let _guard = support::test_mutex()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    support::reset_test_fs();
    let home = support::ensure_test_home();
    seed(&AppType::Claude);
    let settings = home.join(".cc-switch/settings.json");
    let mut value: Value = serde_json::from_slice(&fs::read(&settings).unwrap()).unwrap();
    value["currentProviderClaude"] = json!("deleted-provider");
    fs::write(&settings, serde_json::to_vec(&value).unwrap()).unwrap();
    let before = snapshot(home);

    assert_eq!(success(run(home, "claude", &["current"]))["id"], "old");
    for args in [
        vec!["status"],
        vec!["list"],
        vec!["config"],
        vec!["config", "new"],
        vec!["use", "new", "--dry-run"],
    ] {
        let result = success(run(home, "claude", &args));
        assert!(!result.to_string().contains("fixture-secret"));
        assert!(!result.to_string().contains("private-token"));
    }
    assert!(!run(home, "claude", &["use", "missing"]).status.success());
    assert!(!run(home, "opencode", &["use", "missing"]).status.success());
    assert_eq!(
        snapshot(home),
        before,
        "queries must not repair stale settings or initialize/migrate data"
    );

    let state = home.join(".cc-switch/live-state.json");
    fs::write(
        &state,
        json!({"version":1,"apps":{"claude":{"mode":"proxy","proxy_route":"new"}}}).to_string(),
    )
    .unwrap();
    let before = snapshot(home);
    assert_eq!(success(run(home, "claude", &["current"]))["id"], "new");
    assert!(!run(home, "claude", &["use", "old"]).status.success());
    assert_eq!(snapshot(home), before);

    fs::write(&state, "broken JSON").unwrap();
    let before = snapshot(home);
    assert!(!run(home, "claude", &["use", "new"]).status.success());
    assert_eq!(
        snapshot(home),
        before,
        "unknown routing state must not be treated as direct mode"
    );
}

#[test]
fn absent_or_incompatible_database_is_never_created_or_migrated() {
    // No global settings calls: the child processes use a unique isolated home.
    let home = tempfile::tempdir().unwrap();
    assert!(!run(home.path(), "claude", &["status"]).status.success());
    assert_eq!(fs::read_dir(home.path()).unwrap().count(), 0);
    let dir = home.path().join(".cc-switch");
    fs::create_dir(&dir).unwrap();
    for version in [0, 19, 999] {
        let db = rusqlite::Connection::open(dir.join("cc-switch.db")).unwrap();
        db.pragma_update(None, "user_version", version).unwrap();
        drop(db);
        let before = snapshot(home.path());
        assert!(!run(home.path(), "claude", &["status"]).status.success());
        assert_eq!(snapshot(home.path()), before);
    }
}

#[test]
fn switch_uses_desktop_service_and_preserves_pre_switch_backup() {
    let _guard = support::test_mutex()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    for (app, relative, expected) in [
        (AppType::Claude, ".claude/settings.json", "new.example.test"),
        (AppType::Codex, ".codex/config.toml", "new.example.test"),
        (AppType::Gemini, ".gemini/.env", "new.example.test"),
    ] {
        support::reset_test_fs();
        let home = support::ensure_test_home();
        seed(&app);
        let live = home.join(relative);
        fs::create_dir_all(live.parent().unwrap()).unwrap();
        let original = match app {
            AppType::Claude => "{\"env\":{\"ANTHROPIC_BASE_URL\":\"https://old.example.test\",\"ANTHROPIC_AUTH_TOKEN\":\"fixture-live-secret\"},\"permissions\":{\"allow\":[\"Read\"]}}",
            AppType::Codex => "model = 'old-model'\nmodel_provider = 'custom'\n[model_providers.custom]\nname = 'Custom'\nbase_url = 'https://old.example.test/v1'\nwire_api = 'responses'\n[mcp_servers.keep]\ncommand = 'echo'\n",
            AppType::Gemini => "GEMINI_API_KEY=fixture-live-secret\nGOOGLE_GEMINI_BASE_URL=https://old.example.test\n",
            _ => unreachable!(),
        };
        fs::write(&live, original).unwrap();
        let output = success(run(home, app.as_str(), &["use", "Provider new"]));
        assert_eq!(output["switched"], true);
        assert_eq!(output["provider"]["id"], "new");
        assert!(!output.to_string().contains("fixture-secret"));
        assert!(fs::read_to_string(&live).unwrap().contains(expected));
        if app == AppType::Claude {
            assert!(fs::read_to_string(&live).unwrap().contains("permissions"));
        } else if app == AppType::Codex {
            assert!(fs::read_to_string(&live)
                .unwrap()
                .contains("mcp_servers.keep"));
        }
        let backup = Path::new(output["backup"].as_str().unwrap());
        assert_eq!(fs::read_to_string(backup.join("file-2")).unwrap(), original);
        let saved_db = rusqlite::Connection::open(backup.join("cc-switch.db")).unwrap();
        let old: String = saved_db
            .query_row(
                "SELECT id FROM providers WHERE app_type = ?1 AND is_current = 1",
                [app.as_str()],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(old, "old");
        drop(saved_db);
        assert_eq!(success(run(home, app.as_str(), &["current"]))["id"], "new");
        let before = snapshot(home);
        assert_eq!(
            success(run(home, app.as_str(), &["use", "new"]))["switched"],
            false
        );
        assert_eq!(
            snapshot(home),
            before,
            "reselecting the current provider must not write or back up"
        );
    }
}

#[test]
fn custom_data_directory_is_used_before_settings_are_loaded() {
    let _guard = support::test_mutex()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    support::reset_test_fs();
    let home = support::ensure_test_home();
    seed(&AppType::Claude);
    let data = home.join(format!("custom-cli-data-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&data).unwrap();
    fs::rename(
        home.join(".cc-switch/cc-switch.db"),
        data.join("cc-switch.db"),
    )
    .unwrap();
    let settings_path = home.join(".cc-switch/settings.json");
    let mut settings: Value = serde_json::from_slice(&fs::read(&settings_path).unwrap()).unwrap();
    let custom_live = home.join("custom-claude");
    settings["claudeConfigDir"] = json!(custom_live);
    fs::write(&settings_path, serde_json::to_vec(&settings).unwrap()).unwrap();
    let before = snapshot(home);
    let result = success(run(
        home,
        "claude",
        &["--config-dir", data.to_str().unwrap(), "config"],
    ));
    assert_eq!(result["provider"]["id"], "old");
    assert_eq!(
        result["liveFiles"][0],
        json!(custom_live.join("settings.json"))
    );
    assert_eq!(snapshot(home), before);
    assert!(!home.join(".cc-switch/cc-switch.db").exists());
}
