use super::{lock_backup_file_operations, Database};
use crate::error::AppError;
use crate::settings::{get_settings, update_settings, AppSettings};
use crate::test_support::EnvVars;
use rusqlite::Connection;
use serial_test::serial;

#[test]
#[serial]
fn backups_recover_after_poisoned_file_operation() {
    if !crate::sync_utils::in_isolated_process(
        "database::backup::tests::backups_recover_after_poisoned_file_operation",
    ) {
        return;
    }
    let _home = TestHomeGuard::new();
    let db = Database::memory().unwrap();
    db.set_setting("kept", "yes").unwrap();
    assert!(std::panic::catch_unwind(|| {
        let _guard = lock_backup_file_operations();
        panic!("backup operation interrupted");
    })
    .is_err());
    let path = db
        .backup_database_file()
        .expect("backup after panic")
        .expect("snapshot file");
    let saved = Connection::open(path).unwrap();
    let value: String = saved
        .query_row("SELECT value FROM settings WHERE key = 'kept'", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(value, "yes");
    assert!(!super::BACKUP_FILE_OPERATION_LOCK.is_poisoned());
}

struct TestHomeGuard {
    _env: EnvVars,
    temp_dir: tempfile::TempDir,
}

impl TestHomeGuard {
    fn new() -> Self {
        let temp_dir = tempfile::tempdir().expect("create isolated test home");
        let env = EnvVars::new().set("CC_SWITCH_TEST_HOME", temp_dir.path());
        // Prevent the Windows legacy-HOME fallback without mutating HOME:
        // an existing default DB keeps get_app_config_dir() anchored under
        // CC_SWITCH_TEST_HOME and makes import exercise its safety backup.
        let config_dir = temp_dir.path().join(".cc-switch");
        std::fs::create_dir_all(&config_dir).expect("create isolated config directory");
        std::fs::File::create(config_dir.join("cc-switch.db"))
            .expect("create isolated database sentinel");
        let guard = Self {
            _env: env,
            temp_dir,
        };
        let resolved = crate::config::get_app_config_dir();
        assert!(
            resolved.starts_with(guard.temp_dir.path()),
            "isolated test home resolved outside its temp directory: {}",
            resolved.display()
        );
        guard
    }

    fn path(&self) -> &std::path::Path {
        self.temp_dir.path()
    }
}

struct SettingsGuard {
    previous: AppSettings,
}

impl SettingsGuard {
    fn with_backup_retain_count(retain: u32) -> Self {
        let previous = get_settings();
        let mut next = previous.clone();
        next.backup_retain_count = Some(retain);
        update_settings(next).expect("set backup retention for test");
        Self { previous }
    }
}

impl Drop for SettingsGuard {
    fn drop(&mut self) {
        let _ = update_settings(self.previous.clone());
    }
}

#[test]
#[serial]
fn import_rejects_cross_file_statements_and_leaves_no_file_behind() -> Result<(), AppError> {
    let test_home = TestHomeGuard::new();
    // `VACUUM INTO` 是关键字扫描方案最容易漏的一条：它不含 "ATTACH" 字样，
    // 却和 ATTACH 一样落到 `AuthAction::Attach`（实测），因此同一条规则挡住两者。
    let cases: [(&str, &str); 2] = [
        ("attach", "ATTACH DATABASE '{path}' AS evil;"),
        ("vacuum-into", "VACUUM INTO '{path}';"),
    ];

    for (label, template) in cases {
        let target = test_home
            .path()
            .join(format!("cc-switch-authorizer-{label}.sqlite"));

        // 合法的导出头 + 越界语句。头部校验只比前缀，这份输入过得了它，
        // 真正拦下来的必须是 authorizer。
        let malicious = format!(
            "{}\n{}\n",
            super::CC_SWITCH_SQL_EXPORT_HEADER,
            template.replace("{path}", &target.to_string_lossy().replace('\'', "''"))
        );

        let db = Database::memory()?;
        let result = db.import_sql_string(&malicious);

        let error = result.expect_err("越界 SQL 必须被拒绝");
        assert!(
            error.to_string().to_ascii_lowercase().contains("authoriz"),
            "{label} 必须由 authorizer 拒绝，实际错误: {error}"
        );
        // 光报错不够：文件创建发生在 prepare 之后、暂存库 schema 校验之前，
        // 守卫若失效，即便导入整体失败，文件也已经躺在磁盘上了。
        assert!(
            !target.exists(),
            "被拒绝的 {label} 不得在磁盘上留下文件: {}",
            target.display()
        );
    }
    Ok(())
}

#[test]
#[serial]
fn import_still_accepts_a_genuine_export() -> Result<(), AppError> {
    let _test_home = TestHomeGuard::new();
    // 白名单收得紧，必须有一条回归防线证明它没误伤自家导出格式——
    // 这条测试红了就说明 dump_sql 写出了白名单没覆盖的语句。
    let source = Database::memory()?;
    {
        let conn = crate::database::lock_conn!(source.conn);
        conn.execute(
            "INSERT INTO providers (id, app_type, name, settings_config, meta)
                 VALUES ('p1', 'claude', 'Provider One', '{}', '{}')",
            [],
        )?;
    }
    let exported = source.export_sql_string()?;

    let target = Database::memory()?;
    target.import_sql_string(&exported)?;

    let conn = crate::database::lock_conn!(target.conn);
    let name: String = conn.query_row(
        "SELECT name FROM providers WHERE id = 'p1' AND app_type = 'claude'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(name, "Provider One");
    Ok(())
}

#[test]
#[serial]
fn import_clears_custom_icons_that_fail_validation() -> Result<(), AppError> {
    let _test_home = TestHomeGuard::new();
    // 保存时会拒绝的图标，可以直接写进别人给的 SQL 备份或云端快照：导入时要清掉
    let too_long = format!(
        "{}{}",
        crate::services::provider::icon::CUSTOM_ICON_PREFIX,
        "A".repeat(crate::services::provider::icon::MAX_CUSTOM_ICON_LENGTH)
    );
    let source = Database::memory()?;
    {
        let conn = crate::database::lock_conn!(source.conn);
        conn.execute(
            "INSERT INTO providers (id, app_type, name, settings_config, meta, icon)
                 VALUES ('p1', 'claude', 'Provider One', '{}', '{}', ?1),
                        ('p2', 'claude', 'Provider Two', '{}', '{}', 'claude')",
            [&too_long],
        )?;
        let universal = serde_json::json!({
            "u1": {
                "id": "u1",
                "name": "Universal",
                "providerType": "custom",
                "apps": {},
                "baseUrl": "https://api.example.com",
                "apiKey": "sk-test",
                "icon": too_long,
            },
        });
        conn.execute(
            "INSERT OR REPLACE INTO settings (key, value) VALUES ('universal_providers', ?1)",
            [universal.to_string()],
        )?;
    }
    let exported = source.export_sql_string()?;

    let target = Database::memory()?;
    target.import_sql_string(&exported)?;

    let conn = crate::database::lock_conn!(target.conn);
    let icon_of = |id: &str| -> Result<Option<String>, AppError> {
        Ok(conn.query_row(
            "SELECT icon FROM providers WHERE id = ?1 AND app_type = 'claude'",
            [id],
            |row| row.get(0),
        )?)
    };
    assert_eq!(icon_of("p1")?, None);
    assert_eq!(icon_of("p2")?, Some("claude".to_string()));
    drop(conn);
    let universal = target
        .get_universal_provider("u1")?
        .expect("universal provider survives the import");
    assert_eq!(universal.name, "Universal");
    assert_eq!(universal.icon, None);
    Ok(())
}

#[test]
#[serial]
fn import_accepts_genuine_export_without_provider_or_mcp_rows() -> Result<(), AppError> {
    let _test_home = TestHomeGuard::new();
    let source = Database::memory()?;
    {
        let conn = crate::database::lock_conn!(source.conn);
        let counts: (i64, i64) = conn.query_row(
            "SELECT
                    (SELECT COUNT(*) FROM providers),
                    (SELECT COUNT(*) FROM mcp_servers)",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        assert_eq!(counts, (0, 0));
        conn.execute(
            "INSERT INTO settings (key, value) VALUES ('empty-export-marker', 'kept')",
            [],
        )?;
    }
    let sql = source.export_sql_string()?;

    let target = Database::memory()?;
    {
        let conn = crate::database::lock_conn!(target.conn);
        conn.execute(
            "INSERT INTO providers (id, app_type, name, settings_config, meta)
                 VALUES ('target-sentinel', 'claude', 'Must Be Cleared', '{}', '{}')",
            [],
        )?;
    }
    target.import_sql_string(&sql)?;

    let conn = crate::database::lock_conn!(target.conn);
    let counts: (i64, i64) = conn.query_row(
        "SELECT
                (SELECT COUNT(*) FROM providers),
                (SELECT COUNT(*) FROM mcp_servers)",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    assert_eq!(counts, (0, 0));
    let marker: String = conn.query_row(
        "SELECT value FROM settings WHERE key = 'empty-export-marker'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(marker, "kept");
    Ok(())
}

#[test]
#[serial]
fn import_rejects_header_only_sql_and_keeps_existing_database() -> Result<(), AppError> {
    let _test_home = TestHomeGuard::new();
    let target = Database::memory()?;
    {
        let conn = crate::database::lock_conn!(target.conn);
        conn.execute(
            "INSERT INTO providers (id, app_type, name, settings_config, meta)
                 VALUES ('sentinel', 'claude', 'Existing Provider', '{}', '{}')",
            [],
        )?;
    }

    let header_only = format!(
        "{}\nPRAGMA foreign_keys=OFF;\nBEGIN TRANSACTION;\nCOMMIT;\n",
        super::CC_SWITCH_SQL_EXPORT_HEADER
    );
    let error = target
        .import_sql_string(&header_only)
        .expect_err("缺少原始 schema 的文件必须被拒绝");
    assert!(
        error.to_string().contains("required CC Switch tables")
            || error.to_string().contains("CC Switch 必需表"),
        "应由原始 schema 校验拒绝，实际错误: {error}"
    );

    let conn = crate::database::lock_conn!(target.conn);
    let provider: (i64, String) = conn.query_row(
        "SELECT COUNT(*), MIN(name) FROM providers WHERE id = 'sentinel'",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    assert_eq!(provider, (1, "Existing Provider".into()));
    Ok(())
}

#[test]
#[serial]
fn sql_import_preserves_incremental_auto_vacuum() -> Result<(), AppError> {
    let _test_home = TestHomeGuard::new();
    let source = Database::memory()?;
    {
        let conn = crate::database::lock_conn!(source.conn);
        conn.execute(
            "INSERT INTO providers (id, app_type, name, settings_config, meta)
                 VALUES ('vacuum-provider', 'claude', 'Vacuum Provider', '{}', '{}')",
            [],
        )?;
    }
    let sql = source.export_sql_string()?;

    let target = Database::memory()?;
    {
        let conn = crate::database::lock_conn!(target.conn);
        assert_eq!(Database::get_auto_vacuum_mode(&conn)?, 2);
    }

    target.import_sql_string(&sql)?;

    let conn = crate::database::lock_conn!(target.conn);
    assert_eq!(
        Database::get_auto_vacuum_mode(&conn)?,
        2,
        "SQL 导入不得把主库的 INCREMENTAL auto_vacuum 降级为 NONE"
    );
    Ok(())
}

#[test]
#[serial]
fn sql_file_api_round_trips_existing_export_behavior() -> Result<(), AppError> {
    let test_home = TestHomeGuard::new();
    let source = Database::memory()?;
    {
        let conn = crate::database::lock_conn!(source.conn);
        conn.execute_batch(
            "INSERT INTO providers (id, app_type, name, settings_config, meta)
                 VALUES ('file-provider', 'claude', 'File Provider', '{}', '{}');
                 INSERT INTO proxy_request_logs (
                     request_id, provider_id, app_type, model,
                     input_tokens, output_tokens, total_cost_usd,
                     latency_ms, status_code, created_at
                 ) VALUES ('file-request', 'file-provider', 'claude', 'claude-file', 5, 3, '0', 10, 200, 1);",
        )?;
    }

    let backup_path = test_home.path().join("round-trip.sql");
    source.export_sql(&backup_path)?;

    let target = Database::memory()?;
    {
        let conn = crate::database::lock_conn!(target.conn);
        conn.execute(
            "INSERT INTO providers (id, app_type, name, settings_config, meta)
                 VALUES ('target-sentinel', 'claude', 'Must Be Replaced', '{}', '{}')",
            [],
        )?;
    }
    target.import_sql(&backup_path)?;

    let conn = crate::database::lock_conn!(target.conn);
    let providers = conn
        .prepare("SELECT id FROM providers ORDER BY id")?
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(providers, vec!["file-provider"]);
    let request_exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM proxy_request_logs WHERE request_id = 'file-request')",
        [],
        |row| row.get(0),
    )?;
    assert!(request_exists, "文件 API 必须完整恢复导出数据");
    Ok(())
}

#[test]
#[serial]
fn failed_sql_import_keeps_the_existing_database_unchanged() -> Result<(), AppError> {
    let _test_home = TestHomeGuard::new();
    let target = Database::memory()?;
    {
        let conn = crate::database::lock_conn!(target.conn);
        conn.execute(
            "INSERT INTO providers (id, app_type, name, settings_config, meta)
                 VALUES ('sentinel', 'claude', 'Existing Provider', '{}', '{}')",
            [],
        )?;
    }

    let invalid_sql = format!(
        "{}\nBEGIN TRANSACTION;\nCREATE TABLE partial (id INTEGER);\nTHIS IS NOT SQL;\n",
        super::CC_SWITCH_SQL_EXPORT_HEADER
    );
    assert!(target.import_sql_string(&invalid_sql).is_err());

    let conn = crate::database::lock_conn!(target.conn);
    let provider: (i64, String, String) = conn.query_row(
        "SELECT COUNT(*), MIN(id), MIN(name) FROM providers",
        [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;
    assert_eq!(provider, (1, "sentinel".into(), "Existing Provider".into()));
    let partial_exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'partial')",
        [],
        |row| row.get(0),
    )?;
    assert!(!partial_exists, "失败导入的临时对象不得进入主库");
    Ok(())
}

#[test]
#[serial]
fn import_rejects_truncated_open_transaction_and_keeps_live_database() -> Result<(), AppError> {
    let _test_home = TestHomeGuard::new();
    let source = Database::memory()?;
    {
        let conn = crate::database::lock_conn!(source.conn);
        conn.execute(
            "INSERT INTO providers (id, app_type, name, settings_config, meta)
                 VALUES ('remote-provider', 'claude', 'Remote Provider', '{}', '{}')",
            [],
        )?;
    }
    let exported = source.export_sql_string()?;
    let truncated = exported
        .strip_suffix("COMMIT;\nPRAGMA foreign_keys=ON;\n")
        .expect("CC Switch export should end with a committed transaction");

    let target = Database::memory()?;
    {
        let conn = crate::database::lock_conn!(target.conn);
        conn.execute(
            "INSERT INTO providers (id, app_type, name, settings_config, meta)
                 VALUES ('live-provider', 'claude', 'Live Provider', '{}', '{}')",
            [],
        )?;
    }

    let error = target
        .import_sql_string(truncated)
        .expect_err("an export truncated before COMMIT must be rejected");
    assert!(
        error.to_string().contains("incomplete")
            || error.to_string().contains("未完成")
            || error.to_string().contains("截断"),
        "unexpected error: {error}"
    );

    let conn = crate::database::lock_conn!(target.conn);
    let providers = conn
        .prepare("SELECT id FROM providers ORDER BY id")?
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(providers, vec!["live-provider"]);
    Ok(())
}

#[test]
#[serial]
fn import_still_accepts_legacy_single_row_insert_exports() -> Result<(), AppError> {
    let _test_home = TestHomeGuard::new();
    // This schema is copied from the v3.8.3 tag. Its data statements use
    // the historical one-row-per-INSERT format and omit all newer columns.
    let legacy = format!(
        "{}\nPRAGMA foreign_keys=OFF;\nPRAGMA user_version=1;\nBEGIN TRANSACTION;\n{}
             INSERT INTO providers (
                 id, app_type, name, settings_config, meta, is_current
             ) VALUES (
                 'legacy-provider', 'claude', 'Legacy Provider',
                 '{{\"anthropicApiKey\":\"sk-old\"}}', '{{}}', 1
             );
             INSERT INTO skills (key, installed, installed_at)
             VALUES ('claude:legacy-skill', 1, 1700000000);
             COMMIT;\nPRAGMA foreign_keys=ON;\n",
        super::CC_SWITCH_SQL_EXPORT_HEADER,
        crate::database::tests::V3_8_SCHEMA_V1_SQL,
    );

    let target = Database::memory()?;
    target.import_sql_string(&legacy)?;

    let conn = crate::database::lock_conn!(target.conn);
    let user_version: i32 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    assert_eq!(user_version, crate::database::SCHEMA_VERSION);
    let provider: (String, String) = conn.query_row(
        "SELECT name, settings_config FROM providers WHERE id = 'legacy-provider'",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    assert_eq!(
        provider,
        (
            "Legacy Provider".into(),
            "{\"anthropicApiKey\":\"sk-old\"}".into()
        )
    );
    let cost_multiplier: String = conn.query_row(
        "SELECT cost_multiplier FROM providers WHERE id = 'legacy-provider'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(cost_multiplier, "1.0");
    let skill_snapshot: String = conn.query_row(
        "SELECT value FROM settings WHERE key = 'skills_ssot_migration_snapshot'",
        [],
        |row| row.get(0),
    )?;
    assert!(
        skill_snapshot.contains("legacy-skill"),
        "重建 skills 表时必须保留旧数据迁移快照"
    );
    Ok(())
}

#[test]
#[serial]
fn dump_sql_batches_rows_into_multi_row_inserts() -> Result<(), AppError> {
    let _test_home = TestHomeGuard::new();
    // 每行一条 INSERT 是导入慢的根源（恢复侧逐条解析，2 万行实测 21s）。
    // 这条测试钉死批量格式：450 行必须合并成 ceil(450/200) = 3 条语句。
    // 一旦退回到逐行导出，这里立刻变红。
    let db = Database::memory()?;
    {
        let conn = crate::database::lock_conn!(db.conn);
        for i in 0..450 {
            conn.execute(
                "INSERT INTO providers (id, app_type, name, settings_config, meta)
                     VALUES (?1, 'claude', 'p', '{}', '{}')",
                [format!("p{i}")],
            )?;
        }
    }

    let sql = db.export_sql_string()?;
    let insert_count = sql.matches("INSERT INTO \"providers\"").count();
    assert_eq!(
        insert_count, 3,
        "450 行应合并为 3 条多行 INSERT（每批 200 行），实际 {insert_count} 条"
    );

    let target = Database::memory()?;
    target.import_sql_string(&sql)?;
    let conn = crate::database::lock_conn!(target.conn);
    let row_count: i64 = conn.query_row("SELECT COUNT(*) FROM providers", [], |row| row.get(0))?;
    assert_eq!(row_count, 450, "批次边界不得漏行或重复行");
    for boundary in [0, 199, 200, 399, 400, 449] {
        let exists: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM providers WHERE id = ?1)",
            [format!("p{boundary}")],
            |row| row.get(0),
        )?;
        assert!(exists, "批次边界行 p{boundary} 必须完整恢复");
    }
    Ok(())
}

#[test]
fn dump_sql_splits_large_rows_by_statement_bytes() -> Result<(), AppError> {
    let source = Connection::open_in_memory()?;
    source.execute(
        "CREATE TABLE large_rows (id INTEGER PRIMARY KEY, payload TEXT NOT NULL)",
        [],
    )?;

    // Each row fits below the byte cap, while any pair exceeds it.
    let payload = "x".repeat(super::INSERT_BATCH_MAX_BYTES / 2 + 1024);
    for id in 1..=3 {
        source.execute(
            "INSERT INTO large_rows (id, payload) VALUES (?1, ?2)",
            rusqlite::params![id, payload],
        )?;
    }

    let sql = Database::dump_sql(&source, &[])?;
    let inserts = sql
        .lines()
        .filter(|line| line.starts_with("INSERT INTO \"large_rows\""))
        .collect::<Vec<_>>();
    assert_eq!(inserts.len(), 3, "超大字段应按 SQL 字节数提前切批");
    assert!(
        inserts
            .iter()
            .all(|statement| statement.len() <= super::INSERT_BATCH_MAX_BYTES),
        "每条可独立容纳的 INSERT 都应保持在字节上限内"
    );

    let target = Connection::open_in_memory()?;
    target.execute_batch(&sql)?;
    let (count, min_len, max_len): (i64, i64, i64) = target.query_row(
        "SELECT COUNT(*), MIN(length(payload)), MAX(length(payload)) FROM large_rows",
        [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;
    assert_eq!(count, 3);
    assert_eq!(min_len, payload.len() as i64);
    assert_eq!(max_len, payload.len() as i64);
    Ok(())
}

#[test]
fn dump_sql_round_trips_generated_columns_and_quoted_identifiers() -> Result<(), AppError> {
    let source = Connection::open_in_memory()?;
    source.execute_batch(
        r#"
            CREATE TABLE "generated""values" (
                "a" TEXT NOT NULL,
                "computed" TEXT GENERATED ALWAYS AS ("a" || '-generated') STORED,
                "b""tail" TEXT NOT NULL
            );
            INSERT INTO "generated""values" ("a", "b""tail")
            VALUES ('source', 'ordinary-tail');
            "#,
    )?;

    let sql = Database::dump_sql(&source, &[])?;
    assert!(sql.contains("INSERT INTO \"generated\"\"values\" (\"a\", \"b\"\"tail\") VALUES"));

    let target = Connection::open_in_memory()?;
    target.execute_batch(&sql)?;
    let values: (String, String, String) = target.query_row(
        "SELECT \"a\", \"computed\", \"b\"\"tail\" FROM \"generated\"\"values\"",
        [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;
    assert_eq!(
        values,
        (
            "source".to_string(),
            "source-generated".to_string(),
            "ordinary-tail".to_string()
        )
    );
    Ok(())
}

#[test]
fn dump_sql_preserves_text_bytes_and_real_storage_class() -> Result<(), AppError> {
    let source = Connection::open_in_memory()?;
    source.execute_batch(
        "CREATE TABLE scalar_values (
                 label TEXT PRIMARY KEY,
                 value ANY
             ) STRICT;
             INSERT INTO scalar_values VALUES ('nul-text', CAST(X'610062' AS TEXT));
             INSERT INTO scalar_values VALUES ('invalid-text', CAST(X'80FF' AS TEXT));",
    )?;
    for (label, value) in [
        ("real-one", 1.0),
        ("negative-zero", -0.0),
        ("positive-infinity", f64::INFINITY),
        ("negative-infinity", f64::NEG_INFINITY),
    ] {
        source.execute(
            "INSERT INTO scalar_values (label, value) VALUES (?1, ?2)",
            rusqlite::params![label, value],
        )?;
    }

    let sql = Database::dump_sql(&source, &[])?;
    assert!(
        sql.contains("CAST(X'610062' AS TEXT)") && sql.contains("CAST(X'80FF' AS TEXT)"),
        "含 NUL 或非法 UTF-8 的 TEXT 必须使用十六进制表达式"
    );

    let target = Connection::open_in_memory()?;
    target.execute_batch(&sql)?;

    for (label, expected_hex) in [("nul-text", "610062"), ("invalid-text", "80FF")] {
        let (storage_class, bytes): (String, String) = target.query_row(
            "SELECT typeof(value), hex(value) FROM scalar_values WHERE label = ?1",
            [label],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        assert_eq!(storage_class, "text");
        assert_eq!(bytes, expected_hex);
    }

    let real_value = |label: &str| -> Result<(String, f64), rusqlite::Error> {
        target.query_row(
            "SELECT typeof(value), value FROM scalar_values WHERE label = ?1",
            [label],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
    };
    let (storage_class, one) = real_value("real-one")?;
    assert_eq!(storage_class, "real");
    assert_eq!(one, 1.0);

    let (storage_class, negative_zero) = real_value("negative-zero")?;
    assert_eq!(storage_class, "real");
    assert_eq!(negative_zero, 0.0);
    assert!(negative_zero.is_sign_negative());

    let (storage_class, positive_infinity) = real_value("positive-infinity")?;
    assert_eq!(storage_class, "real");
    assert_eq!(positive_infinity, f64::INFINITY);

    let (storage_class, negative_infinity) = real_value("negative-infinity")?;
    assert_eq!(storage_class, "real");
    assert_eq!(negative_infinity, f64::NEG_INFINITY);
    Ok(())
}

#[test]
fn dump_sql_preserves_autoincrement_high_water_marks() -> Result<(), AppError> {
    let source = Connection::open_in_memory()?;
    source.execute_batch(
        "CREATE TABLE autoincrement_rows (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 value TEXT NOT NULL
             );
             INSERT INTO autoincrement_rows (value) VALUES ('one'), ('two'), ('deleted-high');
             DELETE FROM autoincrement_rows WHERE id = 3;",
    )?;

    let sql = Database::dump_sql(&source, &[])?;
    let target = Connection::open_in_memory()?;
    target.execute_batch(&sql)?;

    let sequence: i64 = target.query_row(
        "SELECT seq FROM sqlite_sequence WHERE name = 'autoincrement_rows'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(sequence, 3, "已删除的最高 ID 仍应保留在序列高水位中");
    target.execute(
        "INSERT INTO autoincrement_rows (value) VALUES ('after-restore')",
        [],
    )?;
    assert_eq!(target.last_insert_rowid(), 4);
    Ok(())
}

#[test]
fn sync_style_restore_preserves_local_autoincrement_high_water_marks() -> Result<(), AppError> {
    let source = Connection::open_in_memory()?;
    source.execute_batch(
        "CREATE TABLE autoincrement_rows (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 value TEXT NOT NULL
             );
             INSERT INTO autoincrement_rows (value) VALUES ('one'), ('two'), ('deleted-high');
             DELETE FROM autoincrement_rows WHERE id = 3;",
    )?;

    // A sync dump skips device-local rows and their sequence metadata.
    let staged_sql = Database::dump_sql(&source, &["autoincrement_rows"])?;
    let target = Connection::open_in_memory()?;
    target.execute_batch(&staged_sql)?;
    let staged_sequence_count: i64 = target.query_row(
        "SELECT COUNT(*) FROM sqlite_sequence WHERE name = 'autoincrement_rows'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(staged_sequence_count, 0);

    Database::restore_tables(&source, &target, &["autoincrement_rows"])?;
    let restored_sequence: i64 = target.query_row(
        "SELECT seq FROM sqlite_sequence WHERE name = 'autoincrement_rows'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(restored_sequence, 3);
    target.execute(
        "INSERT INTO autoincrement_rows (value) VALUES ('after-sync')",
        [],
    )?;
    assert_eq!(target.last_insert_rowid(), 4);
    Ok(())
}

#[test]
fn restore_tables_reads_only_insertable_columns() -> Result<(), AppError> {
    let source = Connection::open_in_memory()?;
    let target = Connection::open_in_memory()?;
    for conn in [&source, &target] {
        conn.execute_batch(
            r#"
                CREATE TABLE generated_values (
                    a TEXT NOT NULL,
                    computed TEXT GENERATED ALWAYS AS (a || '-generated') STORED,
                    "b""tail" TEXT NOT NULL
                );
                "#,
        )?;
    }
    source.execute(
        "INSERT INTO generated_values (a, \"b\"\"tail\") VALUES ('new', 'new-tail')",
        [],
    )?;
    target.execute(
        "INSERT INTO generated_values (a, \"b\"\"tail\") VALUES ('old', 'old-tail')",
        [],
    )?;

    Database::restore_tables(&source, &target, &["generated_values"])?;

    let values: (String, String, String) = target.query_row(
        "SELECT a, computed, \"b\"\"tail\" FROM generated_values",
        [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;
    assert_eq!(
        values,
        (
            "new".to_string(),
            "new-generated".to_string(),
            "new-tail".to_string()
        )
    );
    Ok(())
}

#[test]
fn restore_tables_rolls_back_all_tables_on_late_failure() -> Result<(), AppError> {
    let source = Connection::open_in_memory()?;
    source.execute_batch(
        "CREATE TABLE first_table (value TEXT NOT NULL);
             CREATE TABLE second_table (value INTEGER NOT NULL);
             INSERT INTO first_table VALUES ('replacement');
             INSERT INTO second_table VALUES (-1);",
    )?;

    let target = Connection::open_in_memory()?;
    target.execute_batch(
        "CREATE TABLE first_table (value TEXT NOT NULL);
             CREATE TABLE second_table (value INTEGER NOT NULL CHECK (value >= 0));
             INSERT INTO first_table VALUES ('sentinel-first');
             INSERT INTO second_table VALUES (7);",
    )?;

    let result = Database::restore_tables(&source, &target, &["first_table", "second_table"]);
    assert!(result.is_err(), "第二张表的约束错误必须终止恢复");

    let first: String = target.query_row("SELECT value FROM first_table", [], |row| row.get(0))?;
    let second: i64 = target.query_row("SELECT value FROM second_table", [], |row| row.get(0))?;
    assert_eq!(first, "sentinel-first", "第一张表必须随事务整体回滚");
    assert_eq!(second, 7, "失败表的 DELETE 也必须回滚");
    Ok(())
}

#[test]
fn dump_sql_loads_rows_before_creating_triggers() -> Result<(), AppError> {
    let source = Connection::open_in_memory()?;
    source.execute_batch(
        "CREATE TABLE triggered_rows (seq INTEGER PRIMARY KEY);
             INSERT INTO triggered_rows VALUES (1), (2), (3);
             CREATE TRIGGER ignore_second_row
             BEFORE INSERT ON triggered_rows
             WHEN NEW.seq = 2
             BEGIN
                 SELECT RAISE(IGNORE);
             END;",
    )?;

    let sql = Database::dump_sql(&source, &[])?;
    let data_pos = sql.find("INSERT INTO \"triggered_rows\"").unwrap();
    let trigger_pos = sql.find("CREATE TRIGGER ignore_second_row").unwrap();
    assert!(data_pos < trigger_pos, "触发器必须在数据恢复完成后创建");

    let target = Connection::open_in_memory()?;
    target.execute_batch(&sql)?;
    let rows = target
        .prepare("SELECT seq FROM triggered_rows ORDER BY seq")?
        .query_map([], |row| row.get::<_, i64>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(rows, vec![1, 2, 3]);
    let trigger_exists: bool = target.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'trigger' AND name = 'ignore_second_row')",
        [],
        |row| row.get(0),
    )?;
    assert!(trigger_exists, "触发器本身仍必须随备份恢复");
    Ok(())
}

#[test]
fn dump_sql_preserves_indexes_and_views() -> Result<(), AppError> {
    let source = Connection::open_in_memory()?;
    source.execute_batch(
        "CREATE TABLE indexed_rows (id INTEGER PRIMARY KEY, value TEXT NOT NULL);
             CREATE UNIQUE INDEX indexed_rows_value_idx ON indexed_rows(value);
             CREATE VIEW indexed_rows_view AS
                 SELECT id, value FROM indexed_rows WHERE value LIKE 'kept%';
             CREATE TRIGGER a_insert_indexed_rows_view
             INSTEAD OF INSERT ON indexed_rows_view
             BEGIN
                 INSERT INTO indexed_rows (id, value) VALUES (NEW.id, NEW.value);
             END;
             INSERT INTO indexed_rows VALUES (1, 'kept-value'), (2, 'hidden-value');",
    )?;

    let sql = Database::dump_sql(&source, &[])?;
    let target = Connection::open_in_memory()?;
    target.execute_batch(&sql)?;

    for (object_type, object_name) in [
        ("index", "indexed_rows_value_idx"),
        ("view", "indexed_rows_view"),
        ("trigger", "a_insert_indexed_rows_view"),
    ] {
        let exists: bool = target.query_row(
            "SELECT EXISTS(
                    SELECT 1 FROM sqlite_master WHERE type = ?1 AND name = ?2
                )",
            [object_type, object_name],
            |row| row.get(0),
        )?;
        assert!(exists, "{object_type} {object_name} 必须随 SQL dump 恢复");
    }

    target.execute(
        "INSERT INTO indexed_rows_view (id, value) VALUES (3, 'kept-via-trigger')",
        [],
    )?;
    let view_rows = target
        .prepare("SELECT id, value FROM indexed_rows_view ORDER BY id")?
        .query_map([], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(
        view_rows,
        vec![
            (1, "kept-value".to_string()),
            (3, "kept-via-trigger".to_string()),
        ]
    );
    Ok(())
}

#[test]
#[serial]
fn multi_row_dump_round_trips_special_values() -> Result<(), AppError> {
    let _test_home = TestHomeGuard::new();
    // 多行 VALUES 的转义面比单行宽：单引号、换行、英文逗号（列分隔符）、
    // 中文、emoji、BLOB、NULL——任何一个处理错都会让整批语法崩掉或数据变形。
    let source = Database::memory()?;
    {
        let conn = crate::database::lock_conn!(source.conn);
        conn.execute(
            "INSERT INTO providers (id, app_type, name, settings_config, meta)
                 VALUES ('special', 'claude', ?1, ?2, '{}')",
            rusqlite::params!["O'Brien,\n第二行 \"quoted\" 😀", "{\"key\": \"it's, ok\"}"],
        )?;
        conn.execute(
            "INSERT INTO providers (id, app_type, name, settings_config, meta)
                 VALUES ('with-blob', 'claude', 'blob', X'00FF10', '{}')",
            [],
        )?;
        conn.execute(
            "INSERT INTO providers (id, app_type, name, settings_config, meta, category)
                 VALUES ('with-null', 'claude', 'nullcat', '{}', '{}', NULL)",
            [],
        )?;
    }

    let sql = source.export_sql_string()?;
    let target = Database::memory()?;
    target.import_sql_string(&sql)?;

    let conn = crate::database::lock_conn!(target.conn);
    let name: String = conn.query_row(
        "SELECT name FROM providers WHERE id = 'special'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(name, "O'Brien,\n第二行 \"quoted\" 😀");
    let cfg: String = conn.query_row(
        "SELECT settings_config FROM providers WHERE id = 'special'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(cfg, "{\"key\": \"it's, ok\"}");

    let blob_type: String = conn.query_row(
        "SELECT typeof(settings_config) FROM providers WHERE id = 'with-blob'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(blob_type, "blob", "BLOB 存储类型必须在往返后保留");
    let blob: Vec<u8> = conn.query_row(
        "SELECT settings_config FROM providers WHERE id = 'with-blob'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(blob, vec![0x00, 0xFF, 0x10]);

    let category: Option<String> = conn.query_row(
        "SELECT category FROM providers WHERE id = 'with-null'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(category, None, "NULL 必须在往返后保留");
    Ok(())
}

#[test]
#[serial]
fn full_sql_backup_still_round_trips_session_cursors() -> Result<(), AppError> {
    let _test_home = TestHomeGuard::new();
    let source = Database::memory()?;
    {
        let conn = crate::database::lock_conn!(source.conn);
        conn.execute_batch(
            "INSERT INTO providers (id, app_type, name, settings_config, meta)
                 VALUES ('cursor-provider', 'claude', 'Cursor Provider', '{}', '{}');
                 INSERT INTO session_log_sync (
                     file_path, last_modified, last_line_offset, last_synced_at
                 ) VALUES ('/local/sessions/manual-backup.jsonl', 11, 22, 33);",
        )?;
    }

    let sql = source.export_sql_string()?;
    let target = Database::memory()?;
    target.import_sql_string(&sql)?;

    let conn = crate::database::lock_conn!(target.conn);
    let cursor: (String, i64, i64, i64) = conn.query_row(
        "SELECT file_path, last_modified, last_line_offset, last_synced_at
             FROM session_log_sync",
        [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
    )?;
    assert_eq!(
        cursor,
        ("/local/sessions/manual-backup.jsonl".into(), 11, 22, 33,)
    );
    Ok(())
}

#[test]
fn every_sync_preserved_table_is_skipped_from_remote_payloads() {
    for table in super::SYNC_PRESERVE_TABLES {
        assert!(
            super::SYNC_SKIP_TABLES.contains(table),
            "本地保留表 {table} 也必须从远端 payload 中排除"
        );
    }
}

#[test]
#[serial]
fn sync_import_preserves_local_only_tables() -> Result<(), AppError> {
    let _test_home = TestHomeGuard::new();
    let remote_db = Database::memory()?;
    {
        let conn = crate::database::lock_conn!(remote_db.conn);
        conn.execute_batch(
            "INSERT INTO providers (id, app_type, name, settings_config, meta)
                 VALUES ('remote-provider', 'claude', 'Remote Provider', '{}', '{}');
                 INSERT INTO proxy_request_logs (
                     request_id, provider_id, app_type, model,
                     input_tokens, output_tokens, total_cost_usd,
                     latency_ms, status_code, created_at
                 ) VALUES ('remote-request', 'remote-provider', 'claude', 'remote-model', 1, 1, '1', 1, 200, 1);
                 INSERT INTO usage_daily_rollups (
                     date, app_type, provider_id, model, request_count, success_count,
                     input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                     total_cost_usd, avg_latency_ms
                 ) VALUES ('2099-01-01', 'claude', 'remote-provider', 'remote-model', 1, 1, 1, 1, 0, 0, '1', 1);
                 INSERT INTO stream_check_logs (
                     provider_id, provider_name, app_type, status, success, message,
                     response_time_ms, http_status, model_used, retry_count, tested_at
                 ) VALUES ('remote-provider', 'Remote Provider', 'claude', 'failed', 0, 'remote', 1, 500, 'remote-model', 0, 1);
                 INSERT INTO proxy_live_backup (app_type, original_config, backed_up_at)
                 VALUES ('claude', 'remote-live', '2099-01-01');
                 INSERT INTO provider_health (
                     provider_id, app_type, is_healthy, consecutive_failures, updated_at
                 ) VALUES ('remote-provider', 'claude', 0, 9, '2099-01-01');
                 INSERT INTO session_log_sync (
                     file_path, last_modified, last_line_offset, last_synced_at
                 ) VALUES ('/remote/sessions/one.jsonl', 9, 99, 999);",
        )?;
    }
    let remote_sql = remote_db.export_sql_string_for_sync()?;
    let exported = Connection::open_in_memory()?;
    exported.execute_batch(&remote_sql)?;
    let skipped_counts: (i64, i64, i64, i64, i64, i64) = exported.query_row(
        "SELECT
                (SELECT COUNT(*) FROM proxy_request_logs),
                (SELECT COUNT(*) FROM stream_check_logs),
                (SELECT COUNT(*) FROM provider_health),
                (SELECT COUNT(*) FROM proxy_live_backup),
                (SELECT COUNT(*) FROM usage_daily_rollups),
                (SELECT COUNT(*) FROM session_log_sync)",
        [],
        |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
            ))
        },
    )?;
    assert_eq!(skipped_counts, (0, 0, 0, 0, 0, 0));

    let local_db = Database::memory()?;
    {
        let conn = crate::database::lock_conn!(local_db.conn);
        conn.execute_batch(
            "INSERT INTO providers (id, app_type, name, settings_config, meta)
                 VALUES ('local-provider', 'claude', 'Local Provider', '{}', '{}');
                 INSERT INTO proxy_request_logs (
                     request_id, provider_id, app_type, model,
                     input_tokens, output_tokens, total_cost_usd,
                     latency_ms, status_code, created_at
                 ) VALUES ('req-1', 'local-provider', 'claude', 'claude-3', 100, 50, '0.01', 120, 200, 1000);
                 INSERT INTO usage_daily_rollups (
                     date, app_type, provider_id, model, request_count, success_count,
                     input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                     total_cost_usd, avg_latency_ms
                 ) VALUES ('2026-03-01', 'claude', 'local-provider', 'claude-3', 7, 7, 700, 350, 0, 0, '0.07', 120);
                 INSERT INTO stream_check_logs (
                     provider_id, provider_name, app_type, status, success, message,
                     response_time_ms, http_status, model_used, retry_count, tested_at
                 ) VALUES ('local-provider', 'Local Provider', 'claude', 'operational', 1, 'local-ok', 42, 200, 'claude-3', 0, 1000);
                 INSERT INTO proxy_live_backup (app_type, original_config, backed_up_at)
                 VALUES ('claude', '{\"local\":true}', '2026-03-01');
                 INSERT INTO provider_health (
                     provider_id, app_type, is_healthy, consecutive_failures, updated_at
                 ) VALUES ('local-provider', 'claude', 1, 0, '2026-03-01');
                 INSERT INTO session_log_sync (
                     file_path, last_modified, last_line_offset, last_synced_at
                 ) VALUES ('/local/sessions/one.jsonl', 10, 123, 456);",
        )?;
    }

    local_db.import_sql_string_for_sync(&remote_sql)?;

    let conn = crate::database::lock_conn!(local_db.conn);
    let providers = conn
        .prepare("SELECT id FROM providers ORDER BY id")?
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(providers, vec!["remote-provider"]);

    let preserved_counts: (i64, i64, i64, i64, i64) = conn.query_row(
        "SELECT
                (SELECT COUNT(*) FROM proxy_request_logs),
                (SELECT COUNT(*) FROM stream_check_logs),
                (SELECT COUNT(*) FROM proxy_live_backup),
                (SELECT COUNT(*) FROM usage_daily_rollups),
                (SELECT COUNT(*) FROM session_log_sync)",
        [],
        |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
            ))
        },
    )?;
    assert_eq!(
        preserved_counts,
        (1, 1, 1, 1, 1),
        "同步导入必须替换配置，同时保留本机日志、Live 备份与会话游标"
    );

    let preserved_values: (String, String, i64, String, i64, String, i64) = conn.query_row(
        "SELECT
                (SELECT request_id FROM proxy_request_logs),
                (SELECT model FROM proxy_request_logs),
                (SELECT input_tokens FROM proxy_request_logs),
                (SELECT date FROM usage_daily_rollups),
                (SELECT request_count FROM usage_daily_rollups),
                (SELECT message FROM stream_check_logs),
                (SELECT response_time_ms FROM stream_check_logs)",
        [],
        |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
                row.get(6)?,
            ))
        },
    )?;
    assert_eq!(
        preserved_values,
        (
            "req-1".into(),
            "claude-3".into(),
            100,
            "2026-03-01".into(),
            7,
            "local-ok".into(),
            42,
        )
    );

    let live_backup: (String, String) = conn.query_row(
        "SELECT original_config, backed_up_at FROM proxy_live_backup WHERE app_type = 'claude'",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    assert_eq!(
        live_backup,
        ("{\"local\":true}".into(), "2026-03-01".into())
    );
    let session_cursor: (String, i64, i64, i64) = conn.query_row(
        "SELECT file_path, last_modified, last_line_offset, last_synced_at
             FROM session_log_sync",
        [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
    )?;
    assert_eq!(
        session_cursor,
        ("/local/sessions/one.jsonl".into(), 10, 123, 456)
    );
    let provider_health_count: i64 =
        conn.query_row("SELECT COUNT(*) FROM provider_health", [], |row| row.get(0))?;
    assert_eq!(
        provider_health_count, 0,
        "同步导入应清除可重建的本地 provider_health 状态"
    );
    Ok(())
}

#[test]
#[serial]
fn failed_backup_publish_leaves_no_visible_or_temporary_file() -> Result<(), AppError> {
    let _test_home = TestHomeGuard::new();
    let db = Database::init()?;
    let backup_dir = crate::config::get_app_config_dir().join("backups");
    std::fs::create_dir_all(&backup_dir).map_err(|e| AppError::io(&backup_dir, e))?;
    let mut files_before = std::fs::read_dir(&backup_dir)
        .map_err(|e| AppError::io(&backup_dir, e))?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name())
        .collect::<Vec<_>>();
    files_before.sort();
    let mut visible_before = Database::list_backups()?
        .into_iter()
        .map(|entry| entry.filename)
        .collect::<Vec<_>>();
    visible_before.sort();

    let error = {
        let backup_file_guard = lock_backup_file_operations();
        let conn = crate::database::lock_conn!(db.conn);
        Database::backup_database_file_from_conn_with_hook(
            &backup_file_guard,
            &conn,
            &[],
            |temp_path, target_path| {
                assert!(
                    temp_path.exists(),
                    "completed backup should exist before publish"
                );
                assert_ne!(
                    temp_path.extension().and_then(|ext| ext.to_str()),
                    Some("db"),
                    "staging files must stay invisible to backup discovery"
                );
                assert!(!target_path.exists());
                Err(AppError::Config("simulated publish failure".to_string()))
            },
        )
    }
    .expect_err("publish failure must be returned");
    assert!(error.to_string().contains("simulated publish failure"));
    let mut visible_after = Database::list_backups()?
        .into_iter()
        .map(|entry| entry.filename)
        .collect::<Vec<_>>();
    visible_after.sort();
    assert_eq!(visible_after, visible_before);
    let mut files_after = std::fs::read_dir(&backup_dir)
        .map_err(|e| AppError::io(&backup_dir, e))?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name())
        .collect::<Vec<_>>();
    files_after.sort();
    assert_eq!(
        files_after, files_before,
        "failed publish must not leave either a visible backup or a temporary file"
    );
    Ok(())
}

#[test]
#[serial]
fn backup_publish_retries_a_noclobber_name_collision() -> Result<(), AppError> {
    let _test_home = TestHomeGuard::new();
    let _settings = SettingsGuard::with_backup_retain_count(10);
    let db = Database::init()?;
    let mut claimed_path = None;

    let published_path = {
        let backup_file_guard = lock_backup_file_operations();
        let conn = crate::database::lock_conn!(db.conn);
        Database::backup_database_file_from_conn_with_hook(
            &backup_file_guard,
            &conn,
            &[],
            |_, target_path| {
                claimed_path = Some(target_path.to_path_buf());
                std::fs::write(target_path, b"claimed by another process")
                    .map_err(|e| AppError::io(target_path, e))?;
                Ok(())
            },
        )?
        .expect("file-backed database should create a backup")
    };

    let claimed_path = claimed_path.expect("publish hook should receive the first target");
    assert_ne!(published_path, claimed_path);
    assert_eq!(
        std::fs::read(&claimed_path).map_err(|e| AppError::io(&claimed_path, e))?,
        b"claimed by another process"
    );
    let published_conn =
        Connection::open_with_flags(&published_path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    Database::validate_sqlite_integrity(&published_conn)?;

    let backup_dir = crate::config::get_app_config_dir().join("backups");
    let temporary_files = std::fs::read_dir(&backup_dir)
        .map_err(|e| AppError::io(&backup_dir, e))?
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with(".cc-switch-backup-")
        })
        .count();
    assert_eq!(
        temporary_files, 0,
        "publish retry must consume the temp file"
    );
    Ok(())
}

#[test]
#[serial]
fn concurrent_backup_renames_never_overwrite_the_shared_target() -> Result<(), AppError> {
    let _test_home = TestHomeGuard::new();
    let _settings = SettingsGuard::with_backup_retain_count(10);
    let db = Database::init()?;
    let mut source_filenames = Vec::new();
    for provider_id in ["first-source", "second-source"] {
        {
            let conn = crate::database::lock_conn!(db.conn);
            conn.execute("DELETE FROM providers", [])?;
            conn.execute(
                "INSERT INTO providers (id, app_type, name, settings_config, meta)
                     VALUES (?1, 'claude', ?1, '{}', '{}')",
                [provider_id],
            )?;
        }
        let source_path = db
            .backup_database_file()?
            .expect("file-backed database should create a backup");
        source_filenames.push(
            source_path
                .file_name()
                .expect("backup should have a filename")
                .to_string_lossy()
                .into_owned(),
        );
    }

    let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
    let handles = source_filenames
        .iter()
        .cloned()
        .map(|source_filename| {
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                Database::rename_backup(&source_filename, "shared-target")
                    .map_err(|e| e.to_string())
            })
        })
        .collect::<Vec<_>>();
    barrier.wait();
    let results = handles
        .into_iter()
        .map(|handle| {
            handle
                .join()
                .map_err(|_| AppError::Config("rename thread panicked".to_string()))
        })
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(results.iter().filter(|result| result.is_err()).count(), 1);

    let backup_dir = crate::config::get_app_config_dir().join("backups");
    let target_path = backup_dir.join("shared-target.db");
    let remaining_source = source_filenames
        .iter()
        .map(|filename| backup_dir.join(filename))
        .find(|path| path.exists())
        .expect("the losing source must remain after the target collision");
    let mut provider_ids = [&target_path, &remaining_source]
        .into_iter()
        .map(|path| -> Result<String, AppError> {
            let conn =
                Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
            conn.query_row("SELECT id FROM providers", [], |row| row.get(0))
                .map_err(AppError::from)
        })
        .collect::<Result<Vec<_>, _>>()?;
    provider_ids.sort();
    assert_eq!(provider_ids, vec!["first-source", "second-source"]);
    Ok(())
}

#[test]
#[serial]
fn sync_import_keeps_local_writes_that_arrive_after_staging() -> Result<(), AppError> {
    let _test_home = TestHomeGuard::new();
    let remote_db = Database::memory()?;
    {
        let conn = crate::database::lock_conn!(remote_db.conn);
        conn.execute(
            "INSERT INTO providers (id, app_type, name, settings_config, meta)
                 VALUES ('remote-provider', 'claude', 'Remote Provider', '{}', '{}')",
            [],
        )?;
    }
    let remote_sql = remote_db.export_sql_string_for_sync()?;

    let local_db = Database::memory()?;
    {
        let conn = crate::database::lock_conn!(local_db.conn);
        conn.execute(
            "INSERT INTO providers (id, app_type, name, settings_config, meta)
                 VALUES ('local-provider', 'claude', 'Local Provider', '{}', '{}')",
            [],
        )?;
    }

    local_db.import_sql_string_inner_with_hook(
        &remote_sql,
        super::SYNC_PRESERVE_TABLES,
        || {
            // Deterministically simulate writes after the remote SQL has
            // finished staging but before the main database is replaced.
            let conn = crate::database::lock_conn!(local_db.conn);
            conn.execute_batch(
                "INSERT INTO proxy_request_logs (
                         request_id, provider_id, app_type, model,
                         input_tokens, output_tokens, total_cost_usd,
                         latency_ms, status_code, created_at
                     ) VALUES ('late-request', 'local-provider', 'claude', 'late-model', 1, 1, '0', 1, 200, 1);
                     INSERT INTO usage_daily_rollups (
                         date, app_type, provider_id, model, request_count, success_count,
                         input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                         total_cost_usd, avg_latency_ms
                     ) VALUES ('2026-08-04', 'claude', 'local-provider', 'late-model', 1, 1, 1, 1, 0, 0, '0', 1);
                     INSERT INTO stream_check_logs (
                         provider_id, provider_name, app_type, status, success, message,
                         response_time_ms, http_status, model_used, retry_count, tested_at
                     ) VALUES ('local-provider', 'Local Provider', 'claude', 'operational', 1, 'late', 1, 200, 'late-model', 0, 1);
                     INSERT INTO proxy_live_backup (app_type, original_config, backed_up_at)
                     VALUES ('claude', 'late-live', '2026-08-04');
                     INSERT INTO session_log_sync (
                         file_path, last_modified, last_line_offset, last_synced_at
                     ) VALUES ('/local/sessions/late.jsonl', 1, 2, 3);",
            )?;
            Ok(())
        },
    )?;

    let conn = crate::database::lock_conn!(local_db.conn);
    let providers = conn
        .prepare("SELECT id FROM providers ORDER BY id")?
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(providers, vec!["remote-provider"]);
    let preserved_counts: (i64, i64, i64, i64, i64) = conn.query_row(
        "SELECT
                (SELECT COUNT(*) FROM proxy_request_logs WHERE request_id = 'late-request'),
                (SELECT COUNT(*) FROM usage_daily_rollups WHERE date = '2026-08-04'),
                (SELECT COUNT(*) FROM stream_check_logs WHERE message = 'late'),
                (SELECT COUNT(*) FROM proxy_live_backup WHERE original_config = 'late-live'),
                (SELECT COUNT(*) FROM session_log_sync WHERE file_path = '/local/sessions/late.jsonl')",
        [],
        |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
            ))
        },
    )?;
    assert_eq!(preserved_counts, (1, 1, 1, 1, 1));
    Ok(())
}

#[test]
#[serial]
fn sync_import_safety_backup_captures_late_local_writes() -> Result<(), AppError> {
    let _test_home = TestHomeGuard::new();
    let remote_db = Database::memory()?;
    {
        let conn = crate::database::lock_conn!(remote_db.conn);
        conn.execute(
            "INSERT INTO providers (id, app_type, name, settings_config, meta)
                 VALUES ('remote-provider', 'claude', 'Remote Provider', '{}', '{}')",
            [],
        )?;
    }
    let remote_sql = remote_db.export_sql_string_for_sync()?;

    let local_db = Database::init()?;
    {
        let conn = crate::database::lock_conn!(local_db.conn);
        conn.execute("DELETE FROM providers", [])?;
        conn.execute(
            "INSERT INTO providers (id, app_type, name, settings_config, meta)
                 VALUES ('local-provider', 'claude', 'Local Provider', '{}', '{}')",
            [],
        )?;
    }

    let safety_id = local_db.import_sql_string_inner_with_hook(
        &remote_sql,
        super::SYNC_PRESERVE_TABLES,
        || {
            let conn = crate::database::lock_conn!(local_db.conn);
            conn.execute(
                "INSERT INTO proxy_request_logs (
                         request_id, provider_id, app_type, model,
                         input_tokens, output_tokens, total_cost_usd,
                         latency_ms, status_code, created_at
                     ) VALUES ('late-request', 'local-provider', 'claude', 'late-model', 1, 1, '0', 1, 200, 1)",
                [],
            )?;
            Ok(())
        },
    )?;
    assert!(!safety_id.is_empty());

    {
        let conn = crate::database::lock_conn!(local_db.conn);
        let live_provider: String =
            conn.query_row("SELECT id FROM providers", [], |row| row.get(0))?;
        assert_eq!(live_provider, "remote-provider");
        let late_request_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM proxy_request_logs WHERE request_id = 'late-request'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(late_request_count, 1);
    }

    let safety_path = crate::config::get_app_config_dir()
        .join("backups")
        .join(format!("{safety_id}.db"));
    let safety_conn =
        Connection::open_with_flags(&safety_path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let safety_provider: String =
        safety_conn.query_row("SELECT id FROM providers", [], |row| row.get(0))?;
    assert_eq!(
        safety_provider, "local-provider",
        "safety backup must capture the exact pre-import provider state"
    );
    let safety_late_request_count: i64 = safety_conn.query_row(
        "SELECT COUNT(*) FROM proxy_request_logs WHERE request_id = 'late-request'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(
        safety_late_request_count, 1,
        "safety backup must include writes that arrived after staging"
    );
    Ok(())
}

#[test]
#[serial]
fn restore_with_retain_one_keeps_source_and_exact_safety_snapshot() -> Result<(), AppError> {
    let _test_home = TestHomeGuard::new();
    let _settings = SettingsGuard::with_backup_retain_count(1);
    let db = Database::init()?;

    {
        let conn = crate::database::lock_conn!(db.conn);
        conn.execute("DELETE FROM providers", [])?;
        conn.execute(
            "INSERT INTO providers (id, app_type, name, settings_config, meta)
                 VALUES ('restore-source', 'claude', 'Restore Source', '{}', '{}')",
            [],
        )?;
    }
    let source_path = db
        .backup_database_file()?
        .expect("file-backed database should create a backup");
    let source_filename = source_path
        .file_name()
        .expect("backup should have a filename")
        .to_string_lossy()
        .into_owned();
    let backup_dir = crate::config::get_app_config_dir().join("backups");
    let stale_path = backup_dir.join("stale-unprotected.db");
    std::fs::write(&stale_path, b"stale").map_err(|e| AppError::io(&stale_path, e))?;

    {
        let conn = crate::database::lock_conn!(db.conn);
        conn.execute("DELETE FROM providers", [])?;
        conn.execute(
            "INSERT INTO providers (id, app_type, name, settings_config, meta)
                 VALUES ('live-before-restore', 'claude', 'Live Before Restore', '{}', '{}')",
            [],
        )?;
    }

    let safety_id = db.restore_from_backup(&source_filename)?;
    let safety_path = backup_dir.join(format!("{safety_id}.db"));
    assert!(
        source_path.exists(),
        "selected restore source must be retained"
    );
    assert!(
        safety_path.exists(),
        "pre-restore safety backup must be retained"
    );
    assert!(
        !stale_path.exists(),
        "retention should still remove an unprotected stale backup"
    );

    let backup_count = std::fs::read_dir(&backup_dir)
        .map_err(|e| AppError::io(&backup_dir, e))?
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "db"))
        .count();
    assert_eq!(
        backup_count, 2,
        "retain=1 may be exceeded temporarily to protect both recovery endpoints"
    );

    let live_provider: String = {
        let conn = crate::database::lock_conn!(db.conn);
        conn.query_row("SELECT id FROM providers", [], |row| row.get(0))?
    };
    assert_eq!(live_provider, "restore-source");

    let safety_conn =
        Connection::open_with_flags(&safety_path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let safety_provider: String =
        safety_conn.query_row("SELECT id FROM providers", [], |row| row.get(0))?;
    assert_eq!(
        safety_provider, "live-before-restore",
        "safety backup must exactly represent the live state being replaced"
    );
    Ok(())
}

#[cfg(windows)]
#[test]
#[serial]
fn restore_protects_case_variant_source_path_from_retention() -> Result<(), AppError> {
    let _test_home = TestHomeGuard::new();
    let _settings = SettingsGuard::with_backup_retain_count(1);
    let db = Database::init()?;
    let source_path = db
        .backup_database_file()?
        .expect("file-backed database should create a backup");
    let source_filename = source_path
        .file_name()
        .expect("backup should have a filename")
        .to_string_lossy()
        .into_owned();
    let case_variant = format!(
        "{}.db",
        source_filename
            .strip_suffix(".db")
            .expect("generated backup should use a .db suffix")
            .to_ascii_uppercase()
    );

    db.restore_from_backup(&case_variant)?;
    assert!(
        source_path.exists(),
        "retention must recognize a case-variant path as the selected source"
    );
    Ok(())
}

#[test]
#[serial]
fn restore_blocks_backup_deletion_until_live_replacement_finishes() -> Result<(), AppError> {
    let _test_home = TestHomeGuard::new();
    let db = Database::init()?;
    {
        let conn = crate::database::lock_conn!(db.conn);
        conn.execute("DELETE FROM providers", [])?;
        conn.execute(
            "INSERT INTO providers (id, app_type, name, settings_config, meta)
                 VALUES ('restore-source', 'claude', 'Restore Source', '{}', '{}')",
            [],
        )?;
    }
    let source_path = db
        .backup_database_file()?
        .expect("file-backed database should create a backup");
    let source_filename = source_path
        .file_name()
        .expect("backup should have a filename")
        .to_string_lossy()
        .into_owned();
    {
        let conn = crate::database::lock_conn!(db.conn);
        conn.execute("DELETE FROM providers", [])?;
        conn.execute(
            "INSERT INTO providers (id, app_type, name, settings_config, meta)
                 VALUES ('live-before-restore', 'claude', 'Live Before Restore', '{}', '{}')",
            [],
        )?;
    }

    let (attempt_tx, attempt_rx) = std::sync::mpsc::channel();
    let (result_tx, result_rx) = std::sync::mpsc::channel();
    let mut delete_handle = None;
    let mut observed_safety_filename = None;
    let safety_id = db.restore_from_backup_with_hook(&source_filename, |safety_path| {
        let safety_path = safety_path
            .ok_or_else(|| AppError::Config("restore should create a safety backup".to_string()))?;
        let safety_filename = safety_path
            .file_name()
            .ok_or_else(|| AppError::Config("safety backup has no filename".to_string()))?
            .to_string_lossy()
            .into_owned();
        observed_safety_filename = Some(safety_filename.clone());
        delete_handle = Some(std::thread::spawn(move || {
            let _ = attempt_tx.send(());
            let result = Database::delete_backup(&safety_filename).map_err(|e| e.to_string());
            let _ = result_tx.send(result);
        }));

        attempt_rx
            .recv_timeout(std::time::Duration::from_secs(1))
            .map_err(|e| AppError::Config(format!("delete thread did not start: {e}")))?;
        match result_rx.recv_timeout(std::time::Duration::from_millis(150)) {
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Ok(()),
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => Err(AppError::Config(
                "delete thread disconnected before restore completed".to_string(),
            )),
            Ok(result) => Err(AppError::Config(format!(
                "backup deletion completed before live replacement: {result:?}"
            ))),
        }
    })?;

    let delete_result = result_rx
        .recv_timeout(std::time::Duration::from_secs(2))
        .map_err(|e| AppError::Config(format!("delete did not resume after restore: {e}")))?;
    delete_result.map_err(AppError::Config)?;
    delete_handle
        .expect("delete thread should be created")
        .join()
        .map_err(|_| AppError::Config("delete thread panicked".to_string()))?;

    let expected_safety_filename = format!("{safety_id}.db");
    assert_eq!(
        observed_safety_filename.as_deref(),
        Some(expected_safety_filename.as_str())
    );
    let live_provider: String = {
        let conn = crate::database::lock_conn!(db.conn);
        conn.query_row("SELECT id FROM providers", [], |row| row.get(0))?
    };
    assert_eq!(live_provider, "restore-source");
    Ok(())
}

#[test]
#[serial]
fn restore_rejects_corrupt_db_before_touching_live_database() -> Result<(), AppError> {
    let _test_home = TestHomeGuard::new();
    let db = Database::init()?;
    {
        let conn = crate::database::lock_conn!(db.conn);
        conn.execute("DELETE FROM providers", [])?;
        conn.execute(
            "INSERT INTO providers (id, app_type, name, settings_config, meta)
                 VALUES ('live-provider', 'claude', 'Live Provider', '{}', '{}')",
            [],
        )?;
    }

    let backup_dir = crate::config::get_app_config_dir().join("backups");
    std::fs::create_dir_all(&backup_dir).map_err(|e| AppError::io(&backup_dir, e))?;
    let corrupt_path = backup_dir.join("corrupt.db");
    std::fs::write(&corrupt_path, b"not a sqlite database")
        .map_err(|e| AppError::io(&corrupt_path, e))?;
    let mut backups_before = Database::list_backups()?
        .into_iter()
        .map(|entry| entry.filename)
        .collect::<Vec<_>>();
    backups_before.sort();

    db.restore_from_backup("corrupt.db")
        .expect_err("corrupt backup must be rejected");

    let live_provider: String = {
        let conn = crate::database::lock_conn!(db.conn);
        conn.query_row("SELECT id FROM providers", [], |row| row.get(0))?
    };
    assert_eq!(live_provider, "live-provider");
    let mut backups_after = Database::list_backups()?
        .into_iter()
        .map(|entry| entry.filename)
        .collect::<Vec<_>>();
    backups_after.sort();
    assert_eq!(
        backups_after, backups_before,
        "failed staging must not create a safety backup"
    );
    Ok(())
}

#[test]
#[serial]
fn restore_rejects_future_schema_before_touching_live_database() -> Result<(), AppError> {
    let _test_home = TestHomeGuard::new();
    let db = Database::init()?;

    {
        let conn = crate::database::lock_conn!(db.conn);
        conn.execute("DELETE FROM providers", [])?;
        conn.execute(
            "INSERT INTO providers (id, app_type, name, settings_config, meta)
                 VALUES ('future-source', 'claude', 'Future Source', '{}', '{}')",
            [],
        )?;
    }
    let source_path = db
        .backup_database_file()?
        .expect("file-backed database should create a backup");
    let source_filename = source_path
        .file_name()
        .expect("backup should have a filename")
        .to_string_lossy()
        .into_owned();
    {
        let source_conn = Connection::open(&source_path)?;
        source_conn.execute_batch(&format!(
            "PRAGMA user_version = {};",
            crate::database::SCHEMA_VERSION + 1
        ))?;
    }

    {
        let conn = crate::database::lock_conn!(db.conn);
        conn.execute("DELETE FROM providers", [])?;
        conn.execute(
            "INSERT INTO providers (id, app_type, name, settings_config, meta)
                 VALUES ('live-provider', 'claude', 'Live Provider', '{}', '{}')",
            [],
        )?;
    }

    let backup_dir = crate::config::get_app_config_dir().join("backups");
    let backup_count_before = std::fs::read_dir(&backup_dir)
        .map_err(|e| AppError::io(&backup_dir, e))?
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "db"))
        .count();

    let error = db
        .restore_from_backup(&source_filename)
        .expect_err("future-schema backup must be rejected");
    assert!(
        error.to_string().contains("newer")
            || error.to_string().contains("过新")
            || error.to_string().contains("版本"),
        "unexpected error: {error}"
    );

    let live_provider: String = {
        let conn = crate::database::lock_conn!(db.conn);
        conn.query_row("SELECT id FROM providers", [], |row| row.get(0))?
    };
    assert_eq!(
        live_provider, "live-provider",
        "failed staging validation must not replace the live database"
    );

    let backup_count_after = std::fs::read_dir(&backup_dir)
        .map_err(|e| AppError::io(&backup_dir, e))?
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "db"))
        .count();
    assert_eq!(
        backup_count_after, backup_count_before,
        "staging failure should occur before creating a redundant safety backup"
    );
    Ok(())
}

#[test]
#[serial]
fn startup_defers_cleanup_to_maintenance_even_when_auto_backup_disabled() -> Result<(), AppError> {
    let _test_home = TestHomeGuard::new();

    let settings = AppSettings {
        backup_interval_hours: Some(0),
        ..AppSettings::default()
    };
    update_settings(settings).expect("disable auto backup");

    let db = Database::init()?;
    let now = chrono::Utc::now().timestamp();
    let old_ts = now - 40 * 86400;
    let old_stream_ts = now - 8 * 86400;

    {
        let conn = crate::database::lock_conn!(db.conn);
        conn.execute(
            "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model,
                    input_tokens, output_tokens, total_cost_usd,
                    latency_ms, status_code, created_at
                ) VALUES ('old-req', 'p1', 'claude', 'claude-3', 100, 50, '0.01', 100, 200, ?1)",
            [old_ts],
        )?;
        conn.execute(
            "INSERT INTO stream_check_logs (
                    provider_id, provider_name, app_type, status, success, message,
                    response_time_ms, http_status, model_used, retry_count, tested_at
                ) VALUES ('p1', 'Provider 1', 'claude', 'operational', 1, 'ok', 42, 200, 'claude-3', 0, ?1)",
            [old_stream_ts],
        )?;
    }

    drop(db);
    let db = Database::init()?;
    {
        let conn = crate::database::lock_conn!(db.conn);
        let logs: i64 =
            conn.query_row("SELECT COUNT(*) FROM proxy_request_logs", [], |r| r.get(0))?;
        let stream_logs: i64 =
            conn.query_row("SELECT COUNT(*) FROM stream_check_logs", [], |r| r.get(0))?;
        assert_eq!(
            (logs, stream_logs),
            (1, 1),
            "database opening must defer log cleanup"
        );
    }
    db.periodic_backup_if_needed()?;

    let (remaining_request_logs, stream_logs, rollups): (i64, i64, i64) = {
        let conn = crate::database::lock_conn!(db.conn);
        let remaining_request_logs =
            conn.query_row("SELECT COUNT(*) FROM proxy_request_logs", [], |row| {
                row.get(0)
            })?;
        let stream_logs = conn.query_row("SELECT COUNT(*) FROM stream_check_logs", [], |row| {
            row.get(0)
        })?;
        let rollups = conn.query_row("SELECT COUNT(*) FROM usage_daily_rollups", [], |row| {
            row.get(0)
        })?;
        (remaining_request_logs, stream_logs, rollups)
    };

    assert_eq!(
        remaining_request_logs, 0,
        "old request logs should still be pruned when auto backup is disabled"
    );
    assert_eq!(
        stream_logs, 0,
        "old stream check logs should still be pruned when auto backup is disabled"
    );
    assert_eq!(rollups, 1, "old request logs should be rolled up");

    Ok(())
}

#[test]
#[serial]
fn maintenance_prunes_logs_even_when_backup_destination_is_invalid() -> Result<(), AppError> {
    let _test_home = TestHomeGuard::new();
    update_settings(AppSettings {
        backup_interval_hours: Some(1),
        ..AppSettings::default()
    })?;
    let db = Database::init()?;
    let backup_dir = crate::config::get_app_config_dir().join("backups");
    // Initialization may create migration backups in this isolated home.
    if backup_dir.is_dir() {
        std::fs::remove_dir_all(&backup_dir).map_err(|e| AppError::io(&backup_dir, e))?;
    }
    std::fs::write(&backup_dir, "not a directory").map_err(|e| AppError::io(&backup_dir, e))?;
    {
        let conn = crate::database::lock_conn!(db.conn);
        conn.execute(
            "INSERT INTO proxy_request_logs (request_id, provider_id, app_type, model,
                 input_tokens, output_tokens, total_cost_usd, latency_ms, status_code, created_at)
                 VALUES ('old', 'p1', 'claude', 'test', 10, 5, '0.01', 1, 200, ?1)",
            [chrono::Utc::now().timestamp() - 40 * 86400],
        )?;
    }
    assert!(db.periodic_backup_if_needed().is_err());
    let conn = crate::database::lock_conn!(db.conn);
    let logs: i64 = conn.query_row("SELECT COUNT(*) FROM proxy_request_logs", [], |r| r.get(0))?;
    let rollups: i64 =
        conn.query_row("SELECT COUNT(*) FROM usage_daily_rollups", [], |r| r.get(0))?;
    assert_eq!((logs, rollups), (0, 1));
    Ok(())
}
