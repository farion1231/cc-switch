//! Usage analytics service.

use serde::{Deserialize, Serialize};

use crate::database::Database;
use crate::error::AppError;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageTrendPoint {
    pub date: String,
    pub request_count: u64,
    pub total_cost: String,
    pub total_tokens: u64,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub total_cache_creation_tokens: u64,
    pub total_cache_read_tokens: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageProviderStat {
    pub provider_id: String,
    pub provider_name: String,
    pub request_count: u64,
    pub total_tokens: u64,
    pub total_cost: String,
    pub success_rate: f32,
    pub avg_latency_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageModelStat {
    pub model: String,
    pub request_count: u64,
    pub total_tokens: u64,
    pub total_cost: String,
    pub avg_cost_per_request: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageLogFilters {
    pub app_type: Option<String>,
    pub provider_name: Option<String>,
    pub model: Option<String>,
    pub status_code: Option<u16>,
    pub start_date: Option<i64>,
    pub end_date: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageLogDetail {
    pub request_id: String,
    pub provider_id: String,
    pub provider_name: Option<String>,
    pub app_type: String,
    pub model: String,
    pub request_model: Option<String>,
    pub cost_multiplier: String,
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub cache_read_tokens: u32,
    pub cache_creation_tokens: u32,
    pub input_cost_usd: String,
    pub output_cost_usd: String,
    pub cache_read_cost_usd: String,
    pub cache_creation_cost_usd: String,
    pub total_cost_usd: String,
    pub is_streaming: bool,
    pub latency_ms: u64,
    pub first_token_ms: Option<u64>,
    pub duration_ms: Option<u64>,
    pub status_code: u16,
    pub error_message: Option<String>,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaginatedUsageLogs {
    pub data: Vec<UsageLogDetail>,
    pub total: u32,
    pub page: u32,
    pub page_size: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelPricingInfo {
    pub model_id: String,
    pub display_name: String,
    pub input_cost_per_million: String,
    pub output_cost_per_million: String,
    pub cache_read_cost_per_million: String,
    pub cache_creation_cost_per_million: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderLimitStatus {
    pub provider_id: String,
    pub daily_usage: String,
    pub daily_limit: Option<String>,
    pub daily_exceeded: bool,
    pub monthly_usage: String,
    pub monthly_limit: Option<String>,
    pub monthly_exceeded: bool,
}

pub struct UsageService;

impl UsageService {
    pub fn get_trends(
        db: &Database,
        start_date: Option<i64>,
        end_date: Option<i64>,
    ) -> Result<Vec<UsageTrendPoint>, AppError> {
        db.get_usage_trends(start_date, end_date)
    }

    pub fn get_provider_stats(db: &Database) -> Result<Vec<UsageProviderStat>, AppError> {
        db.get_usage_provider_stats()
    }

    pub fn get_model_stats(db: &Database) -> Result<Vec<UsageModelStat>, AppError> {
        db.get_usage_model_stats()
    }

    pub fn get_logs(
        db: &Database,
        filters: &UsageLogFilters,
        page: u32,
        page_size: u32,
    ) -> Result<PaginatedUsageLogs, AppError> {
        db.get_usage_log_details(filters, page, page_size)
    }

    pub fn get_request_detail(
        db: &Database,
        request_id: &str,
    ) -> Result<Option<UsageLogDetail>, AppError> {
        db.get_usage_request_detail(request_id)
    }

    pub fn get_model_pricing(db: &Database) -> Result<Vec<ModelPricingInfo>, AppError> {
        db.get_model_pricing()
    }

    pub fn update_model_pricing(db: &Database, pricing: ModelPricingInfo) -> Result<(), AppError> {
        db.upsert_model_pricing(&pricing)
    }

    pub fn delete_model_pricing(db: &Database, model_id: &str) -> Result<(), AppError> {
        db.delete_model_pricing(model_id)
    }

    pub fn check_provider_limits(
        db: &Database,
        provider_id: &str,
        app_type: &str,
    ) -> Result<ProviderLimitStatus, AppError> {
        db.check_provider_limits(provider_id, app_type)
    }
}
