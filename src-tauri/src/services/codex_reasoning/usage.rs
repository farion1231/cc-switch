//! Per-round usage aggregation for reasoning continuation.

use bytes::Bytes;
use serde_json::Value;

use crate::error::AppError;
use crate::proxy::usage::calculator::CostBreakdown;

use super::continuation::ContinuationDecision;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RoundUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub cache_read_tokens: u32,
    pub cache_creation_tokens: u32,
}

#[derive(Debug, Clone)]
pub struct ContinuationRoundResult {
    pub round_index: u8,
    pub sse: Bytes,
    pub usage: RoundUsage,
    pub reasoning_tokens: Option<u32>,
    pub duration_ms: u64,
    pub terminal_output: Vec<Value>,
}

/// Per-round audit record (consumed by T14 logger / LogicalCodexRequestResult).
#[derive(Debug, Clone)]
#[allow(dead_code)] // fields read by T14 orchestrator + FE detail panel
pub struct ContinuationRoundRecord {
    pub round_index: u8,
    pub reasoning_tokens: Option<u32>,
    pub decision: String,
    pub status: String,
    pub duration_ms: u64,
    pub error_code: Option<String>,
}

impl ContinuationRoundRecord {
    #[allow(dead_code)] // used by T14 multi-round loop
    pub fn from_result(
        result: &ContinuationRoundResult,
        decision: &ContinuationDecision,
        status: impl Into<String>,
        error_code: Option<String>,
    ) -> Self {
        let decision_str = match decision {
            ContinuationDecision::Continue { grid_multiple } => {
                format!("continue:{grid_multiple}")
            }
            ContinuationDecision::Stop(reason) => format!("stop:{}", reason.as_str()),
        };
        Self {
            round_index: result.round_index,
            reasoning_tokens: result.reasoning_tokens,
            decision: decision_str,
            status: status.into(),
            duration_ms: result.duration_ms,
            error_code,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct RoundUsageAccumulator {
    pub usage: RoundUsage,
    pub reasoning_tokens: Option<u32>,
    pub total_cost: Option<CostBreakdown>,
    pub rounds: Vec<ContinuationRoundRecord>,
}

impl RoundUsageAccumulator {
    pub fn new() -> Self {
        Self::default()
    }

    /// Sum token counters across rounds; keep last non-None reasoning_tokens;
    /// sum cost totals when both sides present.
    pub fn add_round(
        &mut self,
        round: &ContinuationRoundResult,
        cost: Option<&CostBreakdown>,
    ) -> Result<(), AppError> {
        self.usage.input_tokens = self
            .usage
            .input_tokens
            .saturating_add(round.usage.input_tokens);
        self.usage.output_tokens = self
            .usage
            .output_tokens
            .saturating_add(round.usage.output_tokens);
        self.usage.cache_read_tokens = self
            .usage
            .cache_read_tokens
            .saturating_add(round.usage.cache_read_tokens);
        self.usage.cache_creation_tokens = self
            .usage
            .cache_creation_tokens
            .saturating_add(round.usage.cache_creation_tokens);

        if let Some(rt) = round.reasoning_tokens {
            self.reasoning_tokens = Some(self.reasoning_tokens.unwrap_or(0).saturating_add(rt));
        }

        if let Some(c) = cost {
            match &mut self.total_cost {
                Some(acc) => {
                    acc.input_cost += c.input_cost;
                    acc.output_cost += c.output_cost;
                    acc.cache_read_cost += c.cache_read_cost;
                    acc.cache_creation_cost += c.cache_creation_cost;
                    acc.total_cost += c.total_cost;
                }
                None => {
                    self.total_cost = Some(c.clone());
                }
            }
        }

        Ok(())
    }

    pub fn push_record(&mut self, record: ContinuationRoundRecord) {
        self.rounds.push(record);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::Decimal;

    fn sample_cost(total: &str) -> CostBreakdown {
        let t: Decimal = total.parse().unwrap();
        CostBreakdown {
            input_cost: t,
            output_cost: Decimal::ZERO,
            cache_read_cost: Decimal::ZERO,
            cache_creation_cost: Decimal::ZERO,
            total_cost: t,
        }
    }

    #[test]
    fn accumulator_sums_tokens_and_cost() {
        let mut acc = RoundUsageAccumulator::new();
        let r1 = ContinuationRoundResult {
            round_index: 0,
            sse: Bytes::new(),
            usage: RoundUsage {
                input_tokens: 10,
                output_tokens: 20,
                cache_read_tokens: 1,
                cache_creation_tokens: 2,
            },
            reasoning_tokens: Some(516),
            duration_ms: 100,
            terminal_output: vec![],
        };
        let r2 = ContinuationRoundResult {
            round_index: 1,
            sse: Bytes::new(),
            usage: RoundUsage {
                input_tokens: 5,
                output_tokens: 7,
                cache_read_tokens: 3,
                cache_creation_tokens: 0,
            },
            reasoning_tokens: Some(1034),
            duration_ms: 50,
            terminal_output: vec![],
        };
        let c1 = sample_cost("0.01");
        let c2 = sample_cost("0.02");
        acc.add_round(&r1, Some(&c1)).unwrap();
        acc.add_round(&r2, Some(&c2)).unwrap();
        assert_eq!(acc.usage.input_tokens, 15);
        assert_eq!(acc.usage.output_tokens, 27);
        assert_eq!(acc.usage.cache_read_tokens, 4);
        assert_eq!(acc.usage.cache_creation_tokens, 2);
        assert_eq!(acc.reasoning_tokens, Some(1550));
        assert_eq!(
            acc.total_cost.as_ref().unwrap().total_cost,
            Decimal::new(3, 2)
        );
    }
}
