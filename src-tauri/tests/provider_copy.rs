//! 跨应用复制供应商（copy_provider_to_apps）集成测试：
//! 经 test hook 直调编排器，覆盖落库、live 写入、幂等跳过与源门禁。

use serde_json::{json, Value};

use cc_switch_lib::{
    copy_provider_to_apps_test_hook, AppType, CopyStatus, CopyTargetOutcome, MultiAppConfig,
    Provider, ProviderMeta,
};

#[path = "support.rs"]
mod support;
use support::{create_test_state_with_config, ensure_test_home, reset_test_fs, test_mutex};

fn claude_relay(id: &str) -> Provider {
    Provider::with_id(
        id.to_string(),
        "Relay".to_string(),
        json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://relay.example.com",
                "ANTHROPIC_AUTH_TOKEN": "sk-relay",
                "ANTHROPIC_MODEL": "claude-sonnet-4-5"
            }
        }),
        None,
    )
}

fn seed(config: &mut MultiAppConfig, provider: Provider) {
    let manager = config
        .get_manager_mut(&AppType::Claude)
        .expect("claude manager");
    manager.providers.insert(provider.id.clone(), provider);
}

fn meta_json(provider: &Provider) -> Value {
    serde_json::to_value(provider.meta.as_ref().expect("meta present")).expect("serialize meta")
}

fn outcome_of<'a>(outcomes: &'a [CopyTargetOutcome], target_app: &str) -> &'a CopyTargetOutcome {
    outcomes
        .iter()
        .find(|outcome| outcome.target_app == target_app)
        .unwrap_or_else(|| panic!("missing outcome for target '{target_app}'"))
}

#[test]
fn claude_to_codex_rebuilds_settings_and_leaves_source_live_untouched() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();

    let mut config = MultiAppConfig::default();
    seed(&mut config, claude_relay("relay-src"));
    let state = create_test_state_with_config(&config).expect("create test state");

    let outcomes = copy_provider_to_apps_test_hook(
        &state,
        AppType::Claude,
        "relay-src",
        &["claude".to_string(), "codex".to_string()],
    )
    .expect("copy succeeds");

    assert_eq!(outcomes.len(), 2);
    let same = &outcomes[0];
    assert_eq!(same.target_app, "claude");
    assert_eq!(same.status, CopyStatus::Skipped);
    assert_eq!(
        same.reason.as_ref().map(|reason| reason.key.as_str()),
        Some("sameAsSource")
    );

    let codex = &outcomes[1];
    assert_eq!(codex.target_app, "codex");
    assert_eq!(codex.status, CopyStatus::Copied);
    assert_eq!(codex.new_provider_id.as_deref(), Some("relay-src"));

    let row = state
        .db
        .get_provider_by_id("relay-src", "codex")
        .expect("query codex row")
        .expect("codex row exists");
    let config_toml = row
        .settings_config
        .get("config")
        .and_then(Value::as_str)
        .expect("codex config toml");
    assert!(config_toml.contains("wire_api = \"responses\""));
    assert!(config_toml.contains("base_url = \"https://relay.example.com\""));
    assert_eq!(
        row.settings_config
            .pointer("/auth/OPENAI_API_KEY")
            .and_then(Value::as_str),
        Some("sk-relay")
    );
    let meta = meta_json(&row);
    assert_eq!(meta["apiFormat"], "anthropic");
    assert!(meta.get("isFullUrl").is_none());

    // 切换式目标：目标 current 为空 → 副本成为 current 并写 live。
    let codex_live =
        std::fs::read_to_string(home.join(".codex").join("config.toml")).expect("codex live");
    assert!(codex_live.contains("https://relay.example.com"));
    assert_eq!(
        state
            .db
            .get_current_provider("codex")
            .expect("query codex current"),
        Some("relay-src".to_string())
    );

    // 源应用 live 文件未被触碰。
    assert!(!home.join(".claude").join("settings.json").exists());
}

#[test]
fn second_copy_skips_existing_target_and_preserves_row() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();

    let mut config = MultiAppConfig::default();
    seed(&mut config, claude_relay("relay-src"));
    let state = create_test_state_with_config(&config).expect("create test state");

    let first = copy_provider_to_apps_test_hook(
        &state,
        AppType::Claude,
        "relay-src",
        &["codex".to_string()],
    )
    .expect("first copy");
    assert_eq!(outcome_of(&first, "codex").status, CopyStatus::Copied);

    let second = copy_provider_to_apps_test_hook(
        &state,
        AppType::Claude,
        "relay-src",
        &["codex".to_string()],
    )
    .expect("second copy");
    let skipped = outcome_of(&second, "codex");
    assert_eq!(skipped.status, CopyStatus::Skipped);
    let reason = skipped.reason.as_ref().expect("skip reason");
    assert_eq!(reason.key, "alreadyExists");
    assert_eq!(reason.params.get("name").map(String::as_str), Some("Relay"));

    let row = state
        .db
        .get_provider_by_id("relay-src", "codex")
        .expect("query codex row")
        .expect("codex row preserved");
    assert_eq!(
        row.settings_config
            .pointer("/auth/OPENAI_API_KEY")
            .and_then(Value::as_str),
        Some("sk-relay")
    );
}

#[test]
fn desktop_copy_clones_source_and_second_copy_keeps_local_edits() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();

    let mut config = MultiAppConfig::default();
    let source = claude_relay("relay-src");
    let source_settings = source.settings_config.clone();
    seed(&mut config, source);
    let state = create_test_state_with_config(&config).expect("create test state");

    let outcomes = copy_provider_to_apps_test_hook(
        &state,
        AppType::Claude,
        "relay-src",
        &["claude-desktop".to_string()],
    )
    .expect("desktop copy");
    assert_eq!(
        outcome_of(&outcomes, "claude-desktop").status,
        CopyStatus::Copied
    );

    let row = state
        .db
        .get_provider_by_id("relay-src", "claude-desktop")
        .expect("query desktop row")
        .expect("desktop row exists");
    // P1：整份 clone settings_config，不经 builder 重建。
    assert_eq!(row.settings_config, source_settings);
    assert!(!row.in_failover_queue);
    let meta = meta_json(&row);
    assert_eq!(meta["claudeDesktopMode"], "direct");
    assert!(meta.get("claudeDesktopModelRoutes").is_none());

    // 模拟用户在 Desktop 端的本地改动后再次复制：跳过且不覆盖。
    let mut edited = row;
    edited.settings_config["env"]["ANTHROPIC_AUTH_TOKEN"] = json!("sk-local-edit");
    state
        .db
        .save_provider("claude-desktop", &edited)
        .expect("save local edit");

    let second = copy_provider_to_apps_test_hook(
        &state,
        AppType::Claude,
        "relay-src",
        &["claude-desktop".to_string()],
    )
    .expect("second desktop copy");
    assert_eq!(
        outcome_of(&second, "claude-desktop").status,
        CopyStatus::Skipped
    );

    let preserved = state
        .db
        .get_provider_by_id("relay-src", "claude-desktop")
        .expect("query desktop row")
        .expect("desktop row preserved");
    assert_eq!(
        preserved
            .settings_config
            .pointer("/env/ANTHROPIC_AUTH_TOKEN")
            .and_then(Value::as_str),
        Some("sk-local-edit")
    );
}

#[test]
fn desktop_copy_proxies_foreign_models_and_skips_incompatible_shapes() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();

    let mut config = MultiAppConfig::default();
    let mut kimi = claude_relay("kimi-relay");
    kimi.settings_config["env"]["ANTHROPIC_MODEL"] = json!("kimi-k2");
    seed(&mut config, kimi);

    let mut full_url = claude_relay("fullurl-relay");
    let _ = full_url.settings_config["env"]
        .as_object_mut()
        .expect("env object")
        .remove("ANTHROPIC_MODEL");
    full_url.meta = Some(ProviderMeta {
        is_full_url: Some(true),
        ..Default::default()
    });
    seed(&mut config, full_url);
    let state = create_test_state_with_config(&config).expect("create test state");

    // 非 Claude-safe 模型 → Proxy 模式，路由指回 kimi-k2。
    let kimi_outcomes = copy_provider_to_apps_test_hook(
        &state,
        AppType::Claude,
        "kimi-relay",
        &["claude-desktop".to_string()],
    )
    .expect("kimi copy");
    assert_eq!(
        outcome_of(&kimi_outcomes, "claude-desktop").status,
        CopyStatus::Copied
    );
    let kimi_row = state
        .db
        .get_provider_by_id("kimi-relay", "claude-desktop")
        .expect("query kimi desktop row")
        .expect("kimi desktop row");
    let kimi_meta = meta_json(&kimi_row);
    assert_eq!(kimi_meta["claudeDesktopMode"], "proxy");
    let routes = kimi_meta["claudeDesktopModelRoutes"]
        .as_object()
        .expect("routes object");
    assert!(routes
        .values()
        .any(|route| route["model"] == json!("kimi-k2")));

    // 整份 clone 路径下 Desktop 不接受 is_full_url 形状 → skip，不落行。
    let full_url_outcomes = copy_provider_to_apps_test_hook(
        &state,
        AppType::Claude,
        "fullurl-relay",
        &["claude-desktop".to_string()],
    )
    .expect("full-url copy");
    let skipped = outcome_of(&full_url_outcomes, "claude-desktop");
    assert_eq!(skipped.status, CopyStatus::Skipped);
    assert_eq!(
        skipped.reason.as_ref().map(|reason| reason.key.as_str()),
        Some("incompatibleClaudeDesktop")
    );
    assert!(state
        .db
        .get_provider_by_id("fullurl-relay", "claude-desktop")
        .expect("query full-url desktop row")
        .is_none());
}

#[test]
fn pi_target_skips_when_native_models_json_holds_same_key() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();

    let mut config = MultiAppConfig::default();
    seed(&mut config, claude_relay("pi-collide"));
    let state = create_test_state_with_config(&config).expect("create test state");

    let pi_dir = home.join(".pi").join("agent");
    std::fs::create_dir_all(&pi_dir).expect("create pi agent dir");
    std::fs::write(
        pi_dir.join("models.json"),
        json!({
            "providers": {
                "pi-collide": {
                    "name": "Native Pi",
                    "baseUrl": "https://pi.example.com",
                    "api": "openai-completions",
                    "apiKey": "sk-pi"
                }
            }
        })
        .to_string(),
    )
    .expect("write pi models.json");

    let outcomes =
        copy_provider_to_apps_test_hook(&state, AppType::Claude, "pi-collide", &["pi".to_string()])
            .expect("pi copy");
    let skipped = outcome_of(&outcomes, "pi");
    assert_eq!(skipped.status, CopyStatus::Skipped);
    assert_eq!(
        skipped.reason.as_ref().map(|reason| reason.key.as_str()),
        Some("alreadyExists")
    );
    assert!(state
        .db
        .get_provider_by_id("pi-collide", "pi")
        .expect("query pi row")
        .is_none());

    let _ = std::fs::remove_dir_all(home.join(".pi"));
}

#[test]
fn openclaw_copy_lands_in_database_only() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();

    let mut config = MultiAppConfig::default();
    seed(&mut config, claude_relay("relay-src"));
    let state = create_test_state_with_config(&config).expect("create test state");

    let outcomes = copy_provider_to_apps_test_hook(
        &state,
        AppType::Claude,
        "relay-src",
        &["openclaw".to_string()],
    )
    .expect("openclaw copy");
    assert_eq!(outcome_of(&outcomes, "openclaw").status, CopyStatus::Copied);

    let row = state
        .db
        .get_provider_by_id("relay-src", "openclaw")
        .expect("query openclaw row")
        .expect("openclaw row exists");
    assert_eq!(
        row.settings_config.get("api").and_then(Value::as_str),
        Some("anthropic-messages")
    );
    assert_eq!(
        row.settings_config.get("baseUrl").and_then(Value::as_str),
        Some("https://relay.example.com")
    );
    assert_eq!(
        row.settings_config.get("apiKey").and_then(Value::as_str),
        Some("sk-relay")
    );
    let meta = row.meta.as_ref().expect("meta present");
    assert_eq!(meta.api_format.as_deref(), Some("anthropic"));
    assert_eq!(meta.live_config_managed, Some(false));

    // add_to_live=false：additive 目标只落库，不写 live。
    assert!(!home.join(".openclaw").exists());
}

#[test]
fn source_gates_reject_and_unknown_targets_fail_per_target() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();

    let mut config = MultiAppConfig::default();
    seed(&mut config, claude_relay("relay-src"));

    let mut official = claude_relay("official-relay");
    official.category = Some("official".to_string());
    seed(&mut config, official);

    let mut keyless = claude_relay("keyless-relay");
    let _ = keyless.settings_config["env"]
        .as_object_mut()
        .expect("env object")
        .remove("ANTHROPIC_AUTH_TOKEN");
    seed(&mut config, keyless);

    let state = create_test_state_with_config(&config).expect("create test state");

    // 源不存在 → 整体 Err。
    assert!(copy_provider_to_apps_test_hook(
        &state,
        AppType::Claude,
        "missing",
        &["codex".to_string()],
    )
    .is_err());

    // 官方分类 → 整体 Err。
    assert!(copy_provider_to_apps_test_hook(
        &state,
        AppType::Claude,
        "official-relay",
        &["codex".to_string()],
    )
    .is_err());

    // 无凭据 → 整体 Err。
    assert!(copy_provider_to_apps_test_hook(
        &state,
        AppType::Claude,
        "keyless-relay",
        &["codex".to_string()],
    )
    .is_err());

    // 非法目标名 → 逐目标 Failed，不影响其余目标。
    let outcomes = copy_provider_to_apps_test_hook(
        &state,
        AppType::Claude,
        "relay-src",
        &["not-an-app".to_string()],
    )
    .expect("unknown target still returns outcomes");
    let failed = outcome_of(&outcomes, "not-an-app");
    assert_eq!(failed.status, CopyStatus::Failed);
    assert_eq!(
        failed.reason.as_ref().map(|reason| reason.key.as_str()),
        Some("unsupportedTarget")
    );
}
