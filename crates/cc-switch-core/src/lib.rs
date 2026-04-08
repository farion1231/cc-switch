//! CC-Switch Core Library
//!
//! This is the shared core library used by both CLI and Tauri GUI.
//! Contains all business logic, database layer, and configuration management.

pub mod app_config;
pub mod codex_config;
pub mod config;
pub mod database;
pub mod deeplink;
pub mod error;
pub mod gemini_config;
pub mod mcp;
pub mod openclaw_config;
pub mod opencode_config;
pub mod prompt;
pub mod prompt_files;
pub mod provider;
pub mod proxy;
pub mod services;
pub mod settings;
pub mod store;
pub mod usage_script;

pub use app_config::{AppType, InstalledSkill, McpApps, McpServer, SkillApps, UnmanagedSkill};
pub use database::Database;
pub use deeplink::{
    import_mcp_from_deeplink, import_prompt_from_deeplink, import_provider_from_deeplink,
    import_skill_from_deeplink, parse_and_merge_config, parse_deeplink_url, DeepLinkImportRequest,
};
pub use error::AppError;
pub use openclaw_config::{
    OpenClawAgentsDefaults, OpenClawDefaultModel, OpenClawEnvConfig, OpenClawModelCatalogEntry,
    OpenClawProviderConfig, OpenClawToolsConfig,
};
pub use prompt::Prompt;
pub use provider::{Provider, UniversalProvider};
pub use proxy::{
    AppProxyConfig, CircuitBreakerConfig, CircuitBreakerStats, FailoverQueueItem,
    GlobalProxyConfig, LiveBackup, ProviderHealth, ProxyConfig, ProxyStatus, ProxyTakeoverStatus,
};
pub use services::auto_launch::AutoLaunchService;
pub use services::config::{DeeplinkImportResult, DeeplinkService};
pub use services::doctor::{
    DoctorAppSnapshot, DoctorPathStatus, DoctorReport, DoctorRuntimeSnapshot, DoctorService,
    DoctorSettingsSnapshot,
};
pub use services::global_proxy::{
    DetectedProxy, GlobalProxyService, ProxyTestResult, UpstreamProxyStatus,
};
pub use services::host::{HostPreferences, HostService};
pub use services::omo::{OmoLocalFileData, OmoService, OmoVariant, SLIM, STANDARD};
pub use services::plugin::ClaudePluginService;
pub use services::provider::{EndpointLatency, ProviderSortUpdate};
pub use services::runtime::{
    AppInfo, RuntimeService, ToolVersionInfo, UpdateInfo, WslShellPreference,
};
pub use services::session::{SessionMessage, SessionMeta, SessionService};
pub use services::settings::{SettingsSaveResult, SettingsService};
pub use services::skill::{
    migrate_skills_to_ssot, DiscoverableSkill, Skill, SkillRepo, SkillStore,
};
pub use services::stream_check::{
    HealthStatus, StreamCheckConfig, StreamCheckResult, StreamCheckService,
};
pub use services::usage::{
    ModelPricingInfo, PaginatedUsageLogs, ProviderLimitStatus, RequestLog, UsageLogDetail,
    UsageLogFilters, UsageModelStat, UsageProviderStat, UsageService, UsageSummary,
    UsageTrendPoint,
};
pub use services::webdav_sync::{check_connection as webdav_check_connection, fetch_remote_info};
pub use services::workspace::{DailyMemoryFileInfo, DailyMemorySearchResult, WorkspaceService};
pub use services::{
    ConfigService, McpService, PromptService, ProviderService, ProxyService, SkillService,
    SpeedtestService,
};
pub use settings::{AppSettings, WebDavSyncSettings, WebDavSyncStatus};
pub use store::AppState;
