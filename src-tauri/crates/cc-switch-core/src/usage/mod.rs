mod model;
mod query;
mod sql;

use rusqlite::Connection;
use serde::Deserialize;
use serde_json::Value;

use crate::dispatch::CommandError;
use crate::{CoreError, HeadlessState};

pub use model::*;
pub use sql::{
    fresh_input_sql, is_cache_inclusive_app, CACHE_INCLUSIVE_APP_TYPES,
    INPUT_TOKEN_SEMANTICS_FRESH, INPUT_TOKEN_SEMANTICS_LEGACY, INPUT_TOKEN_SEMANTICS_TOTAL,
};

/// 无界面 Usage 查询门面；所有方法只访问传入状态绑定的目标数据库。
pub struct UsageService;

/// 统一 Agent 状态与桌面已加锁连接的只读入口；实现只负责连接生命周期，不承载 Usage 业务语义。
pub trait UsageQueryConnection {
    fn with_usage_connection<T>(
        &self,
        operation: impl FnOnce(&Connection) -> Result<T, CoreError>,
    ) -> Result<T, CoreError>;
}

impl UsageQueryConnection for HeadlessState {
    fn with_usage_connection<T>(
        &self,
        operation: impl FnOnce(&Connection) -> Result<T, CoreError>,
    ) -> Result<T, CoreError> {
        self.with_connection(operation)
    }
}

impl UsageQueryConnection for Connection {
    fn with_usage_connection<T>(
        &self,
        operation: impl FnOnce(&Connection) -> Result<T, CoreError>,
    ) -> Result<T, CoreError> {
        operation(self)
    }
}

impl UsageService {
    pub fn summary(
        source: &impl UsageQueryConnection,
        scope: UsageScope,
    ) -> Result<UsageSummary, CoreError> {
        query::summary(source, &scope)
    }

    pub fn summary_by_app(
        source: &impl UsageQueryConnection,
        scope: UsageScope,
    ) -> Result<Vec<UsageSummaryByApp>, CoreError> {
        query::summary_by_app(source, &scope)
    }

    pub fn trends(
        source: &impl UsageQueryConnection,
        scope: UsageScope,
    ) -> Result<Vec<DailyStats>, CoreError> {
        query::trends(source, &scope)
    }

    pub fn provider_stats(
        source: &impl UsageQueryConnection,
        scope: UsageScope,
    ) -> Result<Vec<ProviderStats>, CoreError> {
        query::provider_stats(source, &scope)
    }

    pub fn model_stats(
        source: &impl UsageQueryConnection,
        scope: UsageScope,
    ) -> Result<Vec<ModelStats>, CoreError> {
        query::model_stats(source, &scope)
    }

    pub fn logs(
        source: &impl UsageQueryConnection,
        filters: LogFilters,
        page: u32,
        page_size: u32,
    ) -> Result<PaginatedLogs, CoreError> {
        query::logs(source, &filters, page, page_size)
    }

    pub fn detail(
        source: &impl UsageQueryConnection,
        request_id: &str,
    ) -> Result<Option<RequestLogDetail>, CoreError> {
        query::detail(source, request_id)
    }

    pub fn data_sources(
        source: &impl UsageQueryConnection,
    ) -> Result<Vec<DataSourceSummary>, CoreError> {
        query::data_sources(source)
    }

    pub fn pricing(source: &impl UsageQueryConnection) -> Result<Vec<ModelPricingInfo>, CoreError> {
        query::pricing(source)
    }
}

/// 将稳定 Usage RPC 名映射到共享只读服务；未迁移的写命令保持显式能力错误。
pub(crate) fn dispatch(
    state: &HeadlessState,
    command: &str,
    args: Value,
) -> Result<Value, CommandError> {
    match command {
        "usage.summary" => {
            let scope = serde_json::from_value::<UsageScope>(args)
                .map_err(|error| CommandError::InvalidArgument(error.to_string()))?;
            serialize(UsageService::summary(state, scope)?)
        }
        "usage.summary_by_app" => {
            let scope = serde_json::from_value::<UsageScope>(args)
                .map_err(|error| CommandError::InvalidArgument(error.to_string()))?;
            serialize(UsageService::summary_by_app(state, scope)?)
        }
        "usage.trends" => {
            let scope = serde_json::from_value::<UsageScope>(args)
                .map_err(|error| CommandError::InvalidArgument(error.to_string()))?;
            serialize(UsageService::trends(state, scope)?)
        }
        "usage.provider_stats" => {
            let scope = serde_json::from_value::<UsageScope>(args)
                .map_err(|error| CommandError::InvalidArgument(error.to_string()))?;
            serialize(UsageService::provider_stats(state, scope)?)
        }
        "usage.model_stats" => {
            let scope = serde_json::from_value::<UsageScope>(args)
                .map_err(|error| CommandError::InvalidArgument(error.to_string()))?;
            serialize(UsageService::model_stats(state, scope)?)
        }
        "usage.logs" => {
            let args: LogsArgs = serde_json::from_value(args)
                .map_err(|error| CommandError::InvalidArgument(error.to_string()))?;
            serialize(UsageService::logs(
                state,
                args.filters,
                args.page,
                args.page_size,
            )?)
        }
        "usage.detail" => {
            let args: RequestArgs = serde_json::from_value(args)
                .map_err(|error| CommandError::InvalidArgument(error.to_string()))?;
            serialize(UsageService::detail(state, &args.request_id)?)
        }
        "usage.data_sources" => serialize(UsageService::data_sources(state)?),
        "usage.pricing.list" => serialize(UsageService::pricing(state)?),
        _ => Err(CommandError::CapabilityUnavailable(command.to_string())),
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LogsArgs {
    #[serde(default)]
    filters: LogFilters,
    #[serde(default)]
    page: u32,
    #[serde(default = "default_page_size")]
    page_size: u32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RequestArgs {
    request_id: String,
}

fn default_page_size() -> u32 {
    20
}

fn serialize(value: impl serde::Serialize) -> Result<Value, CommandError> {
    serde_json::to_value(value).map_err(CommandError::Serialization)
}
