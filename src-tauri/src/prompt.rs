use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Prompt {
    pub id: String,
    pub name: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub enabled: bool,
    #[serde(rename = "createdAt", skip_serializing_if = "Option::is_none")]
    pub created_at: Option<i64>,
    #[serde(rename = "updatedAt", skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<i64>,
}

/// 全局提示词：一份内容，可以同时用到多个应用（类似 Skills 的统一管理）。
///
/// 只存内容本身。用到某个应用时，会在该应用的 `prompts` 里落一条 id 为
/// [`linked_prompt_id`] 的普通记录，再走该应用原有的启用 / 写文件流程；
/// 「在哪些应用里启用」按这些记录现算，不另存开关。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GlobalPrompt {
    #[serde(default)]
    pub id: String,
    pub name: String,
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(rename = "createdAt", default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<i64>,
    #[serde(rename = "updatedAt", default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<i64>,
}

/// 全局提示词落到各应用 `prompts` 表里的记录 id 前缀。
pub const GLOBAL_PROMPT_LINK_PREFIX: &str = "global-";

/// 全局提示词 `global_id` 在每个应用里对应的记录 id（各应用相同）。
pub fn linked_prompt_id(global_id: &str) -> String {
    format!("{GLOBAL_PROMPT_LINK_PREFIX}{global_id}")
}
