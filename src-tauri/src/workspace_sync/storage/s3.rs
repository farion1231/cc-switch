//! `ObjectStorage` backed by the existing S3 transport.
//!
//! Like the WebDAV adapter, this implements no HTTP/signing itself — it delegates
//! to [`crate::services::s3`] (`put_object` / `get_object` / `head_object`, all
//! SigV4-signed there). Credentials come from [`S3SyncSettings`], reusing the
//! bucket the user already configured for db/skills sync.
//!
//! S3 has no directory concept, so keys map 1:1 to object keys (no MKCOL needed).
//! Conditional writes are emulated with HEAD + PUT, identical to the WebDAV
//! adapter (see its module docs for why the race window is harmless here).

use async_trait::async_trait;
use bytes::Bytes;

use super::{ConditionalPutResult, ObjectStorage, PutResult, RemoteObject};
use crate::error::AppError;
use crate::services::s3::{get_object, head_object, put_object, S3Credentials};
use crate::services::sync_protocol::MAX_SYNC_ARTIFACT_BYTES;
use crate::settings::S3SyncSettings;

const OCTET_STREAM: &str = "application/octet-stream";

/// S3-backed object store. Keys are used verbatim as object keys.
pub struct S3ObjectStorage {
    creds: S3Credentials,
}

impl S3ObjectStorage {
    /// Build from the user's existing S3 credentials.
    pub fn from_settings(settings: &S3SyncSettings) -> Self {
        Self {
            creds: S3Credentials {
                access_key_id: settings.access_key_id.clone(),
                secret_access_key: settings.secret_access_key.clone(),
                region: settings.region.clone(),
                bucket: settings.bucket.clone(),
                endpoint: settings.endpoint.clone(),
            },
        }
    }

    async fn put_unconditional(&self, key: &str, bytes: Bytes) -> Result<PutResult, AppError> {
        put_object(&self.creds, key, bytes.to_vec(), OCTET_STREAM).await?;
        let etag = head_object(&self.creds, key).await.unwrap_or(None);
        Ok(PutResult { etag })
    }
}

#[async_trait]
impl ObjectStorage for S3ObjectStorage {
    async fn get(&self, key: &str) -> Result<Option<RemoteObject>, AppError> {
        let Some((bytes, etag)) =
            get_object(&self.creds, key, MAX_SYNC_ARTIFACT_BYTES as usize).await?
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
        match head_object(&self.creds, key).await? {
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
        if head_object(&self.creds, key).await?.is_some() {
            return Ok(ConditionalPutResult::PreconditionFailed);
        }
        let result = self.put_unconditional(key, bytes).await?;
        Ok(ConditionalPutResult::Written { etag: result.etag })
    }

    async fn exists(&self, key: &str) -> Result<bool, AppError> {
        Ok(head_object(&self.creds, key).await?.is_some())
    }

    async fn list(&self, _prefix: &str) -> Result<Vec<String>, AppError> {
        // Blob GC deferred; see WebDAV adapter notes.
        log::debug!("[workspace_sync] S3 list() is a no-op (blob GC deferred)");
        Ok(Vec::new())
    }

    async fn delete(&self, _key: &str) -> Result<(), AppError> {
        Err(AppError::Message(
            "workspace sync S3 delete is not supported yet (blob GC deferred)".to_string(),
        ))
    }
}
