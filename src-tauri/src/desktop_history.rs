//! Read-only Claude Desktop account history; no native session mutation or launch.
use crate::desktop_data::{component, discover, error, field, live_roots, Dir, ProfileKey};
use crate::desktop_session_data::{group, session_record, transcript_ids, Problem};
use serde::Serialize;
use serde_json::Value;
use std::{collections::BTreeMap, io, path::PathBuf};
#[derive(Debug, Clone, Serialize)]
// Unsupported-platform commands retain the response types but construct no entries.
#[cfg_attr(not(unix), allow(dead_code))]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum TranscriptLink {
    Available { id: String, path: String },
    Missing { id: String },
    Ambiguous { id: String },
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopSession {
    pub id: String,
    pub title: String,
    pub cwd: String,
    pub archived: bool,
    pub updated_at: Option<i64>,
    pub transcripts: Vec<TranscriptLink>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    pub key: ProfileKey,
    pub last_known_account: bool,
    pub sessions: Vec<DesktopSession>,
    pub warnings: Vec<String>,
}
#[derive(Debug, Serialize)]
pub struct Inventory {
    pub profiles: Vec<Profile>,
    pub warnings: Vec<String>,
}
pub(crate) struct Store {
    roots: [PathBuf; 2],
    projects: PathBuf,
}
impl Store {
    pub fn live() -> Result<Self, String> {
        Ok(Self {
            roots: live_roots()?,
            projects: crate::config::get_claude_config_dir().join("projects"),
        })
    }
    fn transcript_index(&self) -> Result<BTreeMap<String, Vec<String>>, String> {
        let root = match Dir::open(&self.projects) {
            Ok(root) => root,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
            Err(e) => return Err(error(e)),
        };
        let mut index: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for project in root.entries().map_err(error)? {
            let dir = match root.child(&project) {
                Ok(dir) => dir,
                Err(_) => continue,
            };
            for name in dir.entries().map_err(error)? {
                if let Some(id) = name
                    .strip_suffix(".jsonl")
                    .filter(|id| component(id).is_ok())
                {
                    // The session reader validates containment again when opening.
                    let path = dir.path.join(&name);
                    if std::fs::symlink_metadata(&path).is_ok_and(|m| m.is_file()) {
                        index
                            .entry(id.to_owned())
                            .or_default()
                            .push(path.to_string_lossy().into_owned());
                    }
                }
            }
        }
        Ok(index)
    }

    pub fn inventory(&self) -> Result<Inventory, String> {
        let index = self.transcript_index()?;
        let (locations, warnings) = discover(&self.roots)?;
        let mut profiles = vec![];
        for location in locations {
            let mut profile = Profile {
                key: location.key,
                last_known_account: location.last_known_account,
                sessions: vec![],
                warnings: vec![],
            };
            match group(&self.roots, &profile.key) {
                Ok((_, group)) => {
                    for (id, bytes) in &group.records {
                        match session_record(&group, id, bytes) {
                            Ok(record) => profile.sessions.push(DesktopSession {
                                id: id.clone(),
                                title: field(&record, "title").into(),
                                cwd: field(&record, "cwd").into(),
                                archived: record["isArchived"] == true,
                                updated_at: record.get("lastActivityAt").and_then(Value::as_i64),
                                transcripts: transcript_links(&record, &index),
                            }),
                            Err(Problem::Deleted) => {}
                            Err(_) => profile
                                .warnings
                                .push(format!("Unverified session omitted: {id}")),
                        }
                    }
                }
                Err(e) => profile.warnings.push(e),
            }
            profile
                .sessions
                .sort_by(|a, b| b.updated_at.cmp(&a.updated_at).then(a.id.cmp(&b.id)));
            profiles.push(profile);
        }
        Ok(Inventory { profiles, warnings })
    }
    pub fn open_transcript(
        &self,
        key: &ProfileKey,
        id: &str,
        transcript_id: &str,
    ) -> Result<crate::session_manager::SessionMeta, String> {
        component(id)?;
        component(transcript_id)?;
        let (_, group) = group(&self.roots, key)?;
        let bytes = group
            .records
            .get(id)
            .ok_or("Desktop session no longer exists")?;
        let record = session_record(&group, id, bytes)
            .map_err(|_| "Desktop session is deleted or unverified")?;
        let index = self.transcript_index()?;
        let path = transcript_links(&record, &index)
            .into_iter()
            .find_map(|link| match link {
                TranscriptLink::Available { id, path } if id == transcript_id => Some(path),
                _ => None,
            })
            .ok_or("Referenced transcript is missing or ambiguous")?;
        Ok(crate::session_manager::SessionMeta {
            provider_id: "claude".into(),
            session_id: transcript_id.to_owned(),
            title: Some(field(&record, "title").to_owned()),
            summary: None,
            project_dir: Some(field(&record, "cwd").to_owned()),
            created_at: record.get("createdAt").and_then(Value::as_i64),
            last_active_at: record.get("lastActivityAt").and_then(Value::as_i64),
            source_path: Some(path),
            resume_command: None,
        })
    }
}
fn transcript_links(record: &Value, index: &BTreeMap<String, Vec<String>>) -> Vec<TranscriptLink> {
    // SSH/WSL/VM sessions are not same-machine local transcript sources.
    if record.get("sshConfig").is_some_and(|v| !v.is_null())
        || record.get("wslConfig").is_some_and(|v| !v.is_null())
        || record.get("violinBow") == Some(&Value::Bool(true))
    {
        return vec![];
    }
    transcript_ids(record)
        .into_iter()
        .map(|id| match index.get(&id).map(Vec::as_slice) {
            Some([path]) => TranscriptLink::Available {
                id,
                path: path.clone(),
            },
            Some(paths) if !paths.is_empty() => TranscriptLink::Ambiguous { id },
            _ => TranscriptLink::Missing { id },
        })
        .collect()
}

#[cfg(test)]
#[path = "desktop_history_tests.rs"]
mod tests;
