//! Intelligent ranker for provider/key selection
//!
//! Pluggable trait with default implementation using:
//! - EMA of observed latency
//! - Cost per token
//! - Capability flags
//! - Quota headroom
//! - Carrier quality (LiteBike integration)

use crate::carrier::get_carrier_metrics;
use crate::types::{CarrierMetrics, RankContext};

/// Pluggable ranker trait
pub trait Ranker: Send + Sync {
    /// Calculate score for a provider+key combination
    /// Higher score = better choice
    fn score(&self, ctx: &RankContext) -> f64;

    /// Get ranker name for logging
    fn name(&self) -> &'static str {
        "default"
    }
}

/// Default ranker implementation
///
/// Uses weighted sum of:
/// - Latency score (EMA, lower is better)
/// - Cost score (lower is better)
/// - Capability score (bitmap match)
/// - Quota score (headroom)
/// - Carrier score (LiteBike metrics)
pub struct DefaultRanker {
    latency_weight: f64,
    cost_weight: f64,
    capability_weight: f64,
    quota_weight: f64,
    carrier_weight: f64,
}

impl DefaultRanker {
    pub fn new() -> Self {
        Self {
            latency_weight: 0.30,
            cost_weight: 0.20,
            capability_weight: 0.15,
            quota_weight: 0.20,
            carrier_weight: 0.15,
        }
    }

    /// Create with custom weights
    pub fn with_weights(
        latency: f64,
        cost: f64,
        capability: f64,
        quota: f64,
        carrier: f64,
    ) -> Self {
        let total = latency + cost + capability + quota + carrier;
        Self {
            latency_weight: latency / total,
            cost_weight: cost / total,
            capability_weight: capability / total,
            quota_weight: quota / total,
            carrier_weight: carrier / total,
        }
    }

    /// Get carrier metrics from global instance or use defaults
    fn get_carrier_metrics(&self, _ctx: &RankContext) -> CarrierMetrics {
        if let Some(metrics) = get_carrier_metrics() {
            metrics.to_carrier_metrics()
        } else {
            CarrierMetrics::default()
        }
    }
}

impl Default for DefaultRanker {
    fn default() -> Self {
        Self::new()
    }
}

impl Ranker for DefaultRanker {
    fn score(&self, ctx: &RankContext) -> f64 {
        // Latency score (lower is better, using EMA)
        let latency_score = 1.0 / (1.0 + ctx.observed_latency_ms / 100.0);

        // Cost score (lower is better)
        let cost_score = 1.0 / (1.0 + ctx.cost_per_token * 1_000_000.0);

        // Capability score (bitmap match)
        let capability_score = ctx.capability_flags.match_score();

        // Quota score (headroom, 0.0 - 1.0)
        let quota_score = if let Some(limit) = Some(ctx.quota_remaining) {
            limit.clamp(0.0, 1.0)
        } else {
            1.0 // No limit = full score
        };

        // Carrier quality score (from LiteBike integration or context)
        let carrier_score = ctx.carrier_quality.normalized_score();

        // Weighted sum
        latency_score * self.latency_weight
            + cost_score * self.cost_weight
            + capability_score * self.capability_weight
            + quota_score * self.quota_weight
            + carrier_score * self.carrier_weight
    }

    fn name(&self) -> &'static str {
        "default"
    }
}

/// Select best key from available options using ranker
pub fn select_best_key<F, K>(ranker: &dyn Ranker, keys: &[K], get_context: F) -> Option<usize>
where
    F: Fn(&K) -> RankContext,
{
    if keys.is_empty() {
        return None;
    }

    let mut best_index = 0;
    let mut best_score = ranker.score(&get_context(&keys[0]));

    for (i, key) in keys.iter().enumerate().skip(1) {
        let ctx = get_context(key);
        let score = ranker.score(&ctx);

        if score > best_score {
            best_score = score;
            best_index = i;
        }
    }

    Some(best_index)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ranker_latency_priority() {
        let ranker = DefaultRanker::with_weights(0.5, 0.1, 0.1, 0.2, 0.1);

        let fast_ctx = RankContext {
            provider: "anthropic".to_string(),
            model: "claude-3-5-sonnet".to_string(),
            key_id: "key-1".to_string(),
            observed_latency_ms: 50.0,
            cost_per_token: 0.000003,
            capability_flags: CapabilityFlags::default(),
            quota_remaining: 1.0,
            carrier_quality: CarrierMetrics::default(),
        };

        let slow_ctx = RankContext {
            observed_latency_ms: 500.0,
            ..fast_ctx.clone()
        };

        let fast_score = ranker.score(&fast_ctx);
        let slow_score = ranker.score(&slow_ctx);

        assert!(fast_score > slow_score, "Fast latency should score higher");
    }

    #[test]
    fn test_ranker_cost_priority() {
        let ranker = DefaultRanker::with_weights(0.1, 0.5, 0.1, 0.2, 0.1);

        let cheap_ctx = RankContext {
            provider: "openai".to_string(),
            model: "gpt-4o-mini".to_string(),
            key_id: "key-1".to_string(),
            observed_latency_ms: 100.0,
            cost_per_token: 0.00000015,
            capability_flags: CapabilityFlags::default(),
            quota_remaining: 1.0,
            carrier_quality: CarrierMetrics::default(),
        };

        let expensive_ctx = RankContext {
            cost_per_token: 0.000005,
            ..cheap_ctx.clone()
        };

        let cheap_score = ranker.score(&cheap_ctx);
        let expensive_score = ranker.score(&expensive_ctx);

        assert!(cheap_score > expensive_score, "Cheap should score higher");
    }

    #[test]
    fn test_select_best_key() {
        let ranker = DefaultRanker::new();

        let keys = vec![
            RankContext {
                provider: "anthropic".to_string(),
                model: "claude-3-5-sonnet".to_string(),
                key_id: "key-1".to_string(),
                observed_latency_ms: 100.0,
                cost_per_token: 0.000003,
                capability_flags: CapabilityFlags::default(),
                quota_remaining: 1.0,
                carrier_quality: CarrierMetrics::default(),
            },
            RankContext {
                provider: "anthropic".to_string(),
                model: "claude-3-5-sonnet".to_string(),
                key_id: "key-2".to_string(),
                observed_latency_ms: 200.0,
                cost_per_token: 0.000003,
                capability_flags: CapabilityFlags::default(),
                quota_remaining: 1.0,
                carrier_quality: CarrierMetrics::default(),
            },
        ];

        let best = select_best_key(&ranker, &keys, |ctx| ctx.clone());
        assert_eq!(
            best,
            Some(0),
            "First key should be selected (lower latency)"
        );
    }
}
