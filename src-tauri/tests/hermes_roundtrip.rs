mod support;

use cc_switch_lib::{hermes_config, update_settings, AppSettings};

/// 读取并回写 Hermes provider 时，Hermes v12+ 新增或未来才会出现的字段
/// （例如 `rate_limit_delay`、`key_env`）必须透传，不能因为 UI 不感知就静默丢弃。
/// 否则用户在 Hermes Web UI 配置的高级字段会在 CC Switch 编辑后消失。
fn with_temp_hermes_dir<F: FnOnce(&std::path::Path)>(f: F) {
    let guard = support::test_mutex().lock().expect("test mutex poisoned");
    let home = support::ensure_test_home();
    support::reset_test_fs();

    let hermes_dir = home.join(".hermes-roundtrip");
    let _ = std::fs::remove_dir_all(&hermes_dir);
    std::fs::create_dir_all(&hermes_dir).expect("create temp hermes dir");

    update_settings(AppSettings {
        hermes_config_dir: Some(hermes_dir.to_string_lossy().into_owned()),
        ..AppSettings::default()
    })
    .expect("set hermes_config_dir override");

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(&hermes_dir)));

    // Always restore settings and drop fixture dir, even on test failure.
    let _ = update_settings(AppSettings::default());
    let _ = std::fs::remove_dir_all(&hermes_dir);
    drop(guard);

    if let Err(err) = result {
        std::panic::resume_unwind(err);
    }
}

#[test]
fn set_provider_preserves_unknown_and_future_fields() {
    with_temp_hermes_dir(|dir| {
        let yaml = r#"custom_providers:
  - name: myhost
    base_url: https://api.example.com/v1
    api_key: sk-old
    api_mode: chat_completions
    rate_limit_delay: 0.5
    key_env: MY_API_KEY
    foo_bar: keep-me-around
    models:
      gpt-4:
        context_length: 8192
"#;
        let config_path = dir.join("config.yaml");
        std::fs::write(&config_path, yaml).expect("seed config.yaml");

        // Simulate the UI sending back only the fields it knows about.
        let patch = serde_json::json!({
            "name": "myhost",
            "base_url": "https://api.example.com/v1",
            "api_key": "sk-new",
            "api_mode": "chat_completions",
            "models": [
                { "id": "gpt-4", "context_length": 8192 }
            ]
        });

        hermes_config::set_provider("myhost", patch).expect("set_provider");

        let written = std::fs::read_to_string(&config_path).expect("read written config");

        assert!(
            written.contains("rate_limit_delay"),
            "rate_limit_delay stripped:\n{written}"
        );
        assert!(
            written.contains("key_env"),
            "key_env key stripped:\n{written}"
        );
        assert!(
            written.contains("MY_API_KEY"),
            "key_env value stripped:\n{written}"
        );
        assert!(
            written.contains("foo_bar"),
            "unknown forward-compat field stripped:\n{written}"
        );
        assert!(
            written.contains("sk-new"),
            "api_key was not updated to sk-new:\n{written}"
        );
        assert!(
            !written.contains("sk-old"),
            "old api_key still present:\n{written}"
        );
    });
}

#[test]
fn get_providers_surfaces_rate_limit_delay_and_key_env() {
    with_temp_hermes_dir(|dir| {
        let yaml = r#"custom_providers:
  - name: myhost
    base_url: https://api.example.com/v1
    api_key: sk-xxx
    api_mode: chat_completions
    rate_limit_delay: 2.5
    key_env: FOO_KEY
    models:
      m1: {}
"#;
        std::fs::write(dir.join("config.yaml"), yaml).expect("seed config.yaml");

        let providers = hermes_config::get_providers().expect("get_providers");
        let entry = providers.get("myhost").expect("myhost missing");

        assert_eq!(
            entry.get("rate_limit_delay").and_then(|v| v.as_f64()),
            Some(2.5),
            "rate_limit_delay not surfaced to DAO payload"
        );
        assert_eq!(
            entry.get("key_env").and_then(|v| v.as_str()),
            Some("FOO_KEY"),
            "key_env not surfaced to DAO payload"
        );
    });
}
