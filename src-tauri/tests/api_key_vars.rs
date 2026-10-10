//! API Key 变量：引用解析、切换拦截、刷新传播、写回保护、导出排除。

use serde_json::json;

use cc_switch_lib::api_key_vars::{
    self,
    runner::{ApiKeyRefreshConfig, ApiKeyRefreshState, RefreshOutput, ScriptRun},
    ApiKeyVarInput,
};
use cc_switch_lib::{AppType, MultiAppConfig, Provider, ProviderService};

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
