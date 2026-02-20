//! URL 构建工具模块
//!
//! 提供统一的 URL 构建逻辑，供前端预览和后端代理使用。

use crate::app_config::AppType;
use crate::proxy::providers::{ClaudeAdapter, CodexAdapter, ProviderAdapter};
use crate::proxy::url_utils::{dedup_v1_v1_boundary_safe, split_url_suffix};
use serde::{Deserialize, Serialize};

/// URL 预览结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UrlPreview {
    /// 直连模式请求地址
    pub direct_url: String,
    /// 代理模式请求地址
    pub proxy_url: String,
    /// 是否为全链接（base_url 已包含 API 路径）
    pub is_full_url: bool,
}

/// API 路径模式
struct ApiPathPatterns {
    /// 直连模式默认端点
    direct_endpoint: &'static str,
    /// 代理模式端点（根据 api_format 可能不同）
    proxy_endpoint: &'static str,
    /// 识别为全链接的路径后缀
    full_url_patterns: &'static [&'static str],
}

impl ApiPathPatterns {
    fn for_claude(api_format: Option<&str>) -> Self {
        // 根据 API 格式决定端点和全链接检测模式
        if api_format == Some("openai_chat") {
            Self {
                direct_endpoint: "/v1/messages",
                proxy_endpoint: "/v1/chat/completions",
                // 与运行时 ClaudeAdapter 保持一致：同时识别 messages/chat 两类完整路径
                full_url_patterns: &[
                    "/v1/messages",
                    "/messages",
                    "/v1/chat/completions",
                    "/chat/completions",
                ],
            }
        } else {
            Self {
                direct_endpoint: "/v1/messages",
                proxy_endpoint: "/v1/messages",
                // 与运行时 ClaudeAdapter 保持一致：同时识别 messages/chat 两类完整路径
                full_url_patterns: &[
                    "/v1/messages",
                    "/messages",
                    "/v1/chat/completions",
                    "/chat/completions",
                ],
            }
        }
    }

    fn for_codex(api_format: Option<&str>) -> Self {
        let _ = api_format;
        Self {
            direct_endpoint: "/responses",
            proxy_endpoint: "/responses",
            full_url_patterns: &["/v1/responses", "/responses"],
        }
    }

    fn for_gemini() -> Self {
        Self {
            direct_endpoint: "/v1beta/models",
            proxy_endpoint: "/v1beta/models",
            full_url_patterns: &["/v1beta/models"],
        }
    }
}

/// 检测 URL 是否以指定的 API 路径结尾
fn url_ends_with_api_path(url: &str, patterns: &[&str]) -> bool {
    let (base, _) = split_url_suffix(url);
    let path_part = base.trim_end_matches('/').to_lowercase();
    patterns
        .iter()
        .any(|pattern| path_part.ends_with(&pattern.to_lowercase()))
}

/// 硬拼接 URL（用于直连地址）
///
/// 始终将 endpoint 拼接到 base_url 后面，不做任何智能检测或去重。
fn build_direct_url(base_url: &str, endpoint: &str) -> String {
    let (base, suffix) = split_url_suffix(base_url);
    let base_trimmed = base.trim_end_matches('/');
    let endpoint_trimmed = endpoint.trim_start_matches('/');

    // 直接拼接，不做任何去重
    format!("{base_trimmed}/{endpoint_trimmed}{suffix}")
}

/// 智能构建 URL（用于代理地址）
///
/// 如果 base_url 已经以 API 路径结尾，直接返回；否则追加 endpoint。
pub fn build_smart_url(base_url: &str, endpoint: &str, full_url_patterns: &[&str]) -> String {
    let (base, suffix) = split_url_suffix(base_url);
    let base_trimmed = base.trim_end_matches('/');
    let endpoint_trimmed = endpoint.trim_start_matches('/');

    // 检测 base_url 是否已经以 API 路径结尾
    if url_ends_with_api_path(base_trimmed, full_url_patterns) {
        return format!("{base_trimmed}{suffix}");
    }

    // 拼接 URL
    let url = format!("{base_trimmed}/{endpoint_trimmed}");
    let url = dedup_v1_v1_boundary_safe(url);
    format!("{url}{suffix}")
}

fn build_runtime_like_url(
    app_type: &AppType,
    base_url: &str,
    endpoint: &str,
    is_proxy: bool,
) -> String {
    match app_type {
        // Claude 代理预览需要展示与运行时一致的 URL 归一化结果
        AppType::Claude if is_proxy => ClaudeAdapter::new().build_url(base_url, endpoint),
        // Codex/OpenCode 预览复用运行时 /v1 归一化规则
        AppType::Codex | AppType::OpenCode | AppType::OpenClaw => {
            CodexAdapter::new().build_url(base_url, endpoint)
        }
        _ => build_direct_url(base_url, endpoint),
    }
}

/// 构建 URL 预览
///
/// 根据 app_type、base_url 和 api_format 计算直连和代理模式的请求地址。
/// - 直连地址：始终硬拼接默认后缀
/// - 代理地址：智能检测，如果已包含 API 路径则不重复拼接
pub fn build_url_preview(
    app_type: &AppType,
    base_url: &str,
    api_format: Option<&str>,
) -> UrlPreview {
    let patterns = match app_type {
        AppType::Claude => ApiPathPatterns::for_claude(api_format),
        AppType::Codex => ApiPathPatterns::for_codex(api_format),
        AppType::Gemini => ApiPathPatterns::for_gemini(),
        AppType::OpenCode => ApiPathPatterns::for_codex(api_format), // OpenCode 使用 Codex 逻辑
        AppType::OpenClaw => ApiPathPatterns::for_codex(api_format), // OpenClaw 使用 Codex 逻辑
    };

    let is_full_url = url_ends_with_api_path(base_url, patterns.full_url_patterns);

    // 直连地址：默认硬拼接；Codex/OpenCode 复用运行时规则（含 origin-only /v1 归一化）
    let direct_url = build_runtime_like_url(app_type, base_url, patterns.direct_endpoint, false);
    // 代理地址：Claude/Codex/OpenCode 复用运行时规则；Gemini 继续使用通用智能拼接
    let proxy_url = match app_type {
        AppType::Claude | AppType::Codex | AppType::OpenCode | AppType::OpenClaw => {
            build_runtime_like_url(app_type, base_url, patterns.proxy_endpoint, true)
        }
        _ => build_smart_url(
            base_url,
            patterns.proxy_endpoint,
            patterns.full_url_patterns,
        ),
    };

    UrlPreview {
        direct_url,
        proxy_url,
        is_full_url,
    }
}

/// 检查是否需要代理
///
/// 返回需要代理的原因，None 表示不需要代理
pub fn check_proxy_requirement(
    app_type: &AppType,
    base_url: &str,
    api_format: Option<&str>,
) -> Option<&'static str> {
    // Claude OpenAI Chat 格式必须开启代理（需要格式转换）
    if matches!(app_type, AppType::Claude) && api_format == Some("openai_chat") {
        return Some("openai_chat_format");
    }

    // base_url 缺失时无法做 full_url / url_mismatch 判断，避免误判
    if base_url.trim().is_empty() {
        return None;
    }

    let preview = build_url_preview(app_type, base_url, api_format);

    // 如果是全链接且以直连后缀结尾，需要代理
    if preview.is_full_url {
        // 检查是否以直连后缀结尾
        let direct_suffixes: &[&str] = match app_type {
            AppType::Claude => &[
                "/v1/messages",
                "/messages",
                "/v1/chat/completions",
                "/chat/completions",
            ],
            AppType::Codex | AppType::OpenCode | AppType::OpenClaw => &["/v1/responses", "/responses"],
            _ => return None,
        };

        if url_ends_with_api_path(base_url, direct_suffixes) {
            return Some("full_url");
        }
    }

    // 如果直连地址和代理地址路径不同，需要代理（忽略查询参数差异）
    let (direct_base, _) = split_url_suffix(&preview.direct_url);
    let (proxy_base, _) = split_url_suffix(&preview.proxy_url);
    if direct_base != proxy_base {
        return Some("url_mismatch");
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_url_preview_claude_anthropic() {
        let preview = build_url_preview(
            &AppType::Claude,
            "https://api.example.com",
            Some("anthropic"),
        );
        assert_eq!(preview.direct_url, "https://api.example.com/v1/messages");
        assert_eq!(preview.proxy_url, "https://api.example.com/v1/messages");
        assert!(!preview.is_full_url);
    }

    #[test]
    fn test_build_url_preview_claude_openai_chat() {
        let preview = build_url_preview(
            &AppType::Claude,
            "https://api.example.com",
            Some("openai_chat"),
        );
        assert_eq!(preview.direct_url, "https://api.example.com/v1/messages");
        assert_eq!(
            preview.proxy_url,
            "https://api.example.com/v1/chat/completions"
        );
        assert!(!preview.is_full_url);
    }

    #[test]
    fn test_build_url_preview_claude_full_url() {
        // 全链接时：直连会硬拼接后缀，代理保持原地址
        let preview = build_url_preview(
            &AppType::Claude,
            "https://api.example.com/v1/messages",
            Some("anthropic"),
        );
        assert_eq!(
            preview.direct_url,
            "https://api.example.com/v1/messages/v1/messages"
        );
        assert_eq!(preview.proxy_url, "https://api.example.com/v1/messages");
        assert!(preview.is_full_url);
    }

    #[test]
    fn test_build_url_preview_codex_responses() {
        let preview = build_url_preview(
            &AppType::Codex,
            "https://api.openai.com/v1",
            Some("responses"),
        );
        assert_eq!(preview.direct_url, "https://api.openai.com/v1/responses");
        assert_eq!(preview.proxy_url, "https://api.openai.com/v1/responses");
        assert!(!preview.is_full_url);
    }

    #[test]
    fn test_build_url_preview_codex_origin_normalizes_v1() {
        let preview =
            build_url_preview(&AppType::Codex, "https://api.openai.com", Some("responses"));
        assert_eq!(preview.direct_url, "https://api.openai.com/v1/responses");
        assert_eq!(preview.proxy_url, "https://api.openai.com/v1/responses");
        assert!(!preview.is_full_url);
    }

    #[test]
    fn test_build_url_preview_codex_full_url() {
        // 全链接时：直连/代理均保持原地址（运行时适配器规则）
        let preview = build_url_preview(
            &AppType::Codex,
            "https://api.example.com/v1/responses",
            Some("responses"),
        );
        assert_eq!(preview.direct_url, "https://api.example.com/v1/responses");
        assert_eq!(preview.proxy_url, "https://api.example.com/v1/responses");
        assert!(preview.is_full_url);
    }

    #[test]
    fn test_check_proxy_requirement_claude_openai_chat() {
        let result = check_proxy_requirement(
            &AppType::Claude,
            "https://api.example.com",
            Some("openai_chat"),
        );
        assert_eq!(result, Some("openai_chat_format"));
    }

    #[test]
    fn test_check_proxy_requirement_claude_full_url() {
        let result = check_proxy_requirement(
            &AppType::Claude,
            "https://api.example.com/v1/messages",
            Some("anthropic"),
        );
        assert_eq!(result, Some("full_url"));
    }

    #[test]
    fn test_check_proxy_requirement_codex_full_url() {
        let result = check_proxy_requirement(
            &AppType::Codex,
            "https://api.example.com/v1/responses",
            Some("responses"),
        );
        assert_eq!(result, Some("full_url"));
    }

    #[test]
    fn test_check_proxy_requirement_codex_origin_none() {
        let result =
            check_proxy_requirement(&AppType::Codex, "https://api.openai.com", Some("responses"));
        assert_eq!(result, None);
    }

    #[test]
    fn test_check_proxy_requirement_none() {
        let result = check_proxy_requirement(
            &AppType::Claude,
            "https://api.example.com",
            Some("anthropic"),
        );
        assert_eq!(result, None);
    }

    #[test]
    fn test_v1_dedup() {
        // 代理地址使用 build_smart_url，会去重 /v1/v1
        let url = build_smart_url("https://api.example.com/v1", "/v1/messages", &[]);
        assert_eq!(url, "https://api.example.com/v1/messages");
    }

    #[test]
    fn test_direct_url_no_dedup() {
        // 直连地址硬拼接，不做任何去重
        let preview = build_url_preview(
            &AppType::Claude,
            "https://api.example.com/v1",
            Some("anthropic"),
        );
        assert_eq!(preview.direct_url, "https://api.example.com/v1/v1/messages");
        // 代理地址按运行时规则构建，会进行路径去重
        assert_eq!(preview.proxy_url, "https://api.example.com/v1/messages");
    }

    #[test]
    fn test_query_suffix_preserved_in_preview() {
        let preview = build_url_preview(
            &AppType::Claude,
            "https://api.example.com?beta=true",
            Some("anthropic"),
        );
        assert_eq!(
            preview.direct_url,
            "https://api.example.com/v1/messages?beta=true"
        );
        assert_eq!(
            preview.proxy_url,
            "https://api.example.com/v1/messages?beta=true"
        );
        assert!(!preview.is_full_url);
    }

    #[test]
    fn test_fragment_suffix_preserved_and_full_url_detected() {
        let preview = build_url_preview(
            &AppType::Claude,
            "https://api.example.com/v1/messages#frag",
            Some("anthropic"),
        );
        assert_eq!(
            preview.proxy_url,
            "https://api.example.com/v1/messages#frag"
        );
        assert!(preview.is_full_url);
    }

    #[test]
    fn test_claude_full_url_detection_is_api_format_agnostic() {
        // 与运行时 ClaudeAdapter 一致：/messages 与 /chat/completions 都视为全链接
        let preview = build_url_preview(
            &AppType::Claude,
            "https://api.example.com/v1/messages",
            Some("anthropic"),
        );
        assert!(preview.is_full_url);

        let preview = build_url_preview(
            &AppType::Claude,
            "https://api.example.com/v1/chat/completions",
            Some("anthropic"),
        );
        assert!(preview.is_full_url);

        let preview = build_url_preview(
            &AppType::Claude,
            "https://api.example.com/v1/chat/completions",
            Some("openai_chat"),
        );
        assert!(preview.is_full_url);

        let preview = build_url_preview(
            &AppType::Claude,
            "https://api.example.com/v1/messages",
            Some("openai_chat"),
        );
        assert!(preview.is_full_url);
    }
}
