use async_trait::async_trait;
use bytes::Bytes;

use crate::AppError;

pub mod memory;
pub mod s3;
pub mod webdav;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemoteObject {
    pub bytes: Bytes,
    pub etag: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PutResult {
    pub etag: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConditionalPutResult {
    Written { etag: Option<String> },
    PreconditionFailed,
    Unsupported,
}

#[async_trait]
pub trait ObjectStorage: Send + Sync {
    async fn get(&self, key: &str) -> Result<Option<RemoteObject>, AppError>;

    async fn put(&self, key: &str, bytes: Bytes) -> Result<PutResult, AppError>;

    async fn put_if_match(
        &self,
        key: &str,
        etag: &str,
        bytes: Bytes,
    ) -> Result<ConditionalPutResult, AppError>;

    async fn put_if_absent(
        &self,
        key: &str,
        bytes: Bytes,
    ) -> Result<ConditionalPutResult, AppError>;

    async fn exists(&self, key: &str) -> Result<bool, AppError>;

    async fn list(&self, prefix: &str) -> Result<Vec<String>, AppError>;

    async fn delete(&self, key: &str) -> Result<(), AppError>;
}
