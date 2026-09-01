//! Thinking 能力解析
//!
//! 回答一个问题：给定模型名，Anthropic 的 thinking 参数该用哪种形状？
//!
//! - [`ThinkingMode::Adaptive`] → `thinking: {"type":"adaptive"}` + `output_config.effort`
//! - [`ThinkingMode::Legacy`] → `thinking: {"type":"enabled","budget_tokens":N}`
//!
//! 设计要点是**未知的 Claude 即新 Claude**：需要 adaptive 的模型集合无限增长，而需要
//! `budget_tokens` 的旧模型集合是封闭的（Anthropic 不会再发需要它的新模型）。所以这里
//! 不维护「现代模型白名单」，而是维护旧模型的版本下界 —— 新发布的 Claude 靠家族 + 版本
//! 序自动判对，不需要改代码。
//!
//! 「未知即新」只对**看得出是 Claude** 的名字成立。第三方模型经 Anthropic 兼容端点
//! （`deepseek-v4-pro`、`glm-5.1`）判 [`ThinkingMode::Legacy`] 且 `grounded == false`，
//! 因为那些网关多数只认旧形状，甚至会静默忽略 `output_config` 让思考被丢弃却不报错 ——
//! 整流器无从学习，猜错就是净损失。
//!
//! 猜错的代价由 [`super::thinking_mode_rectifier`] 从上游报错里学回来，写进
//! [`ThinkingCapabilityStore`]，重试即自愈。

use std::collections::{HashMap, VecDeque};
use std::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard};

/// 学习缓存默认容量。(provider, model) 组合的实际基数很小，512 足够宽裕。
const DEFAULT_MAX_ENTRIES: usize = 512;

/// Anthropic thinking 参数的两种形状
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThinkingMode {
    /// `thinking: {"type":"adaptive"}`，深度由 `output_config.effort` 控制
    Adaptive,
    /// `thinking: {"type":"enabled","budget_tokens":N}`
    Legacy,
}

/// 模型家族
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Family {
    Opus,
    Sonnet,
    Haiku,
    Fable,
    Mythos,
}

/// 解析出的模型标识
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ModelId {
    family: Family,
    major: u32,
    minor: u32,
}

/// 模型名归类结果
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Classification {
    /// 认出了家族与版本
    Known(ModelId),
    /// Claude 3 之前的命名（`claude-2`、`claude-2.1` 等），没有家族段
    PreClaude3,
    /// 看得出是 Claude，但解析不出版本：第三方网关别名（`claude-latest`）、
    /// 将来可能出现的新家族（`claude-<新家族>-1`）。走「未知即新」判 Adaptive。
    UnknownClaude,
    /// 看不出跟 Claude 有关：第三方模型经 Anthropic 兼容端点（`deepseek-v4-pro`、
    /// `glm-5.1`、`MiniMax-M2.7`），或空串。
    ///
    /// 这类不能套「未知即新」—— 那些网关多数只认旧形状，甚至会静默忽略
    /// `output_config` 导致思考被丢弃却不报错，整流器无从学习。所以判 Legacy
    /// （即本改动之前的行为），并且发送端不据此改写客户端已给出的形状。
    Unrecognized,
}

/// 解析出的 thinking 能力
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThinkingCapability {
    /// 该用哪种 thinking 形状
    pub mode: ThinkingMode,
    /// 省略 `thinking` 字段时上游是否仍然会思考。
    ///
    /// Claude Opus 5 / Sonnet 5 / Fable / Mythos 为 true；Opus 4.8 / 4.7 为 false
    /// （省略等于不思考，必须显式写 adaptive）。
    pub adaptive_is_default: bool,
    /// 上游是否拒绝 `thinking: {"type":"disabled"}`（Fable / Mythos 家族）
    pub cannot_disable: bool,
    /// `mode` 是否有确切依据 —— 从模型名看出是 Claude，或从上游报错学到过。
    ///
    /// 为 false 时（第三方模型经 Anthropic 兼容端点）`mode` 只是保守回落值，
    /// 发送端**不应**据此改写客户端已经给出的 thinking 形状。
    pub grounded: bool,
}

/// 模型名归一化：trim + 小写 + 把各种分隔符统一成 `-`
///
/// 需要穿透实际会出现的包装形态：`anthropic/claude-opus-5`、
/// `anthropic.claude-opus-5`、`global.anthropic.claude-opus-5`、
/// `claude-opus-5[1m]`、`anthropic.claude-opus-4-6-20250514-v1:0`、
/// `claude-opus-4-5@20251101`。
fn normalize_model_name(model: &str) -> String {
    model
        .trim()
        .to_ascii_lowercase()
        .replace(['.', '_', '/', '@', ':', '[', ']'], "-")
}

fn family_from_token(token: &str) -> Option<Family> {
    match token {
        "opus" => Some(Family::Opus),
        "sonnet" => Some(Family::Sonnet),
        "haiku" => Some(Family::Haiku),
        "fable" => Some(Family::Fable),
        "mythos" => Some(Family::Mythos),
        _ => None,
    }
}

/// 版本号段：纯数字且不超过 2 位。
///
/// 长度上界是为了把日期后缀（`20250514`、`20251101`）排除在版本解析之外。
fn version_token(token: &str) -> Option<u32> {
    if token.is_empty() || token.len() > 2 || !token.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    token.parse().ok()
}

/// 把模型名归类。
///
/// 同时认两种命名布局：
/// - 现代：`claude-<family>-<major>[-<minor>]` —— `claude-opus-5`、`claude-sonnet-4-6`
/// - 旧式：`claude-<major>[-<minor>]-<family>` —— `claude-3-7-sonnet`、`claude-3-opus`
///
/// 用精确 token 匹配而非子串 `contains`，所以 `claude-opus-4-5` 不会被误判成 5 系。
fn classify(model: &str) -> Classification {
    let normalized = normalize_model_name(model);
    let tokens: Vec<&str> = normalized.split('-').filter(|t| !t.is_empty()).collect();

    let family_idx = tokens
        .iter()
        .position(|token| family_from_token(token).is_some());

    let Some(family_idx) = family_idx else {
        // 没有家族段。名字里出现 "claude" 才算 Claude —— 现有全部 Anthropic 模型 ID
        // 都含这一段（含 Bedrock 的 `anthropic.claude-*` 与 Vertex 的 `claude-*@date`），
        // 所以它是够稳的判据。
        let Some(claude_idx) = tokens.iter().position(|token| *token == "claude") else {
            return Classification::Unrecognized;
        };
        // `claude-2` / `claude-2.1` 这类 Claude 3 之前的命名要判成旧模型
        if tokens
            .get(claude_idx + 1)
            .copied()
            .and_then(version_token)
            .is_some()
        {
            return Classification::PreClaude3;
        }
        // `claude-latest` 之类解析不出版本的别名：走「未知即新」
        return Classification::UnknownClaude;
    };

    // Safe: family_idx 来自上面的 position()
    let family = family_from_token(tokens[family_idx]).expect("family token");

    // 现代布局：版本紧跟在家族段之后
    let mut version: Vec<u32> = tokens
        .iter()
        .skip(family_idx + 1)
        .map_while(|token| version_token(token))
        .take(2)
        .collect();

    // 旧式布局：版本在家族段之前，从紧邻家族的一段往前收集，收完再反转回自然顺序
    if version.is_empty() {
        version = tokens
            .iter()
            .take(family_idx)
            .rev()
            .map_while(|token| version_token(token))
            .take(2)
            .collect();
        version.reverse();
    }

    match version.as_slice() {
        [major] => Classification::Known(ModelId {
            family,
            major: *major,
            minor: 0,
        }),
        [major, minor] => Classification::Known(ModelId {
            family,
            major: *major,
            minor: *minor,
        }),
        // Fable / Mythos 不走数字体系（`claude-mythos-preview`），无版本也算认出
        _ if matches!(family, Family::Fable | Family::Mythos) => Classification::Known(ModelId {
            family,
            major: 0,
            minor: 0,
        }),
        // 认出家族但解析不出版本（如第三方的 `my-opus-router`）：按未知的 Claude 处理
        _ => Classification::UnknownClaude,
    }
}

/// 该家族启用 adaptive 的版本下界。`None` 表示该家族恒为 adaptive。
fn adaptive_threshold(family: Family) -> Option<(u32, u32)> {
    match family {
        Family::Opus | Family::Sonnet => Some((4, 6)),
        // Haiku 4.5 既不支持 adaptive 也不支持 effort
        Family::Haiku => Some((5, 0)),
        Family::Fable | Family::Mythos => None,
    }
}

fn mode_for(classification: Classification) -> ThinkingMode {
    match classification {
        Classification::PreClaude3 => ThinkingMode::Legacy,
        // 未知的 Claude 即新 Claude
        Classification::UnknownClaude => ThinkingMode::Adaptive,
        // 看不出是 Claude：保守回落到旧形状，见 Classification::Unrecognized 的说明
        Classification::Unrecognized => ThinkingMode::Legacy,
        Classification::Known(id) => match adaptive_threshold(id.family) {
            None => ThinkingMode::Adaptive,
            Some(min) if (id.major, id.minor) >= min => ThinkingMode::Adaptive,
            Some(_) => ThinkingMode::Legacy,
        },
    }
}

fn adaptive_is_default_for(classification: Classification) -> bool {
    match classification {
        Classification::Known(id) => match id.family {
            Family::Fable | Family::Mythos => true,
            Family::Opus | Family::Sonnet => (id.major, id.minor) >= (5, 0),
            // 目前没有默认开思考的 Haiku；未知的将来版本按保守值处理
            Family::Haiku => false,
        },
        _ => false,
    }
}

/// 结论是否有确切依据。只有「看不出跟 Claude 有关」才算没有。
fn grounded_for(classification: Classification) -> bool {
    !matches!(classification, Classification::Unrecognized)
}

fn cannot_disable_for(classification: Classification) -> bool {
    matches!(
        classification,
        Classification::Known(ModelId {
            family: Family::Fable | Family::Mythos,
            ..
        })
    )
}

/// 纯启发式解析，不查学习缓存。
pub fn resolve_capability(model: &str) -> ThinkingCapability {
    let classification = classify(model);
    ThinkingCapability {
        mode: mode_for(classification),
        adaptive_is_default: adaptive_is_default_for(classification),
        cannot_disable: cannot_disable_for(classification),
        grounded: grounded_for(classification),
    }
}

/// 纯启发式解析出的 thinking 形状。
///
/// 生产链路走 [`ThinkingCapabilityStore::resolve`]（会先查学习缓存）；这里是
/// 只关心形状、没有 provider 上下文时的便捷入口。
#[cfg(test)]
pub fn resolve_mode(model: &str) -> ThinkingMode {
    mode_for(classify(model))
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct CacheKey {
    provider_id: String,
    model: String,
}

impl CacheKey {
    fn new(provider_id: &str, model: &str) -> Self {
        Self {
            provider_id: provider_id.to_string(),
            // 模型名归一化后再做键，避免大小写/分隔符差异造成重复条目
            model: normalize_model_name(model),
        }
    }
}

#[derive(Debug, Default)]
struct Inner {
    learned: HashMap<CacheKey, ThinkingMode>,
    /// 写入顺序，用于容量超限时逐出最早写入的条目
    order: VecDeque<CacheKey>,
}

/// 从上游报错中学到的 thinking 形状缓存。
///
/// 只在内存中，跟随代理生命周期。代理重启后缓存清空，猜错的模型会再吃一次上游 400，
/// 但整流器会自动重试并重新学到 —— 用户只感到那一次略慢。
///
/// 键是 `(provider_id, 客户端原始 model)`：客户端模型名到上游模型名的映射在同一
/// provider 下是确定的，所以这个二元组等价于上游模型身份，且发送端与失败重试循环
/// 都能直接拿到，不必重算整条映射链。
#[derive(Debug)]
pub struct ThinkingCapabilityStore {
    max_entries: usize,
    inner: RwLock<Inner>,
}

impl Default for ThinkingCapabilityStore {
    fn default() -> Self {
        Self::with_limit(DEFAULT_MAX_ENTRIES)
    }
}

impl ThinkingCapabilityStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_limit(max_entries: usize) -> Self {
        Self {
            max_entries: max_entries.max(1),
            inner: RwLock::new(Inner::default()),
        }
    }

    /// 解析 thinking 能力：先查学习缓存，未命中回落到启发式。
    ///
    /// 假定客户端模型名与发往上游的模型名一致，因此**仅供测试使用**。
    ///
    /// 生产链路一律走 [`Self::resolve_mapped`] 并显式传入映射后的上游模型名：
    /// 模型映射（`apply_model_mapping` / `apply_codex_upstream_model`）随时可能让
    /// 两者不同，而发送端与整流器一旦按不同模型名解析，就会得出不同的 `mode`，
    /// 导致「发送端发 A 形状被拒、整流器却认为已经是目标形状」而永不学习（自愈死锁）。
    /// 把这个便捷入口限制在测试内，可让编译器帮忙守住这条不变量。
    #[cfg(test)]
    pub fn resolve(&self, provider_id: &str, model: &str) -> ThinkingCapability {
        self.resolve_mapped(provider_id, model, model)
    }

    /// 解析 thinking 能力，区分「缓存键用哪个模型名」与「启发式按哪个模型名算」。
    ///
    /// 两者会不同 —— 模型映射（`apply_model_mapping` / `apply_codex_upstream_model`）
    /// 会把客户端模型名改写成上游模型名。缓存键沿用**客户端**模型名（失败重试循环持有
    /// 的是客户端原始请求体，只拿得到它），但形状必须按**真正发往上游**的模型判断：
    /// 客户端发 `gpt-5-codex`、provider 配了 `codexUpstreamModel: claude-sonnet-4-5`
    /// 时，按前者解析会判成 adaptive 而上游需要 `budget_tokens`。
    ///
    /// `upstream_model` 为空时回落到 `cache_model`。
    pub fn resolve_mapped(
        &self,
        provider_id: &str,
        cache_model: &str,
        upstream_model: &str,
    ) -> ThinkingCapability {
        let heuristic_model = if upstream_model.trim().is_empty() {
            cache_model
        } else {
            upstream_model
        };
        let mut capability = resolve_capability(heuristic_model);

        if let Some(learned) = self.learned_mode(provider_id, cache_model) {
            capability.mode = learned;
            // 上游亲口否决过，这就是最强的依据
            capability.grounded = true;
            if learned == ThinkingMode::Legacy {
                // 学到 Legacy 说明上游不认 adaptive，那「省略即思考」也不可能成立。
                // 保持两个字段一致，避免转换层据此又写出 adaptive。
                capability.adaptive_is_default = false;
            }
        }

        capability
    }

    /// 记下某 provider 上某模型实际该用的 thinking 形状。
    pub fn learn(&self, provider_id: &str, model: &str, mode: ThinkingMode) {
        let key = CacheKey::new(provider_id, model);
        let mut inner = self.write_inner();

        if let Some(pos) = inner.order.iter().position(|existing| existing == &key) {
            inner.order.remove(pos);
        }
        inner.order.push_back(key.clone());
        inner.learned.insert(key, mode);

        while inner.learned.len() > self.max_entries {
            let Some(evicted) = inner.order.pop_front() else {
                break;
            };
            inner.learned.remove(&evicted);
        }
    }

    fn learned_mode(&self, provider_id: &str, model: &str) -> Option<ThinkingMode> {
        let key = CacheKey::new(provider_id, model);
        self.read_inner().learned.get(&key).copied()
    }

    fn read_inner(&self) -> RwLockReadGuard<'_, Inner> {
        self.inner.read().unwrap_or_else(|poisoned| {
            log::warn!("[ThinkingCapability] recovering poisoned read lock");
            poisoned.into_inner()
        })
    }

    fn write_inner(&self) -> RwLockWriteGuard<'_, Inner> {
        self.inner.write().unwrap_or_else(|poisoned| {
            log::warn!("[ThinkingCapability] recovering poisoned write lock");
            poisoned.into_inner()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ==================== 极性：未知的 Claude 即新 Claude ====================

    #[test]
    fn unparseable_claude_names_resolve_to_adaptive() {
        // 整套设计的核心断言：看得出是 Claude 但解析不出版本的，一律当成新模型
        for model in [
            "claude-latest",
            "claude-sonnet-latest",
            "my-opus-router",
            "claude-code-max",
            // 将来的新家族：认不出家族段，但名字里有 claude
            "claude-newfamily-1",
        ] {
            assert_eq!(resolve_mode(model), ThinkingMode::Adaptive, "model={model}");
            assert!(resolve_capability(model).grounded, "model={model}");
        }
    }

    #[test]
    fn non_claude_model_names_are_not_grounded_and_fall_back_to_legacy() {
        // api_format 默认就是 "anthropic"，所以大量「第三方模型经 Anthropic 兼容端点」
        // 的 provider 会流过解析器（src/config/claudeProviderPresets.ts 里的 ANTHROPIC_MODEL）。
        // 对这些名字套「未知即新」是净损失：那些网关多数只认旧形状，且可能静默忽略
        // output_config —— 不报错，整流器就学不到东西。
        for model in [
            "",
            "   ",
            "gpt-4o",
            "gpt-5-codex",
            "default",
            "qwen3-coder",
            "deepseek-v4-pro",
            "glm-5.1",
            "zai-org/glm-5.1",
            "kimi-k2.7-code",
            "MiniMax-M2.7",
            "step-3.5-flash-2603",
            "doubao-seed-2-1-pro-260628",
            "gemini-3.6-flash",
            "grok-4.5",
            "LongCat-2.0",
            "mimo-v2.5-pro",
        ] {
            let capability = resolve_capability(model);
            assert!(!capability.grounded, "model={model}");
            // 回落值等于本改动之前的行为，发送端不会据此改写客户端请求
            assert_eq!(capability.mode, ThinkingMode::Legacy, "model={model}");
        }
    }

    #[test]
    fn learning_from_upstream_grounds_an_otherwise_unrecognized_model() {
        // 第三方网关背后其实是 Claude 的情况：上游亲口否决过就是最强依据，
        // 此后直通链路也可以放心改写形状
        let store = ThinkingCapabilityStore::new();
        assert!(!store.resolve("p1", "house-model").grounded);

        store.learn("p1", "house-model", ThinkingMode::Adaptive);
        let capability = store.resolve("p1", "house-model");
        assert!(capability.grounded);
        assert_eq!(capability.mode, ThinkingMode::Adaptive);
    }

    #[test]
    fn future_models_resolve_to_adaptive_without_code_changes() {
        for model in [
            "claude-opus-5",
            "claude-opus-6",
            "claude-opus-7-2",
            "claude-sonnet-6",
            "claude-haiku-5",
        ] {
            assert_eq!(resolve_mode(model), ThinkingMode::Adaptive, "model={model}");
        }
    }

    // ==================== 解析：包装形态 ====================

    #[test]
    fn parses_through_vendor_prefixes_and_suffixes() {
        for model in [
            "claude-opus-5",
            "anthropic/claude-opus-5",
            "anthropic.claude-opus-5",
            "global.anthropic.claude-opus-5",
            "claude-opus-5[1m]",
            "  CLAUDE-OPUS-5  ",
        ] {
            assert_eq!(
                classify(model),
                Classification::Known(ModelId {
                    family: Family::Opus,
                    major: 5,
                    minor: 0,
                }),
                "model={model}"
            );
        }
    }

    #[test]
    fn date_and_revision_suffixes_do_not_corrupt_version() {
        assert_eq!(
            classify("anthropic.claude-opus-4-6-20250514-v1:0"),
            Classification::Known(ModelId {
                family: Family::Opus,
                major: 4,
                minor: 6,
            })
        );
        assert_eq!(
            classify("claude-opus-4-5@20251101"),
            Classification::Known(ModelId {
                family: Family::Opus,
                major: 4,
                minor: 5,
            })
        );
        assert_eq!(
            classify("claude-haiku-4-5-20251001"),
            Classification::Known(ModelId {
                family: Family::Haiku,
                major: 4,
                minor: 5,
            })
        );
        assert_eq!(
            classify("claude-opus-4-6[1m]"),
            Classification::Known(ModelId {
                family: Family::Opus,
                major: 4,
                minor: 6,
            })
        );
    }

    #[test]
    fn parses_legacy_family_after_version_layout() {
        assert_eq!(
            classify("claude-3-7-sonnet-20250219"),
            Classification::Known(ModelId {
                family: Family::Sonnet,
                major: 3,
                minor: 7,
            })
        );
        assert_eq!(
            classify("claude-3-5-sonnet-20241022"),
            Classification::Known(ModelId {
                family: Family::Sonnet,
                major: 3,
                minor: 5,
            })
        );
        assert_eq!(
            classify("claude-3-opus-20240229"),
            Classification::Known(ModelId {
                family: Family::Opus,
                major: 3,
                minor: 0,
            })
        );
        assert_eq!(
            classify("claude-3-haiku-20240307"),
            Classification::Known(ModelId {
                family: Family::Haiku,
                major: 3,
                minor: 0,
            })
        );
    }

    #[test]
    fn named_families_parse_without_version() {
        assert_eq!(
            classify("claude-mythos-preview"),
            Classification::Known(ModelId {
                family: Family::Mythos,
                major: 0,
                minor: 0,
            })
        );
        assert_eq!(
            classify("claude-fable-5"),
            Classification::Known(ModelId {
                family: Family::Fable,
                major: 5,
                minor: 0,
            })
        );
    }

    #[test]
    fn pre_claude_3_names_are_legacy_not_unknown() {
        assert_eq!(classify("claude-2.1"), Classification::PreClaude3);
        assert_eq!(classify("claude-2.0"), Classification::PreClaude3);
        assert_eq!(resolve_mode("claude-2.1"), ThinkingMode::Legacy);
    }

    // ==================== 阈值 ====================

    #[test]
    fn opus_4_5_is_not_mistaken_for_the_5_series() {
        // 老白名单用 contains 匹配，这是它最容易出的错
        assert_eq!(resolve_mode("claude-opus-4-5"), ThinkingMode::Legacy);
        assert_eq!(
            resolve_mode("claude-opus-4-5-20251101"),
            ThinkingMode::Legacy
        );
        assert_eq!(resolve_mode("claude-opus-5"), ThinkingMode::Adaptive);
    }

    #[test]
    fn threshold_boundaries() {
        // opus / sonnet 的分界在 4.6
        assert_eq!(resolve_mode("claude-opus-4-6"), ThinkingMode::Adaptive);
        assert_eq!(resolve_mode("claude-opus-4-5"), ThinkingMode::Legacy);
        assert_eq!(resolve_mode("claude-sonnet-4-6"), ThinkingMode::Adaptive);
        assert_eq!(resolve_mode("claude-sonnet-4-5"), ThinkingMode::Legacy);
        // haiku 的分界在 5.0
        assert_eq!(resolve_mode("claude-haiku-4-5"), ThinkingMode::Legacy);
        assert_eq!(resolve_mode("claude-haiku-5"), ThinkingMode::Adaptive);
        // 更老的一律 legacy
        assert_eq!(resolve_mode("claude-3-7-sonnet"), ThinkingMode::Legacy);
        assert_eq!(resolve_mode("claude-3-opus"), ThinkingMode::Legacy);
    }

    #[test]
    fn current_generation_models_use_adaptive() {
        // 取代旧的 uses_adaptive_thinking 白名单测试
        for model in [
            "claude-opus-5",
            "claude-opus-4-8",
            "anthropic/claude-opus-4.8",
            "claude-opus-4-7",
            "claude-opus-4-6",
            "claude-sonnet-5",
            "claude-sonnet-4-6",
            "anthropic/claude-fable-5",
            "claude-mythos-5",
            "claude-mythos-preview",
        ] {
            assert_eq!(resolve_mode(model), ThinkingMode::Adaptive, "model={model}");
        }
    }

    // ==================== adaptive_is_default / cannot_disable ====================

    #[test]
    fn adaptive_is_default_only_for_5_series_and_named_families() {
        for model in [
            "claude-opus-5",
            "claude-sonnet-5",
            "claude-fable-5",
            "claude-mythos-5",
            "claude-mythos-preview",
        ] {
            assert!(
                resolve_capability(model).adaptive_is_default,
                "model={model}"
            );
        }
        // Opus 4.8 / 4.7 省略 thinking 等于不思考
        for model in [
            "claude-opus-4-8",
            "claude-opus-4-7",
            "claude-opus-4-6",
            "claude-sonnet-4-6",
            "claude-haiku-4-5",
            "unknown-alias",
        ] {
            assert!(
                !resolve_capability(model).adaptive_is_default,
                "model={model}"
            );
        }
    }

    #[test]
    fn only_fable_family_cannot_disable_thinking() {
        assert!(resolve_capability("claude-fable-5").cannot_disable);
        assert!(resolve_capability("claude-mythos-5").cannot_disable);
        assert!(resolve_capability("claude-mythos-preview").cannot_disable);
        assert!(!resolve_capability("claude-opus-5").cannot_disable);
        assert!(!resolve_capability("claude-sonnet-5").cannot_disable);
        assert!(!resolve_capability("unknown-alias").cannot_disable);
    }

    // ==================== 学习缓存 ====================

    #[test]
    fn learned_mode_overrides_heuristic() {
        let store = ThinkingCapabilityStore::new();
        assert_eq!(
            store.resolve("p1", "claude-opus-5").mode,
            ThinkingMode::Adaptive
        );

        store.learn("p1", "claude-opus-5", ThinkingMode::Legacy);
        assert_eq!(
            store.resolve("p1", "claude-opus-5").mode,
            ThinkingMode::Legacy
        );

        store.learn("p1", "claude-opus-5", ThinkingMode::Adaptive);
        assert_eq!(
            store.resolve("p1", "claude-opus-5").mode,
            ThinkingMode::Adaptive
        );
    }

    #[test]
    fn learned_legacy_forces_adaptive_is_default_off() {
        let store = ThinkingCapabilityStore::new();
        assert!(store.resolve("p1", "claude-opus-5").adaptive_is_default);

        store.learn("p1", "claude-opus-5", ThinkingMode::Legacy);
        let capability = store.resolve("p1", "claude-opus-5");
        assert_eq!(capability.mode, ThinkingMode::Legacy);
        assert!(!capability.adaptive_is_default);
    }

    // ==================== 缓存键 vs 启发式输入 ====================

    #[test]
    fn heuristic_follows_the_upstream_model_while_the_key_follows_the_client_model() {
        // Codex 客户端发 gpt-5-codex，provider 配了 codexUpstreamModel: claude-sonnet-4-5。
        // 按客户端模型名解析会判 Legacy（认不出）—— 这次恰好也对，但换成
        // codexUpstreamModel: claude-opus-5 就会判错，所以启发式必须看上游模型名。
        let store = ThinkingCapabilityStore::new();
        assert_eq!(
            store
                .resolve_mapped("p1", "gpt-5-codex", "claude-sonnet-4-5")
                .mode,
            ThinkingMode::Legacy
        );
        assert_eq!(
            store
                .resolve_mapped("p1", "gpt-5-codex", "claude-opus-5")
                .mode,
            ThinkingMode::Adaptive
        );

        // 学习缓存按客户端模型名查 —— 失败重试循环持有的是客户端原始请求体
        store.learn("p1", "gpt-5-codex", ThinkingMode::Legacy);
        assert_eq!(
            store
                .resolve_mapped("p1", "gpt-5-codex", "claude-opus-5")
                .mode,
            ThinkingMode::Legacy
        );
    }

    #[test]
    fn empty_upstream_model_falls_back_to_the_cache_model() {
        let store = ThinkingCapabilityStore::new();
        assert_eq!(
            store.resolve_mapped("p1", "claude-opus-5", "").mode,
            ThinkingMode::Adaptive
        );
        assert_eq!(
            store.resolve_mapped("p1", "claude-opus-5", "   ").mode,
            ThinkingMode::Adaptive
        );
    }

    #[test]
    fn learning_is_scoped_per_provider() {
        let store = ThinkingCapabilityStore::new();
        store.learn("p1", "claude-opus-5", ThinkingMode::Legacy);

        assert_eq!(
            store.resolve("p1", "claude-opus-5").mode,
            ThinkingMode::Legacy
        );
        // 另一个 provider 上同名模型不受影响
        assert_eq!(
            store.resolve("p2", "claude-opus-5").mode,
            ThinkingMode::Adaptive
        );
    }

    #[test]
    fn learning_normalizes_the_model_key() {
        let store = ThinkingCapabilityStore::new();
        store.learn("p1", "claude-opus-5", ThinkingMode::Legacy);
        // 大小写与分隔符差异应命中同一条目
        assert_eq!(
            store.resolve("p1", "CLAUDE_OPUS_5").mode,
            ThinkingMode::Legacy
        );
    }

    #[test]
    fn evicts_oldest_entry_when_capacity_is_exceeded() {
        let store = ThinkingCapabilityStore::with_limit(2);
        store.learn("p1", "claude-opus-5", ThinkingMode::Legacy);
        store.learn("p1", "claude-sonnet-5", ThinkingMode::Legacy);
        store.learn("p1", "claude-fable-5", ThinkingMode::Legacy);

        // 最早写入的被逐出，回落到启发式
        assert_eq!(
            store.resolve("p1", "claude-opus-5").mode,
            ThinkingMode::Adaptive
        );
        assert_eq!(
            store.resolve("p1", "claude-sonnet-5").mode,
            ThinkingMode::Legacy
        );
        assert_eq!(
            store.resolve("p1", "claude-fable-5").mode,
            ThinkingMode::Legacy
        );
    }

    #[test]
    fn relearning_refreshes_eviction_order() {
        // 用启发式判 Adaptive 的名字，这样逐出后回落值与学到的值不同、断言才有效
        let store = ThinkingCapabilityStore::with_limit(2);
        store.learn("p1", "claude-a", ThinkingMode::Legacy);
        store.learn("p1", "claude-b", ThinkingMode::Legacy);
        // 重新写入 a 应把它移到队尾，于是下一次逐出的是 b
        store.learn("p1", "claude-a", ThinkingMode::Legacy);
        store.learn("p1", "claude-c", ThinkingMode::Legacy);

        assert_eq!(store.resolve("p1", "claude-a").mode, ThinkingMode::Legacy);
        assert_eq!(store.resolve("p1", "claude-b").mode, ThinkingMode::Adaptive);
        assert_eq!(store.resolve("p1", "claude-c").mode, ThinkingMode::Legacy);
    }
}
