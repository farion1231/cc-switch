//! DevEco Code integration tests.
//!
//! DevEco is an OpenCode fork with an additive-mode provider model: providers in
//! the native `deveco.jsonc` coexist, and CC Switch's database is a catalog
//! rather than the live source of truth. These tests cover the native config
//! probe order, the MCP round trip, and the "native write fails -> database
//! state is preserved and the operation can be retried" contract.

use cc_switch_lib::{
    AppType, McpApps, McpServer, McpService, Prompt, PromptService, ProviderService,
};
use serde_json::json;
use std::fs;

#[path = "support.rs"]
mod support;
use support::{create_test_state, ensure_test_home, reset_test_fs, test_mutex};

fn deveco_config_path() -> std::path::PathBuf {
    ensure_test_home()
        .join(".config")
        .join("deveco")
        .join("deveco.jsonc")
}

fn write_native_config(entries: serde_json::Value) {
    let path = deveco_config_path();
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, serde_json::to_string_pretty(&entries).unwrap()).unwrap();
}

fn read_native_config() -> serde_json::Value {
    let text = fs::read_to_string(deveco_config_path()).unwrap();
    json5::from_str(&text).unwrap()
}

#[test]
fn native_config_probe_prefers_jsonc_and_reports_live_provider_ids() {
    let _guard = test_mutex().lock().unwrap();
    reset_test_fs();
    let state = create_test_state().unwrap();

    // Seed the .json fallback first, then add a .jsonc file: DevEco loads the
    // first existing file in probe order, so jsonc must win.
    let dir = ensure_test_home().join(".config").join("deveco");
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("deveco.json"),
        json!({ "provider": { "shadowed": { "options": {} } } }).to_string(),
    )
    .unwrap();
    write_native_config(json!({
        "$schema": "https://opencode.ai/config.json",
        "provider": {
            "xai01": {
                "options": { "baseURL": "https://api.example.com/v1", "apiKey": "sk-test" },
                "models": { "glm-5.3-flash": { "limit": { "context": 128000 } } }
            }
        },
        "agent": { "ui_verification": { "model": "xai01/glm-5.3-flash" } }
    }));

    let ids = cc_switch_lib::get_deveco_live_provider_ids().unwrap();
    assert_eq!(ids, vec!["xai01".to_string()]);

    // Listing the app merges the native providers into the database catalog and
    // marks them as live-config managed.
    let providers = ProviderService::list(&state, AppType::DevEco).unwrap();
    let imported = providers
        .get("xai01")
        .expect("native provider merged into catalog");
    assert!(imported
        .meta
        .as_ref()
        .is_some_and(|meta| meta.live_config_managed == Some(true)));
    assert!(providers.get("shadowed").is_none());
}

#[test]
fn unreadable_native_config_surfaces_an_error_and_recovers_after_repair() {
    let _guard = test_mutex().lock().unwrap();
    reset_test_fs();

    // A directory where the config file belongs makes reads fail deterministically.
    let path = deveco_config_path();
    fs::create_dir_all(&path).unwrap();
    assert!(cc_switch_lib::get_deveco_live_provider_ids().is_err());

    // Repair and confirm the same call now succeeds.
    fs::remove_dir(&path).unwrap();
    write_native_config(json!({ "provider": { "xai01": { "options": {} } } }));
    assert_eq!(
        cc_switch_lib::get_deveco_live_provider_ids().unwrap(),
        vec!["xai01".to_string()]
    );
}

#[test]
fn failed_deveco_prompt_writes_preserve_state_and_can_be_retried() {
    let _guard = test_mutex().lock().unwrap();
    reset_test_fs();
    let state = create_test_state().unwrap();
    let path = ensure_test_home()
        .join(".config")
        .join("deveco")
        .join("AGENTS.md");
    let active: Prompt = serde_json::from_value(json!({
        "id":"active", "name":"Active", "content":"original", "enabled":true
    }))
    .unwrap();
    state.db.save_prompt("deveco", &active).unwrap();
    // A directory at the file path deterministically rejects the atomic rename.
    fs::create_dir_all(&path).unwrap();
    let mut edited = active.clone();
    edited.content = "updated".into();
    assert!(
        PromptService::upsert_prompt(&state, AppType::DevEco, &active.id, edited.clone()).is_err()
    );
    let stored = &state.db.get_prompts("deveco").unwrap()[&active.id];
    assert!(stored.enabled);
    assert_eq!(stored.content, "original");

    // Retry after repairing the path.
    fs::remove_dir(&path).unwrap();
    fs::write(&path, "original").unwrap();
    PromptService::upsert_prompt(&state, AppType::DevEco, &active.id, edited).unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), "updated");
    assert!(state.db.get_prompts("deveco").unwrap()[&active.id].enabled);
}

#[test]
fn dev_eco_mcp_server_round_trips_and_preserves_unrelated_config() {
    let _guard = test_mutex().lock().unwrap();
    reset_test_fs();
    write_native_config(json!({
        "$schema": "https://opencode.ai/config.json",
        "agent": { "ui_verification": { "model": "xai01/glm-5.3-flash" } }
    }));

    let state = create_test_state().unwrap();
    let server: McpServer = serde_json::from_value(json!({
        "id": "fetch",
        "name": "Fetch",
        "enabled": true,
        "apps": McpApps::default(),
        "server": { "type": "stdio", "command": "uvx", "args": ["mcp-server-fetch"] }
    }))
    .unwrap();
    McpService::upsert_server(&state, server).unwrap();
    McpService::toggle_app(&state, "fetch", AppType::DevEco, true).unwrap();

    let written = read_native_config();
    assert_eq!(written["mcp"]["fetch"]["type"], "local");
    assert_eq!(written["mcp"]["fetch"]["command"][0], "uvx");
    // Unrelated user configuration must survive the write.
    assert_eq!(
        written["agent"]["ui_verification"]["model"],
        "xai01/glm-5.3-flash"
    );

    McpService::toggle_app(&state, "fetch", AppType::DevEco, false).unwrap();
    let after = read_native_config();
    assert!(after["mcp"].get("fetch").is_none());
    assert_eq!(
        after["agent"]["ui_verification"]["model"],
        "xai01/glm-5.3-flash"
    );
}
