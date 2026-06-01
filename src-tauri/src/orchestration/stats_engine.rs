use std::collections::HashMap;
use std::path::PathBuf;

use chrono::{Duration, Utc};

use crate::orchestration::history::HistoryStore;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Aggregated performance statistics for a single model.
#[derive(Debug, Clone)]
pub struct ModelStats {
    pub model_key: String,
    pub avg_quality: f64,
    pub avg_latency_ms: u64,
    pub avg_cost_usd: f64,
    pub total_requests: u32,
    pub pass_rate: f64,
    pub user_satisfaction: f64,
}

/// Time window for statistical queries.
#[derive(Debug, Clone, Copy)]
pub enum TimeWindow {
    Last24h,
    Last7d,
    Last30d,
}

impl TimeWindow {
    /// Return the ISO 8601 timestamp string for the start of this window.
    fn since_timestamp(&self) -> String {
        let now = Utc::now();
        let duration = match self {
            TimeWindow::Last24h => Duration::hours(24),
            TimeWindow::Last7d => Duration::days(7),
            TimeWindow::Last30d => Duration::days(30),
        };
        (now - duration).to_rfc3339()
    }
}

// ---------------------------------------------------------------------------
// StatsEngine
// ---------------------------------------------------------------------------

/// Statistics engine that aggregates orchestration history into actionable
/// model weights and performance metrics.
pub struct StatsEngine {
    db_path: PathBuf,
}

impl StatsEngine {
    pub fn new(db_path: PathBuf) -> Self {
        Self { db_path }
    }

    /// Open the HistoryStore (read-only access to the same DB).
    fn open_store(&self) -> Result<HistoryStore, String> {
        HistoryStore::new(&self.db_path)
    }

    /// Aggregate performance statistics for a specific model, optionally
    /// filtered by task type and time window.
    ///
    /// If no records match, returns zeroed stats (not an error).
    pub fn model_performance(
        &self,
        model_key: &str,
        task_type: Option<&str>,
        window: TimeWindow,
    ) -> Result<ModelStats, String> {
        let store = self.open_store()?;
        let since = window.since_timestamp();
        let records = store.query_records_since(&since)?;

        // Extract ModelCall entries that match the requested model_key.
        let mut qualities: Vec<f64> = Vec::new();
        let mut latencies: Vec<u64> = Vec::new();
        let mut costs: Vec<f64> = Vec::new();
        let mut total_requests: u32 = 0;
        let mut pass_count: u32 = 0;
        let mut positive_feedback: u32 = 0;
        let mut feedback_count: u32 = 0;

        for rec in &records {
            // Optionally filter by task_type.
            if let Some(tt) = task_type {
                if rec.task_type != tt {
                    continue;
                }
            }

            for mc in &rec.models_called {
                if mc.model_key != model_key {
                    continue;
                }
                total_requests += 1;
                qualities.push(mc.quality_score);
                latencies.push(mc.latency_ms);
                costs.push(mc.cost_usd);
            }

            // Count pass rate based on records where this model was called.
            let model_called = rec
                .models_called
                .iter()
                .any(|mc| mc.model_key == model_key);
            if model_called {
                if rec.passed {
                    pass_count += 1;
                }

                // User satisfaction: ratio of positive ratings.
                if let Some(ref rating) = rec.user_rating {
                    feedback_count += 1;
                    if rating == "thumbs_up" || rating == "accepted" {
                        positive_feedback += 1;
                    }
                }
            }
        }

        if total_requests == 0 {
            return Ok(ModelStats {
                model_key: model_key.to_string(),
                avg_quality: 0.0,
                avg_latency_ms: 0,
                avg_cost_usd: 0.0,
                total_requests: 0,
                pass_rate: 0.0,
                user_satisfaction: 0.0,
            });
        }

        let avg_quality = qualities.iter().sum::<f64>() / qualities.len() as f64;
        let avg_latency_ms = latencies.iter().sum::<u64>() / latencies.len() as u64;
        let avg_cost_usd = costs.iter().sum::<f64>() / costs.len() as f64;
        let pass_rate = pass_count as f64 / total_requests as f64;
        let user_satisfaction = if feedback_count > 0 {
            positive_feedback as f64 / feedback_count as f64
        } else {
            0.0
        };

        Ok(ModelStats {
            model_key: model_key.to_string(),
            avg_quality,
            avg_latency_ms,
            avg_cost_usd,
            total_requests,
            pass_rate,
            user_satisfaction,
        })
    }

    /// Calculate combined model weights for the StrategySelector.
    ///
    /// Formula (from design doc):
    /// ```text
    /// quality_weight  = avg_quality
    /// cost_weight     = 1.0 / (avg_cost * 10.0 + 1.0)
    /// latency_weight  = 1.0 / (avg_latency_ms / 1000.0 + 1.0)
    /// combined        = quality_weight * 0.5 + cost_weight * 0.3 + latency_weight * 0.2
    /// ```
    ///
    /// Models with no data are excluded from the result.
    pub fn model_weights(&self, task_type: &str) -> Result<HashMap<String, f64>, String> {
        let store = self.open_store()?;
        let records = store.query_records_since("1970-01-01T00:00:00Z")?;

        // Collect per-model aggregated values for the given task_type.
        let mut model_data: HashMap<String, (Vec<f64>, Vec<f64>, Vec<u64>)> = HashMap::new();

        for rec in &records {
            if rec.task_type != task_type {
                continue;
            }
            for mc in &rec.models_called {
                let entry = model_data.entry(mc.model_key.clone()).or_insert_with(|| {
                    (Vec::new(), Vec::new(), Vec::new())
                });
                entry.0.push(mc.quality_score);
                entry.1.push(mc.cost_usd);
                entry.2.push(mc.latency_ms);
            }
        }

        let mut weights = HashMap::new();
        for (key, (qualities, costs, latencies)) in &model_data {
            if qualities.is_empty() {
                continue;
            }

            let avg_quality = qualities.iter().sum::<f64>() / qualities.len() as f64;
            let avg_cost = costs.iter().sum::<f64>() / costs.len() as f64;
            let avg_latency_ms = latencies.iter().sum::<u64>() / latencies.len() as u64;

            let quality_weight = avg_quality;
            let cost_weight = 1.0 / (avg_cost * 10.0 + 1.0);
            let latency_weight = 1.0 / (avg_latency_ms as f64 / 1000.0 + 1.0);

            let combined = quality_weight * 0.5 + cost_weight * 0.3 + latency_weight * 0.2;
            weights.insert(key.clone(), combined);
        }

        Ok(weights)
    }

    /// Calculate the pass rate for a specific strategy within a time window.
    pub fn pass_rate(&self, strategy: &str, window: TimeWindow) -> Result<f64, String> {
        let store = self.open_store()?;
        let since = window.since_timestamp();
        let records = store.query_records_since(&since)?;

        let mut total: u32 = 0;
        let mut passed: u32 = 0;

        for rec in &records {
            if rec.strategy_used != strategy {
                continue;
            }
            total += 1;
            if rec.passed {
                passed += 1;
            }
        }

        if total == 0 {
            return Ok(0.0);
        }

        Ok(passed as f64 / total as f64)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orchestration::history::{
        ModelCall, OrchestrationRecord, QualityScore,
    };
    use tempfile::TempDir;

    fn db_path(dir: &TempDir) -> PathBuf {
        dir.path().join("stats_test.db")
    }

    fn make_engine(dir: &TempDir) -> StatsEngine {
        let path = db_path(dir);
        // Ensure DB is initialized.
        let _ = HistoryStore::new(&path).expect("init store");
        StatsEngine::new(path)
    }

    fn insert_sample_record(
        dir: &TempDir,
        task_type: &str,
        strategy: &str,
        model_key: &str,
        quality: f64,
        passed: bool,
        latency_ms: u64,
        cost_usd: f64,
        user_rating: Option<&str>,
    ) -> OrchestrationRecord {
        let path = db_path(dir);
        let store = HistoryStore::new(&path).expect("open store");

        let mut rec = OrchestrationRecord::new(
            task_type,
            0.5,
            "low",
            "test prompt",
            strategy,
        );
        rec.passed = passed;
        rec.final_quality = quality;
        rec.total_latency_ms = latency_ms;
        rec.total_cost_usd = cost_usd;
        rec.models_called = vec![ModelCall {
            model_key: model_key.to_string(),
            provider: "test".to_string(),
            latency_ms,
            cost_usd,
            quality_score: quality,
            was_selected: true,
        }];
        rec.quality_scores = vec![QualityScore {
            tool_name: "structural_check".to_string(),
            score: quality,
        }];
        rec.user_rating = user_rating.map(|s| s.to_string());

        store.record(&rec).expect("record should succeed");
        rec
    }

    // ---- Test: Model performance aggregation from multiple records ----

    #[test]
    fn model_performance_aggregates_multiple_records() {
        let dir = TempDir::new().unwrap();
        let _ = make_engine(&dir);

        insert_sample_record(&dir, "coding", "cascade", "claude-sonnet", 0.8, true, 1000, 0.01, None);
        insert_sample_record(&dir, "coding", "cascade", "claude-sonnet", 0.9, true, 1200, 0.02, None);
        insert_sample_record(&dir, "coding", "cascade", "claude-sonnet", 0.7, false, 800, 0.01, None);

        let engine = make_engine(&dir);
        let stats = engine
            .model_performance("claude-sonnet", Some("coding"), TimeWindow::Last30d)
            .unwrap();

        assert_eq!(stats.model_key, "claude-sonnet");
        assert_eq!(stats.total_requests, 3);
        assert!((stats.avg_quality - 0.8).abs() < 0.01, "avg_quality should be ~0.8, got {}", stats.avg_quality);
        assert_eq!(stats.avg_latency_ms, 1000);
        assert!((stats.avg_cost_usd - 0.01333).abs() < 0.001);
        assert!((stats.pass_rate - 0.6667).abs() < 0.01, "pass_rate should be ~0.667, got {}", stats.pass_rate);
    }

    // ---- Test: Model weights calculation ----

    #[test]
    fn model_weights_calculation() {
        let dir = TempDir::new().unwrap();
        let _ = make_engine(&dir);

        // Model A: high quality, higher cost, medium latency
        insert_sample_record(&dir, "coding", "cascade", "model-a", 0.9, true, 1000, 0.05, None);
        // Model B: medium quality, low cost, fast latency
        insert_sample_record(&dir, "coding", "cascade", "model-b", 0.7, true, 200, 0.001, None);

        let engine = make_engine(&dir);
        let weights = engine.model_weights("coding").unwrap();

        assert!(weights.contains_key("model-a"), "model-a should have a weight");
        assert!(weights.contains_key("model-b"), "model-b should have a weight");

        let weight_a = weights["model-a"];
        let weight_b = weights["model-b"];

        // Both weights should be positive
        assert!(weight_a > 0.0, "weight_a should be positive");
        assert!(weight_b > 0.0, "weight_b should be positive");

        // Manual verification for model-a:
        // quality_weight = 0.9
        // cost_weight = 1.0 / (0.05 * 10.0 + 1.0) = 1.0 / 1.5 = 0.6667
        // latency_weight = 1.0 / (1000.0 / 1000.0 + 1.0) = 0.5
        // combined = 0.9 * 0.5 + 0.6667 * 0.3 + 0.5 * 0.2 = 0.45 + 0.2 + 0.1 = 0.75
        let expected_a = 0.9 * 0.5 + (1.0 / 1.5) * 0.3 + 0.5 * 0.2;
        assert!(
            (weight_a - expected_a).abs() < 0.01,
            "model-a weight should be ~{}, got {}",
            expected_a,
            weight_a
        );
    }

    // ---- Test: Pass rate calculation ----

    #[test]
    fn pass_rate_calculation() {
        let dir = TempDir::new().unwrap();
        let _ = make_engine(&dir);

        insert_sample_record(&dir, "coding", "cascade", "model-a", 0.9, true, 1000, 0.01, None);
        insert_sample_record(&dir, "coding", "cascade", "model-a", 0.85, true, 1000, 0.01, None);
        insert_sample_record(&dir, "coding", "cascade", "model-a", 0.4, false, 1000, 0.01, None);
        insert_sample_record(&dir, "coding", "route", "model-b", 0.8, true, 500, 0.005, None);

        let engine = make_engine(&dir);

        let cascade_rate = engine.pass_rate("cascade", TimeWindow::Last30d).unwrap();
        assert!(
            (cascade_rate - 0.6667).abs() < 0.01,
            "cascade pass rate should be ~0.667, got {}",
            cascade_rate
        );

        let route_rate = engine.pass_rate("route", TimeWindow::Last30d).unwrap();
        assert!(
            (route_rate - 1.0).abs() < f64::EPSILON,
            "route pass rate should be 1.0, got {}",
            route_rate
        );
    }

    // ---- Test: Time window filtering works ----

    #[test]
    fn time_window_filtering() {
        let dir = TempDir::new().unwrap();
        let engine = make_engine(&dir);

        // Insert a current record.
        insert_sample_record(&dir, "coding", "cascade", "model-a", 0.9, true, 1000, 0.01, None);

        // Should be found in all windows.
        let stats_24h = engine
            .model_performance("model-a", None, TimeWindow::Last24h)
            .unwrap();
        assert!(
            stats_24h.total_requests >= 1,
            "Should find at least 1 request in 24h window"
        );

        let stats_7d = engine
            .model_performance("model-a", None, TimeWindow::Last7d)
            .unwrap();
        assert!(
            stats_7d.total_requests >= 1,
            "Should find at least 1 request in 7d window"
        );

        let stats_30d = engine
            .model_performance("model-a", None, TimeWindow::Last30d)
            .unwrap();
        assert!(
            stats_30d.total_requests >= 1,
            "Should find at least 1 request in 30d window"
        );
    }

    // ---- Test: No data returns zeroed stats ----

    #[test]
    fn no_data_returns_zeroed_stats() {
        let dir = TempDir::new().unwrap();
        let engine = make_engine(&dir);

        let stats = engine
            .model_performance("nonexistent-model", None, TimeWindow::Last24h)
            .unwrap();

        assert_eq!(stats.model_key, "nonexistent-model");
        assert_eq!(stats.total_requests, 0);
        assert_eq!(stats.avg_quality, 0.0);
        assert_eq!(stats.avg_latency_ms, 0);
        assert_eq!(stats.avg_cost_usd, 0.0);
        assert_eq!(stats.pass_rate, 0.0);
        assert_eq!(stats.user_satisfaction, 0.0);
    }

    #[test]
    fn pass_rate_no_data_returns_zero() {
        let dir = TempDir::new().unwrap();
        let engine = make_engine(&dir);

        let rate = engine.pass_rate("nonexistent-strategy", TimeWindow::Last30d).unwrap();
        assert_eq!(rate, 0.0, "No data should return 0.0 pass rate");
    }

    #[test]
    fn model_weights_no_data_returns_empty_map() {
        let dir = TempDir::new().unwrap();
        let engine = make_engine(&dir);

        let weights = engine.model_weights("nonexistent-task").unwrap();
        assert!(weights.is_empty(), "No data should return empty weights map");
    }

    // ---- Test: User satisfaction calculation ----

    #[test]
    fn user_satisfaction_calculation() {
        let dir = TempDir::new().unwrap();
        let _ = make_engine(&dir);

        insert_sample_record(&dir, "coding", "cascade", "model-a", 0.9, true, 1000, 0.01, Some("thumbs_up"));
        insert_sample_record(&dir, "coding", "cascade", "model-a", 0.85, true, 1000, 0.01, Some("accepted"));
        insert_sample_record(&dir, "coding", "cascade", "model-a", 0.4, false, 1000, 0.01, Some("thumbs_down"));
        insert_sample_record(&dir, "coding", "cascade", "model-a", 0.6, true, 1000, 0.01, None);

        let engine = make_engine(&dir);
        let stats = engine
            .model_performance("model-a", Some("coding"), TimeWindow::Last30d)
            .unwrap();

        // 2 positive (thumbs_up + accepted) out of 3 with feedback = 0.667
        assert!(
            (stats.user_satisfaction - 0.6667).abs() < 0.01,
            "user_satisfaction should be ~0.667, got {}",
            stats.user_satisfaction
        );
    }
}
