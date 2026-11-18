//! Account-scoped Desktop Code record state, shared by history and migration.
use crate::desktop_data::{component, error, field, parse, profile_dirs, Dir, ProfileKey};
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
pub(crate) const RECORD_LIMIT: u64 = 4 * 1024 * 1024;
pub(crate) struct Group {
    pub records: BTreeMap<String, Vec<u8>>,
    pub archived: BTreeSet<String>,
    pub deleted: BTreeSet<String>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Problem {
    Deleted,
    Unverified,
}
pub(crate) fn group(roots: &[PathBuf; 2], key: &ProfileKey) -> Result<(Dir, Group), String> {
    component(&key.account)?;
    component(&key.org)?;
    let (account, dir) = profile_dirs(roots, key)?;
    let mut deleted = BTreeSet::new();
    for org in account.entries().map_err(error)? {
        if component(&org).is_err() {
            continue;
        }
        let child = account.child(&org).map_err(error)?;
        for name in child.entries().map_err(error)? {
            if let Some(id) = name.strip_prefix("deleted_") {
                component(id)?;
                deleted.insert(id.to_owned());
            }
        }
    }
    let index = dir
        .optional("archived-sessions.idx", RECORD_LIMIT)
        .map_err(error)?;
    let mut archived = BTreeSet::new();
    if let Some(bytes) = &index {
        let value = parse(bytes)?;
        if value.get("v").and_then(Value::as_u64) != Some(1) {
            return Err("Unrecognized Desktop archive index".into());
        }
        for id in value
            .get("archived")
            .and_then(Value::as_array)
            .ok_or("Invalid Desktop archive index")?
        {
            let id = id.as_str().ok_or("Invalid archive entry")?;
            component(id)?;
            archived.insert(id.to_owned());
        }
    }
    let mut records = BTreeMap::new();
    for name in dir.entries().map_err(error)? {
        if let Some(id) = name
            .strip_suffix(".json")
            .filter(|id| id.starts_with("local_"))
        {
            component(id)?;
            // A bad or linked source fails this profile closed; it is never copied.
            records.insert(id.to_owned(), dir.read(&name, RECORD_LIMIT).map_err(error)?);
        }
    }
    Ok((
        dir,
        Group {
            records,
            archived,
            deleted,
        },
    ))
}
pub(crate) fn transcript_ids(record: &Value) -> Vec<String> {
    let mut ids = Vec::new();
    for value in record
        .get("priorCliSessionIds")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .chain(
            [
                "preClearCliSessionId",
                "unarchivedCliSessionId",
                "cliSessionId",
            ]
            .iter()
            .filter_map(|key| record.get(*key)),
        )
    {
        if let Some(id) = value.as_str().filter(|id| component(id).is_ok()) {
            if !ids.iter().any(|v| v == id) {
                ids.push(id.to_owned());
            }
        }
    }
    ids
}

pub(crate) fn is_deleted(group: &Group, id: &str, record: &Value) -> bool {
    std::iter::once(id.to_owned())
        .chain(id.strip_prefix("local_").map(str::to_owned))
        .chain(transcript_ids(record))
        .any(|id| group.deleted.contains(&id))
}

pub(crate) fn session_record(group: &Group, id: &str, bytes: &[u8]) -> Result<Value, Problem> {
    let mut value = parse(bytes).map_err(|_| Problem::Unverified)?;
    if field(&value, "sessionId") != id || !value.get("isArchived").is_some_and(Value::is_boolean) {
        return Err(Problem::Unverified);
    }
    if is_deleted(group, id, &value) {
        return Err(Problem::Deleted);
    }
    if group.archived.contains(id) {
        value["isArchived"] = true.into();
    }
    Ok(value)
}
