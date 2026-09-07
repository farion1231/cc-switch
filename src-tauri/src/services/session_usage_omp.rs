//! OMP usage importer.
//!
//! OMP (Oh My Pi) retired its standalone `usage.db` ledger: normalized token
//! and cost data now lives in the session JSONL trees under
//! `<omp agent dir>/sessions/`, written with Pi's tree protocol plus a
//! `{"type":"title"}` preamble line before the session header. Import runs
//! through the shared Pi machinery with [`OMP_PROFILE`], which keeps the
//! per-app ledger namespace (`data_source = "omp_session"`) separate from Pi.

use crate::database::Database;
use crate::error::AppError;
use crate::services::session_usage::SessionSyncResult;
use crate::services::session_usage_pi::{sync_session_files, OMP_PROFILE};

/// Import usage from every OMP session file discoverable by the session
/// browser's current root and layout rules.
pub fn sync_omp_usage(db: &Database) -> Result<SessionSyncResult, AppError> {
    let files = crate::session_manager::providers::omp::session_files()
        .map_err(|error| AppError::Config(format!("无法发现 OMP 会话: {error}")))?;
    Ok(sync_session_files(db, &files, &OMP_PROFILE))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::lock_conn;
    use rust_decimal::Decimal;
    use std::fs::File;
    use std::io::Write;
    use std::path::Path;
    use std::str::FromStr;

    fn write_lines(path: &Path, lines: &[impl AsRef<str>]) {
        let mut file = File::create(path).expect("create session");
        for line in lines {
            writeln!(file, "{}", line.as_ref()).expect("write session line");
        }
    }

    fn omp_session_lines(entry_id: &str, input: u32) -> Vec<String> {
        vec![
            // OMP's title preamble: Pi's strict header-first parser would
            // reject this line; the OMP profile must skip it instead.
            r#"{"type":"title","v":1,"title":"renamed session","updatedAt":"2023-11-14T22:13:20Z"}"#.to_string(),
            r#"{"type":"session","version":3,"id":"01a075f4-4792-74f1-b2a3-239cce0ec474","timestamp":"2023-11-14T22:13:20Z","cwd":"/work"}"#.to_string(),
            format!(
                r#"{{"type":"message","id":"{entry_id}","parentId":null,"timestamp":"2023-11-14T22:13:21Z","message":{{"role":"assistant","content":[{{"type":"text","text":"ok"}}],"provider":"native-provider","model":"omp-model","responseId":"response-1","timestamp":1700000000000,"usage":{{"input":{input},"output":2,"cacheRead":5,"cacheWrite":2,"totalTokens":999,"cost":{{"input":0.00001,"output":0.000014,"cacheRead":5E-7,"cacheWrite":0.000001,"total":0.0000255}}}},"stopReason":"stop"}}}}"#
            ),
        ]
    }

    #[test]
    fn imports_omp_files_with_title_preamble_and_omp_namespace() -> Result<(), AppError> {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("omp-session.jsonl");
        write_lines(&path, &omp_session_lines("assistant-1", 10));

        let db = Database::memory()?;
        let result = sync_session_files(&db, std::slice::from_ref(&path), &OMP_PROFILE);
        assert_eq!(result.imported, 1);
        assert!(result.errors.is_empty());

        let conn = lock_conn!(db.conn);
        let row: (String, String, String, String) = conn.query_row(
            "SELECT request_id, provider_id, app_type, data_source
             FROM proxy_request_logs WHERE data_source = 'omp_session'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )?;
        assert!(row.0.starts_with("omp_session:"));
        assert_eq!(row.1, "native-provider");
        assert_eq!(row.2, "omp");
        assert_eq!(row.3, "omp_session");

        let total_cost: String = conn.query_row(
            "SELECT total_cost_usd FROM proxy_request_logs WHERE data_source = 'omp_session'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(
            Decimal::from_str(&total_cost).expect("reported total"),
            Decimal::from_str("0.0000255").expect("expected total")
        );
        Ok(())
    }

    #[test]
    fn omp_ledger_and_omp_namespace_stay_disjoint_from_pi() -> Result<(), AppError> {
        let temp = tempfile::tempdir().expect("tempdir");
        let omp_path = temp.path().join("omp-session.jsonl");
        write_lines(&omp_path, &omp_session_lines("assistant-1", 10));

        let db = Database::memory()?;
        assert_eq!(
            sync_session_files(&db, std::slice::from_ref(&omp_path), &OMP_PROFILE).imported,
            1
        );
        // Re-import must dedup on the OMP ledger, not re-bill the request.
        assert_eq!(
            sync_session_files(&db, std::slice::from_ref(&omp_path), &OMP_PROFILE).imported,
            0
        );

        // The same content under the Pi profile carries different identity
        // labels, so it is a distinct ledger row (namespaces never collide).
        let pi_path = temp.path().join("pi-session.jsonl");
        write_lines(
            &pi_path,
            &omp_session_lines("assistant-1", 10)
                .into_iter()
                .skip(1)
                .collect::<Vec<_>>(),
        );
        assert_eq!(
            crate::services::session_usage_pi::sync_session_files(
                &db,
                std::slice::from_ref(&pi_path),
                &crate::services::session_usage_pi::PI_PROFILE,
            )
            .imported,
            1
        );
        let counts: (i64, i64) = lock_conn!(db.conn).query_row(
            "SELECT
                (SELECT COUNT(*) FROM proxy_request_logs WHERE data_source = 'omp_session'),
                (SELECT COUNT(*) FROM proxy_request_logs WHERE data_source = 'pi_session')",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        assert_eq!(counts, (1, 1));
        Ok(())
    }

    #[test]
    fn headerless_omp_garbage_file_imports_empty_without_error() -> Result<(), AppError> {
        // 真实形态：SIGHUP 杀掉的 OMP 会话只剩一行 session_exit，无 header。
        // 必须静默导入 0 条（游标照常推进），而不是每次同步重复报错。
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("omp-orphan.jsonl");
        write_lines(
            &path,
            &[
                r#"{"type":"custom","customType":"session_exit","id":"0420d19e","timestamp":"2026-09-05T06:40:26.927Z"}"#,
            ],
        );

        let db = Database::memory()?;
        let first = sync_session_files(&db, std::slice::from_ref(&path), &OMP_PROFILE);
        assert_eq!(first.imported, 0);
        assert!(
            first.errors.is_empty(),
            "无 header 垃圾文件不应报错: {:?}",
            first.errors
        );
        let second = sync_session_files(&db, std::slice::from_ref(&path), &OMP_PROFILE);
        assert_eq!(second.errors.len(), 0);
        Ok(())
    }
}
