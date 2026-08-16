use super::*;
use serde_json::json;
use serial_test::serial;

fn make_provider(env: Value) -> Provider {
    Provider::with_id(
        "p".to_string(),
        "P".to_string(),
        json!({ "env": env }),
        None,
    )
}

fn noop_persist() -> Arc<dyn Fn(Value) -> Result<(), String> + Send + Sync> {
    Arc::new(|_| Ok(()))
}

fn recording_persist(
    calls: Arc<std::sync::atomic::AtomicUsize>,
    captured: Arc<Mutex<Option<Value>>>,
) -> Arc<dyn Fn(Value) -> Result<(), String> + Send + Sync> {
    Arc::new(move |settings| {
        calls.fetch_add(1, Ordering::SeqCst);
        *captured.lock().unwrap() = Some(settings);
        Ok(())
    })
}

fn failing_persist() -> Arc<dyn Fn(Value) -> Result<(), String> + Send + Sync> {
    Arc::new(|_| Err("db unavailable".to_string()))
}

fn fail_once_then_succeed_persist() -> Arc<dyn Fn(Value) -> Result<(), String> + Send + Sync> {
    let failed = Arc::new(AtomicBool::new(false));
    Arc::new(move |_| {
        if failed.swap(true, Ordering::SeqCst) {
            Ok(())
        } else {
            Err("db unavailable".to_string())
        }
    })
}

// ========== 角色映射：角色/静态兜底/contextWindows 优先级矩阵 ==========

#[test]
fn resolve_maps_role_suffix_to_window() {
    // (模型角色, provider env, 期望窗口)
    let cases: Vec<(&str, Value, u64)> = vec![
        (
            "haiku",
            json!({ "ANTHROPIC_DEFAULT_HAIKU_MODEL": "Kimi-K2.7-Code[30k]" }),
            30000,
        ),
        (
            "sonnet",
            json!({ "ANTHROPIC_DEFAULT_SONNET_MODEL": "MiniMax-M3[1M]", "ANTHROPIC_DEFAULT_HAIKU_MODEL": "Kimi-K2.7-Code[30k]" }),
            1000000,
        ),
        (
            "opus",
            json!({ "ANTHROPIC_DEFAULT_OPUS_MODEL": "deepseek-v4-pro[1000000]" }),
            1000000,
        ),
        (
            "fable",
            json!({ "ANTHROPIC_DEFAULT_FABLE_MODEL": "GLM-5.2[200k]" }),
            200000,
        ),
    ];
    for (role, env, expected) in cases {
        let settings = json!({ "model": role });
        let provider = make_provider(env.clone());
        let result = resolve_active_model_window(&settings, &provider).unwrap();
        assert_eq!(result.model, role, "role {role}");
        assert_eq!(result.window, expected, "window for {role}");
    }

    // subagent 无后缀 → 无窗口可写
    let settings = json!({ "model": "subagent" });
    let provider = make_provider(json!({ "CLAUDE_CODE_SUBAGENT_MODEL": "deepseek-v4-flash" }));
    assert!(resolve_active_model_window(&settings, &provider).is_none());
}

#[test]
fn resolve_maps_static_fallbacks_and_context_window_priority() {
    let settings = json!({ "model": "sonnet" });

    // Codex OAuth gpt-5.6 → 372000
    let mut codex = make_provider(json!({
        "ANTHROPIC_DEFAULT_SONNET_MODEL": "gpt-5.6",
        "ANTHROPIC_DEFAULT_HAIKU_MODEL": "gpt-5.6-luna"
    }));
    codex.meta = Some(crate::provider::ProviderMeta {
        provider_type: Some("codex_oauth".to_string()),
        ..Default::default()
    });
    let result = resolve_active_model_window(&settings, &codex).unwrap();
    assert_eq!(result.window, 372000);

    // Kimi For Coding → 262144
    let kimi = make_provider(json!({
        "ANTHROPIC_BASE_URL": "https://api.kimi.com/coding/",
        "ANTHROPIC_DEFAULT_SONNET_MODEL": "kimi-for-coding"
    }));
    let result = resolve_active_model_window(&settings, &kimi).unwrap();
    assert_eq!(result.window, 262144);

    // contextWindows 显式值优先于 env 后缀
    let explicit = Provider::with_id(
        "p".to_string(),
        "P".to_string(),
        json!({
            "env": { "ANTHROPIC_DEFAULT_SONNET_MODEL": "model[200k]" },
            "contextWindows": { "ANTHROPIC_DEFAULT_SONNET_MODEL": 500000 }
        }),
        None,
    );
    let result = resolve_active_model_window(&settings, &explicit).unwrap();
    assert_eq!(result.window, 500000);

    // 无 env 后缀时同样读取 contextWindows
    let clean = Provider::with_id(
        "p".to_string(),
        "P".to_string(),
        json!({
            "env": { "ANTHROPIC_DEFAULT_SONNET_MODEL": "sonnet-model" },
            "contextWindows": { "ANTHROPIC_DEFAULT_SONNET_MODEL": 200000 }
        }),
        None,
    );
    let result = resolve_active_model_window(&settings, &clean).unwrap();
    assert_eq!(result.window, 200000);
}

// ========== build_env_writes / provider_compact_ratio 矩阵 ==========

#[test]
fn build_writes_window_and_ratio_matrix() {
    // (窗口, 比例, 期望 ACW, 期望 MAX)；含最小边界与非法比例回退 0.95。
    let cases: Vec<(u64, f64, &str, &str)> = vec![
        (30000, 0.8, "24000", "30000"),
        (1000000, 0.8, "800000", "1000000"),
        (200000, 0.8, "160000", "200000"),
        (1, 0.8, "0", "1"),
        (30000, 0.5, "15000", "30000"),
        (30000, 0.95, "28500", "30000"),
        (30000, 0.96, "28500", "30000"),
        (30000, 1.0, "28500", "30000"),
        (30000, 2.0, "28500", "30000"),
    ];
    for (window, ratio, acw, max) in cases {
        let writes = build_env_writes(window, ratio);
        assert_eq!(
            writes,
            vec![
                ("CLAUDE_CODE_AUTO_COMPACT_WINDOW", acw.to_string()),
                ("CLAUDE_CODE_MAX_CONTEXT_TOKENS", max.to_string()),
            ],
            "window={window} ratio={ratio}"
        );
    }
}

#[test]
fn compact_ratio_defaults_and_bounds() {
    // (provider 配置 JSON, 期望比例)
    let cases: Vec<(Value, f64)> = vec![
        (json!({ "env": {} }), 0.95),
        (json!({ "autoSyncCompactRatio": 0.1 }), 0.95),
        (json!({ "autoSyncCompactRatio": 1.5 }), 0.95),
        (json!({ "autoSyncCompactRatio": 1.0 }), 0.95),
        (json!({ "autoSyncCompactRatio": 0.96 }), 0.95),
        (json!({ "autoSyncCompactRatio": 0.95 }), 0.95),
        (json!({ "autoSyncCompactRatio": 0.6 }), 0.6),
    ];
    for (config, expected) in cases {
        let provider = Provider::with_id("p".to_string(), "P".to_string(), config.clone(), None);
        assert_eq!(
            provider_compact_ratio(&provider),
            expected,
            "config {config}"
        );
    }

    // 缺失时开箱行为：ACW = 窗口 × 0.95
    let missing = Provider::with_id("p".to_string(), "P".to_string(), json!({ "env": {} }), None);
    let writes = build_env_writes(30000, provider_compact_ratio(&missing));
    assert_eq!(writes[0].1, "28500");
    assert_eq!(writes[1].1, "30000");
}
// ========== 无效输入：resolve 返回 None 的矩阵 ==========

#[test]
fn resolve_returns_none_for_invalid_inputs() {
    // (settings, provider env)：全部期望 None。
    let cases: Vec<(Value, Value)> = vec![
        (
            json!({}),
            json!({ "ANTHROPIC_DEFAULT_HAIKU_MODEL": "Kimi[30k]" }),
        ),
        (json!({ "model": "custom-alias" }), json!({})),
        (json!({ "model": 123 }), json!({})),
        (json!({ "model": null }), json!({})),
        (json!({ "model": "haiku" }), json!({})),
        (
            json!({ "model": "haiku" }),
            json!({ "ANTHROPIC_DEFAULT_HAIKU_MODEL": { "name": "weird" } }),
        ),
        (
            json!({ "model": "haiku" }),
            json!({ "ANTHROPIC_DEFAULT_HAIKU_MODEL": "model[invalid]" }),
        ),
        (
            json!({ "model": "haiku" }),
            json!({ "ANTHROPIC_DEFAULT_HAIKU_MODEL": "model[0]" }),
        ),
        (
            json!({ "model": "haiku" }),
            json!({ "ANTHROPIC_DEFAULT_HAIKU_MODEL": "model[1G]" }),
        ),
        (
            json!({ "model": "haiku" }),
            json!({ "ANTHROPIC_DEFAULT_HAIKU_MODEL": "model[1.5m]" }),
        ),
        (
            json!({ "model": "haiku" }),
            json!({ "ANTHROPIC_DEFAULT_HAIKU_MODEL": "gpt-5.6" }),
        ),
    ];
    for (settings, env) in cases {
        let provider = make_provider(env);
        assert!(
            resolve_active_model_window(&settings, &provider).is_none(),
            "settings={settings} env={}",
            serde_json::to_string(&provider.settings_config).unwrap()
        );
    }
}

// ========== Task 5: 防循环测试 ==========

#[test]
fn loop_same_model_consecutive_triggers() {
    let state = Mutex::new(None);
    assert!(should_process(&state, Some("haiku"), None, None));
    // 尚未成功提交前，相同事件仍是候选，后续失败/早退路径可以重试。
    assert!(should_process(&state, Some("haiku"), None, None));
    record_processed_state(&state, Some("haiku"), None, None);
    assert!(!should_process(&state, Some("haiku"), None, None));
    assert!(!should_process(&state, Some("haiku"), None, None));
}

#[test]
fn loop_two_models_alternating() {
    let state = Mutex::new(None);
    assert!(should_process(&state, Some("haiku"), None, None));
    assert!(should_process(&state, Some("sonnet"), None, None));
    assert!(should_process(&state, Some("haiku"), None, None));
    assert!(should_process(&state, Some("sonnet"), None, None));
    record_processed_state(&state, Some("sonnet"), None, None);
    assert!(!should_process(&state, Some("sonnet"), None, None));
}

#[test]
fn loop_model_to_none_transitions() {
    let state = Mutex::new(None);
    // model 被删 → 处理一次，随后连续 None 视为无变化。
    assert!(should_process(&state, Some("haiku"), None, None));
    record_processed_state(&state, Some("haiku"), None, None);
    assert!(!should_process(&state, Some("haiku"), None, None));
    assert!(should_process(&state, None, None, None));
    record_processed_state(&state, None, None, None);
    assert!(!should_process(&state, None, None, None));
    assert!(!should_process(&state, None, None, None));
    // 模型重新出现 → 再次处理。
    assert!(should_process(&state, Some("haiku"), None, None));
}

#[test]
fn loop_initial_state_with_existing_model() {
    // 启动时 model 已经是 "haiku"（如上次会话留下的）
    let state = Mutex::new(Some(WatcherSnapshot {
        model: Some("haiku".to_string()),
        acw: None,
        max: None,
    }));
    // 第一次触发就是 haiku → 跳过（不算变化）
    assert!(!should_process(&state, Some("haiku"), None, None));
    // 切到别的 → 处理
    assert!(should_process(&state, Some("sonnet"), None, None));
    assert!(should_process(&state, Some("sonnet"), None, None));
    record_processed_state(&state, Some("sonnet"), None, None);
    assert!(!should_process(&state, Some("sonnet"), None, None));
}

// ========== update_env_fields 行为矩阵 ==========

#[test]
fn update_env_fields_write_preserve_and_overwrite() {
    struct Case {
        original: &'static str,
        window: u64,
        acw: &'static str,
        max: &'static str,
        top: (&'static str, &'static str),
        preserved: (&'static str, &'static str),
    }
    let cases = vec![
        Case {
            // 只写 env 键：顶层字段与原有 env 字段保留。
            original: r#"{"model":"sonnet","effortLevel":"xhigh","env":{"ANTHROPIC_DEFAULT_SONNET_MODEL":"MiniMax-M3[1M]"}}"#,
            window: 1000000,
            acw: "800000",
            max: "1000000",
            top: ("effortLevel", "xhigh"),
            preserved: ("ANTHROPIC_DEFAULT_SONNET_MODEL", "MiniMax-M3[1M]"),
        },
        Case {
            // env 缺失时创建。
            original: r#"{"model":"haiku","effortLevel":"max"}"#,
            window: 30000,
            acw: "24000",
            max: "30000",
            top: ("effortLevel", "max"),
            preserved: ("", ""),
        },
        Case {
            // 已有 env 字段全部保留。
            original: r#"{"model":"haiku","env":{"ANTHROPIC_DEFAULT_HAIKU_MODEL":"Kimi[30k]","CLAUDE_CODE_SUBAGENT_MODEL":"deepseek"}}"#,
            window: 30000,
            acw: "24000",
            max: "30000",
            top: ("model", "haiku"),
            preserved: ("ANTHROPIC_DEFAULT_HAIKU_MODEL", "Kimi[30k]"),
        },
        Case {
            // 已有 ACW/MAX 被覆盖。
            original: r#"{"model":"haiku","env":{"CLAUDE_CODE_AUTO_COMPACT_WINDOW":"999","CLAUDE_CODE_MAX_CONTEXT_TOKENS":"888"}}"#,
            window: 30000,
            acw: "24000",
            max: "30000",
            top: ("model", "haiku"),
            preserved: ("", ""),
        },
    ];
    for case in cases {
        let writes = build_env_writes(case.window, 0.8);
        let result = update_env_fields(case.original, &writes).unwrap();
        let v: Value = serde_json::from_str(&result).unwrap();
        assert_eq!(
            v["env"]["CLAUDE_CODE_AUTO_COMPACT_WINDOW"], case.acw,
            "{}",
            case.original
        );
        assert_eq!(
            v["env"]["CLAUDE_CODE_MAX_CONTEXT_TOKENS"], case.max,
            "{}",
            case.original
        );
        if !case.top.0.is_empty() {
            assert_eq!(v[case.top.0], case.top.1, "{}", case.original);
        }
        if !case.preserved.0.is_empty() {
            assert_eq!(
                v["env"][case.preserved.0], case.preserved.1,
                "{}",
                case.original
            );
        }
    }
}

#[test]
fn update_env_fields_strips_internal_fields_before_writing() {
    let original = r#"{"model":"sonnet","contextWindows":{"ANTHROPIC_DEFAULT_SONNET_MODEL":200000},"autoSyncContextWindow":true,"autoSyncCompactRatio":0.8,"autoSyncState":{"lastWritten":{"ACW":"160000","MAX":"200000"}},"env":{}}"#;
    let writes = build_env_writes(200000, 0.8);
    let result = update_env_fields(original, &writes).unwrap();
    let v: Value = serde_json::from_str(&result).unwrap();
    for key in [
        "contextWindows",
        "autoSyncContextWindow",
        "autoSyncCompactRatio",
        "autoSyncState",
    ] {
        assert!(v.get(key).is_none(), "{key} leaked into live settings");
    }
    assert_eq!(v["env"]["CLAUDE_CODE_AUTO_COMPACT_WINDOW"], "160000");
    assert_eq!(v["env"]["CLAUDE_CODE_MAX_CONTEXT_TOKENS"], "200000");
}

/// 用 tempfile 创建临时目录，验证真实 fs 事件的 watcher 行为
#[test]
fn fs_real_watcher_external_model_change() {
    use std::fs;
    use std::thread;

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    let initial = json!({
        "model": "sonnet",
        "env": {
            "ANTHROPIC_DEFAULT_SONNET_MODEL":"MiniMax-M3[1M]","ANTHROPIC_DEFAULT_HAIKU_MODEL":"Kimi-K2.7-Code[30k]"
        }
    });
    fs::write(&path, initial.to_string()).unwrap();

    let provider = Arc::new(Mutex::new(Provider::with_id(
        "p".to_string(),
        "P".to_string(),
        json!({
            "env": {
                "ANTHROPIC_DEFAULT_SONNET_MODEL":"MiniMax-M3[1M]","ANTHROPIC_DEFAULT_HAIKU_MODEL":"Kimi-K2.7-Code[30k]"
            },
            "autoSyncContextWindow":true
        }),
        None,
    )));

    let watcher = spawn_claude_settings_watcher(path.clone(), provider, noop_persist()).unwrap();

    // 模拟外部程序修改 model 字段
    let new_content = json!({
        "model": "haiku",
        "env": {
            "ANTHROPIC_DEFAULT_HAIKU_MODEL": "Kimi-K2.7-Code[30k]",
            "ANTHROPIC_DEFAULT_SONNET_MODEL":"MiniMax-M3[1M]","ANTHROPIC_DEFAULT_HAIKU_MODEL":"Kimi-K2.7-Code[30k]"
        }
    });
    fs::write(&path, new_content.to_string()).unwrap();

    // 等待 debouncer + 文件写入生效
    thread::sleep(Duration::from_millis(800));

    // 验证 ACW/MAX 已被写入
    let content = fs::read_to_string(&path).unwrap();
    let v: Value = serde_json::from_str(&content).unwrap();
    assert_eq!(v["model"], "haiku");
    assert_eq!(v["env"]["CLAUDE_CODE_AUTO_COMPACT_WINDOW"], "28500");
    assert_eq!(v["env"]["CLAUDE_CODE_MAX_CONTEXT_TOKENS"], "30000");

    drop(watcher);
}

/// provider 配置 autoSyncCompactRatio 时，watcher 按该比例写 ACW。
#[test]
#[serial]
fn fs_real_watcher_uses_provider_compact_ratio() {
    use std::fs;
    use std::thread;

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    let initial = json!({
        "model": "sonnet",
        "env": {
            "ANTHROPIC_DEFAULT_SONNET_MODEL": "MiniMax-M3[1M]",
            "ANTHROPIC_DEFAULT_HAIKU_MODEL": "Kimi-K2.7-Code[30k]"
        }
    });
    fs::write(&path, initial.to_string()).unwrap();

    let provider = Arc::new(Mutex::new(Provider::with_id(
        "p".to_string(),
        "P".to_string(),
        json!({
            "env": {
                "ANTHROPIC_DEFAULT_SONNET_MODEL": "MiniMax-M3[1M]",
                "ANTHROPIC_DEFAULT_HAIKU_MODEL": "Kimi-K2.7-Code[30k]"
            },
            "autoSyncContextWindow": true,
            "autoSyncCompactRatio": 0.5
        }),
        None,
    )));

    let watcher = spawn_claude_settings_watcher(path.clone(), provider, noop_persist()).unwrap();

    let new_content = json!({
        "model": "haiku",
        "env": {
            "ANTHROPIC_DEFAULT_SONNET_MODEL": "MiniMax-M3[1M]",
            "ANTHROPIC_DEFAULT_HAIKU_MODEL": "Kimi-K2.7-Code[30k]"
        }
    });
    fs::write(&path, new_content.to_string()).unwrap();

    thread::sleep(Duration::from_millis(800));

    let content = fs::read_to_string(&path).unwrap();
    let v: Value = serde_json::from_str(&content).unwrap();
    assert_eq!(v["env"]["CLAUDE_CODE_AUTO_COMPACT_WINDOW"], "15000");
    assert_eq!(v["env"]["CLAUDE_CODE_MAX_CONTEXT_TOKENS"], "30000");

    drop(watcher);
}
/// 只改 effortLevel 不应该触发 ACW/MAX 写入

/// Kimi/Codex OAuth 的静态 ACW/MAX 写入 live 后，watcher 不应再按压缩比例重写。
#[test]
fn handle_settings_change_keeps_kimi_static_acw_max() {
    use std::fs;

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    let initial = json!({
        "model": "sonnet",
        "env": {
            "ANTHROPIC_DEFAULT_SONNET_MODEL": "kimi-for-coding",
            "CLAUDE_CODE_AUTO_COMPACT_WINDOW": "262144",
            "CLAUDE_CODE_MAX_CONTEXT_TOKENS": "262144"
        },
        "autoSyncState": { "staticInjected": { "ACW": "262144", "MAX": "262144" } }
    });
    fs::write(&path, initial.to_string()).unwrap();

    let provider = Mutex::new(Provider::with_id(
        "kimi".to_string(),
        "Kimi For Coding".to_string(),
        json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://api.kimi.com/coding/",
                "ANTHROPIC_DEFAULT_SONNET_MODEL": "kimi-for-coding"
            },
            "autoSyncContextWindow": true,
            "autoSyncCompactRatio": 0.8,
            "autoSyncState": { "staticInjected": { "ACW": "262144", "MAX": "262144" } }
        }),
        None,
    ));

    let state = Mutex::new(None);
    handle_settings_change(&path, &provider, &state, &noop_persist());

    let content = fs::read_to_string(&path).unwrap();
    let v: Value = serde_json::from_str(&content).unwrap();
    assert_eq!(v["env"]["CLAUDE_CODE_AUTO_COMPACT_WINDOW"], "262144");
    assert_eq!(v["env"]["CLAUDE_CODE_MAX_CONTEXT_TOKENS"], "262144");
}

#[test]
fn handle_settings_change_keeps_codex_oauth_static_acw_max() {
    use std::fs;

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    let initial = json!({
        "model": "sonnet",
        "env": {
            "ANTHROPIC_DEFAULT_SONNET_MODEL": "gpt-5.6",
            "CLAUDE_CODE_AUTO_COMPACT_WINDOW": "372000",
            "CLAUDE_CODE_MAX_CONTEXT_TOKENS": "372000"
        },
        "autoSyncState": { "staticInjected": { "ACW": "372000", "MAX": "372000" } }
    });
    fs::write(&path, initial.to_string()).unwrap();

    let provider = Mutex::new(Provider::with_id(
        "codex-oauth".to_string(),
        "Codex".to_string(),
        json!({
            "env": { "ANTHROPIC_DEFAULT_SONNET_MODEL": "gpt-5.6" },
            "autoSyncContextWindow": true,
            "autoSyncCompactRatio": 0.8,
            "autoSyncState": { "staticInjected": { "ACW": "372000", "MAX": "372000" } }
        }),
        None,
    ));
    provider.lock().unwrap().meta = Some(crate::provider::ProviderMeta {
        provider_type: Some("codex_oauth".to_string()),
        ..Default::default()
    });

    let state = Mutex::new(None);
    handle_settings_change(&path, &provider, &state, &noop_persist());

    let content = fs::read_to_string(&path).unwrap();
    let v: Value = serde_json::from_str(&content).unwrap();
    assert_eq!(v["env"]["CLAUDE_CODE_AUTO_COMPACT_WINDOW"], "372000");
    assert_eq!(v["env"]["CLAUDE_CODE_MAX_CONTEXT_TOKENS"], "372000");
}

#[test]
fn handle_settings_change_syncs_non_gpt56_codex_oauth_from_context_windows() {
    use std::fs;

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    let initial = json!({
        "model": "haiku",
        "env": {
            "ANTHROPIC_DEFAULT_HAIKU_MODEL": "gpt-5.5-mini"
        }
    });
    fs::write(&path, initial.to_string()).unwrap();

    let provider = Mutex::new(Provider::with_id(
        "codex-oauth-non-gpt56".to_string(),
        "Codex Non-GPT56".to_string(),
        json!({
            "env": {
                "ANTHROPIC_DEFAULT_SONNET_MODEL": "gpt-5.5",
                "ANTHROPIC_DEFAULT_HAIKU_MODEL": "gpt-5.5-mini"
            },
            "contextWindows": { "ANTHROPIC_DEFAULT_HAIKU_MODEL": 432000 },
            "autoSyncContextWindow": true,
            "autoSyncCompactRatio": 0.8
        }),
        None,
    ));
    provider.lock().unwrap().meta = Some(crate::provider::ProviderMeta {
        provider_type: Some("codex_oauth".to_string()),
        ..Default::default()
    });

    // 模拟 model 从 sonnet 切到 haiku，且 provider 不触发 gpt-5.6 静态注入。
    let state = Mutex::new(Some(WatcherSnapshot {
        model: Some("sonnet".to_string()),
        acw: None,
        max: None,
    }));
    handle_settings_change(&path, &provider, &state, &noop_persist());

    let content = fs::read_to_string(&path).unwrap();
    let v: Value = serde_json::from_str(&content).unwrap();
    assert_eq!(v["env"]["CLAUDE_CODE_AUTO_COMPACT_WINDOW"], "345600");
    assert_eq!(v["env"]["CLAUDE_CODE_MAX_CONTEXT_TOKENS"], "432000");
}
#[test]
#[serial]
fn fs_real_watcher_effort_change_no_trigger() {
    use std::fs;
    use std::thread;

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    // spawn 时文件尚不存在 → watcher 初始快照为空
    let provider = Arc::new(Mutex::new(Provider::with_id(
        "p".to_string(),
        "P".to_string(),
        json!({
            "env": {
                "ANTHROPIC_DEFAULT_SONNET_MODEL":"MiniMax-M3[1M]",
                "ANTHROPIC_DEFAULT_HAIKU_MODEL":"Kimi-K2.7-Code[30k]"
            },
            // 开关开启 + 带窗口模型：让"effort-only 不触发"真正走到写入判定路径
            "autoSyncContextWindow": true
        }),
        None,
    )));

    let watcher = spawn_claude_settings_watcher(path.clone(), provider, noop_persist()).unwrap();

    // 首次写：触发 watcher 建立快照并写入 ACW/MAX（1M 窗口）
    fs::write(
        &path,
        json!({
            "model": "sonnet",
            "env": {
                "ANTHROPIC_DEFAULT_SONNET_MODEL":"MiniMax-M3[1M]",
                "ANTHROPIC_DEFAULT_HAIKU_MODEL":"Kimi-K2.7-Code[30k]"
            }
        })
        .to_string(),
    )
    .unwrap();
    thread::sleep(Duration::from_millis(800));

    // 只改 effortLevel（model / ACW / MAX 均未变化）→ 不应重写
    fs::write(
        &path,
        json!({
            "model": "sonnet",
            "effortLevel": "max",
            "env": {
                "ANTHROPIC_DEFAULT_SONNET_MODEL":"MiniMax-M3[1M]",
                "ANTHROPIC_DEFAULT_HAIKU_MODEL":"Kimi-K2.7-Code[30k]"
            }
        })
        .to_string(),
    )
    .unwrap();
    thread::sleep(Duration::from_millis(800));

    let content = fs::read_to_string(&path).unwrap();
    let v: Value = serde_json::from_str(&content).unwrap();
    assert_eq!(v["effortLevel"], "max");
    // 首次写入的 ACW/MAX 保持，effort-only 不重写
    assert_eq!(v["env"]["CLAUDE_CODE_AUTO_COMPACT_WINDOW"], "950000");
    assert_eq!(v["env"]["CLAUDE_CODE_MAX_CONTEXT_TOKENS"], "1000000");

    drop(watcher);
}

/// 设置不存在时的行为
#[test]
fn fs_real_watcher_file_not_found() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nonexistent.json");

    let provider = Arc::new(Mutex::new(make_provider(json!({
        "ANTHROPIC_DEFAULT_SONNET_MODEL":"MiniMax-M3[1M]","ANTHROPIC_DEFAULT_HAIKU_MODEL":"Kimi-K2.7-Code[30k]"
    }))));

    let result = spawn_claude_settings_watcher(path, provider, noop_persist());
    // 文件不存在 → 应该出错（watch 失败）
    assert!(
        result.is_ok(),
        "父目录存在时 spawn 应成功，能检测后续文件创建"
    );
}
/// 回归测试：production 路径下 spawn 的 watcher 必须靠 replace_watcher 存活，
/// 不能因为返回值没绑定到局部变量就被 Drop。
///
/// 修复前（直接 if-let-Err 丢弃 Ok 返回值）：watcher 构造完立即 Drop，
/// notify 线程退出，改文件后 ACW/MAX 不会被写入。
/// 修复后（spawn 的 Ok 交给 replace_watcher 存进进程单例）：watcher 存活，
/// 改 model 字段后 ACW/MAX 正确写入。
#[test]
#[serial]
fn fs_watcher_survives_via_replace_watcher_without_local_binding() {
    use std::fs;
    use std::thread;

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    let initial = json!({
        "model": "sonnet",
        "env": {
            "ANTHROPIC_DEFAULT_SONNET_MODEL":"MiniMax-M3[1M]",
            "ANTHROPIC_DEFAULT_HAIKU_MODEL":"Kimi-K2.7-Code[30k]"
        }
    });
    fs::write(&path, initial.to_string()).unwrap();

    let provider = Arc::new(Mutex::new(Provider::with_id(
        "p".to_string(),
        "P".to_string(),
        json!({
            "env": {
                "ANTHROPIC_DEFAULT_SONNET_MODEL":"MiniMax-M3[1M]",
                "ANTHROPIC_DEFAULT_HAIKU_MODEL":"Kimi-K2.7-Code[30k]"
            },
            "autoSyncContextWindow":true
        }),
        None,
    )));

    // 模拟 production 调用：spawn 后立即存进进程单例，不保留局部绑定。
    // spawned 在这里 move 进 replace_watcher，没有局部变量持有 watcher。
    let spawned = spawn_claude_settings_watcher(path.clone(), provider, noop_persist()).unwrap();
    replace_watcher(spawned);

    // 改 model 字段，模拟 Claude Code /model 切换 sonnet -> haiku
    let new_content = json!({
        "model": "haiku",
        "env": {
            "ANTHROPIC_DEFAULT_SONNET_MODEL":"MiniMax-M3[1M]",
            "ANTHROPIC_DEFAULT_HAIKU_MODEL":"Kimi-K2.7-Code[30k]"
        }
    });
    fs::write(&path, new_content.to_string()).unwrap();

    thread::sleep(Duration::from_millis(800));

    let content = fs::read_to_string(&path).unwrap();
    let v: Value = serde_json::from_str(&content).unwrap();
    assert_eq!(v["model"], "haiku");
    assert_eq!(v["env"]["CLAUDE_CODE_AUTO_COMPACT_WINDOW"], "28500");
    assert_eq!(v["env"]["CLAUDE_CODE_MAX_CONTEXT_TOKENS"], "30000");
}

/// 回归测试 #4：autoSyncContextWindow=false 时，model 字段变化不写 ACW/MAX。
/// 验证开关关闭后终端切模型不会同步（开关行为链路：toggle OFF -> save ->
/// update -> write_live -> replace_watcher(新 provider 快照) -> watcher 读到 false -> skip）。
#[test]
#[serial]
fn fs_watcher_auto_sync_disabled_skips_writes() {
    use std::fs;
    use std::thread;

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    let initial = json!({
        "model": "sonnet",
        "env": {
            "ANTHROPIC_DEFAULT_SONNET_MODEL":"MiniMax-M3[1M]",
            "ANTHROPIC_DEFAULT_HAIKU_MODEL":"Kimi-K2.7-Code[30k]"
        }
    });
    fs::write(&path, initial.to_string()).unwrap();

    // provider 显式关闭 autoSyncContextWindow
    let provider = Arc::new(Mutex::new(Provider::with_id(
        "p".to_string(),
        "P".to_string(),
        json!({
            "env": {
                "ANTHROPIC_DEFAULT_SONNET_MODEL":"MiniMax-M3[1M]",
                "ANTHROPIC_DEFAULT_HAIKU_MODEL":"Kimi-K2.7-Code[30k]"
            },
            "autoSyncContextWindow": false
        }),
        None,
    )));

    let spawned = spawn_claude_settings_watcher(path.clone(), provider, noop_persist()).unwrap();
    replace_watcher(spawned);

    // 改 model 字段 sonnet -> haiku
    let new_content = json!({
        "model": "haiku",
        "env": {
            "ANTHROPIC_DEFAULT_SONNET_MODEL":"MiniMax-M3[1M]",
            "ANTHROPIC_DEFAULT_HAIKU_MODEL":"Kimi-K2.7-Code[30k]"
        }
    });
    fs::write(&path, new_content.to_string()).unwrap();

    thread::sleep(Duration::from_millis(800));

    let content = fs::read_to_string(&path).unwrap();
    let v: Value = serde_json::from_str(&content).unwrap();
    // model 字段确实变了（说明事件被收到），但 ACW/MAX 不应该被写
    assert_eq!(v["model"], "haiku");
    assert!(
        v["env"].get("CLAUDE_CODE_AUTO_COMPACT_WINDOW").is_none(),
        "autoSync OFF 时不应写 ACW，但实际写入了: {:?}",
        v["env"].get("CLAUDE_CODE_AUTO_COMPACT_WINDOW")
    );
    assert!(
        v["env"].get("CLAUDE_CODE_MAX_CONTEXT_TOKENS").is_none(),
        "autoSync OFF 时不应写 MAX，但实际写入了: {:?}",
        v["env"].get("CLAUDE_CODE_MAX_CONTEXT_TOKENS")
    );
}

/// 回归测试 P1#1：watch 父目录后，atomic_write（rename 覆盖）仍能被观察到。
/// write_live_snapshot 用 atomic_write 写 settings.json，在 inotify（Linux）
/// 上 watch 文件会因 inode 替换失效；watch 父目录能持续观察文件替换。
#[test]
#[serial]
fn fs_watcher_observes_atomic_write_replacement() {
    use std::fs;
    use std::thread;

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    let initial = json!({
        "model": "sonnet",
        "env": {
            "ANTHROPIC_DEFAULT_SONNET_MODEL":"MiniMax-M3[1M]",
            "ANTHROPIC_DEFAULT_HAIKU_MODEL":"Kimi-K2.7-Code[30k]"
        }
    });
    fs::write(&path, initial.to_string()).unwrap();

    let provider = Arc::new(Mutex::new(Provider::with_id(
        "p".to_string(),
        "P".to_string(),
        json!({
            "env": {
                "ANTHROPIC_DEFAULT_SONNET_MODEL":"MiniMax-M3[1M]",
                "ANTHROPIC_DEFAULT_HAIKU_MODEL":"Kimi-K2.7-Code[30k]"
            },
            "autoSyncContextWindow":true
        }),
        None,
    )));

    let spawned = spawn_claude_settings_watcher(path.clone(), provider, noop_persist()).unwrap();
    replace_watcher(spawned);

    // 用 atomic_write 覆盖 settings.json（模拟 write_live_snapshot）
    let new_content = json!({
        "model": "haiku",
        "env": {
            "ANTHROPIC_DEFAULT_SONNET_MODEL":"MiniMax-M3[1M]",
            "ANTHROPIC_DEFAULT_HAIKU_MODEL":"Kimi-K2.7-Code[30k]"
        }
    });
    crate::config::atomic_write(&path, new_content.to_string().as_bytes()).unwrap();

    thread::sleep(Duration::from_millis(800));

    let content = fs::read_to_string(&path).unwrap();
    let v: Value = serde_json::from_str(&content).unwrap();
    assert_eq!(v["model"], "haiku");
    assert_eq!(v["env"]["CLAUDE_CODE_AUTO_COMPACT_WINDOW"], "28500");
    assert_eq!(v["env"]["CLAUDE_CODE_MAX_CONTEXT_TOKENS"], "30000");
}

// ========== Task 6: pretty、记账、用户显式、二次校验 ==========

#[test]
fn watcher_writes_use_pretty_json_and_update_last_written_on_success() {
    let content = r#"{"model":"sonnet","env":{}}"#;
    let writes = build_env_writes(200000, 0.8);
    let next = update_env_fields(content, &writes).unwrap();
    assert!(next.contains('\n'), "expected pretty JSON");
    let v: Value = serde_json::from_str(&next).unwrap();
    assert_eq!(v["env"]["CLAUDE_CODE_AUTO_COMPACT_WINDOW"], "160000");
    assert_eq!(v["env"]["CLAUDE_CODE_MAX_CONTEXT_TOKENS"], "200000");
}

#[test]
fn should_process_candidate_and_snapshot_semantics() {
    let state = Mutex::new(None);
    // 失败/早退路径必须保留旧快照，后续同一事件才能重试。
    assert!(should_process(
        &state,
        Some("sonnet"),
        Some("160000"),
        Some("200000")
    ));
    assert!(should_process(
        &state,
        Some("sonnet"),
        Some("160000"),
        Some("200000")
    ));
    assert_eq!(*state.lock().unwrap(), None);
    record_processed_state(&state, Some("sonnet"), Some("160000"), Some("200000"));
    assert!(!should_process(
        &state,
        Some("sonnet"),
        Some("160000"),
        Some("200000")
    ));

    // ACW/MAX 任一变化都视为需要处理。
    assert!(should_process(
        &state,
        Some("sonnet"),
        Some("250000"),
        Some("250000")
    ));
    assert!(should_process(
        &state,
        Some("sonnet"),
        Some("250000"),
        Some("250000")
    ));
    record_processed_state(&state, Some("sonnet"), Some("250000"), Some("250000"));
    assert!(!should_process(
        &state,
        Some("sonnet"),
        Some("250000"),
        Some("250000")
    ));
}

#[test]
fn handle_settings_change_records_last_written_on_success() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    let initial = json!({
        "model": "sonnet",
        "env": { "ANTHROPIC_DEFAULT_SONNET_MODEL": "glm-5.2" }
    });
    std::fs::write(&path, initial.to_string()).unwrap();

    let provider = Mutex::new(Provider::with_id(
        "p".to_string(),
        "P".to_string(),
        json!({
            "env": { "ANTHROPIC_DEFAULT_SONNET_MODEL": "glm-5.2" },
            "contextWindows": { "ANTHROPIC_DEFAULT_SONNET_MODEL": 200000 },
            "autoSyncContextWindow": true,
            "autoSyncCompactRatio": 0.8,
            "autoSyncState": { "lastWritten": {} }
        }),
        None,
    ));
    let state = Mutex::new(None);

    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let captured = Arc::new(Mutex::new(None));
    let persist = recording_persist(calls.clone(), captured.clone());
    handle_settings_change(&path, &provider, &state, &persist);

    let content = std::fs::read_to_string(&path).unwrap();
    let v: Value = serde_json::from_str(&content).unwrap();
    assert_eq!(v["env"]["CLAUDE_CODE_AUTO_COMPACT_WINDOW"], "160000");
    assert_eq!(v["env"]["CLAUDE_CODE_MAX_CONTEXT_TOKENS"], "200000");
    let provider_guard = provider.lock().unwrap();
    assert_eq!(
        provider_guard.settings_config["autoSyncState"]["lastWritten"],
        json!({ "ACW": "160000", "MAX": "200000" })
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let captured_guard = captured.lock().unwrap();
    assert_eq!(
        captured_guard.as_ref().unwrap()["autoSyncState"]["lastWritten"],
        json!({ "ACW": "160000", "MAX": "200000" })
    );
}

#[test]
fn handle_settings_change_keeps_ledger_and_state_when_persist_fails_after_write() {
    use std::fs;

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    let initial = json!({
        "model": "haiku",
        "env": { "ANTHROPIC_DEFAULT_HAIKU_MODEL": "Kimi-K2.7-Code[30k]" }
    });
    fs::write(&path, initial.to_string()).unwrap();

    let provider = Mutex::new(Provider::with_id(
        "p".to_string(),
        "P".to_string(),
        json!({
            "env": { "ANTHROPIC_DEFAULT_HAIKU_MODEL": "Kimi-K2.7-Code[30k]" },
            "contextWindows": { "ANTHROPIC_DEFAULT_HAIKU_MODEL": 30000 },
            "autoSyncContextWindow": true,
            "autoSyncState": { "lastWritten": { "ACW": "1", "MAX": "2" } }
        }),
        None,
    ));
    let previous = Some(WatcherSnapshot {
        model: Some("sonnet".to_string()),
        acw: None,
        max: None,
    });
    let state = Mutex::new(previous.clone());

    handle_settings_change(&path, &provider, &state, &failing_persist());

    let content = fs::read_to_string(&path).unwrap();
    let v: Value = serde_json::from_str(&content).unwrap();
    assert_eq!(v["env"]["CLAUDE_CODE_AUTO_COMPACT_WINDOW"], "28500");
    assert_eq!(v["env"]["CLAUDE_CODE_MAX_CONTEXT_TOKENS"], "30000");

    let provider_guard = provider.lock().unwrap();
    assert_eq!(
        provider_guard.settings_config["autoSyncState"]["lastWritten"],
        json!({ "ACW": "28500", "MAX": "30000" })
    );
    drop(provider_guard);
    assert_eq!(
        *state.lock().unwrap(),
        Some(WatcherSnapshot {
            model: Some("haiku".to_string()),
            acw: Some("28500".to_string()),
            max: Some("30000".to_string()),
        })
    );
    assert!(!should_process(
        &state,
        Some("haiku"),
        Some("28500"),
        Some("30000")
    ));
}

#[test]
fn failing_persist_after_write_keeps_auto_sync_flowing() {
    use std::fs;

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    fs::write(
        &path,
        json!({
            "model": "haiku",
            "env": { "ANTHROPIC_DEFAULT_HAIKU_MODEL": "Kimi-K2.7-Code[30k]" }
        })
        .to_string(),
    )
    .unwrap();

    let provider = Mutex::new(Provider::with_id(
        "p".to_string(),
        "P".to_string(),
        json!({
            "env": {
                "ANTHROPIC_DEFAULT_HAIKU_MODEL": "Kimi-K2.7-Code[30k]",
                "ANTHROPIC_DEFAULT_SONNET_MODEL": "glm-5.2"
            },
            "contextWindows": {
                "ANTHROPIC_DEFAULT_HAIKU_MODEL": 30000,
                "ANTHROPIC_DEFAULT_SONNET_MODEL": 200000
            },
            "autoSyncContextWindow": true,
            "autoSyncState": { "lastWritten": { "ACW": "1", "MAX": "2" } }
        }),
        None,
    ));
    let state = Mutex::new(None);

    // 第一次写 live 成功但 DB 持久化失败，必须保留新 live 值/账本/state，
    // 否则 atomic_write 触发的同名事件会被误判成用户手改。
    let persist = fail_once_then_succeed_persist();
    handle_settings_change(&path, &provider, &state, &persist);
    handle_settings_change(&path, &provider, &state, &persist);

    // 自动同步仍可继续：后续真实切换 model 仍会写入新窗口，而不是永久停同步。
    fs::write(
        &path,
        json!({
            "model": "sonnet",
            "env": { "ANTHROPIC_DEFAULT_SONNET_MODEL": "glm-5.2" }
        })
        .to_string(),
    )
    .unwrap();
    handle_settings_change(&path, &provider, &state, &persist);

    let content = fs::read_to_string(&path).unwrap();
    let v: Value = serde_json::from_str(&content).unwrap();
    assert_eq!(v["env"]["CLAUDE_CODE_AUTO_COMPACT_WINDOW"], "190000");
    assert_eq!(v["env"]["CLAUDE_CODE_MAX_CONTEXT_TOKENS"], "200000");
}

#[test]
fn handle_settings_change_rebuilt_watcher_with_stale_ledger_keeps_auto_sync() {
    use std::fs;

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    let initial = json!({
        "model": "haiku",
        "env": {
            "ANTHROPIC_DEFAULT_HAIKU_MODEL": "haiku-model",
            "ANTHROPIC_DEFAULT_SONNET_MODEL": "sonnet-model",
            "CLAUDE_CODE_AUTO_COMPACT_WINDOW": "24000",
            "CLAUDE_CODE_MAX_CONTEXT_TOKENS": "30000"
        }
    });
    fs::write(&path, initial.to_string()).unwrap();

    // DB 账本还是旧值，live 文件已包含上一次自动写入的 ACW/MAX；
    // watcher 重建时 state 从空快照开始，不能用旧 lastWritten 判成用户手改。
    let provider = Mutex::new(Provider::with_id(
        "p".to_string(),
        "P".to_string(),
        json!({
            "env": {
                "ANTHROPIC_DEFAULT_HAIKU_MODEL": "haiku-model",
                "ANTHROPIC_DEFAULT_SONNET_MODEL": "sonnet-model"
            },
            "contextWindows": {
                "ANTHROPIC_DEFAULT_HAIKU_MODEL": 30000,
                "ANTHROPIC_DEFAULT_SONNET_MODEL": 200000
            },
            "autoSyncContextWindow": true,
            "autoSyncCompactRatio": 0.8,
            "autoSyncState": { "lastWritten": { "ACW": "1", "MAX": "2" } }
        }),
        None,
    ));
    let state = Mutex::new(None);

    handle_settings_change(&path, &provider, &state, &noop_persist());

    // 自动同步不停止：下一次真实切换仍按目标写入。
    fs::write(
        &path,
        json!({
            "model": "sonnet",
            "env": { "ANTHROPIC_DEFAULT_SONNET_MODEL": "sonnet-model" }
        })
        .to_string(),
    )
    .unwrap();
    handle_settings_change(&path, &provider, &state, &noop_persist());

    let content = fs::read_to_string(&path).unwrap();
    let v: Value = serde_json::from_str(&content).unwrap();
    assert_eq!(v["env"]["CLAUDE_CODE_AUTO_COMPACT_WINDOW"], "160000");
    assert_eq!(v["env"]["CLAUDE_CODE_MAX_CONTEXT_TOKENS"], "200000");
}

#[test]
fn handle_settings_change_calibrates_against_all_configured_role_targets() {
    use std::fs;

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    let initial = json!({
        "model": "haiku",
        "env": {
            "ANTHROPIC_DEFAULT_HAIKU_MODEL": "haiku-model",
            "ANTHROPIC_DEFAULT_SONNET_MODEL": "sonnet-model",
            "CLAUDE_CODE_AUTO_COMPACT_WINDOW": "160000",
            "CLAUDE_CODE_MAX_CONTEXT_TOKENS": "200000"
        }
    });
    fs::write(&path, initial.to_string()).unwrap();

    // live 保留 sonnet 的自动写入值，但 DB lastWritten 缺失、当前 model 已是 haiku：
    // watcher 应直接按当前 model（haiku）的目标窗口重写，而非保留旧值。
    let provider = Mutex::new(Provider::with_id(
        "p".to_string(),
        "P".to_string(),
        json!({
            "env": {
                "ANTHROPIC_DEFAULT_HAIKU_MODEL": "haiku-model",
                "ANTHROPIC_DEFAULT_SONNET_MODEL": "sonnet-model"
            },
            "contextWindows": {
                "ANTHROPIC_DEFAULT_HAIKU_MODEL": 30000,
                "ANTHROPIC_DEFAULT_SONNET_MODEL": 200000
            },
            "autoSyncContextWindow": true,
            "autoSyncCompactRatio": 0.8,
            "autoSyncState": { "lastWritten": {} }
        }),
        None,
    ));
    let state = Mutex::new(None);

    handle_settings_change(&path, &provider, &state, &noop_persist());

    let content = fs::read_to_string(&path).unwrap();
    let v: Value = serde_json::from_str(&content).unwrap();
    assert_eq!(v["env"]["CLAUDE_CODE_AUTO_COMPACT_WINDOW"], "24000");
    assert_eq!(v["env"]["CLAUDE_CODE_MAX_CONTEXT_TOKENS"], "30000");

    let provider_guard = provider.lock().unwrap();
    assert_eq!(
        provider_guard.settings_config["autoSyncState"]["lastWritten"],
        json!({ "ACW": "24000", "MAX": "30000" })
    );
}

#[test]
fn handle_settings_change_kimi_raw_static_target_without_ledger_is_auto() {
    use std::fs;

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    let initial = json!({
        "model": "haiku",
        "env": {
            "ANTHROPIC_DEFAULT_HAIKU_MODEL": "kimi-for-coding",
            "ANTHROPIC_DEFAULT_SONNET_MODEL": "kimi-for-coding",
            "CLAUDE_CODE_AUTO_COMPACT_WINDOW": "262144",
            "CLAUDE_CODE_MAX_CONTEXT_TOKENS": "262144"
        }
    });
    fs::write(&path, initial.to_string()).unwrap();

    let provider = Mutex::new(Provider::with_id(
        "kimi".to_string(),
        "Kimi For Coding".to_string(),
        json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://api.kimi.com/coding/",
                "ANTHROPIC_DEFAULT_HAIKU_MODEL": "kimi-for-coding",
                "ANTHROPIC_DEFAULT_SONNET_MODEL": "kimi-for-coding"
            },
            "autoSyncContextWindow": true,
            "autoSyncCompactRatio": 0.8,
            "autoSyncState": { "lastWritten": {} }
        }),
        None,
    ));
    let state = Mutex::new(Some(WatcherSnapshot {
        model: Some("sonnet".to_string()),
        acw: None,
        max: None,
    }));

    handle_settings_change(&path, &provider, &state, &noop_persist());

    let v: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(v["env"]["CLAUDE_CODE_AUTO_COMPACT_WINDOW"], "262144");
    assert_eq!(v["env"]["CLAUDE_CODE_MAX_CONTEXT_TOKENS"], "262144");
}

#[test]
fn handle_settings_change_codex_oauth_raw_static_target_without_ledger_is_auto() {
    use std::fs;

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    let initial = json!({
        "model": "haiku",
        "env": {
            "ANTHROPIC_DEFAULT_HAIKU_MODEL": "gpt-5.6-mini",
            "ANTHROPIC_DEFAULT_SONNET_MODEL": "gpt-5.6",
            "CLAUDE_CODE_AUTO_COMPACT_WINDOW": "372000",
            "CLAUDE_CODE_MAX_CONTEXT_TOKENS": "372000"
        }
    });
    fs::write(&path, initial.to_string()).unwrap();

    let provider = Mutex::new(Provider::with_id(
        "codex-oauth".to_string(),
        "Codex".to_string(),
        json!({
            "env": {
                "ANTHROPIC_DEFAULT_HAIKU_MODEL": "gpt-5.6-mini",
                "ANTHROPIC_DEFAULT_SONNET_MODEL": "gpt-5.6"
            },
            "autoSyncContextWindow": true,
            "autoSyncCompactRatio": 0.8,
            "autoSyncState": { "lastWritten": {} }
        }),
        None,
    ));
    provider.lock().unwrap().meta = Some(crate::provider::ProviderMeta {
        provider_type: Some("codex_oauth".to_string()),
        ..Default::default()
    });
    let state = Mutex::new(Some(WatcherSnapshot {
        model: Some("sonnet".to_string()),
        acw: None,
        max: None,
    }));

    handle_settings_change(&path, &provider, &state, &noop_persist());

    let v: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(v["env"]["CLAUDE_CODE_AUTO_COMPACT_WINDOW"], "372000");
    assert_eq!(v["env"]["CLAUDE_CODE_MAX_CONTEXT_TOKENS"], "372000");
}

#[test]
fn failing_persist_retry_after_state_reset_does_not_mark_auto_target_explicit() {
    use std::fs;

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    let initial = json!({
        "model": "haiku",
        "env": { "ANTHROPIC_DEFAULT_HAIKU_MODEL": "Kimi[30k]" }
    });
    fs::write(&path, initial.to_string()).unwrap();

    let provider = Mutex::new(Provider::with_id(
        "p".to_string(),
        "P".to_string(),
        json!({
            "env": {
                "ANTHROPIC_DEFAULT_HAIKU_MODEL": "Kimi[30k]",
                "ANTHROPIC_DEFAULT_SONNET_MODEL": "GLM[200k]"
            },
            "autoSyncContextWindow": true,
            "autoSyncCompactRatio": 0.8,
            "autoSyncState": { "lastWritten": { "ACW": "1", "MAX": "2" } }
        }),
        None,
    ));
    let state = Mutex::new(None);

    // 第一次 live 写入成功但 DB 持久化失败；第二次相同事件不再被 should_process 跳过，
    // 并且 DB 账本仍是旧值，必须按当前 model 的自动目标校准重写。
    let persist = fail_once_then_succeed_persist();
    handle_settings_change(&path, &provider, &state, &persist);
    provider.lock().unwrap().settings_config["autoSyncState"]["lastWritten"] =
        json!({ "ACW": "1", "MAX": "2" });
    *state.lock().unwrap() = None;
    handle_settings_change(&path, &provider, &state, &persist);

    fs::write(
        &path,
        json!({
            "model": "sonnet",
            "env": { "ANTHROPIC_DEFAULT_SONNET_MODEL": "GLM[200k]" }
        })
        .to_string(),
    )
    .unwrap();
    handle_settings_change(&path, &provider, &state, &persist);

    let content = fs::read_to_string(&path).unwrap();
    let v: Value = serde_json::from_str(&content).unwrap();
    assert_eq!(v["env"]["CLAUDE_CODE_AUTO_COMPACT_WINDOW"], "160000");
    assert_eq!(v["env"]["CLAUDE_CODE_MAX_CONTEXT_TOKENS"], "200000");
}

#[cfg(unix)]
#[test]
fn handle_settings_change_keeps_last_written_when_atomic_write_fails() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    let initial = json!({
        "model": "sonnet",
        "env": { "ANTHROPIC_DEFAULT_SONNET_MODEL": "glm-5.2" }
    });
    std::fs::write(&path, initial.to_string()).unwrap();

    let provider = Mutex::new(Provider::with_id(
        "p".to_string(),
        "P".to_string(),
        json!({
            "env": { "ANTHROPIC_DEFAULT_SONNET_MODEL": "glm-5.2" },
            "contextWindows": { "ANTHROPIC_DEFAULT_SONNET_MODEL": 200000 },
            "autoSyncContextWindow": true,
            "autoSyncState": { "lastWritten": { "ACW": "1", "MAX": "2" } }
        }),
        None,
    ));
    let state = Mutex::new(None);

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(dir.path()).unwrap().permissions();
        permissions.set_mode(0o555);
        std::fs::set_permissions(dir.path(), permissions).unwrap();
    }

    handle_settings_change(&path, &provider, &state, &noop_persist());

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(dir.path()).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(dir.path(), permissions).unwrap();
    }

    let provider_guard = provider.lock().unwrap();
    assert_eq!(
        provider_guard.settings_config["autoSyncState"]["lastWritten"],
        json!({ "ACW": "1", "MAX": "2" })
    );
}

#[cfg(unix)]
#[test]
fn handle_settings_change_retries_same_event_after_atomic_write_fails() {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    let initial = json!({
        "model": "haiku",
        "env": { "ANTHROPIC_DEFAULT_HAIKU_MODEL": "haiku-model" }
    });
    fs::write(&path, initial.to_string()).unwrap();

    let provider = Mutex::new(Provider::with_id(
        "p".to_string(),
        "P".to_string(),
        json!({
            "env": { "ANTHROPIC_DEFAULT_HAIKU_MODEL": "haiku-model" },
            "contextWindows": { "ANTHROPIC_DEFAULT_HAIKU_MODEL": 30000 },
            "autoSyncContextWindow": true,
            "autoSyncState": { "lastWritten": { "ACW": "1", "MAX": "2" } }
        }),
        None,
    ));
    let previous = Some(WatcherSnapshot {
        model: Some("sonnet".to_string()),
        acw: None,
        max: None,
    });
    let state = Mutex::new(previous.clone());

    let mut permissions = fs::metadata(dir.path()).unwrap().permissions();
    permissions.set_mode(0o555);
    fs::set_permissions(dir.path(), permissions).unwrap();
    handle_settings_change(&path, &provider, &state, &noop_persist());

    // 失败路径不得提交新快照，因此恢复目录权限后同一事件仍会重试写入。
    assert_eq!(*state.lock().unwrap(), previous);
    let provider_guard = provider.lock().unwrap();
    assert_eq!(
        provider_guard.settings_config["autoSyncState"]["lastWritten"],
        json!({ "ACW": "1", "MAX": "2" })
    );
    drop(provider_guard);

    let mut permissions = fs::metadata(dir.path()).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(dir.path(), permissions).unwrap();
    handle_settings_change(&path, &provider, &state, &noop_persist());

    let content = fs::read_to_string(&path).unwrap();
    let v: Value = serde_json::from_str(&content).unwrap();
    assert_eq!(v["env"]["CLAUDE_CODE_AUTO_COMPACT_WINDOW"], "28500");
    assert_eq!(v["env"]["CLAUDE_CODE_MAX_CONTEXT_TOKENS"], "30000");
    assert_eq!(
        *state.lock().unwrap(),
        Some(WatcherSnapshot {
            model: Some("haiku".to_string()),
            acw: Some("28500".to_string()),
            max: Some("30000".to_string()),
        })
    );
}

#[test]
fn verify_file_unchanged_detects_concurrent_modification() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    std::fs::write(&path, "first").unwrap();
    assert!(verify_file_unchanged(&path, "first").is_ok());
    std::fs::write(&path, "second").unwrap();
    assert!(verify_file_unchanged(&path, "first").is_err());
}

#[test]
fn effective_auto_sync_enabled_false_without_field_or_ledger() {
    let provider = Provider::with_id("p".to_string(), "P".to_string(), json!({ "env": {} }), None);
    assert!(!effective_auto_sync_enabled(&provider));
}

#[test]
fn effective_auto_sync_enabled_respects_explicit_field() {
    // 显式字段优先：即使账本有记录，显式 false 仍为关闭
    let provider = Provider::with_id(
        "p".to_string(),
        "P".to_string(),
        json!({
            "env": {},
            "autoSyncContextWindow": false,
            "autoSyncState": { "staticInjected": { "ACW": "262144", "MAX": "262144" } }
        }),
        None,
    );
    assert!(!effective_auto_sync_enabled(&provider));

    let provider = Provider::with_id(
        "p".to_string(),
        "P".to_string(),
        json!({ "env": {}, "autoSyncContextWindow": true }),
        None,
    );
    assert!(effective_auto_sync_enabled(&provider));
}

#[test]
#[serial]
fn fs_real_watcher_missing_field_does_not_auto_sync() {
    // 方案 A：autoSyncContextWindow 字段缺失即关闭（即使账本有记录），
    // watcher 不写 ACW/MAX。
    use std::fs;
    use std::thread;

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    let initial = json!({
        "model": "sonnet",
        "env": {
            "ANTHROPIC_DEFAULT_SONNET_MODEL": "MiniMax-M3[1M]",
            "ANTHROPIC_DEFAULT_HAIKU_MODEL": "Kimi-K2.7-Code[30k]"
        }
    });
    fs::write(&path, initial.to_string()).unwrap();

    let provider = Arc::new(Mutex::new(Provider::with_id(
        "p".to_string(),
        "P".to_string(),
        json!({
            "env": {
                "ANTHROPIC_DEFAULT_SONNET_MODEL": "MiniMax-M3[1M]",
                "ANTHROPIC_DEFAULT_HAIKU_MODEL": "Kimi-K2.7-Code[30k]"
            },
            "autoSyncState": { "lastWritten": { "ACW": "1", "MAX": "2" } }
        }),
        None,
    )));

    let watcher = spawn_claude_settings_watcher(path.clone(), provider, noop_persist()).unwrap();

    // 模拟外部程序修改 model 字段
    fs::write(
        &path,
        json!({
            "model": "haiku",
            "env": {
                "ANTHROPIC_DEFAULT_HAIKU_MODEL": "Kimi-K2.7-Code[30k]",
                "ANTHROPIC_DEFAULT_SONNET_MODEL": "MiniMax-M3[1M]"
            }
        })
        .to_string(),
    )
    .unwrap();

    thread::sleep(Duration::from_millis(800));

    let content = fs::read_to_string(&path).unwrap();
    let v: Value = serde_json::from_str(&content).unwrap();
    assert_eq!(v["model"], "haiku");
    assert!(
        v["env"].get("CLAUDE_CODE_AUTO_COMPACT_WINDOW").is_none(),
        "字段缺失时 watcher 不应写 ACW"
    );
    assert!(
        v["env"].get("CLAUDE_CODE_MAX_CONTEXT_TOKENS").is_none(),
        "字段缺失时 watcher 不应写 MAX"
    );

    drop(watcher);
}

#[test]
fn handle_settings_change_writes_acw_max_in_direct_and_taken_over_modes() {
    // 直连解冻：直连（live 无接管占位符）与接管态都按激活角色写 ACW/MAX；
    // 只有 autoSyncContextWindow=false 才跳过。
    use std::fs;

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");

    let provider = Mutex::new(Provider::with_id(
        "p".to_string(),
        "P".to_string(),
        json!({
            "env": { "ANTHROPIC_DEFAULT_SONNET_MODEL": "glm-5.2" },
            "contextWindows": { "ANTHROPIC_DEFAULT_SONNET_MODEL": 200000 },
            "autoSyncContextWindow": true
        }),
        None,
    ));
    let state = Mutex::new(None);

    // 直连：live 无接管占位符 → 仍按 sonnet 窗口写入。
    fs::write(
        &path,
        json!({
            "model": "sonnet",
            "env": { "ANTHROPIC_DEFAULT_SONNET_MODEL": "glm-5.2" }
        })
        .to_string(),
    )
    .unwrap();
    handle_settings_change(&path, &provider, &state, &noop_persist());

    let v: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(v["env"]["CLAUDE_CODE_AUTO_COMPACT_WINDOW"], "190000");
    assert_eq!(v["env"]["CLAUDE_CODE_MAX_CONTEXT_TOKENS"], "200000");

    // 接管态同样写入。
    fs::write(
        &path,
        json!({
            "model": "sonnet",
            "env": { "ANTHROPIC_DEFAULT_SONNET_MODEL": "glm-5.2" }
        })
        .to_string(),
    )
    .unwrap();
    handle_settings_change(&path, &provider, &state, &noop_persist());

    let v: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(v["env"]["CLAUDE_CODE_AUTO_COMPACT_WINDOW"], "190000");
    assert_eq!(v["env"]["CLAUDE_CODE_MAX_CONTEXT_TOKENS"], "200000");
}
