//! CC-Switch Core Library
//!
//! This is the shared core library used by both CLI and Tauri GUI.
//! Contains all business logic, database layer, and configuration management.

pub mod app_config;
pub mod codex_config;
pub mod config;
pub mod database;
pub mod error;
pub mod gemini_config;
pub mod mcp;
pub mod openclaw_config;
pub mod opencode_config;
pub mod prompt;
pub mod provider;
pub mod prompt_files;
pub mod proxy;
pub mod services;
pub mod settings;
pub mod store;

pub use app_config::{AppType, InstalledSkill, McpApps, McpServer, SkillApps};
pub use database::Database;
pub use error::AppError;
pub use prompt::Prompt;
pub use provider::{Provider, UniversalProvider};
pub use services::config::{DeeplinkImportResult, DeeplinkService};
pub use services::omo::{OmoLocalFileData, OmoService, OmoVariant, SLIM, STANDARD};
pub use services::provider::{EndpointLatency, ProviderSortUpdate};
pub use services::proxy::{
    CircuitBreakerConfig, FailoverQueueItem, ProviderHealth, ProxyConfig, ProxyStatus,
    ProxyTakeoverStatus, RequestLog, UsageStatsService, UsageSummary,
};
pub use services::usage::{
    PaginatedUsageLogs, UsageLogDetail, UsageLogFilters, UsageModelStat, UsageProviderStat,
    UsageService, UsageTrendPoint,
};
pub use services::{
    ConfigService, McpService, PromptService, ProviderService, ProxyService, SkillService,
    SpeedtestService,
};
pub use settings::AppSettings;
pub use store::AppState;
