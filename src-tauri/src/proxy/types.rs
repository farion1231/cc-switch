use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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
    /// 流式首字超时（秒）- 等待首个数据块的最大时间，范围 1-120 秒，默认 60 秒
    #[serde(default = "default_streaming_first_byte_timeout")]
    pub streaming_first_byte_timeout: u64,
    /// 流式静默超时（秒）- 两个数据块之间的最大间隔，范围 60-600 秒，填 0 禁用（防止中途卡住）
    #[serde(default = "default_streaming_idle_timeout")]
    pub streaming_idle_timeout: u64,
    /// 非流式总超时（秒）- 非流式请求的总超时时间，范围 60-1200 秒，默认 600 秒（10 分钟）
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
    pub grokbuild: bool,
    pub opencode: bool,
    pub openclaw: bool,
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

/// 模型层级路由：按请求的模型层级（opus/sonnet/haiku/fable）把请求分发到不同 Provider，
/// 并把 `body.model` 改写为该层级的真实上游模型名。
///
/// 与每个 Provider 自身的「层级→模型名」env 映射解耦：路由表自包含 providerId + model，
/// 命中层级的请求会同时换 Provider 与模型名；其余层级/未命中时回退到既有 Provider 选择。
///
/// 存于 settings 表（key = `model_tier_routing_config`），沿用 rectifier/optimizer 的 JSON 模式。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ModelTierRoutingConfig {
    /// 总开关；关闭时完全不介入请求路径。
    ///
    /// 兼容旧配置：历史版本只有这个字段，且仅 Claude Code 使用层级路由。
    #[serde(default)]
    pub enabled: bool,
    /// per app_type → 是否启用层级路由。
    ///
    /// 缺失时按旧配置解释：`enabled=true` 只代表 `claude` 开启，避免升级后误把
    /// `claude-desktop` 也切到层级路由。
    #[serde(default)]
    pub enabled_apps: HashMap<String, bool>,
    /// per app_type → tier（"opus"/"sonnet"/"haiku"/"fable"）→ 路由项。
    ///
    /// Legacy 字段：单方案版本直接读写此字段。新版本保存到 `profiles`，运行时仍把
    /// 该字段作为兼容回退，避免旧配置升级后丢失路由。
    #[serde(default)]
    pub routes: HashMap<String, HashMap<String, TierRoute>>,
    /// 多套路由方案。每套方案自包含 app_type → tier → route 的完整映射。
    #[serde(default)]
    pub profiles: Vec<ModelTierRoutingProfile>,
    /// per app_type → 当前使用的 profile id。缺失时回退到第一套 profile；若没有
    /// profiles，则回退到 legacy `routes`。
    #[serde(default)]
    pub active_profile_by_app: HashMap<String, String>,
}

impl ModelTierRoutingConfig {
    pub fn is_enabled_for_app(&self, app_type: &str) -> bool {
        if !self.enabled {
            return false;
        }
        self.enabled_apps
            .get(app_type)
            .copied()
            .unwrap_or(app_type == "claude")
    }

    pub fn active_profile_id_for_app(&self, app_type: &str) -> Option<&str> {
        self.active_profile_by_app
            .get(app_type)
            .map(String::as_str)
            .filter(|id| !id.trim().is_empty())
            .or_else(|| self.profiles.first().map(|profile| profile.id.as_str()))
    }

    pub fn active_routes_for_app(&self, app_type: &str) -> Option<&HashMap<String, TierRoute>> {
        if let Some(profile_id) = self.active_profile_id_for_app(app_type) {
            if let Some(profile) = self
                .profiles
                .iter()
                .find(|profile| profile.id == profile_id)
            {
                if let Some(routes) = profile.routes.get(app_type) {
                    return Some(routes);
                }
            }
        }
        if !self.profiles.is_empty() {
            return None;
        }
        self.routes.get(app_type)
    }

    pub fn active_route_for_tier(&self, app_type: &str, tier: &str) -> Option<&TierRoute> {
        self.active_routes_for_app(app_type)
            .and_then(|routes| routes.get(tier))
    }

    /// Normalize a config before persisting/returning it: migrate legacy `routes`
    /// into a default profile, repair blank/duplicate profile ids, and ensure
    /// active profile ids point at an existing profile when possible.
    pub fn normalized(mut self) -> Self {
        if self.profiles.is_empty() && !self.routes.is_empty() {
            self.profiles.push(ModelTierRoutingProfile {
                id: "default".to_string(),
                name: "Default".to_string(),
                routes: self.routes.clone(),
            });
            self.routes.clear();
        }

        let mut seen = std::collections::HashSet::new();
        for (idx, profile) in self.profiles.iter_mut().enumerate() {
            let id = profile.id.trim();
            let mut repaired_id = if id.is_empty() {
                format!("profile-{}", idx + 1)
            } else {
                id.to_string()
            };
            if !seen.insert(repaired_id.clone()) {
                let base = repaired_id;
                let mut suffix = 2;
                loop {
                    repaired_id = format!("{base}-{suffix}");
                    if seen.insert(repaired_id.clone()) {
                        break;
                    }
                    suffix += 1;
                }
            }
            profile.id = repaired_id;
            if profile.name.trim().is_empty() {
                profile.name = format!("Profile {}", idx + 1);
            }
        }

        if let Some(first_id) = self.profiles.first().map(|profile| profile.id.clone()) {
            let valid_profile_ids: std::collections::HashSet<String> = self
                .profiles
                .iter()
                .map(|profile| profile.id.clone())
                .collect();
            self.active_profile_by_app
                .retain(|_, profile_id| valid_profile_ids.contains(profile_id));
            for app in ["claude", "claude-desktop"] {
                self.active_profile_by_app
                    .entry(app.to_string())
                    .or_insert_with(|| first_id.clone());
            }
            self.routes.clear();
        }

        self
    }
}

/// 一套路由方案。方案本身可以同时保存 Claude Code / Claude Desktop 的映射，
/// 当前生效方案由 `active_profile_by_app` 按 app_type 选择。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ModelTierRoutingProfile {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub routes: HashMap<String, HashMap<String, TierRoute>>,
}

/// 单条层级路由：目标 Provider + 要改写成的上游模型名。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TierRoute {
    /// 目标 Provider 的 id（必须存在于该 app 的 providers 中）。
    pub provider_id: String,
    /// 改写后的上游模型名，如 `glm-5.2`、`kimi-k2.6`。
    pub model: String,
    /// 展示名，写入 `ANTHROPIC_DEFAULT_<TIER>_MODEL_NAME`，供 Claude Code 的
    /// 模型选择菜单显示（如 `GLM-5.2`）。空字符串表示不写该 `_NAME` 变量。
    #[serde(default)]
    pub display_name: String,
    /// 是否向 Claude 声明该层级支持 1M 上下文。
    ///
    /// 勾选 → Claude Code 接管时给 `*_MODEL` 别名补 `[1M]`；Claude Desktop profile
    /// 标 `supports1m=true`。与 `ClaudeDesktopModelRoute.supports_1m` 同义，是 1M
    /// 能力声明的单一真相源（取代旧版「在 model 名后手敲 `[1m]` 后缀」的隐式约定）。
    /// 兼容旧数据：未勾选但 `model` 带 `[1m]` 后缀时，各派生点仍按后缀回退生效。
    #[serde(default)]
    pub supports_1m: bool,
}

/// 整流器配置
///
/// 存储在 settings 表中
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RectifierConfig {
    /// 总开关：是否启用整流器（默认开启）
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// 请求整流：启用 thinking 签名整流器（默认开启）
    ///
    /// 处理错误：Invalid 'signature' in 'thinking' block
    #[serde(default = "default_true")]
    pub request_thinking_signature: bool,
    /// 请求整流：启用 thinking budget 整流器（默认开启）
    ///
    /// 处理错误：budget_tokens + thinking 相关约束
    #[serde(default = "default_true")]
    pub request_thinking_budget: bool,
    /// 请求整流：不支持的图片降级（默认开启）
    ///
    /// 上游拒绝图片输入时，把图片块替换为 [Unsupported Image] 标记，
    /// 让对话不中断。总开关，管辖「显式声明 text-only」与「上游报错后兜底」两条事实驱动路径。
    #[serde(default = "default_true")]
    pub request_media_fallback: bool,
    /// 请求整流：确认纯文本注册表的发送前降级（默认开启）
    ///
    /// 在模型未声明能力时，按内置的确认纯文本注册表预先剥离图片。
    /// 受 request_media_fallback 管辖；单独关闭只停用代理的注册表预判，
    /// 仍保留「显式声明」与「上游兜底」，且不改变 Codex 模型目录声明。
    #[serde(default = "default_true")]
    pub request_media_heuristic: bool,
}

fn default_true() -> bool {
    true
}

fn default_log_level() -> String {
    "info".to_string()
}

impl Default for RectifierConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            request_thinking_signature: true,
            request_thinking_budget: true,
            request_media_fallback: true,
            request_media_heuristic: true,
        }
    }
}

/// 请求优化器配置
///
/// 存储在 settings 表中，key = "optimizer_config"
/// 仅对 Bedrock provider 生效（CLAUDE_CODE_USE_BEDROCK = "1"）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OptimizerConfig {
    /// 总开关（默认关闭，用户需手动启用）
    #[serde(default)]
    pub enabled: bool,
    /// Thinking 优化子开关（总开关开启后默认生效）
    #[serde(default = "default_true")]
    pub thinking_optimizer: bool,
    /// Cache 注入子开关（总开关开启后默认生效）
    #[serde(default = "default_true")]
    pub cache_injection: bool,
}

impl Default for OptimizerConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            thinking_optimizer: true,
            cache_injection: true,
        }
    }
}

/// Copilot 优化器配置
///
/// 存储在 settings 表中，key = "copilot_optimizer_config"
/// 解决 Copilot 代理消耗量异常问题（Issue #1813）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CopilotOptimizerConfig {
    /// 总开关（默认开启 — 对 Copilot 用户至关重要）
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// x-initiator 请求分类（默认开启，P0 优先级）
    #[serde(default = "default_true")]
    pub request_classification: bool,
    /// Tool result 消息合并（默认开启，P1 优先级）
    #[serde(default = "default_true")]
    pub tool_result_merging: bool,
    /// Compact 请求识别（默认开启，P2 优先级）
    #[serde(default = "default_true")]
    pub compact_detection: bool,
    /// 确定性 Request ID（默认开启，P3 优先级）
    #[serde(default = "default_true")]
    pub deterministic_request_id: bool,
    /// Subagent 检测（默认开启）— 识别 Claude Code 子代理请求，
    /// 设置 x-initiator=agent + x-interaction-type=conversation-subagent，避免子代理计费
    #[serde(default = "default_true")]
    pub subagent_detection: bool,
    /// Warmup 小模型降级（默认开启 — 与参考实现对齐，避免探针请求消耗 premium quota）
    #[serde(default = "default_true")]
    pub warmup_downgrade: bool,
    /// Warmup 降级使用的模型（默认 "gpt-5-mini"）
    #[serde(default = "default_warmup_model")]
    pub warmup_model: String,
    /// 请求前主动剥离 assistant 消息里的 thinking / redacted_thinking block
    ///
    /// Copilot 走 OpenAI 兼容端点，thinking block 会被上游拒绝并触发 rectifier 反应式
    /// 重试，那时第一次请求已经消耗了一次 premium quota。主动剥离避免这次浪费。
    #[serde(default = "default_true")]
    pub strip_thinking: bool,
}

fn default_warmup_model() -> String {
    "gpt-5-mini".to_string()
}

impl Default for CopilotOptimizerConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            request_classification: true,
            tool_result_merging: true,
            compact_detection: true,
            deterministic_request_id: true,
            subagent_detection: true,
            warmup_downgrade: true,
            warmup_model: "gpt-5-mini".to_string(),
            strip_thinking: true,
        }
    }
}

/// 日志配置
///
/// 存储在 settings 表的 log_config 字段中（JSON 格式）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogConfig {
    /// 总开关：是否启用日志
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// 日志级别: error, warn, info, debug, trace
    #[serde(default = "default_log_level")]
    pub level: String,
}

impl Default for LogConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            level: "info".to_string(),
        }
    }
}

impl LogConfig {
    /// 将配置转换为 log::LevelFilter
    pub fn to_level_filter(&self) -> log::LevelFilter {
        if !self.enabled {
            return log::LevelFilter::Off;
        }
        match self.level.to_lowercase().as_str() {
            "error" => log::LevelFilter::Error,
            "warn" => log::LevelFilter::Warn,
            "info" => log::LevelFilter::Info,
            "debug" => log::LevelFilter::Debug,
            "trace" => log::LevelFilter::Trace,
            _ => log::LevelFilter::Info,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rectifier_config_default_enabled() {
        // 验证 RectifierConfig::default() 返回全开启状态
        let config = RectifierConfig::default();
        assert!(config.enabled, "整流器总开关默认应为 true");
        assert!(
            config.request_thinking_signature,
            "thinking 签名整流器默认应为 true"
        );
        assert!(
            config.request_thinking_budget,
            "thinking budget 整流器默认应为 true"
        );
        assert!(
            config.request_media_fallback,
            "media 降级总开关默认应为 true"
        );
        assert!(
            config.request_media_heuristic,
            "启发式 text-only 模型识别默认应为 true"
        );
    }

    #[test]
    fn test_rectifier_config_serde_default() {
        // 验证反序列化缺字段时使用默认值 true
        let json = "{}";
        let config: RectifierConfig = serde_json::from_str(json).unwrap();
        assert!(config.enabled);
        assert!(config.request_thinking_signature);
        assert!(config.request_thinking_budget);
        assert!(
            config.request_media_fallback,
            "缺 requestMediaFallback 时应回退默认值 true"
        );
        assert!(
            config.request_media_heuristic,
            "缺 requestMediaHeuristic 时应回退默认值 true"
        );
    }

    #[test]
    fn test_rectifier_config_serde_explicit_true() {
        // 验证显式设置 true 时正确反序列化
        let json =
            r#"{"enabled": true, "requestThinkingSignature": true, "requestThinkingBudget": true}"#;
        let config: RectifierConfig = serde_json::from_str(json).unwrap();
        assert!(config.enabled);
        assert!(config.request_thinking_signature);
        assert!(config.request_thinking_budget);
    }

    #[test]
    fn test_rectifier_config_serde_partial_fields() {
        // 验证只设置部分字段时，缺失字段使用默认值 true
        let json = r#"{"enabled": true, "requestThinkingSignature": false}"#;
        let config: RectifierConfig = serde_json::from_str(json).unwrap();
        assert!(config.enabled);
        assert!(!config.request_thinking_signature);
        assert!(config.request_thinking_budget);
    }

    #[test]
    fn test_rectifier_config_serde_media_explicit_false() {
        // 验证 media 两字段显式 false 时被如实反序列化（用户主动关闭须生效，不能被默认值覆盖）
        let json = r#"{"requestMediaFallback": false, "requestMediaHeuristic": false}"#;
        let config: RectifierConfig = serde_json::from_str(json).unwrap();
        assert!(!config.request_media_fallback);
        assert!(!config.request_media_heuristic);
        // 其余字段仍走默认 true
        assert!(config.enabled);
        assert!(config.request_thinking_signature);
        assert!(config.request_thinking_budget);
    }

    #[test]
    fn test_log_config_default() {
        let config = LogConfig::default();
        assert!(config.enabled);
        assert_eq!(config.level, "info");
    }

    #[test]
    fn test_log_config_serde_default() {
        let json = "{}";
        let config: LogConfig = serde_json::from_str(json).unwrap();
        assert!(config.enabled);
        assert_eq!(config.level, "info");
    }

    #[test]
    fn test_log_config_to_level_filter() {
        let config = LogConfig {
            level: "error".to_string(),
            ..Default::default()
        };
        assert_eq!(config.to_level_filter(), log::LevelFilter::Error);

        let config = LogConfig {
            level: "warn".to_string(),
            ..Default::default()
        };
        assert_eq!(config.to_level_filter(), log::LevelFilter::Warn);

        let config = LogConfig {
            level: "info".to_string(),
            ..Default::default()
        };
        assert_eq!(config.to_level_filter(), log::LevelFilter::Info);

        let config = LogConfig {
            level: "debug".to_string(),
            ..Default::default()
        };
        assert_eq!(config.to_level_filter(), log::LevelFilter::Debug);

        let config = LogConfig {
            level: "trace".to_string(),
            ..Default::default()
        };
        assert_eq!(config.to_level_filter(), log::LevelFilter::Trace);

        // 无效级别回退到 info
        let config = LogConfig {
            level: "invalid".to_string(),
            ..Default::default()
        };
        assert_eq!(config.to_level_filter(), log::LevelFilter::Info);

        // 禁用时返回 Off
        let config = LogConfig {
            enabled: false,
            level: "debug".to_string(),
        };
        assert_eq!(config.to_level_filter(), log::LevelFilter::Off);
    }

    #[test]
    fn test_log_config_serde_roundtrip() {
        let config = LogConfig {
            enabled: true,
            level: "debug".to_string(),
        };
        let json = serde_json::to_string(&config).unwrap();
        let parsed: LogConfig = serde_json::from_str(&json).unwrap();
        assert!(parsed.enabled);
        assert_eq!(parsed.level, "debug");
    }
}
