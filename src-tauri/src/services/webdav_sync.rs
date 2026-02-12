//! WebDAV v2 sync protocol layer.
//!
//! Implements manifest-based synchronization on top of the HTTP transport
//! primitives in [`super::webdav`]. Artifact set: `db.sql` + `skills.zip`.

use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tempfile::{tempdir, TempDir};
use zip::write::SimpleFileOptions;
use zip::DateTime;

use crate::error::AppError;
use crate::services::skill::SkillService;
use crate::services::webdav::{
    auth_from_credentials, build_remote_url, ensure_remote_directories, get_bytes, head_etag,
    path_segments, put_bytes, test_connection, WebDavAuth,
};
use crate::settings::{set_webdav_sync_settings, WebDavSyncSettings};

// ─── Protocol constants ──────────────────────────────────────

const PROTOCOL_FORMAT: &str = "cc-switch-webdav-sync";
const PROTOCOL_VERSION: u32 = 2;
const REMOTE_DB_SQL: &str = "db.sql";
const REMOTE_SKILLS_ZIP: &str = "skills.zip";
const REMOTE_MANIFEST: &str = "manifest.json";

fn localized(key: &'static str, zh: impl Into<String>, en: impl Into<String>) -> AppError {
    AppError::localized(key, zh, en)
}

fn io_context_localized(
    _key: &'static str,
    zh: impl Into<String>,
    en: impl Into<String>,
    source: std::io::Error,
) -> AppError {
    let zh_msg = zh.into();
    let en_msg = en.into();
    AppError::IoContext {
        context: format!("{zh_msg} ({en_msg})"),
        source,
    }
}

// ─── Types ───────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SyncManifest {
    format: String,
    version: u32,
    device_id: String,
    created_at: String,
    artifacts: BTreeMap<String, ArtifactMeta>,
    snapshot_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ArtifactMeta {
    sha256: String,
    size: u64,
}

struct LocalSnapshot {
    db_sql: Vec<u8>,
    skills_zip: Vec<u8>,
    manifest_bytes: Vec<u8>,
    manifest_hash: String,
}

// ─── Public API ──────────────────────────────────────────────

/// Check WebDAV connectivity and ensure remote directory structure.
pub async fn check_connection(settings: &WebDavSyncSettings) -> Result<(), AppError> {
    settings.validate()?;
    let auth = auth_for(settings);
    test_connection(&settings.base_url, &auth).await?;
    let dir_segs = remote_dir_segments(settings);
    ensure_remote_directories(&settings.base_url, &dir_segs, &auth).await?;
    Ok(())
}

/// Upload local snapshot (db + skills) to remote.
pub async fn upload(
    db: &crate::database::Database,
    settings: &mut WebDavSyncSettings,
) -> Result<Value, AppError> {
    settings.validate()?;
    let auth = auth_for(settings);
    let dir_segs = remote_dir_segments(settings);
    ensure_remote_directories(&settings.base_url, &dir_segs, &auth).await?;

    let snapshot = build_local_snapshot(db, settings)?;

    // Upload order: artifacts first, manifest last (best-effort consistency)
    let db_url = remote_file_url(settings, REMOTE_DB_SQL)?;
    put_bytes(&db_url, &auth, snapshot.db_sql, "application/sql").await?;

    let skills_url = remote_file_url(settings, REMOTE_SKILLS_ZIP)?;
    put_bytes(&skills_url, &auth, snapshot.skills_zip, "application/zip").await?;

    let manifest_url = remote_file_url(settings, REMOTE_MANIFEST)?;
    put_bytes(
        &manifest_url,
        &auth,
        snapshot.manifest_bytes,
        "application/json",
    )
    .await?;

    // Fetch etag (best-effort, don't fail the upload)
    let etag = match head_etag(&manifest_url, &auth).await {
        Ok(e) => e,
        Err(e) => {
            log::debug!("[WebDAV] Failed to fetch ETag after upload: {e}");
            None
        }
    };

    persist_sync_success(settings, snapshot.manifest_hash, etag)?;
    Ok(serde_json::json!({ "status": "uploaded" }))
}

/// Download remote snapshot and apply to local database + skills.
pub async fn download(
    db: &crate::database::Database,
    settings: &mut WebDavSyncSettings,
) -> Result<Value, AppError> {
    settings.validate()?;
    let auth = auth_for(settings);

    let manifest_url = remote_file_url(settings, REMOTE_MANIFEST)?;
    let (manifest_bytes, etag) = get_bytes(&manifest_url, &auth)
        .await?
        .ok_or_else(|| {
            localized(
                "webdav.sync.remote_empty",
                "远端没有可下载的同步数据",
                "No downloadable sync data found on the remote.",
            )
        })?;

    let manifest: SyncManifest =
        serde_json::from_slice(&manifest_bytes).map_err(|e| AppError::Json {
            path: REMOTE_MANIFEST.to_string(),
            source: e,
        })?;

    if manifest.format != PROTOCOL_FORMAT {
        return Err(localized(
            "webdav.sync.manifest_format_incompatible",
            format!("远端 manifest 格式不兼容: {}", manifest.format),
            format!("Remote manifest format is incompatible: {}", manifest.format),
        ));
    }
    if manifest.version != PROTOCOL_VERSION {
        return Err(localized(
            "webdav.sync.manifest_version_incompatible",
            format!(
                "远端 manifest 协议版本不兼容: v{} (本地 v{PROTOCOL_VERSION})",
                manifest.version
            ),
            format!(
                "Remote manifest protocol version is incompatible: v{} (local v{PROTOCOL_VERSION})",
                manifest.version
            ),
        ));
    }

    // Download and verify artifacts
    let db_sql = download_and_verify(settings, &auth, REMOTE_DB_SQL, &manifest.artifacts).await?;
    let skills_zip =
        download_and_verify(settings, &auth, REMOTE_SKILLS_ZIP, &manifest.artifacts).await?;

    // Apply snapshot
    apply_snapshot(db, &db_sql, &skills_zip)?;

    let manifest_hash = sha256_hex(&manifest_bytes);
    persist_sync_success(settings, manifest_hash, etag)?;
    Ok(serde_json::json!({ "status": "downloaded" }))
}

/// Fetch remote manifest info without downloading artifacts.
pub async fn fetch_remote_info(
    settings: &WebDavSyncSettings,
) -> Result<Option<Value>, AppError> {
    settings.validate()?;
    let auth = auth_for(settings);
    let manifest_url = remote_file_url(settings, REMOTE_MANIFEST)?;

    let Some((bytes, _)) = get_bytes(&manifest_url, &auth).await? else {
        return Ok(None);
    };

    let manifest: SyncManifest =
        serde_json::from_slice(&bytes).map_err(|e| AppError::Json {
            path: REMOTE_MANIFEST.to_string(),
            source: e,
        })?;

    let compatible =
        manifest.format == PROTOCOL_FORMAT && manifest.version == PROTOCOL_VERSION;

    Ok(Some(serde_json::json!({
        "deviceId": manifest.device_id,
        "createdAt": manifest.created_at,
        "snapshotId": manifest.snapshot_id,
        "version": manifest.version,
        "compatible": compatible,
        "artifacts": manifest.artifacts.keys().collect::<Vec<_>>(),
    })))
}

// ─── Sync status persistence (I3: deduplicated) ─────────────

fn persist_sync_success(
    settings: &mut WebDavSyncSettings,
    manifest_hash: String,
    etag: Option<String>,
) -> Result<(), AppError> {
    settings.status.last_sync_at = Some(Utc::now().timestamp());
    settings.status.last_error = None;
    settings.status.last_local_manifest_hash = Some(manifest_hash.clone());
    settings.status.last_remote_manifest_hash = Some(manifest_hash);
    settings.status.last_remote_etag = etag;
    set_webdav_sync_settings(Some(settings.clone()))
}

// ─── Snapshot building ───────────────────────────────────────

fn build_local_snapshot(
    db: &crate::database::Database,
    settings: &WebDavSyncSettings,
) -> Result<LocalSnapshot, AppError> {
    // Export database to SQL string
    let sql_string = db.export_sql_string()?;
    let db_sql = sql_string.into_bytes();

    // Pack skills into deterministic ZIP
    let tmp = tempdir().map_err(|e| {
        io_context_localized(
            "webdav.sync.snapshot_tmpdir_failed",
            "创建 WebDAV 快照临时目录失败",
            "Failed to create temporary directory for WebDAV snapshot",
            e,
        )
    })?;
    let skills_zip_path = tmp.path().join(REMOTE_SKILLS_ZIP);
    zip_skills_ssot(&skills_zip_path)?;
    let skills_zip = fs::read(&skills_zip_path).map_err(|e| AppError::io(&skills_zip_path, e))?;

    // Build artifact map and compute hashes
    let mut artifacts = BTreeMap::new();
    artifacts.insert(
        REMOTE_DB_SQL.to_string(),
        ArtifactMeta {
            sha256: sha256_hex(&db_sql),
            size: db_sql.len() as u64,
        },
    );
    artifacts.insert(
        REMOTE_SKILLS_ZIP.to_string(),
        ArtifactMeta {
            sha256: sha256_hex(&skills_zip),
            size: skills_zip.len() as u64,
        },
    );

    let snapshot_id = compute_snapshot_id(&artifacts);
    let manifest = SyncManifest {
        format: PROTOCOL_FORMAT.to_string(),
        version: PROTOCOL_VERSION,
        device_id: settings.device_id.clone(),
        created_at: Utc::now().to_rfc3339(),
        artifacts,
        snapshot_id,
    };
    let manifest_bytes =
        serde_json::to_vec_pretty(&manifest).map_err(|e| AppError::JsonSerialize { source: e })?;
    let manifest_hash = sha256_hex(&manifest_bytes);

    Ok(LocalSnapshot {
        db_sql,
        skills_zip,
        manifest_bytes,
        manifest_hash,
    })
}

/// Compute a deterministic snapshot identity from artifact hashes.
///
/// BTreeMap iteration order is sorted by key, ensuring stability.
fn compute_snapshot_id(artifacts: &BTreeMap<String, ArtifactMeta>) -> String {
    let parts: Vec<String> = artifacts
        .iter()
        .map(|(name, meta)| format!("{}:{}", name, meta.sha256))
        .collect();
    sha256_hex(parts.join("|").as_bytes())
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

// ─── Skills ZIP ──────────────────────────────────────────────

fn zip_skills_ssot(dest_path: &Path) -> Result<(), AppError> {
    let source = SkillService::get_ssot_dir()
        .map_err(|e| {
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
        // Canonicalize root once so symlink targets can be bounds-checked
        let canonical_root = fs::canonicalize(&source).unwrap_or_else(|_| source.clone());
        zip_dir_recursive(&canonical_root, &canonical_root, &mut writer, options)?;
    }

    writer
        .finish()
        .map_err(|e| {
            localized(
                "webdav.sync.skills_zip_write_failed",
                format!("写入 skills.zip 失败: {e}"),
                format!("Failed to write skills.zip: {e}"),
            )
        })?;
    Ok(())
}

fn zip_dir_recursive(
    root: &Path,
    current: &Path,
    writer: &mut zip::ZipWriter<fs::File>,
    options: SimpleFileOptions,
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

        // Skip .DS_Store, .git, and hidden files
        if name_str.starts_with('.') {
            continue;
        }

        // Dereference symlinks, but skip targets that escape the root directory
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
            writer
                .add_directory(format!("{rel_str}/"), options)
                .map_err(|e| {
                    localized(
                        "webdav.sync.zip_add_directory_failed",
                        format!("写入 ZIP 目录失败: {e}"),
                        format!("Failed to write ZIP directory entry: {e}"),
                    )
                })?;
            zip_dir_recursive(root, &real_path, writer, options)?;
        } else {
            writer
                .start_file(&rel_str, options)
                .map_err(|e| {
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
            writer
                .write_all(&buf)
                .map_err(|e| {
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

fn restore_skills_zip(raw: &[u8]) -> Result<(), AppError> {
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
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|e| {
            localized(
                "webdav.sync.skills_zip_parse_failed",
                format!("解析 skills.zip 失败: {e}"),
                format!("Failed to parse skills.zip: {e}"),
            )
        })?;

    let extracted = tmp.path().join("skills-extracted");
    fs::create_dir_all(&extracted).map_err(|e| AppError::io(&extracted, e))?;

    for idx in 0..archive.len() {
        let mut entry = archive
            .by_index(idx)
            .map_err(|e| {
                localized(
                    "webdav.sync.skills_zip_entry_read_failed",
                    format!("读取 ZIP 项失败: {e}"),
                    format!("Failed to read ZIP entry: {e}"),
                )
            })?;
        // Zip-slip protection
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
        std::io::copy(&mut entry, &mut out).map_err(|e| AppError::io(&out_path, e))?;
    }

    // Rename-based swap: old → .bak, extracted → SSOT, then delete .bak.
    // Ensures at least one valid copy exists at all times.
    let ssot = SkillService::get_ssot_dir()
        .map_err(|e| {
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
        // Rollback: restore backup
        if bak.exists() {
            let _ = fs::remove_dir_all(&ssot);
            let _ = fs::rename(&bak, &ssot);
        }
        return Err(e);
    }

    // Cleanup backup
    let _ = fs::remove_dir_all(&bak);
    Ok(())
}

struct SkillsBackup {
    _tmp: TempDir,
    backup_dir: PathBuf,
    ssot_path: PathBuf,
    existed: bool,
}

fn backup_current_skills() -> Result<SkillsBackup, AppError> {
    let ssot = SkillService::get_ssot_dir()
        .map_err(|e| {
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

fn restore_skills_from_backup(backup: &SkillsBackup) -> Result<(), AppError> {
    if backup.ssot_path.exists() {
        fs::remove_dir_all(&backup.ssot_path).map_err(|e| AppError::io(&backup.ssot_path, e))?;
    }

    if backup.existed {
        copy_dir_recursive(&backup.backup_dir, &backup.ssot_path)?;
    }

    Ok(())
}

// ─── Download & verify ───────────────────────────────────────

async fn download_and_verify(
    settings: &WebDavSyncSettings,
    auth: &WebDavAuth,
    artifact_name: &str,
    artifacts: &BTreeMap<String, ArtifactMeta>,
) -> Result<Vec<u8>, AppError> {
    let meta = artifacts.get(artifact_name).ok_or_else(|| {
        localized(
            "webdav.sync.manifest_missing_artifact",
            format!("manifest 中缺少 artifact: {artifact_name}"),
            format!("Manifest missing artifact: {artifact_name}"),
        )
    })?;
    let url = remote_file_url(settings, artifact_name)?;
    let (bytes, _) = get_bytes(&url, auth)
        .await?
        .ok_or_else(|| {
            localized(
                "webdav.sync.remote_missing_artifact",
                format!("远端缺少 artifact 文件: {artifact_name}"),
                format!("Remote artifact file missing: {artifact_name}"),
            )
        })?;

    // Quick size check before expensive hash
    if bytes.len() as u64 != meta.size {
        return Err(localized(
            "webdav.sync.artifact_size_mismatch",
            format!(
                "artifact {artifact_name} 大小不匹配 (expected: {}, got: {})",
                meta.size,
                bytes.len(),
            ),
            format!(
                "Artifact {artifact_name} size mismatch (expected: {}, got: {})",
                meta.size,
                bytes.len(),
            ),
        ));
    }

    let actual_hash = sha256_hex(&bytes);
    if actual_hash != meta.sha256 {
        return Err(localized(
            "webdav.sync.artifact_hash_mismatch",
            format!(
                "artifact {artifact_name} SHA256 校验失败 (expected: {}..., got: {}...)",
                meta.sha256.get(..8).unwrap_or(&meta.sha256),
                actual_hash.get(..8).unwrap_or(&actual_hash),
            ),
            format!(
                "Artifact {artifact_name} SHA256 verification failed (expected: {}..., got: {}...)",
                meta.sha256.get(..8).unwrap_or(&meta.sha256),
                actual_hash.get(..8).unwrap_or(&actual_hash),
            ),
        ));
    }
    Ok(bytes)
}

fn apply_snapshot(
    db: &crate::database::Database,
    db_sql: &[u8],
    skills_zip: &[u8],
) -> Result<(), AppError> {
    let sql_str = std::str::from_utf8(db_sql).map_err(|e| {
        localized(
            "webdav.sync.sql_not_utf8",
            format!("SQL 非 UTF-8: {e}"),
            format!("SQL is not valid UTF-8: {e}"),
        )
    })?;
    let skills_backup = backup_current_skills()?;

    // 先替换 skills，再导入数据库；若导入失败则回滚 skills，避免“半恢复”。
    restore_skills_zip(skills_zip)?;

    if let Err(db_err) = db.import_sql_string(sql_str) {
        if let Err(rollback_err) = restore_skills_from_backup(&skills_backup) {
            return Err(localized(
                "webdav.sync.db_import_and_rollback_failed",
                format!("导入数据库失败: {db_err}; 同时回滚 Skills 失败: {rollback_err}"),
                format!(
                    "Database import failed: {db_err}; skills rollback also failed: {rollback_err}"
                ),
            ));
        }
        return Err(db_err);
    }

    Ok(())
}

// ─── Remote path helpers ─────────────────────────────────────

fn remote_dir_segments(settings: &WebDavSyncSettings) -> Vec<String> {
    let mut segs = Vec::new();
    segs.extend(path_segments(&settings.remote_root).map(str::to_string));
    segs.push(format!("v{PROTOCOL_VERSION}"));
    segs.extend(path_segments(&settings.profile).map(str::to_string));
    segs
}

fn remote_file_url(settings: &WebDavSyncSettings, file_name: &str) -> Result<String, AppError> {
    let mut segs = remote_dir_segments(settings);
    segs.extend(path_segments(file_name).map(str::to_string));
    build_remote_url(&settings.base_url, &segs)
}

fn auth_for(settings: &WebDavSyncSettings) -> WebDavAuth {
    auth_from_credentials(&settings.username, &settings.password)
}

// ─── Internal helpers ────────────────────────────────────────

fn copy_dir_recursive(src: &Path, dest: &Path) -> Result<(), AppError> {
    if !src.exists() {
        return Ok(());
    }
    fs::create_dir_all(dest).map_err(|e| AppError::io(dest, e))?;
    for entry in fs::read_dir(src).map_err(|e| AppError::io(src, e))? {
        let entry = entry.map_err(|e| AppError::io(src, e))?;
        let path = entry.path();
        let dest_path = dest.join(entry.file_name());
        if path.is_dir() {
            copy_dir_recursive(&path, &dest_path)?;
        } else {
            fs::copy(&path, &dest_path).map_err(|e| AppError::io(&dest_path, e))?;
        }
    }
    Ok(())
}

// ─── Tests ───────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn artifact(sha256: &str, size: u64) -> ArtifactMeta {
        ArtifactMeta { sha256: sha256.to_string(), size }
    }

    #[test]
    fn snapshot_id_is_stable() {
        let mut artifacts = BTreeMap::new();
        artifacts.insert("db.sql".to_string(), artifact("abc123", 100));
        artifacts.insert("skills.zip".to_string(), artifact("def456", 200));

        let id1 = compute_snapshot_id(&artifacts);
        let id2 = compute_snapshot_id(&artifacts);
        assert_eq!(id1, id2);
    }

    #[test]
    fn snapshot_id_changes_with_artifacts() {
        let mut a1 = BTreeMap::new();
        a1.insert("db.sql".to_string(), artifact("hash-a", 1));

        let mut a2 = BTreeMap::new();
        a2.insert("db.sql".to_string(), artifact("hash-b", 1));

        assert_ne!(compute_snapshot_id(&a1), compute_snapshot_id(&a2));
    }

    #[test]
    fn remote_dir_segments_uses_v2() {
        let settings = WebDavSyncSettings {
            remote_root: "cc-switch-sync".to_string(),
            profile: "default".to_string(),
            ..WebDavSyncSettings::default()
        };
        let segs = remote_dir_segments(&settings);
        assert_eq!(segs, vec!["cc-switch-sync", "v2", "default"]);
    }

    #[test]
    fn sha256_hex_is_correct() {
        let hash = sha256_hex(b"hello");
        assert_eq!(
            hash,
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }
}
