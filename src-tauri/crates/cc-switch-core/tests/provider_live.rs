use std::fs;
use std::path::Path;

use cc_switch_core::{HeadlessState, ProviderRecord, ProviderService, TargetPlatform};
use rusqlite::params;
use serde_json::{json, Value};

const CLI_APPS: &[&str] = &[
    "claude",
    "codex",
    "gemini",
    "grokbuild",
    "opencode",
    "openclaw",
    "hermes",
];

#[test]
fn cli_provider_switches_preserve_unrelated_live_configuration() {
    for app in CLI_APPS {
        let home = tempfile::tempdir().expect("创建临时 HOME");
        let state = HeadlessState::memory_with_platform(home.path(), TargetPlatform::Linux)
            .expect("创建 Linux 无界面状态");
        insert_provider(&state, app, &provider(app, "a"), true);
        insert_provider(&state, app, &provider(app, "b"), false);
        seed_unrelated_live(home.path(), app);

        let result = ProviderService::switch(&state, app, "b")
            .unwrap_or_else(|error| panic!("{app} live 投影失败: {error}"));

        assert!(result.warnings.is_empty(), "{app} 不应产生警告");
        assert_unrelated_live_survives(home.path(), app);
        assert_eq!(
            ProviderService::current(&state, app).expect("读取当前 Provider"),
            "b",
            "{app} 必须切换数据库当前项"
        );
    }
}

#[test]
fn claude_desktop_switch_is_rejected_before_database_change_on_linux() {
    let home = tempfile::tempdir().expect("创建临时 HOME");
    let state = HeadlessState::memory_with_platform(home.path(), TargetPlatform::Linux)
        .expect("创建 Linux 无界面状态");
    insert_provider(
        &state,
        "claude-desktop",
        &provider("claude-desktop", "desktop-a"),
        true,
    );
    insert_provider(
        &state,
        "claude-desktop",
        &provider("claude-desktop", "desktop-b"),
        false,
    );

    let error = ProviderService::switch(&state, "claude-desktop", "desktop-b")
        .expect_err("Linux 不能写 Claude Desktop live 配置");

    assert_eq!(error.code(), "CAPABILITY_UNAVAILABLE");
    assert_eq!(
        ProviderService::current(&state, "claude-desktop").expect("读取当前 Provider"),
        "desktop-a"
    );
}

#[test]
fn claude_desktop_can_be_added_and_edited_without_linux_live_projection() {
    let home = tempfile::tempdir().expect("创建临时 HOME");
    let state = HeadlessState::memory_with_platform(home.path(), TargetPlatform::Linux)
        .expect("创建 Linux 无界面状态");
    let original = provider("claude-desktop", "desktop-a");

    ProviderService::add(&state, "claude-desktop", original, false)
        .expect("Linux 应允许保存 Claude Desktop Provider");
    let mut updated = provider("claude-desktop", "desktop-a");
    updated.name = "Desktop Updated".to_string();
    ProviderService::update(&state, "claude-desktop", "desktop-a", updated)
        .expect("Linux 应允许编辑 Claude Desktop Provider");

    assert_eq!(
        ProviderService::list(&state, "claude-desktop").expect("读取 Provider")["desktop-a"].name,
        "Desktop Updated"
    );
}

#[test]
fn live_write_failure_keeps_committed_current_provider_for_reconciliation() {
    let home = tempfile::tempdir().expect("创建临时 HOME");
    let state = HeadlessState::memory_with_platform(home.path(), TargetPlatform::Linux)
        .expect("创建 Linux 无界面状态");
    insert_provider(&state, "claude", &provider("claude", "a"), true);
    insert_provider(&state, "claude", &provider("claude", "b"), false);
    fs::write(home.path().join(".claude"), b"block directory creation").expect("创建路径冲突");

    let error =
        ProviderService::switch(&state, "claude", "b").expect_err("live 写入失败不能报告成功");

    assert_eq!(error.code(), "LIVE_WRITE_FAILED");
    assert_eq!(
        ProviderService::current(&state, "claude").expect("读取已提交当前项"),
        "b"
    );
}

#[test]
fn explicit_live_path_override_does_not_write_default_home_path() {
    let home = tempfile::tempdir().expect("创建临时 HOME");
    let override_path = home.path().join("custom-claude").join("settings.json");
    let mut selected = provider("claude", "override");
    selected.meta = Some(json!({
        "_ccLive": {
            "paths": { "claudeSettings": override_path.to_string_lossy() }
        }
    }));
    let context = cc_switch_core::LiveContext {
        home: home.path(),
        platform: TargetPlatform::Linux,
    };

    cc_switch_core::project_provider(&context, "claude", &selected).expect("写入显式 Claude 路径");

    assert!(override_path.is_file());
    assert!(!home.path().join(".claude/settings.json").exists());
}

/// 直接构造规范行，避免测试准备阶段触发被测 live writer。
fn insert_provider(state: &HeadlessState, app: &str, provider: &ProviderRecord, is_current: bool) {
    state
        .with_connection(|connection| {
            connection.execute(
                "INSERT INTO providers (
                    id, app_type, name, settings_config, website_url, category,
                    created_at, sort_index, notes, icon, icon_color, meta,
                    is_current, in_failover_queue
                 ) VALUES (?1, ?2, ?3, ?4, NULL, ?5, 1, ?6, NULL, NULL, NULL, '{}', ?7, 0)",
                params![
                    provider.id,
                    app,
                    provider.name,
                    serde_json::to_string(&provider.settings_config)?,
                    provider.category,
                    if is_current { 0_i64 } else { 1_i64 },
                    is_current,
                ],
            )?;
            Ok(())
        })
        .expect("写入 Provider fixture");
}

/// 各应用 fixture 只包含切换所需最小合法字段，格式差异由 live writer 自身负责。
fn provider(app: &str, id: &str) -> ProviderRecord {
    let settings_config = match app {
        "claude" | "claude-desktop" => json!({
            "env": { "ANTHROPIC_AUTH_TOKEN": format!("sk-{id}") }
        }),
        "codex" => json!({
            "auth": { "OPENAI_API_KEY": format!("sk-{id}") },
            "config": format!("model = \"gpt-{id}\"\n")
        }),
        "gemini" => json!({
            "env": { "GEMINI_API_KEY": format!("sk-{id}") },
            "config": { "security": { "auth": { "selectedType": "api-key" } } }
        }),
        "grokbuild" => json!({
            "config": format!(
                "[models]\ndefault = \"grok-{id}\"\n\n[model.\"grok-{id}\"]\nmodel = \"grok-{id}\"\nbase_url = \"https://example.com/v1\"\nname = \"Grok {id}\"\napi_key = \"sk-{id}\"\napi_backend = \"responses\"\ncontext_window = 131072\n"
            )
        }),
        "opencode" => json!({
            "npm": "@ai-sdk/openai-compatible",
            "options": { "apiKey": format!("sk-{id}"), "baseURL": "https://example.com/v1" }
        }),
        "openclaw" => json!({
            "baseUrl": "https://example.com/v1",
            "apiKey": format!("sk-{id}"),
            "models": [{ "id": format!("model-{id}") }]
        }),
        "hermes" => json!({
            "base_url": "https://example.com/v1",
            "api_key": format!("sk-{id}"),
            "models": { format!("model-{id}"): { "context_length": 131072 } }
        }),
        _ => panic!("未覆盖应用 {app}"),
    };
    ProviderRecord {
        id: id.to_string(),
        name: format!("Provider {id}"),
        settings_config,
        website_url: None,
        category: (app == "grokbuild").then(|| "custom".to_string()),
        created_at: Some(1),
        sort_index: Some(0),
        notes: None,
        meta: None,
        icon: None,
        icon_color: None,
        in_failover_queue: false,
    }
}

fn seed_unrelated_live(home: &Path, app: &str) {
    match app {
        "claude" => write(
            home.join(".claude/settings.json"),
            r#"{"unrelated":"keep"}"#,
        ),
        "codex" => {
            write(
                home.join(".codex/auth.json"),
                r#"{"OPENAI_API_KEY":"old-provider-key"}"#,
            );
            write(home.join(".codex/config.toml"), "unrelated = \"keep\"\n");
        }
        "gemini" => {
            write(home.join(".gemini/.env"), "UNRELATED=keep\n");
            write(
                home.join(".gemini/settings.json"),
                r#"{"unrelated":"keep"}"#,
            );
        }
        "grokbuild" => write(home.join(".grok/config.toml"), "unrelated = \"keep\"\n"),
        "opencode" => write(
            home.join(".config/opencode/opencode.json"),
            r#"{"unrelated":"keep","provider":{"existing":{"npm":"existing"}}}"#,
        ),
        "openclaw" => write(
            home.join(".openclaw/openclaw.json"),
            r#"{"unrelated":"keep","models":{"mode":"merge","providers":{"existing":{"models":[]}}}}"#,
        ),
        "hermes" => write(
            home.join(".hermes/config.yaml"),
            "unrelated: keep\ncustom_providers:\n  - name: existing\n    api_key: old\n",
        ),
        _ => panic!("未覆盖应用 {app}"),
    }
}

fn assert_unrelated_live_survives(home: &Path, app: &str) {
    match app {
        "claude" => assert_eq!(
            read_json(&home.join(".claude/settings.json"))["unrelated"],
            "keep"
        ),
        "codex" => {
            assert_eq!(
                read_json(&home.join(".codex/auth.json"))["OPENAI_API_KEY"],
                "sk-b",
                "Codex auth 是 Provider 完整快照，不能保留旧账号字段"
            );
            assert!(read(&home.join(".codex/config.toml")).contains("unrelated = \"keep\""));
        }
        "gemini" => {
            assert!(read(&home.join(".gemini/.env")).contains("UNRELATED=keep"));
            assert_eq!(
                read_json(&home.join(".gemini/settings.json"))["unrelated"],
                "keep"
            );
        }
        "grokbuild" => {
            assert!(read(&home.join(".grok/config.toml")).contains("unrelated = \"keep\""))
        }
        "opencode" => {
            let live = read_json5(&home.join(".config/opencode/opencode.json"));
            assert_eq!(live["unrelated"], "keep");
            assert!(live["provider"]["existing"].is_object());
            assert!(live["provider"]["b"].is_object());
        }
        "openclaw" => {
            let live = read_json5(&home.join(".openclaw/openclaw.json"));
            assert_eq!(live["unrelated"], "keep");
            assert!(live["models"]["providers"]["existing"].is_object());
            assert!(live["models"]["providers"]["b"].is_object());
        }
        "hermes" => {
            let live: serde_yaml::Value =
                serde_yaml::from_str(&read(&home.join(".hermes/config.yaml")))
                    .expect("解析 Hermes YAML");
            assert_eq!(live["unrelated"].as_str(), Some("keep"));
            let providers = live["custom_providers"]
                .as_sequence()
                .expect("Hermes Provider 列表");
            assert!(providers
                .iter()
                .any(|item| item["name"].as_str() == Some("existing")));
            assert!(providers
                .iter()
                .any(|item| item["name"].as_str() == Some("b")));
        }
        _ => panic!("未覆盖应用 {app}"),
    }
}

fn write(path: impl AsRef<Path>, content: &str) {
    let path = path.as_ref();
    fs::create_dir_all(path.parent().expect("live 父目录")).expect("创建 live 目录");
    fs::write(path, content).expect("写入 live fixture");
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).expect("读取 live 文件")
}

fn read_json(path: &Path) -> Value {
    serde_json::from_str(&read(path)).expect("解析 live JSON")
}

fn read_json5(path: &Path) -> Value {
    json5::from_str(&read(path)).expect("解析 live JSON5")
}
