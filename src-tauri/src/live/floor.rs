//! 关键字段（「地板」）与供应商独有字段的定义。
//!
//! 关键字段回答四个问题：请求发到哪、凭什么鉴权、哪个模型名、说哪种协议。它们完全
//! 归供应商所有，切换时一律清空再写目标供应商的值；其余键归用户和客户端，CC Switch
//! 不写也不删。
//!
//! 只有「整个前缀都属于连接和鉴权」的地方才按前缀匹配（`ANTHROPIC_*`、`AWS_*`、
//! Gemini 的 `GOOGLE_*`）。漏掉的键会被当作用户键原样保留，失败方向是安全的。
//!
//! 供应商独有字段是上游的兼容开关和窗口值：切入时写入，切走时只删上一家带进来、而且
//! 值没被改过的。它们不是关键字段，因为用户也可能把它们设成全局。

/// Claude Code 的协议选择器。
///
/// `CLAUDE_CODE_USE_` 不能按前缀匹配：同一前缀下还有 `USE_POWERSHELL_TOOL`、
/// `USE_NATIVE_FILE_SEARCH`、`USE_COWORK_PLUGINS`、`USE_CCR_V2` 这类与供应商无关的
/// 功能开关（Claude Code 2.1.282 核实）。预设里出现的 `CLAUDE_CODE_USE_*` 必须在这里。
pub const CLAUDE_PROTOCOL_SELECTORS: &[&str] = &[
    "CLAUDE_CODE_USE_BEDROCK",
    "CLAUDE_CODE_USE_VERTEX",
    "CLAUDE_CODE_USE_FOUNDRY",
    "CLAUDE_CODE_USE_GATEWAY",
    "CLAUDE_CODE_USE_MANTLE",
    "CLAUDE_CODE_USE_ANTHROPIC_AWS",
    "CLAUDE_CODE_USE_ANTHROPIC_GOOGLE_CLOUD",
];

/// `env` 里整个前缀都属于连接和鉴权的前缀：`ANTHROPIC_*` 是地址、凭据、各档模型名、
/// 自定义头；`AWS_*` 是 Bedrock 的区域与凭据；`VERTEX_REGION_*` 是 Vertex 的分模型区域。
pub const CLAUDE_FLOOR_ENV_PREFIXES: &[&str] = &["ANTHROPIC_", "AWS_", "VERTEX_REGION_"];

/// `env` 里按名字列出的关键字段（协议选择器之外）。
pub const CLAUDE_FLOOR_ENV_KEYS: &[&str] = &[
    "CLAUDE_CODE_SUBAGENT_MODEL",
    "CLAUDE_CODE_SUBAGENT_MODEL_FORCE",
    "CLOUD_ML_REGION",
    // Vertex 的凭据路径。
    "GOOGLE_APPLICATION_CREDENTIALS",
    // 订阅账号的长期 token 及其配套键。
    "CLAUDE_CODE_OAUTH_TOKEN",
    "CLAUDE_CODE_OAUTH_REFRESH_TOKEN",
    "CLAUDE_CODE_OAUTH_SCOPES",
    // 与顶层 apiKeyHelper 配套。
    "CLAUDE_CODE_API_KEY_HELPER_TTL_MS",
];

/// Claude Code `settings.json` 的 `env` 里，这个键是否是关键字段。
///
/// 前端的预设扫描（`tests/config/claudeKeyFields.json`）是这里的镜像，由下面的测试
/// 保证两边一致。
pub fn claude_floor_env(key: &str) -> bool {
    CLAUDE_FLOOR_ENV_PREFIXES
        .iter()
        .any(|prefix| key.starts_with(prefix))
        || CLAUDE_PROTOCOL_SELECTORS.contains(&key)
        || CLAUDE_FLOOR_ENV_KEYS.contains(&key)
        || (key.starts_with("CLAUDE_CODE_SKIP_") && key.ends_with("_AUTH"))
}

/// Claude Code `settings.json` 顶层的关键字段。
pub const CLAUDE_FLOOR_TOP: &[&str] = &[
    "apiKeyHelper",
    "apiBaseUrl",
    "primaryModel",
    "smallFastModel",
    // 旧 Bedrock API Key 预设写在顶层的真实 Key。
    "apiKey",
    // `/model` 保存的选择，属于当时所在的那一家。
    "model",
    // 备用模型链；模型 ID → 供应商专属 ID（如 Bedrock ARN）。
    "fallbackModel",
    "modelOverrides",
    // `/model` 选择器的行：聚合模式下是 CC Switch 列的 Stack 模型，其余时候不留。
    "modelPicker",
    // advisor 只在 Anthropic API 上可用。
    "advisorModel",
    // Bedrock / Vertex 的凭据命令。
    "awsAuthRefresh",
    "awsCredentialExport",
    "gcpAuthRefresh",
];

pub fn claude_floor_top(key: &str) -> bool {
    CLAUDE_FLOOR_TOP.contains(&key)
}

/// Claude Code 的供应商独有字段（`env` 里）。
pub const CLAUDE_EXCLUSIVE_ENV: &[&str] = &[
    // AtlasCloud、Soshow 等不接受实验性 beta 头。
    "CLAUDE_CODE_DISABLE_EXPERIMENTAL_BETAS",
    // DeepSeek 等严格校验工具 schema，带着 Artifact 工具每个请求都 400。
    "CLAUDE_CODE_DISABLE_ARTIFACT",
    // 第三方地址下默认关闭，只有上游转发 tool_reference 块时才能打开。
    "ENABLE_TOOL_SEARCH",
    // 官方文档写明给代理、网关、第三方用的兼容选项。
    "CLAUDE_CODE_DISABLE_THINKING",
    "DISABLE_INTERLEAVED_THINKING",
    "CLAUDE_CODE_ALWAYS_ENABLE_EFFORT",
    "CLAUDE_CODE_EXTRA_BODY",
    "CLAUDE_CODE_ENABLE_FINE_GRAINED_TOOL_STREAMING",
    // 窗口类：取值由上游模型的窗口决定。
    "CLAUDE_CODE_MAX_CONTEXT_TOKENS",
    "CLAUDE_CODE_AUTO_COMPACT_WINDOW",
    "CLAUDE_CODE_MAX_OUTPUT_TOKENS",
    "CLAUDE_CODE_DISABLE_1M_CONTEXT",
    "CLAUDE_CODE_DISABLE_UNKNOWN_MODEL_WINDOW_ENFORCEMENT",
    // 向 ANTHROPIC_BASE_URL 取模型列表：网关（含代理模式下的 Stack 模型）要它，用户也可能
    // 自己设成全局。
    "CLAUDE_CODE_ENABLE_GATEWAY_MODEL_DISCOVERY",
];

pub fn claude_exclusive_env(key: &str) -> bool {
    CLAUDE_EXCLUSIVE_ENV.contains(&key)
}

/// Codex `config.toml` 顶层的关键字段。另有 `[model_providers.custom]` 整表。
pub const CODEX_FLOOR_TOP: &[&str] = &[
    // 选路：`openai_base_url` 会把内置 openai 路由改道到别的地址。
    "model_provider",
    "openai_base_url",
    "model",
    "review_model",
    "model_reasoning_effort",
    "plan_mode_reasoning_effort",
    "disable_response_storage",
    "model_catalog_json",
    // 兜底写法会把 Key 写在顶层。
    "experimental_bearer_token",
    // 旧形态：没有 model_provider 时落在顶层。
    "base_url",
    "wire_api",
];

/// 嵌套在用户自己的表里的模型名：只清这几个键，表里的其他键不动。
pub const CODEX_FLOOR_NESTED: &[&[&str]] = &[
    &["agents", "default_subagent_model"],
    &["agents", "default_subagent_reasoning_effort"],
    &["memories", "extract_model"],
    &["memories", "consolidation_model"],
];

/// CC Switch 写进 Codex live 的供应商表。
pub const CODEX_PROVIDER_TABLE: &[&str] = &["model_providers", "custom"];

/// Codex 的供应商独有字段（顶层）。
///
/// `web_search` 的值不来自行文本，而由 `codex_native_gateway_rejects_web_search`
/// 按供应商判定（需要时为 `"disabled"`）。其余几个会绕过 CC Switch 生成的模型目录，
/// 覆盖按模型设好的能力。
pub const CODEX_EXCLUSIVE_TOP: &[&str] = &[
    "web_search",
    "model_context_window",
    "model_auto_compact_token_limit",
    "model_supports_reasoning_summaries",
    "model_verbosity",
];

/// Gemini CLI `.env` 里，这个键是否是关键字段。
///
/// `GOOGLE_*` 整个前缀都是连接和鉴权（Gemini CLI 0.61.0 读取的全部如此）。
/// `GEMINI_*` 不能按前缀：同一前缀下有 `GEMINI_CLI_HOME`、`GEMINI_SANDBOX`、
/// `GEMINI_SYSTEM_MD`、遥测开关等与供应商无关的设置。
pub fn gemini_floor_env(key: &str) -> bool {
    key.starts_with("GOOGLE_")
        || matches!(
            key,
            "GEMINI_API_KEY"
                | "GEMINI_MODEL"
                // Key 放 x-goog-api-key 头还是 Authorization: Bearer。
                | "GEMINI_API_KEY_AUTH_MECHANISM"
                | "GEMINI_CLI_CUSTOM_HEADERS"
                | "GEMINI_DEFAULT_AUTH_TYPE"
                | "GEMINI_CLI_USE_COMPUTE_ADC"
                // Google 登录走的服务地址。
                | "CODE_ASSIST_ENDPOINT"
                | "CODE_ASSIST_API_VERSION"
        )
}

/// Gemini CLI `settings.json` 里的关键字段：按键路径只清这两个键。
pub const GEMINI_FLOOR_SETTINGS: &[&[&str]] =
    &[&["security", "auth", "selectedType"], &["model", "name"]];

/// Claude Desktop 的 `configLibrary/<id>.json` 里的关键字段。
pub const DESKTOP_PROFILE_FLOOR: &[&str] = &[
    "inferenceProvider",
    "inferenceGatewayBaseUrl",
    "inferenceGatewayApiKey",
    "inferenceGatewayAuthScheme",
    "inferenceModels",
];

pub fn desktop_profile_floor(key: &str) -> bool {
    DESKTOP_PROFILE_FLOOR.contains(&key)
}

/// Claude Desktop profile 里的策略键：缺失时写入，存在时不动，用户可以自己收紧
/// 出站白名单、打开部署模式选择器。
pub const DESKTOP_PROFILE_SEED: &[&str] =
    &["disableDeploymentModeChooser", "coworkEgressAllowedHosts"];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_floor_covers_connection_keys() {
        for key in [
            "ANTHROPIC_BASE_URL",
            "ANTHROPIC_AUTH_TOKEN",
            "ANTHROPIC_DEFAULT_FABLE_MODEL",
            "ANTHROPIC_CUSTOM_HEADERS",
            "CLAUDE_CODE_USE_BEDROCK",
            "CLAUDE_CODE_SKIP_BEDROCK_AUTH",
            "AWS_REGION",
            "AWS_BEARER_TOKEN_BEDROCK",
            "VERTEX_REGION_CLAUDE_4_5_SONNET",
            "CLOUD_ML_REGION",
            "GOOGLE_APPLICATION_CREDENTIALS",
            "CLAUDE_CODE_OAUTH_TOKEN",
            "CLAUDE_CODE_SUBAGENT_MODEL",
        ] {
            assert!(claude_floor_env(key), "{key} should be a key field");
        }
        for key in ["apiKeyHelper", "apiKey", "model", "modelOverrides"] {
            assert!(claude_floor_top(key), "{key} should be a key field");
        }
    }

    #[test]
    fn claude_floor_leaves_feature_switches_alone() {
        for key in [
            "CLAUDE_CODE_USE_POWERSHELL_TOOL",
            "CLAUDE_CODE_USE_NATIVE_FILE_SEARCH",
            "CLAUDE_CODE_USE_COWORK_PLUGINS",
            "CLAUDE_CODE_USE_CCR_V2",
            "CLAUDE_CODE_SKIP_PROMPT_HISTORY",
            "API_TIMEOUT_MS",
            "DISABLE_TELEMETRY",
            // 独有字段不是关键字段。
            "CLAUDE_CODE_DISABLE_ARTIFACT",
            "CLAUDE_CODE_MAX_CONTEXT_TOKENS",
        ] {
            assert!(!claude_floor_env(key), "{key} must stay a user key");
        }
        for key in [
            "hooks",
            "enabledPlugins",
            "permissions",
            "statusLine",
            "env",
        ] {
            assert!(!claude_floor_top(key), "{key} must stay a user key");
        }
    }

    #[test]
    fn exclusive_fields_never_overlap_key_fields() {
        for key in CLAUDE_EXCLUSIVE_ENV {
            assert!(!claude_floor_env(key), "{key} is listed twice");
        }
        for key in CODEX_EXCLUSIVE_TOP {
            assert!(!CODEX_FLOOR_TOP.contains(key), "{key} is listed twice");
        }
    }

    #[test]
    fn gemini_floor_keeps_cli_settings() {
        for key in [
            "GOOGLE_API_KEY",
            "GOOGLE_GEMINI_BASE_URL",
            "GOOGLE_GENAI_USE_VERTEXAI",
            "GOOGLE_CLOUD_PROJECT",
            "GEMINI_API_KEY",
            "GEMINI_MODEL",
            "GEMINI_CLI_CUSTOM_HEADERS",
        ] {
            assert!(gemini_floor_env(key), "{key} should be a key field");
        }
        for key in [
            "GEMINI_SANDBOX",
            "GEMINI_CLI_HOME",
            "GEMINI_SYSTEM_MD",
            "DEBUG",
        ] {
            assert!(!gemini_floor_env(key), "{key} must stay a user key");
        }
    }

    /// 前端预设扫描用的镜像必须和这里的定义一字不差。
    #[test]
    fn frontend_mirror_matches_the_claude_definitions() {
        let mirror: serde_json::Value =
            serde_json::from_str(include_str!("../../../tests/config/claudeKeyFields.json"))
                .expect("mirror is valid JSON");
        let list = |key: &str| -> Vec<String> {
            mirror[key]
                .as_array()
                .unwrap_or_else(|| panic!("{key} is an array"))
                .iter()
                .map(|value| value.as_str().expect("string").to_string())
                .collect()
        };
        let owned =
            |items: &[&str]| -> Vec<String> { items.iter().map(|item| item.to_string()).collect() };
        assert_eq!(list("envPrefixes"), owned(CLAUDE_FLOOR_ENV_PREFIXES));
        assert_eq!(list("envKeys"), owned(CLAUDE_FLOOR_ENV_KEYS));
        assert_eq!(list("protocolSelectors"), owned(CLAUDE_PROTOCOL_SELECTORS));
        assert_eq!(list("top"), owned(CLAUDE_FLOOR_TOP));
        assert_eq!(list("exclusiveEnv"), owned(CLAUDE_EXCLUSIVE_ENV));
        assert_eq!(
            mirror["envSkipAuth"],
            serde_json::json!({ "prefix": "CLAUDE_CODE_SKIP_", "suffix": "_AUTH" })
        );
    }
}
