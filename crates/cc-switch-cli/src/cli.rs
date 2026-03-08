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
    /// Generate shell completion scripts
    Completions {
        /// Target shell
        shell: CompletionShell,
    },
    /// Installation helpers
    Install {
        #[command(subcommand)]
        cmd: InstallCommands,
    },
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
    /// Structured settings management
    Settings {
        #[command(subcommand)]
        cmd: SettingsCommands,
    },
    /// System auto-launch management
    #[command(name = "auto-launch")]
    AutoLaunch {
        #[command(subcommand)]
        cmd: AutoLaunchCommands,
    },
    /// Detect whether the current binary is running in portable mode
    #[command(name = "portable-mode")]
    PortableMode,
    /// Inspect local tool versions
    #[command(name = "tool-versions")]
    ToolVersions {
        /// Restrict output to specific tools
        #[arg(long = "tool")]
        tools: Vec<String>,
        /// Also fetch latest published versions
        #[arg(long)]
        latest: bool,
        /// Override WSL shell per tool, e.g. claude=bash
        #[arg(long = "wsl-shell")]
        wsl_shell: Vec<String>,
        /// Override WSL shell flag per tool, e.g. claude=-lc
        #[arg(long = "wsl-shell-flag", allow_hyphen_values = true)]
        wsl_shell_flag: Vec<String>,
    },
    /// Show app metadata and useful links
    About,
    /// Diagnose runtime, tools and live config health
    Doctor {
        /// Restrict diagnosis to specific apps
        #[arg(long = "app")]
        apps: Vec<String>,
        /// Also fetch latest published versions for local tools
        #[arg(long)]
        latest: bool,
        /// Also query the latest published CC-Switch release metadata
        #[arg(long = "check-updates")]
        check_updates: bool,
    },
    /// Update information
    Update {
        #[command(subcommand)]
        cmd: UpdateCommands,
    },
    /// Print release notes links
    #[command(name = "release-notes")]
    ReleaseNotes {
        /// Print the latest release page instead of the current version page
        #[arg(long)]
        latest: bool,
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
    /// Database backup management
    Backup {
        #[command(subcommand)]
        cmd: BackupCommands,
    },
    /// Environment conflict management
    Env {
        #[command(subcommand)]
        cmd: EnvCommands,
    },
    /// OpenClaw-specific configuration
    Openclaw {
        #[command(subcommand)]
        cmd: OpenClawCommands,
    },
    /// Session management
    Sessions {
        #[command(subcommand)]
        cmd: SessionCommands,
    },
    /// OMO configuration management
    Omo {
        #[command(subcommand)]
        cmd: OmoCommands,
    },
    /// OMO Slim configuration management
    #[command(name = "omo-slim")]
    OmoSlim {
        #[command(subcommand)]
        cmd: OmoCommands,
    },
    /// OpenClaw workspace and daily memory files
    Workspace {
        #[command(subcommand)]
        cmd: WorkspaceCommands,
    },
    /// WebDAV sync management
    Webdav {
        #[command(subcommand)]
        cmd: WebDavCommands,
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
pub enum WorkspaceCommands {
    /// Show the workspace or memory directory path
    Path {
        /// Path target
        #[arg(value_enum, default_value = "workspace")]
        target: WorkspacePathTarget,
    },
    /// Read an OpenClaw workspace file
    Read {
        /// Workspace filename
        filename: String,
    },
    /// Write an OpenClaw workspace file
    Write {
        /// Workspace filename
        filename: String,
        /// Read content from a file
        #[arg(long, conflicts_with = "value")]
        file: Option<String>,
        /// Inline content
        #[arg(long, conflicts_with = "file")]
        value: Option<String>,
    },
    /// Daily memory file operations
    Memory {
        #[command(subcommand)]
        cmd: WorkspaceMemoryCommands,
    },
}

#[derive(Subcommand)]
pub enum WorkspaceMemoryCommands {
    /// List daily memory files
    List,
    /// Read a daily memory file
    Read {
        /// Daily memory filename in YYYY-MM-DD.md format
        filename: String,
    },
    /// Write a daily memory file
    Write {
        /// Daily memory filename in YYYY-MM-DD.md format
        filename: String,
        /// Read content from a file
        #[arg(long, conflicts_with = "value")]
        file: Option<String>,
        /// Inline content
        #[arg(long, conflicts_with = "file")]
        value: Option<String>,
    },
    /// Search daily memory files
    Search {
        /// Search query
        query: String,
    },
    /// Delete a daily memory file
    Delete {
        /// Daily memory filename in YYYY-MM-DD.md format
        filename: String,
    },
}

#[derive(Clone, Copy, Debug, clap::ValueEnum)]
pub enum WorkspacePathTarget {
    Workspace,
    Memory,
}

#[derive(Subcommand)]
pub enum WebDavCommands {
    /// Show current WebDAV sync settings
    Show,
    /// Save WebDAV sync settings
    Save {
        /// Base WebDAV URL
        #[arg(long)]
        base_url: Option<String>,
        /// WebDAV username
        #[arg(long)]
        username: Option<String>,
        /// WebDAV password
        #[arg(long, conflicts_with = "clear_password")]
        password: Option<String>,
        /// Clear the saved password
        #[arg(long, conflicts_with = "password")]
        clear_password: bool,
        /// Remote root directory
        #[arg(long)]
        remote_root: Option<String>,
        /// Sync profile name
        #[arg(long)]
        profile: Option<String>,
        /// Enable WebDAV sync
        #[arg(long, conflicts_with = "disable")]
        enable: bool,
        /// Disable WebDAV sync
        #[arg(long, conflicts_with = "enable")]
        disable: bool,
        /// Enable auto sync
        #[arg(long, conflicts_with = "no_auto_sync")]
        auto_sync: bool,
        /// Disable auto sync
        #[arg(long, conflicts_with = "auto_sync")]
        no_auto_sync: bool,
    },
    /// Test WebDAV connectivity with saved or overridden settings
    Test {
        /// Base WebDAV URL
        #[arg(long)]
        base_url: Option<String>,
        /// WebDAV username
        #[arg(long)]
        username: Option<String>,
        /// WebDAV password
        #[arg(long, conflicts_with = "clear_password")]
        password: Option<String>,
        /// Clear the password for this test request
        #[arg(long, conflicts_with = "password")]
        clear_password: bool,
        /// Remote root directory
        #[arg(long)]
        remote_root: Option<String>,
        /// Sync profile name
        #[arg(long)]
        profile: Option<String>,
    },
    /// Upload the current snapshot to WebDAV
    Upload,
    /// Download the remote snapshot from WebDAV
    Download,
    /// Fetch remote snapshot metadata
    #[command(name = "remote-info")]
    RemoteInfo,
}

#[derive(Subcommand)]
pub enum BackupCommands {
    /// Create a database backup
    Create,
    /// List database backups
    List,
    /// Restore the database from a backup
    Restore {
        /// Backup filename
        filename: String,
        /// Skip confirmation
        #[arg(short, long)]
        yes: bool,
    },
    /// Rename a backup file
    Rename {
        /// Existing backup filename
        filename: String,
        /// New backup name without extension
        new_name: String,
    },
    /// Delete a backup file
    Delete {
        /// Backup filename
        filename: String,
        /// Skip confirmation
        #[arg(short, long)]
        yes: bool,
    },
}

#[derive(Subcommand)]
pub enum EnvCommands {
    /// Check environment variable conflicts for an app
    Check {
        /// App type
        #[arg(short, long, default_value = "claude")]
        app: String,
    },
    /// Delete detected environment conflicts for an app with a backup
    Delete {
        /// App type
        #[arg(short, long, default_value = "claude")]
        app: String,
        /// Also include system-level conflicts when supported
        #[arg(long)]
        include_system: bool,
        /// Skip confirmation
        #[arg(short, long)]
        yes: bool,
    },
    /// Restore environment variables from a backup file
    Restore {
        /// Backup file path
        backup_path: String,
    },
}

#[derive(Subcommand)]
pub enum SessionCommands {
    /// List discovered sessions
    List {
        /// Filter by provider
        #[arg(long)]
        provider: Option<String>,
        /// Case-insensitive keyword filter across title, summary, project path and session id
        #[arg(long)]
        query: Option<String>,
    },
    /// Read messages from one session source file
    Messages {
        /// Provider id
        #[arg(long)]
        provider: String,
        /// Session source file path
        #[arg(long)]
        source_path: String,
    },
    /// Print the resume command for one session
    #[command(name = "resume-command")]
    ResumeCommand {
        /// Session id
        session_id: String,
        /// Filter by provider
        #[arg(long)]
        provider: Option<String>,
        /// Exact session source file path
        #[arg(long)]
        source_path: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum OpenClawCommands {
    /// Manage OpenClaw env config
    Env {
        #[command(subcommand)]
        cmd: OpenClawConfigCommands,
    },
    /// Manage OpenClaw tools config
    Tools {
        #[command(subcommand)]
        cmd: OpenClawConfigCommands,
    },
    /// Manage OpenClaw agents.defaults
    #[command(name = "agents-defaults")]
    AgentsDefaults {
        #[command(subcommand)]
        cmd: OpenClawConfigCommands,
    },
    /// Manage OpenClaw default model
    #[command(name = "default-model")]
    DefaultModel {
        #[command(subcommand)]
        cmd: OpenClawConfigCommands,
    },
    /// Manage OpenClaw model catalog
    #[command(name = "model-catalog")]
    ModelCatalog {
        #[command(subcommand)]
        cmd: OpenClawConfigCommands,
    },
}

#[derive(Subcommand)]
pub enum OpenClawConfigCommands {
    /// Read current config
    Get,
    /// Save config from JSON
    Set {
        /// Read JSON from a file
        #[arg(long, conflicts_with = "value")]
        file: Option<String>,
        /// Inline JSON content
        #[arg(long, conflicts_with = "file")]
        value: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum OmoCommands {
    /// Read the local OMO config file
    #[command(name = "read-local")]
    ReadLocal,
    /// Import the local OMO config as the current provider
    #[command(name = "import-local")]
    ImportLocal,
    /// Show the current OMO provider id
    Current,
    /// Disable the current OMO provider and remove the generated config file
    #[command(name = "disable-current")]
    DisableCurrent,
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
pub enum SettingsCommands {
    /// Show frontend-facing settings
    Show,
    /// Manage language setting
    Language {
        #[command(subcommand)]
        cmd: SettingsValueCommands,
    },
    /// Manage visible apps
    #[command(name = "visible-apps")]
    VisibleApps {
        #[command(subcommand)]
        cmd: SettingsVisibleAppsCommands,
    },
    /// Manage preferred terminal
    Terminal {
        #[command(subcommand)]
        cmd: SettingsValueCommands,
    },
    /// Manage startup-related preferences
    Startup {
        #[command(subcommand)]
        cmd: SettingsStartupCommands,
    },
    /// Manage Claude plugin integration
    Plugin {
        #[command(subcommand)]
        cmd: SettingsToggleCommands,
    },
    /// Manage Claude onboarding skip state
    Onboarding {
        #[command(subcommand)]
        cmd: SettingsOnboardingCommands,
    },
}

#[derive(Subcommand)]
pub enum SettingsValueCommands {
    /// Read the current value
    Get,
    /// Set a new value
    Set {
        /// New value
        value: String,
    },
    /// Clear the current value
    Clear,
}

#[derive(Subcommand)]
pub enum SettingsVisibleAppsCommands {
    /// Read visible app flags
    Get,
    /// Update visible app flags
    Set {
        /// Show Claude
        #[arg(long)]
        claude: Option<bool>,
        /// Show Codex
        #[arg(long)]
        codex: Option<bool>,
        /// Show Gemini
        #[arg(long)]
        gemini: Option<bool>,
        /// Show OpenCode
        #[arg(long)]
        opencode: Option<bool>,
        /// Show OpenClaw
        #[arg(long)]
        openclaw: Option<bool>,
    },
    /// Clear custom visible app flags and fall back to defaults
    Clear,
}

#[derive(Subcommand)]
pub enum SettingsStartupCommands {
    /// Read startup-related preferences
    Show,
    /// Update startup-related preferences
    Set {
        /// Show tray icon
        #[arg(long)]
        show_in_tray: Option<bool>,
        /// Minimize to tray when closing
        #[arg(long)]
        minimize_to_tray_on_close: Option<bool>,
        /// Remember launch-on-startup preference
        #[arg(long)]
        launch_on_startup: Option<bool>,
        /// Remember silent-startup preference
        #[arg(long)]
        silent_startup: Option<bool>,
    },
}

#[derive(Subcommand)]
pub enum SettingsToggleCommands {
    /// Show current status
    Status,
    /// Enable the feature
    Enable,
    /// Disable the feature
    Disable,
}

#[derive(Subcommand)]
pub enum SettingsOnboardingCommands {
    /// Show current onboarding skip status
    Status,
    /// Mark onboarding as completed
    Skip,
    /// Clear the onboarding completion marker
    Clear,
}

#[derive(Subcommand)]
pub enum AutoLaunchCommands {
    /// Show current auto-launch status
    Status,
    /// Enable auto-launch
    Enable,
    /// Disable auto-launch
    Disable,
}

#[derive(Subcommand)]
pub enum InstallCommands {
    /// Show recommended installation methods and completion setup hints
    Guide {
        /// Focus completion hints on one shell
        #[arg(long)]
        shell: Option<CompletionShell>,
    },
    /// Write a completion script to the default shell directory or a custom one
    Completions {
        /// Target shell
        shell: CompletionShell,
        /// Override the output directory
        #[arg(long)]
        dir: Option<String>,
    },
}

#[derive(Clone, Copy, Debug, clap::ValueEnum)]
pub enum CompletionShell {
    Bash,
    Zsh,
    Fish,
}

#[derive(Subcommand)]
pub enum UpdateCommands {
    /// Check the latest published version
    Check,
    /// Show recommended update steps for the current installation style
    Guide,
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
