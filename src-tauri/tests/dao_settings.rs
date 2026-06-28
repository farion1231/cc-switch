use cc_switch_lib::{proxy::types::LogConfig, proxy::types::RectifierConfig, Database};

// ==================== get_setting / set_setting ====================

#[test]
fn test_get_setting_returns_none_for_missing_key() {
    let db = Database::memory().expect("create memory db");
    let result = db.get_setting("nonexistent_key").expect("get setting");
    assert!(result.is_none(), "missing key should return None");
}

#[test]
fn test_set_and_get_setting_round_trip() {
    let db = Database::memory().expect("create memory db");
    db.set_setting("my_key", "my_value").expect("set setting");
    let result = db.get_setting("my_key").expect("get setting");
    assert_eq!(result.as_deref(), Some("my_value"));
}

#[test]
fn test_set_setting_overwrites_existing_value() {
    let db = Database::memory().expect("create memory db");
    db.set_setting("key", "first").expect("set first");
    db.set_setting("key", "second").expect("set second");
    let result = db.get_setting("key").expect("get setting");
    assert_eq!(result.as_deref(), Some("second"));
}

#[test]
fn test_different_keys_are_independent() {
    let db = Database::memory().expect("create memory db");
    db.set_setting("key_a", "value_a").expect("set a");
    db.set_setting("key_b", "value_b").expect("set b");
    assert_eq!(
        db.get_setting("key_a").expect("get a").as_deref(),
        Some("value_a")
    );
    assert_eq!(
        db.get_setting("key_b").expect("get b").as_deref(),
        Some("value_b")
    );
}

// ==================== get_config_snippet / set_config_snippet ====================

#[test]
fn test_get_config_snippet_returns_none_initially() {
    let db = Database::memory().expect("create memory db");
    let result = db.get_config_snippet("claude").expect("get config snippet");
    assert!(result.is_none(), "no snippet should exist initially");
}

#[test]
fn test_set_and_get_config_snippet_round_trip() {
    let db = Database::memory().expect("create memory db");
    let snippet = r#"{"model":"claude-opus-4-5"}"#;
    db.set_config_snippet("claude", Some(snippet.to_string()))
        .expect("set snippet");
    let result = db.get_config_snippet("claude").expect("get config snippet");
    assert_eq!(result.as_deref(), Some(snippet));
}

#[test]
fn test_set_config_snippet_none_deletes_existing() {
    let db = Database::memory().expect("create memory db");
    db.set_config_snippet("claude", Some("some content".to_string()))
        .expect("set snippet");
    db.set_config_snippet("claude", None)
        .expect("delete snippet");
    let result = db.get_config_snippet("claude").expect("get config snippet");
    assert!(
        result.is_none(),
        "snippet should be deleted when set to None"
    );
}

#[test]
fn test_config_snippets_are_isolated_per_app_type() {
    let db = Database::memory().expect("create memory db");
    db.set_config_snippet("claude", Some("claude-snippet".to_string()))
        .expect("set claude");
    db.set_config_snippet("codex", Some("codex-snippet".to_string()))
        .expect("set codex");
    assert_eq!(
        db.get_config_snippet("claude")
            .expect("get claude")
            .as_deref(),
        Some("claude-snippet")
    );
    assert_eq!(
        db.get_config_snippet("codex")
            .expect("get codex")
            .as_deref(),
        Some("codex-snippet")
    );
}

// ==================== get_global_proxy_url / set_global_proxy_url ====================

#[test]
fn test_get_global_proxy_url_returns_none_initially() {
    let db = Database::memory().expect("create memory db");
    let url = db.get_global_proxy_url().expect("get proxy url");
    assert!(url.is_none(), "proxy url should be None initially");
}

#[test]
fn test_set_and_get_global_proxy_url_round_trip() {
    let db = Database::memory().expect("create memory db");
    db.set_global_proxy_url(Some("http://proxy.example.com:8080"))
        .expect("set proxy url");
    let url = db.get_global_proxy_url().expect("get proxy url");
    assert_eq!(url.as_deref(), Some("http://proxy.example.com:8080"));
}

#[test]
fn test_set_global_proxy_url_none_clears_url() {
    let db = Database::memory().expect("create memory db");
    db.set_global_proxy_url(Some("http://proxy.example.com:8080"))
        .expect("set proxy url");
    db.set_global_proxy_url(None).expect("clear proxy url");
    let url = db.get_global_proxy_url().expect("get proxy url");
    assert!(url.is_none(), "proxy url should be None after clearing");
}

#[test]
fn test_set_global_proxy_url_empty_string_clears_url() {
    let db = Database::memory().expect("create memory db");
    db.set_global_proxy_url(Some("http://proxy.example.com:8080"))
        .expect("set proxy url");
    db.set_global_proxy_url(Some(""))
        .expect("set empty proxy url");
    let url = db.get_global_proxy_url().expect("get proxy url");
    assert!(
        url.is_none(),
        "empty string should be treated as clearing the proxy"
    );
}

// ==================== get_rectifier_config / set_rectifier_config ====================

#[test]
fn test_get_rectifier_config_returns_default_when_unset() {
    let db = Database::memory().expect("create memory db");
    let config = db.get_rectifier_config().expect("get rectifier config");
    assert!(config.enabled, "enabled should default to true");
    assert!(
        config.request_thinking_signature,
        "request_thinking_signature should default to true"
    );
    assert!(
        config.request_thinking_budget,
        "request_thinking_budget should default to true"
    );
}

#[test]
fn test_set_and_get_rectifier_config_round_trip() {
    let db = Database::memory().expect("create memory db");
    let config = RectifierConfig {
        enabled: false,
        request_thinking_signature: true,
        request_thinking_budget: false,
        request_media_fallback: true,
        request_media_heuristic: true,
    };
    db.set_rectifier_config(&config)
        .expect("set rectifier config");
    let retrieved = db.get_rectifier_config().expect("get rectifier config");
    assert!(!retrieved.enabled);
    assert!(retrieved.request_thinking_signature);
    assert!(!retrieved.request_thinking_budget);
}

// ==================== get_log_config / set_log_config ====================

#[test]
fn test_get_log_config_returns_default_when_unset() {
    let db = Database::memory().expect("create memory db");
    let config = db.get_log_config().expect("get log config");
    assert!(config.enabled, "log enabled should default to true");
    assert_eq!(config.level, "info", "log level should default to info");
}

#[test]
fn test_set_and_get_log_config_round_trip() {
    let db = Database::memory().expect("create memory db");
    let config = LogConfig {
        enabled: false,
        level: "debug".to_string(),
    };
    db.set_log_config(&config).expect("set log config");
    let retrieved = db.get_log_config().expect("get log config");
    assert!(!retrieved.enabled);
    assert_eq!(retrieved.level, "debug");
}
