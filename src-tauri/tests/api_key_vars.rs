//! API Key 变量：引用解析、切换拦截、刷新传播、写回保护、导出排除。

use std::{
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};

use serde_json::json;

use cc_switch_lib::api_key_vars::{
    self,
    resolve::{cache_remove, lookup, RefStatus},
    runner::{
        self, ApiKeyRefreshConfig, ApiKeyRefreshState, RefreshOutput, ScriptContext, ScriptRun,
    },
    ApiKeyVarInput,
};
use cc_switch_lib::{AppState, AppType, Database, MultiAppConfig, Provider, ProviderService};

#[path = "support.rs"]
mod support;
use support::{create_test_state_with_config, ensure_test_home, reset_test_fs, test_mutex};

fn cfg(enabled: bool) -> ApiKeyRefreshConfig {
    ApiKeyRefreshConfig {
        enabled,
        script_dir: String::new(),
        timeout_secs: None,
        lead_time_secs: None,
    }
}

fn success(value: &str) -> Result<ScriptRun, api_key_vars::runner::RunFailure> {
    Ok(ScriptRun {
        output: RefreshOutput {
            api_key: value.to_string(),
            expires_at: None,
            refresh_after: None,
            message: None,
        },
        resolved_file: "index.mjs".into(),
    })
}

fn config_with(app: AppType, providers: Vec<Provider>) -> MultiAppConfig {
    let mut config = MultiAppConfig::default();
    let manager = config.get_manager_mut(&app).expect("manager");
    for p in providers {
        manager.providers.insert(p.id.clone(), p);
    }
    config
}

fn read(path: std::path::PathBuf) -> String {
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

fn run_node_script(
    dir: &Path,
    var_name: &str,
    timeout_secs: u64,
    known_secrets: &[&str],
) -> Result<ScriptRun, runner::RunFailure> {
    let cfg = ApiKeyRefreshConfig {
        enabled: true,
        script_dir: dir.display().to_string(),
        timeout_secs: Some(timeout_secs),
        lead_time_secs: None,
    };
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime")
        .block_on(async {
            let node = runner::resolve_node(None)
                .await
                .expect("Node is required for the E2E fixture");
            runner::run_script(
                &cfg,
                &ScriptContext {
                    var_name: var_name.to_string(),
                    reason: "manual",
                    current_expires_at: None,
                },
                Some(node.to_str().expect("Unicode node path")),
                known_secrets,
            )
            .await
        })
}

fn shared_key_config() -> MultiAppConfig {
    let codex_config = r#"model_provider = "relay"
model = "gpt-5.4"

[model_providers.relay]
name = "Relay"
base_url = "https://relay.example/v1"
wire_api = "responses"
"#;
    let mut config = config_with(
        AppType::Codex,
        vec![Provider::with_id(
            "codex-var".into(),
            "Codex Var".into(),
            json!({"auth": {"OPENAI_API_KEY": "{{IT_E2E_KEY}}"}, "config": codex_config}),
            None,
        )],
    );
    config.get_manager_mut(&AppType::Claude).unwrap().providers.insert(
        "claude-var".into(),
        Provider::with_id(
            "claude-var".into(),
            "Claude Var".into(),
            json!({"env": {"ANTHROPIC_AUTH_TOKEN": "{{IT_E2E_KEY}}", "ANTHROPIC_BASE_URL": "https://relay.example"}}),
            None,
        ),
    );
    config.get_manager_mut(&AppType::Gemini).unwrap().providers.insert(
        "gemini-var".into(),
        Provider::with_id(
            "gemini-var".into(),
            "Gemini Var".into(),
            json!({"env": {"GEMINI_API_KEY": "{{IT_E2E_KEY}}", "GOOGLE_GEMINI_BASE_URL": "https://relay.example"}}),
            None,
        ),
    );
    config
}

#[test]
fn claude_switch_resolves_ref_and_keeps_template_in_db() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    let home = ensure_test_home();

    let config = config_with(
        AppType::Claude,
        vec![
            Provider::with_id(
                "var-claude".into(),
                "Var Claude".into(),
                json!({"env": {
                    "ANTHROPIC_AUTH_TOKEN": "{{IT_CLAUDE_KEY}}",
                    "ANTHROPIC_BASE_URL": "https://relay.example.com"
                }}),
                None,
            ),
            Provider::with_id(
                "plain".into(),
                "Plain".into(),
                json!({"env": {
                    "ANTHROPIC_AUTH_TOKEN": "sk-plain",
                    "ANTHROPIC_BASE_URL": "https://plain.example.com"
                }}),
                None,
            ),
        ],
    );
    let state = create_test_state_with_config(&config).expect("state");

    // 变量未定义：拒绝切换，live 不写
    let err = ProviderService::switch(&state, AppType::Claude, "var-claude")
        .expect_err("unknown variable must block switching");
    assert!(err.to_string().contains("IT_CLAUDE_KEY"), "{err}");
    let settings_path = home.join(".claude").join("settings.json");
    assert!(
        !settings_path.exists() || !read(settings_path.clone()).contains("{{"),
        "literal ref must never reach live"
    );

    // 已定义但尚无值：同样拒绝
    state
        .db
        .upsert_api_key_var(None, "IT_CLAUDE_KEY", &cfg(true))
        .unwrap();
    assert!(ProviderService::switch(&state, AppType::Claude, "var-claude").is_err());

    // 有值：切换成功，live 写入真实值，DB 保留模板
    api_key_vars::commit_result(&state, "IT_CLAUDE_KEY", success("sk-var-one")).unwrap();
    ProviderService::switch(&state, AppType::Claude, "var-claude").expect("switch");
    let live: serde_json::Value = serde_json::from_str(&read(settings_path.clone())).unwrap();
    assert_eq!(live["env"]["ANTHROPIC_AUTH_TOKEN"], "sk-var-one");
    assert!(!read(settings_path.clone()).contains("{{"));
    let row = state
        .db
        .get_provider_by_id("var-claude", "claude")
        .unwrap()
        .unwrap();
    assert_eq!(
        row.settings_config["env"]["ANTHROPIC_AUTH_TOKEN"],
        "{{IT_CLAUDE_KEY}}"
    );

    // 刷新：当前供应商的 live 被重写
    let outcome =
        api_key_vars::commit_result(&state, "IT_CLAUDE_KEY", success("sk-var-two")).unwrap();
    assert!(outcome.success);
    assert_eq!(
        outcome.updated_providers.len(),
        1,
        "{:?}",
        outcome.live_errors
    );
    let live: serde_json::Value = serde_json::from_str(&read(settings_path.clone())).unwrap();
    assert_eq!(live["env"]["ANTHROPIC_AUTH_TOKEN"], "sk-var-two");

    // 写回保护：把解析后的值写回库时保留模板
    let mut resolved_row = row.clone();
    resolved_row.settings_config["env"]["ANTHROPIC_AUTH_TOKEN"] = json!("sk-var-two");
    state.db.save_provider("claude", &resolved_row).unwrap();
    state
        .db
        .update_provider_settings_config("claude", "var-claude", &resolved_row.settings_config)
        .unwrap();
    let row = state
        .db
        .get_provider_by_id("var-claude", "claude")
        .unwrap()
        .unwrap();
    assert_eq!(
        row.settings_config["env"]["ANTHROPIC_AUTH_TOKEN"],
        "{{IT_CLAUDE_KEY}}"
    );

    // 用量凭据解析
    let (_, key) = row.resolve_usage_credentials(&AppType::Claude);
    assert_eq!(key, "sk-var-two");

    // 删除被引用的变量：拒绝
    let err = api_key_vars::delete(&state, "IT_CLAUDE_KEY").expect_err("in use");
    assert!(err.to_string().contains("Var Claude"), "{err}");

    // 改名：自动改写引用
    api_key_vars::upsert(
        &state,
        &ApiKeyVarInput {
            name: "IT_CLAUDE_RENAMED".into(),
            script_dir: String::new(),
            enabled: false,
            timeout_secs: None,
            lead_time_secs: None,
        },
        Some("IT_CLAUDE_KEY"),
    )
    .unwrap();
    let row = state
        .db
        .get_provider_by_id("var-claude", "claude")
        .unwrap()
        .unwrap();
    assert_eq!(
        row.settings_config["env"]["ANTHROPIC_AUTH_TOKEN"],
        "{{IT_CLAUDE_RENAMED}}"
    );

    // 停用后：切到别的再切回来被拒绝
    ProviderService::switch(&state, AppType::Claude, "plain").expect("switch plain");
    assert!(ProviderService::switch(&state, AppType::Claude, "var-claude").is_err());
    let live: serde_json::Value = serde_json::from_str(&read(settings_path.clone())).unwrap();
    assert_eq!(live["env"]["ANTHROPIC_AUTH_TOKEN"], "sk-plain");

    // 未被引用后可以删除
    ProviderService::delete(&state, AppType::Claude, "var-claude").ok();
    if state
        .db
        .get_provider_by_id("var-claude", "claude")
        .unwrap()
        .is_none()
    {
        api_key_vars::delete(&state, "IT_CLAUDE_RENAMED").expect("delete unreferenced");
    }
}

#[test]
fn codex_and_gemini_switch_resolve_refs() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    let home = ensure_test_home();

    let codex_config = r#"model_provider = "relay"
model = "gpt-5.4"

[model_providers.relay]
name = "Relay"
base_url = "https://relay.example/v1"
wire_api = "responses"
"#;
    let mut config = config_with(
        AppType::Codex,
        vec![Provider::with_id(
            "codex-var".into(),
            "Codex Var".into(),
            json!({"auth": {"OPENAI_API_KEY": "{{IT_SHARED_KEY}}"}, "config": codex_config}),
            None,
        )],
    );
    config
        .get_manager_mut(&AppType::Gemini)
        .unwrap()
        .providers
        .insert(
            "gemini-var".into(),
            Provider::with_id(
                "gemini-var".into(),
                "Gemini Var".into(),
                json!({"env": {
                    "GEMINI_API_KEY": "{{IT_SHARED_KEY}}",
                    "GOOGLE_GEMINI_BASE_URL": "https://g.example.com"
                }}),
                None,
            ),
        );
    let state = create_test_state_with_config(&config).expect("state");
    state
        .db
        .upsert_api_key_var(None, "IT_SHARED_KEY", &cfg(true))
        .unwrap();
    api_key_vars::commit_result(&state, "IT_SHARED_KEY", success("sk-shared-1")).unwrap();

    ProviderService::switch(&state, AppType::Codex, "codex-var").expect("codex switch");
    ProviderService::switch(&state, AppType::Gemini, "gemini-var").expect("gemini switch");

    let codex_live = read(cc_switch_lib::get_codex_config_path());
    assert!(codex_live.contains("sk-shared-1"), "{codex_live}");
    assert!(!codex_live.contains("{{"), "{codex_live}");
    let gemini_env = read(home.join(".gemini").join(".env"));
    assert!(
        gemini_env.contains("GEMINI_API_KEY=sk-shared-1"),
        "{gemini_env}"
    );

    // 一个变量刷新：两个应用的当前供应商都更新
    let outcome =
        api_key_vars::commit_result(&state, "IT_SHARED_KEY", success("sk-shared-2")).unwrap();
    assert_eq!(
        outcome.updated_providers.len(),
        2,
        "{:?}",
        outcome.live_errors
    );
    assert!(read(cc_switch_lib::get_codex_config_path()).contains("sk-shared-2"));
    assert!(read(home.join(".gemini").join(".env")).contains("GEMINI_API_KEY=sk-shared-2"));

    // 视图只含掩码值
    let views = api_key_vars::list_views(&state.db).unwrap();
    let view = views.iter().find(|v| v.name == "IT_SHARED_KEY").unwrap();
    assert_eq!(view.references.len(), 2);
    let json = serde_json::to_string(&views).unwrap();
    assert!(!json.contains("sk-shared-2"), "{json}");
}

#[test]
fn exports_never_contain_variable_values_and_imports_keep_local_vars() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    let _home = ensure_test_home();
    let state = create_test_state_with_config(&MultiAppConfig::default()).expect("state");
    state
        .db
        .upsert_api_key_var(None, "IT_EXPORT_KEY", &cfg(true))
        .unwrap();
    state
        .db
        .update_api_key_var_result(
            "IT_EXPORT_KEY",
            Some("sk-secret-export-value"),
            &ApiKeyRefreshState::default(),
        )
        .unwrap();

    let full = state.db.export_sql_string().unwrap();
    assert!(!full.contains("sk-secret-export-value"));
    assert!(!full.contains("INSERT INTO \"api_key_vars\""));
    let sync = state.db.export_sql_string_for_sync().unwrap();
    assert!(!sync.contains("sk-secret-export-value"));

    // 导入一份带外来变量的 SQL：外来的丢弃，本机的保留
    let foreign = full.replace(
        "COMMIT;",
        "INSERT INTO api_key_vars (name, script_dir, enabled, value) VALUES ('IT_FOREIGN', '/evil', 1, 'sk-foreign');\nCOMMIT;",
    );
    state.db.import_sql_string(&foreign).expect("import");
    let vars = state.db.list_api_key_vars().unwrap();
    assert!(
        vars.iter()
            .any(|v| v.name == "IT_EXPORT_KEY"
                && v.value.as_deref() == Some("sk-secret-export-value"))
    );
    assert!(!vars.iter().any(|v| v.name == "IT_FOREIGN"), "{vars:?}");
}

#[test]
fn real_node_refresh_updates_shared_live_configs_and_retries_same_value_after_live_error() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    let home = ensure_test_home();
    let script_dir = tempfile::tempdir().expect("script directory");
    let script = script_dir.path().join("index.mjs");
    std::fs::write(
        &script,
        r#"process.stdout.write(JSON.stringify({ apiKey: "dummy-e2e-key-one" }));"#,
    )
    .expect("write script");

    let state = create_test_state_with_config(&shared_key_config()).expect("state");
    state
        .db
        .upsert_api_key_var(
            None,
            "IT_E2E_KEY",
            &ApiKeyRefreshConfig {
                enabled: true,
                script_dir: script_dir.path().display().to_string(),
                timeout_secs: Some(5),
                lead_time_secs: None,
            },
        )
        .expect("variable");
    for (app, id) in [
        (AppType::Codex, "codex-var"),
        (AppType::Claude, "claude-var"),
        (AppType::Gemini, "gemini-var"),
    ] {
        state
            .db
            .set_current_provider(app.as_str(), id)
            .expect("current provider");
    }

    let first = api_key_vars::commit_result(
        &state,
        "IT_E2E_KEY",
        run_node_script(script_dir.path(), "IT_E2E_KEY", 5, &[]),
    )
    .expect("commit first script result");
    assert!(first.success);
    assert_eq!(first.updated_providers.len(), 3, "{first:?}");
    let codex_path = cc_switch_lib::get_codex_config_path();
    assert!(
        codex_path.starts_with(home),
        "{} is outside {}",
        codex_path.display(),
        home.display()
    );
    assert!(read(codex_path.clone()).contains("dummy-e2e-key-one"));
    assert!(read(home.join(".claude/settings.json")).contains("dummy-e2e-key-one"));
    assert!(read(home.join(".gemini/.env")).contains("dummy-e2e-key-one"));

    std::fs::write(
        &script,
        r#"process.stdout.write(JSON.stringify({ apiKey: "dummy-e2e-key-two" }));"#,
    )
    .expect("update script");
    let claude_path = home.join(".claude/settings.json");
    std::fs::write(&claude_path, "{ deliberately invalid JSON")
        .expect("corrupt isolated Claude live file");
    let failed_live = api_key_vars::commit_result(
        &state,
        "IT_E2E_KEY",
        run_node_script(script_dir.path(), "IT_E2E_KEY", 5, &[]),
    )
    .expect("commit second script result despite live error");
    assert!(failed_live.success);
    assert!(
        failed_live
            .live_errors
            .iter()
            .any(|error| error.contains("claude")),
        "{:?}",
        failed_live.live_errors
    );
    assert!(read(codex_path).contains("dummy-e2e-key-two"));
    assert!(read(home.join(".gemini/.env")).contains("dummy-e2e-key-two"));

    std::fs::remove_file(&claude_path).expect("remove corrupt isolated live file");
    let recovered = api_key_vars::commit_result(
        &state,
        "IT_E2E_KEY",
        run_node_script(script_dir.path(), "IT_E2E_KEY", 5, &[]),
    )
    .expect("commit unchanged key after repair");
    assert!(recovered.success);
    assert!(
        recovered.live_errors.is_empty(),
        "{:?}",
        recovered.live_errors
    );
    assert!(
        recovered
            .updated_providers
            .iter()
            .any(|reference| reference.app == "claude"),
        "{:?}",
        recovered.updated_providers
    );
    assert!(read(claude_path).contains("dummy-e2e-key-two"));

    for (app, id, pointer) in [
        (AppType::Codex, "codex-var", "/auth/OPENAI_API_KEY"),
        (AppType::Claude, "claude-var", "/env/ANTHROPIC_AUTH_TOKEN"),
        (AppType::Gemini, "gemini-var", "/env/GEMINI_API_KEY"),
    ] {
        let provider = state
            .db
            .get_provider_by_id(id, app.as_str())
            .expect("provider")
            .expect("provider exists");
        assert_eq!(
            provider.settings_config.pointer(pointer),
            Some(&json!("{{IT_E2E_KEY}}"))
        );
    }
}

#[test]
fn real_node_runner_rejects_malformed_stdout() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    let _home = ensure_test_home();
    let script_dir = tempfile::tempdir().expect("script directory");
    std::fs::write(
        script_dir.path().join("index.mjs"),
        "process.stdout.write('not json');",
    )
    .expect("write script");

    let failure = run_node_script(script_dir.path(), "IT_E2E_MALFORMED", 5, &[])
        .expect_err("malformed stdout must fail");
    assert!(!failure.permanent);
    assert!(failure.error.contains("stdout"), "{}", failure.error);
}

#[test]
fn real_node_runner_redacts_known_secret_from_permanent_error() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    let _home = ensure_test_home();
    let script_dir = tempfile::tempdir().expect("script directory");
    std::fs::write(
        script_dir.path().join("index.mjs"),
        "console.error('authentication rejected: dummy-redaction-secret'); process.exit(2);",
    )
    .expect("write script");

    let failure = run_node_script(
        script_dir.path(),
        "IT_E2E_REDACTION",
        5,
        &["dummy-redaction-secret"],
    )
    .expect_err("exit 2 must fail permanently");
    assert!(failure.permanent);
    assert!(failure.error.contains("[REDACTED]"), "{}", failure.error);
    assert!(
        !failure.error.contains("dummy-redaction-secret"),
        "{}",
        failure.error
    );
}

#[test]
fn real_node_runner_times_out_after_configured_five_seconds() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    let _home = ensure_test_home();
    let script_dir = tempfile::tempdir().expect("script directory");
    std::fs::write(
        script_dir.path().join("index.mjs"),
        "setTimeout(() => {}, 60000);",
    )
    .expect("write script");

    let started = Instant::now();
    let failure = run_node_script(script_dir.path(), "IT_E2E_TIMEOUT", 5, &[])
        .expect_err("five-second timeout must stop script");
    assert!(!failure.permanent);
    assert!(
        failure.error.contains("timeout") || failure.error.contains("超时"),
        "{}",
        failure.error
    );
    assert!(started.elapsed() >= Duration::from_secs(5));
    assert!(started.elapsed() < Duration::from_secs(12));
}

#[test]
fn restart_loads_persisted_variable_cache_before_switching_codex_live_config() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    let home = ensure_test_home();
    let config = config_with(
        AppType::Codex,
        vec![Provider::with_id(
            "codex-restart".into(),
            "Codex Restart".into(),
            json!({"auth": {"OPENAI_API_KEY": "{{IT_RESTART_KEY}}"}, "config": r#"model_provider = "relay"
model = "gpt-5.4"

[model_providers.relay]
name = "Relay"
base_url = "https://relay.example/v1"
wire_api = "responses"
"#}),
            None,
        )],
    );
    let state = create_test_state_with_config(&config).expect("initial state");
    state
        .db
        .upsert_api_key_var(None, "IT_RESTART_KEY", &cfg(true))
        .expect("variable");
    let future = api_key_vars::runner::now_ms() + 3_600_000;
    api_key_vars::commit_result(
        &state,
        "IT_RESTART_KEY",
        Ok(ScriptRun {
            output: RefreshOutput {
                api_key: "dummy-restart-key".to_string(),
                expires_at: Some(future),
                refresh_after: None,
                message: None,
            },
            resolved_file: "index.mjs".to_string(),
        }),
    )
    .expect("persist successful refresh");
    ProviderService::switch(&state, AppType::Codex, "codex-restart").expect("initial switch");
    drop(state);

    cache_remove("IT_RESTART_KEY");
    assert_eq!(lookup("IT_RESTART_KEY"), (RefStatus::Unknown, None));
    let db = Arc::new(Database::init().expect("reopen isolated database"));
    assert_eq!(
        db.load_api_key_var_cache().expect("load persisted cache"),
        1
    );
    assert_eq!(
        lookup("IT_RESTART_KEY"),
        (RefStatus::Ok, Some("dummy-restart-key".to_string()))
    );
    let restarted = AppState::new(db);
    ProviderService::switch(&restarted, AppType::Codex, "codex-restart")
        .expect("switch after restart without refresh");
    let live_path = cc_switch_lib::get_codex_config_path();
    assert!(live_path.starts_with(home));
    assert!(read(live_path).contains("dummy-restart-key"));
}
