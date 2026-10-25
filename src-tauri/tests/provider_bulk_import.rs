use cc_switch_lib::{AppState, AppType, Database, Provider, ProviderService};
use serde_json::{json, Value};
use std::sync::{Arc, Barrier};

#[path = "support.rs"]
mod support;
use support::{create_test_state, ensure_test_home, reset_test_fs, test_mutex};

fn row(name: &str, key: &str) -> Value {
    json!({"name":name,"base_url":"https://gateway.example.com/v1", "api_key":key})
}

#[test]
fn repeated_import_recognizes_explicit_default_protocol_from_editor() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    let state = create_test_state().unwrap();
    for app in [AppType::Claude, AppType::Codex] {
        let data = json!([row("Team", "synthetic-editor-key")]);
        let id = import(&state, app.clone(), data.clone())[0].clone();
        let mut saved = state.db.get_all_providers(app.as_str()).unwrap()[&id].clone();
        saved.meta.as_mut().unwrap().api_format = Some(
            if app == AppType::Claude {
                "anthropic"
            } else {
                "openai_responses"
            }
            .into(),
        );
        saved.meta.as_mut().unwrap().endpoint_auto_select = Some(true);
        state.db.save_provider(app.as_str(), &saved).unwrap();
        let preview = ProviderService::preview_bulk_import(&state, app, &data.to_string()).unwrap();
        assert_eq!((preview.new_count, preview.duplicate_count), (0, 1));
    }
}

#[test]
fn repeated_import_recognizes_primary_only_endpoint_but_keeps_extra_endpoints_distinct() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    let state = create_test_state().unwrap();
    let conn =
        rusqlite::Connection::open(ensure_test_home().join(".cc-switch/cc-switch.db")).unwrap();
    for app in [AppType::Claude, AppType::Codex] {
        let data = json!([row("Team", "synthetic-endpoint-key")]);
        let id = import(&state, app.clone(), data.clone())[0].clone();
        conn.execute(
            "INSERT INTO provider_endpoints(provider_id,app_type,url,added_at) VALUES (?1,?2,?3,0)",
            rusqlite::params![id, app.as_str(), "https://GATEWAY.example.com:443/v1/"],
        )
        .unwrap();
        let preview =
            ProviderService::preview_bulk_import(&state, app.clone(), &data.to_string()).unwrap();
        assert_eq!((preview.new_count, preview.duplicate_count), (0, 1));
        conn.execute(
            "INSERT INTO provider_endpoints(provider_id,app_type,url,added_at) VALUES (?1,?2,?3,0)",
            rusqlite::params![id, app.as_str(), "https://other.example.com/v1"],
        )
        .unwrap();
        let preview = ProviderService::preview_bulk_import(&state, app, &data.to_string()).unwrap();
        assert_eq!((preview.new_count, preview.duplicate_count), (1, 0));
    }
}

#[test]
fn silently_ignored_insert_rolls_back_the_whole_batch() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    let state = create_test_state().unwrap();
    let conn =
        rusqlite::Connection::open(ensure_test_home().join(".cc-switch/cc-switch.db")).unwrap();
    conn.execute_batch("CREATE TRIGGER ignore_second BEFORE INSERT ON providers WHEN NEW.name='Ignore' BEGIN SELECT RAISE(IGNORE); END;").unwrap();
    let source = json!([
        row("First", "synthetic-one"),
        row("Ignore", "synthetic-two")
    ])
    .to_string();
    let token = ProviderService::preview_bulk_import(&state, AppType::Claude, &source)
        .unwrap()
        .token
        .unwrap();
    assert!(ProviderService::import_bulk(&state, AppType::Claude, &source, &token).is_err());
    assert!(state.db.get_all_providers("claude").unwrap().is_empty());
}

fn import(state: &AppState, app: AppType, data: Value) -> Vec<String> {
    let source = data.to_string();
    let preview = ProviderService::preview_bulk_import(state, app.clone(), &source).unwrap();
    assert!(preview.errors.is_empty());
    ProviderService::import_bulk(state, app, &source, preview.token.as_deref().unwrap())
        .unwrap()
        .added_ids
}

#[test]
fn appends_without_switching_even_on_empty_database_and_preserves_native_files() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    let state = create_test_state().unwrap();
    let home = ensure_test_home();
    // These bytes deliberately need no parsing: a list-only import has no live-file work.
    let files = [
        ".claude/settings.json",
        ".codex/auth.json",
        ".codex/config.toml",
        ".cc-switch/live-state.json",
    ];
    for file in files {
        let path = home.join(file);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"untouched native sentinel").unwrap();
    }
    let settings = std::fs::read(home.join(".cc-switch/settings.json")).ok();
    for app in [AppType::Claude, AppType::Codex] {
        let source = json!([
            row("Team gateway", "synthetic-key-team-a"),
            row("Team gateway", "synthetic-key-team-b")
        ]);
        let preview =
            ProviderService::preview_bulk_import(&state, app.clone(), &source.to_string()).unwrap();
        assert_eq!(preview.new_count, 2);
        assert!(preview.rows[1].same_name);
        let serialized = serde_json::to_string(&preview).unwrap();
        assert!(!serialized.contains("synthetic-key"));
        assert_eq!(state.db.get_all_providers(app.as_str()).unwrap().len(), 0);
        let ids = import(&state, app.clone(), source);
        assert_eq!(ids.len(), 2);
        assert_ne!(ids[0], ids[1]);
        assert_eq!(state.db.get_current_provider(app.as_str()).unwrap(), None);
        let stored = state.db.get_all_providers(app.as_str()).unwrap();
        assert!(stored.values().all(|p| !p.in_failover_queue));
        assert!(stored[&ids[0]].sort_index < stored[&ids[1]].sort_index);
        assert_eq!(
            stored[&ids[0]].meta.as_ref().unwrap().common_config_enabled,
            Some(true)
        );
        if app == AppType::Claude {
            assert_eq!(
                stored[&ids[1]].settings_config["env"]["ANTHROPIC_AUTH_TOKEN"],
                "synthetic-key-team-b"
            );
        } else {
            assert_eq!(
                stored[&ids[1]].settings_config["auth"]["OPENAI_API_KEY"],
                "synthetic-key-team-b"
            );
        }
    }
    for file in files {
        assert_eq!(
            std::fs::read(home.join(file)).unwrap(),
            b"untouched native sentinel"
        );
    }
    assert_eq!(
        std::fs::read(home.join(".cc-switch/settings.json")).ok(),
        settings
    );
}

#[test]
fn preserves_existing_rows_current_and_failover_skips_duplicates_and_survives_restore() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    let state = create_test_state().unwrap();
    for app in [AppType::Claude, AppType::Codex] {
        let source = json!([row("Existing", "synthetic-existing-key")]);
        let id = import(&state, app.clone(), source.clone())[0].clone();
        let mut existing = state.db.get_all_providers(app.as_str()).unwrap()[&id].clone();
        existing.notes = Some("Keep my notes".into());
        existing.in_failover_queue = true;
        state.db.save_provider(app.as_str(), &existing).unwrap();
        state.db.set_current_provider(app.as_str(), &id).unwrap();
        let snapshot =
            serde_json::to_value(state.db.get_all_providers(app.as_str()).unwrap()[&id].clone())
                .unwrap();
        let more = json!([
            source[0],
            row("Existing", "synthetic-different-key"),
            row("New", "synthetic-new-key"),
            source[0]
        ]);
        let preview =
            ProviderService::preview_bulk_import(&state, app.clone(), &more.to_string()).unwrap();
        assert_eq!((preview.new_count, preview.duplicate_count), (2, 2));
        assert!(preview.rows[1].same_name && !preview.rows[1].duplicate);
        let ids = import(&state, app.clone(), more);
        let all = state.db.get_all_providers(app.as_str()).unwrap();
        assert_eq!(all.len(), 3);
        assert_eq!(serde_json::to_value(&all[&id]).unwrap(), snapshot);
        assert!(all[&id].sort_index < all[&ids[0]].sort_index);
        assert_eq!(
            state
                .db
                .get_current_provider(app.as_str())
                .unwrap()
                .as_deref(),
            Some(id.as_str())
        );
    }
    let dump = state.db.export_sql_string().unwrap();
    let restored = Database::memory().unwrap();
    restored.import_sql_string(&dump).unwrap();
    let reopened = Database::init().unwrap();
    for app in ["claude", "codex"] {
        let snapshot = serde_json::to_value(state.db.get_all_providers(app).unwrap()).unwrap();
        assert_eq!(
            serde_json::to_value(restored.get_all_providers(app).unwrap()).unwrap(),
            snapshot
        );
        assert_eq!(
            serde_json::to_value(reopened.get_all_providers(app).unwrap()).unwrap(),
            snapshot
        );
        assert_eq!(
            restored.get_current_provider(app).unwrap(),
            state.db.get_current_provider(app).unwrap()
        );
    }
}

#[test]
fn validation_is_all_or_nothing_and_does_not_echo_secrets() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    let state = create_test_state().unwrap();
    let secret = "synthetic-secret-never-echo";
    let valid = row("Valid", secret);
    let mut cases = vec![
        format!("[{{\"api_key\":\"{secret}\""),
        "{}".into(),
        "[]".into(),
        json!([valid.clone(), 42]).to_string(),
        json!([valid.clone(), {}]).to_string(),
    ];
    for (field, value) in [
        ("enabled", json!(true)),
        ("id", json!("existing")),
        ("api_key", json!("••••••••")),
        ("api_key", json!(42)),
        ("api_key", json!("bad\nkey")),
        ("name", json!(" ")),
        ("name", json!("x".repeat(257))),
        ("api_format", json!("unknown")),
        ("base_url", json!("file:///tmp/key")),
        ("base_url", json!("https://user:pass@gateway.example.com")),
        (
            "base_url",
            json!("https://gateway.example.com?api_key=synthetic-secret-never-echo"),
        ),
        ("base_url", json!("https://gateway.example.com#secret")),
        (
            "base_url",
            json!("https://a.example.com,https://b.example.com"),
        ),
        ("model", json!(false)),
    ] {
        let mut invalid = valid.clone();
        invalid[field] = value;
        cases.push(json!([valid.clone(), invalid]).to_string());
    }
    cases.push(json!(vec![valid.clone(); 101]).to_string());
    cases.push(" ".repeat(1024 * 1024 + 1));
    for source in cases {
        let preview =
            ProviderService::preview_bulk_import(&state, AppType::Claude, &source).unwrap();
        assert!(!preview.errors.is_empty());
        assert!(preview.token.is_none());
        assert!(!serde_json::to_string(&preview).unwrap().contains(secret));
        let error = ProviderService::import_bulk(&state, AppType::Claude, &source, "forged")
            .err()
            .unwrap();
        assert_eq!(
            error.to_string(),
            "provider.bulkImport.errors.validationFailed"
        );
        assert!(state.db.get_all_providers("claude").unwrap().is_empty());
    }
    assert!(ProviderService::preview_bulk_import(
        &state,
        AppType::Gemini,
        &json!([valid]).to_string()
    )
    .unwrap()
    .errors
    .is_empty());
}

#[test]
fn defaults_protocols_url_normalization_and_toml_escaping_are_preserved() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    let state = create_test_state().unwrap();
    for app in [AppType::Claude, AppType::Codex] {
        let native = if app == AppType::Claude {
            "anthropic"
        } else {
            "openai_responses"
        };
        let mut item = row("Team \"A\"", "short");
        item["base_url"] = json!(" https://GATEWAY.example.com:443/v1/ ");
        let mut explicit = item.clone();
        explicit["api_format"] = json!(native);
        let source = format!("\u{feff}{}", json!([item, explicit]));
        let preview = ProviderService::preview_bulk_import(&state, app.clone(), &source).unwrap();
        assert_eq!((preview.new_count, preview.duplicate_count), (1, 1));
        assert_eq!(preview.rows[0].base_url, "https://gateway.example.com/v1");
        assert_eq!(preview.rows[0].key_hint, "••••••••");
        assert!(!preview.rows[0].requires_routing);
        let result = ProviderService::import_bulk(
            &state,
            app.clone(),
            &source,
            preview.token.as_deref().unwrap(),
        )
        .unwrap();
        if app == AppType::Codex {
            assert_eq!(preview.rows[0].model.as_deref(), Some("gpt-5-codex"));
            let mut provider =
                state.db.get_all_providers("codex").unwrap()[&result.added_ids[0]].clone();
            let config = provider.settings_config["config"].as_str().unwrap();
            let value: toml::Value = toml::from_str(config).unwrap();
            assert_eq!(
                value["model_providers"]["custom"]["name"].as_str(),
                Some("Team \"A\"")
            );
            provider.settings_config["config"] =
                json!(format!("# formatting is not identity\n{config}\n"));
            state.db.save_provider("codex", &provider).unwrap();
            let again =
                ProviderService::preview_bulk_import(&state, AppType::Codex, &source).unwrap();
            assert_eq!(again.duplicate_count, 2);
        }
        for format in [
            "anthropic",
            "openai_chat",
            "openai_responses",
            "gemini_native",
        ] {
            let mut item = row("Routed", "synthetic-route-key");
            item["api_format"] = json!(format);
            item["model"] = json!("model-\"custom\"");
            let preview = ProviderService::preview_bulk_import(
                &state,
                app.clone(),
                &json!([item]).to_string(),
            )
            .unwrap();
            if app == AppType::Codex && format == "gemini_native" {
                assert!(!preview.errors.is_empty());
            } else {
                assert!(preview.errors.is_empty());
                assert_eq!(preview.rows[0].requires_routing, format != native);
            }
        }
    }
}

#[test]
fn stale_preview_and_simultaneous_confirmation_cannot_duplicate_or_overwrite() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    let state = Arc::new(create_test_state().unwrap());
    let source = json!([row("Team", "synthetic-race-key")]).to_string();
    let token = ProviderService::preview_bulk_import(&state, AppType::Claude, &source)
        .unwrap()
        .token
        .unwrap();
    let changed = json!([row("Team", "synthetic-changed-key")]).to_string();
    assert!(ProviderService::import_bulk(&state, AppType::Claude, &changed, &token).is_err());
    assert!(ProviderService::import_bulk(&state, AppType::Codex, &source, &token).is_err());
    let barrier = Arc::new(Barrier::new(3));
    let handles: Vec<_> = (0..2)
        .map(|_| {
            let (state, source, token, barrier) = (
                state.clone(),
                source.clone(),
                token.clone(),
                barrier.clone(),
            );
            std::thread::spawn(move || {
                barrier.wait();
                ProviderService::import_bulk(&state, AppType::Claude, &source, &token).is_ok()
            })
        })
        .collect();
    barrier.wait();
    assert_eq!(
        handles
            .into_iter()
            .filter_map(|h| h.join().ok())
            .filter(|ok| *ok)
            .count(),
        1
    );
    assert_eq!(state.db.get_all_providers("claude").unwrap().len(), 1);
    let source = json!([row("Other", "synthetic-other-key")]).to_string();
    let token = ProviderService::preview_bulk_import(&state, AppType::Claude, &source)
        .unwrap()
        .token
        .unwrap();
    let existing = Provider::with_id(
        "another-id".into(),
        "Other".into(),
        json!({"env":{"ANTHROPIC_AUTH_TOKEN":"different"}}),
        None,
    );
    state.db.save_provider("claude", &existing).unwrap();
    let err = ProviderService::import_bulk(&state, AppType::Claude, &source, &token)
        .err()
        .unwrap();
    assert_eq!(err.to_string(), "provider.bulkImport.errors.previewChanged");
    assert_eq!(state.db.get_all_providers("claude").unwrap().len(), 2);
}

#[test]
fn mid_batch_database_failure_rolls_back_and_does_not_expose_sql_or_keys() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    let state = create_test_state().unwrap();
    let conn =
        rusqlite::Connection::open(ensure_test_home().join(".cc-switch/cc-switch.db")).unwrap();
    conn.execute_batch("CREATE TRIGGER reject_second BEFORE INSERT ON providers WHEN NEW.name = 'Reject' BEGIN SELECT RAISE(ABORT, 'synthetic-private-sql-error'); END;").unwrap();
    let source = json!([
        row("First", "synthetic-key-first"),
        row("Reject", "synthetic-key-reject")
    ])
    .to_string();
    let preview = ProviderService::preview_bulk_import(&state, AppType::Claude, &source).unwrap();
    let err = ProviderService::import_bulk(
        &state,
        AppType::Claude,
        &source,
        preview.token.as_deref().unwrap(),
    )
    .err()
    .unwrap();
    assert_eq!(err.to_string(), "provider.bulkImport.errors.storageFailed");
    assert!(state.db.get_all_providers("claude").unwrap().is_empty());
    assert_eq!(
        Database::init()
            .unwrap()
            .get_all_providers("claude")
            .unwrap()
            .len(),
        0
    );
}

#[test]
fn preview_decisions_match_persisted_batch_for_both_apps() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    let state = create_test_state().unwrap();
    let mut fixtures = serde_json::Map::new();
    for app in [AppType::Claude, AppType::Codex] {
        let mut existing = row("Team gateway", "synthetic-team-key-1111");
        existing["model"] = json!(if app == AppType::Claude {
            "claude-sonnet-4-6"
        } else {
            "gpt-5.4"
        });
        let id = import(&state, app.clone(), json!([existing.clone()]))[0].clone();
        state.db.set_current_provider(app.as_str(), &id).unwrap();
        let mut second = existing.clone();
        second["api_key"] = json!("synthetic-team-key-2222");
        let mut qa = existing.clone();
        qa["name"] = json!("QA gateway");
        qa["base_url"] = json!("https://qa.example.com/v1");
        qa["api_key"] = json!("synthetic-team-key-3333");
        let source = serde_json::to_string_pretty(&json!([existing, second, qa, qa])).unwrap();
        let preview = ProviderService::preview_bulk_import(&state, app.clone(), &source).unwrap();
        assert_eq!((preview.new_count, preview.duplicate_count), (2, 2));
        let result = ProviderService::import_bulk(
            &state,
            app.clone(),
            &source,
            preview.token.as_deref().unwrap(),
        )
        .unwrap();
        assert_eq!((result.added_ids.len(), result.skipped), (2, 2));
        let all = state.db.get_all_providers(app.as_str()).unwrap();
        assert_eq!(all.len(), 3);
        let current = state.db.get_current_provider(app.as_str()).unwrap();
        assert_eq!(current.as_deref(), Some(id.as_str()));
        assert!(all.values().all(|p| !p.in_failover_queue));
        let records: Vec<_> = all.values().map(|p|json!({"id":p.id,"name":p.name,"sortIndex":p.sort_index,"inFailoverQueue":p.in_failover_queue})).collect();
        fixtures.insert(app.as_str().into(), json!({"source":source,"preview":preview,"result":result,"records":records,"beforeCount":1,"afterCount":all.len(),"currentBefore":id,"currentAfter":current}));
    }
    // Optional, synthetic-only fixture for the browser screenshot harness.
    if let Ok(directory) = std::env::var("CC_SWITCH_4184_EVIDENCE_DIR") {
        std::fs::write(
            std::path::Path::new(&directory).join("native-fixtures.json"),
            serde_json::to_string_pretty(&fixtures).unwrap(),
        )
        .unwrap();
    }
}

struct AgentEnvironment(Vec<(&'static str, Option<std::ffi::OsString>)>);
impl AgentEnvironment {
    fn isolated() -> Self {
        ensure_test_home();
        let variables = [
            "MINIMAX_DATA_DIR",
            "MAVIS_DATA_DIR",
            "KIMI_CODE_HOME",
            "HERMES_HOME",
            "PI_CODING_AGENT_DIR",
            "PI_CONFIG_DIR",
            "PI_PROFILE",
            "OMP_PROFILE",
            "XDG_DATA_HOME",
            "XDG_CONFIG_HOME",
        ];
        let originals = variables
            .into_iter()
            .map(|key| {
                let old = std::env::var_os(key);
                std::env::remove_var(key);
                (key, old)
            })
            .collect();
        reset_test_fs();
        for directory in [
            ".pi",
            ".omp",
            ".kimi-code",
            ".minimax",
            ".hermes",
            "custom-hermes-home",
            "Library",
        ] {
            let path = ensure_test_home().join(directory);
            if path.exists() {
                std::fs::remove_dir_all(path).unwrap();
            }
        }
        // Hermes uses LOCALAPPDATA on Windows; keep its native directory isolated too.
        let hermes_home = cc_switch_lib::hermes_config::get_hermes_dir();
        assert!(hermes_home.starts_with(ensure_test_home()) && hermes_home != ensure_test_home());
        if hermes_home.exists() {
            std::fs::remove_dir_all(hermes_home).unwrap();
        }
        Self(originals)
    }
}
impl Drop for AgentEnvironment {
    fn drop(&mut self) {
        for (key, value) in &self.0 {
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
    }
}

fn agent_row(app: &AppType, name: &str, key: &str) -> Value {
    let mut value = row(name, key);
    value["model"] = json!(if *app == AppType::ClaudeDesktop {
        "claude-sonnet-4-6"
    } else {
        "team-model"
    });
    if *app == AppType::Kimi {
        value["context_window"] = json!(32768);
    }
    value
}

fn native_files() -> std::collections::BTreeMap<String, Vec<u8>> {
    fn walk(
        root: &std::path::Path,
        dir: &std::path::Path,
        out: &mut std::collections::BTreeMap<String, Vec<u8>>,
    ) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            let relative = path.strip_prefix(root).unwrap();
            if relative.starts_with(".cc-switch") {
                continue;
            }
            if path.is_dir() {
                walk(root, &path, out);
            } else if path.is_file() {
                out.insert(
                    relative.to_string_lossy().into(),
                    std::fs::read(path).unwrap(),
                );
            }
        }
    }
    let mut out = std::collections::BTreeMap::new();
    walk(ensure_test_home(), ensure_test_home(), &mut out);
    out
}

#[test]
fn all_agents_append_atomically_without_activation_and_roundtrip_sql_backups() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let _env = AgentEnvironment::isolated();
    let state = create_test_state().unwrap();
    let sentinel = ensure_test_home().join("native-sentinel.txt");
    std::fs::write(sentinel, b"preserved").unwrap();
    // Exercise existing client files too, not just absence of newly created files.
    for (file, contents) in [
        (".claude/settings.json", "{\n  \"fixture\": \"keep\"\n}\n"),
        (".codex/auth.json", "{}\n"),
        (".codex/config.toml", "# keep Codex settings\n"),
        (".gemini/.env", "# keep Gemini credentials\n"),
        (".gemini/settings.json", "{}\n"),
        (".gemini/antigravity-cli/settings.json", "{}\n"),
        (".grok/config.toml", "# keep Grok settings\n"),
        (".config/opencode/opencode.json", "{\"provider\":{}}\n"),
        (
            ".openclaw/openclaw.json",
            "{\"models\":{\"providers\":{}}}\n",
        ),
        (".pi/agent/models.json", "{\"providers\":{}}\n"),
        (
            ".omp/agent/models.yml",
            "# keep OMP settings\nproviders: {}\n",
        ),
        (
            ".minimax/config.yaml",
            "# keep MCode settings\ncustom_provider: {}\n",
        ),
        (
            ".kimi-code/config.toml",
            "# keep Kimi settings\n[providers]\n[models]\n",
        ),
    ] {
        let path = ensure_test_home().join(file);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, contents).unwrap();
    }
    let hermes_path = cc_switch_lib::hermes_config::get_hermes_config_path();
    std::fs::create_dir_all(hermes_path.parent().unwrap()).unwrap();
    std::fs::write(
        hermes_path,
        "# keep Hermes settings\ncustom_providers: []\n",
    )
    .unwrap();
    #[cfg(target_os = "macos")]
    for directory in ["Claude", "Claude-3p"] {
        let dir = ensure_test_home()
            .join("Library/Application Support")
            .join(directory);
        std::fs::create_dir_all(dir.join("configLibrary")).unwrap();
        std::fs::write(dir.join("claude_desktop_config.json"), "{}\n").unwrap();
        std::fs::write(dir.join("configLibrary/_meta.json"), "{\"entries\":[]}\n").unwrap();
    }
    let before = native_files();
    let mut fixtures = serde_json::Map::new();
    for app in AppType::all() {
        let first = agent_row(&app, "Team gateway", "synthetic-team-key-1111");
        let existing = import(&state, app.clone(), json!([first.clone()]))[0].clone();
        let second = agent_row(&app, "Team gateway", "synthetic-team-key-2222");
        let source = serde_json::to_string_pretty(&json!([first, second, second])).unwrap();
        let preview = ProviderService::preview_bulk_import(&state, app.clone(), &source).unwrap();
        assert!(
            preview.errors.is_empty(),
            "{}: {}",
            app.as_str(),
            serde_json::to_string(&preview.errors).unwrap()
        );
        assert_eq!(
            (preview.new_count, preview.duplicate_count),
            (1, 2),
            "{}",
            app.as_str()
        );
        let result = ProviderService::import_bulk(
            &state,
            app.clone(),
            &source,
            preview.token.as_deref().unwrap(),
        )
        .unwrap();
        assert_eq!((result.added_ids.len(), result.skipped), (1, 2));
        assert_ne!(existing, result.added_ids[0]);
        let all = state.db.get_all_providers(app.as_str()).unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(state.db.get_current_provider(app.as_str()).unwrap(), None);
        assert_eq!(
            ProviderService::list(&state, app.clone()).unwrap().len(),
            2,
            "{} list",
            app.as_str()
        );
        if app.is_additive_mode() {
            assert!(all
                .values()
                .all(|p| p.meta.as_ref().unwrap().live_config_managed == Some(false)));
        }
        assert!(all.values().all(|p| !p.in_failover_queue));
        assert_eq!(
            native_files(),
            before,
            "{} wrote native files",
            app.as_str()
        );
        let repeated = ProviderService::preview_bulk_import(&state, app.clone(), &source).unwrap();
        assert_eq!(repeated.new_count, 0, "{} repeat", app.as_str());
        let records:Vec<_>=all.values().map(|p|json!({"id":p.id,"name":p.name,"managed":p.meta.as_ref().and_then(|m|m.live_config_managed)})).collect();
        fixtures.insert(app.as_str().into(),json!({"source":source,"preview":preview,"result":result,"beforeCount":1,"afterCount":2,"records":records,"currentBefore":null,"currentAfter":null,"nativeUnchanged":true}));
    }
    let snapshot = state.db.export_sql_string().unwrap();
    let restored = Database::memory().unwrap();
    restored.import_sql_string(&snapshot).unwrap();
    for app in AppType::all() {
        assert_eq!(
            serde_json::to_value(restored.get_all_providers(app.as_str()).unwrap()).unwrap(),
            serde_json::to_value(state.db.get_all_providers(app.as_str()).unwrap()).unwrap()
        );
        let restored_state = AppState::new(Arc::new(Database::init().unwrap()));
        let data = json!([agent_row(&app, "Team gateway", "synthetic-team-key-1111")]);
        assert_eq!(
            ProviderService::preview_bulk_import(&restored_state, app, &data.to_string())
                .unwrap()
                .duplicate_count,
            1
        );
    }
    // Startup/sync must not project rows marked as DB-only for coexist agents.
    for app in [AppType::OpenCode, AppType::OpenClaw, AppType::Hermes] {
        ProviderService::sync_current_provider_for_app(&state, app).unwrap();
    }
    assert!(native_files()
        .values()
        .all(|bytes| !String::from_utf8_lossy(bytes).contains("synthetic-team-key")));
    if let Ok(dir) = std::env::var("CC_SWITCH_4184_EVIDENCE_DIR") {
        std::fs::write(
            std::path::Path::new(&dir).join("agents-fixtures.json"),
            serde_json::to_string_pretty(&fixtures).unwrap(),
        )
        .unwrap();
    }
}

#[test]
fn coexist_agents_enable_distinct_keys_then_reimport_without_duplicates() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let _env = AgentEnvironment::isolated();
    let state = create_test_state().unwrap();
    let scenarios = AppType::all()
        .filter(AppType::is_additive_mode)
        .map(|app| (app, false))
        .chain(std::iter::once((AppType::Hermes, true)));
    for (app, custom_hermes_home) in scenarios {
        let name = if custom_hermes_home {
            // Exercise a non-default directory on every host, including macOS/Linux.
            let directory = ensure_test_home().join("custom-hermes-home");
            cc_switch_lib::update_settings(cc_switch_lib::AppSettings {
                hermes_config_dir: Some(directory.to_string_lossy().into_owned()),
                ..Default::default()
            })
            .unwrap();
            "Custom home"
        } else {
            "Same name"
        };
        let data = json!([
            agent_row(&app, name, "synthetic-key-one"),
            agent_row(&app, name, "synthetic-key-two")
        ]);
        let ids = import(&state, app.clone(), data.clone());
        for id in &ids {
            ProviderService::switch(&state, app.clone(), id).unwrap();
        }
        let listed = ProviderService::list(&state, app.clone()).unwrap();
        assert!(
            ids.iter().all(|id| listed.contains_key(id)),
            "{} lost a key",
            app.as_str()
        );
        if !matches!(app, AppType::Pi | AppType::Omp) {
            assert!(
                ids.iter()
                    .all(|id| listed[id].meta.as_ref().unwrap().live_config_managed == Some(true)),
                "{} activation",
                app.as_str()
            );
        }
        let preview =
            ProviderService::preview_bulk_import(&state, app.clone(), &data.to_string()).unwrap();
        assert_eq!(
            (preview.new_count, preview.duplicate_count),
            (0, 2),
            "{} reimport",
            app.as_str()
        );
        let path = match app {
            AppType::OpenCode => ensure_test_home().join(".config/opencode/opencode.json"),
            AppType::OpenClaw => ensure_test_home().join(".openclaw/openclaw.json"),
            AppType::Hermes => cc_switch_lib::hermes_config::get_hermes_config_path(),
            AppType::Pi => ensure_test_home().join(".pi/agent/models.json"),
            AppType::Omp => ensure_test_home().join(".omp/agent/models.yml"),
            AppType::Mcode => ensure_test_home().join(".minimax/config.yaml"),
            AppType::Kimi => ensure_test_home().join(".kimi-code/config.toml"),
            _ => unreachable!(),
        };
        let contents = std::fs::read_to_string(&path).unwrap_or_else(|error| {
            panic!(
                "{} {}: {error}; native paths: {:?}",
                app.as_str(),
                path.display(),
                native_files().keys().collect::<Vec<_>>()
            )
        });
        for id in &ids {
            assert!(
                contents.contains(id),
                "{} missing native provider ID",
                app.as_str()
            );
        }
        assert!(contents.contains("synthetic-key-one") && contents.contains("synthetic-key-two"));
    }
}

#[test]
fn every_agent_rejects_invalid_batch_and_rolls_back_on_second_insert_failure() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let _env = AgentEnvironment::isolated();
    let state = create_test_state().unwrap();
    let conn =
        rusqlite::Connection::open(ensure_test_home().join(".cc-switch/cc-switch.db")).unwrap();
    conn.execute_batch("CREATE TRIGGER abort_batch BEFORE INSERT ON providers WHEN NEW.name='Reject' BEGIN SELECT RAISE(ABORT,'fixture'); END;").unwrap();
    for app in AppType::all() {
        let data = json!([
            agent_row(&app, "First", "synthetic-key-one"),
            agent_row(&app, "Reject", "synthetic-key-two")
        ])
        .to_string();
        let preview = ProviderService::preview_bulk_import(&state, app.clone(), &data).unwrap();
        assert!(preview.errors.is_empty(), "{}", app.as_str());
        assert!(ProviderService::import_bulk(
            &state,
            app.clone(),
            &data,
            preview.token.as_deref().unwrap()
        )
        .is_err());
        assert!(state.db.get_all_providers(app.as_str()).unwrap().is_empty());
        let invalid =
            json!([agent_row(&app,"Valid","synthetic-key"), {"name":"broken"}]).to_string();
        let preview = ProviderService::preview_bulk_import(&state, app.clone(), &invalid).unwrap();
        assert!(!preview.errors.is_empty());
        assert!(preview.token.is_none());
        if app.is_additive_mode() {
            for key in ["${TOKEN}", "{env:TOKEN}", "{file:/tmp/secret}"] {
                let source = json!([agent_row(&app, "Bad", key)]).to_string();
                let preview =
                    ProviderService::preview_bulk_import(&state, app.clone(), &source).unwrap();
                assert!(!preview.errors.is_empty(), "{} expanded key", app.as_str());
            }
        }
    }
    for app in [AppType::Pi, AppType::Omp] {
        for key in ["!echo forbidden", "$TOKEN"] {
            if app == AppType::Omp && key == "$TOKEN" {
                continue;
            }
            assert!(!ProviderService::preview_bulk_import(
                &state,
                app.clone(),
                &json!([agent_row(&app, "Bad", key)]).to_string()
            )
            .unwrap()
            .errors
            .is_empty());
        }
    }
}

#[test]
fn independent_connections_cannot_import_the_same_preview_twice() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let _env = AgentEnvironment::isolated();
    for mode in ["DELETE", "WAL"] {
        reset_test_fs();
        let one = create_test_state().unwrap();
        let conn =
            rusqlite::Connection::open(ensure_test_home().join(".cc-switch/cc-switch.db")).unwrap();
        conn.pragma_update(None, "journal_mode", mode).unwrap();
        let two = AppState::new(Arc::new(Database::init().unwrap()));
        for app in AppType::all() {
            let source = json!([agent_row(&app, "Team", "synthetic-concurrent")]).to_string();
            let token = ProviderService::preview_bulk_import(&one, app.clone(), &source)
                .unwrap()
                .token
                .unwrap();
            let barrier = Arc::new(Barrier::new(3));
            let handles: Vec<_> = [one.clone(), two.clone()]
                .into_iter()
                .map(|state| {
                    let (app, source, token, barrier) =
                        (app.clone(), source.clone(), token.clone(), barrier.clone());
                    std::thread::spawn(move || {
                        barrier.wait();
                        ProviderService::import_bulk(&state, app, &source, &token).is_ok()
                    })
                })
                .collect();
            barrier.wait();
            assert_eq!(
                handles
                    .into_iter()
                    .map(|h| h.join().unwrap())
                    .filter(|ok| *ok)
                    .count(),
                1,
                "{mode} {}",
                app.as_str()
            );
            assert_eq!(one.db.get_all_providers(app.as_str()).unwrap().len(), 1);
        }
    }
}

#[test]
fn supported_protocols_models_and_context_windows_match_each_agent() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let _env = AgentEnvironment::isolated();
    let state = create_test_state().unwrap();
    for app in AppType::all() {
        for protocol in [
            "anthropic",
            "openai_chat",
            "openai_responses",
            "gemini_native",
        ] {
            let supported = match app {
                AppType::Gemini | AppType::Agy => protocol == "gemini_native",
                AppType::Codex | AppType::GrokBuild | AppType::Mcode | AppType::Hermes => {
                    protocol != "gemini_native"
                }
                _ => true,
            };
            let mut value = agent_row(&app, protocol, "synthetic-protocol-key");
            value["model"] = json!("team-model");
            value["api_format"] = json!(protocol);
            let source = json!([value]).to_string();
            let preview =
                ProviderService::preview_bulk_import(&state, app.clone(), &source).unwrap();
            assert_eq!(
                preview.errors.is_empty(),
                supported,
                "{} {protocol}",
                app.as_str()
            );
            if !supported {
                continue;
            }
            let result = ProviderService::import_bulk(
                &state,
                app.clone(),
                &source,
                preview.token.as_deref().unwrap(),
            )
            .unwrap();
            let saved =
                state.db.get_all_providers(app.as_str()).unwrap()[&result.added_ids[0]].clone();
            if app == AppType::ClaudeDesktop {
                assert!(preview.rows[0].requires_routing);
                let routes = &saved.meta.as_ref().unwrap().claude_desktop_model_routes;
                assert_eq!(routes.len(), 1);
                assert!(routes.values().all(|r| r.model == "team-model"
                    && r.label_override.as_deref() == Some("team-model")));
                assert!(saved.settings_config["env"]
                    .get("ANTHROPIC_MODEL")
                    .is_none());
            } else if app == AppType::Kimi {
                let models = saved.settings_config["models"].as_object().unwrap();
                assert_eq!(models.len(), 1);
                assert!(models
                    .keys()
                    .all(|k| k.starts_with(&format!("{}/", saved.id))));
                assert!(models
                    .values()
                    .all(|m| m["provider"] == saved.id && m["max_context_size"] == 32768));
            }
            let again = ProviderService::preview_bulk_import(&state, app.clone(), &source).unwrap();
            assert_eq!(
                again.duplicate_count,
                1,
                "{} {protocol} repeat",
                app.as_str()
            );
        }
        if app.is_additive_mode() {
            let mut value = agent_row(&app, "No model", "synthetic-model-key");
            value.as_object_mut().unwrap().remove("model");
            let preview = ProviderService::preview_bulk_import(
                &state,
                app.clone(),
                &json!([value]).to_string(),
            )
            .unwrap();
            assert!(preview.errors.iter().any(|e| e.field == Some("model")));
        }
    }
    for size in [
        json!(0),
        json!(-1),
        json!(1.5),
        json!(true),
        Value::Null,
        json!(9007199254740992_u64),
    ] {
        let mut value = agent_row(&AppType::Kimi, "Bad context", "synthetic-context-key");
        value["context_window"] = size;
        assert!(!ProviderService::preview_bulk_import(
            &state,
            AppType::Kimi,
            &json!([value]).to_string()
        )
        .unwrap()
        .errors
        .is_empty());
    }
    let mut routed = row("Desktop needs model", "synthetic-desktop-key");
    routed["api_format"] = json!("openai_chat");
    assert!(!ProviderService::preview_bulk_import(
        &state,
        AppType::ClaudeDesktop,
        &json!([routed]).to_string()
    )
    .unwrap()
    .errors
    .is_empty());
    let direct = agent_row(&AppType::ClaudeDesktop, "Direct", "synthetic-desktop-key");
    let id = import(&state, AppType::ClaudeDesktop, json!([direct]))[0].clone();
    let saved = state.db.get_all_providers("claude-desktop").unwrap()[&id].clone();
    assert_eq!(
        saved.meta.as_ref().unwrap().claude_desktop_model_routes["claude-sonnet-4-6"].model,
        "claude-sonnet-4-6"
    );
    let no_model = json!([row("Discovery", "synthetic-desktop-discovery")]);
    assert!(
        !ProviderService::preview_bulk_import(
            &state,
            AppType::ClaudeDesktop,
            &no_model.to_string()
        )
        .unwrap()
        .rows[0]
            .requires_routing
    );
}

#[test]
fn equivalent_legacy_claude_base_urls_are_duplicates() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    let state = create_test_state().unwrap();
    let source = json!([row("Legacy", "synthetic-legacy-key")]);
    let id = import(&state, AppType::Claude, source.clone())[0].clone();
    let mut stored = state.db.get_all_providers("claude").unwrap()[&id].clone();
    stored.settings_config["env"]["ANTHROPIC_BASE_URL"] =
        json!("https://GATEWAY.example.com:443/v1/");
    state.db.save_provider("claude", &stored).unwrap();
    assert_eq!(
        ProviderService::preview_bulk_import(&state, AppType::Claude, &source.to_string())
            .unwrap()
            .duplicate_count,
        1
    );
}

#[test]
fn partially_overlapping_batches_require_repreview_without_partial_writes() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let _env = AgentEnvironment::isolated();
    let state = create_test_state().unwrap();
    for app in AppType::all() {
        let a = json!([
            agent_row(&app, "A", "synthetic-a"),
            agent_row(&app, "B", "synthetic-b")
        ])
        .to_string();
        let b = json!([
            agent_row(&app, "B", "synthetic-b"),
            agent_row(&app, "C", "synthetic-c")
        ])
        .to_string();
        let token = ProviderService::preview_bulk_import(&state, app.clone(), &b)
            .unwrap()
            .token
            .unwrap();
        import(&state, app.clone(), serde_json::from_str(&a).unwrap());
        assert_eq!(
            ProviderService::import_bulk(&state, app.clone(), &b, &token)
                .err()
                .unwrap()
                .to_string(),
            "provider.bulkImport.errors.previewChanged"
        );
        assert_eq!(state.db.get_all_providers(app.as_str()).unwrap().len(), 2);
        let review = ProviderService::preview_bulk_import(&state, app.clone(), &b).unwrap();
        assert_eq!((review.new_count, review.duplicate_count), (1, 1));
        let result =
            ProviderService::import_bulk(&state, app.clone(), &b, review.token.as_deref().unwrap())
                .unwrap();
        assert_eq!((result.added_ids.len(), result.skipped), (1, 1));
        assert_eq!(state.db.get_all_providers(app.as_str()).unwrap().len(), 3);
    }
}

#[test]
fn kimi_literal_aliases_and_invalid_owner_references_do_not_collapse() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let _env = AgentEnvironment::isolated();
    let state = create_test_state().unwrap();
    let data = json!([agent_row(&AppType::Kimi, "Team", "synthetic-alias-key")]);
    let id = import(&state, AppType::Kimi, data.clone())[0].clone();
    let mut saved = state.db.get_all_providers("kimi").unwrap()[&id].clone();
    let models = saved.settings_config["models"].as_object_mut().unwrap();
    let model = models.values().next().unwrap().clone();
    models.clear();
    models.insert("<owner>/team-model".into(), model.clone());
    state.db.save_provider("kimi", &saved).unwrap();
    assert_eq!(
        ProviderService::preview_bulk_import(&state, AppType::Kimi, &data.to_string())
            .unwrap()
            .new_count,
        1
    );
    for owner in [json!("<owner>"), Value::Null] {
        let models = saved.settings_config["models"].as_object_mut().unwrap();
        let mut invalid = model.clone();
        invalid["provider"] = owner;
        models.clear();
        models.insert(format!("{id}/team-model"), invalid);
        state.db.save_provider("kimi", &saved).unwrap();
        assert_eq!(
            ProviderService::preview_bulk_import(&state, AppType::Kimi, &data.to_string())
                .unwrap()
                .new_count,
            1
        );
    }
}

#[test]
fn omp_plain_keys_warn_and_persist_without_resolving_the_environment() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let _env = AgentEnvironment::isolated();
    let state = create_test_state().unwrap();
    let initial_native = native_files();
    let source =
        serde_json::to_string_pretty(&json!([agent_row(&AppType::Omp, "Team gateway", "abc123")]))
            .unwrap();
    let preview = ProviderService::preview_bulk_import(&state, AppType::Omp, &source).unwrap();
    let fixture = json!({"source":source,"preview":preview});
    if let Ok(dir) = std::env::var("CC_SWITCH_4184_EVIDENCE_DIR") {
        std::fs::write(
            std::path::Path::new(&dir).join("omp-warning-fixture.json"),
            serde_json::to_string_pretty(&fixture).unwrap(),
        )
        .unwrap();
    }
    assert!(
        preview.errors.is_empty(),
        "a plain key is ambiguous, not invalid"
    );
    assert_eq!(
        serde_json::to_value(&preview).unwrap()["rows"][0]["keyMayResolveFromEnvironment"],
        true
    );
    assert_eq!(preview.rows[0].key_hint, "••••••••");
    assert!(!serde_json::to_string(&preview).unwrap().contains("abc123"));
    let result = ProviderService::import_bulk(
        &state,
        AppType::Omp,
        &source,
        preview.token.as_deref().unwrap(),
    )
    .unwrap();
    let saved = state.db.get_all_providers("omp").unwrap();
    assert_eq!(
        saved[&result.added_ids[0]].settings_config["apiKey"],
        "abc123"
    );
    assert_eq!(state.db.get_current_provider("omp").unwrap(), None);
    assert_eq!(native_files(), initial_native);
    if let Ok(dir) = std::env::var("CC_SWITCH_4184_EVIDENCE_DIR") {
        let fixture = json!({"source":source,"preview":preview,"result":result,"storedKey":"abc123","nativeUnchanged":true});
        std::fs::write(
            std::path::Path::new(&dir).join("omp-warning-fixture.json"),
            serde_json::to_string_pretty(&fixture).unwrap(),
        )
        .unwrap();
    }
    const KEY_NAME: &str = "CC_SWITCH_4184_LITERAL_TEST";
    let _credential_env = AgentEnvironment(vec![(KEY_NAME, std::env::var_os(KEY_NAME))]);
    std::env::set_var(KEY_NAME, "synthetic-environment-value-not-to-import");
    let env_path = ensure_test_home().join(".omp/agent/.env");
    std::fs::create_dir_all(env_path.parent().unwrap()).unwrap();
    std::fs::write(
        &env_path,
        format!("{KEY_NAME}=synthetic-dotenv-value-not-to-import\n"),
    )
    .unwrap();
    let before = native_files();
    for key in [
        KEY_NAME,
        "TOKEN_NAME",
        "abcdef123456",
        "123456",
        "sk-synthetic-key",
    ] {
        let value = json!([agent_row(&AppType::Omp, "Plain credential", key)]);
        let review =
            ProviderService::preview_bulk_import(&state, AppType::Omp, &value.to_string()).unwrap();
        assert!(review.errors.is_empty());
        assert_eq!(
            serde_json::to_value(&review).unwrap()["rows"][0]["keyMayResolveFromEnvironment"],
            true
        );
        let ids = import(&state, AppType::Omp, value.clone());
        assert_eq!(
            state.db.get_all_providers("omp").unwrap()[&ids[0]].settings_config["apiKey"],
            key
        );
        assert_eq!(
            ProviderService::preview_bulk_import(&state, AppType::Omp, &value.to_string())
                .unwrap()
                .duplicate_count,
            1
        );
    }
    assert_eq!(native_files(), before);
    assert!(!state
        .db
        .export_sql_string()
        .unwrap()
        .contains("value-not-to-import"));
    let count = state.db.get_all_providers("omp").unwrap().len();
    for bad in [
        "!echo forbidden",
        "${TOKEN}",
        "{env:TOKEN}",
        "{file:/tmp/secret}",
    ] {
        let mixed = json!([
            agent_row(&AppType::Omp, "Valid", "another123"),
            agent_row(&AppType::Omp, "Invalid", bad)
        ])
        .to_string();
        let review = ProviderService::preview_bulk_import(&state, AppType::Omp, &mixed).unwrap();
        assert!(!review.errors.is_empty());
        assert!(review.token.is_none());
        assert!(ProviderService::import_bulk(&state, AppType::Omp, &mixed, "forged").is_err());
        assert_eq!(state.db.get_all_providers("omp").unwrap().len(), count);
        assert_eq!(native_files(), before);
    }
}

#[test]
fn omp_warning_preview_does_not_authorize_changed_or_repeated_submissions() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let _env = AgentEnvironment::isolated();
    let state = Arc::new(create_test_state().unwrap());
    let initial_native = native_files();
    let source = json!([agent_row(&AppType::Omp, "Team", "abc123")]).to_string();
    let changed = json!([agent_row(&AppType::Omp, "Team", "different123")]).to_string();
    let review = ProviderService::preview_bulk_import(&state, AppType::Omp, &source).unwrap();
    assert!(review.errors.is_empty());
    let token = review.token.unwrap();
    assert_eq!(
        ProviderService::import_bulk(&state, AppType::Omp, &changed, &token)
            .err()
            .unwrap()
            .to_string(),
        "provider.bulkImport.errors.previewChanged"
    );
    let barrier = Arc::new(Barrier::new(3));
    let handles: Vec<_> = (0..2)
        .map(|_| {
            let (state, source, token, barrier) = (
                state.clone(),
                source.clone(),
                token.clone(),
                barrier.clone(),
            );
            std::thread::spawn(move || {
                barrier.wait();
                ProviderService::import_bulk(&state, AppType::Omp, &source, &token).is_ok()
            })
        })
        .collect();
    barrier.wait();
    assert_eq!(
        handles
            .into_iter()
            .map(|h| h.join().unwrap())
            .filter(|ok| *ok)
            .count(),
        1
    );
    assert_eq!(state.db.get_all_providers("omp").unwrap().len(), 1);
    assert_eq!(native_files(), initial_native);
}
