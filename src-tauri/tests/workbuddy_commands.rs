use cc_switch_lib::{
    AppType, McpServer, McpService, Prompt, PromptService, Provider, ProviderService,
};
use serde_json::{json, Value};
use std::fs;

#[path = "support.rs"]
mod support;
use support::{create_test_state, ensure_test_home, reset_test_fs, test_mutex};

fn provider(id: &str, model: &str) -> Provider {
    Provider::with_id(
        id.into(),
        "DeepSeek".into(),
        json!({
            "vendor": "DeepSeek",
            "url": "https://api.deepseek.com/v1/chat/completions",
            "apiKey": "test-key",
            "models": [{ "id": model, "name": "Chat", "supportsToolCall": true }]
        }),
        None,
    )
}

fn read_models() -> Value {
    let path = ensure_test_home().join(".workbuddy/models.json");
    serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}

#[test]
fn workbuddy_providers_are_written_to_models_json_and_removed_cleanly() {
    let _guard = test_mutex().lock().unwrap();
    reset_test_fs();
    let state = create_test_state().unwrap();
    let dir = ensure_test_home().join(".workbuddy");
    fs::create_dir_all(&dir).unwrap();
    // WorkBuddy's own entries (no CC Switch marker) and the allow-list must survive.
    fs::write(
        dir.join("models.json"),
        r#"{"models":[{"id":"mine","url":"https://x.test/v1/chat/completions"}],"availableModels":["mine"]}"#,
    )
    .unwrap();

    ProviderService::add(
        &state,
        AppType::WorkBuddy,
        provider("ds", "deepseek-chat"),
        true,
    )
    .unwrap();
    let doc = read_models();
    let models = doc["models"].as_array().unwrap();
    assert_eq!(models.len(), 2);
    assert_eq!(models[0]["id"], "mine");
    assert_eq!(models[1]["id"], "deepseek-chat");
    assert_eq!(
        models[1]["url"],
        "https://api.deepseek.com/v1/chat/completions"
    );
    assert_eq!(models[1]["apiKey"], "test-key");
    assert_eq!(models[1]["ccSwitchProviderId"], "ds");
    assert_eq!(doc["availableModels"], json!(["mine", "deepseek-chat"]));
    let saved = state
        .db
        .get_provider_by_id("ds", "workbuddy")
        .unwrap()
        .unwrap();
    assert_eq!(
        saved.meta.and_then(|meta| meta.live_config_managed),
        Some(true)
    );

    // Editing rewrites the provider's entries in place.
    let mut edited = provider("ds", "deepseek-reasoner");
    edited.meta = state
        .db
        .get_provider_by_id("ds", "workbuddy")
        .unwrap()
        .unwrap()
        .meta;
    ProviderService::update(&state, AppType::WorkBuddy, None, edited).unwrap();
    let doc = read_models();
    let ids: Vec<_> = doc["models"]
        .as_array()
        .unwrap()
        .iter()
        .map(|model| model["id"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(ids, vec!["mine", "deepseek-reasoner"]);
    assert_eq!(doc["availableModels"], json!(["mine", "deepseek-reasoner"]));

    ProviderService::remove_from_live_config(&state, AppType::WorkBuddy, "ds").unwrap();
    let doc = read_models();
    assert_eq!(doc["models"].as_array().unwrap().len(), 1);
    assert_eq!(doc["availableModels"], json!(["mine"]));
    assert!(state
        .db
        .get_provider_by_id("ds", "workbuddy")
        .unwrap()
        .is_some());

    ProviderService::switch(&state, AppType::WorkBuddy, "ds").unwrap();
    assert_eq!(read_models()["models"].as_array().unwrap().len(), 2);
    ProviderService::delete(&state, AppType::WorkBuddy, "ds").unwrap();
    assert_eq!(
        read_models()["models"],
        json!([{"id":"mine","url":"https://x.test/v1/chat/completions"}])
    );
    assert!(state
        .db
        .get_provider_by_id("ds", "workbuddy")
        .unwrap()
        .is_none());
}

fn official_card(id: &str, account_id: &str) -> Provider {
    let mut card = Provider::with_id(id.into(), "Alice".into(), json!({}), None);
    card.category = Some("official".into());
    card.meta = Some(
        serde_json::from_value(json!({
            "authBinding": {
                "source": "managed_account",
                "authProvider": "workbuddy_account",
                "accountId": account_id
            }
        }))
        .unwrap(),
    );
    card
}

#[test]
fn workbuddy_official_cards_never_touch_models_json() {
    let _guard = test_mutex().lock().unwrap();
    reset_test_fs();
    let state = create_test_state().unwrap();
    let path = ensure_test_home().join(".workbuddy/models.json");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    ProviderService::add(
        &state,
        AppType::WorkBuddy,
        provider("ds", "deepseek-chat"),
        true,
    )
    .unwrap();
    let before = fs::read_to_string(&path).unwrap();

    // 添加（即使要求写 live）只存绑定，标成不在 live
    ProviderService::add(
        &state,
        AppType::WorkBuddy,
        official_card("wb-official", "uid-1"),
        true,
    )
    .unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), before);
    let saved = state
        .db
        .get_provider_by_id("wb-official", "workbuddy")
        .unwrap()
        .unwrap();
    assert_eq!(
        saved
            .meta
            .as_ref()
            .and_then(|meta| meta.live_config_managed),
        Some(false)
    );
    assert_eq!(
        saved
            .meta
            .as_ref()
            .and_then(|meta| meta.managed_account_id_for("workbuddy_account")),
        Some("uid-1".to_string())
    );

    // 编辑：换绑账号，文件不动
    let mut edited = official_card("wb-official", "uid-2");
    edited.meta.as_mut().unwrap().live_config_managed = Some(true);
    ProviderService::update(&state, AppType::WorkBuddy, None, edited).unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), before);
    let saved = state
        .db
        .get_provider_by_id("wb-official", "workbuddy")
        .unwrap()
        .unwrap();
    assert_eq!(
        saved
            .meta
            .as_ref()
            .and_then(|meta| meta.live_config_managed),
        Some(false)
    );

    // 「添加到配置」被拒绝；全量同步、移除、删除都不碰文件
    assert!(ProviderService::switch(&state, AppType::WorkBuddy, "wb-official").is_err());
    ProviderService::sync_current_to_live(&state).unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), before);
    ProviderService::remove_from_live_config(&state, AppType::WorkBuddy, "wb-official").unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), before);
    ProviderService::delete(&state, AppType::WorkBuddy, "wb-official").unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), before);
    assert!(state
        .db
        .get_provider_by_id("wb-official", "workbuddy")
        .unwrap()
        .is_none());
    // 自定义模型卡不受影响
    assert!(state
        .db
        .get_provider_by_id("ds", "workbuddy")
        .unwrap()
        .is_some());
}

#[test]
fn workbuddy_official_card_without_live_flag_is_skipped_by_sync() {
    let _guard = test_mutex().lock().unwrap();
    reset_test_fs();
    let state = create_test_state().unwrap();
    let path = ensure_test_home().join(".workbuddy/models.json");
    // 旧数据 / 云同步来的行可能没有 live 标记、甚至带着模型配置：全量同步也不能把它写进 models.json
    let mut card = official_card("wb-official", "uid-1");
    card.settings_config = provider("x", "leftover-model").settings_config;
    card.meta.as_mut().unwrap().live_config_managed = None;
    state.db.save_provider("workbuddy", &card).unwrap();
    ProviderService::sync_current_to_live(&state).unwrap();
    assert!(
        !path.exists()
            || !fs::read_to_string(&path)
                .unwrap()
                .contains("leftover-model")
    );
}

#[test]
fn workbuddy_refuses_to_take_over_a_model_id_it_does_not_own() {
    let _guard = test_mutex().lock().unwrap();
    reset_test_fs();
    let state = create_test_state().unwrap();
    let path = ensure_test_home().join(".workbuddy/models.json");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let native = r#"{"models":[{"id":"deepseek-chat","apiKey":"user-key"}]}"#;
    fs::write(&path, native).unwrap();

    assert!(ProviderService::add(
        &state,
        AppType::WorkBuddy,
        provider("ds", "deepseek-chat"),
        true
    )
    .is_err());
    assert_eq!(fs::read_to_string(&path).unwrap(), native);
    assert!(state
        .db
        .get_provider_by_id("ds", "workbuddy")
        .unwrap()
        .is_none());

    // An edit that would collide is refused without changing the saved row either.
    ProviderService::add(
        &state,
        AppType::WorkBuddy,
        provider("ds", "deepseek-v4"),
        true,
    )
    .unwrap();
    let mut colliding = provider("ds", "deepseek-chat");
    colliding.meta = state
        .db
        .get_provider_by_id("ds", "workbuddy")
        .unwrap()
        .unwrap()
        .meta;
    assert!(ProviderService::update(&state, AppType::WorkBuddy, None, colliding).is_err());
    let saved = state
        .db
        .get_provider_by_id("ds", "workbuddy")
        .unwrap()
        .unwrap();
    assert_eq!(saved.settings_config["models"][0]["id"], "deepseek-v4");
}

#[test]
fn workbuddy_prompts_project_to_codebuddy_md() {
    let _guard = test_mutex().lock().unwrap();
    reset_test_fs();
    let state = create_test_state().unwrap();
    let prompt: Prompt = serde_json::from_value(json!({
        "id": "p", "name": "Rules", "content": "be brief", "enabled": true
    }))
    .unwrap();
    PromptService::upsert_prompt(&state, AppType::WorkBuddy, "p", prompt).unwrap();
    let path = ensure_test_home().join(".workbuddy/CODEBUDDY.md");
    assert_eq!(fs::read_to_string(path).unwrap(), "be brief");
}

fn read_mcp() -> Value {
    let path = ensure_test_home().join(".workbuddy/mcp.json");
    serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}

#[test]
fn workbuddy_mcp_projects_managed_servers_and_keeps_user_entries() {
    let _guard = test_mutex().lock().unwrap();
    reset_test_fs();
    let state = create_test_state().unwrap();
    let dir = ensure_test_home().join(".workbuddy");
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("mcp.json"),
        json!({
            "mcpServers": {
                "mine": {"command": "node", "args": ["mine.js"], "timeout": 5000},
                "paused": {"type": "sse", "url": "https://paused.example.com/sse", "disabled": true}
            },
            "disabledMcpServers": ["legacy"]
        })
        .to_string(),
    )
    .unwrap();

    let server: McpServer = serde_json::from_value(json!({
        "id": "fetch", "name": "Fetch",
        "server": {"type": "http", "url": "https://mcp.example.com/mcp", "headers": {"X-Key": "k"}},
        "apps": {"workbuddy": true}
    }))
    .unwrap();
    McpService::upsert_server(&state, server).unwrap();
    let doc = read_mcp();
    assert_eq!(doc["mcpServers"]["fetch"]["type"], "http");
    assert_eq!(
        doc["mcpServers"]["fetch"]["url"],
        "https://mcp.example.com/mcp"
    );
    assert_eq!(doc["mcpServers"]["mine"]["timeout"], 5000);
    assert_eq!(doc["disabledMcpServers"], json!(["legacy"]));

    // 全量重投影不碰用户自己加的服务器。
    McpService::sync_enabled_for_app(&state, &AppType::WorkBuddy).unwrap();
    assert_eq!(read_mcp()["mcpServers"]["mine"]["command"], "node");

    McpService::toggle_app(&state, "fetch", AppType::WorkBuddy, false).unwrap();
    assert!(read_mcp()["mcpServers"].get("fetch").is_none());
    assert!(
        !state.db.get_all_mcp_servers().unwrap()["fetch"]
            .apps
            .workbuddy
    );
    McpService::toggle_app(&state, "fetch", AppType::WorkBuddy, true).unwrap();
    assert!(read_mcp()["mcpServers"].get("fetch").is_some());

    // 导入：停用的条目进库但不勾选 WorkBuddy。
    McpService::import_from_all_apps(&state).unwrap();
    let servers = state.db.get_all_mcp_servers().unwrap();
    assert!(servers["mine"].apps.workbuddy);
    assert_eq!(servers["mine"].server["type"], "stdio");
    assert!(servers["mine"].server.get("timeout").is_none());
    assert!(!servers["paused"].apps.workbuddy);
    assert_eq!(servers["paused"].server["type"], "sse");

    assert!(McpService::delete_server(&state, "fetch").unwrap());
    let doc = read_mcp();
    assert!(doc["mcpServers"].get("fetch").is_none());
    assert!(doc["mcpServers"].get("mine").is_some());
    assert!(doc["mcpServers"].get("paused").is_some());
}
