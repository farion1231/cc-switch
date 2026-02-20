# Plugin Sync 实现计划

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** 在 cc-switch 中监听 `~/.claude/plugins/installed_plugins.json`，自动同步已安装插件到 SQLite，并在写入 `~/.claude/config.json` 时包含 `enabledPlugins` 字段；同时提供前端 UI 管理每个插件的 enabled/disabled 状态。

**Architecture:** 新增 SQLite 表 `plugin_states` 存储插件状态；新增 `notify` 文件监听服务监听 `installed_plugins.json` 变化；扩展现有 `write_claude_config()` 写入 `enabledPlugins`；新增前端 hook + 组件。

**Tech Stack:** Rust (notify 6.x, rusqlite, tokio), TypeScript (React, TanStack Query, Tauri IPC events)

---

## Task 1: 添加 notify 依赖

**Files:**
- Modify: `src-tauri/Cargo.toml`

**Step 1: 在 [dependencies] 中添加 notify**

在 `src-tauri/Cargo.toml` 的 `[dependencies]` 区块末尾（`uuid` 之后）添加：

```toml
notify = { version = "6", features = ["macos_kqueue"] }
notify-debouncer-mini = "0.4"
```

**Step 2: 验证依赖可以解析**

```bash
cd src-tauri && cargo fetch
```
Expected: 无错误，下载 notify 和 notify-debouncer-mini

**Step 3: Commit**

```bash
git add src-tauri/Cargo.toml src-tauri/Cargo.lock
git commit -m "chore: add notify and notify-debouncer-mini dependencies"
```

---

## Task 2: 添加 plugin_states 数据库表（Schema + Migration）

**Files:**
- Modify: `src-tauri/src/database/mod.rs`（更新 `SCHEMA_VERSION`）
- Modify: `src-tauri/src/database/schema.rs`（新增表定义 + v5→v6 迁移）

**Step 1: 写失败测试（验证新表和迁移）**

在 `src-tauri/src/database/tests.rs` 中添加（或在文件末尾追加）：

```rust
#[test]
fn test_plugin_states_table_exists_after_init() {
    let db = Database::memory().unwrap();
    let conn = db.conn.lock().unwrap();
    assert!(Database::table_exists(&conn, "plugin_states").unwrap());
}

#[test]
fn test_schema_version_is_6() {
    let db = Database::memory().unwrap();
    let conn = db.conn.lock().unwrap();
    let version = Database::get_user_version(&conn).unwrap();
    assert_eq!(version, 6);
}
```

**Step 2: 运行测试，确认失败**

```bash
cd src-tauri && cargo test test_plugin_states_table_exists_after_init test_schema_version_is_6 -- --nocapture
```
Expected: FAILED（表不存在）

**Step 3: 更新 SCHEMA_VERSION**

在 `src-tauri/src/database/mod.rs` 中将：
```rust
pub(crate) const SCHEMA_VERSION: i32 = 5;
```
改为：
```rust
pub(crate) const SCHEMA_VERSION: i32 = 6;
```

**Step 4: 在 create_tables_on_conn 末尾添加表定义**

在 `src-tauri/src/database/schema.rs` 的 `create_tables_on_conn` 函数，在最后的 `Ok(())` 之前添加：

```rust
// 13. Plugin States 表（Claude 插件启用状态）
conn.execute(
    "CREATE TABLE IF NOT EXISTS plugin_states (
        plugin_id    TEXT PRIMARY KEY,
        enabled      BOOLEAN NOT NULL DEFAULT 1,
        install_path TEXT NOT NULL DEFAULT '',
        scope        TEXT NOT NULL DEFAULT 'user',
        version      TEXT,
        created_at   DATETIME DEFAULT (datetime('now')),
        updated_at   DATETIME DEFAULT (datetime('now'))
    )",
    [],
)
.map_err(|e| AppError::Database(e.to_string()))?;
```

**Step 5: 在 apply_schema_migrations_on_conn 中添加 v5→v6 分支**

在 `schema.rs` 的 `apply_schema_migrations_on_conn` 函数的 `while version < SCHEMA_VERSION` 循环中，在 `4 =>` 分支之后添加：

```rust
5 => {
    log::info!("迁移数据库从 v5 到 v6（Claude 插件状态表）");
    Self::migrate_v5_to_v6(conn)?;
    Self::set_user_version(conn, 6)?;
}
```

**Step 6: 添加 migrate_v5_to_v6 方法**

在 `schema.rs` 中 `migrate_v4_to_v5` 方法之后添加：

```rust
/// v5 -> v6 迁移：添加 Claude 插件状态表
fn migrate_v5_to_v6(conn: &Connection) -> Result<(), AppError> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS plugin_states (
            plugin_id    TEXT PRIMARY KEY,
            enabled      BOOLEAN NOT NULL DEFAULT 1,
            install_path TEXT NOT NULL DEFAULT '',
            scope        TEXT NOT NULL DEFAULT 'user',
            version      TEXT,
            created_at   DATETIME DEFAULT (datetime('now')),
            updated_at   DATETIME DEFAULT (datetime('now'))
        )",
        [],
    )
    .map_err(|e| AppError::Database(format!("创建 plugin_states 表失败: {e}")))?;

    log::info!("v5 -> v6 迁移完成：已添加 plugin_states 表");
    Ok(())
}
```

**Step 7: 运行测试，确认通过**

```bash
cd src-tauri && cargo test test_plugin_states_table_exists_after_init test_schema_version_is_6 -- --nocapture
```
Expected: PASSED

**Step 8: Commit**

```bash
git add src-tauri/src/database/
git commit -m "feat: add plugin_states table (schema v5->v6)"
```

---

## Task 3: 添加 DAO（plugin_states 数据访问对象）

**Files:**
- Create: `src-tauri/src/database/dao/plugin_states.rs`
- Modify: `src-tauri/src/database/dao/mod.rs`

**Step 1: 写失败测试**

在 `src-tauri/src/database/tests.rs` 中添加：

```rust
#[test]
fn test_upsert_new_plugin_defaults_enabled() {
    let db = Database::memory().unwrap();
    db.upsert_plugin_state("superpowers@superpowers-marketplace", "/some/path", Some("4.3.0"), "user").unwrap();
    let states = db.get_all_plugin_states().unwrap();
    assert_eq!(states.len(), 1);
    assert!(states[0].enabled);
    assert_eq!(states[0].plugin_id, "superpowers@superpowers-marketplace");
}

#[test]
fn test_upsert_existing_plugin_preserves_enabled_false() {
    let db = Database::memory().unwrap();
    db.upsert_plugin_state("foo@bar", "/path", None, "user").unwrap();
    db.set_plugin_enabled("foo@bar", false).unwrap();
    // Re-upsert (simulating re-install) should NOT reset enabled
    db.upsert_plugin_state("foo@bar", "/path/new", Some("2.0"), "user").unwrap();
    let states = db.get_all_plugin_states().unwrap();
    assert!(!states[0].enabled); // preserved
}

#[test]
fn test_set_plugin_enabled_toggle() {
    let db = Database::memory().unwrap();
    db.upsert_plugin_state("p@r", "/p", None, "user").unwrap();
    db.set_plugin_enabled("p@r", false).unwrap();
    let states = db.get_all_plugin_states().unwrap();
    assert!(!states[0].enabled);
    db.set_plugin_enabled("p@r", true).unwrap();
    let states = db.get_all_plugin_states().unwrap();
    assert!(states[0].enabled);
}

#[test]
fn test_remove_plugin_state() {
    let db = Database::memory().unwrap();
    db.upsert_plugin_state("x@y", "/x", None, "user").unwrap();
    db.remove_plugin_state("x@y").unwrap();
    let states = db.get_all_plugin_states().unwrap();
    assert!(states.is_empty());
}
```

**Step 2: 运行测试，确认失败**

```bash
cd src-tauri && cargo test test_upsert_new_plugin -- --nocapture 2>&1 | head -20
```
Expected: FAILED（方法不存在）

**Step 3: 创建 DAO 文件**

创建 `src-tauri/src/database/dao/plugin_states.rs`：

```rust
//! Claude 插件状态数据访问对象

use crate::database::{lock_conn, Database};
use crate::error::AppError;
use rusqlite::params;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginState {
    pub plugin_id: String,
    pub enabled: bool,
    pub install_path: String,
    pub scope: String,
    pub version: Option<String>,
}

impl Database {
    /// 获取所有插件状态
    pub fn get_all_plugin_states(&self) -> Result<Vec<PluginState>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn
            .prepare(
                "SELECT plugin_id, enabled, install_path, scope, version
                 FROM plugin_states
                 ORDER BY plugin_id ASC",
            )
            .map_err(|e| AppError::Database(e.to_string()))?;

        let iter = stmt
            .query_map([], |row| {
                Ok(PluginState {
                    plugin_id: row.get(0)?,
                    enabled: row.get(1)?,
                    install_path: row.get(2)?,
                    scope: row.get(3)?,
                    version: row.get(4)?,
                })
            })
            .map_err(|e| AppError::Database(e.to_string()))?;

        iter.collect::<Result<Vec<_>, _>>()
            .map_err(|e| AppError::Database(e.to_string()))
    }

    /// 插入或更新插件记录（不覆盖 enabled 状态）
    pub fn upsert_plugin_state(
        &self,
        plugin_id: &str,
        install_path: &str,
        version: Option<&str>,
        scope: &str,
    ) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "INSERT INTO plugin_states (plugin_id, install_path, version, scope, enabled)
             VALUES (?1, ?2, ?3, ?4, 1)
             ON CONFLICT(plugin_id) DO UPDATE SET
                install_path = excluded.install_path,
                version      = excluded.version,
                scope        = excluded.scope,
                updated_at   = datetime('now')",
            params![plugin_id, install_path, version, scope],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    /// 设置插件启用/禁用状态
    pub fn set_plugin_enabled(&self, plugin_id: &str, enabled: bool) -> Result<bool, AppError> {
        let conn = lock_conn!(self.conn);
        let rows = conn
            .execute(
                "UPDATE plugin_states SET enabled = ?1, updated_at = datetime('now')
                 WHERE plugin_id = ?2",
                params![enabled, plugin_id],
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(rows > 0)
    }

    /// 删除插件记录
    pub fn remove_plugin_state(&self, plugin_id: &str) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "DELETE FROM plugin_states WHERE plugin_id = ?1",
            params![plugin_id],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    /// 获取 enabledPlugins map（用于写入 config.json）
    pub fn get_enabled_plugins_map(
        &self,
    ) -> Result<indexmap::IndexMap<String, bool>, AppError> {
        let states = self.get_all_plugin_states()?;
        let map = states
            .into_iter()
            .map(|s| (s.plugin_id, s.enabled))
            .collect();
        Ok(map)
    }
}
```

**Step 4: 在 dao/mod.rs 中注册新模块**

在 `src-tauri/src/database/dao/mod.rs` 中添加：

```rust
pub mod plugin_states;
pub use plugin_states::PluginState;
```

**Step 5: 在 database/mod.rs 中导出 PluginState**

在 `src-tauri/src/database/mod.rs` 的导出行中添加：

```rust
pub use dao::PluginState;
```

**Step 6: 运行测试，确认通过**

```bash
cd src-tauri && cargo test test_upsert_new_plugin test_upsert_existing_plugin test_set_plugin_enabled test_remove_plugin_state -- --nocapture
```
Expected: 4 tests PASSED

**Step 7: Commit**

```bash
git add src-tauri/src/database/
git commit -m "feat: add plugin_states DAO with upsert/toggle/remove"
```

---

## Task 4: 扩展 write_claude_config 写入 enabledPlugins

**Files:**
- Modify: `src-tauri/src/claude_plugin.rs`

**Step 1: 写失败测试**

在 `src-tauri/src/database/tests.rs` 或新建 `src-tauri/src/claude_plugin_tests.rs`，使用 `#[cfg(test)]` 模块。

在 `claude_plugin.rs` 文件末尾添加：

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;
    use std::sync::Arc;
    use tempfile::TempDir;

    fn setup_test_env() -> (TempDir, Arc<Database>) {
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("CC_SWITCH_TEST_HOME", dir.path().to_str().unwrap());
        let db = Arc::new(Database::memory().unwrap());
        (dir, db)
    }

    #[test]
    fn test_write_config_includes_enabled_plugins() {
        let (_dir, db) = setup_test_env();
        db.upsert_plugin_state("p@r", "/p", Some("1.0"), "user").unwrap();
        write_claude_config_with_db(&db).unwrap();
        let content = read_claude_config().unwrap().unwrap();
        let val: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(val["primaryApiKey"], "any");
        assert_eq!(val["enabledPlugins"]["p@r"], true);
    }

    #[test]
    fn test_write_config_no_plugins_writes_empty_object() {
        let (_dir, db) = setup_test_env();
        write_claude_config_with_db(&db).unwrap();
        let content = read_claude_config().unwrap().unwrap();
        let val: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(val["enabledPlugins"], serde_json::json!({}));
    }

    #[test]
    fn test_write_config_preserves_other_fields() {
        let (_dir, db) = setup_test_env();
        // 先写入一个有其他字段的 config
        let path = claude_config_path().unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, r#"{"skipDangerousModePermissionPrompt": true}"#).unwrap();
        write_claude_config_with_db(&db).unwrap();
        let content = read_claude_config().unwrap().unwrap();
        let val: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(val["skipDangerousModePermissionPrompt"], true);
        assert_eq!(val["primaryApiKey"], "any");
    }
}
```

**Step 2: 运行测试，确认失败**

```bash
cd src-tauri && cargo test --lib claude_plugin::tests -- --nocapture 2>&1 | head -20
```
Expected: FAILED（`write_claude_config_with_db` 不存在）

**Step 3: 实现 write_claude_config_with_db**

在 `claude_plugin.rs` 中添加新函数，并修改 `write_claude_config` 调用它：

```rust
use crate::database::Database;
use std::sync::Arc;

/// 写入 config.json（包含 enabledPlugins，从数据库读取插件状态）
pub fn write_claude_config_with_db(db: &Arc<Database>) -> Result<bool, AppError> {
    let path = claude_config_path()?;
    ensure_claude_dir_exists()?;

    let mut obj = match read_claude_config()? {
        Some(existing) => match serde_json::from_str::<serde_json::Value>(&existing) {
            Ok(serde_json::Value::Object(map)) => serde_json::Value::Object(map),
            _ => serde_json::json!({}),
        },
        None => serde_json::json!({}),
    };

    let map = obj.as_object_mut().unwrap();

    // 写入 primaryApiKey
    map.insert(
        "primaryApiKey".to_string(),
        serde_json::Value::String("any".to_string()),
    );

    // 写入 enabledPlugins
    let plugins_map = db.get_enabled_plugins_map().unwrap_or_default();
    let plugins_json: serde_json::Map<String, serde_json::Value> = plugins_map
        .into_iter()
        .map(|(k, v)| (k, serde_json::Value::Bool(v)))
        .collect();
    map.insert(
        "enabledPlugins".to_string(),
        serde_json::Value::Object(plugins_json),
    );

    let serialized = serde_json::to_string_pretty(&obj)
        .map_err(|e| AppError::JsonSerialize { source: e })?;
    fs::write(&path, format!("{serialized}\n")).map_err(|e| AppError::io(&path, e))?;
    Ok(true)
}
```

同时修改原来的 `write_claude_config`（不含数据库版本，保持向后兼容）：
- 在 `is_claude_config_applied` 等位置，保留旧函数不变，作为无 DB 场景的fallback

**Step 4: 运行测试，确认通过**

```bash
cd src-tauri && cargo test --lib claude_plugin::tests -- --nocapture
```
Expected: 3 tests PASSED

**Step 5: Commit**

```bash
git add src-tauri/src/claude_plugin.rs
git commit -m "feat: extend write_claude_config to include enabledPlugins from db"
```

---

## Task 5: 实现 PluginWatcher 服务

**Files:**
- Create: `src-tauri/src/services/plugin_watcher.rs`
- Modify: `src-tauri/src/services/mod.rs`
- Modify: `src-tauri/src/lib.rs`（注册启动）

**Step 1: 写失败测试（同步逻辑单元测试）**

在 `plugin_watcher.rs` 末尾的 `#[cfg(test)]` 块中：

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;
    use std::sync::Arc;

    #[test]
    fn test_sync_new_plugin_added_to_db() {
        let db = Arc::new(Database::memory().unwrap());
        let json = r#"{
            "version": 2,
            "plugins": {
                "foo@bar": [{"scope":"user","installPath":"/tmp/foo","version":"1.0","installedAt":"2026-01-01T00:00:00Z","lastUpdated":"2026-01-01T00:00:00Z","gitCommitSha":"abc"}]
            }
        }"#;
        sync_plugins_from_json(json, &db).unwrap();
        let states = db.get_all_plugin_states().unwrap();
        assert_eq!(states.len(), 1);
        assert_eq!(states[0].plugin_id, "foo@bar");
        assert!(states[0].enabled);
    }

    #[test]
    fn test_sync_removed_plugin_deleted_from_db() {
        let db = Arc::new(Database::memory().unwrap());
        db.upsert_plugin_state("old@reg", "/old", None, "user").unwrap();
        // 同步一个不含 old@reg 的列表
        let json = r#"{"version":2,"plugins":{}}"#;
        sync_plugins_from_json(json, &db).unwrap();
        let states = db.get_all_plugin_states().unwrap();
        assert!(states.is_empty());
    }

    #[test]
    fn test_sync_invalid_json_returns_error() {
        let db = Arc::new(Database::memory().unwrap());
        let result = sync_plugins_from_json("not json", &db);
        assert!(result.is_err());
    }

    #[test]
    fn test_sync_missing_file_returns_ok_empty() {
        let db = Arc::new(Database::memory().unwrap());
        let result = sync_plugins_from_file_path(
            std::path::Path::new("/nonexistent/path/installed_plugins.json"),
            &db,
        );
        assert!(result.is_ok()); // 静默跳过
    }
}
```

**Step 2: 运行测试，确认失败**

```bash
cd src-tauri && cargo test --lib services::plugin_watcher -- --nocapture 2>&1 | head -20
```
Expected: FAILED（模块不存在）

**Step 3: 创建 plugin_watcher.rs**

创建 `src-tauri/src/services/plugin_watcher.rs`：

```rust
//! Claude 插件文件监听服务
//!
//! 监听 `~/.claude/plugins/installed_plugins.json` 的变化，
//! 自动同步到 SQLite plugin_states 表。

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use notify_debouncer_mini::{new_debouncer, notify::RecursiveMode, DebounceEventResult};
use serde::Deserialize;
use tauri::{AppHandle, Emitter};

use crate::database::Database;
use crate::error::AppError;

/// installed_plugins.json 的数据结构
#[derive(Debug, Deserialize)]
struct InstalledPlugins {
    plugins: std::collections::HashMap<String, Vec<InstalledPluginEntry>>,
}

#[derive(Debug, Deserialize)]
struct InstalledPluginEntry {
    scope: String,
    #[serde(rename = "installPath")]
    install_path: String,
    version: String,
}

/// 从 JSON 字符串同步插件到数据库（纯函数，便于测试）
pub fn sync_plugins_from_json(json: &str, db: &Arc<Database>) -> Result<(), AppError> {
    let installed: InstalledPlugins = serde_json::from_str(json)
        .map_err(|e| AppError::Config(format!("解析 installed_plugins.json 失败: {e}")))?;

    // 取当前 DB 中的插件 ID 集合
    let existing_ids: std::collections::HashSet<String> = db
        .get_all_plugin_states()?
        .into_iter()
        .map(|s| s.plugin_id)
        .collect();

    // 新插件集合
    let mut new_ids = std::collections::HashSet::new();

    for (plugin_id, entries) in &installed.plugins {
        if let Some(entry) = entries.first() {
            new_ids.insert(plugin_id.clone());
            db.upsert_plugin_state(
                plugin_id,
                &entry.install_path,
                Some(&entry.version),
                &entry.scope,
            )?;
        }
    }

    // 删除已卸载的插件
    for old_id in existing_ids.difference(&new_ids) {
        db.remove_plugin_state(old_id)?;
    }

    Ok(())
}

/// 从文件路径同步（不存在时静默返回 Ok）
pub fn sync_plugins_from_file_path(path: &Path, db: &Arc<Database>) -> Result<(), AppError> {
    if !path.exists() {
        return Ok(());
    }

    let content = std::fs::read_to_string(path)
        .map_err(|e| AppError::io(path, e))?;

    sync_plugins_from_json(&content, db)
}

/// 启动文件监听服务（在后台 tokio 任务中运行）
pub fn start_watcher(db: Arc<Database>, app_handle: AppHandle) {
    let installed_plugins_path = crate::config::get_claude_config_dir()
        .join("plugins")
        .join("installed_plugins.json");

    // 启动时先同步一次
    if let Err(e) = sync_plugins_from_file_path(&installed_plugins_path, &db) {
        log::warn!("插件初始同步失败: {e}");
    }

    let watch_path = installed_plugins_path.clone();

    tauri::async_runtime::spawn_blocking(move || {
        let (tx, rx) = std::sync::mpsc::channel();

        let mut debouncer = match new_debouncer(Duration::from_millis(500), tx) {
            Ok(d) => d,
            Err(e) => {
                log::warn!("创建插件文件监听器失败（降级为启动时同步）: {e}");
                return;
            }
        };

        // 监听父目录（文件可能还不存在）
        let watch_dir = watch_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf();

        if let Err(e) = debouncer
            .watcher()
            .watch(&watch_dir, RecursiveMode::NonRecursive)
        {
            log::warn!("监听插件目录失败（降级为启动时同步）: {e}");
            return;
        }

        log::info!("✓ 插件文件监听已启动: {}", watch_dir.display());

        for result in rx {
            match result {
                Ok(events) => {
                    let relevant = events.iter().any(|e| {
                        e.path
                            .file_name()
                            .map(|f| f == "installed_plugins.json")
                            .unwrap_or(false)
                    });

                    if relevant {
                        log::debug!("检测到 installed_plugins.json 变化，开始同步...");
                        if let Err(e) = sync_plugins_from_file_path(&watch_path, &db) {
                            log::warn!("同步插件失败: {e}");
                        } else {
                            log::info!("✓ 插件同步完成");
                        }
                        // 通知前端刷新
                        if let Err(e) = app_handle.emit("plugins://changed", ()) {
                            log::warn!("发送插件变化事件失败: {e}");
                        }
                    }
                }
                Err(DebounceEventResult::Error(errors)) => {
                    for e in errors {
                        log::warn!("插件文件监听错误: {e}");
                    }
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    // （上面 Step 1 的测试代码放这里）
    use super::*;
    use crate::database::Database;
    use std::sync::Arc;

    #[test]
    fn test_sync_new_plugin_added_to_db() {
        let db = Arc::new(Database::memory().unwrap());
        let json = r#"{
            "version": 2,
            "plugins": {
                "foo@bar": [{"scope":"user","installPath":"/tmp/foo","version":"1.0","installedAt":"2026-01-01T00:00:00Z","lastUpdated":"2026-01-01T00:00:00Z","gitCommitSha":"abc"}]
            }
        }"#;
        sync_plugins_from_json(json, &db).unwrap();
        let states = db.get_all_plugin_states().unwrap();
        assert_eq!(states.len(), 1);
        assert_eq!(states[0].plugin_id, "foo@bar");
        assert!(states[0].enabled);
    }

    #[test]
    fn test_sync_removed_plugin_deleted_from_db() {
        let db = Arc::new(Database::memory().unwrap());
        db.upsert_plugin_state("old@reg", "/old", None, "user").unwrap();
        let json = r#"{"version":2,"plugins":{}}"#;
        sync_plugins_from_json(json, &db).unwrap();
        let states = db.get_all_plugin_states().unwrap();
        assert!(states.is_empty());
    }

    #[test]
    fn test_sync_invalid_json_returns_error() {
        let db = Arc::new(Database::memory().unwrap());
        let result = sync_plugins_from_json("not json", &db);
        assert!(result.is_err());
    }

    #[test]
    fn test_sync_missing_file_returns_ok() {
        let db = Arc::new(Database::memory().unwrap());
        let result = sync_plugins_from_file_path(
            Path::new("/nonexistent/path/installed_plugins.json"),
            &db,
        );
        assert!(result.is_ok());
    }
}
```

**Step 4: 在 services/mod.rs 中注册模块**

在 `src-tauri/src/services/mod.rs` 中添加：

```rust
pub mod plugin_watcher;
```

**Step 5: 运行测试，确认通过**

```bash
cd src-tauri && cargo test --lib services::plugin_watcher::tests -- --nocapture
```
Expected: 4 tests PASSED

**Step 6: 在 lib.rs setup 中启动监听服务**

在 `lib.rs` 的 `setup` 函数中，在 `webdav_auto_sync::start_worker(...)` 调用之后添加：

```rust
// 启动 Claude 插件文件监听服务
crate::services::plugin_watcher::start_watcher(
    app_state.db.clone(),
    app.handle().clone(),
);
```

注意：这行要在 `app.manage(app_state);` 之前，即在 `app_state` 还有所有权时调用 `app_state.db.clone()`。

实际位置：在 `crate::services::webdav_auto_sync::start_worker(...)` 之后、`app.manage(app_state);` 之前。

**Step 7: 编译验证**

```bash
cd src-tauri && cargo build 2>&1 | tail -5
```
Expected: 编译成功，无错误

**Step 8: Commit**

```bash
git add src-tauri/src/services/ src-tauri/src/lib.rs
git commit -m "feat: add PluginWatcher service with notify file watching"
```

---

## Task 6: 更新 Tauri 命令（plugin.rs）

**Files:**
- Modify: `src-tauri/src/commands/plugin.rs`

**Step 1: 写失败测试**

由于命令需要 Tauri State，此处验证在 `cargo check` 层面。先添加命令代码。

**Step 2: 在 commands/plugin.rs 中添加新命令**

在文件末尾添加：

```rust
/// 获取所有插件列表及启用状态
#[tauri::command]
pub async fn list_plugins(
    state: tauri::State<'_, crate::store::AppState>,
) -> Result<Vec<crate::database::PluginState>, String> {
    state.db.get_all_plugin_states().map_err(|e| e.to_string())
}

/// 设置插件启用/禁用状态，并重写 config.json
#[tauri::command]
pub async fn set_plugin_enabled(
    plugin_id: String,
    enabled: bool,
    state: tauri::State<'_, crate::store::AppState>,
) -> Result<bool, String> {
    state
        .db
        .set_plugin_enabled(&plugin_id, enabled)
        .map_err(|e| e.to_string())?;

    // 检查是否已开启 Claude 插件集成
    let settings = crate::settings::get_settings();
    if settings.enable_claude_plugin_integration {
        crate::claude_plugin::write_claude_config_with_db(&state.db)
            .map_err(|e| e.to_string())?;
    }

    Ok(true)
}
```

**Step 3: 注册新命令到 Tauri**

在 `lib.rs` 的 `.invoke_handler(tauri::generate_handler![...])` 中添加：

搜索现有命令注册（文件中有 `invoke_handler`），在其中添加：
```rust
list_plugins,
set_plugin_enabled,
```

**Step 4: 编译验证**

```bash
cd src-tauri && cargo check 2>&1 | grep -E "^error" | head -10
```
Expected: 无 error

**Step 5: Commit**

```bash
git add src-tauri/src/commands/plugin.rs src-tauri/src/lib.rs
git commit -m "feat: add list_plugins and set_plugin_enabled Tauri commands"
```

---

## Task 7: 更新 apply_claude_plugin_config 使用数据库版本

**Files:**
- Modify: `src-tauri/src/commands/plugin.rs`

**Step 1: 修改 apply_claude_plugin_config 命令**

将 `apply_claude_plugin_config` 命令更新为调用数据库版本：

```rust
#[tauri::command]
pub async fn apply_claude_plugin_config(
    official: bool,
    state: tauri::State<'_, crate::store::AppState>,
) -> Result<bool, String> {
    if official {
        crate::claude_plugin::clear_claude_config().map_err(|e| e.to_string())
    } else {
        crate::claude_plugin::write_claude_config_with_db(&state.db)
            .map_err(|e| e.to_string())
    }
}
```

**Step 2: 编译验证**

```bash
cd src-tauri && cargo check 2>&1 | grep -E "^error" | head -10
```
Expected: 无 error

**Step 3: Commit**

```bash
git add src-tauri/src/commands/plugin.rs
git commit -m "feat: apply_claude_plugin_config now writes enabledPlugins from db"
```

---

## Task 8: 前端 API 封装

**Files:**
- Create: `src/lib/api/plugins.ts`
- Modify: `src/lib/api/index.ts`（导出新 API）

**Step 1: 创建 plugins.ts**

```typescript
import { invoke } from "@tauri-apps/api/core";

export interface PluginState {
  plugin_id: string;
  enabled: boolean;
  install_path: string;
  scope: string;
  version: string | null;
}

export const pluginsApi = {
  async list(): Promise<PluginState[]> {
    return await invoke("list_plugins");
  },

  async setEnabled(pluginId: string, enabled: boolean): Promise<boolean> {
    return await invoke("set_plugin_enabled", { pluginId, enabled });
  },
};
```

**Step 2: 在 src/lib/api/index.ts 中导出**

在现有导出中添加：
```typescript
export { pluginsApi } from "./plugins";
export type { PluginState } from "./plugins";
```

**Step 3: Commit**

```bash
git add src/lib/api/
git commit -m "feat: add pluginsApi frontend wrapper"
```

---

## Task 9: 前端 Hook（usePlugins）

**Files:**
- Create: `src/hooks/usePlugins.ts`

**Step 1: 写失败测试**

创建 `tests/hooks/usePlugins.test.tsx`：

```typescript
import { renderHook, act } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { describe, it, expect, vi, beforeEach } from "vitest";
import type { ReactNode } from "react";
import { usePluginList, useSetPluginEnabled } from "@/hooks/usePlugins";

const listMock = vi.fn();
const setEnabledMock = vi.fn();

vi.mock("@/lib/api", () => ({
  pluginsApi: {
    list: (...args: unknown[]) => listMock(...args),
    setEnabled: (...args: unknown[]) => setEnabledMock(...args),
  },
}));

function wrapper({ children }: { children: ReactNode }) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return <QueryClientProvider client={qc}>{children}</QueryClientProvider>;
}

describe("usePluginList", () => {
  beforeEach(() => {
    listMock.mockResolvedValue([
      { plugin_id: "foo@bar", enabled: true, install_path: "/p", scope: "user", version: "1.0" },
    ]);
  });

  it("returns plugin list", async () => {
    const { result } = renderHook(() => usePluginList(), { wrapper });
    await act(async () => {});
    expect(result.current.data).toHaveLength(1);
    expect(result.current.data?.[0].plugin_id).toBe("foo@bar");
  });
});

describe("useSetPluginEnabled", () => {
  it("calls api.setEnabled", async () => {
    setEnabledMock.mockResolvedValue(true);
    const { result } = renderHook(() => useSetPluginEnabled(), { wrapper });
    await act(async () => {
      await result.current.mutateAsync({ pluginId: "foo@bar", enabled: false });
    });
    expect(setEnabledMock).toHaveBeenCalledWith("foo@bar", false);
  });
});
```

**Step 2: 运行测试，确认失败**

```bash
pnpm test:unit -- tests/hooks/usePlugins.test.tsx 2>&1 | tail -10
```
Expected: FAILED（模块不存在）

**Step 3: 创建 usePlugins.ts**

创建 `src/hooks/usePlugins.ts`：

```typescript
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import { pluginsApi } from "@/lib/api";

export const PLUGINS_QUERY_KEY = ["plugins"] as const;

export function usePluginList() {
  const queryClient = useQueryClient();

  useEffect(() => {
    const unlisten = listen("plugins://changed", () => {
      queryClient.invalidateQueries({ queryKey: PLUGINS_QUERY_KEY });
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, [queryClient]);

  return useQuery({
    queryKey: PLUGINS_QUERY_KEY,
    queryFn: () => pluginsApi.list(),
  });
}

export function useSetPluginEnabled() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: ({ pluginId, enabled }: { pluginId: string; enabled: boolean }) =>
      pluginsApi.setEnabled(pluginId, enabled),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: PLUGINS_QUERY_KEY });
    },
  });
}
```

**Step 4: 运行测试，确认通过**

```bash
pnpm test:unit -- tests/hooks/usePlugins.test.tsx
```
Expected: PASSED

**Step 5: Commit**

```bash
git add src/hooks/usePlugins.ts tests/hooks/usePlugins.test.tsx
git commit -m "feat: add usePluginList and useSetPluginEnabled hooks with tests"
```

---

## Task 10: 前端组件（PluginList）

**Files:**
- Create: `src/components/plugins/PluginList.tsx`

**Step 1: 创建组件**

创建 `src/components/plugins/PluginList.tsx`：

```tsx
import { useTranslation } from "react-i18next";
import { Switch } from "@/components/ui/switch";
import { usePluginList, useSetPluginEnabled } from "@/hooks/usePlugins";
import type { PluginState } from "@/lib/api";

function PluginRow({ plugin }: { plugin: PluginState }) {
  const { mutate: setEnabled, isPending } = useSetPluginEnabled();
  const [name, registry] = plugin.plugin_id.split("@");

  return (
    <div className="flex items-center justify-between py-3 px-4 border-b last:border-b-0">
      <div className="flex flex-col gap-0.5">
        <span className="font-medium text-sm">{name}</span>
        <span className="text-xs text-muted-foreground">
          {registry} · {plugin.version ?? "unknown"}
        </span>
      </div>
      <Switch
        checked={plugin.enabled}
        disabled={isPending}
        onCheckedChange={(enabled) =>
          setEnabled({ pluginId: plugin.plugin_id, enabled })
        }
      />
    </div>
  );
}

export function PluginList() {
  const { t } = useTranslation();
  const { data: plugins = [], isLoading } = usePluginList();

  if (isLoading) return null;

  if (plugins.length === 0) {
    return (
      <div className="text-sm text-muted-foreground text-center py-4">
        {t("plugins.noPluginsInstalled", {
          defaultValue: "未检测到已安装插件",
        })}
      </div>
    );
  }

  return (
    <div className="rounded-md border">
      <div className="px-4 py-2 border-b bg-muted/50">
        <span className="text-xs font-medium text-muted-foreground">
          {t("plugins.title", { defaultValue: "Claude 插件" })} ({plugins.length})
        </span>
      </div>
      {plugins.map((plugin) => (
        <PluginRow key={plugin.plugin_id} plugin={plugin} />
      ))}
    </div>
  );
}
```

**Step 2: 编译验证**

```bash
pnpm typecheck 2>&1 | grep -E "error TS" | head -10
```
Expected: 无 TS 错误

**Step 3: Commit**

```bash
git add src/components/plugins/
git commit -m "feat: add PluginList component with enable/disable toggles"
```

---

## Task 11: 集成 PluginList 到设置界面

**Files:**
- 找到 Claude 插件集成相关的设置组件并添加 PluginList

**Step 1: 找到插件集成设置的位置**

搜索 `enableClaudePluginIntegration` 在前端的显示位置：

```bash
grep -r "enableClaudePluginIntegration\|ClaudePlugin\|claude_plugin" src/components/ --include="*.tsx" -l
```

**Step 2: 在合适位置添加 PluginList**

在找到的设置组件中，在 Claude 插件集成开关下方（仅当 `enableClaudePluginIntegration=true` 时显示）添加：

```tsx
import { PluginList } from "@/components/plugins/PluginList";

// 在 enableClaudePluginIntegration 开关下方：
{settings.enableClaudePluginIntegration && (
  <div className="mt-3">
    <PluginList />
  </div>
)}
```

**Step 3: 编译验证**

```bash
pnpm typecheck 2>&1 | grep -E "error TS" | head -10
```
Expected: 无 TS 错误

**Step 4: Commit**

```bash
git add src/components/
git commit -m "feat: integrate PluginList into Claude plugin settings section"
```

---

## Task 12: 运行全部测试

**Step 1: 运行前端所有单元测试**

```bash
pnpm test:unit
```
Expected: ALL PASSED

**Step 2: 运行 Rust 测试**

```bash
cd src-tauri && cargo test 2>&1 | tail -20
```
Expected: ALL PASSED（含新增的 plugin_states、plugin_watcher、claude_plugin 测试）

**Step 3: 类型检查**

```bash
pnpm typecheck
```
Expected: 无错误

**Step 4: 格式检查**

```bash
pnpm format:check && cd src-tauri && cargo fmt --check && cargo clippy -- -D warnings
```
Expected: 无格式和 lint 错误

**Step 5: Final Commit**

```bash
git add -A
git commit -m "test: verify all plugin sync tests pass"
```

---

## 验收清单

- [ ] 安装新插件后 `config.json` 自动出现对应条目（`enabled: true`）
- [ ] 卸载插件后 `config.json` 对应条目消失
- [ ] 在 cc-switch UI 关闭插件后 `config.json` 变为 `false`
- [ ] 切换 provider 后 `enabledPlugins` 状态保持不变
- [ ] `enableClaudePluginIntegration=false` 时不写 `enabledPlugins`
- [ ] `installed_plugins.json` 不存在时应用正常启动无报错
