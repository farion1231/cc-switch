use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

// SSOT 模式：不再写供应商副本文件

/// 供应商结构体
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Provider {
    pub id: String,
    pub name: String,
    #[serde(rename = "settingsConfig")]
    pub settings_config: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "websiteUrl")]
    pub website_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "createdAt")]
    pub created_at: Option<i64>,
    /// 供应商元数据（不写入 live 配置，仅存于 ~/.cc-switch/config.json）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<ProviderMeta>,

    // 分组和排序相关字段
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "groupId")]
    pub group_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priority: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "contractExpiry")]
    pub contract_expiry: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "lastUsedAt")]
    pub last_used_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "customOrder")]
    pub custom_order: Option<i32>,
}

impl Provider {
    /// 从现有ID创建供应商
    pub fn with_id(
        id: String,
        name: String,
        settings_config: Value,
        website_url: Option<String>,
    ) -> Self {
        Self {
            id,
            name,
            settings_config,
            website_url,
            category: None,
            created_at: None,
            meta: None,
            // 分组和排序相关字段默认为 None
            group_id: None,
            priority: None,
            contract_expiry: None,
            last_used_at: None,
            tags: None,
            custom_order: None,
        }
    }
}

/// 供应商管理器
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderManager {
    pub providers: HashMap<String, Provider>,
    pub current: String,
}

impl Default for ProviderManager {
    fn default() -> Self {
        Self {
            providers: HashMap::new(),
            current: String::new(),
        }
    }
}

/// 供应商元数据
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProviderMeta {
    /// 自定义端点列表（按 URL 去重存储）
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub custom_endpoints: HashMap<String, crate::settings::CustomEndpoint>,
}

impl ProviderManager {
    /// 获取所有供应商
    pub fn get_all_providers(&self) -> &HashMap<String, Provider> {
        &self.providers
    }
}

/// 排序字段枚举
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SortField {
    Name,
    Id,
    CreatedAt,
    LastUsed,
    Priority,
    ContractExpiry,
    Custom,
}

/// 排序顺序
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SortOrder {
    Asc,
    Desc,
}

/// 排序配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SortConfig {
    pub field: SortField,
    pub order: SortOrder,
}

/// 供应商分组
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderGroup {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "parentId")]
    pub parent_id: Option<String>,
    #[serde(rename = "providerIds")]
    pub provider_ids: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub collapsed: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "sortConfig")]
    pub sort_config: Option<SortConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order: Option<i32>,
    #[serde(rename = "createdAt")]
    pub created_at: i64,
    #[serde(rename = "updatedAt")]
    pub updated_at: i64,
}

impl ProviderGroup {
    /// 创建新分组
    pub fn new(name: String) -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;

        Self {
            id: uuid::Uuid::new_v4().to_string(),
            name,
            color: None,
            icon: None,
            parent_id: None,
            provider_ids: Vec::new(),
            collapsed: Some(false),
            sort_config: None,
            order: None,
            created_at: now,
            updated_at: now,
        }
    }
}
