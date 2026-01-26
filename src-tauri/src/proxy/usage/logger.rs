//! Usage Logger - 记录 API 请求使用情况

use super::calculator::{CostBreakdown, CostCalculator, ModelPricing};
use super::parser::TokenUsage;
use crate::database::Database;
use crate::error::AppError;
use crate::services::usage_stats::find_model_pricing_row;
use rust_decimal::Decimal;
use std::{str::FromStr, time::SystemTime};

/// 请求日志
#[derive(Debug, Clone)]
pub struct RequestLog {
    pub request_id: String,
    pub provider_id: String,
    pub app_type: String,
    pub model: String,
    pub request_model: String,
    pub usage: TokenUsage,
    pub cost: Option<CostBreakdown>,
    pub latency_ms: u64,
    pub first_token_ms: Option<u64>,
    pub status_code: u16,
    pub error_message: Option<String>,
    pub session_id: Option<String>,
    /// 供应商类型 (claude, claude_auth, codex, gemini, gemini_cli, openrouter)
    pub provider_type: Option<String>,
    /// 是否为流式请求
    pub is_streaming: bool,
    /// 成本倍数
    pub cost_multiplier: String,
}

/// 使用量记录器
pub struct UsageLogger<'a> {
    db: &'a Database,
}

impl<'a> UsageLogger<'a> {
    pub fn new(db: &'a Database) -> Self {
        Self { db }
    }

    /// 记录成功的请求
    pub fn log_request(&self, log: &RequestLog) -> Result<(), AppError> {
        let conn = crate::database::lock_conn!(self.db.conn);

        let (input_cost, output_cost, cache_read_cost, cache_creation_cost, total_cost) =
            if let Some(cost) = &log.cost {
                (
                    cost.input_cost.to_string(),
                    cost.output_cost.to_string(),
                    cost.cache_read_cost.to_string(),
                    cost.cache_creation_cost.to_string(),
                    cost.total_cost.to_string(),
                )
            } else {
                (
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                )
            };

        let created_at = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or_else(|e| {
                log::warn!("SystemTime is before UNIX_EPOCH, falling back to 0: {e}");
                0
            });

        conn.execute(
            "INSERT INTO proxy_request_logs (
                request_id, provider_id, app_type, model, request_model,
                input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                input_cost_usd, output_cost_usd, cache_read_cost_usd, cache_creation_cost_usd, total_cost_usd,
                latency_ms, first_token_ms, status_code, error_message, session_id,
                provider_type, is_streaming, cost_multiplier, created_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23)",
            rusqlite::params![
                log.request_id,
                log.provider_id,
                log.app_type,
                log.model,
                log.request_model,
                log.usage.input_tokens,
                log.usage.output_tokens,
                log.usage.cache_read_tokens,
                log.usage.cache_creation_tokens,
                input_cost,
                output_cost,
                cache_read_cost,
                cache_creation_cost,
                total_cost,
                log.latency_ms as i64,
                log.first_token_ms.map(|v| v as i64),
                log.status_code as i64,
                log.error_message,
                log.session_id,
                log.provider_type,
                log.is_streaming as i64,
                log.cost_multiplier,
                created_at,
            ],
        )
        .map_err(|e| AppError::Database(format!("记录请求日志失败: {e}")))?;

        Ok(())
    }

    /// 记录失败的请求
    ///
    /// 用于记录无法从上游获取 usage 信息的失败请求
    #[allow(dead_code, clippy::too_many_arguments)]
    pub fn log_error(
        &self,
        request_id: String,
        provider_id: String,
        app_type: String,
        model: String,
        status_code: u16,
        error_message: String,
        latency_ms: u64,
    ) -> Result<(), AppError> {
        let request_model = model.clone();
        let log = RequestLog {
            request_id,
            provider_id,
            app_type,
            model,
            request_model,
            usage: TokenUsage::default(),
            cost: None,
            latency_ms,
            first_token_ms: None,
            status_code,
            error_message: Some(error_message),
            session_id: None,
            provider_type: None,
            is_streaming: false,
            cost_multiplier: "1.0".to_string(),
        };

        self.log_request(&log)
    }

    /// 记录失败的请求（带更多上下文信息）
    ///
    /// 相比 log_error，这个方法接受更多参数以提供完整的请求上下文
    #[allow(clippy::too_many_arguments)]
    pub fn log_error_with_context(
        &self,
        request_id: String,
        provider_id: String,
        app_type: String,
        model: String,
        status_code: u16,
        error_message: String,
        latency_ms: u64,
        is_streaming: bool,
        session_id: Option<String>,
        provider_type: Option<String>,
    ) -> Result<(), AppError> {
        let request_model = model.clone();
        let log = RequestLog {
            request_id,
            provider_id,
            app_type,
            model,
            request_model,
            usage: TokenUsage::default(),
            cost: None,
            latency_ms,
            first_token_ms: None,
            status_code,
            error_message: Some(error_message),
            session_id,
            provider_type,
            is_streaming,
            cost_multiplier: "1.0".to_string(),
        };

        self.log_request(&log)
    }

    /// 获取模型定价
    pub fn get_model_pricing(&self, model_id: &str) -> Result<Option<ModelPricing>, AppError> {
        let conn = crate::database::lock_conn!(self.db.conn);
        let row = find_model_pricing_row(&conn, model_id)?;
        match row {
            Some((input, output, cache_read, cache_creation)) => {
                ModelPricing::from_strings(&input, &output, &cache_read, &cache_creation)
                    .map(Some)
                    .map_err(|e| AppError::Database(format!("解析定价数据失败: {e}")))
            }
            None => Ok(None),
        }
    }

    /// 获取有效的倍率与计费模式来源（供应商优先，未配置则回退全局默认）
    pub async fn resolve_pricing_config(
        &self,
        provider_id: &str,
        app_type: &str,
    ) -> (Decimal, String) {
        let default_multiplier_raw = match self.db.get_default_cost_multiplier(app_type).await {
            Ok(value) => value,
            Err(e) => {
                log::warn!("[USG-003] 获取默认倍率失败 (app_type={app_type}): {e}");
                "1".to_string()
            }
        };
        let default_multiplier = match Decimal::from_str(&default_multiplier_raw) {
            Ok(value) => value,
            Err(e) => {
                log::warn!(
                    "[USG-003] 默认倍率解析失败 (app_type={app_type}): {default_multiplier_raw} - {e}"
                );
                Decimal::from(1)
            }
        };

        let default_pricing_source_raw = match self.db.get_pricing_model_source(app_type).await {
            Ok(value) => value,
            Err(e) => {
                log::warn!("[USG-003] 获取默认计费模式失败 (app_type={app_type}): {e}");
                "response".to_string()
            }
        };
        let default_pricing_source =
            if matches!(default_pricing_source_raw.as_str(), "response" | "request") {
                default_pricing_source_raw
            } else {
                log::warn!(
                "[USG-003] 默认计费模式无效 (app_type={app_type}): {default_pricing_source_raw}"
            );
                "response".to_string()
            };

        let provider = self
            .db
            .get_provider_by_id(provider_id, app_type)
            .ok()
            .flatten();

        let (provider_multiplier, provider_pricing_source) = provider
            .as_ref()
            .and_then(|p| p.meta.as_ref())
            .map(|meta| {
                (
                    meta.cost_multiplier.as_deref(),
                    meta.pricing_model_source.as_deref(),
                )
            })
            .unwrap_or((None, None));

        let cost_multiplier = match provider_multiplier {
            Some(value) => match Decimal::from_str(value) {
                Ok(parsed) => parsed,
                Err(e) => {
                    log::warn!(
                        "[USG-003] 供应商倍率解析失败 (provider_id={provider_id}): {value} - {e}"
                    );
                    default_multiplier
                }
            },
            None => default_multiplier,
        };

        let pricing_model_source = match provider_pricing_source {
            Some(value) if matches!(value, "response" | "request") => value.to_string(),
            Some(value) => {
                log::warn!("[USG-003] 供应商计费模式无效 (provider_id={provider_id}): {value}");
                default_pricing_source.clone()
            }
            None => default_pricing_source.clone(),
        };

        (cost_multiplier, pricing_model_source)
    }

    /// 计算并记录请求
    #[allow(clippy::too_many_arguments)]
    pub fn log_with_calculation(
        &self,
        request_id: String,
        provider_id: String,
        app_type: String,
        model: String,
        request_model: String,
        pricing_model: String,
        usage: TokenUsage,
        cost_multiplier: Decimal,
        latency_ms: u64,
        first_token_ms: Option<u64>,
        status_code: u16,
        session_id: Option<String>,
        provider_type: Option<String>,
        is_streaming: bool,
    ) -> Result<(), AppError> {
        let pricing = self.get_model_pricing(&pricing_model)?;

        if pricing.is_none() {
            log::warn!("[USG-002] 模型定价未找到，成本将记录为 0: {pricing_model}");
        }

        let cost = CostCalculator::try_calculate(&usage, pricing.as_ref(), cost_multiplier);

        let log = RequestLog {
            request_id,
            provider_id,
            app_type,
            model,
            request_model,
            usage,
            cost,
            latency_ms,
            first_token_ms,
            status_code,
            error_message: None,
            session_id,
            provider_type,
            is_streaming,
            cost_multiplier: cost_multiplier.to_string(),
        };

        self.log_request(&log)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_log_request() -> Result<(), AppError> {
        let db = Database::memory()?;

        // 插入测试定价
        {
            let conn = crate::database::lock_conn!(db.conn);
            conn.execute(
                "INSERT INTO model_pricing (model_id, display_name, input_cost_per_million, output_cost_per_million)
                 VALUES ('test-model', 'Test Model', '3.0', '15.0')",
                [],
            )
            .unwrap();
        }

        let logger = UsageLogger::new(&db);

        let usage = TokenUsage {
            input_tokens: 1000,
            output_tokens: 500,
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
            model: None,
        };

        logger.log_with_calculation(
            "req-123".to_string(),
            "provider-1".to_string(),
            "claude".to_string(),
            "test-model".to_string(),
            "req-model".to_string(),
            "test-model".to_string(),
            usage,
            Decimal::from(1),
            100,
            None,
            200,
            None,
            Some("claude".to_string()),
            false,
        )?;

        // 验证记录已插入
        let conn = crate::database::lock_conn!(db.conn);
        let (count, request_model): (i64, String) = conn
            .query_row(
                "SELECT COUNT(*), request_model FROM proxy_request_logs WHERE request_id = 'req-123'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(count, 1);
        assert_eq!(request_model, "req-model");
        Ok(())
    }

    #[test]
    fn test_log_error() -> Result<(), AppError> {
        let db = Database::memory()?;
        let logger = UsageLogger::new(&db);

        logger.log_error(
            "req-error".to_string(),
            "provider-1".to_string(),
            "claude".to_string(),
            "unknown-model".to_string(),
            500,
            "Internal Server Error".to_string(),
            50,
        )?;

        // 验证错误记录已插入
        let conn = crate::database::lock_conn!(db.conn);
        let (status, error): (i64, Option<String>) = conn
            .query_row(
                "SELECT status_code, error_message FROM proxy_request_logs WHERE request_id = 'req-error'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(status, 500);
        assert_eq!(error, Some("Internal Server Error".to_string()));
        Ok(())
    }
}
