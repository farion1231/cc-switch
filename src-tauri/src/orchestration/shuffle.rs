//! CandidateShuffler -- mitigate LLM Judge position bias by randomizing candidate order.
//!
//! LLM Judges exhibit a known *position bias*: they tend to prefer the first (or
//! last) candidate in a list.  `CandidateShuffler` randomly reorders candidates
//! before they are presented to the judge and records the original-to-shuffled
//! position mapping so the caller can map the judge's pick back to the real
//! candidate.
//!
//! ## Dependency note
//!
//! The `rand` crate is **not** currently listed in `Cargo.toml`.  Instead of
//! pulling in an external RNG, we use a simple xorshift64 PRNG seeded from the
//! current nanosecond timestamp -- sufficient for the anti-bias use case here.

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// A single candidate answer produced by a model.
#[derive(Debug, Clone)]
pub struct CandidateAnswer {
    pub model_key: String,
    pub content: String,
    pub quality_score: f64,
    pub latency_ms: u64,
    pub cost_usd: f64,
}

/// The output of a shuffle operation.
///
/// * `candidates` -- answers in the (randomized) presentation order.
/// * `order_map`  -- maps *original index* to *new index* so the caller can
///   translate the judge's position-based pick back to the real candidate.
#[derive(Debug, Clone)]
pub struct ShuffledCandidates {
    pub candidates: Vec<CandidateAnswer>,
    pub order_map: HashMap<usize, usize>, // orig_pos -> new_pos
}

// ---------------------------------------------------------------------------
// Simple xorshift64 PRNG
// ---------------------------------------------------------------------------

/// A minimal 64-bit xorshift PRNG.  Not cryptographically secure, but more
/// than adequate for shuffling candidate answers.
#[derive(Debug, Clone)]
struct Xorshift64 {
    state: u64,
}

impl Xorshift64 {
    /// Seed from the current nanosecond timestamp.
    fn from_time() -> Self {
        let ns = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0xdead_beef);
        // xorshift cannot start at zero
        Self {
            state: if ns == 0 { 1 } else { ns },
        }
    }

    /// Seed with a specific value (useful for deterministic tests).
    fn from_seed(seed: u64) -> Self {
        Self {
            state: if seed == 0 { 1 } else { seed },
        }
    }

    /// Return the next pseudo-random u64.
    fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.state = x;
        x
    }

    /// Return a pseudo-random usize in `[0, n)`.
    fn next_usize(&mut self, n: usize) -> usize {
        (self.next_u64() as usize) % n
    }
}

// ---------------------------------------------------------------------------
// Internal Fisher-Yates shuffle
// ---------------------------------------------------------------------------

/// In-place Fisher-Yates shuffle using the supplied PRNG.
fn fisher_yates<T>(items: &mut [T], rng: &mut Xorshift64) {
    if items.is_empty() {
        return;
    }
    for i in (1..items.len()).rev() {
        let j = rng.next_usize(i + 1);
        items.swap(i, j);
    }
}

// ---------------------------------------------------------------------------
// CandidateShuffler
// ---------------------------------------------------------------------------

/// Shuffles candidate answers to eliminate position bias before LLM judging.
pub struct CandidateShuffler;

impl CandidateShuffler {
    /// Shuffle candidate answers randomly, recording original -> new position mapping.
    ///
    /// Uses a time-seeded xorshift64 PRNG internally (no external `rand` dependency).
    pub fn shuffle(candidates: Vec<CandidateAnswer>) -> ShuffledCandidates {
        Self::shuffle_with_seed(candidates, None)
    }

    /// Deterministic shuffle with an explicit seed (useful for testing).
    pub fn shuffle_with_seed(
        candidates: Vec<CandidateAnswer>,
        seed: Option<u64>,
    ) -> ShuffledCandidates {
        let mut rng = if let Some(s) = seed {
            Xorshift64::from_seed(s)
        } else {
            Xorshift64::from_time()
        };

        // Pair each candidate with its original index.
        let mut indexed: Vec<(usize, CandidateAnswer)> =
            candidates.into_iter().enumerate().collect();

        // Fisher-Yates shuffle on the indexed list.
        fisher_yates(&mut indexed, &mut rng);

        // Build original-pos -> new-pos map.
        let order_map: HashMap<usize, usize> = indexed
            .iter()
            .enumerate()
            .map(|(new_pos, (orig_pos, _))| (*orig_pos, new_pos))
            .collect();

        ShuffledCandidates {
            candidates: indexed.into_iter().map(|(_, c)| c).collect(),
            order_map,
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_candidates(n: usize) -> Vec<CandidateAnswer> {
        (0..n)
            .map(|i| CandidateAnswer {
                model_key: format!("model_{}", i),
                content: format!("answer {}", i),
                quality_score: 0.5 + i as f64 * 0.1,
                latency_ms: 100 * i as u64,
                cost_usd: 0.001 * i as f64,
            })
            .collect()
    }

    // --- Deterministic shuffle preserves elements ---

    #[test]
    fn deterministic_shuffle_preserves_all_elements() {
        let cands = make_candidates(5);
        let result = CandidateShuffler::shuffle_with_seed(cands, Some(42));
        assert_eq!(result.candidates.len(), 5);
        assert_eq!(result.order_map.len(), 5);

        // Every original index must appear exactly once in the map values.
        let mut new_positions: Vec<usize> = result.order_map.values().copied().collect();
        new_positions.sort();
        assert_eq!(new_positions, vec![0, 1, 2, 3, 4]);
    }

    // --- Deterministic shuffle with same seed produces same order ---

    #[test]
    fn same_seed_produces_same_order() {
        let cands1 = make_candidates(4);
        let cands2 = make_candidates(4);
        let r1 = CandidateShuffler::shuffle_with_seed(cands1, Some(123));
        let r2 = CandidateShuffler::shuffle_with_seed(cands2, Some(123));

        let keys1: Vec<&str> = r1.candidates.iter().map(|c| c.model_key.as_str()).collect();
        let keys2: Vec<&str> = r2.candidates.iter().map(|c| c.model_key.as_str()).collect();
        assert_eq!(keys1, keys2, "Same seed should produce identical order");
    }

    // --- Different seeds produce different orders (probabilistic) ---

    #[test]
    fn different_seeds_produce_different_orders() {
        // Run several pairs -- at least one pair should differ.
        let mut found_difference = false;
        for seed in 1..20u64 {
            let cands1 = make_candidates(5);
            let cands2 = make_candidates(5);
            let r1 = CandidateShuffler::shuffle_with_seed(cands1, Some(seed));
            let r2 = CandidateShuffler::shuffle_with_seed(cands2, Some(seed + 1000));
            let keys1: Vec<&str> = r1.candidates.iter().map(|c| c.model_key.as_str()).collect();
            let keys2: Vec<&str> = r2.candidates.iter().map(|c| c.model_key.as_str()).collect();
            if keys1 != keys2 {
                found_difference = true;
                break;
            }
        }
        assert!(
            found_difference,
            "At least one pair of different seeds should yield different orders"
        );
    }

    // --- Non-deterministic shuffle changes order (probabilistic) ---

    #[test]
    fn shuffle_actually_changes_order() {
        // Run 20 shuffles -- at least one should differ from the original order.
        let original_keys: Vec<String> = (0..5).map(|i| format!("model_{}", i)).collect();
        let mut found_change = false;
        for _ in 0..20 {
            let cands = make_candidates(5);
            let result = CandidateShuffler::shuffle(cands);
            let keys: Vec<&str> = result
                .candidates
                .iter()
                .map(|c| c.model_key.as_str())
                .collect();
            let orig_refs: Vec<&str> = original_keys.iter().map(|s| s.as_str()).collect();
            if keys != orig_refs {
                found_change = true;
                break;
            }
        }
        assert!(
            found_change,
            "Shuffle should change order at least once in 20 attempts"
        );
    }

    // --- order_map correctness ---

    #[test]
    fn order_map_correctly_maps_positions() {
        let cands = make_candidates(4);
        let result = CandidateShuffler::shuffle_with_seed(cands, Some(99));

        // For every candidate in the shuffled list, verify the order_map entry.
        for (new_pos, cand) in result.candidates.iter().enumerate() {
            // Parse original index from model_key "model_{i}".
            let orig_idx: usize = cand.model_key.trim_start_matches("model_").parse().unwrap();
            assert_eq!(
                result.order_map[&orig_idx], new_pos,
                "order_map[{}] should be {}, got {}",
                orig_idx, new_pos, result.order_map[&orig_idx]
            );
        }
    }

    // --- Single candidate unchanged ---

    #[test]
    fn single_candidate_unchanged() {
        let cands = vec![CandidateAnswer {
            model_key: "only".into(),
            content: "solo answer".into(),
            quality_score: 0.9,
            latency_ms: 50,
            cost_usd: 0.001,
        }];
        let result = CandidateShuffler::shuffle_with_seed(cands, Some(42));
        assert_eq!(result.candidates.len(), 1);
        assert_eq!(result.candidates[0].model_key, "only");
        assert_eq!(result.order_map[&0], 0);
    }

    // --- Empty candidates handled ---

    #[test]
    fn empty_candidates_handled() {
        let result = CandidateShuffler::shuffle_with_seed(vec![], Some(42));
        assert!(result.candidates.is_empty());
        assert!(result.order_map.is_empty());
    }

    // --- Two candidates ---

    #[test]
    fn two_candidates_both_present() {
        let cands = make_candidates(2);
        let result = CandidateShuffler::shuffle_with_seed(cands, Some(7));
        assert_eq!(result.candidates.len(), 2);
        assert_eq!(result.order_map.len(), 2);

        let keys: Vec<&str> = result
            .candidates
            .iter()
            .map(|c| c.model_key.as_str())
            .collect();
        let mut sorted = keys.clone();
        sorted.sort();
        assert_eq!(sorted, vec!["model_0", "model_1"]);
    }
}
