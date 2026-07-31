//! `ObjectStorage` backed by the existing WebDAV transport.
//!
//! This is a thin adapter: it does NOT implement any HTTP itself. Every network
//! operation delegates to the shared primitives in [`crate::services::webdav`]
//! (`get_bytes` / `put_bytes` / `head_etag` / `ensure_remote_directories`), the
//! same layer that powers the db.sql/skills.zip sync. Credentials come straight
//! from [`WebDavSyncSettings`], so workspace sync reuses the cloud target the
//! user already configured.
//!
//! Conditional writes are emulated on top of unconditional PUT + HEAD:
//! - `put_if_absent`  = HEAD (exists?) → PUT when missing
//! - `put_if_match`   = HEAD (etag compare) → PUT when matching
//!
//! Content-addressed blobs are write-once, so `put_if_absent`'s tiny race window
//! is harmless (identical bytes → identical key). `head.json` uses `put_if_match`
//! for optimistic concurrency; the merge layer's keep-both policy is the ultimate
//! safety net if two devices race.

use std::collections::HashSet;

use async_trait::async_trait;
use bytes::Bytes;
use tokio::sync::Mutex;

use super::{ConditionalPutResult, ObjectStorage, PutResult, RemoteObject};
use crate::error::AppError;
use crate::services::sync_protocol::MAX_SYNC_ARTIFACT_BYTES;
use crate::services::webdav::{
    auth_from_credentials, build_remote_url, ensure_remote_directories, get_bytes, head_etag,
    path_segments, put_bytes, WebDavAuth,
};
use crate::settings::WebDavSyncSettings;

const OCTET_STREAM: &str = "application/octet-stream";

/// WebDAV-backed object store. Keys are `/`-delimited remote paths appended to
/// `base_url` (e.g. `cc-switch-workspace/v1/default/blobs/<sha256>`).
pub struct WebDavObjectStorage {
    base_url: String,
    auth: WebDavAuth,
    /// Remote directories already MKCOL-ed this session, to avoid redundant
    /// round-trips when writing many blobs under the same parent.
    ensured_dirs: Mutex<HashSet<String>>,
}

impl WebDavObjectStorage {
    /// Build from the user's existing WebDAV credentials.
    pub fn from_settings(settings: &WebDavSyncSettings) -> Self {
        Self {
            base_url: settings.base_url.clone(),
            auth: auth_from_credentials(&settings.username, &settings.password),
            ensured_dirs: Mutex::new(HashSet::new()),
        }
    }

    fn segments(key: &str) -> Vec<String> {
        path_segments(key).map(str::to_string).collect()
    }

    fn url_for(&self, key: &str) -> Result<String, AppError> {
        build_remote_url(&self.base_url, &Self::segments(key))
    }

    /// Ensure the parent directory chain of `key` exists (MKCOL), memoized.
    async fn ensure_parent_dirs(&self, key: &str) -> Result<(), AppError> {
        let mut segs = Self::segments(key);
        // Drop the file component; only directories need MKCOL.
        if segs.pop().is_none() || segs.is_empty() {
            return Ok(());
        }
        let dir_key = segs.join("/");
        {
            let guard = self.ensured_dirs.lock().await;
            if guard.contains(&dir_key) {
                return Ok(());
            }
        }
        ensure_remote_directories(&self.base_url, &segs, &self.auth).await?;
        self.ensured_dirs.lock().await.insert(dir_key);
        Ok(())
    }

    async fn head(&self, key: &str) -> Result<Option<String>, AppError> {
        let url = self.url_for(key)?;
        head_etag(&url, &self.auth).await
    }

    async fn put_unconditional(&self, key: &str, bytes: Bytes) -> Result<PutResult, AppError> {
        self.ensure_parent_dirs(key).await?;
        let url = self.url_for(key)?;
        put_bytes(&url, &self.auth, bytes.to_vec(), OCTET_STREAM).await?;
        // Best-effort ETag: not all servers return one on PUT, so HEAD for it.
        let etag = head_etag(&url, &self.auth).await.unwrap_or(None);
        Ok(PutResult { etag })
    }
}

#[async_trait]
impl ObjectStorage for WebDavObjectStorage {
    async fn get(&self, key: &str) -> Result<Option<RemoteObject>, AppError> {
        let url = self.url_for(key)?;
        let Some((bytes, etag)) = get_bytes(&url, &self.auth, MAX_SYNC_ARTIFACT_BYTES as usize).await?
        else {
            return Ok(None);
        };
        Ok(Some(RemoteObject {
            bytes: Bytes::from(bytes),
            etag,
        }))
    }

    async fn put(&self, key: &str, bytes: Bytes) -> Result<PutResult, AppError> {
        self.put_unconditional(key, bytes).await
    }

    async fn put_if_match(
        &self,
        key: &str,
        etag: &str,
        bytes: Bytes,
    ) -> Result<ConditionalPutResult, AppError> {
        match self.head(key).await? {
            Some(current) if current == etag => {
                let result = self.put_unconditional(key, bytes).await?;
                Ok(ConditionalPutResult::Written { etag: result.etag })
            }
            _ => Ok(ConditionalPutResult::PreconditionFailed),
        }
    }

    async fn put_if_absent(
        &self,
        key: &str,
        bytes: Bytes,
    ) -> Result<ConditionalPutResult, AppError> {
        if self.head(key).await?.is_some() {
            return Ok(ConditionalPutResult::PreconditionFailed);
        }
        let result = self.put_unconditional(key, bytes).await?;
        Ok(ConditionalPutResult::Written { etag: result.etag })
    }

    async fn exists(&self, key: &str) -> Result<bool, AppError> {
        Ok(self.head(key).await?.is_some())
    }

    async fn list(&self, _prefix: &str) -> Result<Vec<String>, AppError> {
        // Only needed for blob GC, which is deferred. Returning empty is safe:
        // backup uses put_if_absent for dedup and merge reads objects by exact
        // key, so neither path enumerates the remote.
        log::debug!("[workspace_sync] WebDAV list() is a no-op (blob GC deferred)");
        Ok(Vec::new())
    }

    async fn delete(&self, _key: &str) -> Result<(), AppError> {
        // Blob GC is deferred; no caller deletes in the MVP flows.
        Err(AppError::Message(
            "workspace sync WebDAV delete is not supported yet (blob GC deferred)".to_string(),
        ))
    }
}
