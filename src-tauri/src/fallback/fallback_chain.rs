//! 回退链路定义与解析
//!
//! 实现 oh-my-pi 的 fallback chain 配置格式：
//! - 精确模型Key → 最长匹配通配符 → 角色Key → "default"
//! - 通配符：`provider/*`（保留 model_id）和 `provider/prefix/*`（重加前缀）

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// 回退链路中的一个 Selector。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct FallbackSelector {
    /// 原始选择器字符串，如 "anthropic/claude-sonnet-4"
    pub raw: String,
    /// Provider 标识
    pub provider: String,
    /// Model ID
    pub id: String,
}

/// 回退链路配置：Key → 有序的 Selector 列表。
///
/// Key 格式示例：
/// - `"default"` — 兜底链路
/// - `"anthropic/claude-sonnet-4"` — 精确模型Key
/// - `"google/*"` — Provider 通配符
/// - `"openrouter/google/*"` — Provider/Prefix 通配符
pub type FallbackChains = HashMap<String, Vec<FallbackSelector>>;

/// 回退链路恢复策略
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FallbackRevertPolicy {
    /// 冷却过期后自动切回主模型
    CooldownExpiry,
    /// 永不自动切回
    Never,
}

impl Default for FallbackRevertPolicy {
    fn default() -> Self {
        Self::CooldownExpiry
    }
}

/// 回退配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FallbackConfig {
    /// 是否启用回退链路系统
    pub enabled: bool,
    /// 回退链路定义
    pub chains: FallbackChains,
    /// 主模型恢复策略
    pub revert_policy: FallbackRevertPolicy,
}

impl Default for FallbackConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            chains: FallbackChains::new(),
            revert_policy: FallbackRevertPolicy::default(),
        }
    }
}

/// 链路Key匹配优先级
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ChainKeyMatchKind {
    None = 0,
    Default = 1,
    Role = 2,
    Wildcard = 3,
    Exact = 4,
}

/// 回退链路解析器。
///
/// 负责根据当前模型和角色解析出对应的回退链路Key，
/// 以及通配符 Selector 的展开。
pub struct FallbackChainResolver {
    chains: FallbackChains,
}

impl FallbackChainResolver {
    pub fn new(chains: FallbackChains) -> Self {
        Self { chains }
    }

    /// 解析适用于当前请求的回退链路Key。
    ///
    /// 优先级：
    /// 1. 精确匹配 `model_selector`（如 "anthropic/claude-sonnet-4"）
    /// 2. 最长匹配通配符（如 "google/*"）
    /// 3. 角色匹配（`role`）
    /// 4. "default" 键
    pub fn resolve_chain_key<'a>(
        &'a self,
        model_selector: &'a str,
        role: Option<&'a str>,
    ) -> Option<&'a str> {
        // 1. 精确匹配
        if self.chains.contains_key(model_selector) {
            return Some(model_selector);
        }

        // 2. 通配符匹配（最长前缀优先）
        let best_wildcard = self.find_best_wildcard_key(model_selector);
        if let Some(key) = best_wildcard {
            return Some(key);
        }

        // 3. 角色匹配
        if let Some(role) = role {
            if self.chains.contains_key(role) {
                return Some(role);
            }
        }

        // 4. default
        if self.chains.contains_key("default") {
            return Some("default");
        }

        None
    }

    /// 获取指定Key对应的回退链路。
    pub fn get_chain(&self, key: &str) -> Option<&Vec<FallbackSelector>> {
        self.chains.get(key)
    }

    /// 解析通配符 Selector。
    ///
    /// - `provider/*` → 保留 `current_model_id`，切换到 `provider`
    /// - `provider/prefix/*` → `new_id = prefix + "/" + bare_model_id`，切换到 `provider`
    /// - 普通 Selector → 直接返回
    pub fn resolve_wildcard_selector(
        &self,
        selector: &FallbackSelector,
        current_model_id: &str,
    ) -> FallbackSelector {
        let raw = &selector.raw;
        match classify_selector_wildcard(raw) {
            SelectorWildcardKind::None => selector.clone(),
            SelectorWildcardKind::ProviderWildcard { provider } => {
                FallbackSelector {
                    raw: raw.clone(),
                    provider: provider.to_string(),
                    id: current_model_id.to_string(),
                }
            }
            SelectorWildcardKind::PrefixWildcard { provider, prefix } => {
                let bare_id = current_model_id.rsplit('/').next().unwrap_or(current_model_id);
                let new_id = format!("{}/{}", prefix, bare_id);
                FallbackSelector {
                    raw: raw.clone(),
                    provider: provider.to_string(),
                    id: new_id,
                }
            }
        }
    }

    /// 构建包含主模型和回退链路的完整候选列表。
    ///
    /// 返回 `(primary, chain_candidates)` 其中 primary 是当前应使用的第一个候选项。
    pub fn build_candidate_chain(
        &self,
        chain_key: &str,
        current_selector: &FallbackSelector,
        current_model_id: &str,
    ) -> Vec<FallbackSelector> {
        let chain = self.get_chain(chain_key);
        let mut candidates = Vec::new();

        // 主模型
        candidates.push(current_selector.clone());

        // 链路中的候选项
        if let Some(chain) = chain {
            for selector in chain {
                // 跳过主模型自身
                if selector.provider == current_selector.provider
                    && selector.id == current_selector.id
                {
                    continue;
                }

                // 解析通配符
                let resolved = self.resolve_wildcard_selector(selector, current_model_id);
                candidates.push(resolved);
            }
        }

        candidates
    }

    /// 找到当前 chain key 之后的所有候选项。
    pub fn find_candidates_after_current(
        &self,
        chain_key: &str,
        current_provider: &str,
        current_model_id: &str,
    ) -> Vec<FallbackSelector> {
        let chain = match self.get_chain(chain_key) {
            Some(c) => c,
            None => return Vec::new(),
        };

        let mut found_current = false;
        let mut candidates = Vec::new();

        for selector in chain {
            let resolved = self.resolve_wildcard_selector(selector, current_model_id);

            if !found_current {
                if resolved.provider == current_provider {
                    found_current = true;
                }
                continue;
            }

            candidates.push(resolved);
        }

        candidates
    }

    /// 找到最佳通配符Key。
    fn find_best_wildcard_key(&self, model_selector: &str) -> Option<&str> {
        let mut best: Option<(&str, usize)> = None;

        for key in self.chains.keys() {
            let wildcard = classify_chain_key_wildcard(key);
            match wildcard {
                ChainKeyWildcard::Exact => continue,

                ChainKeyWildcard::ProviderWildcard { provider } => {
                    // 检查 model_selector 是否以 "provider/" 开头
                    if let Some(slash_pos) = model_selector.find('/') {
                        let selector_provider = &model_selector[..slash_pos];
                        if selector_provider == provider {
                            let match_len = provider.len();
                            if best.is_none() || match_len > best.unwrap().1 {
                                best = Some((key.as_str(), match_len));
                            }
                        }
                    }
                }

                ChainKeyWildcard::PrefixWildcard { provider, prefix } => {
                    // 检查 model_selector 是否以 "provider/" 开头
                    if let Some(slash_pos) = model_selector.find('/') {
                        let selector_provider = &model_selector[..slash_pos];
                        if selector_provider == provider {
                            let full_prefix = format!("{}/{}", provider, prefix);
                            let match_len = full_prefix.len();
                            if best.is_none() || match_len > best.unwrap().1 {
                                best = Some((key.as_str(), match_len));
                            }
                        }
                    }
                }
            }
        }

        best.map(|(key, _)| key)
    }
}

// ---- 通配符解析 ----

/// 链路Key的通配符类别
#[derive(Debug, Clone)]
enum ChainKeyWildcard {
    Exact,
    ProviderWildcard { provider: String },
    PrefixWildcard { provider: String, prefix: String },
}

fn classify_chain_key_wildcard(key: &str) -> ChainKeyWildcard {
    let key = key.trim();
    if !key.ends_with("/*") {
        return ChainKeyWildcard::Exact;
    }

    let template = &key[..key.len() - 2]; // 去掉 "/*"
    match template.find('/') {
        // "provider/*" → 整个模板就是 provider
        None => ChainKeyWildcard::ProviderWildcard {
            provider: template.to_string(),
        },
        Some(slash_pos) => {
            let first = &template[..slash_pos];
            let rest = &template[slash_pos + 1..];

            // 如果 rest 非空且不是通配符，则为 PrefixWildcard
            if !rest.is_empty() && rest != "*" {
                ChainKeyWildcard::PrefixWildcard {
                    provider: first.to_string(),
                    prefix: rest.to_string(),
                }
            } else {
                ChainKeyWildcard::ProviderWildcard {
                    provider: first.to_string(),
                }
            }
        }
    }
}

/// Selector 的通配符类别
#[derive(Debug, Clone)]
enum SelectorWildcardKind {
    None,
    ProviderWildcard { provider: String },
    PrefixWildcard { provider: String, prefix: String },
}

fn classify_selector_wildcard(selector: &str) -> SelectorWildcardKind {
    let selector = selector.trim();
    if !selector.ends_with("/*") {
        return SelectorWildcardKind::None;
    }

    let template = &selector[..selector.len() - 2];
    match template.find('/') {
        // "provider/*" → 整个模板就是 provider
        None => SelectorWildcardKind::ProviderWildcard {
            provider: template.to_string(),
        },
        Some(slash_pos) => {
            let first = &template[..slash_pos];
            let rest = &template[slash_pos + 1..];

            if !rest.is_empty() && rest != "*" {
                SelectorWildcardKind::PrefixWildcard {
                    provider: first.to_string(),
                    prefix: rest.to_string(),
                }
            } else {
                SelectorWildcardKind::ProviderWildcard {
                    provider: first.to_string(),
                }
            }
        }
    }
}

/// 解析 Selector 字符串为 FallbackSelector。
pub fn parse_selector(raw: &str) -> Option<FallbackSelector> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }

    // 通配符 Selector — 保留原样
    if raw.ends_with("/*") {
        return Some(FallbackSelector {
            raw: raw.to_string(),
            provider: raw.to_string(),
            id: "*".to_string(),
        });
    }

    // 普通 Selector: "provider/id"
    match raw.find('/') {
        Some(slash_pos) => {
            let provider = raw[..slash_pos].to_string();
            let id = raw[slash_pos + 1..].to_string();
            Some(FallbackSelector { raw: raw.to_string(), provider, id })
        }
        None => Some(FallbackSelector {
            raw: raw.to_string(),
            provider: raw.to_string(),
            id: "default".to_string(),
        }),
    }
}

/// 格式化 Provider 标识（用于抑制和日志）。
pub fn format_selector_identity(provider_id: &str, model_id: &str) -> String {
    format!("{}:{}", provider_id, model_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_chains(entries: Vec<(&str, Vec<&str>)>) -> FallbackChainResolver {
        let mut chains = FallbackChains::new();
        for (key, selectors) in entries {
            chains.insert(
                key.to_string(),
                selectors
                    .iter()
                    .filter_map(|s| parse_selector(s))
                    .collect(),
            );
        }
        FallbackChainResolver::new(chains)
    }

    #[test]
    fn test_exact_match_priority() {
        let resolver = make_chains(vec![
            ("default", vec!["anthropic/claude-haiku"]),
            ("anthropic/claude-sonnet", vec!["google/gemini-pro"]),
        ]);

        assert_eq!(
            resolver.resolve_chain_key("anthropic/claude-sonnet", None),
            Some("anthropic/claude-sonnet")
        );
    }

    #[test]
    fn test_wildcard_match() {
        let resolver = make_chains(vec![
            ("default", vec!["anthropic/claude-haiku"]),
            ("google/*", vec!["anthropic/claude-sonnet"]),
        ]);

        // google/gemini-pro 匹配 google/*
        assert_eq!(
            resolver.resolve_chain_key("google/gemini-pro", None),
            Some("google/*")
        );
    }

    #[test]
    fn test_longest_wildcard_wins() {
        let resolver = make_chains(vec![
            ("google/*", vec!["anthropic/claude-haiku"]),
            ("google/gemini/*", vec!["openai/gpt-4"]),
        ]);

        // google/gemini/pro 匹配 google/gemini/* (更具体)
        let key = resolver.resolve_chain_key("google/gemini/pro", None);
        assert_eq!(key, Some("google/gemini/*"));
    }

    #[test]
    fn test_rollback_to_default() {
        let resolver = make_chains(vec![
            ("default", vec!["anthropic/claude-haiku"]),
            ("special-role", vec!["google/gemini-pro"]),
        ]);

        // 无精确匹配、无通配符匹配 → default
        assert_eq!(
            resolver.resolve_chain_key("openai/gpt-4", None),
            Some("default")
        );
    }

    #[test]
    fn test_role_match_between_exact_and_default() {
        let resolver = make_chains(vec![
            ("default", vec!["anthropic/claude-haiku"]),
            ("subagent:deferred", vec!["anthropic/claude-sonnet"]),
        ]);

        // 角色匹配
        assert_eq!(
            resolver.resolve_chain_key("any-model", Some("subagent:deferred")),
            Some("subagent:deferred")
        );
    }

    #[test]
    fn test_no_match() {
        let resolver = make_chains(vec![
            ("only-opus", vec!["google/gemini-pro"]),
        ]);

        // 无 default 键、无匹配 → None
        assert_eq!(
            resolver.resolve_chain_key("openai/gpt-4", None),
            None
        );
    }

    #[test]
    fn test_provider_wildcard_selector() {
        let resolver = make_chains(vec![("default", vec!["google/*"])]);

        let selector = FallbackSelector {
            raw: "google/*".to_string(),
            provider: "google".to_string(),
            id: "*".to_string(),
        };

        let resolved = resolver.resolve_wildcard_selector(&selector, "claude-sonnet-4");
        assert_eq!(resolved.provider, "google");
        assert_eq!(resolved.id, "claude-sonnet-4");
    }

    #[test]
    fn test_prefix_wildcard_selector() {
        let resolver = make_chains(vec![("default", vec!["openrouter/google/*"])]);

        let selector = FallbackSelector {
            raw: "openrouter/google/*".to_string(),
            provider: "openrouter".to_string(),
            id: "*".to_string(),
        };

        let resolved =
            resolver.resolve_wildcard_selector(&selector, "google-antigravity/gemini-2.5-pro");
        assert_eq!(resolved.provider, "openrouter");
        assert_eq!(resolved.id, "google/gemini-2.5-pro");
    }

    #[test]
    fn test_build_candidate_chain() {
        let resolver = make_chains(vec![(
            "default",
            vec!["google/gemini-pro", "openai/gpt-4"],
        )]);

        let current = FallbackSelector {
            raw: "anthropic/claude-sonnet".to_string(),
            provider: "anthropic".to_string(),
            id: "claude-sonnet".to_string(),
        };

        let candidates = resolver.build_candidate_chain("default", &current, "claude-sonnet");
        assert_eq!(candidates.len(), 3); // primary + 2 fallbacks
        assert_eq!(candidates[0].provider, "anthropic");
        assert_eq!(candidates[1].provider, "google");
        assert_eq!(candidates[2].provider, "openai");
    }

    #[test]
    fn test_find_candidates_after_current() {
        let resolver = make_chains(vec![(
            "default",
            vec!["google/gemini-pro", "anthropic/claude-sonnet", "openai/gpt-4"],
        )]);

        let candidates = resolver.find_candidates_after_current("default", "google", "gemini-pro");
        assert_eq!(candidates.len(), 2);
        assert_eq!(candidates[0].provider, "anthropic");
        assert_eq!(candidates[1].provider, "openai");
    }

    #[test]
    fn test_parse_selector_normal() {
        let s = parse_selector("anthropic/claude-sonnet-4").unwrap();
        assert_eq!(s.provider, "anthropic");
        assert_eq!(s.id, "claude-sonnet-4");
        assert_eq!(s.raw, "anthropic/claude-sonnet-4");
    }

    #[test]
    fn test_parse_selector_wildcard() {
        let s = parse_selector("google/*").unwrap();
        assert_eq!(s.raw, "google/*");
    }

    #[test]
    fn test_format_selector_identity() {
        let identity = format_selector_identity("anthropic", "claude-sonnet-4");
        assert_eq!(identity, "anthropic:claude-sonnet-4");
    }

    #[test]
    fn test_skip_primary_in_chain() {
        // 确保主模型不会在回退链路中重复出现
        let resolver = make_chains(vec![(
            "default",
            vec!["anthropic/claude-sonnet", "google/gemini-pro"],
        )]);

        let current = FallbackSelector {
            raw: "anthropic/claude-sonnet".to_string(),
            provider: "anthropic".to_string(),
            id: "claude-sonnet".to_string(),
        };

        let candidates = resolver.build_candidate_chain("default", &current, "claude-sonnet");
        // 应该只有 2 个：primary + google（跳过重复的 anthropic/claude-sonnet）
        assert_eq!(candidates.len(), 2);
        assert_eq!(candidates[1].provider, "google");
    }
}
