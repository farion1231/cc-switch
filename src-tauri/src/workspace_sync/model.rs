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
    pub updated_at: i64,
    pub schema_fingerprint: String,
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
    pub schema_fingerprint: String,
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
            updated_at: 42,
            schema_fingerprint: "codex-v1".to_string(),
            merge_capability: MergeCapability::AppendOnly,
            sensitivity: Sensitivity::WorkData,
            object_ids: vec!["obj-1".to_string()],
        };

        let value = serde_json::to_value(item).expect("data item should serialize");

        assert_eq!(value["provider"], "codex");
        assert_eq!(value["logicalId"], "thread-1");
        assert_eq!(value["mergeCapability"], "appendOnly");
    }
}
