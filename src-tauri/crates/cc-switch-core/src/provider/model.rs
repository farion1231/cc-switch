use serde::{Deserialize, Serialize};
use serde_json::Value;

/// 桌面与 Agent 共用的 Provider 传输模型。
///
/// meta 保持开放 JSON，避免临时 Agent 因版本较旧而删除桌面端新增字段；需要参与业务判断的
/// 少数字段由对应服务窄化解析，不能在 DTO 层封闭整个对象。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderRecord {
    pub id: String,
    pub name: String,
    pub settings_config: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub website_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sort_index: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon_color: Option<String>,
    #[serde(default)]
    pub in_failover_queue: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
/// 批量排序的最小更新项；领域服务会把整批更新放进同一事务。
pub struct ProviderSortUpdate {
    pub id: String,
    pub sort_index: i64,
}
