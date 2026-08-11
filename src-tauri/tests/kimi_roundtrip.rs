mod support;

use cc_switch_lib::{kimi_config, update_settings, AppSettings};

/// 在临时 Kimi 目录上运行测试（通过 settings 的 kimi_config_dir 覆盖注入），
/// 结束后恢复设置并清理目录，即使测试失败也执行清理。
fn with_temp_kimi_dir<F: FnOnce(&std::path::Path)>(f: F) {
    let guard = support::test_mutex().lock().expect("test mutex poisoned");
    let home = support::ensure_test_home();
    support::reset_test_fs();

    let kimi_dir = home.join(".kimi-roundtrip");
    let _ = std::fs::remove_dir_all(&kimi_dir);
    std::fs::create_dir_all(&kimi_dir).expect("create temp kimi dir");

    update_settings(AppSettings {
        kimi_config_dir: Some(kimi_dir.to_string_lossy().into_owned()),
        ..AppSettings::default()
    })
    .expect("set kimi_config_dir override");

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(&kimi_dir)));

    // Always restore settings and drop fixture dir, even on test failure.
    let _ = update_settings(AppSettings::default());
    let _ = std::fs::remove_dir_all(&kimi_dir);
    drop(guard);

    if let Err(err) = result {
        std::panic::resume_unwind(err);
    }
}

fn sample_provider() -> serde_json::Value {
    serde_json::json!({
        "name": "kimi",
        "type": "openai",
        "base_url": "https://api.moonshot.cn/v1",
        "api_key": "sk-test-123",
        "models": [
            {
                "id": "kimi-k2.7-code",
                "name": "Kimi K2.7 Code",
                "max_context_size": 262144,
                "capabilities": ["thinking", "tool_use"]
            },
            {
                "id": "kimi-k3",
                "name": "Kimi K3",
                "max_context_size": 1048576,
                "capabilities": ["thinking", "always_thinking", "tool_use"]
            }
        ],
        "default_model": "kimi-k2.7-code"
    })
}

#[test]
fn set_and_get_provider_roundtrip_via_settings_override() {
    with_temp_kimi_dir(|dir| {
        kimi_config::set_provider("kimi", sample_provider()).expect("set_provider");

        let config_path = dir.join("config.toml");
        let content = std::fs::read_to_string(&config_path).expect("read config.toml");

        // 顶层 default_model 已写入
        assert!(
            content.contains("default_model = \"kimi-k2.7-code\""),
            "{content}"
        );
        // providers 表与 models 表
        assert!(content.contains("[providers.kimi]"), "{content}");
        assert!(content.contains("type = \"openai\""), "{content}");
        assert!(
            content.contains("base_url = \"https://api.moonshot.cn/v1\""),
            "{content}"
        );
        assert!(content.contains("api_key = \"sk-test-123\""), "{content}");
        assert!(content.contains("[models.\"kimi-k2.7-code\"]"), "{content}");
        // kimi-k3 是合法 bare key，toml_edit 不强制加引号
        assert!(
            content.contains("[models.kimi-k3]") || content.contains("[models.\"kimi-k3\"]"),
            "{content}"
        );
        assert!(content.contains("max_context_size = 262144"), "{content}");
        assert!(content.contains("max_context_size = 1048576"), "{content}");

        // 读回
        let providers = kimi_config::get_providers().expect("get_providers");
        let provider = providers.get("kimi").expect("provider exists");
        assert_eq!(provider["type"], "openai");
        assert_eq!(provider["default_model"], "kimi-k2.7-code");
        let models = provider["models"].as_array().expect("models array");
        assert_eq!(models.len(), 2);
        assert_eq!(models[0]["id"], "kimi-k2.7-code");
        assert_eq!(models[1]["max_context_size"], 1048576);

        assert_eq!(
            kimi_config::get_default_model().expect("default model"),
            Some("kimi-k2.7-code".to_string())
        );
    });
}

#[test]
fn set_provider_preserves_unrelated_toml_sections() {
    with_temp_kimi_dir(|dir| {
        let config_path = dir.join("config.toml");
        std::fs::write(
            &config_path,
            r#"# Kimi Code CLI configuration
# Provider: DeepSeek V4 Flash 0731 via New API gateway

default_model = "pre-existing"

[thinking]
enabled = true
effort = "high"
keep = "all"

[[hooks]]
event = "PreToolUse"
command = "echo hello"
timeout = 5
"#,
        )
        .expect("seed config.toml");

        kimi_config::set_provider("kimi", sample_provider()).expect("set_provider");

        let content = std::fs::read_to_string(&config_path).expect("read config.toml");
        assert!(content.contains("[thinking]"), "{content}");
        assert!(content.contains("effort = \"high\""), "{content}");
        assert!(content.contains("[[hooks]]"), "{content}");
        assert!(content.contains("command = \"echo hello\""), "{content}");
        assert!(content.contains("timeout = 5"), "{content}");
        assert!(
            content.contains("default_model = \"kimi-k2.7-code\""),
            "{content}"
        );
        // 文件头注释必须保留（挂在 default_model 键的前缀 decor 上）
        assert!(
            content.contains("Kimi Code CLI configuration"),
            "header comment lost after set_provider:\n{content}"
        );
        assert!(
            content.contains("via New API gateway"),
            "header comment lost after set_provider:\n{content}"
        );
    });
}

#[test]
fn remove_provider_rolls_back_default_model() {
    with_temp_kimi_dir(|dir| {
        kimi_config::set_provider("kimi", sample_provider()).expect("set kimi");

        let second = serde_json::json!({
            "name": "other",
            "type": "openai",
            "base_url": "https://example.com/v1",
            "api_key": "",
            "models": [{"id": "other-model", "name": "Other", "max_context_size": 65536}],
            "default_model": "other-model"
        });
        kimi_config::set_provider("other", second).expect("set other");
        kimi_config::apply_switch_defaults("kimi", &sample_provider()).expect("switch to kimi");

        assert_eq!(
            kimi_config::get_default_model().expect("default"),
            Some("kimi-k2.7-code".to_string())
        );

        // 删除 kimi → default_model 回退到 other-model
        kimi_config::remove_provider("kimi").expect("remove kimi");
        assert_eq!(
            kimi_config::get_default_model().expect("default"),
            Some("other-model".to_string())
        );

        let providers = kimi_config::get_providers().expect("providers");
        assert!(!providers.contains_key("kimi"));
        assert!(providers.contains_key("other"));

        let content = std::fs::read_to_string(dir.join("config.toml")).expect("read back");
        assert!(
            !content.contains("kimi-k2.7-code"),
            "model tables must be removed:\n{content}"
        );
    });
}

#[test]
fn mcp_json_roundtrip_preserves_unrelated_fields() {
    with_temp_kimi_dir(|dir| {
        std::fs::write(
            dir.join("mcp.json"),
            r#"{"mcpServers": {"filesystem": {"command": "npx", "args": ["-y", "@modelcontextprotocol/server-filesystem"]}}, "custom": 1}"#,
        )
        .expect("seed mcp.json");

        kimi_config::update_mcp_servers_json(|servers| {
            servers.insert(
                "fetch".to_string(),
                serde_json::json!({"command": "uvx", "args": ["mcp-server-fetch"]}),
            );
            Ok(())
        })
        .expect("update mcp");

        let root: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(dir.join("mcp.json")).expect("read mcp.json"),
        )
        .expect("parse mcp.json");
        assert_eq!(root["custom"], 1, "unrelated top-level fields preserved");
        assert!(root["mcpServers"]["filesystem"]["command"].is_string());
        assert!(root["mcpServers"]["fetch"]["command"].is_string());

        let servers = kimi_config::get_mcp_servers_json().expect("get mcp servers");
        assert!(servers.contains_key("filesystem"));
        assert!(servers.contains_key("fetch"));
    });
}

#[test]
fn validate_settings_rejects_bad_type_and_accepts_known_types() {
    assert!(kimi_config::validate_kimi_settings(&serde_json::json!({
        "name": "x", "type": "nonsense"
    }))
    .is_err());

    for ty in [
        "kimi",
        "anthropic",
        "openai",
        "openai_responses",
        "google-genai",
        "vertexai",
    ] {
        assert!(
            kimi_config::validate_kimi_settings(&serde_json::json!({
                "name": "x", "type": ty, "models": []
            }))
            .is_ok(),
            "type {ty} should be accepted"
        );
    }
}
