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
        if let Some(model) = [tiered, base]
            .into_iter()
            .flatten()
            .map(|model| model.trim().to_string())
            .find(|model| !model.is_empty() && !model.eq_ignore_ascii_case("unknown"))
        {
            models.insert(step, model);
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

#[cfg(test)]
mod tests {
    use super::*;

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
