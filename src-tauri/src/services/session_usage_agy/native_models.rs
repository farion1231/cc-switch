//! Read per-generation model names from Antigravity's native conversation database.
use prost::Message;
#[cfg(test)]
use rusqlite::Connection;
use rusqlite::OptionalExtension;
use std::{collections::HashMap, path::Path};

// Only the fields needed for the step -> generation -> model association are decoded.
#[derive(Clone, PartialEq, Message)]
struct StepMetadata {
    #[prost(message, optional, tag = "20")]
    generation: Option<GenerationReference>,
}

#[derive(Clone, PartialEq, Message)]
struct GenerationReference {
    // Protobuf omits the scalar when the generation index is zero.
    #[prost(uint64, tag = "3")]
    index: u64,
}

#[derive(Clone, PartialEq, Message)]
struct GenerationMetadata {
    #[prost(message, optional, tag = "1")]
    response: Option<ResponseMetadata>,
}

#[derive(Clone, PartialEq, Message)]
struct ResponseMetadata {
    #[prost(string, tag = "19")]
    model: String,
}

// Field 21 carries the tiered ID (`gemini-4-argon-high`) that matches `Model Selection`;
// older builds leave it out or store a display name. It is decoded on its own so an
// unexpected value there can never hide field 19.
#[derive(Clone, PartialEq, Message)]
struct GenerationModelId {
    #[prost(message, optional, tag = "1")]
    response: Option<ResponseModelId>,
}

#[derive(Clone, PartialEq, Message)]
struct ResponseModelId {
    #[prost(string, tag = "21")]
    model_id: String,
}

// Field 11 is the wait for the first token and field 12 the rest of the stream; both match the
// step's own created / first-output / done timestamps to the millisecond. Decoded on their own
// so an unexpected wire type there can never hide the model name.
#[derive(Clone, PartialEq, Message)]
struct GenerationTiming {
    #[prost(message, optional, tag = "1")]
    response: Option<ResponseTiming>,
}

#[derive(Clone, PartialEq, Message)]
struct ResponseTiming {
    #[prost(message, optional, tag = "11")]
    first_token: Option<DurationValue>,
    #[prost(message, optional, tag = "12")]
    streaming: Option<DurationValue>,
}

/// `google.protobuf.Duration`.
#[derive(Clone, PartialEq, Message)]
struct DurationValue {
    #[prost(int64, tag = "1")]
    seconds: i64,
    #[prost(int32, tag = "2")]
    nanos: i32,
}

impl DurationValue {
    fn millis(&self) -> Option<i64> {
        let ms = self
            .seconds
            .checked_mul(1000)?
            .checked_add(i64::from(self.nanos) / 1_000_000)?;
        (ms >= 0).then_some(ms)
    }
}

/// Exact timing recorded by agy for one generation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Timing {
    pub latency_ms: i64,
    pub first_token_ms: i64,
}

#[derive(Default)]
pub(super) struct Generation {
    pub model: Option<String>,
    pub timing: Option<Timing>,
}

fn timing_of(bytes: &[u8]) -> Option<Timing> {
    let response = GenerationTiming::decode(bytes).ok()?.response?;
    let first_token_ms = response.first_token?.millis()?;
    let latency_ms = first_token_ms.checked_add(response.streaming?.millis()?)?;
    (latency_ms > 0).then_some(Timing {
        latency_ms,
        first_token_ms,
    })
}

fn native_database_path(transcript: &Path) -> Result<Option<std::path::PathBuf>, String> {
    // brain/<id>/.system_generated/logs/transcript_full.jsonl and conversations/<id>.db
    // share the same CLI/IDE/private-profile root.
    let session = transcript
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .ok_or("Missing transcript session directory")?;
    let root = session
        .parent()
        .filter(|brain| brain.file_name().is_some_and(|name| name == "brain"))
        .and_then(Path::parent)
        .ok_or("Missing Antigravity root")?;
    let id = session.file_name().ok_or("Missing session id")?;
    let path = root.join("conversations").join(id).with_extension("db");
    if !path.is_file() {
        return Ok(None);
    }
    if !database_stays_inside(root, &path)? {
        return Err("Conversation database is outside the Antigravity root".into());
    }
    Ok(Some(path))
}

/// Fingerprint of the companion `conversations/<id>.db` (and its WAL file when present) so
/// `session_log_sync` cursors stay valid across steady-state syncs while still re-importing
/// when the native database is created or updated after the transcript.
pub(super) fn native_state_fingerprint(transcript: &Path) -> String {
    let Ok(Some(path)) = native_database_path(transcript) else {
        return "none".to_string();
    };
    let file_sig = |target: &Path| -> (i64, u64) {
        std::fs::metadata(target)
            .map(|meta| {
                (
                    crate::services::session_usage::metadata_modified_nanos(&meta),
                    meta.len(),
                )
            })
            .unwrap_or((0, 0))
    };
    let (db_mtime, db_len) = file_sig(&path);
    let mut wal_os = path.as_os_str().to_os_string();
    wal_os.push("-wal");
    let (wal_mtime, wal_len) = file_sig(Path::new(&wal_os));
    format!("{db_mtime}:{db_len}:{wal_mtime}:{wal_len}")
}

pub(super) fn generations_for_transcript(
    transcript: &Path,
    steps: &[u64],
) -> Result<HashMap<u64, Generation>, String> {
    let Some(path) = native_database_path(transcript)? else {
        return Ok(HashMap::new());
    };
    read_generations(&path, steps)
}

/// Canonicalize when the platform can. A WSL UNC path often cannot, so compare the
/// `\\wsl.localhost\<distro>` strings instead of refusing to read the database.
fn database_stays_inside(root: &Path, path: &Path) -> Result<bool, String> {
    if let (Ok(root), Ok(path)) = (root.canonicalize(), path.canonicalize()) {
        return Ok(path.starts_with(root));
    }
    match (
        crate::config::wsl_unc_to_linux(&root.to_string_lossy()),
        crate::config::wsl_unc_to_linux(&path.to_string_lossy()),
    ) {
        (Some((root_distro, root_linux)), Some((path_distro, path_linux))) => Ok(root_distro
            .eq_ignore_ascii_case(&path_distro)
            && (path_linux == root_linux || path_linux.starts_with(&format!("{root_linux}/")))),
        _ => Err(format!(
            "cannot confirm {} stays inside {}",
            path.display(),
            root.display()
        )),
    }
}

fn read_generations(path: &Path, steps: &[u64]) -> Result<HashMap<u64, Generation>, String> {
    let db = crate::agy_sqlite::open_read_only(path)?;
    // A busy native writer must not hold up the usage refresh. The direct open keeps
    // a zero timeout; a WSL snapshot is a private copy and uses its own timeout.
    // Immutable mode would miss live generations, so the WAL is included.
    let conn = db.conn();
    let mut step_query = conn
        .prepare("SELECT metadata FROM steps WHERE idx=?1 AND step_type=15")
        .map_err(|err| err.to_string())?;
    let mut generation_query = conn
        .prepare("SELECT data FROM gen_metadata WHERE idx=?1")
        .map_err(|err| err.to_string())?;
    let mut generations = HashMap::new();
    for &step in steps {
        let Ok(index) = i64::try_from(step) else {
            continue;
        };
        let metadata: Option<Vec<u8>> = step_query
            .query_row([index], |row| row.get(0))
            .optional()
            .map_err(|err| err.to_string())?
            .flatten();
        let Some(reference) = metadata
            .and_then(|bytes| StepMetadata::decode(bytes.as_slice()).ok())
            .and_then(|metadata| metadata.generation)
        else {
            continue;
        };
        let Ok(generation) = i64::try_from(reference.index) else {
            continue;
        };
        let data: Option<Vec<u8>> = generation_query
            .query_row([generation], |row| row.get(0))
            .optional()
            .map_err(|err| err.to_string())?;
        let Some(bytes) = data else {
            continue;
        };
        let tiered = GenerationModelId::decode(bytes.as_slice())
            .ok()
            .and_then(|metadata| metadata.response)
            .map(|response| response.model_id);
        let base = GenerationMetadata::decode(bytes.as_slice())
            .ok()
            .and_then(|metadata| metadata.response)
            .map(|response| response.model);
        // Orchestrator's display field names the mode (or just "Gemini Flash"),
        // while field 19 identifies the planner/worker that produced this step.
        // Only these exact AGY aliases are mapped; unknown IDs keep their configured pricing.
        // https://antigravity.google/docs/models#orchestrator
        let orchestrator_model = match base.as_deref() {
            Some("gemini-pro-orchestrator") => Some("gemini-4-carbon"),
            Some("gemini-pro-orchestrator-argon") => Some("gemini-4-argon"),
            Some("gemini-flash-worker-agy") => Some("gemini-4-hydrogen"),
            _ => None,
        };
        let tiered = tiered.filter(|model| {
            let id = super::model_id_of(model);
            // Keep a concrete field-21 model/tier if a newer AGY build supplies one.
            !(orchestrator_model.is_some()
                && (super::is_orchestrator(&id)
                    || matches!(id.as_str(), "gemini-flash" | "gemini-flash-worker-agy")))
        });
        let model = [tiered, orchestrator_model.map(str::to_string), base]
            .into_iter()
            .flatten()
            .map(|model| model.trim().to_string())
            .find(|model| !model.is_empty() && !model.eq_ignore_ascii_case("unknown"));
        let timing = timing_of(&bytes);
        if model.is_some() || timing.is_some() {
            generations.insert(step, Generation { model, timing });
        }
    }
    Ok(generations)
}

#[cfg(test)]
fn read_models(path: &Path, steps: &[u64]) -> Result<HashMap<u64, String>, String> {
    Ok(read_generations(path, steps)?
        .into_iter()
        .filter_map(|(step, generation)| Some((step, generation.model?)))
        .collect())
}

#[cfg(test)]
pub(super) fn write_test_database(path: &Path, entries: &[(i64, u64, &str)]) -> Connection {
    let conn = Connection::open(path).unwrap();
    conn.execute_batch(
        "CREATE TABLE steps (idx INTEGER PRIMARY KEY, step_type INTEGER, metadata BLOB);
         CREATE TABLE gen_metadata (idx INTEGER PRIMARY KEY, data BLOB);",
    )
    .unwrap();
    for &(step, generation, model) in entries {
        let metadata = StepMetadata {
            generation: Some(GenerationReference { index: generation }),
        };
        let data = GenerationMetadata {
            response: Some(ResponseMetadata {
                model: model.into(),
            }),
        };
        conn.execute(
            "INSERT INTO steps VALUES (?1,15,?2)",
            rusqlite::params![step, metadata.encode_to_vec()],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO gen_metadata VALUES (?1,?2)",
            rusqlite::params![generation, data.encode_to_vec()],
        )
        .unwrap();
    }
    conn
}

/// Rewrites a generation with field 19 and an optional raw field 21 (any wire type).
#[cfg(test)]
pub(super) fn set_test_generation(conn: &Connection, generation: i64, base: &str, field21: &[u8]) {
    let mut response = ResponseMetadata { model: base.into() }.encode_to_vec();
    response.extend_from_slice(field21);
    let mut data = Vec::new();
    prost::encoding::bytes::encode(1, &response, &mut data);
    conn.execute(
        "UPDATE gen_metadata SET data=?1 WHERE idx=?2",
        rusqlite::params![data, generation],
    )
    .unwrap();
}

/// Field 21 encoded as a string.
#[cfg(test)]
pub(super) fn test_model_id(id: &str) -> Vec<u8> {
    let mut field = Vec::new();
    prost::encoding::string::encode(21, &id.to_string(), &mut field);
    field
}

/// Fields 11 and 12 as durations of the given milliseconds.
#[cfg(test)]
pub(super) fn test_timing(first_token_ms: i64, streaming_ms: i64) -> Vec<u8> {
    let duration = |ms: i64| DurationValue {
        seconds: ms / 1000,
        nanos: i32::try_from(ms % 1000 * 1_000_000).unwrap(),
    };
    let mut fields = Vec::new();
    prost::encoding::message::encode(11, &duration(first_token_ms), &mut fields);
    prost::encoding::message::encode(12, &duration(streaming_ms), &mut fields);
    fields
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn orchestrator_native_aliases_use_generation_role_instead_of_generic_display() {
        if crate::config::sqlite_unsupported_in_temp_dir() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session.db");
        let conn = write_test_database(
            &path,
            &[
                (1, 0, "gemini-pro-orchestrator"),
                (3, 7, "gemini-flash-worker-agy"),
                (5, 2, "gemini-pro-orchestrator-argon"),
                (7, 3, "future-worker"),
                (9, 4, "gemini-flash-worker-agy"),
            ],
        );
        set_test_generation(
            &conn,
            0,
            "gemini-pro-orchestrator",
            &test_model_id("Orchestrator (Carbon)"),
        );
        set_test_generation(
            &conn,
            7,
            "gemini-flash-worker-agy",
            &test_model_id("Gemini Flash"),
        );
        set_test_generation(
            &conn,
            2,
            "gemini-pro-orchestrator-argon",
            &test_model_id("Orchestrator (Argon)"),
        );
        set_test_generation(
            &conn,
            4,
            "gemini-flash-worker-agy",
            &test_model_id("gemini-4-hydrogen-high"),
        );
        let models = read_models(&path, &[1, 3, 5, 7, 9]).unwrap();
        assert_eq!(models[&1], "gemini-4-carbon");
        assert_eq!(models[&3], "gemini-4-hydrogen");
        assert_eq!(models[&5], "gemini-4-argon");
        assert_eq!(models[&7], "future-worker");
        assert_eq!(models[&9], "gemini-4-hydrogen-high");
    }

    #[test]
    fn uses_step_references_including_zero_instead_of_generation_order() {
        if crate::config::sqlite_unsupported_in_temp_dir() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session.db");
        let conn = write_test_database(
            &path,
            &[
                (1, 0, "gemini-4-argon"),
                (3, 7, "gemini-3.8-flash-tiered"),
                (8, 2, "claude-opus-5-5-medium"),
            ],
        );
        // Unknown fields contain misleading model text and must be skipped.
        let mut data: Vec<u8> = conn
            .query_row("SELECT data FROM gen_metadata WHERE idx=0", [], |r| {
                r.get(0)
            })
            .unwrap();
        data.extend_from_slice(b"\x22\x0efake-model-pro");
        conn.execute("UPDATE gen_metadata SET data=?1 WHERE idx=0", [data])
            .unwrap();
        let models = read_models(&path, &[8, 1, 3, 99]).unwrap();
        assert_eq!(models.len(), 3);
        assert_eq!(models[&1], "gemini-4-argon");
        assert_eq!(models[&3], "gemini-3.8-flash-tiered");
        assert_eq!(models[&8], "claude-opus-5-5-medium");
    }

    #[test]
    fn skips_missing_references_and_malformed_metadata() {
        if crate::config::sqlite_unsupported_in_temp_dir() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session.db");
        let conn = write_test_database(&path, &[(1, 0, "gemini-4-argon")]);
        conn.execute_batch(
            "INSERT INTO steps VALUES (2,15,X'');
             INSERT INTO steps VALUES (3,15,X'80');
             INSERT INTO steps VALUES (4,15,X'A201021809');
             INSERT INTO steps VALUES (5,15,NULL);",
        )
        .unwrap();
        let models = read_models(&path, &[1, 2, 3, 4, 5]).unwrap();
        assert_eq!(models.len(), 1);
        assert_eq!(models[&1], "gemini-4-argon");
        conn.execute("UPDATE gen_metadata SET data=X'80'", [])
            .unwrap();
        assert!(read_models(&path, &[1]).unwrap().is_empty());
    }

    #[test]
    fn prefers_tiered_model_id_and_falls_back_to_the_base_name() {
        if crate::config::sqlite_unsupported_in_temp_dir() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session.db");
        let conn = write_test_database(
            &path,
            &[
                (1, 0, "gemini-3.8-flash-n"),
                (2, 1, "gemini-3.8-flash-n"),
                (3, 2, "gemini-4-argon"),
                (4, 3, "gemini-4-argon"),
                (5, 4, "gemini-4-argon"),
            ],
        );
        set_test_generation(
            &conn,
            0,
            "gemini-3.8-flash-n",
            &test_model_id("gemini-3.8-flash-high"),
        );
        set_test_generation(&conn, 2, "gemini-4-argon", &test_model_id("  "));
        set_test_generation(&conn, 3, "gemini-4-argon", &test_model_id("unknown"));
        // Field 21 as a varint (wrong wire type) must not hide field 19.
        set_test_generation(&conn, 4, "gemini-4-argon", b"\xa8\x01\x07");
        let models = read_models(&path, &[1, 2, 3, 4, 5]).unwrap();
        assert_eq!(models[&1], "gemini-3.8-flash-high");
        // Older builds have no field 21.
        assert_eq!(models[&2], "gemini-3.8-flash-n");
        assert_eq!(models[&3], "gemini-4-argon");
        assert_eq!(models[&4], "gemini-4-argon");
        assert_eq!(models[&5], "gemini-4-argon");
    }

    #[test]
    fn reads_committed_wal_and_returns_promptly_for_an_exclusive_writer() {
        if crate::config::sqlite_unsupported_in_temp_dir() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session.db");
        let conn = write_test_database(&path, &[(1, 0, "gemini-4-argon")]);
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0;")
            .unwrap();
        let data = GenerationMetadata {
            response: Some(ResponseMetadata {
                model: "gemini-3.8-flash-tiered".into(),
            }),
        }
        .encode_to_vec();
        conn.execute("UPDATE gen_metadata SET data=?1 WHERE idx=0", [data])
            .unwrap();
        assert!(path.with_extension("db-wal").exists());
        assert_eq!(
            read_models(&path, &[1]).unwrap()[&1],
            "gemini-3.8-flash-tiered"
        );
        conn.execute_batch(
            "PRAGMA wal_checkpoint(TRUNCATE); PRAGMA journal_mode=DELETE;
                            BEGIN EXCLUSIVE;",
        )
        .unwrap();
        let start = std::time::Instant::now();
        assert!(read_models(&path, &[1]).is_err());
        assert!(start.elapsed() < Duration::from_secs(1));
        conn.execute_batch("ROLLBACK").unwrap();
    }

    #[test]
    fn missing_database_is_not_created() {
        let dir = tempfile::tempdir().unwrap();
        let transcript = dir
            .path()
            .join("brain/session/.system_generated/logs/transcript_full.jsonl");
        assert!(generations_for_transcript(&transcript, &[1])
            .unwrap()
            .is_empty());
        assert!(!dir.path().join("conversations").exists());
    }

    #[test]
    fn reads_first_token_and_streaming_durations_without_hiding_the_model() {
        if crate::config::sqlite_unsupported_in_temp_dir() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session.db");
        let conn = write_test_database(
            &path,
            &[
                (1, 0, "gemini-4-argon"),
                (2, 1, "gemini-4-argon"),
                (3, 2, "gemini-4-argon"),
                (4, 3, "gemini-4-argon"),
            ],
        );
        // Values taken from a real claude-opus generation: 2.799 s wait, 1.722 s stream.
        set_test_generation(&conn, 0, "gemini-4-argon", &test_timing(2799, 1722));
        // Only the wait recorded: no total to compare against, so no timing.
        let mut wait_only = Vec::new();
        prost::encoding::message::encode(
            11,
            &DurationValue {
                seconds: 3,
                nanos: 0,
            },
            &mut wait_only,
        );
        set_test_generation(&conn, 1, "gemini-4-argon", &wait_only);
        // Field 11 as a varint (wrong wire type) must not hide the model.
        set_test_generation(&conn, 2, "gemini-4-argon", b"\x58\x07");
        let got = read_generations(&path, &[1, 2, 3, 4]).unwrap();
        assert_eq!(
            got[&1].timing,
            Some(Timing {
                latency_ms: 4521,
                first_token_ms: 2799,
            })
        );
        for step in [2, 3, 4] {
            assert_eq!(got[&step].model.as_deref(), Some("gemini-4-argon"));
            assert_eq!(got[&step].timing, None, "step {step}");
        }
    }

    #[test]
    #[cfg(unix)]
    fn does_not_follow_database_symlinks_outside_the_profile() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let path = outside.path().join("session.db");
        write_test_database(&path, &[(1, 0, "gemini-4-argon")]);
        std::fs::create_dir(dir.path().join("conversations")).unwrap();
        std::os::unix::fs::symlink(&path, dir.path().join("conversations/session.db")).unwrap();
        let transcript = dir
            .path()
            .join("brain/session/.system_generated/logs/transcript_full.jsonl");
        assert!(generations_for_transcript(&transcript, &[1]).is_err());
    }
}
