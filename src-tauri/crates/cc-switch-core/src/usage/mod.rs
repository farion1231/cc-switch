mod model;
mod mutation;
mod pricing_file;
mod query;
mod script;
mod session;
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
    pub fn update_pricing(state: &HeadlessState, input: PricingUpdate) -> Result<(), CoreError> {
        pricing_file::update_pricing(state, input).map(|_| ())
    }

    pub fn update_pricing_batch(
        state: &HeadlessState,
        entries: Vec<ModelPricingInfo>,
    ) -> Result<usize, CoreError> {
        pricing_file::update_pricing_batch(state, entries)
    }

    /// 桌面适配层已持有数据库锁时直接复用该连接，避免嵌套锁和第二条 SQLite 连接。
    pub fn update_pricing_on_connection(
        connection: &Connection,
        input: PricingUpdate,
    ) -> Result<(), CoreError> {
        mutation::update_pricing(connection, input)
    }

    pub fn delete_pricing(state: &HeadlessState, model_id: &str) -> Result<(), CoreError> {
        pricing_file::delete_pricing(state, model_id)
    }

    pub fn delete_pricing_on_connection(
        connection: &Connection,
        model_id: &str,
    ) -> Result<(), CoreError> {
        mutation::delete_pricing(connection, model_id)
    }

    pub fn limits(
        state: &HeadlessState,
        provider_id: &str,
        app_type: &str,
    ) -> Result<ProviderLimitStatus, CoreError> {
        state.with_connection(|connection| mutation::limits(connection, provider_id, app_type))
    }

    pub fn limits_on_connection(
        connection: &Connection,
        provider_id: &str,
        app_type: &str,
    ) -> Result<ProviderLimitStatus, CoreError> {
        mutation::limits(connection, provider_id, app_type)
    }

    pub fn provider_query(
        state: &HeadlessState,
        input: ProviderUsageInput,
    ) -> Result<UsageResult, CoreError> {
        state.with_connection(|connection| script::provider_query(connection, input))
    }

    pub fn provider_query_on_connection(
        connection: &Connection,
        input: ProviderUsageInput,
    ) -> Result<UsageResult, CoreError> {
        script::provider_query(connection, input)
    }

    pub fn provider_test(
        state: &HeadlessState,
        input: ProviderUsageTestInput,
    ) -> Result<UsageResult, CoreError> {
        state.with_connection(|connection| script::provider_test(connection, input))
    }

    pub fn provider_test_on_connection(
        connection: &Connection,
        input: ProviderUsageTestInput,
    ) -> Result<UsageResult, CoreError> {
        script::provider_test(connection, input)
    }

    pub fn sync_sessions(
        state: &HeadlessState,
        cancellation: &OperationCancellation,
    ) -> Result<SessionSyncResult, CoreError> {
        mutation::sync_sessions(state, cancellation)
    }

    pub fn rebuild_codex(
        state: &HeadlessState,
        cancellation: &OperationCancellation,
    ) -> Result<SessionSyncResult, CoreError> {
        mutation::rebuild_codex(state, cancellation)
    }

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

    pub fn models_dev_sync_state(state: &HeadlessState) -> Result<ModelsDevSyncState, CoreError> {
        pricing_file::models_dev_sync_state(state)
    }

    pub fn save_models_dev_sync_config(
        state: &HeadlessState,
        config: ModelsDevSyncConfig,
    ) -> Result<(), CoreError> {
        pricing_file::save_models_dev_sync_config(state, config)
    }

    pub fn record_models_dev_sync_result(
        state: &HeadlessState,
        synced_at: Option<i64>,
        error: Option<String>,
    ) -> Result<(), CoreError> {
        pricing_file::record_models_dev_sync_result(state, synced_at, error)
    }
}

/// 将稳定 Usage RPC 名映射到共享只读服务；未迁移的写命令保持显式能力错误。
pub(crate) fn dispatch(
    state: &HeadlessState,
    command: &str,
    args: Value,
    cancellation: &OperationCancellation,
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
        "usage.pricing.list" => {
            pricing_file::sync_to_database(state)?;
            serialize(UsageService::pricing(state)?)
        }
        "usage.provider_query" => {
            let input = serde_json::from_value::<ProviderUsageInput>(args)
                .map_err(|error| CommandError::InvalidArgument(error.to_string()))?;
            serialize(UsageService::provider_query(state, input)?)
        }
        "usage.provider_test" => {
            let input = serde_json::from_value::<ProviderUsageTestInput>(args)
                .map_err(|error| CommandError::InvalidArgument(error.to_string()))?;
            serialize(UsageService::provider_test(state, input)?)
        }
        "usage.pricing.update" => {
            let input = serde_json::from_value::<PricingUpdate>(args)
                .map_err(|error| CommandError::InvalidArgument(error.to_string()))?;
            UsageService::update_pricing(state, input)?;
            Ok(Value::Bool(true))
        }
        "usage.pricing.update_batch" => {
            let args = serde_json::from_value::<PricingBatchArgs>(args)
                .map_err(|error| CommandError::InvalidArgument(error.to_string()))?;
            serialize(UsageService::update_pricing_batch(state, args.entries)?)
        }
        "usage.pricing.delete" => {
            let args = serde_json::from_value::<ModelArgs>(args)
                .map_err(|error| CommandError::InvalidArgument(error.to_string()))?;
            UsageService::delete_pricing(state, &args.model_id)?;
            Ok(Value::Bool(true))
        }
        "usage.models_dev_sync.get" => serialize(UsageService::models_dev_sync_state(state)?),
        "usage.models_dev_sync.save" => {
            let args = serde_json::from_value::<ModelsDevConfigArgs>(args)
                .map_err(|error| CommandError::InvalidArgument(error.to_string()))?;
            UsageService::save_models_dev_sync_config(state, args.config)?;
            Ok(Value::Bool(true))
        }
        "usage.models_dev_sync.record" => {
            let args = serde_json::from_value::<ModelsDevResultArgs>(args)
                .map_err(|error| CommandError::InvalidArgument(error.to_string()))?;
            UsageService::record_models_dev_sync_result(state, args.synced_at, args.error)?;
            Ok(Value::Bool(true))
        }
        "usage.limits" => {
            let args = serde_json::from_value::<LimitArgs>(args)
                .map_err(|error| CommandError::InvalidArgument(error.to_string()))?;
            serialize(UsageService::limits(
                state,
                &args.provider_id,
                &args.app_type,
            )?)
        }
        "usage.session_sync" => serialize(UsageService::sync_sessions(state, cancellation)?),
        "usage.codex_rebuild" => {
            // 重建顺序和取消检查统一封装在 Core，分发层不得自行拆开 backup/reset/import，
            // 否则远程断线时可能留下无法解释的半完成状态。
            serialize(UsageService::rebuild_codex(state, cancellation)?)
        }
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

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ModelArgs {
    model_id: String,
}

#[derive(Debug, Deserialize)]
struct PricingBatchArgs {
    entries: Vec<ModelPricingInfo>,
}

#[derive(Debug, Deserialize)]
struct ModelsDevConfigArgs {
    config: ModelsDevSyncConfig,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ModelsDevResultArgs {
    synced_at: Option<i64>,
    error: Option<String>,
}

/// 同时接受前端沿用的 appType；远端协议始终在 Core 内转换为数据库 app_type。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LimitArgs {
    provider_id: String,
    app_type: String,
}

fn default_page_size() -> u32 {
    20
}

fn serialize(value: impl serde::Serialize) -> Result<Value, CommandError> {
    serde_json::to_value(value).map_err(CommandError::Serialization)
}
