use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    error::AppError,
    workspace_sync::model::{DataKind, ProviderSnapshot, Tombstone, WorkspaceProviderId},
};

pub const PROTOCOL_FORMAT: &str = "cc-switch-workspace-sync";
pub const PROTOCOL_VERSION: u32 = 1;
pub const ENCRYPTION_ALGORITHM: &str = "XChaCha20-Poly1305";
pub const KDF_ALGORITHM: &str = "Argon2id";

const MIN_SALT_BYTES: usize = 16;
const MAX_SALT_BYTES: usize = 64;
const KEY_CHECK_BYTES: usize = 32;
const MAX_MEMORY_KIB: u32 = 262_144;
const MAX_ITERATIONS: u32 = 5;
const MAX_PARALLELISM: u32 = 4;
const MAX_KIB_PASSES: u64 = 524_288;
const MIN_MEMORY_KIB_PER_LANE: u32 = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EncryptionMode {
    Encrypted,
    None,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EncryptionBootstrap {
    pub mode: EncryptionMode,
    pub algorithm: Option<String>,
    pub kdf: Option<String>,
    pub salt: Option<String>,
    pub memory_kib: Option<u32>,
    pub iterations: Option<u32>,
    pub parallelism: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Bootstrap {
    pub format: String,
    pub version: u32,
    pub encryption: EncryptionBootstrap,
    pub key_check: String,
}

impl Bootstrap {
    pub fn validate(&self) -> Result<(), AppError> {
        if self.format != PROTOCOL_FORMAT {
            return Err(invalid_input("unsupported workspace sync format"));
        }
        if self.version != PROTOCOL_VERSION {
            return Err(invalid_input("unsupported workspace sync protocol version"));
        }

        match self.encryption.mode {
            EncryptionMode::Encrypted => self.validate_encrypted(),
            EncryptionMode::None => self.validate_unencrypted(),
        }
    }

    fn validate_encrypted(&self) -> Result<(), AppError> {
        let algorithm = required_text(
            self.encryption.algorithm.as_deref(),
            "encrypted workspace sync requires an encryption algorithm",
        )?;
        if normalize_algorithm_name(algorithm) != "xchacha20poly1305" {
            return Err(invalid_input(
                "unsupported workspace sync encryption algorithm",
            ));
        }

        let kdf = required_text(
            self.encryption.kdf.as_deref(),
            "encrypted workspace sync requires a KDF",
        )?;
        if normalize_algorithm_name(kdf) != "argon2id" {
            return Err(invalid_input("unsupported workspace sync KDF"));
        }

        let salt = required_text(
            self.encryption.salt.as_deref(),
            "encrypted workspace sync requires a salt",
        )?;
        if !is_hex_bytes_in_range(salt, MIN_SALT_BYTES, MAX_SALT_BYTES) {
            return Err(invalid_input(
                "workspace sync salt must be 16 to 64 bytes of hexadecimal data",
            ));
        }

        let memory_kib = self
            .encryption
            .memory_kib
            .ok_or_else(|| invalid_input("encrypted workspace sync requires KDF memory"))?;
        let iterations = self
            .encryption
            .iterations
            .ok_or_else(|| invalid_input("encrypted workspace sync requires KDF iterations"))?;
        let parallelism = self
            .encryption
            .parallelism
            .ok_or_else(|| invalid_input("encrypted workspace sync requires KDF parallelism"))?;
        let kib_passes = u64::from(memory_kib) * u64::from(iterations);
        let minimum_memory_kib = u64::from(MIN_MEMORY_KIB_PER_LANE) * u64::from(parallelism);
        if u64::from(memory_kib) < minimum_memory_kib
            || memory_kib > MAX_MEMORY_KIB
            || iterations == 0
            || iterations > MAX_ITERATIONS
            || parallelism == 0
            || parallelism > MAX_PARALLELISM
            || kib_passes > MAX_KIB_PASSES
        {
            return Err(invalid_input("invalid workspace sync KDF parameters"));
        }

        if !is_hex_bytes_in_range(&self.key_check, KEY_CHECK_BYTES, KEY_CHECK_BYTES) {
            return Err(invalid_input(
                "workspace sync key check must be 32 bytes of hexadecimal data",
            ));
        }

        Ok(())
    }

    fn validate_unencrypted(&self) -> Result<(), AppError> {
        let has_encryption_parameters = self.encryption.algorithm.is_some()
            || self.encryption.kdf.is_some()
            || self.encryption.salt.is_some()
            || self.encryption.memory_kib.is_some()
            || self.encryption.iterations.is_some()
            || self.encryption.parallelism.is_some();
        if has_encryption_parameters || !self.key_check.is_empty() {
            return Err(invalid_input(
                "unencrypted workspace sync cannot include encryption parameters or key check",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceRecord {
    pub device_id: String,
    pub device_name: String,
    pub last_snapshot_id: Option<String>,
    pub last_seen_at: i64,
    pub removed_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Head {
    pub snapshot_id: String,
    pub updated_at: i64,
    pub updated_by: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotContent {
    pub parents: Vec<String>,
    pub providers: BTreeMap<WorkspaceProviderId, ProviderSnapshot>,
    pub tombstones: Vec<Tombstone>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotManifest {
    pub snapshot_id: String,
    pub created_at: i64,
    pub created_by: String,
    pub content: SnapshotContent,
}

impl SnapshotManifest {
    pub fn new(
        mut content: SnapshotContent,
        device: impl Into<String>,
        created_at: i64,
    ) -> Result<Self, AppError> {
        normalize_content(&mut content)?;
        let canonical =
            serde_json::to_vec(&content).map_err(|source| AppError::JsonSerialize { source })?;
        let snapshot_id = encode_hex(&Sha256::digest(canonical));

        Ok(Self {
            snapshot_id,
            created_at,
            created_by: device.into(),
            content,
        })
    }
}

fn normalize_content(content: &mut SnapshotContent) -> Result<(), AppError> {
    content.parents.sort();
    content.parents.dedup();

    for snapshot in content.providers.values_mut() {
        normalize_provider_items(snapshot)?;
    }

    normalize_tombstones(&mut content.tombstones)?;

    Ok(())
}

fn normalize_tombstones(tombstones: &mut Vec<Tombstone>) -> Result<(), AppError> {
    let mut unique = BTreeMap::new();

    for tombstone in std::mem::take(tombstones) {
        let key = (
            tombstone.provider,
            data_kind_rank(tombstone.kind),
            tombstone.logical_id.clone(),
        );

        if let Some(existing) = unique.get(&key) {
            if existing != &tombstone {
                return Err(invalid_input(format!(
                    "conflicting duplicate workspace tombstone: provider={:?}, kind={:?}, logical_id={}",
                    tombstone.provider, tombstone.kind, tombstone.logical_id
                )));
            }
            continue;
        }

        unique.insert(key, tombstone);
    }

    *tombstones = unique.into_values().collect();

    Ok(())
}

fn normalize_provider_items(snapshot: &mut ProviderSnapshot) -> Result<(), AppError> {
    let mut unique = BTreeMap::new();

    for mut item in std::mem::take(&mut snapshot.items) {
        item.object_ids.sort();
        item.object_ids.dedup();
        let key = (
            item.provider,
            data_kind_rank(item.kind),
            item.logical_id.clone(),
        );

        if let Some(existing) = unique.get(&key) {
            if existing != &item {
                return Err(invalid_input(format!(
                    "conflicting duplicate workspace item: provider={:?}, kind={:?}, logical_id={}",
                    item.provider, item.kind, item.logical_id
                )));
            }
            continue;
        }

        unique.insert(key, item);
    }

    snapshot.items = unique.into_values().collect();
    snapshot.items.sort_by(|left, right| {
        left.provider
            .cmp(&right.provider)
            .then_with(|| data_kind_rank(left.kind).cmp(&data_kind_rank(right.kind)))
            .then_with(|| left.logical_id.cmp(&right.logical_id))
    });

    Ok(())
}

fn data_kind_rank(kind: DataKind) -> u8 {
    match kind {
        DataKind::Session => 0,
        DataKind::Task => 1,
        DataKind::Todo => 2,
        DataKind::Plan => 3,
        DataKind::Goal => 4,
        DataKind::Memory => 5,
        DataKind::Index => 6,
        DataKind::Attachment => 7,
    }
}

fn normalize_algorithm_name(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn required_text<'a>(value: Option<&'a str>, message: &str) -> Result<&'a str, AppError> {
    value
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| invalid_input(message))
}

fn is_hex_bytes_in_range(value: &str, min_bytes: usize, max_bytes: usize) -> bool {
    value.len() % 2 == 0
        && (min_bytes * 2..=max_bytes * 2).contains(&value.len())
        && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(HEX[(byte >> 4) as usize] as char);
        encoded.push(HEX[(byte & 0x0f) as usize] as char);
    }
    encoded
}

fn invalid_input(message: impl Into<String>) -> AppError {
    AppError::InvalidInput(message.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace_sync::model::{DataItem, MergeCapability, Sensitivity};

    fn item(
        provider: WorkspaceProviderId,
        kind: DataKind,
        logical_id: &str,
        native_path: &str,
        content_hash: &str,
        object_ids: &[&str],
    ) -> DataItem {
        DataItem {
            provider,
            kind,
            logical_id: logical_id.to_string(),
            parent_id: None,
            native_path: native_path.to_string(),
            content_hash: content_hash.to_string(),
            updated_at: Some(42),
            schema_fingerprint: Some("schema-v1".to_string()),
            merge_capability: MergeCapability::AppendOnly,
            sensitivity: Sensitivity::WorkData,
            object_ids: object_ids
                .iter()
                .map(|value| (*value).to_string())
                .collect(),
        }
    }

    fn provider_snapshot(items: Vec<DataItem>) -> ProviderSnapshot {
        ProviderSnapshot {
            provider: WorkspaceProviderId::Codex,
            adapter_version: 1,
            native_version: Some("1.0.0".to_string()),
            schema_fingerprint: Some("schema-v1".to_string()),
            items,
        }
    }

    fn content_with_items(items: Vec<DataItem>) -> SnapshotContent {
        SnapshotContent {
            parents: vec!["parent-b".to_string(), "parent-a".to_string()],
            providers: BTreeMap::from([(WorkspaceProviderId::Codex, provider_snapshot(items))]),
            tombstones: vec![],
        }
    }

    #[test]
    fn snapshot_id_ignores_device_and_created_time() {
        let content = SnapshotContent {
            parents: vec!["base".to_string()],
            providers: BTreeMap::new(),
            tombstones: vec![],
        };

        let a = SnapshotManifest::new(content.clone(), "device-a", 1)
            .expect("first manifest should be valid");
        let b =
            SnapshotManifest::new(content, "device-b", 2).expect("second manifest should be valid");

        assert_eq!(a.snapshot_id, b.snapshot_id);
        assert_ne!(a.created_by, b.created_by);
        assert_ne!(a.created_at, b.created_at);
    }

    #[test]
    fn input_order_does_not_change_snapshot_id() {
        let first = item(
            WorkspaceProviderId::Codex,
            DataKind::Task,
            "task-1",
            "tasks/1.json",
            "hash-a",
            &["object-b", "object-a", "object-a"],
        );
        let second = item(
            WorkspaceProviderId::Codex,
            DataKind::Session,
            "session-1",
            "sessions/1.jsonl",
            "hash-b",
            &["object-d", "object-c"],
        );

        let mut left = content_with_items(vec![first.clone(), second.clone()]);
        left.tombstones = vec![
            Tombstone {
                provider: WorkspaceProviderId::Codex,
                kind: DataKind::Task,
                logical_id: "deleted-b".to_string(),
                last_known_hash: Some("old-b".to_string()),
                deleted_at: 20,
                deleted_by: "device-b".to_string(),
            },
            Tombstone {
                provider: WorkspaceProviderId::Claude,
                kind: DataKind::Session,
                logical_id: "deleted-a".to_string(),
                last_known_hash: None,
                deleted_at: 10,
                deleted_by: "device-a".to_string(),
            },
        ];

        let mut right = content_with_items(vec![second, first]);
        right.parents.reverse();
        right.parents.push("parent-a".to_string());
        right.tombstones = left.tombstones.iter().cloned().rev().collect();
        right
            .providers
            .get_mut(&WorkspaceProviderId::Codex)
            .expect("provider should exist")
            .items[1]
            .object_ids
            .reverse();

        let left = SnapshotManifest::new(left, "device-a", 1).expect("left should be valid");
        let right = SnapshotManifest::new(right, "device-b", 2).expect("right should be valid");

        assert_eq!(left.snapshot_id, right.snapshot_id);
        assert_eq!(left.content, right.content);
        assert_eq!(left.content.parents, vec!["parent-a", "parent-b"]);
        assert_eq!(
            left.content.providers[&WorkspaceProviderId::Codex].items[1].object_ids,
            vec!["object-a", "object-b"]
        );
    }

    #[test]
    fn content_change_changes_snapshot_id() {
        let original = content_with_items(vec![item(
            WorkspaceProviderId::Codex,
            DataKind::Session,
            "session-1",
            "sessions/1.jsonl",
            "hash-a",
            &["object-a"],
        )]);
        let mut changed = original.clone();
        changed
            .providers
            .get_mut(&WorkspaceProviderId::Codex)
            .expect("provider should exist")
            .items[0]
            .content_hash = "hash-b".to_string();

        let original =
            SnapshotManifest::new(original, "device-a", 1).expect("original should be valid");
        let changed =
            SnapshotManifest::new(changed, "device-a", 1).expect("changed should be valid");

        assert_ne!(original.snapshot_id, changed.snapshot_id);
    }

    #[test]
    fn conflicting_duplicate_logical_keys_are_rejected() {
        let first = item(
            WorkspaceProviderId::Codex,
            DataKind::Session,
            "session-1",
            "sessions/1.jsonl",
            "hash-a",
            &["object-a"],
        );
        let mut conflicting = first.clone();
        conflicting.content_hash = "hash-b".to_string();

        let result =
            SnapshotManifest::new(content_with_items(vec![first, conflicting]), "device-a", 1);

        assert!(matches!(
            result,
            Err(crate::error::AppError::InvalidInput(_))
        ));
    }

    #[test]
    fn exact_duplicate_logical_keys_are_deduplicated() {
        let first = item(
            WorkspaceProviderId::Codex,
            DataKind::Session,
            "session-1",
            "sessions/1.jsonl",
            "hash-a",
            &["object-b", "object-a"],
        );
        let mut duplicate = first.clone();
        duplicate.object_ids.reverse();

        let manifest =
            SnapshotManifest::new(content_with_items(vec![first, duplicate]), "device-a", 1)
                .expect("equivalent duplicates should be accepted");

        assert_eq!(
            manifest.content.providers[&WorkspaceProviderId::Codex]
                .items
                .len(),
            1
        );
    }

    #[test]
    fn duplicate_tombstones_are_deduplicated_or_rejected() {
        let tombstone = Tombstone {
            provider: WorkspaceProviderId::Codex,
            kind: DataKind::Task,
            logical_id: "task-1".to_string(),
            last_known_hash: Some("old-a".to_string()),
            deleted_at: 20,
            deleted_by: "device-a".to_string(),
        };

        let mut duplicate_content = SnapshotContent {
            parents: vec![],
            providers: BTreeMap::new(),
            tombstones: vec![tombstone.clone(), tombstone.clone()],
        };
        let manifest = SnapshotManifest::new(duplicate_content.clone(), "device-a", 1)
            .expect("equivalent tombstones should be accepted");
        assert_eq!(manifest.content.tombstones, vec![tombstone.clone()]);

        duplicate_content.tombstones[1].last_known_hash = Some("old-b".to_string());
        let result = SnapshotManifest::new(duplicate_content, "device-a", 1);
        assert!(matches!(
            result,
            Err(crate::error::AppError::InvalidInput(_))
        ));
    }

    #[test]
    fn manifest_serde_is_stable_and_camel_case() {
        let manifest = SnapshotManifest::new(
            SnapshotContent {
                parents: vec![],
                providers: BTreeMap::new(),
                tombstones: vec![],
            },
            "device-a",
            7,
        )
        .expect("manifest should be valid");

        assert_eq!(
            manifest.snapshot_id,
            "e773ae1f1585187ededc03ec72cca4c20b38f8ee2bfea56bd4aea9f28ec42e21"
        );
        let json = serde_json::to_string(&manifest).expect("manifest should serialize");
        assert_eq!(
            json,
            format!(
                "{{\"snapshotId\":\"{}\",\"createdAt\":7,\"createdBy\":\"device-a\",\"content\":{{\"parents\":[],\"providers\":{{}},\"tombstones\":[]}}}}",
                manifest.snapshot_id
            )
        );
        let round_tripped: SnapshotManifest =
            serde_json::from_str(&json).expect("manifest should deserialize");
        assert_eq!(round_tripped, manifest);
    }

    fn encrypted_bootstrap() -> Bootstrap {
        Bootstrap {
            format: PROTOCOL_FORMAT.to_string(),
            version: PROTOCOL_VERSION,
            encryption: EncryptionBootstrap {
                mode: EncryptionMode::Encrypted,
                algorithm: Some("XChaCha20-Poly1305".to_string()),
                kdf: Some("Argon2id".to_string()),
                salt: Some("00112233445566778899aabbccddeeff".to_string()),
                memory_kib: Some(65_536),
                iterations: Some(3),
                parallelism: Some(1),
            },
            key_check: "00".repeat(32),
        }
    }

    #[test]
    fn bootstrap_rejects_wrong_format_or_version() {
        encrypted_bootstrap()
            .validate()
            .expect("canonical encrypted bootstrap should be valid");

        let mut wrong_format = encrypted_bootstrap();
        wrong_format.format = "other-format".to_string();
        assert!(wrong_format.validate().is_err());

        let mut wrong_version = encrypted_bootstrap();
        wrong_version.version += 1;
        assert!(wrong_version.validate().is_err());
    }

    #[test]
    fn bootstrap_rejects_invalid_encryption_parameter_formats() {
        let mut invalid_salt = encrypted_bootstrap();
        invalid_salt.encryption.salt = Some("not-hex".to_string());
        assert!(invalid_salt.validate().is_err());

        let mut invalid_key_check = encrypted_bootstrap();
        invalid_key_check.key_check = "00".repeat(31);
        assert!(invalid_key_check.validate().is_err());

        let mut excessive_kdf = encrypted_bootstrap();
        excessive_kdf.encryption.memory_kib = Some(262_145);
        assert!(excessive_kdf.validate().is_err());

        let mut below_argon2_minimum = encrypted_bootstrap();
        below_argon2_minimum.encryption.memory_kib = Some(7);
        assert!(below_argon2_minimum.validate().is_err());

        let mut below_parallelism_minimum = encrypted_bootstrap();
        below_parallelism_minimum.encryption.memory_kib = Some(16);
        below_parallelism_minimum.encryption.parallelism = Some(4);
        assert!(below_parallelism_minimum.validate().is_err());

        let mut excessive_parallelism = encrypted_bootstrap();
        excessive_parallelism.encryption.parallelism = Some(u32::MAX);
        assert!(excessive_parallelism.validate().is_err());
    }

    #[test]
    fn bootstrap_rejects_mixed_or_incomplete_encryption_modes() {
        let mut none_with_encrypted_parameters = encrypted_bootstrap();
        none_with_encrypted_parameters.encryption.mode = EncryptionMode::None;
        assert!(none_with_encrypted_parameters.validate().is_err());

        let mut encrypted_without_key_check = encrypted_bootstrap();
        encrypted_without_key_check.key_check.clear();
        assert!(encrypted_without_key_check.validate().is_err());

        let none = Bootstrap {
            format: PROTOCOL_FORMAT.to_string(),
            version: PROTOCOL_VERSION,
            encryption: EncryptionBootstrap {
                mode: EncryptionMode::None,
                algorithm: None,
                kdf: None,
                salt: None,
                memory_kib: None,
                iterations: None,
                parallelism: None,
            },
            key_check: String::new(),
        };
        none.validate().expect("none mode should be valid");
    }
}
