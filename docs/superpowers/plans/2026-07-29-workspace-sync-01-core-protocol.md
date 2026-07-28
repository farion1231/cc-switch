# Workspace Sync Core Protocol Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add the transport-independent encrypted snapshot foundation, local journal database, and compare-and-swap object storage required by later provider adapters.

**Architecture:** Introduce a focused `workspace_sync` module without changing the existing WebDAV/S3 snapshot services. Store local transaction metadata in a separate SQLite database and use immutable encrypted objects plus a conditionally updated remote Head.

**Tech Stack:** Rust, rusqlite, async-trait, bytes, Argon2id, HKDF, HMAC-SHA256, XChaCha20-Poly1305, reqwest, existing WebDAV/S3 transports.

---

## File Map

- Create `src-tauri/src/workspace_sync/mod.rs`: module exports.
- Create `src-tauri/src/workspace_sync/model.rs`: provider IDs, data kinds, snapshot and merge-neutral data types.
- Create `src-tauri/src/workspace_sync/state_db.rs`: independent `workspace-sync.db` schema and DAO.
- Create `src-tauri/src/workspace_sync/storage/mod.rs`: object-storage trait and CAS result types.
- Create `src-tauri/src/workspace_sync/storage/memory.rs`: deterministic test transport.
- Create `src-tauri/src/workspace_sync/storage/webdav.rs`: WebDAV adapter.
- Create `src-tauri/src/workspace_sync/storage/s3.rs`: S3 adapter.
- Create `src-tauri/src/workspace_sync/crypto.rs`: KDF, subkeys, object IDs, AEAD.
- Create `src-tauri/src/workspace_sync/manifest.rs`: bootstrap, Head, snapshot manifest and deterministic IDs.
- Create `src-tauri/src/workspace_sync/blob_store.rs`: encrypted object upload/download and dedup.
- Create `src-tauri/src/workspace_sync/repository.rs`: immutable snapshot and CAS Head repository.
- Modify `src-tauri/src/lib.rs`: register the module only; no commands yet.
- Modify `src-tauri/Cargo.toml`: cryptography and async-trait dependencies.

### Task 1: Add core model types

**Files:**
- Create: `src-tauri/src/workspace_sync/mod.rs`
- Create: `src-tauri/src/workspace_sync/model.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Write the failing serialization test**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_item_serializes_with_stable_camel_case_fields() {
        let item = DataItem {
            provider: WorkspaceProviderId::Codex,
            kind: DataKind::Session,
            logical_id: "thread-1".into(),
            parent_id: None,
            native_path: "sessions/thread-1.jsonl".into(),
            content_hash: "abc".into(),
            updated_at: Some(42),
            schema_fingerprint: Some("codex-v1".into()),
            merge_capability: MergeCapability::AppendOnly,
            sensitivity: Sensitivity::WorkData,
            object_ids: vec!["obj-1".into()],
        };
        let value = serde_json::to_value(item).unwrap();
        assert_eq!(value["provider"], "codex");
        assert_eq!(value["logicalId"], "thread-1");
        assert_eq!(value["mergeCapability"], "appendOnly");
    }
}
```

- [ ] **Step 2: Run the test and verify it fails**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::model::tests::data_item_serializes -- --exact
```

Expected: FAIL because `workspace_sync` and `DataItem` do not exist.

- [ ] **Step 3: Implement the model types**

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WorkspaceProviderId {
    Claude,
    Codex,
    GrokBuild,
    OpenCode,
    Cursor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DataKind { Session, Task, Todo, Plan, Goal, Memory, Index, Attachment }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MergeCapability { AppendOnly, RecordSet, Text, Opaque, Unsupported }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Sensitivity { WorkData, PotentialSecret, Blocked }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DataItem {
    pub provider: WorkspaceProviderId,
    pub kind: DataKind,
    pub logical_id: String,
    pub parent_id: Option<String>,
    pub native_path: String,
    pub content_hash: String,
    pub updated_at: Option<i64>,
    pub schema_fingerprint: Option<String>,
    pub merge_capability: MergeCapability,
    pub sensitivity: Sensitivity,
    pub object_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSnapshot {
    pub provider: WorkspaceProviderId,
    pub adapter_version: u32,
    pub native_version: Option<String>,
    pub schema_fingerprint: Option<String>,
    pub items: Vec<DataItem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tombstone {
    pub provider: WorkspaceProviderId,
    pub kind: DataKind,
    pub logical_id: String,
    pub last_known_hash: Option<String>,
    pub deleted_at: i64,
    pub deleted_by: String,
}
```

Create `src-tauri/src/workspace_sync/mod.rs`:

```rust
pub mod model;
```

Add to `src-tauri/src/lib.rs` with the other private modules:

```rust
mod workspace_sync;
```

- [ ] **Step 4: Run the model test**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::model::tests::data_item_serializes -- --exact
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/lib.rs src-tauri/src/workspace_sync
git commit -m "feat(sync): add workspace data model"
```

### Task 2: Add the independent local state database

**Files:**
- Create: `src-tauri/src/workspace_sync/state_db.rs`
- Modify: `src-tauri/src/workspace_sync/mod.rs`

- [ ] **Step 1: Write a failing schema test**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initializes_workspace_sync_schema_v1() {
        let db = WorkspaceSyncDb::memory().unwrap();
        assert_eq!(db.user_version().unwrap(), 1);
        for table in [
            "sync_transactions", "provider_results", "conflicts", "tombstones",
            "devices", "snapshot_cache", "blob_refs", "provider_schema_cache",
        ] {
            assert!(db.table_exists(table).unwrap(), "missing {table}");
        }
    }
}
```

- [ ] **Step 2: Run the test and verify it fails**

```bash
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::state_db::tests::initializes_workspace_sync_schema_v1 -- --exact
```

Expected: FAIL because `WorkspaceSyncDb` is undefined.

- [ ] **Step 3: Implement schema creation**

Implement `WorkspaceSyncDb` with `Connection::open`, `Connection::open_in_memory`, `PRAGMA foreign_keys=ON`, `PRAGMA journal_mode=WAL`, and one transaction that creates the eight tables defined in the approved design. End initialization with:

```rust
conn.pragma_update(None, "user_version", 1)?;
```

Expose these exact methods:

```rust
pub struct WorkspaceSyncDb { conn: std::sync::Mutex<rusqlite::Connection> }
impl WorkspaceSyncDb {
    pub fn open(path: &std::path::Path) -> Result<Self, AppError>;
    pub fn memory() -> Result<Self, AppError>;
    pub fn user_version(&self) -> Result<i32, AppError>;
    #[cfg(test)] fn table_exists(&self, table: &str) -> Result<bool, AppError>;
}
```

Add to `mod.rs`:

```rust
pub mod state_db;
```

- [ ] **Step 4: Run the schema test and the database suite**

```bash
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::state_db::tests::initializes_workspace_sync_schema_v1 -- --exact
cargo test --manifest-path src-tauri/Cargo.toml database::
```

Expected: PASS; existing main database tests remain unchanged.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/workspace_sync/state_db.rs src-tauri/src/workspace_sync/mod.rs
git commit -m "feat(sync): add workspace sync journal database"
```

### Task 3: Add object storage and an in-memory CAS transport

**Files:**
- Create: `src-tauri/src/workspace_sync/storage/mod.rs`
- Create: `src-tauri/src/workspace_sync/storage/memory.rs`
- Modify: `src-tauri/src/workspace_sync/mod.rs`
- Modify: `src-tauri/Cargo.toml`

- [ ] **Step 1: Add the dependency and failing CAS test**

Add:

```toml
async-trait = "0.1"
```

Test:

```rust
#[tokio::test]
async fn stale_etag_does_not_replace_head() {
    let storage = MemoryStorage::default();
    let first = storage.put("head.enc", bytes::Bytes::from_static(b"a")).await.unwrap();
    let second = storage.put_if_match("head.enc", &first.etag.unwrap(), bytes::Bytes::from_static(b"b")).await.unwrap();
    assert!(matches!(second, ConditionalPutResult::Written { .. }));
    let stale = storage.put_if_match("head.enc", &first.etag.unwrap(), bytes::Bytes::from_static(b"c")).await.unwrap();
    assert_eq!(stale, ConditionalPutResult::PreconditionFailed);
    assert_eq!(storage.get("head.enc").await.unwrap().unwrap().bytes, bytes::Bytes::from_static(b"b"));
    assert!(matches!(storage.put_if_absent("head.enc", bytes::Bytes::from_static(b"d")).await.unwrap(), ConditionalPutResult::PreconditionFailed));
}
```

- [ ] **Step 2: Run and verify failure**

```bash
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::storage::memory::tests::stale_etag_does_not_replace_head -- --exact
```

Expected: FAIL because the storage types do not exist.

- [ ] **Step 3: Implement the trait and memory transport**

Define in `storage/mod.rs`:

```rust
#[derive(Debug, Clone)]
pub struct RemoteObject { pub bytes: bytes::Bytes, pub etag: Option<String> }
#[derive(Debug, Clone)]
pub struct PutResult { pub etag: Option<String> }
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConditionalPutResult { Written { etag: Option<String> }, PreconditionFailed, Unsupported }

#[async_trait::async_trait]
pub trait ObjectStorage: Send + Sync {
    async fn get(&self, key: &str) -> Result<Option<RemoteObject>, AppError>;
    async fn put(&self, key: &str, bytes: bytes::Bytes) -> Result<PutResult, AppError>;
    async fn put_if_match(&self, key: &str, etag: &str, bytes: bytes::Bytes)
        -> Result<ConditionalPutResult, AppError>;
    async fn put_if_absent(&self, key: &str, bytes: bytes::Bytes)
        -> Result<ConditionalPutResult, AppError>;
    async fn exists(&self, key: &str) -> Result<bool, AppError>;
    async fn list(&self, prefix: &str) -> Result<Vec<String>, AppError>;
    async fn delete(&self, key: &str) -> Result<(), AppError>;
}
```

Implement `MemoryStorage` with an `Arc<RwLock<BTreeMap<String, StoredObject>>>`; generate the ETag as SHA-256 of bytes plus a monotonically increasing revision so replacing identical bytes still changes ETag.

- [ ] **Step 4: Run the CAS test**

```bash
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::storage::memory::tests::stale_etag_does_not_replace_head -- --exact
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/src/workspace_sync
git commit -m "feat(sync): add object storage abstraction"
```

### Task 4: Add password derivation and authenticated encryption

**Files:**
- Create: `src-tauri/src/workspace_sync/crypto.rs`
- Modify: `src-tauri/src/workspace_sync/mod.rs`
- Modify: `src-tauri/Cargo.toml`

- [ ] **Step 1: Add dependencies and failing tests**

```toml
argon2 = "0.5"
chacha20poly1305 = "0.10"
hkdf = "0.12"
zeroize = { version = "1", features = ["derive"] }
rand = "0.8"
```

```rust
#[test]
fn encrypted_blob_round_trips_and_rejects_wrong_key() {
    let keys = KeyMaterial::derive(b"password", &[7; 16], KdfParams::test()).unwrap();
    let encrypted = keys.encrypt_blob(b"hello", b"codex/session").unwrap();
    assert_eq!(keys.decrypt_blob(&encrypted, b"codex/session").unwrap(), b"hello");
    let wrong = KeyMaterial::derive(b"wrong", &[7; 16], KdfParams::test()).unwrap();
    assert!(wrong.decrypt_blob(&encrypted, b"codex/session").is_err());
}

#[test]
fn object_ids_are_stable_but_not_plain_sha256() {
    let keys = KeyMaterial::derive(b"password", &[7; 16], KdfParams::test()).unwrap();
    let first = keys.object_id(b"v1/profile-a/codex/session", b"hello");
    assert_eq!(first, keys.object_id(b"v1/profile-a/codex/session", b"hello"));
    assert_ne!(first, keys.object_id(b"v1/profile-a/claude/session", b"hello"));
    assert_ne!(first, crate::services::sync_protocol::sha256_hex(b"hello"));
}
```

- [ ] **Step 2: Run and verify failure**

```bash
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::crypto::tests:: -- --nocapture
```

Expected: FAIL because `KeyMaterial` is undefined.

- [ ] **Step 3: Implement crypto primitives**

Implement:

```rust
#[derive(Clone, Copy)]
pub struct KdfParams { pub memory_kib: u32, pub iterations: u32, pub parallelism: u32 }

#[derive(zeroize::Zeroize, zeroize::ZeroizeOnDrop)]
pub struct KeyMaterial {
    manifest_key: [u8; 32],
    blob_key: [u8; 32],
    object_id_key: [u8; 32],
    nonce_key: [u8; 32],
    key_check_key: [u8; 32],
}
```

Use `Argon2::hash_password_into`, then HKDF labels `manifest`, `blob`, `object-id`, `nonce`, and `key-check`. Use HMAC-SHA256 for keyed object IDs over `protocol/profile/provider/kind/plaintextHash`. Derive the XChaCha nonce from `HMAC(nonce_key, object_id)` and use AAD exactly as passed by the caller.

- [ ] **Step 4: Run crypto tests**

```bash
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::crypto::tests::
```

Expected: PASS, including tamper and wrong-password rejection.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/src/workspace_sync/crypto.rs src-tauri/src/workspace_sync/mod.rs
git commit -m "feat(sync): add end-to-end encryption primitives"
```

### Task 5: Add deterministic manifests and snapshot IDs

**Files:**
- Create: `src-tauri/src/workspace_sync/manifest.rs`
- Modify: `src-tauri/src/workspace_sync/mod.rs`

- [ ] **Step 1: Write the failing deterministic-ID test**

```rust
#[test]
fn snapshot_id_ignores_device_and_created_time() {
    let content = SnapshotContent { parents: vec!["base".into()], providers: BTreeMap::new(), tombstones: vec![] };
    let a = SnapshotManifest::new(content.clone(), "device-a", 1);
    let b = SnapshotManifest::new(content, "device-b", 2);
    assert_eq!(a.snapshot_id, b.snapshot_id);
}
```

- [ ] **Step 2: Run and verify failure**

```bash
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::manifest::tests::snapshot_id_ignores_device_and_created_time -- --exact
```

Expected: FAIL because manifest types do not exist.

- [ ] **Step 3: Implement manifest types**

Use `BTreeMap` and sorted vectors for all content-addressed fields. Compute `snapshot_id` from serialized `SnapshotContent`, not from `created_at` or `created_by`. Define:

```rust
pub struct Bootstrap { pub format: String, pub version: u32, pub encryption: EncryptionBootstrap, pub key_check: String }
pub struct Head { pub snapshot_id: String, pub updated_at: i64, pub updated_by: String }
pub struct SnapshotContent { pub parents: Vec<String>, pub providers: BTreeMap<WorkspaceProviderId, ProviderSnapshot>, pub tombstones: Vec<Tombstone> }
pub struct SnapshotManifest { pub snapshot_id: String, pub created_at: i64, pub created_by: String, pub content: SnapshotContent }
```

Validate that parents and tombstones are sorted before hashing.

- [ ] **Step 4: Run manifest tests**

```bash
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::manifest::tests::
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/workspace_sync/manifest.rs src-tauri/src/workspace_sync/mod.rs
git commit -m "feat(sync): add immutable snapshot manifests"
```

### Task 6: Add encrypted blob storage and deduplication

**Files:**
- Create: `src-tauri/src/workspace_sync/blob_store.rs`
- Modify: `src-tauri/src/workspace_sync/mod.rs`

- [ ] **Step 1: Write the failing dedup test**

```rust
#[tokio::test]
async fn uploading_same_plaintext_reuses_one_remote_object() {
    let storage = Arc::new(MemoryStorage::default());
    let keys = Arc::new(KeyMaterial::derive(b"pw", &[1; 16], KdfParams::test()).unwrap());
    let blobs = BlobStore::new(storage.clone(), keys, "profile-a");
    let a = blobs.put(WorkspaceProviderId::Claude, DataKind::Session, b"same").await.unwrap();
    let b = blobs.put(WorkspaceProviderId::Claude, DataKind::Session, b"same").await.unwrap();
    assert_eq!(a.object_id, b.object_id);
    assert_eq!(storage.list("workspace-v1/profile-a/blobs/").await.unwrap().len(), 1);
}
```

- [ ] **Step 2: Run and verify failure**

```bash
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::blob_store::tests::uploading_same_plaintext_reuses_one_remote_object -- --exact
```

Expected: FAIL because `BlobStore` is undefined.

- [ ] **Step 3: Implement `BlobStore`**

Build keys as:

```text
workspace-v1/<profile>/blobs/<first-two-object-id-chars>/<object-id>.blob
```

Before upload call `exists`; encrypt with AAD `<protocol>/<profile>/<provider>/<kind>/<object-id>`. Return:

```rust
pub struct BlobRef { pub object_id: String, pub plain_size: u64, pub stored_size: u64 }
```

Implement `get` to authenticate before returning plaintext.

- [ ] **Step 4: Run tests**

```bash
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::blob_store::tests::
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/workspace_sync/blob_store.rs src-tauri/src/workspace_sync/mod.rs
git commit -m "feat(sync): add encrypted content-addressed blobs"
```

### Task 7: Add snapshot repository and CAS Head updates

**Files:**
- Create: `src-tauri/src/workspace_sync/repository.rs`
- Modify: `src-tauri/src/workspace_sync/mod.rs`

- [ ] **Step 1: Write the failing concurrent-Head test**

```rust
#[tokio::test]
async fn second_device_must_rebase_after_head_changes() {
    let repo = test_repository();
    let base = repo.create_empty_snapshot("a").await.unwrap();
    let observed = repo.read_head().await.unwrap().unwrap();
    repo.publish_child(&base.snapshot_id, observed.etag.as_deref(), "a").await.unwrap();
    let stale = repo.publish_child(&base.snapshot_id, observed.etag.as_deref(), "b").await;
    assert!(matches!(stale, Err(RepositoryError::HeadChanged)));
}
```

- [ ] **Step 2: Run and verify failure**

```bash
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::repository::tests::second_device_must_rebase_after_head_changes -- --exact
```

Expected: FAIL because repository types do not exist.

- [ ] **Step 3: Implement repository operations**

Implement:

```rust
pub async fn load_bootstrap(&self) -> Result<Option<Bootstrap>, RepositoryError>;
pub async fn write_bootstrap_once(&self, value: &Bootstrap) -> Result<(), RepositoryError>;
pub async fn read_head(&self) -> Result<Option<VersionedHead>, RepositoryError>;
pub async fn load_snapshot(&self, id: &str) -> Result<SnapshotManifest, RepositoryError>;
pub async fn read_device(&self, id: &str) -> Result<Option<DeviceRecord>, RepositoryError>;
pub async fn write_device(&self, device: &DeviceRecord) -> Result<(), RepositoryError>;
pub async fn publish_snapshot(&self, manifest: &SnapshotManifest, expected_head_etag: Option<&str>) -> Result<VersionedHead, RepositoryError>;
```

Upload the encrypted immutable snapshot before conditionally updating encrypted Head. Map `ConditionalPutResult::PreconditionFailed` to `RepositoryError::HeadChanged`.

- [ ] **Step 4: Run repository and core tests**

```bash
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::repository::tests::
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/workspace_sync/repository.rs src-tauri/src/workspace_sync/mod.rs
git commit -m "feat(sync): add snapshot repository with CAS head"
```

### Task 8: Add WebDAV conditional requests

**Files:**
- Modify: `src-tauri/src/services/webdav.rs`
- Create: `src-tauri/src/workspace_sync/storage/webdav.rs`
- Test: `src-tauri/src/workspace_sync/storage/webdav.rs`

- [ ] **Step 1: Write a request-building test**

Extract a pure helper and test:

```rust
#[test]
fn conditional_put_uses_if_match_header() {
    let headers = conditional_headers(Some("etag-1"), false).unwrap();
    assert_eq!(headers.get(reqwest::header::IF_MATCH).unwrap(), "etag-1");
    assert!(headers.get(reqwest::header::IF_NONE_MATCH).is_none());
}
```

- [ ] **Step 2: Run and verify failure**

```bash
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::storage::webdav::tests::conditional_put_uses_if_match_header -- --exact
```

Expected: FAIL because helper and adapter do not exist.

- [ ] **Step 3: Implement low-level conditional PUT**

Add to `services/webdav.rs`:

```rust
pub async fn put_bytes_conditional(
    url: &str,
    auth: &WebDavAuth,
    body: bytes::Bytes,
    content_type: &str,
    if_match: Option<&str>,
    if_none_match_star: bool,
) -> Result<ConditionalWebDavPut, AppError>;
```

Return `PreconditionFailed` for HTTP 412, `Unsupported` for servers that reject conditional headers with 405/501, and preserve current error redaction. Implement `WebDavObjectStorage` as an `ObjectStorage` wrapper; do not modify existing `put_bytes` call sites.

- [ ] **Step 4: Run WebDAV tests and existing sync tests**

```bash
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::storage::webdav::tests::
cargo test --manifest-path src-tauri/Cargo.toml services::webdav
cargo test --manifest-path src-tauri/Cargo.toml services::webdav_sync
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/services/webdav.rs src-tauri/src/workspace_sync/storage/webdav.rs src-tauri/src/workspace_sync/storage/mod.rs
git commit -m "feat(sync): add WebDAV conditional object writes"
```

### Task 9: Add S3 conditional writes

**Files:**
- Modify: `src-tauri/src/services/s3.rs`
- Create: `src-tauri/src/workspace_sync/storage/s3.rs`

- [ ] **Step 1: Write a SigV4 header test**

```rust
#[test]
fn signed_conditional_put_includes_if_match() {
    let request = build_signed_request(test_creds(), Method::PUT, "head.enc", b"x", Some("etag-1")).unwrap();
    assert_eq!(request.headers().get(reqwest::header::IF_MATCH).unwrap(), "etag-1");
    assert!(request.headers().contains_key("authorization"));
}
```

- [ ] **Step 2: Run and verify failure**

```bash
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::storage::s3::tests::signed_conditional_put_includes_if_match -- --exact
```

Expected: FAIL because conditional S3 request support is absent.

- [ ] **Step 3: Implement conditional PUT without changing existing APIs**

Add a new S3 primitive that signs `If-Match` as part of canonical headers and maps HTTP 412 to a precondition result. Wrap it in `S3ObjectStorage` implementing `ObjectStorage`.

- [ ] **Step 4: Run S3 and workspace storage tests**

```bash
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::storage::s3::tests::
cargo test --manifest-path src-tauri/Cargo.toml services::s3
cargo test --manifest-path src-tauri/Cargo.toml services::s3_sync
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/services/s3.rs src-tauri/src/workspace_sync/storage/s3.rs src-tauri/src/workspace_sync/storage/mod.rs
git commit -m "feat(sync): add S3 conditional object writes"
```

### Task 10: Add the WebDAV compatibility lease fallback

**Files:**
- Create: `src-tauri/src/workspace_sync/storage/lease.rs`
- Modify: `src-tauri/src/workspace_sync/storage/mod.rs`
- Modify: `src-tauri/src/workspace_sync/repository.rs`

- [ ] **Step 1: Write a failing competing-lease test**

```rust
#[tokio::test]
async fn only_the_read_back_owner_may_publish_under_compatibility_mode() {
    let storage = Arc::new(NonConditionalMemoryStorage::default());
    let first = LeaseGuard::acquire(storage.clone(), "locks/head", "device-a", Duration::from_secs(30)).await.unwrap();
    let second = LeaseGuard::acquire(storage.clone(), "locks/head", "device-b", Duration::from_secs(30)).await;
    assert!(matches!(second, Err(LeaseError::Busy { .. })));
    first.release().await.unwrap();
}
```

- [ ] **Step 2: Run and verify failure**

```bash
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::storage::lease::tests::only_the_read_back_owner_may_publish_under_compatibility_mode -- --exact
```

Expected: FAIL because the lease fallback does not exist.

- [ ] **Step 3: Implement bounded compatibility locking**

When conditional Head update returns `Unsupported`, write an encrypted lease containing a random owner token, device ID, issued-at, and expires-at. Immediately read it back and proceed only if the token matches. Refuse unexpired foreign leases; permit takeover after expiry plus clock-skew allowance. Refresh before long uploads and always release best-effort. Mark the resulting repository status `compatibilityMode=true` so the UI can warn that this backend provides weaker concurrency guarantees.

- [ ] **Step 4: Run lease and repository race tests**

```bash
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::storage::lease::tests::
cargo test --manifest-path src-tauri/Cargo.toml workspace_sync::repository::tests::
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/workspace_sync/storage/lease.rs src-tauri/src/workspace_sync/storage/mod.rs src-tauri/src/workspace_sync/repository.rs
git commit -m "feat(sync): add compatibility lease locking"
```

### Task 11: Run the Plan 1 verification gate

**Files:** none.

- [ ] **Step 1: Run formatting**

```bash
cargo fmt --check --manifest-path src-tauri/Cargo.toml
```

Expected: PASS.

- [ ] **Step 2: Run Clippy**

```bash
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
```

Expected: PASS.

- [ ] **Step 3: Run backend tests**

```bash
cargo test --manifest-path src-tauri/Cargo.toml
```

Expected: PASS, including the new two-device stale-Head test and all existing sync tests.

- [ ] **Step 4: Record the completed gate**

```bash
git status --short
git log --oneline -10
```

Expected: clean worktree and one focused commit per task.
