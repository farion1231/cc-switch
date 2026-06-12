//! CostLedger — independent token cost tracking and budget enforcement.
//!
//! Tracks per-request cost, daily rollups, and enforces budget limits.
//! Works alongside the orchestration engine but is independent of it.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Pricing for a specific model (USD per 1M tokens).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelPricing {
    pub input_price_per_1m: f64,
    pub output_price_per_1m: f64,
}

/// Default pricing for known models. Can be overridden via config.
impl Default for ModelPricing {
    fn default() -> Self {
        Self {
            input_price_per_1m: 0.15,
            output_price_per_1m: 0.60,
        }
    }
}

/// A single cost record after an orchestration request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostRecord {
    pub model: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost_usd: f64,
    pub latency_ms: u64,
}

/// Daily cost rollup for budget tracking.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DailyCosts {
    pub date: String, // YYYY-MM-DD
    pub total_usd: f64,
    pub request_count: u64,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
}

/// The cost ledger tracks per-request costs and enforces budget limits.
pub struct CostLedger {
    pricing: HashMap<String, ModelPricing>,
    daily: DailyCosts,
    budget_usd: Option<f64>,
    records: Vec<CostRecord>,
}

impl CostLedger {
    pub fn new(budget_usd: Option<f64>) -> Self {
        let mut pricing = HashMap::new();
        // Default pricing for common models
        pricing.insert(
            "deepseek-chat".to_string(),
            ModelPricing {
                input_price_per_1m: 0.27,
                output_price_per_1m: 1.10,
            },
        );
        pricing.insert(
            "claude-sonnet-4".to_string(),
            ModelPricing {
                input_price_per_1m: 3.0,
                output_price_per_1m: 15.0,
            },
        );
        pricing.insert(
            "glm-4-flash".to_string(),
            ModelPricing {
                input_price_per_1m: 0.014,
                output_price_per_1m: 0.014,
            },
        );
        Self {
            pricing,
            daily: DailyCosts::default(),
            budget_usd,
            records: Vec::new(),
        }
    }

    /// Set custom pricing for a model.
    pub fn set_pricing(&mut self, model: &str, pricing: ModelPricing) {
        self.pricing.insert(model.to_string(), pricing);
    }

    /// Calculate cost for a model given token counts.
    pub fn calculate_cost(&self, model: &str, input_tokens: u64, output_tokens: u64) -> f64 {
        let pricing = self
            .pricing
            .get(model)
            .unwrap_or(&ModelPricing::default());
        (input_tokens as f64 / 1_000_000.0) * pricing.input_price_per_1m
            + (output_tokens as f64 / 1_000_000.0) * pricing.output_price_per_1m
    }

    /// Record a request and update daily totals.
    /// Returns `Err` if the budget would be exceeded.
    pub fn record(
        &mut self,
        model: &str,
        input_tokens: u64,
        output_tokens: u64,
        latency_ms: u64,
    ) -> Result<CostRecord, String> {
        let cost = self.calculate_cost(model, input_tokens, output_tokens);

        if let Some(budget) = self.budget_usd {
            if self.daily.total_usd + cost > budget {
                return Err(format!(
                    "Budget exceeded: ${:.4} + ${:.4} > ${:.2}",
                    self.daily.total_usd, cost, budget
                ));
            }
        }

        let record = CostRecord {
            model: model.to_string(),
            input_tokens,
            output_tokens,
            cost_usd: cost,
            latency_ms,
        };

        self.daily.total_usd += cost;
        self.daily.request_count += 1;
        self.daily.total_input_tokens += input_tokens;
        self.daily.total_output_tokens += output_tokens;
        self.records.push(record.clone());

        Ok(record)
    }

    /// Get the current daily rollup.
    pub fn daily_summary(&self) -> &DailyCosts {
        &self.daily
    }

    /// Reset daily totals (e.g., at midnight).
    pub fn reset_daily(&mut self, date: &str) {
        self.daily = DailyCosts {
            date: date.to_string(),
            ..Default::default()
        };
    }

    /// Get all recorded cost records.
    pub fn records(&self) -> &[CostRecord] {
        &self.records
    }

    /// Check if budget allows another request of estimated size.
    pub fn can_afford(&self, estimated_cost: f64) -> bool {
        match self.budget_usd {
            Some(budget) => self.daily.total_usd + estimated_cost <= budget,
            None => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calculates_cost_correctly() {
        let ledger = CostLedger::new(None);
        let cost = ledger.calculate_cost("deepseek-chat", 1_000_000, 500_000);
        // 1M * 0.27/1M + 0.5M * 1.10/1M = 0.27 + 0.55 = 0.82
        assert!((cost - 0.82).abs() < 0.01);
    }

    #[test]
    fn budget_enforcement_blocks_excess() {
        let mut ledger = CostLedger::new(Some(1.0));
        // Record a $0.80 request
        assert!(ledger.record("deepseek-chat", 1_000_000, 500_000, 100).is_ok());
        // Another $0.80 would exceed $1.00 budget
        assert!(ledger.record("deepseek-chat", 1_000_000, 500_000, 100).is_err());
    }

    #[test]
    fn daily_totals_accumulate() {
        let mut ledger = CostLedger::new(None);
        ledger.record("deepseek-chat", 1_000_000, 0, 100).unwrap();
        ledger.record("deepseek-chat", 0, 1_000_000, 200).unwrap();
        assert_eq!(ledger.daily_summary().request_count, 2);
        assert!(ledger.daily_summary().total_usd > 0.0);
    }
}
