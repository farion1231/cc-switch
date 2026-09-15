//! 插件系统核心类型
//!
//! 定义插件挂点阶段、错误类型、请求上下文、外部插件清单以及运行时覆盖配置。
//! 详见 docs/dev/plugin-system-contract.md 第 2 章。

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// 插件挂点阶段
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginStage {
    PreRequest,
    PreSend,
    PostResponse,
    /// SSE 事件级挂点（仅常驻模式外部插件支持，见 [`PluginMode`]）
    SseChunk,
}

/// 外部插件进程模式（plugin.json 的 `mode` 字段，snake_case）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginMode {
    /// 每次调用拉起新进程（默认）：stdin 写单个 JSON 对象后关闭，
    /// stdout 收单个 JSON 对象；无跨调用状态
    #[default]
    Oneshot,
    /// 常驻进程：首次调用时拉起、保持存活，按行交换 JSON（请求一行、响应一行）。
    /// 插件进程可自带跨调用状态（如流式处理的 per-stream 缓冲）；
    /// 调用全局串行化，崩溃后由核心自动重启重试一次。
    /// 唯一支持 `sse_chunk` stage 的模式。
    Persistent,
}

#[derive(Debug, thiserror::Error)]
pub enum PluginError {
    #[error("插件 {plugin_id} 执行失败: {message}")]
    Execution { plugin_id: String, message: String },
    #[error("插件 {plugin_id} 超时 ({timeout_ms}ms)")]
    Timeout { plugin_id: String, timeout_ms: u64 },
    #[error("插件清单无效: {0}")]
    InvalidManifest(String),
    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON 错误: {0}")]
    Json(#[from] serde_json::Error),
}

/// 传给插件的请求上下文（只读快照，避免直接依赖 RequestContext 降低耦合）
#[derive(Debug, Clone, Serialize)]
pub struct PluginRequestContext {
    /// "claude" | "codex" | "gemini" | ...
    pub app_type: String,
    pub session_id: String,
    pub request_model: String,
    pub stage: PluginStage,
    /// PreSend / PostResponse 阶段提供；PreRequest 为 None
    pub provider: Option<PluginProviderInfo>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PluginProviderInfo {
    pub id: String,
    pub name: String,
    /// 是否为 Bedrock 供应商。内置优化器插件（cache 注入 / thinking 优化）用它在
    /// 插件内部实现 Bedrock 门：非 Bedrock 或 provider 缺失时不执行任何改写。
    pub is_bedrock: bool,
}

/// 外部插件清单（`plugin.json`），serde snake_case，未知字段忽略
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct PluginManifest {
    /// 必填，`[a-zA-Z0-9_-]{1,64}`，注册后前缀为 `user:`
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    /// 必填，只能是 `pre_request` / `pre_send` / `post_response`
    /// （`sse_chunk` 仅在 `mode: "persistent"` 下支持）
    pub stages: Vec<PluginStage>,
    /// 进程模式（缺省 oneshot，见 [`PluginMode`]）
    #[serde(default)]
    pub mode: PluginMode,
    /// 默认 500
    #[serde(default = "default_manifest_priority")]
    pub priority: i32,
    /// 必填，argv 数组（第一个元素为可执行文件，相对路径时相对于插件目录解析）
    pub command: Vec<String>,
    /// 可选，默认 10000，上限 60000
    #[serde(default = "default_manifest_timeout_ms")]
    pub timeout_ms: u64,
    /// 可选，默认 true
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// 可选，任意 JSON，随调用透传给插件
    #[serde(default)]
    pub settings: serde_json::Value,
}

impl PluginManifest {
    /// 校验清单合法性（加载外部插件前必须调用）
    pub fn validate(&self) -> Result<(), PluginError> {
        let id_valid = !self.id.is_empty()
            && self.id.len() <= 64
            && self
                .id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
        if !id_valid {
            return Err(PluginError::InvalidManifest(format!(
                "id 无效（仅允许 [a-zA-Z0-9_-]，长度 1-64）: {:?}",
                self.id
            )));
        }

        if self.stages.is_empty() {
            return Err(PluginError::InvalidManifest("stages 不能为空".to_string()));
        }
        for stage in &self.stages {
            if matches!(stage, PluginStage::SseChunk) && self.mode != PluginMode::Persistent {
                return Err(PluginError::InvalidManifest(
                    "sse_chunk stage 仅支持常驻模式（\"mode\": \"persistent\"）".to_string(),
                ));
            }
        }

        if self.command.is_empty() || self.command[0].trim().is_empty() {
            return Err(PluginError::InvalidManifest("command 不能为空".to_string()));
        }

        if self.timeout_ms == 0 || self.timeout_ms > MAX_PLUGIN_TIMEOUT_MS {
            return Err(PluginError::InvalidManifest(format!(
                "timeout_ms 必须在 1-{MAX_PLUGIN_TIMEOUT_MS} 之间: {}",
                self.timeout_ms
            )));
        }

        Ok(())
    }
}

/// 外部插件超时上限（毫秒）
pub const MAX_PLUGIN_TIMEOUT_MS: u64 = 60_000;

fn default_manifest_priority() -> i32 {
    500
}

fn default_manifest_timeout_ms() -> u64 {
    10_000
}

fn default_true() -> bool {
    true
}

/// 单个插件的运行时覆盖（enabled/priority），持久化由 DAO 层负责，注册表只存内存态
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PluginOverride {
    /// None 表示不覆盖默认启用状态
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    /// None 表示不覆盖默认优先级
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priority: Option<i32>,
}

/// 插件系统全局配置
///
/// 存储在 settings 表中，key = "plugins_config"（DAO 由阶段 2b 实现）。
/// `enabled` 为全局开关，默认 true 以保证既有 Bedrock 优化行为不回退。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PluginsConfig {
    pub enabled: bool,
    pub overrides: HashMap<String, PluginOverride>,
}

impl Default for PluginsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            overrides: HashMap::new(),
        }
    }
}

/// 插件元信息（Tauri 命令返回/前端展示用）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginInfo {
    pub id: String,
    pub display_name: String,
    pub description: String,
    pub is_builtin: bool,
    pub stages: Vec<PluginStage>,
    pub priority: i32,
    pub enabled: bool,
    /// 用户插件来自 manifest；内置为 None
    pub version: Option<String>,
    /// 用户插件 manifest 路径；内置为 None
    pub source: Option<String>,
    /// 加载失败的插件（list 也要展示失败条目）
    pub error: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn base_manifest() -> PluginManifest {
        PluginManifest {
            id: "my-plugin".to_string(),
            name: "我的插件".to_string(),
            version: Some("0.1.0".to_string()),
            description: Some("示例".to_string()),
            stages: vec![PluginStage::PreRequest],
            mode: PluginMode::Oneshot,
            priority: 500,
            command: vec!["node".to_string(), "index.js".to_string()],
            timeout_ms: 10_000,
            enabled: true,
            settings: json!({}),
        }
    }

    #[test]
    fn test_stage_snake_case_serde() {
        assert_eq!(
            serde_json::to_value(PluginStage::PreRequest).unwrap(),
            json!("pre_request")
        );
        assert_eq!(
            serde_json::to_value(PluginStage::PreSend).unwrap(),
            json!("pre_send")
        );
        assert_eq!(
            serde_json::to_value(PluginStage::PostResponse).unwrap(),
            json!("post_response")
        );
        let stage: PluginStage = serde_json::from_value(json!("pre_request")).unwrap();
        assert_eq!(stage, PluginStage::PreRequest);
    }

    #[test]
    fn test_manifest_deserialize_defaults() {
        let manifest: PluginManifest = serde_json::from_value(json!({
            "id": "my-plugin",
            "name": "我的插件",
            "stages": ["pre_request"],
            "command": ["node", "index.js"],
            "unknown_field": "ignored"
        }))
        .unwrap();

        assert_eq!(manifest.priority, 500);
        assert_eq!(manifest.timeout_ms, 10_000);
        assert!(manifest.enabled);
        assert_eq!(manifest.version, None);
        assert_eq!(manifest.settings, json!(null));
        manifest.validate().unwrap();
    }

    #[test]
    fn test_manifest_validate_rejects_bad_id() {
        for bad_id in ["", "带中文", "has space", "a!", "a".repeat(65).as_str()] {
            let mut manifest = base_manifest();
            manifest.id = bad_id.to_string();
            assert!(manifest.validate().is_err(), "应拒绝非法 id: {bad_id:?}");
        }
        // 恰好 64 字符合法
        let mut manifest = base_manifest();
        manifest.id = "a".repeat(64);
        manifest.validate().unwrap();
        // 65 字符非法
        let mut manifest = base_manifest();
        manifest.id = "a".repeat(65);
        assert!(manifest.validate().is_err());
    }

    #[test]
    fn test_manifest_validate_rejects_bad_stages() {
        let mut manifest = base_manifest();
        manifest.stages = vec![];
        assert!(manifest.validate().is_err());

        let mut manifest = base_manifest();
        manifest.stages = vec![PluginStage::SseChunk];
        assert!(manifest.validate().is_err());
    }

    #[test]
    fn test_manifest_validate_rejects_bad_command_and_timeout() {
        let mut manifest = base_manifest();
        manifest.command = vec![];
        assert!(manifest.validate().is_err());

        let mut manifest = base_manifest();
        manifest.command = vec!["  ".to_string()];
        assert!(manifest.validate().is_err());

        let mut manifest = base_manifest();
        manifest.timeout_ms = 0;
        assert!(manifest.validate().is_err());

        let mut manifest = base_manifest();
        manifest.timeout_ms = MAX_PLUGIN_TIMEOUT_MS + 1;
        assert!(manifest.validate().is_err());

        let mut manifest = base_manifest();
        manifest.timeout_ms = MAX_PLUGIN_TIMEOUT_MS;
        manifest.validate().unwrap();
    }

    #[test]
    fn test_plugins_config_backward_compat() {
        // 空对象 → 默认值（向后兼容）
        let config: PluginsConfig = serde_json::from_str("{}").unwrap();
        assert!(config.enabled);
        assert!(config.overrides.is_empty());

        let config: PluginsConfig = serde_json::from_str(
            r#"{"enabled": true, "overrides": {"user:my-plugin": {"enabled": false, "priority": 300}}}"#,
        )
        .unwrap();
        assert_eq!(
            config.overrides.get("user:my-plugin").cloned(),
            Some(PluginOverride {
                enabled: Some(false),
                priority: Some(300)
            })
        );
    }
}
