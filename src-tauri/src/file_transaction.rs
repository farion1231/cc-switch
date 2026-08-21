use crate::error::AppError;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Debug)]
pub(crate) struct FileSnapshot {
    pub(crate) contents: Option<Vec<u8>>,
}

#[derive(Debug)]
pub(crate) struct FileUpdate {
    path: PathBuf,
    contents: Option<Arc<[u8]>>,
}

impl FileUpdate {
    pub(crate) fn write(path: PathBuf, contents: Vec<u8>) -> Self {
        Self::write_shared(path, Arc::from(contents))
    }

    pub(crate) fn write_shared(path: PathBuf, contents: Arc<[u8]>) -> Self {
        Self {
            path,
            contents: Some(contents),
        }
    }

    #[cfg(any(unix, test))]
    pub(crate) fn delete(path: PathBuf) -> Self {
        Self {
            path,
            contents: None,
        }
    }
}

fn metadata_if_exists(path: &Path) -> Result<Option<fs::Metadata>, AppError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => Ok(Some(metadata)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(AppError::io(path, error)),
    }
}

fn validate_existing_file(
    path: &Path,
    max_bytes: Option<u64>,
    description: &str,
) -> Result<(), AppError> {
    let metadata = metadata_if_exists(path)?.ok_or_else(|| {
        AppError::Config(format!("{description} does not exist: {}", path.display()))
    })?;
    if metadata.file_type().is_symlink() {
        return Err(AppError::InvalidInput(format!(
            "Refusing to modify symlinked {description}: {}",
            path.display()
        )));
    }
    if !metadata.is_file() {
        return Err(AppError::InvalidInput(format!(
            "{description} is not a regular file: {}",
            path.display()
        )));
    }
    if let Some(limit) = max_bytes {
        if metadata.len() > limit {
            return Err(AppError::InvalidInput(format!(
                "{description} exceeds {} MiB: {}",
                limit / 1024 / 1024,
                path.display()
            )));
        }
    }
    Ok(())
}

pub(crate) fn snapshot_file(
    path: &Path,
    max_bytes: Option<u64>,
    description: &str,
) -> Result<FileSnapshot, AppError> {
    if metadata_if_exists(path)?.is_none() {
        return Ok(FileSnapshot { contents: None });
    }
    validate_existing_file(path, max_bytes, description)?;
    Ok(FileSnapshot {
        contents: Some(fs::read(path).map_err(|error| AppError::io(path, error))?),
    })
}

pub(crate) fn restore_snapshot(
    path: &Path,
    snapshot: &FileSnapshot,
    description: &str,
) -> Result<(), AppError> {
    restore_snapshot_with(path, snapshot, description, false)
}

/// Same as [`restore_snapshot`], but recreated files are written owner-only on
/// Unix because their contents may include credentials.
pub(crate) fn restore_snapshot_private(
    path: &Path,
    snapshot: &FileSnapshot,
    description: &str,
) -> Result<(), AppError> {
    restore_snapshot_with(path, snapshot, description, true)
}

fn restore_snapshot_with(
    path: &Path,
    snapshot: &FileSnapshot,
    description: &str,
    private: bool,
) -> Result<(), AppError> {
    match &snapshot.contents {
        Some(contents) => {
            if metadata_if_exists(path)?.is_some() {
                validate_existing_file(path, None, description)?;
            }
            if private {
                crate::config::atomic_write_private(path, contents)
            } else {
                crate::config::atomic_write(path, contents)
            }
        }
        None => match metadata_if_exists(path)? {
            Some(_) => {
                validate_existing_file(path, None, description)?;
                fs::remove_file(path).map_err(|error| AppError::io(path, error))
            }
            None => Ok(()),
        },
    }
}

fn path_identity(path: &Path) -> String {
    let path = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let identity = path.to_string_lossy().replace('\\', "/");
    if cfg!(windows) {
        identity.to_lowercase()
    } else {
        identity
    }
}

fn rollback(updates: &[(FileUpdate, FileSnapshot)], description: &str) -> Result<(), AppError> {
    let mut errors = Vec::new();
    for (update, snapshot) in updates.iter().rev() {
        if let Err(error) = restore_snapshot(&update.path, snapshot, description) {
            errors.push(error.to_string());
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(AppError::Config(format!(
            "Failed to roll back {description} transaction: {}",
            errors.join("; ")
        )))
    }
}

fn apply_update(update: &FileUpdate, description: &str) -> Result<(), AppError> {
    match &update.contents {
        Some(contents) => {
            if metadata_if_exists(&update.path)?.is_some() {
                validate_existing_file(&update.path, None, description)?;
            }
            crate::config::atomic_write(&update.path, contents)
        }
        None => match metadata_if_exists(&update.path)? {
            Some(_) => {
                validate_existing_file(&update.path, None, description)?;
                fs::remove_file(&update.path).map_err(|error| AppError::io(&update.path, error))
            }
            None => Ok(()),
        },
    }
}

fn commit_file_updates_with_hook<F>(
    updates: Vec<FileUpdate>,
    max_bytes: Option<u64>,
    description: &str,
    mut before_apply: F,
) -> Result<(), AppError>
where
    F: FnMut(usize, &FileUpdate) -> Result<(), AppError>,
{
    let mut planned_by_identity: HashMap<String, usize> = HashMap::new();
    let mut unique_updates: Vec<FileUpdate> = Vec::new();

    // Collapse duplicate physical paths only when they request identical final
    // contents, without cloning every potentially large payload into the map.
    for update in updates {
        let identity = path_identity(&update.path);
        if let Some(existing_index) = planned_by_identity.get(&identity) {
            if unique_updates[*existing_index].contents != update.contents {
                return Err(AppError::InvalidInput(format!(
                    "Conflicting updates resolve to the same {description}: {}",
                    update.path.display()
                )));
            }
            continue;
        }
        planned_by_identity.insert(identity, unique_updates.len());
        unique_updates.push(update);
    }

    // Snapshot and validate every target before the first write.
    let mut prepared = Vec::new();
    for update in unique_updates {
        if let (Some(limit), Some(contents)) = (max_bytes, update.contents.as_ref()) {
            if contents.len() as u64 > limit {
                return Err(AppError::InvalidInput(format!(
                    "{description} exceeds {} MiB: {}",
                    limit / 1024 / 1024,
                    update.path.display()
                )));
            }
        }
        let snapshot = snapshot_file(&update.path, max_bytes, description)?;
        if snapshot.contents.as_deref() != update.contents.as_deref() {
            prepared.push((update, snapshot));
        }
    }

    for index in 0..prepared.len() {
        if let Err(error) = before_apply(index, &prepared[index].0) {
            let rollback_result = rollback(&prepared[..index], description);
            return match rollback_result {
                Ok(()) => Err(error),
                Err(rollback_error) => Err(AppError::Config(format!("{error}; {rollback_error}"))),
            };
        }
        if let Err(error) = apply_update(&prepared[index].0, description) {
            let rollback_result = rollback(&prepared[..=index], description);
            return match rollback_result {
                Ok(()) => Err(error),
                Err(rollback_error) => Err(AppError::Config(format!("{error}; {rollback_error}"))),
            };
        }
    }
    Ok(())
}

pub(crate) fn commit_file_updates(
    updates: Vec<FileUpdate>,
    max_bytes: Option<u64>,
    description: &str,
) -> Result<(), AppError> {
    commit_file_updates_with_hook(updates, max_bytes, description, |_, _| Ok(()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rolls_back_prior_files_when_a_later_write_fails() {
        let temp = tempfile::tempdir().expect("temp directory");
        let first = temp.path().join("first.txt");
        let second = temp.path().join("second.txt");
        fs::write(&first, "first-before").expect("first fixture");
        fs::write(&second, "second-before").expect("second fixture");

        let result = commit_file_updates_with_hook(
            vec![
                FileUpdate::write(first.clone(), b"first-after".to_vec()),
                FileUpdate::write(second.clone(), b"second-after".to_vec()),
            ],
            None,
            "test file",
            |index, _| {
                if index == 1 {
                    Err(AppError::Config("injected failure".to_string()))
                } else {
                    Ok(())
                }
            },
        );

        assert!(result.is_err());
        assert_eq!(
            fs::read_to_string(first).expect("first restored"),
            "first-before"
        );
        assert_eq!(
            fs::read_to_string(second).expect("second unchanged"),
            "second-before"
        );
    }

    #[cfg(unix)]
    #[test]
    fn private_restore_recreates_owner_only_file() {
        use std::os::unix::fs::PermissionsExt;
        let temp = tempfile::tempdir().expect("temp directory");
        let path = temp.path().join("secret.json");
        let snapshot = FileSnapshot {
            contents: Some(b"secret".to_vec()),
        };

        restore_snapshot_private(&path, &snapshot, "test file").expect("restore");

        assert_eq!(fs::read(&path).expect("contents"), b"secret");
        let mode = fs::metadata(&path).expect("metadata").permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}
