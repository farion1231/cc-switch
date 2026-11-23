//! 兼容目标概览的数据格式；本阶段不探测或管理 WSL 客户端。
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum InstallStatus {
    Installed,
    /// 找到了但跑不起来（比如 Node 版本不够）。
    Broken,
    NotInstalled,
    /// 探测失败，不等于未安装。
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolInstall {
    pub status: InstallStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// 发行版里 `command -v` 找到的可执行文件路径。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// 本次探测失败的原因；使用上次安装结果时与原来的客户端错误分开显示。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub probe_error: Option<String>,
    pub detected_at: i64,
    /// 使用上次结果（发行版停止、缓存过期或本次探测失败）。
    #[serde(default)]
    pub cached: bool,
}

/// 发行版 → 应用 ID → 安装情况。
pub type Installs = BTreeMap<String, BTreeMap<String, ToolInstall>>;
