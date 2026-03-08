//! CLI Command Definitions using clap

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "cc-switch")]
#[command(about = "CLI for managing AI coding agent providers", long_about = None)]
#[command(version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,

    /// Output format
    #[arg(short, long, global = true, value_enum, default_value = "table")]
    pub format: OutputFormat,

    /// Quiet mode, only output errors
    #[arg(short, long, global = true)]
    pub quiet: bool,

    /// Verbose output
    #[arg(short, long, global = true)]
    pub verbose: bool,
}

#[derive(Clone, Copy, Debug, clap::ValueEnum)]
pub enum OutputFormat {
    Table,
    Json,
    Yaml,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Manage API providers
    Provider {
        #[command(subcommand)]
        cmd: ProviderCommands,
    },
    /// Manage MCP servers
    Mcp {
        #[command(subcommand)]
        cmd: McpCommands,
    },
    /// Proxy server control
    Proxy {
        #[command(subcommand)]
        cmd: ProxyCommands,
    },
    /// Manage prompts
    Prompt {
        #[command(subcommand)]
        cmd: PromptCommands,
    },
    /// Manage skills
    Skill {
        #[command(subcommand)]
        cmd: SkillCommands,
    },
    /// Configuration management
    Config {
        #[command(subcommand)]
        cmd: ConfigCommands,
    },
    /// Usage statistics
    Usage {
        #[command(subcommand)]
        cmd: UsageCommands,
    },
    /// Deep link tooling
    Deeplink {
        #[command(subcommand)]
        cmd: DeeplinkCommands,
    },
    /// Import/Export operations
    Export {
        /// Output file path
        #[arg(short, long)]
        output: String,
    },
    Import {
        /// Input file path
        #[arg(short, long)]
        input: String,
        /// Merge with existing config instead of replacing
        #[arg(long)]
        merge: bool,
    },
    ImportDeeplink {
        /// Deeplink URL
        url: String,
    },
    #[command(name = "__e2e-session", hide = true)]
    E2eSession,
}

#[derive(Subcommand)]
pub enum ProviderCommands {
    /// List all providers
    List {
        /// App type (claude, codex, gemini, opencode)
        #[arg(short, long, default_value = "claude")]
        app: String,
    },
    /// Show provider details
    Show {
        /// Provider ID
        id: String,
        /// App type
        #[arg(short, long, default_value = "claude")]
        app: String,
    },
    /// Add a new provider
    Add {
        /// App type
        #[arg(short, long, default_value = "claude")]
        app: String,
        /// Provider name
        #[arg(short, long)]
        name: Option<String>,
        /// Base URL
        #[arg(short = 'u', long)]
        base_url: Option<String>,
        /// API key
        #[arg(short = 'k', long)]
        api_key: Option<String>,
        /// Import from JSON file
        #[arg(long)]
        from_json: Option<String>,
    },
    /// Edit a provider
    Edit {
        /// Provider ID
        id: String,
        /// App type
        #[arg(short, long, default_value = "claude")]
        app: String,
        /// Set new API key
        #[arg(long)]
        set_api_key: Option<String>,
        /// Set new base URL
        #[arg(long)]
        set_base_url: Option<String>,
        /// Set new name
        #[arg(long)]
        set_name: Option<String>,
    },
    /// Delete a provider
    Delete {
        /// Provider ID
        id: String,
        /// App type
        #[arg(short, long, default_value = "claude")]
        app: String,
        /// Skip confirmation
        #[arg(short, long)]
        yes: bool,
    },
    /// Duplicate an existing provider
    Duplicate {
        /// Source provider ID
        id: String,
        /// App type
        #[arg(short, long, default_value = "claude")]
        app: String,
        /// New provider name
        #[arg(short, long)]
        name: Option<String>,
        /// New provider ID
        #[arg(long)]
        new_id: Option<String>,
    },
    /// Switch current provider
    Switch {
        /// Provider ID
        id: String,
        /// App type
        #[arg(short, long, default_value = "claude")]
        app: String,
    },
    /// Show the current live config for an app
    ReadLive {
        /// App type
        #[arg(short, long, default_value = "claude")]
        app: String,
    },
    /// Import provider config from current live files
    ImportLive {
        /// App type
        #[arg(short, long, default_value = "claude")]
        app: String,
    },
    /// Remove an additive-mode provider from live config without deleting the DB record
    RemoveFromLive {
        /// Provider ID
        id: String,
        /// App type
        #[arg(short, long, default_value = "opencode")]
        app: String,
    },
    /// Update a provider sort order index
    SortOrder {
        /// Provider ID
        id: String,
        /// App type
        #[arg(short, long, default_value = "claude")]
        app: String,
        /// New sort index
        #[arg(short, long)]
        index: usize,
    },
    /// Query provider usage
    Usage {
        /// Provider ID
        id: String,
        /// App type
        #[arg(short, long, default_value = "claude")]
        app: String,
    },
    /// Manage provider endpoints
    Endpoint {
        #[command(subcommand)]
        cmd: ProviderEndpointCommands,
    },
    /// Manage provider common config snippets
    #[command(name = "common-config-snippet")]
    CommonConfigSnippet {
        #[command(subcommand)]
        cmd: ProviderCommonConfigSnippetCommands,
    },
    /// Manage provider usage scripts
    #[command(name = "usage-script")]
    UsageScript {
        #[command(subcommand)]
        cmd: ProviderUsageScriptCommands,
    },
    /// Run provider stream checks
    #[command(name = "stream-check")]
    StreamCheck {
        #[command(subcommand)]
        cmd: ProviderStreamCheckCommands,
    },
    /// Universal provider management (cross-app)
    #[command(subcommand)]
    Universal(UniversalProviderCommands),
}

#[derive(Subcommand)]
pub enum ProviderEndpointCommands {
    /// List custom endpoints for a provider
    List {
        /// Provider ID
        id: String,
        /// App type
        #[arg(short, long, default_value = "claude")]
        app: String,
    },
    /// Add a custom endpoint for a provider
    Add {
        /// Provider ID
        id: String,
        /// App type
        #[arg(short, long, default_value = "claude")]
        app: String,
        /// Endpoint URL
        #[arg(short = 'u', long)]
        url: String,
    },
    /// Remove a custom endpoint from a provider
    Remove {
        /// Provider ID
        id: String,
        /// App type
        #[arg(short, long, default_value = "claude")]
        app: String,
        /// Endpoint URL
        #[arg(short = 'u', long)]
        url: String,
    },
    /// Mark an endpoint as last used for a provider
    MarkUsed {
        /// Provider ID
        id: String,
        /// App type
        #[arg(short, long, default_value = "claude")]
        app: String,
        /// Endpoint URL
        #[arg(short = 'u', long)]
        url: String,
    },
    /// Run latency checks for the provider primary endpoint and custom endpoints
    Speedtest {
        /// Provider ID
        id: String,
        /// App type
        #[arg(short, long, default_value = "claude")]
        app: String,
        /// Timeout in seconds
        #[arg(long)]
        timeout: Option<u64>,
    },
}

#[derive(Subcommand)]
pub enum ProviderCommonConfigSnippetCommands {
    /// Show the saved common config snippet for an app
    Get {
        /// App type
        #[arg(short, long, default_value = "claude")]
        app: String,
    },
    /// Save or clear the common config snippet for an app
    Set {
        /// App type
        #[arg(short, long, default_value = "claude")]
        app: String,
        /// Read snippet content from a file
        #[arg(long, conflicts_with_all = ["value", "clear"])]
        file: Option<String>,
        /// Inline snippet content
        #[arg(long, conflicts_with_all = ["file", "clear"])]
        value: Option<String>,
        /// Clear the saved snippet
        #[arg(long, conflicts_with_all = ["file", "value"])]
        clear: bool,
    },
    /// Extract a snippet from the current provider for an app
    Extract {
        /// App type
        #[arg(short, long, default_value = "claude")]
        app: String,
    },
}

#[derive(Subcommand)]
pub enum ProviderUsageScriptCommands {
    /// Show the saved usage script for a provider
    Show {
        /// Provider ID
        id: String,
        /// App type
        #[arg(short, long, default_value = "claude")]
        app: String,
    },
    /// Save or clear the usage script for a provider
    Save {
        /// Provider ID
        id: String,
        /// App type
        #[arg(short, long, default_value = "claude")]
        app: String,
        /// Read usage script JSON from a file
        #[arg(long, conflicts_with_all = ["value", "clear"])]
        file: Option<String>,
        /// Inline usage script JSON
        #[arg(long, conflicts_with_all = ["file", "clear"])]
        value: Option<String>,
        /// Clear the saved usage script
        #[arg(long, conflicts_with_all = ["file", "value"])]
        clear: bool,
    },
    /// Test a saved or temporary usage script
    Test {
        /// Provider ID
        id: String,
        /// App type
        #[arg(short, long, default_value = "claude")]
        app: String,
        /// Read temporary usage script JSON from a file
        #[arg(long, conflicts_with = "value")]
        file: Option<String>,
        /// Inline temporary usage script JSON
        #[arg(long, conflicts_with = "file")]
        value: Option<String>,
    },
    /// Query usage directly through the saved usage script
    Query {
        /// Provider ID
        id: String,
        /// App type
        #[arg(short, long, default_value = "claude")]
        app: String,
    },
}

#[derive(Subcommand)]
pub enum ProviderStreamCheckCommands {
    /// Run a stream check for a single provider
    Run {
        /// Provider ID
        id: String,
        /// App type
        #[arg(short, long, default_value = "claude")]
        app: String,
    },
    /// Run stream checks for all providers of an app
    #[command(name = "run-all")]
    RunAll {
        /// App type
        #[arg(short, long, default_value = "claude")]
        app: String,
        /// Only check the current provider and failover targets
        #[arg(long)]
        proxy_targets_only: bool,
    },
    /// Manage stream-check configuration
    Config {
        #[command(subcommand)]
        cmd: ProviderStreamCheckConfigCommands,
    },
}

#[derive(Subcommand)]
pub enum ProviderStreamCheckConfigCommands {
    /// Show the current stream-check configuration
    Get,
    /// Save stream-check configuration from JSON
    Set {
        /// Read config JSON from a file
        #[arg(long, conflicts_with = "value")]
        file: Option<String>,
        /// Inline config JSON
        #[arg(long, conflicts_with = "file")]
        value: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum UniversalProviderCommands {
    /// List universal providers
    List,
    /// Show a universal provider
    Show {
        /// Provider ID
        id: String,
    },
    /// Add a universal provider
    Add {
        /// Provider name
        #[arg(short, long)]
        name: String,
        /// Comma-separated list of apps
        #[arg(short, long)]
        apps: String,
        /// Base URL
        #[arg(short = 'u', long)]
        base_url: Option<String>,
        /// API key
        #[arg(short = 'k', long)]
        api_key: Option<String>,
    },
    /// Edit a universal provider
    Edit {
        /// Provider ID
        id: String,
        /// Set a new provider name
        #[arg(long)]
        set_name: Option<String>,
        /// Replace the enabled app list
        #[arg(long)]
        set_apps: Option<String>,
        /// Set a new base URL
        #[arg(long)]
        set_base_url: Option<String>,
        /// Set a new API key
        #[arg(long)]
        set_api_key: Option<String>,
    },
    /// Save a universal provider and immediately sync it to enabled apps
    #[command(name = "save-and-sync")]
    SaveAndSync {
        /// Provider name
        #[arg(short, long)]
        name: String,
        /// Optional explicit provider ID
        #[arg(long)]
        id: Option<String>,
        /// Comma-separated list of apps
        #[arg(short, long)]
        apps: String,
        /// Base URL
        #[arg(short = 'u', long)]
        base_url: String,
        /// API key
        #[arg(short = 'k', long)]
        api_key: String,
    },
    /// Sync universal provider to apps
    Sync {
        /// Provider ID
        id: String,
    },
    /// Delete a universal provider
    Delete {
        /// Provider ID
        id: String,
        /// Skip confirmation
        #[arg(short, long)]
        yes: bool,
    },
}

#[derive(Subcommand)]
pub enum McpCommands {
    /// List all MCP servers
    List,
    /// Show server details
    Show {
        /// Server ID
        id: String,
    },
    /// Add a new MCP server
    Add {
        /// Server ID
        #[arg(short, long)]
        id: Option<String>,
        /// Command to run
        #[arg(short, long)]
        command: Option<String>,
        /// Comma-separated arguments
        #[arg(short = 'g', long)]
        args: Option<String>,
        /// Comma-separated list of apps
        #[arg(short, long)]
        apps: Option<String>,
        /// Import from JSON file
        #[arg(long)]
        from_json: Option<String>,
    },
    /// Edit an MCP server
    Edit {
        /// Server ID
        id: String,
        /// Enable for app
        #[arg(long)]
        enable_app: Option<String>,
        /// Disable for app
        #[arg(long)]
        disable_app: Option<String>,
    },
    /// Delete an MCP server
    Delete {
        /// Server ID
        id: String,
        /// Skip confirmation
        #[arg(short, long)]
        yes: bool,
    },
    /// Enable server for an app
    Enable {
        /// Server ID
        id: String,
        /// App type
        #[arg(short, long)]
        app: String,
    },
    /// Disable server for an app
    Disable {
        /// Server ID
        id: String,
        /// App type
        #[arg(short, long)]
        app: String,
    },
    /// Import MCP servers from apps
    Import,
    /// Validate an MCP server spec already saved in SSOT
    Validate {
        /// Server ID
        id: String,
    },
    /// Show homepage / docs links for an MCP server
    #[command(name = "docs-link")]
    DocsLink {
        /// Server ID
        id: String,
    },
}

#[derive(Subcommand)]
pub enum ProxyCommands {
    /// Start proxy server
    Start {
        /// Port to listen on
        #[arg(short, long, default_value = "9527")]
        port: u16,
        /// Host to bind to
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
    },
    /// Stop proxy server
    Stop,
    /// Show proxy status
    Status,
    /// Proxy configuration
    #[command(subcommand)]
    Config(ProxyConfigCommands),
    /// Global proxy configuration
    #[command(name = "global-config", subcommand)]
    GlobalConfig(ProxyGlobalConfigCommands),
    /// Per-app proxy configuration
    #[command(name = "app-config", subcommand)]
    AppConfig(ProxyAppConfigCommands),
    /// Auto failover management
    #[command(name = "auto-failover", subcommand)]
    AutoFailover(ProxyAutoFailoverCommands),
    /// List providers that can still be added into failover queue
    #[command(name = "available-providers")]
    AvailableProviders {
        /// App type
        #[arg(short, long)]
        app: String,
    },
    /// Show provider health tracked by the proxy
    #[command(name = "provider-health")]
    ProviderHealth {
        /// Provider ID
        id: String,
        /// App type
        #[arg(short, long)]
        app: String,
    },
    /// Default cost multiplier
    #[command(name = "default-cost-multiplier", subcommand)]
    DefaultCostMultiplier(ProxyDefaultCostMultiplierCommands),
    /// Pricing model source
    #[command(name = "pricing-model-source", subcommand)]
    PricingModelSource(ProxyPricingModelSourceCommands),
    /// Takeover management
    #[command(subcommand)]
    Takeover(ProxyTakeoverCommands),
    /// Failover management
    #[command(subcommand)]
    Failover(ProxyFailoverCommands),
    /// Circuit breaker management
    #[command(subcommand)]
    Circuit(ProxyCircuitCommands),
}

#[derive(Subcommand)]
pub enum ProxyConfigCommands {
    /// Show proxy configuration
    Show,
    /// Set proxy configuration
    Set {
        /// Port to listen on
        #[arg(long)]
        port: Option<u16>,
        /// Host to bind to
        #[arg(long)]
        host: Option<String>,
        /// Enable logging
        #[arg(long)]
        log_enabled: Option<bool>,
    },
}

#[derive(Subcommand)]
pub enum ProxyGlobalConfigCommands {
    /// Show global proxy configuration
    Show,
    /// Update global proxy configuration
    Set {
        /// Toggle the global proxy switch
        #[arg(long)]
        proxy_enabled: Option<bool>,
        /// Host to bind to
        #[arg(long)]
        host: Option<String>,
        /// Port to listen on
        #[arg(long)]
        port: Option<u16>,
        /// Enable logging
        #[arg(long)]
        log_enabled: Option<bool>,
    },
}

#[derive(Subcommand)]
pub enum ProxyAppConfigCommands {
    /// Show per-app proxy configuration
    Show {
        /// App type
        #[arg(short, long)]
        app: String,
    },
    /// Update per-app proxy configuration
    Set {
        /// App type
        #[arg(short, long)]
        app: String,
        /// Whether proxy takeover is enabled for this app
        #[arg(long)]
        enabled: Option<bool>,
        /// Whether automatic failover is enabled for this app
        #[arg(long)]
        auto_failover_enabled: Option<bool>,
        /// Maximum retries
        #[arg(long)]
        max_retries: Option<u32>,
        /// Streaming first-byte timeout in seconds
        #[arg(long)]
        streaming_first_byte_timeout: Option<u32>,
        /// Streaming idle timeout in seconds
        #[arg(long)]
        streaming_idle_timeout: Option<u32>,
        /// Non-streaming timeout in seconds
        #[arg(long)]
        non_streaming_timeout: Option<u32>,
        /// Circuit breaker failure threshold
        #[arg(long)]
        circuit_failure_threshold: Option<u32>,
        /// Circuit breaker success threshold
        #[arg(long)]
        circuit_success_threshold: Option<u32>,
        /// Circuit breaker timeout in seconds
        #[arg(long)]
        circuit_timeout_seconds: Option<u32>,
        /// Circuit breaker error rate threshold
        #[arg(long)]
        circuit_error_rate_threshold: Option<f64>,
        /// Circuit breaker minimum requests
        #[arg(long)]
        circuit_min_requests: Option<u32>,
    },
}

#[derive(Subcommand)]
pub enum ProxyAutoFailoverCommands {
    /// Show whether auto failover is enabled for an app
    Show {
        /// App type
        #[arg(short, long)]
        app: String,
    },
    /// Enable auto failover for an app
    Enable {
        /// App type
        #[arg(short, long)]
        app: String,
    },
    /// Disable auto failover for an app
    Disable {
        /// App type
        #[arg(short, long)]
        app: String,
    },
}

#[derive(Subcommand)]
pub enum ProxyTakeoverCommands {
    /// Show takeover status
    Status,
    /// Enable takeover for an app
    Enable {
        /// App type
        #[arg(short, long)]
        app: String,
    },
    /// Disable takeover for an app
    Disable {
        /// App type
        #[arg(short, long)]
        app: String,
    },
}

#[derive(Subcommand)]
pub enum ProxyFailoverCommands {
    /// Show failover queue
    Queue {
        /// App type
        #[arg(short, long)]
        app: String,
    },
    /// Add provider to failover queue
    Add {
        /// Provider ID
        id: String,
        /// App type
        #[arg(short, long)]
        app: String,
        /// Priority (lower = higher priority)
        #[arg(short, long)]
        priority: Option<i32>,
    },
    /// Remove provider from failover queue
    Remove {
        /// Provider ID
        id: String,
        /// App type
        #[arg(short, long)]
        app: String,
    },
    /// Switch to a provider
    Switch {
        /// Provider ID
        id: String,
        /// App type
        #[arg(short, long)]
        app: String,
    },
}

#[derive(Subcommand)]
pub enum ProxyCircuitCommands {
    /// Show circuit breaker status
    Show {
        /// Provider ID
        id: String,
        /// App type
        #[arg(short, long)]
        app: String,
    },
    /// Reset circuit breaker
    Reset {
        /// Provider ID
        id: String,
        /// App type
        #[arg(short, long)]
        app: String,
    },
    /// Show circuit breaker runtime stats
    Stats {
        /// Provider ID
        id: String,
        /// App type
        #[arg(short, long)]
        app: String,
    },
    /// Circuit breaker configuration
    #[command(subcommand)]
    Config(ProxyCircuitConfigCommands),
}

#[derive(Subcommand)]
pub enum ProxyCircuitConfigCommands {
    /// Show circuit breaker configuration
    Show,
    /// Set circuit breaker configuration
    Set {
        /// Failure threshold
        #[arg(long)]
        failure_threshold: Option<u32>,
        /// Recovery timeout in seconds
        #[arg(long)]
        recovery_timeout: Option<u64>,
        /// Half-open requests
        #[arg(long)]
        half_open_requests: Option<u32>,
    },
}

#[derive(Subcommand)]
pub enum ProxyDefaultCostMultiplierCommands {
    /// Show default cost multiplier for an app
    Get {
        /// App type
        #[arg(short, long)]
        app: String,
    },
    /// Set default cost multiplier for an app
    Set {
        /// App type
        #[arg(short, long)]
        app: String,
        /// Multiplier value
        value: String,
    },
}

#[derive(Subcommand)]
pub enum ProxyPricingModelSourceCommands {
    /// Show pricing model source for an app
    Get {
        /// App type
        #[arg(short, long)]
        app: String,
    },
    /// Set pricing model source for an app
    Set {
        /// App type
        #[arg(short, long)]
        app: String,
        /// Source value: request or response
        value: String,
    },
}

#[derive(Subcommand)]
pub enum PromptCommands {
    /// List all prompts
    List {
        /// App type
        #[arg(short, long, default_value = "claude")]
        app: String,
    },
    /// Show prompt details
    Show {
        /// Prompt ID
        id: String,
        /// App type
        #[arg(short, long, default_value = "claude")]
        app: String,
    },
    /// Add a new prompt
    Add {
        /// App type
        #[arg(short, long, default_value = "claude")]
        app: String,
        /// Prompt ID
        #[arg(short, long)]
        id: Option<String>,
        /// File containing prompt content
        #[arg(short = 'p', long)]
        file: Option<String>,
    },
    /// Edit a prompt
    Edit {
        /// Prompt ID
        id: String,
        /// App type
        #[arg(short, long, default_value = "claude")]
        app: String,
        /// File containing prompt content
        #[arg(short = 'p', long)]
        file: Option<String>,
    },
    /// Delete a prompt
    Delete {
        /// Prompt ID
        id: String,
        /// App type
        #[arg(short, long, default_value = "claude")]
        app: String,
        /// Skip confirmation
        #[arg(short, long)]
        yes: bool,
    },
    /// Enable a prompt
    Enable {
        /// Prompt ID
        id: String,
        /// App type
        #[arg(short, long, default_value = "claude")]
        app: String,
    },
    /// Import prompts from files
    Import {
        /// App type
        #[arg(short, long, default_value = "claude")]
        app: String,
    },
    /// Show the current live prompt file content
    #[command(name = "current-live-file-content")]
    CurrentLiveFileContent {
        /// App type
        #[arg(short, long, default_value = "claude")]
        app: String,
    },
}

#[derive(Subcommand)]
pub enum DeeplinkCommands {
    /// Parse a ccswitch:// URL into a structured request
    Parse {
        /// Deeplink URL
        url: String,
    },
    /// Parse and merge inline config carried by a deeplink URL
    Merge {
        /// Deeplink URL
        url: String,
    },
    /// Show both the parsed and merged view of a deeplink URL
    Preview {
        /// Deeplink URL
        url: String,
    },
}

#[derive(Subcommand)]
pub enum SkillCommands {
    /// List installed skills
    List,
    /// Search for skills
    Search {
        /// Search keyword
        keyword: String,
    },
    /// Install a skill
    Install {
        /// Skill ID
        id: String,
        /// App type
        #[arg(short, long, default_value = "claude")]
        app: String,
    },
    /// Uninstall a skill
    Uninstall {
        /// Skill ID
        id: String,
        /// Skip confirmation
        #[arg(short, long)]
        yes: bool,
    },
    /// Enable skill for an app
    Enable {
        /// Skill ID
        id: String,
        /// App type
        #[arg(short, long)]
        app: String,
    },
    /// Disable skill for an app
    Disable {
        /// Skill ID
        id: String,
        /// App type
        #[arg(short, long)]
        app: String,
    },
    /// Manage unmanaged skills discovered from live app directories
    Unmanaged {
        #[command(subcommand)]
        cmd: SkillUnmanagedCommands,
    },
    /// Manage skill repositories
    Repo {
        #[command(subcommand)]
        cmd: SkillRepoCommands,
    },
    /// Install local skills from a ZIP archive
    #[command(name = "zip-install")]
    ZipInstall {
        /// ZIP file path
        #[arg(long)]
        file: String,
        /// App type
        #[arg(short, long, default_value = "claude")]
        app: String,
    },
}

#[derive(Subcommand)]
pub enum SkillUnmanagedCommands {
    /// Scan unmanaged skills in live app directories
    Scan,
    /// Import unmanaged skills into CC-Switch tracking
    Import {
        /// Skill directory names to import
        directories: Vec<String>,
    },
}

#[derive(Subcommand)]
pub enum SkillRepoCommands {
    /// List skill repositories
    List,
    /// Add or update a skill repository
    Add {
        /// GitHub repo URL or owner/name
        repo: String,
        /// Branch name
        #[arg(short, long, default_value = "main")]
        branch: String,
        /// Add the repo as disabled
        #[arg(long)]
        disabled: bool,
    },
    /// Remove a skill repository
    Remove {
        /// GitHub repo URL or owner/name
        repo: String,
    },
}

#[derive(Subcommand)]
pub enum ConfigCommands {
    /// Show all configuration
    Show,
    /// Get a configuration value
    Get {
        /// Configuration key
        key: String,
    },
    /// Set a configuration value
    Set {
        /// Configuration key
        key: String,
        /// Configuration value
        value: String,
    },
    /// Show configuration file paths
    Path,
}

#[derive(Subcommand)]
pub enum UsageCommands {
    /// Show usage summary
    Summary {
        /// App type
        #[arg(short, long, default_value = "claude")]
        app: String,
        /// Number of recent days to include; omit to summarize all recorded usage
        #[arg(short, long)]
        days: Option<u32>,
    },
    /// Show request logs
    Logs {
        /// App type
        #[arg(short, long, default_value = "claude")]
        app: String,
        /// From date (YYYY-MM-DD)
        #[arg(long)]
        from: Option<String>,
        /// To date (YYYY-MM-DD)
        #[arg(long)]
        to: Option<String>,
    },
    /// Export usage data
    Export {
        /// Output file path
        #[arg(short, long)]
        output: String,
        /// App type
        #[arg(short, long, default_value = "claude")]
        app: String,
    },
    /// Show daily usage trends
    Trends {
        /// From date (YYYY-MM-DD)
        #[arg(long)]
        from: Option<String>,
        /// To date (YYYY-MM-DD)
        #[arg(long)]
        to: Option<String>,
    },
    /// Show aggregated provider usage stats
    #[command(name = "provider-stats")]
    ProviderStats,
    /// Show aggregated model usage stats
    #[command(name = "model-stats")]
    ModelStats,
    /// Show one request detail
    #[command(name = "request-detail")]
    RequestDetail {
        /// Request ID
        request_id: String,
    },
    /// Manage model pricing records
    #[command(name = "model-pricing")]
    ModelPricing {
        #[command(subcommand)]
        cmd: UsageModelPricingCommands,
    },
    /// Check provider usage limits
    #[command(name = "provider-limits")]
    ProviderLimits {
        #[command(subcommand)]
        cmd: UsageProviderLimitsCommands,
    },
}

#[derive(Subcommand)]
pub enum UsageModelPricingCommands {
    /// List model pricing rows
    List,
    /// Upsert one model pricing row
    Update {
        /// Model ID
        model_id: String,
        /// Display name
        #[arg(long)]
        display_name: String,
        /// Input cost per million tokens
        #[arg(long)]
        input_cost: String,
        /// Output cost per million tokens
        #[arg(long)]
        output_cost: String,
        /// Cache read cost per million tokens
        #[arg(long)]
        cache_read_cost: String,
        /// Cache creation cost per million tokens
        #[arg(long)]
        cache_creation_cost: String,
    },
    /// Delete one model pricing row
    Delete {
        /// Model ID
        model_id: String,
    },
}

#[derive(Subcommand)]
pub enum UsageProviderLimitsCommands {
    /// Check limit status for one provider
    Check {
        /// Provider ID
        provider_id: String,
        /// App type
        #[arg(short, long, default_value = "claude")]
        app: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn cli_definition_has_no_short_flag_conflicts() {
        Cli::command().debug_assert();
    }
}
