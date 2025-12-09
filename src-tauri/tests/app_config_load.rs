use std::fs;
use std::path::PathBuf;

use cc_switch_lib::{AppError, MultiAppConfig};

mod support;
use support::{ensure_test_home, reset_test_fs, test_mutex};

fn cfg_path() -> PathBuf {
    let home = std::env::var("HOME").expect("HOME should be set by ensure_test_home");
    PathBuf::from(home).join(".cc-switch").join("config.json")
}

#[test]
fn load_v1_config_returns_error_and_does_not_write() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let path = cfg_path();
    fs::create_dir_all(path.parent().unwrap()).expect("create cfg dir");

    // 最小 v1 形状：providers + current，且不含 version/apps/mcp
    let v1_json = r#"{"providers":{},"current":""}"#;
    fs::write(&path, v1_json).expect("seed v1 json");
    let before = fs::read_to_string(&path).expect("read before");

    let err = MultiAppConfig::load().expect_err("v1 should not be auto-migrated");
    match err {
        AppError::Localized { key, .. } => assert_eq!(key, "config.unsupported_v1"),
        other => panic!("expected Localized v1 error, got {other:?}"),
    }

    // 文件不应有任何变化，且不应生成 .bak
    let after = fs::read_to_string(&path).expect("read after");
    assert_eq!(before, after, "config.json should not be modified");
    let bak = home.join(".cc-switch").join("config.json.bak");
    assert!(!bak.exists(), ".bak should not be created on load error");
}

#[test]
fn load_v1_with_extra_version_still_treated_as_v1() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let path = cfg_path();
    std::fs::create_dir_all(path.parent().unwrap()).expect("create cfg dir");

    // 畸形：包含 providers + current + version，但没有 apps，应按 v1 处理
    let v1_like = r#"{"providers":{},"current":"","version":2}"#;
    std::fs::write(&path, v1_like).expect("seed v1-like json");
    let before = std::fs::read_to_string(&path).expect("read before");

    let err = MultiAppConfig::load().expect_err("v1-like should not be parsed as v2");
    match err {
        AppError::Localized { key, .. } => assert_eq!(key, "config.unsupported_v1"),
        other => panic!("expected Localized v1 error, got {other:?}"),
    }

    let after = std::fs::read_to_string(&path).expect("read after");
    assert_eq!(before, after, "config.json should not be modified");
    let bak = home.join(".cc-switch").join("config.json.bak");
    assert!(!bak.exists(), ".bak should not be created on v1-like error");
}

#[test]
fn load_invalid_json_returns_parse_error_and_does_not_write() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let path = cfg_path();
    fs::create_dir_all(path.parent().unwrap()).expect("create cfg dir");

    fs::write(&path, "{not json").expect("seed invalid json");
    let before = fs::read_to_string(&path).expect("read before");

    let err = MultiAppConfig::load().expect_err("invalid json should error");
    match err {
        AppError::Json { .. } => {}
        other => panic!("expected Json error, got {other:?}"),
    }

    let after = fs::read_to_string(&path).expect("read after");
    assert_eq!(before, after, "config.json should remain unchanged");
    let bak = home.join(".cc-switch").join("config.json.bak");
    assert!(!bak.exists(), ".bak should not be created on parse error");
}

#[test]
fn load_valid_v2_config_succeeds() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let _home = ensure_test_home();
    let path = cfg_path();
    fs::create_dir_all(path.parent().unwrap()).expect("create cfg dir");

    // 使用默认结构序列化为 v2
    let default_cfg = MultiAppConfig::default();
    let json = serde_json::to_string_pretty(&default_cfg).expect("serialize default cfg");
    fs::write(&path, json).expect("write v2 json");

    let loaded = MultiAppConfig::load().expect("v2 should load successfully");
    assert_eq!(loaded.version, 2);
    assert!(loaded
        .get_manager(&cc_switch_lib::AppType::Claude)
        .is_some());
    assert!(loaded.get_manager(&cc_switch_lib::AppType::Codex).is_some());
}
