use crate::cloud_sync::{CloudSyncResult, CloudSyncError};
use serde_json::{json, Value};
use std::collections::HashSet;

#[derive(Debug, Clone)]
pub enum DiffType {
    LocalOnly,
    CloudOnly,
    Modified,
}

#[derive(Debug, Clone)]
pub struct DiffItem {
    pub key: String,
    pub diff_type: DiffType,
    pub local_value: Option<Value>,
    pub cloud_value: Option<Value>,
}

pub struct DiffService;

impl DiffService {
    pub fn new() -> Self {
        Self
    }

    pub fn calculate_diff(&self, local: &Value, cloud: &Value) -> CloudSyncResult<Value> {
        let diff_items = self.compare_values("", local, cloud)?;

        let diff_json = json!({
            "has_conflicts": !diff_items.is_empty(),
            "diff_count": diff_items.len(),
            "differences": diff_items.iter().map(|item| {
                json!({
                    "key": item.key,
                    "type": match item.diff_type {
                        DiffType::LocalOnly => "local_only",
                        DiffType::CloudOnly => "cloud_only",
                        DiffType::Modified => "modified",
                    },
                    "local_value": item.local_value,
                    "cloud_value": item.cloud_value,
                })
            }).collect::<Vec<_>>(),
        });

        Ok(diff_json)
    }

    fn compare_values(&self, prefix: &str, local: &Value, cloud: &Value) -> CloudSyncResult<Vec<DiffItem>> {
        let mut diff_items = Vec::new();

        match (local, cloud) {
            (Value::Object(local_obj), Value::Object(cloud_obj)) => {
                let local_keys: HashSet<_> = local_obj.keys().cloned().collect();
                let cloud_keys: HashSet<_> = cloud_obj.keys().cloned().collect();

                // Keys only in local
                for key in local_keys.difference(&cloud_keys) {
                    let full_key = if prefix.is_empty() {
                        key.clone()
                    } else {
                        format!("{}.{}", prefix, key)
                    };

                    diff_items.push(DiffItem {
                        key: full_key,
                        diff_type: DiffType::LocalOnly,
                        local_value: local_obj.get(key).cloned(),
                        cloud_value: None,
                    });
                }

                // Keys only in cloud
                for key in cloud_keys.difference(&local_keys) {
                    let full_key = if prefix.is_empty() {
                        key.clone()
                    } else {
                        format!("{}.{}", prefix, key)
                    };

                    diff_items.push(DiffItem {
                        key: full_key,
                        diff_type: DiffType::CloudOnly,
                        local_value: None,
                        cloud_value: cloud_obj.get(key).cloned(),
                    });
                }

                // Keys in both - check for modifications
                for key in local_keys.intersection(&cloud_keys) {
                    let local_val = local_obj.get(key).unwrap();
                    let cloud_val = cloud_obj.get(key).unwrap();

                    let full_key = if prefix.is_empty() {
                        key.clone()
                    } else {
                        format!("{}.{}", prefix, key)
                    };

                    if local_val != cloud_val {
                        if local_val.is_object() && cloud_val.is_object() {
                            // Recursively compare nested objects
                            let nested_diffs = self.compare_values(&full_key, local_val, cloud_val)?;
                            diff_items.extend(nested_diffs);
                        } else {
                            diff_items.push(DiffItem {
                                key: full_key,
                                diff_type: DiffType::Modified,
                                local_value: Some(local_val.clone()),
                                cloud_value: Some(cloud_val.clone()),
                            });
                        }
                    }
                }
            }
            _ if local != cloud => {
                // Non-object values that differ
                diff_items.push(DiffItem {
                    key: prefix.to_string(),
                    diff_type: DiffType::Modified,
                    local_value: Some(local.clone()),
                    cloud_value: Some(cloud.clone()),
                });
            }
            _ => {
                // Values are the same, no diff
            }
        }

        Ok(diff_items)
    }

    pub fn merge_configurations(
        &self,
        local: &Value,
        cloud: &Value,
        strategy: MergeStrategy,
    ) -> CloudSyncResult<Value> {
        match strategy {
            MergeStrategy::PreferLocal => Ok(local.clone()),
            MergeStrategy::PreferCloud => Ok(cloud.clone()),
            MergeStrategy::Merge => self.deep_merge(local, cloud),
        }
    }

    fn deep_merge(&self, local: &Value, cloud: &Value) -> CloudSyncResult<Value> {
        match (local, cloud) {
            (Value::Object(local_obj), Value::Object(cloud_obj)) => {
                let mut merged = local_obj.clone();

                for (key, cloud_value) in cloud_obj.iter() {
                    if let Some(local_value) = local_obj.get(key) {
                        // Key exists in both - merge recursively if both are objects
                        if local_value.is_object() && cloud_value.is_object() {
                            merged.insert(key.clone(), self.deep_merge(local_value, cloud_value)?);
                        } else {
                            // Use cloud value for non-object conflicts (could be made configurable)
                            merged.insert(key.clone(), cloud_value.clone());
                        }
                    } else {
                        // Key only exists in cloud - add it
                        merged.insert(key.clone(), cloud_value.clone());
                    }
                }

                Ok(Value::Object(merged))
            }
            _ => {
                // For non-objects, prefer cloud value
                Ok(cloud.clone())
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum MergeStrategy {
    PreferLocal,
    PreferCloud,
    Merge,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_calculate_diff_no_changes() {
        let service = DiffService::new();
        let config = json!({
            "key1": "value1",
            "key2": "value2"
        });

        let diff = service.calculate_diff(&config, &config).unwrap();
        assert_eq!(diff["has_conflicts"], false);
        assert_eq!(diff["diff_count"], 0);
    }

    #[test]
    fn test_calculate_diff_with_changes() {
        let service = DiffService::new();
        let local = json!({
            "key1": "value1",
            "key2": "local_value",
            "local_only": "test"
        });
        let cloud = json!({
            "key1": "value1",
            "key2": "cloud_value",
            "cloud_only": "test"
        });

        let diff = service.calculate_diff(&local, &cloud).unwrap();
        assert_eq!(diff["has_conflicts"], true);
        assert_eq!(diff["diff_count"], 3); // modified, local_only, cloud_only
    }

    #[test]
    fn test_deep_merge() {
        let service = DiffService::new();
        let local = json!({
            "shared": "local",
            "local_only": "value",
            "nested": {
                "local_nested": "value"
            }
        });
        let cloud = json!({
            "shared": "cloud",
            "cloud_only": "value",
            "nested": {
                "cloud_nested": "value"
            }
        });

        let merged = service.merge_configurations(&local, &cloud, MergeStrategy::Merge).unwrap();

        assert_eq!(merged["shared"], "cloud");
        assert_eq!(merged["local_only"], "value");
        assert_eq!(merged["cloud_only"], "value");
        assert_eq!(merged["nested"]["local_nested"], "value");
        assert_eq!(merged["nested"]["cloud_nested"], "value");
    }
}