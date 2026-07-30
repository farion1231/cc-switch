//! 可被桌面端与临时 Agent 共同使用的无界面业务核心。
//!
//! 该边界禁止依赖 Tauri、托盘、窗口和代理服务器生命周期。所有目标相关路径、数据库连接
//! 与平台能力都必须由调用方显式传入，避免远程请求静默操作桌面宿主机。

mod dispatch;
mod error;
mod provider;
mod schema;
mod state;
mod usage;

pub use dispatch::{dispatch_command, CommandError};
pub use error::CoreError;
// Provider 的完整 DTO 与数据库服务统一从独立模块导出，RPC 与桌面适配层不得再维护私有 SQL。
pub use provider::{project_provider, LiveContext, SwitchResult, TargetPlatform};
pub use provider::{ProviderRecord, ProviderService, ProviderSortUpdate};
pub use schema::{SchemaError, DESKTOP_SCHEMA_VERSION};
pub use state::HeadlessState;
pub use usage::{
    fresh_input_sql, is_cache_inclusive_app, DailyStats, DataSourceSummary, LogFilters,
    ModelPricingInfo, ModelStats, OperationCancellation, PaginatedLogs, PricingUpdate,
    ProviderLimitStatus, ProviderStats, ProviderUsageInput, ProviderUsageTestInput,
    RequestLogDetail, SessionSyncResult, UsageData, UsageQueryConnection, UsageResult, UsageScope,
    UsageService, UsageSummary, UsageSummaryByApp, CACHE_INCLUSIVE_APP_TYPES,
    INPUT_TOKEN_SEMANTICS_FRESH, INPUT_TOKEN_SEMANTICS_LEGACY, INPUT_TOKEN_SEMANTICS_TOTAL,
};

/// 协议握手的兼容导出；其值代表桌面规范数据库版本，不再维护 Agent 私有版本。
pub const SCHEMA_VERSION: i32 = DESKTOP_SCHEMA_VERSION;
