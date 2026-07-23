use std::collections::HashSet;
use std::fs;
use std::io::{Read, Seek, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tempfile::{tempdir, TempDir};
use zip::write::SimpleFileOptions;
use zip::DateTime;

use crate::error::AppError;
use crate::services::skill::SkillService;

use crate::services::sync_protocol::{
    io_context_localized, localized, MAX_SYNC_ARTIFACT_BYTES, REMOTE_SKILLS_ZIP,
};

/// Maximum number of file and directory entries allowed in a Skills archive.
pub(crate) const MAX_SKILLS_ARCHIVE_ENTRIES: u64 = 100_000;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SkillsArchiveStats {
    pub entry_count: u64,
    pub uncompressed_size: u64,
}

impl SkillsArchiveStats {
    fn add_entry(&mut self) -> Result<(), AppError> {
        let next = self
            .entry_count
            .checked_add(1)
            .ok_or_else(skills_archive_stats_overflow)?;
        let candidate = Self {
            entry_count: next,
            ..*self
        };
        validate_skills_archive_stats(candidate)?;
        self.entry_count = next;
        Ok(())
    }

    fn add_uncompressed_bytes(&mut self, bytes: u64) -> Result<(), AppError> {
        let next = self
            .uncompressed_size
            .checked_add(bytes)
            .ok_or_else(skills_archive_stats_overflow)?;
        let candidate = Self {
            uncompressed_size: next,
            ..*self
        };
        validate_skills_archive_stats(candidate)?;
        self.uncompressed_size = next;
        Ok(())
    }
}

pub(crate) struct SkillsBackup {
    _tmp: TempDir,
    backup_dir: PathBuf,
    ssot_path: PathBuf,
    existed: bool,
}

pub(crate) fn zip_skills_ssot(dest_path: &Path) -> Result<SkillsArchiveStats, AppError> {
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
    let mut stats = SkillsArchiveStats::default();

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
            &mut stats,
        )?;
    }

    writer.finish().map_err(|e| {
        localized(
            "webdav.sync.skills_zip_write_failed",
            format!("写入 skills.zip 失败: {e}"),
            format!("Failed to write skills.zip: {e}"),
        )
    })?;
    validate_skills_archive_stats(stats)?;
    Ok(stats)
}

pub(crate) fn restore_skills_zip(raw: &[u8]) -> Result<(), AppError> {
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

    inspect_skills_archive(&mut archive)?;

    let extracted = tmp.path().join("skills-extracted");
    fs::create_dir_all(&extracted).map_err(|e| AppError::io(&extracted, e))?;
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

pub(crate) fn backup_current_skills() -> Result<SkillsBackup, AppError> {
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

pub(crate) fn restore_skills_from_backup(backup: &SkillsBackup) -> Result<(), AppError> {
    if backup.ssot_path.exists() {
        fs::remove_dir_all(&backup.ssot_path).map_err(|e| AppError::io(&backup.ssot_path, e))?;
    }

    if backup.existed {
        copy_dir_recursive(&backup.backup_dir, &backup.ssot_path)?;
    }

    Ok(())
}

fn zip_dir_recursive(
    root: &Path,
    current: &Path,
    writer: &mut zip::ZipWriter<fs::File>,
    options: SimpleFileOptions,
    visited: &mut HashSet<PathBuf>,
    stats: &mut SkillsArchiveStats,
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
            stats.add_entry()?;
            writer
                .add_directory(format!("{rel_str}/"), options)
                .map_err(|e| {
                    localized(
                        "webdav.sync.zip_add_directory_failed",
                        format!("写入 ZIP 目录失败: {e}"),
                        format!("Failed to write ZIP directory entry: {e}"),
                    )
                })?;
            zip_dir_recursive(root, &real_path, writer, options, visited, stats)?;
        } else {
            stats.add_entry()?;
            writer.start_file(&rel_str, options).map_err(|e| {
                localized(
                    "webdav.sync.zip_start_file_failed",
                    format!("写入 ZIP 文件头失败: {e}"),
                    format!("Failed to start ZIP file entry: {e}"),
                )
            })?;
            let mut file = fs::File::open(&real_path).map_err(|e| AppError::io(&real_path, e))?;
            copy_entry_with_total_limit(
                &mut file,
                writer,
                &mut stats.uncompressed_size,
                MAX_SYNC_ARTIFACT_BYTES,
                &real_path,
            )?;
        }
    }
    Ok(())
}

fn inspect_skills_archive<R: Read + Seek>(
    archive: &mut zip::ZipArchive<R>,
) -> Result<SkillsArchiveStats, AppError> {
    let mut stats = SkillsArchiveStats::default();
    for idx in 0..archive.len() {
        let entry = archive.by_index(idx).map_err(|e| {
            localized(
                "webdav.sync.skills_zip_entry_read_failed",
                format!("读取 ZIP 项失败: {e}"),
                format!("Failed to read ZIP entry: {e}"),
            )
        })?;
        stats.add_entry()?;
        if !entry.is_dir() {
            stats.add_uncompressed_bytes(entry.size())?;
        }
    }
    Ok(stats)
}

fn validate_skills_archive_stats(stats: SkillsArchiveStats) -> Result<(), AppError> {
    if stats.entry_count > MAX_SKILLS_ARCHIVE_ENTRIES {
        return Err(localized(
            "webdav.sync.skills_zip_too_many_entries",
            format!(
                "Skills 归档文件/目录条目数过多（{}），上限 {MAX_SKILLS_ARCHIVE_ENTRIES}",
                stats.entry_count
            ),
            format!(
                "Skills archive has too many file/directory entries ({}), limit is {MAX_SKILLS_ARCHIVE_ENTRIES}",
                stats.entry_count
            ),
        ));
    }
    if stats.uncompressed_size > MAX_SYNC_ARTIFACT_BYTES {
        let max_mb = MAX_SYNC_ARTIFACT_BYTES / 1024 / 1024;
        return Err(localized(
            "webdav.sync.skills_zip_too_large",
            format!("Skills 归档预计解压体积超过上限（{max_mb} MB）"),
            format!("Skills archive estimated extracted size exceeds limit ({max_mb} MB)"),
        ));
    }
    Ok(())
}

fn skills_archive_stats_overflow() -> AppError {
    localized(
        "webdav.sync.skills_zip_stats_overflow",
        "Skills 归档规模统计溢出",
        "Skills archive size statistics overflowed",
    )
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
        copy_entry_with_total_limit, inspect_skills_archive, mark_visited_dir,
        validate_skills_archive_stats, zip_dir_recursive, SkillsArchiveStats,
        MAX_SKILLS_ARCHIVE_ENTRIES,
    };
    use std::collections::HashSet;
    use std::fs;
    use std::io::{Cursor, Write};
    use std::path::Path;
    use tempfile::tempdir;
    use zip::write::SimpleFileOptions;
    use zip::DateTime;

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
    fn archive_policy_accepts_limits_and_rejects_excess() {
        let max_bytes = crate::services::sync_protocol::MAX_SYNC_ARTIFACT_BYTES;

        assert!(validate_skills_archive_stats(SkillsArchiveStats {
            entry_count: MAX_SKILLS_ARCHIVE_ENTRIES,
            uncompressed_size: max_bytes,
        })
        .is_ok());

        let entry_err = validate_skills_archive_stats(SkillsArchiveStats {
            entry_count: MAX_SKILLS_ARCHIVE_ENTRIES + 1,
            uncompressed_size: max_bytes,
        })
        .expect_err("entry count above the limit must fail");
        assert!(entry_err.to_string().contains("100001"));

        let size_err = validate_skills_archive_stats(SkillsArchiveStats {
            entry_count: MAX_SKILLS_ARCHIVE_ENTRIES,
            uncompressed_size: max_bytes + 1,
        })
        .expect_err("uncompressed size above the limit must fail");
        assert!(size_err.to_string().contains("512"));
    }

    #[test]
    fn archive_stats_checked_add_rejects_overflow() {
        let mut stats = SkillsArchiveStats {
            entry_count: 0,
            uncompressed_size: u64::MAX,
        };

        assert!(stats.add_uncompressed_bytes(1).is_err());
        assert_eq!(stats.uncompressed_size, u64::MAX);
    }

    #[test]
    fn inspect_skills_archive_reports_entries_and_uncompressed_size() {
        let cursor = Cursor::new(Vec::new());
        let mut writer = zip::ZipWriter::new(cursor);
        let options = SimpleFileOptions::default();
        writer
            .add_directory("examples/", options)
            .expect("add directory");
        writer
            .start_file("examples/a.txt", options)
            .expect("file a");
        writer.write_all(b"hi").expect("write file a");
        writer.start_file("b.txt", options).expect("file b");
        writer.write_all(b"abc").expect("write file b");
        let cursor = writer.finish().expect("finish zip");

        let mut archive = zip::ZipArchive::new(cursor).expect("open zip");
        let stats = inspect_skills_archive(&mut archive).expect("inspect zip");

        assert_eq!(
            stats,
            SkillsArchiveStats {
                entry_count: 3,
                uncompressed_size: 5,
            }
        );
    }

    #[test]
    fn zip_dir_recursive_returns_stats_for_entries_it_writes() {
        let temp = tempdir().expect("tempdir");
        let root = temp.path().join("skills");
        fs::create_dir_all(root.join("examples")).expect("create skill directories");
        fs::write(root.join("examples/a.txt"), b"hi").expect("write nested file");
        fs::write(root.join("b.txt"), b"abc").expect("write root file");

        let zip_path = temp.path().join("skills.zip");
        let file = fs::File::create(&zip_path).expect("create zip");
        let mut writer = zip::ZipWriter::new(file);
        let options = SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .last_modified_time(DateTime::default());
        let canonical_root = fs::canonicalize(&root).expect("canonical root");
        let mut visited = HashSet::new();
        mark_visited_dir(&canonical_root, &mut visited).expect("mark root");
        let mut stats = SkillsArchiveStats::default();

        zip_dir_recursive(
            &canonical_root,
            &canonical_root,
            &mut writer,
            options,
            &mut visited,
            &mut stats,
        )
        .expect("zip skills");
        writer.finish().expect("finish zip");

        assert_eq!(
            stats,
            SkillsArchiveStats {
                entry_count: 3,
                uncompressed_size: 5,
            }
        );
    }
}
