//! Oh My Pi (omp.sh) session usage importer.
//!
//! Oh My Pi is its own coding agent, not Pi (pi.dev): it keeps its session
//! store under the Oh My Pi agent dir and records a few entry kinds Pi never
//! writes. It does persist Pi's session JSONL format, so the parsing engine is
//! shared with [`super::session_usage_pi`] and this module only declares what
//! differs.

use crate::database::Database;
use crate::error::AppError;
use crate::services::session_usage::SessionSyncResult;
use crate::services::session_usage_pi::{sync_flavor, Flavor};
use std::path::PathBuf;

pub(crate) const OMP: Flavor = Flavor {
    app_type: "omp",
    data_source: "omp_session",
    provider_placeholder: "_omp_session",
    cutoff_key: "omp:child-cutoff",
    id_prefix: "omp_session",
    hash_tag: "omp-session",
    label: "Oh My Pi",
    model_usage_carrier: true,
    title_slot: true,
    session_files,
};

fn session_files() -> Result<Vec<PathBuf>, String> {
    crate::session_manager::providers::pi::session_files_for(
        &crate::session_manager::providers::omp::OMP,
    )
}

/// Import usage from every Oh My Pi session file under the session browser's
/// current root and layout rules, including nested subagent sessions.
pub fn sync_omp_usage(db: &Database) -> Result<SessionSyncResult, AppError> {
    sync_flavor(db, &OMP)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::lock_conn;
    use crate::services::session_usage_pi::{sync_pi_files, PI};
    use rust_decimal::Decimal;
    use std::fs::File;
    use std::io::Write;
    use std::path::Path;
    use std::str::FromStr;

    fn write_lines(path: &Path, lines: &[&str]) {
        let mut file = File::create(path).expect("create session");
        for line in lines {
            writeln!(file, "{line}").expect("write session line");
        }
    }

    #[test]
    fn imports_own_store_carriers_and_placeholders() -> Result<(), AppError> {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("omp-session.jsonl");
        write_lines(
            &path,
            &[
                // Oh My Pi opens every session with a title slot line.
                r#"{"type":"title","v":1,"title":"Fix the parser","updatedAt":"2026-10-29T13:17:48.750Z","pad":""}"#,
                r#"{"type":"session","version":3,"id":"01a187ce-171e-7335-946a-3114ab4dcbe7","timestamp":"2026-10-29T13:16:32.926Z","cwd":"/work"}"#,
                r#"{"type":"message","id":"call-1","parentId":null,"timestamp":"2026-10-29T13:17:42.984Z","message":{"role":"assistant","content":[{"type":"text","text":"ok"}],"api":"openai-completions","provider":"workbuddy","model":"deepseek-v4.1-flash","responseId":"chatcmpl-1","duration":6744.172099999996,"ttft":6192.651100000003,"usage":{"input":282,"output":194,"cacheRead":12800,"cacheWrite":0,"totalTokens":13276,"reasoningTokens":41,"cost":{"input":0.0000846,"output":0.0002328,"cacheRead":0.0000768,"cacheWrite":0,"total":0.0003942}},"stopReason":"toolUse"}}"#,
                r#"{"type":"model_usage","id":"judge-1","parentId":"call-1","timestamp":"2026-10-29T13:26:53.160Z","purpose":"job-widget","api":"openai-completions","provider":"workbuddy","model":"deepseek-v4.1-flash","usage":{"input":312,"output":110,"cacheRead":2432,"cacheWrite":0,"totalTokens":2854,"cost":{"input":0.0000936,"output":0.000132,"cacheRead":0.000014592,"cacheWrite":0,"total":0.000240192}},"stopReason":"toolUse"}"#,
                r#"{"type":"message","id":"call-2","parentId":"call-1","timestamp":"2026-10-29T13:28:00.000Z","message":{"role":"assistant","content":[],"api":"openai-completions","provider":"workbuddy","model":"deepseek-v4.1-flash","usage":{"input":0,"output":0,"cacheRead":0,"cacheWrite":0,"totalTokens":0,"cost":{"input":0,"output":0,"cacheRead":0,"cacheWrite":0,"total":0}},"stopReason":"aborted","errorMessage":"Request was aborted"}}"#,
                // A compaction has no provider/model of its own, so it lands on
                // this app's placeholder row.
                r#"{"type":"compaction","id":"compact-1","parentId":"call-2","timestamp":"2026-10-29T13:29:00.000Z","summary":"summary","usage":{"input":11,"output":12,"cacheRead":2,"cacheWrite":3,"totalTokens":28,"cost":{"input":0,"output":0,"cacheRead":0,"cacheWrite":0,"total":0}}}"#,
            ],
        );

        let db = Database::memory()?;
        let result = sync_pi_files(&db, &OMP, std::slice::from_ref(&path));
        assert!(result.errors.is_empty(), "{:?}", result.errors);
        assert_eq!(result.imported, 4);

        {
            let conn = lock_conn!(db.conn);
            let rows: Vec<(String, i64, i64, String)> = conn
                .prepare(
                    "SELECT app_type, input_tokens, latency_ms, total_cost_usd
                     FROM proxy_request_logs WHERE data_source = 'omp_session'
                     ORDER BY created_at",
                )?
                .query_map([], |row| {
                    Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
                })?
                .collect::<Result<_, _>>()?;
            assert_eq!(rows.len(), 4);
            assert!(rows.iter().all(|row| row.0 == "omp"));
            // `model_usage` keeps its request metadata at the entry's top
            // level, so it must still be attributed and billed.
            assert_eq!(rows[0].1, 282);
            assert_eq!(rows[1].1, 312);
            assert_eq!(
                rows.iter()
                    .map(|row| Decimal::from_str(&row.3).expect("reported total"))
                    .sum::<Decimal>(),
                // assistant 0.0003942 + job widget 0.000240192
                Decimal::from_str("0.000634392").expect("expected total")
            );
            // Oh My Pi records the measured generation time itself.
            assert_eq!(rows[0].2, 6744);

            let aborted: (i64, String) = conn.query_row(
                "SELECT status_code, error_message FROM proxy_request_logs
                 WHERE data_source = 'omp_session' AND status_code = 499",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )?;
            assert_eq!(aborted, (499, "Request was aborted".to_string()));
        }

        let providers = db.get_provider_stats(None, None, Some(OMP.app_type), None, None)?;
        assert!(providers.iter().any(|provider| {
            provider.provider_id == OMP.provider_placeholder
                && provider.provider_name == "Oh My Pi (Session)"
        }));
        Ok(())
    }

    #[test]
    fn pi_importer_does_not_read_the_oh_my_pi_store() -> Result<(), AppError> {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("omp-session.jsonl");
        write_lines(
            &path,
            &[
                r#"{"type":"title","v":1,"title":"Fix the parser","updatedAt":"2026-10-29T13:17:48.750Z","pad":""}"#,
                r#"{"type":"session","version":3,"id":"01a187ce-171e-7335-946a-3114ab4dcbe7","timestamp":"2026-10-29T13:16:32.926Z","cwd":"/work"}"#,
            ],
        );

        // Pi's first valid JSON must be the session header; the title slot is
        // Oh My Pi's alone, so the two apps never absorb each other's rows.
        let db = Database::memory()?;
        let result = sync_pi_files(&db, &PI, std::slice::from_ref(&path));
        assert_eq!(result.imported, 0);
        assert_eq!(result.errors.len(), 1);
        Ok(())
    }
}
