use crate::codex_config::{get_codex_config_dir, CC_SWITCH_CODEX_MODEL_PROVIDER_ID};
use crate::database::Database;
use crate::error::AppError;
use crate::session_manager;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct CodexProviderVisibility {
    pub provider_id: String,
    pub linked: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderCodexSession {
    pub session: session_manager::SessionMeta,
    pub linked_provider_ids: Vec<String>,
    pub visible_to_current_provider: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetCodexSessionProvidersRequest {
    pub session_id: String,
    pub source_path: String,
    pub provider_ids: Vec<String>,
    pub link_mode: Option<String>,
    pub sync_to_codex: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexVisibilitySyncResult {
    pub changed_jsonl_files: u32,
    pub changed_state_rows: u32,
    pub skipped: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexSessionProviderUpdateResult {
    pub provider_ids: Vec<String>,
    pub sync: Option<CodexVisibilitySyncResult>,
}

fn now_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0)
}

fn validate_codex_session_source_path(
    codex_dir: &Path,
    source_path: &Path,
) -> Result<PathBuf, AppError> {
    let canonical_source = source_path
        .canonicalize()
        .map_err(|e| AppError::io(source_path, e))?;
    let roots = [
        codex_dir.join("sessions"),
        codex_dir.join("archived_sessions"),
    ];
    let allowed = roots.iter().any(|root| {
        root.canonicalize()
            .map(|canonical_root| canonical_source.starts_with(canonical_root))
            .unwrap_or(false)
    });
    if !allowed {
        return Err(AppError::Message(format!(
            "Codex session path is outside configured session roots: {}",
            source_path.display()
        )));
    }
    Ok(canonical_source)
}

#[derive(Debug, Clone)]
struct JsonlRewriteResult {
    changed: bool,
}

fn rewrite_jsonl_session_provider_bucket(
    source_path: &Path,
    codex_dir: &Path,
    backup_root: &Path,
    target_model_provider: &str,
) -> Result<JsonlRewriteResult, AppError> {
    let source_path = validate_codex_session_source_path(codex_dir, source_path)?;
    let metadata_before = fs::metadata(&source_path).map_err(|e| AppError::io(&source_path, e))?;
    let modified_before = metadata_before.modified().ok();
    let len_before = metadata_before.len();
    let content = fs::read_to_string(&source_path).map_err(|e| AppError::io(&source_path, e))?;
    let mut changed = false;
    let mut rewritten = String::with_capacity(content.len());

    for segment in content.split_inclusive('\n') {
        let (line, newline) = segment
            .strip_suffix('\n')
            .map(|line| (line, "\n"))
            .unwrap_or((segment, ""));
        if line.contains("\"session_meta\"") {
            if let Ok(mut value) = serde_json::from_str::<Value>(line) {
                if value.get("type").and_then(Value::as_str) == Some("session_meta") {
                    if let Some(payload) = value.get_mut("payload").and_then(Value::as_object_mut) {
                        let old = payload.get("model_provider").and_then(Value::as_str);
                        if old != Some(target_model_provider) {
                            payload.insert(
                                "model_provider".to_string(),
                                Value::String(target_model_provider.to_string()),
                            );
                            rewritten.push_str(&serde_json::to_string(&value).map_err(|e| {
                                AppError::Config(format!(
                                    "serialize Codex session_meta failed: {e}"
                                ))
                            })?);
                            rewritten.push_str(newline);
                            changed = true;
                            continue;
                        }
                    }
                }
            }
        }
        rewritten.push_str(line);
        rewritten.push_str(newline);
    }

    if changed {
        ensure_session_file_unchanged(&source_path, modified_before, len_before)?;
        backup_codex_jsonl_file(&source_path, codex_dir, backup_root)?;
        ensure_session_file_unchanged(&source_path, modified_before, len_before)?;
        crate::config::atomic_write(&source_path, rewritten.as_bytes())?;
    }

    Ok(JsonlRewriteResult { changed })
}

fn ensure_session_file_unchanged(
    path: &Path,
    modified_before: Option<SystemTime>,
    len_before: u64,
) -> Result<(), AppError> {
    let metadata_after = fs::metadata(path).map_err(|e| AppError::io(path, e))?;
    if metadata_after.modified().ok() != modified_before || metadata_after.len() != len_before {
        return Err(AppError::Message(format!(
            "Codex session file changed during sharing sync: {}",
            path.display()
        )));
    }
    Ok(())
}

fn backup_codex_jsonl_file(
    source_path: &Path,
    codex_dir: &Path,
    backup_root: &Path,
) -> Result<(), AppError> {
    let backup_path = backup_root
        .join(now_nanos().to_string())
        .join("jsonl")
        .join(relative_backup_path(source_path, codex_dir));
    if let Some(parent) = backup_path.parent() {
        fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
    }
    fs::copy(source_path, &backup_path).map_err(|e| AppError::io(source_path, e))?;
    Ok(())
}

fn relative_backup_path(path: &Path, codex_dir: &Path) -> PathBuf {
    let canonical_path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let canonical_codex_dir = codex_dir
        .canonicalize()
        .unwrap_or_else(|_| codex_dir.to_path_buf());
    canonical_path
        .strip_prefix(&canonical_codex_dir)
        .map(Path::to_path_buf)
        .unwrap_or_else(|_| {
            path.file_name()
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("session.jsonl"))
        })
}

pub fn set_codex_session_provider_links(
    db: &Database,
    request: SetCodexSessionProvidersRequest,
) -> Result<CodexSessionProviderUpdateResult, AppError> {
    let link_mode = request.link_mode.as_deref().unwrap_or("manual");
    let sync = if request.sync_to_codex {
        Some(sync_codex_session_visibility(
            &request.source_path,
            &request.provider_ids,
        )?)
    } else {
        None
    };

    let links = db.replace_codex_session_provider_links(
        &request.session_id,
        &request.source_path,
        &request.provider_ids,
        link_mode,
    )?;
    let provider_ids = links
        .into_iter()
        .map(|link| link.provider_id)
        .collect::<Vec<_>>();

    Ok(CodexSessionProviderUpdateResult { provider_ids, sync })
}

pub fn sync_codex_session_visibility(
    source_path: &str,
    provider_ids: &[String],
) -> Result<CodexVisibilitySyncResult, AppError> {
    if provider_ids.is_empty() {
        return Ok(CodexVisibilitySyncResult {
            changed_jsonl_files: 0,
            changed_state_rows: 0,
            skipped: Vec::new(),
            warnings: vec!["No target providers were selected".to_string()],
        });
    }

    let codex_dir = get_codex_config_dir();
    let backup_root = crate::config::get_app_config_dir()
        .join("backups")
        .join("codex-session-sharing");
    let source_path = PathBuf::from(source_path);
    let rewrite = rewrite_jsonl_session_provider_bucket(
        &source_path,
        &codex_dir,
        &backup_root,
        CC_SWITCH_CODEX_MODEL_PROVIDER_ID,
    )?;

    Ok(CodexVisibilitySyncResult {
        changed_jsonl_files: u32::from(rewrite.changed),
        changed_state_rows: 0,
        skipped: Vec::new(),
        warnings: Vec::new(),
    })
}

fn merge_codex_sessions_with_links(
    db: &Database,
    current_provider_id: &str,
    sessions: Vec<session_manager::SessionMeta>,
) -> Result<Vec<ProviderCodexSession>, AppError> {
    let mut out = Vec::new();
    for session in sessions.into_iter().filter(|s| s.provider_id == "codex") {
        let Some(source_path) = session.source_path.clone() else {
            continue;
        };
        let links = db.get_codex_session_provider_links(&session.session_id, &source_path)?;
        let linked_provider_ids = links
            .into_iter()
            .map(|link| link.provider_id)
            .collect::<Vec<_>>();
        let visible_to_current_provider = linked_provider_ids
            .iter()
            .any(|id| id == current_provider_id)
            || session.model_provider.as_deref() == Some(CC_SWITCH_CODEX_MODEL_PROVIDER_ID);
        out.push(ProviderCodexSession {
            session,
            linked_provider_ids,
            visible_to_current_provider,
        });
    }
    Ok(out)
}

pub fn list_provider_codex_sessions(
    db: &Database,
    provider_id: &str,
) -> Result<Vec<ProviderCodexSession>, AppError> {
    let sessions = session_manager::scan_sessions();
    merge_codex_sessions_with_links(db, provider_id, sessions)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn write_codex_session(path: &std::path::Path, provider: &str) {
        std::fs::write(
            path,
            format!(
                "{{\"timestamp\":\"2026-06-13T08:00:00Z\",\"type\":\"session_meta\",\"payload\":{{\"id\":\"session-1\",\"cwd\":\"/tmp/project\",\"model_provider\":\"{provider}\"}}}}\n\
                 {{\"timestamp\":\"2026-06-13T08:00:01Z\",\"type\":\"response_item\",\"payload\":{{\"type\":\"message\",\"role\":\"user\",\"content\":\"hello\"}}}}\n"
            ),
        )
        .expect("write session");
    }

    #[test]
    fn validates_source_path_under_codex_roots() {
        let root = tempdir().expect("root");
        let sessions = root.path().join("sessions");
        std::fs::create_dir_all(&sessions).expect("mkdir");
        let allowed = sessions.join("session.jsonl");
        write_codex_session(&allowed, "old-provider");

        assert!(validate_codex_session_source_path(root.path(), &allowed).is_ok());
        assert!(validate_codex_session_source_path(
            root.path(),
            &root.path().join("../outside.jsonl")
        )
        .is_err());
    }

    #[test]
    fn rewrites_session_meta_provider_bucket_with_backup() {
        let root = tempdir().expect("root");
        let backup = tempdir().expect("backup");
        let sessions = root.path().join("sessions");
        std::fs::create_dir_all(&sessions).expect("mkdir");
        let path = sessions.join("session.jsonl");
        write_codex_session(&path, "old-provider");

        let result =
            rewrite_jsonl_session_provider_bucket(&path, root.path(), backup.path(), "custom")
                .expect("rewrite");

        assert!(result.changed);
        let rewritten = std::fs::read_to_string(&path).expect("read");
        assert!(rewritten.contains("\"model_provider\":\"custom\""));
        assert!(backup
            .path()
            .read_dir()
            .expect("read backup dir")
            .next()
            .is_some());
    }

    #[test]
    fn sync_empty_provider_list_does_not_rewrite_jsonl() {
        let result =
            sync_codex_session_visibility("C:/missing/codex/session.jsonl", &[]).expect("sync");

        assert_eq!(result.changed_jsonl_files, 0);
        assert_eq!(result.changed_state_rows, 0);
        assert_eq!(
            result.warnings,
            vec!["No target providers were selected".to_string()]
        );
    }

    #[test]
    fn set_links_does_not_persist_when_sync_fails() -> Result<(), AppError> {
        let db = Database::memory()?;
        let source_path = "C:/outside/missing-session.jsonl";

        let err = set_codex_session_provider_links(
            &db,
            SetCodexSessionProvidersRequest {
                session_id: "session-1".to_string(),
                source_path: source_path.to_string(),
                provider_ids: vec!["provider-a".to_string()],
                link_mode: Some("manual".to_string()),
                sync_to_codex: true,
            },
        )
        .expect_err("sync should fail");

        assert!(err.to_string().contains("missing-session.jsonl"));
        assert!(db
            .get_codex_session_provider_links("session-1", source_path)?
            .is_empty());
        Ok(())
    }

    #[test]
    fn set_links_returns_deduplicated_provider_ids() -> Result<(), AppError> {
        let db = Database::memory()?;

        let result = set_codex_session_provider_links(
            &db,
            SetCodexSessionProvidersRequest {
                session_id: "session-1".to_string(),
                source_path: "C:/Users/Test/.codex/sessions/session-1.jsonl".to_string(),
                provider_ids: vec!["provider-a".to_string(), "provider-a".to_string()],
                link_mode: Some("manual".to_string()),
                sync_to_codex: false,
            },
        )?;

        assert_eq!(result.provider_ids, vec!["provider-a".to_string()]);
        Ok(())
    }

    #[test]
    fn provider_session_listing_marks_linked_sessions() -> Result<(), AppError> {
        let db = Database::memory()?;
        db.replace_codex_session_provider_links(
            "session-1",
            "C:/Users/Test/.codex/sessions/session-1.jsonl",
            &["provider-a".to_string()],
            "manual",
        )?;

        let sessions = vec![session_manager::SessionMeta {
            provider_id: "codex".to_string(),
            session_id: "session-1".to_string(),
            model_provider: Some("custom".to_string()),
            title: Some("hello".to_string()),
            summary: None,
            project_dir: None,
            created_at: Some(1),
            last_active_at: Some(2),
            source_path: Some("C:/Users/Test/.codex/sessions/session-1.jsonl".to_string()),
            resume_command: Some("codex resume session-1".to_string()),
        }];

        let visible = merge_codex_sessions_with_links(&db, "provider-a", sessions)?;
        assert_eq!(visible.len(), 1);
        assert!(visible[0].visible_to_current_provider);
        assert_eq!(
            visible[0].linked_provider_ids,
            vec!["provider-a".to_string()]
        );
        Ok(())
    }
}
