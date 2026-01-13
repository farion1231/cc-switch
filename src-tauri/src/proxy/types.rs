use serde::{Deserialize, Serialize};

/// 代理服务器配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyConfig {
    /// 监听地址
    pub listen_address: String,
    /// 监听端口
    pub listen_port: u16,
    /// 最大重试次数
    pub max_retries: u8,
    /// 请求超时时间（秒）- 已废弃，保留兼容
    pub request_timeout: u64,
    /// 是否启用日志
    pub enable_logging: bool,
    /// 是否正在接管 Live 配置
    #[serde(default)]
    pub live_takeover_active: bool,
    /// 流式首字超时（秒）- 等待首个数据块的最大时间
    #[serde(default = "default_streaming_first_byte_timeout")]
    pub streaming_first_byte_timeout: u64,
    /// 流式静默超时（秒）- 两个数据块之间的最大间隔
    #[serde(default = "default_streaming_idle_timeout")]
    pub streaming_idle_timeout: u64,
    /// 非流式总超时（秒）- 非流式请求的总超时时间
    #[serde(default = "default_non_streaming_timeout")]
    pub non_streaming_timeout: u64,
}

fn default_streaming_first_byte_timeout() -> u64 {
    60
}

fn default_streaming_idle_timeout() -> u64 {
    120
}

fn default_non_streaming_timeout() -> u64 {
    600
}

impl Default for ProxyConfig {
    fn default() -> Self {
        Self {
            listen_address: "127.0.0.1".to_string(),
            listen_port: 15721, // 使用较少占用的高位端口
            max_retries: 3,
            request_timeout: 600,
            enable_logging: true,
            live_takeover_active: false,
            streaming_first_byte_timeout: 60,
            streaming_idle_timeout: 120,
            non_streaming_timeout: 600,
        }
    }
}

/// 代理服务器状态
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProxyStatus {
    /// 是否运行中
    pub running: bool,
    /// 监听地址
    pub address: String,
    /// 监听端口
    pub port: u16,
    /// 活跃连接数
    pub active_connections: usize,
    /// 总请求数
    pub total_requests: u64,
    /// 成功请求数
    pub success_requests: u64,
    /// 失败请求数
    pub failed_requests: u64,
    /// 成功率 (0-100)
    pub success_rate: f32,
    /// 运行时间（秒）
    pub uptime_seconds: u64,
    /// 当前使用的Provider名称
    pub current_provider: Option<String>,
    /// 当前Provider的ID
    pub current_provider_id: Option<String>,
    /// 最后一次请求时间
    pub last_request_at: Option<String>,
    /// 最后一次错误信息
    pub last_error: Option<String>,
    /// Provider故障转移次数
    pub failover_count: u64,
    /// 当前活跃的代理目标列表
    #[serde(default)]
    pub active_targets: Vec<ActiveTarget>,
}

/// 活跃的代理目标信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveTarget {
    pub app_type: String, // "Claude" | "Codex" | "Gemini"
    pub provider_name: String,
    pub provider_id: String,
}

/// 代理服务器信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyServerInfo {
    pub address: String,
    pub port: u16,
    pub started_at: String,
}

/// 各应用的接管状态（是否改写该应用的 Live 配置指向本地代理）
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProxyTakeoverStatus {
    pub claude: bool,
    pub codex: bool,
    pub gemini: bool,
}

/// API 格式类型（预留，当前不需要格式转换）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum ApiFormat {
    Claude,
    OpenAI,
    Gemini,
}

/// Provider健康状态
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderHealth {
    pub provider_id: String,
    pub app_type: String,
    pub is_healthy: bool,
    pub consecutive_failures: u32,
    pub last_success_at: Option<String>,
    pub last_failure_at: Option<String>,
    pub last_error: Option<String>,
    pub updated_at: String,
}

/// Live 配置备份记录
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiveBackup {
    /// 应用类型 (claude/codex/gemini)
    pub app_type: String,
    /// 原始配置 JSON
    pub original_config: String,
    /// 备份时间
    pub backed_up_at: String,
}

/// 全局代理配置（统一字段，三行镜像）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GlobalProxyConfig {
    /// 代理总开关
    pub proxy_enabled: bool,
    /// 监听地址
    pub listen_address: String,
    /// 监听端口
    pub listen_port: u16,
    /// 是否启用日志
    pub enable_logging: bool,
}

/// 应用级代理配置（每个 app 独立）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppProxyConfig {
    /// 应用类型 (claude/codex/gemini)
    pub app_type: String,
    /// 该 app 代理启用开关
    pub enabled: bool,
    /// 该 app 自动故障转移开关
    pub auto_failover_enabled: bool,
    /// 最大重试次数
    pub max_retries: u32,
    /// 流式首字超时（秒）
    pub streaming_first_byte_timeout: u32,
    /// 流式静默超时（秒）
    pub streaming_idle_timeout: u32,
    /// 非流式总超时（秒）
    pub non_streaming_timeout: u32,
    /// 熔断失败阈值
    pub circuit_failure_threshold: u32,
    /// 熔断恢复阈值
    pub circuit_success_threshold: u32,
    /// 熔断恢复等待时间（秒）
    pub circuit_timeout_seconds: u32,
    /// 错误率阈值
    pub circuit_error_rate_threshold: f64,
    /// 计算错误率的最小请求数
    pub circuit_min_requests: u32,
}

/// 整流器配置
///
/// 存储在 settings 表中
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RectifierConfig {
    /// 总开关：是否启用整流器
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// 请求整流：启用 thinking 签名整流器
    ///
    /// 处理错误：Invalid 'signature' in 'thinking' block
    #[serde(default = "default_true")]
    pub request_thinking_signature: bool,
}

impl Default for RectifierConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            request_thinking_signature: true,
        }
    }
}

fn default_true() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rectifier_config_default_enabled() {
        // 验证 RectifierConfig::default() 返回全启用状态
        // 防止回归：#[derive(Default)] 会使 bool 默认为 false
        let config = RectifierConfig::default();
        assert!(config.enabled, "整流器总开关默认应为 true");
        assert!(
            config.request_thinking_signature,
            "thinking 签名整流器默认应为 true"
        );
    }

    #[test]
    fn test_rectifier_config_serde_default() {
        // 验证反序列化缺字段时使用 default_true
        let json = "{}";
        let config: RectifierConfig = serde_json::from_str(json).unwrap();
        assert!(config.enabled);
        assert!(config.request_thinking_signature);
    }

    #[test]
    fn test_rectifier_config_serde_explicit_false() {
        // 验证显式设置 false 时正确反序列化
        let json = r#"{"enabled": false, "requestThinkingSignature": false}"#;
        let config: RectifierConfig = serde_json::from_str(json).unwrap();
        assert!(!config.enabled);
        assert!(!config.request_thinking_signature);
    }
}
