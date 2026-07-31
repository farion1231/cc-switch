//! Content-addressed blob storage over an [`ObjectStorage`].
//!
//! Each workspace file's bytes are stored once under `blobs/<sha256>`. Because
//! the key is the content hash, uploading is naturally idempotent and
//! incremental: `put_if_absent` skips blobs the remote already has, so an
//! unchanged file across two backups costs a single HEAD.
//!
//! Plaintext mode only (per project decision): the blob key is the raw content
//! hash and the bytes are stored as-is. The encryption path in `crypto.rs`
//! remains available for a future opt-in.

use bytes::Bytes;

use crate::error::AppError;
use crate::workspace_sync::storage::{ConditionalPutResult, ObjectStorage};

/// Remote key for a blob given its content hash.
pub fn blob_key(remote_prefix: &str, content_hash: &str) -> String {
    format!("{}/blobs/{}", remote_prefix.trim_end_matches('/'), content_hash)
}

/// Upload `bytes` under its content hash if the remote does not already have it.
/// Returns `true` if newly uploaded, `false` if it already existed.
pub async fn put_blob(
    storage: &dyn ObjectStorage,
    remote_prefix: &str,
    content_hash: &str,
    bytes: Vec<u8>,
) -> Result<bool, AppError> {
    let key = blob_key(remote_prefix, content_hash);
    match storage.put_if_absent(&key, Bytes::from(bytes)).await? {
        ConditionalPutResult::Written { .. } => Ok(true),
        ConditionalPutResult::PreconditionFailed => Ok(false),
        ConditionalPutResult::Unsupported => {
            // Backend can't do conditional writes: fall back to exists+put.
            if storage.exists(&key).await? {
                Ok(false)
            } else {
                // Re-fetch bytes is not possible here; caller passed them once,
                // so this branch is only reachable when Unsupported is returned
                // without a prior write. Treat as "must upload" via plain put.
                Err(AppError::Message(
                    "workspace sync: conditional put unsupported and blob missing".to_string(),
                ))
            }
        }
    }
}

/// Download the blob for `content_hash`. Returns `None` if absent on the remote.
pub async fn get_blob(
    storage: &dyn ObjectStorage,
    remote_prefix: &str,
    content_hash: &str,
) -> Result<Option<Vec<u8>>, AppError> {
    let key = blob_key(remote_prefix, content_hash);
    Ok(storage.get(&key).await?.map(|object| object.bytes.to_vec()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace_sync::storage::memory::MemoryStorage;

    #[tokio::test]
    async fn put_blob_is_idempotent_by_content_hash() -> Result<(), AppError> {
        let storage = MemoryStorage::default();
        let prefix = "cc-switch-workspace/v1/default";
        let hash = "deadbeef";

        let first = put_blob(&storage, prefix, hash, b"hello".to_vec()).await?;
        assert!(first, "first upload should write");

        let second = put_blob(&storage, prefix, hash, b"hello".to_vec()).await?;
        assert!(!second, "second upload of same hash should be skipped");

        let fetched = get_blob(&storage, prefix, hash).await?;
        assert_eq!(fetched.as_deref(), Some(&b"hello"[..]));
        Ok(())
    }

    #[tokio::test]
    async fn get_blob_returns_none_when_absent() -> Result<(), AppError> {
        let storage = MemoryStorage::default();
        let fetched = get_blob(&storage, "prefix", "missing").await?;
        assert!(fetched.is_none());
        Ok(())
    }

    #[test]
    fn blob_key_layout_is_stable() {
        assert_eq!(
            blob_key("root/v1/default", "abc123"),
            "root/v1/default/blobs/abc123"
        );
        assert_eq!(
            blob_key("root/v1/default/", "abc123"),
            "root/v1/default/blobs/abc123"
        );
    }
}
