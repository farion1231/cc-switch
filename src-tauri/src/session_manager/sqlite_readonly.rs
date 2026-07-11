//! 跨平台只读 SQLite 打开辅助。
//!
//! UNC 路径（WSL/网络共享）下自动使用 immutable 模式绕过锁机制。
//! WSL 的 virtiofs/9P 文件系统不支持 SQLite 的文件锁，导致即使无进程在写，
//! 读者也会收到 `database is locked`。`immutable=1` 让 SQLite 完全跳过锁
//! 和 WAL/SHM 机制，以只读快照方式直接读主库文件。

use std::path::Path;
use std::time::Duration;

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

/// 对 `file:` URI 的 path 部分做百分号编码。
///
/// SQLite URI 解析器要求对空格、`%`、`#`、`?` 及非 ASCII 字符编码。
/// `/`（路径分隔符）不编码，`:`（盘符分隔符）也不编码。
fn percent_encode_for_uri(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '/' | ':' | '-' | '_' | '.' | '~' | '$' | '!' | '&' | '\'' | '(' | ')' | '*' | '+'
            | ',' | ';' | '=' => out.push(c),
            c if c.is_ascii_alphanumeric() => out.push(c),
            // 空格、%、#、? 及所有非 ASCII 字符
            _ => {
                let bytes = c.to_string();
                for b in bytes.bytes() {
                    out.push_str(&format!("%{:02X}", b));
                }
            }
        }
    }
    out
}

/// 将 UNC 路径转为 `file:` URI + `immutable=1` 查询参数。
///
/// `\\wsl.localhost\archlinux\home\user\opencode.db`
///   → `file:////wsl.localhost/archlinux/home/user/opencode.db?immutable=1`
///
/// 四斜杠的原理：`file://`（scheme + 空 authority）+ `/wsl.localhost/...`（path 以 // 开头表示 UNC）。
/// Windows 版 SQLite VFS 会把以 `//` 开头的 path 当作 UNC 路径处理。
fn to_immutable_uri(path: &Path) -> String {
    let path_str = path.to_string_lossy().replace('\\', "/");
    let encoded = percent_encode_for_uri(&path_str);
    format!("file://{encoded}?immutable=1")
}

/// 打开 SQLite 数据库进行只读访问。
///
/// - 本地路径：`READ_ONLY | NO_MUTEX` + `busy_timeout(2s)`，应对本地写者持锁时的短时竞争
/// - UNC 路径：`READ_ONLY | NO_MUTEX | URI` + `immutable=1`，跳过锁和 WAL/SHM
///
/// 调用方只需将 `Connection::open_with_flags(...)` 替换为此函数。
pub(crate) fn open_readonly(path: &Path) -> Result<Connection, rusqlite::Error> {
    if is_unc_path(path) {
        let uri = to_immutable_uri(path);
        Connection::open_with_flags(
            &uri,
            OpenFlags::SQLITE_OPEN_READ_ONLY
                | OpenFlags::SQLITE_OPEN_NO_MUTEX
                | OpenFlags::SQLITE_OPEN_URI,
        )
    } else {
        let conn = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        conn.busy_timeout(Duration::from_secs(2))?;
        Ok(conn)
    }
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
    fn test_to_immutable_uri_wsl_localhost() {
        let path = Path::new(r"\\wsl.localhost\archlinux\home\user\opencode.db");
        let uri = to_immutable_uri(path);
        assert_eq!(
            uri,
            "file:////wsl.localhost/archlinux/home/user/opencode.db?immutable=1"
        );
    }

    #[test]
    fn test_to_immutable_uri_wsl_dollar() {
        let path = Path::new(r"\\wsl$\Ubuntu\home\user\opencode.db");
        let uri = to_immutable_uri(path);
        assert_eq!(
            uri,
            "file:////wsl$/Ubuntu/home/user/opencode.db?immutable=1"
        );
    }

    #[test]
    fn test_to_immutable_uri_spaces() {
        let path = Path::new(r"\\wsl.localhost\archlinux\home\my user\opencode.db");
        let uri = to_immutable_uri(path);
        assert_eq!(
            uri,
            "file:////wsl.localhost/archlinux/home/my%20user/opencode.db?immutable=1"
        );
    }

    #[test]
    fn test_to_immutable_uri_chinese() {
        // 中文目录名测试百分号编码
        let path = Path::new(r"\\wsl.localhost\archlinux\home\用户\opencode.db");
        let uri = to_immutable_uri(path);
        // UTF-8: 用 = E7 94 A8, 户 = E6 88 B7
        assert_eq!(
            uri,
            "file:////wsl.localhost/archlinux/home/%E7%94%A8%E6%88%B7/opencode.db?immutable=1"
        );
    }

    #[test]
    fn test_to_immutable_uri_preserves_dash_dot_underscore() {
        let path = Path::new(r"\\wsl.localhost\archlinux\home\user-1.0\db_v2.db");
        let uri = to_immutable_uri(path);
        assert_eq!(
            uri,
            "file:////wsl.localhost/archlinux/home/user-1.0/db_v2.db?immutable=1"
        );
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
}
