//! Copy-only migration of existing Desktop Code records between local profiles.
use crate::desktop_data::{component, discover, error, field, live_roots, parse, Dir, ProfileKey};
use crate::desktop_recovery::{ensure_stopped, write, RECOVERY_LOCK};
use crate::desktop_session_data::{group, is_deleted, session_record, Problem, RECORD_LIMIT};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, io, path::PathBuf};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub id: String,
    pub title: String,
    pub cwd: String,
    pub archived: bool,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    pub key: ProfileKey,
    pub last_known_account: bool,
    pub sessions: Vec<Session>,
    pub warnings: Vec<String>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Inventory {
    pub profiles: Vec<Profile>,
    pub warnings: Vec<String>,
    pub writes_supported: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Request {
    pub source: ProfileKey,
    pub target: ProfileKey,
    pub session_ids: Vec<String>,
}
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Reason {
    Deleted,
    Unverified,
    TargetArchived,
}
#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Disposition {
    Ready,
    Exists,
    Blocked { reason: Reason },
}
#[derive(Debug, Serialize)]
pub struct Item {
    pub id: String,
    pub disposition: Disposition,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    pub token: String,
    pub items: Vec<Item>,
    pub can_apply: bool,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Receipt {
    pub copied_sessions: Vec<String>,
    pub skipped: Vec<String>,
    pub error: Option<String>,
}
struct Prepared {
    preview: Preview,
    target: Dir,
    sessions: Vec<(String, Vec<u8>)>,
}
pub(crate) struct Store {
    roots: [PathBuf; 2],
}
impl Store {
    pub fn live() -> Result<Self, String> {
        Ok(Self {
            roots: live_roots()?,
        })
    }
    pub fn inventory(&self) -> Result<Inventory, String> {
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
                Ok((_, state)) => {
                    for (id, bytes) in &state.records {
                        match session_record(&state, id, bytes) {
                            Ok(record) => profile.sessions.push(Session {
                                id: id.clone(),
                                title: field(&record, "title").into(),
                                cwd: field(&record, "cwd").into(),
                                archived: record["isArchived"] == true,
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
            profiles.push(profile);
        }
        Ok(Inventory {
            profiles,
            warnings,
            writes_supported: cfg!(target_os = "macos"),
        })
    }
    pub fn preview(&self, request: &Request) -> Result<Preview, String> {
        Ok(self.prepare(request)?.preview)
    }
    fn prepare(&self, request: &Request) -> Result<Prepared, String> {
        if request.source == request.target {
            return Err("Choose a different target account or profile".into());
        }
        if request.session_ids.len() > 10_000 {
            return Err("Too many selected sessions".into());
        }
        let mut seen = BTreeSet::new();
        for id in &request.session_ids {
            component(id)?;
            if !seen.insert(id) {
                return Err("Duplicate selected identifier".into());
            }
        }
        let (_, source) = group(&self.roots, &request.source)?;
        let (target_dir, target) = group(&self.roots, &request.target)?;
        let mut digest = Sha256::new();
        digest.update(serde_json::to_vec(request).map_err(error)?);
        for state in [&source, &target] {
            digest.update(
                serde_json::to_vec(&(&state.records, &state.archived, &state.deleted))
                    .map_err(error)?,
            );
        }
        let mut items = vec![];
        let mut sessions = vec![];
        for id in &request.session_ids {
            let bytes = source
                .records
                .get(id)
                .ok_or("Selected source session disappeared; refresh the inventory")?;
            let disposition = if target.records.contains_key(id) {
                Disposition::Exists
            } else {
                match session_record(&source, id, bytes) {
                    Err(Problem::Deleted) => Disposition::Blocked {
                        reason: Reason::Deleted,
                    },
                    Err(Problem::Unverified) => Disposition::Blocked {
                        reason: Reason::Unverified,
                    },
                    Ok(record) if is_deleted(&target, id, &record) => Disposition::Blocked {
                        reason: Reason::Deleted,
                    },
                    Ok(_) if target.archived.contains(id) => Disposition::Blocked {
                        reason: Reason::TargetArchived,
                    },
                    Ok(record) => {
                        let bytes = if parse(bytes)?.get("isArchived") == record.get("isArchived") {
                            bytes.clone()
                        } else {
                            serde_json::to_vec_pretty(&record).map_err(error)?
                        };
                        sessions.push((id.clone(), bytes));
                        Disposition::Ready
                    }
                }
            };
            items.push(Item {
                id: id.clone(),
                disposition,
            });
        }
        let can_apply = items
            .iter()
            .any(|i| matches!(i.disposition, Disposition::Ready))
            && !items
                .iter()
                .any(|i| matches!(i.disposition, Disposition::Blocked { .. }));
        Ok(Prepared {
            preview: Preview {
                token: format!("{:x}", digest.finalize()),
                items,
                can_apply,
            },
            target: target_dir,
            sessions,
        })
    }
    fn apply_checked(
        &self,
        request: &Request,
        token: &str,
        check_stopped: impl Fn() -> Result<(), String>,
    ) -> Result<Receipt, String> {
        check_stopped()?;
        let prepared = self.prepare(request)?;
        if prepared.preview.token != token {
            return Err("Source or target changed; preview again".into());
        }
        if !prepared.preview.can_apply {
            return Err("No recoverable selection, or a selected entry is blocked".into());
        }
        let mut receipt = Receipt {
            copied_sessions: vec![],
            skipped: prepared
                .preview
                .items
                .iter()
                .filter(|i| matches!(i.disposition, Disposition::Exists))
                .map(|i| i.id.clone())
                .collect(),
            error: None,
        };
        let result = (|| -> Result<(), String> {
            for (id, bytes) in prepared.sessions {
                check_stopped()?;
                match write(&prepared.target, &format!("{id}.json"), &bytes, None) {
                    Ok(()) => receipt.copied_sessions.push(id.clone()),
                    Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                        receipt.skipped.push(id);
                        continue;
                    }
                    Err(e) => {
                        if e.published {
                            receipt.copied_sessions.push(id);
                        }
                        return Err(error(e));
                    }
                }
                if prepared
                    .target
                    .read(&format!("{id}.json"), RECORD_LIMIT)
                    .map_err(error)?
                    != bytes
                {
                    return Err("Session read-back did not match".into());
                }
            }
            Ok(())
        })();
        receipt.error = result.err();
        Ok(receipt)
    }
}
pub(crate) fn apply_live(request: Request, token: String) -> Result<Receipt, String> {
    let _lock = RECOVERY_LOCK
        .lock()
        .map_err(|_| "Desktop recovery lock is unavailable")?;
    Store::live()?.apply_checked(&request, &token, ensure_stopped)
}
#[cfg(test)]
#[path = "desktop_migration_tests.rs"]
mod tests;
