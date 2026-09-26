//! 降级契约的旧版一侧。
//!
//! 重构版的退路是降级到上一个正式版，不迁移数据库。降级后旧版启动时看到的是：
//! live 处于代理模式（占位符是字面值 `PROXY_MANAGED`），DB 里没有接管备份行，
//! 供应商行都是真实配置。这里锁住旧版在这种状态下的行为：
//!
//! - 按字面值 `PROXY_MANAGED` 识别出接管态（重构版因此必须继续用这个字面值）；
//! - 没有备份行时，按当前供应商的行重建 live，不把占位符和本地地址写回去；
//! - 有备份行时会原样回放。所以重构版迁移时必须删掉残留的备份行，
//!   否则降级后旧版会把一份陈旧的 live 写回去。
//!
//! 旧版遇到更高的 schema 版本一律拒开，这一条由
//! `database::tests::schema_migration_rejects_future_version` 锁住，所以重构版不升 schema。
//!
//! 这组测试随接管备份机制一起退役：重构版发版时，改用上一个正式版的二进制做降级回放。

use serde_json::json;

use cc_switch_lib::{AppState, AppType};

use crate::support::{create_test_state, reset_test_fs, test_mutex};
use crate::util::{provider, read_home_file, read_home_json, seed_providers, write_home_file};

const PLACEHOLDER: &str = "PROXY_MANAGED";
const LOCAL_PROXY: &str = "127.0.0.1";

fn recover(state: &AppState) {
    futures::executor::block_on(state.proxy_service.recover_from_crash())
        .expect("recover from crash");
}

fn seed_taken_over_live() {
    write_home_file(
        ".claude/settings.json",
        r#"{"env":{"ANTHROPIC_BASE_URL":"http://127.0.0.1:15721","ANTHROPIC_AUTH_TOKEN":"PROXY_MANAGED"},"hooks":{}}"#,
    );
    write_home_file(
        ".codex/config.toml",
        "model_provider = \"custom\"\nmodel = \"gpt-5\"\n\n[model_providers.custom]\nname = \"custom\"\nbase_url = \"http://127.0.0.1:15721/v1\"\nwire_api = \"responses\"\nexperimental_bearer_token = \"PROXY_MANAGED\"\n",
    );
    write_home_file(
        ".gemini/.env",
        "GEMINI_API_KEY=PROXY_MANAGED\nGOOGLE_GEMINI_BASE_URL=http://127.0.0.1:15721\n",
    );
    write_home_file(".gemini/settings.json", "{}");
    write_home_file(
        ".grok/config.toml",
        "[models]\ndefault = \"grok-4.5\"\n\n[model.\"grok-4.5\"]\nmodel = \"grok-4.5\"\nbase_url = \"http://127.0.0.1:15721/grokbuild/v1\"\nname = \"Relay\"\napi_key = \"PROXY_MANAGED\"\napi_backend = \"responses\"\ncontext_window = 500000\n",
    );
}

fn seed_real_providers(state: &AppState) {
    seed_providers(
        state,
        &AppType::Claude,
        &[provider(
            "relay",
            json!({ "env": {
                "ANTHROPIC_BASE_URL": "https://relay.example",
                "ANTHROPIC_AUTH_TOKEN": "sk-claude"
            } }),
            None,
        )],
        "relay",
    );
    seed_providers(
        state,
        &AppType::Codex,
        &[provider(
            "relay",
            json!({
                "auth": { "OPENAI_API_KEY": "sk-codex" },
                "config": "model_provider = \"custom\"\nmodel = \"gpt-5\"\n\n[model_providers.custom]\nname = \"custom\"\nbase_url = \"https://relay.example/v1\"\nwire_api = \"responses\"\n"
            }),
            None,
        )],
        "relay",
    );
    seed_providers(
        state,
        &AppType::Gemini,
        &[provider(
            "relay",
            json!({
                "env": { "GEMINI_API_KEY": "g-relay", "GOOGLE_GEMINI_BASE_URL": "https://relay.example" },
                "config": {}
            }),
            None,
        )],
        "relay",
    );
    seed_providers(
        state,
        &AppType::GrokBuild,
        &[provider(
            "relay",
            json!({ "config": "[models]\ndefault = \"grok-4.5\"\n\n[model.\"grok-4.5\"]\nmodel = \"grok-4.5\"\nbase_url = \"https://relay.example/v1\"\nname = \"Relay\"\napi_key = \"xai-relay\"\napi_backend = \"responses\"\ncontext_window = 500000\n" }),
            None,
        )],
        "relay",
    );
}

/// 旧版按 `PROXY_MANAGED` 字面值识别四个可代理应用的接管态。
#[test]
fn old_version_detects_takeover_by_placeholder_literal() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    let state = create_test_state().expect("create test state");
    assert!(!state.proxy_service.detect_takeover_in_live_configs());

    let cases: [(&str, &str); 4] = [
        (
            ".claude/settings.json",
            r#"{"env":{"ANTHROPIC_BASE_URL":"http://127.0.0.1:15721","ANTHROPIC_AUTH_TOKEN":"PROXY_MANAGED"}}"#,
        ),
        (
            ".codex/config.toml",
            "model_provider = \"custom\"\n\n[model_providers.custom]\nname = \"custom\"\nbase_url = \"http://127.0.0.1:15721/v1\"\nexperimental_bearer_token = \"PROXY_MANAGED\"\n",
        ),
        (
            ".gemini/.env",
            "GEMINI_API_KEY=PROXY_MANAGED\nGOOGLE_GEMINI_BASE_URL=http://127.0.0.1:15721\n",
        ),
        (
            ".grok/config.toml",
            "[models]\ndefault = \"grok-4.5\"\n\n[model.\"grok-4.5\"]\nmodel = \"grok-4.5\"\nbase_url = \"http://127.0.0.1:15721/grokbuild/v1\"\nname = \"Relay\"\napi_key = \"PROXY_MANAGED\"\napi_backend = \"responses\"\ncontext_window = 500000\n",
        ),
    ];
    for (path, content) in cases {
        reset_test_fs();
        write_home_file(path, content);
        assert!(
            state.proxy_service.detect_takeover_in_live_configs(),
            "{path} with the placeholder must be detected as taken over"
        );
        // 本地地址本身不算接管，凭据是真实值时不能误判。
        write_home_file(path, &content.replace(PLACEHOLDER, "real-key"));
        assert!(
            !state.proxy_service.detect_takeover_in_live_configs(),
            "{path} with a real key must not be detected as taken over"
        );
    }
}

/// 没有备份行时，恢复按当前供应商的行重建 live：占位符和本地地址都不留。
#[test]
fn old_version_rebuilds_live_from_current_rows_without_backups() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    seed_taken_over_live();
    let state = create_test_state().expect("create test state");
    seed_real_providers(&state);
    assert!(state.proxy_service.detect_takeover_in_live_configs());

    recover(&state);

    let claude = read_home_json(".claude/settings.json");
    assert_eq!(claude["env"]["ANTHROPIC_AUTH_TOKEN"], json!("sk-claude"));
    assert_eq!(
        claude["env"]["ANTHROPIC_BASE_URL"],
        json!("https://relay.example")
    );

    let codex = read_home_file(".codex/config.toml");
    assert!(codex.contains("https://relay.example/v1"), "{codex}");
    assert!(codex.contains("sk-codex"), "{codex}");

    let gemini_env = read_home_file(".gemini/.env");
    assert!(
        gemini_env.contains("GEMINI_API_KEY=g-relay"),
        "{gemini_env}"
    );

    let grok = read_home_file(".grok/config.toml");
    assert!(grok.contains("xai-relay"), "{grok}");

    for path in [
        ".claude/settings.json",
        ".codex/config.toml",
        ".gemini/.env",
        ".grok/config.toml",
    ] {
        let text = read_home_file(path);
        assert!(
            !text.contains(PLACEHOLDER),
            "{path} kept the placeholder:\n{text}"
        );
        assert!(
            !text.contains(LOCAL_PROXY),
            "{path} kept the local proxy:\n{text}"
        );
    }
    assert!(!state.proxy_service.detect_takeover_in_live_configs());
}

/// 有备份行时，恢复会把备份原样写回 live，而不看当前供应商。
/// 这是重构版迁移必须删掉残留备份行的原因。
#[test]
fn old_version_replays_backup_rows_verbatim() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    seed_taken_over_live();
    let state = create_test_state().expect("create test state");
    seed_real_providers(&state);
    let stale = json!({ "env": {
        "ANTHROPIC_BASE_URL": "https://stale.example",
        "ANTHROPIC_AUTH_TOKEN": "sk-stale"
    } });
    futures::executor::block_on(
        state
            .db
            .save_live_backup(AppType::Claude.as_str(), &stale.to_string()),
    )
    .expect("save stale backup");

    recover(&state);

    let claude = read_home_json(".claude/settings.json");
    assert_eq!(
        claude["env"]["ANTHROPIC_AUTH_TOKEN"],
        json!("sk-stale"),
        "the old version replays a leftover backup row instead of the current provider"
    );
    assert!(!futures::executor::block_on(state.db.has_any_live_backup()).expect("query backups"));
}
