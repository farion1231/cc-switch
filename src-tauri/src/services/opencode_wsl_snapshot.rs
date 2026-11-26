//! Local cache for OpenCode databases hosted inside WSL.
//!
//! The live database is never opened by Windows. [`read_db_path`] asks the
//! distro to create a consistent SQLite online-backup snapshot, copies that
//! static file into the user's CC Switch directory, and returns the local path.
//! All existing session/usage readers then use their normal `rusqlite` code.
//!
//! The cache is refreshed only when the live main DB/WAL stamp changes. When a
//! distro is stopped, the last snapshot is reused without waking WSL.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::SystemTime;

use sha2::{Digest, Sha256};

use crate::session_manager::opencode_wsl;

fn snapshot_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn cache_dir() -> PathBuf {
    let dir = crate::config::get_home_dir()
        .join(".cc-switch")
        .join("opencode-wsl-snapshots");
    if crate::config::is_wsl_path(&dir) {
        // A developer may point CC Switch's home at a WSL share in tests or
        // debugging. Never put the read cache on the path that has no SQLite
        // locks; use the Windows temp directory instead.
        std::env::temp_dir().join("cc-switch-wsl-snapshots")
    } else {
        dir
    }
}

fn cache_key(db_path: &Path) -> String {
    let mut hasher = Sha256::new();
    hasher.update(db_path.to_string_lossy().as_bytes());
    let digest = hasher.finalize();
    digest
        .iter()
        .take(16)
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn metadata_stamp(metadata: Option<&fs::Metadata>) -> String {
    let Some(metadata) = metadata else {
        return "0:0".to_string();
    };
    let modified = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map(|duration| duration.as_nanos().min(i64::MAX as u128) as i64)
        .unwrap_or(0);
    format!("{modified}:{}", metadata.len())
}

fn wal_path(db_path: &Path) -> PathBuf {
    let mut path = db_path.as_os_str().to_os_string();
    path.push("-wal");
    PathBuf::from(path)
}

fn source_stamp(db_path: &Path) -> Result<String, String> {
    let db = fs::metadata(db_path)
        .map_err(|error| format!("Cannot read OpenCode WSL database metadata: {error}"))?;
    let wal = fs::metadata(wal_path(db_path)).ok();
    Ok(format!(
        "{}|{}",
        metadata_stamp(Some(&db)),
        metadata_stamp(wal.as_ref())
    ))
}

const CACHE_RETENTION: std::time::Duration = std::time::Duration::from_secs(30 * 24 * 60 * 60);

fn cleanup_cache(dir: &Path, keep: &Path) {
    let now = SystemTime::now();
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path == keep
            || !path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("opencode-"))
        {
            continue;
        }
        let Some(modified) = entry.metadata().ok().and_then(|meta| meta.modified().ok()) else {
            continue;
        };
        if now
            .duration_since(modified)
            .is_ok_and(|age| age > CACHE_RETENTION)
        {
            let _ = fs::remove_file(&path);
        }
    }
}

/// Return a local database path to read.
///
/// Non-WSL databases are returned unchanged. WSL databases return the cached
/// snapshot, creating or refreshing it first when the distro is running.
pub(crate) fn read_db_path(db_path: &Path) -> Result<PathBuf, String> {
    if !opencode_wsl::is_wsl(db_path) {
        return Ok(db_path.to_path_buf());
    }

    let _guard = crate::sync_utils::lock_file_operation(snapshot_lock());
    let distro = opencode_wsl::distro(db_path)
        .ok_or_else(|| "Invalid OpenCode WSL database path".to_string())?;
    let dir = cache_dir();
    fs::create_dir_all(&dir)
        .map_err(|error| format!("Cannot create OpenCode WSL snapshot cache: {error}"))?;
    let key = cache_key(db_path);
    let cache = dir.join(format!("opencode-{key}.db"));
    let sidecar = dir.join(format!("opencode-{key}.stamp"));

    if !opencode_wsl::distro_running(&distro) {
        if cache.exists() {
            return Ok(cache);
        }
        return Err(format!(
            "WSL distro '{distro}' is not running and no OpenCode snapshot is cached"
        ));
    }

    let stamp = match source_stamp(db_path) {
        Ok(stamp) => stamp,
        Err(error) => {
            if cache.exists() {
                log::warn!("Using cached OpenCode WSL snapshot: {error}");
                return Ok(cache);
            }
            return Err(error);
        }
    };
    if cache.exists()
        && fs::read_to_string(&sidecar)
            .ok()
            .is_some_and(|cached| cached == stamp)
    {
        return Ok(cache);
    }

    let snapshot = match opencode_wsl::create_snapshot(db_path) {
        Ok(snapshot) => snapshot,
        Err(error) => {
            if cache.exists() {
                log::warn!("Failed to refresh OpenCode WSL snapshot, using cache: {error}");
                return Ok(cache);
            }
            return Err(error);
        }
    };
    let unc = opencode_wsl::unc_path(&distro, &snapshot.native_path);
    let temp = dir.join(format!("opencode-{key}.db.tmp.{}", std::process::id()));
    let result = (|| -> Result<(), String> {
        fs::copy(&unc, &temp).map_err(|error| {
            format!(
                "Cannot copy OpenCode WSL snapshot {} to {}: {error}",
                unc.display(),
                temp.display()
            )
        })?;
        match fs::remove_file(&cache) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(format!("Cannot replace OpenCode WSL snapshot: {error}")),
        }
        fs::rename(&temp, &cache)
            .map_err(|error| format!("Cannot install OpenCode WSL snapshot: {error}"))?;
        fs::write(&sidecar, stamp)
            .map_err(|error| format!("Cannot write OpenCode WSL snapshot stamp: {error}"))?;
        Ok(())
    })();
    opencode_wsl::remove_snapshot(&distro, &snapshot.native_path);
    match result {
        Ok(()) => {
            cleanup_cache(&dir, &cache);
            Ok(cache)
        }
        Err(error) => {
            let _ = fs::remove_file(&temp);
            if cache.exists() {
                log::warn!("OpenCode WSL snapshot copy failed, using previous cache: {error}");
                Ok(cache)
            } else {
                Err(error)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_keys_are_stable_and_distinct() {
        assert_eq!(
            cache_key(Path::new(
                r"\\wsl$\Ubuntu\home\a\.local\share\opencode\opencode.db"
            )),
            cache_key(Path::new(
                r"\\wsl$\Ubuntu\home\a\.local\share\opencode\opencode.db"
            ))
        );
        assert_ne!(
            cache_key(Path::new(
                r"\\wsl$\Ubuntu\home\a\.local\share\opencode\opencode.db"
            )),
            cache_key(Path::new(
                r"\\wsl$\Ubuntu\home\b\.local\share\opencode\opencode.db"
            ))
        );
    }

    #[test]
    fn non_wsl_paths_bypass_snapshot_cache() {
        let path = Path::new(r"C:\Users\user\.local\share\opencode\opencode.db");
        assert_eq!(read_db_path(path).unwrap(), path);
    }

    #[test]
    fn wal_path_appends_instead_of_replacing_extension() {
        assert_eq!(
            wal_path(Path::new(r"C:\data\custom.sqlite")),
            PathBuf::from(r"C:\data\custom.sqlite-wal")
        );
    }

    #[test]
    fn stamps_distinguish_missing_and_existing_wal() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("opencode.db");
        std::fs::write(&path, b"main").unwrap();
        let without_wal = source_stamp(&path).unwrap();
        std::fs::write(dir.path().join("opencode.db-wal"), b"wal").unwrap();
        let with_wal = source_stamp(&path).unwrap();
        assert_ne!(without_wal, with_wal);
    }
}
