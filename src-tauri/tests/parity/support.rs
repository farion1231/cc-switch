use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::json;

use cc_switch_lib::{
    update_settings, AppSettings, AppState, AppType, Database, McpServer, MultiAppConfig, Prompt,
    Provider,
};

#[path = "../support.rs"]
mod legacy_support;

pub use legacy_support::{ensure_test_home, reset_test_fs, test_mutex};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProviderSwitchSnapshot {
    pub app: String,
    pub result: serde_json::Value,
    pub current: Option<String>,
    pub providers: serde_json::Value,
    pub files: BTreeMap<String, String>,
}

pub fn create_legacy_state_with_config(config: &MultiAppConfig) -> AppState {
    let _ = update_settings(AppSettings::default());
    let _ = cc_switch_core::settings::update_settings(cc_switch_core::AppSettings::default());
    let db = std::sync::Arc::new(Database::init().expect("init legacy db"));
    db.migrate_from_json(config).expect("migrate legacy config");
    AppState::new(db)
}

pub fn create_core_state_with_config(config: &MultiAppConfig) -> cc_switch_core::AppState {
    let _ = update_settings(AppSettings::default());
    let _ = cc_switch_core::settings::update_settings(cc_switch_core::AppSettings::default());
    let state = cc_switch_core::AppState::new(
        cc_switch_core::Database::new().expect("init core db"),
    );

    for (app, manager) in &config.apps {
        for provider in manager.providers.values() {
            state
                .db
                .save_provider(app, &convert_provider(provider.clone()))
                .expect("save core provider");
        }

        if !manager.current.is_empty() {
            state
                .db
                .set_current_provider(app, &manager.current)
                .expect("set core current provider");

            let app_type = app.parse::<cc_switch_core::AppType>().expect("parse app type");
            if !app_type.is_additive_mode() {
                cc_switch_core::settings::set_current_provider(&app_type, Some(&manager.current))
                    .expect("set core effective current provider");
            }
        }
    }

    if let Some(servers) = &config.mcp.servers {
        for server in servers.values() {
            state
                .db
                .save_mcp_server(&convert_mcp_server(server.clone()))
                .expect("save core mcp");
        }
    }

    for app in AppType::all() {
        let prompts = match app {
            AppType::Claude => &config.prompts.claude.prompts,
            AppType::Codex => &config.prompts.codex.prompts,
            AppType::Gemini => &config.prompts.gemini.prompts,
            AppType::OpenCode => &config.prompts.opencode.prompts,
            AppType::OpenClaw => &config.prompts.openclaw.prompts,
        };

        for prompt in prompts.values() {
            state
                .db
                .save_prompt(app.as_str(), &convert_prompt(prompt.clone()))
                .expect("save core prompt");
        }
    }

    for (app, snippet) in [
        ("claude", config.common_config_snippets.claude.clone()),
        ("codex", config.common_config_snippets.codex.clone()),
        ("gemini", config.common_config_snippets.gemini.clone()),
        ("opencode", config.common_config_snippets.opencode.clone()),
        ("openclaw", config.common_config_snippets.openclaw.clone()),
    ] {
        state
            .db
            .set_config_snippet(app, snippet)
            .expect("save core snippet");
    }

    state
}

pub fn create_empty_legacy_state() -> AppState {
    create_legacy_state_with_config(&MultiAppConfig::default())
}

pub fn create_empty_core_state() -> cc_switch_core::AppState {
    create_core_state_with_config(&MultiAppConfig::default())
}

pub fn test_db_path() -> std::path::PathBuf {
    ensure_test_home().join(".cc-switch").join("cc-switch.db")
}

pub fn seed_usage_log(request_id: &str, created_at_secs: i64) {
    let db_path = test_db_path();
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent).expect("create db dir");
    }

    let conn = rusqlite::Connection::open(&db_path).expect("open parity db");
    conn.execute(
        "INSERT INTO proxy_request_logs (
            request_id, provider_id, app_type, model, request_model,
            input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
            input_cost_usd, output_cost_usd, cache_read_cost_usd, cache_creation_cost_usd,
            total_cost_usd, latency_ms, first_token_ms, duration_ms, status_code,
            error_message, session_id, provider_type, is_streaming, cost_multiplier, created_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24)",
        rusqlite::params![
            request_id,
            "provider-a",
            "claude",
            "claude-haiku-4-5-20251001",
            Option::<String>::None,
            120_i64,
            80_i64,
            10_i64,
            5_i64,
            "0.000100",
            "0.000200",
            "0.000010",
            "0.000005",
            "0.000315",
            321_i64,
            Option::<i64>::None,
            Option::<i64>::None,
            200_i64,
            Option::<String>::None,
            Option::<String>::None,
            Option::<String>::None,
            1_i64,
            "1.0",
            created_at_secs
        ],
    )
    .expect("insert usage log");
}

fn convert_provider(provider: Provider) -> cc_switch_core::Provider {
    serde_json::from_value(serde_json::to_value(provider).expect("provider to value"))
        .expect("provider convert")
}

fn convert_mcp_server(server: McpServer) -> cc_switch_core::McpServer {
    serde_json::from_value(serde_json::to_value(server).expect("mcp to value"))
        .expect("mcp convert")
}

fn convert_prompt(prompt: Prompt) -> cc_switch_core::Prompt {
    serde_json::from_value(serde_json::to_value(prompt).expect("prompt to value"))
        .expect("prompt convert")
}

pub fn provider_state_snapshot(
    root: &Path,
    app: &str,
    result: serde_json::Value,
    providers: serde_json::Value,
    current: Option<String>,
) -> ProviderSwitchSnapshot {
    let mut files = BTreeMap::new();
    for (key, path) in [
        ("claude/settings.json", root.join(".claude").join("settings.json")),
        ("claude/mcp.json", root.join(".claude.json")),
        ("codex/auth.json", root.join(".codex").join("auth.json")),
        ("codex/config.toml", root.join(".codex").join("config.toml")),
        ("gemini/.env", root.join(".gemini").join(".env")),
        (
            "opencode/opencode.json",
            root.join(".config").join("opencode").join("opencode.json"),
        ),
        ("openclaw/openclaw.json", root.join(".openclaw").join("openclaw.json")),
    ] {
        if let Ok(content) = std::fs::read_to_string(path) {
            files.insert(key.to_string(), content);
        }
    }

    ProviderSwitchSnapshot {
        app: app.to_string(),
        result,
        current,
        providers,
        files,
    }
}

pub fn codex_switch_config() -> MultiAppConfig {
    let mut config = MultiAppConfig::default();
    {
        let manager = config
            .get_manager_mut(&AppType::Codex)
            .expect("codex manager");
        manager.current = "old-provider".to_string();
        manager.providers.insert(
            "old-provider".to_string(),
            Provider::with_id(
                "old-provider".to_string(),
                "Legacy".to_string(),
                json!({
                    "auth": {"OPENAI_API_KEY": "stale"},
                    "config": "stale-config"
                }),
                None,
            ),
        );
        manager.providers.insert(
            "new-provider".to_string(),
            Provider::with_id(
                "new-provider".to_string(),
                "Latest".to_string(),
                json!({
                    "auth": {"OPENAI_API_KEY": "fresh-key"},
                    "config": r#"[mcp_servers.latest]
type = "stdio"
command = "say"
"#
                }),
                None,
            ),
        );
    }

    config.mcp.servers = Some(std::collections::HashMap::from([(
        "echo-server".into(),
        McpServer {
            id: "echo-server".to_string(),
            name: "Echo Server".to_string(),
            server: json!({
                "type": "stdio",
                "command": "echo"
            }),
            apps: cc_switch_lib::McpApps {
                claude: false,
                codex: true,
                gemini: false,
                opencode: false,
            },
            description: None,
            homepage: None,
            docs: None,
            tags: Vec::new(),
        },
    )]));

    config
}

pub fn claude_switch_config() -> MultiAppConfig {
    let mut config = MultiAppConfig::default();
    {
        let manager = config
            .get_manager_mut(&AppType::Claude)
            .expect("claude manager");
        manager.current = "old-provider".to_string();
        manager.providers.insert(
            "old-provider".to_string(),
            Provider::with_id(
                "old-provider".to_string(),
                "Legacy Claude".to_string(),
                json!({
                    "env": { "ANTHROPIC_API_KEY": "stale-key" }
                }),
                None,
            ),
        );
        manager.providers.insert(
            "new-provider".to_string(),
            Provider::with_id(
                "new-provider".to_string(),
                "Fresh Claude".to_string(),
                json!({
                    "env": { "ANTHROPIC_API_KEY": "fresh-key" },
                    "workspace": { "path": "/tmp/new-workspace" }
                }),
                None,
            ),
        );
    }

    config
}

pub fn seed_codex_live() {
    let legacy_auth = json!({"OPENAI_API_KEY": "legacy-key"});
    let legacy_config = r#"[mcp_servers.legacy]
type = "stdio"
command = "echo"
"#;
    cc_switch_lib::write_codex_live_atomic(&legacy_auth, Some(legacy_config))
        .expect("seed existing codex live config");
}

pub fn seed_claude_live() {
    let settings_path = cc_switch_lib::get_claude_settings_path();
    if let Some(parent) = settings_path.parent() {
        std::fs::create_dir_all(parent).expect("create claude settings dir");
    }
    let legacy_live = json!({
        "env": {
            "ANTHROPIC_API_KEY": "legacy-key"
        },
        "workspace": {
            "path": "/tmp/workspace"
        }
    });
    std::fs::write(
        &settings_path,
        serde_json::to_string_pretty(&legacy_live).expect("serialize legacy live"),
    )
    .expect("seed claude live config");
}

pub fn run_legacy_switch_case(
    config: &MultiAppConfig,
    app_type: AppType,
    provider_id: &str,
) -> Result<ProviderSwitchSnapshot, cc_switch_lib::AppError> {
    let state = create_legacy_state_with_config(config);
    let result =
        cc_switch_lib::provider_bridge::legacy_switch_provider(&state, app_type.clone(), provider_id)?;
    let providers =
        cc_switch_lib::provider_bridge::legacy_get_providers(&state, app_type.clone())?;
    let current =
        cc_switch_lib::provider_bridge::legacy_get_current_provider(&state, app_type.clone())?;
    let root = std::env::var("CC_SWITCH_TEST_HOME").expect("CC_SWITCH_TEST_HOME");
    Ok(provider_state_snapshot(
        Path::new(&root),
        app_type.as_str(),
        serde_json::to_value(result).expect("legacy switch result"),
        serde_json::to_value(providers).expect("legacy providers"),
        if current.is_empty() { None } else { Some(current) },
    ))
}

pub fn run_core_switch_case(
    config: &MultiAppConfig,
    app_type: AppType,
    provider_id: &str,
) -> Result<ProviderSwitchSnapshot, cc_switch_lib::AppError> {
    let _state = create_core_state_with_config(config);
    let result = cc_switch_lib::provider_bridge::switch_provider(app_type.clone(), provider_id)?;
    let providers = cc_switch_lib::provider_bridge::get_providers(app_type.clone())?;
    let current = cc_switch_lib::provider_bridge::get_current_provider(app_type.clone())?;
    let root = std::env::var("CC_SWITCH_TEST_HOME").expect("CC_SWITCH_TEST_HOME");
    Ok(provider_state_snapshot(
        Path::new(&root),
        app_type.as_str(),
        serde_json::to_value(result).expect("core switch result"),
        serde_json::to_value(providers).expect("core providers"),
        if current.is_empty() { None } else { Some(current) },
    ))
}
