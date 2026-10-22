//! Read per-generation model names from Antigravity's native conversation database.
use prost::Message;
use rusqlite::{Connection, OpenFlags, OptionalExtension};
use std::{collections::HashMap, path::Path, time::Duration};

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

pub(super) fn models_for_transcript(
    transcript: &Path,
    steps: &[u64],
) -> Result<HashMap<u64, String>, String> {
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
        return Ok(HashMap::new());
    }
    let root = root.canonicalize().map_err(|error| error.to_string())?;
    let path = path.canonicalize().map_err(|error| error.to_string())?;
    if !path.starts_with(root) {
        return Err("Conversation database is outside the Antigravity root".into());
    }
    read_models(&path, steps).map_err(|error| error.to_string())
}

fn read_models(path: &Path, steps: &[u64]) -> rusqlite::Result<HashMap<u64, String>> {
    let mut conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    // A busy native writer must not hold up the usage refresh. Normal SQLite reads
    // include the WAL; immutable mode would miss live generations.
    conn.busy_timeout(Duration::ZERO)?;
    let tx = conn.transaction()?;
    let mut step_query = tx.prepare("SELECT metadata FROM steps WHERE idx=?1 AND step_type=15")?;
    let mut generation_query = tx.prepare("SELECT data FROM gen_metadata WHERE idx=?1")?;
    let mut models = HashMap::new();
    for &step in steps {
        let Ok(index) = i64::try_from(step) else {
            continue;
        };
        let metadata: Option<Vec<u8>> = step_query
            .query_row([index], |row| row.get(0))
            .optional()?
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
            .optional()?;
        let Some(response) = data
            .and_then(|bytes| GenerationMetadata::decode(bytes.as_slice()).ok())
            .and_then(|metadata| metadata.response)
        else {
            continue;
        };
        let model = response.model.trim();
        if !model.is_empty() && !model.eq_ignore_ascii_case("unknown") {
            models.insert(step, model.to_string());
        }
    }
    Ok(models)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uses_step_references_including_zero_instead_of_generation_order() {
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
    fn reads_committed_wal_and_returns_promptly_for_an_exclusive_writer() {
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
        assert!(models_for_transcript(&transcript, &[1]).unwrap().is_empty());
        assert!(!dir.path().join("conversations").exists());
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
        assert!(models_for_transcript(&transcript, &[1]).is_err());
    }
}
