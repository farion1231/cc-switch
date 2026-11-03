use std::collections::HashSet;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use tempfile::{tempdir, TempDir};
use zip::write::SimpleFileOptions;
use zip::DateTime;

use crate::error::AppError;
use crate::services::skill::SkillService;

use crate::services::sync_protocol::{
    io_context_localized, localized, MAX_SYNC_ARTIFACT_BYTES, REMOTE_SKILLS_ZIP,
};

/// Maximum number of entries allowed in a zip archive.
///
/// skills.zip 打包的是用户已安装的全部技能，单个大型技能（如 ppt-master，13k+ 文件）
/// 就能超过原来的 10_000；与 `services/skill.rs` 的 `MAX_ARCHIVE_ENTRIES` 保持一致，
/// 否则技能装得上、同步却恢复不了。字节上限 `MAX_SYNC_ARTIFACT_BYTES` 仍然兜底。
const MAX_EXTRACT_ENTRIES: usize = 30_000;

pub(crate) struct SkillsBackup {
    temporary_dir: Option<TempDir>,
    backup_dir: PathBuf,
    ssot_path: PathBuf,
    existed: bool,
}

pub(crate) fn zip_skills_ssot(dest_path: &Path) -> Result<(), AppError> {
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
    replace_skills_dir(Some(&extracted), &ssot, &RealSkillsFs)
}

// Keep filesystem failures injectable without touching process-global state in tests.
trait SkillsFs {
    fn copy_dir(&self, source: &Path, destination: &Path) -> Result<(), AppError> {
        copy_dir_recursive(source, destination)
    }

    fn rename(&self, source: &Path, destination: &Path) -> std::io::Result<()> {
        fs::rename(source, destination)
    }

    fn remove_dir_all(&self, path: &Path) -> std::io::Result<()> {
        fs::remove_dir_all(path)
    }
}

struct RealSkillsFs;
impl SkillsFs for RealSkillsFs {}

/// Stage on the SSOT filesystem before moving live skills. Only this attempt's
/// directory is ever cleaned up; an older recovery copy may be the only intact one.
fn replace_skills_dir(
    source: Option<&Path>,
    ssot: &Path,
    operations: &impl SkillsFs,
) -> Result<(), AppError> {
    if let Some(source) = source {
        // copy_dir_recursive intentionally skips missing sources in other callers.
        // A missing recovery snapshot must not be interpreted as an empty snapshot.
        if !fs::metadata(source)
            .map_err(|e| AppError::io(source, e))?
            .is_dir()
        {
            return Err(AppError::io(
                source,
                std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "Skills source is not a directory",
                ),
            ));
        }
    }
    let parent = ssot.parent().ok_or_else(|| {
        AppError::io(
            ssot,
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Skills path has no parent",
            ),
        )
    })?;
    let transaction = tempfile::Builder::new()
        .prefix(".skills-restore-")
        .tempdir_in(parent)
        .map_err(|e| AppError::io(parent, e))?;
    let staged = transaction.path().join("new");
    let previous = transaction.path().join("previous");
    if let Some(source) = source {
        operations.copy_dir(source, &staged)?;
    }

    // Disable automatic cleanup BEFORE the first rename: once live skills move
    // here, even unwinding must not delete the only complete original tree.
    let transaction = transaction.keep();
    let had_live = ssot.exists();
    if had_live {
        if let Err(error) = operations.rename(ssot, &previous) {
            cleanup_skills_transaction(&transaction, operations);
            return Err(skills_rename_error(ssot, &previous, error));
        }
    }

    if source.is_some() {
        if let Err(error) = operations.rename(&staged, ssot) {
            let install_error = skills_rename_error(&staged, ssot, error);
            if had_live {
                // Do not delete a live path that appeared meanwhile. A conflict
                // must leave the original tree in its recovery directory.
                if let Err(error) = operations.rename(&previous, ssot) {
                    let rollback_error = skills_rename_error(&previous, ssot, error);
                    return Err(localized(
                        "webdav.sync.skills_replace_and_rollback_failed",
                        format!("替换 Skills 失败: {install_error}; 回滚也失败: {rollback_error}; 原 Skills 保留在 {}", previous.display()),
                        format!("Skills replacement failed: {install_error}; rollback also failed: {rollback_error}; original skills retained at {}", previous.display()),
                    ));
                }
            }
            cleanup_skills_transaction(&transaction, operations);
            return Err(install_error);
        }
    }

    // Files are committed. Cleanup failure must not report a failed restore and
    // cause apply_snapshot to skip the matching database import.
    cleanup_skills_transaction(&transaction, operations);
    Ok(())
}

fn skills_rename_error(source: &Path, destination: &Path, error: std::io::Error) -> AppError {
    io_context_localized(
        "webdav.sync.skills_rename_failed",
        format!(
            "移动 Skills 目录失败（{} → {}）",
            source.display(),
            destination.display()
        ),
        format!(
            "Failed to move skills directory ({} -> {})",
            source.display(),
            destination.display()
        ),
        error,
    )
}

fn cleanup_skills_transaction(path: &Path, operations: &impl SkillsFs) {
    if let Err(error) = operations.remove_dir_all(path) {
        // remove_dir_all may have removed only part of the directory.
        log::warn!(
            "[Sync] Skills restore cleanup failed at {}: {error}",
            path.display()
        );
    }
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
        temporary_dir: Some(tmp),
        backup_dir,
        ssot_path: ssot,
        existed,
    })
}

pub(crate) fn restore_skills_from_backup(backup: &SkillsBackup) -> Result<(), AppError> {
    replace_skills_dir(
        backup.existed.then_some(backup.backup_dir.as_path()),
        &backup.ssot_path,
        &RealSkillsFs,
    )
}

impl SkillsBackup {
    pub(crate) fn rollback_after_import_error(self, db_error: AppError) -> AppError {
        self.rollback_after_import_error_with(db_error, restore_skills_from_backup)
    }

    fn rollback_after_import_error_with(
        mut self,
        db_error: AppError,
        rollback: impl FnOnce(&SkillsBackup) -> Result<(), AppError>,
    ) -> AppError {
        // Retain the pre-import snapshot before rollback can move files or panic.
        // A replacement helper's recovery tree may contain the remote snapshot.
        let recovery_container = self.temporary_dir.take().map(TempDir::keep);
        if let Err(rollback_error) = rollback(&self) {
            let recovery_path = self.backup_dir.clone();
            return localized(
                "sync.db_import_and_rollback_failed",
                format!("导入数据库失败: {db_error}; 同时回滚 Skills 失败: {rollback_error}; 导入前的 Skills 快照保留在 {}", recovery_path.display()),
                format!("Database import failed: {db_error}; skills rollback also failed: {rollback_error}; pre-import skills snapshot retained at {}", recovery_path.display()),
            );
        }
        if let Some(path) = recovery_container {
            cleanup_skills_transaction(&path, &RealSkillsFs);
        }
        db_error
    }
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
    use super::{copy_entry_with_total_limit, mark_visited_dir};
    use std::collections::HashSet;
    use std::io::Cursor;
    use std::path::Path;
    use tempfile::tempdir;

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

    #[derive(Default)]
    struct FaultySkillsFs {
        rename_calls: std::cell::Cell<usize>,
        fail_renames: Vec<usize>,
        fail_copy: bool,
        fail_cleanup: bool,
        panic_on_install: bool,
        create_conflict_on_install: bool,
    }

    impl super::SkillsFs for FaultySkillsFs {
        fn copy_dir(
            &self,
            source: &Path,
            destination: &Path,
        ) -> Result<(), crate::error::AppError> {
            if self.fail_copy {
                std::fs::create_dir_all(destination).unwrap();
                std::fs::write(destination.join("partial.txt"), "partial").unwrap();
                return Err(crate::error::AppError::io(
                    destination,
                    std::io::Error::other("injected stage copy failure"),
                ));
            }
            super::copy_dir_recursive(source, destination)
        }

        fn rename(&self, source: &Path, destination: &Path) -> std::io::Result<()> {
            let call = self.rename_calls.get() + 1;
            self.rename_calls.set(call);
            if call == 2 {
                assert!(
                    !self.panic_on_install,
                    "injected panic after moving live skills"
                );
                if self.create_conflict_on_install {
                    std::fs::create_dir_all(destination)?;
                    std::fs::write(destination.join("concurrent.txt"), "concurrent")?;
                }
            }
            if self.fail_renames.contains(&call) {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    format!("injected rename failure {call}"),
                ));
            }
            std::fs::rename(source, destination)
        }

        fn remove_dir_all(&self, path: &Path) -> std::io::Result<()> {
            if self.fail_cleanup {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "injected cleanup failure",
                ));
            }
            std::fs::remove_dir_all(path)
        }
    }

    struct RestoreFixture {
        root: tempfile::TempDir,
        source: std::path::PathBuf,
        live: std::path::PathBuf,
    }

    impl RestoreFixture {
        fn new() -> Self {
            let root = tempdir().unwrap();
            let source = root.path().join("source");
            let live = root.path().join("skills");
            std::fs::create_dir(&source).unwrap();
            std::fs::write(source.join("remote.txt"), "remote skills").unwrap();
            std::fs::create_dir(&live).unwrap();
            std::fs::write(live.join("original.txt"), "original skills").unwrap();
            Self { root, source, live }
        }

        fn transactions(&self) -> Vec<std::path::PathBuf> {
            std::fs::read_dir(self.root.path())
                .unwrap()
                .map(|entry| entry.unwrap().path())
                .filter(|path| {
                    path.file_name()
                        .unwrap()
                        .to_string_lossy()
                        .starts_with(".skills-restore-")
                })
                .collect()
        }

        fn assert_original(&self) {
            assert_eq!(
                std::fs::read_to_string(self.live.join("original.txt")).unwrap(),
                "original skills"
            );
            assert!(!self.live.join("remote.txt").exists());
        }

        fn backup(&self) -> super::SkillsBackup {
            let tmp = tempdir().unwrap();
            let backup_dir = tmp.path().join("skills-backup");
            super::copy_dir_recursive(&self.live, &backup_dir).unwrap();
            super::SkillsBackup {
                temporary_dir: Some(tmp),
                backup_dir,
                ssot_path: self.live.clone(),
                existed: true,
            }
        }
    }

    #[test]
    fn skills_restore_staging_failure_keeps_live_and_cleans_partial_stage() {
        let fixture = RestoreFixture::new();
        let operations = FaultySkillsFs {
            fail_copy: true,
            ..Default::default()
        };
        let error = super::replace_skills_dir(Some(&fixture.source), &fixture.live, &operations)
            .unwrap_err();
        assert!(error.to_string().contains("injected stage copy failure"));
        fixture.assert_original();
        assert_eq!(operations.rename_calls.get(), 0);
        assert!(fixture.transactions().is_empty());
    }

    #[test]
    fn skills_restore_first_rename_failure_keeps_live_and_reports_both_paths() {
        let fixture = RestoreFixture::new();
        let operations = FaultySkillsFs {
            fail_renames: vec![1],
            ..Default::default()
        };
        let error = super::replace_skills_dir(Some(&fixture.source), &fixture.live, &operations)
            .unwrap_err()
            .to_string();
        fixture.assert_original();
        assert!(error.contains(&fixture.live.display().to_string()));
        assert!(error.contains("previous"));
        assert!(error.contains("injected rename failure 1"));
        assert_eq!(operations.rename_calls.get(), 1);
        assert!(fixture.transactions().is_empty());
    }

    #[test]
    fn skills_restore_install_failure_rolls_back_original() {
        let fixture = RestoreFixture::new();
        let operations = FaultySkillsFs {
            fail_renames: vec![2],
            ..Default::default()
        };
        let error = super::replace_skills_dir(Some(&fixture.source), &fixture.live, &operations)
            .unwrap_err();
        assert!(error.to_string().contains("injected rename failure 2"));
        fixture.assert_original();
        assert_eq!(operations.rename_calls.get(), 3);
        assert!(fixture.transactions().is_empty());
    }

    #[test]
    fn skills_restore_failed_rollback_retains_original_across_retry() {
        let fixture = RestoreFixture::new();
        let operations = FaultySkillsFs {
            fail_renames: vec![2, 3],
            ..Default::default()
        };
        let error = super::replace_skills_dir(Some(&fixture.source), &fixture.live, &operations)
            .unwrap_err()
            .to_string();
        let transactions = fixture.transactions();
        assert_eq!(transactions.len(), 1);
        let recovery = transactions[0].join("previous");
        assert!(error.contains(&recovery.display().to_string()));
        assert!(error.contains("injected rename failure 2"));
        assert!(error.contains("injected rename failure 3"));
        assert!(!fixture.live.exists());
        assert_eq!(
            std::fs::read_to_string(recovery.join("original.txt")).unwrap(),
            "original skills"
        );
        // get_ssot_dir recreates an absent live directory on the next attempt.
        std::fs::create_dir(&fixture.live).unwrap();
        std::fs::write(fixture.live.join("partial.txt"), "partial").unwrap();
        let failure = FaultySkillsFs {
            fail_copy: true,
            ..Default::default()
        };
        assert!(super::replace_skills_dir(Some(&fixture.source), &fixture.live, &failure).is_err());
        super::replace_skills_dir(Some(&fixture.source), &fixture.live, &super::RealSkillsFs)
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(recovery.join("original.txt")).unwrap(),
            "original skills"
        );
        assert_eq!(
            std::fs::read_to_string(fixture.live.join("remote.txt")).unwrap(),
            "remote skills"
        );
    }

    #[test]
    fn skills_restore_conflicting_live_path_is_not_deleted_during_rollback() {
        let fixture = RestoreFixture::new();
        let operations = FaultySkillsFs {
            create_conflict_on_install: true,
            ..Default::default()
        };
        assert!(
            super::replace_skills_dir(Some(&fixture.source), &fixture.live, &operations).is_err()
        );
        assert_eq!(
            std::fs::read_to_string(fixture.live.join("concurrent.txt")).unwrap(),
            "concurrent"
        );
        assert_eq!(
            std::fs::read_to_string(fixture.transactions()[0].join("previous/original.txt"))
                .unwrap(),
            "original skills"
        );
    }

    #[test]
    fn skills_restore_unwind_after_live_move_does_not_delete_original() {
        let fixture = RestoreFixture::new();
        let operations = FaultySkillsFs {
            panic_on_install: true,
            ..Default::default()
        };
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            super::replace_skills_dir(Some(&fixture.source), &fixture.live, &operations)
        }));
        assert!(result.is_err());
        assert_eq!(
            std::fs::read_to_string(fixture.transactions()[0].join("previous/original.txt"))
                .unwrap(),
            "original skills"
        );
    }

    #[test]
    fn skills_restore_successful_install_cleanup_failure_still_succeeds() {
        let fixture = RestoreFixture::new();
        let operations = FaultySkillsFs {
            fail_cleanup: true,
            ..Default::default()
        };
        super::replace_skills_dir(Some(&fixture.source), &fixture.live, &operations).unwrap();
        assert_eq!(
            std::fs::read_to_string(fixture.live.join("remote.txt")).unwrap(),
            "remote skills"
        );
        assert_eq!(fixture.transactions().len(), 1);
    }

    #[test]
    fn skills_restore_never_touches_legacy_backup() {
        let fixture = RestoreFixture::new();
        let legacy = fixture.live.with_extension("bak");
        std::fs::create_dir(&legacy).unwrap();
        std::fs::write(legacy.join("only-copy.txt"), "recover me").unwrap();
        // An open handle can also prevent cleanup on Windows; no operation should
        // address this backup, regardless of platform-specific sharing semantics.
        let _held_file = std::fs::File::open(legacy.join("only-copy.txt")).unwrap();
        super::replace_skills_dir(Some(&fixture.source), &fixture.live, &super::RealSkillsFs)
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(legacy.join("only-copy.txt")).unwrap(),
            "recover me"
        );
    }

    #[test]
    fn skills_restore_missing_or_file_source_does_not_replace_live() {
        let fixture = RestoreFixture::new();
        for source in [
            fixture.root.path().join("missing"),
            fixture.source.join("remote.txt"),
        ] {
            assert!(
                super::replace_skills_dir(Some(&source), &fixture.live, &super::RealSkillsFs)
                    .is_err()
            );
            fixture.assert_original();
        }
        assert!(fixture.transactions().is_empty());
    }

    #[test]
    fn skills_restore_supports_absent_live_and_absent_snapshot() {
        let fixture = RestoreFixture::new();
        std::fs::remove_dir_all(&fixture.live).unwrap();
        super::replace_skills_dir(Some(&fixture.source), &fixture.live, &super::RealSkillsFs)
            .unwrap();
        assert!(fixture.live.join("remote.txt").exists());
        super::replace_skills_dir(None, &fixture.live, &super::RealSkillsFs).unwrap();
        assert!(!fixture.live.exists());
        assert!(fixture.transactions().is_empty());
    }

    #[test]
    fn skills_restore_failed_install_without_live_leaves_no_live() {
        let fixture = RestoreFixture::new();
        std::fs::remove_dir_all(&fixture.live).unwrap();
        let operations = FaultySkillsFs {
            fail_renames: vec![1],
            ..Default::default()
        };
        assert!(
            super::replace_skills_dir(Some(&fixture.source), &fixture.live, &operations).is_err()
        );
        assert!(!fixture.live.exists());
        assert!(fixture.transactions().is_empty());
    }

    #[test]
    fn skills_import_rollback_success_restores_snapshot_and_returns_database_error() {
        let fixture = RestoreFixture::new();
        let backup = fixture.backup();
        let temporary_backup = backup.temporary_dir.as_ref().unwrap().path().to_path_buf();
        super::replace_skills_dir(Some(&fixture.source), &fixture.live, &super::RealSkillsFs)
            .unwrap();
        let error = backup.rollback_after_import_error(crate::error::AppError::Message(
            "database import failed".into(),
        ));
        assert_eq!(error.to_string(), "database import failed");
        fixture.assert_original();
        assert!(!temporary_backup.exists());
    }

    #[test]
    fn skills_import_failed_rollback_preserves_pre_import_snapshot_after_drop() {
        let fixture = RestoreFixture::new();
        let backup = fixture.backup();
        let original_snapshot = backup.backup_dir.clone();
        let temporary_backup = backup.temporary_dir.as_ref().unwrap().path().to_path_buf();
        super::replace_skills_dir(Some(&fixture.source), &fixture.live, &super::RealSkillsFs)
            .unwrap();
        let operations = FaultySkillsFs {
            fail_renames: vec![2, 3],
            ..Default::default()
        };
        let error = backup
            .rollback_after_import_error_with(
                crate::error::AppError::Message("database import failed".into()),
                |backup| {
                    super::replace_skills_dir(
                        Some(&backup.backup_dir),
                        &backup.ssot_path,
                        &operations,
                    )
                },
            )
            .to_string();
        assert!(error.contains("database import failed"));
        assert!(error.contains(&original_snapshot.display().to_string()));
        assert!(error.contains("pre-import"));
        assert_eq!(
            std::fs::read_to_string(original_snapshot.join("original.txt")).unwrap(),
            "original skills"
        );
        // The helper's recovery tree is the remote snapshot, not the original one.
        assert_eq!(
            std::fs::read_to_string(fixture.transactions()[0].join("previous/remote.txt")).unwrap(),
            "remote skills"
        );
        std::fs::remove_dir_all(temporary_backup).unwrap();
    }
    #[test]
    fn skills_import_rollback_unwind_preserves_pre_import_snapshot() {
        let fixture = RestoreFixture::new();
        let backup = fixture.backup();
        let original_snapshot = backup.backup_dir.clone();
        let temporary_backup = backup.temporary_dir.as_ref().unwrap().path().to_path_buf();
        super::replace_skills_dir(Some(&fixture.source), &fixture.live, &super::RealSkillsFs)
            .unwrap();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            backup.rollback_after_import_error_with(
                crate::error::AppError::Message("database import failed".into()),
                |_| panic!("injected rollback panic"),
            )
        }));
        assert!(result.is_err());
        assert_eq!(
            std::fs::read_to_string(original_snapshot.join("original.txt")).unwrap(),
            "original skills"
        );
        std::fs::remove_dir_all(temporary_backup).unwrap();
    }
}
