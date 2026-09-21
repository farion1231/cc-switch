//! Cross-application regression tests. Every client file lives in a temporary home.
use crate::codex_config as config;
use crate::{
    AppSettings, AppState, AppType, Database, McpService, ProfileScope, ProfileService,
    PromptService, Provider, ProviderService, SkillService,
};
use serde_json::{json, Value};
use serial_test::serial;
use std::{ffi::OsString, fs, path::PathBuf, sync::Arc};

struct TestHome {
    dir: tempfile::TempDir,
    previous: Option<OsString>,
}
impl TestHome {
    fn new(separate: bool) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let previous = std::env::var_os("CC_SWITCH_TEST_HOME");
        std::env::set_var("CC_SWITCH_TEST_HOME", dir.path());
        crate::settings::reload_settings().unwrap();
        crate::settings::update_settings(AppSettings {
            codex_desktop_config_dir: separate
                .then(|| dir.path().join("desktop").to_string_lossy().into_owned()),
            skill_sync_method: crate::services::skill::SyncMethod::Copy,
            ..Default::default()
        })
        .unwrap();
        Self { dir, previous }
    }
    fn state(&self) -> AppState {
        AppState::new(Arc::new(Database::memory().unwrap()))
    }
    fn root(&self, app: &AppType) -> PathBuf {
        config::get_codex_config_dir_for_app(app)
    }
}
impl Drop for TestHome {
    fn drop(&mut self) {
        match &self.previous {
            Some(value) => std::env::set_var("CC_SWITCH_TEST_HOME", value),
            None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
        }
        crate::settings::reload_settings().unwrap();
    }
}
fn provider(id: &str, key: &str, url: &str) -> Provider {
    Provider::with_id(
        id.into(),
        id.into(),
        json!({
            "auth": {"OPENAI_API_KEY":key},
            "config":format!("model_provider = \"custom\"\nmodel = \"gpt-5.5\"\n[model_providers.custom]\nname = \"test\"\nbase_url = \"{url}\"\nwire_api = \"responses\"\nrequires_openai_auth = true\n"),
            "modelCatalog":{"models":[{"model":format!("model-{key}")}]}
        }),
        None,
    )
}
fn contents(path: PathBuf) -> Vec<u8> {
    fs::read(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}
fn live_bundle(app: &AppType) -> Vec<Option<Vec<u8>>> {
    // Third-party providers keep their key in TOML; auth.json can be absent.
    vec![
        fs::read(config::get_codex_auth_path_for_app(app)).ok(),
        Some(contents(config::get_codex_config_path_for_app(app))),
        Some(contents(config::get_codex_model_catalog_path_for_app(app))),
    ]
}
fn seed_template(app: &AppType) {
    let root = config::get_codex_config_dir_for_app(app);
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("models_cache.json"), json!({"models":[{"slug":"gpt-5.5","display_name":"GPT-5.5","model_messages":{"instructions_template":"test"},"additional_speed_tiers":[],"context_window":128000}]}).to_string()).unwrap();
}

#[test]
#[serial]
fn desktop_initialization_is_official_only_and_keeps_schema_version() {
    let home = TestHome::new(false);
    let native = home.root(&AppType::Codex);
    fs::create_dir_all(&native).unwrap();
    fs::write(
        native.join("config.toml"),
        "# untouched\nmodel = \"native\"\n",
    )
    .unwrap();
    fs::write(native.join("auth.json"), r#"{"OPENAI_API_KEY":"native"}"#).unwrap();
    let original = (
        contents(native.join("config.toml")),
        contents(native.join("auth.json")),
    );
    {
        let db = Database::init().unwrap();
        db.save_provider(
            "codex",
            &provider("existing", "cli", "https://example.invalid/v1"),
        )
        .unwrap();
        db.set_current_provider("codex", "existing").unwrap();
        db.set_setting("official_providers_seeded", "true").unwrap();
        assert_eq!(db.init_default_official_providers().unwrap(), 1);
        assert_eq!(db.init_default_official_providers().unwrap(), 0);
        assert_eq!(db.get_all_providers("codex-desktop").unwrap().len(), 1);
        assert!(db
            .get_provider_by_id("codex-desktop-official", "codex-desktop")
            .unwrap()
            .is_some());
        assert_eq!(db.get_current_provider("codex-desktop").unwrap(), None);
        let state = AppState::new(Arc::new(db));
        assert!(!ProviderService::should_import_default_config_on_startup(
            &state,
            &AppType::CodexDesktop
        )
        .unwrap());
        assert!(!ProviderService::import_default_config(&state, AppType::CodexDesktop).unwrap());
    }
    let db = Database::init().unwrap();
    db.init_default_official_providers().unwrap();
    assert_eq!(db.get_all_providers("codex-desktop").unwrap().len(), 1);
    assert_eq!(db.get_all_providers("codex").unwrap().len(), 1);
    assert_eq!(
        db.get_current_provider("codex").unwrap().as_deref(),
        Some("existing")
    );
    let conn = db.conn.lock().unwrap();
    let version: u32 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(version, 19);
    Database::ensure_codex_desktop_schema(&conn).unwrap();
    assert!(!conn
        .query_row(
            "SELECT enabled FROM proxy_config WHERE app_type='codex-desktop'",
            [],
            |r| r.get::<_, bool>(0)
        )
        .unwrap());
    assert_eq!(
        original,
        (
            contents(native.join("config.toml")),
            contents(native.join("auth.json"))
        )
    );
}

#[test]
#[serial]
fn desktop_legacy_config_load_and_prompt_startup_skip_import() {
    let home = TestHome::new(true);
    let root = home.root(&AppType::CodexDesktop);
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("AGENTS.md"), "Desktop instructions").unwrap();
    let legacy = crate::MultiAppConfig::default();
    legacy.save().unwrap();
    let loaded = crate::MultiAppConfig::load().unwrap();
    assert!(loaded.prompts.codex_desktop.prompts.is_empty());
    let state = home.state();
    let native_mcp = "[mcp_servers.shared]\ncommand = \"native\"\n";
    fs::write(root.join("config.toml"), native_mcp).unwrap();
    fs::create_dir_all(root.join("skills/shared")).unwrap();
    fs::write(root.join("skills/shared/SKILL.md"), "Native skill").unwrap();
    state
        .db
        .save_mcp_server(
            &serde_json::from_value(
                json!({"id":"shared","name":"shared","server":{"command":"managed"},"apps":{}}),
            )
            .unwrap(),
        )
        .unwrap();
    state.db.save_skill(&serde_json::from_value(json!({"id":"shared","name":"shared","directory":"shared","apps":{},"installedAt":1})).unwrap()).unwrap();
    assert_eq!(
        PromptService::import_from_file_on_first_launch(&state, AppType::CodexDesktop).unwrap(),
        0
    );
    PromptService::sync_to_live(&state, AppType::CodexDesktop).unwrap();
    McpService::sync_enabled_for_app(&state, &AppType::CodexDesktop).unwrap();
    SkillService::sync_to_app(&state.db, &AppType::CodexDesktop).unwrap();
    assert_eq!(
        fs::read_to_string(root.join("config.toml")).unwrap(),
        native_mcp
    );
    assert_eq!(
        fs::read_to_string(root.join("skills/shared/SKILL.md")).unwrap(),
        "Native skill"
    );
    assert_eq!(
        fs::read_to_string(root.join("AGENTS.md")).unwrap(),
        "Desktop instructions"
    );
    assert!(state.db.get_prompts("codex-desktop").unwrap().is_empty());
}

#[test]
#[serial]
fn desktop_switch_mcp_skills_prompts_and_profile_are_independent() {
    let home = TestHome::new(true);
    let state = home.state();
    for (app, key) in [(AppType::Codex, "cli"), (AppType::CodexDesktop, "desktop")] {
        seed_template(&app);
        state
            .db
            .save_provider(
                app.as_str(),
                &provider("same-id", key, "https://example.invalid/v1"),
            )
            .unwrap();
        state
            .db
            .set_config_snippet(app.as_str(), Some(format!("test_namespace = \"{key}\"")))
            .unwrap();
        ProviderService::switch(&state, app, "same-id").unwrap();
    }
    let cli_before = live_bundle(&AppType::Codex);
    let desktop_before = live_bundle(&AppType::CodexDesktop);
    assert_ne!(cli_before, desktop_before);
    assert_eq!(
        state
            .db
            .get_config_snippet("codex-desktop")
            .unwrap()
            .as_deref(),
        Some("test_namespace = \"desktop\"")
    );
    let server = serde_json::from_value(json!({"id":"desktop-mcp","name":"desktop-mcp","server":{"command":"echo","args":["desktop"]},"apps":{"codex-desktop":true}})).unwrap();
    McpService::upsert_server(&state, server).unwrap();
    assert!(
        fs::read_to_string(home.root(&AppType::CodexDesktop).join("config.toml"))
            .unwrap()
            .contains("desktop-mcp")
    );
    assert_eq!(live_bundle(&AppType::Codex), cli_before);
    let skill = serde_json::from_value(json!({"id":"shared-skill","name":"shared-skill","directory":"shared-skill","apps":{},"installedAt":1})).unwrap();
    state.db.save_skill(&skill).unwrap();
    let source = SkillService::get_ssot_dir().unwrap().join("shared-skill");
    fs::create_dir_all(&source).unwrap();
    fs::write(
        source.join("SKILL.md"),
        "---\nname: shared-skill\ndescription: test\n---\n",
    )
    .unwrap();
    SkillService::toggle_app(&state.db, "shared-skill", &AppType::CodexDesktop, true).unwrap();
    assert!(home
        .root(&AppType::CodexDesktop)
        .join("skills/shared-skill/SKILL.md")
        .exists());
    assert!(!home
        .root(&AppType::Codex)
        .join("skills/shared-skill")
        .exists());
    for (app, text) in [
        (AppType::Codex, "CLI instructions"),
        (AppType::CodexDesktop, "Desktop instructions"),
    ] {
        let prompt = serde_json::from_value(
            json!({"id":"same-prompt","name":"Prompt","content":text,"enabled":false}),
        )
        .unwrap();
        state.db.save_prompt(app.as_str(), &prompt).unwrap();
        PromptService::enable_prompt(&state, app, "same-prompt").unwrap();
    }
    let profile =
        ProfileService::create(&state, "shared project", ProfileScope::CodexDesktop).unwrap();
    McpService::toggle_app(&state, "desktop-mcp", AppType::CodexDesktop, false).unwrap();
    SkillService::toggle_app(&state.db, "shared-skill", &AppType::CodexDesktop, false).unwrap();
    let (warnings, _) =
        ProfileService::apply(&state, &profile.id, ProfileScope::CodexDesktop).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    assert!(
        state.db.get_all_mcp_servers().unwrap()["desktop-mcp"]
            .apps
            .codex_desktop
    );
    assert!(
        state.db.get_all_installed_skills().unwrap()["shared-skill"]
            .apps
            .codex_desktop
    );
    assert_eq!(state.db.get_current_profile_id("codex").unwrap(), None);
    assert_eq!(
        fs::read_to_string(home.root(&AppType::Codex).join("AGENTS.md")).unwrap(),
        "CLI instructions"
    );
    assert_eq!(
        fs::read_to_string(home.root(&AppType::CodexDesktop).join("AGENTS.md")).unwrap(),
        "Desktop instructions"
    );
    // Remote snapshots can disable the last binding without invoking the toggle command.
    state
        .db
        .update_mcp_server_app_enabled("desktop-mcp", &AppType::CodexDesktop, false)
        .unwrap();
    state
        .db
        .update_skill_apps("shared-skill", &Default::default())
        .unwrap();
    crate::settings::reload_settings().unwrap();
    McpService::sync_enabled_for_app(&state, &AppType::CodexDesktop).unwrap();
    SkillService::sync_to_app(&state.db, &AppType::CodexDesktop).unwrap();
    assert!(
        !fs::read_to_string(home.root(&AppType::CodexDesktop).join("config.toml"))
            .unwrap()
            .contains("desktop-mcp")
    );
    assert!(!home
        .root(&AppType::CodexDesktop)
        .join("skills/shared-skill")
        .exists());
    assert_eq!(live_bundle(&AppType::Codex), cli_before);
}

#[tokio::test]
#[serial]
async fn desktop_conflict_blocks_writes_takeover_recovery_and_sync() {
    let home = TestHome::new(false);
    let state = home.state();
    seed_template(&AppType::Codex);
    let p = provider("shared", "cli", "https://example.invalid/v1");
    state.db.save_provider("codex", &p).unwrap();
    ProviderService::switch(&state, AppType::Codex, "shared").unwrap();
    let before = live_bundle(&AppType::Codex);
    assert!(config::codex_desktop_directory_conflict());
    ProviderService::add(&state, AppType::CodexDesktop, p, false).unwrap();
    assert_eq!(
        state.db.get_current_provider("codex-desktop").unwrap(),
        None
    );
    assert!(ProviderService::switch(&state, AppType::CodexDesktop, "shared").is_err());
    assert!(state
        .proxy_service
        .set_takeover_for_app("codex-desktop", true)
        .await
        .is_err());
    assert!(!state.proxy_service.is_running().await);
    assert!(
        crate::codex_history_migration::restore_codex_official_history_from_backups_for_app(
            &AppType::CodexDesktop
        )
        .is_err()
    );
    assert!(
        crate::services::session_usage_codex::sync_codex_usage_for_app(
            &AppType::CodexDesktop,
            &state.db
        )
        .is_err()
    );
    ProviderService::sync_current_to_live(&state).unwrap();
    assert_eq!(live_bundle(&AppType::Codex), before);
    state
        .db
        .save_live_backup(
            "codex-desktop",
            &json!({"auth":{"OPENAI_API_KEY":"wrong"},"config":""}).to_string(),
        )
        .await
        .unwrap();
    assert!(state
        .proxy_service
        .stop_with_restore_keep_state()
        .await
        .is_err());
    assert!(state
        .db
        .get_live_backup("codex-desktop")
        .await
        .unwrap()
        .is_some());
    assert_eq!(live_bundle(&AppType::Codex), before);
}

#[test]
#[serial]
fn desktop_directory_aliases_are_detected() {
    let home = TestHome::new(false);
    let root = home.root(&AppType::Codex);
    fs::create_dir_all(&root).unwrap();
    let alias = home.dir.path().join("alias");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&root, &alias).unwrap();
    #[cfg(windows)]
    assert!(std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(&alias)
        .arg(&root)
        .output()
        .unwrap()
        .status
        .success());
    let mut settings = crate::settings::get_settings();
    settings.codex_desktop_config_dir = Some(alias.to_string_lossy().into_owned());
    crate::settings::update_settings(settings.clone()).unwrap();
    assert!(config::codex_desktop_directory_conflict());
    #[cfg(windows)]
    {
        settings.codex_desktop_config_dir =
            Some(root.to_string_lossy().replace('\\', "/").to_uppercase() + "/");
        crate::settings::update_settings(settings).unwrap();
        assert!(config::codex_desktop_directory_conflict());
    }
}

#[test]
#[serial]
fn desktop_same_session_id_reads_deletes_and_rebuilds_only_its_directory() {
    let home = TestHome::new(true);
    let state = home.state();
    let id = "11111111-1111-4111-8111-111111111111";
    let mut paths = Vec::new();
    for (app, folder, tokens) in [
        (AppType::Codex, "sessions", 100),
        (AppType::CodexDesktop, "archived_sessions", 200),
    ] {
        let dir = home.root(&app).join(folder);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("rollout-2026-07-10T03-00-00-{id}.jsonl"));
        let records = [
            json!({"timestamp":"2026-07-10T03:00:00Z","type":"session_meta","payload":{"id":id,"cwd":"/project","model_provider":"openai"}}),
            json!({"timestamp":"2026-07-10T03:00:01Z","type":"turn_context","payload":{"model":"gpt-5.5"}}),
            json!({"timestamp":"2026-07-10T03:00:02Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":tokens,"cached_input_tokens":10,"output_tokens":5,"total_tokens":tokens+5}}}}),
            json!({"timestamp":"2026-07-10T03:00:03Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":app.as_str()}]}}),
        ];
        fs::write(
            &path,
            records
                .iter()
                .map(Value::to_string)
                .collect::<Vec<_>>()
                .join("\n")
                + "\n",
        )
        .unwrap();
        let result =
            crate::services::session_usage_codex::sync_codex_usage_for_app(&app, &state.db)
                .unwrap();
        assert_eq!(result.imported, 1, "{}: {result:?}", app.as_str());
        assert_eq!(
            crate::session_manager::providers::codex::scan_sessions_for_app(&app)[0].provider_id,
            app.as_str()
        );
        paths.push(path);
    }
    let cli_path = paths[0].to_string_lossy();
    let desktop_path = paths[1].to_string_lossy();
    assert!(crate::session_manager::load_messages("codex-desktop", &cli_path).is_err());
    assert!(crate::session_manager::load_messages("codex", &desktop_path).is_err());
    assert!(
        crate::session_manager::load_messages("codex-desktop", &desktop_path)
            .unwrap()
            .iter()
            .any(|m| m.content == "codex-desktop")
    );
    assert!(crate::session_manager::delete_session("codex-desktop", id, &cli_path).is_err());
    state
        .db
        .reset_codex_usage_for_app(&AppType::CodexDesktop)
        .unwrap();
    {
        let conn = state.db.conn.lock().unwrap();
        let rows: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM proxy_request_logs WHERE app_type='codex'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(rows, 1);
    }
    assert_eq!(
        crate::services::session_usage_codex::sync_codex_usage_for_app(
            &AppType::CodexDesktop,
            &state.db
        )
        .unwrap()
        .imported,
        1
    );
    crate::session_manager::delete_session("codex-desktop", id, &desktop_path).unwrap();
    assert!(paths[0].exists());
    assert!(!paths[1].exists());
}

#[derive(Clone)]
struct Upstream {
    url: String,
    fail: Arc<std::sync::atomic::AtomicBool>,
    requests: Arc<std::sync::Mutex<Vec<(String, String)>>>,
}
async fn mock_upstream(label: &'static str) -> (Upstream, tokio::task::JoinHandle<()>) {
    use axum::{
        extract::State,
        http::{HeaderMap, StatusCode, Uri},
        routing::any,
        Json, Router,
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mock = Upstream {
        url: format!("http://{}/v1", listener.local_addr().unwrap()),
        fail: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        requests: Default::default(),
    };
    let router=Router::new().fallback(any(move |State(mock): State<Upstream>, uri: Uri, headers: HeaderMap, Json(_body): Json<Value>| async move {
        mock.requests.lock().unwrap().push((uri.path().to_string(),headers.get("authorization").and_then(|v|v.to_str().ok()).unwrap_or("").to_string()));
        if mock.fail.load(std::sync::atomic::Ordering::SeqCst) {
            return (StatusCode::SERVICE_UNAVAILABLE,Json(json!({"error":{"message":"temporary test outage"}})));
        }
        let body=if uri.path().ends_with("chat/completions") {
            json!({"id":"same-response-id","object":"chat.completion","model":"gpt-5.5","choices":[{"index":0,"message":{"role":"assistant","content":label},"finish_reason":"stop"}],"usage":{"prompt_tokens":30,"completion_tokens":5,"total_tokens":35}})
        } else {
            json!({"id":"same-response-id","object":"response","model":"gpt-5.5","status":"completed","output":[{"type":"message","role":"assistant","content":[{"type":"output_text","text":label}]}],"usage":{"input_tokens":30,"output_tokens":5,"total_tokens":35}})
        };
        (StatusCode::OK,Json(body))
    })).with_state(mock.clone());
    let task = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    (mock, task)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial]
async fn desktop_proxy_routes_protocol_conversion_failover_and_stop_are_independent() {
    let home = TestHome::new(true);
    let state = home.state();
    let (cli_upstream, cli_task) = mock_upstream("CLI upstream").await;
    let (desktop_upstream, desktop_task) = mock_upstream("Desktop upstream").await;
    let mut proxy_config = state.db.get_proxy_config().await.unwrap();
    proxy_config.listen_address = "127.0.0.1".into();
    proxy_config.listen_port = 0;
    state.db.update_proxy_config(proxy_config).await.unwrap();
    for (app, key, url) in [
        (AppType::Codex, "cli", &cli_upstream.url),
        (AppType::CodexDesktop, "desktop", &desktop_upstream.url),
    ] {
        seed_template(&app);
        state
            .db
            .save_provider(app.as_str(), &provider("same-id", key, url))
            .unwrap();
        ProviderService::switch(&state, app, "same-id").unwrap();
    }
    state
        .proxy_service
        .set_takeover_for_app("codex", true)
        .await
        .unwrap();
    let cli_taken_over = live_bundle(&AppType::Codex);
    state
        .proxy_service
        .set_takeover_for_app("codex-desktop", true)
        .await
        .unwrap();
    assert_eq!(live_bundle(&AppType::Codex), cli_taken_over);
    let status = state.proxy_service.get_takeover_status().await.unwrap();
    assert!(status.codex && status.codex_desktop);
    let port = state.proxy_service.get_status().await.unwrap().port;
    let base = format!("http://127.0.0.1:{port}");
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    for path in [
        "responses",
        "responses/compact",
        "alpha/search",
        "images/generations",
        "images/edits",
        "chat/completions",
    ] {
        let response=client.post(format!("{base}/codex-desktop/v1/{path}")).json(&json!({"model":"gpt-5.5","input":"test","messages":[{"role":"user","content":"test"}],"stream":false})).send().await.unwrap();
        assert!(
            response.status().is_success(),
            "{path}: {}",
            response.text().await.unwrap()
        );
    }
    let model_list: Value = client
        .get(format!("{base}/codex-desktop/v1/models"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(model_list.to_string().contains("model-desktop"));
    assert!(desktop_upstream
        .requests
        .lock()
        .unwrap()
        .iter()
        .all(|(path, key)| !path.contains("codex-desktop") && key == "Bearer desktop"));
    assert_eq!(cli_upstream.requests.lock().unwrap().len(), 0);
    // Enable the Chat bridge only for Desktop and verify conversion back to Responses.
    let mut chat = state
        .db
        .get_provider_by_id("same-id", "codex-desktop")
        .unwrap()
        .unwrap();
    chat.meta = Some(crate::ProviderMeta {
        api_format: Some("openai_chat".into()),
        ..Default::default()
    });
    ProviderService::update(&state, AppType::CodexDesktop, None, chat).unwrap();
    let converted: Value = client
        .post(format!("{base}/codex-desktop/v1/responses"))
        .json(&json!({"model":"gpt-5.5","input":"hello","stream":false}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(converted["object"], "response");
    assert!(converted.to_string().contains("Desktop upstream"));
    assert!(desktop_upstream
        .requests
        .lock()
        .unwrap()
        .last()
        .unwrap()
        .0
        .ends_with("chat/completions"));
    let fallback = provider("fallback", "desktop-fallback", &cli_upstream.url);
    state.db.save_provider("codex-desktop", &fallback).unwrap();
    state
        .db
        .add_to_failover_queue("codex-desktop", "same-id")
        .unwrap();
    state
        .db
        .add_to_failover_queue("codex-desktop", "fallback")
        .unwrap();
    let mut desktop_config = state
        .db
        .get_proxy_config_for_app("codex-desktop")
        .await
        .unwrap();
    desktop_config.auto_failover_enabled = true;
    desktop_config.max_retries = 0;
    state
        .db
        .update_proxy_config_for_app(desktop_config)
        .await
        .unwrap();
    desktop_upstream
        .fail
        .store(true, std::sync::atomic::Ordering::SeqCst);
    let response = client
        .post(format!("{base}/codex-desktop/v1/responses"))
        .json(&json!({"model":"gpt-5.5","input":"fallback","stream":false}))
        .send()
        .await
        .unwrap();
    assert!(
        response.status().is_success(),
        "{}",
        response.text().await.unwrap()
    );
    assert!(cli_upstream
        .requests
        .lock()
        .unwrap()
        .iter()
        .any(|(_, key)| key == "Bearer desktop-fallback"));
    let response = client
        .post(format!("{base}/v1/responses"))
        .json(&json!({"model":"gpt-5.5","input":"CLI still works","stream":false}))
        .send()
        .await
        .unwrap();
    assert!(response.status().is_success());
    assert_eq!(
        cli_upstream.requests.lock().unwrap().last().unwrap().1,
        "Bearer cli"
    );
    assert_eq!(
        state.db.get_current_provider("codex").unwrap().as_deref(),
        Some("same-id")
    );
    state
        .proxy_service
        .set_takeover_for_app("codex-desktop", false)
        .await
        .unwrap();
    assert_eq!(live_bundle(&AppType::Codex), cli_taken_over);
    assert!(state.proxy_service.is_running().await);
    assert!(
        !state
            .proxy_service
            .get_takeover_status()
            .await
            .unwrap()
            .codex_desktop
    );
    state
        .proxy_service
        .set_takeover_for_app("codex", false)
        .await
        .unwrap();
    assert!(!state.proxy_service.is_running().await);
    assert!(
        !fs::read_to_string(home.root(&AppType::Codex).join("config.toml"))
            .unwrap()
            .contains(&base)
    );
    cli_task.abort();
    desktop_task.abort();
}

#[test]
#[serial]
fn desktop_managed_auth_refresh_and_cleanup_are_scoped() {
    let _home = TestHome::new(true);
    let id_token = config::test_codex_id_token("shared-user");
    let auth = config::codex_managed_oauth_auth_value(
        "workspace",
        "access-old",
        Some(&id_token),
        "refresh-old",
        "2026-09-01T00:00:00Z",
    );
    let refreshed = config::codex_managed_oauth_auth_value(
        "workspace",
        "access-new",
        Some(&id_token),
        "refresh-new",
        "2026-09-02T00:00:00Z",
    );
    for app in [AppType::Codex, AppType::CodexDesktop] {
        config::write_codex_live_atomic_for_app(&app, &auth, Some("model = \"gpt-5.5\"\n"))
            .unwrap();
        config::record_codex_managed_oauth_live_auth_for_app(&app, &auth, "shared-account")
            .unwrap();
    }
    let cli_before = contents(config::get_codex_auth_path_for_app(&AppType::Codex));
    assert!(
        config::sync_codex_managed_oauth_live_auth_after_refresh_for_app(
            &AppType::CodexDesktop,
            "shared-account",
            "refresh-old",
            &refreshed
        )
        .unwrap()
    );
    assert_eq!(
        contents(config::get_codex_auth_path_for_app(&AppType::Codex)),
        cli_before
    );
    assert!(
        !config::sync_codex_managed_oauth_live_auth_after_refresh_for_app(
            &AppType::CodexDesktop,
            "shared-account",
            "refresh-old",
            &auth
        )
        .unwrap()
    );
    config::clear_codex_live_auth_for_managed_account_for_app(
        &AppType::CodexDesktop,
        "shared-account",
    )
    .unwrap();
    assert!(!config::get_codex_auth_path_for_app(&AppType::CodexDesktop).exists());
    assert!(config::codex_managed_oauth_live_auth_marker_exists_for_app(
        &AppType::Codex
    ));
    assert_eq!(
        contents(config::get_codex_auth_path_for_app(&AppType::Codex)),
        cli_before
    );
}

#[tokio::test]
#[serial]
async fn desktop_crash_recovery_detects_takeover_without_a_backup() {
    let home = TestHome::new(true);
    let state = home.state();
    for (app, key) in [
        (AppType::Codex, "cli-key"),
        (AppType::CodexDesktop, "desktop-key"),
    ] {
        seed_template(&app);
        state
            .db
            .save_provider(
                app.as_str(),
                &provider("same-id", key, "https://example.invalid/v1"),
            )
            .unwrap();
        ProviderService::switch(&state, app, "same-id").unwrap();
    }
    let cli_before = live_bundle(&AppType::Codex);
    let desktop_path = config::get_codex_config_path_for_app(&AppType::CodexDesktop);
    let original = fs::read_to_string(&desktop_path).unwrap();
    let taken_over = original.replace("desktop-key", "PROXY_MANAGED");
    assert!(taken_over.contains("PROXY_MANAGED"));
    fs::write(&desktop_path, taken_over).unwrap();
    assert!(state
        .db
        .get_live_backup("codex-desktop")
        .await
        .unwrap()
        .is_none());
    assert!(state.proxy_service.detect_takeover_in_live_configs());
    state.proxy_service.recover_from_crash().await.unwrap();
    assert!(!state.proxy_service.detect_takeover_in_live_configs());
    assert_eq!(fs::read_to_string(desktop_path).unwrap(), original);
    assert_eq!(live_bundle(&AppType::Codex), cli_before);
}

#[test]
#[serial]
fn desktop_history_migration_and_restore_keep_cli_files_and_markers() {
    let home = TestHome::new(true);
    let mut paths = Vec::new();
    for app in [AppType::Codex, AppType::CodexDesktop] {
        let root = home.root(&app);
        fs::create_dir_all(root.join("sessions")).unwrap();
        let file = root.join("sessions/rollout-test.jsonl");
        fs::write(
            &file,
            json!({"type":"session_meta","payload":{"id":"same-id","model_provider":"openai"}})
                .to_string()
                + "\n",
        )
        .unwrap();
        fs::write(root.join("config.toml"), "model_provider = \"custom\"\n").unwrap();
        paths.push(file);
    }
    let cli_before = contents(paths[0].clone());
    let mut settings = crate::settings::get_settings();
    settings.unify_codex_desktop_session_history = true;
    settings.unify_codex_desktop_migrate_existing = Some(true);
    crate::settings::update_settings(settings).unwrap();
    let outcome = crate::codex_history_migration::maybe_migrate_codex_official_history_to_unified_bucket_for_app(&AppType::CodexDesktop).unwrap();
    assert_eq!(outcome.migrated_jsonl_files, 1, "{outcome:?}");
    assert!(fs::read_to_string(&paths[1]).unwrap().contains("custom"));
    assert_eq!(contents(paths[0].clone()), cli_before);
    assert!(
        !crate::codex_history_migration::has_codex_official_history_unify_backup_for_app(
            &AppType::Codex
        )
    );
    let mut settings = crate::settings::get_settings();
    settings.unify_codex_desktop_session_history = false;
    crate::settings::update_settings(settings).unwrap();
    crate::codex_history_migration::restore_codex_official_history_from_backups_for_app(
        &AppType::CodexDesktop,
    )
    .unwrap();
    assert_eq!(contents(paths[1].clone()), cli_before);
    assert_eq!(contents(paths[0].clone()), cli_before);
}

#[test]
#[serial]
fn desktop_extension_upgrades_existing_v19_and_round_trips_sync() {
    let _home = TestHome::new(true);
    let db = Database::init().unwrap();
    {
        let conn = db.conn.lock().unwrap();
        // Simulate the existing v19 layout without Desktop columns or CHECK value.
        let ddl: String = conn
            .query_row(
                "SELECT sql FROM sqlite_master WHERE name='proxy_config'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let legacy = ddl
            .replacen("proxy_config", "legacy_proxy_config", 1)
            .replace("'codex-desktop',", "");
        conn.execute_batch(&legacy).unwrap();
        conn.execute_batch("INSERT INTO legacy_proxy_config SELECT * FROM proxy_config WHERE app_type <> 'codex-desktop'; DROP TABLE proxy_config; ALTER TABLE legacy_proxy_config RENAME TO proxy_config; ALTER TABLE mcp_servers DROP COLUMN enabled_codex_desktop; ALTER TABLE skills DROP COLUMN enabled_codex_desktop; UPDATE proxy_config SET enabled=1, auto_failover_enabled=1 WHERE app_type='codex';").unwrap();
        Database::ensure_codex_desktop_schema(&conn).unwrap();
        let flags: (bool,bool,bool) = conn.query_row("SELECT (SELECT enabled FROM proxy_config WHERE app_type='codex'), enabled, auto_failover_enabled FROM proxy_config WHERE app_type='codex-desktop'", [], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?))).unwrap();
        assert_eq!(flags, (true, false, false));
        assert_eq!(
            conn.query_row("PRAGMA user_version", [], |row| row.get::<_, u32>(0))
                .unwrap(),
            19
        );
    }
    for (app, key) in [("codex", "cli"), ("codex-desktop", "desktop")] {
        db.save_provider(app, &provider("same-id", key, "https://example.invalid/v1"))
            .unwrap();
    }
    db.save_skill(&serde_json::from_value(json!({"id":"shared","name":"shared","directory":"shared","apps":{"codex-desktop":true},"installedAt":1})).unwrap()).unwrap();
    db.save_mcp_server(&serde_json::from_value(json!({"id":"shared","name":"shared","server":{"command":"echo"},"apps":{"codex-desktop":true}})).unwrap()).unwrap();
    let remote = db.export_sql_string_for_sync().unwrap();
    let restored = Database::memory().unwrap();
    restored.import_sql_string_for_sync(&remote).unwrap();
    assert_eq!(
        restored
            .get_provider_by_id("same-id", "codex")
            .unwrap()
            .unwrap()
            .settings_config["auth"]["OPENAI_API_KEY"],
        "cli"
    );
    assert_eq!(
        restored
            .get_provider_by_id("same-id", "codex-desktop")
            .unwrap()
            .unwrap()
            .settings_config["auth"]["OPENAI_API_KEY"],
        "desktop"
    );
    assert!(
        restored.get_all_installed_skills().unwrap()["shared"]
            .apps
            .codex_desktop
    );
    assert!(
        !restored.get_all_installed_skills().unwrap()["shared"]
            .apps
            .codex
    );
    assert!(
        restored.get_all_mcp_servers().unwrap()["shared"]
            .apps
            .codex_desktop
    );
}
