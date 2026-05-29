use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};

use chrono::Utc;
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
const CODEX_SESSION_ROOTS: [&str; 2] = ["sessions", "archived_sessions"];
const CODEX_SESSION_BACKUP_DIR: &str = "session_sync_backups";

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

pub(super) fn zip_codex_sessions(dest_path: &Path) -> Result<(), AppError> {
    zip_codex_sessions_from_config_dir(&get_codex_config_dir(), dest_path)
}

fn zip_codex_sessions_from_config_dir(config_dir: &Path, dest_path: &Path) -> Result<(), AppError> {
    if let Some(parent) = dest_path.parent() {
        fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
    }

    let file = fs::File::create(dest_path).map_err(|e| AppError::io(dest_path, e))?;
    let mut writer = zip::ZipWriter::new(file);
    let options = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .last_modified_time(DateTime::default());

    for root_name in CODEX_SESSION_ROOTS {
        let root_path = config_dir.join(root_name);
        if !root_path.exists() {
            continue;
        }

        let canonical_root = fs::canonicalize(&root_path).unwrap_or(root_path.clone());
        let mut visited = HashSet::new();
        mark_visited_dir(&canonical_root, &mut visited)?;
        zip_codex_session_dir_recursive(
            root_name,
            &canonical_root,
            &canonical_root,
            &mut writer,
            options,
            &mut visited,
        )?;
    }

    writer.finish().map_err(|e| {
        localized(
            "webdav.sync.codex_sessions_zip_write_failed",
            format!("写入 codex-sessions.zip 失败: {e}"),
            format!("Failed to write codex-sessions.zip: {e}"),
        )
    })?;
    Ok(())
}

pub(super) fn restore_codex_sessions_zip(raw: &[u8]) -> Result<(), AppError> {
    restore_codex_sessions_zip_into_config_dir(raw, &get_codex_config_dir())
}

fn restore_codex_sessions_zip_into_config_dir(
    raw: &[u8],
    config_dir: &Path,
) -> Result<(), AppError> {
    let tmp = tempdir().map_err(|e| {
        io_context_localized(
            "webdav.sync.codex_sessions_extract_tmpdir_failed",
            "创建 Codex 会话解压临时目录失败",
            "Failed to create temporary directory for Codex session extraction",
            e,
        )
    })?;
    let zip_path = tmp.path().join("codex-sessions.zip");
    fs::write(&zip_path, raw).map_err(|e| AppError::io(&zip_path, e))?;

    let file = fs::File::open(&zip_path).map_err(|e| AppError::io(&zip_path, e))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| {
        localized(
            "webdav.sync.codex_sessions_zip_parse_failed",
            format!("解析 codex-sessions.zip 失败: {e}"),
            format!("Failed to parse codex-sessions.zip: {e}"),
        )
    })?;

    if archive.len() > MAX_EXTRACT_ENTRIES {
        return Err(localized(
            "webdav.sync.codex_sessions_zip_too_many_entries",
            format!(
                "codex-sessions.zip 条目数过多（{}），上限 {MAX_EXTRACT_ENTRIES}",
                archive.len()
            ),
            format!(
                "codex-sessions.zip has too many entries ({}), limit is {MAX_EXTRACT_ENTRIES}",
                archive.len()
            ),
        ));
    }

    let extracted = tmp.path().join("codex-sessions-extracted");
    fs::create_dir_all(&extracted).map_err(|e| AppError::io(&extracted, e))?;

    let mut total_bytes: u64 = 0;
    for idx in 0..archive.len() {
        let mut entry = archive.by_index(idx).map_err(|e| {
            localized(
                "webdav.sync.codex_sessions_zip_entry_read_failed",
                format!("读取 Codex 会话 ZIP 项失败: {e}"),
                format!("Failed to read Codex session ZIP entry: {e}"),
            )
        })?;
        if entry.is_dir() {
            continue;
        }
        let Some(safe_name) = entry.enclosed_name() else {
            continue;
        };
        if !is_allowed_codex_session_rel(&safe_name) {
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

    restore_codex_sessions_from_dir(&extracted, config_dir)
}

#[derive(Debug)]
struct CodexSessionFileInfo {
    session_id: Option<String>,
    last_active_at: Option<i64>,
    hash: String,
    bytes: Vec<u8>,
}

fn restore_codex_sessions_from_dir(extracted: &Path, config_dir: &Path) -> Result<(), AppError> {
    let mut incoming_files = Vec::new();
    collect_codex_jsonl_files(extracted, &mut incoming_files)?;
    if incoming_files.is_empty() {
        return Ok(());
    }

    let mut local_index = build_codex_session_index(config_dir)?;
    let backup_root = config_dir
        .join(CODEX_SESSION_BACKUP_DIR)
        .join(Utc::now().format("%Y%m%dT%H%M%S%.3fZ").to_string());

    for source in incoming_files {
        let rel = source.strip_prefix(extracted).map_err(|e| {
            localized(
                "webdav.sync.codex_sessions_relative_path_failed",
                format!("生成 Codex 会话相对路径失败: {e}"),
                format!("Failed to build Codex session relative path: {e}"),
            )
        })?;

        if let Some((session_id, target_path)) =
            merge_codex_session_file(&source, rel, config_dir, &local_index, &backup_root)?
        {
            local_index.insert(session_id, target_path);
        }
    }

    Ok(())
}

fn merge_codex_session_file(
    incoming_path: &Path,
    rel: &Path,
    config_dir: &Path,
    local_index: &HashMap<String, PathBuf>,
    backup_root: &Path,
) -> Result<Option<(String, PathBuf)>, AppError> {
    let incoming = read_codex_session_file_info(incoming_path)?;
    let default_target = config_dir.join(rel);
    let existing_by_id = incoming
        .session_id
        .as_ref()
        .and_then(|session_id| local_index.get(session_id))
        .cloned();
    let compare_path = existing_by_id.unwrap_or_else(|| default_target.clone());

    if compare_path.exists() {
        if is_symlink(&compare_path)? {
            log::warn!(
                "[WebDAV] Skipping Codex session restore over symlink: {}",
                compare_path.display()
            );
            return Ok(None);
        }

        let local = read_codex_session_file_info(&compare_path)?;
        if local.hash == incoming.hash {
            return Ok(None);
        }

        let local_ts = local.last_active_at.unwrap_or(0);
        let incoming_ts = incoming.last_active_at.unwrap_or(0);
        if local_ts > incoming_ts {
            return Ok(None);
        }

        backup_codex_session_file(config_dir, &compare_path, backup_root)?;
        if compare_path != default_target {
            fs::remove_file(&compare_path).map_err(|e| AppError::io(&compare_path, e))?;
        }
    }

    if default_target.exists() {
        if is_symlink(&default_target)? {
            log::warn!(
                "[WebDAV] Skipping Codex session restore over symlink: {}",
                default_target.display()
            );
            return Ok(None);
        }
        if default_target != compare_path {
            backup_codex_session_file(config_dir, &default_target, backup_root)?;
        }
    }

    write_codex_session_file(&default_target, &incoming.bytes)?;
    Ok(incoming
        .session_id
        .map(|session_id| (session_id, default_target)))
}

fn zip_codex_session_dir_recursive(
    root_name: &str,
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
    entries.sort_by_key(|entry| entry.file_name());

    for entry in entries {
        let path = entry.path();
        if is_symlink(&path)? {
            continue;
        }

        if path.is_dir() {
            let real_path = fs::canonicalize(&path).unwrap_or(path.clone());
            if !real_path.starts_with(root) || !mark_visited_dir(&real_path, visited)? {
                continue;
            }
            zip_codex_session_dir_recursive(root_name, root, &real_path, writer, options, visited)?;
            continue;
        }

        if path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
            continue;
        }

        let real_path = fs::canonicalize(&path).unwrap_or(path.clone());
        if !real_path.starts_with(root) {
            continue;
        }

        let rel = real_path
            .strip_prefix(root)
            .or_else(|_| path.strip_prefix(root))
            .map_err(|e| {
                localized(
                    "webdav.sync.codex_sessions_zip_relative_path_failed",
                    format!("生成 Codex 会话 ZIP 相对路径失败: {e}"),
                    format!("Failed to build Codex session ZIP relative path: {e}"),
                )
            })?;
        let zip_rel = Path::new(root_name).join(rel);
        let zip_rel_str = zip_rel.to_string_lossy().replace('\\', "/");

        writer.start_file(&zip_rel_str, options).map_err(|e| {
            localized(
                "webdav.sync.codex_sessions_zip_start_file_failed",
                format!("写入 Codex 会话 ZIP 文件头失败: {e}"),
                format!("Failed to start Codex session ZIP file entry: {e}"),
            )
        })?;
        let mut file = fs::File::open(&real_path).map_err(|e| AppError::io(&real_path, e))?;
        let mut buf = Vec::new();
        file.read_to_end(&mut buf)
            .map_err(|e| AppError::io(&real_path, e))?;
        writer.write_all(&buf).map_err(|e| {
            localized(
                "webdav.sync.codex_sessions_zip_write_file_failed",
                format!("写入 Codex 会话 ZIP 文件内容失败: {e}"),
                format!("Failed to write Codex session ZIP file content: {e}"),
            )
        })?;
    }

    Ok(())
}

fn build_codex_session_index(config_dir: &Path) -> Result<HashMap<String, PathBuf>, AppError> {
    let mut files = Vec::new();
    for root_name in CODEX_SESSION_ROOTS {
        collect_codex_jsonl_files(&config_dir.join(root_name), &mut files)?;
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

fn collect_codex_jsonl_files(root: &Path, files: &mut Vec<PathBuf>) -> Result<(), AppError> {
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
            collect_codex_jsonl_files(&path, files)?;
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("jsonl") {
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

fn backup_codex_session_file(
    config_dir: &Path,
    source: &Path,
    backup_root: &Path,
) -> Result<(), AppError> {
    let rel = source.strip_prefix(config_dir).unwrap_or_else(|_| {
        source
            .file_name()
            .map(Path::new)
            .unwrap_or_else(|| Path::new("session.jsonl"))
    });
    let backup_path = backup_root.join(rel);
    if let Some(parent) = backup_path.parent() {
        fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
    }
    fs::copy(source, &backup_path).map_err(|e| AppError::io(&backup_path, e))?;
    Ok(())
}

fn write_codex_session_file(target: &Path, bytes: &[u8]) -> Result<(), AppError> {
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
    }
    crate::config::atomic_write(target, bytes)
}

fn is_allowed_codex_session_rel(path: &Path) -> bool {
    let Some(root) = path
        .components()
        .next()
        .and_then(|component| match component {
            std::path::Component::Normal(value) => value.to_str(),
            _ => None,
        })
    else {
        return false;
    };
    CODEX_SESSION_ROOTS.contains(&root)
        && path.extension().and_then(|ext| ext.to_str()) == Some("jsonl")
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
        copy_entry_with_total_limit, mark_visited_dir, restore_codex_sessions_zip_into_config_dir,
        zip_codex_sessions_from_config_dir,
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
    fn zip_codex_sessions_includes_jsonl_under_session_roots_only() {
        let temp = tempdir().expect("tempdir");
        let config_dir = temp.path().join(".codex");
        let session_dir = config_dir.join("sessions").join("2026").join("05");
        std::fs::create_dir_all(&session_dir).expect("create sessions");
        std::fs::write(
            session_dir.join("session.jsonl"),
            codex_session("session-1", "2026-05-01T00:00:00Z", "hello"),
        )
        .expect("write session");
        std::fs::write(session_dir.join("ignore.txt"), "ignore").expect("write ignored");
        std::fs::create_dir_all(config_dir.join("logs")).expect("create logs");
        std::fs::write(config_dir.join("logs").join("other.jsonl"), "{}")
            .expect("write outside session root");

        let zip_path = temp.path().join("codex-sessions.zip");
        zip_codex_sessions_from_config_dir(&config_dir, &zip_path).expect("zip sessions");

        let file = std::fs::File::open(&zip_path).expect("open zip");
        let mut archive = zip::ZipArchive::new(file).expect("read zip");
        let mut names = Vec::new();
        for idx in 0..archive.len() {
            names.push(archive.by_index(idx).expect("entry").name().to_string());
        }

        assert_eq!(names, vec!["sessions/2026/05/session.jsonl"]);
    }

    #[test]
    fn restore_codex_sessions_zip_uses_newer_remote_and_backs_up_local() {
        let temp = tempdir().expect("tempdir");
        let local_config = temp.path().join("local-codex");
        let remote_config = temp.path().join("remote-codex");
        let local_session = local_config
            .join("sessions")
            .join("2026")
            .join("05")
            .join("session.jsonl");
        let remote_session = remote_config
            .join("sessions")
            .join("2026")
            .join("05")
            .join("session.jsonl");
        std::fs::create_dir_all(local_session.parent().unwrap()).expect("local dir");
        std::fs::create_dir_all(remote_session.parent().unwrap()).expect("remote dir");
        std::fs::write(
            &local_session,
            codex_session("same-session", "2026-05-01T00:00:00Z", "local"),
        )
        .expect("write local");
        std::fs::write(
            &remote_session,
            codex_session("same-session", "2026-05-02T00:00:00Z", "remote"),
        )
        .expect("write remote");

        let zip_path = temp.path().join("codex-sessions.zip");
        zip_codex_sessions_from_config_dir(&remote_config, &zip_path).expect("zip remote");
        let raw = std::fs::read(&zip_path).expect("read zip");

        restore_codex_sessions_zip_into_config_dir(&raw, &local_config).expect("restore zip");

        let restored = std::fs::read_to_string(&local_session).expect("read restored");
        assert!(restored.contains("remote"));

        let backup_root = local_config.join("session_sync_backups");
        let backups = std::fs::read_dir(&backup_root)
            .expect("backup root should exist")
            .flat_map(|entry| {
                let entry = entry.expect("backup entry");
                let path = entry.path().join("sessions/2026/05/session.jsonl");
                std::fs::read_to_string(path).ok()
            })
            .collect::<Vec<_>>();
        assert!(
            backups.iter().any(|content| content.contains("local")),
            "expected replaced local session to be backed up"
        );
    }

    #[test]
    fn restore_codex_sessions_zip_keeps_newer_local_session() {
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

        let zip_path = temp.path().join("codex-sessions.zip");
        zip_codex_sessions_from_config_dir(&remote_config, &zip_path).expect("zip remote");
        let raw = std::fs::read(&zip_path).expect("read zip");

        restore_codex_sessions_zip_into_config_dir(&raw, &local_config).expect("restore zip");

        let restored = std::fs::read_to_string(&local_session).expect("read restored");
        assert!(restored.contains("local-newer"));
        assert!(!local_config.join("session_sync_backups").exists());
    }
}
