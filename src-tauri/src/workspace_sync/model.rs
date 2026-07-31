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
pub enum DataKind {
    Session,
    Task,
    Todo,
    Plan,
    Goal,
    Memory,
    Index,
    Attachment,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MergeCapability {
    AppendOnly,
    RecordSet,
    Text,
    Opaque,
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Sensitivity {
    WorkData,
    PotentialSecret,
    Blocked,
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_item_serializes_with_stable_camel_case_fields() {
        let item = DataItem {
            provider: WorkspaceProviderId::Codex,
            kind: DataKind::Session,
            logical_id: "thread-1".to_string(),
            parent_id: None,
            native_path: "sessions/thread-1.jsonl".to_string(),
            content_hash: "abc".to_string(),
            updated_at: Some(42),
            schema_fingerprint: Some("codex-v1".to_string()),
            merge_capability: MergeCapability::AppendOnly,
            sensitivity: Sensitivity::WorkData,
            object_ids: vec!["obj-1".to_string()],
        };

        let value = serde_json::to_value(item).expect("data item should serialize");

        assert_eq!(value["provider"], "codex");
        assert_eq!(value["logicalId"], "thread-1");
        assert_eq!(value["mergeCapability"], "appendOnly");
    }

    #[test]
    fn data_item_without_timestamp_or_schema_round_trips_and_deserializes() {
        let item = DataItem {
            provider: WorkspaceProviderId::Codex,
            kind: DataKind::Session,
            logical_id: "thread-1".to_string(),
            parent_id: None,
            native_path: "sessions/thread-1.jsonl".to_string(),
            content_hash: "abc".to_string(),
            updated_at: None,
            schema_fingerprint: None,
            merge_capability: MergeCapability::AppendOnly,
            sensitivity: Sensitivity::WorkData,
            object_ids: vec!["obj-1".to_string()],
        };

        let value = serde_json::to_value(&item).expect("data item should serialize");
        let round_tripped: DataItem =
            serde_json::from_value(value.clone()).expect("data item should deserialize");
        assert_eq!(round_tripped, item);

        let mut without_optional_fields = value;
        without_optional_fields
            .as_object_mut()
            .expect("data item should serialize as an object")
            .remove("updatedAt");
        without_optional_fields
            .as_object_mut()
            .expect("data item should serialize as an object")
            .remove("schemaFingerprint");

        let deserialized: DataItem = serde_json::from_value(without_optional_fields)
            .expect("missing timestamp and schema should deserialize");
        assert_eq!(deserialized.updated_at, None);
        assert_eq!(deserialized.schema_fingerprint, None);
    }

    #[test]
    fn provider_snapshot_without_schema_round_trips_and_deserializes() {
        let snapshot = ProviderSnapshot {
            provider: WorkspaceProviderId::Codex,
            adapter_version: 1,
            native_version: None,
            schema_fingerprint: None,
            items: Vec::new(),
        };

        let value = serde_json::to_value(&snapshot).expect("provider snapshot should serialize");
        let round_tripped: ProviderSnapshot =
            serde_json::from_value(value.clone()).expect("provider snapshot should deserialize");
        assert_eq!(round_tripped, snapshot);

        let mut without_schema = value;
        without_schema
            .as_object_mut()
            .expect("provider snapshot should serialize as an object")
            .remove("schemaFingerprint");

        let deserialized: ProviderSnapshot = serde_json::from_value(without_schema)
            .expect("missing provider schema should deserialize");
        assert_eq!(deserialized.schema_fingerprint, None);
    }

    #[test]
    fn workspace_provider_id_uses_stable_wire_values() {
        let cases = [
            (WorkspaceProviderId::Claude, "claude"),
            (WorkspaceProviderId::Codex, "codex"),
            (WorkspaceProviderId::GrokBuild, "grokbuild"),
            (WorkspaceProviderId::OpenCode, "opencode"),
            (WorkspaceProviderId::Cursor, "cursor"),
        ];

        for (provider, wire_value) in cases {
            let serialized = serde_json::to_string(&provider).expect("provider should serialize");
            assert_eq!(serialized, format!(r#""{wire_value}""#));

            let deserialized: WorkspaceProviderId =
                serde_json::from_str(&serialized).expect("provider should deserialize");
            assert_eq!(deserialized, provider);
        }
    }
}
