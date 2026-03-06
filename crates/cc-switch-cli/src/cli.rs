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

#[derive(Clone, clap::ValueEnum)]
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
    /// Switch current provider
    Switch {
        /// Provider ID
        id: String,
        /// App type
        #[arg(short, long, default_value = "claude")]
        app: String,
    },
    /// Query provider usage
    Usage {
        /// Provider ID
        id: String,
        /// App type
        #[arg(short, long, default_value = "claude")]
        app: String,
    },
    /// Universal provider management (cross-app)
    #[command(subcommand)]
    Universal(UniversalProviderCommands),
}

#[derive(Subcommand)]
pub enum UniversalProviderCommands {
    /// List universal providers
    List,
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
        /// Number of days to include
        #[arg(short, long, default_value = "7")]
        days: u32,
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
