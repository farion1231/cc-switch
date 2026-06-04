use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Component, Path, PathBuf};

use chrono::Utc;
use rusqlite::{
    params_from_iter, types::Value as SqlValue, Connection, OpenFlags, OptionalExtension,
};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tempfile::{tempdir, TempDir};
use zip::write::SimpleFileOptions;
use zip::DateTime;

use crate::codex_config::get_codex_config_dir;
use crate::error::AppError;
use crate::services::skill::SkillService;

use super::{io_context_localized, localized, MAX_SYNC_ARTIFACT_BYTES, REMOTE_SKILLS_ZIP};

/// Maximum number of entries allowed in a zip archive.
const MAX_EXTRACT_ENTRIES: usize = 10_000;
const CODEX_SYNC_BACKUP_DIR: &str = "codex_sync_backups";
const CODEX_SESSION_ROOTS: [&str; 2] = ["sessions", "archived_sessions"];
const CODEX_SQLITE_FILES: [&str; 2] = ["state_5.sqlite", "goals_1.sqlite"];
const CODEX_EXCLUDED_ROOT_DIRS: [&str; 9] = [
    ".tmp",
    "tmp",
    "cache",
    ".sandbox",
    ".sandbox-bin",
    ".sandbox-secrets",
    "session_sync_backups",
    CODEX_SYNC_BACKUP_DIR,
    "vendor_imports",
];
const CODEX_EXCLUDED_ROOT_FILES: [&str; 6] = [
    "logs_2.sqlite",
    "logs_2.sqlite-wal",
    "logs_2.sqlite-shm",
    "models_cache.json",
    "cap_sid",
    "installation_id",
];

pub(super) struct SkillsBackup {
    _tmp: TempDir,
    backup_dir: PathBuf,
    ssot_path: PathBuf,
    existed: bool,
}

pub(super) fn zip_skills_ssot(dest_path: &Path) -> Result<(), AppError> {
    let source = SkillService::get_ssot_dir().map_err(|e| {
        localized(
            "webdav.sync.skills_ssot_dir_failed",
            format!("获取 Skills SSOT 目录失败: {e}"),
            format!("Failed to resolve Skills SSOT directory: {e}"),
        )
    })?;
    if let Some(parent) = dest_path.parent() {
        fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
    }

    let file = fs::File::create(dest_path).map_err(|e| AppError::io(dest_path, e))?;
    let mut writer = zip::ZipWriter::new(file);
    let options = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .last_modified_time(DateTime::default());

    if source.exists() {
        let canonical_root = fs::canonicalize(&source).unwrap_or_else(|_| source.clone());
        let mut visited = HashSet::new();
        mark_visited_dir(&canonical_root, &mut visited)?;
        zip_dir_recursive(
            &canonical_root,
            &canonical_root,
            &mut writer,
            options,
            &mut visited,
        )?;
    }

    writer.finish().map_err(|e| {
        localized(
            "webdav.sync.skills_zip_write_failed",
            format!("写入 skills.zip 失败: {e}"),
            format!("Failed to write skills.zip: {e}"),
        )
    })?;
    Ok(())
}

pub(super) fn restore_skills_zip(raw: &[u8]) -> Result<(), AppError> {
    let tmp = tempdir().map_err(|e| {
        io_context_localized(
            "webdav.sync.skills_extract_tmpdir_failed",
            "创建 skills 解压临时目录失败",
            "Failed to create temporary directory for skills extraction",
            e,
        )
    })?;
    let zip_path = tmp.path().join(REMOTE_SKILLS_ZIP);
    fs::write(&zip_path, raw).map_err(|e| AppError::io(&zip_path, e))?;

    let file = fs::File::open(&zip_path).map_err(|e| AppError::io(&zip_path, e))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| {
        localized(
            "webdav.sync.skills_zip_parse_failed",
            format!("解析 skills.zip 失败: {e}"),
            format!("Failed to parse skills.zip: {e}"),
        )
    })?;

    let extracted = tmp.path().join("skills-extracted");
    fs::create_dir_all(&extracted).map_err(|e| AppError::io(&extracted, e))?;

    if archive.len() > MAX_EXTRACT_ENTRIES {
        return Err(localized(
            "webdav.sync.skills_zip_too_many_entries",
            format!(
                "skills.zip 条目数过多（{}），上限 {MAX_EXTRACT_ENTRIES}",
                archive.len()
            ),
            format!(
                "skills.zip has too many entries ({}), limit is {MAX_EXTRACT_ENTRIES}",
                archive.len()
            ),
        ));
    }

    let mut total_bytes: u64 = 0;
    for idx in 0..archive.len() {
        let mut entry = archive.by_index(idx).map_err(|e| {
            localized(
                "webdav.sync.skills_zip_entry_read_failed",
                format!("读取 ZIP 项失败: {e}"),
                format!("Failed to read ZIP entry: {e}"),
            )
        })?;
        let Some(safe_name) = entry.enclosed_name() else {
            continue;
        };
        let out_path = extracted.join(safe_name);
        if entry.is_dir() {
            fs::create_dir_all(&out_path).map_err(|e| AppError::io(&out_path, e))?;
            continue;
        }
        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
        }
        let mut out = fs::File::create(&out_path).map_err(|e| AppError::io(&out_path, e))?;
        let _written = copy_entry_with_total_limit(
            &mut entry,
            &mut out,
            &mut total_bytes,
            MAX_SYNC_ARTIFACT_BYTES,
            &out_path,
        )?;
    }

    let ssot = SkillService::get_ssot_dir().map_err(|e| {
        localized(
            "webdav.sync.skills_ssot_dir_failed",
            format!("获取 Skills SSOT 目录失败: {e}"),
            format!("Failed to resolve Skills SSOT directory: {e}"),
        )
    })?;
    let bak = ssot.with_extension("bak");

    if ssot.exists() {
        if bak.exists() {
            let _ = fs::remove_dir_all(&bak);
        }
        fs::rename(&ssot, &bak).map_err(|e| AppError::io(&ssot, e))?;
    }

    if let Err(e) = copy_dir_recursive(&extracted, &ssot) {
        if bak.exists() {
            let _ = fs::remove_dir_all(&ssot);
            let _ = fs::rename(&bak, &ssot);
        }
        return Err(e);
    }

    let _ = fs::remove_dir_all(&bak);
    Ok(())
}

pub(super) fn backup_current_skills() -> Result<SkillsBackup, AppError> {
    let ssot = SkillService::get_ssot_dir().map_err(|e| {
        localized(
            "webdav.sync.skills_ssot_dir_failed",
            format!("获取 Skills SSOT 目录失败: {e}"),
            format!("Failed to resolve Skills SSOT directory: {e}"),
        )
    })?;
    let tmp = tempdir().map_err(|e| {
        io_context_localized(
            "webdav.sync.skills_backup_tmpdir_failed",
            "创建 skills 备份临时目录失败",
            "Failed to create temporary directory for skills backup",
            e,
        )
    })?;
    let backup_dir = tmp.path().join("skills-backup");

    let existed = ssot.exists();
    if existed {
        copy_dir_recursive(&ssot, &backup_dir)?;
    }

    Ok(SkillsBackup {
        _tmp: tmp,
        backup_dir,
        ssot_path: ssot,
        existed,
    })
}

pub(super) fn restore_skills_from_backup(backup: &SkillsBackup) -> Result<(), AppError> {
    if backup.ssot_path.exists() {
        fs::remove_dir_all(&backup.ssot_path).map_err(|e| AppError::io(&backup.ssot_path, e))?;
    }

    if backup.existed {
        copy_dir_recursive(&backup.backup_dir, &backup.ssot_path)?;
    }

    Ok(())
}

pub(super) fn zip_codex_data(dest_path: &Path) -> Result<(), AppError> {
    zip_codex_data_from_config_dir(&get_codex_config_dir(), dest_path)
}

fn zip_codex_data_from_config_dir(config_dir: &Path, dest_path: &Path) -> Result<(), AppError> {
    if let Some(parent) = dest_path.parent() {
        fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
    }

    let tmp = tempdir().map_err(|e| {
        io_context_localized(
            "webdav.sync.codex_tmpdir_failed",
            "创建 Codex 同步临时目录失败",
            "Failed to create temporary directory for Codex sync",
            e,
        )
    })?;
    let sqlite_snapshots = stage_codex_sqlite_snapshots(config_dir, tmp.path())?;

    let file = fs::File::create(dest_path).map_err(|e| AppError::io(dest_path, e))?;
    let mut writer = zip::ZipWriter::new(file);
    let options = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .last_modified_time(DateTime::default());

    if config_dir.exists() {
        let canonical_root =
            fs::canonicalize(config_dir).unwrap_or_else(|_| config_dir.to_path_buf());
        let mut visited = HashSet::new();
        mark_visited_dir(&canonical_root, &mut visited)?;
        zip_codex_dir_recursive(
            &canonical_root,
            &canonical_root,
            &mut writer,
            options,
            &mut visited,
            &sqlite_snapshots,
        )?;
    }

    writer.finish().map_err(|e| {
        localized(
            "webdav.sync.codex_zip_write_failed",
            format!("写入 codex.zip 失败: {e}"),
            format!("Failed to write codex.zip: {e}"),
        )
    })?;
    Ok(())
}

pub(super) fn restore_codex_data_zip(raw: &[u8]) -> Result<(), AppError> {
    restore_codex_data_zip_into_config_dir(raw, &get_codex_config_dir())
}

pub(super) fn codex_data_fingerprint() -> Result<Option<String>, AppError> {
    codex_data_fingerprint_from_config_dir(&get_codex_config_dir())
}

fn restore_codex_data_zip_into_config_dir(raw: &[u8], config_dir: &Path) -> Result<(), AppError> {
    let tmp = tempdir().map_err(|e| {
        io_context_localized(
            "webdav.sync.codex_extract_tmpdir_failed",
            "创建 Codex 解压临时目录失败",
            "Failed to create temporary directory for Codex extraction",
            e,
        )
    })?;
    let zip_path = tmp.path().join("codex.zip");
    fs::write(&zip_path, raw).map_err(|e| AppError::io(&zip_path, e))?;

    let file = fs::File::open(&zip_path).map_err(|e| AppError::io(&zip_path, e))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| {
        localized(
            "webdav.sync.codex_zip_parse_failed",
            format!("解析 codex.zip 失败: {e}"),
            format!("Failed to parse codex.zip: {e}"),
        )
    })?;

    if archive.len() > MAX_EXTRACT_ENTRIES {
        return Err(localized(
            "webdav.sync.codex_zip_too_many_entries",
            format!(
                "codex.zip 条目数过多（{}），上限 {MAX_EXTRACT_ENTRIES}",
                archive.len()
            ),
            format!(
                "codex.zip has too many entries ({}), limit is {MAX_EXTRACT_ENTRIES}",
                archive.len()
            ),
        ));
    }

    let extracted = tmp.path().join("codex-extracted");
    fs::create_dir_all(&extracted).map_err(|e| AppError::io(&extracted, e))?;

    let mut total_bytes: u64 = 0;
    for idx in 0..archive.len() {
        let mut entry = archive.by_index(idx).map_err(|e| {
            localized(
                "webdav.sync.codex_zip_entry_read_failed",
                format!("读取 Codex ZIP 项失败: {e}"),
                format!("Failed to read Codex ZIP entry: {e}"),
            )
        })?;
        if entry.is_dir() {
            continue;
        }
        let Some(safe_name) = entry.enclosed_name() else {
            continue;
        };
        if should_skip_codex_rel(&safe_name) {
            continue;
        }

        let out_path = extracted.join(safe_name);
        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
        }
        let mut out = fs::File::create(&out_path).map_err(|e| AppError::io(&out_path, e))?;
        let _written = copy_entry_with_total_limit(
            &mut entry,
            &mut out,
            &mut total_bytes,
            MAX_SYNC_ARTIFACT_BYTES,
            &out_path,
        )?;
    }

    restore_codex_data_from_dir(&extracted, config_dir)
}

fn codex_data_fingerprint_from_config_dir(config_dir: &Path) -> Result<Option<String>, AppError> {
    if !config_dir.exists() {
        return Ok(None);
    }

    let root = fs::canonicalize(config_dir).unwrap_or_else(|_| config_dir.to_path_buf());
    let mut visited = HashSet::new();
    mark_visited_dir(&root, &mut visited)?;

    let mut entries = Vec::new();
    collect_codex_fingerprint_entries(&root, &root, &mut visited, &mut entries)?;
    entries.sort_by(|a, b| a.0.cmp(&b.0));

    let mut hasher = Sha256::new();
    for (rel, len, modified_ms) in entries {
        hasher.update(rel.as_bytes());
        hasher.update([0]);
        hasher.update(len.to_le_bytes());
        hasher.update(modified_ms.to_le_bytes());
        hasher.update([0]);
    }
    Ok(Some(format!("{:x}", hasher.finalize())))
}

fn collect_codex_fingerprint_entries(
    root: &Path,
    current: &Path,
    visited: &mut HashSet<PathBuf>,
    entries: &mut Vec<(String, u64, i64)>,
) -> Result<(), AppError> {
    let mut dir_entries: Vec<_> = fs::read_dir(current)
        .map_err(|e| AppError::io(current, e))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| AppError::io(current, e))?;
    dir_entries.sort_by_key(|entry| entry.file_name());

    for entry in dir_entries {
        let path = entry.path();
        if is_symlink(&path)? {
            continue;
        }
        let rel = path.strip_prefix(root).map_err(|e| {
            localized(
                "webdav.sync.codex_fingerprint_relative_path_failed",
                format!("生成 Codex 指纹相对路径失败: {e}"),
                format!("Failed to build Codex fingerprint relative path: {e}"),
            )
        })?;
        if should_skip_codex_fingerprint_rel(rel) {
            continue;
        }

        if path.is_dir() {
            let real_path = fs::canonicalize(&path).unwrap_or(path.clone());
            if !real_path.starts_with(root) || !mark_visited_dir(&real_path, visited)? {
                continue;
            }
            collect_codex_fingerprint_entries(root, &real_path, visited, entries)?;
            continue;
        }

        let metadata = fs::metadata(&path).map_err(|e| AppError::io(&path, e))?;
        let modified_ms = file_modified_ms(&path).unwrap_or(0);
        entries.push((
            rel.to_string_lossy().replace('\\', "/"),
            metadata.len(),
            modified_ms,
        ));
    }

    Ok(())
}

#[derive(Debug)]
struct CodexSessionFileInfo {
    session_id: Option<String>,
    last_active_at: Option<i64>,
    hash: String,
    bytes: Vec<u8>,
}

#[derive(Clone, Copy)]
struct SqlMergeTable {
    name: &'static str,
    primary_keys: &'static [&'static str],
    timestamp_column: Option<&'static str>,
}

const CODEX_STATE_MERGE_TABLES: &[SqlMergeTable] = &[
    SqlMergeTable {
        name: "threads",
        primary_keys: &["id"],
        timestamp_column: Some("updated_at_ms"),
    },
    SqlMergeTable {
        name: "thread_dynamic_tools",
        primary_keys: &["thread_id", "position"],
        timestamp_column: None,
    },
    SqlMergeTable {
        name: "stage1_outputs",
        primary_keys: &["thread_id"],
        timestamp_column: Some("source_updated_at"),
    },
    SqlMergeTable {
        name: "agent_jobs",
        primary_keys: &["id"],
        timestamp_column: Some("updated_at"),
    },
    SqlMergeTable {
        name: "agent_job_items",
        primary_keys: &["job_id", "item_id"],
        timestamp_column: Some("updated_at"),
    },
    SqlMergeTable {
        name: "thread_spawn_edges",
        primary_keys: &["child_thread_id"],
        timestamp_column: None,
    },
    SqlMergeTable {
        name: "remote_control_enrollments",
        primary_keys: &["websocket_url", "account_id", "app_server_client_name"],
        timestamp_column: Some("updated_at"),
    },
];

const CODEX_GOAL_MERGE_TABLES: &[SqlMergeTable] = &[SqlMergeTable {
    name: "thread_goals",
    primary_keys: &["thread_id"],
    timestamp_column: Some("updated_at_ms"),
}];

fn restore_codex_data_from_dir(extracted: &Path, config_dir: &Path) -> Result<(), AppError> {
    if !extracted.exists() {
        return Ok(());
    }

    let backup_root = config_dir
        .join(CODEX_SYNC_BACKUP_DIR)
        .join(Utc::now().format("%Y%m%dT%H%M%S%.3fZ").to_string());

    restore_codex_sessions_from_dir(extracted, config_dir, &backup_root)?;
    restore_codex_sqlite_files(extracted, config_dir, &backup_root)?;
    restore_regular_codex_files(extracted, config_dir, &backup_root)?;
    Ok(())
}

fn restore_codex_sessions_from_dir(
    extracted: &Path,
    config_dir: &Path,
    backup_root: &Path,
) -> Result<(), AppError> {
    let mut incoming_files = Vec::new();
    for root_name in CODEX_SESSION_ROOTS {
        collect_codex_files(&extracted.join(root_name), &mut incoming_files)?;
    }

    if incoming_files.is_empty() {
        return Ok(());
    }

    let mut local_index = build_codex_session_index(config_dir)?;
    for source in incoming_files {
        let rel = source.strip_prefix(extracted).map_err(|e| {
            localized(
                "webdav.sync.codex_relative_path_failed",
                format!("生成 Codex 相对路径失败: {e}"),
                format!("Failed to build Codex relative path: {e}"),
            )
        })?;
        let incoming = read_codex_session_file_info(&source)?;
        let target = incoming
            .session_id
            .as_ref()
            .and_then(|session_id| local_index.get(session_id).cloned())
            .unwrap_or_else(|| config_dir.join(rel));

        if target.exists() {
            if is_symlink(&target)? {
                log::warn!(
                    "[WebDAV] Skipping Codex session restore over symlink: {}",
                    target.display()
                );
                continue;
            }
            let local = read_codex_session_file_info(&target)?;
            if local.hash == incoming.hash || is_local_session_newer(&local, &incoming) {
                continue;
            }
            let _ = backup_codex_file(config_dir, &target, backup_root)?;
        }

        write_codex_file(&target, &incoming.bytes)?;
        if let Some(session_id) = incoming.session_id {
            local_index.insert(session_id, target);
        }
    }

    Ok(())
}

fn restore_codex_sqlite_files(
    extracted: &Path,
    config_dir: &Path,
    backup_root: &Path,
) -> Result<(), AppError> {
    for file_name in CODEX_SQLITE_FILES {
        let incoming = extracted.join(file_name);
        if !incoming.exists() {
            continue;
        }
        let target = config_dir.join(file_name);
        if target.exists() && is_symlink(&target)? {
            log::warn!(
                "[WebDAV] Skipping Codex SQLite restore over symlink: {}",
                target.display()
            );
            continue;
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
        }

        if !target.exists() {
            fs::copy(&incoming, &target).map_err(|e| AppError::io(&target, e))?;
            rewrite_codex_sqlite_paths(&target, config_dir, file_name)?;
            continue;
        }

        let backup_path = backup_codex_file(config_dir, &target, backup_root)?;
        let merge_result = merge_codex_sqlite(&incoming, &target, config_dir, file_name);
        if let Err(err) = merge_result {
            if let Some(backup_path) = backup_path {
                let _ = fs::copy(&backup_path, &target);
            }
            return Err(err);
        }
        rewrite_codex_sqlite_paths(&target, config_dir, file_name)?;
    }

    Ok(())
}

fn restore_regular_codex_files(
    extracted: &Path,
    config_dir: &Path,
    backup_root: &Path,
) -> Result<(), AppError> {
    let mut files = Vec::new();
    collect_codex_files(extracted, &mut files)?;
    for source in files {
        let rel = source.strip_prefix(extracted).map_err(|e| {
            localized(
                "webdav.sync.codex_relative_path_failed",
                format!("生成 Codex 相对路径失败: {e}"),
                format!("Failed to build Codex relative path: {e}"),
            )
        })?;
        if is_codex_session_rel(rel) || is_codex_sqlite_rel(rel) || should_skip_codex_rel(rel) {
            continue;
        }
        let target = config_dir.join(rel);
        if target.exists() {
            if is_symlink(&target)? {
                log::warn!(
                    "[WebDAV] Skipping Codex file restore over symlink: {}",
                    target.display()
                );
                continue;
            }
            let incoming = fs::read(&source).map_err(|e| AppError::io(&source, e))?;
            let local = fs::read(&target).map_err(|e| AppError::io(&target, e))?;
            if incoming == local {
                continue;
            }
            let _ = backup_codex_file(config_dir, &target, backup_root)?;
            write_codex_file(&target, &incoming)?;
        } else {
            let incoming = fs::read(&source).map_err(|e| AppError::io(&source, e))?;
            write_codex_file(&target, &incoming)?;
        }
    }

    Ok(())
}

fn stage_codex_sqlite_snapshots(
    config_dir: &Path,
    tmp_root: &Path,
) -> Result<HashMap<PathBuf, PathBuf>, AppError> {
    let mut snapshots = HashMap::new();
    for file_name in CODEX_SQLITE_FILES {
        let source = config_dir.join(file_name);
        if !source.exists() {
            continue;
        }
        let dest = tmp_root.join(file_name);
        copy_sqlite_snapshot(&source, &dest)?;
        snapshots.insert(PathBuf::from(file_name), dest);
    }
    Ok(snapshots)
}

fn copy_sqlite_snapshot(source: &Path, dest: &Path) -> Result<(), AppError> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
    }
    let source_conn = Connection::open_with_flags(source, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|e| AppError::Database(format!("Open Codex SQLite failed: {e}")))?;
    let mut dest_conn = Connection::open(dest)
        .map_err(|e| AppError::Database(format!("Create Codex SQLite snapshot failed: {e}")))?;
    let backup = rusqlite::backup::Backup::new(&source_conn, &mut dest_conn)
        .map_err(|e| AppError::Database(format!("Create Codex SQLite backup failed: {e}")))?;
    backup
        .step(-1)
        .map_err(|e| AppError::Database(format!("Copy Codex SQLite backup failed: {e}")))?;
    Ok(())
}

fn zip_codex_dir_recursive(
    root: &Path,
    current: &Path,
    writer: &mut zip::ZipWriter<fs::File>,
    options: SimpleFileOptions,
    visited: &mut HashSet<PathBuf>,
    sqlite_snapshots: &HashMap<PathBuf, PathBuf>,
) -> Result<(), AppError> {
    let mut entries: Vec<_> = fs::read_dir(current)
        .map_err(|e| AppError::io(current, e))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| AppError::io(current, e))?;
    entries.sort_by_key(|entry| entry.file_name());

    for entry in entries {
        let path = entry.path();
        if is_symlink(&path)? {
            continue;
        }
        let rel = path.strip_prefix(root).map_err(|e| {
            localized(
                "webdav.sync.codex_zip_relative_path_failed",
                format!("生成 Codex ZIP 相对路径失败: {e}"),
                format!("Failed to build Codex ZIP relative path: {e}"),
            )
        })?;
        if should_skip_codex_rel(rel) {
            continue;
        }

        if path.is_dir() {
            let real_path = fs::canonicalize(&path).unwrap_or(path.clone());
            if !real_path.starts_with(root) || !mark_visited_dir(&real_path, visited)? {
                continue;
            }
            zip_codex_dir_recursive(root, &real_path, writer, options, visited, sqlite_snapshots)?;
            continue;
        }

        let zip_rel = rel.to_string_lossy().replace('\\', "/");
        writer.start_file(&zip_rel, options).map_err(|e| {
            localized(
                "webdav.sync.codex_zip_start_file_failed",
                format!("写入 Codex ZIP 文件头失败: {e}"),
                format!("Failed to start Codex ZIP file entry: {e}"),
            )
        })?;

        let bytes = if let Some(snapshot_path) = sqlite_snapshots.get(rel) {
            fs::read(snapshot_path).map_err(|e| AppError::io(snapshot_path, e))?
        } else {
            fs::read(&path).map_err(|e| AppError::io(&path, e))?
        };
        writer.write_all(&bytes).map_err(|e| {
            localized(
                "webdav.sync.codex_zip_write_file_failed",
                format!("写入 Codex ZIP 文件内容失败: {e}"),
                format!("Failed to write Codex ZIP file content: {e}"),
            )
        })?;
    }

    Ok(())
}

fn build_codex_session_index(config_dir: &Path) -> Result<HashMap<String, PathBuf>, AppError> {
    let mut files = Vec::new();
    for root_name in CODEX_SESSION_ROOTS {
        collect_codex_files(&config_dir.join(root_name), &mut files)?;
    }

    let mut index = HashMap::new();
    for path in files {
        let info = read_codex_session_file_info(&path)?;
        if let Some(session_id) = info.session_id {
            index.insert(session_id, path);
        }
    }
    Ok(index)
}

fn collect_codex_files(root: &Path, files: &mut Vec<PathBuf>) -> Result<(), AppError> {
    if !root.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(root).map_err(|e| AppError::io(root, e))? {
        let entry = entry.map_err(|e| AppError::io(root, e))?;
        let path = entry.path();
        if is_symlink(&path)? {
            continue;
        }
        if path.is_dir() {
            collect_codex_files(&path, files)?;
        } else {
            files.push(path);
        }
    }
    files.sort();
    Ok(())
}

fn read_codex_session_file_info(path: &Path) -> Result<CodexSessionFileInfo, AppError> {
    let bytes = fs::read(path).map_err(|e| AppError::io(path, e))?;
    let hash = sha256_hex_local(&bytes);
    let mut session_id = None;
    let mut last_active_at = None;
    let reader = BufReader::new(bytes.as_slice());

    for line in reader.lines() {
        let line = match line {
            Ok(value) => value,
            Err(_) => continue,
        };
        let value: Value = match serde_json::from_str(&line) {
            Ok(parsed) => parsed,
            Err(_) => continue,
        };

        last_active_at = max_timestamp(
            last_active_at,
            value.get("timestamp").and_then(parse_codex_timestamp_to_ms),
        );

        if value.get("type").and_then(Value::as_str) == Some("session_meta") {
            if let Some(payload) = value.get("payload") {
                if session_id.is_none() {
                    session_id = payload
                        .get("id")
                        .and_then(Value::as_str)
                        .map(str::to_string);
                }
                last_active_at = max_timestamp(
                    last_active_at,
                    payload
                        .get("timestamp")
                        .and_then(parse_codex_timestamp_to_ms),
                );
            }
        }
    }

    Ok(CodexSessionFileInfo {
        session_id,
        last_active_at: last_active_at.or_else(|| file_modified_ms(path)),
        hash,
        bytes,
    })
}

fn is_local_session_newer(local: &CodexSessionFileInfo, incoming: &CodexSessionFileInfo) -> bool {
    match (local.last_active_at, incoming.last_active_at) {
        (Some(local_ts), Some(incoming_ts)) => local_ts > incoming_ts,
        (Some(_), None) => true,
        _ => false,
    }
}

fn merge_codex_sqlite(
    incoming_path: &Path,
    target_path: &Path,
    config_dir: &Path,
    file_name: &str,
) -> Result<(), AppError> {
    let incoming = Connection::open_with_flags(incoming_path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|e| AppError::Database(format!("Open incoming Codex SQLite failed: {e}")))?;
    let target = Connection::open(target_path)
        .map_err(|e| AppError::Database(format!("Open local Codex SQLite failed: {e}")))?;

    let tables = match file_name {
        "state_5.sqlite" => CODEX_STATE_MERGE_TABLES,
        "goals_1.sqlite" => CODEX_GOAL_MERGE_TABLES,
        _ => &[],
    };

    for table in tables {
        merge_sqlite_table(&incoming, &target, table, config_dir)?;
    }
    Ok(())
}

fn merge_sqlite_table(
    incoming: &Connection,
    target: &Connection,
    table: &SqlMergeTable,
    config_dir: &Path,
) -> Result<(), AppError> {
    if !sqlite_table_exists(incoming, table.name)? || !sqlite_table_exists(target, table.name)? {
        return Ok(());
    }

    let incoming_columns = sqlite_table_columns(incoming, table.name)?;
    let target_columns = sqlite_table_columns(target, table.name)?;
    let columns: Vec<String> = incoming_columns
        .into_iter()
        .filter(|column| target_columns.contains(column))
        .collect();
    if columns.is_empty()
        || !table
            .primary_keys
            .iter()
            .all(|pk| columns.iter().any(|column| column == pk))
    {
        return Ok(());
    }

    let timestamp_column = table
        .timestamp_column
        .filter(|column| columns.iter().any(|existing| existing == column))
        .or_else(|| {
            (table.name == "threads")
                .then(|| {
                    ["updated_at", "created_at"]
                        .into_iter()
                        .find(|column| columns.iter().any(|existing| existing == column))
                })
                .flatten()
        });
    let select_sql = format!(
        "SELECT {} FROM {}",
        columns
            .iter()
            .map(|column| quote_ident(column))
            .collect::<Vec<_>>()
            .join(", "),
        quote_ident(table.name)
    );
    let mut stmt = incoming
        .prepare(&select_sql)
        .map_err(|e| AppError::Database(format!("Prepare Codex SQLite select failed: {e}")))?;
    let mut rows = stmt
        .query([])
        .map_err(|e| AppError::Database(format!("Query Codex SQLite rows failed: {e}")))?;

    while let Some(row) = rows
        .next()
        .map_err(|e| AppError::Database(format!("Read Codex SQLite row failed: {e}")))?
    {
        let mut values = Vec::with_capacity(columns.len());
        for idx in 0..columns.len() {
            values.push(
                row.get::<_, SqlValue>(idx).map_err(|e| {
                    AppError::Database(format!("Read Codex SQLite value failed: {e}"))
                })?,
            );
        }

        rewrite_thread_rollout_path(table.name, &columns, &mut values, config_dir);

        if should_replace_sqlite_row(target, table, &columns, &values, timestamp_column)? {
            insert_or_replace_sqlite_row(target, table.name, &columns, &values)?;
        }
    }

    Ok(())
}

fn should_replace_sqlite_row(
    target: &Connection,
    table: &SqlMergeTable,
    columns: &[String],
    values: &[SqlValue],
    timestamp_column: Option<&str>,
) -> Result<bool, AppError> {
    let where_clause = table
        .primary_keys
        .iter()
        .map(|pk| format!("{} = ?", quote_ident(pk)))
        .collect::<Vec<_>>()
        .join(" AND ");
    let key_values = table
        .primary_keys
        .iter()
        .filter_map(|pk| columns.iter().position(|column| column == pk))
        .map(|idx| values[idx].clone())
        .collect::<Vec<_>>();

    if let Some(timestamp_column) = timestamp_column {
        let incoming_ts = columns
            .iter()
            .position(|column| column == timestamp_column)
            .and_then(|idx| sql_value_as_i64(&values[idx]))
            .unwrap_or(0);
        let query = format!(
            "SELECT {} FROM {} WHERE {}",
            quote_ident(timestamp_column),
            quote_ident(table.name),
            where_clause
        );
        let local_ts = target
            .query_row(&query, params_from_iter(key_values.iter()), |row| {
                row.get::<_, SqlValue>(0)
            })
            .optional()
            .map_err(|e| {
                AppError::Database(format!("Read local Codex SQLite timestamp failed: {e}"))
            })?
            .and_then(|value| sql_value_as_i64(&value));
        return Ok(local_ts.map(|ts| incoming_ts >= ts).unwrap_or(true));
    }

    Ok(true)
}

fn insert_or_replace_sqlite_row(
    target: &Connection,
    table_name: &str,
    columns: &[String],
    values: &[SqlValue],
) -> Result<(), AppError> {
    let sql = format!(
        "INSERT OR REPLACE INTO {} ({}) VALUES ({})",
        quote_ident(table_name),
        columns
            .iter()
            .map(|column| quote_ident(column))
            .collect::<Vec<_>>()
            .join(", "),
        std::iter::repeat("?")
            .take(columns.len())
            .collect::<Vec<_>>()
            .join(", ")
    );
    target
        .execute(&sql, params_from_iter(values.iter()))
        .map_err(|e| AppError::Database(format!("Merge Codex SQLite row failed: {e}")))?;
    Ok(())
}

fn rewrite_thread_rollout_path(
    table_name: &str,
    columns: &[String],
    values: &mut [SqlValue],
    config_dir: &Path,
) {
    if table_name != "threads" {
        return;
    }
    let Some(idx) = columns.iter().position(|column| column == "rollout_path") else {
        return;
    };
    let rewritten = match &values[idx] {
        SqlValue::Text(raw) => Some(rewrite_codex_path_to_local(raw, config_dir)),
        _ => None,
    };
    if let Some(rewritten) = rewritten {
        values[idx] = SqlValue::Text(rewritten);
    }
}

fn rewrite_codex_path_to_local(raw: &str, config_dir: &Path) -> String {
    let normalized = raw.replace('\\', "/");
    for marker in ["archived_sessions/", "sessions/"] {
        if let Some(pos) = normalized.find(marker) {
            let rel = &normalized[pos..];
            return config_dir.join(rel).to_string_lossy().to_string();
        }
    }
    raw.to_string()
}

fn rewrite_codex_sqlite_paths(
    target_path: &Path,
    config_dir: &Path,
    file_name: &str,
) -> Result<(), AppError> {
    if file_name != "state_5.sqlite" {
        return Ok(());
    }
    let conn = Connection::open(target_path).map_err(|e| {
        AppError::Database(format!("Open Codex SQLite for path rewrite failed: {e}"))
    })?;
    if !sqlite_table_exists(&conn, "threads")? {
        return Ok(());
    }
    let columns = sqlite_table_columns(&conn, "threads")?;
    if !columns.contains("id") || !columns.contains("rollout_path") {
        return Ok(());
    }

    let updates = {
        let mut stmt = conn
            .prepare("SELECT id, rollout_path FROM threads")
            .map_err(|e| AppError::Database(format!("Prepare Codex path rewrite failed: {e}")))?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|e| AppError::Database(format!("Read Codex paths failed: {e}")))?;
        let mut updates = Vec::new();
        for row in rows {
            let (id, path) =
                row.map_err(|e| AppError::Database(format!("Read Codex path row failed: {e}")))?;
            let rewritten = rewrite_codex_path_to_local(&path, config_dir);
            if rewritten != path {
                updates.push((id, rewritten));
            }
        }
        updates
    };

    for (id, path) in updates {
        conn.execute(
            "UPDATE threads SET rollout_path=?1 WHERE id=?2",
            [&path, &id],
        )
        .map_err(|e| AppError::Database(format!("Rewrite Codex rollout path failed: {e}")))?;
    }
    Ok(())
}

fn sqlite_table_exists(conn: &Connection, table_name: &str) -> Result<bool, AppError> {
    conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
        [table_name],
        |row| row.get::<_, i64>(0),
    )
    .map(|count| count > 0)
    .map_err(|e| AppError::Database(format!("Check Codex SQLite table failed: {e}")))
}

fn sqlite_table_columns(conn: &Connection, table_name: &str) -> Result<HashSet<String>, AppError> {
    let sql = format!("PRAGMA table_info({})", quote_ident(table_name));
    let mut stmt = conn
        .prepare(&sql)
        .map_err(|e| AppError::Database(format!("Read Codex SQLite schema failed: {e}")))?;
    let rows = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|e| AppError::Database(format!("Read Codex SQLite columns failed: {e}")))?;
    let mut columns = HashSet::new();
    for row in rows {
        columns.insert(
            row.map_err(|e| AppError::Database(format!("Read Codex SQLite column failed: {e}")))?,
        );
    }
    Ok(columns)
}

fn quote_ident(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}

fn sql_value_as_i64(value: &SqlValue) -> Option<i64> {
    match value {
        SqlValue::Integer(value) => Some(*value),
        SqlValue::Real(value) => Some(*value as i64),
        SqlValue::Text(value) => value.parse().ok(),
        _ => None,
    }
}

fn backup_codex_file(
    config_dir: &Path,
    source: &Path,
    backup_root: &Path,
) -> Result<Option<PathBuf>, AppError> {
    if !source.exists() {
        return Ok(None);
    }
    let rel = source.strip_prefix(config_dir).unwrap_or_else(|_| {
        source
            .file_name()
            .map(Path::new)
            .unwrap_or_else(|| Path::new("codex-file"))
    });
    let backup_path = backup_root.join(rel);
    if let Some(parent) = backup_path.parent() {
        fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
    }
    fs::copy(source, &backup_path).map_err(|e| AppError::io(&backup_path, e))?;
    Ok(Some(backup_path))
}

fn write_codex_file(target: &Path, bytes: &[u8]) -> Result<(), AppError> {
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
    }
    crate::config::atomic_write(target, bytes)
}

fn should_skip_codex_rel(path: &Path) -> bool {
    let components = path
        .components()
        .filter_map(normal_component)
        .collect::<Vec<_>>();
    let Some(first) = components.first().copied() else {
        return true;
    };
    if CODEX_EXCLUDED_ROOT_DIRS.contains(&first) {
        return true;
    }
    if first == "plugins" && components.get(1).copied() == Some("cache") {
        return true;
    }

    let Some(file_name) = path.file_name().and_then(|value| value.to_str()) else {
        return false;
    };
    CODEX_EXCLUDED_ROOT_FILES.contains(&file_name)
        || file_name.ends_with(".sqlite-wal")
        || file_name.ends_with(".sqlite-shm")
}

fn should_skip_codex_fingerprint_rel(path: &Path) -> bool {
    if is_codex_sqlite_sidecar_rel(path) {
        return false;
    }
    should_skip_codex_rel(path)
}

fn normal_component(component: Component<'_>) -> Option<&str> {
    match component {
        Component::Normal(value) => value.to_str(),
        _ => None,
    }
}

fn is_codex_session_rel(path: &Path) -> bool {
    path.components()
        .next()
        .and_then(normal_component)
        .map(|root| CODEX_SESSION_ROOTS.contains(&root))
        .unwrap_or(false)
}

fn is_codex_sqlite_rel(path: &Path) -> bool {
    path.file_name()
        .and_then(|value| value.to_str())
        .map(|file_name| CODEX_SQLITE_FILES.contains(&file_name))
        .unwrap_or(false)
}

fn is_codex_sqlite_sidecar_rel(path: &Path) -> bool {
    let Some(file_name) = path.file_name().and_then(|value| value.to_str()) else {
        return false;
    };
    CODEX_SQLITE_FILES.iter().any(|db_name| {
        file_name == format!("{db_name}-wal") || file_name == format!("{db_name}-shm")
    })
}

fn parse_codex_timestamp_to_ms(value: &Value) -> Option<i64> {
    value
        .as_str()
        .and_then(|raw| chrono::DateTime::parse_from_rfc3339(raw).ok())
        .map(|ts| ts.timestamp_millis())
}

fn max_timestamp(current: Option<i64>, next: Option<i64>) -> Option<i64> {
    match (current, next) {
        (Some(a), Some(b)) => Some(a.max(b)),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    }
}

fn file_modified_ms(path: &Path) -> Option<i64> {
    let modified = fs::metadata(path).ok()?.modified().ok()?;
    let duration = modified.duration_since(std::time::UNIX_EPOCH).ok()?;
    Some(duration.as_millis().min(i64::MAX as u128) as i64)
}

fn sha256_hex_local(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn is_symlink(path: &Path) -> Result<bool, AppError> {
    Ok(fs::symlink_metadata(path)
        .map_err(|e| AppError::io(path, e))?
        .file_type()
        .is_symlink())
}

fn zip_dir_recursive(
    root: &Path,
    current: &Path,
    writer: &mut zip::ZipWriter<fs::File>,
    options: SimpleFileOptions,
    visited: &mut HashSet<PathBuf>,
) -> Result<(), AppError> {
    let mut entries: Vec<_> = fs::read_dir(current)
        .map_err(|e| AppError::io(current, e))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| AppError::io(current, e))?;
    entries.sort_by_key(|e| e.file_name());

    for entry in entries {
        let path = entry.path();
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        if name_str.starts_with('.') {
            continue;
        }

        let real_path = match fs::canonicalize(&path) {
            Ok(p) if p.starts_with(root) => p,
            Ok(_) => {
                log::warn!(
                    "[WebDAV] Skipping symlink outside skills root: {}",
                    path.display()
                );
                continue;
            }
            Err(_) => path.clone(),
        };

        let rel = real_path
            .strip_prefix(root)
            .or_else(|_| path.strip_prefix(root))
            .map_err(|e| {
                localized(
                    "webdav.sync.zip_relative_path_failed",
                    format!("生成 ZIP 相对路径失败: {e}"),
                    format!("Failed to build relative ZIP path: {e}"),
                )
            })?;
        let rel_str = rel.to_string_lossy().replace('\\', "/");

        if real_path.is_dir() {
            if !mark_visited_dir(&real_path, visited)? {
                log::warn!(
                    "[WebDAV] Skipping already visited directory: {}",
                    real_path.display()
                );
                continue;
            }
            writer
                .add_directory(format!("{rel_str}/"), options)
                .map_err(|e| {
                    localized(
                        "webdav.sync.zip_add_directory_failed",
                        format!("写入 ZIP 目录失败: {e}"),
                        format!("Failed to write ZIP directory entry: {e}"),
                    )
                })?;
            zip_dir_recursive(root, &real_path, writer, options, visited)?;
        } else {
            writer.start_file(&rel_str, options).map_err(|e| {
                localized(
                    "webdav.sync.zip_start_file_failed",
                    format!("写入 ZIP 文件头失败: {e}"),
                    format!("Failed to start ZIP file entry: {e}"),
                )
            })?;
            let mut file = fs::File::open(&real_path).map_err(|e| AppError::io(&real_path, e))?;
            let mut buf = Vec::new();
            file.read_to_end(&mut buf)
                .map_err(|e| AppError::io(&real_path, e))?;
            writer.write_all(&buf).map_err(|e| {
                localized(
                    "webdav.sync.zip_write_file_failed",
                    format!("写入 ZIP 文件内容失败: {e}"),
                    format!("Failed to write ZIP file content: {e}"),
                )
            })?;
        }
    }
    Ok(())
}

fn copy_dir_recursive(src: &Path, dest: &Path) -> Result<(), AppError> {
    let mut visited = HashSet::new();
    copy_dir_recursive_inner(src, dest, &mut visited)
}

fn copy_dir_recursive_inner(
    src: &Path,
    dest: &Path,
    visited: &mut HashSet<PathBuf>,
) -> Result<(), AppError> {
    if !src.exists() {
        return Ok(());
    }
    if !mark_visited_dir(src, visited)? {
        log::warn!(
            "[WebDAV] Skipping already visited copy path: {}",
            src.display()
        );
        return Ok(());
    }
    fs::create_dir_all(dest).map_err(|e| AppError::io(dest, e))?;
    for entry in fs::read_dir(src).map_err(|e| AppError::io(src, e))? {
        let entry = entry.map_err(|e| AppError::io(src, e))?;
        let path = entry.path();
        let dest_path = dest.join(entry.file_name());
        if path.is_dir() {
            copy_dir_recursive_inner(&path, &dest_path, visited)?;
        } else {
            fs::copy(&path, &dest_path).map_err(|e| AppError::io(&dest_path, e))?;
        }
    }
    Ok(())
}

fn mark_visited_dir(path: &Path, visited: &mut HashSet<PathBuf>) -> Result<bool, AppError> {
    let canonical = fs::canonicalize(path).map_err(|e| AppError::io(path, e))?;
    Ok(visited.insert(canonical))
}

fn copy_entry_with_total_limit<R: Read, W: Write>(
    reader: &mut R,
    writer: &mut W,
    total_bytes: &mut u64,
    max_total_bytes: u64,
    out_path: &Path,
) -> Result<u64, AppError> {
    let mut buffer = [0u8; 16 * 1024];
    let mut written = 0u64;
    loop {
        let n = reader
            .read(&mut buffer)
            .map_err(|e| AppError::io(out_path, e))?;
        if n == 0 {
            break;
        }

        if total_bytes.saturating_add(n as u64) > max_total_bytes {
            let max_mb = max_total_bytes / 1024 / 1024;
            return Err(localized(
                "webdav.sync.skills_zip_too_large",
                format!("skills.zip 解压后体积超过上限（{max_mb} MB）"),
                format!("skills.zip extracted size exceeds limit ({max_mb} MB)"),
            ));
        }

        writer
            .write_all(&buffer[..n])
            .map_err(|e| AppError::io(out_path, e))?;
        *total_bytes += n as u64;
        written += n as u64;
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::{
        copy_entry_with_total_limit, mark_visited_dir, restore_codex_data_zip_into_config_dir,
        zip_codex_data_from_config_dir,
    };
    use std::collections::HashSet;
    use std::io::Cursor;
    use std::path::Path;
    use tempfile::tempdir;

    fn codex_session(session_id: &str, timestamp: &str, body: &str) -> String {
        format!(
            "{{\"timestamp\":\"{timestamp}\",\"type\":\"session_meta\",\"payload\":{{\"id\":\"{session_id}\",\"cwd\":\"/tmp/project\"}}}}\n\
             {{\"timestamp\":\"{timestamp}\",\"type\":\"response_item\",\"payload\":{{\"type\":\"message\",\"role\":\"user\",\"content\":\"{body}\"}}}}\n"
        )
    }

    #[test]
    fn mark_visited_dir_tracks_canonical_duplicates() {
        let temp = tempdir().expect("tempdir");
        let dir = temp.path().join("skills");
        std::fs::create_dir_all(&dir).expect("create dir");

        let mut visited = HashSet::new();
        assert!(mark_visited_dir(&dir, &mut visited).expect("first visit"));
        assert!(!mark_visited_dir(&dir, &mut visited).expect("second visit"));
    }

    #[test]
    fn copy_entry_with_total_limit_rejects_oversized_stream_before_write() {
        let mut reader = Cursor::new(vec![1u8; 16]);
        let mut writer = Vec::new();
        let mut total_bytes = 0u64;

        let err = copy_entry_with_total_limit(
            &mut reader,
            &mut writer,
            &mut total_bytes,
            8,
            Path::new("skills-extracted/file.bin"),
        )
        .expect_err("stream larger than limit should be rejected");
        assert!(
            err.to_string().contains("too large") || err.to_string().contains("超过"),
            "unexpected error: {err}"
        );
        assert_eq!(
            writer.len(),
            0,
            "should not write when the first chunk exceeds limit"
        );
    }

    #[test]
    fn zip_codex_data_includes_auth_and_excludes_runtime_cache() {
        let temp = tempdir().expect("tempdir");
        let config_dir = temp.path().join(".codex");
        std::fs::create_dir_all(config_dir.join("sessions/2026/05")).expect("create sessions");
        std::fs::create_dir_all(config_dir.join("plugins/cache/big")).expect("create cache");
        std::fs::create_dir_all(config_dir.join(".tmp")).expect("create tmp");
        std::fs::write(config_dir.join("auth.json"), "{\"token\":\"secret\"}").expect("write auth");
        std::fs::write(
            config_dir.join("sessions/2026/05/session.jsonl"),
            codex_session("session-1", "2026-05-01T00:00:00Z", "hello"),
        )
        .expect("write session");
        std::fs::write(config_dir.join("plugins/cache/big/cache.bin"), "cache")
            .expect("write cache");
        std::fs::write(config_dir.join(".tmp/runtime.txt"), "runtime").expect("write runtime");
        std::fs::write(config_dir.join("logs_2.sqlite"), "logs").expect("write logs");

        let zip_path = temp.path().join("codex.zip");
        zip_codex_data_from_config_dir(&config_dir, &zip_path).expect("zip codex data");

        let file = std::fs::File::open(&zip_path).expect("open zip");
        let mut archive = zip::ZipArchive::new(file).expect("read zip");
        let mut names = Vec::new();
        for idx in 0..archive.len() {
            names.push(archive.by_index(idx).expect("entry").name().to_string());
        }
        names.sort();

        assert!(names.contains(&"auth.json".to_string()));
        assert!(names.contains(&"sessions/2026/05/session.jsonl".to_string()));
        assert!(!names.iter().any(|name| name.starts_with("plugins/cache")));
        assert!(!names.iter().any(|name| name.starts_with(".tmp")));
        assert!(!names.contains(&"logs_2.sqlite".to_string()));
    }

    #[test]
    fn restore_codex_data_merges_sessions_and_preserves_newer_local() {
        let temp = tempdir().expect("tempdir");
        let local_config = temp.path().join("local-codex");
        let remote_config = temp.path().join("remote-codex");
        let local_session = local_config.join("sessions").join("session.jsonl");
        let remote_session = remote_config.join("sessions").join("session.jsonl");
        std::fs::create_dir_all(local_session.parent().unwrap()).expect("local dir");
        std::fs::create_dir_all(remote_session.parent().unwrap()).expect("remote dir");
        std::fs::write(
            &local_session,
            codex_session("same-session", "2026-05-03T00:00:00Z", "local-newer"),
        )
        .expect("write local");
        std::fs::write(
            &remote_session,
            codex_session("same-session", "2026-05-02T00:00:00Z", "remote-older"),
        )
        .expect("write remote");
        std::fs::write(remote_config.join("auth.json"), "{\"token\":\"remote\"}")
            .expect("write remote auth");

        let zip_path = temp.path().join("codex.zip");
        zip_codex_data_from_config_dir(&remote_config, &zip_path).expect("zip remote");
        let raw = std::fs::read(&zip_path).expect("read zip");

        restore_codex_data_zip_into_config_dir(&raw, &local_config).expect("restore zip");

        let restored = std::fs::read_to_string(&local_session).expect("read restored");
        assert!(restored.contains("local-newer"));
        assert_eq!(
            std::fs::read_to_string(local_config.join("auth.json")).expect("read auth"),
            "{\"token\":\"remote\"}"
        );
    }

    #[test]
    fn restore_codex_data_rewrites_thread_rollout_paths() {
        let temp = tempdir().expect("tempdir");
        let local_config = temp.path().join("local-codex");
        let remote_config = temp.path().join("remote-codex");
        std::fs::create_dir_all(&local_config).expect("local dir");
        std::fs::create_dir_all(remote_config.join("sessions/2026/05")).expect("remote sessions");
        std::fs::create_dir_all(remote_config.join("archived_sessions/2026/05"))
            .expect("remote archived sessions");
        std::fs::write(
            remote_config.join("sessions/2026/05/session.jsonl"),
            codex_session("thread-1", "2026-05-02T00:00:00Z", "remote"),
        )
        .expect("write remote session");
        std::fs::write(
            remote_config.join("archived_sessions/2026/05/archived.jsonl"),
            codex_session("thread-2", "2026-05-03T00:00:00Z", "archived"),
        )
        .expect("write remote archived session");

        let db_path = remote_config.join("state_5.sqlite");
        let conn = rusqlite::Connection::open(&db_path).expect("open db");
        conn.execute_batch(
            "CREATE TABLE threads (
                id TEXT PRIMARY KEY,
                rollout_path TEXT NOT NULL,
                updated_at_ms INTEGER NOT NULL
            );
            INSERT INTO threads (id, rollout_path, updated_at_ms)
            VALUES
                ('thread-1', 'C:/Other/.codex/sessions/2026/05/session.jsonl', 10),
                ('thread-2', 'C:/Other/.codex/archived_sessions/2026/05/archived.jsonl', 20);",
        )
        .expect("seed db");

        let zip_path = temp.path().join("codex.zip");
        zip_codex_data_from_config_dir(&remote_config, &zip_path).expect("zip remote");
        let raw = std::fs::read(&zip_path).expect("read zip");

        restore_codex_data_zip_into_config_dir(&raw, &local_config).expect("restore zip");

        let local_db = rusqlite::Connection::open(local_config.join("state_5.sqlite"))
            .expect("open restored db");
        let rollout_path: String = local_db
            .query_row(
                "SELECT rollout_path FROM threads WHERE id='thread-1'",
                [],
                |row| row.get(0),
            )
            .expect("read rollout path");
        assert!(
            rollout_path.starts_with(local_config.to_string_lossy().as_ref()),
            "rollout path should be rewritten to local config dir: {rollout_path}"
        );
        assert!(
            rollout_path
                .replace('\\', "/")
                .contains("/sessions/2026/05/"),
            "active rollout path should stay under sessions: {rollout_path}"
        );
        let archived_rollout_path: String = local_db
            .query_row(
                "SELECT rollout_path FROM threads WHERE id='thread-2'",
                [],
                |row| row.get(0),
            )
            .expect("read archived rollout path");
        assert!(
            archived_rollout_path.starts_with(local_config.to_string_lossy().as_ref()),
            "archived rollout path should be rewritten to local config dir: {archived_rollout_path}"
        );
        assert!(
            archived_rollout_path
                .replace('\\', "/")
                .contains("/archived_sessions/2026/05/"),
            "archived rollout path should stay under archived_sessions: {archived_rollout_path}"
        );
    }
}
