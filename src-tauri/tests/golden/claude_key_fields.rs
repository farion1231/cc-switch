//! ② Claude Code 关键字段的切换结果。
//!
//! 关键字段回答「请求发到哪、凭什么鉴权、哪个模型名、哪种协议」，由供应商完全拥有：
//! 切换后 live 里的关键字段必须恰好等于目标供应商行里的，上一家的一个都不能留。
//! 旧代码靠整份覆盖做到这一点，重构版改成「清空关键字段再写入」，结果必须一样。
//!
//! 关键字段的定义直接引用 `live::floor`，和切换用的是同一份。

use std::collections::BTreeMap;

use serde_json::{json, Value};

use cc_switch_lib::live::floor::{claude_floor_env, claude_floor_top};
use cc_switch_lib::{AppState, AppType, Provider, ProviderService};

use crate::support::{create_test_state, reset_test_fs, test_mutex};
use crate::util::{official, provider, read_home_json, seed_providers, write_home_file};

/// 取出关键字段：`env.<KEY>` 与顶层键，按名字排序。
fn floor_view(settings: &Value) -> BTreeMap<String, Value> {
    let mut view = BTreeMap::new();
    if let Some(env) = settings.get("env").and_then(Value::as_object) {
        for (key, value) in env {
            if claude_floor_env(key) {
                view.insert(format!("env.{key}"), value.clone());
            }
        }
    }
    if let Some(obj) = settings.as_object() {
        for (key, value) in obj {
            if claude_floor_top(key) {
                view.insert(key.clone(), value.clone());
            }
        }
    }
    view
}

const LIVE: &str = ".claude/settings.json";

fn setup(providers: &[Provider], current: &str) -> AppState {
    let state = create_test_state().expect("create test state");
    seed_providers(&state, &AppType::Claude, providers, current);
    let current_row = providers
        .iter()
        .find(|p| p.id == current)
        .expect("current provider");
    write_home_file(
        LIVE,
        &serde_json::to_string_pretty(&current_row.settings_config).expect("serialize live"),
    );
    state
}

/// 切到 `target`，断言 live 的关键字段恰好等于它行里的。
fn switch_and_check(state: &AppState, target: &str) {
    let row = state
        .db
        .get_provider_by_id(target, AppType::Claude.as_str())
        .expect("query provider")
        .expect("provider exists");
    ProviderService::switch(state, AppType::Claude, target).expect("switch provider");
    let live = read_home_json(LIVE);
    assert_eq!(
        floor_view(&live),
        floor_view(&row.settings_config),
        "after switching to {target}, live key fields must be exactly the target's\nlive: {live:#}"
    );
}

fn relay(id: &str, common_config: Option<bool>) -> Provider {
    provider(
        id,
        json!({
            "env": {
                "ANTHROPIC_BASE_URL": format!("https://{id}.example"),
                "ANTHROPIC_AUTH_TOKEN": format!("sk-{id}"),
                "ANTHROPIC_MODEL": format!("{id}-model"),
                "ANTHROPIC_DEFAULT_SONNET_MODEL": format!("{id}-sonnet"),
                "CLAUDE_CODE_SUBAGENT_MODEL": format!("{id}-subagent")
            }
        }),
        common_config,
    )
}

#[test]
fn third_party_to_third_party_replaces_every_key_field() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    let b = provider(
        "b",
        json!({ "env": {
            "ANTHROPIC_BASE_URL": "https://b.example",
            "ANTHROPIC_API_KEY": "sk-b"
        } }),
        None,
    );
    let state = setup(&[relay("a", None), b], "a");

    switch_and_check(&state, "b");
    switch_and_check(&state, "a");
}

/// `/model` 写在 live 顶层的 `model` 属于切走前的那家，不能带给下一家。
#[test]
fn model_picked_in_client_does_not_follow_the_switch() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    let state = setup(&[relay("a", None), relay("b", None)], "a");
    let mut live = read_home_json(LIVE);
    live["model"] = json!("a-picked-model");
    write_home_file(LIVE, &live.to_string());

    switch_and_check(&state, "b");
    assert!(read_home_json(LIVE).get("model").is_none());
}

#[test]
fn bedrock_to_official_leaves_no_provider_keys() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    let bedrock = provider(
        "bedrock",
        json!({
            "env": {
                "CLAUDE_CODE_USE_BEDROCK": "1",
                "AWS_REGION": "us-west-2",
                "AWS_BEARER_TOKEN_BEDROCK": "bedrock-key",
                "ANTHROPIC_MODEL": "us.anthropic.claude-sonnet-4-5-v1:0"
            },
            // 旧版 Bedrock API Key 预设写在顶层的 Key。
            "apiKey": "legacy-bedrock-key"
        }),
        None,
    );
    let state = setup(
        &[bedrock, official("claude-official", json!({ "env": {} }))],
        "bedrock",
    );

    switch_and_check(&state, "claude-official");
    let live = read_home_json(LIVE);
    assert!(floor_view(&live).is_empty(), "official live: {live:#}");

    // 按计划改变：旧 Bedrock API Key 行顶层的 `apiKey` 投影成
    // `env.AWS_BEARER_TOKEN_BEDROCK`（`env` 里已有就以它为准），不再写进 live 顶层。
    ProviderService::switch(&state, AppType::Claude, "bedrock").expect("switch back");
    let live = read_home_json(LIVE);
    assert_eq!(
        live["env"]["AWS_BEARER_TOKEN_BEDROCK"],
        json!("bedrock-key")
    );
    assert!(live.get("apiKey").is_none(), "live: {live:#}");
}

/// 只在顶层 `apiKey` 里存着 Key 的存量 Bedrock 行：切过去后 Key 出现在 Claude Code
/// 真正读取的 `AWS_BEARER_TOKEN_BEDROCK` 里，行本身不改写。
#[test]
fn legacy_bedrock_api_key_row_is_projected_to_the_bearer_env() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    let legacy = provider(
        "bedrock",
        json!({
            "env": { "CLAUDE_CODE_USE_BEDROCK": "1", "AWS_REGION": "us-west-2" },
            "apiKey": "legacy-bedrock-key"
        }),
        None,
    );
    let state = setup(&[relay("a", None), legacy.clone()], "a");

    ProviderService::switch(&state, AppType::Claude, "bedrock").expect("switch");
    let live = read_home_json(LIVE);
    assert_eq!(
        live["env"]["AWS_BEARER_TOKEN_BEDROCK"],
        json!("legacy-bedrock-key")
    );
    assert!(live.get("apiKey").is_none(), "live: {live:#}");
    let row = state
        .db
        .get_provider_by_id("bedrock", AppType::Claude.as_str())
        .expect("query")
        .expect("row");
    assert_eq!(row.settings_config, legacy.settings_config);
}

#[test]
fn vertex_to_third_party_leaves_no_vertex_keys() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    let vertex = provider(
        "vertex",
        json!({ "env": {
            "CLAUDE_CODE_USE_VERTEX": "1",
            "CLOUD_ML_REGION": "us-east5",
            "ANTHROPIC_VERTEX_PROJECT_ID": "my-project",
            "GOOGLE_APPLICATION_CREDENTIALS": "/Users/me/gcp.json",
            "VERTEX_REGION_CLAUDE_4_0_OPUS": "europe-west1"
        } }),
        None,
    );
    let state = setup(&[vertex, relay("b", None)], "vertex");

    switch_and_check(&state, "b");
    switch_and_check(&state, "vertex");
}

#[test]
fn api_key_helper_does_not_outlive_its_provider() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    let helper = provider(
        "helper",
        json!({
            "apiKeyHelper": "~/bin/print-key.sh",
            "env": {
                "ANTHROPIC_BASE_URL": "https://helper.example",
                "CLAUDE_CODE_API_KEY_HELPER_TTL_MS": "600000"
            }
        }),
        None,
    );
    let state = setup(&[helper, relay("b", None)], "helper");

    switch_and_check(&state, "b");
    switch_and_check(&state, "helper");
}

#[test]
fn official_roundtrip_through_third_party() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    let state = setup(
        &[
            official("claude-official", json!({ "env": {} })),
            relay("b", None),
        ],
        "claude-official",
    );

    switch_and_check(&state, "b");
    switch_and_check(&state, "claude-official");
    assert!(floor_view(&read_home_json(LIVE)).is_empty());
}

/// 两家都勾选通用配置时，片段只共享非关键字段，关键字段照样逐家替换；
/// live 里与供应商无关的功能开关（同在 `CLAUDE_CODE_USE_` 前缀下）来回切换后值不变。
#[test]
fn common_config_opt_in_keeps_key_fields_per_provider() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    let state = setup(&[relay("a", Some(true)), relay("b", Some(true))], "a");
    let mut live = read_home_json(LIVE);
    live["env"]["CLAUDE_CODE_USE_POWERSHELL_TOOL"] = json!("1");
    live["env"]["CLAUDE_CODE_USE_NATIVE_FILE_SEARCH"] = json!("1");
    live["includeCoAuthoredBy"] = json!(false);
    write_home_file(LIVE, &live.to_string());

    switch_and_check(&state, "b");
    switch_and_check(&state, "a");

    let live = read_home_json(LIVE);
    assert_eq!(live["env"]["CLAUDE_CODE_USE_POWERSHELL_TOOL"], json!("1"));
    assert_eq!(
        live["env"]["CLAUDE_CODE_USE_NATIVE_FILE_SEARCH"],
        json!("1")
    );
    assert_eq!(live["includeCoAuthoredBy"], json!(false));
}

/// 两家都勾选通用配置时，协议选择器、`AWS_*` 和顶层 `model` 不进共享片段，不会跟着切换
/// 带给下一家。旧版曾经会：从 Bedrock 切到官方后，官方的 live 里仍有
/// `CLAUDE_CODE_USE_BEDROCK=1`，Claude Code 继续走 Bedrock。
#[test]
fn common_config_opt_in_does_not_carry_key_fields_to_the_next_provider() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    let bedrock = provider(
        "bedrock",
        json!({ "env": {
            "CLAUDE_CODE_USE_BEDROCK": "1",
            "AWS_REGION": "us-west-2",
            "AWS_BEARER_TOKEN_BEDROCK": "bedrock-key",
            "ANTHROPIC_MODEL": "us.anthropic.claude-sonnet-4-5-v1:0"
        } }),
        Some(true),
    );
    let mut claude_official = official("claude-official", json!({ "env": {} }));
    claude_official.meta = Some(cc_switch_lib::ProviderMeta {
        common_config_enabled: Some(true),
        ..Default::default()
    });
    let state = setup(&[bedrock, claude_official], "bedrock");
    let mut live = read_home_json(LIVE);
    live["model"] = json!("picked-on-bedrock");
    write_home_file(LIVE, &live.to_string());

    switch_and_check(&state, "claude-official");
}
