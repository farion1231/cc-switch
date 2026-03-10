//! KeyMux - OpenAI-compatible LLM proxy with intelligent routing
//!
//! Features:
//! - `/provider/model` routing (e.g., `/anthropic/claude-3-5-sonnet`)
//! - literbike QUIC (custom implementation, not quinn/h3)
//! - SSH pubkey authentication for agent sessions
//! - Intelligent ranker with pluggable trait
//! - Encrypted SQLite key vault
//! - Per-key quota management
//! - Provider spoofing without litellm dependency

mod acl_vault;
mod ranker;
mod router;
mod quic_server;
mod known_peers;
mod secret_lock;
mod carrier;
mod handlers;
mod types;
mod openapi;
mod feed;

use anyhow::Result;
use clap::Parser;
use log::{info, warn};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Port to listen on (default: 8888)
    #[arg(short, long, default_value_t = 8888)]
    port: u16,

    /// Protocol: quic, tcp, or auto (default: auto)
    #[arg(short, long, default_value = "auto")]
    proto: String,

    /// Config file path (default: ~/.keymux/muxer.json)
    #[arg(short, long)]
    config: Option<String>,

    /// SQLite database path (default: ~/.keymux/keymux.db)
    #[arg(long)]
    db_path: Option<String>,

    /// Enable verbose logging
    #[arg(short, long)]
    verbose: bool,

    /// Fail fast at startup if required readiness conditions are not met (e.g. no keys loaded)
    #[arg(long)]
    fail_fast: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Initialize logging
    let log_level = if args.verbose { "debug" } else { "info" };
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(log_level))
        .init();

    info!("🚀 KeyMux starting...");
    info!("  Port: {}", args.port);
    info!("  Protocol: {}", args.proto);
    info!("  Config: {:?}", args.config);
    info!("  Database: {:?}", args.db_path);

    // Initialize ACL key vault (filesystem + env vars)
    let base_dir = args.db_path
        .as_ref()
        .and_then(|p| PathBuf::from(p).parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| {
            dirs::home_dir()
                .map(|h| h.join(".keymux"))
                .unwrap_or_else(|| PathBuf::from("."))
        });
    
    let key_vault = Arc::new(acl_vault::AclKeyVault::open(&base_dir)?);
    info!("  ACL vault: {}", base_dir.display());
    info!("  Loaded {} keys", key_vault.list_keys().len());
    
    // Log loaded providers
    for provider in key_vault.list_providers() {
        let keys = key_vault.get_keys_for_provider(&provider);
        info!("    Provider '{}': {} key(s)", provider, keys.len());
    }

    if args.fail_fast {
        let key_count = key_vault.list_keys().len();
        if key_count == 0 {
            anyhow::bail!(
                "Fail-fast startup check failed: no provider keys loaded (filesystem ACL or env). \
Use /health or /ready after adding keys, or start without --fail-fast."
            );
        }
        info!("  Fail-fast checks: passed ({} key(s) loaded)", key_count);
    }

    // Initialize ranker
    let ranker = Arc::new(ranker::DefaultRanker::new());
    
    // Initialize carrier metrics
    carrier::init_carrier_metrics()?;
    info!("  Carrier metrics: initialized");
    
    // LiteBike integration
    let litebike_url = std::env::var("LITEBIKE_URL")
        .unwrap_or_else(|_| "http://localhost:8889/v1".to_string());
    info!("  LiteBike URL: {}", litebike_url);
    
    // Log environment variable keys (opt-in)
    let env_vars = ["ANTHROPIC_API_KEY", "OPENAI_API_KEY", "GOOGLE_API_KEY", 
                    "DEEPSEEK_API_KEY", "MOONSHOT_API_KEY", "MINIMAX_API_KEY"];
    for env_var in env_vars {
        if std::env::var(env_var).is_ok() {
            info!("  ✓ Environment variable: {}", env_var);
        }
    }

    // Start background tasks (carrier probing, etc.)
    let metrics_handle = if let Some(metrics) = carrier::get_carrier_metrics_mut() {
        let metrics = std::sync::Arc::new(tokio::sync::Mutex::new(metrics));
        Some(tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                let mut m = metrics.lock().await;
                if let Err(e) = m.probe_latencies().await {
                    warn!("Carrier probe failed: {}", e);
                }
            }
        }))
    } else {
        None
    };
    
    // Start server based on protocol
    let addr = SocketAddr::from(([0, 0, 0, 0], args.port));

    match args.proto.as_str() {
        "quic" => {
            info!("  Starting literbike QUIC server...");
            quic_server::start_quic_server(addr, key_vault, ranker, base_dir.clone(), None).await?;
        }
        "tcp" => {
            info!("  Starting TCP/HTTP server...");
            router::start_tcp_server(addr, key_vault, ranker).await?;
        }
        "auto" => {
            info!("  Starting auto protocol server (QUIC + TCP fallback)...");
            quic_server::start_auto_server(addr, key_vault, ranker, base_dir.clone(), None).await?;
        }
        _ => {
            anyhow::bail!("Unknown protocol: {}. Use 'quic', 'tcp', or 'auto'.", args.proto);
        }
    }

    Ok(())
}
