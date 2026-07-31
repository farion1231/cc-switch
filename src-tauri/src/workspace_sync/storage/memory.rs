use std::{
    collections::BTreeMap,
    sync::{Arc, RwLock},
};

use async_trait::async_trait;
use bytes::Bytes;
use sha2::{Digest, Sha256};

use super::{ConditionalPutResult, ObjectStorage, PutResult, RemoteObject};
use crate::AppError;

#[derive(Clone, Debug)]
struct StoredObject {
    bytes: Bytes,
    etag: String,
}

#[derive(Debug, Default)]
struct MemoryState {
    objects: BTreeMap<String, StoredObject>,
    revision: u64,
}

#[derive(Clone, Debug, Default)]
pub struct MemoryStorage {
    state: Arc<RwLock<MemoryState>>,
}

impl MemoryStorage {
    fn next_etag(state: &mut MemoryState, bytes: &Bytes) -> Result<String, AppError> {
        let revision = state
            .revision
            .checked_add(1)
            .ok_or_else(|| AppError::Message("memory storage revision exhausted".to_string()))?;
        let digest = Sha256::digest(bytes);
        let digest = digest
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();

        state.revision = revision;
        Ok(format!("{revision}-{digest}"))
    }

    #[cfg(test)]
    pub(crate) fn put_count(&self) -> Result<u64, AppError> {
        Ok(self.state.read()?.revision)
    }
}

#[async_trait]
impl ObjectStorage for MemoryStorage {
    async fn get(&self, key: &str) -> Result<Option<RemoteObject>, AppError> {
        let state = self.state.read()?;
        Ok(state.objects.get(key).map(|object| RemoteObject {
            bytes: object.bytes.clone(),
            etag: Some(object.etag.clone()),
        }))
    }

    async fn put(&self, key: &str, bytes: Bytes) -> Result<PutResult, AppError> {
        let mut state = self.state.write()?;
        let etag = Self::next_etag(&mut state, &bytes)?;
        state.objects.insert(
            key.to_string(),
            StoredObject {
                bytes,
                etag: etag.clone(),
            },
        );
        Ok(PutResult { etag: Some(etag) })
    }

    async fn put_if_match(
        &self,
        key: &str,
        etag: &str,
        bytes: Bytes,
    ) -> Result<ConditionalPutResult, AppError> {
        let mut state = self.state.write()?;
        let matches = state
            .objects
            .get(key)
            .is_some_and(|object| object.etag == etag);
        if !matches {
            return Ok(ConditionalPutResult::PreconditionFailed);
        }

        let new_etag = Self::next_etag(&mut state, &bytes)?;
        state.objects.insert(
            key.to_string(),
            StoredObject {
                bytes,
                etag: new_etag.clone(),
            },
        );
        Ok(ConditionalPutResult::Written {
            etag: Some(new_etag),
        })
    }

    async fn put_if_absent(
        &self,
        key: &str,
        bytes: Bytes,
    ) -> Result<ConditionalPutResult, AppError> {
        let mut state = self.state.write()?;
        if state.objects.contains_key(key) {
            return Ok(ConditionalPutResult::PreconditionFailed);
        }

        let etag = Self::next_etag(&mut state, &bytes)?;
        state.objects.insert(
            key.to_string(),
            StoredObject {
                bytes,
                etag: etag.clone(),
            },
        );
        Ok(ConditionalPutResult::Written { etag: Some(etag) })
    }

    async fn exists(&self, key: &str) -> Result<bool, AppError> {
        Ok(self.state.read()?.objects.contains_key(key))
    }

    async fn list(&self, prefix: &str) -> Result<Vec<String>, AppError> {
        Ok(self
            .state
            .read()?
            .objects
            .keys()
            .filter(|key| key.starts_with(prefix))
            .cloned()
            .collect())
    }

    async fn delete(&self, key: &str) -> Result<(), AppError> {
        self.state.write()?.objects.remove(key);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use bytes::Bytes;

    use super::MemoryStorage;
    use crate::{
        workspace_sync::storage::{ConditionalPutResult, ObjectStorage},
        AppError,
    };

    #[tokio::test]
    async fn stale_etag_does_not_replace_head() -> Result<(), AppError> {
        let storage = MemoryStorage::default();

        let initial = storage.put("head", Bytes::from_static(b"a")).await?;
        let initial_etag = initial.etag.expect("memory storage must return an ETag");

        let replacement = storage
            .put_if_match("head", &initial_etag, Bytes::from_static(b"b"))
            .await?;
        assert!(matches!(
            replacement,
            ConditionalPutResult::Written { etag: Some(_) }
        ));

        let stale_write = storage
            .put_if_match("head", &initial_etag, Bytes::from_static(b"c"))
            .await?;
        assert_eq!(stale_write, ConditionalPutResult::PreconditionFailed);

        let current = storage.get("head").await?.expect("head should still exist");
        assert_eq!(current.bytes, Bytes::from_static(b"b"));

        let create_existing = storage
            .put_if_absent("head", Bytes::from_static(b"c"))
            .await?;
        assert_eq!(create_existing, ConditionalPutResult::PreconditionFailed);

        Ok(())
    }

    #[tokio::test]
    async fn put_if_absent_writes_missing_key() -> Result<(), AppError> {
        let storage = MemoryStorage::default();

        let result = storage
            .put_if_absent("head", Bytes::from_static(b"a"))
            .await?;

        assert!(matches!(
            result,
            ConditionalPutResult::Written { etag: Some(_) }
        ));
        assert_eq!(
            storage.get("head").await?.map(|object| object.bytes),
            Some(Bytes::from_static(b"a"))
        );
        Ok(())
    }

    #[tokio::test]
    async fn list_filters_by_prefix_and_sorts_keys() -> Result<(), AppError> {
        let storage = MemoryStorage::default();
        storage.put("objects/c", Bytes::new()).await?;
        storage.put("unrelated", Bytes::new()).await?;
        storage.put("objects/a", Bytes::new()).await?;
        storage.put("objects/b", Bytes::new()).await?;

        assert_eq!(
            storage.list("objects/").await?,
            vec!["objects/a", "objects/b", "objects/c"]
        );
        Ok(())
    }

    #[tokio::test]
    async fn delete_is_idempotent() -> Result<(), AppError> {
        let storage = MemoryStorage::default();
        storage.put("head", Bytes::from_static(b"a")).await?;

        storage.delete("head").await?;
        storage.delete("head").await?;

        assert!(!storage.exists("head").await?);
        Ok(())
    }

    #[tokio::test]
    async fn overwriting_identical_bytes_changes_etag() -> Result<(), AppError> {
        let storage = MemoryStorage::default();
        let first = storage.put("head", Bytes::from_static(b"a")).await?;
        let second = storage.put("head", Bytes::from_static(b"a")).await?;

        assert_ne!(first.etag, second.etag);
        assert_eq!(storage.put_count()?, 2);
        Ok(())
    }
}
