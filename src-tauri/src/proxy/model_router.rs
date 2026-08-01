//! 模型路由模块
//!
//! 在 UniversalProvider 列表中查找能处理指定模型的那个。
//! 利用 UniversalProvider 已有的 models 配置作为路由依据。

use crate::provider::UniversalProvider;
use std::collections::HashMap;

/// 匹配前剥离 [1m] / [1M] 上下文能力标记，使 `claude-sonnet-4-6[1m]`
/// 能正确匹配精确 route `claude-sonnet-4-6`，避免因后缀差异导致不同请求
/// 落在不同 route 上。
fn strip_one_m_context_marker(model: &str) -> &str {
    let bytes = model.as_bytes();
    let marker = b"[1m]";
    // 检查结尾（大小写不敏感）
    if bytes.len() >= marker.len()
        && bytes[bytes.len() - marker.len()..].eq_ignore_ascii_case(marker)
    {
        return model[..model.len() - marker.len()].trim_end();
    }
    model
}

/// 模型路由解析器
pub struct ModelRouter;

impl ModelRouter {
    /// 在 UniversalProvider 列表中匹配模型
    ///
    /// 遍历所有 UniversalProvider，检查其 models 配置（按 app_type）
    /// 是否能处理请求的 model。
    ///
    /// 匹配规则：
    /// 1. 只检查 app_type 对应的 models 配置
    /// 2. 精确匹配：请求 model == 配置值（大小写不敏感）
    /// 3. 通配符匹配：配置值以 `*` 结尾，且请求以该前缀开头
    /// 4. 多个匹配时，精确 > 通配符 > 更短的通配符
    /// 5. 同等匹配质量下，sort_index 较小的优先（未设置视为最大）
    /// 6. sort_index 相同或均未设置 → 随机（尊重 HashMap 的遍历顺序）
    /// 7. 未匹配 → None
    pub fn match_model<'a>(
        model: &str,
        providers: &'a HashMap<String, UniversalProvider>,
        app_type: &str,
    ) -> Option<&'a UniversalProvider> {
        if providers.is_empty() {
            return None;
        }

        let model_lower = model.to_lowercase();
        let mut best: Option<(&'a UniversalProvider, MatchKind)> = None;

        // 按 sort_index 升序遍历，使低 sort_index 的 provider 优先被考虑
        let mut sorted: Vec<&'a UniversalProvider> = providers.values().collect();
        sorted.sort_by_key(|p| p.sort_index.unwrap_or(usize::MAX));

        for provider in sorted {
            // 跳过有 routes 的 UP：routes 路由只由 find_matching_route 驱动
            // （且仅在它作为当前 provider 时）。若这里按 models 匹配到它，
            // to_*_provider 的 ANTHROPIC_BASE_URL 会指向本地代理自身
            // （cc_switch preset 的 baseUrl=127.0.0.1:15721），造成请求自环。
            if !provider.routes.is_empty() {
                continue;
            }

            let kind = match app_type {
                "claude" => Self::match_claude(&model_lower, provider),
                "codex" => Self::match_codex(&model_lower, provider),
                "gemini" => Self::match_gemini(&model_lower, provider),
                _ => None,
            };

            if let Some(k) = kind {
                let is_better = match best {
                    None => true,
                    Some((best_p, best_kind)) => {
                        if k == best_kind {
                            // 同等匹配质量 → sort_index 较小的优先
                            provider.sort_index.unwrap_or(usize::MAX)
                                < best_p.sort_index.unwrap_or(usize::MAX)
                        } else {
                            k.better_than(best_kind)
                        }
                    }
                };
                if is_better {
                    best = Some((provider, k));
                }
            }
        }

        best.map(|(p, _)| p)
    }

    /// 匹配 Claude 模型配置
    ///
    /// 检查 ClaudeModelConfig 的以下字段（按优先级）：
    /// - model（通用模型）
    /// - sonnetModel
    /// - opusModel
    /// - haikuModel
    fn match_claude(model: &str, provider: &UniversalProvider) -> Option<MatchKind> {
        let config = provider.models.claude.as_ref()?;
        let fields = [
            ("main", config.model.as_deref()),
            ("sonnet", config.sonnet_model.as_deref()),
            ("opus", config.opus_model.as_deref()),
            ("haiku", config.haiku_model.as_deref()),
        ];
        Self::check_fields(model, &fields)
    }

    /// 匹配 Codex 模型配置
    fn match_codex(model: &str, provider: &UniversalProvider) -> Option<MatchKind> {
        let config = provider.models.codex.as_ref()?;
        let fields = [("main", config.model.as_deref())];
        Self::check_fields(model, &fields)
    }

    /// 匹配 Gemini 模型配置
    fn match_gemini(model: &str, provider: &UniversalProvider) -> Option<MatchKind> {
        let config = provider.models.gemini.as_ref()?;
        let fields = [("main", config.model.as_deref())];
        Self::check_fields(model, &fields)
    }

    /// 统一字段检查逻辑
    fn check_fields(model: &str, fields: &[(&str, Option<&str>)]) -> Option<MatchKind> {
        let mut best: Option<MatchKind> = None;

        for &(_field_name, value) in fields {
            let value = match value {
                Some(v) => v,
                None => continue,
            };
            if let Some(kind) = Self::match_value(model, value) {
                let is_better = match best {
                    None => true,
                    Some(best_kind) => kind.better_than(best_kind),
                };
                if is_better {
                    best = Some(kind);
                }
            }
        }

        best
    }

    /// 单值匹配
    ///
    /// 匹配前剥离 `[1m]` 后缀，使 Claude Code 带 1M 标记的模型名
    /// 仍能命中精确 route 或 model。
    fn match_value(model: &str, pattern: &str) -> Option<MatchKind> {
        let model = strip_one_m_context_marker(model);
        let pattern_lower = pattern.to_lowercase();
        let model_lower = model.to_lowercase();

        // 精确匹配
        if model_lower == pattern_lower {
            return Some(MatchKind::Exact);
        }

        // 通配符匹配：pattern 以 * 结尾，且 model 以前缀开头
        if let Some(prefix) = pattern_lower.strip_suffix('*') {
            if model_lower.starts_with(prefix) {
                return Some(MatchKind::Wildcard(prefix.len()));
            }
        }

        None
    }

    /// 检查 UpstreamRoute 是否能处理指定 model
    ///
    /// 匹配前剥离 `[1m]` 后缀（同 match_value）。
    pub fn match_route(model: &str, route: &crate::provider::UpstreamRoute) -> bool {
        let model = strip_one_m_context_marker(model);
        let model_lower = model.to_lowercase();
        route.model_names.iter().any(|name| {
            let name_lower = name.to_lowercase();
            // 精确匹配
            if model_lower == name_lower {
                return true;
            }
            // 通配符匹配：name 以 * 结尾
            if let Some(prefix) = name_lower.strip_suffix('*') {
                if model_lower.starts_with(prefix) {
                    return true;
                }
            }
            false
        })
    }
}

/// 匹配类型（用于优先级排序）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MatchKind {
    /// 精确匹配（优先级最高）
    Exact,
    /// 通配符匹配（带前缀长度，越长越优先）
    Wildcard(usize),
}

impl MatchKind {
    fn better_than(&self, other: MatchKind) -> bool {
        match (self, other) {
            (MatchKind::Exact, _) => true,
            (_, MatchKind::Exact) => false,
            (MatchKind::Wildcard(a), MatchKind::Wildcard(b)) => *a > b,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{
        ClaudeModelConfig, CodexModelConfig, GeminiModelConfig, UniversalProvider,
        UniversalProviderModels,
    };

    fn make_up(id: &str, models: UniversalProviderModels) -> UniversalProvider {
        UniversalProvider {
            id: id.to_string(),
            name: id.to_string(),
            provider_type: "custom".to_string(),
            apps: Default::default(),
            base_url: format!("https://{}.com", id),
            api_key: "sk-test".to_string(),
            models,
            website_url: None,
            notes: None,
            icon: None,
            icon_color: None,
            meta: None,
            created_at: None,
            sort_index: None,
            enabled: true,
            routes: vec![],
        }
    }

    fn claude_models(
        model: Option<&str>,
        sonnet: Option<&str>,
        opus: Option<&str>,
        haiku: Option<&str>,
    ) -> UniversalProviderModels {
        UniversalProviderModels {
            claude: Some(ClaudeModelConfig {
                model: model.map(String::from),
                haiku_model: haiku.map(String::from),
                sonnet_model: sonnet.map(String::from),
                opus_model: opus.map(String::from),
            }),
            codex: None,
            gemini: None,
        }
    }

    #[test]
    fn match_claude_sonnet_model() {
        let up = make_up(
            "up-a",
            claude_models(None, Some("claude-sonnet-4-6"), None, None),
        );
        let mut map = HashMap::new();
        map.insert(up.id.clone(), up);

        let result = ModelRouter::match_model("claude-sonnet-4-6", &map, "claude");
        assert!(result.is_some());
        assert_eq!(result.unwrap().id, "up-a");
    }

    #[test]
    fn match_codex_model_exact() {
        let up = make_up(
            "up-b",
            UniversalProviderModels {
                codex: Some(CodexModelConfig {
                    model: Some("gpt-5".to_string()),
                    reasoning_effort: None,
                }),
                ..Default::default()
            },
        );
        let mut map = HashMap::new();
        map.insert(up.id.clone(), up);

        let result = ModelRouter::match_model("gpt-5", &map, "codex");
        assert!(result.is_some());
    }

    #[test]
    fn match_wildcard() {
        let up = make_up(
            "up-c",
            UniversalProviderModels {
                gemini: Some(GeminiModelConfig {
                    model: Some("gemini-2*".to_string()),
                }),
                ..Default::default()
            },
        );
        let mut map = HashMap::new();
        map.insert(up.id.clone(), up);

        let result = ModelRouter::match_model("gemini-2.0-flash", &map, "gemini");
        assert!(result.is_some());

        let result = ModelRouter::match_model("gemini-2.5-pro", &map, "gemini");
        assert!(result.is_some());
    }

    #[test]
    fn case_insensitive() {
        let up = make_up(
            "up-d",
            claude_models(None, Some("Claude-Sonnet-4-6"), None, None),
        );
        let mut map = HashMap::new();
        map.insert(up.id.clone(), up);

        let result = ModelRouter::match_model("claude-sonnet-4-6", &map, "claude");
        assert!(result.is_some());
    }

    #[test]
    fn no_match_returns_none() {
        let up = make_up(
            "up-e",
            claude_models(Some("claude-opus-4-8"), None, None, None),
        );
        let mut map = HashMap::new();
        map.insert(up.id.clone(), up);

        let result = ModelRouter::match_model("claude-sonnet-4-6", &map, "claude");
        assert!(result.is_none());
    }

    #[test]
    fn wrong_app_type_skipped() {
        let up = make_up(
            "up-f",
            claude_models(Some("claude-sonnet-4-6"), None, None, None),
        );
        let mut map = HashMap::new();
        map.insert(up.id.clone(), up);

        // Claude model config 不应该匹配 codex endpoint
        let result = ModelRouter::match_model("claude-sonnet-4-6", &map, "codex");
        assert!(result.is_none());
    }

    #[test]
    fn empty_universal_providers() {
        let map: HashMap<String, UniversalProvider> = HashMap::new();
        let result = ModelRouter::match_model("any-model", &map, "claude");
        assert!(result.is_none());
    }

    #[test]
    fn claude_sonnet_preferred_over_main() {
        // 同时配了 main model 和 sonnet model，请求 sonnet 应该优先匹配 sonnet
        let up = make_up(
            "up-g",
            claude_models(
                Some("claude-opus-4-8"),
                Some("claude-sonnet-4-6"),
                None,
                None,
            ),
        );
        let mut map = HashMap::new();
        map.insert(up.id.clone(), up);

        let result = ModelRouter::match_model("claude-sonnet-4-6", &map, "claude");
        assert!(result.is_some());
        assert_eq!(result.unwrap().id, "up-g");
    }

    #[test]
    fn wildcard_longest_prefix_wins() {
        let up1 = make_up(
            "short",
            UniversalProviderModels {
                gemini: Some(GeminiModelConfig {
                    model: Some("gemini-2*".to_string()),
                }),
                ..Default::default()
            },
        );
        let up2 = make_up(
            "long",
            UniversalProviderModels {
                gemini: Some(GeminiModelConfig {
                    model: Some("gemini-2.0*".to_string()),
                }),
                ..Default::default()
            },
        );
        let mut map = HashMap::new();
        map.insert(up1.id.clone(), up1);
        map.insert(up2.id.clone(), up2);

        let result = ModelRouter::match_model("gemini-2.0-flash", &map, "gemini");
        assert!(result.is_some());
        // 应该匹配最长前缀的
        assert_eq!(result.unwrap().id, "long");
    }

    #[test]
    fn exact_over_wildcard() {
        let up1 = make_up(
            "wildcard",
            UniversalProviderModels {
                gemini: Some(GeminiModelConfig {
                    model: Some("gemini-2*".to_string()),
                }),
                ..Default::default()
            },
        );
        let up2 = make_up(
            "exact",
            UniversalProviderModels {
                gemini: Some(GeminiModelConfig {
                    model: Some("gemini-2.0-flash".to_string()),
                }),
                ..Default::default()
            },
        );
        let mut map = HashMap::new();
        map.insert(up1.id.clone(), up1);
        map.insert(up2.id.clone(), up2);

        let result = ModelRouter::match_model("gemini-2.0-flash", &map, "gemini");
        assert!(result.is_some());
        // 精确匹配优先于通配符
        assert_eq!(result.unwrap().id, "exact");
    }

    #[test]
    fn match_route_strips_one_m_marker() {
        let route = crate::provider::UpstreamRoute {
            id: "test".to_string(),
            name: "test".to_string(),
            model_names: vec!["claude-sonnet-4-6".to_string()],
            enabled: true,
            base_url: "https://test.com".to_string(),
            api_key: "sk-test".to_string(),
            protocol: "openai_chat".to_string(),
            priority: 0,
        };
        assert!(ModelRouter::match_route("claude-sonnet-4-6[1m]", &route));
        assert!(ModelRouter::match_route("claude-sonnet-4-6[1M]", &route));
        assert!(ModelRouter::match_route("claude-sonnet-4-6", &route));
    }

    #[test]
    fn match_model_strips_one_m_marker_exact() {
        let up = make_up(
            "up-a",
            claude_models(None, Some("claude-sonnet-4-6"), None, None),
        );
        let mut map = HashMap::new();
        map.insert(up.id.clone(), up);

        // 带 [1m] 后缀也应该匹配精确 model
        let result = ModelRouter::match_model("claude-sonnet-4-6[1m]", &map, "claude");
        assert!(result.is_some());
        assert_eq!(result.unwrap().id, "up-a");
    }

    #[test]
    fn match_model_strips_one_m_marker_wildcard() {
        let up = make_up(
            "up-b",
            UniversalProviderModels {
                gemini: Some(GeminiModelConfig {
                    model: Some("gemini-2*".to_string()),
                }),
                ..Default::default()
            },
        );
        let mut map = HashMap::new();
        map.insert(up.id.clone(), up);

        let result = ModelRouter::match_model("gemini-2.0-flash[1m]", &map, "gemini");
        assert!(result.is_some());
    }

    #[test]
    fn match_model_skips_up_with_routes() {
        // routes 模式只由 find_matching_route 驱动（且仅在它作为当前 provider 时）。
        // 若这里按 models 匹配到有 routes 的 UP，to_*_provider 会把请求转发回本地
        // 代理自身造成自环，因此 match_model 必须跳过有 routes 的 UP。
        let up = make_up(
            "up-routes",
            claude_models(Some("claude-sonnet-4-6"), None, None, None),
        );
        let mut up_with_routes = up;
        up_with_routes.routes = vec![crate::provider::UpstreamRoute {
            id: "r1".to_string(),
            name: "r1".to_string(),
            protocol: "openai_chat".to_string(),
            base_url: "https://route.example.com".to_string(),
            api_key: "sk-route".to_string(),
            model_names: vec!["claude-sonnet-4-6".to_string()],
            enabled: true,
            priority: 0,
        }];
        let mut map = HashMap::new();
        map.insert(up_with_routes.id.clone(), up_with_routes);

        // 有 routes 的 UP 不应被 models 匹配命中
        let result = ModelRouter::match_model("claude-sonnet-4-6", &map, "claude");
        assert!(result.is_none());
    }

    #[test]
    fn match_model_skips_up_with_routes_even_if_no_model_match() {
        // 确保跳过逻辑不是"有 routes 就不看 models"，而是彻底不参与 models 匹配
        let up = make_up(
            "up-routes",
            claude_models(Some("claude-sonnet-4-6"), None, None, None),
        );
        let mut up_with_routes = up;
        up_with_routes.routes = vec![crate::provider::UpstreamRoute {
            id: "r1".to_string(),
            name: "r1".to_string(),
            protocol: "openai_chat".to_string(),
            base_url: "https://route.example.com".to_string(),
            api_key: "sk-route".to_string(),
            model_names: vec!["claude-sonnet-4-6".to_string()],
            enabled: true,
            priority: 0,
        }];
        let mut map = HashMap::new();
        map.insert(up_with_routes.id.clone(), up_with_routes);

        let result = ModelRouter::match_model("other-model", &map, "claude");
        assert!(result.is_none());
    }
}
