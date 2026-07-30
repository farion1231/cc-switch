use std::time::Duration;

use cc_switch_core::{CoreError, HeadlessState, ProviderRecord, ProviderService};
use rusqlite::{params, Connection};
use serde_json::json;

/// 构造完整 Provider DTO；测试显式填写所有字段，避免模型扩展后写路径静默遗漏数据。
fn provider(id: &str) -> ProviderRecord {
    ProviderRecord {
        id: id.to_string(),
        name: format!("Provider {id}"),
        settings_config: json!({ "env": { "ANTHROPIC_AUTH_TOKEN": format!("sk-{id}") } }),
        website_url: None,
        category: Some("custom".to_string()),
        created_at: None,
        sort_index: None,
        notes: None,
        meta: None,
        icon: None,
        icon_color: None,
        in_failover_queue: false,
    }
}

#[test]
fn lists_desktop_provider_fields_endpoints_and_current_state() {
    let home = tempfile::tempdir().expect("创建临时 HOME");
    let state = HeadlessState::memory(home.path()).expect("创建规范内存数据库");
    state
        .with_connection(|connection| {
            connection.execute(
                "INSERT INTO providers (
                    id, app_type, name, settings_config, website_url, category,
                    created_at, sort_index, notes, icon, icon_color, meta,
                    is_current, in_failover_queue
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, 1, 1)",
                params![
                    "remote-codex",
                    "codex",
                    "Remote Codex",
                    json!({ "model": "gpt-5" }).to_string(),
                    "https://example.com",
                    "custom",
                    1_700_000_000_i64,
                    4_i64,
                    "远端已有配置",
                    "openai",
                    "#111111",
                    json!({
                        "commonConfigEnabled": true,
                        "futureDesktopField": { "keep": true }
                    })
                    .to_string(),
                ],
            )?;
            connection.execute(
                "INSERT INTO provider_endpoints (provider_id, app_type, url, added_at)
                 VALUES (?1, ?2, ?3, ?4)",
                params![
                    "remote-codex",
                    "codex",
                    "https://backup.example/v1",
                    1_700_000_001_i64
                ],
            )?;
            Ok(())
        })
        .expect("写入桌面 Provider fixture");

    let providers = ProviderService::list(&state, "codex").expect("列出远端 Provider");
    let provider = &providers["remote-codex"];
    assert_eq!(provider.icon.as_deref(), Some("openai"));
    assert_eq!(provider.icon_color.as_deref(), Some("#111111"));
    assert!(provider.in_failover_queue);
    let meta = provider.meta.as_ref().expect("保留 meta");
    assert_eq!(meta["futureDesktopField"]["keep"], true);
    assert_eq!(
        meta["customEndpoints"]["https://backup.example/v1"]["addedAt"],
        1_700_000_001_i64
    );
    assert_eq!(
        ProviderService::current(&state, "codex").expect("读取当前 Provider"),
        "remote-codex"
    );
}

#[test]
fn provider_writes_use_app_type_and_keep_exactly_one_current_row() {
    let home = tempfile::tempdir().expect("创建临时 HOME");
    let state = HeadlessState::memory(home.path()).expect("创建规范内存数据库");

    ProviderService::add(&state, "claude", provider("a"), false).expect("新增 A");
    ProviderService::add(&state, "claude", provider("b"), false).expect("新增 B");
    ProviderService::switch(&state, "claude", "b").expect("切换 B");

    assert_eq!(
        ProviderService::current(&state, "claude").expect("当前项"),
        "b"
    );
    state
        .with_connection(|connection| {
            let current_count: i64 = connection.query_row(
                "SELECT COUNT(*) FROM providers WHERE app_type = 'claude' AND is_current = 1",
                [],
                |row| row.get(0),
            )?;
            assert_eq!(current_count, 1);
            assert!(!column_exists(connection, "providers", "app"));
            Ok(())
        })
        .expect("检查规范写入结果");
}

#[test]
fn rejected_provider_mutations_leave_existing_rows_unchanged() {
    let home = tempfile::tempdir().expect("创建临时 HOME");
    let state = HeadlessState::memory(home.path()).expect("创建规范内存数据库");
    ProviderService::add(&state, "claude", provider("a"), false).expect("新增 A");

    assert!(matches!(
        ProviderService::add(&state, "claude", provider("a"), false),
        Err(CoreError::Database(_))
    ));
    assert!(matches!(
        ProviderService::delete(&state, "claude", "a"),
        Err(CoreError::CurrentProviderDeletion)
    ));
    assert!(matches!(
        ProviderService::update(&state, "claude", "a", provider("renamed")),
        Err(CoreError::ProviderIdChangeUnsupported)
    ));
    assert!(matches!(
        ProviderService::add(&state, "unknown", provider("x"), false),
        Err(CoreError::UnsupportedApp(_))
    ));

    let listed = ProviderService::list(&state, "claude").expect("重新读取 Provider");
    assert_eq!(listed.len(), 1);
    assert!(listed.contains_key("a"));
    assert_eq!(
        ProviderService::current(&state, "claude").expect("当前项"),
        "a"
    );
}

#[test]
fn provider_update_preserves_endpoints_when_payload_omits_them() {
    let home = tempfile::tempdir().expect("创建临时 HOME");
    let state = HeadlessState::memory(home.path()).expect("创建规范内存数据库");
    let mut original = provider("a");
    original.meta = Some(json!({
        "customEndpoints": {
            "https://backup.example/v1": {
                "url": "https://backup.example/v1",
                "addedAt": 1234
            }
        }
    }));
    ProviderService::add(&state, "claude", original, false).expect("新增带 endpoint 的 Provider");

    let mut update = provider("a");
    update.name = "Updated A".to_string();
    update.meta = Some(json!({ "futureDesktopField": { "keep": true } }));
    ProviderService::update(&state, "claude", "a", update).expect("更新 Provider");

    let listed = ProviderService::list(&state, "claude").expect("读取更新结果");
    let meta = listed["a"].meta.as_ref().expect("保留 meta");
    assert_eq!(meta["futureDesktopField"]["keep"], true);
    assert_eq!(
        meta["customEndpoints"]["https://backup.example/v1"]["addedAt"],
        1234
    );
}

#[test]
fn locked_database_returns_database_busy_without_partial_provider_write() {
    let home = tempfile::tempdir().expect("创建临时 HOME");
    let state = HeadlessState::open(home.path()).expect("创建规范磁盘数据库");
    // 缩短测试等待时间；生产连接仍使用 schema 模块配置的五秒有界等待。
    state
        .with_connection(|connection| {
            connection.busy_timeout(Duration::from_millis(10))?;
            Ok(())
        })
        .expect("缩短 busy timeout");
    let database_path = home.path().join(".cc-switch").join("cc-switch.db");
    let locker = Connection::open(database_path).expect("打开竞争连接");
    locker
        .execute_batch("BEGIN EXCLUSIVE")
        .expect("持有数据库写锁");

    let error = ProviderService::add(&state, "claude", provider("locked"), false)
        .expect_err("写锁竞争必须返回稳定错误");
    locker.execute_batch("ROLLBACK").expect("释放数据库写锁");

    assert!(matches!(&error, CoreError::DatabaseBusy));
    assert_eq!(error.code(), "DATABASE_BUSY");
    assert!(ProviderService::list(&state, "claude")
        .expect("锁释放后读取 Provider")
        .is_empty());
}

fn column_exists(connection: &Connection, table: &str, column: &str) -> bool {
    // 表名仅来自测试常量，不接受外部输入；SQLite PRAGMA 不能参数绑定表名。
    let mut statement = connection
        .prepare(&format!("PRAGMA table_info({table})"))
        .expect("读取列信息");
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))
        .expect("查询列信息");
    let found = columns
        .map(|name| name.expect("读取列名"))
        .any(|name| name == column);
    drop(statement);
    found
}
