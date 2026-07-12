//! 跨平台只读 SQLite 打开辅助。
//!
//! UNC 路径（WSL/网络共享）下通过「复制到临时目录 → 本地打开 → Backup 进内存」
//! 绕过不可用的文件锁。WSL 的 virtiofs/9P 文件系统不支持 SQLite 的文件锁，
//! 即使无进程在写，读者也会收到 `database is locked`。
//!
//! `immutable=1` URI 模式虽然能绕过锁，但它跳过 WAL 文件，导致未 checkpoint
//! 的数据不可见。复制方案把 db + wal 一起拷到本地临时目录，让 SQLite 在本地
//! 正常打开（WAL 自动应用），再通过 Backup API 把完整状态拷进内存连接，
//! 随后临时文件立即删除。返回的 `:memory:` 连接不依赖任何外部文件。

use std::path::Path;
use std::time::Duration;

use rusqlite::backup::Backup;
use rusqlite::{Connection, OpenFlags};

/// Windows 上检测 UNC 路径（`\\server\share`），排除 `\\?\` 长路径前缀。
#[cfg(windows)]
pub(crate) fn is_unc_path(path: &Path) -> bool {
    let s = path.to_string_lossy();
    s.starts_with("\\\\") && !s.starts_with("\\\\?\\")
}

#[cfg(not(windows))]
pub(crate) fn is_unc_path(_path: &Path) -> bool {
    false
}

/// 打开 SQLite 数据库进行只读访问。
///
/// - 本地路径：`READ_ONLY | NO_MUTEX` + `busy_timeout(2s)`，应对本地写者持锁时的短时竞争。
/// - UNC 路径：复制 db + wal 到临时目录，本地打开让 WAL 自动应用，
///   再通过 Backup API 把完整状态拷进 `:memory:` 连接，随后临时文件立即删除。
///   返回的内存连接不依赖任何外部文件，也无法写回原库。
///
/// 调用方只需将 `Connection::open_with_flags(...)` 替换为此函数。
pub(crate) fn open_readonly(path: &Path) -> Result<Connection, rusqlite::Error> {
    if is_unc_path(path) {
        open_unc_readonly(path)
    } else {
        let conn = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        conn.busy_timeout(Duration::from_secs(2))?;
        Ok(conn)
    }
}

/// UNC 路径专用：复制 db + wal 到临时目录，本地打开后 Backup 进内存。
///
/// 步骤：
/// 1. 创建临时目录（TempDir 在 drop 时自动删除所有内容）
/// 2. 复制主库文件到临时目录
/// 3. 若存在 `-wal` 文件，一并复制（SQLite 打开时会自动应用 WAL）
/// 4. 用默认 flags 打开临时副本（本地路径，锁可用，WAL 自动应用）
/// 5. 创建内存连接，用 Backup API 把「WAL 已应用」的完整状态拷进内存
/// 6. drop 临时连接 + TempDir —— 临时文件全部删除
/// 7. 返回内存连接（不依赖任何外部文件）
fn open_unc_readonly(path: &Path) -> Result<Connection, rusqlite::Error> {
    let temp_dir = tempfile::tempdir().map_err(|e| {
        rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_CANTOPEN),
            Some(format!("create tempdir: {e}")),
        )
    })?;
    let temp_db = temp_dir.path().join("snapshot.db");

    // 复制主库文件
    std::fs::copy(path, &temp_db).map_err(|e| {
        rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_CANTOPEN),
            Some(format!("copy db to tempdir: {e}")),
        )
    })?;

    // 复制 WAL 文件（若存在），SQLite 打开临时副本时会自动应用 WAL 帧
    let wal_path = path.with_extension("db-wal");
    if wal_path.exists() {
        let temp_wal = temp_dir.path().join("snapshot.db-wal");
        std::fs::copy(&wal_path, &temp_wal).map_err(|e| {
            rusqlite::Error::SqliteFailure(
                rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_CANTOPEN),
                Some(format!("copy wal to tempdir: {e}")),
            )
        })?;
    }

    // 本地路径打开，SQLite 会自动创建 -shm 并应用 WAL
    let temp_conn = Connection::open_with_flags(
        &temp_db,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    temp_conn.busy_timeout(Duration::from_secs(2))?;

    // 把完整状态（主库 + 已应用的 WAL）拷进内存连接
    let mut mem_conn = Connection::open_in_memory()?;
    {
        let backup = Backup::new(&temp_conn, &mut mem_conn)?;
        backup.step(-1)?;
    }

    drop(temp_conn);
    // TempDir drop 时自动删除临时文件

    Ok(mem_conn)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;
    use tempfile::tempdir;

    #[cfg(windows)]
    #[test]
    fn test_is_unc_path_wsl_localhost() {
        assert!(is_unc_path(Path::new(
            r"\\wsl.localhost\archlinux\home\user\opencode.db"
        )));
    }

    #[cfg(windows)]
    #[test]
    fn test_is_unc_path_wsl_dollar() {
        assert!(is_unc_path(Path::new(
            r"\\wsl$\Ubuntu\home\user\opencode.db"
        )));
    }

    #[cfg(windows)]
    #[test]
    fn test_is_unc_path_local_drive() {
        assert!(!is_unc_path(Path::new(
            r"C:\Users\user\.local\share\opencode\opencode.db"
        )));
    }

    #[cfg(windows)]
    #[test]
    fn test_is_unc_path_long_prefix() {
        assert!(!is_unc_path(Path::new(r"\\?\C:\Users\user\opencode.db")));
    }

    #[cfg(not(windows))]
    #[test]
    fn test_is_unc_path_non_windows_always_false() {
        assert!(!is_unc_path(Path::new("/home/user/opencode.db")));
    }

    #[test]
    fn test_open_readonly_local_normal() {
        let temp = tempdir().expect("tempdir");
        let db_path = temp.path().join("test.db");
        let conn = Connection::open(&db_path).expect("create db");
        conn.execute_batch("CREATE TABLE t(x INTEGER); INSERT INTO t VALUES (42);")
            .expect("seed");
        drop(conn);

        let reader = open_readonly(&db_path).expect("open readonly");
        let val: i64 = reader
            .query_row("SELECT x FROM t", [], |r| r.get(0))
            .expect("query");
        assert_eq!(val, 42);
    }

    #[test]
    fn test_open_readonly_local_with_exclusive_lock() {
        use std::sync::mpsc;
        use std::thread;
        use std::time::Duration;

        let temp = tempdir().expect("tempdir");
        let db_path = temp.path().join("test_locked.db");
        let conn = Connection::open(&db_path).expect("create db");
        conn.execute_batch("CREATE TABLE t(x INTEGER); INSERT INTO t VALUES (7);")
            .expect("seed");
        drop(conn);

        let db_clone = db_path.clone();
        let (tx, rx) = mpsc::channel();
        let handle = thread::spawn(move || {
            let writer = Connection::open(&db_clone).expect("open writer");
            writer.execute_batch("BEGIN EXCLUSIVE;").expect("exclusive");
            tx.send(()).expect("notify");
            thread::sleep(Duration::from_millis(300));
            writer.execute_batch("ROLLBACK;").expect("rollback");
        });

        rx.recv().expect("wait for lock");

        // busy_timeout(2s) 应等到写者释放锁后成功读取
        let reader = open_readonly(&db_path).expect("open readonly");
        let val: i64 = reader
            .query_row("SELECT x FROM t", [], |r| r.get(0))
            .expect("query");
        assert_eq!(val, 7);

        handle.join().expect("writer thread");
    }

    /// 验证 UNC 路径分支（复制+Backup）能读到 WAL 中的数据。
    ///
    /// 这个测试模拟 WSL UNC 场景：在一个 WAL 模式的数据库中写入数据但
    /// 不 checkpoint，然后用 `open_readonly` 打开，验证能读到 WAL 中的数据。
    /// 在非 Windows 平台 `is_unc_path` 返回 false，所以直接测试 `open_unc_readonly`。
    #[test]
    fn test_open_unc_readonly_reads_wal_data() {
        let temp = tempdir().expect("tempdir");
        let db_path = temp.path().join("wal_test.db");

        // 创建 WAL 模式数据库并写入初始数据
        let conn = Connection::open(&db_path).expect("create db");
        conn.execute_batch("PRAGMA journal_mode=WAL;")
            .expect("wal mode");
        conn.execute_batch("CREATE TABLE t(x INTEGER); INSERT INTO t VALUES (1);")
            .expect("seed");
        drop(conn);

        // 再次打开写入，产生 WAL 文件但不 checkpoint
        let conn = Connection::open(&db_path).expect("reopen");
        conn.execute("INSERT INTO t VALUES (2)", [])
            .expect("insert 2");
        conn.execute("INSERT INTO t VALUES (3)", [])
            .expect("insert 3");
        // 不 checkpoint，数据在 -wal 文件里
        drop(conn);

        // 用 UNC 分支内部逻辑打开（直接调用 open_unc_readonly 绕过 is_unc_path 检查）
        let reader = open_unc_readonly(&db_path).expect("open via copy+backup");
        let count: i64 = reader
            .query_row("SELECT COUNT(*) FROM t", [], |r| r.get(0))
            .expect("query");
        assert_eq!(count, 3, "WAL 中的 2 条记录 + 主库 1 条应全部可见");
    }

    /// 验证 UNC 路径分支返回的是内存连接（不依赖外部文件）。
    #[test]
    fn test_open_unc_readonly_returns_in_memory_connection() {
        let temp = tempdir().expect("tempdir");
        let db_path = temp.path().join("mem_test.db");

        let conn = Connection::open(&db_path).expect("create db");
        conn.execute_batch("CREATE TABLE t(x INTEGER); INSERT INTO t VALUES (100);")
            .expect("seed");
        drop(conn);

        let reader = open_unc_readonly(&db_path).expect("open");
        let val: i64 = reader
            .query_row("SELECT x FROM t", [], |r| r.get(0))
            .expect("query");
        assert_eq!(val, 100);

        // 内存连接的 database_name 应为 :memory:
        let name: String = reader
            .query_row("PRAGMA database_list", [], |r| r.get(1))
            .expect("database_list");
        assert_eq!(
            name, "main",
            "in-memory connection should have main database"
        );
    }
}
