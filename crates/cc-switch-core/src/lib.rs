//! CC-Switch Core Library
//!
//! This is the shared core library used by both CLI and Tauri GUI.
//! Contains all business logic, database layer, and configuration management.

pub mod app_config;
pub mod config;
pub mod database;
pub mod error;
pub mod mcp;
pub mod prompt;
pub mod provider;
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
pub use services::provider::{EndpointLatency, ProviderSortUpdate};
pub use services::proxy::{
    CircuitBreakerConfig, FailoverQueueItem, ProviderHealth, ProxyConfig, ProxyStatus,
    ProxyTakeoverStatus, RequestLog, UsageStatsService, UsageSummary,
};
pub use services::{
    ConfigService, McpService, PromptService, ProviderService, ProxyService, SkillService,
};
pub use settings::AppSettings;
pub use store::AppState;
